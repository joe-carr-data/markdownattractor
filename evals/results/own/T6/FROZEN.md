# FROZEN — our own corpora (T6), protocol: final, table T6

Written by `scripts/eval/freeze.sh` (execution plan §2.0). The **Inputs** section is compared byte for byte by `scripts/eval/preflight.sh` before any run; the **Runtime** section is recorded for the reader and a difference there is reported, never a failure. Numbers produced under this freeze are labelled *final*.

## Inputs

### Corpora (public: `git archive <commit>:<subtree>` of this repository; a reader rebuilds them from the published commit)
- golden: d061f8c12948ee306fb39425e4b2bbc5320c923c:evals/golden/docs · tree c14771261e476c779ee816cd6166f60f11273738 · 32 markdown files · page inventory sha256 c11bafacf197e876b0c1bd8fa91d545e89c7351d44e1876e271a12c4d83a95f4 (committed as T6/golden/pages.txt) · store sha256 1387cd0748c4a5673e1292269954bf68bc6001e886a087a4c51785b99136ccce · effective config.toml sha256 808738cc33c8e91fec330fc497141694d2fb79b5fbb49140200cdc67b5b9155f
- docs: d061f8c12948ee306fb39425e4b2bbc5320c923c:docs · tree dfb1ffd23282201c8edf69888716a76c93980812 · 73 markdown files · page inventory sha256 a45828b6dd36bf05e45e20e12f67539e9f53a6457e32a32760cfef97c1ee4e66 (committed as T6/docs/pages.txt) · store sha256 ac47b5174f740b922fc78e6761f6c69b1eebbb13eb66e8e370d2ae68be03ace7 · effective config.toml sha256 808738cc33c8e91fec330fc497141694d2fb79b5fbb49140200cdc67b5b9155f
- private: the owner's trading-research repository, supporting evidence only; its freeze (kept under $RUN) has sha256 25a53dd581106e9c18f7b0e1f033f8339e1977012687e8d978566e5ee62fc985 (committed as T6/private/FROZEN.sha256); exempt from independent reproduction

### Cards (committed, rule 0.9)
- cards-0.1.1-golden.json: sha256 e35768b9b8f7f641629e46ae93e9f0ef946446c791c18119217dedf99311a737 · 117 cards for 117 of 117 sections (complete: true) · model claude-haiku-4-5,none · prompt deterministic,section.v2 · provenance sha256 c3a4fff9001f07564df69a9e216bc88cce3881c5b7b9f8defd36a3efab81fe64
- cards-0.1.1-docs.json: sha256 ace9eac3feedea7adf3db9d20f612b42a0f3b08f8e9e45cd36daa7c67b03255f · 506 cards for 514 of 514 sections (complete: true) · model claude-haiku-4-5,none · prompt deterministic,section.v2 · provenance sha256 742ac8d4e9bf80cc4e4510b094633f45f50e8a0ec2f9fe471f9e9e31453dbd4f

### Questions (generated from the corpus, panel-validated; texts committed for the public corpora)
- golden: questions.jsonl sha256 5af4e8df421ca41e7ba1b07b95d138b94f7e0936d5ab370a3a02aad9621c6b1f (16 counted) · pilot sha256 d67968cb4ac2797cd28f528983aed582f270831aaafb16ec120db3e24c50cb18 (4 pilot) · candidates sha256 1373828337e1084556a5ada9250219c17486fc6a5a910bee422f7cdc63c14ea5 · rejected sections sha256 e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 · summary sha256 e42ef5a040475fb1c2ce813d2ce643174fd3e3ed8f851392073bb914a75c620a
- docs: questions.jsonl sha256 9a3e02d3e5fd6a530bc1967937ec20a17a6251541c23ed05020a5e7b2214258c (25 counted) · pilot sha256 e63c2f9ea4678056453e6e485792115974f9f3cbc25fd410ae3b335ec0407c63 (10 pilot) · candidates sha256 11c06dc5be06db8eb5264518dee58eb66ade7494f0751ff79273aee236b693e9 · rejected sections sha256 a4ba911a2ec65c8de2cb9fb54dc138113e641c5f77a4f3920e5cf33d079952c6 · summary sha256 64d552257af79379fc39343b60b2e8d91c6a18afed5e02a708b9ec6b9190f080
- private: questions.jsonl (ids and digests) sha256 92952d5d7cc13e75fb38d705a01bcf202108b8a86963a98257cd0c76a5e84102 (25 counted)

### mda
- version: mda 0.1.1 · binary sha256 432bac9166c9cb2a8bcaaa98e7da780100eb39a6aa55247fd848cb0ef2106346 (the preflight rebuilds with `cargo build --release --locked` from the source commit and requires this hash)
- source commit: 4451a7a6e6fbadfda94659cb260f6dc00507a63f (the code paths that change a number: crates prompts skills scripts/eval Cargo.lock Cargo.toml rust-toolchain.toml .cargo; a check requires them unchanged since this commit)
- analysis: `mda eval --analysis` (crates/mda-core/src/eval/analysis.rs: question medians over runs with failed runs as 0, 10,000-draw paired bootstrap, seed 20260922, gates lower bound ≥ −0.25 · mda mean ≥ 4.0 · grounding ≥ 0.95)
- embeddings: local-small · model bge-small-en-v1.5-q (Qdrant/bge-small-en-v1.5-onnx-Q) · revision 52398278842ec682c6f32300af41344b1c0b0bb2 · model files sha256 57ea5d7e9f6ec8f153d64a65e4c30e33e85b704cf8997144289cfe83354fb2fc (evals/results/docsqa/model.sha)
- store configuration template: evals/results/own/T6/config.toml sha256 808738cc33c8e91fec330fc497141694d2fb79b5fbb49140200cdc67b5b9155f (backend claude-cli, summariser claude-haiku-4-5, embeddings local-small; the ignore rules inside it)

### Calibration (the development pilot; required before the freeze, review F4/F11/F22)
- calibration.json sha256 e2ca0e48421ce2956586f6c545e48695b07ce16d86a3397db6b6c5b2e30a41ad · resolved model ids: answering claude-sonnet-5-5 · grader claude-sonnet-5-5 · grounding claude-sonnet-5-5 · panel_fable claude-fable-5-1 · panel_astra  · declared resolution rule: either-member-differs-by-more-than-one-point-then-panel-mean

### Prompts, rubrics, harness
- prompts/section.v2.txt sha256: 1370605504cc5d480992983692f09b7dd92e26bdcae5ae691d79b6e6d327025e
- prompts/section.schema.v1.json sha256: c6fe125b437c94ee1612211bab0d9a585f58d1d1fddbf3e794149989b37c4bcf
- skills/search-first/SKILL.md sha256: d16e83f1be3667e1e121235660246dee086a5cd6c6daaf5d742e4940c4dfb732
- scripts/eval/t2.sh sha256: 2b200c533d4574feb4cc823adc73cc9f8e4c3afba43bf5194e455bf687d00947
- scripts/eval/t6.sh sha256: f1e2422e96179951867f40a10a11f8e7eac6a1d56141b743de9d37b737697630
- scripts/eval/t6_resolve.py sha256: 46bf0066714ac457ee8711d09835faa9cadf891b599d4cced1c861c6f0249ae4
- scripts/eval/t6_private_export.py sha256: 61cc4e76875b11d1f0be2bbe5062bed3c1a12718c10144432a6eca380b008257
- scripts/eval/panel.sh sha256: 7489c1ef88e4dbf4508fbe49c80e1cb2842da1470ad40dc83552598920ca1e73
- scripts/eval/probe.sh sha256: b653a014074ecdaba6ceff341e0e15dadd8da50c8f90d2bba01b0e8c1ff4dda3
- scripts/eval/freeze.sh sha256: 46e6d33caddc45701fbdc1a15ba6933453fc3dd19e3df8df2f9135c756d71661
- scripts/eval/lib.sh sha256: 6dd339475d143e664c96d26be5f50c110c53b0363422c743faded323c71c270f
- scripts/eval/arms/qmd.sh sha256: 945f83ca02600de005c2005a3c01e87043dd0eaf451eb1bf38e2704bf2e9a95e
- scripts/eval/arms/graphify.sh sha256: e730f686c372ef5c547667cfd07acfb24ac88b0e86e9c53f1dda4116087b97c0
- rubrics: the T6 adaptation (T2_RUBRIC=t6 in t2.sh and panel.sh: required facts + evidence section; factual claims about the corpus's subject matter); the question writer (t6.sh candidates) and the panel's two-call question rubric (panel.sh questions) — all inside the hashed files above

### Run and analysis parameters
- answering model requested: sonnet · grader: sonnet · panel: claude-fable-5-1 through the owner's Claude Code login, gpt-6-astra through the owner's Codex login · question writer: sonnet (resolved ids per the calibration record above and the run rows)
- 25 counted questions per corpus (single-section lookup; synthetic, panel-validated, one per page; a corpus that reached fewer publishes with its count) and up to 10 disjoint pilot questions (never counted), 3 runs per question and arm, max 12 turns, 600 s per run, one declared retry, every MCP server cold (rule 0.9 as amended); the arm set certified by the preflight
- arms: grep (Read/Grep/Glob) · mda (`mda mcp` + search-first rules) · qmd (its MCP server on index t6-<corpus>, its skill) · graphify (its MCP server on a Sonnet-built graph of a copy; where the build did not complete the arm is absent and the record says so)
- arm versions: qmd qmd 2.8.3 (facd35e) · graphify graphify 0.9.66 · claude 2.1.302 (Claude Code) · codex codex-cli 0.155.1

### Arms (records under evals/results/own/arms/; the live artifacts they serve from)
- qmd-golden: record sha256 d95dec4c9cedf667bb2d63d6d41949e6473726395aa825fcf0e7f79132fe340e · index t6-golden fingerprint 3e863b4f86a69322ea9fe0b3550baf3acf8caa49e336f7851bb994eb4bfd6c6e · collection file sha256 29824900901104dd5c75b16643f5ea63af48b012654e74cb09794f9a93dc6b97
- graphify-golden: record sha256 6dea193fe80f97ebf1651173c27f6daa72967b0aeae6fad364c06ef8f8d4555d · served graph sha256 f94ef06394f3a1facf35c15e83dab279bbc891bf3388ce500179c6a5a1a6e113 · skill sha256 3e4d30df5a936965acaa25117230a6f3ff44353c137e63030e85e3bc3a391fb7 · hook settings sha256 6150edf44c68a31969bf1453eb8c4a0263cb150fee228d468b487b0dc36fb84a
- qmd-docs: record sha256 8c636f3d7f84c96efac92ccdb943529dadb3f7715d0681faec321ecb8b190d39 · index t6-docs fingerprint f74386635a1f5f664da4b5ba4a77473e8a6a06904ace9c6987972e5e8e17cf02 · collection file sha256 9d4def49119c842bcb7e558d822d9d2ada851f58ce7f82125dda3fba0fdb0e7c
- graphify-docs: record sha256 4ee9e9a1b6df2afe833e329667ef65827b3b9f1f0b53a6a4389cbabfa8859087 · served graph sha256 89055aac1e8dd73425b71f036f50e7b2613e5e6e1cbb8ade02fa5b2a689dfa0c · skill sha256 3e4d30df5a936965acaa25117230a6f3ff44353c137e63030e85e3bc3a391fb7 · hook settings sha256 6150edf44c68a31969bf1453eb8c4a0263cb150fee228d468b487b0dc36fb84a

## Runtime

- frozen_at: 2026-10-27T02:16:48Z
- hardware: Apple M3 · 24 GB · macOS 15.2
- toolchain: rustc 1.98.1 (48a229cea 2026-09-01)
- claude: 2.1.302 (Claude Code) (the answering, grading and probe model ids are resolved per run and read from the run logs, never from an alias)
