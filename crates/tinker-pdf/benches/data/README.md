# Benchmark inputs the tree cannot write

Three files the decode group and the JPEG 2000 page read. The tree writes
baseline JPEG and no progressive JPEG or JPEG 2000 at all, so each was made
once, outside the tree, from bytes the tree's own code writes; every other
input of the set is written in memory by `support/inputs.rs`
(`docs/design/performance.md`, "The inputs").

## The inputs

- `photo(2480, 3508, 0)` (`support/inputs.rs`) written as a binary PPM (`P6`,
  2480 × 3508, maxval 255): 26 099 537 bytes, SHA-256
  `2b39d6713c02629959ceb5df23054b5edaa06c45d132039d333a19621472d550`. The
  generator is seeded integer arithmetic, so every target writes these bytes.
  The two JPEG 2000 codestreams were coded from it.
- `baseline_jpeg()` (`support/inputs.rs`), that picture through the tree's
  baseline encoder at its default options: 275 538 bytes, SHA-256
  `754c262d187b8459117b897f5d79c97d71062c58ed6ff494b82438d8ed8dcbbc`. The
  progressive JPEG was transcoded from it, and `baseline_jpeg()` asserts this
  digest, so a change to the encoder fails there first rather than as a
  mismatch in the decode.

## The files

| File | Bytes | SHA-256 | How it was coded |
| --- | ---: | --- | --- |
| `progressive-2480x3508.jpg` | 143 710 | `bb6ae4408fc528fe81840d6ec926247964745271ac8b54b109d8ceb0d868cae6` | `baseline_jpeg()`'s coefficients re-scanned progressively by a lossless transcode (SOF2): its quantisation tables and 4:4:4 sampling, ten scans — the DC and AC first scans and their successive-approximation refinements — with Huffman tables optimised for them |
| `irreversible-2480x3508.jp2` | 1 087 461 | `0527a55b6caf36d2250016191996ea031493bfc2234c7b2d07a92e46d46745ca` | JP2 box container, the irreversible 9/7 wavelet and ICT, one tile, five decomposition levels, 64 × 64 code-blocks, one layer at a rate of one bit a pixel, LRCP |
| `reversible-2480x3508.jp2` | 11 379 732 | `d2381c135ff137d9bd191dc950a881a1bbe4b53f630588702349e194e476079b` | JP2 box container, the reversible 5/3 wavelet and RCT, lossless, one tile, five decomposition levels, 64 × 64 code-blocks, one layer, LRCP |

Two of the three are lossless, so `benches/decode.rs` holds their decodes to
their sources exactly: the progressive file's to `baseline_jpeg()`'s decode
sample for sample, the two differing in their scan schedule and nothing else,
and the reversible file's to `photo(2480, 3508, 0)`. The generator's output is
the expected output, and a change to either side fails there.
