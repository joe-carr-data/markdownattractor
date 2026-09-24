# FROZEN — DocsQA-Repo, protocol: final, table T1

Written by `scripts/eval/freeze.sh` (execution plan §2.0). The **Inputs** section is compared byte for byte by `scripts/eval/preflight.sh` before any run; the **Runtime** section is recorded for the reader and a difference there is reported, never a failure. Numbers produced under this freeze are labelled *final*. T1 final freeze (M4): the pre-tuning configuration is the winner of plan §3 (both embedding-text candidates discarded; the post-stop c3–c7 diagnostics are excluded from selection). Test split scored once for every arm of §2.1; the arm builds are the M2 builds (same records and artifact hashes). Re-written at 626baa6 after the Codex M4 pass (BM25-over-files table fingerprints added at 1333553; archive naming at 11c9fc6; the judge's excerpt audit at 0a1dab1; the pooled sampler restricted to corpus pages at 626baa6; inputs otherwise unchanged); rows produced before a rewrite are listed with their source commit in each arm's manifest.

## Inputs

### Dataset
- PowderXu/docsqa-data commit: 19af578bead6c8317d29598c409e982886951cbe
- data/manifest.json sha256: c6193cc88cdf88adc2c8561441b03280415bf871e0b22f7d006a5968c714a361
- data/corpus.jsonl sha256: d7ade1a007c04fcec5627b1583d31f360ef5f672da466599d5b156c3ea9ff1b4
- data/questions.jsonl sha256: 72ca3c11313c0d06d22e41b49a229de379b04d79758f73ba79fc035af4b53fd7
- data/answers.jsonl sha256: 28efa3a03b33def6be0ee7245abe55f682baaff55c85b1f935914c83a8d2dcd0

### Repositories at their pinned commits (`.mda-pinned` = `git rev-parse HEAD`; tracked content verified unchanged; untracked: only `.mda-pinned` and `.markdownattractor/`)
- github-docs (`github-docs/`): c34e3dccad00f61133c799d20e7d1208a0e6cc92 · 3742 markdown/MDX files on disk · effective config.toml sha256 808738cc33c8e91fec330fc497141694d2fb79b5fbb49140200cdc67b5b9155f · .git/info/exclude rules: none
- prisma (`prisma/`): c4ac0e9dd35d46ae34b5e979b2768be5cd0c390c · 693 markdown/MDX files on disk · effective config.toml sha256 808738cc33c8e91fec330fc497141694d2fb79b5fbb49140200cdc67b5b9155f · .git/info/exclude rules: none
- supabase (`supabase/`): 6ea3567948178e81369cd485bc06c5aa40009db3 · 836 markdown/MDX files on disk · effective config.toml sha256 808738cc33c8e91fec330fc497141694d2fb79b5fbb49140200cdc67b5b9155f · .git/info/exclude rules: none
- tailwind-css (`tailwindcss/`): bd868a314bd05ca78acd047e3da289274dd6ccd7 · 198 markdown/MDX files on disk · effective config.toml sha256 808738cc33c8e91fec330fc497141694d2fb79b5fbb49140200cdc67b5b9155f · .git/info/exclude rules: none

### mda
- version: mda 0.1.1
- source commit: 626baa620f77f0a44bc5bc0c8e6c22a5a495d819 (the code paths that change a number: crates prompts skills scripts/eval Cargo.lock Cargo.toml rust-toolchain.toml .cargo; a check requires them unchanged since this commit; the preflight builds with `cargo build --release --locked` and uses the executable Cargo reports, recording its sha256)
- build profile: release
- embeddings: local-small · model bge-small-en-v1.5-q (Qdrant/bge-small-en-v1.5-onnx-Q) · revision 52398278842ec682c6f32300af41344b1c0b0bb2 · model files: evals/results/docsqa/model.sha (regular files and the snapshot links the loader opens, each with the sha256 of its content; sha256 of the file 57ea5d7e9f6ec8f153d64a65e4c30e33e85b704cf8997144289cfe83354fb2fc)
- search configuration: the code defaults at the source commit (adapter fetch 30, doubled until ten distinct pages; RRF k 60; cards_fts weights heading 3 / tldr 3 / summary 1 / keywords 1 / questions_answered 2 / entities 1; sections_raw_fts heading 3 / text 1; recency off in the adapter; OR fallback on)
- rows: lexical (raw only) · lexical (cards + raw) · hybrid (cards + raw + vectors); page-level success@5, MRR@5, nDCG@10 (`mda_core::eval`); every question's first ten distinct pages are archived in results.json, from which the metrics regenerate without a store

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
- scripts/eval/probe.sh sha256: 602ce1c1e4fd70951a7390829a1b266bdce960106770af403eb316ab9044e6ea
- scripts/eval/preflight.sh sha256: 6ad5a17b29db8b8131554850d3e2e6db7c6c002b3fb8e1a6b1c33c93f8edb192
- scripts/eval/freeze.sh sha256: 609040b71cb9e044e3b8a1e25e7b67c97ee786d3c86f85fd0f07ae51074ff048
- scripts/eval/lib.sh sha256: a0e7ea29457a7c74e78ec9cc162e738a84a91040cd3ba27c3da1685103a06730
- scripts/eval/table.sh sha256: bab1f61064a4ef2eea36f332e7850ca9ce3e14500fb4122cc4512b1cec0638ab

### Split and question sample (seed 20260922; `blake3(seed ‖ id)` order; dev 30% / test 55% / holdout 15%, sealed)
- github-docs: split.json sha256 913c6260f40ec602c8d8a4ec0b1d437fa0209ed02836c87cce99074efe240cbb · dev 59 ids sha256 cdef7fcf2e443f3bb3e1539097e82516f8835964986627bdded65fc33ee74a7c · test 108 ids sha256 59d5294ae1ec70f81e4831dd44848952e8c9273bc8d5de2af336795cb3c02cc3 · holdout 30 (never scored before 1.0)
- prisma: split.json sha256 5dc54a10988b03304f1fa6fe9b4cf8003515512075ef3b88ea07decf73763fa9 · dev 37 ids sha256 6f3e90411bc13b0c5ae657be1c342b8d2376e33e8573e917d73fe406b9d28352 · test 68 ids sha256 9601eb68cd15fb95cfdf9e76a551d773fd8531bbb014f9d32d392c4255c9b7d7 · holdout 20 (never scored before 1.0)
- supabase: split.json sha256 955587e23940d562544d1304efb81a2b92aa30d4c8da7ef7edaf1bb6b629806a · dev 15 ids sha256 47afcffb6402e7aef7744ec5dc2d5f027f17fecc23af51352d8730cd8eb85077 · test 28 ids sha256 ddc4ef341245a478ad29af499e5a911af05045ddfda61056ba9f53170fc0312e · holdout 9 (never scored before 1.0)
- tailwind-css: split.json sha256 554de8e865a4f2c268e8bb9666ae9c35532f5b3ef8f191f9b71edde799029990 · dev 27 ids sha256 91c4d3bbf2375781ff7c26b279c942c7b1a83384a8a52f6e7b41cd74a3b661fd · test 51 ids sha256 846e2b621ccf1c55475e2323b4713e0d7ba2e8744a2a905c76f79b0df66ff8dc · holdout 15 (never scored before 1.0)

### Analysis
- table: T1 · protocol final · split scored: test, once (plan §2.0; reuse of the test split by a later release is stated on the page)
- selection: original §3 winner (the pre-tuning configuration, `evals/results/docsqa/TUNING.md`); post-stop diagnostics excluded from selection (plan §3, 2026-09-24 amendment)
- intervals: per row, 95% bootstrap over the row's questions, 5,000 draws, seed 20260922 (`mda eval --interval`); mda hybrid vs qmd full per project: within-project paired bootstrap (`mda eval --compare`), reported as the product target met or not
- scorer: `mda eval --dataset docsqa` at the source commit (the store's own rows and `--arm-output` for external arms, one page rule for all: sections deduplicated by path in rank order, a truncated list is scored and counted)
- regeneration (plan §2.0b): `scripts/eval/preflight.sh` feeds the archived page lists of `evals/results/docsqa/T1/<project>/results.json` back through `--arm-output` and requires the same metrics and per-question results; the store replay and the clean reconstruction from the committed cards are separate checks, each compared on every row and question (latency excluded)

### Arms
- mda: this binary through `mda mcp` (`mda_search`, default k 5, up to 50; `mda_open`); the search-first rules `skills/search-first/SKILL.md`; store per checkout under `.markdownattractor/` (`backend = claude-cli`, `claude_cli_policy_ack = true`, `embeddings = local-small`); the adapter scores the store directly
- grep: Claude Code's own Read, Grep and Glob over the checkout, no MCP server, no extra instructions beyond the probe preamble (`scripts/eval/probe.sh`)
- bm25-files: bm25-files-github-docs (version python3 sqlite3 3.43.2 FTS5 · coverage 1 · record sha256 4dc8b2a9a8b81908019a5e98c12a0eda4392a24a1073a1acda88fc915354d291 · table fingerprint de679c94f3472ae0194fa8dba2451b34de5f648a3f7dcd38eddf94222c28d11b); bm25-files-prisma (version python3 sqlite3 3.43.2 FTS5 · coverage 1 · record sha256 9a29a7070bdfc652a471d253828ed27ae06fba26a98c7770dd3681aa14297f36 · table fingerprint ea223ca8e928c30138aa601cb0e1a3da0bb69bfed9db65b5034246644fb5697f); bm25-files-supabase (version python3 sqlite3 3.43.2 FTS5 · coverage 1 · record sha256 4cf857c2ea155c27cef49a18b3d3a8e247a91650c13bbe03ec3d3883602f2d1d · table fingerprint d1af44f338a7d0a5d6462462e88adcec14b114ce686e8c8fe5e68391fa754254); bm25-files-tailwind-css (version python3 sqlite3 3.43.2 FTS5 · coverage 1 · record sha256 0ab1630cadabfc11f5c60f9aa00ef19c46cf43e6d550b6291a16e90e0531abf2 · table fingerprint dff24961dbe4380e5486814f4fd917e0f8ed94a07dfcd83f2eb6f6ce7eea3e3f);
- graphify: graphify-github-docs (version ? · did not complete · record sha256 ce5aad86e75405debca5b96a6ccf4d9933fdb667f32807ebac5ccf6dc0d04374); graphify-prisma (version ? · did not complete · record sha256 2f1f2b5878f60df57694d46add39f487d7fbb752a8cf290a63870ce58c58c431); graphify-supabase (version graphify 0.9.66 · coverage 0.961038961038961 · record sha256 7037cce67cda90a3c077a3f4bb2a3debe4fb36e9a0bbc66fa2b68cb06fc400b4 · graph sha256 ade877996d6aac620d429733d0567dbff65480985361574a2fb5a18cb9d9635b); graphify-tailwind-css (version graphify 0.9.66 · coverage 1 · record sha256 a2c3d4ec69d7ebf6ebbb3ed77ddf56bd06c5584b8e442f9cdf1e886316143c56 · graph sha256 316b55639f58bf7fa351dde8e5fdd102e974b0bf9cb4d5e2024a925ffc02cbcd);
- graphify-haiku: graphify-haiku-github-docs (version graphify 0.9.66 · coverage 0.9953241895261845 · record sha256 89907410f7ec87489472875a855d25f7eeed2e61aa3f4c9a3da5c050291dd368 · graph sha256 cca9c0ceb967e9031114b0795f47f16510e55adb8bee5b11caa7d16c0b77f80d); graphify-haiku-prisma (version graphify 0.9.66 · coverage 0.8540145985401459 · record sha256 4e5b5e32aa3c26e2a93af546e39418240a10c766dec7fec5a93a6ae59184dd58 · graph sha256 7117e221b558ae31b50d0863deaee8abfdbc1617d051622d41d781af4d3a66aa); graphify-haiku-supabase (version graphify 0.9.66 · coverage 0.05714285714285714 · record sha256 24d14ea49a61e5cf63a779bf9a31b657a77890c7dfb54a4f960585b6755ca7de · graph sha256 565ce68aa2aecbe916e99c23fb8ede831156e498beb56e37d404e1400301903b); graphify-haiku-tailwind-css (version graphify 0.9.66 · coverage 0.2436548223350254 · record sha256 4c55e2d19b62c6e03792652b947541838f446bfa70f72f1f29c80986d3bbccc7 · graph sha256 90fd58f8b010227745643f65679a1329b0ffff0d8f83a54186a73abb412d3dd0);
- qmd: qmd-github-docs (version qmd 2.8.3 (facd35e) · coverage 1 · record sha256 0a41a2e9ed46e7112af2a512e526047adc68d007ac8fe664f465cef01662ae8f · index fingerprint 3c923be793e89d9b85cad8eb91277b83a6ff00cfbc379c0d49b7ce092ef35c27); qmd-prisma (version qmd 2.8.3 (facd35e) · coverage 1 · record sha256 64ea62673c42cb625f46279701b5a3e711889df661763041698fc939cbf599ef · index fingerprint 01876e586902675a07da98a2b8dca6dcd0baefc61c1ca74860365f7fbd8fbf93); qmd-supabase (version qmd 2.8.3 (facd35e) · coverage 1 · record sha256 edc7f47bf3bcb04a15538250ae8fdeead5dc2960a19fd9f958a6621205d46300 · index fingerprint eb7f663c2f107c2fb5b9f1618417f9b46a419efe6faccde2078da8c21732eb49); qmd-tailwind-css (version qmd 2.8.3 (facd35e) · coverage 1 · record sha256 77a684e54b0d98bbc08b60ddbea17265eb61271995c243e82ef6a22efa72f061 · index fingerprint d67b10f11530fdcb6267faa0a4eb756f97fefc1819278c6cf0f1afd5223e5801);

## Runtime

- frozen_at: 2026-09-24T18:21:19Z
- hardware: Apple M3 · 24 GB · macOS 15.2
- toolchain: rustc 1.98.1 (48a229cea 2026-09-01)
- claude: 2.1.281 (Claude Code) (the answering, grading and probe model ids are resolved per run and read from the run logs, never from an alias)
