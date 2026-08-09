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
   `color`, `raster`. Bytes and values in, values out; no COS types, no
   PDF-spec vocabulary in their public APIs. This is what keeps them
   independently fuzzable, testable and publishable — and it is also the
   off-ramp insurance in [PLAN.md](../PLAN.md).

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
