# Build sequence for the remaining gaps

Written August 2026, after a survey of every open item in
[STATUS.md](../STATUS.md) against the live tree. The phase plans say what each
feature *is*; this says what order to build them in and why, and — more
usefully — where a half-implementation would be worse than none.

Ranked by **value to the parity goal ÷ risk of silently-wrong output**.

## Two facts that set the order

**Only one parity function is really in play.** Tinker's engine seam is eleven
functions wide: open, page count, metadata, permissions, geometry, encryption
info, render, text, search, outline. Almost every remaining gap touches
`render` and nothing else. Form calculations and encrypt-plus-linearize
contribute *nothing* to parity — nothing in Tinker's seam runs `/AA` or writes
a file. They are ranked accordingly, however well specified they are.

**Tinker hands the engine a `FontProvider` or every non-embedding document
renders textless.** That is a render-parity blocker, it is not in any phase
plan, and it belongs before anything else in the render list.

## The sequence

### Done since this was written

1. ~~Gate `pdfcmp` on changed pixels rather than the mean.~~ It is the
   instrument every claim below is measured with, and it was measuring
   something Tinker does not. A glyph moving one pixel changes a few hundred
   pixels completely and moves the mean by a ten-thousandth.
2. ~~Fix the `xtask` tools loop.~~ `"xtask"` resolved to `tools/xtask`, which
   does not exist, so the one crate whose job is enforcing the rule the
   compiler cannot enforce was exempt from it.
3. ~~Annotation `/BBox` clipping.~~
4. ~~Form XObject `/BBox` clipping.~~ The same defect one layer down, hitting
   ordinary page content rather than annotations.

### Next, cheap and high value

5. **Wire Tinker's `FontProvider` through the seam.** Without it, every
   document that does not embed its fonts renders with no text at all — the
   widest possible class of real files.
6. **A golden hash table and the cross-target determinism test.** Ruling 4 is
   claimed and unproven. Land it *before* the render items below start moving
   pixels, so each later change reads as a deliberate golden update rather
   than as noise.
7. **Appearance resource scoping, and indirect `/F`, `/AS` and `/Subtype`.**
   Image stamps and FreeText draw nothing or the wrong thing; `/F 9 0 R`
   defeats the hidden-flag check outright.
8. **Settle the `Canvas` alpha convention.** `clear` and `encode` write
   straight colour, `blend` writes premultiplied, `pixel` reads straight.
   They agree only while the destination is opaque — which every current test
   guarantees and which the first group buffer breaks.

### Then, contained render parity

9. **CCITT packed 1-bpp *and* the `resources.rs` rewire, in one commit.**
   There is no end-to-end CCITT test in the tree, so flipping the decoder
   without the caller ships a photographic negative of every fax with a green
   suite.
10. **CCITT `/EndOfLine`, `/EndOfBlock`, and true T.4 mixed mode for `/K > 0`.**
    The tag bit is read at the top of every row whether or not an EOL preceded
    it, so every mixed-mode row that is not EOL-separated loses a bit and
    decodes to noise.
11. **Blend modes, with no group machinery.** The whole separable and
    non-separable set ships on its own; the non-separable helpers must be
    written in integers, and `cargo xtask libm` will *not* catch a float
    regression there.
12. **Tiling patterns.** Common in real files and currently a documented
    no-op.

    **This depends on item 8, which the survey did not catch.** The obvious
    implementation — clip to the filled path, then replay the tile's content
    once per lattice position with a translated CTM — is correct and
    pathologically slow. Each tile needs its own bounding-box clip, every clip
    goes through `save_state`, and `save_state` clones a page-sized mask: a
    hatch with a 10pt step over A4 is some 4,800 tiles and several gigabytes of
    memcpy. Dropping the per-tile clip to avoid that is precisely the
    silently-wrong trade this document warns against elsewhere.

    The right shape is the one every real implementation uses: rasterise the
    tile *once* into a small offscreen buffer and blit it across the lattice.
    That needs `Canvas::composite` and a settled alpha convention — item 8.
    Do item 8 first and this becomes straightforward; do it first and it will
    be rewritten.
13. **The corpus runner and ratchet, offline half.** Everything but the fetch
    is testable today against `testdata/` and builder output.

### Large; ship the partial, name the remainder

14. **Transparency groups and soft masks.** The remainder after item 11, and
    the only genuinely week-scale item here.
15. **Run the corpus.** The blocker STATUS names. Everything below this line
    should be scheduled by its hit-rate report rather than by judgement
    (ruling 3).

### Gated on that report, or off the parity path

16. Mesh shadings 4–7.
17. JBIG2 generic region and MMR — only with the "no region decoded" refusal
    wired first; see below.
18. Encrypt-plus-linearize, and the `/ID` and `/Size` writer defects. No
    parity value; the missing `/ID` is a live 7.5.5 non-conformance on the
    *existing* encrypted path, which is the only reason it ranks this high.
19. Forms `/AA`, `/CO` and `/Names /JavaScript` **surfaced as data only** —
    read them, report them, stop. Do not build the interpreter.
20. JPX **header probe only**, for colour-space inference and `/SMaskInData`.

## Where a half-implementation is worse than none

This is the part worth re-reading before starting anything above.

- **JBIG2 without a "no region decoded" refusal.** Generic region covers the
  scanner lineage; files from jbig2enc and OCRmyPDF are symbol-dictionary plus
  text region. Skipping unknown segments and returning the page yields a
  **blank white page reported as success** — indistinguishable from a correct
  decode of a blank scan, and strictly worse than today's grey placeholder.
  The refusal path is not polish, it is the feature.
- **JBIG2 or CCITT with inverted polarity.** JBIG2 has 1 = black; a 1-bit
  DeviceGray image has 0 = black. Ship a negative and no "it is not the
  placeholder" assertion will catch it.
- **JPX partials.** Decoding only the lowest resolution level still needs the
  codestream parser, tier-2, tier-1 and dequantisation; it saves the wavelet
  and produces a broken-looking image. And calling a header probe "JPX
  support" is the claim to avoid — the placeholder is already correctly sized
  and positioned.
- **Soft masks with the wrong default backdrop.** `/Luminosity` with no `/BC`
  defaults to **black**, meaning fully masked. White or transparent inverts
  every drop shadow in the corpus and still looks plausible on a light page.
- **Transparency groups without backdrop removal or knockout.** A
  double-counted backdrop renders *darker*, not broken — a "groups are
  supported" claim that is quietly wrong everywhere.
- **Tiling patterns anchored to the paint-time CTM.** 8.7.3.1 anchors them to
  the parent stream's default space. Anchored wrongly, the lattice slides
  under each transform and reads as a small offset rather than a defect —
  turning a tested, documented, honest degradation into a false capability.
- **`/AA` calculations with non-transactional writes.** A script that sets
  three fields and then submits leaves a file that looks filled and is wrong,
  which is worse than one that was never calculated.
- **A ratchet whose "rendered" means "rendered cleanly".** That makes ruling 2
  — degrade, do not fail — count *against* the pass rate, and turns every
  honest placeholder into a regression. Define it as "returned a bitmap
  without crashing or timing out" and track degradation on its own axis.

## One sequencing rule

Tinker's goldens are MuPDF output. Re-baseline **once**, after item 14 — not
per item. Six re-baselines means each one hides the next regression, which is
the exact failure the visual-regression suite exists to prevent.
