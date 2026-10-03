# PDF/X

`Document::validate_pdfx` answers "which PDF/X level does this file claim,
which of that level's requirements does it break, and which did this build
not read?" — and for ISO 15930 the third list is the long one. The standard's
requirement bodies are sold and are not in this tree, and no annotated PDF/X
conformance corpus exists anywhere this project could find, so **this
validator has a false-positive bar and no false-negative bar**: nothing a
third party published says a rule here fires when it should.

The design, the search for evidence and the milestones are in
[design/pdfx.md](../design/pdfx.md).

## What it does

**The claim comes from `/Info`, and costs no XML parse.** ISO 15930 puts it in
the document information dictionary — `GTS_PDFXVersion`, with
`GTS_PDFXConformance` beside it for the 2001 levels — and 15930-4 clause 5
says the header's version and the catalog's `/Version` do not count. Six
levels are identified: PDF/X-1:2001, PDF/X-1a:2001, PDF/X-3:2002,
PDF/X-1a:2003, PDF/X-3:2003 and PDF/X-4, by exact string. A claim made only in
an XMP packet is no claim for these levels; a version string no level above
matches is carried in the verdict and named as not identified. A file claiming
nothing is not judged at all.

**Rules run under the two 2003 levels only**, PDF/X-1a:2003 (ISO 15930-4) and
PDF/X-3:2003 (ISO 15930-6), because the one free restatement of PDF/X's
requirements — the CGATS *Application Notes for PDF/X Standards*, Version 4,
2006 — covers those two and no other. Every rule is a transcription of that
secondary source as the design quotes it, and cites its section:

- **Syntax**, the COS document alone: no encryption (AN 2.11); no
  `LZWDecode` and no `JBIG2Decode` on any stream (AN 2.8); `/Trapped` present
  and the name `/True` or `/False` — not `/Unknown`, not a boolean (AN 2.17);
  on every page a `/MediaBox`, own or inherited, exactly one of `/TrimBox` and
  `/ArtBox`, and that box inside the `/BleedBox` and the `/CropBox` where they
  are present (AN 2.10); every annotation's `/Rect` sharing no area with the
  bleed box, a `PrinterMark`'s with the trim or art box (AN 2.28); every
  private `/Info` entry a text string (AN 2.29). An annotation is judged
  once, on the first page that names it, and an indirect `/Annots` array
  read once; the walk reads at most 2^18 entries across the document, and
  the box, filter and annotation rules report at most sixty-four findings
  each, so a file naming one defect from every page is one finding's worth
  of provenance rather than millions.
- **Print**, the content walk and the output intent: a `GTS_PDFX` output
  intent, embedding a destination profile or naming a registered
  characterization by `/OutputConditionIdentifier` and `/RegistryName`
  (AN 2.16); `DeviceRGB`, directly or as an alternate space, under a CMYK
  profile unless a `/DefaultRGB` stands in (AN 2.16); under PDF/X-3, an
  embedded profile wherever device-independent colour is used (AN 2.16); no
  transparency (AN 2.25); no PostScript XObject and no `PS` operator
  (AN 2.26).
- **Fonts**: every font the pages draw with embedded (AN 2.18).

**Clause numbers are what is in hand for each level.** Under PDF/X-1a:2003 a
finding cites the clause of ISO 15930-4 whose title in the published table of
contents names the subject — 6.2 colour, 6.3 fonts, 6.5 compression, 6.6
trapping, 6.8 boxes, 6.10 PostScript, 6.11 encryption, 6.13 annotations, 6.16
transparency — and nothing finer, since the subclauses are past the preview.
Under PDF/X-3:2003 not even ISO 15930-6's contents are in hand, so a finding
cites the application note it was transcribed from, written `AN 2.11`. The
private-key rule cites the note under both, since no 15930-4 title plainly
owns it.

**One rule, two standards.** Where ISO 15930 asks what ISO 19005 asks, the
PDF/A rule runs and only the clause differs: "no transparency" is ISO 19005-1
6.4's function, and "every font embedded" is the PDF/A font group's embedding
rule over the same "drawn at a visible rendering mode" reading; the device
colour rules ride the PDF/A colour group's content walk. Finding kinds are the
one closed `FindingKind`; the ten PDF/X adds are the ones ISO 19005 has no
rule for.

**Abstentions are values.** `PdfXVerdict::abstained` lists every clause of
the claimed level this build did not decide, in two classes: `Staged` — the
text is in hand and the machinery or reading is not (an inline image's
filter, a shading's colour space, the ICC registry lookup, a font only
invisible text uses) — and `Unread` — the requirement is in no source in hand.
Every PDF/X-4 clause is `Unread` by its 15930-7 title, and each unvalidated
level is one `Unread` gap naming the whole part. `PDFX_STAGED` and
`PDFX_UNREAD` are the static lists.

## API

```rust
use tinker_pdf::{Document, PdfXAbstentionClass, PdfXCoverage};

let verdict = Document::open(bytes)?.validate_pdfx();
match verdict.flavour {
    Some(level) if level.is_validated() => {
        for finding in &verdict.findings {
            println!("{finding}"); // "6.6: TrappedMissing"
        }
    }
    Some(level) => println!("claims {level}, which this build does not validate"),
    None => println!("claims {:?}", verdict.claim),
}
let unread = verdict
    .abstained
    .iter()
    .filter(|a| a.class == PdfXAbstentionClass::Unread)
    .count();
// The object graph alone, which reaches for no walk and no font.
let syntax_only = document.validate_pdfx_with(PdfXCoverage::SYNTAX);
```

`tpdf check --pdfx` prints the claimed level, every finding with its clause,
the groups that ran and how many clauses were abstained on in each class; it
exits non-zero when a file breaks a requirement of the level it claims.

## Verified

`crates/tinker-pdf/tests/pdfx_rules.rs` holds one fixture per rule, each with
the near-miss twin that must not fire, from a baseline that is clean under
both 2003 levels — and, for every rule, the same change under both levels with
each level's citation. **These twins are the only thing that makes a PDF/X
rule fire**, and each is this project's reading of a secondary source in both
directions.

`crates/tinker-pdf/tests/pdfx_census.rs` reads every file of every fetched
corpus and prints each PDF/X claim with the level it is read as; it asserts
that a file claiming nothing gets an empty verdict, and that a real claim to a
validated level has no finding nobody has read. It runs nightly in
`corpus.yml` and honours `TINKER_CORPUS_REQUIRED`.

## What is not measured

**Nothing.** The census was written where the corpora are not reachable, and
the design's own reading of the real claims they carry names `PDF/X-4`,
`PDF/X-3:2002`, `PDF/X-1:2001` and an empty string — no level these rules run
under. The two published suites the design names are a PDF/X-3:2002 set
(Altona 1.2) and a PDF/X-4 one (Ghent Output Suite 5.0), and neither is
pinned. So for the 2003 levels the false-positive bar is, today, empty as
well: no file anyone else made that claims PDF/X-1a:2003 or PDF/X-3:2003 is
in reach.

## Refused by name

| What | Why (one line) | See |
| --- | --- | --- |
| Rules under PDF/X-4 and PDF/X-6 | the requirement bodies are not in hand, and a rule written from a clause title is a guess; every X-4 clause is an `Unread` gap by name | design/pdfx.md, milestone 7 |
| Rules under PDF/X-1a:2001 and PDF/X-3:2002 | the application notes v4 defer them to a Version 3 that was not found | design/pdfx.md |
| PDF/X-1:2001, PDF/X-2, PDF/X-5 | non-goals: a deprecated level, and partial exchange a blind validator cannot settle | design/pdfx.md, non-goals |
| A PDF/X flavour on the writer's archival profile | `ArchivalProfile` is PDF/A's and lives in `tinker-pdf-cos`, `PageBuilder` has no `set_trim_box`, and the writer's third leg — the shape comparison against a pinned suite file — waits on milestone 2's blocked pin | design/pdfx.md, milestone 6 |
| Looking a `RegistryName` up in the ICC registry | the registry is data with a date on it; vendoring it is a decision | design/pdfx.md, non-goals |
| Conversion to PDF/X | validation reports; nothing rewrites colour or fonts | design/pdfx.md, non-goals |
