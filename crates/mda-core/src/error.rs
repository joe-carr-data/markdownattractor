//! Crate-wide error type.
//!
//! Library code returns [`Error`]; only the binary converts it into `anyhow` for display.
//! Variants are coarse on purpose: callers match on *what stage failed*, and the message
//! carries the detail.

use std::path::PathBuf;

/// Everything that can go wrong inside `mda-core`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A filesystem operation failed. `path` is always the path that was being touched.
    #[error("i/o error on {path}: {source}")]
    Io {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// The file is not valid UTF-8, or the markdown could not be parsed.
    #[error("cannot parse {path}: {reason}")]
    Parse {
        /// File that failed to parse.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },

    /// `config.toml` is malformed or contains an invalid value.
    #[error("config error: {0}")]
    Config(String),

    /// A SQLite operation failed.
    #[error("store error: {0}")]
    Store(#[from] rusqlite::Error),

    /// JSON (de)serialisation failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// A model-produced card did not pass validation (schema, caps, or evidence grounding).
    #[error("validation failed for {subject}: {reason}")]
    Validation {
        /// What was being validated, e.g. `section 3 of docs/foo.md`.
        subject: String,
        /// Why it failed.
        reason: String,
    },

    /// The summarization worker could not run or returned something unusable.
    /// Retry classification lives in [`crate::worker`]; this is the terminal form.
    #[error("worker error: {0}")]
    Worker(String),

    /// Walking the watched root failed (bad ignore file, unreadable directory).
    #[error("walk error: {0}")]
    Walk(#[from] ignore::Error),

    /// A stored record is missing or inconsistent (e.g. unknown `section_id`).
    #[error("not found: {0}")]
    NotFound(String),

    /// The daemon could not start, watch, or talk to its socket.
    #[error("daemon error: {0}")]
    Daemon(String),

    /// The embedding model could not be loaded or run.
    #[error("embedding error: {0}")]
    Embed(String),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Build an [`Error::Io`] with the path attached.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path: path.into(), source }
    }

    /// Build an [`Error::Parse`].
    pub fn parse(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self::Parse { path: path.into(), reason: reason.into() }
    }
}
