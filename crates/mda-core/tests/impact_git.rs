//! The git side of a change-impact review (ADR-0009): a revision resolved once, the files
//! that differ from it, and their earlier text. Real temporary repositories; needs `git`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_panics_doc, clippy::panic)]

use std::path::Path;
use std::process::Command;

use mda_core::gittime::{self, Discovery, FileChange, Repo};
use mda_core::impact::{Skip, compare};
use mda_core::markdown::Flavor;

fn git(dir: &Path, args: &[&str]) -> String {
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
    cmd.args(args);
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(dir: &Path, msg: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

fn repo(root: &Path) -> Repo {
    match gittime::discover(root) {
        Discovery::Usable(r) => r,
        other => panic!("not usable: {}", other.name()),
    }
}

fn working(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

fn md(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

const GUIDE: &str =
    "# Guide\n\n## Timeouts\n\nThe default timeout is 30 seconds.\n\n## Auth\n\nUse a token.\n";

/// A repository with `docs/` as the watched root and one commit.
fn fixture() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    write(dir.path(), "docs/guide.md", GUIDE);
    write(dir.path(), "docs/old.md", "# Old\n\nGoing away.\n");
    write(dir.path(), "outside.md", "# Outside the root\n");
    let first = commit(dir.path(), "first");
    (dir, first)
}

#[test]
fn a_revision_resolves_to_a_commit_id_and_option_like_or_unknown_text_is_refused() {
    let (dir, first) = fixture();
    let r = repo(&dir.path().join("docs"));
    assert_eq!(r.resolve_commit("HEAD").unwrap(), first);
    assert_eq!(r.resolve_commit(&first[..10]).unwrap(), first);
    assert_eq!(r.resolve_commit(" main ").unwrap(), first);
    for bad in ["", "--output=/tmp/x", "-n", "no-such-branch", "HEAD\n--all", "HEAD~7"] {
        assert!(r.resolve_commit(bad).is_err(), "{bad:?} must not resolve");
    }
    // A tree or blob id is not a commit.
    let tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
    assert!(r.resolve_commit(&tree).is_err());
}

#[test]
fn changes_since_a_commit_cover_committed_and_uncommitted_edits_under_the_root_only() {
    let (dir, first) = fixture();
    let docs = dir.path().join("docs");
    write(dir.path(), "docs/guide.md", &GUIDE.replace("30 seconds", "45 seconds"));
    commit(dir.path(), "second");
    // Uncommitted on top: another edit, a deletion, a staged new file, an untracked file, and
    // an edit outside the root.
    write(dir.path(), "docs/guide.md", &GUIDE.replace("30 seconds", "60 seconds"));
    std::fs::remove_file(docs.join("old.md")).unwrap();
    write(dir.path(), "docs/new.md", "# New\n");
    git(dir.path(), &["add", "docs/new.md"]);
    write(dir.path(), "docs/untracked.md", "# Untracked\n");
    write(dir.path(), "outside.md", "# Outside, edited\n");

    let r = repo(&docs);
    let changed =
        r.changed_since(&first, &working(&["guide.md", "new.md", "untracked.md"]), &md).unwrap();
    assert_eq!(
        changed,
        [
            (FileChange::Modified, "guide.md".to_owned()),
            (FileChange::Added, "new.md".to_owned()),
            (FileChange::Deleted, "old.md".to_owned()),
            (FileChange::Added, "untracked.md".to_owned()),
        ],
        "root-relative, sorted, nothing from outside the root; a file git never saw is an addition"
    );
    // An unchanged file is not listed, and a file `wanted` rejects is not a deletion.
    let none = r.changed_since(&first, &working(&["guide.md", "new.md"]), &|_| false).unwrap();
    assert!(!none.iter().any(|(k, _)| *k == FileChange::Deleted), "{none:?}");
    assert!(r.changed_since(&first, &working(&["a\nb.md"]), &md).is_err(), "a newline in a path");

    // The earlier text comes from the pinned commit, not from HEAD or the index.
    let tree = r.tree(&first).unwrap();
    let before = String::from_utf8(r.blob(&tree["guide.md"]).unwrap()).unwrap();
    assert_eq!(before, GUIDE);
    let after = std::fs::read_to_string(docs.join("guide.md")).unwrap();
    let review = compare(&before, &after, Flavor::Markdown);
    assert_eq!(review.changes.len(), 1);
    assert_eq!(review.changes[0].removed_lines, ["The default timeout is 30 seconds."]);
    assert_eq!(review.changes[0].added_lines, ["The default timeout is 60 seconds."]);
    assert!(r.blob("not-hex").is_err());
}

#[test]
fn a_crlf_working_file_against_an_lf_blob_is_no_change_and_a_rename_is_not_paired() {
    let (dir, first) = fixture();
    let docs = dir.path().join("docs");
    let r = repo(&docs);
    let before = String::from_utf8(r.blob(&r.tree(&first).unwrap()["guide.md"]).unwrap()).unwrap();

    write(dir.path(), "docs/guide.md", &GUIDE.replace('\n', "\r\n"));
    let after = std::fs::read_to_string(docs.join("guide.md")).unwrap();
    let review = compare(&before, &after, Flavor::Markdown);
    assert!(review.changes.is_empty() && review.not_reviewed.is_empty(), "{review:?}");

    // A renamed file is a deletion and an addition: no earlier text is attributed to it.
    git(dir.path(), &["mv", "docs/old.md", "docs/renamed.md"]);
    let changed = r.changed_since(&first, &working(&["guide.md", "renamed.md"]), &md).unwrap();
    assert!(
        changed.contains(&(FileChange::Modified, "guide.md".to_owned())),
        "the bytes differ (CRLF), so the file is listed; the section comparison found no change in it"
    );
    assert!(changed.contains(&(FileChange::Deleted, "old.md".to_owned())));
    assert!(changed.contains(&(FileChange::Added, "renamed.md".to_owned())));

    // A renamed heading inside a file is reported, never treated as new text.
    let review = compare(GUIDE, &GUIDE.replace("## Auth", "## Authentication"), Flavor::Markdown);
    assert!(review.changes.is_empty());
    assert!(review.not_reviewed.iter().any(|n| n.reason == Skip::RenamedOrMoved));
}

#[cfg(unix)]
#[test]
fn a_program_named_by_the_repository_configuration_is_never_run() {
    let (dir, first) = fixture();
    let marker = dir.path().join("executed");
    let hook = format!("touch '{}'; cat", marker.display());
    // Every way a read could start a configured program: the fsmonitor hook, a clean filter
    // and a process filter on the file, a textconv driver, a pager, an external diff.
    write(dir.path(), ".gitattributes", "*.md filter=review diff=review\n");
    for key in [
        "core.fsmonitor",
        "filter.review.clean",
        "filter.review.smudge",
        "filter.review.process",
        "diff.review.textconv",
        "diff.review.command",
        "core.pager",
        "diff.external",
    ] {
        git(dir.path(), &["config", key, &hook]);
    }
    write(dir.path(), "docs/guide.md", &GUIDE.replace("30 seconds", "60 seconds"));
    assert!(!marker.exists(), "the fixture itself must not have run the hook");

    let r = repo(&dir.path().join("docs"));
    let changed = r.changed_since(&first, &working(&["guide.md", "old.md"]), &md).unwrap();
    assert_eq!(changed, [(FileChange::Modified, "guide.md".to_owned())]);
    let tree = r.tree(&first).unwrap();
    r.blob(&tree["guide.md"]).unwrap();
    r.resolve_commit("HEAD").unwrap();
    r.blob_of(b"x").unwrap();
    // The dating calls of ADR-0008 as well: history and blame.
    let now = jiff::Timestamp::now();
    r.document_times(&first, "guide.md", now).unwrap();
    r.line_times(&first, "guide.md", now).unwrap();
    assert!(!marker.exists(), "a configured hook, filter, textconv, pager or diff was started");
}

#[test]
fn revision_syntax_that_searches_or_stands_in_for_the_commit_check_is_refused() {
    let (dir, first) = fixture();
    // A tracked file whose name ends like the peel suffix.
    write(dir.path(), "docs/x^{commit}", "not a commit\n");
    commit(dir.path(), "odd name");
    let r = repo(&dir.path().join("docs"));
    for bad in [":/first", "HEAD^{/first}", "HEAD:docs/x", &"a".repeat(201)] {
        assert!(r.resolve_commit(bad).is_err(), "{bad:?} must not resolve");
    }
    assert_eq!(r.resolve_commit(&first).unwrap(), first);
    assert_eq!(r.resolve_commit("HEAD~1").unwrap(), first);
}

#[cfg(unix)] // a `file://` URL built from a path
#[test]
fn a_missing_object_in_a_partial_clone_is_an_error_and_nothing_is_fetched() {
    let (src, _) = fixture();
    git(src.path(), &["config", "uploadpack.allowFilter", "true"]);
    let dst = tempfile::tempdir().unwrap();
    let url = format!("file://{}", src.path().display());
    let out = Command::new("git")
        .args(["clone", "-q", "--no-checkout", "--filter=blob:none", &url])
        .arg(dst.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let missing = |dir: &Path| {
        git(dir, &["rev-list", "--objects", "--missing=print", "HEAD"])
            .lines()
            .filter(|l| l.starts_with('?'))
            .count()
    };
    let before = missing(dst.path());
    if before == 0 {
        return; // this git did not make a partial clone over file://; nothing to prove
    }
    // Discovery from the top (no checkout, so `docs/` does not exist on disk).
    let r = repo(dst.path());
    let head = r.head().unwrap();
    let tree = r.tree(&head).unwrap();
    assert!(
        r.blob(&tree["docs/guide.md"]).is_err(),
        "the blob is not here and must not be fetched"
    );
    assert_eq!(missing(dst.path()), before, "no object arrived");
}
