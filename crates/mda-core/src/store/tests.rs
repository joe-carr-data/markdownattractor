use jiff::Timestamp;

use super::*;
use crate::card::{Entities, SCHEMA_VERSION as CARD_SCHEMA, SectionSummary};
use crate::markdown::parse_str;

const DOC_A: &str = "# Alpha\n\nintro text about zebras\n\n## Deploy\n\nrun the deploy script\n\n## Rollback\n\nrevert to the previous release\n";

fn ts(secs: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + secs).unwrap()
}

fn times(secs: i64) -> DocTimes {
    DocTimes { created_at: None, modified_at: ts(secs), now: ts(secs), size_bytes: 123 }
}

fn summary(tldr: &str, body: &str) -> SectionSummary {
    SectionSummary {
        tldr: tldr.into(),
        summary: body.into(),
        keywords: vec!["deploy".into(), "release".into()],
        questions_answered: vec!["how do I deploy?".into()],
        entities: Entities { technologies: vec!["bash".into()], ..Entities::default() },
        mentioned_dates: vec![],
        decisions: vec![],
        action_items: vec![],
    }
}

fn provenance(secs: i64) -> Provenance {
    Provenance {
        model: "haiku".into(),
        prompt_version: "section.v1".into(),
        schema_version: CARD_SCHEMA,
        backend: "mock".into(),
        summarized_at: ts(secs),
        truncated: false,
    }
}

fn usage() -> Usage {
    Usage { input_tokens: 100, output_tokens: 20, cost_usd: 0.001 }
}

fn store_with_doc_a() -> (Store, UpsertOutcome) {
    let mut store = Store::open_in_memory().unwrap();
    let out = store.upsert_document("a.md", &parse_str(DOC_A), &times(0)).unwrap();
    (store, out)
}

#[test]
fn fts5_is_compiled_in() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE t USING fts5(x, tokenize='unicode61 remove_diacritics 2')",
    )
    .expect("bundled SQLite must have FTS5");
}

#[test]
fn migrations_are_idempotent_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("state.db");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    drop(store);
    let again = Store::open(&path).unwrap();
    assert_eq!(again.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(path.exists());
}

#[test]
fn timestamps_are_fixed_width_and_round_trip() {
    let a = Timestamp::from_second(10).unwrap();
    let b = Timestamp::new(10, 5).unwrap();
    let (sa, sb) = (fmt_ts(a), fmt_ts(b));
    assert_eq!(sa.len(), sb.len());
    assert!(sa < sb, "{sa} should sort before {sb}");
    assert_eq!(sb.parse::<Timestamp>().unwrap(), b);
}

#[test]
fn doc_id_is_16_hex_over_forward_slash_path() {
    let id = doc_id_for("docs/plan.md");
    assert_eq!(id.len(), 16);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(id, doc_id_for("docs\\plan.md"));
    assert_ne!(id, doc_id_for("docs/plan2.md"));
}

#[test]
fn upsert_new_doc_reports_created_and_all_hashes_new() {
    let (store, out) = store_with_doc_a();
    let doc = parse_str(DOC_A);
    assert!(out.created);
    assert!(out.changed);
    assert_eq!(out.new_hashes.len(), doc.sections.len());
    assert_eq!(out.reused, 0);
    assert_eq!(out.unchanged, 0);
    assert_eq!(out.removed, 0);

    let counts = store.counts().unwrap();
    assert_eq!(counts.docs, 1);
    assert_eq!(counts.sections, doc.sections.len() as u64);
    assert_eq!(counts.pending, doc.sections.len() as u64);
    assert_eq!(counts.summarized, 0);

    let stored = store.document_by_path("a.md").unwrap().unwrap();
    assert_eq!(stored.doc_id, out.doc_id);
    assert_eq!(stored.title.as_deref(), Some("Alpha"));
    assert_eq!(stored.created_at_source, CreatedAtSource::FirstSeen);
    assert_eq!(stored.created_at, ts(0));
    assert_eq!(stored.size_bytes, 123);
    assert!(stored.deleted_at.is_none());

    let sections = store.sections_of(&out.doc_id).unwrap();
    assert_eq!(sections.len(), doc.sections.len());
    assert_eq!(sections[1].section_id, format!("{}#1", out.doc_id));
    assert_eq!(sections[1].heading_path, vec!["Alpha", "Deploy"]);
    assert_eq!(sections[1].state, SectionState::Pending);
    assert_eq!(sections[1].created_at, ts(0));
    assert!(store.section(&sections[2].section_id).unwrap().is_some());
    assert!(store.section("nope#0").unwrap().is_none());

    let events = store.timeline(None, None, 10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::DocCreated);
}

#[test]
fn birthtime_becomes_created_at_and_is_never_overwritten() {
    let mut store = Store::open_in_memory().unwrap();
    let t = DocTimes { created_at: Some(ts(-500)), ..times(0) };
    store.upsert_document("a.md", &parse_str(DOC_A), &t).unwrap();
    let t2 = DocTimes { created_at: Some(ts(-1)), ..times(5) };
    store.upsert_document("a.md", &parse_str(DOC_A), &t2).unwrap();
    let doc = store.document_by_path("a.md").unwrap().unwrap();
    assert_eq!(doc.created_at, ts(-500));
    assert_eq!(doc.created_at_source, CreatedAtSource::Birthtime);
    assert_eq!(doc.first_seen_at, ts(0));
    assert_eq!(doc.updated_at, ts(5));
}

#[test]
fn upsert_same_doc_again_is_unchanged() {
    let (mut store, first) = store_with_doc_a();
    let out = store.upsert_document("a.md", &parse_str(DOC_A), &times(1)).unwrap();
    assert!(!out.created);
    assert!(!out.changed);
    assert!(out.new_hashes.is_empty());
    assert_eq!(out.unchanged, first.new_hashes.len());
    assert_eq!(out.reused, 0);
    assert_eq!(out.removed, 0);
    let events = store.timeline(None, None, 10).unwrap();
    assert_eq!(events.len(), 1, "no DocChanged for identical content");
    assert_eq!(store.counts().unwrap().pending, first.new_hashes.len() as u64);
}

#[test]
fn editing_one_section_yields_one_new_hash_and_refreshes_line_ranges() {
    let (mut store, first) = store_with_doc_a();
    let before = store.sections_of(&first.doc_id).unwrap();
    let rollback_before =
        before.iter().find(|s| s.heading_path.last().unwrap() == "Rollback").unwrap();

    // Insert lines above and edit the Deploy section only.
    let edited = DOC_A
        .replace("intro text about zebras", "intro text about zebras\n\nmore intro\nand more")
        .replace("run the deploy script", "run the NEW deploy script");
    let out = store.upsert_document("a.md", &parse_str(&edited), &times(10)).unwrap();
    assert!(!out.created);
    assert!(out.changed);
    assert_eq!(out.new_hashes.len(), 2, "preamble-with-title and Deploy changed");
    assert_eq!(out.unchanged, 1);
    assert_eq!(out.removed, 2);

    let after = store.sections_of(&first.doc_id).unwrap();
    let rollback_after =
        after.iter().find(|s| s.heading_path.last().unwrap() == "Rollback").unwrap();
    assert_eq!(rollback_after.section_hash, rollback_before.section_hash);
    assert_eq!(rollback_after.line_start, rollback_before.line_start + 3);
    assert_eq!(rollback_after.line_end, rollback_before.line_end + 3);
    assert_eq!(rollback_after.updated_at, ts(0), "unchanged content keeps its updated_at");
    let deploy_after = after.iter().find(|s| s.heading_path.last().unwrap() == "Deploy").unwrap();
    assert_eq!(deploy_after.updated_at, ts(10));

    let kinds: Vec<_> =
        store.timeline(None, None, 10).unwrap().into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec![EventKind::DocCreated, EventKind::DocChanged]);
}

#[test]
fn attach_summary_makes_cards_searchable_and_updates_counts() {
    let (mut store, out) = store_with_doc_a();
    let deploy = &store.sections_of(&out.doc_id).unwrap()[1];
    let hash = deploy.section_hash.clone();
    assert!(store.search_cards(&fts_escape("kubernetes"), 10).unwrap().is_empty());

    let n = store
        .attach_summary(&hash, &summary("ships to kubernetes", "body"), &provenance(20), &usage())
        .unwrap();
    assert_eq!(n, 1);

    let hits = store.search_cards(&fts_escape("kubernetes"), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].section_id, deploy.section_id);
    assert!(hits[0].bm25 > 0.0, "score is positive, higher is better");

    let counts = store.counts().unwrap();
    assert_eq!(counts.summarized, 1);
    assert_eq!(counts.pending, 2);
    assert_eq!(counts.total_input_tokens, 100);
    assert_eq!(counts.total_output_tokens, 20);
    assert!((counts.total_cost_usd - 0.001).abs() < 1e-9);

    let stored = store.section(&deploy.section_id).unwrap().unwrap();
    assert_eq!(stored.state, SectionState::Summarized);
    assert_eq!(stored.summary.as_ref().map(|s| s.tldr.as_str()), Some("ships to kubernetes"));
    assert_eq!(stored.provenance.as_ref().map(|p| p.model.as_str()), Some("haiku"));
    let doc = store.document(&out.doc_id).unwrap().unwrap();
    assert_eq!(doc.last_summarized_at, Some(ts(20)));

    let events = store.timeline(None, None, 10).unwrap();
    let last = events.last().unwrap();
    assert_eq!(last.kind, EventKind::SectionSummarized);
    assert_eq!(last.section_id.as_deref(), Some(deploy.section_id.as_str()));

    // Re-upserting an unchanged doc keeps the card searchable.
    store.upsert_document("a.md", &parse_str(DOC_A), &times(30)).unwrap();
    assert_eq!(store.search_cards(&fts_escape("kubernetes"), 10).unwrap().len(), 1);
}

#[test]
fn attach_summary_for_unknown_hash_is_not_found() {
    let mut store = Store::open_in_memory().unwrap();
    let err =
        store.attach_summary("nope", &summary("t", "s"), &provenance(1), &usage()).unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err}");
    assert!(matches!(store.mark_failed("nope", "x").unwrap_err(), Error::NotFound(_)));
}

#[test]
fn same_text_in_two_docs_shares_one_summary() {
    let (mut store, a) = store_with_doc_a();
    let doc_b = "# Beta\n\n## Deploy\n\nrun the deploy script\n";
    let b = store.upsert_document("b.md", &parse_str(doc_b), &times(1)).unwrap();
    assert_eq!(b.new_hashes.len(), 1, "only the Beta preamble is new");
    assert_eq!(b.reused, 1);
    assert_eq!(b.unchanged, 0);

    let a_deploy = store.sections_of(&a.doc_id).unwrap()[1].clone();
    let b_deploy = store.sections_of(&b.doc_id).unwrap()[1].clone();
    assert_eq!(a_deploy.section_hash, b_deploy.section_hash);
    assert_eq!(b_deploy.created_at, ts(0), "created_at is when the hash was first seen anywhere");

    let rows: i64 = store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM summaries WHERE section_hash = ?1",
            [&a_deploy.section_hash],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);

    let pending = store.pending_hashes(100).unwrap();
    assert_eq!(pending.iter().filter(|p| p.section_hash == a_deploy.section_hash).count(), 1);

    let n = store
        .attach_summary(
            &a_deploy.section_hash,
            &summary("ships it", "body"),
            &provenance(5),
            &usage(),
        )
        .unwrap();
    assert_eq!(n, 2);
    let mut ids: Vec<_> = store
        .search_cards(&fts_escape("ships"), 10)
        .unwrap()
        .into_iter()
        .map(|h| h.section_id)
        .collect();
    ids.sort();
    let mut expected = vec![a_deploy.section_id.clone(), b_deploy.section_id.clone()];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(store.sections_by_hash(&a_deploy.section_hash).unwrap().len(), 2);
}

#[test]
fn pending_hashes_are_smallest_first_with_representative_section() {
    let (store, out) = store_with_doc_a();
    let pending = store.pending_hashes(10).unwrap();
    assert_eq!(pending.len(), 3);
    assert!(pending.windows(2).all(|w| w[0].token_estimate <= w[1].token_estimate));
    for p in &pending {
        assert_eq!(p.rel_path, "a.md");
        assert!(p.section_id.starts_with(&out.doc_id));
        assert!(!p.text.is_empty());
    }
    assert_eq!(store.pending_hashes(1).unwrap().len(), 1);
}

#[test]
fn mark_failed_then_retry_failed() {
    let (mut store, out) = store_with_doc_a();
    let hash = out.new_hashes[0].clone();
    store.mark_failed(&hash, "timeout").unwrap();
    assert_eq!(store.pending_hashes(10).unwrap().len(), 2);
    let counts = store.counts().unwrap();
    assert_eq!(counts.failed, 1);
    assert_eq!(counts.pending, 2);
    let section = store.sections_by_hash(&hash).unwrap().remove(0);
    assert_eq!(section.state, SectionState::Failed);
    assert_eq!(section.fail_reason.as_deref(), Some("timeout"));
    let last = store.timeline(None, None, 10).unwrap().pop().unwrap();
    assert_eq!(last.kind, EventKind::SectionFailed);
    assert_eq!(last.detail.as_deref(), Some("timeout"));

    assert_eq!(store.retry_failed().unwrap(), 1);
    assert_eq!(store.retry_failed().unwrap(), 0);
    assert_eq!(store.pending_hashes(10).unwrap().len(), 3);
    assert!(store.sections_by_hash(&hash).unwrap()[0].fail_reason.is_none());
}

#[test]
fn tombstone_removes_sections_and_fts_rows_but_keeps_summaries() {
    let (mut store, out) = store_with_doc_a();
    let hash = store.sections_of(&out.doc_id).unwrap()[1].section_hash.clone();
    store.attach_summary(&hash, &summary("ships it", "body"), &provenance(5), &usage()).unwrap();
    assert_eq!(store.search_raw(&fts_escape("zebras"), 10).unwrap().len(), 1);

    assert!(store.tombstone("a.md", ts(50)).unwrap());
    assert!(!store.tombstone("a.md", ts(51)).unwrap(), "already tombstoned");
    assert!(!store.tombstone("missing.md", ts(51)).unwrap());

    assert!(store.search_raw(&fts_escape("zebras"), 10).unwrap().is_empty());
    assert!(store.search_cards(&fts_escape("ships"), 10).unwrap().is_empty());
    assert!(store.sections_of(&out.doc_id).unwrap().is_empty());
    assert!(store.documents().unwrap().is_empty());
    let doc = store.document(&out.doc_id).unwrap().unwrap();
    assert_eq!(doc.deleted_at, Some(ts(50)));
    let counts = store.counts().unwrap();
    assert_eq!(counts.tombstoned, 1);
    assert_eq!(counts.docs, 0);
    assert_eq!(counts.sections, 0);
    assert_eq!(counts.summarized, 0, "no live section carries the hash");
    assert_eq!(counts.total_input_tokens, 100, "spend is never forgotten");
    let summaries: i64 =
        store.conn.query_row("SELECT COUNT(*) FROM summaries", [], |r| r.get(0)).unwrap();
    assert_eq!(summaries, 3);
    assert_eq!(store.timeline(None, None, 10).unwrap().pop().unwrap().kind, EventKind::DocDeleted);

    // Resurrection reuses the summary and reports created.
    let back = store.upsert_document("a.md", &parse_str(DOC_A), &times(60)).unwrap();
    assert!(back.created);
    assert!(back.new_hashes.is_empty());
    assert_eq!(back.reused, 3);
    assert_eq!(store.search_cards(&fts_escape("ships"), 10).unwrap().len(), 1);
    assert!(store.document(&out.doc_id).unwrap().unwrap().deleted_at.is_none());
    assert_eq!(store.document(&out.doc_id).unwrap().unwrap().first_seen_at, ts(0));
}

#[test]
fn fts_escape_neutralises_syntax() {
    assert_eq!(fts_escape("hello world"), r#""hello" "world""#);
    assert_eq!(fts_escape(r#"say "hi""#), r#""say" """hi""""#);
    assert_eq!(fts_escape("foo*"), r#""foo"*"#);
    assert_eq!(fts_escape("fo*o"), r#""fo*o""#);
    assert_eq!(fts_escape("a OR b"), r#""a" "OR" "b""#);
    assert_eq!(fts_escape("NEAR(a b)"), r#""NEAR(a" "b)""#);
    assert_eq!(fts_escape("title:foo"), r#""title:foo""#);
    assert_eq!(fts_escape("  \t\n "), "");
    assert_eq!(fts_escape("*"), "");
    assert_eq!(fts_escape("-x ^y"), r#""-x" "^y""#);

    // Every one of these must be a valid MATCH expression against a real table.
    let (store, _) = store_with_doc_a();
    for q in [r#"say "hi""#, "a OR b", "NEAR(a b)", "title:foo", "-x ^y", "(", "\"", "zeb*", "AND"]
    {
        let expr = fts_escape(q);
        let res = store.search_raw(&expr, 5);
        assert!(res.is_ok(), "{q:?} -> {expr:?}: {:?}", res.err());
    }
    assert!(store.search_raw("", 5).unwrap().is_empty());
    assert_eq!(store.search_raw(&fts_escape("zeb*"), 5).unwrap().len(), 1, "prefix match works");
    assert!(store.search_raw(&fts_escape("zeb"), 5).unwrap().is_empty(), "no implicit prefix");
}

#[test]
fn search_raw_finds_a_term_only_in_raw_text() {
    let (mut store, out) = store_with_doc_a();
    let sections = store.sections_of(&out.doc_id).unwrap();
    let hits = store.search_raw(&fts_escape("previous release"), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].section_id, sections[2].section_id);
    assert!(store.search_cards(&fts_escape("previous release"), 10).unwrap().is_empty());
    // Heading path is indexed and weighted.
    let hits = store.search_raw(&fts_escape("rollback"), 10).unwrap();
    assert_eq!(hits[0].section_id, sections[2].section_id);
    // Diacritics are folded.
    let doc = parse_str("# Résumé\n\ncafé au lait\n");
    store.upsert_document("r.md", &doc, &times(1)).unwrap();
    assert_eq!(store.search_raw(&fts_escape("cafe"), 10).unwrap().len(), 1);
    assert_eq!(store.search_raw(&fts_escape("resume"), 10).unwrap().len(), 1);
    assert!(store.search_raw(&fts_escape("nothing-here"), 10).unwrap().is_empty());
}

#[test]
fn search_cards_ranks_tldr_hit_above_summary_hit() {
    let (mut store, out) = store_with_doc_a();
    let sections = store.sections_of(&out.doc_id).unwrap();
    let in_summary = summary("about something else", "explains the canary rollout in detail");
    let in_tldr = summary("canary rollout explained", "a longer body without the key term");
    store.attach_summary(&sections[1].section_hash, &in_summary, &provenance(1), &usage()).unwrap();
    store.attach_summary(&sections[2].section_hash, &in_tldr, &provenance(2), &usage()).unwrap();
    let hits = store.search_cards(&fts_escape("canary"), 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].section_id, sections[2].section_id, "tldr weight 3.0 wins");
    assert!(hits[0].bm25 > hits[1].bm25);
    assert_eq!(store.search_cards(&fts_escape("canary"), 1).unwrap().len(), 1);
}

#[test]
fn timeline_filters_by_since_and_until() {
    let mut store = Store::open_in_memory().unwrap();
    for (i, at) in [ts(10), ts(20), ts(30)].into_iter().enumerate() {
        store
            .record_event(&Event {
                at,
                kind: EventKind::DocChanged,
                doc_id: format!("d{i}"),
                section_id: None,
                detail: Some(format!("e{i}")),
            })
            .unwrap();
    }
    let all = store.timeline(None, None, 10).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].at, ts(10));
    assert_eq!(all[2].detail.as_deref(), Some("e2"));
    let since = store.timeline(Some(ts(20)), None, 10).unwrap();
    assert_eq!(since.iter().map(|e| e.at).collect::<Vec<_>>(), vec![ts(20), ts(30)]);
    let until = store.timeline(None, Some(ts(20)), 10).unwrap();
    assert_eq!(until.iter().map(|e| e.at).collect::<Vec<_>>(), vec![ts(10)]);
    let window = store.timeline(Some(ts(15)), Some(ts(30)), 10).unwrap();
    assert_eq!(window.iter().map(|e| e.at).collect::<Vec<_>>(), vec![ts(20)]);
    assert_eq!(store.timeline(None, None, 2).unwrap().len(), 2);
}

#[test]
fn counts_sum_usage_across_summaries() {
    let (mut store, out) = store_with_doc_a();
    let sections = store.sections_of(&out.doc_id).unwrap();
    let u1 = Usage { input_tokens: 10, output_tokens: 1, cost_usd: 0.5 };
    let u2 = Usage { input_tokens: 20, output_tokens: 2, cost_usd: 0.25 };
    store
        .attach_summary(&sections[0].section_hash, &summary("a", "b"), &provenance(1), &u1)
        .unwrap();
    store
        .attach_summary(&sections[1].section_hash, &summary("c", "d"), &provenance(2), &u2)
        .unwrap();
    store.mark_failed(&sections[2].section_hash, "boom").unwrap();
    let c = store.counts().unwrap();
    assert_eq!(c.docs, 1);
    assert_eq!(c.sections, 3);
    assert_eq!(c.summarized, 2);
    assert_eq!(c.pending, 0);
    assert_eq!(c.failed, 1);
    assert_eq!(c.tombstoned, 0);
    assert_eq!(c.total_input_tokens, 30);
    assert_eq!(c.total_output_tokens, 3);
    assert!((c.total_cost_usd - 0.75).abs() < 1e-9);
    assert!(store.section(&sections[1].section_id).unwrap().unwrap().summary.is_some());
}

#[test]
fn documents_lists_live_docs_by_path() {
    let mut store = Store::open_in_memory().unwrap();
    store.upsert_document("z.md", &parse_str("# Z\n"), &times(0)).unwrap();
    store.upsert_document("a/b.md", &parse_str("# B\n"), &times(0)).unwrap();
    store.upsert_document("m.md", &parse_str("# M\n"), &times(0)).unwrap();
    store.tombstone("m.md", ts(1)).unwrap();
    let paths: Vec<_> = store.documents().unwrap().into_iter().map(|d| d.rel_path).collect();
    assert_eq!(paths, vec!["a/b.md", "z.md"]);
}

#[test]
fn document_hash_reports_live_documents_only() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(store.document_hash("a.md").unwrap(), None);
    let doc = parse_str(DOC_A);
    store.upsert_document("a.md", &doc, &times(0)).unwrap();
    assert_eq!(store.document_hash("a.md").unwrap().as_deref(), Some(doc.hash.as_str()));
    store.tombstone("a.md", ts(5)).unwrap();
    assert_eq!(store.document_hash("a.md").unwrap(), None);
}

#[test]
fn note_rename_moves_history_and_tombstones_the_old_path() {
    let mut store = Store::open_in_memory().unwrap();
    let doc = parse_str(DOC_A);
    let birth = DocTimes { created_at: Some(ts(-500)), ..times(0) };
    store.upsert_document("old.md", &doc, &birth).unwrap();
    store
        .attach_summary(
            &doc.sections[1].hash,
            &summary("t", "b"),
            &provenance(1),
            &Usage::default(),
        )
        .unwrap();

    // The file shows up at its new path first (the watcher indexes what exists)...
    let out = store.upsert_document("new/path.md", &doc, &times(10)).unwrap();
    assert!(out.created);
    assert!(out.new_hashes.is_empty(), "cards are keyed by hash: nothing to summarize");
    // ...then the intake notices the old one is gone with the same content.
    assert!(store.note_rename("old.md", "new/path.md", ts(11)).unwrap());

    let old = store.document_by_path("old.md").unwrap().unwrap();
    assert_eq!(old.deleted_at, Some(ts(11)));
    assert!(store.sections_of(&old.doc_id).unwrap().is_empty());
    let new = store.document_by_path("new/path.md").unwrap().unwrap();
    assert_eq!(new.created_at, ts(-500));
    assert_eq!(new.created_at_source, CreatedAtSource::Birthtime);
    assert_eq!(new.first_seen_at, ts(0));
    assert_eq!(new.deleted_at, None);
    assert_eq!(store.sections_of(&new.doc_id).unwrap().len(), 3);
    assert_eq!(store.counts().unwrap().summarized, 1, "the card followed the content");

    let events = store.timeline(None, None, 100).unwrap();
    let renamed: Vec<_> = events.iter().filter(|e| e.kind == EventKind::DocRenamed).collect();
    assert_eq!(renamed.len(), 1);
    assert_eq!(renamed[0].doc_id, new.doc_id);
    assert_eq!(renamed[0].detail.as_deref(), Some("old.md"));
    assert!(!events.iter().any(|e| e.kind == EventKind::DocDeleted), "a rename is not a delete");

    // Idempotent and safe when a side is missing.
    assert!(!store.note_rename("old.md", "new/path.md", ts(12)).unwrap());
    assert!(!store.note_rename("ghost.md", "new/path.md", ts(12)).unwrap());
    assert!(!store.note_rename("new/path.md", "nowhere.md", ts(12)).unwrap());
    assert!(!store.note_rename("x.md", "x.md", ts(12)).unwrap());
}

#[test]
fn embeddings_round_trip_live_filter_and_counts() {
    let mut store = Store::open_in_memory().unwrap();
    let doc = parse_str(DOC_A);
    store.upsert_document("a.md", &doc, &times(0)).unwrap();
    let h1 = doc.sections[1].hash.clone();
    let h2 = doc.sections[2].hash.clone();
    store.attach_summary(&h1, &summary("t1", "b1"), &provenance(1), &Usage::default()).unwrap();
    assert!(store.vector_set("m").unwrap().is_empty());

    // One card, no vector yet: it is what needs embedding.
    let todo = store.cards_without_embedding("m", None, 10).unwrap();
    assert_eq!(todo.len(), 1);
    assert_eq!(todo[0].section_hash, h1);
    assert_eq!(todo[0].title.as_deref(), Some("Alpha"));
    assert_eq!(todo[0].heading_path, vec!["Alpha", "Deploy"]);
    assert_eq!(todo[0].summary.tldr, "t1");

    store.put_embedding(&h1, "m", &[0.6, 0.8]).unwrap();
    let set = store.vector_set("m").unwrap();
    assert_eq!(set.len(), 1);
    assert_eq!(set.dim, 2);
    assert_eq!(set.hashes, vec![h1.clone()]);
    assert!((set.row(0)[0] - 0.6).abs() < 1e-6 && (set.row(0)[1] - 0.8).abs() < 1e-6);
    assert!(store.cards_without_embedding("m", None, 10).unwrap().is_empty());
    assert!(store.vector_set("other-model").unwrap().is_empty(), "model filter");

    // Replace is fine; a second card shows up as pending work; counts add up.
    store.put_embedding(&h1, "m", &[1.0, 0.0]).unwrap();
    assert_eq!(store.vector_set("m").unwrap().row(0), &[1.0, 0.0]);
    store.attach_summary(&h2, &summary("t2", "b2"), &provenance(2), &Usage::default()).unwrap();
    assert_eq!(store.cards_without_embedding("m", None, 10).unwrap()[0].section_hash, h2);
    assert!(store.cards_without_embedding("m", Some(&h2), 10).unwrap().is_empty(), "cursor");
    let c = store.embedding_counts("m").unwrap();
    assert_eq!((c.embedded, c.carded), (1, 2));

    // Vectors of other models can be dropped; tombstoned documents hide their vectors.
    store.put_embedding(&h2, "old", &[0.0, 1.0]).unwrap();
    assert_eq!(store.delete_embeddings_not("m").unwrap(), 1);
    let ids = store.section_ids_by_hashes(&[h1.as_str(), "nope"]).unwrap();
    assert_eq!(ids[&h1].len(), 1);
    assert!(ids["nope"].is_empty());
    store.tombstone("a.md", ts(9)).unwrap();
    assert!(store.vector_set("m").unwrap().is_empty());
    assert_eq!(store.embedding_counts("m").unwrap(), EmbeddingCounts::default());
}

#[test]
fn recent_documents_and_documents_with_open_sections() {
    let mut store = Store::open_in_memory().unwrap();
    let doc = parse_str(DOC_A);
    store.upsert_document("old.md", &doc, &times(0)).unwrap();
    store.upsert_document("new.md", &parse_str("# New\n\nfresh\n"), &times(100)).unwrap();
    let recent = store.recent_documents(5).unwrap();
    assert_eq!(
        recent.iter().map(|d| d.rel_path.as_str()).collect::<Vec<_>>(),
        vec!["new.md", "old.md"]
    );
    assert_eq!(store.recent_documents(1).unwrap().len(), 1);

    let open = store.documents_with_open_sections().unwrap();
    assert_eq!(open.len(), 2);
    let old = open.iter().find(|(d, _, _)| d.rel_path == "old.md").unwrap();
    assert_eq!((old.1, old.2), (3, 0));
    store.mark_failed(&doc.sections[0].hash, "boom").unwrap();
    for s in &doc.sections[1..] {
        store
            .attach_summary(&s.hash, &summary("t", "b"), &provenance(1), &Usage::default())
            .unwrap();
    }
    let open = store.documents_with_open_sections().unwrap();
    let old = open.iter().find(|(d, _, _)| d.rel_path == "old.md").unwrap();
    assert_eq!((old.1, old.2), (0, 1));
    store
        .attach_summary(
            &parse_str("# New\n\nfresh\n").sections[0].hash,
            &summary("t", "b"),
            &provenance(1),
            &Usage::default(),
        )
        .unwrap();
    assert_eq!(
        store.documents_with_open_sections().unwrap().len(),
        1,
        "fully carded docs drop out"
    );
}
