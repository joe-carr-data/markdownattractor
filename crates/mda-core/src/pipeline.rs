//! The engine: wires walk → parse → store → plan → worker → validate → store.
//!
//! [`Engine`] owns the store and the config for one watched root. Everything the CLI and the
//! daemon do goes through it, so the two can never disagree about semantics.
//!
//! Indexing is two-phase by design (plan §5, G1):
//!
//! 1. [`Engine::index_file`] parses and upserts. The section text is raw-searchable the moment
//!    this returns, no model involved. It reports which section hashes still need a card.
//! 2. [`Engine::summarize_pending`] drains those hashes through a worker pool, validates every
//!    card, and attaches it. Cards are keyed by hash, so a section that already has one anywhere
//!    in the index never costs a model call.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::card::{Provenance, SCHEMA_VERSION};
use crate::config::{Config, STATE_DIR};
use crate::embed::{EMBED_BATCH, Embedder, embed_text};
use crate::markdown::{self, Document};
use crate::planner::{PlanConfig, chunk_text};
use crate::store::{DocTimes, PendingSection, Store, UpsertOutcome, Usage as StoredUsage};
use crate::validate::{Caps, validate};
use crate::worker::{Backend, JobResult, Outcome, Pool, PoolConfig, PoolStats, SummarizeRequest};
use crate::{Error, Result};

/// File name of the SQLite database under [`STATE_DIR`].
pub const INDEX_FILE: &str = "index.sqlite";

/// One watched root: its config and its store.
pub struct Engine {
    root: PathBuf,
    config: Config,
    store: Store,
}

/// What indexing one file did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexOutcome {
    /// Path relative to the root, forward slashes.
    pub rel_path: String,
    /// Store-level result.
    pub upsert: UpsertOutcome,
    /// Sections in the parsed document.
    pub sections: usize,
}

/// What indexing the whole root did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexReport {
    /// Files seen by the walker.
    pub files: usize,
    /// Files whose content changed (or that were new).
    pub changed: usize,
    /// Section hashes that now need a card.
    pub pending: usize,
    /// Documents that disappeared and were tombstoned.
    pub tombstoned: usize,
    /// Files that could not be read or parsed, with the reason.
    pub errors: Vec<(String, String)>,
}

/// Progress event emitted while summarizing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// Jobs finished so far (ok + failed).
    pub done: usize,
    /// Jobs in this run.
    pub total: usize,
    /// Cards attached.
    pub ok: usize,
    /// Jobs that ended in failure.
    pub failed: usize,
    /// The section that just finished, for display.
    pub last: Option<String>,
}

/// Knobs for one summarization run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummarizeOptions {
    /// Summarize at most this many sections this run (smallest first). `None` = all pending.
    pub limit: Option<usize>,
    /// Relative paths whose sections go first, in this order (most urgent first); the daemon
    /// puts files the user just saved here so an edit never waits behind a backfill. Within
    /// a document, and for everything else, smallest sections still go first.
    pub hot_paths: Vec<String>,
    /// Start the pool at this concurrency instead of the configured initial value (clamped to
    /// the configured maximum). The daemon feeds the previous round's final concurrency back
    /// so AIMD does not restart from scratch every round.
    pub initial_concurrency: Option<u16>,
}

/// What [`Engine::sync_path`] did about one path the watcher reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncOutcome {
    /// The file exists and was (re)indexed.
    Indexed(IndexOutcome),
    /// The file is gone and its document was tombstoned.
    Tombstoned {
        /// Path relative to the root.
        rel_path: String,
        /// Content hash the document had, for rename detection.
        content_hash: String,
    },
    /// Nothing to do: not markdown, not inside the root, ignored, or never indexed.
    Ignored {
        /// Why.
        reason: String,
    },
}

/// Average tokens one section costs end to end (input + output), used to turn a token budget
/// into a section count before the run. Measured in the Phase 0 spike.
pub const TOKENS_PER_SECTION_ESTIMATE: u64 = 3_500;

/// Result of a summarization run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummarizeReport {
    /// Jobs submitted to the model.
    pub submitted: usize,
    /// Heading-only sections carded deterministically, without a model call.
    pub deterministic: usize,
    /// Pending sections left for a later run because of `limit` or the daily budget.
    pub deferred: usize,
    /// `true` when the daily token budget stopped this run from submitting everything.
    pub budget_exhausted: bool,
    /// Cards attached.
    pub ok: usize,
    /// Jobs that ended in failure (recorded in the store with a reason).
    pub failed: usize,
    /// Cards that passed validation with nothing dropped or trimmed.
    pub clean: usize,
    /// Total dates dropped by grounding across all cards.
    pub dropped_dates: usize,
    /// Total entities dropped by grounding across all cards.
    pub dropped_entities: usize,
    /// Pool statistics (usage, concurrency, rate-limit events).
    pub pool: PoolStats,
}

/// What one embedding pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedReport {
    /// Model the vectors were made with.
    pub model: String,
    /// Cards embedded in this pass.
    pub embedded: usize,
    /// Carded hashes still without a vector after the pass.
    pub remaining: u64,
    /// Wall-clock milliseconds.
    pub ms: u128,
}

/// A document whose index is not final: sections still waiting for a card, failed ones, or a
/// file whose content on disk no longer matches what was indexed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleDoc {
    /// Path relative to the root.
    pub rel_path: String,
    /// Sections waiting for a card.
    pub pending: u64,
    /// Sections whose last attempt failed.
    pub failed: u64,
    /// The file changed after it was indexed (parsed and compared by hash).
    pub changed_on_disk: bool,
    /// The file is gone but the document is still live (no rescan since).
    pub missing: bool,
}

/// A recently updated document, for `mda recent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentDoc {
    /// Path relative to the root.
    pub rel_path: String,
    /// First level-1 heading, if any.
    pub title: Option<String>,
    /// When the content last changed.
    pub updated_at: Timestamp,
    /// When the document was created (birth time or first seen).
    pub created_at: Timestamp,
    /// Sections in the document.
    pub sections: usize,
    /// Sections still without a card.
    pub pending: usize,
}

/// One row of `mda timeline`: a store event with its document's path resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// When it happened.
    pub at: Timestamp,
    /// What happened.
    pub kind: crate::store::EventKind,
    /// Path relative to the root (the current path of the document, tombstoned or not).
    pub rel_path: String,
    /// Section concerned, for section-level kinds.
    pub section_id: Option<String>,
    /// Free-form detail (old path of a rename, a failure reason).
    pub detail: Option<String>,
}

/// Exact source lines of a section, re-checked against the file at read time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opened {
    /// The id that was asked for.
    pub requested_id: String,
    /// The section's *current* id. Differs from `requested_id` when sections moved.
    pub section_id: String,
    /// Path relative to the root.
    pub rel_path: String,
    /// Headings down to the section.
    pub heading_path: Vec<String>,
    /// First line returned, 1-based.
    pub line_start: u32,
    /// Last line returned, 1-based, inclusive.
    pub line_end: u32,
    /// The lines, joined with `\n`, exactly as on disk.
    pub text: String,
    /// `true` when the file changed after it was indexed. The lines are still the *current*
    /// ones, and the file has been re-indexed as a side effect.
    pub stale: bool,
}

impl Engine {
    /// Open (or create) the engine for `root`: loads `config.toml` and opens the store.
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize().map_err(|e| Error::io(root, e))?;
        let config = Config::load(&root)?;
        let store = Store::open(&crate::config::state_dir(&root)?.join(INDEX_FILE))?;
        Ok(Self { root, config, store })
    }

    /// Build an engine from parts. Used by tests with an in-memory store.
    pub fn with_parts(root: PathBuf, config: Config, store: Store) -> Self {
        Self { root, config, store }
    }

    /// Where the database lives for a root.
    pub fn index_path(root: &Path) -> PathBuf {
        root.join(STATE_DIR).join(INDEX_FILE)
    }

    /// The watched root (canonical).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Active configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Read access to the store.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Write access to the store.
    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Path relative to the root with forward slashes, or an error if outside the root.
    pub fn rel_path(&self, abs: &Path) -> Result<String> {
        let rel = abs.strip_prefix(&self.root).map_err(|_| {
            Error::NotFound(format!(
                "{} is outside the watched root {}",
                abs.display(),
                self.root.display()
            ))
        })?;
        if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(Error::NotFound(format!(
                "refusing {}: path components must stay inside the root",
                abs.display()
            )));
        }
        Ok(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    }

    /// Join a stored relative path onto the root, refusing anything that could leave it:
    /// absolute paths, `..` components, and symlinks that resolve outside the root.
    pub fn safe_join(&self, rel_path: &str) -> Result<PathBuf> {
        use std::path::Component;
        let rel = Path::new(rel_path);
        if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
            return Err(Error::NotFound(format!("refusing {rel_path:?}: not inside the root")));
        }
        let joined = self.root.join(rel);
        let canon = joined.canonicalize().map_err(|e| Error::io(&joined, e))?;
        if !canon.starts_with(&self.root) {
            return Err(Error::NotFound(format!(
                "refusing {rel_path:?}: resolves outside the root"
            )));
        }
        Ok(canon)
    }

    /// Parse one file and upsert it. The file is raw-searchable when this returns.
    pub fn index_file(&mut self, abs: &Path) -> Result<IndexOutcome> {
        // Resolve symlinks first so a link pointing outside the root is rejected by `rel_path`.
        let abs = abs.canonicalize().map_err(|e| Error::io(abs, e))?;
        let rel_path = self.rel_path(&abs)?;
        let doc = markdown::parse_file(&abs)?;
        let times = file_times(&abs)?;
        let outcome = self.index_parsed(&rel_path, &doc, &times)?;
        Ok(outcome)
    }

    /// Upsert an already-parsed document (lets tests and the watcher skip the filesystem).
    ///
    /// A live document whose stored content hash equals `doc.hash` is left untouched: same
    /// content means the same sections at the same lines, so there is nothing to refresh.
    pub fn index_parsed(
        &mut self,
        rel_path: &str,
        doc: &Document,
        times: &DocTimes,
    ) -> Result<IndexOutcome> {
        if self.store.document_hash(rel_path)?.as_deref() == Some(doc.hash.as_str()) {
            tracing::debug!(path = rel_path, "unchanged");
            return Ok(IndexOutcome {
                rel_path: rel_path.to_owned(),
                upsert: UpsertOutcome {
                    doc_id: crate::store::doc_id_for(rel_path),
                    created: false,
                    changed: false,
                    new_hashes: Vec::new(),
                    reused: 0,
                    unchanged: doc.sections.len(),
                    removed: 0,
                },
                sections: doc.sections.len(),
            });
        }
        let upsert = self.store.upsert_document(rel_path, doc, times)?;
        tracing::info!(
            path = rel_path,
            new = upsert.new_hashes.len(),
            reused = upsert.reused,
            unchanged = upsert.unchanged,
            "indexed"
        );
        Ok(IndexOutcome { rel_path: rel_path.to_owned(), upsert, sections: doc.sections.len() })
    }

    /// Bring the index in line with one path the watcher reported: index it if it exists and
    /// the walker would discover it, tombstone it if it is gone, ignore everything else. Never
    /// trusts the event that named the path; the filesystem is the source of truth.
    pub fn sync_path(&mut self, abs: &Path) -> Result<SyncOutcome> {
        let ignored = |reason: &str| Ok(SyncOutcome::Ignored { reason: reason.to_owned() });
        if !crate::walk::is_markdown(abs) {
            return ignored("not markdown");
        }
        let Ok(rel_path) = self.rel_path(abs) else {
            return ignored("outside the root");
        };
        if rel_path.split('/').any(|c| c == STATE_DIR) {
            return ignored("state directory");
        }
        let known = self.store.document_hash(&rel_path)?;
        match std::fs::symlink_metadata(abs) {
            Ok(meta) if meta.file_type().is_file() => {
                // A file the index has never seen must pass the walker's ignore rules; a
                // known one already did.
                if known.is_none() {
                    let discoverable =
                        crate::walk::discover(&self.root, &self.config)?.iter().any(|p| p == abs);
                    if !discoverable {
                        return ignored("ignored by walker rules");
                    }
                }
                Ok(SyncOutcome::Indexed(self.index_file(abs)?))
            }
            Ok(_) => ignored("not a regular file"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match known {
                Some(content_hash) => {
                    self.store.tombstone(&rel_path, Timestamp::now())?;
                    Ok(SyncOutcome::Tombstoned { rel_path, content_hash })
                }
                None => ignored("never indexed"),
            },
            Err(e) => Err(Error::io(abs, e)),
        }
    }

    /// Walk the root and index every markdown file, smallest first, then tombstone documents
    /// the walker no longer finds: deleted files, and files that an ignore rule now excludes.
    /// A discovered file that fails to read or parse keeps its previous index.
    pub fn index_root(&mut self) -> Result<IndexReport> {
        let mut files = crate::walk::discover(&self.root, &self.config)?;
        // Small docs first: something is searchable within seconds of `start` (plan §4.3).
        files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));

        let mut report = IndexReport { files: files.len(), ..IndexReport::default() };
        // What the walker found is what the index should hold: membership is decided here,
        // lexically, so a file that vanishes mid-walk is judged next round, not now.
        let discovered: std::collections::HashSet<String> =
            files.iter().filter_map(|p| self.rel_path(p).ok()).collect();
        for path in &files {
            match self.index_file(path) {
                Ok(out) => {
                    if out.upsert.created || out.upsert.changed {
                        report.changed += 1;
                    }
                    report.pending += out.upsert.new_hashes.len();
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "skipping file");
                    report.errors.push((path.display().to_string(), e.to_string()));
                }
            }
        }

        let now = Timestamp::now();
        for doc in self.store.documents()? {
            if !discovered.contains(&doc.rel_path) && self.store.tombstone(&doc.rel_path, now)? {
                tracing::info!(path = doc.rel_path, "tombstoned: no longer discovered");
                report.tombstoned += 1;
            }
        }
        Ok(report)
    }

    /// Summarize every pending section through `backend`, validating and attaching each card.
    ///
    /// `on_progress` is called after every finished job. Cancelling `cancel` stops scheduling
    /// new jobs; in-flight ones finish and are recorded.
    pub async fn summarize_pending<B: Backend + ?Sized + 'static>(
        &mut self,
        backend: Arc<B>,
        cancel: CancellationToken,
        opts: SummarizeOptions,
        mut on_progress: impl FnMut(&Progress) + Send,
    ) -> Result<SummarizeReport> {
        let all_pending = self.store.pending_hashes(usize::MAX)?;

        // Heading-only sections have nothing for a model to read; give them a deterministic
        // card so they are searchable by heading and never cost a call.
        let (trivial, pending): (Vec<PendingSection>, Vec<PendingSection>) =
            all_pending.into_iter().partition(|p| is_heading_only(&p.text));
        let trivial_cards = self.card_heading_only(&trivial)?;

        let Round { pending, deferred, budget_exhausted } = self.select_round(pending, &opts)?;

        let total = pending.len();
        let plan_cfg = PlanConfig::default();
        let requests: Vec<SummarizeRequest> =
            pending.iter().map(|p| request_for(p, plan_cfg)).collect();
        let truncated: std::collections::HashSet<String> = requests
            .iter()
            .zip(&pending)
            .filter(|(r, p)| r.text.len() < p.text.len())
            .map(|(r, _)| r.id.clone())
            .collect();
        let by_hash: std::collections::HashMap<String, PendingSection> =
            pending.into_iter().map(|p| (p.section_hash.clone(), p)).collect();

        let mut pool_cfg = PoolConfig::from_config(&self.config);
        if let Some(c) = opts.initial_concurrency {
            pool_cfg.initial_concurrency = c.clamp(1, pool_cfg.max_concurrency);
        }
        let backend_name = backend.name().to_owned();
        let pool = Pool::new(backend, pool_cfg, cancel);

        // The pool calls back from its own tasks; collect results and apply them here so the
        // store (which is not Sync) is only touched from this task.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<JobResult>();
        let run = pool.run(requests, move |r| {
            let _ = tx.send(r);
        });
        tokio::pin!(run);

        let mut report = SummarizeReport {
            submitted: total,
            deterministic: trivial_cards,
            deferred,
            budget_exhausted,
            ok: 0,
            failed: 0,
            clean: 0,
            dropped_dates: 0,
            dropped_entities: 0,
            pool: PoolStats::default(),
        };
        let mut progress = Progress { done: 0, total, ok: 0, failed: 0, last: None };
        let caps = Caps::default();

        let stats = loop {
            tokio::select! {
                stats = &mut run => {
                    // Drain anything still queued after the pool finished.
                    while let Ok(r) = rx.try_recv() {
                        self.apply(&r, &by_hash, &truncated, &backend_name, &caps, &mut report, &mut progress, &mut on_progress)?;
                    }
                    break stats;
                }
                Some(r) = rx.recv() => {
                    self.apply(&r, &by_hash, &truncated, &backend_name, &caps, &mut report, &mut progress, &mut on_progress)?;
                }
            }
        };
        report.pool = stats;
        Ok(report)
    }

    /// Attach a deterministic card to every heading-only section. Returns how many.
    fn card_heading_only(&mut self, trivial: &[PendingSection]) -> Result<usize> {
        for p in trivial {
            let summary = synthetic_summary(p);
            let provenance = Provenance {
                model: "none".to_owned(),
                prompt_version: "deterministic".to_owned(),
                schema_version: SCHEMA_VERSION,
                backend: "deterministic".to_owned(),
                summarized_at: Timestamp::now(),
                truncated: false,
            };
            self.store.attach_summary(
                &p.section_hash,
                &summary,
                &provenance,
                &StoredUsage::default(),
            )?;
        }
        if !trivial.is_empty() {
            tracing::info!(
                count = trivial.len(),
                "heading-only sections carded without a model call"
            );
        }
        Ok(trivial.len())
    }

    /// Order the pending sections (hot documents first, then smallest first) and cut the
    /// list to what this run may submit: the explicit limit, then the daily token budget
    /// (tokens already spent today, divided by the measured per-section cost).
    fn select_round(
        &self,
        mut pending: Vec<PendingSection>,
        opts: &SummarizeOptions,
    ) -> Result<Round> {
        let mut cap = opts.limit.unwrap_or(usize::MAX);
        let mut budget_exhausted = false;
        if let Some(budget) = self.config.daily_token_budget {
            let spent = self.store.usage_since(start_of_today())?;
            let remaining = budget.saturating_sub(spent.input_tokens + spent.output_tokens);
            let affordable =
                usize::try_from(remaining / TOKENS_PER_SECTION_ESTIMATE).unwrap_or(usize::MAX);
            // The budget "exhausted" a run only when it, not the explicit limit, is what cut
            // it: a bounded daemon round over a large backlog is not a budget problem.
            if affordable < pending.len().min(cap) {
                budget_exhausted = true;
                tracing::warn!(
                    budget,
                    remaining,
                    affordable,
                    pending = pending.len(),
                    "daily token budget caps this run"
                );
            }
            cap = cap.min(affordable);
        }
        // `pending_hashes` sorted by size and the sort is stable, so smallest-first holds
        // inside every tier.
        if !opts.hot_paths.is_empty() {
            pending.sort_by_key(|p| {
                opts.hot_paths.iter().position(|h| *h == p.rel_path).unwrap_or(usize::MAX)
            });
        }
        let deferred = pending.len().saturating_sub(cap);
        pending.truncate(cap);
        Ok(Round { pending, deferred, budget_exhausted })
    }

    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        r: &JobResult,
        by_hash: &std::collections::HashMap<String, PendingSection>,
        truncated: &std::collections::HashSet<String>,
        backend_name: &str,
        caps: &Caps,
        report: &mut SummarizeReport,
        progress: &mut Progress,
        on_progress: &mut (impl FnMut(&Progress) + Send),
    ) -> Result<()> {
        let Some(p) = by_hash.get(&r.id) else {
            tracing::warn!(id = %r.id, "pool returned an unknown job id");
            return Ok(());
        };
        // Every job's spend goes to the ledger, whatever happened, so budgets see it.
        let spent = StoredUsage {
            input_tokens: r.usage.input_tokens,
            output_tokens: r.usage.output_tokens,
            cost_usd: r.usage.cost_usd,
        };
        self.store.record_usage(&r.id, &r.model_used, &spent, r.outcome.kind())?;
        if r.was_stopped() {
            // Never started, cut off by a stop or cancel, or an environment failure that
            // stopped the pool (bad key, dead server): the section is fine, the run was not.
            // It stays pending for the next run instead of being marked failed.
            report.deferred += 1;
            return Ok(());
        }
        match &r.outcome {
            Outcome::Ok { summary, usage } => match validate(&p.text, summary.clone(), caps) {
                Ok(v) => {
                    let provenance = Provenance {
                        model: usage.model.clone(),
                        prompt_version: crate::worker::PROMPT_VERSION.to_owned(),
                        schema_version: SCHEMA_VERSION,
                        backend: backend_name.to_owned(),
                        summarized_at: Timestamp::now(),
                        truncated: truncated.contains(&r.id),
                    };
                    let stored_usage = StoredUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cost_usd: usage.cost_usd,
                    };
                    self.store.attach_summary(&r.id, &v.summary, &provenance, &stored_usage)?;
                    report.ok += 1;
                    progress.ok += 1;
                    if v.is_clean() {
                        report.clean += 1;
                    }
                    report.dropped_dates += v.dropped_dates.len();
                    report.dropped_entities += v.dropped_entities.len();
                }
                Err(e) => {
                    self.store.mark_failed(&r.id, &e.to_string())?;
                    report.failed += 1;
                    progress.failed += 1;
                }
            },
            Outcome::Malformed { reason, raw, .. } => {
                let tail: String = raw.chars().take(300).collect();
                self.store.mark_failed(&r.id, &format!("{reason}; model said: {tail}"))?;
                report.failed += 1;
                progress.failed += 1;
            }
            Outcome::Retryable { reason }
            | Outcome::RateLimited { reason }
            | Outcome::Fatal { reason } => {
                self.store.mark_failed(&r.id, reason)?;
                report.failed += 1;
                progress.failed += 1;
            }
        }
        progress.done += 1;
        progress.last = Some(format!("{} › {}", p.rel_path, p.heading_path.join(" › ")));
        on_progress(progress);
        Ok(())
    }

    /// Embed carded sections that have no vector for the embedder's model yet, up to `limit`,
    /// in batches. Blocking (the model runs on this thread); the daemon wraps it in
    /// `block_in_place`. A model that cannot be loaded fails the whole pass with
    /// [`Error::Embed`]; nothing is stored for a batch that failed.
    pub fn embed_pending(&mut self, embedder: &dyn Embedder, limit: usize) -> Result<EmbedReport> {
        let started = std::time::Instant::now();
        let model = embedder.model();
        let mut done = 0usize;
        while done < limit {
            let batch = EMBED_BATCH.min(limit - done);
            let cards = self.store.cards_without_embedding(model, batch)?;
            if cards.is_empty() {
                break;
            }
            let texts: Vec<String> = cards
                .iter()
                .map(|c| embed_text(c.title.as_deref(), &c.heading_path, &c.summary))
                .collect();
            let vectors = embedder.embed(&texts)?;
            if vectors.len() != cards.len() {
                return Err(Error::Embed(format!(
                    "model returned {} vectors for {} texts",
                    vectors.len(),
                    cards.len()
                )));
            }
            for (card, vector) in cards.iter().zip(&vectors) {
                self.store.put_embedding(&card.section_hash, model, vector)?;
            }
            done += cards.len();
        }
        let counts = self.store.embedding_counts(model)?;
        let report = EmbedReport {
            model: model.to_owned(),
            embedded: done,
            remaining: counts.carded.saturating_sub(counts.embedded),
            ms: started.elapsed().as_millis(),
        };
        if done > 0 {
            tracing::info!(
                model,
                embedded = done,
                remaining = report.remaining,
                ms = report.ms,
                "embedded cards"
            );
        }
        Ok(report)
    }

    /// Documents whose index is not final. Files newer on disk than their indexed content are
    /// parsed and compared by hash, so a `touch` alone does not count.
    pub fn stale(&self) -> Result<Vec<StaleDoc>> {
        let mut by_path: std::collections::BTreeMap<String, StaleDoc> =
            std::collections::BTreeMap::new();
        for (doc, pending, failed) in self.store.documents_with_open_sections()? {
            by_path.insert(
                doc.rel_path.clone(),
                StaleDoc {
                    rel_path: doc.rel_path,
                    pending,
                    failed,
                    changed_on_disk: false,
                    missing: false,
                },
            );
        }
        for doc in self.store.documents()? {
            let abs = self.root.join(&doc.rel_path);
            let (changed, missing) = match std::fs::metadata(&abs) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, true),
                Err(_) => (false, false),
                Ok(meta) => {
                    let newer = meta
                        .modified()
                        .ok()
                        .and_then(|t| Timestamp::try_from(t).ok())
                        .is_some_and(|m| m > doc.updated_at);
                    let changed = newer
                        && std::fs::read_to_string(&abs)
                            .is_ok_and(|t| markdown::parse_str(&t).hash != doc.content_hash);
                    (changed, false)
                }
            };
            if changed || missing {
                let entry = by_path.entry(doc.rel_path.clone()).or_insert(StaleDoc {
                    rel_path: doc.rel_path.clone(),
                    pending: 0,
                    failed: 0,
                    changed_on_disk: false,
                    missing: false,
                });
                entry.changed_on_disk = changed;
                entry.missing = missing;
            }
        }
        Ok(by_path.into_values().collect())
    }

    /// The `n` most recently updated documents.
    pub fn recent(&self, n: usize) -> Result<Vec<RecentDoc>> {
        let mut out = Vec::new();
        for doc in self.store.recent_documents(n)? {
            let sections = self.store.sections_of(&doc.doc_id)?;
            let pending = sections
                .iter()
                .filter(|s| s.state != crate::store::SectionState::Summarized)
                .count();
            out.push(RecentDoc {
                rel_path: doc.rel_path,
                title: doc.title,
                updated_at: doc.updated_at,
                created_at: doc.created_at,
                sections: sections.len(),
                pending,
            });
        }
        Ok(out)
    }

    /// Store events in a window, oldest first, with document paths resolved and an optional
    /// path prefix filter applied.
    pub fn timeline(
        &self,
        since: Option<Timestamp>,
        until: Option<Timestamp>,
        path_prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<TimelineEntry>> {
        // Fetch deeper when filtering by path, since the filter runs after the store's limit.
        let fetch = if path_prefix.is_some() { limit.saturating_mul(8).max(500) } else { limit };
        let mut paths: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut out = Vec::new();
        for ev in self.store.timeline(since, until, fetch)? {
            let rel_path = if let Some(p) = paths.get(&ev.doc_id) {
                p.clone()
            } else {
                let p = self
                    .store
                    .document(&ev.doc_id)?
                    .map_or_else(|| ev.doc_id.clone(), |d| d.rel_path);
                paths.insert(ev.doc_id.clone(), p.clone());
                p
            };
            if let Some(prefix) = path_prefix
                && !rel_path.starts_with(prefix.trim_start_matches("./"))
            {
                continue;
            }
            out.push(TimelineEntry {
                at: ev.at,
                kind: ev.kind,
                rel_path,
                section_id: ev.section_id,
                detail: ev.detail,
            });
            if out.len() == limit {
                break;
            }
        }
        Ok(out)
    }

    /// Return the exact source lines of a section, re-checking the file at read time.
    ///
    /// If the file changed since indexing it is re-indexed on the spot, the section is
    /// located again (by hash, then heading path, then position) and the result is flagged
    /// `stale`. `section_id` in the result is the section's *current* id, which can differ
    /// from `requested_id` when sections moved.
    pub fn open_section(&mut self, section_id: &str) -> Result<Opened> {
        let stored = self
            .store
            .section(section_id)?
            .ok_or_else(|| Error::NotFound(format!("section {section_id}")))?;
        let stored_doc = self
            .store
            .document(&stored.doc_id)?
            .ok_or_else(|| Error::NotFound(format!("document {}", stored.doc_id)))?;
        let abs = self.safe_join(&stored.rel_path)?;
        let text = std::fs::read_to_string(&abs).map_err(|e| Error::io(&abs, e))?;
        let doc = markdown::parse_str(&text);

        let stale = doc.hash != stored_doc.content_hash;
        if stale {
            let times = file_times(&abs)?;
            self.index_parsed(&stored.rel_path, &doc, &times)?;
        }
        let same_place =
            doc.sections.get(stored.index as usize).filter(|s| s.hash == stored.section_hash);
        let section = same_place
            .or_else(|| doc.sections.iter().find(|s| s.hash == stored.section_hash))
            .or_else(|| doc.sections.iter().find(|s| s.heading_path == stored.heading_path))
            .or_else(|| doc.sections.get(stored.index as usize))
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "section {section_id} no longer exists in {}",
                    stored.rel_path
                ))
            })?;

        let lines: Vec<&str> = text.lines().collect();
        let start = (section.line_start as usize).saturating_sub(1).min(lines.len());
        let end = (section.line_end as usize).min(lines.len());
        Ok(Opened {
            requested_id: section_id.to_owned(),
            section_id: crate::store::section_id_for(&stored.doc_id, section.index),
            rel_path: stored.rel_path,
            heading_path: section.heading_path.clone(),
            line_start: section.line_start,
            line_end: section.line_end,
            text: lines[start..end].join("\n"),
            stale,
        })
    }
}

/// What one summarization run will submit, and what it leaves for later.
struct Round {
    pending: Vec<PendingSection>,
    deferred: usize,
    budget_exhausted: bool,
}

/// Midnight UTC today. The daily budget resets on UTC days so it is the same everywhere.
fn start_of_today() -> Timestamp {
    let now = Timestamp::now();
    let secs = now.as_second();
    Timestamp::from_second(secs - secs.rem_euclid(86_400)).unwrap_or(now)
}

/// `true` when the section is a heading with no body (only blank lines after it).
fn is_heading_only(text: &str) -> bool {
    let mut lines = text.lines();
    let Some(first) = lines.next() else { return false };
    first.trim_start().starts_with('#') && lines.all(|l| l.trim().is_empty())
}

/// A card for a heading-only section: the heading is the summary.
fn synthetic_summary(p: &PendingSection) -> crate::card::SectionSummary {
    let heading = p.heading_path.last().cloned().unwrap_or_else(|| "(untitled)".to_owned());
    let path = p.heading_path.join(" › ");
    crate::card::SectionSummary {
        tldr: format!("Heading only: {heading}."),
        summary: format!(
            "The section \"{path}\" contains only its heading; its content lives in the subsections below it."
        ),
        keywords: heading.split_whitespace().map(str::to_lowercase).take(8).collect(),
        questions_answered: vec![format!("Where is the {heading} section?")],
        entities: crate::card::Entities::default(),
        mentioned_dates: Vec::new(),
        decisions: Vec::new(),
        action_items: Vec::new(),
    }
}

/// Build the worker request for a pending section, truncating oversized sections.
fn request_for(p: &PendingSection, cfg: PlanConfig) -> SummarizeRequest {
    let (text, _truncated) = chunk_text(&p.text, &cfg);
    SummarizeRequest {
        id: p.section_hash.clone(),
        rel_path: p.rel_path.clone(),
        heading_path: p.heading_path.clone(),
        token_estimate: markdown::token_estimate(&text),
        text,
    }
}

/// Filesystem timestamps for a document. Birth time is used when the platform has it.
pub fn file_times(path: &Path) -> Result<DocTimes> {
    let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
    let to_ts = |t: std::time::SystemTime| Timestamp::try_from(t).ok();
    let modified = meta.modified().ok().and_then(to_ts).unwrap_or_else(Timestamp::now);
    let created = meta.created().ok().and_then(to_ts).filter(|c| *c <= modified);
    Ok(DocTimes {
        created_at: created,
        modified_at: modified,
        now: Timestamp::now(),
        size_bytes: meta.len(),
    })
}

#[cfg(test)]
mod tests;
