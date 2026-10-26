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
  is `no_std` so `x.sin()` does not compile inside it. The boundary is
  precise: `sqrt`, `floor`, `ceil`, `round`, `trunc` and `abs` **are**
  correctly rounded by the standard and may be used freely; only the
  transcendental family diverges between libms, and last-ulp differences
  become visible pixels after quantization. `cargo xtask libm` fails the
  build if a pixel-path crate calls a platform transcendental.
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
`analytic_image`, `default_spaces` — each a hash of rendered pixels plus the
page dimensions and ink counts it is computed from. `default_spaces` is the
one page that names a default colour space (8.6.5.6), each of the three
through its own arithmetic: a CalGray's `pow`, a CalRGB's matrix and an ICC
profile's lookup table.

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
there is this table's `analytic_blend`, copied. The `wasm32-wasip1` leg does
not run that file, so on that target those hashes are claimed, not measured.

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
| `x86_64-unknown-linux-gnu` | `test (ubuntu-latest)` |
| `x86_64-pc-windows-msvc` | `test (windows-latest)` |
| `aarch64-apple-darwin` | `test (macos-14)` |
| `wasm32-wasip1` | `wasm-determinism`, under wasmtime |

All four are measured on every CI run. Between them they vary everything
below the arithmetic — a different `std`, allocator and linker — and
`wasm32-wasip1` varies width as well: 64-bit against 32-bit, where a `usize`
assumption would show.

Each leg runs `determinism.rs` on its own and greps its log for
`rendering_is_stable_across_targets ... ok`: `cargo test --workspace` runs
those tests too, but a green tick from it is not evidence that it did, and
an `#[ignore]` added one afternoon would look identical. That is
[verification.md](../verification.md)'s "a check that can be absent is red,
not green".
