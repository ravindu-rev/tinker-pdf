"""Book-by-book difference of two probe outputs, A -> B: the books worse in B and
the books better.

usage: python3 -I cmp.py A B [--builds LIST] [--families REGEX] [--show N]

A and B are each a run or baseline directory (or a baseline's name, such as
935b077), or a single family's output file. Per family and build it prints the
rows on each side, the books A has and B lacks, and the books that lost or
gained a metric (content, logical, cons, text, search, holds); with --show N,
up to N of each with the metrics that moved. It also reports overprint that
rose (the rr3m lines' overlap, TWROW's vov, VOVX), probe_paren's failing keys,
fixtures whose hashes changed, ZZN overprint and LINE readings that changed.
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
    if show:
        for k in sorted(b.paren - a.paren)[:show]:
            print(f'    paren-new\t{k[:200]!r}')
        for k in list((a.lines - b.lines))[:show]:
            print(f'    line-gone\t{k[:300]}')
        for k in list((b.lines - a.lines))[:show]:
            print(f'    line-new\t{k[:300]}')
    return len(worse)


def main():
    args = sys.argv[1:]
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
    for build in opts['--builds'].split(','):
        fams = sorted(set(K.families_in(A, build)) | set(K.families_in(B, build)))
        for fam in fams:
            if pat and not pat.search(fam):
                continue
            total += diff(f'{build} {fam}', K.load(K.family_file(A, build, fam)), K.load(K.family_file(B, build, fam)), show)
    print(f'TOTAL worse {total}')


if __name__ == '__main__':
    main()
