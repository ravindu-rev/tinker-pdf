"""Counted injections: put one rule back broken, run the suites that should
notice, count the tests that fire, and restore the source byte for byte.

usage:
  python3 -I inject.py list  [--catalogue FILE]
  python3 -I inject.py check WORKTREE [--catalogue FILE]
  python3 -I inject.py run   WORKTREE OUTDIR [--build default|bundled] [--suite 'cargo test ...']
                             [--catalogue FILE] NAME... | control

The catalogue (injections.json beside this script) names each injection, the
round that wrote it, its edits (file, the text it replaces, the text it puts
there, how many times that text must occur) and the suites its round ran.
`check` says which injections still apply to a tree: an edit whose text no
longer occurs exactly as often as it says belongs to code that has changed.

`run` applies one injection at a time in WORKTREE, runs its suites (or
--suite, which may be given more than once), records every failing test in
OUTDIR/<name>-<build>.log and one line in OUTDIR/summary-<build>.txt, and
restores the files, checking their sha256 after every run, also when the run
is interrupted. `control` runs, with nothing injected, every suite the other
injections named in the same call run (or --suite). A run in
which a suite printed no `test result`, or a crate did not compile, is marked
incomplete, and its count is absent rather than zero (CONTRIBUTING.md,
"Working an item"). With --build bundled, `--features bundled-fonts` goes into
every suite that tests tinker-pdf and does not name its features already.
"""
import hashlib
import json
import os
import re
import shlex
import signal
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def catalogue(path):
    with open(path or os.path.join(HERE, 'injections.json'), encoding='utf-8') as f:
        return {e['name']: e for e in json.load(f)['injections']}


def sha(path):
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


def applies(tree, e):
    out = []
    for ed in e['edits']:
        p = os.path.join(tree, ed['file'])
        n = open(p, encoding='utf-8').read().count(ed['old']) if os.path.isfile(p) else -1
        out.append((ed['file'], n, ed['count']))
    return out


def with_build(cmd, build):
    if build != 'bundled' or '--features' in cmd:
        return cmd
    out = list(cmd)
    for i, a in enumerate(out[:-1]):
        if a == '-p' and out[i + 1] == 'tinker-pdf':
            return out[:i + 2] + ['--features', 'bundled-fonts'] + out[i + 2:]
    return out


def run_suites(tree, suites, build, log):
    env = dict(os.environ)
    env.setdefault('CARGO_BUILD_JOBS', '3')
    env.update(CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
    failing, complete = [], True
    for cmd in suites:
        cmd = with_build(cmd, build)
        r = subprocess.run(cmd, cwd=tree, env=env, capture_output=True, text=True)
        text = r.stdout + r.stderr
        log.write('$ ' + ' '.join(cmd) + f'\nexit {r.returncode}\n' + text + '\n')
        if not re.search(r'^test result: ', text, re.M):
            complete = False
        if 'could not compile' in text or 'failed to run custom build command' in text:
            complete = False
            failing.append('BUILD-REFUSED')
        failing += re.findall(r'^test (\S+) \.\.\. FAILED', text, re.M)
    return failing, complete


def main():
    args = sys.argv[1:]
    cat_path = None
    if '--catalogue' in args:
        i = args.index('--catalogue')
        cat_path = args[i + 1]
        del args[i:i + 2]
    build = 'default'
    if '--build' in args:
        i = args.index('--build')
        build = args[i + 1]
        del args[i:i + 2]
    suites_override = []
    while '--suite' in args:
        i = args.index('--suite')
        suites_override.append(shlex.split(args[i + 1]))
        del args[i:i + 2]
    if not args:
        raise SystemExit(__doc__)
    cat = catalogue(cat_path)
    if args[0] == 'list':
        for e in cat.values():
            rec = f"  recorded {e['recorded']}" if e.get('recorded') else ''
            print(f"{e['name']:44} {e['round']}{rec}")
        return
    if args[0] == 'check' and len(args) == 2:
        bad = 0
        for e in cat.values():
            res = applies(args[1], e)
            ok = all(n == want for _, n, want in res)
            bad += not ok
            print(('applies   ' if ok else 'NOT       ') + e['name'] + ('' if ok else '  ' + repr(res)))
        print(f'{len(cat) - bad} of {len(cat)} apply')
        return
    if args[0] != 'run' or len(args) < 4:
        raise SystemExit(__doc__)
    tree, out, names = args[1], args[2], args[3:]
    os.makedirs(out, exist_ok=True)
    summary = open(os.path.join(out, f'summary-{build}.txt'), 'a', encoding='utf-8')

    def stop(signum, frame):
        raise SystemExit(f'interrupted by signal {signum}')
    signal.signal(signal.SIGTERM, stop)

    # The control runs what the injections beside it run, once each.
    control_suites = []
    for name in names:
        for s in (suites_override or (cat[name]['suites'] if name in cat else [])):
            if s not in control_suites:
                control_suites.append(s)
    for name in names:
        if name == 'control':
            if not control_suites:
                raise SystemExit('control alone needs --suite, or the injections it is the control for')
            e = {'edits': [], 'suites': control_suites}
        else:
            e = cat[name]
        files = sorted({ed['file'] for ed in e['edits']})
        saved = {f: open(os.path.join(tree, f), 'rb').read() for f in files}
        digests = {f: sha(os.path.join(tree, f)) for f in files}
        res = applies(tree, e) if e['edits'] else []
        if any(n != want for _, n, want in res):
            summary.write(f'{name} {build} NOT RUN: anchors {res}\n')
            summary.flush()
            continue
        try:
            for ed in e['edits']:
                p = os.path.join(tree, ed['file'])
                text = open(p, encoding='utf-8').read()
                open(p, 'w', encoding='utf-8', newline='').write(text.replace(ed['old'], ed['new']))
            with open(os.path.join(out, f'{name.replace("/", "_")}-{build}.log'), 'w', encoding='utf-8') as log:
                failing, complete = run_suites(tree, suites_override or e['suites'], build, log)
        finally:
            for f, data in saved.items():
                open(os.path.join(tree, f), 'wb').write(data)
            for f, d in digests.items():
                if sha(os.path.join(tree, f)) != d:
                    summary.write(f'{name}: RESTORE FAILED for {f}\n')
                    summary.flush()
                    raise SystemExit(f'{f} was not restored; stop and look at it')
        fires = len(failing) if complete else 'absent (incomplete run)'
        summary.write(f'{name} {build} fires {fires} complete={complete} {sorted(set(failing))}\n')
        summary.flush()
        print(f'{name} {build}: fires {fires}')
    summary.close()


if __name__ == '__main__':
    main()
