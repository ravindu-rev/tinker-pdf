# Fonts

Every font format a PDF can carry is parsed in `crates/tinker-pdf-font` —
bytes in, metrics and outlines out, no PDF types on its API (ruling 8,
[rulings](../rulings.md)) — and the facade binds those programs to font
dictionaries, encodings and CMaps. The engine bundles no font programs and
reads no directories, by policy: bundling a face is a licensing decision and
reading a directory is an operating-system dependency `wasm32-unknown-unknown`
does not have. A document that embeds its fonts needs nothing else; one that
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
embed TrueType programs with widths taken from the program's own `hmtx`, and
`set_subset_fonts` (on by default) cuts each program down to the glyphs the
pages drew: `glyf`/`loca` rebuilt, composite closures followed, glyph
identifiers never renumbered (a dropped glyph becomes a zero-length `loca`
entry, so `/Widths`, `/W`, `cmap` and `/CIDToGIDMap` all stay right), hinting
tables kept for readers that interpret them, and the 9.6.4 six-letter tag
prefixed to `/BaseFont`. A subset that cannot be built embeds the whole face
instead — larger and correct (ruling 2).

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

## Refused by name

| What | Typed variant | Why (one line) | See |
|---|---|---|---|
| Shaping: GSUB/GPOS, kerning, bidi — advances are per-character from `/Widths`/`/W` | none — layout uses the file's own advances; nothing is dropped, so nothing warns | Long a stated non-goal, since overturned: the roadmap stages a shaping leaf crate | [ROADMAP](../ROADMAP.md) |
| CFF subsetting on write | `tinker_pdf_font::subset` answers `None`; the whole face is embedded | A CFF subset needs its charstring INDEX rebuilt, and a broken subset renders *almost* right | [ROADMAP](../ROADMAP.md) |
| Bundled fallback faces | `RenderWarning::UnreadableFont` when no provider answers | Bundling is a licensing decision and directory reading an OS dependency; the seam is `FontProvider` | this page |
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
- The whole workspace stands at 2 911 passed / 0 failed / 8 ignored
  (Windows x86_64, August 2026), and the corpus run — 4 525 files, 4 484
  rendered every page, 0 crashes — exercises real embedded fonts of every
  kind here. See [verification](../verification.md).
