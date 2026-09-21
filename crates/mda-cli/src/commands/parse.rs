//! `mda parse <file>` — show what the parser sees. A debugging and demo command.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use mda_core::markdown;

use crate::output::{self, Style};

/// Arguments for `mda parse`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Markdown file to parse.
    pub file: PathBuf,
    /// Also print each section's normalised text.
    #[arg(long)]
    pub text: bool,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let doc = markdown::parse_file(&args.file)
        .with_context(|| format!("parsing {}", args.file.display()))?;

    if json {
        output::json(&doc);
        return Ok(ExitCode::SUCCESS);
    }

    let st = Style::auto();
    println!(
        "{} {} · {} lines · {} sections · ~{} tokens · {}",
        st.bold(&args.file.display().to_string()),
        st.dim(&doc.hash[..12]),
        doc.line_count,
        doc.sections.len(),
        doc.token_estimate,
        doc.title.as_deref().map_or_else(|| st.dim("(no title)"), |t| st.accent(t)),
    );
    if let Some(fm) = &doc.frontmatter {
        println!("  frontmatter: {} line(s)", fm.lines().count());
    }
    for s in &doc.sections {
        let path = if s.heading_path.is_empty() {
            st.dim("(preamble)")
        } else {
            s.heading_path.join(" › ")
        };
        let mut flags = Vec::new();
        if !s.code_langs.is_empty() {
            flags.push(format!("code:{}", s.code_langs.join(",")));
        }
        if s.has_tables {
            flags.push("table".to_owned());
        }
        println!(
            "  {:>3}  L{:<4}–{:<4} ~{:<5} {}  {}  {}",
            s.index,
            s.line_start,
            s.line_end,
            s.token_estimate,
            st.dim(&s.hash[..8]),
            path,
            st.dim(&flags.join(" ")),
        );
        if args.text {
            for line in s.text.lines() {
                println!("         │ {line}");
            }
        }
    }
    if !doc.links_internal.is_empty() || !doc.links_external.is_empty() {
        println!(
            "  links: {} internal, {} external",
            doc.links_internal.len(),
            doc.links_external.len()
        );
    }
    Ok(ExitCode::SUCCESS)
}
