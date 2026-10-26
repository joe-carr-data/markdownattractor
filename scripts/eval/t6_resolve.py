#!/usr/bin/env python3
"""The T6 citation resolver (M9 plan, Codex pre-mortem F6).

Usage: t6_resolve.py <corpus-copy> <pages.txt> <project> <cited-path>

Prints the repository-relative page a citation resolves to, or nothing. A citation resolves
only when its canonical path (realpath) is inside the corpus copy -- no traversal, no symlink
escape, no store or arm artifact -- AND the relative result is a member of the frozen page
inventory (pages.txt, one page per line). A leading `<project>/` (qmd's collection prefix)
is stripped as a second candidate. A bare suffix resolves only when exactly one inventory
page ends with it. Called by scripts/eval/t2.sh under T2_PAGES; tested by
scripts/eval/tests/t6-resolver.sh.
"""
import os
import sys


def main() -> None:
    corpus, pages_f, project, cited = sys.argv[1:5]
    root = os.path.realpath(corpus)
    with open(pages_f, encoding="utf-8") as fh:
        pages = {line.rstrip("\n") for line in fh if line.strip()}

    def inside(rel: str):
        if not rel or rel.startswith("/"):
            return None
        rp = os.path.realpath(os.path.join(root, rel))
        if rp != root and not rp.startswith(root + os.sep):
            return None
        # every component must be a real directory/file inside the copy (no symlink anywhere)
        cur = root
        for part in rel.split("/"):
            cur = os.path.join(cur, part)
            if os.path.islink(cur):
                return None
        if not os.path.isfile(rp):
            return None
        r = os.path.relpath(rp, root)
        return r if r in pages else None

    c = cited.strip()
    while c.startswith("./"):
        c = c[2:]
    cands = [c]
    if c.startswith(project + "/"):
        cands.append(c[len(project) + 1:])
    for cand in cands:
        r = inside(cand)
        if r:
            print(r)
            return
    suffix = [p for p in pages if p == c or p.endswith("/" + c)]
    if len(suffix) == 1 and inside(suffix[0]):
        print(suffix[0])


if __name__ == "__main__":
    main()
