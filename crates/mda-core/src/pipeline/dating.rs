//! The git dating pass (ADR-0008): give documents and sections their authored times from git
//! history, when — and only when — git's bytes can be proven to be the indexed bytes.
//!
//! The pass never runs before a file is raw-searchable and never on the save path. It reads
//! one pinned commit, computes everything outside a transaction, and writes each document in
//! one short conditional transaction ([`Store::apply_git_dates`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::Engine;
use crate::Result;
use crate::config::TimeSourceMode;
use crate::gittime::{self, Discovery, Repo};
use crate::markdown;
use crate::store::{GitDates, GitState, SectionKey, StoredDocument};

/// Limits on one call of [`Engine::date_from_git`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DateLimits<'a> {
    /// Stop after examining this many documents (`None`: no limit).
    pub max_documents: Option<usize>,
    /// Stop when this instant has passed (`None`: no limit).
    pub deadline: Option<Instant>,
    /// Stop when this flag is set.
    pub cancel: Option<&'a AtomicBool>,
    /// Examine only documents whose path sorts after this one: the cursor of a sweep made of
    /// several calls ([`DateReport::last_path`] of the previous call).
    pub start_after: Option<&'a str>,
}

impl DateLimits<'_> {
    fn reached(&self, examined: usize) -> bool {
        // The deadline never stops a call before its first document: discovery and the tree
        // listing count against it, and a call that only set up would never get anywhere.
        self.max_documents.is_some_and(|m| examined >= m)
            || (examined > 0 && self.deadline.is_some_and(|d| Instant::now() >= d))
            || self.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }
}

/// What one call of [`Engine::date_from_git`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateReport {
    /// Why git was or was not used: `usable`, `disabled` (`time_source = "filesystem"`),
    /// `clock_override` (`MDA_NOW`), `no_git`, `not_a_repository`, `shallow`.
    pub source: String,
    /// The commit the pass read, when git was usable.
    pub commit: Option<String>,
    /// Documents now carrying git times.
    pub dated: usize,
    /// Documents examined and proven not eligible (untracked, modified, filtered, …).
    pub ineligible: usize,
    /// Documents on which a read or a git call failed.
    pub failed: usize,
    /// Documents whose index was not the file on disk, or changed during the pass; they wait.
    pub skipped: usize,
    /// Documents still waiting for a date when the call returned.
    pub remaining: u64,
    /// Documents this call examined, whatever the outcome.
    pub examined: usize,
    /// The path of the last document examined: the next call's `start_after` in a sweep.
    pub last_path: Option<String>,
}

impl DateReport {
    fn off(source: &str) -> Self {
        Self { source: source.to_owned(), ..Self::default() }
    }

    /// Whether the call changed anything, so another call could make further progress.
    #[must_use]
    pub fn progressed(&self) -> bool {
        self.dated + self.ineligible + self.failed > 0
    }
}

/// The verdict on one document.
enum Verdict {
    /// Git times, ready to store with the section identities they were computed for.
    Dated(Vec<SectionKey>, GitDates),
    /// Proven not eligible; `blob` is the working file's blob id when it was computed.
    Ineligible { blob: Option<String>, why: &'static str },
    /// The index does not hold the file on disk; the indexer has to catch up first.
    Mismatch,
}

impl Engine {
    /// Why git cannot be used for this root right now, or `None` when it may be tried.
    fn git_times_off(&self) -> Option<&'static str> {
        if crate::clock::override_active() {
            // A replay stamps the store with history's dates through the clock (ADR-0006).
            Some("clock_override")
        } else if self.config.time_source == TimeSourceMode::Filesystem {
            Some("disabled")
        } else {
            None
        }
    }

    /// Whether a dating pass may call git at all for this root (`time_source`, `MDA_NOW`).
    #[must_use]
    pub fn git_times_enabled(&self) -> bool {
        self.git_times_off().is_none()
    }

    /// The commit `HEAD` names, when git may be used and the root is in a full work tree.
    /// One cheap call; the daemon polls it to notice commits.
    #[must_use]
    pub fn git_head(&self) -> Option<String> {
        if !self.git_times_enabled() {
            return None;
        }
        match gittime::discover(&self.root) {
            Discovery::Usable(repo) => repo.head().ok(),
            _ => None,
        }
    }

    /// Date waiting documents from git, within `limits`.
    ///
    /// Pins `HEAD` to a commit; when it differs from the commit the store was last validated
    /// against, marks the documents whose history changed in between (all of them when the
    /// old commit is not an ancestor, or the history cannot be listed); then examines waiting
    /// documents in path order. Does nothing, and calls no git, under `MDA_NOW` or with
    /// `time_source = "filesystem"`.
    pub fn date_from_git(&mut self, limits: DateLimits<'_>) -> Result<DateReport> {
        if let Some(why) = self.git_times_off() {
            return Ok(DateReport::off(why));
        }
        let repo = match gittime::discover(&self.root) {
            Discovery::Usable(repo) => repo,
            other => return Ok(DateReport::off(other.name())),
        };
        let commit = repo.head()?;
        let mut report = DateReport {
            source: "usable".to_owned(),
            commit: Some(commit.clone()),
            ..DateReport::default()
        };

        let validated = self.store.git_validated_commit()?;
        if validated.as_deref() != Some(commit.as_str()) {
            // Paths whose history changed since the validated commit; `None` marks everything.
            let changed = match &validated {
                Some(old) if repo.is_ancestor(old, &commit).unwrap_or(false) => {
                    repo.changed_paths(old, &commit).ok()
                }
                _ => None,
            };
            let waiting = self.store.advance_git_commit(&commit, changed.as_deref())?;
            tracing::debug!(commit, waiting, all = changed.is_none(), "git commit changed");
        }

        let tree = repo.tree(&commit)?;
        let now = crate::clock::now();
        let waiting = self.store.documents_needing_git(limits.start_after, usize::MAX)?;
        for (examined, doc) in waiting.iter().enumerate() {
            if limits.reached(examined) {
                break;
            }
            report.examined = examined + 1;
            report.last_path = Some(doc.rel_path.clone());
            match self.examine(&repo, &commit, &tree, doc, now) {
                Ok(Verdict::Dated(keys, dates)) => {
                    if self.store.apply_git_dates(
                        &doc.doc_id,
                        &doc.content_hash,
                        doc.file_mtime,
                        &keys,
                        &dates,
                    )? {
                        report.dated += 1;
                    } else {
                        report.skipped += 1;
                    }
                }
                Ok(Verdict::Ineligible { blob, why }) => {
                    tracing::debug!(path = doc.rel_path, why, "not dated from git");
                    if self.store.set_git_state(
                        &doc.doc_id,
                        &doc.content_hash,
                        doc.file_mtime,
                        GitState::Filesystem,
                        Some(&commit),
                        blob.as_deref(),
                    )? {
                        report.ineligible += 1;
                    } else {
                        report.skipped += 1;
                    }
                }
                Ok(Verdict::Mismatch) => report.skipped += 1,
                Err(e) => {
                    tracing::warn!(path = doc.rel_path, error = %e, "git dating failed");
                    if self.store.set_git_state(
                        &doc.doc_id,
                        &doc.content_hash,
                        doc.file_mtime,
                        GitState::Failed,
                        Some(&commit),
                        None,
                    )? {
                        report.failed += 1;
                    } else {
                        report.skipped += 1;
                    }
                }
            }
        }
        report.remaining = self.store.git_state_counts()?.needs;
        Ok(report)
    }

    /// Decide one document. An `Err` is an operational failure (a read or a git call).
    fn examine(
        &self,
        repo: &Repo,
        commit: &str,
        tree: &HashMap<String, String>,
        doc: &StoredDocument,
        now: Timestamp,
    ) -> Result<Verdict> {
        let ineligible = |blob: Option<String>, why| Ok(Verdict::Ineligible { blob, why });
        if doc.rel_path.contains('\n') {
            return ineligible(None, "a newline in the path");
        }
        // Exact, case-sensitive: the path the index holds must be a regular file in the tree.
        let Some(tree_blob) = tree.get(&doc.rel_path) else {
            return ineligible(None, "not a regular file in the commit");
        };
        let abs = self.safe_join(&doc.rel_path)?;
        let bytes = std::fs::read(&abs).map_err(|e| crate::Error::io(&abs, e))?;
        if bytes.contains(&b'\r') {
            // The parser breaks lines on a carriage return; git does not.
            return ineligible(None, "carriage returns");
        }
        let blob = repo.blob_of(&bytes)?;
        if blob != *tree_blob {
            return ineligible(Some(blob), "differs from the commit");
        }
        let line_total = gittime::line_count(&bytes);
        let Ok(text) = String::from_utf8(bytes) else {
            return ineligible(Some(blob), "not UTF-8");
        };

        // The bytes git describes are these bytes. Are they what the index holds?
        let parsed = markdown::parse_str_as(&text, markdown::Flavor::of_path(&abs));
        if parsed.hash != doc.content_hash {
            return Ok(Verdict::Mismatch);
        }
        let keys = self.store.section_keys(&doc.doc_id)?;
        let same = keys.len() == parsed.sections.len()
            && keys.iter().zip(&parsed.sections).all(|(k, s)| {
                k.section_hash == s.hash && k.line_start == s.line_start && k.line_end == s.line_end
            });
        if !same {
            return Ok(Verdict::Mismatch);
        }

        let Some((mut created, mut updated)) = repo.document_times(commit, &doc.rel_path, now)?
        else {
            return Err(crate::Error::Git(format!("no history for {}", doc.rel_path)));
        };
        let lines = repo.line_times(commit, &doc.rel_path, now)?;
        if lines.len() != line_total {
            return Err(crate::Error::Git(format!(
                "blame covers {} of {line_total} lines of {}",
                lines.len(),
                doc.rel_path
            )));
        }
        let mut section_updated_at = Vec::with_capacity(keys.len());
        for key in &keys {
            let (first, last) = (key.line_start as usize, key.line_end as usize);
            let newest = (first >= 1 && first <= last)
                .then(|| lines.get(first - 1..last))
                .flatten()
                .and_then(|span| span.iter().max().copied());
            let Some(newest) = newest else {
                return Err(crate::Error::Git(format!(
                    "section lines {first}-{last} outside the {line_total} lines of {}",
                    doc.rel_path
                )));
            };
            created = created.min(newest);
            updated = updated.max(newest);
            section_updated_at.push(newest);
        }
        let dates = GitDates {
            commit: commit.to_owned(),
            blob,
            created_at: created,
            updated_at: updated,
            section_updated_at,
        };
        Ok(Verdict::Dated(keys, dates))
    }

    /// Put git-dated documents back on filesystem times when `time_source = "filesystem"`.
    /// Returns how many documents were restored (0 when the setting is `auto`).
    pub fn restore_filesystem_times(&mut self) -> Result<u64> {
        if self.config.time_source != TimeSourceMode::Filesystem {
            return Ok(0);
        }
        self.store.restore_filesystem_times()
    }
}
