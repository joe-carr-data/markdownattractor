#![allow(clippy::panic, reason = "test assertions")]
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::*;
use crate::config::Config;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/claude/");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}{name}")).unwrap()
}

fn req(id: &str) -> SummarizeRequest {
    SummarizeRequest {
        id: id.to_owned(),
        rel_path: "docs/x.md".into(),
        heading_path: vec!["X".into()],
        text: format!("# X\n\nSection {id} body text.\n"),
        token_estimate: 10,
    }
}

fn reqs(n: usize) -> Vec<SummarizeRequest> {
    (0..n).map(|i| req(&format!("s{i}"))).collect()
}

fn fast_cfg() -> PoolConfig {
    PoolConfig {
        initial_concurrency: 4,
        max_concurrency: 4,
        model: "haiku".into(),
        escalation_model: None,
        max_retries: 1,
        backoff: vec![Duration::from_millis(5), Duration::from_millis(5), Duration::from_millis(5)],
    }
}

fn valid_summary_json() -> String {
    serde_json::to_string(&Mock::canned_summary("hand")).unwrap()
}

// ---------------------------------------------------------------- parse_result on fixtures

#[test]
fn single_turn_fixture_is_ok_with_usage() {
    let Outcome::Ok { summary, usage } = parse_result(&fixture("success-single-turn.json"), 8400)
    else {
        panic!("expected Ok")
    };
    assert_eq!(usage.input_tokens, 2530);
    assert_eq!(usage.output_tokens, 773);
    assert_eq!(usage.api_ms, 8256);
    assert_eq!(usage.wall_ms, 8400);
    assert_eq!(usage.turns, 2);
    assert_eq!(usage.model, "claude-haiku-4-5-20251001");
    assert!((usage.cost_usd - 0.006_395).abs() < 1e-9);
    assert!(summary.tldr.starts_with("Distribution strategy"));
    assert_eq!(summary.keywords.len(), 8);
}

#[test]
fn reminder_turn_fixture_is_ok() {
    let Outcome::Ok { summary, usage } = parse_result(&fixture("success-reminder-turn.json"), 0)
    else {
        panic!("expected Ok")
    };
    assert_eq!(usage.input_tokens, 2530);
    assert_eq!(usage.output_tokens, 855);
    assert_eq!(usage.turns, 2);
    assert_eq!(summary.mentioned_dates.len(), 3);
}

#[test]
fn dates_fixture_yields_nine_dates() {
    let Outcome::Ok { summary, usage } = parse_result(&fixture("success-with-dates.json"), 0)
    else {
        panic!("expected Ok")
    };
    assert_eq!(summary.mentioned_dates.len(), 9);
    assert_eq!(usage.input_tokens, 2833);
    assert_eq!(usage.output_tokens, 1466);
    assert_eq!(usage.model, "claude-haiku-4-5-20251001");
    assert_eq!(summary.mentioned_dates[1].iso, "2026-07-28");
}

#[test]
fn missing_structured_output_fixture_is_malformed_with_raw_and_usage() {
    let Outcome::Malformed { reason, raw, usage } =
        parse_result(&fixture("missing-structured-output.json"), 0)
    else {
        panic!("expected Malformed")
    };
    assert!(reason.contains("structured_output"), "{reason}");
    assert!(raw.contains("I need the actual markdown section"));
    let usage = usage.unwrap();
    assert_eq!(usage.input_tokens, 5094);
    assert_eq!(usage.output_tokens, 212);
    assert_eq!(usage.turns, 2);
}

#[test]
fn budget_exhausted_fixture_is_fatal_not_pool_stopping() {
    let out = parse_result(&fixture("error-budget-exhausted.json"), 0);
    let Outcome::Fatal { reason } = &out else { panic!("expected Fatal, got {out:?}") };
    assert!(reason.contains("budget exhausted"), "{reason}");
    assert!(reason.contains("0.0025"), "{reason}");
    assert!(!out.stops_pool());
}

#[test]
fn bad_model_fixture_is_fatal_and_stops_pool_despite_subtype_success() {
    let out = parse_result_with_model(&fixture("error-bad-model-404.json"), 0, "nope-model");
    let Outcome::Fatal { reason } = &out else { panic!("expected Fatal, got {out:?}") };
    assert!(reason.starts_with(FATAL_BAD_MODEL), "{reason}");
    assert!(reason.contains("nope-model"), "{reason}");
    assert!(out.stops_pool());
}

// ---------------------------------------------------------------- parse_result on hand-written JSON

fn doc(extra: &str) -> String {
    format!(
        r#"{{"type":"result","is_error":true,"num_turns":1,"duration_api_ms":5,"total_cost_usd":0,
            "usage":{{"input_tokens":1,"output_tokens":2}},"modelUsage":{{}},"subtype":"success",
            "result":"boom",{extra}}}"#
    )
}

#[test]
fn status_401_is_fatal_not_logged_in() {
    let out = parse_result(&doc(r#""api_error_status":401"#), 0);
    assert!(matches!(&out, Outcome::Fatal { reason } if reason.starts_with(FATAL_NOT_LOGGED_IN)));
    assert!(out.stops_pool());
}

#[test]
fn status_403_is_fatal_not_logged_in() {
    let out = parse_result(&doc(r#""api_error_status":403"#), 0);
    assert!(out.stops_pool());
}

#[test]
fn status_429_and_529_are_rate_limited() {
    for s in [429, 529] {
        let out = parse_result(&doc(&format!(r#""api_error_status":{s}"#)), 0);
        assert!(matches!(out, Outcome::RateLimited { .. }), "{s}: {out:?}");
    }
}

#[test]
fn overloaded_message_without_status_is_rate_limited() {
    let out = parse_result(&doc(r#""errors":["API overloaded, try again"]"#), 0);
    assert!(matches!(out, Outcome::RateLimited { .. }), "{out:?}");
}

#[test]
fn budget_exhausted_by_subtype_is_fatal() {
    let out = parse_result(&doc(r#""subtype":"error_max_budget_usd","terminal_reason":"x""#), 0);
    let Outcome::Fatal { reason } = &out else { panic!("expected Fatal, got {out:?}") };
    assert!(reason.contains("budget exhausted"));
    assert!(!out.stops_pool());
}

#[test]
fn is_error_with_unknown_status_is_retryable() {
    let out = parse_result(&doc(r#""api_error_status":500"#), 0);
    assert!(matches!(&out, Outcome::Retryable { reason } if reason.contains("boom")), "{out:?}");
    let out = parse_result(&doc(r#""api_error_status":null"#), 0);
    assert!(matches!(out, Outcome::Retryable { .. }), "{out:?}");
}

#[test]
fn null_structured_output_with_valid_result_text_is_ok() {
    let summary = valid_summary_json();
    let json = serde_json::json!({
        "type": "result", "is_error": false, "num_turns": 2, "duration_api_ms": 7,
        "total_cost_usd": 0.01, "usage": {"input_tokens": 10, "output_tokens": 20},
        "modelUsage": {"claude-haiku-4-5-20251001": {}},
        "structured_output": null, "result": summary,
    });
    let Outcome::Ok { summary, usage } = parse_result(&json.to_string(), 1) else {
        panic!("expected Ok")
    };
    assert_eq!(summary.tldr, "Canned summary for hand.");
    assert_eq!(usage.turns, 2);
}

#[test]
fn fenced_result_text_is_accepted() {
    let json = serde_json::json!({
        "type": "result", "is_error": false, "usage": {}, "modelUsage": {},
        "result": format!("```json\n{}\n```", valid_summary_json()),
    });
    assert!(matches!(parse_result(&json.to_string(), 0), Outcome::Ok { .. }));
}

#[test]
fn structured_output_with_wrong_shape_is_malformed() {
    let json = serde_json::json!({
        "type": "result", "is_error": false, "usage": {}, "modelUsage": {},
        "structured_output": {"tldr": "only this"}, "result": "text that is not json",
    });
    let out = parse_result(&json.to_string(), 0);
    let Outcome::Malformed { reason, raw, usage } = out else { panic!("expected Malformed") };
    assert!(reason.starts_with("structured_output:"), "{reason}");
    assert_eq!(raw, "text that is not json");
    assert!(usage.is_some());
}

#[test]
fn garbage_text_is_malformed_with_no_usage() {
    let out = parse_result("Segmentation fault (core dumped)", 3);
    let Outcome::Malformed { raw, usage, .. } = out else { panic!("expected Malformed") };
    assert_eq!(raw, "Segmentation fault (core dumped)");
    assert!(usage.is_none());
}

#[test]
fn requested_model_is_fallback_when_model_usage_is_empty() {
    let json = doc(r#""api_error_status":500"#);
    // is_error → Retryable carries no usage; check via a success-shaped doc instead.
    let ok = json.replace(r#""is_error":true"#, r#""is_error":false"#).replace(
        r#""result":"boom""#,
        &format!(r#""result":{}"#, serde_json::to_string(&valid_summary_json()).unwrap()),
    );
    let Outcome::Ok { usage, .. } = parse_result_with_model(&ok, 0, "haiku") else {
        panic!("expected Ok")
    };
    assert_eq!(usage.model, "haiku");
}

#[test]
fn usage_sums() {
    let mut total = Usage::default();
    total += &Mock::canned_usage("a");
    total += &Mock::canned_usage("b");
    assert_eq!(total.input_tokens, 200);
    assert_eq!(total.output_tokens, 100);
    assert_eq!(total.turns, 2);
    assert_eq!(total.model, "b");
    assert!((total.cost_usd - 0.002).abs() < 1e-12);
}

// ---------------------------------------------------------------- Pool with Mock

#[tokio::test]
async fn pool_runs_everything_ok() {
    let mock = Arc::new(Mock::new().default_ok());
    let pool = Pool::new(Arc::clone(&mock), fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(10), |r| done.push(r)).await;
    assert_eq!(done.len(), 10);
    assert!(done.iter().all(|r| matches!(r.outcome, Outcome::Ok { .. }) && r.attempts == 1));
    assert_eq!(stats.ok, 10);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.completed, 10);
    assert_eq!(stats.in_flight, 0);
    assert_eq!(mock.calls().len(), 10);
    assert!(mock.calls().iter().all(|(_, m)| m == "haiku"));
}

#[tokio::test]
async fn retryable_then_ok_takes_two_attempts() {
    let mock = Arc::new(Mock::new().default_ok().on_sequence(
        "s1",
        vec![Outcome::Retryable { reason: "blip".into() }, Mock::ok_for("s1", "haiku")],
    ));
    let pool = Pool::new(Arc::clone(&mock), fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(3), |r| done.push(r)).await;
    let s1 = done.iter().find(|r| r.id == "s1").unwrap();
    assert_eq!(s1.attempts, 2);
    assert!(matches!(s1.outcome, Outcome::Ok { .. }));
    assert_eq!(stats.ok, 3);
    assert_eq!(mock.calls().iter().filter(|(id, _)| id == "s1").count(), 2);
}

#[tokio::test]
async fn retryable_twice_without_escalation_fails_after_two_attempts() {
    let mock = Arc::new(Mock::new().on("s0", Outcome::Retryable { reason: "down".into() }));
    let pool = Pool::new(mock, fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(1), |r| done.push(r)).await;
    assert_eq!(done[0].attempts, 2);
    assert!(matches!(done[0].outcome, Outcome::Retryable { .. }));
    assert_eq!(stats.failed, 1);
}

#[tokio::test]
async fn malformed_twice_then_escalation_succeeds() {
    let malformed = Outcome::Malformed { reason: "null".into(), raw: "text".into(), usage: None };
    let mock = Arc::new(
        Mock::new()
            .on_sequence("s0", vec![malformed.clone(), malformed, Mock::ok_for("s0", "sonnet")]),
    );
    let cfg = PoolConfig { escalation_model: Some("sonnet".into()), ..fast_cfg() };
    let pool = Pool::new(Arc::clone(&mock), cfg, CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(1), |r| done.push(r)).await;
    assert_eq!(done[0].attempts, 3);
    assert_eq!(done[0].model_used, "sonnet");
    assert!(matches!(done[0].outcome, Outcome::Ok { .. }));
    let models: Vec<String> = mock.calls().into_iter().map(|(_, m)| m).collect();
    assert_eq!(models, ["haiku", "haiku", "sonnet"]);
    assert_eq!(stats.ok, 1);
}

#[tokio::test]
async fn malformed_after_escalation_fails_with_raw_kept() {
    let malformed =
        Outcome::Malformed { reason: "null".into(), raw: "the raw".into(), usage: None };
    let mock = Arc::new(Mock::new().on("s0", malformed));
    let cfg = PoolConfig { escalation_model: Some("sonnet".into()), ..fast_cfg() };
    let pool = Pool::new(mock, cfg, CancellationToken::new());
    let mut done = Vec::new();
    pool.run(reqs(1), |r| done.push(r)).await;
    assert_eq!(done[0].attempts, 3);
    assert_eq!(done[0].model_used, "sonnet");
    assert!(matches!(&done[0].outcome, Outcome::Malformed { raw, .. } if raw == "the raw"));
}

#[tokio::test]
async fn rate_limited_halves_concurrency_and_still_completes() {
    let mock = Arc::new(Mock::new().default_ok().on_sequence(
        "s0",
        vec![Outcome::RateLimited { reason: "429".into() }, Mock::ok_for("s0", "haiku")],
    ));
    let pool = Pool::new(Arc::clone(&mock), fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(4), |r| done.push(r)).await;
    assert_eq!(stats.concurrency, 2);
    assert_eq!(stats.rate_limit_events, 1);
    assert_eq!(stats.ok, 4);
    let s0 = done.iter().find(|r| r.id == "s0").unwrap();
    assert_eq!(s0.attempts, 2);
    assert!(matches!(s0.outcome, Outcome::Ok { .. }));
}

#[tokio::test]
async fn rate_limit_retries_do_not_count_against_max_retries() {
    let rl = Outcome::RateLimited { reason: "529".into() };
    let mock = Arc::new(Mock::new().on_sequence(
        "s0",
        vec![
            rl.clone(),
            rl.clone(),
            rl,
            Outcome::Retryable { reason: "blip".into() },
            Mock::ok_for("s0", "haiku"),
        ],
    ));
    let pool = Pool::new(mock, fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(1), |r| done.push(r)).await;
    assert_eq!(done[0].attempts, 5);
    assert!(matches!(done[0].outcome, Outcome::Ok { .. }));
    assert_eq!(stats.rate_limit_events, 3);
    assert_eq!(stats.concurrency, 1);
}

#[tokio::test]
async fn rate_limit_beyond_backoff_steps_fails_the_job() {
    let mock = Arc::new(Mock::new().on("s0", Outcome::RateLimited { reason: "429".into() }));
    let pool = Pool::new(mock, fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    pool.run(reqs(1), |r| done.push(r)).await;
    assert_eq!(done[0].attempts, 4, "1 + backoff.len() attempts");
    assert!(matches!(done[0].outcome, Outcome::RateLimited { .. }));
}

#[tokio::test]
async fn fatal_401_stops_pool_and_reports_remaining_jobs() {
    let mock = Arc::new(
        Mock::new()
            .default_ok()
            .with_latency(Duration::from_millis(20))
            .on("s0", Outcome::Fatal { reason: format!("{FATAL_NOT_LOGGED_IN} (HTTP 401)") }),
    );
    let cancel = CancellationToken::new();
    let cfg = PoolConfig { initial_concurrency: 1, max_concurrency: 1, ..fast_cfg() };
    let pool = Pool::new(Arc::clone(&mock), cfg, cancel.clone());
    let mut done = Vec::new();
    let stats = pool.run(reqs(6), |r| done.push(r)).await;
    assert!(cancel.is_cancelled());
    assert_eq!(done.len(), 6, "every request is reported");
    assert_eq!(stats.completed, 6);
    assert_eq!(stats.failed, 6);
    assert_eq!(mock.calls().len(), 1, "no call after the fatal one");
    let stopped = done.iter().filter(|r| r.id != "s0").collect::<Vec<_>>();
    assert!(stopped.iter().all(|r| {
        r.attempts == 0
            && matches!(&r.outcome, Outcome::Fatal { reason }
                if reason.starts_with("pool stopped:") && reason.contains(FATAL_NOT_LOGGED_IN))
    }));
}

#[tokio::test]
async fn other_fatal_fails_only_that_job() {
    let mock = Arc::new(
        Mock::new().default_ok().on("s1", Outcome::Fatal { reason: "empty input".into() }),
    );
    let pool = Pool::new(mock, fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(3), |r| done.push(r)).await;
    assert_eq!(stats.ok, 2);
    assert_eq!(stats.failed, 1);
    assert_eq!(done.iter().find(|r| r.id == "s1").unwrap().attempts, 1);
}

#[tokio::test]
async fn cancellation_token_stops_early() {
    let mock = Arc::new(Mock::new().default_ok().with_latency(Duration::from_millis(30)));
    let cancel = CancellationToken::new();
    let cfg = PoolConfig { initial_concurrency: 2, max_concurrency: 2, ..fast_cfg() };
    let pool = Pool::new(Arc::clone(&mock), cfg, cancel.clone());
    let canceller = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(45)).await;
        canceller.cancel();
    });
    let mut done = Vec::new();
    let stats = pool.run(reqs(20), |r| done.push(r)).await;
    assert_eq!(done.len(), 20);
    assert!(stats.ok >= 2 && stats.ok < 20, "ok = {}", stats.ok);
    assert!(mock.calls().len() < 20);
    assert!(done.iter().any(|r| matches!(&r.outcome, Outcome::Fatal { reason }
        if reason == "pool stopped: cancelled")));
}

#[tokio::test]
async fn aimd_ramps_after_eight_successes() {
    let mock = Arc::new(Mock::new().default_ok());
    let cfg = PoolConfig { initial_concurrency: 2, max_concurrency: 4, ..fast_cfg() };
    let pool = Pool::new(mock, cfg, CancellationToken::new());
    assert_eq!(pool.stats().concurrency, 2);
    let stats = pool.run(reqs(8), |_| {}).await;
    assert_eq!(stats.concurrency, 3);
    let stats = pool.run(reqs(16), |_| {}).await;
    assert_eq!(stats.concurrency, 4, "capped at max");
}

#[tokio::test]
async fn pool_never_exceeds_concurrency() {
    let mock = Arc::new(Mock::new().default_ok().with_latency(Duration::from_millis(10)));
    let cfg = PoolConfig { initial_concurrency: 3, max_concurrency: 3, ..fast_cfg() };
    let pool = Pool::new(mock, cfg, CancellationToken::new());
    let peak = Arc::new(AtomicU16::new(0));
    let peak2 = Arc::clone(&peak);
    let stats_probe = {
        let peak = peak2;
        move |s: &PoolStats| peak.fetch_max(s.in_flight, Ordering::Relaxed)
    };
    let stats = pool
        .run(reqs(12), |_| {
            stats_probe(&pool.stats());
        })
        .await;
    assert!(peak.load(Ordering::Relaxed) <= 3);
    assert_eq!(stats.in_flight, 0);
}

#[tokio::test]
async fn on_done_called_exactly_once_per_request_and_stats_sum_usage() {
    let mock = Arc::new(Mock::new().default_ok().on_sequence(
        "s2",
        vec![Outcome::Retryable { reason: "x".into() }, Mock::ok_for("s2", "haiku")],
    ));
    let pool = Pool::new(mock, fast_cfg(), CancellationToken::new());
    let count = AtomicUsize::new(0);
    let mut seen = std::collections::HashSet::new();
    let stats = pool
        .run(reqs(5), |r| {
            count.fetch_add(1, Ordering::Relaxed);
            assert!(seen.insert(r.id.clone()), "duplicate on_done for {}", r.id);
        })
        .await;
    assert_eq!(count.load(Ordering::Relaxed), 5);
    assert_eq!(stats.usage.input_tokens, 500, "5 ok attempts report usage; the retryable one none");
    assert_eq!(stats.usage.output_tokens, 250);
    assert_eq!(stats.usage.turns, 5);
    assert!((stats.usage.cost_usd - 0.005).abs() < 1e-12);
    assert_eq!(stats.usage.model, "haiku");
}

#[tokio::test]
async fn backend_error_fails_only_that_job() {
    struct Broken;
    impl Backend for Broken {
        fn summarize<'a>(
            &'a self,
            req: &'a SummarizeRequest,
            _model: &'a str,
        ) -> BoxFuture<'a, crate::Result<Outcome>> {
            Box::pin(async move {
                if req.id == "s0" {
                    Err(crate::Error::Worker("cannot spawn".into()))
                } else {
                    Ok(Mock::ok_for(&req.id, "haiku"))
                }
            })
        }
        fn name(&self) -> &'static str {
            "broken"
        }
    }
    let pool = Pool::new(Arc::new(Broken), fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(2), |r| done.push(r)).await;
    assert_eq!(stats.ok, 1);
    assert_eq!(stats.failed, 1);
    let s0 = done.iter().find(|r| r.id == "s0").unwrap();
    assert!(matches!(&s0.outcome, Outcome::Fatal { reason } if reason.contains("cannot spawn")));
}

#[test]
fn pool_config_from_config() {
    let cfg = PoolConfig::from_config(&Config::default());
    assert_eq!(cfg.initial_concurrency, 4);
    assert_eq!(cfg.max_concurrency, 16);
    assert_eq!(cfg.model, "haiku");
    assert_eq!(cfg.max_retries, 1);
    assert_eq!(
        cfg.backoff,
        vec![Duration::from_secs(1), Duration::from_secs(4), Duration::from_secs(16)]
    );
    let fixed = PoolConfig::from_config(&Config { concurrency: Some(2), ..Config::default() });
    assert_eq!((fixed.initial_concurrency, fixed.max_concurrency), (2, 2));
}

// ---------------------------------------------------------------- ClaudeCli with a fake binary

#[test]
fn command_line_matches_adr_0001() {
    let cli = ClaudeCli::new(&Config::default()).unwrap();
    let argv = cli.command_line("haiku");
    assert_eq!(argv[0], "claude");
    assert_eq!(&argv[1..4], ["-p", "--model", "haiku"]);
    assert_eq!(argv[4], "--system-prompt");
    assert_eq!(argv[5], SYSTEM_PROMPT);
    assert!(SYSTEM_PROMPT.contains("StructuredOutput"));
    assert_eq!(&argv[6..8], ["--output-format", "json"]);
    assert_eq!(argv[8], "--json-schema");
    let schema: serde_json::Value = serde_json::from_str(&argv[9]).unwrap();
    assert_eq!(schema["type"], "object");
    assert!(!argv[9].contains('\n'), "schema is compact");
    assert_eq!(
        &argv[10..],
        [
            "--tools",
            "",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--no-session-persistence",
            "--max-budget-usd",
            "0.05"
        ]
    );
    assert!(cli.scratch_dir().is_dir());
    assert_eq!(PROMPT_VERSION, "section.v1");
}

#[test]
fn scratch_dir_is_removed_on_drop() {
    let cli = ClaudeCli::new(&Config::default()).unwrap();
    let dir = cli.scratch_dir().to_path_buf();
    assert!(dir.is_dir());
    drop(cli);
    assert!(!dir.exists());
}

#[cfg(unix)]
mod fake_binary {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("claude");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn cli(dir: &std::path::Path, body: &str, cfg: &Config) -> ClaudeCli {
        ClaudeCli::new(cfg).unwrap().with_binary(fake(dir, body))
    }

    #[tokio::test]
    async fn fixture_on_stdout_is_ok_and_receives_stdin_and_env() {
        let dir = tempfile::tempdir().unwrap();
        let stdin_copy = dir.path().join("stdin.txt");
        let env_copy = dir.path().join("env.txt");
        let body = format!(
            "cat > {} ; env > {} ; cat {}success-single-turn.json",
            stdin_copy.display(),
            env_copy.display(),
            FIXTURES
        );
        let cli = cli(dir.path(), &body, &Config::default());
        let request = req("cli-ok");
        let out = cli.summarize(&request, "haiku").await.unwrap();
        let Outcome::Ok { usage, .. } = out else { panic!("expected Ok, got {out:?}") };
        assert_eq!(usage.input_tokens, 2530);
        assert_eq!(usage.model, "claude-haiku-4-5-20251001");
        assert!(usage.wall_ms < 5_000);
        assert_eq!(std::fs::read_to_string(&stdin_copy).unwrap(), request.text);
        let env = std::fs::read_to_string(&env_copy).unwrap();
        assert!(env.lines().any(|l| l == "MAX_THINKING_TOKENS=0"), "{env}");
        assert!(env.lines().any(|l| l == "MARKDOWNATTRACTOR_WORKER=1"), "{env}");
        assert!(!env.lines().any(|l| l.starts_with("CLAUDECODE=")), "{env}");
        assert!(!env.lines().any(|l| l.starts_with("CLAUDE_CODE_")), "{env}");
        assert_eq!(cli.name(), "claude-cli");
    }

    #[tokio::test]
    async fn nonzero_exit_with_json_is_still_classified() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!("cat {FIXTURES}error-bad-model-404.json; exit 1");
        let cli = cli(dir.path(), &body, &Config::default());
        let out = cli.summarize(&req("cli-404"), "nope-model").await.unwrap();
        assert!(out.stops_pool(), "{out:?}");
    }

    #[tokio::test]
    async fn timeout_kills_the_child_and_is_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // Generous margins: spawning /bin/sh under a sandbox has been seen to take > 1 s.
        let body = format!("echo $$ > {}; exec sleep 30", pid_file.display());
        let cfg = Config { worker_timeout_secs: 3, ..Config::default() };
        let cli = cli(dir.path(), &body, &cfg);
        let started = std::time::Instant::now();
        let out = cli.summarize(&req("cli-slow"), "haiku").await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(
            matches!(&out, Outcome::Retryable { reason } if reason.contains("timeout")),
            "{out:?}"
        );
        let listing: Vec<_> =
            std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().path()).collect();
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap_or_else(|e| panic!("{e}: dir has {listing:?}; outcome {out:?}"))
            .trim()
            .parse()
            .unwrap();
        // Give the kernel a moment to reap, then check the process is gone.
        for _ in 0..50 {
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .unwrap()
                .status
                .success();
            if !alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("child {pid} still alive after timeout");
    }

    #[tokio::test]
    async fn input_must_be_provided_is_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let cli =
            cli(dir.path(), "echo 'Error: Input must be provided' >&2; exit 1", &Config::default());
        let out = cli.summarize(&req("cli-empty"), "haiku").await.unwrap();
        assert!(
            matches!(&out, Outcome::Fatal { reason } if reason.contains("empty input")),
            "{out:?}"
        );
        assert!(!out.stops_pool());
    }

    #[tokio::test]
    async fn empty_chunk_is_fatal_without_spawning() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let cli = cli(dir.path(), &format!("touch {}", marker.display()), &Config::default());
        let mut r = req("cli-blank");
        r.text = "  \n".into();
        let out = cli.summarize(&r, "haiku").await.unwrap();
        assert!(matches!(out, Outcome::Fatal { .. }));
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn not_logged_in_on_stderr_is_fatal_and_stops_pool() {
        let dir = tempfile::tempdir().unwrap();
        let cli = cli(
            dir.path(),
            "echo 'Not logged in. Run claude login' >&2; exit 1",
            &Config::default(),
        );
        let out = cli.summarize(&req("cli-auth"), "haiku").await.unwrap();
        assert!(out.stops_pool(), "{out:?}");
    }

    #[tokio::test]
    async fn crash_without_json_is_retryable_with_stderr_tail() {
        let dir = tempfile::tempdir().unwrap();
        let body =
            "i=0; while [ $i -lt 400 ]; do printf x >&2; i=$((i+1)); done; echo TAIL >&2; exit 2";
        let cli = cli(dir.path(), body, &Config::default());
        let out = cli.summarize(&req("cli-crash"), "haiku").await.unwrap();
        let Outcome::Retryable { reason } = out else { panic!("expected Retryable, got {out:?}") };
        assert!(reason.ends_with("TAIL"), "{reason}");
        assert!(reason.contains("exit status: 2"), "{reason}");
        assert!(reason.len() < 400, "tail is bounded: {}", reason.len());
    }

    #[tokio::test]
    async fn exit_zero_without_json_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let cli = cli(dir.path(), "echo hello", &Config::default());
        let out = cli.summarize(&req("cli-text"), "haiku").await.unwrap();
        assert!(matches!(&out, Outcome::Malformed { raw, .. } if raw.trim() == "hello"), "{out:?}");
    }

    #[tokio::test]
    async fn missing_binary_is_an_error() {
        let cli = ClaudeCli::new(&Config::default())
            .unwrap()
            .with_binary("/nonexistent/claude-binary".into());
        let err = cli.summarize(&req("cli-none"), "haiku").await.unwrap_err();
        assert!(matches!(err, crate::Error::Worker(_)), "{err}");
    }
}

/// Runs the real CLI. `MDA_LIVE_TESTS=1 cargo nextest run -p mda-core --run-ignored all`.
#[tokio::test]
#[ignore = "needs a logged-in claude CLI; set MDA_LIVE_TESTS=1"]
async fn live_claude_summarizes_chunk_with_dates() {
    if std::env::var("MDA_LIVE_TESTS").as_deref() != Ok("1") {
        return;
    }
    let cli = ClaudeCli::new(&Config::default()).unwrap();
    let request = SummarizeRequest {
        id: "live".into(),
        rel_path: "chunk-dates.md".into(),
        heading_path: vec![],
        text: fixture("chunk-dates.md"),
        token_estimate: 700,
    };
    let out = cli.summarize(&request, "haiku").await.unwrap();
    let Outcome::Ok { summary, usage } = out else { panic!("expected Ok, got {out:?}") };
    assert!(summary.mentioned_dates.len() >= 5, "{:?}", summary.mentioned_dates);
    assert!(usage.input_tokens > 0);
}
