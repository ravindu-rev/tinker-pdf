# Anti-aliasing levels

*For [ROADMAP](../ROADMAP.md) row RC-11.* Today a render's edges are
anti-aliased or not (`RenderOptions::antialias`): on, every pixel shows the
8-bit coverage `fill` measures; off, `Mask::harden` makes each pixel whole or
empty at half its coverage. This document fixes coverage levels from 0 to 8
bits, chosen for graphics and for text apart; two more rules for the
non-anti-aliased level, a pixel painted when the shape holds its centre or
when it meets any part of it; a minimum line width at each level and rule
that keeps a hairline whole; how the options sit beside `antialias`, which
keeps meaning what it means; and how the levels combine with RC-10's stripe
text ([lcd-text](lcd-text.md)).

## Scope

- **Levels** (RC-11.2): 8-bit coverage quantised to 1 to 7 bits, and what 0
  bits is; `Mask::quantise`, and an image's edge carrying a level.
- **The centre-of-pixel rule** (RC-11.3) and **the any-part-of-pixel rule**
  (RC-11.4), each stated on the filler's grid.
- **Text and graphics** (RC-11.5): which coverage each site makes is which.
- **The minimum line width** (RC-11.6): its default at each level and rule,
  and why each keeps a hairline whole.
- **The options**, beside `antialias`, and **stripes and contrast** with them.
- **Tiles** (ruling 5), the pinned hashes and the speed rows (RC-11.7a).

## Non-goals

- **More samples.** Sixteen sub-scanlines a row and 1/256 of a pixel across
  stay the grid (`fill.rs`); raising them moves every fingerprint and is a
  decision of its own.
- **A level per object.** One level for graphics and one for text a render,
  as `antialias` is one switch a render.
- **Dithering** a quantised coverage, **gamma**, and **grid-fitting** an
  outline to the pixels.
- **Levels for what is not coverage.** A constant alpha, a soft mask's
  levels, an image's own alpha and the colours of its samples are the
  document's, as `antialias` leaves them today.

## Design

### Levels

A level is a number of bits `b` of coverage a pixel shows, 0 to 8. Level 8 is
the 8-bit coverage `fill` measures. Levels 1 to 7 take each pixel's coverage
`c` to the nearest of `2^b` values spaced evenly from 0 to 255:

```text
n      = 2^b − 1                   the highest level's index
q      = (c·n + 127) / 255         the nearest index, a half rounding up
Q_b(c) = (q·255 + (n − 1)/2) / n   that index's value, to the nearest byte
```

in integers. `n` is odd, so `q·255 / n` is never a whole number and a half,
and the last division rounds to the nearest with no tie. The same formula is
the identity at `b = 8` (`n = 255`, `q = c`), so levels 1 to 8 are one
function. `Q_b(0) = 0` and `Q_b(255) = 255`, `Q_b` is non-decreasing, and its
values are `round(k·255 / n)` for `k` from 0 to `n`: at two bits 0, 85, 170
and 255, at four the multiples of 17.

**Level 1 is the half rule.** At `b = 1`, `q` is 1 exactly when `c ≥ 128`, so
`Q_1` is `Mask::harden`: whole at half a pixel's coverage, empty below it.

**Level 0 is no anti-aliasing, under a rule.** Every pixel is whole or empty,
and which is decided by one of three rules:

| Rule | A pixel is painted when |
| --- | --- |
| `Half`, the default | its coverage is at least 128 — `harden`, `Q_1` |
| `Centre` | the shape holds the pixel's centre, on the filler's grid |
| `AnyPart` | the shape meets the pixel's open square |

`Half` reads the coverage, so level 0 under it and level 1 draw alike. The
other two read the shape, which an 8-bit coverage does not hold, and that is
why level 0 is a rule rather than a quantisation.

**`Mask::quantise(bits)`** applies a level to a mask in place: 8 leaves it, 1
to 7 apply `Q_b`, 0 is `harden`, and a number above 8 reads as 8. It is a
function of one pixel's coverage, so a tile is the page under it (ruling 5).
`ImageDraw::edge_bits` replaces `ImageDraw::antialias`: the image's unit
square is quantised at its level before the samples are read, 8 by default,
so an image's edge quantises as a path's does.

### The centre rule, on the filler's grid

The filler samples a row at sixteen sub-scanlines, the `k`-th at
`y = row + k/16`, and on each measures spans in 1/256 of a pixel, each edge's
crossing to the nearest unit, a span `[L, R)` holding units `L` to `R − 1`.
Sub-scanline 8 lies at the row's middle exactly, so the centre of pixel
`(col, row)`, the point `(col + ½, row + ½)`, is unit `256·col + 128` of
sub-scanline `16·row + 8`. **The pixel is painted when that unit is inside**:
when the winding the rule reads there calls it inside.

For a rectangle `[x0, x1) × [y0, y1)` that is

```text
⌈16·y0⌉ ≤ 16·row + 8 < ⌈16·y1⌉   and   round(256·x0) ≤ 256·col + 128 < round(256·x1)
```

— the filler's own two bounds — and for any other shape each edge's crossing
of `y = row + ½`, from the edge's equation, to the nearest 1/256, and the
winding of the crossings left of the centre. That is what RC-11.3's test
computes.

The spans are half-open, so an edge exactly through a centre takes it on its
left and top and gives it up on its right and bottom: two shapes sharing an
edge through a row of centres split those pixels, neither leaving a gap nor
painting one twice. One sample in the page's frame decides a pixel, so a tile
is the page (ruling 5). A shape narrower than a pixel between two centres
paints nothing; a stroke's minimum width (below) keeps a line from that.

### The any-part rule

**Meeting.** A pixel meets a shape when the shape — the region its rule calls
inside together with its outline, every subpath closed and flattened as the
filler flattens it, every crossing across taken to the nearest 1/256 as the
filler takes it — has a point inside the pixel's open square,
`(col, col + 1) × (row, row + 1)`.

On the filler's grid that is one of two things:

1. **A unit of the pixel is inside on one of its sixteen sub-scanlines.** This
   is any shape wider than a unit and taller than a sixteenth, and it is a
   unit, not the byte: a pixel holding one to fifteen units has a coverage of
   0 and still meets the shape.
2. **An edge of the outline passes through the open square.** The edge's part
   inside the open row `(row, row + 1)` — from where it crosses the row's top
   or begins inside the row to where it crosses the bottom or ends, each point
   across to the nearest 1/256 — reaches into the pixel's open interval of
   units, `(256·col, 256·col + 256)`. A horizontal edge inside the row counts,
   which the filler's crossings never do. This is what a sliver lying wholly
   between two sub-scanlines is made of, and the samples cannot see it.

The square is open, so a shape that only touches a pixel's edge or corner
does not paint it: a rectangle on whole pixels paints its own pixels and no
more, as its 8-bit coverage does. An outline of no area — a subpath of one
point, or a line traced there and back — still meets the squares it passes
through, which is 8.5.3.3.1's degenerate subpath enclosing "the single device
pixel lying under that point"; a point exactly on a pixel's edge lies under
none and paints none. The mechanism — each edge's extent over each row it
crosses, beside the samples — is RC-11.4's, and its cost is RC-11.7a's to
record. A circle is held to its flattened outline, the polygon the filler
fills, so a test's geometry is the filler's.

### Text and graphics

Each coverage a render makes is text or graphics, and takes that level:

| Site | Text or graphics |
| --- | --- |
| A glyph's fill or stroke, on the cached mask's route (P-06) or the path route | text |
| The glyph outlines a clipping text mode (9.3.6, modes 4 to 7) adds to the text clip | text |
| Every paint of a Type 3 glyph's procedure | text |
| A path's fill and stroke, a clip by `W` or `W*` | graphics |
| An image's edge, a mesh shading's silhouette | graphics |
| A tiling pattern's cell | the cell's own: its renderer is handed both levels and the rule through `TileRequest`, so text in a cell is text |
| `sh` and a shading pattern | the clip or shape they paint through, already at its level |

`Renderer::coverage` reads the text level while a glyph is being shown and
the graphics level otherwise. Every site applies its level or rule before the
clip and the soft mask multiply in, as `harden` is applied today, so a
clipped edge is the product of two masks each at its own level.

### The minimum line width

8.4.3.2 asks a line of width 0 for "the thinnest line that can be rendered at
device resolution". Today that is 0.8 px, 0.6 px for a stroked glyph, with
anti-aliasing on, and 1 px with it off (`Renderer::thinnest`). The floor
becomes a function of the level and the rule of the stroke's own kind —
graphics for a path, text for a stroked glyph — in device pixels, in the
page's frame:

| Level | Rule | A path | A stroked glyph |
| --- | --- | --- | --- |
| 8 | — | 0.8 | 0.6 |
| 1 to 7 | — | 1 | 1 |
| 0 | `Half` | 1 | 1 |
| 0 | `Centre` | 65/64 | 65/64 |
| 0 | `AnyPart` | 1/256 | 1/256 |

So the default, level 8, and `antialias` off, level 0 under `Half`, draw what
they draw today. **Each keeps a hairline whole** — no column a line no
steeper than one in one crosses left empty, and no row a steeper one crosses:

- **Level 8.** No level thresholds anything, so a pixel shows a band as soon
  as it holds sixteen units. Where a band at least 0.6 px thick crosses a
  column, at the column's middle `x` it holds at least nine of the sixteen
  sub-scanline heights, at worst five in one row and four in the next. On each
  of those its span holds the middle and is at least `0.6 / sin θ` long —
  0.84 px for a line no steeper than one in one — so at least half a pixel of
  it lies in the column: 128 units, 127 after the crossings round. One pixel
  holds five such spans, 635 units, a coverage of 39. A steeper band crosses
  each row whole: each of the row's sixteen sub-scanlines holds a span at
  least 0.6 px long, 152 units, and the band drifts at most a pixel across the
  row, so the spans lie in at most three pixels and one holds a third of them,
  810 units.
- **Levels 1 to 7.** `Q_b(c)` is above zero whenever `c ≥ 128`, since the
  first level's midpoint `⌈128 / (2^b − 1)⌉` is at most 128; so every pixel the
  half rule keeps, every level from 1 to 7 keeps, and a line the half rule
  draws whole they draw whole.
- **Level 0, `Half`.** A band 1 px thick holds sixteen consecutive sub-scanline
  heights at every `x` it crosses, at worst eight in each of two rows, so a
  level line puts 2 048 units, 128, in one pixel of every column — the
  threshold exactly; a sloped band puts more of itself in each column.
  `a_hard_edged_hairline_leaves_no_column_it_crosses_empty` sweeps slopes from
  level to one in one at eight phases and holds it today.
- **Level 0, `Centre`.** At a column's middle `x`, a band at least 65/64 px
  thick spans at least that much down, so some row's middle, `row + ½`, has at
  least 1/128 px of the band above it and below it there. For a line no
  steeper than one in one that puts the centre at least 1/128 px inside both
  of the band's edges across, at the centre's own height; the filler takes
  each crossing to the nearest 1/256, which moves it at most 1/512, so the
  centre's unit is inside. A steeper band is the same across: on sub-scanline
  8 of a row it crosses its span is at least 65/64 px, 260 units and 259 after
  rounding, and 256 consecutive units always hold the 128th of a pixel.
  Exactly 1 px is not enough: rounded, a span can end a unit short of every
  centre.
- **Level 0, `AnyPart`.** A band of any width meets the open square of every
  pixel its centre line passes through the inside of, and a centre line that
  runs along a pixel boundary has the band's two edges either side of it,
  1/512 px away, each through the open squares beside it. 1/256 is the
  filler's own unit across, the narrowest outline its crossings tell apart.

`RenderOptions::thinnest_line` overrides the floor, in device pixels, for
every stroke; `None`, the default, takes the table's.

### The options, beside `antialias`

```rust
pub enum HardEdge { Half, Centre, AnyPart }

// tinker_pdf::RenderOptions
pub antialias: bool,              // true by default, as today
pub graphics_bits: u8,            // 0 to 8, 8 by default
pub text_bits: u8,                // 0 to 8, 8 by default
pub hard_edge: HardEdge,          // the rule at level 0, Half by default
pub thinnest_line: Option<f64>,   // device pixels; None takes the table's
```

**`antialias` keeps its meaning.** `antialias: false` reads both levels as 0,
whatever `graphics_bits` and `text_bits` say, under `hard_edge` — so today's
`antialias: false`, with the default `Half`, is level 0 under `Half`
everywhere, which is `harden` at every site and 1 px strokes: the bytes it
writes today, and `hard_edged_output_is_pinned` keeps its hashes. `antialias:
true` takes the two levels as given, and the defaults, 8 and 8, are the
render today's default makes, so `the_default_is_unchanged` keeps its hash. A
caller who wants hard edges under another rule turns `antialias` off and names
the rule.

### Stripes and contrast

RC-10's stripe text ([lcd-text](lcd-text.md)) and its contrast curve act on
the same glyph coverage, and combine with the text level so:

- **Stripes need the text's eight bits.** At a text level below 8, or with
  `antialias` off, a glyph is grey at its level: a quantised or thresholded
  coverage has lost the fractions a stripe's filter distributes. This is a
  condition beside lcd-text's others.
- **Contrast before the level.** A glyph's coverage goes through the contrast
  curve, then the text level, then the clip: the curve shapes the coverage the
  level then quantises, so a darkened thin stem survives a coarse level that
  would have dropped it. The `Centre` and `AnyPart` rules read the shape, not
  its coverage, so contrast does not reach them; under `Half` it moves the
  threshold's input, which at level 0 is the identity by default.
- **The glyph cache's key** gains the text level and the rule, as it gains
  the stripe order and the contrast level, so a mask made under one is never
  drawn under another.

### Tiles

Every piece is a function of the page's frame: a level is a function of one
pixel's coverage, which already agrees; the centre rule reads one sample, in
the page's device pixels; the any-part rule reads the outline's geometry, and
a tile's region is a part of the page's, so an edge's extent over a row is the
same number in both; and a minimum width is a stroke's floor in device
pixels, which the page's one transform makes the same in a tile. A tile at
every level and rule is the page under it.

## Milestones

1. **RC-11.2, quantisation.** `Mask::quantise` and `ImageDraw::edge_bits` in
   place of `ImageDraw::antialias`, the render crate's one construction taking
   8 or 0 from `antialias`. Exit: `fill.rs`
   `every_coverage_quantises_to_its_level` — each of the 256 coverages at each
   level 0 to 8 is `Q_b` computed in the test, level 0 `harden`, level 8 the
   coverage — and `image.rs` `an_image_edge_quantises_as_a_path_edge`;
   `a_hardened_mask_is_whole_or_empty_everywhere` and
   `hardened_shapes_sharing_an_edge_partition_its_pixels` unchanged.
2. **RC-11.3, the centre rule** in the filler, for paths, clips, an image's
   unit square and a mesh's silhouette, where a caller asks for it. Exit:
   `analytic_coverage.rs`
   `the_centre_rule_paints_exactly_the_pixels_whose_centres_the_shape_holds`
   — rectangles at every sixteenth of a pixel, triangles, a circle and an
   even-odd ring, each pixel's answer from the shape's equation on the
   filler's grid — and `fill.rs` `the_centre_rule_on_a_tile_is_the_page_s`.
3. **RC-11.4, the any-part rule** in the filler, the same shapes. Exit:
   `analytic_coverage.rs`
   `the_any_part_rule_paints_exactly_the_pixels_the_shape_touches` —
   rectangles at every sixteenth, triangles, a circle and slivers thinner
   than a sub-scanline, each pixel's answer the test's intersection of the
   shape with its open square — `a_sliver_between_sub_scanlines_is_not_lost`,
   and `fill.rs` `the_any_part_rule_on_a_tile_is_the_page_s`.
4. **RC-11.5, through the renderer.** The four options and every site of
   *Text and graphics*. Exit: `render_options.rs`
   `coverage_at_each_level_is_the_eight_bit_coverage_quantised` — each
   element of the hard-edge page at levels 1 to 8, graphics and text apart,
   against its black-on-white twin's 8-bit coverage quantised in the test —
   and `each_rule_reaches_every_coverage_site`; `hard_edged_output_is_pinned`
   and `the_default_is_unchanged` keep their hashes; `render_regions.rs`
   `a_tile_at_every_level_and_rule_is_the_page_under_it`; both builds.
5. **RC-11.6, the minimum width.** `RenderOptions::thinnest_line` and the
   table in place of `Renderer::thinnest`'s floors. Exit: `render_options.rs`
   `a_hairline_at_the_minimum_width_drops_out_nowhere` — zero-width strokes at
   the slopes `a_hard_edged_hairline_leaves_no_column_it_crosses_empty`
   sweeps, at several phases, leave no column (row, past one in one) they
   cross empty at every level and rule — and `levels_rules_and_widths_are_pinned`,
   pinned on Linux, Windows and `wasm32-wasip1` and entered in K-07's roster.
6. **RC-11.7a, the speed rows.** The render group's page of 250 paths at
   150 dpi (`paths(250)`) under the centre and any-part rules and at a 4-bit
   graphics level, and its text page with an embedded face at 150 dpi at a
   4-bit text level, each beside the default render of its page, which the
   planning baseline draws in its place, unmeasured until RC-11.7b.

## Risks

| Risk | Mitigation |
| --- | --- |
| The defaults move a byte | Level 8 and level 0 under `Half` are today's two paths and floors; `the_default_is_unchanged` and `hard_edged_output_is_pinned` keep their hashes |
| A rule's definition differs between the filler and its test | Both rules are stated on the filler's own grid — its sub-scanline heights, its rounding across, its half-open spans — and the tests compute from the shapes' equations on that grid |
| The any-part rule misses a sliver between sub-scanlines | Meeting counts an edge through the open square, horizontal ones included; `a_sliver_between_sub_scanlines_is_not_lost` |
| A hairline drops out at a level or rule | Each floor is argued above; `a_hairline_at_the_minimum_width_drops_out_nowhere` sweeps every level and rule |
| Level 0 under `Half` and level 1 look like a duplicate | They are one function by design: level 1 is the coarsest quantisation and level 0 names the rule, and the two agree on the one rule that reads coverage |
| A cached glyph mask drawn at another level or rule | The text level and the rule join the glyph cache's key |
| The any-part rule's extents cost a row's edges again | Recorded by RC-11.7a; a target is the owner's (H-77) |
