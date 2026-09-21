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
async fn malformed_twice_then_escalation_succeeds_and_job_usage_sums_every_attempt() {
    let malformed = Outcome::Malformed {
        reason: "null".into(),
        raw: "text".into(),
        usage: Some(Mock::canned_usage("haiku")),
    };
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
    // Two malformed haiku attempts plus the sonnet one: every attempt's usage is reported.
    let usage = &done[0].usage;
    assert_eq!(usage.input_tokens, 300);
    assert_eq!(usage.output_tokens, 150);
    assert_eq!(usage.turns, 3);
    assert_eq!(usage.model, "sonnet", "model of the last attempt");
    assert!((usage.cost_usd - 0.003).abs() < 1e-12);
    assert_eq!(stats.usage, *usage, "one job, so pool and job usage agree");
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
async fn rate_limit_retries_run_under_the_reduced_concurrency() {
    let mut mock = Mock::new().with_latency(Duration::from_millis(20));
    for i in 0..4 {
        let id = format!("s{i}");
        mock = mock.on_sequence(
            &id,
            vec![Outcome::RateLimited { reason: "429".into() }, Mock::ok_for(&id, "haiku")],
        );
    }
    let mock = Arc::new(mock);
    let pool = Pool::new(Arc::clone(&mock), fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(4), |r| done.push(r)).await;
    assert_eq!(stats.ok, 4);
    assert_eq!(stats.rate_limit_events, 4);
    assert!(done.iter().all(|r| r.attempts == 2), "{done:?}");

    let calls = mock.calls();
    let widths = mock.in_flight_at_call();
    assert_eq!(calls.len(), 8);
    // The first wave ran at the initial width of 4 ...
    let first_wave = widths[..4].iter().copied().max().unwrap();
    assert_eq!(first_wave, 4, "{widths:?}");
    // ... and every retry, admitted after the halving, ran at the reduced width.
    let mut seen = std::collections::HashSet::new();
    let retry_wave = calls
        .iter()
        .zip(&widths)
        .filter(|((id, _), _)| !seen.insert(id.clone()))
        .map(|(_, w)| *w)
        .max()
        .unwrap();
    assert!(retry_wave <= 2, "retry wave ran {retry_wave} wide: {widths:?}");
    assert!(stats.concurrency <= 2, "{}", stats.concurrency);
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
    let mut job_usage = Vec::new();
    let stats = pool
        .run(reqs(5), |r| {
            count.fetch_add(1, Ordering::Relaxed);
            assert!(seen.insert(r.id.clone()), "duplicate on_done for {}", r.id);
            job_usage.push(r.usage);
        })
        .await;
    assert_eq!(count.load(Ordering::Relaxed), 5);
    assert_eq!(stats.usage.input_tokens, 500, "5 ok attempts report usage; the retryable one none");
    assert_eq!(
        job_usage.into_iter().map(|u| u.input_tokens).sum::<u64>(),
        500,
        "per-job usage sums to the pool total"
    );
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
    assert_eq!(cfg.model, "claude-haiku-4-5");
    assert_eq!(cfg.escalation_model.as_deref(), Some("claude-sonnet-5"));
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
    assert_eq!(PROMPT_VERSION, "section.v2");
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
        let received = std::fs::read_to_string(&stdin_copy).unwrap();
        // `user_message` picks a fresh nonce per call, so compare with the nonces masked.
        let mask = |s: &str| {
            let nonce = s.strip_prefix("<section-").and_then(|r| r.get(..8)).unwrap().to_owned();
            s.replace(&nonce, "NONCE")
        };
        assert_eq!(
            mask(&received),
            mask(&super::user_message(&request)),
            "stdin carries the delimited section"
        );
        assert!(received.starts_with("<section-"));
        assert!(received.contains(&request.text));
        let env = std::fs::read_to_string(&env_copy).unwrap();
        assert!(env.lines().any(|l| l == "MAX_THINKING_TOKENS=0"), "{env}");
        assert!(env.lines().any(|l| l == "MARKDOWNATTRACTOR_WORKER=1"), "{env}");
        assert!(!env.lines().any(|l| l.starts_with("CLAUDECODE=")), "{env}");
        assert!(!env.lines().any(|l| l.starts_with("CLAUDE_CODE_")), "{env}");
        assert_eq!(cli.name(), "claude-cli");
    }

    #[tokio::test]
    async fn child_that_floods_stdout_and_stderr_before_reading_stdin_does_not_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let stdin_copy = dir.path().join("stdin.txt");
        // 1 MiB on stdout and 256 KiB on stderr, both well past any pipe buffer, before the
        // first read of stdin; then the whole of stdin; then a valid result document.
        let body = format!(
            "head -c 1048576 /dev/zero | tr '\\0' x; head -c 262144 /dev/zero | tr '\\0' y >&2; \
             cat > {}; echo; cat {}success-single-turn.json",
            stdin_copy.display(),
            FIXTURES
        );
        let cfg = Config { worker_timeout_secs: 20, ..Config::default() };
        let cli = cli(dir.path(), &body, &cfg).with_timeout(Duration::from_secs(20));
        let mut request = req("cli-flood");
        request.text = "line of section text that must be consumed in full\n".repeat(8_000);
        assert!(request.text.len() > 256 * 1024, "stdin is larger than a pipe buffer");
        let started = std::time::Instant::now();
        let out = cli.summarize(&request, "haiku").await.unwrap();
        let elapsed = started.elapsed();
        assert!(matches!(out, Outcome::Ok { .. }), "{out:?}");
        assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}, pipes deadlocked?");
        let received = std::fs::read_to_string(&stdin_copy).unwrap();
        assert!(received.contains(&request.text), "stdin was consumed in full");
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

// ---------------------------------------------------------------- in-test HTTP server

/// A scripted HTTP/1.1 server on a loopback port: records every request, answers from a
/// list of `(status, json body)` consumed in order (the last one repeats).
mod fake_http {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Debug, Clone)]
    pub struct Recorded {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    impl Recorded {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }

        pub fn json(&self) -> serde_json::Value {
            serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("{e}: {}", self.body))
        }
    }

    pub struct Server {
        pub url: String,
        requests: Arc<Mutex<Vec<Recorded>>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl Server {
        pub async fn start(responses: Vec<(u16, String)>) -> Self {
            Self::start_with_headers(responses, vec![]).await
        }

        /// `extra_headers` are added to every response.
        pub async fn start_with_headers(
            responses: Vec<(u16, String)>,
            extra_headers: Vec<(String, String)>,
        ) -> Self {
            assert!(!responses.is_empty());
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&requests);
            let responses = Arc::new(responses);
            let extra = Arc::new(extra_headers);
            let counter = Arc::new(AtomicUsize::new(0));
            let task = tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else { break };
                    let seen = Arc::clone(&seen);
                    let responses = Arc::clone(&responses);
                    let extra = Arc::clone(&extra);
                    let counter = Arc::clone(&counter);
                    tokio::spawn(async move {
                        let mut stream = stream;
                        let Some(rec) = read_request(&mut stream).await else { return };
                        seen.lock().unwrap().push(rec);
                        let n = counter.fetch_add(1, Ordering::SeqCst).min(responses.len() - 1);
                        let (status, body) = &responses[n];
                        let mut head = format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                             content-length: {}\r\nconnection: close\r\n",
                            body.len()
                        );
                        for (k, v) in extra.iter() {
                            head.push_str(k);
                            head.push_str(": ");
                            head.push_str(v);
                            head.push_str("\r\n");
                        }
                        head.push_str("\r\n");
                        let _ = stream.write_all(head.as_bytes()).await;
                        let _ = stream.write_all(body.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    });
                }
            });
            Self { url, requests, task }
        }

        pub fn requests(&self) -> Vec<Recorded> {
            self.requests.lock().unwrap().clone()
        }

        pub fn last(&self) -> Recorded {
            self.requests().last().cloned().expect("server saw no request")
        }
    }

    async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Recorded> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            if let Some(i) = find(&buf, b"\r\n\r\n") {
                break i;
            }
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let mut lines = head.lines();
        let request_line = lines.next()?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next()?.to_owned();
        let path = parts.next()?.to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        let len: usize = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0);
        let mut body = buf[head_end + 4..].to_vec();
        while body.len() < len {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(len);
        Some(Recorded { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() })
    }

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    /// A loopback URL nothing listens on: bind, read the port, drop the listener.
    pub async fn dead_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        url
    }
}

use fake_http::Server;

// ---------------------------------------------------------------- ApiBackend

const UNSET_ENV: &str = "MDA_TEST_API_KEY_THAT_IS_NEVER_SET";

fn api_cfg(url: &str) -> Config {
    Config {
        api_base_url: url.to_owned(),
        api_key_env: UNSET_ENV.to_owned(),
        worker_timeout_secs: 5,
        ..Config::default()
    }
}

fn api(url: &str) -> ApiBackend {
    ApiBackend::with_key(&api_cfg(url), "sk-ant-test").unwrap()
}

fn api_message(stop_reason: &str, text: &str) -> String {
    serde_json::json!({
        "id": "msg_01", "type": "message", "role": "assistant",
        "model": "claude-haiku-4-5-20251001",
        "content": [{"type": "text", "text": text}],
        "stop_reason": stop_reason, "stop_sequence": null,
        "usage": {
            "input_tokens": 1000, "output_tokens": 200,
            "cache_creation_input_tokens": 500, "cache_read_input_tokens": 2000
        }
    })
    .to_string()
}

fn api_error(kind: &str, message: &str) -> String {
    serde_json::json!({"type": "error", "error": {"type": kind, "message": message}}).to_string()
}

#[tokio::test]
async fn api_request_has_the_messages_api_shape() {
    let server = Server::start(vec![(200, api_message("end_turn", &valid_summary_json()))]).await;
    let out = api(&server.url).summarize(&req("a1"), "claude-haiku-4-5").await.unwrap();
    assert!(matches!(out, Outcome::Ok { .. }), "{out:?}");

    let r = server.last();
    assert_eq!(r.method, "POST");
    assert_eq!(r.path, "/v1/messages");
    assert_eq!(r.header("x-api-key"), Some("sk-ant-test"));
    assert_eq!(r.header("anthropic-version"), Some("2023-06-01"));
    assert!(r.header("content-type").unwrap().starts_with("application/json"));
    let body = r.json();
    assert_eq!(body["model"], "claude-haiku-4-5");
    assert_eq!(body["max_tokens"], 2048);
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    assert_eq!(body["output_config"]["format"]["schema"]["type"], "object");
    assert!(body["output_config"].get("effort").is_none(), "no effort for the summarization model");
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["system"][0]["text"], SYSTEM_PROMPT);
    assert!(body.get("thinking").is_none(), "no thinking field");
    assert_eq!(body["messages"][0]["role"], "user");
    let content = body["messages"][0]["content"].as_str().unwrap();
    assert!(content.starts_with("<section-"), "{content}");
    assert!(content.contains("Section a1 body text."));
}

#[tokio::test]
async fn api_escalation_model_sends_low_effort() {
    let server = Server::start(vec![(200, api_message("end_turn", &valid_summary_json()))]).await;
    let backend = api(&server.url);
    backend.summarize(&req("a2"), "claude-sonnet-5").await.unwrap();
    assert_eq!(server.last().json()["output_config"]["effort"], "low");
}

#[tokio::test]
async fn api_ok_counts_cache_tokens_and_prices_the_call() {
    let server = Server::start(vec![(200, api_message("end_turn", &valid_summary_json()))]).await;
    let out = api(&server.url).summarize(&req("a3"), "claude-haiku-4-5").await.unwrap();
    let Outcome::Ok { summary, usage } = out else { panic!("expected Ok, got {out:?}") };
    assert_eq!(summary.tldr, "Canned summary for hand.");
    assert_eq!(usage.input_tokens, 3500, "input + cache write + cache read");
    assert_eq!(usage.output_tokens, 200);
    assert_eq!(usage.model, "claude-haiku-4-5-20251001", "model from the response");
    assert_eq!(usage.turns, 1);
    // 1000 × $1 + 500 × $1.25 + 2000 × $0.10 + 200 × $5, per million.
    assert!((usage.cost_usd - 0.002_825).abs() < 1e-12, "{}", usage.cost_usd);
    assert!(usage.cost_usd > 0.0);
    assert!(usage.wall_ms >= usage.api_ms);
}

#[tokio::test]
async fn api_sends_workspace_header_only_when_configured() {
    let server = Server::start(vec![(200, api_message("end_turn", &valid_summary_json()))]).await;
    let cfg = Config {
        api_base_url: server.url.clone(),
        api_workspace_id: Some("wrkspc_test123".into()),
        ..Config::default()
    };
    let backend = ApiBackend::with_key(&cfg, "sk-test").unwrap();
    let _ = backend.summarize(&req("ws"), "claude-haiku-4-5").await.unwrap();
    let last = server.last();
    let header = last
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("anthropic-workspace-id"))
        .map(|(_, v)| v.as_str());
    assert_eq!(header, Some("wrkspc_test123"));

    let server2 = Server::start(vec![(200, api_message("end_turn", &valid_summary_json()))]).await;
    let cfg2 =
        Config { api_base_url: server2.url.clone(), api_workspace_id: None, ..Config::default() };
    let backend2 = ApiBackend::with_key(&cfg2, "sk-test").unwrap();
    let _ = backend2.summarize(&req("nows"), "claude-haiku-4-5").await.unwrap();
    assert!(
        !server2
            .last()
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("anthropic-workspace-id")),
        "no header without a workspace id (unless ANTHROPIC_WORKSPACE_ID is set in the test env)"
    );
}

#[tokio::test]
async fn api_max_tokens_stop_is_malformed_truncated() {
    let server = Server::start(vec![(200, api_message("max_tokens", "{\"tldr\": \"cut"))]).await;
    let out = api(&server.url).summarize(&req("a4"), "claude-haiku-4-5").await.unwrap();
    let Outcome::Malformed { reason, raw, usage } = out else {
        panic!("expected Malformed, got {out:?}")
    };
    assert!(reason.starts_with("truncated"), "{reason}");
    assert_eq!(raw, "{\"tldr\": \"cut");
    assert_eq!(usage.unwrap().output_tokens, 200, "spend is recorded even when truncated");
}

#[tokio::test]
async fn api_refusal_is_fatal_for_the_job_only() {
    let server = Server::start(vec![(200, api_message("refusal", ""))]).await;
    let out = api(&server.url).summarize(&req("a5"), "claude-haiku-4-5").await.unwrap();
    assert!(matches!(&out, Outcome::Fatal { reason } if reason.contains("refused")), "{out:?}");
    assert!(!out.stops_pool());
}

#[tokio::test]
async fn api_garbage_text_is_malformed() {
    let server =
        Server::start(vec![(200, api_message("end_turn", "Sure! Here is a summary."))]).await;
    let out = api(&server.url).summarize(&req("a6"), "claude-haiku-4-5").await.unwrap();
    let Outcome::Malformed { raw, usage, .. } = out else {
        panic!("expected Malformed, got {out:?}")
    };
    assert_eq!(raw, "Sure! Here is a summary.");
    assert!(usage.is_some());
}

#[tokio::test]
async fn api_non_json_200_body_is_malformed() {
    let server = Server::start(vec![(200, "<html>proxy</html>".into())]).await;
    let out = api(&server.url).summarize(&req("a7"), "claude-haiku-4-5").await.unwrap();
    assert!(matches!(&out, Outcome::Malformed { usage: None, .. }), "{out:?}");
}

#[tokio::test]
async fn api_401_is_fatal_and_stops_pool() {
    let server =
        Server::start(vec![(401, api_error("authentication_error", "invalid x-api-key"))]).await;
    let out = api(&server.url).summarize(&req("a8"), "claude-haiku-4-5").await.unwrap();
    let Outcome::Fatal { reason } = &out else { panic!("expected Fatal, got {out:?}") };
    assert!(reason.starts_with(FATAL_NO_API_KEY), "{reason}");
    assert!(reason.contains("invalid x-api-key"), "{reason}");
    assert!(out.stops_pool());
}

#[tokio::test]
async fn api_403_is_fatal_and_stops_pool() {
    let server = Server::start(vec![(403, api_error("permission_error", "no"))]).await;
    let out = api(&server.url).summarize(&req("a9"), "claude-haiku-4-5").await.unwrap();
    assert!(out.stops_pool(), "{out:?}");
}

#[tokio::test]
async fn api_404_is_fatal_bad_model_and_stops_pool() {
    let server = Server::start(vec![(404, api_error("not_found_error", "model: nope"))]).await;
    let out = api(&server.url).summarize(&req("a10"), "nope").await.unwrap();
    let Outcome::Fatal { reason } = &out else { panic!("expected Fatal, got {out:?}") };
    assert!(reason.starts_with(FATAL_BAD_MODEL), "{reason}");
    assert!(reason.contains("nope"), "{reason}");
    assert!(out.stops_pool());
}

#[tokio::test]
async fn api_429_is_rate_limited_and_quotes_retry_after() {
    let server = Server::start_with_headers(
        vec![(429, api_error("rate_limit_error", "slow down"))],
        vec![("retry-after".into(), "7".into())],
    )
    .await;
    let out = api(&server.url).summarize(&req("a11"), "claude-haiku-4-5").await.unwrap();
    let Outcome::RateLimited { reason } = &out else { panic!("expected RateLimited, got {out:?}") };
    assert!(reason.contains("429") && reason.contains("slow down"), "{reason}");
    assert!(reason.contains("retry-after: 7"), "{reason}");
}

#[tokio::test]
async fn api_529_is_rate_limited() {
    let server = Server::start(vec![(529, api_error("overloaded_error", "Overloaded"))]).await;
    let out = api(&server.url).summarize(&req("a12"), "claude-haiku-4-5").await.unwrap();
    assert!(matches!(out, Outcome::RateLimited { .. }), "{out:?}");
}

#[tokio::test]
async fn api_5xx_is_retryable() {
    for status in [500, 502, 503] {
        let server = Server::start(vec![(status, api_error("api_error", "boom"))]).await;
        let out = api(&server.url).summarize(&req("a13"), "claude-haiku-4-5").await.unwrap();
        assert!(
            matches!(&out, Outcome::Retryable { reason } if reason.contains("boom")),
            "{status}: {out:?}"
        );
    }
}

#[tokio::test]
async fn api_400_and_413_are_fatal_for_the_job_only() {
    for status in [400, 413] {
        let server =
            Server::start(vec![(status, api_error("invalid_request_error", "too big"))]).await;
        let out = api(&server.url).summarize(&req("a14"), "claude-haiku-4-5").await.unwrap();
        let Outcome::Fatal { reason } = &out else {
            panic!("{status}: expected Fatal, got {out:?}")
        };
        assert!(reason.contains("too big"), "{reason}");
        assert!(!out.stops_pool());
    }
}

#[tokio::test]
async fn api_connection_refused_is_retryable() {
    let url = fake_http::dead_url().await;
    let out = api(&url).summarize(&req("a15"), "claude-haiku-4-5").await.unwrap();
    assert!(
        matches!(&out, Outcome::Retryable { reason } if reason.contains("unreachable")),
        "{out:?}"
    );
}

#[tokio::test]
async fn api_empty_input_is_fatal_without_a_request() {
    let server = Server::start(vec![(200, api_message("end_turn", &valid_summary_json()))]).await;
    let mut request = req("a16");
    request.text = "  \n".into();
    let out = api(&server.url).summarize(&request, "claude-haiku-4-5").await.unwrap();
    assert!(
        matches!(&out, Outcome::Fatal { reason } if reason.starts_with("empty input")),
        "{out:?}"
    );
    assert!(server.requests().is_empty());
}

#[test]
fn api_new_fails_without_the_key_variable_and_reads_it_when_set() {
    let err = ApiBackend::new(&api_cfg("http://127.0.0.1:1")).unwrap_err();
    assert!(matches!(&err, crate::Error::Worker(m) if m.contains(UNSET_ENV)), "{err}");
    assert!(err.to_string().contains("set "), "{err}");

    // Any variable that is certainly set will do; the value is never sent here.
    let cfg = Config { api_key_env: "PATH".into(), ..api_cfg("http://127.0.0.1:1") };
    let backend = ApiBackend::new(&cfg).unwrap();
    assert_eq!(backend.name(), "api");
    assert!(!format!("{backend:?}").contains(&std::env::var("PATH").unwrap()), "Debug redacts");

    let err = ApiBackend::with_key(&cfg, "   ").unwrap_err();
    assert!(err.to_string().contains("empty"), "{err}");
}

#[test]
fn api_price_table_matches_dated_ids_and_unknown_models_cost_nothing() {
    assert_eq!(api::price_for("claude-haiku-4-5"), Some((1.0, 5.0)));
    assert_eq!(api::price_for("claude-haiku-4-5-20251001"), Some((1.0, 5.0)));
    assert_eq!(api::price_for("claude-sonnet-5"), Some((2.0, 10.0)));
    assert_eq!(api::price_for("claude-sonnet-4-6"), Some((3.0, 15.0)));
    assert_eq!(api::price_for("claude-opus-5"), Some((5.0, 25.0)));
    assert_eq!(api::price_for("claude-sonnet-55"), None, "prefix must end at a dash");
    assert_eq!(api::price_for("gpt-oss-20b"), None);
    assert!((api::cost_usd("claude-sonnet-5", 1_000_000, 0, 0, 100_000) - 3.0).abs() < 1e-9);
    assert!(api::cost_usd("mystery-model", 1_000_000, 0, 0, 1_000_000).abs() < f64::EPSILON);
}

#[tokio::test]
async fn api_check_returns_display_name_on_200() {
    let server = Server::start(vec![(
        200,
        serde_json::json!({"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "type": "model"}).to_string(),
    )])
    .await;
    let name = api::check_with_key(&api_cfg(&server.url), "sk-ant-test").await.unwrap();
    assert_eq!(name, "Claude Haiku 4.5");
    let r = server.last();
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/v1/models/claude-haiku-4-5");
    assert_eq!(r.header("x-api-key"), Some("sk-ant-test"));
}

#[tokio::test]
async fn api_check_names_the_key_variable_on_401_and_the_model_on_404() {
    let server = Server::start(vec![(401, api_error("authentication_error", "bad key"))]).await;
    let err = api::check_with_key(&api_cfg(&server.url), "sk-ant-test").await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains(FATAL_NO_API_KEY) && msg.contains(UNSET_ENV) && msg.contains("bad key"),
        "{msg}"
    );

    let server = Server::start(vec![(404, api_error("not_found_error", "no such model"))]).await;
    let err = api::check_with_key(&api_cfg(&server.url), "sk-ant-test").await.unwrap_err();
    assert!(err.to_string().contains("model not available: claude-haiku-4-5"), "{err}");

    let err = api::check(&api_cfg(&server.url)).await.unwrap_err();
    assert!(err.to_string().contains(UNSET_ENV), "check reads the env var too: {err}");
}

// ---------------------------------------------------------------- LocalBackend

fn local_cfg(url: &str) -> Config {
    Config {
        local_base_url: format!("{url}/v1"),
        local_model: "gpt-oss-20b".into(),
        local_reasoning_effort: Some("low".into()),
        worker_timeout_secs: 5,
        ..Config::default()
    }
}

fn local(url: &str) -> LocalBackend {
    LocalBackend::new(&local_cfg(url)).unwrap()
}

fn chat_completion(finish_reason: &str, content: &str) -> String {
    serde_json::json!({
        "id": "chatcmpl-1", "object": "chat.completion", "model": "gpt-oss-20b-served",
        "choices": [{
            "index": 0, "finish_reason": finish_reason,
            "message": {"role": "assistant", "content": content, "reasoning_content": "thinking…"}
        }],
        "usage": {"prompt_tokens": 1500, "completion_tokens": 300, "total_tokens": 1800}
    })
    .to_string()
}

#[tokio::test]
async fn local_request_has_the_chat_completions_shape() {
    let server = Server::start(vec![(200, chat_completion("stop", &valid_summary_json()))]).await;
    let out = local(&server.url).summarize(&req("l1"), "claude-haiku-4-5").await.unwrap();
    assert!(matches!(out, Outcome::Ok { .. }), "{out:?}");

    let r = server.last();
    assert_eq!(r.method, "POST");
    assert_eq!(r.path, "/v1/chat/completions");
    assert!(r.header("x-api-key").is_none());
    let body = r.json();
    assert_eq!(body["model"], "gpt-oss-20b", "configured model, not the requested one");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], SYSTEM_PROMPT);
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(body["messages"][1]["content"].as_str().unwrap().starts_with("<section-"));
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(body["response_format"]["json_schema"]["name"], "section_summary");
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    assert_eq!(body["response_format"]["json_schema"]["schema"]["type"], "object");
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["max_tokens"], 2048);
    assert_eq!(body["chat_template_kwargs"]["reasoning_effort"], "low");
}

#[tokio::test]
async fn local_omits_chat_template_kwargs_when_effort_is_none() {
    let server = Server::start(vec![(200, chat_completion("stop", &valid_summary_json()))]).await;
    let cfg = Config { local_reasoning_effort: None, ..local_cfg(&server.url) };
    LocalBackend::new(&cfg).unwrap().summarize(&req("l2"), "x").await.unwrap();
    assert!(server.last().json().get("chat_template_kwargs").is_none());
}

#[tokio::test]
async fn local_ok_costs_nothing_and_reports_prompt_tokens() {
    let server = Server::start(vec![(200, chat_completion("stop", &valid_summary_json()))]).await;
    let backend = local(&server.url);
    assert_eq!(backend.name(), "local");
    assert_eq!(backend.model(), "gpt-oss-20b");
    let out = backend.summarize(&req("l3"), "x").await.unwrap();
    let Outcome::Ok { summary, usage } = out else { panic!("expected Ok, got {out:?}") };
    assert_eq!(summary.keywords.len(), 4);
    assert_eq!(usage.input_tokens, 1500);
    assert_eq!(usage.output_tokens, 300);
    assert!(usage.cost_usd.abs() < f64::EPSILON, "{}", usage.cost_usd);
    assert_eq!(usage.model, "gpt-oss-20b-served", "model as the server reports it");
    assert_eq!(usage.turns, 1);
}

#[tokio::test]
async fn local_fenced_content_is_accepted() {
    let fenced = format!("```json\n{}\n```", valid_summary_json());
    let server = Server::start(vec![(200, chat_completion("stop", &fenced))]).await;
    let out = local(&server.url).summarize(&req("l4"), "x").await.unwrap();
    assert!(matches!(out, Outcome::Ok { .. }), "{out:?}");
}

#[tokio::test]
async fn local_length_finish_is_malformed_truncated() {
    let server = Server::start(vec![(200, chat_completion("length", "{\"tldr\": \"cut"))]).await;
    let out = local(&server.url).summarize(&req("l5"), "x").await.unwrap();
    let Outcome::Malformed { reason, raw, usage } = out else {
        panic!("expected Malformed, got {out:?}")
    };
    assert!(reason.starts_with("truncated"), "{reason}");
    assert_eq!(raw, "{\"tldr\": \"cut");
    assert_eq!(usage.unwrap().output_tokens, 300);
}

#[tokio::test]
async fn local_prose_content_is_malformed() {
    let server = Server::start(vec![(200, chat_completion("stop", "I cannot do that."))]).await;
    let out = local(&server.url).summarize(&req("l6"), "x").await.unwrap();
    assert!(
        matches!(&out, Outcome::Malformed { raw, .. } if raw == "I cannot do that."),
        "{out:?}"
    );
}

#[tokio::test]
async fn local_connection_refused_is_fatal_and_stops_pool() {
    let url = fake_http::dead_url().await;
    let out = local(&url).summarize(&req("l7"), "x").await.unwrap();
    let Outcome::Fatal { reason } = &out else { panic!("expected Fatal, got {out:?}") };
    assert!(reason.starts_with(FATAL_LOCAL_DOWN), "{reason}");
    assert!(reason.contains(&url), "{reason}");
    assert!(out.stops_pool());
}

#[tokio::test]
async fn local_5xx_is_retryable() {
    for status in [500, 503] {
        let server = Server::start(vec![(status, "loading model".into())]).await;
        let out = local(&server.url).summarize(&req("l8"), "x").await.unwrap();
        assert!(
            matches!(&out, Outcome::Retryable { reason } if reason.contains("loading model")),
            "{status}: {out:?}"
        );
    }
}

#[tokio::test]
async fn local_429_is_rate_limited() {
    let server = Server::start(vec![(429, r#"{"error":{"message":"busy"}}"#.into())]).await;
    let out = local(&server.url).summarize(&req("l9"), "x").await.unwrap();
    assert!(matches!(&out, Outcome::RateLimited { reason } if reason.contains("busy")), "{out:?}");
}

#[tokio::test]
async fn local_4xx_is_fatal_for_the_job_with_the_servers_message() {
    let server = Server::start(vec![(
        400,
        r#"{"error":{"code":400,"message":"context too long","type":"invalid_request_error"}}"#
            .into(),
    )])
    .await;
    let out = local(&server.url).summarize(&req("l10"), "x").await.unwrap();
    let Outcome::Fatal { reason } = &out else { panic!("expected Fatal, got {out:?}") };
    assert!(reason.contains("HTTP 400") && reason.contains("context too long"), "{reason}");
    assert!(!out.stops_pool());
}

#[tokio::test]
async fn local_check_lists_the_model() {
    let listing = serde_json::json!({"object": "list", "data": [
        {"id": "other-model", "object": "model"},
        {"id": "gpt-oss-20b", "object": "model"},
    ]})
    .to_string();
    let server = Server::start(vec![(200, listing)]).await;
    assert_eq!(local::check(&local_cfg(&server.url)).await.unwrap(), "gpt-oss-20b");
    let r = server.last();
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/v1/models"));

    let cfg = Config { local_model: "not-loaded".into(), ..local_cfg(&server.url) };
    assert_eq!(local::check(&cfg).await.unwrap(), "other-model", "first listed when unmatched");

    let empty = Server::start(vec![(200, r#"{"object":"list","data":[]}"#.into())]).await;
    assert!(
        local::check(&local_cfg(&empty.url)).await.unwrap_err().to_string().contains("no models")
    );

    let err = local::check(&local_cfg(&fake_http::dead_url().await)).await.unwrap_err();
    assert!(err.to_string().contains(FATAL_LOCAL_DOWN), "{err}");
}

// ---------------------------------------------------------------- factory and dyn dispatch

#[test]
fn backend_for_picks_the_configured_backend() {
    use crate::config::Backend as Kind;
    let api_cfg = Config { backend: Kind::Api, api_key_env: "PATH".into(), ..Config::default() };
    assert_eq!(backend_for(&api_cfg).unwrap().name(), "api");

    let no_key = Config { api_key_env: UNSET_ENV.into(), ..api_cfg };
    let Err(err) = backend_for(&no_key) else { panic!("expected Err without a key") };
    assert!(err.to_string().contains(UNSET_ENV), "{err}");

    let local_cfg = Config { backend: Kind::Local, ..Config::default() };
    assert_eq!(backend_for(&local_cfg).unwrap().name(), "local");

    let cli_cfg =
        Config { backend: Kind::ClaudeCli, claude_cli_policy_ack: true, ..Config::default() };
    assert_eq!(backend_for(&cli_cfg).unwrap().name(), "claude-cli");
}

#[tokio::test]
async fn pool_runs_on_a_dyn_backend() {
    let backend: DynBackend = Arc::new(Mock::new().default_ok());
    let pool = Pool::new(backend, fast_cfg(), CancellationToken::new());
    let mut done = Vec::new();
    let stats = pool.run(reqs(3), |r| done.push(r)).await;
    assert_eq!(stats.ok, 3);
    assert!(format!("{pool:?}").contains("mock"));
}

#[tokio::test]
async fn pool_stops_on_local_down_and_api_key_rejected() {
    for prefix in [FATAL_LOCAL_DOWN, FATAL_NO_API_KEY] {
        let mock = Arc::new(
            Mock::new().default_ok().on("s0", Outcome::Fatal { reason: format!("{prefix}: x") }),
        );
        let cfg = PoolConfig { initial_concurrency: 1, max_concurrency: 1, ..fast_cfg() };
        let pool = Pool::new(Arc::clone(&mock), cfg, CancellationToken::new());
        let stats = pool.run(reqs(3), |_| {}).await;
        assert_eq!(stats.failed, 3, "{prefix}");
        assert_eq!(mock.calls().len(), 1, "{prefix}");
    }
}

// ---------------------------------------------------------------- live backends (opt-in)

fn live_request() -> SummarizeRequest {
    SummarizeRequest {
        id: "live".into(),
        rel_path: "chunk-dates.md".into(),
        heading_path: vec![],
        text: fixture("chunk-dates.md"),
        token_estimate: 700,
    }
}

/// Runs the real Messages API with `$ANTHROPIC_API_KEY`.
/// `MDA_LIVE_API=1 cargo nextest run -p mda-core --run-ignored all live_api`.
#[tokio::test]
#[ignore = "needs ANTHROPIC_API_KEY; set MDA_LIVE_API=1"]
async fn live_api_summarizes_chunk_with_dates() {
    if std::env::var("MDA_LIVE_API").as_deref() != Ok("1") {
        return;
    }
    let cfg = Config::default();
    let backend = ApiBackend::new(&cfg).unwrap();
    let out = backend.summarize(&live_request(), &cfg.summarization_model).await.unwrap();
    let Outcome::Ok { summary, usage } = out else { panic!("expected Ok, got {out:?}") };
    assert!(summary.mentioned_dates.len() >= 5, "{:?}", summary.mentioned_dates);
    assert!(usage.input_tokens > 0 && usage.cost_usd > 0.0, "{usage:?}");
    assert!(usage.model.starts_with("claude-haiku-4-5"), "{}", usage.model);
}

/// Runs against a local server at the default `http://127.0.0.1:8080/v1`.
/// `MDA_LIVE_LOCAL=1 cargo nextest run -p mda-core --run-ignored all live_local`.
#[tokio::test]
#[ignore = "needs a llama.cpp server on 127.0.0.1:8080; set MDA_LIVE_LOCAL=1"]
async fn live_local_summarizes_chunk() {
    if std::env::var("MDA_LIVE_LOCAL").as_deref() != Ok("1") {
        return;
    }
    let cfg = Config { worker_timeout_secs: 300, ..Config::default() };
    let model = local::check(&cfg).await.unwrap();
    let cfg = Config { local_model: model, ..cfg };
    let backend = LocalBackend::new(&cfg).unwrap();
    let out = backend.summarize(&live_request(), &cfg.summarization_model).await.unwrap();
    let Outcome::Ok { summary, usage } = out else { panic!("expected Ok, got {out:?}") };
    assert!(!summary.tldr.is_empty());
    assert!(usage.cost_usd.abs() < f64::EPSILON, "{}", usage.cost_usd);
}
