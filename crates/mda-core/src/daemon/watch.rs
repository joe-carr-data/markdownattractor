//! Filesystem watcher and debouncer.
//!
//! [`Watcher`] wraps `notify` and turns every event path into a [`Hint`]. The daemon never
//! trusts the event *kind*: a hint means "look at this path", and the intake decides what to
//! do by looking at the filesystem. [`Debouncer`] holds hints back until a path has been quiet
//! for a while and its size has stopped changing, so a file that an editor writes in several
//! steps is parsed once, complete.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher as _};
use tokio::sync::mpsc;

use crate::config::STATE_DIR;
use crate::{Error, Result};

/// "Something happened at this path."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// The path the OS reported, absolute.
    pub path: PathBuf,
    /// When the hint arrived.
    pub at: Instant,
    /// The OS lost events and the whole tree should be rescanned.
    pub rescan: bool,
}

/// A live `notify` watcher on one root. Dropping it stops the watch.
pub struct Watcher {
    _inner: notify::RecommendedWatcher,
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Watcher")
    }
}

impl Watcher {
    /// Watch `root` recursively. Hints arrive on the returned channel; watch errors are logged
    /// and turned into rescan hints so nothing is silently lost.
    pub fn start(root: &Path) -> Result<(Self, mpsc::UnboundedReceiver<Hint>)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let root_owned = root.to_path_buf();
        let handler = move |res: notify::Result<notify::Event>| match res {
            Ok(ev) => {
                let rescan = ev.need_rescan();
                let at = Instant::now();
                if ev.paths.is_empty() || rescan {
                    let _ = tx.send(Hint { path: root_owned.clone(), at, rescan: true });
                }
                for path in ev.paths {
                    let _ = tx.send(Hint { path, at, rescan: false });
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "watcher error; scheduling a rescan");
                let _ =
                    tx.send(Hint { path: root_owned.clone(), at: Instant::now(), rescan: true });
            }
        };
        let mut inner = notify::recommended_watcher(handler)
            .map_err(|e| Error::Daemon(format!("cannot create watcher: {e}")))?;
        inner
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| Error::Daemon(format!("cannot watch {}: {e}", root.display())))?;
        tracing::info!(root = %root.display(), "watching");
        Ok((Self { _inner: inner }, rx))
    }
}

/// What a hinted path is to the intake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKind {
    /// A markdown file (present or not): index or tombstone it.
    Markdown,
    /// Something that changes what the walker would find (a directory, an ignore file, a
    /// lost-events signal): rescan the root.
    Structural,
    /// Nothing the index cares about.
    Ignore,
}

const IGNORE_FILES: &[&str] = &[".gitignore", ".ignore", ".markdownattractorignore"];

/// Classify a hinted path. Anything under the state directory or `.git/` is ignored first,
/// because the index itself generates a stream of events there.
pub fn classify(root: &Path, path: &Path, rescan: bool) -> HintKind {
    if rescan {
        return HintKind::Structural;
    }
    let rel = path.strip_prefix(root).unwrap_or(path);
    if rel.components().any(|c| {
        let s = c.as_os_str();
        s == STATE_DIR || s == ".git"
    }) {
        return HintKind::Ignore;
    }
    if crate::walk::is_markdown(path) {
        return HintKind::Markdown;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if IGNORE_FILES.contains(&name) {
        return HintKind::Structural;
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => HintKind::Structural,
        // A vanished path with no extension was most likely a directory.
        Err(_) if path.extension().is_none() => HintKind::Structural,
        _ => HintKind::Ignore,
    }
}

/// Paths released by [`Debouncer::due`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Batch {
    /// Markdown paths that went quiet, in the order they were first hinted.
    pub paths: Vec<PathBuf>,
    /// A structural hint went quiet: the whole root should be reconciled.
    pub rescan: bool,
}

impl Batch {
    /// `true` when there is nothing to do.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.rescan
    }
}

#[derive(Debug)]
struct Entry {
    first: Instant,
    last: Instant,
    size: Option<u64>,
}

/// Holds hinted paths until they have been quiet for `quiet` and their size stopped moving.
#[derive(Debug)]
pub struct Debouncer {
    quiet: Duration,
    entries: HashMap<PathBuf, Entry>,
    rescan_at: Option<Instant>,
}

impl Debouncer {
    /// A debouncer with the given quiet period.
    pub fn new(quiet: Duration) -> Self {
        Self { quiet, entries: HashMap::new(), rescan_at: None }
    }

    /// Record a hint for a markdown path.
    pub fn push(&mut self, path: PathBuf, at: Instant) {
        let size = size_of(&path);
        self.entries
            .entry(path)
            .and_modify(|e| {
                e.last = at;
                e.size = size;
            })
            .or_insert(Entry { first: at, last: at, size });
    }

    /// Record a structural hint.
    pub fn push_rescan(&mut self, at: Instant) {
        self.rescan_at = Some(at);
    }

    /// When the earliest held hint becomes due, if anything is held.
    pub fn next_deadline(&self) -> Option<Instant> {
        let paths = self.entries.values().map(|e| e.last + self.quiet).min();
        let rescan = self.rescan_at.map(|t| t + self.quiet);
        match (paths, rescan) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Number of paths currently held.
    pub fn held(&self) -> usize {
        self.entries.len()
    }

    /// Release everything that has been quiet for the quiet period. A path whose size changed
    /// since its last hint is held for another period instead.
    pub fn due(&mut self, now: Instant) -> Batch {
        self.due_with(now, size_of)
    }

    fn due_with(&mut self, now: Instant, size: impl Fn(&Path) -> Option<u64>) -> Batch {
        let mut batch = Batch::default();
        if self.rescan_at.is_some_and(|t| now.duration_since(t) >= self.quiet) {
            self.rescan_at = None;
            batch.rescan = true;
        }
        let mut released: Vec<(Instant, PathBuf)> = Vec::new();
        for (path, entry) in &mut self.entries {
            if now.duration_since(entry.last) < self.quiet {
                continue;
            }
            let current = size(path);
            if current != entry.size {
                entry.size = current;
                entry.last = now;
                continue;
            }
            released.push((entry.first, path.clone()));
        }
        released.sort();
        for (_, path) in released {
            self.entries.remove(&path);
            batch.paths.push(path);
        }
        batch
    }
}

fn size_of(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths() {
        let root = Path::new("/r");
        assert_eq!(classify(root, Path::new("/r/a.md"), false), HintKind::Markdown);
        assert_eq!(classify(root, Path::new("/r/deep/x/b.markdown"), false), HintKind::Markdown);
        assert_eq!(
            classify(root, Path::new("/r/.markdownattractor/index.sqlite"), false),
            HintKind::Ignore
        );
        assert_eq!(
            classify(root, Path::new("/r/.markdownattractor/n.md"), false),
            HintKind::Ignore
        );
        assert_eq!(classify(root, Path::new("/r/.git/HEAD"), false), HintKind::Ignore);
        assert_eq!(classify(root, Path::new("/r/.gitignore"), false), HintKind::Structural);
        assert_eq!(
            classify(root, Path::new("/r/sub/.markdownattractorignore"), false),
            HintKind::Structural
        );
        assert_eq!(classify(root, Path::new("/r/a.md"), true), HintKind::Structural);
        assert_eq!(classify(root, Path::new("/r/gone-dir"), false), HintKind::Structural);
        assert_eq!(classify(root, Path::new("/r/notes.txt"), false), HintKind::Ignore);
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(classify(dir.path().parent().unwrap(), dir.path(), false), HintKind::Structural);
    }

    #[test]
    fn releases_after_quiet_period_in_first_hint_order() {
        let quiet = Duration::from_millis(100);
        let mut d = Debouncer::new(quiet);
        let t0 = Instant::now();
        assert_eq!(d.next_deadline(), None);
        d.push(PathBuf::from("/x/b.md"), t0);
        d.push(PathBuf::from("/x/a.md"), t0 + Duration::from_millis(10));
        d.push(PathBuf::from("/x/b.md"), t0 + Duration::from_millis(50));
        assert_eq!(d.held(), 2);
        assert_eq!(d.next_deadline(), Some(t0 + Duration::from_millis(110)));

        let early = d.due_with(t0 + Duration::from_millis(90), |_| None);
        assert!(early.is_empty());
        let mid = d.due_with(t0 + Duration::from_millis(120), |_| None);
        assert_eq!(mid.paths, vec![PathBuf::from("/x/a.md")]);
        let late = d.due_with(t0 + Duration::from_millis(160), |_| None);
        assert_eq!(late.paths, vec![PathBuf::from("/x/b.md")]);
        assert_eq!(d.held(), 0);
    }

    #[test]
    fn a_growing_file_is_held_until_its_size_settles() {
        let quiet = Duration::from_millis(100);
        let mut d = Debouncer::new(quiet);
        let t0 = Instant::now();
        d.push(PathBuf::from("/x/big.md"), t0);
        // Still being written at flush time: held for another period.
        let b1 = d.due_with(t0 + Duration::from_millis(150), |_| Some(10));
        assert!(b1.paths.is_empty());
        assert_eq!(d.next_deadline(), Some(t0 + Duration::from_millis(250)));
        let b2 = d.due_with(t0 + Duration::from_millis(260), |_| Some(20));
        assert!(b2.paths.is_empty(), "size moved again");
        let b3 = d.due_with(t0 + Duration::from_millis(400), |_| Some(20));
        assert_eq!(b3.paths, vec![PathBuf::from("/x/big.md")]);
    }

    #[test]
    fn rescan_is_debounced_too() {
        let quiet = Duration::from_millis(100);
        let mut d = Debouncer::new(quiet);
        let t0 = Instant::now();
        d.push_rescan(t0);
        d.push_rescan(t0 + Duration::from_millis(50));
        assert_eq!(d.next_deadline(), Some(t0 + Duration::from_millis(150)));
        assert!(!d.due_with(t0 + Duration::from_millis(120), |_| None).rescan);
        let b = d.due_with(t0 + Duration::from_millis(160), |_| None);
        assert!(b.rescan);
        assert!(!d.due_with(t0 + Duration::from_millis(300), |_| None).rescan, "consumed");
    }

    #[test]
    fn watcher_reports_a_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (_w, mut rx) = Watcher::start(&root).unwrap();
        // Give FSEvents/inotify a moment to arm before writing.
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(root.join("new.md"), "# New\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while Instant::now() < deadline {
            match rx.try_recv() {
                Ok(h) if h.rescan || h.path.ends_with("new.md") => {
                    seen = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        assert!(seen, "no hint for new.md within 10 s");
    }
}
