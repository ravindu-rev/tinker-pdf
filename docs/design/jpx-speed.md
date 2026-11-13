# JPEG 2000 speed

The JPEG 2000 decoder in `crates/tinker-pdf-filters/src/jpx/` decodes every
codestream it accepts exactly, and decodes all of it every time: every
code-block of every resolution, through tier-1's per-coefficient state and
an MQ decoder that keeps its registers in a struct, up an inverse wavelet
that copies each row and column into a scratch line, to the full image —
even for a page that draws the image at a sixteenth of its size, or shows a
corner of it. Row P-10 makes it fast without changing one decoded sample:
packed tier-1 state, the MQ decoder in registers, row-blocked lifting, a
decode request that drops resolution levels and asks for a window, the
page's drawn size and visible window turned into that request, and
independent code-blocks and synthesis on P-15's pool. This design settles
each mechanism, the invariant they all keep, and the order they land in.

## Scope

- **The invariant**: every decode of every committed codestream stays
  byte-identical at a full request, and what holds it.
- **Tier-1's packed per-stripe state** (P-10.5).
- **The MQ decoder kept in registers**, which JBIG2 shares (P-10.6).
- **Row-blocked lifting** for both inverse wavelets (P-10.7).
- **The decode request**: levels discarded and a window, its geometry under
  T.800 B.5, and the counts a decode hands back (P-10.2, P-10.3).
- **The page's request**: how the drawn size and the visible window become
  one, and what a cache keys on (P-10.8, P-10.9).
- **Code-blocks and synthesis on P-15's pool** (P-10.10, P-10.11).
- **Guard A7's JPX half** and the counters it reads.
- **Memory**: the JPX page at 72 dpi within 140 MB.

## Non-goals

- **JPEG 2000 Part 2** (FI-13): its markers stay refused by name.
- **The encoder** (ED-26). The test-only writers stay test-only.
- **`unsafe` and SIMD intrinsics** (H-16's default). Speed comes from loops
  the compiler can vectorise over plain slices, and from cores.
- **A new arithmetic.** The plane format is the one gap 18a settled before
  the decoder existed — `i32` planes at Q0 for the 5/3 and Q12 for the 9/7,
  `i64` constants at Q24, every product in `i64` at Q36 rounded back with
  `(p + (1 << 23)) >> 24` (`wavelet.rs`'s header) — and nothing here changes
  it.
- **Quality layers as a request.** Dropping layers is a lossy decode of a
  different picture; dropping levels and cropping are decodes of the same
  picture at a smaller size.
- **Decoding outside tier-2's order.** Tier-2 still reads every packet header,
  because a packet's position is only known from the headers before it.

## Design

### The invariant: every committed decode, byte for byte

Each mechanism below is a faster way to compute the numbers the decoder
computes today, so at a full request — no level discarded, no window — every
committed codestream decodes to today's samples exactly. What holds it:

- `tests/jpx_reference.rs`, the committed reference decodes: the reversible
  ones byte-identical, the irreversible ones within a tolerance;
- `tests/jpx_annex_j.rs` (T.800 J.10's published samples), `jpx_annex_h.rs`,
  `jpx_poc.rs` and `jpx_seeds.rs`;
- the `jpx` determinism fingerprint (`tests/determinism.rs`), on the four
  targets;
- and, because a tolerance cannot see a sample move by one level,
  `tests/jpx_decodes.rs` (P-10.2): every codestream under `tests/jpx/`
  decoded at a full request and pinned by its geometry,
  its precision and the CRC-32 of its samples and its opacity channel, taken
  before any mechanism lands, so each later step proves it moved nothing. A
  moved pin is a finding, never an edit.

The writer-made codestreams the tests below build are written by this
repository's own test writers and read back by this repository's decoder, so
an agreement between two paths of it — a reduced decode and a full decode of
the reduced codestream, a window and the crop of the whole — proves the
paths agree and not that either is right. Correctness at a full request
rests on the reference decodes and the published annexes; the speed work
adds paths and holds each to that one.

### Tier-1: packed per-stripe state (P-10.5)

The transcribed `Block` (`tier1.rs`) keeps a flag a coefficient — σ, the
sign, the refinement flag and π — and forms each context from eight
neighbours read out of `Vec<bool>`, over a stripe list every pass collects.
The decoder's `Packed` state is one `u64` word per stripe column instead:
σ over a window three columns wide and six rows tall — the column and its
neighbours on either side, from the row above the stripe to the row below
it — the signs of the window's coefficients that D.3 reads, the column's own
six and its neighbours' four, and the four coefficients' π and refined bits.
So a context of Table D.1, D.3 or D.4 is a shift, a mask and a lookup on one
word: a row's nine σ bits index a 512-entry Table D.1 for the block's
orientation, the four across the coefficient with their signs index a
256-entry Tables D.2 and D.3, and Table D.4 reads whether any of the nine is
set beside the refined bit. The tables are `zero_coding_context`,
`sign_coding_context` and `magnitude_refinement_context` evaluated at every
index when the crate is compiled. When a coefficient becomes significant the
pass sets its bits in its own word and in the up to eight words around it,
the code-block's words bordered by one on every side so an edge needs no
check. Magnitudes and the known plane stay per coefficient, since they are
values, not flags. Under the vertically causal mode a stripe's first row sets
nothing in the words of the stripe above, whose row below then reads
insignificant, so causal and not are one table. The test-only encoder keeps
the transcribed `Block`, so a round trip pits the packed decoder against an
independent state rather than against itself (gap 17's lesson, `tier1.rs`'s
header).

### The MQ decoder in registers (P-10.6)

`MqDecoder` (`crates/tinker-pdf-filters/src/mq.rs`) keeps A, C, CT, the byte
position and the decision count as one value, and `MqDecoder::run` takes that
value into a run's locals for a whole coding pass — a JBIG2 row likewise —
which writes it back once, when the run is dropped. Tier-1's passes are
written once over a source of decisions, an MQ run or D.6's raw reader, so an
arithmetic pass carries no branch on which it reads. RENORMD shifts by A's
leading-zero count, as many bits a step as the byte being read has left, so
it runs BYTEIN only when CT runs out — exactly where E.3.3's bit-at-a-time
flowchart runs it — and takes at most one step more than the bytes it reads.
Its API and its decision count do not change: `take_decisions` feeds JBIG2's
work budget, so a run counts exactly the decisions the struct decoder does,
and JBIG2 and JPEG 2000 share it as before. The struct decoder, E.3's
flowcharts register for register, stays in `mq.rs`'s tests as the reference.

### Row-blocked lifting (P-10.7)

Each arithmetic's `Arith::synthesise_2d` lifts the plane where it lies: the
horizontal pass in place along each row, and the vertical pass over the
block of whole rows, each lifting step updating every row of one parity from
the two rows beside it, so the inner loop runs along contiguous memory for
every step and the compiler can vectorise it. The symmetric extension
(F.3.7's mirror) is applied at the two ends of a line, or of the block, by
index, not through `mirror` at every sample, and no line is copied out and
back. The 5/3 lifts its `i32` samples with wrapping arithmetic, so a sum of
two coefficients only a hostile stream asks for wraps alike on every target
rather than panicking, and allocates nothing. The 9/7 keeps its order of
operations exactly — the scaling, then delta, gamma, beta and alpha, each
product formed in `i64` and rounded by `mul_q24` — so every intermediate is
the one a line at a time computes, and each pass ends clamped into the plane
bound as `Fixed`'s synthesis clamps it; the overflow proof in `wavelet.rs`'s
header still holds, step for step. A 9/7 pass lifts the plane's own `i32`
samples where every sample it reads is within `2^25` at Q12 — 8 192 sample
units, thirty-two times an 8-bit picture's range — from which that proof
leaves no intermediate past `i32`, and every committed codestream's decode
lifts so; otherwise in `i64`: each row widened for the horizontal pass, and
the vertical pass over strips of 64 columns, whole rows of each widened into
one block. The windowed synthesis P-10.3 lands keeps its rows and columns. The
line-at-a-time synthesis, each line copied into a scratch and every
neighbour read through `mirror`, stays in the tests as the reference the
row-blocked lifting is held to.

### The decode request (P-10.2, P-10.3)

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct JpxRequest {
    /// Resolution levels dropped from the top; 0 decodes the full size.
    pub discard: u8,
    /// The rectangle wanted, in the reduced image's own pixels from its
    /// top-left corner; `None` for the whole image.
    pub window: Option<JpxWindow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JpxWindow { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JpxCounts {
    /// Inverse-wavelet levels synthesised, every tile-component summed.
    pub levels_synthesised: u64,
    /// Code-blocks tier-1 decoded.
    pub code_blocks_decoded: u64,
}

pub fn jpx_decode_request(
    input: &[u8],
    limits: &Limits,
    request: &JpxRequest,
    warnings: &mut Vec<Warning>,
) -> Result<(JpxImage, JpxCounts), FilterError>;
```

`jpx_decode` is `jpx_decode_request` at the default request with the counts
dropped, so no caller that asks for the full image changes. The counts come
back as a value because the leaf has no `Work` (ruling 8); the facade adds
them to its tally where it decodes (P-10.8). The request is a plain,
hashable value, because it is what the facade's image cache keys on, and the
executor P-10.10 adds is a parameter beside it, not a field of it.

**Levels discarded.** A discard of `d` drops the top `d` resolutions of every
tile-component. The applied discard is the request's, clamped to the fewest
levels any tile-component of the codestream has, so a discard past a
tile-component's own levels is clamped, never a panic, and every plane
comes out at one scale — and clamped to none where a `pclr` palette maps a
component (I.5.3.4): its samples are indices, and a lower resolution of
indices is their average, which selects an entry neither sample chose, so
such a codestream at a smaller size is not the same picture. `JpxImage`
gains `discard`, the applied figure, so a caller knows the scale it got, and
`levels`, the most it could have been, which a caller's cache reads its
next draw's discard against. Tier-1 skips every code-block of a dropped
resolution; `wavelet::ladder` stops at resolution `N_L − d` of each
tile-component and hands back that resolution's plane, whose rectangle is
B.5's `trx0 = ceil(tcx0 / 2^d)` to `trx1 = ceil(tcx1 / 2^d)` (B-14) — which
tier-2 has already computed as the resolution's `x0..x1`.

**The reduced grid.** The image the reduced decode returns lies on the
reference grid reduced by `2^d`: its area is `ceil(XOsiz / 2^d)` to
`ceil(Xsiz / 2^d)` across, likewise down, each tile's rectangle is its
bounds reduced the same way, and `blit` maps a reduced reference sample
`x_d` to component sample `floor(x_d / XRsiz)` of the plane at resolution
`N_L − d`, as the full decode maps `x` to `floor(x / XRsiz)` — so a
subsampled component replicates on the reduced grid as on the full one, and
every tile, edge tiles included, lands where B.5 puts it. The reduced width
is between `floor(W / 2^d)` and `ceil(W / 2^d)` for a full width `W`, by the
origin.

**A window.** The window is a rectangle on the reduced image; the decode
returns its samples, clamped to the image, and a window off the image is an
empty image, never a panic. Which samples are needed is decided per
tile-component, from the top resolution down: a sample at index `n` of a
resolution's interleaved signal depends on the interleaved inputs within
`R` of it — `R = 2` for the 5/3 and `R = 4` for the 9/7, the reach of F.3.8's
lifting steps over one level — so the window at resolution `r`, widened by
`R` each way and clamped to the resolution's rectangle, is what that level's
synthesis reads. Its even positions are resolution `r − 1`'s window, widened
in turn, and its odd positions select the high bands' rows and columns, so
each subband of each level has a rectangle that is needed, and tier-1
decodes exactly the code-blocks that meet it. A mirror at a resolution's
edge reads within `R` of the edge, inside the widened window, so the
symmetric extension needs nothing more. Tiles wholly outside the window
skip tier-1 and synthesis; tier-2 still reads their packet headers. The
ladder synthesises each level's widened rows and, of those, the widened
columns, as sub-lines whose first sample's parity is its absolute index's;
a value within `R` of a sub-line's end that is not the resolution's own end
is wrong and is never read, because the window's samples depend on nothing
outside the widened lines. Annex G's transform, the level shift and the
`blit` then run over the window alone.

### The page's request (P-10.8, P-10.9)

**The drawn size.** P-09.8 gives `GlyphSource::image` an image request
carrying the drawn size — the device pixels the unit square spans along each
of the image's own axes, from its transform in the page frame, never a
tile's. The facade's JPX path turns it into a discard: the most levels `d`
for which `W >> d` by `H >> d` — the fewest samples a reduced image of the
codestream's `W` by `H` can have, whatever its origin — is still at least the
drawn pixels on both axes, and at least one, `W` and `H` read from the
codestream's own header, and the decoder clamping it to `JpxImage::levels`.
An image with a soft mask or a `/Mask`, which are sampled against the grid
they were drawn for, and one under an `/Indexed` space, whose samples are
indices, are decoded whole. The reduced samples stand for `2^d` of the
image's own each way (`Reduced`), placed from where the first reduced cell
starts: `ceil(XOsiz / 2^d) · 2^d − XOsiz` columns into the image, none for
an image at the grid's origin (`Reduced::skip`).
The image cache keys on the reduction, read on a hit from the size and the
levels the name's first decode found, so a hit returns exactly what a miss
computes. `Page::images`, `to_svg` and every extraction path ask for the
full size. Because the drawn size is the page frame's, a tile and the page
pick the same levels (ruling 5).

**The visible window.** P-10.9 adds the window: the request carries the part
of the unit square the canvas reaches and the part the clip and the soft mask
leave visible of it (`ImageRequest::reach`, `ImageRequest::visible`), the
facade's JPX path takes it in the samples of the image the chosen levels
leave, widened by the sampler's margin (below), and `DecodedImage` carries the
window it covers, its width and height staying the whole image's, so the
rasterizer samples the whole image and reads the held rectangle
(`ImageSource::window`), a sample outside it read as its nearest held one. The
sampling policy (`sampling_for` in `crates/tinker-pdf-raster/src/image.rs`)
decides on the **whole** reduced image's dimensions, which the window carries
beside its samples, never on the window's: the halvings an image is reduced
by before its area filter runs depend on its size, and a window that halved
differently would draw different pixels. The cache keys on the window as
well. A window that holds the whole image is none, and one that shows nothing
holds a sample, so the stand-in covers every pixel of the square.

**The margin a region needs for ruling 5.** A destination pixel reads the
decoded samples its filter reaches: one beyond its nearest sample for
bilinear's second tap, and for the area filter its footprint — at most two
texels per axis once the pyramid has halved the image — at the pyramid level
it samples, which spans `2^h` decoded samples per texel after `h` halvings.
The facade works the policy out from the drawn size the request carries, and
the draw from its own transform, so the two may round a threshold apart by a
level; the window is therefore widened by `(⌈footprint⌉ + 2) × 2^(h + 1)`
samples on each side, and each edge is rounded out to a multiple of
`2^(h + 1)`, so every 2 × 2 block the pyramid averages inside the window is the
block the whole image's pyramid averages at either level. With that margin a
region's visible pixels read only samples its window holds, and a window
decode is the crop of the whole decode (P-10.3), so a tile is byte-equal to
the page under it.

**A window refuses where the whole image does.** A tile's window and the
page's differ, so whether a decode refuses must not depend on its window, or
a tile of a damaged codestream would draw what the page refuses. The decoder
accounts for every code-block of a kept resolution in index order — a block
the window leaves out has its pass count checked and its cost charged as
though decoded — decodes every block of a tile-component that signals
segmentation symbols, whose D.5 check is made only by decoding, checks every
tile's structure as reconstructing it would, and charges the output at the
image the discard returns whole.

**The image-run shape ruling 5 pins.** Whether an image draw joins the run
held back so that abutting images do not conflate is decided by its overlap
with what the run holds on the canvas — the coverage it accumulated wherever
the clip hides it, composited through the clip only when the run ends. So a
draw that can join a run is decoded over what its canvas reaches unless every
sample is opaque: an opaque image's stand-in sample covers each pixel as its
own would, so it may be held to what the clip shows, and one with opacity of
its own is held to what the canvas reaches, where every pixel reads its own
samples. Over its canvas a run then holds what it holds of the whole image,
and decides what the whole image's run decides there — in a tile, ruling 5's
named shape included, the canvas's decision H-15's default keeps. A draw that
composites alone, in a knockout group or on a page too large for runs, takes
what the clip shows.

### Code-blocks and synthesis on P-15's pool (P-10.10, P-10.11)

The decoder takes the pool as `&dyn tinker_pdf_pool::Executor` beside its
request ([parallelism](parallelism.md), "JPEG 2000: P-10's seam"), filled by
the facade from the caller's `Parallelism` and by a test with its own scoped
threads, so `tinker-pdf-filters` stays a leaf (ruling 8).

**Code-blocks.** Tier-1 walks every code-block of the kept resolutions in
index order (tile, component, resolution, band, precinct, block), checking
each one's pass count and charging the work budget its area times its passes,
known from tier-2, and lists the blocks the request reads — a discard and a
window leave the rest unlisted, so none of them reaches a worker — so the
block the budget refuses is the same at every thread count, and the same with
any window (a window refuses where the whole image does). The listed blocks
are decoded on the pool, each with MQ contexts of its own, each into a
pre-sized slot, and the slots are read in index order: the first block whose
decode refused gives the refusal, as a serial decode would have stopped there,
the budget left as it stood once that block was charged, and a refusal the
walk stopped at after it gives its own. Serial unless the caller's
`Parallelism` asks for threads: `jpx_decode_on` takes the executor, and
`jpx_decode_request` is it on the caller.

**Synthesis.** A horizontal pass's rows and a vertical pass's blocks of rows
are independent within a level, and so are tiles, and components where no
multiple-component transform joins them; each writes a pre-sized slice of
its plane, joined in index order. The levels of one tile-component stay in
order, since each reads the one below. At the 9/7's single-thread target,
four threads within the target need most of the decode on the pool, which is
why synthesis joins tier-1 there.

### Guard A7's JPX half

`JpxCounts` is what a decode did: `levels_synthesised`, the inverse-wavelet
levels run, every tile-component summed, and `code_blocks_decoded`, the
blocks tier-1 ran. The facade adds both to MS-01's tally where it decodes
(new `Stats` counters, P-10.8), so a page's render reports them. A7's JPX
half, `a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer`
(`crates/tinker-pdf/tests/jpx_images.rs`), draws a codestream of `N` levels
at a quarter of its size and reads `N − 2` levels a tile-component; the
leaf's own `guard_a7_levels_decoded_are_the_levels_needed` (P-10.2) holds a
reduced decode to `N − d` levels and to the code-blocks of the kept
resolutions alone.

### Memory: the JPX page at 72 dpi within 140 MB

The decode holds, today, every tile's segments, then every code-block's
coefficients at once (an `i32` a coefficient), then a tile's planes (an
`i32` a sample a component), then the output. The plan:

- **Tier-1 a tile at a time.** Tier-2 reads every packet header first, as it
  must; tier-1 then runs on one tile's blocks right before that tile is
  reconstructed, and each block's coefficients are dropped once
  `write_band` has placed them, so the coefficients alive at once are one
  tile's.
- **A reduced decode keeps only what it draws.** At 72 dpi a 300-dpi page's
  image is drawn at about a quarter of its size, so the facade discards two
  levels: tier-1 decodes the blocks of the kept resolutions, about a
  sixteenth of the coefficients, and the planes and the output are the
  reduced image's.
- **No plane format change.** The 9/7's planes stay `i32` at Q12: the
  memory comes from holding less, not from narrower samples.

MS-05's peak-memory test holds the JPX page at 72 dpi within its band, at or
under 140 MB (P-10.13).

## Milestones

| Step | What lands | Exit test |
| --- | --- | --- |
| P-10.2 | `JpxRequest`'s `discard`, `jpx_decode_request`, `JpxCounts`, the reduced grid in `blit`, the clamp; `tests/jpx_decodes.rs`'s pins; the fuzz target's discard knob | `jpx/tests/reduced.rs` `a_reduced_decode_is_the_full_decode_of_its_own_low_bands` (writer-made 5/3 and 9/7 codestreams of `N` levels, decoded with `d` dropped, equal the full decode of the same bands written with `N − d` levels and expounded steps, sample for sample) and `guard_a7_levels_decoded_are_the_levels_needed`; every committed decode byte-identical at discard 0 (`jpx_decodes.rs`, `jpx_annex_j.rs`, `jpx_annex_h.rs`, `jpx_poc.rs`, `jpx_reference.rs`, `jpx_seeds.rs`) |
| P-10.3 | `JpxRequest`'s `window`: the needed rectangles per subband, the code-blocks that meet them, the windowed ladder, transform, level shift and blit; the fuzz target's window knob | `jpx/tests/window.rs` `a_window_decode_is_the_crop_of_the_full_decode` (every fixture in `tests/jpx/` and writer-made multi-tile, multi-precinct 5/3 and 9/7 codestreams, random windows at every discard level, equal to the same rectangle of the full decode) and `a_window_decodes_only_the_code_blocks_it_reaches`; P-10.2's tests and every committed decode unchanged |
| P-10.4a | The decode group's two-levels-discarded and window operations | `xtask/src/bench.rs` `the_committed_baseline_agrees_with_the_tree` with the four operations in it |
| P-10.4b | Their figures, in the timing phase | the figures committed and `cargo xtask baseline` holding them |
| P-10.5 | The packed per-stripe state | `jpx/tests/tier1.rs` `the_packed_contexts_are_the_transcribed_ones_at_every_coefficient` and `a_context_costs_one_state_word`; the Annex D table tests, the round trips, every committed decode and the `jpx` fingerprint unchanged |
| P-10.6 | The register MQ decoder | `mq.rs` `register_and_struct_decoders_agree_on_every_decision` and `a_renormalisation_shifts_once_per_byte_it_reads`; `annex_h2_test_sequence_reproduces_exactly`, `annex_h2_through_the_context_array` and every JBIG2 Annex H test unchanged; the `jbig2` and `jpx` fingerprints unmoved |
| P-10.7 | Row-blocked lifting | `jpx/tests/wavelet.rs` `row_blocked_synthesis_is_the_column_synthesis_at_every_shape` and `a_synthesis_copies_no_line_into_scratch`; `the_fixed_point_97_tracks_an_f64_reference_of_f382`, P-10.3's window tests, every committed decode and the `jpx` fingerprint unchanged |
| P-10.8 | The page's discard from the drawn size; the cache keyed on the request; the JPX counters in `Stats`; the `jpx_minified` fixture | `jpx_images.rs` `a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer` (A7); `jpx_minified` in `rendering_is_stable_across_targets` on Linux, Windows and `wasm32-wasip1`; `render_regions.rs` byte-equal over it; the `jpx` fingerprint and the display-list replay test unmoved |
| P-10.9 | The visible window with its margin; `DecodedImage`'s window; the cache keyed on it | `render_regions.rs` `a_region_of_a_jpx_page_is_the_crop_of_the_whole_page` at tile sizes that do not divide the page; `jpx_images.rs` `a_region_render_decodes_only_the_code_blocks_its_window_reaches`; the `jpx` and `jpx_minified` fingerprints and `a_display_list_replays_every_fingerprinted_page_byte_for_byte` unmoved |
| P-10.10 | Code-blocks on the pool, the budget charged in index order first; the filters to pool edge | `jpx/tests/parallel.rs` `code_blocks_on_2_3_4_and_7_threads_are_the_serial_decode` (every fixture, and a writer-made codestream that exhausts the budget part way: samples, warnings and refusals identical) and `a_decode_on_four_workers_shares_its_code_blocks`; `jpx_images.rs` pages under `Parallelism::Threads(n)` equal to serial for n = 2, 3, 4, 7 |
| P-10.11 | Synthesis on the pool | `jpx/tests/wavelet.rs` `synthesis_on_2_3_4_and_7_workers_is_the_serial_synthesis` and `a_synthesis_on_four_workers_shares_its_rows`; P-10.10's thread-count equality with synthesis on the pool; the `jpx` fingerprint on the four targets |
| P-10.12a | The four-thread 9/7 decode in MS-04's decode group | `the_committed_baseline_agrees_with_the_tree` with the operation in it |
| P-10.12b | Its figure, in the timing phase | the figure committed and `cargo xtask baseline` holding it |
| P-10.13 | The row's floors and MS-05's band | a local MS-02 paired run green with the floors; A7's JPX half in `cargo test` on Linux and Windows and under `wasm32-wasip1`; MS-05's peak-memory test holding the JPX page at 72 dpi within its band at or under 140 MB |

## Risks

| Risk | Mitigation |
| --- | --- |
| A speed mechanism moves a sample, which a 9/7 tolerance cannot see | `jpx_decodes.rs` pins every committed decode's CRC-32 at a full request before anything lands; a moved pin is a finding, never an edit |
| The plane format drifts — a wider plane, a narrower constant, a reordered product — and the overflow proof or the error analysis stops holding | Planes stay `i32` at Q0 and Q12, constants `i64` at Q24, products `i64` at Q36 with `mul_q24`'s rounding, in today's order; `the_fixed_point_97_tracks_an_f64_reference_of_f382` and `the_lifting_stays_inside_the_i64_product_bound` keep running |
| Packed tier-1 state agrees with a round trip that shares its mistake | The test encoder keeps the transcribed `Block`; the packed contexts are compared with the transcribed ones at every coefficient, and the Annex D tables entry by entry |
| The register MQ decoder counts decisions differently and moves JBIG2's budget | `take_decisions` is asserted equal to the struct decoder's over noise streams with markers and truncation; JBIG2's Annex H tests and fingerprint unmoved |
| A reduced or windowed decode is right only where it agrees with itself | Both are held to the full decode — of the reduced codestream, or cropped — and the full decode to the reference decodes and the annexes |
| A discard past some tile-component's levels | Clamped to the fewest levels in the codestream, reported in `JpxImage::discard`; the fuzz target draws discards past every level count |
| A window or a discard drops a code-block a kept sample depends on | The needed rectangles widen by the filter's reach at every level; `a_window_decode_is_the_crop_of_the_full_decode` runs random windows at every discard over multi-tile, multi-precinct codestreams of both filters |
| **A region render misses samples its pixels read, and a tile differs from the page (ruling 5)** | The window is widened by `(⌈footprint⌉ + 1) × 2^h` samples and its edges rounded out to `2^h`, the sampling policy decides on the whole image's size, and `a_region_of_a_jpx_page_is_the_crop_of_the_whole_page` holds tiles that do not divide the page byte-equal |
| **The image-run shape ruling 5 pins (H-15's default)** decides a tile's run from samples the tile's window would drop | A draw that can join a run is held to what its canvas reaches, or for an opaque image to what its clip shows, a window's nearest-sample stand-in covering every pixel; over its canvas the run holds what it holds of the whole image |
| A tile's window leaves out the damage a page's decode refuses, and the tile draws what the page refuses | Every kept code-block is accounted for, a tile-component with segmentation symbols decodes every block, every tile's structure is checked and the output charged whole: `a_window_refuses_whatever_the_whole_decode_refuses` |
| A minified JPX image now draws from its reduced decode, and its pixels move | That is the row's design: every JPX image drawn at half its size or less on both axes draws from the wavelet's low band rather than the box pyramid over the full decode. The `jpx` fingerprint's images are drawn magnified and do not move; `jpx_minified` is a new fingerprint; any committed render of a minified JPX image that moves is a finding for the owner before it is committed |
| A thread count changes which block a refusal names | The budget is charged for every listed block in index order before any decode, and the slots are read in index order, so the first refusal is the serial decode's |
| Tier-1 a tile at a time changes a refusal's order | Today tier-1 runs every tile before any reconstruction; a tier-1 refusal in a later tile then surfaces after an earlier tile's reconstruction work rather than before it, which is the same `Err` and the same warning, since the decode returns nothing but the refusal |
| The fuzz target's new knob bits hide the old inputs | The discard and the window take bits the knob byte leaves free, and `jpx_seeds.rs` restates the reading, so every committed seed still decodes or refuses by name |
| Memory peaks while every tile's coefficients are alive | Tier-1 runs a tile at a time and blocks drop their coefficients once placed; MS-05's band at or under 140 MB is held in P-10.13 |
