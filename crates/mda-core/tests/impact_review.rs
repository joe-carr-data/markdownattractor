//! A whole review (ADR-0009) against a real temporary repository and a scripted backend:
//! the snapshot, what is not reviewed, the judgments, the ledger, and what is withheld.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_panics_doc, clippy::panic)]

use std::path::Path;
use std::process::Command;

use mda_core::impact::judgment::Verdict;
use mda_core::impact::review::{Options, Report, pair_id, review};
use mda_core::pipeline::Engine;
use mda_core::store::{doc_id_for, section_id_for};
use mda_core::worker::{Mock, StructuredResult, StructuredStatus};
use serde_json::json;

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
    std::fs::write(dir.join(rel), text).unwrap();
}

const DEPLOY: &str =
    "# Deploy\n\nHow we ship.\n\n## Timeout\n\nThe gateway timeout is 30 seconds.\n";
const FAQ: &str = "# FAQ\n\n## Limits\n\nRequests end after 30 seconds at the gateway.\n";
const NOTES: &str = "# Notes\n\n## Gateway\n\nThe gateway sits in front of the API.\n";

/// A repository with three committed pages, then the deploy page's timeout changed to 90.
fn changed_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    write(root, "deploy.md", DEPLOY);
    write(root, "faq.md", FAQ);
    write(root, "notes.md", NOTES);
    write(root, ".gitignore", ".markdownattractor/\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "first"]);
    write(root, "deploy.md", &DEPLOY.replace("30 seconds", "90 seconds"));
    dir
}

fn id_for(candidate_path: &str) -> String {
    // Each page: section 0 is the title, section 1 the second-level heading.
    pair_id(
        "deploy.md",
        &["Deploy".into(), "Timeout".into()],
        &section_id_for(&doc_id_for(candidate_path), 1),
    )
}

fn outdated() -> StructuredResult {
    Mock::structured_ok(
        json!({"verdict": "possibly_outdated", "before_quote": "timeout is 30 seconds",
               "after_quote": "timeout is 90 seconds", "candidate_quote": "end after 30 seconds",
               "reason": "The FAQ gives 30 seconds; the deploy page gives 90."}),
        "m",
    )
}

fn verdict(v: &str) -> StructuredResult {
    Mock::structured_ok(
        json!({"verdict": v, "before_quote": "", "after_quote": "", "candidate_quote": "", "reason": "x"}),
        "m",
    )
}

async fn run(root: &Path, backend: Option<&Mock>, opts: &Options) -> Report {
    let mut engine = Engine::open(root).unwrap();
    review(&mut engine, backend.map(|b| b as &dyn mda_core::worker::Backend), opts).await.unwrap()
}

#[tokio::test]
async fn a_changed_value_finds_the_stale_copy_with_both_passages_from_the_files() {
    let dir = changed_repo();
    let root = dir.path().canonicalize().unwrap();
    let mock = Mock::new()
        .on_structured(id_for("faq.md"), vec![outdated()])
        .on_structured(id_for("notes.md"), vec![verdict("related")]);
    let report = run(&root, Some(&mock), &Options::new("m")).await;

    assert_eq!((report.files_changed, report.changes, report.candidates), (1, 1, 2));
    assert_eq!((report.judged, report.not_judged, report.failed), (2, 0, 0));
    assert_eq!(report.verdicts["possibly_outdated"], 1);
    assert_eq!(report.verdicts["related"], 1);
    let [f] = report.findings.as_slice() else { panic!("{:?}", report.findings) };
    assert_eq!(f.verdict, Verdict::PossiblyOutdated);
    assert_eq!(
        (f.change.rel_path.as_str(), f.candidate.rel_path.as_str()),
        ("deploy.md", "faq.md")
    );
    assert_eq!(f.before.as_ref().unwrap().text, "timeout is 30 seconds");
    assert_eq!((f.after.text.as_str(), f.after.line_start), ("timeout is 90 seconds", 7));
    assert_eq!(
        (f.candidate_quote.text.as_str(), f.candidate_quote.line_start),
        ("end after 30 seconds", 5)
    );
    assert_eq!(report.pairs.len(), 2);

    // Every attempt is in the ledger, under an outcome that names the review.
    let engine = Engine::open(&root).unwrap();
    let days = engine.store().usage_by_day(None).unwrap();
    assert!(!days.is_empty() && days.iter().all(|d| d.outcome == "impact_ok"), "{days:?}");
    assert!(report.usage.input_tokens > 0);
    // The request carried no instruction from the files and both documents' context.
    let sent = mock.structured_calls();
    assert_eq!(sent.len(), 2);
    assert!(sent[0].user.contains("How we ship.") && sent[0].user.contains("faq.md"));
    // Nothing was written into the watched files.
    assert_eq!(std::fs::read_to_string(root.join("faq.md")).unwrap(), FAQ);
}

#[tokio::test]
async fn without_a_backend_candidates_are_listed_and_nothing_is_called_an_alert() {
    let dir = changed_repo();
    let root = dir.path().canonicalize().unwrap();
    let report = run(&root, None, &Options::new("m")).await;
    assert_eq!((report.candidates, report.judged, report.not_judged), (2, 0, 2));
    assert!(report.findings.is_empty() && report.backend.is_none());
    assert!(
        report.pairs.iter().all(|p| p.outcome == "not_judged"
            && p.detail.as_deref() == Some("no summarization backend"))
    );
}

#[tokio::test]
async fn the_cap_a_failing_call_and_a_stopped_backend_are_each_counted_apart() {
    let dir = changed_repo();
    let root = dir.path().canonicalize().unwrap();

    // The cap: one judgment, the other pair is listed as not judged.
    let mock = Mock::new()
        .on_structured(id_for("faq.md"), vec![outdated()])
        .on_structured(id_for("notes.md"), vec![verdict("related")]);
    let capped = Options { max_judgments: 1, ..Options::new("m") };
    let report = run(&root, Some(&mock), &capped).await;
    assert_eq!((report.judged, report.not_judged), (1, 1));
    assert_eq!(mock.structured_calls().len(), 1);
    assert!(report.stopped.is_none(), "a cap is not a stopped run");

    // A malformed answer is retried once; a second failure is a failed pair, not a verdict.
    let bad = StructuredResult {
        status: StructuredStatus::Malformed { reason: "not JSON".into(), raw: "…".into() },
        usage: None,
    };
    let mock = Mock::new()
        .on_structured(id_for("faq.md"), vec![bad.clone(), outdated()])
        .on_structured(id_for("notes.md"), vec![bad]);
    let report = run(&root, Some(&mock), &Options::new("m")).await;
    assert_eq!((report.judged, report.failed, report.findings.len()), (1, 1, 1));
    assert_eq!(mock.structured_calls().len(), 4, "two attempts for each pair");
    assert_eq!(report.usage_unknown, 3, "the attempts that reported no usage");

    // An unscripted mock is fatal: the run stops and says so; nothing is a finding.
    let report = run(&root, Some(&Mock::new()), &Options::new("m")).await;
    assert_eq!((report.judged, report.failed, report.not_judged), (0, 1, 1));
    assert!(report.stopped.as_deref().is_some_and(|s| s.starts_with("the backend stopped")));
}

#[tokio::test]
async fn an_alert_whose_quotes_are_not_in_the_files_is_dropped_and_counted() {
    let dir = changed_repo();
    let root = dir.path().canonicalize().unwrap();
    let invented = Mock::structured_ok(
        json!({"verdict": "possibly_outdated", "before_quote": "timeout is 30 seconds",
               "after_quote": "timeout is 90 seconds", "candidate_quote": "ends after 45 seconds",
               "reason": "x"}),
        "m",
    );
    let mock = Mock::new()
        .on_structured(id_for("faq.md"), vec![invented])
        .on_structured(id_for("notes.md"), vec![verdict("abstain")]);
    let report = run(&root, Some(&mock), &Options::new("m")).await;
    assert!(report.findings.is_empty());
    assert_eq!((report.dropped_unquoted, report.verdicts["abstain"]), (1, 1));
    let dropped = report.pairs.iter().find(|p| p.outcome == "dropped_unquoted").unwrap();
    assert!(dropped.detail.as_deref().unwrap().contains("candidate_quote: not in the source"));
}

#[tokio::test]
async fn new_deleted_and_renamed_things_are_listed_as_not_reviewed_never_as_clean() {
    let dir = changed_repo();
    let root = dir.path().canonicalize().unwrap();
    write(&root, "new.md", "# New\n\nA page about the gateway and 30 seconds.\n");
    std::fs::remove_file(root.join("notes.md")).unwrap();
    write(&root, "faq.md", &FAQ.replace("## Limits", "## Ceilings"));
    let report = run(&root, None, &Options::new("m")).await;
    let reasons: Vec<(&str, &str)> =
        report.not_reviewed.iter().map(|n| (n.rel_path.as_str(), n.reason.as_str())).collect();
    assert!(reasons.contains(&("new.md", "new_file")), "{reasons:?}");
    assert!(reasons.contains(&("notes.md", "deleted_file")), "{reasons:?}");
    assert!(reasons.contains(&("faq.md", "renamed_or_moved")), "{reasons:?}");
    assert_eq!(report.files_changed, 4);
    assert_eq!(report.changes, 1, "only the deploy page's section is a change");
}

#[tokio::test]
async fn a_revision_that_is_not_an_ancestor_and_a_folder_without_git_are_refused() {
    let dir = changed_repo();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["checkout", "-q", "-b", "side"]);
    write(&root, "side.md", "# Side\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "side"]);
    git(&root, &["checkout", "-q", "main"]);
    let mut engine = Engine::open(&root).unwrap();
    let opts = Options { since: "side".into(), ..Options::new("m") };
    let err = review(&mut engine, None, &opts).await.unwrap_err().to_string();
    assert!(err.contains("not an ancestor of HEAD"), "{err}");
    let opts = Options { since: "no-such-rev".into(), ..Options::new("m") };
    assert!(review(&mut engine, None, &opts).await.is_err());

    let plain = tempfile::tempdir().unwrap();
    write(plain.path(), "a.md", "# A\n");
    let mut engine = Engine::open(plain.path()).unwrap();
    let err = review(&mut engine, None, &Options::new("m")).await.unwrap_err().to_string();
    assert!(err.contains("not usable here"), "{err}");
}
