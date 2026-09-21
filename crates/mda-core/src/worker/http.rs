//! HTTP plumbing shared by [`super::ApiBackend`] and [`super::LocalBackend`]: one `reqwest`
//! client per backend, a `send` that reads the whole body and classifies transport failures,
//! and the small body helpers both backends need.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::card::SectionSummary;
use crate::{Error, Result};

/// Longest body excerpt kept in a reason string.
const BODY_TAIL: usize = 300;

/// Build a client whose every request, connect to last body byte, is bounded by `timeout`.
pub(super) fn client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| Error::Worker(format!("cannot build http client: {e}")))
}

/// A completed exchange: status and body read in full.
#[derive(Debug)]
pub(super) struct Response {
    /// HTTP status code.
    pub status: u16,
    /// The `retry-after` header, verbatim, if the server sent one.
    pub retry_after: Option<String>,
    /// Body text (lossy UTF-8).
    pub body: String,
    /// Milliseconds from sending the request to reading the last body byte.
    pub elapsed_ms: u64,
}

impl Response {
    /// `retry-after` rendered for a reason string: `" (retry-after: 7)"` or nothing.
    pub fn retry_after_note(&self) -> String {
        self.retry_after.as_deref().map(|v| format!(" (retry-after: {v})")).unwrap_or_default()
    }
}

/// Why no response came back at all.
#[derive(Debug)]
pub(super) enum Transport {
    /// The client-side timeout fired.
    Timeout(String),
    /// TCP/TLS connection could not be established (refused, unresolved host, reset).
    Connect(String),
    /// Anything else: bad URL, body read error.
    Other(String),
}

impl Transport {
    /// The underlying message.
    pub fn message(&self) -> &str {
        match self {
            Self::Timeout(m) | Self::Connect(m) | Self::Other(m) => m,
        }
    }
}

/// Send `req`, read the body, and time the round trip.
pub(super) async fn send(req: reqwest::RequestBuilder) -> std::result::Result<Response, Transport> {
    let started = Instant::now();
    let resp = req.send().await.map_err(|e| classify(&e))?;
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_owned());
    let body = resp.text().await.map_err(|e| classify(&e))?;
    Ok(Response { status, retry_after, body, elapsed_ms: millis_since(started) })
}

fn classify(e: &reqwest::Error) -> Transport {
    let msg = full_message(e);
    if e.is_timeout() {
        Transport::Timeout(msg)
    } else if e.is_connect() {
        Transport::Connect(msg)
    } else {
        Transport::Other(msg)
    }
}

/// `reqwest` errors print only their top layer; the useful part ("connection refused") is a
/// source or two down.
fn full_message(e: &dyn std::error::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut cur = e.source();
    while let Some(s) = cur {
        parts.push(s.to_string());
        cur = s.source();
    }
    parts.dedup();
    parts.join(": ")
}

/// Milliseconds elapsed since `started`, saturating.
pub(super) fn millis_since(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The error message inside a JSON error body: `error.message`, a bare `error` string, or
/// `message`; otherwise an excerpt of the body itself.
pub(super) fn error_message(body: &str) -> String {
    let doc: Option<Value> = serde_json::from_str(body.trim()).ok();
    let from_doc = doc.as_ref().and_then(|d| {
        d.get("error")
            .and_then(|e| e.get("message").and_then(Value::as_str).or_else(|| e.as_str()))
            .or_else(|| d.get("message").and_then(Value::as_str))
            .map(str::to_owned)
    });
    from_doc.unwrap_or_else(|| tail(body))
}

/// Parse model output as a [`SectionSummary`], tolerating a code fence around it.
pub(super) fn parse_summary(text: &str) -> std::result::Result<SectionSummary, String> {
    let candidate = super::parse::strip_fences(text);
    if candidate.is_empty() {
        return Err("empty output".to_owned());
    }
    serde_json::from_str::<SectionSummary>(candidate).map_err(|e| format!("not a card: {e}"))
}

/// The last [`BODY_TAIL`] characters of `s`, trimmed.
pub(super) fn tail(s: &str) -> String {
    let t = s.trim();
    let n = t.chars().count();
    if n <= BODY_TAIL { t.to_owned() } else { t.chars().skip(n - BODY_TAIL).collect() }
}

/// A `u64` token count as `f64` for pricing arithmetic.
#[expect(clippy::cast_precision_loss, reason = "token counts are far below 2^53")]
pub(super) fn tokens_f64(n: u64) -> f64 {
    n as f64
}
