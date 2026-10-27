#!/usr/bin/env python3
"""Small, testable helpers of the T6 harness that bash gets wrong at the edges (Codex pass 3).

Usage:
  t6_scan.py cand-hash <candidate.json-line>        # the identity hash of a question candidate (one function for every caller)
  t6_scan.py cand-hash-file <candidates.jsonl>      # id<TAB>hash per line
  t6_scan.py is-binary <file>                       # exit 0 when the WHOLE file holds a NUL byte (or cannot be read: exit 2)
  t6_scan.py collide <items.txt> <baseline.txt>     # the items (one per line) that occur, case-insensitively, anywhere in the baseline — every item tested
  t6_scan.py scan <denylist.txt> <text>             # how many denylist entries occur in the text (one pass: 40-char excerpts by rolling window, the rest by substring); prints the count
  t6_scan.py tree-manifest <dir> [exclude-prefix...] # "type<TAB>path<TAB>digest-or-link-target" per entry under dir (files and symlinks; a read error is fatal)
"""
import hashlib
import json
import os
import sys


def cand_hash(c: dict) -> str:
    """sha256 over the compact, key-sorted, UTF-8 JSON of [q, section, required_facts, page, heading, section_id]."""
    payload = [c.get("q") or "", c["section"], c.get("required_facts") or [], c["page"], c["heading"], c["section_id"]]
    return hashlib.sha256(json.dumps(payload, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8")).hexdigest()


def is_binary(path: str) -> int:
    try:
        with open(path, "rb") as fh:
            while True:
                chunk = fh.read(1 << 20)
                if not chunk:
                    return 1
                if b"\0" in chunk:
                    return 0
    except OSError:
        return 2


def collide(items_f: str, baseline_f: str):
    with open(baseline_f, "rb") as fh:
        base = fh.read().decode("utf-8", errors="replace").lower()
    with open(items_f, encoding="utf-8") as fh:
        for line in fh:
            item = line.rstrip("\n")
            if item and item.lower() in base:
                print(item)


def scan(denylist_f: str, text_f: str) -> int:
    """How many denylist entries occur (case-insensitively) in the text: one pass. Entries of exactly
    40 characters (the excerpts) are looked up through a rolling 40-character window; every other
    entry (paths, names, headings) is a substring test. Prints the count; the entries themselves
    stay private."""
    with open(text_f, "rb") as fh:
        text = fh.read().decode("utf-8", errors="replace").lower()
    excerpts, others = set(), []
    with open(denylist_f, encoding="utf-8") as fh:
        for line in fh:
            e = line.rstrip("\n")
            if not e:
                continue
            (excerpts.add if len(e) == 40 else others.append)(e.lower())
    hits = 0
    if excerpts:
        seen = set()
        for i in range(0, max(0, len(text) - 39)):
            w = text[i:i + 40]
            if w in excerpts and w not in seen:
                seen.add(w); hits += 1
    listing = os.environ.get("T6_SCAN_LIST") == "1"   # debugging aid for the owner's terminal only: prints the matching entries
    for e in others:
        if e in text:
            hits += 1
            if listing: print("HIT", len(e), e[:60], file=sys.stderr)
    print(hits)
    return hits


def tree_manifest(root: str, excludes):
    root = os.path.abspath(root)
    rows = []
    def onerror(e): raise e
    for d, dirs, files in os.walk(root, onerror=onerror, followlinks=False):
        dirs.sort()
        for name in sorted(dirs + files):
            p = os.path.join(d, name)
            rel = os.path.relpath(p, root)
            if any(rel == ex or rel.startswith(ex.rstrip("/") + "/") for ex in excludes):
                if name in dirs: dirs.remove(name)
                continue
            if os.path.islink(p):
                rows.append(("link", rel, os.readlink(p)))
            elif os.path.isfile(p):
                h = hashlib.sha256()
                with open(p, "rb") as fh:
                    for chunk in iter(lambda: fh.read(1 << 20), b""):
                        h.update(chunk)
                rows.append(("file", rel, h.hexdigest()))
    for t, rel, v in sorted(rows, key=lambda r: r[1]):
        print(f"{t}\t{rel}\t{v}")


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "cand-hash":
        print(cand_hash(json.loads(sys.argv[2])))
    elif cmd == "cand-hash-file":
        with open(sys.argv[2], encoding="utf-8") as fh:
            for line in fh:
                if line.strip():
                    c = json.loads(line); print(f"{c['id']}\t{cand_hash(c)}")
    elif cmd == "is-binary":
        sys.exit(is_binary(sys.argv[2]))
    elif cmd == "collide":
        collide(sys.argv[2], sys.argv[3])
    elif cmd == "scan":
        scan(sys.argv[2], sys.argv[3])
    elif cmd == "tree-manifest":
        tree_manifest(sys.argv[2], sys.argv[3:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
