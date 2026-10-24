"""Hold every part of brief.py to a fixture.

usage: python3 -I tools/roadmap/briefcheck.py [--inject] [--quiet]

Builds a small roadmap and tree in a temporary folder, as selfcheck.py does
for the sweep. Its steps wait on a decided H row, an open one with a default
and one without, a landed step, a split one, an open row and a row that has
left the roadmap; cite a line, the same line twice, a line past a file's end,
a missing file and a range too long to print whole; name a defined function,
a plain word the code defines, a name only a comment uses, names other steps
build, a test an exit adds and a name nothing has; cite rulings; and share
files with a step after them, one no wait orders and more than a list shows.
One row's Today is longer than a step's brief keeps, one row is not yet
reviewed, and one step creates a file another step uses. Each part of the
brief must give what the fixture says it must.

The commits since the base come from `git`: the fixture's history is
`git log`'s output written out, so no commit is made, and a folder inside
another checkout and a checkout without the base are each read through `git`
itself.

With `--inject`, each rule of brief.py is put back broken in the source, one
at a time, and the run fails on any break that changes nothing the
invocations show. Prints one line per check, or with `--quiet` only the
failures and the count; exits 1 on any failure.
"""
import contextlib
import io
import os
import shutil
import subprocess
import sys
import tempfile
import types

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import brief  # noqa: E402
import sweep  # noqa: E402

UNREVIEWED = '*Steps not yet reviewed; the review runs once all the work is done.*'
HEAD = '| Step | Kind | What | Where | Exit | Depends on |\n| --- | --- | --- | --- | --- | --- |'


def row(rid, steps, today='Absent.', reviewed=True):
    out = [f'## {rid} — Row {rid}', '', f'**Row {rid}**: the fixture.', '', f'**Today.** {today}', '',
           '**Exit criterion.** a test passes', '', '**Doc:** — · **Depends on:** —', '']
    out += [UNREVIEWED, ''] if not reviewed else []
    out += [HEAD] + [f'| {sid} | {kind} | {what} | {where} | {exit_} | {deps or "—"} |'
                     for sid, kind, what, where, exit_, deps in steps]
    return '\n'.join(out + ['', '**Closes the row:** Its steps meet the exit.', ''])


LIB = ['//! The fixture crate.', 'pub struct ExistingType;', 'pub fn existing_fn() {}', 'fn abbreviate() {}',
       '// UsedOnlyName appears only in this comment.'] + [f'// line {i}' for i in range(6, 31)]
L, OTHER, MANY = '`crates/a/src/lib.rs`', '`crates/a/src/other.rs`', '`crates/a/src/many.rs`'
FRESH = '`crates/a/src/fresh.rs` (new)'
TREE = {
    'crates/a/src/lib.rs': '\n'.join(LIB) + '\n',
    'crates/a/src/other.rs': '//! Another file.\n',
    'crates/a/src/many.rs': '//! A file many steps change.\n',
    'docs/rulings.md': '\n'.join(['# Engineering rulings', '', 'The rulings.', '', '## Rulings', '',
                                  '1. **Never panic.** Text.', '', "2. **Degrade, don't fail.** Text.", '',
                                  '13. **Verification is first-party.** Text', '   that runs on.', '',
                                  '## How to add a ruling', '', 'Steps.', '']),
}
ROWS = {'MS': [
    row('MS-01', [
        ('MS-01.1', 'build', 'Uses `ExistingType` (`crates/a/src/lib.rs:2`), `existing_fn`, `abbreviate`, `NewThing` '
         '(new), `LaterThing` and `NotedThing`, as ruling 13 and rulings 1 and 2 say. *Note:* Past the end '
         '(`crates/a/src/lib.rs:99`), a missing file (`crates/a/src/gone.rs:3`), a long range '
         '(`crates/a/src/lib.rs:6-26`), `UsedOnlyName` and `NoSuchName`.', L,
         '`lib.rs` `a_new_test_name` passes, as `ExistingType` (`crates/a/src/lib.rs:2`) says.',
         'H-01, H-02, H-03, MS-04'),
        ('MS-01.2', 'build', 'Builds `LaterThing` after it.', L, 'A test passes.', 'MS-01.1'),
        ('MS-01.3', 'build', 'A change no wait orders. *Note:* `NotedThing` (new) lands here.', L, 'A test passes.', ''),
        ('MS-01.5', 'build', 'Creates a file.', FRESH, 'A test passes.', ''),
    ]),
    row('MS-02', [
        ('MS-02.1', 'build', "Reads MS-01.1's work.", OTHER, 'A test passes.', 'MS-01.4, MS-01.2'),
        ('MS-02.2', 'build', 'Uses the file.', FRESH, 'A test passes.', 'MS-01'),
        ('MS-02.3', 'build', 'A change.', OTHER, 'A test passes.', 'MS-05.1, MS-99'),
    ], reviewed=False),
    row('MS-03', [(f'MS-03.{n}', 'build', f'Change {n}.', MANY, 'A test passes.', '') for n in range(1, 15)]),
    row('MS-04', [('MS-04.1', 'build', 'A change.', OTHER, 'A test passes.', '')], today='Long. ' * 400),
    row('MS-05', [('MS-05.1a', 'build', 'A part.', OTHER, 'A test passes.', ''),
                  ('MS-05.1b', 'build', 'A part.', OTHER, 'A test passes.', 'MS-05.1a')]),
]}
H_MD = '\n'.join(['# People', '', '## Decisions', '',
                  '| ID | Decision | Why the owner | Blocks | Default until decided |',
                  '| --- | --- | --- | --- | --- |',
                  '| H-01 | **Decided** by the owner: yes | policy | MS-01 | the default |',
                  '| H-02 | A decision | policy | MS-01 | build toward A |',
                  '| H-03 | Another | policy | MS-01 | none — until then |', ''])
NG_MD = '# Hard limits\n\n**NG-01 — A limit.** Never.\n'
FAKE_LOG = ('\x00abc1234 2026-10-25 change the lib\n\ncrates/a/src/lib.rs\n'
            '\x00def5678 2026-10-24 another change\n\ncrates/a/src/other.rs\ncrates/a/src/lib.rs\n')


def build(repo):
    for rel, text in TREE.items():
        path = os.path.join(repo, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(text)
    os.makedirs(os.path.join(repo, 'docs', 'roadmap'), exist_ok=True)
    counts = []
    for tier in sweep.TIERS:
        body = '\n'.join(ROWS.get(tier, []))
        with open(os.path.join(repo, 'docs', 'roadmap', f'{tier}.md'), 'w', encoding='utf-8') as f:
            f.write(f'# Tier {tier}\n\nThe fixture.\n\n{body}\n')
        steps = sum(1 for line in body.split('\n') if sweep.STEP_LINE.match(line))
        counts.append(f'| {tier} | Tier {tier} | [roadmap/{tier}.md](roadmap/{tier}.md) | {len(ROWS.get(tier, []))} | '
                      f'{len(ROWS.get(tier, []))} | {steps} |')
    index = ['# Roadmap', '', '| Prefix | Tier | File | Rows | Cut into steps | Steps |',
             '| --- | --- | --- | ---: | ---: | ---: |'] + counts + [
        f'| H | People | [roadmap/H.md](roadmap/H.md) | {H_MD.count(chr(10) + "| H-")} | — | — |',
        '| NG | Limits | [roadmap/NG.md](roadmap/NG.md) | 1 | — | — |', '']
    with open(os.path.join(repo, 'docs', 'ROADMAP.md'), 'w', encoding='utf-8') as f:
        f.write('\n'.join(index))
    for name, text in (('H.md', H_MD), ('NG.md', NG_MD)):
        with open(os.path.join(repo, 'docs', 'roadmap', name), 'w', encoding='utf-8') as f:
            f.write(text)


class FakeGit:
    """`git` for the fixture: the checkout is the fixture, and its log is FAKE_LOG."""
    TimeoutExpired = subprocess.TimeoutExpired

    def __init__(self, repo):
        self.repo = repo

    def run(self, args, **kwargs):
        if 'rev-parse' in args:
            return types.SimpleNamespace(returncode=0, stdout=self.repo + '\n', stderr='')
        return types.SimpleNamespace(returncode=0, stdout=FAKE_LOG, stderr='')


def run_main(mod, argv, git=None):
    mod.subprocess = git or subprocess
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        try:
            code = mod.main(argv)
        except Exception as exc:  # a broken rule may crash; that is an observation too
            code = f'crash: {type(exc).__name__}'
    mod.subprocess = subprocess
    return code, out.getvalue()


def snapshot(root):
    return sorted((os.path.relpath(os.path.join(d, n), root), os.path.getsize(os.path.join(d, n)),
                   os.path.getmtime(os.path.join(d, n))) for d, _, names in os.walk(root) for n in names)


class Fixtures:
    def __init__(self, root):
        self.main = os.path.join(root, 'main')
        build(self.main)
        # a fixture inside another checkout, and a checkout without the base
        outer = os.path.join(root, 'outer')
        os.makedirs(outer)
        subprocess.run(['git', 'init', '-q', outer], check=True, capture_output=True)
        self.inside = os.path.join(outer, 'repo')
        build(self.inside)
        self.nobase = os.path.join(root, 'nobase')
        build(self.nobase)
        subprocess.run(['git', 'init', '-q', self.nobase], check=True, capture_output=True)
        self.broken = os.path.join(root, 'broken')
        build(self.broken)
        os.remove(os.path.join(self.broken, 'docs', 'roadmap', 'H.md'))
        self.accepted = os.path.join(root, 'accepted')
        build(self.accepted)
        os.makedirs(os.path.join(self.accepted, 'tools', 'roadmap'))
        with open(os.path.join(self.accepted, 'tools', 'roadmap', 'accepted.tsv'), 'w', encoding='utf-8') as f:
            f.write('idents\tMS-01.1\tnotes: NoSuchName\tthe fixture\n')
        self.calls = {
            'MS-01.1': (['MS-01.1', '--repo', self.main], FakeGit(self.main)),
            'MS-01.1 all': (['MS-01.1', '--all', '--repo', self.main], FakeGit(self.main)),
            'MS-01.5': (['MS-01.5', '--repo', self.main], FakeGit(self.main)),
            'MS-02.1': (['MS-02.1', '--repo', self.main], FakeGit(self.main)),
            'MS-02.2': (['MS-02.2', '--repo', self.main], FakeGit(self.main)),
            'MS-02.3': (['MS-02.3', '--repo', self.main], FakeGit(self.main)),
            'MS-03.1': (['MS-03.1', '--repo', self.main], FakeGit(self.main)),
            'MS-03.1 all': (['MS-03.1', '--all', '--repo', self.main], FakeGit(self.main)),
            'MS-04.1': (['MS-04.1', '--repo', self.main], FakeGit(self.main)),
            'row MS-01': (['MS-01', '--repo', self.main], None),
            'row MS-02': (['MS-02', '--repo', self.main], None),
            'row MS-04': (['MS-04', '--repo', self.main], None),
            'file': (['--file', 'crates/a/src/lib.rs', '--repo', self.main], None),
            'unknown': (['NO-01.1', '--repo', self.main], None),
            'neither': (['--repo', self.main], None),
            'both': (['MS-01.1', '--file', 'crates/a/src/lib.rs', '--repo', self.main], None),
            'inside another checkout': (['MS-01.1', '--repo', self.inside], None),
            'no base': (['MS-01.1', '--repo', self.nobase], None),
            'unreadable': (['MS-01.1', '--repo', self.broken], None),
            'accepted': (['MS-01.1', '--repo', self.accepted], FakeGit(self.accepted)),
        }

    def signature(self, mod):
        return {name: run_main(mod, argv, git) for name, (argv, git) in self.calls.items()}


# Each rule put back broken: what the rule is, its source text and the broken form.
INJECT = [
    ('a bold decided is a decided H row', r"""DECIDED = re.compile(r'\*\*(?:decided|authori[sz]ed|answered|done)\b', re.I)""",
     "DECIDED = re.compile(r'(?!)')"),
    ('an open H row with a default says so', "        if h['has default']:", "        if not h['has default']:"),
    ('a landed step is landed', "    if kind == 'landed':\n        return 'landed'",
     "    if kind == 'landed':\n        return 'not landed'"),
    ('a split step names its parts', "return 'split into ' + ', '.join(sweep.split_parts(rm, dep)) + ', none landed'",
     "return 'not landed'"),
    ('an open row counts its steps', """return f'open, {left} step{"" if left == 1 else "s"} left'""", "return 'open'"),
    ('a row that left the roadmap says so', "return 'not in the roadmap: its row has left it, or never was'",
     "return 'names nothing in the roadmap'"),
    ('a pointer is printed once', 'if a is None or (ref, a, b) in seen:', 'if a is None:'),
    ('a missing file says so', "('a step creates the file' if made else 'no such file in the tree')",
     "'a step creates the file'"),
    ('a line past the end says how long the file is', 'if lo < 1 or hi < lo or hi > len(lines):',
     'if lo < 1 or hi < lo:'),
    ('a long range is cut', '            if len(shown) > SPAN:', '            if False:'),
    ('a function is a definition', "DEF_KINDS = r'(?:fn|struct|enum|trait|const|static|type|mod|union)'",
     "DEF_KINDS = r'(?:struct|enum|trait|const|static|type|mod|union)'"),
    ('a name only used says where', '        elif n in tree.idents and uses[n]:', '        elif False:'),
    ("a test an exit adds is the exit's", "        elif fields[n] == {'exit test'} and sweep.SNAKE.match(n):",
     '        elif False:'),
    ("a name another step's What builds names that step",
     '        if name in sweep.idents_in(s.what):\n            return s.id',
     '        if False:\n            return s.id'),
    ('a name another step marks new names that step',
     '                if name in sweep.idents_in(m.group(1)):\n                    return s.id',
     '                if False:\n                    return s.id'),
    ('a plain word the code defines is a name', '        if sweep.WORD_RE.fullmatch(span) and len(span) > 2:',
     '        if False:'),
    ('a cited ruling is found', r"""RULING_CITED = re.compile(r'\brulings? ((?:\d+(?:, | and | or ))*\d+)\b', re.I)""",
     "RULING_CITED = re.compile(r'(?!)')"),
    ('every ruling of a list is found', "for n in re.findall(r'\\d+', group)})",
     "for n in re.findall(r'\\d+', group)[:1]})"),
    ('a ruling ends at the next heading',
     "stops = sorted({h[1] for h in heads} | {i + 1 for i, l in enumerate(lines) if l.startswith('#')})",
     'stops = sorted({h[1] for h in heads})'),
    ("a ruling's range leaves out the blank lines after it",
     '        while end > start and not lines[end - 1].strip():', '        while False:'),
    ('a step before is before', "    if other in ctx['before'][sid]:\n        return 'before'",
     "    if False:\n        return 'before'"),
    ('a step after is after', "    if sid in ctx['before'][other]:\n        return 'after'",
     "    if False:\n        return 'after'"),
    ('a long list says how many it left out', '    if full or len(items) <= LIST:', '    if True:'),
    ('a folder inside another checkout is no checkout',
     'if top.returncode != 0 or os.path.realpath(top.stdout.strip()) != os.path.realpath(repo):',
     'if top.returncode != 0:'),
    ('each commit names the files it touched',
     """out.append(f'  {clip(head, 120)}: {", ".join(f for f in files if f in paths)}')""",
     "out.append(f'  {clip(head, 120)}')"),
    ("git's refusal is reported", "return None, err[-1] if err else f'git log exited {log.returncode}'",
     'return [], None'),
    ('the steps waiting on a step are listed', '    waiting = [x.id for x in rm.steps.values() if s.id in x.depends]',
     '    waiting = []'),
    ('the steps waiting on its whole row are listed', '    whole = [x.id for x in rm.steps.values() if s.row in x.depends]',
     '    whole = []'),
    ('the texts that name it are listed', 'and s.id in sweep.STEP_IN_TEXT.findall(text)} - set(waiting))',
     'and False} - set(waiting))'),
    ("only the step's own findings are shown", '            if f.subject in subjects and f.key() not in seen:',
     '            if f.key() not in seen:'),
    ('an accepted finding is marked accepted', '                if f.key() in accepted:', '                if False:'),
    ("a long field of the row is cut in a step's brief", '        if limit is None or len(text) <= limit:',
     '        if True:'),
    ('only the steps near it are summarised', '        if full or abs(i - at) <= NEAR:', '        if True:'),
    ('a step that creates a file says so',
     """state += ', created here' if s in first else f', created by {", ".join(x.id for x in first) or "no step"}'""",
     "state += ''"),
    ('a step brief says whether its row was reviewed',
     """row {r.id} ({"reviewed" if r.reviewed else "not yet reviewed"}) · '""", """row {r.id} ("reviewed") · '"""),
    ('a row brief says whether it was reviewed',
     """{r.title} · {"reviewed" if r.reviewed else "not yet reviewed"} · {where_of(r, repo)}'""",
     """{r.title} · {where_of(r, repo)}'"""),
    ("a row brief lists each step's waits",
     "        out += [f'    waits: ' + '; '.join(f'{d} {wait_state(rm, d)}' for d in s.depends)] if s.depends else []",
     '        out += []'),
    ('a row brief lists the steps of other rows that wait on it',
     '                      for d in x.depends if d == r.id or d in ids})', '                      for d in x.depends if False})'),
    ('a file brief says what each step comes after', "        waits = [x.id for x in steps if x.id in ctx['before'][s.id]]",
     '        waits = []'),
    ('a file brief lists the pairs no wait orders', "             if order_of(ctx, a.id, b.id) == 'unordered']",
     '             if False]'),
    ('an ID that names nothing exits 2',
     "        print(f'{args.id}: no step or row of that ID in the roadmap')\n        return 2",
     "        print(f'{args.id}: no step or row of that ID in the roadmap')\n        return 0"),
    ('one ID or one file, never both or neither', '    if bool(args.id) == bool(args.file):', '    if False:'),
    ('a roadmap it cannot read exits 2', '    if rm.errors:', '    if False:'),
]


def load(src):
    mod = types.ModuleType('brief_injected')
    mod.__file__ = os.path.join(HERE, 'brief.py')
    exec(compile(src, mod.__file__, 'exec'), mod.__dict__)
    return mod


class Report:
    def __init__(self, quiet):
        self.quiet, self.total, self.failed = quiet, 0, 0

    def check(self, name, cond, detail=''):
        self.total += 1
        if not cond:
            self.failed += 1
        if not cond or not self.quiet:
            print(f"{'ok  ' if cond else 'FAIL'} {name}" + (f': {detail}' if detail and not cond else ''))
        return cond


def main(argv):
    rep = Report('--quiet' in argv)
    with tempfile.TemporaryDirectory() as root:
        fx = Fixtures(root)
        before = snapshot(root)
        sig = fx.signature(brief)
        out = {name: text for name, (code, text) in sig.items()}
        code = {name: c for name, (c, text) in sig.items()}

        def has(name, *texts):
            missing = [t for t in texts if t not in out[name]]
            return rep.check(f'{name} says {" / ".join(t.strip()[:60] for t in texts)}', not missing,
                             f'missing {missing!r} in:\n{out[name][-1500:]}')

        def lacks(name, *texts):
            present = [t for t in texts if t in out[name]]
            return rep.check(f'{name} does not say {" / ".join(t.strip()[:60] for t in texts)}', not present,
                             f'present {present!r}')

        for name in ('MS-01.1', 'MS-02.1', 'MS-03.1', 'MS-04.1', 'row MS-01', 'row MS-02', 'row MS-04', 'file'):
            rep.check(f'{name}: exit 0', code[name] == 0, f'exit {code[name]}: {out[name][-500:]}')
        has('MS-01.1', 'MS-01.1 · build · row MS-01 (reviewed) · docs/roadmap/MS.md:')
        has('MS-02.1', 'MS-02.1 · build · row MS-02 (not yet reviewed)')
        # waits
        has('MS-01.1', 'H-01: decided: **Decided** by the owner: yes',
            'H-02: open; the work builds toward its default, build toward A; A decision',
            'H-03: open, with no default: the step waits for it; Another', 'MS-04: open, 1 step left')
        has('MS-02.1', 'MS-01.4: landed', 'MS-01.2: not landed')
        has('MS-02.3', 'MS-05.1: split into MS-05.1a, MS-05.1b, none landed',
            'MS-99: not in the roadmap: its row has left it, or never was')
        # the lines it cites
        has('MS-01.1', '  crates/a/src/lib.rs:2\n    2: pub struct ExistingType;\n',
            '  crates/a/src/lib.rs:99: crates/a/src/lib.rs has 31 lines',
            '  crates/a/src/gone.rs:3: no such file in the tree',
            '  crates/a/src/lib.rs:6-26\n    6: // line 6\n', '    15: // line 15\n      …\n    26: // line 26\n')
        lacks('MS-01.1', '    16: // line 16')
        rep.check('MS-01.1 prints a line it cites twice once', out['MS-01.1'].count('  crates/a/src/lib.rs:2\n') == 1)
        # the code names it writes
        has('MS-01.1', '`ExistingType`: defined at crates/a/src/lib.rs:2', '`existing_fn`: defined at crates/a/src/lib.rs:3',
            '`abbreviate`: defined at crates/a/src/lib.rs:4',
            '`UsedOnlyName`: not defined in the code; used at crates/a/src/lib.rs:5',
            '`a_new_test_name`: not in the tree; a test its exit names', '`NewThing`: not in the tree; this step builds it',
            '`LaterThing`: not in the tree; MS-01.2 builds it', '`NotedThing`: not in the tree; MS-01.3 builds it',
            '`NoSuchName`: not in the tree, and no step builds it')
        # the rulings it cites
        has('MS-01.1', 'ruling 1, Never panic.: docs/rulings.md:7-7', "ruling 2, Degrade, don't fail.: docs/rulings.md:9-9",
            'ruling 13, Verification is first-party.: docs/rulings.md:11-12')
        # the other steps that change its files
        has('MS-01.1', '  crates/a/src/lib.rs (31 lines): 2 other steps\n    unordered: MS-01.3\n    after: MS-01.2\n')
        has('MS-01.5', '  crates/a/src/fresh.rs (new, created here): 1 other steps\n    after: MS-02.2')
        has('MS-02.2', '  crates/a/src/fresh.rs (new, created by MS-01.5): 1 other steps\n    before: MS-01.5')
        has('MS-03.1', 'unordered: ' + ', '.join(f'MS-03.{n}' for n in range(2, 14)) + ', and 1 more (--all lists them)')
        has('MS-03.1 all', 'unordered: ' + ', '.join(f'MS-03.{n}' for n in range(2, 15)) + '\n')
        # the commits since the base
        has('MS-01.1', '  abc1234 2026-10-25 change the lib: crates/a/src/lib.rs',
            '  def5678 2026-10-24 another change: crates/a/src/lib.rs\n')
        has('inside another checkout', 'cannot tell: ', 'is not a git checkout')
        has('no base', 'cannot tell: ')
        lacks('no base', 'is not a git checkout')
        # who waits on it
        has('MS-01.1', '  on this step: MS-01.2\n', '  on its whole row: MS-02.2\n', '  named in the text of: MS-02.1\n')
        # the sweep on it
        has('MS-01.1', 'FINDING   idents MS-01.1 notes: NoSuchName')
        lacks('MS-01.1', 'deps MS-02.3')
        has('MS-02.3', 'FINDING   deps MS-02.3 MS-99')
        has('accepted', 'accepted  idents MS-01.1 notes: NoSuchName (the fixture)')
        # the row in a step's brief
        has('MS-04.1', 'characters more: `brief.py MS-04` prints the row')
        lacks('row MS-04', 'characters more')
        has('MS-03.1', 'MS-03.4 build [—]: Change 4.;', 'MS-03.5 build;')
        lacks('MS-03.1', 'Change 5.')
        has('MS-03.1 all', 'Change 14.')
        # a row's brief
        has('row MS-02', 'MS-02 — Row MS-02 · not yet reviewed', '  MS-02.1 · build · depends on: MS-01.4, MS-01.2',
            '    waits: MS-01.4 landed; MS-01.2 not landed', 'Who waits on it from other rows\n  none')
        has('row MS-01', 'MS-01 — Row MS-01 · reviewed', 'MS-02.1 on MS-01.2', 'MS-02.2 on MS-01')
        # a file's brief
        has('file', 'crates/a/src/lib.rs (31 lines): 3 steps change it', '  MS-01.2 · build · after: MS-01.1',
            'Pairs no wait orders: MS-01.1 and MS-01.3, MS-01.2 and MS-01.3')
        # what it refuses
        rep.check('an ID that names nothing: exit 2, saying so', code['unknown'] == 2 and 'no step or row' in out['unknown'])
        rep.check('neither an ID nor a file: exit 2', code['neither'] == 2 and 'name one step or row' in out['neither'])
        rep.check('both an ID and a file: exit 2', code['both'] == 2 and 'name one step or row' in out['both'])
        rep.check('a roadmap it cannot read: exit 2, saying so',
                  code['unreadable'] == 2 and 'cannot read the roadmap' in out['unreadable'], out['unreadable'][-300:])
        rep.check('it changes no file', snapshot(root) == before)

        if '--inject' in argv:
            src = open(os.path.join(HERE, 'brief.py'), encoding='utf-8').read()
            base = fx.signature(load(src))
            rep.check('inject: the source as it is shows what the plain run shows', base == sig)
            for name, old, new in INJECT:
                if src.count(old) != 1:
                    rep.check(f'inject: {name}: its source is no longer where the injection expects it', False,
                              f'{src.count(old)} matches for {old[:60]!r}')
                    continue
                diff = [k for k, v in fx.signature(load(src.replace(old, new))).items() if v != base[k]]
                rep.check(f'inject: {name} broken shows in {", ".join(diff[:4]) or "nothing"}'
                          + (f' and {len(diff) - 4} more' if len(diff) > 4 else ''), bool(diff))
        shutil.rmtree(os.path.join(root, 'outer', '.git'), ignore_errors=True)
    print(f'{rep.total} checks, {rep.failed} failed')
    return 1 if rep.failed else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
