"""Compress a run directory into a baseline: every normalised family output and
every TSV, xz -9, in the same layout.

usage: python3 -I pack.py RUNDIR DEST [--note TEXT]

RUNDIR is run.sh's OUTDIR. DEST gets DEST/<build>/<family>.txt.xz,
DEST/<build>/<name>.tsv.xz and DEST/lines/<name>.tsv.xz. Raw cargo output,
exit files and done markers are not copied: the exit status is the last line
of each normalised file. With --note, every .txt starts with `# TEXT`, in
which {family} and {build} are replaced by the file's family and build.
A run whose done marker is missing is refused.
"""
import lzma
import os
import sys


def main():
    args = sys.argv[1:]
    if '-h' in args or '--help' in args:
        print(__doc__.strip())
        return
    note = None
    if '--note' in args:
        i = args.index('--note')
        note = args[i + 1]
        del args[i:i + 2]
    if len(args) != 2:
        raise SystemExit(__doc__)
    run, dest = args
    if not os.path.isfile(os.path.join(run, 'done')):
        raise SystemExit(f'{run}: no done marker, so the run did not finish; not packing it')
    n = 0
    for build in ('default', 'bundled', 'lines'):
        src = os.path.join(run, build)
        if not os.path.isdir(src):
            continue
        os.makedirs(os.path.join(dest, build), exist_ok=True)
        for name in sorted(os.listdir(src)):
            if not (name.endswith('.txt') or name.endswith('.tsv')) or not name.startswith(('zz_', 'bct', 'sent', 'wrapped', 'laneR')):
                continue
            data = open(os.path.join(src, name), 'rb').read()
            if name.endswith('.txt') and note:
                text = note.replace('{family}', name[:-4]).replace('{build}', build)
                data = ('# ' + text + '\n').encode('utf-8') + data
            with lzma.open(os.path.join(dest, build, name + '.xz'), 'wb', preset=9) as g:
                g.write(data)
            n += 1
    print(f'{n} files packed into {dest}')


if __name__ == '__main__':
    main()
