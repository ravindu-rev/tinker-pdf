# Container depth

[The roadmap](../ROADMAP.md) says how a step is worked and what comes next.

Debts each container format records about itself, in its feature doc's
refusal table, and the layout engine's reach toward print-grade HTML.

## CD-01 — EPUB and HTML CSS: the thirty-seven known and unimplemented properties

**EPUB and HTML CSS: the thirty-seven known and unimplemented properties** — `animation`, `background-attachment`, `border-image`, `caption-side`, `clip`, `clip-path`, `cursor`, `empty-cells`, `filter`, `font`, `font-display`, `font-stretch`, `font-variant-numeric`, `grid` and its seven related properties (`grid-area`, `grid-column`, `grid-row`, `grid-template`, `grid-template-areas`, `grid-template-columns`, `grid-template-rows`), `justify-items`, `justify-self`, `list-style-image`, `mix-blend-mode`, `page`, `resize`, `speak`, `src`, `tab-size`, `text-emphasis`, `text-emphasis-style`, `text-overflow`, `transition`, `unicode-range`, `word-wrap`, `writing-mode` — and the counted remainders (repeating and conic gradients, translucent stops and hints, blurred shadows, 3D transforms, `opacity` as a true group, a second background layer, `reversed()`, bidi overrides and right-to-left tables, flex and multi-column), custom properties and `calc()`

**Exit criterion.** each property's warning stops firing on a reftest pair in `epub_reftest.rs`, and each landing deletes its name from the table; the order inside the row is the fetched corpus's `UnimplementedProperty` counts, highest first — **owed**: that ordering cannot be measured here, so the landings follow the row's own list and the committed counts; the corpus counts recorded (MS-13) and falling — the design's milestones 2 to 7, written with the replaced box

**Doc:** [design/epub-layout.md](../design/epub-layout.md) · **Depends on:** MS-13

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-01.2 | build | Properties with no effect on a paged rendering accepted as recognised and inert, each documented: `animation`, `transition`, `cursor`, `resize` and `speak`. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/tests/epub_reftest.rs`, `docs/features/epub.md` | Each name leaves `UNSUPPORTED_PROPERTIES`; a reftest pair with and without each declaration draws the same page. | — |
| CD-01.3 | build | Aliases and font longhands: `word-wrap` as `overflow-wrap`, the `font` shorthand, `font-stretch` and `font-variant-numeric` through the face's features, and `tab-size`. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf-css/src/longhand.rs`, `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each warning stops firing on a reftest pair against its explicit equivalent, with its mismatch reference; each name leaves the table; in the default and `bundled-fonts` builds. | — |
| CD-01.4 | build | Table properties: `caption-side` and `empty-cells`. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf-layout/src/table.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each warning stops firing on a reftest pair; both names leave the table. | — |
| CD-01.5 | build | `@font-face`'s `unicode-range` read and honoured in the per-character choice of face, `font-display` accepted as inert, and `src`, `unicode-range` and `font-display` met in a style rule classed as descriptors out of place rather than unimplemented properties. | `crates/tinker-pdf-css/src/font_face.rs`, `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/src/epub/typeface.rs`, `crates/tinker-pdf/tests/epub_fonts.rs` | A range keeps a book face off the characters outside it on a reftest pair; the three names leave the table; in the default and `bundled-fonts` builds. | — |
| CD-01.6 | build | Text decoration of overflow and emphasis: `text-overflow`, `text-emphasis` and `text-emphasis-style`. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each warning stops firing on a reftest pair, the ellipsis and emphasis marks placed where the test computes them; the names leave the table; in the default and `bundled-fonts` builds. | — |
| CD-01.7 | build | Backgrounds and lists: `background-attachment` and a second background layer, `list-style-image`, and `reversed()` with `<ol reversed>`. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf-css/src/counter.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each warning and count stops firing on a reftest pair; the names leave the table. | — |
| CD-01.8 | build | Clipping: `clip` and `clip-path`, the shapes and references as clips the painter writes. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each warning stops firing on a reftest pair, the clip geometry asserted arithmetically; both names leave the table. | — |
| CD-01.9 | build | Blending and filters: `mix-blend-mode` as PDF blend modes, and `filter`'s functions, exact ones as transparency and the rest through CD-10's rasterised primitives. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each function's output equals Filter Effects' formula computed in the test; both names leave the table. | CD-10.4 |
| CD-01.10 | build | `border-image`: the slices, widths, outsets and repeat modes. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | The warning stops firing on a reftest pair, each slice placed where the test computes it; the name leaves the table. | — |
| CD-01.11 | build | The gradient remainders: repeating and conic gradients, translucent stops through soft masks, interpolation hints and colour spaces, and lists past `MAX_CSS_GRADIENT_STOPS` as a ledger row. | `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs`, `crates/tinker-pdf/tests/bounds_ledger.rs` | Each count stops firing on a reftest pair, the shading functions equal to those computed in the test. | — |
| CD-01.12 | build | Blurred box and text shadows through CD-10's blur, and shadow lists past `MAX_CSS_SHADOWS` as a ledger row. | `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs`, `crates/tinker-pdf/tests/bounds_ledger.rs` | Each count stops firing on a reftest pair, the blur equal to the Gaussian computed in the test; in the default and `bundled-fonts` builds. | CD-10.4 |
| CD-01.13 | build | The 3D transform functions: a matrix that reduces to two dimensions drawn through `cm`, and the rest drawn as the box's content mapped through the projection at a stated resolution. | `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf-layout/src/style.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each count stops firing on a reftest pair, corner positions equal to the projection computed in the test. | — |
| CD-01.14 | build | `opacity` as one transparency group over the element's content, a form XObject the tagged writer carries marked content into. | `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/src/epub/tagging.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Content overlapping its own background composites as §15.1 says on a reftest pair, the tagged output still validating; the count stops firing. | — |
| CD-01.15 | build | Grid placement: `grid`, `grid-template` with its areas, rows and columns, and `grid-area`, `grid-row` and `grid-column`, with auto-placement. | `crates/tinker-pdf-layout/src/grid.rs` (new), `crates/tinker-pdf-layout/src/lib.rs`, `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each item lands in the cell the test computes on reftest pairs; the eight grid names leave the table. | — |
| CD-01.16 | build | Grid sizing and alignment: `fr`, `minmax()` and auto tracks, `justify-items` and `justify-self`, and a grid broken across pages. | `crates/tinker-pdf-layout/src/grid.rs` (new), `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | Track sizes equal those computed in the test, alignment places each item where the test says, a tall grid fragments at row boundaries; both names leave the table. | CD-01.15 |
| CD-01.17 | build | Vertical text: glyph orientation, vertical metrics and the vertical features, for `writing-mode`'s vertical values. | `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_shaped.rs` | Upright and rotated runs are placed at the advances the face's vertical metrics give, computed in the test. | — |
| CD-01.18 | build | Vertical block flow: lines stacked across the block axis, pages progressing as `writing-mode` and the spine direction say. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | A vertical book's lines and pages land where the test computes on reftest pairs; `writing-mode` leaves the table. | CD-01.17 |
| CD-01.19 | build | The bidi remainders: `bidi-override` and `isolate-override` with `<bdo>`, right-to-left tables, flex containers and column sets, and over-constrained or positioned boxes in a right-to-left containing block. | `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf-layout/src/table.rs`, `crates/tinker-pdf-layout/src/flex.rs`, `crates/tinker-pdf-layout/src/position.rs`, `crates/tinker-pdf/tests/epub_shaped.rs` | Each count stops firing, positions asserted in `epub_shaped.rs` as the bidi algorithm computes them. | — |
| CD-01.20 | build | Custom properties and `calc()`: `var()` substituted with cycles detected, and `calc()`, `min()`, `max()` and `clamp()` in every numeric property. | `crates/tinker-pdf-css/src/cascade.rs`, `crates/tinker-pdf-css/src/longhand.rs`, `crates/tinker-pdf/tests/epub_css.rs` | Each computed value equals the one the test computes; a cycle makes the property invalid at computed-value time, asserted. | — |
| CD-01.21 | test | `page` closed through CD-16.2's named pages. | `crates/tinker-pdf-css/src/property.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | The warning stops firing on a reftest pair; the name leaves the table. | CD-16.2 |

## CD-02 — EPUB layout refusals

**EPUB layout refusals**: `MaxHeightAsAuto`; an atomic box taller than a page in a table band, a flex line or a column; `ColumnSpanAsNone` below a container's children; a positioned box's static position inside a line (`PositionedInLine`); a column background image; `::first-line`; `content: url()`; `@page` size and margins

**Exit criterion.** each warning stops firing on a reftest pair in `epub_reftest.rs`; `@page` as H-13 rules

**Doc:** [design/epub-layout.md](../design/epub-layout.md) · **Depends on:** H-13 (`@page`)

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-02.1 | build | `MaxHeightAsAuto`: content past `max-height` on a box whose `overflow-y` is `visible` drawn over the boxes after it, a straddling child painted whole. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf-layout/src/fragment.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | The warning stops firing on a reftest pair, the overflow drawn where the test computes it. | — |
| CD-02.2 | build | An atomic box taller than a page inside a table band, a flex line or a column, sliced across pages, each slice clipped to its page. | `crates/tinker-pdf-layout/src/table.rs`, `crates/tinker-pdf-layout/src/flex.rs`, `crates/tinker-pdf-layout/src/fragment.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | The warning stops firing on a reftest pair per context, each slice's offset equal to the test's. | — |
| CD-02.3 | build | `ColumnSpanAsNone`: `column-span: all` on a box below a multi-column container's own children, lifted out and the columns balanced before and after it. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | The warning stops firing on a reftest pair. | — |
| CD-02.4 | build | `PositionedInLine`: an absolutely positioned box's static position on the line it was written in, the lines placed before positioned boxes are. | `crates/tinker-pdf-layout/src/position.rs`, `crates/tinker-pdf-layout/src/floats.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | The warning stops firing on a reftest pair, the box at the line's position the test computes. | — |
| CD-02.5 | build | A table column's background image (`ColumnBoxNotPainted`). | `crates/tinker-pdf-layout/src/table.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | The warning stops firing on a reftest pair. | — |
| CD-02.6 | build | `content: url()`: a replaced box inside a generated one, the painter keying pictures by pseudo-element as well as element. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_pseudo.rs` | The warning stops firing on a reftest pair; `::before` and `::after` with different images draw both. | — |
| CD-02.7 | test | `::first-line` closed through CD-15.3's second layout pass. | `crates/tinker-pdf/tests/epub_reftest.rs` | `PseudoElementUnsupported` stops firing for `::first-line` on a reftest pair. | CD-15.3 |
| CD-02.8 | test | `@page`'s size and margins closed through CD-16.2 and, for EPUB, CD-16.9 as H-13 rules. | `crates/tinker-pdf/tests/epub_reftest.rs` | `AtRuleUnsupported` stops firing for `@page` on a reftest pair, the page box as H-13 rules; in the default and `bundled-fonts` builds. | CD-16.9 |

## CD-03 — EPUB text past 224 characters in a standard-14 fallback face, in a build without `bundled-fonts`

**EPUB text past 224 characters in a standard-14 fallback face, in a build without `bundled-fonts`**

**Exit criterion.** a default build that loses nothing past 224, or the owner's recorded decision that the default build cannot (footprint)

**Doc:** — · **Depends on:** H-04

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-03.1 | build | As H-04 decides: a face the default build carries for characters past a simple font's codes, keyed as an `/Identity-H` composite font within the footprint budget, or the owner's decision that it cannot recorded, with the counted ceiling kept. | `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_fallback.rs`, `docs/features/epub.md` | A default build sets the 234 characters `epub_fallback.rs` sets with the feature, none lost and none a notdef, inside MS-06's footprint figure; or H-04's decision is recorded and the ceiling's assertion stays. | H-04 |

## CD-04 — MathML layout

**MathML layout** in EPUB, HTML and Markdown, and MathML carried into the structure tree

**Exit criterion.** MathML Core's layout rules held to analytic fixtures (box metrics computed in the test from the font's `MATH` table); the formulas tagged as `Formula` with MathML by namespace inclusion (SD-19)

**Doc:** `design/mathml.md` (owed) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-04.1 | design | Create `docs/design/mathml.md`: MathML Core's layout on the layout engine, the font's `MATH` table and what happens with a face that has none, the operator dictionary as data, stretchy and large operators, how `<math>` sits in a line and as a block, the structure tagging through SD-19.4, a math face committed as test data, and milestones with analytic exit tests. | `docs/design/mathml.md` (new) | The doc exists in the design-doc order, names CD-04.2 to CD-04.8's exit tests. | — |
| CD-04.2 | build | The OpenType `MATH` table read: constants, italics corrections, top-accent attachment, kerning, glyph variants and assemblies; a math face under OFL-1.1 committed as test data. | `crates/tinker-pdf-font/src/math_table.rs` (new), `crates/tinker-pdf-font/src/lib.rs`, `testdata`, `THIRDPARTY.md` | Every value read from the committed face equals the table bytes decoded by hand in the test. | CD-04.1 |
| CD-04.3 | build | MathML parsed into a box tree: token elements, `mrow`, `mstyle`, `mspace`, `mpadded`, `mphantom` and `merror`, with the operator dictionary vendored under its licence. | `crates/tinker-pdf-layout/src/math/mod.rs` (new), `crates/tinker-pdf-layout/src/lib.rs`, `THIRDPARTY.md`, `crates/tinker-pdf/tests/mathml.rs` (new) | `mathml.rs` (new): each element's box metrics equal those computed in the test from the face's `MATH` constants. | CD-04.2 |
| CD-04.4 | build | Fractions, radicals and scripts: `mfrac`, `msqrt`, `mroot`, `msub`, `msup`, `msubsup`, `munder`, `mover`, `munderover` and `mmultiscripts`, by MathML Core's rules. | `crates/tinker-pdf-layout/src/math/mod.rs` (new), `crates/tinker-pdf/tests/mathml.rs` (new) | Each layout's box metrics and child offsets equal those computed in the test from the `MATH` constants. | CD-04.3 |
| CD-04.5 | build | Stretchy and large operators through glyph variants and assemblies. | `crates/tinker-pdf-layout/src/math/mod.rs` (new), `crates/tinker-pdf/tests/mathml.rs` (new) | A stretched operator's variant or assembly and its size equal those the test computes for each target size. | CD-04.4 |
| CD-04.6 | build | Tables (`mtable`, `mtr`, `mtd`) and the remaining MathML Core elements, with `display` block and inline baselines. | `crates/tinker-pdf-layout/src/math/mod.rs` (new), `crates/tinker-pdf/tests/mathml.rs` (new) | Each layout's metrics equal those computed in the test; an inline formula's baseline sits on the line's. | CD-04.4 |
| CD-04.7 | build | `<math>` in EPUB, HTML and Markdown laid out inline and as a block, in place of the `UnimplementedFeature` tally. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/src/markdown.rs`, `crates/tinker-pdf/tests/mathml.rs` (new) | A book, an HTML document and a Markdown file with formulas draw them at the positions the test computes, with no `UnimplementedFeature` for MathML. | CD-04.5, CD-04.6 |
| CD-04.8 | build | Formulas tagged as `Formula` carrying their MathML by namespace inclusion, through SD-19.4. | `crates/tinker-pdf/src/epub/tagging.rs`, `crates/tinker-pdf/tests/mathml.rs` (new) | Each formula's structure element reads back with its MathML under the namespace; the tagged output passes SD-04's rules as they stand. | CD-04.7, SD-19.4 |

## CD-05 — Media overlays

**Media overlays** carried into the PDF as rendition actions over the synchronised text, for a host to play

**Exit criterion.** a generated book's overlay yields one rendition per synchronised fragment, pointing at the right text and clip

**Doc:** — · **Depends on:** AN-12

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-05.1 | build | Media overlays read: each content document's SMIL `par` elements with their text fragments and audio clips, and the package's overlay properties and durations. | `crates/tinker-pdf/src/epub/overlays.rs` (new), `crates/tinker-pdf/src/epub/package.rs`, `crates/tinker-pdf/tests/epub_overlays.rs` (new) | `epub_overlays.rs` (new): a generated book's overlay reads to its fragments and clips with their times. | — |
| CD-05.2 | build | Each synchronised fragment carried into the PDF as a rendition action over its text, its clip embedded, for a host to play, read back through AN-12.1's model. | `crates/tinker-pdf/src/epub/overlays.rs` (new), `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_overlays.rs` (new) | A generated book's overlay yields one rendition per synchronised fragment, pointing at the right text and clip, read back through AN-12.1. | CD-05.1, AN-12.1 |

## CD-06 — `epub:switch` and the other conditional constructs

**`epub:switch` and the other conditional constructs** honoured by their rules

**Exit criterion.** fixtures per case draw the branch the rules select

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-06.1 | build | `epub:switch` honoured: the first `epub:case` whose required namespace this build supports drawn, else `epub:default`. | `crates/tinker-pdf/src/epub/xhtml.rs`, `crates/tinker-pdf/tests/epub_reading.rs` | Fixtures per case draw the branch the rules select, and `UnimplementedFeature` no longer names it. | — |
| CD-06.2 | build | The other conditional constructs EPUB 3 defines: manifest fallback chains, and `<object>` fallback content. | `crates/tinker-pdf/src/epub/package.rs`, `crates/tinker-pdf/src/epub/xhtml.rs`, `crates/tinker-pdf/tests/epub_reading.rs` | Fixtures per case draw the item or fallback the rules select. | — |

## CD-07 — The EPUB corpus gaps

**The EPUB corpus gaps**: a producer's book declaring `@layer`, and one carrying `.woff`/`.woff2`, run through the reader once found

**Exit criterion.** a real producer's book for each draws with no unexpected warning, its result pinned

**Doc:** — · **Depends on:** H-107 (the books)

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-07.1 | test | A producer's book declaring `@layer`, found under H-107, run through the reader. | `corpus/corpora.lock`, `crates/tinker-pdf/tests/epub_fetched.rs` | On a dispatched run on `develop` the book draws with no unexpected warning, its result pinned. | H-107, MS-11 |
| CD-07.2 | test | A producer's book carrying `.woff` or `.woff2`, found under H-107, run through the reader. | `corpus/corpora.lock`, `crates/tinker-pdf/tests/epub_fetched.rs` | On a dispatched run on `develop` the book draws with no unexpected warning, its faces used, its result pinned. | H-107, MS-11 |

## CD-08 — SVG in the spine

**SVG in the spine**: `mask` and `clip-path` in `objectBoundingBox` units on a `<tspan>`; `<textPath>`, `<tref>` and `<altGlyph>`; the `epub_fetched.rs` measurement

**Exit criterion.** a `<tspan>`'s bounding-box mask and clip resolved against its `<text>`'s box, as its paint already is; `<textPath>` and its two relatives drawn, each held to a fixture whose geometry is computed in the test; `the_six_svg_spine_items_draw_rather_than_placehold` run on the corpus

**Doc:** — · **Depends on:** MS-13

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-08.1 | build | A `<tspan>`'s `objectBoundingBox` mask and clip resolved against its `<text>`'s box, the text measured before its spans' groups are built, as its paint already is. | `crates/tinker-pdf-svg/src/scene.rs`, `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | `TextBoxUnmeasured` no longer fires; the mask and clip geometry equal the test's computation from the text's box. | — |
| CD-08.2 | build | `<textPath>`: glyphs placed along the path with `startOffset`, `method` and `spacing`. | `crates/tinker-pdf-svg/src/scene.rs`, `crates/tinker-pdf-svg/src/path.rs`, `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | Each glyph's position and angle equal those computed in the test from the path's length; `TextLayoutUnsupported` no longer names it. | — |
| CD-08.3 | build | `<tref>` and `<altGlyph>`. | `crates/tinker-pdf-svg/src/scene.rs`, `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | Each draws the text or glyph its reference selects, held to a fixture whose geometry is computed in the test. | — |

## CD-09 — SVG `<switch>`

**SVG `<switch>`** conditional processing (`systemLanguage`, `requiredExtensions`)

**Exit criterion.** fixtures per condition draw the selected child

**Doc:** [design/svg.md](../design/svg.md) (amend) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-09.1 | design | Amend `docs/design/svg.md`: `<switch>` from non-goal to design, its conditions evaluated against a stated user language and the extensions this build supports. | `docs/design/svg.md` | The amendment names CD-09.2's exit test. | — |
| CD-09.2 | build | `<switch>`: the first child whose `systemLanguage`, `requiredExtensions` and `requiredFeatures` pass is drawn. | `crates/tinker-pdf-svg/src/scene.rs`, `crates/tinker-pdf-svg/src/document.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | Fixtures per condition draw the selected child. | CD-09.1 |

## CD-10 — SVG `<filter>`

**SVG `<filter>`**: the filter primitives rasterised inside the writer at a stated resolution, or expressed as PDF transparency where the primitive maps exactly (offset, flood, blend, composite, colour matrix to soft masks and groups)

**Exit criterion.** each primitive's output equals Filter Effects' formulas computed in the test; the vector text under a filter stays text where the mapping is exact

**Doc:** [design/svg.md](../design/svg.md) (amend) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-10.1 | design | Amend `docs/design/svg.md` with filters: the exact vector mappings first (offset, flood, merge, blend, composite, colour matrix to soft masks and groups), the rest rasterised inside the writer at a stated resolution; the filter region, primitive subregions, units and `color-interpolation-filters`; and what keeps the text under a filter as text. *Note:* Exact mappings as vectors, with the rest rasterised as an option, is H-31's default. | `docs/design/svg.md` | The amendment names CD-10.2 to CD-10.6's exit tests. | — |
| CD-10.2 | build | The filter graph: region and subregions in both unit systems, `SourceGraphic` and `SourceAlpha`, named results and the default chaining. | `crates/tinker-pdf-svg/src/filter.rs` (new), `crates/tinker-pdf-svg/src/lib.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | Each region and subregion equals the one computed in the test; a graph with an unknown primitive is named and drawn without it. | CD-10.1 |
| CD-10.3 | build | The exact mappings: `feOffset`, `feFlood`, `feMerge`, `feBlend`, `feComposite`'s operators and `feColorMatrix` expressed as PDF transparency. | `crates/tinker-pdf-svg/src/filter.rs` (new), `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | Each primitive's output equals Filter Effects' formula computed in the test, and text under each stays text. | CD-10.2 |
| CD-10.4 | build | The rasteriser inside the writer at the stated resolution, and the first raster primitives: `feGaussianBlur`, `feMorphology`, `feConvolveMatrix` and `feComponentTransfer`. | `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | Each primitive's pixels equal Filter Effects' formula computed in the test at the stated resolution. | CD-10.2 |
| CD-10.5 | build | The rest of the raster primitives: `feTurbulence`, `feDisplacementMap`, `feTile`, `feImage` and the two lighting primitives. | `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | Each primitive's pixels equal Filter Effects' formula computed in the test. | CD-10.4 |
| CD-10.6 | test | A filter over vector text, held both ways: kept as text where every primitive maps exactly, and rasterised where one does not. | `crates/tinker-pdf/tests/epub_svg.rs`, `docs/design/svg.md` | The extracted text of each exact case equals its source; `ArchiveWarning::Svg` no longer names a filter. | CD-10.3, CD-10.5 |

## CD-11 — SVG `<foreignObject>`

**SVG `<foreignObject>`**: the embedded XHTML laid out by the facade's layout engine (the SVG leaf crate stays PDF-free and layout-free, ruling 8)

**Exit criterion.** the embedded content equals the same XHTML laid out at that box

**Doc:** [design/svg.md](../design/svg.md) (amend) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-11.1 | design | Amend `docs/design/svg.md` with `<foreignObject>`: the leaf crate hands the subtree and its box to its caller and lays nothing out (ruling 8), and the facade lays the XHTML out at that box. | `docs/design/svg.md` | The amendment names CD-11.2 and CD-11.3's exit tests. | — |
| CD-11.2 | build | The SVG crate surfaces a `<foreignObject>`'s subtree and box to its caller, still PDF-free and layout-free. | `crates/tinker-pdf-svg/src/scene.rs`, `crates/tinker-pdf-svg/src/document.rs`, `crates/tinker-pdf-svg/src/tests.rs` | A unit test reads the subtree and the box the test computes, and the crate's dependencies are unchanged. | CD-11.1 |
| CD-11.3 | build | The facade lays the XHTML out at the box with the EPUB cascade, layout engine and painter, clipped to it. | `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/epub_svg.rs` | The embedded content's operators equal those of the same XHTML laid out at that box through `from_html`. | CD-11.2 |

## CD-12 — Scripted content documents

**Scripted content documents**: `<script>` in EPUB, HTML and SVG content run by the in-tree engine against a DOM subset before layout, deny-by-default as form scripts are

**Exit criterion.** html5lib's `#script-on` tree-construction tests pass; scripted fixtures draw their post-script DOM; budgets as FJ-04

**Doc:** `design/scripted-documents.md` (owed) · **Depends on:** FJ-03e

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-12.1 | design | Create `docs/design/scripted-documents.md`: the DOM subset, the in-tree engine's binding to it, when scripts run (before layout, to quiescence), deny-by-default capabilities as form scripts have them, FJ-04's budgets, `<noscript>`, SVG's `<script>`, html5lib's `#script-on` tests as the adjudicator, and milestones with exit tests. | `docs/design/scripted-documents.md` (new) | The doc exists in the design-doc order, names CD-12.2 to CD-12.9's exit tests. | FJ-03e |
| CD-12.2 | build | The HTML parser's scripting flag: tree construction with scripting on, `<noscript>` parsed as text. | `crates/tinker-pdf-xml/src/html/tree.rs`, `crates/tinker-pdf-xml/tests/html5lib.rs` | html5lib's `#script-on` tree-construction tests pass, with the count recorded beside the `#script-off` one. | CD-12.1 |
| CD-12.3 | build | The DOM subset over the engine: traversal and mutation, element creation, attributes, text, and lookup by ID and by selector through the CSS selector engine. | `crates/tinker-pdf/src/epub/dom_script.rs` (new), `crates/tinker-pdf/tests/scripted_documents.rs` (new) | `scripted_documents.rs` (new): each operation's effect on the tree equals the expected tree, compared node by node. | CD-12.1 |
| CD-12.4 | build | Script order: classic scripts in document order, `defer` and `async` in a deterministic order, scripts from the container only, `DOMContentLoaded` and `load`, and timers run to quiescence. | `crates/tinker-pdf/src/epub/dom_script.rs` (new), `crates/tinker-pdf/tests/scripted_documents.rs` (new) | Fixtures exercising each order record the order the design states; two runs give the same tree. | CD-12.3 |
| CD-12.5 | build | Deny by default as form scripts are: no network, storage or navigation, each refusal named, under FJ-04's budgets. *Note:* Deny by default, with hosts opting in per trigger, is H-26's and H-36's default. | `crates/tinker-pdf/src/epub/dom_script.rs` (new), `crates/tinker-pdf/tests/scripted_documents.rs` (new) | Each denied capability is refused by name; a hostile script stops at its budget with a typed warning and the document still draws. | CD-12.4, FJ-04 |
| CD-12.6 | build | EPUB: spine items with the `scripted` property run before layout and their post-script DOM laid out, in place of the `UnimplementedFeature` tally. | `crates/tinker-pdf/src/epub/read.rs`, `crates/tinker-pdf/src/epub/xhtml.rs`, `crates/tinker-pdf/tests/scripted_documents.rs` (new) | Scripted EPUB fixtures draw their post-script DOM; `<noscript>` is not drawn where scripts ran. | CD-12.5 |
| CD-12.7 | build | HTML creation runs a document's scripts when asked, by an option off by default. | `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/html_creation.rs` | A scripted HTML fixture draws its post-script DOM with the option and its unscripted DOM without. | CD-12.5 |
| CD-12.8 | build | SVG's `<script>` in spine SVG and standalone SVG, against the same subset. | `crates/tinker-pdf/src/epub/svg.rs`, `crates/tinker-pdf/tests/scripted_documents.rs` (new) | A scripted SVG fixture draws its post-script tree. | CD-12.5 |
| CD-12.9 | test | The row's suite run whole: html5lib's `#script-on` tests, every scripted fixture, and the budget fixtures. | `crates/tinker-pdf/tests/scripted_documents.rs` (new), `crates/tinker-pdf/tests/hostile_input.rs`, `docs/features/opening.md` | All pass; `hostile_input.rs` runs scripted documents with zero panics; `opening.md` no longer says no script engine. | CD-12.2, CD-12.6, CD-12.7, CD-12.8 |

## CD-13 — XPS remainder

**XPS remainder**: a `ContextColor` gradient stop whose profile this build cannot evaluate; a producer's package per feature; and XPS digital signatures, print tickets, story fragments and 3D content read

**Exit criterion.** the evaluable profile set grown with `tinker-pdf-color`'s (RC-20); a producer's package per feature in the sweep; signatures verified by SG-13; print tickets surfaced as metadata; story fragments feeding reading order

**Doc:** — · **Depends on:** RC-20, SG-13, H-106

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-13.1 | build | `ContextColor` gradient stops evaluated through each profile RC-20 makes evaluable, a CMYK press profile with no `A2B*` table and an `mAB ` one among them, rather than approximated. | `crates/tinker-pdf/src/xps/brush.rs`, `crates/tinker-pdf/src/xps/profiles.rs`, `crates/tinker-pdf/tests/xps_context_colour.rs` | Each newly evaluable fixture converts through its profile to values computed in the test, and `BrushApproximated` no longer fires on it. | RC-20.2, RC-20.13, RC-20.14 |
| CD-13.2 | test | A producer's package per row feature, from H-106, in the conservation sweep in place of the derived fixtures. | `crates/tinker-pdf/tests/xps_conservation.rs`, `crates/tinker-pdf/tests/xps_rows` | Each feature has a producer's package in the sweep, its census result pinned. | H-106 |
| CD-13.3 | build | XPS digital signatures read and verified through SG-13. | `crates/tinker-pdf/src/xps/opc.rs`, `crates/tinker-pdf/src/xps/signatures.rs` (new), `crates/tinker-pdf/tests/xps_signatures.rs` (new) | `xps_signatures.rs` (new): a signed package verifies and a tampered twin does not, each verdict naming the signed parts. | SG-13 |
| CD-13.4 | build | Print tickets at job, document and page level surfaced as metadata. | `crates/tinker-pdf/src/xps/opc.rs`, `crates/tinker-pdf/src/xps.rs`, `crates/tinker-pdf/tests/xps_opc.rs` | Each level's ticket reads back as typed metadata on a fixture. | — |
| CD-13.5 | build | Story fragments feeding reading order: the document structure and story-fragment parts mapped to the page's reading order and structure. | `crates/tinker-pdf/src/xps/markup.rs`, `crates/tinker-pdf/src/xps.rs`, `crates/tinker-pdf/tests/xps_markup.rs` | A fixture's extracted reading order follows its story fragments, not its drawing order. | — |
| CD-13.6 | build | 3D content parts read, each surfaced and named where it is not drawn. | `crates/tinker-pdf/src/xps/opc.rs`, `crates/tinker-pdf/tests/xps_opc.rs`, `docs/features/xps.md` | A package carrying 3D content reads with each part surfaced and named; `xps.md` no longer lists 3D as not read. | — |

## CD-14 — Language-conditional CSS data

**Language-conditional CSS data**: `quotes: auto` per language, `text-transform` in Lithuanian, Turkish and Azeri, `full-width` and `full-size-kana`

**Exit criterion.** each held to its specification's tables, vendored under an admitted licence

**Doc:** — · **Depends on:** H-116

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-14.1 | build | `quotes: auto` per language from HTML's per-language table, vendored under a licence the allowlist admits. | `crates/tinker-pdf-css/src/counter.rs`, `crates/tinker-pdf-css/data/quotes` (new), `THIRDPARTY.md`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each language's marks equal the table's on reftest pairs; the count against `quotes` stops firing; in the default and `bundled-fonts` builds. | H-116 |
| CD-14.2 | build | `text-transform` in Lithuanian, Turkish and Azeri from the special-casing data, and `full-width` and `full-size-kana` from their specification's tables. | `crates/tinker-pdf-layout/src/case.rs`, `crates/tinker-pdf-layout/src/unicode.rs`, `THIRDPARTY.md`, `crates/tinker-pdf/tests/epub_reftest.rs` | Each mapping equals its table's on reftest pairs; the refusals by value stop firing. | H-116 |

## CD-15 — Replaced-element and pseudo-element remainder

**Replaced-element and pseudo-element remainder**: `object-fit` and `object-position`; `::first-line` with its second layout pass

**Exit criterion.** reftest pairs per value

**Doc:** [design/epub-layout.md](../design/epub-layout.md) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-15.1 | design | Amend `docs/design/epub-layout.md`: `object-fit`, `object-position` and `::first-line` move from non-goals into the design, the second layout pass's shape and cost stated. | `docs/design/epub-layout.md` | The amendment names CD-15.2 and CD-15.3's exit tests. | — |
| CD-15.2 | build | `object-fit` and `object-position` on replaced boxes. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/epub_reftest.rs` | A reftest pair per value, the image's placement equal to the test's computation. | CD-15.1 |
| CD-15.3 | build | `::first-line` with its second layout pass: the first formatted line restyled and laid out again. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf/tests/epub_pseudo.rs` | A reftest pair per property `::first-line` admits; `PseudoElementUnsupported` stops firing for it. | CD-15.1 |

## CD-16 — CSS paged media and generated content for paged media

**CSS paged media and generated content for paged media**: `@page` with sizes, margins, `:first`/`:left`/`:right`/`:blank` and named pages, the sixteen margin boxes, running elements, `string-set`, page and pages counters, `target-counter()` and `target-text()` tables of contents, `leader()`, footnotes, bleed and marks, outlines from headings — for HTML creation (`FromHtml`), and for EPUB as H-13 decides

**Exit criterion.** each feature held to reftest pairs whose page geometry is computed in the test; a generated report's table of contents carries the right page numbers

**Doc:** `design/paged-media.md` (owed) · **Depends on:** H-13

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-16.1 | design | Create `docs/design/paged-media.md`: the page model with sizes, margins, page selectors and named pages, the sixteen margin boxes, running elements and `string-set`, the counters, `target-counter()` and `target-text()` through a second pass, `leader()`, footnotes, bleed and marks, outlines from headings, EPUB as H-13 decides, and milestones with reftests whose page geometry is computed in the test. | `docs/design/paged-media.md` (new) | The doc exists in the design-doc order, names CD-16.2 to CD-16.10's exit tests. | — |
| CD-16.2 | build | `@page` with sizes and margins, `:first`, `:left`, `:right` and `:blank`, and named pages through `page`, for HTML creation. | `crates/tinker-pdf-css/src/parser.rs`, `crates/tinker-pdf-layout/src/fragment.rs`, `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/paged_media.rs` (new) | `paged_media.rs` (new): reftest pairs per feature, each page's box and margins equal to the test's computation; in the default and `bundled-fonts` builds. | CD-16.1 |
| CD-16.3 | build | The sixteen margin boxes, running elements, and `string-set` with `string()`. | `crates/tinker-pdf-layout/src/fragment.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/paged_media.rs` (new) | Each margin box's content and position equal the test's on reftest pairs; a running element repeats on the pages the test names. | CD-16.2 |
| CD-16.4 | build | The page and pages counters, in margin boxes and content. | `crates/tinker-pdf-css/src/counter.rs`, `crates/tinker-pdf/tests/paged_media.rs` (new) | Each page shows its number and the total, as the test computes. | CD-16.3 |
| CD-16.5 | build | `target-counter()` and `target-text()` through a second pass, and `leader()`. | `crates/tinker-pdf-layout/src/fragment.rs`, `crates/tinker-pdf-css/src/counter.rs`, `crates/tinker-pdf/tests/paged_media.rs` (new) | A generated report's table of contents carries the right page numbers, with leaders filling to the test's widths. | CD-16.4 |
| CD-16.6 | build | Footnotes: `float: footnote`, the footnote area, and the call and marker pseudo-elements. | `crates/tinker-pdf-layout/src/floats.rs`, `crates/tinker-pdf-layout/src/fragment.rs`, `crates/tinker-pdf/tests/paged_media.rs` (new) | Each footnote lands at the foot of its call's page, the area's height equal to the test's. | CD-16.2 |
| CD-16.7 | build | Bleed and crop and cross marks, with the trim and bleed boxes written. | `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/paged_media.rs` (new) | The boxes and marks equal the test's geometry on a fixture. | CD-16.2 |
| CD-16.8 | build | Outlines from headings, by level and label. | `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/paged_media.rs` (new) | A generated document's outline equals its headings with the page of each. | CD-16.2 |
| CD-16.9 | build | EPUB: `@page` honoured as H-13 decides. | `crates/tinker-pdf/src/epub/read.rs`, `crates/tinker-pdf/tests/epub_reftest.rs`, `docs/features/epub.md` | A book's `@page` changes its pages exactly as H-13 rules, held by a reftest pair, and the caller's box rules otherwise; in the default and `bundled-fonts` builds. | CD-16.2, H-13 |
| CD-16.10 | test | The row's reftests run whole over HTML creation and EPUB. | `crates/tinker-pdf/tests/paged_media.rs` (new), `docs/features/epub.md` | Every feature's reftest pair passes with its page geometry computed in the test; `AtRuleUnsupported` no longer names `@page`; in the default and `bundled-fonts` builds. | CD-16.5, CD-16.6, CD-16.7, CD-16.8, CD-16.9 |

## CD-17 — Typography quality

**Typography quality**: paragraph-optimal line breaking, optical margin alignment, glyph-scaling justification, as layout options

**Exit criterion.** an optimal break equals the minimum-demerits solution computed in the test; defaults unchanged

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-17.1 | build | Paragraph-optimal line breaking as a layout option: minimum total demerits over UAX #14's break opportunities. | `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf-layout/src/uax14.rs`, `crates/tinker-pdf/tests/line_breaking.rs` (new) | `line_breaking.rs` (new): an optimal break equals the minimum-demerits solution computed in the test; with the option off every reftest is unchanged. | — |
| CD-17.2 | build | Optical margin alignment as an option: punctuation and edges protruding by a stated table. | `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf/tests/line_breaking.rs` (new) | Each protrusion equals the table's amount on a fixture; defaults unchanged. | — |
| CD-17.3 | build | Glyph-scaling justification as an option, within stated bounds. | `crates/tinker-pdf-layout/src/text.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/tests/line_breaking.rs` (new) | Each line's scale equals the one computed in the test and stays within the bounds; defaults unchanged. | CD-17.1 |

## CD-18 — HTML form controls

**HTML form controls**: `<input>`, `<select>`, `<textarea>` and `<button>` drawn with appearances, and turned into AcroForm fields on request

**Exit criterion.** each control drawn; the converted form's fields fill through the editor

**Doc:** — · **Depends on:** AN-14

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| CD-18.1 | build | HTML form controls drawn with appearances: `<input>` of each type, `<select>`, `<textarea>` and `<button>`, as atomic boxes with the user-agent style. | `crates/tinker-pdf-layout/src/flow.rs`, `crates/tinker-pdf/src/epub/paint.rs`, `crates/tinker-pdf/src/epub/ua.css`, `crates/tinker-pdf/tests/html_form_controls.rs` (new) | `html_form_controls.rs` (new): each control is drawn, its box equal to the test's computation and its state shown. | — |
| CD-18.2 | build | Controls turned into AcroForm fields on request, each with its value, options and flags, through AN-14's field types. | `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/html_form_controls.rs` (new) | Each control becomes the field the test expects, its appearance matching the drawn control. | CD-18.1, AN-14 |
| CD-18.3 | test | The converted form filled through the editor. | `crates/tinker-pdf/tests/html_form_controls.rs` (new) | Each field fills through the editor, saves, and reads back with its new value, the output passing the strict validator. | CD-18.2 |
