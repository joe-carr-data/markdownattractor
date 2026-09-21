//! The default backend (ADR-0002): the Claude Messages API over raw HTTP with the user's own
//! API key. There is no official Rust SDK, so this is the whole client.
//!
//! One call is one turn:
//!
//! ```text
//! POST {api_base_url}/v1/messages
//! x-api-key: <$api_key_env>   anthropic-version: 2023-06-01   content-type: application/json
//! {
//!   "model": "<model>", "max_tokens": 2048,
//!   "system": [{"type": "text", "text": <SYSTEM_PROMPT>, "cache_control": {"type": "ephemeral"}}],
//!   "messages": [{"role": "user", "content": <user_message(req)>}],
//!   "output_config": {"format": {"type": "json_schema", "schema": <SectionSummary schema>}}
//! }
//! ```
//!
//! Structured output means the first text block *is* the card, so there is no reminder turn.
//! The system prompt carries a cache breakpoint, so after the first call the prompt is a
//! cache read. No `thinking` field is sent: Haiku 4.5 does not think unless asked, and for
//! the escalation model (Sonnet 5 runs adaptive thinking by default) `output_config.effort`
//! is set to `low`.
//!
//! The key is read from the environment once, in [`ApiBackend::new`]; a missing key is a
//! construction error, never a per-call outcome.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::http::{self, Transport};
use super::{
    Backend, BoxFuture, FATAL_BAD_MODEL, FATAL_NO_API_KEY, Outcome, SYSTEM_PROMPT,
    SummarizeRequest, Usage,
};
use crate::card::SectionSummary;
use crate::config::Config;
use crate::{Error, Result};

/// Name reported by [`Backend::name`] and stamped into provenance.
pub const BACKEND_NAME: &str = "api";

/// The `anthropic-version` header sent on every request.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// `max_tokens` for one card. A card is ~200 tokens; 2048 leaves room for long date lists.
pub const MAX_TOKENS: u32 = 2048;

/// List prices in USD per million tokens, `(model id prefix, input, output)`. Cache reads
/// cost 0.1× input and cache writes 1.25× input. Matched on the model id the API returns
/// (`claude-haiku-4-5-20251001` matches `claude-haiku-4-5`).
pub const PRICES_PER_MTOK: &[(&str, f64, f64)] = &[
    ("claude-haiku-4-5", 1.00, 5.00),
    ("claude-sonnet-5", 2.00, 10.00),
    ("claude-opus-5", 5.00, 25.00),
    ("claude-sonnet-4-6", 3.00, 15.00),
];

/// Input and output list price for `model`, or `None` if it is not in the table.
pub fn price_for(model: &str) -> Option<(f64, f64)> {
    PRICES_PER_MTOK
        .iter()
        .filter(|(prefix, _, _)| {
            model == *prefix || model.strip_prefix(prefix).is_some_and(|r| r.starts_with('-'))
        })
        .max_by_key(|(prefix, _, _)| prefix.len())
        .map(|(_, i, o)| (*i, *o))
}

/// List-price cost of one call. Unknown models cost 0 and are warned about once.
pub fn cost_usd(
    model: &str,
    input_tokens: u64,
    cache_write_tokens: u64,
    cache_read_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let Some((input, output)) = price_for(model) else {
        warn_unknown_model_once(model);
        return 0.0;
    };
    let t = http::tokens_f64;
    (t(input_tokens) * input
        + t(cache_write_tokens) * input * 1.25
        + t(cache_read_tokens) * input * 0.1
        + t(output_tokens) * output)
        / 1_000_000.0
}

fn warn_unknown_model_once(model: &str) {
    static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let mut warned = WARNED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if warned.insert(model.to_owned()) {
        tracing::warn!(model, "no price known for model; cost recorded as $0");
    }
}

/// Read the key from the environment variable named `name`.
fn key_from_env(name: &str) -> Result<String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v.trim().to_owned()),
        _ => Err(Error::Worker(format!(
            "no API key: set {name} to your Anthropic API key (backend \"api\"), or switch to \
             backend = \"local\""
        ))),
    }
}

/// Backend that calls the Messages API. Holds the key in memory; `Debug` redacts it.
pub struct ApiBackend {
    client: reqwest::Client,
    base_url: String,
    key: String,
    key_env: String,
    escalation_model: Option<String>,
    schema: Value,
    timeout: Duration,
}

impl std::fmt::Debug for ApiBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiBackend")
            .field("base_url", &self.base_url)
            .field("key", &"<redacted>")
            .field("key_env", &self.key_env)
            .field("escalation_model", &self.escalation_model)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ApiBackend {
    /// Build from config, reading the key from `$api_key_env`. Fails here, not on the first
    /// call, when the variable is unset or empty.
    pub fn new(cfg: &Config) -> Result<Self> {
        let key = key_from_env(&cfg.api_key_env)?;
        Self::with_key(cfg, key)
    }

    /// Build with an explicit key instead of reading the environment. For tests and for a
    /// CLI that has already resolved the key.
    pub fn with_key(cfg: &Config, key: impl Into<String>) -> Result<Self> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(Error::Worker("api key is empty".into()));
        }
        let timeout = Duration::from_secs(cfg.worker_timeout_secs);
        Ok(Self {
            client: http::client(timeout)?,
            base_url: cfg.api_base_url.trim_end_matches('/').to_owned(),
            key: key.trim().to_owned(),
            key_env: cfg.api_key_env.clone(),
            escalation_model: cfg.escalation_model.clone(),
            schema: SectionSummary::json_schema(),
            timeout,
        })
    }

    /// The exact JSON body that would be posted for `req` with `model`. For `mda doctor`,
    /// debug output and tests; the same body drives [`Backend::summarize`].
    pub fn request_body(&self, req: &SummarizeRequest, model: &str) -> Value {
        let mut output_config = json!({
            "format": {"type": "json_schema", "schema": self.schema},
        });
        if self.escalation_model.as_deref() == Some(model)
            && let Some(oc) = output_config.as_object_mut()
        {
            oc.insert("effort".into(), json!("low"));
        }
        json!({
            "model": model,
            "max_tokens": MAX_TOKENS,
            "system": [{
                "type": "text",
                "text": SYSTEM_PROMPT,
                "cache_control": {"type": "ephemeral"},
            }],
            "messages": [{"role": "user", "content": super::user_message(req)}],
            "output_config": output_config,
        })
    }

    fn headers(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("x-api-key", &self.key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
    }

    async fn run_once(&self, req: &SummarizeRequest, model: &str) -> Result<Outcome> {
        if req.text.trim().is_empty() {
            return Ok(Outcome::Fatal { reason: "empty input: chunk has no content".into() });
        }
        let started = Instant::now();
        let body = self.request_body(req, model);
        let request = self.headers(self.client.post(format!("{}/v1/messages", self.base_url)));
        let resp = match http::send(request.json(&body)).await {
            Ok(r) => r,
            Err(t) => {
                let reason = match &t {
                    Transport::Timeout(m) => {
                        tracing::warn!(id = %req.id, timeout_s = self.timeout.as_secs(), "api call timed out");
                        format!("timeout after {} s: {m}", self.timeout.as_secs())
                    }
                    Transport::Connect(m) | Transport::Other(m) => format!("api unreachable: {m}"),
                };
                return Ok(Outcome::Retryable { reason });
            }
        };
        let wall_ms = http::millis_since(started);
        tracing::debug!(id = %req.id, status = resp.status, api_ms = resp.elapsed_ms, "messages api responded");
        if resp.status != 200 {
            return Ok(classify_error(&resp, model));
        }
        Ok(classify_success(&resp.body, model, resp.elapsed_ms, wall_ms))
    }
}

impl Backend for ApiBackend {
    fn summarize<'a>(
        &'a self,
        req: &'a SummarizeRequest,
        model: &'a str,
    ) -> BoxFuture<'a, Result<Outcome>> {
        Box::pin(self.run_once(req, model))
    }

    fn name(&self) -> &'static str {
        BACKEND_NAME
    }
}

/// Classify a non-200 response.
///
/// | Status | Outcome |
/// |---|---|
/// | 401, 403 | `Fatal` starting with [`FATAL_NO_API_KEY`] (stops the pool) |
/// | 404 | `Fatal` starting with [`FATAL_BAD_MODEL`] (stops the pool) |
/// | 429, 529 | `RateLimited`, `retry-after` quoted in the reason |
/// | 400, 413, other 4xx | `Fatal` for this job only |
/// | 500, 502, 503, anything else | `Retryable` |
fn classify_error(resp: &http::Response, model: &str) -> Outcome {
    let status = resp.status;
    let msg = http::error_message(&resp.body);
    match status {
        401 | 403 => {
            Outcome::Fatal { reason: format!("{FATAL_NO_API_KEY} (HTTP {status}): {msg}") }
        }
        404 => Outcome::Fatal {
            reason: format!("{FATAL_BAD_MODEL} (HTTP {status}, model {model}): {msg}"),
        },
        429 | 529 => Outcome::RateLimited {
            reason: format!("HTTP {status}: {msg}{}", resp.retry_after_note()),
        },
        400..=499 => Outcome::Fatal { reason: format!("HTTP {status}: {msg}") },
        _ => Outcome::Retryable { reason: format!("HTTP {status}: {msg}") },
    }
}

/// Classify a 200 response body.
///
/// | Signal | Outcome |
/// |---|---|
/// | body is not JSON | `Malformed` |
/// | `stop_reason == "max_tokens"` | `Malformed` ("truncated"), usage kept |
/// | `stop_reason == "refusal"` | `Fatal` for this job only |
/// | first text block is a [`SectionSummary`] | `Ok` |
/// | otherwise | `Malformed`, raw text and usage kept |
fn classify_success(body: &str, requested_model: &str, api_ms: u64, wall_ms: u64) -> Outcome {
    let doc: Value = match serde_json::from_str(body.trim()) {
        Ok(v) => v,
        Err(e) => {
            return Outcome::Malformed {
                reason: format!("HTTP 200 but body is not JSON: {e}"),
                raw: body.to_owned(),
                usage: None,
            };
        }
    };
    let usage = usage_from(&doc, requested_model, api_ms, wall_ms);
    let text = first_text_block(&doc);
    let stop_reason = doc.get("stop_reason").and_then(Value::as_str).unwrap_or_default();
    match stop_reason {
        "max_tokens" => Outcome::Malformed {
            reason: format!("truncated: stop_reason max_tokens at {MAX_TOKENS} output tokens"),
            raw: text,
            usage: Some(usage),
        },
        "refusal" => {
            let category = doc
                .pointer("/stop_details/category")
                .and_then(Value::as_str)
                .unwrap_or("unspecified");
            Outcome::Fatal {
                reason: format!("model refused (category {category}): {}", http::tail(&text)),
            }
        }
        _ => match http::parse_summary(&text) {
            Ok(summary) => Outcome::Ok { summary, usage },
            Err(reason) => Outcome::Malformed { reason, raw: text, usage: Some(usage) },
        },
    }
}

fn first_text_block(doc: &Value) -> String {
    doc.get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| {
            blocks.iter().find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        })
        .and_then(|b| b.get("text").and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn usage_from(doc: &Value, requested_model: &str, api_ms: u64, wall_ms: u64) -> Usage {
    let u = doc.get("usage");
    let get = |k: &str| u.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    let input = get("input_tokens");
    let cache_write = get("cache_creation_input_tokens");
    let cache_read = get("cache_read_input_tokens");
    let output = get("output_tokens");
    let model = doc.get("model").and_then(Value::as_str).unwrap_or(requested_model).to_owned();
    Usage {
        input_tokens: input + cache_write + cache_read,
        output_tokens: output,
        cost_usd: cost_usd(&model, input, cache_write, cache_read, output),
        api_ms,
        wall_ms,
        model,
        turns: 1,
    }
}

/// `mda doctor`: `GET {api_base_url}/v1/models/{summarization_model}` with the key from
/// `$api_key_env`. Returns the model's `display_name` (or `id`).
pub async fn check(cfg: &Config) -> Result<String> {
    let key = key_from_env(&cfg.api_key_env)?;
    check_with_key(cfg, &key).await
}

/// [`check`] with an explicit key. Errors name the key variable on 401/403 and say "model
/// not available" on 404.
pub async fn check_with_key(cfg: &Config, key: &str) -> Result<String> {
    let backend = ApiBackend::with_key(cfg, key)?;
    let model = &cfg.summarization_model;
    let url = format!("{}/v1/models/{model}", backend.base_url);
    let resp = http::send(backend.headers(backend.client.get(&url))).await.map_err(|t| {
        Error::Worker(format!("cannot reach {}: {}", backend.base_url, t.message()))
    })?;
    let msg = http::error_message(&resp.body);
    match resp.status {
        200 => {
            let doc: Value = serde_json::from_str(&resp.body)?;
            let name = doc
                .get("display_name")
                .or_else(|| doc.get("id"))
                .and_then(Value::as_str)
                .unwrap_or(model);
            Ok(name.to_owned())
        }
        401 | 403 => Err(Error::Worker(format!(
            "{FATAL_NO_API_KEY} (HTTP {}): key from {} was rejected: {msg}",
            resp.status, backend.key_env
        ))),
        404 => Err(Error::Worker(format!("model not available: {model} (HTTP 404): {msg}"))),
        s => Err(Error::Worker(format!("HTTP {s} from {url}: {msg}"))),
    }
}
