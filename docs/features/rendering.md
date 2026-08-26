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
0..1 and clamping them there renders the whole space black. ICC and CIE
spaces are `ColorSpace::Approximated` — read by component count, which is the
alternate-space reading 8.6.5.5 permits, and the approximation is stated on
the type rather than hidden. `[/Pattern base]` carries the underlying space
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
removal at close (11.4.7.2), read from Table 147's `/I` and `/K`. `/CS` is read
too — on a form's group and on the **page's own** group (11.4.7), which reaches
no `Do` and so is read off the page dictionary — and reduced to the shape a
compositor needs: how many components, and whether they are subtractive.
Compositing happens in RGB, which is the same arithmetic for a one- or
three-component group and a different one for CMYK or Lab; those two are
reported by name rather than blended silently, once per space. Making them
right is [design/icc.md](../design/icc.md)'s stage 1. ExtGState
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

## API

The facade is the whole public surface (ruling 11): `Page::render` takes a
`RenderOptions` — `scale` (pixels per point, or `RenderOptions::at_dpi`),
`format` (`PixelFormat`), `cancel` (an optional `CancelToken`, cloneable and
checked between operations and scanline bands) and `annotations` (on by
default) — and returns a `Bitmap`: `width`, `height`, `format`, `stride`,
`data`, and `warnings`, the `Vec<RenderWarning>` that carries every named
degradation. Rendering never fails; it degrades and reports.

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
helpers `Page::render` composes.

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
| Blending a group in the space it declares | `RenderWarning::UnsupportedGroupSpace` | 11.3.5's formulas over subtractive components are not the same arithmetic over additive ones, so a CMYK or Lab group is blended in RGB and **named**. One- and three-component spaces are not reported: for those RGB is the same arithmetic rather than an approximation of it | [ROADMAP](../ROADMAP.md) |
| Exact ICC/CIE colour | `ColorSpace::Approximated`, stated on the type | Component count decides the reading, the 8.6.5.5 fallback | [ROADMAP](../ROADMAP.md) |

## Verified

- Facade integration tests, one file per capability:
  `crates/tinker-pdf/tests/colour_spaces.rs`, `blend_modes.rs`,
  `transparency_groups.rs`, `optional_content.rs`, `mesh_shadings.rs`,
  `text_render_modes.rs`, `images.rs`, `inline_images.rs`,
  `stroke_parameters.rs`, `form_xobjects.rs`, `page_geometry.rs`,
  `annotation_appearances.rs` — each asserting pixels, not absence of error.
- In-crate: unit tests in `tinker-pdf-render/src/lib.rs` (among them
  `a_small_fill_on_a_large_page_stays_small`, which counts mask pixels asked
  for so an O(canvas) regression fails rather than merely costs, and
  `a_cancelled_clip_and_text_clip_rasterize_nothing`), `shading.rs`,
  `mesh.rs`, and `tinker-pdf-color`'s tests over conversion, palettes, tint
  transforms and all function types.
- Determinism: ten of the 15 render fingerprints in
  `crates/tinker-pdf/tests/determinism.rs` — `text`, `curves`, `shading`,
  `blend`, `pattern`, `optional`, `image`, `transparency`, `tiling`, `mesh`
  — pin this device's output bit-for-bit across x86_64 Windows, Linux and
  `wasm32-wasip1`, each with an ink floor so a fixture that draws nothing
  fails instead of becoming a baseline ([determinism](determinism.md)).
- Fuzzing: `render_page` among the 24 fuzz targets renders whole hostile
  documents; `crates/tinker-pdf/tests/hostile_input.rs` replays the sweep on
  stable.
- Corpus, as of August 2026: 4 525 files, 4 484 rendered every page, zero
  crashes.
- The workspace stands at 2 963 passed / 0 failed / 8 ignored
  (Windows x86_64, August 2026). See [verification](../verification.md).
