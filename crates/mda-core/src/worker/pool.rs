//! Adaptive worker pool: runs many requests through a [`Backend`] with AIMD concurrency and
//! the plan §4.2 retry policy.
//!
//! Per job:
//!
//! | Outcome | Action |
//! |---|---|
//! | `Ok` | done; after 8 consecutive `Ok` across the pool, concurrency +1 (up to `max_concurrency`) |
//! | `Retryable` / `Malformed` | retry up to `max_retries` with the same model, then once with `escalation_model` if set, then fail |
//! | `RateLimited` | halve concurrency (min 1), sleep the next backoff step, retry; does not count against `max_retries`; fails after `backoff.len()` retries |
//! | `Fatal` with [`Outcome::stops_pool`] | cancel the token; every unfinished job reports `Fatal("pool stopped: …")` |
//! | other `Fatal`, or `Err` from the backend | fail that job only |
//!
//! Concurrency gates two things. A job takes a *job slot* from spawn to completion, so at most
//! `concurrency` jobs are ever active at once. Every backend attempt, first or retry, then
//! takes an *in-flight slot* under the concurrency limit current at that moment, so a wave of
//! rate-limit retries after a halving runs at the reduced width rather than all at once.
//! Slots are released before the backoff sleep, and a halving never kills an in-flight call.
//! Cancellation is honoured between jobs, while waiting for a slot, during backoff sleeps and
//! while a call is in flight (dropping the backend future kills the child process).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::{Backend, Outcome, SummarizeRequest, Usage};
use crate::config::Config;

/// Concurrency the pool starts at when the config says "adaptive".
pub const DEFAULT_INITIAL_CONCURRENCY: u16 = 4;

/// Ceiling for adaptive concurrency.
pub const DEFAULT_MAX_CONCURRENCY: u16 = 16;

/// Consecutive `Ok` results needed before concurrency grows by one.
pub const AIMD_SUCCESS_WINDOW: u32 = 8;

/// Tunables for a [`Pool`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolConfig {
    /// Concurrency at start.
    pub initial_concurrency: u16,
    /// Concurrency never exceeds this.
    pub max_concurrency: u16,
    /// Model for every first attempt.
    pub model: String,
    /// Model for the last attempt after `max_retries` failures, if set.
    pub escalation_model: Option<String>,
    /// Same-model retries for `Retryable`/`Malformed`. Default 1.
    pub max_retries: u8,
    /// Sleeps before successive rate-limit retries. Default 1 s, 4 s, 16 s.
    pub backoff: Vec<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            initial_concurrency: DEFAULT_INITIAL_CONCURRENCY,
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            model: "haiku".to_owned(),
            escalation_model: None,
            max_retries: 1,
            backoff: vec![Duration::from_secs(1), Duration::from_secs(4), Duration::from_secs(16)],
        }
    }
}

impl PoolConfig {
    /// Derive from the user's config. A fixed `concurrency` pins both initial and max;
    /// `None` means adaptive from 4 up to 16.
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            initial_concurrency: cfg.concurrency.unwrap_or(DEFAULT_INITIAL_CONCURRENCY).max(1),
            max_concurrency: cfg.concurrency.unwrap_or(DEFAULT_MAX_CONCURRENCY).max(1),
            model: cfg.summarization_model.clone(),
            escalation_model: cfg.escalation_model.clone(),
            ..Self::default()
        }
    }
}

/// Final word on one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobResult {
    /// The request id.
    pub id: String,
    /// Backend calls made, including rate-limit retries. `0` if the job never ran.
    pub attempts: u8,
    /// Model of the last attempt.
    pub model_used: String,
    /// `Ok`, or the terminal failure. Never `Retryable` or `RateLimited`.
    pub outcome: Outcome,
    /// What this job cost: the sum over every attempt that reported usage, including failed,
    /// `Malformed` and escalation attempts. Default when the job never ran.
    pub usage: Usage,
}

/// Snapshot of pool counters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PoolStats {
    /// Current concurrency limit.
    pub concurrency: u16,
    /// Backend calls in flight right now. Jobs sleeping in backoff or waiting for a slot are
    /// not counted.
    pub in_flight: u16,
    /// Jobs finished, ok or not.
    pub completed: u64,
    /// Jobs that ended `Ok`.
    pub ok: u64,
    /// Jobs that ended in a terminal failure.
    pub failed: u64,
    /// Times a backend call came back `RateLimited`.
    pub rate_limit_events: u64,
    /// Summed usage over every attempt that reported one.
    pub usage: Usage,
}

#[derive(Debug)]
struct State {
    stats: PoolStats,
    consecutive_ok: u32,
    stop_reason: Option<String>,
}

/// Runs requests through a backend. See the module docs for the policy.
pub struct Pool<B: Backend + ?Sized> {
    backend: Arc<B>,
    cfg: Arc<PoolConfig>,
    cancel: CancellationToken,
    state: Arc<Mutex<State>>,
    /// Woken whenever an in-flight slot may have become free.
    slots: Arc<Notify>,
}

impl<B: Backend + ?Sized> std::fmt::Debug for Pool<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("backend", &self.backend.name())
            .field("cfg", &self.cfg)
            .field("stats", &lock(&self.state).stats)
            .finish_non_exhaustive()
    }
}

impl<B: Backend + ?Sized + 'static> Pool<B> {
    /// Create a pool. `cancel` stops it from outside; the pool also cancels it itself on a
    /// pool-stopping `Fatal`.
    pub fn new(backend: Arc<B>, cfg: PoolConfig, cancel: CancellationToken) -> Self {
        let stats =
            PoolStats { concurrency: cfg.initial_concurrency.max(1), ..PoolStats::default() };
        Self {
            backend,
            cfg: Arc::new(cfg),
            cancel,
            state: Arc::new(Mutex::new(State { stats, consecutive_ok: 0, stop_reason: None })),
            slots: Arc::new(Notify::new()),
        }
    }

    /// Snapshot of the counters.
    pub fn stats(&self) -> PoolStats {
        lock(&self.state).stats.clone()
    }

    /// Run every request to completion (or until the token is cancelled), calling `on_done`
    /// once per request in completion order. Returns the final stats.
    pub async fn run(
        &self,
        requests: Vec<SummarizeRequest>,
        mut on_done: impl FnMut(JobResult) + Send,
    ) -> PoolStats {
        let mut queue: VecDeque<SummarizeRequest> = requests.into();
        let mut set: JoinSet<JobResult> = JoinSet::new();
        let mut running: HashMap<tokio::task::Id, String> = HashMap::new();

        loop {
            while !self.cancel.is_cancelled() && self.has_job_slot(set.len()) {
                let Some(req) = queue.pop_front() else { break };
                let id = req.id.clone();
                let handle = set.spawn(Job::new(self, req).run());
                running.insert(handle.id(), id);
            }

            if self.cancel.is_cancelled() {
                let reason = self.stop_reason();
                for req in queue.drain(..) {
                    on_done(self.stopped_result(req.id, &reason));
                }
            }

            let Some(joined) = set.join_next_with_id().await else {
                if queue.is_empty() {
                    break;
                }
                continue;
            };
            let result = match joined {
                Ok((task_id, result)) => {
                    running.remove(&task_id);
                    result
                }
                Err(e) => {
                    let id = running.remove(&e.id()).unwrap_or_default();
                    tracing::error!(%id, error = %e, "worker task failed");
                    let mut st = lock(&self.state);
                    st.stats.completed += 1;
                    st.stats.failed += 1;
                    JobResult {
                        id,
                        attempts: 0,
                        model_used: self.cfg.model.clone(),
                        outcome: Outcome::Fatal { reason: format!("worker task failed: {e}") },
                        usage: Usage::default(),
                    }
                }
            };
            on_done(result);
        }
        self.stats()
    }

    /// Whether another job may start given `active` jobs already spawned and unfinished.
    fn has_job_slot(&self, active: usize) -> bool {
        active < usize::from(lock(&self.state).stats.concurrency)
    }

    fn stop_reason(&self) -> String {
        lock(&self.state).stop_reason.clone().unwrap_or_else(|| "cancelled".to_owned())
    }

    fn stopped_result(&self, id: String, reason: &str) -> JobResult {
        let mut st = lock(&self.state);
        st.stats.completed += 1;
        st.stats.failed += 1;
        JobResult {
            id,
            attempts: 0,
            model_used: self.cfg.model.clone(),
            outcome: Outcome::Fatal { reason: format!("pool stopped: {reason}") },
            usage: Usage::default(),
        }
    }
}

/// An admitted backend attempt. Dropping it frees the in-flight slot and wakes waiters.
struct Slot {
    state: Arc<Mutex<State>>,
    slots: Arc<Notify>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        {
            let mut st = lock(&self.state);
            st.stats.in_flight = st.stats.in_flight.saturating_sub(1);
        }
        self.slots.notify_waiters();
    }
}

/// One request's retry loop. Owns clones of everything it needs so it can be spawned.
struct Job<B: Backend + ?Sized> {
    backend: Arc<B>,
    cfg: Arc<PoolConfig>,
    cancel: CancellationToken,
    state: Arc<Mutex<State>>,
    slots: Arc<Notify>,
    req: SummarizeRequest,
    attempts: u8,
    usage: Usage,
    retries_used: u8,
    rate_limit_retries: usize,
    escalated: bool,
    model: String,
}

/// What the loop should do after classifying one attempt.
#[expect(clippy::large_enum_variant, reason = "one per attempt, never stored")]
enum Step {
    Retry,
    Finish(Outcome),
}

impl<B: Backend + ?Sized + 'static> Job<B> {
    fn new(pool: &Pool<B>, req: SummarizeRequest) -> Self {
        Self {
            backend: Arc::clone(&pool.backend),
            cfg: Arc::clone(&pool.cfg),
            cancel: pool.cancel.clone(),
            state: Arc::clone(&pool.state),
            slots: Arc::clone(&pool.slots),
            req,
            attempts: 0,
            usage: Usage::default(),
            retries_used: 0,
            rate_limit_retries: 0,
            escalated: false,
            model: pool.cfg.model.clone(),
        }
    }

    async fn run(mut self) -> JobResult {
        loop {
            if self.cancel.is_cancelled() {
                return self.finish(self.stopped());
            }
            let admit = self.acquire_slot();
            let slot = tokio::select! {
                () = self.cancel.cancelled() => return self.finish(self.stopped()),
                slot = admit => slot,
            };
            self.attempts = self.attempts.saturating_add(1);
            let call = self.backend.summarize(&self.req, &self.model);
            let outcome = tokio::select! {
                () = self.cancel.cancelled() => return self.finish(self.stopped()),
                res = call => res,
            };
            // Free the slot before classifying: a backoff sleep must not hold it.
            drop(slot);
            let outcome = match outcome {
                Ok(o) => o,
                Err(e) => {
                    tracing::warn!(id = %self.req.id, error = %e, "backend error");
                    return self.finish(Outcome::Fatal { reason: e.to_string() });
                }
            };
            tracing::debug!(id = %self.req.id, attempt = self.attempts, model = %self.model, kind = outcome.kind(), "attempt done");
            if let Some(u) = outcome.usage() {
                lock(&self.state).stats.usage += u;
                self.usage += u;
            }
            match self.classify(outcome).await {
                Step::Retry => {}
                Step::Finish(o) => return self.finish(o),
            }
        }
    }

    /// Wait until a backend call may start under the concurrency limit current at that
    /// moment, then take the slot. Registers for the wake-up before checking, so a release
    /// between the check and the wait is never missed.
    async fn acquire_slot(&self) -> Slot {
        loop {
            let notified = self.slots.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut st = lock(&self.state);
                if st.stats.in_flight < st.stats.concurrency {
                    st.stats.in_flight += 1;
                    return Slot { state: Arc::clone(&self.state), slots: Arc::clone(&self.slots) };
                }
            }
            notified.await;
        }
    }

    async fn classify(&mut self, outcome: Outcome) -> Step {
        match outcome {
            Outcome::Ok { .. } => {
                let grew = {
                    let mut st = lock(&self.state);
                    st.consecutive_ok += 1;
                    let mut grew = false;
                    if st.consecutive_ok >= AIMD_SUCCESS_WINDOW {
                        st.consecutive_ok = 0;
                        if st.stats.concurrency < self.cfg.max_concurrency {
                            st.stats.concurrency += 1;
                            grew = true;
                            tracing::info!(
                                concurrency = st.stats.concurrency,
                                "AIMD: increased concurrency"
                            );
                        }
                    }
                    grew
                };
                if grew {
                    self.slots.notify_waiters();
                }
                Step::Finish(outcome)
            }
            Outcome::Retryable { .. } | Outcome::Malformed { .. } => {
                lock(&self.state).consecutive_ok = 0;
                if self.retries_used < self.cfg.max_retries {
                    self.retries_used += 1;
                    tracing::info!(id = %self.req.id, retry = self.retries_used, kind = outcome.kind(), "retrying");
                    return Step::Retry;
                }
                if !self.escalated
                    && let Some(esc) = &self.cfg.escalation_model
                {
                    self.escalated = true;
                    self.model.clone_from(esc);
                    tracing::info!(id = %self.req.id, model = %self.model, "escalating");
                    return Step::Retry;
                }
                Step::Finish(outcome)
            }
            Outcome::RateLimited { ref reason } => {
                let step = {
                    let mut st = lock(&self.state);
                    st.stats.rate_limit_events += 1;
                    st.consecutive_ok = 0;
                    st.stats.concurrency = (st.stats.concurrency / 2).max(1);
                    tracing::warn!(id = %self.req.id, concurrency = st.stats.concurrency, %reason, "rate limited; halved concurrency");
                    self.cfg.backoff.get(self.rate_limit_retries).copied()
                };
                let Some(delay) = step else { return Step::Finish(outcome) };
                self.rate_limit_retries += 1;
                tokio::select! {
                    () = self.cancel.cancelled() => Step::Finish(self.stopped()),
                    () = tokio::time::sleep(delay) => Step::Retry,
                }
            }
            Outcome::Fatal { ref reason } => {
                if outcome.stops_pool() {
                    tracing::error!(id = %self.req.id, %reason, "stopping pool");
                    lock(&self.state).stop_reason.get_or_insert_with(|| reason.clone());
                    self.cancel.cancel();
                }
                Step::Finish(outcome)
            }
        }
    }

    fn stopped(&self) -> Outcome {
        let reason =
            lock(&self.state).stop_reason.clone().unwrap_or_else(|| "cancelled".to_owned());
        Outcome::Fatal { reason: format!("pool stopped: {reason}") }
    }

    fn finish(&self, outcome: Outcome) -> JobResult {
        {
            let mut st = lock(&self.state);
            st.stats.completed += 1;
            if matches!(outcome, Outcome::Ok { .. }) {
                st.stats.ok += 1;
            } else {
                st.stats.failed += 1;
            }
        }
        JobResult {
            id: self.req.id.clone(),
            attempts: self.attempts,
            model_used: self.model.clone(),
            outcome,
            usage: self.usage.clone(),
        }
    }
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
