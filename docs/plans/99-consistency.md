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

9. **Oracles are subprocesses, never dependencies.** Binds
   [14](14-testing-and-corpora.md) and every phase that cites an oracle
   diff. mutool, pdftoppm, pdfium_test and qpdf are invoked as external
   CLIs in CI; nothing links them, and their outputs are transient
   comparison references, never committed or redistributed.

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
