"""Hold every rule of gates.py to a synthetic log or a fake run.

usage: python3 -I tools/gatescheck.py [--inject] [--quiet]

Each rule that reads a log (the test counts, a failed test and where it
panicked, a compiler error and its place, a file fmt would change) is held to
a synthetic log it must read and logs it must not. Each rule that judges a
gate (a command's status, an example that prints nothing or not its line, a
leg's grep, `--first`, a gate that does not exist) is held to runs whose
commands write canned output. The real runner is held to one command that
writes to both streams and fails, and to one that cannot start.

With `--inject`, each rule is put back broken in gates.py's source, one at a
time, and the run fails on any break the checks do not see. Prints one line
per check, or with `--quiet` only the failures and the count; exits 1 on any
failure.
"""
import contextlib
import io
import os
import re
import sys
import tempfile
import types

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gates  # noqa: E402

PASS_LOG = '''   Compiling x v0.1.0
     Running unittests src/lib.rs (target/debug/deps/x-1)

running 3 tests
test a ... ok
test b ... ok
test c ... ignored

test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/determinism.rs (target/debug/deps/determinism-2)

running 1 test
test rendering_is_stable_across_targets ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
'''
FAIL_LOG = '''running 2 tests
test good ... ok
test bad_one ... FAILED

failures:

---- bad_one stdout ----

thread 'bad_one' panicked at crates/x/src/lib.rs:10:5:
assertion failed

failures:
    bad_one

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
'''
CLIPPY_LOG = '''    Checking x v0.1.0
error: unused variable: `y`
  --> crates/x/src/lib.rs:3:9
   |
3  |     let y = 1;
   |

error: aborting because of the error above

error[E0425]: cannot find value `z` in this scope
 --> crates/x/src/main.rs:7:5

error: could not compile `x` (lib) due to 2 previous errors
'''
FMT_LOG = '''Diff in /work/crates/x/src/lib.rs:12:
 fn a() {
-    let x=1;
+    let x = 1;
 }
Diff in /work/crates/x/src/main.rs at line 4:
'''


def fake_execute(outputs):
    """execute() for the checks: each command writes its canned output and returns its status."""
    def run(argv, log, cwd):
        text, code = outputs.get(' '.join(argv), ('', 0))
        log.write(f'$ {" ".join(argv)}\n{text}')
        return code
    return run


def run_main(mod, argv):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        try:
            code = mod.main(argv)
        except Exception as exc:  # a broken rule may crash; that is an observation too
            code = f'crash: {type(exc).__name__}'
    return code, out.getvalue()


def observations(mod, logdir):
    """What the module shows: what it reads from each synthetic log, and what main prints over fake runs."""
    obs = {'pass counts': mod.counts(PASS_LOG), 'fail counts': mod.counts(FAIL_LOG), 'no counts': mod.counts(CLIPPY_LOG),
           'fail details': mod.details(FAIL_LOG, '/work'), 'clippy details': mod.details(CLIPPY_LOG, '/work'),
           'fmt details': mod.details(FMT_LOG, '/work'), 'pass details': mod.details(PASS_LOG, '/work')}
    det = 'cargo test -p tinker-pdf --test determinism'
    ex = {f'cargo run -p tinker-pdf --example {n}': (f'{line}\n', 0) for n, line in mod.EXAMPLES}
    cases = {
        'all green': {det: (PASS_LOG, 0), **ex},
        'a failing test': {det: (FAIL_LOG, 101)},
        'green but silent': {det: ('test result: ok. 1 passed; 0 failed; 0 ignored\n', 0)},
        'an example that prints nothing': {**ex, 'cargo run -p tinker-pdf --example open': ('', 0)},
        'an example without its line': {**ex, 'cargo run -p tinker-pdf --example render': ('something else\n', 0)},
    }
    real = mod.execute
    for name, outputs in cases.items():
        mod.execute = fake_execute(outputs)
        obs[name] = run_main(mod, ['determinism', 'examples', '--log', logdir, '--repo', '/work'])
        if name == 'all green':
            with open(os.path.join(logdir, 'determinism.log'), encoding='utf-8') as f:
                obs['log'] = f.read()
        obs[name + ', first'] = run_main(mod, ['determinism', 'examples', '--first', '--log', logdir, '--repo', '/work'])
    mod.execute = real
    obs['no such gate'] = run_main(mod, ['nonesuch', '--log', logdir])
    obs['list'] = run_main(mod, ['--list'])
    # the real runner, on a command that writes to both streams and fails, and on one that cannot start
    path = os.path.join(logdir, 'real.log')
    with open(path, 'w', encoding='utf-8') as log:
        status = mod.execute([sys.executable, '-c', 'import sys; print("to stdout"); sys.stderr.write("to stderr\\n");'
                              ' sys.exit(3)'], log, logdir)
    with open(path, encoding='utf-8') as f:
        obs['real'] = (status, f.read())
    with open(path, 'w', encoding='utf-8') as log:
        obs['real missing'] = mod.execute([os.path.join(logdir, 'no-such-program')], log, logdir)
    mod.execute = fake_execute({det: ('cannot run cargo: [Errno 2] No such file or directory\n', 127)})
    obs['cannot start'] = run_main(mod, ['determinism', '--log', logdir, '--repo', '/work'])
    mod.execute = real
    # cargo found in CARGO_HOME when PATH lacks it, and left alone when PATH has it
    home = os.path.join(logdir, 'cargo-home')
    os.makedirs(os.path.join(home, 'bin'), exist_ok=True)
    os.makedirs(os.path.join(logdir, 'on-path'), exist_ok=True)
    for d in (os.path.join(home, 'bin'), os.path.join(logdir, 'on-path')):
        with open(os.path.join(d, 'cargo'), 'w', encoding='utf-8') as f:
            f.write('#!/bin/sh\n')
        os.chmod(os.path.join(d, 'cargo'), 0o755)
    saved = dict(os.environ)
    try:
        os.environ.update(CARGO_HOME=home, PATH=os.path.join(logdir, 'nowhere'))
        argv, env = mod.resolve(['cargo', 'fmt'])
        obs['resolve from home'] = (argv, env and env['PATH'].split(os.pathsep)[0])
        os.environ['PATH'] = os.path.join(logdir, 'on-path')
        obs['resolve on path'] = mod.resolve(['cargo', 'fmt'])
        obs['resolve another program'] = mod.resolve(['git', 'log'])
    finally:
        os.environ.clear()
        os.environ.update(saved)
    return obs


# Each rule put back broken: what it is, its source text in gates.py and the broken form.
INJECT = [
    ('test counts add over every result line', 'return tuple(sum(int(x[i]) for x in found) for i in range(3))',
     'return tuple(int(found[0][i]) for i in range(3))'),
    ('a log without a result line has no counts', '    if not found:\n        return None',
     '    if not found:\n        return (0, 0, 0)'),
    ('a failed test is named', r"""FAILED_TEST = re.compile(r'^test (\S+) \.\.\. FAILED$', re.M)""",
     "FAILED_TEST = re.compile(r'(?!)')"),
    ('a failed test says where it panicked', """(f' (panicked at {panicked[name]})' if name in panicked else '')""",
     "''"),
    ('a compiler error with a code is listed', r"""ERROR = re.compile(r'^(error(?:\[E\d+\])?: .+)$', re.M)""",
     r"""ERROR = re.compile(r'^(error: .+)$', re.M)"""),
    ('an error carries its place', """(f' ({loc.group(1)})' if follows else '')""", "''"),
    ("an error's place is its own, not the next error's", 'follows = loc and not ERROR.search(text, m.end(), loc.start())',
     'follows = loc'),
    ('a file fmt would change is listed', r"""FMT_DIFF = re.compile(r'^Diff in (\S+?)(?::(\d+):| at line (\d+):)$', re.M)""",
     "FMT_DIFF = re.compile(r'(?!)')"),
    ('a path fmt prints is relative to the repository',
     'path = os.path.relpath(m.group(1), repo) if os.path.isabs(m.group(1)) else m.group(1)', 'path = m.group(1)'),
    ("rustfmt's two ways of giving the line are both read", 'at line {m.group(2) or m.group(3)}', 'at line {m.group(2)}'),
    ('a command that fails fails its gate', "            elif code != 0:\n                problems.append(f'`",
     "            elif False:\n                problems.append(f'`"),
    ('a command that cannot start says why', "            if code == 127 and text.startswith('cannot run '):",
     '            if False:'),
    ('cargo is found in CARGO_HOME when PATH lacks it', "    if argv[0] != 'cargo' or shutil.which('cargo'):",
     '    if True:'),
    ("CARGO_HOME's folder goes on the child's PATH",
     "dict(os.environ, PATH=os.path.join(home, 'bin') + os.pathsep + os.environ.get('PATH', ''))", 'None'),
    ('a command that must print and prints nothing fails', '            if prints and not text.strip():',
     '            if False:'),
    ('a line a gate must print and does not fails it', '                if not re.search(pattern, text, re.M):',
     '                if False:'),
    ('--first stops at the first failure', '            if args.first:\n                break',
     '            if False:\n                break'),
    ('a gate that does not exist exits 2',
     "        print(f'no gate called {\", \".join(unknown)}; the gates are {\", \".join(names)}')\n        return 2",
     "        print(f'no gate called {\", \".join(unknown)}; the gates are {\", \".join(names)}')\n        return 0"),
    ('a failed run exits 1', '    return 1 if failed else 0', '    return 0'),
    ("a command's output goes to its gate's log", 'subprocess.run(argv, cwd=cwd, stdout=log, stderr=subprocess.STDOUT, env=env)',
     'subprocess.run(argv, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, env=env)'),
    ("a command's errors go to its gate's log too", 'subprocess.run(argv, cwd=cwd, stdout=log, stderr=subprocess.STDOUT, env=env)',
     'subprocess.run(argv, cwd=cwd, stdout=log, stderr=subprocess.DEVNULL, env=env)'),
    ("a command's status is its gate's", '.returncode\n    except OSError as exc:', '.returncode * 0\n    except OSError as exc:'),
    ('a command that cannot start fails', "        log.write(f'cannot run {argv[0]}: {exc}\\n')\n        return 127",
     "        log.write(f'cannot run {argv[0]}: {exc}\\n')\n        return 0"),
    ('each gate counts once', '        ran += 1', '        ran += 2'),
    ("a gate's log starts after the command line", "text = f.read().split('\\n', 1)[-1]", 'text = f.read()'),
]


def load(src):
    mod = types.ModuleType('gates_injected')
    mod.__file__ = os.path.join(HERE, 'gates.py')
    exec(compile(src, mod.__file__, 'exec'), mod.__dict__)
    return mod


def main(argv):
    quiet = '--quiet' in argv
    total = failed = 0

    def check(name, cond, detail=''):
        nonlocal total, failed
        total += 1
        failed += not cond
        if not cond or not quiet:
            print(f"{'ok  ' if cond else 'FAIL'} {name}" + (f': {detail}' if detail and not cond else ''))

    with tempfile.TemporaryDirectory() as logdir:
        obs = observations(gates, logdir)
        check('counts add over every result line', obs['pass counts'] == (3, 0, 1), str(obs['pass counts']))
        check('a failing run counts its failure', obs['fail counts'] == (1, 1, 0), str(obs['fail counts']))
        check('a log without a result line has no counts', obs['no counts'] is None)
        check('a failed test is named with where it panicked',
              obs['fail details'] == ['failed: bad_one (panicked at crates/x/src/lib.rs:10:5)'], str(obs['fail details']))
        check('each compiler error is listed with its own place', obs['clippy details'] == [
            'error: unused variable: `y` (crates/x/src/lib.rs:3:9)', 'error: aborting because of the error above',
            'error[E0425]: cannot find value `z` in this scope (crates/x/src/main.rs:7:5)',
            'error: could not compile `x` (lib) due to 2 previous errors'], str(obs['clippy details']))
        check('each file fmt would change is listed, relative to the repository', obs['fmt details'] == [
            'fmt would change crates/x/src/lib.rs at line 12', 'fmt would change crates/x/src/main.rs at line 4'],
            str(obs['fmt details']))
        check('a green log has no details', obs['pass details'] == [], str(obs['pass details']))
        code, out = obs['all green']
        check('green gates: exit 0, one line each, the counts and the total',
              code == 0 and re.search(r'^ok    determinism +\d+\.\ds  3 passed, 0 failed, 1 ignored$', out, re.M) is not None
              and re.search(r'^ok    examples ', out, re.M) is not None and out.rstrip().endswith('2 gates: 2 ok'), out)
        code, out = obs['a failing test']
        check('a failing test: exit 1, its exit status, its name and its log',
              code == 1 and '`test -p tinker-pdf --test determinism` exited 101' in out
              and 'failed: bad_one (panicked at crates/x/src/lib.rs:10:5)' in out
              and re.search(r'log: \S*determinism\.log', out) is not None
              and out.rstrip().endswith('2 gates: 0 ok, 2 failed (determinism, examples)'), out)
        code, out = obs['a failing test, first']
        check('--first stops after the first gate that fails',
              code == 1 and 'examples' not in out and out.rstrip().endswith('1 gates: 0 ok, 1 failed (determinism)'), out)
        code, out = obs['green but silent']
        check('a green run without the lines its leg greps for fails',
              code == 1 and 'did not print /rendering_is_stable_across_targets [.]{3} ok/' in out, out)
        code, out = obs['an example that prints nothing']
        check('an example that prints nothing fails',
              code == 1 and '`run -p tinker-pdf --example open` printed nothing' in out, out)
        code, out = obs['an example without its line']
        check('an example without its line fails',
              code == 1 and '`run -p tinker-pdf --example render` did not print /rendered\\ \\ /' in out, out)
        code, out = obs['no such gate']
        check('a gate that does not exist: exit 2, naming the gates', code == 2 and 'the gates are fmt, clippy' in out, out)
        code, out = obs['list']
        check("--list prints every gate of ci.yml's test job and runs none",
              code == 0 and len(out.strip().split('\n')) == len(gates.GATES)
              and out.startswith('fmt: cargo fmt --all -- --check'), out)
        check("each command's output goes to its gate's log, after the command",
              obs['log'].startswith('$ cargo test -p tinker-pdf --test determinism\n')
              and 'test result: ok. 1 passed' in obs['log'], obs['log'][-300:])
        check("the real runner logs both streams and returns the command's status",
              obs['real'][0] == 3 and 'to stdout' in obs['real'][1] and 'to stderr' in obs['real'][1], str(obs['real']))
        check('a command that cannot start: status 127', obs['real missing'] == 127, str(obs['real missing']))
        code, out = obs['cannot start']
        check('a command that cannot start says why, not only its status',
              code == 1 and 'cannot run cargo: [Errno 2] No such file or directory' in out and 'exited 127' not in out, out)
        resolved, first = obs['resolve from home']
        check('cargo missing from PATH is run from CARGO_HOME, its folder first on the PATH',
              resolved[0].endswith(os.path.join('cargo-home', 'bin', 'cargo')) and resolved[1:] == ['fmt']
              and first is not None and first.endswith(os.path.join('cargo-home', 'bin')), str(obs['resolve from home']))
        check('cargo on PATH is run as it is', obs['resolve on path'] == (['cargo', 'fmt'], None), str(obs['resolve on path']))
        check('another program is run as it is', obs['resolve another program'] == (['git', 'log'], None),
              str(obs['resolve another program']))
        if '--inject' in argv:
            src = open(os.path.join(HERE, 'gates.py'), encoding='utf-8').read()
            base = observations(load(src), logdir)
            check('inject: the source as it is shows what the plain run saw',
                  {k: v for k, v in base.items() if k != 'real'} == {k: v for k, v in obs.items() if k != 'real'})
            for name, old, new in INJECT:
                if src.count(old) != 1:
                    check(f'inject: {name}: its source is no longer where the injection expects it', False,
                          f'{src.count(old)} matches for {old[:60]!r}')
                    continue
                broken = observations(load(src.replace(old, new)), logdir)
                diff = [k for k in base if broken.get(k) != base[k]]
                check(f'inject: {name} broken shows in {", ".join(diff[:4]) or "nothing"}', bool(diff))
    print(f'{total} checks, {failed} failed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
