//! SQLite state: documents, sections, summaries keyed by section hash, events, and the two
//! FTS5 indexes that make everything searchable.
//!
//! ## Shape
//!
//! ```text
//! docs ──< sections >── summaries          events
//!            │  │
//!            │  └── cards_fts   (one row per section whose hash is summarized)
//!            └───── sections_raw_fts (one row per section, written at parse time)
//! ```
//!
//! - `docs` is one row per markdown file, identified by [`doc_id_for`] (blake3 of the relative
//!   path). Deleted files are tombstoned (`deleted_at`), never removed.
//! - `sections` is replaced wholesale on every [`Store::upsert_document`], so line ranges are
//!   always those of the last parse. Section ids are `<doc_id>#<index>`.
//! - `summaries` is keyed by `section_hash`, so a section that moves within a file or appears
//!   verbatim in another file reuses its card. The row doubles as the job queue: its `state`
//!   is `pending`, `summarized` or `failed`.
//! - `events` is an append-only timeline of what changed and when.
//!
//! Both FTS tables use the section row's `rowid` as their own rowid, so replacing or removing
//! a document's sections touches only the rows that belong to it.
//!
//! ## Time
//!
//! Every timestamp is stored as RFC 3339 UTC text with nine fractional digits, produced by
//! [`fmt_ts`]. Fixed width means SQLite's lexical comparison is chronological, which the
//! timeline queries rely on.

use std::path::Path;

use jiff::Timestamp;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::card::{Provenance, SectionSummary};
use crate::markdown::Document;
use crate::{Error, Result};

/// Schema migrations, applied in order. Version `n` is `MIGRATIONS[n - 1]`. To add a
/// version, append one entry; [`Store::open`] runs whatever the file is missing.
const MIGRATIONS: &[&str] = &[SCHEMA_V1, SCHEMA_V2];

/// How long a connection waits for another writer before giving up.
pub const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Current schema version: the one a freshly opened store reports.
#[allow(clippy::cast_possible_truncation)] // a handful of migrations, never 2^32
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// v2: an append-only ledger of every model attempt, so the daily budget counts tokens that
/// bought nothing (retries, malformed replies, validation failures) as well as cards.
const SCHEMA_V2: &str = r"
CREATE TABLE usage_log (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    at            TEXT    NOT NULL,
    section_hash  TEXT    NOT NULL,
    model         TEXT    NOT NULL,
    outcome       TEXT    NOT NULL,
    input_tokens  INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cost_usd      REAL    NOT NULL
);
CREATE INDEX usage_log_at ON usage_log (at);
";

const SCHEMA_V1: &str = r"
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE docs (
    doc_id             TEXT PRIMARY KEY,
    rel_path           TEXT NOT NULL UNIQUE,
    title              TEXT,
    content_hash       TEXT NOT NULL,
    size_bytes         INTEGER NOT NULL,
    line_count         INTEGER NOT NULL,
    token_estimate     INTEGER NOT NULL,
    frontmatter        TEXT,
    links_internal     TEXT NOT NULL,
    links_external     TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    created_at_source  TEXT NOT NULL CHECK (created_at_source IN ('birthtime', 'first_seen')),
    updated_at         TEXT NOT NULL,
    first_seen_at      TEXT NOT NULL,
    last_summarized_at TEXT,
    deleted_at         TEXT
);

CREATE TABLE summaries (
    section_hash  TEXT PRIMARY KEY,
    state         TEXT NOT NULL CHECK (state IN ('pending', 'summarized', 'failed')),
    summary       TEXT,
    provenance    TEXT,
    input_tokens  INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd      REAL NOT NULL DEFAULT 0,
    fail_reason   TEXT,
    first_seen_at TEXT NOT NULL,
    summarized_at TEXT
);

CREATE TABLE sections (
    section_id     TEXT PRIMARY KEY,
    doc_id         TEXT NOT NULL REFERENCES docs(doc_id) ON DELETE CASCADE,
    idx            INTEGER NOT NULL,
    level          INTEGER NOT NULL,
    heading_path   TEXT NOT NULL,
    line_start     INTEGER NOT NULL,
    line_end       INTEGER NOT NULL,
    token_estimate INTEGER NOT NULL,
    section_hash   TEXT NOT NULL REFERENCES summaries(section_hash),
    code_langs     TEXT NOT NULL,
    has_tables     INTEGER NOT NULL,
    text           TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
CREATE INDEX sections_by_doc  ON sections(doc_id, idx);
CREATE INDEX sections_by_hash ON sections(section_hash);

CREATE TABLE events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    at         TEXT NOT NULL,
    kind       TEXT NOT NULL,
    doc_id     TEXT NOT NULL,
    section_id TEXT,
    detail     TEXT
);
CREATE INDEX events_by_at ON events(at, id);

CREATE VIRTUAL TABLE sections_raw_fts USING fts5(
    section_id UNINDEXED,
    heading_path,
    text,
    tokenize='unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE cards_fts USING fts5(
    section_id UNINDEXED,
    heading_path,
    tldr,
    summary,
    keywords,
    questions_answered,
    entities,
    tokenize='unicode61 remove_diacritics 2'
);
";

/// `bm25()` call for the raw index. Weights follow column order: `section_id` (unindexed),
/// `heading_path` 3.0, `text` 1.0.
const RAW_BM25: &str = "bm25(sections_raw_fts, 0.0, 3.0, 1.0)";

/// `bm25()` call for the cards index: `heading_path` 3.0, `tldr` 3.0, `summary` 1.0,
/// `keywords` 1.0, `questions_answered` 2.0, `entities` 1.0.
const CARDS_BM25: &str = "bm25(cards_fts, 0.0, 3.0, 3.0, 1.0, 1.0, 2.0, 1.0)";

/// Columns of a joined section row, shared by every query that returns [`StoredSection`].
const SECTION_COLUMNS: &str = "
    s.section_id, s.doc_id, d.rel_path, s.idx, s.level, s.heading_path, s.line_start, s.line_end,
    s.token_estimate, s.section_hash, s.code_langs, s.has_tables, s.text, m.first_seen_at,
    s.updated_at, m.summary, m.provenance, m.state, m.fail_reason
    FROM sections s
    JOIN docs d ON d.doc_id = s.doc_id
    JOIN summaries m ON m.section_hash = s.section_hash";

/// Columns of a document row, shared by every query that returns [`StoredDocument`].
const DOC_COLUMNS: &str = "
    doc_id, rel_path, title, content_hash, size_bytes, line_count, token_estimate, frontmatter,
    created_at, created_at_source, updated_at, first_seen_at, last_summarized_at, deleted_at,
    links_internal, links_external
    FROM docs";

/// Stable identifier for a document: the first 16 hex characters of blake3 over its path
/// relative to the watched root, with forward slashes.
pub fn doc_id_for(rel_path: &str) -> String {
    let normalised = normalise_rel_path(rel_path);
    let mut hex = blake3::hash(normalised.as_bytes()).to_hex().to_string();
    hex.truncate(16);
    hex
}

/// Section identifier: `<doc_id>#<index>`.
pub fn section_id_for(doc_id: &str, index: u32) -> String {
    format!("{doc_id}#{index}")
}

/// Format a timestamp the way the store persists it: RFC 3339 UTC with nine fractional
/// digits, so that lexical order is chronological order.
pub fn fmt_ts(ts: Timestamp) -> String {
    format!("{ts:.9}")
}

fn normalise_rel_path(rel_path: &str) -> String {
    rel_path.replace('\\', "/")
}

/// Filesystem facts about a document that the parser cannot know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocTimes {
    /// File birth time, if the platform reports one. Used as `created_at` the first time the
    /// document is seen; ignored afterwards.
    pub created_at: Option<Timestamp>,
    /// File modification time. Becomes `updated_at`.
    pub modified_at: Timestamp,
    /// When this upsert happens. Used for `first_seen_at`, for `created_at` when there is no
    /// birth time, and for the emitted event.
    pub now: Timestamp,
    /// Size of the file on disk in bytes.
    pub size_bytes: u64,
}

/// What [`Store::upsert_document`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpsertOutcome {
    /// Identifier of the document row.
    pub doc_id: String,
    /// The document had no live row before (first sighting or a resurrected tombstone).
    pub created: bool,
    /// The whole-document content hash differs from the stored one. Always true when
    /// `created` is true.
    pub changed: bool,
    /// Section hashes that had no `summaries` row anywhere; each now has a pending row.
    /// Deduplicated, in document order.
    pub new_hashes: Vec<String>,
    /// Sections whose hash already had a `summaries` row, from another document or from an
    /// earlier section of this one, but was not in this document before.
    pub reused: usize,
    /// Sections whose hash was already in this document before this upsert.
    pub unchanged: usize,
    /// Distinct hashes that were in this document before and are not any more.
    pub removed: usize,
}

/// Where a document's `created_at` came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreatedAtSource {
    /// The filesystem birth time.
    Birthtime,
    /// The time the store first saw the file; the platform reported no birth time.
    FirstSeen,
}

impl CreatedAtSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Birthtime => "birthtime",
            Self::FirstSeen => "first_seen",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "birthtime" => Some(Self::Birthtime),
            "first_seen" => Some(Self::FirstSeen),
            _ => None,
        }
    }
}

/// A document row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDocument {
    /// See [`doc_id_for`].
    pub doc_id: String,
    /// Path relative to the watched root, forward slashes.
    pub rel_path: String,
    /// First level-1 heading, if any.
    pub title: Option<String>,
    /// blake3 of the normalised file text.
    pub content_hash: String,
    /// Size on disk at last upsert.
    pub size_bytes: u64,
    /// Line count at last upsert.
    pub line_count: u32,
    /// Sum of section token estimates.
    pub token_estimate: u32,
    /// Raw front matter, if any.
    pub frontmatter: Option<String>,
    /// When the file was created, per `created_at_source`. Set once.
    pub created_at: Timestamp,
    /// Where `created_at` came from.
    pub created_at_source: CreatedAtSource,
    /// File modification time at last upsert.
    pub updated_at: Timestamp,
    /// When the store first saw this path. Set once.
    pub first_seen_at: Timestamp,
    /// When a summary was last attached to one of this document's sections.
    pub last_summarized_at: Option<Timestamp>,
    /// When the file was tombstoned, if it has been.
    pub deleted_at: Option<Timestamp>,
    /// Link targets without a scheme.
    pub links_internal: Vec<String>,
    /// Link targets with a scheme.
    pub links_external: Vec<String>,
}

/// Lifecycle of a section hash's summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionState {
    /// Waiting for a worker.
    Pending,
    /// A summary is attached.
    Summarized,
    /// The last attempt failed; see `fail_reason`. Cleared by [`Store::retry_failed`].
    Failed,
}

impl SectionState {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "summarized" => Some(Self::Summarized),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// A section row joined with its document and its summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSection {
    /// `<doc_id>#<index>`.
    pub section_id: String,
    /// Owning document.
    pub doc_id: String,
    /// Owning document's relative path.
    pub rel_path: String,
    /// 0-based position in the document.
    pub index: u32,
    /// Heading level, 0 for the preamble.
    pub level: u8,
    /// Headings from the document root down to this section.
    pub heading_path: Vec<String>,
    /// First line, 1-based, inclusive, as of the last parse.
    pub line_start: u32,
    /// Last line, 1-based, inclusive, as of the last parse.
    pub line_end: u32,
    /// Rough token count of `text`.
    pub token_estimate: u32,
    /// blake3 of the normalised section text; the key into `summaries`.
    pub section_hash: String,
    /// Fenced code block languages present.
    pub code_langs: Vec<String>,
    /// Whether the section contains a table.
    pub has_tables: bool,
    /// Normalised section text.
    pub text: String,
    /// First time this hash was seen anywhere in the store.
    pub created_at: Timestamp,
    /// Last time this document's section at this position changed content.
    pub updated_at: Timestamp,
    /// The card, if the hash is summarized.
    pub summary: Option<SectionSummary>,
    /// How the card was produced, if it exists.
    pub provenance: Option<Provenance>,
    /// Queue state of the hash.
    pub state: SectionState,
    /// Why the last summarization attempt failed, when `state` is [`SectionState::Failed`].
    pub fail_reason: Option<String>,
}

/// One item of summarization work: a hash plus a representative section to build the
/// prompt from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSection {
    /// Hash to summarize. The result applies to every section with this hash.
    pub section_hash: String,
    /// The lowest section id carrying the hash.
    pub section_id: String,
    /// Relative path of that section's document.
    pub rel_path: String,
    /// Heading path of that section.
    pub heading_path: Vec<String>,
    /// Text of that section.
    pub text: String,
    /// Rough token count of `text`.
    pub token_estimate: u32,
}

/// Token and cost accounting for one summarization call, stored beside the summary.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Prompt tokens consumed.
    pub input_tokens: u64,
    /// Completion tokens produced.
    pub output_tokens: u64,
    /// Cost as reported by the backend, in US dollars.
    pub cost_usd: f64,
}

/// A full-text hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FtsHit {
    /// Matching section.
    pub section_id: String,
    /// Relevance, higher is better. This is the negation of FTS5's `bm25()`, which returns
    /// lower-is-better negative numbers; callers can treat it as a plain score.
    pub bm25: f64,
}

/// Aggregate numbers for `mda status`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Counts {
    /// Live documents.
    pub docs: u64,
    /// Sections across live documents.
    pub sections: u64,
    /// Distinct hashes of live sections that have a summary.
    pub summarized: u64,
    /// Distinct hashes of live sections waiting for a worker.
    pub pending: u64,
    /// Distinct hashes of live sections whose last attempt failed.
    pub failed: u64,
    /// Documents with a tombstone.
    pub tombstoned: u64,
    /// Sum of prompt tokens over every summary ever attached.
    pub total_input_tokens: u64,
    /// Sum of completion tokens over every summary ever attached.
    pub total_output_tokens: u64,
    /// Sum of reported cost over every summary ever attached.
    pub total_cost_usd: f64,
}

/// What kind of thing an [`Event`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A document row was created, or a tombstone was resurrected.
    DocCreated,
    /// A document's content hash changed.
    DocChanged,
    /// A document was tombstoned.
    DocDeleted,
    /// A document moved: the old path was tombstoned and this one carries its history on.
    /// `detail` is the old relative path.
    DocRenamed,
    /// A summary was attached to a section.
    SectionSummarized,
    /// Summarizing a section failed.
    SectionFailed,
}

impl EventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::DocCreated => "doc_created",
            Self::DocChanged => "doc_changed",
            Self::DocDeleted => "doc_deleted",
            Self::DocRenamed => "doc_renamed",
            Self::SectionSummarized => "section_summarized",
            Self::SectionFailed => "section_failed",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "doc_created" => Some(Self::DocCreated),
            "doc_changed" => Some(Self::DocChanged),
            "doc_deleted" => Some(Self::DocDeleted),
            "doc_renamed" => Some(Self::DocRenamed),
            "section_summarized" => Some(Self::SectionSummarized),
            "section_failed" => Some(Self::SectionFailed),
            _ => None,
        }
    }
}

/// One entry of the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// When it happened.
    pub at: Timestamp,
    /// What happened.
    pub kind: EventKind,
    /// Document concerned.
    pub doc_id: String,
    /// Section concerned, for section-level kinds.
    pub section_id: Option<String>,
    /// Free-form detail, e.g. a failure reason.
    pub detail: Option<String>,
}

/// Turn arbitrary user text into an FTS5 `MATCH` expression that cannot be misparsed.
///
/// The text is split on whitespace, every term is wrapped in double quotes (inner quotes
/// doubled) so that FTS5 treats it as a literal phrase, and the terms are joined with spaces,
/// which FTS5 reads as AND. A trailing `*` on a term is kept outside the quotes to request
/// prefix matching; a `*` anywhere else is literal. Operators (`OR`, `NOT`, `NEAR`), column
/// filters (`title:foo`), parentheses and caret are all neutralised. Returns an empty string
/// for text with no terms; the search functions treat that as "no results".
pub fn fts_escape(user_query: &str) -> String {
    let mut out = String::with_capacity(user_query.len() + 8);
    for raw in user_query.split_whitespace() {
        let (term, prefix) = match raw.strip_suffix('*') {
            Some(stem) => (stem, true),
            None => (raw, false),
        };
        if term.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push('"');
        out.push_str(&term.replace('"', "\"\""));
        out.push('"');
        if prefix {
            out.push('*');
        }
    }
    out
}

/// Handle to the SQLite database. One connection, WAL mode, foreign keys on.
///
/// `Store` is not `Sync`; give each thread its own by calling [`Store::open`] again.
pub struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("path", &self.conn.path()).finish()
    }
}

impl Store {
    /// Open (creating if needed) the database at `path`, create its parent directory, and
    /// apply any missing migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Open a private in-memory database with the full schema. For tests and dry runs.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        // journal_mode returns a row (the resulting mode); in-memory databases answer
        // "memory" instead of "wal", which is fine.
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        // The daemon's indexer and summarizer, plus any `mda` command in another shell, share
        // one file. WAL serialises their short write transactions; waiting here instead of
        // failing with "database is locked" is what makes that safe.
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        let has_meta: bool = tx.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |r| r.get::<_, i64>(0).map(|n| n > 0),
        )?;
        let current = if has_meta { read_schema_version(&tx)? } else { 0 };
        for (version, sql) in (1u32..).zip(MIGRATIONS) {
            if version <= current {
                continue;
            }
            tracing::debug!(version, "applying store migration");
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![version.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The schema version recorded in `meta`.
    pub fn schema_version(&self) -> Result<u32> {
        read_schema_version(&self.conn)
    }

    /// Insert or refresh a document and replace all of its sections.
    ///
    /// Section rows and their raw FTS rows are rewritten from `doc`, so every line range is
    /// current afterwards. Hashes never seen before get a pending `summaries` row; hashes that
    /// already have a summary get their `cards_fts` row back immediately. Emits
    /// [`EventKind::DocCreated`] for a new or resurrected document and
    /// [`EventKind::DocChanged`] when the content hash changed.
    pub fn upsert_document(
        &mut self,
        rel_path: &str,
        doc: &Document,
        times: &DocTimes,
    ) -> Result<UpsertOutcome> {
        let rel_path = normalise_rel_path(rel_path);
        let doc_id = doc_id_for(&rel_path);
        let tx = self.conn.transaction()?;

        let existing: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT content_hash, deleted_at FROM docs WHERE doc_id = ?1",
                params![doc_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let created = !matches!(&existing, Some((_, None)));
        let changed = created || existing.as_ref().is_some_and(|(h, _)| *h != doc.hash);

        // Hash -> updated_at of the sections this document had before.
        let old: Vec<(String, Timestamp)> = tx
            .prepare("SELECT section_hash, updated_at FROM sections WHERE doc_id = ?1")?
            .query_map(params![doc_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut old_by_hash: std::collections::HashMap<&str, Timestamp> =
            std::collections::HashMap::with_capacity(old.len());
        for (hash, at) in &old {
            old_by_hash.entry(hash.as_str()).or_insert(*at);
        }

        delete_doc_sections(&tx, &doc_id)?;

        write_doc_row(&tx, &doc_id, &rel_path, doc, times, existing.is_some())?;
        let now = fmt_ts(times.now);

        let mut outcome = UpsertOutcome {
            doc_id: doc_id.clone(),
            created,
            changed,
            new_hashes: Vec::new(),
            reused: 0,
            unchanged: 0,
            removed: 0,
        };
        let mut new_hash_set: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut current_hashes: std::collections::HashSet<&str> =
            std::collections::HashSet::with_capacity(doc.sections.len());

        for section in &doc.sections {
            let hash = section.hash.as_str();
            current_hashes.insert(hash);
            let updated_at = if let Some(at) = old_by_hash.get(hash) {
                outcome.unchanged += 1;
                *at
            } else {
                if new_hash_set.contains(hash) || summary_state(&tx, hash)?.is_some() {
                    outcome.reused += 1;
                } else {
                    tx.execute(
                        "INSERT INTO summaries (section_hash, state, first_seen_at)
                         VALUES (?1, 'pending', ?2)",
                        params![hash, now],
                    )?;
                    new_hash_set.insert(hash);
                    outcome.new_hashes.push(section.hash.clone());
                }
                times.modified_at
            };

            insert_section(&tx, &doc_id, section, updated_at)?;
        }

        outcome.removed = old_by_hash.keys().filter(|h| !current_hashes.contains(*h)).count();

        if created {
            let detail = existing.is_some().then(|| "restored".to_owned());
            insert_event(&tx, times.now, EventKind::DocCreated, &doc_id, None, detail.as_deref())?;
        } else if changed {
            insert_event(&tx, times.now, EventKind::DocChanged, &doc_id, None, None)?;
        }

        tx.commit()?;
        tracing::debug!(
            doc_id,
            rel_path,
            created,
            changed,
            new = outcome.new_hashes.len(),
            reused = outcome.reused,
            unchanged = outcome.unchanged,
            removed = outcome.removed,
            "upserted document"
        );
        Ok(outcome)
    }

    /// Attach a summary to a hash and make every current section carrying it card-searchable.
    ///
    /// Returns the number of sections now carded. Emits one
    /// [`EventKind::SectionSummarized`] per section. Fails with [`Error::NotFound`] if the hash
    /// has never been seen.
    pub fn attach_summary(
        &mut self,
        section_hash: &str,
        summary: &SectionSummary,
        provenance: &Provenance,
        usage: &Usage,
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let at = provenance.summarized_at;
        let updated = tx.execute(
            "UPDATE summaries SET state = 'summarized', summary = ?2, provenance = ?3,
                input_tokens = ?4, output_tokens = ?5, cost_usd = ?6, fail_reason = NULL,
                summarized_at = ?7
             WHERE section_hash = ?1 AND state != 'summarized'",
            params![
                section_hash,
                serde_json::to_string(summary)?,
                serde_json::to_string(provenance)?,
                to_i64(usage.input_tokens),
                to_i64(usage.output_tokens),
                usage.cost_usd,
                fmt_ts(at),
            ],
        )?;
        if updated == 0 {
            // Already summarized by another writer: keep the existing card untouched.
            return if row_exists(&tx, section_hash)? {
                tracing::debug!(section_hash, "attach_summary skipped: already summarized");
                Ok(0)
            } else {
                Err(Error::NotFound(format!("section hash {section_hash}")))
            };
        }

        let rows: Vec<(i64, String, String, String)> = tx
            .prepare(
                "SELECT rowid, section_id, doc_id, heading_path FROM sections
                 WHERE section_hash = ?1 ORDER BY section_id",
            )?
            .query_map(params![section_hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (rowid, section_id, doc_id, heading_json) in &rows {
            let heading_path: Vec<String> = serde_json::from_str(heading_json)?;
            tx.execute("DELETE FROM cards_fts WHERE rowid = ?1", params![rowid])?;
            insert_card_row(&tx, *rowid, section_id, &heading_path.join(" / "), summary)?;
            tx.execute(
                "UPDATE docs SET last_summarized_at = ?2 WHERE doc_id = ?1
                 AND (last_summarized_at IS NULL OR last_summarized_at < ?2)",
                params![doc_id, fmt_ts(at)],
            )?;
            insert_event(&tx, at, EventKind::SectionSummarized, doc_id, Some(section_id), None)?;
        }
        tx.commit()?;
        tracing::debug!(section_hash, sections = rows.len(), "attached summary");
        Ok(rows.len())
    }

    /// Record that summarizing a hash failed. The hash stays out of
    /// [`Store::pending_hashes`] until [`Store::retry_failed`]. Emits one
    /// [`EventKind::SectionFailed`] per current section carrying the hash. Fails with
    /// [`Error::NotFound`] if the hash has never been seen.
    pub fn mark_failed(&mut self, section_hash: &str, reason: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let now = Timestamp::now();
        let updated = tx.execute(
            "UPDATE summaries SET state = 'failed', fail_reason = ?2
             WHERE section_hash = ?1 AND state = 'pending'",
            params![section_hash, reason],
        )?;
        if updated == 0 {
            // Another writer already settled this hash (or it was never seen).
            return if row_exists(&tx, section_hash)? {
                tracing::debug!(section_hash, "mark_failed skipped: hash no longer pending");
                Ok(())
            } else {
                Err(Error::NotFound(format!("section hash {section_hash}")))
            };
        }
        let rows: Vec<(String, String)> = tx
            .prepare(
                "SELECT section_id, doc_id FROM sections WHERE section_hash = ?1
                 ORDER BY section_id",
            )?
            .query_map(params![section_hash], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (section_id, doc_id) in &rows {
            insert_event(
                &tx,
                now,
                EventKind::SectionFailed,
                doc_id,
                Some(section_id),
                Some(reason),
            )?;
        }
        tx.commit()?;
        tracing::debug!(section_hash, reason, "marked section failed");
        Ok(())
    }

    /// Hashes waiting for a worker, smallest first, each with one representative section
    /// (the lowest section id carrying the hash) to build the prompt from. Hashes whose
    /// sections all belong to tombstoned documents are not returned.
    pub fn pending_hashes(&self, limit: usize) -> Result<Vec<PendingSection>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.section_hash, MIN(s.section_id), d.rel_path, s.heading_path, s.text,
                    s.token_estimate
             FROM summaries m
             JOIN sections s ON s.section_hash = m.section_hash
             JOIN docs d ON d.doc_id = s.doc_id
             WHERE m.state = 'pending'
             GROUP BY m.section_hash
             ORDER BY s.token_estimate ASC, m.section_hash ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![to_i64(limit as u64)], |r| {
            Ok(PendingSection {
                section_hash: r.get(0)?,
                section_id: r.get(1)?,
                rel_path: r.get(2)?,
                heading_path: json_col(r, 3)?,
                text: r.get(4)?,
                token_estimate: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Put every failed hash back into the pending state. Returns how many were reset.
    pub fn retry_failed(&mut self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE summaries SET state = 'pending', fail_reason = NULL WHERE state = 'failed'",
            [],
        )?;
        tracing::debug!(reset = n, "retrying failed sections");
        Ok(n)
    }

    /// One section by id, or `None` if it does not exist (or its document is tombstoned).
    pub fn section(&self, section_id: &str) -> Result<Option<StoredSection>> {
        let sql = format!("SELECT {SECTION_COLUMNS} WHERE s.section_id = ?1");
        Ok(self.conn.query_row(&sql, params![section_id], row_to_section).optional()?)
    }

    /// Every section of a document, in document order.
    pub fn sections_of(&self, doc_id: &str) -> Result<Vec<StoredSection>> {
        let sql = format!("SELECT {SECTION_COLUMNS} WHERE s.doc_id = ?1 ORDER BY s.idx");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![doc_id], row_to_section)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every current section whose hash is `section_hash`, ordered by section id.
    pub fn sections_by_hash(&self, section_hash: &str) -> Result<Vec<StoredSection>> {
        let sql =
            format!("SELECT {SECTION_COLUMNS} WHERE s.section_hash = ?1 ORDER BY s.section_id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![section_hash], row_to_section)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// One document by id, tombstoned or not.
    pub fn document(&self, doc_id: &str) -> Result<Option<StoredDocument>> {
        let sql = format!("SELECT {DOC_COLUMNS} WHERE doc_id = ?1");
        Ok(self.conn.query_row(&sql, params![doc_id], row_to_document).optional()?)
    }

    /// One document by relative path, tombstoned or not.
    pub fn document_by_path(&self, rel_path: &str) -> Result<Option<StoredDocument>> {
        self.document(&doc_id_for(rel_path))
    }

    /// Content hash of the live document at `rel_path`, or `None` if there is none.
    pub fn document_hash(&self, rel_path: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT content_hash FROM docs WHERE doc_id = ?1 AND deleted_at IS NULL",
                params![doc_id_for(rel_path)],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Record that the live document at `old_rel` moved to `new_rel`, which must already be
    /// upserted. The new row inherits `created_at`, `created_at_source` and `first_seen_at`;
    /// the old row is tombstoned without a delete event and a [`EventKind::DocRenamed`] event
    /// names the old path. Returns `false` (and changes nothing) when either row is missing
    /// or the old one is already tombstoned.
    pub fn note_rename(&mut self, old_rel: &str, new_rel: &str, at: Timestamp) -> Result<bool> {
        let old_id = doc_id_for(old_rel);
        let new_id = doc_id_for(new_rel);
        if old_id == new_id {
            return Ok(false);
        }
        let tx = self.conn.transaction()?;
        let history: Option<(String, String, String)> = tx
            .query_row(
                "SELECT created_at, created_at_source, first_seen_at FROM docs
                 WHERE doc_id = ?1 AND deleted_at IS NULL",
                params![old_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((created_at, source, first_seen_at)) = history else {
            return Ok(false);
        };
        let moved = tx.execute(
            "UPDATE docs SET created_at = ?2, created_at_source = ?3, first_seen_at = ?4
             WHERE doc_id = ?1 AND deleted_at IS NULL",
            params![new_id, created_at, source, first_seen_at],
        )?;
        if moved == 0 {
            return Ok(false);
        }
        tx.execute(
            "UPDATE docs SET deleted_at = ?2 WHERE doc_id = ?1",
            params![old_id, fmt_ts(at)],
        )?;
        delete_doc_sections(&tx, &old_id)?;
        insert_event(&tx, at, EventKind::DocRenamed, &new_id, None, Some(old_rel))?;
        tx.commit()?;
        tracing::debug!(from = old_rel, to = new_rel, "recorded rename");
        Ok(true)
    }

    /// Every live (not tombstoned) document, ordered by path.
    pub fn documents(&self) -> Result<Vec<StoredDocument>> {
        let sql = format!("SELECT {DOC_COLUMNS} WHERE deleted_at IS NULL ORDER BY rel_path");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_document)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Tombstone a document: set `deleted_at`, drop its sections and their FTS rows, keep its
    /// summaries so the content gets its cards back if it reappears. Emits
    /// [`EventKind::DocDeleted`]. Returns `false` if there was no live document at that path.
    pub fn tombstone(&mut self, rel_path: &str, at: Timestamp) -> Result<bool> {
        let doc_id = doc_id_for(rel_path);
        let tx = self.conn.transaction()?;
        let updated = tx.execute(
            "UPDATE docs SET deleted_at = ?2 WHERE doc_id = ?1 AND deleted_at IS NULL",
            params![doc_id, fmt_ts(at)],
        )?;
        if updated == 0 {
            return Ok(false);
        }
        delete_doc_sections(&tx, &doc_id)?;
        insert_event(&tx, at, EventKind::DocDeleted, &doc_id, None, None)?;
        tx.commit()?;
        tracing::debug!(doc_id, rel_path, "tombstoned document");
        Ok(true)
    }

    /// BM25 search over raw section text and heading paths. `fts_expr` must already be a
    /// valid FTS5 expression; use [`fts_escape`] for user input. Best hit first.
    pub fn search_raw(&self, fts_expr: &str, k: usize) -> Result<Vec<FtsHit>> {
        let sql = format!(
            "SELECT section_id, {RAW_BM25} AS score FROM sections_raw_fts
             WHERE sections_raw_fts MATCH ?1 ORDER BY score, section_id LIMIT ?2"
        );
        self.search(&sql, fts_expr, k)
    }

    /// BM25 search over cards (heading path, tldr, summary, keywords, questions, entities).
    /// `fts_expr` must already be a valid FTS5 expression; use [`fts_escape`] for user input.
    /// Best hit first.
    pub fn search_cards(&self, fts_expr: &str, k: usize) -> Result<Vec<FtsHit>> {
        let sql = format!(
            "SELECT section_id, {CARDS_BM25} AS score FROM cards_fts
             WHERE cards_fts MATCH ?1 ORDER BY score, section_id LIMIT ?2"
        );
        self.search(&sql, fts_expr, k)
    }

    fn search(&self, sql: &str, fts_expr: &str, k: usize) -> Result<Vec<FtsHit>> {
        if fts_expr.trim().is_empty() || k == 0 {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![fts_expr, to_i64(k as u64)], |r| {
            Ok(FtsHit { section_id: r.get(0)?, bm25: -r.get::<_, f64>(1)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Aggregate counts and totals.
    pub fn counts(&self) -> Result<Counts> {
        const STATE_COUNT: &str = "SELECT COUNT(*) FROM summaries m WHERE m.state = ?1
            AND EXISTS (SELECT 1 FROM sections s WHERE s.section_hash = m.section_hash)";
        let count = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> Result<u64> {
            let n: i64 = self.conn.query_row(sql, p, |r| r.get(0))?;
            Ok(u64::try_from(n).unwrap_or_default())
        };
        let (input, output, cost): (i64, i64, f64) = self.conn.query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cost_usd), 0.0) FROM summaries",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(Counts {
            docs: count("SELECT COUNT(*) FROM docs WHERE deleted_at IS NULL", &[])?,
            sections: count("SELECT COUNT(*) FROM sections", &[])?,
            summarized: count(STATE_COUNT, &[&"summarized"])?,
            pending: count(STATE_COUNT, &[&"pending"])?,
            failed: count(STATE_COUNT, &[&"failed"])?,
            tombstoned: count("SELECT COUNT(*) FROM docs WHERE deleted_at IS NOT NULL", &[])?,
            total_input_tokens: u64::try_from(input).unwrap_or_default(),
            total_output_tokens: u64::try_from(output).unwrap_or_default(),
            total_cost_usd: cost,
        })
    }

    /// Append an event to the timeline.
    pub fn record_event(&mut self, ev: &Event) -> Result<()> {
        insert_event(
            &self.conn,
            ev.at,
            ev.kind,
            &ev.doc_id,
            ev.section_id.as_deref(),
            ev.detail.as_deref(),
        )
    }

    /// Events with `since <= at < until`, oldest first, at most `limit`. Either bound may be
    /// omitted.
    pub fn timeline(
        &self,
        since: Option<Timestamp>,
        until: Option<Timestamp>,
        limit: usize,
    ) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT at, kind, doc_id, section_id, detail FROM events
             WHERE (?1 IS NULL OR at >= ?1) AND (?2 IS NULL OR at < ?2)
             ORDER BY at, id LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![since.map(fmt_ts), until.map(fmt_ts), to_i64(limit as u64)],
            |r| {
                let kind: String = r.get(1)?;
                Ok(Event {
                    at: r.get(0)?,
                    kind: EventKind::parse(&kind)
                        .ok_or_else(|| bad_column(1, format!("unknown event kind {kind}")))?,
                    doc_id: r.get(2)?,
                    section_id: r.get(3)?,
                    detail: r.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn read_schema_version(conn: &Connection) -> Result<u32> {
    let v: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
        .optional()?;
    Ok(v.and_then(|s| s.parse().ok()).unwrap_or(0))
}

/// Remove a document's section rows and their FTS rows.
fn delete_doc_sections(tx: &Transaction<'_>, doc_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM sections_raw_fts WHERE rowid IN (SELECT rowid FROM sections WHERE doc_id = ?1)",
        params![doc_id],
    )?;
    tx.execute(
        "DELETE FROM cards_fts WHERE rowid IN (SELECT rowid FROM sections WHERE doc_id = ?1)",
        params![doc_id],
    )?;
    tx.execute("DELETE FROM sections WHERE doc_id = ?1", params![doc_id])?;
    Ok(())
}

/// Insert a new document row, or refresh an existing one (clearing any tombstone) while
/// keeping `created_at` and `first_seen_at`.
fn write_doc_row(
    tx: &Transaction<'_>,
    doc_id: &str,
    rel_path: &str,
    doc: &Document,
    times: &DocTimes,
    exists: bool,
) -> Result<()> {
    let links_internal = serde_json::to_string(&doc.links_internal)?;
    let links_external = serde_json::to_string(&doc.links_external)?;
    if exists {
        tx.execute(
            "UPDATE docs SET title = ?2, content_hash = ?3, size_bytes = ?4, line_count = ?5,
                token_estimate = ?6, frontmatter = ?7, links_internal = ?8,
                links_external = ?9, updated_at = ?10, deleted_at = NULL
             WHERE doc_id = ?1",
            params![
                doc_id,
                doc.title,
                doc.hash,
                to_i64(times.size_bytes),
                doc.line_count,
                doc.token_estimate,
                doc.frontmatter,
                links_internal,
                links_external,
                fmt_ts(times.modified_at),
            ],
        )?;
    } else {
        let (created_at, source) = match times.created_at {
            Some(t) => (t, CreatedAtSource::Birthtime),
            None => (times.now, CreatedAtSource::FirstSeen),
        };
        tx.execute(
            "INSERT INTO docs (doc_id, rel_path, title, content_hash, size_bytes, line_count,
                token_estimate, frontmatter, links_internal, links_external, created_at,
                created_at_source, updated_at, first_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                doc_id,
                rel_path,
                doc.title,
                doc.hash,
                to_i64(times.size_bytes),
                doc.line_count,
                doc.token_estimate,
                doc.frontmatter,
                links_internal,
                links_external,
                fmt_ts(created_at),
                source.as_str(),
                fmt_ts(times.modified_at),
                fmt_ts(times.now),
            ],
        )?;
    }
    Ok(())
}

/// Insert one section row, its raw FTS row, and its card row if the hash is summarized.
fn insert_section(
    tx: &Transaction<'_>,
    doc_id: &str,
    section: &crate::markdown::Section,
    updated_at: Timestamp,
) -> Result<()> {
    let section_id = section_id_for(doc_id, section.index);
    tx.execute(
        "INSERT INTO sections (section_id, doc_id, idx, level, heading_path, line_start,
            line_end, token_estimate, section_hash, code_langs, has_tables, text, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            section_id,
            doc_id,
            section.index,
            section.level,
            serde_json::to_string(&section.heading_path)?,
            section.line_start,
            section.line_end,
            section.token_estimate,
            section.hash,
            serde_json::to_string(&section.code_langs)?,
            section.has_tables,
            section.text,
            fmt_ts(updated_at),
        ],
    )?;
    let rowid = tx.last_insert_rowid();
    let heading_text = section.heading_path.join(" / ");
    tx.execute(
        "INSERT INTO sections_raw_fts (rowid, section_id, heading_path, text)
         VALUES (?1, ?2, ?3, ?4)",
        params![rowid, section_id, heading_text, section.text],
    )?;
    if let Some(summary) = stored_summary(tx, &section.hash)? {
        insert_card_row(tx, rowid, &section_id, &heading_text, &summary)?;
    }
    Ok(())
}

fn summary_state(conn: &Connection, hash: &str) -> Result<Option<SectionState>> {
    let s: Option<String> = conn
        .query_row("SELECT state FROM summaries WHERE section_hash = ?1", params![hash], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(s.as_deref().and_then(SectionState::parse))
}

/// The summary attached to `hash`, if its state is summarized.
fn stored_summary(conn: &Connection, hash: &str) -> Result<Option<SectionSummary>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT summary FROM summaries WHERE section_hash = ?1 AND state = 'summarized'",
            params![hash],
            |r| r.get(0),
        )
        .optional()?;
    json.map(|j| serde_json::from_str(&j).map_err(Error::from)).transpose()
}

fn insert_card_row(
    conn: &Connection,
    rowid: i64,
    section_id: &str,
    heading_text: &str,
    summary: &SectionSummary,
) -> Result<()> {
    let e = &summary.entities;
    let entities = [&e.people, &e.orgs, &e.products, &e.technologies, &e.files_paths, &e.commands]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    conn.execute(
        "INSERT INTO cards_fts (rowid, section_id, heading_path, tldr, summary, keywords,
            questions_answered, entities)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            rowid,
            section_id,
            heading_text,
            summary.tldr,
            summary.summary,
            summary.keywords.join(", "),
            summary.questions_answered.join("\n"),
            entities,
        ],
    )?;
    Ok(())
}

fn insert_event(
    conn: &Connection,
    at: Timestamp,
    kind: EventKind,
    doc_id: &str,
    section_id: Option<&str>,
    detail: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO events (at, kind, doc_id, section_id, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![fmt_ts(at), kind.as_str(), doc_id, section_id, detail],
    )?;
    tracing::debug!(kind = kind.as_str(), doc_id, section_id, "recorded event");
    Ok(())
}

fn row_to_section(r: &Row<'_>) -> rusqlite::Result<StoredSection> {
    let state: String = r.get(17)?;
    let summary: Option<String> = r.get(15)?;
    let provenance: Option<String> = r.get(16)?;
    Ok(StoredSection {
        section_id: r.get(0)?,
        doc_id: r.get(1)?,
        rel_path: r.get(2)?,
        index: r.get(3)?,
        level: r.get(4)?,
        heading_path: json_col(r, 5)?,
        line_start: r.get(6)?,
        line_end: r.get(7)?,
        token_estimate: r.get(8)?,
        section_hash: r.get(9)?,
        code_langs: json_col(r, 10)?,
        has_tables: r.get(11)?,
        text: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
        summary: summary.as_deref().map(|j| parse_json(15, j)).transpose()?,
        provenance: provenance.as_deref().map(|j| parse_json(16, j)).transpose()?,
        state: SectionState::parse(&state)
            .ok_or_else(|| bad_column(17, format!("unknown section state {state}")))?,
        fail_reason: r.get(18)?,
    })
}

fn row_to_document(r: &Row<'_>) -> rusqlite::Result<StoredDocument> {
    let source: String = r.get(9)?;
    let size: i64 = r.get(4)?;
    Ok(StoredDocument {
        doc_id: r.get(0)?,
        rel_path: r.get(1)?,
        title: r.get(2)?,
        content_hash: r.get(3)?,
        size_bytes: u64::try_from(size).unwrap_or_default(),
        line_count: r.get(5)?,
        token_estimate: r.get(6)?,
        frontmatter: r.get(7)?,
        created_at: r.get(8)?,
        created_at_source: CreatedAtSource::parse(&source)
            .ok_or_else(|| bad_column(9, format!("unknown created_at_source {source}")))?,
        updated_at: r.get(10)?,
        first_seen_at: r.get(11)?,
        last_summarized_at: r.get(12)?,
        deleted_at: r.get(13)?,
        links_internal: json_col(r, 14)?,
        links_external: json_col(r, 15)?,
    })
}

fn json_col<T: DeserializeOwned>(r: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let text: String = r.get(idx)?;
    parse_json(idx, &text)
}

fn parse_json<T: DeserializeOwned>(idx: usize, text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e)))
}

fn bad_column(idx: usize, msg: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, msg.into())
}

/// SQLite integers are signed 64-bit; saturate rather than wrap.
fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Does any `summaries` row exist for this hash, in any state?
fn row_exists(conn: &Connection, section_hash: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM summaries WHERE section_hash = ?1",
        [section_hash],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

impl Store {
    /// Append one model attempt (or a whole job's attempts) to the usage ledger. Called for
    /// every job the pool reports, whatever its outcome, so budgets see the real spend.
    pub fn record_usage(
        &mut self,
        section_hash: &str,
        model: &str,
        usage: &Usage,
        outcome: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO usage_log (at, section_hash, model, outcome, input_tokens, output_tokens, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                fmt_ts(Timestamp::now()),
                section_hash,
                model,
                outcome,
                to_i64(usage.input_tokens),
                to_i64(usage.output_tokens),
                usage.cost_usd,
            ],
        )?;
        Ok(())
    }

    /// Tokens and cost of every attempt logged at or after `since`, successful or not.
    /// Backs the daily budget.
    pub fn usage_since(&self, since: Timestamp) -> Result<Usage> {
        let (input, output, cost): (i64, i64, f64) = self.conn.query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cost_usd), 0.0)
             FROM usage_log WHERE at >= ?1",
            [fmt_ts(since)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(Usage {
            input_tokens: u64::try_from(input).unwrap_or(0),
            output_tokens: u64::try_from(output).unwrap_or(0),
            cost_usd: cost,
        })
    }
}

#[cfg(test)]
mod tests;
