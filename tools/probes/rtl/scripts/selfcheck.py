"""The kit's own checks: its readers on what cargo prints on Linux and on
Windows, which must read alike.

usage: python3 -I selfcheck.py

Builds nothing and runs nothing; it prints one line a check and exits 1 when
any does not hold. The suites hold their comparisons in their own tests
(`cargo test -p tinker-pdf --test rtl_probes`, `cargo test -p
tinker-pdf-shape --test rtl_lines`); these hold the scripts, which no cargo
test may run (ruling 13: no test spawns a program).
"""
import importlib.util
import os
import sys


def load_kit():
    sys.dont_write_bytecode = True
    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location('rtlkit', os.path.join(here, 'rtlkit.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


K = load_kit()

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


def main():
    args = sys.argv[1:]
    K.help_requested(args, __doc__)
    if args:
        raise SystemExit(__doc__)
    failed = 0
    for n, (name, got, want) in enumerate(checks(), 1):
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
