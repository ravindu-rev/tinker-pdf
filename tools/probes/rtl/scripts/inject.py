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
injections named in the same call run (or --suite). With --build bundled,
`--features bundled-fonts` goes into every suite that tests tinker-pdf and
does not name its features already.

A run's count comes from the bytes on disk and from every test binary it
started, or it is absent rather than zero (CONTRIBUTING.md, "Working an
item"). Cargo calls a crate fresh by file times, so a tree whose files are
older than a target's last build, another tree's injected one in a shared
target, would run that build. Before every run, the control included, every
file the edits of the call's injections name is touched, and a run is
incomplete unless its log shows each crate it must have built compiling: the
crates of its own edits, and for the control those of every file touched.
(A crate whose files no injection of the call names is not touched: a target
shared with another tree can still hold that tree's build of it, so give
each tree a target of its own.) A run is also incomplete when a suite's
build was refused, when a test binary cargo started (`Running ...`,
`Doc-tests ...`) printed no `test result` (a crash: a stack overflow, an
abort), or when a suite exited non-zero naming no failing test.
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


def crate_of(tree, path):
    """The package `path` (relative to `tree`) belongs to: the name in the
    nearest Cargo.toml above it with a [package], or None."""
    parts = path.replace('\\', '/').split('/')[:-1]
    while parts:
        toml = os.path.join(tree, *parts, 'Cargo.toml')
        if os.path.isfile(toml):
            text = open(toml, encoding='utf-8').read()
            if '[package]' in text:
                m = re.search(r'^name\s*=\s*"([^"]+)"', text.split('[package]', 1)[1], re.M)
                return m.group(1) if m else None
        parts.pop()
    return None


def freshen(tree, files):
    """Touch `files` (relative to `tree`), so that cargo compiles the bytes on
    disk whatever a shared target last built; their crates, which the run must
    then show compiling."""
    for f in files:
        if os.path.isfile(os.path.join(tree, f)):
            os.utime(os.path.join(tree, f))
    return sorted({c for c in (crate_of(tree, f) for f in files) if c})


# cargo's header of each test binary it starts, and of the doc tests.
STARTED = re.compile(r'^ +(Running \S.*\(.*\)|Doc-tests \S+)$', re.M)
# A failing test as libtest prints it, `#[should_panic]` ones too.
FAILED = re.compile(r'^test (\S+)(?: - should panic)? \.\.\. FAILED', re.M)


def read_run(outputs, crates):
    """A run's verdict from what its suites printed, [(cargo's output with its
    test binaries', in the order printed; exit status)], and the crates it
    must show compiling: the failing tests, whether the run is complete, and
    the crates it did not show compiling."""
    failing, complete = [], True
    for text, code in outputs:
        starts = [m.start() for m in STARTED.finditer(text)]
        if not starts or not all(re.search(r'^test result: ', text[a:b], re.M)
                                 for a, b in zip(starts, starts[1:] + [len(text)])):
            complete = False
        if 'could not compile' in text or 'failed to run custom build command' in text:
            complete = False
            failing.append('BUILD-REFUSED')
        named = FAILED.findall(text)
        if code != 0 and not named:
            complete = False
        failing += named
    built = '\n'.join(text for text, _ in outputs)
    unbuilt = [c for c in crates if not re.search(r'^ *Compiling ' + re.escape(c) + r' v', built, re.M)]
    return failing, complete and not unbuilt, unbuilt


def run_suites(tree, suites, build, log):
    env = dict(os.environ)
    env.setdefault('CARGO_BUILD_JOBS', '3')
    env.update(CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
    outputs = []
    for cmd in suites:
        cmd = with_build(cmd, build)
        # One stream, so that each test binary's output follows its header.
        r = subprocess.run(cmd, cwd=tree, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                           text=True, encoding='utf-8', errors='replace')
        log.write('$ ' + ' '.join(cmd) + f'\nexit {r.returncode}\n' + r.stdout + '\n')
        outputs.append((r.stdout, r.returncode))
    return outputs


def main():
    args = sys.argv[1:]
    if '-h' in args or '--help' in args:
        print(__doc__.strip())
        return
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
    # Every file the call's injections edit, touched before every run.
    touched = sorted({ed['file'] for name in names if name in cat for ed in cat[name]['edits']})
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
            # The crates the run must show compiling: its own edits', and for
            # the control, which runs every suite of the call, every file's.
            crates = freshen(tree, touched)
            if e['edits']:
                crates = sorted({c for c in (crate_of(tree, f) for f in files) if c})
            for ed in e['edits']:
                p = os.path.join(tree, ed['file'])
                text = open(p, encoding='utf-8').read()
                open(p, 'w', encoding='utf-8', newline='').write(text.replace(ed['old'], ed['new']))
            with open(os.path.join(out, f'{name.replace("/", "_")}-{build}.log'), 'w', encoding='utf-8') as log:
                outputs = run_suites(tree, suites_override or e['suites'], build, log)
            failing, complete, unbuilt = read_run(outputs, crates)
        finally:
            for f, data in saved.items():
                open(os.path.join(tree, f), 'wb').write(data)
            for f, d in digests.items():
                if sha(os.path.join(tree, f)) != d:
                    summary.write(f'{name}: RESTORE FAILED for {f}\n')
                    summary.flush()
                    raise SystemExit(f'{f} was not restored; stop and look at it')
        fires = len(failing) if complete else 'absent (incomplete run)'
        stale = f' not compiled: {" ".join(unbuilt)}' if unbuilt else ''
        summary.write(f'{name} {build} fires {fires} complete={complete} {sorted(set(failing))}{stale}\n')
        summary.flush()
        print(f'{name} {build}: fires {fires}')
    summary.close()


if __name__ == '__main__':
    main()
