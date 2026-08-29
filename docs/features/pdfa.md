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

**Four rule groups, keyed by the machinery they need.**

- **Syntax** needs only the opened `CosDocument`: the header and its binary
  comment, the trailer's `/ID`, encryption, external streams, forbidden
  filters and actions, embedded files, `/Perms`, optional content, XFA, and
  part 4's constraints on `/Info` and the catalog's `/Version`.
- **Metadata** needs the XMP pull parser: the packet's well-formedness and,
  for part 1, the eight `/Info` entries ISO 19005-1 6.7.3 pairs with an XMP
  property, compared as instants where they are dates.
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
| annotated `-fail-` | 1 540 | 371 |
| **total** | **2 371** | **1 201** |

The single disagreement on the `pass` side is a **reading**, recorded as one:
ISO 19005-1 6.1.2 says the header consists of `%PDF-1.n`, one fixture carries
`%PDF-2.0` and is annotated `pass`, and this build reads the clause literally.

Every disagreement has a row in `crates/tinker-pdf/tests/pdfa_ledger.tsv`
carrying a class — our bug, a staged rule, or a reading — and a **mandatory
reason**; a row without one is refused by the reader that loads the file, and a
row whose subject no longer disagrees fails as stale. `PDFA_STAGED` names 37
rules this build knows it does not run, each with its clause and what it is
waiting for, and a `staged` ledger row has to point at one.

## Verified

`crates/tinker-pdf/tests/` carries one file per rule group, each built on the
same discipline: a conforming baseline, one change per test, exactly one
finding of exactly one kind asserted by kind, and a **near-miss twin** that
must not fire. `pdfa_syntax.rs`, `pdfa_fonts.rs`, `pdfa_colour.rs`,
`pdfa_metadata.rs` and `pdfa_flavour.rs` are the reading half;
`pdfa_writer.rs` is the writing half and judges every fixture twice — by the
full validator with complete coverage, and by the strict structural validator
in `tinker-pdf-cos`, which reads bytes rather than the object graph and was
written for the writer rather than for PDF/A.

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
made. It is why "1 201 of 2 371" is the honest measure of how much of ISO
19005 this build understands, and why a document the writer produces is
reported as *"this validator and the structural one find nothing"* rather than
as *"it conforms"*.

Two more limits worth naming rather than discovering:

- **A defect that exists only in the bytes is invisible to a rule engine that
  reads the object graph.** Implementation limits, hexadecimal string syntax,
  the EOL markers around `obj` and `stream`, cross-reference subsection
  spelling — the reader has normalised all of it away by the time an object
  exists to apply a rule to. The strict structural validator already walks
  those bytes, and joining the two is the honest way to close that family.
- **Level A is written but not validated.** The writer claims it by tagging
  and refuses the ways of getting it wrong; the validator's structure-tree
  rules are staged, so a level A file this build reports nothing about has had
  its tagging read by nobody.

The design and its milestones are in [design/pdfa.md](../design/pdfa.md).
