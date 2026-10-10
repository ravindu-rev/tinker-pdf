# Engineering rulings

Cross-cutting decisions that bind more than one feature. When a feature doc
and a ruling disagree, the ruling wins. **Numbering is frozen**: source
comments cite these by number (342 citations across the tree as of
August 2026), so a retired ruling keeps its number and is marked retired;
new rulings append. Every time two features could plausibly answer the same
question differently, the answer gets a numbered ruling here instead of
living in one feature's head.

## Rulings

1. **Never panic on untrusted input.** Binds every crate. Malformed bytes
   produce errors or warnings, never a panic; the rule is enforced by the
   per-format fuzzers ([verification.md](verification.md)), and a fuzz crash
   is a release blocker, not a backlog item.

2. **Degrade, don't fail.** Binds [filters](features/filters.md) and
   [rendering](features/rendering.md). A missing capability (a JBIG2 symbol
   dictionary, a refused JPX marker, full ICC) renders a neutral placeholder
   and appends a structured warning to `Bitmap.warnings`. A page render never
   hard-fails because one image used a rare codec.

3. **Capability scheduling is evidence-driven.** Binds
   [filters](features/filters.md), [rendering](features/rendering.md) and the
   [roadmap](ROADMAP.md). A deferred capability is implemented when the
   corpus hit-rate report ([verification.md](verification.md)) says real
   documents need it — not before, however interesting it looks.

   **Amended 9 October 2026, by the owner's decision on the roadmap's
   scope.** The owner decided that this engine has every capability the field
   offers, and more: every capability a mature, widely deployed PDF engine
   offers and every capability the rest of the field offers, with speed
   verified by tests, form JavaScript a complete implementation, signing with
   a private key in the engine, and OCR in scope and last. **Every capability
   that decision covers is in scope** — the [roadmap](ROADMAP.md)'s rows of
   9 October 2026 record them — and none waits for a corpus to ask for it. What this ruling still decides is the order: inside
   a tier, corpus hit-rate evidence puts the row a real document needs before
   the row none has asked for yet, and a capability whose count is zero is
   late in its tier rather than refused. The tiers themselves run in the
   order the roadmap states.

   What the amendment does not change is the half that kept this ruling
   honest. A count is still what a scheduling argument cites, and "however
   interesting it looks" still binds the order inside a tier; a capability
   not yet built still refuses by name (ruling 2) until its row closes; and a
   scope choice is never filed as a limit — the roadmap's named non-goals
   hold only what is outside this repository's power, and a capability
   declined for want of a count is a row, however late its tier. The
   amendment is recorded here rather than left in the roadmap's preamble
   because a ruling is amended by the owner, not drifted past; the roadmap's
   last section asks the owner to ratify this wording.

4. **Determinism is a contract, not a hope.** Binds
   [rasterizer](features/rasterizer.md) and
   [rendering](features/rendering.md). Fixed-point coverage accumulation,
   integer inner loops, no platform libm on any pixel path; the same input
   produces bit-identical bitmaps on linux, windows, macos and wasm, and the
   committed fingerprints prove it ([determinism](features/determinism.md)).

   The rule has a crate and a check behind it. `tinker-pdf-math` supplies
   `sin`, `cos`, `atan2`, `ln`, `powf` and their relatives, built from
   nothing but the operations IEEE 754 pins exactly, and it is `no_std` so
   that `x.sin()` does not compile inside it. `cargo xtask libm` fails the
   build if any pixel-path crate calls a platform transcendental.

   The boundary is worth stating precisely: `sqrt`, `floor`, `ceil`,
   `round`, `trunc` and `abs` **are** correctly rounded by the standard, so
   they are identical on every target and pixel-path code may use them
   freely. It is only the transcendental family that diverges.

5. **Tiles share the full-page code path.** Binds
   [rasterizer](features/rasterizer.md) and
   [rendering](features/rendering.md). A clipped render is the same pipeline
   over a smaller canvas that stands at its corner of the page — never a
   second implementation, and never a second frame. A tile must be pinned
   byte-equal to the full-page subregion, and that test is the permanent
   guard.

   *Corrected 13 September 2026; satisfied, with one stated exception,
   15 September 2026; that exception closed 26 September 2026, and a
   second, geometric rather than a scale, stated and pinned 2 October 2026.*
   The correction is kept rather than overwritten,
   because it is the reason the present entry can be read at face value.
   This ruling used to say the tile rows "are pinned byte-equal" and that
   the test "is the permanent guard", in the present tense, when **there was
   no such test in the tree** and there never had been: a sweep for a tile
   or region test found nothing, the facade had no `RenderOptions::region`
   to write one against, and the one nearby citation in
   `crates/tinker-pdf-render/src/lib.rs` cited ruling 7 rather than this
   one. A ruling that describes a guard nobody wrote is worse than a ruling
   with no guard, because it stops anyone looking.

   **The guard is `crates/tinker-pdf/tests/render_regions.rs`.**
   `every_tile_is_byte_equal_to_the_page_under_it` renders each of ten
   fixtures whole and then one tile at a time through
   `RenderOptions::region`, and asserts each tile equals its rectangle of
   the whole render byte for byte, with no tolerance. The first ten are
   text, a diagonal axial shading, an image and strokes — four different
   paths through the rasterizer, only one of which the scanline filler
   anti-aliases — three of them turned a quarter or three-quarter turn, two
   with a crop box offset from the media box on both axes, and one with both
   at once; six more put a canvas somewhere inside the page — transparency
   and soft-mask groups, a tiling pattern's cell and a Gouraud mesh's
   buffer. The tile sizes are 64, 37 and 23 against a 91×131 page, so no
   lattice divides it and the last row and column are always partial.
   `tiles_at_other_scales_are_byte_equal_too` repeats the whole set at 0.5×,
   2× and 4×, and
   `tiles_at_the_scales_where_two_frames_rounded_apart_are_byte_equal` at
   0.75×, 1.5× and 3×; `single_pixel_tiles_are_byte_equal` runs a one-pixel
   lattice; `a_stroke_whose_corners_sit_on_a_sub_scanline_tiles_exactly_at_1x`
   holds the stroke described below; `annotations_are_tiled_with_the_page`
   covers the annotation layer, which no other fixture there draws from.

   **Every fixture in the guard is byte-equal at every scale** — which is
   not the same claim as "a tile is byte-equal to the page", and the one
   page shape known to part them is named below. From 15 to 26
   September 2026 even the guard's fixtures were not, and what the exception
   was is kept because
   it is the reason the mechanism is what it is. A tile used to be drawn
   in a frame of its own — the page's transform with a whole number of
   pixels taken off `e` and `f` — which is exact as arithmetic and not as
   floating point: `fl(u + e)` and `fl(u + e − tx)` are two roundings at
   two magnitudes, and the last ulp reached a byte wherever the exact value
   sat on one of the rasterizer's grids. Measured then: the axial shading
   at 3× differed on one pixel of 107 289 by one level, because a shading
   sampler goes from the affine to a colour with nothing to absorb an ulp;
   and a stroked rectangle (`3 3 54 34 re` at width 4, found by
   `render_parts.rs`) differed **at 1×** on its four miter corners by 15
   levels, because `fill`, which rounds a crossing's `x` to the nearest
   1/256 and so is immune across, takes an edge's first sub-scanline as
   `ceil(y × 16)` and so was not immune down. The ruling said `fill` was
   immune; that was true of one axis.

   **The mechanism now is one frame.** The page is drawn through the one
   transform whatever part of it is asked for; the canvas carries its
   origin (`Canvas::origin`), every mask, sampled coordinate, mesh buffer
   and image run is in device pixels of the page, and only the index into a
   canvas subtracts the corner — an integer subtraction, which cannot
   round. A group's buffer and a soft mask's stand in the same frame rather
   than translating `base`. So every coordinate a tile computes is the
   number the page computes, and the sampler, the filler's `ceil`, the
   image's quad and the mesh all see one lattice.

   **What one frame does not reach, and what reaches it instead.** A few
   things are decided by a *rectangle* rather than pixel by pixel, and a
   mask in a tile only knows the tile's rectangle: which cells of a tiling
   lattice are composited (a cell's rounded-out buffer spills a pixel past
   its box, so a lattice indexed from the tile dropped a cell the page
   kept — 10 levels at 1×), which neighbours a mesh's anti-aliased fringe
   takes its colour from (two pixels of them — 10 levels at 1×, 30 at 4×),
   and whether a lattice, a mesh or an image run is within its budget. The
   renderer keeps, beside each clip and soft mask, the rectangle it would
   have on a render of the whole page, computed from nothing but device
   coordinates (`Bounds`), and takes all of those decisions there. Both
   defects were found by the canvases-in-the-page fixtures the day the
   frame became one.

   **One decision is still the canvas's, so this ruling has an open
   exception, and it is pinned.** Whether an image joins the run of images
   held back so that abutting ones do not conflate is decided by whether it
   *overlaps* what the run already holds, and the run holds fragments only
   over the canvas. Two images that overlap outside a tile and abut inside
   it are one run in the tile and two on the page, and the abutting edge
   then conflates on the page and not in the tile. On 26 September 2026
   this paragraph said no fixture had that shape and called the ruling
   satisfied; a review on 2 October built one in three images — a 100×100
   page, the first and third abutting at `x = 40.5` across the top forty
   rows, the third overlapping the second at the bottom — and the tile over
   the top half differs from the page on 40 pixels, all of column 40, by
   **63 levels at 1×**.
   `render_regions.rs`'s
   `an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`
   pins exactly that, so the day it changes the test says so, and the
   ROADMAP keeps the row ("A tile byte-equal to the page when an image
   run's overlap falls outside it") that a satisfied ruling would have had
   no need of. What reaches it is holding the run's coverage over the whole
   page rather than over the canvas, which costs a tile the image work of
   the page — every image of a run walked over the page frame, per tile —
   and that trade is the row's to make, not a fix's to slip in.

   *What the guard found on its first run is worth recording, because none
   of it was about tiles:* two defects in `tinker-pdf-raster`'s scanline
   filler that were wrong for a whole page exactly as much as for a tile,
   and that nothing else in the tree could see
   ([rasterizer](features/rasterizer.md)).

6. **Destinations are an enum, everywhere.** Binds
   [document-model](features/document-model.md),
   [writing](features/writing.md), [creation](features/creation.md).
   `Explicit | Named | Uri` — reading never collapses them, writing
   round-trips them. A URI destination is never rewritten into a named one
   and a named one is never flattened to explicit, because the collapse
   loses information a round-trip cannot recover — and it must hold on both
   sides of the API forever.

7. **The `Device` trait is the only seam between interpretation and
   consumers.** Binds [content-and-text](features/content-and-text.md) and
   [rendering](features/rendering.md). Text extraction, rasterization and
   SVG output are all devices; nothing reaches around the interpreter to
   read content streams directly, except the `ContentFilter` rewrite path
   built for redaction ([editing](features/editing.md)) — which is itself
   part of the interpreter.

8. **Leaf crates stay PDF-free.** Binds every leaf crate. Bytes and plain
   parameters in, bytes and values out; no COS types, no PDF-spec vocabulary
   in their public APIs. This is what keeps them independently fuzzable,
   testable and publishable.

   A leaf is defined, not enumerated: any crate that takes bytes and plain
   parameters and returns bytes and values is a leaf, whatever any list
   says — the list drifted twice before the definition was made the rule.
   As of August 2026 there are twelve: `filters`, `crypto`, `font`, `color`,
   `raster`, `math`, `zip`, `xml`, `css`, `layout`, `pki`, `shape`. Five
   leaf-to-leaf edges exist (`font → filters`, `zip → filters`,
   `layout → css`, `pki → crypto`, `shape → font`), and a leaf-to-leaf edge
   does not weaken this ruling, which is about public APIs rather than about
   edges. `tinker-pdf-layout` is the one leaf whose input
   is not bytes — a caller hands it a tree of plain structs — so it is a
   leaf on the definition rather than on the shape, and its fuzz target
   drives a structured generator with no parser in front of it. Format
   semantics stay in the facade: `zip` turns an archive into names and byte
   ranges with no opinion about what an entry is *for*; `xml` returns markup
   events with no XPS or EPUB vocabulary in its API.

9. **Oracles are subprocesses, never dependencies.** *Retired August 2026;
   superseded by ruling 13.* Bound [verification.md](verification.md) and
   every feature that cited an oracle diff. mutool, pdftoppm, pdfium_test,
   qpdf, openjpeg and a headless Chromium were invoked as external CLIs in
   CI; nothing linked them, and their outputs were transient comparison
   references, never committed or redistributed.

   The rule was right about *how* to hold an external program at arm's
   length. What ruling 13 overturns is that it held one at all. One part
   outlives it and is restated there: a check that can be absent must
   announce whether it ran, which is why the `RAN` / `SKIPPED` grep is still
   the pattern for every job that depends on something being present.

   The reasoning is kept rather than deleted, because it is the argument
   ruling 13 has to answer, and answering it costs something real. XPS
   markup has one right answer, so agreement with a second reader was
   evidence there — bounded by the recorded risk that where the second
   reader is wrong about XPS, both engines agree and both are wrong. **CSS
   does not work like that.** An engine whose EPUB support is itself a
   partial CSS implementation is not a reference, so:

   > For XPS, agreeing with the oracle was evidence. For EPUB, disagreeing
   > with it is not evidence of a bug.

   A browser is the reference implementation of CSS, and that is why one
   was the fifth oracle. Under ruling 13 it is not, and the sentence above
   becomes a limit this repository accepts and names
   ([features/epub.md](features/epub.md)) rather than a job it runs.

10. **Warnings carry provenance.** Binds all reading paths. Every leniency
    action (repaired xref, truncated stream decoded short, substituted
    font, placeholder image) is a typed warning naming the object it
    touched — so "it opened" and "it opened cleanly" are distinguishable,
    which is what makes the leniency ladder debuggable.

11. **The facade is the only public surface.** Binds
    [bindings](features/bindings.md). Bindings and the C ABI project the
    `tinker-pdf` crate 1:1; no binding adds logic, caching or defaults of
    its own. If a binding needs behavior, the facade grows it first.

12. **Own the parity tests verbatim.** Binds
    [verification.md](verification.md). `tinker_parity.rs` ports the host
    application's test files assertion-for-assertion; when those tests
    change (bug fixes only), the ports follow. Parity claims are
    `cargo test` output, not judgment.

13. **Verification is first-party.** Binds
    [verification.md](verification.md) and every feature that cited an
    oracle. Nothing outside this repository renders, parses, validates or
    measures a document as evidence. Ruling 9 is retired by this one.

    The line is drawn between *adjudicating* and *supplying*, because "no
    third party" read literally is unsatisfiable — the compiler, the CI
    runner and `curl` are all third-party programs. A third-party program
    may host this code, execute it, fetch bytes for it, or generate inputs
    for it. It may never be the thing that says whether the output is
    right. The reason is the same one that put every filter, cipher, font
    parser and rasterizer in this tree: an implementation this project does
    not own is one it cannot answer for.

    Third-party **bytes** stay admissible, with provenance recorded — the
    fetched corpora, fixtures real producers emitted, published normative
    data (Adobe's CMap resources, the Unicode character database), and the
    committed output of a tool that was run once, which is a dated
    measurement rather than a check. A corpus is not an implementation; it
    is what an implementation is for.

    What this costs is written down rather than absorbed.
    [verification.md](verification.md) names the properties that left the
    suite with the oracles and did not come back, in its own voice. A known
    gap is manageable; a suite that has quietly stopped proving something
    is not.

    **Amended 5 September 2026, on the question the roadmap's tier 0 asked:
    is a one-time visual comparison against an outside viewer admissible?**
    Yes, as an input, and never as a check. It is the same standing the
    committed JPEG 2000 decodes already have in `jpx_reference.rs` and the
    epubcheck verdicts have in `tests/epub/EPUBCHECK.tsv`: bytes a program
    produced once, on a named day, recorded with the tool, its version and
    the command. Such a record may be cited in a document and read by a
    person. It may not be re-run, it may not gate anything, and it may not
    be called a test — the moment it decides a build, an implementation this
    project does not own has said whether the output is right.

    The distinction is not a loophole and it is worth stating why it holds.
    A dated measurement is *evidence about one day*, and it decays: nobody
    re-runs it, so it cannot silently start disagreeing, and a reader sees
    its date. A check is *evidence about now*, and a check this project does
    not own is one it cannot answer for on the day it breaks. The two
    failure modes are different enough to be worth different words.

14. **Extracted text is in logical order.** Binds
    [content-and-text](features/content-and-text.md),
    [fonts](features/fonts.md), [epub](features/epub.md) and
    [forms](features/forms.md), and every output that reads `Page::text`:
    `plain_text`, `search`, the structured view and the JSON, XML and HTML
    formats alike. A page draws right-to-left text in visual order, and
    reading it in the order the content stream drew it reads an Arabic or
    Hebrew line backwards — a searchable Arabic PDF that searches as nothing
    and a screen reader that reads every word from its last letter. So a line
    holding a right-to-left character is put back into the order it is read
    in by **UAX #9 read backwards and checked forwards**: the line is sorted
    along its baseline, marks kept with the base glyph they sit on, and the
    order returned is one whose text the algorithm draws as the line stands
    (`tinker_pdf_shape::bidi::logical_order`). It is not L2 applied to levels
    resolved over the drawn line: L2 undoes itself only with the logical
    line's levels, and rules W2, W5, W7 and N1 read neighbours, so the drawn
    line resolves to other levels — `نسبة 50%` is drawn with the `%` on the
    number's left and read that way came back as `نسبة %50`. The paragraph
    direction is P2 read off the drawn line, by `Bidi_Class`: a
    left-to-right paragraph draws its first strong character leftmost and a
    right-to-left one rightmost, so the two ends decide, and the majority
    only where they disagree.

    *Decided 3 October 2026, and why this way.* The pin in
    `crates/tinker-pdf/tests/epub_shaped.rs` asserted the backwards line by
    name and said the decision was not a book's to take: reversing by
    `TextLine::rtl` would be a decision about every PDF this engine reads.
    That is the reason it is a ruling and not a fix, and the three properties
    that make it safe for every PDF are each asserted rather than argued:

    - **A line with no right-to-left character is untouched**, byte for byte
      and quad for quad — `text_logical_order.rs`'s
      `no_left_to_right_page_moves` compares every committed `testdata`
      document and every committed EPUB book against the opt-out.
    - **Both producer habits give one answer.** A producer that draws a
      right-to-left word in visual order and one that draws it in reading
      order with the pen moving left describe the same page; the line is
      sorted by where its glyphs are before it is read back, so the content
      stream's order decides nothing. Reversing the stream instead would
      have broken the second habit, which extracted right before this.
    - **The algorithm is Unicode's, held to Unicode's file in the direction
      extraction uses it.** `bidi_conformance.rs` feeds every visual order
      `BidiCharacterTest.txt` states (the 91 616 cases with nothing X9
      removes) back through `logical_order`: every answer is a permutation,
      every answer but three draws the stated line, and 90 947 come back to
      the file's own text. Each of the other 669 holds a bracket pair. Before
      the forward check, 8 100 came back as other text. The drawing
      direction, `order_units`, which the check calls, runs the whole of
      both files.

    UAX #9 is not one-to-one, so "back to the text that was typed" is not a
    property any reader can have: in a right-to-left paragraph
    `שלום 2026 now` and `שלום now 2026` are one picture, and this reads it
    as the second. What it does not undo is named in
    `crates/tinker-pdf/src/text_order.rs`: texts drawn alike; mirroring (L4),
    because whether a producer's `/ToUnicode` names a mirrored glyph's
    character or its shape is not on the page, and bracket pairs, which N0
    reads off the logical text; a paragraph, because a line is resolved
    alone; and vertical lines. The opt-out is
    additive — `Page::text_with(&TextOptions { content_order: true })` is the
    order the content stream drew, which is what `Page::text` returned before
    this ruling — and the reordering lives in the facade, so
    `tinker-pdf-content` keeps no edge to `tinker-pdf-shape` and
    `TextDevice` itself still collects in stream order.

## How to add a ruling

State it in one bold sentence, name the features it binds, give the reason
in two or three more. If a ruling reverses a feature doc's text, edit the
feature doc in the same commit and cite the ruling number there. A ruling is
never renumbered and never deleted — a retired one keeps its number and a
`*Retired*` marker, because the number is cited from source comments that
outlive any doc edit.
