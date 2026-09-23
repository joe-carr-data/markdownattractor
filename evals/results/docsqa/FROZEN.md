# FROZEN — DocsQA-Repo, protocol: development

Written by `scripts/eval/freeze.sh` (execution plan §2.0). The **Inputs** section is compared byte for byte by `scripts/eval/preflight.sh` before any run; the **Runtime** section is recorded for the reader and a difference there is reported, never a failure. Numbers produced under this freeze are labelled *development*.

## Inputs

### Dataset
- PowderXu/docsqa-data commit: 19af578bead6c8317d29598c409e982886951cbe
- data/manifest.json sha256: c6193cc88cdf88adc2c8561441b03280415bf871e0b22f7d006a5968c714a361
- data/corpus.jsonl sha256: d7ade1a007c04fcec5627b1583d31f360ef5f672da466599d5b156c3ea9ff1b4
- data/questions.jsonl sha256: 72ca3c11313c0d06d22e41b49a229de379b04d79758f73ba79fc035af4b53fd7
- data/answers.jsonl sha256: 28efa3a03b33def6be0ee7245abe55f682baaff55c85b1f935914c83a8d2dcd0

### Repositories at their pinned commits (`.mda-pinned` = `git rev-parse HEAD`)
- github-docs (`github-docs/`): c34e3dccad00f61133c799d20e7d1208a0e6cc92 · 3742 markdown/MDX files on disk
- prisma (`prisma/`): c4ac0e9dd35d46ae34b5e979b2768be5cd0c390c · 693 markdown/MDX files on disk
- supabase (`supabase/`): 6ea3567948178e81369cd485bc06c5aa40009db3 · 836 markdown/MDX files on disk
- tailwind-css (`tailwindcss/`): bd868a314bd05ca78acd047e3da289274dd6ccd7 · 198 markdown/MDX files on disk

### mda
- version: mda 0.1.1
- source commit: e388c65fe1f3e30002311db379dd8d0f8c0ad12f (the code paths that change a number: crates prompts skills scripts/eval Cargo.lock Cargo.toml rust-toolchain.toml; a check requires them unchanged since this commit)
- build profile: release
- embeddings: local-small · model bge-small-en-v1.5-q (Qdrant/bge-small-en-v1.5-onnx-Q) · revision 52398278842ec682c6f32300af41344b1c0b0bb2 · model files: evals/results/docsqa/model.sha (sha256 152c2d0ba35a04ac44c16682d17678fd4f5b0188a23349850c0477baf1b7ebd9)
- search configuration: the code defaults at the source commit (adapter fetch 30, doubled until ten distinct pages; RRF k 60; cards_fts weights heading 3 / tldr 3 / summary 1 / keywords 1 / questions_answered 2 / entities 1; sections_raw_fts heading 3 / text 1; recency off in the adapter; OR fallback on)
- rows: lexical (raw only) · lexical (cards + raw) · hybrid (cards + raw + vectors); page-level success@5, MRR@5, nDCG@10 (`mda_core::eval`)

### Cards (committed, rule 0.9)
- cards-0.1.1-github-docs.json: sha256 1872bcda4b507c6671811f181b26383fa7cf0aaf7759ca88e311b02757262481 · 20842 cards for 23066 of 23066 sections (complete: true) · backend claude-cli,deterministic · model claude-haiku-4-5,none · prompt deterministic,section.v2 · schema 1 · truncated 4 · 2026-09-22..2026-09-23 · provenance sha256 0788ea973eb4475bdc67357c640d85e259aa95b13724640d09a6cce4ce98dda2
- cards-0.1.1-prisma.json: sha256 80a05cf7807989a21480c3e6c47e2591201c3aeb74dc0478b579ba01a48d0d87 · 8339 cards for 10438 of 10438 sections (complete: true) · backend claude-cli,deterministic · model claude-haiku-4-5,none · prompt deterministic,section.v2 · schema 1 · truncated 3 · 2026-09-22..2026-09-22 · provenance sha256 ba0faa4ebb571a6a7940abdab73b019c920638f60ca795b05b01b6ac75e6b30c
- cards-0.1.1-supabase.json: sha256 92027a0578c933e128cbec9aab201963b4829b8bf0c2b099e85501322c390692 · 6386 cards for 6548 of 6548 sections (complete: true) · backend claude-cli,deterministic · model claude-haiku-4-5,none · prompt deterministic,section.v2 · schema 1 · truncated 4 · 2026-09-22..2026-09-22 · provenance sha256 8562fbb94d87a898e3d961871f3331ebac1b08d752c923ae04a30915836dfac8
- cards-0.1.1-tailwind-css.json: sha256 4ba04219891dda408474711ddccff52fe85376260fe69febd96a25de4eebb041 · 1332 cards for 1518 of 1518 sections (complete: true) · backend claude-cli,deterministic · model claude-haiku-4-5,none · prompt deterministic,section.v2 · schema 1 · truncated 1 · 2026-09-22..2026-09-22 · provenance sha256 ea8b35f86451da867d05db5c9f497e0ef65da6fffabd7556a7ba115acc15d766

### Prompts, rubrics, harness
- prompts/section.v2.txt sha256: 1370605504cc5d480992983692f09b7dd92e26bdcae5ae691d79b6e6d327025e
- prompts/section.schema.v1.json sha256: c6fe125b437c94ee1612211bab0d9a585f58d1d1fddbf3e794149989b37c4bcf
- skills/search-first/SKILL.md sha256: d16e83f1be3667e1e121235660246dee086a5cd6c6daaf5d742e4940c4dfb732
- scripts/eval/ab.sh sha256: c469c9b14fc85388fc4eec51ece154c31ad59d7680743a5469916ede83eac36c
- scripts/eval/grade.sh sha256: b9d79fb26807be4fb4f767067138b29cb5c0a24d739865db9a7714efcc323693
- scripts/eval/probe.sh sha256: b58437cc1898ebf9fce475e13c4c7bbb50c3274b7b3fce7f25ad1de3f7e4c2e6
- scripts/eval/preflight.sh sha256: ae72af83123627412a7552a3a7da8e11f2363a15ac918f3d517a2af087972299
- scripts/eval/freeze.sh sha256: 768e77fbd727b57ffb611d975af9162e8cf4c2bd43c59681811fef6e9e446667
- scripts/eval/lib.sh sha256: 99f712ad14c2885a8bb0567535800ec59809f5176f3ad6c97562d9807ea7ad09

### Split and question sample (seed 20260922; `blake3(seed ‖ id)` order; dev 30% / test 55% / holdout 15%, sealed)
- github-docs: split.json sha256 913c6260f40ec602c8d8a4ec0b1d437fa0209ed02836c87cce99074efe240cbb · dev 59 ids sha256 cdef7fcf2e443f3bb3e1539097e82516f8835964986627bdded65fc33ee74a7c · test 108 ids sha256 59d5294ae1ec70f81e4831dd44848952e8c9273bc8d5de2af336795cb3c02cc3 · holdout 30 (never scored before 1.0)
- prisma: split.json sha256 5dc54a10988b03304f1fa6fe9b4cf8003515512075ef3b88ea07decf73763fa9 · dev 37 ids sha256 6f3e90411bc13b0c5ae657be1c342b8d2376e33e8573e917d73fe406b9d28352 · test 68 ids sha256 9601eb68cd15fb95cfdf9e76a551d773fd8531bbb014f9d32d392c4255c9b7d7 · holdout 20 (never scored before 1.0)
- supabase: split.json sha256 955587e23940d562544d1304efb81a2b92aa30d4c8da7ef7edaf1bb6b629806a · dev 15 ids sha256 47afcffb6402e7aef7744ec5dc2d5f027f17fecc23af51352d8730cd8eb85077 · test 28 ids sha256 ddc4ef341245a478ad29af499e5a911af05045ddfda61056ba9f53170fc0312e · holdout 9 (never scored before 1.0)
- tailwind-css: split.json sha256 554de8e865a4f2c268e8bb9666ae9c35532f5b3ef8f191f9b71edde799029990 · dev 27 ids sha256 91c4d3bbf2375781ff7c26b279c942c7b1a83384a8a52f6e7b41cd74a3b661fd · test 51 ids sha256 846e2b621ccf1c55475e2323b4713e0d7ba2e8744a2a905c76f79b0df66ff8dc · holdout 15 (never scored before 1.0)

### Analysis
- scorer: `mda eval --dataset docsqa` at the source commit (the store's own rows and `--arm-output` for external arms, one page rule for all: sections deduplicated by path in rank order, a truncated list is scored and counted)
- regeneration: `scripts/eval/preflight.sh` re-scores the committed store and a clean reconstruction from the committed cards and compares every row and question with `evals/results/docsqa/<project>/results.json` byte for byte (latency excluded)

### Arms
- mda: this binary through `mda mcp` (`mda_search`, default k 5, up to 50; `mda_open`); the search-first rules `skills/search-first/SKILL.md`; store per checkout under `.markdownattractor/` (`backend = claude-cli`, `claude_cli_policy_ack = true`, `embeddings = local-small`); the adapter scores the store directly
- grep: Claude Code's own Read, Grep and Glob over the checkout, no MCP server, no extra instructions beyond the probe preamble (`scripts/eval/probe.sh`)

## Runtime

- frozen_at: 2026-09-23T06:08:39Z
- hardware: Apple M3 · 24 GB · macOS 15.2
- toolchain: rustc 1.98.1 (48a229cea 2026-09-01)
- claude: 2.1.280 (Claude Code) (the answering, grading and probe model ids are resolved per run and read from the run logs, never from an alias)
