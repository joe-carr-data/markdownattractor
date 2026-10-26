#!/usr/bin/env python3
"""Allowlisted exports of the private corpus's T6 artifacts, and their check.

M9 plan (amendments after the Codex pre-mortem, F8–F13): nothing content-bearing from the
private corpus may enter the repository. Every committed private file is BUILT by `export`
from approved fields only — opaque ids, fixed arm and tool names, hex digests, numbers,
booleans, timestamps and a closed set of literal strings that also occur in the public
corpora's files of the same kind — never by deleting known keys from a full file. `check`
validates every file under the committed private directory against the same rules and is run
by `make check` (scripts/eval/tests/own-private-schema.sh); `literals` prints the literal
strings a public export of a kind contains (the schema's allowed literals are built from the
public corpora, so no private-only string can enter through the schema).

Usage:
  t6_private_export.py export <private-run-dir> <committed-dir> <schema.json>
  t6_private_export.py check <committed-dir> <schema.json>
  t6_private_export.py build-schema <public-dir>... > schema.json
"""
import glob
import json
import os
import re
import sys

HEX64 = re.compile(r"^[0-9a-f]{64}$")
HEX40 = re.compile(r"^[0-9a-f]{7,40}$")
ID = re.compile(r"^private-c\d{3}$")
SAMPLE_ID = re.compile(r"^[rc]\d{3}$")
ISO = re.compile(r"^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}:\d{2}(\.\d+)?Z?)?$|^\d{8}T\d{6}Z$")
MODEL = re.compile(r"^[a-z][a-z0-9.-]{1,40}$")  # a model alias or resolved id: no path, no space
VERSION = re.compile(r"^(mda )?[0-9][0-9A-Za-z. ()-]{0,40}$")
ARMS = {"mda", "grep", "qmd", "graphify", "graphify-haiku"}
TOOLS = {"Read", "Grep", "Glob", "Bash", "mcp__markdownattractor__mda_search", "mcp__markdownattractor__mda_open",
         "mcp__markdownattractor__mda_card", "mcp__markdownattractor__mda_timeline", "mcp__markdownattractor__mda_recent",
         "mcp__markdownattractor__mda_stale", "mcp__markdownattractor__mda_status", "mcp__qmd__query", "mcp__qmd__get",
         "mcp__qmd__multi_get", "mcp__qmd__status", "mcp__graphify__query_graph", "mcp__graphify__get_node",
         "mcp__graphify__get_neighbors", "mcp__graphify__get_community", "mcp__graphify__god_nodes", "mcp__graphify__shortest_path"}
PLACEHOLDER = "<private>"
REASON_CODES = {"accepted", "skipped-page-taken", "refused", "member-incomplete", "member-rejected", "not-judged"}
REFUSALS = {"path-in-question", "title-in-question", "span-not-in-section", "writer-no-output", "too-few-facts"}
REJECT_KEYS = {"corpus_specific", "standalone", "not_leading", "facts_stated_at_spans", "writer_facts_supported"}

# Allowed keys per file kind (recursive: a nested object's keys are listed under "<parent>.<key>").
KEYS = {
    "questions.jsonl": {"id", "split", "category", "question_sha256", "reference_sha256", "evidence_sha256", "required_facts_n"},
    "questions.candidates.jsonl": {"id", "round", "refused", "accepted", "fable_accepts", "astra_accepts", "reason_code", "rejected_on"},
    "manifest.json": {"project", "model", "runs", "arms", "question_ids", "questions_sha256", "mda", "mda_sha256", "search_first_sha256",
                      "source_commit", "max_turns", "timeout_s", "retries", "recorded_at", "launches",
                      "launches.*", "launches.*.with_git", "launches.*.mcp_server", "launches.*.system_prompt_sha256"},
    "grades.jsonl": {"id", "arm", "run", "error", "exit_code", "attempts_made", "timed_out", "wall_s", "turns", "turns_with_usage", "tool_calls",
                     "failed_calls", "input_tokens", "output_tokens", "thinking_tokens", "source_tokens", "source_tokens_signed",
                     "source_tokens_negative_total", "source_tokens_negative_turns", "cost_usd", "cost_usd_all_attempts", "answer_sha256",
                     "question_sha256", "score", "correctness", "completeness", "grounded", "graded", "grader", "graded_at", "models", "tools",
                     "grounding", "grounding.cited_pages_n", "grounding.unresolved_n", "grounding.oversized_n", "grounding.batches_n",
                     "grounding.unsupported_claims_n", "grounding.evidence_policy",
                     "adjudicated", "sonnet_score", "panel", "panel.fable", "panel.astra", "panel.triggered"},
    "analysis.json": {"analysis", "grades", "manifest", "options",
                      "analysis.arms", "analysis.arms.*", "analysis.draws", "analysis.missing_comparators", "analysis.other_arms", "analysis.pairs",
                      "analysis.pairs.*", "analysis.questions", "analysis.questions.*", "analysis.seed",
                      "options.comparators", "options.draws", "options.gates", "options.gates.*", "options.mda_arm", "options.seed"},
    "regrade.json": {"agreement", "agreement.*", "agreement.*.*", "incomplete", "mean_astra", "mean_fable", "mean_sonnet", "resolved_by_panel", "rows",
                     "rows.*", "sample", "seed", "stamp", "trigger"},
    "regrade-member.jsonl": {"sample_id", "member", "score", "model", "cost_usd", "complete"},
    "cards.json": {"seed", "stamp", "cards_file", "sampled_from", "sections_missing_from_store", "sample", "values", "unsupported_rate",
                   "unsupported_rate.*", "incomplete", "agreement", "rows", "rows.*"},
    "preflight.json": {"table", "project", "run_at", "status", "passed", "checks", "checks.*", "probes", "probes.*", "probes.*.*",
                       "store_sha256", "frozen_sha256", "private_frozen_sha256"},
    "arms.json": {"qmd", "qmd.*", "qmd.*.*", "graphify", "graphify.*", "graphify.*.*"},
    "index.json": {"seconds", "counts", "counts.*", "store_sha256", "cards", "cards.*", "cards.*.*", "retries", "embeddings", "embeddings.*"},
    "pilot.json": {"*", "*.*", "*.*.*"},
    "FROZEN.sha256": set(),
}
# keys whose nested objects are free-form numeric summaries (numbers, bools, null, allowed strings)
FREEFORM = {"analysis.arms.*", "analysis.pairs.*", "analysis.questions.*", "options.gates.*", "rows.*", "checks.*", "probes.*.*",
            "qmd.*.*", "graphify.*.*", "cards.*.*", "counts.*", "embeddings.*", "unsupported_rate.*", "agreement.*.*", "launches.*.*",
            "*", "*.*", "*.*.*"}


def kind_of(path):
    b = os.path.basename(path)
    if b in KEYS:
        return b
    if b.startswith("regrade-") and b.endswith(".jsonl"):
        return "regrade-member.jsonl"
    if b in ("grades.adjudicated.jsonl",):
        return "grades.jsonl"
    if b in ("analysis.adjudicated.json",):
        return "analysis.json"
    return None


def string_ok(s, literals):
    return (s == PLACEHOLDER or HEX64.match(s) or ID.match(s) or SAMPLE_ID.match(s) or ISO.match(s) or s in ARMS or s in TOOLS
            or s in literals or s in REASON_CODES or s in REFUSALS or s in REJECT_KEYS or MODEL.match(s) or VERSION.match(s)
            or s in ("fable", "astra", "sonnet", "private", "own", "single-section", "T6", "completed", "ok", "FAIL", "skipped"))


def walk(v, prefix, kind, literals, errors, path):
    """Recursively validate keys and strings; prefix is the dotted key path with `*` for list items."""
    allowed = KEYS[kind]
    if isinstance(v, dict):
        for k, x in v.items():
            kp = f"{prefix}.{k}" if prefix else k
            free = any(prefix.startswith(f) or prefix == f for f in FREEFORM) if prefix else False
            wild = f"{prefix}.*" if prefix else "*"
            if kp not in allowed and wild not in allowed and not free:
                errors.append(f"{path}: key not allowed: {kp}")
                continue
            if not re.match(r"^[A-Za-z0-9_.-]{1,64}$", k) and k not in ARMS:
                errors.append(f"{path}: key form not allowed: {k!r}")
            walk(x, kp if kp in allowed or free or kp.endswith("*") else wild, kind, literals, errors, path)
    elif isinstance(v, list):
        for x in v:
            walk(x, f"{prefix}.*" if prefix else "*", kind, literals, errors, path)
    elif isinstance(v, str):
        if not string_ok(v, literals):
            errors.append(f"{path}: string value not allowed at {prefix}: {v[:12]!r}… ({len(v)} chars)")
    elif v is None or isinstance(v, (bool, int, float)):
        pass
    else:
        errors.append(f"{path}: value type not allowed at {prefix}: {type(v).__name__}")


def read_rows(path):
    if path.endswith(".jsonl"):
        with open(path, encoding="utf-8") as fh:
            return [json.loads(line) for line in fh if line.strip()]
    with open(path, encoding="utf-8") as fh:
        return [json.load(fh)]


def check(committed, schema_path):
    schema = json.load(open(schema_path, encoding="utf-8"))
    literals = set(schema.get("literals", []))
    errors, n = [], 0
    for path in sorted(glob.glob(os.path.join(committed, "**", "*"), recursive=True)):
        if os.path.isdir(path):
            continue
        rel = os.path.relpath(path, committed)
        if rel == "schema.json":
            continue
        kind = kind_of(path)
        if kind is None:
            errors.append(f"{rel}: file kind not allowed")
            continue
        if kind == "FROZEN.sha256":
            text = open(path, encoding="utf-8").read().strip()
            if not re.match(r"^[0-9a-f]{64}(  [A-Za-z0-9._/-]+)?$", text):
                errors.append(f"{rel}: not a bare sha256 line")
            n += 1
            continue
        try:
            rows = read_rows(path)
        except Exception as e:  # noqa: BLE001
            errors.append(f"{rel}: not JSON ({e})")
            continue
        for r in rows:
            walk(r, "", kind, literals, errors, rel)
        n += 1
    return n, errors


def literals_of(public_dir):
    """Every string value in a public corpus's committed T6 files (the allowed literal set)."""
    out = set()

    def grab(v):
        if isinstance(v, dict):
            for x in v.values():
                grab(x)
        elif isinstance(v, list):
            for x in v:
                grab(x)
        elif isinstance(v, str) and 0 < len(v) <= 200 and "/" not in v and "\n" not in v:
            out.add(v)

    for path in glob.glob(os.path.join(public_dir, "**", "*.json*"), recursive=True):
        b = os.path.basename(path)
        if b in ("questions.jsonl", "questions.candidates.jsonl", "questions.pilot.jsonl") or b.startswith("cards-"):
            continue  # question texts and card texts are not literals a private export may carry
        try:
            for r in read_rows(path):
                if isinstance(r, dict):
                    r = {k: v for k, v in r.items() if k not in ("grades", "manifest", "q", "answer", "trace", "stderr", "why", "note", "reference", "evidence")}
                grab(r)
        except Exception:  # noqa: BLE001
            continue
    return sorted(out)


def h(s):
    import hashlib
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


def export(run_dir, committed, schema_path):
    """Build the committed private files from the private run directory (approved fields only)."""
    os.makedirs(committed, exist_ok=True)
    literals = set(json.load(open(schema_path, encoding="utf-8")).get("literals", []))

    def write_rows(name, rows):
        p = os.path.join(committed, name)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8") as fh:
            if name.endswith(".jsonl"):
                for r in rows:
                    fh.write(json.dumps(r, sort_keys=True) + "\n")
            else:
                fh.write(json.dumps(rows[0], sort_keys=True, indent=1) + "\n")

    def num(v):
        return v if v is None or isinstance(v, (bool, int, float)) else None

    def lit(v):
        return v if isinstance(v, str) and string_ok(v, literals) else None

    def opt(path):
        return os.path.join(run_dir, path)

    # questions
    if os.path.isfile(opt("questions.jsonl")):
        write_rows("questions.jsonl", [{"id": r["id"], "split": "own", "category": "single-section", "question_sha256": h(r["q"]),
                                        "reference_sha256": h(r["reference"]), "evidence_sha256": r.get("evidence_sha256") or h(r.get("evidence", "")),
                                        "required_facts_n": len(r.get("required_facts", []))} for r in read_rows(opt("questions.jsonl"))])
    if os.path.isfile(opt("questions.candidates.jsonl")):
        rows = []
        for r in read_rows(opt("questions.candidates.jsonl")):
            d = r.get("decision") or {}
            reason = d.get("reason", "")
            if d.get("accepted"):
                code = "accepted"
            elif reason.startswith("skipped"):
                code = "skipped-page-taken"
            elif reason.startswith("refused"):
                code = "refused"
            elif reason.startswith("a member returned no"):
                code = "member-incomplete"
            elif reason.startswith("a member rejected"):
                code = "member-rejected"
            else:
                code = "not-judged"
            rejected_on = sorted(k for k in REJECT_KEYS if k in reason) if code == "member-rejected" else []
            rows.append({"id": r["id"], "round": num(r.get("round")), "refused": r.get("refused") if r.get("refused") in REFUSALS else (None if not r.get("refused") else "refused"),
                         "accepted": bool(d.get("accepted")), "fable_accepts": d.get("fable_accepts"), "astra_accepts": d.get("astra_accepts"),
                         "reason_code": code, "rejected_on": rejected_on})
        write_rows("questions.candidates.jsonl", rows)
    # manifest
    if os.path.isfile(opt("runs/manifest.json")):
        m = read_rows(opt("runs/manifest.json"))[0]
        launches = {}
        for arm, l in (m.get("launches") or {}).items():
            mcp = (l or {}).get("mcp") or {}
            sp = (l or {}).get("system_prompt") or {}
            launches[arm] = {"with_git": bool((l or {}).get("with_git")), "mcp_server": lit(mcp.get("server")),
                             "system_prompt_sha256": sp.get("sha256") if HEX64.match(str(sp.get("sha256", ""))) else (m.get("search_first_sha256") if sp.get("file") else None)}
        write_rows("manifest.json", [{"project": "private", "model": lit(m.get("model")), "runs": num(m.get("runs")), "arms": [a for a in m.get("arms", []) if a in ARMS],
                                      "question_ids": [i for i in m.get("question_ids", []) if ID.match(i)], "questions_sha256": m.get("questions_sha256"),
                                      "mda": lit(m.get("mda")), "mda_sha256": m.get("mda_sha256"), "search_first_sha256": m.get("search_first_sha256"),
                                      "source_commit": m.get("source_commit"), "max_turns": num(m.get("max_turns")), "timeout_s": num(m.get("timeout_s")),
                                      "retries": num(m.get("retries")), "recorded_at": m.get("recorded_at"), "launches": launches}])
    # grades (original and adjudicated)
    for src, dst in (("runs/grades.jsonl", "grades.jsonl"), ("runs/panel/grades.adjudicated.jsonl", "grades.adjudicated.jsonl")):
        if not os.path.isfile(opt(src)):
            continue
        rows = []
        for r in read_rows(opt(src)):
            g = r.get("grounding") or {}
            gr = (r.get("grade") or {}).get("out") or {}
            row = {"id": r["id"], "arm": r["arm"], "run": num(r.get("run")), "error": bool(r.get("error")), "exit_code": num(r.get("exit_code")),
                   "attempts_made": num(r.get("attempts_made")), "timed_out": any(a.get("timed_out") for a in r.get("attempts", [])),
                   "wall_s": num(r.get("wall_s")), "turns": num(r.get("turns")), "turns_with_usage": num(r.get("turns_with_usage")),
                   "tool_calls": num(r.get("tool_calls")), "failed_calls": num(r.get("failed_calls")), "input_tokens": num(r.get("input_tokens")),
                   "output_tokens": num(r.get("output_tokens")), "thinking_tokens": num(r.get("thinking_tokens")), "source_tokens": num(r.get("source_tokens")),
                   "source_tokens_signed": num(r.get("source_tokens_signed")), "source_tokens_negative_total": num(r.get("source_tokens_negative_total")),
                   "source_tokens_negative_turns": num(r.get("source_tokens_negative_turns")), "cost_usd": num(r.get("cost_usd")),
                   "cost_usd_all_attempts": num(r.get("cost_usd_all_attempts")), "answer_sha256": h(r.get("answer", "")), "question_sha256": h(r.get("q", "")),
                   "score": num(r.get("score")), "correctness": num(gr.get("correctness")), "completeness": num(gr.get("completeness")),
                   "grounded": r.get("grounded") if isinstance(r.get("grounded"), bool) else None, "graded": bool(r.get("graded")),
                   "grader": lit(r.get("grader")), "graded_at": r.get("graded_at"), "models": [x for x in r.get("models", []) if MODEL.match(str(x))],
                   "tools": [t for t in r.get("tools", []) if t in TOOLS],
                   "grounding": {"cited_pages_n": len(g.get("cited_pages") or []), "unresolved_n": len(g.get("unresolved_citations") or []),
                                 "oversized_n": len(g.get("oversized_pages") or []), "batches_n": len(g.get("batches") or []),
                                 "unsupported_claims_n": len(((g.get("out") or {}).get("unsupported_claims")) or []),
                                 "evidence_policy": lit(g.get("evidence_policy"))}}
            if "adjudicated" in r:
                row["adjudicated"] = bool(r.get("adjudicated"))
                row["sonnet_score"] = num(r.get("sonnet_score"))
                p = r.get("panel") or {}
                row["panel"] = {"fable": num(p.get("fable")), "astra": num(p.get("astra")), "triggered": p.get("triggered") if isinstance(p.get("triggered"), bool) else None}
            rows.append(row)
        write_rows(dst, rows)
    # analyses
    for src, dst in (("runs/analysis.json", "analysis.json"), ("runs/analysis.adjudicated.json", "analysis.adjudicated.json")):
        if os.path.isfile(opt(src)):
            a = read_rows(opt(src))[0]
            a["grades"] = PLACEHOLDER
            a["manifest"] = PLACEHOLDER
            write_rows(dst, [a])
    # panel
    if os.path.isfile(opt("runs/panel/regrade.json")):
        rg = read_rows(opt("runs/panel/regrade.json"))[0]
        rg["rows"] = [{k: v for k, v in row.items() if k in ("sample_id", "id", "arm", "run", "sonnet", "fable", "astra", "panel_mean", "triggered", "resolved", "resolution")} for row in rg.get("rows", [])]
        write_rows("panel/regrade.json", [rg])
    for member in ("fable", "astra"):
        src = opt(f"runs/panel/regrade-{member}.jsonl")
        if os.path.isfile(src):
            write_rows(f"panel/regrade-{member}.jsonl", [{"sample_id": r.get("sample_id"), "member": member, "score": num(r.get("score")), "model": lit(r.get("model")),
                                                           "cost_usd": num(r.get("cost_usd")), "complete": r.get("out") is not None} for r in read_rows(src)])
    if os.path.isfile(opt("panel/cards.json")):
        c = read_rows(opt("panel/cards.json"))[0]
        c["cards_file"] = PLACEHOLDER
        c["rows"] = [{"sample_id": r.get("sample_id"), "values": num(r.get("values")), "fable": r.get("fable"), "astra": r.get("astra")} for r in c.get("rows", [])]
        write_rows("panel/cards.json", [c])
    # preflight, arms, index, pilot
    if os.path.isfile(opt("preflight.json")):
        p = read_rows(opt("preflight.json"))[0]
        write_rows("preflight.json", [{"table": "T6", "project": "private", "run_at": p.get("run_at"), "status": lit(p.get("status")), "passed": bool(p.get("passed")),
                                       "checks": [{"name": lit(c.get("name")) or "check", "status": lit(c.get("status")) or "FAIL"} for c in p.get("checks", [])],
                                       "probes": {a: {"activated": num(v.get("activated")), "of": num(v.get("of"))} for a, v in (p.get("probes") or {}).items() if a in ARMS},
                                       "store_sha256": p.get("store_sha256"), "frozen_sha256": p.get("frozen_sha256"), "private_frozen_sha256": p.get("private_frozen_sha256")}])
    if os.path.isfile(opt("arms.json")):
        write_rows("arms.json", [read_rows(opt("arms.json"))[0]])
    if os.path.isfile(opt("index.json")):
        write_rows("index.json", [read_rows(opt("index.json"))[0]])
    if os.path.isfile(opt("pilot.json")):
        write_rows("pilot.json", [read_rows(opt("pilot.json"))[0]])
    if os.path.isfile(opt("FROZEN.md")):
        import hashlib
        with open(os.path.join(committed, "FROZEN.sha256"), "w", encoding="utf-8") as fh:
            fh.write(hashlib.sha256(open(opt("FROZEN.md"), "rb").read()).hexdigest() + "\n")
    n, errors = check(committed, schema_path)
    if errors:
        for e in errors:
            print(e, file=sys.stderr)
        sys.exit(f"export: {len(errors)} schema violation(s) in {committed}: nothing may be committed")
    print(f"export: {n} private file(s) written under {committed}, schema check passed")


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "check":
        n, errors = check(sys.argv[2], sys.argv[3])
        for e in errors:
            print(e, file=sys.stderr)
        if errors:
            sys.exit(f"own-private-schema: {len(errors)} violation(s) in {n} file(s)")
        print(f"own-private-schema: {n} file(s) conform")
    elif cmd == "build-schema":
        lits = set()
        for d in sys.argv[2:]:
            lits.update(literals_of(d))
        print(json.dumps({"literals": sorted(lits), "note": "allowed literal strings of a private export: every string value of the public corpora's committed T6 files (question and card texts excluded); built by scripts/eval/t6_private_export.py build-schema"}, indent=1))
    elif cmd == "export":
        export(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
