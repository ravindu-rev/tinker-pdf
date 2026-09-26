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
   with a translated viewport — never a second implementation. A tile must be
   pinned byte-equal to the full-page subregion, and that test is the
   permanent guard.

   *Corrected 13 September 2026; satisfied, with one stated exception,
   15 September 2026.* The correction is kept rather than overwritten,
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
   the whole render byte for byte, with no tolerance. The ten are text, a
   diagonal axial shading, an image and strokes — four different paths
   through the rasterizer, only one of which the scanline filler
   anti-aliases — three of them turned a quarter or three-quarter turn, two
   with a crop box offset from the media box on both axes, and one with both
   at once. The tile sizes are 64, 37 and 23 against a 91×131 page, so no
   lattice divides it and the last row and column are always partial.
   `tiles_at_other_scales_are_byte_equal_too` repeats the whole set at 0.5×,
   2× and 4×; `single_pixel_tiles_are_byte_equal` runs a one-pixel lattice;
   `annotations_are_tiled_with_the_page` covers the annotation layer, which
   no other fixture there draws from.

   **It is byte-equal at those scales and not at every scale, and the
   exception is measured rather than waved at.** A tile's transform is the
   page's with a whole number of pixels taken off `e` and `f`, which is
   exact as arithmetic and not as floating point: `fl(u + e)` and
   `fl(u + e − tx)` are two roundings at two magnitudes and differ in the
   last ulp, which reaches a byte only where the exact value sits on one of
   the rasterizer's 1/256 steps. Measured 15 September 2026 over all ten
   fixtures at 0.75×, 1.5× and 3×, tiling at 53: **29 of the 30 lattices
   are exact, and the thirtieth — the axial shading at 3× — differs on one
   pixel of 107 289 by one level of 255.**
   `at_the_scales_where_two_frames_round_apart_the_gap_is_one_level` pins
   that as the bound rather than above it, so a change makes it fail
   whichever way it moves.

   Which sampler is left is the useful half of that measurement. `fill`
   reduces a crossing to the *nearest* 1/256 unit by adding a half and
   shifting, which is a `floor` of a shifted value and commutes with moving
   a shape a whole number of pixels; `draw_image` snaps its quad to the same
   grid, which [rasterizer](features/rasterizer.md) records was put there
   for this reason. A shading sampler does neither — it takes the axial
   parameter from `a·x + c·y + e` straight into a colour with no grid in
   between — and the one lattice that diverges is a shading. Closing it
   means carrying the canvas's origin through the sampler, the mesh and the
   image run so that both frames compute in one lattice; that is a change to
   `tinker-pdf-raster`'s shape and it has its own [roadmap](ROADMAP.md) row.
   Until it lands, this ruling is a byte-equality claim **with a named
   exception** rather than an unconditional one, and saying so is the whole
   point of the September correction above.

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
   [rendering](features/rendering.md). Text extraction and rasterization are
   both devices; nothing reaches around the interpreter to read content
   streams directly, except the `ContentFilter` rewrite path built for
   redaction ([editing](features/editing.md)) — which is itself part of the
   interpreter.

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

## How to add a ruling

State it in one bold sentence, name the features it binds, give the reason
in two or three more. If a ruling reverses a feature doc's text, edit the
feature doc in the same commit and cite the ruling number there. A ruling is
never renumbered and never deleted — a retired one keeps its number and a
`*Retired*` marker, because the number is cited from source comments that
outlive any doc edit.
