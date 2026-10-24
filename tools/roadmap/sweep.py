"""Check every step of the roadmap at once for the defects its reviews found.

usage: python3 -I tools/roadmap/sweep.py [--repo DIR] [--accepted FILE] [--only CHECK,...] [--quiet]
                                            [--waits]

Reads the roadmap as it is on disk: `docs/roadmap/<TIER>.md` (rows and their
steps), `docs/roadmap/H.md` (the owner's rows), `docs/roadmap/NG.md` (the
limits) and the index's counts in `docs/ROADMAP.md`, and the tree beside
them, less what `.gitignore` names as build output. Every check is one class
of defect a review of the cut found by hand; `README.md` beside this file says
which review found each, what the check decides and what it cannot see.

A finding is fixed in the roadmap, or accepted in `accepted.tsv` beside this
file with the reason it is not a defect. The run prints every finding,
marks the accepted ones, and exits 1 when any finding is unexplained or any
accepted line matches no finding (an acceptance may not outlive what it
accepted), 2 when the roadmap, the tree or the acceptance file cannot be read.
It changes no file, fetches nothing and spawns nothing; `selfcheck.py` holds
each check to a fixture it must fire on and a twin it must not.
"""
import argparse
import collections
import os
import re
import sys

TIERS = ['MS', 'K', 'P', 'RC', 'FT', 'ED', 'AN', 'FJ', 'SG', 'SE', 'FI', 'FO', 'SD', 'SU', 'CD', 'OC']
KINDS = {'design', 'build', 'fix', 'test', 'measure', 'ci', 'doc', 'watch'}

ROW_ID = r'[A-Z]{1,2}-\d+[a-z]?'
STEP_ID = ROW_ID + r'\.\d+[a-z]?'
ROW_HEAD = re.compile(r'^## (' + ROW_ID + r') — (.+)$')
STEP_RE = re.compile(r'^(?P<row>' + ROW_ID + r')\.(?P<n>\d+)(?P<s>[a-z]?)$')
ROW_RE = re.compile(r'^' + ROW_ID + r'$')
H_RE = re.compile(r'^H-\d+$')
NG_RE = re.compile(r'^NG-\d+$')
STEP_IN_TEXT = re.compile(r'(?<![\w.-])(' + STEP_ID + r')(?![\w])')
H_IN_TEXT = re.compile(r'(?<![\w-])(H-\d+)(?![\w-])')
ID_IN_CELL = re.compile(r'(?<![\w-])(H-\d+|NG-\d+|' + STEP_ID + r'|' + ROW_ID + r')(?![\w-]|\.\d)')
STEP_HEADER = '| Step | Kind | What | Where | Exit | Depends on |'
# Lines the reader takes or refuses, and never passes over: a row heading, a
# step line, a row's label or a step's note written another way.
HEAD_LIKE = re.compile(r'^#+\s*[\[*_`]*\s*' + ROW_ID + r'(?![\w-])')
STEP_LIKE = re.compile(r'^\s*\|\s*' + STEP_ID + r'\s*\|')
STEP_LINE = re.compile(r'^\| ' + STEP_ID + r' \|.*\|$')
LABEL_LIKE = re.compile(r'^\s*\*\*\s*(?:today|exit criterion|closes the row|doc|depends on|the tree at)\b', re.I)
NOTE_LIKE = re.compile(r'[*_]+\s*notes?\s*[*_]*\s*[:.]|\bnotes?\s*[*_]*\s*:', re.I)
H_LIKE = re.compile(r'^\s*\|\s*[*_`]*\s*H-\d')
LABELS = (('today', '**Today.** '), ('exit', '**Exit criterion.** '), ('closes', '**Closes the row:** '))
LABEL_NAMES = (('today', 'Today'), ('exit', 'Exit criterion'), ('doc', 'Doc'), ('closes', 'Closes the row'))
TICK = re.compile(r'`([^`]+)`')
FILE_EXT = ('.rs', '.md', '.yml', '.yaml', '.toml', '.json', '.tsv', '.txt', '.py', '.sh', '.h', '.cs',
            '.go', '.java', '.rb', '.mjs', '.js', '.html', '.css', '.pdf', '.epub', '.ttf', '.otf',
            '.icc', '.xml', '.svg', '.lock', '.ts', '.swift', '.c', '.ppm', '.j2c', '.rsp', '.sha256',
            '.xps', '.dat', '.raw')
CODE_EXT = ('.rs', '.py', '.yml', '.yaml', '.toml', '.cs', '.go', '.java', '.rb', '.js', '.mjs',
            '.ts', '.h', '.swift', '.sh', '.c')
SCAN_TREES = ('crates', 'tools', 'xtask', 'bindings', 'fuzz', '.github')
SKIP_PARTS = ('/target/', '/node_modules/', '/data/', '/.git/', '/obj/', '/__pycache__/', '/tools/roadmap/')
SKIP_DIRS = ('target', 'node_modules', '.git', 'obj', '__pycache__')
# What `.gitignore` names as build output or fetched data: never the tree.
SKIP_TREES = {'bindings/js/pkg', 'bindings/js/demo/node_modules', 'bindings/dotnet/bin', 'bindings/dotnet/obj',
              'bindings/dotnet/runtimes', 'bindings/dotnet/tests/Smoke/packages', 'bindings/dotnet/tests/Smoke/bin',
              'bindings/dotnet/tests/Smoke/obj', 'testdata/golden/actual', 'fuzz/artifacts', 'fuzz/coverage',
              'corpus/cache', 'corpus/files', 'corpus/data'}
# How many parts of a path name its crate or package: `crates/x`, `xtask`.
CRATE_ROOTS = {'crates': 2, 'tools': 2, 'bindings': 2, 'xtask': 1, 'fuzz': 1, '.github': 1}
# Basenames that every crate repeats; sharing one is no sign of a second home.
COMMON_BASENAMES = {'mod.rs', 'lib.rs', 'main.rs', 'tests.rs', 'build.rs', 'Cargo.toml', 'README.md'}
# A new file named for a whole family where the family is already split.
GENERIC_STEMS = {'rules', 'tests', 'test', 'all', 'misc', 'common', 'util', 'utils', 'helpers', 'support'}
TOOLING = re.compile(r'(?<![\w/])(tiers/[A-Z]|steps/[A-Z]|rows\.json|check_steps\.py|render\.py|roadmap-cut|'
                     r'scratchpad|batch_[A-Z])')
# What a change whose output must not move says about itself (STEP_SPEC's first
# trap): a refactor, a speed-up or a rewrite. Its test of equal output passes
# before it too, so its exit needs a guard that fails first: a count, an
# injection, a band. Read with code spans blanked, so a name is not a word.
SAME_OUTPUT = re.compile(r"\b(cach(e|es|ed|ing)\b(?! (values?|data|results?)\b)|once per \w+|faster|"
                         r"speed(s|ed|ing)? ?-?ups?\b|quadratic|without (a |the )?copy|zero-copy|in place of `x`|"
                         r"mov(e|ed|es) (from|into|to) `x`|refactor\w*|memoi[sz]\w*|hoist\w*|deduplicat\w*|"
                         r"streamed|streaming)", re.I)
FAIL_FIRST = re.compile(r"\b(fail\w*|inject\w*|flips?|refus\w*|panics?|fires?|count(s|ed|er|ers)?|band)\b", re.I)
BOTH_BUILDS = ('both builds', 'bundled-fonts')
# Text a test involves: a font, a glyph, shaping or a text format, or text the
# step draws or lays out (not text as data: a report's text, the standard's).
TEXT_WORDS = re.compile(r'\b(fonts?|glyphs?|typefaces?|shap(ed|ing|er)|epub|x?html|markdown|cmaps?|tounicode|bidi|'
                        r'reading order)\b', re.I)
TEXT_DRAWN = re.compile(r'\b(draw\w*|drew|render\w*|paint\w*|laid out|lay\w* out|layout|wrap\w*|reflow\w*|justif\w*|'
                        r'line[- ]break\w*|text box(es)?|shadows?|decoration|measur\w*)\b', re.I)
# A value an exit pins, which the two builds can draw differently.
PINNED = re.compile(r'\b(fingerprint\w*|hash\w*|goldens?|pin(s|ned)?|byte-identical\w*|byte for byte|byte-equal|'
                    r'sha-256|reftests?)\b', re.I)
# What a clause says when it claims a test exists, and when it does not.
TEST_CLAIM = re.compile(r'\b(unchanged|keeps?|stays?|still|existing|already)\b')
TEST_NEW = re.compile(r'\bnew\b|flips|fails first|failing first')
DEF_RE = re.compile(r'\b(?:fn|struct|enum|trait|const|static|type|mod|union)\s+([A-Za-z_][A-Za-z0-9_]*)')
WORD_RE = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
SNAKE = re.compile(r'^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$')
SCREAM = re.compile(r'^[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+$')
POINTER = re.compile(r'^(?P<file>[\w./+-]+\.(?:' + '|'.join(e[1:] for e in FILE_EXT) + r'))'
                     r'(?::(?P<a>\d+)(?:-(?P<b>\d+))?)?$')
BARE_POINTER = re.compile(r'^:(?P<a>\d+)(?:-(?P<b>\d+))?$')
INSTRUMENTS = [
    # (name, pattern in the step's text, the row whose step must come first, the least step that lands it);
    # a name's patterns rise in least step, so the last that matches a text is the step it needs
    # (selfcheck.py holds the order).
    ('MS-02', re.compile(r"MS-02's (?:paired )?job"), 'MS-02', 6),
    ('MS-04', re.compile(r"MS-04's (?:open|text|calibration)"), 'MS-04', 3),
    ('MS-04', re.compile(r"MS-04's (?:write|merge|analysis)"), 'MS-04', 4),
    ('MS-04', re.compile(r"MS-04's render"), 'MS-04', 5),
    ('MS-04', re.compile(r"MS-04's (?:encode|decode)"), 'MS-04', 6),
    ('MS-04', re.compile(r"MS-04's (?:sign|layout|fonts|shaping)"), 'MS-04', 7),
    ('MS-04', re.compile(r"MS-04's (?:set|groups?|operations?|benchmark|suite|CLI)"), 'MS-04', 8),
    ('MS-04', re.compile(r"MS-04's (?:window|20\b)"), 'MS-04', 9),
    ('MS-05', re.compile(r"MS-05's (?:probe|test|case|open|merge|'|memory)"), 'MS-05', 1),
    ('MS-05', re.compile(r"MS-05's (?:band|peak|seven)"), 'MS-05', 3),
    ('MS-01 guards', re.compile(r'flips from owed'), 'MS-01', 6),
    ('MS-01 wasm leg', re.compile(r"`wasm32-wasip1` guard leg"), 'MS-01', 8),
]
# A step that reads a run of CI on `develop`.
RUN_ON_DEVELOP = re.compile(r'\b(?:run|runs|job|jobs|leg|legs|dispatch\w*)\b[^.;]*\bon `develop`')
# A speed row: an operation a step gives MS-04's benchmark set.
SPEED_ROW = re.compile(r"\bspeed rows?\b|MS-04's (?:[a-z]+ group|set) gains", re.I)
# A Default cell that says the work waits: "none —", "not scheduled until ruled".
NO_DEFAULT = re.compile(r'^\s*(none\s*(—|-|$)|not scheduled until)', re.I)


class Step:
    def __init__(self, row, cells, line):
        self.row = row
        self.id, self.kind, what, where, self.exit, deps = cells
        self.what, _, self.notes = what.partition(' *Note:* ')
        self.where = [(m.group(1), bool(m.group(2))) for m in re.finditer(r'`([^`]+)`(\s*\(new\))?', where)]
        self.where_text = where
        self.depends = [] if deps.strip() in ('—', '') else [d.strip() for d in deps.split(',') if d.strip()]
        self.line = line

    def text(self):
        return ' '.join([self.what, self.notes, self.exit])

    def fields(self):
        return [('what', self.what), ('notes', self.notes), ('exit', self.exit)]


class Row:
    def __init__(self, rid, title, tier, path, line):
        self.id, self.title, self.tier, self.path, self.line = rid, title, tier, path, line
        self.item = self.today = self.exit = self.doc = self.depends_text = self.closes = ''
        self.findings = []
        self.steps = []
        self.reviewed = True
        self.tables = 0
        self.labels = collections.Counter()


NO_ROW = Row('', '', '', '', 0)


class Roadmap:
    def __init__(self, repo):
        self.repo = repo
        self.rows = collections.OrderedDict()
        self.steps = collections.OrderedDict()
        self.h = {}
        self.ng = set()
        self.errors = []
        for tier in TIERS:
            self._read_tier(tier)
        self._read_h()
        self._read_ng()
        self._read_index()

    def _read_text(self, path):
        try:
            with open(path, encoding='utf-8') as f:
                return f.read()
        except FileNotFoundError:
            self.errors.append(f'{path}: missing')
        except (OSError, UnicodeDecodeError) as exc:
            self.errors.append(f'{path}: cannot be read ({type(exc).__name__})')
        return None

    def _read_index(self):
        """docs/ROADMAP.md counts each tier's rows and steps, the H rows and the
        limits; a row, step or H row the reader dropped shows as a count that
        does not match."""
        path = os.path.join(self.repo, 'docs', 'ROADMAP.md')
        text = self._read_text(path)
        if text is None:
            return
        self.index, counts = {}, {}
        for line in text.split('\n'):
            m = re.match(r'^\| ([A-Z]{1,2}) \| .+ \| (\d+) \| (?:\d+|—) \| (\d+) \|$', line)
            if m and m.group(1) in TIERS:
                self.index[m.group(1)] = (int(m.group(2)), int(m.group(3)))
            m = re.match(r'^\| (H|NG) \| .+ \| (\d+) \| — \| — \|$', line)
            if m:
                counts[m.group(1)] = int(m.group(2))
        for tier in TIERS:
            have = (sum(1 for r in self.rows.values() if r.tier == tier),
                    sum(len(r.steps) for r in self.rows.values() if r.tier == tier))
            if self.index.get(tier) != have:
                self.errors.append(f'{path}: the index counts {self.index.get(tier)} rows and steps for {tier}, '
                                   f'and the reader found {have}')
        for name, have in (('H', len(self.h)), ('NG', len(self.ng))):
            if counts.get(name) != have:
                self.errors.append(f'{path}: the index counts {counts.get(name)} {name} rows, and the reader found {have}')

    def _close_row(self, row):
        if row is None:
            return
        for attr, name in LABEL_NAMES:
            if row.labels[attr] != 1:
                self.errors.append(f'{row.path}:{row.line}: row {row.id} has {row.labels[attr]} "{name}" lines, not one')

    def _read_tier(self, tier):
        path = os.path.join(self.repo, 'docs', 'roadmap', f'{tier}.md')
        text = self._read_text(path)
        if text is None:
            return
        row = None
        in_table = in_findings = False
        for n, line in enumerate(text.split('\n'), 1):
            where = f'{path}:{n}'
            if line.startswith('#'):
                self._close_row(row)
                row = None
                in_table = in_findings = False
                m = ROW_HEAD.match(line)
                if m:
                    row = Row(m.group(1), m.group(2), tier, path, n)
                    if row.id in self.rows:
                        self.errors.append(f'{where}: row {row.id} appears twice')
                    self.rows[row.id] = row
                elif HEAD_LIKE.match(line):
                    self.errors.append(f'{where}: a row heading the reader cannot parse: {line[:60]}')
                continue
            if STEP_LIKE.match(line):
                if row is None or not in_table:
                    self.errors.append(f"{where}: a step line outside its row's step table")
                elif not STEP_LINE.match(line):
                    self.errors.append(f'{where}: a step line the reader cannot parse: {line[:60]}')
                elif len(split_cells(line)) != 6:
                    self.errors.append(f'{where}: a step line with {len(split_cells(line))} cells')
                else:
                    s = Step(row.id, split_cells(line), n)
                    if s.id in self.steps:
                        self.errors.append(f'{where}: step {s.id} appears twice')
                    if NOTE_LIKE.search(code_blanked(s.what)) or NOTE_LIKE.search(code_blanked(s.notes)):
                        self.errors.append(f'{where}: step {s.id} has a note the reader cannot parse')
                    self.steps[s.id] = s
                    row.steps.append(s)
                continue
            if row is None:
                continue
            if line == STEP_HEADER:
                in_table = True
                row.tables += 1
                continue
            if in_table:
                if line.startswith('| ---'):
                    continue
                if line.startswith('|'):
                    self.errors.append(f'{where}: a line in a step table that is not a step')
                    continue
                in_table = False
            if in_findings:
                if line.startswith('- '):
                    row.findings.append(line[2:])
                    continue
                if line.strip():
                    in_findings = False
            for attr, label in LABELS:
                if line.startswith(label):
                    setattr(row, attr, line[len(label):])
                    row.labels[attr] += 1
                    break
            else:
                if line.startswith('**Doc:** '):
                    doc, sep, deps = line[len('**Doc:** '):].partition(' · **Depends on:** ')
                    if not sep:
                        self.errors.append(f'{where}: a Doc line without its Depends on')
                    row.doc, row.depends_text = doc, deps
                    row.labels['doc'] += 1
                elif line.startswith("**The tree at `") and line.endswith(":**"):
                    in_findings = True
                elif LABEL_LIKE.match(line):
                    self.errors.append(f'{where}: a label the reader cannot parse: {line[:60]}')
                elif line.startswith('*Steps not yet reviewed'):
                    row.reviewed = False
                elif line.startswith('**') and not row.item:
                    row.item = line
        self._close_row(row)

    def _read_h(self):
        path = os.path.join(self.repo, 'docs', 'roadmap', 'H.md')
        text = self._read_text(path)
        if text is None:
            return
        header, prev = None, ''
        for n, line in enumerate(text.split('\n'), 1):
            where = f'{path}:{n}'
            if not line.startswith('|'):
                header = None
            elif line.startswith('| ---'):
                header = split_cells(prev) if prev.startswith('|') else None
            elif H_LIKE.match(line):
                cells = split_cells(line) if line.startswith('| H-') and line.rstrip().endswith('|') else []
                if not cells or not H_RE.match(cells[0]):
                    self.errors.append(f'{where}: an H row the reader cannot parse: {line[:40]}')
                elif header is None or header[0] != 'ID':
                    self.errors.append(f'{where}: H row {cells[0]} is in a table whose first column is not ID')
                elif 'Blocks' not in header and 'Duty' not in header:
                    self.errors.append(f'{where}: H row {cells[0]} is in a table with no Blocks column')
                elif len(cells) != len(header):
                    self.errors.append(f'{where}: H row {cells[0]} has {len(cells)} cells, and its table {len(header)}')
                elif cells[0] in self.h:
                    self.errors.append(f'{where}: H row {cells[0]} appears twice')
                else:
                    row = dict(zip(header, cells))
                    default = next((v for k, v in row.items() if k.startswith('Default')), None)
                    row['has default'] = default is not None and not NO_DEFAULT.match(default)
                    self.h[cells[0]] = row
            prev = line

    def _read_ng(self):
        path = os.path.join(self.repo, 'docs', 'roadmap', 'NG.md')
        text = self._read_text(path)
        if text is None:
            return
        ids = re.findall(r'\*\*(NG-\d+) — ', text)
        for x in sorted({x for x in ids if ids.count(x) > 1}):
            self.errors.append(f'{path}: {x} appears twice')
        self.ng = set(ids)


def split_cells(line):
    inner = line.strip()[1:-1]
    out, buf, i = [], [], 0
    while i < len(inner):
        c = inner[i]
        if c == '\\' and i + 1 < len(inner) and inner[i + 1] == '|':
            buf.append('|')
            i += 2
            continue
        if c == '|':
            out.append(''.join(buf).strip())
            buf = []
        else:
            buf.append(c)
        i += 1
    out.append(''.join(buf).strip())
    return out


RANGE = re.compile(r'(?<![\w-])([A-Z]{1,2})-(\d+)([a-z]?)(?:\s*(?:…|–|\.\.\.)\s*|—)(?:([A-Z]{1,2})-)?(\d+)?([a-z]?)'
                   r'(?![\w])')
TO_RANGE = re.compile(r'(?<![\w.-])(' + ROW_ID + r'(?:\.\d+[a-z]?)?) to (' + ROW_ID + r'(?:\.\d+[a-z]?)?)(?![\w])')
# "hands AN-41 to AN-46" passes one row to another: no range.
TRANSFER = re.compile(r'\b(?:hand|pass|give|move|send|leave|return|add|appl(?:y|ie)|point|tie|link|map|cop(?:y|ie))'
                      r'(?:s|es|d|ed|ing)?\s+$', re.I)
SLASHED = re.compile(r'(?<![\w-])([A-Z]{1,2}-\d+)([a-z])((?:/[a-z])+)(?![\w])')


def ids_in(text, rm):
    """The IDs a cell names: ranges such as `AN-27…AN-34`, `SG-01…11`,
    `FT-04a–d`, `ED-12—ED-14`, `CD-19.9 to CD-19.13` and `H-11 to H-13`
    expanded to the rows, steps and H rows the roadmap holds between their
    ends, and `FI-07a/b` to both rows."""
    found = set(ID_IN_CELL.findall(text))
    universe = list(rm.rows) + list(rm.h)
    for m in TO_RANGE.finditer(text):
        if TRANSFER.search(text[:m.start()]):
            continue
        a, b = m.groups()
        sa, sb = STEP_RE.match(a), STEP_RE.match(b)
        if sa and sb and sa.group('row') == sb.group('row') and sa.group('row') in rm.rows:
            found.update(x.id for x in rm.rows[sa.group('row')].steps if step_key(a) <= step_key(x.id) <= step_key(b))
        elif not sa and not sb and a.split('-')[0] == b.split('-')[0]:
            lo, hi = row_key(a), row_key(b)
            found.update(r for r in universe if r.split('-')[0] == a.split('-')[0] and lo <= row_key(r) <= hi)
    for m in SLASHED.finditer(text):
        found.update(m.group(1) + x for x in [m.group(2)] + m.group(3).split('/')[1:])
    for m in RANGE.finditer(text):
        pre, lo_n, lo_s, pre2, hi_n, hi_s = m.groups()
        if pre2 and pre2 != pre:
            continue
        lo = (int(lo_n), lo_s or '')
        hi = (int(hi_n), hi_s or '~') if hi_n else (int(lo_n), hi_s or '~')
        for rid in universe:
            r = re.match(r'^([A-Z]{1,2})-(\d+)([a-z]?)$', rid)
            if r and r.group(1) == pre and lo <= (int(r.group(2)), r.group(3)) <= hi:
                found.add(rid)
    return found


def step_key(sid):
    m = STEP_RE.match(sid)
    return (int(m.group('n')), m.group('s')) if m else (0, '')


def row_key(rid):
    m = re.match(r'^[A-Z]{1,2}-(\d+)([a-z]?)$', rid)
    return (int(m.group(1)), m.group(2)) if m else (0, '')


def split_parts(rm, dep):
    """A step split after the cut (`K-07.2` into `K-07.2a` and `K-07.2b`): its parts."""
    m = STEP_RE.match(dep)
    if not m or m.group('s') or m.group('row') not in rm.rows:
        return []
    return [x.id for x in rm.rows[m.group('row')].steps if re.fullmatch(re.escape(dep) + r'[a-z]', x.id)]


def camel(word):
    """A camel name: a capital first, a lower-case letter, and a hump — a
    lower-case letter or digit before a capital (`PdfNoSuchA`), or a run of
    capitals before a word (`XRefNoSuch`), so `IDs` and `PDFs` are words."""
    return bool(re.fullmatch(r'[A-Z][A-Za-z0-9]*', word)) and any(c.islower() for c in word) and \
        bool(re.search(r'[a-z0-9][A-Z]|^[A-Z]{2,}[a-z]{2,}', word))


class Tree:
    """What the checks ask of the files beside the roadmap."""

    def __init__(self, repo):
        self.repo = repo
        self.files = []
        self.idents = set()
        self.errors = []
        for top in SCAN_TREES + ('docs', 'testdata', 'corpus'):
            base = os.path.join(repo, top)
            for d, dirs, names in os.walk(base, onerror=self._walk_error):
                rel = os.path.relpath(d, repo).replace(os.sep, '/')
                dirs[:] = [x for x in dirs if x not in SKIP_DIRS and f'{rel}/{x}' not in SKIP_TREES]
                for name in names:
                    self.files.append(f'{rel}/{name}')
        for name in os.listdir(repo):
            if os.path.isfile(os.path.join(repo, name)):
                self.files.append(name)
        self.by_base = collections.defaultdict(list)
        for f in self.files:
            self.by_base[f.rsplit('/', 1)[-1]].append(f)
        for f in self.files:
            if f.split('/', 1)[0] in SCAN_TREES and f.endswith(CODE_EXT) and not any(p in '/' + f for p in SKIP_PARTS):
                self.idents.update(WORD_RE.findall(self.read(f)))
        self._lines = {}
        self._defs = {}

    def _walk_error(self, exc):
        if not isinstance(exc, FileNotFoundError):
            self.errors.append(f'{exc.filename}: cannot be read ({type(exc).__name__})')

    def exists(self, rel):
        return os.path.exists(os.path.join(self.repo, rel))

    def read(self, rel):
        try:
            with open(os.path.join(self.repo, rel), encoding='utf-8', errors='replace') as f:
                return f.read()
        except (IsADirectoryError, FileNotFoundError):
            return ''
        except OSError as exc:
            self.errors.append(f'{rel}: cannot be read ({type(exc).__name__})')
            return ''

    def lines(self, rel):
        if rel not in self._lines:
            self._lines[rel] = self.read(rel).split('\n')
        return self._lines[rel]

    def defs(self, rel):
        if rel not in self._defs:
            self._defs[rel] = set(DEF_RE.findall(self.read(rel)))
        return self._defs[rel]

    def fns(self, rel):
        return set(re.findall(r'\bfn\s+([A-Za-z_][A-Za-z0-9_]*)', self.read(rel)))

    def resolve(self, ref):
        """Every file a reference written in the roadmap can mean."""
        if '/' in ref:
            if self.exists(ref):
                return [ref]
            return [f for f in self.files if f.endswith('/' + ref)]
        return list(self.by_base.get(ref, []))

    def verbs(self):
        text = self.read('tools/tpdf/src/main.rs')
        return set(re.findall(r'^\s*"([a-z][a-z0-9-]*)" => (?:run|check|probe|writing::print)\(', text, re.M))


class Finding:
    def __init__(self, check, subject, item, message):
        self.check, self.subject, self.item, self.message = check, subject, item, message

    def key(self):
        return (self.check, self.subject, self.item)


def closure(rm):
    """For each step, every step and H row that must be done before it. Only
    Depends on makes a wait: a step whose Depends on are done is picked even
    while an earlier step of its row waits (the index's "How a step runs", 1)."""
    memo = {}
    state = {}
    cycles = []

    def deps_of(s):
        out = []
        for d in s.depends:
            if d in rm.steps:
                out.append(d)
            elif d in rm.rows:
                out.extend(x.id for x in rm.rows[d].steps)
            else:
                out.extend(split_parts(rm, d))
        return out

    def visit(sid, stack):
        if sid in memo:
            return memo[sid]
        if state.get(sid) == 'open':
            cycles.append(stack[stack.index(sid):] + [sid])
            return set()
        state[sid] = 'open'
        s = rm.steps[sid]
        acc = set(d for d in s.depends if H_RE.match(d))
        for d in deps_of(s):
            if d == sid:
                continue
            acc.add(d)
            acc |= visit(d, stack + [d])
        state[sid] = 'done'
        memo[sid] = acc
        return acc

    sys.setrecursionlimit(max(10000, sys.getrecursionlimit()))
    for sid in rm.steps:
        visit(sid, [sid])
    return memo, cycles


def resolve_dep(rm, dep):
    if H_RE.match(dep):
        return 'h' if dep in rm.h else 'unknown'
    if NG_RE.match(dep):
        return 'ng' if dep in rm.ng else 'unknown'
    m = STEP_RE.match(dep)
    if m:
        if dep in rm.steps:
            return 'step'
        if split_parts(rm, dep):
            return 'split'
        row = rm.rows.get(m.group('row'))
        if row is None:
            return 'gone'
        # A landed step's line leaves and its ID is never reused, so an ID
        # below the row's last step that the row lacks has landed.
        if row.steps and step_key(dep) < max(step_key(s.id) for s in row.steps):
            return 'landed'
        return 'unknown'
    if ROW_RE.match(dep):
        return 'row' if dep in rm.rows else 'gone'
    return 'unknown'


def text_fields(rm):
    """Every step and closing text the cut wrote, with where it is."""
    for s in rm.steps.values():
        for name, value in s.fields():
            if value:
                yield s.id, name, value
    for r in rm.rows.values():
        if r.closes:
            yield r.id, 'closes', r.closes
        for i, f in enumerate(r.findings):
            yield r.id, f'finding {i + 1}', f


def row_text_fields(rm):
    """Every row's own text, kept as it was before the cut."""
    for r in rm.rows.values():
        for name in ('item', 'today', 'exit', 'depends_text'):
            value = getattr(r, name)
            if value:
                yield r.id, name, value


def all_text_fields(rm):
    yield from text_fields(rm)
    yield from row_text_fields(rm)


# ---- the checks --------------------------------------------------------------

def check_shape(rm, tree, ctx):
    out = []
    for r in rm.rows.values():
        if r.tables != 1:
            out.append(Finding('shape', r.id, 'table', f'{r.tables} step tables'))
        if not r.steps:
            out.append(Finding('shape', r.id, 'steps', 'no steps'))
        last = None
        for s in r.steps:
            m = STEP_RE.match(s.id)
            if not m or m.group('row') != r.id:
                out.append(Finding('shape', s.id, 'id', f'step ID does not belong to {r.id}'))
                continue
            if s.kind not in KINDS:
                out.append(Finding('shape', s.id, 'kind', f'kind {s.kind!r} is not one of the eight'))
            for name, value in (('what', s.what), ('exit', s.exit), ('where', s.where_text)):
                if not value.strip() or value.strip() == '—' and name != 'where':
                    out.append(Finding('shape', s.id, name, f'{name} is empty'))
            if last is not None and step_key(s.id) <= step_key(last):
                out.append(Finding('shape', s.id, 'order', f'comes after {last}'))
            last = s.id
    return out


def check_deps(rm, tree, ctx):
    out = []
    for s in rm.steps.values():
        for d in s.depends:
            kind = resolve_dep(rm, d)
            if kind in ('unknown', 'gone'):
                what = 'names no row, step or H row in the roadmap' if kind == 'unknown' else \
                    "names a row that is not in the roadmap (left it, or never was)"
                out.append(Finding('deps', s.id, d, what))
                continue
            if d == s.row:
                out.append(Finding('deps', s.id, d, 'depends on its own row'))
            m = STEP_RE.match(d)
            if m and m.group('row') == s.row and step_key(d) >= step_key(s.id):
                out.append(Finding('deps', s.id, d, 'depends on itself or a later step of its row'))
    for cyc in ctx['cycles']:
        members = sorted(set(cyc[:-1]))
        out.append(Finding('deps', members[0], 'cycle', ' -> '.join(cyc)))
    return out


def check_paths(rm, tree, ctx):
    out = []
    for s in rm.steps.values():
        for p, new in s.where:
            if new and tree.exists(p):
                out.append(Finding('paths', s.id, p, 'marked (new) but exists'))
            elif not new and not tree.exists(p):
                if p not in ctx['new_paths']:
                    out.append(Finding('paths', s.id, p, 'does not exist and is not marked (new)'))
                elif not any(c.id in ctx['before'][s.id] for c in creators(rm, ctx, p)[0]):
                    out.append(Finding('paths', s.id, p, 'another step creates it, and this one does not wait on it'))
    return out


def twins(a, b):
    """Two stems naming one thing: `lex` and `lexer`, `font` and `fonts`,
    `shape` and `shaping`."""
    if a == b:
        return False
    for x, y in ((a, b), (b, a)):
        if any(y == x + s for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')):
            return True
        # a final e drops before a vowel: `shape`, `shaping`; not `pdfe`, `pdfr`
        if x.endswith('e') and any(y == x[:-1] + s for s in ('ing', 'ion', 'ions')):
            return True
    return False


def check_new_home(rm, tree, ctx):
    """A new file beside an existing home for the same thing. Every step that
    names a file the tree lacks marks it `(new)`, so only the step that creates
    it, the first in the order, chose its place."""
    out = []
    in_dir = collections.defaultdict(set)
    for f in list(tree.files) + sorted(ctx['new_paths']):
        d, _, b = f.rpartition('/')
        in_dir[d].add(b)
    for path in sorted(ctx['new_paths']):
        base = path.rsplit('/', 1)[-1]
        d = path.rsplit('/', 1)[0] if '/' in path else ''
        stem = base.rsplit('.', 1)[0]
        top = path.split('/', 1)[0]
        for s in creators(rm, ctx, path)[0]:
            named = {p for p, _ in s.where}
            if base.endswith('.rs') and base != 'mod.rs' and tree.exists(f'{d}/{stem}/mod.rs'):
                out.append(Finding('new-home', s.id, path, f'the module already exists as {d}/{stem}/mod.rs'))
            if base == 'mod.rs' and d and tree.exists(d + '.rs'):
                out.append(Finding('new-home', s.id, path, f'the module already exists as {d}.rs'))
            if top in CRATE_ROOTS and '/' in path and base not in COMMON_BASENAMES:
                root = '/'.join(path.split('/')[:CRATE_ROOTS[top]])
                # The same name in a folder above or below this one; siblings
                # (`jpeg/encode.rs`, `jxl/encode.rs`) are the crate's layout.
                same = [f for f in tree.by_base.get(base, []) if f.startswith(root + '/') and f not in named
                        and (d.startswith(f.rsplit('/', 1)[0] + '/') or f.startswith(d + '/'))]
                if same:
                    out.append(Finding('new-home', s.id, path,
                                       f'{", ".join(sorted(same))} already exists, and the step does not name it'))
            if '.' in base:
                for other in sorted(in_dir[d]):
                    if '.' in other and other.rsplit('.', 1)[1] == base.rsplit('.', 1)[1] \
                            and twins(stem, other.rsplit('.', 1)[0]):
                        where = 'in the tree' if tree.exists(f'{d}/{other}') else 'planned by another step'
                        out.append(Finding('new-home', s.id, path, f'{d}/{other}, {where}, names the same thing'))
            parts = stem.split('_')
            if len(parts) >= 2 and parts[-1] in GENERIC_STEMS and d:
                family = [f for f in tree.files if f.startswith(d + '/') and '/' not in f[len(d) + 1:]
                          and f[len(d) + 1:].startswith('_'.join(parts[:-1]) + '_')]
                if len(family) >= 2:
                    out.append(Finding('new-home', s.id, path,
                                       f'the family is already split: {", ".join(sorted(family)[:6])}'))
    return out


def file_refs(text):
    """Every file reference in a text, with the line range it cites; a bare
    `:N` cites the file named before it."""
    last = None
    for m in TICK.finditer(text):
        span = m.group(1).strip()
        if any(c in span for c in ' *$<>{}') or span.startswith(('/', '-', 'http')):
            continue
        p = POINTER.match(span)
        if p:
            last = p.group('file')
            yield last, p.group('a'), p.group('b')
            continue
        b = BARE_POINTER.match(span)
        if b and last:
            yield last, b.group('a'), b.group('b')


def check_refs(rm, tree, ctx, rows_text=False):
    out = []
    name = 'row-refs' if rows_text else 'refs'
    fields = row_text_fields(rm) if rows_text else text_fields(rm)
    new_paths = ctx['new_paths']
    for subject, field, text in fields:
        local = []
        if not rows_text and subject in rm.steps:
            local = [p for p, _ in rm.steps[subject].where]
        for ref, a, b in file_refs(text):
            # `design/x.md` finds `docs/design/x.md`: a partial path resolves by its end.
            hits = tree.resolve(ref)
            if not hits:
                if any(np == ref or np.endswith('/' + ref) for np in new_paths):
                    continue
                if rows_text and recorded(rm.rows[subject], ref):
                    continue
                out.append(Finding(name, subject, f'{field}: {ref}', 'no such file in the tree, and no step creates it'))
                continue
            if a is None:
                continue
            lo, hi = int(a), int(b) if b else int(a)
            if len(hits) > 1:
                near = [h for h in hits if any(h.split('/')[:2] == lp.split('/')[:2] for lp in local)]
                if len(near) == 1:
                    hits = near
                elif all(len(tree.lines(h)) < hi for h in hits):
                    # Whichever file it means, none has the line.
                    hits = [max(hits, key=lambda h: len(tree.lines(h)))]
                else:
                    continue
            lines = tree.lines(hits[0])
            if lo < 1 or hi < lo or hi > len(lines):
                if rows_text and recorded(rm.rows[subject], ref):
                    continue
                out.append(Finding(name, subject, f'{field}: {ref}:{a}' + (f'-{b}' if b else ''),
                                   f'{hits[0]} has {len(lines)} lines'))
    return out


# What a row's finding says of a file that has left the tree.
GONE = re.compile(r'\b(?:not in the tree|no longer|gone|removed|deleted|renamed|moved|absent)\b', re.I)


def recorded(row, ref):
    """A row's own findings record the file a pointer in its text cites as gone."""
    base = ref.rsplit('/', 1)[-1]
    return any(base in f and GONE.search(f) for f in row.findings)


def check_ref_content(rm, tree, ctx, rows_text=False):
    """`name` (`file:N`): the name is at line N, in the fifteen lines above it or
    in the three below."""
    out = []
    name = 'row-pointer' if rows_text else 'pointer'
    fields = row_text_fields(rm) if rows_text else text_fields(rm)
    pat = re.compile(r'`([A-Za-z_][\w:]*)(?:\([^`]*\))?`\s*(?:\(|at |is at |, at )`([\w./+-]+\.(?:rs|py|yml|toml|h|cs|go|java|rb|js|mjs|sh|swift)):(\d+)(?:-(\d+))?`')
    for subject, field, text in fields:
        local = []
        if not rows_text and subject in rm.steps:
            local = [p for p, _ in rm.steps[subject].where]
        for m in pat.finditer(text):
            ident = m.group(1).split('::')[-1]
            if not WORD_RE.fullmatch(ident) or not (camel(ident) or SNAKE.match(ident) or SCREAM.match(ident)
                                                    or ident.islower()):
                continue
            if re.match(r'\s*,\s*`', text[m.end():]) or re.search(r'\b(no|without|not)\s*$', text[:m.start()]):
                continue
            hits = tree.resolve(m.group(2))
            if len(hits) > 1:
                hits = [h for h in hits if any(h.split('/')[:2] == lp.split('/')[:2] for lp in local)] or hits
            if len(hits) != 1:
                continue
            lines = tree.lines(hits[0])
            lo = int(m.group(3))
            hi = int(m.group(4)) if m.group(4) else lo
            window = '\n'.join(lines[max(0, lo - 16):min(len(lines), hi + 3)])
            if re.search(r'\b' + re.escape(ident) + r'\b', window):
                continue
            if rows_text and recorded(rm.rows[subject], m.group(2)):
                continue
            out.append(Finding(name, subject, f'{field}: {ident}@{m.group(2)}:{m.group(3)}',
                               f'`{ident}` is not at {hits[0]}:{m.group(3)}, in the fifteen lines above it or the three below'))
    return out


def check_tests(rm, tree, ctx):
    """`file.rs` `test_name`: a test named as existing is in the file named."""
    out = []
    pat = re.compile(r"`([\w/.-]+\.rs)`(\s*\(new\b[^)]*\))?(?:'s)?\s+(?:\(tests\)\s+)?`([a-z][a-z0-9_]*_[a-z0-9_]+)`(\s*\(new\b[^)]*\))?")
    for subject, field, text in text_fields(rm):
        for m in pat.finditer(text):
            # the clause: from the sentence or semicolon before to the one after
            start = max(text.rfind(';', 0, m.start()), text.rfind('. ', 0, m.start())) + 1
            ends = [i for i in (text.find(';', m.end()), text.find('. ', m.end())) if i >= 0]
            clause = text[start:min(ends) + 1 if ends else len(text)]
            if not TEST_CLAIM.search(clause) or TEST_NEW.search(clause):
                continue
            file, file_new, test, test_new = m.group(1), m.group(2), m.group(3), m.group(4)
            if file_new or test_new:
                continue
            hits = tree.resolve(file)
            if not hits:
                continue
            if any(re.search(r'\bfn ' + re.escape(test) + r'\b', tree.read(h)) for h in hits):
                continue
            # a test another step builds
            if test in ctx['introduced']:
                continue
            out.append(Finding('tests', subject, f'{field}: {file} {test}', f'no `fn {test}` in {", ".join(hits)}'))
    return out


def name_spans(text):
    """The code names a text writes in backticks, each with its span and its
    match: a path's last segment, a dotted name's last part
    (`Intl.PluralRules`), or a camel, snake or screaming name."""
    for m in TICK.finditer(text):
        span = m.group(1).strip()
        if ' ' in span or span.startswith(('/', '-', '.')) or '/' in span:
            continue
        span = re.sub(r'\(.*$', '', span)
        if POINTER.match(span) or BARE_POINTER.match(span):
            continue
        last = span.split('::')[-1]
        if '.' in last and all(WORD_RE.fullmatch(x) for x in last.split('.')):
            last = last.split('.')[-1]
        if SNAKE.match(last) and last.count('_') >= 4:
            continue
        if '::' in span or camel(last) or SNAKE.match(last) or SCREAM.match(last):
            if WORD_RE.fullmatch(last):
                yield last, span, m


def idents_in(text, spans=False):
    for name, span, _ in name_spans(text):
        yield (name, span) if spans else name


# The names a text marks new: each span, or list of spans, just before `(new`.
NEW_SPANS = re.compile(r'((?:`[^`]+`(?:,? and |, | or ))*`[^`]+`)\s*\(new\b')


def introduced(rm):
    """Names a step builds: those in any step's What, and those a text marks
    new."""
    names = set()
    for s in rm.steps.values():
        names.update(idents_in(s.what))
        for _, value in s.fields():
            for m in NEW_SPANS.finditer(value):
                names.update(idents_in(m.group(1)))
    return names


def check_idents(rm, tree, ctx):
    """A code name the tree does not have and no step builds. An exit names
    the tests that close its step, so a plain snake name there is a test the
    step adds, not a claim about the code."""
    out = []
    for subject, field, text in text_fields(rm):
        for name, span in sorted(set(idents_in(text, spans=True))):
            if name in tree.idents or name in ctx['introduced']:
                continue
            if field == 'exit' and SNAKE.match(name) and '::' not in span:
                continue
            out.append(Finding('idents', subject, f'{field}: {name}', f'no `{name}` in the code, and no step builds it'))
    return out


def check_verbs(rm, tree, ctx):
    out = []
    have = tree.verbs()
    if not have:
        return [Finding('verbs', 'tools/tpdf/src/main.rs', 'dispatch', 'no verb read from the dispatch, which the '
                        'reader no longer matches')]
    built = set()
    for s in rm.steps.values():
        if any(p == 'tools/tpdf/src/main.rs' for p, _ in s.where):
            built.update(re.findall(r'`tpdf ([a-z][a-z0-9-]*)', s.what + ' ' + s.exit))
    for subject, field, text in text_fields(rm):
        for verb in sorted(set(re.findall(r'`tpdf ([a-z][a-z0-9-]*)', text))):
            if verb not in have and verb not in built:
                out.append(Finding('verbs', subject, f'{field}: {verb}', f'`tpdf` has no `{verb}`, and no step adds it'))
    return out


def creators(rm, ctx, path):
    """The steps naming a new path that no other step naming it comes before."""
    named = [s for s in rm.steps.values() if any(p == path and new for p, new in s.where)]
    before = ctx['before']
    return [s for s in named if not any(o.id in before[s.id] for o in named if o is not s)], named


def homes(path, tree, new_paths):
    """The new files that declare a directory a new path sits in: a crate's
    `Cargo.toml` or `src/lib.rs`, or a module's `mod.rs` or `<dir>.rs`."""
    parts = path.split('/')
    for i in range(len(parts) - 1, 0, -1):
        d = '/'.join(parts[:i])
        if tree.exists(d):
            break
        for home in (f'{d}/Cargo.toml', f'{d}/mod.rs', f'{d}.rs', f'{d}/lib.rs'):
            if home != path and home in new_paths:
                yield home


def check_double_create(rm, tree, ctx):
    """Two rows that could each be first to create one source or doc file. A
    `(new)` path is one the tree lacks, so every step naming it says `(new)`;
    the first in the order creates it. One row's steps share a plan, and a test
    file or a directory only gathers, so neither is a second home."""
    out = []
    before = ctx['before']
    for path in sorted(ctx['new_paths']):
        first, named = creators(rm, ctx, path)
        base = path.rsplit('/', 1)[-1]
        rows = sorted({s.row for s in first})
        if len(rows) > 1 and '.' in base and '/tests/' not in '/' + path:
            out.append(Finding('double-create', '|'.join(sorted(s.id for s in first)), path,
                               f'{len(first)} steps of {len(rows)} rows could each be first to create it'))
        if path.startswith('docs/design/'):
            for s in first:
                if s.kind != 'design':
                    out.append(Finding('design-first', s.id, path,
                                       f'a {s.kind} step could be the first to write a design doc'))
    crates = collections.defaultdict(set)
    for s in rm.steps.values():
        for p, new in s.where:
            parts = p.split('/')
            if new and len(parts) > 2 and parts[0] == 'crates' and not tree.exists('/'.join(parts[:2])):
                crates['/'.join(parts[:2])].add(s.id)
    for crate, ids in sorted(crates.items()):
        if f'{crate}/Cargo.toml' not in ctx['new_paths']:
            out.append(Finding('no-crate', crate, 'Cargo.toml',
                               f'{len(ids)} steps create files in it, and no step creates its Cargo.toml'))
    for s in rm.steps.values():
        for p, new in s.where:
            if not new:
                continue
            for home in homes(p, tree, ctx['new_paths']):
                if p.endswith('/mod.rs') and home == p[:-len('/mod.rs')] + '.rs':
                    out.append(Finding('two-homes', s.id, p, f'{home} is planned too, and a module has one file'))
                    continue
                first, named = creators(rm, ctx, home)
                if s in named or any(o.id in before[s.id] for o in named) or all(o.row == s.row for o in first):
                    continue
                out.append(Finding('new-home-first', s.id, p,
                                   f'it sits in what {home} declares, which {", ".join(sorted(o.id for o in first))} '
                                   f'creates, and this step does not wait on it'))
    return out


# Words that say a function named before them in its segment is left as it
# is: it stays, is unchanged, untouched or unaffected, or already, still or
# never does something, or does it today (not "today's", the old behaviour a
# change replaces), as before. "Keeps" is not one: "`x` keeps its signature"
# and "`x` keeps each parsed space" change `x`.
MENTION_WORDS = re.compile(r"\b(?:stays?|unchanged|untouched|unaffected|already|today(?!['’]s)|still|never|"
                           r"as before)\b", re.I)
# Words that say a function named before them changes, whatever else its
# segment says: it changes, gains, adds, now does, is rewritten, replaced,
# extended, fixed or routed.
CHANGE_WORDS = re.compile(r"\b(?:changes|changed|gains?|gained|adds?|added|now|rewrit\w*|replac(?:es|ed)|"
                          r"extend(?:s|ed)|becomes?|became|fix(?:es|ed)|routes?|routed)\b", re.I)
# A segment that begins "which" after a step's or row's ID tells of that
# step's work: "after K-02.2, which changes what `parse_space` answers".
ANTECEDENT = re.compile(r'(?<![\w-])' + ROW_ID + r'(?:\.\d+[a-z]?)?$')
# A form that names a function to leave it be: an edit rather than of it,
# work beside it, built on it or as for it, as it does, and what it draws (not
# what a change changes it to answer). It holds for the naming it covers.
MENTION_OF = re.compile(r"(?:\brather\s+than\s+an\s+edit\s+of|\bbeside|\bbuilt\s+on|\bas\s+for)\s+`x+`|"
                        r"\bas\s+`x+`(?:\s+and\s+`x+`)?\s+(?:\w+s|do)\b|"
                        r"(?<!\bchanges )(?<!\bchange )(?<!\brewrites )(?<!\balters )"
                        r"\bwhat\s+(?:\w+'s\s+)?`x+`(?:\s+and\s+`x+`)?\s+\w+", re.I)
# A name that is a fuzz target's or a seed's, not the function's.
TARGET_NAME = re.compile(r'^\s*(?:fuzz target|seed)\b')
# Where a function's own segment of a clause ends: a comma before anything but
# the next name of a list, or "so".
SEGMENT_END = re.compile(r',\s+(?!`)|\s+so\s+')
# What a segment holding only names joins the next with: a list's words.
LIST_WORDS = re.compile(r'(?:`x+`|\b(?:and|or|the|a|an)\b|[,\s])*')


def code_blanked(text):
    """The text with the insides of its code spans blanked, its length kept."""
    chars = list(text)
    for m in TICK.finditer(text):
        for i in range(m.start() + 1, m.end() - 1):
            chars[i] = 'x'
    return ''.join(chars)


def paren_spans(text):
    """Where each outermost parenthesis of a text runs, code spans aside; one
    left open runs to the end."""
    spans, depth, start = [], 0, 0
    for i, c in enumerate(code_blanked(text)):
        if c == '(':
            if not depth:
                start = i
            depth += 1
        elif c == ')' and depth:
            depth -= 1
            if not depth:
                spans.append((start, i + 1))
    if depth:
        spans.append((start, len(text)))
    return spans


def clauses(text):
    """A text's clauses: cut after a full stop, colon or semicolon outside
    parentheses and code spans."""
    blank = code_blanked(text)
    inside = set()
    for a, b in paren_spans(text):
        inside.update(range(a, b))
    out, start = [], 0
    for m in re.finditer(r'[;:.](?=\s)', blank):
        if m.start() not in inside:
            out.append(text[start:m.end()])
            start = m.end()
    out.append(text[start:])
    return [c.strip() for c in out if c.strip()]


def outer(clause):
    """The clause with the insides of code spans and parenthesised text
    blanked, its length kept: a comma inside either cuts nothing, and a word
    inside a parenthesis says nothing of the function outside it."""
    chars = list(code_blanked(clause))
    for a, b in paren_spans(clause):
        for i in range(a, b):
            chars[i] = ' '
    return ''.join(chars)


def cites(text):
    """Whether a text cites a line of a file (`file.rs:N`, or a bare `:N`)."""
    for span in TICK.findall(text):
        p = POINTER.match(span.strip())
        if p and p.group('a') or BARE_POINTER.match(span.strip()):
            return True
    return False


def changed_functions(text, defs):
    """The functions of `defs` a step's text changes, each with the type it is
    named under (`Page::render`), or none: a function is changed when one of
    its namings is not a mention. A naming is read in its own segment of its
    clause (a segment of names alone reads on into the next, as a list does).
    It is a mention when a form around it leaves it be, or when a mention word
    follows it in the segment and no change word does, or when its segment
    tells of another step's work. A naming inside a parenthesis that cites a
    file's line is a citation. A clause that ends in a colon, names nothing and
    says a mention word and no change word makes the next clause's namings
    mentions ("These stay: `a` and `b`")."""
    out = set()
    carry = False
    for clause in clauses(text):
        shell = outer(clause)
        parens = paren_spans(clause)
        cuts = [0] + [m.start() for m in SEGMENT_END.finditer(shell)] + [len(clause)]
        namings = list(name_spans(clause))
        preamble = carry and not CHANGE_WORDS.search(shell)
        carry = clause.endswith(':') and not namings and bool(MENTION_WORDS.search(shell)) \
            and not CHANGE_WORDS.search(shell)
        for name, span, m in namings:
            if name not in defs or preamble or TARGET_NAME.match(clause[m.end():]):
                continue
            if any(a <= m.start() < b and cites(clause[a:b]) for a, b in parens):
                continue
            lo = max(c for c in cuts if c <= m.start())
            hi = min(c for c in cuts if c > m.start())
            while hi < len(clause) and LIST_WORDS.fullmatch(shell[lo:hi]):
                hi = min(c for c in cuts if c > hi)
            if any(x.start() <= m.start() - lo < x.end() for x in MENTION_OF.finditer(shell[lo:hi])):
                continue
            if re.match(r'[,\s]*which\b', shell[lo:hi]) and ANTECEDENT.search(clause[:lo].rstrip(', ')):
                continue
            after = shell[m.end():hi]
            if MENTION_WORDS.search(after) and not CHANGE_WORDS.search(after):
                continue
            out.add((span.split('::')[-2] if '::' in span else None, name))
    return out


def same_function(a, b):
    """Two namings of one function: a long or snake name by itself, a short one
    (`render`, which several types of a file may have) under one type, and
    never two names under two types."""
    (qa, na), (qb, nb) = a, b
    if na != nb or (qa is not None and qb is not None and qa != qb):
        return False
    return '_' in na or len(na) >= 8 or (qa is not None and qa == qb)


# A sentence that settles two unordered edits: "whichever lands second takes
# the other's form", naming the other row.
SETTLED = re.compile(r'\bwhichever (?:lands|comes) (?:second|later|last)\b', re.I)


def settles(rm, s, row):
    """Whether a sentence of a step's text settles its edit with a row's by
    saying what whichever lands second keeps."""
    for sentence in re.split(r'(?<=\.)\s+', s.text()):
        if SETTLED.search(sentence) and row in {x.split('.')[0] for x in ids_in(sentence, rm)}:
            return True
    return False


def check_edit_order(rm, tree, ctx):
    out = []
    before = ctx['before']
    by_file = collections.defaultdict(list)
    for s in rm.steps.values():
        for p, new in s.where:
            if not new and p.endswith(CODE_EXT) and tree.exists(p):
                by_file[p].append(s)
    for path, steps in sorted(by_file.items()):
        defs = tree.fns(path)
        if not defs:
            continue
        names = {s.id: changed_functions(s.what + ' ' + s.notes, defs) for s in steps}
        for i, a in enumerate(steps):
            for b in steps[i + 1:]:
                if a.row == b.row or a.id in before[b.id] or b.id in before[a.id]:
                    continue
                if settles(rm, a, b.row) or settles(rm, b, a.row):
                    continue
                shared = sorted({x[1] for x in names[a.id] for y in names[b.id] if same_function(x, y)})
                if shared:
                    pair = '|'.join(sorted((a.id, b.id)))
                    out.append(Finding('edit-order', pair, f'{path}: {", ".join(shared)}',
                                       'both change the same functions, with no order between them'))
    return out


def reaches(rm, ctx, sid, target):
    """Whether step `sid` waits, at any depth, on a row, a step or an H row."""
    req = ctx['before'][sid]
    if target in req:
        return True
    if target in rm.rows:
        return any(x.id in req for x in rm.rows[target].steps)
    return False


def carried(rm, ctx, row, hid):
    """A row carries an H row by waiting on it; where the H row has a default
    the work builds toward, by naming it in a step or the closing line; and
    otherwise by a clause of the closing line that names it as outside the
    row's exit."""
    if any(hid in s.depends or reaches(rm, ctx, s.id, hid) for s in row.steps):
        return True
    named = re.compile(r'(?<![\w-])' + hid + r'(?![\w-])')
    if rm.h.get(hid, {}).get('has default'):
        return any(named.search(s.text()) for s in row.steps) or bool(named.search(row.closes))
    return any(named.search(c) and OUTSIDE.search(c) and not NOT_OUTSIDE.search(c)
               for c in re.split(r'(?<=[.;])\s+', row.closes))


OUTSIDE = re.compile(r"\boutside (?:this|the row's|the) exit\b", re.I)
NOT_OUTSIDE = re.compile(r"(?:\bnot|\bnever|n['’]t)\s+(?:\w+\s+){0,2}outside\b", re.I)
# What an H row can block that is no row's work: a release, a publication, or
# what the docs may claim. No step can wait on those, so a clause of the
# Blocks cell about shipping or publishing holds no row, and a row whose
# claims or shipped work are blocked is not itself blocked.
SHIPPING = re.compile(r'^\s*(?:shipping|publishing)\b', re.I)
# One ID or a range of them: `P-07…P-12`, `FO-09a–e`, `FO-08a/b`, `A to B`.
ID_SPAN = ROW_ID + r'(?:(?:\s*(?:…|–|\.\.\.)\s*|—)(?:(?:[A-Z]{1,2}-)?\d+[a-z]?|[a-z])|(?:/[a-z])+| to ' + ROW_ID + r')?'
POSSESSOR = ID_SPAN + r"['’]s"
# Every possessor of the claims or the shipped work: `FO-08a/b's and FO-09a–e's claims`.
CLAIMED = re.compile(r'(?<![\w-])((?:' + POSSESSOR + r'(?:,? and |, | or ))*' + POSSESSOR +
                     r') (?:\w+ )?(?:claims?|shipped)\b')


def blocked(cell, rm):
    out = set()
    for clause in cell.split(';'):
        if SHIPPING.match(clause):
            continue
        ids = ids_in(clause, rm)
        for m in CLAIMED.finditer(clause):
            ids -= ids_in(m.group(1), rm)
        out |= ids
    return out


def check_row_deps(rm, tree, ctx):
    out = []
    for r in rm.rows.values():
        for ref in sorted(ids_in(r.depends_text, rm)):
            if ref == r.id or NG_RE.match(ref):
                continue
            kind = resolve_dep(rm, ref)
            if kind == 'landed':
                continue
            if kind in ('unknown', 'gone'):
                # Only a tier's or H's prefix is an ID: `PDF/X-4` is a level.
                if ref.split('-')[0] in TIERS + ['H']:
                    out.append(Finding('row-deps', r.id, ref, "the row's Depends on names something the roadmap does not hold"))
                continue
            for t in split_parts(rm, ref) if kind == 'split' else [ref]:
                if H_RE.match(t):
                    if carried(rm, ctx, r, t):
                        continue
                elif any(t in s.depends or reaches(rm, ctx, s.id, t) for s in r.steps):
                    continue
                out.append(Finding('row-deps', r.id, t, "the row's Depends on names it, and no step of the row waits on it"))
    return out


def check_h_blocks(rm, tree, ctx):
    out = []
    for hid, cells in sorted(rm.h.items(), key=lambda kv: int(kv[0][2:])):
        for ref in sorted(blocked(cells.get('Blocks', ''), rm)):
            if H_RE.match(ref) or NG_RE.match(ref):
                continue
            kind = resolve_dep(rm, ref)
            if ref.split('-')[0] in TIERS and kind in ('unknown', 'gone'):
                out.append(Finding('h-blocks', hid, ref, f"{hid}'s Blocks names {ref}, which the roadmap does not hold"))
                continue
            for t in split_parts(rm, ref) if kind == 'split' else [ref]:
                if t in rm.steps:
                    if not reaches(rm, ctx, t, hid) and hid not in rm.steps[t].depends:
                        out.append(Finding('h-blocks', hid, t, f'{hid} says it blocks {t}, which does not wait on it'))
                elif t in rm.rows:
                    if not carried(rm, ctx, rm.rows[t], hid):
                        out.append(Finding('h-blocks', hid, t, f'{hid} says it blocks {t}, and no step of it waits on {hid}'
                                           + (' or names it' if cells.get('has default') else '')))
    return out


def check_h_listed(rm, tree, ctx):
    """An H row a step waits on names the step's row, or the step, in its
    Blocks cell: each H row says what it blocks (H.md). A table of standing
    duties has no Blocks column, and a duty blocks nothing."""
    out = []
    for s in rm.steps.values():
        for d in s.depends:
            cells = rm.h.get(d)
            if not cells or 'Blocks' not in cells:
                continue
            named = ids_in(cells['Blocks'], rm)
            if s.row not in {x.split('.')[0] for x in named}:
                out.append(Finding('h-listed', d, s.row, f"{s.id} waits on {d}, and {d}'s Blocks cell does not name {s.row}"))
    return out


def check_wiring(rm, tree, ctx):
    out = []
    for s in rm.steps.values():
        text = s.text()
        if s.kind == 'design':
            continue
        need = {}
        for name, pat, row, least in INSTRUMENTS:
            if s.row != row and pat.search(text):
                need[name] = (row, least)
        for name, (row, least) in sorted(need.items()):
            req = ctx['before'][s.id]
            if not any(x.id in req and step_key(x.id)[0] >= least for x in rm.rows.get(row, NO_ROW).steps):
                out.append(Finding('wiring', s.id, name, f'names {name} and does not wait on {row}.{least} or later'))
        # `ci.yml` fires on `develop`, and `fuzz.yml` and `corpus.yml` are dispatched
        # there, only after MS-11; a run of another workflow the step names does not
        # need it.
        others = [w for w in re.findall(r'`(?:\.github/workflows/)?([\w-]+\.yml)`', text) +
                  [p.rsplit('/', 1)[-1] for p, _ in s.where if p.startswith('.github/workflows/')]
                  if w not in ('ci.yml', 'fuzz.yml', 'corpus.yml')]
        if s.kind in ('watch', 'ci', 'test', 'measure') and s.row != 'MS-11' and not others and \
                RUN_ON_DEVELOP.search(text):
            if not any(x.id in ctx['before'][s.id] for x in rm.rows.get('MS-11', NO_ROW).steps):
                out.append(Finding('wiring', s.id, 'MS-11', 'reads a run on `develop` and does not wait on MS-11'))
    return out


def check_fail_first(rm, tree, ctx):
    out = []
    for s in rm.steps.values():
        if s.kind not in ('build', 'fix'):
            continue
        m = SAME_OUTPUT.search(re.sub(r'`[^`]*`', '`x`', s.what))
        if m and not FAIL_FIRST.search(s.exit + ' ' + s.notes):
            out.append(Finding('fail-first', s.id, f'what: {m.group(0).lower()}',
                               'output that must not move, and no guard that fails first in the exit or the note'))
    return out


def facade_test(s, tree, ctx):
    """Whether the files a step names, in its Where, exit or note, hold a test
    of the facade's (`crates/tinker-pdf/tests/`), by path or by basename."""
    refs = [p for p, _ in s.where] + [r for r, _, _ in file_refs(s.exit + ' ' + s.notes)]
    for ref in refs:
        hits = tree.resolve(ref) + [p for p in ctx['new_paths'] if p == ref or p.endswith('/' + ref)]
        if any(h.startswith('crates/tinker-pdf/tests/') for h in hits + [ref]):
            return True
    return False


def check_both_builds(rm, tree, ctx):
    """A pin on text in the facade's tests that does not say which builds it
    holds in. Every step runs both builds where text is involved (the index's
    "How a step runs", 3), so only a pinned value needs saying: a page without
    an embedded face draws differently with `bundled-fonts`."""
    out = []
    for s in rm.steps.values():
        if s.kind not in ('build', 'fix', 'test'):
            continue
        text = TEXT_WORDS.search(s.what) or (re.search(r'\btext\b', s.what, re.I) and TEXT_DRAWN.search(s.what))
        if not text or not PINNED.search(s.exit) or not facade_test(s, tree, ctx):
            continue
        if any(w in (s.exit + ' ' + s.notes).lower() for w in BOTH_BUILDS):
            continue
        out.append(Finding('both-builds', s.id, f'exit: {PINNED.search(s.exit).group(0).lower()}',
                           'pins a value over text, and does not say it holds in both builds'))
    return out


def check_deadlock(rm, tree, ctx):
    """A step that waits on a whole row whose closing line names the step's
    row, directly or through what it waits on: a deadlock if that close counts
    on this row's work. A wait that comes through an earlier step of the
    step's own row is that step's finding."""
    out = {}
    rowdeps = {sid: {d for d in s.depends if d in rm.rows} for sid, s in rm.steps.items()}
    reach, names = {}, {}

    def rows_reached(sid):
        if sid not in reach:
            acc = set(rowdeps[sid])
            for u in ctx['before'][sid]:
                acc |= rowdeps.get(u, set())
            reach[sid] = acc
        return reach[sid]

    for s in rm.steps.values():
        for e in s.depends:
            via = {}
            if e in rm.rows:
                if e == s.row:
                    continue
                via[e] = None
                for t in rm.rows[e].steps:
                    for x in rows_reached(t.id):
                        via.setdefault(x, e)
            else:
                starts = [e] if e in rm.steps else split_parts(rm, e)
                if not starts or any(STEP_RE.match(t).group('row') == s.row for t in starts):
                    continue
                for t in starts:
                    for x in rows_reached(t):
                        via.setdefault(x, t)
            for x, through in sorted(via.items()):
                if x == s.row or (s.id, x) in out:
                    continue
                if x not in names:
                    names[x] = ids_in(rm.rows[x].closes, rm)
                if s.row in names[x] or any(n.startswith(s.row + '.') for n in names[x]):
                    how = f'through {through}' if through else 'directly'
                    out[(s.id, x)] = Finding('deadlock', s.id, x,
                                             f'waits on the whole row {x} ({how}), whose closing line names {s.row}: '
                                             f'a deadlock if that close counts on this row\'s work')
    return list(out.values())


def affirmed(pat, text):
    """Whether a match of `pat` in `text` is not denied by the words just before
    it ("no speed row", "without a speed row")."""
    return any(not re.search(r'\b(?:no|not|without|never)\b(?:\s+\S+){0,2}\s*$', text[:m.start()], re.I)
               for m in pat.finditer(text))


def check_speed_kind(rm, tree, ctx):
    """A step whose only work is a speed row is a `measure` step, a benchmark
    whose committed output is a number (the index's kinds); its only code is
    the benches'. `tail-wait` reads the kind: a speed row typed as other work
    is no tail, and a stall behind it goes unseen."""
    out = []
    for s in rm.steps.values():
        if s.kind in ('measure', 'design', 'doc', 'watch') or not affirmed(SPEED_ROW, s.what):
            continue
        if not any(p.endswith(CODE_EXT) and not p.startswith('crates/tinker-pdf/benches/') for p, _ in s.where):
            out.append(Finding('speed-kind', s.id, s.kind, f'its only work is a speed row, and it is a {s.kind} step'))
    return out


def acts(rm, ctx, sids):
    """The H rows without a default that steps wait on, at any depth: a
    person's acts."""
    out = set()
    for x in sids:
        out |= {h for h in ctx['before'][x] | set(rm.steps[x].depends) if h in rm.h and not rm.h[h]['has default']}
    return out


def tail(rm, ctx, row):
    """A row's tail: its `watch` and `measure` steps that no other kind of its
    steps waits on, at any depth. It observes the row's work; it adds none."""
    steps = [x.id for x in row.steps]
    t = {x for x in steps if rm.steps[x].kind in ('watch', 'measure')}
    changed = True
    while changed:
        changed = False
        for x in sorted(t):
            if any(x in ctx['before'][o] for o in steps if o not in t):
                t.discard(x)
                changed = True
    return t


def dep_acts(rm, ctx, e):
    """The person's acts one dependency brings: an H row, a step, a split
    step's parts or a whole row."""
    if e in rm.h:
        return {e} if not rm.h[e]['has default'] else set()
    if e in rm.steps:
        return acts(rm, ctx, [e])
    if e in rm.rows:
        return acts(rm, ctx, [x.id for x in rm.rows[e].steps])
    return acts(rm, ctx, split_parts(rm, e))


def other_acts(rm, ctx, s, d):
    """The person's acts a step waits on through every dependency but `d`."""
    return set().union(*[dep_acts(rm, ctx, e) for e in s.depends if e != d])


def whole_row_waits(rm, ctx):
    """Each wait on a whole row, with the person's acts it alone brings: those
    the row's steps wait on and the step's other dependencies do not."""
    for s in rm.steps.values():
        for d in s.depends:
            if d in rm.rows:
                extra = acts(rm, ctx, [x.id for x in rm.rows[d].steps]) - other_acts(rm, ctx, s, d)
                if extra:
                    yield s, d, extra


def check_tail_wait(rm, tree, ctx):
    """A step that waits on whole rows, and through them on a person's act that
    only their tails wait on: machine work stalled on a decision it does not
    need (H.md). Every waited row's tail is left out at once, so two tails
    bringing one act do not hide each other. A `watch` or `measure` step in
    its own row's tail may mean to wait on a tail; one that other kinds of its
    row's steps wait on is their prerequisite, and stalls them. The acts MS-11
    waits on are taken with MS-11, second in the order of work, after CD-19
    and before every other row, so no other row stalls on them."""
    out = []
    tails = {}

    def tail_of(rid):
        if rid not in tails:
            tails[rid] = tail(rm, ctx, rm.rows[rid])
        return tails[rid]

    front = acts(rm, ctx, [x.id for x in rm.rows['MS-11'].steps]) if 'MS-11' in rm.rows else set()
    for s in rm.steps.values():
        rows = [d for d in s.depends if d in rm.rows]
        if not rows or (s.kind in ('watch', 'measure') and s.id in tail_of(s.row)):
            continue
        full, kept = set(), set()
        for e in s.depends:
            if e in rm.rows:
                full |= acts(rm, ctx, [x.id for x in rm.rows[e].steps])
                kept |= acts(rm, ctx, [x.id for x in rm.rows[e].steps if x.id not in tail_of(e)])
            else:
                # An act the step reaches another way is no stall the tail brings.
                mine = dep_acts(rm, ctx, e)
                full |= mine
                kept |= mine
        stalled = full - kept - front
        for d in rows:
            mine = stalled & acts(rm, ctx, sorted(tail_of(d)))
            if mine:
                out.append(Finding('tail-wait', s.id, d, f'waits on all of {d}, and so on {", ".join(sorted(mine))}, '
                                                        f'which only its tail ({", ".join(sorted(tail_of(d)))}) waits on'))
    return out


def check_tooling(rm, tree, ctx):
    out = []
    for subject, field, text in all_text_fields(rm):
        for m in TOOLING.finditer(text):
            out.append(Finding('tooling', subject, f'{field}: {m.group(1)}', 'names a file of the cut\'s working folder, which is not in the tree'))
    return out


def check_step_refs(rm, tree, ctx):
    out = []
    for subject, field, text in all_text_fields(rm):
        for sid in sorted(set(STEP_IN_TEXT.findall(text))):
            kind = resolve_dep(rm, sid)
            if kind in ('unknown', 'gone'):
                out.append(Finding('step-refs', subject, f'{field}: {sid}', 'names a step the roadmap does not hold'))
        for hid in sorted(set(H_IN_TEXT.findall(text))):
            if hid not in rm.h:
                out.append(Finding('step-refs', subject, f'{field}: {hid}', 'names an H row H.md does not hold'))
    return out


CHECKS = collections.OrderedDict([
    ('shape', check_shape), ('deps', check_deps), ('paths', check_paths), ('new-home', check_new_home),
    ('refs', check_refs), ('row-refs', lambda rm, t, c: check_refs(rm, t, c, rows_text=True)),
    ('pointer', check_ref_content), ('row-pointer', lambda rm, t, c: check_ref_content(rm, t, c, rows_text=True)),
    ('tests', check_tests), ('idents', check_idents), ('verbs', check_verbs),
    ('double-create', check_double_create), ('edit-order', check_edit_order), ('row-deps', check_row_deps),
    ('h-blocks', check_h_blocks), ('h-listed', check_h_listed), ('wiring', check_wiring),
    ('fail-first', check_fail_first),
    ('both-builds', check_both_builds), ('deadlock', check_deadlock), ('speed-kind', check_speed_kind),
    ('tail-wait', check_tail_wait), ('tooling', check_tooling), ('step-refs', check_step_refs),
])
# Checks that report under more than one name.
EMITS = {'double-create': {'double-create', 'design-first', 'new-home-first', 'two-homes', 'no-crate'}}


def context(rm):
    before, cycles = closure(rm)
    new_paths = {p for s in rm.steps.values() for p, new in s.where if new}
    ctx = {'before': before, 'cycles': cycles, 'new_paths': new_paths}
    ctx['introduced'] = introduced(rm)
    return ctx


def run(repo, only=None):
    """The roadmap, its findings (each once) and what could not be read."""
    rm = Roadmap(repo)
    if rm.errors:
        return rm, None, [f'the roadmap: {e}' for e in rm.errors]
    tree = Tree(repo)
    ctx = context(rm)
    findings, seen = [], set()
    for name, fn in CHECKS.items():
        if only and name not in only:
            continue
        for f in fn(rm, tree, ctx):
            if f.key() not in seen:
                seen.add(f.key())
                findings.append(f)
    if tree.errors:
        return rm, None, [f'the tree: {e}' for e in tree.errors]
    return rm, findings, []


def read_accepted(path):
    """The acceptances by key, and the lines that are not one; None for the
    acceptances when the file cannot be read."""
    accepted, problems = {}, []
    if not os.path.exists(path):
        return None, [f'the acceptance file: {path}: missing']
    name = os.path.basename(path)
    try:
        with open(path, encoding='utf-8') as f:
            lines = f.read().split('\n')
    except (OSError, UnicodeDecodeError) as exc:
        return None, [f'the acceptance file: {path}: cannot be read ({type(exc).__name__})']
    known = set(CHECKS) | set().union(*EMITS.values())
    for n, line in enumerate(lines, 1):
        if not line.strip() or line.startswith('#'):
            continue
        parts = line.split('\t')
        if len(parts) != 4 or not all(p.strip() for p in parts):
            problems.append(f'{name}:{n}: four tab-separated fields, none empty: check, subject, item, reason')
            continue
        if parts[0] not in known:
            problems.append(f'{name}:{n}: no check is called {parts[0]!r}')
            continue
        key = tuple(parts[:3])
        if key in accepted:
            problems.append(f'{name}:{n}: accepts the same finding twice')
        accepted[key] = (parts[3], n)
    return accepted, problems


def main(argv):
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser()
    ap.add_argument('--repo', default=os.path.normpath(os.path.join(here, '..', '..')))
    ap.add_argument('--accepted', default=os.path.join(here, 'accepted.tsv'))
    ap.add_argument('--only', default='')
    ap.add_argument('--quiet', action='store_true', help='print unexplained findings only')
    ap.add_argument('--waits', action='store_true',
                    help='report each wait on a whole row that brings a person\'s act the step has no other way to')
    args = ap.parse_args(argv)
    only = set(x.strip() for x in args.only.split(',') if x.strip())
    emitted = {n: parent for parent, names in EMITS.items() for n in names}
    unknown = only - set(CHECKS) - set(emitted)
    if unknown:
        print(f'no such check: {", ".join(sorted(unknown))}; the checks are {", ".join(CHECKS)}, and '
              f'{", ".join(sorted(set(emitted) - set(CHECKS)))} within double-create')
        return 2
    shown = set()
    for n in only:
        shown |= EMITS.get(n, {n})
    rm, findings, errors = run(args.repo, {emitted.get(n, n) for n in only})
    if errors:
        for e in errors:
            print(f'cannot read {e}')
        return 2
    if args.waits:
        ctx = context(rm)
        waits = list(whole_row_waits(rm, ctx))
        for s, d, extra in waits:
            print(f'{s.id:10} waits on all of {d:7} and so on {", ".join(sorted(extra))}')
        print(f'\n{len(waits)} whole-row waits by {len({s.id for s, _, _ in waits})} steps bring a person\'s act '
              f'the step waits on no other way')
        return 0
    if only:
        findings = [f for f in findings if f.check in shown]
    accepted, problems = read_accepted(args.accepted)
    if accepted is None:
        for p in problems:
            print(f'cannot read {p}')
        return 2
    unexplained = 0
    used = set()
    by_check = collections.OrderedDict((c, []) for c in CHECKS)
    for f in findings:
        by_check.setdefault(f.check if f.check in by_check else next(k for k, v in EMITS.items() if f.check in v), []).append(f)
    for check, items in by_check.items():
        for f in items:
            acc = accepted.get(f.key())
            if acc:
                used.add(f.key())
                if not args.quiet:
                    print(f'accepted  {f.check:13} {f.subject:22} {f.item}  ({acc[0]})')
            else:
                unexplained += 1
                print(f'FINDING   {f.check:13} {f.subject:22} {f.item}  — {f.message}')
    stale = []
    for key, (reason, n) in accepted.items():
        if key not in used and (not only or key[0] in shown):
            stale.append(f'{os.path.basename(args.accepted)}:{n}: matches no finding: {key[0]} {key[1]} {key[2]}')
    for p in problems + stale:
        print(p)
    total = len(findings)
    print(f'\n{len(rm.rows)} rows, {len(rm.steps)} steps, {len(rm.h)} H rows read; '
          f'{total} findings, {total - unexplained} accepted, {unexplained} unexplained; '
          f'{len(stale)} stale acceptances; {len(problems)} malformed acceptances')
    return 1 if unexplained or stale or problems else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
