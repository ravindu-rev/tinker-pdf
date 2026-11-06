# Benchmark inputs the tree cannot write

Four files the decode group, the render group's JPEG 2000 page and
`tests/peak_memory.rs`'s read. The tree writes baseline JPEG and no
progressive JPEG or JPEG 2000 at all, so each was made once, outside the
tree, from bytes the tree's own code writes; every other input of the set is
written in memory by `support/inputs.rs` (`docs/design/performance.md`, "The
inputs"). The three a benchmark reads are the size that brings its
operations inside the set's 20-300 ms window on the hosted runner; the
full-page codestream is the JPEG 2000 page whose peak `peak_memory.rs`
holds.

## The inputs

- `photo(2480, 3508, 0)` (`support/inputs.rs`) written as a binary PPM (`P6`,
  2480 × 3508, maxval 255): 26 099 537 bytes, SHA-256
  `2b39d6713c02629959ceb5df23054b5edaa06c45d132039d333a19621472d550`. The
  full-page codestream was coded from it.
- `photo(620, 877, 0)` and `photo(310, 438, 0)` (`support/inputs.rs`), each
  written as a binary PPM (`P6`, maxval 255): 1 631 235 bytes, SHA-256
  `c93e6acce74c68fc2ebe0c38002d7689605da410c144205abc20c5a272dc01ae`, and
  407 355 bytes, SHA-256
  `0659b3fe02c66739567cb94c9cc92ed4f52396bbe324dfd608ba9319cd4d8883`. The
  generator is seeded integer arithmetic, so every target writes these bytes.
  The irreversible JPEG 2000 codestream was coded from the first and the
  reversible one from the second.
- `baseline_jpeg(2)` (`support/inputs.rs`), `photo(1240, 1754, 0)` through the
  tree's baseline encoder at its default options: 78 738 bytes, SHA-256
  `2dd7f148015119516d79e4ea8b97096f99d20fd3a848e361abc895248ee2475f`. The
  progressive JPEG was transcoded from it, and `baseline_jpeg(2)` asserts this
  digest, so a change to the encoder fails there first rather than as a
  mismatch in the decode.

## The files

| File | Bytes | SHA-256 | How it was coded |
| --- | ---: | --- | --- |
| `progressive-1240x1754.jpg` | 46 117 | `aa279e93ae2ff788e5684a033fe20cd2a5be187944adfb31b6338ab17cfff9b1` | `baseline_jpeg(2)`'s coefficients re-scanned progressively by a lossless transcode (SOF2, libjpeg-turbo 2.1.5's `jpegtran -progressive -optimize -copy none`): its quantisation tables and 4:4:4 sampling, ten scans — the DC and AC first scans and their successive-approximation refinements — with Huffman tables optimised for them |
| `irreversible-2480x3508.jp2` | 1 087 461 | `0527a55b6caf36d2250016191996ea031493bfc2234c7b2d07a92e46d46745ca` | JP2 box container, the irreversible 9/7 wavelet and ICT, one tile, five decomposition levels, 64 × 64 code-blocks, one layer at a rate of one bit a pixel, LRCP |
| `irreversible-620x877.jp2` | 67 628 | `4f2a9bf5bbbcaeeac2f28853411b46c7ade09d82a2c6625f5fe173327a77f3ff` | OpenJPEG 2.5.0's `opj_compress -I -n 6 -b 64,64 -r 24 -p LRCP -mct 1`: JP2 box container, the irreversible 9/7 wavelet and ICT, one tile, five decomposition levels, 64 × 64 code-blocks, one layer at a rate of one bit a pixel, LRCP |
| `reversible-310x438.jp2` | 191 380 | `3e562780e1ff2ade78980ad6046b69987da2921652bc3a25febd2a9475fbd85f` | OpenJPEG 2.5.0's `opj_compress -n 6 -b 64,64 -p LRCP -mct 1`: JP2 box container, the reversible 5/3 wavelet and RCT, lossless, one tile, five decomposition levels, 64 × 64 code-blocks, one layer, LRCP |

Two of the four are lossless, so `benches/decode.rs` holds their decodes to
their sources exactly: the progressive file's to `baseline_jpeg(2)`'s decode
sample for sample, the two differing in their scan schedule and nothing else,
and the reversible file's to `photo(310, 438, 0)`. The generator's output is
the expected output, and a change to either side fails there.
