# PDF/A validation and writing

When this is done, the engine answers "does this file conform to PDF/A, and
if not, which clause did it break?" with typed findings a test can assert on,
the 2 907-file veraPDF corpus — today purely a never-crash bar
([verification.md](../verification.md)) — graduates to a conformance bar whose
agreement rate is ratcheted like every other corpus number, and
`DocumentBuilder` gains an archival profile that either produces a file this
engine's own validator accepts with zero findings, or refuses at build time
with an error naming the clause it could not satisfy. The [roadmap](../ROADMAP.md) Tier 3 item this
implements says exactly that: the corpus "graduates to a conformance bar."

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
  and belongs to [design/tagged-pdf.md](tagged-pdf.md). Level **A** of parts
  1–3 also requires tagged structure, so Level A verdicts are staged behind
  that design too; this one delivers B and U honestly rather than A wrongly.
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
  caller-supplied ICC profile, or a vendored sRGB profile if one clears
  the [THIRDPARTY.md](../../THIRDPARTY.md) gate — vendored data must carry
  its licence, appear in that file's table, and pass `cargo xtask vendor`
  against the `deny.toml` allowlist, the same path the Adobe CMaps and the
  UCD took. If no profile with a shippable licence exists, the parameter
  is mandatory and the doc says so; a licence problem must not become an
  API surprise.
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
| 5 | Font and colour rule groups (colour rules needing profile internals staged behind [design/icc.md](icc.md)) | Font- and colour-clause corpus agreement rates recorded and ratcheted; staged colour rules are named refusals asserted by a test, not silent passes | L |
| 6 | Writer profile on `DocumentBuilder` + `WriteOptions`, output intent, XMP generation, typed refusals | Built fixtures pass milestone 1–5's validator with zero findings and the strict structural validator clean; each fixture is built deliberately at the edge of its clause and its near-miss twin is asserted to fail; one refusal test per forbidden feature; a PDF/A fixture joins the determinism byte-hashes | L |

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
of the workspace's 3 375 tests.

## Dependencies

- **[design/icc.md](icc.md)** — ICC profile parsing for colour rules that
  inspect profile internals, and validation of the writer's destination
  profile. Milestones 1–4 and 6 do not block on it; milestone 5 stages
  behind it.
- **[design/tagged-pdf.md](tagged-pdf.md)** — Level A verdicts for parts
  1–3 need the structure tree; Level A stays "not yet claimable" until it
  lands.
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
| **Nothing outside this repository ever validates a file this engine wrote** (ruling 13), so the writer is checked by the rule table that would also accept its mistakes | Every built fixture has a near-miss twin that must fail, so the rule is shown to discriminate rather than merely to pass; the limit is named in this doc and in [verification.md](../verification.md). Not closed |
| XMP is a graph serialisation; a pull parser yields tokens, not the graph (the `xmp_metadata` amendment's own warning) | Parse only the property shapes 19005 checks, in the facade, behind fixtures taken from real producers' packets; a packet the subset cannot read is a finding ("metadata not checkable"), not a pass |
| No shippable ICC profile licence for the writer's default output intent | The THIRDPARTY.md vendor gate decides before the API does: if no profile clears `cargo xtask vendor`, the profile parameter is mandatory and documented, matching the no-bundled-faces precedent |
| Writer profile refusals drift from validator rules, so the builder emits what the validator rejects | One rule table serves both: builder refusals cite the same `Clause` values, and a round-trip test validates every built fixture with the full validator in the same suite |
