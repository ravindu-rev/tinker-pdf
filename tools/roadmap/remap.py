"""Move the roadmap's line pointers with the lines they cite.

usage: python3 -I tools/roadmap/remap.py BASE [--check] [--quiet]

A pointer is a backticked `path:a` or `path:a-b` in docs/ROADMAP.md or a tier
file under docs/roadmap/, or a bare `:a` after one, read as sweep.py reads
them. A change that inserts or removes lines in a file the roadmap cites moves
the lines its pointers name, and this moves the pointers with them. For each
file that differs between BASE and the working tree, a pointer into it moves
through the file's diff when its line of the roadmap is unchanged since BASE,
so it was written against BASE, and the lines it cites survive the diff whole:
a range moves only if every line in it is kept and they stay together. Every
other pointer into a changed file is left as it is and printed for a person to
read: one whose lines the change itself edited, one into a file that is gone,
one whose bare name more than one file has, a changed one among them long
enough to hold the lines it cites, and one on a line of the roadmap
that has changed since BASE, the step's own edits and an earlier run of this
one among them, so a second run moves nothing the first moved.

Run it after a change to code the roadmap cites, before the change lands, with
BASE the commit the change is made on. It changes only the roadmap's files and
runs `git` only to read BASE. With `--check` it changes nothing and exits 1
when a pointer would move. Prints one line per pointer it moves or leaves, or
with `--quiet` only those it leaves and the count; exits 2 when BASE cannot be
read.
"""
import argparse
import difflib
import glob
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import sweep  # noqa: E402

INDEX = 'docs/ROADMAP.md'


def roadmap_files(repo):
    """The files a pointer can stand in: the index and every tier file."""
    tiers = glob.glob(os.path.join(repo, 'docs', 'roadmap', '*.md'))
    return [INDEX] + sorted(os.path.relpath(p, repo).replace(os.sep, '/') for p in tiers)


def line_map(old, new):
    """Each line of `old` the diff to `new` keeps, as its line in `new` (both 1-based)."""
    kept = {}
    for tag, i1, i2, j1, _ in difflib.SequenceMatcher(None, old, new, autojunk=False).get_opcodes():
        if tag == 'equal':
            for k in range(i2 - i1):
                kept[i1 + k + 1] = j1 + k + 1
    return kept


def moved_range(kept, a, b):
    """Where lines `a` to `b` stand after the diff, or None where the change
    edited, removed or parted any of them."""
    start = kept.get(a)
    if start is None:
        return None
    for k in range(a, (b or a) + 1):
        if kept.get(k) != start + (k - a):
            return None
    return start, b and start + (b - a)


def plan(repo, changed, base_read, tree):
    """What moves, what is left and why, and each roadmap file's new text.

    `changed` is the repository paths that differ from BASE; `base_read(path)`
    returns BASE's copy of a path, or None where BASE has none."""
    maps, gone, lengths = {}, set(), {}
    for path in changed:
        if path == INDEX or path.startswith('docs/roadmap/'):
            continue
        old = base_read(path)
        if old is None:
            continue
        lengths[path] = len(old.split('\n'))
        here = os.path.join(repo, path)
        if not os.path.isfile(here):
            gone.add(path)
            continue
        with open(here, encoding='utf-8', errors='replace') as f:
            maps[path] = line_map(old.split('\n'), f.read().split('\n'))

    moved, left, texts = [], [], {}
    for rf in roadmap_files(repo):
        with open(os.path.join(repo, rf), encoding='utf-8') as f:
            text = f.read()
        before = base_read(rf)
        anchored = set(line_map(before.split('\n'), text.split('\n')).values()) if before is not None else set()
        parts, at, last, edited = [], 0, None, False
        for m in sweep.TICK.finditer(text):
            span = m.group(1)
            full = sweep.POINTER.match(span)
            bare = None if full else sweep.BARE_POINTER.match(span)
            if full:
                last = resolve(tree, full.group('file'), text, m.start(), maps, gone)
                target, p = last, full
            elif bare and last:
                target, p = last, bare
            else:
                continue
            if p.group('a') is None:
                continue
            line = text.count('\n', 0, m.start()) + 1
            where = f'{rf}:{line}'
            if isinstance(target, tuple):
                end = int(p.group('b') or p.group('a'))
                # A name it cannot place matters only if a changed file it
                # might mean is long enough to hold the lines it cites.
                if any(c in gone or lengths.get(c, 0) >= end for c in target[3]):
                    left.append((where, span, target[2]))
                continue
            if target in gone:
                left.append((where, span, 'its file is gone'))
                continue
            if target not in maps:
                continue
            a, b = int(p.group('a')), p.group('b') and int(p.group('b'))
            to = moved_range(maps[target], a, b)
            if to == (a, b):
                continue
            if line not in anchored:
                left.append((where, span, 'its line of the roadmap has changed since the base'))
                continue
            if to is None:
                left.append((where, span, 'the change edited the lines it cites'))
                continue
            na, nb = to
            head = span[:p.start('a')]
            new_span = f'{head}{na}' + (f'-{nb}' if b else '') + span[p.end('b') if b else p.end('a'):]
            parts.append(text[at:m.start(1)] + new_span)
            at = m.end(1)
            edited = True
            moved.append((where, span, new_span))
        if edited:
            parts.append(text[at:])
            texts[rf] = ''.join(parts)
    return moved, left, texts


def resolve(tree, ref, text, start, maps, gone):
    """The one file a pointer's name means, or why it means no one file.

    A bare name more than one file has means the file of the nearest full
    path ending in it earlier on the same line of the roadmap, as a row that
    names `xtask/src/main.rs` and then `main.rs:770` does; otherwise it is
    ambiguous. A tuple is a pointer that cannot be moved: (a path it might
    mean, whether a changed file is among those it might mean, the reason,
    the changed or gone files it might mean)."""
    if ref in gone:
        return ref
    hits = tree.resolve(ref)
    if len(hits) == 1:
        return hits[0]
    if not hits:
        return (ref, False, 'its file is not in the tree', [])
    line = text[text.rfind('\n', 0, start) + 1:start]
    for m in reversed(list(sweep.TICK.finditer(line))):
        full = sweep.POINTER.match(m.group(1))
        if full and '/' in full.group('file') and full.group('file').endswith('/' + ref):
            near = tree.resolve(full.group('file'))
            if len(near) == 1:
                return near[0]
    touched = [h for h in hits if h in maps or h in gone]
    return (touched[0] if touched else hits[0], bool(touched), f'its bare name is {len(hits)} files\'', touched)


def git_base(repo, base):
    def read(path):
        p = subprocess.run(['git', '-C', repo, 'show', f'{base}:{path}'], capture_output=True, text=True)
        return p.stdout if p.returncode == 0 else None
    return read


def main(argv):
    ap = argparse.ArgumentParser(description="Move the roadmap's line pointers with the lines they cite.")
    ap.add_argument('base', help='the commit the change is made on')
    ap.add_argument('--check', action='store_true', help='change nothing; exit 1 when a pointer would move')
    ap.add_argument('--quiet', action='store_true', help='print only the pointers left, and the count')
    ap.add_argument('--repo', default=os.path.normpath(os.path.join(HERE, '..', '..')))
    args = ap.parse_args(argv)
    ok = subprocess.run(['git', '-C', args.repo, 'rev-parse', '--verify', '--quiet', args.base + '^{commit}'],
                        capture_output=True, text=True)
    if ok.returncode != 0:
        print(f'cannot read {args.base}: no such commit')
        return 2
    diff = subprocess.run(['git', '-C', args.repo, 'diff', '--name-only', args.base], capture_output=True, text=True)
    return run(args.repo, diff.stdout.split(), git_base(args.repo, args.base), args.check, args.quiet)


def run(repo, changed, base_read, check=False, quiet=False, out=print):
    """Plan, write unless `check`, print, and return the exit status."""
    moved, left, texts = plan(repo, changed, base_read, sweep.Tree(repo))
    if not check:
        for rf, text in texts.items():
            with open(os.path.join(repo, rf), 'w', encoding='utf-8') as f:
                f.write(text)
    verb = 'would move' if check else 'moved'
    if not quiet:
        for where, old, new in moved:
            out(f'{verb:10} {where}  `{old}` -> `{new}`')
    for where, span, why in left:
        out(f'{"left":10} {where}  `{span}`: {why}')
    out(f'{len(moved)} pointers {verb}, {len(left)} left for a person to read')
    return 1 if check and moved else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
