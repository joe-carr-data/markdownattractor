//! Scripted backend for tests: replays outcomes per request id and records every call.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::{Backend, BoxFuture, Outcome, SummarizeRequest, Usage};
use crate::Result;
use crate::card::{Entities, SectionSummary};

/// Name reported by [`Backend::name`].
pub const BACKEND_NAME: &str = "mock";

/// A backend that answers from a script instead of a model.
///
/// Configure with the builder methods, wrap in an `Arc`, hand to a [`super::Pool`].
/// Outcomes scripted with [`Mock::on_sequence`] are consumed in order and the last one
/// repeats. Requests with no script get the default: a canned card if [`Mock::default_ok`]
/// was called, otherwise [`Outcome::Fatal`].
#[derive(Debug, Default)]
pub struct Mock {
    scripts: Mutex<HashMap<String, VecDeque<Outcome>>>,
    default_ok: bool,
    latency: Option<Duration>,
    calls: Mutex<Vec<(String, String)>>,
    /// Per call, in call order: how many calls (this one included) were in flight when it
    /// started.
    in_flight_at_call: Mutex<Vec<usize>>,
    in_flight: AtomicUsize,
    peak_in_flight: AtomicUsize,
}

impl Mock {
    /// An empty mock: every call is `Fatal` until scripted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Always answer `outcome` for request `id`.
    #[must_use]
    pub fn on(self, id: impl Into<String>, outcome: Outcome) -> Self {
        self.on_sequence(id, vec![outcome])
    }

    /// Answer `outcomes` for request `id` in order; the last one repeats.
    #[must_use]
    pub fn on_sequence(self, id: impl Into<String>, outcomes: Vec<Outcome>) -> Self {
        lock(&self.scripts).insert(id.into(), outcomes.into());
        self
    }

    /// Requests without a script get a canned valid card whose `tldr` names the request id.
    #[must_use]
    pub fn default_ok(mut self) -> Self {
        self.default_ok = true;
        self
    }

    /// Sleep this long inside every call, so pool tests can observe concurrency.
    #[must_use]
    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = Some(latency);
        self
    }

    /// Every call made so far, as `(request id, model)`, in call order.
    pub fn calls(&self) -> Vec<(String, String)> {
        lock(&self.calls).clone()
    }

    /// For every call so far, aligned with [`Mock::calls`]: how many calls were in flight
    /// when it started, counting itself. Only meaningful with [`Mock::with_latency`].
    pub fn in_flight_at_call(&self) -> Vec<usize> {
        lock(&self.in_flight_at_call).clone()
    }

    /// The most calls that were ever in flight at once.
    pub fn peak_in_flight(&self) -> usize {
        self.peak_in_flight.load(Ordering::Relaxed)
    }

    /// The canned card handed out by [`Mock::default_ok`].
    pub fn canned_summary(id: &str) -> SectionSummary {
        SectionSummary {
            tldr: format!("Canned summary for {id}."),
            summary: format!("A mock summary of request {id}, produced without a model."),
            keywords: vec!["mock".into(), "test".into(), "canned".into(), "summary".into()],
            questions_answered: vec!["what is this?".into(), "is this a mock?".into()],
            entities: Entities::default(),
            mentioned_dates: vec![],
            decisions: vec![],
            action_items: vec![],
        }
    }

    /// The usage attached to a canned `Ok`: 100 in, 50 out, $0.001, one turn.
    pub fn canned_usage(model: &str) -> Usage {
        Usage {
            input_tokens: 100,
            output_tokens: 50,
            cost_usd: 0.001,
            api_ms: 10,
            wall_ms: 12,
            model: model.to_owned(),
            turns: 1,
        }
    }

    /// A ready-made `Ok` outcome for `id`, for building scripts.
    pub fn ok_for(id: &str, model: &str) -> Outcome {
        Outcome::Ok { summary: Self::canned_summary(id), usage: Self::canned_usage(model) }
    }

    fn next_outcome(&self, id: &str, model: &str) -> Outcome {
        let mut scripts = lock(&self.scripts);
        if let Some(queue) = scripts.get_mut(id) {
            if queue.len() > 1
                && let Some(o) = queue.pop_front()
            {
                return o;
            }
            if let Some(last) = queue.front() {
                return last.clone();
            }
        }
        if self.default_ok {
            Self::ok_for(id, model)
        } else {
            Outcome::Fatal { reason: format!("mock: no scripted outcome for {id}") }
        }
    }
}

impl Backend for Mock {
    fn summarize<'a>(
        &'a self,
        req: &'a SummarizeRequest,
        model: &'a str,
    ) -> BoxFuture<'a, Result<Outcome>> {
        Box::pin(async move {
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak_in_flight.fetch_max(now, Ordering::SeqCst);
            lock(&self.calls).push((req.id.clone(), model.to_owned()));
            lock(&self.in_flight_at_call).push(now);
            if let Some(d) = self.latency {
                tokio::time::sleep(d).await;
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(self.next_outcome(&req.id, model))
        })
    }

    fn name(&self) -> &'static str {
        BACKEND_NAME
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
