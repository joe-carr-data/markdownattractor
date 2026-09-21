//! The output contract: what a model produces for a section, and the card we store.
//!
//! Two layers, kept apart on purpose:
//!
//! - [`SectionSummary`] is **exactly** what the model fills in. Its derived JSON Schema is what
//!   `claude -p --json-schema` receives, so the Rust type is the single source of truth for the
//!   prompt contract. Change a field here and the schema, the validator and the store all follow.
//! - [`SectionCard`] is the summary plus everything we know deterministically: identity, line
//!   ranges, hashes, timestamps, provenance. Nothing in this layer ever comes from a model.
//!
//! The schema handed to the model is versioned by [`SCHEMA_VERSION`]; bump it whenever
//! [`SectionSummary`] changes shape, so cards produced under the old contract can be re-summarised.

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Version of the [`SectionSummary`] contract. Stamped into every card's [`Provenance`].
pub const SCHEMA_VERSION: u16 = 1;

/// How precisely a date in the text pins down a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DatePrecision {
    /// A full calendar day, e.g. `2026-09-21`.
    Day,
    /// A month, e.g. `Sept 2026`.
    Month,
    /// A quarter, e.g. `Q4 2026`.
    Quarter,
    /// A year, or a vaguer span inside one year (`spring 2026`).
    Year,
    /// Relative to the document's own time (`last week`, `next sprint`). Resolved later.
    Relative,
}

/// A date the model found in the section, with the evidence that proves it was there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MentionedDate {
    /// The date exactly as written in the text.
    pub raw: String,
    /// ISO 8601, as precise as the text allows: `2026`, `2026-09`, or `2026-09-21`.
    pub iso: String,
    /// How precise `iso` is.
    pub precision: DatePrecision,
    /// A verbatim substring of the section, 5 to 15 words, containing the date.
    pub evidence: String,
}

/// Named things that literally appear in the section. Never inferred.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entities {
    /// People, as written.
    pub people: Vec<String>,
    /// Organisations and teams.
    pub orgs: Vec<String>,
    /// Products, services, libraries referred to by name.
    pub products: Vec<String>,
    /// Technologies, protocols, formats, algorithms.
    pub technologies: Vec<String>,
    /// File paths as written.
    pub files_paths: Vec<String>,
    /// Complete shell commands as written.
    pub commands: Vec<String>,
}

/// What the model writes about one section. See the module docs for why this is separate
/// from [`SectionCard`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionSummary {
    /// One sentence, max 25 words. Does not repeat the heading.
    pub tldr: String,
    /// Max 3 sentences, max 80 words.
    pub summary: String,
    /// 4 to 8 lowercase keywords or keyphrases.
    pub keywords: Vec<String>,
    /// 2 to 4 questions a developer would type into a search box that this section answers.
    pub questions_answered: Vec<String>,
    /// Named things that literally appear in the text.
    pub entities: Entities,
    /// Dates found in the text, each with verbatim evidence.
    pub mentioned_dates: Vec<MentionedDate>,
    /// Explicit decisions stated in the section, max 4, one short sentence each.
    pub decisions: Vec<String>,
    /// Explicit TODOs or future steps, max 3. Empty if none.
    pub action_items: Vec<String>,
}

impl SectionSummary {
    /// The JSON Schema handed to `claude -p --json-schema`.
    ///
    /// Generated from the type, so it cannot drift from what the validator accepts.
    pub fn json_schema() -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(Self))
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
        // The model does not need to know the schema dialect, and some validators choke on it.
        if let serde_json::Value::Object(map) = &mut schema {
            map.remove("$schema");
        }
        simplify_enums(&mut schema);
        schema
    }
}

/// Rewrite schemars' `oneOf: [{const, description}, …]` encoding of a unit enum into the plain
/// `{"type": "string", "enum": […]}` that every structured-output engine accepts (the Claude
/// API rejects `oneOf`; llama.cpp grammars prefer `enum`). Variant descriptions are folded
/// into the parent description so the model still sees them.
fn simplify_enums(v: &mut serde_json::Value) {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            let as_enum = map.get("oneOf").and_then(Value::as_array).and_then(|items| {
                let mut values = Vec::new();
                let mut notes = Vec::new();
                for item in items {
                    let obj = item.as_object()?;
                    let c = obj.get("const")?.as_str()?;
                    values.push(Value::String(c.to_owned()));
                    if let Some(d) = obj.get("description").and_then(Value::as_str) {
                        notes.push(format!("`{c}`: {d}"));
                    }
                }
                Some((values, notes))
            });
            if let Some((values, notes)) = as_enum {
                map.remove("oneOf");
                map.insert("type".into(), Value::String("string".into()));
                map.insert("enum".into(), Value::Array(values));
                if !notes.is_empty() {
                    let mut desc = map
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_default();
                    if !desc.is_empty() {
                        desc.push(' ');
                    }
                    desc.push_str(&notes.join(" "));
                    map.insert("description".into(), Value::String(desc));
                }
            }
            for child in map.values_mut() {
                simplify_enums(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(simplify_enums),
        _ => {}
    }
}

/// Where a card came from. Lets `mda stale` and re-summarisation reason about old cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Model id that produced the summary, e.g. `claude-haiku-4-5-20251001`.
    pub model: String,
    /// Version of the system prompt file, e.g. `section.v1`.
    pub prompt_version: String,
    /// [`SCHEMA_VERSION`] at the time.
    pub schema_version: u16,
    /// Backend that ran the call: `claude-cli` or `api`.
    pub backend: String,
    /// When the summary was produced.
    pub summarized_at: Timestamp,
    /// `true` when the model saw only a prefix of the section (planner truncation), so the
    /// card describes the beginning of the section, not all of it.
    #[serde(default)]
    pub truncated: bool,
}

/// A section card: the model's summary plus deterministic metadata.
///
/// Rendered at about 80–150 tokens. This is what search returns at level L1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionCard {
    /// Stable id: `<doc_id>#<section index>`.
    pub section_id: String,
    /// Owning document.
    pub doc_id: String,
    /// Headings from the document root down to this section, e.g. `["Deploy", "Rollback"]`.
    pub heading_path: Vec<String>,
    /// First line of the section in the source file, 1-based, inclusive.
    pub line_start: u32,
    /// Last line of the section in the source file, 1-based, inclusive.
    pub line_end: u32,
    /// Rough token count of the source section (chars / 4).
    pub token_estimate: u32,
    /// blake3 of the normalised section text. Unchanged hash ⇒ no re-summarisation.
    pub section_hash: String,
    /// Fenced code block languages present, deduplicated, in order of first appearance.
    pub has_code: Vec<String>,
    /// Whether the section contains a table.
    pub has_tables: bool,
    /// When this section's content first appeared in the index.
    pub section_created_at: Timestamp,
    /// When this section's content last changed.
    pub section_updated_at: Timestamp,
    /// The model's part.
    #[serde(flatten)]
    pub summary: SectionSummary,
    /// How and when the summary was produced.
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_object_with_required_fields() {
        let s = SectionSummary::json_schema();
        assert_eq!(s["type"], "object");
        let required: Vec<&str> =
            s["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        for f in
            ["tldr", "summary", "keywords", "questions_answered", "entities", "mentioned_dates"]
        {
            assert!(required.contains(&f), "missing required field {f}");
        }
        assert!(s.get("$schema").is_none());
        assert_eq!(s["additionalProperties"], false);
    }

    #[test]
    fn schema_has_no_one_of_and_encodes_enums_plainly() {
        let s = SectionSummary::json_schema();
        assert!(!s.to_string().contains("oneOf"), "structured-output APIs reject oneOf");
        let p = &s["$defs"]["DatePrecision"];
        assert_eq!(p["type"], "string");
        assert_eq!(p["enum"], serde_json::json!(["day", "month", "quarter", "year", "relative"]));
        assert!(p["description"].as_str().unwrap().contains("`day`:"), "variant docs folded in");
    }

    #[test]
    fn schema_snapshot() {
        insta::assert_json_snapshot!(SectionSummary::json_schema());
    }

    #[test]
    fn summary_rejects_unknown_fields() {
        let json = r#"{"tldr":"x","summary":"y","keywords":[],"questions_answered":[],
            "entities":{"people":[],"orgs":[],"products":[],"technologies":[],"files_paths":[],"commands":[]},
            "mentioned_dates":[],"decisions":[],"action_items":[],"bogus":1}"#;
        assert!(serde_json::from_str::<SectionSummary>(json).is_err());
    }

    #[test]
    fn card_round_trips_with_flattened_summary() {
        let now = Timestamp::UNIX_EPOCH;
        let card = SectionCard {
            section_id: "abc#0".into(),
            doc_id: "abc".into(),
            heading_path: vec!["Deploy".into(), "Rollback".into()],
            line_start: 10,
            line_end: 42,
            token_estimate: 300,
            section_hash: "deadbeef".into(),
            has_code: vec!["bash".into()],
            has_tables: false,
            section_created_at: now,
            section_updated_at: now,
            summary: SectionSummary {
                tldr: "t".into(),
                summary: "s".into(),
                keywords: vec!["k".into()],
                questions_answered: vec!["q?".into()],
                entities: Entities::default(),
                mentioned_dates: vec![],
                decisions: vec![],
                action_items: vec![],
            },
            provenance: Provenance {
                model: "haiku".into(),
                prompt_version: "section.v1".into(),
                schema_version: SCHEMA_VERSION,
                backend: "claude-cli".into(),
                summarized_at: now,
                truncated: false,
            },
        };
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["tldr"], "t", "summary fields are flattened to the top level");
        let back: SectionCard = serde_json::from_value(json).unwrap();
        assert_eq!(back, card);
    }
}
