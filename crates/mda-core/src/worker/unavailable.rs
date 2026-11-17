//! A backend that cannot run: every call fails with a pool-stopping reason.
//!
//! The daemon uses it when the configured backend cannot be built (no API key in the
//! environment, for example) so that watching and raw indexing still work. Nothing is ever
//! sent anywhere. The daemon reads [`Backend::unavailable_reason`] and runs no model rounds;
//! a caller that tries anyway sees one pool-stopping `Fatal`.

use std::future::ready;

use super::{
    Backend, BoxFuture, FATAL_BACKEND_UNAVAILABLE, Outcome, StructuredRequest, StructuredResult,
    StructuredStatus, SummarizeRequest, Usage,
};
use crate::Result;

/// Name reported by [`Backend::name`].
pub const BACKEND_NAME: &str = "unavailable";

/// See the module docs.
#[derive(Debug, Clone)]
pub struct Unavailable {
    reason: String,
}

impl Unavailable {
    /// A backend that fails every call with `reason`.
    pub fn new(reason: impl Into<String>) -> Self {
        Self { reason: reason.into() }
    }

    /// Why it is unavailable.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl Backend for Unavailable {
    fn summarize<'a>(
        &'a self,
        _req: &'a SummarizeRequest,
        _model: &'a str,
    ) -> BoxFuture<'a, Result<Outcome>> {
        Box::pin(ready(Ok(Outcome::Fatal {
            reason: format!("{FATAL_BACKEND_UNAVAILABLE}: {}", self.reason),
        })))
    }

    fn structured<'a>(
        &'a self,
        _req: &'a StructuredRequest,
        _model: &'a str,
    ) -> BoxFuture<'a, Result<StructuredResult>> {
        // No call is made, so the usage is known: nothing.
        Box::pin(ready(Ok(StructuredResult {
            status: StructuredStatus::Fatal {
                reason: format!("{FATAL_BACKEND_UNAVAILABLE}: {}", self.reason),
            },
            usage: Some(Usage::default()),
        })))
    }

    fn name(&self) -> &'static str {
        BACKEND_NAME
    }

    fn unavailable_reason(&self) -> Option<&str> {
        Some(&self.reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fails_every_call_and_stops_the_pool() {
        let b = Unavailable::new("ANTHROPIC_API_KEY is not set");
        let req = SummarizeRequest {
            id: "h".into(),
            rel_path: "a.md".into(),
            heading_path: vec![],
            text: "# A".into(),
            token_estimate: 1,
        };
        let out = b.summarize(&req, "m").await.unwrap();
        assert!(out.stops_pool(), "{out:?}");
        assert!(matches!(out, Outcome::Fatal { reason } if reason.contains("not set")));
        assert_eq!(b.name(), "unavailable");
        assert_eq!(b.reason(), "ANTHROPIC_API_KEY is not set");
        assert_eq!(b.unavailable_reason(), Some("ANTHROPIC_API_KEY is not set"));
    }
}
