"""The books left worse than 9297d2d (strictly) or than 56dfd93 (in conserving),
sorted into the classes ROADMAP CD-19's Left list names.

usage: python3 -I classes.py TABLE.json [--builds LIST]

TABLE.json is what `table.py ... --json TABLE.json` writes. Per class and
build: the books (unique), those worse than 9297d2d, those conserving at
56dfd93 and not now, and both. Every book no class claims is listed under
"others": a book there is a class nobody has named yet.

The classes are the ones the second CD-19 fix (935b077) reported, and round
6's (its hunt's and sweep's families), matched by family and book name (and,
for a class one style of a family has, the style too); a new round adds its own
here.
"""
import collections
import json
import re
import sys

CLASSES = [
    # Ruling 14's price, as the owner accepted it: a run of ! and ?, a closing
    # quote before stops, an ellipsis, opening a right-to-left line before a
    # pair after <br/> or a soft wrap.
    ('price', lambda p, n: (p == 'laneR' and re.match(r'^(price|soft)/', n) is not None)
        or (p == 'fv_extra' and re.match(r'^c/[a-z]+/m7/[^/]+/br/rtl$', n) is not None)
        or (p == 'h1_v1' and re.match(r'^[a-z]+/(ell|qe)-[a-z]+/opens/rtl$', n) is not None)
        or (p == 'h2_v1b' and re.match(r'^[a-z]+/(dots|ell|qe|exex|qq)-[a-z]+/open/rtl$', n) is not None)
        or (p == 'h1min' and re.match(r'^g-(qe|ell)-', n) is not None)),
    # H6: a run that mixes the stops, which the owner's decision does not name.
    ('H6 mixed stops', lambda p, n: (p == 'h1_v1' and re.match(r'^[a-z]+/(dotex|exell)-', n) is not None)
        or (p == 'h1min' and re.match(r'^g-(dotex|exell)-', n) is not None)),
    ('since 56dfd93: right-to-left Persian digits read whole at 9297d2d',
     lambda p, n: (p == 'h1_ofscripts' and n == 'hen/prose/out/rtl') or (p == 'h1min' and n == 'e-full')),
    # H1: dir-less or left-to-right Hebrew ending a number and a stop.
    ('H1 number and stop', lambda p, n: (p == 'h1_breadth' and n.startswith('A/'))
        or (p == 'h1_ofscripts' and n.startswith('hen/'))
        or (p == 'h1min' and n.startswith('a-full'))
        or (p == 'h1min2' and n.startswith('a3-'))),
    # H2-3: an inline-block inside an overflow-script word.
    ('H2-3 inline-block in a word', lambda p, n: p == 'h2_r2ibword' or (p == 'h2_gaps2' and '/ib' in n)),
    # H2-2: a styled overflow-script word before a bare right-to-left gloss and a Latin tail.
    ('H2-2 styled word, bare gloss, Latin tail', lambda p, n: p == 'h2_r2gloss' and re.match(r'^(grk|cyr|arm)-', n) is not None),
    # H2-7a: a Latin gloss before a bracketed right-to-left word.
    ('H2-7a Latin gloss', lambda p, n: p == 'h2_r2gloss' and n.startswith('lat-')),
    # H2-7b: table cells, a dash or a word before a bracketed Hebrew cell.
    ('H2-7b table cells', lambda p, n: p == 'h2_tables'),
    # H2-7d: a dir-less Hebrew line ending a right-to-left stop after a Latin word in brackets.
    ('H2-7d RTL stop after Latin', lambda p, n: p == 'h2_v1b' and '/ltrin/' in n),
    ('H2-7c ARABIC COMMA in a cell', lambda p, n: p == 'h2_v3b' and 'acomma' in n),
    ('since 56dfd93: pointed Hebrew, two pairs and an ARABIC COMMA', lambda p, n: p == 'fv_extra' and '/m6/' in n),
    # A dir-less Arabic paragraph with an inline word-spacing span cutting a word.
    ('hunt mid-ws', lambda p, n: p == 'hunt_punct' and n == 'ar/mid-ws/none'),
    # Round 6, V2 (Left (1)): a dir-less line of styled words whose left end is
    # a closing bracket, `W <b>W</b> W <i>W</i>, (W).`, its stop read first.
    ('V2 closing bracket at the left end', lambda p, n, s='': p in ('zz_v6_units', 'zz_v6_transform', 'zz_f6_transform')
        and n in ('he/styled/none', 'ar/styled/none')),
    # Round 6, V3 (Left (4)): two touching right-to-left cells with no dir,
    # read as one line from the right.
    ('V3 touching right-to-left cells', lambda p, n, s='': p == 'zz_v6_cells' and n in ('he/styled/none', 'ar/styled/none')),
    # Round 6, Left (6): `25٪ <b>W</b> ١٢٣ … 2010،` under a half turn at
    # word-spacing 1em, which conserved at 56dfd93 only.
    ('Left (6) half turn', lambda p, n, s='': p in ('zz_v6_transform', 'zz_f6_transform')
        and n in ('ar/num/none', 'ar/num/rtl') and 'rotate(180deg)' in s),
    # Round 6's sweep, Left (7), with bundled-fonts: a dir-less Hebrew
    # paragraph of inline blocks squeezed by scaleX(0.5), a word of which
    # search found at 9297d2d only.
    ('Left (7) squeezed inline blocks', lambda p, n, s='': p == 'zz_f6_transform' and n == 'he/ib/none'),
]


def main():
    args = sys.argv[1:]
    if '-h' in args or '--help' in args:
        print(__doc__.strip())
        return
    builds = 'default,bundled'
    if '--builds' in args:
        i = args.index('--builds')
        builds = args[i + 1]
        del args[i:i + 2]
    if len(args) != 1:
        raise SystemExit(__doc__)
    data = json.load(open(args[0], encoding='utf-8'))
    for build in builds.split(','):
        books = collections.defaultdict(dict)
        for key, rec in data.items():
            b, fam = key.split('/', 1)
            if b != build:
                continue
            for k in rec.get('worse_b', []):
                books[(fam, k)]['b'] = True
            for k in rec.get('worse_m', []):
                books[(fam, k)]['m'] = True
        per = collections.OrderedDict((c, collections.Counter()) for c, _ in CLASSES)
        per['others'] = collections.Counter()
        others = []
        for (fam, key), v in sorted(books.items()):
            p = fam.replace('zz_rr3m_', '')
            name = key.split('\t')[-1]
            style = key.split('\t')[0]
            cname = next((c for c, f in CLASSES if (f(p, name, style) if f.__code__.co_argcount == 3 else f(p, name))), 'others')
            c = per[cname]
            c['books'] += 1
            c['worse than 9297d2d'] += 'b' in v
            c['conserving at 56dfd93, not now'] += 'm' in v
            c['both'] += 'b' in v and 'm' in v
            if cname == 'others':
                others.append((fam, key))
        print(f'## {build}')
        for cname, c in per.items():
            if c:
                print(f'{cname}\t' + ', '.join(f'{k} {v}' for k, v in c.items()))
        for fam, key in others:
            print(f'  other\t{fam}\t{key[:200]!r}')


if __name__ == '__main__':
    main()
