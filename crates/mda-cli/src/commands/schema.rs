//! `mda schema <kind>` — print a worker JSON schema. `prompts/*.schema.*.json` are generated
//! from this command; a test keeps them in sync.

use std::process::ExitCode;

use clap::ValueEnum;
use mda_core::card::SectionSummary;

use crate::output;

/// Which schema to print.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Kind {
    /// The per-section summary contract.
    Section,
}

/// Arguments for `mda schema`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Which schema to print.
    #[arg(value_enum)]
    pub kind: Kind,
}

/// Run the command. Always JSON; `--json` is implied.
#[expect(clippy::unnecessary_wraps, reason = "every command shares the same signature")]
pub fn run(args: &Args) -> anyhow::Result<ExitCode> {
    match args.kind {
        Kind::Section => output::json(&SectionSummary::json_schema()),
    }
    Ok(ExitCode::SUCCESS)
}
