//! `config.toml` — the only configuration file.
//!
//! Lives at `<root>/.markdownattractor/config.toml`. Every field has a default, so an empty
//! file (or no file) is a valid configuration. Field names are the same words the
//! `/mda <setting>` commands use, so the two never drift.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Name of the state directory created under the watched root.
pub const STATE_DIR: &str = ".markdownattractor";

/// Name of the config file inside [`STATE_DIR`].
pub const CONFIG_FILE: &str = "config.toml";

/// Which process produces summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// Spawn the user's own `claude -p`. Uses their Claude Code login; no API key.
    #[default]
    ClaudeCli,
    /// Use `claude --bare -p` with `ANTHROPIC_API_KEY`. Faster startup, costs API dollars.
    Api,
}

/// Top-level configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Model used for section and document cards. Any id `claude --model` accepts.
    pub summarization_model: String,
    /// Model tried once after the summarization model has failed twice on a section.
    /// Defaults to `sonnet`; `None` disables escalation.
    pub escalation_model: Option<String>,
    /// Which process produces summaries.
    pub backend: Backend,
    /// Worker pool size. `None` means adaptive (AIMD, starting at 4).
    pub concurrency: Option<u16>,
    /// Daily token cap across all workers. `None` means unlimited.
    pub daily_token_budget: Option<u64>,
    /// Per-call spend cap handed to `--max-budget-usd`.
    pub per_call_budget_usd: f64,
    /// Wall-clock timeout for one worker call, in seconds.
    pub worker_timeout_secs: u64,
    /// Glob patterns (gitignore syntax) to skip, in addition to `.gitignore` and
    /// `.markdownattractorignore`.
    pub ignore: Vec<String>,
    /// Days to keep tombstones for deleted documents. `None` means forever.
    pub retention_days: Option<u32>,
    /// Inject a one-line reminder when Claude is about to `Grep`/`Read` a markdown file.
    pub nudge: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            summarization_model: "haiku".to_owned(),
            escalation_model: Some("sonnet".to_owned()),
            backend: Backend::default(),
            concurrency: None,
            daily_token_budget: None,
            per_call_budget_usd: 0.05,
            worker_timeout_secs: 90,
            ignore: vec![
                "node_modules/".to_owned(),
                "target/".to_owned(),
                ".git/".to_owned(),
                "vendor/".to_owned(),
                "*.min.md".to_owned(),
            ],
            retention_days: None,
            nudge: true,
        }
    }
}

impl Config {
    /// Path of the config file for a given watched root.
    pub fn path_for(root: &Path) -> PathBuf {
        root.join(STATE_DIR).join(CONFIG_FILE)
    }

    /// Load the config for `root`, returning defaults if the file does not exist.
    pub fn load(root: &Path) -> Result<Self> {
        let path = Self::path_for(root);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Parse from TOML text.
    pub fn from_toml(text: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(text).map_err(|e| Error::Config(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Serialise to TOML text (stable field order, comments not preserved).
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))
    }

    /// Write the config to its path under `root`, creating the state directory if needed.
    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path_for(root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        std::fs::write(&path, self.to_toml()?).map_err(|e| Error::io(path, e))
    }

    fn validate(&self) -> Result<()> {
        if self.summarization_model.trim().is_empty() {
            return Err(Error::Config("summarization_model must not be empty".into()));
        }
        if self.concurrency == Some(0) {
            return Err(Error::Config("concurrency must be at least 1 (or unset for auto)".into()));
        }
        if self.per_call_budget_usd.is_nan() || self.per_call_budget_usd <= 0.0 {
            return Err(Error::Config("per_call_budget_usd must be positive".into()));
        }
        if self.worker_timeout_secs == 0 {
            return Err(Error::Config("worker_timeout_secs must be positive".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_is_defaults() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
    }

    #[test]
    fn round_trips() {
        let cfg = Config { concurrency: Some(8), nudge: false, ..Config::default() };
        let text = cfg.to_toml().unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), cfg);
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = Config::from_toml("modle = \"haiku\"").unwrap_err();
        assert!(matches!(err, Error::Config(_)), "{err}");
    }

    #[test]
    fn rejects_zero_concurrency() {
        assert!(Config::from_toml("concurrency = 0").is_err());
    }

    #[test]
    fn load_missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(dir.path()).unwrap(), Config::default());
    }

    #[test]
    fn save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config { summarization_model: "sonnet".into(), ..Config::default() };
        cfg.save(dir.path()).unwrap();
        assert_eq!(Config::load(dir.path()).unwrap(), cfg);
        assert!(dir.path().join(STATE_DIR).join(CONFIG_FILE).exists());
    }
}
