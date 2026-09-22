//! The long-running process behind `mda start`: it keeps the index of one root live.
//!
//! ```text
//!  notify ─► Debouncer ─► indexer task (Engine A): sync_path / index_root, rename detection,
//!                              hot set  ──wake──►  summarizer task (Engine B): bounded rounds
//!                                                   of summarize_pending, hot paths first
//!  local socket ─► server task: status / stop / pause / resume / index / rescan / watch
//! ```
//!
//! Decisions and their reasons are in ADR-0003. The two engines open the same SQLite file;
//! WAL plus the store's busy timeout serialise their short transactions. Every command
//! (`mda index`, `mda open`) can still write from another process for the same reason.
//!
//! [`run`] returns when the token is cancelled or a [`Request::Stop`] arrives. In-flight
//! model calls finish and are recorded; everything else stays pending for the next start.

pub mod hot;
pub mod ipc;
pub mod watch;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub use hot::HotSet;
pub use ipc::{
    Client, DaemonInfo, Request, Response, Server, SocketLocation, info_path, is_running, pid_path,
    socket_location,
};
pub use watch::{Batch, Debouncer, Hint, HintKind, Watcher, classify};

use crate::pipeline::{Engine, IndexReport, Progress, SummarizeOptions, SyncOutcome};
use crate::worker::DynBackend;
use crate::{Error, Result};

/// Tunables of the daemon loop. Defaults are what `mda start` uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// A path must be quiet this long (and its size stable) before it is indexed.
    pub debounce: Duration,
    /// Sections per summarization round. `None` means twice the pool's maximum concurrency.
    pub round_size: Option<usize>,
    /// How often the summarizer re-checks the store while nothing is pending.
    pub idle_poll: Duration,
    /// How long a document stays "hot" (served first) after the user changed it.
    pub hot_ttl: Duration,
    /// First pause after a round in which nothing succeeded; doubles each time.
    pub backoff_min: Duration,
    /// Longest pause between failing rounds.
    pub backoff_max: Duration,
    /// Pause after the daily token budget stopped a round.
    pub budget_pause: Duration,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_secs(1),
            round_size: None,
            idle_poll: Duration::from_secs(30),
            hot_ttl: Duration::from_secs(600),
            backoff_min: Duration::from_secs(5),
            backoff_max: Duration::from_secs(300),
            budget_pause: Duration::from_secs(600),
        }
    }
}

/// Live counters, as `mda status` shows them while the daemon runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveStatus {
    /// Process id.
    pub pid: u32,
    /// `mda` version.
    pub version: String,
    /// Watched root, canonical.
    pub root: String,
    /// When the daemon started.
    pub started_at: Timestamp,
    /// Seconds since start.
    pub uptime_secs: u64,
    /// Where the control socket is.
    pub socket: String,
    /// Summarization is paused (`mda pause`); indexing continues.
    pub paused: bool,
    /// The filesystem watcher is active.
    pub watching: bool,
    /// Why the watcher is not active, if it is not.
    pub watcher_error: Option<String>,
    /// Files (re)indexed from watcher hints or `mda index`.
    pub synced: u64,
    /// Documents tombstoned because their file disappeared.
    pub tombstoned: u64,
    /// Renames detected (same content at a new path).
    pub renamed: u64,
    /// Full reconciles of the root.
    pub rescans: u64,
    /// Summarization rounds run.
    pub rounds: u64,
    /// Cards attached since start (model and deterministic).
    pub cards: u64,
    /// Sections whose summarization failed since start.
    pub failures: u64,
    /// Spend since start, in US dollars.
    pub cost_usd: f64,
    /// Seconds the summarizer is currently backing off, `0` when it is not.
    pub backoff_secs: u64,
    /// Last error the daemon logged, if any.
    pub last_error: Option<String>,
    /// When the last summarization round finished.
    pub last_round_at: Option<Timestamp>,
    /// Documents currently served first, most recent first.
    pub hot_paths: Vec<String>,
}

/// What the daemon tells `mda watch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DaemonEvent {
    /// The daemon is up and watching.
    Started {
        /// Watched root.
        root: String,
    },
    /// A file was (re)indexed after a change.
    Indexed {
        /// Path relative to the root.
        rel_path: String,
        /// Sections in the file.
        sections: usize,
        /// Sections that now need a card.
        new_hashes: usize,
        /// The file was new to the index.
        created: bool,
    },
    /// A document's file disappeared.
    Tombstoned {
        /// Path relative to the root.
        rel_path: String,
    },
    /// A document moved.
    Renamed {
        /// Old path.
        from: String,
        /// New path.
        to: String,
    },
    /// The root was reconciled with the walker.
    Rescanned {
        /// Files seen.
        files: usize,
        /// Files changed or new.
        changed: usize,
        /// Documents tombstoned.
        tombstoned: usize,
    },
    /// A summarization round began.
    RoundStarted {
        /// Sections pending before the round.
        pending: u64,
        /// Hot documents served first.
        hot: usize,
    },
    /// One section finished in the current round.
    Progress(Progress),
    /// A summarization round ended.
    RoundFinished {
        /// Cards attached (model calls).
        ok: usize,
        /// Heading-only cards attached without a call.
        deterministic: usize,
        /// Sections that failed.
        failed: usize,
        /// Sections left pending for a later round.
        deferred: usize,
        /// Spend of this round.
        cost_usd: f64,
    },
    /// The summarizer is pausing itself.
    Backoff {
        /// For how long.
        secs: u64,
        /// Why.
        reason: String,
    },
    /// Summarization paused by request.
    Paused,
    /// Summarization resumed by request.
    Resumed,
    /// Something failed; the daemon keeps running.
    Error {
        /// What.
        message: String,
    },
    /// The daemon is shutting down.
    Stopping,
}

#[derive(Debug, Default)]
struct Live {
    watcher_error: Option<String>,
    synced: u64,
    tombstoned: u64,
    renamed: u64,
    rescans: u64,
    rounds: u64,
    cards: u64,
    failures: u64,
    cost_usd: f64,
    backoff_secs: u64,
    last_error: Option<String>,
    last_round_at: Option<Timestamp>,
}

/// State every task can reach.
struct Shared {
    info: DaemonInfo,
    started: Instant,
    live: Mutex<Live>,
    hot: Mutex<HotSet>,
    events: broadcast::Sender<DaemonEvent>,
    wake: Notify,
    paused: AtomicBool,
    cancel: CancellationToken,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn publish(&self, event: DaemonEvent) {
        tracing::debug!(?event, "event");
        let _ = self.events.send(event);
    }

    fn error(&self, context: &str, err: &dyn std::fmt::Display) {
        let message = format!("{context}: {err}");
        tracing::error!(%message);
        lock(&self.live).last_error = Some(message.clone());
        self.publish(DaemonEvent::Error { message });
    }

    fn status(&self) -> LiveStatus {
        let live = lock(&self.live);
        let hot_paths = lock(&self.hot).snapshot(Instant::now());
        LiveStatus {
            pid: self.info.pid,
            version: self.info.version.clone(),
            root: self.info.root.clone(),
            started_at: self.info.started_at,
            uptime_secs: self.started.elapsed().as_secs(),
            socket: self.info.socket.clone(),
            paused: self.paused.load(Ordering::Relaxed),
            watching: live.watcher_error.is_none(),
            watcher_error: live.watcher_error.clone(),
            synced: live.synced,
            tombstoned: live.tombstoned,
            renamed: live.renamed,
            rescans: live.rescans,
            rounds: live.rounds,
            cards: live.cards,
            failures: live.failures,
            cost_usd: live.cost_usd,
            backoff_secs: live.backoff_secs,
            last_error: live.last_error.clone(),
            last_round_at: live.last_round_at,
            hot_paths,
        }
    }
}

/// What the server asks the indexer to do on a client's behalf.
enum IndexerCmd {
    Index { path: Option<String>, reply: oneshot::Sender<Result<IndexReport>> },
    Rescan { reply: oneshot::Sender<Result<IndexReport>> },
}

/// Run the daemon for `root` until `cancel` fires or a client sends [`Request::Stop`].
///
/// Fails before doing anything if another daemon answers on the root's socket.
pub async fn run(
    root: &Path,
    backend: DynBackend,
    dcfg: DaemonConfig,
    cancel: CancellationToken,
) -> Result<()> {
    let root = root.canonicalize().map_err(|e| Error::io(root, e))?;
    let indexer_engine = Engine::open(&root)?;
    let summarizer_engine = Engine::open(&root)?;
    let server = Server::bind(&root).await?;

    let info = DaemonInfo {
        pid: std::process::id(),
        version: crate::VERSION.to_owned(),
        started_at: Timestamp::now(),
        root: root.display().to_string(),
        socket: server.location().to_string(),
    };
    info.write(&root)?;
    let (events, _) = broadcast::channel(256);
    let shared = Arc::new(Shared {
        info,
        started: Instant::now(),
        live: Mutex::new(Live::default()),
        hot: Mutex::new(HotSet::new(dcfg.hot_ttl)),
        events,
        wake: Notify::new(),
        paused: AtomicBool::new(false),
        cancel: cancel.clone(),
    });

    // The watcher is armed before the initial scan so nothing written meanwhile is missed.
    let (watcher, hints) = match Watcher::start(&root) {
        Ok((w, rx)) => (Some(w), Some(rx)),
        Err(e) => {
            shared.error("watcher", &e);
            lock(&shared.live).watcher_error = Some(e.to_string());
            (None, None)
        }
    };

    let (cmd_tx, cmd_rx) = mpsc::channel(32);
    let indexer = tokio::spawn(indexer_loop(
        indexer_engine,
        hints,
        cmd_rx,
        Arc::clone(&shared),
        dcfg.clone(),
    ));
    let summarizer =
        tokio::spawn(summarizer_loop(summarizer_engine, backend, Arc::clone(&shared), dcfg));
    let server_task = tokio::spawn(serve(server, cmd_tx, Arc::clone(&shared)));
    shared.publish(DaemonEvent::Started { root: root.display().to_string() });
    tracing::info!(root = %root.display(), pid = shared.info.pid, "daemon started");

    cancel.cancelled().await;
    tracing::info!("daemon stopping");
    let _ = indexer.await;
    let _ = summarizer.await;
    server_task.abort();
    drop(watcher);
    DaemonInfo::remove(&root);
    tracing::info!("daemon stopped");
    Ok(())
}

async fn recv_hint(rx: &mut Option<mpsc::UnboundedReceiver<Hint>>) -> Option<Hint> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn indexer_loop(
    mut engine: Engine,
    mut hints: Option<mpsc::UnboundedReceiver<Hint>>,
    mut cmds: mpsc::Receiver<IndexerCmd>,
    shared: Arc<Shared>,
    dcfg: DaemonConfig,
) {
    let root = engine.root().to_path_buf();
    let mut deb = Debouncer::new(dcfg.debounce);
    let _ = rescan(&mut engine, &shared);
    loop {
        let deadline = deb.next_deadline();
        let sleep_to =
            deadline.map_or_else(tokio::time::Instant::now, tokio::time::Instant::from_std);
        tokio::select! {
            () = shared.cancel.cancelled() => break,
            cmd = cmds.recv() => match cmd {
                Some(cmd) => handle_cmd(&mut engine, cmd, &shared),
                None => break,
            },
            hint = recv_hint(&mut hints) => {
                if let Some(h) = hint {
                    match classify(&root, &h.path, h.rescan) {
                        HintKind::Markdown => deb.push(h.path, h.at),
                        HintKind::Structural => deb.push_rescan(h.at),
                        HintKind::Ignore => {}
                    }
                } else {
                    lock(&shared.live).watcher_error = Some("watcher channel closed".to_owned());
                    hints = None;
                }
            },
            () = tokio::time::sleep_until(sleep_to), if deadline.is_some() => {
                let batch = deb.due(Instant::now());
                apply_batch(&mut engine, batch, &shared);
            }
        }
    }
}

fn handle_cmd(engine: &mut Engine, cmd: IndexerCmd, shared: &Arc<Shared>) {
    match cmd {
        IndexerCmd::Rescan { reply } | IndexerCmd::Index { path: None, reply } => {
            let _ = reply.send(rescan(engine, shared));
        }
        IndexerCmd::Index { path: Some(p), reply } => {
            let p = PathBuf::from(p);
            let abs = if p.is_absolute() { p } else { engine.root().join(p) };
            let result = engine.sync_path(&abs).map(|out| {
                let mut report = IndexReport { files: 1, ..IndexReport::default() };
                if let SyncOutcome::Indexed(out) = out {
                    lock(&shared.live).synced += 1;
                    if out.upsert.created || out.upsert.changed {
                        report.changed = 1;
                        lock(&shared.hot).touch(&out.rel_path, Instant::now());
                    }
                    report.pending = out.upsert.new_hashes.len();
                    if report.pending > 0 {
                        shared.wake.notify_one();
                    }
                }
                report
            });
            let _ = reply.send(result);
        }
    }
}

fn rescan(engine: &mut Engine, shared: &Arc<Shared>) -> Result<IndexReport> {
    let started = Instant::now();
    let report = engine.index_root();
    match &report {
        Ok(r) => {
            lock(&shared.live).rescans += 1;
            tracing::info!(
                files = r.files,
                changed = r.changed,
                pending = r.pending,
                tombstoned = r.tombstoned,
                ms = started.elapsed().as_millis(),
                "rescan"
            );
            shared.publish(DaemonEvent::Rescanned {
                files: r.files,
                changed: r.changed,
                tombstoned: r.tombstoned,
            });
            for (path, err) in &r.errors {
                tracing::warn!(path, err, "skipped during rescan");
            }
            // Pending sections may predate this daemon; always give the summarizer a look.
            shared.wake.notify_one();
        }
        Err(e) => shared.error("rescan", e),
    }
    report
}

fn apply_batch(engine: &mut Engine, batch: Batch, shared: &Arc<Shared>) {
    if batch.rescan {
        let _ = rescan(engine, shared);
    }
    let now = Instant::now();
    let (present, missing): (Vec<PathBuf>, Vec<PathBuf>) =
        batch.paths.into_iter().partition(|p| p.exists());

    // Files that exist first, so a rename's new path is known before its old path is judged.
    let mut created: Vec<(String, String)> = Vec::new();
    let mut wake = false;
    for path in present {
        match engine.sync_path(&path) {
            Ok(SyncOutcome::Indexed(out)) => {
                lock(&shared.live).synced += 1;
                if out.upsert.created || out.upsert.changed {
                    lock(&shared.hot).touch(&out.rel_path, now);
                    if out.upsert.created
                        && let Ok(Some(hash)) = engine.store().document_hash(&out.rel_path)
                    {
                        created.push((out.rel_path.clone(), hash));
                    }
                    shared.publish(DaemonEvent::Indexed {
                        rel_path: out.rel_path,
                        sections: out.sections,
                        new_hashes: out.upsert.new_hashes.len(),
                        created: out.upsert.created,
                    });
                }
                wake |= !out.upsert.new_hashes.is_empty();
            }
            Ok(SyncOutcome::Tombstoned { rel_path, .. }) => {
                lock(&shared.live).tombstoned += 1;
                shared.publish(DaemonEvent::Tombstoned { rel_path });
            }
            Ok(SyncOutcome::Ignored { reason }) => {
                tracing::debug!(path = %path.display(), reason, "ignored");
            }
            Err(e) => shared.error(&format!("indexing {}", path.display()), &e),
        }
    }
    for path in missing {
        let Ok(rel) = engine.rel_path(&path) else { continue };
        if let Ok(Some(hash)) = engine.store().document_hash(&rel)
            && let Some((to, _)) = created.iter().find(|(_, h)| *h == hash)
        {
            match engine.store_mut().note_rename(&rel, to, Timestamp::now()) {
                Ok(true) => {
                    lock(&shared.live).renamed += 1;
                    shared.publish(DaemonEvent::Renamed { from: rel, to: to.clone() });
                    continue;
                }
                Ok(false) => {}
                Err(e) => shared.error(&format!("recording rename of {rel}"), &e),
            }
        }
        match engine.sync_path(&path) {
            Ok(SyncOutcome::Tombstoned { rel_path, .. }) => {
                lock(&shared.live).tombstoned += 1;
                shared.publish(DaemonEvent::Tombstoned { rel_path });
            }
            Ok(SyncOutcome::Indexed(_)) => wake = true, // reappeared between the two loops
            Ok(SyncOutcome::Ignored { .. }) => {}
            Err(e) => shared.error(&format!("syncing {}", path.display()), &e),
        }
    }
    if wake {
        shared.wake.notify_one();
    }
}

async fn wait_wake_or(shared: &Shared, timeout: Duration) {
    tokio::select! {
        () = shared.cancel.cancelled() => {}
        () = shared.wake.notified() => {}
        () = tokio::time::sleep(timeout) => {}
    }
}

async fn sleep_cancellable(shared: &Shared, d: Duration) {
    tokio::select! {
        () = shared.cancel.cancelled() => {}
        () = tokio::time::sleep(d) => {}
    }
}

async fn summarizer_loop(
    mut engine: Engine,
    backend: DynBackend,
    shared: Arc<Shared>,
    dcfg: DaemonConfig,
) {
    let round_size =
        dcfg.round_size.unwrap_or_else(|| usize::from(engine.config().pool_bounds().1) * 2).max(1);
    let mut backoff: Option<Duration> = None;
    let next_backoff = |b: Option<Duration>| match b {
        None => dcfg.backoff_min,
        Some(b) => (b * 2).min(dcfg.backoff_max),
    };
    loop {
        if shared.cancel.is_cancelled() {
            break;
        }
        if shared.paused.load(Ordering::Relaxed) {
            wait_wake_or(&shared, dcfg.idle_poll).await;
            continue;
        }
        if let Some(b) = backoff.take() {
            lock(&shared.live).backoff_secs = b.as_secs();
            sleep_cancellable(&shared, b).await;
            lock(&shared.live).backoff_secs = 0;
            // The doubled value only applies if the next round fails again.
            backoff = Some(b);
        }
        let pending = match engine.store().counts() {
            Ok(c) => c.pending,
            Err(e) => {
                shared.error("reading counts", &e);
                backoff = Some(next_backoff(backoff));
                continue;
            }
        };
        if pending == 0 {
            backoff = None;
            wait_wake_or(&shared, dcfg.idle_poll).await;
            continue;
        }
        let hot_paths = lock(&shared.hot).snapshot(Instant::now());
        shared.publish(DaemonEvent::RoundStarted { pending, hot: hot_paths.len() });
        lock(&shared.live).rounds += 1;
        let events = shared.events.clone();
        let opts = SummarizeOptions { limit: Some(round_size), hot_paths };
        let result = engine
            .summarize_pending(Arc::clone(&backend), shared.cancel.child_token(), opts, move |p| {
                let _ = events.send(DaemonEvent::Progress(p.clone()));
            })
            .await;
        match result {
            Ok(r) => {
                {
                    let mut live = lock(&shared.live);
                    live.cards += (r.ok + r.deterministic) as u64;
                    live.failures += r.failed as u64;
                    live.cost_usd += r.pool.usage.cost_usd;
                    live.last_round_at = Some(Timestamp::now());
                }
                shared.publish(DaemonEvent::RoundFinished {
                    ok: r.ok,
                    deterministic: r.deterministic,
                    failed: r.failed,
                    deferred: r.deferred,
                    cost_usd: r.pool.usage.cost_usd,
                });
                if r.budget_exhausted {
                    backoff = Some(dcfg.budget_pause);
                    shared.publish(DaemonEvent::Backoff {
                        secs: dcfg.budget_pause.as_secs(),
                        reason: "daily token budget reached".to_owned(),
                    });
                } else if r.submitted > 0 && r.ok == 0 && r.failed > 0 {
                    // Every call failed: a bad key, a dead server, or an outage. Do not spin.
                    let b = next_backoff(backoff);
                    backoff = Some(b);
                    shared.publish(DaemonEvent::Backoff {
                        secs: b.as_secs(),
                        reason: format!("{} of {} sections failed", r.failed, r.submitted),
                    });
                } else {
                    backoff = None;
                }
            }
            Err(e) => {
                shared.error("summarization round", &e);
                let b = next_backoff(backoff);
                backoff = Some(b);
                shared.publish(DaemonEvent::Backoff { secs: b.as_secs(), reason: e.to_string() });
            }
        }
    }
}

async fn serve(server: Server, cmds: mpsc::Sender<IndexerCmd>, shared: Arc<Shared>) {
    loop {
        let conn = tokio::select! {
            () = shared.cancel.cancelled() => break,
            c = server.accept() => c,
        };
        match conn {
            Ok(conn) => {
                tokio::spawn(handle(conn, cmds.clone(), Arc::clone(&shared)));
            }
            Err(e) => {
                tracing::warn!(error = %e, "accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle(mut conn: ipc::Connection, cmds: mpsc::Sender<IndexerCmd>, shared: Arc<Shared>) {
    loop {
        let req = tokio::select! {
            () = shared.cancel.cancelled() => return,
            r = conn.read() => r,
        };
        let req = match req {
            Ok(Some(r)) => r,
            Ok(None) => return,
            Err(e) => {
                let _ = conn.write(&Response::Error { message: e.to_string() }).await;
                return;
            }
        };
        let resp = match req {
            Request::Ping => Response::Ok,
            Request::Status => Response::Status(Box::new(shared.status())),
            Request::Stop => {
                shared.publish(DaemonEvent::Stopping);
                shared.cancel.cancel();
                Response::Ok
            }
            Request::Pause => {
                shared.paused.store(true, Ordering::Relaxed);
                shared.publish(DaemonEvent::Paused);
                Response::Ok
            }
            Request::Resume => {
                shared.paused.store(false, Ordering::Relaxed);
                shared.publish(DaemonEvent::Resumed);
                shared.wake.notify_one();
                Response::Ok
            }
            Request::Index { path } => {
                let (reply, rx) = oneshot::channel();
                relay(&cmds, IndexerCmd::Index { path, reply }, rx).await
            }
            Request::Rescan => {
                let (reply, rx) = oneshot::channel();
                relay(&cmds, IndexerCmd::Rescan { reply }, rx).await
            }
            Request::Watch => {
                if conn.write(&Response::Ok).await.is_err() {
                    return;
                }
                let mut rx = shared.events.subscribe();
                loop {
                    let ev = tokio::select! {
                        () = shared.cancel.cancelled() => {
                            let _ = conn.write(&Response::Event(DaemonEvent::Stopping)).await;
                            return;
                        }
                        ev = rx.recv() => ev,
                    };
                    match ev {
                        Ok(ev) => {
                            if conn.write(&Response::Event(ev)).await.is_err() {
                                return;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::debug!(skipped = n, "watch client lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            }
        };
        if conn.write(&resp).await.is_err() {
            return;
        }
    }
}

async fn relay(
    cmds: &mpsc::Sender<IndexerCmd>,
    cmd: IndexerCmd,
    rx: oneshot::Receiver<Result<IndexReport>>,
) -> Response {
    if cmds.send(cmd).await.is_err() {
        return Response::Error { message: "indexer is not running".to_owned() };
    }
    match rx.await {
        Ok(Ok(report)) => Response::Indexed(Box::new(report)),
        Ok(Err(e)) => Response::Error { message: e.to_string() },
        Err(_) => Response::Error { message: "indexer stopped before answering".to_owned() },
    }
}

#[cfg(test)]
mod tests;
