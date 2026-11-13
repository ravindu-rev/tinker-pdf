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
0..1 and clamping them there renders the whole space black; `a*` and `b*` are
held to `/Range` in whichever order its pairs come, and a backwards pair is
named — `RenderWarning::RepairedColorSpace` with the resource name the space
was reached by and `LabRangeUnordered`, on a render, every display-list replay
and a page written as SVG
(`an_unordered_lab_range_is_named_wherever_it_is_read`). An `ICCBased` space
is converted through **its own profile** (ICC.1): the header and tag table are
read, the three `XYZ` columns and three tone curves compile once into
fixed-point tables, and each colour is a lookup plus an integer matrix
multiply, so nothing on the pixel path evaluates a transcendental and ruling 4
holds. Grey profiles are the same with one curve. A printer profile carries a
multi-dimensional lookup table instead — `mft1` or `mft2` at an `A2B*` tag,
three stages of curve with an interpolated grid between them. **v4's `mAB `
reads too**, which is five optional stages where v2 has three fixed ones — A
curves, a grid whose axes may each carry their own point count, M curves, a
matrix and B curves, run in that order although the header lists their
offsets in the reverse. A grey profile whose connection space is `Lab` rather
than `XYZ` reads as well: its one curve gives lightness instead of luminance,
and the two differ by 8.6.5.4's cube root. A `D2B*` tag's floating-point
transform reads as well (`multiProcessElementsType`: curve sets of formula
and sampled segments, matrices with offsets and grids), compiled once into
integer stages, and converts its intent before that intent's `A2B*` table;
a tag holding an element of another type is refused by the element's name
and passed over. A lookup table over one, three or four inputs, and a
`D2B*` tag's elements over as many, convert through an integer grid of
their own pipeline — 256, 33 or 17 nodes an axis, each holding the
pipeline's linear sRGB, evaluated the first time a conversion reaches it and
kept — read linearly, tetrahedrally, or tetrahedrally over the first three
inputs and linearly in the fourth, in 16-bit fixed point, then encoded as the
pipeline encodes: every node is the pipeline's colour exactly, and between
them a read is within two levels of 255 (`docs/design/icc.md`, P-22). A fill
and an image sample of one colour read the same nodes. A table or a `D2B*`
tag over any other input count, a matrix/TRC profile and a grey one convert
through their pipeline. A table over more than six inputs — a seven- or
eight-ink printer profile — is interpolated one axis at a time, so no weight
overflows. A profile carrying a table for more than one rendering intent
keeps each, and compiles one the first time an intent selects it, by ICC.1
8.10.2's precedence (`docs/design/icc.md`). A profile may carry tables and a
matrix/TRC or grey model both, and 8.10.2 puts the tables first: an intent
converts through the model only where the profile carries neither the
`A2B*` tag designated for it nor `A2B0`. A flat colour — a fill, a stroke,
a glyph, a stencil mask, an uncoloured pattern's colour — converts in the
intent in force at its painting operator, `ri`'s or a graphics state's
`/RI`, a name 8.6.5.8 does not list read as relative colorimetric: the
interpreter keeps beside each colour the space and components it was set
with (`GraphicsState::fill_source`, `ColorSource`) and resolves it again
when the intent changes, so a colour set before `ri` paints in the intent at
the paint (11.7.5.3) and `Q` restores it with the intent it was resolved
in. An image and a shading convert in relative colorimetric, 8.6.5.8's
initial intent. Relative colorimetric converts through the first of `D2B1`,
`A2B1`, `A2B0`, the model and `A2B2` the profile carries. Absolute
colorimetric converts through `D2B3` as it stands where the profile carries
one, that tag being ICC-absolute already, and otherwise is relative
colorimetric's `A2B*` table or model with the connection-space value scaled
by the profile's media white (`wtpt`) over the PCS white, channel by channel
(ICC.1 6.2.3): folded into the matrix for a matrix/TRC or grey profile, so a
grey on a tinted medium draws in its tint, and applied in XYZ to a table's
value, a stage its grid's nodes evaluate. A media white of D50, or none,
makes it relative colorimetric's transform itself.

A table that does not read is passed over to the next of those, and so is a
model that contradicts its profile — a monitor profile whose data space is
`LAB` carrying the `rXYZ` columns a matrix applies to linear RGB; one naming a
three-channel space no registry defines; a CMYK printer profile with a single
`kTRC` and no matrix, one curve for four channels of ink — so such a profile
converts through its tables. One with no table it can convert through is
`ColorSpace::Approximated`, read by component count, which is the
alternate-space reading 8.6.5.5 permits.

**`CalGray` and `CalRGB` convert through their own parameters** (8.6.5.2,
8.6.5.3): the components go through `/Gamma`, `/Matrix` takes them into XYZ
relative to `/WhitePoint`, and the white point is adapted to D50 before the
sRGB matrix.

The adaptation is **von Kries in XYZ**, scaling each axis by the ratio of the
two whites, and not the Bradford transform baked into the profile path's
matrix. Named rather than hidden: the two differ on saturated colours far from
the neutral axis. A bare `/CalGray` or `/CalRGB` *name*, with no parameter
dictionary behind it, is the device space — there is nothing else the file
has said. `/G`, `/RGB` and `/CMYK` are Table 93's inline-image abbreviations
and are read as the device spaces only where no `/ColorSpace` resource has the
name: 8.6.8 gives `cs` a device space's own name or a resource. A `/Lab`
image with no `/Decode` takes Table 90's default — `L*` over 0..100, `a*` and
`b*` over the space's `/Range`; `writer_cie.rs` holds both, with the
CIE-based spaces this engine writes. `[/Pattern base]` carries the underlying
space of an uncoloured pattern (8.7.3.2), so an `scn`'s components reach the
paint. Initial colours follow 8.6.8 — CMYK starts at full black ink, not all
zeros. Out-of-range components clamp rather than wrap.

**Default colour spaces** (8.6.5.6). A scope's `/DefaultGray`, `/DefaultRGB`
and `/DefaultCMYK` — entries of the `/ColorSpace` subdictionary of a page's, a
form's, a tiling cell's or an appearance stream's resources — stand in for
the device space each is named for wherever one is selected for painting:
`g`/`G`, `rg`/`RG` and `k`/`K`; `cs` naming the device space, a `/ColorSpace`
entry that is one, Table 93's `/G`, `/RGB` and `/CMYK` and a bare `/CalGray`
or `/CalRGB` name; `sc`/`scn` with no space named, read in the family its
count gives; a sampled image's `/ColorSpace`, a JPEG's and a JPEG 2000
image's among them, and an inline image's `/CS`; a shading's, under `sh` and in a shading pattern; an `/Indexed` base, an
uncoloured pattern's base, and the alternate of a `/Separation` or `/DeviceN`
space — not of `/Separation /All` or `/None`, which 8.6.6.4 is read as never
selecting; a group's `/CS`, and with it a luminosity mask's `/BC`. A default
is read in the scope the stream's other names resolve in: a page's stops at a
form or a cell with `/Resources` of its own, empty ones included (8.10.1), a
form's applies inside it only, and a form group's `/CS` is remapped by the
scope its contents run in. A Type 3 glyph and a soft-mask group run in the
scope that invokes them, and so do their defaults
([design/icc.md](../design/icc.md)). A group keeps only its space's shape,
which a default cannot change, so remapping its `/CS` moves no pixel; the
`/BC` is where it shows. `cs` resets to the device space's initial colour
carried into the default, so `/DeviceCMYK cs` paints what `0 0 0 1 k` paints
where an `ICCBased` default's own zeros would be no ink at all. A default's
own spaces are read as written and never remapped again — `/DefaultRGB
/DeviceRGB` is a legal default, and no default at all. A `/Lab`, `/Indexed`
or `/Pattern` default, one with another number of components and one that is
no colour space is refused: the device space is drawn as it would be without
it, and the refusal is named (the refusal table). A JPEG 2000 codestream
with no `/ColorSpace` over it carries its own colour, which selects no
device space and is not remapped. A scope's defaults are read once, on its first device-space
question, and the interpreter asks once a scope which families remap
(`FontSource::device_defaults`), so a page naming none pays a load and a
branch per selection and one flag check per device operator. A space a
resource name selects is read once a scope too, at the first `cs`, `CS`, `sc`
or `scn` that names it, as painted and as selected together, and kept for
every render over the scope; a render that finds it kept names again what
reading it repaired, so a render's warnings never depend on whether it read
the space. A function an object holds — a shading's, a tint transform, a
graphics state's — is read once a scope as well.
`tests/default_spaces.rs` holds every path and scope above to a twin naming
the substitute directly, byte for byte, under CalGray and ICCBased N=1,
CalRGB and ICCBased N=3, and ICCBased N=4, each beside the page without its
default that it must not equal
(`each_default_space_draws_as_its_substitute_named_directly`). `to_svg`
writes the interpreter's colour and decodes images and shadings as the
renderer does, so every path writes its twin's markup
(`a_default_space_page_writes_the_svg_its_twin_writes`), and `Page::images`
reports, beside the space an image states, the space a default puts in for
it, by the same rules
(`an_extracted_image_names_the_space_its_samples_are_in`; content-and-text,
"Image extraction").

`g`, `rg` and `k` select their device space, so a following `sc` reads in the
family the operator selected (8.6.8). An inline image's `[/I …]` is Table
94's `/Indexed`, a palette written as a literal string is read as its bytes,
and an inline image naming a `/ColorSpace` resource (8.9.7), or an `/Indexed`
one over a resource, is drawn in the space it names, as `Page::images` lists
it (`the_selections_a_default_rides_on_read_what_the_file_names`).

**Functions** (7.10). All four types: sampled (`Function::Sampled`, 7.10.2),
exponential (`Function::Exponential`, 7.10.3), stitching
(`Function::Stitching`, 7.10.4) and the PostScript calculator
(`Function::PostScript`, 7.10.5, bounded on every axis: a 100-entry stack, 32
levels of `if`/`ifelse`, 65 536 tokens, no loop operator), plus
`Function::Array` for the arrays a `/Function` entry may hold. A shading's
one `/Function` may be a sampled or calculator stream reached through its
reference; `tests/shading_functions.rs` holds both stream types to their own
colours. A sampled function of more than one input — a `/DeviceN` tint
transform, a type 1 shading's — is 7.10.2's multilinear interpolation, the
first input varying fastest, across up to eight inputs and at the nearest
sample past that, where 2^m corners a lookup stops being a cost worth paying
(`a_two_input_sampled_function_shades_across_both_inputs`). A type 1
shading's `/Matrix` is not read. An evaluation enumerates only the axes the
point is strictly inside, reads a byte-aligned sample in a few instructions
and reads no more than 8 192 samples whatever `/Range` asks for — past that,
the nearest sample, as past eight inputs
(`one_evaluation_reads_no_more_than_its_budget`). `Function::eval_into`
evaluates into a buffer the caller keeps, the outputs `Function::eval`
returns to the bit, and allocates nothing once the buffer has grown: a
table's sums are taken in it, and a calculator's operand stack runs in it
above what an array's earlier members wrote, out of the program's reach, with
eight waiting `{ … }` procedures held in place
(`eval_into_starts_afresh_and_a_type_4_member_keeps_to_its_own_stack`). An image in any space other
than the device spaces converts each colour once, remembered by its samples
in a table of 4 096 slots that never answers one colour for another
(`an_images_repeated_colour_is_converted_once`,
`a_device_n_images_pixels_are_each_their_own_tint`). A picture of a million
distinct colours costs a million evaluations, which is the clause's price, as
is a calculator's 65 536 tokens a pixel.

**Shadings** (8.7.4.5). Type 1 (function-based) is evaluated at every
pixel through `Shading::color_at`; the radial parameter is the largest circle
in the family containing the point (8.7.4.5.4), which is what paints
front-to-back correctly. Types 2 (axial) and 3 (radial) are read a device row
at a time (`Shading::row`): an axial shading's `t` steps from column to column
by one addition and a radial shading's quadratic terms by three, a radial
pixel then taking its square root, restarting from the clause's expression at
every 64th column of the page frame and never at a region's edge, so a tile
steps exactly as the page does (ruling 5). A stepped pixel is within one level
of 255 of the expression evaluated at it (ruling 22,
`a_stepped_parameter_paints_within_a_level_of_the_clause`, and
`render_analytic.rs`'s two shading pages); where the steps are too near a
point the clause decides on — where a gradient that does not extend starts or
stops, where a circle's radius reaches zero, where no circle reaches the
point — or where their error bound is past 2^-24, the pixel takes the
expression, so a shading paints the pixels the expression paints. A
function-based shading, a radial family whose quadratic is linear, and an axis
a pixel crosses more than eight thousand times are evaluated at every pixel.
A one-input function of types 0, 2 or 3, or an array of them
(`Function::tabulable`), is read through a `ColorTable` wherever the shape's
rectangle on the whole page holds more pixels than the table has entries:
its colours, converted, at 1 025 evenly spaced inputs — `0` to `1` for an
axial or radial shading, the span of its vertices' inputs for a mesh — each
evaluated once (`a_shading_evaluates_its_function_at_most_once_per_table_entry`),
and read per pixel through the input in 16.16 fixed point, the two entries
either side mixed in integers. A lookup is a function of the input alone, so
a tile reads what the page reads, and the decision is taken on the page's
rectangle, never the tile's (ruling 5). At an entry a lookup is the
function's colour; between entries it is within a level of 255 of it wherever
two intervals' middle entry lies within a level of the mean of their ends,
and where it does not — a stitch's jump, a curve's steep foot — the input is
evaluated as it is asked
(`a_table_agrees_with_the_clause_at_and_between_its_entries`, ruling 22). A
type 4 function is evaluated at every pixel. Types 4–7 —
free-form and lattice-form triangle meshes, Coons and tensor patches
(8.7.4.5.5–8.7.4.5.8) — are geometry, carried as a `Mesh`: a Coons patch is
stored as the tensor patch 8.7.4.5.7's formula makes it, so there is one
surface evaluator; patches subdivide at a fixed step count decided on 16.16
integers (ruling 4, no float termination test on a pixel path); and the whole
mesh renders through a single coverage buffer from one non-zero fill, so
shared edges do not seam against the backdrop. Bounds: 262 144 vertices,
16 384 patches, 131 072 triangles total, 32 subdivisions per edge. `sh`
fills the current clip (8.7.4.2). Each sweep — `sh`, a shading pattern's fill
and a mesh's colour pass — keeps one `ShadingScratch` for all its pixels: the
function evaluates into it, and a `/Separation` or `/DeviceN` space's tint
transform converts through it (`ColorSpace::to_rgb_with`), so painting a
shading allocates nothing per pixel (`tests/shading_work.rs`, with MS-01's
counting allocator). A row's inverse-map products `c·y` and `d·y` are taken
once a row and each pixel maps to the bits `Matrix::apply` gives it
(`a_row_map_is_the_matrix_s_map_to_the_bit`).

**Patterns** (8.7.3). Tiling patterns paint in both paint types, anchored to
the *parent content stream's default space* as 8.7.3.1 requires — not the
CTM in force at paint time, which is the whole correctness question in a
tiling pattern. The cell is rasterised once into a `/BBox`-sized offscreen
canvas, with the pattern's own `/Resources`, and blitted across the lattice.
A `PaintType 2` cell is a shape (8.7.3.3): it is asked for with no colour
and drawn as its content stream draws it, and the blit composites its
coverage in the underlying-space colour (`Canvas::composite_tinted`, which
writes exactly what recolouring the cell and compositing it wrote), so what
a cell holds is the same whatever colour it is filled in and the colour
operators inside it reach nothing. The lattice is bounded before a pixel
exists: 65 536 positions, 16.7 Mpx per cell buffer, and 33.5 million square points of compositing per
fill — pixels at 72 dpi, counted in the default space and not in device
pixels, so a lattice is drawn or refused alike at every scale and a page
does not lose its pattern at a higher resolution — and past any bound the
fill paints nothing and names the pattern. A pattern that fills with
itself terminates at a nesting depth of 4. Shading patterns paint through
the same shading machinery, and pattern *strokes* route through fill-of-
outline, so both slots share one warning path and cannot diverge: the
outline the pen writes into the fill's edges is the shape a pattern fills,
its bounds deciding the lattice as a path's do.

**Images.** `/SMask` (Table 89, 11.6.5.3), `/Decode` (8.9.5.2), stencil
masks that take the current fill colour (8.9.6.2) and inline images (8.9.7)
all draw. `/Interpolate` (Table 89) selects bilinear sampling on
magnification; minification averages every sample a pixel covers, through a
box pyramid that bounds the footprint ([rasterizer](rasterizer.md)). A codec this build
does not decode draws a placeholder and is named; an image that decoded with
damage tolerated is drawn *and* reported, so a half-decoded fax stays
distinguishable from a blank one.

An image XObject and an inline image are read by one sample loop. An 8-bit
DeviceGray or DeviceRGB image whose `/Decode` pairs are each `[0 1]` or
absent, with no colour key and no `/ImageMask`, is its own colour: its
samples are copied, a grey replicated to three, and none is converted
(`an_eight_bit_device_image_converts_by_identity`). A one-bit image of one
component — a fax, a JBIG2 region, a one-bit Flate or uncompressed stream, an
`/ImageMask` — has two samples: what each becomes through `/Decode`, its
space and a colour key, or a mask's paint, is worked out once. Such an image
is held as those two colours and one byte a sample
(`DecodedImage::two_colour`), each byte of a row expanded to its eight
samples' bits through a table, and the rasterizer samples it in one channel
as it would sample its RGB expansion ([rasterizer](rasterizer.md)), so a
2480 by 3508 scan is held at one byte a sample rather than three or four, and
so is the copy the image cache hands out on a hit
(`a_bilevel_image_is_held_at_one_byte_a_sample`,
`two_colour_samples_are_the_one_bit_expansion`,
`a_two_colour_image_blits_as_its_rgb_expansion`). A colour key selecting one
of the two raw values is that colour's alpha of zero. An image under a soft
mask, which is composed into its alpha a sample at a time, and one whose
colour key selects both values are held as RGB, expanded a byte at a time
through a 256-entry table of eight samples
(`one_bit_samples_expand_through_the_table`); `Page::to_svg` writes a
two-colour image's PNG as its RGB expansion
(`a_two_colour_image_is_written_as_its_rgb_expansion`). Every other image is
read a sample at a time through its space.

**JPEG and JPEG 2000 images** are drawn in the `/ColorSpace` their
dictionary names (8.9.5) — a codestream's own colour specification gives way
to it (7.4.9) — and their samples are read through the space as a sampled
image's are, so the same samples written as a stream in the same space draw
the same bytes. A JPEG's samples take `/Decode` first; a codestream's take
none (8.9.5.4), at its own precision, eight bits or sixteen. With no space
named, or with the device space of the samples' own count named, the image
is drawn in the codec's own colour: one component grey, three RGB, four
CMYK, a codestream's colour specification read for nothing more. A space of
another number of components than the samples cannot hold them; it is
refused and named, and the codec's own colour drawn (the refusal table).
`images.rs` and `jpx_images.rs` hold each to its twin
(`a_jpeg_draws_in_the_space_its_dictionary_names`,
`a_jpx_image_draws_in_the_space_its_dictionary_names` and their inline
twins). **A JPEG is decoded straight into its image**: the decoder hands its
strips, one MCU row at a time, to a sink that converts each into the image's
RGB as it arrives, so no whole decoded image and no second copy of its
colour exist beside the image, for an image XObject and an inline image alike
(`a_jpeg_decodes_into_its_image_without_a_second_copy`); `Page::images` and
image recoding keep the decoder's whole samples.

**Images drawn smaller than they are.** Every image draw asks for the size it
is drawn at (`ImageRequest`): the device pixels its unit square spans along
each of the image's own axes, the lengths of the two columns of its transform
in the page frame, which a tile, a band and a display list's replay compute as
the page does (ruling 5). A JPEG is decoded at the smallest of an eighth, a
quarter and a half whose image still has at least as many samples as drawn
pixels on both axes — the scale chosen from the frame's own size as the decode
reads it, so the reduced grid is never drawn magnified — and at full size when
none has, when the drawn size is not finite, and whenever the image has a soft
mask or a `/Mask`, which are sampled against the grid they were drawn for. A
reduced image says which image it stands for (`DecodedImage::reduced`), and the
draw maps the unit square onto the samples the full image's would span, so its
last column and row are cut where the image ends rather than stretched to the
square's edge. The image cache keeps each size's decode apart, so a hit is
exactly what that size's miss computes. `Page::to_svg`, `Page::images` and
recoding ask for the image whole. `images.rs` holds it:
`a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel` (A7's JPEG half,
through `Stats::jpeg_samples_transformed`),
`a_jpeg_is_decoded_at_the_smallest_scale_that_covers_its_drawn_size`,
`a_masked_jpeg_is_decoded_at_full_size`,
`a_reduced_jpeg_covers_what_the_image_covers`,
`each_scale_of_a_jpeg_is_cached_apart`,
`an_inline_jpeg_is_decoded_at_its_drawn_size` and
`the_svg_writer_and_the_image_list_take_a_jpeg_whole`; the `jpeg_minified`
fingerprint ([determinism](determinism.md)) and `render_regions.rs` hold the
reduced draws on every target and tile by tile.

**Form XObjects.** A form's own `/Resources` are consulted (8.10.1), so a form
pasted in from another document resolves its names in the dictionary it brought
rather than in the page's. Both seams change scope together — the interpreter
resolves fonts, colours and ExtGState, the device resolves images, shadings and
patterns — because they are asked about the same form, by the same name, at the
same moment. A form that omits the key falls back to the invoking scope, which
is what its producer is relying on. An annotation's appearance stream gets the
same treatment by a different route: it is reached by reference rather than by
name, so `Page::render` announces it instead of the interpreter.

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
never handed back in Lab (`a_page_asked_for_in_lab_comes_back_in_rgb`), and in
CMYK only when asked for twice, because a `Bitmap` says how many components it
has and nothing about what they mean.

**CMYK page output** is the one way past that, and it takes two fields rather
than one: `format: PixelFormat::CmykA8` *and* `RenderOptions::allow_cmyk`, so a
caller has said they know the bytes are ink. The page composites over ink on a
canvas that starts with none, and the buffer comes back as it stands. **A
DeviceCMYK colour is the document's own ink**: the interpreter keeps the
components a fill, stroke or glyph colour was set with in DeviceCMYK — `k`,
`K`, and `sc`/`scn` in a space the resource seam resolves to DeviceCMYK
(`FontSource::resolve_ink`) — beside the light it stands for
(`GraphicsState::fill_ink`, `stroke_ink`), and a CMYK canvas composites those
(`Canvas::fill_mask_inked`), so `1 0 0 0 k` arrives as `(255, 0, 0, 0)` and a
rich black `1 1 1 1 k` as all four inks. A stencil mask is painted in the fill
colour (8.9.6.2), a flat colour like any fill's, so it is handed the fill's ink
too (`ImageDraw::ink`, and a run of abutting stencils is kept to one ink and
composited with it; `a_stencil_mask_paints_the_fill_colour_s_own_ink`). A
non-isolated group over an ink page starts from the page's own ink and takes
that ink out again at close (11.4.4, 11.4.7.2;
`a_non_isolated_group_over_ink_blends_against_the_page_s_own_ink`). Every
other colour is light, which a CMYK buffer takes through 8.6.4.4's relation
inverted with maximum undercolour removal — `1 0 0 rg` arrives as
`(0, 255, 255, 0)` — and so is what is not a flat colour (an image, a
shading) and every space whose components are not DeviceCMYK's, ink or not:
an ICC CMYK profile, and a `/Separation` or `/DeviceN` through its alternate.
What is drawn on ink keeps its ink wherever it is drawn: a form on the page; a
coloured tiling pattern's cell (`/PaintType 1`), drawn into a buffer of the
page's own format and composited component for component, so a `k` inside it
arrives as its components and an `rg` as light turned to ink, as on the page;
and a transparency group that composites in ink (no `/CS`, or
`/CS /DeviceCMYK`) — one that names `/CS /DeviceRGB` composites in light
(11.6.6), so its rich black arrives as pure `K`. An uncoloured pattern
(`/PaintType 2`) paints in the colour its `scn` operands gave, which is light
even over a DeviceCMYK base: `[/Pattern /DeviceCMYK]` with `1 1 1 1 /P0 scn`
is pure `K` too
(`a_pattern_or_group_keeps_a_device_cmyk_colour_s_ink_where_it_is_drawn_on_ink`).
`a_page_asked_for_in_ink_with_the_opt_in_comes_back_in_ink` pins the four
inks, and `a_device_cmyk_colour_reaches_an_ink_page_unchanged` holds `k`,
`/DeviceCMYK cs … sc`, a stroke's `K`, a glyph and an RGB fill to
`round(255 × component)` byte for byte. `cs` selecting DeviceCMYK resets the
colour to 8.6.8's `0 0 0 1` — black — and `cs` selecting a `/Separation` or
`/DeviceN` space resets it to the whole colorant, a tint of 1.0 in every
component (8.6.6.4, 8.6.6.5; `cs_resets_a_spot_space_to_full_tint`); a
`/Lab` space starts at zero, or the nearest value a `/Range` that leaves zero
out allows (8.6.5.4). `Bitmap::to_png` writes an ink page as the light
it stands for — PNG has no CMYK — and where every partly covered or blended
pixel is one ink and black those bytes are exactly what the same render
without the switch returns
(`an_ink_page_written_as_png_is_the_light_the_switch_would_have_returned`);
where two inks and black share a partial pixel they are not, because
compositing is linear in whichever components the canvas holds and 8.6.4.4 is
a product: half a pixel of rich black is a quarter of white over ink and half
of it over light (`a_rich_black_edge_composited_over_ink_is_not_the_light_edge`).
**Under a `/DefaultCMYK` the ink stays the document's** (ROADMAP H-135): `k`,
`K`, `sc`/`scn` in a space selected as DeviceCMYK, four components with no
space named, and a stencil painted in one arrive on an ink page as their own
components, byte for byte the page without the default, while their light —
what a light render paints, and what an ink page reads where it needs light,
inside a `/CS /DeviceRGB` group — is the default's
(`an_ink_render_keeps_a_device_cmyk_colour_s_ink_under_default_cmyk`).
A light render composites that ink too wherever it composites in ink, which
is inside a `/DeviceCMYK` group: there `k` under a `/DefaultCMYK` paints what
the page without the default paints, and not its twin naming the profile, so
the light render does not paint the default inside such a group.
`FontSource::resolve_ink` reads the space as selected and
`FontSource::resolve_color` the space as substituted; choosing conversion
instead is one change to the first. It follows that `Bitmap::to_png` writes
such an ink page as 8.6.4.4's light, which is not that page's light render.

**`/Lab` composites in Lab too.** Its components are not in the unit
interval — `L*` runs 0..100 and `a`/`b` roughly −128..127 — so `LabA8`
encodes them into bytes (`L/100`, `(a + 128)/255`, `(b + 128)/255`) and
11.3.5's separable formulas apply to *that*. The encoding is a choice the
clause does not make, and it is stated on the format rather than buried:
blending in the encoded domain is what makes a `/Lab` group composite in Lab
rather than in RGB, and it is not the same as blending the unencoded values.

Every space a group can declare has a buffer. ExtGState `/SMask` works in
both kinds — `/Alpha` and `/Luminosity` (11.6.5.2) — with `/BC` read in the
mask group's own `/Group /CS` and defaulting to black (fully masked, the
default that does not invert every drop shadow), and `/TR` read as written —
so a sampled or a calculator function, a stream, is reached through its
reference — and sampled to 256 entries. An absent `/SMask` key leaves the mask in force where `/None`
removes it (11.6.5.1); `q`/`Q` save and restore the mask beside the clip
(8.5.4). A clip is the path times the clip in force and nothing else — a
mask in force is not multiplied into it, so a group painted under a mask is
masked once and a clip set under a mask stops masking at `/SMask /None`
(`tests/soft_mask_clips.rs`). All 16 standard blend modes apply (11.3.5), the
twelve separable and the four non-separable. Group buffers are a budget, not
just a depth: nesting caps at 16 and the page as a whole at 2 000 buffers,
because a soft mask that opens further groups branches rather than descends
— past the budget the group is declined and reported once. A graphics state
is read once a name in its scope, at the first `gs` that sets it — its
functions and its soft mask's group, whose content is decoded then — and
every later `gs` of the name takes that reading, the colour spaces
it repaired reported again, so a page that sets one masked state before each
of its paths decodes the mask's group once (`Caches::states`, up to 4 096
names a scope).

**Text render modes** (9.3.6). All eight: fill, stroke and clip are decided
independently, so mode 1 strokes rather than fills and modes 4–7 accumulate
glyph outlines into a clip applied at `ET`. A text object that selects a
clipping mode and produces no glyphs clips everything away — spec-correct,
and reported, because it usually means the glyphs could not be resolved.

**A filled glyph is drawn from a mask of its own** (`glyph.rs`,
[design/performance.md](../design/performance.md), P-06). In mode 0, in a
colour, a glyph's em-to-device transform is split into its 2x2, each entry
rounded to the nearest 1/256 of a pixel an em, its origin's phase, the
fraction of a pixel past the whole pixel it falls in rounded to the nearest
sixteenth on each axis, and that whole pixel, in the page's frame. The outline
is filled under the 2x2 and the phase alone into a mask in the glyph's own
frame, and the mask is placed at the whole pixel, windowed to what the
canvas, the clip and the soft mask can show, multiplied by the clip and the
soft mask, restored under in a knockout group and composited by
`fill_mask_inked`, as a path's coverage is. So the same glyph at the same
phase is the same pixels wherever it stands, and a tile, which takes the
phase from the page's frame and subtracts its corner only to index the
mask, draws the page's pixels (ruling 5;
`a_glyph_moved_by_whole_pixels_draws_the_same_mask`). A glyph stands at
most a thirty-second of a pixel from where its outline says on each axis.
Strokes
(modes 1 and 2, whose fill keeps its stroke's position), pattern fills, the
clipping modes and a glyph whose mask would hold more than 65 536 pixels
are drawn as paths in the page's frame, so a glyph far larger than its
canvas is rasterised only where it can show.

**A render keeps its glyph masks** (`glyph_cache.rs`). A mask is a function
of the font, the code, the quantised 2x2 and the phase, one of 256, and is
kept for the rest of the render under those four; a later glyph with the same
four is drawn from it, building no path, and a hit is the mask a miss would
rasterise, because a mask a cancelled render stopped short is drawn and never
kept. The font is the glyph source's answer, `GlyphSource::font_identity`, so
the render crate holds no dictionary: the facade gives a font with an object —
its `/Font` entry's reference, or the object a reserved name spells — as
`FontIdentity::Object`, one font in every scope; a font written into a
resource dictionary as `FontIdentity::Scoped`, keyed beside the renderer's
serial for the scope's turn — the page's, and a new one at each form that
brings a scope and each scope a caller pushes — so two scopes naming two
fonts alike never share a mask; and an id that selects no font as none, whose
masks are not kept. The cache is bounded by bytes, each mask charged its
pixels and 64 bytes beside them, against 16 MiB a render, and evicts the masks
least recently asked for, by a counter every request takes a number of, so
which it evicts is the same on every run and every target. A tiling cell is
drawn by a renderer of its own, with a cache of its own. Path-route glyphs and
Type 3 procedures bypass it.

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
(8.11.2.1), falling back to the resource name. The group an `/OC /name BDC`
names, the `/Properties` table it sits in and every value read out of the
group — `/Name`, `/Type`, and a membership dictionary's `/VE` (each operand
too), `/OCGs` and `/P` — are read where they lie rather than copied at each
`BDC`, which would cost the page's sequences times a long array beside or
under any of them (`tests/property_list_work.rs`).

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
one pipeline and nothing else: the page is drawn through `page_view_transform`
whatever part of it is asked for, onto a canvas that **stands at the region's
corner of the page** (`Canvas::origin`), so the same interpreter, the same
glyphs, the same sampler and a smaller canvas draw the tile, and every
coordinate they compute is the number the whole page computes. A group's
buffer, a soft mask's and a mesh's stand in the same frame, and the few
decisions taken by rectangle rather than by pixel — which cells of a tiling
lattice to composite, which neighbours a mesh's fringe reads, whether a
lattice, a mesh or an image run is within its budget — are taken on
rectangles the renderer keeps in the page's frame (`Bounds`), so a tile takes
the page's decision. **One is not, a known gap:** whether an image *joins*
the run held back so abutting images do not conflate is decided by overlap
with what the run holds over the canvas only, so two images that overlap
outside a tile and abut inside it are one run in the tile and two on the page
(`render_regions.rs`'s
`an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`;
ROADMAP RC-21). A region reaching past the page is **intersected**, never
slid back on, because a moved rectangle returns real pixels from coordinates
the caller did not name; one that misses entirely comes back with no pixels.
Both trims are reported as `RenderWarning::RegionClamped`. Ruling 5's
byte-equality guard and its fixtures are in [rulings](../rulings.md).

**Each stream once a render.** A render is one operation, and every resource
scope it opens — the page's, each form's, a tiling pattern cell's, an
annotation appearance's two — reads every stream through the operation's
decoded streams, keyed by the stream's reference
(`tinker_pdf_cos::DecodedStreams`; the font reader reads a font's CMaps
through the `Cached` view over them): a form drawn three times, a Type 3
glyph's procedure shown fifty times, a tiling cell drawn at every fill, a soft
mask's group two scopes set, an appearance two annotations share, and a font's
program and CMaps the page and its forms both read are each decoded by the
first scope that reads them and handed to every later one behind an `Arc`,
with no filter warning raised again. A chain's refusal is kept as firmly. The
page's own content is read once, by the door, before any scope exists. An
image's samples are not held there, since a scope's image cache keeps the
decoded image. The key is the stream, never a rectangle, so a region decodes
what the page does (ruling 5). The decoded streams are the render's and go
when it returns: a form's kept scope, and a retained page's resources, hold
none, and each render is handed them bound to its own. `Page::render_form`
reads the form it is asked for in the render's operation, so its name's scope
and the render's `Do` share the one decode, and `Page::render_annotation`
decodes the appearance it draws once.

**A retained page.** `Page::display_list` interprets a page once and keeps
what it drew: every call the interpreter made, with the graphics state it saw,
in the space the interpreter runs in — nothing recorded is a pixel — and the
page's resources beside it, so the images, outlines and form scopes a render
resolves are resolved once for every render. `DisplayList::render` takes the
same `RenderOptions` as `Page::render` and returns the same bitmap, because it
is the same pipeline (`render_layer_with`) with the interpretation replaced by
`tinker_pdf_content::replay`. The one thing a transcript cannot carry by
itself is the renderer's answer to the interpreter's three questions — enter
this form, this transparency group, this soft mask — on which the states
recorded afterwards depend; so those answers are `Admission`, one type the
renderer and the recording device (`DisplayRecorder`) both ask, and they
depend on the content stream alone (hidden optional content, how many groups
are open, the group-buffer budget), never on a pixel or a scale. A group or
soft mask whose buffer misses the canvas is accepted over no pixels for the
same reason ([rulings](../rulings.md) 5). Annotations are recorded with the
resource scope each appearance resolves in and replayed when the options ask
for them. **The warnings are the replay's own.** The kept resources are also
where a render writes down what it tolerated, so each replay gets resources of
its own over the list's caches (`PageResources::for_one_render`): a cached
image brings back the damage, or the codec failure, its first decode
reported, and a cached outline the font that resolved no glyph, so a replay
that meets them says what a direct render's own decode would; one that does
not — a region that misses a patterned fill, a render cancelled before it drew
— says nothing about them. What the recording's interpretation could not
resolve is kept beside the events and reported by every replay that runs to
the end; a cancelled one reports none of it, which is what a direct render
cancelled before the first such font reports — less than a later
cancellation might, never more (`display_list.rs`'s
`a_cancelled_replay_reports_no_font_it_never_reached`,
`a_region_that_draws_no_cell_reports_nothing_a_cell_met`,
`every_replay_names_the_images_the_page_draws` and
`every_replay_names_the_glyph_its_font_could_not_resolve`).

**Renders of one list at once fill each cache once.** The five dear caches a
list's renders share — images, font programs, glyph outlines, ICC transforms
and form scopes — are single-flight (`tinker_pdf_cos::Flights`): the first
render on any thread to miss a key computes it while renders of that key on
other threads wait for its value, and a render that is itself in the middle
of a fill computes rather than waits, so no wait can form a cycle and none
deadlocks; a same-thread re-entry for a key computes, as an uncached read
does. A render that meets a value another computed counts a hit and says
again what the fill reported — an image's damage and repairs, an outline's
unresolved font — so a hit is exactly what a miss computes, warnings
included. A fill never reads a render's cancel token. So four renders of one
list decode a shared image once between them (`parallelism.rs`'s
`four_renders_of_one_display_list_decode_a_shared_image_once`) and extract
each glyph once (`four_renders_of_one_display_list_extract_a_shared_glyph_once`);
a font program, read inside an outline's fill, can be read once for each
concurrent extraction that meets another's read, and is bounded by them.

**A page too large to retain is drawn the direct way.** A recording keeps
every call with its own copy of the state and the path, so what it holds is
the interpreter's *work*, and work multiplies through forms: a page that
draws a heavy form a few hundred times records hundreds of times what it
holds, where a direct render holds one canvas. So the recording has a budget,
`tinker_pdf_render::MAX_DISPLAY_LIST_BYTES` (64 MiB), counted by
`kept_bytes` — each event's size, its state, its path at 56 bytes a segment,
and every name, string, inline image and mask-group stream it copies. Past
it `DisplayRecorder` drops what it kept, declines every question and tells
the interpreter to stop, and the list is **not retained**
(`DisplayList::is_retained`): it holds no calls, and every `render` is
`Page::render` and every `to_svg` interprets the page into the writer —
the same output at a direct render's cost, never a partial picture
(`display_list.rs`'s `a_page_too_large_to_retain_is_drawn_the_direct_way`, at
the cap's own value). What the cap bounds is memory. The time a fan of forms
costs is the interpreter's, whose nested-content budget bounds how wide forms
fan as well as how deep they nest ([content and text](content-and-text.md)):
a short file of forms each invoking the next several times stops where that
budget does, in a recording as in a render, and its list is retained
(`the_review_s_fan_out_of_forms_is_bounded_where_it_is_read`).

**A page as SVG.** `Page::to_svg` writes SVG 1.1 through a third `Device`
(`crates/tinker-pdf/src/svg_out.rs`), fed by replaying the page's display list
rather than interpreting the page again, so `DisplayList::to_svg` writes the
same bytes from a list already held. The decisions, each stated in the
module header: coordinates are the page's displayed points (crop box and
`/Rotate` applied, `y` down), with the root sized in `pt` so the picture keeps
the page's physical size; numbers are rounded to four places, never `-0`,
`inf` or `NaN`, and a path segment with a point that is not finite is
dropped as the rasterizer drops it — so a clip whose every point overflows
installs no clip, as on a render, rather than one of no area that hides the
page; every path carries its transform already applied, so only an
image, a gradient and a stroke under a map that is not a similarity carry a
`transform`; fills and strokes carry colour, opacity, the fill rule and the
whole pen — width and dashes scaled by the transform's expansion under a
similarity, as the renderer scales them, and otherwise the path written in
user space scaled by the map's largest stretch, where the pen is round,
under a `transform` that takes it to the page, which is SVG 1.1 §11.4
stroking in user space as 8.4.3.2 does (a zero-width dashed line there is
written as its dashes, cut in user space); caps, joins, miter limit;
a clip is a `<clipPath>` in page space, and a clip inside a clip names its
parent with `clip-path` on the `<clipPath>`, which is §14.3.5's
intersection; a clipped stroke under its pen's `transform` sits inside a
`<g>` that names the clip, because §14.3.5 reads a clip named by the
`<path>` through that path's own `transform`
(`a_clipped_stroke_under_its_pen_transform_is_clipped_in_page_space`); an
image is a PNG `data:` URI of its decoded samples — a
stencil in the fill colour, a soft mask as its alpha — on the unit square,
inside a `<g>` that names its clip because a clip named by the `<image>`
would be read through the image's own transform; **a picture drawn again is
not embedded again** — an image's PNG, or a rasterised paint's, of 512 bytes
or more is written once into `<defs>` and every draw of the same bytes is a
`<use>` carrying that draw's transform and opacity, up to the 4 096
expansions `tinker-pdf-svg` allows, past which pictures are written in place
so the reader never refuses a file for its references; a transparency group is a
`<g>` with the group's alpha as `opacity`. **The markup has a budget**,
`MAX_SVG_BYTES` (256 MiB) of elements or the smaller `SvgOptions::max_bytes`:
markup grows with what a page *does* — every operator an element, every image
`Do` a picture — so a short stream asks for as much as it likes, and an element
that would pass the budget is not written, nothing after it is, the replay is
told to stop and `SvgWarning::Truncated { limit }` says so; the document is
well-formed and ends there. A zero-width dashed line under a stretch, written
as its pieces, stops cutting them once its path data passes what the budget
has left, so what it holds is the budget and not the pieces
(`svg_dash_memory.rs`). **Text is written as paths**, one
per glyph, from the outline the renderer draws: SVG 1.1 carries a font only
as `@font-face` or `<font>`, and the reader this writer is held to reads
neither, so glyph-positioned `<text>` would look like the page only where the
viewer happened to have the face. The trade, named: the SVG's text is not
text — not selectable, not searchable. **An axial or radial shading is
a gradient where that is exact** — DeviceRGB or DeviceGray, a piecewise
linear function whose values stay inside 0 to 1, both ends extended, and for
a radial one a first circle that is a point strictly inside the second, which
is the only radial shape §13.2.3 and 8.7.4.5.4 draw alike — with a stop at
every breakpoint and two at a discontinuity. Everything else — a mesh, a
function-based shading, a CMYK ramp, a tiling pattern, a patterned stroke —
is **rasterised** through the renderer at `SvgOptions::raster_scale`, clipped
as the page clips it, and embedded as an image, and each is named
(`SvgWarning::Rasterised`). The writer never emits a `<mask>`, a `<pattern>`
or a `<filter>`; what would need one is the writer's own named refusal
(below). What a render of the same page reports —
an undecodable image (drawn as the renderer's grey placeholder), a shading or
pattern this build does not paint, an unreadable font, a hidden layer, an
empty text clip, a damaged image, and anything the renderer said while
drawing a rasterised paint — comes through in the renderer's words as
`SvgWarning::Render(RenderWarning)`.

One place the reader is short of the file, pinned by a test:
`tinker-pdf-svg` follows one `clip-path` per element, so it reads the inner
clip of a nested pair and not their intersection. The file says it
correctly. A clipped image reads back clipped, in page space, and
`a_clipped_image_is_clipped_in_page_space` asserts the clip as well as the
placement.

**Anti-aliasing off.** `RenderOptions::antialias` is on by default; off, every
pixel of every shape is wholly covered or not covered at all. It is one
threshold — `Mask::harden`, half a pixel's coverage — applied wherever a
coverage value is produced, and there are five such places rather than one:
`Renderer::coverage`, through which every fill, stroke, clip, text clip,
pattern shape and glyph drawn as a path passes; a filled glyph's own mask,
before the clip multiplies into it; an image's unit square, hardened inside the
rasterizer through `ImageDraw::edge_bits` at 0; a mesh shading's silhouette after
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

**A stroke under a transform that is not a similarity.** 8.4.3.2 measures
the line width in user space — a stroke paints every point within half the
width of the path *in user space* — so under an anisotropic scale or a
shear the stroke a device shows is wider in one direction than another, and
8.4.3.6's dashes are measured, and cut square, in the same space. Under a
similarity — a uniform scale with any rotation or reflection, which is
nearly every page — that is one device width, the user width times
`Matrix::expansion`, and `stroke_path` strokes in device space at it. Under
any other map (`user_space_pen` decides, from the map's two singular values,
and the SVG writer asks the same function) it takes the path back to user
space, strokes it there with the width, caps, joins, miter limit and dashes
the content stream gave, and carries the outline out
(`tinker_pdf_raster::stroke_mapped_with`); the thinnest-line floor stays in
device pixels in every direction. Either way the outline is written straight
into the renderer's fill scratch as edges and never built as a path, so the
path is flattened once, by the stroker, and the outline not at all; its mask
is the one filling the outline's path would give, byte for byte
([rasterizer](rasterizer.md)). A glyph stroked by text rendering modes
1, 2, 5 and 6 goes the same way, with the CTM's user space and not the text
matrix's. `stroke_parameters.rs`'s
`a_circle_under_scale_1_3_is_six_wide_at_its_top_and_two_at_its_side` and
`a_dash_on_a_sheared_line_is_sheared` hold the clause's arithmetic in
pixels, and `the_current_transform_scales_the_line_width` asserts that only
a `y` scale thickens a horizontal line. A map that stretches one direction
more than 10⁹ times another is stroked at its expansion — going back to user
space through it would cost the path its position.

**A page as a layer, and premultiplied alpha.** `RenderOptions::transparent`
starts the page with nothing on it instead of white — `(0, 0, 0, 0)` where
nothing paints, a half-opaque fill at half alpha, an anti-aliased edge at the
alpha its coverage gave it — for a format with alpha to hold it in (`GrayA8`,
`Rgba8`, and ink with `allow_cmyk`); a format without alpha is on white
whatever it is asked, because a transparent canvas converted to `Rgb8` would
keep the black stored under an alpha of zero. The page composites exactly as it
does over white, against a backdrop alpha of zero, which is 11.3.6's formula and
what every transparency group's buffer already starts from.
`RenderOptions::premultiplied` hands the colour back multiplied by alpha, and
`Bitmap::premultiplied` says which a bitmap is. The canvas stays straight
throughout — every blend formula in 11.3 is written in straight alpha — so it
is a conversion at the very end, `round(c·a/255)` with no tie possible because
255 is odd, and on an opaque page it changes no byte. `Bitmap::to_png` divides
it back out, because PNG's alpha is straight: exact at full alpha, and below it
losing only what premultiplying lost, so multiplying the file's samples by their
alpha again returns the premultiplied bytes exactly, which
`a_premultiplied_page_writes_a_png_that_multiplies_back_exactly` holds.

**One part of a page.** `Page::render_form` draws one form XObject the page
names in its `/XObject` dictionary, and `Page::render_annotation` one entry of
its `/Annots`, each on its own over the page's white. Neither is a second
renderer: `Page::render` is a paint over one private pipeline,
`Page::render_layer`, and the two parts are two more paints over it — the same
scale clamp, view transform, canvas, `Renderer`, warnings and conversion at the
end — differing only in what is painted and in the default viewport. A form is
painted by interpreting `/Name Do` at the identity, so the interpreter reaches
it exactly as the page's content does (`/Matrix`, the `/BBox` clip, a group,
`/OC`, its own resources), and its viewport is its box on the page: `/BBox`
through `/Matrix` and the page's view transform, rounded outward and trimmed to
the page. An annotation is painted by the code the page draws every annotation
with (`annots.rs`, split into `prepare` and `draw_prepared` so that one
piece of code does both), and its viewport is its `/Rect`. `options.region`, if
set, wins over either. A form is named by its resource name — the only name
that means "as this page places it"; a form reachable only inside another
form's resources has no placement to render at — and an annotation by its index
in `/Annots`, which is its index in `Page::annotations()`, because
`Annotation::reference` is `None` for a direct dictionary. **Each is byte-equal
to its rectangle of a page that draws it there and nothing else**, against the
page rendered with that rectangle as its region and against the whole page
cropped, at 1x, 1.5x and 2x (`render_parts.rs`). Where a page skips an
annotation silently, a caller who asked for that one is told why:
`RenderPartError::AnnotationNotDrawn` with a `NotDrawn` reason.

## API

The facade is the whole public surface (ruling 11): `Page::render` takes a
`RenderOptions` — `scale` (pixels per point, or `RenderOptions::at_dpi`),
`format` (`PixelFormat`), `cancel` (an optional `CancelToken`, cloneable and
checked between operations and scanline bands), `annotations` (on by
default), `region` (an optional `PixelRegion`, `None` for the whole page) and
`allow_cmyk` (off; with `format: CmykA8`, hands the page back as ink rather than
light), `antialias` (on; off makes every pixel of every shape whole or
empty), `transparent` (off; on starts a page whose format has alpha with
nothing painted rather than white), `premultiplied` (off), `stats` (off;
on counts the render's work, below) and `parallelism` (`Parallelism::Serial`;
the threads a page walk spends, below) — and returns a `Bitmap`: `width`,
`height`, `format`, `stride`, `data`, `warnings`, the `Vec<RenderWarning>`
that carries every named degradation, `premultiplied`, which says whether
`data` is, and `stats`, what the render counted when it was asked to.
Rendering never fails; it degrades and reports.

**Counted work.** With `RenderOptions::stats` on, a render counts the steps it
takes and hands them back as `Bitmap::stats`, an `Option<Stats>` that is
`None` whenever it was not asked — and for a bitmap `Bitmap::from_png` reads
or a caller builds. `Stats` (`Copy`, `Default`, `Eq`, `#[non_exhaustive]`,
every field a public `u64`) names them: `glyphs_shown`, every glyph the
interpreter showed the renderer, `outlines_filled`, every glyph outline it
built into a path and painted — on the path route, and for a mask its cache
did not hold — `glyph_masks_rasterised` and `glyph_mask_hits`, each filled
glyph drawn from a mask rasterised for it and each drawn from a mask the
render's cache held, and `shading_functions_evaluated`, every evaluation of a
shading's function for a colour — once a table entry where a sweep reads the
function through a `ColorTable`, once a colour asked of it otherwise — all
counted by the renderer, a tiling pattern's cell included;
`font_dictionaries_resolved`, each font a resource scope reads — once for
each name its content selects a font by, the first time the scope is asked
about the name, and kept for every render over the scope: a font a scope
names and never selects is not read, a glyph asks the font its scope read
whether it is a Type 3 font, and a substitute is asked for from the
dictionary read;
`images_decoded`, an image XObject's decode on a cache miss and every inline
image's; and a `_hits` and a `_misses` for each of the five caches a page's
resources keep — `font_program`, `image`, `icc_transform`, `glyph_outline` and
`form_scope`; `page_walks`, each walk of the page tree from its root,
`page_nodes_visited`, each node a walk or a one-page lookup reads, and
`page_entries_copied`, each page a lookup or a read of every page copies out
of the page index a count built ([document model](document-model.md)), which
no render makes and a page lookup, a count, the validators, an editor and a
save do; `keys_looked_up`, each lookup by its bytes of a name the document's
name table pre-interns — a key read where its `Name` constant answers without
the table, which no reader on a counted path does, or a resource name that
spells one, such as a font named `/Font` — counted by the resource scopes and
by a font read through them; `color_spaces_parsed`, each colour space a
resource scope reads out of `/ColorSpace` by the name its content selects it
by, both its readings at once, the first time it is asked about the name;
`functions_read`, each function a scope reads from an object of its own — a
shading's, a tint transform, a stitching function's piece, a graphics
state's — once an object, a stitching function once for each depth it is read
at; `graphics_states_read`, each graphics state a scope reads out of
`/ExtGState`, at the first `gs` of its name; `pages_interpreted`, each page a
reading-order inference interprets, which no render does
([content and text](content-and-text.md)); `tokens_lexed`,
`xref_entries_read`, `objects_parsed` and `bytes_scanned`, the COS parser's
steps — each token its lexer produced, each cross-reference row read, each
indirect object parsed and each byte the repair scan's searches looked
through — which only `CosDocument::open_counted` and
`CosDocument::get_counted` count ([opening](opening.md));
`content_tokens_read` and `operators_run`, each token the content interpreter
took and each operator it ran, nested content included, which only
`tinker_pdf_content::interpret_counted` counts
([content and text](content-and-text.md)); `text_forms_built`, each page's
text a search folded in one of its four folds, `word_boundaries_computed`,
each such form's word boundaries computed for a whole-word search, and
`search_steps`, each step of a search's matcher and of its mapping back to
glyphs, which only `TextPage::search_counted` counts
([content and text](content-and-text.md)); and
`streams_decoded`, each run of a stream's filter chain — the page's content;
once a render, each stream its scopes read: a form's, a Type 3 glyph's
procedure, a tiling pattern's cell, an appearance, a font's program and its
CMaps, an ICC profile, a function's; and an image's samples and its soft
mask's at each decode the image cache makes —
with `most_decodes_of_one_stream`, the most runs any one stream had,
`most_decoded_stream`, the reference of the stream that had them (the lowest
of several that tie), and `bytes_decoded`, the bytes the runs returned
([filters](filters.md)); `jpeg_held`, the most bytes a JPEG decode held
at once beside the image it decoded into — its coefficients, its planes and
its strip; `jpeg_samples_transformed`, the samples the JPEG decodes' inverse
transforms output, `N × N` for each block a decode at `N/8` transformed; from
`Bitmap::to_png_counted`, `png_image_scratch` and
`png_peak`, maxima of the bytes a PNG write held; `stream_seal_passes`, each
pass of the AES core that sealed a save's streams side by side, which only a
save that encrypts counts ([writing](writing.md)); and
`pool_items_on_workers` and `pool_items_on_caller`, where a page of a page walk
ran — on one of the pool's workers or on the caller — counted into the first
counted operation the page makes, its render or its counted text, and never
into a page rendered outside a walk; and where each band of a page drawn in
bands ran, counted into that render's one tally. `Stats::work()` totals the step counts,
which leaves out those maxima, those quantities and the two pool counts — where
a part ran is no step, so a walked page totals what the page rendered alone
does — and two `Stats` add with `+`, a maximum keeping the larger. Every
render door counts the same way: `Page::render`, `Page::render_form` (the
scope it reads the form's name in included), `Page::render_annotation` (both
of the appearance's scopes) and `DisplayList::render`, which counts through
the resources the list keeps, so a second replay finds what the first decoded
and says so. Each render counts into a tally of its own, so two at once — of
one page, or of one display list from two threads — never mix. A count is a
property of the page, the options and the build — a region that misses a
patterned fill draws no cell for it — and never of the machine or the clock.
Nothing reads the tally but the door that hands it back, so a counted render's
pixels and warnings are the uncounted one's; and an uncounted render carries
no tally at all, so it pays one untaken branch a counting site, and the
renderer's two additions a glyph, which it makes either way. The design is in
[performance](../design/performance.md); the C ABI and the bindings do not
carry the stats ([bindings](bindings.md)).

**Page walks.** `Document::render_pages(pages, &options, each)` renders each
of `pages` as `options` asks and hands `each` the page's index and its bitmap,
`None` for a page the tree will not hand back, in the order `pages` names
them, on the calling thread. With `RenderOptions::parallelism` at its default,
`Parallelism::Serial`, every page renders on the caller, in order; with
`Threads(n)` the pages render on at most `n` threads of the engine's own pool
(`tinker-pdf-pool`), started in a scope for the call and joined before it
returns, never more than the pages and never more than 256. Each page is one
operation, rendered on its thread as `Page::render` renders it, serially
whatever the options say, so each bitmap is the serial page byte for byte,
its warnings included; the walk shares the one `Document` by reference and
takes each page through `Document::page`. Worker `k` renders the `k`th page of
each wave of `n`, and each wave is handed to `each` before the next starts, so
at most `n` bitmaps are alive at once. A worker that will not start — every
worker on wasm32, a host at its thread limit — leaves its pages to the caller,
with the same output. `Document::walk_pages(pages, parallelism, work, each)`
is the walk both page walks are, for a caller whose page is more than one
door: `work` runs for each page on its thread and `each` takes what it
returned, in page order; called on one of the pool's workers, a walk runs on
that worker. The design is [parallelism](../design/parallelism.md).

**A page in bands.** `DisplayList::render` with `RenderOptions::parallelism`
at `Threads(n)`, `n` two or more, cuts the rows it draws — the page, or the
region asked for — into at most `n` horizontal bands at rows of the page
frame, draws each as a region of the page (`RenderOptions::region`) on the
engine's pool over the list's shared caches, and writes each band's rows into
one bitmap sized to the view; `Page::render` with such options records the
page's list for the call and draws it so. Each band is the page's own region
in its one frame (ruling 5), so each is byte-equal to its rows of the page
drawn whole. The one decision ruling 5 leaves to the canvas — whether an
image draw joins the run of images held back — is kept out of the cuts: the
planner reads the list for each sequence of two image draws or more with
nothing the renderer ends a run on between them
(`tinker_pdf_render::ends_image_run`, which
`the_planners_rule_is_the_renderers_flush` holds to the renderer one kind of
call at a time), in the content, its forms, groups and soft masks and the
annotations drawn after it, takes the rows of the page its draws cover, and
moves each cut from `round(k × height ÷ n)` to the nearest row no such
sequence has rows on both sides of, between the cut before it and the next
cut's own row, dropping a cut that has none — a page whose run of images
covers every row, a scan of strips, draws as one band. The warnings are the
page's: every band replays the same calls, so each band stamps what it met
with the ordinal of the call it met it at, and the merge keeps each warning
once at the least ordinal any band met it at, in that order, then what the
renderers add at the end in their fixed order, then each damaged image and
repaired space likewise, then the page's own scale and region. A list that is
not retained, a view of fewer than two rows, or one thread draws the page
whole. Where no thread starts — wasm32, a host at its thread limit — the bands
are cut and stitched on the caller, with the same output. Each band counts
into the render's one tally, where it ran included
(`parallelism.rs`'s `a_four_band_render_runs_no_band_on_the_caller`); a
render through `Page::render` counts the bands' replay, not the recording.

`Page::display_list()` returns a `DisplayList` — the page interpreted once —
whose `render(&RenderOptions)` returns what `Page::render` returns for the
same options, at any scale and for any region, without interpreting the page
again; `len`, `is_empty` and `page_index` say what it holds, and
`is_retained` whether it holds the page's calls at all — false for a page
whose recording would pass `tinker_pdf_render::MAX_DISPLAY_LIST_BYTES`, whose
every render is then a direct one. It owns what it
needs (the page, its recorded calls and its resources), so it outlives the
`Page` it came from and crosses threads.

`Page::to_svg(&SvgOptions)` and `DisplayList::to_svg(&SvgOptions)` return an
`Svg`: `markup` (the document, UTF-8), `width` and `height` (the page's
displayed size in points, which are the root's `width`, `height` and
`viewBox`), and `warnings`, a `Vec<SvgWarning>` deduplicated per page.
`SvgOptions` (`#[non_exhaustive]`, `Default`) has `annotations` (on, as on a
render), `raster_scale` (pixels per point for what is rasterised, 2 by
default, clamped to 0.25–16; a value that is not a finite positive number is
read as the default) and `max_bytes` (the markup's budget in bytes of
elements, `tinker_pdf::MAX_SVG_BYTES` by default; a larger value is read as
the cap, so it lowers the ceiling and cannot raise it). `SvgWarning` is
`Rasterised { what: Rasterised, name }` with
`Rasterised::{Shading, TilingPattern, PatternedStroke}` and the shading's or
pattern's resource name, `SoftMaskRefused { group }` with the mask group's
`ObjRef`, `BlendModeRefused { mode }`, `KnockoutRefused { form }` with the
group form's resource name, `Truncated { limit }` and
`Render(RenderWarning)` — each naming what it touched (ruling 10), except
the blend mode, whose `ExtGState` no device is told: the interpreter hands
every device the state a `gs` made, never the `gs`.

The parameters a `gs` stores rather than applies — black-point
compensation, overprint, flatness, smoothness, stroke adjustment, alpha
source, text knockout, the black generation, undercolour removal and
transfer functions, and the halftone and its origin — travel in the state as
one shared `StoredParameters` value with the rendering intent, which flat
colours apply, `None` while every one has its initial value, so a
`q` copies one word more and a copy allocates nothing. The display list's
budget counts that value once per change rather than once per event: the
recorder remembers the value it last counted and counts again when an event
holds another. A function is kept parsed, never sampled — sampling is the
applying row's, at its stage and depth — and `/Default` stores the device's
own, `None`. Each scope reads a function it names by reference once, however
many states share it. A halftone is kept as its type and parameters — a type
1's frequency, angle, spot function (a function, or its predefined names as
written) and accuracy, a type 5's halftone per colorant, a threshold
halftone's dimensions — with each threshold array kept by its stream's
reference, decoded only where a render screens with it; a halftone of the
wrong shape is absent, never a refusal of the page.
The output is the same bytes every time and on every target that runs the
fingerprints: `determinism.rs`'s `svg_output_is_stable_across_targets` writes
every page the render fingerprints cover with the default `SvgOptions` and
holds the SHA-256 of its markup to a committed value, on Linux, Windows and
`wasm32-wasip1` ([determinism](determinism.md)).

`Page::render_form(name, options)` and `Page::render_annotation(index,
options)` return `Result<Bitmap, RenderPartError>`: one part of the page,
through the same pipeline, over the part's own rectangle unless
`options.region` names another. The error is the only way either draws
nothing — a caller who asked for one part is owed the reason rather than a
white rectangle.

`Bitmap::to_png` writes the page out as a PNG file (ISO/IEC 15948), eight bits
a component, through `tinker_pdf_filters::png_encode` — where the zlib
stream, the chunk CRC-32 and 9.2's row filters a PNG is made of live — its
rows streamed through a zlib stream at the fast level into the file, so
writing holds no buffer the size of the image. `Bitmap::to_png_counted` is
the counted door: the file, and a `Stats` whose `png_image_scratch` is the
most bytes the write held at once in buffers sized from the image — none for
the four formats PNG has, the straightened copy of a premultiplied bitmap, the
RGBA copy of a `CmykA8` or `LabA8` one — and whose `png_peak` is the most held
at once with the file (guard A8, [performance](../design/performance.md)). It is
**total over all six `PixelFormat`s**, which matters because a
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
asymmetry: the largest page this engine renders writes a PNG this will not
read back, because a reader's budget against a thirteen-byte header asking for
2^63 samples is not a writer's.

**The bitmap toolbox** works on a rendered `Bitmap` in place, every closed
form the one [bitmap toolbox](../design/bitmap-toolbox.md) states and every
byte of its arithmetic in `tinker_pdf_raster::toolbox`, which `cargo xtask
libm` binds. `Bitmap::gamma` takes every tone component through a table of
`floor(255 * pow(v / 255, gamma) + 0.5)` built through `tinker_pdf_math::pow`
— an ink as the light it takes away, a Lab bitmap's `L*` alone — and ignores
an exponent that is not finite and positive; `invert` complements every colour
byte; `invert_luminance` turns each pixel's lightness `Y` (`Color::luma`) over
and keeps its chroma, `c + 255 - 2Y` clamped, a Lab bitmap's `L*` alone, an
ink bitmap through light and back; `tint(dark, light)` places each pixel's
lightness on the ramp between two `Color`s, each channel
`(dark * (255 - Y) + light * Y + 127) / 255`, written in the bitmap's own
format as a canvas of it is filled; and `grayscale` makes each pixel its own
lightness — RGB `(Y, Y, Y)`, ink black ink alone, Lab its `L*` with neutral
`a*` and `b*`. Alpha is never mapped, and a premultiplied bitmap is mapped
straight, each pixel straightened and premultiplied again. Three reductions
answer about the whole bitmap, reading the bytes as stored:
`colour_count(limit)`, the distinct pixels counted to `limit` and no further;
`is_unicolour`; and `is_monochrome`, every pixel opaque black or opaque white
by the canvas's reading of its format. Each walks the rows over `stride` and
touches none of a row's padding, and on a bitmap that is not a picture — `to_png`'s
condition — a map changes nothing and a reduction answers `None`.

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
helpers `Page::render` composes, with `region_canvas_in` and
`region_canvas_clear` the two that take a `PixelRegion` — a canvas standing at
the region's corner — and `Renderer::with_page_size` telling the renderer how
large the page around it is. `None` is not a special case in the facade: it
becomes the region covering the whole page, whose corner is the origin, so a
tile and a page take one code path and there is no un-tiled spelling for a
defect to hide in.

## Refused by name

| What | Typed variant | Why (one line) | See |
|---|---|---|---|
| An image codec not built in | `RenderWarning::UnsupportedImage` | A neutral placeholder is drawn and the codec named | [filters](filters.md) |
| An image decoded with damage tolerated | `RenderWarning::DamagedImage` | Drawn *and* named, so a damaged scan is distinguishable from a blank one (ruling 10) | [rulings](../rulings.md) |
| A page above 67.1 Mpx at the requested scale | `RenderWarning::PageScaledDown` | A whole page smaller beats a fragment at full size (ruling 2) | [rulings](../rulings.md) |
| A font program that could not be read | `RenderWarning::UnreadableFont` | Its glyphs are not drawn; the rest of the page is | [content and text](content-and-text.md) |
| A shading that cannot be read into types 1–7 | `RenderWarning::UnsupportedShading` | Reported by type number rather than guessed at | [rulings](../rulings.md) |
| A pattern whose cell cannot be read, or a lattice past 65 536 positions, a cell past 16.7 Mpx, or a fill past 33.5 million square points of compositing, the same at every scale | `RenderWarning::UnsupportedPattern` | Unpainted reads as missing; `/Pattern`'s nominal black reads as content and hides the gap | [rulings](../rulings.md) |
| A colour space whose parameters no space can mean, read the nearest way that does: a `/Lab` `/Range` pair written backwards, read as the span it covers; and a default colour space 8.6.5.6 does not allow — a `/Lab`, `/Indexed` or `/Pattern` one, one with another number of components than its device space, one that is no colour space — refused, the device space drawn as without it; and a JPEG's or JPEG 2000 image's `/ColorSpace` with another number of components than its samples, refused, the image drawn in the codec's own colour | `RenderWarning::RepairedColorSpace { name, reason }`, `reason` `LabRangeUnordered`; for a default, `name` the default's own (`DefaultRGB`) and `reason` `DefaultSpaceFamily`, `DefaultSpaceComponents` or `DefaultSpaceUnreadable`; for such an image, `name` the image's (`inline` for an inline one) and `reason` `ImageSpaceComponents`; once a render | Drawn *and* named (ruling 10). Named where the space is reached through the page's resources or a tiling cell's, as every tolerance the resource layer reports is; **not** through a form XObject's or an annotation appearance's own `/Resources`, which keep what they tolerate to themselves — a font a form's own resources cannot resolve goes unreported the same way | [rulings](../rulings.md) |
| A layer the default configuration turns off | `RenderWarning::HiddenOptionalContent` | Correct behaviour, reported all the same — the one leniency a reader cannot see | [rulings](../rulings.md) |
| A form, a soft mask's group or a Type 3 glyph procedure that names itself while it runs, directly or through the others, drawn once | `RenderWarning::RecursiveContent`, and `SvgWarning::Render` of it from `to_svg` | A cycle has no last run; refused at its first repeat by the stream's identity, the page is what every reader can draw, where the depth cap ended a branching cycle only after two or more runs to the sixteenth | [content and text](content-and-text.md) |
| Nested runs past the interpreter's budget for nested content — 4 MiB, and 4 096 bytes for every byte of content the page reaches — past which no stream that has already run is run again | `RenderWarning::NestedBudgetSpent`, and `SvgWarning::Render` of it from `to_svg` | A tree of distinct forms or glyph procedures, each drawing the next many times, is no cycle and no group; every stream the page holds still runs once, so what is passed over is repeats of content already drawn | [content and text](content-and-text.md) |
| More than 2 000 transparency-group buffers on one page, or more than 2 000 groups and soft masks' groups run in one stream, past which the interpreter runs no more of either | `RenderWarning::GroupBudgetSpent`, and `SvgWarning::Render` of it from `to_svg` | A budget, not a depth: branching group and soft-mask recursion stays inside any depth cap, and every reading of the page — text, images, SVG — is bounded by the same walk | [rulings](../rulings.md) |
| A text object that clips and shows no glyphs | `RenderWarning::EmptyTextClip` | Spec-correct and almost never intended | [content and text](content-and-text.md) |
| A render stopped by its `CancelToken` | `RenderWarning::Cancelled` | Reported only when work was actually skipped | — |
| A `RenderOptions::region` reaching past the page edge | `RenderWarning::RegionClamped` | The part on the page is rendered rather than refused (ruling 2), and a bitmap smaller than the rectangle asked for is named rather than left to arithmetic (ruling 10). A region that misses the page entirely trims to no pixels | [rulings](../rulings.md) |
| A PNG read back whose raster stops short of its declared height | `PngReadError::Incomplete`, carrying the decoder's own identifiers | The decoder degrades for a comic page; a file read back to be *compared* would have its missing rows scored as a rendering difference. Every refusal the decoder makes is `PngReadError::Refused` with its own reason | [filters](filters.md) |
| A form render naming an XObject the page does not have, one that is not a form, or one whose stream cannot be read | `RenderPartError::NoSuchXObject`, `NotAForm { subtype }`, `UnreadableForm` | The page renders what it can; a caller who asked for one form asked about that form, and a blank bitmap is a wrong answer that looks right | — |
| An annotation render at an index past `/Annots`, or of an entry that draws nothing | `RenderPartError::NoSuchAnnotation { count }`, `AnnotationNotDrawn { why }` with `NotDrawn::{NotADictionary, Hidden, Popup, NoRect, NoAppearance, UnreadableAppearance, Degenerate}` | Every reason `Page::render` skips an annotation silently, named where a caller asked for that one | [document model](document-model.md) |
| A soft mask, a blend mode other than `Normal`, or a knockout group, on a page written as SVG | `SvgWarning::SoftMaskRefused { group }`, `BlendModeRefused { mode }`, `KnockoutRefused { form }` | SVG 1.1 says the first two only with `<mask>` and `<filter>`'s `feBlend`, and the third not at all; the writer emits neither element — `tinker-pdf-svg` refuses `<filter>` — so what was masked is drawn unmasked, the blend as `Normal`, the group as an ordinary one | [design/svg.md](../design/svg.md) |
| A shading no SVG gradient states exactly, a tiling pattern or a patterned stroke, on a page written as SVG | `SvgWarning::Rasterised { what, name }` | Drawn through the renderer at `SvgOptions::raster_scale` and embedded as pixels: a fallback rather than a refusal, named because the file is not vectors there. A `<pattern>` would be exact for a tiling pattern; the writer does not emit one | — |
| A page whose recording would hold more than `MAX_DISPLAY_LIST_BYTES` (64 MiB) | `DisplayList::is_retained()` is false | Not a warning, because nothing is drawn differently: the list keeps no calls and every render and SVG is a direct one, the same output at a direct render's cost. A fan of forms makes a short file record without bound, which a direct render's one canvas never does | `bounds_ledger.rs` |
| A page whose SVG would pass `MAX_SVG_BYTES` (256 MiB) of elements, or the smaller `SvgOptions::max_bytes` | `SvgWarning::Truncated { limit }` | Markup grows with what the page does rather than with what the file holds, so it has a budget a render's one canvas does not need; what fits is written, the document is well-formed and ends there, and the replay stops | `bounds_ledger.rs` |
| An ICC profile whose data space and tags contradict each other, carrying no `A2B*` table that reads | `ColorSpace::Approximated`, stated on the type | `icc_census.rs` names all three shapes. Not a capability gap: a matrix over Lab components, a data space no registry defines, and one tone curve for four channels of ink. A table beside such a model is what the profile converts through; without one, the fallback is 8.6.5.5's alternate-space reading | [ROADMAP](../ROADMAP.md) |

## Verified

- Facade integration tests, one file per capability:
  `crates/tinker-pdf/tests/colour_spaces.rs`, `default_spaces.rs`, `blend_modes.rs`,
  `transparency_groups.rs`, `optional_content.rs`, `mesh_shadings.rs`,
  `text_render_modes.rs`, `images.rs`, `inline_images.rs`,
  `stroke_parameters.rs`, `form_xobjects.rs`, `page_geometry.rs`,
  `annotation_appearances.rs` — each asserting pixels, not absence of error.
- Page walks: `crates/tinker-pdf/tests/guards.rs`'s
  `a9_page_walks_are_the_serial_walk_at_2_3_4_and_7_threads` holds every page
  of a text, an image and an annotated document walked on 2, 3, 4 and 7
  threads to its serial render — pixels and warnings — and its serial text;
  `crates/tinker-pdf/tests/parallelism.rs` reads where each page ran from its
  stats (`a_page_walk_on_four_threads_runs_no_page_on_the_caller`), proves
  four workers are four threads at a barrier, and holds a walk to the order
  its pages were named in, one wave alive at once, a walk inside a walk on its
  worker and a cancelled walk to the pages' own renders.
- A page in bands: `guards.rs`'s
  `a9_band_renders_are_the_serial_page_at_2_3_4_and_7_threads` holds every page
  `render_regions.rs` tiles (`tests/region_support/`) — the three-image page of
  ruling 5's named exception among them, whose run covers every row — every
  page of A9's documents, a paths page, a full-page JPEG and a page whose
  bands meet its warnings in the other order from the page's, drawn through
  `DisplayList::render` on 2, 3, 4 and 7 threads and through `Page::render`,
  at 0.5x to 4x, to the page drawn whole: pixels and warnings; `display.rs`'s
  band tests hold the planner's cuts, its runs and the merge.
- Counted work: `crates/tinker-pdf/tests/stats.rs`.
  `render_stats_are_none_unless_asked` over the page, a tile, a display list,
  a form and an annotation; `render_stats_count_what_a_generated_page_draws`,
  fourteen glyphs in three fonts and one image drawn twice reporting fourteen
  glyphs shown, the font resolutions the code makes, one decode and one
  cache hit, with every counter derived from the fixture in the test and the
  bundled build's substitute requests counted where it makes them; one test
  each for a form shown three times over its one kept scope, `render_form`'s
  scope of its own, an ICC profile two images share, inline images, a tiling
  pattern's cell, an annotation's two scopes, a display list's replays of its
  page and of its annotations, and two renders at once, of the page and of its
  display list, each counting only its own; and `stats_do_not_change_a_pixel`,
  the page, every tile of a lattice over it, two replays, the form and the
  annotation with stats on and off, byte for byte and warning for warning at
  two scales. Each runs in the default and the `bundled-fonts` builds.
- What a scope reads once: `crates/tinker-pdf/tests/resource_work.rs`'s
  `a_scope_parses_each_named_space_and_state_once`, a page of a thousand
  `cs`, `scn`, `sh` and `gs` operators whose render reads each colour space
  it names, each function object its spaces, shadings and states hold, and
  each graphics state once — at most the distinct names and objects it uses,
  and what a page of forty-eight of them reads; and
  `a_space_found_kept_says_what_reading_it_said`, two replays of a display
  list whose pattern cell selects a repaired space, the second finding it
  kept and naming the repair as the first, which read it, does.
- Parts: `crates/tinker-pdf/tests/render_parts.rs` holds a form — one with
  text, a fill cut by its `/BBox` and a curve, one with a `/Matrix` and a name
  that needs escaping — and an annotation drawn at its own `/Rect` and one
  fitted onto another size, each byte-equal to the page's own render of its
  rectangle at three scales; every refusal by name; and a fixed-seed campaign
  of mutated documents that must never panic either entry point.
- Output options: `crates/tinker-pdf/tests/render_options.rs` pins each
  `RenderOptions` field that changes what a page's bytes are with its own
  SHA-256, computed as `determinism.rs` computes one and floored by ink the same
  way, beside the claim that the default render is unchanged — the blend grid's
  default render hashes to `determinism.rs`'s `analytic_blend` value.
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
- The bitmap toolbox: `crates/tinker-pdf/tests/bitmap_toolbox.rs` holds every
  map and every reduction to its closed form over every reviewed raster
  golden, read through `golden_support`'s PPM reader, in each of the six
  formats, with an alpha plane the test lays on, straight and premultiplied,
  computing each byte from the design's formula and the canvas's colour
  relations; gamma at exponents of a half, one and two, whose powers it
  computes exactly in integers. It holds a padded stride unread and unwritten
  and a bitmap that is not a picture refused whole.
  `tinker-pdf-raster`'s `toolbox` tests hold gamma's table to its formula at
  other exponents and the toolbox's decoding and encoding of each format to
  the canvas's own.
- SVG output: `crates/tinker-pdf/tests/svg_output.rs` reads every file the
  writer makes back through `tinker-pdf-svg` and compares the scene with what
  the page states, worked out from the content stream's own numbers — a
  rectangle's corners, every control point of every glyph from `glyf` and
  the text matrix, an image's samples byte for byte and its corners, a
  gradient's axis, focus and stops, a dashed stroke's whole pen, and a
  stroke under a non-uniform scale whose width is also counted in the
  renderer's rows of ink — never with a second rendering. One noisy image
  drawn four hundred times is one copy and four hundred references, each read
  back where its `cm` puts it; a raster drawn three times is one; a tiny
  picture is written in place, and 4 100 draws of a large one are exactly the
  4 096 references the reader expands and four copies; three thousand squares
  under a 4 KiB budget are cut short with `Truncated`, the replay stopping
  before the operator the page ends on. A soft mask, a blend mode, a knockout group, a mesh and
  a tiling pattern on one page each produce their warning, no refused element
  is written and the reader reports none of its own refusals; the nested
  clip pins the reader's shortfall above and the clipped image its clip; a
  fixed-seed campaign of mutated pages must never panic the writer, and the
  `render_page` fuzz target writes every page it reaches as SVG.
  `surface_goldens.rs`'s `to-svg` is the writer's whole output for one page —
  paths, text in an embedded face, an image, a clip, a group at half opacity
  and a fill under a soft mask, the mask the one thing declined and named —
  committed as a reviewed-golden surface with its header in an XML comment,
  compared past that comment byte for byte in both builds, and listed
  unreviewed until a person reads it against SVG 1.1 5.2, 5.7, 8.3, 11.3,
  11.4, 14.3 and 14.5.
- The retained page: `determinism.rs`'s
  `a_display_list_replays_every_fingerprinted_page_byte_for_byte` records
  every fingerprinted page once and replays it at 1× (the fingerprints'
  scale), 0.5×, 1.5× and 2.25×, with and without annotations, byte-equal to a
  direct render in pixels, size, format and warnings;
  `render_regions.rs`'s `a_display_list_tiles_as_the_page_does` tiles a
  replay of every region fixture and an annotated page against the direct
  render; `display_list.rs` holds the answers on the pages where the renderer
  says no — past the group budget, inside hidden content — and a cancelled
  replay against a cancelled render, the warnings a replay reports render
  after render, and a page past `MAX_DISPLAY_LIST_BYTES` drawn the direct
  way, pixels, warnings and SVG; `tinker-pdf-render`'s `display.rs` holds the
  recorder's count, its overflow and the interpreter told to stop;
  `tinker-pdf-content`'s `replay.rs`
  holds a replay into a recorder equal to the recording, and what a replay
  does when the device answers differently.
- Regions and ruling 5: `crates/tinker-pdf/tests/render_regions.rs`. Fixtures
  over four rasterizer paths and the three kinds of canvas that stand
  somewhere in the page — transparency and soft-mask groups, a tiling
  pattern's cell, a mesh's buffer — turned and cropped ones among them,
  tiled at 64, 37, 23 and 53 pixels against a 91×131 page at 0.5×, 0.75×, 1×,
  1.5×, 2×, 3× and 4× and down to a one-pixel lattice, each tile asserted
  **byte-equal** to its rectangle of the whole render with no tolerance at any
  scale; a stroked rectangle whose miter corners sit on a sub-scanline, at 1×;
  ruling 5's one named exception — an image run whose overlap falls outside
  the tile — pinned exactly, so a change in either direction fails;
  plus the trim at the page edge and its warning, a region wholly off the page, and — because tile equality alone is
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
- Determinism: the render fingerprints `text`, `curves`, `shading`, `blend`,
  `pattern`, `optional`, `image`, `transparency`, `tiling` and `mesh` in
  `crates/tinker-pdf/tests/determinism.rs` pin this device's output
  bit-for-bit across targets, each with an ink floor so a fixture that draws
  nothing fails instead of becoming a baseline ([determinism](determinism.md)).
- Fuzzing: the `render_page` fuzz target renders whole hostile documents;
  `crates/tinker-pdf/tests/hostile_input.rs` replays the sweep on stable. See
  [verification](../verification.md).
