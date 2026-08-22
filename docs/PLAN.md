# tinker-pdf — the master plan

A from-scratch, pure-Rust PDF engine, built to replace MuPDF in
[Tinker](https://github.com/ravindu-rev/Tinker) and to stand alone as a
library for Rust, JavaScript/wasm, Python and .NET. Every phase below has its
own plan file with scope, design, milestones and exit criteria; this file is
the map: what depends on what, where the checkpoints are, and — because a
project this size without exits is the biggest risk it has — where the
off-ramps are.

Locked decisions this plan builds on, not up for relitigation here:

- **License:** MIT OR Apache-2.0, dual.
- **Everything hand-rolled.** All PDF logic and all primitives (inflate, JPEG,
  fonts, rasterizer, crypto) are ours. Dev/build/binding tooling is exempt.
  The policy boundary is defined precisely in [00-architecture](plans/00-architecture.md).
- **Tinker is frozen** while the engine reaches parity standalone;
  integration is the final phase. The parity bar is Tinker's own test suite.
- **wasm32-unknown-unknown is a first-class target** from the first commit.
- **Spec baseline:** PDF 1.7 (ISO 32000-1). PDF 2.0 deltas that matter early
  (AES-256/R6, UTF-8 strings, deprecations) are tracked in `pdf20-deltas.md`
  as they arise; full 2.0 conformance is explicitly not a v1 goal.
- **Formats: PDF, and then CBZ, XPS and EPUB** (*decided 16 August 2026*).
  This engine was "a PDF engine and always will be" until the owner answered
  gaps/28's first decision
  with a fourth option that document did not offer: build the three formats
  MuPDF also opened, natively, rather than drop them, convert them out of
  process or keep MuPDF for them. It is what removes the last AGPL code from
  Tinker's tree without losing a format. Sized honestly — CBZ S, XPS L, EPUB
  XL+, the last being a layout engine rather than a renderer. None is built;
  each gets its own plan, and rule 1 applies to all of it, so the ZIP reader,
  the XML parser and the CSS are ours.

## The phases

| Phase | Plan | Size | Lane |
| --- | --- | --- | --- |
| 00 | [Architecture](plans/00-architecture.md) | S | — |
| 01 | [COS & object model](plans/01-cos-and-object-model.md) | L | semantics |
| 02 | [Filters](plans/02-filters.md) | M + L | semantics (wave 2 feeds render) |
| 03 | [Encryption](plans/03-encryption.md) | M | semantics |
| 04 | [Document semantics](plans/04-document-semantics.md) | M | semantics |
| 05 | [Fonts](plans/05-fonts.md) | L + XL | semantics (wave 2 feeds render) |
| 06 | [Content & text](plans/06-content-and-text.md) | L | semantics |
| 07 | [Rasterizer](plans/07-rasterizer.md) | L | raster |
| 08 | [Rendering device](plans/08-rendering-device.md) | XL | raster |
| 09 | [Writing](plans/09-writing.md) | L | write |
| 10 | [Editing](plans/10-editing.md) | XL | post-integration OK |
| 11 | [Forms](plans/11-forms.md) | L | post-integration OK |
| 12 | [Creation](plans/12-creation.md) | M | post-integration OK |
| 13 | [Bindings](plans/13-bindings.md) | M | shipping |
| 14 | [Testing & corpora](plans/14-testing-and-corpora.md) | S + ongoing | doctrine |
| 15 | [Tinker integration](plans/15-tinker-integration.md) | M | shipping |
| 99 | [Consistency rulings](plans/99-consistency.md) | S | overrides all |
| — | [Build sequence](plans/16-build-sequence.md) | — | ordering |
| — | [Gap plans](plans/gaps/README.md) | see index | remediation |

Sizes: S ≈ 0.5 engine-months, M ≈ 1–2, L ≈ 2–4, XL ≈ 5–8. Engine-months are
focused work by one person who knows the codebase; the project parallelizes
across at most ~3 people before coordination eats the gain.

## Dependency lanes

Three lanes run in parallel; nothing in one blocks another until they merge.

```text
semantics lane   01 COS ──► 02 filters(w1) ──► 03 crypto ──► 04 document ──► 05 fonts(w1) ──► 06 content+text
                                                                                                   │
                                                                                          CHECKPOINT A
raster lane      07 rasterizer (starts day 1, no dependencies)                                     │
                       └────────────► 05 fonts(w2) ─► 08 rendering device ◄── 02 filters(w2) ──────┤
                                                                                                   │
write lane       09 writing (starts after 02 w1)                                                   │
                                                                                          CHECKPOINT B
                 13 bindings ──► 15 Tinker integration                                             │
                 10 editing · 11 forms · 12 creation   (after B, any order, post-integration OK)
```

- **Checkpoint A** — everything Tinker consumes *except* rendering: open,
  repair, decrypt with correct user/owner levels and permissions, metadata,
  geometry, outline, structured text, search. Demonstrable on Tinker's own
  fixtures with ports of Tinker's tests. **≈ 12–16 engine-months in.**
- **Checkpoint B** — the integration bar: rendering passes the ports of
  Tinker's `render_pages.rs` exactly, corpus-wide perceptual parity vs the
  MuPDF oracle ≥ 95% and ratcheting, writing round-trips with `qpdf --check`
  green. **≈ another 20–30 engine-months; fonts wave 2 and the rendering
  device dominate — they are the long pole, and historically the part
  everyone underestimates.**
- **Checkpoint C** — bindings published (crates.io, npm, PyPI, NuGet),
  Tinker running on tinker-pdf, MuPDF deleted from Tinker's tree.

Honest total to Checkpoint C: **35–50 engine-months.** No calendar promises;
the checkpoints exist so progress is measurable and the off-ramps exist so a
change of heart has a plan too.

## Off-ramps

Reviewed at every checkpoint, written down now so nobody has to invent an
exit under pressure:

- **At any point before A:** the leaf crates (inflate, JPEG, rasterizer,
  crypto, fonts) are each independently publishable MIT/Apache libraries.
  Killing the engine does not kill their value; publish and stop.
- **At A without continuing to B:** the engine is a complete PDF
  *data* library — parse, decrypt, extract, later write — competitive with
  `lopdf` but with correct security semantics. Tinker keeps MuPDF for
  rendering only and uses tinker-pdf for everything MuPDF's bindings fumbled
  (permissions, auth levels, raw streams, incremental writing for signing).
  That hybrid is a genuinely good end state, not a consolation prize.
- **At B without 10–12:** integrate for viewing (Tinker's phase 1 product),
  keep Tinker's editing plans gated on MuPDF or defer them; editing phases
  land post-integration at their own pace — the plan already assumes this
  ordering is acceptable.
- **Descope levers that never break the architecture:** JBIG2, JPX,
  arithmetic JPEG, mesh shadings, full ICC, linearization — all capability
  flags with documented degradation, all schedulable by corpus hit-rate
  evidence rather than ambition.

## Risk register (project level; per-phase risks live in each plan)

| Risk | Mitigation |
| --- | --- |
| Hand-rolled crypto | Decrypt-only scope; NIST CAVP + RFC vectors as merge gates; constant-time password compares; fuzzed decrypt paths; SECURITY.md inviting review. See [03](plans/03-encryption.md). |
| Unhinted small-text render quality | The honest hard part of parity. Analytic-coverage AA + stem darkening; perceptual budgets, not pixel-exactness; Tinker's goldens are regenerated at integration — "as good", never "identical to MuPDF". See [05](plans/05-fonts.md), [08](plans/08-rendering-device.md). |
| Malformed-file leniency (MuPDF's 30-year moat) | Repair scanner from the first COS milestone; never-panic enforced by fuzzers; corpus pass-rate ratchets and dashboards make the gap visible instead of surprising. See [01](plans/01-cos-and-object-model.md), [14](plans/14-testing-and-corpora.md). |
| JPEG progressive complexity | Baseline first, progressive second, arithmetic behind a capability flag; perceptual oracle diff, not bit-exactness. See [02](plans/02-filters.md). |
| Reduced ICC color in v1 | sRGB assumption + alternate-space fallbacks; shifts vs MuPDF absorbed at golden regeneration; full ICC is a named later capability. See [08](plans/08-rendering-device.md). |
| Base-14 substitutes (Symbol/ZapfDingbats licensing) | Liberation covers the core 12 (OFL); the two symbol fonts are a named open item with a Unicode-remap fallback. See [05](plans/05-fonts.md). |
| Bus factor | ADRs in-repo; spec-section citations in code comments; conformance dashboards; committed fuzz corpora — the codebase must be legible to a future second person. |
| Scope creep | 99-consistency rulings override phase plans; capability flags + hit-rate gates decide schedule, not enthusiasm. |

## Working with this plan

One phase file per implementation effort. Read the phase plan, read
[99-consistency](plans/99-consistency.md) (its rulings win), build to the
exit criteria, update the phase file where reality disagreed — the plans are
living documents, and a limitation discovered late costs a feature, so write
down what you learn the way Tinker's `mupdf-limitations.md` did.
