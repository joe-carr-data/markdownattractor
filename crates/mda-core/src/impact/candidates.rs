//! Step 1 of a review: which other sections could a change have left out of date?
//!
//! No model is called here. A change yields terms ([`super::terms`]); the raw full-text index
//! answers which sections carry them; the documents' own links say which documents stand next
//! to the changed one. The result is a short, deterministic list, with counts of what was
//! found and what the caps cut.
//!
//! - **Terms.** At most [`Limits::max_terms`], the replaced wording first. A term carried by
//!   more than a fifth of all sections says nothing and is dropped (with a floor, so that a
//!   small folder keeps its terms); so is a term no section carries. Among plain words the
//!   rarest come first.
//! - **Query safety.** Every term reaches FTS5 as one quoted phrase ([`fts_literal`]) joined
//!   by `OR`: file content never becomes query syntax, and there is no prefix matching.
//! - **Order.** Sections carrying a term of the change's leading role (the replaced wording
//!   when there is any) come before sections that only share other terms; within each of
//!   the two groups, sections of linked documents come first, then the rest by rank.
//! - **Caps.** At most [`Limits::per_document`] sections of one document and
//!   [`Limits::per_change`] in all.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Serialize;

use super::changes::SectionChange;
use super::terms::{Role, Term, TermKind, terms_for};
use crate::Result;
use crate::store::{Store, StoredSection, doc_id_for, fts_literal};

/// The caps of the candidate step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limits {
    /// Terms in the query.
    pub max_terms: usize,
    /// Terms of a change whose frequency is looked up before the best are chosen.
    pub max_terms_examined: usize,
    /// Longest term, in bytes; a longer one (a long URL) is left out.
    pub max_term_bytes: usize,
    /// A term carried by more than this share of all sections is dropped.
    pub common_fraction: f64,
    /// A term carried by this many sections or fewer is never dropped as common.
    pub common_floor: u64,
    /// Sections of one document among the candidates of a change.
    pub per_document: usize,
    /// Candidates of a change.
    pub per_change: usize,
    /// Hits asked of the index per query, before the caps.
    pub pool: usize,
    /// Linked documents searched for one change (in path order).
    pub max_linked_docs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_terms: 12,
            max_terms_examined: 48,
            max_term_bytes: 120,
            common_fraction: 0.20,
            common_floor: 10,
            per_document: 2,
            per_change: 8,
            pool: 50,
            max_linked_docs: 50,
        }
    }
}

/// A term that went into the query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryTerm {
    /// The term.
    #[serde(flatten)]
    pub term: Term,
    /// How many sections carry it.
    pub sections: u64,
}

/// How a candidate was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Its document links to the changed document, or is linked from it, and it shares a term.
    Linked,
    /// It shares terms with the change.
    Lexical,
}

/// A section to compare the change against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// The section as the index holds it.
    pub section: StoredSection,
    /// How it was found.
    pub source: Source,
    /// Whether it carries a term of the change's leading role (the replaced wording, when
    /// the change replaced any).
    pub leading: bool,
    /// The query terms it carries, in query order.
    pub shared_terms: Vec<String>,
}

/// The candidates of one change, and what happened on the way.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Candidates {
    /// The terms of the query, best first.
    pub terms: Vec<QueryTerm>,
    /// Terms dropped because too many sections carry them.
    pub dropped_common: Vec<String>,
    /// The candidates, best first.
    pub kept: Vec<Candidate>,
    /// Distinct sections the queries returned (each query is bounded by [`Limits::pool`]),
    /// the changed section and exact copies of its new text left out.
    pub retrieved: usize,
    /// Of those, how many the caps cut.
    pub cut_by_caps: usize,
    /// Linked documents left unsearched by [`Limits::max_linked_docs`].
    pub linked_documents_cut: usize,
}

/// Finds candidates against one store. Built once per review: it reads the section count and
/// the documents' links at construction.
pub struct Finder<'a> {
    store: &'a Store,
    limits: Limits,
    total_sections: u64,
    /// For a document path, the paths of the documents it links to or is linked from.
    neighbours: HashMap<String, BTreeSet<String>>,
}

impl<'a> Finder<'a> {
    /// Read what every change of a review shares.
    pub fn new(store: &'a Store, limits: Limits) -> Result<Self> {
        let docs = store.documents()?;
        let known: HashSet<&str> = docs.iter().map(|d| d.rel_path.as_str()).collect();
        let mut neighbours: HashMap<String, BTreeSet<String>> = HashMap::new();
        for doc in &docs {
            for target in &doc.links_internal {
                let Some(other) = resolve_link(&doc.rel_path, target, &known) else { continue };
                if other == doc.rel_path {
                    continue;
                }
                neighbours.entry(doc.rel_path.clone()).or_default().insert(other.clone());
                neighbours.entry(other).or_default().insert(doc.rel_path.clone());
            }
        }
        let total_sections = store.counts()?.sections;
        Ok(Self { store, limits, total_sections, neighbours })
    }

    /// The candidates for `change`, a change of the document at `rel_path`.
    pub fn for_change(&self, rel_path: &str, change: &SectionChange) -> Result<Candidates> {
        let mut out = Candidates::default();
        let chosen = self.choose_terms(change, &mut out)?;
        let Some(leading_role) = chosen.iter().map(|(t, _)| t.term.role).min() else {
            return Ok(out);
        };
        let or = |pick: &dyn Fn(&QueryTerm) -> bool| -> String {
            let parts: Vec<&str> =
                chosen.iter().filter(|(t, _)| pick(t)).map(|(_, lit)| lit.as_str()).collect();
            parts.join(" OR ")
        };
        let leading_expr = or(&|t| t.term.role == leading_role);
        let all_expr = or(&|_| true);

        let linked = self.linked_doc_ids(rel_path, &mut out);
        let pool = self.limits.pool;
        // Leading group first; in each group the linked documents' sections, then the rest.
        let lists = [
            (self.store.search_raw_in_docs(&leading_expr, &linked, pool)?, Source::Linked, true),
            (self.store.search_raw(&leading_expr, pool)?, Source::Lexical, true),
            (self.store.search_raw_in_docs(&all_expr, &linked, pool)?, Source::Linked, false),
            (self.store.search_raw(&all_expr, pool)?, Source::Lexical, false),
        ];

        let own_doc = doc_id_for(rel_path);
        let mut seen: HashSet<String> = HashSet::new();
        let mut per_doc: HashMap<String, usize> = HashMap::new();
        for (hits, source, leading) in lists {
            for hit in hits {
                if !seen.insert(hit.section_id.clone()) {
                    continue;
                }
                let Some(section) = self.store.section(&hit.section_id)? else { continue };
                // The changed section itself, and any section that already reads as it does
                // now, cannot be out of date with it.
                if section.section_hash == change.after.hash
                    || (section.doc_id == own_doc && section.heading_path == change.heading_path)
                {
                    continue;
                }
                out.retrieved += 1;
                let of_doc = per_doc.entry(section.doc_id.clone()).or_default();
                if out.kept.len() >= self.limits.per_change || *of_doc >= self.limits.per_document {
                    out.cut_by_caps += 1;
                    continue;
                }
                *of_doc += 1;
                out.kept.push(Candidate { section, source, leading, shared_terms: Vec::new() });
            }
        }
        self.fill_shared_terms(&chosen, &mut out)?;
        out.terms = chosen.into_iter().map(|(t, _)| t).collect();
        Ok(out)
    }

    /// The query terms of a change with their FTS literal, best first.
    fn choose_terms(
        &self,
        change: &SectionChange,
        out: &mut Candidates,
    ) -> Result<Vec<(QueryTerm, String)>> {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a share of a section count; the count is far below 2^52"
        )]
        let share = (self.total_sections as f64 * self.limits.common_fraction).floor() as u64;
        let common_above = share.max(self.limits.common_floor);

        let mut scored: Vec<(usize, QueryTerm, String)> = Vec::new();
        let usable = terms_for(change).into_iter().filter_map(|t| {
            if t.text.len() > self.limits.max_term_bytes {
                return None;
            }
            fts_literal(&t.text).map(|lit| (t, lit))
        });
        for (order, (term, literal)) in usable.take(self.limits.max_terms_examined).enumerate() {
            let sections = self.store.raw_match_count(&literal)?;
            if sections == 0 {
                continue;
            }
            if sections > common_above {
                out.dropped_common.push(term.text);
                continue;
            }
            scored.push((order, QueryTerm { term, sections }, literal));
        }
        // Role, then identifiers and numbers in order of appearance, then words rarest first.
        scored.sort_by_key(|(order, t, _)| {
            let rarity = if t.term.kind == TermKind::Word { t.sections } else { 0 };
            (t.term.role, t.term.kind, rarity, *order)
        });
        scored.truncate(self.limits.max_terms);
        Ok(scored.into_iter().map(|(_, t, lit)| (t, lit)).collect())
    }

    /// Ids of the documents linked with `rel_path` either way, in path order, capped.
    fn linked_doc_ids(&self, rel_path: &str, out: &mut Candidates) -> Vec<String> {
        let Some(paths) = self.neighbours.get(rel_path) else { return Vec::new() };
        out.linked_documents_cut = paths.len().saturating_sub(self.limits.max_linked_docs);
        paths.iter().take(self.limits.max_linked_docs).map(|p| doc_id_for(p)).collect()
    }

    /// Record which query terms each kept candidate carries.
    fn fill_shared_terms(
        &self,
        chosen: &[(QueryTerm, String)],
        out: &mut Candidates,
    ) -> Result<()> {
        let ids: Vec<String> = out.kept.iter().map(|c| c.section.section_id.clone()).collect();
        for (term, literal) in chosen {
            let carrying: HashSet<String> =
                self.store.raw_matches_among(literal, &ids)?.into_iter().collect();
            for candidate in &mut out.kept {
                if carrying.contains(&candidate.section.section_id) {
                    candidate.shared_terms.push(term.term.text.clone());
                }
            }
        }
        Ok(())
    }
}

/// The indexed document a link in `from` points at, if any. Text only: the fragment and the
/// query are cut, a leading `/` means the indexed root, `.` and `..` are resolved, and a
/// target above the root is nothing. A target without an extension is tried as a page
/// (`x.md`, `x.mdx`, `x/index.md`, `x/README.md`, `x/index.mdx`).
fn resolve_link(from: &str, target: &str, known: &HashSet<&str>) -> Option<String> {
    let target = target.split(['#', '?']).next().unwrap_or_default().trim();
    if target.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        parts.extend(from.split('/'));
        parts.pop(); // the file name
    }
    for piece in target.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    let path = parts.join("/");
    if path.is_empty() {
        return None;
    }
    if known.contains(path.as_str()) {
        return Some(path);
    }
    ["{}.md", "{}.mdx", "{}/index.md", "{}/README.md", "{}/index.mdx"]
        .iter()
        .map(|shape| shape.replace("{}", &path))
        .find(|p| known.contains(p.as_str()))
}

// `Role` orders the groups above; keep its order in step with this module's docs.
const _: () = assert!((Role::Replaced as u8) < (Role::Context as u8));

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use jiff::Timestamp;

    use super::*;
    use crate::impact::compare;
    use crate::markdown::{Flavor, parse_str};
    use crate::store::DocTimes;

    fn times() -> DocTimes {
        let at = Timestamp::from_second(1_700_000_000).unwrap();
        DocTimes { created_at: None, modified_at: at, now: at, size_bytes: 1 }
    }

    fn store_of(files: &[(&str, &str)]) -> Store {
        let mut store = Store::open_in_memory().unwrap();
        for (path, text) in files {
            store.upsert_document(path, &parse_str(text), &times()).unwrap();
        }
        store
    }

    fn one_change(before: &str, after: &str) -> SectionChange {
        let mut review = compare(before, after, Flavor::Markdown);
        assert_eq!(review.changes.len(), 1, "{review:?}");
        review.changes.remove(0)
    }

    fn paths(c: &Candidates) -> Vec<(&str, &str)> {
        c.kept
            .iter()
            .map(|k| (k.section.rel_path.as_str(), k.section.heading_path.last().map_or("", |h| h)))
            .collect()
    }

    const BEFORE: &str = "# Deploy\n\n## Timeout\n\nThe gateway timeout is 30 seconds.\n";
    const AFTER: &str = "# Deploy\n\n## Timeout\n\nThe gateway timeout is 90 seconds.\n";

    #[test]
    fn finds_the_stale_copy_first_and_never_the_changed_section() {
        let store = store_of(&[
            ("deploy.md", AFTER),
            ("faq.md", "# FAQ\n\n## Limits\n\nRequests end after 30 seconds at the gateway.\n"),
            ("net.md", "# Network\n\n## Gateway\n\nThe gateway sits in front of the API.\n"),
            ("pets.md", "# Pets\n\n## Cats\n\nCats sleep most of the day.\n"),
        ]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("deploy.md", &one_change(BEFORE, AFTER)).unwrap();

        assert_eq!(paths(&found), [("faq.md", "Limits"), ("net.md", "Gateway")]);
        assert!(found.kept[0].leading, "it carries the replaced `30 seconds`");
        assert_eq!(found.kept[0].shared_terms, ["30 seconds", "gateway"]);
        assert!(!found.kept[1].leading);
        assert_eq!(found.terms[0].term.text, "30 seconds");
        assert_eq!(found.terms[0].term.role, Role::Replaced);
        assert_eq!((found.retrieved, found.cut_by_caps), (2, 0));
    }

    #[test]
    fn a_section_that_already_reads_like_the_new_text_is_not_a_candidate() {
        let store = store_of(&[
            ("deploy.md", AFTER),
            ("copy.md", "# Copy\n\n## Timeout\n\nThe gateway timeout is 90 seconds.\n"),
        ]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("deploy.md", &one_change(BEFORE, AFTER)).unwrap();
        assert!(found.kept.is_empty(), "{found:?}");
        assert_eq!(found.retrieved, 0);
    }

    #[test]
    fn the_changed_section_is_left_out_even_when_the_index_still_holds_its_old_text() {
        let store = store_of(&[("deploy.md", BEFORE)]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("deploy.md", &one_change(BEFORE, AFTER)).unwrap();
        assert!(found.kept.is_empty(), "{found:?}");
    }

    #[test]
    fn a_term_most_sections_carry_is_dropped_and_said_so() {
        let mut files: Vec<(String, String)> = (0..60)
            .map(|i| (format!("n{i}.md"), format!("# Note {i}\n\nThe gateway is mentioned.\n")))
            .collect();
        files.push(("deploy.md".into(), AFTER.into()));
        files.push(("faq.md".into(), "# FAQ\n\nRequests end after 30 seconds.\n".into()));
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let store = store_of(&refs);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("deploy.md", &one_change(BEFORE, AFTER)).unwrap();

        assert_eq!(found.dropped_common, ["gateway"]);
        assert_eq!(paths(&found), [("faq.md", "FAQ")]);
    }

    #[test]
    fn caps_hold_per_document_and_per_change_and_the_cut_is_counted() {
        let many = (0..5)
            .map(|i| format!("## Part {i}\n\nPart {i} waits 30 seconds for the gateway.\n\n"))
            .collect::<Vec<_>>()
            .concat();
        let big = format!("# Big\n\n{many}");
        let store = store_of(&[
            ("deploy.md", AFTER),
            ("big.md", &big),
            ("b.md", "# B\n\nAlso 30 seconds.\n"),
            ("c.md", "# C\n\nAlso 30 seconds.\n"),
        ]);
        let limits = Limits { per_change: 3, ..Limits::default() };
        let finder = Finder::new(&store, limits).unwrap();
        let found = finder.for_change("deploy.md", &one_change(BEFORE, AFTER)).unwrap();

        assert_eq!(found.kept.len(), 3);
        let of_big = found.kept.iter().filter(|k| k.section.rel_path == "big.md").count();
        assert!(of_big <= 2, "{:?}", paths(&found));
        assert_eq!(found.retrieved, 7);
        assert_eq!(found.cut_by_caps, 4);
    }

    #[test]
    fn a_linked_document_comes_first_in_its_group_and_only_when_it_shares_a_term() {
        let after = "# Deploy\n\nSee [the FAQ](faq.md) and [pets](./pets.md).\n\n## Timeout\n\nThe gateway timeout is 90 seconds.\n";
        let before = after.replace("90 seconds", "30 seconds");
        let store = store_of(&[
            ("deploy.md", after),
            ("a.md", "# A\n\nRetry after 30 seconds. Retry after 30 seconds, 30 seconds.\n"),
            ("faq.md", "# FAQ\n\nRequests end after 30 seconds.\n"),
            ("pets.md", "# Pets\n\nCats sleep most of the day.\n"),
            ("up.md", "# Up\n\nSee [deploy](deploy.md#timeout): it waits 30 seconds.\n"),
        ]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("deploy.md", &one_change(&before, after)).unwrap();

        let got = paths(&found);
        assert_eq!(&got[..2], [("faq.md", "FAQ"), ("up.md", "Up")], "linked both ways, first");
        assert_eq!(got[2], ("a.md", "A"));
        assert!(!got.iter().any(|(p, _)| *p == "pets.md"), "linked, but shares nothing");
        assert_eq!(found.kept[0].source, Source::Linked);
        assert_eq!(found.kept[2].source, Source::Lexical);
    }

    #[test]
    fn file_content_never_becomes_query_syntax() {
        let before =
            "# X\n\n## Y\n\nUse `title:\"a\" OR b*` and NEAR(c d) with ^e AND \"f\" (g) - h.\n";
        let after = "# X\n\n## Y\n\nNothing of the sort.\n";
        let store = store_of(&[
            ("x.md", after),
            ("other.md", "# Other\n\nWe wrote NEAR(c d) once, and title: stays plain.\n"),
        ]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("x.md", &one_change(before, after)).unwrap();
        assert_eq!(paths(&found), [("other.md", "Other")]);
        // `b*` is a literal `b`, not a prefix: no section holds a lone `b`.
        assert!(found.terms.iter().all(|t| !t.term.text.starts_with('b')), "{:?}", found.terms);
    }

    #[test]
    fn the_same_change_gives_the_same_list() {
        let store = store_of(&[
            ("deploy.md", AFTER),
            ("a.md", "# A\n\nWaits 30 seconds.\n"),
            ("b.md", "# B\n\nWaits 30 seconds.\n"),
        ]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let change = one_change(BEFORE, AFTER);
        let first = finder.for_change("deploy.md", &change).unwrap();
        assert_eq!(first, finder.for_change("deploy.md", &change).unwrap());
        // Equal ranks break by section id, which is a digest of the path: fixed, not alphabetical.
        let mut got = paths(&first);
        got.sort_unstable();
        assert_eq!(got, [("a.md", "A"), ("b.md", "B")]);
    }

    #[test]
    fn links_resolve_as_text_and_never_above_the_root() {
        let known: HashSet<&str> =
            ["docs/a.md", "docs/guide/index.md", "b.mdx", "docs/c/README.md"].into();
        let r = |from: &str, to: &str| resolve_link(from, to, &known);
        assert_eq!(r("docs/x.md", "a.md#part").as_deref(), Some("docs/a.md"));
        assert_eq!(r("docs/x.md", "./a.md?v=1").as_deref(), Some("docs/a.md"));
        assert_eq!(r("docs/deep/x.md", "../a.md").as_deref(), Some("docs/a.md"));
        assert_eq!(r("docs/x.md", "/docs/a.md").as_deref(), Some("docs/a.md"));
        assert_eq!(r("docs/x.md", "guide").as_deref(), Some("docs/guide/index.md"));
        assert_eq!(r("docs/x.md", "guide/").as_deref(), Some("docs/guide/index.md"));
        assert_eq!(r("docs/x.md", "../b").as_deref(), Some("b.mdx"));
        assert_eq!(r("docs/x.md", "c").as_deref(), Some("docs/c/README.md"));
        assert_eq!(r("docs/x.md", "#only-an-anchor"), None);
        assert_eq!(r("docs/x.md", "../../a.md"), None, "above the root");
        assert_eq!(r("docs/x.md", "missing.md"), None);
    }
}
