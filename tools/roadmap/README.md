# The roadmap sweep

`sweep.py` checks every step of the roadmap at once for the defects the
reviews of the cut found by hand. It reads the roadmap as it is on disk —
`docs/roadmap/<TIER>.md`, `H.md`, `NG.md`, and the counts in the index
`docs/ROADMAP.md` — and the tree beside it, less what `.gitignore` names as
build output or fetched data. It prints one line per finding, each finding
once. It changes no file, fetches nothing and spawns nothing.

```sh
python3 -I tools/roadmap/sweep.py              # every check
python3 -I tools/roadmap/sweep.py --quiet      # unexplained findings only
python3 -I tools/roadmap/sweep.py --only h-blocks,row-deps
python3 -I tools/roadmap/sweep.py --waits      # every whole-row wait that brings a person's act
python3 -I tools/roadmap/selfcheck.py --inject # the sweep's own tests
```

It exits 0 when every finding is fixed or accepted. It exits 1 when a finding
is unexplained, or an acceptance matches nothing (an acceptance may not
outlive what it accepted) or is malformed. It exits 2 when `--only` names no
check, or when it cannot read the roadmap, the tree or the acceptance file:

- a file it reads is missing, or cannot be read (not UTF-8, a folder, no
  permission);
- a row, a step, an H row or a limit appears twice;
- a row heading, a step line, a row's label or a step's note is written in a
  way the reader would pass over (each such line is refused, never skipped);
- a row lacks one of its four labels (Today, Exit criterion, Doc with its
  Depends on, Closes the row);
- an H row's ID is not `H-<n>`, its table's first column is not ID, its table
  has no Blocks column (a table of standing duties needs none), or its cells
  do not fill its table;
- the index's counts differ from what the files hold, H rows and limits
  included.

Run it after every change to the roadmap, and before a roadmap commit lands.

## What to do with a finding

Fix it in the roadmap: the row's text in its tier's file under
`docs/roadmap/`, the step's cells, its closing line or its findings.

A finding that is not a defect is accepted instead. Add a line to
`accepted.tsv` with four fields separated by tabs:

1. the check;
2. the subject;
3. the item, as the run prints them;
4. the reason.

The reason says why the finding is not a defect, so a later reader can
disagree. A finding read from a text names that text's field in its item
(`what:`, `exit:`, `closes:`, `finding 2:`), so an acceptance covers that
field alone. Never accept a finding to get green: an acceptance is a claim
about the roadmap.

## How it reads the roadmap

- **A wait is what Depends on says.** A row's steps are numbered in an order
  that runs, but the index picks "the first step in Order of work whose
  Depends on are all done" ("How a step runs", 1). So a step whose Depends on
  are done is picked even while an earlier step of its row waits, and number
  order adds no wait.
- **When a step lands, its line leaves and its ID is never reused.** An ID
  below its row's last step that the row no longer has reads as landed, and
  a Depends on naming it, a step's or a row's, is satisfied. A step split
  into `<step>a` and `<step>b` reads as both parts, wherever it is named.
- **A row that has left** cannot be told from a row that never was, and the
  sweep reports it as gone. The commit that removes a row also removes it
  from every Depends on that names it, since that dependency is done.
- **What a check reads.** Most checks read backticked names and paths and the
  Depends on cells. Some read words as well: fail-first, both-builds,
  wiring, h-blocks, deadlock, tests, edit-order and speed-kind. refs,
  row-refs and row-pointer read a row's findings for the words that say a
  file is gone. Every check that reads IDs reads ranges, and the words that
  make one none ("hands X to Y"). The rows below say which words, and
  `selfcheck.py` holds each word to a phrase it must match and a twin it
  must not.

## The checks

Each check is one class of defect. "Found" says where the class was first
found by hand.

| Check | What it decides | Found | What it cannot see |
| --- | --- | --- | --- |
| `shape` | Each row has one step table and a step in it. Each step's ID belongs to its row and its kind is one of the eight. Its What and Exit are not empty, its Where is not blank, and its steps are in order. | STEP_SPEC's rules, held by the cut's own checker | Whether the cells say anything sensible |
| `deps` | Each Depends on names a step, row, H row or limit the roadmap holds, or a step that has landed or was split. No step waits on itself, on a later step of its row or on its own row, and there is no cycle. | STEP_SPEC; the cut's cycle check | A wait wider than the step needs (`tail-wait` sees one kind) |
| `paths` | A `(new)` path is absent from the tree. An unmarked path exists, or a step this one waits on creates it. | STEP_SPEC; the pilot's review | Whether the path is the right home (`new-home`) |
| `new-home` | The step that first creates a file does not put it beside an existing home (every later step naming the file also says `(new)`). An existing home is: a module that is already a folder or a file; a file of the same name in a folder above or below, in the same crate or package (`crates/x`, `tools/x`, `bindings/x`, `xtask`, `fuzz`, `.github`), that the step does not name; a twin stem (`lex.rs` beside `lexer.rs`, `shaping.rs` beside `shape.rs`); or a family already split. Siblings (`jpeg/encode.rs`, `jxl/encode.rs`) are the crate's layout. | The reviews of 23 and 24 October: "a new file beside an existing home" | A home with a different name |
| `refs`, `row-refs` | Each `file` or `file:N` a text names exists, and the line, or range of lines, is within it; the texts are a step's cells, a closing line, a finding or a row's text. A partial path (`design/x.md`) resolves by its end, and a file a step creates counts. A name several files carry is read in the step's own crate; failing that, it is a finding only when none of them has the line. A row's text may cite a file its own findings record as gone, moved or renamed. | The class fix of `c5e307c3`: steps citing the single-file roadmap's lines | What the line says (`pointer`); a reference written without its extension, or with one the sweep does not read |
| `pointer`, `row-pointer` | For `` `name` (`file:N`) ``, the name is at line N, in the fifteen lines above it or in the three below. A file several crates carry is read in the step's own crate. A pointer after "no", "not" or "without", or in a list of pointers, is not read. | The rows' pointers, read from the tree on 9 October | A pointer written another way |
| `tests` | A test that a clause says exists is in the file it names, or a step builds it. Such a clause has `` `file.rs` `test_name` `` with "keeps", "stays", "still", "unchanged", "existing" or "already"; it runs between full stops and semicolons. A clause that says "new", "flips" or "fails first" claims nothing. | The reviews: "a name the tree does not have" | A test named without its file |
| `idents` | A backticked code name is in the code, or a step builds it. A code name is a `::` path's last segment, a dotted name's last part (`Intl.PluralRules`), or a camel, snake or screaming name; a camel name has a hump, after a lower-case letter (`PdfNoSuchA`) or a run of capitals (`XRefNoSuch`), so `IDs` and `PDFs` are words. A step builds a name it gives in its What, or one it marks new (the name or list of names just before `(new`). A snake name in an Exit is the test that closes the step. Build output, data folders and this folder are not the code. | The reviews: "a name the tree does not have" | A name in prose; a name the code has for something else; a snake name of five words or more, which reads as a test's name |
| `verbs` | Each `` `tpdf verb` `` is an arm of `tpdf`'s dispatch, or a step that changes `tools/tpdf/src/main.rs` adds it; an option's arm (`"keep" => …`) is no verb. A dispatch the reader no longer matches is one finding. | The reviews | A flag of a verb |
| `double-create` | Two rows that could each be first to create one source or doc file. It also reports under other names (list below). One row's steps share a plan, and a test file or folder only gathers, so neither counts. | The reviews: "two rows building one thing"; the review of this step: no step created the OCR crate | Two rows building one thing in two files |
| `edit-order` | Two steps of two rows that change the same function of an existing code file wait on one another, or one of them says what whichever lands second keeps (list below). A short name (`render`) is one function only under one type (`Page::render`); a snake name, or one of eight letters or more, is one by itself; one name under two types (`Canvas::new_page`, `Writer::new_page`) is two. | The reviews: "two rows editing one file with no order between them" | A function named by one plain word, or changed in other words; a mention word whose subject is a noun before the function ("`Bitmap` already turns … for `to_png`"); a fix written as what the function never does |
| `row-deps` | Each row, step or H row a row's Depends on line names is in the roadmap and carried by a step: waited on, or, for an H row, as `h-blocks` says. Ranges (`H-31 to H-33`) are read; a landed step is done, and a split step is its parts. | The H rows' rule: a step whose exit needs a person's act names it in Depends on | An ID whose prefix is no tier's or H's (`PDF/X-4` reads the same) |
| `h-blocks` | Each row or step an H row's Blocks cell names is in the roadmap and carries the H row; a split step carries it in each part. The rules are in the list below. | This sweep, on 24 October 2026 | Whether "outside this exit" is true |
| `h-listed` | An H row a step waits on names the step's row in its Blocks cell, so the cell says all the act blocks (H.md: each row says what it blocks). A standing duty blocks nothing and has no Blocks cell. | The review of this step: SG-12.7 waits on H-92, whose cell did not name SG-12 | A wait that comes through another row's step |
| `wiring` | A step that names an instrument waits on the step that lands it. This holds for every step but a `design` step, and not in the instrument's own row. MS-02's job lands at MS-02.6; MS-04's groups at MS-04.3 (open, text, calibration) to MS-04.7 (sign, layout, fonts, shaping), its set at MS-04.8 and its window at MS-04.9; MS-05's probe at MS-05.1 and its bands at MS-05.3; MS-01's guards at MS-01.6 and its wasm leg at MS-01.8. A `watch`, `ci`, `test` or `measure` step that reads a run, job, leg or dispatch on `develop` waits on MS-11. This does not apply in MS-11 itself, or when the step names, in its text or Where, a workflow other than `ci.yml`, `fuzz.yml` and `corpus.yml`; those three run on `develop` only after MS-11. | The review of 24 October (49 steps re-pointed to MS-01.8) | An instrument named another way |
| `fail-first` | Some `build` or `fix` steps say their output must not move: a cache, "once per", a speed-up, a copy removed, code moved or deduplicated, streaming. Such a step has a guard that fails first in its exit or note: a count, an injection, a band, a refusal. | STEP_SPEC's first trap | A refactor described in other words |
| `both-builds` | A `build`, `fix` or `test` step whose facade test pins a value over text (a fingerprint, a hash, a golden, a reftest, bytes) says the pin holds in both builds. A facade test is a file under `crates/tinker-pdf/tests/` the step names, in its Where, exit or note, by path or by basename. Every step runs both builds where text is involved ("How a step runs", 3, in the index); a pinned value is what can differ. | AGENTS.md: both builds wherever fonts or text are involved | A pin worded otherwise |
| `deadlock` | A step waits on a whole row whose closing line names the step's row, or a range or a step that holds it, directly or through what it waits on. That is a deadlock if the close counts on this row's work. A wait that comes through an earlier step of the step's own row is that step's finding. "Hands X to Y" is no range. | The review of 24 October: MS-01 could never close | A close counting work it does not name |
| `speed-kind` | A step whose only work is a speed row is a `measure` step: the index's kind for a benchmark. Such a step adds a speed row, or gives MS-04's set or one of its groups an operation, and names no code outside `crates/tinker-pdf/benches/`. "No speed row" and "without a speed row" add none. A `design`, `doc` or `watch` step may name speed rows. `tail-wait` reads the kind: a speed row typed as other work is no tail, so a stall behind it goes unseen. | The review of this step: RC-20.15 and RC-21.5 were typed `ci` | Any other step whose kind does not fit its work |
| `tail-wait` | A step waits on whole rows, and through them on a person's act that only their tails wait on (the rules are in the list below). | The reviews of this step: whole-row waits stalled the machine work on a person, and a measure step passed a tail's act to the steps after it | A wait through a step that the waiting step may need |
| `tooling` | No text, a row's own included, names a file of the cut's working folder (`tiers/`, `steps/`, `rows.json`…), which is not in the tree. | The verification round of 24 October: MS-02.5 cited `tiers/P.md` | |
| `step-refs` | Each step ID and H row a text names, a row's own included, is in the roadmap, has landed, or was split. | The cut's checker | |

`double-create` also reports under these names:

- `design-first`: a design doc that a step other than a `design` step could
  be the first to write.
- `new-home-first`: a file put in a crate or module that another row creates
  (its `Cargo.toml`, `lib.rs`, `mod.rs` or `<module>.rs`), by a step that
  waits on no step naming that file.
- `two-homes`: a module planned as both `x.rs` and `x/mod.rs`.
- `no-crate`: files planned in a crate that no step creates.

`edit-order` reads each naming of a function in its own segment of its
clause, and a function is changed when one naming is not a mention:

- A clause ends at a full stop, colon or semicolon outside parentheses. A
  segment ends at a comma, unless the next name of a list follows it, or at
  "so". A segment of names alone reads on into the next, as a list does.
- A naming is a mention when a form around it leaves it be: an edit "rather
  than an edit of" it, work beside it, built on it or "as for" it, "as it
  does", and "what it draws" (not "changes what it answers"). Such a form
  holds only for the naming it covers.
- A naming is also a mention when a mention word follows it in its segment
  and no change word does. The mention words say the function stays, is
  unchanged, untouched or unaffected, is as before, or already, still or
  never does something, or does it today ("today's" is the old behaviour a
  change replaces). The change words say it changes, gains, adds, now does
  something, is rewritten, replaced, extended, fixed or routed, or becomes
  something. "Keeps" is no mention word: "`x` keeps its signature" and "`x`
  keeps each parsed space" change `x`.
- A segment that begins "which" after another step's or row's ID tells of
  that step's work ("after K-02.2, which changes what `parse_space`
  answers"). A clause that ends in a colon, names nothing and says a mention
  word and no change word makes the next clause's namings mentions ("These
  stay: `a` and `b`").
- A naming followed by "fuzz target" or "seed" is that target's or seed's. A
  naming inside a parenthesis that cites a file's line is a citation. Words
  inside a parenthesis say nothing of a function outside it.
- A sentence of either step that says what "whichever lands second" (or
  later, or last) keeps, and names the other step's row, settles the pair.

`h-blocks` decides that a row carries an H row this way:

- A step waits on the H row.
- An H row with a default may instead be named in a step or in the closing
  line.
- An H row without one may be named in a clause of the closing line that puts
  it outside the exit, written "outside this exit", "outside the row's exit"
  or "outside the exit", in any case. A negation just before it ("not",
  "never", "isn't") does not count.
- An H row has a default when its table has a Default column and the cell is
  not "none" or "not scheduled until…".
- A clause of Blocks that begins with "shipping" or "publishing" holds no row,
  and nor does a row whose claims or shipped work are named
  (`FO-08a/b's and FO-09a–e's claims`); a row named before it in the clause
  still does.
- Ranges (`AN-27…AN-34`, `SG-01…11`, `FT-04a–d`, `ED-12—ED-14`,
  `CD-19.9 to CD-19.13`), `FI-07a/b` and an ID before a full stop are read; a
  spaced dash (`RC-07 — 16 steps`) is no range.

`tail-wait` holds machine work to H.md's rule that it never stalls on a
decision, so such a wait is narrowed to the row's other steps:

- A person's act is an H row with no default.
- A row's tail is its `watch` and `measure` steps that no other kind of its
  steps waits on, at any depth.
- The waited rows' tails are all left out at once.
- An act the step reaches another way is no stall.
- MS-11's acts are taken with MS-11, third in the order of work after CD-19
  and P-27.1 and before every other row, so they stall nothing.
- A `watch` or `measure` step in its own row's tail may mean to wait on a
  tail. One that other kinds of its row's steps wait on is their
  prerequisite, and is read as they are.

## What it does not check

- **A wait wider than the step needs, through steps the waiting step may
  need.** `--waits` lists every wait on a whole row that brings a person's
  act the step waits on in no other way. At this commit there are 289 such
  waits, by 282 steps:

  | Kind of wait | Waits |
  | --- | ---: |
  | By a `watch` or `measure` step in its own row's tail | 63 |
  | By another step, bringing only MS-11's act (H-02) | 128 |
  | By another step, through steps outside the waited row's tail | 94 |
  | `tail-wait`'s four acceptances | 4 |

  The sweep cannot tell whether the waiting step needs those steps.
- **Whether a step's kind fits its work**, except for a speed row
  (`speed-kind`). `tail-wait`, `fail-first`, `both-builds`, `wiring` and
  `design-first` read kinds, so a step of the wrong kind can hide a finding
  from them.
- **Whether a step is right.** It reads names, paths and dependencies. It
  does not read the code a step changes, or whether the exit really fails
  first.
- **Prose.** A name or path written without backticks is not read. Nor is a
  function named by one plain word (`composite`): a backticked plain word is
  as often a word as a name.

## How the sweep is tested

`selfcheck.py` builds a small roadmap and tree in a temporary folder. For
every check, and every rule inside one, it writes a case that must fire and a
twin that must not, and it requires exactly the expected findings, each
once. It also tests:

- every way the input can be unreadable, each a variant of the fixture that
  must exit 2 with its own message (39 variants);
- the words each rule reads, in a table of phrases each rule must match and
  twins it must not (369 phrases);
- the acceptance file, the exit codes, `--only`, `--quiet` and `--waits`.

With `--inject` it does more:

- It knocks each check out, one at a time (24).
- It puts each rule back broken in the source, one at a time (232).
- It drops each word of each rule's vocabulary, one at a time, by swapping
  the rule's compiled pattern in the loaded module (192).
- It fails if any of these breaks leaves what the fixture, the variants and
  the table show unchanged.
- An injection whose source text has moved fails too, so an edit to a rule
  also updates its injection.

## At this commit

The sweep reads 459 rows, 2 532 steps and 137 H rows. It reports 68
findings, and every one is accepted in `accepted.tsv` (68 lines). None is
unexplained:

| Check | Accepted |
| --- | ---: |
| `refs` | 13 |
| `edit-order` | 11 |
| `deadlock` | 11 |
| `row-refs` | 8 |
| `new-home` | 8 |
| `fail-first` | 5 |
| `tail-wait` | 4 |
| `idents` | 4 |
| `wiring` | 2 |
| `h-blocks` | 1 |
| `double-create` | 1 |
