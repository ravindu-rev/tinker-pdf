# Validate on save

Every test that writes a document holds what it wrote to the strict
validator and to every profile the document claims, through one helper, and
`cargo xtask written` fails a test file that writes a document and does not
call it. The validators are first-party already (ruling 13). What this
design adds is that no writer path stays outside them: the builder, the
editor's save, the facade's save door, the signer and the sealer, the C
ABI's two writers, the CLI's writing commands, and every conversion that
synthesises a PDF from something that is not one.

The helper adds no rule. It decides which verdicts a written document is
held to, and how strictly, from who authored what is in it: a document this
engine wrote from nothing else is held to every verdict being empty; a save
of somebody else's file is held to owning the bytes it wrote and to leaving
the file no worse; a test that breaks a file on purpose says exactly what it
broke.

## Scope

- One helper, `written_support`: one policy file that every test tree
  compiles, and two readings that feed it — a cos reading over
  `tinker_pdf_cos`, and a facade reading over `tinker_pdf` that the C ABI's
  and the CLI's test modules include as well.
- Four doors: `authored`, `rewritten`, `expecting`, and the facade's
  `synthesised`.
- Profile claims — PDF/A by XMP `pdfaid`, PDF/UA by XMP `pdfuaid`, PDF/X by
  `/Info /GTS_PDFXVersion` — each read by its own validator's reading and
  held within the coverage this build implements.
- `cargo xtask written`: what it counts as a writing test, what it counts as
  calling the helper, and the pending list of files not migrated yet.
- Where SD-20's object grammar joins.

## Non-goals

- **A rule of its own.** The helper reads `tinker_pdf_cos::validate` and
  `Document::validate_pdfa`, `validate_pdfua` and `validate_pdfx`, and adds
  nothing to what any of them decides. A defect it surfaces is one those
  validators already define.
- **Acceptance anywhere else.** See *What a clean verdict proves*.
- **The corpus.** `xtask corpus-run`'s strict pass rewrites every corpus
  file this engine reads cleanly and ratchets the rewrite's structure tier
  (`docs/verification.md`); the helper holds what tests write.
- **Documentation tests, examples, benches, fuzz targets and the bindings'
  parity scripts.** None is a test file. A doc example writes to show a
  call; the gates run the examples for the line each prints; a bench times;
  a fuzz target asserts its own property (K-04's); and each parity surface
  re-opens its own artefact through the strict validator before it reports
  (`xtask/src/parity.rs`, and its row in `SPAWNERS`).
- **Bytes a test assembles or damages and then only reads.** `corrupt.rs`'s
  hand-built files and `hostile_input.rs`'s mutated fixtures are inputs to
  the reader; no writer produced them.
- **Profiles this build has no validator for**, and the clauses a profile
  validator stages or abstains on.
- **Fixing what the helper finds during a migration.** K-03.4's rule: a
  defect the helper surfaces is reproduced and fixed by class as a `fix` of
  its own, and its file stays on the pending list until then.
- **A proof per call.** The lint's unit is a file or a `src` test module, as
  the row's exit is; see the risks.
- **An engine change.** The helper is test code over the public API. The
  only lines it puts under `src` are two `#[cfg(test)]` self-aliases.

## Design

### Where the helper lives

The crate graph decides it, through three constraints that are each
enforced:

1. A `tinker-pdf-cos` test cannot reach the facade. `cargo xtask dag` holds
   every crate to `ALLOWED` (`xtask/src/main.rs:636`), and
   `internal_dependencies` counts a dev-dependency as an edge
   (`xtask/src/version.rs:373-382`), so the cos crate may not take
   `tinker-pdf` even for its tests — and the profile validators are the
   facade's (`crates/tinker-pdf/src/lib.rs:2020-2081`).
2. The C ABI and the CLI reach the facade and nothing under it:
   `("tinker-pdf-ffi", &["tinker-pdf"])` (`xtask/src/main.rs:807`) and
   `TOOLS` (`xtask/src/main.rs:836`), ruling 11. Their test modules can name
   `tinker_pdf` and not `tinker_pdf_cos`.
3. A crate's own `src` test modules cannot name the crate they are in:
   inside `tinker-pdf-cos`, `tinker_pdf_cos::` does not resolve, and inside
   the facade, `tinker_pdf::` does not.

So the policy names no crate at all, and a reading stands on each side of
it:

| File | Names | Compiled into |
| --- | --- | --- |
| `crates/tinker-pdf-cos/tests/written_support/held.rs`, the policy | no crate | every tree, through one of the two readings |
| `crates/tinker-pdf-cos/tests/written_support/mod.rs`, the cos reading | `tinker_pdf_cos` | the cos crate's tests and `src` test modules |
| `crates/tinker-pdf/tests/written_support/mod.rs`, the facade reading | `tinker_pdf` | the facade's tests and `src` test modules, and the C ABI's and the CLI's test modules |

`held.rs` is the one place a door's rule is written. A reading opens the
bytes, authenticates, runs its validators, and hands the policy every
finding as a neutral item:

```rust
pub struct Item {
    pub verdict: &'static str, // "structure", "semantics", "PDF/A", "PDF/UA", "PDF/X"
    pub owned: bool,           // the writer's whatever it was handed
    pub exact: String,         // the finding's kind as `Debug` prints it: the expected form's key
    pub class: String,         // what survives renumbering: a rewrite's key
    pub at: Option<u64>,       // the byte it is at, when it has one
    pub shown: String,         // how a failure prints it
}
```

Because the policy names no engine type, it compiles under either crate,
and a verdict SD-20 adds is more items from the two readings with no change
to it.

How each tree reaches it:

- **cos tests**: `mod written_support;`.
- **Facade tests**: `mod written_support;`, the facade reading, which takes
  the policy by
  `#[path = "../../../tinker-pdf-cos/tests/written_support/held.rs"]`. Both
  readings declare the policy with an explicit `#[path]`, which resolves
  from the reading's own directory in whatever context the reading is
  itself included.
- **The C ABI's and the CLI's test modules**: the facade reading, by
  `#[path]`. A `#[path]` inside an inline `mod tests { }` resolves from the
  file's directory with a directory added for each enclosing inline module,
  and, in a file that is not a `lib.rs`, `main.rs` or `mod.rs`, one named
  after the file before them — the reference's rule for path attributes
  inside inline module blocks — so from `tools/tpdf/src/writing.rs`'s test
  module the include climbs from `src/writing/tests/`.
- **The `src` test modules of the cos crate and of the facade**: K-03.2 adds
  `#[cfg(test)] extern crate self as tinker_pdf_cos;` and
  `#[cfg(test)] extern crate self as tinker_pdf;` to the two crates'
  `lib.rs`, so a reading's paths resolve to the crate itself in its own
  unit-test build and nowhere else.

A packaged crate's tests therefore name a file outside the package. That is
already so — `crates/tinker-pdf/tests/pubsec_write.rs:36` takes
`testdata/simple-text.pdf` by `include_bytes!` — and publishing builds the
library, not its tests.

### The two readings are one strict implementation

The cos reading opens with `CosDocument::open` and calls
`tinker_pdf_cos::validate` (`crates/tinker-pdf-cos/src/validate.rs:771`).
The facade reading opens with `Document::open` and calls
`Document::validate`, which is `tinker_pdf_cos::validate` over the
document's `inner` (`crates/tinker-pdf/src/lib.rs:1715-1717`), and `inner`
is `CosDocument::open(bytes)` whenever the bytes are read as a PDF
(`lib.rs:1458`). `Document::open` converts what it does not read as one — a
container signature at a fixed position (`lib.rs:1433-1437`), a standalone
SVG, image, HTML or FB2 file (`lib.rs:1444-1456`) — and `standalone::sniff`
answers `None` for any bytes whose header window holds `%PDF-`
(`crates/tinker-pdf/src/standalone.rs:184-191`). The facade reading fails a
document whose `Document::archive()` (`lib.rs:1640`) is `Some`, so it
validates the bytes it was handed, read as a PDF, or nothing. One function
over one parse, in every tree.

### Opening, and a locked document

A door takes the bytes with a label, or `Locked { bytes, key }`, where `key`
is `Key::Password` or `Key::Recipient` — the two ways a document is opened
(`CosDocument::authenticate` and `authenticate_with_recipient`,
`crates/tinker-pdf-cos/src/doc.rs:1147` and `:1172`, and the facade's at
`lib.rs:1752` and `:1955`). `validate` wants an authenticated document,
because a stream it cannot decrypt reads as one that does not decode
(`validate.rs:764-771`). An encrypted document handed over with no key is
tried with the empty user password, which `Document::open` leaves untried
on purpose (`lib.rs:1773-1777`); if that fails, the door fails and says the
key is missing. Bytes that do not open fail the
door. A door never changes what it is handed: it opens its own copy, so no
fingerprint and no pinned page moves when a test starts calling it.

### The doors

Each verdict's findings become items as follows.

| Verdict | Owned | Class, a rewrite's key | Exact, the expected form's key |
| --- | --- | --- | --- |
| strict, `Tier::Structure` | yes | `DefectKind::as_str` | the `DefectKind` |
| strict, `Tier::Semantics` | no | `DefectKind::as_str` | the `DefectKind` |
| a claimed profile | no | the claim, the clause and the `FindingKind` | the `FindingKind` |

**`authored(label, bytes)`** — a document this engine wrote from nothing it
did not write: built by `DocumentBuilder`, synthesised by a conversion, or
saved from an input that was itself held as authored. Every verdict is
empty. For the strict verdict that includes having opened on `Trust` with
no repair, because the ladder and every repair the reader performed are the
validator's first rules (`DefectKind::OpenedLenient`, `Repaired` and
`RepairedWhileReading`); the two leniencies `validate` does not count — a
DCT stream left encoded, and a predefined CMap this build did not compile
in — are its own call (`is_a_defect`, `validate.rs:851`).

**`rewritten(label, input, output)`** — a save of an input this engine did
not write: a committed fixture, a corpus file, a hand-built or damaged file.
The door reads the input through the same reading first, with the input's
own key.

- A full rewrite re-serialises every object and owns the file: no owned
  item; of every other class, no more items than the input has.
- An incremental update (7.5.6) keeps the input's bytes as its prefix — the
  door checks that the output begins with them — and with them the input's
  sections, so a structure defect of the input's lives on in the output.
  Owned items are then held to no more of each class than the input's, and
  to none at or past the input's length: the appended section, its objects
  and its trailer are the writer's. The trailer rules read the newest
  section and carry no offset (`validate.rs:1346-1401`), so a trailer
  defect the input did not have is a class it did not have, and fails.
- A profile the output claims is held to no more findings of each class
  than the input's under the same claim. The claim is part of the class, so
  an output claiming what the input did not is held to none.

Classes rather than findings, because a rewrite renumbers: a defect's
object and offset move, and so do the kinds that carry a number
(`PageParentWrong { found }`, `ObjStmNumberMismatch { container, .. }`),
while a kind's label does not. It is the corpus report's own unit
(`kind_counts`, `validate.rs:757`). With a clean input, no more than nothing
is nothing: `rewritten` is then `authored`, so choosing it cannot loosen a
test whose input is this engine's.

**`expecting(label, bytes, expected)`** — the expected-defects form, for a
test that writes damage on purpose: `strict_validator.rs`'s injections, a
profile rule's near-miss twin, a semantic fault built through the builder's
raw calls to test a reader. The items of every verdict, matched by `exact`
as a multiset, are exactly the expected list: each listed as often as it
occurs, nothing found that is not listed, nothing listed that is not found.
`Expected::Defect(DefectKind)` in both readings; the facade's adds
`Expected::PdfA`, `Expected::PdfUa` and `Expected::PdfX` over
`FindingKind`. They are typed values, so the compiler checks each name and
a test states a kind's fields (`StreamLengthNotExact { declared, actual }`)
as well as its name. An empty list is `authored`. Nothing is exempt
anywhere: no door skips a verdict, and the lint keeps no list of excused
files — its pending list is of files not migrated yet, and it only shrinks.

**`synthesised(label, document)`**, the facade's — a conversion's output.
`document.archive()` must be `Some`; then `document.cos().bytes()` is held
as `authored`, re-opened from those bytes through the same reading, and the
`Document` is handed back. A conversion writes every object of what it
synthesises, so what it synthesises is authored.

The cos doors hand back the `CosDocument` they opened and the facade's the
`Document`, so a test reads on from the call that held it. A door that
fails panics at its caller (`#[track_caller]`) with one line per item its
form does not admit, grouped by verdict; a rewrite's counts stand beside
the input's, and a profile's lines beside the groups that ran and how many
clauses the build did not decide.

### A profile claim

The helper reads no packet and no `/Info` of its own. Each validator reads
its claim whatever groups it is asked for, and the helper takes the claim
from the verdict it holds — one reading, the one the findings come from.

- **PDF/A.** `validate_counting` reads the claim before any rule runs
  (`crates/tinker-pdf/src/pdfa.rs:2268-2276`), through `flavour_of`
  (`pdfa.rs:2330`). A document claims PDF/A unless its verdict's `flavour`
  is `None` and the verdict holds `FindingKind::MetadataMissing` or
  `FindingKind::NoFlavourClaimed` (`pdfa.rs:1219`, `:1221`): no packet, or a
  packet that names no part. A `pdfaid:part` naming no part ISO 19005
  defines is a claim and a finding (`PartUnknown`), and the door fails it.
  The rule is written as the absence of a claim so that a malformed-claim
  kind added later is held rather than passed.
- **PDF/UA.** `claim_of` (`crates/tinker-pdf/src/pdfua.rs:870`) reports
  `PdfUaIdentifierMissing` when the packet carries no `pdfuaid:part`
  (`pdfua.rs:895-900`). A document claims PDF/UA unless its verdict holds
  that finding; `PdfUaPartUnknown` is a claim, held and failed.
- **PDF/X.** `GTS_PDFXVersion` in `/Info`, as a text string
  (`crates/tinker-pdf/src/pdfx.rs:939`): the verdict's `claim` is `Some`, or
  the document claims nothing. A value that is not a string is no claim, as
  the validator reads it.

Held within the implemented coverage: `validate_pdfa`, `validate_pdfua` and
`validate_pdfx` run every group this build has rules for
(`PdfACoverage::IMPLEMENTED` and its two counterparts), and the door holds
their `findings`. It never fails on what the build does not decide — PDF/A's
staged rules, PDF/UA's and PDF/X's `abstained`, a claimed PDF/X level that
`XFlavour::is_validated` (`pdfx.rs:139`) does not run — and prints them only
beside a failure. A claimed level no rule runs under is held to nothing.

The cos reading cannot run these validators, and it does not pass what it
cannot hold. It refuses a document it is asked to hold that might claim a
profile, by a test that over-reads every claim the facade's readers make:
the catalog's decoded `/Metadata` packet (`tinker_pdf_cos::xmp_metadata`,
`crates/tinker-pdf-cos/src/outline.rs:625`) containing `pdfaid`, `pdfuaid`
or either identification namespace (`www.aiim.org/pdfa/ns/id/`,
`www.aiim.org/pdfua/ns/id/`), in ASCII or in UTF-16 of either byte order;
or a `/GTS_PDFXVersion` key in `/Info`, whatever its value. The refusal
decides nothing about the document: it is the cos reading declining to
vouch for what it cannot hold. It names the facade tree, and it costs a test
a move, never a pass. The archival writer (`DocumentBuilder::archival` and
`finish_archival`, `crates/tinker-pdf-cos/src/build.rs:9183` and `:9219`) is
the cos crate's, and a test that finishes an archival document lives in the
facade tree, as `pdfa_writer.rs` does.

### What `cargo xtask written` counts

`xtask/src/written.rs`, run by `cargo xtask check`, whose count in `USAGE`
(`xtask/src/main.rs:34`) becomes eleven.

**The units**, each judged on its own text:

- every `.rs` file under a `tests/` directory of a crate under `crates/` or
  `tools/`, support modules included, except the helper's own files;
- under `src/`, every module declared with a `#[cfg(...)]` whose predicate
  names `test`: an inline one from its attribute to its closing brace, and
  an out-of-line one as the whole file the declaration names — its
  `#[path]`, or else `name.rs` or `name/mod.rs` beside the declaring file
  when that file is a `lib.rs`, `main.rs` or `mod.rs`, and under a
  directory named after it otherwise;
- a file outside those that a unit includes by `#[path]` is read as part of
  the unit that includes it, so a generator a test takes from
  `crates/tinker-pdf/benches/support/inputs.rs` is that test's.

A `#[cfg(test)]` item that is not a module — a counter, a helper function —
is not a unit, nothing under `xtask/`, `fuzz/`, `bindings/`, `benches/` or
`examples/` is a unit of its own, and no doc comment is read.

**Reading a unit.** Comments are dropped before matching, as
`check_oracles` drops them (`xtask/src/main.rs:1105-1110`), but by a lexer
that knows string and character literals: `line.split("//")` cuts a line
at the `//` of a URL inside a literal, and an inline module's extent needs
its braces counted outside literals. A writer named in a comment or inside
a literal is not a write.

**What counts as a write.** A unit writes when its code names a writer
entry point:

| Path | Matched |
| --- | --- |
| the builder | `.finish_reporting(` or `.finish_archival(` anywhere; `.finish(` in a unit that names `DocumentBuilder` |
| the editor, the signer and the sealer | `.save_signed(`, `.save_timestamped(` or `.save_sealed(` anywhere; `.save(` in a unit that names `DocumentEditor` or `.editor()` |
| the facade's save door | `write::save` or `SaveOptions` |
| the cos crate's lower entry points | `incremental_update(` or `linearize::linearize` |
| the C ABI | `tpdf_builder_finish` or `tpdf_editor_save` |
| the CLI | a writing command of `tools/tpdf/src/writing.rs` — `merge`, `split`, `rotate`, `encrypt`, `decrypt`, `attach`, `stamp`, `sanitise` — called through `writing::`, or as a free call, not a method of the same name, from that file's own test module |
| a conversion | `open_markdown`, `.archive()`, or a conversion fixture module: `epub_support`, `xps_support`, `cbz_support` |

The entry points are the public functions that hand back a document's
bytes: `DocumentBuilder::finish`, `finish_reporting` and `finish_archival`
(`crates/tinker-pdf-cos/src/build.rs:8322`, `:8335`, `:9219`);
`DocumentEditor::save` and `save_sealed`
(`crates/tinker-pdf-cos/src/edit/save.rs:261`, `:284`); `save_signed` and
`save_timestamped` (`crates/tinker-pdf-cos/src/edit/signing.rs:38`, `:97`);
`tinker_pdf::write::save` (`crates/tinker-pdf/src/write.rs:337`);
`write::incremental_update` and `linearize::linearize`
(`crates/tinker-pdf-cos/src/write.rs:810`,
`crates/tinker-pdf-cos/src/linearize.rs:144`); `tpdf_editor_save` and
`tpdf_builder_finish` (`crates/tinker-pdf-ffi/src/lib.rs:2794`, `:4509`);
and the CLI's commands, which all write through `save_to`
(`tools/tpdf/src/writing.rs:556`). The pairings keep a name that is not a
writer from counting: `corrupt.rs`'s hand-built `b.finish("/Root 1 0 R")`
(`crates/tinker-pdf-cos/tests/corrupt.rs:152`) and a digest's `.finish()`
sit in units that name no `DocumentBuilder`. A conversion writes inside
`Document::open`, a name every reading test shares, so a conversion is
recognised by what only a conversion test names: Markdown's own entry
point (`Document::open_markdown`, `lib.rs:1502`), the synthesised document's
report, and the modules that build EPUB, XPS and comic containers.
`.archive()` is also the name of the XPS and EPUB package readers' accessor
for their ZIP (`crates/tinker-pdf/src/xps/opc.rs:855`,
`crates/tinker-pdf/src/epub/ocf.rs:798`), which only a conversion's own
tests reach. `DocumentBuilder::from_html` hands back a builder and is
counted by the builder row.

**What counts as calling the helper.** The unit names `written_support` —
in a `mod`, a `use` or a `#[path]` — and calls a door, through it or
imported from it. A fixture function that shares a door's name, as
`strict_validator.rs`'s `rewritten` does, does not count without the
module, and an imported door and a local item of one name do not compile
together.

**Outcomes**, each a problem naming the file, and for a write the entry
point it matched and its line:

- a unit that writes, does not call the helper, and is not pending fails;
- a pending unit that calls the helper fails until it is taken off the
  list — `SPAWNERS`' second direction (`xtask/src/main.rs:1128-1136`), for
  its reason: an allowance that outlives its need is how the next one gets
  in;
- a pending entry that names no unit, or a unit that no longer writes,
  fails;
- the list is sorted and names no file twice.

**The pending list** is `PENDING` in `written.rs`: the units that write and
do not call the helper when K-03.3 lands, by path, an inline `src` module
under its file's path. From then on every change to it is a deletion, and
K-03.8 deletes it.

**A unit, not a call.** A unit that calls the helper once passes however
many documents it writes. That is the row's exit, which is per file, and the
honest reach of a check by name; each migration's exit is that every
document its files write is held, and the doors hand back what they are
given, so that the held write is the shortest one to spell.

### Where SD-20's grammar joins

SD-20.3 lands `validate_grammar(version)` in
`crates/tinker-pdf-cos/src/validate/grammar.rs`, with its facade spelling in
`crates/tinker-pdf/src/lib.rs`, so both readings can call it. SD-20.7 joins
it in those two readings and nowhere else:

- each reading runs it at the document's declared version: the later of the
  header's and the catalog's `/Version`, as `outline::version_string` reads
  it (`crates/tinker-pdf-cos/src/outline.rs:294`) and `Document::pdf_version`
  returns it — the version read the way the engine already reads it;
- its findings become a third verdict of items, not owned, whose class is
  the finding's kind and its object path and whose exact key is the kind.
  The class needs the path spelled in keys from the trailer down, as
  SD-20.3's walk from the trailer finds each object, and not in object
  numbers, so that a rewrite's findings compare with its input's across
  renumbering; a path that carried numbers would leave the class to the
  kind alone;
- `Expected::Grammar` joins both readings' expected forms.

The policy does not change, which is why SD-20.7's files are the two
readings. A rewrite's input and output are each held at their own declared
version, so a save that changes the version owns what the change makes
unknown or deprecated. SD-20.7's exit holds every test on the helper to the
grammar in that step's own commit, so what the grammar finds across the
migrated tests is fixed by class before it, under K-03.4's rule.

### What a clean verdict proves

The helper proves that a written document agrees with this project's own
readers: the strict validator, which is this project's reader with the
leniency ladder off; the profile validators, which are this project's
reading of ISO 19005, 14289 and 15930 within the coverage each states; and,
once SD-20 joins, this project's reading of the published object-grammar
model. It proves nothing about any other reader. A clause misread in a
writer and in the validator agrees with itself, and the helper then passes
it on every writer path at once. That is the first property
[verification.md](../verification.md) says first-party verification cannot
prove — a reader nobody here wrote accepting this engine's output, NG-04 —
and this design does not narrow it. What it changes is reach: no writer
path is outside the agreement.

## Milestones

1. **K-03.2, the helper.** The policy, the two readings and the two
   `#[cfg(test)]` self-aliases; `docs/verification.md`'s strict-validator
   section names the helper and this design. Exit:
   `crates/tinker-pdf/tests/written_helper.rs` (new) —
   `an_undamaged_written_document_passes` (a built document, its full
   rewrite, its incremental save, its linearized save and its encrypted save
   with the key, each `authored`); `one_injected_structure_defect_fails_by_name`
   (a cross-reference offset moved: `authored` fails naming the defect);
   `a_claimed_profile_with_a_finding_fails` (a PDF/A claim over an
   unembedded font — a plain builder's page draws a standard font and
   `DocumentEditor::set_xmp_metadata`, `crates/tinker-pdf-cos/src/edit/metadata.rs:166`,
   adds the claim, since the archival builder refuses the font at the call
   — fails the facade reading naming the font finding, and the twin without
   the claim passes);
   `the_expected_defects_form_passes_only_on_exactly_those_defects` (the
   moved offset with its exact defects passes; one fewer, one more, or one
   field wrong fails); and three of this design's —
   `a_rewrite_is_held_to_its_input` (a hand-built fixture carrying a
   semantics defect a rewrite copies through — an annotation with no
   `/Subtype` — rewritten in full and incrementally, passes `rewritten` and
   fails `authored` naming `annot-subtype-missing`; a rewrite that adds a
   defect fails naming it; an incremental save damaged past the input's last
   byte fails), `the_two_readings_give_one_strict_verdict` (the
   cos reading, taken by `#[path]`, and the facade reading give the same
   items for the same bytes, damaged and not), and
   `the_cos_reading_refuses_a_profile_claim` (documents claiming PDF/A and
   PDF/X fail the cos reading naming the facade tree, and their twins
   without the claim pass).
2. **K-03.3, the lint.** `cargo xtask written`, run by `cargo xtask check`,
   with `PENDING`; `docs/verification.md` names it beside
   `cargo xtask oracles`. Exit: `xtask/src/written.rs`'s unit tests
   `a_writing_test_that_skips_the_helper_fails`,
   `a_listed_file_that_now_calls_the_helper_fails` and
   `a_writer_named_in_a_comment_is_not_a_write`, and three of this
   design's — `a_conversion_is_a_write`,
   `only_the_test_modules_of_a_src_file_are_read` (writer code outside a
   test module is not a write, and a test module whose literals hold
   unbalanced braces is read to its own closing brace) and
   `a_listed_file_that_no_longer_writes_fails`; `cargo xtask check` is green
   on the tree with the pending list, and fails when one listed file is
   taken off it.
3. **K-03.4, the creation writers' tests.** Each file the step names holds
   its documents by `authored`, or by `expecting` where it writes damage on
   purpose. `hostile_input.rs` holds what it builds and synthesises from
   hostile input — its `from_html` builders by `authored`, its Markdown and
   standalone conversions by `synthesised` — so hostile input never makes a
   writer emit a file the validator refuses; the fixtures it mutates and
   only reads are not writes. Exit: `cargo test -p tinker-pdf` passes in the
   default and `bundled-fonts` builds with every file the step names calling
   the helper, and `cargo xtask check` passes with them off the pending
   list.
4. **K-03.5, the conversions.** Each file the step names holds what it
   synthesises by `synthesised`, and its file-local check goes, among them
   `valid` in `epub_validated.rs:133`, `xps_validated.rs:49` and
   `cbz_validated.rs:42`. Exit: the same two commands, with those files off
   the pending list.
5. **K-03.6, saves of a changed document.** The editor's, the signer's, the
   sealer's, encryption's, the redactor's and the recoder's tests, and the
   `write.rs` and `subset.rs` test modules: `rewritten` for a fixture, its
   incremental saves included, `authored` for a document this engine wrote;
   a signed or sealed save held after the signature is written, over the
   bytes a reader gets; an encrypted one with its key. Exit: the same two
   commands, with those files off the pending list.
6. **K-03.7, every other facade unit on the list.** Exit: the same two
   commands, with no unit under `crates/tinker-pdf` left pending.
7. **K-03.8, the cos crate, the C ABI and the CLI.** The cos crate's tests
   and `src` modules, `strict_validator.rs` through `expecting` in place of
   its `verdict` and `refused` (`strict_validator.rs:120`, `:132`); the C
   ABI's `src` test modules, their `validate().is_empty()` checks gone; the
   CLI's test modules, `assert_clean` (`tools/tpdf/src/writing.rs:963`) gone;
   `PENDING` emptied and deleted; `README.md` and `docs/verification.md` say
   the rule holds by test. Exit: `written.rs`'s
   `a_new_writing_test_without_the_helper_fails`; `cargo xtask check`;
   `cargo test --workspace`; and `cargo test -p tinker-pdf-cos` and
   `-p tinker-pdf` with `--no-default-features`.
8. **SD-20.7, in SD-20's own commit.** The grammar in both readings, as
   *Where SD-20's grammar joins* settles. Exit: SD-20.7's own — every test
   on the helper holds its output to the grammar, and one injected unknown
   key fails by name.

## Risks

| Risk | Mitigation |
| --- | --- |
| Writer, reader and validator share one reading of ISO 32000, so a clause misread in all three passes on every writer path at once | Not mitigated here, and said so in *What a clean verdict proves* and in `docs/verification.md`'s list of what first-party verification cannot prove. What narrows it is the part of the suite that answers to something other than this engine |
| A file calls the helper once and writes other documents it never holds, and the lint passes it | The unit is a file, as the row's exit is. Each migration's exit is every document its files write, and the doors hand back what they are given, so the held write is the shortest one to spell |
| Matching by name misses a write — a builder made in one unit and finished in another — or counts something that is not one | The entry points are one closed table in `written.rs`, each row pinned by a unit test; `.finish(` and `.save(` count only beside their writer's type; a misfire is fixed by refining a row with its test, never by listing the file |
| A conversion whose test builds its input without the three fixture modules and never reads `Document::archive()` escapes the conversion row | A new conversion's design adds its entry point to the row, as `open_markdown` is there; every conversion that exists is reached by the row as written |
| A new writing test is put on the pending list instead of calling the helper | The list is K-03.3's snapshot and every later change to it is a deletion; adding to it weakens a check, which `AGENTS.md` forbids; K-03.8 deletes it, and then there is nothing to add to |
| The cos reading's claim test misses a packet that spells the identification only through character references or in UTF-32, and passes a claim it cannot hold | No writer in this tree emits one; the test reads the identification schemas' own spellings in UTF-8 and in both UTF-16 orders; a test that writes a claim belongs in the facade tree, where each validator's own reading applies |
| Holding a rewrite by class lets it cure one defect and introduce another of the same kind | Stated. The class is the unit renumbering leaves, and with a clean input the rewrite form is the authored form, so it is loose only where the input was already wrong |
| The facade reading opens through `Document::open`, which converts what is not a PDF | It fails any document whose `archive()` is `Some`, so it validates the bytes handed to it as a PDF or not at all, and `the_two_readings_give_one_strict_verdict` holds the two readings together |
| A test target names a file outside its package by `#[path]`, so a published crate's tests do not build on their own | Already so for `include_bytes!` from `testdata/`; publishing builds the library |
| Validating every written document lengthens the suite | One strict read per door call; a profile validator only on a claim; the strict and profile verdicts from one open |
| A defect the helper surfaces stalls a migration | K-03.4's rule: reproduced and fixed by class as a `fix` of its own, its file pending until then; a fix that costs something is a row of its own, at the owner's price |
| SD-20's grammar, joining every migrated test in one commit, surfaces findings across many files at once | Fixed by class before SD-20.7's commit under the same rule; the policy needs no change for the join, so that commit is the two readings and the fixes |
