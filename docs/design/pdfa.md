# PDF/A validation and writing

When this is done, the engine answers "does this file conform to PDF/A, and
if not, which clause did it break?" with typed findings a test can assert on,
the 2 907-file veraPDF corpus — today purely a never-crash bar
([verification.md](../verification.md)) — graduates to a conformance bar whose
agreement rate is ratcheted like every other corpus number, and
`DocumentBuilder` gains an archival profile that either produces a file this
engine's own validator accepts with zero findings, or refuses at build time
with an error naming the clause it could not satisfy. The roadmap item this
implemented said exactly that — the corpus "graduates to a conformance bar" —
and has left the [roadmap](../ROADMAP.md), which now carries the staged rules
and Level A validation from this design and nothing else of it.

## Scope

- **A validator**: a rule engine over the opened `CosDocument`, covering
  ISO 19005 parts 1 through 4, keyed by conformance level (A/B for part 1;
  A/B/U for parts 2 and 3; the e/f variants of part 4). Verdicts are typed
  findings naming the 19005 clause and the object that broke it, in the
  shape ruling 10 ([rulings.md](../rulings.md)) already gives warnings
  (`Warning { offset, object, kind }` in
  `crates/tinker-pdf-cos/src/warn.rs`).
- **Flavour detection**: reading the claimed `pdfaid:part` /
  `pdfaid:conformance` from the XMP packet (ISO 32000-1 14.3.2), so a caller can ask
  "validate against what this file claims" or name a flavour explicitly.
- **The bar**: the veraPDF *corpus*, whose expected verdict is annotated in
  each file's name (clause, test number, `pass`/`fail`). The annotations are
  published data and admissible under ruling 13; the tool that produced them
  is not invoked.
- **A writer profile**: on `DocumentBuilder` and `WriteOptions`
  (`crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf-cos/src/write.rs`):
  `/OutputIntents` with an embedded ICC destination profile (ISO 32000-1
  14.11.5), the
  XMP conformance packet, every font embedded, and every forbidden feature
  refused at build time rather than discovered at validation time.
- **Surfacing**: `Document::validate_pdfa` on the facade, and a `--pdfa`
  mode on `tpdf check`, which already opens files and reports warnings.

## Non-goals

- **PDF/UA.** Same corpus, different standard; it needs the structure tree,
  and belongs to [design/tagged-pdf.md](tagged-pdf.md).

  *Amended at milestone 6.* This paragraph used to continue: "Level **A** of
  parts 1–3 also requires tagged structure, so Level A verdicts are staged
  behind that design too; this one delivers B and U honestly rather than A
  wrongly." That was true when it was written and is not now.
  [design/tagged-pdf.md](tagged-pdf.md) closed all six of its milestones and
  `PageBuilder::tagged` exists, so **the writer claims Level A** — by tagging,
  with an untagged page and a missing natural language both refused at
  `finish_archival` rather than written and discovered.

  *Amended again, 14 September 2026.* The paragraph here used to say the
  validator side of Level A was "a different matter and is still staged", and
  that a Level A file this build reports nothing about had had its tagging
  looked at by nobody. Both claims have landed now: the `structure` group
  carries the four rules ISO 19005-1 6.8 and ISO 19005-2/3 6.7 make over the
  object graph, 19 of the 21 logical-structure fixtures in the corpus agree,
  and the writer's own Level A output is judged by them — see "What the level A
  group measured" below. What is still staged there is the part of the clause
  that lives in a **content stream**: a `/Lang` in a marked-content property
  list (two fixtures), and 6.2.11.7.3's `/ActualText` per character (five).
- **PDF/X and PDF/E.** Out entirely.
- **Conversion.** No "fix this file into PDF/A" repair mode. Validation
  reports; building conforms; nothing rewrites an arbitrary document's
  colour or fonts into compliance.
- **A general RDF store.** XMP is RDF/XML, and the amendment on
  `xmp_metadata` in `crates/tinker-pdf-cos/src/outline.rs` already records
  why `tinker-pdf-cos` will not parse it. The validator reads exactly the
  XMP shapes 19005 checks — not arbitrary RDF graphs.
- **Bundled typefaces.** The no-bundled-faces policy
  ([THIRDPARTY.md](../../THIRDPARTY.md)) stands: the writer profile
  requires embedding, therefore the caller must supply face bytes, and the
  API says so instead of shipping a font.

## Design

**Where it lives.** The rule engine is a new module in the facade,
`crates/tinker-pdf/src/pdfa.rs`. This is a placement argument, not a
convenience: the checks group by the machinery they need — syntax checks
need only the COS document, font checks need `tinker-pdf-font`, colour
checks need `tinker-pdf-color`, XMP checks need `tinker-pdf-xml` — and the
facade is the one crate that already depends on all four
(`crates/tinker-pdf/Cargo.toml`). Putting XMP parsing in `tinker-pdf-cos`
would add the cos→xml edge that the `xmp_metadata` amendment deliberately
refused; putting the engine anywhere lower splits it. `xmp_metadata` keeps
its bytes-out contract, and the facade parses those bytes through the
`tinker-pdf-xml` `Reader`/`Event` pull API into the handful of properties
19005 metadata clauses examine.

**Types.** A `PdfAFlavour { part, level }` names what is being validated
against. A finding is:

```rust
pub struct ConformanceFinding {
    /// The clause broken, e.g. part 1, "6.1" (file structure).
    pub clause: Clause,
    /// The object that broke it, when one is addressable (ruling 10).
    pub object: Option<ObjRef>,
    /// The closed set of defects the engine can detect.
    pub kind: FindingKind,
}
```

`FindingKind` is closed, like `WarningKind`: a new rule is a deliberate,
reviewable change to what "validates" means. A verdict is the flavour plus
the findings; pass is an empty list, and there is no boolean that discards
the list. Clause identifiers cite the ISO 19005 part's own numbering (part
1 groups as 6.1 file structure, 6.2 graphics, 6.3 fonts, and so on) —
findings name the clause, and where the same defect maps to different
clauses across parts, the mapping table is data, not duplicated rules.

**Rule groups, by machinery.** Rules register in a static table keyed by
flavour applicability, each declaring which group it belongs to:

1. **Syntax-only** (largest count, least machinery): header and trailer
   shape, no `/Encrypt` (ISO 32000-1 7.5.5), file identifier present,
   forbidden filters
   (LZWDecode in part 1 — ISO 32000-1 7.4 names the filter set), forbidden action and
   annotation types, embedded-file rules per part (forbidden in 1, PDF/A
   attachments only in 2, anything in 3), no XFA. These read the
   `CosDocument` through its existing resolution API and nothing else.
2. **Font rules**: every font used by a content stream has an embedded
   program (ISO 32000-1 9.9), `/Widths` consistent with the program, symbolic-flag and
   encoding constraints, Unicode mapping present for level U. These reuse
   `tinker-pdf-font` to parse the embedded program — the same leaf the
   renderer already trusts — never a second parser.
3. **Colour rules**: an `/OutputIntents` entry with subtype `GTS_PDFA1`
   and an ICC `DestOutputProfile` (ISO 32000-1 14.11.5); device colour spaces only
   under a matching output intent; ICC profile validity. Profile parsing is
   [design/icc.md](icc.md)'s deliverable — until it lands, colour rules
   that need to look *inside* a profile are staged, and the staging is a
   named refusal in the module, not a silent pass.
4. **XMP rules**: packet well-formed, `pdfaid:part`/`pdfaid:conformance`
   present and matching the claimed flavour, `/Info` consistency with XMP
   where the part requires it.

Machinery is built lazily per group, so a syntax-only sweep over 2 907
files never parses a font program it does not need. The validator runs on
untrusted input, so ruling 1 binds it: every rule returns findings or
nothing, never panics, and the existing hostile-input sweep
(`crates/tinker-pdf/tests/hostile_input.rs`) grows a validator call so
mutated fixtures exercise the rules on every commit.

**The bar is the corpus's own annotations.** The veraPDF corpus is
unusual, and this whole design leans on why: it is *atomic* (one clause per
file) and *annotated* (the expected verdict is in the filename). Those
annotations are a published statement about what each file is, made by the
people who wrote the conformance suite — data, not a program — so ruling 13
admits them. Run the validator over the corpus, compare verdict to
annotation, record the agreement rate as a new per-corpus row in
`corpus/ratchet.json`, which `cargo xtask corpus-run` already refuses to
regress.

Each disagreement is classified and recorded rather than averaged away:
our bug (fix), a known staged rule (named in the module), or an annotation
this project reads differently (recorded with the file, the clause, and the
reading — the shape the bounds ledger uses for a justification field).

**What is lost, and it is real.** A second validator's verdict on a file
*this engine wrote* has no substitute here. The annotations cover the
corpus's files, not ours, so the writer profile is checked by the same rule
table that would accept its mistakes. Milestone 6 narrows that by building
fixtures deliberately at the edge of each clause; it does not close it.

**The writer profile.** `DocumentBuilder` gains a conformance mode set at
construction; `WriteOptions` gains the matching field for rewrites of
existing documents. Under a profile:

- **Fonts**: only embedded fonts are accepted. `add_base_font` (standard
  14, no program) is refused under the profile with a typed error saying
  why — the caller supplies face bytes exactly as `FontProvider`
  (`crates/tinker-pdf/src/fonts.rs`) already demands for rendering, and
  the subsetter the builder already runs at `finish` serves unchanged.
- **Output intent**: the builder writes `/OutputIntents` with a
  caller-supplied ICC profile. **The parameter is mandatory and there is no
  vendored default**, which is the branch this paragraph reserved and the
  gate decided.

  The reasoning is the licence and nothing else. `cargo xtask vendor`
  requires every vendored tree to declare an SPDX identifier `deny.toml`
  already allows. The ICC's own sRGB profiles — the ones every other producer
  embeds — carry the ICC's bespoke permission notice, which is permissive in
  substance and **has no SPDX identifier at all**, so it cannot declare one
  and cannot clear the gate. A third party's CC0 rebuild would clear it, and
  is not the canonical profile: it is one person's regeneration, and a file
  saying "these colours are for *this* device" is a claim about the caller's
  document that this crate has no standing to make on their behalf. That is
  the same argument the no-bundled-faces policy makes about a typeface.

  `ArchivalProfile::destination_profile` is therefore a `Vec<u8>` with no
  `Option` around it and no default, and `destination_space` beside it says
  which device it characterises — declared rather than parsed, because
  reading it would put an ICC parser in `tinker-pdf-cos`, which has no edge
  to `tinker-pdf-color` and should not grow one to answer a question the
  caller already knows the answer to.
- **Metadata**: the XMP packet with `pdfaid:*` is generated at `finish`,
  byte-deterministic like the rest of the writer (ruling 4's spirit: the
  determinism suite's document byte-hashes in
  `crates/tinker-pdf/tests/determinism.rs` gain a PDF/A fixture).
- **Refusals**: encryption plus a profile is refused in `WriteOptions`
  validation (`/Encrypt` is forbidden); transparency under part 1,
  forbidden filters, and every other buildable-but-nonconforming request
  is refused at the call that makes it, in the builder's existing
  refused-at-the-door style — nothing is discovered by the validator that
  the builder allowed.

The loop closes end to end within this repository: files the profile writes
are validated by our own validator and by the strict structural validator
([render-verification](render-verification.md)'s sibling for the writer).
Under ruling 13 there is no outside verdict on them, which is the limit
named above.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
| --- | --- | --- | --- |
| 1 | Validator core: flavour types, `ConformanceFinding`, rule table, XMP flavour detection, `Document::validate_pdfa`, `tpdf check --pdfa` | Unit fixtures per rule shape pass; hostile-input sweep calls the validator with zero panics; `tpdf check --pdfa` exits by verdict | M |
| 2 | Syntax-only rule group (part 1 clauses first — largest count, least machinery), then parts 2–4 syntax deltas | Validator verdict vs filename annotation over the veraPDF corpus's file-structure clauses recorded as a ratchet row in `corpus/ratchet.json`; `corpus-run` refuses regression | L |
| 3 | XMP rule group: packet well-formedness, `pdfaid` agreement, `/Info` consistency | Metadata-clause corpus files agree with annotations at the recorded rate; a wrong-flavour fixture yields exactly the metadata finding, asserted by kind | M |
| 4 | Disagreement ledger against the corpus annotations | Clause-level disagreement list committed, every row carrying a mandatory reason string; each classified (our bug / staged rule / a reading recorded with its clause); a row without a reason fails the test that reads the ledger | M |
| 5 | Font and colour rule groups (colour rules needing profile internals staged behind [design/icc.md](icc.md)) | **Done.** Font-clause files 64/200 to 103/200, colour-clause 134/449 to 278/449, the bar 1017/2371 to 1201/2371 with the false-positive count unchanged at one. Eleven staged rules named in `PDFA_STAGED` and asserted by clause in `pdfa_fonts.rs` and `pdfa_colour.rs` | L |
| 6 | Writer profile on `DocumentBuilder` + `WriteOptions`, output intent, XMP generation, typed refusals | **Done for `DocumentBuilder`.** Fifteen fixtures in `pdfa_writer.rs`, each with a near-miss twin, each judged twice — by the full validator with complete coverage and by the strict structural validator. One refusal test per forbidden feature. `WriteOptions` is **not** done and the note below says why | L |

## What milestones 2 to 4 actually measured

*Recorded August 2026, at the commit that landed the disagreement ledger.*
Numbers rather than adjectives, because the risk table above says coverage is
a measured number and not a word.

**The bar is 2 371 files, not 2 896.** The veraPDF corpus carries 2 896 files
annotated `-pass-` or `-fail-`, and 525 of them are tests of a *different*
standard — 434 PDF/UA fixtures, 85 in the suite's own TWG directory, six
against ISO 32000 itself. A PDF/UA file annotated `pass` is a statement that
it conforms to PDF/UA; it makes no PDF/A claim at all, so this engine
correctly reports that it claims none, and scoring that as a disagreement
measures the measurement rather than the engine. The first census did exactly
that and produced 195 spurious disagreements. They are counted and printed,
and excluded from the rate. `Isartor test files` **are** in: they are the
original PDF/A-1b conformance suite.

**1 017 of 2 371 agree.** 830 of 831 files annotated `pass`, and 187 of
1 540 annotated `fail`. The shape of that is the shape of an early rule
engine: almost nothing conforming is reported wrongly, and most defects are
in clauses that have no rule yet.

| | files | agree |
| --- | --- | --- |
| annotated `-pass-` | 831 | 830 |
| annotated `-fail-` | 1 540 | 187 |
| **total** | **2 371** | **1 017** |

**The single false positive is a reading, and it is recorded as one.** ISO
19005-1 6.1.2 says the file header consists of `%PDF-1.n`. One fixture carries
`%PDF-2.0`, is annotated `pass`, and this build reports it. The rule is left
as it is and the ledger's first row records the reading against the clause,
which is the shape ruling 13 asks for when a first-party reading and a
published annotation disagree.

**Every disagreement is in the ledger.**
`crates/tinker-pdf/tests/pdfa_ledger.tsv`, 116 rows, each carrying a class
(`bug` / `staged` / `reading`) and a mandatory reason. The census asserts
coverage in both directions: a disagreement with no row fails, and a row whose
subject no longer disagrees fails as stale.

**The stale half of that assertion earned its keep immediately.** The first
ledger carried three rows saying, in those words, that nobody had established
why — two part 1 information-dictionary fixtures the consistency rule ought to
have caught, and one part 2 file-header fixture that survived every header
rule. Writing them down was what made them findable, and all three turned out
to be defects in this engine rather than readings:

- the `/Info` value was trimmed before comparison, so ` veraPDF Consortium `
  read as equal to `veraPDF Consortium`;
- an `/Info` entry that is present and is *not* a string was skipped rather
  than reported, and one fixture's `/Title` is an indirect reference to a font
  program;
- the header version rule accepted any digit, so `%PDF-1.9` passed — a version
  of PDF that has never been published.

Fixing them moved the rate by three files and turned three "I do not know"
rows into no rows at all, which is the whole argument for making the reason
field mandatory. What is left is four `reading` rows and 112 `staged` ones.

**Where the 1 353 unagreed `fail` files are.** 490 in the XMP
predefined-schema property rule, which needs the XMP specification's property
tables as vendored data and is the largest single staged rule in the build.
Roughly 600 in graphics, fonts, annotations and transparency — milestone 5 and
`docs/design/icc.md`. The rest are file-structure clauses whose defects the
reader has already normalised away by the time an object exists to apply a
rule to: implementation limits, hexadecimal string syntax, the EOL markers
around `obj` and `stream`, cross-reference subsection spelling. That last
family is worth naming as a limit rather than a backlog: **this rule engine
reads the object graph, and a defect that only exists in the bytes is
invisible to it.** The strict structural validator in `tinker-pdf-cos` already
walks those bytes, and joining the two is the honest way to close that group.

**`PDFA_STAGED` has 27 entries** — every rule this build knows it does not
run, each with its clause and what it is waiting for. A `staged` row in the
ledger has to point at one, so the classification is checkable rather than a
story.

**The laziness requirement is counted, not asserted.** Every reach past the
COS document goes through `Machinery::reach`, and a syntax-only validation of
a document that embeds a font program leaves the font and colour counters at
zero. The counter is not vacuous: it records the one XML parse a syntax sweep
genuinely needs, the flavour claim, so the mechanism fires on nearly every
document in the corpus. Injecting a font rule into the syntax group fails four
of the workspace's 4 446 tests.

## What the structural join measured

*Recorded 4 September 2026, tier 1 of the roadmap.* Same corpus, same
2 371-file bar.

**1 211 of 2 371 agree, against 1 201 before, with the false-positive count
unchanged at one.** The join reports the strict validator's structure tier
under ISO 19005's clauses; what it took to keep that second number still is the
part worth recording.

| | files | agreed before | agreed after |
| --- | --- | --- | --- |
| annotated `-pass-` | 831 | 830 | 830 |
| annotated `-fail-` | 1 540 | 371 | 381 |
| **total** | **2 371** | **1 201** | **1 211** |

The first draft mapped every structural defect kind it could name and scored
**1 206** with **53** false positives against the recorded one. Two kinds
accounted for 48 of the 52 new ones: `StreamDoesNotDecode`, which is a
statement about how forgiving this reader has to be rather than about what the
standard requires, and `FreeHeadMissing`, which is a *classic table's* concept
that a file with a cross-reference stream cannot satisfy and parts 2 to 4 do
not ask it to. `EntryPastSize` (two files) is 7.5.5's instruction to a reader,
and `StartxrefNotASection` fires on one `pass` file, which is not enough to
tell that file's defect from a reading of Annex F. All five are refused a
clause, each with its count and its reason in `pdfa/structure.rs`.

**Counted injections, and one of them is a zero.** Each mapping was removed in
turn and the census re-run; the unit is the bar, because these rules change a
number rather than a verdict.

| Injected | The bar, of 2 371 | False positives | The census failed |
| --- | ---: | ---: | --- |
| none (control) | 1 211 | 1 | no |
| the indirect-object mapping removed | 1 204 | 1 | no |
| the stream-extent mapping removed | 1 208 | 1 | **yes**, on a ledger row |
| the group left out of `IMPLEMENTED` | 1 201 | 1 | **yes** |
| the corpus root left at the fetch directory | **0/0** | — | **yes**, on the new assertion |
| **the cross-reference mapping removed** | **1 211** | 1 | no |

The last row is the one to read. Removing 6.1.4's mapping outright changes the
bar by nothing: not one of the 2 371 files reaches it, so a rule that ran and a
rule that did not would have measured the same, and `CONTRIBUTING.md` is
explicit that a guard which catches nothing when its defect is injected is not
a guard. `tests/pdfa_structure.rs` is what closes it — three documents built
here with exactly one thing wrong in their bytes, each with the undamaged twin
that keeps "it reported something" from passing for "it reported this", and a
fourth test proving the group stays off in a syntax-only sweep.

**The census itself was measuring nothing, and had been.** `suite_of` takes the
first path component of a file relative to the corpus root and asks whether it
names a PDF/A suite; rooted at `corpus/files`, that component is `verapdf`,
which names none — so every one of the 2 896 annotated files was excluded as a
test of some other standard, the bar printed `0/0`, and a full page of
plausible output came with it. The root now finds the corpus inside a fetch
directory, and an empty bar is an assertion rather than a printed zero. The
`1 201` above was reproduced by switching the new group off, not carried.

## What the XMP value-type rule measured

*Recorded 13 September 2026, at the commit that landed it.* Same corpus, same
2 371-file bar.

**1 476 of 2 371 agree, against 1 211 before, and the false-positive count did
not move.** That second clause is the acceptance criterion rather than the
first: this rule judges every property of every packet, `pass` files included,
so it is the first group here whose natural failure mode is reporting a
conforming file.

| | files | agreed before | agreed after |
| --- | --- | --- | --- |
| annotated `-pass-` | 831 | 830 | 830 |
| annotated `-fail-` | 1 540 | 381 | 646 |
| **total** | **2 371** | **1 211** | **1 476** |

**ISO 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 have two halves and only one of
them landed.** The clause requires every property to belong to a predefined
schema *and* to be written in the value type that schema declares. The second
needs a table both revisions agree on; the first needs the revision each part
cites, and the table this build had was Adobe's current one, which is neither.
The difference is not academic: `xmp:Advisory`, `xmpMM:LastURL`,
`xmpMM:RenditionOf`, `xmpMM:SaveID`, `exif:MakerNote`,
`exif:ComponentsConfiguration`, four `xmpDM:` properties and the whole of
`xmpidq` and the Exif `aux` namespace appear in fixtures annotated **pass** and
in none of the published tables, so reading that table as a membership list
would have reported every one of them. `PDFA_STAGED` still carries the entry,
still counts 37, and now says which half it is about.

*Amended at the commit recorded in “The predefined-schema rule, whole” below,
which landed the membership half and its extension-schema exception together.
`PDFA_STAGED` no longer carries either entry; it still counts 37, because two
narrower refusals took their place.*

*Amended at the commit recorded in “The two revisions the standards cite”
below.* The paragraph above said the revision "ISO 19005 cites" is XMP 2004,
which is true of part 1 and false of parts 2 and 3; the wording has been
corrected here and the cause is recorded there.

Per ledger row, in files still disagreeing:

| subject | before | after |
| --- | ---: | ---: |
| `Isartor test files/PDFA-1b/6.7 Metadata` | 19 | 18 |
| `PDF_A-1b/6.7 Metadata/6.7.2 Properties` | 195 | 88 |
| `PDF_A-2b/6.6 Metadata/6.6.2 Metadata streams` | 295 | 138 |
| `PDF_A-4/6.7 Metadata/6.7.2 Metadata streams` | 3 | 3 |

265 of the 512 files those four rows named now agree, and the 247 left are the
membership half. This supersedes the remainder named in *What milestones 5 and
6 actually measured* — "roughly 490 are still the XMP predefined-schema
property rule" is now 247 — and the rest of that paragraph stands.

*Amended 14 September 2026.* "The 247 left are the membership half" was an
estimate and it was wrong by a factor of four. The membership rule landed and
took **60**; what the four rows actually held was mostly the value-type half
still, at a granularity below the four serialised forms. The measurement is in
*The predefined-schema rule, whole* below, and the estimate is left standing
here because a prediction corrected is worth more than a prediction deleted.

**The defect the corpus never saw.** The walk that reads a property's value
form had the attribute shorthand for a structure — `<xmpMM:DerivedFrom
stRef:instanceID="…"/>`, which is what a real producer writes — stubbed out as
`let shorthand = false`, so a conforming structure read as a simple value. It
was a false positive rather than a miss, which is the direction that costs the
`pass` side, and the number above was measured after it was fixed rather than
before.

**Counted injections.** Each defect re-introduced in turn, the whole
`tinker-pdf` crate run with `--no-fail-fast` before `-p`, and the count
recorded — zeros included, because a guard that catches nothing when its defect
is injected is not a guard.

| Injected | Tests that failed |
| --- | ---: |
| the attribute shorthand not detected | 3 |
| every `rdf:Alt` counted as a language alternative | 3 |
| the RDF and XML attribute exclusions dropped | 3 |
| part 4 runs the rule | 2 |
| the revision-drift row ignores which part asked | 2 |
| the finding names the metadata clause instead of its own | 1 |
| the top-level match keyed to depth instead of structure | 1 |
| **the `rdf:RDF` grandparent test dropped** | **0, then 1** |
| the rule never called at all (the control) | 10 |

**The zero is the row worth reading.** Dropping the grandparent half of the
top-level test — so that anything under any `rdf:Description` is judged as a
property of the document — failed nothing at all. The reason is that a
structure's fields are reached with a property already open, so the test is
never consulted for them; the case that does consult it is a `dc:title` nested
one level too deep, which no fixture had. Writing that fixture took the row to
1. The rule is unchanged; what changed is that it is now guarded.

**Two things the corpus cannot see, stated rather than implied.**

- **Part 4's exclusion is worth nothing to the bar.** ISO 19005-4 dropped the
  requirement rather than renumbering it — the conformance suite has
  directories for it under `PDF_A-1b` and `PDF_A-2b` and none anywhere under
  `PDF_A-4` — and running the rule there anyway moves the bar by **zero**, in
  either direction. So the exclusion is a reading held by two tests and by
  nothing else, and if the reading is wrong no corpus number here would say so.
- **The `pass` side is where the risk was**, and 454 of the 831 `pass` files
  live in the two directories whose fixtures are adversarial about XMP
  serialisation. They still agree. The guard that runs on every machine, with
  no corpus at all, is `pdfa_writer.rs`: it validates a document this engine
  wrote at every flavour it can claim and asserts zero findings, against a
  packet generated from the builder's own table.

## The two revisions the standards cite

*Recorded 14 September 2026, at the commit that landed it.* Same corpus, same
2 371-file bar. **This commit ships no new rule.** It replaces one table with
the two the standards actually cite, so that the membership half has something
to be built on; the value-type half is the only rule reading them and it was
not touched.

**Which revision each part cites.**

| part | revision | table | schemas | properties |
| --- | --- | --- | ---: | ---: |
| ISO 19005-1 | *XMP Specification*, January 2004, 94 pp, pp. 37–58 | `PREDEFINED_2004` | 11 | 169 |
| ISO 19005-2, -3 | *XMP Specification*, September 2005, 112 pp, pp. 39–70 | `PREDEFINED_2005` | 14 | 274 |
| ISO 19005-4 | — | neither | — | — |

The evidence is the conformance suite, from two directions. Its fixtures state
their own expectation in words: **all 366** part-1 membership fixtures say the
property is or is not "in XMP 2004" and **all 549** part-2 ones say "in XMP
2005", with no counterexample either way. And its machine-readable profiles
bind the revision to the part by name — `PDFA-1B.xml` calls
`isPredefinedInXMP2004`, `PDFA-2B.xml` calls `isPredefinedInXMP2005`.

**The suite's own fixtures were then used to check the transcription, and they
are 421 for 422.** Every fixture message of the form *the property X, which is
(not) permitted in \<schema\> in XMP 2004/2005* is a membership claim; there
are 422 distinct ones, and 421 agree with the transcribed tables schema by
schema and name by name. The one that does not is `xmpMM:InstanceID`, which
`6-7-2-t09-fail-q` calls permitted in XMP 2004 and whose name does not occur
anywhere in the 94 pages of the January 2004 document — September 2005
introduces it, with an editorial marker its own author left in (`<< new
InstanceID stuff>>`, p45). The table is not patched to agree; the disagreement
is between two published sources and belongs in the ledger when the membership
rule lands.

**The census moved by eleven files and the false-positive count did not move.**

| | before | after |
| --- | ---: | ---: |
| the bar | 1 691/2 371 | **1 702**/2 371 |
| false positives, of 831 annotated `pass` | 1 | **1** |
| the value-type half's four ledger rows | 265/512 | **276**/512 |

The movement was not predicted and is worth reading, because the cross-check
that made this commit safe compared only the properties the old table and the
two revisions **share** — on which every single form is identical, so no file
could move for that reason, and none did. What moved is the properties they do
*not* share.

- **Thirteen files gained**, every one annotated `fail`, every one a property
  the cited revision defines and Adobe's current tables had dropped:
  `xmpMM:LastURL` and `xmpMM:RenditionOf` (under both parts), `exif:MakerNote`
  and `exif:ComponentsConfiguration`, the four `xmpDM:` modification-date and
  copyright properties, and `aux:Lens` and `aux:SerialNumber`. The old table
  could say nothing about any of them because it no longer carried the name.
- **Two files lost**, both under part 1, and each for its own good reason.
  `6-7-2-t03-fail-u` writes `photoshop:History`, which January 2004 has no such
  property in — and the fixture's own message is *"The property 'History' is
  not permitted in Photoshop Schema in XMP 2004"*, a **membership** finding. The
  old table reported that file on a value type, which was the right verdict for
  the wrong reason; the right reason is the staged half. `6-7-2-t09-fail-q` is
  the `xmpMM:InstanceID` disagreement above.

Both losses are already covered by the four ledger rows, and the census still
reports 0 uncovered disagreements and 0 stale rows.

**`REVISION_DRIFT` is deleted, and nothing replaces it.** It held exactly one
row — `photoshop:SupplementalCategories` under part 1 — a hand-written override
saying "the suite wins here", bolted beside a table that could not explain why
the suite said two different things under two parts. The specifications explain
it: `Text` on page 47 of January 2004, `bag Text` on page 55 of September 2005,
and the September 2005 changelog records the change under April 2005 in as many
words. The override is now an ordinary row in each of two ordinary tables, and
the four fixtures that pinned it still pass.

**TechNote 0008 had been generalised, and all five places are corrected.** The
note is titled *Predefined XMP Properties in **PDF/A-1***: authority for part 1
and for no other part. Read as though it said "ISO 19005" it became a claim
about parts 2 and 3 too, and that claim reached `xmp_schemas.rs`, `xmp.rs`,
`pdfa.rs`, `docs/design/pdfa.md` and `THIRDPARTY.md` — including one file that
contradicted itself, saying the revision "is XMP 2004" for both parts in one
paragraph and that the parts cite different revisions in another.

**Counted injections.** Each defect re-introduced in turn, the whole
`tinker-pdf` crate run with `--no-fail-fast` before `-p`, and the count
recorded — zeros included, because a guard that catches nothing when its defect
is injected is not a guard.

| Injected | Tests that failed |
| --- | ---: |
| the properties left unsorted | 18 |
| `Lang Alt` mapped to `Array` | 10 |
| a `bag` value type mapped to `Simple` | 6 |
| part 2 routed to the 2004 table | 5 |
| part 1 routed to the 2005 table | 3 |
| `photoshop:SupplementalCategories` given one form in both tables | 3 |
| **part 4 routed to the 2005 table** | **0, then 1** |

**The zero is the row worth reading.** Routing part 4 to a table it does not
cite failed nothing at all, because `part_carries_the_predefined_schema_rule`
decides part 4 before `value_form` is ever asked — so the `Part::Four => None`
arm was a sentence in a comment and not a claim anything held.
`the_table_router_gives_part_four_nothing` is what makes it one, and the row
above is measured before and after it was written.

## The predefined-schema rule, whole

*Recorded 14 September 2026, at the commit that landed it.* Same corpus, same
2 371-file bar. The **membership** half of ISO 19005-1 6.7.2 and ISO 19005-2
6.6.2.3.1, and the **extension-schema description** of 6.7.8 and 6.6.2.3.2 and
6.6.2.3.3, land together. They had to: membership without the exception reports
every conforming file that carries a custom property, and the exception without
membership checks the escape hatch of a rule nobody enforces.

**What the rule is.** Every top-level property of every packet belongs to a
predefined schema of the revision its part cites — `PREDEFINED_2004` for part 1,
`PREDEFINED_2005` for parts 2 and 3, neither for part 4, which dropped the
requirement — or the packet describes it in `pdfaExtension:schemas`. A property
that is neither is `XmpPropertyUndescribed`, named with its schema's preferred
prefix where one exists and with its namespace in braces where none does.

**What an extension schema must carry, settled by the fixtures rather than by
reading the clause table.** Every veraPDF fixture states its own expectation in
its outline, so each of these is a published claim:

| required | optional |
| --- | --- |
| `pdfaSchema:` `schema`, `namespaceURI`, `prefix` | `pdfaSchema:` `property`, `valueType` |
| `pdfaProperty:` `name`, `valueType`, `category`, `description` | |
| `pdfaType:` `type`, `namespaceURI`, `prefix`, `description`, `field` | |
| `pdfaField:` `name`, `valueType`, `description` | |

The two optional entries are the ones that shaped the code: `6-6-2-3-3-t05-pass-a`
carries no `pdfaSchema:property` and `t01-pass-e` carries no
`pdfaSchema:valueType`, and both are annotated **conforming**. A description
that carries neither describes no property, and the consequence — that a
property it would have described is not a member — is the membership rule's to
report.

**And the prefixes are matched as spellings.** ISO 19005 fixes
`pdfaExtension`, `pdfaSchema`, `pdfaProperty`, `pdfaType` and `pdfaField`, and
the suite pins the four inner ones from the failing side eight times: four
under part 2 (`6-6-2-3-3-t01-fail-f`, `t02-fail-e`, `t03-fail-f`, `t04-fail-d`)
and four under part 1 (`6-7-8-t04` to `t07-fail-a`). **In every one of those
eight files the wrong prefix is bound to the right namespace URI** —
`xmlns:nonpdfaSchema="http://www.aiim.org/pdfa/ns/schema#"` — so a reader that
compared resolved namespaces, which is what XML means by a name, would pass all
eight. So an element is recognised by its namespace and then its prefix is
checked, and the description is still read: a file gets one finding about the
prefix rather than that plus a cascade about entries it plainly carries.

**Part 2 has a term part 1 does not, and it is structural rather than about the
revision.** veraPDF's part-1 profile asks `isPredefinedInXMP2004 ||
isDefinedInCurrentPackage`; its part-2 profile asks `isPredefinedInXMP2005 ||
isDefinedInMainPackage || isDefinedInCurrentPackage`. Under parts 2 and 3 the
catalog's packet may therefore describe a property a **page's** packet uses.
`6-6-2-3-2-t01-pass-b` asserts exactly that and says so in its own outline —
*"The Catalog metadata defines custom property, which is used in the page
metadata"* — annotated conforming. So the membership walk reads the catalog's
packet and every page's, and carries the catalog's descriptions into a page
under parts 2 and 3 only. The value-type half still reads the catalog's packet
alone: that half was delivered and its movement counted over one packet, and
widening its input in the same commit would leave neither number attributable.

**Two exemptions, each with its evidence.**

- **The schemas ISO 19005 defines for itself.** `pdfaid` and the five
  extension vocabularies appear in no revision of the XMP specification, and
  the suite's 831 `pass` files carry **1 646** `pdfaid` properties between them
  while only **seven** of them describe any extension schema at all. A
  membership rule that judged `pdfaid` would report nearly every conforming
  file there is. Whether the packet describes `pdfaid` where the part asks it
  to is clause 6.7.11's question and `PDFA_STAGED` still carries that row.
- **`xmpMM:InstanceID` under part 1**, which is a disagreement between two
  published sources and was flagged for this commit by the one before it. The
  name does not occur in the 94 pages of the January 2004 XMP specification.
  `PDF_A-1b` `6-7-2-t09-pass-q` writes it, states in its own outline that it
  "is permitted in XMP Media Management Schema in XMP 2004", and is annotated
  **conforming**. One source has to lose, and which way the mistake falls
  settles it: reading the table strictly reports a file a conformance suite
  calls conforming, which is the one outcome this rule group is held to avoid,
  while admitting the name costs only the ability to report
  `6-7-2-t09-fail-q` — which this build cannot report anyway, because the
  value-type half needs a declared type and the document that would have
  declared one never printed the row. **The exception is membership-only and
  lives in the rule, not in the table**: a row added to `PREDEFINED_2004` would
  be a claim about a document, and this is a claim about a corpus. The fail
  file it forgives keeps its ledger row, which now names the disagreement.

**The census.**

| | before | after |
| --- | ---: | ---: |
| the bar | 1 702/2 371 | **1 762**/2 371 |
| false positives, of 831 annotated `pass` | 1 | **1** |
| ledger | 94 rows, 0 uncovered, 0 stale | **93** rows, 0 uncovered, 0 stale |

Sixty files, every one annotated `fail`, and the false-positive count did not
move — which was the acceptance criterion rather than the bar, because a rule
that reports a conforming file has failed however far it moves the total.

Per ledger row, in files still disagreeing:

| subject | before | after |
| --- | ---: | ---: |
| `Isartor test files/PDFA-1b/6.7 Metadata` | 18 | 8 |
| `PDF_A-1b/6.7 Metadata/6.7.2 Properties` | 86 | 61 |
| `PDF_A-1b/6.7 Metadata/6.7.8 Extension schemas` | 4 | **0**, row deleted |
| `PDF_A-2b/6.6 Metadata/6.6.2 Metadata streams` | 129 | 109 |
| `PDF_A-2b/6.6 Metadata/6.6.4 Version identification` | 2 | 1 |

**The estimate this row carried was wrong by a factor of four, and the reason
is worth more than the number.** The roadmap said "roughly 247 files turn on
the membership half", taken from the four ledger rows' totals. Between them,
membership and the description checks took **60**. Membership took the 25
part-1 fixtures the suite states as membership claims ("not permitted in ... in
XMP 2004", "the Camera Raw Schema is not defined in XMP 2004"), the one
6.6.2.3.2 fixture, and a version-identification fixture that binds the prefix
`pdfaid` to a namespace that is not the identification schema's; the
description checks took the 19 under `6.6.2.3.3` and the 4 under part 1's
`6.7.8`; Isartor's `6.7.8` directory gave ten, split between the two. What the
rows actually held was the value-type half still, at a granularity below the four
forms an RDF/XML serialisation distinguishes: 61 part-1 and 107 part-2 fixtures
whose property *is* in the table, *is* written as the simple value its schema
declares a simple value for, and whose text is not the kind of simple value it
declares — an `Integer` where a `Rational` is asked for, a closed choice
written outside its choices. The corpus said so all along and nobody had asked
it: **not one** of the 549 files under `6.6.2.3.1 General` states a membership
expectation, and all 549 state a value-type one.

So `PDFA_STAGED` loses two entries and gains two, and still counts 37 at this
milestone. What was staged then was named at the granularity the corpus
actually has: the value types below the four serialised forms (6.6.2.3, 168
files), and an extension schema's custom value types — whether a type a
property names is described anywhere, and whether a type with fields is used
where a simple value is declared (6.7.8, four Isartor files). The first of
those two is what "What the predefined value types measured" below closes; the
second is still staged.

**Counted injections.** Each defect re-introduced in turn, the whole
`tinker-pdf` crate run with `--no-fail-fast` before `-p`, the count recorded —
zeros included, because a guard that catches nothing when its defect is
injected is not a guard.

| Injected | Tests that failed |
| --- | ---: |
| the `pdfaid` and `pdfaExtension` exemption dropped | 127 |
| the membership rule never called at all | 7 |
| a property in a namespace no table names admitted as a member | 4 |
| the required-entry check never run | 4 |
| the extension-schema lookup never consulted | 3 |
| a described property matched by name but not by namespace | 2 |
| the `pdfa*` prefix requirement dropped | 2 |
| part 1 given part 2's main-package term | 2 |
| part 2 denied the main-package term | 2 |
| the rule applied to part 4 | 2 |
| the `xmpMM:InstanceID` exception dropped | 1 |
| the attribute shorthand for a description not read | 1 |
| page packets never read | 1 |

**No zeros, and that is the one result a campaign cannot take credit for.** The
first row is the shape of the risk rather than a guard: dropping the exemption
reports `pdfaid` on every conforming fixture in the suite, so 127 tests fail
and none of them is about this rule. The rows worth reading are the three
**ones**, each a single fixture written for it — `xmpMM:InstanceID` admitted
under part 1, the attribute shorthand for a description, and the page packets
the main-package term needs. Each was one test before the injection was run and
is one test after, which is what a guard looks like when it is the only one.

## What the predefined value types measured

*Recorded 15 September 2026, at the commit that landed it.* Same corpus, same
2 371-file bar. This closes the first of the two entries the section above left
staged: the value types below the four forms an RDF/XML serialisation
distinguishes.

**1 948 of 2 371 agree, against 1 781 before, with the false-positive count
still 1.** 167 files, every one annotated `fail`, and the 831 annotated `pass`
gained nothing to say — which is the acceptance criterion rather than the
total, because a value-type rule that is too strict reports conforming files
and there are 831 of them standing ready to catch it.

| | files | agreed before | agreed after |
| --- | --- | --- | --- |
| annotated `-pass-` | 831 | 830 | 830 |
| annotated `-fail-` | 1 540 | 951 | 1 118 |
| **total** | **2 371** | **1 781** | **1 948** |

| ledger row | before | after |
| --- | ---: | ---: |
| `PDF_A-1b/6.7 Metadata/6.7.2 Properties` | 61 | **1** |
| `PDF_A-2b/6.6 Metadata/6.6.2 Metadata streams` | 109 | **2** |

Neither row is deleted, and neither leftover is a value type. The part-1 row
is `6-7-2-t09-fail-q`, the `xmpMM:InstanceID` disagreement the section above
named — judging it would mean inventing a table row no page of the January
2004 document carries — and it is reclassified from `staged` to `reading`. The
part-2 row is `6-6-2-1-t01-fail-b` and `-fail-c`, whose own outlines say *"The
bytes attribute is used in the header of an XMP packet"* and *"The encoding
attribute…"*: a different clause, 6.6.2.1, with no rule at all, and
`PDFA_STAGED` gains an entry that says so rather than letting them shelter
under a row about value types.

**A value type is two questions and they have different inputs, which is why
the old rule could not ask the second.** `Shape` is how the value is arrayed
— `bag`, `seq`, `alt`, `Lang Alt`, or none of those — and is settled by the
element names alone. `Item` is what one value *is*, and settling it means
reading the characters against a published grammar. The table these replaced
carried one `ValueForm` per property collapsing both, with all three RDF
containers as a single `Array`, so `Integer` and `Rational` and `Date` were
indistinguishable and so were `rdf:Bag` and `rdf:Seq`.

**Splitting the containers is worth 35 fixtures on its own**, and it is the
cheaper half: `dc:creator` is `seq ProperName` and `dc:subject` is `bag Text`,
and each written as the other's container was, under one `Array`,
indistinguishable from the conforming spelling. `PDF_A-1b` `6-7-2-t06-fail-p`
and `-fail-m` are exactly that pair.

**And a language alternative stays separate from a bare `alt`, which is a
strictness rather than a leniency and so had to be measured.** A `Lang Alt`
satisfies a declared `alt`, because a language alternative *is* an alternative
array; the converse is not granted. Granting it — reading a bare `rdf:Alt` as
a `Lang Alt` — takes the bar from 1 948 to **1 944**, two files under
`PDF_A-1b/6.7 Metadata` and two under `PDF_A-2b/6.6 Metadata`, and gains no
conforming file. A comment in this tree claimed that leniency cost ten
fixtures across seven suites; it costs four across two, and the number here is
the one measured on this branch.

**Four types are read and the rest deliberately are not.** `Integer`, `Real`,
`Boolean` and `Date`, each against the lexis its own specification prints —
September 2005 pp. 74–77 for parts 2 and 3, January 2004 pp. 62–63 for part 1.
Grouping the 168 files this rule was staged for by what their value actually
violates gives 96 an integer that is not one, 35 an array of the wrong kind,
17 a date that is not one, 10 a boolean that is not one, 9 a real that is not
one, and one `xmpMM:InstanceID`. Nothing else. So `Rational`, `URI`, `URL`,
`GPSCoordinate`, `XPath`, `Locale`, `MIMEType`, `ProperName`, `AgentName`,
`RenditionClass` and a structure's own fields have no rule written for them:
**467 of the 468 `-fail-` fixtures in the two clause directories this rule
serves are already reported on**, and of the 448 `-pass-` fixtures there,
none is. A grammar written against no test is a guess, and a guess that is too
strict reports conforming files.

**The `Date` grammar is the one place a rule could have been built on half its
evidence.** September 2005 p. 75 prints six forms and the range of every
field. January 2004 p. 62 prints one sentence and defers to
`http://www.w3.org/TR/NOTE-datetime`, which lists the same six forms with the
same ranges — so one function serves both parts, and `a_date_follows_the_six_printed_forms`
runs under both to say that is a decision rather than an accident. The time
zone designator is **not optional** in the three forms that carry a time, and
that is the reading that could have cost conforming files. It does not: the
only `pass` files carrying a time with no designator are six under `PDF_A-4`
and `PDF_A-4e`, and ISO 19005-4 carries no predefined-schema requirement, so
the rule never runs there.

**The closed and open choices are read for their base type and not for their
vocabulary**, which is the same decision made once more. 51 of the 168 are
choice fixtures and **not one** writes a syntactically valid integer that is
merely outside a printed list: every one fails because the value is not an
integer at all — `2.0`, `2/5`, `Pos - 1`, `value: 3`. Reading the
vocabularies would also be reading a moving target, because the September 2005
changelog records correcting `exif:ColorSpace`'s "uncalibrated" value from
−32768 to 65535, so the two revisions print different admissible sets for one
property.

**One property's value is not read at all, and the reason is the one that
settled `xmpMM:InstanceID` a section above.** September 2005 p. 41 declares
`xmp:Rating` a `Closed Choice of Integer`; `PDF_A-2b`
`6-6-2-3-1-t07-pass-m` writes `1.0` into it, is annotated **conforming**, and
has no failing twin anywhere in the suite. Two published sources disagree and
ruling 13 settles which this build follows: where a conformance suite calls a
file conforming, this build does not report it. The exception lives at the
call site in `xmp::schema_value_types` rather than in the table, because a
patched cell would be a claim about a document that never printed it, and the
next reader checking the transcription against the page would "fix" it back.

**The transcription was read a second time by a check that does not share a
failure mode with the first.** 168 properties are printed in both documents.
**166 declare the same value type in both**; the two that differ are
`photoshop:SupplementalCategories`, whose change the September 2005 changelog
records, and `exif:GPSMeasureMode`, which January 2004 p. 57 prints as `Closed
Choice of Integer` and September 2005 p. 68 prints as `Text` — a difference
neither changelog mentions and both pages state plainly. Only the first moves
a *form*, which is why the table this replaced could carry one disagreement
and be right about the rule it ran. And collapsing every freshly read value
type back to the form the old table carried reproduces **all 443** of its
rows, with the property set unchanged: the same 443 `(namespace, name)` pairs,
neither added to nor removed from. So this moves the type column and nothing
else, which is what keeps the membership half where it was.

## What the level A group measured

*Recorded 14 September 2026, at the commit that landed it.* Same corpus, same
2 371-file bar.

**1 781 of 2 371 agree, against 1 762 before, with the false-positive count
still 1.** Level A was the last conformance level the writer claimed and the
validator said nothing about. It says four things about it now.

| | files | agreed before | agreed after |
| --- | --- | --- | --- |
| annotated `-pass-` | 831 | 830 | 830 |
| annotated `-fail-` | 1 540 | 932 | 951 |
| **total** | **2 371** | **1 762** | **1 781** |

| clause group | files | agreed before | agreed after |
| --- | --- | --- | --- |
| `PDF_A-1a/6.8 Logical structure` | 19 | 8 | 18 |
| `PDF_A-2a/6.7 Logical structure` | 16 | 6 | 15 |

**The rules, and where they live.** `RuleGroup` is a taxonomy of machinery, and
level A's machinery is a bounded walk of a `/K` graph — not the COS document
the syntax group already has, and not the second parse the strict validator
does. It is the `structure` flag's second subject, in
`crates/tinker-pdf/src/pdfa/logical.rs`, and `Coverage::SYNTAX` leaves it off
with the rest of that group. The four rules are `/MarkInfo /Marked true`
(6.8.2.2 / 6.7.2.2), a `/StructTreeRoot` (6.8.3.3 / 6.7.3.3), every element's
`/S` resolving through the `/RoleMap` to one of ISO 32000-1 14.8.4's 49
standard types (6.8.3.4 / 6.7.3.4), and every `/Lang` being a language
identifier (6.8.4 / 6.7.4).

**The tree is read by the reader that already reads it.**
`crate::structure::bind` resolves the role map, bounds a cyclic or exponential
`/K`, and reports what it tolerated. A second walk here would be a second
reader to keep in step, and the drift would surface as the validator
disagreeing with `Document::structure` about what a file's tagging says.

**Four things the fixtures decided and the clause text would not have**, each
one a rule that would otherwise report conforming files:

- **`/Lang` is not required, only well formed.** `6-8-4-t01-pass-b` carries
  `/Lang (sl)` on a `/Span` element and nothing in its catalog;
  `6-8-4-t01-pass-c` carries its only `/Lang` inside a marked-content property
  list. Both are annotated `pass`. Reading "the document shall specify its
  natural language" as "the catalog shall carry `/Lang`" costs **12** false
  positives, measured.
- **The empty string is a permitted `/Lang`.** `6-8-4-t01-pass-d` writes
  `/Lang ()` and its own outline says "its value is empty text string which is
  permitted". No grammar for a language tag admits it. Refusing it costs
  **3**.
- **The value is a text string.** `6-8-4-t01-pass-f` writes
  `/Lang <FEFF0065006E002D00470042>` — UTF-16BE for `en-GB`, annotated `pass`
  — and `6-8-4-t01-fail-c` writes the same encoding around Cyrillic. Both are
  hexadecimal strings and what separates them is only visible after 7.9.2.2's
  decoding.
- **Part 1 and parts 2–3 cite different RFCs, and neither clause says so.**
  Part 1 sends a reader to PDF Reference 9.8.1, which defines the value by
  RFC 1766, where a subtag is `1*8ALPHA`; parts 2 and 3 send a reader to ISO
  32000-1 14.9.2, which defines it by RFC 3066, where a subtag is
  `1*8(ALPHA / DIGIT)`. The suite states the difference twice: part 1 has
  `6-8-4-t01-fail-b`, `/Lang (en-12)`, "Subtag of Lang entry contains digits";
  parts 2 and 3 have **no such fixture** and instead `6-7-4-t01-pass-c`,
  `/Lang (ru-petr1708)`, annotated `pass`.

**And one the corpus settled by being read rather than by being reasoned
about.** `6-8-3-4-t02-fail-a`'s outline says "a circular mapping shall not
exist" and its role map is `<< /Document /Document /Span /Span /Standard
/Standard >>`. Written as the fixture describes it — a rule about the role map
cycling — it catches nothing, because `crate::structure` treats `/X → /X` as a
*termination* and is right to: `/P /P` is the commonest role-map entry in the
wild and calling it a loop produced 63 warnings over the fetched corpora
against a handful of real ones. Asking instead what the type resolved *to*
catches both this file and `6-8-3-4-t01-fail-a`, under the requirement they
actually break.

**The independent check is outside the suite.** Fifteen real PDF/A-1a and
PDF/A-3a documents sit in the fetched pdfjs and SafeDocs corpora, unannotated
and so invisible to the bar — one of them 444 pages and 59 737 structure
elements, of which 8 657 are typed `/Standard` and resolved through a role
map. They gain **one** level A finding between them, and that one file already
carries a malformed object header and claims level A with no `/MarkInfo` and
no `/StructTreeRoot` at all. A corpus of conformance fixtures cannot say
whether a rule survives contact with what real producers write into a role
map; those fifteen files can.

**Counted injections.** Each defect re-introduced in turn, `cargo test
--no-fail-fast -p tinker-pdf` (1 438 tests in the control, measured on this
branch on 15 September 2026), and the census run beside it. The false-positive
column is the one to read.

| Injected | tests | the bar | false positives |
| --- | ---: | ---: | ---: |
| the group never called (the control) | 15 | 1 762 | 1 |
| **the level gate removed — every level judged** | 130 | 1 561 | **763** |
| `/Marked true` read as the defect rather than as the requirement | 14 | 1 758 | **30** |
| the `/RoleMap` ignored — `/S` judged as the file wrote it | 11 | 1 774 | **16** |
| the catalog's `/Lang` made mandatory | 2 | 1 775 | **12** |
| the empty `/Lang` not permitted | 2 | 1 779 | **3** |
| the `/MarkInfo` rule never called | 6 | 1 775 | 1 |
| the catalog `/Lang` rule never called | 4 | 1 776 | 1 |
| the structure-type rule never called | 4 | 1 777 | 1 |
| the missing-`/StructTreeRoot` finding never pushed | 4 | 1 779 | 1 |
| the element-`/Lang` rule never called | 1 | 1 779 | 1 |
| digits in a subtag admitted under every part | 3 | 1 780 | 1 |
| digits in a subtag refused under every part | 3 | *1 781* | 1 |
| `ALPHA` read as "any letter" rather than as `A-Za-z` | 2 | *1 781* | 1 |
| `Annot`, `THead`, `TBody`, `TFoot` dropped from the type list | 2 | *1 781* | 1 |
| parts 2 and 3 given part 1's clause numbers | 5 | *1 781* | 1 |
| the per-document finding cap removed | 1 | *1 781* | 1 |

**The control decomposes the gain exactly**: 1 781 − 1 762 = 19, and 19 is the
number of logical-structure fixtures this commit moves.

**Three of those rows count a writer fixture**, and that is the exit criterion
made checkable rather than asserted:
`a_level_a_document_tagged_with_a_type_nobody_defines_is_reported_by_its_own_validator`
builds a Level A document with `PageBuilder::tagged(b"Chapitre", …)`, and the
validator reports it. It fails under the control, under the inverted `/Marked`
rule and under the structure-type rule never being called — so the *other*
writer fixture's zero findings at 2A are a verdict rather than a silence. It
does not fail when the `/RoleMap` is ignored or when the four ISO 32000-1
types are dropped, because `/Chapitre` and `/P` are on neither side of those
readings; those two rows are held by `pdfa_logical.rs` alone.

**Five injections move the census by nothing, and each has a fixture rather
than an excuse.** Four of the five share one cause, and it is worth naming
because it looks like a hole and is not: the corpus fixtures that would
separate those readings — `6-8-4-t01-fail-c` and `6-7-4-t01-pass-c` — put
their `/Lang` inside a **content stream**, which this group does not read. So
the RFC 1766/3066 split in *both* directions, and the ASCII reading of
`ALPHA`, are held by `pdfa_logical.rs` and `logical.rs`'s own unit tests and
by nothing else, and that is recorded here rather than discovered later. The
fifth zero is the four ISO 32000-1 structure types: no level A fixture uses
one unmapped, so `every_standard_structure_type_is_admitted_unmapped` is what
holds the wider list — and the wider list is the *permissive* direction, so
the cost of being wrong about it is only a defect not reported.

The clause-table and finding-cap rows are zeros for a different and ordinary
reason: the census scores a file by whether the verdict is empty, so neither a
clause number nor the length of the finding list is visible to it. Each is
held by the test written for it.

**Two fixtures stay staged and one clause stays untouched.**
`6-8-4-t01-fail-c` and `6-7-4-t01-fail-c` — one file per numbering — write
their only `/Lang` as `/Span <</Lang <feff0430043d002d04210410>>> BDC` in a
page's content stream. Reading that needs `pdfa/content.rs`'s walk, which is
the machinery the font and colour groups reach for; making this group a third
consumer changes what `Coverage::STRUCTURE` costs, which is a decision for the
laziness requirement above rather than for a rule. And level A's *font* rule,
6.2.11.7.3, is five more files and a harder problem: a character mapped into a
Unicode Private Use Area needs an `/ActualText` **for that character**, and two
of the five fixtures carry one for a different character in the same run.
That is the code-to-glyph mapping of every code a `Tj` drew, correlated with
the marked-content sequence it was drawn inside — the interpreter rather than
a tokenizer. Both are in `PDFA_STAGED` with those words.

## What the annotation group measured

*Recorded 13 September 2026, at the commit that landed it.* Same corpus, same
2 371-file bar.

**1 624 of 2 371 agree, against 1 476 before, with the false-positive count
still 1.** This was the clause group with no rules at all — 164 annotated
fixtures across four parts, and this build had nothing to say about any of
them. It now has nothing left to say about them for the opposite reason.

| | files | agreed before | agreed after |
| --- | --- | --- | --- |
| annotated `-pass-` | 831 | 830 | 830 |
| annotated `-fail-` | 1 540 | 646 | 794 |
| **total** | **2 371** | **1 476** | **1 624** |

**Eighteen ledger rows went stale in one run, and five of them were not
annotation rows.** `PDFA_STAGED`'s 6.9 entry said the interactive-form
fixtures were waiting on "the appearance streams the annotation group will
bring, and there is no annotation group". They were, and it did: the Isartor
forms row and four `PDF_A-1b/6.9` files stopped disagreeing without a line
being written for them. What is left of that entry is one requirement the
forms clause makes and this group does not — `6-4-1-t01-fail-a` says in its
own words that a widget annotation dictionary contains the `/A` key — and it
belongs to the forms ledger class rather than this one.

**The staged entry was wrong about what the clause needs, and the corpus is
what said so.** It read: "the appearance rules need the annotation appearance
machinery and the colour rules need an output intent, and neither is in this
group." The second half was right and is why four fixtures live in the colour
group. The first was not: across 164 fixtures the suite never once asks what
an appearance *draws* — only whether one exists, whether the `/AP` carries
anything besides `/N`, and whether `/N` is a stream or the sub-dictionary of
states a push-button needs. All of that is in the COS document, which is why
the group reaches for nothing and rides the syntax group's own walk.

**Four exemptions this build would not have guessed**, each taken from a
fixture the suite annotates `pass` rather than from the clause text:

- a `Popup` needs no `/F` at all, stated twice — once under part 2 and again
  under part 4;
- a `Popup`, a `Link` and part 4's `Projection` need no appearance;
- an annotation whose `/Rect` is a **point** needs none either, while one that
  is merely zero-width does. `6-3-3-t01-pass-a` writes `[50 110 50 110]` and
  passes; `6-3-3-t01-fail-p` writes `[50 600 50 50]` — zero wide and 550 tall
  — and fails. "No area" is the reading that looks right and passes a file the
  clause fails;
- a push-button widget's `/N` is a sub-dictionary of states, and the `/FT`
  that says so **may be inherited**. Reading it off the widget alone reported
  `6-4-1-t01-pass-b`, a conforming radio group whose kids carry the states and
  whose parent carries the `/FT`.

**And one number that was simply wrong.** ToggleNoView is bit **9** of ISO
32000-1 table 165, not bit 10. `6-3-2-t02-fail-e` writes `/F 268` — bits 3, 4
and 9 — and a rule reading bit 10 had nothing to say about a file the suite
annotates fail.

**Counted injections.** Each defect re-introduced in turn, the whole
`tinker-pdf` crate run with `--no-fail-fast` before `-p`, and the census run
beside it, because most of these move a number rather than a verdict. The
false-positive count is the column to read.

| Injected | tests | the bar | false positives |
| --- | ---: | ---: | ---: |
| an annotation matched by its `/Rect` rather than by `/Type /Annot` | 1 | 1 624 | 1 |
| a `Popup` not exempt from `/F` | 2 | 1 622 | **3** |
| `Link` and `Popup` not exempt from an appearance | 2 | 1 620 | **5** |
| ToggleNoView read as bit 10 | 2 | 1 623 | 1 |
| the `/Rect` exemption read as no area rather than a point | 1 | 1 623 | 1 |
| `/FT` read off the widget and not inherited | 1 | 1 623 | **2** |
| part 4's levels permitting nothing extra | 2 | 1 617 | **9** |
| ISO 32000-2's types admitted under every part | 2 | 1 623 | 1 |
| the annotation colour rule run on every part | 1 | 1 623 | **2** |
| the group never called (the control) | 17 | 1 480 | 1 |

**The control decomposes the gain exactly.** With `annotations::rules`
unreachable the bar is 1 480 rather than 1 476, which is the four Isartor `/C`
and `/IC` fixtures the *colour* group answers. So of the 148 files this
commit moves, 144 are the annotation group and 4 are the colour rule beside
it.

**Five of the ten injections cost false positives rather than coverage**, and
that is the shape this group was expected to have: an annotation is the most
ordinary thing a real document carries, so a rule that is too strict reports
conforming files by the dozen. Removing part 4's level permissions costs nine
of them at once.

**The first row is the one with no corpus behind it.** Matching an annotation
by its `/Rect` instead of by `/Type /Annot` changes the bar by nothing and the
false-positive count by nothing — no file in the corpus has a non-annotation
dictionary that would be caught. It is held by a test and by nothing else, and
that is recorded here rather than discovered later.

**What stays staged, and why it is not a backlog.** Two requirements in the
clause have no fixture anywhere under any part: a `/Popup`'s `/Parent`
back-reference, and the `/AS` that says which state a sub-dictionary appearance
is showing. A rule for either would be this build's reading of the clause text
held to nothing, which is the thing `PDFA_STAGED` exists to say out loud. The
third staged row is new and is a measurement: an annotation's own `/C` and
`/IC` are judged against the output intent under part 1, where four Isartor
fixtures test it, and **not** outside it — running it on part 4 anyway reported
`6-3-3-t01-pass-d`, a `Projection` carrying `/C [1 0 0]` in a file with no
output intent at all, annotated pass.

## What milestones 5 and 6 actually measured

*Recorded August 2026, at the commit that landed the writer profile.* Same
corpus, same 2 371-file bar, same rule: numbers rather than adjectives.

**1 201 of 2 371 agree, against 1 017 before.** The false-positive count did
not move, and that is the number worth watching rather than the total: a rule
group that raised agreement by reporting conforming files would have raised it
for nothing.

| | files | agreed before | agreed after |
| --- | --- | --- | --- |
| annotated `-pass-` | 831 | 830 | 830 |
| annotated `-fail-` | 1 540 | 187 | 371 |
| **total** | **2 371** | **1 017** | **1 201** |

Per rule group, over the files that are tests of that group's clauses:

| clause group | files | agreed before | agreed after |
| --- | --- | --- | --- |
| fonts (6.3, 6.2.11, 6.2.10) | 200 | 64 | 103 |
| colour (6.2.2–6.2.4, 6.2.9, 6.4) | 449 | 134 | 278 |

**The corpus taught four clauses that no reading of the text would have
given.** Each cost a run and each is now a test with a counted injection:

- **"used for rendering" is the clause's own qualifier and it is
  load-bearing.** The first font group judged every `/Type /Font` dictionary
  in the file and reported 53 conforming files: an interactive form's `/DR`
  names three of the standard 14 that nothing draws with, and two fixtures
  whose own titles say *"the text rendering mode is 3"* are annotated `pass`
  because mode 3 paints nothing. The group now finds out what is drawn before
  it judges anything.
- **A Type 1 font is exempt from the Unicode rule and a TrueType font is
  not**, because a Type 1 program is keyed by glyph *name* and a name is a
  route to Unicode through the Adobe Glyph List.
- **A composite font's character collection is a route to Unicode too.**
  Three fixtures whose titles read *"…uses the Adobe-Korea1 character
  collection does not include a ToUnicode entry"* are annotated `pass`, and
  the reason is that Adobe publishes a `…-UCS2` mapping for each published
  collection.
- **A transparency group's blending space stands in for a device colour
  space**, exactly as `/DefaultRGB` does, and **part 4 admits an output intent
  on a page**. Between them those two accounted for fifteen conforming files
  the first colour group reported.

**The sharpest staged rule is 6.3.8, and the corpus is what staged it.**
`6-3-8-t01-fail-b.pdf` and `6-3-8-t01-pass-e.pdf` carry **byte-identical font
dictionaries** — the same two Type 1 fonts, the same `/Encoding` dictionary,
no `/ToUnicode` on either — and opposite annotations. Whatever separates them
is not in a font dictionary, so no rule over font dictionaries can find it: it
needs the glyph list as vendored data and the glyphs the content stream drew.
The half that *is* in the dictionary runs; the half that is not is a named
refusal.

**`PDFA_STAGED` has 37 entries**, against 27 before. That number went *up*
while coverage went up, and it should have: milestone 5 replaced two vague
entries — "graphics: colour spaces, output intents, transparency, rendering
intents" and "fonts: embedding, widths, symbolic flags, Unicode mapping" —
with eleven specific ones that each name a rule and why it is not running. A
staged list that shrinks as rules land and never grows is a list nobody is
reading carefully. *It stands at 39 today: the predefined value types closed
the 6.6.2.3 entry down to the types no fixture exercises and added one at
6.6.2.1 for the packet header's `bytes` and `encoding` attributes, which the
row it used to shelter under no longer covers — see "What the predefined value
types measured" above.*

**Where the 1 169 unagreed `fail` files are.** Roughly 490 are still the XMP
predefined-schema property rule. About 250 are in graphics clauses this build
now runs and does not run *far enough*: the profile's own conformance, the
Separation tint transform, the image and XObject prohibitions filed under the
graphics clause, and parts 2-to-4's transparency constraints. About 90 are
font clauses whose remaining half needs the code-to-glyph mapping — metrics,
`/CharSet`, the `.notdef` glyph. The annotation clauses (6.3.1 to 6.3.3 in
parts 2 to 4) are about 120 and have no group at all; they are milestone 5's
neighbour rather than milestone 5. *They have one now — see "What the
annotation group measured" above, which closed them and took five interactive
form rows with them.*

**The writer.** `DocumentBuilder::archival` produces documents that this
build's validator finds nothing wrong with at every flavour it can claim —
1B, 2B, 2U, 2A, 3B, 4, 4E, 4F — and that the strict structural validator in
`tinker-pdf-cos` also passes. Both gates run on every fixture, and the second
one matters more than it looks: it reads the *bytes* rather than the object
graph and was written for the writer rather than for PDF/A, so it is the one
judgement here that the conformance rules did not also write.

## The limit this milestone narrows and does not close

The risk table below has always said the writer is checked by the rule table
that would also accept its mistakes. Milestones 5 and 6 sharpened that
sentence rather than removing it, and it is worth saying precisely what is
left.

Every near-miss twin in `pdfa_writer.rs` is a second document differing from a
conforming one by one call, asserted to *fail*. That shows the rule
**discriminates** — it is not passing everything — which is strictly more than
"the fixture passed". What it does not show is that the rule discriminates on
the right axis: a twin fails against the same table the fixture passed
against, so a clause this build reads wrongly is read wrongly in both
directions and the pair agrees with itself. Five counted injections say the
same thing from the other side: undoing the packet, the output intent, the
header version, the transparency refusal or the standard-14 refusal each turns
the suite red, so each is load-bearing — but a defect *nobody wrote a rule
for* is invisible to all of it.

The corpus is the one place that asymmetry breaks, and only for files this
engine did not write: 2 371 documents somebody else made, with somebody else's
verdict attached, where a rule read wrongly shows up as a disagreement rather
than as a matched pair. That is why the census number and not the writer suite
is the honest measure of how much of ISO 19005 this build understands — and
why the writer's own conformance is reported as *"our validator, and the
structural one, find nothing"* rather than as *"it conforms"*.

## `WriteOptions` is not done, and that is a scope decision

The scope list at the top of this document puts the profile on
`DocumentBuilder` **and** `WriteOptions`, so that a rewrite of an existing
document could be held to the same standard. Only the builder has it.

The reason is structural rather than a shortage of will. `rewrite` returns
`Vec<u8>`, so a `WriteOptions` field asking for archival output has nowhere to
put a refusal: it would have to be discovered by the validator afterwards,
which is the exact failure the builder's design exists to avoid. Making it
honest means `rewrite` returning a `Result`, which every caller of the
serializer would have to be changed for. That is a writer change rather than a
PDF/A one and it belongs in [features/writing.md](../features/writing.md)'s
own work.

What the builder does do is set the write options it needs — the header
version follows the part it claims — so a profiled document is written
correctly even though `WriteOptions` cannot be *asked* for one.

## Dependencies

- **[design/icc.md](icc.md)** — ICC profile parsing for colour rules that
  inspect profile internals, and validation of the writer's destination
  profile.

  *Amended at milestone 5.* That design is **done** through its milestone 7,
  so the profile fields the colour rules need — the data colour space
  signature, and through it the channel count — are read rather than staged,
  and 6.2.3.3 and 6.2.3.2 both run. The staging moved somewhere more specific
  and is named in `PDFA_STAGED` under 6.2.2: `icc::Profile::parse` is a
  *transform builder*, and it refuses profiles that conform to ICC.1 and that
  it cannot render — a v4 profile whose only route to the connection space is
  an `mAB ` tag, for one. Refusing to render is right; refusing to conform is
  not. So a destination profile this build cannot read leaves the output
  intent's colour space **unknown**, the rules that depend on knowing it do
  not fire in either direction, and a test asserts that silence.
- **[design/tagged-pdf.md](tagged-pdf.md)** — **Done, and used.** Level A is
  claimable by the writer, which tags, and the validator's level A rules read
  the tree through that design's own reader (`crate::structure::bind`) rather
  than walking `/StructTreeRoot` a second time. What it does *not* yet give is
  a `/RoleMap` on the writer's side, so a custom tag at level A is a file this
  writer emits and this validator reports — asserted, with its twin, in
  `pdfa_writer.rs`.
- **`tinker-pdf-xml`** — exists, one of ruling 8's ten leaf crates;
  already a facade dependency, with the cos-side amendment on
  `xmp_metadata` deciding where the parse happens.
- **Corpus machinery** — `corpus/corpora.lock` pins the veraPDF corpus;
  `xtask` `corpus-run`/`ratchet.rs` provide the ratchet the agreement rate
  rides on. Exists.
- **[features/writing.md](../features/writing.md)** — the writer whose
  options and builder this extends, and the strict structural validator
  beside it.

## Risks

| Risk | Mitigation |
| --- | --- |
| ISO 19005 has hundreds of sub-clauses; "validates PDF/A" overclaims what any first delivery checks | Coverage is a measured number, not a word: the ratchet row records agreement per clause group, staged rules are named refusals with tests, and docs state the rate rather than the ambition (the injection discipline in [verification.md](../verification.md)) |
| **Nothing outside this repository ever validates a file this engine wrote** (ruling 13), so the writer is checked by the rule table that would also accept its mistakes | Every built fixture has a near-miss twin that must fail, and five counted injections show each thing the profile does is load-bearing. Both show the rules *discriminate*; neither shows they discriminate on the right axis, because a twin fails against the same table its fixture passed. [The limit this milestone narrows and does not close](#the-limit-this-milestone-narrows-and-does-not-close) says so at length, and [verification.md](../verification.md) names it too. **Not closed** |
| XMP is a graph serialisation; a pull parser yields tokens, not the graph (the `xmp_metadata` amendment's own warning) | Parse only the property shapes 19005 checks, in the facade, behind fixtures taken from real producers' packets; a packet the subset cannot read is a finding ("metadata not checkable"), not a pass |
| No shippable ICC profile licence for the writer's default output intent | **Realised, and taken.** The ICC's own sRGB profiles carry a permission notice with no SPDX identifier, so no vendored profile can declare one `deny.toml` allows. `ArchivalProfile::destination_profile` is mandatory with no default and the type's own documentation says why, matching the no-bundled-faces precedent |
| Writer profile refusals drift from validator rules, so the builder emits what the validator rejects | One rule table serves both: builder refusals cite the same `Clause` values, and a round-trip test validates every built fixture with the full validator in the same suite |
