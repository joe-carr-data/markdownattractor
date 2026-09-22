# ADR-0004 — Phase 2 stack: vectors in plain SQLite scanned from Rust, `fastembed` for embeddings, MCP as `mda mcp` on `rmcp`

Status: **Accepted** · 2026-09-22 · Plan §5, §13 questions 1 and 3, §16 · Evidence: `docs/plans/2026-09-phase2-search.md`

## Context

Phase 2 adds the third ranked list (vectors), the MCP surface Claude talks to, the time commands and the eval harness. The plan (§5, §16) named `sqlite-vec` + `fastembed` + `rmcp` and left three things open: the default embedding model (§13.1), MCP as a subcommand or a separate binary (§13.3), and how to deal with `sqlite-vec` pinning `rusqlite ^0.31` (§16.3).

Facts checked on 2026-09-22:

- The workspace forbids `unsafe` (`unsafe_code = "forbid"`). Registering a SQLite extension from Rust is an `unsafe` FFI call (`sqlite3_auto_extension`) whatever crate wraps it. `sqlite-vec` 0.1.10 is also still alpha and pins `rusqlite ^0.31` against our 0.40.
- `fastembed` 7.0.1 (Apache-2.0, MSRV 1.88): `BGESmallENV15Q` is 384-dimensional, ~33 MB quantised, CPU-only, ~ms per card. Its default features pull `native-tls` (OpenSSL on Linux, banned by `deny.toml`); `ort-download-binaries-rustls-tls` + `hf-hub-rustls-tls` give a pure-rustls tree (verified: no `openssl-sys`, no `native-tls`). `ort` 2.0.0-rc.13 downloads a prebuilt ONNX Runtime at build time and links it **statically** (`cargo:rustc-link-lib=static=onnxruntime`): one binary, no dylib to ship.
- `rmcp` 3.4.0 (Apache-2.0, MSRV 1.88): `#[tool_router]`/`#[tool]` macros, `Parameters<T>` with `schemars` 1.x (the same schemars we use for the card schema), `stdio()` transport, `Json<T>` structured results.
- The plugin reference: a plugin ships MCP servers as `.mcp.json` in the plugin root (`mcpServers` map, `${CLAUDE_PLUGIN_ROOT}` substitution), started automatically when the plugin is enabled; servers may call `roots/list` for the session's working directories.
- Corpus sizes this project targets are hundreds to low thousands of files: 10K sections × 384 f32 is 15 MB, and a brute-force dot product over it is single-digit milliseconds.

## Decision

1. **Vectors live in a plain SQLite table and are scanned from Rust.** Schema v3 adds `embeddings(section_hash PRIMARY KEY, model, dim, vector BLOB)` with L2-normalised little-endian `f32`. Search loads the rows of the current model into a `VectorIndex` and takes the top-k by dot product. No `sqlite-vec`: it would cost an `unsafe` exemption, a rusqlite downgrade and an alpha dependency for a speed-up we do not need below ~50K sections. If a corpus ever gets there, `sqlite-vec` (or its `rescore` ANN) slots in behind the same `VectorIndex` interface.
2. **Embeddings come from `fastembed` with `BGESmallENV15Q` as the only built-in model** (`embeddings = "local-small"`, default; `"off"` disables everything and never downloads). Features: `ort-download-binaries-rustls-tls`, `hf-hub-rustls-tls`, nothing else. The model cache is `embedding_cache_dir` from the config, else `$MDA_MODEL_DIR`, else `~/.cache/markdownattractor/models`; the plugin's launcher and `.mcp.json` set `MDA_MODEL_DIR` to `${CLAUDE_PLUGIN_DATA}/models` (the binary never reads `CLAUDE_PLUGIN_DATA` itself: in a developer's shell it can belong to another plugin). Multilingual or larger models are a later option behind the same `Embedder` trait.
3. **What is embedded** (plan §5): the document title, the heading path, `tldr`, `summary`, `keywords` and `questions_answered` of a card. Only carded sections get vectors, keyed by `section_hash` like the cards, produced right after a card is attached and backfilled by `mda rebuild --embeddings`. The model name is stored with every row; rows of another model are ignored and rebuilt.
4. **The download never blocks a query.** The model is fetched on the first embedding pass (daemon or `mda index`), one line of progress, never from `mda search` or the MCP server. If it cannot be fetched, one warning, and search stays lexical until it can. `mda doctor` reports the model state.
5. **Fusion is a third RRF list** with the same `k = 60`; hits carry `vector: bool` and `Matched::Vector` for vector-only hits. `mda explain` prints all three lists and the fused result.
6. **MCP is a subcommand of the same binary: `mda mcp`** on `rmcp` 3 (`server`, `transport-io`, `macros`, `schemars`), stdio. Tools: `mda_search`, `mda_card`, `mda_open`, `mda_timeline`, `mda_recent`, `mda_stale`, `mda_status`. The root is `--root`, else `$MDA_ROOT`, else the nearest indexed ancestor of the working directory. The plugin's `.mcp.json` runs `${CLAUDE_PLUGIN_ROOT}/scripts/mda mcp`. Results are the same serde types the CLI prints with `--json`, so a skill and a tool call see one format.
7. **Evals start with retrieval metrics, offline.** `mda eval --golden <dir>` runs `queries.jsonl` against a corpus directory and reports recall@k and MRR for lexical-only and hybrid. The golden set ships in `evals/golden/`. The answer-quality A/B parity protocol (§11) is scripted in Phase 4; nothing in Phase 2 claims token savings.

## Consequences

- Three new crates: `fastembed` (with `ort`, `tokenizers`, `hf-hub`, `ureq` behind it) and `rmcp` (+ `rmcp-macros`). The binary grows by the static ONNX Runtime (tens of MB); build time grows by the one-time binary download. Cargo comments reference this ADR.
- Coverage and MSRV jobs compile `ort` too; CI needs network access at build time, which it already has for crates.
- `Matched` gains a variant; `Hit` gains `vector`. `search.rs` gets a `VectorIndex` and a query-embedding step that is skipped when no embedder is available.
- Search latency budget: lexical ≈ 2 ms plus a vector scan that must stay under 10 ms at 10K sections (measured in the plan's exit criteria).
- §13 questions 1 and 3 are closed by this ADR.

## Alternatives considered

- **`sqlite-vec`** — see above: `unsafe`, rusqlite pin, alpha; and premature for the corpus sizes at hand.
- **`model2vec-rs` (static embeddings, no ONNX)** — smaller and pure Rust, but a less proven retrieval quality and a crate whose license metadata is unset; kept in mind as a lean alternative if binary size becomes a complaint.
- **A separate `mda-mcp` binary** — two binaries to bootstrap, version and checksum for no isolation gain; the server is a thin wrapper over the engine.
- **Embedding raw section text instead of cards** — cards are what users search by question; raw-chunk embeddings are a future opt-in for very technical corpora (plan §5).
