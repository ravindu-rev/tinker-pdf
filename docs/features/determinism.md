# Determinism

Rendering the same bytes with the same options produces bit-identical
output on every supported target. Not "visually identical" — bit-identical.
This is ruling 4, and it is what makes single-golden CI possible: a
fingerprint is stored once and every target must reproduce it exactly, so a
perceptual diff never has to absorb platform noise, and a real regression
cannot hide inside a tolerance.

## What it does

Three sources of cross-target variance are removed structurally rather than
chased:

- **Fixed-point coverage accumulation** in the rasterizer
  ([rasterizer](rasterizer.md)): edge walking and coverage sums use integer
  fixed-point, so there is no float summation-order or FMA-contraction
  variance to leak into pixel values.
- **No platform libm on any pixel path.** `tinker-pdf-math` supplies `sin`,
  `cos`, `tan`, `atan`, `atan2`, `ln`, `exp`, `pow`, `cbrt`, `log2` and
  `log10`, built from nothing but operations IEEE 754 pins exactly, and it
  is `no_std` so `x.sin()` does not compile inside it. `sin` and `cos`
  reduce every finite argument exactly — by Cody and Waite's split π/2 while
  the quadrant count is below 2^22, and past it by Payne and Hanek's integer
  product with 1 216 bits of 2/π — so a turn of any size is a point on the
  circle and never a NaN. The boundary is
  precise: `sqrt`, `floor`, `ceil`, `round`, `trunc` and `abs` **are**
  correctly rounded by the standard and may be used freely; only the
  transcendental family diverges between libms, and last-ulp differences
  become visible pixels after quantization. `cargo xtask libm`, part of
  `cargo xtask check`, fails if a pixel-path crate calls a platform
  transcendental.
- **No environment-dependent behavior**: no locale-sensitive parsing, no
  hash-map iteration order on any output path, no time or randomness in the
  render path. Where a step count or level count is computed from geometry,
  it branches on 16.16 fixed-point integers, because a count one different
  is a different image.

The contract has a second half for the container formats: a book's page
count is a function of the box the caller passes
([epub](epub.md)), so the same book at two page boxes must each be stable
on every target **and the two must differ** — a build that ignored the
argument would satisfy "stable" twice over and be exactly as wrong as one
that paginated at random.

## API

Nothing to call — determinism is a property of `Page::render` and
`DocumentEditor::save`, not an option. The contract is stated here and
enforced by the suite below.

## Verified

`crates/tinker-pdf/tests/determinism.rs` commits a **render fingerprint** for
each page of its table — `text`, `curves`, `shading`, `pattern`, `optional`,
`image`, `jbig2`, `jpx`, `blend`, `transparency`, `tiling`, `mesh`, `cbz`,
`xps`, `epub`, `analytic_axial`, `analytic_radial`, `analytic_blend`,
`analytic_image`, `default_spaces`, `gs_text`, `strokes`, `bilevel`, `icc`,
`jpeg_minified`, `jpx_minified`, `intents`, `substituted_cjk`,
`substituted_symbols`, `multiple_master`, `colour_glyphs`, and in a build
with the `hyphenation` feature `hyphenation` — each a hash of
rendered pixels
plus the page dimensions and
ink counts it is computed from. `strokes` is the stroker's whole pen: the
three caps and the three joins
(8.4.3.3, 8.4.3.4), a corner a miter limit bevels beside one it lets through
(8.4.3.5), two dash patterns at two phases along two curves (8.4.3.6) and a
star filled under the even-odd rule; `crates/tinker-pdf/tests/render_regions.rs`
tiles the same page, built by the same function. `bilevel` is one raster of
one-bit samples drawn five ways — DeviceGray as the samples stand and as a
CCITT G4 fax, under `/Decode [1 0]`, through a two-colour `/Indexed` palette
and as an `/ImageMask` in a fill colour — each magnified, at 1:1 and minified
2:1 and 3:1 at fractional offsets, and `render_regions.rs` tiles the same
page, built by the same function in `tests/render_support/`. `jpeg_minified`
is a baseline and a progressive 4:2:0 JPEG, both odd-sized, each drawn at
exactly a half, a quarter and an eighth of its size — flipped, quarter-turned
and clipped among them — so each is decoded at the scale it is drawn at
([rendering](rendering.md), "Images drawn smaller than they are"): the
reduced inverse transforms, the reduced grid's placement and its cut at the
image's edge, on every target; `render_regions.rs` tiles it too.
`jpx_minified` is six JPEG 2000 codestreams of both wavelets — two odd-sized,
one of components sampled every second column and row — each drawn at the
sizes that drop one resolution level and two, flipped, quarter-turned,
sheared and clipped among them, and once at a size that would drop a level
its codestream does not have: the low bands, the reduced grid's placement
and its cut at the image's edge, on every target; `render_regions.rs` tiles
it too. `gs_text` is the
`text` page with its font selected by graphics states (8.4.5's `/Font`) and
its `/Font` taken away, so it holds the `text` page's own fingerprint: a font a
state selects draws as its `Tf` twin on every target. `default_spaces` is the
one page that names a default colour space (8.6.5.6), each of the three
through its own arithmetic: a CalGray's `pow`, a CalRGB's matrix and an ICC
profile's lookup table. `icc` converts through every model of profile the
engine reads, each built in the test: an 8-bit CMYK image through a
four-channel `mft2` table into Lab, the same samples as `/DeviceCMYK` beside
it, an RGB image and a fill through one matrix/TRC profile, and a grey-profile
image, every sample drawn ten pixels square, so each pixel is one
conversion's colour. `intents` paints, in each of four columns under one of
Table 69's rendering intents by `ri` (8.6.5.8), a fill, a 2 × 2 image and an
axial shading in an `ICCBased` space over a profile carrying a table for each
intent and a media white other than D50, so each intent's table, and
absolute colorimetric's scaling by the media white, are one picture on every
target; `crates/tinker-pdf/tests/rendering_intents.rs`
holds the same page, built by the same function in `tests/render_support/`,
to the transforms recomputed from ICC.1. `substituted_cjk` is unembedded
Adobe-Japan1 text drawn by a face a provider substitutes, a face built in
`tests/render_support/` whose glyph at each CID's number is a decoy: `日本語A`
under `UniJIS-UTF16-H` and `、` under `UniJIS-UTF16-V`, each reached through
the collection's inverted `Uni*-UTF32` CMaps and the face's `cmap`, and the
comma through the face's `vert` ([fonts](fonts.md)), so the inversion and the
vertical form are one picture on every target; `substitute_fonts.rs` reads
the same page.
`substituted_symbols` is every code of Symbol's and ZapfDingbats' built-in
encodings in unembedded fonts, each alone in its cell, drawn by a face built
in `tests/render_support/` whose `post` table names each of its glyphs, a
box of its own size: each code reaches its glyph through its built-in
encoding's name ([fonts](fonts.md)), so the route by name is one picture on
every target; `substitute_fonts.rs` reads the same page box by box.
`multiple_master` is an `/MMType1` font embedding a two-axis, four-master
Type 1 program, built in `tests/type1_support/`, drawn at the instance its
`/BaseFont` names: the design vector read from the name, normalised through
the program's maps, the masters weighted by their corners and blended by the
blend othersubrs ([fonts](fonts.md)), so the instance is one picture on every
target; `multiple_master.rs` holds the same page to a plain Type 1 twin drawn
from the instance's outlines. `colour_glyphs` is a colour face's glyphs
written as Type 3 procedures ([fonts](fonts.md)), built in
`tests/colour_support/`: a `COLR` version 1 graph — a reflected linear
gradient multiplied over a padded radial one with a translucent stop, each a
calculator of the colour line in linear light, a soft mask and a blend mode
— an `sbix` PNG with an alpha placed by its strike's metrics, and an `SVG `
element of a path, a gradient and a `<use>` through the book's scene writer,
so the shadings' programs, the transparency they need and the scene's
operators are one picture on every target; `colour_fonts.rs` holds each kind
to its formulas.
`hyphenation` is the first page of a book generated in the test, one
chapter in British English whose paragraph of long words is set in
`hyphens: auto`, opened at 288 × 360 points: its lines break at the vendored
`en-GB` dictionary's points ([design/hyphenation.md](../design/hyphenation.md))
and draw a hyphen there, so the trie's walk, the points joined to the line
breaker's opportunities and the hyphen at a line's end are one picture on
every target. Its text holds no hyphen of its own, and the fixture asserts
that at least one of the page's lines ends in one, at an automatic point,
before it is hashed. It needs the dictionaries, so the page, and its entry
in each of the file's tables, exist only in a build with the feature, and
the file is run once without it and once with it on every target.

The `analytic_` pages are a different kind of fixture from the rest: their
pixels have a **closed-form right answer**, which
`crates/tinker-pdf/tests/render_analytic.rs` evaluates per pixel over the
whole page ([design/render-verification.md](../design/render-verification.md)).
Both files build the page through the same function in `tests/render_support/`,
so the document the equation adjudicates and the document the hash covers
cannot drift apart — and what the hash adds is the one thing the equation
cannot say, that the same arithmetic comes out the same on every target.
Every fixture asserts a minimum ink count and the absence of `UnreadableFont`
before it is hashed, so a fixture that draws nothing fails rather than
becoming a baseline.

**The options that change what a page's bytes are** — ink instead of light,
hard edges, coverage levels, the hard-edge rules and the thinnest line, a page
with nothing under it, premultiplied alpha, forced colours — are pinned beside
these rather than among them, in
`crates/tinker-pdf/tests/render_options.rs`: this file's `Fixture` renders
with the default options and nothing else, and its table is the one place a
determinism bug shows, so a row meaning "this page, but differently" does not
join it. Each is hashed the same way and floored by ink the same way, and the
same file holds that the defaults did not move: the blend grid's default render
there is this table's `analytic_blend`, copied. `levels_rules_and_widths_are_pinned`
holds the hard-edge page at levels apart and together, at 0 bits under each
rule, with anti-aliasing off under the centre and any-part rules and at
explicit thinnest lines, at 1x and 1.7x, to a table of its own
(`LEVELS_RULES_AND_WIDTHS`), every row checked and every moved line printed
before it fails; a value is recorded from the first target's run and held on
every target after it. `forced_colour_output_is_pinned` holds the forced
page — text, translucent and blended paint, a dashed stroke, a turned image, a
knockout ink group and a soft mask — in forced colours with and without
fill-to-stroke, black on white at 1x and in two hues at 1.7x, to
`FORCED_COLOUR_OUTPUT` the same way, each render's black-on-white coverage
floored by ink first; the page embeds its face, so one table serves both
builds. `placed_annotation_output_is_pinned` holds a page turned a quarter
whose annotation is flagged `/NoZoom` and `/NoRotate`, at magnification 2,
where 12.5.3 places the appearance at half its size and upright about its
fixed corner, to `PLACED_ANNOTATION_OUTPUT`. The local `wasm32-wasip1` run
takes that file as well.

**The bitmap toolbox's output** is pinned the same way, over a committed
render rather than a page: `crates/tinker-pdf/tests/bitmap_toolbox.rs`'s
`the_toolbox_is_stable_across_targets` lays the shading golden out in RGBA
with an alpha plane, and in ink and Lab, and hashes each operation's output
as a render is hashed — gamma at four exponents, the two inversions, the
tint, grayscale, the three reductions' answers, scales down and up, a
subsample, and conversions into and out of ink and Lab — to its own table
(`TOOLBOX_FINGERPRINTS`), each map checked to change its input. Gamma's
table, built through `tinker_pdf_math::pow`, and Lab's arithmetic, through
`tinker-pdf-color`, are where a platform transcendental would show first, and
a hash two targets disagree on is a determinism bug there as everywhere.

**A page's separations** are pinned plate by plate:
`crates/tinker-pdf/tests/separations.rs`'s
`separations_are_stable_across_targets` draws the separations fixture — four
spots in fills and a stroke, in text set in an embedded face, in a knockout
group under a blend mode and an alpha and in a group of DeviceRGB, in an image
of two spots and a stencil, in an axial ramp and in a mesh, the page the render
group times (`benches/support/plates.rs`) — through `Page::separations` at
72 dpi, and hashes each of its eight plates as a render is hashed, the
dimensions with it, to its own table (`SEPARATION_FINGERPRINTS`), each plate
floored by ink first and the render held to draw with no warning. The face is
embedded and every byte of the page is written in the test's support, so one
table serves both builds and nothing is read at run time.

**A page drawn in bands is the page.**
`banded_rendering_reproduces_every_fingerprint` renders every page of the
table as four horizontal bands through `RenderOptions::region`, stacks them
in order and holds the result to the page's own committed fingerprint, so it
adds no hash. `render_regions.rs` holds ruling 5's tiles to the whole page
they come from; this holds the bands to the committed value itself, on every
target the roster runs on, `wasm32-wasip1` among them.

**A page drawn on threads is the page.**
`parallel_and_banded_renders_are_the_fingerprints` draws every page of the
table on 2, 3, 4 and 7 threads of the engine's pool — named that many times
in one `Document::render_pages` walk, so that many workers draw it at once,
and drawn in that many bands through `DisplayList::render` and
`Page::render` ([rendering](rendering.md), "A page in bands") — and holds
each to the page's committed fingerprint, so it adds no hash either. On
`wasm32-wasip1` no thread starts, and the walk and the bands are drawn and
stitched on the caller, which must come to the same bytes.

**What a document keeps across its renders is what it computes.**
`warm_and_starved_caches_render_the_fingerprints` renders every page of the
table twice through a document whose cache budget keeps everything its
renders compute ([rendering](rendering.md)'s "The document's cache"), so the
second render is served by the cache, and twice through one whose budget of
zero keeps nothing, so every request computes again; each render is held to
the page's committed fingerprint and to the warnings a render with no budget
reports, so it adds no hash. Every kind the cache keeps is under it, since a
page renders through every kind its document keeps, and so that it cannot
stop exercising the cache unnoticed the warm second renders must find
something kept between them and the starved ones nothing.
`budgets_stated_at_open_render_the_fingerprints` does the same with each
budget stated at open, which charges the document's parsed objects beside its
cache: under a budget of zero no parsed object is kept and every read parses
again, and each render is held to the page's fingerprint and warnings all the
same.

**A patched page is the page.**
`a_patched_render_reproduces_the_edited_pages_fingerprint` appends an
annotation edit fixed in the test — a square's appearance redrawn, a second
annotation moved onto it, a third added — to a page of shapes and an image
under two annotations, patches the page over its render before the edit by the
rectangle the edit changed (`Appended::patch`, [rendering](rendering.md)), and
renders it whole: the patch takes the page rather than rendering it whole, and
both bitmaps hash, as the table's pages hash, to one committed value of their
own, `PATCHED_PAGE`, on every target the roster runs on. It is the edited
page's alone, so it joins no table and moves no other fingerprint.

**The SVG writer's output** is pinned beside the render fingerprints:
`svg_output_is_stable_across_targets` writes every page of the table with the
default `SvgOptions` and holds the SHA-256 of its markup to a committed value.
The markup carries arithmetic a render does not show — each coordinate
rounded to four places and printed at its shortest, each pen measured
through its transform — and what the writer rasterises reaches it through the
renderer and the PNG encoder. Text is written as the outlines of the
fixtures' own faces, so one table serves both builds. Each page is held to
write at least one drawing element, and to report neither an unreadable font
nor a truncation, before it is hashed.

**The page analysis's box** is pinned beside them:
`the_mark_boxes_are_the_same_on_every_target` analyses every page of the
table with the default options and holds the box of its marks, each number
at its shortest and the box in pixels, to a committed value. The box is
arithmetic a render does not show — a curve's extremes through a square root,
a stroke's outline, the page's transform rounded outward — so a target that
computes it differently fails here as one that draws differently fails above.

Beside them, **document byte-hashes** pin the writer as well as the renderer:
a synthesised PDF, a synthesised fixed-layout document and a synthesised book
are each hashed as *bytes*, which is where object numbering, dictionary key
order and stream framing are pinned — none of which a rendered hash can see.
`a_book_is_stable_at_each_page_box_and_the_two_boxes_differ` asserts the
two-box claim above: the book is stable at 432 x 648 and at 600 x 800, and
the two differ in bytes and in page count. A fourth pins the save door's
embedding pass (`docs/design/font-embedding.md`):
`the_rewritten_document_with_embedded_faces_is_the_same_bytes_on_every_target`
saves a standard-14 page and an unembedded TrueType page with
`SaveOptions::embed` asking a synthetic face, built byte by byte in
`tests/embed_support`, and holds the two documents' hashes, joined and hashed,
to a committed value: the cut program, the descriptor computed from the
face's tables, the widths written and the subset tag are bytes a render does
not see all of.

**The engine's own sRGB profile** is pinned as bytes too:
`the_first_party_srgb_profile_is_one_byte_string_on_every_target` holds the
SHA-256 of `tinker_pdf::SRGB_PROFILE` to a committed value. The profile is
computed by `crates/tinker-pdf-color/build.rs` when the engine is built
([rendering](rendering.md)), and a build script runs on the build host
whatever the target, so the pin holds the host's arithmetic to one answer as
the render fingerprints hold the target's: four IEEE operations and
`tinker_pdf_math::pow`, the same bytes wherever they are computed. A hash that
differs between build hosts is a determinism bug in `build.rs`, never a table
edit, and a one-byte change to the profile's description fails it.

**The ICC transforms are pinned by what they answer.**
`icc_transforms_are_stable_across_targets` builds profiles of every kind
`tinker-pdf-color`'s ICC reader converts through
([rendering](rendering.md)), byte by byte in
`crates/tinker-pdf/tests/icc_support/transforms.rs` from sums and products
alone: the models inverted out of the connection space — a matrix/TRC
profile's parametric, gamma and sampled curves, and grey under XYZ and Lab —
`mft1`, `mft2` and `mBA ` tables out of it, device links through a `D2B0` and
an `A2B0`, a gamut tag, named colours over Lab and XYZ, multi-process
elements both ways beside a model and in a version 5 profile, a version 5
calculator and a spectral connection space. Each set is hashed over
everything it answers under every intent at a fixed grid of inputs — each
transform into sRGB at sixteen bits over device values stepped in fortieths,
tenths or sixths, each transform out of the connection space's device values
over Lab and XYZ stepped in eighths, each gamut verdict, each named colour's
name, coordinates and sRGB, and each link's device values — to its own
committed value (`ICC_TRANSFORMS`); the hashing is the test's, since the
colour crate has no edge to a hash. The second half raises the first entry of
one `B2A0` grid by one level, and holds the device value at that node to have
moved by one and the set's hash to be another.
`a_v5_profiled_page_renders_the_same_on_every_target` draws three pictures
through version 5 profiles — multi-process elements, a calculator, spectra
reduced against the profile's own observer — each first held to differ from
the same samples drawn as `/DeviceRGB`, which is how a picture whose profile
the parser refused would draw, and holds the page's render to `V5_PAGE`. A
hash two targets disagree on is a determinism bug, never a table edit.

**The font measurement API** is pinned by its answers:
`font_measurements_are_stable_across_targets` hashes every answer
`FontMetrics` gives over `LiberationSerif-Italic.ttf`, included by bytes —
each glyph's advance, box and name both ways, in font units and at
fractional sizes, the font's box, units and flags, every code point and its
glyph, and the lengths of four texts at five fractional sizes, one of them
past the face's characters — each number as its IEEE bits, so a last-place
difference between targets is another hash ([fonts](fonts.md)). A clone,
and a second reading of the face whose last block of glyphs is read first,
answer the same bytes.

Three fixtures carry guarantees of different kinds: one is synthesised
rather than parsed, so it pins the writer and parser to each other; one is
a package a third-party serialiser wrote, so it can disagree with this
engine about the *format* rather than only about arithmetic; and one is a
reflowable book, whose pagination is a function of the open options.

### Targets: measured versus claimed

| Target | Measured by |
| --- | --- |
| `x86_64-unknown-linux-gnu` | CI's tests, `ubuntu-latest` |
| `x86_64-pc-windows-msvc` | CI's tests, `windows-latest` |
| `aarch64-apple-darwin` | nothing: claimed, not measured |
| `wasm32-wasip1` | the local run, under wasmtime |

CI's tests measure the two native targets on every pull request. The
`wasm32-wasip1` run is local proof, made by whoever changes what a fingerprint
covers: each fingerprint test built with `--target wasm32-wasip1` and run with
wasmtime as the runner, and `determinism.rs` once more with `--features
hyphenation`, as the gates' `determinism` gate runs it natively. CI's tests
build no `hyphenation`, so the `hyphenation` page is held on every target by
those local runs. `aarch64-apple-darwin` is run nowhere, so on it the
contract is claimed. Between them the three measured targets vary everything
below the arithmetic — a different `std`, allocator and linker — and
`wasm32-wasip1` varies width as well: 64-bit against 32-bit, where a `usize`
assumption would show.

A green tick is not evidence that a test ran, and an `#[ignore]` added one
afternoon would look identical, so a fingerprint test is held to running by
the roster below rather than by a reading of any log: `cargo xtask
determinism` fails on an entry that is not an un-ignored `#[test]` in its
file. That is
[verification.md](../verification.md)'s "a check that can be absent is red,
not green".

**The determinism roster** (`xtask/src/determinism.rs`) names them:
`determinism.rs`'s render fingerprints and the same pages drawn in bands, the
patched page's, its SVG output's hashes, its three synthesised documents' byte
hashes and its rewrite with embedded faces, the bytes the default save
writes for a page over a Type 1 font it cuts down, the first-party sRGB
profile's bytes, the ICC transforms' answers and the version 5 page, its
display list's replay of every fingerprinted page and the same pages drawn
through a warm and a starved document cache, and under a warm and a starved
budget stated at open,
and the `icc` and `tiling` pages twice through one document, the second
render of each compiling no transform and rasterising no cell;
`pdfa_writer.rs`'s archival hash; `epub_shaped.rs`'s shaped page;
`render_options.rs`'s pinned ink, transparent and premultiplied, hard-edged,
forced-colour and placed-annotation output and levels, rules and widths;
`bitmap_toolbox.rs`'s
toolbox outputs; `separations.rs`'s plates of the separations fixture; the
shaper's three fingerprints — its shaping, its bidi
levels and its columns of mixed orientation, the last shaped through faces
`crates/tinker-pdf-shape/tests/column_face` builds byte by byte; and the
guard suite's
roster of guards and the work counts it pins (`guards.rs`), which CI's tests
run in each build they test and the local `wasm32-wasip1` run under wasmtime,
beside `guard_allocations.rs`; the colours an image's converter asks of its
space's conversion and what it allocates for them (`image_conversion.rs`);
the transforms a document builds over the profiles it embeds
(`icc_cache.rs`) and the cells its fills rasterise (`pattern_cache.rs`);
and the requests page one costs over a streamed open, linearized and not
(`streaming_open.rs`). `cargo xtask
determinism`, part of `cargo xtask check` — which the gates run, and
`release.yml`'s preflight before anything is built — holds each to be a
`#[test]` in its file with no `#[ignore]`. A row that lands a fingerprint adds
its roster entry in the same commit.
