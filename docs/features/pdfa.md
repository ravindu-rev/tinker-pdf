# PDF/A

Two halves of one standard. **Reading**: `Document::validate_pdfa` answers
"which PDF/A does this file claim to be, and which clauses of that flavour does
it break" with typed findings naming the ISO 19005 clause and the object.
**Writing**: `DocumentBuilder::archival` takes a profile and from then on
refuses what that profile forbids, at the call that would have written it.

Neither half has a boolean. A verdict is a list of findings plus a
`PdfACoverage` saying which rule groups ran, because a validator that cannot
say what it did *not* check is a validator whose empty answer cannot be read.
A refusal is a typed `ArchivalRefusal` carrying its clause, because `false`
alone is a caller told no and not told why.

## What it does

**The flavour comes from the metadata.** ISO 19005 puts the claim in an XMP
packet — `pdfaid:part`, `pdfaid:conformance` for parts 1 to 3, `pdfaid:rev`
for part 4 — and nothing else in a PDF says which flavour it is meant to be.
Both RDF spellings are read, the attribute form and the element form, and the
namespace is checked: PDF/UA declares `pdfuaid:part`, an entirely different
standard's version number, and matching on the local name alone read 434
corpus files as claiming a PDF/A part they say nothing about.

**Five rule groups, keyed by the machinery they need.** Five and not six
because the annotation rules need no machinery at all — a subtype name, a flag
word, the shape of an `/AP` and four numbers in a `/Rect` are all in the COS
document — so they ride the syntax group rather than counting a reach that
never happens.

- **Syntax** needs only the opened `CosDocument`: the header and its binary
  comment, the trailer's `/ID`, encryption, external streams, forbidden
  filters and actions, embedded files, `/Perms`, optional content, XFA,
  part 4's constraints on `/Info` and the catalog's `/Version`, and **every
  annotation** — the subtypes each part admits, the `/F` flag word, `/CA`, and
  the presence and shape of an appearance dictionary.
- **Metadata** needs the XMP pull parser: the packet's well-formedness; the
  eight `/Info` entries ISO 19005-1 6.7.3 pairs with an XMP property, for
  part 1, compared as instants where they are dates; and both halves of the
  predefined-schema rule. **Membership** — every top-level property belongs to
  a predefined schema, or the packet describes it in an extension schema of
  its own, in the `pdfaExtension` markup ISO 19005-1 6.7.8 and ISO 19005-2
  6.6.2.3.2 define, with the entries and the five fixed `pdfa*` prefixes ISO
  19005-2 6.6.2.3.3 requires. **Value type** — a simple value, an array, a
  language alternative or a structure, read from the serialisation's own shape
  rather than from its text. Both read **the revision the part cites**: part 1
  the January 2004 XMP specification, parts 2 and 3 the September 2005 one.
  Parts 1 to 3 carry the requirement and **part 4 dropped it**, so neither half
  runs there. Under parts 2 and 3 the catalog's packet may describe a property
  a page's packet uses; under part 1 it may not.
- **Structure** needs the file read a second time with the leniency ladder
  off, and the logical structure tree: the cross-reference table's per-section
  spelling, an indirect object's framing and a stream's extent under 6.1.4,
  6.1.8 and 6.1.7, and **everything conformance level A adds** — `/MarkInfo
  /Marked true`, a `/StructTreeRoot`, every structure element's `/S` resolving
  through the `/RoleMap` to one of the 49 standard types, and every `/Lang` in
  the catalog or on an element being a language identifier the part's own
  reference specification defines. The level A rules run **only for a file
  that claimed level A**, because nothing below it requires any of this.
- **Fonts** needs `tinker-pdf-font` and the content walk: every font embedded
  including the standard 14, the program's format against the key that names
  it, the subset tag's shape, symbolic and non-symbolic `/Encoding`,
  `/CIDSystemInfo` and `/CIDToGIDMap`, and the determinable half of the
  Unicode rule.
- **Colour** needs `tinker-pdf-color`'s ICC reader and the same walk: the
  output intent's shape, device colour spaces against the destination
  profile's own colour space, an `ICCBased` stream's `/N` against its
  profile's channel count, the four rendering intents, and part 1's outright
  prohibition on transparency.

**Machinery is built lazily, and it is counted rather than asserted.** Every
reach past the COS document goes through one counter, so a syntax-only sweep
of a document that embeds a font program leaves the font and colour counters
at zero — and the counter is not vacuous, because it records the one XML parse
a syntax sweep genuinely needs, the flavour claim.

**"Used for rendering" is enforced, not assumed.** Nearly every clause in
ISO 19005 6.3 and 6.2.3.3 opens with that qualifier, and it decides answers: a
form's `/DR` names standard-14 fonts nothing draws with, and a font drawn only
at text rendering mode 3 paints nothing. `pdfa/content.rs` walks the page
contents, the form XObjects they invoke and the appearance streams their
annotations carry, tracking the rendering mode and the selected font through
`q` and `Q`, and the font and colour groups are visitors over it.

**The writer refuses at the call.** Under an `ArchivalProfile`,
`add_base_font` and `add_named_font` return false because the standard 14 have
no embedded program; a transparent `/ExtGState` or a form's `/Group` is
refused under part 1; a device colour the destination profile cannot reproduce
is refused on the page and on an image; and `set_info` refuses an entry part 4
has no room for, or a date the generated packet could not restate. At `finish`
the document gains an output intent naming an embedded ICC profile, an XMP
packet generated from the same table that wrote `/Info`, and the `/Lang` level
A asks for — and the header version follows the part.

## API

```rust
use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, DeviceSpace, Document,
    DocumentBuilder, PdfACoverage,
};

// Reading.
let verdict = Document::open(bytes)?.validate_pdfa();
if verdict.findings.is_empty() && verdict.coverage.is_complete() {
    // Both halves, always: an empty list from a partial validation means
    // "nothing that ran was broken".
}
// The cheap sweep, which parses no font program and no ICC profile.
let syntax = document.validate_pdfa_with(PdfACoverage::SYNTAX);

// Writing.
let mut builder = DocumentBuilder::archival(ArchivalProfile {
    part: ArchivalPart::Two,
    level: Some(ArchivalLevel::B),
    destination_profile: my_icc_bytes,     // mandatory: see THIRDPARTY.md
    destination_space: DeviceSpace::Rgb,
    output_condition: "sRGB IEC61966-2.1".to_string(),
    language: None,                        // required at level A
});
assert!(builder.add_embedded_font(b"F1", b"Face", &program));
let bytes = builder.finish_archival()?;
```

`tpdf check --pdfa` runs the same validator over files on the command line and
exits by verdict.

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| `add_base_font` / `add_named_font` under a profile | `ArchivalRefusal::UnembeddedFont` | ISO 19005 has no standard-14 exception: a file whose appearance depends on a face the reader happens to own is what the standard exists to prevent | 6.3.4 |
| A transparent `/ExtGState` or a form `/Group` under part 1 | `ArchivalRefusal::Transparency` | part 1 admits no transparency at all, so the alpha, the blend mode, the soft mask and the group are each refused | 6.4 |
| A device colour the destination profile cannot reproduce | `ArchivalRefusal::DeviceColour` | `DeviceRGB` under a CMYK output intent is a colour nobody can reproduce; grey is admitted under any, being a value on the neutral axis | 6.2.3.3 |
| An `/Info` entry other than `/ModDate`, under part 4 | `ArchivalRefusal::InfoEntry` | ISO 19005-4 leaves the document information dictionary one entry | 6.1.3 |
| An `/Info` date this crate's own parser cannot read | `ArchivalRefusal::InfoEntry` | 6.7.3 requires the dictionary and the packet to agree, and a date nobody can parse has nothing to agree with | 6.7.3 |
| A conformance level the part does not define, or none where the part requires one | `ArchivalRefusal::LevelNotInPart`, `LevelMissing` | part 4 is the only part where declaring no level is correct | 6.7.11 |
| Level A with an untagged page or no natural language | `ArchivalRefusal::UntaggedPage`, `LanguageMissing` | level A *is* a tagged structure tree with a stated language; claiming it without one would be the claim this profile exists to make honest | 6.8.2, 6.8.4 |
| A profile with no destination profile bytes | `ArchivalRefusal::DestinationProfileMissing` | there is no vendored default and no `Option`; the licence decision is in [THIRDPARTY.md](../../THIRDPARTY.md) | 6.2.2 |

## Coverage, as a measured number

Against the veraPDF corpus's own annotations, over the 2 371 files that are
tests **of PDF/A** — the other 525 are PDF/UA, TWG and ISO 32000 fixtures that
make no PDF/A claim, and scoring them would measure the measurement:

| | files | agree |
| --- | --- | --- |
| annotated `-pass-` | 831 | 830 |
| annotated `-fail-` | 1 540 | 951 |
| **total** | **2 371** | **1 781** |

The single disagreement on the `pass` side is a **reading**, recorded as one:
ISO 19005-1 6.1.2 says the header consists of `%PDF-1.n`, one fixture carries
`%PDF-2.0` and is annotated `pass`, and this build reads the clause literally.

Every disagreement has a row in `crates/tinker-pdf/tests/pdfa_ledger.tsv`
carrying a class — our bug, a staged rule, or a reading — and a **mandatory
reason**; a row without one is refused by the reader that loads the file, and a
row whose subject no longer disagrees fails as stale. `PDFA_STAGED` names 38
rules this build knows it does not run, each with its clause and what it is
waiting for, and a `staged` ledger row has to point at one.

The level A rules moved that total from 1 762 to 1 781 and left `830 of 831`
untouched, which is the number that matters for them: a structure tree this
build cannot follow reads exactly like a file with none, so a level A rule
group that raised the bar by reporting conforming files would have raised it
for nothing. The independent check is outside the suite — fifteen real
PDF/A-1a and PDF/A-3a documents in the fetched pdfjs and SafeDocs corpora,
unannotated and so invisible to the bar, gain exactly **one** level A finding
between them, and that one file already carries a malformed object header and
claims level A with no `/MarkInfo` and no `/StructTreeRoot` at all.

## Verified

`crates/tinker-pdf/tests/` carries one file per rule group, each built on the
same discipline: a conforming baseline, one change per test, exactly one
finding of exactly one kind asserted by kind, and a **near-miss twin** that
must not fire. `pdfa_syntax.rs`, `pdfa_fonts.rs`, `pdfa_colour.rs`,
`pdfa_metadata.rs`, `pdfa_annotations.rs`, `pdfa_structure.rs`,
`pdfa_logical.rs` and `pdfa_flavour.rs` are the reading half;
`pdfa_writer.rs` is the writing half and judges every fixture twice — by the
full validator with complete coverage, and by the strict structural validator
in `tinker-pdf-cos`, which reads bytes rather than the object graph and was
written for the writer rather than for PDF/A.

The predefined-schema rule is the one whose failure mode is reporting a
*conforming* file — it judges every property of every packet rather than
waiting to be reached — so every one of its fixtures has a twin that must stay
silent. The membership half is sharper still, because it reports a property
rather than a spelling: its twins are a packet whose extension schema
describes the property, a packet under the part whose revision defines it, and
the schemas ISO 19005 defines for itself, which every conforming file carries
and no XMP revision names.

The level A rules are the second such group, and two of their twins are not a
spelling at all but a **level**: the same broken bytes claiming `B`, and the
same claiming part 4, are silent, because a rule that ran below level A would
report every untagged PDF/A-1b in existence. The others are the pairs the
corpus itself draws — a custom structure type with the `/RoleMap` entry that
explains it against the same type without one, an identity `/RoleMap` entry on
a standard type against one on a custom type, `/Lang ()` and
`/Lang <FEFF0065006E002D00470042>` against the same hex encoding around
Cyrillic, and `en-12` reported under part 1 and admitted under parts 2 and 3.

`pdfa_ledger.rs` is the census, which walks the fetched corpus and asserts in
both directions that the ledger accounts for every disagreement.

## What is lost, and it is real

**Nothing outside this repository ever validates a file this engine wrote**
(ruling 13, [rulings](../rulings.md)). The corpus annotations are a published
statement about *other people's* files; there is no equivalent for ours. So
the writer is checked by the same rule table that would also accept its
mistakes, and the near-miss twins narrow that without closing it: a twin fails
against the table its fixture passed, so a clause this build reads wrongly is
read wrongly in both directions and the pair agrees with itself.

The corpus is where that asymmetry breaks, and only for files somebody else
made. It is why "1 781 of 2 371" is the honest measure of how much of ISO
19005 this build understands, and why a document the writer produces is
reported as *"this validator and the structural one find nothing"* rather than
as *"it conforms"*.

One limit closed and one still open, both worth naming rather than
discovering:

- **A defect that exists only in the bytes is invisible to a rule engine that
  reads the object graph** — and the readable half of that family is closed.
  `PdfACoverage::structure` runs the strict structural validator and reports
  its **structure tier** under ISO 19005's own clauses: the cross-reference
  table's per-section spelling under 6.1.4, an indirect object's framing under
  6.1.8, and `/Length` against where `endstream` actually is under 6.1.7. It is
  a group of its own, and off in `PdfACoverage::SYNTAX`, because it parses the
  file a second time with the leniency ladder off — which is machinery a
  syntax-only sweep must not build.

  What it deliberately does **not** report is as much the point. A repair is a
  statement about this reader rather than about the file. ISO 19005 requires no
  linearization at all. The header and the trailer already have rules in the
  syntax group, and one defect reported twice under one clause is worse than
  once. And five kinds more were mapped and then unmapped **against the
  corpus**: `StreamDoesNotDecode` and `FreeHeadMissing` between them accounted
  for 48 of the 52 false positives the first draft added, and a join that
  closes by adding false positives has closed nothing. The bar moved from
  1 201 to 1 211 with the false-positive count unchanged at one; the XMP
  value-type rule took it to 1 476 on the same terms and the annotation group
  to 1 624.

  What is still staged here is what nothing in the tree reads: hexadecimal
  string syntax, and the EOL markers around `obj` and `stream`. Implementation
  limits are staged for a different reason and keep their own row.
- **Level A is validated now, in the same group and short of two files.** The
  writer claimed level A by tagging and the validator said nothing about it;
  it says four things about it now, under 6.8.2.2 / 6.7.2.2, 6.8.3.3 / 6.7.3.3,
  6.8.3.4 / 6.7.3.4 and 6.8.4 / 6.7.4, and 19 of the 21 logical-structure
  fixtures the corpus carries agree.

  The two that do not are one file per numbering and one reason:
  `6-8-4-t01-fail-c` and `6-7-4-t01-fail-c` write their only `/Lang` inside a
  marked-content property list in a page's content stream, and reading that
  needs `pdfa/content.rs`'s walk — the font and colour groups' machinery,
  which this group is deliberately not a third consumer of. `PDFA_STAGED`
  carries the refusal.

  Level A's own font rule, 6.2.11.7.3, is staged for a harder reason and is
  five more files: a character mapped into a Unicode Private Use Area needs an
  `/ActualText` **for that character**, and two of the five fixtures carry one
  for a *different* character in the same run. Deciding that is the
  code-to-glyph mapping of every code a `Tj` drew, correlated with the
  marked-content sequence it was drawn inside, which is the interpreter rather
  than a tokenizer.

The design and its milestones are in [design/pdfa.md](../design/pdfa.md).
