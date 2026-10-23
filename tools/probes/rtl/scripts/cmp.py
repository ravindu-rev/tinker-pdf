"""Book-by-book difference of two probe outputs, A -> B: the books worse in B and
the books better.

usage: python3 -I cmp.py A B [--builds LIST] [--families REGEX] [--show N]

A and B are each a run or baseline directory (or a baseline's name, such as
935b077), or a single family's output file. Per family and build it prints the
rows on each side, the number of books A has and B lacks (counted, not named;
books only B has show only in B's row count), and the books that lost or
gained a metric (content, logical, cons, text, search, holds); with --show N,
up to N of each with the metrics that moved. It also reports overprint that
rose (the rr3m lines' overlap, TWROW's vov, VOVX, and the rr3m families' VOV
lines, which say where), probe_paren's failing keys, fixtures whose hashes
changed, ZZN overprint, LINE readings that changed, zz_vx_k11rv_dirless's
TALLY of lines read right and wrong, the td probe's ZZ row count, and books
whose probe panicked (PANIC).

A family one side has and the other does not is named as such and not
compared: a run of a few families against a whole baseline compares those
few, and says which of the baseline's it did not run. Such a comparison is
incomplete, and so is one of a run.sh output without its build's done
marker (a cut run): each reason is printed after TOTAL on an `INCOMPLETE`
line, and the exit status is 1. --families narrows both what is compared and
what both sides must have.
"""
import importlib.util
import os
import re
import sys


def load_kit():
    sys.dont_write_bytecode = True
    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location('rtlkit', os.path.join(here, 'rtlkit.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


K = load_kit()


def diff(label, a, b, show):
    worse, better, missing = [], [], 0
    for k, v in a.rows.items():
        if k not in b.rows:
            missing += 1
            continue
        w = K.lost(v, b.rows[k])
        g = K.lost(b.rows[k], v)
        if w:
            worse.append((k, w))
        if g:
            better.append((k, g))
    rose = [k for k, x in b.over.items() if x > a.over.get(k, 0) and k in a.over]
    vrose = [k for k, x in b.vovx.items() if k in a.vovx and x > a.vovx[k]]
    extra = []
    if a.paren or b.paren:
        extra.append(f'paren failing {len(a.paren)} -> {len(b.paren)}, new {len(b.paren - a.paren)}, '
                     f'gone {len(a.paren - b.paren)}')
    if a.fix or b.fix:
        extra.append(f'fixtures changed {sum(1 for k in a.fix if b.fix.get(k) != a.fix[k])}')
    if a.zzn or b.zzn:
        extra.append('ZZN overprinted %d -> %d' % (sum(1 for x in a.zzn.values() if x), sum(1 for x in b.zzn.values() if x)))
    if a.lines or b.lines:
        extra.append(f'LINE lines {sum(a.lines.values())} -> {sum(b.lines.values())}, '
                     f'{sum((a.lines - b.lines).values())} of A\'s gone in B')
    if a.vovx or b.vovx:
        extra.append(f'VOVX books rose {len(vrose)}')
    # A VOV line is printed only for a book drawing a glyph over another, so a
    # book of B's with no VOV line has none, and one A lacks had none in A.
    vov_rose = [k for k, (n, _) in b.vov.items() if k in b.rows and k in a.rows and n > a.vov.get(k, (0, ''))[0]]
    vov_moved = [k for k, (n, at) in b.vov.items() if k in a.vov and a.vov[k][0] == n and a.vov[k][1] != at]
    if a.vov or b.vov:
        extra.append(f'VOV books {len(a.vov)} -> {len(b.vov)}, rose {len(vov_rose)}, moved {len(vov_moved)}')
    if a.tally or b.tally:
        changed = sorted(k for k in set(a.tally) | set(b.tally) if a.tally.get(k, 0) != b.tally.get(k, 0))
        extra.append(f'TALLY lines misread {K.tally_misread(a)} -> {K.tally_misread(b)} of '
                     f'{K.tally_lines(a)} -> {K.tally_lines(b)}, kinds changed {len(changed)}')
    if a.zz or b.zz:
        extra.append('ZZ ' + ', '.join(f'{k} {a.zz.get(k, "-")} -> {b.zz.get(k, "-")}' for k in sorted(set(a.zz) | set(b.zz))))
    if a.panics or b.panics:
        extra.append(f'PANIC {len(a.panics)} -> {len(b.panics)}, new {len(b.panics - a.panics)}')
    for tag, o in (('A', a), ('B', b)):
        if o.exit not in (0, None) or o.failed:
            extra.append(f'{tag} exit {o.exit}' + (f', failing {o.failed}' if o.failed else ''))
    print(f'{label}: rows {len(a.rows)}/{len(b.rows)} missing {missing}: worse {len(worse)}, better {len(better)}, '
          f'overprint rose {len(rose)}' + ('; ' + '; '.join(extra) if extra else ''))
    for tag, xs in (('worse', worse), ('better', better)):
        for k, m in xs[:show]:
            print(f'    {tag}\t{",".join(m)}\t{k[:200]!r}')
    for k in rose[:show]:
        print(f'    over\t{a.over.get(k)} -> {b.over[k]}\t{k[:200]!r}')
    for k in vov_rose[:show]:
        print(f'    vov\t{a.vov.get(k, (0, ""))[0]} -> {b.vov[k][0]} at {b.vov[k][1][:80]}\t{k[:200]!r}')
    for k in vov_moved[:show]:
        print(f'    vov-moved\t{a.vov[k][1][:80]} -> {b.vov[k][1][:80]}\t{k[:200]!r}')
    if show:
        for k in sorted(set(a.tally) | set(b.tally)):
            if a.tally.get(k, 0) != b.tally.get(k, 0):
                print(f'    tally\t{a.tally.get(k, 0)} -> {b.tally.get(k, 0)}\t{k[:200]!r}')
        for k in sorted(b.panics - a.panics)[:show]:
            print(f'    panic-new\t{k[:200]!r}')
        for k in sorted(b.paren - a.paren)[:show]:
            print(f'    paren-new\t{k[:200]!r}')
        for k in list((a.lines - b.lines))[:show]:
            print(f'    line-gone\t{k[:300]}')
        for k in list((b.lines - a.lines))[:show]:
            print(f'    line-new\t{k[:300]}')
    tally_worse = 0
    if a.tally and b.tally and K.tally_misread(b) > K.tally_misread(a):
        tally_worse = K.tally_misread(b) - K.tally_misread(a)
    return len(worse) + len(vov_rose) + len(b.panics - a.panics) + tally_worse


def main():
    args = sys.argv[1:]
    K.help_requested(args, __doc__)
    opts = {'--builds': 'default,bundled,lines', '--families': '', '--show': '0'}
    for flag in list(opts):
        if flag in args:
            i = args.index(flag)
            opts[flag] = args[i + 1]
            del args[i:i + 2]
    if len(args) != 2:
        raise SystemExit(__doc__)
    show = int(opts['--show'])
    if os.path.isfile(args[0]) and os.path.isfile(args[1]):
        diff(os.path.basename(args[1]), K.load(args[0]), K.load(args[1]), show)
        return
    A, B = K.resolve(args[0]), K.resolve(args[1])
    pat = re.compile(opts['--families']) if opts['--families'] else None
    total = 0
    problems = []
    for build in opts['--builds'].split(','):
        problems += [f'{tag} {why}' for tag, why in (('A', K.unfinished(A, build)), ('B', K.unfinished(B, build))) if why]
        in_a, in_b = set(K.families_in(A, build)), set(K.families_in(B, build))
        only = {'A': [], 'B': []}
        for fam in sorted(in_a | in_b):
            if pat and not pat.search(fam):
                continue
            if fam not in in_a or fam not in in_b:
                only['A' if fam in in_a else 'B'].append(fam)
                continue
            total += diff(f'{build} {fam}', K.load(K.family_file(A, build, fam)), K.load(K.family_file(B, build, fam)), show)
        for side, fams in only.items():
            if fams:
                print(f'{build}: {len(fams)} families only in {side}, not compared: {" ".join(fams)}')
                problems.append(f'{build}: {len(fams)} families only in {side}, not compared')
    print(f'TOTAL worse {total} (books worse, VOV books whose overprint rose, new panics, TALLY lines misread more)')
    K.incomplete(problems, 'cmp.py')


if __name__ == '__main__':
    main()
