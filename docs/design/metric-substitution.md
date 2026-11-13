# Metric-matched substitution

A document that does not embed a font is drawn through a face a host's
`FontProvider`, or the bundled set, hands over for it
([fallback faces](fallback-faces.md), [symbol faces](symbol-faces.md)). The
pen moves by the widths the document states (9.2.4, 9.6.2.1, 9.7.4.3), and the
face's glyphs are drawn at the face's own widths, slant, heights and weight.
Where the face is not the font the document was made with, every line comes
out with its glyphs overlapping or gapped, upright where the font slanted, and
regular where it was bold. This design settles how a substituted glyph is
fitted to the width the document gives it, and how a face is sheared, scaled
and emboldened towards what the font's descriptor states, each from the
descriptor against the face's own tables, so that a substitute stands where
the font stood without moving the pen.

The rules are read from ISO 32000-2: 9.2.4 (glyph positioning and metrics:
glyph space, the displacement, the glyph bounding box), 9.6.2.1 (`/FirstChar`,
`/LastChar`, `/Widths` and `/MissingWidth`), 9.6.2.2 (the standard 14's
published widths), 9.7.4.3 (`/W`, `/DW`, `/W2` and `/DW2`), 9.8.1's Table 120
(`/ItalicAngle`, `/CapHeight`, `/XHeight`, `/StemV`, `/FontWeight`) and
9.8.2's Table 121 (Table 123 of ISO 32000-1: `ForceBold`). From the faces'
own formats: OpenType's `hmtx`, `OS/2` (`usWeightClass`, `fsSelection`,
`sxHeight`, `sCapHeight`), `post` (`italicAngle`) and `head` (`macStyle`);
Adobe's TN #5176, the CFF specification, Table 9 (`ItalicAngle`) and Table 23
(`StdVW`); and *Adobe Type 1 Font Format*, chapters 2 and 5 (`/FontInfo`'s
`/ItalicAngle`, the Private dictionary's `/StdVW`). The facts each face
states are read by the readers the [font measurement API](font-metrics.md)
answers from: `Sfnt`'s style facts and `Cff`'s and `Type1`'s Top DICT and
`/FontInfo` facts (FT-16.2 and FT-16.3).

Ruling 23 holds the owner's answer to H-147: the synthesis is **opt-in**.
FT-14.2 to FT-14.4 land behind the switch, off, and no fingerprint, golden or
pinned render moves.

## Scope

- **The switch** (FT-14.2): an option on the document, off by default, that
  turns everything below on.
- **The per-glyph horizontal fit** (FT-14.2): a substituted glyph scaled
  horizontally so that its advance is the width the document states.
- **The italic angle as a shear** and **x-height and cap height as vertical
  scales** (FT-14.3), each from the descriptor against the face.
- **Weight as emboldening** (FT-14.4): an outline offset in the font crate,
  and the amount it moves, from the descriptor's `/StemV`, `/FontWeight` and
  the font's bold flag or name against the face's own stem and weight.
- **Their order and their place** in the outline seam, which outputs draw
  them, and what an embedding of a fitted face draws (FT-12).
- **The price, the default and the proof**: FT-14.5a's list of every pin the
  switch moves and its timings, H-147's answer landed (FT-14.6), the speed row
  (FT-14.7) and the corpus run (FT-14.9).

## Non-goals

- **An embedded program.** A document's own program is drawn as it is
  written, whatever its widths say; only a face the engine substituted for a
  program the document does not carry is ever transformed.
- **Type 3 fonts.** Their glyphs are the document's own content streams and
  are never substituted (`fonts::is_substitutable`).
- **Vertical writing.** A font that writes vertically moves its pen by `/W2`'s
  vertical displacement (9.7.4.3), which no horizontal scale meets, and its
  descriptor's heights describe horizontal text; its glyphs are drawn as the
  face draws them.
- **Choosing the face.** Which face stands in is the provider's
  ([fallback faces](fallback-faces.md), FT-13); this design transforms the one
  it is handed.
- **Undoing a face.** An italic face is not straightened for an upright font,
  a bold face not thinned for a regular one, a wide face not condensed beyond
  what the fit does glyph by glyph.
- **Hinting.** The transforms change the em outline; a hinter fits a face's
  program to the pixel grid and a transformed glyph is no longer the shape its
  program's instructions were written for (*The order, and where in the
  seam*).
- **Text extraction.** It places glyphs by the document's widths and reads no
  outline, so nothing here changes what a page's text reads.

## Design

### The switch

```rust
// tinker_pdf::OpenOptions
pub fn with_metric_matching(self, on: bool) -> OpenOptions;   // off by default
// tinker_pdf::Document
pub fn with_metric_matching(self, on: bool) -> Document;
pub fn metric_matching(&self) -> bool;
```

A property of the document, as its provider is, and not of a render: every
output that draws the document's substituted glyphs draws them one way.
`Document::with_metric_matching` hands back a document whose cache, where it
has one, is its own, as `Document::with_fonts` does, so an outline or a glyph
mask kept under one setting is never drawn under the other. Off, every page
is the bytes it is today; that is the test each of FT-14.2 to FT-14.4 carries.

### Which glyphs are transformed

A glyph is transformed when its program is a substitute
(`FontProgram::substituted` in `resources.rs`), the switch is on, and the font
writes horizontally. Each transform below then has its own condition, and a
glyph none applies to is drawn as the face draws it.

### The fit (FT-14.2)

**The width the document states.** The width `width_of` answers as exact
(`cos::font::Font::width_of`): a simple font's `/Widths` entry for the code,
where the code is inside `/FirstChar` to `/LastChar` and the entry is a
number; a composite font's `/W` entry for the code's CID; and a standard-14
font with no `/Widths`, its published width (9.6.2.2), which the pen moves by
as it moves by a stated one. `/MissingWidth` (9.6.2.1), `/DW` (9.7.4.3), a
standard-14 width approximated from a letter's base letter, and the 600 a
Symbol or ZapfDingbats name the published metrics do not list are defaults,
not the width of the glyph — `width_of` answers each as inexact — and a glyph
that takes one is not fitted.

**The face's own advance.** What the face's own metrics give the glyph, in
em: `hmtx`'s advance over `head`'s units per em for an sfnt, whatever its
outlines; the charstring's width times the font matrix's `a` — the glyph's
own, for a CID-keyed CFF — for a bare CFF or a Type 1 program.

**The map.** With `w` the stated width in em (the entry divided by 1000) and
`a` the face's advance in em, both positive and finite, every point of the
glyph's outline, on and off the curve, moves to

```text
x' = (x / a) × w      y' = y
```

the division first, so the advance's end, `x = a`, lands on `w` exactly: `a /
a` is 1 in IEEE 754 arithmetic, and `1 × w` is `w`. The origin is the glyph
origin, `x = 0`, the left edge of the advance box, so the left side bearing
scales with the glyph and the box `[0, a]` becomes `[0, w]`: the glyph's
advance is the width the pen moves by.

**What is not fitted.** A face advance of zero — a mark the face draws with
no advance — has no ratio to take, and a stated width of zero for a glyph the
face advances, or a negative one, would collapse or mirror it; each is drawn
as the face draws it, as is a glyph whose advance the face does not state.

### The italic angle (FT-14.3)

**The descriptor's angle.** `/ItalicAngle` (Table 120): degrees
counterclockwise from the vertical, negative for a font that slopes right.

**An upright face.** A face is upright when it states no slant: its own angle
is zero — `post`'s `italicAngle` for an sfnt, the Top DICT's `ItalicAngle` for
a bare CFF, `/FontInfo`'s `/ItalicAngle` for a Type 1 program, each its
specification's default of zero where absent — and, for an sfnt, `OS/2`'s
`fsSelection` sets neither `ITALIC` (bit 0) nor `OBLIQUE` (bit 9) and `head`'s
`macStyle` does not set italic (bit 1). A face that states a slant in any of
them is not sheared again, whatever its angle.

**The shear.** An upright face drawn for a font whose angle `θ` is non-zero,
finite and within 45 degrees of the vertical is sheared about the baseline:

```text
t  = tan(−θ × π / 180)      x' = x + y × t      y' = y
```

`tinker_pdf_math::to_radians` and `tinker_pdf_math::tan`, never the platform's
(ruling 4). The baseline does not move, so neither does the advance box; an
angle past 45 degrees describes no text face and is taken as the descriptor's
error.

### Heights (FT-14.3)

**The descriptor's heights.** `/XHeight` and `/CapHeight` (Table 120), in
glyph space, each divided by 1000 into em. Zero or absent is unknown (Table
120's default for `/XHeight` is 0).

**The face's heights.** An sfnt's `OS/2` `sxHeight` and `sCapHeight` where its
version is 2 or later and the value is positive; otherwise, and for a bare CFF
or Type 1 program, the top of the face's own glyph for `x` (U+0078) and for
`H` (U+0048) — its `glyf` header's `yMax`, or the top of its outline's box as
the measurement API defines a CFF or Type 1 glyph's (`docs/design/font-metrics.md`,
*Glyph boxes*) — reached through the face's `cmap`, or by the names `x` and `H`
in a program that names its glyphs. In em: an `OS/2` value over `head`'s units
per em, a glyph's top as its outline is drawn.

**Which glyphs.** A glyph whose code's text (`Font::text_of`) is exactly one
character is scaled by that character's `General_Category` in the vendored
`UnicodeData.txt` (field 2, read by `tinker_pdf_shape::unicode`): `Ll` takes
the x-height's ratio, `Lu` and `Lt` the cap height's. Every other glyph —
digits, punctuation, marks, letters of other categories, a code whose text is
empty or several characters — is not scaled.

**The scale.** `k = descriptor height / face height`, applied about the
baseline, `y' = y × k`, where both heights are positive and `1/2 ≤ k ≤ 2`.
Faces' x-heights and cap heights lie within a few tenths of each other as a
fraction of the em, so a ratio past a factor of two is a descriptor in error
rather than a difference of faces, and the glyph is not scaled.

### Weight (FT-14.4)

**What the font asks.** Three statements, each in thousandths of an em:

- `/StemV` (Table 120), where it is positive, finite and at most 500 — zero is
  Table 120's "unknown", and half an em is no stem: `T₁ = /StemV`.
- A weight: `/FontWeight` (Table 120, PDF 1.5) where it is one of 100 to 900;
  else 700 where the font asks for bold as `FontRequest::bold` reads it —
  `/Flags` bit 19 (`ForceBold`), `Bold`, `Black` or `Heavy` in its name, or a
  `/StemV` of 120 or more; else none. Where it exceeds the face's weight,
  `T₂ = S × weight / face weight`: a stem in proportion to the weight class,
  the scale `usWeightClass` and `/FontWeight` both measure weight on.
- The target is the larger of `T₁` and `T₂` where either is defined.

**The face's stem and weight.** The face's weight is `OS/2`'s
`usWeightClass` where it is 100 to 1 000, else 400. Its stem `S`, in
thousandths of an em:

- a bare CFF's or an OpenType CFF face's Private DICT `StdVW`, or a Type 1
  program's Private `/StdVW`, where positive;
- otherwise measured: the face's glyph for `l` (U+006C), else for `I`
  (U+0049), is cut by the horizontal line halfway between its box's bottom and
  top; each curve flattened into sixteen lines through its points at `t = i /
  16`, evaluated by de Casteljau's construction, each line crossing the cut
  where `y₀ ≤ y < y₁` or `y₁ ≤ y < y₀`, at `x₀ + (y − y₀) × (x₁ − x₀) / (y₁ −
  y₀)`, with its direction's sign; the crossings sorted by `x` and summed, the
  ink where the sum is not zero (glyphs fill by the non-zero rule); `S` is the
  width of the leftmost run of ink. A face with neither glyph has no stem and
  is not emboldened.

**The amount.** Where the target `T` exceeds `S`, the outline is offset by
`h = (min(T, 2 × S) − S) / 2` on each side, in em: the stem becomes `T`, never
more than twice the face's own, so no counter loses more than half a stem on
each side.

**The offset** (`Outline::embolden` in `crates/tinker-pdf-font/src/outline.rs`).
The construction of stem darkening's `Path::offset`
([hinting](hinting.md#the-offset)), on a glyph's outline in glyph space, with
the vertical amount zero:

- **Horizontal only.** Each point moves by `h × m.x`, `m` the miter vector of
  its two edges' outward normals in its contour's control polygon, held at
  four times the amount at a spike and kept at a reversal; a straight vertical
  edge moves by exactly `h`, and nothing moves vertically, so the baseline, the
  x-height and the cap height stay where the heights put them, and the only
  stem the descriptor is required to state, `/StemV`, is the one met. A bold
  weight thickens a face's horizontals less than its verticals, and they keep
  the face's weight.
- **Holes.** A contour whose signed area has the sign of the glyph's total is
  an outer contour and grows; one of the other sign is a counter and shrinks,
  by the same amount, as a stem gains it.
- **A contour that would turn inside out keeps its points.** Where a counter
  is narrower than `2h`, its offset crosses itself; a contour whose signed
  area after the offset has the other sign from before, or is not finite,
  keeps the points it had (ruling 1), so a narrow counter stays open and
  unthickened rather than filling with a sliver.
- **Self-intersection.** Contours that overlap one another — a stem drawn
  across a bar, as overlapping contours of a variable face's default are —
  are each offset on their own and the non-zero fill unions what they cover,
  so the union grows as each part does. A single contour that crosses itself
  is offset as one ring: a lobe wound against the glyph's total shrinks where
  a lobe wound with it grows, since the fill does not say which lobe is which.
- **Its place.** The offset moves the ink into the side bearings and leaves
  the advance box as it was, so the emboldened glyph is fitted, sheared and
  scaled as the face's glyph would be, and the pen moves by the stated width.

A glyph whose stem is straight, measured at half its height after the offset,
is `T` to within 10⁻⁹ em: the offset moves each straight edge by `h` in exact
arithmetic, and the bound is the rounding of the few operations that move it.

### The order, and where in the seam

Each glyph's em outline, as `PageResources::extract_outline` reads it from the
substituted face whatever branch read it — `glyf`, `CFF `, `CFF2` or a Type 1
program — takes the transforms in one order:

1. **Embolden** by `h`, in the face's own shape, so the normals are the
   face's;
2. **scale vertically** by `k`;
3. **fit** horizontally, `x' = (x / a) × w`;
4. **shear** by `t`, last, so the slant is the descriptor's angle on the
   drawn heights and widths, and the baseline, which the shear does not move,
   keeps the fitted advance box `[0, w]`.

Steps 2 to 4 compose to `x' = (x / a) × w + y × k × t` and `y' = y × k`. The
facts each step reads — the descriptor's angle, heights, stem and weight, and
the face's — are read once a font, beside the program in the scope's cache,
so a glyph pays its transform alone; the transformed outline is what
`PageResources::outline` keeps under `(font id, code)`. The fit is per glyph
in glyph space and carries no tile origin, so a tile draws the page's glyph
(ruling 5). FT-10 and FT-11 also edit `extract_outline`; the transforms run on
the outline any branch returns, after it, so whichever lands later keeps the
others' branches.

**What draws it.** Every output that reads a glyph's outline through
`GlyphSource::outline`: the renders, their tiles and bands, the display list
and the command stream, which record glyphs and draw through the renderer, the
page analysis's mark boxes, and `to_svg`. A glyph these transforms change is
drawn from its transformed em outline whatever `RenderOptions::hinting` says;
hinting's per-size seam, as [hinting](hinting.md#the-per-size-request) gives
it, takes its glyph through `selection`, which answers none for a substitute,
so no substitute is hinted. Stem darkening (FT-04a) offsets whatever outline
it is handed, a transformed one included.

### What a render reports

**Nothing new.** A glyph a transform cannot take — no stated width, no
descriptor height, a ratio out of its band, no reference glyph for the stem —
is drawn as the face draws it, silently: what it lacks is not a fault of the
page, and a render reports what it reported before, the `UnreadableFont` of a
font no face answered among it. So the corpus count "rendered with something
reported" cannot rise for this, and FT-14.9 holds it.

### The same bits on every target

Every step is IEEE 754's four operations, `sqrt` (the offset's normals, which
the standard rounds correctly), `floor`-free comparisons, and, for the shear,
`tinker_pdf_math::to_radians` and `tinker_pdf_math::tan`, so the facade gains
its edge to `tinker-pdf-math`, declared in `xtask`'s graph, and `cargo xtask
libm` stays green (ruling 4). The flattening evaluates fixed `t`s by de
Casteljau's construction, the same operations in the same order on every
target.

### An embedding of a fitted face

FT-12 embeds a provider's face for a font the document does not embed, keeps
`/Widths`, and sets each drawn glyph's advance in the program to the stated
width (FT-12.4). An embedded program is drawn as written and never
transformed, so a face embedded as it is would draw on reopening otherwise
than the document drew before the save. **So an embedding of a face the
switch transformed writes the transforms into the program**: each drawn
glyph's outline as this design leaves it — emboldened, scaled, fitted and
sheared — and its advance the stated width, at the program's own precision (a
`glyf` point rounded to the nearest font unit, a CFF operand at its 16.16
precision), and a glyph none applied to as the face has it. The saved document
then draws its glyphs within half a font unit a point of the transformed
render, with the switch off or on, since what it draws is its own program.
FT-12's pinned fingerprints are taken with the switch off; whichever of FT-12
and this row lands later carries the rule and its test.

## Milestones

1. **FT-14.2, the fit, behind the switch.** Exit: `metric_substitution.rs`'s
   `substituted_advances_equal_widths_exactly` — for every code a page draws
   through a face synthesised in the test whose advances differ from
   `/Widths`, the fitted glyph's advance equals the stated width exactly and
   its outline is the face's scaled by that ratio, in rendering and `to_svg`;
   switch off, every page byte-identical to today; `render_regions.rs` keeps
   tiles byte-equal; default and `bundled-fonts` builds.
2. **FT-14.3, the shear and the heights.** Exit: `metric_substitution.rs`'s
   `an_upright_face_takes_the_descriptors_italic_angle` and
   `x_height_and_cap_height_meet_the_descriptor`: outline points equal the
   face's under the shear and scales computed in the test with
   `tinker-pdf-math`; a face already italic is not sheared again; switch off,
   unchanged; default and `bundled-fonts` builds.
3. **FT-14.4, emboldening.** Exit: `outline.rs`'s
   `an_emboldened_stem_is_wider_by_the_offset`, over contours of both
   windings, and `metric_substitution.rs`'s
   `a_regular_face_stands_in_for_a_bold_font_at_its_stem`: the drawn stem
   measured from the outline equals the descriptor's `/StemV` within this
   design's bound while the advance still equals `/Widths`; switch off,
   unchanged; default and `bundled-fonts` builds.
4. **FT-14.5a, the price.** Exit: `metric_substitution.rs`'s
   `the_listed_pins_are_the_ones_that_move` (`#[ignore]`d, with a `RAN`
   banner) renders every listed page on and off and fails if a page outside
   the list moves or one on it does not; the timings recorded with the command
   that regenerates them. FT-14.5b takes the wall-clock figure in the timing
   phase; its exit is the figure committed and `cargo xtask baseline` holding
   it.
5. **FT-14.6, H-147's answer.** The switch stays opt-in (ruling 23), and
   `tpdf` and the corpus runner's two face-bearing runs turn it on. Exit:
   `xtask`'s `the_face_bearing_runs_ask_for_metric_matching`; every test passes
   in the default and `bundled-fonts` builds; `determinism.rs`'s new page drawn
   with the fit, a shear and emboldening holds in
   `rendering_is_stable_across_targets`.
6. **FT-14.7, the speed row.** Exit: `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` with the operation that
   renders the substituted text page at 150 dpi, and `cargo bench -p
   tinker-pdf` reporting its time.
7. **FT-14.9, the corpora.** FT-14.8, the watch step that held the row's
   corpus exit, left with the corpus workflow. Exit: a local `cargo xtask
   corpus-run --check` over the fetched corpora, in its three settings, prints
   no regression line for "rendered with something reported".

## Risks

| Risk | Mitigation |
| --- | --- |
| A dictionary whose `/Widths` belong to another font stretches every glyph | The pen moves by those widths either way; fitted, each glyph stands in the space the pen gives it rather than overlapping its neighbour. Opt-in, and FT-14.5a prices it |
| A face that is italic without saying so is sheared again | Its `post` angle, `fsSelection` and `macStyle` are all read, and any one of them stating a slant is enough to leave it |
| An ascender or descender scales with its letter's x-height | Stated: the descriptor states no ascender height to hold them to, and the ratio is bounded to a factor of two |
| Emboldening closes a counter | The amount is at most half the face's stem a side, and a contour that would turn inside out keeps its points |
| A descriptor's `/StemV` is a producer's constant rather than the font's | The larger of the stem and the weight is the target, so a bold name is not overruled by a light constant; a stem past half an em is ignored |
| The fit's division lands an ulp off the stated width | The division is taken first, so `a / a` is exactly 1 and the advance's end exactly `w` |
| A glyph mask kept with the switch off is drawn with it on | The switch is the document's, and a handle with the other setting has a cache of its own |
| A target draws the shear differently | `tinker-pdf-math`'s tangent and `cargo xtask libm` (ruling 4); FT-14.6's page in `rendering_is_stable_across_targets` |
