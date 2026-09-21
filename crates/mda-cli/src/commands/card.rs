//! `mda card <section_id>` — the full card for a section, rendered as markdown or JSON.

use std::path::PathBuf;
use std::process::ExitCode;

use mda_core::pipeline::Engine;
use mda_core::store::SectionState;

use crate::output::{self, Style};

/// Arguments for `mda card`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Section id from a search hit.
    pub section_id: String,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let engine = Engine::open(&root)?;
    let section = engine.store().section(&args.section_id)?.ok_or_else(|| {
        anyhow::anyhow!("no section {} (run `mda search` to find ids)", args.section_id)
    })?;

    if json {
        output::json(&section);
        return Ok(ExitCode::SUCCESS);
    }

    let st = Style::auto();
    let heading = if section.heading_path.is_empty() {
        "(preamble)".to_owned()
    } else {
        section.heading_path.join(" › ")
    };
    println!(
        "{} › {}   {} · ~{} tok · updated {}",
        st.bold(&section.rel_path),
        heading,
        st.accent(&format!("L{}–{}", section.line_start, section.line_end)),
        section.token_estimate,
        super::humanize_age(section.updated_at),
    );
    match (&section.summary, section.state) {
        (Some(s), _) => {
            println!();
            println!("{}", st.bold(&s.tldr));
            println!("{}", s.summary);
            if !s.questions_answered.is_empty() {
                println!();
                println!("{}", st.dim("answers:"));
                for q in &s.questions_answered {
                    println!("  · {q}");
                }
            }
            if !s.keywords.is_empty() {
                println!("{} {}", st.dim("keywords:"), s.keywords.join(", "));
            }
            if !s.mentioned_dates.is_empty() {
                println!("{}", st.dim("dates:"));
                for d in &s.mentioned_dates {
                    println!("  · {} ({}) — “{}”", d.iso, d.raw, d.evidence);
                }
            }
            if !s.decisions.is_empty() {
                println!("{}", st.dim("decisions:"));
                for d in &s.decisions {
                    println!("  · {d}");
                }
            }
            if !s.action_items.is_empty() {
                println!("{}", st.dim("action items:"));
                for a in &s.action_items {
                    println!("  · {a}");
                }
            }
            let e = &s.entities;
            let mut ents = Vec::new();
            for (name, list) in [
                ("people", &e.people),
                ("orgs", &e.orgs),
                ("products", &e.products),
                ("tech", &e.technologies),
                ("files", &e.files_paths),
                ("commands", &e.commands),
            ] {
                if !list.is_empty() {
                    ents.push(format!("{name}: {}", list.join(", ")));
                }
            }
            if !ents.is_empty() {
                println!("{} {}", st.dim("entities:"), ents.join(" · "));
            }
            if let Some(p) = &section.provenance {
                println!(
                    "{}",
                    st.dim(&format!(
                        "card by {} via {} · prompt {} · schema v{} · {}",
                        p.model,
                        p.backend,
                        p.prompt_version,
                        p.schema_version,
                        super::humanize_age(p.summarized_at)
                    ))
                );
            }
        }
        (None, SectionState::Failed) => {
            println!(
                "{} {}",
                st.fail("no card: summarization failed —"),
                section.fail_reason.as_deref().unwrap_or("unknown reason")
            );
        }
        (None, _) => println!("{}", st.warn("no card yet (pending); raw text is searchable")),
    }
    println!("{}", st.dim(&format!("mda open {}", section.section_id)));
    Ok(ExitCode::SUCCESS)
}
