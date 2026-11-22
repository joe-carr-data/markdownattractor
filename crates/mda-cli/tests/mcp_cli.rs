//! `mda mcp` end to end: an rmcp client spawns the real binary over stdio, lists the tools
//! and calls search, card, open, timeline, recent, stale and status on an indexed root.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::missing_panics_doc)]

use std::path::Path;

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::TokioChildProcess;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, text).unwrap();
}

fn bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("mda")
}

#[tokio::test]
async fn mcp_server_serves_the_index_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(
        &root,
        "runbook.md",
        "# Runbook\n\n## Rollback\n\nRun deployctl rollback --to <sha>.\n\n## Contacts\n\nPage the on-call.\n",
    );
    write(&root, ".markdownattractor/config.toml", "embeddings = \"off\"\n");
    let status = std::process::Command::new(bin())
        .args(["index", "--no-summarize", "--root"])
        .arg(&root)
        .status()
        .unwrap();
    assert!(status.success());

    let mut cmd = tokio::process::Command::new(bin());
    cmd.arg("mcp").arg("--root").arg(&root).env("NO_COLOR", "1");
    let transport = TokioChildProcess::new(cmd).unwrap();
    let client = ().serve(transport).await.expect("initialize");

    let info = client.peer_info().expect("server info");
    assert_eq!(info.server_info.as_ref().expect("implementation").name, "markdownattractor");
    assert!(info.instructions.as_deref().unwrap_or("").contains("Search first"));
    // The answer contract travels with the server, so every client gets it.
    let instructions = info.instructions.as_deref().unwrap_or("");
    for part in [
        "file and line range next to each such claim",
        "updated_source",
        "unless you opened it",
        "Answer what was asked and stop",
    ] {
        assert!(instructions.contains(part), "instructions lack {part:?}");
    }

    let tools = client.list_tools(None).await.unwrap().tools;
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "mda_card",
            "mda_open",
            "mda_recent",
            "mda_search",
            "mda_stale",
            "mda_status",
            "mda_timeline"
        ]
    );
    let search_tool = tools.iter().find(|t| t.name == "mda_search").unwrap();
    assert!(search_tool.input_schema.contains_key("properties"));

    let call = |name: &'static str, args: serde_json::Value| -> CallToolRequestParams {
        serde_json::from_value(serde_json::json!({ "name": name, "arguments": args })).unwrap()
    };

    let r = client
        .call_tool(call("mda_search", serde_json::json!({ "query": "deployctl" })))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(false));
    let v = r.structured_content.clone().expect("structured content");
    assert_eq!(v["hits"].as_array().unwrap().len(), 1);
    assert_eq!(v["hits"][0]["rel_path"], "runbook.md");
    assert_eq!(v["hits"][0]["pending"], true);
    assert_eq!(v["partial"], false);
    // The lean view: a snippet because there is no card, no ranking diagnostics, a
    // timestamp to the second.
    let hit = v["hits"][0].as_object().unwrap();
    assert!(hit["snippet"].as_str().unwrap().starts_with("Run deployctl"), "{hit:?}");
    for absent in ["tldr", "score", "vector", "vector_score", "title", "via_or_fallback"] {
        assert!(!hit.contains_key(absent), "{absent} should not be in the MCP hit: {hit:?}");
    }
    assert_eq!(hit["updated_at"].as_str().unwrap().len(), "2026-09-22T08:34:46Z".len());
    assert_eq!(hit["updated_source"], "filesystem");
    assert!(hit["token_estimate"].as_u64().unwrap() > 0);
    let id = v["hits"][0]["section_id"].as_str().unwrap().to_owned();

    let r =
        client.call_tool(call("mda_open", serde_json::json!({ "section_id": id }))).await.unwrap();
    let v = r.structured_content.clone().unwrap();
    assert_eq!(v["line_start"], 3);
    assert_eq!(v["text"], "## Rollback\n\nRun deployctl rollback --to <sha>.");
    assert_eq!(v["stale"], false);
    assert_eq!(v["updated_source"], "filesystem");
    assert!(v["updated_at"].as_str().is_some(), "{v}");

    let r =
        client.call_tool(call("mda_card", serde_json::json!({ "section_id": id }))).await.unwrap();
    let v = r.structured_content.clone().unwrap();
    assert_eq!(v["state"], "pending");
    assert_eq!(v["heading_path"], serde_json::json!(["Runbook", "Rollback"]));

    let r =
        client.call_tool(call("mda_timeline", serde_json::json!({ "since": "1d" }))).await.unwrap();
    assert_eq!(r.structured_content.clone().unwrap().as_array().unwrap().len(), 1);
    let r = client.call_tool(call("mda_recent", serde_json::json!({}))).await.unwrap();
    assert_eq!(r.structured_content.clone().unwrap()[0]["rel_path"], "runbook.md");
    let r = client.call_tool(call("mda_stale", serde_json::json!({}))).await.unwrap();
    assert_eq!(r.structured_content.clone().unwrap()[0]["pending"], 3);
    let r = client.call_tool(call("mda_status", serde_json::json!({}))).await.unwrap();
    let v = r.structured_content.clone().unwrap();
    assert_eq!(v["counts"]["sections"], 3);
    assert_eq!(v["embeddings"]["setting"], "off");
    assert!(v["daemon"].is_null());

    // A bad id is an error result, not a crash.
    let r = client.call_tool(call("mda_open", serde_json::json!({ "section_id": "nope#9" }))).await;
    assert!(r.is_err() || r.unwrap().is_error == Some(true));

    client.cancel().await.unwrap();
}

/// Stops the daemon when the test ends, pass or fail, so no process outlives the test.
struct StopGuard(std::path::PathBuf);
impl Drop for StopGuard {
    fn drop(&mut self) {
        let _ = std::process::Command::new(bin()).args(["stop", "--root"]).arg(&self.0).output();
    }
}

/// `mda mcp --start` on a folder never indexed: it starts the daemon, which indexes the folder,
/// and serves it; a second `--start` reuses the running daemon. Without `--start`, nothing is
/// started (the first test asserts `daemon` is null).
#[tokio::test]
async fn mcp_start_starts_the_daemon_then_serves() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let _guard = StopGuard(root.clone());
    write(&root, "runbook.md", "# Runbook\n\n## Rollback\n\nRun deployctl rollback --to <sha>.\n");
    // The backend points at a closed port: no model is ever contacted.
    write(
        &root,
        ".markdownattractor/config.toml",
        "backend = \"local\"\nlocal_base_url = \"http://127.0.0.1:1/v1\"\nembeddings = \"off\"\n",
    );

    let serve = || {
        let mut cmd = tokio::process::Command::new(bin());
        cmd.arg("mcp").arg("--start").env("MDA_ROOT", &root).env("NO_COLOR", "1");
        TokioChildProcess::new(cmd).unwrap()
    };
    let call = |name: &'static str, args: serde_json::Value| -> CallToolRequestParams {
        serde_json::from_value(serde_json::json!({ "name": name, "arguments": args })).unwrap()
    };

    let client = ().serve(serve()).await.expect("initialize");
    let status = client.call_tool(call("mda_status", serde_json::json!({}))).await.unwrap();
    let v = status.structured_content.clone().unwrap();
    let pid = v["daemon"]["pid"].as_u64().expect("a daemon answers");

    // The daemon indexes the folder on its own; raw search finds it within seconds.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let r = client
            .call_tool(call("mda_search", serde_json::json!({ "query": "deployctl" })))
            .await
            .unwrap();
        let hits = r.structured_content.clone().unwrap()["hits"].as_array().unwrap().len();
        if hits == 1 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the daemon never indexed the folder");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    client.cancel().await.unwrap();

    // A second client reuses the same daemon.
    let client = ().serve(serve()).await.expect("initialize again");
    let v = client
        .call_tool(call("mda_status", serde_json::json!({})))
        .await
        .unwrap()
        .structured_content
        .clone()
        .unwrap();
    assert_eq!(v["daemon"]["pid"].as_u64(), Some(pid), "{v}");
    client.cancel().await.unwrap();
}

/// Two clients run `mda mcp --start` on a never-indexed folder at the same moment: both
/// initialise, and both end up served by the same single daemon.
#[tokio::test]
async fn two_simultaneous_first_starts_share_one_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let _guard = StopGuard(root.clone());
    write(&root, "notes.md", "# Notes\n\nThe deploy window is Tuesday.\n");
    write(
        &root,
        ".markdownattractor/config.toml",
        "backend = \"local\"\nlocal_base_url = \"http://127.0.0.1:1/v1\"\nembeddings = \"off\"\n",
    );
    let serve = || {
        let mut cmd = tokio::process::Command::new(bin());
        cmd.arg("mcp").arg("--start").arg("--root").arg(&root).env("NO_COLOR", "1");
        TokioChildProcess::new(cmd).unwrap()
    };
    let (a, b) = tokio::join!(().serve(serve()), ().serve(serve()));
    let (a, b) = (a.expect("first client initialises"), b.expect("second client initialises"));
    let status: CallToolRequestParams =
        serde_json::from_value(serde_json::json!({ "name": "mda_status", "arguments": {} }))
            .unwrap();
    let pa = a.call_tool(status.clone()).await.unwrap().structured_content.unwrap();
    let pb = b.call_tool(status).await.unwrap().structured_content.unwrap();
    let pid = pa["daemon"]["pid"].as_u64().expect("a daemon answers the first client");
    assert_eq!(pb["daemon"]["pid"].as_u64(), Some(pid), "one daemon: {pa} {pb}");
    a.cancel().await.unwrap();
    b.cancel().await.unwrap();
}
