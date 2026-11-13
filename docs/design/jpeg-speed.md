# JPEG decoding speed

*For [ROADMAP](../ROADMAP.md) row P-09.* The decoder in
`crates/tinker-pdf-filters/src/jpeg.rs` reads a Huffman code one bit at a
time, comparing the code so far against each length's largest code; decodes
every frame, baseline included, into one coefficient buffer covering the
whole image; and renders it once at the end, in `finish`, which transforms
every block by a direct separable IDCT summed in `i64`, writes one
full-resolution plane per component with chroma replicated into it, and
interleaves the planes into the output, converting colour as it goes. The
facade's `jpeg_image` (`crates/tinker-pdf/src/resources.rs`) then copies that
output into the RGB a page draws (`jpeg_to_rgb`). This section fixes the
invariant every change keeps, the look-ahead Huffman reader, the transform over
32-bit lanes, strips, which frames stream, the decode straight into the image,
the reduced transforms and their geometry, the drawn-size request and the scale
rule, guard A7's JPEG half, and the memory plan for 1.3 times the output.

## Scope

- **The invariant** every full-size decode keeps, and how each step shows it.
- **The look-ahead Huffman reader** (P-09.2): tables answering a symbol in one
  probe, or two for a long code, over a word-sized bit buffer that meets
  stuffing, restart markers, markers and the end where today's reader does.
- **The lane transform** (P-09.3): today's transform, exactly, with its shared
  products made once and its sums in `i32` within a stated bound.
- **Strips** (P-09.4): one MCU row transformed, replicated and converted at a
  time, handed to a sink or painted into the output.
- **Which frames stream** (P-09.5): the frames entropy-decoded one MCU row of
  coefficients at a time.
- **Decoding into the image** (P-09.6): the facade's JPEG path writing strips
  into its image's RGB.
- **The reduced transforms** (P-09.7): 1/2, 1/4 and 1/8 in the DCT domain,
  their definitions and their output geometry.
- **The drawn-size request** (P-09.8): what a page asks for, and the scale it
  is decoded at.
- **Guard A7's JPEG half** and the two counters it and P-09.6 read.
- **The memory plan** for MS-05's 1.3 times the output, and what a progressive
  frame's coefficients cost beside it.

## Non-goals

- **The lossless and differential frames.** SOF3, SOF7, SOF11 and SOF15 stay
  refused as `JpegError::Lossless`, and SOF5, SOF6, SOF13 and SOF14 as
  `JpegError::Differential`.
- **A 16-bit sample path.** A twelve-bit frame is narrowed to eight bits and
  says so (`Warning::JpegPrecisionNarrowed`), at every scale.
- **ICC and CMYK conversion throughput**, P-22's: a CMYK image is converted as
  it is today, a strip at a time.
- **`unsafe` and SIMD intrinsics.** H-16's default: safe code shaped for the
  autovectoriser, fixed-width loops over integer arrays.
- **The arithmetic coder.** SOF9 and SOF10 share everything here but the
  Huffman reader; `crate::qm` and `jpeg/arith.rs` are unchanged.
- **Threads**, P-15's. One decode is one thread's.
- **The extraction and writing paths.** `Page::images`, image recoding and
  `to_svg` keep `jpeg_decode`'s full-size samples.

## Design

### The invariant: a full-size decode is byte-identical for every stream

A decode at full size gives, for every input, the bytes it gives today: the
same `JpegImage`, or the same `JpegError` — damaged, truncated and
restart-marked streams, odd geometry and the fuzz corpus included, since a
document's JPEG is whatever its producer wrote. Each step that touches the
full-size path keeps the code it replaces in the test build and holds the new
code to it over generated streams that reach every path: the bitwise reader
(P-09.2), the direct transform (P-09.3) and the whole-image `finish` (P-09.4).
The streams are headers this crate writes around random entropy-coded data —
every frame type and both coders, 8 and 12 bits, 4:4:4, 4:2:2, 4:2:0, 4:1:1
and ratios that do not divide, Gray, YCbCr, CMYK, YCCK and inverted CMYK,
restart intervals, and cuts at every length — beside the encoder's own output
and the committed fuzz seeds. Random entropy data decodes, leniently, to
coefficients of every magnitude, which is what reaches a transform's far
ranges.

Only the full-size decode has a past to keep. A reduced decode (P-09.7) is new
output, held to its own definition.

### The look-ahead Huffman reader (P-09.2)

**The word.** A `u64` of buffered bits, the next bit its top one, and a count.
The refill runs while the word holds 56 bits or fewer and takes bytes from the
entropy-coded data: a byte other than `X'FF'` goes in; `X'FF00'` goes in as
`X'FF'`, its stuffed zero consumed (T.81 F.1.2.3); anything else — `X'FF'`
followed by a marker code, by `X'FF'` or by nothing, or the end of the data —
stops the refill without consuming it. So the word only ever holds data bytes.

**Every event is met where today's reader meets it.** Today's `bit` reads a
byte only when it has no bits left, and it is there that it steps over a
restart marker and records `crossed_restart`, or meets a marker or the end and
records `exhausted` and returns one zero, its position past the `X'FF'`. The
new reader keeps that code as its slow path, and reaches it only when the word
is empty and a bit is asked for — the moment today's reader, having read every
byte before the event, asks too. The bytes after a marker are then read as
today's reader reads them, as data, and past the end every bit is zero.

**The tables.** Each DC and AC table keeps today's per-length maximum codes and
value offsets, and gains two levels of lookup. The first is indexed by the next
nine bits: 512 entries, each a code length and a symbol, a code length and no
symbol, or a pointer to the second level. The second has 128 entries, indexed by
the seven bits after those, for each first-level prefix that begins a longer
code. Every entry is built by running today's search — each length in turn,
`code <= max_code[length]` — over the bits it is indexed by, so a table answers
as today's search does whatever it is: complete, incomplete, oversubscribed or
never defined. That includes the bits a failure reads: a matched code whose
value the table does not hold reads its length, and a prefix no code of any
length begins with reads sixteen and answers nothing, with no second level of
its own.

**When the tables answer.** When the word holds at least sixteen bits and
`exhausted` is clear, every bit today's search could read is a data bit in the
word, so the probe's answer is the search's: one probe, or two for a code
longer than nine bits. With nine to fifteen bits a first-level entry whose
length fits still answers. Otherwise — near a marker or the end, or once
`exhausted` is set, where today's search stops at the first length that fails
— the bit-at-a-time search runs. Extra bits come from the word when it holds
them, and bit by bit when it does not.

**Restarts.** Today's reader holds at most the rest of one byte at a restart;
the word may hold whole bytes beyond it that today's reader never read. They go
back: the position steps back over each whole byte the word holds, two bytes
for an `X'FF'`, and today's search for the next RSTn runs from there. The
position is restored exactly rather than approximately because that search
stops one byte short of the end, so where it starts decides what the next bit
reads in a stream that ends without its marker.

**The arithmetic frames** keep `crate::qm`.

### The lane transform (P-09.3)

**Today's transform.** `COS_TABLE[x][u] = round(2^14 · C(u)/2 ·
cos((2x+1)uπ/16))`, `C(0) = 1/√2`, `C(u) = 1` otherwise. A row pass sums
`in[u] · COS_TABLE[x][u]` in `i64`, skipping zero coefficients, floors by
`>> 14` and truncates to `i32`; the column pass sums the same way over the row
results, floors by `>> 14`, adds A.3.1's level shift `2^(P−1)`, clamps to the
frame's range and narrows to eight bits.

**The symmetry the table has.** Every entry satisfies
`COS_TABLE[7−x][u] = (−1)^u · COS_TABLE[x][u]`, and within the even half
`COS_TABLE[x][0] = 5 793` for every `x`, `COS_TABLE[x][4] = ±5 793`, and
`COS_TABLE[x][2]` and `COS_TABLE[x][6]` are each `±7 568` or `±3 135`. Every
entry lies at least 0.079 from a rounding boundary, so every target's `cos`
rounds it to the same integer. So outputs `x` and `7−x` of an eight-point pass
are `E(x) + O(x)` and `E(x) − O(x)`, `E` summing the even coefficients' products
and `O` the odd ones', and the products are made once: sixteen odd products,
`in[u] · COS_TABLE[x][u]` for the four odd `u` and `x` below four, and six even
ones — `in[0]` and `in[4]` by 5 793, `in[2]` and `in[6]` each by 7 568 and by
3 135. Twenty-two multiplies an eight-point pass, where today's makes up to
sixty-four. Integer sums are the same in any grouping, so every pass is
today's to the bit; no product of different entries is folded into one
constant, which is the rearrangement that would move pixels no fingerprint sees
(the one fingerprinted JPEG is DC-only).

**The `i32` bound.** Every row of the table has the same absolute sum,
43 284. A block whose dequantised coefficients are all at most 18 779 in
magnitude keeps every sum of both passes inside `i32`: a row's sum is at most
18 779 × 43 284 = 812 830 236, its floored result at most 49 612, and a
column's sum at most 49 612 × 43 284 = 2 147 405 808, under 2^31. Dequantising a
block finds its largest magnitude; a block within the bound runs in `i32`, and
one past it — only a damaged stream or a twelve-bit frame with a coarse
quantiser reaches it — runs the same shape in `i64`, its row results truncated
to `i32` as today's are.

**Shortcuts, each exact.** A block with no AC term is one constant, today's two
products and two floors; a row with no AC term is that row's constant; a row
whose upper half (`u` at least 4) is zero skips the products it would multiply
by zero, eleven where a full row makes twenty-two; in the column pass, a block
whose rows 1 to 7 are zero makes each column a constant, and one whose rows 4 to
7 are zero takes the half shape. The column pass runs the eight columns as
lanes of `i32`, the row pass a row at a time, and nothing branches on a
coefficient. A dense block costs 352 multiplies, against 1 024.

### Strips (P-09.4)

**A strip is one MCU row**: `8 · Vmax` image rows, the last cut at the image's
height. For each strip, each component's block rows that the strip's image
rows read are dequantised and transformed into that component's plane, and
each sample is replicated across the image's width as it is written — the
plane is that many component rows of `width` bytes. Image row `y` reads plane
row `y / (Vmax / V)` of each component, and is converted into the strip with
today's `f32` expressions in today's order (T.871's YCbCr, YCCK's three and its
`K`, Adobe's inversion), so every byte is today's. A pixel no block covers —
possible only where a ratio does not divide — keeps the 128 today's plane was
filled with.

**Ratios that divide** (`Hmax / H` and `Vmax / V` integers, which every encoder
writes) make a strip's rows exactly its MCU row's block rows. **Ratios that do
not** (an `H` of 3 beside an `H` of 2) make a strip read block rows that
straddle MCU rows, which the strip transforms from the whole-image coefficients;
such a frame never streams.

**The sink.** `JpegSink` has `begin`, called once with the image's width,
height, colour and precision, and `strip`, called with whole rows of
interleaved samples — `JpegImage::data`'s rows — top to bottom. Every check
`finish` makes is made before the first strip, so **a decode that hands out a
strip succeeds**. The strip decode returns the frame's geometry, its warnings
and what it held (`JpegHeld`: coefficient bytes, plane bytes and the strip
buffer's bytes). `jpeg_decode` runs the same strips and paints them straight
into its `JpegImage`'s buffer, which is the whole of what it holds beside the
coefficients and one MCU row of planes. `tiff.rs`'s JPEG path keeps
`jpeg_decode`.

**One MCU row of planes** is at most `8 · Vmax · width · components` bytes: the
bytes one MCU row of the output holds.

### Which frames stream (P-09.5)

At the frame's first scan, a frame streams when all of these hold:

- it is sequential — SOF0, SOF1 or SOF9;
- the scan names every component of the frame, once each;
- every sampling ratio divides;
- the frame is one `finish` accepts: a width and a height, one, three or four
  components, and an output within the ceiling;
- nothing after the scan changes what the end of the decode reads: the marker
  walk the decode makes after the scan, run ahead of it from where the decode
  would resume, meets no frame header, no further scan, no `DQT` and no Adobe
  `APP14` before EOI or the end. A later `DQT` would change the dequantisation
  and a later Adobe marker the colour, both of which today's decode reads only
  at its end, and a later scan or frame anything at all.

A streaming frame's coefficients are one MCU row of each component, `V` block
rows of it. Entropy decoding writes into them; at the end of each MCU row the
row is painted into its strip and cleared. A scan that stops early leaves its
current row painted as it stands and every row after it painted from zeros,
which is what the whole-image buffer held for them. Restart intervals, the DC
predictions and the arithmetic statistics run across MCU rows exactly as they
do today.

Every other frame keeps whole-image coefficients: a progressive frame, whose
later scans refine them; a sequential frame in several scans; a frame whose
end reads a marker after its scan. This reverses the header's choice that
baseline shares the buffered path against drift; the drift guard is P-09.4's
equality run over every sequential stream with the buffered path forced on,
kept permanent.

### Decoding into the image (P-09.6)

`jpeg_image` takes the strip decode, and its sink writes each strip into the
`DecodedImage`'s RGB: a grey replicated to three, RGB copied, CMYK converted
through `DeviceCmyk` as `jpeg_to_rgb` converts it, and `/Decode`'s reversal
applied to the strip's bytes; an image drawn in its dictionary's space converts
each strip through one converter whose remembered colours last the whole image,
so each pixel's colour is the one `samples_in_space` gives. No `JpegImage` and
no second RGB copy exist beside the image, and `jpeg_to_rgb` goes. The inline
path takes the same route; `Page::images` and recoding keep `jpeg_decode`.

`Stats` gains **`jpeg_held`**, the most bytes a JPEG decode held at once beside
the image it decoded into — `JpegHeld`'s total, every buffer sized from the
image: coefficients, planes and the strip. A maximum: `work()` leaves it out
and `+` keeps the larger. The facade adds it from the value the decode returns,
as it adds the PNG writer's (ruling 8: the leaf has no tally).

### The reduced transforms (P-09.7)

**Definition.** At scale `N/8`, `N` one of 4, 2 and 1, a block's top-left `N×N`
dequantised coefficients `S(u, v)` give `N×N` samples

```text
s_N(x, y) = 1/4 · Σ_u Σ_v C(u) C(v) S(u, v) cos((2x+1)uπ/2N) cos((2y+1)vπ/2N) + 2^(P−1)
```

over `u` and `v` below `N`, clamped to the frame's range and narrowed to eight
bits as a full-size sample is. `s_N(x, y)` is the block's band-limited reconstruction at the centre of the
cell of `8/N` by `8/N` full-size samples that output sample covers; at `N = 1`
it is the block's DC level.

**Arithmetic.** `K_N[x][u] = round(2^16 · C(u)/2 · cos((2x+1)uπ/2N))`. Each
kept coefficient is clamped to `±2^20`; the row pass sums `S · K_N` exactly in
`i64`, the column pass sums `K_N` times those exactly, and the one rounding is
the last: add `2^31`, floor by `>> 32`, then the level shift, the clamp and the
narrowing. The sums stay inside `i64` (four terms of `2^15 · 2^37`). **A block
whose kept coefficients have no AC term — and, at 1/8, every block — takes the
full transform's DC constant**, so a flat block is the same level at every
scale, exactly. Against the definition evaluated in `f64` and rounded, a
reduced sample is within one level: the rounded table moves a sample by at most
`2^-17` of the kept coefficients' summed magnitude and the last rounding by
half a level, which stays under one level for every block an eight-bit frame's
samples produce (sixteen coefficients of at most 1 152 once dequantised, 18 432
in all) and under one level after a twelve-bit frame's narrowing; the DC
constant is within one level of `S(0, 0)/8` by its two floors.

**Geometry.** A `W × H` image at `N/8` is `⌈W·N/8⌉ × ⌈H·N/8⌉` samples, and its
sample `X` covers full-size columns `[8X/N, 8(X+1)/N)`, the last cut at `W`.
Each component's block grid is reduced by the same factor before its
replication, which is unchanged, so a strip is `N · Vmax` rows. Every frame
type, both coders and both precisions decode at every scale. The `jpeg` fuzz
target decodes each input at the scale its length selects — the length modulo
four, 0 for full size, then 1/2, 1/4 and 1/8 — so every seed reaches a scale
and a mutation that adds or drops a byte reaches another;
`the_jpeg_seeds_reach_the_decoder` restates that call.

### The drawn-size request (P-09.8)

**What a page asks.** `GlyphSource::image` and `inline_image` take a request
carrying the drawn size: the device pixels the image's unit square spans along
each of the image's own axes, `√(a² + b²)` and `√(c² + d²)` of its transform in
the page frame (`sqrt` is correctly rounded). A tile computes it from that same
transform, so a tile and the page ask alike (ruling 5); a display list's replay
computes it from the transform it replays at.

**The scale rule.** The decode takes the smallest `N/8` of 1/8, 1/4 and 1/2
for which `W·N/8` is at least the drawn width and `H·N/8` at least the drawn
height — so the reduced grid is never drawn magnified — and full size when none
is. A drawn size that is not finite asks for full size. An image with a soft
mask, a mask or a colour key decodes at full size, so the mask is sampled
against the grid it was drawn for. The image cache keys on the scale, so a hit
returns exactly what that scale's miss computes. `to_svg`, `Page::images` and
recoding ask for full size.

**Placing the reduced grid.** The decoded image carries the full size and the
scale, and the draw maps the unit square onto `W·N/8` by `H·N/8` samples, so the
last column and row are cut where the full image ends rather than stretched to
the square's edge; P-08's resampler draws it.

### Guard A7's JPEG half, and the counters

- **`Stats::jpeg_held`** (P-09.6), above.
- **`Stats::jpeg_samples_transformed`** (P-09.8): samples the inverse
  transforms output, every component's, a quantity `work()` leaves out. The
  strip decode returns it with its geometry (`JpegFrame::transformed`), which
  P-09.7's unit test reads.
- **A7**, `a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel` in
  `crates/tinker-pdf/tests/images.rs`: a JPEG drawn at a quarter of its size
  each way transforms at most four samples a drawn pixel. At 1/4 a 4:2:0 image
  transforms a sample and a half a drawn pixel, and its full decode twenty-four.
  The roster entry in `guards.rs` flips to landed, and the test joins the
  `wasm32-wasip1` run's command in `xtask/src/determinism.rs`, in P-09.8's
  commit.

### The memory plan for 1.3 times the output

**The case MS-05's band holds** is the 2480 × 3508 baseline stream — the tree's
encoder at its defaults: 4:4:4 RGB, one interleaved scan — decoded by
`jpeg_decode`. It streams, and its strips are painted into the output, so the
decode holds the output (26 099 520 bytes), one MCU row of coefficients (three
components of 310 blocks at 128 bytes, 119 040 bytes), one MCU row of planes
(3 × 8 × 2 480 = 59 520 bytes), its input and its tables. P-09.6's 0.3 is the
band less the output, held through the facade for a 4:2:0 image.

**A progressive frame's coefficients** are two bytes a coefficient over the
whole image: at 4:4:4 six bytes a pixel, twice the RGB output, and at 4:2:0
three, once — so a progressive decode peaks near three or two times its output,
and the 1.3 band belongs to the baseline case alone.

## Milestones

1. **P-09.2, the look-ahead reader.** Exit: `jpeg.rs`
   `the_lookahead_reader_decodes_what_the_bitwise_reader_did` (random complete,
   incomplete and oversubscribed tables with codes to 16 bits; streams with
   stuffed `X'FF00'`, RSTn mid-symbol and on a byte boundary, a marker inside a
   code, fill bytes and truncation; every symbol, extra-bits value and end state
   equal to the bitwise reader's, kept test-only) and
   `a_symbol_costs_at_most_two_table_probes`, counted; every JPEG test,
   `the_jpeg_seeds_reach_the_decoder` and TIFF's JPEG tests unchanged.
2. **P-09.3, the lane transform.** Exit:
   `the_lane_transform_is_the_direct_transform_on_every_block` (random blocks
   at 8 and 12 bits, sparse and dense, at and past the bound, equal to
   `idct_block`, kept test-only) and `a_block_costs_the_shared_products`
   (multiplies a block, counted); `the_cosine_table_matches_its_formula`,
   `a_flat_block_inverts_to_a_flat_image`, the round trips and the `cbz`
   fingerprint unchanged.
3. **P-09.4, strips.** Exit: `a_strip_decode_is_the_whole_image_decode` (every
   frame type and coder, both precisions, every sampling above, odd sizes,
   every colour model, restarts and truncation: the same rows through the sink,
   through `jpeg_decode` and through today's `finish`, kept test-only) and
   `a_decode_holds_no_whole_image_plane` (plane bytes at most one MCU row's).
4. **P-09.5, streaming.** Exit:
   `a_single_scan_sequential_frame_holds_one_mcu_row_of_coefficients`, Huffman
   and arithmetic; P-09.4's equality over every sequential stream with the
   buffered path forced, `a_restart_interval_resets_the_coder_and_the_statistics`,
   `a_truncated_arithmetic_scan_warns_rather_than_failing` and
   `a_truncated_progressive_file_keeps_what_it_decoded` unchanged.
5. **P-09.6, into the image.** Exit: `images.rs`
   `a_jpeg_decodes_into_its_image_without_a_second_copy` (`jpeg_held` at most
   0.3 times the RGB for a single-scan sequential 4:2:0 image);
   `a_decode_array_inverts_a_jpeg_too`,
   `a_progressive_jpeg_is_decoded_rather_than_placeheld`, the inline JPEG tests,
   `page_images.rs` and the `cbz` fingerprint unchanged.
6. **P-09.7, the reduced transforms.** Exit:
   `a_flat_block_is_the_same_level_at_every_scale` and
   `an_eighth_scale_decode_is_each_blocks_dc_level`, exactly;
   `the_reduced_transforms_match_their_definition`, within one level;
   `a_quarter_scale_decode_transforms_a_sixteenth_of_the_samples`, counted; at
   full size every JPEG test and P-09.4's equality unchanged.
7. **P-09.8, the request.** Exit: A7's
   `a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel`; a
   `jpeg_minified` fixture — odd-sized baseline and progressive 4:2:0 JPEGs at
   1/2, 1/4 and 1/8, one flipped, one quarter-turned, one clipped — in
   `rendering_is_stable_across_targets` on the four targets and in
   `render_regions.rs`; every existing fingerprint and golden unchanged.
8. **P-09.9a, the speed row.** Exit: `the_committed_baseline_agrees_with_the_tree`
   with 'JPEG decode at quarter scale' in MS-04's decode group, paired with the
   planning baseline's full-size decode of the same stream; its first paired
   ratio taken by a local MS-02 paired run, recorded where the row's record
   goes.
9. **P-09.9b, its figure**, in the timing phase: committed, and
   `cargo xtask baseline` holds it.
10. **P-09.11, the close.** Exit: a local MS-02 paired run green with the
    row's five floors; MS-05's peak-memory test holding the 2480 × 3508
    baseline decode at or under 1.3 times its output; A7 and P-09's counted
    guards in `cargo test` on Linux and Windows and under `wasm32-wasip1`.

## Risks

| Risk | Mitigation |
| --- | --- |
| The word reads past a marker or a restart and changes when `exhausted` or `crossed_restart` is set | The refill takes data bytes only; every event is the slow path's, at an empty word; the equality test compares both readers' state after every operation |
| A restart starts its search from the wrong byte | The word's whole bytes are given back, stuffing counted, before today's search runs |
| A table answers differently from today's search on a table no encoder writes | Entries are built by running today's search; the test draws incomplete, oversubscribed and undefined tables |
| The table's symmetry fails on some target, or a regrouping changes a sum | Every entry is 0.079 or more from a rounding boundary; the transform is held to `idct_block` on random blocks; no constant combines two entries |
| An `i32` sum overflows | The bound is derived from the table, and blocks at and past it are in the test |
| Colour conversion moves a byte | Today's `f32` expressions in today's order; the strip equality runs every colour model |
| Streaming changes a stream's output | The tail walk keeps whole-image coefficients for any frame whose end reads a later marker; the forced-buffered equality is permanent |
| A strip is handed out and the decode then fails | Every check `finish` makes runs before the first strip, and a streaming frame is chosen only when nothing after its scan can fail it |
| A reduced decode moves a pinned output | As the tree stands, no fingerprint or golden draws a JPEG at half its size or less — `cbz`'s 16 × 16 JPEG is drawn one sample a pixel — and no test that asserts a pixel draws one: `images.rs`, `inline_images.rs` and `default_spaces.rs` magnify theirs, the comic, standalone and `cbz` fixtures draw a page's image one sample a pixel at 72 dpi (`cbz.rs` magnifies it at 144), and the XPS and EPUB fixtures draw theirs at three quarters of a sample a pixel or larger. What draws one minified is `hostile_input.rs`'s sweeps at scales 0.05 and 0.25, which assert no pixel, and the render bench's half-size JPEG page at 72 dpi, a quarter. A pin a later fixture moves is the owner's to review |
| A reduced decode moves the corpus | Every JPEG a real page draws at half its size or less draws from a reduced decode after P-09.8, so the corpus `dpi` relation, MS-18's resolution check, is where it shows; a fall in a bar's count is H-78's |
| A reduced sample sits a level from the full decode box-filtered | The full transform floors twice and a reduced one rounds; a flat block is equal at every scale by construction, and the corpus relation above is where any other difference shows |
| A tile and the page pick different scales | The request is computed from the page frame's transform; `render_regions.rs` holds `jpeg_minified` tile by tile |
| A cache hit returns another scale's decode | The cache keys on the scale |
| A masked image is sampled against a reduced grid | An image with a soft mask, a mask or a colour key decodes at full size |
| The peak band's figure goes stale | `tests/peak_memory.tsv`'s `jpeg-300` line holds an upper bound the strip decode undercuts; P-09.11 adds the 2480 × 3508 decode as its own case at its measured peak |
