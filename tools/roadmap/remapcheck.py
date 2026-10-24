"""Hold every rule of remap.py to a fixture.

usage: python3 -I tools/roadmap/remapcheck.py [--quiet]

Builds a small roadmap and tree in a temporary folder, with BASE's copy of
each file given by a dictionary rather than by `git`, so no commit is made.
Its code file gains lines above some pointers and between others, and
changes lines that others cite; its pointers are a line, a range, a bare `:N`
after a full path, a name only one file has, a name two files have with and
without a full path before it on its line, a pointer on a line of the roadmap
written after BASE, one into a file that is gone and one into a file BASE does
not have, and a second run over the first's output. Each must
move, stay or be left as remap.py says. Prints one line per check, or with
`--quiet` only the failures and the count; exits 1 on any failure.
"""
import os
import shutil
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import remap  # noqa: E402

CODE = 'crates/a/src/lib.rs'
OLD = [f'old {n}' for n in range(1, 13)]
# Two lines inserted before line 4, line 7 rewritten, line 10 removed.
NEW = OLD[:3] + ['new a', 'new b'] + OLD[3:6] + ['seven, rewritten'] + OLD[7:9] + OLD[10:]

ROW_BASE = '\n'.join([
    '# Tier X',
    '',
    '| X-01.1 | build | Above the insertion `crates/a/src/lib.rs:2`; below it `crates/a/src/lib.rs:5`. | `x` | e | — |',
    '| X-01.2 | build | A range `crates/a/src/lib.rs:4-6`, a bare `:9` after it, and `lib.rs:12`. | `x` | e | — |',
    '| X-01.3 | build | An edited line `crates/a/src/lib.rs:7`; a range over it `crates/a/src/lib.rs:6-8`. | `x` | e | — |',
    '| X-01.4 | build | A removed end `crates/a/src/lib.rs:9-10`, a gone file `crates/a/src/gone.rs:3`. | `x` | e | — |',
    '| X-01.5 | build | Two mains: `crates/c/src/main.rs:5` then `main.rs:6`. | `x` | e | — |',
    '| X-01.8 | build | A bare `main.rs:4` with no full path before it on its line. | `x` | e | — |',
    '| X-01.6 | build | A new file `crates/a/src/fresh.rs:2`, and the roadmap `docs/roadmap/X.md:3`. | `x` | e | — |',
    '',
])
# The working copy adds one pointer written after BASE.
ROW_HERE = ROW_BASE + '| X-01.7 | build | Written since: `crates/a/src/lib.rs:11`. | `x` | e | — |\n'
INDEX = '# Roadmap\n\nThe index cites `crates/a/src/lib.rs:8`.\n'


def build():
    root = tempfile.mkdtemp(prefix='remapcheck-')
    files = {
        CODE: '\n'.join(NEW),
        'crates/b/src/main.rs': '\n'.join(f'b {n}' for n in range(1, 9)),
        'crates/c/src/main.rs': '\n'.join(['c new'] + [f'c {n}' for n in range(1, 9)]),
        'crates/a/src/fresh.rs': 'fresh 1\nfresh 2\n',
        'docs/roadmap/X.md': ROW_HERE,
        'docs/ROADMAP.md': INDEX,
    }
    for path, text in files.items():
        os.makedirs(os.path.join(root, os.path.dirname(path)), exist_ok=True)
        with open(os.path.join(root, path), 'w', encoding='utf-8') as f:
            f.write(text)
    base = {
        CODE: '\n'.join(OLD),
        'crates/b/src/main.rs': files['crates/b/src/main.rs'],
        'crates/c/src/main.rs': '\n'.join(f'c {n}' for n in range(1, 9)),
        'crates/a/src/gone.rs': 'g1\ng2\ng3\ng4\n',
        'docs/roadmap/X.md': ROW_BASE,
        'docs/ROADMAP.md': INDEX,
    }
    changed = [CODE, 'crates/c/src/main.rs', 'crates/a/src/gone.rs', 'crates/a/src/fresh.rs', 'docs/roadmap/X.md']
    return root, base, changed


def main(argv):
    quiet = '--quiet' in argv
    root, base, changed = build()
    results = []

    def check(name, ok, detail=''):
        results.append((name, ok, detail))

    try:
        lines = []
        status = remap.run(root, changed, base.get, check=True, out=lines.append)
        with open(os.path.join(root, 'docs/roadmap/X.md'), encoding='utf-8') as f:
            check('--check changes no file', f.read() == ROW_HERE)
        check('--check exits 1 when a pointer would move', status == 1, str(status))
        check('--check says "would move"', any(l.startswith('would move') for l in lines))

        moved, left, texts = remap.plan(root, changed, base.get, remap.sweep.Tree(root))
        out = dict((old, new) for _, old, new in moved)
        why = dict((span, reason) for _, span, reason in left)
        check('a pointer above the insertion stays', 'crates/a/src/lib.rs:2' not in out and 'crates/a/src/lib.rs:2' not in why)
        check('a pointer below the insertion moves by its size', out.get('crates/a/src/lib.rs:5') == 'crates/a/src/lib.rs:7', str(out))
        check('both ends of a range move', out.get('crates/a/src/lib.rs:4-6') == 'crates/a/src/lib.rs:6-8', str(out))
        check('a bare :N after a full path moves through that file', out.get(':9') == ':11', str(out))
        check('a name one file has moves', out.get('lib.rs:12') == 'lib.rs:13', str(out))
        check('a pointer at a line the change edited is left',
              why.get('crates/a/src/lib.rs:7') == 'the change edited the lines it cites', str(why))
        check('a range whose middle the change edited is left',
              why.get('crates/a/src/lib.rs:6-8') == 'the change edited the lines it cites', str(why))
        check('a range whose end the change removed is left',
              why.get('crates/a/src/lib.rs:9-10') == 'the change edited the lines it cites', str(why))
        check('a pointer into a file that is gone is left', why.get('crates/a/src/gone.rs:3') == 'its file is gone', str(why))
        check('a full path into a changed file moves', out.get('crates/c/src/main.rs:5') == 'crates/c/src/main.rs:6', str(out))
        check('a bare name two files have takes the full path before it on its line',
              out.get('main.rs:6') == 'main.rs:7', str(out))
        check('a bare name two files have, with no full path before it, is left',
              why.get('main.rs:4') == "its bare name is 2 files'", str(why))
        check('a pointer written after the base is left',
              why.get('crates/a/src/lib.rs:11') == 'its line of the roadmap has changed since the base', str(why))
        check('a file the base does not have is not read', 'crates/a/src/fresh.rs:2' not in out
              and 'crates/a/src/fresh.rs:2' not in why)
        check("the roadmap's own files are not read as code", 'docs/roadmap/X.md:3' not in out
              and 'docs/roadmap/X.md:3' not in why)
        check('a pointer in the index moves too', out.get('crates/a/src/lib.rs:8') == 'crates/a/src/lib.rs:10', str(out))

        status = remap.run(root, changed, base.get, out=lambda line: None)
        with open(os.path.join(root, 'docs/roadmap/X.md'), encoding='utf-8') as f:
            written = f.read()
        check('a run writes the moved pointers and nothing else',
              written == texts['docs/roadmap/X.md'] and '`crates/a/src/lib.rs:7`;' in written
              and '`crates/a/src/lib.rs:6-8`, a bare `:11`' in written, written)
        check('a run exits 0', status == 0, str(status))
        again = remap.plan(root, changed, base.get, remap.sweep.Tree(root))[0]
        check('a second run moves nothing the first moved', again == [], str(again))
    finally:
        shutil.rmtree(root, ignore_errors=True)

    failed = [r for r in results if not r[1]]
    for name, ok, detail in results:
        if not ok or not quiet:
            print(f'{"ok" if ok else "FAIL":4}  {name}' + (f'\n      {detail}' if not ok and detail else ''))
    print(f'{len(results)} checks, {len(failed)} failed')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
