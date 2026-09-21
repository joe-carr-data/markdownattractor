//! `mda backend [api|local|claude-cli]` — show or switch the summarization backend.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::ValueEnum;
use mda_core::config::{Backend, CLAUDE_CLI_POLICY, Config};

use crate::output::{self, Style};

/// Backend choices, as typed on the command line.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Choice {
    /// Claude Messages API with your own API key (default).
    Api,
    /// An OpenAI-compatible local server such as llama.cpp.
    Local,
    /// Spawn your own `claude -p`. Opt-in; see the policy note.
    ClaudeCli,
}

impl From<Choice> for Backend {
    fn from(c: Choice) -> Self {
        match c {
            Choice::Api => Self::Api,
            Choice::Local => Self::Local,
            Choice::ClaudeCli => Self::ClaudeCli,
        }
    }
}

/// Arguments for `mda backend`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Backend to switch to. Omit to show the current one.
    #[arg(value_enum)]
    pub backend: Option<Choice>,
    /// Required with `claude-cli`: acknowledge Anthropic's third-party login policy.
    #[arg(long)]
    pub i_accept_the_policy: bool,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let st = Style::auto();

    // Load leniently: a config that names claude-cli without the acknowledgement must still be
    // readable so the user can switch away from it.
    let mut cfg = match Config::load(&root) {
        Ok(c) => c,
        Err(mda_core::Error::Config(msg)) if msg.contains("claude_cli_policy_ack") => {
            Config { backend: Backend::ClaudeCli, ..Config::default() }
        }
        Err(e) => return Err(e.into()),
    };

    let Some(choice) = args.backend else {
        if json {
            output::json(&serde_json::json!({ "backend": cfg.backend.as_str(), "config": cfg }));
        } else {
            println!("{} {}", st.bold("backend"), describe(&cfg, &st));
        }
        return Ok(ExitCode::SUCCESS);
    };

    let backend: Backend = choice.into();
    if backend == Backend::ClaudeCli && !args.i_accept_the_policy {
        if json {
            output::json(&serde_json::json!({ "error": CLAUDE_CLI_POLICY }));
        } else {
            eprintln!("{}", st.warn(CLAUDE_CLI_POLICY));
            eprintln!("  {} mda backend claude-cli --i-accept-the-policy", st.dim("to proceed:"));
        }
        return Ok(ExitCode::FAILURE);
    }
    cfg.backend = backend;
    cfg.claude_cli_policy_ack = backend == Backend::ClaudeCli && args.i_accept_the_policy;
    cfg.save(&root)?;

    if json {
        output::json(
            &serde_json::json!({ "backend": cfg.backend.as_str(), "saved": Config::path_for(&root) }),
        );
    } else {
        println!("{} {}", st.ok("backend set to"), describe(&cfg, &st));
        println!("  {} mda doctor", st.dim("check it:"));
    }
    Ok(ExitCode::SUCCESS)
}

fn describe(cfg: &Config, st: &Style) -> String {
    match cfg.backend {
        Backend::Api => format!(
            "{} · model {} · escalation {} · key from ${}",
            st.accent("api"),
            cfg.summarization_model,
            cfg.escalation_model.as_deref().unwrap_or("off"),
            cfg.api_key_env
        ),
        Backend::Local => format!(
            "{} · {} at {} · reasoning_effort {}",
            st.accent("local"),
            cfg.local_model,
            cfg.local_base_url,
            cfg.local_reasoning_effort.as_deref().unwrap_or("default")
        ),
        Backend::ClaudeCli => format!(
            "{} · model {} · {}",
            st.accent("claude-cli"),
            cfg.summarization_model,
            st.warn("opt-in: routes requests through your Claude subscription")
        ),
    }
}
