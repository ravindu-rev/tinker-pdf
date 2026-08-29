# Fonts

Every font format a PDF can carry is parsed in `crates/tinker-pdf-font` —
bytes in, metrics and outlines out, no PDF types on its API (ruling 8,
[rulings](../rulings.md)) — and the facade binds those programs to font
dictionaries, encodings and CMaps. The engine reads no font directories, by
policy: that is an operating-system dependency `wasm32-unknown-unknown` does
not have. It bundles no font programs **by default** either — a face is a
licensing decision, and the host that has one supplies it — but the
`bundled-fonts` feature carries twelve Liberation faces for the host that has
none, which is a decision this page's measurement settled rather than a
default that changed. A document that embeds its fonts needs nothing else; one that
does not draws no text without a host-supplied [`FontProvider`](#api), and the
gap is reported, never silent.

## What it does

**TrueType** (9.6.6, 9.9). `Sfnt` parses the table directory and `glyf.rs`
reads outlines from `glyf`/`loca`, including the implied on-curve midpoints
between consecutive off-curve points. Composite glyphs are followed, bounded
two ways: a nesting depth of 8 and a total budget of 256 components, because a
depth cap alone leaves a 64-component self-referencing glyph asking for 64^8
outlines (ruling 1). Character lookup goes through the font's `cmap`,
including the 9.6.6.4 route for symbolic fonts whose (3,0) subtable maps codes
into the private-use block. Hinting is deliberately not interpreted; outlines
render unhinted with anti-aliasing.

**CFF / Type 2** (9.6.6). `Cff` parses the INDEX and DICT structures and runs
Type 2 charstrings — implicit width on the first stack-clearing operator,
`hintmask` operand counting (hints are parsed exactly far enough to skip),
local and global subroutines with the count-dependent bias. Glyph selection is
the charset's job, never the character code's: all three written charset
formats (0, 1, 2) plus the three predefined ones (ISOAdobe, Expert,
ExpertSubset), names resolved through the 391 standard strings and the font's
own string INDEX, and the font's built-in encoding with its supplements. A
CID-keyed font (`ROS` in the Top DICT) selects glyphs by CID through the
charset, with `FDSelect` routing each glyph to its own Font DICT's Private
DICT, local subroutines and — where one is stated — font matrix
(`Cff::font_matrix_for` is per glyph for exactly this reason).

**OpenType/CFF** (the `OTTO` tag). `Sfnt::parse` accepts an `OTTO` face, and
a face with no `glyf` routes its `CFF ` table into the CFF path. Before that
routing existed such a font parsed, found no `glyf`, and drew every glyph as
an empty outline that read as a legitimate space.

**Type 1** (9.9, `/FontFile`). `Type1::parse` reads PFB, PFA and the bare
bytes a PDF embeds: eexec decryption, the second decryption layer around each
charstring with the font's own `/lenIV`, and Type 1 charstrings — `hsbw`'s
side bearing, `seac` accent composition, and `flex` through the
`callothersubr`/`pop` callback protocol. Glyphs are addressed by name, via
the encoding the font dictionary specifies or the program's built-in one.

**Type 3** (9.6.5). A Type 3 glyph is a content stream, not an outline: it
can fill, stroke, draw images. The content interpreter fetches the procedure
named by `/Encoding /Differences` out of `/CharProcs` and runs it recursively
under the font's `/FontMatrix` — which is required, because a Type 3 font
chooses its own glyph-space units rather than the 1/1000 convention.

**Composite fonts** (9.7.4). The CID the encoding CMap produces — not the
code — selects both the glyph and the advance. For a CIDFontType2,
`/CIDToGIDMap` in both its forms (`/Identity`, or a stream of two-byte GIDs,
9.7.4.2) turns CID into glyph index; the font program's own `cmap` is not
consulted on this path, because a composite font's code is not a character.
For a CIDFontType0 the CFF charset answers. A CID the font does not carry
draws `.notdef` **and** reports — glyph 0 is usually blank, and without the
report the failure looks like nothing happened.

**CMaps** (9.7.5, 9.10.3). One scanner reads both encoding CMaps and
`/ToUnicode`. Codespace ranges are matched **per byte** as 9.7.6.2 requires —
`90ms-RKSJ-H`'s `<8140> <9FFC>` is one interval per byte position, not one
integer interval — and an undefined code takes its byte length from its lead
byte (9.7.6.3), so a damaged string stays in step. Differential CMaps are
followed through both spellings, the `usecmap` operator and the stream
dictionary's `/UseCMap` (Table 120): the child's mappings stay in front of
the parent's wherever they overlap, the chain is capped at
`MAX_USECMAP_DEPTH` (4) links, and a CMap that uses itself — directly or
around a chain — is refused by source comparison, not by name. All 202 CMaps
of Adobe's registry (9.7.5.2) are vendored
([THIRDPARTY](../../THIRDPARTY.md)) and compiled by `tinker-pdf-font`'s
`build.rs` into 1.19 MB of delta-encoded, deflated tables behind the `cmap-predefined`
feature, default on; with it off, the codespaces still ship in 4.6 KB so
strings split correctly, and `CMap::is_approximate` says the CIDs are
guesses. A name outside the registry answers `None` and is warned about,
never approximated silently.

**Vertical writing** (9.7.4.3). `/W2` in both of Table 117's forms and
`/DW2` with its `[880 -1000]` default supply the displacement and the
position vector, keyed by the same CID that selects the glyph; the position
vector places the glyph within its em box and the advance moves the pen down
by 9.4.4's `ty` (with `Tz` correctly not applied to a vertical advance).
`/WMode` and the `-V` CMap suffix both set the mode, and 9.7.5.3's rule that
the child's `/WMode` wins over an inherited one is kept.

**Standard-14 metrics** (9.6.2.2, Annex D.5). A simple font naming one of the
standard faces may omit `/Widths`, so `Standard14` carries Adobe's AFM
advances for the printable ASCII range exactly, with the aliases every viewer
honours (Arial for Helvetica, TimesNewRoman for Times). Above 0x7F a width is
approximated from the glyph's base letter, and the approximation is reported
by the flag `Standard14::advance` returns beside the width, never silently.

**The host seam.** `FontRequest::read` distils a font dictionary and its
descriptor (9.8.2 Table 123 flags, the 9.6.4 subset tag stripped, weight and
slope read from the name too because plenty of producers set no flags) into a
request a `FontProvider` answers with a TrueType or bare CFF program — or
declines. `SimpleFontProvider` covers the common case: up to four faces
chosen by weight and slope, falling back to whatever is set, and declining
symbolic fonts unless `substituting_symbolic()` was called, because a text
face standing in for a symbol font draws confidently wrong glyphs.

**Writing** (9.9). `DocumentBuilder::add_embedded_font` and `add_cid_font`
embed a TrueType, an `OpenType/CFF` face or a bare CFF program, with widths
taken from the program's own `hmtx` — or, for a bare CFF, from its charstrings
through its `FontMatrix`, which need not be the usual 1/1000. Each gets the
descriptor entry 9.9 Table 126 gives it: `/FontFile2`, `/FontFile3
/Subtype /OpenType`, or `/FontFile3 /Subtype /Type1C` (`/CIDFontType0C` under a
composite font).

`set_subset_fonts` (on by default) cuts each program down to the glyphs the
pages drew, and **glyph identifiers are never renumbered** — which is what
lets `/Widths`, `/W`, `cmap`, `/CIDToGIDMap` and `/ToUnicode` stay as written.
For TrueType that means `glyf`/`loca` rebuilt with composite closures followed
and a dropped glyph left as a zero-length `loca` entry. For CFF it means the
CharStrings INDEX, both subroutine INDEXes, the Private DICTs and the Top DICT
rebuilt together, with a dropped glyph left as a single `endchar`; the charset,
the encoding, the String INDEX and `FDSelect` are copied through byte for byte,
because nothing moved and re-encoding them would only be a chance to map a
glyph to the wrong name. A `callsubr` operand is recomputed from the new index
and the **new bias** rather than adjusted, since subsetting can move an INDEX
across the 1 240 and 33 900 thresholds. A global subroutine reached from two
Font DICTs is written twice, because the local subroutines it calls are the
caller's.

The 9.6.4 six-letter tag is prefixed to `/BaseFont`. A face that is not cut
down is embedded whole — larger and correct (ruling 2) — and
`DocumentBuilder::finish_reporting` returns an `EmbeddedWhole` naming the
resource and one of three `SubsetRefusal` reasons: the font claimed none of the
text drawn with it, the program could not be rebuilt, or the rebuild came out
no smaller than the face. That last one is common: 212 of the fetched corpora's
441 CFF faces are already producer-made subsets with nothing left to remove.

## API

Everything reaches callers through the facade (ruling 11). Reading:
`Document::open`, then `Document::with_fonts` (or `OpenOptions::with_fonts`
via `Document::open_with`, which for reflowable formats arrives early enough
to affect pagination) installs a provider; `Page::render` reports
`RenderWarning::UnreadableFont` on `Bitmap::warnings` when glyphs went
undrawn, and `Page::text` — which needs no outlines and consults no provider
— reports `TextWarning::UnknownFont` and `TextWarning::UnmappedCode` on
`TextPage::warnings`. Parse-time CMap leniencies land on
`Document::warnings()` as typed `WarningKind` values. Writing:
`DocumentBuilder::add_base_font`, `add_named_font`, `add_embedded_font`,
`add_cid_font`, `set_subset_fonts`, and `PageBuilder::text` or
`PageBuilder::glyphs` (glyphs addressed by index, for a composite font).

```rust
use std::sync::Arc;
use tinker_pdf::{Document, SimpleFontProvider};

let face = std::fs::read("DejaVuSans.ttf")?;
let doc = Document::open(bytes)?
    .with_fonts(Arc::new(SimpleFontProvider::new(face)));
// A document that embeds nothing now draws text; without the provider it
// would extract perfectly and render none of it, reporting UnreadableFont.
```

## What the missing faces cost, measured

This engine bundles no font programs, so a document that names Helvetica and
embeds nothing extracts its text perfectly and draws none of it. That is a
policy, and until now the corpus could not separate its cost from the engine's
own defects: every such file counts as *rendered with something reported*, and
`corpus/ratchet.json` said so in its own note without being able to say how
much.

Both numbers now exist. `corpus/ratchet-fonts.json` is the same 4 525 files
measured with a face supplied — the one `cargo xtask synth-face` writes, whose
every glyph from 32 up is a filled box, so it answers *was a face available*
and nothing else:

| Corpus | Files | No faces | Synthetic face | Bundled faces |
| --- | ---: | ---: | ---: | ---: |
| pdf.js | 974 | 382 | 190 | 199 |
| veraPDF | 2 907 | 55 | 39 | 39 |
| qpdf | 637 | 530 | 110 | 132 |
| PDF Association | 7 | 6 | 4 | 4 |
| **Total** | **4 525** | **973 (21.5 %)** | **343 (7.6 %)** | **374 (8.3 %)** |

Three bars, in three files, and `corpus-run` refuses to compare any of them
against another. The last column is the faces this project now ships
(`corpus/ratchet-bundled.json`); the middle one is a face it synthesises for
the measurement (`corpus/ratchet-fonts.json`), every glyph a filled box.

**The bundled column is 31 files worse, and that is the bundled set being
right.** Every one of them is a symbolic font — an embedded face the engine
could not read, with `/Flags` bit 3 set — which the bundled faces decline and
one all-purpose face silently answered with squares. The synthetic bar was
flattering itself on those files, and the difference between the two columns is
exactly the size of that flattery.

**630 of the 973 — 65 % — were the absence of a face**, and in qpdf's corpus it
is four fifths of them. The synthetic face exists so the measurement can
be reproduced anywhere: no licence, no download, the same bytes on every
machine forever, and no dependence on what a runner image happens to ship.

Neither figure says the text was *set* correctly; both say whether a glyph was
drawn at all.

Recording the second bar found a defect in the corpus probe rather than in the
engine, which is worth keeping because of the shape of it. The two metamorphic
relations that rewrite a document reopened it **without the provider**, so the
rotated or cropped copy had no way to draw text the original had drawn. With no
faces anywhere the two renders were equally blank and the relations held; with
faces, 309 of pdf.js's 839 files failed `rotate` instead of 219. A comparison
between two renders is only a comparison if both are made under the same
conditions, and nothing could see that until one of the conditions existed.

**What it settled.** The base 14 are required to be available by 9.6.2.2, so a
conforming file that names one and embeds nothing was a file this engine could
not draw — a conformance gap rather than only a policy. The `bundled-fonts`
feature below is the answer, and this measurement is why it exists.

## The bundled faces

`bundled-fonts`, **off by default**, embeds the twelve Liberation faces —
Sans, Serif and Mono, four styles each. They are metric-compatible with Arial,
Times New Roman and Courier New, which are in turn what every reader
substitutes for Helvetica, Times and Courier, so the advance widths match what
the document's own `/Widths` array already says. That is twelve of the standard
14; Symbol and ZapfDingbats are the other two and are declined rather than
approximated.

```toml
tinker-pdf = { version = "0.0.1", features = ["bundled-fonts"] }
```

Three things about the shape of it:

- **The bundled set is asked last.** A host provider is consulted first and the
  bundled faces answer only what it declines — per *request*, not per document,
  so a host with a face for the body text and none for the monospaced code
  sample gets both right. Turning the feature on can therefore only add
  answers; it cannot take one away.
- **Symbolic fonts are declined by name as well as by flag.** A base-14 font
  dictionary carries no `/FontDescriptor`, so `/Flags` is zero and the symbolic
  bit is absent for exactly the two faces that need it. Without the name check
  a `/BaseFont /Symbol` page rendered its text as Latin letters — legible,
  plausible and wrong, which is worse than the gap it replaced.
- **The family comes from the name before the flags**, for the same reason:
  producers set the serif bit wrongly all the time, and a document that embeds
  nothing names `Helvetica`, `Times-Roman` or `Courier` exactly. The names nest
  — `sans-serif` contains `serif`, `DejaVu Sans Mono` contains `sans` — so the
  narrowest claim is tested first, and each of those sentences is a test.

Off by default because a desktop application, a server with a font package or a
web page with a face already loaded all have better faces than these and a way
to hand them over, and none of them should carry 4.2 MB of ours. `FontProvider`
remains the seam either way. Provenance and the OFL text are in
[THIRDPARTY.md](../../THIRDPARTY.md).

## Shaping, and what it is allowed to claim

`crates/tinker-pdf-shape` is the eleventh leaf: face bytes and text in,
positioned glyph runs out, in integer font design units, with no PDF or CSS
vocabulary on its API. Nothing in `tinker-pdf-render` calls it and nothing ever
will — see the row above. Its design and its eight milestones are
[design/shaping.md](../design/shaping.md).

Landed so far:

- **OpenType Layout.** `GDEF`, `GSUB` types 1–8 and `GPOS` types 1–9,
  coverage and class definitions, extension and chaining-context lookups.
- **The default shaper.** `cmap` through `tinker_pdf_font::Sfnt`, plus `cmap`
  format 14 read here because a variation selector is consumed by a shaper and
  never reaches a renderer; script itemization; `locl`/`ccmp`/`rlig`/`liga`/
  `clig`/`calt`, then `kern`/`dist`/`curs`/`mark`/`mkmk`; a cluster on every
  glyph that is a byte offset into the caller's own text.
- **UAX #9.** Level resolution per paragraph, bracket pairs, mirroring, and
  L1/L2 per *line*, as a function the caller applies after breaking — because
  only the caller knows where a line ends.
- **Cursive joining.** `Joining_Type` from the UCD, the four forms from the
  Unicode Standard's own rule, and a feature mask per glyph so `init` reaches
  the first letter of a word and no other. Seven `GSUB` stages instead of one,
  because a face's `medi` lookup is written expecting `init` not to have run.
- **The Universal Shaping Engine, in part.** `Indic_Syllabic_Category` and
  `Indic_Positional_Category`, a syllable per Brahmic cluster, every `GSUB`
  feature applied inside one syllable and never across two, USE's feature
  stages, the canonical decomposition of a Brahmic character that has one, and
  one reordering pause that moves a pre-base glyph to the front of its
  syllable — over the glyphs, through a category each one carries from the
  character it came from, so a conjunct formed before the pause is still
  reordered. **Milestone 5's exit criterion is not met**; the table below says
  by how much.

- **A layout seam, and one path owning a run.** `Shaper` sits beside `Metrics`
  in `crates/tinker-pdf-layout/src/metrics.rs` as plain structs and `f64`, so
  the layout crate gains no dependency; `Metrics::shaper()` is asked once per
  provider, in one place in `flow.rs`, and a run measured through the shaper is
  never also measured through `Metrics::measure`.
  `crates/tinker-pdf-layout/tests/shaper.rs` drives a provider whose `advance`
  **panics**, so a second measurement path is a failure and not a discrepancy.
  `BookMetrics` implements it over a book's own `@font-face` faces.
- **Shaped runs into a document.** `tinker_pdf::shaping` turns a run's clusters
  back into the text each glyph stands for and writes it through
  `DocumentBuilder::glyph_run`. An Arabic string built that way extracts back
  to itself through the `/ToUnicode` the writer wrote, across a ligature, and
  the document is clean under the strict structural validator.

- **Shaped values into a form field.** `tinker_pdf_cos::Font::program` walks
  `/DescendantFonts` → `/FontDescriptor` → `/FontFile2` (or `/FontFile3`, or
  `/FontFile`) and returns the stream's *address*, which is what unblocked
  milestone 8: `fill.rs` reaches its font through the AcroForm `/DR`, and a
  `Font` that knew every width and no outline had nothing to shape against.
  Where the `/DA` font is composite, `/Identity-H`, and embeds an sfnt, a
  field's value is shaped and written as a `TJ` run of two-byte codes in
  visual order. Everywhere else the single-byte path stands and every
  character it could not write is named by
  `WarningKind::FieldCharacterUnrepresentable`, against the field's own
  object — see [forms.md](forms.md).

- **A paginated Arabic book.** `epub/paint.rs` resolved fallback per character
  and then asked that character's face for a glyph, so an Arabic paragraph was
  *measured* through the shaper and *drawn* as isolated letters in the order
  they were typed — two measurement paths disagreeing, which is what
  `metrics.rs` warns about. Drawing now walks the same segments measurement
  does (`paint::face_runs`, `css-fonts-4` §5.3 resolved **before** shaping,
  because a glyph index means nothing outside its own face), and an embedded
  face's segment is shaped whole. `crates/tinker-pdf/tests/epub_shaped.rs`
  pins it: joined forms, the line drawn from its last letter, a render
  fingerprint, and a page-level assertion that the line was measured the way
  it is drawn. `epub_reftest.rs` gains the EPUB tier's right-to-left pair.

  Two limits, named rather than implied:

  - **Reordering is per face segment.** A right-to-left line whose characters
    need two faces is drawn as two left-to-right pieces, because fallback cuts
    the run before UAX #9's rule L2 is applied to it. Closing it means
    resolving levels above the segmentation, which `flow.rs` does not do at
    all today.
  - **`GPOS` offsets are not carried onto the page.**
    `PageBuilder::glyphs` writes one hex string at one origin, so a mark sits
    where its advance puts it rather than where its anchor does. The
    positioned form exists — `DocumentBuilder::glyph_run`, which milestone 7
    writes through — and the EPUB painter does not use it yet.

**What no shaping engine here adjudicates.** Ruling 13 rules out running
another shaper and diffing, so the claim for a script is exactly as strong as
the fixture behind it, and the scripts divide in five:

| Script | What is behind it |
|---|---|
| Latin, Ethiopic | text-rendering-tests sections `CMAP-1`, `CMAP-2`, `GSUB-1`, `GSUB-2`, `GPOS-1`–`GPOS-4`: 48 cases, 38 of them discriminating against an implementation with no shaper at all |
| Hebrew, Arabic and every other bidirectional script, for **direction only** | `BidiTest.txt` and `BidiCharacterTest.txt` in full — 861 948 resolutions. This says the levels and the visual order are right; it says nothing about the glyphs |
| Arabic *shaping* | `SHARAN-1`: six words of Urdu in Nasta‘līq, all six reproduced glyph for glyph and position for position. It is the corpus's only Arabic-script section, so joining, `rlig` and cursive attachment are adjudicated **for one face of one style of one language**. Naskh, and the vowelled Arabic of a Qur'an, have no fixture here |
| Balinese, Kannada, Tai Tham | `SHBALI`, `SHKNDA`, `SHLANA`: 333 cases, of which **258 are reproduced and 75 are not**. Three of the sixteen sections pass whole. `crates/tinker-pdf-shape/tests/text_rendering.rs`'s `PASSING` holds the number per section and is a ratchet — it may rise and may not fall |
| Every other Brahmic and Southeast Asian script — Devanagari, Bengali, Gujarati, Gurmukhi, Malayalam, Odia, Sinhala, Tamil, Telugu, Myanmar, Khmer, Lao, Thai, Javanese, Sundanese, Tibetan, Tagalog and the rest — and Syriac, N'Ko, Mongolian, Adlam, Thaana, Mandaic, Hanifi Rohingya, Phags-pa | **shaped, and unverified.** The cluster model runs over them because it is driven by the Unicode properties rather than by a list of scripts — and so, since milestone 5 closed, does the canonical decomposition, which reaches every two-part vowel in Devanagari, Bengali, Oriya, Tamil, Telugu, Malayalam and Sinhala. No fixture in either vendored corpus contains a face for any of them. What that produces is deterministic and plausible; nothing in this repository says it is right |

Three things milestone 5 **closed**, and the largest of them was not on the
list of what was wrong:

- **The halant no longer moves the reordering insertion point.** A pre-base
  vowel goes in front of the whole conjunct, not in front of the consonant it
  attaches to. `SHBALI-2/1` — `KA ADEG-ADEG PA TALING` — expects the taling
  first, and the opposite rule stood here on a plausible sentence until the
  fixtures were run against it. Thirty cases across six sections.
- **The reordering permutation is computed over the glyphs**, through a USE
  category carried on each one from the character it came from. It was
  computed over the characters and skipped whenever a substitution had changed
  their number, which is exactly the clusters where reordering matters.
- **Canonical decomposition**, of Brahmic characters that have one, from
  `UnicodeData.txt` field 5 fully expanded at build time. Five cases — worth
  recording, because it was named as the single largest cause and it was the
  smallest of the three.

Four things it does **not** do, each named so they are a backlog and not a
mystery:

- **The Indic shaper's base-finding.** Kannada, Devanagari and their seven
  relatives form conjuncts by a different model from USE's, in which `rphf`,
  `half` and `blwf` apply at *one position* of a syllable rather than to the
  whole of it. This crate applies them to the syllable, which is why
  `SHKNDA-3` reproduces none of its 31 cases and `SHKNDA-2` four of its 16.
- **Canonical ordering.** What is done above is decomposition and not NFD: the
  `Canonical_Combining_Class` sort that would follow it is not applied. No
  case in the corpus is known to need it, so it is a gap rather than a cause.
- **Hangul.** Its decomposition is arithmetic rather than tabulated (UAX #15
  §3.12) and `UnicodeData.txt` lists none, so a Hangul syllable is not
  decomposed. Hangul does not reach the Brahmic plan anyway; the row is here
  so the absence is a decision.
- **Dotted circles.** USE inserts one into a cluster that its grammar calls
  broken. This crate never inserts a glyph the text did not ask for, so a
  malformed cluster renders as its parts.

Two capabilities are refused rather than absent, and both are in
`tinker-pdf-font` rather than here: a `cmap` of format 13, and a Macintosh
`cmap` read in a non-Roman encoding. The corpus has a section for each
(`CMAP-4`, `CMAP-3`) and `crates/tinker-pdf-shape/tests/text_rendering.rs`
declines them by name with the fix each one wants.

Two more are refused inside `tinker-pdf-shape`, for ruling 13's reason rather
than for want of code: **Syriac's Alaph**, which selects `fin2`, `fin3` and
`med2` in place of `fina` by its `Joining_Group`, and the **topographical
features of a joining Brahmic script**, which would need the four joining masks
on a run that computes syllables instead. Neither has a fixture in either
vendored corpus, so implementing either would be adding behavior nothing here
could show was right.

## Refused by name

| What | Typed variant | Why (one line) | See |
|---|---|---|---|
| Shaping **while reading a PDF**: `TJ` arrays are honored as written | none — the producer positioned every glyph and re-shaping them would be wrong | Permanent, and the only half of the old non-goal that survived; the producing half is `tinker-pdf-shape`, below | [shaping](../design/shaping.md) |
| A CFF whose `callsubr` operand is not the token before the call, or that calls a subroutine it does not carry, or whose subroutine calls itself, or that declares `CharstringType 1` | `SubsetRefusal::ProgramNotRebuildable`; the whole face is embedded | Each needs the subsetter to invent what the font meant, and a broken subset renders *almost* right | this page |
| A CFF subset that comes out no smaller than the face | `SubsetRefusal::SubsetNotSmaller`; the whole face is embedded | A producer's own subset has nothing left to remove, and the face is also the one it tested | this page |
| A **CID-keyed** CFF under `add_cid_font` | `add_cid_font` returns false | Its charset maps a CID onto a glyph and the two are different numbers; `PageBuilder::glyphs` addresses glyphs, and `/Identity-H` would make every one of them a CID (9.7.4.2) | this page |
| Symbol and ZapfDingbats when nothing embeds them | `RenderWarning::UnreadableFont`, in a `bundled-fonts` build too | Liberation has no equivalent, and a text face drawn for a symbolic font puts letters where the document meant arrows | this page |
| A CID the descendant font does not carry | `.notdef` drawn + `RenderWarning::UnreadableFont`; extraction: `TextWarning::UnknownFont` | Drawing whichever glyph the code happens to number is the invisible failure | this page |
| A predefined CMap name outside Adobe's registry | `WarningKind::PredefinedCMapUnknown` | A guessed codespace mis-splits the string, so glyphs *and* advances go wrong silently | [rulings](../rulings.md) ruling 10 |
| Registry CID tables in a `cmap-predefined`-off build | `WarningKind::PredefinedCMapApproximate`, `CMap::is_approximate` | Codespaces still ship (4.6 KB) so strings split right; the CIDs are admitted guesses | this page |
| A `usecmap` parent that cannot be resolved | `WarningKind::CMap(cmap::Warning::ParentUnresolved)` | The child keeps what it declared itself rather than inheriting from nothing | 9.7.5.3 |
| A `usecmap` chain past 4 links, or one that revisits a source | `cmap::Warning::ParentChainCapped`, `cmap::Warning::ParentCycle` | The names come out of the document; being finite is the property that matters (ruling 1) | [rulings](../rulings.md) |
| A truncated CMap mapping section | `cmap::Warning::SectionUnterminated(Section)` | What parsed is kept and the section is named, so partial coverage is visible | [rulings](../rulings.md) ruling 10 |
| TrueType and Type 2 hinting | none — outlines are unhinted by design, not degraded | The bytecode interpreter makes small text differently wrong; subset output keeps `cvt `/`fpgm`/`prep` for readers that disagree | this page |

## Verified

- `crates/tinker-pdf/tests/cff_fonts.rs` — CFF glyph selection: charset over
  code, string INDEX, built-in encodings, CID-keyed `ROS`/FDArray/FDSelect.
- `crates/tinker-pdf-font/src/cff_subset/tests.rs` — 21 tests over fonts built
  byte by byte: local and global subroutine renumbering, a global subroutine
  reached from two Font DICTs, `hintmask` counting stems a subroutine declared,
  `seac` components kept, and the four refusals. Two of them cross a **bias
  threshold**: 1 300 local subroutines cut to one (1 131 → 107), and 34 000 cut
  to 1 300 (32 768 → 1 131), the second keeping a non-prefix, non-contiguous
  range so the renumbering is the identity nowhere and every operand form
  appears on the new side and none on the old. That test pins the whole
  rewritten charstring against a byte string derived from the renumbering rule
  rather than read back from the subsetter.
- `crates/tinker-pdf-cos/tests/cff_subsetting.rs` — the writer end: the 9.6.4
  tag, the Table 126 descriptor entry for each of the three shapes, `/W` from
  the original program, and each `SubsetRefusal` reported by name.
- `crates/tinker-pdf/tests/cff_subset_census.rs` — every CFF face in the
  fetched corpora cut to nine glyphs: 297 files, 441 faces (222 CID-keyed, 200
  bare simple, 19 `OpenType/CFF`), 439 rebuilt and 2 refused, 14.5 MB of font
  program down to 1.49 MB, and **zero divergences** — every retained glyph's
  outline, advance and font matrix, every glyph's name, every CID's glyph and
  every one of the 256 codes identical to the original's.
- **Counted injection over the CFF subsetter** (`docs/verification.md`'s house
  practice). Each defect put back, and the assertions that fire. The writer
  also verifies its own output by outlining every retained glyph, so each row
  was run twice — with that self-check on and off — to separate what the tests
  catch from what the writer catches. The counts were the same both ways, so
  nothing here depends on the self-check:

  | defect reintroduced | unit (21) | writer (9) | census (1) |
  |---|---|---|---|
  | none | 0 | 0 | 0 |
  | the operand is recomputed with the **old** bias | 2 | **0** | 1 |
  | one global subroutine left out of the rebuilt INDEX | 2 | **0** | 1 |
  | the operand names the subroutine's **old** index | 7 | 3 | 1 |

  The two zeroes are the finding. The writer's fixtures carry 26 local
  subroutines and no global ones, so their bias never changes and there is no
  global INDEX to damage — exactly the "a test that only uses small fonts never
  crosses a threshold" hole. The two bias-threshold fixtures are the only
  things in the suite that close it, and the corpus census is the only thing
  that closes it over fonts nobody here wrote. A third finding came out of the
  same run: the 33 900 fixture originally kept subroutines 0..1299, a *prefix*
  of the original numbering that renumbers onto itself, and reintroducing "use
  the old index" changed nothing there — it now keeps every third subroutine
  from 20 000 up, and catches that defect too.
- `crates/tinker-pdf/tests/composite_fonts.rs` — the CID selects glyph and
  advance together; `/CIDToGIDMap` in both forms; `.notdef` plus report for a
  CID the font does not carry.
- `crates/tinker-pdf/tests/cmap_inheritance.rs` — `usecmap` and `/UseCMap`
  merge order, the depth cap, and cycle refusal, at the rendered-page level.
- `crates/tinker-pdf/tests/predefined_cmap_rendering.rs` and
  `predefined_cmap_warnings.rs` — registry CMaps choose the glyphs, and a
  name outside the set is warned about in both spellings.
- `crates/tinker-pdf-font/tests/predefined_cmaps.rs` — 805 632 range bounds
  and 474 codespace bounds (all 237 declared codespaces) checked against
  Adobe's own text through a second, deliberately different reader, because a
  round trip through the engine's encoder proves only self-agreement;
  `predefined_cmaps_absent.rs` pins the `cmap-predefined`-off behaviour.
- `crates/tinker-pdf/tests/type3_fonts.rs` — glyph procedures run as content
  under `/FontMatrix`; `vertical_metrics.rs` — `/W2` both forms, the `/DW2`
  default, and the position vector; `substitute_fonts.rs` — the
  `FontProvider` seam, including declining symbolic fonts.
- Fuzzing: five of the 24 fuzz targets exercise this feature — `cff`,
  `cmap`, `sfnt`, `truetype`, `type1` (ruling 1).
- Determinism: the `text` fixture among the 15 render fingerprints in
  `crates/tinker-pdf/tests/determinism.rs` embeds a synthetic six-glyph face
  built in the test itself and pins glyph rasterisation bit-for-bit across
  targets, asserting a least-ink floor so a face that stops drawing cannot
  read as a pass ([determinism](determinism.md)); the `epub` fixture renders
  through `SimpleFontProvider`, covering the provider path.
- The whole workspace stands at 2 963 passed / 0 failed / 8 ignored
  (Windows x86_64, August 2026), and the corpus run — 4 525 files, 4 484
  rendered every page, 0 crashes — exercises real embedded fonts of every
  kind here. See [verification](../verification.md).
