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

- **Metadata**: the `pdfuaid` claim.
- **Structure**: the tree the structure reader binds — a `/StructTreeRoot`
  (UA-1 7.1, UA-2 8.2.1), one that can be walked, `/MarkInfo /Marked true`
  (6.2 in both published profiles), `/Suspects` not true (UA-1 7.1), a
  `Figure` with `/Alt` or `/ActualText` (UA-1 7.3, UA-2 8.2.5.28.2), headings
  starting at `H1` and never skipping a level in reading order (UA-1 7.4.2),
  and a natural language stated somewhere (UA-1 7.2, UA-2 8.4.4).
- **Fonts**: every font a page's resources name carries an embedded program,
  the standard 14 included (UA-1 7.21.4.1, UA-2 8.4.5.5.1); a Type 3 font,
  whose glyphs are content streams, has none to carry.

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
*before* the validator existed. The rules here were moved in where the corpus
was not reachable, so the census has not been run through the facade, and the
first nightly run owes the figure.

## Refused by name

| What | Why (one line) | See |
| --- | --- | --- |
| Judging tagging correct | whether a `/P` is a paragraph, a reading order the author's, an `/Alt` truthful, is visible only to a person; each such clause is an `Undecidable` gap | design/pdfua.md, non-goals |
| Auto-tagging an untagged file | an untagged file is reported as untagged; nothing inferred reaches a verdict | design/pdfua.md, non-goals |
| Repairing a file | validation reports; the tagged writer conforms or refuses | design/pdfua.md, non-goals |
