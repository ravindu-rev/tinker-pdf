# JPEG XR

When this is done, the image format ISO/IEC 29500-2 §9.1.5.1 recommends for
XPS — and which nothing outside Microsoft's stack implements — decodes in
`crates/tinker-pdf-filters/src/jxr/`, hand-rolled from ITU-T T.832 like every
other codec in this tree, and the `Kind::JpegXr` that
`crates/tinker-pdf/src/xps/image.rs` already sniffs and refuses by name stops
being a refusal. What still refuses refuses for a *narrower named reason* than
"no decoder here either", and the list of those reasons is published beside
the list of configurations that decode with nothing checking them — because
those are two different things and a green tick over both would be a lie.

## Scope

- **Annex A's tag-based container**: the `II 0xBC` file header, image file
  directories, the four Required tags of Table A.4, and Table A.6's
  `PIXEL_FORMAT` GUIDs. Also a **bare `CODED_IMAGE( )`**, recognised by
  8.3.2's `WMPHOTO\0` signature, because a caller holding bytes should not
  have to decide which of the two it has.
- **Clause 8's header layers**: `IMAGE_HEADER( )` (8.3),
  `IMAGE_PLANE_HEADER( )` (8.4), `INDEX_TABLE_TILES( )` (8.5),
  `PROFILE_LEVEL_INFO( )` (8.6), and the tile geometry Tables 24 to 26 derive
  from them.
- **Clause 8.7's coefficient layers**: `TILE_SPATIAL( )` and the frequency-mode
  tile packets, the DC, LP, HP and FLEXBITS bands, `MB_DC( )`, `MB_LP( )`,
  `MB_CBPHP( )`, `MB_HP( )`, and the run-level block coding of 8.7.18.
- **The adaptation machinery** that makes those parseable at all: adaptive VLC
  table selection (8.8), CBPLP state (8.9), CBPHP prediction (8.10), adaptive
  inverse scanning (8.11) and adaptive coefficient normalization (8.12). None
  of these is optional — the bitstream is not self-describing without them.
- **Clause 9's reconstruction**: coefficient remapping (9.5), prediction
  (9.6), quantization parameter derivation (9.7), dequantization (9.8), and
  sample reconstruction (9.9) — the two-level photo core transform (PCT) and
  **all three modes** of the photo overlap transform (POT).
- **Clause 9.10's output formatting** for the integer path: the internal
  colour transform back to RGB or luma, `SHIFT_BITS`, and clipping and packing
  at BD8 and BD16.
- **A separate alpha image plane** (A.3.2): a second `CODED_IMAGE( )` at
  `ALPHA_OFFSET`, one `YONLY` component at the primary's dimensions. This is
  in scope because it is what the encoder on this machine actually emits —
  `32bppBGRA` and `64bppRGBA` fixtures carry alpha this way and *not* as
  8.3.18's interleaved plane, which is a measurement rather than a guess
  (ruling 3).
- **Bounds** in the `bounds_ledger.rs` style: total samples, total
  macroblocks, tiles and components, each a named constant checked before
  allocation.

## Non-goals

- **Encoding.** Nothing here writes a JPEG XR file. The fixtures are made by
  the platform encoder, which is a supplier of bytes and not an adjudicator
  (ruling 13).
- **The interleaved alpha image plane** (8.3.18), refused by name. The
  separate form is what exists in evidence here; the interleaved one would be
  built when something emits it.
- **CMYK, CMYKDIRECT, NCOMPONENT and RGBE** output colour formats, and
  **YUV420, YUV422 and YUVK** internal formats. The subsampled internal
  formats bring 9.10.3's chroma upsampling and a different macroblock
  geometry; the rest bring a colour pipeline with no consumer in this engine.
- **Fixed-point, half-float and 32-bit float pixel formats** (Table A.6's
  SINT and Float rows) and 9.10.7's postscaling for them. `JxrImage` carries
  8- or 16-bit unsigned samples, and a format whose numbers mean something
  else is refused rather than reinterpreted.
- **Packed output depths**: BD1WHITE1, BD1BLACK1, BD5, BD565, BD10 — 9.10.8.3
  to 9.10.8.6's sub-byte and cross-byte packings.
- **A windowed origin.** A non-zero `TOP_MARGIN` or `LEFT_MARGIN` shifts the
  whole sample grid; the bottom and right margins are the ordinary padding to
  a multiple of 16 and are always handled.
- **`SPATIAL_XFRM_SUBORDINATE` and `SPATIAL_XFRM_PRIMARY`** are reported and
  never applied. 8.3.8 calls the transformation *preferred* and subordinate to
  the application, and this crate has no application (ruling 8).
- **No new crate and no `Filter` variant.** See below.

## Design

**Where it lives, and why it is not a `Filter`.** `crates/tinker-pdf-filters/`
already holds every image codec, and JPEG XR needs nothing that crate does not
have. But it is **not** added to `Filter` (`lib.rs:409`) or `ImageCodec`
(`lib.rs:437`): those enums are PDF `/Filter` dispatch — what a `/Filter` name
resolves to — and no `/Filter` name reaches JPEG XR. It arrives only as an XPS
image part. So it is a free function beside `png_decode`, with the same shape:
`jxr_decode(bytes: &[u8], limits: &Limits) -> Result<JxrImage, JxrError>`.

A separate crate was considered and rejected: it would depend on nothing
`filters` does not already have, and its one honest argument — fuzz isolation
— is answered by a per-target fuzz entry, of which `filters` already has
several.

**Warnings are `JxrWarning`, not `Warning`.** `crate::Warning` is the closed
set of leniencies a *PDF stream filter* performs, and its variants are matched
where a reader attaches the object they happened in. A container-level codec
no `/Filter` reaches has no object to attach, so it carries its own set and
leaves that enum closed.

**A module directory, deliberately.** `jpx/` is the only other one. Both
formats split on the same seam — bit reader, container, headers,
entropy-coded coefficients, transform, colour — with narrow interfaces
between them; `jpeg.rs`, `jbig2.rs` and `ccitt.rs` stay single files because
they do not have that seam. It is a choice, not an accident, and it is
recorded here so it does not read as one.

**Determinism (ruling 4).** T.832's transform is specified in **integers**:
9.9.7's inverse transform is a lifting structure over `i32` and 9.8's
dequantization is a multiply, so nothing on the pixel path has any reason to
reach for a float. Every module in the directory carries
`#![deny(clippy::float_arithmetic)]` the way `tinker-pdf-shape` does, which
makes that a build failure rather than a convention; `cargo xtask libm` covers
the other half. The fixture generator is integer-only too, so the fixture set
is reproducible on any target that regenerates it.

**Bounds (ruling 1).** The structure branches on tiles × macroblocks ×
components × blocks × coefficients, and T.832 bounds every factor without
bounding the product: 8.3.23 and 8.3.24 permit 4096 tile columns and 4096 tile
rows, 8.4.12 permits 4111 components, 8.3.21's dimensions are 32-bit. The
budgets are **totals**, checked before any buffer exists, and each per-item cap
says in as many words that it is not the work cap — the lesson `jpx/mod.rs`
records at length. One trap is specific to this format and is checked where it
is introduced rather than later: **8.3.25 derives the last tile's width by
subtraction**, so a file whose declared tile widths sum past `MBWidth`
underflows a `u32` into four billion macroblocks.

## The evidence design

**This is the part that decided the shape of everything else, and it was
written before the decoder.**

T.832's conformance bitstreams are not freely licensed, so they are not here.
Ruling 13 forbids running another decoder and diffing. There is no oracle.
Three first-party checks stand in its place, and what matters about them is
what each one *cannot* catch.

### 1. The lossless identity — the primary gate

JPEG XR has a genuinely lossless mode and the encoder on this machine can
produce it. So:

1. `RASTERS` in `crates/tinker-pdf-filters/tests/jxr_fixtures.rs` authors a
   raster from integer arithmetic **in this repository**.
2. `make-fixtures.ps1` hands those bytes to `WmpBitmapEncoder` with
   `Lossless = true` and commits what comes back.
3. The decoder must return the raster from step 1, **bit for bit**.

Nothing third-party adjudicates: the pixels going in are ours, so step 3
compares against a value this repository chose. Windows *supplies* bytes,
which ruling 13 admits explicitly; it never says whether the output is right.

**What it catches:** every coefficient-coding, prediction, dequantization,
transform, overlap and colour-conversion defect, on the configurations that
have a fixture. It is total — one wrong bit anywhere fails it — which is
exactly what a format whose failure mode is a *plausible* picture needs.

**What it cannot catch:**

- Anything on a configuration with no fixture. The list of those is published
  in `docs/features/filters.md` by name, not as a percentage.
- Anything on the **lossy** path. Lossless coding pins the reversible
  transform and the QP-1 dequantization case; it says nothing about 9.8's
  `QuantMap( )` for other QPs, and a decoder can be exactly right losslessly
  and wrong at every other quantizer.
- A defect in the *encoder* that this decoder then faithfully mirrors. If WIC
  and this decoder shared a misreading of a clause, the identity would hold
  and both would be wrong. This is the residual of ruling 9's retired
  argument, restated one format over, and it is not closed — see Risks.

### 2. The seam property — aimed at the overlap filter

The POT (9.9.3, 9.9.6) runs **across** block boundaries, not within them. A
wrong overlap filter yields a picture that looks right except for faint seams
at the block edges — which a loosely-set PSNR floor passes, and which is
therefore the defect most likely to survive.

So: encode an image that is smooth across a macroblock boundary *by
construction* — a pure horizontal ramp, whose discrete second difference along
a row is zero everywhere — and assert the decoded second difference at the
boundary columns is no worse than at the interior columns. For all three
values of `OVERLAP_MODE`.

**The seam fixtures are lossy on purpose.** A lossless ramp reconstructs
exactly whatever the overlap filter does, so the property would be measuring
the identity again and would fire on nothing the identity had not already
caught. It is on the quantized path that a wrong filter shows as a step the
identity cannot see, because on that path there is no identity.

**What it catches:** a wrong lifting step, a wrong filter length, a filter
applied within a block instead of across it, and a first-level filter applied
when `OVERLAP_MODE` is 1.

**What it cannot catch:** an overlap filter that is wrong *symmetrically* —
one that distorts the interior exactly as much as the boundary. The property
is a comparison between two regions of the same image, so a uniform error
cancels. The lossless identity covers that case and the two are complementary
rather than redundant.

**A property that fires on nothing is not a property**, so it is checked
against a deliberately broken filter: `overlap::tests` disables the
second-level POT and asserts the seam metric crosses its threshold, with the
catch counted. If that injection ever stops firing, the property has stopped
testing what it claims and the test says so.

### 3. Monotonicity for the lossy path

The same source encoded at rising quality must decode monotonically closer to
it. Three fixtures, one source, `QualityLevel` 48, 16 and 4.

**This check is weak and is here for one reason:** it is the only thing that
reaches 9.8's dequantization at quantizers the lossless identity never
exercises. It catches a gross error — a `QuantMap( )` that is inverted, a QP
index read from the wrong band, a shift in the wrong direction. It would not
notice an error of a few LSBs, and it is not evidence of correctness at any
particular quality.

### What none of the three reaches

Published by name in `docs/features/filters.md` as **decoded but
unadjudicated**, the treatment `docs/features/fonts.md` gives
shaped-but-unverified scripts. A configuration this decoder will happily
decode but nothing checks is worth more written down than counted as done.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
|---|---|---|---|
| 1 | Fixtures, Annex A container, clause 8 header layers | `make-fixtures.ps1` beside the fixtures; an `#[ignore]`d test authors the rasters and a second writes the fuzz seeds; headers parse for every fixture with the *reported geometry* asserted; every refusal reachable by test; `fuzz_jxr` and its committed corpus land; `deny.toml` rows added | M |
| 2 | 8.7 to 8.12's coefficient layers and 9.4 to 9.8's remapping, prediction and dequantization, in **both** spatial and frequency mode | **Every packet of every fixture is consumed to exactly the byte 8.5.3's index table predicts**, and no tile is dropped; the code tables' structural properties (prefix-free, complete, one value per code) hold, each with a counted injection | XL |
| 3 | 9.9.2, 9.9.5 and 9.9.7: the photo core transform | The forward and inverse PCT **round-trip bit-exactly** on random input, which is checkable because T.832's transform is a reversible integer lifting structure; `cargo xtask libm` stays green | M |
| 4 | 9.9.3, 9.9.6 and 9.9.8: the photo overlap transform, all three modes | **The seam property holds for all three overlap modes and its injection fires**, counted | L |
| 5 | 9.10's output formatting: the colour transforms, the bit depths, the separate alpha plane | **The lossless identity holds bit-identically across every fixture**; the monotonicity property holds | L |
| 6 | The XPS arm, the refusal table and the docs | `crates/tinker-pdf/src/xps/image.rs`'s `Kind::JpegXr` draws; `docs/features/filters.md` gains JPEG XR with its refusals **and its unadjudicated list**; this document's As-built section records what actually landed | M |

**Why milestone 2's exit criterion is not the lossless identity**, which the
first draft of this table said it was: the identity compares *pixels*, and
there are no pixels until 9.9 runs. A coefficient decoder has to be provable
on its own or it is not a milestone boundary at all. 8.5.3's index table
supplies that proof and needs no oracle — it is the codestream's own statement
about where each packet ends, and a decoder that has lost synchronisation
cannot land on it by accident.

## Dependencies

- `crates/tinker-pdf-filters/src/lib.rs`'s `Limits` and the re-export block —
  three lines, and nothing else in that file changes.
- The Windows Imaging Component JPEG XR encoder, through WPF's
  `WmpBitmapEncoder`, for fixture generation only. It is a **supplier**, never
  an adjudicator (ruling 13), and nothing in `cargo test` invokes it: the
  fixtures are committed and the generation step is a separate, manual
  command.
- `fuzz/fuzz_targets/jxr.rs` and `fuzz/corpus/jxr/`, whose seeds are written
  from the fixtures by an `#[ignore]`d test in the crate that owns them, so
  the two cannot drift.
- `crates/tinker-pdf/src/xps/image.rs`'s `Kind::JpegXr` is the consumer, and
  it is milestone 6 above. The decoder is a leaf-crate concern and the arm a
  `tinker-pdf` one, so they are separate commits — but the row in
  `docs/ROADMAP.md` closes only when an image actually decodes **and draws**,
  which is why the wiring is a milestone rather than a footnote.

## Risks

| Risk | Mitigation |
|---|---|
| **No conformance bitstreams and no oracle.** T.832's are not freely licensed and ruling 13 rules out diffing against another decoder | The lossless identity above, over rasters this repository authors. It is a total check on the configurations it covers, and `docs/features/filters.md` names the ones it does not. **Not closed** |
| **Thirty code tables transcribed by hand.** Tables 51 to 91 are pure data, and one wrong bit decodes most symbols correctly and then desynchronises | Three structural properties over every table — prefix-free, **complete**, one value per code — plus a `read_vlc` round trip, each with a counted injection. Not theoretical: the completeness check caught Table 52's code table 0 transcribed with its last two rows transposed, and caught it only once the exception it had been granted was taken away |
| **A shared misreading.** If the platform encoder and this decoder read a clause the same wrong way, the identity holds and both are wrong | Named rather than mitigated. It is the residual of ruling 9's retired argument — "where the second reader is wrong, both engines agree and both are wrong" — one format over. What bounds it is that the decoder is transcribed from T.832's own pseudocode rather than inferred from the encoder's behaviour, so a shared error would have to be a shared *misreading of the same text*, not a shared convention. **Not closed** |
| A wrong decode looks like a photograph: the inverse transform is a smoothing operator over a lapped basis, so wrong coefficients give a soft plausible picture rather than noise | Every unimplemented capability is refused by name and reached by a test in `src/jxr/tests/refusals.rs`; nothing is defaulted past. The same stance `jpx/mod.rs` takes, and sharper here because the lapped basis hides the seam a block transform would show |
| The overlap filter is the defect most likely to survive, because it is wrong only near block edges | The seam property, on lossy ramps, at all three overlap modes — plus a counted injection, so a property that has stopped firing fails a test rather than passing quietly |
| **A silently ignored encoder knob empties a whole claim.** Two were found: `ImageQualityLevel` does nothing once `UseCodecOptions` is set, and tile *slices* count from 1 rather than 0, so the first fixture set was fifteen single-tile files whose tile tests passed vacuously | `src/jxr/tests/fixtures.rs` asserts the overlap mode, tile count and frequency flag actually reached the codestream, for every fixture that claims one. A fixture set that stops covering something now fails a test instead of quietly narrowing |
| Allocation blowup: 4096 × 4096 tiles, 4111 components, 32-bit dimensions, an index table read before any macroblock exists | Totals rather than per-item caps, checked before allocation; the tile-boundary subtraction of 8.3.25 checked where it is introduced. `fuzz_jxr` from milestone 1, with seeds, because a target with no seeds never reaches a parser behind a four-byte magic |
| The adaptation machinery (8.8-8.12) is stateful across macroblocks, so one wrong update decodes plausibly wrong from that point on rather than failing | The lossless identity is bit-exact and total: any drift in adaptive state fails it at the first affected sample. The multi-macroblock and multi-tile fixtures exist so that a state bug cannot hide in a single-macroblock image |
| Scope is large enough that a partial landing is likely | Milestones are commit boundaries and each one ends at a testable claim. A build that reads the headers and refuses the coefficient layers by name is a legitimate stopping point; a build that returns pictures nothing checks is not |

## As built

*Filled in as milestones land.*

**Milestone 1.** Landed. One finding overturns a standing assumption and is
recorded because it changed how the rest of the work could be done: **ITU-T
T.832 was reachable from this machine.** The project's standing note was that
itu.int's WAF blocks this box and that clauses would have to be derived from
fixtures. That is not what happened — `curl` with an ordinary browser
User-Agent retrieved the full 230-page Recommendation from
`itu.int/rec/dologin_pub.asp` on the first attempt, and this repository's own
`tpdf text` extracted its clause text. The decoder is therefore transcribed
from the standard's own pseudocode and tables, not inferred, and no reference
implementation has been read. The note about T.88 may still hold; the general
claim that ITU specifications are unreachable here does not.

Two encoder findings, both recorded in
`crates/tinker-pdf-filters/tests/jxr/README.md` because each cost a fixture
set: `ImageQualityLevel` is silently ignored when `UseCodecOptions` is set,
and `HorizontalTileSlices` counts slices rather than extra slices. Both had
the same shape — a knob that does nothing looks exactly like a knob that
worked — which is why the fixture assertions in `src/jxr/tests/fixtures.rs`
now check that the settings reached the codestream.

**Milestone 2.** Landed. Clause 8.7 to 8.12's entropy layers and clause 9.4 to
9.8's remapping, prediction and dequantization decode, in **both** spatial and
frequency mode. 9.9's sample reconstruction is outstanding and refused by name
as `JxrRefusal::SampleReconstruction`, so a build that holds every transform
coefficient and cannot yet turn them into samples says so, rather than
returning the coefficients as a picture — which is what they would look like,
the inverse transform being a smoothing operator over a lapped basis.

Four findings, each of which changed how the work could be checked.

**The index table is an oracle-free total check on the entropy decoder, and it
was not in the original evidence design.** 8.5.3 states where every packet
begins, so the next larger entry states where this one must end. The entropy
layer is a chain of stateful decisions — adaptive VLC table selection,
adaptive scan reordering, coefficient normalization, CBPHP prediction — and
any one of them being wrong desynchronises the bit reader, which then goes on
decoding plausible-looking symbols and finishes somewhere else. All **45
packets across all 21 fixtures** now end on exactly the predicted byte. That
is first-party in the strictest sense: the codestream is compared against its
own statement about itself, not against another program's opinion of the
picture.

**A test with an exception list is a test that has been talked out of
firing.** The check that every code table is a *complete* prefix code was
written with one exception — Table 52's code table 0, which appeared to leave
one leaf unassigned. The exception was wrong: that table's last two rows had
been transcribed in the wrong order, and the "missing leaf" was the symptom,
not a property of the standard. Removing the exception and re-reading the
Recommendation found it. The exception list is empty and must stay empty.

**Predicted injection counts are worth nothing; measured ones are the point.**
Two of the six counted injections in `src/jxr/tests/coefficients.rs` were
written with a guessed number and both guesses were wrong — 8 against a
measured 24, and 2 against a measured 1. The second is the interesting one:
conflating 8.8.4.5's two discriminants diverges on only one of five adaptation
steps, because they usually agree. In a decoder whose state carries forward, a
divergence that rare is not a small bug.

**No whole-image `MBBuffer` exists.** 9.9.4's combination maps each HP
coefficient to a fixed position in the sample plane, so the HP half of it runs
at parse time and the per-macroblock buffer is a 256-entry array. Frequency
mode would otherwise have forced a whole-image copy, because 8.7.9's FLEXBITS
packet is a pass of its own after the HIGHPASS pass; it is instead read
through a **second reader stepped alongside the first**, which works because
8.7.19.1 walks a tile's macroblocks in exactly the order 8.7.18.2 does. That
the two stay in step is checked by asserting the FLEXBITS packet also ends on
its predicted byte, *separately* from the HIGHPASS one — if they drifted apart
the highpass packet would still end correctly and only the picture would be
wrong.

**Milestone 3.** Landed: 9.9.2, 9.9.5 and 9.9.7's photo core transform, and
9.9.4's coefficient combination. The overlap filter and 9.10's output
formatting remain, so `JxrRefusal::SampleReconstruction` still fires.

**One reading of the Recommendation had to be settled by measurement.**
9.9.7.2's NOTE says "the inverse of `T2x2Th( )` is two successive applications
of `T2x2Th`, operating on variables of the array `iCoeff[ ]` with the same
value of `valRound`". Taken literally that makes the operator order three. It
is not: two applications are the **identity**, so the operator is an
involution and its inverse is one application. Measured over 20 000 random
vectors at both values of `valRound`, and then pinned in
`src/jxr/tests/transform.rs`. The distinction is not academic — building the
forward operator on the literal reading gives something that is wrong
everywhere, looks principled, and makes the round-trip evidence fail with no
indication why.

**The round trip is real evidence only because the forward direction is
derived independently.** It is not a mirror of the inverse: each of 9.9.7's
lifting steps is reversed from the clause's own text. Four injections applied
to the inverse alone are caught on 256, 130, 256 and 256 of 256 vectors. The
130 is `valRound`, which changes a result only when a parity works out.

**What the transform's own evidence does not reach, by name.** A slip
*mirrored* into both directions passes everything in that file, and the count
is recorded as a zero: a bijection composed with its own inverse is the
identity whatever the bijection is. The DC-flatness property, which is the
only one derived from what the transform *means* rather than from its
structure, catches one of the four injections and is blind to three — a
DC-only block is degenerate, so most lifting positions carry zero through it.
So the transform's own evidence is **necessary and not sufficient**, and the
check that closes this stage is the lossless identity in milestone 5.
