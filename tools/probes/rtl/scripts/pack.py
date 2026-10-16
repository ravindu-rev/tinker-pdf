"""Pack a run directory into a baseline, in the layout of baselines/.

usage: python3 -I pack.py RUNDIR DEST [--note TEXT] [--suites]

RUNDIR is run.sh's OUTDIR, or with --suites a directory the suites wrote
through TINKER_RTL_KIT_OUT (crates/tinker-pdf/tests/rtl_probes.rs and
crates/tinker-pdf-shape/tests/rtl_lines.rs). DEST gets:

  DEST/<build>/<family>.txt.xz   every normalised family output of the default
                                 and bundled builds, xz -9
  DEST/<build>/<name>.tsv.xz     the td probe's TSV, as it was written
  DEST/lines/<family>.txt        the line families' outputs, plain
  DEST/lines/<name>.tsv.xz       the line probes' TSVs, as they were written
  DEST/lines/<name>.not-ok.tsv   each of those reduced to the rows not read
                                 right or not drawn (rtlkit.not_ok), plain

The suites read the first, second, third and fifth: the tinker-pdf suite
decodes the .xz files with the engine's own LZMA2 (a single-stream .xz, which
is what Python's lzma writes), and the shape suite, whose crate has no
decompressor, reads the plain ones. Raw cargo output, exit files and done
markers are not copied: the exit status is the last line of each normalised
file. With --note, every family output starts with `# TEXT`, in which {family}
and {build} are replaced by the file's family and build; the TSVs are copied
as written, with no note.

A run whose done marker is missing is refused. With --suites there is no done
marker, and the directory is refused unless every family families.tsv gives
the suites (rows and tsv in each build present, lines in lines/) has an
output whose last line is `exit 0`, which a suite writes only once its books
are all built.
"""
import importlib.util
import lzma
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
LINE_TSVS = ('bct-auto', 'wrapped', 'sent', 'laneR')


def suites_complete(run):
    """The families a suite run lacks, or whose output did not finish."""
    manifest = K.manifest()
    missing = []
    for build in ('default', 'bundled', 'lines'):
        if not os.path.isdir(os.path.join(run, build)):
            continue
        kinds = ('lines',) if build == 'lines' else ('rows', 'tsv')
        for row in manifest:
            if row['kind'] not in kinds:
                continue
            path = os.path.join(run, build, row['family'] + '.txt')
            last = ''
            if os.path.isfile(path):
                with open(path, encoding='utf-8') as f:
                    lines = f.read().split('\n')
                last = next((l for l in reversed(lines) if l), '')
            if last != 'exit 0':
                missing.append(f'{build}/{row["family"]}')
    return missing


def main():
    args = sys.argv[1:]
    K.help_requested(args, __doc__)
    note = None
    if '--note' in args:
        i = args.index('--note')
        note = args[i + 1]
        del args[i:i + 2]
    suites = '--suites' in args
    if suites:
        args.remove('--suites')
    if len(args) != 2:
        raise SystemExit(__doc__)
    run, dest = args
    if suites:
        missing = suites_complete(run)
        if missing:
            raise SystemExit(f'{run}: not every family finished, so not packing it: {" ".join(missing)}')
    elif not os.path.isfile(os.path.join(run, 'done')):
        raise SystemExit(f'{run}: no done marker, so the run did not finish; not packing it')
    n = 0
    for build in ('default', 'bundled', 'lines'):
        src = os.path.join(run, build)
        if not os.path.isdir(src):
            continue
        os.makedirs(os.path.join(dest, build), exist_ok=True)
        for name in sorted(os.listdir(src)):
            if not (name.endswith('.txt') or name.endswith('.tsv')) or not name.startswith(('zz_',) + LINE_TSVS):
                continue
            data = open(os.path.join(src, name), 'rb').read()
            if name.endswith('.txt') and note:
                text = note.replace('{family}', name[:-4]).replace('{build}', build)
                data = ('# ' + text + '\n').encode('utf-8') + data
            if build == 'lines' and name.endswith('.txt'):
                with open(os.path.join(dest, build, name), 'wb') as g:
                    g.write(data)
                n += 1
                continue
            with lzma.open(os.path.join(dest, build, name + '.xz'), 'wb', preset=9) as g:
                g.write(data)
            n += 1
            if build == 'lines' and name[:-4] in LINE_TSVS:
                text = K.not_ok_text(name, data.decode('utf-8'), (note or 'packed').replace('{family}', name[:-4]).replace('{build}', build))
                with open(os.path.join(dest, build, name[:-4] + '.not-ok.tsv'), 'w', encoding='utf-8', newline='\n') as g:
                    g.write(text)
                n += 1
    print(f'{n} files packed into {dest}')


if __name__ == '__main__':
    main()
