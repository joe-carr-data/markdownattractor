//! Reading times from git history (ADR-0008).
//!
//! Everything here is a read-only question to the `git` binary about one pinned commit. The
//! caller ([`crate::pipeline::Engine::date_from_git`]) decides what the answers mean and
//! proves, before trusting them, that the bytes git describes are the bytes the index holds.
//!
//! Rules every call follows:
//!
//! - no shell; arguments are passed as a vector, path arguments after `--` with
//!   `--literal-pathspecs`, listings with `-z`;
//! - the environment variables that would point git at another repository are removed, and
//!   optional locks are off, so a call never writes into the user's repository;
//! - stdin is written and stdout and stderr are drained on their own threads while the parent
//!   waits under a timeout; the child is killed on a timeout or when its output passes a cap.
//!
//! A *git time* is a commit's **authored** time: its author time, capped at its committer time
//! (a commit cannot have been authored after it was committed) and at the clock when it is
//! read (no dates in the future).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use jiff::Timestamp;

use crate::{Error, Result};

/// How long one git call may run.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// How much output one git call may produce before it is killed.
pub const OUTPUT_CAP: usize = 64 * 1024 * 1024;

/// What [`discover`] found at a root.
#[derive(Debug)]
pub enum Discovery {
    /// There is no `git` binary to run.
    NoGit,
    /// The root is not inside a git work tree (or the repository has no commit yet).
    NotARepository,
    /// The repository is a shallow clone: its history is cut, so its dates would be wrong.
    Shallow,
    /// A full work tree.
    Usable(Repo),
}

impl Discovery {
    /// A short name for reports: `no_git`, `not_a_repository`, `shallow`, `usable`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::NoGit => "no_git",
            Self::NotARepository => "not_a_repository",
            Self::Shallow => "shallow",
            Self::Usable(_) => "usable",
        }
    }
}

/// A git work tree that contains the watched root.
#[derive(Debug, Clone)]
pub struct Repo {
    /// The work tree's top level, as git prints it.
    top: PathBuf,
    /// The root's path below the top level: empty, or ending in `/`.
    prefix: String,
}

/// The output of a finished git call.
struct Output {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: String,
}

/// The base command: no optional locks, no prompts, a fixed locale, and none of the
/// environment variables that redirect git to another repository or index.
fn base(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(["--no-optional-locks", "--literal-pathspecs", "-c", "core.quotepath=off"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Read a pipe to its end, keeping at most `cap` bytes. Returns the bytes and whether the
/// cap was passed (the rest is read and dropped so the child never blocks on a full pipe).
fn drain(mut pipe: impl Read, cap: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut over = false;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(out.len());
                if n > room {
                    over = true;
                }
                out.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
    (out, over)
}

/// Run one git call to completion under [`CALL_TIMEOUT`] and [`OUTPUT_CAP`].
///
/// `Ok(None)` means the binary could not be started because it does not exist.
fn run(mut cmd: Command, stdin: Option<Vec<u8>>, what: &str) -> Result<Option<Output>> {
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Git(format!("cannot start git {what}: {e}"))),
    };
    let child_in = child.stdin.take();
    let child_out = child.stdout.take();
    let child_err = child.stderr.take();

    // The three pipes are served concurrently with the wait: a child that fills stdout before
    // it reads stdin would otherwise deadlock with us.
    let feeder = std::thread::spawn(move || {
        if let (Some(mut pipe), Some(bytes)) = (child_in, stdin) {
            // A write error only means the child exited early; its status explains why.
            let _ = pipe.write_all(&bytes);
        }
    });
    let out_reader =
        std::thread::spawn(move || child_out.map_or((Vec::new(), false), |p| drain(p, OUTPUT_CAP)));
    let err_reader =
        std::thread::spawn(move || child_err.map_or((Vec::new(), false), |p| drain(p, 64 * 1024)));

    let deadline = Instant::now() + CALL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => break None,
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Git(format!("waiting for git {what}: {e}")));
            }
        }
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = feeder.join();
    let (stdout, over) = out_reader.join().unwrap_or_default();
    let (stderr, _) = err_reader.join().unwrap_or_default();
    let Some(status) = status else {
        return Err(Error::Git(format!("git {what} timed out after {} s", CALL_TIMEOUT.as_secs())));
    };
    if over {
        return Err(Error::Git(format!("git {what} produced more than {OUTPUT_CAP} bytes")));
    }
    Ok(Some(Output {
        success: status.success(),
        code: status.code(),
        stdout,
        stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
    }))
}

/// Run a call that must succeed, and return its stdout.
fn output(cmd: Command, stdin: Option<Vec<u8>>, what: &str) -> Result<Vec<u8>> {
    let out =
        run(cmd, stdin, what)?.ok_or_else(|| Error::Git(format!("git {what}: no git binary")))?;
    if out.success {
        Ok(out.stdout)
    } else {
        Err(Error::Git(format!("git {what} failed ({:?}): {}", out.code, out.stderr)))
    }
}

/// Look for a usable git work tree around `root`.
///
/// Never an error: anything that prevents dating is reported as the reason it cannot happen.
#[must_use]
pub fn discover(root: &Path) -> Discovery {
    let mut cmd = base(root);
    cmd.args([
        "rev-parse",
        "--is-inside-work-tree",
        "--is-shallow-repository",
        "--show-toplevel",
        "--show-prefix",
    ]);
    let out = match run(cmd, None, "rev-parse") {
        Ok(None) => return Discovery::NoGit,
        Ok(Some(out)) if out.success => out,
        Ok(Some(_)) | Err(_) => return Discovery::NotARepository,
    };
    let Ok(text) = String::from_utf8(out.stdout) else { return Discovery::NotARepository };
    let mut lines = text.split('\n');
    let (inside, shallow, top) = (lines.next(), lines.next(), lines.next());
    let prefix = lines.next().unwrap_or_default();
    if inside != Some("true") {
        return Discovery::NotARepository;
    }
    if shallow != Some("false") {
        return Discovery::Shallow;
    }
    let Some(top) = top.filter(|t| !t.is_empty()) else { return Discovery::NotARepository };
    let repo = Repo { top: PathBuf::from(top), prefix: prefix.to_owned() };
    // A repository without a commit has nothing to date from.
    if repo.head().is_err() {
        return Discovery::NotARepository;
    }
    Discovery::Usable(repo)
}

/// A commit's authored time: author time capped at committer time and at `now`.
fn authored(author_s: i64, committer_s: i64, now: Timestamp) -> Option<Timestamp> {
    let t = Timestamp::from_second(author_s.min(committer_s)).ok()?;
    Some(t.min(now))
}

impl Repo {
    fn git(&self) -> Command {
        base(&self.top)
    }

    /// The repository path of a root-relative path.
    fn full(&self, rel_path: &str) -> String {
        format!("{}{rel_path}", self.prefix)
    }

    /// The root-relative form of a repository path, or `None` when it is outside the root.
    fn rel<'a>(&self, full: &'a str) -> Option<&'a str> {
        full.strip_prefix(self.prefix.as_str()).filter(|r| !r.is_empty())
    }

    /// The commit `HEAD` names, as a full object id.
    pub fn head(&self) -> Result<String> {
        let mut cmd = self.git();
        cmd.args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
        let out = output(cmd, None, "rev-parse HEAD")?;
        let id = String::from_utf8_lossy(&out).trim().to_owned();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Git(format!("unexpected HEAD {id:?}")));
        }
        Ok(id)
    }

    /// Is `old` an ancestor of `new`? `false` also when `old` no longer exists.
    pub fn is_ancestor(&self, old: &str, new: &str) -> Result<bool> {
        let mut cmd = self.git();
        cmd.args(["merge-base", "--is-ancestor", old, new]);
        let out = run(cmd, None, "merge-base")?
            .ok_or_else(|| Error::Git("git merge-base: no git binary".to_owned()))?;
        Ok(out.success)
    }

    /// Every root-relative path named by a commit in `old..new`, merges included (each merge
    /// is compared with every parent, so a change that only a merge resolution made is
    /// listed). Paths outside the root are dropped. Duplicates are removed.
    pub fn changed_paths(&self, old: &str, new: &str) -> Result<Vec<String>> {
        let mut cmd = self.git();
        cmd.args(["log", "--format=", "--name-only", "-z", "--diff-merges=separate"])
            .arg(format!("{old}..{new}"))
            .arg("--")
            .arg(if self.prefix.is_empty() { "." } else { self.prefix.as_str() });
        let out = output(cmd, None, "log --name-only")?;
        let mut paths: Vec<String> = out
            .split(|b| *b == 0)
            .filter_map(|raw| {
                // With an empty format, commit boundaries leave stray newlines before a name.
                let name = std::str::from_utf8(raw).ok()?.trim_start_matches('\n');
                self.rel(name).map(str::to_owned)
            })
            .collect();
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }

    /// The regular files of `commit` under the root: root-relative path to blob id. Symlinks
    /// and submodules are left out, so they are never dated.
    pub fn tree(&self, commit: &str) -> Result<HashMap<String, String>> {
        let mut cmd = self.git();
        cmd.args(["ls-tree", "-r", "-z", "--full-name", commit]);
        if !self.prefix.is_empty() {
            cmd.arg("--").arg(self.prefix.as_str());
        }
        let out = output(cmd, None, "ls-tree")?;
        let mut tree = HashMap::new();
        for entry in out.split(|b| *b == 0).filter(|e| !e.is_empty()) {
            // `<mode> SP <type> SP <object> TAB <path>`
            let Some(tab) = entry.iter().position(|b| *b == b'\t') else { continue };
            let (meta, path) = (&entry[..tab], &entry[tab + 1..]);
            let (Ok(meta), Ok(path)) = (std::str::from_utf8(meta), std::str::from_utf8(path))
            else {
                continue;
            };
            let mut fields = meta.split(' ');
            let (mode, kind, object) = (fields.next(), fields.next(), fields.next());
            let regular = matches!(mode, Some("100644" | "100755"));
            if !regular || kind != Some("blob") {
                continue;
            }
            if let (Some(object), Some(rel)) = (object, self.rel(path)) {
                tree.insert(rel.to_owned(), object.to_owned());
            }
        }
        Ok(tree)
    }

    /// The blob id git would give exactly these bytes, with no filter or conversion.
    pub fn blob_of(&self, bytes: &[u8]) -> Result<String> {
        let mut cmd = self.git();
        cmd.args(["hash-object", "--no-filters", "--stdin"]);
        let out = output(cmd, Some(bytes.to_vec()), "hash-object")?;
        Ok(String::from_utf8_lossy(&out).trim().to_owned())
    }

    /// When the file at `rel_path` was first and last authored, over the history of `commit`
    /// that `git log --follow` reports for it (renames followed; an earlier file of the same
    /// name, or a copy source git follows, counts). `None` when no commit touched it.
    pub fn document_times(
        &self,
        commit: &str,
        rel_path: &str,
        now: Timestamp,
    ) -> Result<Option<(Timestamp, Timestamp)>> {
        let mut cmd = self.git();
        cmd.args(["log", "--follow", "--format=%at %ct", commit, "--"]).arg(self.full(rel_path));
        let out = output(cmd, None, "log --follow")?;
        let mut span: Option<(Timestamp, Timestamp)> = None;
        for line in String::from_utf8_lossy(&out).lines() {
            let mut fields = line.split(' ');
            let (Some(at), Some(ct)) = (
                fields.next().and_then(|f| f.parse::<i64>().ok()),
                fields.next().and_then(|f| f.parse::<i64>().ok()),
            ) else {
                continue;
            };
            let Some(t) = authored(at, ct, now) else { continue };
            span = Some(span.map_or((t, t), |(lo, hi)| (lo.min(t), hi.max(t))));
        }
        Ok(span)
    }

    /// The authored time of the commit that last touched each line of the file at
    /// `rel_path` in `commit`: one entry per line, in file order (plain blame, no move or
    /// copy detection).
    pub fn line_times(
        &self,
        commit: &str,
        rel_path: &str,
        now: Timestamp,
    ) -> Result<Vec<Timestamp>> {
        let mut cmd = self.git();
        cmd.args(["blame", "--line-porcelain", commit, "--"]).arg(self.full(rel_path));
        let out = output(cmd, None, "blame")?;
        parse_line_porcelain(&out, now)
    }
}

/// Parse `git blame --line-porcelain`: for every line a header `<id> <orig> <final> [<n>]`,
/// `key value` lines including `author-time` and `committer-time`, then the line itself
/// prefixed with a tab. Lines must arrive as final line 1, 2, 3, …; anything else is an error.
fn parse_line_porcelain(out: &[u8], now: Timestamp) -> Result<Vec<Timestamp>> {
    let bad = |why: &str| Error::Git(format!("unexpected blame output: {why}"));
    let mut times = Vec::new();
    let (mut author, mut committer, mut final_line): (Option<i64>, Option<i64>, Option<usize>) =
        (None, None, None);
    for line in out.split(|b| *b == b'\n') {
        if line.first() == Some(&b'\t') {
            let (Some(at), Some(ct), Some(n)) = (author, committer, final_line) else {
                return Err(bad("a line without its header"));
            };
            if n != times.len() + 1 {
                return Err(bad("lines out of order"));
            }
            times.push(authored(at, ct, now).ok_or_else(|| bad("a time out of range"))?);
            (author, committer, final_line) = (None, None, None);
            continue;
        }
        let Ok(text) = std::str::from_utf8(line) else { continue };
        if let Some(v) = text.strip_prefix("author-time ") {
            author = v.trim().parse().ok();
        } else if let Some(v) = text.strip_prefix("committer-time ") {
            committer = v.trim().parse().ok();
        } else if final_line.is_none() {
            // The header: an object id, the original line, the final line.
            let mut fields = text.split(' ');
            let id = fields.next().unwrap_or_default();
            if id.len() >= 40 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
                final_line = fields.nth(1).and_then(|f| f.parse().ok());
            }
        }
    }
    Ok(times)
}

/// How many lines git sees in `bytes`: one per newline, plus a last line without one.
#[must_use]
pub fn line_count(bytes: &[u8]) -> usize {
    let newlines = bytes.split(|b| *b == b'\n').count() - 1;
    newlines + usize::from(bytes.last().is_some_and(|b| *b != b'\n'))
}

#[cfg(test)]
mod tests;
