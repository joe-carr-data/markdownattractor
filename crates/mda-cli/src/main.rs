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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    output::init_logging(cli.verbose);

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
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            output::error(&err, cli.json);
            ExitCode::FAILURE
        }
    }
}
