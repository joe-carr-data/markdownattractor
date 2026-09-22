//! End-to-end tests of the engine with the mock backend. No model, no network.

use std::sync::Arc;

use super::*;
use crate::worker::Mock;

fn engine_in(dir: &Path) -> Engine {
    Engine::with_parts(dir.to_path_buf(), Config::default(), Store::open_in_memory().unwrap())
}

fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
    p
}

#[test]
fn index_file_makes_sections_raw_searchable_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    write(&root, "docs/deploy.md", "# Deploy\n\n## Rollback\n\nRun deployctl rollback now.\n");
    let out = e.index_file(&root.join("docs/deploy.md")).unwrap();
    assert_eq!(out.rel_path, "docs/deploy.md");
    assert_eq!(out.sections, 2);
    assert_eq!(out.upsert.new_hashes.len(), 2);

    let hits =
        crate::search::search(e.store(), "deployctl", &crate::search::SearchOptions::default())
            .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].pending);
    assert_eq!(hits[0].matched, crate::search::Matched::Raw);
    assert_eq!(hits[0].heading_path, vec!["Deploy", "Rollback"]);
}

#[test]
fn index_root_walks_sorts_and_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    write(&root, "a.md", "# A\n\nlong ".repeat(50).as_str());
    write(&root, "b.md", "# B\n\nshort\n");
    write(&root, "skip.txt", "not markdown");
    let r = e.index_root().unwrap();
    assert_eq!(r.files, 2);
    assert_eq!(r.changed, 2);
    assert_eq!(r.tombstoned, 0);

    std::fs::remove_file(root.join("b.md")).unwrap();
    let r2 = e.index_root().unwrap();
    assert_eq!(r2.files, 1);
    assert_eq!(r2.changed, 0, "unchanged file is not re-reported");
    assert_eq!(r2.tombstoned, 1);
    assert!(e.store().document_by_path("b.md").unwrap().unwrap().deleted_at.is_some());
}

#[tokio::test]
async fn summarize_pending_attaches_validated_cards() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    write(&root, "n.md", "# Notes\n\nSince March 2026 we use blue-green.\n\n## Other\n\ntext\n");
    e.index_file(&root.join("n.md")).unwrap();

    let backend = Arc::new(Mock::new().default_ok());
    let mut seen = Vec::new();
    let report = e
        .summarize_pending(
            backend.clone(),
            CancellationToken::new(),
            SummarizeOptions::default(),
            |p| seen.push(p.done),
        )
        .await
        .unwrap();
    assert_eq!(report.submitted, 2);
    assert_eq!(report.ok, 2);
    assert_eq!(report.failed, 0);
    assert_eq!(seen, vec![1, 2]);
    assert_eq!(backend.calls().len(), 2);

    let counts = e.store().counts().unwrap();
    assert_eq!(counts.summarized, 2);
    assert_eq!(counts.pending, 0);

    // Cards are now searchable and no longer pending.
    let hits = crate::search::search(e.store(), "notes", &crate::search::SearchOptions::default())
        .unwrap();
    assert!(hits.iter().all(|h| !h.pending));
    assert!(hits.iter().any(|h| h.tldr.is_some()));

    // Nothing left to do; a second run submits nothing.
    let again = e
        .summarize_pending(backend, CancellationToken::new(), SummarizeOptions::default(), |_| {})
        .await
        .unwrap();
    assert_eq!(again.submitted, 0);
}

#[tokio::test]
async fn failed_jobs_are_recorded_with_reason() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    write(&root, "f.md", "# F\n\nbody\n");
    let out = e.index_file(&root.join("f.md")).unwrap();
    let hash = out.upsert.new_hashes[0].clone();

    let backend =
        Arc::new(Mock::new().on(&hash, Outcome::Fatal { reason: "budget exhausted".into() }));
    let report = e
        .summarize_pending(backend, CancellationToken::new(), SummarizeOptions::default(), |_| {})
        .await
        .unwrap();
    assert_eq!(report.failed, 1);
    let counts = e.store().counts().unwrap();
    assert_eq!(counts.failed, 1);
    let section = e.store().section(&format!("{}#0", out.upsert.doc_id)).unwrap().unwrap();
    assert_eq!(section.fail_reason.as_deref(), Some("budget exhausted"));
}

#[tokio::test]
async fn limit_and_budget_defer_sections() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    write(&root, "l.md", "# A\n\none\n\n# B\n\ntwo\n\n# C\n\nthree\n");
    e.index_file(&root.join("l.md")).unwrap();

    let backend = Arc::new(Mock::new().default_ok());
    let r = e
        .summarize_pending(
            backend.clone(),
            CancellationToken::new(),
            SummarizeOptions { limit: Some(2), ..SummarizeOptions::default() },
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(r.submitted, 2);
    assert_eq!(r.deferred, 1);
    assert!(!r.budget_exhausted);
    assert_eq!(e.store().counts().unwrap().pending, 1);

    // A budget smaller than one section's estimated cost submits nothing.
    let cfg =
        Config { daily_token_budget: Some(TOKENS_PER_SECTION_ESTIMATE / 2), ..Config::default() };
    let mut e2 = Engine::with_parts(root.clone(), cfg, Store::open_in_memory().unwrap());
    e2.index_file(&root.join("l.md")).unwrap();
    let r2 = e2
        .summarize_pending(backend, CancellationToken::new(), SummarizeOptions::default(), |_| {})
        .await
        .unwrap();
    assert_eq!(r2.submitted, 0);
    assert_eq!(r2.deferred, 3);
    assert!(r2.budget_exhausted);
}

#[test]
fn editing_one_section_needs_exactly_one_card() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    let p = write(&root, "e.md", "# A\n\none\n\n# B\n\ntwo\n");
    e.index_file(&p).unwrap();
    write(&root, "e.md", "# A\n\none\n\n# B\n\ntwo changed\n");
    let out = e.index_file(&p).unwrap();
    assert_eq!(out.upsert.new_hashes.len(), 1);
    assert_eq!(out.upsert.unchanged, 1);
}

#[test]
fn inserting_lines_above_refreshes_ranges_without_new_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    let p = write(&root, "m.md", "# A\n\none\n\n# B\n\ntwo\n");
    let first = e.index_file(&p).unwrap();
    let doc_id = first.upsert.doc_id.clone();
    let before = e.store().section(&format!("{doc_id}#1")).unwrap().unwrap();

    // Front matter above everything: every section shifts, no section text changes.
    write(&root, "m.md", "---\nstatus: current\n---\n\n# A\n\none\n\n# B\n\ntwo\n");
    let out = e.index_file(&p).unwrap();
    assert!(out.upsert.new_hashes.is_empty(), "no model call needed: {:?}", out.upsert);
    let after = e.store().section(&format!("{doc_id}#1")).unwrap().unwrap();
    assert_eq!(after.section_hash, before.section_hash);
    assert_eq!(after.line_start, before.line_start + 4);
}

#[test]
fn open_section_returns_exact_lines_and_detects_staleness() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    let p = write(&root, "o.md", "# A\n\none\n\n## B\n\ntwo\nthree\n");
    let out = e.index_file(&p).unwrap();
    let id = format!("{}#1", out.upsert.doc_id);

    let o = e.open_section(&id).unwrap();
    assert!(!o.stale);
    assert_eq!((o.line_start, o.line_end), (5, 8));
    assert_eq!(o.text, "## B\n\ntwo\nthree");
    assert_eq!(o.heading_path, vec!["A", "B"]);

    // Change the file behind the index's back.
    write(&root, "o.md", "# A\n\nintro\n\none\n\n## B\n\ntwo\nthree\nfour\n");
    let o2 = e.open_section(&id).unwrap();
    assert!(o2.stale);
    assert_eq!(o2.text, "## B\n\ntwo\nthree\nfour", "current lines, not the indexed ones");
    assert_eq!((o2.line_start, o2.line_end), (7, 11));
    // And the store was refreshed as a side effect.
    let refreshed = e.store().section(&id).unwrap().unwrap();
    assert_eq!(refreshed.line_start, 7);
}

#[test]
fn open_after_prepend_returns_current_id_and_refreshes_store() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    let p = write(&root, "mv.md", "# A\n\none\n\n# B\n\ntwo\n");
    let out = e.index_file(&p).unwrap();
    let doc_id = out.upsert.doc_id.clone();
    let b_old = format!("{doc_id}#1");

    // Prepend a whole section: B moves from #1 to #2 without changing content.
    write(&root, "mv.md", "# X\n\nnew\n\n# A\n\none\n\n# B\n\ntwo\n");
    let o = e.open_section(&b_old).unwrap();
    assert!(o.stale);
    assert_eq!(o.requested_id, b_old);
    assert_eq!(o.section_id, format!("{doc_id}#2"), "the current id, not the requested one");
    assert_eq!(o.text, "# B\n\ntwo");
    // The store now agrees: #2 is B, #1 is A.
    assert_eq!(e.store().section(&format!("{doc_id}#2")).unwrap().unwrap().heading_path, vec!["B"]);
    assert_eq!(e.store().section(&format!("{doc_id}#1")).unwrap().unwrap().heading_path, vec!["A"]);
    // Same-hash, same-index but shifted lines (front matter) is also reported and refreshed.
    write(&root, "mv.md", "---\nk: v\n---\n# X\n\nnew\n\n# A\n\none\n\n# B\n\ntwo\n");
    let o2 = e.open_section(&format!("{doc_id}#2")).unwrap();
    assert!(o2.stale);
    assert_eq!(o2.line_start, 12);
    assert_eq!(e.store().section(&format!("{doc_id}#2")).unwrap().unwrap().line_start, 12);
}

#[test]
fn paths_cannot_escape_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let e = engine_in(&root);
    assert!(matches!(e.rel_path(&root.join("../x.md")), Err(Error::NotFound(_))));
    assert!(matches!(e.rel_path(Path::new("/x.md")), Err(Error::NotFound(_))));
    assert!(matches!(e.safe_join("../etc/passwd"), Err(Error::NotFound(_))));
    assert!(matches!(e.safe_join("/etc/passwd"), Err(Error::NotFound(_))));
    assert!(e.safe_join("missing.md").is_err(), "must exist to be joined");
}

#[cfg(unix)]
#[test]
fn symlinks_out_of_the_root_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    let outside = tempfile::tempdir().unwrap();
    let target = write(outside.path(), "secret.md", "# Secret\n\nkeys\n");
    std::os::unix::fs::symlink(&target, root.join("link.md")).unwrap();
    let err = e.index_file(&root.join("link.md")).unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err}");
    assert!(matches!(e.safe_join("link.md"), Err(Error::NotFound(_))));
    assert_eq!(e.index_root().unwrap().files, 0, "walker skips the symlink too");
}

#[test]
fn unreadable_file_is_not_tombstoned() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    let p = write(&root, "keep.md", "# Keep\n\nbody\n");
    e.index_root().unwrap();
    // Invalid UTF-8 makes the parse fail; the file still exists, so it must survive.
    std::fs::write(&p, [0xff, 0xfe, b'#']).unwrap();
    let r = e.index_root().unwrap();
    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.tombstoned, 0);
    let doc = e.store().document_by_path("keep.md").unwrap().unwrap();
    assert!(doc.deleted_at.is_none());
    assert_eq!(e.store().counts().unwrap().sections, 1, "old sections stay searchable");
}

#[tokio::test]
async fn pool_stop_leaves_unstarted_jobs_pending() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let cfg = Config { concurrency: Some(1), ..Config::default() };
    let mut e = Engine::with_parts(root.clone(), cfg, Store::open_in_memory().unwrap());
    write(
        &root,
        "s.md",
        "# A\n\nshort\n\n# B\n\nlonger text here\n\n# C\n\neven longer text here\n",
    );
    let out = e.index_file(&root.join("s.md")).unwrap();
    // Smallest section runs first; make it a pool-stopping auth failure.
    let first = out.upsert.new_hashes[0].clone();
    let backend = Arc::new(Mock::new().default_ok().on(
        &first,
        Outcome::Fatal { reason: format!("{} (HTTP 401)", crate::worker::FATAL_NOT_LOGGED_IN) },
    ));
    let r = e
        .summarize_pending(backend, CancellationToken::new(), SummarizeOptions::default(), |_| {})
        .await
        .unwrap();
    assert_eq!(r.failed, 1);
    assert_eq!(r.deferred, 2, "jobs that never started are deferred, not failed");
    let c = e.store().counts().unwrap();
    assert_eq!(c.failed, 1);
    assert_eq!(c.pending, 2);
}

#[tokio::test]
async fn usage_ledger_counts_failed_attempts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    write(&root, "u.md", "# A\n\nbody\n");
    let out = e.index_file(&root.join("u.md")).unwrap();
    let h = out.upsert.new_hashes[0].clone();
    let backend = Arc::new(Mock::new().on(
        &h,
        Outcome::Malformed {
            reason: "structured_output is null".into(),
            raw: "nope".into(),
            usage: Some(crate::worker::Usage {
                input_tokens: 1000,
                output_tokens: 50,
                ..Mock::canned_usage("haiku")
            }),
        },
    ));
    let r = e
        .summarize_pending(backend, CancellationToken::new(), SummarizeOptions::default(), |_| {})
        .await
        .unwrap();
    assert_eq!(r.failed, 1);
    let spent = e.store().usage_since(Timestamp::UNIX_EPOCH).unwrap();
    assert!(spent.input_tokens >= 1000, "failed attempts are on the ledger: {spent:?}");
}

#[test]
fn open_unknown_section_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut e = engine_in(&root);
    assert!(matches!(e.open_section("nope#0"), Err(Error::NotFound(_))));
}

#[test]
fn rel_path_rejects_outside_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let e = engine_in(&root);
    assert!(e.rel_path(Path::new("/definitely/elsewhere.md")).is_err());
    assert_eq!(e.rel_path(&root.join("a/b.md")).unwrap(), "a/b.md");
}

#[test]
fn file_times_are_sane() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(dir.path(), "t.md", "# T\n");
    let t = file_times(&p).unwrap();
    assert!(t.modified_at <= t.now);
    if let Some(c) = t.created_at {
        assert!(c <= t.modified_at);
    }
}
