# Output pixel models

*For [ROADMAP](../ROADMAP.md) row RC-06.* A page is rendered today into one
of the light formats a `Bitmap` names — `Gray8`, `GrayA8`, `Rgb8`, `Rgba8` —
or, asked twice, into 8.6.4.4's ink (`CmykA8` with
`RenderOptions::allow_cmyk`). This design adds four: the two BGR byte orders,
the device values of an ICC destination profile, and 1-bit monochrome by
ordered dither and by error diffusion. It says where each conversion of the
composited page happens, how each result is labelled and handed back, and how
each keeps ruling 4 (integers on the pixel path) and ruling 5 (a tile
byte-equal to the page under it).

## Scope

- **`Bgr8` and `Bgra8`**: blue, green, red, and alpha last — the orders a
  host's surfaces store — as `PixelFormat`s a page comes back in.
- **A destination**: a compiled ICC profile the page is converted into, per
  pixel, its bitmap labelled with the profile's data space.
- **1-bit monochrome**: a packed bilevel bitmap beside `Bitmap`, by ordered
  dither through a default screen anchored at the page frame, and by
  Floyd–Steinberg error diffusion from the page frame's origin.
- **The threshold-array seam** RC-08's `/HT` feeds: a PDF-free cell of
  thresholds in `tinker-pdf-raster`, which the default screen is the first
  user of.
- The order of the conversions at the end of `render_layer_with`, and what
  each composes with: RC-05's proof, RC-08's transfer functions and halftones.
- RC-06.2 to RC-06.7's exit tests.

## Non-goals

- **16 bits a component** — RC-07.
- **LCD sub-pixel text** — RC-10, whose sub-pixel order is a property of the
  glyph's coverage, not of the byte order a page comes back in.
- **The document's own halftones and transfer functions** — RC-08, which
  feeds the threshold-array seam below and applies `/TR` before a page is
  screened ([device-dependent-state](device-dependent-state.md)).
- **A destination for a 1-bit render.** The grey a 1-bit render screens is
  the sRGB render's; a caller wanting a device's grey to screen renders to a
  `GRAY` destination and screens it through `tinker_pdf_raster::halftone`
  itself, which takes any grey plane.
- **Multi-level output** (2 or 4 bits a pixel). 10.6.4 generalises a
  threshold array to it; no step asks for it.
- **The BGR orders and a destination in the C ABI and the bindings.** The
  C ABI keeps its four formats (`TpdfPixelFormat`); the bindings follow with
  SU-12.
- **Reading a PNG's `iCCP`.** `Bitmap::from_png` labels what it reads as
  sRGB, as it does today.

## Design

### Where each conversion happens

Every render — `Page::render`, `render_form`, `render_annotation` and a
display list's — goes through `Page::render_layer_with`, and every output
model is a conversion of the canvas it finishes with, made there, per pixel,
in this order:

1. **The page composites** in its page group's space (11.4.7), or in a
   proof's device space (RC-05), or in the light format the request
   composites in — `Rgb8` or `Rgba8` for the BGR orders.
2. **The canvas is extracted** to the light format the request is handed back
   in, or to ink with `allow_cmyk` (`Canvas::extract`), as today.
3. **A destination converts** that light to its device values (RC-06.5).
4. **Transfer functions** map the output's device components (RC-08.2): the
   destination's, or the light's.
5. **The byte order** is the asked one: `Bgr8` and `Bgra8` reorder.
6. **Premultiplication**, last, as today.

A 1-bit render takes the `Rgba8` render through steps 1 and 2, then its grey,
then the grey transfer function (RC-08.2: a monochrome device uses the grey
function, 10.5), then screens it. Each step reads one pixel and nothing else,
except error diffusion, whose region rule is below; so a tile's pixels are the
page's there (ruling 5) without anything a tile must be told.

### BGR and BGRA (RC-06.2)

**Output orders the canvas never composites in.** `PixelFormat` gains
`Bgr8` and `Bgra8`, each a page format — they store light — and
`tinker_pdf_render::page_format` hands each back as itself.
`tinker_pdf_render::canvas_format` is the format a request composites in:
`Rgb8` for `Bgr8`, `Rgba8` for `Bgra8`, and every other format as itself; the
page canvas is made in it, so every buffer the render opens — a group's, a
soft mask's, a tiling cell's, a mesh's, an image run's fragments — keeps the
formats it has, and no path through the renderer learns a second channel
order. The finished canvas is reordered once, by `Canvas::extract` into the
asked order, which goes through `Canvas::pixel` and `Canvas::encode`: the
colour is the same `Color`, so the reorder is exact.

**The leaf composites a BGR canvas right anyway.** `PixelFormat` is public,
and a caller of `tinker-pdf-raster` may composite into a `Bgr8` canvas. The
separable modes (11.3.5.2) are per channel and do not see an order. The
non-separable ones (11.3.5.3) are defined over red, green and blue, and
`blend` reads a three-channel pixel as RGB; on a BGR canvas it hands them the
channels in RGB order and writes the result back in BGR order. So a BGR
canvas composited directly holds the bytes the RGB canvas holds, reordered,
under every mode — which is what lets the renderer's one reorder at the end
and a caller's direct composite agree.

**Alpha, regions, `transparent` and `premultiplied`** behave as for `Rgba8`:
alpha is last in `Bgra8`, as in `Rgba8`, so `Bitmap::premultiply` and
`straightened` read it where they read it now; `transparent` applies to
`Bgra8` and not to `Bgr8`, as to `Rgba8` and not `Rgb8`.

**`Bitmap::to_png` writes light in PNG's order**: a `Bgr8` bitmap as colour
type 2 and a `Bgra8` one as colour type 6, through a copy reordered to RGB,
so each file is byte for byte the `Rgb8` or `Rgba8` render's. `to_png`'s
table of formats gains the two rows, and `to_png_counted`'s
`png_image_scratch` counts the copy, as it counts `CmykA8`'s.

### 1-bit monochrome (RC-06.3, RC-06.4)

**The type.** `Bilevel`, beside `Bitmap` in the facade:

```rust
/// A page rendered to one bit a pixel.
#[derive(Clone, Debug)]
pub struct Bilevel {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row: `width` bits rounded up to whole bytes.
    pub stride: usize,
    /// Rows from the top; in each, pixels from the left, the most
    /// significant bit first. A set bit is black, a clear one white, as a
    /// PBM file stores them; the bits past `width` in a row's last byte are
    /// clear.
    pub data: Vec<u8>,
    /// What the renderer could not do exactly (ruling 2).
    pub warnings: Vec<RenderWarning>,
    /// What the render counted, when `RenderOptions::stats` asked.
    pub stats: Option<Stats>,
}
```

A 1-bit pixel does not fit `PixelFormat`, whose `components` counts whole
bytes (`crates/tinker-pdf-raster/src/canvas.rs`), so it is a type of its own
rather than a seventh format every exhaustive match would have to refuse.
`Bilevel::to_png` writes a greyscale PNG of bit depth 1 (ISO/IEC 15948 Table
11.1 admits it for colour type 0), where a sample of 0 is black: the bits are
inverted on the way out. The encoder in `tinker-pdf-filters` gains that one
layout, rows of `stride` bytes filtered with one byte as the filter unit
(9.2). `None` comes back only for a bilevel that is not a picture, as for
`Bitmap::to_png`.

**The doors.** `Page::render_bilevel(&RenderOptions, Screening) -> Bilevel`
and `DisplayList::render_bilevel` take the same options as a render, through
the same pipeline: `scale`, `region`, `antialias`, `annotations`, `cancel`
and `stats` apply; `format`, `allow_cmyk`, `transparent` and `premultiplied`
do not, since a bilevel page has one layout and no alpha.

```rust
/// How a 1-bit render decides each pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Screening {
    /// Ordered dither through the default screen.
    OrderedDither,
    /// Floyd–Steinberg error diffusion.
    ErrorDiffusion,
}
```

RC-08.9 adds the screening that reads the document's halftones; neither of
these does (10.6.5.1 lets a processor "use different halftones that may be
better suited for that device").

**The grey.** The page is rendered as its `Rgba8` render, opaque (a bilevel
page has no alpha, so `transparent` does not reach it), and each pixel's grey
is the one a `Gray8` buffer stores it as — `Color::luma`, the Rec. 601
weights summing to 1 000, what `Canvas::extract` to `Gray8` computes. So a
bilevel page is the `Gray8` extraction of the colour render rather than a
render that composites in grey, which would blend in one channel and part
from the colour page wherever a non-separable mode or a group's conversion
reads three.

**The default screen** is the 16 × 16 dispersed-dot matrix: `B₁ = [0 2; 3 1]`
and `B₂ₙ = [4Bₙ 4Bₙ+2; 4Bₙ+3 4Bₙ+1]`, so `B₁₆` holds each of 0 to 255 once.
Its threshold at `(i, j)` is `min(B₁₆(i, j) + 1, 255)`, and a pixel of grey
`g` is black exactly when `g < threshold` — 10.6.4's rule, under which a grey
of 0 paints every pixel black (no threshold is below 1) and 255 none. The
`+1` and the clamp are what make a flat grey come out as 10.6.2 says, "a gray
value g … shall be produced by making i pixels white, where i = floor(g × n)":
over one 256-pixel period a flat grey `g` (a byte) whitens
`floor(g × 256 / 255)` pixels — `g` of them up to 254 and all 256 at 255 —
which `a_flat_grey_dithers_to_its_own_share_of_black` holds level by level.

**The anchor is the page frame.** The threshold at device pixel `(x, y)` is
the cell's at `((x − ox) mod 16, (y − oy) mod 16)`, the remainder taken
non-negative, with `(ox, oy)` the screen's origin in device pixels of the
whole page — `(0, 0)` for the default screen, `/HTO`'s point for a document's
(RC-08.9). A tile's pixel is the page's pixel at the same device coordinates
(`Canvas::origin`), so it reads the same threshold and a dithered tile is the
dithered page under it. 10.6.2 asks the same of a screen: "defined entirely
in device space and … unaffected by modifications to the current
transformation matrix".

**The threshold-array seam.** `tinker_pdf_raster::halftone` takes a grey
plane and a cell and knows no PDF (ruling 8):

```rust
/// A screen: thresholds tiled over device space from an origin.
pub struct Cell { /* width, height, thresholds as u16, origin */ }

/// Screens `grey` — `width` × `height` bytes standing at device pixel `at` —
/// into packed bits, each black where its grey is below the cell's
/// threshold at that device pixel.
pub fn screen(grey: &[u8], width: u32, height: u32, at: (i32, i32), cell: &Cell) -> Vec<u8>;
```

Thresholds are 16-bit: an 8-bit one is widened by 257, and so is the grey,
so 8- and 16-bit arrays (types 6 and 16) are one comparison in integers; a
threshold of 0 reads as 1, as 10.6.4 says. The default screen is one
rectangle; RC-08.8 widens `Cell` to the shapes a halftone dictionary
describes — two rectangles (type 16) and type 10's two squares — tiled from
the same origin, through the same `screen`.

**Error diffusion** is Floyd–Steinberg — of each pixel's error, 7/16 to the
right, 3/16 below-left, 5/16 below, 1/16 below-right — in raster order, left
to right along every row, from row 0 of the page frame. It is integer
throughout: the error carried into a pixel is a count of sixteenths of a
level, `T = 16 × grey + carried`; the pixel is white when `T ≥ 16 × 128`, its
error `E = T − 16 × out` with `out` 255 or 0, and the shares are
`(7E).div_euclid(16)`, `(3E).div_euclid(16)` and `(5E).div_euclid(16)` with
the below-right neighbour taking `E` less the three, so no sixteenth is lost
to rounding inside the page. A share that would land outside the page is
dropped, and the leaf's diffusion reports the sixteenths it dropped, `L`,
beside the bits. A pixel's error is under 128 levels before the rounding its
four received shares carry, under three sixteenths a pixel, so the arithmetic
fits `i32` whatever the page's size. Because nothing is lost inside the
field, a flat field `W` wide and `H` tall of grey `g` whitens exactly the
pixels the conservation gives, `16 × 255 × whites = 16 × g × W × H − L`, and
`a_flat_grey_diffuses_to_its_own_share_of_black` holds the black count to
that closed form, the error carried off the field's edges the only term the
diffusion supplies. In the leaf it is a value fed the page a strip at a time:

```rust
/// Floyd–Steinberg over a page `width` pixels wide, from its first row.
pub struct Diffusion { /* width, the error carried into the next row, L */ }

impl Diffusion {
    pub fn new(width: u32) -> Diffusion;
    /// Diffuses the page's next `rows` rows of grey; their packed bits.
    pub fn rows(&mut self, grey: &[u8], rows: u32) -> Vec<u8>;
    /// The sixteenths dropped at the left and right edges so far, and those
    /// carried into the row below the last one diffused, which leave the
    /// page if it ends there.
    pub fn dropped(&self) -> i64;
}
```

**What a region pays to stay byte-equal.** A diffused pixel depends on every
pixel before it in raster order: the rows above it across the whole page, and
the pixels left of it in its own row. So a region is diffused from the page's
first row, across the page's full width, down to the region's last row, and
its window is kept; nothing relaxes ruling 5's byte equality. The band is
rendered in strips of the page's rows, each a region render and so byte-equal
to the page there, and diffused as it comes, the two error rows carried from
strip to strip: memory is a strip's, and time is the band's — a tile at the
foot of the page pays for the page above it. A caller tiling a page for
diffusion renders it whole, or in bands from the top, and pays once.

### A destination (RC-06.5)

**Built once.** `tinker_pdf_color::icc::Destination::new(bytes, intent)`
compiles sRGB into the profile's device space — IEC 61966-2-1's decode as a
256-entry table a channel, `SRGB_TO_XYZ_D50` in s15.16, and RC-20's output
direction at the intent (relative colorimetric unless the caller names
another; the intents are RC-01's) — and returns `Result<Destination,
DestinationError>`: `Profile(IccError)` for bytes the parser refuses,
`NoOutputDirection` for a profile RC-20 cannot invert or that carries no
`B2A*`, `DataSpace([u8; 4])` for a data space other than `GRAY`, `RGB ` and
`CMYK`, and `Class([u8; 4])` for a device link, an abstract or a
named-colour profile. `RenderOptions::destination` holds an
`Option<Arc<Destination>>`, so one compiled profile serves every page.

**Applied per pixel at the end of `render_layer_with`** (step 3 above): each
pixel's light through the destination, alpha kept, through a 4 096-slot memo
keyed by the pixel — the `/DeviceN` image path's table — which holds what the
conversion returns and so changes no byte. Integer per pixel (ruling 4); one
pixel in, one out (ruling 5). With a proof (RC-05) the display leg targets
the destination directly, through the connection space once, rather than
through sRGB.

**The layout and the label.** A destination decides the layout: `GRAY` is
`Gray8`, or `GrayA8` when the asked format has alpha; `RGB ` is `Rgb8` or
`Rgba8`; `CMYK` is `CmykA8`, the one four-channel layout, its alpha 255 on an
opaque page. `allow_cmyk` and the BGR orders do not apply. A layout says how
many components there are and nothing about what they mean, which is
`page_format`'s whole argument, so `Bitmap` gains a label:

```rust
/// What a bitmap's colour components are.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BitmapSpace {
    /// sRGB light, or its grey: every render without a destination.
    Srgb,
    /// 8.6.4.4's ink: a page asked for in `CmykA8` with `allow_cmyk`.
    Ink,
    /// A destination's device values: its data space (`GRAY`, `RGB `,
    /// `CMYK`) and the profile they are values of.
    Device { data_space: [u8; 4], profile: Arc<[u8]> },
}
```

`Bitmap::space` is `Srgb` for every render that exists today, and for every
bitmap `Bitmap::from_png` reads.

**`to_png` refuses device data it cannot write as light.** `GRAY` and
`RGB ` device values are written as their bytes under colour types 0, 2, 4
or 6 with the profile in an `iCCP` chunk (ISO/IEC 15948 11.3.3.3: a profile
name, compression method 0, the profile deflated), which the encoder gains,
so a reader that manages colour shows the light they are and none mistakes
them for sRGB. `CMYK` device values come back `None`: PNG has no colour type
for them, and writing them as light would need the profile's input direction,
a conversion this method does not make — the same reason `page_format` keeps
ink from a caller who did not ask for it twice.

## Milestones

1. **RC-06.2, BGR and BGRA.** `PixelFormat::Bgr8` and `Bgra8`;
   `canvas_format` and `page_format`; `Canvas::encode`, `pixel`, `components`,
   `has_alpha`, `color_channels` and `blend`'s non-separable order;
   `to_png`'s rows. Exit: `render_options.rs`'s
   `bgr_and_bgra_are_the_rgba8_render_reordered` — on the blend grid and a
   text page, with `transparent` and `premultiplied` each on and off, every
   `Bgr8` and `Bgra8` byte the `Rgba8` render's pixel reordered in the test
   (alpha dropped for `Bgr8`, whose page is on white whatever `transparent`
   says), and `to_png` of each the `Rgb8` or `Rgba8` render's file;
   `only_formats_that_store_light_are_page_formats` decides both;
   `canvas.rs`'s `a_bgr_canvas_composites_as_the_rgb_canvas_reordered` for
   every blend mode; `render_regions.rs`'s `bgra_tiles_are_the_bgra_page`;
   both builds.
2. **RC-06.3, ordered dither.** `tinker_pdf_raster::halftone` with `Cell` and
   `screen`, the default screen, `Bilevel`, `Screening::OrderedDither`, the
   doors and the 1-bit PNG layout. Exit: `render_options.rs`'s
   `an_ordered_dither_is_the_rgba8_render_thresholded_in_the_test` — every
   bit the test's own comparison of the `Rgba8` render's grey with the
   matrix at that pixel's page-frame position, packed as above;
   `halftone.rs`'s `a_flat_grey_dithers_to_its_own_share_of_black`;
   `render_regions.rs`'s `dithered_tiles_are_the_dithered_page`; both builds.
3. **RC-06.4, error diffusion.** `Screening::ErrorDiffusion`, the band
   rendered in strips. Exit: `render_options.rs`'s
   `an_error_diffusion_is_the_rgba8_render_diffused_in_the_test` — every bit
   the test's own integer diffusion of the `Rgba8` render's grey by the
   kernel and order above; `halftone.rs`'s
   `a_flat_grey_diffuses_to_its_own_share_of_black`; `render_regions.rs`'s
   `diffused_tiles_are_the_diffused_page`, a tile at the page's foot among
   them; both builds.
4. **RC-06.5, a destination.** `icc::Destination`, `RenderOptions::destination`,
   `BitmapSpace`, the `iCCP` chunk. Exit: `render_options.rs`'s
   `a_destination_render_is_the_rgba8_render_converted_in_the_test` — for a
   matrix/TRC RGB, a grey and a LUT CMYK destination built in the test, every
   pixel the `Rgba8` render's converted in the test from the profile's
   equations (curves and matrix in closed form, the grid at its nodes), each
   bitmap labelled with its data space and `to_png` writing the first two
   with their profile and refusing the third; `icc.rs`'s
   `srgb_composed_with_the_output_direction_is_one_table`; each
   `DestinationError` reached; `render_regions.rs`'s
   `destination_tiles_are_the_destination_page`; the `icc_profile` fuzz
   target builds one.
5. **RC-06.6, halftone fingerprints.** `render_options.rs`'s
   `halftones_are_stable_across_targets`: the SHA-256 of a page of a grey
   ramp, a shading, an image and text in an embedded face, to 1 bit by each
   screening, on four targets, in K-07's roster.
6. **RC-06.7a and RC-06.7b, the speed rows**: a text-and-image page at
   150 dpi to `Bgra8` and to a LUT CMYK destination, and at 300 dpi to 1 bit
   by each screening, beside its `Rgba8` render; RC-06.7b takes the figures.

## Risks

| Risk | Mitigation |
| --- | --- |
| A BGR page path composites in a reordered buffer and a non-separable blend reads it wrong | The renderer never composites in BGR (`canvas_format`); the leaf reorders the non-separable operands for a caller who does, and `a_bgr_canvas_composites_as_the_rgb_canvas_reordered` holds every mode |
| Error diffusion makes a tile at the foot of a page cost the page | Stated as ruling 5's price rather than relaxed; the band is rendered in strips so memory stays a strip's; a caller tiling for diffusion renders in bands from the top |
| A destination's device values are read as light | `BitmapSpace` labels them; `to_png` writes grey and RGB device values with their profile and refuses CMYK ones |
| The default screen and the document's screens drift into two paths | One `screen` over one `Cell`; RC-08.8 widens the cell and RC-06's dither tests pass unchanged |
| A threshold or error rule that differs across targets | Integer comparison and integer sixteenths with one stated rounding (`div_euclid`); RC-06.6 fingerprints both screenings on four targets |
| The 1-bit PNG layout is a second encoder path | One filter unit and the same zlib stream as the four layouts; `Bilevel::to_png` read back by `png_decode` is the bits, which RC-06.3's test checks |
