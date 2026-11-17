#!/usr/bin/env python3
"""Miner for the change-impact evaluation (plan 2026-11-v0.3, Phase C; runbook section 4f).

Finds hypotheses, not labels: a commit A that replaces a few tokens in markdown file X, and a
later commit B (within the window) that makes the same replacement in another file Y that A
did not touch and that still carried the old tokens at A. Flags mark the kinds a panel is not
asked about (translations, logs, mirrored copies, word-only and link-only edits, timestamps).

usage: c_mine.py <bare-repo> [--years N] [--window-days D]  -> JSON lines on stdout, counts on stderr
No model, read-only on the clone.
"""
import sys, re, json, subprocess, collections

repo = sys.argv[1]
years = int(sys.argv[sys.argv.index('--years') + 1]) if '--years' in sys.argv else 3
window = (int(sys.argv[sys.argv.index('--window-days') + 1]) if '--window-days' in sys.argv else 60) * 86400
TOK = re.compile(r"[A-Za-z0-9_@][A-Za-z0-9_.\-/:@+]*[A-Za-z0-9_+]|[A-Za-z0-9]")
STOP = set("the a an and or of to in on for with is are was be this that it as at by from you your we our can will not if then use used using see more new also".split())
LOCALE = re.compile(r"(^|/)(i18n|locales?|translations?|[a-z]{2}(-[A-Za-z]{2,4})?)(/|$)")
MIRROR = re.compile(r"version|nostarch|translated|/v\d")
LOG = re.compile(r"(changelog|release[-_ ]?notes?|releases?|history|news|migration)", re.I)

def git(*a):
    return subprocess.run(['git', '-C', repo, *a], capture_output=True, text=True, errors='replace').stdout

def informative(t):
    return (any(c.isdigit() for c in t) and len(t) >= 2) or (len(t) >= 4 and t.lower() not in STOP)

out = git('log', '--no-merges', f'--since={years}.years', '-p', '-U0', '--no-color', '--no-renames',
          '--format=@@C %H %ct %s', '--', '*.md', '*.mdx')
sig_index = collections.defaultdict(list)   # signature -> [(time, commit, file, subject)]
touched = collections.defaultdict(set)      # commit -> files
commit = t = subj = f = None
rem, add = [], []

def flush():
    global rem, add
    if commit and f and rem and add:
        R = set(TOK.findall(' '.join(rem))); A = set(TOK.findall(' '.join(add)))
        r, a = R - A, A - R
        if 1 <= len(r) <= 3 and 1 <= len(a) <= 3 and any(informative(x) for x in r) and any(informative(x) for x in a):
            sig_index[(tuple(sorted(r)), tuple(sorted(a)))].append((t, commit, f, subj))
    rem, add = [], []

for line in out.split('\n'):
    if line.startswith('@@C '):
        flush(); _, commit, ts, *rest = line.split(' ', 3); t = int(ts); subj = rest[0] if rest else ''; f = None
    elif line.startswith('diff --git '):
        flush(); f = None
    elif line.startswith('+++ '):
        f = line[6:] if line.startswith('+++ b/') else None
        if f: touched[commit].add(f)
    elif line.startswith('--- '):
        pass
    elif line.startswith('@@'):
        flush()
    elif line.startswith('-'):
        rem.append(line[1:])
    elif line.startswith('+'):
        add.append(line[1:])
flush()

cases, seen = [], set()
DATEISH = re.compile(r"^\d{4}-\d{2}-\d{2}(T[\d:.Z+-]*)?$|^\d{1,2}[:/.]\d{1,2}([:/.]\d{1,4})?$")
show_cache = {}
def show(c, y):
    if (c, y) not in show_cache: show_cache[(c, y)] = git('show', f'{c}:{y}')
    return show_cache[(c, y)]
skipped_bulk = 0
for sig, occ in sig_index.items():
    if len(occ) > 30:            # a bulk or translated replacement: not a natural missed copy
        skipped_bulk += 1; continue
    if all(DATEISH.match(x) for x in sig[0] + sig[1]): continue   # timestamps being bumped
    occ.sort()
    for i, (ta, ca, x, sa) in enumerate(occ):
        if (ca, sig) in seen: continue
        ys = []
        for tb, cb, y, sb in occ[i + 1:]:
            if cb == ca or y == x or not (0 < tb - ta <= window): continue
            if y in touched[ca] or 'revert' in sb.lower(): continue
            ys.append((cb, y, tb - ta, sb))
        if not ys: continue
        # Y must exist at A and still carry the old tokens there
        keep = []
        for cb, y, dt, sb in ys:
            txt = show(ca, y)
            if txt and all(tok in txt for tok in sig[0]):
                keep.append({'fix_commit': cb, 'file': y, 'days_later': round(dt / 86400, 1), 'fix_subject': sb[:100]})
        keep = list({k['file']: k for k in reversed(keep)}.values())[::-1]   # one entry per file, the earliest fix
        if not keep: continue
        seen.add((ca, sig))
        flags = []
        if any(LOCALE.search(k['file']) for k in keep) or LOCALE.search(x): flags.append('locale_path')
        if LOG.search(x) or any(LOG.search(k['file']) for k in keep): flags.append('log_like')
        if all(tok.isalpha() for tok in sig[0] + sig[1]): flags.append('words_only')
        if MIRROR.search(x) or any(MIRROR.search(k['file']) for k in keep): flags.append('mirror_path')
        if any(k['file'].rsplit('/', 1)[-1] == x.rsplit('/', 1)[-1] for k in keep): flags.append('same_basename')
        if all(('/' in tok and ('.' in tok or tok.startswith('http'))) for tok in sig[0]): flags.append('link_only')
        cases.append({'repo': repo.rstrip('/').split('/')[-1], 'change_commit': ca, 'change_file': x, 'subject': sa[:100],
                      'removed': sig[0], 'added': sig[1], 'files_in_change': len(touched[ca]), 'affected': keep, 'flags': flags})
for c in cases: print(json.dumps(c))
clean = [c for c in cases if not c['flags']]
print(f"{repo.split('/')[-1]}: signatures={len(sig_index)} cases={len(cases)} unflagged={len(clean)} bulk_skipped={skipped_bulk}", file=sys.stderr)
