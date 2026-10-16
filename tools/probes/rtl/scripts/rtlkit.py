"""Shared reading of the right-to-left probe kit's outputs.

Imported by the kit's other scripts through `load_kit()`, never through
`sys.path`: every script runs under `python3 -I`.

A run directory and a baseline directory have the same shape:

    <dir>/default/<family>.txt      one family's normalised output, default build
    <dir>/bundled/<family>.txt      the same with --features bundled-fonts
    <dir>/lines/<family>.txt        a line probe's normalised output
    <dir>/lines/<name>.tsv          the per-line TSVs the line probes write
    <dir>/<build>/<family>.tsv      the td probe's TSV (zz_code1_tdprobe)

A baseline holds the same files compressed (`.txt.xz`, `.tsv.xz`), each `.txt`
with a `# ` line on top saying what it is; the TSVs are as the probes wrote
them. Every reader here takes either.
"""
import collections
import csv
import lzma
import os
import re

KIT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINES = os.path.join(KIT, 'baselines')
BUILDS = ('default', 'bundled')

# The first token of a line a probe prints as data: ROW, VOVX, TWROW, BOOK,
# FAIL, FIX, LINE, ZZN, SENT, ... Upper case, then a tab or a space.
DATA = re.compile(r'^[A-Z][A-Z0-9_-]+[\t ]')
# A probe's own summary line: rows=, books=, twbooks=, opened=.
SUMMARY = re.compile(r'^(rows|books|twbooks|opened|fails)=\S')
STATUS_PREFIX = re.compile(r'^test (\S+) \.\.\. ?(.*)$')
RUNNING = re.compile(r'Running (tests/\S+\.rs)')
METRICS = ('content', 'logical', 'cons', 'text', 'search', 'holds')


def open_text(path):
    """A text file, plain or xz-compressed."""
    if path.endswith('.xz'):
        return lzma.open(path, 'rt', encoding='utf-8', errors='replace', newline='\n')
    return open(path, encoding='utf-8', errors='replace', newline='\n')


def existing(path):
    """`path`, or `path.xz`, whichever exists; None if neither does."""
    for p in (path, path + '.xz'):
        if os.path.isfile(p):
            return p
    return None


def family_file(root, build, family):
    """The output of one family in a run or baseline directory, or None."""
    return existing(os.path.join(root, build, family + '.txt'))


def families_in(root, build):
    """Every family a run or baseline directory holds for one build."""
    d = os.path.join(root, build)
    if not os.path.isdir(d):
        return []
    out = set()
    for name in os.listdir(d):
        m = re.match(r'^(zz_\w+)\.txt(\.xz)?$', name)
        if m:
            out.add(m.group(1))
    return sorted(out)


def manifest():
    """families.tsv as a list of dicts, in file order."""
    rows = []
    with open(os.path.join(KIT, 'families.tsv'), encoding='utf-8') as f:
        for row in csv.DictReader((l for l in f if not l.startswith('#')), delimiter='\t'):
            rows.append(row)
    return rows


def normalise(lines, binary=None, note=None, exit_line=None):
    """Cargo's output of one family reduced to what a comparison reads.

    Kept: the `Running tests/<family>.rs` header; every data line (a probe's
    upper-case tag, or its own `rows=`/`books=` summary), sorted, so that the
    order the test threads printed in does not matter; then `test result`
    lines without their time, the names of failing tests, panics and compiler
    errors, in order; then the run's exit status. Dropped: compiling, timing
    and libtest's per-test status lines. A data line printed straight after
    libtest's `test <name> ... ` keeps its data and loses the prefix.

    With `binary`, only that test binary's section of a longer output is read.
    """
    header, data, status = [], [], []
    current = None
    in_failures = False
    exits = []
    for raw in lines:
        line = raw.rstrip('\n').rstrip('\r')
        m = RUNNING.search(line)
        if m and line.lstrip().startswith('Running '):
            current = m.group(1)
            in_failures = False
            if binary is None or current == f'tests/{binary}.rs':
                header.append(f'Running {current}')
            continue
        if line.startswith('exit ') and re.match(r'^exit -?\d+$', line):
            exits.append(line)
            continue
        if binary is not None and current != f'tests/{binary}.rs':
            # Compiler errors come before any section; keep them for every binary.
            if current is None and line.startswith('error'):
                status.append(line)
            continue
        m = STATUS_PREFIX.match(line)
        if m:
            rest = m.group(2)
            if rest in ('', 'ok', 'FAILED', 'ignored') or rest.startswith('ignored'):
                continue
            line = rest
        if line == 'failures:':
            in_failures = True
            continue
        if in_failures:
            if line.startswith('    ') and line.strip() and ' ' not in line.strip():
                status.append('FAILED\t' + line.strip())
                continue
            if not line.strip():
                continue
            in_failures = False
        if DATA.match(line) or SUMMARY.match(line):
            data.append(line)
        elif line.startswith('test result:'):
            status.append(re.sub(r'; finished in [0-9.]+s$', '', line))
        elif 'panicked at' in line:
            status.append(line.strip())
        elif line.startswith('error'):
            status.append(line)
    out = []
    if note:
        out += ['# ' + n for n in note.split('\n')]
    out += header
    out += sorted(data)
    out += status
    if exit_line:
        out.append(exit_line)
    elif exits:
        out.append(exits[-1])
    return out


class Output:
    """One family's output, parsed.

    rows   key -> metrics: content/logical/search (kit lines), text/cons/logical/
           search (the rr3m lines), holds (dirless BOOK lines), cons/logical
           (TWROW lines). True or False; a metric a line does not carry is absent.
    over   key -> glyphs drawn over another (the rr3m lines' overlap, TWROW's vov)
    vovx   (style, lang, hash) -> overprint count (the VOVX lines)
    paren  the failing keys of zz_vx_probe_paren's FAIL lines; paren_total its rows= line
    fix    the fixtures' FIX lines, name -> hashes
    zzn    ZZN lines, (spacing, lang) -> glyphs drawn over another
    lines  LINE lines, as a multiset of whole lines: zz_fv_shape's readings, and
           the misread lines zz_vx_k11rv_dirless prints beside its BOOK lines
    vov    VOV lines, the rr3m families' book key -> (glyphs drawn over another
           on one baseline, where the first one is): printed only for a book
           that has some, so a book missing here has none
    tally  TALLY lines, zz_vx_k11rv_dirless's (gloss, kind of line) -> lines of
           that kind; the kind `right` is the lines read right
    zz     ZZ lines, the td probe's `ZZ <what> <n>`, what -> n (`rows`: the TSV
           rows it wrote)
    panics PANIC lines, the books whose probe panicked
    """

    def __init__(self, path):
        self.path = path
        self.rows = {}
        self.over = {}
        self.vovx = {}
        self.paren = set()
        self.paren_total = None
        self.fix = {}
        self.zzn = {}
        self.lines = collections.Counter()
        self.vov = {}
        self.tally = {}
        self.zz = {}
        self.panics = set()
        self.status = []
        self.failed = []
        self.notes = []
        self.dups = 0
        self.n = 0
        self.exit = None
        if path is None:
            return
        for line in open_text(path):
            self._line(line.rstrip('\n'))

    def _line(self, line):
        if line.startswith('# '):
            self.notes.append(line[2:])
            return
        if re.match(r'^exit -?\d+$', line):
            self.exit = int(line.split()[1])
            return
        if line.startswith('test result:') or 'panicked at' in line or line.startswith('error'):
            self.status.append(line)
            return
        if line.startswith('FAILED\t'):
            self.failed.append(line.split('\t', 1)[1])
            return
        p = line.split('\t')
        tag = p[0].split(' ')[0]
        if tag == 'ROW':
            self.n += 1
            if len(p) > 3 and p[3].startswith('m='):
                k = p[1] + '\t' + p[2]
                v = {}
                for f in p[3:]:
                    m = re.match(r'^(text|cons|logical)=([01])$', f)
                    if m:
                        v[m.group(1)] = m.group(2) == '1'
                        continue
                    m = re.match(r'^search=(\d+)/(\d+)$', f)
                    if m:
                        v['search'] = m.group(1) == m.group(2)
                        continue
                    m = re.match(r'^overlap=(\d+)/(\d+)$', f)
                    if m:
                        self.over[k] = int(m.group(2))
            else:
                keyparts, v = [], {}
                for f in p[1:]:
                    m = re.match(r'^(content|logical|search)=(\w+)$', f)
                    if m:
                        v[m.group(1)] = m.group(2) == 'true'
                    elif not v:
                        keyparts.append(f)
                k = '\t'.join(keyparts)
            if k in self.rows:
                self.dups += 1
            self.rows[k] = v
        elif tag == 'BOOK':
            self.n += 1
            k = '\t'.join(p[1:4])
            if k in self.rows:
                self.dups += 1
            self.rows[k] = {'holds': 'holds=true' in line}
        elif tag == 'FAIL':
            self.paren.add('\t'.join(x for x in p if not x.startswith(('content=', 'logical=', '['))))
        elif line.startswith('rows=') and 'fails=' in line:
            self.paren_total = line
        elif tag == 'FIX':
            self.n += 1
            self.fix[p[1]] = '\t'.join(p[2:])
        elif tag == 'TWROW':
            self.n += 1
            k = p[1] + '\t' + p[2]
            d = {}
            for f in p[3:]:
                m = re.match(r'^(cons|logical|vov|twbad|bx32)=(\d+)$', f)
                if m:
                    d[m.group(1)] = int(m.group(2))
            v = {}
            if 'cons' in d:
                v['cons'] = d['cons'] == 1
            if 'logical' in d:
                v['logical'] = d['logical'] == 1
            if k in self.rows:
                self.dups += 1
            self.rows[k] = v
            self.over[k] = d.get('vov', 0)
        elif tag == 'VOVX':
            self.vovx[(p[1], p[2], p[3])] = int(p[4]) if len(p) > 4 and p[4].isdigit() else 0
        elif tag == 'ZZN':
            self.n += 1
            m = re.search(r'overprinted (\d+)', line)
            self.zzn[(p[0], p[1])] = int(m.group(1)) if m else 0
        elif tag == 'LINE':
            self.lines[line] += 1
        elif tag == 'VOV':
            k = p[1] + '\t' + p[2] if len(p) > 2 else line
            n = int(p[3]) if len(p) > 3 and p[3].isdigit() else 0
            self.vov[k] = (n, p[4] if len(p) > 4 else '')
        elif tag == 'TALLY':
            if len(p) >= 4 and p[-1].isdigit():
                self.tally['\t'.join(p[1:-1])] = int(p[-1])
        elif tag == 'ZZ':
            w = line.split()
            if len(w) >= 3 and w[-1].isdigit():
                self.zz[' '.join(w[1:-1])] = int(w[-1])
        elif tag == 'PANIC':
            self.panics.add('\t'.join(p[1:]))

    def complete(self):
        """Whether the run that wrote this finished: exit 0 and every test binary passed."""
        return self.exit == 0 and not self.failed and all('FAILED' not in s for s in self.status)


def load(path):
    return Output(path)


def help_requested(args, doc):
    """`-h` or `--help` anywhere among a script's arguments: print its usage and exit 0."""
    if '-h' in args or '--help' in args:
        print(doc.strip())
        raise SystemExit(0)


def tally_misread(o):
    """zz_vx_k11rv_dirless's lines read wrong, every gloss and kind together (None
    without TALLY lines). The lines read right are no measure on their own: a
    change to line breaking changes how many lines there are."""
    if not o.tally:
        return None
    return sum(n for k, n in o.tally.items() if k.split('\t')[-1] != 'right')


def tally_lines(o):
    """zz_vx_k11rv_dirless's lines, every kind (None without TALLY lines)."""
    if not o.tally:
        return None
    return sum(o.tally.values())


def conserve(v):
    """Conserving: every one of content, logical and cons the line carries is true."""
    ks = [m for m in ('content', 'logical', 'cons') if m in v]
    return bool(ks) and all(v[m] for m in ks)


def lost(a, b):
    """The metrics true in `a` and not true in `b` (['missing'] if b has no such book)."""
    if b is None:
        return ['missing']
    return [m for m in METRICS if a.get(m) is True and b.get(m) is not True]


def strict(a, b):
    """Worse in the strict sense the round reports use: conserving lost, or read whole,
    found by search or holds lost."""
    out = []
    if conserve(a) and not conserve(b or {}):
        out.append('cons')
    for m in ('text', 'search', 'holds'):
        if a.get(m) is True and (b or {}).get(m) is not True:
            out.append(m)
    return out


def style_class(key):
    """The spacing a book is drawn under, read from its style: ws (positive word
    spacing or justify), neg (negative letter or word spacing), neg+ws, other."""
    style = key.split('\t')[0]
    neg = re.search(r'(letter|word)-spacing:\s*-', style) is not None
    ws = (re.search(r'word-spacing:\s*[0-9.]*[1-9]', style) is not None or 'justify' in style
          or 'INLINE-WS' in style)
    if neg:
        return 'neg+ws' if ws else 'neg'
    return 'ws' if ws else 'other'


def resolve(arg):
    """A run or baseline directory: a path, or a baseline's commit (`935b077`)."""
    if os.path.isdir(arg):
        return arg
    d = os.path.join(BASELINES, arg)
    if os.path.isdir(d):
        return d
    raise SystemExit(f'not a directory or a baseline: {arg}')
