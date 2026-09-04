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

  The *validator* side of Level A is a different matter and is still staged:
  reading a structure tree and deciding whether it is a correct one is
  `PDFA_STAGED`'s 6.7 entry, so a Level A file this build reports nothing
  about has had its tagging looked at by nobody. Writing A and validating A
  are two claims and only the first has landed.
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
of the workspace's 4 403 tests.

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
reading carefully.

**Where the 1 169 unagreed `fail` files are.** Roughly 490 are still the XMP
predefined-schema property rule. About 250 are in graphics clauses this build
now runs and does not run *far enough*: the profile's own conformance, the
Separation tint transform, the image and XObject prohibitions filed under the
graphics clause, and parts 2-to-4's transparency constraints. About 90 are
font clauses whose remaining half needs the code-to-glyph mapping — metrics,
`/CharSet`, the `.notdef` glyph. The annotation clauses (6.3.1 to 6.3.3 in
parts 2 to 4) are about 120 and have no group at all; they are milestone 5's
neighbour rather than milestone 5.

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
- **[design/tagged-pdf.md](tagged-pdf.md)** — **Done.** Level A is claimable
  by the writer, which tags; the validator's structure-tree rules stay staged
  (`PDFA_STAGED`, 6.7). See the amended non-goal above for why those are two
  different claims.
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
