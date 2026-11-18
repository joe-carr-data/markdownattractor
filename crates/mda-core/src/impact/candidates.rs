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
//!   [`Limits::per_change`] in all. Ranked hits are read page by page until the places are
//!   filled or the hits run out, so one document with many matching sections cannot hide the
//!   others; a search stopped by [`Limits::max_scanned`] is reported as truncated.
//! - **Freshness is the caller's.** The index must hold the working files (the review syncs it
//!   first): the changed section is recognised by its document and heading path or by its
//!   new text, which an index older than the reviewed revision may not show.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Serialize;

use super::changes::SectionChange;
use super::terms::{Role, Term, TermKind, terms_for};
use crate::Result;
use crate::store::{RawHit, Store, StoredSection, doc_id_for, fts_literal};

/// The caps of the candidate step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limits {
    /// Terms in the query.
    pub max_terms: usize,
    /// Terms of a change whose frequency is always looked up before the best are chosen.
    pub max_terms_examined: usize,
    /// Past [`Limits::max_terms_examined`], lookups go on while fewer than
    /// [`Limits::max_terms`] usable terms were found, up to this many in all.
    pub max_term_lookups: usize,
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
    /// Hits read from the index per page.
    pub pool: usize,
    /// Hits read from one ranked search at most.
    pub max_scanned: usize,
    /// Linked documents searched for one change (in path order).
    pub max_linked_docs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_terms: 12,
            max_terms_examined: 48,
            max_term_lookups: 400,
            max_term_bytes: 120,
            common_fraction: 0.20,
            common_floor: 10,
            per_document: 2,
            per_change: 8,
            pool: 50,
            max_scanned: 400,
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
    /// Sections of the index that carry at least one query term (the changed section too,
    /// when it does).
    pub matching: u64,
    /// Sections looked at, the changed section and exact copies of its new text left out.
    pub examined: usize,
    /// Of those, how many the per-document cap passed over.
    pub cut_per_document: usize,
    /// A ranked search was stopped by [`Limits::max_scanned`] with places still free: sections
    /// that share a term were not looked at.
    pub scan_truncated: bool,
    /// Terms of the change whose frequency was never looked up
    /// ([`Limits::max_term_lookups`]).
    pub terms_unexamined: usize,
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
        let docs = store.document_links()?;
        let known: HashSet<&str> = docs.iter().map(|(path, _)| path.as_str()).collect();
        let mut neighbours: HashMap<String, BTreeSet<String>> = HashMap::new();
        for (path, links) in &docs {
            for target in links {
                let Some(other) = resolve_link(path, target, &known) else { continue };
                if &other == path {
                    continue;
                }
                neighbours.entry(path.clone()).or_default().insert(other.clone());
                neighbours.entry(other).or_default().insert(path.clone());
            }
        }
        let total_sections = store.live_section_count()?;
        Ok(Self { store, limits, total_sections, neighbours })
    }

    /// The candidates for `change`, a change of the document at `rel_path`. The index must
    /// hold the working files (see the module docs).
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
        out.matching = self.store.raw_match_count(&all_expr)?;

        let linked = self.linked_doc_ids(rel_path, &mut out);
        let mut picked = Picked {
            own_doc: doc_id_for(rel_path),
            change,
            seen: HashSet::new(),
            per_doc: HashMap::new(),
            hits: Vec::new(),
        };
        // Leading group first; in each group the linked documents' sections, then the rest.
        self.scan(
            &leading_expr,
            Some(&linked),
            None,
            (Source::Linked, true),
            &mut picked,
            &mut out,
        )?;
        self.scan(&leading_expr, None, None, (Source::Lexical, true), &mut picked, &mut out)?;
        if all_expr != leading_expr {
            let skip = Some(leading_expr.as_str());
            self.scan(
                &all_expr,
                Some(&linked),
                skip,
                (Source::Linked, false),
                &mut picked,
                &mut out,
            )?;
            self.scan(&all_expr, None, skip, (Source::Lexical, false), &mut picked, &mut out)?;
        }
        // Only the winners' text is loaded.
        for (hit, source, leading) in picked.hits {
            if let Some(section) = self.store.section(&hit.section_id)? {
                out.kept.push(Candidate { section, source, leading, shared_terms: Vec::new() });
            }
        }
        self.fill_shared_terms(&chosen, &mut out)?;
        out.terms = chosen.into_iter().map(|(t, _)| t).collect();
        Ok(out)
    }

    /// Read one ranked search page by page, taking sections until the places are filled, the
    /// hits run out or [`Limits::max_scanned`] is reached. Hits that match `skip` belong to an
    /// earlier group and are passed over.
    fn scan(
        &self,
        expr: &str,
        only_docs: Option<&[String]>,
        skip: Option<&str>,
        (source, leading): (Source, bool),
        picked: &mut Picked<'_>,
        out: &mut Candidates,
    ) -> Result<()> {
        let mut offset = 0;
        while picked.hits.len() < self.limits.per_change {
            if offset >= self.limits.max_scanned {
                out.scan_truncated = true;
                break;
            }
            let page = self.store.raw_hits(expr, only_docs, self.limits.pool, offset)?;
            let full = page.len() == self.limits.pool;
            offset += page.len();
            let earlier: HashSet<String> = match skip {
                Some(skip) => {
                    let ids: Vec<String> = page.iter().map(|h| h.section_id.clone()).collect();
                    self.store.raw_matches_among(skip, &ids)?.into_iter().collect()
                }
                None => HashSet::new(),
            };
            for hit in page {
                if picked.hits.len() >= self.limits.per_change {
                    break;
                }
                if earlier.contains(&hit.section_id) || !picked.seen.insert(hit.section_id.clone())
                {
                    continue;
                }
                // The changed section itself, and any section that already reads as it does
                // now, cannot be out of date with it.
                if hit.section_hash == picked.change.after.hash
                    || (hit.doc_id == picked.own_doc
                        && hit.heading_path == picked.change.heading_path)
                {
                    continue;
                }
                out.examined += 1;
                let of_doc = picked.per_doc.entry(hit.doc_id.clone()).or_default();
                if *of_doc >= self.limits.per_document {
                    out.cut_per_document += 1;
                    continue;
                }
                *of_doc += 1;
                picked.hits.push((hit, source, leading));
            }
            if !full {
                break;
            }
        }
        Ok(())
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
        let usable: Vec<(Term, String)> = terms_for(change)
            .into_iter()
            .filter(|t| t.text.len() <= self.limits.max_term_bytes)
            .filter_map(|t| fts_literal(&t.text).map(|lit| (t, lit)))
            .collect();
        let total = usable.len();
        for (order, (term, literal)) in usable.into_iter().enumerate() {
            // Terms come best first. The first ones are always looked up; after that only
            // while the query is short of terms, so that a change whose first wording is found
            // nowhere still reaches the wording that is.
            let enough = scored.len() >= self.limits.max_terms;
            if order >= self.limits.max_term_lookups
                || (order >= self.limits.max_terms_examined && enough)
            {
                out.terms_unexamined = total - order;
                break;
            }
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

/// The indexed document a link in `from` points at, if any. Text only, and the answer is
/// always an indexed path.
///
/// The fragment and the query are cut; a target with an authority (`//host/…`) is not local;
/// a leading `/` means the indexed root (a site mounted elsewhere is not known here: such a
/// link is then simply not found, or found at the same path under the root); path segments
/// are percent-decoded; `.` and `..` are resolved and a target above the root is nothing.
/// Case and backslashes are taken as written. A target that names a folder (it ends with
/// `/`, or is the root) is tried as that folder's page first; a target whose last segment has
/// no extension is tried as a page (`x.md`, `x.mdx`, then the folder's page); a target with
/// an extension is only ever itself.
fn resolve_link(from: &str, target: &str, known: &HashSet<&str>) -> Option<String> {
    let target = target.split(['#', '?']).next().unwrap_or_default().trim();
    if target.is_empty() || target.starts_with("//") {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if !target.starts_with('/') {
        parts.extend(from.split('/').map(str::to_owned));
        parts.pop(); // the file name
    }
    for piece in target.split('/') {
        let piece = percent_decode(piece)?;
        match piece.as_str() {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name if name.contains(['/', '\0']) => return None,
            _ => parts.push(piece),
        }
    }
    let path = parts.join("/");
    let folder = target.ends_with('/') || path.is_empty();
    let has_extension = parts.last().is_some_and(|last| last.contains('.'));
    let join =
        |name: &str| if path.is_empty() { name.to_owned() } else { format!("{path}/{name}") };
    let folder_pages = [join("index.md"), join("README.md"), join("index.mdx")];
    let file_pages = if path.is_empty() || has_extension {
        Vec::new()
    } else {
        vec![format!("{path}.md"), format!("{path}.mdx")]
    };
    let mut tries: Vec<String> = Vec::new();
    if folder {
        tries.extend(folder_pages);
        tries.extend(file_pages);
    } else {
        tries.push(path.clone());
        tries.extend(file_pages);
        if !has_extension {
            tries.extend(folder_pages);
        }
    }
    tries.into_iter().find(|p| known.contains(p.as_str()))
}

/// `%41` → `A`. `None` when the result is not UTF-8. A `%` not followed by two hex digits is
/// kept as written.
fn percent_decode(piece: &str) -> Option<String> {
    if !piece.contains('%') {
        return Some(piece.to_owned());
    }
    let bytes = piece.as_bytes();
    let hex = |b: u8| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok());
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) =
                (bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex))
        {
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What a review of one change has taken so far.
struct Picked<'c> {
    own_doc: String,
    change: &'c SectionChange,
    seen: HashSet<String>,
    per_doc: HashMap<String, usize>,
    hits: Vec<(RawHit, Source, bool)>,
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
        assert_eq!((found.matching, found.examined, found.cut_per_document), (3, 2, 0));
        assert!(!found.scan_truncated);
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
        assert_eq!(found.examined, 0);
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
    fn caps_hold_per_document_and_per_change_and_the_passed_over_are_counted() {
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
        // Room for five: b, c and two parts of big; its other three parts are passed over.
        let roomy = Limits { per_change: 5, ..Limits::default() };
        let found = Finder::new(&store, roomy)
            .unwrap()
            .for_change("deploy.md", &one_change(BEFORE, AFTER))
            .unwrap();
        assert_eq!(found.kept.len(), 4, "{:?}", paths(&found));
        let of_big = found.kept.iter().filter(|k| k.section.rel_path == "big.md").count();
        assert_eq!(of_big, 2, "{:?}", paths(&found));
        assert_eq!((found.examined, found.cut_per_document), (7, 3));

        // Room for three: the search stops as soon as they are taken.
        let tight = Limits { per_change: 3, ..Limits::default() };
        let found = Finder::new(&store, tight)
            .unwrap()
            .for_change("deploy.md", &one_change(BEFORE, AFTER))
            .unwrap();
        assert_eq!(found.kept.len(), 3);
        assert_eq!(found.matching, 8, "five parts, b, c and the changed section");
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

    #[test]
    fn one_document_with_many_matching_sections_cannot_hide_another() {
        // Sixty short sections of one manual carry the replaced word and outrank the FAQ's
        // longer one; a single page of fifty hits would never reach the FAQ.
        let parts = (0..60).map(|i| format!("## P{i}\n\nlegacy_mode {i}\n\n")).collect::<Vec<_>>();
        let manual = format!("# Manual\n\n{}", parts.concat());
        let filler = (0..300)
            .map(|i| (format!("f{i}.md"), format!("# Filler {i}\n\nNothing here {i}.\n")))
            .collect::<Vec<_>>();
        let faq = "# FAQ\n\nA long answer that goes on about many other things, settings, \
                   options and current_mode, before it mentions legacy_mode at the very end.\n";
        let before = "# Config\n\n## Mode\n\nSet legacy_mode to run.\n";
        let after = "# Config\n\n## Mode\n\nSet current_mode to run.\n";
        let mut files: Vec<(&str, &str)> =
            vec![("config.md", after), ("manual.md", &manual), ("faq.md", faq)];
        files.extend(filler.iter().map(|(p, t)| (p.as_str(), t.as_str())));
        let store = store_of(&files);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("config.md", &one_change(before, after)).unwrap();

        let got = paths(&found);
        assert_eq!(got.iter().filter(|(p, _)| *p == "manual.md").count(), 2, "{got:?}");
        let faq_hit = found.kept.iter().find(|k| k.section.rel_path == "faq.md");
        let faq_hit = faq_hit.unwrap_or_else(|| panic!("the FAQ was never reached: {got:?}"));
        assert!(faq_hit.leading, "it carries the replaced word, whatever else it carries");
        assert!(faq_hit.shared_terms.contains(&"legacy_mode".to_owned()));
        assert!(found.cut_per_document >= 58, "{found:?}");
        assert!(!found.scan_truncated);
    }

    #[test]
    fn a_search_stopped_by_the_scan_limit_says_so() {
        let parts = (0..30).map(|i| format!("## P{i}\n\nlegacy_mode {i}\n\n")).collect::<Vec<_>>();
        let manual = format!("# Manual\n\n{}", parts.concat());
        let before = "# Config\n\n## Mode\n\nSet legacy_mode to run.\n";
        let after = "# Config\n\n## Mode\n\nSet current_mode to run.\n";
        let store = store_of(&[("config.md", after), ("manual.md", &manual)]);
        let limits = Limits { pool: 5, max_scanned: 10, common_floor: 100, ..Limits::default() };
        let finder = Finder::new(&store, limits).unwrap();
        let found = finder.for_change("config.md", &one_change(before, after)).unwrap();
        assert_eq!(found.kept.len(), 2);
        assert!(found.scan_truncated);
    }

    #[test]
    fn wording_found_nowhere_does_not_use_up_the_term_lookups() {
        let gone = (0..60).map(|i| format!("`old_{i:02}`")).collect::<Vec<_>>().join(" ");
        let before = format!("# X\n\n## Flags\n\n{gone} `retired_flag`\n");
        let after = "# X\n\n## Flags\n\n`current_flag`\n";
        let store = store_of(&[("x.md", after), ("faq.md", "# FAQ\n\nPass retired_flag.\n")]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("x.md", &one_change(&before, after)).unwrap();
        assert_eq!(paths(&found), [("faq.md", "FAQ")]);
        assert_eq!(found.terms_unexamined, 0);

        let tight = Limits { max_term_lookups: 10, ..Limits::default() };
        let found = Finder::new(&store, tight)
            .unwrap()
            .for_change("x.md", &one_change(&before, after))
            .unwrap();
        assert!(found.kept.is_empty());
        assert!(found.terms_unexamined > 0, "and it says what it did not look up");
    }

    #[test]
    fn a_code_span_of_symbols_is_a_safe_term_that_matches_nothing() {
        let before = "# X\n\n## Y\n\nUse `---` and `\u{e000}` here.\n";
        let after = "# X\n\n## Y\n\nUse nothing here.\n";
        let store =
            store_of(&[("x.md", after), ("o.md", "# O\n\nA \u{e000} mark and --- rule.\n")]);
        let finder = Finder::new(&store, Limits::default()).unwrap();
        let found = finder.for_change("x.md", &one_change(before, after)).unwrap();
        // The private-use character is a token to the index; the dashes are not.
        assert_eq!(paths(&found), [("o.md", "O")]);
        assert_eq!(found.kept[0].shared_terms, ["\u{e000}"]);
    }

    #[test]
    fn links_keep_folder_intent_decode_percent_and_never_guess_past_an_extension() {
        let known: HashSet<&str> = [
            "guide.md",
            "guide/index.md",
            "index.md",
            "Install Guide.md",
            "manual.pdf.md",
            "example.org/guide.md",
            "sub/README.md",
        ]
        .into();
        let r = |from: &str, to: &str| resolve_link(from, to, &known);
        assert_eq!(r("a.md", "guide/").as_deref(), Some("guide/index.md"));
        assert_eq!(r("a.md", "guide").as_deref(), Some("guide.md"));
        assert_eq!(r("sub/a.md", "/").as_deref(), Some("index.md"));
        assert_eq!(r("sub/a.md", "..").as_deref(), Some("index.md"));
        assert_eq!(r("sub/a.md", ".").as_deref(), Some("sub/README.md"));
        assert_eq!(r("a.md", "Install%20Guide.md#setup").as_deref(), Some("Install Guide.md"));
        assert_eq!(r("a.md", "//example.org/guide"), None, "a host, not a folder");
        assert_eq!(r("a.md", "manual.pdf"), None, "an extension is never added to");
        assert_eq!(r("a.md", "%2e%2e/guide.md"), None, "above the root, however spelled");
        assert_eq!(r("a.md", "sub%2Fa.md"), None, "an encoded separator is not a path");
        assert_eq!(r("a.md", "GUIDE.md"), None, "case as written");
    }
}
