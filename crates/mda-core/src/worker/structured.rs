//! A schema-constrained model call that is not a card (ADR-0009).
//!
//! [`Backend::structured`](super::Backend::structured) sends a caller-supplied system prompt,
//! one user message and a JSON schema, and returns a [`StructuredResult`]: the parsed JSON
//! object or a classified failure, and in every case the usage the backend reported. `None`
//! means the backend reported no usage for the call (a transport failure, a crash), which is
//! "unknown", not "free". The caller owns retries, budgets and the ledger.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Outcome, Usage};

/// One schema-constrained call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredRequest {
    /// An id for logs and for scripting the mock; not sent to the model.
    pub id: String,
    /// The system prompt.
    pub system: String,
    /// The user message. The caller delimits any untrusted text inside it.
    pub user: String,
    /// The JSON schema the answer must satisfy.
    pub schema: Value,
    /// A short name for the schema (`[a-z_]+`), for backends that want one.
    pub schema_name: String,
    /// The cap on output tokens for this call. `api` and `local` send it to the model, which
    /// stops there. `claude-cli` has no such flag: the call is bounded by the per-call dollar
    /// budget, and an answer that reports more output than the cap is rejected after the fact.
    pub max_output_tokens: u32,
}

/// How a structured call ended. The variant drives the caller's retry rule, as for cards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StructuredStatus {
    /// A JSON object. Not yet validated against anything but JSON syntax and object-ness.
    Ok {
        /// The model's answer.
        value: Value,
    },
    /// The call completed without a usable object (truncated, not JSON, not an object).
    Malformed {
        /// Why the output was rejected.
        reason: String,
        /// The model's text, for diagnostics.
        raw: String,
    },
    /// A transient failure (timeout, 5xx, crash).
    Retryable {
        /// What went wrong.
        reason: String,
    },
    /// HTTP 429/529 or a rate-limit message.
    RateLimited {
        /// What the backend said.
        reason: String,
    },
    /// Not worth retrying: no credentials, a bad model id, a refusal, budget exhausted.
    Fatal {
        /// Why.
        reason: String,
    },
}

/// The result of one structured call: its status, and the usage whatever the status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredResult {
    /// How the call ended.
    pub status: StructuredStatus,
    /// What the backend reported the call cost; `None` when it reported nothing.
    pub usage: Option<Usage>,
}

impl StructuredResult {
    /// A failure taken from the card path's classification. `usage` wins over any usage the
    /// outcome carries. An [`Outcome::Ok`] cannot come from a structured call and is reported
    /// as malformed.
    #[must_use]
    pub fn failed(outcome: Outcome, usage: Option<Usage>) -> Self {
        let (status, own) = match outcome {
            Outcome::Ok { usage, .. } => (
                StructuredStatus::Malformed {
                    reason: "a card where a structured answer was expected".to_owned(),
                    raw: String::new(),
                },
                Some(usage),
            ),
            Outcome::Malformed { reason, raw, usage } => {
                (StructuredStatus::Malformed { reason, raw }, usage)
            }
            Outcome::Retryable { reason } => (StructuredStatus::Retryable { reason }, None),
            Outcome::RateLimited { reason } => (StructuredStatus::RateLimited { reason }, None),
            Outcome::Fatal { reason } => (StructuredStatus::Fatal { reason }, None),
        };
        Self { status, usage: usage.or(own) }
    }

    /// Parse the model's text as one JSON object, tolerating a code fence around it.
    #[must_use]
    pub fn from_text(text: &str, usage: Option<Usage>) -> Self {
        let candidate = super::parse::strip_fences(text);
        let status = match serde_json::from_str::<Value>(candidate) {
            Ok(value) if value.is_object() => StructuredStatus::Ok { value },
            Ok(_) => StructuredStatus::Malformed {
                reason: "the answer is JSON but not an object".to_owned(),
                raw: text.to_owned(),
            },
            Err(e) if candidate.is_empty() => StructuredStatus::Malformed {
                reason: format!("empty output: {e}"),
                raw: String::new(),
            },
            Err(e) => StructuredStatus::Malformed {
                reason: format!("the answer is not JSON: {e}"),
                raw: text.to_owned(),
            },
        };
        Self { status, usage }
    }

    /// A truncated answer: the output cap was reached.
    #[must_use]
    pub fn truncated(cap: u32, raw: String, usage: Option<Usage>) -> Self {
        Self {
            status: StructuredStatus::Malformed {
                reason: format!("truncated at {cap} output tokens"),
                raw,
            },
            usage,
        }
    }

    /// A refusal or a content filter: never retried, usage kept.
    #[must_use]
    pub fn refused(reason: String, usage: Option<Usage>) -> Self {
        Self { status: StructuredStatus::Fatal { reason }, usage }
    }

    /// Reject an answer whose reported output is above `cap`. For a backend that cannot pass
    /// the cap to the model (`claude-cli`): the tokens are spent and stay in `usage`, but the
    /// caller never receives an answer larger than it asked for.
    #[must_use]
    pub fn enforce_output_cap(self, cap: u32) -> Self {
        let over = self.usage.as_ref().is_some_and(|u| u.output_tokens > u64::from(cap));
        if over && matches!(self.status, StructuredStatus::Ok { .. }) {
            let got = self.usage.as_ref().map_or(0, |u| u.output_tokens);
            return Self {
                status: StructuredStatus::Malformed {
                    reason: format!("answer of {got} output tokens is above the cap of {cap}"),
                    raw: String::new(),
                },
                usage: self.usage,
            };
        }
        self
    }

    /// The JSON object, when the call succeeded.
    #[must_use]
    pub fn value(&self) -> Option<&Value> {
        match &self.status {
            StructuredStatus::Ok { value } => Some(value),
            _ => None,
        }
    }

    /// Short variant name for logs, the ledger and `--json` output.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self.status {
            StructuredStatus::Ok { .. } => "ok",
            StructuredStatus::Malformed { .. } => "malformed",
            StructuredStatus::Retryable { .. } => "retryable",
            StructuredStatus::RateLimited { .. } => "rate_limited",
            StructuredStatus::Fatal { .. } => "fatal",
        }
    }
}
