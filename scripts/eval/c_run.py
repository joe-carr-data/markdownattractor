#!/usr/bin/env python3
"""The change-impact evaluation: freeze the cases, build one repository per case, run
`mda check`, score mined-label recall (plan 2026-11-v0.3, Phase C; runbook 4h).

  c_run.py freeze <confirm-dir> <out-dir>              cases.json from the panel's tally; no model
  c_run.py run    <cases.json> <clones> <work> <mda> --half dev|test [--limit N] [--jobs N]
  c_run.py score  <cases.json> <work> --half dev|test  recall and the alert list; no model
  c_run.py adjudicate <cases.json> <work> --half dev|test [--jobs N]   the panel on every alert
  c_run.py baseline   <cases.json> <clones> <work> --half dev|test [--jobs N] [--model M]

A case is one change: commit A's edit of markdown file X, with the files Y the panel confirmed
as left out of date. Per case, `run` builds a repository of two commits and nothing else: the
markdown files as they stood before A, then as they stood at A. No later object, no tag, no
remote. It then runs `mda check --since HEAD~1` there with the `claude-cli` backend, through
the owner's Claude Code login (provider keys are removed from the environment), and stores the
JSON report. A case whose report exists is not run again.

`adjudicate` asks the three panel members of c_confirm.py, with its rubric, about every alert
(deduplicated per case and document pair, whichever arm raised it, shown without the arm's
name): the change to X from the case repository and the passages of Y around the alert's
quote. Precision = alerts confirmed by at least two members over all alerts. `baseline` is the
comparison arm: Claude headless in the same closed repository with file reading, search and
read-only git, the same model, asked to list documents the change left possibly out of date.

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
    labels = hit = strict = 0
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
        found_docs = {y for _, y in found}
        pairs = {(p['change']['rel_path'], p['candidate']['rel_path']): p for p in r['pairs']}
        for a in c['affected']:
            labels += 1
            strict += (c['x'], a['y']) in found
            # A label is a (case, affected document): the review of commit A named Y. Which
            # of A's changed files the finding is attached to is reported apart (strict).
            if a['y'] in found_docs:
                hit += 1
            else:
                p = [v for (x, y), v in pairs.items() if x == c['x'] and y == a['y']]
                why_missed['not a candidate' if not p else '; '.join(sorted({v['outcome'] for v in p}))] += 1
        labelled = {a['y'] for a in c['affected']}
        for f in r['findings']:
            key = (f['change']['rel_path'], f['candidate']['rel_path'])
            alerts.append({'case_id': c['case_id'], 'repo': c['repo'], 'x': key[0], 'y': key[1],
                           'verdict': f['verdict'], 'labelled': key[1] in labelled})
    out = {'half': half, 'cases': len(cases), 'cases_without_report': missing, 'labels': labels, 'labels_alerted': hit,
           'mined_label_recall': round(hit / labels, 3) if labels else None,
           'labels_alerted_from_x_itself': strict, 'why_missed': dict(why_missed),
           'alerts': len(alerts), 'alerts_on_labelled_documents': sum(a['labelled'] for a in alerts),
           'alerts_to_adjudicate': sum(not a['labelled'] for a in alerts), 'judged': judged, 'usage': dict(usage)}
    with open(os.path.join(work, f'score-{half}.json'), 'w') as f:
        json.dump({'summary': out, 'alerts': alerts}, f, indent=1)
    print(json.dumps(out, indent=1))


def panel_module():
    import importlib.util
    spec = importlib.util.spec_from_file_location('c_confirm', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'c_confirm.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def arm_alerts(work, case, arm):
    """The (x, y, quote) alerts of one arm for one case; None when the arm has no result."""
    if arm == 'mda':
        path = os.path.join(work, 'reports', case['case_id'] + '.json')
        if not os.path.exists(path):
            return None
        return [(f['change']['rel_path'], f['candidate']['rel_path'], f['candidate_quote']['text'])
                for f in json.load(open(path))['findings']]
    path = os.path.join(work, 'baseline', case['case_id'] + '.json')
    if not os.path.exists(path):
        return None
    out = json.load(open(path)).get('out') or {}
    return [(case['x'], d.get('path', ''), d.get('quote', '')) for d in out.get('documents', [])]


def alert_prompt(panel, repo, x, y, quote):
    """What a member sees for one alert: the change to X, and Y around the quoted words."""
    env = clean_env()
    diff = sh(['git', '-C', repo, '--no-pager', 'diff', '--no-color', '--no-ext-diff', '-U6', 'HEAD~1', 'HEAD', '--', x], env=env, check=False).stdout.decode('utf-8', 'replace')
    try:
        xa = open(os.path.join(repo, x), encoding='utf-8', errors='replace').read()
        ya = open(os.path.join(repo, y), encoding='utf-8', errors='replace').read()
    except OSError:
        return None
    lines = ya.split('\n')
    words = [w for w in re.findall(r'[A-Za-z0-9_.-]{3,}', quote)][:8]
    hits = [i for i, l in enumerate(lines) if quote.strip() and quote.strip().split('\n')[0].strip() in l] or \
           [i for i, l in enumerate(lines) if words and sum(w in l for w in words) >= max(1, len(words) // 2)]
    if not hits or not diff.strip():
        return None
    lo, hi = max(0, hits[0] - 10), min(len(lines), hits[0] + 11)
    passage = panel.numbered(lines[lo:hi], lo + 1)
    return (f"<x_path>{x}</x_path>\n<x_head>\n{panel.head(xa)}\n</x_head>\n<x_change>\n{diff[:9000]}\n</x_change>\n\n"
            f"<y_path>{y}</y_path>\n<y_head>\n{panel.head(ya)}\n</y_head>\n<y_passages>\n{passage[:9000]}\n</y_passages>\n")


def adjudicate(cases_path, work, half, jobs):
    panel = panel_module()
    cases = load(cases_path, half)
    items, arms_of = {}, collections.defaultdict(set)
    for c in cases:
        # The two arms build identical repositories; either copy serves.
        repo = os.path.join(work, 'repos', c['case_id'])
        if not os.path.exists(os.path.join(repo, '.built')):
            repo = os.path.join(work, 'baseline-work', 'repos', c['case_id'])
        for arm in ('mda', 'baseline'):
            for x, y, quote in arm_alerts(work, c, arm) or []:
                key = (c['case_id'], x, y)
                arms_of[key].add(arm)
                if key not in items and x != y and os.path.isfile(os.path.join(repo, y)):
                    items[key] = (repo, quote)
    path = os.path.join(work, f'adjudication-{half}.jsonl')
    have = {}
    if os.path.exists(path):
        for line in open(path):
            v = json.loads(line)
            if panel.valid(v.get('out')):
                have[(v['case_id'], v['x'], v['y'], v['member'])] = v['out']['verdict']
    todo = [(k, m) for k in sorted(items) for m in panel.MEMBERS if (*k, m) not in have]
    print(f'{len(items)} alerts, {len(todo)} answers to ask', file=sys.stderr)
    sink = open(path, 'a')

    def one(job):
        (case_id, x, y), member = job
        repo, quote = items[(case_id, x, y)]
        prompt = alert_prompt(panel, repo, x, y, quote)
        v = panel.ask(member, prompt) if prompt else {'out': None, 'error': 'no evidence could be shown'}
        if not panel.valid(v.get('out')):
            v = dict(v, out=None, error=v.get('error') or 'answer does not fit the schema')
        return dict(v, case_id=case_id, x=x, y=y, member=member)

    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        for v in pool.map(one, todo):
            sink.write(json.dumps(v) + '\n'); sink.flush()
            if v.get('out'):
                have[(v['case_id'], v['x'], v['y'], v['member'])] = v['out']['verdict']
    labelled_docs = {(c['case_id'], a['y']) for c in cases for a in c['affected']}
    labelled = {k for k in arms_of if (k[0], k[2]) in labelled_docs}
    summary = {}
    for arm in ('mda', 'baseline'):
        keys = [k for k in arms_of if arm in arms_of[k]]
        confirmed = [k for k in keys if sum(have.get((*k, m)) == 'confirm' for m in panel.MEMBERS) >= 2]
        unanswered = [k for k in keys if sum((*k, m) in have for m in panel.MEMBERS) < 3 and k not in confirmed]
        summary[arm] = {'alerts': len(keys), 'confirmed': len(confirmed), 'not_fully_answered': len(unanswered),
                        'precision': round(len(confirmed) / len(keys), 3) if keys else None,
                        'on_labelled_documents': sum(k in labelled for k in keys),
                        'labelled_but_not_confirmed': sum(k in labelled and k not in confirmed for k in keys)}
    json.dump(summary, open(os.path.join(work, f'precision-{half}.json'), 'w'), indent=1)
    print(json.dumps(summary, indent=1))


BASELINE_SCHEMA = {
    'type': 'object', 'additionalProperties': False, 'required': ['documents'],
    'properties': {'documents': {'type': 'array', 'items': {
        'type': 'object', 'additionalProperties': False, 'required': ['path', 'quote'],
        'properties': {'path': {'type': 'string'}, 'quote': {'type': 'string'}}}}},
}
BASELINE_PROMPT = """This folder is a git repository of documentation with two commits. The last commit changed the file {x}.
Look at that change (git diff HEAD~1 HEAD -- {x}), then find other markdown files in this repository that the change left possibly out of date: files that still state, as a current fact or instruction in the same scope, what the change replaced, or that now conflict with it. Do not list changelogs, release notes, migration guides, archives, pages for another version, translations or arbitrary examples. Do not list {x} itself or files the last commit also changed consistently.
Answer with the JSON object the schema describes: for each such file its path relative to the repository root and one short exact quote from it that shows the problem. An empty list is a valid answer."""


def baseline_case(case, clones, work, model):
    out = os.path.join(work, 'baseline', case['case_id'] + '.json')
    if os.path.exists(out):
        return 'kept'
    try:
        # Its own copy of the repository: the two arms can run at the same time.
        repo = build(case, clones, os.path.join(work, 'baseline-work'))
        r = subprocess.run(
            ['claude', '--print', '--model', model, '--setting-sources', '', '--strict-mcp-config', '--no-session-persistence',
             '--max-turns', '40', '--output-format', 'json', '--permission-mode', 'dontAsk',
             '--allowedTools', 'Read', 'Grep', 'Glob', 'Bash(git diff:*)', 'Bash(git log:*)', 'Bash(git show:*)',
             '--json-schema', json.dumps(BASELINE_SCHEMA), '--', BASELINE_PROMPT.format(x=case['x'])],
            capture_output=True, cwd=repo, env=clean_env(), timeout=1800, stdin=subprocess.DEVNULL)
        env = json.loads(r.stdout.decode('utf-8', 'replace'))
        row = {'out': env.get('structured_output'), 'is_error': bool(env.get('is_error')), 'turns': env.get('num_turns'),
               'cost_usd': env.get('total_cost_usd'), 'usage': env.get('usage'), 'models': sorted((env.get('modelUsage') or {}).keys()),
               'duration_ms': env.get('duration_ms')}
        if row['out'] is None:
            row['error'] = str(env.get('result') or 'no structured output')[:300]
    except (RuntimeError, ValueError, subprocess.TimeoutExpired) as e:
        row = {'out': None, 'error': f'{type(e).__name__}: {str(e)[-300:]}'}
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out + '.tmp', 'w') as f:
        json.dump(row, f)
    os.replace(out + '.tmp', out)
    return 'ran' if row['out'] is not None else 'no answer'


def baseline(cases_path, clones, work, half, jobs, model):
    cases = load(cases_path, half)
    done = collections.Counter()
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        for case, state in zip(cases, pool.map(lambda c: baseline_case(c, clones, work, model), cases)):
            done[state] += 1
            print(f"{case['case_id']} {case['repo']}: {state}", file=sys.stderr)
    labels = hit = 0
    cost = 0.0
    for c in cases:
        alerts = arm_alerts(work, c, 'baseline')
        row = json.load(open(os.path.join(work, 'baseline', c['case_id'] + '.json')))
        cost += row.get('cost_usd') or 0
        found = {y for _, y, _ in alerts or []}
        labels += len(c['affected'])
        hit += sum(a['y'] in found for a in c['affected'])
    print(json.dumps({'half': half, **done, 'labels': labels, 'labels_alerted': hit,
                      'mined_label_recall': round(hit / labels, 3) if labels else None, 'cost_usd': round(cost, 2), 'model': model}))


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
    elif cmd == 'adjudicate' and len(sys.argv) >= 4 and '--half' in sys.argv:
        adjudicate(sys.argv[2], sys.argv[3], opt('--half', 'dev'), int(opt('--jobs', 4)))
    elif cmd == 'baseline' and len(sys.argv) >= 5 and '--half' in sys.argv:
        baseline(sys.argv[2], sys.argv[3], sys.argv[4], opt('--half', 'dev'), int(opt('--jobs', 2)),
                 opt('--model', 'claude-haiku-4-5'))
    else:
        sys.exit(__doc__)
