//! The default backend: spawn the user's own `claude -p` per chunk (ADR-0001).
//!
//! Call shape, verbatim from plan §4.2:
//!
//! ```text
//! MAX_THINKING_TOKENS=0 MARKDOWNATTRACTOR_WORKER=1 \
//! claude -p --model <m> --system-prompt <prompts/section.v1.txt> \
//!   --output-format json --json-schema <SectionSummary schema> \
//!   --tools "" --setting-sources "" --strict-mcp-config --no-session-persistence \
//!   --max-budget-usd <cfg>   < chunk     # cwd = empty scratch dir
//! ```
//!
//! The chunk goes over stdin, which is closed immediately (the CLI otherwise waits 3 s for
//! more input). Stdout and stderr are drained concurrently so neither pipe can fill and
//! deadlock the child. The whole call is bounded by `worker_timeout_secs`; on expiry the child
//! is killed and the call counts as [`Outcome::Retryable`].

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use super::{
    Backend, BoxFuture, Outcome, SYSTEM_PROMPT, SummarizeRequest, parse_result_with_model,
};
use crate::card::SectionSummary;
use crate::config::Config;
use crate::{Error, Result};

/// Name reported by [`Backend::name`] and stamped into provenance.
pub const BACKEND_NAME: &str = "claude-cli";

/// Longest stderr excerpt kept in a reason string.
const STDERR_TAIL: usize = 300;

static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// An empty directory the CLI runs in, so it never picks up a project's `CLAUDE.md`, hooks or
/// MCP servers. Removed on drop.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn create() -> Result<Self> {
        let n = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path =
            std::env::temp_dir().join(format!("mda-worker-{}-{n}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| Error::io(&path, e))?;
        Ok(Self(path))
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            tracing::debug!(path = %self.0.display(), error = %e, "could not remove scratch dir");
        }
    }
}

/// Backend that spawns `claude -p`. Cheap to clone the config out of; holds one scratch
/// directory for its lifetime.
pub struct ClaudeCli {
    binary: PathBuf,
    timeout: Duration,
    per_call_budget_usd: f64,
    schema: String,
    scratch: ScratchDir,
}

impl std::fmt::Debug for ClaudeCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeCli")
            .field("binary", &self.binary)
            .field("timeout", &self.timeout)
            .field("per_call_budget_usd", &self.per_call_budget_usd)
            .field("scratch", &self.scratch.0)
            .finish_non_exhaustive()
    }
}

impl ClaudeCli {
    /// Build from config. Creates the scratch directory now, so a read-only temp dir fails
    /// here rather than on the first call.
    pub fn new(cfg: &Config) -> Result<Self> {
        let schema = serde_json::to_string(&SectionSummary::json_schema())?;
        Ok(Self {
            binary: PathBuf::from("claude"),
            timeout: Duration::from_secs(cfg.worker_timeout_secs),
            per_call_budget_usd: cfg.per_call_budget_usd,
            schema,
            scratch: ScratchDir::create()?,
        })
    }

    /// Use a specific executable instead of `claude` from `PATH`. For tests and `mda doctor`.
    #[must_use]
    pub fn with_binary(mut self, path: PathBuf) -> Self {
        self.binary = path;
        self
    }

    /// Override the per-call wall-clock timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The scratch directory the CLI runs in.
    pub fn scratch_dir(&self) -> &std::path::Path {
        &self.scratch.0
    }

    /// The exact argv that would be spawned for `model`, executable first. For `mda doctor`
    /// and debug output; the same list drives [`Backend::summarize`].
    pub fn command_line(&self, model: &str) -> Vec<String> {
        vec![
            self.binary.to_string_lossy().into_owned(),
            "-p".into(),
            "--model".into(),
            model.into(),
            "--system-prompt".into(),
            SYSTEM_PROMPT.into(),
            "--output-format".into(),
            "json".into(),
            "--json-schema".into(),
            self.schema.clone(),
            "--tools".into(),
            String::new(),
            "--setting-sources".into(),
            String::new(),
            "--strict-mcp-config".into(),
            "--no-session-persistence".into(),
            "--max-budget-usd".into(),
            format!("{}", self.per_call_budget_usd),
        ]
    }

    fn build_command(&self, model: &str) -> Command {
        let argv = self.command_line(model);
        let mut cmd = Command::new(&self.binary);
        cmd.args(&argv[1..])
            .env("MAX_THINKING_TOKENS", "0")
            .env("MARKDOWNATTRACTOR_WORKER", "1")
            .current_dir(&self.scratch.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // A worker spawned from inside a Claude Code session must not look like a nested one.
        for (key, _) in std::env::vars_os() {
            let k = key.to_string_lossy();
            if k == "CLAUDECODE" || k.starts_with("CLAUDE_CODE_") {
                cmd.env_remove(&key);
            }
        }
        cmd
    }

    async fn run_once(&self, req: &SummarizeRequest, model: &str) -> Result<Outcome> {
        if req.text.trim().is_empty() {
            return Ok(Outcome::Fatal { reason: "empty input: chunk has no content".into() });
        }
        let started = Instant::now();
        let mut child = self
            .build_command(model)
            .spawn()
            .map_err(|e| Error::Worker(format!("cannot spawn {}: {e}", self.binary.display())))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let text = super::user_message(req);

        let work = async {
            // Write the chunk and close stdin straight away; a write error only means the
            // child exited early, which the exit status will explain.
            if let Some(mut stdin) = stdin {
                if let Err(e) = stdin.write_all(text.as_bytes()).await {
                    tracing::debug!(error = %e, "stdin write failed");
                }
                drop(stdin);
            }
            let (out, err, status) = tokio::join!(read_all(stdout), read_all(stderr), child.wait());
            (out, err, status)
        };

        let finished = tokio::time::timeout(self.timeout, work).await;
        let wall_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let Ok((stdout, stderr, status)) = finished else {
            if let Err(e) = child.kill().await {
                tracing::debug!(error = %e, "kill after timeout failed");
            }
            tracing::warn!(id = %req.id, timeout_s = self.timeout.as_secs(), "worker timed out");
            return Ok(Outcome::Retryable {
                reason: format!("timeout after {} s", self.timeout.as_secs()),
            });
        };
        let status = status.map_err(|e| Error::Worker(format!("wait failed: {e}")))?;
        tracing::debug!(id = %req.id, %status, wall_ms, "claude -p exited");

        if let Some(json) = json_document(&stdout) {
            return Ok(parse_result_with_model(json, wall_ms, model));
        }
        if status.success() {
            return Ok(Outcome::Malformed {
                reason: "exit 0 but stdout is not a result document".into(),
                raw: stdout,
                usage: None,
            });
        }
        let tail = tail(&stderr);
        let lower = tail.to_ascii_lowercase();
        if lower.contains("input must be provided") {
            return Ok(Outcome::Fatal { reason: format!("empty input: {tail}") });
        }
        if lower.contains("not logged in") || lower.contains("authentication") {
            return Ok(Outcome::Fatal {
                reason: format!("{} : {tail}", super::FATAL_NOT_LOGGED_IN),
            });
        }
        Ok(Outcome::Retryable { reason: format!("claude exited with {status}: {tail}") })
    }
}

impl Backend for ClaudeCli {
    fn summarize<'a>(
        &'a self,
        req: &'a SummarizeRequest,
        model: &'a str,
    ) -> BoxFuture<'a, Result<Outcome>> {
        Box::pin(self.run_once(req, model))
    }

    fn name(&self) -> &'static str {
        BACKEND_NAME
    }
}

async fn read_all<R: tokio::io::AsyncRead + Unpin>(reader: Option<R>) -> String {
    let mut buf = Vec::new();
    if let Some(mut r) = reader
        && let Err(e) = r.read_to_end(&mut buf).await
    {
        tracing::debug!(error = %e, "pipe read failed");
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Find the result document in stdout: the whole thing, or the outermost `{ … }` if the CLI
/// printed anything around it.
fn json_document(stdout: &str) -> Option<&str> {
    let t = stdout.trim();
    if t.starts_with('{') && t.ends_with('}') {
        return Some(t);
    }
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    (end > start).then(|| &t[start..=end])
}

fn tail(s: &str) -> String {
    let t = s.trim();
    let n = t.chars().count();
    if n <= STDERR_TAIL { t.to_owned() } else { t.chars().skip(n - STDERR_TAIL).collect() }
}
