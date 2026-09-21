//! Section-level diff between what the index knew about a document and a fresh parse.
//!
//! Identity is the content hash, not the position: a section that moves keeps its hash and is
//! reported as *moved*, not as removed-and-added. This is what makes "insert three lines at the
//! top" a zero-LLM-call operation. Pure; no I/O.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::markdown::Document;

/// What the index previously knew about one section of a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Known {
    /// blake3 hex digest of the section's normalised text.
    pub hash: String,
    /// Position the section had in the document.
    pub index: u32,
}

/// A section that kept its content but changed position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    /// The section's hash.
    pub hash: String,
    /// Where it was.
    pub from: u32,
    /// Where it is now.
    pub to: u32,
}

/// Result of comparing a stored document with a fresh parse.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionDelta {
    /// Hashes present now that were not present before, in document order, deduplicated.
    /// These are the sections that need a model call (unless another document already
    /// produced a card for the same hash).
    pub added: Vec<String>,
    /// Hashes present before and now, at the same index.
    pub unchanged: Vec<String>,
    /// Hashes present before and now, at a different index. Line ranges must be refreshed;
    /// nothing else.
    pub moved: Vec<Moved>,
    /// Hashes present before but not now.
    pub removed: Vec<String>,
}

impl SectionDelta {
    /// Compare `old` (what the store had) with `doc` (what the file contains now).
    ///
    /// Duplicate hashes within one document (two identical sections) are handled by
    /// multiplicity: two old copies and one new copy yields one `removed`.
    #[must_use]
    pub fn compute(old: &[Known], doc: &Document) -> Self {
        // hash -> old indexes (a hash can legitimately appear more than once)
        let mut old_positions: HashMap<&str, Vec<u32>> = HashMap::new();
        for k in old {
            old_positions.entry(k.hash.as_str()).or_default().push(k.index);
        }

        let known: HashMap<&str, u32> = old.iter().map(|k| (k.hash.as_str(), k.index)).collect();
        let mut delta = Self::default();
        let mut seen_added = BTreeSet::new();

        for section in &doc.sections {
            let hash = section.hash.as_str();
            match old_positions.get_mut(hash).and_then(Vec::pop_first_matching_or_any) {
                Some(from) if from == section.index => delta.unchanged.push(hash.to_owned()),
                Some(from) => {
                    delta.moved.push(Moved { hash: hash.to_owned(), from, to: section.index });
                }
                // An extra copy of a hash that existed before: no model call, just a move.
                None if known.contains_key(hash) => {
                    delta.moved.push(Moved {
                        hash: hash.to_owned(),
                        from: known[hash],
                        to: section.index,
                    });
                }
                None => {
                    if seen_added.insert(hash) {
                        delta.added.push(hash.to_owned());
                    }
                }
            }
        }

        // Whatever is left in `old_positions` was not matched by any new section.
        let mut leftovers: Vec<(u32, &str)> = old_positions
            .into_iter()
            .flat_map(|(hash, idxs)| idxs.into_iter().map(move |i| (i, hash)))
            .collect();
        leftovers.sort_unstable();
        delta.removed = leftovers.into_iter().map(|(_, h)| h.to_owned()).collect();
        delta.removed.dedup();

        delta
    }

    /// `true` when nothing changed at all, including positions.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.added.is_empty() && self.moved.is_empty() && self.removed.is_empty()
    }

    /// `true` when no model call is needed (only moves or removals).
    #[must_use]
    pub fn needs_no_summaries(&self) -> bool {
        self.added.is_empty()
    }
}

/// Small helper so the matching above reads as one expression.
trait PopFirst {
    /// Pop the first element, or `None` when empty.
    fn pop_first_matching_or_any(&mut self) -> Option<u32>;
}

impl PopFirst for Vec<u32> {
    fn pop_first_matching_or_any(&mut self) -> Option<u32> {
        if self.is_empty() { None } else { Some(self.remove(0)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse_str;

    fn known(doc: &Document) -> Vec<Known> {
        doc.sections.iter().map(|s| Known { hash: s.hash.clone(), index: s.index }).collect()
    }

    #[test]
    fn identical_parse_is_noop() {
        let a = parse_str("# A\n\none\n\n# B\n\ntwo\n");
        let d = SectionDelta::compute(&known(&a), &a);
        assert!(d.is_noop());
        assert_eq!(d.unchanged.len(), 2);
    }

    #[test]
    fn first_index_is_all_added() {
        let a = parse_str("# A\n\none\n\n# B\n\ntwo\n");
        let d = SectionDelta::compute(&[], &a);
        assert_eq!(d.added.len(), 2);
        assert!(d.unchanged.is_empty() && d.removed.is_empty());
        assert!(!d.needs_no_summaries());
    }

    #[test]
    fn editing_one_section_adds_one_and_removes_one() {
        let a = parse_str("# A\n\none\n\n# B\n\ntwo\n");
        let b = parse_str("# A\n\none\n\n# B\n\ntwo changed\n");
        let d = SectionDelta::compute(&known(&a), &b);
        assert_eq!(d.added, vec![b.sections[1].hash.clone()]);
        assert_eq!(d.removed, vec![a.sections[1].hash.clone()]);
        assert_eq!(d.unchanged, vec![a.sections[0].hash.clone()]);
    }

    #[test]
    fn inserting_a_section_moves_the_rest() {
        let a = parse_str("# A\n\none\n\n# B\n\ntwo\n");
        let b = parse_str("# A\n\none\n\n# New\n\nnew\n\n# B\n\ntwo\n");
        let d = SectionDelta::compute(&known(&a), &b);
        assert_eq!(d.added, vec![b.sections[1].hash.clone()]);
        assert_eq!(d.moved, vec![Moved { hash: a.sections[1].hash.clone(), from: 1, to: 2 }]);
        assert!(d.removed.is_empty());
        assert!(!d.is_noop());
    }

    #[test]
    fn reordering_only_moves() {
        let a = parse_str("# A\n\none\n\n# B\n\ntwo\n");
        let b = parse_str("# B\n\ntwo\n\n# A\n\none\n");
        let d = SectionDelta::compute(&known(&a), &b);
        assert!(d.added.is_empty() && d.removed.is_empty());
        assert_eq!(d.moved.len(), 2);
        assert!(d.needs_no_summaries());
    }

    #[test]
    fn duplicate_sections_use_multiplicity() {
        let a = parse_str("# X\n\nsame\n\n# X\n\nsame\n");
        let b = parse_str("# X\n\nsame\n");
        let d = SectionDelta::compute(&known(&a), &b);
        assert!(d.added.is_empty());
        assert_eq!(d.unchanged.len(), 1);
        assert_eq!(d.removed.len(), 1, "one copy gone");

        let d2 = SectionDelta::compute(&known(&b), &a);
        assert!(d2.added.is_empty(), "the hash is already known");
        assert_eq!(d2.unchanged.len(), 1);
        assert_eq!(d2.moved.len(), 1, "the extra copy is reported as a move");
        assert!(d2.removed.is_empty());
    }

    #[test]
    fn added_is_deduplicated_within_a_document() {
        let b = parse_str("# X\n\nsame\n\n# X\n\nsame\n");
        let d = SectionDelta::compute(&[], &b);
        assert_eq!(d.added.len(), 1);
    }

    #[test]
    fn deleting_everything() {
        let a = parse_str("# A\n\none\n");
        let d = SectionDelta::compute(&known(&a), &parse_str(""));
        assert_eq!(d.removed.len(), 1);
        assert!(d.needs_no_summaries());
    }
}
