"""The kit's own checks: its readers on what cargo prints on Linux and on
Windows, which must read alike, and inject.py's verdict on a run.

usage: python3 -I selfcheck.py

Builds nothing and runs nothing: inject.py's checks hand it, in a throwaway
tree, what cargo would print. It prints one line a check and exits 1 when
any does not hold, or raises. The suites hold their comparisons in their own
tests (`cargo test -p tinker-pdf --test rtl_probes`, `cargo test -p
tinker-pdf-shape --test rtl_lines`); these hold the scripts, which no cargo
test may run (ruling 13: no test spawns a program).
"""
import contextlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile


def load(name):
    sys.dont_write_bytecode = True
    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location(name, os.path.join(here, name + '.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


K = load('rtlkit')

# Two families' cargo output as run.sh captures it on Linux, after the crate's
# unit tests.
LINUX = '''\
   Compiling tinker-pdf v0.1.0 (/home/r/tinker-pdf/crates/tinker-pdf)
    Finished `test` profile [unoptimized] target(s) in 61.20s
     Running unittests src/lib.rs (target/debug/deps/tinker_pdf-0123456789abcdef)

running 1 test
ROW\tunit\tnot a family's\tcontent=true\tlogical=true
test unit ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/zz_other.rs (target/debug/deps/zz_other-0123456789abcdef)

running 1 test
ROW\tother\tbook\tcontent=true\tlogical=true
test other ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s

     Running tests/zz_k11rv_fixtures.rs (target/debug/deps/zz_k11rv_fixtures-fedcba9876543210)

running 1 test
opened=2
FIX\t/testdata/simple-text.pdf\tpages=1\tlines=1\trtl=0\tlogical=01\tcontent=01\tstreams=02
FIX\t/fuzz/corpus/cos_object/dictionary\tpages=1\tlines=0\trtl=0\tlogical=03\tcontent=03\tstreams=04
test fixtures ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.74s
exit 0
'''

# What the normaliser makes of LINUX, whole and with --binary zz_k11rv_fixtures.
WHOLE = [
    'Running tests/zz_other.rs',
    'Running tests/zz_k11rv_fixtures.rs',
    'FIX\t/fuzz/corpus/cos_object/dictionary\tpages=1\tlines=0\trtl=0\tlogical=03\tcontent=03\tstreams=04',
    'FIX\t/testdata/simple-text.pdf\tpages=1\tlines=1\trtl=0\tlogical=01\tcontent=01\tstreams=02',
    'ROW\tother\tbook\tcontent=true\tlogical=true',
    "ROW\tunit\tnot a family's\tcontent=true\tlogical=true",
    'opened=2',
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out',
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out',
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out',
    'exit 0',
]
BINARY = [
    'Running tests/zz_k11rv_fixtures.rs',
    'FIX\t/fuzz/corpus/cos_object/dictionary\tpages=1\tlines=0\trtl=0\tlogical=03\tcontent=03\tstreams=04',
    'FIX\t/testdata/simple-text.pdf\tpages=1\tlines=1\trtl=0\tlogical=01\tcontent=01\tstreams=02',
    'opened=2',
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out',
    'exit 0',
]

# The header cargo printed for this family on Windows (run.sh at 56dfd93,
# 17 October 2026): the platform's separator after `tests` and in the binary's
# path, beside the target directory as it was given.
WINDOWS_HEADER = ('     Running tests\\zz_k11rv_fixtures.rs (C:/Users/ravin/Projects/tinker-pdf/'
                  '.scratch/t/base-56\\debug\\deps\\zz_k11rv_fixtures-43f6be906bcb4f8f.exe)')

# A test's panic as libtest prints it on Linux and on Windows: the file as
# cargo handed it to rustc, with the platform's separator.
PANIC = "thread 'fixtures' (4242) panicked at crates/tinker-pdf/tests/zz_k11rv_fixtures.rs:170:9:"
PANIC_WINDOWS = PANIC.replace('crates/tinker-pdf/tests/', 'crates\\tinker-pdf\\tests\\')


def on_windows(text):
    """`text` as cargo prints it on Windows: `\\` in the paths of its headers,
    and each test binary an `.exe`."""
    out = []
    for line in text.split('\n'):
        if line.lstrip().startswith('Running '):
            line = line.replace('/', '\\')[:-1] + '.exe)'
        elif line.lstrip().startswith('Compiling '):
            line = line.replace('(/home/r/', '(C:\\Users\\r\\').replace('/', '\\')
        out.append(line)
    return '\n'.join(out)


def lines(text):
    return [l + '\n' for l in text.split('\n')]


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        f.write(text)


# One test binary's output as libtest prints it: passing, failing, a failing
# #[should_panic] test, and one that overflowed its stack, as abort.sh's did
# on Windows (round 6's review), which prints no result.
PASS = ('running 1 test\ntest guard ... ok\n\n'
        'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n')
FAIL = ('running 1 test\ntest guard ... FAILED\n\nfailures:\n\nfailures:\n    guard\n\n'
        'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n')
SHOULD = FAIL.replace('test guard ... FAILED', 'test guard - should panic ... FAILED')
CRASH = ("running 1 test\n\nthread 'guard' (30652) has overflowed its stack\n"
         'error: test failed, to rerun pass `--test a`\n\nCaused by:\n'
         "  process didn't exit successfully: `C:/t\\debug\\deps\\a-d63ce9ea72c79cb9.exe` "
         '(exit code: 0xc00000fd, STATUS_STACK_OVERFLOW)\n')


def cargo(*binaries, compiling=True):
    """`cargo test`'s output, its own and its test binaries' in the order
    printed, as on Windows: one binary a body; without `compiling`, a build
    cargo found fresh."""
    out = '   Compiling demo v0.1.0 (C:\\Users\\r\\demo)\n' if compiling else ''
    out += '    Finished `test` profile [unoptimized] target(s) in 0.44s\n'
    for i, body in enumerate(binaries):
        out += f'     Running tests\\t{i}.rs (C:/t\\debug\\deps\\t{i}-d63ce9ea72c79cb9.exe)\n\n{body}\n'
    return out


OLD = 1790812800  # 1 October 2026: a checkout older than the target's last build


def injected(out, code, names):
    """inject.py's summary lines for `run TREE OUT --catalogue CAT NAMES` in a
    throwaway tree holding crate `demo`, whose source carries the time OLD
    and which injection `demo/v` edits, with every suite answered by `out` and
    exit `code` in place of cargo; and whether the source was newer than OLD
    when the first suite ran."""
    seen = []

    def fake(cmd, **kw):
        seen.append(os.stat(src).st_mtime > OLD)
        if kw.get('stderr') == subprocess.STDOUT:
            return subprocess.CompletedProcess(cmd, code, out, None)
        # Two streams: cargo's lines and a binary's panics on stderr, the rest on stdout.
        err = [l.startswith(('   ', 'error', 'Caused by', 'thread ')) for l in out.splitlines(True)]
        return subprocess.CompletedProcess(cmd, code, ''.join(l for l, e in zip(out.splitlines(True), err) if not e),
                                           ''.join(l for l, e in zip(out.splitlines(True), err) if e))
    with tempfile.TemporaryDirectory() as tmp:
        tree, cat = os.path.join(tmp, 'tree'), os.path.join(tmp, 'injections.json')
        src = os.path.join(tree, 'crates', 'demo', 'src', 'lib.rs')
        write(os.path.join(tree, 'crates', 'demo', 'Cargo.toml'), '[package]\nname = "demo"\nversion = "0.1.0"\n')
        write(src, 'pub fn v() -> u32 {\n    1\n}\n')
        os.utime(src, (OLD, OLD))
        write(cat, json.dumps({'injections': [{
            'name': 'demo/v', 'round': 'selfcheck', 'suites': [['cargo', 'test', '--no-fail-fast']],
            'edits': [{'file': 'crates/demo/src/lib.rs', 'old': '    1\n', 'new': '    2\n', 'count': 1}]}]}))
        inject, run, argv = load('inject'), subprocess.run, sys.argv
        subprocess.run, sys.argv = fake, ['inject.py', 'run', tree, os.path.join(tmp, 'out'), '--catalogue', cat] + names
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                inject.main()
        finally:
            subprocess.run, sys.argv = run, argv
        with open(os.path.join(tmp, 'out', 'summary-default.txt'), encoding='utf-8') as f:
            return f.read().splitlines(), seen[:1]


def checks():
    windows = on_windows(LINUX)
    fixtures = WINDOWS_HEADER + '\n' + LINUX.split('fedcba9876543210)\n', 1)[1]
    yield ('a Linux output normalises as it always did',
           K.normalise(lines(LINUX)), WHOLE)
    yield ('a Windows output normalises as the Linux one, headers with a /',
           K.normalise(lines(windows)), WHOLE)
    yield ('one binary of a Linux output, --binary',
           K.normalise(lines(LINUX), binary='zz_k11rv_fixtures'), BINARY)
    yield ('one binary of a Windows output, --binary, keeps its lines',
           K.normalise(lines(windows), binary='zz_k11rv_fixtures'), BINARY)
    yield ("the header cargo printed on this box, --binary",
           K.normalise(lines(fixtures), binary='zz_k11rv_fixtures'), BINARY)
    yield ('a Windows header is no data line',
           [bool(K.DATA.match(WINDOWS_HEADER) or K.SUMMARY.match(WINDOWS_HEADER))], [False])
    yield ('a panic on Linux keeps the file it names as it was',
           K.normalise(lines(PANIC + '\nexit 101\n')), [PANIC, 'exit 101'])
    yield ('a panic on Windows names its file with a /',
           K.normalise(lines(PANIC_WINDOWS + '\nexit 101\n')), [PANIC, 'exit 101'])
    # inject.py: a count comes from every test binary cargo started, and from
    # the bytes on disk (round 6's review, F4 and F3).
    control = ['--suite', 'cargo test --no-fail-fast', 'control']
    absent = 'control default fires absent (incomplete run) complete=False []'
    yield ('inject: a failing test is counted',
           lambda: injected(cargo(FAIL, PASS), 101, control)[0], ["control default fires 1 complete=True ['guard']"])
    yield ('inject: a failing #[should_panic] test is counted',
           lambda: injected(cargo(SHOULD), 101, control)[0], ["control default fires 1 complete=True ['guard']"])
    yield ('inject: a test binary that crashed before its result leaves the run incomplete',
           lambda: injected(cargo(CRASH, PASS), 101, control)[0], [absent])
    yield ('inject: a test binary that crashed beside one that failed leaves the run incomplete',
           lambda: injected(cargo(FAIL, CRASH), 101, control)[0], [absent.replace('[]', "['guard']")])
    yield ('inject: an exit not 0 naming no failing test leaves the run incomplete',
           lambda: injected(cargo(PASS) + 'error: 1 target failed:\n    `--test t0`\n', 101, control)[0], [absent])
    yield ("inject: a control touches the call's files before cargo runs, and reads as before when they compile",
           lambda: injected(cargo(PASS), 0, ['control', 'demo/v']),
           (['control default fires 0 complete=True []', 'demo/v default fires 0 complete=True []'], [True]))
    yield ('inject: a run that shows no crate of its files compiling is incomplete',
           lambda: injected(cargo(PASS, compiling=False), 0, ['control', 'demo/v'])[0],
           [f'{n} default fires absent (incomplete run) complete=False [] not compiled: demo' for n in ('control', 'demo/v')])


def main():
    args = sys.argv[1:]
    K.help_requested(args, __doc__)
    if args:
        raise SystemExit(__doc__)
    failed = 0
    for n, (name, got, want) in enumerate(checks(), 1):
        if callable(got):
            try:
                got = got()
            except (Exception, SystemExit) as e:
                got = f'raised {type(e).__name__}: {e}'
        if got == want:
            print(f'ok      {name}')
            continue
        failed += 1
        print(f'FAILED  {name}')
        print(f'    want {want!r}')
        print(f'    got  {got!r}')
    print(f'selfcheck: {n} checks, {failed} failed')
    raise SystemExit(1 if failed else 0)


if __name__ == '__main__':
    main()
