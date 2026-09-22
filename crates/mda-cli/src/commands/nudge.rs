//! `mda nudge [on|off] [--global]` — the `PreToolUse` reminder that an index exists.
//!
//! Per root, the switch is `nudge` in `config.toml`. `--global` writes or removes the marker
//! file the plugin's hook checks first (`$MDA_NUDGE_FILE`, set by the launcher to
//! `${CLAUDE_PLUGIN_DATA}/nudge.off`).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::ValueEnum;
use mda_core::config::{Config, nudge_off_file, set_global_nudge};

use crate::output::{self, Style};

/// Choices, as typed on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Choice {
    /// Remind Claude to search the index before reading markdown whole (default).
    On,
    /// Stay silent.
    Off,
}

/// Arguments for `mda nudge`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Setting to switch to. Omit to show the current state.
    #[arg(value_enum)]
    pub setting: Option<Choice>,
    /// Apply to every project (the plugin's data directory) instead of this root.
    #[arg(long)]
    pub global: bool,
    /// Watched root.
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// Run the command.
pub fn run(args: &Args, json: bool) -> anyhow::Result<ExitCode> {
    let root = super::resolve_root(args.root.as_deref())?;
    let st = Style::auto();
    let mut cfg = Config::load(&root)?;
    let mut marker = nudge_off_file();
    if let Some(choice) = args.setting {
        let on = choice == Choice::On;
        if args.global {
            marker = set_global_nudge(on)?;
        } else {
            cfg.nudge = on;
            cfg.save(&root)?;
        }
    }
    let global_off = marker.exists();
    let effective = cfg.nudge && !global_off;

    if json {
        output::json(&serde_json::json!({
            "root": root,
            "nudge": cfg.nudge,
            "global_off": global_off,
            "global_marker": marker,
            "effective": effective,
            "saved": args.setting.is_some(),
        }));
        return Ok(ExitCode::SUCCESS);
    }
    let verb = match (args.setting, args.global) {
        (Some(_), true) => "nudge set globally:",
        (Some(_), false) => "nudge set for this root:",
        (None, _) => "nudge",
    };
    println!(
        "{} {} · this root {} · everywhere {}",
        st.ok(verb),
        if effective { st.ok("on") } else { st.warn("off") },
        if cfg.nudge { "on" } else { "off" },
        if global_off { st.warn("off") } else { "on".to_owned() },
    );
    println!("  {} {}", st.dim("global marker:"), st.dim(&marker.display().to_string()));
    Ok(ExitCode::SUCCESS)
}
