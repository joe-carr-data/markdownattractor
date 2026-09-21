//! Presentation helpers: logging setup, JSON envelope, error rendering.
//!
//! Conventions every command follows:
//!
//! - Human output is one result line first, details after. Colour is never required to
//!   understand it, and `NO_COLOR` is honoured.
//! - `--json` prints exactly one JSON document to stdout and nothing else.
//! - Errors go to stderr; with `--json` they are `{"error": "..."}` on stdout so a caller only
//!   ever has to parse one stream.

use serde::Serialize;
use tracing_subscriber::EnvFilter;

/// Route `tracing` to stderr at a level chosen by `-v` count, overridable with `RUST_LOG`.
pub fn init_logging(verbosity: u8) {
    let default = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(verbosity >= 2)
        .without_time()
        .init();
}

/// Print a value as pretty JSON on stdout.
pub fn json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("error: could not serialise output: {e}"),
    }
}

/// Render an error, honouring `--json`.
pub fn error(err: &anyhow::Error, json_mode: bool) {
    if json_mode {
        let chain: Vec<String> = err.chain().map(ToString::to_string).collect();
        json(&serde_json::json!({ "error": chain[0], "causes": &chain[1..] }));
    } else {
        eprintln!("error: {err:#}");
    }
}

/// `true` when colour output is appropriate: stdout is a terminal and `NO_COLOR` is unset.
pub fn use_color() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

/// Minimal ANSI styling that degrades to plain text.
pub struct Style {
    on: bool,
}

impl Style {
    /// Detect from the environment.
    pub fn auto() -> Self {
        Self { on: use_color() }
    }

    /// Bold.
    pub fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }

    /// Dim.
    pub fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }

    /// Green.
    pub fn ok(&self, s: &str) -> String {
        self.wrap("32", s)
    }

    /// Yellow.
    pub fn warn(&self, s: &str) -> String {
        self.wrap("33", s)
    }

    /// Red.
    pub fn fail(&self, s: &str) -> String {
        self.wrap("31", s)
    }

    /// Cyan.
    pub fn accent(&self, s: &str) -> String {
        self.wrap("36", s)
    }

    fn wrap(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_owned() }
    }
}
