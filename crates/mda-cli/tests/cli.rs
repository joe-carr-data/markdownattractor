//! End-to-end tests of the `mda` binary.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_panics_doc)]

use assert_cmd::Command;
use predicates::prelude::*;

fn mda() -> Command {
    Command::cargo_bin("mda").expect("binary builds")
}

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../mda-core/tests/fixtures/runbook.md");

#[test]
fn version_prints_semver() {
    mda().arg("--version").assert().success().stdout(predicate::str::contains("mda 0."));
}

#[test]
fn parse_human_output_lists_sections() {
    mda()
        .args(["parse", FIXTURE])
        .env("NO_COLOR", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("Deploy › Rollback"))
        .stdout(predicate::str::contains("code:bash"))
        .stdout(predicate::str::contains("5 sections"));
}

#[test]
fn parse_json_is_a_document() {
    let out =
        mda().args(["--json", "parse", FIXTURE]).assert().success().get_output().stdout.clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(v["title"], "Deploy runbook");
    assert_eq!(v["sections"].as_array().unwrap().len(), 5);
    assert_eq!(v["sections"][2]["line_start"], 19);
}

#[test]
fn parse_missing_file_fails_cleanly() {
    mda()
        .args(["parse", "/definitely/not/here.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error:"))
        .stderr(predicate::str::contains("not/here.md"));
}

#[test]
fn parse_missing_file_json_error_on_stdout() {
    let out =
        mda().args(["--json", "parse", "/nope.md"]).assert().failure().get_output().stdout.clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("json error envelope");
    assert!(v["error"].as_str().unwrap().contains("parsing"));
}

#[test]
fn schema_section_matches_checked_in_file() {
    let out = mda().args(["schema", "section"]).assert().success().get_output().stdout.clone();
    let generated: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let on_disk: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../prompts/section.schema.v1.json"
    )))
    .unwrap();
    assert_eq!(
        generated, on_disk,
        "prompts/section.schema.v1.json is stale: run `cargo run -q -- schema section > prompts/section.schema.v1.json`"
    );
}

#[test]
fn doctor_json_has_checks() {
    let dir = tempfile::tempdir().unwrap();
    let out = mda()
        .args(["--json", "doctor", "--root"])
        .arg(dir.path())
        .assert()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let names: Vec<&str> =
        v["checks"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["root", "state dir", "backend"]);
}

#[test]
fn doctor_fails_on_missing_root() {
    mda().args(["doctor", "--root", "/definitely/missing"]).assert().failure();
}
