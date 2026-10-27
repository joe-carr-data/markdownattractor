#!/usr/bin/env python3
"""The T6 page's companion tables, rendered from the committed artifacts (M9, D1): nothing retyped.

Usage:
  scripts/eval/t6_report.py [evals/results/own/T6]        # markdown to stdout

`scripts/eval/t6.sh table` (hashed in the T6 freeze) renders the consumption, quality and gate
tables. This file renders what the T2 page carried beside them and what T6's own reading needs:
the corpora as built (files, sections, cards, index cost, candidates, acceptance, the arms'
coverage), the run totals, the adjudicated pairs, the panel calibration, the card audit, and the
grounding verdicts split by their recorded cause. It is deliberately outside the freeze — a
renderer added after the runs; it reads only committed files (the private corpus: the allowlisted
exports, which carry counts and no text), and its output's sha256 is published in runbook §7.6.
"""
import json
import os
import re
import sys

ARMS = ["grep", "mda", "qmd", "graphify"]
EXT = re.compile(r"^\.(md|mdx|markdown)$")


def load(p):
    return json.load(open(p, encoding="utf-8")) if os.path.isfile(p) else None


def rows(p):
    return [json.loads(l) for l in open(p, encoding="utf-8") if l.strip()] if os.path.isfile(p) else []


def fmt(x, d=2):
    if x is None:
        return "—"
    if isinstance(x, bool):
        return "yes" if x else "no"
    if isinstance(x, float):
        return f"{x:.{d}f}"
    return str(x)


def pct(x):
    return "—" if x is None else f"{100 * x:.1f}%"


def table(header, body):
    out = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    out += ["| " + " | ".join(r) + " |" for r in body]
    return "\n".join(out)


def corpora(T, own, cs):
    body = []
    for c in cs:
        d = os.path.join(T, c)
        idx = load(os.path.join(d, "index.json")) or {}
        cs_ = load(os.path.join(d, "candidates.summary.json")) or {}
        if c == "private":
            arms = load(os.path.join(d, "arms.json")) or {}
            q, g = arms.get("qmd") or {}, arms.get("graphify") or {}
            pages = (q.get("coverage") or {}).get("corpus_pages")
        else:
            q = load(os.path.join(own, "arms", f"qmd-{c}.json")) or {}
            g = load(os.path.join(own, "arms", f"graphify-{c}.json")) or {}
            pages = sum(1 for l in open(os.path.join(d, "pages.txt"), encoding="utf-8") if l.strip()) if os.path.isfile(os.path.join(d, "pages.txt")) else None
        cnt, cards = idx.get("counts") or {}, idx.get("cards") or {}
        qc, gc, gb = q.get("coverage") or {}, g.get("coverage") or {}, g.get("build") or {}
        gq = (g.get("graph") or {})
        body.append([
            c, fmt(pages), fmt(cnt.get("docs")), fmt(cnt.get("sections")),
            f"{fmt(cards.get('cards'))} ({'complete' if cards.get('complete') else 'incomplete'})",
            fmt(idx.get("seconds")), fmt(cnt.get("total_cost_usd"), 2),
            f"{fmt(cs_.get('eligible_sections'))} ({fmt(cs_.get('documents'))})",
            f"{fmt(cs_.get('written'))} / {fmt(sum((cs_.get('refused') or {}).values()) if cs_.get('refused') else None)} / {fmt(cs_.get('judged'))}",
            f"{fmt(cs_.get('accepted_counted'))} + {fmt(cs_.get('accepted_pilot'))}",
            f"{fmt(qc.get('corpus_pages_indexed'))} of {fmt(qc.get('corpus_pages'))}",
            (f"{'completed' if gb.get('completed') else 'did not complete'}, {fmt(gb.get('wall_s'))} s, ${fmt(gb.get('cost_usd_list_price'), 2)}, {fmt(gq.get('nodes'))} nodes / {fmt(gq.get('edges'))} edges, {fmt(gc.get('corpus_pages_with_a_node'))} of {fmt(gc.get('corpus_pages'))} pages with a node") if g else "—",
        ])
    return table(["corpus", "pages (inventory)", "documents indexed by mda", "sections", "cards", "index wall s", "index cost $ (Haiku, list price)",
                  "eligible sections (documents)", "candidates written / writer refusals / panel-judged", "accepted counted + pilot", "qmd pages indexed", "graphify build"], body)


def totals(T, cs):
    body = []
    for c in cs:
        g = rows(os.path.join(T, c, "grades.jsonl"))
        if not g:
            continue
        ats = sorted(a.get("at") for r in g for a in (r.get("attempts") or []) if a.get("at"))
        body.append([c, str(len(g)), str(sum(1 for r in g if r.get("error"))), str(sum(1 for r in g if (r.get("attempts_made") or 1) > 1)),
                     f"{sum(r.get('cost_usd') or 0 for r in g):.2f}", f"{sum(r.get('cost_usd_all_attempts') or r.get('cost_usd') or 0 for r in g):.2f}",
                     str(int(round(sum(r.get("wall_s") or 0 for r in g)))),
                     (f"{ats[0]} → {ats[-1]}" if ats else "in the run directory")])
    return table(["corpus", "rows", "failed", "retried", "cost $ (final attempts)", "cost $ (all attempts)", "wall s (sum of rows)", "first → last attempt (UTC)"], body)


def adjudicated(T, cs):
    out = []
    for c in cs:
        a = load(os.path.join(T, c, "analysis.adjudicated.json"))
        if not a:
            continue
        body = []
        for p in a["analysis"]["pairs"]:
            g = p["gates"]
            body.append([f"mda vs {p['comparator']}", str(p["n"]), f"{p['mean_delta']:+.2f}", f"[{p['ci95'][0]:+.2f}, {p['ci95'][1]:+.2f}]",
                         f"{p['wins']}/{p['losses']}/{p['ties']}", "pass" if g["lower_bound_ok"] else "FAIL", "pass" if g["mean_ok"] else "FAIL",
                         "pass" if g["grounding_ok"] else "FAIL", "none claimed" if not p.get("savings") else "see analysis"])
        arms = " · ".join(f"{x['arm']} mean {x['mean_score']:.2f}, grounding {x['grounding_rate']:.2f}" for x in a["analysis"]["arms"])
        out.append(f"*{c}* (adjudicated) — per arm: {arms}\n\n" + table(["pair", "n", "mean Δ", "95% paired", "wins/losses/ties", "gate a (lb ≥ −0.25)", "gate b (mean ≥ 4.0)", "gate c (grounding ≥ 95%)", "savings"], body))
    return "\n\n".join(out)


def panel(T, cs):
    body = []
    for c in cs:
        r = load(os.path.join(T, c, "panel", "regrade.json"))
        if not r:
            continue
        ag = r["agreement"]
        body.append([c, str(r["sample"]), str(r["incomplete"]), fmt(ag["fable_astra"]["exact"]), fmt(ag["fable_astra"]["within_one"]),
                     fmt(ag["fable_sonnet"]["exact"]), fmt(ag["astra_sonnet"]["exact"]), str(r["resolved_by_panel"]),
                     f"{fmt(r['mean_sonnet'])} / {fmt(r['mean_fable'])} / {fmt(r['mean_astra'])}"])
    return table(["corpus", "answers", "incomplete", "Fable/Astra exact", "within one", "Fable/Sonnet exact", "Astra/Sonnet exact", "trigger fired", "mean Sonnet / Fable / Astra"], body)


def cards(T, cs):
    body = []
    for c in cs:
        r = load(os.path.join(T, c, "panel", "cards.json"))
        if not r:
            continue
        u = r.get("unsupported_rate") or {}
        body.append([c, str(r["sample"]), str(r["values"]), fmt(u.get("fable"), 3), fmt(u.get("astra"), 3), fmt(r.get("agreement"), 3), str(r["incomplete"]), str(r.get("sections_missing_from_store"))])
    return table(["corpus", "cards", "values", "unsupported (Fable)", "unsupported (Astra)", "per-value agreement", "incomplete verdicts", "sections missing from the store"], body)


def grounding(T, cs):
    """Ungrounded rows by their recorded cause, exclusive categories: (U) unresolved citations and no
    unsupported claim; (C) at least one unsupported claim; (N) neither recorded (the grader found no
    resolvable citation). Public corpora carry the unresolved tokens, so U is split further: every
    token a bare extension (`.md`, `.mdx`, `.markdown` mentioned as a file type), every token a bare
    file name without a slash (a file named, not cited by path), other. The last column is
    descriptive, not a gate: the grounding rate if every U row were counted grounded."""
    body = []
    for c in cs:
        g = rows(os.path.join(T, c, "grades.jsonl"))
        if not g:
            continue
        for arm in ARMS:
            rs = [r for r in g if r["arm"] == arm]
            if not rs:
                continue
            ok = [r for r in rs if r.get("grounded") is True]
            bad = [r for r in rs if r.get("grounded") is not True]
            U, C, N, ext, base = [], [], [], 0, 0
            for r in bad:
                gd = r.get("grounding") or {}
                if "unresolved_n" in gd:   # the private export: counts only
                    un, uc = gd.get("unresolved_n") or 0, gd.get("unsupported_claims_n") or 0
                    toks = None
                else:
                    toks = gd.get("unresolved_citations") or []
                    un, uc = len(toks), len(((gd.get("out") or {}).get("unsupported_claims")) or [])
                if uc > 0:
                    C.append(r)
                elif un > 0:
                    U.append(r)
                    if toks is not None:
                        if all(EXT.match(t) for t in toks):
                            ext += 1
                        elif all("/" not in t for t in toks):
                            base += 1
                else:
                    N.append(r)
            split = f"{ext} / {base} / {len(U) - ext - base}" if c != "private" else "n/a (texts not exported)"
            body.append([c, arm, str(len(rs)), str(len(ok)), pct(len(ok) / len(rs)), str(len(U)), split, str(len(C)), str(len(N)), pct((len(ok) + len(U)) / len(rs))])
    return table(["corpus", "arm", "rows", "grounded", "grounding", "U: unresolved citation(s) only", "U split: bare extension / bare file name / other",
                  "C: unsupported claim(s)", "N: no resolvable citation", "grounding if U were grounded (descriptive, not the gate)"], body)


def main():
    T = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join("evals", "results", "own", "T6"))
    own = os.path.dirname(T)
    pub = [c for c in ("golden", "docs") if os.path.isfile(os.path.join(T, c, "grades.jsonl"))]
    priv = ["private"] if os.path.isfile(os.path.join(T, "private", "grades.jsonl")) else []
    PRIV = " — the private corpus (supporting evidence only; exported fields, exempt from independent reproduction)"
    sections = [
        ("the corpora as built", "(the inventory the resolver uses; mda's store from `index.json`, cards counted per distinct section hash, so identical sections share one; the writer's candidates and the panel's acceptance from `candidates.summary.json`; the arms' coverage from their build records)", corpora, (own,)),
        ("run totals", "(from the committed grade rows; a retried row carries its final attempt's metrics and every attempt's cost; the private rows carry no timestamps)", totals, ()),
        ("the same pairs on the adjudicated grades", "(panel-resolved scores in place for the 30 sampled answers per corpus, rule 0.8)", adjudicated, ()),
        ("panel calibration", "(30 answers per corpus, Fable and Astra, blind; trigger: either member differs from the Sonnet grade by more than one point → the panel mean, in the adjudicated grades only)", panel, ()),
        ("card audit", "(up to 100 cards per corpus with at least one date or entity, seeded; each value judged against the section text by both members; an incomplete verdict is counted, not scored)", cards, ()),
        ("grounding verdicts by recorded cause", "(one row per arm; U, C and N are exclusive and sum to the ungrounded rows; U is split only where the unresolved tokens are committed)", grounding, ()),
    ]
    for title, note, fn, extra in sections:
        if pub:
            print(f"**T6 — {title}, public corpora** {note}\n")
            print((fn(T, *extra, pub) if extra else fn(T, pub)) + "\n")
        if priv:
            print(f"**T6 — {title}{PRIV}** {note}\n")
            print((fn(T, *extra, priv) if extra else fn(T, priv)) + "\n")
    print("Generated by `scripts/eval/t6_report.py` from `evals/results/own/T6/<corpus>/{index.json,candidates.summary.json,pages.txt,grades.jsonl,analysis.adjudicated.json,panel/regrade.json,panel/cards.json}` and `evals/results/own/arms/{qmd,graphify}-<corpus>.json` (the private corpus: the allowlisted exports under `T6/private/`, `arms.json` included).")


if __name__ == "__main__":
    main()
