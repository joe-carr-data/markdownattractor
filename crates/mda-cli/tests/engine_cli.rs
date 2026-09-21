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
