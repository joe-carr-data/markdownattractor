use proptest::prelude::*;

use super::*;

const FIXTURE: &str = include_str!("../../tests/fixtures/runbook.md");

#[test]
fn fixture_snapshot() {
    let doc = parse_str(FIXTURE);
    insta::assert_yaml_snapshot!(doc, {
        ".sections[].text" => "[text]",
    });
}

#[test]
fn fixture_basics() {
    let doc = parse_str(FIXTURE);
    assert_eq!(doc.title.as_deref(), Some("Deploy runbook"));
    assert!(doc.frontmatter.as_deref().unwrap().contains("status: current"));
    assert_eq!(doc.line_count, 43);

    let paths: Vec<Vec<&str>> =
        doc.sections.iter().map(|s| s.heading_path.iter().map(String::as_str).collect()).collect();
    assert_eq!(
        paths,
        vec![
            vec!["Deploy runbook"],
            vec!["Deploy runbook", "Deploy"],
            vec!["Deploy runbook", "Deploy", "Rollback"],
            vec!["Deploy runbook", "Deploy", "Rollback", "Rollback of a database migration"],
            vec!["Deploy runbook", "Contacts"],
        ]
    );

    let rollback = &doc.sections[2];
    assert_eq!(rollback.level, 3);
    assert_eq!((rollback.line_start, rollback.line_end), (19, 28));
    assert_eq!(rollback.code_langs, vec!["bash"]);
    assert!(!rollback.has_tables);

    let contacts = doc.sections.last().unwrap();
    assert!(contacts.has_tables);
    assert_eq!(contacts.line_end, 40, "trailing blank lines are trimmed");

    assert_eq!(doc.links_internal, vec!["../adr/0007-blue-green.md", "#contacts"]);
    assert_eq!(doc.links_external, vec!["https://status.example.com"]);
}

#[test]
fn preamble_without_heading_is_a_section() {
    let doc = parse_str("Just a note.\n\nSecond paragraph.\n");
    assert_eq!(doc.sections.len(), 1);
    let s = &doc.sections[0];
    assert_eq!(s.level, 0);
    assert!(s.heading_path.is_empty());
    assert_eq!((s.line_start, s.line_end), (1, 3));
    assert!(doc.title.is_none());
}

#[test]
fn front_matter_is_excluded_from_sections() {
    let doc = parse_str("---\ntitle: x\n---\n\n# H\n\nbody\n");
    assert_eq!(doc.frontmatter.as_deref(), Some("title: x"));
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].line_start, 5);
}

#[test]
fn front_matter_then_preamble() {
    let doc = parse_str("---\na: 1\n---\nintro line\n\n# H\n");
    assert_eq!(doc.sections[0].level, 0);
    assert_eq!((doc.sections[0].line_start, doc.sections[0].line_end), (4, 4));
}

#[test]
fn crlf_and_trailing_whitespace_do_not_change_hashes() {
    let a = parse_str("# T\n\nhello world\n\n## S\n\ntext\n");
    let b = parse_str("# T   \r\n\r\nhello world  \r\n\r\n## S\r\n\r\ntext\r\n");
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.sections.len(), b.sections.len());
    for (x, y) in a.sections.iter().zip(&b.sections) {
        assert_eq!(x.hash, y.hash);
        assert_eq!((x.line_start, x.line_end), (y.line_start, y.line_end));
    }
}

#[test]
fn inserting_text_above_shifts_lines_but_not_hash() {
    let a = parse_str("# A\n\none\n\n# B\n\ntwo\n");
    let b = parse_str("# A\n\none\nmore\nlines\n\n# B\n\ntwo\n");
    let (sa, sb) = (&a.sections[1], &b.sections[1]);
    assert_eq!(sa.hash, sb.hash);
    assert_ne!(sa.line_start, sb.line_start);
}

#[test]
fn heading_only_section_has_one_line() {
    let doc = parse_str("# A\n# B\n");
    assert_eq!(doc.sections.len(), 2);
    assert_eq!((doc.sections[0].line_start, doc.sections[0].line_end), (1, 1));
    assert_eq!((doc.sections[1].line_start, doc.sections[1].line_end), (2, 2));
}

#[test]
fn heading_path_pops_on_level_decrease() {
    let doc = parse_str("# A\n## B\n### C\n## D\n# E\n");
    let paths: Vec<usize> = doc.sections.iter().map(|s| s.heading_path.len()).collect();
    assert_eq!(paths, vec![1, 2, 3, 2, 1]);
    assert_eq!(doc.sections[3].heading_path, vec!["A", "D"]);
}

#[test]
fn skipped_levels_still_nest() {
    let doc = parse_str("# A\n#### D\n## B\n");
    assert_eq!(doc.sections[1].heading_path, vec!["A", "D"]);
    assert_eq!(doc.sections[2].heading_path, vec!["A", "B"]);
}

#[test]
fn heading_with_inline_code_and_emphasis() {
    let doc = parse_str("## Run `mda start` *now*\n");
    assert_eq!(doc.sections[0].heading_path, vec!["Run mda start now"]);
}

#[test]
fn setext_headings_span_two_lines() {
    let doc = parse_str("Title\n=====\n\nbody\n\nSub\n---\n\nmore\n");
    assert_eq!(doc.title.as_deref(), Some("Title"));
    assert_eq!(doc.sections.len(), 2);
    assert_eq!((doc.sections[0].line_start, doc.sections[0].line_end), (1, 4));
    assert_eq!((doc.sections[1].line_start, doc.sections[1].line_end), (6, 9));
}

#[test]
fn hashes_in_code_fences_are_not_headings() {
    let doc = parse_str("# A\n\n```sh\n# not a heading\necho hi\n```\n");
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].code_langs, vec!["sh"]);
}

#[test]
fn unlabelled_fence_counts_as_text() {
    let doc = parse_str("# A\n\n```\nraw\n```\n");
    assert_eq!(doc.sections[0].code_langs, vec!["text"]);
}

#[test]
fn empty_and_blank_documents() {
    assert!(parse_str("").sections.is_empty());
    assert!(parse_str("\n\n   \n").sections.is_empty());
    assert_eq!(parse_str("").line_count, 0);
}

#[test]
fn token_estimate_is_ceil_chars_over_four() {
    assert_eq!(token_estimate(""), 0);
    assert_eq!(token_estimate("abcd"), 1);
    assert_eq!(token_estimate("abcde"), 2);
}

#[test]
fn scheme_detection() {
    assert!(has_scheme("https://x.y"));
    assert!(has_scheme("mailto:a@b.c"));
    assert!(!has_scheme("docs/a.md"));
    assert!(!has_scheme("#anchor"));
    assert!(!has_scheme("C:\\not\\a\\url"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Sections are ordered, non-overlapping, non-empty, and inside the document.
    #[test]
    fn section_ranges_are_well_formed(text in "[ -~\n#`*]{0,400}") {
        let doc = parse_str(&text);
        let mut prev_end = 0u32;
        for (i, s) in doc.sections.iter().enumerate() {
            prop_assert_eq!(s.index as usize, i);
            prop_assert!(s.line_start >= 1, "line_start must be 1-based");
            prop_assert!(s.line_end >= s.line_start, "{:?}", s);
            prop_assert!(s.line_end <= doc.line_count.max(1));
            prop_assert!(s.line_start > prev_end, "overlap at section {}", i);
            prev_end = s.line_end;
            prop_assert!(!s.text.trim().is_empty());
            prop_assert_eq!(s.hash.len(), 64);
        }
    }

    /// Line endings never influence hashes or ranges.
    #[test]
    fn crlf_invariance(text in "[ -~\n#]{0,300}") {
        let lf = parse_str(&text);
        let crlf = parse_str(&text.replace('\n', "\r\n"));
        prop_assert_eq!(lf.hash, crlf.hash);
        let a: Vec<_> = lf.sections.iter().map(|s| (s.line_start, s.line_end, s.hash.clone())).collect();
        let b: Vec<_> = crlf.sections.iter().map(|s| (s.line_start, s.line_end, s.hash.clone())).collect();
        prop_assert_eq!(a, b);
    }

    /// The section text is exactly the normalised source lines of its range.
    #[test]
    fn section_text_matches_its_lines(text in "[ -~\n#]{0,300}") {
        let doc = parse_str(&text);
        let lines: Vec<&str> = text.split('\n').collect();
        for s in &doc.sections {
            let slice = &lines[(s.line_start - 1) as usize..s.line_end as usize];
            prop_assert_eq!(&s.text, &normalize_block(slice));
        }
    }
}

// ------------------------------------------------------------------ MDX (docs/design/ingestion.md)

const MDX_FIXTURE: &str = include_str!("../../tests/fixtures/page.mdx");

#[test]
fn mdx_fixture_snapshot() {
    let doc = parse_str(MDX_FIXTURE);
    insta::assert_yaml_snapshot!(doc, {
        ".sections[].text" => "[text]",
    });
}

#[test]
fn mdx_fixture_keeps_every_heading_and_drops_the_esm_block() {
    let doc = parse_str(MDX_FIXTURE);
    assert_eq!(doc.title.as_deref(), Some("Deploying with Tabs"), "front matter title");
    let paths: Vec<String> = doc.sections.iter().map(|s| s.heading_path.join(" > ")).collect();
    assert_eq!(
        paths,
        vec!["", "Install with npm", "Install with pnpm", "Rollback", "Rollback > Verify"]
    );
    // The preamble starts after the imports and the export, at the first prose line.
    let preamble = &doc.sections[0];
    assert_eq!(preamble.level, 0);
    assert_eq!(preamble.line_start, 15);
    assert!(preamble.text.starts_with("Pick your package manager"), "{}", preamble.text);
    // A heading right after a JSX tag line (no blank line between) is still a heading.
    let pnpm = &doc.sections[2];
    assert_eq!(pnpm.line_start, 28);
    assert_eq!(pnpm.code_langs, vec!["bash"]);
    let rollback = &doc.sections[3];
    assert_eq!(rollback.line_start, 38);
    assert!(rollback.text.contains("props.window"), "JSX expressions stay as text");
    // A `#` line inside a fence stays inside its fence.
    let verify = doc.sections.last().unwrap();
    assert_eq!(verify.code_langs, vec!["text"]);
    assert_eq!(verify.line_end, 54);
}

#[test]
fn leading_esm_block_is_excluded_like_front_matter() {
    let doc = parse_str(
        "import A from 'a'\nimport B from 'b'\n\nexport const x = {\n  y: 1,\n};\n\nHello.\n\n## S\n\nbody\n",
    );
    assert_eq!(doc.sections.len(), 2);
    assert_eq!((doc.sections[0].line_start, doc.sections[0].line_end), (8, 8));
    assert_eq!(doc.sections[0].text, "Hello.");
    assert_eq!(doc.sections[1].line_start, 10);
}

#[test]
fn esm_block_directly_after_front_matter() {
    let doc = parse_str("---\ntitle: T\n---\nimport X from 'x'\n\n# H\n\nbody\n");
    assert_eq!(doc.sections.len(), 1);
    assert_eq!(doc.sections[0].line_start, 6);
    assert_eq!(doc.title.as_deref(), Some("H"), "an H1 wins over front matter");
}

#[test]
fn esm_block_only_document_has_no_sections() {
    let doc = parse_str("import X from 'x'\n\nexport const title = \"Only exports\";\n");
    assert!(doc.sections.is_empty());
    assert_eq!(doc.title.as_deref(), Some("Only exports"));
}

#[test]
fn import_lines_after_content_stay_text() {
    let doc = parse_str("# H\n\nimport { x } from 'y'\n\ntext\n");
    assert_eq!(doc.sections.len(), 1);
    assert!(doc.sections[0].text.contains("import { x }"));
}

#[test]
fn title_falls_back_to_front_matter_then_export_const() {
    assert_eq!(
        parse_str("---\ntitle: Plain title\n---\n\n## Not an H1\n").title.as_deref(),
        Some("Plain title")
    );
    assert_eq!(
        parse_str("---\ntitle: 'Single: quoted'\n---\n").title.as_deref(),
        Some("Single: quoted")
    );
    assert_eq!(
        parse_str("---\ntitle: \"Double\" # comment\n---\n").title.as_deref(),
        Some("Double")
    );
    assert_eq!(
        parse_str("---\ntitle: bare value # comment\n---\n").title.as_deref(),
        Some("bare value")
    );
    assert_eq!(parse_str("---\ntitle: |\n  block\n---\n").title, None);
    assert_eq!(
        parse_str("---\ntitle:\nseo:\n  title: nested\n---\n").title,
        None,
        "only a column-0 title"
    );
    assert_eq!(
        parse_str(
            "export const title = \"Padding\";\nexport const description = \"d\";\n\n## Examples\n"
        )
        .title
        .as_deref(),
        Some("Padding")
    );
    assert_eq!(parse_str("export const title = 'Single';\n").title.as_deref(), Some("Single"));
    assert_eq!(
        parse_str("---\ntitle: Front\n---\n\nexport const title = \"Export\";\n\n# Heading\n")
            .title
            .as_deref(),
        Some("Heading"),
        "H1, then front matter, then export"
    );
    assert_eq!(
        parse_str("---\ntitle: Front\n---\n\nexport const title = \"Export\";\n\n## Sub\n")
            .title
            .as_deref(),
        Some("Front")
    );
}

#[test]
fn heading_inside_html_block_starts_a_section() {
    let doc =
        parse_str("<Admonition type=\"note\">\ntext\n</Admonition>\n## How it works\n\nbody\n");
    let paths: Vec<Vec<&str>> =
        doc.sections.iter().map(|s| s.heading_path.iter().map(String::as_str).collect()).collect();
    assert_eq!(paths, vec![Vec::<&str>::new(), vec!["How it works"]]);
    assert_eq!((doc.sections[0].line_start, doc.sections[0].line_end), (1, 3));
    assert_eq!((doc.sections[1].line_start, doc.sections[1].line_end), (4, 6));
    // Indented up to three spaces, with inline code and a closing sequence.
    let doc = parse_str(
        "<TabPanel id=\"swift\" label=\"Swift\">\n  ### Deep `link` config ##\n  1. Go there.\n</TabPanel>\n",
    );
    assert_eq!(doc.sections[1].heading_path, vec!["Deep link config"]);
    assert_eq!(doc.sections[1].level, 3);
    // Four spaces is not a heading; neither is a hash inside <pre>, a comment, or a fence.
    let doc = parse_str("<div>\n    ## not\n</div>\n");
    assert_eq!(doc.sections.len(), 1);
    let doc = parse_str("<pre>\n## not\n</pre>\n\n<!--\n## not\n-->\n\n```\n## not\n```\n");
    assert_eq!(doc.sections.len(), 1);
}

#[test]
fn heading_inside_html_block_sets_the_title_when_level_one() {
    let doc = parse_str("<div>\n# Big\n</div>\n");
    assert_eq!(doc.title.as_deref(), Some("Big"));
    assert_eq!(doc.sections.len(), 2);
}

#[test]
fn atx_heading_line_parsing() {
    assert_eq!(atx_heading("## Two"), Some((2, "Two".to_owned())));
    assert_eq!(atx_heading("   ###### Six ###"), Some((6, "Six".to_owned())));
    assert_eq!(atx_heading("#"), Some((1, String::new())));
    assert_eq!(atx_heading("####### seven"), None);
    assert_eq!(atx_heading("#hashtag"), None);
    assert_eq!(atx_heading("    # code"), None);
    assert_eq!(atx_heading("text # not"), None);
}

#[test]
fn export_title_and_front_matter_title_parsing() {
    assert_eq!(export_title("export const title = \"A \\\"quote\\\"\";"), Some("A \\".to_owned()));
    assert_eq!(export_title("export const title = `tpl`;"), None);
    assert_eq!(export_title("export const titles = \"x\";"), None);
    assert_eq!(export_title("export const title = \"\";"), None);
    assert_eq!(front_matter_title("a: 1\ntitle: Hello world\n"), Some("Hello world".to_owned()));
    assert_eq!(front_matter_title("title:   \n"), None);
    assert_eq!(front_matter_title("title: \"\"\n"), None);
    assert_eq!(front_matter_title("title: >\n  folded\n"), None);
    assert_eq!(
        front_matter_title("title = \"TOML style\"\ntopics = [ \"db\" ]\n"),
        Some("TOML style".to_owned())
    );
    assert_eq!(front_matter_title("titles: no\ntitle: yes\n"), Some("yes".to_owned()));
}
