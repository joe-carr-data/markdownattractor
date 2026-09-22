//! `config.toml` — the only configuration file.
//!
//! Lives at `<root>/.markdownattractor/config.toml`. Every field has a default, so an empty
//! file (or no file) is a valid configuration. Field names are the same words the
//! `/mda <setting>` commands use, so the two never drift.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Name of the state directory created under the watched root.
pub const STATE_DIR: &str = ".markdownattractor";

/// Name of the config file inside [`STATE_DIR`].
pub const CONFIG_FILE: &str = "config.toml";

/// `<root>/.markdownattractor`, refusing a symlink in its place. A checked-out repository can
/// carry a planted `.markdownattractor` link; following it would put the database, the pid
/// file and the socket wherever the link points.
pub fn state_dir(root: &Path) -> Result<PathBuf> {
    let dir = root.join(STATE_DIR);
    match std::fs::symlink_metadata(&dir) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Error::Config(format!(
            "{} is a symlink; refusing to use it as the state directory",
            dir.display()
        ))),
        Ok(meta) if !meta.is_dir() => {
            Err(Error::Config(format!("{} exists and is not a directory", dir.display())))
        }
        _ => Ok(dir),
    }
}

/// Write a file under the state directory, refusing to follow a symlink at `path` and
/// keeping the file private to the user on Unix.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(Error::Config(format!(
            "{} is a symlink; refusing to write through it",
            path.display()
        )));
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).map_err(|e| Error::io(path, e))?;
    f.write_all(bytes).map_err(|e| Error::io(path, e))
}

/// Shown whenever `backend = "claude-cli"` is selected without the acknowledgement.
pub const CLAUDE_CLI_POLICY: &str = "backend \"claude-cli\" routes requests through your Claude \
subscription. Anthropic's terms do not permit third-party tools to do that on your behalf \
(see docs/adr/0002-backends-and-login-policy.md). Use the default `api` backend with an \
API key, or `local` with a llama.cpp server. To run claude-cli anyway for personal use, set \
`claude_cli_policy_ack = true` in .markdownattractor/config.toml.";

/// Which process produces summaries. See ADR-0002 for why the API is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// The Claude Messages API with the user's own API key (`api_key_env`). Default.
    #[default]
    Api,
    /// An OpenAI-compatible local server (llama.cpp, LM Studio, Ollama) at `local_base_url`.
    /// No key, no cost, no policy question; slower per call.
    Local,
    /// Spawn the user's own `claude -p`. Anthropic's terms do not permit third-party tools to
    /// route requests through Pro/Max plan credentials, so this is opt-in only and requires
    /// `claude_cli_policy_ack = true`.
    ClaudeCli,
}

impl Backend {
    /// Stable name used in provenance and `--json` output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Local => "local",
            Self::ClaudeCli => "claude-cli",
        }
    }
}

/// Which embedding model produces card vectors (ADR-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Embeddings {
    /// `bge-small-en-v1.5` quantised through fastembed: 384 dimensions, ~33 MB, CPU. Default.
    #[default]
    LocalSmall,
    /// No vectors: search is lexical only and no model is ever downloaded.
    Off,
}

impl Embeddings {
    /// Stable name used in `--json` output and `mda embeddings`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalSmall => "local-small",
            Self::Off => "off",
        }
    }
}

/// Top-level configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Model used for section and document cards on the `api` and `claude-cli` backends.
    pub summarization_model: String,
    /// Model tried once after the summarization model has failed twice on a section.
    /// Defaults to Sonnet; `None` disables escalation. Ignored by the `local` backend.
    pub escalation_model: Option<String>,
    /// Which process produces summaries.
    pub backend: Backend,
    /// Base URL of the Messages API (`api` backend).
    pub api_base_url: String,
    /// Environment variable holding the API key (`api` backend). Never stored in this file.
    pub api_key_env: String,
    /// Workspace id sent as `anthropic-workspace-id` (`api` backend). Required by keys that are
    /// not scoped to a workspace; `None` falls back to `$ANTHROPIC_WORKSPACE_ID`, then to no header.
    pub api_workspace_id: Option<String>,
    /// Base URL of the OpenAI-compatible server (`local` backend), including `/v1`.
    pub local_base_url: String,
    /// Model name as the local server reports it (`local` backend).
    pub local_model: String,
    /// `reasoning_effort` passed to the local chat template (gpt-oss); `None` sends nothing.
    pub local_reasoning_effort: Option<String>,
    /// Acknowledge Anthropic's third-party login policy before `claude-cli` is allowed.
    pub claude_cli_policy_ack: bool,
    /// Worker pool size. `None` means adaptive (AIMD, starting at 4).
    pub concurrency: Option<u16>,
    /// Daily token cap across all workers. `None` means unlimited.
    pub daily_token_budget: Option<u64>,
    /// Per-call spend cap handed to `--max-budget-usd`.
    pub per_call_budget_usd: f64,
    /// Wall-clock timeout for one worker call, in seconds.
    pub worker_timeout_secs: u64,
    /// Glob patterns (gitignore syntax) to skip, in addition to `.gitignore` and
    /// `.markdownattractorignore`.
    pub ignore: Vec<String>,
    /// Days to keep tombstones for deleted documents. `None` means forever.
    pub retention_days: Option<u32>,
    /// Inject a one-line reminder when Claude is about to `Grep`/`Read` a markdown file.
    pub nudge: bool,
    /// Which embedding model to use for the vector list, or `off`.
    pub embeddings: Embeddings,
    /// Where embedding models are cached. `None` means `$MDA_MODEL_DIR`, then
    /// `~/.cache/markdownattractor/models`.
    pub embedding_cache_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            summarization_model: "claude-haiku-4-5".to_owned(),
            escalation_model: Some("claude-sonnet-5".to_owned()),
            backend: Backend::default(),
            api_base_url: "https://api.anthropic.com".to_owned(),
            api_key_env: "ANTHROPIC_API_KEY".to_owned(),
            api_workspace_id: None,
            local_base_url: "http://127.0.0.1:8080/v1".to_owned(),
            local_model: "gpt-oss-20b".to_owned(),
            local_reasoning_effort: Some("low".to_owned()),
            claude_cli_policy_ack: false,
            concurrency: None,
            daily_token_budget: None,
            per_call_budget_usd: 0.05,
            worker_timeout_secs: 90,
            ignore: vec![
                "node_modules/".to_owned(),
                "target/".to_owned(),
                ".git/".to_owned(),
                "vendor/".to_owned(),
                "*.min.md".to_owned(),
            ],
            retention_days: None,
            nudge: true,
            embeddings: Embeddings::default(),
            embedding_cache_dir: None,
        }
    }
}

/// Local servers hold a handful of requests at once; more than this only queues.
pub const LOCAL_MAX_CONCURRENCY: u16 = 4;
/// Local servers start conservatively; AIMD may grow to [`LOCAL_MAX_CONCURRENCY`].
pub const LOCAL_INITIAL_CONCURRENCY: u16 = 2;
/// A local model can legitimately take minutes on a long section under load.
pub const LOCAL_MIN_TIMEOUT_SECS: u64 = 300;

impl Config {
    /// `(initial, max)` worker concurrency for the selected backend. The local backend is
    /// capped at [`LOCAL_MAX_CONCURRENCY`] whatever `concurrency` says: a single model on one
    /// GPU gains nothing from a deeper queue.
    #[must_use]
    pub fn pool_bounds(&self) -> (u16, u16) {
        match self.backend {
            Backend::Local => {
                let max = self
                    .concurrency
                    .unwrap_or(LOCAL_MAX_CONCURRENCY)
                    .clamp(1, LOCAL_MAX_CONCURRENCY);
                let initial = self.concurrency.unwrap_or(LOCAL_INITIAL_CONCURRENCY).clamp(1, max);
                (initial, max)
            }
            Backend::Api | Backend::ClaudeCli => {
                let initial = self.concurrency.unwrap_or(4).max(1);
                let max = self.concurrency.unwrap_or(16).max(1);
                (initial, max)
            }
        }
    }

    /// Per-call wall-clock timeout for the selected backend. The local backend never goes
    /// below [`LOCAL_MIN_TIMEOUT_SECS`].
    #[must_use]
    pub fn effective_worker_timeout(&self) -> std::time::Duration {
        let secs = match self.backend {
            Backend::Local => self.worker_timeout_secs.max(LOCAL_MIN_TIMEOUT_SECS),
            Backend::Api | Backend::ClaudeCli => self.worker_timeout_secs,
        };
        std::time::Duration::from_secs(secs)
    }

    /// The workspace id to send, from the config or `$ANTHROPIC_WORKSPACE_ID`.
    #[must_use]
    pub fn api_workspace_id(&self) -> Option<String> {
        self.api_workspace_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                std::env::var("ANTHROPIC_WORKSPACE_ID").ok().filter(|s| !s.trim().is_empty())
            })
    }

    /// Path of the config file for a given watched root.
    pub fn path_for(root: &Path) -> PathBuf {
        root.join(STATE_DIR).join(CONFIG_FILE)
    }

    /// Load the config for `root`, returning defaults if the file does not exist.
    pub fn load(root: &Path) -> Result<Self> {
        let path = state_dir(root)?.join(CONFIG_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Parse from TOML text.
    pub fn from_toml(text: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(text).map_err(|e| Error::Config(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Serialise to TOML text (stable field order, comments not preserved).
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))
    }

    /// Write the config to its path under `root`, creating the state directory if needed.
    pub fn save(&self, root: &Path) -> Result<()> {
        let dir = state_dir(root)?;
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        write_private(&dir.join(CONFIG_FILE), self.to_toml()?.as_bytes())
    }

    fn validate(&self) -> Result<()> {
        if self.summarization_model.trim().is_empty() {
            return Err(Error::Config("summarization_model must not be empty".into()));
        }
        if self.concurrency == Some(0) {
            return Err(Error::Config("concurrency must be at least 1 (or unset for auto)".into()));
        }
        if self.per_call_budget_usd.is_nan() || self.per_call_budget_usd <= 0.0 {
            return Err(Error::Config("per_call_budget_usd must be positive".into()));
        }
        if self.worker_timeout_secs == 0 {
            return Err(Error::Config("worker_timeout_secs must be positive".into()));
        }
        if self.backend == Backend::ClaudeCli && !self.claude_cli_policy_ack {
            return Err(Error::Config(CLAUDE_CLI_POLICY.to_owned()));
        }
        if self.api_base_url.trim().is_empty() || self.local_base_url.trim().is_empty() {
            return Err(Error::Config("api_base_url and local_base_url must not be empty".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_is_defaults() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
    }

    #[test]
    fn round_trips() {
        let cfg = Config { concurrency: Some(8), nudge: false, ..Config::default() };
        let text = cfg.to_toml().unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), cfg);
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = Config::from_toml("modle = \"haiku\"").unwrap_err();
        assert!(matches!(err, Error::Config(_)), "{err}");
    }

    #[test]
    fn claude_cli_requires_acknowledgement() {
        let err = Config::from_toml("backend = \"claude-cli\"").unwrap_err();
        assert!(err.to_string().contains("claude_cli_policy_ack"), "{err}");
        assert!(
            Config::from_toml("backend = \"claude-cli\"\nclaude_cli_policy_ack = true").is_ok()
        );
        assert_eq!(Config::from_toml("backend = \"local\"").unwrap().backend, Backend::Local);
        assert_eq!(Config::default().backend, Backend::Api);
    }

    #[test]
    fn local_backend_bounds_and_timeout() {
        let local = Config { backend: Backend::Local, ..Config::default() };
        assert_eq!(local.pool_bounds(), (2, 4));
        assert_eq!(local.effective_worker_timeout().as_secs(), 300);
        let capped = Config { backend: Backend::Local, concurrency: Some(16), ..Config::default() };
        assert_eq!(capped.pool_bounds(), (4, 4));
        let one = Config { backend: Backend::Local, concurrency: Some(1), ..Config::default() };
        assert_eq!(one.pool_bounds(), (1, 1));
        let api = Config::default();
        assert_eq!(api.pool_bounds(), (4, 16));
        assert_eq!(api.effective_worker_timeout().as_secs(), 90);
    }

    #[test]
    fn rejects_zero_concurrency() {
        assert!(Config::from_toml("concurrency = 0").is_err());
    }

    #[test]
    fn load_missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(dir.path()).unwrap(), Config::default());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_state_dir_and_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join(STATE_DIR)).unwrap();
        assert!(matches!(state_dir(dir.path()), Err(Error::Config(_))));
        assert!(Config::load(dir.path()).is_err());
        assert!(Config::default().save(dir.path()).is_err());

        let target = elsewhere.path().join("victim");
        std::fs::write(&target, "keep me").unwrap();
        let link = elsewhere.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(write_private(&link, b"clobbered").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me");
    }

    #[test]
    fn save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config { summarization_model: "sonnet".into(), ..Config::default() };
        cfg.save(dir.path()).unwrap();
        assert_eq!(Config::load(dir.path()).unwrap(), cfg);
        assert!(dir.path().join(STATE_DIR).join(CONFIG_FILE).exists());
    }
}
