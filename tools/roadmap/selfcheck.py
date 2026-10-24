"""Hold every rule of sweep.py to a fixture it must fire on and a twin it must not.

usage: python3 -I tools/roadmap/selfcheck.py [--inject]

Builds a small roadmap and tree in a temporary folder: every tier file, the
index's counts, an H.md with a table that has a default, one that has none
and a table of standing duties, an NG.md, and the few files the cases point
at. Each case is a step, a row or an H row written to trip one rule, beside a
twin that differs in the one respect the rule reads. The sweep's findings over
the fixture must be exactly the expected ones: a missing one is a rule that
stopped firing, an extra one a twin it no longer tells apart. Each way the
roadmap, the tree or the acceptance file can be unreadable is a variant of the
fixture that must exit 2 with its own message. The words a rule reads (a
mention, a guard, a negation, a range) are held by a table of phrases each
rule must match and twins it must not. The acceptance file, the exit codes,
`--only`, `--quiet` and `--waits` are held the same way.

With `--inject`, each check is knocked out, and each rule is put back broken
in the source, one at a time; the run reads what the fixture, the variants
and the table show each time, and fails on any change that shows nothing. A
rule whose break shows nothing is not held by this file.

Prints one line per check and rule; exits 1 on any failure.
"""
import contextlib
import io
import os
import re
import shutil
import sys
import tempfile
import types

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import sweep  # noqa: E402

STEP_HEAD = '| Step | Kind | What | Where | Exit | Depends on |\n| --- | --- | --- | --- | --- | --- |'


def table(rid, steps):
    out = [STEP_HEAD]
    for n, kind, what, where, exit_, deps in steps:
        sid = n if '-' in str(n) else f'{rid}.{n}'
        out.append(f'| {sid} | {kind} | {what} | {where} | {exit_} | {deps or "—"} |')
    return out + ['']


def row(rid, steps=(), depends='—', today='Absent.', closes=None, findings=(), more=(), item=None, exit_text=None):
    """A row as the renderer writes it; a step is (n, kind, what, where, exit, deps).
    No steps is a row with no step table; `more` is a second table's steps."""
    out = [f'## {rid} — Row {rid}', '', item or f'**Row {rid}**: the fixture.', '', f'**Today.** {today}', '',
           f'**Exit criterion.** {exit_text or "a test passes"}', '', f'**Doc:** — · **Depends on:** {depends}', '']
    if steps:
        out += table(rid, steps)
    if more:
        out += table(rid, more)
    out += [f'**Closes the row:** {closes or f"{rid}.1 meets the exit."}', '']
    if findings:
        out += ["**The tree at `5297133`, against this row's text:**", ''] + [f'- {f}' for f in findings] + ['']
    return '\n'.join(out)


def ok(n=1, what='A change.', where='`crates/tinker-pdf/src/lib.rs`', exit_='A test passes.', deps='', kind='build'):
    return (n, kind, what, where, exit_, deps)


def note(n, what_and_note, exit_='A test passes.', deps='', kind='build', where='`crates/tinker-pdf/src/lib.rs`'):
    return (n, kind, what_and_note, where, exit_, deps)


LIB = ['//! The fixture crate.', 'pub struct ExistingType;', 'pub fn existing_fn() {}'] + \
      [f'// line {i}' for i in range(4, 50)] + ['pub fn other_fn() {}'] + [f'// line {i}' for i in range(51, 71)]
EDIT2 = ['pub fn alpha_fn() {}', 'pub fn new_page() {}', 'pub fn nu_fn() {}', 'pub fn xi_fn() {}', 'pub fn pi_fn() {}']
MAIN = ('fn dispatch(verb: &str) {\n    match verb {\n        "info" => run(),\n'
        '        "merge" => writing::print(&options, writing::merge(&options)),\n        _ => {}\n    }\n}\n'
        'fn option(word: &str) {\n    match word {\n        "keep" => writing::Continuous::Keep,\n        _ => {}\n'
        '    }\n}\n')
TREE = {
    'crates/tinker-pdf/src/lib.rs': '\n'.join(LIB) + '\n',
    'crates/tinker-pdf/src/tables.rs': '//! Tables.\n',
    'crates/tinker-pdf/src/lexer.rs': '//! The lexer.\n',
    'crates/tinker-pdf/src/shape.rs': '//! Shapes.\n',
    'crates/tinker-pdf/src/fx_one.rs': '//! One of a family.\n',
    'crates/tinker-pdf/src/fx_two.rs': '//! Another of it.\n',
    'crates/tinker-pdf/src/deeper/shallow.rs': '//! A name a folder below holds.\n',
    'crates/tinker-pdf/src/sib/mod.rs': '//! A module with a folder.\n',
    'crates/tinker-pdf/src/sib/encode.rs': '//! A sibling.\n',
    'crates/tinker-pdf/src/edit.rs': 'pub fn shared_function() {}\npub fn other_function() {}\npub fn run() {}\n',
    'crates/tinker-pdf/src/edit2.rs': '\n'.join(EDIT2) + '\n',
    'crates/a/src/dup_name.rs': 'a\nb\nc\n',
    'crates/b/src/dup_name.rs': 'a\nb\nc\nd\ne\n',
    'crates/tinker-pdf-font/src/lib.rs': '//! The font crate.\n',
    'crates/tinker-pdf/obj/gen.cs': 'class ObjOnlyName {}\n',
    'crates/tinker-pdf/tests/existing.rs': '#[test]\nfn existing_test_name() {}\n',
    'crates/tinker-pdf/benches/engine.rs': '//! The benches.\n',
    'crates/tinker-pdf/benches/baseline.json': '{}\n',
    'tools/roadmap/helper.py': 'RoadmapOnlyName = 1\n',
    'tools/tpdf/src/main.rs': MAIN,
    'tools/tpdf/src/helper.rs': '//! A helper.\n',
    'xtask/src/sub/count.rs': '//! A count.\n',
    '.github/workflows/ci.yml': 'on: push\n',
    '.github/workflows/bench.yml': 'on: workflow_dispatch\n',
    '.github/workflows/fuzz.yml': 'on: workflow_dispatch\n',
    '.github/workflows/corpus.yml': 'on: workflow_dispatch\n',
    'docs/design/existing.md': '# Existing\n',
    'docs/verification.md': '# Verification\n',
    'testdata/golden/kept.txt': 'kept\n',
    'testdata/golden/actual/stale_only.txt': 'stale\n',
}
W = '`crates/tinker-pdf/src/lib.rs`'
E2 = '`crates/tinker-pdf/src/edit2.rs`'
BENCH = '`crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/benches/baseline.json`'

ROWS = {
    'MS': [
        row('MS-01', [ok(n) for n in range(1, 9)]),
        # an instrument's own row may name it
        row('MS-02', [ok(n) for n in range(1, 6)] + [ok(6, what="The job: MS-02's paired job runs.")]),
        row('MS-04', [ok(n) for n in range(1, 10)]),
        row('MS-05', [ok(n) for n in range(1, 4)]),
        # MS-11 reads its own runs on develop
        row('MS-11', [ok(kind='ci', deps='H-22', exit_='A run on `develop` is green.')]),
    ],
    'K': [
        # shape: a kind not of the eight, an empty exit; an order out of order; no table; a foreign ID, an empty
        # What; a blank Where (a dash is none); two step tables
        row('K-01', [ok(1, kind='debug'), ok(2, exit_='—'), ok(3), ok(4)]),
        row('K-02', [ok(2), ok(1)]),
        row('K-03', closes='The row has no steps yet.'),
        row('K-07', [ok('K-09.1'), ok(2, what='')]),
        row('K-11', [ok(1, where=''), ok(2, where='—')]),
        row('K-12', [ok(1)], more=[ok(2)]),
        # deps: a gone row, an unknown H row, a later step; the twin's names are real; its own row; and the
        # H row it waits on does not name K-04 (h-listed)
        row('K-04', [ok(1, deps='ZZ-99, H-999'), ok(2, deps='K-04.3'), ok(3, deps='MS-02.1, H-02')]),
        row('K-05', [ok(1, deps='K-06.1')]),
        row('K-06', [ok(1, deps='K-05.1')]),
        row('K-08', [ok(1, deps='K-08')]),
        # a split step: K-10.2 is K-10.2a and K-10.2b; naming it waits on both; H-38 blocks K-10.2, and only
        # K-10.2a waits on it; K-10.2b waits on all of AN-53, whose close names AN-52
        row('K-10', [ok(1), ok('K-10.2a', where='`crates/tinker-pdf/src/split_made.rs` (new)', deps='H-38'),
                     ok('K-10.2b', deps='AN-53'),
                     ok(3, where='`crates/tinker-pdf/src/split_made.rs`', deps='K-10.2')]),
    ],
    'P': [
        # paths: (new) on a file the tree has, a missing file unmarked, an unmarked file another step creates
        # without waiting; the twins wait, on the step or the whole row
        row('P-01', [ok(1, where='`crates/tinker-pdf/src/lib.rs` (new), `crates/nowhere/x.rs`, '
                                 '`crates/tinker-pdf/src/brand_new.rs` (new)'),
                     ok(2, where='`crates/tinker-pdf/src/later.rs`'),
                     ok(3, where='`crates/tinker-pdf/src/later.rs`', deps='P-02.1')]),
        row('P-02', [ok(1, where='`crates/tinker-pdf/src/later.rs` (new)')]),
        row('P-09', [ok(1, where='`crates/tinker-pdf/src/later2.rs`', deps='P-10')]),
        row('P-10', [ok(1, where='`crates/tinker-pdf/src/later2.rs` (new)')]),
        # new-home: a name a folder above holds; a sibling's name; a step naming the home too; a twin stem; a
        # module already a folder; a later step naming the file; a name a folder below holds; a family already
        # split; a mod.rs beside x.rs; a basename every crate repeats
        row('P-03', [ok(1, where='`crates/tinker-pdf/src/deep/tables.rs` (new)'),
                     ok(2, where='`crates/tinker-pdf/src/other/encode.rs` (new)'),
                     ok(3, where='`crates/tinker-pdf/src/deep2/tables.rs` (new), `crates/tinker-pdf/src/tables.rs`'),
                     ok(4, where='`crates/tinker-pdf/src/lex.rs` (new)'),
                     ok(5, where='`crates/tinker-pdf/src/sib.rs` (new)'),
                     ok(6, where='`crates/tinker-pdf/src/deep/tables.rs` (new)', deps='P-03.1'),
                     ok(7, where='`crates/tinker-pdf/src/shallow.rs` (new)'),
                     ok(8, where='`crates/tinker-pdf/src/fx_tests.rs` (new)'),
                     ok(9, where='`crates/tinker-pdf/src/tables/mod.rs` (new)'),
                     ok(10, where='`crates/tinker-pdf/src/deep3/lib.rs` (new)')]),
        # new-home in tools and xtask, whose roots differ, and a sibling there; a twin stem by a dropped e, and
        # two stems that are not twins
        row('P-11', [ok(1, where='`tools/tpdf/src/cmd/helper.rs` (new)'),
                     ok(2, where='`xtask/src/count.rs` (new)'),
                     ok(3, where='`xtask/src/other/count.rs` (new)'),
                     ok(4, where='`crates/tinker-pdf/src/shaping.rs` (new)'),
                     ok(5, where='`crates/tinker-pdf/src/pdfe.rs` (new)'),
                     ok(6, where='`crates/tinker-pdf/src/pdfr.rs` (new)')]),
        # design-first: two unordered steps of one row could each write the doc first
        row('P-12', [ok(1, where='`docs/design/two.md` (new)'), ok(2, kind='design', where='`docs/design/two.md` (new)')]),
        # refs: past the end, a file no one has; twins: the line exists, the file is planned; design/ is docs/;
        # a bare :N after a file; a name two files carry, both too short, and one long enough; a range upside
        # down; one file twice in a field; an extension added; a file only build output holds, and its twin
        row('P-04', [ok(1, exit_='`crates/tinker-pdf/src/lib.rs:999` and `missing_file.rs` hold.'),
                     ok(2, exit_='`crates/tinker-pdf/src/lib.rs:2` and `brand_new.rs` hold.'),
                     ok(3, exit_='`design/existing.md` and `design/missing.md` hold.'),
                     ok(4, exit_='`crates/tinker-pdf/src/lib.rs:2` and `:999` hold.'),
                     ok(5, exit_='`dup_name.rs:99` holds.'),
                     ok(6, exit_='`dup_name.rs:4` holds.'),
                     ok(8, exit_='`crates/tinker-pdf/src/lib.rs:5-3` holds.'),
                     ok(9, exit_='`missing_twice.rs` and `missing_twice.rs` hold.'),
                     ok(10, exit_='`missing.ppm` holds.'),
                     ok(11, exit_='`stale_only.txt` and `kept.txt` hold.')]),
        # pointer: a name away from its line; twins: at it, within the window above, after "not", in a list;
        # and a name two crates' files carry, read in the step's own crate (both refs and pointer)
        row('P-05', [note(1, 'A change. *Note:* `other_fn` (`crates/tinker-pdf/src/lib.rs:3`) moves; '
                             '`existing_fn` (`crates/tinker-pdf/src/lib.rs:3`) stays.'),
                     note(2, 'A change. *Note:* `other_fn` (`crates/tinker-pdf/src/lib.rs:60`) is near.'),
                     note(3, 'A change. *Note:* it is not `other_fn` (`crates/tinker-pdf/src/lib.rs:3`).'),
                     note(4, 'A change. *Note:* `other_fn` (`crates/tinker-pdf/src/lib.rs:3`, '
                             '`crates/tinker-pdf/src/lib.rs:50`).'),
                     note(5, 'A change. *Note:* `e` (`dup_name.rs:5`) holds.', where='`crates/a/src/dup_name.rs`'),
                     note(6, 'A change. *Note:* `e` (`dup_name.rs:5`) holds.', where='`crates/b/src/dup_name.rs`')]),
        # tests: a test claimed to exist that the file lacks; twins: it is there, a clause saying new, one a
        # step's What builds, a file the tree lacks; a sentence ends the clause, before it and after it
        row('P-06', [ok(1, exit_='`existing.rs` `a_missing_test` stays green.'),
                     ok(2, exit_='`existing.rs` `existing_test_name` stays green.'),
                     ok(3, exit_='`existing.rs` `another_missing_test`, all new, and a guard that still holds.'),
                     ok(4, exit_='`existing.rs` `introduced_elsewhere` stays green.'),
                     ok(5, what='`introduced_elsewhere` added.'),
                     ok(6, exit_='`nowhere_tests.rs` `a_kept_test` stays green.'),
                     ok(7, exit_='`existing.rs` `a_missing_test2` stays green. A new guard holds.'),
                     ok(8, exit_='A new guard holds. `existing.rs` `a_missing_test3` stays green.')]),
        # idents: a name no code has; twins: one the code has, a test an exit names, one another What builds, a
        # dotted name built elsewhere, a name marked new, a long snake name; names only build output and this
        # folder hold; camel names with humps of capitals and of a letter, and words of capitals; a name after
        # another's (new) mark, and a list marked new
        row('P-07', [note(1, 'A change. *Note:* `NoSuchThing` already reads it; `ExistingType` too; '
                             '`BuiltElsewhere` follows.', exit_='`a_brand_new_check`: it holds.'),
                     ok(2, what='`BuiltElsewhere` built.'),
                     ok(3, what='`Intl.PluralRules` built.'),
                     ok(4, exit_='The `PluralRules` tests pass.'),
                     note(5, 'A change. *Note:* `FreshlyMade` (new) holds it.'),
                     note(6, 'A change. *Note:* `a_very_long_snake_case_name` stays.'),
                     note(7, 'A change. *Note:* `ObjOnlyName` and `RoadmapOnlyName` already exist.'),
                     note(8, 'A change. *Note:* `XRefNoSuch` and `PdfNoSuchA` read it; `IDs` are words.'),
                     note(9, 'A change. *Note:* `FreshlyMade2` (new) replaces `GoneOldThing`, which already reads it.'),
                     note(10, 'A change. *Note:* `ListA`, `ListB` and `ListC` (new) hold it.'),
                     ok(11, what='The `release_notes.md` reader.')]),
        # verbs: a verb tpdf lacks, an option word that is no verb; twins: one it has, one a step adds
        row('P-08', [ok(1, exit_='`tpdf nosuch` and `tpdf info` print it.'),
                     ok(2, what='`tpdf newverb` added.', where='`tools/tpdf/src/main.rs`'),
                     ok(3, exit_='`tpdf newverb` prints it.', deps='P-08.2'),
                     ok(4, exit_='`tpdf keep` and `tpdf merge` print it.')]),
    ],
    'RC': [
        # double-create: two rows, no order; twins: an order, a shared test file, a folder, one row twice
        row('RC-01', [ok(where='`crates/tinker-pdf/src/dup.rs` (new)')]),
        row('RC-02', [ok(where='`crates/tinker-pdf/src/dup.rs` (new)')]),
        row('RC-03', [ok(where='`crates/tinker-pdf/src/ordered.rs` (new)')]),
        row('RC-04', [ok(where='`crates/tinker-pdf/src/ordered.rs` (new)', deps='RC-03.1')]),
        row('RC-05', [ok(where='`crates/tinker-pdf/tests/shared.rs` (new)')]),
        row('RC-06', [ok(where='`crates/tinker-pdf/tests/shared.rs` (new)')]),
        row('RC-15', [ok(where='`crates/tinker-pdf/data/shared_dir` (new)')]),
        row('RC-16', [ok(where='`crates/tinker-pdf/data/shared_dir` (new)')]),
        row('RC-17', [ok(1, where='`crates/tinker-pdf/src/same_row.rs` (new)'),
                      ok(2, where='`crates/tinker-pdf/src/same_row.rs` (new)')]),
        # design-first: a design doc first written by a build step; the twin by a design step
        row('RC-07', [ok(where='`docs/design/fresh.md` (new)')]),
        row('RC-08', [ok(kind='design', where='`docs/design/fresh2.md` (new)')]),
        # new-home-first: a file in a module another row creates, not waited on; twins: waits, one row's own
        row('RC-09', [ok(where='`crates/tinker-pdf/src/m/mod.rs` (new)')]),
        row('RC-10', [ok(where='`crates/tinker-pdf/src/m/x.rs` (new)')]),
        row('RC-11', [ok(where='`crates/tinker-pdf/src/m/y.rs` (new)', deps='RC-09.1')]),
        row('RC-14', [ok(1, where='`crates/tinker-pdf/src/m2/mod.rs` (new)'),
                      ok(2, where='`crates/tinker-pdf/src/m2/z.rs` (new)')]),
        # two-homes: `q.rs` and `q/mod.rs` both planned
        row('RC-12', [ok(where='`crates/tinker-pdf/src/q.rs` (new)')]),
        row('RC-13', [ok(where='`crates/tinker-pdf/src/q/mod.rs` (new)')]),
        # no-crate: a crate no step creates; twins: a crate whose Cargo.toml and src/lib.rs a step creates; a
        # file in it from a row that does not wait on that step
        row('RC-18', [ok(where='`crates/brand-new-crate/src/x.rs` (new)')]),
        row('RC-19', [ok(where='`crates/planned-crate/Cargo.toml` (new), `crates/planned-crate/src/lib.rs` (new)')]),
        row('RC-20', [ok(where='`crates/planned-crate/src/y.rs` (new)', deps='RC-19.1')]),
        row('RC-21', [ok(where='`crates/planned-crate/src/z.rs` (new)')]),
        row('RC-22', [ok(where='`crates/libonly-crate/src/lib.rs` (new)')]),
        row('RC-23', [ok(where='`crates/libonly-crate/src/w.rs` (new)')]),
    ],
    'FT': [
        # edit-order: two rows change one function, no order; twins: ordered, another function, mentions,
        # one row's own steps, a short name under two types (fires under one); and lib.rs is no exception
        row('FT-01', [ok(what='`shared_function` changed.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-02', [ok(what='`shared_function` changed again.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-03', [ok(what='`shared_function` changed last.', where='`crates/tinker-pdf/src/edit.rs`',
                         deps='FT-01.1, FT-02.1')]),
        row('FT-04', [ok(what='`other_function` changed.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-10', [ok(what='`shared_function` stays as it is.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-11', [ok(what='A merge built on `shared_function`.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-12', [ok(what='The `shared_function` fuzz target runs.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-13', [ok(what='A text device over what `shared_function` draws.',
                         where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-14', [ok(what='Regions as for `shared_function`; `shared_function` is unaffected.',
                         where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-15', [ok(1, what='`other_function` changed.', where='`crates/tinker-pdf/src/edit.rs`', deps='FT-04.1'),
                      ok(2, what='`other_function` changed too.', where='`crates/tinker-pdf/src/edit.rs`',
                         deps='FT-04.1')]),
        row('FT-16', [ok(what='`Canvas::run` changed.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-17', [ok(what='`Page::run` changed.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-18', [ok(what='`Canvas::run` changed again.', where='`crates/tinker-pdf/src/edit.rs`')]),
        row('FT-08', [ok(what='`existing_fn` changed.', where=W)]),
        row('FT-09', [ok(what='`existing_fn` changed again.', where=W)]),
        # a change read in its own segment, though another segment says "today"
        row('FT-20', [ok(what='`alpha_fn` routes the new format, where today it draws nothing.', where=E2)]),
        row('FT-40', [ok(what='`alpha_fn` changed.', where=E2)]),
        # one name under two types is two functions
        row('FT-29', [ok(what='`Canvas::new_page` changed.', where=E2)]),
        row('FT-30', [ok(what='`Writer::new_page` changed.', where=E2)]),
        # a note that settles the edit with the other row; one that names a third; one whose sentence naming
        # the other row says nothing of which lands second
        row('FT-31', [ok(what='`nu_fn` changed.', where=E2)]),
        row('FT-32', [note(1, '`nu_fn` changed too. *Note:* FT-31 also edits `nu_fn`; whichever lands second '
                              'keeps both.', where=E2)]),
        row('FT-33', [ok(what='`xi_fn` changed.', where=E2)]),
        row('FT-34', [note(1, '`xi_fn` changed too. *Note:* FT-99 also edits it; whichever lands second keeps '
                              'both.', where=E2)]),
        row('FT-36', [note(1, '`pi_fn` changed after FT-37 lands. *Note:* FT-99 also edits it; whichever lands '
                              'second keeps both.', where=E2)]),
        row('FT-37', [ok(what='`pi_fn` changed.', where=E2)]),
        # row-deps: a row's Depends on no step carries; the twin's step waits
        row('FT-05', [ok()], depends='FT-06'),
        row('FT-06', [ok()]),
        row('FT-07', [ok(deps='FT-06.1')], depends='FT-06'),
    ],
    'ED': [
        # h-blocks with no default: not waited on; waited on; outside this exit; a release; ED-05a/b
        row('ED-01', [ok()]),
        row('ED-02', [ok(deps='H-02')], depends='H-02'),
        row('ED-03', [ok()], closes="ED-03.1 meets the exit; H-03's reading is a dated record, outside this exit."),
        row('ED-04', [ok()]),
        row('ED-05a', [ok(deps='H-05')]),
        row('ED-05b', [ok()]),
        # a default: named in a step (ED-06), not (ED-07), a range's middle and its end (ED-07, ED-08, ED-08a)
        row('ED-06', [note(1, "A change. *Note:* Builds toward H-10's default.")]),
        row('ED-07', [ok()]),
        row('ED-08', [ok()], depends='H-02'),
        row('ED-08a', [ok()]),
        # a step an H row blocks, not waiting; its twin waits; a default named in a closing line
        row('ED-09', [ok(1, deps='H-07'), ok(2)]),
        row('ED-10', [ok()], depends='H-10', closes="ED-10.1 meets the exit, building toward H-10's default."),
        # "X to Y" ranges of rows and of steps: the middle does not wait
        row('ED-12', [ok(deps='H-11')]),
        row('ED-13', [ok()]),
        row('ED-14', [ok(deps='H-11')]),
        row('ED-15', [ok(1, deps='H-12'), ok(2), ok(3, deps='H-12')]),
        # an ID before a full stop; a default that is "none"; "not outside this exit"; a claim with an
        # adjective; a release in one clause and a row in the next; outside the exit in another clause; a
        # no-default H row named but not waited on
        row('ED-16', [ok()]),
        row('ED-17', [note(1, 'A change. *Note:* Builds toward H-20.')]),
        row('ED-18', [ok()], closes='ED-18.1 meets the exit; H-15 is not outside this exit.'),
        row('ED-19', [ok()]),
        row('ED-20', [ok()]),
        row('ED-21', [ok()]),
        row('ED-22', [ok()], closes="H-18's reading is needed; nothing here is outside this exit."),
        row('ED-23', [note(1, 'A change. *Note:* H-19 judges it.')]),
        # an em-dash range, whose middle does not wait; a spaced em dash, which is no range
        row('ED-24', [ok(deps='H-24')]),
        row('ED-25', [ok()]),
        row('ED-26', [ok(deps='H-24')]),
        row('ED-27', [ok(deps='H-25')]),
        row('ED-28', [ok()]),
        row('ED-29', [ok()]),
        # a range of H rows in a row's Depends on, whose middle no step waits on; its twin waits on all three
        row('ED-30', [ok(deps='H-31, H-33')], depends='H-31 to H-33'),
        row('ED-31', [ok(deps='H-31, H-32, H-33')], depends='H-31 to H-33'),
        # possessors of claims: a list, a range, a slash; one row before a possessor still blocked
        row('ED-32a', [ok()]), row('ED-32b', [ok()]),
        row('ED-33a', [ok()]), row('ED-33b', [ok()]), row('ED-33c', [ok()]),
        row('ED-34', [ok()]), row('ED-35', [ok()]),
        row('ED-36', [ok()]), row('ED-37', [ok()]),
        # publishing holds no row
        row('ED-38', [ok()]),
        # "isn't outside" is not outside; "Outside", capitalised, is
        row('ED-39', [ok()], closes="ED-39.1 meets the exit; H-30's reading isn't outside this exit."),
        row('ED-41', [ok()], closes="ED-41.1 meets the exit; H-37's reading: Outside this exit."),
        # a row's Depends on: a landed step is done; a split step is its parts; a level such as PDF/X-4 is no
        # ID; a row the roadmap lacks; a limit
        row('ED-42', [ok()], depends='AN-10.4'),
        row('ED-43', [ok(deps='K-10.2a')], depends='K-10.2'),
        row('ED-44', [ok()], depends='PDF/X-4'),
        row('ED-45', [ok()], depends='ED-99'),
        row('ED-46', [ok(1, deps='NG-99')], depends='NG-01'),
        row('ED-48', [ok(deps='NG-01')]),
        # an H row whose default is "not scheduled until ruled" has none: naming it carries nothing
        row('ED-49', [note(1, 'A change. *Note:* Builds toward H-40.')]),
    ],
    'AN': [
        # wiring: an instrument not waited on, waited on below its least step, at it; a run on develop of any
        # kind of step; a run of another workflow, named in the text or the Where; a design step; a build
        # step; MS-01's least steps; MS-04's and MS-05's
        row('AN-01', [ok(1, what="Timed in MS-02's paired job."),
                      ok(2, what="Timed in MS-02's paired job too.", deps='MS-02.6'),
                      ok(3, what="Timed in MS-02's paired job again.", deps='MS-02.5')]),
        row('AN-02', [ok(1, kind='watch', where='`.github/workflows/ci.yml`', exit_='A run on `develop` is green.'),
                      ok(2, kind='watch', where='`.github/workflows/ci.yml`', exit_='A run on `develop` is green.',
                         deps='MS-11.1')]),
        row('AN-13', [ok(1, kind='test', exit_='On a dispatched run on `develop` the book draws.'),
                      ok(2, kind='test', exit_='A `bench.yml` dispatch on `develop` reports it.'),
                      ok(3, kind='build', exit_='A run on `develop` is read.'),
                      ok(4, kind='test', where='`.github/workflows/bench.yml`', exit_='A run on `develop` is read.'),
                      ok(5, kind='watch', exit_='A `fuzz.yml` run on `develop` is green.'),
                      ok(6, kind='watch', exit_='A `corpus.yml` run on `develop` is green.'),
                      ok(7, kind='ci', exit_='A job on `develop` is green.'),
                      ok(8, kind='measure', exit_='A leg on `develop` is timed.')]),
        row('AN-14', [ok(kind='design', what="Add MS-02's paired job's gate to the doc.",
                         where='`docs/design/existing.md`')]),
        row('AN-15', [ok(1, what='Guard A1 flips from owed.', deps='MS-01.5'),
                      ok(2, what='Guard A2 flips from owed.', deps='MS-01.6'),
                      ok(3, what='The `wasm32-wasip1` guard leg runs.', deps='MS-01.7'),
                      ok(4, what='The `wasm32-wasip1` guard leg runs too.', deps='MS-01.8')]),
        row('AN-40', [ok(1, kind='measure', what="MS-04's render group gains 'a page'.", where=BENCH, deps='MS-04.4'),
                      ok(2, kind='measure', what="MS-04's render group gains 'a page' too.", where=BENCH,
                         deps='MS-04.5')]),
        row('AN-41', [ok(1, what="MS-05's bands hold.", deps='MS-05.2'),
                      ok(2, what="MS-05's bands hold too.", deps='MS-05.3')]),
        # fail-first: same output with no guard; twins: a guard, a name, a speed row, a guard in the note, a
        # measure step
        row('AN-03', [ok(1, what='The parse cached once per document.', exit_='The output is unchanged.'),
                      ok(2, what='The parse cached once per document.',
                         exit_='A `cfg(test)` count fails against a parse per call.'),
                      ok(3, what='`deduplicate_streams` exposed.', exit_='The option reads back.'),
                      ok(4, what='A speed row for the parse.', exit_='The row is in the set.'),
                      note(5, 'The parse cached. *Note:* Counted injections on each path.',
                           exit_='The output is unchanged.'),
                      ok(6, kind='measure', what='The parse cached once per document.',
                         exit_='The output is unchanged.')]),
        # both-builds: a pin over text, no word of the builds; twins: both said, no pin, text drawn (fires),
        # text as data, outside the facade, a measure step; the facade's test named by its basename and by
        # its path in the exit
        row('AN-04', [ok(1, what='Glyphs drawn from the face.', where='`crates/tinker-pdf/tests/glyphs.rs` (new)',
                         exit_='The page is fingerprinted on every target.'),
                      ok(2, what='Glyphs drawn from the face.', where='`crates/tinker-pdf/tests/glyphs.rs` (new)',
                         exit_='The page is fingerprinted; in the default and `bundled-fonts` builds.'),
                      ok(3, what='Glyphs drawn from the face.', where='`crates/tinker-pdf/tests/glyphs.rs` (new)',
                         exit_='The text extracts.'),
                      ok(4, what='Text drawn in its box.', where='`crates/tinker-pdf/tests/glyphs.rs` (new)',
                         exit_='The page is fingerprinted.'),
                      ok(5, what='The report as text.', where='`crates/tinker-pdf/tests/glyphs.rs` (new)',
                         exit_='The report is fingerprinted.'),
                      ok(6, what='Glyphs drawn.', where='`crates/tinker-pdf-font/tests/g.rs` (new)',
                         exit_='The page is fingerprinted.'),
                      ok(7, kind='measure', what='Glyphs drawn.', where='`crates/tinker-pdf/tests/glyphs.rs` (new)',
                         exit_='The page is fingerprinted.'),
                      ok(8, what='Glyphs drawn from the face.', exit_='The page is fingerprinted; `existing.rs` holds.'),
                      ok(9, what='Glyphs drawn from the face.',
                         exit_='The page is fingerprinted, in both builds; `existing.rs` holds.'),
                      ok(10, what='Glyphs drawn from the face.',
                         exit_='`crates/tinker-pdf/tests/existing.rs` fingerprints the page.')]),
        # deadlock: a whole-row wait on a row whose close names this one, or a range holding it; twins: a step,
        # another row's ID that starts like it
        row('AN-05', [ok(deps='AN-06')]),
        row('AN-06', [ok()], closes='AN-06.1 meets the exit, with AN-05 counted in it.'),
        row('AN-07', [ok(deps='AN-06.1')]),
        row('AN-17', [ok(deps='AN-18')]),
        row('AN-18', [ok()], closes='AN-18.1 meets the exit; AN-17a is another row.'),
        row('AN-19', [ok(deps='AN-20')]),
        row('AN-20', [ok()], closes='AN-20.1 meets the exit, and AN-18…AN-21 build on it.'),
        # deadlock through a step of another row, through a row's steps, through a split step; a close naming
        # a step of the row; an "X to Y" range; twins: a wait through the row's own earlier step, a transfer
        row('AN-42', [ok(1, deps='AN-43.1'), ok(2, deps='AN-42.1')]),
        row('AN-43', [ok(deps='AN-44')]),
        row('AN-44', [ok()], closes='AN-44.1 meets the exit, with AN-42 counted in it.'),
        row('AN-45', [ok(deps='AN-46')]),
        row('AN-46', [ok(deps='AN-47')]),
        row('AN-47', [ok()], closes="AN-47.1 meets the exit after AN-45's work."),
        row('AN-50', [ok(deps='AN-51')]),
        row('AN-51', [ok()], closes='AN-51.1 meets the exit after AN-50.1.'),
        row('AN-52', [ok(deps='K-10.2')]),
        row('AN-53', [ok()], closes='AN-53.1 meets the exit with AN-52.'),
        row('AN-60', [ok()], closes='AN-60.1 hands AN-61 to AN-63.'),
        row('AN-61', [ok()]), row('AN-62', [ok(deps='AN-60')]), row('AN-63', [ok()]),
        row('AN-64', [ok()], closes='AN-64.1 meets the exit; AN-65 to AN-67 build on it.'),
        row('AN-65', [ok()]), row('AN-66', [ok(deps='AN-64')]), row('AN-67', [ok()]),
        # tail-wait: a build step on a row whose tail waits on a person; twins: a watch step, a step, MS-11's
        # acts, an act reached another way; two tails bringing one act are both named; a measure step other
        # steps of its row wait on; a watch step a build step waits on is no tail
        row('AN-21', [ok(1), ok(2, kind='watch', deps='H-21')]),
        row('AN-22', [ok(deps='AN-21')]),
        row('AN-23', [ok(kind='watch', deps='AN-21')]),
        row('AN-24', [ok(deps='AN-21.1')]),
        row('AN-25', [ok(1), ok(2, kind='watch', deps='MS-11.1')]),
        row('AN-26', [ok(deps='AN-25')]),
        row('AN-28', [ok(1), ok(2, kind='watch', deps='H-21')]),
        row('AN-27', [ok(deps='AN-21, AN-28')]),
        row('AN-30', [ok(deps='H-21')]),
        row('AN-29', [ok(deps='AN-21, AN-30.1')]),
        row('AN-54', [ok(1, kind='measure', deps='AN-21'), ok(2, deps='AN-54.1')]),
        row('AN-55', [ok(1, kind='watch', deps='H-21'), ok(2, deps='AN-55.1')]),
        row('AN-56', [ok(deps='AN-55')]),
        row('AN-69', [ok(1), ok(2, kind='measure', deps='H-21')]),
        row('AN-70', [ok(deps='AN-69')]),
        row('AN-71', [ok(kind='measure', deps='AN-21')]),
        row('AN-72', [ok(1), ok(2, kind='watch', deps='H-10')]),
        row('AN-73', [ok(deps='AN-72')]),
        row('AN-74', [ok(deps='AN-21, H-21')]),
        # a standing duty waited on: no Blocks cell to name the row
        row('AN-68', [ok(deps='H-90')]),
        # tooling, and step-refs: a gone row's step, a step past its row's last, an H row H.md lacks; twins: landed
        # steps (below the first, a gap below the last), a split step, a step that exists; and a row's own text
        row('AN-08', [note(1, 'A change. *Note:* as steps/K-01.json had it.')]),
        row('AN-09', [note(1, 'A change. *Note:* after ZZ-01.5, AN-09.7, AN-10.7 and H-999; after AN-10.1, '
                              'AN-10.4, K-10.2 and MS-02.1.'), ok(2)]),
        row('AN-10', [ok(3), ok(5)]),
        row('AN-58', [ok()], today='steps/K-01.json names K-99.1 and H-998.'),
        row('AN-75', [ok()], item='**Row AN-75**: as `gone_file5.rs`, steps/K-02.json and K-98.1 say.'),
        row('AN-76', [ok()], exit_text='a test passes; `gone_file6.rs` and K-97.1 hold'),
        row('AN-77', [ok(deps='AN-01.1')], depends='AN-01, rows.json, `gone_file7.rs`, K-96.1'),
        # row-refs and row-pointer, in a row's own text; twins: its findings record the file as gone; a finding
        # that names the file without saying it is gone records nothing
        row('AN-11', [ok()], today='`gone_file.rs` and `other_fn` (`crates/tinker-pdf/src/lib.rs:3`) are read.'),
        row('AN-12', [ok()], today='`gone_file2.rs` is read.', findings=['gone_file2.rs is not in the tree.']),
        row('AN-57', [ok()], today='`gone_file4.rs` is read.', findings=['gone_file4.rs is read twice.']),
        row('AN-59', [ok()], today='`lib.rs:999` is read, and `other_fn` (`crates/tinker-pdf/src/lib.rs:3`) too.',
            findings=['lib.rs moved: its lines are gone.']),
    ],
    'SG': [
        # speed-kind: a speed row alone in a ci, a test and a fix step, named each way; twins: a measure step, a
        # speed row riding with code, design, doc and watch steps that name speed rows, and a speed row denied
        row('SG-01', [ok(1, kind='ci', what="MS-04's render group gains 'a page in four bands'.", where=BENCH,
                         deps='MS-04.5'),
                      ok(2, kind='test', what='Speed rows for the parse.', where='`docs/verification.md`'),
                      ok(3, kind='measure', what="MS-04's render group gains 'a page in two bands'.", where=BENCH,
                         deps='MS-04.5'),
                      ok(4, what='The parse, and its speed row.', where=W + ', ' + BENCH),
                      ok(5, kind='design', what='The speed rows the design gives.', where='`docs/design/existing.md`'),
                      ok(6, kind='fix', what="MS-04's set gains 'a parse'.", where=BENCH, deps='MS-04.8'),
                      ok(7, kind='doc', what='The speed rows listed.', where='`docs/verification.md`'),
                      ok(8, kind='watch', what='The speed rows read on the named machine.',
                         where='`docs/verification.md`'),
                      ok(9, what='Adds no speed row.', where='`docs/verification.md`')]),
    ],
}

H_MD = '\n'.join([
    '# Human verification and owner decisions', '',
    '## Reviews and manual verification', '',
    '| ID | What the person does | Why no machine can | Blocks |',
    '| --- | --- | --- | --- |',
    '| H-01 | A review | judgement | ED-01 |',
    '| H-02 | A review | judgement | ED-02 |',
    '| H-03 | A reading | judgement | ED-03 |',
    '| H-04 | A review | judgement | shipping ED-04 |',
    '| H-05 | A review | judgement | ED-05a/b |',
    '| H-06 | A review | judgement | ED-09.2 |',
    '| H-07 | A review | judgement | ED-09.1 |',
    '| H-11 | A review | judgement | ED-12 to ED-14 |',
    '| H-12 | A review | judgement | ED-15.1 to ED-15.3 |',
    '| H-13 | A review | judgement | ED-16. |',
    '| H-14 | A review | judgement | ED-99 and PDF/X-4 |',
    '| H-15 | A review | judgement | ED-18 |',
    "| H-16 | A review | judgement | ED-19's public claims |",
    '| H-17 | A review | judgement | shipping ED-20; ED-21 |',
    '| H-18 | A review | judgement | ED-22 |',
    '| H-19 | A review | judgement | ED-23 |',
    '| H-21 | A review | judgement | AN-21, AN-28, AN-30, AN-55, AN-69, AN-74 |',
    '| H-22 | A review | judgement | MS-11 |',
    '| H-24 | A review | judgement | ED-24—ED-26 |',
    '| H-25 | A review | judgement | ED-27 — 29 checks |',
    "| H-26 | A review | judgement | ED-32a/b's and ED-33a–c's claims |",
    "| H-27 | A review | judgement | ED-34, ED-35's claims |",
    "| H-28 | A review | judgement | ED-36…ED-37's shipped speed |",
    '| H-29 | A review | judgement | publishing ED-38 |',
    '| H-30 | A review | judgement | ED-39 |',
    '| H-31 | A review | judgement | ED-30, ED-31 |',
    '| H-32 | A review | judgement | ED-31 |',
    '| H-33 | A review | judgement | ED-30, ED-31 |',
    '| H-37 | A review | judgement | ED-41 |',
    '| H-38 | A review | judgement | K-10.2 |',
    '| H-39 | A review | judgement | AN-10.4 |', '',
    '## Decisions with a default the work already builds toward', '',
    '| ID | Decision | Why the owner | Blocks | Default until decided |',
    '| --- | --- | --- | --- | --- |',
    '| H-10 | A decision | policy | ED-06…08; AN-72 | the default |',
    '| H-20 | A decision | policy | ED-17 | none — until then |',
    '| H-40 | A decision | policy | ED-49 | not scheduled until ruled |', '',
    '## Standing duties', '',
    '| ID | Duty |',
    '| --- | --- |',
    '| H-90 | A duty |', '',
])
NG_MD = '# Hard limits\n\n**NG-01 — A limit.** Never.\n'

EXPECTED = {
    ('shape', 'K-01.1', 'kind'), ('shape', 'K-01.2', 'exit'), ('shape', 'K-02.1', 'order'),
    ('shape', 'K-03', 'table'), ('shape', 'K-03', 'steps'), ('shape', 'K-09.1', 'id'), ('shape', 'K-07.2', 'what'),
    ('shape', 'K-11.1', 'where'), ('shape', 'K-12', 'table'),
    ('deps', 'K-04.1', 'ZZ-99'), ('deps', 'K-04.1', 'H-999'), ('deps', 'K-04.2', 'K-04.3'), ('deps', 'K-05.1', 'cycle'),
    ('deps', 'K-08.1', 'K-08'), ('deps', 'ED-46.1', 'NG-99'),
    ('paths', 'P-01.1', 'crates/tinker-pdf/src/lib.rs'), ('paths', 'P-01.1', 'crates/nowhere/x.rs'),
    ('paths', 'P-01.2', 'crates/tinker-pdf/src/later.rs'),
    ('new-home', 'P-03.1', 'crates/tinker-pdf/src/deep/tables.rs'), ('new-home', 'P-03.4', 'crates/tinker-pdf/src/lex.rs'),
    ('new-home', 'P-03.5', 'crates/tinker-pdf/src/sib.rs'), ('new-home', 'P-03.7', 'crates/tinker-pdf/src/shallow.rs'),
    ('new-home', 'P-03.8', 'crates/tinker-pdf/src/fx_tests.rs'),
    ('new-home', 'P-03.9', 'crates/tinker-pdf/src/tables/mod.rs'),
    ('new-home', 'P-11.1', 'tools/tpdf/src/cmd/helper.rs'), ('new-home', 'P-11.2', 'xtask/src/count.rs'),
    ('new-home', 'P-11.4', 'crates/tinker-pdf/src/shaping.rs'),
    ('refs', 'P-04.1', 'exit: crates/tinker-pdf/src/lib.rs:999'), ('refs', 'P-04.1', 'exit: missing_file.rs'),
    ('refs', 'P-04.3', 'exit: design/missing.md'), ('refs', 'P-04.4', 'exit: crates/tinker-pdf/src/lib.rs:999'),
    ('refs', 'P-04.5', 'exit: dup_name.rs:99'), ('refs', 'P-04.8', 'exit: crates/tinker-pdf/src/lib.rs:5-3'),
    ('refs', 'P-04.9', 'exit: missing_twice.rs'), ('refs', 'P-04.10', 'exit: missing.ppm'),
    ('refs', 'P-04.11', 'exit: stale_only.txt'), ('refs', 'P-05.5', 'notes: dup_name.rs:5'),
    ('refs', 'P-06.6', 'exit: nowhere_tests.rs'),
    ('pointer', 'P-05.1', 'notes: other_fn@crates/tinker-pdf/src/lib.rs:3'),
    ('pointer', 'P-05.5', 'notes: e@dup_name.rs:5'),
    ('tests', 'P-06.1', 'exit: existing.rs a_missing_test'), ('tests', 'P-06.7', 'exit: existing.rs a_missing_test2'),
    ('tests', 'P-06.8', 'exit: existing.rs a_missing_test3'),
    ('idents', 'P-07.1', 'notes: NoSuchThing'), ('idents', 'P-07.7', 'notes: ObjOnlyName'),
    ('idents', 'P-07.7', 'notes: RoadmapOnlyName'), ('idents', 'P-07.8', 'notes: XRefNoSuch'),
    ('idents', 'P-07.8', 'notes: PdfNoSuchA'), ('idents', 'P-07.9', 'notes: GoneOldThing'),
    ('verbs', 'P-08.1', 'exit: nosuch'), ('verbs', 'P-08.4', 'exit: keep'),
    ('double-create', 'RC-01.1|RC-02.1', 'crates/tinker-pdf/src/dup.rs'),
    ('design-first', 'RC-07.1', 'docs/design/fresh.md'), ('design-first', 'P-12.1', 'docs/design/two.md'),
    ('new-home-first', 'RC-10.1', 'crates/tinker-pdf/src/m/x.rs'),
    ('new-home-first', 'RC-21.1', 'crates/planned-crate/src/z.rs'),
    ('two-homes', 'RC-13.1', 'crates/tinker-pdf/src/q/mod.rs'),
    ('no-crate', 'crates/brand-new-crate', 'Cargo.toml'), ('no-crate', 'crates/libonly-crate', 'Cargo.toml'),
    ('new-home-first', 'RC-23.1', 'crates/libonly-crate/src/w.rs'),
    ('edit-order', 'FT-01.1|FT-02.1', 'crates/tinker-pdf/src/edit.rs: shared_function'),
    ('edit-order', 'FT-08.1|FT-09.1', 'crates/tinker-pdf/src/lib.rs: existing_fn'),
    ('edit-order', 'FT-16.1|FT-18.1', 'crates/tinker-pdf/src/edit.rs: run'),
    ('edit-order', 'FT-20.1|FT-40.1', 'crates/tinker-pdf/src/edit2.rs: alpha_fn'),
    ('edit-order', 'FT-33.1|FT-34.1', 'crates/tinker-pdf/src/edit2.rs: xi_fn'),
    ('edit-order', 'FT-36.1|FT-37.1', 'crates/tinker-pdf/src/edit2.rs: pi_fn'),
    ('row-deps', 'FT-05', 'FT-06'), ('row-deps', 'ED-08', 'H-02'), ('row-deps', 'ED-30', 'H-32'),
    ('row-deps', 'ED-43', 'K-10.2b'), ('row-deps', 'ED-45', 'ED-99'),
    ('h-blocks', 'H-01', 'ED-01'), ('h-blocks', 'H-05', 'ED-05b'), ('h-blocks', 'H-06', 'ED-09.2'),
    ('h-blocks', 'H-10', 'ED-07'), ('h-blocks', 'H-10', 'ED-08'), ('h-blocks', 'H-10', 'ED-08a'),
    ('h-blocks', 'H-11', 'ED-13'), ('h-blocks', 'H-12', 'ED-15.2'), ('h-blocks', 'H-13', 'ED-16'),
    ('h-blocks', 'H-14', 'ED-99'), ('h-blocks', 'H-15', 'ED-18'), ('h-blocks', 'H-17', 'ED-21'),
    ('h-blocks', 'H-18', 'ED-22'), ('h-blocks', 'H-19', 'ED-23'), ('h-blocks', 'H-20', 'ED-17'),
    ('h-blocks', 'H-24', 'ED-25'), ('h-blocks', 'H-27', 'ED-34'), ('h-blocks', 'H-30', 'ED-39'),
    ('h-blocks', 'H-38', 'K-10.2b'), ('h-blocks', 'H-40', 'ED-49'),
    ('h-listed', 'H-02', 'K-04'),
    ('wiring', 'AN-01.1', 'MS-02'), ('wiring', 'AN-01.3', 'MS-02'), ('wiring', 'AN-02.1', 'MS-11'),
    ('wiring', 'AN-13.1', 'MS-11'), ('wiring', 'AN-15.1', 'MS-01 guards'), ('wiring', 'AN-15.3', 'MS-01 wasm leg'),
    ('wiring', 'AN-40.1', 'MS-04'), ('wiring', 'AN-41.1', 'MS-05'), ('wiring', 'AN-13.5', 'MS-11'),
    ('wiring', 'AN-13.6', 'MS-11'), ('wiring', 'AN-13.7', 'MS-11'), ('wiring', 'AN-13.8', 'MS-11'),
    ('fail-first', 'AN-03.1', 'what: cached'),
    ('both-builds', 'AN-04.1', 'exit: fingerprinted'), ('both-builds', 'AN-04.4', 'exit: fingerprinted'),
    ('both-builds', 'AN-04.8', 'exit: fingerprinted'), ('both-builds', 'AN-04.10', 'exit: fingerprints'),
    ('deadlock', 'AN-05.1', 'AN-06'), ('deadlock', 'AN-19.1', 'AN-20'), ('deadlock', 'AN-42.1', 'AN-44'),
    ('deadlock', 'AN-45.1', 'AN-47'), ('deadlock', 'AN-50.1', 'AN-51'), ('deadlock', 'AN-52.1', 'AN-53'),
    ('deadlock', 'AN-66.1', 'AN-64'),
    ('speed-kind', 'SG-01.1', 'ci'), ('speed-kind', 'SG-01.2', 'test'), ('speed-kind', 'SG-01.6', 'fix'),
    ('tail-wait', 'AN-22.1', 'AN-21'), ('tail-wait', 'AN-27.1', 'AN-21'), ('tail-wait', 'AN-27.1', 'AN-28'),
    ('tail-wait', 'AN-54.1', 'AN-21'), ('tail-wait', 'AN-70.1', 'AN-69'),
    ('tooling', 'AN-08.1', 'notes: steps/K'), ('tooling', 'AN-58', 'today: steps/K'),
    ('tooling', 'AN-75', 'item: steps/K'), ('tooling', 'AN-77', 'depends_text: rows.json'),
    ('step-refs', 'AN-09.1', 'notes: ZZ-01.5'), ('step-refs', 'AN-09.1', 'notes: AN-09.7'),
    ('step-refs', 'AN-09.1', 'notes: AN-10.7'), ('step-refs', 'AN-09.1', 'notes: H-999'),
    ('step-refs', 'AN-58', 'today: K-99.1'), ('step-refs', 'AN-58', 'today: H-998'),
    ('step-refs', 'AN-75', 'item: K-98.1'), ('step-refs', 'AN-76', 'exit: K-97.1'),
    ('step-refs', 'AN-77', 'depends_text: K-96.1'), ('row-deps', 'AN-77', 'K-96.1'),
    ('row-refs', 'AN-11', 'today: gone_file.rs'), ('row-refs', 'AN-57', 'today: gone_file4.rs'),
    ('row-refs', 'AN-75', 'item: gone_file5.rs'), ('row-refs', 'AN-76', 'exit: gone_file6.rs'),
    ('row-refs', 'AN-77', 'depends_text: gone_file7.rs'), ('refs', 'P-07.11', 'what: release_notes.md'),
    ('row-pointer', 'AN-11', 'today: other_fn@crates/tinker-pdf/src/lib.rs:3'),
}
STEP_LINE = re.compile(r'^\| ' + sweep.STEP_ID + r' \|', re.M)
# A heading that is no row ends the row before it: its text is no row's.
TIER_TAIL = {'K': '\n## Notes on the tier\n\n**Today.** A note about the tier, not a row.\n'}


def build(repo, skip_tier=None, index=None):
    for rel, text in TREE.items():
        path = os.path.join(repo, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'w', encoding='utf-8') as f:
            f.write(text)
    os.makedirs(os.path.join(repo, 'docs', 'roadmap'), exist_ok=True)
    counts = {}
    for tier in sweep.TIERS:
        body = '\n'.join(ROWS.get(tier, []))
        counts[tier] = (len(ROWS.get(tier, [])), len(STEP_LINE.findall(body)))
        if tier != skip_tier:
            with open(os.path.join(repo, 'docs', 'roadmap', f'{tier}.md'), 'w', encoding='utf-8') as f:
                f.write(f'# Tier {tier}\n\nThe fixture.\n\n{body}\n' + TIER_TAIL.get(tier, ''))
    counts.update(index or {})
    rows = ['| Prefix | Tier | File | Rows | Cut into steps | Steps |', '| --- | --- | --- | ---: | ---: | ---: |']
    rows += [f'| {t} | Tier {t} | [roadmap/{t}.md](roadmap/{t}.md) | {r} | {r} | {n} |' for t, (r, n) in counts.items()]
    rows += [f'| H | People | [roadmap/H.md](roadmap/H.md) | {len(re.findall(r"^[|] H-[0-9]+ [|]", H_MD, re.M))} | — | — |',
             '| NG | Limits | [roadmap/NG.md](roadmap/NG.md) | 1 | — | — |']
    with open(os.path.join(repo, 'docs', 'ROADMAP.md'), 'w', encoding='utf-8') as f:
        f.write('# Roadmap\n\n## Where the rows are\n\n' + '\n'.join(rows) + '\n')
    for name, text in (('H.md', H_MD), ('NG.md', NG_MD)):
        with open(os.path.join(repo, 'docs', 'roadmap', name), 'w', encoding='utf-8') as f:
            f.write(text)


def keys(mod, repo):
    """Every finding's key, each as often as the run reports it."""
    rm, findings, errors = mod.run(repo)
    assert not errors, errors
    return sorted(f.key() for f in findings)


def run_main(mod, argv):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        try:
            code = mod.main(argv)
        except Exception as exc:  # a broken rule may crash; that is an observation too
            code = f'crash: {type(exc).__name__}'
    return code, out.getvalue()


def check(name, cond, detail=''):
    print(f"{'ok  ' if cond else 'FAIL'} {name}" + (f': {detail}' if detail and not cond else ''))
    return cond


def edit(repo, rel, old, new):
    path = os.path.join(repo, rel)
    with open(path, encoding='utf-8') as f:
        text = f.read()
    assert old in text, (rel, old)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(text.replace(old, new, 1))


def append(repo, rel, text):
    with open(os.path.join(repo, rel), 'a', encoding='utf-8') as f:
        f.write(text)


def remove(repo, rel):
    os.remove(os.path.join(repo, rel))


def write_bytes(repo, rel, data):
    with open(os.path.join(repo, rel), 'wb') as f:
        f.write(data)


def to_folder(repo, rel):
    os.remove(os.path.join(repo, rel))
    os.makedirs(os.path.join(repo, rel))


def unreadable(repo, rel):
    os.chmod(os.path.join(repo, rel), 0)


K_LINE = '| K-08.1 | build | A change. | `crates/tinker-pdf/src/lib.rs` | A test passes. | K-08 |'
KC = '**Closes the row:** K-08.1 meets the exit.'
H_LINE = '| H-01 | A review | judgement | ED-01 |'
K, AN, H, NG, IDX = ('docs/roadmap/K.md', 'docs/roadmap/AN.md', 'docs/roadmap/H.md', 'docs/roadmap/NG.md',
                     'docs/ROADMAP.md')
# Each way the input cannot be read: the variant, what its message says, and the edit that makes it.
VARIANTS = [
    ('an en dash heading', 'cannot parse', lambda r: append(r, K, '\n## K-98 – A heading with an en dash\n')),
    ('a heading in bold', 'cannot parse', lambda r: append(r, K, '\n## **K-98** — bold\n')),
    ('a step line outside its table', 'outside its row',
     lambda r: append(r, K, '\n| K-03.1 | build | A change. | — | A test passes. | — |\n')),
    ('a step line without its closing pipe', 'cannot parse', lambda r: edit(r, K, K_LINE, K_LINE[:-2])),
    ('a step line written tight', 'cannot parse', lambda r: edit(r, K, K_LINE, '|' + K_LINE[2:])),
    ('a step line with seven cells', 'with 7 cells', lambda r: edit(r, K, K_LINE, K_LINE.replace('| build |', '| build | x |'))),
    ('a line in a step table that is not a step', 'not a step', lambda r: edit(r, K, K_LINE, K_LINE + '\n| a | b |')),
    ('a row twice', 'appears twice', lambda r: append(r, K, '\n' + row('K-08', [ok(1)]))),
    ('a step twice', 'appears twice', lambda r: append(r, K, '\n' + row('K-13', [ok(1), ok(1)]))),
    ('a label written another way', 'label the reader cannot parse',
     lambda r: edit(r, K, '**Closes the row:** K-01.1', '**Closes the row**: K-01.1')),
    ('a step line after its table', 'outside its row', lambda r: edit(r, K, KC, KC + '\n\n' + K_LINE.replace('K-08.1', 'K-08.2'))),
    ('a findings header written another way', 'label the reader cannot parse',
     lambda r: edit(r, K, KC, KC + '\n\n**The tree at 5297133:**\n\n- a finding')),
    ('a Depends on line of its own', 'label the reader cannot parse', lambda r: edit(r, K, KC, KC + '\n\n**Depends on:** K-01')),
    ('a Today written another way', 'label the reader cannot parse',
     lambda r: edit(r, K, '**Today.** Absent.', '**Today**: Absent.')),
    ('an Exit criterion written another way', 'label the reader cannot parse',
     lambda r: edit(r, K, '**Exit criterion.** a test passes', '**Exit criterion**: a test passes')),
    ('a Doc line written another way', 'label the reader cannot parse',
     lambda r: edit(r, K, '**Doc:** — · **Depends on:** —', '**Doc**: — · **Depends on:** —')),
    ('a note marked in lower case', 'note the reader cannot parse',
     lambda r: edit(r, AN, 'A change. *Note:* as steps/K-01.json', 'A change. *note:* as steps/K-01.json')),
    ('a Doc line without its Depends on', 'without its Depends on',
     lambda r: edit(r, K, '**Doc:** — · **Depends on:** —', '**Doc:** —')),
    ('a row without its Today', '"Today" lines', lambda r: edit(r, K, '**Today.** Absent.\n', '')),
    ('a note written another way', 'note the reader cannot parse',
     lambda r: edit(r, AN, 'A change. *Note:* as steps/K-01.json', 'A change. **Note:** as steps/K-01.json')),
    ('H.md missing', 'H.md: missing', lambda r: remove(r, H)),
    ('the index missing', 'ROADMAP.md: missing', lambda r: remove(r, IDX)),
    ('NG.md missing', 'NG.md: missing', lambda r: remove(r, NG)),
    ('an H row ID the reader does not know', 'H row the reader cannot parse',
     lambda r: append(r, H, '| H-12a | A review | judgement | ED-01 |\n')),
    ('an H row in bold', 'H row the reader cannot parse', lambda r: append(r, H, '| **H-23** | A review | judgement | ED-01 |\n')),
    ('an H row written tight', 'H row the reader cannot parse', lambda r: append(r, H, '|H-95 | A review | judgement | ED-01 |\n')),
    ('an H row under a header that is not ID', 'first column is not ID',
     lambda r: append(r, H, '\n| Row | What | Why | Blocks |\n| --- | --- | --- | --- |\n| H-91 | A review | judgement | ED-01 |\n')),
    ('an H row with no Blocks column', 'no Blocks column',
     lambda r: append(r, H, '\n| ID | What | Why | Gates |\n| --- | --- | --- | --- |\n| H-92 | A review | judgement | ED-01 |\n')),
    ('an H row after its table ended', 'first column is not ID', lambda r: append(r, H, '\n| H-94 | A review | judgement | ED-01 |\n')),
    ('an H row with a cell too many', 'and its table', lambda r: edit(r, H, H_LINE, H_LINE + '\n| H-93 | A review | judge | ment | ED-01 |')),
    ('an H row twice', 'appears twice', lambda r: edit(r, H, H_LINE, H_LINE + '\n' + H_LINE)),
    ('a limit twice', 'NG-01 appears twice', lambda r: append(r, NG, '\n**NG-01 — A limit.** Again.\n')),
    ('an index that miscounts the H rows', 'H rows, and the reader', lambda r: edit(r, IDX, '](roadmap/H.md) | ', '](roadmap/H.md) | 9')),
    ('a tier file not UTF-8', 'cannot be read (UnicodeDecodeError)', lambda r: write_bytes(r, 'docs/roadmap/OC.md', b'\xff\xfe\x00')),
    ('a tier file that is a folder', 'cannot be read (IsADirectoryError)', lambda r: to_folder(r, 'docs/roadmap/OC.md')),
    ('a file of the tree it cannot read', 'edit.rs: cannot be read', lambda r: unreadable(r, 'crates/tinker-pdf/src/edit.rs')),
    ('a folder of the tree it cannot read', 'cannot be read (PermissionError)', lambda r: unreadable(r, 'crates/b/src')),
]
# Variants built another way: by the index's counts, by a tier left out.
BUILT = [
    ('an index that does not count what the files hold', 'the index counts', {'index': {'K': (1, 1)}}),
    ('a tier file missing', 'OC.md: missing', {'skip_tier': 'OC'}),
]


def same_args(s):
    return [(x.split('::')[-2] if '::' in x else None, x.split('::')[-1]) for x in s.split()]


# What one rule reads, applied to a phrase: (rule, phrase, what it must give).
def _search(name):
    return lambda m, rm, s: bool(getattr(m, name).search(s))


VOCAB_FNS = {
    'MENTION_WORDS': _search('MENTION_WORDS'), 'CHANGE_WORDS': _search('CHANGE_WORDS'),
    'MENTION_OF': _search('MENTION_OF'), 'RUN_ON_DEVELOP': _search('RUN_ON_DEVELOP'), 'TOOLING': _search('TOOLING'),
    'NOTE_LIKE': lambda m, rm, s: bool(m.NOTE_LIKE.search(m.code_blanked(s))),
    'LABEL_LIKE': lambda m, rm, s: bool(m.LABEL_LIKE.match(s)),
    'SAME_OUTPUT': _search('SAME_OUTPUT'), 'FAIL_FIRST': _search('FAIL_FIRST'),
    'PINNED': _search('PINNED'), 'TEST_CLAIM': _search('TEST_CLAIM'), 'TEST_NEW': _search('TEST_NEW'),
    'OUTSIDE': _search('OUTSIDE'), 'NOT_OUTSIDE': _search('NOT_OUTSIDE'), 'SPEED_ROW': _search('SPEED_ROW'),
    'GONE': _search('GONE'), 'SETTLED': _search('SETTLED'), 'TRANSFER': _search('TRANSFER'),
    'TEXT_WORDS': _search('TEXT_WORDS'), 'TEXT_DRAWN': _search('TEXT_DRAWN'),
    'NO_DEFAULT': lambda m, rm, s: bool(m.NO_DEFAULT.match(s)),
    'SHIPPING': lambda m, rm, s: bool(m.SHIPPING.match(s)),
    'affirmed': lambda m, rm, s: m.affirmed(m.SPEED_ROW, s),
    'camel': lambda m, rm, s: m.camel(s),
    'twins': lambda m, rm, s: m.twins(*s.split()),
    'same': lambda m, rm, s: m.same_function(*same_args(s)),
    'instruments': lambda m, rm, s: tuple(sorted({(n, l) for n, p, r, l in m.INSTRUMENTS if p.search(s)})),
    'ids': lambda m, rm, s: tuple(sorted(m.ids_in(s, rm))),
    'blocked': lambda m, rm, s: tuple(sorted(m.blocked(s, rm))),
    'new': lambda m, rm, s: tuple(sorted(n for x in m.NEW_SPANS.finditer(s) for n in m.idents_in(x.group(1)))),
    'clauses': lambda m, rm, s: tuple(m.clauses(s)),
    'changed': lambda m, rm, s: tuple(sorted({n for _, n in m.changed_functions(s, {'fn_a', 'fn_b', 'run'})})),
}
VOCAB = [
    # a mention word: the function stays, is unchanged, untouched or unaffected, already, still or never does
    # something, or does it today (not "today's"), as before; "keeps" is none
    ('MENTION_WORDS', 'it stays', True), ('MENTION_WORDS', 'its output unchanged', True),
    ('MENTION_WORDS', 'it is untouched', True), ('MENTION_WORDS', 'it is unaffected', True),
    ('MENTION_WORDS', 'it already reads', True), ('MENTION_WORDS', 'as it is today', True),
    ('MENTION_WORDS', 'it still reads', True), ('MENTION_WORDS', 'it never reads', True),
    ('MENTION_WORDS', 'as before', True), ('MENTION_WORDS', "equal to today's product", False),
    ('MENTION_WORDS', 'it stayed', False), ('MENTION_WORDS', 'it keeps its bytes', False),
    ('MENTION_WORDS', 'it is kept', False), ('MENTION_WORDS', 'it changes', False),
    # a change word, which overrules a mention word
    ('CHANGE_WORDS', 'it changes', True), ('CHANGE_WORDS', 'it changed', True), ('CHANGE_WORDS', 'it gains', True),
    ('CHANGE_WORDS', 'it gained', True), ('CHANGE_WORDS', 'it adds', True), ('CHANGE_WORDS', 'it added', True),
    ('CHANGE_WORDS', 'it now reads', True), ('CHANGE_WORDS', 'rewritten', True), ('CHANGE_WORDS', 'it replaces', True),
    ('CHANGE_WORDS', 'replaced', True), ('CHANGE_WORDS', 'it extends', True), ('CHANGE_WORDS', 'extended', True),
    ('CHANGE_WORDS', 'it becomes', True), ('CHANGE_WORDS', 'it became', True), ('CHANGE_WORDS', 'it fixes', True),
    ('CHANGE_WORDS', 'fixed', True), ('CHANGE_WORDS', 'they route', True), ('CHANGE_WORDS', 'routed', True),
    ('CHANGE_WORDS', 'the change to it', False), ('CHANGE_WORDS', 'it stays', False),
    # a form that leaves the naming it covers be
    ('MENTION_OF', 'rather than an edit of `x`', True), ('MENTION_OF', 'it sits beside `x`', True),
    ('MENTION_OF', 'built on `x`', True), ('MENTION_OF', 'as for `x`', True), ('MENTION_OF', 'as `x` does', True),
    ('MENTION_OF', 'as `x` and `x` folds', True), ('MENTION_OF', 'as `x` and `x` read', False),
    ('MENTION_OF', 'what `x` draws', True), ("MENTION_OF", "what redaction's `x` answers", True),
    ('MENTION_OF', 'what `x` and `x` answer', True), ('MENTION_OF', 'it changes what `x` answers', False),
    ('MENTION_OF', 'they change what `x` answers', False), ('MENTION_OF', 'it rewrites what `x` answers', False),
    ('MENTION_OF', 'it alters what `x` answers', False), ('MENTION_OF', 'beside the rest', False),
    # a run of CI on develop
    ('RUN_ON_DEVELOP', 'a run on `develop`', True), ('RUN_ON_DEVELOP', 'it runs on `develop`', True),
    ('RUN_ON_DEVELOP', 'a job on `develop`', True), ('RUN_ON_DEVELOP', 'the jobs on `develop`', True),
    ('RUN_ON_DEVELOP', 'a leg on `develop`', True), ('RUN_ON_DEVELOP', 'the legs on `develop`', True),
    ('RUN_ON_DEVELOP', 'a dispatch on `develop`', True), ('RUN_ON_DEVELOP', 'dispatched on `develop`', True),
    ('RUN_ON_DEVELOP', 'a run on `main`', False), ('RUN_ON_DEVELOP', 'a run. On `develop`', False),
    # the cut's working files
    ('TOOLING', 'tiers/P.md', True), ('TOOLING', 'steps/K-01.json', True), ('TOOLING', 'rows.json', True),
    ('TOOLING', 'check_steps.py', True), ('TOOLING', 'render.py', True), ('TOOLING', 'the roadmap-cut folder', True),
    ('TOOLING', 'a scratchpad', True), ('TOOLING', 'batch_AN_1.json', True), ('TOOLING', 'docs/steps/x', False),
    # a note and a label written another way
    ('NOTE_LIKE', 'A change. **Note:** x', True), ('NOTE_LIKE', '*note:* x', True), ('NOTE_LIKE', '*Notes:* x', True),
    ('NOTE_LIKE', '*Note.* x', True), ('NOTE_LIKE', 'Note: x', True), ('NOTE_LIKE', 'a note about it', False),
    ('NOTE_LIKE', 'the `release_notes.md` reader', False),
    ('LABEL_LIKE', '**Today**: x', True), ('LABEL_LIKE', '**Exit criterion**: x', True),
    ('LABEL_LIKE', '**Closes the row**: x', True), ('LABEL_LIKE', '**Doc**: x', True),
    ('LABEL_LIKE', '**Depends on:** x', True), ('LABEL_LIKE', '**The tree at 5297133:**', True),
    ('LABEL_LIKE', '**Row K-01**: x', False),
    # what a change whose output must not move says, and what it does not
    ('SAME_OUTPUT', 'the parse cached', True), ('SAME_OUTPUT', 'once per page', True), ('SAME_OUTPUT', 'faster', True),
    ('SAME_OUTPUT', 'a speed-up', True), ('SAME_OUTPUT', 'quadratic', True), ('SAME_OUTPUT', 'without a copy', True),
    ('SAME_OUTPUT', 'zero-copy', True), ('SAME_OUTPUT', 'in place of `x`', True), ('SAME_OUTPUT', 'moved into `x`', True),
    ('SAME_OUTPUT', 'refactored', True), ('SAME_OUTPUT', 'memoized', True), ('SAME_OUTPUT', 'hoisted', True),
    ('SAME_OUTPUT', 'deduplicated', True), ('SAME_OUTPUT', 'streamed', True), ('SAME_OUTPUT', 'streaming', True),
    ('SAME_OUTPUT', 'cached values', False), ('SAME_OUTPUT', 'a speed row', False),
    # a guard that fails first
    ('FAIL_FIRST', 'it fails first', True), ('FAIL_FIRST', 'injected', True), ('FAIL_FIRST', 'it flips', True),
    ('FAIL_FIRST', 'refused', True), ('FAIL_FIRST', 'it panics', True), ('FAIL_FIRST', 'it fires', True),
    ('FAIL_FIRST', 'it counts', True), ('FAIL_FIRST', 'a band', True), ('FAIL_FIRST', 'it passes', False),
    # a pinned value
    ('PINNED', 'a fingerprint', True), ('PINNED', 'a hash', True), ('PINNED', 'a golden', True),
    ('PINNED', 'it pins', True), ('PINNED', 'byte-identical', True), ('PINNED', 'byte for byte', True),
    ('PINNED', 'byte-equal', True), ('PINNED', 'its sha-256', True), ('PINNED', 'a reftest', True),
    ('PINNED', 'the text', False),
    # a test claimed to exist, and a clause that claims nothing
    ('TEST_CLAIM', 'unchanged', True), ('TEST_CLAIM', 'keeps', True), ('TEST_CLAIM', 'stays', True),
    ('TEST_CLAIM', 'still', True), ('TEST_CLAIM', 'existing', True), ('TEST_CLAIM', 'already', True),
    ('TEST_CLAIM', 'passes', False),
    ('TEST_NEW', 'a new test', True), ('TEST_NEW', 'it flips', True), ('TEST_NEW', 'it fails first', True),
    ('TEST_NEW', 'failing first', True), ('TEST_NEW', 'renewed', False),
    # a default that is none
    ('NO_DEFAULT', 'none — until then', True), ('NO_DEFAULT', 'none - until then', True), ('NO_DEFAULT', 'none', True),
    ('NO_DEFAULT', 'not scheduled until ruled', True), ('NO_DEFAULT', 'the default', False),
    ('NO_DEFAULT', 'nonexistent rows', False),
    # what holds no row
    ('SHIPPING', 'shipping ED-20', True), ('SHIPPING', 'publishing', True), ('SHIPPING', ' Shipping it', True),
    ('SHIPPING', 'the shipping row', False),
    # outside the exit, and not
    ('OUTSIDE', 'outside this exit', True), ("OUTSIDE", "outside the row's exit", True),
    ('OUTSIDE', 'outside the exit', True), ('OUTSIDE', 'Outside this exit', True), ('OUTSIDE', 'outside the row', False),
    ('NOT_OUTSIDE', 'not outside', True), ("NOT_OUTSIDE", "isn't outside", True), ('NOT_OUTSIDE', 'is never outside', True),
    ('NOT_OUTSIDE', 'not wholly outside', True), ('NOT_OUTSIDE', 'Not outside', True),
    ('NOT_OUTSIDE', 'no step can do it, outside', False), ('NOT_OUTSIDE', 'nothing here is outside', False),
    # a speed row, and one denied
    ('SPEED_ROW', 'a speed row', True), ('SPEED_ROW', 'speed rows', True),
    ('SPEED_ROW', "MS-04's render group gains", True), ('SPEED_ROW', "MS-04's set gains", True),
    ('SPEED_ROW', 'a speed-up', False),
    ('affirmed', 'adds a speed row', True), ('affirmed', 'adds no speed row', False),
    ('affirmed', 'without a speed row', False), ('affirmed', 'with no new speed row', False),
    ('affirmed', 'never a speed row', False), ('affirmed', 'not a speed row', False),
    # a finding that records a file as gone
    ('GONE', 'it is not in the tree', True), ('GONE', 'no longer there', True), ('GONE', 'it is gone', True),
    ('GONE', 'removed', True), ('GONE', 'deleted', True), ('GONE', 'renamed', True), ('GONE', 'moved', True),
    ('GONE', 'absent', True), ('GONE', 'it is read', False),
    # a note that settles two edits
    ('SETTLED', 'whichever lands second', True), ('SETTLED', 'whichever lands later', True),
    ('SETTLED', 'whichever comes last', True), ('SETTLED', 'whichever is first', False),
    # a verb that passes one row to another
    ('TRANSFER', 'it hands ', True), ('TRANSFER', 'it passes ', True), ('TRANSFER', 'it gives ', True),
    ('TRANSFER', 'it moves ', True), ('TRANSFER', 'it sends ', True), ('TRANSFER', 'it leaves ', True),
    ('TRANSFER', 'it returns ', True), ('TRANSFER', 'it adds ', True), ('TRANSFER', 'it applies ', True),
    ('TRANSFER', 'it points ', True), ('TRANSFER', 'it ties ', True), ('TRANSFER', 'it links ', True),
    ('TRANSFER', 'it maps ', True), ('TRANSFER', 'it copies ', True), ('TRANSFER', 'it names ', False),
    ('TRANSFER', 'it handed ', True), ('TRANSFER', 'it moved ', True), ('TRANSFER', 'handing ', True),
    # text a test involves
    ('TEXT_WORDS', 'a font', True), ('TEXT_WORDS', 'glyphs', True), ('TEXT_WORDS', 'shaping', True),
    ('TEXT_WORDS', 'an EPUB', True), ('TEXT_WORDS', 'XHTML', True), ('TEXT_WORDS', 'Markdown', True),
    ('TEXT_WORDS', 'cmaps', True), ('TEXT_WORDS', 'ToUnicode', True), ('TEXT_WORDS', 'bidi', True),
    ('TEXT_WORDS', 'reading order', True), ('TEXT_WORDS', 'a typeface', True), ('TEXT_WORDS', 'a report', False),
    ('TEXT_DRAWN', 'drawn', True), ('TEXT_DRAWN', 'rendered', True), ('TEXT_DRAWN', 'laid out', True),
    ('TEXT_DRAWN', 'wrapped', True), ('TEXT_DRAWN', 'justified', True), ('TEXT_DRAWN', 'a text box', True),
    ('TEXT_DRAWN', 'its shadow', True), ('TEXT_DRAWN', 'measured', True), ('TEXT_DRAWN', 'read', False),
    ('TEXT_DRAWN', 'it drew', True), ('TEXT_DRAWN', 'painted', True), ('TEXT_DRAWN', 'it lays out', True),
    ('TEXT_DRAWN', 'the layout', True), ('TEXT_DRAWN', 'reflowed', True), ('TEXT_DRAWN', 'line breaks', True),
    ('TEXT_DRAWN', 'a decoration', True),
    # names
    ('camel', 'XRefNoSuch', True), ('camel', 'XRef', True), ('camel', 'PdfNoSuchA', True), ('camel', 'ExistingType', True),
    ('camel', 'IDs', False), ('camel', 'PDFs', False), ('camel', 'Page', False), ('camel', 'PDF', False),
    ('camel', 'A9B', False),
    ('twins', 'shape shaper', True), ('twins', 'box boxes', True), ('twins', 'read reading', True),
    ('twins', 'connect connection', True), ('twins', 'connect connections', True),
    ('twins', 'locate location', True), ('twins', 'locate locations', True), ('twins', 'abc abing', False),
    ('twins', 'lex lexer', True), ('twins', 'font fonts', True), ('twins', 'shape shaping', True),
    ('twins', 'encode encoding', True), ('twins', 'code codes', True), ('twins', 'pdfe pdfr', False),
    ('twins', 'base basis', False),
    ('same', 'Canvas::new_page Writer::new_page', False), ('same', 'Canvas::run Canvas::run', True),
    ('same', 'Page::run Canvas::run', False), ('same', 'run run', False), ('same', 'shared_function shared_function', True),
    ('same', 'longnamex longnamex', True), ('same', 'Canvas::new_page new_page', True),
    # the instruments and the least step each names
    ('instruments', "MS-02's paired job", (('MS-02', 6),)), ('instruments', "MS-04's open group", (('MS-04', 3),)),
    ('instruments', "MS-04's write group", (('MS-04', 4),)), ('instruments', "MS-04's render group", (('MS-04', 5),)),
    ('instruments', "MS-04's decode group", (('MS-04', 6),)), ('instruments', "MS-04's fonts group", (('MS-04', 7),)),
    ('instruments', "MS-04's set", (('MS-04', 8),)), ('instruments', "MS-04's window", (('MS-04', 9),)),
    ('instruments', "MS-04's 20-300 ms", (('MS-04', 9),)), ('instruments', "MS-04's 200 pages", ()),
    ('instruments', "MS-05's probe", (('MS-05', 1),)), ('instruments', "MS-05's bands", (('MS-05', 3),)),
    ('instruments', "MS-02's job", (('MS-02', 6),)), ('instruments', "MS-04's text group", (('MS-04', 3),)),
    ('instruments', "MS-04's calibration", (('MS-04', 3),)), ('instruments', "MS-04's merge", (('MS-04', 4),)),
    ('instruments', "MS-04's analysis group", (('MS-04', 4),)), ('instruments', "MS-04's encode group", (('MS-04', 6),)),
    ('instruments', "MS-04's sign group", (('MS-04', 7),)), ('instruments', "MS-04's layout group", (('MS-04', 7),)),
    ('instruments', "MS-04's shaping operations", (('MS-04', 7),)), ('instruments', "MS-04's groups", (('MS-04', 8),)),
    ('instruments', "MS-04's operations", (('MS-04', 8),)), ('instruments', "MS-04's benchmark", (('MS-04', 8),)),
    ('instruments', "MS-04's suite", (('MS-04', 8),)), ('instruments', "MS-04's CLI operations", (('MS-04', 8),)),
    ('instruments', "MS-05's test", (('MS-05', 1),)), ('instruments', "MS-05's case", (('MS-05', 1),)),
    ('instruments', "MS-05's open case", (('MS-05', 1),)), ('instruments', "MS-05's merge case", (('MS-05', 1),)),
    ('instruments', "MS-05's 'open 23 MB' case", (('MS-05', 1),)), ('instruments', "MS-05's memory test", (('MS-05', 1),)),
    ('instruments', "MS-05's peak", (('MS-05', 3),)), ('instruments', "MS-05's seven figures", (('MS-05', 3),)),
    ('instruments', 'Guard A1 flips from owed', (('MS-01 guards', 6),)),
    ('instruments', 'the `wasm32-wasip1` guard leg', (('MS-01 wasm leg', 8),)),
    # the IDs a cell names
    ('ids', 'ED-12…ED-14', ('ED-12', 'ED-13', 'ED-14')), ('ids', 'ED-12...ED-14', ('ED-12', 'ED-13', 'ED-14')),
    ('ids', 'ED-12–14', ('ED-12', 'ED-13', 'ED-14')), ('ids', 'ED-12—ED-14', ('ED-12', 'ED-13', 'ED-14')),
    ('ids', 'ED-12 — 14', ('ED-12',)), ('ids', 'ED-12 to ED-14', ('ED-12', 'ED-13', 'ED-14')),
    ('ids', 'hands ED-12 to ED-14', ('ED-12', 'ED-14')), ('ids', 'H-31 to H-33', ('H-31', 'H-32', 'H-33')),
    ('ids', 'H-31…H-33', ('H-31', 'H-32', 'H-33')), ('ids', 'ED-05a/b', ('ED-05a', 'ED-05b')),
    ('ids', 'ED-32a–b', ('ED-32a', 'ED-32b')), ('ids', 'ED-16.', ('ED-16',)),
    ('ids', 'ED-15.1 to ED-15.3', ('ED-15.1', 'ED-15.2', 'ED-15.3')),
    # what a Blocks cell holds
    ('blocked', 'shipping ED-20; ED-21', ('ED-21',)), ('blocked', "ED-34, ED-35's claims", ('ED-34',)),
    ('blocked', "ED-32a/b's and ED-33a–c's claims", ()), ('blocked', "ED-36…ED-37's shipped speed", ()),
    ('blocked', 'publishing ED-38', ()), ('blocked', 'Shipping ED-20', ()), ('blocked', "ED-19's public claims", ()),
    ('blocked', 'ED-20, ED-21', ('ED-20', 'ED-21')), ("blocked", "ED-36...ED-37's claims", ()),
    ("blocked", "ED-36—ED-37's claims", ()), ("blocked", "ED-32a/b's, ED-33a–c's claims", ()),
    ("blocked", "ED-32a/b's or ED-33a–c's claims", ()),
    # the names a text marks new
    ('new', '`ListA`, `ListB` and `ListC` (new) hold it', ('ListA', 'ListB', 'ListC')),
    ('new', '`FreshOne` (new) replaces `GoneOld`, which reads it', ('FreshOne',)),
    ('new', 'a `jpeg_min` (new) fixture', ('jpeg_min',)),
    # a text's clauses
    ('clauses', 'A (b: c). D.', ('A (b: c).', 'D.')), ('clauses', 'A: b; c.', ('A:', 'b;', 'c.')),
    ('clauses', 'A `x. y` z.', ('A `x. y` z.',)),
    # the functions a step changes, each read in its own segment
    ('changed', '`fn_a` changed.', ('fn_a',)), ('changed', '`fn_a` stays.', ()),
    ('changed', '`fn_a` routes it, where today it draws nothing.', ('fn_a',)),
    ('changed', '`fn_a` reports the space (the stated space kept and the other beside it).', ('fn_a',)),
    ('changed', "`fn_a` takes the form equal to today's product.", ('fn_a',)),
    ('changed', '`fn_a` and the `fn_a` fuzz target follow.', ('fn_a',)),
    ('changed', 'The `fn_a` fuzz target runs.', ()), ('changed', 'A `fn_a` seed joins.', ()),
    ('changed', '`fn_a`, regions and `fn_b` stay.', ()),
    ('changed', '`fn_a`, regions and `fn_b` keep the clamp.', ('fn_a', 'fn_b')),
    ('changed', '`fn_a` of the page, `fn_b` and the rest stay.', ()),
    ('changed', '`fn_a` changes so `fn_b` still holds.', ('fn_a',)),
    ("changed", "The record is the leaf's (`fn_a`, `edit.rs:2`).", ()),
    ("changed", "The record is the leaf's (`fn_a`, `:12`).", ()),
    ("changed", "The record is the leaf's (`fn_a`, `edit.rs`).", ('fn_a',)),
    ("changed", "The record is the leaf's (`fn_a`).", ('fn_a',)),
    ('changed', 'A walk (`fn_a`: the normal appearance) is unchanged.', ()),
    ('changed', 'What `fn_a` (to sRGB) answers is the rule.', ()),
    ('changed', '`fn_a` never applies it.', ()),
    ('changed', '`fn_a` changes and `fn_b` stays.', ('fn_a',)), ('changed', '`fn_a` changes but `fn_b` stays.', ('fn_a',)),
    ('changed', '`fn_a` changes while `fn_b` stays.', ('fn_a',)),
    ('changed', '`fn_a` keeps its signature and gains an option.', ('fn_a',)),
    ('changed', '`fn_a` keeps its signature.', ('fn_a',)), ('changed', '`fn_a` keeps each parsed space.', ('fn_a',)),
    ('changed', '`fn_a` now stays out of the loop.', ('fn_a',)),
    ('changed', 'This step changes what `fn_a` answers.', ('fn_a',)),
    ('changed', '`fn_a` is rewritten as `fn_b` already is.', ('fn_a',)),
    ('changed', '`fn_a` gains what `fn_b` lacks.', ('fn_a',)),
    ('changed', 'A buffer stays inked (`fn_a`), and more.', ('fn_a',)),
    ('changed', 'Already the `fn_a` path reads it.', ('fn_a',)),
    ('changed', 'The `fn_a` path is untouched by the change to its caller `fn_b`.', ('fn_b',)),
    ('changed', 'These stay unchanged: `fn_a` and `fn_b`.', ()),
    ('changed', 'These change: `fn_a` and `fn_b`.', ('fn_a', 'fn_b')),
    ('changed', 'These stay, and one changes: `fn_a`.', ('fn_a',)),
    ('changed', 'They stay: `fn_a` changes now.', ('fn_a',)),
    ('changed', '`fn_b` stays: `fn_a` changes.', ('fn_a',)),
    ('changed', '`fn_b` stays: `fn_a` reads it.', ('fn_a',)),
    ('changed', 'These stay. `fn_a` reads it.', ('fn_a',)),
    ('changed', '`fn_a` reads it so the page stays.', ('fn_a',)),
    ('changed', 'It reads them after K-02.2, which changes what `fn_a` answers.', ()),
    ('changed', 'It reads them, which changes what `fn_a` answers.', ('fn_a',)),
    ('changed', '`fn_a` changes (`edit.rs:2`).', ('fn_a',)),
    ('changed', '`fn_a` and `fn_b`, regions and the rest stay.', ()),
    ('changed', '`fn_a` or `fn_b`, regions and the rest stay.', ()),
    ('changed', 'the `fn_a`, regions and the rest stay.', ()), ('changed', 'a `fn_a`, regions and the rest stay.', ()),
    ('changed', 'an `fn_a`, regions and the rest stay.', ()),
    ('changed', '`Page::run` changed.', ('run',)),
]
# What a rule must say at once, read from the module: a name's instrument patterns rise in least step.
def ordered_instruments(m):
    last = {}
    for name, _, _, least in m.INSTRUMENTS:
        if least < last.get(name, 0):
            return False
        last[name] = least
    return True


def vocab(mod, rm):
    out = []
    for kind, sample, _ in VOCAB:
        try:
            out.append(VOCAB_FNS[kind](mod, rm, sample))
        except Exception as exc:
            out.append(f'crash: {type(exc).__name__}')
    return tuple(out)


class Fixtures:
    """The fixture and its variants, each built once."""

    def __init__(self, root):
        self.main = os.path.join(root, 'main')
        build(self.main)
        self.acc = os.path.join(root, 'accepted.tsv')
        every = [f'{c}\t{s}\t{i}\tthe fixture\n' for c, s, i in sorted(EXPECTED)]
        self.every = every
        self.acc_files = {}
        for name, lines in (('every', every), ('one short', every[1:]),
                            ('stale', every + ['shape\tK-99.1\tkind\tnothing matches this\n']),
                            ('a line of two fields', every + ['shape\tK-01.1\n']),
                            ('a field empty', every + ['shape\t\tkind\ta reason\n']),
                            ('twice', every + [every[0]]),
                            ('no such check', every + ['no-such-check\tK-01.1\tkind\ta reason\n'])):
            path = os.path.join(root, f'acc_{len(self.acc_files)}.tsv')
            with open(path, 'w', encoding='utf-8') as f:
                f.write('# fixture\n' + ''.join(lines))
            self.acc_files[name] = path
        self.acc = self.acc_files['every']
        self.acc_bad = os.path.join(root, 'acc_bad.tsv')
        with open(self.acc_bad, 'wb') as f:
            f.write(b'\xff\xfe\x00')
        self.variants = {}
        self.fragments = {}
        for name, fragment, change in VARIANTS:
            path = os.path.join(root, 'v_' + re.sub(r'\W+', '_', name))
            build(path)
            change(path)
            self.variants[name], self.fragments[name] = path, fragment
        for name, fragment, kwargs in BUILT:
            path = os.path.join(root, 'v_' + re.sub(r'\W+', '_', name))
            build(path, **kwargs)
            self.variants[name], self.fragments[name] = path, fragment
        path = os.path.join(root, 'v_dispatch')
        build(path)
        with open(os.path.join(path, 'tools/tpdf/src/main.rs'), 'w', encoding='utf-8') as f:
            f.write('fn main() {}\n')
        self.dispatch = path

    def signature(self, mod):
        """What a module shows over every fixture: findings, exit codes, messages, reports, the table."""
        sig = {}
        try:
            sig['findings'] = tuple(keys(mod, self.main))
        except Exception as exc:
            sig['findings'] = f'crash: {type(exc).__name__}'
        for name, path in list(self.variants.items()) + [('dispatch', self.dispatch)]:
            code, out = run_main(mod, ['--repo', path, '--accepted', self.acc])
            sig[name] = (code, frozenset(l for l in out.split('\n') if l.startswith(('FINDING', 'cannot read'))))
        for name, path in self.acc_files.items():
            sig['acc ' + name] = run_main(mod, ['--repo', self.main, '--accepted', path])
        sig['acc stale under --only'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc_files['stale'],
                                                       '--only', 'deps'])
        sig['acc unreadable'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc_bad])
        sig['acc missing'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc_bad + '.none'])
        sig['only double-create'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc, '--only', 'double-create'])
        sig['only'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc, '--only', 'two-homes'])
        sig['only spaced'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc, '--only', ' two-homes '])
        sig['only no check'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc, '--only', 'no-such-check'])
        sig['quiet'] = run_main(mod, ['--repo', self.main, '--accepted', self.acc_files['one short'], '--quiet'])
        sig['waits'] = run_main(mod, ['--repo', self.main, '--waits'])
        try:
            sig['vocab'] = vocab(mod, mod.Roadmap(self.main))
        except Exception as exc:
            sig['vocab'] = f'crash: {type(exc).__name__}'
        sig['instruments ordered'] = ordered_instruments(mod)
        return sig


# Each rule put back broken: what the rule is, then pairs of source text and its broken form.
INJECT = [
    # the reader
    ('a missing file is a read error', "except FileNotFoundError:\n            self.errors.append(f'{path}: missing')",
     "except FileNotFoundError:\n            pass"),
    ('a file it cannot read is a read error', "except (OSError, UnicodeDecodeError) as exc:\n            self.errors",
     "except ZeroDivisionError as exc:\n            self.errors"),
    ('a heading the reader cannot parse', 'elif HEAD_LIKE.match(line):', 'elif False:'),
    ('a heading in bold or brackets', "HEAD_LIKE = re.compile(r'^#+\\s*[\\[*_`]*\\s*' + ROW_ID",
     "HEAD_LIKE = re.compile(r'^#+\\s*' + ROW_ID"),
    ('a step line outside its table', 'if row is None or not in_table:', 'if False:'),
    ('a step line written another way', 'elif not STEP_LINE.match(line):', 'elif False:'),
    ('a step line with the wrong count of cells', 'elif len(split_cells(line)) != 6:', 'elif False:'),
    ('a step twice', "if s.id in self.steps:\n                        self.errors.append",
     "if False:\n                        self.errors.append"),
    ('a row twice', 'if row.id in self.rows:', 'if False:'),
    ('a note written another way', 'if NOTE_LIKE.search(code_blanked(s.what)) or NOTE_LIKE.search(code_blanked(s.notes)):',
     'if False:'),
    ('a line in a step table that is not a step',
     "if line.startswith('|'):\n                    self.errors.append(f'{where}: a line in a step table",
     "if False:\n                    self.errors.append(f'{where}: a line in a step table"),
    ('a Doc line without its Depends on', 'if not sep:', 'if False:'),
    ('a label written another way', 'elif LABEL_LIKE.match(line):', 'elif False:'),
    ('each row has its four labels', 'if row.labels[attr] != 1:', 'if False:'),
    ('an H row ID not H-<n>', ' or not H_RE.match(cells[0])', ''),
    ('an H row written another way',
     "cells = split_cells(line) if line.startswith('| H-') and line.rstrip().endswith('|') else []",
     'cells = split_cells(line)'),
    ('an H row under a header not ID', "elif header is None or header[0] != 'ID':", 'elif False:'),
    ('an H row with no Blocks column', "elif 'Blocks' not in header and 'Duty' not in header:", 'elif False:'),
    ('a table of duties needs no Blocks column', " and 'Duty' not in header", ''),
    ('an H row with the wrong count of cells', 'elif len(cells) != len(header):', 'elif False:'),
    ('an H row twice', 'elif cells[0] in self.h:', 'elif False:'),
    ('a table header is the line above its rule', "header = split_cells(prev) if prev.startswith('|') else None",
     'header = None'),
    ('a table ends at a line that is not one', "if not line.startswith('|'):\n                header = None",
     "if False:\n                header = None"),
    ('a limit twice', 'for x in sorted({x for x in ids if ids.count(x) > 1}):', 'for x in []:'),
    ('the index counts the H rows and the limits', 'if counts.get(name) != have:', 'if False:'),
    ("the index's counts", 'if self.index.get(tier) != have:', 'if False:'),
    ("a file of the tree it cannot read", "self.errors.append(f'{rel}: cannot be read ({type(exc).__name__})')", 'pass'),
    ('a folder of the tree it cannot read', 'if not isinstance(exc, FileNotFoundError):', 'if False:'),
    ('a tree read badly is no result', 'if tree.errors:', 'if False:'),
    ('the acceptance file unreadable', "except (OSError, UnicodeDecodeError) as exc:\n        return None,",
     "except ZeroDivisionError as exc:\n        return None,"),
    ('what .gitignore names is not the tree', " and f'{rel}/{x}' not in SKIP_TREES", ''),
    ('each finding once', 'if f.key() not in seen:', 'if True:'),
    ('--only read without spaces', "only = set(x.strip() for x in args.only.split(',') if x.strip())",
     "only = set(x for x in args.only.split(',') if x)"),
    ('--quiet prints only what is unexplained', 'if not args.quiet:', 'if True:'),
    # the acceptance file
    ('an acceptance of four fields', 'len(parts) != 4 or ', ''),
    ('an acceptance with no field empty', ' or not all(p.strip() for p in parts)', ''),
    ('an acceptance of a check that exists', 'if parts[0] not in known:', 'if False:'),
    ('a finding accepted once', "if key in accepted:\n            problems.append", "if False:\n            problems.append"),
    ('an acceptance must match a finding', 'if key not in used and (not only or key[0] in shown):', 'if False:'),
    ('a stale line outside --only is not stale there', ' and (not only or key[0] in shown)', ''),
    # the IDs a cell names
    ('an ID before a full stop is read', r"(?![\w-]|\.\d)')", r"(?![\w.-]|\.\d)')"),
    ("'X to Y' ranges", 'for m in TO_RANGE.finditer(text):', 'for m in []:'),
    ("'X to Y' over steps",
     "found.update(x.id for x in rm.rows[sa.group('row')].steps if step_key(a) <= step_key(x.id) <= step_key(b))",
     'pass'),
    ('a transfer is no range', 'if TRANSFER.search(text[:m.start()]):\n            continue',
     'if False:\n            continue'),
    ('ranges with a dash or an ellipsis', 'for m in RANGE.finditer(text):', 'for m in []:'),
    ('a range by an em dash', '\\s*|—)(?:([A-Z]{1,2})-)?', '\\s*)(?:([A-Z]{1,2})-)?'),
    ('a spaced em dash is no range', '|—)(?:([A-Z]{1,2})-)?', '|\\s*—\\s*)(?:([A-Z]{1,2})-)?'),
    ('a range of H rows', 'universe = list(rm.rows) + list(rm.h)', 'universe = list(rm.rows)'),
    ('a/b read as both rows', "found.update(m.group(1) + x for x in [m.group(2)] + m.group(3).split('/')[1:])", 'pass'),
    ("a split step's parts",
     "return [x.id for x in rm.rows[m.group('row')].steps if re.fullmatch(re.escape(dep) + r'[a-z]', x.id)]",
     'return []'),
    ('a gap below the last step has landed', 'step_key(dep) < max(step_key(s.id) for s in row.steps)',
     'step_key(dep) < min(step_key(s.id) for s in row.steps)'),
    ('a whole-row wait waits on its steps', 'out.extend(x.id for x in rm.rows[d].steps)', 'pass'),
    # H rows
    ('a "none" default is none', "row['has default'] = default is not None and not NO_DEFAULT.match(default)",
     "row['has default'] = default is not None"),
    ('only a Default column gives a default',
     "row['has default'] = default is not None and not NO_DEFAULT.match(default)", "row['has default'] = True"),
    ('a release holds no row', 'if SHIPPING.match(clause):\n            continue', 'if False:\n            continue'),
    ('a claim holds no row', 'ids -= ids_in(m.group(1), rm)', 'pass'),
    ('a Blocks cell read clause by clause', "for clause in cell.split(';'):", 'for clause in [cell]:'),
    ('every possessor of the claims', "(?:,? and |, | or ))*' + POSSESSOR", "(?:,? and |, | or )){0}' + POSSESSOR"),
    ('a possessor that is a range',
     "(?:(?:\\s*(?:…|–|\\.\\.\\.)\\s*|—)(?:(?:[A-Z]{1,2}-)?\\d+[a-z]?|[a-z])|(?:/[a-z])+| to '",
     "(?:(?:/[a-z])+| to '"),
    ('a possessor slashed', "|[a-z])|(?:/[a-z])+| to '", "|[a-z])| to '"),
    ('a claim with an adjective', " (?:\\w+ )?(?:claims?|shipped)\\b')", " (?:claims?|shipped)\\b')"),
    ('outside the exit, said', 'OUTSIDE = re.compile(r"\\boutside (?:this|the row\'s|the) exit\\b", re.I)',
     'OUTSIDE = re.compile(r"(?!)")'),
    ('"Outside" with a capital', 'the) exit\\b", re.I)', 'the) exit\\b")'),
    ('a negated outside is not outside', ' and not NOT_OUTSIDE.search(c)', ''),
    ("not, never and n't", "(?:\\bnot|\\bnever|n['’]t)\\s+", "(?:\\bnot)\\s+"),
    ('a clause of the closing line', "for c in re.split(r'(?<=[.;])\\s+', row.closes))", 'for c in [row.closes])'),
    ('a default named carries', "if rm.h.get(hid, {}).get('has default'):", 'if False:'),
    ('a Blocks cell naming what the roadmap lacks', "if ref.split('-')[0] in TIERS and kind in ('unknown', 'gone'):",
     'if False:'),
    ('only a tier prefix is a row', "if ref.split('-')[0] in TIERS and kind in ('unknown', 'gone'):",
     "if kind in ('unknown', 'gone'):"),
    ('a split step in Blocks is its parts',
     "for t in split_parts(rm, ref) if kind == 'split' else [ref]:\n                if t in rm.steps:",
     "for t in [ref]:\n                if t in rm.steps:"),
    ('an H row blocking a step', 'if not reaches(rm, ctx, t, hid) and hid not in rm.steps[t].depends:', 'if False:'),
    ("a step's H row names its row", "if s.row not in {x.split('.')[0] for x in named}:", 'if False:'),
    ('a duty blocks nothing', "if not cells or 'Blocks' not in cells:", 'if not cells:'),
    # a row's Depends on
    ("a landed step is done in a row's Depends on", "if kind == 'landed':\n                continue",
     "if False:\n                continue"),
    ("a row's Depends on naming what the roadmap lacks", "if ref.split('-')[0] in TIERS + ['H']:", 'if False:'),
    ('only a tier or H prefix is an ID there', "if ref.split('-')[0] in TIERS + ['H']:", 'if True:'),
    ("a split step in a row's Depends on is its parts",
     "for t in split_parts(rm, ref) if kind == 'split' else [ref]:\n                if H_RE.match(t):",
     "for t in [ref]:\n                if H_RE.match(t):"),
    ("a limit in a row's Depends on", 'if ref == r.id or NG_RE.match(ref):', 'if ref == r.id:'),
    # the reader's texts
    ('a heading the reader cannot parse is no row', 'row = None\n                in_table = in_findings = False',
     'in_table = in_findings = False'),
    # new-home and its kin
    ("a crate's src/lib.rs is a home", "f'{d}.rs', f'{d}/lib.rs'):", "f'{d}.rs'):"),
    ('a crate no step creates', "if f'{crate}/Cargo.toml' not in ctx['new_paths']:", 'if False:'),
    ('two homes for one module', "if p.endswith('/mod.rs') and home == p[:-len('/mod.rs')] + '.rs':", 'if False:'),
    ("a row's own module file", ' or all(o.row == s.row for o in first)', ''),
    ('a shared test file', " and '/tests/' not in '/' + path", ''),
    ('a shared folder', "if len(rows) > 1 and '.' in base and", 'if len(rows) > 1 and'),
    ('one row creating twice', 'rows = sorted({s.row for s in first})', 'rows = sorted({s.id for s in first})'),
    ('every step that could write a design doc first', "if path.startswith('docs/design/'):\n            for s in first:",
     "if path.startswith('docs/design/') and len(first) == 1:\n            for s in first:"),
    ('a sibling is the layout',
     "\n                        and (d.startswith(f.rsplit('/', 1)[0] + '/') or f.startswith(d + '/'))", ''),
    ('a step naming the home', ' and f not in named', ''),
    ('twin stems', "if any(y == x + s for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')):", 'if False:'),
    ('a final e drops before a vowel', "if x.endswith('e') and any(", 'if False and any('),
    ('not before a consonant', "x[:-1] + s for s in ('ing', 'ion', 'ions')",
     "x[:-1] + s for s in ('r', 's', 'ing', 'ion', 'ions')"),
    ('a final e drops only from a stem that has one', "if x.endswith('e') and any(", 'if any('),
    ('twins by -er', "for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')", "for s in ('r', 's', 'es', 'ing', 'ion', 'ions')"),
    ('twins by -r', "for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')", "for s in ('er', 's', 'es', 'ing', 'ion', 'ions')"),
    ('twins by -s', "for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')", "for s in ('er', 'r', 'es', 'ing', 'ion', 'ions')"),
    ('twins by -es', "for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')", "for s in ('er', 'r', 's', 'ing', 'ion', 'ions')"),
    ('twins by -ing', "for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')", "for s in ('er', 'r', 's', 'es', 'ion', 'ions')"),
    ('twins by -ion', "for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')", "for s in ('er', 'r', 's', 'es', 'ing', 'ions')"),
    ('twins by -ions', "for s in ('er', 'r', 's', 'es', 'ing', 'ion', 'ions')", "for s in ('er', 'r', 's', 'es', 'ing', 'ion')"),
    ('twins by -ing after a dropped e', "x[:-1] + s for s in ('ing', 'ion', 'ions')", "x[:-1] + s for s in ('ion', 'ions')"),
    ('twins by -ion after a dropped e', "x[:-1] + s for s in ('ing', 'ion', 'ions')", "x[:-1] + s for s in ('ing', 'ions')"),
    ('twins by -ions after a dropped e', "x[:-1] + s for s in ('ing', 'ion', 'ions')", "x[:-1] + s for s in ('ing', 'ion')"),
    ("a crate's root by its tree", "root = '/'.join(path.split('/')[:CRATE_ROOTS[top]])",
     "root = '/'.join(path.split('/')[:3])"),
    ('every tree of the code has homes', "if top in CRATE_ROOTS and '/' in path", "if top == 'crates' and '/' in path"),
    ('only the creator placed it', 'for s in creators(rm, ctx, path)[0]:', 'for s in creators(rm, ctx, path)[1]:'),
    ('a family already split', 'if len(family) >= 2:', 'if False:'),
    ('a mod.rs beside x.rs', "if base == 'mod.rs' and d and tree.exists(d + '.rs'):", 'if False:'),
    ('a module already a folder',
     "if base.endswith('.rs') and base != 'mod.rs' and tree.exists(f'{d}/{stem}/mod.rs'):", 'if False:'),
    ('a basename every crate repeats', ' and base not in COMMON_BASENAMES', ''),
    # names
    ('an exit names its tests', "if field == 'exit' and SNAKE.match(name) and '::' not in span:", 'if False:'),
    ('a dotted name', "if '.' in last and all(WORD_RE.fullmatch(x) for x in last.split('.')):", 'if False:'),
    ('a name marked new', 'for m in NEW_SPANS.finditer(value):', 'for m in []:'),
    ('a list marked new', "NEW_SPANS = re.compile(r'((?:`[^`]+`(?:,? and |, | or ))*`[^`]+`)",
     "NEW_SPANS = re.compile(r'(`[^`]+`)"),
    ('a long snake name is a test', "if SNAKE.match(last) and last.count('_') >= 4:", 'if False:'),
    ('a camel hump of capitals', '|^[A-Z]{2,}[a-z]{2,}', ''),
    ('a camel hump after a lower-case letter', "r'[a-z0-9][A-Z]|", "r'"),
    ('a word of capitals is no camel', ' and any(c.islower() for c in word)', ''),
    ('build output is not the code', "SKIP_DIRS = ('target', 'node_modules', '.git', 'obj', '__pycache__')",
     "SKIP_DIRS = ('target', 'node_modules', '.git', '__pycache__')", "'/obj/', ", ''),
    ('this folder is not the code', ", '/tools/roadmap/')", ')'),
    ('an option word is no verb', '(?:run|check|probe|writing::print)\\(', '(?:run|check|probe|writing::)'),
    ('a dispatch the reader no longer matches', 'if not have:', 'if False:'),
    ('a note read with code spans blanked', 'NOTE_LIKE.search(code_blanked(s.what))', 'NOTE_LIKE.search(s.what)'),
    ('a missing acceptance file', "return None, [f'the acceptance file: {path}: missing']", 'return accepted, problems'),
    ('a measure step as a tail', "if rm.steps[x].kind in ('watch', 'measure')}", "if rm.steps[x].kind in ('watch',)}"),
    ('a measure step in its own tail', "(s.kind in ('watch', 'measure') and s.id in tail_of(s.row))",
     "(s.kind in ('watch',) and s.id in tail_of(s.row))"),
    ('an H row with a default is no act', "if h in rm.h and not rm.h[h]['has default']}", 'if h in rm.h}'),
    ('an act the step waits on itself', "return {e} if not rm.h[e]['has default'] else set()", 'return set()'),
    ('a citation holds only the naming inside it', 'if any(a <= m.start() < b and cites(clause[a:b]) for a, b in parens):',
     'if any(cites(clause[a:b]) for a, b in parens):'),
    ('a fuzz.yml run waits on MS-11', "if w not in ('ci.yml', 'fuzz.yml', 'corpus.yml')]", "if w not in ('ci.yml', 'corpus.yml')]"),
    ('a corpus.yml run waits on MS-11', "if w not in ('ci.yml', 'fuzz.yml', 'corpus.yml')]", "if w not in ('ci.yml', 'fuzz.yml')]"),
    ('a watch step reads runs', "if s.kind in ('watch', 'ci', 'test', 'measure') and s.row != 'MS-11'",
     "if s.kind in ('ci', 'test', 'measure') and s.row != 'MS-11'"),
    ('a ci step reads runs', "if s.kind in ('watch', 'ci', 'test', 'measure') and s.row != 'MS-11'",
     "if s.kind in ('watch', 'test', 'measure') and s.row != 'MS-11'"),
    ('a test step reads runs', "if s.kind in ('watch', 'ci', 'test', 'measure') and s.row != 'MS-11'",
     "if s.kind in ('watch', 'ci', 'measure') and s.row != 'MS-11'"),
    ('a measure step reads runs', "if s.kind in ('watch', 'ci', 'test', 'measure') and s.row != 'MS-11'",
     "if s.kind in ('watch', 'ci', 'test') and s.row != 'MS-11'"),
    ("a row's own fields are read", "for name in ('item', 'today', 'exit', 'depends_text'):", "for name in ('today',):"),
    ('--only by the check that emits', "EMITS = {'double-create': {'double-create', ", "EMITS = {'double-create': {"),
    ('possessors listed with commas', "(?:,? and |, | or ))*' + POSSESSOR", "(?:,? and | or ))*' + POSSESSOR"),
    ('possessors listed with "or"', "(?:,? and |, | or ))*' + POSSESSOR", "(?:,? and |, ))*' + POSSESSOR"),
    ('a possessor range by three dots', "ID_SPAN = ROW_ID + r'(?:(?:\\s*(?:…|–|\\.\\.\\.)\\s*|—)",
     "ID_SPAN = ROW_ID + r'(?:(?:\\s*(?:…|–)\\s*|—)"),
    ('a possessor range by an em dash', "ID_SPAN = ROW_ID + r'(?:(?:\\s*(?:…|–|\\.\\.\\.)\\s*|—)",
     "ID_SPAN = ROW_ID + r'(?:(?:\\s*(?:…|–|\\.\\.\\.)\\s*)"),
    ('a transfer by any ending', "(?:s|es|d|ed|ing)?\\s+$'", "(?:s|es)?\\s+$'"),
    # tests
    ('a clause saying new', "TEST_NEW = re.compile(r'\\bnew\\b|", "TEST_NEW = re.compile(r'\\(new\\b|"),
    ('a test a What builds', "if test in ctx['introduced']:", 'if False:'),
    ('a sentence ends the clause after the test',
     "ends = [i for i in (text.find(';', m.end()), text.find('. ', m.end())) if i >= 0]",
     "ends = [i for i in (text.find(';', m.end()),) if i >= 0]"),
    ('a sentence ends the clause before the test',
     "start = max(text.rfind(';', 0, m.start()), text.rfind('. ', 0, m.start())) + 1",
     "start = text.rfind(';', 0, m.start()) + 1"),
    ('a test claimed in a file the tree has', 'hits = tree.resolve(file)\n            if not hits:\n                continue',
     'hits = tree.resolve(file)'),
    # refs and pointers
    ('a partial path resolves by its end', "return [f for f in self.files if f.endswith('/' + ref)]", 'return []'),
    ('a bare :N', 'b = BARE_POINTER.match(span)', 'b = None'),
    ('a name two files carry', 'elif all(len(tree.lines(h)) < hi for h in hits):', 'elif False:'),
    ("refs read in the step's own crate", 'if len(near) == 1:\n                    hits = near',
     'if False:\n                    hits = near'),
    ('a range upside down', 'if lo < 1 or hi < lo or hi > len(lines):', 'if lo < 1 or hi > len(lines):'),
    ('more files are read as references', "'.ppm', ", ''),
    ("a row's finding records a file as gone", 'return any(base in f and GONE.search(f) for f in row.findings)',
     'return any(base in f for f in row.findings)'),
    ("the pointer's window", 'lines[max(0, lo - 16):min(len(lines), hi + 3)]', 'lines[max(0, lo - 1):min(len(lines), hi)]'),
    ('a pointer after "not"', " or re.search(r'\\b(no|without|not)\\s*$', text[:m.start()])", ''),
    ('a list of pointers', "if re.match(r'\\s*,\\s*`', text[m.end():]) or", 'if'),
    ("pointers read in the step's own crate",
     "hits = [h for h in hits if any(h.split('/')[:2] == lp.split('/')[:2] for lp in local)] or hits",
     'hits = hits'),
    # edit-order
    ('a mention word after the naming', 'if MENTION_WORDS.search(after) and not CHANGE_WORDS.search(after):\n                continue',
     'if False:\n                continue'),
    ('a change word overrules a mention word', ' and not CHANGE_WORDS.search(after):', ':'),
    ('a mention word only after the naming', 'after = shell[m.end():hi]', 'after = shell[lo:hi]'),
    ('a form leaves its naming be',
     'if any(x.start() <= m.start() - lo < x.end() for x in MENTION_OF.finditer(shell[lo:hi])):\n                continue',
     'if False:\n                continue'),
    ('a form covers only its own naming',
     'if any(x.start() <= m.start() - lo < x.end() for x in MENTION_OF.finditer(shell[lo:hi])):',
     'if MENTION_OF.search(shell[lo:hi]):'),
    ("another step's work",
     "if re.match(r'[,\\s]*which\\b', shell[lo:hi]) and ANTECEDENT.search(clause[:lo].rstrip(', ')):\n                continue",
     'if False:\n                continue'),
    ('"which" after an ID', " and ANTECEDENT.search(clause[:lo].rstrip(', '))", ''),
    ('a colon preamble carries a mention', 'preamble = carry and not CHANGE_WORDS.search(shell)', 'preamble = False'),
    ('a change word overrules a preamble', 'preamble = carry and not CHANGE_WORDS.search(shell)', 'preamble = carry'),
    ('a preamble names nothing', "carry = clause.endswith(':') and not namings and", "carry = clause.endswith(':') and"),
    ('a preamble ends in a colon', "carry = clause.endswith(':') and", 'carry = True and'),
    ('a preamble says a mention word', ' and bool(MENTION_WORDS.search(shell)) \\\n            and not CHANGE_WORDS.search(shell)',
     ' \\\n            and not CHANGE_WORDS.search(shell)'),
    ('a preamble says no change word', 'bool(MENTION_WORDS.search(shell)) \\\n            and not CHANGE_WORDS.search(shell)',
     'bool(MENTION_WORDS.search(shell))'),
    ('a segment ends at a comma', "SEGMENT_END = re.compile(r',\\s+(?!`)|\\s+so\\s+')",
     "SEGMENT_END = re.compile(r'\\s+so\\s+')"),
    ('a comma before a name goes on with a list', ",\\s+(?!`)|\\s+so", ",\\s+|\\s+so"),
    ('"so" ends a segment', "(?!`)|\\s+so\\s+')", "(?!`)')"),
    ('a segment of names reads on', 'while hi < len(clause) and LIST_WORDS.fullmatch(shell[lo:hi]):', 'while False:'),
    ('a parenthesis says nothing outside it', "chars[i] = ' '\n    return ''.join(chars)",
     "pass\n    return ''.join(chars)"),
    ('a citation is no edit', 'if any(a <= m.start() < b and cites(clause[a:b]) for a, b in parens):\n                continue',
     'if False:\n                continue'),
    ('a citation cites a line', "if p and p.group('a') or BARE_POINTER.match(span.strip()):",
     'if p or BARE_POINTER.match(span.strip()):'),
    ('a bare :N cites', " or BARE_POINTER.match(span.strip()):\n            return True", ":\n            return True"),
    ('clauses cut outside parentheses', 'if m.start() not in inside:', 'if True:'),
    ('a fuzz target or a seed is no function', ' or TARGET_NAME.match(clause[m.end():]):', ':'),
    ("today's is the old behaviour", "today(?!['’]s)", 'today'),
    ('never', '|still|never|', '|still|'),
    ("one row's own edits", 'if a.row == b.row or a.id in before[b.id]', 'if a.id in before[b.id]'),
    ('a short name under one type is one function', 'len(na) >= 8 or (qa is not None and qa == qb)', 'len(na) >= 8'),
    ('a short name with no type is none', 'len(na) >= 8 or (qa is not None and qa == qb)', 'len(na) >= 8 or True'),
    ('two names under two types differ', 'if na != nb or (qa is not None and qb is not None and qa != qb):',
     'if na != nb:'),
    ('a note that settles two edits', 'if settles(rm, a, b.row) or settles(rm, b, a.row):\n                    continue',
     'if False:\n                    continue'),
    ('the settling note names the other row', " and row in {x.split('.')[0] for x in ids_in(sentence, rm)}", ''),
    ('a note settles by its sentence', "for sentence in re.split(r'(?<=\\.)\\s+', s.text()):",
     'for sentence in [s.text()]:'),
    # wiring
    ('a design step names an instrument freely', "if s.kind == 'design':\n            continue", 'pass'),
    ("an instrument's own row", 'if s.row != row and pat.search(text):', 'if pat.search(text):'),
    ("an instrument's least step", 'step_key(x.id)[0] >= least for x in', 'True for x in'),
    ('a build step reads no run',
     "if s.kind in ('watch', 'ci', 'test', 'measure') and s.row != 'MS-11' and not others",
     "if s.row != 'MS-11' and not others"),
    ('MS-11 reads its own runs', "and s.row != 'MS-11' and not others", 'and not others'),
    ('another workflow', 'and not others and', 'and'),
    ('a workflow named in the Where', "[p.rsplit('/', 1)[-1] for p, _ in s.where if p.startswith('.github/workflows/')]",
     '[]'),
    # fail-first and both-builds
    ('code spans blanked', "m = SAME_OUTPUT.search(re.sub(r'`[^`]*`', '`x`', s.what))", 'm = SAME_OUTPUT.search(s.what)'),
    ('a speed row is no speed-up', 'r"speed(s|ed|ing)? ?-?ups?\\b|', 'r"speed\\w*|'),
    ('a guard in the note', "if m and not FAIL_FIRST.search(s.exit + ' ' + s.notes):",
     'if m and not FAIL_FIRST.search(s.exit):'),
    ('fail-first reads build and fix steps', "if s.kind not in ('build', 'fix'):", 'if False:'),
    ('only a pin needs the builds', ' or not PINNED.search(s.exit) or', ' or',
     "f'exit: {PINNED.search(s.exit).group(0).lower()}'", "'exit: pin'"),
    ('text as data',
     "text = TEXT_WORDS.search(s.what) or (re.search(r'\\btext\\b', s.what, re.I) and TEXT_DRAWN.search(s.what))",
     "text = TEXT_WORDS.search(s.what) or re.search(r'\\btext\\b', s.what, re.I)"),
    ('the facade', ' or not facade_test(s, tree, ctx)', ''),
    ("the facade's test by its basename",
     "hits = tree.resolve(ref) + [p for p in ctx['new_paths'] if p == ref or p.endswith('/' + ref)]", 'hits = []'),
    ("the facade's test named in the exit", " + [r for r, _, _ in file_refs(s.exit + ' ' + s.notes)]", ''),
    ('both-builds reads build, fix and test steps', "if s.kind not in ('build', 'fix', 'test'):", 'if False:'),
    # deadlock
    ('a deadlock through a step it waits on', 'for x in rows_reached(t):\n                        via.setdefault(x, t)',
     'pass'),
    ("a deadlock through a row's steps",
     'for t in rm.rows[e].steps:\n                    for x in rows_reached(t.id):\n                        via.setdefault(x, e)',
     'pass'),
    ("a wait through its own row's step is that step's",
     "if not starts or any(STEP_RE.match(t).group('row') == s.row for t in starts):", 'if not starts:'),
    ("a split step's wait", 'starts = [e] if e in rm.steps else split_parts(rm, e)',
     'starts = [e] if e in rm.steps else []'),
    ('a close naming a step of the row', " or any(n.startswith(s.row + '.') for n in names[x])", ''),
    ('a range in a closing line', 'names[x] = ids_in(rm.rows[x].closes, rm)',
     'names[x] = set(ID_IN_CELL.findall(rm.rows[x].closes))'),
    # speed-kind
    ('a speed row by its name', 'r"\\bspeed rows?\\b|MS-04', 'r"MS-04'),
    ('a speed row by the group it joins', "|MS-04's (?:[a-z]+ group|set) gains\", re.I)", '", re.I)'),
    ("a speed row by MS-04's set", '(?:[a-z]+ group|set) gains', '(?:[a-z]+ group) gains'),
    ("a speed row riding with code takes the work's kind",
     "if not any(p.endswith(CODE_EXT) and not p.startswith('crates/tinker-pdf/benches/') for p, _ in s.where):",
     'if True:'),
    ("the benches are a speed row's own code", " and not p.startswith('crates/tinker-pdf/benches/') for p", ' for p'),
    ('a measure step is what a speed row is', "if s.kind in ('measure', 'design', 'doc', 'watch')",
     "if s.kind in ('design', 'doc', 'watch')"),
    ('a design step may name speed rows', "if s.kind in ('measure', 'design', 'doc', 'watch')",
     "if s.kind in ('measure', 'doc', 'watch')"),
    ('a doc step may name speed rows', "if s.kind in ('measure', 'design', 'doc', 'watch')",
     "if s.kind in ('measure', 'design', 'watch')"),
    ('a watch step may name speed rows', "if s.kind in ('measure', 'design', 'doc', 'watch')",
     "if s.kind in ('measure', 'design', 'doc')"),
    ('a denied speed row is none', 'not affirmed(SPEED_ROW, s.what)', 'not SPEED_ROW.search(s.what)'),
    ('no, not, without or never denies', "r'\\b(?:no|not|without|never)\\b(?:\\s+\\S+){0,2}\\s*$'", "r'(?!)'"),
    # tail-wait
    ('a watch or measure step in its tail may wait on a tail',
     "if not rows or (s.kind in ('watch', 'measure') and s.id in tail_of(s.row)):", 'if not rows:'),
    ("only in its own row's tail", ' and s.id in tail_of(s.row)', ''),
    ('a tail is what no other kind waits on', "if any(x in ctx['before'][o] for o in steps if o not in t):", 'if False:'),
    ("MS-11's acts come first", 'stalled = full - kept - front', 'stalled = full - kept'),
    ('an act reached another way', '                kept |= mine\n', ''),
    ('tails left out together', 'stalled = full - kept - front',
     'stalled = set().union(*[dep_acts(rm, ctx, d) - other_acts(rm, ctx, s, d) for d in rows]) - kept - front'),
    # flags
    ('--only takes an emitted name', 'emitted = {n: parent for parent, names in EMITS.items() for n in names}',
     'emitted = {}'),
    ('--waits reports', 'if args.waits:', 'if False:'),
]


# Each word of a rule's vocabulary put back broken, one at a time: the rule's
# compiled pattern swapped, in the loaded module, for one without the word (a
# fragment to drop, or an (old, new) pair).
WORDS = [
    ('MENTION_WORDS', ['stays?|', 'unchanged|', 'untouched|', 'unaffected|', 'already|', "today(?!['’]s)|", 'still|',
                       'never|', '|as before']),
    ('CHANGE_WORDS', ['changes|', 'changed|', 'gains?|', 'gained|', 'adds?|', 'added|', 'now|', 'rewrit\\w*|',
                      'replac(?:es|ed)|', 'extend(?:s|ed)|', 'becomes?|', 'became|', 'fix(?:es|ed)|', 'routes?|', '|routed']),
    ('MENTION_OF', [('(?:\\brather\\s+than\\s+an\\s+edit\\s+of|', '(?:'), '|\\bbeside', '|\\bbuilt\\s+on',
                    '|\\bas\\s+for', '\\bas\\s+`x+`(?:\\s+and\\s+`x+`)?\\s+(?:\\w+s|do)\\b|',
                    '(?<!\\bchanges )', '(?<!\\bchange )', '(?<!\\brewrites )', '(?<!\\balters )',
                    "|(?<!\\bchanges )(?<!\\bchange )(?<!\\brewrites )(?<!\\balters )\\bwhat\\s+(?:\\w+'s\\s+)?`x+`"
                    "(?:\\s+and\\s+`x+`)?\\s+\\w+"]),
    ('RUN_ON_DEVELOP', ['run|', 'runs|', 'job|', 'jobs|', 'leg|', 'legs|', '|dispatch\\w*']),
    ('SAME_OUTPUT', ['cach(e|es|ed|ing)\\b(?! (values?|data|results?)\\b)|', 'once per \\w+|', 'faster|',
                     'speed(s|ed|ing)? ?-?ups?\\b|', 'quadratic|', 'without (a |the )?copy|', 'zero-copy|',
                     'in place of `x`|', 'mov(e|ed|es) (from|into|to) `x`|', 'refactor\\w*|', 'memoi[sz]\\w*|',
                     'hoist\\w*|', 'deduplicat\\w*|', 'streamed|', '|streaming']),
    ('FAIL_FIRST', ['fail\\w*|', 'inject\\w*|', 'flips?|', 'refus\\w*|', 'panics?|', 'fires?|',
                    'count(s|ed|er|ers)?|', '|band']),
    ('PINNED', ['fingerprint\\w*|', 'hash\\w*|', 'goldens?|', 'pin(s|ned)?|', 'byte-identical\\w*|',
                'byte for byte|', 'byte-equal|', 'sha-256|', '|reftests?']),
    ('TEST_CLAIM', ['unchanged|', 'keeps?|', 'stays?|', 'still|', 'existing|', '|already']),
    ('TEST_NEW', ['\\bnew\\b|', 'flips|', 'fails first|', '|failing first']),
    ('NO_DEFAULT', ['none\\s*(—|-|$)|', '—|', '-|', '|$', '|not scheduled until']),
    ('SHIPPING', ['shipping|', '|publishing']),
    ('OUTSIDE', ['this|', "|the row's", ('|the) exit', ') exit')]),
    ('NOT_OUTSIDE', ['\\bnot|', '|\\bnever', "|n['’]t"]),
    ('GONE', ['not in the tree|', 'no longer|', 'gone|', 'removed|', 'deleted|', 'renamed|', 'moved|', '|absent']),
    ('SETTLED', ['lands|', '|comes', 'second|', 'later|', '|last']),
    ('TEXT_WORDS', ['fonts?|', 'glyphs?|', 'typefaces?|', 'shap(ed|ing|er)|', 'epub|', 'x?html|', 'markdown|',
                    'cmaps?|', 'tounicode|', 'bidi|', '|reading order']),
    ('TEXT_DRAWN', ['draw\\w*|', 'drew|', 'render\\w*|', 'paint\\w*|', 'laid out|', 'lay\\w* out|', 'layout|',
                    'wrap\\w*|', 'reflow\\w*|', 'justif\\w*|', 'line[- ]break\\w*|', 'text box(es)?|', 'shadows?|',
                    'decoration|', '|measur\\w*']),
    ('TOOLING', ['tiers/[A-Z]|', 'steps/[A-Z]|', 'rows\\.json|', 'check_steps\\.py|', 'render\\.py|', 'roadmap-cut|',
                 'scratchpad|', '|batch_[A-Z]']),
    ('NOTE_LIKE', ['[*_]+\\s*notes?\\s*[*_]*\\s*[:.]|', '|\\bnotes?\\s*[*_]*\\s*:', 'flags']),
    ('LABEL_LIKE', ['today|', 'exit criterion|', 'closes the row|', 'doc|', 'depends on|', '|the tree at']),
    ('LIST_WORDS', ['and|', 'or|', 'the|', 'a|', '|an', '|[,\\s]']),
]
# Each word of an instrument's pattern, by the pattern's place in INSTRUMENTS;
# a pattern of one word is broken whole.
INSTRUMENT_WORDS = [
    (0, ['(?:paired )?']), (1, ['open|', 'text|', '|calibration']), (2, ['write|', 'merge|', '|analysis']),
    (3, ['whole']), (4, ['encode|', '|decode']), (5, ['sign|', 'layout|', 'fonts|', '|shaping']),
    (6, ['set|', 'groups?|', 'operations?|', 'benchmark|', 'suite|', '|CLI']), (7, ['window|', '|20\\b']),
    (8, ['probe|', 'test|', 'case|', 'open|', 'merge|', "'|", '|memory']), (9, ['band|', 'peak|', '|seven']),
    (10, ['whole']), (11, ['whole']),
]


def dropped(pattern, fragment):
    """A pattern with one word dropped."""
    old, new = fragment if isinstance(fragment, tuple) else (fragment, '')
    assert pattern.count(old) >= 1, (pattern, old)
    out = pattern.replace(old, new, 1)
    re.compile(out)
    return out


def word_breaks(src):
    """Each vocabulary word put back broken: (its name, a module without it)."""
    for const, fragments in WORDS:
        for fragment in fragments:
            mod = load(src)
            pat = getattr(mod, const)
            if fragment == 'flags':
                setattr(mod, const, re.compile(pat.pattern))
            else:
                setattr(mod, const, re.compile(dropped(pat.pattern, fragment), pat.flags))
            yield f'{const} without {fragment!r}', mod
    for i, fragments in INSTRUMENT_WORDS:
        for fragment in fragments:
            mod = load(src)
            name, pat, row_, least = mod.INSTRUMENTS[i]
            new = '(?!)' if fragment == 'whole' else dropped(pat.pattern, fragment)
            mod.INSTRUMENTS[i] = (name, re.compile(new), row_, least)
            yield f'instrument {i} ({name}) without {fragment!r}', mod


def load(src):
    mod = types.ModuleType('sweep_injected')
    mod.__file__ = os.path.join(HERE, 'sweep.py')
    exec(compile(src, mod.__file__, 'exec'), mod.__dict__)
    return mod


def main(argv):
    good = True
    with tempfile.TemporaryDirectory() as root:
        fx = Fixtures(root)
        found = keys(sweep, fx.main)
        names = set(sweep.CHECKS) | set().union(*sweep.EMITS.values())
        for name in sorted(names):
            want = {k for k in EXPECTED if k[0] == name}
            got = {k for k in found if k[0] == name}
            good &= check(f'{name}: fires on each fixture and on no twin ({len(want)} expected)',
                          got == want and bool(want), f'missing {sorted(want - got)}, extra {sorted(got - want)}')
        good &= check('no finding under a name outside the checks', all(k[0] in names for k in found),
                      str(sorted(k for k in found if k[0] not in names)))
        good &= check('each finding reported once', len(found) == len(set(found)),
                      str(sorted(k for k in set(found) if found.count(k) > 1)))

        acc = fx.acc_files
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', acc['every']])
        good &= check('every finding accepted: exit 0', code == 0, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', acc['one short'], '--quiet'])
        good &= check('one finding unexplained: exit 1, and --quiet prints it alone',
                      code == 1 and 'FINDING' in out and 'accepted ' not in out, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', acc['stale']])
        good &= check('an acceptance that matches nothing: exit 1', code == 1 and 'matches no finding' in out, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', acc['stale'], '--only', 'deps'])
        good &= check('a stale line outside --only is not stale there', code == 0, out[-300:])
        for name, text in (('a line of two fields', 'four tab-separated fields'),
                           ('a field empty', 'four tab-separated fields'), ('twice', 'twice'),
                           ('no such check', 'no check is called')):
            code, out = run_main(sweep, ['--repo', fx.main, '--accepted', acc[name]])
            good &= check(f'an acceptance file with {name}: exit 1', code == 1 and text in out, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', fx.acc_bad])
        good &= check('an acceptance file it cannot read: exit 2',
                      code == 2 and 'cannot read the acceptance file' in out, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', fx.acc_bad + '.none'])
        good &= check('an acceptance file that is missing: exit 2',
                      code == 2 and 'cannot read the acceptance file' in out and 'missing' in out, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', fx.acc, '--only', 'no-such-check'])
        good &= check('--only naming no check: exit 2', code == 2, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--accepted', fx.acc, '--only', 'double-create'])
        shown = {l.split()[1] for l in out.split('\n') if l.startswith(('accepted', 'FINDING'))}
        good &= check('--only double-create shows it and every name it reports under',
                      code == 0 and shown == sweep.EMITS['double-create'], out[-300:])
        for args in ('two-homes', ' two-homes '):
            code, out = run_main(sweep, ['--repo', fx.main, '--accepted', fx.acc, '--only', args])
            shown = {l.split()[1] for l in out.split('\n') if l.startswith(('accepted', 'FINDING'))}
            good &= check(f'--only {args!r} takes a name double-create emits, and shows it alone',
                          code == 0 and shown == {'two-homes'}, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.main, '--waits'])
        good &= check("--waits reports each whole-row wait that brings a person's act",
                      code == 0 and re.search(r'^AN-22\.1 +waits on all of AN-21', out, re.M) is not None
                      and 'whole-row waits' in out, out[-300:])
        for name, path in fx.variants.items():
            code, out = run_main(sweep, ['--repo', path, '--accepted', fx.acc])
            good &= check(f'{name}: exit 2, saying so',
                          code == 2 and 'cannot read' in out and fx.fragments[name] in out, out[-300:])
        code, out = run_main(sweep, ['--repo', fx.dispatch, '--accepted', fx.acc])
        got = {l.split()[2] for l in out.split('\n') if l.startswith('FINDING   verbs')}
        good &= check('a dispatch the reader no longer matches: one finding, not a flood',
                      got == {'tools/tpdf/src/main.rs'}, out[-300:])
        rm = sweep.Roadmap(fx.main)
        for (kind, sample, want), got in zip(VOCAB, vocab(sweep, rm)):
            good &= check(f'{kind}: {sample!r} gives {want!r}', got == want, f'gave {got!r}')
        good &= check("a name's instrument patterns rise in least step", ordered_instruments(sweep))

        if '--inject' in argv:
            src = open(os.path.join(HERE, 'sweep.py'), encoding='utf-8').read()
            base = fx.signature(load(src))
            for name in list(sweep.CHECKS):
                mod = load(src)
                mod.CHECKS[name] = lambda rm, t, c: []
                diff = [k for k, v in fx.signature(mod).items() if v != base[k]]
                good &= check(f'inject: {name} knocked out shows in {", ".join(diff) or "nothing"}', bool(diff))
            for name, *pairs in INJECT:
                broken = src
                for old, new in zip(pairs[::2], pairs[1::2]):
                    if broken.count(old) != 1:
                        good &= check(f'inject: {name}: its source is no longer where the injection expects it', False,
                                      f'{broken.count(old)} matches for {old[:60]!r}')
                        break
                    broken = broken.replace(old, new)
                else:
                    diff = [k for k, v in fx.signature(load(broken)).items() if v != base[k]]
                    good &= check(f'inject: {name} broken shows in {", ".join(diff[:4]) or "nothing"}'
                                  + (f' and {len(diff) - 4} more' if len(diff) > 4 else ''), bool(diff))
            for name, mod in word_breaks(src):
                diff = [k for k, v in fx.signature(mod).items() if v != base[k]]
                good &= check(f'inject: {name} shows in {", ".join(diff[:4]) or "nothing"}', bool(diff))
        for path in fx.variants.values():  # give back what the variants took away, so the folder can go
            for d, dirs, _ in os.walk(path):
                for x in dirs:
                    os.chmod(os.path.join(d, x), 0o755)
    return 0 if good else 1


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
