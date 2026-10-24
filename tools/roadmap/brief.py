"""Print what a run needs to know about one roadmap step, in one call.

usage: python3 -I tools/roadmap/brief.py STEP|ROW [--repo DIR] [--base REV] [--all]
       python3 -I tools/roadmap/brief.py --file PATH [--repo DIR] [--all]

A step's brief is what a run reads before the code: the step; its row's
text, the row's other steps and its closing line; the state of each wait; the
lines each `file:line` it cites points at; where each code name it writes is
defined; the rulings it cites; the other steps that change its files, and
whether each is ordered against it; the commits since the cut's tree that
touched those files; the steps that wait on it or name it; and the sweep's
findings on it. A row's brief is the row with every step in full. `--file`
lists the steps that change one file and the pairs of them no wait orders.

It reads the roadmap and the tree through sweep.py, so it reads exactly what
the sweep reads, and exits 2 when the sweep could not read them or the ID
names nothing. It changes no file and fetches nothing; it runs `git` only to
list the commits since `--base` (the tree the cut was read against, by
default). A list longer than twelve says how many it left out; `--all`
prints every item. `briefcheck.py` beside this file holds each part of the
brief to a fixture.
"""
import argparse
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import sweep  # noqa: E402

BASE = '5297133'
LIST = 12    # the items a list shows before it says how many it left out
SPAN = 12    # the lines a cited range shows
WIDTH = 200  # the characters a quoted line keeps
ROW_TEXT = 1500  # the characters of each of its row's fields a step's brief keeps
NEAR = 3     # the steps either side of a step whose What its brief summarises
# An H row a person has acted on says so in bold, as H-01 and H-03 do.
DECIDED = re.compile(r'\*\*(?:decided|authori[sz]ed|answered|done)\b', re.I)
RULING_CITED = re.compile(r'\brulings? ((?:\d+(?:, | and | or ))*\d+)\b', re.I)
RULING_HEAD = re.compile(r'^(\d+)\. \*\*(.+?)\*\*')
DEF_KINDS = r'(?:fn|struct|enum|trait|const|static|type|mod|union)'


def listed(items, full):
    items = list(items)
    if full or len(items) <= LIST:
        return ', '.join(items)
    return ', '.join(items[:LIST]) + f', and {len(items) - LIST} more (--all lists them)'


def clip(text, n=WIDTH):
    text = ' '.join(text.split())
    return text if len(text) <= n else text[:n - 1] + '…'


def wait_state(rm, dep):
    """What a name in Depends on stands for today, read as the pick rule reads it."""
    kind = sweep.resolve_dep(rm, dep)
    if kind == 'step':
        return 'not landed'
    if kind == 'landed':
        return 'landed'
    if kind == 'split':
        return 'split into ' + ', '.join(sweep.split_parts(rm, dep)) + ', none landed'
    if kind == 'row':
        left = len(rm.rows[dep].steps)
        return f'open, {left} step{"" if left == 1 else "s"} left'
    if kind == 'h':
        h = rm.h[dep]
        cells = [v for k, v in h.items() if k not in ('ID', 'has default')]
        what = cells[0] if cells else ''
        if DECIDED.search(what):
            return 'decided: ' + clip(what, 160)
        if h['has default']:
            default = next(v for k, v in h.items() if k.startswith('Default'))
            return f'open; the work builds toward its default, {clip(default, 120)}; ' + clip(what, 120)
        return 'open, with no default: the step waits for it; ' + clip(what, 160)
    if kind == 'ng':
        return 'a hard limit'
    if kind == 'gone':
        return 'not in the roadmap: its row has left it, or never was'
    return 'names nothing in the roadmap'


def near(hits, local):
    """Of the files a reference can mean, those in the crate the step changes."""
    if len(hits) > 1:
        close = [h for h in hits if any(h.split('/')[:2] == lp.split('/')[:2] for lp in local)]
        return close or hits
    return hits


def cited_lines(tree, ctx, texts, local):
    """Each `file:line` the texts cite, once, with the lines it points at."""
    out, seen = [], set()
    for text in texts:
        for ref, a, b in sweep.file_refs(text):
            if a is None or (ref, a, b) in seen:
                continue
            seen.add((ref, a, b))
            label = f'{ref}:{a}' + (f'-{b}' if b else '')
            hits = near(tree.resolve(ref), local)
            if not hits:
                made = any(np == ref or np.endswith('/' + ref) for np in ctx['new_paths'])
                out.append(f'  {label}: ' + ('a step creates the file' if made else 'no such file in the tree'))
                continue
            if len(hits) > 1:
                out.append(f'  {label}: means one of {", ".join(hits[:4])}')
                continue
            lines = tree.lines(hits[0])
            lo, hi = int(a), int(b) if b else int(a)
            if lo < 1 or hi < lo or hi > len(lines):
                out.append(f'  {label}: {hits[0]} has {len(lines)} lines')
                continue
            out.append(f'  {hits[0]}:{lo}' + (f'-{hi}' if hi != lo else ''))
            shown = list(range(lo, hi + 1))
            if len(shown) > SPAN:
                shown = shown[:SPAN - 2] + [None] + shown[-1:]
            for n in shown:
                out.append('      …' if n is None else f'    {n}: {clip(lines[n - 1])}')
    return out


def code_files(tree):
    return [f for f in tree.files if f.split('/', 1)[0] in sweep.SCAN_TREES and f.endswith(sweep.CODE_EXT)
            and not any(p in '/' + f for p in sweep.SKIP_PARTS)]


def located(tree, names):
    """Where each name is defined in the code, or else where it first appears."""
    defs, uses = {n: [] for n in names}, {n: [] for n in names}
    if not names:
        return defs, uses
    alt = '|'.join(re.escape(n) for n in sorted(names, key=len, reverse=True))
    def_re = re.compile(r'\b' + DEF_KINDS + r'\s+(' + alt + r')\b|\bmacro_rules!\s*(' + alt + r')\b')
    use_re = re.compile(r'(?<![\w])(' + alt + r')(?![\w])')
    for f in code_files(tree):
        text = tree.read(f)
        for m in def_re.finditer(text):
            name = m.group(1) or m.group(2)
            defs[name].append(f'{f}:{text.count(chr(10), 0, m.start()) + 1}')
        for m in use_re.finditer(text):
            if len(uses[m.group(1)]) < 3:
                uses[m.group(1)].append(f'{f}:{text.count(chr(10), 0, m.start()) + 1}')
    return defs, uses


def introducer(rm, name, but):
    """The first step, other than `but`, whose What or a `(new)` span names the name."""
    for s in rm.steps.values():
        if s.id == but:
            continue
        if name in sweep.idents_in(s.what):
            return s.id
        for _, value in s.fields():
            for m in sweep.NEW_SPANS.finditer(value):
                if name in sweep.idents_in(m.group(1)):
                    return s.id
    return None


def plain_words(text):
    """Backticked words the sweep does not read as code names (`abbreviate`):
    named only where the code defines them."""
    for m in sweep.TICK.finditer(text):
        span = re.sub(r'\(.*$', '', m.group(1).strip())
        if sweep.WORD_RE.fullmatch(span) and len(span) > 2:
            yield span


def names_part(rm, tree, ctx, s, full):
    fields, plain = {}, set()
    for field, value in s.fields():
        for n, span in sweep.idents_in(value, spans=True):
            fields.setdefault(n, set()).add(field if field != 'exit' or '::' in span else 'exit test')
        plain.update(w for w in plain_words(value) if w in tree.idents)
    plain -= set(fields)
    defs, uses = located(tree, set(fields) | plain)
    names = sorted(set(fields) | {w for w in plain if defs[w]})
    if not names:
        return ['  (none)']
    out = []
    for n in names:
        if defs[n]:
            out.append(f'  `{n}`: defined at {listed(defs[n], full)}')
        elif n in tree.idents and uses[n]:
            out.append(f'  `{n}`: not defined in the code; used at {", ".join(uses[n])}')
        elif fields[n] == {'exit test'} and sweep.SNAKE.match(n):
            out.append(f'  `{n}`: not in the tree; a test its exit names')
        elif n in ctx['introduced']:
            who = introducer(rm, n, s.id)
            out.append(f'  `{n}`: not in the tree; ' + (f'{who} builds it' if who else 'this step builds it'))
        else:
            out.append(f'  `{n}`: not in the tree, and no step builds it')
    return out


def rulings(repo, texts):
    """Each ruling the texts cite, with its title and the lines it spans."""
    nums = sorted({int(n) for t in texts for group in RULING_CITED.findall(t) for n in re.findall(r'\d+', group)})
    if not nums:
        return []
    path = os.path.join(repo, 'docs', 'rulings.md')
    try:
        with open(path, encoding='utf-8') as f:
            lines = f.read().split('\n')
    except (OSError, UnicodeDecodeError):
        return [f'  docs/rulings.md cannot be read; cited: {", ".join(map(str, nums))}']
    heads = [(int(m.group(1)), i + 1, m.group(2)) for i, l in enumerate(lines) for m in [RULING_HEAD.match(l)] if m]
    # a ruling runs to the line before the next ruling or heading
    stops = sorted({h[1] for h in heads} | {i + 1 for i, l in enumerate(lines) if l.startswith('#')})
    out = []
    for n in nums:
        at = [h for h in heads if h[0] == n]
        if not at:
            out.append(f'  ruling {n}: no such ruling in docs/rulings.md')
            continue
        start = at[0][1]
        end = next((x - 1 for x in stops if x > start), len(lines))
        while end > start and not lines[end - 1].strip():
            end -= 1
        out.append(f'  ruling {n}, {clip(at[0][2], 120)}: docs/rulings.md:{start}-{end}')
    return out


def order_of(ctx, sid, other):
    if other in ctx['before'][sid]:
        return 'before'
    if sid in ctx['before'][other]:
        return 'after'
    return 'unordered'


def coeditors(rm, tree, ctx, s, full):
    out = []
    for path, new in s.where:
        state = 'new' if new else (f'{len(tree.lines(path))} lines' if tree.exists(path) else 'not in the tree')
        if new:
            first, named = sweep.creators(rm, ctx, path)
            state += ', created here' if s in first else f', created by {", ".join(x.id for x in first) or "no step"}'
        others = [x for x in rm.steps.values() if x.id != s.id and any(p == path for p, _ in x.where)]
        groups = {'before': [], 'after': [], 'unordered': []}
        for x in others:
            groups[order_of(ctx, s.id, x.id)].append(x.id)
        out.append(f'  {path} ({state}): ' + (f'{len(others)} other steps' if others else 'no other step'))
        for name in ('unordered', 'before', 'after'):
            if groups[name]:
                out.append(f'    {name}: {listed(groups[name], full)}')
    return out


def history(repo, base, paths):
    """The commits since `base` that touched the paths: (lines, or None and why)."""
    paths = sorted({p for p in paths if os.path.exists(os.path.join(repo, p))})
    if not paths:
        return [], None
    try:
        top = subprocess.run(['git', '-C', repo, 'rev-parse', '--show-toplevel'], capture_output=True, text=True,
                             timeout=60)
        if top.returncode != 0 or os.path.realpath(top.stdout.strip()) != os.path.realpath(repo):
            return None, f'{repo} is not a git checkout'
        log = subprocess.run(['git', '-C', repo, 'log', '--format=%x00%h %ad %s', '--date=short', '--name-only',
                              f'{base}..HEAD', '--', *paths], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return None, f'git could not run ({type(exc).__name__})'
    if log.returncode != 0:
        err = log.stderr.strip().splitlines()
        return None, err[-1] if err else f'git log exited {log.returncode}'
    out = []
    for chunk in log.stdout.split('\x00')[1:]:
        head, *files = [l for l in chunk.split('\n') if l.strip()]
        out.append(f'  {clip(head, 120)}: {", ".join(f for f in files if f in paths)}')
    return out, None


def findings_on(rm, tree, ctx, subjects, accepted):
    out, seen = [], set()
    for fn in sweep.CHECKS.values():
        for f in fn(rm, tree, ctx):
            if f.subject in subjects and f.key() not in seen:
                seen.add(f.key())
                if f.key() in accepted:
                    out.append(f'  accepted  {f.check} {f.subject} {f.item} ({accepted[f.key()][0]})')
                else:
                    out.append(f'  FINDING   {f.check} {f.subject} {f.item}: {f.message}')
    return out or ['  (none)']


def row_text(r, limit=None):
    """The row's own text; with a limit, each field cut there, saying how much it left out."""
    def cut(text):
        if limit is None or len(text) <= limit:
            return text
        return text[:limit].rstrip() + f' … ({len(text) - limit} characters more: `brief.py {r.id}` prints the row)'
    return [f'  {cut(r.item)}', f'  Today. {cut(r.today)}', f'  Exit criterion. {cut(r.exit)}',
            f'  Doc: {r.doc} · Depends on: {cut(r.depends_text)}']


def step_full(s, indent='  '):
    out = [f'{indent}{s.id} · {s.kind} · depends on: {", ".join(s.depends) or "—"}', f'{indent}  What: {s.what}']
    if s.notes:
        out.append(f'{indent}  Note: {s.notes}')
    out += [f'{indent}  Where: {s.where_text}', f'{indent}  Exit: {s.exit}']
    return out


def where_of(r, repo):
    return f'{os.path.relpath(r.path, repo)}:{r.line}'


def step_brief(rm, tree, ctx, accepted, repo, s, base, full):
    r = rm.rows[s.row]
    out = [f'{s.id} · {s.kind} · row {r.id} ({"reviewed" if r.reviewed else "not yet reviewed"}) · '
           f'{os.path.relpath(r.path, repo)}:{s.line}', '']
    out += step_full(s, indent='')[1:]
    out.append(f'  Depends on: {", ".join(s.depends) or "—"}')
    out += ['', f'Row {r.id} — {r.title} · {where_of(r, repo)}'] + row_text(r, None if full else ROW_TEXT)
    at = r.steps.index(s)

    def summary(i, x):
        if x is s:
            return f'{x.id} {x.kind} (this step)'
        if full or abs(i - at) <= NEAR:
            return f'{x.id} {x.kind} [{", ".join(x.depends) or "—"}]: {clip(x.what, 90)}'
        return f'{x.id} {x.kind}'
    out.append('  Steps: ' + '; '.join(summary(i, x) for i, x in enumerate(r.steps)))
    out.append(f'  Closes the row: {r.closes}')
    for f in r.findings:
        out.append(f'  The tree at `5297133`: {f}')
    out += ['', 'Waits']
    out += [f'  {d}: {wait_state(rm, d)}' for d in s.depends] or ['  (none)']
    texts = [v for _, v in s.fields()]
    local = [p for p, _ in s.where]
    out += ['', 'Lines it cites'] + (cited_lines(tree, ctx, texts, local) or ['  (none)'])
    out += ['', 'Code names it writes'] + names_part(rm, tree, ctx, s, full)
    cited = rulings(repo, texts + [r.item, r.today, r.exit])
    if cited:
        out += ['', 'Rulings it or its row cites'] + cited
    out += ['', 'Files it changes, and the other steps that change them'] + (coeditors(rm, tree, ctx, s, full)
                                                                             or ['  (none)'])
    refs = {hit for text in texts for ref, _, _ in sweep.file_refs(text) for hit in near(tree.resolve(ref), local)}
    lines, why = history(repo, base, set(local) | refs)
    out += ['', f'Commits since {base} that touched them']
    out += [f'  cannot tell: {why}'] if lines is None else (lines or ['  (none)'])
    waiting = [x.id for x in rm.steps.values() if s.id in x.depends]
    whole = [x.id for x in rm.steps.values() if s.row in x.depends]
    named = sorted({sid for sid, _, text in sweep.all_text_fields(rm) if sid not in (s.id, s.row)
                    and s.id in sweep.STEP_IN_TEXT.findall(text)} - set(waiting))
    out += ['', 'Who waits on it', f'  on this step: {listed(waiting, full) or "none"}',
            f'  on its whole row: {listed(whole, full) or "none"}', f'  named in the text of: {listed(named, full) or "none"}']
    out += ['', 'The sweep on this step'] + findings_on(rm, tree, ctx, {s.id}, accepted)
    return out


def row_brief(rm, tree, ctx, accepted, repo, r, full):
    out = [f'{r.id} — {r.title} · {"reviewed" if r.reviewed else "not yet reviewed"} · {where_of(r, repo)}', '']
    out += row_text(r) + ['', 'Steps']
    for s in r.steps:
        out += step_full(s)
        out += [f'    waits: ' + '; '.join(f'{d} {wait_state(rm, d)}' for d in s.depends)] if s.depends else []
    out += ['', f'Closes the row: {r.closes}']
    for f in r.findings:
        out.append(f'The tree at `5297133`: {f}')
    ids = {s.id for s in r.steps}
    outside = sorted({f'{x.id} on {d}' for x in rm.steps.values() if x.row != r.id
                      for d in x.depends if d == r.id or d in ids})
    out += ['', 'Who waits on it from other rows', f'  {listed(outside, full) or "none"}']
    out += ['', 'The sweep on this row and its steps'] + findings_on(rm, tree, ctx, ids | {r.id}, accepted)
    return out


def file_brief(rm, tree, ctx, path, full):
    state = f'{len(tree.lines(path))} lines' if tree.exists(path) else 'not in the tree'
    steps = [s for s in rm.steps.values() if any(p == path for p, _ in s.where)]
    out = [f'{path} ({state}): {len(steps)} steps change it', '']
    for s in steps:
        waits = [x.id for x in steps if x.id in ctx['before'][s.id]]
        out.append(f'  {s.id} · {s.kind} · after: {listed(waits, full) or "none of these"}')
    pairs = [f'{a.id} and {b.id}' for i, a in enumerate(steps) for b in steps[i + 1:]
             if order_of(ctx, a.id, b.id) == 'unordered']
    out += ['', f'Pairs no wait orders: {listed(pairs, full) or "none"}']
    return out


def main(argv):
    ap = argparse.ArgumentParser(description='Print what a run needs to know about one roadmap step.')
    ap.add_argument('id', nargs='?', help='a step (MS-03.1) or a row (MS-03)')
    ap.add_argument('--file', help='list the steps that change one file')
    ap.add_argument('--repo', default=os.path.normpath(os.path.join(HERE, '..', '..')))
    ap.add_argument('--base', default=BASE, help='the commit history is counted from (default: %(default)s)')
    ap.add_argument('--all', action='store_true', help='print every item of every list')
    args = ap.parse_args(argv)
    if bool(args.id) == bool(args.file):
        print('name one step or row, or one --file')
        return 2
    rm = sweep.Roadmap(args.repo)
    if rm.errors:
        for e in rm.errors:
            print(f'cannot read the roadmap: {e}')
        return 2
    tree = sweep.Tree(args.repo)
    if tree.errors:
        for e in tree.errors:
            print(f'cannot read the tree: {e}')
        return 2
    ctx = sweep.context(rm)
    accepted, _ = sweep.read_accepted(os.path.join(args.repo, 'tools', 'roadmap', 'accepted.tsv'))
    accepted = accepted or {}
    if args.file:
        out = file_brief(rm, tree, ctx, args.file.strip('/'), args.all)
    elif args.id in rm.steps:
        out = step_brief(rm, tree, ctx, accepted, args.repo, rm.steps[args.id], args.base, args.all)
    elif args.id in rm.rows:
        out = row_brief(rm, tree, ctx, accepted, args.repo, rm.rows[args.id], args.all)
    else:
        print(f'{args.id}: no step or row of that ID in the roadmap')
        return 2
    print('\n'.join(out))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
