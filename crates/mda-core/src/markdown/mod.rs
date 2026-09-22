//! Markdown parsing into heading-delimited sections.
//!
//! This is the first stage of the pipeline and the only one that looks at raw markdown. It is
//! pure: given text, it returns a [`Document`]. It never touches the filesystem except through
//! [`parse_file`], which only reads.
//!
//! ## Section model
//!
//! A section starts at a heading and runs to the line before the next heading of *any* level.
//! Hierarchy is carried by [`Section::heading_path`], not by nesting, so every section has one
//! contiguous line range that `mda_open` can hand straight to `Read(path, offset, limit)`.
//! Text before the first heading becomes a preamble section with an empty heading path, if it
//! has any non-blank content.
//!
//! ## MDX and templated markdown
//!
//! `.mdx` files (Docusaurus, Nextra, Mintlify, the Tailwind and Prisma docs) are parsed as
//! [`Flavor::Mdx`], with two deterministic additions that `docs/design/ingestion.md` spells
//! out: a leading block of `import`/`export` statements is excluded from sections like front
//! matter is, and the title falls back to `export const title = "…"`. For both flavours an
//! ATX heading line inside a raw HTML block (which `CommonMark` would swallow because the tag
//! and the heading are not separated by a blank line) still starts a section, outside any
//! fence, comment or `<pre>`-like element nested in that block, and the title falls back to
//! front matter `title:`. JSX tags, expressions and template tags (Liquid, MDX comments) stay
//! as text: the section text is always the source lines of its range.
//!
//! ## Normalisation
//!
//! Hashes are computed over text with `\r\n` folded to `\n` and trailing whitespace stripped
//! from every line. Editor noise (line endings, trailing spaces) therefore never triggers a
//! re-summarisation. Line numbers always refer to the file as it is on disk.

use std::path::Path;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// One heading-delimited slice of a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// 0-based position in the document. Stable only until the document changes.
    pub index: u32,
    /// Heading level (1–6). `0` for the preamble before the first heading.
    pub level: u8,
    /// Headings from the top of the document down to and including this one.
    pub heading_path: Vec<String>,
    /// First line, 1-based, inclusive. For headed sections this is the heading line.
    pub line_start: u32,
    /// Last non-blank line, 1-based, inclusive.
    pub line_end: u32,
    /// Normalised section text (see module docs), including the heading line.
    pub text: String,
    /// blake3 hex digest of `text`.
    pub hash: String,
    /// Rough token count: `ceil(chars / 4)`.
    pub token_estimate: u32,
    /// Languages of fenced code blocks, deduplicated, in order of appearance. Unlabelled
    /// fences contribute `"text"`.
    pub code_langs: Vec<String>,
    /// Whether the section contains a GFM table.
    pub has_tables: bool,
}

/// A parsed document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Text of the first level-1 heading; else front matter `title:`; else the MDX
    /// `export const title = "…"` of the leading ESM block. `None` when the file has none.
    pub title: Option<String>,
    /// Raw YAML front matter (without the `---` fences), if present.
    pub frontmatter: Option<String>,
    /// Sections in document order.
    pub sections: Vec<Section>,
    /// Link targets without a scheme (relative paths, anchors).
    pub links_internal: Vec<String>,
    /// Link targets with a scheme (`https://…`, `mailto:…`).
    pub links_external: Vec<String>,
    /// Number of lines in the file as read.
    pub line_count: u32,
    /// Sum of section token estimates.
    pub token_estimate: u32,
    /// blake3 hex digest of the whole normalised text.
    pub hash: String,
}

/// Which dialect a file is parsed as. Chosen from the extension by [`Flavor::of_path`]; a
/// document must be parsed with the same flavour every time or its hashes change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flavor {
    /// Plain markdown (`.md`, `.markdown`).
    #[default]
    Markdown,
    /// MDX (`.mdx`): a leading ESM block is excluded and `export const title` names the page.
    Mdx,
}

impl Flavor {
    /// `Mdx` for a `.mdx` extension (any case), `Markdown` otherwise.
    #[must_use]
    pub fn of_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("mdx") => Self::Mdx,
            _ => Self::Markdown,
        }
    }
}

/// Read and parse a file with the flavour its extension implies. The only I/O in this module.
pub fn parse_file(path: &Path) -> Result<Document> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    let text = String::from_utf8(bytes).map_err(|e| Error::parse(path, e.to_string()))?;
    Ok(parse_str_as(&text, Flavor::of_path(path)))
}

/// Parse plain markdown text. Never fails: any byte sequence that is valid UTF-8 is a document.
#[must_use]
pub fn parse_str(text: &str) -> Document {
    parse_str_as(text, Flavor::Markdown)
}

/// Parse text as the given flavour. Never fails.
#[must_use]
pub fn parse_str_as(text: &str, flavor: Flavor) -> Document {
    let text = normalize_newlines(text);
    let lines: Vec<&str> = text.split('\n').collect();
    // `split` yields a trailing empty element when the text ends with '\n'; that is not a line.
    let line_count = if text.ends_with('\n') || text.is_empty() {
        lines.len().saturating_sub(1)
    } else {
        lines.len()
    };

    let arena = Arena::new();
    let root = parse_document(&arena, &text, &options());

    // Front matter ends where comrak says; the leading ESM block, if any, follows it.
    #[allow(clippy::cast_possible_truncation)]
    let after_front_matter = root
        .first_child()
        .filter(|n| matches!(n.data.borrow().value, NodeValue::FrontMatter(_)))
        .map_or(1, |n| n.data.borrow().sourcepos.end.line as u32 + 1);
    let esm = match flavor {
        Flavor::Mdx => leading_esm_block(&lines, after_front_matter),
        Flavor::Markdown => LeadingEsm { body_start: after_front_matter, title: None },
    };

    let mut builder = Builder::new(&lines, line_count, esm.body_start, esm.title);
    for node in root.children() {
        builder.visit_top_level(node);
    }
    builder.finish()
}

/// The MDX ESM block at the top of a document, if any: one or more `import` / `export`
/// statements (see [`is_esm_statement`]), each running to the next blank line (the MDX
/// rule), starting at `after_front_matter`, possibly after blank lines.
struct LeadingEsm {
    /// First line after the block (or `after_front_matter` when there is none), 1-based.
    body_start: u32,
    /// The value of `export const title = "…"` inside the block, when present.
    title: Option<String>,
}

#[allow(clippy::cast_possible_truncation)]
fn leading_esm_block(lines: &[&str], after_front_matter: u32) -> LeadingEsm {
    let mut i = (after_front_matter.max(1) - 1) as usize;
    let mut body_start = after_front_matter;
    let mut title = None;
    loop {
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        let Some(line) = lines.get(i) else { break };
        if !is_esm_statement(line) {
            break;
        }
        while i < lines.len() && !lines[i].trim().is_empty() {
            if title.is_none() {
                title = export_title(lines[i]);
            }
            i += 1;
        }
        body_start = u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1);
    }
    LeadingEsm { body_start, title }
}

/// Whether a line at column 0 opens an ESM statement rather than prose that happens to
/// start with the word: `import` needs a binding form (`{`, `*`, a module string, or a
/// name followed by ` from `); `export` needs a declaration keyword, `default`, `{` or `*`.
/// "import duties apply." and "export it now" are prose.
fn is_esm_statement(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("import ") {
        let rest = rest.trim_start();
        return rest.starts_with(['{', '*', '"', '\''])
            || rest.strip_prefix("type ").is_some_and(|r| r.trim_start().starts_with('{'))
            || rest.contains(" from ");
    }
    if let Some(rest) = line.strip_prefix("export ") {
        let rest = rest.trim_start();
        return rest.starts_with(['{', '*'])
            || [
                "const ",
                "let ",
                "var ",
                "function ",
                "async ",
                "class ",
                "default ",
                "type ",
                "interface ",
            ]
            .iter()
            .any(|kw| rest.starts_with(kw));
    }
    false
}

/// `export const title = "Padding";` → `Padding` (double or single quotes, JavaScript
/// backslash escapes honoured). An unterminated or empty string gives `None`.
fn export_title(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export const title")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let value = quoted_string(rest, Escapes::Backslash)?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The `title:` of a YAML front matter block at column 0 (or `title = "…"` when the block
/// is TOML between the same `---` fences), unquoted (YAML `''` inside single quotes and
/// backslash escapes inside double quotes honoured). Block scalars (`|`, `>`), unterminated
/// strings and empty values give `None`.
fn front_matter_title(front_matter: &str) -> Option<String> {
    let value = front_matter
        .lines()
        .find_map(|l| {
            let rest = l.strip_prefix("title")?.trim_start();
            rest.strip_prefix(':').or_else(|| rest.strip_prefix('='))
        })?
        .trim();
    let value = match value.chars().next()? {
        '|' | '>' => return None,
        '"' => quoted_string(value, Escapes::Backslash)?,
        '\'' => quoted_string(value, Escapes::Doubled)?,
        _ => value.split(" #").next().unwrap_or(value).to_owned(),
    };
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// How a quote character is escaped inside a quoted string.
#[derive(Clone, Copy)]
enum Escapes {
    /// `\"` (JavaScript, YAML double quotes).
    Backslash,
    /// `''` (YAML single quotes).
    Doubled,
}

/// The contents of the quoted string `text` starts with, or `None` when it does not start
/// with a quote or is never closed.
fn quoted_string(text: &str, escapes: Escapes) -> Option<String> {
    let mut chars = text.chars().peekable();
    let quote = chars.next().filter(|c| *c == '"' || *c == '\'')?;
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match (c, escapes) {
            ('\\', Escapes::Backslash) => out.push(chars.next()?),
            (c, Escapes::Doubled) if c == quote && chars.peek() == Some(&quote) => {
                chars.next();
                out.push(quote);
            }
            (c, _) if c == quote => return Some(out),
            (c, _) => out.push(c),
        }
    }
    None
}

/// `## Text` → `(2, "Text")` for an ATX heading line (up to three spaces of indent, one to
/// six `#`, then a space or the end of the line). The line is parsed by comrak as a document
/// of its own, so the closing-sequence rule (`# C#` keeps its hash, `# Title ##` drops the
/// trailing ones) and inline markup are handled exactly as for a heading outside a block.
fn atx_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }
    let arena = Arena::new();
    let root = parse_document(&arena, trimmed, &options());
    let node = root.first_child()?;
    let level = match &node.data.borrow().value {
        NodeValue::Heading(h) => h.level,
        _ => return None,
    };
    Some((level, inline_text(node)))
}

/// Raw-content state while scanning the lines of an HTML block for headings: inside a code
/// fence, an HTML comment, or a `<pre>`, `<script>`, `<style>` or `<textarea>` element, a
/// `#` line is content, not a heading.
#[derive(Default)]
struct RawScan {
    /// Open fence: its character and length; closed by a line of at least that many of the
    /// same character and nothing else.
    fence: Option<(char, usize)>,
    /// Inside `<!-- … -->`.
    comment: bool,
    /// The raw-text element whose closing tag ends the skip, lower-case (`pre`, …).
    raw_element: Option<&'static str>,
}

impl RawScan {
    /// Feed one line; `true` when the line is content that cannot be a heading.
    fn skip(&mut self, line: &str) -> bool {
        let t = line.trim();
        if let Some((ch, len)) = self.fence {
            if t.chars().all(|c| c == ch) && t.chars().count() >= len {
                self.fence = None;
            }
            return true;
        }
        if self.comment {
            if t.contains("-->") {
                self.comment = false;
            }
            return true;
        }
        if let Some(name) = self.raw_element {
            if closes_element(t, name) {
                self.raw_element = None;
            }
            return true;
        }
        let fence_len = t.chars().take_while(|c| *c == '`').count();
        let tilde_len = t.chars().take_while(|c| *c == '~').count();
        if fence_len >= 3 {
            self.fence = Some(('`', fence_len));
            return true;
        }
        if tilde_len >= 3 {
            self.fence = Some(('~', tilde_len));
            return true;
        }
        if t.starts_with("<!--") {
            self.comment = !t.contains("-->");
            return true;
        }
        for name in ["pre", "script", "style", "textarea"] {
            if opens_element(t, name) {
                self.raw_element = (!closes_element(t, name)).then_some(name);
                return true;
            }
        }
        false
    }
}

fn opens_element(t: &str, name: &str) -> bool {
    t.get(1..).is_some_and(|r| {
        t.starts_with('<')
            && r.len() >= name.len()
            && r.is_char_boundary(name.len())
            && r[..name.len()].eq_ignore_ascii_case(name)
            && r[name.len()..].starts_with([' ', '>', '\t', '/'])
    })
}

fn closes_element(t: &str, name: &str) -> bool {
    t.to_ascii_lowercase().contains(&format!("</{name}"))
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.front_matter_delimiter = Some("---".to_owned());
    o.extension.table = true;
    o.extension.strikethrough = true;
    o.extension.tasklist = true;
    o.extension.autolink = false;
    o.extension.footnotes = true;
    o.parse.smart = false;
    o
}

fn normalize_newlines(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_owned()
    }
}

/// Section text with trailing whitespace stripped per line. Hash input.
fn normalize_block(lines: &[&str]) -> String {
    let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out
}

fn blake3_hex(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

/// Rough token count used everywhere in the crate: `ceil(chars / 4)`.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn token_estimate(text: &str) -> u32 {
    text.chars().count().div_ceil(4) as u32
}

/// Accumulates sections while walking the top-level AST nodes in order.
struct Builder<'a> {
    lines: &'a [&'a str],
    line_count: u32,
    /// Heading stack: (level, text).
    stack: Vec<(u8, String)>,
    /// Section currently being built.
    current: Option<Open>,
    sections: Vec<Section>,
    /// First level-1 heading.
    h1: Option<String>,
    /// `export const title` from the leading ESM block.
    export_title: Option<String>,
    frontmatter: Option<String>,
    links_internal: Vec<String>,
    links_external: Vec<String>,
    /// First content line after front matter and the leading ESM block, 1-based.
    body_start: u32,
}

/// A section whose end is not yet known.
struct Open {
    level: u8,
    heading_path: Vec<String>,
    line_start: u32,
    code_langs: Vec<String>,
    has_tables: bool,
}

impl<'a> Builder<'a> {
    #[allow(clippy::cast_possible_truncation)]
    fn new(
        lines: &'a [&'a str],
        line_count: usize,
        body_start: u32,
        export_title: Option<String>,
    ) -> Self {
        Self {
            lines,
            line_count: line_count as u32,
            stack: Vec::new(),
            current: None,
            sections: Vec::new(),
            h1: None,
            export_title,
            frontmatter: None,
            links_internal: Vec::new(),
            links_external: Vec::new(),
            body_start,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn visit_top_level(&mut self, node: &'a AstNode<'a>) {
        enum Kind {
            FrontMatter(String),
            Heading(u8),
            /// A raw HTML or JSX block of `CommonMark` type 6 or 7: a heading line inside it
            /// is still a heading (types 1–5 are script/pre/style, comments and the like).
            HtmlBlock,
            Other,
        }
        let (start_line, end_line, kind) = {
            let data = node.data.borrow();
            let kind = match &data.value {
                NodeValue::FrontMatter(raw) => Kind::FrontMatter(raw.clone()),
                NodeValue::Heading(h) => Kind::Heading(h.level),
                NodeValue::HtmlBlock(b) if b.block_type >= 6 => Kind::HtmlBlock,
                _ => Kind::Other,
            };
            (data.sourcepos.start.line as u32, data.sourcepos.end.line as u32, kind)
        };

        let html_block = matches!(kind, Kind::HtmlBlock);
        match kind {
            Kind::FrontMatter(raw) => {
                self.frontmatter = Some(strip_front_matter_fences(&raw));
                self.body_start = self.body_start.max(end_line + 1);
                return;
            }
            Kind::Heading(level) => {
                // A heading inside the excluded ESM block (a template literal) is not one.
                if start_line >= self.body_start {
                    self.open_heading(level, inline_text(node), start_line);
                }
                return;
            }
            Kind::HtmlBlock | Kind::Other => {}
        }
        // The leading ESM block is excluded like front matter.
        if end_line < self.body_start {
            return;
        }

        // Any other top-level node belongs to the current section, or opens the preamble.
        if self.current.is_none() {
            self.current = Some(Open {
                level: 0,
                heading_path: Vec::new(),
                line_start: self.body_start.max(start_line),
                code_langs: Vec::new(),
                has_tables: false,
            });
        }
        if html_block {
            let mut raw = RawScan::default();
            for line in start_line.max(self.body_start)..=end_line {
                let text = self.line_at(line);
                if raw.skip(text) {
                    continue;
                }
                if let Some((level, heading)) = atx_heading(text) {
                    self.open_heading(level, heading, line);
                }
            }
        }
        self.collect_features(node);
    }

    /// Close the open section and start one at `start_line` for a heading.
    fn open_heading(&mut self, level: u8, text: String, start_line: u32) {
        self.close_current(start_line.saturating_sub(1));
        while self.stack.last().is_some_and(|(l, _)| *l >= level) {
            self.stack.pop();
        }
        self.stack.push((level, text.clone()));
        if level == 1 && self.h1.is_none() {
            self.h1 = Some(text);
        }
        self.current = Some(Open {
            level,
            heading_path: self.stack.iter().map(|(_, t)| t.clone()).collect(),
            line_start: start_line,
            code_langs: Vec::new(),
            has_tables: false,
        });
    }

    /// Record code fences, tables and links found anywhere under `node`.
    fn collect_features(&mut self, node: &'a AstNode<'a>) {
        for n in node.descendants() {
            let d = n.data.borrow();
            match &d.value {
                NodeValue::CodeBlock(cb) if cb.fenced => {
                    let lang = cb.info.split_whitespace().next().unwrap_or("text").to_owned();
                    if let Some(cur) = &mut self.current
                        && !cur.code_langs.contains(&lang)
                    {
                        cur.code_langs.push(lang);
                    }
                }
                NodeValue::Table(_) => {
                    if let Some(cur) = &mut self.current {
                        cur.has_tables = true;
                    }
                }
                NodeValue::Link(link) => {
                    let url = link.url.clone();
                    if has_scheme(&url) {
                        self.links_external.push(url);
                    } else {
                        self.links_internal.push(url);
                    }
                }
                _ => {}
            }
        }
    }

    /// Close the open section so that it ends at `end_line` (inclusive), trimming trailing
    /// blank lines. Sections that would be empty are dropped.
    #[allow(clippy::cast_possible_truncation)]
    fn close_current(&mut self, end_line: u32) {
        let Some(open) = self.current.take() else { return };
        let mut end = end_line.min(self.line_count);
        while end >= open.line_start && self.line_at(end).trim().is_empty() {
            if end == 0 {
                break;
            }
            end -= 1;
        }
        if end < open.line_start {
            return;
        }
        let slice = &self.lines[(open.line_start - 1) as usize..end as usize];
        let text = normalize_block(slice);
        let section = Section {
            index: self.sections.len() as u32,
            level: open.level,
            heading_path: open.heading_path,
            line_start: open.line_start,
            line_end: end,
            hash: blake3_hex(&text),
            token_estimate: token_estimate(&text),
            code_langs: open.code_langs,
            has_tables: open.has_tables,
            text,
        };
        self.sections.push(section);
    }

    fn line_at(&self, line: u32) -> &str {
        if line == 0 {
            return "";
        }
        self.lines.get((line - 1) as usize).copied().unwrap_or("")
    }

    fn finish(mut self) -> Document {
        self.close_current(self.line_count);
        let all_lines: Vec<&str> = self.lines[..self.line_count as usize].to_vec();
        let whole = normalize_block(&all_lines);
        let title = self
            .h1
            .or_else(|| self.frontmatter.as_deref().and_then(front_matter_title))
            .or(self.export_title);
        Document {
            title,
            frontmatter: self.frontmatter,
            token_estimate: self.sections.iter().map(|s| s.token_estimate).sum(),
            hash: blake3_hex(&whole),
            line_count: self.line_count,
            sections: self.sections,
            links_internal: self.links_internal,
            links_external: self.links_external,
        }
    }
}

/// Concatenate the plain text of all inline descendants of a node.
fn inline_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    for n in node.descendants() {
        let d = n.data.borrow();
        match &d.value {
            NodeValue::Text(t) => out.push_str(t),
            NodeValue::Code(c) => out.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
            _ => {}
        }
    }
    out.trim().to_owned()
}

fn strip_front_matter_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    let inner = trimmed.strip_prefix("---").unwrap_or(trimmed);
    let inner = inner.strip_suffix("---").unwrap_or(inner);
    inner.trim().to_owned()
}

fn has_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
            && (rest.starts_with("//") || scheme.eq_ignore_ascii_case("mailto"))
    })
}

#[cfg(test)]
mod tests;
