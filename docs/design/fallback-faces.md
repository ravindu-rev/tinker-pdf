# Fallback faces

A composite font that embeds no program names its glyphs by CID, a number in
a character collection the font's `/CIDSystemInfo` names (9.7.3). A face the
engine substitutes for it — a host's, through `FontProvider`, or one the
build carries — numbers its glyphs in an order of its own, so a CID is not an
index into it. This document describes how a substituted face is read for a
composite font, which faces a build carries for the registry's CJK
collections, how they are stored and switched on, and how the corpus counts
the files they are for (ROADMAP FT-01); and, in its last section, the faces a
build carries for every other script, emoji, mathematics, music and symbols,
and the lookup by script and language that chooses among them (ROADMAP
FT-02).

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
CMaps reach is mapped back to the characters that reach it: the CMaps in the
order above, each with its `usecmap` parent (a `-V` CMap inherits its `-H`,
so a vertical table holds the horizontal CIDs too), and a CID the first CMap
does not reach taken from the next, so Adobe-Japan1's JIS X 0213 additions
are reached as well. A CID that several characters reach keeps the best two:
a Kangxi or supplementary radical, or a CJK compatibility ideograph, after
every other character — the radical `U+2F47` reaches Adobe-Japan1's CID for
`日` as `U+65E5` does, and lies below it in the code space — and the lower
code first within each class; a face lacking the first is asked for the
second. The table is built once per collection and writing mode, on its first
use, and kept for the process behind a `OnceLock`, so a page of ten thousand
glyphs inverts nothing twice. A build without `cmap-predefined` holds no CID
tables, builds an empty table, and reaches no CID by this route: it reports
rather than guessing.

**The routes, in order.** A code reaches a CID only through the font's
encoding CMap: a code it maps to none, which the advance reads as the code
itself, names no CID here and draws `.notdef`, reported. For each CID a
composite font shows, a substituted face is read by the first route that
reaches a glyph in it:

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
character first, as an embedded program's codes are asked. A code neither
reaches draws `.notdef` and reports; the code as a glyph index is a reading
only of a face with no `cmap`. A CFF-flavoured face is read by the name the
code's encoding gives, in its charset, as an embedded CFF is: a name stands
for its character. `PageResources::selection` answers an embedded program
alone, which is what `subset.rs` keeps glyphs of.

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

| Candidate | Regions | Files | Upstream | SPDX |
| --- | --- | --- | --- | --- |
| Source Han Sans, Regular: the language-specific OTFs | one each, Hong Kong's beside Traditional Chinese's | `SourceHanSansJP-Regular.otf`, `SourceHanSansKR-Regular.otf`, `SourceHanSansCN-Regular.otf`, `SourceHanSansTW-Regular.otf`, `SourceHanSansHK-Regular.otf` | <https://github.com/adobe-fonts/source-han-sans>, release `2.005R`, `SubsetOTF/` | `OFL-1.1` |
| Source Han Sans, Regular: the Pan-CJK collection | all four | `SourceHanSans-Regular.ttc` | the same release, `OTC/` | `OFL-1.1` |
| Noto Sans CJK, Regular: the language-specific OTFs | one each, Hong Kong's beside Traditional Chinese's | `NotoSansJP-Regular.otf`, `NotoSansKR-Regular.otf`, `NotoSansSC-Regular.otf`, `NotoSansTC-Regular.otf`, `NotoSansHK-Regular.otf` | <https://github.com/notofonts/noto-cjk>, release `Sans2.004`, `Sans/SubsetOTF/` | `OFL-1.1` |
| Noto Sans CJK, Regular: the collection | all four | `NotoSansCJK-Regular.ttc` | the same release, `Sans/OTC/` | `OFL-1.1` |
| BIZ UDGothic, Regular | Japan | `BIZUDGothic-Regular.ttf` | <https://github.com/googlefonts/morisawa-biz-ud-gothic>, release `v1.051`, `fonts/ttf/` | `OFL-1.1` |
| Nanum Gothic, Regular | Korea | `NanumGothic-Regular.ttf` | <https://github.com/google/fonts>, `ofl/nanumgothic/` | `OFL-1.1` |
| Droid Sans Fallback, and its full build | all four, one face | `DroidSansFallback.ttf`, `DroidSansFallbackFull.ttf` | <https://android.googlesource.com/platform/frameworks/base>, `data/fonts/` | `Apache-2.0` |

For each, FT-01.4 counts the CIDs of each collection and supplement the routes
reach in it, which also says which route each candidate's glyphs take. The
rule FT-01.4's figures were read by: per region the candidate that reaches the
most of its collection's CIDs, and of two that reach as many the smaller in
the stored form, a collection of faces that share glyphs across regions
weighed as one storage of the four.

**The faces vendored.** By that rule the Source Han Sans collection is every
region's face, and it passes crates.io's ceiling in every form it can be
stored in, so ruling 23 answers FT-01.8 with the region files instead: Source
Han Sans JP for Adobe-Japan1, CN for Adobe-GB1, TW for Adobe-CNS1 — which
reaches as many of its CIDs as the Hong Kong file and is the smaller — and KR
for Adobe-Korea1 and Adobe-KR. The CIDs the collection reaches and its region
files do not, which `cjk_coverage.tsv`'s rows for the collection and for the
region files state, are the price ruling 23 records.

**The storage form.** Three are weighed: the program as published; the
program deflated, inflated by `tinker-pdf-filters` on first use; and WOFF2,
which `woff.rs` and the in-tree Brotli decoder already read. FT-01.4 measures
each form's bytes and the time to make it ready on first use, and WOFF2 is
the smallest, so each face is stored as WOFF2. A face is made ready once a
process, behind a `OnceLock` (`tinker_pdf_font::bundled::cjk_face`), on its
region's first request, and the ready program is shared by every request
after.

The WOFF2 files are the reference encoder's, `woff2_compress`, named with its
version in `THIRDPARTY.md` (ruling 23), and nothing outside the engine vouches
for them: `crates/tinker-pdf/tests/cjk_faces.rs` unpacks each with `woff.rs`
and holds every table it gives back to the published face's, by
`cjk_faces.tsv`, each published table's length and SHA-256. WOFF2 §5 changes
three things and the test holds each: an encoder removes `DSIG` and sets bit
11 of `head`'s flags, and a decoder recalculates `head`'s
`checkSumAdjustment`. Everything else is the published face's byte for byte,
so the font data are unchanged apart from the compression, which is what
lets each face keep the name its licence reserves.

**Where the faces live.** crates.io refuses a package over
`CRATES_IO_CEILING` (`xtask/src/release.rs`), and the four region faces
together pass it, so each region's face is the only data of a crate of its
own (ruling 23): `tinker-pdf-font-cjk-jp`, `-cn`, `-tw` and `-kr`, each
holding its WOFF2 file and the OFL text it travels with under
`data/source-han-sans` and exposing the file as one `static`. `release.rs`'s
`each_cjk_face_crate_publishes_its_face_under_the_ceiling` holds each
package to carrying both and to the ceiling. `tinker-pdf-font` depends on the
four through `cjk-fonts`, optional edges `cargo xtask dag` declares (the
fifteenth amendment), each crate is a manifest `cargo xtask versions` reads,
and each `data/` tree is a vendored tree with its licence file and a
`THIRDPARTY.md` row.

**The feature and the profile.** `cjk-fonts`, in `tinker-pdf-font`, the facade
and `bindings/js`, carries the faces; `bundled-fonts` turns it on in
`tinker-pdf-font` and the facade, so a bundled build answers CJK requests
after the host's provider and before the Latin families, whatever the
descriptor's symbolic flag says (FT-01.9). The wasm CJK profile is the
`bindings/js` build with `cjk-fonts`, held by `cargo xtask footprint` at its
own figure and tolerance (ruling 19); the default profile carries no face.
Because `bundled-fonts` turns `cjk-fonts` on, the `bundled-fonts` profile
carries the faces too, and its figure grows by them.

**The census** (FT-01.4) prices the choice and makes none.
`crates/tinker-pdf-font/tests/cjk_coverage.rs`, `#[ignore]`d,
`cjk_face_coverage_census`, reads each file of the candidate table from the
directory `TINKER_CJK_FACES` names, and a candidate's WOFF2 form, made once
by a compressor outside the engine, from the same directory under the file's
stem with `.woff2`. It prints `RAN` and three tables, each row led by its
table's name, or `SKIPPED` where the variable names no directory or the build
holds no CID tables:

- `inversion`: each collection's horizontal and vertical tables, the CIDs
  each reaches and the time its first use takes to build it, taken before any
  face asks for one;
- `coverage`: a row for each face of each file — a collection file's faces in
  turn — each collection and each supplement, then the collection's `all`
  row: the CIDs the supplement adds, read from the registry's own
  `Adobe-<ordering>-<n>` CMaps with CID 0 left out, and in each writing mode
  the CIDs the face's own charset reaches, those its characters reach through
  its `cmap`, those the index reading reaches in a face with neither, and
  those nothing reaches. These are routes 1 and 2, in their order and through
  the functions `resources.rs` draws with; route 3 is a document's, not a
  face's;
- `storage`: a row for each file: its bytes as published, deflated at
  `tinker-pdf-filters`' ratio level and as WOFF2, the least of five times each
  form takes to be made ready — unpacked to the program, parsed, and its first
  character looked up — and the build's profile.

It runs in release and in debug, whose counts agree and whose times are each
its own:

```text
TINKER_CJK_FACES=<directory> cargo test -p tinker-pdf-font --release --test cjk_coverage -- --ignored --nocapture
TINKER_CJK_FACES=<directory> cargo test -p tinker-pdf-font --test cjk_coverage -- --ignored --nocapture
```

Three sizes are measured beside it by building, each candidate in each storage
form carried by a scratch feature that is never committed: `tinker-pdf-font`
holds the form by `include_bytes!` and the facade's `BundledFonts` answers a
request in one of the five collections with it, so no linker drops it.

- The `tpdf` release binary: `cargo build --release -p tpdf --features
  bundled-fonts`, against the same build without a candidate.
- The packaged `tinker-pdf-font`, the form under
  `crates/tinker-pdf-font/data/cjk`: the `.crate` that `cargo package -p
  tinker-pdf-font --no-verify --allow-dirty` writes under `target/package/`,
  against `CRATES_IO_CEILING`.
- The CJK profile's wasm, built as `release.yml` builds the default one,
  `wasm-pack build --release --target web --out-dir pkg bindings/js`, with the
  feature forwarded, and gzipped as it gzips it, `gzip -9 -c
  bindings/js/pkg/tinker_pdf_js_bg.wasm`, against the default profile's.

**Where the figures go** (ruling 16). The census's counts and sizes are
`crates/tinker-pdf-font/tests/cjk_coverage.tsv`, beside it: every row it
prints, in its order, less the columns that hold a time and the profile a
time was taken in, since a count or a size is the same in a release and a
debug build. Its head names the command, and `the_committed_census_is_whole`
holds the record whole and adding up: every candidate's faces, collections
and supplements, each writing mode's counts summing to the row's CIDs and
each `all` row to its supplements. A time is the named machine's (ruling
15): the release and debug runs there give the `inversion` and `storage` rows
their times. The three sizes go to
`crates/tinker-pdf-font/tests/cjk_sizes.tsv`, with the columns `file`,
`form`, `tpdf_release_bytes`, `crate_bytes` and `wasm_gzip_bytes`, a row for
each candidate and form and one, `none`, for the builds without a candidate.
Each run's revision, machine and day go in the message of the commit that
records it.

**The `cjk` capability.** `tpdf probe`'s object scan prints `cap cjk` for a
file holding a composite font whose descendant's `/CIDSystemInfo` reads, as
the seam reads it, registry `Adobe` and one of the five orderings, and whose
descendant's descriptor names no `/FontFile`, `/FontFile2` or `/FontFile3`
reference, which is what the seam reads as an embedded program. A font that
embeds a program, an `Identity` ordering, another registry and a simple font
are not `cjk`. The record's version does not move, since the runner reads
every `cap` line by its name, whatever the name. `corpus-run` counts, per
capability, the files that rendered with something reported, prints them
beside the capability table and records them in each bar as
`degraded_capabilities`, held as `degraded` is: the `cjk` count is the figure
FT-01's exit falls from.

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
   `TINKER_CJK_FACES` and prints `RAN` with a row per face, collection and
   supplement, or `SKIPPED`; its instruments are held by
   `each_collections_supplements_are_the_registrys`,
   `the_census_counts_each_cid_by_its_route`,
   `a_face_is_tallied_supplement_by_supplement`,
   `each_face_of_a_collection_is_read_as_its_own`,
   `each_storage_form_is_measured_as_it_is_made_ready`,
   `a_faces_name_is_read_windows_english_first` and
   `every_candidate_is_the_designs`, and without the tables by
   `without_the_tables_no_supplement_is_read`. Its counts and sizes are
   `cjk_coverage.tsv`, held by `the_committed_census_is_whole`; the three
   sizes go to `cjk_sizes.tsv`.
4. **FT-01.5: the capability.** Exit: `tools/tpdf/src/main.rs`
   `an_unembedded_cjk_font_is_the_cjk_capability`.
5. **FT-01.6: counts per capability.** Exit: `xtask/src/report.rs`
   `degraded_files_are_counted_per_capability`, `xtask/src/ratchet.rs`
   `a_capability_whose_degraded_share_rises_is_named` and
   `a_bar_without_capability_counts_still_reads`; `cargo xtask check`.
6. **FT-01.7: the figure before the face.** Exit: the three local corpus runs
   green to the end, their `cjk` counts in the three ratchet files.
7. **FT-01.8: the faces vendored.** Exit: `bundled.rs`
   `the_cjk_face_is_compiled_in_and_reads` (each face parses and covers the
   collections FT-01.4 counted) and `the_cjk_face_is_made_ready_once`;
   `crates/tinker-pdf/tests/cjk_faces.rs`
   `each_cjk_face_hashes_to_the_sha256_thirdparty_records`,
   `each_cjk_face_holds_every_table_of_the_published_face` and
   `the_record_is_whole`, and `the_record_is_the_published_faces` where the
   published faces are fetched; `cargo xtask vendor`, `cargo deny check
   licenses bans sources`, and `release.rs`'s
   `a_crate_that_vendors_data_publishes_it`,
   `the_cmap_registry_is_in_the_font_crates_package` and
   `each_cjk_face_crate_publishes_its_face_under_the_ceiling`.
8. **FT-01.9: the bundled faces answer.** Exit: `substitute_fonts.rs`
   `a_bundled_build_draws_unembedded_cjk_text_in_every_collection`,
   `a_cjk_request_is_never_answered_with_a_latin_face`,
   `without_the_cjk_profile_unembedded_cjk_text_is_reported`,
   `without_the_tables_the_cjk_faces_draw_no_cid_by_guess`, and
   `a_host_provider_is_asked_first_and_the_bundled_faces_only_after` extended
   to a CJK request; the facade's `fonts.rs`
   `a_cjk_collection_is_answered_by_its_regions_face` and
   `a_cjk_request_is_answered_by_no_latin_face`.
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
| Several characters reach one CID, a radical or a compatibility ideograph among them, and a face draws that one in another form or not at all | Radicals and compatibility ideographs rank after every other character, and the two best are kept, so a face lacking the first is asked for the second |
| A face's `vert` substitutes a form that differs from the collection's vertical variant | The vertical variant's character is drawn in the face's vertical form, which is that face's statement of the same glyph; a face with no `vert` draws it upright and the line still advances by `/W2` |
| A build without `cmap-predefined` has no tables to invert | It reaches no CID by route 2 and reports; nothing is guessed |
| The faces pass crates.io's package ceiling | A crate a region, each holding its region's file; the collection the census prefers passes the ceiling in every form, and the CIDs only it reaches are the price ruling 23 records |
| The WOFF2 encoder changes the font data beyond the compression, and the face loses the name its licence reserves | `cjk_faces.rs` holds every table each file unpacks to against the published face's, and names the three changes WOFF2 §5 makes |
| The `bundled-fonts` wasm profile grows by the faces | Its figure is re-recorded at FT-01.10 at the price ruling 19 leaves with the owner; the default profile carries no face |
| A region's best face lacks CIDs of a later supplement | FT-01.4 counts the CIDs each candidate reaches per supplement, and a CID no route reaches draws `.notdef` and reports |

## Every script, emoji, mathematics, music and symbols (FT-02)

FT-01's faces answer a composite font of the registry's CJK collections.
This section extends the build's faces to every character an admitted face
can draw: a face for each script one covers, and faces for emoji,
mathematics, music and other symbols, chosen a character at a time by a
lookup the producing path and the reading path both ask (ROADMAP FT-02).

### Scope

- The lookup: a character, the script of the run it is in, and a language
  tag or a character collection where the caller has one, in; the face the
  build carries to draw it with, or none, out.
- The faces: one candidate a script, by the Unicode Character Database's
  `Script` property, and the candidates for the emoji, mathematics, music and
  symbol classes; the groups that carry them, each a feature and a wasm
  profile; their storage, their homes and their licences.
- The two paths that ask the lookup: the producing path, which lays out a
  book's, a Markdown document's or an HTML document's text and embeds the
  faces it draws with (`epub/paint.rs`), and the reading path, which draws a
  glyph of an unembedded font that no font-level substitute covers
  (`resources.rs`).
- Which scripts the reading path draws without shaping, and which it reports
  by name.
- The census that prices the faces, the page-per-script proof, and what "no
  `.notdef`" is measured by.

### Non-goals

- **Shaping in the reading path.** A PDF's producer placed every glyph, and
  the reading path draws a glyph a character where the producer put it,
  unshaped, as `features/fonts.md`'s refusal table keeps permanent. A script
  whose text that would draw wrongly is reported by name instead.
- **A second style.** One style, Regular, in a sans-serif design where the
  family has one, as FT-01's faces.
- **Colour emoji.** The emoji group carries a monochrome outline face, drawn
  on both paths as every outline is. Whether a book draws its emoji in colour
  is H-141's (a), and a colour face joins the group only on its ruling.
- **A face under a licence `deny.toml` does not admit** (ruling 18).
- **A script no admitted face covers.** Each is named below, and its
  characters are counted as uncovered, never drawn from a face that lacks
  them.
- **Han, Hiragana, Katakana, Hangul and Bopomofo.** FT-01's four CJK faces
  draw them, and the lookup reaches those faces rather than adding others.

### Design

**The classes, from data the tree vendors** (Unicode 17.0.0). A character's
script is `Scripts.txt`'s, through `tinker_pdf_shape::unicode::script`
(`crates/tinker-pdf-shape/data/ucd`). Its classes are read from
`crates/tinker-pdf-layout/data/ucd`: **emoji** is `emoji-data.txt`'s
`Emoji_Presentation`, and `Extended_Pictographic` where U+FE0F follows; a
character followed by U+FE0E asks text presentation and is no emoji;
**mathematics** is `DerivedCoreProperties.txt`'s `Math`; **symbols** are
`DerivedGeneralCategory.txt`'s `So` and `Sm`, and `No` of the `Common`
script — Mayan numerals and the Siyaq numbers among them. No property names
music: a musical symbol is a symbol, reached through the symbol faces' fixed
order below, and the census counts the four musical blocks — Musical
Symbols, Byzantine Musical Symbols, Ancient Greek Musical Notation and
Znamenny Musical Notation — by the names `UnicodeData.txt` gives their
characters (`MUSICAL SYMBOL`, `BYZANTINE MUSICAL SYMBOL`, `GREEK VOCAL
NOTATION SYMBOL`, `GREEK INSTRUMENTAL NOTATION SYMBOL`, `ZNAMENNY`).

**The lookup.** For a character `c`, its run's script, and the caller's
language tag or character collection where it has one, the first of these
that names a face whose `cmap` covers `c`:

1. **Emoji presentation**: `c` is an emoji, as above — the emoji face.
2. **The CJK scripts**: `c` is Han, Hiragana, Katakana, Hangul or Bopomofo —
   FT-01's faces, narrowed below.
3. **Its script**: `Scripts.txt`'s, or, where that is `Common` or
   `Inherited`, its run's: the script of the nearest character before it in
   the run that is neither, else of the nearest after it, else Latin — the
   script's face.
4. **Mathematics**: `c` is `Math` — the mathematics face.
5. **Symbols**: `c` is a symbol, as above — the first, in this fixed order,
   of Noto Sans Symbols, Noto Sans Symbols 2, Noto Music, Noto Znamenny
   Musical Notation, Noto Sans Mayan Numerals, Noto Sans Indic Siyaq Numbers,
   Noto Serif Ottoman Siyaq, the mathematics face and the emoji face, which
   draws a pictograph asked in text presentation.
6. **The Latin face**, Noto Sans, for a `Common` character no face before it
   covers: punctuation and spaces, whose shapes no script's face owns.
7. **None.** The character is counted as uncovered, by the path that asked.

A run is what the asking path shapes or shows as one: a shaping run of the
producing path, and the string of one text-showing operator in one font on
the reading path. The result is a function of the character, the run's
script, the language or collection and the features the build carries, so a
build draws a page the same way every time (ruling 4).

**Han, and the scripts whose forms vary by language.** Han's forms differ by
region, so step 2 chooses among FT-01's faces by the caller's BCP 47 tag:
`ja` the Japanese face; `ko` the Korean face; `zh` with the `Hant` script
subtag, or with the region `TW`, `HK` or `MO`, the Traditional Chinese face;
any other `zh` the Simplified Chinese face. Without a tag, the font's
character collection chooses, as `Region::of` reads it for FT-01; with
neither, the four faces are asked in the order of how many of the script's
characters each covers, most first, by FT-02.2's census, and the first that
covers `c` draws it. Hiragana and Katakana prefer the Japanese face, Hangul
the Korean and Bopomofo the Traditional Chinese, before the rest, whatever
the tag. Every other script has one face here, so a language changes its
form only through the face's own `locl` lookups, which the producing path's
shaper runs under the OpenType language system the tag names (FT-15); the
reading path, which does not shape, draws the default forms.

**Where the producing path takes the language.** From the element's computed
style: the `lang` and `xml:lang` FT-15 resolves and carries into layout's
computed style (FT-15.7), read where `paint.rs` shapes a run rather than
carried a second time. A Markdown document has no element language and asks
with none; neither does the builder's own text.

**The reading path, drawn and reported.** The reading path draws a glyph a
character at the producer's position, so it admits a script only where that
draws it right: where no character of it joins its neighbours, forms a
conjunct or is drawn before the character it follows. By the shape crate's
vendored data, a script is **reported** where any character of its own has a
`Joining_Type` of `D`, `L`, `R` or `C` (`DerivedJoiningType.txt`), an
`Indic_Syllabic_Category` of `Virama` or `Invisible_Stacker`
(`IndicSyllabicCategory.txt`), or an `Indic_Positional_Category` of `Left`
(`IndicPositionalCategory.txt`), and three more are reported by name because
a face combines their characters by its own layout tables, which no
property states: Duployan's joining, SignWriting's fills and rotations, and
Egyptian Hieroglyphs' quadrat controls. Every other script is **drawn**.
A reported script's character is not drawn from any face: the page reports
`RenderWarning::ScriptNotDrawn` naming the script, once a font a page, which
`features/fonts.md`'s refusal table lists, and the character's glyph is the
font's own `.notdef`, as an unreached glyph is. The tables below give each
script's side.

**The candidates.** Each is fetched once to be measured and never committed
until FT-02.4 vendors it, and each carries `OFL-1.1`, which `deny.toml`
admits (H-91 ratifies the entry; ruling 18). Every face of the three script
tables is its family's Regular as the Noto project publishes it,
[notofonts/notofonts.github.io](https://github.com/notofonts/notofonts.github.io)
at `3460ae9`, `fonts/<Family>/unhinted/ttf/<Family>-Regular.ttf`, its
family's sans-serif face, or its serif face where the family has no sans.
The groups are UAX #31's three tables of scripts for Unicode 17.0.0
(revision 43): its Recommended Scripts, Limited Use Scripts and Excluded
Scripts, each a feature of its own.

`fallback-scripts`, the Recommended Scripts but `Common`, `Inherited` and the
CJK scripts:

| Face | Scripts | Reading path |
| --- | --- | --- |
| Noto Sans Arabic | Arabic `Arab` | reported |
| Noto Sans Armenian | Armenian `Armn` | drawn |
| Noto Sans Bengali | Bengali `Beng` | reported |
| Noto Sans | Cyrillic `Cyrl`, Greek `Grek`, Latin `Latn` | drawn |
| Noto Sans Devanagari | Devanagari `Deva` | reported |
| Noto Sans Ethiopic | Ethiopic `Ethi` | drawn |
| Noto Sans Georgian | Georgian `Geor` | drawn |
| Noto Sans Gujarati | Gujarati `Gujr` | reported |
| Noto Sans Gurmukhi | Gurmukhi `Guru` | reported |
| Noto Sans Hebrew | Hebrew `Hebr` | drawn |
| Noto Sans Kannada | Kannada `Knda` | reported |
| Noto Sans Khmer | Khmer `Khmr` | reported |
| Noto Sans Lao | Lao `Laoo` | drawn |
| Noto Sans Malayalam | Malayalam `Mlym` | reported |
| Noto Sans Myanmar | Myanmar `Mymr` | reported |
| Noto Sans Oriya | Oriya `Orya` | reported |
| Noto Sans Sinhala | Sinhala `Sinh` | reported |
| Noto Sans Tamil | Tamil `Taml` | reported |
| Noto Sans Telugu | Telugu `Telu` | reported |
| Noto Sans Thaana | Thaana `Thaa` | drawn |
| Noto Sans Thai | Thai `Thai` | drawn |
| Noto Serif Tibetan | Tibetan `Tibt` | reported |

`fallback-scripts-limited`, the Limited Use Scripts but Bopomofo:

| Face | Scripts | Reading path |
| --- | --- | --- |
| Noto Sans Adlam | Adlam `Adlm` | reported |
| Noto Sans Balinese | Balinese `Bali` | reported |
| Noto Sans Bamum | Bamum `Bamu` | drawn |
| Noto Sans Batak | Batak `Batk` | drawn |
| Noto Sans Chakma | Chakma `Cakm` | reported |
| Noto Sans Canadian Aboriginal | Canadian Aboriginal Syllabics `Cans` | drawn |
| Noto Sans Cham | Cham `Cham` | reported |
| Noto Sans Cherokee | Cherokee `Cher` | drawn |
| Noto Serif NP Hmong | Nyiakeng Puachue Hmong `Hmnp` | drawn |
| Noto Sans Javanese | Javanese `Java` | reported |
| Noto Sans Kayah Li | Kayah Li `Kali` | drawn |
| Noto Sans Tai Tham | Tai Tham `Lana` | reported |
| Noto Sans Lepcha | Lepcha `Lepc` | reported |
| Noto Sans Limbu | Limbu `Limb` | drawn |
| Noto Sans Lisu | Lisu `Lisu` | drawn |
| Noto Sans Mandaic | Mandaic `Mand` | reported |
| Noto Sans Meetei Mayek | Meetei Mayek `Mtei` | reported |
| Noto Sans Newa | Newa `Newa` | reported |
| Noto Sans NKo | Nko `Nkoo` | reported |
| Noto Sans Ol Chiki | Ol Chiki `Olck` | drawn |
| Noto Sans Osage | Osage `Osge` | drawn |
| Noto Sans Miao | Miao `Plrd` | drawn |
| Noto Sans Hanifi Rohingya | Hanifi Rohingya `Rohg` | reported |
| Noto Sans Saurashtra | Saurashtra `Saur` | reported |
| Noto Sans Sundanese | Sundanese `Sund` | reported |
| Noto Sans Syloti Nagri | Syloti Nagri `Sylo` | reported |
| Noto Sans Syriac | Syriac `Syrc` | reported |
| Noto Sans Tai Le | Tai Le `Tale` | drawn |
| Noto Sans New Tai Lue | New Tai Lue `Talu` | drawn |
| Noto Sans Tai Viet | Tai Viet `Tavt` | drawn |
| Noto Sans Tifinagh | Tifinagh `Tfng` | drawn |
| Noto Sans Vai | Vai `Vaii` | drawn |
| Noto Sans Wancho | Wancho `Wcho` | drawn |
| Noto Sans Yi | Yi `Yiii` | drawn |

`fallback-scripts-historic`, the Excluded Scripts that have a face:

| Face | Scripts | Reading path |
| --- | --- | --- |
| Noto Sans Caucasian Albanian | Caucasian Albanian `Aghb` | drawn |
| Noto Serif Ahom | Ahom `Ahom` | reported |
| Noto Sans Imperial Aramaic | Imperial Aramaic `Armi` | drawn |
| Noto Sans Avestan | Avestan `Avst` | drawn |
| Noto Sans Bassa Vah | Bassa Vah `Bass` | drawn |
| Noto Sans Bhaiksuki | Bhaiksuki `Bhks` | reported |
| Noto Sans Brahmi | Brahmi `Brah` | reported |
| Noto Sans Buginese | Buginese `Bugi` | reported |
| Noto Sans Buhid | Buhid `Buhd` | drawn |
| Noto Sans Carian | Carian `Cari` | drawn |
| Noto Sans Chorasmian | Chorasmian `Chrs` | reported |
| Noto Sans Coptic | Coptic `Copt` | drawn |
| Noto Sans Cypro Minoan | Cypro-Minoan `Cpmn` | drawn |
| Noto Sans Cypriot | Cypriot `Cprt` | drawn |
| Noto Serif Dives Akuru | Dives Akuru `Diak` | reported |
| Noto Serif Dogra | Dogra `Dogr` | reported |
| Noto Sans Deseret | Deseret `Dsrt` | drawn |
| Noto Sans Duployan | Duployan `Dupl` | reported |
| Noto Sans Egyptian Hieroglyphs | Egyptian Hieroglyphs `Egyp` | reported |
| Noto Sans Elbasan | Elbasan `Elba` | drawn |
| Noto Sans Elymaic | Elymaic `Elym` | drawn |
| Noto Sans Glagolitic | Glagolitic `Glag` | drawn |
| Noto Sans Gunjala Gondi | Gunjala Gondi `Gong` | reported |
| Noto Sans Masaram Gondi | Masaram Gondi `Gonm` | reported |
| Noto Sans Gothic | Gothic `Goth` | drawn |
| Noto Sans Grantha | Grantha `Gran` | reported |
| Noto Sans Hanunoo | Hanunoo `Hano` | drawn |
| Noto Sans Hatran | Hatran `Hatr` | drawn |
| Noto Sans Anatolian Hieroglyphs | Anatolian Hieroglyphs `Hluw` | drawn |
| Noto Sans Pahawh Hmong | Pahawh Hmong `Hmng` | drawn |
| Noto Sans Old Hungarian | Old Hungarian `Hung` | drawn |
| Noto Sans Old Italic | Old Italic `Ital` | drawn |
| Noto Sans Kawi | Kawi `Kawi` | reported |
| Noto Sans Kharoshthi | Kharoshthi `Khar` | reported |
| Noto Sans Khojki | Khojki `Khoj` | reported |
| Noto Serif Khitan Small Script | Khitan Small Script `Kits` | drawn |
| Noto Sans Kaithi | Kaithi `Kthi` | reported |
| Noto Sans Linear A | Linear A `Lina` | drawn |
| Noto Sans Linear B | Linear B `Linb` | drawn |
| Noto Sans Lycian | Lycian `Lyci` | drawn |
| Noto Sans Lydian | Lydian `Lydi` | drawn |
| Noto Serif Makasar | Makasar `Maka` | reported |
| Noto Sans Mahajani | Mahajani `Mahj` | drawn |
| Noto Sans Manichaean | Manichaean `Mani` | reported |
| Noto Sans Marchen | Marchen `Marc` | reported |
| Noto Sans Medefaidrin | Medefaidrin `Medf` | drawn |
| Noto Sans Mende Kikakui | Mende Kikakui `Mend` | drawn |
| Noto Sans Meroitic | Meroitic Cursive `Merc`, Meroitic Hieroglyphs `Mero` | drawn |
| Noto Sans Modi | Modi `Modi` | reported |
| Noto Sans Mongolian | Mongolian `Mong` | reported |
| Noto Sans Mro | Mro `Mroo` | drawn |
| Noto Sans Multani | Multani `Mult` | drawn |
| Noto Sans Nag Mundari | Nag Mundari `Nagm` | drawn |
| Noto Sans Old North Arabian | Old North Arabian `Narb` | drawn |
| Noto Sans Nandinagari | Nandinagari `Nand` | reported |
| Noto Sans Nabataean | Nabataean `Nbat` | drawn |
| Noto Sans Nushu | Nushu `Nshu` | drawn |
| Noto Sans Ogham | Ogham `Ogam` | drawn |
| Noto Sans Old Turkic | Old Turkic `Orkh` | drawn |
| Noto Sans Osmanya | Osmanya `Osma` | drawn |
| Noto Serif Old Uyghur | Old Uyghur `Ougr` | reported |
| Noto Sans Palmyrene | Palmyrene `Palm` | drawn |
| Noto Sans Pau Cin Hau | Pau Cin Hau `Pauc` | drawn |
| Noto Sans Old Permic | Old Permic `Perm` | drawn |
| Noto Sans Phags Pa | Phags-pa `Phag` | reported |
| Noto Sans Inscriptional Pahlavi | Inscriptional Pahlavi `Phli` | drawn |
| Noto Sans Psalter Pahlavi | Psalter Pahlavi `Phlp` | reported |
| Noto Sans Phoenician | Phoenician `Phnx` | drawn |
| Noto Sans Inscriptional Parthian | Inscriptional Parthian `Prti` | drawn |
| Noto Sans Rejang | Rejang `Rjng` | drawn |
| Noto Sans Runic | Runic `Runr` | drawn |
| Noto Sans Samaritan | Samaritan `Samr` | drawn |
| Noto Sans Old South Arabian | Old South Arabian `Sarb` | drawn |
| Noto Sans Sign Writing | SignWriting `Sgnw` | reported |
| Noto Sans Shavian | Shavian `Shaw` | drawn |
| Noto Sans Sharada | Sharada `Shrd` | reported |
| Noto Sans Siddham | Siddham `Sidd` | reported |
| Noto Sans Khudawadi | Khudawadi `Sind` | reported |
| Noto Sans Sora Sompeng | Sora Sompeng `Sora` | drawn |
| Noto Sans Sogdian | Sogdian `Sogd` | reported |
| Noto Sans Old Sogdian | Old Sogdian `Sogo` | drawn |
| Noto Sans Soyombo | Soyombo `Soyo` | reported |
| Noto Sans Sunuwar | Sunuwar `Sunu` | drawn |
| Noto Sans Tagbanwa | Tagbanwa `Tagb` | drawn |
| Noto Sans Takri | Takri `Takr` | reported |
| Noto Serif Tangut | Tangut `Tang` | drawn |
| Noto Sans Tagalog | Tagalog `Tglg` | drawn |
| Noto Sans Tirhuta | Tirhuta `Tirh` | reported |
| Noto Sans Tangsa | Tangsa `Tnsa` | drawn |
| Noto Serif Todhri | Todhri `Todr` | drawn |
| Noto Serif Toto | Toto `Toto` | drawn |
| Noto Sans Ugaritic | Ugaritic `Ugar` | drawn |
| Noto Sans Vithkuqi | Vithkuqi `Vith` | drawn |
| Noto Sans Warang Citi | Warang Citi `Wara` | drawn |
| Noto Sans Old Persian | Old Persian `Xpeo` | drawn |
| Noto Sans Cuneiform | Cuneiform `Xsux` | drawn |
| Noto Serif Yezidi | Yezidi `Yezi` | drawn |
| Noto Sans Zanabazar Square | Zanabazar Square `Zanb` | reported |

No admitted face covers nine scripts of the Excluded Scripts, each new in
Unicode 16.0 or 17.0: Beria Erfe `Berf`, Garay `Gara`, Gurung Khema `Gukh`,
Kirat Rai `Krai`, Ol Onal `Onao`, Sidetic `Sidt`, Tai Yo `Tayo`, Tolong Siki
`Tols` and Tulu-Tigalari `Tutg`. Their characters stay counted, as the
non-goals say. Braille is in none of UAX #31's tables; Noto Sans Symbols 2
draws it, in the symbols group.

The class groups, each a feature of its own:

| Feature | Candidates | What it serves | Upstream |
| --- | --- | --- | --- |
| `fallback-emoji` | Noto Emoji, Regular, its variable face's default instance | emoji presentation, and a pictograph asked in text presentation after the symbol faces | [google/fonts](https://github.com/google/fonts) at `29b4ec6`, `ofl/notoemoji/NotoEmoji[wght].ttf` |
| `fallback-math` | Noto Sans Math; STIX Two Math | `Math` | notofonts.github.io as above, `fonts/NotoSansMath/`; google/fonts as above, `ofl/stixtwomath/STIXTwoMath-Regular.ttf` |
| `fallback-music` | Noto Music; Noto Znamenny Musical Notation | the four musical blocks, in the symbol order | notofonts.github.io as above, `fonts/NotoMusic/`, `fonts/NotoZnamennyMusicalNotation/` |
| `fallback-symbols` | Noto Sans Symbols; Noto Sans Symbols 2; Noto Sans Mayan Numerals; Noto Sans Indic Siyaq Numbers; Noto Serif Ottoman Siyaq | `So`, `Sm` and `Common`'s `No`, in the symbol order; Braille | notofonts.github.io as above, each family's `fonts/<Family>/` |

All of them are `OFL-1.1`. Of the two mathematics candidates the group
carries one: the one whose `cmap` covers the most `Math` characters by
FT-02.2's census, and of two that cover as many the smaller stored. Every
other group carries all its candidates, which serve different characters.
`fallback-fonts` turns all seven groups on; `bundled-fonts` turns none of
them on, so the bundled profile grows by FT-01's faces alone.

**The storage form, and where the faces live.** As FT-01's: WOFF2, the
reference encoder's, named with its version in each face's provenance, made
ready once a process behind a `OnceLock` on its first request. The test that
holds each file to its published face (`crates/tinker-pdf/tests/cjk_faces.rs`
is FT-01's) holds every table but `glyf` and `loca` byte for byte, and those
two by the outlines they give each glyph: WOFF2 §5.1 transforms `glyf`, and
its reconstruction need not be the published bytes. No Noto face names a
reserved font name, so the transform changes no name. Each group is a data
crate of its own, `tinker-pdf-font-fallback-<group>`, split into numbered
crates where its faces pass `CRATES_IO_CEILING`, each holding whole faces and
their licences, an edge for `cargo xtask dag` and a manifest for `cargo xtask
versions`, depended on by `tinker-pdf-font` through the group's feature.

**The features and the profiles.** Each group's feature is declared in
`tinker-pdf-font`, forwarded by the facade and by `bindings/js`, and is a wasm
profile `cargo xtask footprint` holds at its own figure and tolerance (ruling
19), the default profile carrying none of them.

**The two paths.** The producing path asks for a character no book face and
no Liberation stand-in covers, with its run's script and its element's
language; it shapes the run in the face the lookup gives, embeds that face as
a composite font as it embeds the stand-in, and measures the run in it,
because layout's `metrics::FontRequest` carries the language. The reading
path asks for a character of an unembedded font whose font-level substitute
lacks its glyph, with its run's script and the font's collection; it draws
the glyph from the face the lookup gives where the script is drawn, and
reports it by name where the script is reported. An embedded font's missing
glyph is the font's own: neither path asks the lookup for it.

**The census** (FT-02.2) prices the faces and chooses none of them but the
mathematics face, by the rule above. `crates/tinker-pdf/tests/fallback_coverage.rs`,
`#[ignore]`d, `fallback_face_coverage_census`, reads each candidate of the
tables above from the directory `TINKER_FALLBACK_FACES` names — and FT-01's
four region faces, which step 2 orders by it — and each one's WOFF2 form from
the same directory under its stem with `.woff2`. It prints `RAN` and two
tables, each row led by its table's name, or `SKIPPED`: `coverage`, a row for
each face and each `Script` value and class its `cmap` covers any character
of, with the count it covers and the count the value or class holds; and
`storage`, a row for each face, its bytes as published and as WOFF2, and a
row for each group, its faces' WOFF2 bytes together, the bytes it adds to a
native build. Its rows are `crates/tinker-pdf/tests/fallback_coverage.tsv`
(ruling 16), held whole by `the_committed_census_is_whole`, and each group's
gzipped wasm, built as `release.yml` builds the default profile with a
scratch feature that carries the group's WOFF2 files, is `fallback_sizes.tsv`
beside it. Each run's revision, machine and day go in its commit's message.

**The page-per-script proof, and what "no `.notdef`" is measured by.** For
every script the tables give a face, and for each class group, FT-02.5
generates a Markdown page and opens it with `Document::open_markdown`, the
features on: the first 64 of the script's letters — `General_Category` `L`,
in code point order — that its face covers, or the first 64 of the class's
characters its group's faces cover. "No `.notdef`" is three facts of that document: no font it embeds is
shown glyph 0 anywhere on the page, read from the page's own text-showing
operators through the font's encoding; the producing path's `Fonts::uncovered`
counts no character; and the page renders reporting nothing. The reading
path's twin is FT-02.6's page of an unembedded font whose `/Differences`
name characters of drawn scripts and of reported ones.

### Milestones

1. **FT-02.1: this section.**
2. **FT-02.2: the price.** Exit: `fallback_coverage.rs`
   `fallback_face_coverage_census` (ignored) reads the candidates from
   `TINKER_FALLBACK_FACES` and prints `RAN` with a row per face and script,
   or `SKIPPED`; its rows, with each group's bytes and wasm figures, are
   `fallback_coverage.tsv` and `fallback_sizes.tsv`, held by
   `the_committed_census_is_whole`.
3. **FT-02.3: the lookup.** Exit: `crates/tinker-pdf/src/fallback.rs`'s
   `every_script_resolves_to_its_groups_face`,
   `common_and_inherited_take_their_runs_script`,
   `emoji_mathematics_music_and_symbols_resolve_by_class`,
   `han_resolves_by_language_and_collection` and
   `a_character_no_face_covers_resolves_to_none`, over a face table given in
   the test; `tinker-pdf-layout`'s property tests hold each table it compiles
   to the vendored file it reads.
4. **FT-02.4: the faces vendored.** Exit: `bundled.rs`
   `every_fallback_face_is_compiled_in_once_and_reads` and
   `each_fallback_face_is_made_ready_once`; `cargo xtask vendor`, `cargo deny
   check licenses bans sources` and `release.rs`'s package tests.
5. **FT-02.5: the producing path asks.** Exit: `fallback_faces.rs`
   `a_page_per_script_draws_no_notdef_and_reports_nothing` and
   `han_is_drawn_in_the_face_its_language_selects`, with the features on.
6. **FT-02.6: the reading path asks.** Exit: `substitute_fonts.rs`
   `a_glyph_the_substitute_lacks_is_drawn_from_its_scripts_face`,
   `a_script_the_reading_path_refuses_is_reported_by_name`, and an embedded
   font's missing glyph still drawn `.notdef` and reported.
7. **FT-02.7: the fingerprints.** Exit: `determinism.rs`'s
   `rendering_is_stable_across_targets` holds a page set in several fallback
   faces through the producing path and one drawn through the reading path,
   with the features, on Linux, Windows and `wasm32-wasip1`.
8. **FT-02.8: the profiles.** Exit: a local MS-06 footprint check with a row
   per feature at its committed figure, the default profile's unchanged.
9. **FT-02.9: the speed row.** Exit: `xtask/src/bench.rs`
   `the_committed_baseline_agrees_with_the_tree` with the layout group's
   operation over every covered script's sample, and its time reported.

### Risks

| Risk | Mitigation |
| --- | --- |
| A script the reading path draws unshaped draws wrongly after all | The rule is read from the vendored properties and errs toward reporting; a script found wrong moves to the reported side by name, with a test |
| A `Common` character takes the wrong script's face | It takes its run's script, the face drawing its neighbours, and the Latin face only where no face before covers it |
| A group passes crates.io's ceiling | Numbered crates of whole faces, never a split file |
| A face of the census is not the face vendored | FT-02.4 holds each vendored file to its published face, table by table, and to the SHA-256 `THIRDPARTY.md` records |
| The historic group's faces make its profile large | It is a profile of its own, off by default, held at its own figure (ruling 19) |
| Han drawn in another region's forms where a document names no language | The collection chooses where the font names one; otherwise the faces are asked by the census's coverage, a stated order rather than a guess |
