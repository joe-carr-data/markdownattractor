//! Turn sections into model-sized chunks.
//!
//! v1 keeps this deliberately simple: **one section is one chunk**. Sections larger than the
//! budget are truncated at a paragraph boundary, never inside a fenced code block, and the
//! chunk is flagged so the card's provenance records that the model saw a prefix. Merging tiny
//! sibling sections and splitting huge ones into several cards are Phase 2 refinements;
//! the interface here does not change for them.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::markdown::{Document, Section};

/// Budget for one model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanConfig {
    /// Largest chunk the model receives, in estimated tokens. Text beyond it is cut.
    pub max_tokens: u32,
}

impl Default for PlanConfig {
    fn default() -> Self {
        Self { max_tokens: 6_000 }
    }
}

/// One unit of work for the summarizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Hash of the section this chunk represents. The summary is stored under it.
    pub section_hash: String,
    /// Position of the section in the document.
    pub section_index: u32,
    /// Headings down to the section, for context.
    pub heading_path: Vec<String>,
    /// Text the model receives.
    pub text: String,
    /// Estimated tokens of `text`.
    pub token_estimate: u32,
    /// `true` when `text` is a prefix of the section, not the whole of it.
    pub truncated: bool,
}

/// Plan chunks for every section in the document, in document order.
#[must_use]
pub fn plan(doc: &Document, cfg: &PlanConfig) -> Vec<Chunk> {
    doc.sections.iter().map(|s| chunk_for(s, cfg)).collect()
}

/// Plan a chunk for a single section.
#[must_use]
pub fn chunk_for(section: &Section, cfg: &PlanConfig) -> Chunk {
    let (text, truncated) = chunk_text(&section.text, cfg);
    Chunk {
        section_hash: section.hash.clone(),
        section_index: section.index,
        heading_path: section.heading_path.clone(),
        token_estimate: crate::markdown::token_estimate(&text),
        text,
        truncated,
    }
}

/// Fit raw section text into the budget: returns the text to send and whether it was cut.
#[must_use]
pub fn chunk_text(text: &str, cfg: &PlanConfig) -> (String, bool) {
    if crate::markdown::token_estimate(text) <= cfg.max_tokens {
        (text.to_owned(), false)
    } else {
        (truncate_at_paragraph(text, cfg.max_tokens), true)
    }
}

/// Cut `text` to at most `max_tokens` estimated tokens, at the last blank line before the limit
/// that is not inside a fenced code block. Appends a marker naming how many lines were dropped.
fn truncate_at_paragraph(text: &str, max_tokens: u32) -> String {
    let max_chars = (max_tokens as usize).saturating_mul(4);
    let total_lines = text.lines().count();

    let mut chars_so_far = 0usize;
    let mut in_fence = false;
    let mut fence_open_line: Option<usize> = None;
    let mut last_boundary_line = 0usize; // number of lines to keep

    for (i, line) in text.lines().enumerate() {
        let next = chars_so_far + line.chars().count() + 1;
        if next > max_chars {
            break;
        }
        chars_so_far = next;
        if is_fence(line) {
            if in_fence {
                in_fence = false;
                fence_open_line = None;
            } else {
                in_fence = true;
                fence_open_line = Some(i);
            }
        }
        if line.trim().is_empty() && !in_fence {
            last_boundary_line = i;
        }
    }

    // If we stopped inside a fence, back up to before it opened.
    if in_fence && let Some(open) = fence_open_line {
        last_boundary_line = last_boundary_line.min(open);
    }
    if last_boundary_line == 0 {
        // No blank line before the limit: keep the heading line at least.
        last_boundary_line = 1.min(total_lines);
    }

    let kept: Vec<&str> = text.lines().take(last_boundary_line).collect();
    let dropped = total_lines.saturating_sub(kept.len());
    let mut out = kept.join("\n");
    // Writing to a String cannot fail.
    let _ =
        write!(out, "\n\n[… section truncated for summarization: {dropped} more lines not shown]");
    out
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse_str;

    #[test]
    fn small_sections_are_passed_through() {
        let doc = parse_str("# A\n\nshort\n\n# B\n\nalso short\n");
        let chunks = plan(&doc, &PlanConfig::default());
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|c| !c.truncated));
        assert_eq!(chunks[1].text, doc.sections[1].text);
        assert_eq!(chunks[1].section_hash, doc.sections[1].hash);
        assert_eq!(chunks[1].heading_path, vec!["B"]);
    }

    #[test]
    fn large_section_is_truncated_at_a_blank_line() {
        let para = "word ".repeat(50); // ~250 chars ≈ 63 tokens
        let text = format!("# Big\n\n{para}\n\n{para}\n\n{para}\n\n{para}\n");
        let doc = parse_str(&text);
        let cfg = PlanConfig { max_tokens: 100 };
        let c = &plan(&doc, &cfg)[0];
        assert!(c.truncated);
        assert!(c.text.contains("truncated for summarization"));
        assert!(c.token_estimate <= 120, "marker adds a little: {}", c.token_estimate);
        assert!(c.text.starts_with("# Big\n\nword "));
        // Cut happened at a paragraph boundary: no dangling partial paragraph.
        let body = c.text.split("\n\n[…").next().unwrap();
        assert!(body.ends_with("word") || body.ends_with("word "), "{body:?}");
    }

    #[test]
    fn never_cuts_inside_a_code_fence() {
        let code = "line of code\n".repeat(40);
        let text = format!("# Big\n\nintro paragraph here.\n\n```rust\n{code}```\n\nafter\n");
        let doc = parse_str(&text);
        let cfg = PlanConfig { max_tokens: 60 };
        let c = &plan(&doc, &cfg)[0];
        assert!(c.truncated);
        let body = c.text.split("\n\n[…").next().unwrap();
        assert!(!body.contains("```rust"), "fence must be cut before it opens: {body}");
        assert!(body.contains("intro paragraph"));
    }

    #[test]
    fn no_blank_line_keeps_at_least_the_heading() {
        let text = format!("# H\n{}\n", "x".repeat(2000));
        let doc = parse_str(&text);
        let c = &plan(&doc, &PlanConfig { max_tokens: 10 })[0];
        assert!(c.truncated);
        assert!(c.text.starts_with("# H\n"));
    }

    #[test]
    fn truncation_marker_counts_dropped_lines() {
        let text = format!("# H\n\n{}", "para\n\n".repeat(30));
        let doc = parse_str(&text);
        let c = &plan(&doc, &PlanConfig { max_tokens: 8 })[0];
        let dropped: usize = c
            .text
            .rsplit("summarization: ")
            .next()
            .and_then(|s| s.split(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap();
        assert!(dropped > 0);
    }
}
