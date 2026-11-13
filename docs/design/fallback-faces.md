# Fallback faces for composite fonts

A composite font that embeds no program names its glyphs by CID, a number in
a character collection the font's `/CIDSystemInfo` names (9.7.3). A face the
engine substitutes for it — a host's, through `FontProvider`, or one the
build carries — numbers its glyphs in an order of its own, so a CID is not an
index into it. This document describes how a substituted face is read for a
composite font, which faces a build carries for the registry's CJK
collections, how they are stored and switched on, and how the corpus counts
the files they are for (ROADMAP FT-01).

## Scope

- Drawing an unembedded composite font in one of the registry's CJK
  collections — Adobe-Japan1, Adobe-GB1, Adobe-CNS1, Adobe-Korea1 and
  Adobe-KR — with a substituted face: a host's, or a bundled OFL face.
- How a substituted face is addressed: a CID through the character it stands
  for, by the collection's inverted `Uni*-UTF32` CMaps or by `/ToUnicode`, or
  through the face's own charset where the face is CID-keyed in the same
  collection; what a CID no route reaches draws and reports; and the readings
  a face with a Unicode `cmap` may take for a simple font's code.
- The character collection carried to the host seam (`FontRequest`), so a
  provider can choose a face by it.
- The candidate faces, one style and one face per region, their storage form,
  where they live against crates.io's package ceiling, the `cjk-fonts`
  feature and the wasm CJK profile.
- The corpus `cjk` capability, and the counts that say whether the face
  helped.

## Non-goals

- The producing paths' CJK characters: laying out and embedding CJK text a
  caller writes is FT-02's lookup by script and language, which adds its own
  section here.
- Text extraction through a collection's characters. A CID's text is what
  `/ToUnicode` or the encoding says, as today; the inverted CMaps draw glyphs
  and are not read for text.
- An embedded program. A composite font that embeds one is drawn by it, as
  today: the document's own program always wins over a substitute.
- Synthesising a style. A bold or italic request is drawn with the regular
  face, as the existing providers fill a missing style.
- Faces for any other script, emoji, mathematics or symbols (FT-02).

## Design

**The collections.** Each of the five has `Uni*-UTF32` CMaps in the vendored
registry (`crates/tinker-pdf-font/data/cmap-resources`): a CMap whose codes
are Unicode scalar values and whose values are the collection's CIDs.

| Collection | Region | Its `Uni*-UTF32` CMaps |
| --- | --- | --- |
| Adobe-Japan1 | Japan | `UniJIS-UTF32`, then `UniJIS2004-UTF32`, `UniJISX0213-UTF32`, `UniJISX02132004-UTF32`, each `-H` and `-V` |
| Adobe-GB1 | Simplified Chinese | `UniGB-UTF32-H`, `-V` |
| Adobe-CNS1 | Traditional Chinese | `UniCNS-UTF32-H`, `-V` |
| Adobe-Korea1 | Korea | `UniKS-UTF32-H`, `-V` |
| Adobe-KR | Korea | `UniAKR-UTF32-H` |

**The inverted table.** For a collection and a writing mode, every CID its
CMaps reach is mapped back to the character that reaches it: the CMaps in
the order above, each with its `usecmap` parent (a `-V` CMap inherits its
`-H`), and a CID that several characters reach keeps the lowest — which puts a
unified ideograph before its compatibility form, which lies higher in the
code space. A CID the first CMap does not reach is taken from the next, so
Adobe-Japan1's JIS X 0213 additions are reached too. The table is built once
per collection and writing mode, on its first use, and kept for the process
behind a `OnceLock`, so a page of ten thousand glyphs inverts nothing twice.
A build without `cmap-predefined` holds no CID tables, builds no inverted
table, and reaches no CID by this route: it reports rather than guessing.

**The routes, in order.** For each CID a composite font shows, a substituted
face is read by the first route that reaches a glyph in it:

1. **The face's own charset**, where the face is CID-keyed — a CFF program
   with a `ROS` — in the font's registry and ordering. Its charset maps the
   CID to the glyph, as an embedded CID-keyed program's does. A CID past the
   face's own supplement is not in its charset and falls to the next route.
2. **The collection's inverted table for the font's writing mode**: the CID's
   character, and the face's Unicode `cmap` for that character. A
   vertical-variant CID is reached through the `-V` table, which holds the
   vertical forms; in a vertical font every glyph so reached is replaced by
   the face's `vert` single substitution, `vrt2` where the face has it
   (tinker-pdf-shape's GSUB reader), and drawn upright where the face has
   neither. Adobe-KR has no `-V` CMap, and a vertical KR font reads its `-H`
   table and the face's `vert`.
3. **`/ToUnicode`**: the code's text, where it is one character, and the
   face's `cmap` for it. A code whose text is several characters — a ligature
   — names no one glyph and falls through.
4. **Nothing**: the face's glyph 0, `.notdef`, and `UnreadableFont` for the
   font, as an embedded program's unmapped CID reports.

**Which route wins when a font has both `/ToUnicode` and a collection.** The
collection's. The registry's CMaps are the published statement of which
character each CID is, complete for the supplement vendored; a `/ToUnicode`
says what a reader should copy, is often partial in producer output, maps
ligatures to strings no one glyph draws, and maps a vertical variant to its
horizontal character. So routes 1 and 2 come before 3, and `/ToUnicode`
serves the font whose collection says nothing: `Adobe-Identity`, or a
collection the registry does not hold.

**Never the CID as a glyph index.** Ruling 23 answers FT-01.3's question:
a substituted face is read through the font's collection, and an
`Adobe-Identity` font is no collection, so an Identity-ordered CIDFontType2
with no `/ToUnicode` draws `.notdef` and reports `UnreadableFont`, even where
the host's face is the document's own font. A face with no `cmap` at all — the
synthetic face `cargo xtask corpus-run --fonts synthetic` measures the corpus
with (`xtask/src/face.rs`), and the test faces — has no characters to be
addressed by and keeps today's reading, the CID as its index; every face with
a `cmap` is read by the routes above.

**A simple font's code, in a face with a Unicode `cmap`.** Two readings, and
no third: the code's character — `/ToUnicode`'s, or else its encoding's glyph
name through the glyph list — through the `cmap`; and the code itself as the
`cmap` reads a code, which for a `(3,0)` symbol subtable is the `F0xx`
block. A symbolic font asks its code first and a non-symbolic one its
character first, as today. A code neither reaches draws `.notdef` and
reports; the code as a glyph index is a reading only of a face with no
`cmap`. `PageResources::selection` keeps answering an embedded program alone,
which is what `subset.rs` keeps glyphs of.

**The character collection at the seam.** `tinker-pdf-cos`'s `Font` reads the
descendant's `/CIDSystemInfo` leniently, as rendering reads a font rather than
as PDF/A judges one: `/Registry` and `/Ordering` each a string or, where a
producer wrote one, a name, and `/Supplement` an integer, or none where it is
missing or is not one. A descendant with no readable dictionary, or one with
no readable registry or ordering, carries no collection, and neither does a
simple font. `FontRequest` carries what was read, so a provider can choose a
face by collection; `SimpleFontProvider` ignores it.

**One face per region, one style.** The five collections share code points
and differ in glyph forms: the same Han character is drawn one way for a
Japanese reader and another for a Chinese one, and a form from the wrong
region is a wrong character to the reader it reaches. So a build carries one
face per region — Japan for Adobe-Japan1, Simplified Chinese for Adobe-GB1,
Traditional Chinese for Adobe-CNS1, Korea for Adobe-Korea1 and Adobe-KR — and
a collection is answered with its region's face. One style: a sans-serif
(gothic, hei) Regular. A document's serif CJK face drawn sans keeps every
character legible at every size and resolution, where a serif face's thin
strokes break up at a low one, and one style keeps the profile to four
faces.

**The candidates FT-01.4 measures.** Each is fetched once to be measured and
never committed, and each carries a licence the allowlist admits (ruling 18):

| Candidate | Regions | Upstream | SPDX |
| --- | --- | --- | --- |
| Source Han Sans, Regular: the language-specific OTFs and the Pan-CJK collection | all four | `github.com/adobe-fonts/source-han-sans` | `OFL-1.1` |
| Noto Sans CJK, Regular: the language-specific OTFs and the collection | all four | `github.com/notofonts/noto-cjk` | `OFL-1.1` |
| BIZ UDGothic, Regular | Japan | `github.com/googlefonts/morisawa-biz-ud-gothic` | `OFL-1.1` |
| Nanum Gothic, Regular | Korea | `github.com/google/fonts`, `ofl/nanumgothic` | `OFL-1.1` |
| Droid Sans Fallback | all four, one face | `android.googlesource.com/platform/frameworks/base`, `data/fonts` | `Apache-2.0` |

For each, FT-01.4 counts the CIDs of each collection and supplement the routes
reach in it, which also says which route each candidate's glyphs take. FT-01.8
vendors, per region, the candidate that reaches the most of its collection's
CIDs, and of two that reach as many the smaller in the stored form; a
collection of faces that share glyphs across regions is weighed as one
storage of the four.

**The storage form.** Three are weighed: the program as published; the
program deflated, inflated by `tinker-pdf-filters` on first use; and WOFF2,
which `woff.rs` and the in-tree Brotli decoder already read. FT-01.4 measures
each form's bytes and the time to make it ready on first use. FT-01.8 stores
the smallest; whatever it is, a face is made ready once a process, behind a
`OnceLock`, and the ready program is shared by every request after.

**Where the faces live.** `tinker-pdf-font`'s package already holds the CMap
registry and the Liberation faces, and crates.io refuses a package over
`CRATES_IO_CEILING` (`xtask/src/release.rs`). The faces go under
`crates/tinker-pdf-font/data/cjk` where FT-01.4's packaged size of the font
crate with them stays under the ceiling, and otherwise into a data crate of
their own, `tinker-pdf-font-cjk`, which `tinker-pdf-font` depends on through
`cjk-fonts`; a region whose face alone passes the ceiling gets a crate of its
own. A data crate is a new edge for `cargo xtask dag` and a manifest for
`cargo xtask versions`, and either home is a vendored tree with its licence
file and a `THIRDPARTY.md` row.

**The feature and the profile.** `cjk-fonts`, in `tinker-pdf-font`, the facade
and `bindings/js`, carries the faces; `bundled-fonts` turns it on in
`tinker-pdf-font` and the facade, so a bundled build answers CJK requests
after the host's provider and before the Latin families, whatever the
descriptor's symbolic flag says (FT-01.9). The wasm CJK profile is the
`bindings/js` build with `cjk-fonts`, held by `cargo xtask footprint` at its
own figure and tolerance (ruling 19); the default profile carries no face.
Because `bundled-fonts` turns `cjk-fonts` on, the `bundled-fonts` profile
carries the faces too, and its figure grows by them.

**The `cjk` capability.** `tpdf probe`'s object scan prints `cap cjk` for a
file holding a composite font whose descendant's `/CIDSystemInfo` reads, as
the seam reads it, registry `Adobe` and one of the five orderings, and whose
descendant's descriptor names no `/FontFile`, `/FontFile2` or `/FontFile3`. A
font that embeds a program, an `Identity` ordering and a simple font are not
`cjk`. `corpus-run` counts, per capability, the files that rendered with
something reported, which is the figure FT-01's exit falls from.

## Milestones

1. **FT-01.2: the collection at the seam.** Exit: `tinker-pdf-cos`
   `tests/fonts.rs` `a_composite_fonts_character_collection_is_read` — the
   three entries from the descendant, a name where a string is due, a
   missing or non-integer supplement, and a simple font answering `None`;
   `substitute_fonts.rs` `the_provider_is_told_the_character_collection`, a
   recording provider asked with `Adobe`, `Japan1` and the supplement, in
   the default and `bundled-fonts` builds.
2. **FT-01.3: a substituted face read by character.** Exit: `substitute_fonts.rs`
   `a_substituted_face_draws_a_composite_font_by_character_not_by_cid` (a
   face whose `cmap` gives each character its own box and whose glyph at each
   CID's index is a decoy), `an_unreached_cid_draws_notdef_and_reports` and
   `the_collection_is_inverted_once`; `determinism.rs` pins the page; the
   default, `bundled-fonts` and `--no-default-features` builds.
3. **FT-01.4: the price.** Exit: `crates/tinker-pdf-font/tests/cjk_coverage.rs`
   `cjk_face_coverage_census` (ignored) reads each candidate from
   `TINKER_CJK_FACES` and prints `RAN` with a row per face and collection, or
   `SKIPPED`; its table is committed beside the binary, package and wasm
   sizes.
4. **FT-01.5: the capability.** Exit: `tools/tpdf/src/main.rs`
   `an_unembedded_cjk_font_is_the_cjk_capability`.
5. **FT-01.6: counts per capability.** Exit: `xtask/src/report.rs`
   `degraded_files_are_counted_per_capability`, `xtask/src/ratchet.rs`
   `a_capability_whose_degraded_share_rises_is_named` and
   `a_bar_without_capability_counts_still_reads`; `cargo xtask check`.
6. **FT-01.7: the figure before the face.** Exit: the three local corpus runs
   green to the end, their `cjk` counts in the three ratchet files.
7. **FT-01.8: the faces vendored.** Exit: `bundled.rs`
   `the_cjk_face_is_compiled_in_and_reads` and
   `the_cjk_face_is_made_ready_once`; `cargo xtask vendor`, `cargo deny
   check licenses bans sources`, and `release.rs`'s
   `a_crate_that_vendors_data_publishes_it` and
   `the_cmap_registry_is_in_the_font_crates_package`.
8. **FT-01.9: the bundled faces answer.** Exit: `substitute_fonts.rs`
   `a_bundled_build_draws_unembedded_cjk_text_in_every_collection`,
   `a_cjk_request_is_never_answered_with_a_latin_face`,
   `without_the_cjk_profile_unembedded_cjk_text_is_reported`, and
   `a_host_provider_is_asked_first_and_the_bundled_faces_only_after` extended
   to a CJK request.
9. **FT-01.10: the profile's footprint.** Exit: a local MS-06 footprint check
   green with a CJK profile row at its committed figure and the default
   profile's figure unmoved.
10. **FT-01.11: the speed row.** Exit: `xtask/src/bench.rs`
    `the_committed_baseline_agrees_with_the_tree` with the first-render and
    second-render operations, and `cargo bench -p tinker-pdf --features
    bundled-fonts` reporting their times.
11. **FT-01.12: the figure after the face.** Exit: the three local corpus
    runs green, the bundled run's `cjk` files rendered with something
    reported below FT-01.7's figure, recorded in
    `corpus/ratchet-bundled.json` and `features/fonts.md`.

## Risks

| Risk | Mitigation |
| --- | --- |
| A page a host face drew by CID index, correctly because the face was the document's own, now draws `.notdef` | Ruling 23 takes that price: the reading by index is right only by coincidence, and the report says the glyphs are missing rather than drawing a guess |
| Pages a bundled Latin face drew as wrong glyphs now report instead | That is the point; FT-01.7's counts are taken after FT-01.3 so the figure the face must lower is the honest one |
| Several characters reach one CID, and the lowest is not the form a face draws best | The lowest is a unified ideograph before its compatibility form; a face draws the unified form, which is the one the collection's CID stands for |
| A face's `vert` substitutes a form that differs from the collection's vertical variant | The vertical variant's character is drawn in the face's vertical form, which is that face's statement of the same glyph; a face with no `vert` draws it upright and the line still advances by `/W2` |
| A build without `cmap-predefined` has no tables to invert | It reaches no CID by route 2 and reports; nothing is guessed |
| The faces pass crates.io's package ceiling | A data crate of their own, and a crate a region where one face passes it alone |
| The `bundled-fonts` wasm profile grows by the faces | Its figure is re-recorded at FT-01.10 at the price ruling 19 leaves with the owner; the default profile carries no face |
| A region's best face lacks CIDs of a later supplement | FT-01.4 counts the CIDs each candidate reaches per supplement, and a CID no route reaches draws `.notdef` and reports |
