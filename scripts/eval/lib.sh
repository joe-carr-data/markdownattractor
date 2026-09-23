#!/usr/bin/env bash
# Shared by scripts/eval/{freeze,preflight,probe}.sh: the benchmark's fixed locations and
# the few helpers every step repeats. Source it; never run it. Every path is absolute.
#   REPO   the source checkout (this repository)
#   RUN    the bench data: docsqa-data and the four checkouts with their stores
#   MDA    the release binary built from REPO (preflight replaces it with the artifact
#          Cargo reports for the frozen commit and records its hash)
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

die() { echo "$*" >&2; exit 1; }

# sha256 of one regular file; a missing, unreadable or non-regular file is fatal, never an
# empty field (a freeze must not record a sentinel as an input).
sha256() {
  [ -f "$1" ] && [ ! -L "$1" ] || die "sha256: $1 is not a regular file"
  shasum -a 256 "$1" | cut -c1-64
}

# A benchmark identifier (table, project, arm): one path component, no traversal.
ident() { [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || die "not an identifier: $1"; }

# A file we are about to write: refuse a symlink at the path or at any ancestor under
# $REPO/evals/results or $RUN (the state-file rule of .claude/rules/rust.md, in shell).
safe_target() {
  local p="$1" d
  [ ! -L "$p" ] || die "$p is a symlink"
  d="$(dirname "$p")"
  mkdir -p "$d"
  while [ "$d" != "/" ] && [ "$d" != "$REPO" ] && [ "$d" != "$RUN" ] && [ "$d" != "$HOME" ]; do
    [ ! -L "$d" ] || die "$d is a symlink"
    d="$(dirname "$d")"
  done
}

# The dataset names the Tailwind project tailwind-css; its checkout directory is tailwindcss.
project_dir() { case "$1" in tailwind-css) echo tailwindcss ;; *) echo "$1" ;; esac; }

# One line per model artifact, path relative to the cache, sorted: every regular file (lock
# files excluded) with its sha256, and every symlink (the snapshot paths the loader actually
# opens) with the sha256 of the file it resolves to and its target, which must stay inside
# the cache. A broken or outward link is fatal. This is evals/results/docsqa/model.sha.
model_sha() {
  local f target resolved
  ( cd "$MDA_MODEL_DIR" || exit 1
    find . \( -type f -o -type l \) ! -name '*.lock' | sed 's|^\./||' | LC_ALL=C sort | while IFS= read -r f; do
      if [ -L "$f" ]; then
        target="$(readlink "$f")"
        resolved="$(cd "$(dirname "$f")" && cd "$(dirname "$target")" 2>/dev/null && pwd -P)/$(basename "$target")"
        [ -f "$resolved" ] || die "model link $f -> $target does not resolve to a file"
        case "$resolved" in "$(pwd -P)"/*) ;; *) die "model link $f -> $target leaves the cache" ;; esac
        printf '%s  %s -> %s\n' "$(shasum -a 256 "$resolved" | cut -c1-64)" "$f" "$target"
      else
        printf '%s  %s\n' "$(shasum -a 256 "$f" | cut -c1-64)" "$f"
      fi
    done )
}

# No arm may reach a model through a provider key (plan §0a.3: every model call goes through
# the owner's Claude Code login). The owner's shell exports such keys; a headless graphify
# session found GEMINI_API_KEY and ran its extraction through Gemini until this was added.
unset_provider_keys() {
  local v
  for v in $(env | grep -oE '^[A-Z0-9_]*(API_KEY|_TOKEN)=' | sed 's/=$//'); do
    case "$v" in GITHUB_TOKEN|GH_TOKEN) ;; *) unset "$v" ;; esac
  done
  unset GEMINI_API_KEY GOOGLE_API_KEY OPENAI_API_KEY ANTHROPIC_API_KEY 2>/dev/null || true
}

# A run of `claude -p` from inside a Claude Code session refuses to start while these are set.
unset_nested_session() {
  local v
  for v in $(env | grep -oE '^(CLAUDE_CODE_[A-Z_]*|CLAUDECODE|CLAUDE_PID|CLAUDE_PLUGIN_DATA|CLAUDE_PLUGIN_ROOT|CLAUDE_PROJECT_DIR|CLAUDE_EFFORT)'); do unset "$v"; done
}

# The scored rows of a results.json without the machine-dependent parts (latency), sorted
# keys, written to $2: what "identical on the whole split" compares. Fails on bad JSON.
normalize_runs() {
  jq -S '{seed, split, fetch, runs: [.runs[] | {run, split, missing, unknown,
           questions: .metrics.questions, success_at_5: .metrics.success_at_5,
           mrr_at_5: .metrics.mrr_at_5, ndcg_at_10: .metrics.ndcg_at_10,
           results: [.results[] | {id, split, rank, ndcg_at_10, fetched, truncated, pages, relevant}]}]}' "$1" > "$2" \
    || die "normalize_runs: cannot read $1"
}

# The archived page lists of one run of a results.json as --arm-output rows (plan §2.0b:
# metrics regenerate from archived observations, not from a store).
archived_rows() { # results.json run-index > rows.jsonl
  jq -c --argjson i "$2" '.runs[$i].results[] | {question_id: .id, paths: .pages, truncated}' "$1"
}

# The first three eligible questions of the dev split of a project, from its committed
# results (the raw row lists every scored question in dataset order): the probe questions.
probe_ids() { jq -r '.runs[0].results[:3][].id' "$RESULTS/$1/results.json"; }

# The text of one question of one project; exactly one row must match.
question_text() { # project question_id
  local rows
  rows="$(jq -c --arg p "$1" --arg id "$2" 'select(.project == $p and .question_id == $id) | .query' "$RUN/docsqa-data/data/questions.jsonl")"
  [ "$(printf '%s\n' "$rows" | grep -c .)" = 1 ] || die "question $2 of $1: expected exactly one row in questions.jsonl"
  jq -r . <<<"$rows"
}

# The identity of a qmd index: a sha256 over the rows of every table but `llm_cache` (qmd
# writes its query-expansion and rerank results there on every full query, so the file's own
# hash changes when the index is merely used) and the sqlite internals; the vec0 virtual
# tables are read through their shadow tables. Stable across queries, changed by any
# re-index or re-embed.
qmd_fingerprint() { # index-name
  local db="$HOME/.cache/qmd/$1.sqlite" t
  [ -f "$db" ] || die "no qmd index $db"
  sqlite3 "$db" 'PRAGMA wal_checkpoint(TRUNCATE);' >/dev/null 2>&1 || true
  for t in $(sqlite3 "$db" "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name != 'llm_cache' AND name NOT IN ('vectors_vec') ORDER BY name"); do
    printf '%s\n' "$t"; sqlite3 "$db" "SELECT * FROM \"$t\" ORDER BY 1" 2>/dev/null || printf 'unreadable\n'
  done | shasum -a 256 | cut -c1-64
}

# The split a question belongs to, from the committed split.json.
question_split() { jq -r --arg id "$2" '.questions[] | select(.id == $id) | .split' "$RESULTS/$1/split.json"; }
