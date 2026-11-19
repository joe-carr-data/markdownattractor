# Codex review — PR #122, the Claude subscription backend documented (2026-11-19)

Reviewer: Codex (`gpt-6.2-astra`, read-only). Scope: `git diff main...docs/subscription-backend` (README, `CLAUDE_CLI_POLICY` and a doc comment in `crates/mda-core/src/config.rs`, the `/mda` skill, ADR-0010, CHANGELOG).

| # | Severity | Finding | Triage |
|---|---|---|---|
| 1 | High | ADR-0010 said the terms' sign-in allowance covers mda's automated calls; the page does not say that, and keeps the third-party routing restriction. | **Accepted.** The ADR now leaves that question open, with volume, and calls the documentation the project's decision, not Anthropic's approval. |
| 2 | High | `mda backend` repairing a `claude-cli` config without the acknowledgement saved `Config::default()` over the user's other settings (e.g. a daily token budget); the new message points users at that path. | **Accepted, fixed.** `Config::load_unvalidated` keeps the parsed settings; a CLI test checks a budget survives. |
| 3 | Medium | "No extra bill" is unconditional, but the spawned `claude` uses `ANTHROPIC_API_KEY` if it is set. | **Accepted.** The table says "your plan's usage limits"; the section says to unset the key. |
| 4 | Medium | "4 workers" and "slower than the API key": both backends start at 4 and scale to 16. | **Accepted.** Table says "4 workers, up to 16"; the speed comparison is removed. |
| 5 | Low | The example mixed a shell `mda` command with a slash command; the plugin does not put `mda` on PATH. | **Accepted.** Both lines are `/mda …`. |

Passed: the anchor matches the heading; the CLI accepts the flag and the plugin forwards `restart`; no test or snapshot depended on the old wording.
