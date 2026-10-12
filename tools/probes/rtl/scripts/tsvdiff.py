"""Compare two per-line TSVs of the line probes (bct-auto, wrapped, sent, laneR,
or the td probe's): the lines OK in A and not in B, and the reverse.

usage: python3 -I tsvdiff.py A B [--show N]

A and B are TSV files (plain or .xz), or run or baseline directories (or a
baseline's name), in which case every TSV the two share is compared. A line's
key is its fields up to its OK/NO verdict, with its position among lines of the
same key; lines without a verdict (the td probe's) are compared whole.
"""
import collections
import importlib.util
import os
import sys


def load_kit():
    sys.dont_write_bytecode = True
    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location('rtlkit', os.path.join(here, 'rtlkit.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


K = load_kit()


def load(path):
    rows = {}
    seen = collections.Counter()
    plain = True
    for line in K.open_text(path):
        parts = line.rstrip('\n').split('\t')
        if len(parts) < 2:
            continue
        at = next((i for i, p in enumerate(parts) if p in ('OK', 'NO', 'ok', 'no')), None)
        if at is None:
            key = parts[0]
        else:
            plain = False
            key = '\t'.join(parts[:at])
        seen[key] += 1
        key = f'{key}#{seen[key]}'
        rows[key] = (parts[at] if at is not None else None, parts[at + 1:] if at is not None else parts[1:])
    return rows, plain


def ok(v):
    return v[0] is not None and v[0].upper() == 'OK'


def compare(label, a_path, b_path, show):
    a, plain = load(a_path)
    b, _ = load(b_path)
    common = [k for k in a if k in b]
    if plain:
        changed = [k for k in common if a[k] != b[k]]
        print(f'{label}: rows {len(a)}/{len(b)} missing {len(a) - len(common)} changed {len(changed)}')
        for k in changed[:show]:
            print('CHANGED', k, '\t'.join(a[k][1])[:300], ' ==> ', '\t'.join(b[k][1])[:300])
        return
    worse = [k for k in common if ok(a[k]) and not ok(b[k])]
    better = [k for k in common if not ok(a[k]) and ok(b[k])]
    print(f'{label}: rows {len(a)}/{len(b)} missing {len(a) - len(common)} okA {sum(ok(v) for v in a.values())} '
          f'okB {sum(ok(v) for v in b.values())} worse {len(worse)} better {len(better)}')
    for tag, ks in (('WORSE', worse), ('BETTER', better)):
        for k in ks[:show]:
            print(tag, k, '\t'.join(a[k][1])[:300], ' ==> ', '\t'.join(b[k][1])[:300])


def tsvs(root):
    out = {}
    for build in ('lines', 'default', 'bundled'):
        d = os.path.join(root, build)
        if not os.path.isdir(d):
            continue
        for name in os.listdir(d):
            if name.endswith('.tsv') or name.endswith('.tsv.xz'):
                out[f'{build}/{name[:-3] if name.endswith(".xz") else name}'] = os.path.join(d, name)
    return out


def main():
    args = sys.argv[1:]
    show = 20
    if '--show' in args:
        i = args.index('--show')
        show = int(args[i + 1])
        del args[i:i + 2]
    if len(args) != 2:
        raise SystemExit(__doc__)
    if os.path.isfile(args[0]) and os.path.isfile(args[1]):
        compare(os.path.basename(args[1]), args[0], args[1], show)
        return
    A, B = tsvs(K.resolve(args[0])), tsvs(K.resolve(args[1]))
    for name in sorted(set(A) & set(B)):
        compare(name, A[name], B[name], show)
    for name in sorted(set(A) ^ set(B)):
        print(f'{name}: only in {"A" if name in A else "B"}')


if __name__ == '__main__':
    main()
