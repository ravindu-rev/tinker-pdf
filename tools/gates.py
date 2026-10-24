"""Run the checks CI's `test` job runs, and print one line per gate.

usage: python3 -I tools/gates.py [GATE ...] [--first] [--log DIR] [--list]

The gates are the steps of the `test` job in `.github/workflows/ci.yml`, in
its order: fmt; clippy; the workspace's tests; the determinism and shaping
fingerprints, with the lines their legs grep for; the font crate and the
facade without default features; the facade with `bundled-fonts`, and its
clippy; the seven examples, each with the line it must print; and `xtask
check`. Name gates to run only those.

Each gate's whole output goes to `target/gates/<gate>.log`. The screen gets
one line per gate, ok or FAIL with its time and, for tests, the counts; a
failed gate adds its exit status, its failing tests and where each panicked,
its first compiler errors with their places, the files fmt would change, and
the lines it had to print and did not. Open a log only for what those lines
do not say. Every gate runs, so one run reports every failure; `--first`
stops at the first. Exits 1 when a gate fails, 2 for a gate that does not
exist. It runs cargo and nothing else, and changes nothing but its logs and
cargo's own target directory.

`gatescheck.py` beside this file holds each rule that reads a log or runs a
gate to a synthetic log or a fake run.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, '..'))
SHOWN = 10  # failing tests, errors or files a failed gate lists

TEST_RESULT = re.compile(r'^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored', re.M)
FAILED_TEST = re.compile(r'^test (\S+) \.\.\. FAILED$', re.M)
PANICKED = re.compile(r"^thread '([^']+)' panicked at (\S+?):?$", re.M)
ERROR = re.compile(r'^(error(?:\[E\d+\])?: .+)$', re.M)
LOCATION = re.compile(r'^\s*--> (\S+)$', re.M)
FMT_DIFF = re.compile(r'^Diff in (\S+?)(?::(\d+):| at line (\d+):)$', re.M)

EXAMPLES = [('open', 'pages     3'), ('render', 'rendered  '), ('extract', 'Tinker fixture'),
            ('edit', 'the original, untouched'), ('create', 'read back page 1 says'),
            ('convert', 'as a document'), ('parallel', 'byte-identical to the serial render')]
# (name, [(command, lines it must print, whether it must print anything)]), as ci.yml's test job runs them
GATES = [
    ('fmt', [(['cargo', 'fmt', '--all', '--', '--check'], [], False)]),
    ('clippy', [(['cargo', 'clippy', '--workspace', '--all-targets', '--', '-D', 'warnings'], [], False)]),
    ('test', [(['cargo', 'test', '--workspace'], [], False)]),
    ('determinism', [(['cargo', 'test', '-p', 'tinker-pdf', '--test', 'determinism'],
                      [r'test result: ok\. [1-9][0-9]* passed', r'rendering_is_stable_across_targets [.]{3} ok'], False)]),
    ('shaping', [(['cargo', 'test', '-p', 'tinker-pdf-shape', '--test', 'fingerprints'],
                  [r'test result: ok\. [1-9][0-9]* passed', r'shaping_is_stable_across_targets [.]{3} ok',
                   r'bidi_is_stable_across_targets [.]{3} ok'], False)]),
    ('font-no-default', [(['cargo', 'test', '-p', 'tinker-pdf-font', '--no-default-features'], [], False)]),
    ('facade-no-default', [(['cargo', 'test', '-p', 'tinker-pdf', '--no-default-features'], [], False)]),
    ('bundled-fonts', [(['cargo', 'test', '-p', 'tinker-pdf', '--features', 'bundled-fonts'], [], False)]),
    ('clippy-bundled-fonts', [(['cargo', 'clippy', '-p', 'tinker-pdf', '--features', 'bundled-fonts', '--all-targets',
                                '--', '-D', 'warnings'], [], False)]),
    ('examples', [(['cargo', 'run', '-p', 'tinker-pdf', '--example', name], [re.escape(line)], True)
                  for name, line in EXAMPLES]),
    ('xtask', [(['cargo', 'run', '-p', 'xtask', '--', 'check'], [], False)]),
]


def counts(text):
    """passed, failed and ignored over every `test result:` line, or None when there is none."""
    found = TEST_RESULT.findall(text)
    if not found:
        return None
    return tuple(sum(int(x[i]) for x in found) for i in range(3))


def details(text, repo):
    """What a failed gate's log says went wrong, a line each."""
    out = []
    panicked = dict(PANICKED.findall(text))
    for name in FAILED_TEST.findall(text)[:SHOWN]:
        out.append(f'failed: {name}' + (f' (panicked at {panicked[name]})' if name in panicked else ''))
    for m in list(ERROR.finditer(text))[:SHOWN]:
        loc = LOCATION.search(text, m.end(), m.end() + 400)
        follows = loc and not ERROR.search(text, m.end(), loc.start())
        out.append(m.group(1) + (f' ({loc.group(1)})' if follows else ''))
    for m in list(FMT_DIFF.finditer(text))[:SHOWN]:
        path = os.path.relpath(m.group(1), repo) if os.path.isabs(m.group(1)) else m.group(1)
        out.append(f'fmt would change {path} at line {m.group(2) or m.group(3)}')
    return out


def resolve(argv):
    """The command as it can run here, and the environment it runs in: cargo
    from `CARGO_HOME` (`~/.cargo` by default) when PATH lacks it, as a shell
    that never read rustup's profile line does, with that folder put on PATH."""
    if argv[0] != 'cargo' or shutil.which('cargo'):
        return argv, None
    home = os.environ.get('CARGO_HOME') or os.path.join(os.path.expanduser('~'), '.cargo')
    found = shutil.which('cargo', path=os.path.join(home, 'bin'))
    if not found:
        return argv, None
    return [found] + argv[1:], dict(os.environ, PATH=os.path.join(home, 'bin') + os.pathsep + os.environ.get('PATH', ''))


def execute(argv, log, cwd):
    """Run one command, its output appended to the log; its exit status."""
    log.write(f'$ {" ".join(argv)}\n')
    log.flush()
    argv, env = resolve(argv)
    try:
        return subprocess.run(argv, cwd=cwd, stdout=log, stderr=subprocess.STDOUT, env=env).returncode
    except OSError as exc:
        log.write(f'cannot run {argv[0]}: {exc}\n')
        return 127


def run_gate(name, steps, repo, logdir):
    path = os.path.join(logdir, f'{name}.log')
    start = time.monotonic()
    problems = []
    with open(path, 'w', encoding='utf-8') as log:
        for argv, required, prints in steps:
            mark = log.tell()
            code = execute(argv, log, repo)
            log.flush()
            with open(path, encoding='utf-8', errors='replace') as f:
                f.seek(mark)
                text = f.read().split('\n', 1)[-1]
            if code == 127 and text.startswith('cannot run '):
                problems.append(text.split('\n', 1)[0])
            elif code != 0:
                problems.append(f'`{" ".join(argv[1:])}` exited {code}')
            if prints and not text.strip():
                problems.append(f'`{" ".join(argv[1:])}` printed nothing')
            for pattern in required:
                if not re.search(pattern, text, re.M):
                    problems.append(f'`{" ".join(argv[1:])}` did not print /{pattern}/')
    with open(path, encoding='utf-8', errors='replace') as f:
        text = f.read()
    return problems, text, time.monotonic() - start, path


def main(argv):
    ap = argparse.ArgumentParser(description="Run the checks CI's test job runs, one line per gate.")
    ap.add_argument('gates', nargs='*', help='the gates to run (default: all)')
    ap.add_argument('--first', action='store_true', help='stop at the first gate that fails')
    ap.add_argument('--log', default=os.path.join(REPO, 'target', 'gates'), help='where the logs go')
    ap.add_argument('--list', action='store_true', help='print each gate and its commands, and run none')
    ap.add_argument('--repo', default=REPO, help=argparse.SUPPRESS)
    args = ap.parse_args(argv)
    names = [g for g, _ in GATES]
    unknown = [g for g in args.gates if g not in names]
    if unknown:
        print(f'no gate called {", ".join(unknown)}; the gates are {", ".join(names)}')
        return 2
    chosen = [(g, steps) for g, steps in GATES if not args.gates or g in args.gates]
    if args.list:
        for g, steps in chosen:
            print(f'{g}: ' + '; '.join(' '.join(a) for a, _, _ in steps))
        return 0
    os.makedirs(args.log, exist_ok=True)
    failed, ran = [], 0
    for g, steps in chosen:
        ran += 1
        problems, text, secs, path = run_gate(g, steps, args.repo, args.log)
        c = counts(text)
        tally = f'  {c[0]} passed, {c[1]} failed, {c[2]} ignored' if c else ''
        print(f'{"FAIL" if problems else "ok  "}  {g:<21} {secs:7.1f}s{tally}', flush=True)
        if problems:
            failed.append(g)
            for line in problems + details(text, args.repo):
                print(f'      {line}')
            print(f'      log: {os.path.relpath(path, args.repo)}')
            if args.first:
                break
    print(f'{ran} gates: {ran - len(failed)} ok' + (f', {len(failed)} failed ({", ".join(failed)})' if failed else ''))
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
