//! Git dating end to end (ADR-0008): real temporary repositories with fixed commit dates.
//!
//! Every test builds its own repository; author and committer dates are set per commit so the
//! expected times are exact. The tests need a `git` binary, like the product feature.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_panics_doc)]

use std::path::{Path, PathBuf};
use std::process::Command;

use jiff::Timestamp;
use mda_core::pipeline::{DateLimits, DateReport, Engine};
use mda_core::store::{GitState, StoredDocument, TimeSource};

const T1: &str = "2024-03-01T10:00:00Z";
const T2: &str = "2025-06-15T12:00:00Z";
const T3: &str = "2026-01-20T08:30:00Z";

fn ts(s: &str) -> Timestamp {
    s.parse().unwrap()
}

/// Run git in `dir` and return stdout; panics with stderr on failure.
fn git_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args([
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.autocrlf=false",
        "-c",
        "init.defaultBranch=main",
    ]);
    cmd.args(args).envs(env.iter().copied());
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn git(dir: &Path, args: &[&str]) -> String {
    git_env(dir, args, &[])
}

/// Stage everything and commit with the given author date (committer date the same).
fn commit_at(dir: &Path, date: &str, msg: &str) {
    commit_with(dir, date, date, msg);
}

fn commit_with(dir: &Path, author: &str, committer: &str, msg: &str) {
    git(dir, &["add", "-A"]);
    git_env(
        dir,
        &["commit", "-q", "--allow-empty", "-m", msg],
        &[("GIT_AUTHOR_DATE", author), ("GIT_COMMITTER_DATE", committer)],
    );
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A fresh repository in a temp dir that ignores the index's state directory.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    git(&dir, &["init", "-q"]);
    write(&dir, ".gitignore", ".markdownattractor/\n");
    (tmp, dir)
}

fn index(root: &Path) -> Engine {
    let mut engine = Engine::open(root).unwrap();
    let report = engine.index_root().unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    engine
}

fn date(engine: &mut Engine) -> DateReport {
    engine.date_from_git(DateLimits::default()).unwrap()
}

fn doc(engine: &Engine, rel: &str) -> StoredDocument {
    engine.store().document_by_path(rel).unwrap().expect("the document is indexed")
}

/// `(heading, updated_at, source)` of each section of a document.
fn sections(engine: &Engine, rel: &str) -> Vec<(String, Timestamp, TimeSource)> {
    let id = doc(engine, rel).doc_id;
    engine
        .store()
        .sections_of(&id)
        .unwrap()
        .into_iter()
        .map(|s| {
            (s.heading_path.last().cloned().unwrap_or_default(), s.updated_at, s.updated_source)
        })
        .collect()
}

const TWO_SECTIONS: &str =
    "# Guide\n\nIntro.\n\n## Install\n\nRun the installer.\n\n## Use\n\nType things.\n";

#[test]
fn a_committed_file_gets_document_and_per_section_times_from_git() {
    let (_tmp, dir) = repo();
    write(&dir, "guide.md", TWO_SECTIONS);
    commit_at(&dir, T1, "add guide");
    // Change only the "Use" section in a later commit.
    write(&dir, "guide.md", &TWO_SECTIONS.replace("Type things.", "Type other things."));
    commit_at(&dir, T2, "edit use");

    let mut engine = index(&dir);
    let before = doc(&engine, "guide.md");
    assert_eq!(before.git_state, GitState::Needs);
    assert_eq!(before.updated_source, TimeSource::Filesystem);

    let report = date(&mut engine);
    assert_eq!(
        (report.source.as_str(), report.dated, report.remaining),
        ("usable", 1, 0),
        "{report:?}"
    );

    let d = doc(&engine, "guide.md");
    assert_eq!(d.git_state, GitState::Git);
    assert_eq!((d.created_at, d.created_source), (ts(T1), TimeSource::Git));
    assert_eq!((d.updated_at, d.updated_source), (ts(T2), TimeSource::Git));
    assert_eq!(d.git_commit.as_deref(), Some(git(&dir, &["rev-parse", "HEAD"]).as_str()));
    assert!(d.file_mtime.is_some(), "the file's own time is kept beside the git time");

    let s = sections(&engine, "guide.md");
    assert_eq!(
        s,
        vec![
            ("Guide".to_owned(), ts(T1), TimeSource::Git),
            ("Install".to_owned(), ts(T1), TimeSource::Git),
            ("Use".to_owned(), ts(T2), TimeSource::Git),
        ]
    );
}

#[test]
fn a_folder_that_is_not_a_repository_keeps_filesystem_times() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    write(&dir, "notes.md", "# Notes\n\nText.\n");
    let mut engine = index(&dir);
    let before = doc(&engine, "notes.md");
    let report = date(&mut engine);
    assert_eq!(report.source, "not_a_repository");
    assert_eq!(report.dated, 0);
    assert_eq!(doc(&engine, "notes.md"), before, "nothing about the document changed");
}

#[test]
fn untracked_modified_and_staged_files_keep_filesystem_times() {
    let (_tmp, dir) = repo();
    write(&dir, "clean.md", "# Clean\n\nSame.\n");
    write(&dir, "modified.md", "# Modified\n\nBefore.\n");
    write(&dir, "staged.md", "# Staged\n\nBefore.\n");
    commit_at(&dir, T1, "three files");
    write(&dir, "modified.md", "# Modified\n\nAfter.\n");
    write(&dir, "staged.md", "# Staged\n\nAfter.\n");
    git(&dir, &["add", "staged.md"]);
    write(&dir, "untracked.md", "# Untracked\n\nNew.\n");

    let mut engine = index(&dir);
    let report = date(&mut engine);
    assert_eq!((report.dated, report.ineligible, report.failed), (1, 3, 0), "{report:?}");
    assert_eq!(doc(&engine, "clean.md").updated_source, TimeSource::Git);
    for rel in ["modified.md", "staged.md", "untracked.md"] {
        let d = doc(&engine, rel);
        assert_eq!(d.git_state, GitState::Filesystem, "{rel}");
        assert_eq!(
            (d.created_source, d.updated_source),
            (TimeSource::Filesystem, TimeSource::Filesystem),
            "{rel}"
        );
        assert!(sections(&engine, rel).iter().all(|s| s.2 == TimeSource::Filesystem), "{rel}");
    }
}

#[test]
fn an_edit_falls_back_to_file_time_and_a_commit_dates_it_again() {
    let (_tmp, dir) = repo();
    write(&dir, "guide.md", TWO_SECTIONS);
    commit_at(&dir, T1, "add guide");
    let mut engine = index(&dir);
    date(&mut engine);

    // Edit one section without committing: that section is on file time, the others keep git.
    write(&dir, "guide.md", &TWO_SECTIONS.replace("Run the installer.", "Run the new installer."));
    engine.index_root().unwrap();
    let d = doc(&engine, "guide.md");
    assert_eq!((d.git_state, d.updated_source), (GitState::Needs, TimeSource::Filesystem));
    let s = sections(&engine, "guide.md");
    assert_eq!((s[0].1, s[0].2), (ts(T1), TimeSource::Git), "untouched section keeps its git time");
    assert_eq!(s[1].2, TimeSource::Filesystem, "edited section is on file time");
    assert!(s[1].1 > ts(T3), "and that time is the save, not history");
    let report = date(&mut engine);
    assert_eq!((report.dated, report.ineligible), (0, 1), "dirty: not dated, {report:?}");

    // Commit it: the next pass sees the new commit and dates the edited section from it.
    commit_at(&dir, T2, "edit install");
    let report = date(&mut engine);
    assert_eq!((report.dated, report.remaining), (1, 0), "{report:?}");
    let s = sections(&engine, "guide.md");
    assert_eq!((s[1].1, s[1].2), (ts(T2), TimeSource::Git));
    assert_eq!((s[2].1, s[2].2), (ts(T1), TimeSource::Git));
}

#[test]
fn a_new_commit_redates_only_the_files_it_names_and_an_amend_redates_everything() {
    let (_tmp, dir) = repo();
    write(&dir, "a.md", "# A\n\nOne.\n");
    write(&dir, "b.md", "# B\n\nOne.\n");
    commit_at(&dir, T1, "a and b");
    let mut engine = index(&dir);
    assert_eq!(date(&mut engine).dated, 2);

    // An edit and its revert: the bytes are back, the history is not.
    write(&dir, "a.md", "# A\n\nTwo.\n");
    commit_at(&dir, T2, "edit a");
    write(&dir, "a.md", "# A\n\nOne.\n");
    commit_at(&dir, T3, "revert a");
    let report = date(&mut engine);
    assert_eq!(report.dated, 1, "only a.md was named in between: {report:?}");
    assert_eq!(doc(&engine, "a.md").updated_at, ts(T3), "the revert is its newest history");
    assert_eq!(doc(&engine, "b.md").updated_at, ts(T1));

    // Rewrite the last commit's date: the validated commit is no longer an ancestor.
    git_env(
        &dir,
        &["commit", "-q", "--amend", "--no-edit", "--date", T2],
        &[("GIT_COMMITTER_DATE", T2)],
    );
    let report = date(&mut engine);
    assert_eq!(report.dated, 2, "rewritten history re-dates every document: {report:?}");
    assert_eq!(doc(&engine, "a.md").updated_at, ts(T2));
}

#[test]
fn a_change_made_only_by_a_merge_is_noticed() {
    let (_tmp, dir) = repo();
    write(&dir, "doc.md", "# Doc\n\nBase.\n");
    commit_at(&dir, T1, "base");
    git(&dir, &["checkout", "-q", "-b", "side"]);
    write(&dir, "doc.md", "# Doc\n\nSide.\n");
    commit_at(&dir, T2, "side");
    git(&dir, &["checkout", "-q", "main"]);
    write(&dir, "doc.md", "# Doc\n\nMain.\n");
    commit_at(&dir, T2, "main");
    let mut engine = index(&dir);
    assert_eq!(date(&mut engine).dated, 1);

    // Merge with a conflict, resolved to text neither parent had, indexed before the commit.
    let merged = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "merge",
            "-q",
            "side",
        ])
        .output()
        .unwrap();
    assert!(!merged.status.success(), "the merge conflicts");
    write(&dir, "doc.md", "# Doc\n\nResolved.\n");
    engine.index_root().unwrap();
    assert_eq!(date(&mut engine).ineligible, 1, "dirty during the merge");
    commit_at(&dir, T3, "merge side");

    let report = date(&mut engine);
    assert_eq!(report.dated, 1, "the merge commit named the file: {report:?}");
    let s = sections(&engine, "doc.md");
    assert_eq!((s[0].1, s[0].2), (ts(T3), TimeSource::Git));
}

#[test]
fn a_rename_is_followed_for_the_created_time() {
    let (_tmp, dir) = repo();
    write(
        &dir,
        "old-name.md",
        "# Doc\n\nA paragraph long enough for git to call it a rename.\n\nMore text.\n",
    );
    commit_at(&dir, T1, "add");
    git(&dir, &["mv", "old-name.md", "new-name.md"]);
    commit_at(&dir, T2, "rename");
    let mut engine = index(&dir);
    date(&mut engine);
    let d = doc(&engine, "new-name.md");
    assert_eq!((d.created_at, d.updated_at), (ts(T1), ts(T2)));
    assert_eq!(
        sections(&engine, "new-name.md")[0].1,
        ts(T1),
        "the lines are from the first commit"
    );
}

#[test]
fn a_root_below_the_top_level_and_awkward_names_work() {
    let (_tmp, dir) = repo();
    write(&dir, "docs/space and ünïcode.md", "# Odd\n\nText.\n");
    write(&dir, "docs/sub/deep.md", "# Deep\n\nText.\n");
    write(&dir, "outside.md", "# Outside\n\nNot under the root.\n");
    commit_at(&dir, T1, "add");
    let root = dir.join("docs");
    let mut engine = index(&root);
    let report = date(&mut engine);
    assert_eq!((report.dated, report.ineligible, report.failed), (2, 0, 0), "{report:?}");
    assert_eq!(doc(&engine, "space and ünïcode.md").updated_at, ts(T1));
    assert_eq!(doc(&engine, "sub/deep.md").created_source, TimeSource::Git);

    // A later commit outside the root marks nothing under it.
    write(&dir, "outside.md", "# Outside\n\nChanged.\n");
    commit_at(&dir, T2, "outside");
    let report = date(&mut engine);
    assert_eq!((report.dated, report.remaining), (0, 0), "{report:?}");
}

#[test]
fn carriage_returns_and_filtered_files_fall_back() {
    let (_tmp, dir) = repo();
    write(&dir, "crlf.md", "# CRLF\r\n\r\nText.\r\n");
    write(&dir, "lone-cr.md", "# Lone\n\nA\rB\n");
    write(&dir, "plain.md", "# Plain\n\nText.\n");
    commit_at(&dir, T1, "add");
    let mut engine = index(&dir);
    let report = date(&mut engine);
    assert_eq!((report.dated, report.ineligible), (1, 2), "{report:?}");
    assert_eq!(doc(&engine, "crlf.md").git_state, GitState::Filesystem);
    assert_eq!(doc(&engine, "lone-cr.md").git_state, GitState::Filesystem);
}

#[test]
fn duplicate_sections_front_matter_and_a_missing_final_newline_are_dated_per_occurrence() {
    let (_tmp, dir) = repo();
    let first = "---\ntitle: T\n---\n\n# Doc\n\n## Notes\n\nSame.\n\n## Other\n\nText.";
    write(&dir, "doc.md", first);
    commit_at(&dir, T1, "add");
    // A second, identical "Notes" section appended later, still no final newline.
    write(&dir, "doc.md", &format!("{first}\n\n## Notes\n\nSame."));
    commit_at(&dir, T2, "duplicate");
    let mut engine = index(&dir);
    let report = date(&mut engine);
    assert_eq!((report.dated, report.failed), (1, 0), "{report:?}");
    let s = sections(&engine, "doc.md");
    let notes: Vec<Timestamp> = s.iter().filter(|x| x.0 == "Notes").map(|x| x.1).collect();
    assert_eq!(notes, vec![ts(T1), ts(T2)], "each occurrence has its own time: {s:?}");

    // Editing another section while duplicates exist: the duplicates cannot be told apart,
    // so both fall back to file time; the unique untouched section keeps git.
    write(&dir, "doc.md", &format!("{first}!\n\n## Notes\n\nSame."));
    engine.index_root().unwrap();
    let s = sections(&engine, "doc.md");
    assert!(s.iter().filter(|x| x.0 == "Notes").all(|x| x.2 == TimeSource::Filesystem), "{s:?}");
    assert_eq!(s.iter().find(|x| x.0 == "Doc").map(|x| x.2), Some(TimeSource::Git), "{s:?}");
}

#[test]
fn an_author_date_after_the_committer_date_or_in_the_future_is_capped() {
    let (_tmp, dir) = repo();
    write(&dir, "late.md", "# Late\n\nText.\n");
    commit_with(&dir, T3, T1, "authored after it was committed");
    write(&dir, "future.md", "# Future\n\nText.\n");
    commit_with(&dir, "2099-01-01T00:00:00Z", "2099-01-01T00:00:00Z", "from the future");
    let mut engine = index(&dir);
    let started = Timestamp::now();
    date(&mut engine);
    assert_eq!(doc(&engine, "late.md").updated_at, ts(T1));
    let future = doc(&engine, "future.md").updated_at;
    assert!(
        future <= Timestamp::now()
            && future >= started.checked_sub(jiff::SignedDuration::from_secs(5)).unwrap()
    );
}

#[test]
fn a_shallow_clone_keeps_filesystem_times() {
    let (_tmp, dir) = repo();
    write(&dir, "a.md", "# A\n\nOne.\n");
    commit_at(&dir, T1, "one");
    write(&dir, "a.md", "# A\n\nTwo.\n");
    commit_at(&dir, T2, "two");
    let clone_tmp = tempfile::tempdir().unwrap();
    let clone = clone_tmp.path().canonicalize().unwrap().join("clone");
    let url = format!("file://{}", dir.display());
    git(&dir, &["clone", "-q", "--depth", "1", &url, clone.to_str().unwrap()]);
    let mut engine = index(&clone);
    let report = date(&mut engine);
    assert_eq!((report.source.as_str(), report.dated), ("shallow", 0));
    assert_eq!(doc(&engine, "a.md").updated_source, TimeSource::Filesystem);
}

#[test]
fn a_stale_result_is_discarded_when_the_document_changed_meanwhile() {
    let (_tmp, dir) = repo();
    write(&dir, "a.md", "# A\n\nOne.\n");
    commit_at(&dir, T1, "one");
    let mut engine = index(&dir);
    let d = doc(&engine, "a.md");
    let keys = engine.store().section_keys(&d.doc_id).unwrap();
    let dates = mda_core::store::GitDates {
        commit: "c".repeat(40),
        blob: "b".repeat(40),
        created_at: ts(T1),
        updated_at: ts(T1),
        section_updated_at: vec![ts(T1); keys.len()],
    };
    // Another writer re-indexes the file between the computation and the write.
    write(&dir, "a.md", "# A\n\nOne.\n\n## New\n\nMore.\n");
    engine.index_root().unwrap();
    let applied =
        engine.store_mut().apply_git_dates(&d.doc_id, &d.content_hash, &keys, &dates).unwrap();
    assert!(!applied, "the hash moved on: nothing is written");
    assert_eq!(doc(&engine, "a.md").updated_source, TimeSource::Filesystem);

    // Same hash but the document is no longer waiting: also refused.
    let d = doc(&engine, "a.md");
    let keys = engine.store().section_keys(&d.doc_id).unwrap();
    let dates = mda_core::store::GitDates { section_updated_at: vec![ts(T1); keys.len()], ..dates };
    assert!(engine.store_mut().apply_git_dates(&d.doc_id, &d.content_hash, &keys, &dates).unwrap());
    assert!(
        !engine.store_mut().apply_git_dates(&d.doc_id, &d.content_hash, &keys, &dates).unwrap()
    );
}

#[test]
fn limits_stop_a_pass_and_the_next_one_continues() {
    let (_tmp, dir) = repo();
    for i in 0..5 {
        write(&dir, &format!("f{i}.md"), &format!("# F{i}\n\nText.\n"));
    }
    commit_at(&dir, T1, "five");
    let mut engine = index(&dir);
    let limits = DateLimits { max_documents: Some(2), ..DateLimits::default() };
    let first = engine.date_from_git(limits).unwrap();
    assert_eq!((first.dated, first.remaining), (2, 3));
    assert!(first.progressed());
    let cancel = std::sync::atomic::AtomicBool::new(true);
    let none = engine
        .date_from_git(DateLimits { cancel: Some(&cancel), ..DateLimits::default() })
        .unwrap();
    assert_eq!((none.dated, none.remaining), (0, 3), "cancelled before the first document");
    assert_eq!(date(&mut engine).remaining, 0);
}

#[test]
fn time_source_filesystem_calls_no_git_and_restores_file_times() {
    let (_tmp, dir) = repo();
    write(&dir, "a.md", "# A\n\nOne.\n");
    commit_at(&dir, T1, "one");
    let mut engine = index(&dir);
    date(&mut engine);
    assert_eq!(doc(&engine, "a.md").updated_at, ts(T1));
    let mtime = doc(&engine, "a.md").file_mtime.unwrap();
    drop(engine);

    std::fs::write(dir.join(".markdownattractor/config.toml"), "time_source = \"filesystem\"\n")
        .unwrap();
    let mut engine = Engine::open(&dir).unwrap();
    assert!(!engine.git_times_enabled());
    assert_eq!(engine.git_head(), None);
    assert_eq!(date(&mut engine).source, "disabled");
    assert_eq!(engine.restore_filesystem_times().unwrap(), 1);
    let d = doc(&engine, "a.md");
    assert_eq!(
        (d.updated_at, d.updated_source, d.created_source),
        (mtime, TimeSource::Filesystem, TimeSource::Filesystem)
    );
    assert_eq!(d.created_at, d.first_seen_at);
    assert!(
        sections(&engine, "a.md").iter().all(|s| s.1 == mtime && s.2 == TimeSource::Filesystem)
    );
    assert_eq!(engine.restore_filesystem_times().unwrap(), 0, "idempotent");
}

#[test]
fn a_git_dated_document_is_not_reported_stale() {
    let (_tmp, dir) = repo();
    write(&dir, "a.md", "# A\n\nOne.\n");
    commit_at(&dir, T1, "one");
    let mut engine = index(&dir);
    date(&mut engine);
    // `updated_at` is now years before the file's modification time; the stale check must
    // compare against the file's own stored time, not the git time.
    assert!(doc(&engine, "a.md").updated_at < doc(&engine, "a.md").file_mtime.unwrap());
    let stale = engine.stale().unwrap();
    assert!(stale.iter().all(|s| !s.changed_on_disk), "{stale:?}");
    write(&dir, "a.md", "# A\n\nTwo.\n");
    let stale = engine.stale().unwrap();
    assert!(stale.iter().any(|s| s.rel_path == "a.md" && s.changed_on_disk), "{stale:?}");
}
