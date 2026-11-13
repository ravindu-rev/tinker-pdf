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
three stages of curve with an interpolated grid between them, each reading a
Lab connection space in its own encoding (ICC.1 6.3.4.2): an `mft2` in
version 2's legacy one, `L*` 100 at `FF00h`, an `mft1` in its 8-bit one,
`L*` 100 at `FFh`. **v4's `mAB `
reads too**, which is five optional stages where v2 has three fixed ones — A
curves, a grid whose axes may each carry their own point count, M curves, a
matrix and B curves, run in that order although the header lists their
offsets in the reverse — and whose Lab is v4's, each component's range over
the whole currency, `L*` 100 at `FFFFh` and `a*` 0 at `8080h`. A grey
profile whose connection space is `Lab` rather than `XYZ` reads as well: its
one curve gives lightness instead of luminance, and the two differ by
8.6.5.4's cube root. A `D2B*` tag's floating-point
transform reads as well (`multiProcessElementsType`: curve sets of formula
and sampled segments, matrices with offsets and grids), compiled once into
integer stages, and converts its intent before that intent's `A2B*` table;
a tag holding an element of another type is refused by the element's name
and passed over. ICC.2's elements read beside ICC.1's: formula segments of
function types 3 to 7 (Table 111), single sampled curves clipped or extended
past their ends, extended grids of half floats or 8- or 16-bit fractions,
and the **calculator element** (11.2.1), a program in reverse Polish
notation over input, output and temporary channels, with conditional and
selected streams and sub-elements it invokes — validated whole before it
runs, every operation known, every channel index in range and the stack
never taken below what is on it, and run at every node of a grid over its
inputs' intervals when the table is built, never per colour; a sampled
calculator curve runs its calculator into a sampled curve when it is read.
Every count a program can spend is capped — the operations it holds, and the
steps one run takes, a step a value an operation moves, 65 536; its stack,
1 024; sub-elements and conditionals within one another, 16; temporary
channels, 1 024; the steps all of a tag's calculator runs take while it is
read, 67 108 864 — and a program past a cap is refused as
`IccError::CalculatorLimit`, naming it, before it runs. A calculator's
sub-elements, and a curve set's curves, are each read once however many
positions name them, and the tables they hold count against the tag's grid
entries. A segmented curve finds its segment by halving its break-points.
**A version 5 profile** (ICC.2) reads its header's fields past
byte 100 — the spectral connection space, its wavelength ranges, the
multiplex space and the sub-class (`Profile::version5`) — and its `A2B*` and
`B2A*` tags may hold such elements, read as a `D2B*` tag's are; it adds
`A2B3` and `B2A3`, absolute colorimetric's own tables, which absolute
colorimetric converts through as they stand before `A2B1`, and which another
intent reaches last with the media white divided out (ICC.2 9.2.93). Its
`D2B*` and `B2D*` tags take a spectral connection space. Where the header
names reflectance or transmission spectra, the `D2B*` tags give a spectrum,
one channel a wavelength, which is **reduced to XYZ when the table is
built**: integrated against the observer's colour matching functions and the
illuminant's power the profile's `spectralViewingConditionsTag` carries, each
resampled to the spectrum's wavelengths, `X = Σ R S x̄ / Σ S ȳ` and alike, a
last matrix stage of the pipeline. An intent converts through them only
where none of the profile's colorimetric tags or its model serves it, in
ICC.2 8.14.2's order, so a spectral-only profile converts to sRGB and,
through another profile's output transform, to its device space; the `B2D*`
tags take a spectrum, which no colorimetric value gives, and are not read.
Viewing conditions other than the CIE 1931 observer under D50 connect every
transform into the connection space through the profile's
`customToStandardPccTag` and every one out of it through its
`standardToCustomPccTag`; a profile carrying neither is refused for that
direction, and a model under them, which takes no stage after it, is passed
over. Emission, bi-spectral and sparse-matrix spectra, a profile with no
viewing conditions, and a standard observer or illuminant named without its
own steps are refused by name as `IccError::Spectral`, each where nothing
else serves the profile; every spectral range is held to 1 024 steps before
its values are read. A lookup table over one, three or four inputs, and a
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
in. An image converts in its own `/Intent` where that is a name (Table 89;
an inline image's under the same key), a name 8.6.5.8 does not list read as
relative colorimetric, and in the intent in force at `Do` or `EI` where the
entry is absent or not a name — rendered and written as SVG alike. An image
mask's `/Intent` is never read: its paint is the fill colour, converted as
any flat colour is. The decoded-image cache — a render's, and a document's
under a cache budget — keeps an image's decode under the conversion its
colours took, so an image in an ICC space, or one holding
one, drawn under two intents decodes once in each, and an image no intent
changes — in a device space, a mask, or one naming its own intent — decodes
once. A shading converts in the intent in force at its painting operator —
`sh`, or the fill or stroke that paints a shading pattern — an axial,
radial and function-based shading's colours, a mesh's vertices' and the
colour table a sweep reads its function through alike; an ICC shading
rasterised for SVG converts there as it renders. Relative colorimetric
converts through the first of `D2B1`, `A2B1`, `A2B0`, the model and `A2B2`
the profile carries. Absolute colorimetric converts through `D2B3` as it
stands where the profile carries one, that tag being ICC-absolute already,
and otherwise is relative
colorimetric's `A2B*` table or model with the connection-space value scaled
by the profile's media white (`wtpt`) over the PCS white, channel by channel
(ICC.1 6.2.3): folded into the matrix for a matrix/TRC or grey profile, so a
grey on a tinted medium draws in its tint, and applied in XYZ to a table's
value, a stage its grid's nodes evaluate. A media white of D50, or none,
makes it relative colorimetric's transform itself.
**The engine's own sRGB profile**, `tinker_pdf_color::icc::SRGB_PROFILE`, is
computed by the colour crate's `build.rs` from IEC 61966-2-1's published
primaries, D65 white point and transfer function
([design/icc.md](../design/icc.md), RC-19): a version 2.4.0 display profile
whose three colorant columns are adapted to D50 by ICC.1's linearized
Bradford transform, whose three channels share one tone curve of 1 024
entries, whose media white is the PCS white and whose `chad` is the
adaptation. To the CMM it is a profile like any other, a matrix/TRC model
that takes every grey and every primary back to itself within one level
(`srgb_profile.rs`), and nothing applies it unasked: a caller states it where
a profile is stated — the facade hands it out as `tinker_pdf::SRGB_PROFILE`,
an archival destination profile every part of ISO 19005 the writer claims
admits (`pdfa_writer.rs`) and the stream of an `/ICCBased` `/DefaultRGB`
that excuses `DeviceRGB` under any output intent (`pdfa_colour.rs`). Its
bytes are computed, not vendored, and are one string on every build host.
A scope compiles a profile stream once, the first time a space it names
reads it, and keeps the transforms — or the profile's refusal — for every
render over it; a document with a cache budget keeps them across its
renders, below.

A table that does not read is passed over to the next of those, and so is a
model that contradicts its profile — a monitor profile whose data space is
`LAB` carrying the `rXYZ` columns a matrix applies to linear RGB; one naming a
three-channel space no registry defines; a CMYK printer profile with a single
`kTRC` and no matrix, one curve for four channels of ink — so such a profile
converts through its tables. One with no table it can convert through is
`ColorSpace::Approximated`, read by component count, which is the
alternate-space reading 8.6.5.5 permits.

**The other direction**, a profile's transform out of the connection space,
is `tinker_pdf_color::icc::Output::compile(profile, intent)`: a value in the
profile's own connection space — D50 XYZ with the PCS white's `Y` at one, or
`L*a*b*` — to its device channels, each from 0 to 65 535, at most fifteen
(`DeviceColour`). It converts through the first source ICC.1 8.10.2's rows
name for the intent with `B` for `A` and `D` — the intent's own `B2D*` tag,
its own `B2A*` table, then `B2A0`, then the model inverted, then the other
`B2A*` tables — passing over one that does not read. A `B2D*` tag, and a
version 5 profile's `B2A*` holding elements, takes the connection space's
three channels, each held to its range — XYZ to what `u1Fixed15` encodes,
`L*` 0 to 100 and `a*`, `b*` −128 to 127 — through its elements compiled
into integer stages, to the data space's channels; one giving more than
fifteen is refused before any element is read. Absolute colorimetric's
`B2D3` is ICC-absolute already, and the value goes into it undivided. A `B2A*` table, `mft1` or `mft2`, takes the value
encoded as its type defines it — XYZ in `u1Fixed15` through the table's
matrix (10.8), Lab in the table's own encoding — through its input curves,
grid and output curves to as many channels as the data space has; one
declaring more than fifteen is refused before its curves are read, and one
of another count than the data space passed over. **v4's `mBA ` reads at a
`B2A*` tag too**, `mAB `'s five stages the other way round and in the order
its header lists them: B curves, a matrix with offsets whose results are
clipped to 0..1, M curves, the grid, and A curves, one a device channel
(10.11), each present where its offset is not zero, the B curves required
and a tag with no grid as many outputs as inputs; its XYZ is `u1Fixed15`
and its Lab v4's, as `mAB `'s. An `mBA ` at an `A2B*` tag, or an `mAB ` at
a `B2A*` one, is refused as `IccError::NeedsLut`. A matrix/TRC profile's
columns are inverted when it is built, by their cofactors in `f64`, and held
in s15.16, and each tone curve is inverted by bisection over its compiled
table into 4 096 entries read between them in fixed point; a grey profile
takes `Y`, or `L*` over 100 under Lab, through its inverted `grayTRC`.
Absolute colorimetric first divides the value by the media white over the
PCS white, in XYZ — under Lab through `icc::lab_to_xyz` and
`icc::xyz_to_lab`. A matrix whose determinant is below one s15.16 step, or
a curve that decreases anywhere or never rises, is refused as
`IccError::NoInverse`, and a profile carrying neither a `B2A*` table nor a
model as `MissingTags`. `Profile::connection_value(intent, components)` is
the forward half a transform out of another profile takes: the value the
intent's source gives, in the profile's own connection space. The profile's
**gamut tag**, `gamt` (9.2.28), is read beside its output tables:
`Output::in_gamut(pcs)` takes the value as a `B2A*` table does — divided by
the media white first under absolute colorimetric, encoded as the tag's type
encodes the connection space — through the tag's `mft1`, `mft2` or `mBA `
table to one channel, read with its grid's fractions as they are, and
answers out of gamut where that is half a step of the table's own precision
or more (1/255 for an `mft1` and an `mBA ` whose grid holds a byte an entry,
1/65 535 otherwise) and in gamut below it; a profile with no `gamt`, or one
whose `gamt` does not read, answers none, never in gamut. Nothing maps a
colour by it. No render converts through either transform; they are the
colour crate's transforms toward a device.

**A device link**, a profile of class `link` (ICC.1 8.6), takes one device's
values to another's: its data space is the space it takes, and its
connection-space field names the space it gives (7.2.7), a device's.
`Profile::parse` reads it when it parses the profile — its `D2B0` first where
that reads, as 8.10.3 has it, and otherwise its `A2B0`, an `mft1`, `mft2` or
`mAB ` — and `tinker_pdf_color::icc::Link::compile(profile)` is its transform:
`Link::apply` takes the input device's components, each held to `0..1`,
through the table's stages, or the elements', with no connection-space
encoding at either end, to the output device's channels, each from 0 to
65 535. The table's input count is held to the data space's and its output
count to fifteen before any of its curves is read, then to the output
space's; an `mAB ` carrying a matrix or M curves has the matrix's three
outputs. A link with neither tag is refused as `MissingTags`, and one whose
only `D2B0` holds an element this build does not read, by the element's
name. No transform to sRGB is built from a link, nor one out of the
connection space: an `ICCBased` space's profile may not be one (ISO 32000-2
Table 67), so a page naming one keeps 8.6.5.5's alternate-space reading.

**Named colours**, a profile's `ncl2` tag (ICC.1 10.15), are
`Profile::named`: each colour's name is the tag's prefix, its root and the
tag's suffix joined, each field 7-bit ASCII ended by a NUL, and
`NamedColours::get(name)` answers its connection-space coordinates — XYZ in
`u1Fixed15`, Lab in version 2's legacy encoding, relative colorimetric — and
its device coordinates as stored, each from 0 to 65 535; `NamedColour::to_rgb`
converts it to sRGB through the connection space's own path, as a table's
value goes. A profile of class `nmcl` (8.9) parses with its colours and no
transform, and is refused where its `ncl2` is absent or does not read; any
other profile carries its colours beside its transforms. The device
coordinate count is held to fifteen and the colour count to 65 536
(`icc::MAX_NAMED_COLOURS`) before any entry is read, and their entries to
the tag's own bytes. No render converts through a named colour.

**A soft proof** is `tinker_pdf_color::icc::Proof::new(bytes, display)`
([design/icc.md](../design/icc.md), "Soft proofing (RC-05)"): a profile of a
grey, RGB or CMYK device compiled once into the colour as that device
reproduces it, shown on the display. Its device leg, `Proof::device`, takes
an sRGB colour — IEC 61966-2-1's decode a 256-entry table in s15.16, then
`SRGB_TO_XYZ_D50` in s15.16, then for a profile whose connection space is
Lab 8.6.5.4's cube root sampled at every 1/4 096 of the ratio to the white
and read linearly in 32 fractional bits — or, as
`Proof::device_from_connection`, a colour's own connection-space value,
through the profile's transform out of the connection space at the colour's
intent, each intent's compiled the first time a colour asks for it and an
intent compiling to another's sharing it, to the device's components
rounded to bytes. Its display leg, `Proof::display`, takes those bytes back
to sRGB through the profile's input direction, an `A2B*` table or its model,
relative colorimetric so the paper shows as white, or ICC-absolute where
`ProofDisplay::Paper` shows the paper: a grey proof's as a table of the 256
bytes each evaluated exactly, a matrix/TRC proof's through its curves at the
256 bytes and its columns times XYZ-to-sRGB in s15.16, a lookup-table
proof's through the table in integers — its grid multilinear with 16-bit
fractions at its own nodes, its curves interpolated in 16 bits, a Lab
connection space taken to XYZ with the cube in 32 fractional bits — then
IEC 61966-2-1's encode sampled at 16 385 points, its index rounded;
`Proof::display_with` remembers a lookup-table proof's colours in a
`ProofMemo` of 4 096 slots keyed by the device's bytes, which changes no
byte. `Proof::apply` is the two composed. A profile that cannot proof is a
`ProofError`: refused by `Profile::parse`; a device link, an abstract or a
named-colour profile; a data space other than grey, RGB and CMYK; nothing
out of the connection space; nothing from the device back to the display.

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

**Every space answers at sixteen bits a channel beside its bytes**
(`docs/design/sixteen-bit.md`, ruling 23). `ColorSpace::to_rgb16_in`
evaluates a colour as `to_rgb_in` does and quantises each channel twice: to
the byte `to_rgb_in` gives it, and to `round(65 535·x)` of the value `x`
that byte is rounded from — a device component, or IEC 61966-2-1's transfer
of the light — held to `256·byte ..= 256·byte + 255`. The top byte of each
16-bit channel is `to_rgb_in`'s byte, bit for bit, and the low byte refines
it (`every_space_at_sixteen_bits_tops_out_at_its_eight_bit_colour`,
`device_spaces_at_sixteen_bits_are_their_relations`). An ICC colour is its
transform's one evaluation at both widths — a grid's interpolated light, a
table's or a pipeline's linear light, a matrix's sum — through
`Transform::apply16`, never a second transform. `lab_to_srgb16` is
`lab_to_srgb`'s evaluation quantised the same way, and `srgb_to_lab16` reads
a 16-bit sRGB colour by `srgb_to_lab`'s evaluation, each sample over
65 535, so a byte widened to sixteen bits, `257·v`, reads exactly as the
byte does. The renderer paints the bytes; nothing on a page asks for the
sixteen bits.

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
(`eval_into_starts_afresh_and_a_type_4_member_keeps_to_its_own_stack`). An image no table of its
converter stands for (*Images*, below), in any space other than the device
spaces, converts each colour once, remembered by its samples in a table of
4 096 slots that never answers one colour for another
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
operators inside it reach nothing. A document with a budget keeps the cell
between fills and pages (*The document's cache*, below): under its pattern
object, `TileRequest::to_pixels` by its bits — the device matrix and the
sub-pixel phase unquantised, so a hit is the bytes a miss draws — the
`/BBox` by its bits, the buffer's size and format, the anti-aliasing, the
levels, the rule at level 0 and the thinnest stroke it is drawn with, the
nesting depth, the scope its names resolve in where it brings no resources
of its own, and the usage the render names. So every fill of a pattern under
one page frame meets one cell, a `PaintType 2` one in every colour it is
filled in, and the cells rasterised over a document are at most the distinct
keys its fills ask for (`Stats::tiling_cells_rasterised`, `pattern_cache.rs`'s
`cell_rasterisations_are_at_most_the_distinct_keys`); another matrix or phase
is a cell of its own. A kept cell says again what its drawing reported — its
warnings, and the fonts, damaged images, repaired spaces and images drawn
reduced its scope was told — a cell a cancelled render drew is never kept,
and past the budget every fill rasterises its own, as with none. The lattice
is bounded before a pixel
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
read a sample at a time, and its colours asked of a converter chosen once for
the image by its space, its depth, its `/Decode` and whether a colour key reads
it (`docs/design/icc.md`, P-22, "The per-image converter"): an 8-bit ICC image
with `/Decode` absent or `[0 1]` and no colour key converts through its
transform's row path, which converts bytes as the transform converts
components, through tables the transform builds once; an image of one
component of up to eight bits, whatever its space and `/Decode`, reads a
table of its `2^bpc` values' colours, each converted through its space the
first time a sample holds it; an 8-bit DeviceCMYK image under `/Decode` absent
or `[0 1]`, and a CMYK JPEG drawn in its own colour a strip at a time, reads a
table of 8.6.4.4's `(1 − x)(1 − k)` for each ink byte against each black
byte, each entry filled by the conversion of the pixel that first asks for
it; and every other image converts each pixel through its space, its
components and a tint transform's outputs in buffers it keeps, remembering
its colours where a conversion is dear (below). A JPEG's and a JPEG 2000
image's samples drawn in the dictionary's space take the same converter, and
a refused profile approximated as a device space takes that space's. Each
arm's colour is the one its space's conversion gives the sample, so the
converter moves no pixel; what an image asks of the conversion is counted,
`Stats::image_colours_converted`, and does not grow with its pixels — none for
an 8-bit DeviceRGB or ICC image, at most 65 536 for an 8-bit DeviceCMYK one
and `2^bpc` for one component (`image_conversion.rs`'s
`a_converter_is_chosen_once_per_image`,
`each_device_arm_draws_what_to_rgb_gives_each_input`) — nor what it allocates
with its colours (`an_image_converts_in_allocations_flat_in_its_colours`).

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

A JPEG 2000 codestream is decoded with the most resolution levels dropped
whose image still covers the draw: the most `d` for which its own width and
height, as its header gives them, shifted right by `d` — the fewest samples
its image reduced by `2^d` has, whatever its origin on the reference grid —
are still at least the drawn pixels on both axes, and at least one, never
more than the decoder can drop (`JpxImage::levels`, which is none for a
codestream a `pclr` palette reads), and none when the drawn size is not
finite or the image has a soft mask or a `/Mask`. The same rule keeps an
image under an `/Indexed` space whole, a JPEG's and a codestream's alike:
its samples are indices, and a smaller scale averages two indices into a
third. The reduced samples stand for `2^d` of the image's own each way,
placed from where the first reduced cell starts — `ceil(XOsiz / 2^d)` cells
of `2^d` from the grid's origin, none into an image that starts there —
and cut where the image ends (`Reduced::skip`). The cache keeps each
discard's decode apart, read on a hit from the size and the levels the
name's first decode found. `jpx_images.rs` holds it:
`a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer` (A7's JPX half,
through `Stats::jpx_levels_synthesised`),
`a_jpx_image_is_decoded_with_the_most_levels_dropped_that_cover_its_drawn_size`,
`a_masked_jpx_image_is_decoded_whole`,
`a_jpx_image_whose_samples_are_indices_is_decoded_whole`,
`a_reduced_jpx_image_covers_what_the_image_covers`,
`each_discard_of_a_jpx_image_is_cached_apart`,
`an_inline_jpx_image_is_decoded_at_its_drawn_size` and
`the_svg_writer_and_the_image_list_take_a_jpx_image_whole`; `images.rs`'s
`an_indexed_jpeg_is_decoded_at_full_size`; the `jpx_minified` fingerprint and
`render_regions.rs` hold the reduced draws on every target and tile by
tile.

**Images drawn in part.** A draw's request says what of the image its canvas
reaches and what of that the clip and the soft mask show, each as a rectangle
of the image's unit square (`ImageRequest::reach`, `ImageRequest::visible`,
`UnitArea`): the page's canvas, a tile's, a band's, a group buffer's or a
pattern cell's mapped back into the square, and the clip's and the soft
mask's rectangles. A JPEG 2000 codestream is decoded over that part alone, in
the samples of the image its discard leaves, widened by the sampler's reach —
`(⌈footprint⌉ + 2) · 2^(h + 1)` samples for the area filter's footprint after
`h` halvings of the pyramid, its edges moved out to multiples of `2^(h + 1)` —
and the decoded image carries the rectangle it holds (`DecodedImage::window`),
its width and height staying the whole image's. The rasterizer samples a
windowed image over the whole of it and reads only the held rectangle, its
nearest held sample standing in for one outside it, so a pixel a draw shows
reads the samples the whole decode would hand it and every pixel of the
image's square is still covered. A draw that joins a run of images, which
holds what it accumulates wherever the clip hides it and decides by that
coverage whether a later draw joins it, is decoded over the part it shows
only when every sample is opaque, and otherwise over what it reaches; a draw
that composites alone, in a knockout group or on a page too large for runs,
shows and reaches the same part. A window holding all of the image is no
window, one that shows nothing holds a sample, and the cache keeps each
window's decode apart: a render's own at most sixty-four decodes an image,
the document's under its budget. A tile therefore
decodes the code-blocks its window reaches and is still the page under it
(ruling 5). A render's `RenderOptions::parallelism` reaches its JPEG 2000
decodes: a page drawn whole on the caller with threads, as one whose runs of
images leave no row to cut bands at is, decodes on them — its code-blocks,
tiles, components and wavelet levels (`jpx_decode_on`) — and a band, drawn
on a worker, decodes on that worker —
the same pixels and warnings as the serial page
(`a_jpx_page_on_2_3_4_and_7_threads_is_the_serial_page`). `jpx_images.rs`
holds it: `a_region_render_decodes_only_the_code_blocks_its_window_reaches`,
`a_clipped_jpx_image_is_shown_from_the_samples_under_its_clip` and
`each_window_of_a_jpx_image_is_cached_apart`; `render_regions.rs`'s
`a_region_of_a_jpx_page_is_the_crop_of_the_whole_page` tiles a page of
windowed draws, a clipped run among them, at lattices that do not divide it.

**The handshake, end to end.** `image_requests.rs` draws every answer a
decode gives a drawn size — a JPEG at a half, a quarter and an eighth, a
JPEG 2000 codestream with one, two and three levels dropped and with a
window — at the sizes that pick it, axis-aligned, flipped, quarter-turned and sheared, the
images odd in size, and holds each to the same draw of its full decode, which
a `/Mask` of backwards colour-key ranges asks for without masking anything:
`every_reduced_answer_lands_where_the_full_decode_does` (the pixels reached
and covered wholly, the edge's coverage and where the picture's weight lands)
and `a_reduced_answer_is_never_magnified` (the answer read from
`Stats::jpeg_samples_transformed` and `Stats::jpx_levels_discarded`, its
samples along each axis at least the drawn pixels, A7's bounds beside it);
`every_tile_of_the_answers_is_the_page_under_it` tiles a page of them.

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
removal at close (11.4.7.2), read from Table 147's `/I` and `/K`. A group's
buffer is the clip's overlap with the canvas, held once: while the group is
open the canvas beneath it moves into the buffer, nothing paints into it, and
a non-isolated group reads its backdrop there where it lies — its alpha at
each pixel it composites, its colour at adoption and again at removal — as a
knockout group reads its initial state, so neither keeps a copy; the buffer
hands the canvas back before anything else at close, a cancelled render and
a group left open at the end of one included (`group_buffers.rs`). `/CS` is read and
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

**Blue-first output orders.** `format: PixelFormat::Bgr8` or `Bgra8` hands
the page back blue, green, red and, for `Bgra8`, alpha last — the orders a
host's surfaces store. They are output orders: the page composites in `Rgb8`
or `Rgba8` (`tinker_pdf_render::canvas_format`) and the finished canvas is
reordered once by `Canvas::extract`, so every group, mask, cell and image run
keeps the format it has and no blend meets a second channel order;
`page_format` hands each back as itself. `transparent` and `premultiplied`
apply to `Bgra8` as to `Rgba8` and not to `Bgr8`, which like `Rgb8` is on
white; a region is the page's rectangle in the same order. Every byte is the
`Rgba8` render's pixel with red and blue exchanged, the alpha dropped for
`Bgr8` (`bgr_and_bgra_are_the_rgba8_render_reordered`,
`bgra_tiles_are_the_bgra_page`). The C ABI keeps its four formats.

**A page's spot colorants** (8.6.6.4, 8.6.6.5;
[design/overprint-separations.md](../design/overprint-separations.md)).
`Page::spot_colorants` lists each spot colorant the page's resource scopes
name once, as a `SpotColorant` carrying its name — bytes, `#` escapes decoded
— in the order the scopes first name it, which is the order a separations
render lays its spot plates out in. A spot is a colorant of a `/Separation`
or `/DeviceN` space that is not one of the four process colorants 8.6.6.4
reserves (`Cyan`, `Magenta`, `Yellow`, `Black`), not `/All` or `/None`, and
not a process component an `NChannel` space's `/Process` names; a plain
`/DeviceN`'s `/Process` is not read, as 8.6.6.5 has it. The scopes are the
ones `Document::fonts` walks, by the same walk, with what a render draws: the
page's `/Resources`, each Type 3 font's, form's, tiling pattern's and soft
mask's group's within them, and every state of each annotation's normal
appearance — not its down or rollover one. Within one `/Resources` dictionary
its `/ColorSpace` entries come first, then its XObjects' spaces — an image's,
not a stencil mask's, and a form's transparency group's `/CS` — its
shadings', its shading patterns' and its soft masks' groups', each space
followed by an `/Indexed` base or a `/Pattern` space's underlying space, a
`/DeviceN` giving its names in array order and then its `/Colorants` keys;
then the scopes the dictionary opens, in the walk's order, and the
annotations last, an appearance's group before its resources. Names are read
by the readers `Page::images` and the renderer use (`separation_name`,
`device_n_names`), and nothing is decoded or evaluated, so listing adds
nothing to
`Document::warnings`. A scope is walked once, the walk stops sixteen scopes
deep and the listing at 4 096 spots, each with what it had read
(`separations.rs`, `the_colorant_inventory_is_every_spot_its_resources_name`).

**A separation keeps its colorants.** `ColorSpace::Separation` carries
`Colorants` beside its alternate and tint transform: each component's
`Colorant` in the space's order — a name's bytes, `/All` and `/None` told
apart in a `/Separation`, while a `/DeviceN`, which 8.6.6.5 gives no `/All`,
names a colorant called `All` — and a `/DeviceN`'s attributes, `/Subtype
/NChannel`, each `/Colorants` entry that is a `/Separation` space, the
colorant alone, and `/Process`, its space and its components' names.
`parse_space` reads the names through the readers `Page::images` uses, so the
two give every space the same names in the same order
(`parse_space_and_describe_space_read_the_same_names`), and reads the spaces
an attributes dictionary holds — and a separation's alternate — without
attributes of their own, so a `/DeviceN` naming itself in its `/Colorants`
reads once rather than fanning out. No conversion to light reads any of it,
so a light render draws as it did; two spaces differing only in a name convert
alike and compare unequal, and no comparison in the engine compares two
separations.

**The resources answer what a colour paints by colorant.** A page's
resources answer `FontSource::colorants` — asked only for a device that
composites colorants — from the space as selected, as `resolve_ink` asks it,
and the colour's light as painted in the intent in force (the design's
"Names on plates"):
DeviceCMYK is its four process colorants at its own components, a `k` under
a `/DefaultCMYK` among them (H-135); a `/Separation` names its colorant at its
tint, and a plain `/DeviceN` each of its colorants, `/None` discarded and a
process colorant under its own name, beside the process ink the whole colour
paints through its alternate — a DeviceCMYK alternate's own components, any
other alternate's light turned into ink with maximum undercolour removal;
`/All` is one tint for every colorant and never reverts, and a space naming
only `/None` paints nothing. An `NChannel` space answers component by
component: a reserved process name, or a component a CMYK `/Process` names,
under its process colorant's name; each spot with its own process ink, through
its `/Colorants` space or, without one, the space's tint transform at its tint
alone; and the components of any other process space converted together
through it, those its names leave out at the space's initial values, to light
and to ink. An `/Indexed` colour answers as its palette entry in the base, an
uncoloured pattern's components as its underlying space, and every space that
names no colorant — and a separation whose names cannot all be read — answers
none, its light being what a device turns into ink
(`the_seam_answers_each_spaces_colorants`). A recorded event counts its
state's colorants — their record, their tints and every colorant's name — in
`Event::kept_bytes`, with each state that carries them as a colour's source
is counted, so `MAX_DISPLAY_LIST_BYTES` and a kept form's
`form_recording_bytes` bound what a recording holds with them
(`a_recorded_state_counts_its_colorants`).

**Flat colours on plates.** A `Renderer` over a `PixelFormat::Plates` canvas
handed a `PlatePlan` (`Renderer::with_plates`) composites colorants: the
interpreter asks each colour what it paints by colorant, and the renderer
maps those colorants to the canvas's channels by the plan (the design's
"Colours on plates"). The plan is the four process plates, then a plate for
each spot it names `PlateUse::Spot` or `PlateUse::Disabled`, in its order and
at most `MAX_SPOT_PLATES` of them; a spot it names `PlateUse::Composite`, one
past the cap and one it does not name have none, a name given twice keeps its
first use, and `PlatePlan::channels` lists each plate with its colorant and
its use. DeviceCMYK as the current colour paints its own components on the
process plates; a `/Separation` or a plain `/DeviceN` paints each colorant on
its plate when every name it gives has one — a process name its process
plate, a disabled spot its own — and otherwise the whole colour's process ink
through its alternate; an `NChannel` space paints each component that has a
plate there, each spot without one its own process ink on the process plates
and another process space's components their ink, inks meeting on one plate
added and held at solid; `/All` tints every plate the canvas carries; and
`/None`, or a `/DeviceN` of `/None` alone, has no effect at all — nothing
drawn and nothing knocked out. Every channel a paint names no tint for takes
none, so under `Normal` a paint knocks out every other plate, as 8.6.7 has a
paint do with overprint off. Fills, strokes, glyphs, stencil masks and
uncoloured patterns' cells (`Canvas::composite_inked`) paint so, through the
clip, the soft mask, the constant alpha and the blend mode as on a light
canvas; a colour naming no colorant of its own — DeviceGray, DeviceRGB, a
CIE-based or ICC space — is its light turned into ink on the process plates,
as on `CmykA8`. A stencil tinting a spot composites on its own rather than in
an image run, which holds light and ink; one tinting the process plates alone
is that ink, as on `CmykA8`. A soft mask's group renders in `CmykA8` and is
read as a mask as on an ink page, every spot in it through its alternate
(11.7.3). Light becomes ink on the process plates by the black generation and
undercolour removal in force, as on `CmykA8`. So the process plates of a page
in DeviceCMYK, DeviceGray and DeviceRGB are its `CmykA8` render's bytes
(`process_plates_draw_as_the_ink_canvas_does`), and a spot's paint is exactly
its tints on its plates and no ink on the rest
(`a_spot_paint_lands_on_its_plate_and_knocks_out_the_rest`). A renderer over
any other canvas keeps no plan and asks no colour its colorants, so a light
or ink render draws as it did.

**Groups and coloured patterns on plates** (the design's "Groups on plates"
and "Patterns on plates"). A transparency group with no `/CS`, or one whose
`/CS` is a four-component subtractive space, composites on plates in a plates
buffer of its own carrying the canvas's spots — isolated, non-isolated over
its parent's plates, the backdrop adopted and taken out by 11.4.4 and
11.4.7.2, and knockout (11.4.5) — so a spot painted inside it keeps its plate
(11.7.3's first option) and process ink inside it gives the bytes a `CmykA8`
page gives (`a_group_on_plates_composites_as_a_cmyk_group_does`,
`a_spot_inside_a_group_keeps_its_plate`). A group declaring DeviceGray,
DeviceRGB or `/Lab` composites in that space, every spot in it through its
alternate (11.7.3's second option), and its buffer reaches the plates as light
turned into ink, by the black generation and undercolour removal at its `Do`,
every spot plate under its shape at no ink. A coloured tiling cell
(`/PaintType 1`) is drawn in the page's plates by the page's plan
(`TileRequest::plates`) and tiled channel for channel from the page-frame
lattice, so a spot inside it reaches its own plate
(`a_coloured_cell_tiles_its_plates`); a document's kept cells are kept under
the plan they were drawn by. A plates group is one group against
`MAX_GROUP_BUFFERS` (`plate_groups_spend_the_group_budget_as_any_group_does`).
Every plates buffer live at once — the page's, each open group's and a cell's,
each at the size it has on a render of the whole page, a cell's renderer told
what is held outside it (`Renderer::with_held_plates`) — is held to
`MAX_PLATE_BYTES`, 1 GiB: a group or a cell whose buffer would pass it is
drawn in ink, `CmykA8`, its spots through their alternates, and the render
carries `RenderWarning::PlatesCapped { named, kept }`, `kept` 0 of the spot
plates the page carries
(`a_plates_buffer_past_the_byte_bound_draws_in_ink`,
`a_group_inside_a_cell_is_charged_beside_the_page_s_plates`).
`spots_within_bytes` is how many spot plates a page of a number of pixels keeps
inside the bound: eleven at `MAX_PAGE_PIXELS`, sixteen bytes a pixel, and the
cap's sixty-four on an A4 page at 300 dpi.

**A page's separations** (the design's "Plates handed out").
`Page::separations(&SeparationOptions)` draws the page through the pipeline
`Page::render` draws through — `draw_canvas`, the drawing half of
`draw_sheet`, so the region, the scale and its clamp, the rotation, the crop
box, the antialiasing, the annotations and the warnings are that render's —
on plates, and hands back `Separations`: each plate as a `Plate`, its
colorant's name, its `PlateKind` (`Process` or `Spot`) and its tints, a byte a
pixel, 0 no ink and 255 solid, row by row at the render's size; with
`RenderOptions::transparent` the page's alpha beside them; the warnings; and
the counts where `RenderOptions::stats` asked. The plan is the four process
plates and the page's inventory (`Page::spot_colorants`), in its order, each
colorant `PlateUse::Spot` unless `SeparationOptions::plates` names it —
`Composite` draws a spot through its alternate on the process plates and hands
back no plate for it, `Disabled` draws its plate and leaves it out of what
comes back, a name's first entry decides, a process colorant is never
composite, and a name the page has no colorant of is passed over. A page whose
transparency group's `/CS` is a four-component subtractive space, or that has
none, is drawn on plates; one of another space composites in it, every spot
through its alternate (11.7.3), and its plates are that light turned into ink.
The plan keeps at most `MAX_SPOT_PLATES`, 64, spot plates and as many as keep
the whole page's plates inside `MAX_PLATE_BYTES` (`spots_within_bytes` of the
page's pixels, whatever region is asked, so a tile keeps the page's plates);
the spots past either, in inventory order, are drawn as composites — so a
`/DeviceN` naming one reverts to its alternate as a whole, the spots of it
that kept a plate included — and the render carries
`RenderWarning::PlatesCapped { named, kept }` once, `named` the spots that
asked for a plate, a composite one asking for none and a disabled one
counting, and `kept` how many of them, the first in inventory order, kept
one. A
page whose `/DeviceN` spaces name sixty-four spots hands back sixty-eight
plates, each spot's holding exactly its own fill and every other plate none
there (`sixty_four_spots_render_sixty_eight_plates`, over the render group's
64-spot page); `a_page_past_the_plate_cap_reports_what_it_left` holds a page
of seventy spots, with composite and disabled ones, and a page of
`MAX_PAGE_PIXELS` whose byte bound keeps eleven of thirteen, every plate of
each to what the page puts there. Both caps are rows of `bounds_ledger.rs`,
and the `render_page` fuzz target's memory limits hold the plates beside its
canvas. The separations fixture's eight plates — four spots in fills, text in
an embedded face, groups, an image, a shading and a mesh — are fingerprinted
on every target (`separations_are_stable_across_targets`,
[determinism](determinism.md)). A plate is not a
`Bitmap`, whose single channel reads as grey light. The format, `allow_cmyk`,
premultiplication and forced colours say how light comes back and are not
read, and the page is drawn on the caller's thread whatever `parallelism`
asks. With every pixel whole or empty, opaque paints, whole-level tints and
tint transforms linear from no ink into DeviceCMYK, each spot's plate folded
through its transform and added to the process plates is, byte for byte, the
page drawn with every spot composite
(`n_spots_render_n_plus_four_plates_whose_sums_reproduce_the_composite`, for
one, two and eight spots in `/Separation` and `/DeviceN` spaces);
`each_plate_behaviour_paints_as_it_says`,
`a_page_group_of_another_space_draws_its_spots_through_their_alternates` and
`separations_take_the_render_s_options` hold the rest, and
`render_regions.rs`'s `separation_tiles_are_the_page_s_plates` holds every
tile's plates byte-equal to the page's, turned, cropped and at 2x. The
`render_page` fuzz target draws a page's separations.

**Images on plates** (the design's "Images on plates"). On plates by a plan
the renderer asks for an image through `GlyphSource::image_on_plates` and
`inline_image_on_plates`, whose defaults are `image`'s and `inline_image`'s
decode. A page's resources answer them with the light decode — every image
as RGB, never as two colours — and, where the image's space names colorants
(DeviceCMYK, a `/Separation`, a `/DeviceN`, or an `/Indexed` over one of
them), `DecodedImage::plates`: `ImagePlates::Tints`, a tint for each plate of
the plan a sample, or `ImagePlates::Nothing` for a space naming only
`/None`. Each sample's tints are its colour's as a flat colour of the same
space and components puts them (`colorant_tints`, `PlatePlan::sample_tints`):
DeviceCMYK's components after `/Decode`, a separation's names on their plates
or the whole colour through its alternate where one has none, an `NChannel`
space component by component, an `/Indexed` sample its palette entry's — read
from the raw values the light's sample loop reads, worked out once for each
distinct sample up to 4 096 of them, for a stored or inline image of the
sample loop, a JPEG (a CMYK one drawn in its own colour as DeviceCMYK) and a
JPEG 2000 codestream (its components taken out of a premultiplied opacity as
its light is). A decode for plates is the draw's own, at the size the draw
asks: the scope's and the document's caches keep light. The renderer samples
the tints plate by plate through `draw_plate_image`, and holds abutting ones in
a run of plate images (`Fragments::with_channels`) as it holds light, a draw
of the other kind ending the run; a run of tints is charged against
`MAX_PLATE_BYTES` at its canvas's size on the whole page, and past it each
image draws on its own. An image in `/None` has no effect on the page; one in
any other space reaches the process plates as its light turned into ink, as
the `CmykA8` page has it; a stencil paints its fill colour's colorants, as
before (`an_image_in_each_ink_space_lands_on_its_plates`, at 1:1 and 2:1,
stored and inline, and
`an_image_of_none_or_of_light_reaches_plates_as_a_light_page_has_it`). A light
render decodes as it did.

**Shadings on plates** (the design's "Shadings on plates"). The colorant
answer is the render crate's `colorant_tints`, with `tint_byte` its tint's
byte, which a page's resources give the interpreter for a flat colour and the
renderer reads a shading's colours by. On plates by a plan an axial, radial
or function-based shading, painted by `sh` or as a shading pattern, takes its
road to the plates once a paint, from its space (`Shading::space`):
DeviceCMYK each component onto its process plate; a `/Separation` or plain
`/DeviceN` every name of which has a plate each component onto its
colorant's; any other space naming colorants — a separation with a spot the
plan composites, an `NChannel` space, `/All` — through `colorant_tints` and
`PlatePlan::sample_tints` colour by colour, as a flat colour of the space and
components paints. Each pixel's tints are read off the function outputs the
same evaluation converts to light (`ShadingScratch::outputs`), so a sweep on
plates keeps no colour table and evaluates its function at every pixel it
paints, and they composite through `Canvas::blend_tints` at the pixel's
coverage and the paint's alpha; under `Normal` a shading knocks out every
plate it does not name. A shading in `/None` has no effect on the page, and
one in a space naming no colorant reaches the process plates as its light
turned into ink (`a_shading_in_each_ink_space_lands_on_its_plates`, types 1, 2
and 3 by `sh` and as a pattern, each plate the tint the test reads off the
shading's function at every pixel centre, within ruling 22's level, and
`a_shading_in_none_paints_nothing`). A light sweep tabulates and converts as
it did.

**Meshes on plates.** On plates by a plan a mesh (types 4 to 7) in a space
naming colorants is drawn in tints, through the rasterizer's
`draw_mesh_tints_over`: one tint a plate a pixel, from the same silhouette,
walk and fringe spread a light draw of the mesh makes. Each pixel's
interpolated inputs are converted once, and its tints read off the components
that light came from (`Mesh::color_components`: the inputs themselves, or a
`/Function`'s outputs at them, evaluated afresh, so a mesh on plates keeps no
colour table) by its space's road, as a shading's are; they composite through
`Canvas::blend_tints` at the mesh's coverage and the paint's alpha. A mesh in
`/None` has no effect on the page, and one in a space naming no colorant
reaches the process plates as its light turned into ink
(`a_mesh_in_each_ink_space_lands_on_its_plates`, a type 4 mesh and a type 6
patch in a `/DeviceN`, through a `/Function`, in a `/Separation`, DeviceCMYK
and a composited spot, by `sh` and as a pattern, each plate the tint the test
interpolates at every pixel centre, and `a_mesh_in_none_paints_nothing`). A
light mesh draws and tabulates as it did.

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
reference — and sampled to 256 entries. A soft mask is kept at its group's
box, beside the value it reads outside it — `/BC`'s luminosity through `/TR`,
zero for black and for `/Alpha` — so a pale backdrop costs the box as a black
one does, and a tile holds the box clipped to the tile with the same value
outside (`group_buffers.rs`). An absent `/SMask` key leaves the mask in force where `/None`
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
`fill_mask_inked`, as a path's coverage is; it writes the rendering-parameter
plane with the placed mask times the clip and takes the ink tables, as a
path's paint does (`device_state.rs`'s
`a_glyph_takes_the_transfer_and_the_ink_tables`). So the same glyph at the same
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
of the font, the code, the quantised 2x2, the phase, one of 256, and the
flattening tolerance its outline is flattened at, the flatness in force, and is
kept for the rest of the render under those five; a later glyph with the same
five is drawn from it, building no path, and a hit is the mask a miss would
rasterise, because a mask a cancelled render stopped short is drawn and never
kept (`device_state.rs`'s `a_glyph_mask_is_kept_per_flatness`). The font is the glyph source's answer, `GlyphSource::font_identity`, so
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
Type 3 procedures bypass it. Where the glyph source keeps masks beyond the
render — `GlyphSource::kept_mask`, which a document with a cache budget
answers (below) — the source holds them and the render's cache none: it is
asked by `MaskKey`, the render's key with the font as the id its glyphs carry
in the scope asked and the render's anti-aliasing beside it, and handed each
mask a render that is not cancelled rasterised to the end
(`GlyphSource::keep_mask`).

**Optional content** (8.11). The render's configuration — the catalog's
`/OCProperties` `/D`, or the entry of `/Configs` `RenderOptions::layers`
chooses — and the overrides it names are resolved once a render, by the
render's operation, and every
scope the render opens — the page's, each form's, each tiling cell's and each
annotation appearance's — reads that one binding (`optional.rs`'s
`one_render_binds_one_configuration` and
`a_chosen_configuration_is_bound_once_a_render`): `/BaseState` sets every
group in `/OCGs` and `/ON`/`/OFF` name the exceptions (8.11.4.3 Table 101),
the configuration's `/Intent` decides which groups count at all (8.11.2.3),
OCMDs
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

**A render names its usage** (ISO 32000-2 8.11.4.4,
[design/render-usage.md](../design/render-usage.md)). `RenderOptions::usage`
is `None` by default, and then every group is in the initial state `/D` gives
it, the render every page has always drawn. Naming `Usage::View`,
`Usage::Print` or `Usage::Export` applies `/D`'s `/AS` entries whose `/Event`
is that usage over the initial states: each group an entry's `/OCGs` lists is
recommended a state by each of the entry's `/Category` names its `/Usage`
answers — `/View`'s `/ViewState`, `/Print`'s `/PrintState` and `/Export`'s
`/ExportState`, whichever event's entry names them, and `/Zoom`, `/Language`
and `/User` against the render's inputs (below) — and a group with any
recommendation is on only if every one it has, across every entry of the
event, is on. A category with no entry in a group's `/Usage`, or no state in
it, recommends nothing, and so do `/Language` and `/User` where the render
states no language or no user of the kind asked, and `/CreatorInfo` and
`/PageElement`, which carry no state — so a group nothing recommended keeps
`/D`'s state. A print render applies print's entries over
the initial states, as a printing application's render of the document as
opened, not view's first. What is malformed — an `/AS` or a `/Category` that
is no array, an entry that is no dictionary or names no known `/Event`, a
`/Usage` or a category that is no dictionary, a state that is neither `/ON`
nor `/OFF` — recommends nothing and leaves its group as `/D` set it
(`optional_content.rs`'s
`a_print_usage_render_of_an_ocmd_with_print_usage_is_its_d_twin`,
`each_usage_and_state_category_renders_as_its_d_twin`,
`recommendations_combine_and_what_is_malformed_leaves_d` and
`a_usages_states_reach_every_scope_a_render_opens`). Text and image
extraction, `to_svg`, a save's passes and the validators name no usage and
read the initial states, bound once an operation as a render binds them.

**The zoom, language and user categories** (8.11.4.4,
[design](../design/optional-content-config.md)) are read against what the
render names, and nothing reads the host's zoom, locale or user. `/Zoom`
recommends a group on where `RenderOptions::magnification` is at least its
`/min` (0 by default) and below its `/max` (infinity by default), and off
elsewhere; a magnification that is no finite number above zero is read as 1.
`/Language` reads `LayerOptions::language`, one language tag, `None` or empty
stating none, and decides over each entry's `/OCGs` together: where a group's
`/Lang` matches the tag exactly — the whole tag, ignoring ASCII case — those
groups are on and the entry's other groups with a `/Language` off; where
none does but some match its primary subtag, before the first hyphen, those
whose `/Preferred` is `/ON` are on and the others off; where none matches,
all are off. `/User` reads `LayerOptions::user`, a `LayerUser` of an
individual's name, a title and an organisation, each optional: a group's
`/Type` (`/Ind`, `/Ttl` or `/Org`) picks which is compared, and its `/Name`, a
text string or an array of them, recommends on where one equals it exactly
and off where none does. A `/Zoom` or a `/Language` or `/User` entry this
build cannot read — a bound that is no number, a `/Lang` that is no text
string, a `/Preferred` neither `/ON` nor `/OFF`, a `/Type` none of the three,
a `/Name` that is no text string or holds something else — recommends
nothing and leaves its group as configured. A display list keeps the
magnification its states were bound at, 1 for the lists a caller asks for,
and a render at another replays it only where those states read no `/Zoom`
entry or come to the render's own; a form's kept drawing is keyed by the
magnification where its states read one, and by nothing more where they do
not (`optional_content.rs`'s `each_auto_state_category_renders_as_its_d_twin`
and `a_forms_kept_drawing_is_keyed_by_the_magnification_its_zoom_reads`).

**A render chooses its configuration** (8.11.4.3,
[design](../design/optional-content-config.md)). `RenderOptions::layers` is
a `LayerOptions`, whose `configuration` is `None` by default for `/D` and
`Some(i)` for the `i`th entry of `/Configs`, the index
`Document::layer_configurations` lists it at; visibility is no longer
something only an edit of `/D` changes. The states are built in one order:
`/D`'s initial states; then the chosen configuration over them, applied to
the document as opened — `/BaseState /ON` or `/OFF` setting every group
`/OCGs` lists, `/Unchanged` leaving each as `/D` set it, one missing or none
of the three `/ON` — and its own `/ON`, then `/OFF`; then the chosen
configuration's own `/AS` entries for the render's usage, none where it has
none, since Table 99 gives `/AS` no default from `/D`. The configuration's
`/Intent`, a name or an array of names and `/View` otherwise, then decides
which groups count: a group none of whose intents (`/View` by default) the
set holds has no effect and reads as one the configuration never mentioned
does, visible, as itself and as a member of a membership dictionary; `/All`
counts every group and an empty array none. `/D`'s own `/Intent` decides the
same for a render of `/D`, so a `/Design` layer `/D` turns off draws. An
index past `/Configs`, past 256 entries, or of an entry that is no dictionary
renders as `/D` and is reported as
`RenderWarning::UnreadableLayerConfiguration { index }`. A form's kept
drawing is keyed by the configuration beside the usage. A display list
records under the layer options `Page::display_list_with` names — the
defaults for `display_list` and `display_list_for` — and keeps the states its
recording bound; a render naming its usage replays it where the render's own
states are those, whether it names the same configuration or another that
comes to the same states, binding its states once to compare them and drawing
with that binding either way, and draws the direct way otherwise; `to_svg`
from a list recorded under another configuration interprets the page again
(`optional_content.rs`'s `each_configuration_renders_as_its_d_twin` and
`an_intent_the_configuration_does_not_hold_has_no_effect`).

**A render overrides groups** (8.11.4.3,
[design](../design/optional-content-config.md)). `LayerOptions::overrides`,
empty by default, lists groups — the references `Document::layers` reports —
each turned on or off, applied in order after the configuration and its
auto-state and before its `/Intent` is read, so a later override of a group
wins over an earlier one. Turning a group on turns off every other member of
each of the configuration's `/RBGroups` collections that holds it — a
configuration other than `/D` with no `/RBGroups` takes `/D`'s — and turning
one off changes nothing else; the collections constrain the overrides alone,
so a configuration whose own `/ON` turns two members of one on shows both
until an override turns one of them on. A group the configuration's `/Locked`
lists keeps its state whatever an override asks, and a locked member of a
collection is not turned off; each is reported once a render as
`RenderWarning::LockedLayer { layer }`, by the group's `/Name` or by its
reference, `12 0 R`, where it has none. The locks are the chosen
configuration's own, never `/D`'s, and an unreadable configuration draws
`/D`'s with `/D`'s locks. An override of a reference `/OCGs` does not list
changes nothing, and a `/RBGroups` or `/Locked` this build cannot read
constrains nothing. The overridden states reach every scope a render opens,
a form's kept drawing is keyed by them with the configuration, and a display
list replays a render whose overrides come to its own states
(`optional_content.rs`'s
`overrides_radio_buttons_and_locks_render_as_their_d_twins` and
`an_override_reaches_every_scope_a_render_opens`, `render_options.rs`'s
`overridden_layer_output_is_pinned`).

**Which annotations draw** is their flags' and their `/OC`'s to say, for the
render's usage (12.5.3 Table 167, [design](../design/render-usage.md)): one
`annots::prepare` decides for a direct render, a display list's recording
and `Page::render_annotation`. `/Hidden` never draws. `/Invisible` on an
annotation whose `/Subtype` is none of those ISO 32000 defines — the list
`Page::annotations` reads `AnnotationKind` by — or that has none draws in no
usage, since every annotation is drawn by its appearance and none by a
handler. A print render draws only an annotation whose `/Print` is set,
`/NoView` or not; every other render — `Usage::View`, `Usage::Export` and a
render naming none, which is today's — draws every annotation whose
`/NoView` is clear, `/Print` or not, so an export is the picture the screen
shows. An annotation's `/OC` is read under the render's states by the code
that reads a content stream's (8.11.3.3), its fallbacks visible; one it hides
is not drawn and is reported as `RenderWarning::HiddenOptionalContent` by its
layer's `/Name`, or its reference where it has none, as content in that layer
is. `to_svg` writes the annotations a render naming no usage draws. A display
list records what its own usage draws and replays only that usage.

**`/NoZoom` and `/NoRotate` are placed at render time** (12.5.3).
`RenderOptions::magnification` is the viewer's zoom beside the resolution
`scale` is, 1 unless the caller states one, a value that is not a finite
number above zero read as 1; these two flags and a `/Zoom` auto-state (above)
read it and nothing else does, so no other annotation, and no page whose
states read no `/Zoom`, changes with it. About `P`, `/Rect`'s upper-left
corner in default user space, `(min x, max y)` — "the annotation pivots
around that point" — a `/NoZoom` appearance, after 12.5.5's fit, is scaled by
`1 / magnification`, so it keeps the size it has at magnification 1, and a
`/NoRotate` one is turned counter-clockwise by the page's `/Rotate`, which
the view's clockwise turn undoes, so it stands upright on a turned page. Both
are computed in the page's frame from the magnification and `/Rotate` alone,
in exact quarter turns and an exact reciprocal, never from a region, so a tile
draws the page's pixels (ruling 5). `Page::render_annotation`'s viewport is
the placed `/Rect`; a display list keeps such an annotation prepared and
interprets it at each replay where that render places it, and a page drawing
one is drawn whole rather than in bands; `to_svg`, at magnification 1, places
`/NoRotate`; a patch's rectangle is the placed one.

**The content, the widgets and the other annotations render separately.**
Beside `RenderOptions::annotations`, which off still draws no annotation of
any kind, three switches, each on by default so that no render changes:
`content`, the page's content stream, which off is not read and reports
nothing; `widgets`, widget annotations (12.5.6.19); and `other_annotations`,
every annotation that is not a widget — markup annotations, links, stamps
and watermarks alike. Three, because a page's widgets alone, and its content
with its widgets alone, are two of the seven combinations of the three parts
and need a switch of their own for the annotations that are not widgets. A
direct render draws only the parts that are on, each as the page whose other
parts were deleted draws it, and an annotation of a kind left out is passed
over before anything of it is read. A widget draws the normal appearance its
`/AS` names in every usage, print included: there is no field highlight to
leave out. A display list keeps the content's calls and each annotation's
apart, beside whether it is a widget, and replays only the parts on; the one
state a part hands on to the next is the group budget, so on a page whose
recording spent it, a render that leaves out a part recorded before a part it
draws draws the direct way. The page analysis reads the same switches;
`Page::render_annotation` reads none of them; `to_svg` keeps its own
`annotations` switch and writes every part.
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
outside a tile and abut inside it are one run in the tile and two on the page.
The gap is a class of eleven shapes, which
[design/image-edges.md](../design/image-edges.md) names: the overlap outside
the tile in either direction, a draw wholly outside the tile that overlaps a
member, soft-masked and stencil images, a run in a group's canvas and in a
soft mask's, a turned, a cropped and a turned and cropped page, and a run in
an annotation's appearance, each drawn from a display list as directly
(`render_regions.rs`'s
`an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`
and `every_shape_of_the_image_run_exception_is_pinned`; ROADMAP RC-21). A renderer reports what its canvas could not see of the runs
(`Renderer::run_reach`, `RunReach`): where an image that could join a run
meets the run's bounds in the page's frame outside the canvas, and every
image a run could hold that the canvas does not hold whole — nothing, for a
render of the whole page. A region reaching past the page is **intersected**,
never slid back on, because a moved rectangle returns real pixels from
coordinates the caller did not name; one that misses entirely comes back with
no pixels. Both trims are reported as `RenderWarning::RegionClamped`. Ruling
5's byte-equality guard and its fixtures are in [rulings](../rulings.md).

**Patching an edited page.** `tinker_pdf::write::append` writes an editor's
edits as an update and reopens the document over it
([writing](writing.md)), and `Appended::patch(index, &mut previous, &options)`
re-renders page `index` over `previous`, the bitmap of the same page before the
edit at the same options, by the rectangle the update changed. A page whose
update changed nothing it draws but annotations is the page before it
everywhere outside the rectangles of the annotations it changed, added or
removed, before the edit and after it: each one's `/Rect`, where 12.5.5's fit
puts an appearance with a `/BBox` and the box clips it
(`annots::drawn`), carried through the page's view transform, their hull
rounded outward and widened by a pixel. That rectangle is rendered through
`RenderOptions::region` and written over `previous`, and ruling 5 makes it the
full render's own pixels — but for an image run, whose decision a region
cannot see past its edge: a region's render that reports where an image met
its run outside it grows the rectangle to hold that, and from the second round
every image a run could hold, and renders it again, until nothing is unseen.
Afterwards `previous` is the full render after the edit, pixels and warnings,
and `Patch::region` is the rectangle rendered. The page is rendered whole
instead, and `Patch::fallback` says why (`PatchFallback`), when the document
before the update has another page there (`OtherPage`); when the update
changed what the page draws besides its annotations — its dictionary but for
`/Annots` and `/Parent`, what it inherits, an object its content, its `/Group`
or its resources reach, or the optional-content configuration that shows its
layers (`ContentChanged`); when annotations the update left alone change
order (`Reordered`); when an annotation it changed, added or removed has an
appearance with no `/BBox` (`Unbounded`); when `previous` is not the page's
whole render at these options, or `options` names a region (`Mismatch`); and
when `previous` carries warnings, since a region's render raises only what its
part of the page raises (`PreviousWarned`). An update that changed no pixel of
the page leaves `previous` as it was. The rectangle comes from the
annotations' geometry, never from pixels: the bitmap is written into and never
read for what changed.

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
for them. **A list draws its own usage**: the recorded calls carry the
optional-content states of the usage the list was recorded for —
`Page::display_list`'s none, or the one `Page::display_list_for` names
(`DisplayList::usage`) — so a render naming that usage replays the list and
one naming another draws the direct way, through `Page::render`; and a list
recorded for a usage writes SVG by interpreting the page again, since SVG
draws no usage's states (`display_list.rs`'s
`a_list_draws_another_usage_the_direct_way`). **The warnings are the replay's own.** The kept resources are also
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

**The command stream** ([design](../design/command-stream.md)).
`DisplayList::command_stream` writes a list's page as versioned bytes a host
keeps and draws without the document, and `Page::command_stream` writes the
page's list's: every call the list holds, in `tinker-pdf-content`'s codec,
and the answer to every question a render of them asks the renderer's
`GlyphSource`, asked of the page's resources in the scope the renderer asks
it in — through resources of their own for each answer, so what each noted
(an outline's unresolved font, an image's damage, a space's repair) rides
with it. Every glyph shown in a mode that paints or clips has its outline
and its font's identity; every image drawn has its frame and a decode at
every reduction a draw can ask for (`Frame::reductions`: a JPEG's four
scales, a codestream's levels), under the intent in force at the draw; every
`sh` and every pattern a paint names has its definition, an ICC space by the
bytes of its profile, which `icc::Transforms` keeps, shared with whoever
handed them over, where it compiled from them; an image XObject's entry holds
beside its frame the bytes the writing document's budget leaves its decode
(`PageResources::image_room`), which a draw's reduction is read against; each tiling cell is interpreted once, as `PageResources::tile`
interprets it, into a stream of its own in the cell's resources or the
asking scope's, to the depth the renderer reaches; and each form that brings
resources and each appearance is a scope of its own, its record saying
whether it is a widget's, an appearance `/NoZoom` or `/NoRotate` places at
render time written placed as a render at magnification 1 places it. The
page's crop box,
rotation, group space, usage and index lead the stream. What the
interpretation met is written where a replay reaches it: a retained list's
after its content, as its renders note it, and an interpretation's — a list
that is not retained is interpreted into the writer, as `to_svg` writes it —
before the call it was met ahead of. The tables a stream holds answer every
question a render of its page asks, with the value the page's resources give
(`command_stream.rs`'s `the_stream_answers_every_question_a_render_asks`),
and a page written either way is one stream
(`a_page_interpreted_into_the_writer_writes_its_lists_stream`).

**A command stream drawn.** `CommandStream::parse` reads a stream under
budgets (`CommandBudgets`, the design's unless a caller gives others) and
refuses by name what the design refuses (`CommandStreamError`): beside the
codec's refusals, a page's stream with no page record, a page or appearance
record out of its place, a reference to what the stream has not defined, and
an image without a decode at each reduction a draw can ask for. Every tiling
cell's stream is read with it, under the one meter. `CommandStream::render`
replays the calls through the engine's renderer, through the pipeline a
page's render goes through (`draw_sheet`, over the stream's `Sheet`), drawn
whole on the caller whatever `parallelism` asks, with a `GlyphSource` over
the tables: each question is answered from the scope it is asked in, an
inline image by the first of the stream's with its bytes, an image's decode
at the reduction its frame gives the draw under the room its entry holds,
one the budget took past what the drawn size asks noted as a reduced image,
each answer's notes are said where a render reaches it, and each tiling cell
is drawn by replaying its stream at the request's geometry, in a scope of its
own where the cell brings resources. Every option applies as
`DisplayList::render` applies it — the content drawn or not, and the
appearances by their kind — except `usage`, `layers` and `magnification`: a
stream is the drawing of the usage and the configuration it was written for
(`CommandStream::usage`), its appearances placed as at magnification 1. What a
stream draws is what its page draws, in pixels, size, format and warnings:
every fingerprinted page at four scales with and without its annotations
(`determinism.rs`'s
`a_command_stream_replays_every_fingerprinted_page_byte_for_byte`), one page
under every option a render reads (`command_stream.rs`'s
`a_stream_draws_what_its_page_draws_under_every_option`), and the pages where
the renderer declines a group or a render meets a note only some renders
reach (`command_stream.rs`); each fingerprinted page's stream is pinned by
the hash of its bytes (`the_command_stream_is_the_same_bytes_on_every_target`).
`CommandStream::records` hands the records back, a kind this build does not
know kept by its number, `CommandStream::from_records` takes records back,
and `CommandStream::to_bytes` writes them at the stream's own version.

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

**The document's cache** (`crates/tinker-pdf/src/cache.rs`). A document
opened with a budget — `OpenOptions::cache_limits`, or
`Document::with_cache_limits` on an open one ([opening](opening.md)) — keeps
what its renders compute across renders, pages, clones of the `Document` and
the display lists of its pages, in one cache every one of them shares; with no
budget stated it keeps nothing past a render, as above. What it keeps is
**decoded images**: an image XObject's decode under its object, what it is
asked for — the reduction it is made at and the window it holds — and the
default colour spaces its scope paints device spaces in (8.6.5.6), each written exactly — a number by its bits, a name by its symbol,
a reference by its number — which is all of what outside the object a decode
reads; so two scopes naming one image under two names and the same defaults
share one entry, and two that paint it in different defaults keep two. What
it is asked for is what a render's own cache reads: every sample at full size
for every image but a JPEG or a JPEG 2000 codestream with no mask; for a JPEG
the smallest of an eighth, a quarter and a half that the size it is drawn at
allows, and for a codestream the levels it drops and the window it holds, each
read against its frame. The frame is kept beside the decodes, under the
object and the defaults, learnt by the image's first decode, which is made for
that draw and kept as its ask's; so an image drawn at two sizes keeps two
decodes and one frame, and each draw of it looks up its frame and then its
decode. Every
value is a function of its key and the document's bytes, so a render draws
the same pixels whatever was drawn before it, under any budget: a hit is the
bytes a miss computes. A refusal is kept as firmly as a picture, in its
codec's words, and an entry keeps what its decode reported — damage, and the
colour spaces it read with a repair — which every render that meets it reports
again, the image's own name as that render gives it (a soft mask's `SMask` as
it was). The decode is single-flight (`docs/design/parallelism.md`): renders
on several threads that reach one image at once decode it once, the others
waiting for its value, and a thread already inside a fill — of this cache or
of a `tinker_pdf_cos::Flights`, which `tinker_pdf_cos::fill_open` says —
decodes for itself and keeps nothing, so no thread waits on one that waits.
Each entry is charged its samples, its alpha and its two-colour bytes at the
decoded size, the words of what it reported and the bytes of its key's
defaults, and 64 bytes beside them, and a frame its key's defaults and 64
bytes; the bytes held never pass the budget,
because an entry is charged under the cache's lock before it is published.
Every lookup takes the next number of one counter and the entry it reaches
records it, and a fill that would pass the budget evicts the entries least
recently requested first, through one ordered index, until it fits; an entry
larger than the whole budget is used by the render that made it and not kept,
so a budget of zero keeps nothing and every draw decodes. **An image the
budget cannot hold at the size it is drawn is drawn smaller** rather than
refused: a JPEG or a JPEG 2000 codestream with no mask, whose decode at that
size — three bytes a sample, four for a codestream whose own opacity
`/SMaskInData` puts to use — would not fit the budget beside the rest of its
entry's charge, its 64 bytes and its key's defaults, is decoded at the least
reduction past that size whose decode fits: the largest of a JPEG's half,
quarter and eighth, or the fewest levels a codestream can drop, read off its
whole reduced image and never the window a canvas holds, so every tile and
band of a page draws from the page's one decode (ruling 5). It is kept under
that reduction, so a draw whose own size asks for it finds it and a hit is
that request's miss's bytes, and every render that draws the image so names
it, `RenderWarning::ReducedImage { name, factor }`, the reduction its decode
was made at. Which reduction is taken is a function of the image, the size
it is drawn at and the budget, never of what the cache holds. An image no
reduction fits is decoded at the size it is drawn at, as with no budget, and
not kept; one with a mask, one whose samples are palette indices and an
inline image are decoded as with no budget whatever it is; and with no budget
nothing is reduced. The maps are looked
up and never iterated, so on one thread which entries leave is a function of
the requests, the same on every run and every target. **A budget stated at
open holds the document's parsed objects beside the entries**
([opening](opening.md)): the cache keeps its entries in a
`tinker_pdf_cos::Accountant`'s ledger, which the object store charges too,
every read of a kept object takes a number from the same counter, and a fill
past the budget evicts whichever holding was requested longest ago, passing
over an object a caller holds; an entry that what no caller holds cannot make
room for is used by its render and not kept. `Stats::cache_bytes_held` and
`Stats::cache_evictions` count the cache's own entries. A budget stated on an
open document holds the entries alone. An inline image has no
object and stays the render's. **Compiled ICC transforms** are kept beside the
images under the profile's bytes (`docs/design/icc.md`, P-22, "The document
cache: its key and its charge"): their SHA-256 stands for them in the map and
the bytes themselves are compared in full on a hit, beside the destination,
sRGB, so a profile named on every page, in a form, in an appearance and by
every render, or embedded as many objects, compiles once while it fits, and
the transforms built are at most the distinct keys of a profile's bytes, its
destination and the conversion each transform is compiled for, the intent
among it, which the kept transforms key within themselves
(`icc_cache.rs`'s `transforms_built_are_at_most_the_distinct_keys`). The
bytes an ICC stream holds are kept beside them under the stream's object,
charged their length, so a stream is decoded and its bytes digested once a
document while that is kept. A profile that will not compile is refused under
its bytes and a stream that will not decode under its object, each once a
document, its refusal kept. A set of transforms is charged the profile's bytes
its key holds and the most it can come to hold
whichever intents are compiled after it is kept and whatever they convert
(`tinker_pdf_color::icc::Transforms::bytes_bound`): the profile's model, and
its tables and `D2B*` pipelines as read — one not read yet at the most reading
it can allocate — and each transform an intent compiles of its own with its
grid's every node filled and its largest row tables built: relative
colorimetric's, and one for each other intent at the most any of the
profile's sources compiles, a model's curves or the largest grid a table or a
pipeline is read through. So an intent compiled, a grid node filled or a row
converted later never takes the cache past its budget; past the budget each
use compiles its own, as with none, and draws the same page
(`under_a_budget_below_one_grid_every_page_is_the_same`). **A form's drawing**
is kept too: the interpreter asks for a recording of each form it enters and
offers each it makes ([content and text](content-and-text.md)), and the
document keeps it under the interpreter's `FormInvocation`, the usage the
render names — whose optional-content states its marked content is shown or
hidden by — and, for a form that brings no resources of its own, the scope
its names resolve in — the scope's resource dictionary written exactly, so
pages whose resources are one dictionary's worth share it. A form stamped on every page, invoked under
one state, is interpreted once while its drawing fits, and replayed after
that by every page, display list, tile and resolution, since a recording is a
function of the stream and the invoking state and never of the canvas (ruling
5); past the budget it is interpreted at every invocation, as it always was.
The renderer and the display list's recorder replay one only where their
admission would answer as it was recorded. A replay says again the fonts its
interpretation found none for; a drawing whose interpretation read a colour
space with a repair is not kept, since that repair is said among what the
renderer says, in an order a recording cannot keep. A drawing is charged its
recording as `Event::kept_bytes` counts it, its key's bytes and the words it
says again, and is never filled single-flight: it is made by the render that
draws the form, as it draws it, so two renders that miss one at once each
interpret it and the first to offer it keeps it. **Glyph masks** are kept
too, in place of each render's own cache of them (above): under P-06's key,
the font named as the document knows it across renders — its object, or a
font written into a scope's own dictionary by its id there and the scope's
resources written exactly — beside the code, the quantised 2x2, the phase,
the flattening tolerance and the render's anti-aliasing, so a hit is the mask
a miss rasterises whichever render asks. A mask is charged its pixels, its
key's resources and 64 bytes, is evicted with every other entry in the one
order, and, made by the render that draws the glyph, is never filled
single-flight; one evicted and drawn again is rasterised again to the same
bytes. **A tiling pattern's cell** is kept too (*Patterns*, above), charged
its pixels, the words it says again and its key's resources, and filled
single-flight, since a cell is a function of its key alone. `Page::render`, `Page::render_form`, `Page::render_annotation`,
`Page::display_list`'s recording, `DisplayList::render` and the page walk
read through it; `Page::to_svg` keeps what it computes for the write alone.

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

**Coverage levels and the hard-edge rules** ([design](../design/anti-aliasing.md)).
`RenderOptions::graphics_bits` and `RenderOptions::text_bits`, 8 by default,
are the bits of coverage each class keeps: 8 the anti-aliased coverage, 1 to 7
each pixel's coverage taken to the nearest of `2^bits` values
(`Mask::quantise`), 0 whole or empty by `RenderOptions::hard_edge` — `Half`, the
default, the threshold above; `Centre`, the pixels whose centres a shape holds
on the rasterizer's grid; `AnyPart`, every pixel a shape meets, a sliver
between sub-scanlines included. `antialias` keeps its meaning: off, both
levels read as 0 under the rule, so `antialias: false` with the default rule
is the render it was. Text is every glyph shown, however it is painted — from
its mask, whose key in the render's and the document's glyph caches holds the
level and the rule, as a path, stroked or through a pattern — and everything a
text object paints or clips: a Type 3 glyph's procedure, and the outlines the
clipping modes add at `ET`. A text object counts as `tinker_pdf_content::cull`
counts it, so none outlives its stream: one a form, a group or a mask's group
leaves open ends with it, and a restore ends every one opened since its save.
Graphics is everything else, and the renderer hands each site its class's
level and the rule: `Renderer::coverage`, a glyph's mask, an image's edge
(`ImageDraw::edge_bits` and `edge_rule`), a mesh's silhouette
(`MeshDraw::edge_bits` and `edge_rule`, the colours spread over the
silhouette before it is quantised) and a tiling cell's renderer
(`TileRequest`'s levels and rule, so text in a cell is text). Each level is
applied where the coverage is made, before the clip and the soft mask multiply
in, and a tile at every level and rule is the page under it. Under `AnyPart`
the renderer's scratch keeps each stroke's pieces, which the rule reads where
an edge passes; under every other rule a stroke holds its edges alone.

**The minimum line width.** 8.4.3.2's thinnest line is a floor in device
pixels of the page's frame for every stroke, at the level and rule of the
stroke's own class — a path's the graphics level, a stroked glyph's the text
level: 0.8, or 0.6 for a stroked glyph, at 8 bits; 1 at 1 to 7 bits and at 0
under `Half`; 65/64 under `Centre`; 1/256 under `AnyPart`. Each is the
narrowest width at which a zero-width line leaves no column it crosses (no
row, past one in one) empty — the design argues each — so the default and
`antialias: false` keep the floors they had. `RenderOptions::thinnest_line`
replaces the table for every stroke; a width that is not a finite number above
zero reads as none. A tiling cell's renderer takes the page's. Under stroke
adjustment the floor raises the width before it is made whole, so a floor
under a pixel and a half — every floor of the table — adjusts a stroke as
the asked width alone does, and a wider one draws as its nearest whole
pixels.

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
piece of code does both), and its viewport is its `/Rect`, placed where
`/NoZoom` and `/NoRotate` place it. `options.region`, if
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

**Page analysis** ([design](../design/page-analysis.md)). `Page::analysis`
and `DisplayList::analysis` take a `RenderOptions` and return a
`PageAnalysis`, read from what the page draws rather than from its pixels: a
page's analysis is its display list's, the list's events replayed into one
analysis device (`analysis.rs`) that paints nothing and answers the
interpreter's questions through the renderer's own `Admission`, and a list
that is not retained interprets the page into the same device, its
annotations drawn through it. Of the options, `usage` decides what optional
content hides, a list answering options of another usage by the page's list
for that usage as its render draws them the direct way; `annotations` decides
whether the appearances are part of the page; and `scale`, with the clamp
`Page::pixel_size` applies, the box in pixels; nothing else is read.
**`marks`** is the bounding box of every mark a render paints — a fill's path
by its exact bounds, a stroke's outline as the renderer's stroker writes it
under the pen, a glyph's outline from the face, an image's unit square, an
`sh` its clip and a mesh its points — each met with the clip's box, the soft
mask's box and the crop box; `None` for a page that paints nothing on itself.
Hidden content, a mode 3 or mode 7 glyph, a glyph no face draws, a mask
group's content, and a fill, an image or a shading of no area are no marks; a
white fill or one at `ca 0` is, since geometry decides and never colour, and
a stroke of no width is, at its path. `Marks` holds the box in points of
default user space and as the `PixelRegion` of the bitmap `Page::render`
returns for the options, rounded outward and at least a pixel a side.
**`transparency`** is `pdfa/colour.rs`'s reading of ISO 19005-1 6.4 taken
over what the page paints — its own `/Group` `/S /Transparency`, a form's
group, a soft mask in force at a paint, an `/SMask` on an image or form it
draws, a blend mode other than Normal or a constant alpha below one, less
`pdfa/colour.rs`'s tolerance, at a paint of that kind — with nothing hidden,
masked or never painted under counted. The two readings part at a `/BM` this
engine does not recognise, which draws as Normal (11.3.5) and which PDF/A-1
refuses as written. **`overprint`** (`Overprint`) says which paints are made
with overprint on, from the keys K-01 stores in every state: `fills`, a fill,
a filled glyph, an image or an `sh` made with `/op` in force — `/OP`'s value
where a state writes no `/op` (Table 58) — and `strokes`, a stroke or a
stroked glyph made with `/OP`; `nonzero_mode`, whether one of those was made
under `/OPM 1` (8.6.7.4); and `used()`, either of the first two. It reads the
state at the paints, under the same rules, and simulates nothing. **Colour or
grey** is the one question pixels decide, so it is asked of a render:
`Bitmap::greyness(&GreyThresholds)` reads every pixel as the colour
`Bitmap::to_png` writes for it — straightened when premultiplied, ink through
8.6.4.4's relation, Lab back to sRGB — and counts it coloured when the largest
of its red, green and blue less the smallest is more than `tolerance`; a pixel
whose alpha is zero holds nothing and is not. The page is grey when its
coloured pixels number at most `allowed`. The defaults are a tolerance of 8
levels of 255 and none allowed. `Greyness` holds the verdict, the coloured
pixels, the pixels read and the widest spread met, and is `None` exactly for
a bitmap that is not a picture; `Page::greyness` and `DisplayList::greyness`
render with the caller's options and read that render, so a hidden layer's
colour, and one an opaque grey paints over, are not counted.
`page_analysis.rs` holds each answer to a fixture whose
answer is known, `analysis.rs` holds the box to its own walk of the recorded
events, and `determinism.rs` pins every fingerprinted page's box.

## API

The facade is the whole public surface (ruling 11): `Page::render` takes a
`RenderOptions` — `scale` (pixels per point, or `RenderOptions::at_dpi`),
`magnification` (1; the viewer's zoom, which `/NoZoom` and a `/Zoom`
auto-state read),
`content`, `widgets` and `other_annotations` (each on; the parts drawn, below
`annotations`), `layers` (a `LayerOptions`; the optional-content
configuration drawn, `/D` by default, the groups the render turns on or off,
none by default, and the language and user its auto-state reads, none by
default), `format` (`PixelFormat`), `cancel` (an optional `CancelToken`, cloneable and
checked between operations and scanline bands), `annotations` (on by
default), `region` (an optional `PixelRegion`, `None` for the whole page) and
`allow_cmyk` (off; with `format: CmykA8`, hands the page back as ink rather than
light), `antialias` (on; off makes every pixel of every shape whole or
empty), `graphics_bits` and `text_bits` (8 each; the bits of coverage each
class keeps), `hard_edge` (`HardEdge::Half`; the rule at 0 bits),
`thinnest_line` (`None`; the thinnest stroke, in device pixels), `transparent` (off; on starts a page whose format has alpha with
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
render's cache held, or the document's under a stated budget, and
`shading_functions_evaluated`, every evaluation of a shading's function for a
colour — once a table entry where a sweep reads the function through a
`ColorTable`, once a colour asked of it otherwise — all counted by the
renderer, a tiling pattern's cell included, as are four
quantities: `group_buffer_pixels`, the pixels of every buffer a transparency
group or a soft mask's group allocated for its own pixels while it was open —
its canvas, and nothing else — and `group_bound_pixels`, the pixels of the
regions those groups opened over, once a group; and `soft_mask_pixels` and
`soft_mask_bound_pixels`, the pixels of every soft mask installed and of the
region of the group it was read from, once a mask; and a fifth,
`pixels_rendered`, the pixels of the canvas a render drew the page onto: the
page at its scale, a tile's region, the rectangle a form's or an annotation's
door draws, summed over the renders an operation makes, a cell's renderer
adding none;
`font_dictionaries_resolved`, each font a resource scope reads — once for
each name its content selects a font by, the first time the scope is asked
about the name, and kept for every render over the scope: a font a scope
names and never selects is not read, a glyph asks the font its scope read
whether it is a Type 3 font, and a substitute is asked for from the
dictionary read;
`images_decoded`, an image XObject's decode on a cache miss and every inline
image's; `image_colours_converted`, each colour an image's converter asked of
its space's conversion — a table entry filled, or a pixel no copy, table, row
path or remembered colour answered; `tiling_cells_rasterised`, each tiling
pattern's cell rasterised for a fill no kept cell answered; and a `_hits` and
a `_misses` for each of
the five caches a page's
resources keep — `font_program`, `image`, `icc_transform`, `glyph_outline` and
`form_scope`, an image the document's cache keeps counted as `image` and a
transform as `icc_transform` too;
`cache_hits` and `cache_misses`, each lookup of the document's cache that
found its entry kept and each that computed it, of every kind it keeps,
`cache_evictions`, each entry it evicted to make room for one the operation
kept, and `cache_bytes_held`, the most bytes it held at any of the
operation's lookups — all four zero for a document with no budget;
`forms_interpreted` and `forms_replayed`, each form drawn by interpreting its
content and each drawn from a kept drawing, counted only where the document's
cache can keep one — a budget, and not one of zero;
`page_walks`, each walk of the page tree from its root,
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
transforms output, `N × N` for each block a decode at `N/8` transformed;
`jpx_levels_synthesised`, `jpx_code_blocks_decoded` and
`jpx_levels_discarded`, the inverse-wavelet levels the JPEG 2000 decodes
synthesised, every tile-component summed, the code-blocks their tier-1
decoded and the resolution levels each dropped; from
`Bitmap::to_png_counted`, `png_image_scratch` and
`png_peak`, maxima of the bytes a PNG write held, as `cache_bytes_held` is a
maximum; `stream_seal_passes`, each
pass of the AES core that sealed a save's streams side by side, which only a
save that encrypts counts ([writing](writing.md)); and
`pool_items_on_workers` and `pool_items_on_caller`, where a page of a page walk
ran — on one of the pool's workers or on the caller — counted into the first
counted operation the page makes, its render or its counted text, and never
into a page rendered outside a walk; and where each band of a page drawn in
bands ran, counted into that render's one tally. Beside them a render's
**statistics** of what it drew ([page analysis](../design/page-analysis.md)),
counted by the renderer once a call and never once a pixel, so a
display-list replay and a render in bands count what a direct render
counts: `drawn_glyphs`, `drawn_fills`, `drawn_strokes`, `drawn_images`,
`drawn_shadings`, `drawn_groups` and `drawn_soft_masks`, the glyphs,
fills, strokes, images (a placeholder among them), `sh` calls, groups and
soft masks it painted or accepted outside hidden content; `drawn_clips`,
each clip it installed — a `W` or `W*` path, a form's or a mask group's
`/BBox`, and a text object's at `ET`; `hidden_calls`, each glyph, fill,
stroke, image, `sh` and group hidden optional content kept from painting;
and `decoded_images_drawn` and `decoded_image_samples`, each image drawn
from decoded samples and the samples it was decoded to, its width by its
height, at every draw — fewer than `/Width` by `/Height` for a JPEG
decoded at the size the page draws it, which is the one figure a scale
moves. A paint the clip leaves nothing of is drawn. A render in bands
counts each call once: every band replays the same calls and answers
them alike, and the first band's renderer counts them. A tiling
pattern's cell is a paint of the fill that names it, drawn once for the
page to repeat, and its own calls are not among the page's. Under forced
colours what a soft mask's group would paint is skipped, not drawn.
`Stats::work()` totals the step counts, which leaves out those maxima,
those quantities, the two pool counts — where a part ran is no step, so a
walked page totals what the page rendered alone does — and the
statistics, which classify calls whose steps are counted where they were
taken; two `Stats` add with `+`, a maximum keeping the larger. Every
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

The parameters a `gs` stores — black-point
compensation, overprint, flatness, smoothness, stroke adjustment, alpha
source, text knockout, the black generation, undercolour removal and
transfer functions, and the halftone and its origin — travel in the state as
one shared `StoredParameters` value with the rendering intent, which flat
colours, images and shadings apply, `None` while every one has its initial value, so a
`q` copies one word more and a copy allocates nothing. The transfer functions,
black generation and undercolour removal, flatness, smoothness and stroke
adjustment are applied
([design/device-dependent-state.md](../design/device-dependent-state.md)); the
rest move no pixel until their rows apply them.

**Flatness** (`/FL` and `i`, 10.7.2) is the tolerance each paint's curves are
flattened to, in device pixels, from the paint's own state: a fill, a stroke
(its path and the stroker's round joins and caps), a clip (at the paint that
ends its path, as a fill there would be) and a glyph's outline, filled,
stroked or clipped by — a text clip's glyphs each at the flatness they were
shown at, kept as curves and flattened together while they share one. Table
56's range bounds it: past 100 it is 100, and it is no finer than 1/256 of a
pixel, the grid the filler places a crossing on; 0, a negative value and none
set are the device's own 0.2, so a page that sets none keeps its bytes (Table
52's initial 1.0 is a tolerance 10.7.2 lets a processor ignore, and 0.2 is no
coarser). The flattener's rule is unchanged, only its tolerance is the state's
(`device_state.rs`'s `flatness_sets_the_tolerance_a_curve_is_flattened_to`,
`a_text_clip_flattens_each_glyph_at_its_own_flatness`,
`a_glyph_mask_is_kept_per_flatness`,
`a_flattened_tile_is_the_page_under_it`).

**Smoothness** (`/SM`, 10.7.3) bounds the colour error where the renderer
approximates a shading's colour, which is a Coons or tensor patch alone: a
patch's colour is bilinear over its parameters (8.7.4.5.7) and the grid of
triangles it is drawn as carries it linearly, so at `n` steps a side the grid
misses at most `|c₁ − c₂ + c₃ − c₄| / (4 n²)` of a component's range, at a
cell's centre. Under `/SM` a patch takes the larger of its device-length count
and the least power of two whose error is within the smoothness for every
component, at most 32, decided in 16.16 fixed point (`Mesh::tessellate`,
ruling 4). A component's range is the span its space converts within — 0 to
1, an `/Indexed` index's 0 to `hival`, Lab's L\* 0 to 100 and a\* and b\* their
`/Range` — and a patch carrying a parametric value is weighed by its
function's slope, sampled once a mesh at 256 intervals over the values its
patches' corners span. `/SM` is read in 0 to 1 and no finer than one level of
255; unset, or not a number, it is the device's own and a patch is cut by its
length alone, as before. Types 1 to 3 are evaluated at every pixel and types 4
and 5 interpolate their vertices as 8.7.4.5.5 defines them, so `/SM` moves
none of them; the count reads the patch and the page's transform alone, so a
tile cuts the page's mesh (`device_state.rs`'s
`smoothness_bounds_a_patch_s_colour_error`,
`smoothness_moves_no_shading_it_does_not_approximate`,
`a_smoothed_tile_is_the_page_under_it`).

**Stroke adjustment** (`/SA`, 10.7.5) turned on makes a stroke's device width
whole and puts its edges on pixel boundaries, for strokes and stroked glyphs
alike. Under a similarity — nearly every page — the width the stroke would
draw at, the asked width raised to the thinnest line, becomes the nearest
whole number of pixels, never under one, "no more than half a pixel
different" and a line under half a pixel "a single-pixel line"; every point
of the device path, vertices and control points alike, moves to the centre of
its pixel along each axis for an odd width and to the nearest pixel boundary
for an even one (`tinker_pdf_raster::whole_pixel_width` and
`adjusted_to_pixels`). So a thin line covers whole pixels across its width at
any offset, and copies of it at offsets within one pixel's reach paint the
same. Under any other map no one device width is the pen, so its width stays
the user-space pen's and each axis's points go to the grid of the stroke's
extent along it, the pen's raised to the thinnest line, made whole. The grid
is the page's device pixels, the frame a tile stands in (ruling 5); off,
`false` and unset, strokes draw as before (`device_state.rs`'s
`stroke_adjustment_gives_a_thin_line_one_width`,
`stroke_adjustment_rounds_the_thinnest_line` and
`an_adjusted_tile_is_the_page_under_it`, `render_regions.rs`'s
`stroke_adjusted_tiles_are_the_adjusted_page`).

**Transfer functions** (`/TR2` over `/TR`, 10.5) are kept per point, as
11.7.5.2 has them: each elementary object writes, where its shape is not zero,
its own transfer if it is fully opaque — its alpha constant 1, `Normal`, no
soft mask, an image without a soft mask, every enclosing group invoked the same
way, a tiling cell all of whose paints were — and the device's own if it is
not, into a one-byte plane over the page canvas (`DeviceParameters`) made at
the first paint that needs it; a soft mask's group writes nothing. The shape
is the coverage without the soft mask, an image's square through its `/Mask`
or stencil drawn once more by the sampler, `sh`'s geometry, a mesh's coverage,
a pattern fill where its cells painted. At the end of the render, once the page
is in its output format, each pixel's components pass through its entry's
functions sampled once to 256 entries: red, green and blue through the first
three, in either byte order; grey through the fourth; ink through all four in
additive form, `255 − T(255 − c)`, and black alone through the fourth where the
topmost object was DeviceGray. `/Identity` and `/Default` are the device's own,
and a page that sets no transfer keeps no plane and moves no byte
(`device_state.rs`'s `a_transfer_function_maps_each_component_through_its_curve`,
`a_transfer_function_is_the_topmost_fully_opaque_object_s`,
`a_device_gray_colour_on_ink_takes_the_grey_function_alone`,
`a_transferred_tile_is_the_page_under_it`). Past 255 distinct sets on one page
a further set is kept as the device's own and named,
`RenderWarning::DeviceParametersSpent`.

**Black generation and undercolour removal** (`/BG2` over `/BG`, `/UCR2` over
`/UCR`) apply only where DeviceRGB becomes DeviceCMYK (11.7.5.3): a colour of
DeviceRGB's family painted onto an ink buffer — the ink page, or a
`/DeviceCMYK` group's buffer on any render — with the functions in force at
the paint, and an RGB group composited onto an ink buffer, with those in force
at its `Do`. The colour's family travels in the state (`GraphicsState::fill_family`,
`stroke_family`: set by `g`, `rg` and `k`, by `cs` through
`FontSource::color_family`, and by `sc` read by count; an `/Indexed` space is its
base's, a `/Separation` its alternate's, a defaulted device space its
substitute's), on `DecodedImage::family` for an image and from its space for a
shading. The ink is 10.4.2.4's, `k = 255 − max(r, g, b)`, each ink
`clamp(255 − v − UCR[k])` and the black `clamp(BG[k])`, through the tables an ink
canvas holds while the paint lasts (`Canvas::set_ink_tables`), an uncoloured
tiling pattern's colour among them, which `Canvas::composite_tinted` takes into
ink through the tables in force at its blit; with one function
set the other is `k` itself, and with neither the conversion is maximum
undercolour removal, today's, byte for byte. A DeviceGray colour stays 10.4.2.3's
black alone and a DeviceCMYK one its own ink
(`black_generation_and_undercolour_removal_make_the_ink_the_test_computes`). The display list's
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

`Page::separations(&SeparationOptions)` returns the page's plates through the
same pipeline: `SeparationOptions` carries the `RenderOptions` and each
colorant's `PlateUse` by name, and `Separations` carries `width`, `height`,
`plates` (each a `Plate`: `name`, `kind`, a `PlateKind`, and `tints`, a byte a
pixel), `alpha` (with `transparent`), `warnings` and `stats` ("A page's
separations", above).

`Bitmap::to_png` writes the page out as a PNG file (ISO/IEC 15948), eight bits
a component, through `tinker_pdf_filters::png_encode` — where the zlib
stream, the chunk CRC-32 and 9.2's row filters a PNG is made of live — its
rows streamed through a zlib stream at the fast level into the file, so
writing holds no buffer the size of the image. `Bitmap::to_png_counted` is
the counted door: the file, and a `Stats` whose `png_image_scratch` is the
most bytes the write held at once in buffers sized from the image — none for
the four formats PNG has, the straightened copy of a premultiplied bitmap, the
copy of a `Bgr8` or `Bgra8` one reordered red first, the RGBA copy of a
`CmykA8` or `LabA8` one — and whose `png_peak` is the most held
at once with the file (guard A8, [performance](../design/performance.md)). It is
**total over all nine `PixelFormat`s**, which matters because a
page comes back in two of them and the fields are public: `Gray8`, `Rgb8`,
`GrayA8` and `Rgba8` are colour types 0, 2, 4 and 6 byte for byte, `Bgr8` and
`Bgra8` are colour types 2 and 6 written red first, so each file is the
`Rgb8` or `Rgba8` render's, and the two
PNG has no colour type for are converted rather than relabelled — `CmykA8`
through 8.6.4.4's device relation and `LabA8` back out of `L*a*b*`, both
keeping their alpha and both landing on type 6. Writing ink under a label
saying RGB is the failure `page_format` exists to prevent one layer up, and it
would be just as invisible here. `None` comes back only for a bitmap that is
not a picture: a zero dimension, a stride narrower than a row, a buffer
shorter than the rows the other fields promise, or one in `Plates`, which no
render returns — `page_format` answers `Rgba8` to a caller asking for it, as
it does for `CmykA8` without `allow_cmyk` and for `LabA8` — and whose spot
channels are tints of colorants whose colours a bitmap does not carry. `Page::render` produces a
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
condition, a `Plates` bitmap's among it — a map changes nothing and a
reduction answers `None`.
`scaled(width, height)` resizes a bitmap through the image sampler's area
filter, its spans planned in integers: every pixel the average of the
rectangle of the source it covers, each sample weighted by the area of it
covered and each byte rounded half up, at any ratio down or up — grown, a
pixel inside one source pixel is that pixel and one across a boundary the two
by their share — with no pyramid; every channel of every format takes the
same weights, colour is averaged premultiplied and handed back as the bitmap
holds it. `subsampled(x, y)` keeps the first pixel of each `x` by `y` block,
`ceil(width / x)` by `ceil(height / y)`, every byte as stored. Both return a
new bitmap with packed rows, the source's format, premultiplied flag and
warnings and no `stats`; a scale to the bitmap's own size and a subsample by
one are copies of it, stride and all, and each answers `None` for a bitmap
that is not a picture, a size or factor of zero, or a result whose bytes
cannot be allocated.
`converted(format, premultiplied)` re-expresses a bitmap in any of the eight
formats by the canvas's own per-pixel relation (`Canvas::extract`): each
pixel straightened where the bitmap is premultiplied, read as sRGB and alpha
as a canvas of its format reads it, and written as a canvas of `format` is
filled — grey by `Color::luma`, ink by 8.6.4.4's maximum-undercolour-removal
inverse, Lab through `L*a*b*`, the blue-first formats blue first — its alpha
carried, 255 where the source has none and dropped where `format` has none,
and premultiplied after where asked and `format` has an alpha. A same-format
conversion never goes through a colour, so an ink bitmap keeps its split: one
premultiplied as it already is is a copy, stride and all, and one that only
changes the premultiplied state straightens or premultiplies each byte where
it stands. Every other result has packed rows and the source's warnings and
no `stats`, and it is `None` for a bitmap that is not a picture, for plates,
which nothing converts into either, and for a result whose bytes cannot be
allocated. `to_png`'s straightened, red-first and RGBA copies are this
conversion.

**Forced colours** draw a page in two colours for high contrast:
`RenderOptions::forced_colours` takes a `ForcedColours` — a `background`, a
`foreground` and `fill_to_stroke` — and is `None` by default. The renderer
(`Renderer::with_forced_colours`) draws the page's coverage black on white
into grey: every fill, stroke and glyph paints opaque black by its own
coverage whatever its colour, ink or pattern, a stencil mask included; an
image, `sh` and a mesh paint the grey of their own lightness; and no constant
alpha, blend mode, soft mask, knockout or group colour space applies. It
answers the interpreter's questions as an ordinary render does — every group
opened and composited at full strength, every soft mask's group accepted,
drawn into nothing and never installed — so a display list replays into it
as into any render. The grey is then tinted into the format asked for by
`toolbox::tint_grey`, each channel `(foreground * (255 - v) + background * v +
127) / 255` for the black-on-white grey `v`, written as a canvas of the format
is filled and opaque, so every pixel is the background plus its coverage times
the difference, rounded once, and no transfer function is applied to it: the
page's colours are what forced colours replace. The page is opaque whatever
`transparent` asks.
With `fill_to_stroke`, each path fill paints the stroke of its own path at the
renderer's thinnest width, solid, under the same clip, with the caps and joins
in force — the bytes `0 w [] 0 d` and `S` paint — and a fill inside a text
object, which is a Type 3 glyph's, stays a fill. A tile of a forced-colour
page is its rectangle of the page (ruling 5).

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

`Renderer::snapshot` hands back what a render has drawn so far, as `finish`
would hand it back were the render to stop there, on copies, so a host
pausing a render (`docs/design/progressive-render.md`) can show it without
changing what it goes on to draw: the canvas copied, the run of abutting
images held back composited onto the copy and never onto the canvas, each
open transparency group closed onto a copy of what is under it innermost
first — backdrop out, knockout restore, the group's alpha, blend and soft
mask — and each open soft mask's group dropped, the page under it shown as
it stood at its `gs`. It walks them in the order they opened, which each
frame records as how many of the other kind were open when it opened — the
order `finish` closes any still open in — and a group's buffer holds what is
under it, so the copy holds every canvas the walk closes onto.

## Refused by name

| What | Typed variant | Why (one line) | See |
|---|---|---|---|
| An image codec not built in | `RenderWarning::UnsupportedImage` | A neutral placeholder is drawn and the codec named | [filters](filters.md) |
| An image decoded with damage tolerated | `RenderWarning::DamagedImage` | Drawn *and* named, so a damaged scan is distinguishable from a blank one (ruling 10) | [rulings](../rulings.md) |
| A JPEG or JPEG 2000 image whose decode at the size it is drawn would not fit the document's cache budget | `RenderWarning::ReducedImage { name, factor }` | Drawn from the least reduction past that size the budget holds — a JPEG at a half, a quarter or an eighth, a codestream with levels dropped — rather than refused, and named so a coarser picture is never taken for the image (ruling 10). Only under a stated budget; an image no reduction fits is drawn as with none | [opening](opening.md) |
| A page above 67.1 Mpx at the requested scale | `RenderWarning::PageScaledDown` | A whole page smaller beats a fragment at full size (ruling 2) | [rulings](../rulings.md) |
| A font program that could not be read | `RenderWarning::UnreadableFont` | Its glyphs are not drawn; the rest of the page is | [content and text](content-and-text.md) |
| A shading that cannot be read into types 1–7 | `RenderWarning::UnsupportedShading` | Reported by type number rather than guessed at | [rulings](../rulings.md) |
| A pattern whose cell cannot be read, or a lattice past 65 536 positions, a cell past 16.7 Mpx, or a fill past 33.5 million square points of compositing, the same at every scale | `RenderWarning::UnsupportedPattern` | Unpainted reads as missing; `/Pattern`'s nominal black reads as content and hides the gap | [rulings](../rulings.md) |
| A colour space whose parameters no space can mean, read the nearest way that does: a `/Lab` `/Range` pair written backwards, read as the span it covers; and a default colour space 8.6.5.6 does not allow — a `/Lab`, `/Indexed` or `/Pattern` one, one with another number of components than its device space, one that is no colour space — refused, the device space drawn as without it; and a JPEG's or JPEG 2000 image's `/ColorSpace` with another number of components than its samples, refused, the image drawn in the codec's own colour | `RenderWarning::RepairedColorSpace { name, reason }`, `reason` `LabRangeUnordered`; for a default, `name` the default's own (`DefaultRGB`) and `reason` `DefaultSpaceFamily`, `DefaultSpaceComponents` or `DefaultSpaceUnreadable`; for such an image, `name` the image's (`inline` for an inline one) and `reason` `ImageSpaceComponents`; once a render | Drawn *and* named (ruling 10). Named where the space is reached through the page's resources or a tiling cell's, as every tolerance the resource layer reports is; **not** through a form XObject's or an annotation appearance's own `/Resources`, which keep what they tolerate to themselves — a font a form's own resources cannot resolve goes unreported the same way | [rulings](../rulings.md) |
| Content, or an annotation by its `/OC`, in a layer the render's optional-content states turn off | `RenderWarning::HiddenOptionalContent` | Correct behaviour, reported all the same — the one leniency a reader cannot see | [rulings](../rulings.md) |
| A render naming an optional-content configuration the document does not have: an index past `/Configs` or 256 entries, or of an entry that is no dictionary | `RenderWarning::UnreadableLayerConfiguration { index }`, the page drawn in `/D`'s states | A page in `/D`'s states looks like a page in any other, so the caller is told its choice was not the one drawn (ruling 10) | [design/optional-content-config.md](../design/optional-content-config.md) |
| A render's override of a group the configuration's `/Locked` lists, or one turning on a group whose `/RBGroups` collection holds a locked group that is on | `RenderWarning::LockedLayer { layer }`, once a render, by the group's `/Name` or its reference; the locked group keeps its state | "The state of a locked group cannot be changed through the user interface" (Table 99), and a render's override is the caller's change; the page is drawn with the lock kept and the caller told which group refused (ruling 10) | [design/optional-content-config.md](../design/optional-content-config.md) |
| A form, a soft mask's group or a Type 3 glyph procedure that names itself while it runs, directly or through the others, drawn once | `RenderWarning::RecursiveContent`, and `SvgWarning::Render` of it from `to_svg` | A cycle has no last run; refused at its first repeat by the stream's identity, the page is what every reader can draw, where the depth cap ended a branching cycle only after two or more runs to the sixteenth | [content and text](content-and-text.md) |
| Nested runs past the interpreter's budget for nested content — 4 MiB, and 4 096 bytes for every byte of content the page reaches — past which no stream that has already run is run again | `RenderWarning::NestedBudgetSpent`, and `SvgWarning::Render` of it from `to_svg` | A tree of distinct forms or glyph procedures, each drawing the next many times, is no cycle and no group; every stream the page holds still runs once, so what is passed over is repeats of content already drawn | [content and text](content-and-text.md) |
| More than 2 000 transparency-group buffers on one page, or more than 2 000 groups and soft masks' groups run in one stream, past which the interpreter runs no more of either | `RenderWarning::GroupBudgetSpent`, and `SvgWarning::Render` of it from `to_svg` | A budget, not a depth: branching group and soft-mask recursion stays inside any depth cap, and every reading of the page — text, images, SVG — is bounded by the same walk | [rulings](../rulings.md) |
| A render on plates whose plates would pass a cap: a page naming more spots than `MAX_SPOT_PLATES` (64), or more than its plates keep inside `MAX_PLATE_BYTES` (1 GiB) at its size; a group or a coloured tiling cell whose plates buffer would take the plates live at once past `MAX_PLATE_BYTES` | `RenderWarning::PlatesCapped { named, kept }`, once a render: for the page, `named` spots asked for a plate and the first `kept` in inventory order kept one; for a group or a cell, `kept` 0 of the `named` spot plates the page carries | The spots past the cap are drawn as composites, through their alternates on the process plates, rather than refused (ruling 2), and named, since a spot drawn on the process plates looks like process ink (ruling 10) | [design/overprint-separations.md](../design/overprint-separations.md) |
| A text object that clips and shows no glyphs | `RenderWarning::EmptyTextClip` | Spec-correct and almost never intended | [content and text](content-and-text.md) |
| A render stopped by its `CancelToken` | `RenderWarning::Cancelled` | Reported only when work was actually skipped | — |
| A `RenderOptions::region` reaching past the page edge | `RenderWarning::RegionClamped` | The part on the page is rendered rather than refused (ruling 2), and a bitmap smaller than the rectangle asked for is named rather than left to arithmetic (ruling 10). A region that misses the page entirely trims to no pixels | [rulings](../rulings.md) |
| A PNG read back whose raster stops short of its declared height | `PngReadError::Incomplete`, carrying the decoder's own identifiers | The decoder degrades for a comic page; a file read back to be *compared* would have its missing rows scored as a rendering difference. Every refusal the decoder makes is `PngReadError::Refused` with its own reason | [filters](filters.md) |
| A form render naming an XObject the page does not have, one that is not a form, or one whose stream cannot be read | `RenderPartError::NoSuchXObject`, `NotAForm { subtype }`, `UnreadableForm` | The page renders what it can; a caller who asked for one form asked about that form, and a blank bitmap is a wrong answer that looks right | — |
| An annotation render at an index past `/Annots`, or of an entry that draws nothing in the render's usage | `RenderPartError::NoSuchAnnotation { count }`, `AnnotationNotDrawn { why }` with `NotDrawn::{NotADictionary, Hidden, Invisible, NoView, NotPrinted, OptionalContentOff, Popup, NoRect, NoAppearance, UnreadableAppearance, Degenerate}` | Every reason `Page::render` skips an annotation silently, named where a caller asked for that one | [document model](document-model.md) |
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
- Patching an edited page: `crates/tinker-pdf/tests/dirty_region.rs`'s
  `a_patched_render_is_the_full_render_after_every_annotation_edit` holds an
  annotation moved, resized, recoloured, removed and added — one whose
  appearance warns among them — on pages upright, turned every way and
  cropped, at 0.5x to 4x, and over ruling 5's image-run page, each patched
  over the page before the edit, to the full render after it, pixels and
  warnings; `a_patch_renders_only_its_rectangle` counts the rectangle's pixels
  and the grown rectangle's; the rest hold an edit no pixel shows and every
  `PatchFallback`. `tinker-pdf-render`'s
  `a_tile_reports_where_an_image_meets_a_run_outside_it` holds `RunReach` over
  the image-run page, and the tile grown to what it reports to the page's
  pixels.
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
- What groups and soft masks hold: `crates/tinker-pdf/tests/group_buffers.rs`.
  `a_soft_mask_is_kept_at_its_box` holds the committed transparency
  fixture's pale-backdrop luminosity mask at 150 dpi, whole and in each
  quarter of the page and a strip that misses it, to at most 1.1 times the
  pixels of its group's region, and that region to the box the page states
  clipped to the tile; `every_kind_of_soft_mask_is_kept_at_its_box` does the
  same for a pale, a black and an alpha mask at two scales;
  `group_buffers_hold_their_clipped_bounds_once` holds the buffers groups and
  soft masks' groups allocate to at most 1.1 times their regions, and the
  regions to the boxes the page states, over the transparency fixture and
  pages of a non-isolated group under a blend mode, an isolated and a
  non-isolated knockout group, groups nested three deep and a page group, at
  two scales, whole and in quarters; and each tile is the page's pixels under
  it. `tinker-pdf-render`'s `a_group_left_open_hands_back_the_page` finishes
  renderers inside groups and soft masks' groups, nested either way, and
  cancels one inside a group, each handing back the page.
- The document's cache: `crates/tinker-pdf/tests/caches.rs`.
  `a_second_render_at_another_dpi_decodes_no_image`, a page's two images
  decoded at 72 dpi and found at 150 dpi and by a tile at 300, the second
  render byte-equal to a document's with no budget;
  `pages_sharing_an_image_object_decode_it_once`, five pages and two names
  over one object, one decode;
  `a_jpeg_drawn_at_two_sizes_keeps_a_decode_at_each`, a JPEG decoded at an
  eighth at 72 dpi and at a quarter at 144, its frame looked up first and
  kept once, each render byte-equal to a document's with no budget, and a
  budget of zero decoding once a draw at the draw's own size;
  `eviction_follows_the_request_sequence_and_never_passes_the_budget`, three
  images under a budget of two scripted A, B, A, C, A, B, each render's hits,
  misses, evictions and bytes held pinned, the same on every run;
  `a_document_with_no_budget_keeps_nothing_past_its_render` and
  `a_budget_of_zero_decodes_an_image_at_every_draw`; a damaged fax and a
  refused codestream reported under each page's own name from one entry; one
  image in two scopes' defaults kept twice and in a third with the first's
  defaults found; clones, pages, display lists and the four-thread page walk
  sharing one cache, a new budget or new fonts starting another.
  `a_profile_named_on_every_page_compiles_once`, twenty pages naming an RGB and
  a CMYK profile, and a form naming both again, rendered twice under a budget,
  two transforms compiled where a document with no budget compiles four a
  render; a profile that will not parse refused once a document; and a profile
  and an image under a budget with room for the image alone, the bytes held
  never past it. `tinker-pdf-color`'s `transforms_hold_no_more_than_their_bound`
  holds a matrix/TRC, a grey, a three-intent, a v4 and a CMYK profile's
  transforms, an RGB profile's whose perceptual table takes four inputs, and
  one's carrying a matrix, three tables and four pipelines beside a media
  white that is not D50, every intent compiled and converting a colour and a
  row, within the bound charged for them.
  `a_form_stamped_on_a_hundred_pages_is_interpreted_once`, a stamp after each
  page's own text and path, with resources of its own and resolving its font
  in the pages', one interpretation and ninety-nine replays, every page
  byte-equal to a document's with no budget; a tile at another resolution
  replaying the page's drawing; a form whose missing font is said again on
  the pages that replay it beside one whose repaired space keeps it from
  being kept; display lists recording through the kept drawing; a form whose
  marked content a print render hides kept once for each usage, each render
  byte-equal to a document's with no budget; and no drawing kept with no
  budget, a budget of zero or a cancelled render.
  `glyph_masks_share_the_documents_budget`, a page of an image and two lines
  of three glyphs: under a roomy budget the masks kept beside the image and
  found by the second line and the second render; under a budget with room
  for the image alone, the image and the masks taking turns, the bytes held
  never past it, each render rasterising as the first and byte-equal to a
  document's with no budget. `tinker-pdf-render`'s
  `a_source_that_keeps_masks_holds_them_across_renders` holds the seam: a
  source that keeps masks is asked for every glyph and the render keeps none,
  a second render rasterises nothing, and a render at another text level
  asks by keys of its own.
  `determinism.rs`'s `warm_and_starved_caches_render_the_fingerprints` holds
  every fingerprinted page to its fingerprint and its warnings through a warm
  cache and a starved one ([determinism](determinism.md)). The
  machinery's own tests are `cache.rs`'s: the eviction order whatever the hash
  order, an entry past the budget used once, single-flight fills on four
  threads, a re-entry and crossed fills — across a `Flights` too — that
  compute rather than wait, and a fill that unwinds releasing its waiters.
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
  rectangle at three scales; every refusal by name, each reason a usage
  gives (`a_part_the_usage_hides_is_refused_by_name`) among them; and a
  fixed-seed campaign of mutated documents that must never panic either entry
  point.
- Usage and flags: `annotation_appearances.rs`'s
  `each_flag_draws_as_its_usage_says` holds `/Hidden`, `/Print`, `/NoView`,
  both, `/Print` clear, `/Invisible` on a known, an unknown and no subtype and
  an `/OC` on and off, in every usage, to the page with the annotation
  dropped or drawn plainly, directly and from a display list, the hidden layer
  reported; `no_zoom_and_no_rotate_keep_their_fixed_corner` holds `/NoZoom`,
  `/NoRotate` and both at magnifications 0.5, 1 and 2 on pages turned 0, 90,
  180 and 270 to an unflagged twin whose `/Matrix` and `/Rect` are the
  placement the clause gives, directly, from a display list and alone;
  `render_regions.rs`'s `a_no_zoom_page_tiles_as_the_page_does` tiles such a
  page at four sizes; `render_options.rs`'s
  `placed_annotation_output_is_pinned` pins one; `svg_output.rs`'s
  `to_svg_writes_the_annotations_the_screen_draws` holds the SVG writer's
  annotations to a twin; and `annots.rs`'s unit tests work the flag table and
  the placement by hand.
- Parts drawn apart: `render_parts.rs`'s
  `content_annotations_and_widgets_render_separately` holds a page of
  content, a markup annotation and two widgets under each combination of the
  three parts to the page with the others deleted, directly and from a
  display list, `annotations` off to no annotation of either kind, and each
  widget's print render to its view render, its `/AS` state's appearance
  drawn; `a_list_leaving_out_a_part_that_spent_the_group_budget_draws_the_direct_way`
  holds a list whose content spent the group budget, rendered without its
  content, to the direct render; and `render_options.rs`'s
  `the_default_is_unchanged` passes as it stands.
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
  `coverage_at_each_level_is_the_eight_bit_coverage_quantised` holds each
  element of the hard-edge page, alone in black, at every level from 1 to 8 of
  its own class to its 8-bit twin quantised in the test, the other class's
  level moving none of it; `each_rule_reaches_every_coverage_site` holds a
  path's fill and stroke, a clip, a glyph from its mask, a stroked glyph, a
  glyph clip, a Type 3 glyph, an image's edge, a mesh, a tiling cell and `sh`
  through a clip, each whole or empty under every rule, the any-part rule
  painting every pixel the other two paint and more, and each site's own class
  at 0 bits hard where the other's leaves it soft;
  `with_anti_aliasing_off_the_levels_read_as_zero` holds the switch's meaning;
  `a_hairline_at_the_minimum_width_drops_out_nowhere` sweeps zero-width lines
  across and down the page at every level and rule, each at its floor, for a
  column or a row left empty, and `levels_rules_and_widths_are_pinned` pins the
  hard-edge page at levels, rules and widths;
  and `render_regions.rs`'s
  `a_tile_at_every_level_and_rule_is_the_page_under_it` tiles every fixture at
  levels and rules apart and together. `tinker-pdf-render`'s
  `a_text_object_left_open_ends_with_its_save_and_its_stream` holds the text
  class's bookkeeping, and `glyph.rs`'s
  `a_glyph_mask_keeps_its_level_and_its_rule` a glyph's mask.
- Output: `png_output.rs` holds `Bitmap::to_png` over all eight formats, and
  `png_input.rs` holds `Bitmap::from_png` — the round trip over every page
  format, 16-bit rounding at the samples where truncation would differ, every
  decoded layout, both refusals, and a fixed-seed campaign of random and
  mutated files that must never panic and must reach both a picture and a
  refusal more than five hundred times each.
- The bitmap toolbox: `crates/tinker-pdf/tests/bitmap_toolbox.rs` holds every
  map and every reduction to its closed form over every reviewed raster
  golden, read through `golden_support`'s PPM reader, in each of the eight
  formats, with an alpha plane the test lays on, straight and premultiplied,
  computing each byte from the design's formula and the canvas's colour
  relations; gamma at exponents of a half, one and two, whose powers it
  computes exactly in integers. It holds a padded stride unread and unwritten
  and a bitmap that is not a picture refused whole.
  `a_scaled_render_is_the_area_average_the_test_computes` holds every variant
  of every golden scaled by halves, thirds, two thirds, five sevenths, double,
  one and a half, half across and double down, one pixel either side of its
  own size and to one pixel, every byte the average computed from its
  definition with rationals in `u128`;
  `a_subsampled_render_is_the_samples_the_test_takes` holds subsamples by
  factors that divide each golden and factors that do not, past its extent
  included; a scale to the same size returns the same bytes, a padded stride
  scales and subsamples as its packed twin, and the refusals refuse.
  `every_conversion_is_the_relation_the_test_computes` converts every variant
  of every golden to each of the eight formats, straight and premultiplied —
  all 56 ordered pairs and each format to itself — every byte computed from
  the canvas's relations; `a_format_converted_to_itself_is_the_same_bytes`
  holds the copies, packed and padded, and an ink bitmap premultiplied and
  straightened in place; `light_through_ink_and_back_is_exact` and
  `to_png_writes_what_conversion_gives` hold the round trip through ink and
  the PNG writer's conversions to the toolbox's;
  `the_toolbox_is_stable_across_targets` pins every operation's output over
  one golden on every target ([determinism](determinism.md)).
  `tinker-pdf-raster`'s `toolbox` tests hold gamma's table to its formula at
  other exponents, the toolbox's decoding and encoding of each format to the
  canvas's own, every conversion of a row of every byte pattern to
  `Canvas::extract`'s copy of it, and scale, subsample and conversion on
  cases worked by hand; its `image`
  tests hold `resample`'s planned lines to the rationals and every resampled
  pixel to an area average written from the definition.
- Forced colours: `render_options.rs`'s
  `forced_colours_are_coverage_over_the_two_colours` holds every pixel of a
  page of text, translucent and blended paint, a turned image, a knockout ink
  group and a soft mask, of a page of a mesh, `sh` and two pattern fills, and
  of a Type 3 page, in every page format and four pairs of colours, at two
  scales, with and without fill-to-stroke, to the tint of its coverage read
  from the black-on-white render, computed in the test;
  `forced_colours_apply_no_alpha_blend_mask_knockout_or_group_space` holds the
  black-on-white render to the ordinary grey render of a twin that asks for
  none of it; `a_pattern_fill_paints_its_shape_and_a_shading_its_lightness`
  does the same for patterns and shadings;
  `fill_to_stroke_draws_each_fill_as_its_stroke` holds a page of fills under a
  wide dashed pen to its twin stroking the same paths with `0 w [] 0 d`, at
  three scales, anti-aliased and not; `a_type3_glyph_stays_filled_under_fill_to_stroke`,
  `forced_colours_apply_no_transfer_function`, the opacity and premultiplied
  cases and a display list's forced render are
  held beside them, and `forced_colour_output_is_pinned` pins the forced page
  with and without fill-to-stroke on every target. `render_regions.rs`'s
  `a_forced_colour_tile_is_the_page_under_it` tiles every fixture and the
  annotated page in forced colours, directly and from a display list. The
  render crate's `forced_colour_tests` drive the device calls: each paint
  black whatever its state, fill-to-stroke as the thinnest stroke, a Type 3
  fill kept, and the questions answered as an ordinary render answers them.
- Snapshots: the render crate's
  `a_snapshot_changes_nothing_the_render_goes_on_to_draw` steps calls holding
  an abutting image run, a knockout group with a group in it and a soft mask
  being drawn with a group of its own one at a time, snapshots after every
  call, and holds the finished canvas and warnings to the calls never
  snapshotted and each snapshot taken with no bracket open to what the calls
  up to it finish with; `a_snapshot_shows_a_run_and_an_open_group_and_not_a_mask_being_drawn`
  holds what each shows, and
  `a_render_ended_inside_brackets_closes_them_as_a_snapshot_shows` holds the
  end of a render stopped after every call to the snapshot there.
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
  the tile — pinned exactly, so a change in either direction fails, its first
  page over one tile and its class of eleven pages at the guard's lattices and
  scales, with each class page's tiles drawn from a display list held to its
  direct tiles;
  plus the trim at the page edge and its warning, a region wholly off the page, and — because tile equality alone is
  satisfied by a *consistent* mistake — two fixtures that assert which quadrant
  of the displayed picture one mark lands in, on a rotated page and on a
  cropped one. Every fixture carries an ink floor, because a blank page tiles
  perfectly.
- In-crate: unit tests in `tinker-pdf-render/src/lib.rs` (among them
  `a_small_fill_on_a_large_page_stays_small`, which counts mask pixels asked
  for so an O(canvas) regression fails rather than merely costs, and
  `a_cancelled_clip_and_text_clip_rasterize_nothing`), `run_shadows.rs`
  (`a_run_reads_only_the_band_where_a_draw_meets_it`, which counts the pixels
  the run-membership question reads, and the test-only shadows RC-21 prices
  in the page's frame, `docs/design/image-edges.md`), `shading.rs`,
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
