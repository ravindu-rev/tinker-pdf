# PDF/UA

`Document::validate_pdfua` answers "which part of ISO 14289 does this file
claim, which of that part's clauses does it break, and which did this build
not decide?" — the third half of the question is the one PDF/A does not have.
Most of ISO 14289 is a judgement about meaning that no reader can make, so a
verdict that reported only findings would read as "conforms" over clauses
nobody checked. The verdict carries the abstentions in the same struct.

The design and its milestones are in [design/pdfua.md](../design/pdfua.md).

## What it does

**The claim comes from the metadata.** ISO 14289 puts it in the XMP packet:
`pdfuaid:part`, and for part 2 `pdfuaid:rev`, a four-digit year. Both RDF
spellings are read, the namespace is checked, and `pdfaid` is never read as
`pdfuaid` — that confusion once made 434 PDF/UA files read as PDF/A claims. A
property in the identification namespace written under another prefix is read
and reported (clause 5 of both parts). A file claiming no part is told so and
numbered as part 1 numbers it; that is a presentation choice, since the kind
is what a caller matches on.

**Findings are the PDF/A kernel's.** A finding is a `ConformanceFinding`, its
kind the one closed `FindingKind` both standards share, and the reaches are
counted by the same `Machinery` the PDF/A groups use. Where ISO 14289 asks
what ISO 19005 already asks, the kind is the PDF/A one and only the clause
number differs.

**Rule groups**, keyed by machinery:

- **Syntax**, the COS document alone: the catalog's `/Metadata` a stream with
  `/Type /Metadata /Subtype /XML` (UA-1 7.1, UA-2 8.11.1);
  `/ViewerPreferences /DisplayDocTitle true` (7.1, 8.11.2); every optional
  content configuration named and none carrying `/AS` (7.10, 8.7 — part 2
  names them only once `/Configs` holds one, as its sentence says); an
  embedded file's specification with non-empty `/F` and `/UF` (UA-1 7.11) or,
  in the `/EmbeddedFiles` tree, a `/Desc` (UA-2 8.14); no reference XObject
  (UA-1 7.20); an encryption dictionary whose `/P` sets bit 10, the
  accessibility-extraction permission (UA-1 7.16); and no XFA form at all
  (UA-2 8.10.1).
- **Metadata**: the `pdfuaid` claim, and the packet's `dc:title` (7.1,
  8.11.1), read in the same pass.
- **Structure**: the tree the structure reader binds — a `/StructTreeRoot`
  (UA-1 7.1, UA-2 8.2.1), one that can be walked, `/MarkInfo /Marked true`
  (6.2 in both published profiles), `/Suspects` not true (UA-1 7.1), a
  `Figure` with `/Alt` or `/ActualText` (UA-1 7.3, UA-2 8.2.5.28.2) and a
  `Formula` likewise (UA-1 7.7) — an empty `/Alt` describing nothing under
  part 1 and standing under part 2, an empty `/ActualText` standing under
  both — headings
  starting at `H1` and never skipping a level in reading order (UA-1 7.4.2),
  at most one `H` under any node and never an `H` beside an `Hn` (7.4.4), the
  content models of tables, lists and tables of contents — which parent a
  row, a cell, a list item or a TOC item sits in, which kids a table, a row,
  a table section, a list, an item or a TOC may hold, at most one `THead`,
  `TFoot` and `Caption` in a table, a `TBody` beside a `THead` or `TFoot`, a
  caption where its container admits one (UA-1 7.2, as veraPDF's 7.2-3 to
  7.2-20, 7.2-26 to 7.2-28 and 7.2-36 to 7.2-40 state them) — every `Note`
  carrying an `/ID` no other `Note` carries (7.9), a
  natural language stated somewhere (UA-1 7.2) or on the catalog itself and
  not empty (UA-2 8.4.4), every `/Lang` on the catalog and on an element an
  RFC 3066 language tag and never empty (7.2, 8.4.4), every structure type
  resolving to a standard one and no standard type remapped (UA-1 7.1), and
  every structure element carrying its `/P` (7.1, 8.2.1).
- **Annotations** (UA-1 7.18), each page's `/Annots` against the tree's
  `/OBJR` kids: an annotation in an `Annot` element, a widget in a `Form`, a
  link in a `Link` (7.18.1, 7.18.4, 7.18.5); `/Contents` or the element's
  `/Alt` — for a widget, its *field's* `/TU` or the `/Alt` (7.18.1); a link's
  own `/Contents` (7.18.5); `/Tabs /S` on every page with an annotation
  (7.18.3); no `TrapNet` (7.18.2); a `PrinterMark` in no element (7.18.8).
  A hidden annotation, one whose `/Rect` misses the crop box, and a subtype
  ISO 32000-1 Table 169 does not define (the corpus's upper-case `FREETEXT`)
  are held to none of these; a `Popup`, a `Form` element's `Role`, and a
  media clip's `/CT` and `/Alt` are staged by name. An annotation is judged
  once, on the first page that names it, and an indirect `/Annots` array
  read once (`/Tabs` is still read on every page); the walk reads at most
  2^18 entries across the document and stops at the group's 256 findings.
- **Fonts**: **the PDF/A font group, run for a PDF/UA claim** and
  re-numbered kind by kind, over the fonts a page draws with at a visible
  rendering mode — embedding (UA-1 7.21.4.1, UA-2 8.4.5.5.1), a Type 2
  CIDFont's `/CIDToGIDMap` (7.21.3.2, 8.4.5.3.2), a TrueType font's
  `/Encoding` (7.21.6, 8.4.5.7), a mapping to Unicode with the PDF/A group's
  exemptions (7.21.7, 8.4.5.8), glyph widths against the program (7.21.5,
  8.4.5.6); and three rules of its own — a composite
  font's encoding CMap embedded unless Table 118 predefines it, its `/WMode`
  agreeing with its program, no `usecmap` outside Table 118 (7.21.3.3,
  8.4.5.4), its collection the CIDFont's (7.21.3.1, 8.4.5.3.1) — and no drawn
  code whose `/ToUnicode` maps to U+0000, U+FEFF or U+FFFE (7.21.7, 8.4.5.8),
  over every code below 2^16 a font draws and its first 1 024 wider ones —
  kept as codes, the drawn strings held a mebibyte at a time.
  A Type 3 font, whose glyphs are content streams, has no program to embed.

**One rule, two standards.** Where ISO 14289 asks what ISO 19005 asks, the
PDF/A rule runs and the clause is ISO 14289's: the font group above, the
level A structure-type rule, and the language grammar, which is one function
with one flag between them — ISO 19005 admits `/Lang ()`, ISO 14289 refuses
it, and `pdfa::logical`'s unit test holds both sides. A finding kind with no
PDF/UA clause — a subset tag's spelling, a program under the wrong
`/FontFile` key, part 1's `/Differences` prohibition — is dropped rather than
reported under the nearest number.

**Abstentions are values.** `PdfUaVerdict::abstained` lists every clause of
the part the verdict did not decide, each a `PdfUaGap` with the clause, what
it asks and why it is not decided, in one of two classes: `Staged` — a reader
could decide it and the machinery is missing, which is a milestone — and
`Undecidable` — a judgement about meaning, which is a limit. `PDFUA_STAGED`
and `PDFUA_UNDECIDABLE` are the static lists.

## API

```rust
use tinker_pdf::{Document, PdfUaAbstentionClass, PdfUaCoverage};

let verdict = Document::open(bytes)?.validate_pdfua();
for finding in &verdict.findings {
    println!("{finding}"); // "7.3 object 13 0: AlternativeDescriptionMissing { .. }"
}
// What was not decided, by name — never a rate.
let staged = verdict
    .abstained
    .iter()
    .filter(|a| a.class == PdfUaAbstentionClass::Staged)
    .count();
// The structure group alone, which reads no font program.
let tree_only = document.validate_pdfua_with(PdfUaCoverage::STRUCTURE);
```

`tpdf check --pdfua` prints the claimed part, every finding with its clause,
the groups that ran, and how many clauses were abstained on in each class; it
exits non-zero when a file breaks a clause of the part it claims.

## Where the clause numbers come from

ISO 14289-1 and -2 are sold and are not in this tree. Clause numbers are the
ones the veraPDF corpus's directories, its fixtures' outlines and veraPDF's
published PDF/UA profiles (`PDFUA-1.xml`, `PDFUA-2.xml`, read as data at
`veraPDF-validation-profiles` `070d39f`) agree on. Nothing of veraPDF's runs
(ruling 13); its profiles are a published statement of which clauses are
checkable, the standing the machine-readable PDF/A profiles have in
[design/pdfa.md](../design/pdfa.md). A rule with no number under a part does
not run under it — `/Suspects` and heading order have none under ISO 14289-2.

## Verified

`crates/tinker-pdf/tests/pdfua_rules.rs` holds one fixture per rule at the edge
of its clause, each with the near-miss twin that must not fire, on the
discipline `pdfa_fonts.rs` uses: a clean baseline under both parts, one change
per test, one finding of one kind under one clause.

`crates/tinker-pdf/tests/pdfua.rs` is the census against the veraPDF corpus's
434 annotated PDF/UA fixtures, through the facade: caught, abstained, and
**false alarms, which must be zero** — the hard assertion — with what each
abstaining verdict named printed beside the counts. It runs nightly in
`corpus.yml`, and now finds `corpus/files` there without `TINKER_CORPUS`; it
honours `TINKER_CORPUS_REQUIRED`.

## What is not measured

The census figure in the roadmap — 29 of 239 caught, 210 abstained, 0 false
alarms over 195 conforming — was measured on 16 September 2026 by the census
*before* the validator existed. The rules here — the nine moved in, the
twenty-four milestone 2 adds, milestone 3's grammar and milestone 4's
annotation rules — were written where the corpus was not
reachable, so the census has not been run through the facade: the first
nightly run owes the caught figure, and — the number that decides whether a
rule stays — the false-alarm count over the 195 conforming
files, which the census asserts is zero. Each rule's reading is held only to
the fixture and the twin built here, which are this project's reading of the
clause in both directions.

This engine's own output carries seven findings, each named in `pdfua.rs`
with its reason: no `pdfuaid` claim, no metadata stream, no
`/DisplayDocTitle`, and the unembedded standard 14 — the writer's to close
when it claims PDF/UA (design/pdfua.md milestone 7) — and, since the
annotation rules, a link annotation with no `/Contents` and no `/Alt` on its
`Link` element, on a page with no `/Tabs /S` — the tagged writer's to close,
since it knows both the link's text and that the page carries one.

## Refused by name

| What | Why (one line) | See |
| --- | --- | --- |
| Judging tagging correct | whether a `/P` is a paragraph, a reading order the author's, an `/Alt` truthful, is visible only to a person; each such clause is an `Undecidable` gap | design/pdfua.md, non-goals |
| Auto-tagging an untagged file | an untagged file is reported as untagged; nothing inferred reaches a verdict | [ROADMAP](../ROADMAP.md) SD-07 |
| Repairing a file | validation reports; the tagged writer conforms or refuses | [ROADMAP](../ROADMAP.md) SD-08 |
