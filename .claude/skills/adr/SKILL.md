---
name: adr
description: Scaffold an Architecture Decision Record in docs/adr/ with the next zero-padded number and a kebab-case title, then register it in docs/index.md. Use when a crate, storage, schema, auth or public-API decision is being made. Argument: the decision title.
---

# /adr <title>

`$ARGUMENTS` is the title, for example `/adr use sqlite-vec for vectors`.

## Steps

1. Compute the next number: `ls docs/adr/ 2>/dev/null | grep -E '^[0-9]{4}-' | sort | tail -1`, take its 4-digit prefix, add 1. Zero-pad to 4 (`0001` if the directory is empty or missing). Create `docs/adr/` if needed.
2. Kebab-case the title: lowercase, non-alphanumerics to `-`, collapse repeats, trim. Filename: `docs/adr/NNNN-<kebab-title>.md`.
3. Write the file with exactly these sections:

   ```markdown
   # ADR NNNN: <Title as given>

   Date: YYYY-MM-DD
   Status: Proposed

   ## Context
   What forces are at play. Cite the plan section, the finding, or the failure that triggered this.

   ## Decision
   One paragraph, present tense: "We use X for Y."

   ## Consequences
   - Positive: …
   - Negative: …
   - Follow-ups: …

   ## Alternatives considered
   - <alternative> — why not.
   ```

   `Status` is one of `Proposed`, `Accepted`, `Superseded by NNNN`. New ADRs start `Proposed`; flip to `Accepted` in the same commit if the user confirms.

4. Fill the sections from the conversation; do not leave placeholder text. If context is missing, ask one question.
5. Run `scripts/dev/gen-index.sh` so `docs/index.md` gets the new row (the PostToolUse hook does this automatically when you use Write; run the script explicitly if you wrote the file another way).
6. If the ADR introduces a crate, remind the user that `Cargo.toml` should reference `ADR NNNN` in a comment (see `.claude/rules/rust.md`).

Never edit an ADR after it is Accepted; write a new one that supersedes it.
