"""Normalise one family's cargo output: drop cargo's noise, keep the data lines.

usage: python3 -I norm.py RAW OUT [--binary NAME] [--note TEXT] [--exit 'exit N']

RAW is cargo's output (plain or .xz). With --binary, only that test binary's
section of a longer output is kept. OUT ending in .xz is written compressed.
The rules are rtlkit.normalise's: header, sorted data lines, test results,
failing tests, panics, errors, exit status.
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


def main():
    kit = load_kit()
    args = sys.argv[1:]
    kit.help_requested(args, __doc__)
    opts = {}
    for flag in ('--binary', '--note', '--exit'):
        if flag in args:
            i = args.index(flag)
            opts[flag] = args[i + 1]
            del args[i:i + 2]
    if len(args) != 2:
        raise SystemExit(__doc__)
    raw, out = args
    with kit.open_text(raw) as f:
        lines = kit.normalise(f, binary=opts.get('--binary'), note=opts.get('--note'),
                              exit_line=opts.get('--exit'))
    text = ''.join(l + '\n' for l in lines)
    if out.endswith('.xz'):
        with lzma.open(out, 'wt', encoding='utf-8', preset=9, newline='\n') as g:
            g.write(text)
    else:
        with open(out, 'w', encoding='utf-8', newline='\n') as g:
            g.write(text)
    data = sum(1 for l in lines if kit.DATA.match(l))
    print(f'{os.path.basename(out)}\t{data} data lines')


if __name__ == '__main__':
    main()
