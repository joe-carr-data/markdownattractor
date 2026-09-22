//! # mda-core
//!
//! The engine behind [markdownattractor](https://github.com/joe-carr-data/markdownattractor):
//! a time-aware, searchable knowledge layer over a folder of markdown, built for Claude Code.
//!
//! The crate is organised as a pipeline. Each stage is a module with a small, testable surface
//! and no knowledge of the stages around it:
//!
//! ```text
//! markdown ─► diff ─► planner ─► worker ─► validate ─► reduce ─► store ─► index/search
//! ```
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`markdown`] | Parse a document into heading-delimited sections with exact line ranges and content hashes. Pure. |
//! | [`card`] | The output contract: section and document cards, the JSON schema handed to the model, and the rendered markdown form. |
//! | [`config`] | `config.toml` model with defaults. |
//! | [`error`] | One error type for the whole crate. |
//! | [`walk`] | Find the markdown files under a root, honouring ignore files. |
//! | [`diff`] | Which section hashes are new, unchanged, or gone between two parses. Pure. |
//! | [`planner`] | Turn sections into model-sized chunks. |
//! | [`worker`] | Run chunks through `claude -p` with retries, timeouts and adaptive concurrency. |
//! | [`validate`] | Enforce caps and evidence grounding on what the model returned. |
//! | [`store`] | SQLite state: documents, sections, summaries keyed by hash, jobs, events, FTS5. |
//! | [`search`] | Hybrid retrieval over the store: BM25 on cards and raw text, fused, time-aware. |
//! | [`pipeline`] | The engine that wires the stages together. |
//! | [`daemon`] | The long-running process: watcher, debounced intake, summarizer loop, control socket. |
//!
//! Two rules hold everywhere in this crate:
//!
//! 1. **Nothing here writes into the watched root.** The only files this crate creates live under
//!    `.markdownattractor/`.
//! 2. **Nothing here talks to the user.** No stdout, no prompts. The CLI crate owns presentation.

#![doc(html_root_url = "https://docs.rs/mda-core/0.1.0")]

pub mod card;
pub mod config;
pub mod daemon;
pub mod diff;
pub mod error;
pub mod markdown;
pub mod pipeline;
pub mod planner;
pub mod search;
pub mod store;
pub mod validate;
pub mod walk;
pub mod worker;

pub use error::{Error, Result};

/// Crate version, for `mda --version` and for stamping cards with provenance.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
