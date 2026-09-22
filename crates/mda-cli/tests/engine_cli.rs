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
