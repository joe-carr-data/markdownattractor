//! The second backend (ADR-0002): an OpenAI-compatible chat-completions server on the user's
//! machine. llama.cpp with `gpt-oss-20b` is the reference setup; LM Studio and Ollama speak
//! the same protocol. No key, no cost, no policy question.
//!
//! ```text
//! POST {local_base_url}/chat/completions          # base url already ends with /v1
//! {
//!   "model": "<local_model>",
//!   "messages": [{"role": "system", "content": <SYSTEM_PROMPT>},
//!                {"role": "user",   "content": <user_message(req)>}],
//!   "response_format": {"type": "json_schema",
//!                       "json_schema": {"name": "section_summary", "schema": <schema>, "strict": true}},
//!   "temperature": 0.2, "max_tokens": 2048,
//!   "chat_template_kwargs": {"reasoning_effort": "<local_reasoning_effort>"}   # only when set
//! }
//! ```
//!
//! The `model` argument of [`Backend::summarize`] is ignored: there is one local model, so
//! the pool's escalation attempt simply runs it again.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::http::{self, Transport};
use super::{
    Backend, BoxFuture, FATAL_LOCAL_DOWN, Outcome, SYSTEM_PROMPT, SummarizeRequest, Usage,
};
use crate::card::SectionSummary;
use crate::config::Config;
use crate::{Error, Result};

/// Name reported by [`Backend::name`] and stamped into provenance.
pub const BACKEND_NAME: &str = "local";

/// `max_tokens` for one card.
pub const MAX_TOKENS: u32 = 2048;

/// Sampling temperature. Low, for stable JSON and evidence strings copied verbatim.
pub const TEMPERATURE: f64 = 0.2;

/// Backend that posts chat completions to a local server.
#[derive(Debug)]
pub struct LocalBackend {
    client: reqwest::Client,
    base_url: String,
    model: String,
    reasoning_effort: Option<String>,
    schema: Value,
    timeout: Duration,
}

impl LocalBackend {
    /// Build from config. Does not contact the server; [`check`] does that.
    pub fn new(cfg: &Config) -> Result<Self> {
        let timeout = cfg.effective_worker_timeout();
        Ok(Self {
            client: http::client(timeout)?,
            base_url: cfg.local_base_url.trim_end_matches('/').to_owned(),
            model: cfg.local_model.clone(),
            reasoning_effort: cfg.local_reasoning_effort.clone(),
            schema: SectionSummary::json_schema(),
            timeout,
        })
    }

    /// The model name sent to the server.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The exact JSON body that would be posted for `req`. For `mda doctor`, debug output
    /// and tests; the same body drives [`Backend::summarize`].
    pub fn request_body(&self, req: &SummarizeRequest) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": super::user_message(req)},
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {"name": "section_summary", "schema": self.schema, "strict": true},
            },
            "temperature": TEMPERATURE,
            "max_tokens": MAX_TOKENS,
        });
        if let Some(effort) = &self.reasoning_effort
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert("chat_template_kwargs".into(), json!({"reasoning_effort": effort}));
        }
        body
    }

    async fn run_once(&self, req: &SummarizeRequest, model: &str) -> Result<Outcome> {
        if req.text.trim().is_empty() {
            return Ok(Outcome::Fatal { reason: "empty input: chunk has no content".into() });
        }
        if model != self.model {
            tracing::debug!(requested = model, local = %self.model, "local backend ignores the requested model");
        }
        let started = Instant::now();
        let url = format!("{}/chat/completions", self.base_url);
        let resp = match http::send(self.client.post(&url).json(&self.request_body(req))).await {
            Ok(r) => r,
            Err(t) => return Ok(classify_transport(&t, &self.base_url, self.timeout)),
        };
        let wall_ms = http::millis_since(started);
        tracing::debug!(id = %req.id, status = resp.status, api_ms = resp.elapsed_ms, "local server responded");
        if resp.status != 200 {
            return Ok(classify_error(&resp));
        }
        Ok(classify_success(&resp.body, &self.model, resp.elapsed_ms, wall_ms))
    }
}

impl Backend for LocalBackend {
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

/// | Transport failure | Outcome |
/// |---|---|
/// | connection refused / unreachable | `Fatal` starting with [`FATAL_LOCAL_DOWN`] (stops the pool) |
/// | timeout | `Retryable` |
/// | anything else | `Retryable` |
fn classify_transport(t: &Transport, base_url: &str, timeout: Duration) -> Outcome {
    match t {
        Transport::Connect(m) => {
            tracing::error!(base_url, error = %m, "local model server unreachable");
            Outcome::Fatal { reason: format!("{FATAL_LOCAL_DOWN} at {base_url}: {m}") }
        }
        Transport::Timeout(m) => {
            Outcome::Retryable { reason: format!("timeout after {} s: {m}", timeout.as_secs()) }
        }
        Transport::Other(m) => Outcome::Retryable { reason: format!("request failed: {m}") },
    }
}

/// | Status | Outcome |
/// |---|---|
/// | 429 | `RateLimited` |
/// | other 4xx | `Fatal` for this job only, with the body's `error.message` |
/// | 5xx and anything else | `Retryable` |
fn classify_error(resp: &http::Response) -> Outcome {
    let status = resp.status;
    let msg = http::error_message(&resp.body);
    match status {
        429 => {
            Outcome::RateLimited { reason: format!("HTTP 429: {msg}{}", resp.retry_after_note()) }
        }
        400..=499 => Outcome::Fatal { reason: format!("HTTP {status}: {msg}") },
        _ => Outcome::Retryable { reason: format!("HTTP {status}: {msg}") },
    }
}

/// | Signal | Outcome |
/// |---|---|
/// | body is not JSON | `Malformed` |
/// | `choices[0].finish_reason == "length"` | `Malformed` ("truncated"), usage kept |
/// | `choices[0].message.content` is a [`SectionSummary`] | `Ok` |
/// | otherwise | `Malformed`, raw content and usage kept |
///
/// `reasoning_content`, which some servers add next to `content`, is ignored.
fn classify_success(body: &str, configured_model: &str, api_ms: u64, wall_ms: u64) -> Outcome {
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
    let usage = usage_from(&doc, configured_model, api_ms, wall_ms);
    let choice = doc.pointer("/choices/0");
    let text = choice
        .and_then(|c| c.pointer("/message/content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let finish = choice.and_then(|c| c.get("finish_reason")).and_then(Value::as_str);
    if finish == Some("length") {
        return Outcome::Malformed {
            reason: format!("truncated: finish_reason length at {MAX_TOKENS} output tokens"),
            raw: text,
            usage: Some(usage),
        };
    }
    match http::parse_summary(&text) {
        Ok(summary) => Outcome::Ok { summary, usage },
        Err(reason) => Outcome::Malformed { reason, raw: text, usage: Some(usage) },
    }
}

fn usage_from(doc: &Value, configured_model: &str, api_ms: u64, wall_ms: u64) -> Usage {
    let u = doc.get("usage");
    let get = |k: &str| u.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input_tokens: get("prompt_tokens"),
        output_tokens: get("completion_tokens"),
        cost_usd: 0.0,
        api_ms,
        wall_ms,
        model: doc.get("model").and_then(Value::as_str).unwrap_or(configured_model).to_owned(),
        turns: 1,
    }
}

/// `mda doctor`: `GET {local_base_url}/models`. Returns the configured model's id if the
/// server lists it, otherwise the first id listed; an error if the server is down or lists
/// nothing.
pub async fn check(cfg: &Config) -> Result<String> {
    let backend = LocalBackend::new(cfg)?;
    let url = format!("{}/models", backend.base_url);
    let resp = http::send(backend.client.get(&url)).await.map_err(|t| match t {
        Transport::Connect(m) => {
            Error::Worker(format!("{FATAL_LOCAL_DOWN} at {}: {m}", backend.base_url))
        }
        other => Error::Worker(format!("cannot reach {url}: {}", other.message())),
    })?;
    if resp.status != 200 {
        return Err(Error::Worker(format!(
            "HTTP {} from {url}: {}",
            resp.status,
            http::error_message(&resp.body)
        )));
    }
    let doc: Value = serde_json::from_str(&resp.body)?;
    let ids: Vec<&str> = doc
        .get("data")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).collect())
        .unwrap_or_default();
    if ids.contains(&backend.model.as_str()) {
        return Ok(backend.model.clone());
    }
    ids.first()
        .map(|s| (*s).to_owned())
        .ok_or_else(|| Error::Worker(format!("{url} lists no models; is a model loaded?")))
}
