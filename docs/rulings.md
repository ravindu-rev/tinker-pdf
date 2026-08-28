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
   with a translated viewport — never a second implementation. Tile rows are
   pinned byte-equal to the full-page subregion, and that test is the
   permanent guard.

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
   As of August 2026 there are eleven: `filters`, `crypto`, `font`, `color`,
   `raster`, `math`, `zip`, `xml`, `css`, `layout`, `pki`. Four leaf-to-leaf
   edges exist (`font → filters`, `zip → filters`, `layout → css`,
   `pki → crypto`), and a leaf-to-leaf edge does not weaken this ruling, which
   is about public APIs rather than about edges. `tinker-pdf-layout` is the one leaf whose input
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

## How to add a ruling

State it in one bold sentence, name the features it binds, give the reason
in two or three more. If a ruling reverses a feature doc's text, edit the
feature doc in the same commit and cite the ruling number there. A ruling is
never renumbered and never deleted — a retired one keeps its number and a
`*Retired*` marker, because the number is cited from source comments that
outlive any doc edit.
