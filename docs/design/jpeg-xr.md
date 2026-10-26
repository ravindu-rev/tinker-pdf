# JPEG XR

JPEG XR, the image format ISO/IEC 29500-2 §9.1.5.1 recommends for XPS,
decodes in `crates/tinker-pdf-filters/src/jxr/`, hand-rolled from ITU-T T.832
like every other codec in this tree, and the `Kind::JpegXr` that
`crates/tinker-pdf/src/xps/image.rs` sniffs draws through it. What it refuses,
it refuses for a named reason (`JxrRefusal`), and the list of those reasons is
published beside the list of configurations that decode with nothing checking
them — because those are two different things and a green tick over both
would be a lie.

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
  in scope because it is what the platform encoder emits — `32bppBGRA` and
  `64bppRGBA` fixtures carry alpha this way and *not* as 8.3.18's interleaved
  plane (ruling 3).
- **Bounds** in the `bounds_ledger.rs` style: total samples, total
  macroblocks, tiles and components (`MAX_JXR_SAMPLES`, `MAX_JXR_MACROBLOCKS`,
  `MAX_JXR_TILES`, `MAX_JXR_COMPONENTS`), each checked before allocation.

## Non-goals

- **Encoding.** Nothing here writes a JPEG XR file. The fixtures are made by
  the platform encoder, which is a supplier of bytes and not an adjudicator
  (ruling 13). An encoder is [ROADMAP](../ROADMAP.md) row FO-20.
- **The interleaved alpha image plane** (8.3.18), refused by name. The
  separate form is what the platform encoder emits. [ROADMAP](../ROADMAP.md)
  row FI-12 covers it and the four refusals below it.
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
  never applied by this crate. 8.3.8 calls the transformation *preferred* and
  subordinate to the application, and this crate has no application (ruling
  8). Applying it in the facade when converting is [ROADMAP](../ROADMAP.md)
  row FI-14.
- **No new crate and no `Filter` variant.** See below.

## Design

**Where it lives, and why it is not a `Filter`.** `crates/tinker-pdf-filters/`
holds every image codec, and JPEG XR needs nothing that crate does not have.
But it is **not** in `Filter` or `ImageCodec` (`lib.rs`): those enums are PDF
`/Filter` dispatch — what a `/Filter` name resolves to — and no `/Filter` name
reaches JPEG XR. It arrives only as an XPS image part. So it is a free
function beside `png_decode`, with the same shape:
`jxr_decode(bytes: &[u8], limits: &Limits) -> Result<JxrImage, JxrError>`.

A separate crate would depend on nothing `filters` does not already have, and
its one honest argument — fuzz isolation — is answered by a per-target fuzz
entry.

**Warnings are `JxrWarning`, not `Warning`.** `crate::Warning` is the closed
set of leniencies a *PDF stream filter* performs, and its variants are matched
where a reader attaches the object they happened in. A container-level codec
no `/Filter` reaches has no object to attach, so it carries its own set and
leaves that enum closed.

**A module directory.** Like `jpx/`, `jxr/` splits on the seam both formats
share — bit reader, container, headers, entropy-coded coefficients,
transform, colour — with narrow interfaces between them.

**Determinism (ruling 4).** T.832's transform is specified in **integers**:
9.9.7's inverse transform is a lifting structure over `i32` and 9.8's
dequantization is a multiply, so nothing on the pixel path has any reason to
reach for a float. Every module in the directory carries
`#![deny(clippy::float_arithmetic)]` the way `tinker-pdf-shape` does, which
makes that a build failure rather than a convention; `cargo xtask libm` covers
the other half. The fixture rasters are authored in integer arithmetic too,
so any target that regenerates them gets the same sources.

**Bounds (ruling 1).** The structure branches on tiles × macroblocks ×
components × blocks × coefficients, and T.832 bounds every factor without
bounding the product: 8.3.23 and 8.3.24 permit 4096 tile columns and 4096 tile
rows, 8.4.12 permits 4111 components, 8.3.21's dimensions are 32-bit. The
budgets are **totals**, checked before any buffer exists, and each per-item cap
says in as many words that it is not the work cap, as `jpx/mod.rs` does. One
trap is specific to this format and is checked where it is introduced rather
than later: **8.3.25 derives the last tile's width by subtraction**, so a file
whose declared tile widths sum past `MBWidth` would underflow a `u32` into four
billion macroblocks.

## The evidence design

**This is the part that decides the shape of everything else.**

T.832's conformance bitstreams are not freely licensed, so they are not here.
Ruling 13 forbids running another decoder and diffing. There is no oracle.
Three first-party checks stand in its place, and what matters about them is
what each one *cannot* catch.

### 1. The lossless identity — the primary gate

JPEG XR has a genuinely lossless mode and the platform encoder produces it.
So:

1. `RASTERS` in `crates/tinker-pdf-filters/tests/jxr_fixtures.rs` authors a
   raster from integer arithmetic **in this repository**.
2. `make-fixtures.ps1` hands those bytes to `WmpBitmapEncoder` with
   `Lossless = true` and `QualityLevel = 1` — `Lossless` on its own does
   nothing — and commits what comes back.
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
  and both would be wrong. It is not closed — see Risks.

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
the identity again and would fire on nothing the identity does not already
catch. It is on the quantized path that a wrong filter shows as a step the
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
against a deliberately broken filter:
`disabling_the_second_level_filter_makes_the_seam_metric_fire` in
`src/jxr/tests/overlap.rs` disables the second-level POT and asserts the seam
metric crosses its threshold on both filtered modes. If the property stops
testing what it claims, that test says so.

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

1. **Fixtures, Annex A container, clause 8 header layers.**
   `make-fixtures.ps1` beside the fixtures; an `#[ignore]`d test authors the
   rasters and a second writes the fuzz seeds; headers parse for every fixture
   with the *reported geometry* asserted; every refusal reachable by test;
   the `jxr` fuzz target and its committed corpus.
2. **8.7 to 8.12's coefficient layers and 9.4 to 9.8's remapping, prediction
   and dequantization, in both spatial and frequency mode.** Every packet of
   every fixture is consumed to exactly the byte 8.5.3's index table predicts,
   and no tile is dropped; the code tables are prefix-free, complete and one
   value per code.
3. **9.9.2, 9.9.5 and 9.9.7: the photo core transform.** The forward and
   inverse PCT round-trip bit-exactly on random input, which is checkable
   because T.832's transform is a reversible integer lifting structure;
   `cargo xtask libm` passes.
4. **9.9.3, 9.9.6 and 9.9.8: the photo overlap transform, all three modes.**
   The seam property holds for all three overlap modes and fires with the
   second-level filter disabled.
5. **9.10's output formatting: the colour transforms, the bit depths, the
   separate alpha plane.** The lossless identity holds bit-identically across
   every lossless fixture; the monotonicity property holds.
6. **The XPS arm, the refusal table and the docs.** `Kind::JpegXr` draws;
   `docs/features/filters.md` carries JPEG XR with its refusals and its
   unadjudicated list.

**Milestone 2's exit is the index table, not the lossless identity**, because
the identity compares *pixels*, and there are no pixels until 9.9 runs. A
coefficient decoder has to be provable on its own or it is not a milestone
boundary at all. 8.5.3's index table supplies that proof and needs no oracle —
it is the codestream's own statement about where each packet ends, and a
decoder that has lost synchronisation cannot land on it by accident.

## Dependencies

- `crates/tinker-pdf-filters/src/lib.rs`'s `Limits` and its re-export block.
- The Windows Imaging Component JPEG XR encoder, through WPF's
  `WmpBitmapEncoder`, for fixture generation only. It is a **supplier**, never
  an adjudicator (ruling 13), and nothing in `cargo test` invokes it: the
  fixtures are committed and the generation step is a separate, manual
  command, given in `crates/tinker-pdf-filters/tests/jxr/README.md`.
- `fuzz/fuzz_targets/jxr.rs` and `fuzz/corpus/jxr/`, whose seeds are written
  from the fixtures by an `#[ignore]`d test in the crate that owns them, so
  the two cannot drift.
- `crates/tinker-pdf/src/xps/image.rs`'s `Kind::JpegXr` is the consumer, and
  `crates/tinker-pdf-cos/src/jxr_embed.rs` turns a decode into an image
  XObject.

## Risks

| Risk | Mitigation |
|---|---|
| **No conformance bitstreams and no oracle.** T.832's are not freely licensed and ruling 13 rules out diffing against another decoder | The lossless identity above, over rasters this repository authors. It is a total check on the configurations it covers, and `docs/features/filters.md` names the ones it does not. **Not closed** |
| **Code tables transcribed by hand.** Tables 51 to 91 are pure data, and one wrong bit decodes most symbols correctly and then desynchronises | Three structural properties over every table — prefix-free, **complete**, one value per code — plus a `read_vlc` round trip, each beside a test that a broken table fails it (`src/jxr/tests/coefficients.rs`). The completeness check has no exception list, and must not have one: a test with an exception list is a test that has been talked out of firing |
| **A shared misreading.** If the platform encoder and this decoder read a clause the same wrong way, the identity holds and both are wrong | Named rather than mitigated: where the second reader is wrong, both agree and both are wrong. What bounds it is that the decoder is transcribed from T.832's own pseudocode rather than inferred from the encoder's behaviour, so a shared error would have to be a shared *misreading of the same text*, not a shared convention. **Not closed** |
| A wrong decode looks like a photograph: the inverse transform is a smoothing operator over a lapped basis, so wrong coefficients give a soft plausible picture rather than noise | Every unimplemented capability is refused by name and reached by a test in `src/jxr/tests/`; nothing is defaulted past. The same stance `jpx/mod.rs` takes, and sharper here because the lapped basis hides the seam a block transform would show |
| The overlap filter is the defect most likely to survive, because it is wrong only near block edges | The seam property, on lossy ramps, at all three overlap modes — plus a test with the filter disabled, so a property that has stopped firing fails a test rather than passing quietly |
| **A silently ignored encoder knob empties a whole claim.** Three are known: `Lossless` does nothing without `QualityLevel = 1`, `ImageQualityLevel` does nothing once `UseCodecOptions` is set, and tile *slices* count from 1 rather than 0 | `src/jxr/tests/fixtures.rs` asserts the overlap mode, tile count and frequency flag actually reached the codestream, for every fixture that claims one. A fixture set that stops covering something fails a test instead of quietly narrowing |
| Allocation blowup: 4096 × 4096 tiles, 4111 components, 32-bit dimensions, an index table read before any macroblock exists | Totals rather than per-item caps, checked before allocation; the tile-boundary subtraction of 8.3.25 checked where it is introduced. The `jxr` fuzz target, with seeds, because a target with no seeds never reaches a parser behind a four-byte magic |
| The adaptation machinery (8.8-8.12) is stateful across macroblocks, so one wrong update decodes plausibly wrong from that point on rather than failing | The lossless identity is bit-exact and total: any drift in adaptive state fails it at the first affected sample. The multi-macroblock and multi-tile fixtures exist so that a state bug cannot hide in a single-macroblock image |

## As built

The decoder is transcribed from T.832's own pseudocode and tables, not
inferred from fixtures or from a reference implementation.

### The entropy layers

Clause 8.7 to 8.12's entropy layers and clause 9.4 to 9.8's remapping,
prediction and dequantization decode in **both** spatial and frequency mode.

**The index table is an oracle-free total check on the entropy decoder.**
8.5.3 states where every packet begins, so the next larger entry states where
this one must end. The entropy layer is a chain of stateful decisions —
adaptive VLC table selection, adaptive scan reordering, coefficient
normalization, CBPHP prediction — and any one of them being wrong
desynchronises the bit reader, which then goes on decoding plausible-looking
symbols and finishes somewhere else.
`every_packet_is_consumed_to_the_byte_the_index_table_predicts`
(`src/jxr/tests/fixtures.rs`) holds every packet of every fixture to its
predicted byte. That is first-party in the strictest sense: the codestream is
compared against its own statement about itself, not against another
program's opinion of the picture.

**No whole-image `MBBuffer` exists.** 9.9.4's combination maps each HP
coefficient to a fixed position in the sample plane, so the HP half of it runs
at parse time and the per-macroblock buffer is a 256-entry array. Frequency
mode would otherwise force a whole-image copy, because 8.7.9's FLEXBITS
packet is a pass of its own after the HIGHPASS pass; it is instead read
through a **second reader stepped alongside the first**, which works because
8.7.19.1 walks a tile's macroblocks in exactly the order 8.7.18.2 does. That
the two stay in step is checked by asserting the FLEXBITS packet also ends on
its predicted byte, *separately* from the HIGHPASS one — if they drifted apart
the highpass packet would still end correctly and only the picture would be
wrong.

**9.6.1.3's `MBDCMode` is held per macroblock.** It is written by the DC
decode and read by the LP decode. In spatial mode those are adjacent within
one macroblock; in frequency mode 8.7.1 decodes **every** tile's DC band
before **any** tile's LP band, so a single field would hand the LP decode the
DC mode of a different macroblock entirely. Only a frequency-mode file can
show that, which is why the fixture set carries both codestream layouts
rather than the one that is easier to produce.

### The transforms

9.9.2, 9.9.5 and 9.9.7's photo core transform, 9.9.4's coefficient
combination, and 9.9.3, 9.9.6 and 9.9.8's photo overlap transform at all
three values of `OVERLAP_MODE`.

**9.9.7.2's NOTE is read as its arithmetic requires.** It says "the inverse of
`T2x2Th( )` is two successive applications of `T2x2Th`, operating on
variables of the array `iCoeff[ ]` with the same value of `valRound`". Taken
literally that makes the operator order three. It is not: two applications
are the **identity**, so the operator is an involution and its inverse is one
application, pinned by `t2x2h_is_an_involution` and
`the_literal_reading_of_9_9_7_2s_note_is_not_the_identity` in
`src/jxr/tests/transform.rs`. The distinction is not academic — building the
forward operator on the literal reading gives something that is wrong
everywhere, looks principled, and makes the round-trip evidence fail with no
indication why.

**The round trip is real evidence only because the forward direction is
derived independently.** It is not a mirror of the inverse: each of 9.9.7's
lifting steps is reversed from the clause's own text. A slip *mirrored* into
both directions passes the round trip, because a bijection composed with its
own inverse is the identity whatever the bijection is;
`a_mirrored_injection_survives_everything_here` pins that limit. The
DC-flatness property, the only one derived from what the transform *means*
rather than from its structure, is blind to most lifting positions, because a
DC-only block is degenerate and carries zero through them. So the transform's
own evidence is **necessary and not sufficient**, and the check that closes
this stage is the lossless identity.

**The two overlap levels are one geometry at two scales.** The Recommendation
writes 9.9.3's first-level filter and 9.9.6's second-level filter as two long,
unrelated-looking pseudocode functions — one indexed by macroblock and
coefficient, the other by sample. Reading `MbDCLP[x][y][i][j]` as a sample at
`(4x + j % 4, 4y + j / 4)` of a DC plane makes every one of 9.9.3.2's index
quadruples identical to 9.9.6's, with tile boundaries at
`4 * LeftMBIndexOfTile[ ]` rather than `16 *`. Interior, all four edges, all
four corners and all seven soft-tile cases line up. So there is one filter,
called at scale 4 and at scale 16, and a transcription slip cannot hide in one
level and not the other.

**One index in the Recommendation does not line up, and this build departs
from its text.** 9.9.3.2's "right edge for soft tiles" block reads
`MbDCLP[x][y+1][i][4]` where its own non-soft counterpart reads `[3]` and
where the geometry requires `[3]`: the filter runs down a single column, `[3]`
is at `(4x + 3, 4y + 4)` and `[4]` is at `(4x, 4y + 5)` — a different column.
This build implements `[3]`. **No fixture reaches that path**, so the choice
is unadjudicated either way; it is listed below and in
`docs/features/xps.md`.

**The seam property discriminates.** With the filter correct, the largest
second difference at block-edge columns is no larger than the largest inside
blocks, so the ramp is genuinely smooth across the boundaries. With the
second-level filter disabled the edge figure rises well above the interior one
while the interior figure does not move — the signature of a seam and of
nothing else. `seam0` is a check in its own right: encoded at `OVERLAP_MODE`
0, it is unchanged with the filter disabled, which says `OVERLAP_MODE` is
honoured rather than the filter applied unconditionally.

### Output formatting and alpha

9.10's output formatting — the colour transform, the bias, the scaling, the
post-scaling, the clip and the crop — plus A.3.2's separate alpha image
plane. **`jxr_decode` returns a raster.**

**The lossless identity holds bit-exactly on every lossless fixture**: eight
pixel formats, all three overlap modes, the tiled layouts, both frequency-mode
files, and both alpha formats
(`every_lossless_fixture_decodes_to_the_raster_this_repository_authored`,
`tests/jxr_fixtures.rs`).

**An unreadable alpha plane degrades rather than refusing.** A.3.2's separate
alpha plane is a second, complete `CODED_IMAGE( )`. When it cannot be decoded
the image is returned **opaque**, with `JxrWarning::AlphaPlaneDropped` and
`complete` false, rather than refused (ruling 2): a damaged alpha channel
costs transparency, not the picture, and a page that loses its illustration
over a channel the caller may not use is the worse outcome. The warning is
what distinguishes that from a file with no alpha plane at all, which produces
the identical bytes.

### The XPS arm

`crates/tinker-pdf/src/xps/image.rs`'s `Kind::JpegXr` arm and
`crates/tinker-pdf-cos/src/jxr_embed.rs`: **a JPEG XR in an XPS package
decodes and draws**, so all four of ISO/IEC 29500-2 9.1.5's image formats
reach the page.

**No pass-through route exists, and that is a property of PDF rather than a
limitation here.** `png_embed` and `tiff_embed` are mostly deciding modules: a
non-interlaced PNG's IDAT already *is* a `/FlateDecode` stream with
`/Predictor 15`, and four of TIFF 6.0's codings already have a `/Filter` name.
No `/Filter` in ISO 32000-2 Table 6 reads a T.832 codestream — `/JPXDecode` is
JPEG 2000 — so every JPEG XR image costs its pixels, and a page's peak is
*w × h × 3* plus *w × h* for a soft mask where the other three formats cost a
multiple of the part. Stated in `xps/image.rs`'s module note because it is the
one place a caller's peak memory depends on which of 9.1.5's formats a package
happened to use.

**Three rearrangements PDF needs and the format does not have**, none of them
optional and none a leniency: Table A.6's BGR rows permuted into
`/DeviceRGB`'s one order; 16-bit samples swapped from A.7.3's little-endian to
ISO 32000-2 8.9.5.2's big-endian; and A.3.2's alpha, which arrives interleaved,
split out into 11.6.5.3's separate `/DeviceGray` `/SMask` image. Each is
checked by a *relation* rather than a table of expected values — `rgb24` and
`bgr24` are one raster in two channel orders, so after arrangement their
samples must be byte-identical, which needs no expected values and which a
build ignoring channel order fails outright.

**Annex A's resolution tags are read.** XPS 13.4.1 makes 96 dpi the default
and lets an image state otherwise, and an `ImageBrush` states its `Viewbox` in
the image's own units — so a 300 dpi scan with an assumed 96 would draw at
three times its intended size. `WIDTH_RESOLUTION` and `HEIGHT_RESOLUTION` are
the only FLOAT entries this decoder reads; they are metadata rather than
samples, so ruling 4's ban on floats *on the pixel path* is untouched.
`JxrImage` and `Container` do not derive `Eq` because of it, which is the
honest consequence of carrying a float.

**A content type that disagrees with the bytes is named (ruling 10).** When a
part's content type and its magic bytes disagree, the bytes win — a decoder
reads bytes — and the disagreement is reported. `Images::place_one` returns
`Result<Image, XpsElementDefect>`, so the only thing it could say on its
error side is a *refusal* — and a refusal here would lose a picture the
package plainly holds, which is the opposite of what the leniency is for. So
the name rides the **success** side: `Image::lenience` carries an
`Option<XpsElementDefect>`, `place_one` sets it to `ImageMediaTypeMismatch` on
the disagreeing arm, and `paint.rs`'s `State::tile` pushes it into the page's
defects. The asymmetry in where it is set and where it is read is deliberate:
`place_one` runs once per *part* and `tile` once per *use*, and `State::warn`
deduplicates — so a page tiling one mis-declared picture forty times says it
once, and a package that never draws the part says nothing.

Two cases stay silent, and both are the same rule: silence from one
identification rule is not disagreement with it. A part with no resolved
content type, or one whose bytes match no signature, has only one witness and
is believed. And a disagreement whose bytes then fail to decode answers
`ImageUnreadable` alone — the leniency is a fact about a picture that reached
the page, and there is no picture.

`a_content_type_that_disagrees_with_the_bytes_is_drawn_from_the_bytes_and_named`
pins the leniency from the PNG side and `a_tiff_named_by_its_magic_bytes_is_drawn`
from the TIFF side; `one_mis_declared_part_used_twice_is_named_once` pins the
deduplication, which no package that draws its picture once can see. All
three are in `crates/tinker-pdf/tests/xps_images.rs`.

### Decoded but unadjudicated

Configurations this decoder will happily decode and **nothing here checks**.
Named rather than counted as covered, the treatment `docs/features/fonts.md`
gives shaped-but-unverified scripts.

- **The quantised lossy path in general.** This is the big one. The lossless
  identity pins the reversible transform and the QP-1 dequantization case and
  says *nothing* about 9.8's `QuantMap( )` at any other quantizer. The seam
  property is a *relative* comparison within one image, so a filter wrong by a
  constant everywhere passes it. The transform round trip is blind to an error
  mirrored into both directions. And the monotonicity property only orders
  three error totals — it would not notice an error of a few least significant
  bits. A decoder that is exactly right losslessly and wrong at every other
  quantizer passes everything in this repository.

  There is no fourth leg available. Ruling 13: WIC generates a fixture and
  never judges an output, so "decode it with something else and compare" is
  not a check this project can make.
- **The first-level overlap filter across a soft tile boundary.** Needs a
  multi-tile image at `OVERLAP_MODE` 2, and both tiled fixtures were encoded
  at mode 1. This is also the one path where 9.9.3.2's text disagrees with its
  own geometry, and where this build follows the geometry.
- **`HARD_TILING_FLAG`.** WIC does not expose it, so only the soft-tile path
  has a fixture and both settings cannot be produced here.
- **`SHIFT_BITS`.** 8.4.13 reads it for BD16, and every BD16 fixture carries
  zero, so 9.10.5's `iBias >> SHIFT_BITS` and 9.10.7.2's `sample <<
  SHIFT_BITS` are exercised only at the value where they do nothing.
- **`TRIM_FLEXBITS`.** 8.3.15's flag is clear in every fixture, so the trim in
  8.7.19.2 is only ever zero.
- **More than one QP per tile.** `NUM_LP_QPS_MINUS1` and `NUM_HP_QPS_MINUS1`
  are non-zero in no fixture, so 8.7.10.10's `DECODE_QP_INDEX( )` and 9.7.2.2's
  per-macroblock QP selection are reached only in their one-QP form.
- **A damaged codestream's *values*.** The fuzz target proves a damaged file
  does not panic and that a dropped tile is reported; it does not check that
  what survives is what a conformant decoder would produce.

Everything else in `docs/features/filters.md`'s refusal table is refused by
name and reached by a test, which is a different thing from unadjudicated: a
refusal is a decision, and this list is where decisions are absent.

**This list is a limit, not a backlog.** It is the roadmap's named non-goal
NG-06 ([roadmap/NG.md](../roadmap/NG.md)): ruling 13 bars a second decoder
from adjudicating an output, so nothing but T.832's own conformance bitstreams
would close it, and those are not freely licensed. Encoding is a different
case — a scope choice, and [ROADMAP](../ROADMAP.md) row FO-20.
