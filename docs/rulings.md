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
    only where they disagree — with one tie-break for a line with no
    left-to-right character, the amendment below.

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

    **Amended 10 October 2026 by the owner, Ravindu Wijegunawardhana: the
    comma tie-break.** A line that holds at least one right-to-left strong
    character (`R` or `AL`) and **no** left-to-right one (`L`) was always
    read as a right-to-left paragraph, since both of its ends are right to
    left. It still is, with one exception: where the line, sorted along its
    baseline, has a strong right-to-left character as its **leftmost** unit
    and **closing punctuation** as its **rightmost** — a unit of
    `Bidi_Class` `CS`, `ON`, `ES` or `ET`; not a number (`EN`, `AN`) or a
    mark (`NSM`); and not an opening bracket or quotation mark,
    `General_Category` `Ps` or `Pi`, because a mark that trails a
    left-to-right reading closes — it is read as a **left-to-right**
    paragraph, so the punctuation trails the word. Units that draw nothing
    are passed over at either end, the set rule L1 resets with whitespace
    at a line's end: `WS`, `S`, `B`, the characters X9 removes and the
    isolate formatting characters, and also the two `White_Space`
    characters whose class is `CS` because they separate digits, U+00A0
    NO-BREAK SPACE and U+202F NARROW NO-BREAK SPACE, so that neither is
    taken for a comma. Every other line keeps
    the rule above, and a line with an `L` character anywhere is out of the
    tie-break's reach. And the tie-break is taken **only where a
    left-to-right paragraph draws the line**: `logical_order`'s search, run
    for a left-to-right reading, must reach an order that `order_units`
    draws as the line stands — the forward check every answer already
    passes, asked once more before the direction is chosen — or the line
    stays right to left. As first written, the shape alone decided, on the
    belief that both paragraphs always draw it, and any mark of those
    classes counted; a line holding a number is the counter-example to the
    first and an opening bracket at the right to the second (the price,
    below), and review caught both the same day.
    `tinker_pdf_shape::bidi::drawn_direction` chooses and asks;
    `logical_order` for a given direction is untouched, and
    `bidi_conformance.rs` holds it to Unicode's file as before.

    *Why.* CI's `epub-corpus` job reads Project Gutenberg's Moby-Dick, whose
    etymology table sets the Hebrew `חו,` alone in a cell: a left-to-right
    paragraph, which draws the comma right of the word. Once the word was
    drawn right (6d08c6b), the cell's line held nothing left to right, was
    read as right to left, and extracted `,חו` — the comma first — which the
    conservation sweep counts as a transposition. A right-to-left paragraph
    whose line *opens* with punctuation draws the same picture, so nothing
    on the line decides between the two; trailing punctuation after a
    right-to-left word quoted in left-to-right text is far commoner than a
    lone right-to-left line that opens with punctuation, so the commoner
    reading wins.

    *The price, accepted with that knowledge.* A line of a right-to-left
    paragraph that is alone on its line, with no left-to-right character,
    and opens with a mark of `Bidi_Class` `CS`, `ON`, `ES` or `ET` that is
    not an opening bracket or quotation mark (`Ps`, `Pi`) reads with that
    mark at its end. That class is wide: a dialogue dash (`— שלום`, drawn
    `םולש —`, reads `שלום —`), a bullet, `*`, `+`, `#`, `%`, an ellipsis
    (`…שלום` reads `שלום…`), and a straight or closing-form quotation mark
    used to open (`"שלום` reads `שלום"`; `”` and `»` likewise) — `"` and
    `'` are `Po`, closing as often as they open.
    **An opening bracket or quotation mark** does not pay where the
    producer's `/ToUnicode` names the typed character: `(١) بند`, `“שלום`
    and `«مرحبا` read as typed. Before it was excluded, `(١) بند`, drawn
    `دنب )١(`, read `١) بند(`, a text a left-to-right paragraph does draw
    that way. Where `/ToUnicode` names the mirrored glyph instead — `»` for
    `«`, `)` for `(`, as one derived from a font's `cmap` may — the line
    ends in a closing mark and pays like a dash (`«مرحبا` drawn with a
    mirrored `»` reads `مرحبا»`), unless a European number follows the
    mark, as below; which character a producer named is not on the page.
    **A European number after the mark** spares the line: `(1) פריט`,
    `— 2026 שלום` and `• 5 תפוחים` are drawn with the number next to the
    mark, `טירפ )1(`, which no left-to-right paragraph draws — there a
    European number with no right-to-left letter before it is left to right
    (W7), and one with a letter before it is drawn left of that letter — so
    they read as typed. Before the forward check was asked they read
    `פריט )1(`, an order that draws another line. **Two shapes still pay**:
    Arabic-Indic digits, which are `AN` and untouched by W7, so `— ١ بند`
    reads `١ بند —`; and a number that is not next to the mark,
    `— שלום 5 חו`, which reads `שלום 5 חו —`. All of it is pinned by name,
    so that changing the decision is a visible one. **The EPUB writer does
    not widen it**: a run of neutrals styled apart that opens such a line,
    `<p dir="rtl"><i>?!</i> שלום</p>`, is drawn as typed, so that it reads
    `שלום ?!`, the mark as typed, as before ROADMAP K-11. Drawn in its level's
    order, as K-11's fix first drew it, it read `שלום !?`, and 51 probe books
    that conserved in logical order at 9297d2d did not (review of the K-11
    fix; `epub_fallback.rs`'s
    `a_run_of_neutrals_opening_a_right_to_left_line_is_drawn_as_typed`).

    *The tests that hold it.* `text_logical_order.rs`:
    `a_right_to_left_word_and_its_comma_alone_on_a_line_read_as_written`
    (`חו,` and `حوت,` on a left-to-right page),
    `a_right_to_left_line_ending_in_a_full_stop_still_reads_right_to_left`
    (`שלום.`, the stop drawn leftmost, unchanged),
    `a_line_holding_a_left_to_right_character_keeps_its_rule`,
    `a_right_to_left_line_opening_with_a_mark_and_a_number_reads_as_typed`
    (the forward check),
    `a_right_to_left_line_opening_with_a_bracket_or_quotation_mark_reads_as_typed`
    (what opens), `a_right_to_left_line_opening_with_a_dash_reads_the_dash_last`
    (the price) and `no_left_to_right_page_moves`; `epub_fallback.rs`'s
    `a_standard_14_hebrew_word_alone_on_its_line_is_drawn_right_to_left`
    (the book's table, `חו,`); `text_order.rs`'s
    `a_word_and_its_comma_alone_read_as_their_paragraph_drew_them` (the
    comma drawn at either end of the word, and `TextLine::rtl` saying which
    direction was read); and in `tinker-pdf-shape`'s `bidi.rs`, the
    tie-break's classes and both readings drawing the line
    (`a_right_to_left_word_and_its_punctuation_alone_read_left_to_right`,
    `the_tie_break_reaches_no_other_line`,
    `a_mark_and_a_number_opening_a_right_to_left_line_read_as_typed`,
    `an_opening_bracket_or_quotation_mark_at_the_right_does_not_trail`,
    `the_blank_separators_are_the_white_space_that_bidi_class_misses`, and
    `a_right_to_left_line_opening_with_a_dash_reads_it_trailing`, which
    pins the price's four shapes); `unicode.rs`'s
    `what_opens_is_general_category_ps_and_pi`.

    **Amended 10 October 2026, a second time: the bracket tie-break.**
    *Proposed by the ROADMAP K-11 fix and revised on its reviews; accepted by
    the owner, Ravindu Wijegunawardhana, on 11 October 2026, with the cost
    measured below and its named price.* After the rules above have chosen a line's
    paragraph direction, a line that draws a **closing bracket left of an
    opening one of its kind** — `)histone deacetylase(`, each hollow turned
    away from what it encloses — is read in the other direction where three
    things hold of that direction's reading: it draws the line as it stands
    (the forward check); it pairs **more brackets in all**, by BD16 over the
    text it reads, than the reading in the direction chosen; and **more of its
    pairs face outward and hug what they enclose** — no whitespace just inside
    either bracket, as a bracket is set against its own text. A pair stands
    facing outward on the page in exactly one case: at a right-to-left level,
    where `/ToUnicode` names the typed character — as this engine's writers
    name it, and as this ruling's reading, which swaps no character, already
    takes it. A pair at a left-to-right level faces inward, and so does a pair
    at a right-to-left level whose `/ToUnicode` names the mirrored glyph; **a
    pair facing inward turns nothing**, since nothing on the page says which
    of the two drew it. But two brackets that are **no pair as typed** stand
    facing outward all the time in left-to-right text — a list marker and a
    later parenthesis, `Step 1) open the file (שלום) and 2) close it.`, a
    smiley, `:)`, a parenthesis opened at the end of a line and closed on the
    next — and read from the right they pair; the other two conditions keep
    those lines as typed. Such a reading pairs no more brackets in all than
    the typed text (`(שלום)` read as typed, `) open the file (` read from the
    right: one each), or pairs them with the spaces that stood outside the two
    brackets now inside the pair (`see( מלחמה )b` from `b) מלחמה (see`).
    And **a pair that may have taken its level from the line before is no
    evidence**: N0 resolves a pair enclosing only the other direction's
    letters by the strong character before its opening bracket, which for a
    pair that opens a line is on the line above, so a left-to-right
    paragraph's line opening with `(הארץ)` after a line that ended in Hebrew
    draws the pair facing outward at its left end, as a right-to-left
    paragraph draws a pair that ends it. A pair with a bracket at the end of
    the line where a reading in the direction chosen starts, enclosing no
    strong character of that direction, is passed over by both readings where
    the line's letters say the direction chosen — they outnumber the other's
    and the reading in that direction pairs a bracket facing outward of its
    own, or they are at least twice the other's and at least eight more. And
    where the comma tie-break chose left to right for a line that ends a
    sentence — read leftward from its right end, any closing quotation
    marks, then one stop or more — a left-to-right paragraph drew that line
    as right-to-left text with the sentence's end after it, and the line
    opens just left of the stops: a pair with a bracket there, enclosing no
    left-to-right letter, is passed over too. The stops are the full stop,
    the comma, the colon, the semicolon, `!`, `?`, the ARABIC COMMA and the
    ellipses U+2024, U+2025, U+2026 and U+FE19, one or a run (`.`, `...`,
    `?!`); the closing quotation marks are `"`, `'` and the ten of
    `General_Category` `Pf` (`»`, `’`, `”`, `›`, U+2E03, U+2E05, U+2E0A,
    U+2E0D, U+2E1D and U+2E21), so that `."`, `.»` and `.”` end a sentence
    as `.` does. A closing quotation mark alone ends none, nor does a closing
    bracket. UAX #14 (Unicode 17.0, the `LineBreak.txt` `tinker-pdf-layout`
    vendors) puts each of these first on a line only after a forced break or
    after a space in the one case it names: no line breaks before `!` or
    `?`, its `EX`, even after a space (LB13, `× EX`); before the full stop,
    the comma, the colon, the semicolon or the ARABIC COMMA, its `IS`, only
    after a space and before a digit (LB15c, `SP ÷ IS NU`, which opens a line
    with `.5`; LB15d, `× IS`, refuses every other); before an ellipsis, its
    `IN`, only after a space (LB22, `× IN`, which LB18, `SP ÷`, precedes);
    and before a closing quotation mark, its `QU`, only after a space (LB19,
    `× [QU - Pi]`, after LB18), and before a `Pf` one that a space, a stop,
    another closing mark or the text's end follows not even then (LB15b).
    That an ellipsis ends a sentence is the owner's decision, with its price
    (below).
    `tinker_pdf_shape::bidi::drawn_direction` asks it last, of a line that
    draws a closing bracket left of an opening one and of no other, and
    `logical_order` for a given direction is untouched: `bidi_conformance.rs`
    holds it to Unicode's file as before, its 90 947 and its 669 unmoved.

    *Why.* ROADMAP K-11. A right-to-left paragraph narrowed so that a line
    opens with a bracketed Latin run, `(histone deacetylase). وقد أصبحنا نعرف
    الآن`, holds eighteen Latin letters and seventeen Arabic ones; its ends
    disagree, and the majority read it as a left-to-right paragraph, `وقد
    أصبحنا نعرف الآن .)histone deacetylase(` — a text a left-to-right
    paragraph draws as that line stands, so the forward check could not refuse
    it. Both readings draw the line; only the brackets tell them apart, and
    only because they face outward. And the third review of the K-11 fix
    found the pair from the line before: a justified English paragraph
    quoting Hebrew in brackets, `Words שלום עולם (מלחמה) and more English
    text, …`, whose third line opens with `(הארץ)` and holds fifty Latin
    letters to fifteen Hebrew ones, was turned and read backwards (4 books in
    a default build, 1 with `bundled-fonts`, under `text-align: justify` and
    `word-spacing: 0.3em`; `bidi.rs`'s
    `a_pair_the_line_above_resolved_turns_no_line`). Its review found the
    same pair in a paragraph with no `dir`: `جميل بيت كبير (نور) يوم.` broken
    before the bracket draws `موي )رون(.`, which the comma tie-break reads
    left to right and which, read from the right, pairs `(نور)` and reads
    its full stop first (32 books in a default build and 29 with
    `bundled-fonts` that read their stop last at 9297d2d, styled brackets
    under `margin: 0 42%`, and a letter-spaced line opening with `(١٬٢٣٤),`
    that the join of split lines read whole for the turn;
    `a_dirless_line_opening_with_a_bracket_pair_reads_its_full_stop_last`,
    `bidi.rs`'s `a_pair_before_a_full_stop_the_line_above_resolved_turns_no_line`).
    And the review of that fix found the exception asked too narrowly (K-11
    review, finding V1): only of a line whose rightmost unit was one of
    those seven marks, with the line's start at the unit just left of it, so
    that a line ending `...`, `?!`, `!!` or `!?` — the unit left of its last
    stop another stop — or `."`, `.»`, `.”` or `…` was still turned and read
    its marks first (76 of its 864 books in a default build and 82 with
    `bundled-fonts` that read their marks last at 9297d2d, and a line with a
    second pair;
    `a_dirless_line_opening_with_a_bracket_pair_reads_its_closing_quotes_and_stops_last`,
    `bidi.rs`'s `a_pair_before_closing_quotes_and_several_stops_turns_no_line`).

    *What it gains and what it costs, measured.* Over `BidiCharacterTest.txt`,
    each of its 91 616 drawn lines read in the direction `drawn_direction`
    chooses: 59 246 draw a closing bracket left of an opening one, and the
    tie-break turns 24 504 of them, all but 2 to the file's own paragraph
    level. The lines that read back as the file's own text go from 48 366 to
    72 650 (the direction is the file's own on 46 054, then 70 554), and **no
    line reads worse**. In running text, where a line is a piece of a
    paragraph: of the review's 2 235 lines — 82 sentences in Arabic and
    Hebrew, list markers, smileys, glosses and bracketed Latin runs in both
    directions, wrapped at nine widths — 101 read better and none worse, and
    of 986 more, from 30 sentences of numbered and lettered lists, code,
    brackets side by side, brackets spaced from what they enclose and
    fragments typed closing first, 57 better and none worse; of the second
    review's 13 300, its sentences typed and with each bracket's mirrored
    glyph named, 413 better and 62 worse, every one of the 62 the price below;
    and of the books K-11's probe and its three reviews built — 480, 3 276,
    2 880 and 11 184 in each build, with 1 034, 120 and 90 more of their
    neighbours and the marks fix's 11 646 — none that conserved, read whole
    or was found by search at 9297d2d fails. Passing over the pair from the
    line before changes none of those lines, nor any line of the file: none
    of the file's lines the tie-break turns holds more than six letters
    against the turn, and asked of every line it opens whose letters
    outnumber the other's it read 1 393 of them worse and 52 better (1 341
    fewer read as typed) and 2 of the sentence lines worse, asked wherever
    the reading as chosen pairs a bracket facing outward too, 8 worse and
    none better. Passing over a pair before a sentence's end — its stops and
    closing quotation marks — changes none of them either; asked of every
    mark that tie-break takes, a lone closing mark among them, it read 215 of
    the file's lines worse and 12 better — lines that open with a symbol,
    `⚀(א)ב`, drawn as such a line is. **The price** is a text typed with
    brackets that pair only read the other way round, each set against the
    text between them: `x)שלום(y` in a left-to-right paragraph, drawn
    `x)םולש(y`, reads `y(שלום)x`. No book above pays it, and of the lines
    only the second review's fragments written in that shape, `x)שלום(y and
    a)עולם(b`; `bidi.rs` pins it by name. **As first proposed** the tie-break asked only
    that the other reading pair more brackets facing outward and no fewer in
    all, which Unicode's file, whose lines are each a whole paragraph, priced
    at 1 250 lines typed with brackets that do not pair. The review measured
    running text and found the price in the lines above: 23 of the 2 235 read
    as typed before and backwards after — `Step 1) open the file (שלום) and 2)
    close it.` as `.and 2) close it )שלום( open the file )Step 1` — and 24
    books in each build that had conserved no longer did (`<p>Step 1) read
    the word (…) aloud and 2) write it down, then 3) check (…) again.</p>`,
    Arabic and Hebrew, six styles of nine, pointed and not), and 2 more with
    `bundled-fonts`. Asking for more pairs in all left 3 of those
    lines, each an `a)` marker and a parenthesis wrapped onto the next line;
    asking the pairs to hug left none. Without the pairs in all, hugging alone
    turns 31 087 lines and reads 5 325 worse, 1 376 of them `()()`-shaped
    texts whose brackets all pair, read from the right as the `)(` between
    them. And **what it leaves**: a pair facing inward — a left-to-right
    paragraph with no `dir` whose line has Arabic at both ends and a Latin run
    in brackets between, `… عدة (HDAC). وقد …`, draws exactly what a
    right-to-left paragraph whose `/ToUnicode` names the mirrored glyph draws
    for `وقد … (HDAC) … عدة`, and still reads as the second, `(HDAC).` as
    `.)HDAC(` — two texts drawn alike (`content-and-text.md` names it, with
    the K-11 probe's count). Taking it the other way would assume the typed
    character on every page, and would read every such right-to-left line of a
    producer that names the mirrored glyph with its runs in left-to-right
    order; that assumption is the owner's to make, and this does not make it.
    Nor does it reach a line holding one bracket of a pair a line break split,
    or quotation marks, which BD16 does not pair (`content-and-text.md` names
    both).

    *A sentence's end, the ellipsis and the price: the owner's decisions of
    11 and 12 October 2026.* That an ellipsis ends a sentence — U+2026,
    U+2024, U+2025, U+FE19 or a run of full stops — is the owner's decision,
    Ravindu Wijegunawardhana's, of 11 October 2026; and on 12 October 2026
    the owner accepted **the whole price of the rule as built**, with its
    figures known: a run of `!` and `?`, a closing quotation mark before
    stops, and an ellipsis. A right-to-left paragraph whose line opens with
    one of them before a bracket pair, `…(نور) يوم` or `?!(نور) يوم` after a
    `<br/>` or a soft wrap, draws exactly what a paragraph with no `dir`
    draws for a line ending `(نور) يوم…`, and now reads as that one does, its
    ending last: `نور) يوم(…`, the pair the other way round as well, since
    the line above that gave it its level is not on the line (with the
    brackets styled apart, a book's structure tree still holds their order).
    Measured again on the tree that carries this text, the fix of the
    second review of CD-19's fix, against 56dfd93, in a default build and
    (in brackets) with `bundled-fonts`: of the K-11 review's 144 such books
    (Arabic, Hebrew and pointed Hebrew, up to five kinds of bracket, twelve
    styles), 96 (83) that conserved at 56dfd93 do not now, none of which
    conserved at 9297d2d. A probe of the price — 342 books for each
    opening, in each build: Arabic, Hebrew and pointed Hebrew, three kinds
    of bracket, styled and not, nineteen styles — counts the books that
    read worse than at 56dfd93, opening after a `<br/>` with `…` 291 (241),
    U+2024 297 (273), U+2025 297 (267), U+FE19 270 (195), `..` 288 (270)
    and `...` 277 (236); with `!!` 267 (212), `!?` 255 (195), `?!` 240
    (195) and `??` 267 (192); and with a closing quotation mark before
    stops, `".` 261 (214), `'.` 264 (249), `».` 269 (203), `”.` 261 (195),
    `"...` 259 (221) and `”…` 279 (264) — 4 342 (3 622) in all, every one of
    which conserved there: 1 029 (794) of the runs of `!` and `?`, 1 593
    (1 346) of the closing quotation marks before stops and 1 720 (1 482) of
    the ellipses. `.`, `?`, the ARABIC COMMA, `"` and `”` alone, which pay
    nothing more, are its controls. Measured with the rule alone on
    56dfd93 it was 4 630 (3 925) in all, 4 618 (3 916) conserving there —
    1 101 (866) of the runs of `!` and `?`, 1 809 (1 574) of the closing
    quotation marks and 1 720 (1 485) of the ellipses: the EPUB writer, which
    reads each line that holds a right-to-left character back as a reader
    of the page reads it and draws it otherwise where that reads it right
    (ROADMAP CD-19), draws 288 (303) of them so that they read as at
    56dfd93, and none of the probe's books reads worse than with the rule
    alone but the three `'.` books below. None of those books conserved at
    9297d2d, and none that read whole or was found by search there fails to
    now. With styled brackets 667 of the closing quotation marks' (543),
    whose structure tree read in order at 9297d2d and at 56dfd93 where
    their page did not, lose that too: the line's `".` comes back `."`.
    Opening after a space, where a soft wrap may put `"`, `'`, `”…` or an
    ellipsis first on a line, 22 of the probe's 1 197 books pay it (20): 16
    (18) with a closing quotation mark and 6 (2) with an ellipsis; the
    ARABIC COMMA's control there, letter-spaced by half an em and
    justified, which read worse with the font at `0 Tw` until the first fix
    of CD-19's review, reads as at 56dfd93 again. All of it is the price the
    owner accepted.

    *What the owner's decision does not cover.* The rule as built takes
    any run of the eleven stops, so a run that mixes them, `.!` or `!…`,
    opening a right-to-left line before a styled pair after a `<br/>`, pays
    the same: of the review's probe of them (H6 of the review of CD-19's
    fix), 2 (2) books with `.!` and 2 (2) with `!…`, with no spacing, that
    conserved at 56dfd93 do not now, none of which conserved at 9297d2d, and
    as many again of that review's minimised books; at `word-spacing:
    0.6em`, where as many more lost conservation and two of each their
    structure tree's order until the fix of the second review, each
    conserves again, as at 56dfd93. The decision names none of them,
    and whether they are of its price is the owner's to say (ROADMAP CD-19
    keeps them). Nor is a book the price that loses more than the rule
    costs where the overflow font at `0 Tw` meets it: three `'.` books in
    each build at `word-spacing: 0.5em`, whose structure tree read in order
    at 9297d2d and with the rule alone but not at 56dfd93, nor under both
    now — ROADMAP CD-19 keeps them as findings. The nine (six) `!?` and
    `?!` books before styled brackets at `word-spacing: 0.6em` and `1em`
    that lost their structure tree's order where the font met the rule, as
    neither alone loses it, conserve since the second review's fix, as
    they did at 56dfd93.

    *The tests that hold it.* `tinker-pdf-shape`'s `bidi.rs`:
    `a_pair_drawn_facing_outward_reads_in_the_direction_that_pairs_it`,
    `a_pair_drawn_facing_inward_decides_nothing`,
    `two_pairs_side_by_side_are_not_broken_for_the_bracket_between_them` (the
    pairs in all),
    `brackets_that_are_no_pair_as_typed_keep_the_lines_direction` (the
    review's lines),
    `brackets_typed_closing_first_against_a_word_read_as_the_other_direction_pairs_them`
    (the price),
    `a_pair_before_a_full_stop_the_line_above_resolved_turns_no_line` and
    `a_pair_before_closing_quotes_and_several_stops_turns_no_line` (a
    sentence's end, and the ellipsis's price); `text_logical_order.rs`:
    `a_right_to_left_line_opening_with_a_bracketed_latin_run_reads_as_written`
    and
    `a_list_marker_and_a_wrapped_parenthesis_in_left_to_right_text_read_as_typed`;
    `epub_conservation.rs`:
    `a_bracketed_run_and_its_full_stop_in_right_to_left_text_read_as_written`,
    `a_dirless_line_with_arabic_at_both_ends_and_latin_in_brackets_reads_as_its_drawn_alike_twin`,
    `list_markers_and_glosses_in_left_to_right_text_conserve`,
    `a_bracket_pair_the_line_above_resolved_turns_no_line`,
    `a_dirless_line_opening_with_a_bracket_pair_reads_its_full_stop_last` and
    `a_dirless_line_opening_with_a_bracket_pair_reads_its_closing_quotes_and_stops_last`
    (a sentence's end).

    *The EPUB writer's read-back and its prices: the owner's decisions of
    17 and 23 October 2026.* The EPUB writer reads each line that holds a
    right-to-left character back as a reader of the page reads it, on the
    page it is drawn on, and draws it otherwise where that reads it right
    (ROADMAP CD-19). Round 6's fix of CD-19 measured what its first form
    cost against 935b077, and the owner, Ravindu Wijegunawardhana, declined
    three prices, each with its figures known:

    - **Its time** (17 October 2026). Reading each line back on the page
      took 1.06–1.13 times 935b077's time to open every shape of the probe
      kit's `zz_v6_time` in debug, and a letter-spaced line under
      `scaleX(2)` (`ls-scaled`) 1.47–1.51 (best of six interleaved runs on
      the owner's Windows machine; two series of 935b077 0.0–1.7% apart).
      The landed fix plans each run once a page for every reading of its
      line, and searches each line of units' reading once a page: in debug
      it opens every shape in 0.20–0.80 of 935b077's time (best of three
      interleaved rounds; a build of 935b077's code again at 0.94–1.00).
    - **`read_as`'s text** (17 October 2026). Reading each glyph back as
      the page's font gives it back turned a styled `?!` that 935b077 read
      as typed: in `W W <b>1 234</b> <i>?!</i> W.`, twice, the number's
      halves a NO-BREAK SPACE apart, a reader reads that space by its glyph
      name, UAX #9 then reads two numbers, and no drawing tried reads the
      line's letters in order. The price put to the owner was 12 books of
      the NO-BREAK SPACE grid in a default build and 7 with
      `bundled-fonts`, at `word-spacing: 0.6em`, `1em` and `12px`. Measured
      since, the class is wider: 16 (10) books of the probe kit's
      `zz_f6_readas`, the 12 (7) and 4 (3) under `scaleY(2)`, where 935b077
      turned the first `?!` already; and beside a letter the overflow font
      names `space`, or a SOFT HYPHEN, 52 (39) books of a 64-book probe read
      a `!?`, where 935b077 read 16 (20). The landed fix searches such a line
      again with each glyph read as typed where the page's fonts' reading
      finds nothing, and draws what that finds only where it reads more of
      the line's pairs of letters in order on the page: every one of those
      books reads its `?!` as typed, and none of the 64 a `!?`
      (`epub_fallback.rs`'s
      `a_styled_mark_after_a_number_a_no_break_space_holds_together_reads_as_typed`
      and
      `a_styled_mark_after_a_number_reads_as_typed_beside_a_letter_the_page_gives_back_otherwise`).
    - **The second reading's time, and the books its removal costs** (23
      October 2026). Under a transform, a line whose rules' drawing reads
      on the page with its letters in order and not its words is read again
      in its runs' own space, as 935b077 read every line under a transform.
      In release that reading kept `ls-scaled` at 1.02–1.09 times 935b077's
      time to open (best of ten interleaved rounds; median 1.065–1.08), and
      leaving it out made 12 books worse in each build, 10 of them right at
      935b077, and failed two case tests. The owner declined both, and
      chose to keep searching for a form that costs neither. The landed fix
      keeps the second reading and searches each line of units' reading
      once a page (`text_order::Readings`): in release `ls-scaled` opens in
      0.90–0.95 of 935b077's time (best of ten rounds, median 0.92–0.95; a
      build of 935b077's code again 0.98–1.02) and every shape in 0.24–0.91
      (best of six), and every book of the kit reads as before, its outputs
      byte for byte.

    No time or text price of the read-back is accepted. Every other option
    measured for the owner — holding each drawing to the line's words,
    searching on past a reading of the rules' drawing that joins or cuts a
    word, reading each drawing once, and H1, H2-3 and H6 — is in ROADMAP
    CD-19 with its price, and none is taken.

## How to add a ruling

State it in one bold sentence, name the features it binds, give the reason
in two or three more. If a ruling reverses a feature doc's text, edit the
feature doc in the same commit and cite the ruling number there. A ruling is
never renumbered and never deleted — a retired one keeps its number and a
`*Retired*` marker, because the number is cited from source comments that
outlive any doc edit.
