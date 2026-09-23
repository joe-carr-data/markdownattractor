#!/usr/bin/env bash
# Shared by scripts/eval/{freeze,preflight,probe}.sh: the benchmark's fixed locations and
# the few helpers every step repeats. Source it; never run it. Every path is absolute.
#   REPO   the source checkout (this repository)
#   RUN    the bench data: docsqa-data and the four checkouts with their stores
#   MDA    the release binary built from REPO
#   MDA_MODEL_DIR  the embedding model cache (hashed into FROZEN.md and model.sha)
REPO="${REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
RUN="${RUN:-$HOME/.cache/markdownattractor/bench}"
MDA="${MDA:-$REPO/target/release/mda}"
export MDA_MODEL_DIR="${MDA_MODEL_DIR:-$HOME/.cache/markdownattractor/models}"
RESULTS="$REPO/evals/results/docsqa"
# The Rust toolchain, for the recorded rustc version and the release build.
[ ! -f "$HOME/.cargo/env" ] || . "$HOME/.cargo/env"
# shellcheck disable=SC2034  # used by the scripts that source this file
PROJECTS="github-docs prisma supabase tailwind-css"
# shellcheck disable=SC2034
SEED=20260922

sha256() { shasum -a 256 "$1" | cut -c1-64; }

# The dataset names the Tailwind project tailwind-css; its checkout directory is tailwindcss.
project_dir() { case "$1" in tailwind-css) echo tailwindcss ;; *) echo "$1" ;; esac; }

# One line per model file (lock files excluded, symlinks excluded), path relative to the
# cache, sorted: the content of evals/results/docsqa/model.sha.
model_sha() {
  ( cd "$MDA_MODEL_DIR" && find . -type f ! -name '*.lock' | sed 's|^\./||' | LC_ALL=C sort \
    | while IFS= read -r f; do printf '%s  %s\n' "$(sha256 "$f")" "$f"; done )
}

# A run of `claude -p` from inside a Claude Code session refuses to start while these are set.
unset_nested_session() {
  local v
  for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_PID|CLAUDE_PLUGIN_DATA|CLAUDE_PLUGIN_ROOT|CLAUDE_PROJECT_DIR|CLAUDE_EFFORT)'); do unset "$v"; done
}

# The scored rows of a results.json without the machine-dependent parts (latency), sorted
# keys: what "identical metrics on the whole split" compares.
normalize_runs() {
  jq -S '{seed, split, fetch, runs: [.runs[] | {run, split, missing, unknown,
           questions: .metrics.questions, success_at_5: .metrics.success_at_5,
           mrr_at_5: .metrics.mrr_at_5, ndcg_at_10: .metrics.ndcg_at_10,
           results: [.results[] | {id, split, rank, ndcg_at_10, fetched, truncated, top, relevant}]}]}' "$1"
}

# The first three eligible questions of the dev split of a project, from its committed
# results (the raw row lists every scored question in dataset order): the probe questions.
probe_ids() { jq -r '.runs[0].results[:3][].id' "$RESULTS/$1/results.json"; }

question_text() { # project question_id
  jq -r --arg id "$2" 'select(.question_id == $id) | .query' "$RUN/docsqa-data/data/questions.jsonl" | head -c 20000
}
