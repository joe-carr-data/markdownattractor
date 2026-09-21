//! Classification of a `claude -p --output-format json` result document.
//!
//! Implements the guardrail table from plan §4.2 literally. The `subtype` field is never used
//! as a success signal: the spike saw `subtype: "success"` on a 404. Only
//! `subtype == "error_max_budget_usd"` is consulted, as a second spelling of
//! `terminal_reason == "budget_exhausted"`.

use serde_json::Value;

use super::{FATAL_BAD_MODEL, FATAL_NOT_LOGGED_IN, Outcome, Usage};
use crate::card::SectionSummary;

/// Classify a result document. `wall_ms` is the caller's measured wall-clock time; it is
/// copied into the returned [`Usage`].
///
/// Rules, in order:
///
/// | Signal | Outcome |
/// |---|---|
/// | not JSON | `Malformed` (raw = input) |
/// | `api_error_status` 401/403 | `Fatal` (`claude not logged in …`) |
/// | `api_error_status` 404 | `Fatal` (`bad model id …`) |
/// | `api_error_status` 429/529, or `rate_limit`/`overloaded` in the error text | `RateLimited` |
/// | `terminal_reason == "budget_exhausted"` or `subtype == "error_max_budget_usd"` | `Fatal` |
/// | `is_error == true` otherwise | `Retryable` |
/// | `structured_output` is a [`SectionSummary`] | `Ok` |
/// | `structured_output` missing but `result` text parses as a [`SectionSummary`] | `Ok` |
/// | otherwise | `Malformed` (raw = `result`) |
///
/// The model in `usage` is the first key of `modelUsage`; it is empty if that map is empty.
/// Use [`parse_result_with_model`] to supply the requested model as a fallback.
pub fn parse_result(json: &str, wall_ms: u64) -> Outcome {
    parse_result_with_model(json, wall_ms, "")
}

/// [`parse_result`] with a fallback model name for when the document does not report one
/// (the 404 fixture has an empty `modelUsage`).
pub fn parse_result_with_model(json: &str, wall_ms: u64, requested_model: &str) -> Outcome {
    let doc: Value = match serde_json::from_str(json.trim()) {
        Ok(v) => v,
        Err(e) => {
            return Outcome::Malformed {
                reason: format!("stdout is not JSON: {e}"),
                raw: json.to_owned(),
                usage: None,
            };
        }
    };
    let usage = extract_usage(&doc, wall_ms, requested_model);
    let result_text = doc.get("result").and_then(Value::as_str).unwrap_or_default();
    let error_text = error_text(&doc, result_text);

    if let Some(status) = doc.get("api_error_status").and_then(Value::as_u64) {
        match status {
            401 | 403 => {
                return Outcome::Fatal {
                    reason: format!("{FATAL_NOT_LOGGED_IN} (HTTP {status}): {error_text}"),
                };
            }
            404 => {
                return Outcome::Fatal {
                    reason: format!("{FATAL_BAD_MODEL} (HTTP {status}): {error_text}"),
                };
            }
            429 | 529 => {
                return Outcome::RateLimited { reason: format!("HTTP {status}: {error_text}") };
            }
            _ => {}
        }
    }

    let terminal_reason = doc.get("terminal_reason").and_then(Value::as_str).unwrap_or_default();
    let subtype = doc.get("subtype").and_then(Value::as_str).unwrap_or_default();
    if terminal_reason == "budget_exhausted" || subtype == "error_max_budget_usd" {
        tracing::warn!(cost_usd = usage.cost_usd, "per-call budget exhausted");
        return Outcome::Fatal {
            reason: format!(
                "per-call budget exhausted (spent ${:.4}): {error_text}",
                usage.cost_usd
            ),
        };
    }

    if doc.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
        if looks_rate_limited(&error_text) {
            return Outcome::RateLimited { reason: error_text };
        }
        return Outcome::Retryable { reason: format!("claude reported an error: {error_text}") };
    }

    match doc.get("structured_output") {
        Some(so) if !so.is_null() => match serde_json::from_value::<SectionSummary>(so.clone()) {
            Ok(summary) => Outcome::Ok { summary, usage },
            Err(e) => summary_from_text(result_text, usage, format!("structured_output: {e}")),
        },
        _ => summary_from_text(result_text, usage, "structured_output is null".to_owned()),
    }
}

/// The reminder-turn fallback: the model wrote the card as text instead of calling the
/// tool. Accept it if the text is a valid card, otherwise `Malformed` with the text kept.
fn summary_from_text(result_text: &str, usage: Usage, reason: String) -> Outcome {
    let candidate = strip_fences(result_text);
    if candidate.starts_with('{')
        && let Ok(summary) = serde_json::from_str::<SectionSummary>(candidate)
    {
        tracing::debug!("accepted card from result text (reminder turn)");
        return Outcome::Ok { summary, usage };
    }
    Outcome::Malformed { reason, raw: result_text.to_owned(), usage: Some(usage) }
}

/// Strip a ```` ```json ```` … ```` ``` ```` wrapper if the model added one.
pub(super) fn strip_fences(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else { return t };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

fn error_text(doc: &Value, result_text: &str) -> String {
    let errors: Vec<&str> = doc
        .get("errors")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if errors.is_empty() { result_text.to_owned() } else { errors.join("; ") }
}

fn looks_rate_limited(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("rate_limit") || t.contains("rate limit") || t.contains("overloaded")
}

fn extract_usage(doc: &Value, wall_ms: u64, requested_model: &str) -> Usage {
    let u = doc.get("usage");
    let get = |k: &str| u.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    let model = doc
        .get("modelUsage")
        .and_then(Value::as_object)
        .and_then(|m| m.keys().next().cloned())
        .unwrap_or_else(|| requested_model.to_owned());
    Usage {
        input_tokens: get("input_tokens"),
        output_tokens: get("output_tokens"),
        cost_usd: doc.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
        api_ms: doc.get("duration_api_ms").and_then(Value::as_u64).unwrap_or(0),
        wall_ms,
        model,
        turns: doc
            .get("num_turns")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
    }
}
