# Rasterizer

`tinker-pdf-raster` is a pure 2D scanline rasterizer with zero PDF knowledge
([ruling 8](../rulings.md)): paths, pixels and plain parameters in, coverage
and pixels out, with no COS type or spec vocabulary in its public API. Its
output is deterministic by construction — fixed-point coverage accumulation,
integer blending, and no platform transcendental on any pixel path — so the
same input produces bit-identical pixels on every supported target
([ruling 4](../rulings.md), [determinism](determinism.md)). Every paint costs
what it covers, not what the page measures, and a long operation stops
mid-sweep when it is cancelled.

## What it does

**Paths and flattening.** A `Path` is a sequence of `Verb`s over `f64`
points: `MoveTo`, `LineTo`, `QuadTo`, `CurveTo`, `Close`. Quadratics are
first class — every TrueType glyph outline is quadratic, and raising them to
cubics would run on the hottest path in the engine for a control point the
curve does not have. The quadratic arm of the flattener takes its step count
from the cubic the curve is exactly equal to, so the same curve breaks into
the same pieces whichever verb describes it, and a typeface that mixes
`glyf` and `CFF` outlines keeps one smoothness character. Flattening
subdivides by a fixed count derived from the control polygon — never by a
floating-point termination test, which is exactly the class of cross-target
divergence ruling 4 exists to forbid. Single-point subpaths are kept, because
stroking draws them as dots under a round or square cap (8.4.3.3). Non-finite
points are dropped at construction, so nothing downstream ever sees a NaN.

**Offsetting an outline.** `Path::offset(horizontal, vertical)` moves every
contour outward by one amount across and another down, stem darkening's
offset ([design](../design/hinting.md#stem-darkening)): each point, on and off
the curve alike, moves along the bisector of its two edges' outward normals
by their miter vector, `x` scaled by the one amount and `y` by the other, so
an axis-aligned rectangle grows by exactly twice each. Outward is the side
that grows the path's total signed area, so a contour wound with the total
grows and a counter wound against it shrinks, in either winding. The miter is
held at `OFFSET_MITER_LIMIT`, four times the amount; a turn straight back and
a move that is not finite keep their points, and the points of a zero-length
edge move as one vertex; a subpath left open is offset as the closed contour a
fill makes of it. It uses `sqrt` and the four operations alone (ruling 4). No
render calls it.

**Filling.** `fill` rasterizes a path into a coverage `Mask` under the
non-zero winding (8.5.3.3.2) or even-odd (8.5.3.3.3) rule. Coverage is
accumulated at sixteen sub-scanlines per pixel row with exact horizontal
spans in 1/256-pixel fixed point, entirely in integer arithmetic — the
choice ruling 4 pays for over analytic exact-area coverage. The fill visits
only the rows a shape reaches, so a glyph-tall fill on a page-tall region
does the shape's work, not the paper's.

*Nothing is sorted* ([design](../design/rasterizer-accumulation.md)). A row
of the region is a line of cells, one per pixel, each with a lane per
sub-scanline; every edge reaching the row deposits its crossings into the
lanes by position, and the row is walked left to right carrying each lane's
winding — a prefix sum — turning each cell into its byte as it passes. A
pixel's coverage on a sub-scanline is the count of its 1/256 units the rule
calls inside, which the crossings decide whatever order they come in, so no
edge is ordered by its top and no sub-scanline's crossings by `x`. A lane
holding several crossings in one pixel has them bucketed by their offset in
the pixel rather than compared, so a crowded pixel costs its crossings and no
more. *A shape's interior is one write a run*: the row keeps a bit for each
cell holding a crossing, and the walk visits only those, writing the run of
pixels between two of them — where no lane's winding changes — as one slice
fill of `min(16 × lanes inside, 255)`, and nothing at all where no lane is
inside. A run ends at the first cell holding a crossing on any lane, so a
pixel that reaches 255 only by the clamp still goes through its cell. An edge
waits in a list of the row it begins in and leaves the active
set when it ends, and a cancellation is asked before a row's deposit, so a
stop leaves no row half converted.

*The loops a pixel runs are written for the vectoriser*: fixed-width integer
loops over arrays and slices, bounds checks outside them and no call inside,
so the `+simd128` wasm build vectorises them and every other build runs the
same code. A touched cell's lanes that hold one crossing are taken all
sixteen at once, one loop of `i32` with no branch — the lane's bit a
multiplier on its crossing's winding, and the rule a mask on the winding,
every bit for non-zero and the lowest for even-odd — and a lane holding
several is bucketed after them. An edge none of whose crossings can saturate,
which `build_edges` decides once from its position, slope and height, steps
from one sub-scanline's crossing to the next by one add; any other keeps the
saturating product. Integer lanes give the same bytes vectorised or not, so
none of this moves a pixel.

*Memory.* A fill works in a `FillScratch` the caller owns — the flattened
points, the edges and their row lists, the active set, one row of cells
(forty bytes a pixel of the region's width) with the crowded lanes' later
crossings, and the buffers of a row that does not close — grown when a fill
needs more and never shrunk, so after the largest fill a renderer has made the
only allocation a fill makes is the mask it returns. The flattener writes
every subpath into one reused buffer rather than a vector each. `fill` is a
fill through a scratch of its own; `tinker-pdf-render`'s renderer holds one and
fills every path, clip, glyph, image quad and mesh through it, on the page,
in a group's or a soft mask's buffer and in a tile alike, while a tiling
cell, drawn by a renderer of its own, has its own. A scratch also holds the
last stroke's outline, as edges, apart from a path's (*Stroking*, below).
Nothing left in a scratch reaches a pixel: a fill empties what it reuses
before it uses it, a stroke empties the outline before it writes one, every
cell is cleared as it is converted, and a stop comes before a row's deposit,
so the cells are clear between fills however the last one ended.

A sub-scanline closes — its windings sum
to zero — wherever every edge of a closed subpath reaches it, so the only row
that does not is one where an edge was dropped — a point past the finite or
too large to take sixteen times, a slope that is not finite, a height under
`f64::EPSILON` across a sub-scanline; such a row is
swept as a sorted sweep sweeps it, crossings ordered by position, then the
edge's top, then its place in the path, and that is the one comparison a
fill can make.

*Every subpath is closed before it is filled* (8.5.3.1), and before it clips
(8.5.4), which reaches the same `fill`: the segment from a polyline's last point
back to its first is an edge whether or not the path said `h`.
`an_open_subpath_is_closed_before_it_is_filled_or_clipped` holds it by
half-plane arithmetic over `f`, `f*`, `W n` and two subpaths.

*What the fixed grid costs.* Sixteen sub-scanlines quantise an edge's
vertical position to a sixteenth of a pixel, so against the true area of a
half-plane a pixel's coverage is not exact to one 8-bit level: the error is
largest at edges near horizontal and smallest near vertical. A fixed grid is
what makes ruling 4's bit-identical output possible at all, and a
floating-point area integrator would not be; `SAMPLES` is the knob that buys
quality back at a linear cost and at the price of re-baselining every
fingerprint in the tree.

*An edge carries sixteen more bits than the spans do.* An edge's slope is
multiplied by the height of the edge and its position is not, so the slope
needs far more precision: held at a whole 1/256 unit per sub-scanline, every
slope shallower than one pixel of `x` per sixteen of `y` would truncate to
zero and the edge would be drawn vertical. `SUBSTEP_BITS` puts sixteen bits
under the 1/256 unit, which leaves a page-tall edge under a thousandth of a
level from the line it states. The reduction back to 1/256 takes the
**nearest** unit — `+ half, >> bits`, a `floor` of a shifted value — rather
than truncating toward zero, which is a different operation on the two sides
of the origin and would quantise a shape differently once it moved across
one. That is the property a tile rests on (ruling 5), and it is also simply
the closer answer: a span is linear in the crossing, so the nearest unit is
the one whose span is nearest the geometry's.

**Stroking.** `stroke` expands a path under a `StrokeStyle` pen into an
outline filled with the non-zero rule, so a stroked edge anti-aliases
identically to a filled one. Caps are butt, round and square (8.4.3.3,
Table 54); joins are miter with the 8.4.3.5 limit, round and bevel (8.4.3.4,
Table 55); dashes follow 8.4.3.6, with an empty or zero-sum array meaning a
solid line, and a width of zero drawing the thinnest device line — one
pixel (8.4.3.2). A point repeated has no direction to join or cap along,
so repeats are dropped before the joins are read — the close of a circle
of four Béziers and an `h` ends on its start point twice, and would
otherwise lose the join at the start.

*The outline goes straight into the fill.* Every piece of an outline — a
segment's quad, a join, a cap — is written through one sink, which fixes its
orientation (after mapping it, for a pen in another space) and hands it to a
target: a `Path` for a caller that wants the outline (`stroke`,
`stroke_mapped`), or a `FillScratch` for one that wants its coverage
(`stroke_with`, `stroke_mapped_with`), where the piece becomes the fill's
edges as it is made. `fill_outline` then sweeps those edges over the region
the caller gives — `FillScratch::outline_bounds` gives the bounds the
outline's path would have — so the path is flattened once, by the stroker,
and the outline never is; the mask is the one `fill` gives the outline's
path, byte for byte, because the edges are the ones filling that path makes,
in the same order. A piece is an array of at most sixteen points (a circle's
`ARC_STEPS`), so mapping and turning it over allocate nothing. The outline
counts the verbs its pieces would have been in a path and stops at
`MAX_VERBS` where the path would; it stays in the scratch until the next
stroke, whatever paths are filled through the scratch meanwhile.
`tinker-pdf-render` strokes paths, stroked glyphs and pattern strokes this
way, and the SVG writer calls only `dash`, so no outline is built as a path
on the way to a page.

*Dashes are outlined as they are cut.* The walk cuts each flattened segment
into the dash in hand, one buffer the stroke keeps for all its subpaths, and
hands it to the outliner the moment it ends, then clears it; so a stroke
holds one piece at a time however many it cuts, and once the buffer is as
long as the longest piece no piece allocates — the outliner drops repeated
points in place, and a pen in another space carries the piece to the device
for its floor in a second buffer kept the same way. The stop predicate is
asked every 1 024 steps as before, and a stop keeps every piece already
outlined and the one in hand.

**A pen in another space.** `stroke_mapped` is 8.4.3.2 read literally for
a map that is not a similarity: the path, the width and the dashes are in
the pen's own space, the outline is built there exactly as `stroke` builds
one, and each piece is carried through the map before its orientation is
fixed — so a reflecting map cannot wind the pieces against each other.
Under `scale(1, 3)` a pen two units wide is six device units across a line
along `x` and two across one along `y`, and a dash cut square in the pen's
space is cut along the shear on the device. Its `floor` is the device's:
wherever the mapped pen is thinner than it, the same dashed pieces are also
mapped first and stroked at the floor, and the union is at least that wide
every way. Its curves are flattened in the pen's space at the device
tolerance over the map's largest stretch, honoured however small —
`flatten`'s floor of a millionth guards a *device* tolerance and does not
apply in the pen's space; `dash` flattens at its caller's tolerance the same
way. `stretches` gives a map's two singular values, which is how a
caller tells a similarity from anything else, and `dash` hands a caller
the pieces a pattern leaves without stroking them — one at a time, as each
is cut, and stops cutting when the caller answers `false`: the 100 000-step
bound is per segment, so forty segments under `[0.01 0.01]` are two million
pieces, and a caller writing them somewhere bounded pays only for what it
keeps. A pattern whose every dash has no length — `[0 0.01]` — leaves no
piece, and the walk answers that without stepping along the path
(`dashes_of_no_length_cut_nothing_without_walking_the_line`).
A dash of no length among dashes with length is taken out before the walk
and the gaps either side of it joined (`without_empty_dashes`; an odd pattern
is written out twice first, as the walk reads it, and turned to start at its
first dash with length, its phase moved to match), which leaves every piece
where it was and costs the walk no step, so a pattern padded with dashes of
no length writes what it writes without them, at the same cost
(`a_dash_of_no_length_costs_no_step_of_the_walk`).

**Stroke adjustment.** Two pieces of 10.7.5, without its vocabulary:
`whole_pixel_width` makes a device line width whole — the nearest whole
number of pixels, never under one, and one for a width that is not a finite
number — and `adjusted_to_pixels` moves every point of a path, vertices and
control points alike, to the grid that puts the edges of a stroke that many
pixels across on pixel boundaries: along an axis on which it is odd, to the
centre of the pixel the point is in (`floor(v) + 0.5`), along one on which it
is even, to the nearest pixel boundary (`floor(v + 0.5)`). No point moves by
more than half a pixel, and a path moved by whole pixels lands moved by the
same, so copies of a line at offsets inside one pixel's reach land on one
place and a thin line stroked at its whole width covers whole pixels across
it (`an_adjusted_width_is_the_nearest_whole_pixel_and_never_under_one`,
`every_point_goes_to_the_grid_its_axis_asks_for`,
`an_adjusted_thin_line_covers_whole_pixels_across_its_width`).

**Hard edges.** `Mask::harden` turns a coverage mask into one that is whole or
empty at every pixel: at least half becomes 255, less becomes 0. It is what
turning anti-aliasing off means one layer up ([rendering](rendering.md)). A
mask value is `floor(units / 16)` of the 4 096 units a pixel holds,
so it is at least 128 exactly when the shape holds half the units; two shapes
that split a pixel's units between them split the pixel too, except at an
exact half, where both take it — which is the property
`hardened_shapes_sharing_an_edge_partition_its_pixels` holds, and the reason
the threshold is a half and not anything above it.

*The rules of a hard edge.* `HardEdge` names the rule a fill with no
anti-aliasing paints a pixel whole by ([design](../design/anti-aliasing.md)):
`Half`, the default, is the coverage hardened; `Centre` paints a pixel whose
centre the shape holds, read on the filler's own grid — the centre of pixel
`(col, row)` is unit `256·col + 128` of sub-scanline `16·row + 8`, and the
pixel is whole when the winding the fill rule reads there calls it inside. A
rule that reads the shape rather than its coverage is the filler's, asked of it
by `fill_hard`, `fill_hard_with` and `fill_outline_hard`, which give a mask
whole or empty everywhere; `fill`, `fill_with` and `fill_outline` keep the
coverage. Under `Centre` a sweep deposits each row's middle sub-scanline alone,
one lane of the cells, and walks the cells carrying that lane's winding: a
pixel's centre takes the crossings left of it and those of its own cell at an
offset up to 128, a sum, so nothing is ordered, and the runs between cells are
written at once. A crossing at a unit begins or ends a span that holds that
unit, so an edge through a centre gives it to the shape on the edge's right or
below, and two shapes sharing an edge split its pixels; a shape narrower than
a pixel between two centres paints nothing. A row whose middle sub-scanline
does not close is read as the sorted sweep reads it, the span left open adding
nothing. One sample in the page's device pixels decides a pixel, and a
region's left edge is a whole pixel, so a tile under the rule is the page under
it.

`AnyPart` paints a pixel the shape meets: its region and its outline, every
crossing across to the nearest 1/256, with a point inside the pixel's open
square. On the grid that is two halves. The samples: a sweep under the rule
deposits every sub-scanline, as the coverage does, and a cell is whole where
its total of units inside is above zero — a pixel holding one to fifteen units
has a coverage of 0 and still meets the shape — the walk compiled for the rule
apart from the coverage's, so the coverage's costs what it did. The edges:
each segment of the flattened outline, closed, horizontal and short ones
included and a subpath of one point a segment of no length, over each open
row `(r, r + 1)` it reaches, paints the pixels whose open interval of units
`(256·col, 256·col + 256)` the extent of its part in the row reaches, both
ends across to the nearest 1/256 — which is what a sliver lying wholly
between two sub-scanlines is made of, and what the samples cannot see. A
shape that only touches a pixel's edge or corner leaves it empty, so a
rectangle on whole pixels paints its own pixels and no more, and a point
paints the pixel under it unless it lies on a pixel's edge. Both halves read
device pixels, so a tile under the rule is the page under it; a stopped fill
keeps the rows above the cut, both halves. A path's outline is the points
the fill flattened; a stroke's is its pieces, which a scratch keeps beside
their edges only after `FillScratch::keep_outline_points(true)`, so a stroke
filled any other way holds and costs what it did, and `fill_outline_hard` on
an outline written without them reads its samples alone.

`ImageDraw::edge_rule` and `MeshDraw::edge_rule` are the rule an image's unit
square and a mesh's silhouette are drawn by at 0 bits; the renderer passes its
level and rule to both ([rendering](rendering.md)).

**Coverage levels.** `Mask::quantise(bits)` reduces a mask to a number of
bits of coverage ([design](../design/anti-aliasing.md)): 8 leaves it, 1 to 7
take each pixel to the nearest of `2^bits` values spaced evenly from 0 to 255
— with `n = 2^bits − 1`, the index `(c·n + 127) / 255` and its value
`(q·255 + (n − 1) / 2) / n`, in integers and with no tie, since `n` is odd —
and 0 is `harden`; one bit is `harden` too, and a number above 8 reads as 8.
At two bits a pixel is 0, 85, 170 or 255. It is a function of one pixel's
coverage, so a tile and the page under it agree (ruling 5).
`ImageDraw::edge_bits`, 8 by default, is the level an image's unit square is
quantised at before the samples are read, so an image's edge quantises as a
path's does; the renderer passes the level of the class the image is drawn
in, 0 with anti-aliasing off.
`MeshDraw::edge_bits` is the same for a mesh's silhouette, applied after the
colours are spread over it, so they reach the pixels they reach anti-aliased.

**Coverage a stripe at a time.** `fill_stripes` measures a shape for a
liquid-crystal panel's red, green and blue stripes
([design](../design/lcd-text.md)). A stripe is a third of a pixel wide —
pixel `p` holds stripes `3p` to `3p + 2` of the page's frame — and a shape's
stripes are `fill` itself, of the path mapped by `(3x, y)`, under its rule and
tolerance, over stripe columns as though each were a pixel, so a stripe's
coverage is the grid's. The fill covers the region widened by one pixel each
side, and five integer taps, `[1, 2, 3, 2, 1]` divided by nine and rounded,
filter each stripe from the two either side of it into a `StripeMask`: three
coverages a pixel, kept red, green, blue whatever `SubpixelOrder` the panel
has — `Rgb` reads a pixel's stripes left to right, `Bgr` right to left. The
taps are a pixel's average taken twice, so a run of equal stripes filters to
itself and a horizontal edge is its grey coverage in all three channels. The
widening is geometry in the page frame, never what a canvas holds, so a
tile's edge pixel reads the stripes the page reads for it (ruling 5). A
region whose stripe columns would not fit the fill's coordinates holds no
coverage.

**Clipping.** A clip is a `Mask` like any other, and a clip stack composes by
multiplication: `Mask::intersect_in_place` multiplies coverages, which is
what keeps a clipped anti-aliased edge looking right rather than doubly
hard. **A mask reads `Mask::outside` beyond its rectangle**: zero for every
clip, every paint's coverage and every mask `Canvas::to_mask` returns, and
for a soft mask the luminosity of `/BC` alone, the answer outside the region
its group covered (11.6.5.2) — which is not zero unless `/BC` is black. So a
pale soft mask is its group's box and one value, never a field the size of the
canvas. Every consumer reads it: `Mask::at` returns it; `Mask::overlap_at`
reaches the whole canvas for a mask whose outside value is not zero; the
product of two masks reads the product of their outside values, over the
rectangle the two share when both read zero outside, over the one that reads
zero's own when only one does — a clip times a soft mask — and over the
smallest rectangle holding both when neither does; `intersect_in_place` keeps
its receiver's rectangle, a paint's coverage reading zero outside; `harden`
hardens the outside value too, and `quantise` reduces it to its level;
`Fragments::would_overlap` walks a run's whole
rectangle under such a mask; and the opaque composite, which reads a mask's
rows as slices, is never taken for one. `Canvas::to_mask` reads a buffer back
row by row in integer, each format's bytes as `Canvas::pixel` and
`Color::luminosity` read them — a grey is its own luminosity, RGB takes the
weights directly, a blue-first buffer the same weights over its bytes in
their order, ink goes through `cmyk_to_rgb` — and a `LabA8` buffer through
`lab_to_rgb`, as `pixel` does.

**Compositing.** A `Canvas` stores pixels in one of nine formats (`Gray8`,
`GrayA8`, `Rgb8`, `Rgba8`, `Bgr8`, `Bgra8`, `CmykA8`, `LabA8` and `Plates`)
and composites in integer arithmetic throughout. `Bgr8` and `Bgra8` are
`Rgb8`'s and `Rgba8`'s light blue first, the orders a host's surfaces store:
a page composites red first and is reordered once at the end, and a canvas
made in a blue-first order composites right too — the separable modes are
per channel, and `blend` hands the non-separable ones, which reason about
red, green and blue, the channels in that order
(`a_bgr_canvas_composites_as_the_rgb_canvas_reordered`, every mode). `CmykA8`
is a **subtractive** format, as `Plates` is below — its components are
quantities of ink — and it exists for transparency groups that declare
`/DeviceCMYK`, whose blends the specification says happen over ink rather than
over light ([rendering](rendering.md)). A page comes back in it only when a
caller asks for it twice (`RenderOptions::allow_cmyk`). The two device relations of 8.6.4.4 live here rather than in
`tinker-pdf-color`, for the reason `Color::luma`'s coefficients do — a
rasterizer turns stored components into light, and a leaf takes bytes and plain
values in; a test holds the copy to the original so the two cannot drift. The
inverse takes maximum undercolour removal, which makes it **exact**: every one
of the sixteen million colours survives the round trip. A caller may put black
generation and undercolour removal in force on an ink canvas instead
(`Canvas::set_ink_tables` with `InkTables`, two tables over the nominal black),
and it then turns light into ink by 10.4.2.4's formula wherever it encodes
light, until the device's own is put back.
`fill_mask_with` blends a colour through a mask under any of the sixteen
`BlendMode`s — the twelve separable modes of 11.3.5.2 and the four
non-separable ones of 11.3.5.3 — with the whole operation scaled by an
alpha, which is what the graphics state's `ca` and `CA` do (8.6.4.4).
`fill_mask_inked` is the same with the colour's own ink beside it (and
`blend_pixel_inked` for a caller that samples — a shading, a mesh and, by its
level, every image pixel, which is how `ImageDraw::ink` reaches the canvas for
a tinted stencil, directly or through a run's
`Fragments::composite_region_inked`): a
`CmykA8` canvas composites those four bytes rather than the colour turned
back into ink, which matters because light has one ink for each colour and
a document's rich black is four; every other format ignores them.
`Canvas::composite` blits one canvas onto another (11.3.6), bounded by the
source's rectangle. A transparency group's buffer holds the canvas it will be
composited onto while the group is open, moved in rather than copied:
`adopt_backdrop` for a non-isolated group (11.4.4), which composites its
backdrop's colour in from it and reads its alpha there at every pixel a paint
composites, and `hold_parent` for an isolated one, which reads nothing of it;
`release_parent` hands it back, `remove_backdrop` takes 11.4.7.2's backdrop
out reading the same pixels again, and `knock_out` puts 11.4.5's initial state
back — transparent, or the parent's colour at no alpha — with no snapshot of
it kept. Each read is at the same device pixel of the parent, in its own
format where it shares the buffer's, so an ink backdrop is the document's ink.

*Plates.* `PixelFormat::Plates { spots }` is the plates a press prints: cyan,
magenta, yellow and black, then `spots` spot channels, then alpha, `5 + spots`
bytes a pixel, every channel a tint — a quantity of ink, subtractive as
`CmykA8`'s are. `Spots::new` holds the count to `MAX_SPOT_PLATES`, sixty-four,
and refuses one past it by name (`TooManySpots`, which `Canvas::plates`
returns), so a canvas is allocated for the spots it is asked for and never
past the cap. A paint is a `PlateInk`, a tint for each channel by its index —
cyan 0 to black 3 and spot `i` at `4 + i`; a colorant reaches this crate as an
index and never as a name (ruling 8) — and every channel it names no tint for
holds no ink, so `fill_mask_plates` under `Normal` knocks those channels out.
Each channel composites by `CmykA8`'s arithmetic, line for line: complemented
through the separable blend functions, 11.3.6's straight-alpha composite, the
coverage and the alpha multiplied; so four process channels with no spot are
a `CmykA8` canvas's bytes for the same masks, inks, alphas and modes, the
non-separable ones taken through light as on `CmykA8` and counted in
`approximated_blends`. A spot channel blends only under a separable mode that
preserves white (11.7.3, 11.7.4.2), as a process channel holding its tints
would: under `Difference`, `Exclusion` and the four non-separable modes the
mode reaches the process channels alone and every spot channel takes
`Normal`. Groups composite on plates as on ink: `Canvas::composite` blends a
plates buffer onto its parent channel for channel; a non-isolated group's
`adopt_backdrop` reads its parent's tints, spots among them, where the parent
lies, whole pixels at a time since a plates pixel is wider than the five-byte
word, and `remove_backdrop` takes them out again by 11.4.7.2's step; and a
knockout group's `knock_out` restores its initial state under each element
(11.4.5), the backdrop's tints at no alpha or the transparent pixel it was
made as — so process ink in isolated, non-isolated and knockout groups is a
`CmykA8` page's bytes, and a spot under a non-isolated group comes back as it
went in. Across
formats a plates source composites onto `CmykA8` as its process ink and onto
light as that ink's light, and a light or ink source onto plates as ink on the
process channels with every spot under its shape knocked out, a group painting
every component; an uncoloured cell's colour on plates is its light turned
into ink. The five other
formats keep their five-byte compositing word and run no line they did
before: a plates paint builds a word as wide as its canvas once a paint. A
colour handed to plates through `fill_mask_inked` or `blend_pixel_inked` is
its ink on the process channels, as on `CmykA8` — its own components, or its
light turned into ink — with every spot knocked out, and a `PlateInk` handed
to another format paints its process tints as the DeviceCMYK colour they are.
`Canvas::channel` reads one channel out as `width × height` bytes, a plate's
tints; `pixel` reads a plates pixel as its process channels' light, a spot's
colour being its caller's to know; and `extract` copies plates as tints, into
`CmykA8` as process ink, into light as that light, and ink or light into
plates on the process channels.

*The opaque composite.* Under `Normal`, with the colour's alpha and the
state's both whole, onto a canvas that holds no initial backdrop and is not
ink, `fill_mask_inked` takes a path of its own, reading the mask's rows as
slices. Over an opaque pixel — any pixel of `Gray8` or `Rgb8`, and one whose
alpha byte is 255 in a format with alpha — the general blend collapses to
`min(mul255(cs, c) + mul255(cb, 255 − c), 255)` a colour channel at coverage
`c`, the alpha staying whole, and one of none is untouched. **At a full
effective alpha under `Normal` the general blend collapses over any pixel**:
the colour's channels and an alpha of 255, whatever the pixel held, its
initial backdrop and its format — the two rounded terms of the mix sum to the
source channel, `ab·cs/255` never being a half-integer, and an ink channel is
complemented in and out. So a row is walked in blocks of sixteen pixels: a
block of no coverage is passed over, a block of full coverage is one slice
copy of the colour whatever its pixels held, any other block over opaque
pixels takes that expression at every byte, alpha included, in one loop of
sixteen-bit lanes, and a block holding a pixel that is not opaque, like the
pixels after the last whole block, is taken a pixel at a time, the colour
where the coverage is full. And wherever coverage, the colour's alpha and the
state's alpha multiply to 255 under `Normal`, every other route writes the
colour and 255 the same way: the general fill onto an ink buffer, the
document's ink kept, or onto a group's buffer that has a backdrop; an image
or a shading's pixel at a full level; and a nested group's composite. A pixel
partly covered that is not opaque, and every other case — an alpha below 255,
another mode — goes through the general blend, whose rounding they need.

**A canvas is a rectangle of a page.** `Canvas::origin` is the device pixel
its top-left is — `(0, 0)` for a canvas that is the whole page, a tile's
corner for a tile, a group's corner for a group's buffer — and every mask,
every sampled coordinate and every `at` the crate is handed is in **device**
pixels of the page. Only the index into `Canvas::data` subtracts the origin,
which is an integer subtraction and cannot round. `Mask::overlap_at`,
`image_bounds_in`, `Fragments` and `Canvas::composite`/`extract` all read it.
This is ruling 5's lattice ([rulings](../rulings.md)): one frame makes every
coordinate one number, so a tile and the page under it agree even where the
exact value sits on one of this crate's grids — a shading's colour step, and
an edge's first sub-scanline, which `fill` takes as `ceil(y × 16)` with
nothing to absorb an ulp. The image quad's snap to the 1/256 grid, below, is
the reason a *cropped* page and the page under it agree, which is a different
transform and not a region.

**Images.** `draw_image` maps every destination pixel backwards through the
inverse transform into the samples — a forward map leaves seams and
double-writes. Three things decide what a pixel gets: how much of it the image
covers, which samples it reads, and whether it is composited alone.

*Coverage.* The image occupies the unit square of its transform (8.9.5.2), and
that square is rasterised through the same `fill` every path uses — same
sixteen sub-scanlines a row, same exact spans in 1/256 of a pixel, same
cancellation. So an image edge anti-aliases exactly as a filled edge does, and
a pixel the image half covers takes half the paint. The quad's corners are
snapped to that 1/256 grid first, because a rectangle placed by a translation
lands on a grid line far more often than not and `a + (e - t)` need not equal
`(a + e) - t` to the last bit; without the snap a cropped page and the page
under it would disagree.

*Sampling.* Decided per draw by `sampling_for` from the ratio alone, with
`/Interpolate` (Table 89) the only thing the caller supplies. Above 1:1 the
file gets what it asked for — `/Interpolate true` smooths four taps
(`Bilinear`), false keeps hard samples — and below it, the average of every
sample the pixel covers, weighted by how much of it the pixel covers —
integrated over the destination pixel's **true source rectangle**, which is
what makes a render at twice the scale, box-filtered down, the same picture.

Past 128:1 that gives way to exact 2x2 box-filter averaging into a `Pyramid` of
halved levels until the residual is within 4:1, which bounds the cost of the
one case exact integration does not: the same image drawn many times at
extreme minification, where the levels are built once and reused. The trade
it makes is written up in [design/image-edges.md](../design/image-edges.md),
because a pyramid is quantised to powers of two and therefore
*scale-dependent*.

Hard samples are `Area` over the pixel's own footprint rather than one nearest
tap, and above 1:1 that footprint is *smaller than a sample*: a pixel inside a
sample reads that sample and nothing else, and only a pixel straddling two
mixes them. That is the sample's edge anti-aliased, not the image blurred —
`/Interpolate false` still gets its hard pixels, and a 1:1 blit is still
byte-preserving. A nearest tap would quantise every internal sample boundary
to whole device pixels, which is the same defect as the outer edge one level
down: a render at twice the scale resolves those boundaries twice as finely, so
box-filtering it back down could not reproduce the coarser render.

*Two colours.* An image whose every sample is one of two colours — a bilevel
scan, a one-bit image of one component, a stencil — is handed over as
`ImageSource::two_colour`: one byte a sample choosing between two RGBA
colours (`TwoColour`), in place of three bytes of colour and one of alpha.
Both filters are linear in their weights, so such an image is sampled by
summing one channel, the weight on samples of the second colour, beside the
weight, and each of a pixel's four channels comes from the two sums exactly as
the average of the image expanded to RGB rounds it: the area filter's channel
is `(X0 × W0 + X1 × W1 + W / 2) / W` and bilinear's
`(X0 × (65536 − B1) + X1 × B1 + 32768) >> 16`, the integers the expansion's
sums divide. A pixel wholly on one colour is that colour, with no division. A
stencil is the pair with its paint opaque and the rest transparent, drawn
under `ImageDraw::tint`. Past 128:1 the pyramid's per-channel round-half-up is
not linear in coverage, so a two-colour image that reduces builds its first
level from the colour each sample chooses and is sampled through RGB levels,
as its expansion is.

*Part of an image.* An image whose buffers hold a rectangle of it rather
than all of it (`ImageSource::window`, a `SourceWindow`) is still sampled as
the whole image: the policy, the plans and every tap are worked out over its
full width and height in its own sample coordinates, and only the read of a
sample subtracts the window's corner, an integer subtraction that cannot
round. A sample outside the window reads the nearest one inside it, so every
pixel of the image's square is covered as it would be, and the pyramid halves
the window alone, pairing samples `2x` and `2x + 1` as the whole image's
levels pair them. A caller that widens the window by the sampler's reach
around every pixel it shows, its edges on the pyramid's pairs, gets those
pixels exactly as the whole image draws them
(`a_window_draws_the_whole_image_wherever_it_holds_the_samples_a_pixel_reads`,
`a_window_halves_to_the_whole_images_level_over_it`).

*Compositing, and the artefact anti-aliasing creates.* Two images that share an
edge each cover the pixels along it partly, and compositing them one after the
other lets the page through in between — a half-and-half boundary keeps a
quarter of the backdrop, because source-over of two half-covered draws is not
the average of them. That is conflation, and it is what every scan assembled
from strips is made of. So a *run* of consecutive image draws accumulates in
`fragments.rs` first — coverage adding rather than compositing, premultiplied,
four bytes a pixel, integers throughout — and reaches the canvas once. Two
strips each covering half a boundary pixel accumulate to one whole pixel of
their average, which is what a render at twice the scale box-filters down to.

Fragments are *added*, so a run holds only draws that do not overlap: a picture
laid over another ends the run and composites, and so does any change of alpha,
blend mode, clip or soft mask, and any other drawing operation. A run survives
`q`, `Q` and a form boundary, because none of those paints — which matters,
since real content brackets every image in `q`/`Q` and a run that ended there
would never hold two of anything. Past `MAX_IMAGE_RUN_PIXELS`, a quarter of
`MAX_PAGE_PIXELS`, images composite one at a time and abutting ones conflate;
that is bounded memory rather than a page that will not render.
A knockout group composites image by image too, because 11.4.5 gives every
element its own shape.

*Determinism, throughout.* The level count branches on 16.16 fixed-point
integers, because `log2` is a transcendental and a count one different is not a
rounding difference — it is a different image. The averaging weights are
integers for the same reason: an area is a product of two overlaps, and in
floats the accumulation order would decide the last bit. A draw with no shear
or rotation term — every axis-aligned placement, flipped or not — works out
each column's taps and each row's once, from the page-frame pixel centres, and
each pixel sums integer weights over them in `u64`, a row at a time; a rotated
or sheared draw maps each pixel through the inverse on its own, with the same
sums. Neither multiplies in `u128` or calls `floor` per pixel, and
[design/image-edges.md](../design/image-edges.md) shows both give the numbers
one pixel mapped on its own gives. So is the run's
accumulation, and so is the coverage the quad contributes. The only floats left
on the path are `sqrt`, division and `round`, which IEEE 754 pins exactly
(ruling 4). An image pixel's opacity — the draw's alpha times its coverage, its
sample's alpha and the clip, or a run's coverage and the clip — becomes a
level with no call to `round`: in integers at an alpha of one, and at any other
as the `f64` expression, in its order, rounded exactly by a truncation and a
comparison, the draw's alpha read once a draw. Both give the level the
expression rounded by `round` gives, and
[design/image-edges.md](../design/image-edges.md) shows why. The pyramid belongs to the caller, so an image's lifetime is
decided where it is known. A stencil's colour is `ImageDraw::tint`
(8.9.6.2): the image says where, the caller says what — and `ImageDraw::ink`
what that colour is in ink, which only a `CmykA8` canvas reads.

The trade is written down in [design/image-edges.md](../design/image-edges.md).

**Meshes.** `draw_mesh` rasterizes a whole Gouraud-shaded triangle mesh into
one `MeshBuffer` — coverage from a single non-zero fill over every triangle,
colour from a barycentric walk that interpolates the vertex inputs and only
then asks the caller's closure what colour they are. Each row of a triangle
walks the columns between its edges' crossings of the row and one more each
side, not the triangle's whole box, through the same inside test and weights,
so it paints the pixels a walk of the box paints; a row within 1/1024 of a
pixel of a vertex, a triangle under a quarter of a pixel wide and one reaching
past 65 536 pixels from the origin, where rounding could move the test's
answer by a pixel, walk the whole box through the test at every pixel
(`a_span_walk_paints_what_the_box_walk_paints`,
`a_triangle_visits_its_span_not_its_box`). Along a span the weights and the
interpolated inputs step by their increments a column from every 64th column
of the page frame, so a part of the page steps as the page does
(`a_part_of_the_page_steps_as_the_page_does`, ruling 5): one division by the
area for the triangle (`a_span_divides_once`), and each input as
`v2 + w0·(v0 − v2) + w1·(v1 − v2)`, so three corners of one value paint it
exactly. A pixel whose stepped weights are not clear of zero by four times
their error bound takes the inside test itself, so the pixels painted do not
change, and a stepped colour is within one level of 255 of the test's
weights' (ruling 22). One buffer rather than
one fill per triangle is what removes the lattice of pale seams that
per-triangle compositing paints along every shared edge. A silhouette pixel no
triangle's interior reaches takes its colour from a neighbour, spread two
rounds outward, so what a pixel gets depends on two pixels around it:
`draw_mesh_over` draws the part of the mesh a caller wants widened by that
reach and kept inside the region a render of the whole page uses, and measures
`MAX_MESH_WORK` over the whole region, so a tile's mesh is the page's mesh
pixel for pixel and a tile refuses exactly the meshes the page refuses.

**Cost and cancellation.** A paint costs what it covers: the caller asks for
a region per shape clipped to the clip's rectangle, every consumer walks the
mask's rectangle rather than the canvas, and the fill sweeps only the rows a
shape reaches. Long operations take a `stop`
predicate — the render layer's `CancelToken` — asked every sixteenth row of
a fill or composite and every 1 024 steps of a dash expansion, and what
comes back on a stop is the partial result, because a half-drawn shape is a
better progressive frame than a missing one. The predicate decides only
whether the sweep continues, never what a continued row computes, so the
check interval cannot change a pixel.

## API

Most hosts reach the rasterizer through the facade: `Page::render` takes a
`RenderOptions` — scale, `PixelFormat`, an optional `CancelToken`, whether
to draw annotations — and returns a `Bitmap`. The crate itself is a leaf,
usable directly with no PDF anywhere near it:

```rust
use tinker_pdf_raster::{fill, Canvas, Color, FillRule, Path, PixelFormat};

let mut path = Path::new();
path.move_to(10.0, 10.0);
path.quad_to(50.0, 90.0, 90.0, 10.0);
path.close();

// A mask over only the region the shape can touch, then one composite.
let mask = fill(&path, FillRule::NonZero, 0, 0, 100, 100, 0.1, None);
let mut canvas = Canvas::new(100, 100, PixelFormat::Rgb8, Color::WHITE);
canvas.fill_mask(&mask, Color::BLACK, 1.0);
```

A caller filling many shapes keeps a `FillScratch` and fills through
`fill_with(&mut scratch, …)`; `draw_image`, `accumulate_image`,
`image_coverage`, `draw_mesh` and `draw_mesh_over` take one too, for the
image's unit square and the mesh's coverage. A stroke's coverage goes through
one as well: `stroke_with(&mut scratch, …)` writes the outline into it, and
`fill_outline(&mut scratch, …)` fills it over the region taken from
`scratch.outline_bounds()`.

The crate root re-exports the working set: `Path`, `Verb`, `Point`,
`FillRule`, `flatten`; `fill`, `fill_with`, `fill_outline`, `fill_hard`,
`fill_hard_with`, `fill_outline_hard`, `HardEdge`, `FillScratch`, `Mask`;
`stroke`, `stroke_mapped`, `stroke_with`, `stroke_mapped_with`,
`stretches`, `dash`, `whole_pixel_width`, `adjusted_to_pixels`,
`StrokeStyle`, `LineCap`, `LineJoin`; `Canvas`, `Color`, `PixelFormat`, `MaskKind`;
`PlateInk`, `Spots`, `TooManySpots`, `MAX_SPOT_PLATES`; `BlendMode` (in
`blend`); `draw_image`, `ImageDraw`, `ImageSource`, `SourceWindow`, `TwoColour`, `Transform`,
`Filter`, `Sampling`, `Pyramid`, `image_bounds_in`, `resample`; `draw_mesh`, `draw_mesh_over`,
`MeshDraw`, `MeshBuffer`; `fill_stripes`, `fill_stripes_with`, `StripeMask`,
`SubpixelOrder`. The bridge
from PDF vocabulary — `/BM` names to `BlendMode`, content-stream operators
to paths — lives one layer up, in [rendering](rendering.md).

`toolbox` is the arithmetic of the facade's bitmap toolbox
([bitmap toolbox](../design/bitmap-toolbox.md)): over a buffer described by a
`toolbox::Layout` — its format, size, stride and whether it is premultiplied —
`gamma`, `invert`, `invert_luminance`, `tint` and `grayscale` map every pixel
in place and `colour_count`, `is_unicolour` and `is_monochrome` answer about
them, refusing whole a buffer that is not a picture (`Layout::is_picture`),
plates among them — tints of colorants whose colours the bytes do not carry —
and never reading or writing a row's padding. `scale` and `subsample` hand back
a new layout and its bytes: `scale` gives the colour to `image::resample` in
three-channel groups, premultiplied for the average where the buffer is
straight, and `resample` is the area filter's `area_taps` over a grid the
image covers exactly — column `x` of `width` covering
`[x W / width, (x + 1) W / width)` of the image, each tap that span's overlap
with a sample measured in `1 / width` of one, so the weights are the
rationals themselves rather than a centre a floating-point inverse found;
`subsample` keeps the first sample of each block; `convert` re-expresses a
buffer in another format by `Canvas::extract`'s per-pixel relation,
straightened first where it is premultiplied and premultiplied after where
asked, a same-format copy never through a colour. `tint_grey` writes a `Gray8`
picture into any format but plates, opaque, each grey on `tint`'s ramp between two
colours — what a forced-colour render's black-on-white coverage becomes in the
facade. `gamma_table` is gamma's 256 bytes,
built through `tinker_pdf_math::pow`, and `decode` and `encode` are a pixel's
sRGB and a colour's bytes exactly as a `Canvas` of the format reads and fills
them; `canvas::lab_bytes` is public beside `lab_to_rgb`, its inverse, so the
toolbox and its tests encode Lab as a Lab canvas does.

## Refused by name

| What | Typed variant / named bound | Why (one line) | See |
| --- | --- | --- | --- |
| A path past 1 048 576 verbs | `MAX_VERBS` (`geom.rs`); further verbs are dropped, and a stroke's outline written into a fill stops where its path would | A content stream can ask for unbounded geometry; a real page never does | [ruling 1](../rulings.md) |
| A verb with a non-finite point | dropped at `Path` construction | A NaN cannot be rasterized, and dropping the verb keeps the rest of the path usable | [ruling 1](../rulings.md) |
| A mesh costing more than 67 108 864 units of work, counted in the space it is drawn from (pixels at 72 dpi) so the same mesh is drawn or refused alike at every scale | `MAX_MESH_WORK` (`mesh.rs`); `draw_mesh` returns `None`, reported as `RenderWarning::UnsupportedShading` | Half a mesh reads as an artefact where nothing reads as the gap it is | [rendering](rendering.md) |
| An image under a singular transform | `Transform::invert` returns `None`; the draw paints nothing | A determinant near zero maps the image to a sliver with no useful bits left | `image.rs` |
| A plates canvas of more than 64 spot channels | `TooManySpots { asked }`, from `Spots::new` and `Canvas::plates` | Sixty-nine bytes a pixel is the most a canvas carries, and which spots to keep is the caller's plan, not this crate's | `canvas.rs` |
| A cancelled render | `RenderWarning::Cancelled` | The sweep stops at the next scanline band, keeping the rows already swept | [rendering](rendering.md) |

A dash expansion is additionally capped at 100 000 steps per segment, so a
tiny pattern over a long line cannot generate millions of pieces.

## Verified

- **Unit tests inside the crate**: `geom.rs` proves a
  quadratic flattens point-for-point like its exact cubic equivalent across
  seven curve shapes and five tolerances, and that flattening is
  deterministic; `fill.rs` covers both fill rules, mask algebra and the
  partial-mask stop, `every_coverage_quantises_to_its_level` holds
  `Mask::quantise` to its definition at each of the 256 coverages and every
  level, and `a_fill_sorts_nothing` counts the comparisons a fill
  makes — none on any closed path or on the 300 triangles of the benchmark
  page — beside the crossings that share a lane, which must be the count the
  sorted sweep's own arithmetic gives; `fill_reference.rs` keeps that sweep,
  test-only, and `fill_is_the_reference_sweep_on_generated_paths` holds `fill`
  to it byte for byte under both rules over seven classes of generated path —
  overlapping subpaths wound either way, self-intersections, crowded pixels,
  slivers, coordinates near 1e9, every cap, join and dash of the stroker, and
  dropped edges — through a fresh scratch and back to back through one;
  `the_centre_rule_is_the_sorted_sweep_of_each_row_s_middle` holds the centre
  rule there to the sweep of each row's middle sub-scanline, through one
  scratch that coverage fills share; in `fill.rs`,
  `the_centre_rule_on_a_tile_is_the_page_s` holds it over every tile of
  rectangles at sixteenths, a triangle, a circle, an even-odd ring, an
  unclosed middle and a stroke's outline to the page (ruling 5), and
  `the_half_rule_is_the_coverage_hardened`,
  `the_centre_rule_outlines_as_it_fills`,
  `a_centre_on_a_sub_scanline_that_does_not_close_is_read_as_the_sweep_reads_it`,
  `an_edge_through_a_row_of_centres_gives_them_to_one_side` and
  `a_stopped_centre_fill_keeps_the_rows_it_had_already_swept` its pieces;
  `the_any_part_rule_is_the_sweep_s_spans_and_the_outline_s_extents` holds
  the any-part rule to the sorted sweep's spans and the outline's extents over
  the generated classes, the coverage's, the centre rule's and the samples'
  pixels inside it, and `the_any_part_rule_on_a_tile_is_the_page_s`,
  `an_outline_under_the_any_part_rule_reads_the_pieces_it_kept` and
  `a_stopped_any_part_fill_keeps_the_rows_it_had_already_swept` its tiles, a
  stroke's kept pieces and its stop;
  `a_fill_after_the_first_allocates_no_scratch` counts the scratch's buffers
  that grow, none after a first fill as large as any that follows;
  `an_interior_run_is_one_slice_fill` counts conversions, exactly the cells
  holding a crossing plus one a run on every closed generated path — five a
  row for a rectangle 193 pixels wide;
  `the_sixteen_lane_walk_is_the_walk_lane_by_lane` holds the branch-free walk
  of a cell's lanes to the walk a lane at a time over random cells and
  windings, both rules, `the_rule_mask_reads_a_winding_as_the_rule_does` the
  rule's mask to the rule, and
  `a_stepped_crossing_is_the_saturating_one_wherever_it_is_taken` a stepped
  edge's crossings to the saturating ones at every scale, the bound itself
  exact, while `an_edge_that_could_saturate_keeps_the_saturating_crossing`
  fills edges past it as the sorted sweep does;
  `stroke.rs` covers caps, joins, the miter limit and
  dash phase, holds the outline written into a scratch to `fill` of the path
  the same stroke returns, byte for byte and from the same bounds, over every
  cap, join, miter limit, width, dash pattern and phase, mapped pen and floor,
  open, closed, curved, degenerate and far-flung paths, partial outlines a
  stop leaves, and an outline cut at `MAX_VERBS`
  (`the_direct_outline_fills_as_the_path_outline_did`,
  `an_outline_stops_where_its_path_reaches_the_verb_bound`), counts
  flattening passes, one a stroke written and filled
  (`a_stroke_is_flattened_once`), and gauges what dashes hold: one piece at
  a time, and buffers that grow for the first piece and not for the ten
  thousandth (`a_dashed_stroke_holds_one_piece_at_a_time`); `blend.rs` pins all sixteen modes in integers; `image.rs`
  measures the sampling policy against numbers — a magnified gradient's
  largest step, a 2:1 checkerboard averaging to uniform 128, and an image
  wholly off the canvas costing nothing — holds the planned walk pixel for
  pixel to every pixel mapped on its own, from 8:1 magnified to past 128:1
  minified, flipped, short and turned
  (`the_column_plan_is_the_per_pixel_sampler`), counts one plan a column
  and one a row (`a_draw_plans_its_weights_once_per_column_and_row`) and no
  `round` call per pixel (`an_image_draw_rounds_no_float_per_pixel`), holds
  a two-colour image pixel for pixel to its RGB expansion over every filter,
  ratio, flip, offset and colour pair and under a tint, past 128:1 included
  (`a_two_colour_image_draws_as_its_rgb_expansion`), and counts one channel
  summed a tap where the expansion sums four
  (`a_coverage_image_sums_one_channel`), holds an image's edge at every level
  to its unit square's coverage quantised and filled
  (`an_image_edge_quantises_as_a_path_edge`) and under the centre rule to the
  pixels whose centres its square holds
  (`an_image_edge_under_the_centre_rule_paints_the_pixels_whose_centres_it_holds`)
  and under the any-part rule to every pixel it meets
  (`an_image_edge_under_the_any_part_rule_paints_every_pixel_its_square_meets`),
  and
  `canvas.rs` holds every image pixel's level to the expression rounded, over
  every coverage, sample alpha and clip at a sweep of alphas
  (`an_image_pixel_composes_as_the_float_expression_rounded`); `mesh.rs`
  proves shared edges leave no seam and holds a silhouette at every level and
  under each rule to its coverage quantised, its triangles' centres and the
  pixels they meet, the colours spread before the level
  (`a_mesh_silhouette_takes_its_level_and_its_rule`); `canvas.rs` covers formats, backdrops
  and bounded compositing, holds the opaque composite to the general blend
  byte for byte — exhaustively over coverage, source and destination channel
  on every format it is taken on, and over whole fills
  (`the_opaque_path_is_the_general_blend_where_it_applies`) — and counts the
  general blends an opaque Normal fill onto an opaque canvas makes, none
  (`an_opaque_normal_fill_calls_no_general_blend`); holds a block of opaque
  pixels to the composite a pixel at a time at every coverage and source
  byte (`an_opaque_block_is_the_composite_a_pixel_at_a_time`), the
  sixteen-bit product to `mul255` over every pair of bytes
  (`mul255_in_sixteen_bits_is_mul255`), and rows of every count of blocks and
  every tail to the general blend, with the general blends counted to the
  pixels partly covered that are not opaque
  (`the_row_walk_takes_every_block_and_tail_as_the_general_blend`); holds
  `to_mask` to `pixel` read the long way round, over every value each format's
  reading depends on (`to_mask_reads_every_format_as_pixel_does`); and
  `fill.rs` holds every consumer of a mask that reads a value outside itself —
  `at`, `overlap_at`, both products, `harden`, a run's overlap and the
  canvas's fill, composite and knockout restore — to the same consumer of a
  field the size of the canvas holding the same values, on canvases at the
  origin and away from it and over boxes inside, hanging off, wholly off and
  of no pixels (`every_mask_consumer_reads_the_outside_value`,
  `a_product_of_two_masks_reading_outside_themselves_holds_both`);
  `canvas.rs` holds a group's backdrop read from its parent to a copy of the
  parent's rectangle — adoption, `a0` at every pixel, the knockout restore
  and the removal — for every parent format under every group format, over
  buffers inside the parent, all of it and hanging off it
  (`a_group_reads_its_backdrop_from_its_parent_as_a_copy_would_hold_it`), and
  a copy past a canvas's right edge to nothing there
  (`a_copy_past_a_canvas_s_right_edge_reads_nothing_there`); holds the
  composite at a full effective alpha to `blend` over every source and
  destination channel value against a lattice of own and backdrop alphas in
  every format (`the_opaque_normal_path_is_blend_in_every_group_format`), every
  route into a group's buffer — a fill, sampled pixels and a nested group's
  composite, into isolated, non-isolated and knockout buffers, an ink buffer
  with and without the document's ink — to `blend` at every pixel
  (`every_route_into_a_group_writes_what_blend_writes`), and counts the
  general blends an opaque paint inside a group makes, none
  (`an_opaque_paint_inside_a_group_skips_blend`).
- **Determinism fingerprints** (`crates/tinker-pdf/tests/determinism.rs`):
  the render fingerprints `text`, `curves`, `shading`, `blend`, `pattern`,
  `image`, `transparency`, `tiling`, `mesh` and `strokes` exercise this crate
  directly, each a pixel hash plus dimensions and an ink floor, reproduced
  byte-for-byte across targets ([determinism](determinism.md)). `strokes` is
  the pen: every cap and join, a miter past its limit, dashes at two phases
  and an even-odd star.
- **Analytic coverage** (`tinker-pdf-raster/tests/analytic_coverage.rs`):
  every expectation a closed form of the sampling grid, evaluated in the test
  from the path's own equation and compared byte for byte — both fill rules, a
  half-covered edge, a diagonal against the area the line states, and
  `a_shallow_edge_keeps_the_slope_the_line_states`, a parallelogram two pixels
  wide and 256 tall drifting fourteen across, whose slope is 0.875 of a 1/256
  unit per sub-scanline — a slope a whole 1/256 unit would truncate to zero
  (above). `stripe_coverage_is_three_times_horizontal_coverage_filtered`
  holds stripes to it as well: rectangles at every third of a pixel against
  the closed form for a pixel a third as wide, filtered in the test with the
  design's taps, and a diagonal, a quadratic and an even-odd pair against
  `fill` of the path mapped three times across, filtered there, both orders;
  `a_stripe_mask_over_a_tile_is_the_page_s` holds every tile up to four
  pixels wide, at every column, to the page's stripes; `stripes.rs`'s unit
  tests hold the taps to a pixel's average taken twice, a horizontal edge to
  its grey coverage, a vertical one at a stripe to its ninths, `Bgr` to `Rgb`
  reversed, and a region with no stripes to fill to no coverage.
  `the_centre_rule_paints_exactly_the_pixels_whose_centres_the_shape_holds`
  holds the centre rule to the grid's own answer: rectangles at every
  sixteenth on each edge and between the gradations by the closed form from
  their corners, and triangles, a circle and an even-odd ring under both rules
  by each edge's crossing of a row's middle, from its equation, to the nearest
  1/256, and the winding of the crossings left of each centre.
  `the_any_part_rule_paints_exactly_the_pixels_the_shape_touches` holds the
  any-part rule to the test's own intersection of each shape with each
  pixel's open square on the grid — rectangles at every sixteenth from their
  corners, slivers thinner than a sub-scanline, triangles, a line traced
  there and back, points and a circle from their edges' equations — and
  `a_sliver_between_sub_scanlines_is_not_lost` the sliver no sample sees. What
  this tier adjudicates is the
  implementation against the *stated sampling model*, which is this crate's
  own contract rather than ISO 32000's — named here because that is a weaker
  claim than a third-party vector and is worth not mistaking for one.
- **Analytic sampling**
  (`tinker-pdf-raster/tests/analytic_sampling.rs`): every expectation computed
  in the test from the geometry. Minification against an independent box
  average at six ratios; a half-covered edge against its area on either axis at
  all fifteen sixteenths, and a corner against the product of its two
  fractions; a 1:1 integer-aligned draw asserted byte-preserving so a coverage
  off-by-one would show as a rim; and the conflation pair — two abutting black
  strips leaving **63** composited one at a time and **0** as one run.
- **Integration tests** through the facade: `blend_modes.rs`,
  `stroke_parameters.rs`, `images.rs`, `inline_images.rs`,
  `mesh_shadings.rs` and `transparency_groups.rs` under
  `crates/tinker-pdf/tests/`.
- **Fuzzing**: the `render_page` target drives whole documents through the
  interpreter into this rasterizer under ruling 1
  ([verification](../verification.md)).
- **Cost regression**: `a_small_fill_on_a_large_page_stays_small` (in
  `crates/tinker-pdf-render/src/lib.rs`) asserts on mask pixels *asked for*
  rather than pixels painted, so a paint that costs the whole canvas fails it
  while passing everything else.
