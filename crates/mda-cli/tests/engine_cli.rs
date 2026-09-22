//! End-to-end tests of `mda index | search | open | card | status` on a temporary root.
//! No model is involved: `--no-summarize` keeps everything deterministic.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_panics_doc)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

fn mda() -> Command {
    let mut c = Command::cargo_bin("mda").expect("binary builds");
    c.env("NO_COLOR", "1");
    c
}

fn root_with_docs() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "docs/runbook.md",
        "# Runbook\n\n## Rollback\n\nRun deployctl rollback --to <sha>.\n\n## Contacts\n\nPage the on-call.\n",
    );
    write(
        dir.path(),
        "notes/adr.md",
        "---\nstatus: current\n---\n# ADR 7\n\nWe chose blue-green in March 2026.\n",
    );
    write(dir.path(), "ignored.txt", "not markdown");
    dir
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

#[test]
fn index_then_status_search_open_card() {
    let dir = root_with_docs();
    let root = dir.path();

    // index (raw only)
    mda()
        .args(["index", "--no-summarize", "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("indexed 2 file(s)"))
        .stdout(predicate::str::contains("4 section(s) need a card"))
        .stdout(predicate::str::contains("pending"));
    assert!(root.join(".markdownattractor/index.sqlite").exists());

    // status
    let out = mda()
        .args(["--json", "status", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    assert_eq!(v["counts"]["docs"], 2);
    assert_eq!(v["counts"]["sections"], 4);
    assert_eq!(v["counts"]["pending"], 4);
    assert_eq!(v["config"]["summarization_model"], "claude-haiku-4-5");

    // search (raw text, pending)
    let out = mda()
        .args(["--json", "search", "deployctl", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    let hits = v["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["rel_path"], "docs/runbook.md");
    assert_eq!(hits[0]["heading_path"], serde_json::json!(["Runbook", "Rollback"]));
    assert_eq!(hits[0]["pending"], true);
    assert_eq!(hits[0]["matched"], "raw");
    let id = hits[0]["section_id"].as_str().unwrap().to_owned();

    // human search output shows the next command
    mda()
        .args(["search", "deployctl", "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("Runbook › Rollback"))
        .stdout(predicate::str::contains("(pending)"))
        .stdout(predicate::str::contains(format!("mda open {id}")));

    // open: exact lines
    let out = mda()
        .args(["--json", "open", &id, "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    assert_eq!(v["line_start"], 3);
    assert_eq!(v["line_end"], 5);
    assert_eq!(v["stale"], false);
    assert_eq!(v["text"], "## Rollback\n\nRun deployctl rollback --to <sha>.");
    mda()
        .args(["open", &id, "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("3 │ ## Rollback"))
        .stdout(predicate::str::contains("5 │ Run deployctl"));

    // card: pending
    mda()
        .args(["card", &id, "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("no card yet (pending)"));
    let out = mda()
        .args(["--json", "card", &id, "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    assert_eq!(v["state"], "pending");
    assert!(v["summary"].is_null());
}

#[test]
fn index_accepts_mdx_and_titles_it_from_front_matter() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "guides/tabs.mdx",
        "---\ntitle: Tabs guide\n---\n\nimport Tabs from '@theme/Tabs';\n\n<Tabs>\n<TabItem value=\"a\">\n## Install with pnpm\n\nRun pnpm add deployctl.\n</TabItem>\n</Tabs>\n",
    );
    mda()
        .args(["index", "--no-summarize", "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("indexed 1 file(s)"));
    let out = mda()
        .args(["--json", "search", "pnpm", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let hits = json_of(&out)["hits"].as_array().unwrap().clone();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0]["rel_path"], "guides/tabs.mdx");
    assert_eq!(hits[0]["heading_path"], serde_json::json!(["Install with pnpm"]));
    assert_eq!(hits[0]["line_start"], 9);
    assert_eq!(
        hits[0]["snippet"], "Run pnpm add deployctl.",
        "tag-only lines stay out of the snippet"
    );
    let out = mda()
        .args(["--json", "recent", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let recent = json_of(&out);
    assert_eq!(recent["docs"][0]["title"], "Tabs guide", "{recent}");
}

#[test]
fn open_reports_stale_after_edit() {
    let dir = root_with_docs();
    let root = dir.path();
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();
    let out = mda()
        .args(["--json", "search", "on-call", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let id = json_of(&out)["hits"][0]["section_id"].as_str().unwrap().to_owned();

    write(
        root,
        "docs/runbook.md",
        "# Runbook\n\nintro\n\n## Rollback\n\nRun deployctl rollback --to <sha>.\n\n## Contacts\n\nPage the on-call.\nThen escalate.\n",
    );
    let out = mda()
        .args(["--json", "open", &id, "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    assert_eq!(v["stale"], true);
    assert_eq!(v["text"], "## Contacts\n\nPage the on-call.\nThen escalate.");
    // Opening re-indexed the file as a side effect, so the next open is fresh again.
    mda()
        .args(["open", &id, "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("Then escalate"))
        .stdout(predicate::str::contains("file changed since indexing").not());
}

#[test]
fn search_filters_and_no_hits() {
    let dir = root_with_docs();
    let root = dir.path();
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();

    mda()
        .args(["search", "zzzz-nothing-here", "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("no hits"));

    let out = mda()
        .args(["--json", "search", "blue-green", "--in", "notes", "--since", "1d", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_of(&out)["hits"].as_array().unwrap().len(), 1);

    let out = mda()
        .args(["--json", "search", "blue-green", "--in", "docs", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_of(&out)["hits"].as_array().unwrap().len(), 0, "path filter excludes notes/");

    mda()
        .args(["search", "x", "--since", "nonsense", "--root"])
        .arg(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot parse"));
}

#[test]
fn index_single_file_and_reindex_is_idempotent() {
    let dir = root_with_docs();
    let root = dir.path();
    let file = root.join("notes/adr.md");
    mda()
        .args(["index", "--no-summarize", "--root"])
        .arg(root)
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("indexed 1 file(s)"));
    mda()
        .args(["index", "--no-summarize", "--root"])
        .arg(root)
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("0 changed"))
        .stdout(predicate::str::contains("0 section(s) need a card"));
}

#[test]
fn index_tombstones_deleted_files() {
    let dir = root_with_docs();
    let root = dir.path();
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();
    std::fs::remove_file(root.join("notes/adr.md")).unwrap();
    mda()
        .args(["index", "--no-summarize", "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("1 tombstoned"));
    let out = mda()
        .args(["--json", "status", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_of(&out)["counts"]["tombstoned"], 1);
}

#[test]
fn unknown_section_id_is_a_clean_error() {
    let dir = root_with_docs();
    let root = dir.path();
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();
    mda()
        .args(["open", "nope#9", "--root"])
        .arg(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
    mda()
        .args(["card", "nope#9", "--root"])
        .arg(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no section"));
}

#[test]
fn root_is_discovered_from_a_subdirectory() {
    let dir = root_with_docs();
    let root = dir.path();
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();
    // Run from docs/ with no --root: the nearest ancestor with .markdownattractor/ wins.
    let out = mda()
        .current_dir(root.join("docs"))
        .args(["--json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_of(&out)["counts"]["docs"], 2);
}

#[test]
fn backend_command_switches_and_guards_claude_cli() {
    let dir = root_with_docs();
    let root = dir.path();
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();

    // default
    let out = mda()
        .args(["--json", "backend", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_of(&out)["backend"], "api");

    // switch to local, persisted in config.toml
    mda()
        .args(["backend", "local", "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("backend set to"))
        .stdout(predicate::str::contains("local"));
    let cfg = std::fs::read_to_string(root.join(".markdownattractor/config.toml")).unwrap();
    assert!(cfg.contains("backend = \"local\""), "{cfg}");

    // claude-cli needs the acknowledgement
    mda()
        .args(["backend", "claude-cli", "--root"])
        .arg(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("claude_cli_policy_ack"));
    mda()
        .args(["backend", "claude-cli", "--i-accept-the-policy", "--root"])
        .arg(root)
        .assert()
        .success();
    let cfg = std::fs::read_to_string(root.join(".markdownattractor/config.toml")).unwrap();
    assert!(cfg.contains("claude_cli_policy_ack = true"), "{cfg}");

    // and can switch back even though the config is in the acknowledged claude-cli state
    mda().args(["backend", "api", "--root"]).arg(root).assert().success();
    let cfg = std::fs::read_to_string(root.join(".markdownattractor/config.toml")).unwrap();
    assert!(cfg.contains("backend = \"api\""));
    assert!(cfg.contains("claude_cli_policy_ack = false"));
}

#[test]
fn doctor_reports_missing_api_key() {
    let dir = root_with_docs();
    let root = dir.path();
    let out = mda()
        .args(["--json", "doctor", "--root"])
        .arg(root)
        .env_remove("ANTHROPIC_API_KEY")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    let backend = v["checks"].as_array().unwrap().iter().find(|c| c["name"] == "backend").unwrap();
    assert_eq!(backend["status"], "fail");
    assert!(backend["detail"].as_str().unwrap().contains("ANTHROPIC_API_KEY"));
}

#[test]
fn index_accepts_a_directory_as_the_root() {
    let dir = root_with_docs();
    let root = dir.path();
    mda()
        .args(["index", "--no-summarize"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("indexed 2 file(s)"));
    assert!(root.join(".markdownattractor/index.sqlite").exists());
    mda()
        .args(["index", "--no-summarize", "--root"])
        .arg(root)
        .arg(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("not both"));
}

#[test]
fn cost_reads_the_ledger_and_honours_since() {
    let dir = root_with_docs();
    let root = dir.path();
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();
    let out = mda()
        .args(["--json", "cost", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    assert_eq!(v["window"]["calls"], 0);
    assert_eq!(v["rows"].as_array().unwrap().len(), 0);
    assert!(v["tokens_saved"].is_null());
    mda()
        .args(["cost", "--since", "7d", "--root"])
        .arg(root)
        .env("NO_COLOR", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("since 7d"))
        .stdout(predicate::str::contains("nothing in the ledger"))
        .stdout(predicate::str::contains("not measured yet"));
    mda().args(["cost", "--since", "nonsense", "--root"]).arg(root).assert().failure();
}

#[test]
fn nudge_switches_the_root_and_the_global_marker() {
    let dir = root_with_docs();
    let root = dir.path();
    let marker = root.join("data").join("nudge.off");
    let mut cmd = mda();
    cmd.env("MDA_NUDGE_FILE", &marker).env("NO_COLOR", "1");
    let out = cmd
        .args(["--json", "nudge", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    assert_eq!(v["nudge"], true);
    assert_eq!(v["global_off"], false);
    assert_eq!(v["effective"], true);
    assert_eq!(v["global_marker"], marker.to_str().unwrap());

    // Per root: config.toml carries it.
    mda()
        .env("MDA_NUDGE_FILE", &marker)
        .env("NO_COLOR", "1")
        .args(["nudge", "off", "--root"])
        .arg(root)
        .assert()
        .success()
        .stdout(predicate::str::contains("nudge set for this root: off"));
    let cfg = std::fs::read_to_string(root.join(".markdownattractor/config.toml")).unwrap();
    assert!(cfg.contains("nudge = false"), "{cfg}");
    assert!(!marker.exists());

    // Global: the marker file.
    let out = mda()
        .env("MDA_NUDGE_FILE", &marker)
        .args(["--json", "nudge", "off", "--global", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&out);
    assert!(marker.is_file());
    assert_eq!(v["global_off"], true);
    assert_eq!(v["effective"], false);
    mda()
        .env("MDA_NUDGE_FILE", &marker)
        .args(["nudge", "on", "--global", "--root"])
        .arg(root)
        .assert()
        .success();
    assert!(!marker.exists());
    let out = mda()
        .env("MDA_NUDGE_FILE", &marker)
        .args(["--json", "nudge", "on", "--root"])
        .arg(root)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_of(&out)["effective"], true);
}

#[test]
fn diagnostics_bundle_is_redacted_and_writes_to_a_file() {
    let dir = root_with_docs();
    let root = dir.path();
    write(
        root,
        ".markdownattractor/config.toml",
        "api_workspace_id = \"wrkspc_secret\"\nembeddings = \"off\"\napi_base_url = \"https://bob:hunter2@api.example.com/v1\"\n",
    );
    mda().args(["index", "--no-summarize", "--root"]).arg(root).assert().success();
    // A planted "log" that is a symlink to a document must be ignored by the log tail.
    std::fs::create_dir_all(root.join(".markdownattractor/logs")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        root.join("docs/runbook.md"),
        root.join(".markdownattractor/logs/daemon.2099-01-01.log"),
    )
    .unwrap();
    let out = mda()
        .args(["diagnostics", "--root"])
        .arg(root)
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env_remove("ANTHROPIC_API_KEY")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out.clone()).unwrap();
    assert!(!text.contains("wrkspc_secret"), "workspace id redacted: {text}");
    assert!(!text.contains(root.to_str().unwrap()), "home redacted: {text}");
    let v = json_of(&out);
    assert_eq!(v["config"]["api_workspace_id_set"], true);
    assert!(v["config"].get("api_workspace_id").is_none(), "allowlisted view only");
    assert_eq!(
        v["config"]["api_base_url"], "https://api.example.com",
        "credentials and path dropped"
    );
    assert!(!text.contains("hunter2"), "URL credential redacted: {text}");
    assert_eq!(v["root"], "~");
    assert_eq!(v["store"]["counts"]["docs"], 2);
    assert!(v["doctor"].as_array().unwrap().iter().any(|c| c["name"] == "backend"));
    assert!(v["daemon"].is_null());
    assert_eq!(v["mda_version"], env!("CARGO_PKG_VERSION"));
    // The log tail comes from regular files only: a symlinked "log" pointing at a document
    // is skipped, and nothing of that document appears.
    assert!(!text.contains("deployctl"), "document content never leaks: {text}");

    let out_file = root.join("bundle.json");
    mda()
        .args(["diagnostics", "--out"])
        .arg(&out_file)
        .arg("--root")
        .arg(root)
        .env("NO_COLOR", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("bundle written"));
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out_file).unwrap()).unwrap();
    assert_eq!(v["os"], std::env::consts::OS);
    // Never overwrites (and therefore never truncates a symlink target).
    mda()
        .args(["diagnostics", "--out"])
        .arg(&out_file)
        .arg("--root")
        .arg(root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn index_of_a_subdirectory_uses_the_enclosing_root() {
    let dir = root_with_docs();
    let root = dir.path();
    write(root, ".markdownattractor/config.toml", "ignore = [\"notes/\"]\n");
    mda()
        .args(["index", "--no-summarize"])
        .arg(root.join("docs"))
        .env("NO_COLOR", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("is inside the indexed root"))
        .stdout(predicate::str::contains("indexed 1 file(s)")); // notes/ stays ignored
    assert!(!root.join("docs/.markdownattractor").exists(), "no second root was created");
}

/// A three-page DocsQA-shaped fixture: dataset files plus a "checkout" of the source repo.
fn docsqa_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("docsqa-data");
    let root = dir.path().join("checkout");
    write(
        &root,
        "docs/rollback.mdx",
        "---\ntitle: Rollback\n---\n\nimport X from 'x';\n\n## Roll back a deploy\n\nRun deployctl rollback --to the previous sha.\n",
    );
    write(
        &root,
        "docs/paging.mdx",
        "---\ntitle: Paging\n---\n\n## Who is paged\n\nPagerDuty pages the primary on-call for SEV1.\n",
    );
    write(
        &root,
        "docs/other.mdx",
        "---\ntitle: Other\n---\n\n## Unrelated\n\nNothing about incidents here.\n",
    );
    write(
        &data,
        "data/questions.jsonl",
        concat!(
            r#"{"question_id":"q1","project":"demo","query":"how do I roll back a deploy with deployctl","title":"t","question_modalities":["text"]}"#,
            "\n",
            r#"{"question_id":"q2","project":"demo","query":"who gets paged for a SEV1","title":"t","question_modalities":["text"]}"#,
            "\n",
            r#"{"question_id":"q3","project":"demo","query":"what does the screenshot show","title":"t","question_modalities":["text","image_derived_text"]}"#,
            "\n",
            r#"{"question_id":"q4","project":"demo","query":"a page we never indexed","title":"t","question_modalities":["text"]}"#,
            "\n",
            r#"{"question_id":"q9","project":"elsewhere","query":"not this project","title":"t","question_modalities":["text"]}"#,
            "\n",
        ),
    );
    write(
        &data,
        "data/answers.jsonl",
        concat!(
            r#"{"question_id":"q1","qrel_ids":["demo::/rollback","demo::/rollback"],"anchor_resolution":[{"doc_id":"demo::/rollback","canonical_anchor":"roll-back-a-deploy","canonical_heading":"Roll back a `deploy`"},{"doc_id":"demo::/rollback","canonical_anchor":"document"},{"doc_id":"demo::/rollback","canonical_anchor":"title","canonical_heading":"Rollback"}],"image_text_evidence_used":[],"requires_multimodal_judgment":false}"#,
            "\n",
            r#"{"question_id":"q2","qrel_ids":["demo::/paging"],"anchor_resolution":[{"doc_id":"demo::/paging","canonical_anchor":"gone","canonical_heading":"A heading that is not there"}],"image_text_evidence_used":false,"requires_multimodal_judgment":true}"#,
            "\n",
            r#"{"question_id":"q3","qrel_ids":["demo::/paging"],"image_text_evidence_used":[{"kind":"image_derived_text"}],"requires_multimodal_judgment":true}"#,
            "\n",
            r#"{"question_id":"q4","qrel_ids":["demo::/missing","demo::/nowhere"],"image_text_evidence_used":false,"requires_multimodal_judgment":false}"#,
            "\n",
            r#"{"question_id":"q9","qrel_ids":["else::/x"],"image_text_evidence_used":false,"requires_multimodal_judgment":false}"#,
            "\n",
        ),
    );
    write(
        &data,
        "data/corpus.jsonl",
        concat!(
            r#"{"doc_id":"demo::/rollback","project":"demo","repository_source_path":"docs/rollback.mdx","local_path":"docs/demo/docs/rollback.mdx"}"#,
            "\n",
            r#"{"doc_id":"demo::/paging","project":"demo","repository_source_path":"docs/paging.mdx","local_path":"docs/demo/docs/paging.mdx"}"#,
            "\n",
            r#"{"doc_id":"demo::/other","project":"demo","repository_source_path":"docs/other.mdx","local_path":"docs/demo/docs/other.mdx"}"#,
            "\n",
            r#"{"doc_id":"demo::/missing","project":"demo","repository_source_path":"docs/missing.mdx","local_path":"docs/demo/docs/missing.mdx"}"#,
            "\n",
            r#"{"doc_id":"else::/x","project":"elsewhere","repository_source_path":"x.md","local_path":"docs/elsewhere/x.md"}"#,
            "\n",
        ),
    );
    (dir, data, root)
}

#[test]
fn eval_docsqa_reports_coverage_split_and_page_metrics() {
    let (dir, data, root) = docsqa_fixture();
    // Not indexed yet: a clear error, no crash.
    mda()
        .args(["eval", "--dataset", "docsqa", "--data"])
        .arg(&data)
        .args(["--project", "demo", "--root"])
        .arg(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("not indexed yet"));
    mda().args(["index", "--no-summarize", "--root"]).arg(&root).assert().success();

    let out = dir.path().join("out");
    let stdout = mda()
        .args(["--json", "eval", "--dataset", "docsqa", "--data"])
        .arg(&data)
        .args(["--project", "demo", "--root"])
        .arg(&root)
        .args(["--split", "all", "--seed", "7", "--out"])
        .arg(&out)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&stdout);
    let cov = &v["coverage"];
    assert_eq!(cov["questions"], 4, "{cov}");
    assert_eq!(cov["corpus_pages"], 4);
    assert_eq!(cov["corpus_pages_indexed"], 3);
    assert_eq!(cov["qrels"], 5, "a duplicated label counts once");
    assert_eq!(cov["qrels_indexed"], 3);
    assert_eq!(cov["anchors"], 4, "{cov}");
    assert_eq!(cov["anchors_found"], 3, "q1's backticked heading, page and title anchors");
    assert_eq!(cov["questions_with_missing_anchor"], 1);
    assert_eq!(cov["missing_anchor_ids"], serde_json::json!(["q2"]));
    assert_eq!(cov["qrels_unmapped"], 1, "demo::/nowhere has no corpus row");
    assert_eq!(cov["excluded_image_evidence"], 1);
    assert_eq!(cov["excluded_missing_page"], 1);
    assert_eq!(cov["eligible"], 2);
    assert_eq!(cov["multimodal_judgment"], 1);
    assert!((cov["qrel_coverage"].as_f64().unwrap() - 0.6).abs() < 1e-9);
    let runs = v["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1, "raw only without cards: {runs:?}");
    let m = &runs[0]["metrics"];
    assert_eq!(m["questions"], 2);
    assert_eq!(m["success_at_5"], 1.0);
    assert_eq!(m["mrr_at_5"], 1.0);
    assert_eq!(m["ndcg_at_10"], 1.0);
    let results = runs[0]["results"].as_array().unwrap();
    assert_eq!(results[0]["id"], "q1");
    assert_eq!(results[0]["rank"], 1);
    assert_eq!(results[0]["top"][0], "docs/rollback.mdx");
    assert_eq!(results[0]["relevant"].as_array().unwrap().len(), 1, "deduplicated label");
    assert_eq!(results[0]["truncated"], false);
    let root_str = v["root"].as_str().unwrap();
    assert!(root_str.starts_with('~') || !root_str.contains("/Users/"), "{root_str}");
    assert!(
        out.join("coverage.json").is_file()
            && out.join("split.json").is_file()
            && out.join("results.json").is_file()
    );
    let split: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("split.json")).unwrap()).unwrap();
    assert_eq!(split["seed"], 7);
    assert_eq!(
        split["questions"].as_array().unwrap().len(),
        4,
        "every question of the project gets a split, eligible or not"
    );

    // A split that holds no question scores nothing and says so; the same seed gives the same split.
    let stdout2 = mda()
        .args(["--json", "eval", "--dataset", "docsqa", "--data"])
        .arg(&data)
        .args(["--project", "demo", "--root"])
        .arg(&root)
        .args(["--split", "all", "--seed", "7"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_of(&stdout2)["split_counts"], v["split_counts"]);
    mda()
        .args(["eval", "--dataset", "docsqa", "--data"])
        .arg(&data)
        .args(["--project", "nope", "--root"])
        .arg(&root)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no questions for project"));
}

#[test]
fn eval_docsqa_fetches_past_a_page_that_hogs_the_list() {
    let (dir, data, root) = docsqa_fixture();
    // One page with 40 sections that all match the query terms outranks the relevant page
    // section-for-section; with `--fetch 4` the adapter must keep fetching until it has ten
    // distinct pages (or the results run out) before it ranks pages.
    let mut hog = String::from("---\ntitle: Hog\n---\n");
    for i in 0..40 {
        hog.push_str("## Deploy note ");
        hog.push_str(&i.to_string());
        hog.push_str("\n\ndeployctl rollback deploy rollback deploy.\n\n");
    }
    write(&root, "docs/hog.mdx", &hog);
    let corpus = std::fs::read_to_string(data.join("data/corpus.jsonl")).unwrap()
        + r#"{"doc_id":"demo::/hog","project":"demo","repository_source_path":"docs/hog.mdx","local_path":"x"}"#
        + "\n";
    write(&data, "data/corpus.jsonl", &corpus);
    mda().args(["index", "--no-summarize", "--root"]).arg(&root).assert().success();
    let stdout = mda()
        .args(["--json", "eval", "--dataset", "docsqa", "--data"])
        .arg(&data)
        .args(["--project", "demo", "--root"])
        .arg(&root)
        .args(["--split", "all", "--fetch", "4"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json_of(&stdout);
    let q1 = &v["runs"][0]["results"][0];
    assert_eq!(q1["id"], "q1");
    assert!(q1["fetched"].as_u64().unwrap() > 4, "{q1}");
    assert_eq!(q1["truncated"], false);
    assert!(q1["rank"].as_u64().is_some(), "the relevant page is found behind the hog: {q1}");
    drop(dir);
}

#[test]
fn eval_docsqa_keeps_the_holdout_sealed_and_refuses_bad_output_targets() {
    let (dir, data, root) = docsqa_fixture();
    mda().args(["index", "--no-summarize", "--root"]).arg(&root).assert().success();
    let base = || {
        let mut c = mda();
        c.args(["--json", "eval", "--dataset", "docsqa", "--data"])
            .arg(&data)
            .args(["--project", "demo", "--root"])
            .arg(&root);
        c
    };
    // Without --open-holdout, `holdout` is refused and `all` scores dev + test only.
    base()
        .args(["--split", "holdout"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("sealed"));
    let all = json_of(&base().args(["--split", "all"]).assert().success().get_output().stdout);
    for r in all["runs"][0]["results"].as_array().unwrap() {
        assert_ne!(r["split"], "holdout", "{r}");
    }
    let opened = json_of(
        &base().args(["--split", "all", "--open-holdout"]).assert().success().get_output().stdout,
    );
    assert!(
        opened["runs"][0]["metrics"]["questions"].as_u64().unwrap()
            >= all["runs"][0]["metrics"]["questions"].as_u64().unwrap()
    );
    // Reports never land inside the checkout, and never follow a symlink.
    base()
        .args(["--split", "all", "--out"])
        .arg(root.join("reports"))
        .assert()
        .failure()
        .stdout(predicate::str::contains("inside the checkout"));
    #[cfg(unix)]
    {
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let victim = root.join("docs/rollback.mdx");
        let before = std::fs::read_to_string(&victim).unwrap();
        std::os::unix::fs::symlink(&victim, out.join("results.json")).unwrap();
        base()
            .args(["--split", "all", "--out"])
            .arg(&out)
            .assert()
            .failure()
            .stdout(predicate::str::contains("not a plain file"));
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            before,
            "the source file is untouched"
        );
    }
}
