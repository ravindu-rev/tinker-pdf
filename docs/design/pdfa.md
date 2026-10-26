# PDF/A validation and writing

The engine answers "does this file conform to PDF/A, and if not, which clause
did it break?" with typed findings a test can assert on; the veraPDF corpus is
its conformance bar, every disagreement with the corpus's annotations a ledger
row with a reason; and `DocumentBuilder::archival` takes a profile and either
produces a file this engine's own validator accepts with zero findings, or
refuses at build time with an error naming the clause it could not satisfy.

## Scope

- **A validator**: a rule engine over the opened `CosDocument`, covering
  ISO 19005 parts 1 through 4, keyed by conformance level (A/B for part 1;
  A/B/U for parts 2 and 3; the e/f variants of part 4). Verdicts are typed
  findings naming the 19005 clause and the object that broke it, in the
  shape ruling 10 ([rulings.md](../rulings.md)) gives warnings
  (`Warning { offset, object, kind }` in
  `crates/tinker-pdf-cos/src/warn.rs`).
- **Flavour detection**: reading the claimed `pdfaid:part` /
  `pdfaid:conformance` from the XMP packet (ISO 32000-1 14.3.2), so a caller can ask
  "validate against what this file claims" or name a flavour explicitly.
- **The bar**: the veraPDF *corpus*, whose expected verdict is annotated in
  each file's name (clause, test number, `pass`/`fail`). The annotations are
  published data and admissible under ruling 13; the tool that produced them
  is not invoked.
- **A writer profile** on `DocumentBuilder`
  (`crates/tinker-pdf-cos/src/build.rs`): `/OutputIntents` with an embedded
  ICC destination profile (ISO 32000-1 14.11.5), the XMP conformance packet,
  every font embedded, and every forbidden feature refused at build time
  rather than discovered at validation time. `WriteOptions` carries no
  profile (see below).
- **Surfacing**: `Document::validate_pdfa` on the facade, and a `--pdfa`
  mode on `tpdf check`.

## Non-goals

- **PDF/UA.** Same corpus, different standard: [design/pdfua.md](pdfua.md).
  Level A of parts 1 to 3, which requires tagged structure, is this design's:
  the writer claims it by tagging, with an untagged page and a missing natural
  language both refused at `finish_archival`, and the validator's structure
  group carries its rules.
- **PDF/X and PDF/E.** PDF/X has its own design ([pdfx.md](pdfx.md)); PDF/E is
  [ROADMAP](../ROADMAP.md) row SD-13.
- **Conversion.** No "fix this file into PDF/A" repair mode. Validation
  reports; building conforms; nothing rewrites an arbitrary document's
  colour or fonts into compliance. Conversion with fix-ups is
  [ROADMAP](../ROADMAP.md) row SD-02.
- **A general RDF store.** XMP is RDF/XML, and the doc comment on
  `xmp_metadata` in `crates/tinker-pdf-cos/src/outline.rs` records why
  `tinker-pdf-cos` does not parse it. The validator reads exactly the XMP
  shapes 19005 checks — not arbitrary RDF graphs. An XMP model with schemas,
  editing and repair is [ROADMAP](../ROADMAP.md) row SD-23.
- **Bundled typefaces.** The no-bundled-faces policy
  ([THIRDPARTY.md](../../THIRDPARTY.md)) stands: the writer profile
  requires embedding, therefore the caller must supply face bytes, and the
  API says so instead of shipping a font. Fallback faces and embedding
  missing fonts on write are [ROADMAP](../ROADMAP.md) rows FT-01, FT-02 and
  FT-12.

## Design

**Where it lives.** The rule engine is in the facade,
`crates/tinker-pdf/src/pdfa.rs` and `pdfa/`. This is a placement argument,
not a convenience: the checks group by the machinery they need — syntax checks
need only the COS document, font checks need `tinker-pdf-font`, colour
checks need `tinker-pdf-color`, XMP checks need `tinker-pdf-xml` — and the
facade is the one crate that depends on all four
(`crates/tinker-pdf/Cargo.toml`). Putting XMP parsing in `tinker-pdf-cos`
would add the cos→xml edge the `xmp_metadata` doc comment deliberately
refuses; putting the engine anywhere lower splits it. `xmp_metadata` keeps
its bytes-out contract, and the facade parses those bytes through the
`tinker-pdf-xml` `Reader`/`Event` pull API into the properties 19005's
metadata clauses examine.

**Types.** A `Flavour { part, level }` names what is being validated
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
reviewable change to what "validates" means. A verdict is the flavour, the
findings and the `Coverage` that ran; pass is an empty list from complete
coverage, and there is no boolean that discards the list. Clause identifiers
cite the ISO 19005 part's own numbering (part 1 groups as 6.1 file structure,
6.2 graphics, 6.3 fonts, and so on) — findings name the clause, and where the
same defect maps to different clauses across parts, the mapping table is
data, not duplicated rules.

**Rule groups, by machinery.** Rules register in a static table keyed by
flavour applicability, each declaring which `RuleGroup` it belongs to, and
`Coverage` says which ran:

1. **Syntax** (largest count, least machinery): header and trailer shape,
   no `/Encrypt`, file identifier present, forbidden filters (LZWDecode in
   part 1 — ISO 32000-1 7.4 names the filter set), forbidden actions,
   embedded-file rules per part (forbidden in 1, PDF/A attachments only in 2,
   anything in 3), no XFA, and the annotation rules. These read the
   `CosDocument` through its resolution API and nothing else.
2. **Metadata**: the XMP packet — well-formedness, the packet header,
   `pdfaid` agreement with the claimed flavour, `/Info` consistency with XMP
   where the part requires it, and the predefined-schema rule.
3. **Structure**: the strict structural validator's structure tier,
   reported under ISO 19005's clauses, and level A's logical-structure rules.
4. **Fonts**: every font used by a content stream has an embedded program
   (ISO 32000-1 9.9), `/Widths` consistent with the program, symbolic-flag
   and encoding constraints, the Unicode mapping, the encoding CMap. These
   reuse `tinker-pdf-font` to parse the embedded program — the same leaf the
   renderer trusts — never a second parser.
5. **Colour**: an `/OutputIntents` entry with subtype `GTS_PDFA1` and an ICC
   `DestOutputProfile` (ISO 32000-1 14.11.5); device colour spaces only
   under a matching output intent; ICC profile validity, read through
   `tinker-pdf-color` ([design/icc.md](icc.md)); part 1's prohibition on
   transparency.

The font and colour groups are visitors over one content walk,
`pdfa/content.rs`, so "used for rendering" means what the pages, their forms
and their annotations' appearances draw ([features/pdfa.md](../features/pdfa.md)).

Machinery is built lazily per group, so a syntax-only sweep never parses a
font program it does not need — and **the laziness is counted, not
asserted.** Every reach past the COS document goes through `Machinery::reach`,
and a syntax-only validation of a document that embeds a font program leaves
the font and colour counters at zero. The counter is not vacuous: it records
the one XML parse a syntax sweep genuinely needs, the flavour claim. The
validator runs on untrusted input, so ruling 1 binds it: every rule returns
findings or nothing, never panics, and the hostile-input sweep
(`crates/tinker-pdf/tests/hostile_input.rs`) calls the validator whole and
with the syntax and metadata groups alone, so mutated fixtures exercise the
rules on every commit.

**The bar is the corpus's own annotations.** The veraPDF corpus is
unusual, and this whole design leans on why: it is *atomic* (one clause per
file) and *annotated* (the expected verdict is in the filename). Those
annotations are a published statement about what each file is, made by the
people who wrote the conformance suite — data, not a program — so ruling 13
admits them. `crates/tinker-pdf/tests/pdfa_ledger.rs` runs the validator over
the files that are tests **of PDF/A** — the `PDF_A-*` suites and the Isartor
files, the original PDF/A-1b conformance suite; the corpus's PDF/UA, TWG and
ISO 32000 fixtures make no PDF/A claim, and scoring them would measure the
measurement — compares verdict to annotation, and prints the agreement per
clause group. It asserts the bar is not empty, because a census rooted in the
wrong directory excludes every file and prints `0/0`, and it runs nightly in
`corpus.yml`.

Each disagreement is classified and recorded rather than averaged away, in
`crates/tinker-pdf/tests/pdfa_ledger.tsv`: our bug (fix), a known staged rule
(named in `PDFA_STAGED`), or an annotation this project reads differently
(recorded with the file, the clause, and the reading — the shape the bounds
ledger uses for a justification field). Every row carries a mandatory reason,
and a row without one is refused by the reader. The census asserts coverage
in both directions: a disagreement with no row fails, and a row whose subject
no longer disagrees fails as stale. `PDFA_STAGED` names every rule this build
knows it does not run, each with its clause and what it is waiting for, and a
`staged` row has to point at one, so the classification is checkable rather
than a story.

**What is lost, and it is real.** A second validator's verdict on a file
*this engine wrote* has no substitute here. The annotations cover the
corpus's files, not ours, so the writer profile is checked by the same rule
table that would accept its mistakes. Fixtures built at the edge of each
clause, each with a near-miss twin, narrow that; they do not close it.

**The writer profile.** `DocumentBuilder::archival` sets the profile at
construction. Under it:

- **Fonts**: only embedded fonts are accepted. `add_base_font` (standard
  14, no program) is refused under the profile with a typed error saying
  why — the caller supplies face bytes exactly as `FontProvider`
  (`crates/tinker-pdf/src/fonts.rs`) demands for rendering, and the
  subsetter the builder runs at `finish` serves unchanged.
- **Output intent**: the builder writes `/OutputIntents` with a
  caller-supplied ICC profile. **The parameter is mandatory and there is no
  vendored default.**

  The reasoning is the licence and nothing else. `cargo xtask vendor`
  requires every vendored tree to declare an SPDX identifier `deny.toml`
  allows. The ICC's own sRGB profiles — the ones every other producer
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
- **Metadata**: the XMP packet with `pdfaid:*` is generated at `finish` from
  the same table that writes `/Info`, byte-deterministic like the rest of the
  writer (ruling 4; `pdfa_writer.rs` asserts it).
- **Refusals**: every buildable-but-nonconforming request is refused at the
  call that makes it, as a typed `ArchivalRefusal` naming its clause, in the
  builder's refused-at-the-door style — nothing is discovered by the
  validator that the builder allowed. [features/pdfa.md](../features/pdfa.md)
  tables them.

The loop closes end to end within this repository: files the profile writes
are validated by our own validator and by the strict structural validator
([render-verification](render-verification.md)'s sibling for the writer).
`DocumentBuilder::archival` produces documents this build's validator finds
nothing wrong with at every flavour it can claim — 1B, 2B, 2U, 2A, 3B, 4, 4E,
4F — and that the strict structural validator in `tinker-pdf-cos` also
passes; `pdfa_writer.rs` runs both gates on every fixture. The second matters
more than it looks: it reads the *bytes* rather than the object graph and was
written for the writer rather than for PDF/A, so it is the one judgement here
that the conformance rules did not also write. Under ruling 13 there is no
outside verdict on these files, which is the limit named above.

## Milestones

1. Validator core: flavour types, `ConformanceFinding`, rule table, XMP
   flavour detection, `Document::validate_pdfa`, `tpdf check --pdfa`.
2. The syntax rule group, part 1's clauses first, then parts 2–4's deltas.
3. The XMP rule group: packet well-formedness, `pdfaid` agreement, `/Info`
   consistency.
4. The disagreement ledger against the corpus annotations.
5. The font and colour rule groups.
6. The writer profile on `DocumentBuilder`: output intent, XMP generation,
   typed refusals.

## The structure tier

**This rule engine reads the object graph, and a defect that exists only in
the bytes is invisible to it.** `pdfa/structure.rs` closes the readable half
of that family by reporting the strict structural validator's structure tier
under ISO 19005's clauses: the cross-reference table's per-section spelling
under 6.1.4, an indirect object's framing under 6.1.8, and `/Length` against
where `endstream` is under 6.1.7. It is the `structure` flag's first subject,
off in `Coverage::SYNTAX`, because it parses the file a second time with the
leniency ladder off.

Refused a clause, each with its reason in the module: the repairs, which are
statements about this reader rather than about the file; linearization, which
ISO 19005 does not require; the header and the trailer, which the syntax group
already judges, since one defect reported twice under one clause is worse than
once; and five kinds the corpus annotations argue against.
`StreamDoesNotDecode` is a statement about how forgiving this reader has to
be rather than about what the standard requires. `FreeHeadMissing` and
`FreeHeadGeneration` are a *classic table's* free-list head, which a file with
a cross-reference stream cannot satisfy and parts 2 to 4 do not ask it to.
`EntryPastSize` is 7.5.5's instruction to a reader. `StartxrefNotASection`
fires on one `pass` file, which is not enough to tell that file's defect from
a reading of Annex F.

Staged: hexadecimal string syntax and the EOL markers around `obj` and
`stream`, which the lexer and the parser normalise away before an object
exists to apply a rule to; implementation limits, under their own row.

`tests/pdfa_structure.rs` builds documents with exactly one thing wrong in
their bytes, each with the undamaged twin that keeps "it reported something"
from passing for "it reported this", and a test that the group stays off in a
syntax-only sweep.

## The predefined-schema rule

ISO 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 require every property to belong to
a predefined schema *and* to be written in the value type that schema
declares, and 6.7.8 and 6.6.2.3.2–6.6.2.3.3 let a packet describe its own
properties in an extension schema. Both halves and the exception are in the
metadata group: `pdfa/xmp.rs`, `pdfa/xmp_extension.rs`, and the tables in
`pdfa/xmp_schemas.rs`.

**The two revisions the standards cite.**

| part | revision | table |
| --- | --- | --- |
| ISO 19005-1 | *XMP Specification*, January 2004, 94 pp, pp. 37–58 | `PREDEFINED_2004` |
| ISO 19005-2, -3 | *XMP Specification*, September 2005, 112 pp, pp. 39–70 | `PREDEFINED_2005` |
| ISO 19005-4 | — | neither |

The evidence is the conformance suite, from two directions. Its fixtures state
their own expectation in words — part 1's membership fixtures say the property
is or is not "in XMP 2004", part 2's say "in XMP 2005". And its
machine-readable profiles bind the revision to the part by name —
`PDFA-1B.xml` calls `isPredefinedInXMP2004`, `PDFA-2B.xml` calls
`isPredefinedInXMP2005`. TechNote 0008 is titled *Predefined XMP Properties in
**PDF/A-1***: authority for part 1 and for no other part. Where the revisions
differ the tables differ: `photoshop:SupplementalCategories` is `Text` on
page 47 of January 2004 and `bag Text` on page 55 of September 2005, a change
the September 2005 changelog records; `exif:GPSMeasureMode` is `Closed Choice
of Integer` on January 2004 p. 57 and `Text` on September 2005 p. 68. ISO
19005-4 dropped the requirement rather than renumbering it, so neither half
runs under part 4, and `the_table_router_gives_part_four_nothing` holds the
router to that.

**Membership.** Every top-level property of every packet — the catalog's and
every page's — belongs to a predefined schema of the revision its part cites,
or the packet describes it in `pdfaExtension:schemas`. A property that is
neither is `XmpPropertyUndescribed`, named with its schema's preferred prefix
where one exists and with its namespace in braces where none does.

**Part 2 has a term part 1 does not, and it is structural rather than about
the revision.** veraPDF's part-1 profile asks `isPredefinedInXMP2004 ||
isDefinedInCurrentPackage`; its part-2 profile asks `isPredefinedInXMP2005 ||
isDefinedInMainPackage || isDefinedInCurrentPackage`. Under parts 2 and 3 the
catalog's packet may therefore describe a property a **page's** packet uses.
`6-6-2-3-2-t01-pass-b` asserts exactly that and says so in its own outline —
*"The Catalog metadata defines custom property, which is used in the page
metadata"* — annotated conforming. So the membership walk carries the
catalog's descriptions into a page under parts 2 and 3 only.

**What an extension schema must carry, settled by the fixtures rather than by
reading the clause table.** Every veraPDF fixture states its own expectation
in its outline, so each of these is a published claim:

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
the suite pins the four inner ones from the failing side under both parts
(`6-6-2-3-3-t01-fail-f`, `t02-fail-e`, `t03-fail-f`, `t04-fail-d`; and
`6-7-8-t04` to `t07-fail-a`). **In every one of those files the wrong prefix is
bound to the right namespace URI** —
`xmlns:nonpdfaSchema="http://www.aiim.org/pdfa/ns/schema#"` — so a reader that
compared resolved namespaces, which is what XML means by a name, would pass
them all. So an element is recognised by its namespace and then its prefix is
checked, and the description is still read: a file gets one finding about the
prefix rather than that plus a cascade about entries it plainly carries.

**Two exemptions, each with its evidence.**

- **The schemas ISO 19005 defines for itself.** `pdfaid` and the five
  extension vocabularies appear in no revision of the XMP specification, and
  nearly every conforming file in the suite carries `pdfaid` properties while
  few describe any extension schema at all. A membership rule that judged
  `pdfaid` would report nearly every conforming file there is. Whether the
  packet describes `pdfaid` where the part asks it to is clause 6.7.11's
  question, and `PDFA_STAGED` carries that row.
- **`xmpMM:InstanceID` under part 1**, a disagreement between two published
  sources. The name does not occur in the 94 pages of the January 2004 XMP
  specification; September 2005 introduces it. `PDF_A-1b`
  `6-7-2-t09-pass-q` writes it, states in its own outline that it "is
  permitted in XMP Media Management Schema in XMP 2004", and is annotated
  **conforming**. One source has to lose, and which way the mistake falls
  settles it: reading the table strictly reports a file a conformance suite
  calls conforming, which is the one outcome this rule group is held to avoid,
  while admitting the name costs only the ability to report
  `6-7-2-t09-fail-q` — which this build cannot report anyway, because the
  value-type half needs a declared type and the document that would have
  declared one never printed the row. **The exception is membership-only and
  lives in the rule, not in the table**: a row added to `PREDEFINED_2004` would
  be a claim about a document, and this is a claim about a corpus. The fail
  file is a `reading` row in the ledger.

**A value type is two questions and they have different inputs.** `Shape` is
how the value is arrayed — `bag`, `seq`, `alt`, `Lang Alt`, or none of those —
and is settled by the element names alone, so `rdf:Bag`, `rdf:Seq` and
`rdf:Alt` are told apart: `dc:creator` is `seq ProperName` and `dc:subject` is
`bag Text`, and `PDF_A-1b` `6-7-2-t06-fail-p` and `-fail-m` write each as the
other's container. A `Lang Alt` satisfies a declared `alt`, because a language
alternative *is* an alternative array; the converse is not granted, so an
alternative array with no language on its items, where a `Lang Alt` is
declared, is a finding. `Item` is what one value *is*, and settling it means
reading the characters against a published grammar.

**Four types are read and the rest deliberately are not.** `Integer`, `Real`,
`Boolean` and `Date`, each against the lexis its own specification prints —
September 2005 pp. 74–77 for parts 2 and 3, January 2004 pp. 62–63 for part 1.
`Rational`, `URI`, `URL`, `GPSCoordinate`, `XPath`, `Locale`, `MIMEType`,
`ProperName`, `AgentName`, `RenditionClass` and a structure's own fields have
no rule written for them, and `PDFA_STAGED` names them under 6.6.2.3: no
fixture exercises them, a grammar written against no test is a guess, and a
guess that is too strict reports conforming files.

**The `Date` grammar serves both parts.** September 2005 p. 75 prints six
forms and the range of every field. January 2004 p. 62 prints one sentence and
defers to `http://www.w3.org/TR/NOTE-datetime`, which lists the same six forms
with the same ranges — so one function serves both, and
`a_date_follows_the_six_printed_forms` runs under both to say that is a
decision rather than an accident. The time zone designator is **not optional**
in the three forms that carry a time; the `pass` fixtures that write a time
with no designator are under part 4, where the rule does not run.

**The closed and open choices are read for their base type and not for their
vocabulary.** The choice fixtures fail because the value is not an integer at
all — `2.0`, `2/5`, `Pos - 1`, `value: 3` — not because a valid integer falls
outside a printed list. Reading the vocabularies would also be reading a
moving target: the September 2005 changelog records correcting
`exif:ColorSpace`'s "uncalibrated" value from −32768 to 65535, so the two
revisions print different admissible sets for one property.

**One property's value is not read at all.** September 2005 p. 41 declares
`xmp:Rating` a `Closed Choice of Integer`; `PDF_A-2b`
`6-6-2-3-1-t07-pass-m` writes `1.0` into it, is annotated **conforming**, and
has no failing twin anywhere in the suite. Two published sources disagree and
ruling 13 settles which this build follows: where a conformance suite calls a
file conforming, this build does not report it. The exception lives at the
call site in `xmp::schema_value_types` rather than in the table, because a
patched cell would be a claim about a document that never printed it, and the
next reader checking the transcription against the page would "fix" it back.

**What is read, and where.** The attribute shorthand for a structure —
`<xmpMM:DerivedFrom stRef:instanceID="…"/>`, which is what a real producer
writes — is read as a structure rather than a simple value. Membership reads
the catalog's packet and every page's; the value-type half reads the catalog's
packet alone. A metadata stream anywhere else — a font's, an image's, a form
XObject's — is a packet this group does not open, and `PDFA_STAGED` names the
6.6.2.1 header rule's gap there. Also staged, under 6.7.8: an extension
schema's custom value types — whether a type a property names is described
anywhere, and whether a type with fields is used where a simple value is
declared.

**The guard that runs with no corpus at all** is `pdfa_writer.rs`: it
validates a document this engine wrote at every flavour it can claim and
asserts zero findings, against a packet generated from the builder's own
table.

## Level A

**The rules, and where they live.** `RuleGroup` is a taxonomy of machinery,
and level A's machinery is a bounded walk of a `/K` graph — not the COS
document the syntax group has, and not the second parse the strict validator
does. It is the `structure` flag's second subject, in
`crates/tinker-pdf/src/pdfa/logical.rs`, and `Coverage::SYNTAX` leaves it off
with the rest of that group. The four rules are `/MarkInfo /Marked true`
(6.8.2.2 / 6.7.2.2), a `/StructTreeRoot` (6.8.3.3 / 6.7.3.3), every element's
`/S` resolving through the `/RoleMap` to one of ISO 32000-1 14.8.4's 49
standard types (6.8.3.4 / 6.7.3.4), and every `/Lang` being a language
identifier (6.8.4 / 6.7.4). They run only for a file that claims level A: a
level B file is not required to be tagged.

**The tree is read by the reader that already reads it.**
`crate::structure::bind` resolves the role map, bounds a cyclic or exponential
`/K`, and reports what it tolerated. A second walk here would be a second
reader to keep in step, and the drift would surface as the validator
disagreeing with `Document::structure` about what a file's tagging says. The
standard type list, `STANDARD_STRUCTURE_TYPES`, lives in the writer's crate,
so the writer's `/RoleMap` and this rule agree on what "standard" means.

**Four things the fixtures decided and the clause text would not have**, each
one a rule that would otherwise report conforming files:

- **`/Lang` is not required, only well formed.** `6-8-4-t01-pass-b` carries
  `/Lang (sl)` on a `/Span` element and nothing in its catalog;
  `6-8-4-t01-pass-c` carries its only `/Lang` inside a marked-content property
  list. Both are annotated `pass`.
- **The empty string is a permitted `/Lang`.** `6-8-4-t01-pass-d` writes
  `/Lang ()` and its own outline says "its value is empty text string which is
  permitted". No grammar for a language tag admits it.
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

**A circular mapping is caught by what the type resolves to.**
`6-8-3-4-t02-fail-a`'s outline says "a circular mapping shall not exist" and
its role map is `<< /Document /Document /Span /Span /Standard /Standard >>`.
`crate::structure` treats `/X → /X` as a *termination*, and is right to:
`/P /P` is the commonest role-map entry in the wild. Asking what the type
resolved *to* catches both this file and `6-8-3-4-t01-fail-a`, under the
requirement they actually break.

**The writer's fixture makes its own zero a verdict.**
`a_level_a_document_tagged_with_a_type_nobody_defines_is_reported_by_its_own_validator`
builds a Level A document with `PageBuilder::tagged(b"Chapitre", …)`, and the
validator reports it; its twin maps `/Chapitre` to `/Sect` with
`DocumentBuilder::map_role`, and nothing reports that. Readings no corpus
fixture separates are held by tests written for them: the RFC 1766/3066 split
in both directions and the ASCII reading of `ALPHA` by `pdfa_logical.rs` and
`logical.rs`'s own unit tests, and `Annot`, `THead`, `TBody` and `TFoot`
admitted unmapped by `every_standard_structure_type_is_admitted_unmapped`.

**Two fixtures stay staged and one clause stays untouched.**
`6-8-4-t01-fail-c` and `6-7-4-t01-fail-c` — one file per numbering — write
their only `/Lang` as `/Span <</Lang <feff0430043d002d04210410>>> BDC` in a
page's content stream. Reading that needs `pdfa/content.rs`'s walk, which is
the machinery the font and colour groups reach for; making this group a third
consumer changes what `Coverage::STRUCTURE` costs, which is a decision for the
laziness requirement rather than for a rule. And level A's *font* rule,
6.2.11.7.3, is a harder problem: a character mapped into a Unicode Private
Use Area needs an `/ActualText` **for that character**, and fixtures carry
one for a different character in the same run. That is the code-to-glyph
mapping of every code a `Tj` drew, correlated with the marked-content
sequence it was drawn inside — the interpreter rather than a tokenizer. Both
are in `PDFA_STAGED` with those words.

## The annotation group

**The annotation rules need no machinery.** Across the suite's annotation
fixtures the suite never once asks what an appearance *draws* — only whether
one exists, whether the `/AP` carries anything besides `/N`, and whether `/N`
is a stream or the sub-dictionary of states a push-button needs. All of that
is in the COS document, which is why the group reaches for nothing and rides
the syntax group's own walk. An annotation's own `/C` and `/IC` are judged
against the output intent by the colour group, under part 1 only:
`6-3-3-t01-pass-d`, a part 4 `Projection` carrying `/C [1 0 0]` in a file
with no output intent at all, is annotated pass, and `PDFA_STAGED` names the
gap outside part 1.

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
  that says so **may be inherited**: `6-4-1-t01-pass-b` is a conforming radio
  group whose kids carry the states and whose parent carries the `/FT`.

**ToggleNoView is bit 9** of ISO 32000-1 table 165, not bit 10.
`6-3-2-t02-fail-e` writes `/F 268` — bits 3, 4 and 9 — and the suite
annotates it fail.

**What stays staged, and why it is not a backlog.** Two requirements in the
clause have no fixture anywhere under any part: a `/Popup`'s `/Parent`
back-reference, and the `/AS` that says which state a sub-dictionary
appearance is showing. A rule for either would be this build's reading of the
clause text held to nothing, which is the thing `PDFA_STAGED` exists to say
out loud. A widget annotation's `/A` in parts 2 to 4 (`6-4-1-t01-fail-a`) is
a forms requirement rather than an annotation one, and is staged under 6.9.

## The font and colour groups

**The corpus taught four readings that no reading of the text would have
given**, each held by a test:

- **"used for rendering" is the clause's own qualifier and it is
  load-bearing.** An interactive form's `/DR` names standard-14 fonts that
  nothing draws with, and fixtures whose own titles say *"the text rendering
  mode is 3"* are annotated `pass` because mode 3 paints nothing. The group
  finds out what is drawn before it judges anything.
- **A Type 1 font is exempt from the Unicode rule and a TrueType font is
  not**, because a Type 1 program is keyed by glyph *name* and a name is a
  route to Unicode through the Adobe Glyph List.
- **A composite font's character collection is a route to Unicode too.**
  Fixtures whose titles read *"…uses the Adobe-Korea1 character collection
  does not include a ToUnicode entry"* are annotated `pass`, and the reason is
  that Adobe publishes a `…-UCS2` mapping for each published collection.
- **A transparency group's blending space stands in for a device colour
  space**, exactly as `/DefaultRGB` does, and **part 4 admits an output intent
  on a page**.

**The sharpest staged rule is 6.3.8, and the corpus is what staged it.**
`6-3-8-t01-fail-b.pdf` and `6-3-8-t01-pass-e.pdf` carry **byte-identical font
dictionaries** — the same two Type 1 fonts, the same `/Encoding` dictionary,
no `/ToUnicode` on either — and opposite annotations. Whatever separates them
is not in a font dictionary, so no rule over font dictionaries can find it: it
needs the glyph list as vendored data and the glyphs the content stream drew.
The half that *is* in the dictionary runs; the half that is not is a named
refusal.

`PDFA_STAGED` names the rest of what these groups do not run, among it the
destination profile's own conformance past its header (6.2.2), the image and
XObject prohibitions (6.2.4, 6.2.5), the extended graphics state beyond
transparency (6.2.8), parts 2-to-4's transparency constraints (6.4), font
metrics for a Type 1 program (6.3.6) and the `.notdef` glyph (6.3.9).

## The limit the twins narrow and do not close

The writer is checked by the rule table that would also accept its mistakes,
and the near-miss twins sharpen that sentence rather than removing it.

Every near-miss twin in `pdfa_writer.rs` is a second document differing from a
conforming one by one call, asserted to *fail*. That shows the rule
**discriminates** — it is not passing everything — which is strictly more than
"the fixture passed". What it does not show is that the rule discriminates on
the right axis: a twin fails against the same table the fixture passed
against, so a clause this build reads wrongly is read wrongly in both
directions and the pair agrees with itself — and a defect *nobody wrote a rule
for* is invisible to all of it.

The corpus is the one place that asymmetry breaks, and only for files this
engine did not write: documents somebody else made, with somebody else's
verdict attached, where a rule read wrongly shows up as a disagreement rather
than as a matched pair. That is why the census and not the writer suite is the
honest measure of how much of ISO 19005 this build understands — and why the
writer's own conformance is reported as *"our validator, and the structural
one, find nothing"* rather than as *"it conforms"*.

## `WriteOptions` carries no profile

The profile is on `DocumentBuilder` alone, so a rewrite of an existing
document cannot be held to it. The reason is structural. `rewrite` returns
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

- **[design/icc.md](icc.md)** — ICC profile parsing for the colour rules that
  inspect profile internals. The fields they need — the version, the device
  class and the data colour space, and through it the channel count — are read
  from the header, and 6.2.3.3 and 6.2.3.2 both run. What is staged is named
  in `PDFA_STAGED` under 6.2.2: the profile's own conformance past its header.
  `icc::Profile::parse` is a *transform builder*, and it refuses profiles that
  conform to ICC.1 and that it cannot render — a v4 profile whose only route
  to the connection space is an `mAB ` tag, for one. Refusing to render is
  right; refusing to conform is not. So a destination profile this build
  cannot read leaves the output intent's colour space **unknown**, the rules
  that depend on knowing it do not fire in either direction, and a test
  asserts that silence.
- **[design/tagged-pdf.md](tagged-pdf.md)** — Level A is claimable by the
  writer, which tags, and the validator's level A rules read the tree through
  that design's own reader (`crate::structure::bind`) rather than walking
  `/StructTreeRoot` a second time.
- **`tinker-pdf-xml`** — one of ruling 8's leaf crates, a facade dependency,
  with the doc comment on `xmp_metadata` deciding where the parse happens.
- **Corpus machinery** — `corpus/corpora.lock` pins the veraPDF corpus, and
  `corpus.yml` runs the census nightly.
- **[features/writing.md](../features/writing.md)** — the writer whose
  builder this extends, and the strict structural validator beside it.

## Risks

| Risk | Mitigation |
| --- | --- |
| ISO 19005 has hundreds of sub-clauses; "validates PDF/A" overclaims what any rule engine checks | Coverage is stated, not implied: `PdfACoverage` says which groups ran, staged rules are named refusals in `PDFA_STAGED` with tests, the census prints agreement per clause group, and every disagreement is a ledger row with a reason |
| **Nothing outside this repository ever validates a file this engine wrote** (ruling 13), so the writer is checked by the rule table that would also accept its mistakes | Every built fixture has a near-miss twin that must fail. Twins show the rules *discriminate*; they do not show they discriminate on the right axis, because a twin fails against the same table its fixture passed. [The limit the twins narrow and do not close](#the-limit-the-twins-narrow-and-do-not-close) says so at length. **Not closed** |
| XMP is a graph serialisation; a pull parser yields tokens, not the graph (the `xmp_metadata` doc comment's own warning) | Parse only the property shapes 19005 checks, in the facade, behind fixtures taken from real producers' packets; a packet the subset cannot read is a finding (`MetadataUnreadable`), not a pass |
| No shippable ICC profile licence for the writer's default output intent | The ICC's own sRGB profiles carry a permission notice with no SPDX identifier, so no vendored profile can declare one `deny.toml` allows. `ArchivalProfile::destination_profile` is mandatory with no default and the type's own documentation says why, matching the no-bundled-faces precedent |
| Writer profile refusals drift from validator rules, so the builder emits what the validator rejects | Builder refusals cite their ISO 19005 clause (`ArchivalRefusal::clause`), and `pdfa_writer.rs` validates every built fixture with the full validator in the same suite |
