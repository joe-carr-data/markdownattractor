#!/usr/bin/env python3
"""Build the demo corpus: a copy of the fictional golden set with realistic file times.

markdownattractor dates a section by its file's modification time (`pipeline::file_times`), so a
plain copy reads "just now" everywhere. This script copies `evals/golden/docs` (never modified: it
is a frozen benchmark corpus) to DEST and sets each file's modification time: dated file names keep
their own date, everything else gets a fixed age relative to today, so the demo always shows hits
like "updated 2 days ago" next to ADRs from last year.

Usage: scripts/demo/make-corpus.py DEST   (DEST must not exist)
"""
import datetime as dt
import os
import re
import shutil
import sys

# Days before today for files without a date in their name. Runbooks and specs are the living
# documents (recent edits); ADRs, onboarding and the README are older.
AGE_DAYS = {
    "runbooks/deploy.md": 2,
    "runbooks/on-call.md": 9,
    "runbooks/queue-backlog.md": 23,
    "runbooks/database-failover.md": 41,
    "runbooks/secrets-rotation.md": 64,
    "runbooks/certificate-renewal.md": 118,
    "specs/payments-sepa.md": 5,
    "specs/order-api-v2.md": 17,
    "specs/selective-ci.md": 33,
    "specs/rate-limiting.md": 76,
    "specs/search-index-rewrite.md": 150,
    "onboarding/faq.md": 12,
    "onboarding/first-week.md": 95,
    "onboarding/glossary.md": 210,
    "adr/0001-postgres-over-mysql.md": 690,
    "adr/0002-blue-green-deploys.md": 540,
    "adr/0003-drop-redis-sessions.md": 420,
    "adr/0004-monorepo.md": 330,
    "adr/0005-feature-flags.md": 260,
    "adr/0009-split-workers-repo.md": 45,
    "README.md": 300,
}
DATED = re.compile(r"(\d{4})-(\d{2})-(\d{2})")


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    repo = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    src = os.path.join(repo, "evals", "golden", "docs")
    dest = os.path.abspath(sys.argv[1])
    if os.path.exists(dest):
        print(f"refusing: {dest} exists", file=sys.stderr)
        return 1
    shutil.copytree(src, dest)
    today = dt.datetime.now().replace(hour=10, minute=0, second=0, microsecond=0)
    n = 0
    for root, _dirs, files in os.walk(dest):
        for name in files:
            if not name.endswith(".md"):
                continue
            path = os.path.join(root, name)
            rel = os.path.relpath(path, dest).replace(os.sep, "/")
            m = DATED.search(name)
            if m:
                when = dt.datetime(int(m[1]), int(m[2]), int(m[3]), 16, 0)
            elif rel.startswith("changelog/"):
                year = int(name[:4])
                when = min(dt.datetime(year, 12, 15, 12, 0), today - dt.timedelta(days=8))
            elif rel in AGE_DAYS:
                when = today - dt.timedelta(days=AGE_DAYS[rel])
            else:
                when = today - dt.timedelta(days=180)
            ts = when.timestamp()
            os.utime(path, (ts, ts))
            n += 1
    print(f"demo corpus: {n} files at {dest}; next: mda index --root {dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
