#!/usr/bin/env python3
"""Allowlisted exports of the private corpus's T6 artifacts, and their check.

M9 plan (amendments after the Codex pre-mortem, F8–F13; harness review F1–F6): nothing
content-bearing from the private corpus may enter the repository. Every committed private file
is BUILT by `export` from explicit scalar projections of the private run's files — never by
deleting known keys — into a staging directory, validated there against a FIELD-SPECIFIC typed
schema (every key named, every string typed: opaque id, digest, timestamp, arm, tool, verified
model id, version, or a closed enum), and published atomically only when every file conforms.
`check` validates a committed directory against the same schema and is run by `make check`
(scripts/eval/tests/own-private-schema.sh). `digest-questions` is the one canonical serializer
of the committed private question file (ids and digests), used both when the questions are
drawn and by `export`, so the file's bytes never change under the freeze.

Usage:
  t6_private_export.py export <private-run-dir> <committed-dir>
  t6_private_export.py check <committed-dir>
  t6_private_export.py digest-questions <questions.full.jsonl>   # canonical rows on stdout
  t6_private_export.py project-pilot <pilot.json> <corpus>      # the typed pilot projection (the calibration record is built from these)
  t6_private_export.py check-file calibration <calibration.json>
"""
import hashlib
import json
import os
import re
import shutil
import sys
import tempfile

PLACEHOLDER = "<private>"
ARMS = ["mda", "grep", "qmd", "graphify", "graphify-haiku"]
TOOLS = ["Read", "Grep", "Glob", "Bash", "mcp__markdownattractor__mda_search", "mcp__markdownattractor__mda_open",
         "mcp__markdownattractor__mda_card", "mcp__markdownattractor__mda_timeline", "mcp__markdownattractor__mda_recent",
         "mcp__markdownattractor__mda_stale", "mcp__markdownattractor__mda_status", "mcp__qmd__query", "mcp__qmd__get",
         "mcp__qmd__multi_get", "mcp__qmd__status", "mcp__graphify__query_graph", "mcp__graphify__get_node",
         "mcp__graphify__get_neighbors", "mcp__graphify__get_community", "mcp__graphify__god_nodes", "mcp__graphify__shortest_path"]
# Model identifiers a row may carry: the aliases the harness requests and the resolved ids the
# transcripts report (verified allowlist; a new model id is added here, never inferred).
MODELS = ["sonnet", "haiku", "opus", "none", "golden", "claude-sonnet-5", "claude-sonnet-5-5", "claude-haiku-4-5", "claude-opus-5",
          "claude-opus-5-5", "claude-fable-5-1", "gpt-6-astra"]
REASON_CODES = ["accepted", "skipped-page-taken", "refused", "member-incomplete", "member-rejected", "not-judged"]
REFUSALS = ["path-in-question", "title-in-question", "span-not-in-section", "writer-no-output", "too-few-facts"]
REJECT_KEYS = ["corpus_specific", "standalone", "not_leading", "facts_stated_at_spans", "writer_facts_supported"]
EXCLUSIONS = ["frozen-exclusion", "table-list-or-listing", "empty"]
RESOLUTIONS = ["incomplete", "panel mean (a member differed from Sonnet by more than one point)", "sonnet stands"]
TRIGGER = "either panel member differs from the Sonnet grade by more than one point on 0–6 → the panel mean"
SAMPLED_FROM = "cards with at least one date or entity, seeded order"
EVIDENCE_POLICIES = ["every cited page checked whole, in batches of four", "ungrounded: a cited path does not exist in the checkout",
                     "ungrounded: a cited page exceeds 120,000 characters", "unresolved or oversized citation → ungrounded"]
CHECK_STATUS = ["ok", "FAIL", "skipped", "ACCEPTED-FAIL"]

# ---- the typed schema -------------------------------------------------------------------------
# A spec is: a type name (str), a dict (object with named keys; "?key" marks an optional key),
# a one-element list (array of that spec), or a tuple of specs (any of). Types: hex64, id, sid,
# iso, int, num, bool, null, arm, tool, model, version, placeholder, digit_str, enum:<name>.
NUM = ("num", "null")
INT = ("int", "null")
BOOL = ("bool", "null")
METRIC_RATIO = {"median_ratio": NUM, "questions": "int", "excluded_missing": "int", "excluded_zero_denominator": "int"}
ARM_SUMMARY = {"arm": "arm", "completed": "int", "cost_usd": NUM, "failed": "int", "grounding_rate": "num", "mean_score": "num",
               "mean_score_completed": NUM, "questions": "int", "questions_with_completed": "int", "runs": "int", "source_tokens": NUM, "tool_calls": NUM}
PAIR = {"ci95": ["num"], "comparator": "arm", "gates": {"grounding_ok": "bool", "lower_bound_ok": "bool", "mean_ok": "bool", "pass": "bool"},
        "losses": "int", "mean_delta": "num", "n": "int", "ties": "int", "wins": "int",
        "savings": ({"questions": "int", "source_tokens": METRIC_RATIO, "tool_calls": METRIC_RATIO, "cost_usd": METRIC_RATIO}, "null")}
QUESTION_ROW = {"arm": "arm", "completed": "int", "cost_usd": NUM, "failed": "int", "grounded": BOOL, "id": "id", "runs": "int", "score": NUM,
                "score_completed": NUM, "source_tokens": NUM, "tool_calls": NUM}
ANALYSIS = {"analysis": {"arms": [ARM_SUMMARY], "pairs": [PAIR], "questions": [QUESTION_ROW], "draws": "int", "seed": "int",
                         "missing_comparators": ["arm"], "other_arms": ["arm"]},
            "grades": "placeholder", "manifest": "placeholder",
            "options": {"comparators": ["arm"], "draws": "int", "gates": {"?min_lower_bound": "num", "?min_mean": "num", "?min_grounding": "num"}, "mda_arm": "arm", "seed": "int"}}
GRADE_ROW = {"id": "id", "arm": "arm", "run": "int", "error": "bool", "exit_code": INT, "attempts_made": INT, "timed_out": "bool", "wall_s": NUM,
             "turns": INT, "turns_with_usage": INT, "tool_calls": INT, "failed_calls": INT, "input_tokens": INT, "output_tokens": INT,
             "thinking_tokens": INT, "source_tokens": INT, "source_tokens_signed": INT, "source_tokens_negative_total": INT,
             "source_tokens_negative_turns": INT, "cost_usd": NUM, "cost_usd_all_attempts": NUM, "answer_sha256": "hex64", "question_sha256": "hex64",
             "score": NUM, "correctness": INT, "completeness": INT, "grounded": BOOL, "graded": "bool", "grader": ("model", "null"), "graded_at": ("iso", "null"),
             "models": ["model"], "tools": ["tool"],
             "grounding": {"cited_pages_n": "int", "unresolved_n": "int", "oversized_n": "int", "batches_n": "int", "unsupported_claims_n": "int",
                           "evidence_policy": ("enum:evidence", "null")},
             "?adjudicated": "bool", "?sonnet_score": NUM, "?panel": {"fable": NUM, "astra": NUM, "triggered": BOOL}}
AGREE = {"exact": NUM, "within_one": NUM}
REGRADE = {"agreement": {"fable_astra": AGREE, "fable_sonnet": AGREE, "astra_sonnet": AGREE}, "incomplete": "int", "mean_astra": NUM, "mean_fable": NUM,
           "mean_sonnet": NUM, "resolved_by_panel": "int", "sample": "int", "seed": "digit_str", "stamp": "iso", "trigger": "enum:trigger",
           "rows": [{"sample_id": "sid", "id": "id", "arm": "arm", "run": "int", "sonnet": NUM, "fable": NUM, "astra": NUM, "panel_mean": NUM,
                     "triggered": BOOL, "resolved": NUM, "resolution": "enum:resolution"}]}
REGRADE_MEMBER = {"sample_id": "sid", "member": "enum:member", "score": NUM, "model": ("model", "null"), "cost_usd": NUM, "complete": "bool"}
CARDS = {"seed": "digit_str", "stamp": "iso", "cards_file": "placeholder", "sampled_from": "enum:sampled_from", "sections_missing_from_store": "int",
         "sample": "int", "values": "int", "unsupported_rate": {"fable": NUM, "astra": NUM}, "incomplete": "int", "agreement": NUM,
         "rows": [{"sample_id": "sid", "values": INT, "fable": (["bool"], "null"), "astra": (["bool"], "null")}]}
PREFLIGHT = {"table": "enum:T6", "project": "enum:private", "run_at": "iso", "status": "enum:status", "passed": "bool",
             "checks": [{"name": "enum:check_name", "status": "enum:check_status"}],
             "probes": {"?mda": {"activated": "int", "of": "int"}, "?grep": {"activated": "int", "of": "int"}, "?qmd": {"activated": "int", "of": "int"}, "?graphify": {"activated": "int", "of": "int"}},
             "store_sha256": "hex64", "inventory_sha256": "hex64", "frozen_sha256": "hex64", "private_frozen_sha256": "hex64", "source_commit": "hex40"}
CHECK_NAMES = ["env", "frozen", "frozen-private", "copy", "extras", "inventory", "store", "complete", "cards", "reconstruction", "questions",
               "question-shape", "qmd", "graphify", "probes-mda", "probes-grep", "probes-qmd", "probes-graphify", "leak-scan", "binary", "calibration"]
ARMS_REC = {"qmd": ({"version": "version", "build": {"collection_add_s": "int", "update_s": "int", "embed_s": "int", "total_s": "int"},
                     "coverage": {"files_indexed": "int", "markdown_files_on_disk": "int", "corpus_pages": "int", "corpus_pages_indexed": "int", "coverage": "num"}}, "null"),
            "graphify": ({"version": ("version", "null"), "build": {"completed": "bool", "wall_s": INT, "cost_usd_list_price": NUM, "turns": INT},
                          "graph": ({"sha256": "hex64", "nodes": "int", "edges": "int"}, "null"),
                          "coverage": ({"corpus_pages": "int", "corpus_pages_with_a_node": "int", "coverage": "num"}, "null")}, "null")}
INDEX = {"seconds": "int", "retries": "int", "store_sha256": "hex64", "indexed_at": "iso",
         "counts": {"docs": "int", "sections": "int", "summarized": "int", "pending": "int", "failed": "int", "tombstoned": "int",
                    "total_input_tokens": "int", "total_output_tokens": "int", "total_cost_usd": "num"},
         "embeddings": {"embedded": INT, "model": ("model", "enum:embedding_model", "null")},
         "cards": {"cards": "int", "sections": "int", "sections_carded": "int", "complete": "bool", "truncated": "int",
                   "by_model": {"?claude-haiku-4-5": "int", "?none": "int", "?claude-sonnet-5": "int", "?claude-sonnet-5-5": "int"},
                   "summarized_from": ("iso", "null"), "summarized_to": ("iso", "null"),
                   "usage": {"input_tokens": "int", "output_tokens": "int", "cost_usd_list_price_equivalent": "num"}}}
PILOT = {"corpus": "enum:corpus", "at": "iso", "index": {"seconds": "int", "counts": INDEX["counts"]},
         "candidates": {"written": "int", "refused": "int", "accepted": "int", "judged": "int"},
         "runs": {"n": "int", "errors": "int", "wall_s_total": NUM, "cost_usd": NUM, "grounding_batches": INT, "retried": "int"},
         "seconds": {"runs": "int", "grade": "int", "panel": "int"},
         "panel": {"sample": "int", "incomplete": "int", "agreement": REGRADE["agreement"], "resolved_by_panel": "int", "mean_sonnet": NUM, "mean_fable": NUM, "mean_astra": NUM}}
QUESTIONS = {"id": "id", "split": "enum:own", "category": "enum:single-section", "question_sha256": "hex64", "reference_sha256": "hex64",
             "evidence_sha256": "hex64", "required_facts_n": "int"}
CANDIDATES = {"id": "id", "round": INT, "refused": ("enum:refusal", "null"), "accepted": "bool", "fable_accepts": BOOL, "astra_accepts": BOOL,
              "reason_code": "enum:reason", "rejected_on": ["enum:reject_key"], "role": ("enum:role", "null")}
CAND_SUMMARY = {"headed_sections_200": "int", "eligible_sections": "int", "documents": "int", "excluded": {"frozen-exclusion": "int", "table-list-or-listing": "int", "empty": "int"},
                "written": "int", "refused": {"path-in-question": "int", "title-in-question": "int", "span-not-in-section": "int", "writer-no-output": "int", "too-few-facts": "int"},
                "judged": "int", "accepted_counted": "int", "accepted_pilot": "int"}
MANIFEST = {"project": "enum:private", "model": "model", "runs": "int", "arms": ["arm"], "question_ids": ["id"], "questions_sha256": "hex64", "mda": "version",
            "mda_sha256": "hex64", "search_first_sha256": "hex64", "source_commit": "hex40", "max_turns": "int", "timeout_s": "int", "retries": "int",
            "recorded_at": "iso", "launches": {"?mda": {"with_git": "bool", "mcp_server": ("enum:mcp_server", "null"), "system_prompt_sha256": ("hex64", "null")},
                                               "?grep": {"with_git": "bool", "mcp_server": ("enum:mcp_server", "null"), "system_prompt_sha256": ("hex64", "null")},
                                               "?qmd": {"with_git": "bool", "mcp_server": ("enum:mcp_server", "null"), "system_prompt_sha256": ("hex64", "null")},
                                               "?graphify": {"with_git": "bool", "mcp_server": ("enum:mcp_server", "null"), "system_prompt_sha256": ("hex64", "null")}}}
# The resolution policies the harness implements (panel.sh regrade); the calibration record names one by id,
# never by prose, so the declared rule is the executed rule (pass 3 F12).
POLICIES = ["either-member-differs-by-more-than-one-point-then-panel-mean"]
RESOLVED = re.compile(r"^(claude|gpt)-[a-z0-9]+(-[a-z0-9]+)*$")   # a resolved model id, never an alias (pass 3 F13)
CALIBRATION_NOTE = "the development pilot: disjoint questions, one run per arm, both panel members on every answer; nothing here is a published number"
ROLES = ["answering", "grader", "grounding", "panel_fable", "panel_astra"]
CALIBRATION = {"at": "iso", "corpora": {"?golden": PILOT, "?docs": PILOT, "?private": PILOT},
               "resolved_models": {"answering": ["resolved"], "grader": ["resolved"], "grounding": ["resolved"], "panel_fable": ["resolved"], "panel_astra": ["resolved"]},
               "requested_models": {"answering": ["model"], "grader": ["model"], "grounding": ["model"], "panel_fable": ["model"], "panel_astra": ["model"]},
               # a role whose transcripts carry no resolved id (the Codex CLI's JSON stream names no model) is listed here explicitly, with the requested alias beside it (pass 3 F13, pass 4 F2)
               # only a role whose client names no model in its output may be declared unresolved (today: the Codex CLI's panel member);
               # the preflight requires resolved ids for every other role, so schema and preflight apply one policy (pass 5 F3)
               "unresolved_roles": ["enum:unresolvable_role"],
               "resolution_rule": "enum:policy", "estimates": {"runs_per_hour": NUM, "cost_per_run_usd": NUM, "grade_seconds_per_run": NUM, "acceptance_rate": NUM}, "note": "enum:calibration_note"}
UNRESOLVABLE_ROLES = ["panel_astra"]
ENUMS = {"corpus": ["golden", "docs", "private"], "model_role": ROLES, "unresolvable_role": UNRESOLVABLE_ROLES, "policy": POLICIES, "calibration_note": [CALIBRATION_NOTE], "evidence": EVIDENCE_POLICIES, "trigger": [TRIGGER], "resolution": RESOLUTIONS, "member": ["fable", "astra"], "sampled_from": [SAMPLED_FROM],
         "T6": ["T6"], "private": ["private"], "status": ["completed", "in progress"], "check_name": CHECK_NAMES, "check_status": CHECK_STATUS,
         "embedding_model": ["bge-small-en-v1.5-q", "local-small"], "own": ["own"], "single-section": ["single-section"], "refusal": REFUSALS,
         "reason": REASON_CODES, "reject_key": REJECT_KEYS, "role": ["counted", "pilot"], "mcp_server": ["markdownattractor", "qmd", "graphify"]}
KINDS = {"questions.jsonl": QUESTIONS, "questions.candidates.jsonl": CANDIDATES, "candidates.summary.json": CAND_SUMMARY, "manifest.json": MANIFEST,
         "grades.jsonl": GRADE_ROW, "grades.adjudicated.jsonl": GRADE_ROW, "analysis.json": ANALYSIS, "analysis.adjudicated.json": ANALYSIS,
         "panel/regrade.json": REGRADE, "panel/regrade-fable.jsonl": REGRADE_MEMBER, "panel/regrade-astra.jsonl": REGRADE_MEMBER, "panel/cards.json": CARDS,
         "preflight.json": PREFLIGHT, "arms.json": ARMS_REC, "index.json": INDEX, "pilot.json": PILOT, "FROZEN.sha256": "frozen"}

RX = {"hex64": re.compile(r"^[0-9a-f]{64}$"), "hex40": re.compile(r"^[0-9a-f]{40}$"), "id": re.compile(r"^private-c\d{3}$"), "sid": re.compile(r"^[rc]\d{3}$"),
      "iso": re.compile(r"^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}:\d{2}(\.\d+)?Z?)?$|^\d{8}T\d{6}Z$"), "digit_str": re.compile(r"^\d{1,12}$"),
      "version": re.compile(r"^(mda )?\d+\.\d+\.\d+( \([0-9a-f]{7,12}\))?$|^qmd \d+\.\d+\.\d+( \([0-9a-f]{7}\))?$|^graphify \d+\.\d+\.\d+$")}


def type_ok(v, t):
    if t == "null": return v is None
    if t == "bool": return isinstance(v, bool)
    if t == "int": return isinstance(v, int) and not isinstance(v, bool)
    if t == "num": return isinstance(v, (int, float)) and not isinstance(v, bool)
    if t == "placeholder": return v == PLACEHOLDER
    if t == "arm": return v in ARMS
    if t == "tool": return v in TOOLS
    if t == "model": return v in MODELS
    if t == "frozen": return isinstance(v, str) and re.fullmatch(r"[0-9a-f]{64}\n?", v) is not None
    if t == "resolved": return isinstance(v, str) and v in MODELS and RESOLVED.match(v) is not None
    if t.startswith("enum:"): return v in ENUMS[t[5:]]
    if t in RX: return isinstance(v, str) and RX[t].match(v) is not None
    raise ValueError(f"unknown type {t}")


def validate(v, spec, path, errors):
    if isinstance(spec, tuple):
        ok = any(not probe(v, s) for s in spec)
        if not ok: errors.append(f"{path}: {describe(v)} matches none of {spec}")
        return
    if isinstance(spec, dict):
        if not isinstance(v, dict): errors.append(f"{path}: expected an object, got {describe(v)}"); return
        allowed = {k.lstrip("?"): s for k, s in spec.items()}
        required = {k for k in spec if not k.startswith("?")}
        for k in v:
            if k not in allowed: errors.append(f"{path}.{k}: key not allowed")
        for k in required:
            if k not in v: errors.append(f"{path}.{k}: required key missing")
        for k, x in v.items():
            if k in allowed: validate(x, allowed[k], f"{path}.{k}", errors)
        return
    if isinstance(spec, list):
        if not isinstance(v, list): errors.append(f"{path}: expected an array, got {describe(v)}"); return
        for i, x in enumerate(v): validate(x, spec[0], f"{path}[{i}]", errors)
        return
    if not type_ok(v, spec): errors.append(f"{path}: {describe(v)} is not {spec}")


def probe(v, spec):
    e = []
    validate(v, spec, "", e)
    return e


def describe(v):
    if isinstance(v, str): return f"a string of {len(v)} chars"
    return type(v).__name__


def read_rows(path):
    with open(path, encoding="utf-8") as fh:
        if path.endswith(".jsonl"): return [json.loads(line) for line in fh if line.strip()]
        if path.endswith(".sha256"): return [fh.read()]
        return [json.load(fh)]


def every_file(root):
    """Every entry under root, dotfiles and dot-directories included (glob would skip them); a
    symlink anywhere or a traversal error is refused (an export holds plain files only)."""
    def onerror(e): raise e
    if os.path.islink(root): raise RuntimeError(f"the export root is a symlink: {root}")   # (system ancestors such as /var → /private/var are not the export's business)
    for d, dirs, files in os.walk(root, onerror=onerror, followlinks=False):
        dirs.sort()
        for x in dirs + files:
            if os.path.islink(os.path.join(d, x)): raise RuntimeError(f"symlink in the export: {os.path.relpath(os.path.join(d, x), root)}")
        for f in sorted(files): yield os.path.join(d, f)


def check(committed):
    errors, n = [], 0
    try: paths = list(every_file(committed))
    except (RuntimeError, OSError) as e: return 0, [str(e)]
    for path in paths:
        rel = os.path.relpath(path, committed)
        if rel not in KINDS: errors.append(f"{rel}: file not allowed in a private export"); continue
        try: rows = read_rows(path)
        except Exception as e:  # noqa: BLE001
            errors.append(f"{rel}: unreadable ({e})"); continue
        for i, r in enumerate(rows): validate(r, KINDS[rel], f"{rel}[{i}]", errors)
        n += 1
    return n, errors


def h(s):
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


def digest_questions(full):
    rows = []
    for l in open(full, encoding="utf-8"):
        if not l.strip(): continue
        r = json.loads(l)
        rows.append({"id": r["id"], "split": "own", "category": "single-section", "question_sha256": h(r["q"]), "reference_sha256": h(r["reference"]),
                     "evidence_sha256": r.get("evidence_sha256") or h(r.get("evidence", "")), "required_facts_n": len(r.get("required_facts", []))})
    return rows


def dumps(r):
    return json.dumps(r, sort_keys=True, ensure_ascii=True)


def num(v):
    return v if v is None or (isinstance(v, (int, float)) and not isinstance(v, bool)) else None


def integer(v):
    return v if v is None or (isinstance(v, int) and not isinstance(v, bool)) else (int(v) if isinstance(v, float) and v.is_integer() else None)


def boolean(v):
    return v if isinstance(v, bool) else None


def model(v):
    return v if v in MODELS else None


def hexv(v, n=64):
    return v if isinstance(v, str) and re.match(r"^[0-9a-f]{%d}$" % n, v) else None


def project_pilot(p0, corpus=None):
    """The typed projection of one corpus's pilot.json (used by `export` and by the calibration record)."""
    ix0 = p0.get("index") or {}; cn = ix0.get("counts") or {}; pn = p0.get("panel") or {}
    def agree(x): return {"exact": num((x or {}).get("exact")), "within_one": num((x or {}).get("within_one"))}
    return {"corpus": corpus or (p0.get("corpus") if p0.get("corpus") in ENUMS["corpus"] else None), "at": p0.get("at"),
            "index": {"seconds": integer(ix0.get("seconds")) or 0, "counts": {k: (num if k == "total_cost_usd" else integer)(cn.get(k)) or 0 for k in INDEX["counts"]}},
            "candidates": {k: integer((p0.get("candidates") or {}).get(k)) or 0 for k in ("written", "refused", "accepted", "judged")},
            "runs": {k: ((integer((p0.get("runs") or {}).get(k)) or 0) if k in ("n", "errors", "grounding_batches", "retried") else num((p0.get("runs") or {}).get(k))) for k in ("n", "errors", "wall_s_total", "cost_usd", "grounding_batches", "retried")},
            "seconds": {k: integer((p0.get("seconds") or {}).get(k)) or 0 for k in ("runs", "grade", "panel")},
            "panel": {"sample": integer(pn.get("sample")) or 0, "incomplete": integer(pn.get("incomplete")) or 0, "agreement": {k: agree((pn.get("agreement") or {}).get(k)) for k in ("fable_astra", "fable_sonnet", "astra_sonnet")},
                      "resolved_by_panel": integer(pn.get("resolved_by_panel")) or 0, "mean_sonnet": num(pn.get("mean_sonnet")), "mean_fable": num(pn.get("mean_fable")), "mean_astra": num(pn.get("mean_astra"))}}


def export(run_dir, committed):
    def opt(*p): return os.path.join(run_dir, *p)
    def rows(*p):
        f = opt(*p)
        return read_rows(f) if os.path.isfile(f) else None
    parent = os.path.dirname(os.path.abspath(committed.rstrip("/"))) or "."
    os.makedirs(parent, exist_ok=True)
    bak = committed.rstrip("/") + ".replaced"
    for p in (committed, bak):
        if os.path.islink(p): sys.exit(f"export: {p} is a symlink: refusing")   # pass 4 F4
    if os.path.islink(parent): sys.exit(f"export: the destination's directory {parent} is a symlink: refusing")
    # an interrupted earlier replacement: the backup is the only copy → restore it; both present → refuse (pass 3 F6)
    if os.path.isdir(bak):
        if os.path.isdir(committed): sys.exit(f"export: both {committed} and {bak} exist after an interrupted replacement: resolve by hand before exporting")
        os.rename(bak, committed)
    stage = tempfile.mkdtemp(prefix=".mda-t6-priv-stage.", dir=parent)   # same filesystem: renames, never copies
    out = {}
    q = rows("questions.full.jsonl")
    if q is not None: out["questions.jsonl"] = digest_questions(opt("questions.full.jsonl"))
    c = rows("questions.candidates.jsonl")
    if c is not None:
        rws = []
        for r in c:
            d = r.get("decision") or {}; reason = d.get("reason", "")
            code = ("accepted" if d.get("accepted") else "skipped-page-taken" if reason.startswith("skipped") else "refused" if reason.startswith("refused")
                    else "member-incomplete" if reason.startswith("a member returned no") else "member-rejected" if reason.startswith("a member rejected") else "not-judged")
            rws.append({"id": r["id"], "round": integer(r.get("round")), "refused": r.get("refused") if r.get("refused") in REFUSALS else None,
                        "accepted": bool(d.get("accepted")), "fable_accepts": boolean(d.get("fable_accepts")), "astra_accepts": boolean(d.get("astra_accepts")),
                        "reason_code": code, "rejected_on": sorted(k for k in REJECT_KEYS if k in reason) if code == "member-rejected" else [],
                        "role": d.get("role") if d.get("role") in ("counted", "pilot") else None})
        out["questions.candidates.jsonl"] = rws
    cs = rows("candidates.summary.json")
    if cs is not None:
        s0 = cs[0]
        out["candidates.summary.json"] = [{"headed_sections_200": integer(s0.get("headed_sections_200")) or 0, "eligible_sections": integer(s0.get("eligible_sections")) or 0,
                                           "documents": integer(s0.get("documents")) or 0, "excluded": {k: integer((s0.get("excluded") or {}).get(k)) or 0 for k in EXCLUSIONS},
                                           "written": integer(s0.get("written")) or 0, "refused": {k: integer((s0.get("refused") or {}).get(k)) or 0 for k in REFUSALS},
                                           "judged": integer(s0.get("judged")) or 0, "accepted_counted": integer(s0.get("accepted_counted")) or 0, "accepted_pilot": integer(s0.get("accepted_pilot")) or 0}]
    m = rows("runs", "manifest.json")
    if m is not None:
        m0 = m[0]; launches = {}
        for arm, l in (m0.get("launches") or {}).items():
            if arm not in ("mda", "grep", "qmd", "graphify"): continue
            mcp = (l or {}).get("mcp") or {}; sp = (l or {}).get("system_prompt") or {}
            launches[arm] = {"with_git": bool((l or {}).get("with_git")), "mcp_server": mcp.get("server") if mcp.get("server") in ENUMS["mcp_server"] else None,
                             "system_prompt_sha256": hexv(sp.get("sha256")) or (hexv(m0.get("search_first_sha256")) if sp.get("file") else None)}
        out["manifest.json"] = [{"project": "private", "model": model(m0.get("model")), "runs": integer(m0.get("runs")), "arms": [a for a in m0.get("arms", []) if a in ARMS],
                                 "question_ids": [i for i in m0.get("question_ids", []) if RX["id"].match(str(i))], "questions_sha256": hexv(m0.get("questions_sha256")),
                                 "mda": m0.get("mda") if RX["version"].match(str(m0.get("mda", ""))) else None, "mda_sha256": hexv(m0.get("mda_sha256")),
                                 "search_first_sha256": hexv(m0.get("search_first_sha256")), "source_commit": hexv(m0.get("source_commit"), 40),
                                 "max_turns": integer(m0.get("max_turns")), "timeout_s": integer(m0.get("timeout_s")), "retries": integer(m0.get("retries")),
                                 "recorded_at": m0.get("recorded_at"), "launches": launches}]
    for src, dst in (("grades.jsonl", "grades.jsonl"), ("panel/grades.adjudicated.jsonl", "grades.adjudicated.jsonl")):
        g = rows("runs", *src.split("/"))
        if g is None: continue
        rws = []
        for r in g:
            gr = (r.get("grade") or {}).get("out") or {}; gd = r.get("grounding") or {}
            row = {"id": r["id"], "arm": r["arm"], "run": integer(r.get("run")), "error": bool(r.get("error")), "exit_code": integer(r.get("exit_code")),
                   "attempts_made": integer(r.get("attempts_made")), "timed_out": any(bool(a.get("timed_out")) for a in r.get("attempts", [])),
                   "wall_s": num(r.get("wall_s")), "turns": integer(r.get("turns")), "turns_with_usage": integer(r.get("turns_with_usage")),
                   "tool_calls": integer(r.get("tool_calls")), "failed_calls": integer(r.get("failed_calls")), "input_tokens": integer(r.get("input_tokens")),
                   "output_tokens": integer(r.get("output_tokens")), "thinking_tokens": integer(r.get("thinking_tokens")), "source_tokens": integer(r.get("source_tokens")),
                   "source_tokens_signed": integer(r.get("source_tokens_signed")), "source_tokens_negative_total": integer(r.get("source_tokens_negative_total")),
                   "source_tokens_negative_turns": integer(r.get("source_tokens_negative_turns")), "cost_usd": num(r.get("cost_usd")),
                   "cost_usd_all_attempts": num(r.get("cost_usd_all_attempts")), "answer_sha256": h(r.get("answer") or ""), "question_sha256": h(r.get("q") or ""),
                   "score": num(r.get("score")), "correctness": integer(gr.get("correctness")), "completeness": integer(gr.get("completeness")),
                   "grounded": boolean(r.get("grounded")), "graded": bool(r.get("graded")), "grader": model(r.get("grader")), "graded_at": r.get("graded_at"),
                   "models": [x for x in r.get("models", []) if x in MODELS], "tools": [t for t in r.get("tools", []) if t in TOOLS],
                   "grounding": {"cited_pages_n": len(gd.get("cited_pages") or []), "unresolved_n": len(gd.get("unresolved_citations") or []),
                                 "oversized_n": len(gd.get("oversized_pages") or []), "batches_n": len(gd.get("batches") or []),
                                 "unsupported_claims_n": len(((gd.get("out") or {}).get("unsupported_claims")) or []),
                                 "evidence_policy": gd.get("evidence_policy") if gd.get("evidence_policy") in EVIDENCE_POLICIES else None}}
            if "adjudicated" in r:
                p = r.get("panel") or {}
                row.update({"adjudicated": bool(r.get("adjudicated")), "sonnet_score": num(r.get("sonnet_score")),
                            "panel": {"fable": num(p.get("fable")), "astra": num(p.get("astra")), "triggered": boolean(p.get("triggered"))}})
            rws.append(row)
        out[dst] = rws
    for src, dst in (("analysis.json", "analysis.json"), ("analysis.adjudicated.json", "analysis.adjudicated.json")):
        a = rows("runs", src)
        if a is None: continue
        a0 = a[0]; an = a0.get("analysis", {})
        def arm_summary(x): return {k: (integer(x.get(k)) if k in ("completed", "failed", "questions", "questions_with_completed", "runs") else num(x.get(k)) if k != "arm" else x.get("arm")) for k in ARM_SUMMARY}
        def ratio(x): return None if x is None else {"median_ratio": num(x.get("median_ratio")), "questions": integer(x.get("questions")) or 0, "excluded_missing": integer(x.get("excluded_missing")) or 0, "excluded_zero_denominator": integer(x.get("excluded_zero_denominator")) or 0}
        def pair(p):
            sv = p.get("savings")
            return {"ci95": [num(x) for x in p.get("ci95", [])], "comparator": p.get("comparator"), "gates": {k: bool((p.get("gates") or {}).get(k)) for k in ("grounding_ok", "lower_bound_ok", "mean_ok", "pass")},
                    "losses": integer(p.get("losses")), "mean_delta": num(p.get("mean_delta")), "n": integer(p.get("n")), "ties": integer(p.get("ties")), "wins": integer(p.get("wins")),
                    "savings": None if sv is None else {"questions": integer(sv.get("questions")) or 0, "source_tokens": ratio(sv.get("source_tokens")), "tool_calls": ratio(sv.get("tool_calls")), "cost_usd": ratio(sv.get("cost_usd"))}}
        def qrow(x): return {k: (integer(x.get(k)) if k in ("completed", "failed", "runs") else boolean(x.get(k)) if k == "grounded" else x.get(k) if k in ("arm", "id") else num(x.get(k))) for k in QUESTION_ROW}
        opts = a0.get("options", {})
        out[dst] = [{"analysis": {"arms": [arm_summary(x) for x in an.get("arms", [])], "pairs": [pair(p) for p in an.get("pairs", [])], "questions": [qrow(x) for x in an.get("questions", [])],
                                  "draws": integer(an.get("draws")), "seed": integer(an.get("seed")), "missing_comparators": [x for x in an.get("missing_comparators", []) if x in ARMS],
                                  "other_arms": [x for x in an.get("other_arms", []) if x in ARMS]},
                     "grades": PLACEHOLDER, "manifest": PLACEHOLDER,
                     "options": {"comparators": [x for x in opts.get("comparators", []) if x in ARMS], "draws": integer(opts.get("draws")),
                                 "gates": {k: num(v) for k, v in (opts.get("gates") or {}).items() if k in ("min_lower_bound", "min_mean", "min_grounding")},
                                 "mda_arm": opts.get("mda_arm"), "seed": integer(opts.get("seed"))}}]
    rg = rows("runs", "panel", "regrade.json")
    if rg is not None:
        r0 = rg[0]
        def agree(x): return {"exact": num((x or {}).get("exact")), "within_one": num((x or {}).get("within_one"))}
        out["panel/regrade.json"] = [{"agreement": {k: agree((r0.get("agreement") or {}).get(k)) for k in ("fable_astra", "fable_sonnet", "astra_sonnet")},
                                      "incomplete": integer(r0.get("incomplete")), "mean_astra": num(r0.get("mean_astra")), "mean_fable": num(r0.get("mean_fable")),
                                      "mean_sonnet": num(r0.get("mean_sonnet")), "resolved_by_panel": integer(r0.get("resolved_by_panel")), "sample": integer(r0.get("sample")),
                                      "seed": str(r0.get("seed")), "stamp": r0.get("stamp"), "trigger": TRIGGER,
                                      "rows": [{"sample_id": x.get("sample_id"), "id": x.get("id"), "arm": x.get("arm"), "run": integer(x.get("run")), "sonnet": num(x.get("sonnet")),
                                                "fable": num(x.get("fable")), "astra": num(x.get("astra")), "panel_mean": num(x.get("panel_mean")), "triggered": boolean(x.get("triggered")),
                                                "resolved": num(x.get("resolved")), "resolution": x.get("resolution") if x.get("resolution") in RESOLUTIONS else "incomplete"} for x in r0.get("rows", [])]}]
    for member in ("fable", "astra"):
        mr = rows("runs", "panel", f"regrade-{member}.jsonl")
        if mr is not None:
            out[f"panel/regrade-{member}.jsonl"] = [{"sample_id": x.get("sample_id"), "member": member, "score": num(x.get("score")), "model": model(x.get("model")),
                                                     "cost_usd": num(x.get("cost_usd")), "complete": x.get("out") is not None} for x in mr]
    cd = rows("panel", "cards.json")
    if cd is not None:
        c0 = cd[0]
        out["panel/cards.json"] = [{"seed": str(c0.get("seed")), "stamp": c0.get("stamp"), "cards_file": PLACEHOLDER, "sampled_from": SAMPLED_FROM,
                                    "sections_missing_from_store": integer(c0.get("sections_missing_from_store")) or 0, "sample": integer(c0.get("sample")) or 0,
                                    "values": integer(c0.get("values")) or 0, "unsupported_rate": {k: num((c0.get("unsupported_rate") or {}).get(k)) for k in ("fable", "astra")},
                                    "incomplete": integer(c0.get("incomplete")) or 0, "agreement": num(c0.get("agreement")),
                                    "rows": [{"sample_id": x.get("sample_id"), "values": integer(x.get("values")),
                                              "fable": [bool(b) for b in x["fable"]] if isinstance(x.get("fable"), list) else None,
                                              "astra": [bool(b) for b in x["astra"]] if isinstance(x.get("astra"), list) else None} for x in c0.get("rows", [])]}]
    pf = rows("preflight.json")
    if pf is not None:
        p0 = pf[0]
        out["preflight.json"] = [{"table": "T6", "project": "private", "run_at": p0.get("run_at"), "status": p0.get("status") if p0.get("status") in ENUMS["status"] else "completed",
                                  "passed": bool(p0.get("passed")),
                                  "checks": [{"name": c.get("name") if c.get("name") in CHECK_NAMES else "env", "status": c.get("status") if c.get("status") in CHECK_STATUS else "FAIL"} for c in p0.get("checks", [])],
                                  "probes": {a: {"activated": integer(v.get("activated")) or 0, "of": integer(v.get("of")) or 0} for a, v in (p0.get("probes") or {}).items() if a in ("mda", "grep", "qmd", "graphify")},
                                  "store_sha256": hexv(p0.get("store_sha256")), "inventory_sha256": hexv(p0.get("inventory_sha256")), "frozen_sha256": hexv(p0.get("frozen_sha256")),
                                  "private_frozen_sha256": hexv(p0.get("private_frozen_sha256")), "source_commit": hexv(p0.get("source_commit"), 40)}]
    ar = rows("arms.json")
    if ar is not None:
        a0 = ar[0]; qm = a0.get("qmd"); gf = a0.get("graphify")
        out["arms.json"] = [{"qmd": None if not qm else {"version": qm.get("version") if RX["version"].match(str(qm.get("version", ""))) else None,
                                                          "build": {k: integer((qm.get("build") or {}).get(k)) or 0 for k in ("collection_add_s", "update_s", "embed_s", "total_s")},
                                                          "coverage": {k: (num if k == "coverage" else integer)((qm.get("coverage") or {}).get(k)) or 0 for k in ("files_indexed", "markdown_files_on_disk", "corpus_pages", "corpus_pages_indexed", "coverage")}},
                             "graphify": None if not gf else {"version": gf.get("version") if RX["version"].match(str(gf.get("version", ""))) else None,
                                                              "build": {"completed": bool((gf.get("build") or {}).get("completed")), "wall_s": integer((gf.get("build") or {}).get("wall_s")),
                                                                        "cost_usd_list_price": num((gf.get("build") or {}).get("cost_usd_list_price")), "turns": integer((gf.get("build") or {}).get("turns"))},
                                                              "graph": None if not gf.get("graph") else {"sha256": hexv(gf["graph"].get("sha256")), "nodes": integer(gf["graph"].get("nodes")) or 0, "edges": integer(gf["graph"].get("edges")) or 0},
                                                              "coverage": None if not gf.get("coverage") else {"corpus_pages": integer(gf["coverage"].get("corpus_pages")) or 0, "corpus_pages_with_a_node": integer(gf["coverage"].get("corpus_pages_with_a_node")) or 0, "coverage": num(gf["coverage"].get("coverage")) or 0}}}]
    ix = rows("index.json")
    if ix is not None:
        i0 = ix[0]; cn = i0.get("counts") or {}; emb = i0.get("embeddings") or {}; cards = i0.get("cards") or {}
        out["index.json"] = [{"seconds": integer(i0.get("seconds")) or 0, "retries": integer(i0.get("retries")) or 0, "store_sha256": hexv(i0.get("store_sha256")), "indexed_at": i0.get("indexed_at"),
                              "counts": {k: (num if k == "total_cost_usd" else integer)(cn.get(k)) or 0 for k in INDEX["counts"]},
                              "embeddings": {"embedded": integer(emb.get("embedded") if isinstance(emb, dict) else None), "model": emb.get("model") if isinstance(emb, dict) and (emb.get("model") in MODELS or emb.get("model") in ENUMS["embedding_model"]) else None},
                              "cards": {"cards": integer(cards.get("cards")) or 0, "sections": integer(cards.get("sections")) or 0, "sections_carded": integer(cards.get("sections_carded")) or 0,
                                        "complete": bool(cards.get("complete")), "truncated": integer(cards.get("truncated")) or 0,
                                        "by_model": {k: integer(v) or 0 for k, v in (cards.get("by_model") or {}).items() if k in ("claude-haiku-4-5", "none", "claude-sonnet-5", "claude-sonnet-5-5")},
                                        "summarized_from": cards.get("summarized_from"), "summarized_to": cards.get("summarized_to"),
                                        "usage": {k: (num if k.startswith("cost") else integer)((cards.get("usage") or {}).get(k)) or 0 for k in ("input_tokens", "output_tokens", "cost_usd_list_price_equivalent")}}}]
    pl = rows("pilot.json")
    if pl is not None: out["pilot.json"] = [project_pilot(pl[0], "private")]
    fz = opt("FROZEN.md")
    if os.path.isfile(fz): out["FROZEN.sha256"] = [hashlib.sha256(open(fz, "rb").read()).hexdigest() + "\n"]
    # write the staging directory, validate it, then publish atomically
    for rel, rows_ in out.items():
        p = os.path.join(stage, rel); os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8") as fh:
            if rel.endswith(".jsonl"):
                for r in rows_: fh.write(dumps(r) + "\n")
            elif rel.endswith(".sha256"): fh.write(rows_[0])
            else: fh.write(json.dumps(rows_[0], sort_keys=True, indent=1, ensure_ascii=True) + "\n")
    n, errors = check(stage)
    if errors:
        for e in errors: print(e, file=sys.stderr)
        shutil.rmtree(stage)
        sys.exit(f"export: {len(errors)} schema violation(s) in the staging export: nothing published")
    old_q = os.path.join(committed, "questions.jsonl"); new_q = os.path.join(stage, "questions.jsonl")
    if os.path.isfile(old_q) and not os.path.isfile(new_q):
        shutil.rmtree(stage)
        sys.exit("export: the committed private question digests exist but the run has no questions.full.jsonl: refusing to publish an export without them")
    if os.path.isfile(old_q) and os.path.isfile(new_q) and open(old_q, "rb").read() != open(new_q, "rb").read():
        shutil.rmtree(stage)
        sys.exit("export: the private question digests differ from the committed ones: the committed question file is frozen; nothing published")
    if os.path.isdir(committed): os.rename(committed, bak)
    try: os.rename(stage, committed)
    except OSError:
        if os.path.isdir(bak) and not os.path.isdir(committed): os.rename(bak, committed)
        raise
    if os.path.isdir(bak): shutil.rmtree(bak)
    print(f"export: {n} private file(s) published under {committed} after the schema check")


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "check":
        n, errors = check(sys.argv[2])
        for e in errors: print(e, file=sys.stderr)
        if errors: sys.exit(f"own-private-schema: {len(errors)} violation(s) in {n} file(s)")
        print(f"own-private-schema: {n} file(s) conform to the typed schema")
    elif cmd == "digest-questions":
        for r in digest_questions(sys.argv[2]): print(dumps(r))
    elif cmd == "project-pilot":
        # <pilot.json> <corpus>: the typed projection on stdout (the calibration record is built from these)
        print(json.dumps(project_pilot(read_rows(sys.argv[2])[0], sys.argv[3]), sort_keys=True))
    elif cmd == "check-file":
        # <kind> <file>: validate one file against a kind's schema (calibration, pilot.json, …)
        kind = sys.argv[2]; spec = CALIBRATION if kind == "calibration" else KINDS[kind]
        errors = []
        for i, r in enumerate(read_rows(sys.argv[3])):
            validate(r, spec, f"{os.path.basename(sys.argv[3])}[{i}]", errors)
            if kind == "calibration":
                # every model role the pilot exercised has at least one resolved id (pass 3 F13)
                for role in ROLES:
                    if not (r.get("resolved_models") or {}).get(role) and role not in (r.get("unresolved_roles") or []): errors.append(f"calibration: no resolved id for {role} and it is not declared unresolved")
                    if role in (r.get("unresolved_roles") or []) and not (r.get("requested_models") or {}).get(role): errors.append(f"calibration: unresolved role {role} without its requested alias")
                if not any((c.get("runs") or {}).get("n", 0) > 0 for c in (r.get("corpora") or {}).values()): errors.append("calibration: no corpus has pilot runs")
        for e in errors: print(e, file=sys.stderr)
        if errors: sys.exit(f"check-file: {len(errors)} violation(s)")
        print(f"check-file: {sys.argv[3]} conforms to {kind}")
    elif cmd == "export":
        export(sys.argv[2], sys.argv[3])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
