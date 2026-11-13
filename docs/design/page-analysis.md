# Page analysis

A caller often wants to know something about a page without wanting its
pixels: where the page draws, so that it can be cropped to its content;
whether it needs transparency or overprint, so that a print path knows what it
is handed; whether it is grey, so that it is printed on a cheaper device; and
what a render of it drew, so that a slow page can be explained. Each of those
is a choice an implementer would otherwise make alone — what a mark is, which
clip decides, what "grey enough" means, which of the engine's two readings of
"transparency" is the page's — so this document makes them once, for roadmap
row RC-17, and names the test that holds each.

## Scope

- **The bounding box of the page's marks**: the union of the boxes of every
  mark a render paints, clipped, in points and in the pixels of a render.
- **Whether the page uses transparency**, read from what it draws in the five
  ways `crates/tinker-pdf/src/pdfa/colour.rs`'s `transparency` names.
- **Whether the page uses overprint**, read from the graphics-state keys K-01
  stores, at the paints made under them.
- **Colour or grey**: a render read pixel by pixel against a spread tolerance
  and a count of coloured pixels allowed, with defaults.
- **Per-render statistics**: the calls a render drew by class, the calls
  optional content hid, and the images it drew from decoded samples with
  their sample counts, beside MS-01's counted-work tally.

## Non-goals

- **RC-09's bitmap tests.** The toolbox's monochrome, unicolour and colour
  count tests (`docs/design/bitmap-toolbox.md`) each ask an exact question of
  a bitmap's bytes. The grey test asks a tolerant one of a page — whether it
  needs colour at all — and is this row's.
- **FO-23's ink coverage.** Coverage per plate, its heat map and maximum, and
  the spot and overprint inventories are print production's. The overprint
  answer here says whether a paint is made with overprint on; what that does
  to a plate is FO-23's, and simulating it is RC-03's.
- **An ink box.** The mark box is geometry, read from the drawing. Where a
  render puts visible ink — what `optional_content.rs`, `page_geometry.rs` and
  `vertical_metrics.rs` compute with their own pixel `ink_bounds` helpers — is
  a question about pixels, and those helpers stay what they are: a check
  independent of this one.
- **A region.** Every answer is the whole page's; a caller's
  `RenderOptions::region` is not read.
- **Time.** No answer and no statistic reads a clock.

## Design

### The query

`Page::analysis(&RenderOptions) -> PageAnalysis` and
`DisplayList::analysis(&RenderOptions) -> PageAnalysis` answer the box, the
transparency and the overprint questions. A page's analysis is its display
list's: `Page::analysis` records the page once through `Page::display_list`
and runs one analysis device over the recorded events
(`tinker_pdf_content::replay`) with the list's resources, so the two doors
give one answer by construction. A list that is not retained — a page past
`MAX_DISPLAY_LIST_BYTES` — interprets the page into the same device instead,
its annotations drawn through it as `Page::render` draws them through the
renderer (`annots::draw` takes any device that resolves names in a scope it is
handed).

Of the options, two are read, both because a render reads them: `annotations`,
which decides whether the page's annotation appearances are part of what it
draws, and `scale`, which with `Page::scales`' clamp decides the pixel box. The
rest — the format, the region, anti-aliasing, a transparent canvas — change no
answer. No pixel is touched, so the three answers cost an interpretation and a
walk of its events.

The grey test is the one answer pixels decide, so it is asked of a render:
`Bitmap::greyness(&GreyThresholds) -> Option<Greyness>` reads one, and
`Page::greyness` and `DisplayList::greyness` render with the caller's options —
the caller's scale — and read that render. `None` exactly when the bitmap is
not a picture, `Bitmap::to_png`'s condition.

The statistics are a render's: `RenderOptions::stats` asks for them, and
`Bitmap::stats` carries them with the rest of MS-01's tally.

### The analysis device

One `tinker_pdf_content::Device`, in `crates/tinker-pdf/src/analysis.rs`. It
answers the interpreter's three questions as the renderer does, through the
renderer's own `tinker_pdf_render::Admission`, so a replay of a list recorded
with the renderer's answers never disagrees with it, and a direct
interpretation runs the brackets a render runs. It resolves glyph outlines,
shadings, form scopes and annotation scopes through the same
`GlyphSource` resources a render reads, so its glyphs are the faces' own: a
glyph's box comes from its outline, in both builds, and a glyph no face draws
is no mark, as it is no ink.

It keeps what the renderer keeps to decide *where* a paint can land, as
rectangles in the page frame:

- **the clip's box.** A `W` or `W*` path's exact bounds met with the box in
  force; at `ET`, a text object's clip — the union of its clipping glyphs'
  boxes, and nothing at all where a clipping mode was selected and no glyph
  drew. A clip of no area clips everything away, as its coverage does.
- **the soft mask's box.** At the end of a mask group: the group's `/BBox`
  where the mask reads zero outside it, and the whole page where it does not —
  a luminosity mask whose `/BC` is not black, through its `/TR` — which is the
  rule `Renderer::close_soft_mask` keeps its `soft_bounds` by.

`q`, a form, a transparency group and a mask group save both, and `Q` and
their ends restore them, as the renderer saves and restores its clip and its
mask: a group's soft mask shapes its *result* (11.6.6), so it bounds what the
group paints from outside the group; a mask group draws under no clip and no
mask of its own. Hidden optional content is `Admission`'s question.

### The bounding box of marks (RC-17.2)

**A mark is a paint a render makes on the page**: a fill, a stroke, a glyph
that a fill or stroke mode paints (9.3.6, Table 106), an image or an image
mask — and the grey placeholder of one that does not decode, which is painted
where the image would have been — and an `sh`. Not marks: a clip; a glyph in
mode 3 or mode 7, and one with no outline; anything in hidden optional
content (8.11.3.2), which a render runs and does not paint; anything inside a
soft mask's group, which is a picture of a mask and not of the page; and a
fill, an image or a shading whose box has no area, which covers no pixel. A
stroke of zero width is a mark, at its path's own box, because a render draws
it as the thinnest line it can (8.4.3.2).

Each mark's box, in the page frame — default user space, in points:

| Mark | Its box |
| --- | --- |
| A fill | Its path's exact bounds: a Bézier's extremes, not its control points |
| A stroke | The outline `tinker-pdf-raster`'s stroker writes for the path under its pen — width, caps, joins, miter limit and dashes in user space, carried to the page by the CTM as `Renderer::pen` carries it, by `user_space_pen`'s choice — with no thinnest-line floor |
| A glyph | Its outline from the face through the glyph's transform, exact bounds; a stroking mode's outline widened as a stroke is; both for fill-and-stroke |
| An image | The unit square under the CTM (8.9.5.2); none under a transform with no area |
| `sh` | The clip and the soft mask it paints (8.7.4.2), the page where neither is in force; a mesh (types 4 to 7) the box of its points under the CTM, met with those |

Each is met with the clip's box, the soft mask's box and the crop box, and a
box that meets nothing is no mark. **The answer is the union of every mark's
box**, `None` for a page that paints nothing on itself:

- **in points**, `(x0, y0, x1, y1)` in default user space, inside the crop
  box — the numbers a `/CropBox` is written in;
- **in pixels**, that box carried by `tinker_pdf_render::page_view_transform`
  at the scale `Page::scales` applies — the transform the renderer draws the
  page through — rounded outward to whole pixels, at least one pixel a side,
  and clamped to `Page::pixel_size`: a `PixelRegion` of the bitmap
  `Page::render` returns for the same options.

### Uses transparency (RC-17.2)

**One reading of transparency, the one `pdfa/colour.rs` makes for ISO 19005-1
6.4 and PDF/X, taken over what one page draws.** Its five ways, each read off
the drawing rather than off the file:

1. **A transparency group**: the page's own `/Group` `/S /Transparency`, read
   by `pdfa/colour.rs`'s `is_transparency_group`, or a form the page draws
   whose group the interpreter offers (`begin_group`) — accepted or declined
   by a render's budgets alike, since the page asked for it.
2. **A soft mask set by `gs`**: a paint made while one is in force, on the
   paint or on the result of a group that encloses it.
3. **An `/SMask` on an XObject the page draws**: an image or a form whose
   dictionary carries the key, read as `pdfa/colour.rs` reads it — the key at
   all, which has no `/None` spelling on an XObject.
4. **A blend mode other than Normal** at a paint; `/Compatible` is Normal's
   synonym (11.3.5) and is Normal.
5. **A constant alpha below `1 − ALPHA_TOLERANCE`** at a paint, with
   `pdfa/colour.rs`'s tolerance: `ca` for a fill, a filled glyph, an image and
   `sh`, `CA` for a stroke and a stroked glyph.

Only what is painted counts: a paint in hidden content, a paint inside a mask
group — whose mask is read at the paints it masks — and a `gs` nothing is
painted under say nothing. A paint the clip leaves nothing of is still a
paint, and is read as made, since a render makes it. A group's own `ca` and `/BM` are read at the group,
since they apply to its result, and the alphas the interpreter resets inside
an accepted group (11.6.6) are the paints'.

### Uses overprint (RC-17.3)

K-01 stores three keys in every state it carries: `/OP`, `/op` — `/OP` where
`/op` is absent, Table 58 — and `/OPM`, the integer written. **A paint is made
with overprint when the key for its kind is true at it**: `/op` for a fill, a
filled glyph, an image and `sh` — 8.6.7's non-stroking painting operations —
and `/OP` for a stroke and a stroked glyph. The answer is
`Overprint { fills, strokes, nonzero_mode }`: whether any non-stroking paint
was made with overprint on, whether any stroking one was, and whether one of
those was made under `/OPM 1`, which lets a DeviceCMYK component of zero leave
the ink beneath it (8.6.7.4) — the mode carried as the paints settled it, not
as a dictionary wrote it. `uses_overprint()` is either of the first two. Under
the same rules as transparency: hidden content, a mask group and a `gs` nothing
is painted under say nothing, and a form's paints are the page's. It reads the
state and simulates nothing.

### Colour or grey (RC-17.4)

**A pixel's colour is what `Bitmap::to_png` writes for it**: straightened when
premultiplied, ink through 8.6.4.4's relation, Lab back to sRGB, grey
replicated — `tinker_pdf_raster::toolbox::decode`, the conversion `to_png`
makes. An ink page is therefore read as the light it stands for. A pixel whose
alpha is zero holds nothing and is not coloured; every other pixel is.

A pixel is **coloured** when its components spread — the largest of red,
green and blue less the smallest — more than `GreyThresholds::tolerance`, in
levels of 255. The page is **grey** when its coloured pixels number at most
`GreyThresholds::allowed`. The answer is
`Greyness { grey, coloured, pixels, widest }`: the verdict, the coloured
pixels counted, the pixels read, and the widest spread met, so a caller can
see how near the line a page fell.

The defaults are **a tolerance of 8 and none allowed**. A grey drawn in any
space spreads by nothing — anti-aliased black on white is grey at every
coverage, and a DeviceCMYK grey turns to equal light — so the tolerance is
there for what grey pictures carry in colour form: an image coded as colour
whose chroma wanders a few levels either side of neutral. Eight levels, about
3 %, is past that and short of the palest tint a page draws on purpose. A
caller who would rather call a page grey with a coloured logo raises
`allowed`.

What a render decides, the test inherits: a hidden layer's colour is not
drawn and not counted, a colour an opaque grey paints over is not seen, and
annotations count as the options say.

### Per-render statistics (RC-17.5)

Counted by the renderer as it draws, in `tinker_pdf_render::RenderCounts`, and
added to the operation's tally by the facade as `glyphs_shown` is — so
`Bitmap::stats` carries them, a tiling cell's renderer counts into the same
tally, and **a replay counts what a direct render counts**, at any scale,
because each is counted once a call and never once a pixel:

| `Stats` field | One for each |
| --- | --- |
| `drawn_glyphs` | glyph the renderer painted: a fill or stroke mode, an outline, outside hidden content |
| `drawn_fills` | `fill_path` outside hidden content with a path to fill, a pattern's included |
| `drawn_strokes` | `stroke_path` likewise |
| `drawn_clips` | clip installed: a `W`/`W*` path, and a text object's clip at `ET` |
| `drawn_images` | `draw_image` outside hidden content, decoded or drawn as a placeholder |
| `drawn_shadings` | `sh` whose shading the renderer painted |
| `drawn_groups` | transparency group the renderer accepted |
| `drawn_soft_masks` | soft-mask group the renderer accepted |
| `hidden_calls` | glyph, fill, stroke, image, `sh` or group that hidden optional content kept from painting |
| `decoded_images_drawn` | image drawn from decoded samples |
| `decoded_image_samples` | sample of those images: width by height by components, each draw |

A paint the clip leaves nothing of is drawn: the clip decides pixels, not
calls. None of these is a step, so `Stats::work` leaves them out, as it leaves
out the pool's two counts; `+` sums them.

## Milestones

1. **RC-17.1**, this document.
2. **RC-17.2, the box and transparency.** `crates/tinker-pdf/tests/page_analysis.rs`
   `each_answer_is_the_fixtures_known_one`: a fill, a wide stroke, a glyph of
   the synthetic face, a rotated image, a clipped `sh`, a mark in hidden
   content, a mark clipped away and one off the page each give its known box
   in points and in pixels, and each of the five transparency ways and its
   twin without it give true and false, in both builds;
   `crates/tinker-pdf/src/analysis.rs`
   `the_box_is_the_union_of_the_recorded_events` recomputes the box from the
   display-list events of pages covering every operator family; and
   `crates/tinker-pdf/tests/determinism.rs`
   `the_mark_boxes_are_the_same_on_every_target` pins every fingerprint
   fixture's box, entered in K-07's roster (`xtask/src/determinism.rs`).
3. **RC-17.3, overprint.** `page_analysis.rs`
   `overprint_is_reported_where_a_paint_uses_it`: `/OP` on a stroke, `/op` on a
   fill, both set by `gs` and never painted under, set inside a form and inside
   hidden content, and the mode at `/OPM 1`, each against its known answer.
4. **RC-17.4, colour or grey.** `page_analysis.rs`
   `grey_and_colour_are_decided_at_their_thresholds`: a grey page, a page with
   one coloured mark at allowed counts either side of its size, a near-grey
   tint either side of the tolerance, a DeviceCMYK grey, a colour image, a
   colour shading, colour hidden under an opaque grey fill and colour in a
   hidden layer, each against its known answer, the boundaries computed from
   the fixtures' own colours.
5. **RC-17.5, the statistics.** `page_analysis.rs`
   `the_statistics_count_what_the_render_drew`: a page of known counts of each
   class, some hidden and some clipped away, reports exactly those counts,
   directly and from a display list at two scales.
6. **RC-17.6a, the speed rows**, unmeasured, in MS-04's analysis group: the
   box, transparency and overprint answers for the 150-dpi text page from its
   display list, and the grey test at 72 dpi, each paired with the planning
   baseline's render of the same page; `xtask/src/bench.rs`
   `the_committed_baseline_agrees_with_the_tree` holds the bench file and
   `baseline.json` to one list. RC-17.6b takes the figures in the timing phase.

## Risks

| Risk | Mitigation |
| --- | --- |
| **A mark that paints nothing visible** — a white fill, a paint at `ca 0` | **It counts.** Geometry decides, never colour or alpha: whether a mark shows depends on what is under it, which the analysis does not read, and a white rectangle over a photograph is the most visible thing on its page. A caller who wants visible ink reads a render, as the three tests' `ink_bounds` helpers do |
| A clip is a shape and its box is a rectangle | Stated: a mark inside a clip's box and outside its path counts. The fixtures clip by rectangles, whose box is the clip |
| An axial or radial shading without `/Extend` paints less than its clip | Its box is the clip's, which the step names; the shading's own extent across one direction is unbounded, and the box does not chase it along the other |
| A stroke's box comes from a flattened outline | Flattened at a hundredth of a point, so it falls short of the curve by at most that, and fills' boxes are exact |
| Rounding outward is not the renderer's coverage | A render's anti-aliased edge can tint a pixel the rounded box leaves out by a fraction of a pixel; the box says where the geometry lands, the pixel box is that rounded out, and the fixtures put edges on whole pixels |
| Two readings of transparency drift apart | The page's reads `pdfa/colour.rs`'s group predicate and its alpha tolerance rather than copies. **They part at one place, named here:** a `/BM` this engine does not recognise is refused by PDF/A-1's rule, which judges the name as written, and draws as Normal (11.3.5), so the page, which is read off what it draws, does not use transparency there |
| The analysis replays a display list a large page will not keep | A list that is not retained interprets the page into the same device, which answers as the renderer does, so the answer is the same |
| A grey picture coded as colour carries chroma noise past the tolerance | The tolerance is the caller's; the default is the line drawn above, and `widest` says how near a page came |
| Counting slows a render | Each statistic is one addition a call, kept whether asked or not, as `glyphs_shown` is |
