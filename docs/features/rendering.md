# Rendering

The rasterizing device turns an interpreted page into pixels. It lives in
`crates/tinker-pdf-render` as `Renderer`, the `Device` implementation that
joins the content interpreter to the rasterizer, with colour conversion in
`crates/tinker-pdf-color` — a leaf crate that takes component values and
returns sRGB, with no PDF types on its surface (ruling 8,
[rulings](../rulings.md)). The contract throughout is ruling 2: a page never
fails because one thing on it was unsupported. It renders what it can,
substitutes a neutral placeholder for what it cannot, and reports every
tolerance as a typed `RenderWarning` on the bitmap (ruling 10).

## What it does

**Colour spaces** (`tinker-pdf-color`). DeviceGray, DeviceRGB and DeviceCMYK
convert as 8.6.4 says; `/Indexed` reads its palette over any base space
(8.6.6.3); `/Separation` and `/DeviceN` run their real tint transforms into
the alternate space (8.6.6.4, 8.6.6.5); `/Lab` converts through XYZ at the
D50 white point (8.6.5.4), kept separate because its components are not in
0..1 and clamping them there renders the whole space black. An `ICCBased` space is converted
through **its own profile** (ICC.1): the header and tag table are read, the
three `XYZ` columns and three tone curves compile once into fixed-point
tables, and each colour is a lookup plus an integer matrix multiply, so
nothing on the pixel path evaluates a transcendental and ruling 4 holds. Grey
profiles are the same with one curve. A printer profile carries a multi-dimensional lookup table
instead — `mft1` or `mft2` at an `A2B*` tag, three stages of curve with an
interpolated grid between them. **v4's `mAB ` reads too**, which is five
optional stages where v2 has three fixed ones — A curves, a grid whose axes may
each carry their own point count, M curves, a matrix and B curves, run in that
order although the header lists their offsets in the reverse. A grey profile
whose connection space is `Lab` rather than `XYZ` reads as well: its one curve
gives lightness instead of luminance, and the two differ by 8.6.5.4's cube
root.

Measured against the corpus's **3 235** real profiles, September 2026:
**3 229 compile, 99.8 %** — and 682 of the 5 525 files name an `ICCBased` space
that paints through one, which the corpus report counts as `iccbased`. **The
six that do not are profiles contradicting themselves**, and the census names
each: a monitor profile whose data space is `LAB` carrying the `rXYZ` columns a
matrix applies to linear RGB; one naming a three-channel space no registry
defines; and a CMYK printer profile with a single `kTRC` and no matrix — one
curve for four channels of ink. Those are `ColorSpace::Approximated`, read by
component count, which is the alternate-space reading 8.6.5.5 permits.

**`CalGray` and `CalRGB` convert through their own parameters** (8.6.5.1,
8.6.5.2): the components go through `/Gamma`, `/Matrix` takes them into XYZ
relative to `/WhitePoint`, and the white point is adapted to D50 before the
sRGB matrix. They were aliased to `DeviceGray` and `DeviceRGB` until September
2026, which read neither the white point nor the gamma and left *nothing*
recording that an approximation had happened — unlike an ICC profile this build
refuses, where `Approximated` says so on the type.

The adaptation is **von Kries in XYZ**, scaling each axis by the ratio of the
two whites, and not the Bradford transform baked into the profile path's
matrix. Named rather than hidden: the two differ on saturated colours far from
the neutral axis. A bare `/CalGray` or `/CalRGB` *name*, with no parameter
dictionary behind it, is still the device space — there is nothing else the
file has said. `[/Pattern base]` carries the underlying space
of an uncoloured pattern (8.7.3.2), so an `scn`'s components reach the paint.
Initial colours follow 8.6.8 — CMYK starts at full black ink, not all zeros.
Out-of-range components clamp rather than wrap.

**Functions** (7.10). All four types: sampled (`Function::Sampled`, 7.10.2),
exponential (`Function::Exponential`, 7.10.3), stitching
(`Function::Stitching`, 7.10.4) and the PostScript calculator
(`Function::PostScript`, 7.10.5, bounded on every axis: a 100-entry stack, 32
levels of `if`/`ifelse`, 65 536 tokens, no loop operator), plus
`Function::Array` for the arrays a `/Function` entry may hold — an array no
longer truncates to its first element.

**Shadings** (8.7.4.5). Type 1 (function-based), type 2 (axial) and type 3
(radial) are evaluated per pixel through `Shading::color_at`; the radial
parameter is the largest circle in the family containing the point
(8.7.4.5.4), which is what paints front-to-back correctly. Types 4–7 —
free-form and lattice-form triangle meshes, Coons and tensor patches
(8.7.4.5.5–8.7.4.5.8) — are geometry, carried as a `Mesh`: a Coons patch is
stored as the tensor patch 8.7.4.5.7's formula makes it, so there is one
surface evaluator; patches subdivide at a fixed step count decided on 16.16
integers (ruling 4, no float termination test on a pixel path); and the whole
mesh renders through a single coverage buffer from one non-zero fill, so
shared edges do not seam against the backdrop. Bounds: 262 144 vertices,
16 384 patches, 131 072 triangles total, 32 subdivisions per edge. `sh`
fills the current clip (8.7.4.2).

**Patterns** (8.7.3). Tiling patterns paint in both paint types, anchored to
the *parent content stream's default space* as 8.7.3.1 requires — not the
CTM in force at paint time, which is the whole correctness question in a
tiling pattern. The cell is rasterised once into a `/BBox`-sized offscreen
canvas, with the pattern's own `/Resources`, and blitted across the lattice;
`PaintType 2` recolours the finished cell with the underlying-space colour
(8.7.3.3). The lattice is bounded before a pixel exists: 65 536 positions,
16.7 Mpx per cell buffer, 33.5 Mpx of compositing per fill — past any bound
the fill paints nothing and names the pattern. A pattern that fills with
itself terminates at a nesting depth of 4. Shading patterns paint through
the same shading machinery, and pattern *strokes* route through fill-of-
outline, so both slots share one warning path and cannot diverge.

**Images.** `/SMask` (Table 89, 11.6.5.3), `/Decode` (8.9.5.2), stencil
masks that take the current fill colour (8.9.6.2) and inline images (8.9.7)
all draw. `/Interpolate` (Table 89) selects bilinear sampling on
magnification; minification averages every sample a pixel covers, through a
box pyramid that bounds the footprint ([rasterizer](rasterizer.md)). A codec this build
does not decode draws a placeholder and is named; an image that decoded with
damage tolerated is drawn *and* reported, so a half-decoded fax stays
distinguishable from a blank one.

**Form XObjects.** A form's own `/Resources` are consulted (8.10.1), so a form
pasted in from another document resolves its names in the dictionary it brought
rather than in the page's. Both seams change scope together — the interpreter
resolves fonts, colours and ExtGState, the device resolves images, shadings and
patterns — because they are asked about the same form, by the same name, at the
same moment. A form that omits the key falls back to the invoking scope, which
is what its producer is relying on. An annotation's appearance stream gets the
same treatment by a different route: it is reached by reference rather than by
name, so `Page::render` announces it instead of the interpreter, and until it
did an appearance's images resolved against the page while its text resolved
against the appearance.

**Transparency.** `/Group /S /Transparency` on a form XObject composites as
a unit (11.6.6), with isolation (11.4.4), knockout (11.4.5) and backdrop
removal at close (11.4.7.2), read from Table 147's `/I` and `/K`. `/CS` is read and
**honoured** — on a form's group and on the **page's own** group (11.4.7),
which reaches no `Do` and so is read off the page dictionary.

A group composites in the space it declared: its buffer holds that space's
components, and conversion happens at the group's boundaries rather than per
element. `/DeviceCMYK` gets a `CmykA8` buffer, and because 11.3.5's separable
formulas are written for additive components, a subtractive channel enters and
leaves them complemented — only the blend function, since 11.3.6's weighting
averages colour values in the group's own space. The four non-separable modes
(11.3.5.3) reason about hue and luminosity, which ink quantities do not have,
so on a CMYK buffer their operands convert to light, blend, and convert back.
A page-level group decides the format of the page canvas itself and is
converted for the caller at the end, which is 11.4.7's own last step; a page is
never handed back in Lab, and in CMYK only when asked for twice, because a
`Bitmap` says how many components it has and nothing about what they mean. *Lab was handed back until
September 2026*: `page_format` named `CmykA8` alone, so `format: LabA8` returned
four bytes of encoded `L*a*b*` while `PixelFormat::LabA8`'s own documentation
said it was not a page format;
`a_page_asked_for_in_lab_comes_back_in_rgb` pins the correction.

**CMYK page output** is the one way past that, and it takes two fields rather
than one: `format: PixelFormat::CmykA8` *and* `RenderOptions::allow_cmyk`, so a
caller has said they know the bytes are ink. The page composites over ink on a
canvas that starts with none — which it already did for `format: CmykA8`, and
then converted — and the buffer comes back as it stands. **The ink is light
turned back into ink**, not the document's own components: colour is flattened
to sRGB where a resource is read, and a CMYK buffer takes that back through
8.6.4.4's relation inverted with maximum undercolour removal, so `1 0 0 0 k`
arrives as exactly `(255, 0, 0, 0)` and a rich black `1 1 1 1 k` arrives as pure
`K`. `a_page_asked_for_in_ink_with_the_opt_in_comes_back_in_ink` pins both, the
second as the limitation it is. `Bitmap::to_png` writes an ink page as the light
it stands for — PNG has no CMYK — and those bytes are exactly what the same
render without the switch returns, which
`an_ink_page_written_as_png_is_the_light_the_switch_would_have_returned` holds.

**`/Lab` composites in Lab too**, which was the last space that did not. Its
components are not in the unit interval — `L*` runs 0..100 and `a`/`b` roughly
−128..127 — so `LabA8` encodes them into bytes (`L/100`, `(a + 128)/255`,
`(b + 128)/255`) and 11.3.5's separable formulas apply to *that*. The encoding
is a choice the clause does not make, and it is stated on the format rather
than buried: blending in the encoded domain is what makes a `/Lab` group
composite in Lab rather than in RGB, and it is not the same as blending the
unencoded values.

Every space a group can declare now has a buffer, so
`RenderWarning::UnsupportedGroupSpace` is gone — a variant nothing can reach is
a claim rather than a check. **No corpus file declares a `/Lab` group**: over
the 5 525 files of September 2026 it was reported zero times in all three
recorded bars, so this capability is held by fixtures rather than by demand,
which the [roadmap](../ROADMAP.md) records. The count of 35 `/DeviceCMYK`
groups is an earlier measurement over a smaller corpus and is left attributed
to it rather than restated as current. ExtGState
`/SMask` works in both kinds — `/Alpha` and `/Luminosity` (11.6.5.2) — with
`/BC` read in the mask group's own `/Group /CS` and defaulting to black
(fully masked, the default that does not invert every drop shadow), and
`/TR` pre-sampled to 256 entries. An absent `/SMask` key leaves the mask in
force where `/None` removes it (11.6.5.1); `q`/`Q` save and restore the mask
beside the clip (8.5.4). All 16 standard blend modes apply (11.3.5), the
twelve separable and the four non-separable. Group buffers are a budget, not
just a depth: nesting caps at 16 and the page as a whole at 2 000 buffers,
because a soft mask that opens further groups branches rather than descends
— past the budget the group is declined and reported once.

**Text render modes** (9.3.6). All eight: fill, stroke and clip are decided
independently, so mode 1 strokes rather than fills and modes 4–7 accumulate
glyph outlines into a clip applied at `ET`. A text object that selects a
clipping mode and produces no glyphs clips everything away — spec-correct,
and reported, because it usually means the glyphs could not be resolved.

**Optional content** (8.11). The catalog's `/OCProperties` `/D`
configuration is resolved once at bind: `/BaseState` sets every group in
`/OCGs` and `/ON`/`/OFF` name the exceptions (8.11.4.3 Table 101), OCMDs
honour all four `/P` policies and `/VE` visibility expressions (8.11.2.3,
capped at depth 16 and 256 operands), and `/OC` on both XObject kinds hides
the whole XObject (8.11.4.4) — expressed as a scope around the `Do`, so a
hidden image skips its decode. Suppression happens at the *paint*
(8.11.3.2): hidden content still advances the pen, balances `q`/`Q`,
installs clips and extracts as text, so the render device and the text
device cannot disagree about what a page contains. Every fallback resolves
to visible — a malformed `/VE`, an unknown `/P`, an unlisted group, a cycle
— because content wrongly hidden is invisible to the reader while content
wrongly shown is theirs to ignore. A hidden layer is reported by its `/Name`
(8.11.2.1), falling back to the resource name.

**Page geometry.** `page_view_transform` applies `/Rotate` (7.7.3.3,
normalised to quarter turns) and the `/CropBox` origin, so the bitmap is the
page's displayed size. Pixel dimensions round *outward* — a page never loses
its last row or column; A4 at 150 dpi is 1240×1755. A page whose area would
exceed `MAX_PAGE_PIXELS` (67.1 Mpx) renders whole at a smaller scale and
says so, because a complete page at lower resolution beats a fragment.
Annotation appearance streams draw over the content when asked (12.5.5).
`Page::pixel_size` returns that size without rendering, because a caller who
tiles has to compute a lattice against the same three rules the renderer
applies — outward rounding, a non-finite or non-positive scale read as 1.0,
and the `MAX_PAGE_PIXELS` clamp — and a lattice computed against different
ones leaves a strip of the page that nothing ever asks for.

**Regions.** `RenderOptions::region` narrows the render to a `PixelRegion`,
a rectangle of **the rendered bitmap's own pixels** counting down from its
top-left — not points, and not PDF user space's upward `y`. The bitmap is
already turned and already cropped, so a region indexes the picture a reader
sees: `(0, 0, 32, 32)` of a `/Rotate 90` page is the top-left of the sideways
picture, never the corner of the upright sheet. The mechanism is ruling 5's
translated viewport and nothing else — `region_view_transform` composes a
pixel translation *after* `page_view_transform`, so the same interpreter,
the same glyphs, the same sampler and a smaller canvas draw the tile. A
region reaching past the page is **intersected**, never slid back on, because
a moved rectangle returns real pixels from coordinates the caller did not
name; one that misses entirely comes back with no pixels. Both trims are
reported as `RenderWarning::RegionClamped`. Ruling 5's byte-equality guard,
its fixtures and the one scale-dependent exception are in
[rulings](../rulings.md).

**Anti-aliasing off.** `RenderOptions::antialias` is on by default; off, every
pixel of every shape is wholly covered or not covered at all. It is one
threshold — `Mask::harden`, half a pixel's coverage — applied wherever a
coverage value is produced, and there are four such places rather than one:
`Renderer::coverage`, through which every fill, stroke, glyph, clip, text clip
and pattern shape passes; an image's unit square, hardened inside the
rasterizer through `ImageDraw::antialias`; a mesh shading's silhouette after
`draw_mesh`; and a tiling pattern's cell, drawn by a renderer of its own and
handed the same answer on `TileRequest::antialias`. `sh` and a shading pattern
paint through a clip or a shape that was hardened when it was made. Because the
threshold is on coverage `fill` already measured, a hard edge lands where the
soft one is half-way and a shape keeps its area, two shapes sharing an edge
split its pixels rather than leaving a gap, and a tile still equals the page
under it. What is not coverage stays as the document wrote it: a constant
alpha, a soft mask, an image's own alpha and the colours inside an image. A
stroke is at least a whole pixel wide with the switch off — 8.4.3.2's thinnest
line, on a device that cannot draw part of one — because a hairline eight
tenths of a pixel wide straddling two rows leaves each less than half covered
and vanishes; `a_hard_edged_hairline_leaves_no_column_it_crosses_empty` sweeps
eight slopes for it. A glyph feature narrower than half a pixel can still
vanish where it straddles a pixel edge, which is what any threshold costs.

## API

The facade is the whole public surface (ruling 11): `Page::render` takes a
`RenderOptions` — `scale` (pixels per point, or `RenderOptions::at_dpi`),
`format` (`PixelFormat`), `cancel` (an optional `CancelToken`, cloneable and
checked between operations and scanline bands), `annotations` (on by
default), `region` (an optional `PixelRegion`, `None` for the whole page) and
`allow_cmyk` (off; with `format: CmykA8`, hands the page back as ink rather than
light) and `antialias` (on; off makes every pixel of every shape whole or
empty) — and returns a `Bitmap`: `width`, `height`, `format`, `stride`,
`data`, and `warnings`, the `Vec<RenderWarning>` that carries every named
degradation. Rendering never fails; it degrades and reports.

`Bitmap::to_png` writes the page out as a PNG file (ISO/IEC 15948), eight bits
a component, through `tinker_pdf_filters::png_encode` — which is where the
zlib stream, the chunk CRC-32 and 9.2's row filters a PNG is made of already
lived. It is **total over all six `PixelFormat`s**, which matters because a
page comes back in two of them and the fields are public: `Gray8`, `Rgb8`,
`GrayA8` and `Rgba8` are colour types 0, 2, 4 and 6 byte for byte, and the two
PNG has no colour type for are converted rather than relabelled — `CmykA8`
through 8.6.4.4's device relation and `LabA8` back out of `L*a*b*`, both
keeping their alpha and both landing on type 6. Writing ink under a label
saying RGB is the failure `page_format` exists to prevent one layer up, and it
would be just as invisible here. `None` comes back only for a bitmap that is
not a picture: a zero dimension, a stride narrower than a row, or a buffer
shorter than the rows the other fields promise. `Page::render` produces a
picture for every page at every scale and for every `region` that meets the
page; the one way to get a bitmap that is not one is to ask for a region
wholly off the page, which trims to nothing and says so. `Bitmap`'s fields
are public besides, so a caller may always build one by hand. `tpdf render`
writes `.png` through it, and so does `examples/render.rs`.

`Bitmap::from_png` is the other direction, over the same
`tinker_pdf_filters::png_decode` CBZ pages go through. The decoder has already
widened sub-byte samples, applied the palette and applied `tRNS`, so what it
hands back is one of four layouts and each is one `PixelFormat` byte for byte —
grey `Gray8`, grey and alpha `GrayA8`, truecolour `Rgb8`, truecolour and alpha
`Rgba8`. Sixteen-bit samples round to the nearest eight, `round(v / 257)`, the
exact inverse of the replication that widens eight to sixteen and not the high
byte. **For the four formats `to_png` writes as they are, the round trip is
exact** — dimensions, stride and bytes — which
`render_to_png_and_back_is_byte_identical_for_every_page_format`
(`crates/tinker-pdf/tests/png_input.rs`) holds over four pages at four formats
and two scales; `CmykA8` and `LabA8` come back as the `Rgba8` light `to_png`
wrote for them, because PNG has no colour type that could carry them back.
Every decoder refusal comes through by name as `PngReadError::Refused`, and one
more is added: a raster that stops short of its declared height is
`PngReadError::Incomplete` rather than a picture with zeroes for its missing
rows, because the caller this exists for — `pdfcmp` — would score the damage as
a rendering difference. Damage that costs no pixels, an ancillary chunk with a
bad CRC, is named on `warnings` as `RenderWarning::DamagedImage` with the name
`PNG`. The decoder's `MAX_PNG_SAMPLES` is the only budget, which leaves one
asymmetry stated rather than discovered: the largest page this engine renders
writes a PNG this will not read back, because a reader's budget against a
thirteen-byte header asking for 2^63 samples is not a writer's.

```rust
use tinker_pdf::{Document, RenderOptions, RenderWarning};

let doc = Document::open(bytes)?;
let page = doc.page(0).ok_or("no first page")?; // `Document::page` is an Option
let bitmap = page.render(&RenderOptions::at_dpi(150.0));
for w in &bitmap.warnings {
    if let RenderWarning::HiddenOptionalContent { layer } = w {
        println!("layer {layer} is off in this document's default view");
    }
}
```

Inside the workspace, `tinker_pdf_render::Renderer` implements the
interpreter's `Device` trait and pulls outlines, images, shadings, patterns
and tiles through the `GlyphSource` seam — implemented by the facade's
`PageResources`, so no COS type enters the render crate. `page_scale`,
`page_pixels`, `page_canvas` and `page_view_transform` are the geometry
helpers `Page::render` composes, with `region_view_transform` and
`region_canvas_in` the two that take a `PixelRegion`. `None` is not a special
case in the facade: it becomes the region covering the whole page, whose
translation is zero, so a tile and a page take one code path and there is no
un-tiled spelling left for a defect to hide in.

## Refused by name

| What | Typed variant | Why (one line) | See |
|---|---|---|---|
| An image codec not built in | `RenderWarning::UnsupportedImage` | A neutral placeholder is drawn and the codec named | [filters](filters.md) |
| An image decoded with damage tolerated | `RenderWarning::DamagedImage` | Drawn *and* named, so a damaged scan is distinguishable from a blank one (ruling 10) | [rulings](../rulings.md) |
| A page above 67.1 Mpx at the requested scale | `RenderWarning::PageScaledDown` | A whole page smaller beats a fragment at full size (ruling 2) | [rulings](../rulings.md) |
| A font program that could not be read | `RenderWarning::UnreadableFont` | Its glyphs are not drawn; the rest of the page is | [content and text](content-and-text.md) |
| A shading that cannot be read into types 1–7 | `RenderWarning::UnsupportedShading` | Reported by type number rather than guessed at | [rulings](../rulings.md) |
| A pattern whose cell cannot be read, or a lattice past 65 536 positions / 16.7 Mpx / 33.5 Mpx | `RenderWarning::UnsupportedPattern` | Unpainted reads as missing; `/Pattern`'s nominal black reads as content and hides the gap | [rulings](../rulings.md) |
| A layer the default configuration turns off | `RenderWarning::HiddenOptionalContent` | Correct behaviour, reported all the same — the one leniency a reader cannot see | [rulings](../rulings.md) |
| More than 2 000 transparency-group buffers on one page | `RenderWarning::GroupBudgetSpent` | A budget, not a depth: branching soft-mask recursion stays inside any depth cap | [rulings](../rulings.md) |
| A text object that clips and shows no glyphs | `RenderWarning::EmptyTextClip` | Spec-correct and almost never intended | [content and text](content-and-text.md) |
| A render stopped by its `CancelToken` | `RenderWarning::Cancelled` | Reported only when work was actually skipped | — |
| A `RenderOptions::region` reaching past the page edge | `RenderWarning::RegionClamped` | The part on the page is rendered rather than refused (ruling 2), and a bitmap smaller than the rectangle asked for is named rather than left to arithmetic (ruling 10). A region that misses the page entirely trims to no pixels | [rulings](../rulings.md) |
| The document's own CMYK components on a page asked for in ink | stated on `RenderOptions::allow_cmyk` | Colour is flattened to sRGB where a resource is read, so an ink page is light converted back with maximum undercolour removal: a rich black arrives as pure `K`. Separations want the file's components, carried through the resource seam, which is its own row | [ROADMAP](../ROADMAP.md) |
| A PNG read back whose raster stops short of its declared height | `PngReadError::Incomplete`, carrying the decoder's own identifiers | The decoder degrades for a comic page; a file read back to be *compared* would have its missing rows scored as a rendering difference. Every refusal the decoder makes is `PngReadError::Refused` with its own reason | [filters](filters.md) |
| An ICC profile whose data space and tags contradict each other | `ColorSpace::Approximated`, stated on the type | **6 of the corpus's 3 235 profiles**, September 2026, and `icc_census.rs` names all three shapes. Not a capability gap: a matrix over Lab components, a data space no registry defines, and one tone curve for four channels of ink. The fallback is 8.6.5.5's alternate-space reading, which is what every ICC space got before profiles were read | [ROADMAP](../ROADMAP.md) |

## Verified

- Facade integration tests, one file per capability:
  `crates/tinker-pdf/tests/colour_spaces.rs`, `blend_modes.rs`,
  `transparency_groups.rs`, `optional_content.rs`, `mesh_shadings.rs`,
  `text_render_modes.rs`, `images.rs`, `inline_images.rs`,
  `stroke_parameters.rs`, `form_xobjects.rs`, `page_geometry.rs`,
  `annotation_appearances.rs` — each asserting pixels, not absence of error.
- Output options: `crates/tinker-pdf/tests/render_options.rs` pins each
  `RenderOptions` field that changes what a page's bytes are with its own
  SHA-256, computed as `determinism.rs` computes one and floored by ink the same
  way, beside the claim that the default render is unchanged — the blend grid's
  default render still hashes to `determinism.rs`'s `analytic_blend` value.
  The anti-aliasing switch's exit criterion is
  `with_anti_aliasing_off_every_pixel_is_wholly_covered_or_not_at_all`: text,
  a diagonal fill, a stroked curve, a hairline and a rotated image's edge, each
  in its own opaque colour, at 1x, 1.7x and 3x, where every pixel of the
  hard-edged render must be one of six colours and every element must still be
  there by its own; `with_anti_aliasing_off_a_shading_is_wholly_covered_or_not_at_all`
  does the same for a mesh, a clipped `sh`, a shading pattern and a tiling
  pattern whose cell is drawn by a renderer of its own.
- Output: `png_output.rs` holds `Bitmap::to_png` over all six formats, and
  `png_input.rs` holds `Bitmap::from_png` — the round trip over every page
  format, 16-bit rounding at the samples where truncation would differ, every
  decoded layout, both refusals, and a fixed-seed campaign of random and
  mutated files that must never panic and must reach both a picture and a
  refusal more than five hundred times each.
- Regions and ruling 5: `crates/tinker-pdf/tests/render_regions.rs`. Ten
  fixtures over four rasterizer paths, three of them turned and three cropped,
  tiled at 64, 37, 23 and 53 pixels against a 91×131 page and down to a
  one-pixel lattice, each tile asserted **byte-equal** to its rectangle of the
  whole render with no tolerance; plus the trim at the page edge and its
  warning, a region wholly off the page, and — because tile equality alone is
  satisfied by a *consistent* mistake — two fixtures that assert which quadrant
  of the displayed picture one mark lands in, on a rotated page and on a
  cropped one. Every fixture carries an ink floor, because a blank page tiles
  perfectly.
- In-crate: unit tests in `tinker-pdf-render/src/lib.rs` (among them
  `a_small_fill_on_a_large_page_stays_small`, which counts mask pixels asked
  for so an O(canvas) regression fails rather than merely costs, and
  `a_cancelled_clip_and_text_clip_rasterize_nothing`), `shading.rs`,
  `mesh.rs`, and `tinker-pdf-color`'s tests over conversion, palettes, tint
  transforms and all function types.
- Determinism: ten of the 19 render fingerprints in
  `crates/tinker-pdf/tests/determinism.rs` — `text`, `curves`, `shading`,
  `blend`, `pattern`, `optional`, `image`, `transparency`, `tiling`, `mesh`
  — pin this device's output bit-for-bit across x86_64 Windows, Linux and
  `wasm32-wasip1`, each with an ink floor so a fixture that draws nothing
  fails instead of becoming a baseline ([determinism](determinism.md)).
- Fuzzing: `render_page` among the 24 fuzz targets renders whole hostile
  documents; `crates/tinker-pdf/tests/hostile_input.rs` replays the sweep on
  stable.
- Corpus, as of September 2026: 5 525 files, 5 516 rendered every page, zero
  crashes.
- The workspace stands at 4 879 passed / 0 failed / 58 ignored
  (Windows x86_64, 14 September 2026). See [verification](../verification.md).
