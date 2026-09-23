//! `mcp_time`: the one MCP client every arm's latency is measured with (execution plan
//! §2.7). It spawns an MCP server over stdio, waits for `initialize`, then calls one tool
//! with the same arguments once per query and prints one JSON line per call with the
//! wall-clock of the call; the first call is the cold number (process start and model load
//! included in `startup_ms`, the first call's own time in `ms`), the rest are warm.
//!
//! Usage: `cargo run --release --example mcp_time -- <tool> <args-template-json> <queries.jsonl> -- <server command> [args…]`
//! The template's string values may contain `{query}`, replaced by each line's `q`.
//! Every query line is `{"id": "…", "q": "…"}`; output lines are
//! `{"id", "ms", "startup_ms" (first line only), "ok", "bytes"}`.
//! With `MCP_TIME_DUMP=<file>` every tool result is appended to that file as
//! `{"id", "ok", "result"}` (the arm drivers read the ranked lists from it: the same call
//! that is timed is the call that is scored). With `<tool>` = `--list-tools` the server's
//! tool list (names, descriptions, input schemas) is printed once and nothing is called.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stdout, clippy::panic)] // a measurement tool: a bad invocation stops it

use std::io::BufRead;
use std::time::Instant;

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::TokioChildProcess;

fn fill(template: &serde_json::Value, query: &str) -> serde_json::Value {
    match template {
        serde_json::Value::String(s) => serde_json::Value::String(s.replace("{query}", query)),
        serde_json::Value::Object(m) => {
            serde_json::Value::Object(m.iter().map(|(k, v)| (k.clone(), fill(v, query))).collect())
        }
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.iter().map(|v| fill(v, query)).collect())
        }
        other => other.clone(),
    }
}

#[tokio::main]
async fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let sep = argv
        .iter()
        .position(|a| a == "--")
        .expect("usage: <tool> <args-json> <queries.jsonl> -- <server cmd…>");
    let (head, server) = argv.split_at(sep);
    let [tool, template, queries] = head else {
        panic!("usage: <tool> <args-json> <queries.jsonl> -- <server cmd…>")
    };
    let template: serde_json::Value =
        serde_json::from_str(template).expect("args template is JSON");
    let server = &server[1..];
    assert!(!server.is_empty(), "no server command after --");

    let started = Instant::now();
    let mut cmd = tokio::process::Command::new(&server[0]);
    cmd.args(&server[1..]);
    let transport = TokioChildProcess::new(cmd).expect("spawn server");
    let client = ().serve(transport).await.expect("initialize");
    let startup_ms = started.elapsed().as_millis();
    if tool == "--list-tools" {
        let tools = client.list_tools(None).await.expect("list tools").tools;
        println!("{}", serde_json::to_string_pretty(&tools).unwrap());
        client.cancel().await.ok();
        return;
    }
    let mut dump = std::env::var_os("MCP_TIME_DUMP")
        .map(|d| std::fs::OpenOptions::new().create(true).append(true).open(d).expect("dump file"));

    let file = std::fs::File::open(queries).expect("queries file");
    let mut first = true;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(&line).expect("query line is JSON");
        let id = row["id"].as_str().unwrap_or("").to_owned();
        let q = row["q"].as_str().unwrap_or("").to_owned();
        let filled = fill(&template, &q);
        let t = Instant::now();
        let params: CallToolRequestParams =
            serde_json::from_value(serde_json::json!({ "name": tool, "arguments": filled }))
                .expect("call params");
        let result = client.call_tool(params).await;
        let ms = t.elapsed().as_millis();
        let (ok, bytes) = match &result {
            Ok(r) => {
                (!r.is_error.unwrap_or(false), serde_json::to_string(r).map_or(0, |s| s.len()))
            }
            Err(_) => (false, 0),
        };
        if let Some(d) = dump.as_mut() {
            use std::io::Write as _;
            let payload = match &result {
                Ok(r) => serde_json::to_value(r).unwrap_or(serde_json::Value::Null),
                Err(e) => serde_json::json!({"error": e.to_string()}),
            };
            writeln!(d, "{}", serde_json::json!({"id": id, "ok": ok, "result": payload}))
                .expect("dump write");
        }
        let mut out = serde_json::json!({"id": id, "ms": ms, "ok": ok, "bytes": bytes});
        if first {
            out["startup_ms"] = serde_json::json!(startup_ms);
            out["cold"] = serde_json::json!(true);
            first = false;
        }
        println!("{out}");
    }
    client.cancel().await.ok();
}
