# Render controls

*For [ROADMAP](../ROADMAP.md) row RC-12.* A caller asks a render for less than
the page: its text alone, everything but its text, no paths, images,
shadings or clips; its text or its vectors in black; a layout-only pass that
decodes no image; a render culled to rectangles of the page; and two hints,
"do not interpolate images" and "do not cache". Each control is a decision
about which of the interpreter's calls reach the renderer, or how, and each
is made once here: which calls make up each class, what the two presets drop,
what forced black paints, what a layout-only pass draws for an image it does
not decode, which calls a rectangle cull skips, and which caches the no-cache
hint covers. Every control at its default leaves every render byte for byte
as it is.

## Scope

- **Five classes** of content — text, paths, clips, images and shadings — and
  which [`Device`](../../crates/tinker-pdf-content/src/device.rs) call belongs
  to each, a Type 3 glyph's paints and the text-clip modes among them.
- **A filtering device** in `tinker-pdf-content` that drops the calls of the
  classes it is asked to cull and forces the text or the path class to black
  (RC-12.2), and the presets *text only* and *no text*.
- **Where it stands**: every scope a render draws, on every render door
  (RC-12.3).
- **Two image switches**: layout-only and no interpolation (RC-12.4).
- **Culling to rectangles** of the page's pixels (RC-12.5).
- **The no-cache hint** (RC-12.6).
- **The speed rows** (RC-12.7a).

## Non-goals

- **`Page::to_svg` and text extraction keep every class.** The SVG writer and
  `Page::text` (and every output that reads it: the structured view, search,
  the JSON, XML and HTML formats) read the page as it is; a control is a
  render option and reaches only pixels. A caller wanting a page's text has
  `Page::text`, not a render with paths culled.
- **Classes by meaning.** A class is a kind of call, not what the call
  depicts: a rule drawn as a filled rectangle is a path, an image of text is an
  image, and an `/Artifact` is whatever its calls are. Marked content decides
  nothing here.
- **Recolouring other than black.** Forced colours for high contrast are
  RC-09.5's, the toolbox's ([bitmap-toolbox](bitmap-toolbox.md)); forced black
  composes with them as *Forced black* says.
- **A culled document.** Nothing is removed from the file: a control is a
  render option, and an edit that removes a class of content is ED's.
- **Annotations as a class.** `RenderOptions::annotations` already turns every
  appearance off; an appearance that is drawn is content like the page's, and
  each control reaches it as it reaches the page.

## Design

### The classes, and which calls make them

The interpreter hands the renderer one call per thing drawn. Each call is in
exactly one class, decided by the call and, for one rule, by where it
arrives:

| Class | Calls |
| --- | --- |
| **Text** | every `show_glyph`, in every text rendering mode 0 to 7; and every `fill_path`, `stroke_path`, `clip_path`, `draw_image` and `draw_shading` that arrives inside a text object — between a `begin_text` and its `end_text` |
| **Paths** | every `fill_path` and `stroke_path` outside a text object, a fill or a stroke painted with a pattern among them, tiling or shading |
| **Clips** | every `clip_path` outside a text object: `W` and `W*` |
| **Images** | every `draw_image` outside a text object: an image XObject, an image mask (`/ImageMask`, a stencil painted in the fill colour) and an inline image alike |
| **Shadings** | every `draw_shading` outside a text object: `sh` |

Three of those lines settle what the calls alone do not:

- **A Type 3 glyph is text.** 9.6.5 makes its glyph a content stream: the
  interpreter runs the procedure where the glyph is shown, and its paints
  arrive as fills, strokes, images and shadings with no `show_glyph` at all.
  They arrive *inside the text object* that shows the glyph, and nothing else
  can: 8.2's grammar admits no path construction, path painting, `Do` or `sh`
  inside `BT`…`ET`. So everything that draws or clips inside a text object is
  the text's — the procedure's own `W n` among it, which its own `q`…`Q`
  bounds. A text object a Type 3 procedure opens inside the one showing it
  counts as one more level, and the rule holds until the outermost `ET`.
- **A text clip is text.** A glyph in modes 4 to 7 adds its outline to the
  clip that 9.3.6 applies at `ET`; it arrives as a `show_glyph`, and the clip
  is the renderer's accumulation of those calls. Culling text drops the glyph
  in every mode, and with it its paint and its share of the clip: content a
  text clip would have bounded is drawn as if the text object had clipped
  nothing. Culling clips leaves it, since a text object's clip is its glyphs'.
- **A shading-pattern fill is a path.** A fill or a stroke whose paint is a
  pattern names the pattern and nothing else, and which kind the pattern is
  the renderer learns from the resources; the filter has none, standing in
  front of a device, which is handed calls and not resources. The path is
  what is drawn and the pattern its paint, as a colour is, so the call is a
  path's. `sh`, which paints a shading over the clip with no path at all, is
  the shading class's.

Everything else always passes: `begin_text` and `end_text`, `save_state` and
`restore_state`, marked content, forms, transparency groups, soft masks, the
budget and recursion notices, and the three questions — `begin_form`,
`begin_group`, `begin_soft_mask` — and `is_cancelled`, so the inner device
answers each as it would unwrapped and the interpreter runs what it would have
run.

### The filter

`tinker_pdf_content::cull::Culled<'d, D>` wraps a `&mut D: Device` with a
`Cull` — one switch a class, `text`, `paths`, `clips`, `images` and `shadings`,
each `true` to drop — and a `Black` — `text` and `paths`, each `true` to
force that class black. It counts the text objects it is inside. A call of a
culled class is not forwarded at all, so it never reaches the renderer: not
its paint, not its image-run flush, not its warnings. A call of a class forced
black is forwarded with its state rewritten as *Forced black* says. Every
other call is forwarded as it came.

`Cull::NONE` culls nothing, and the presets are two values of the same type:

- **`Cull::TEXT_ONLY`**: paths, images and shadings dropped, text and clips
  kept — so text shows where the page shows it, clipped as the page clips it,
  and hidden text stays hidden.
- **`Cull::NO_TEXT`**: text dropped, everything else kept.

The filter is the whole mechanism for the classes: the renderer is not told
what was culled and draws what reaches it.

### Forced black

Forcing a class black rewrites the colour of each of its calls' states before
they are forwarded, and nothing else: the fill and stroke colours become
light's black, `Rgb::BLACK`; the fill and stroke inks become `K` alone,
`[0, 0, 0, 255]` — 8.6.4.4's `0 0 0 1 k`, never a rich black; the patterns are
taken away, so a pattern fill paints black; and the colour spaces named are
cleared, the colour being no longer theirs. The alphas, the blend mode, the
transform, the text state, the line parameters, the soft mask and the stored
parameters are the call's own. So a light page paints `(0, 0, 0)`, an ink page
paints the `K` plate alone at full, and constant alpha still fades a forced
mark.

Text forced black is every `show_glyph` and every paint inside a text object;
paths forced black is every fill and stroke outside one. A Type 3 glyph's
paints are text's and turn black with it; an image keeps its own samples,
since only a stencil is painted in the fill colour, and a stencil inside text
paints black; a shading keeps its colours, having no fill colour to force.
Under RC-09.5's forced colours every paint is drawn opaque black by its
coverage before the tint, so forced black changes nothing further there; the
two compose in that order.

### Where the filter stands: every scope, every door

The filter is installed once a render, in front of the device the render
draws through, so every scope that render draws reaches it: the page's
content, each form, transparency group and soft mask's group it runs, and each
annotation's appearance, drawn after the content through the same device. A
culled class is culled inside a soft mask's group too: a mask drawn by paths
is, under *paths culled*, a mask of its group's backdrop alone (11.6.5.2),
which is what the classes say and is named here so that it is not mistaken
for a defect.

The doors are the render's four (RC-12.3): `Page::render` — and so every page
of a page walk and every patch of an edited page, whose region renders carry
the options they are handed — `Page::render_form`, `Page::render_annotation`
and `DisplayList::render`, which wraps the renderer it replays into, so a
replay under a control equals the direct render under it. A tiling pattern's
cell is drawn by a renderer of its own, and `TileRequest` carries the
controls into it as it carries `antialias`, so a cell's content is filtered as
the page's is. A display list records unfiltered and filters at each replay,
so one list serves every control.

**At its default no control installs anything**: `Cull::NONE` with no class
forced black installs no filter, and the two image switches, the rectangles and
the hint are each off. So every existing render, fingerprint and golden is
byte-identical, and `the_default_is_unchanged` in `render_options.rs` holds
it.

### Images a layout-only pass does not decode

A layout-only render (RC-12.4) asks the `GlyphSource` for no image at all,
named or inline: the cost of a page's images is their decode, and a layout
pass — where things are, not what they show — wants none of it. Each image is
drawn as ruling 2's placeholder, the neutral grey `0xBF` over the image's unit
square through the current transform, clipped as the image would have been;
that covers every pixel the decoded draw covers, a stencil's included, since a
stencil paints within its unit square. An image never decoded is never refused
and never damaged, so a layout-only render reports no `UnsupportedImage` and no
`DamagedImage` for it. A layout-only pass forms no image runs, every image
being a fill. The placeholder is the image class's, and culling images drops
it too.

**No interpolation** samples every image as if its `/Interpolate` were false
(8.9.5.3): the hint is the caller's, and an image that asked to be smoothed is
drawn as one that did not. Both switches reach tiling cells through
`TileRequest`.

### Culling to rectangles

Under `RenderOptions` rectangles of the page's pixels (RC-12.5), a paint whose
bounds miss every rectangle is not painted: a fill's bounds are its path's
with the slack a fill's region keeps, a stroke's its outline's — widened by
its pen — a glyph's its outline's box, an image's its unit square through the
transform, and `sh`'s the clip's, or the page's where nothing clips it. The
bounds are the page frame's (`Bounds`), never the canvas's, so a tile and the
page skip the same calls (ruling 5). Clips, `q` and `Q`, marked content,
forms, groups and soft masks always run, since each changes what later calls
draw.

The cull comes **after a call's image-run decision**. A fill, a stroke or a
glyph still ends the run it would end. An image whose bounds miss every
rectangle still takes its decision — it ends the run or joins it — and its
coverage joins the run's, so every later image is asked about overlap as on
the page; its samples are never drawn, and every pixel it would have touched
is outside every rectangle. Inside the rectangles, a culled render is the
whole render byte for byte.

### The no-cache hint

A render that carries the hint (RC-12.6) neither reads nor fills the caches
whose hits MS-01 counts: the font programs, the decoded images, the compiled
ICC transforms and the glyph outlines a resource scope keeps, and the glyph
masks a render keeps (P-06) — so its stats count no hit of any of them. A
display list's replay with the hint reads none of the caches the list keeps
and leaves them as they were, for the next replay without it. A cache P-16
adds to the document honours the hint the same way.

Three things stay in force, because each bounds work rather than keeps a
result: **the form-scope cache**, which builds a form's resources once however
many times a page invokes it — a page stamping one form a thousand times would
parse its fonts a thousand times without it, the shape two corpus files never
finished on; the colour spaces, functions and graphics states a scope reads
once a name (P-02's guards); and an operation's decoded streams, which are
dropped with the operation and are why a stream is decoded once a render
(MS-01's A5).

## Milestones

1. **RC-12.1: this document.**
2. **RC-12.2: the filter.** `crates/tinker-pdf-content/src/cull.rs`
   `each_class_culled_leaves_exactly_the_recorded_events_of_the_others`:
   content holding every class — a Type 3 glyph among ordinary ones, a text
   clip, an inline image, a stencil, `sh`, a shading-pattern fill, a form, a
   group and a soft mask — interpreted through the filter into a
   `RecordingDevice` records exactly the unfiltered recording less that
   class's events, for each class and both presets; and
   `forced_black_rewrites_only_its_class`, which holds every other state field
   and every call of every other class unchanged. `cargo test -p
   tinker-pdf-content`.
3. **RC-12.3: the options on every door.** `display.rs`
   `a_culled_render_is_the_display_list_without_that_class`: for each class,
   preset and forced-black switch, over a page with text in an embedded face
   and a Type 3 font, an abutting image pair with a culled call between them,
   a clip, `sh`, a tiling cell holding every class and an annotation, the
   render equals byte for byte the page's display-list events filtered as the
   filter filters them, replayed through `render_layer_with`;
   `render_options.rs` pins each control's fingerprint, entered in K-07's
   roster; `the_default_is_unchanged` and every render fingerprint stay as
   they are; both builds.
4. **RC-12.4: the image switches.** `images.rs`
   `with_the_no_interpolation_hint_an_interpolated_image_draws_as_one_without_it`;
   the render crate's `a_layout_only_render_asks_for_no_decode`, which counts
   no `image` and no `inline_image` call through a counting `GlyphSource` and
   each placeholder covering the pixels the decoded draw covers;
   `render_options.rs` pins a layout-only render through K-07's roster.
5. **RC-12.5: culling to rectangles.** `render_regions.rs`
   `a_page_culled_to_rectangles_is_the_page_inside_them`: every region
   fixture, ruling 5's image-run shape included, culled to a lattice of
   rectangles at the seven scales equals the whole render byte for byte
   inside each rectangle, a mark wholly outside every rectangle is absent, a
   tile culled to its own rectangle equals the tile, and a display-list replay
   under the cull equals the direct render under it; both builds.
6. **RC-12.6: the no-cache hint.** `display_list.rs`
   `with_the_no_cache_hint_no_cache_is_hit`: a page drawing one image four
   times, one glyph forty times and one ICC-based space twice, rendered
   directly and from a display list with the hint, counts no cache hit in
   `Bitmap::stats()` where the same renders without it count hits, and gives
   the pixels and warnings of the render without it; both builds.
7. **RC-12.7a: the speed rows**, unmeasured: a layout-only render of the JPEG
   page at 300 dpi, and the 300-dpi text page culled to one 256-pixel
   rectangle, each beside the planning baseline's whole render of the same
   page; `xtask/src/bench.rs` `the_committed_baseline_agrees_with_the_tree`
   holds them. **RC-12.7b** takes their figures in the timing phase.

## Risks

| Risk | Mitigation |
| --- | --- |
| **Ruling 5's image run**: a culled call between two images changes whether they are one run, so an abutting pair conflates or not as the filter decides | Calls of a culled class are dropped *before* the renderer, run flush and all, so the run is the one the remaining calls make — the page's display list filtered the same way and replayed is the oracle RC-12.3 holds pixels to, an abutting pair with a culled call between them among its cases. Culling to rectangles is the other way round: it skips only after the run decision, so inside the rectangles every run is the page's |
| **The form-scope cache**, which bounds the work of a page invoking one form many times, is turned off by the no-cache hint and a page stamping one form a thousand times parses its fonts a thousand times | The hint leaves it in force, with the colour spaces, functions and graphics states P-02 keeps once a name and the operation's decoded streams: each bounds work rather than keeping a result, and the hint covers only the caches whose hits MS-01 counts |
| A Type 3 glyph's paints are taken for paths and survive *no text* | Everything that draws inside a text object is the text's, which 8.2's grammar makes exact; the RC-12.2 test holds a Type 3 glyph among ordinary ones |
| A culled class still raises its warnings | A culled call never reaches the renderer, so it decodes nothing and refuses nothing; a layout-only image is never decoded and raises neither image warning |
| A control at its default moves a fingerprint | At their defaults no filter is installed and every switch is off; `the_default_is_unchanged` and every fingerprint hold it |
| A tile and the page cull different calls to a rectangle | Bounds are the page frame's, never the canvas's (ruling 5) |
| A soft mask drawn by a culled class is read as a defect | It is what the classes say — the mask is its group's backdrop — and *Where the filter stands* names it |
