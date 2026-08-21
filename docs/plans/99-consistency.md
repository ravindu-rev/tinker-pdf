# Phase 99 — Consistency rulings

Cross-phase decisions that bind more than one plan. When a phase plan and a
ruling here disagree, the ruling wins — the same contract Tinker's own
`docs/plans/99-consistency-review.md` uses. This file starts short and grows:
every time two phases could plausibly answer the same question differently,
the answer gets a numbered ruling here instead of living in one phase's head.

## Rulings

1. **Never panic on untrusted input.** Binds every crate. Malformed bytes
   produce errors or warnings, never a panic; the rule is enforced by the
   per-format fuzzers ([14](14-testing-and-corpora.md)), and a fuzz crash is
   a release blocker, not a backlog item.

2. **Degrade, don't fail.** Binds [02](02-filters.md) and
   [08](08-rendering-device.md). A missing capability (JBIG2, JPX,
   arithmetic JPEG, mesh shadings, full ICC) renders a neutral placeholder
   and appends a structured warning to `Bitmap.warnings`. A page render never
   hard-fails because one image used a rare codec.

3. **Capability scheduling is evidence-driven.** Binds [02](02-filters.md),
   [08](08-rendering-device.md), and the master plan's descope levers. A
   deferred capability is implemented when the nightly corpus hit-rate report
   ([14](14-testing-and-corpora.md)) says real documents need it — not
   before, however interesting it looks.

4. **Determinism is a contract, not a hope.** Binds [07](07-rasterizer.md)
   and [08](08-rendering-device.md). Fixed-point coverage accumulation,
   integer inner loops, no platform libm in hot paths; the same input
   produces bit-identical bitmaps on linux, windows, macos and wasm, and CI
   compares goldens across all four to prove it.

   *Amended, August 2026.* "No platform libm in hot paths" was written as
   guidance and held as nothing: `sin`, `cos`, `atan2`, `ln`, `log10` and
   `powf` were being called from the stroker's round joins, from the
   PostScript calculator functions and from the sRGB transfer function — all
   of which reach pixels, and none of which any platform rounds the same way.
   The rule now has a crate and a check behind it. `tinker-pdf-math` supplies
   the functions, built from nothing but the operations IEEE 754 pins exactly,
   and it is `no_std` so that `x.sin()` does not compile inside it.
   `cargo xtask libm` fails the build if any pixel-path crate calls one of
   them.

   The boundary is worth stating precisely, because half of the original rule
   was over-broad: `sqrt`, `floor`, `ceil`, `round`, `trunc` and `abs` **are**
   correctly rounded by the standard, so they are identical on every target
   and pixel-path code may use them freely. It is only the transcendental
   family that diverges.

5. **Tiles share the full-page code path.** Binds [07](07-rasterizer.md) and
   [08](08-rendering-device.md). A clipped render is the same pipeline with a
   translated viewport — never a second implementation. Tinker's
   `render_pages.rs` pins tile rows byte-equal to the full-page subregion,
   and that test is the permanent guard.

6. **Destinations are an enum, everywhere.** Binds
   [04](04-document-semantics.md), [09](09-writing.md),
   [12](12-creation.md). `Explicit | Named | Uri` — reading never collapses
   them, writing round-trips them. This is the design answer to MuPDF
   limitation #6 (URIs silently rewritten into named destinations) and it
   must hold on both sides of the API forever.

7. **The `Device` trait is the only seam between interpretation and
   consumers.** Binds [06](06-content-and-text.md) and
   [08](08-rendering-device.md). Text extraction and rasterization are both
   devices; nothing reaches around the interpreter to read content streams
   directly, except the `ContentFilter` rewrite path built for redaction
   ([10](10-editing.md)) — which is itself part of the interpreter.

8. **Leaf crates stay PDF-free.** Binds `filters`, `crypto`, `font`,
   `color`, `raster`, `math`, `zip`. Bytes and values in, values out; no COS
   types, no PDF-spec vocabulary in their public APIs. This is what keeps them
   independently fuzzable, testable and publishable — and it is also the
   off-ramp insurance in [PLAN.md](../PLAN.md).

   *Amended, August 2026.* This named five, and had named five since it was
   written. `tinker-pdf-math` arrived with ruling 4's amendment above and this
   ruling did not move; `tinker-pdf-zip` arrived with
   [gap 29](gaps/29-cbz.md), which found the first drift while sweeping for
   the second. The rule is unchanged and its scope is wider than the list
   suggested — which is the failure mode of a rule that enumerates rather than
   defines, so: a leaf is any crate that takes bytes and plain parameters and
   returns bytes and values, whatever the list says. `tinker-pdf-zip` turns an
   archive into names and byte ranges and has no opinion about what an entry is
   *for*; gap 29's page semantics live in the facade for exactly that reason.
   Two leaves depend on `filters` — `font` for its CMap asset pipeline and
   `zip` for raw DEFLATE and CRC-32 — and a leaf-to-leaf edge does not weaken
   this ruling, which is about public APIs rather than about edges.

   *Amended again, 19 August 2026, [gap 30](gaps/30-xps.md) milestone 9.*
   **Eight**, and this is the first update the amendment above predicted: it
   said the scope is wider than the list, and `tinker-pdf-xml` is a crate the
   list did not name and the definition always covered. It takes bytes of
   markup and returns events, holds no PDF and no XPS vocabulary in its public
   API — gap 30's package layer lives in the facade for exactly that reason,
   the same place gap 29 put page semantics — and it has **no internal
   dependency at all**, which is the third crate in the workspace of which that
   is true. What makes it worth a sentence rather than a name in a list is that
   [31](gaps/README.md), EPUB, reuses this crate and reuses **none** of gap
   30's package layer, because EPUB's container is OCF with
   `META-INF/container.xml` rather than OPC: the leaf is the reusable part
   precisely because it is a leaf.

   *Amended a third time, 22 August 2026, [gap 31](gaps/31-epub.md) milestone
   13.* **Ten**, and one word of the amendment above is corrected in the same
   edit. It said gap 31 *"reuses this crate"*, and the truth is **reuses, after
   changing**: `tinker-pdf-xml` as gap 30 froze it refuses every Project
   Gutenberg EPUB 2 content document and the cover wrapper of every EPUB 3 one,
   on `<!DOCTYPE`, so gap 31's milestone 2 gave it a two-valued doctype mode
   before a single book could be read. Reuse of a leaf that must first grow a
   mode is a different claim from reuse of a leaf as frozen. The dangerous half
   — the internal subset, where all four of gap 30's bombs live — stays refused
   by name under both modes, and all four refusals are re-asserted under the new
   one.

   The two new leaves are `tinker-pdf-css` and `tinker-pdf-layout`. They are two
   crates rather than one because a parser and an algorithm need different fuzz
   targets: `css` takes bytes and `layout` takes a caller-built tree, which
   makes it the only leaf here that is a leaf by the definition and not by the
   shape. The definition is what binds, which is what this ruling has said since
   its first amendment.

9. **Oracles are subprocesses, never dependencies.** Binds
   [14](14-testing-and-corpora.md) and every phase that cites an oracle
   diff. mutool, pdftoppm, pdfium_test and qpdf are invoked as external
   CLIs in CI; nothing links them, and their outputs are transient
   comparison references, never committed or redistributed.

   **Amended, 21 August 2026, gap 31 milestone 8: a headless browser is a
   fifth, and only for CSS.** [31](gaps/31-epub.md)'s oracle section works out
   why the four are not enough for a reflowable format, and the argument is
   worth keeping rather than the conclusion alone.

   Gap 30 used mutool as its XPS oracle and recorded the risk it was taking:
   *"where MuPDF is wrong about XPS this engine will agree with it and both
   will be wrong"*. That is bounded, because XPS markup has one right answer.
   **CSS does not work like that.** MuPDF reads EPUB — `mutool draw` lists it
   and takes `-W`, `-H` and `-S` for its layout — but MuPDF's EPUB engine is
   *itself* a partial CSS implementation, so:

   > For XPS, agreeing with mutool was evidence. For EPUB, disagreeing with
   > mutool is not evidence of a bug.

   A browser is the reference implementation of CSS, and comparing a CSS
   implementation against a partial one is comparing it against nothing. So
   `tests/epub_browser.rs` invokes Chromium — `chrome`, `msedge` or `chromium`,
   found by path or named by `TINKER_BROWSER` — with `--headless=new`,
   `--dump-dom` and `--print-to-pdf`.

   **The ruling's substance is unchanged and that is why this is an amendment
   rather than an exception**: the browser is a subprocess, nothing links it,
   nothing is vendored, and its output is transient. What is new is only which
   binary is on the list. Two constraints come with it, both from gap 31's own
   oracle section rather than discovered afterwards:

   - **It is not a pixel comparison and never becomes one.** A browser lays a
     content document into one continuous column, so there is no page 3 to
     compare against page 3; what is compared is `y` offsets and the partition
     of the text across pages. Gap 18a pre-argued the same point for a
     fixed-point wavelet against a float reference.
   - **The job goes red when the browser is missing.** Gap 20's finding, for
     the fourth time: a skipped oracle exits 0 and reads exactly like a pass.
     The `browser-oracle: RAN` / `SKIPPED` line is printed and greped, exactly
     as `qpdf-oracle:` is.

10. **Warnings carry provenance.** Binds all reading phases. Every leniency
    action (repaired xref, truncated stream decoded short, substituted
    font, placeholder image) is a typed warning naming the object it
    touched — so "it opened" and "it opened cleanly" are distinguishable,
    which is what makes the leniency ladder debuggable.

11. **The facade is the only public surface.** Binds
    [13](13-bindings.md). Bindings and the C ABI project the `tinker-pdf`
    crate 1:1; no binding adds logic, caching or defaults of its own. If a
    binding needs behavior, the facade grows it first.

12. **Own the parity tests verbatim.** Binds [14](14-testing-and-corpora.md)
    and [15](15-tinker-integration.md). `tinker_parity.rs` ports Tinker's
    five test files assertion-for-assertion; when Tinker's tests change
    during the freeze (bug fixes only), the ports follow. Parity claims are
    `cargo test` output, not judgment.

## How to add a ruling

State it in one bold sentence, name the phases it binds, give the reason in
two or three more. If a ruling reverses a phase plan's text, edit the phase
plan in the same commit and cite the ruling number there.
