#!/usr/bin/env python3
"""The change-impact evaluation: freeze the cases, build one repository per case, run
`mda check`, score mined-label recall (plan 2026-11-v0.3, Phase C; runbook 4h).

  c_run.py freeze <confirm-dir> <out-dir>              cases.json from the panel's tally; no model
  c_run.py run    <cases.json> <clones> <work> <mda> --half dev|test [--limit N] [--jobs N]
  c_run.py score  <cases.json> <work> --half dev|test  recall and the alert list; no model

A case is one change: commit A's edit of markdown file X, with the files Y the panel confirmed
as left out of date. Per case, `run` builds a repository of two commits and nothing else: the
markdown files as they stood before A, then as they stood at A. No later object, no tag, no
remote. It then runs `mda check --since HEAD~1` there with the `claude-cli` backend, through
the owner's Claude Code login (provider keys are removed from the environment), and stores the
JSON report. A case whose report exists is not run again.

Split: repositories ordered by the sha256 of their name, assigned dev, test, dev, ... A
repository is in one half only.
"""
import sys, os, re, json, hashlib, subprocess, shutil, collections, concurrent.futures

MD = ('*.md', '*.mdx', '*.markdown')


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def freeze(confirm_dir, out_dir):
    rows = json.load(open(os.path.join(confirm_dir, 'tally.json')))
    confirmed = [r for r in rows if r['state'] == 'confirmed']
    repos = sorted({r['repo'] for r in confirmed}, key=sha)
    half = {name: ('dev' if i % 2 == 0 else 'test') for i, name in enumerate(repos)}
    cases = collections.OrderedDict()
    for r in sorted(confirmed, key=lambda r: (r['repo'], r['change_commit'], r['x'], r['y'])):
        key = (r['repo'], r['change_commit'], r['x'])
        case = cases.setdefault(key, {
            'case_id': sha('|'.join(key))[:12], 'half': half[r['repo']], 'repo': r['repo'],
            'change_commit': r['change_commit'], 'x': r['x'], 'affected': []})
        case['affected'].append({'y': r['y'], 'removed': r['removed'], 'added': r['added'], 'votes': r['votes']})
    os.makedirs(out_dir, exist_ok=True)
    out = {'split_rule': 'repositories ordered by sha256(name), alternately dev and test',
           'pairs_asked': len(rows), 'pairs_confirmed': len(confirmed), 'cases': list(cases.values())}
    with open(os.path.join(out_dir, 'cases.json'), 'w') as f:
        json.dump(out, f, indent=1)
        f.write('\n')
    for h in ('dev', 'test'):
        cs = [c for c in cases.values() if c['half'] == h]
        print(f"{h}: {len(cs)} cases, {sum(len(c['affected']) for c in cs)} labelled documents, "
              f"{len({c['repo'] for c in cs})} repositories")


def clean_env():
    env = {}
    for k, v in os.environ.items():
        if re.search(r'(API_KEY|_TOKEN)$', k) and k not in ('GITHUB_TOKEN', 'GH_TOKEN'):
            continue
        if re.match(r'(CLAUDE_CODE_|CLAUDECODE|CLAUDE_PID|CLAUDE_PLUGIN_|CLAUDE_PROJECT_DIR|CLAUDE_EFFORT)', k):
            continue
        if k.startswith('GIT_'):
            continue
        env[k] = v
    return env


def sh(args, cwd=None, env=None, check=True, timeout=None):
    r = subprocess.run(args, cwd=cwd, env=env, capture_output=True, timeout=timeout)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(args[:6])}: {r.stderr.decode('utf-8', 'replace')[-400:]}")
    return r


def export(bare, commit, dest):
    """The markdown files of `commit`, into `dest` (which is emptied of everything but .git)."""
    for name in os.listdir(dest):
        if name != '.git':
            p = os.path.join(dest, name)
            shutil.rmtree(p) if os.path.isdir(p) and not os.path.islink(p) else os.remove(p)
    names = sh(['git', '-C', bare, 'ls-tree', '-r', '--name-only', '-z', commit]).stdout.decode('utf-8', 'replace').split('\0')
    wanted = [n for n in names if n and n.lower().endswith(('.md', '.mdx', '.markdown'))]
    if not wanted:
        return 0
    # The setup may fetch a large blob the partial clone lacks; the run itself never does.
    tar = subprocess.run(['git', '-C', bare, 'archive', '--format=tar', commit, '--', *wanted], capture_output=True)
    if tar.returncode != 0:
        raise RuntimeError(f"git archive {commit[:10]}: {tar.stderr.decode('utf-8', 'replace')[-300:]}")
    subprocess.run(['tar', '-x', '-C', dest], input=tar.stdout, check=True)
    return len(wanted)


def build(case, clones, work):
    """A repository of two commits: the markdown before A, then at A. Returns its path."""
    bare = os.path.join(clones, case['repo'] + '.git')
    repo = os.path.join(work, 'repos', case['case_id'])
    if os.path.exists(os.path.join(repo, '.built')):
        return repo
    shutil.rmtree(repo, ignore_errors=True)
    os.makedirs(repo)
    env = dict(clean_env(), GIT_AUTHOR_DATE='2020-01-01T00:00:00Z', GIT_COMMITTER_DATE='2020-01-01T00:00:00Z')
    ident = ['-c', 'user.name=eval', '-c', 'user.email=eval@example.invalid', '-c', 'commit.gpgsign=false', '-c', 'core.autocrlf=false']
    sh(['git', 'init', '-q', '-b', 'main', repo], env=env)
    for commit, msg in ((case['change_commit'] + '^', 'before'), (case['change_commit'], 'the change')):
        export(bare, commit, repo)
        open(os.path.join(repo, '.gitignore'), 'w').write('.markdownattractor/\n')
        sh(['git', '-C', repo, *ident, 'add', '-A'], env=env)
        sh(['git', '-C', repo, *ident, 'commit', '-q', '--allow-empty', '-m', msg], env=env)
    # Nothing later than A can be in here: two commits, no remote, no tag, no alternate.
    count = sh(['git', '-C', repo, 'rev-list', '--all', '--count'], env=env).stdout.decode().strip()
    remotes = sh(['git', '-C', repo, 'remote'], env=env).stdout.decode().strip()
    tags = sh(['git', '-C', repo, 'tag'], env=env).stdout.decode().strip()
    alternates = os.path.exists(os.path.join(repo, '.git', 'objects', 'info', 'alternates'))
    if count != '2' or remotes or tags or alternates:
        raise RuntimeError(f"{case['case_id']}: not a closed two-commit repository")
    open(os.path.join(repo, '.built'), 'w').write('ok\n')
    return repo


def run_case(case, clones, work, mda):
    out = os.path.join(work, 'reports', case['case_id'] + '.json')
    if os.path.exists(out):
        return 'kept'
    try:
        repo = build(case, clones, work)
        env = dict(clean_env(), NO_COLOR='1', MDA_NO_DAEMON='1')
        sh([mda, 'backend', 'claude-cli', '--i-accept-the-policy', '--root', repo], env=env, timeout=120)
        r = sh([mda, '--json', 'check', '--since', 'HEAD~1', '--root', repo], env=env, check=False, timeout=3600)
        text = r.stdout.decode('utf-8', 'replace')
        report = json.loads(text)
        if 'findings' not in report:
            raise RuntimeError(text[-300:])
    except (RuntimeError, ValueError, subprocess.TimeoutExpired) as e:
        os.makedirs(os.path.join(work, 'errors'), exist_ok=True)
        open(os.path.join(work, 'errors', case['case_id'] + '.txt'), 'w').write(str(e)[-2000:])
        return 'error'
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out + '.tmp', 'w') as f:
        json.dump(report, f)
    os.replace(out + '.tmp', out)
    return 'ran'


def load(cases_path, half):
    return [c for c in json.load(open(cases_path))['cases'] if c['half'] == half]


def run(cases_path, clones, work, mda, half, limit, jobs):
    cases = load(cases_path, half)[:limit or None]
    done = collections.Counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        for case, state in zip(cases, pool.map(lambda c: run_case(c, clones, work, mda), cases)):
            done[state] += 1
            print(f"{case['case_id']} {case['repo']}: {state}", file=sys.stderr)
    print(dict(done))


def score(cases_path, work, half):
    cases = load(cases_path, half)
    labels = hit = 0
    alerts, missing, usage, judged = [], [], collections.Counter(), 0
    why_missed = collections.Counter()
    for c in cases:
        path = os.path.join(work, 'reports', c['case_id'] + '.json')
        if not os.path.exists(path):
            missing.append(c['case_id'])
            labels += len(c['affected'])     # a case that did not run misses its labels
            why_missed['no report'] += len(c['affected'])
            continue
        r = json.load(open(path))
        judged += r['judged']
        usage['input_tokens'] += r['usage']['input_tokens']
        usage['output_tokens'] += r['usage']['output_tokens']
        usage['cost_usd'] += r['usage']['cost_usd']
        found = {(f['change']['rel_path'], f['candidate']['rel_path']) for f in r['findings']}
        pairs = {(p['change']['rel_path'], p['candidate']['rel_path']): p for p in r['pairs']}
        for a in c['affected']:
            labels += 1
            if (c['x'], a['y']) in found:
                hit += 1
            else:
                p = [v for (x, y), v in pairs.items() if x == c['x'] and y == a['y']]
                why_missed['not a candidate' if not p else '; '.join(sorted({v['outcome'] for v in p}))] += 1
        labelled = {(c['x'], a['y']) for a in c['affected']}
        for f in r['findings']:
            key = (f['change']['rel_path'], f['candidate']['rel_path'])
            alerts.append({'case_id': c['case_id'], 'repo': c['repo'], 'x': key[0], 'y': key[1],
                           'verdict': f['verdict'], 'labelled': key in labelled})
    out = {'half': half, 'cases': len(cases), 'cases_without_report': missing, 'labels': labels, 'labels_alerted': hit,
           'mined_label_recall': round(hit / labels, 3) if labels else None, 'why_missed': dict(why_missed),
           'alerts': len(alerts), 'alerts_on_labelled_documents': sum(a['labelled'] for a in alerts),
           'alerts_to_adjudicate': sum(not a['labelled'] for a in alerts), 'judged': judged, 'usage': dict(usage)}
    with open(os.path.join(work, f'score-{half}.json'), 'w') as f:
        json.dump({'summary': out, 'alerts': alerts}, f, indent=1)
    print(json.dumps(out, indent=1))


def opt(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else ''
    if cmd == 'freeze' and len(sys.argv) == 4:
        freeze(sys.argv[2], sys.argv[3])
    elif cmd == 'run' and len(sys.argv) >= 6 and '--half' in sys.argv:
        run(sys.argv[2], sys.argv[3], sys.argv[4], os.path.abspath(sys.argv[5]), opt('--half', 'dev'),
            int(opt('--limit', 0)), int(opt('--jobs', 2)))
    elif cmd == 'score' and len(sys.argv) >= 4 and '--half' in sys.argv:
        score(sys.argv[2], sys.argv[3], opt('--half', 'dev'))
    else:
        sys.exit(__doc__)
