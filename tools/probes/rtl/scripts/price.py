"""Ruling 14's price as the round reports state it: lane R's price/ and soft/
books, per opening, against 56dfd93 (and 9297d2d).

usage: python3 -I price.py [NEW] [--base B] [--mid M] [--tip T] [--builds LIST]

Reads zz_rr3m_laneR from each directory (a run or baseline directory, or a
baseline's name; B, M and T default to 9297d2d, 56dfd93 and 935b077; NEW
defaults to T). Per opening of price/ (the runs of ! and ?, a closing quote
before stops, an ellipsis, opening a right-to-left line before a pair after
<br/> or a soft wrap), and per group of openings:
  n          books
  worse      worse now than at M in any metric
  consM      conserving at M and not now
  wbc        worse than M and conserving at B
  bstrict    worse than B in the strict sense (conserving, read whole, found)
  logBM      logical order held at B and M and lost now
soft/ is the same books with a soft wrap where price/ has <br/>.
"""
import collections
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
FAMILY = 'zz_rr3m_laneR'
GROUPS = {
    'bang': ['ee', 'eq', 'qe', 'qq'],
    'closer': ['quot-stop', 'apos-stop', 'raq-stop', 'rdq-stop', 'quot-dots', 'rdq-ell'],
    'ellipsis': ['ell', 'one-dot', 'two-dot', 'vert', 'dots2', 'dots3'],
    'control': ['stop', 'q', 'acomma', 'rdq', 'quot'],
}


def main():
    args = sys.argv[1:]
    opts = {'--base': '9297d2d', '--mid': '56dfd93', '--tip': '935b077', '--builds': 'default,bundled'}
    for flag in list(opts):
        if flag in args:
            i = args.index(flag)
            opts[flag] = args[i + 1]
            del args[i:i + 2]
    if len(args) > 1:
        raise SystemExit(__doc__)
    new = K.resolve(args[0]) if args else K.resolve(opts['--tip'])
    for build in opts['--builds'].split(','):
        Bk = K.load(K.family_file(K.resolve(opts['--base']), build, FAMILY)).rows
        Mk = K.load(K.family_file(K.resolve(opts['--mid']), build, FAMILY)).rows
        Nk = K.load(K.family_file(new, build, FAMILY)).rows
        if not Nk:
            print(f'## {build}: no {FAMILY} output')
            continue
        print(f'## {build}')
        for fam in ('price', 'soft'):
            per = collections.defaultdict(collections.Counter)
            for k, n in Nk.items():
                name = k.split('\t')[1]
                mm = re.match(r'^' + fam + r'/[a-z]+/([^/]+)', name)
                if not mm:
                    continue
                c = per[mm.group(1)]
                m, b = Mk.get(k, {}), Bk.get(k, {})
                c['n'] += 1
                c['worse'] += bool(K.lost(m, n))
                c['consM'] += K.conserve(m) and not K.conserve(n)
                c['wbc'] += bool(K.lost(m, n)) and K.conserve(b)
                c['bstrict'] += bool(K.strict(b, n))
                c['logBM'] += b.get('logical') is True and m.get('logical') is True and n.get('logical') is not True
            tot = collections.defaultdict(collections.Counter)
            for g, ops in GROUPS.items():
                for op in ops:
                    if op in per:
                        print(f'{fam:5} {g:8} {op:10} ' + ' '.join(f'{x} {per[op][x]}' for x in
                                                                ('n', 'worse', 'consM', 'wbc', 'bstrict', 'logBM')))
                        tot[g].update(per[op])
            allc = collections.Counter()
            for g in GROUPS:
                print(f'{fam:5} {g:8} {"TOTAL":10} ' + ' '.join(f'{x} {tot[g][x]}' for x in
                                                          ('n', 'worse', 'consM', 'wbc', 'bstrict', 'logBM')))
                if g != 'control':
                    allc.update(tot[g])
            print(f'{fam:5} {"priced":8} {"TOTAL":10} ' + ' '.join(f'{x} {allc[x]}' for x in
                                                              ('n', 'worse', 'consM', 'wbc', 'bstrict', 'logBM')))


if __name__ == '__main__':
    main()
