# Backlog

Ideas and work that are agreed in principle but not scheduled. One entry each: what, why, what we already know, and the exit criterion that would move it into a phase plan. Newest first.

## B-0001 — Ingest non-markdown documents as "virtual markdown" (added 2026-09-22)

**What.** Let the same pipeline summarize Office, web and (optionally) PDF documents by converting them to markdown at intake. v1 stays markdown-only (plan §1 non-goals); this is Phase 2+.

**Design sketch.**
- Add an `Ingest` trait now, before the daemon lands, so a document's source is `path → markdown text` and everything downstream (parser, hashes, cards, search) stays untouched. Markdown files use an identity ingester.
- Provenance on the doc card: `source_format`, `converter`, `converter_version`. Hash the **original bytes**, not the generated markdown, so a converter upgrade re-summarizes intentionally, never silently.
- Line ranges for `mda open` on converted docs refer to the *generated* markdown; store it under `.markdownattractor/converted/<doc_id>.md` so `open` still returns exact lines.

**Candidate libraries (surveyed 2026-09-22; the Rust ecosystem here is 2025–26 work).**

| Library | Formats | Pure Rust | Notes |
|---|---|---|---|
| **anydoc** (2026) | DOC, DOCX, PPT, PPTX, XLS, XLSX, ODT/ODS/ODP, RTF, EPUB, CSV, PDF | yes | Content-marker detection, one document model, one GFM serializer. First pick for Office/web: small, clean, broad. |
| **anytomd** (v1.2.x) | DOCX, PPTX, XLSX, HTML, CSV, JSON, XML, images | yes | MarkItDown clone in Rust, zero runtime. PDF deliberately out of scope. Streams large XLSX. Second opinion to diff against anydoc. |
| **kreuzberg** (v4.2.x, Rust core) | 97+ formats incl. PDF, Office, images (OCR), HTML, email, archives; code intelligence via tree-sitter | mostly (PDFium +25 MB, Tesseract/Paddle OCR, ONNX layout) | Near Docling quality on PDFs (91.0 vs 91.4 structure F1 on 171 PDFs) at 2.8× faster. Feature-flagged. Ships an MCP server and a Claude Code skill. **Licence check needed**: docs.rs says MIT, a recent README says Elastic License 2.0. |
| markdownify (in `mcat`) | DOCX, ODT/ODP, PDF, PPTX, XLSX, CSV, ZIP | yes | Simpler, less structural fidelity; fallback. |
| html-to-markdown (Kreuzberg's engine), htmd, fast_html2md | HTML | yes | For web content; Kreuzberg's is the most complete. |

Non-Rust calibration points: MarkItDown (Microsoft, breadth reference, weak on PDF), Docling (IBM, best open PDF structure, heavy), Marker / MinerU (quality ceiling for scanned or scientific PDFs, GPU), downmark (Go, single binary).

**Plan when scheduled.**
1. Office + web in core: `anydoc` first, `anytomd` as a second opinion. Decide by running both on a 30-file corpus and diffing outputs. Both are pure Rust and do not bloat the bootstrap binary.
2. PDF + OCR as an optional feature: `kreuzberg` behind `--features pdf`, shipped as a separate binary variant (`mda-full`) because of PDFium and the ONNX weights. Resolve the licence first; if ELv2 is confirmed, use anydoc's PDF path for text PDFs and leave OCR to users who bring their own tool.
3. ADR for the converter choice and the licence decision; `mda doctor` reports which converters the installed build has.

**Exit criterion to schedule it.** Phase 2 search is green and at least one design partner has a corpus where more than a quarter of the knowledge lives outside `.md` files.

**Open questions.** Kreuzberg licence; whether converted-doc line ranges should point at the original (page/paragraph) rather than the generated markdown; token cost of converted spreadsheets (cap rows per sheet?).
