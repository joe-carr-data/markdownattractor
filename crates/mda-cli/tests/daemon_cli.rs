//! `mda start | status | index | watch | pause | resume | stop` against the real binary on a
//! temporary root. The backend points at a closed port, so no model is ever contacted: the
//! daemon indexes, its rounds fail fast and back off, and raw search stays live.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::too_many_lines // one scenario, one daemon: splitting it would spawn several
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use predicates::prelude::*;

fn mda() -> Command {
    let mut c = Command::cargo_bin("mda").expect("binary builds");
    c.env("NO_COLOR", "1");
    c
}

fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
    p
}

fn json_of(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).expect("valid json on stdout")
}

fn status(root: &Path) -> serde_json::Value {
    let out = mda().args(["--json", "status", "--root"]).arg(root).output().unwrap();
    json_of(&out.stdout)
}

fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Stops the daemon when the test ends, pass or fail, so no process outlives the test.
struct StopGuard(PathBuf);
impl Drop for StopGuard {
    fn drop(&mut self) {
        let _ = mda().args(["stop", "--root"]).arg(&self.0).output();
    }
}

#[test]
fn start_status_index_pause_resume_stop() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(&root, "one.md", "# One\n\nfirst body\n");
    write(
        &root,
        ".markdownattractor/config.toml",
        "backend = \"local\"\nlocal_base_url = \"http://127.0.0.1:1/v1\"\nembeddings = \"off\"\n",
    );

    // Nothing running yet: stop is a no-op, status says so.
    mda()
        .args(["stop", "--root"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("not running"));
    assert!(status(&root)["daemon"].is_null());

    // Bounded: a `start` whose output never reaches EOF is a bug, not something to wait on.
    let out = mda()
        .args(["--json", "start", "--root"])
        .arg(&root)
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let _guard = StopGuard(root.clone());
    let v = json_of(&out);
    assert_eq!(v["started"], true);
    assert_eq!(v["files"], 1);
    let pid = v["pid"].as_u64().unwrap();
    assert!(root.join(".markdownattractor/daemon.pid").exists());
    assert!(root.join(".markdownattractor/logs").is_dir());

    // Starting again is idempotent.
    mda()
        .args(["start", "--root"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("already running"));

    // Live status is merged into `mda status`.
    wait_for("initial scan", || status(&root)["counts"]["docs"] == 1);
    let s = status(&root);
    assert_eq!(s["daemon"]["pid"], pid);
    assert_eq!(s["daemon"]["watching"], true);
    mda()
        .args(["status", "--root"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("daemon running"));

    // A saved file is raw-searchable without running anything.
    write(&root, "two.md", "# Two\n\nzebra crossing\n");
    wait_for("two.md indexed by the watcher", || status(&root)["counts"]["docs"] == 2);
    mda()
        .args(["--json", "search", "zebra", "--root"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("two.md"));

    // `mda index` while the daemon runs delegates instead of writing itself.
    write(&root, "three.md", "# Three\n\nvia index\n");
    mda()
        .args(["index", "three.md", "--root"])
        .arg(&root)
        .current_dir(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("via the daemon"))
        .stdout(predicate::str::contains("1 section(s) queued"));
    assert_eq!(status(&root)["counts"]["docs"], 3);

    // pause / resume round-trip and show in status.
    mda()
        .args(["pause", "--root"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("paused"));
    assert_eq!(status(&root)["daemon"]["paused"], true);
    mda()
        .args(["resume", "--root"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("resumed"));
    assert_eq!(status(&root)["daemon"]["paused"], false);

    // watch streams JSON lines; --count bounds it.
    write(&root, "four.md", "# Four\n\nwatched\n");
    let out = mda()
        .args(["--json", "watch", "--count", "1", "--root"])
        .arg(&root)
        .timeout(Duration::from_secs(20))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let line = String::from_utf8(out).unwrap();
    let ev: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert!(ev["event"].is_string(), "{ev}");

    // doctor sees it.
    mda()
        .args(["doctor", "--root"])
        .arg(&root)
        .assert()
        .stdout(predicate::str::contains("running · pid"));

    // stop: files gone, status shows not running, cards left pending for the next start.
    mda()
        .args(["stop", "--root"])
        .arg(&root)
        .assert()
        .success()
        .stdout(predicate::str::contains("stopped"));
    assert!(!root.join(".markdownattractor/daemon.pid").exists());
    assert!(status(&root)["daemon"].is_null());
    let logs: Vec<String> = std::fs::read_dir(root.join(".markdownattractor/logs"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.starts_with("daemon.") && Path::new(n).extension().is_some_and(|e| e == "log")
        })
        .collect();
    assert_eq!(logs.len(), 1, "one dated log file: {logs:?}");
    let log = std::fs::read_to_string(root.join(".markdownattractor/logs").join(&logs[0])).unwrap();
    assert!(log.contains("daemon started"), "log file has the startup line: {log}");
    assert!(log.contains("daemon stopped"), "log file has the shutdown line: {log}");
}
