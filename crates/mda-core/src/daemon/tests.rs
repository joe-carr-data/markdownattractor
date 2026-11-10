//! The daemon end to end on a temporary root with the mock backend: start, write, rename,
//! delete, stop. Real watcher, real socket, no model.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::store::{EventKind, Store};
use crate::worker::Mock;

fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
    p
}

/// Tests must never download a model: turn embeddings off in the root's config.
fn embeddings_off(root: &Path) {
    let cfg =
        crate::config::Config { embeddings: crate::config::Embeddings::Off, ..Default::default() };
    cfg.save(root).unwrap();
}

fn store(root: &Path) -> Store {
    Store::open(&Engine::index_path(root)).unwrap()
}

async fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn fast() -> DaemonConfig {
    DaemonConfig {
        debounce: Duration::from_millis(150),
        round_size: Some(4),
        idle_poll: Duration::from_millis(200),
        backoff_min: Duration::from_millis(100),
        backoff_max: Duration::from_millis(400),
        ..DaemonConfig::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daemon_indexes_summarizes_renames_deletes_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(&root, "first.md", "# First\n\nalready here\n");
    embeddings_off(&root);

    let backend: DynBackend = Arc::new(Mock::new().default_ok());
    let cancel = CancellationToken::new();
    let task = {
        let (root, cancel) = (root.clone(), cancel.clone());
        tokio::spawn(async move { run(&root, backend, fast(), cancel).await })
    };

    wait_until("daemon socket", || futures_lite_block(is_running(&root))).await;
    let info = DaemonInfo::read(&root).unwrap().expect("info file");
    assert_eq!(info.pid, std::process::id());
    assert!(pid_path(&root).exists());

    // The initial scan indexed the file that was already there; the summarizer carded it.
    wait_until("first.md carded", || {
        let c = store(&root).counts().unwrap();
        c.docs == 1 && c.summarized == 1
    })
    .await;

    // A new file shows up: raw-searchable, then carded, and served as hot. (A new directory
    // would be a structural hint and go through a rescan instead; that path is exercised by
    // `sub/asked.md` below.)
    let mut client = Client::connect(&root).await.unwrap();
    assert_eq!(client.request(&Request::Watch).await.unwrap(), Response::Ok);
    write(&root, "new.md", "# New\n\nfresh text\n\n## Sub\n\nmore\n");
    wait_until("new.md indexed", || store(&root).document_by_path("new.md").unwrap().is_some())
        .await;
    wait_until("new.md carded", || store(&root).counts().unwrap().summarized == 3).await;
    let mut saw_indexed = false;
    let mut saw_round = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while (!saw_indexed || !saw_round) && Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), client.next_response()).await {
            Ok(Ok(Some(Response::Event(DaemonEvent::Indexed { rel_path, .. }))))
                if rel_path == "new.md" =>
            {
                saw_indexed = true;
            }
            Ok(Ok(Some(Response::Event(DaemonEvent::RoundFinished { .. })))) => saw_round = true,
            Ok(Ok(Some(_))) => {}
            _ => break,
        }
    }
    assert!(saw_indexed, "watch stream reported the indexed file");
    assert!(saw_round, "watch stream reported a round");
    drop(client);

    // Status over the socket.
    let mut c = Client::connect(&root).await.unwrap();
    let Response::Status(status) = c.request(&Request::Status).await.unwrap() else {
        unreachable!("status")
    };
    assert!(status.watching, "{status:?}");
    assert!(status.synced >= 1);
    assert!(status.cards >= 3);
    assert_eq!(status.failures, 0);
    assert!(!status.paused);

    // Rename: same content at a new path costs no call and keeps history.
    std::fs::rename(root.join("new.md"), root.join("moved.md")).unwrap();
    wait_until("rename recorded", || {
        let s = store(&root);
        s.document_by_path("moved.md").unwrap().is_some_and(|d| d.deleted_at.is_none())
            && s.document_by_path("new.md").unwrap().is_some_and(|d| d.deleted_at.is_some())
    })
    .await;
    let s = store(&root);
    let events = s.timeline(None, None, 1000).unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.kind == EventKind::DocRenamed && e.detail.as_deref() == Some("new.md")),
        "{events:?}"
    );
    assert_eq!(s.counts().unwrap().summarized, 3, "cards followed the content");
    assert_eq!(s.counts().unwrap().pending, 0);
    drop(s);

    // Delete: tombstoned.
    std::fs::remove_file(root.join("moved.md")).unwrap();
    wait_until("moved.md tombstoned", || {
        store(&root).document_by_path("moved.md").unwrap().is_some_and(|d| d.deleted_at.is_some())
    })
    .await;

    // Index on request, pause and resume.
    write(&root, "sub/asked.md", "# Asked\n\nvia the socket\n");
    assert_eq!(c.request(&Request::Pause).await.unwrap(), Response::Ok);
    let Response::Indexed(report) =
        c.request(&Request::Index { path: Some("sub/asked.md".into()) }).await.unwrap()
    else {
        unreachable!("indexed")
    };
    assert_eq!(report.files, 1);
    assert_eq!(report.pending, 1);
    let Response::Status(status) = c.request(&Request::Status).await.unwrap() else {
        unreachable!("status")
    };
    assert!(status.paused);
    assert_eq!(c.request(&Request::Resume).await.unwrap(), Response::Ok);
    wait_until("asked.md carded", || {
        store(&root).document_by_path("sub/asked.md").unwrap().is_some()
            && store(&root).counts().unwrap().pending == 0
    })
    .await;

    // Stop: files gone, run returns Ok.
    assert_eq!(c.request(&Request::Stop).await.unwrap(), Response::Ok);
    let result = tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert!(!pid_path(&root).exists());
    assert_eq!(DaemonInfo::read(&root).unwrap(), None);
    assert!(!is_running(&root).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_daemon_on_the_same_root_is_refused_and_failures_back_off() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(&root, "a.md", "# A\n\nbody\n");
    embeddings_off(&root);
    // Every call fails: the summarizer must back off instead of spinning.
    let backend: DynBackend = Arc::new(Mock::new());
    let cancel = CancellationToken::new();
    let task = {
        let (root, cancel, backend) = (root.clone(), cancel.clone(), backend.clone());
        tokio::spawn(async move { run(&root, backend, fast(), cancel).await })
    };
    wait_until("daemon socket", || futures_lite_block(is_running(&root))).await;

    let err = run(&root, backend, fast(), CancellationToken::new()).await.unwrap_err();
    assert!(err.to_string().contains("already"), "{err}");

    wait_until("a.md failed once", || store(&root).counts().unwrap().failed == 1).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let mut c = Client::connect(&root).await.unwrap();
    let Response::Status(status) = c.request(&Request::Status).await.unwrap() else {
        unreachable!("status")
    };
    assert!(status.rounds >= 1);
    assert!(status.rounds <= 3, "rounds should be rate-limited by backoff: {status:?}");

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), task).await.unwrap().unwrap().unwrap();
    assert!(!is_running(&root).await);
}

/// Poll an async liveness check from a sync closure without nesting runtimes.
fn futures_lite_block(fut: impl std::future::Future<Output = bool>) -> bool {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
}

/// Run git in `dir` with a fixed identity and commit date.
fn git_at(dir: &Path, date: &str, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid"])
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daemon_dates_from_git_falls_back_on_a_save_and_redates_after_a_commit() {
    use crate::store::{GitState, TimeSource};
    const T1: &str = "2024-03-01T10:00:00Z";
    const T2: &str = "2025-06-15T12:00:00Z";
    const GUIDE: &str = "# Guide\n\nIntro.\n\n## Install\n\nRun the installer.\n";

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git_at(&root, T1, &["init", "-q"]);
    write(&root, ".gitignore", ".markdownattractor/\n");
    write(&root, "guide.md", GUIDE);
    git_at(&root, T1, &["add", "-A"]);
    git_at(&root, T1, &["commit", "-q", "-m", "add guide"]);
    embeddings_off(&root);

    let backend: DynBackend = Arc::new(Mock::new().default_ok());
    let cancel = CancellationToken::new();
    let dcfg = DaemonConfig { git_poll: Duration::from_millis(200), ..fast() };
    let task = {
        let (root, cancel) = (root.clone(), cancel.clone());
        tokio::spawn(async move { run(&root, backend, dcfg, cancel).await })
    };
    let doc = |root: &Path| store(root).document_by_path("guide.md").unwrap();
    let sources = |root: &Path| -> Vec<(Timestamp, TimeSource)> {
        let s = store(root);
        let id = s.document_by_path("guide.md").unwrap().unwrap().doc_id;
        s.sections_of(&id).unwrap().into_iter().map(|x| (x.updated_at, x.updated_source)).collect()
    };
    let t1: Timestamp = T1.parse().unwrap();
    let t2: Timestamp = T2.parse().unwrap();

    // The initial scan indexed the file; the dating task gave it its history's time.
    // Like the test above: nothing opens the store before the daemon has created it.
    wait_until("daemon socket", || futures_lite_block(is_running(&root))).await;
    wait_until("guide.md dated from git", || {
        let d = doc(&root);
        d.is_some_and(|d| d.git_state == GitState::Git && d.updated_at == t1)
    })
    .await;
    assert_eq!(sources(&root), vec![(t1, TimeSource::Git), (t1, TimeSource::Git)]);

    // A save: the edited section is on file time at once; the untouched one keeps git. The
    // dating task examines the dirty file and leaves it alone.
    write(&root, "guide.md", &GUIDE.replace("Run the installer.", "Run the new installer."));
    wait_until("the save is indexed and judged dirty", || {
        doc(&root).is_some_and(|d| d.git_state == GitState::Filesystem)
    })
    .await;
    let s = sources(&root);
    assert_eq!(s[0], (t1, TimeSource::Git));
    assert_eq!(s[1].1, TimeSource::Filesystem);
    assert!(s[1].0 > t2, "the save's own time");

    // A commit, with no further save: the poll sees the new commit and the section is dated.
    git_at(&root, T2, &["commit", "-q", "-am", "edit install"]);
    wait_until("the commit re-dates the edited section", || {
        doc(&root).is_some_and(|d| d.git_state == GitState::Git)
            && sources(&root)[1] == (t2, TimeSource::Git)
    })
    .await;
    assert_eq!(sources(&root)[0], (t1, TimeSource::Git));

    cancel.cancel();
    task.await.unwrap().unwrap();
}
