//! `mda` — the markdownattractor command-line interface.
//!
//! Thin by design: argument parsing, output formatting, and process exit codes live here;
//! everything else is in `mda-core`. Every command supports `--json` so that skills and scripts
//! never have to parse human-formatted output.

#![allow(clippy::print_stdout, clippy::print_stderr)] // this crate *is* the presentation layer

mod commands;
mod output;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// A time-aware, searchable knowledge layer over your markdown, for Claude Code.
#[derive(Debug, Parser)]
#[command(name = "mda", version, about, long_about = None, propagate_version = true)]
struct Cli {
    /// Emit machine-readable JSON instead of human-formatted output.
    #[arg(long, global = true)]
    json: bool,

    /// Increase log verbosity (-v info, -vv debug, -vvv trace). Logs go to stderr.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Index the root (or one file): parse, make raw-searchable, then summarize.
    Index(commands::index::Args),
    /// Search the index: cards and raw text, fused, time-aware.
    Search(commands::search::Args),
    /// Print the exact source lines of a section (re-checked against the file).
    Open(commands::open::Args),
    /// Show the full card of a section.
    Card(commands::card::Args),
    /// What is indexed, pending, failed, and what it cost.
    Status(commands::status::Args),
    /// Show or switch the summarization backend (api, local, claude-cli).
    Backend(commands::backend::Args),
    /// Parse a markdown file and show its sections, line ranges and hashes.
    Parse(commands::parse::Args),
    /// Print the JSON schema handed to the summarization model.
    Schema(commands::schema::Args),
    /// Check that everything mda needs is present and working.
    Doctor(commands::doctor::Args),
    /// Start the daemon: watch the root, index on save, summarize in the background.
    Start(commands::start::Args),
    /// Stop the daemon gracefully; unfinished work resumes on the next start.
    Stop(commands::stop::Args),
    /// Stop and start the daemon (after config changes).
    Restart(commands::stop::Args),
    /// Stream the daemon's events live.
    Watch(commands::watch::Args),
    /// Keep indexing but stop calling the model.
    Pause(commands::pause::Args),
    /// Resume summarization after `pause`.
    Resume(commands::pause::Args),
    /// Show the score breakdown behind a query: cards, raw text, vectors, fused.
    Explain(commands::explain::Args),
    /// What was created, changed, renamed or deleted, grouped by day.
    Timeline(commands::timeline::Args),
    /// The most recently updated documents.
    Recent(commands::recent::Args),
    /// Documents whose index is not final (should print nothing).
    Stale(commands::stale::Args),
    /// Regenerate derived artefacts (`--embeddings`).
    Rebuild(commands::rebuild::Args),
    /// Show or switch the embedding setting (local-small, off).
    Embeddings(commands::embeddings::Args),
    /// Serve the index to Claude over MCP (stdio).
    Mcp(commands::mcp::Args),
    /// Retrieval metrics (recall@k, MRR) on a golden set, offline.
    Eval(Box<commands::eval::Args>),
    /// What summarization cost, per day and per model, from the usage ledger.
    Cost(commands::cost::Args),
    /// Write a redacted diagnostics bundle to attach to an issue.
    Diagnostics(commands::diagnostics::Args),
    /// Turn the "search first" reminder on or off (per root, or --global).
    Nudge(commands::nudge::Args),
    /// The daemon process itself (spawned by `start`).
    #[command(hide = true)]
    Daemon(commands::daemon::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match &cli.command {
        Command::Daemon(args) if args.log_dir.is_some() => {
            if let Some(dir) = &args.log_dir
                && let Err(e) = output::init_file_logging(dir)
            {
                eprintln!("error: cannot open log directory {}: {e}", dir.display());
                return ExitCode::FAILURE;
            }
        }
        _ => output::init_logging(cli.verbose),
    }

    let result = match cli.command {
        Command::Index(args) => commands::index::run(&args, cli.json),
        Command::Search(args) => commands::search::run(&args, cli.json),
        Command::Open(args) => commands::open::run(&args, cli.json),
        Command::Card(args) => commands::card::run(&args, cli.json),
        Command::Status(args) => commands::status::run(&args, cli.json),
        Command::Backend(args) => commands::backend::run(&args, cli.json),
        Command::Parse(args) => commands::parse::run(&args, cli.json),
        Command::Schema(args) => commands::schema::run(&args),
        Command::Doctor(args) => commands::doctor::run(&args, cli.json),
        Command::Start(args) => commands::start::run(&args, cli.json),
        Command::Stop(args) => commands::stop::run(&args, cli.json),
        Command::Restart(args) => commands::stop::restart(&args, cli.json),
        Command::Watch(args) => commands::watch::run(&args, cli.json),
        Command::Pause(args) => commands::pause::pause(&args, cli.json),
        Command::Resume(args) => commands::pause::resume(&args, cli.json),
        Command::Explain(args) => commands::explain::run(&args, cli.json),
        Command::Timeline(args) => commands::timeline::run(&args, cli.json),
        Command::Recent(args) => commands::recent::run(&args, cli.json),
        Command::Stale(args) => commands::stale::run(&args, cli.json),
        Command::Rebuild(args) => commands::rebuild::run(&args, cli.json),
        Command::Embeddings(args) => commands::embeddings::run(&args, cli.json),
        Command::Mcp(args) => commands::mcp::run(&args, cli.json),
        Command::Eval(args) => commands::eval::run(&args, cli.json),
        Command::Cost(args) => commands::cost::run(&args, cli.json),
        Command::Diagnostics(args) => commands::diagnostics::run(&args, cli.json),
        Command::Nudge(args) => commands::nudge::run(&args, cli.json),
        Command::Daemon(args) => commands::daemon::run(&args, cli.json),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            output::error(&err, cli.json);
            ExitCode::FAILURE
        }
    }
}
