"""Three- or four-way table of the right-to-left probes: 9297d2d -> 56dfd93 ->
935b077 [-> a new run], per family and build.

usage: python3 -I table.py [NEW] [--base B] [--mid M] [--tip T] [--builds LIST]
                           [--families REGEX] [--by family|group] [--show N] [--json OUT]

B, M and T are run or baseline directories, or a baseline's name; they default
to the committed baselines 9297d2d, 56dfd93 and 935b077. NEW is run.sh's
OUTDIR. Without NEW the table reads B -> M -> T, and "now" below means T.
LIST is a comma-separated subset of default,bundled,lines (default: all three).

Columns, per family and build:
  rows       books now (missing: books T has that now lacks, a truncated run)
  conserving books whose content, logical order and cons (every one the line
             carries) all hold, at B/M/T[/NEW]
  >B         books worse now than at B in the strict sense the round reports
             use: conserving lost, or read whole (text), found by search or
             holds lost
  >M         books conserving at M and not now
  >T         (with NEW) books worse now than at T in any metric / in conserving
  +T         (with NEW) books conserving now and not at T
  over-ws    books drawing a glyph over another under word spacing or
             justification (the rr3m lines' overlap, TWROW's vov), B/M/T[/NEW]
  notes      VOVX books with overprint, probe_paren's failing keys, fixtures
             changed, ZZN overprint, exit status when not 0
--show N lists up to N books of each of >B, >M, >T with their metrics at each
tree: c conserving, t read whole, s found by search, h holds; 1 true, 0 false.
Under each build's table, the verifiers' repro tests (group regressions) show
their passed/failed counts at each tree that ran them, and with --show the
names failing now. The line probes (build `lines`) show their own totals.
"""
import collections
import importlib.util
import json
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


def fmt(v):
    if v is None:
        return '--'
    out = 'c%d' % K.conserve(v)
    for m, c in (('text', 't'), ('search', 's'), ('holds', 'h')):
        if m in v:
            out += '%s%d' % (c, v[m])
    return out


def over_ws(o):
    return sum(1 for k, x in o.over.items() if x and K.style_class(k) in ('ws', 'neg+ws'))


def compare(fam, outs, new):
    """outs: [B, M, T] or [B, M, T, N] Output objects (empty Output when missing)."""
    B, M, T = outs[:3]
    now = outs[-1]
    rec = collections.defaultdict(list)
    for k, v in B.rows.items():
        s = K.strict(v, now.rows.get(k))
        if s and k in now.rows:
            rec['worse_b'].append(k)
    for k, v in M.rows.items():
        if k in now.rows and K.conserve(v) and not K.conserve(now.rows[k]):
            rec['worse_m'].append(k)
    if new:
        for k, v in T.rows.items():
            if k not in now.rows:
                continue
            if K.lost(v, now.rows[k]):
                rec['worse_t'].append(k)
            if K.conserve(v) and not K.conserve(now.rows[k]):
                rec['worse_t_cons'].append(k)
        for k, v in now.rows.items():
            if k in T.rows and K.conserve(v) and not K.conserve(T.rows[k]):
                rec['gain_t'].append(k)
    ref = T if new else B
    missing = sum(1 for k in ref.rows if k not in now.rows) if now.path else len(ref.rows)
    notes = []
    if any(o.vovx for o in outs):
        notes.append('VOVX ' + '/'.join(str(sum(1 for x in o.vovx.values() if x)) if o.vovx else '-' for o in outs))
    if any(o.paren or o.paren_total for o in outs) and fam.endswith('probe_paren'):
        notes.append('paren fails ' + '/'.join(str(len(o.paren)) if o.path else '-' for o in outs)
                     + f' (new vs B {len(now.paren - B.paren)})')
    if any(o.fix for o in outs):
        ch = [k for k in B.fix if now.fix.get(k) != B.fix[k]]
        notes.append(f'fixtures changed vs B {len(ch)}' + (
            f', vs T {sum(1 for k in T.fix if now.fix.get(k) != T.fix[k])}' if new else ''))
    if any(o.zzn for o in outs):
        notes.append('ZZN overprinted ' + '/'.join(str(sum(1 for x in o.zzn.values() if x)) if o.path else '-' for o in outs))
    if any(o.lines for o in outs):
        ref_lines = T.lines if new else M.lines
        notes.append(f"LINE lines {'/'.join(str(sum(o.lines.values())) if o.path else '-' for o in outs)}, "
                     f"{sum((ref_lines - now.lines).values())} of {'T' if new else 'M'}'s gone now")
    for tag, o in zip('BMTN', outs):
        if o.path is None:
            continue
        if o.exit not in (0, None) or o.failed:
            notes.append(f'{tag}: exit {o.exit}' + (f', {len(o.failed)} tests failed' if o.failed else ''))
    return {
        'rows': len(now.rows) or now.n,
        'missing': missing,
        'cons': ['%d' % sum(K.conserve(v) for v in o.rows.values()) if o.path else '-' for o in outs],
        'over': ['%d' % over_ws(o) if o.path else '-' for o in outs],
        'rec': rec,
        'notes': notes,
    }


def summary_lines(o):
    """A line probe's own totals (SENT total=.. ok=.., BCT-AUTO ..., LANER ...)."""
    out = []
    if o.path is None:
        return out
    for line in K.open_text(o.path):
        line = line.rstrip('\n')
        if re.match(r'^(SENT|WRAPPED|BCT-AUTO|LANER) ', line):
            out.append(line)
    return out


def main():
    args = sys.argv[1:]
    opts = {'--base': '9297d2d', '--mid': '56dfd93', '--tip': '935b077', '--builds': 'default,bundled,lines',
            '--families': '', '--by': 'family', '--show': '0', '--json': None}
    for flag in list(opts):
        if flag in args:
            i = args.index(flag)
            opts[flag] = args[i + 1]
            del args[i:i + 2]
    if len(args) > 1 or any(a.startswith('--') for a in args):
        raise SystemExit(__doc__)
    dirs = [K.resolve(opts['--base']), K.resolve(opts['--mid']), K.resolve(opts['--tip'])]
    new = bool(args)
    if new:
        dirs.append(K.resolve(args[0]))
    show = int(opts['--show'])
    order = [r['family'] for r in K.manifest()]
    group_of = {r['family']: r['group'] for r in K.manifest()}
    kind_of = {r['family']: r['kind'] for r in K.manifest()}
    pat = re.compile(opts['--families']) if opts['--families'] else None
    jout = {}
    labels = 'B/M/T/N' if new else 'B/M/T'
    for build in opts['--builds'].split(','):
        fams = set()
        for d in dirs:
            fams.update(K.families_in(d, build))
        fams = [f for f in order if f in fams] + sorted(f for f in fams if f not in order)
        fams = [f for f in fams if (not pat or pat.search(f)) and kind_of.get(f) not in ('timing', 'tests')]
        if not fams:
            continue
        print(f'## {build}: conserving and overprint are {labels}' + ('' if new else '; now is T'))
        if build == 'lines':
            for fam in fams:
                outs = [K.load(K.family_file(d, build, fam)) for d in dirs]
                r = compare(fam, outs, new)
                print(f'{fam}  ' + '; '.join(r['notes']) if r['notes'] else fam)
                for tag, o in zip('BMTN', outs):
                    for s in summary_lines(o):
                        print(f'    {tag} {s}')
            print('    (per line: tsvdiff.py on lines/*.tsv)')
            continue
        head = f"{'family':32} {'rows':>6} {'miss':>4}  {'conserving':24} {'>B':>4} {'>M':>4}"
        if new:
            head += f" {'>T':>9} {'+T':>4}"
        head += f"  {'over-ws':14} notes"
        print(head)
        tot = collections.Counter()
        groups = collections.OrderedDict()
        shown = []
        for fam in fams:
            outs = [K.load(K.family_file(d, build, fam)) for d in dirs]
            r = compare(fam, outs, new)
            tsvs = [K.existing(os.path.join(d, build, fam + '.tsv')) for d in dirs]
            if any(tsvs):
                counts = [str(sum(1 for _ in K.open_text(t))) if t else '-' for t in tsvs]
                r['notes'].append(f"TSV rows {'/'.join(counts)} (tsvdiff.py compares them)")
            key = group_of.get(fam, '?') if opts['--by'] == 'group' else fam
            g = groups.setdefault(key, {'rows': 0, 'missing': 0, 'cons': [0] * len(dirs), 'over': [0] * len(dirs),
                                        'wb': 0, 'wm': 0, 'wt': 0, 'wtc': 0, 'gt': 0, 'notes': []})
            g['rows'] += r['rows']
            g['missing'] += r['missing']
            for i in range(len(dirs)):
                g['cons'][i] = g['cons'][i] + int(r['cons'][i]) if r['cons'][i] != '-' and g['cons'][i] != '-' else '-'
                g['over'][i] = g['over'][i] + int(r['over'][i]) if r['over'][i] != '-' and g['over'][i] != '-' else '-'
            g['wb'] += len(r['rec']['worse_b'])
            g['wm'] += len(r['rec']['worse_m'])
            g['wt'] += len(r['rec']['worse_t'])
            g['wtc'] += len(r['rec']['worse_t_cons'])
            g['gt'] += len(r['rec']['gain_t'])
            g['notes'] += ([f'{fam[3:]}: ' + n for n in r['notes']] if opts['--by'] == 'group' else r['notes'])
            for c in ('worse_b', 'worse_m', 'worse_t', 'gain_t'):
                tot[c] += len(r['rec'][c])
            tot['rows'] += r['rows']
            tot['missing'] += r['missing']
            jout[f'{build}/{fam}'] = {c: r['rec'][c] for c in r['rec']}
            if show:
                for c in ('worse_b', 'worse_m', 'worse_t'):
                    for k in r['rec'][c][:show]:
                        vals = ' '.join(fmt(o.rows.get(k)) for o in outs)
                        shown.append(f'    {c}\t{fam}\t{vals}\t{k[:160]!r}')
        for key, g in groups.items():
            line = (f"{key:32} {g['rows']:>6} {g['missing']:>4}  {'/'.join(str(x) for x in g['cons']):24} "
                    f"{g['wb']:>4} {g['wm']:>4}")
            if new:
                line += f" {str(g['wt']) + '/' + str(g['wtc']):>9} {g['gt']:>4}"
            line += f"  {'/'.join(str(x) for x in g['over']):14} {'; '.join(g['notes'])}"
            print(line)
        line = f"{'TOTAL':32} {tot['rows']:>6} {tot['missing']:>4}  {'':24} {tot['worse_b']:>4} {tot['worse_m']:>4}"
        if new:
            line += f" {tot['worse_t']:>9} {tot['gain_t']:>4}"
        print(line)
        for s in shown:
            print(s)
        regressions(dirs, build, order, kind_of, pat, show)
        print()
    if opts['--json']:
        with open(opts['--json'], 'w', encoding='utf-8') as f:
            json.dump(jout, f)


def regressions(dirs, build, order, kind_of, pat, show):
    """The verifiers' repro tests: passed and failed at each tree that ran them."""
    fams = [f for f in order if kind_of.get(f) == 'tests' and (not pat or pat.search(f))]
    rows = []
    for fam in fams:
        outs = [K.load(K.family_file(d, build, fam)) for d in dirs]
        if not any(o.path for o in outs):
            continue
        cells = []
        for o in outs:
            if o.path is None:
                cells.append('-')
                continue
            m = [re.search(r'(\d+) passed; (\d+) failed', s) for s in o.status]
            m = [x for x in m if x]
            cells.append(f'{sum(int(x.group(1)) for x in m)}/{sum(int(x.group(2)) for x in m)}' if m else 'none')
        rows.append(f"{fam:32} passed/failed {' '.join(cells)}")
        if show:
            for name in outs[-1].failed[:show]:
                rows.append(f'    failing now\t{name}')
    if rows:
        print('regressions (the verifiers\' tests; a failure is read against its finding, see README):')
        for r in rows:
            print(r)


if __name__ == '__main__':
    main()
