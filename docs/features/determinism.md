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
`jpeg_minified` — each a hash of rendered pixels plus the page dimensions and
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
image's edge, on every target; `render_regions.rs` tiles it too. `gs_text` is the
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
conversion's colour.

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
hard edges, a page with nothing under it, premultiplied alpha — are pinned
beside these rather than among them, in
`crates/tinker-pdf/tests/render_options.rs`: this file's `Fixture` renders
with the default options and nothing else, and its table is the one place a
determinism bug shows, so a row meaning "this page, but differently" does not
join it. Each is hashed the same way and floored by ink the same way, and the
same file holds that the defaults did not move: the blend grid's default render
there is this table's `analytic_blend`, copied. The local `wasm32-wasip1` run
takes that file as well.

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

Beside them, **document byte-hashes** pin the writer as well as the renderer:
a synthesised PDF, a synthesised fixed-layout document and a synthesised book
are each hashed as *bytes*, which is where object numbering, dictionary key
order and stream framing are pinned — none of which a rendered hash can see.
`a_book_is_stable_at_each_page_box_and_the_two_boxes_differ` asserts the
two-box claim above: the book is stable at 432 x 648 and at 600 x 800, and
the two differ in bytes and in page count.

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
wasmtime as the runner. `aarch64-apple-darwin` is run nowhere, so on it the
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
hashes, its display list's replay of every fingerprinted page and the same
pages drawn through a warm and a starved document cache;
`pdfa_writer.rs`'s archival hash; `epub_shaped.rs`'s shaped page;
`render_options.rs`'s pinned ink, transparent and premultiplied, and
hard-edged output; the shaper's two fingerprints; and the guard suite's
roster of guards and the work counts it pins (`guards.rs`), which CI's tests
run in each build they test and the local `wasm32-wasip1` run under wasmtime,
beside `guard_allocations.rs`; and the requests page one costs over a streamed
open, linearized and not (`streaming_open.rs`). `cargo xtask
determinism`, part of `cargo xtask check` — which the gates run, and
`release.yml`'s preflight before anything is built — holds each to be a
`#[test]` in its file with no `#[ignore]`. A row that lands a fingerprint adds
its roster entry in the same commit.
