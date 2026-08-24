# Filters and image codecs

Every stream filter and image codec the engine decodes is hand-rolled in
`crates/tinker-pdf-filters` — no C library, no codec dependency. The crate is
bytes in, bytes out, with no PDF types crossing its boundary (ruling 8,
[rulings](../rulings.md)), and it holds one output contract throughout:
**corrupt input truncates and warns; it never errors.** `Decoded::complete`
is false whenever input ended early, was damaged, or hit `Limits::max_output`,
and every tolerated condition leaves a typed `Warning` (ruling 10). Errors are
reserved for decisions, not data: `FilterError::BadParams` for parameters that
cannot describe any stream, `FilterError::Unsupported` for a capability this
build refuses by name.

## What it does

**Byte filters.** FlateDecode (7.4.4) is an own inflate — stored,
fixed-Huffman and dynamic-Huffman blocks (RFC 1951), the zlib wrapper
(RFC 1950) with Adler-32 verified and a raw-deflate fallback when the header
is absent. The encoder exists too: `deflate.rs` is a hash-chain LZ77 match
finder feeding fixed Huffman codes, with a stored block whenever that is
smaller — it is what compresses every stream the writer emits, object streams
included. LZWDecode (7.4.4) covers 9-to-12-bit variable codes, Clear/EOD and
both `/EarlyChange` values. ASCIIHexDecode (7.4.2), ASCII85Decode (7.4.3) and
RunLengthDecode (7.4.5) round out the set, and the Table 10 predictors
(7.4.4.4) implement `/Predictor 2` (TIFF horizontal) and the five PNG row
filters. `apply_chain` runs a `/Filter` chain in order; an image codec
terminates the chain, handing back its still-encoded payload tagged with
`ImageCodec`, and any filter named after one is dropped with
`Warning::ChainTailIgnored`. Every decoder runs under a mandatory output
ceiling (`Limits`), because a 1 KB flate stream can legally expand to
gigabytes (ruling 1).

**JPEG** (DCTDecode, 7.4.8; T.81): Huffman-coded baseline (SOF0), extended
sequential (SOF1) and progressive (SOF2) at 8 bits — spectral selection,
successive approximation and EOB runs (T.81 G.1.1.1.1) all decode. Every mode
fills the same per-component coefficient buffer and is rendered once by a
single dequantise-and-transform pass with an integer separable IDCT.
`JpegColor` names what came out: greyscale, YCbCr already converted to RGB,
and CMYK with the Adobe transform undone — including the inverted-CMYK
convention, reported as its own variant.

**CCITT** (CCITTFaxDecode, 7.4.6; T.4, T.6): G3 one-dimensional (`/K 0`), G3
mixed two-dimensional (`/K > 0`, each line announcing its mode with the tag
bit of T.4 4.2.1.3.1) and G4 (`/K < 0`). All the decode-relevant Table 11
parameters are honoured — `/K`, `/Columns`, `/Rows`, `/BlackIs1`,
`/EncodedByteAlign`, `/EndOfLine`, `/EndOfBlock` — with EOL, RTC and EOFB
recognised wherever they appear (T.4 4.1.2). A row that will not decode is
one row, not the rest of the page: the row above is replicated with a warning,
which is the standard fax recovery. Output is packed one bit per pixel,
MSB-first, rows byte-padded — exactly the shape `/BitsPerComponent 1`
describes, so `/ImageMask`, `/Decode` and `/ColorSpace` compose with a fax
the way they do with any other image.

**JBIG2** (JBIG2Decode, 7.4.7; T.88): the generic-region lineage. The MQ
arithmetic coder (T.88 Annex E) lives in its own module, `mq.rs`, shared with
the JPEG 2000 tier-1 coder — T.88 Annex E and T.800 Annex C are the same coder
— with `MqContexts::set_state` covering the one place the two callers differ
(T.88 E.3.6 against T.800 Table D.7). Around it: clause 7 segment headers, the
embedded organisation of D.3 with `/JBIG2Globals` read first, generic regions
under templates 0–3 with AT pixels and typical prediction (TPGDON, 6.2.5.7),
and MMR (6.2.6) through the same T.6 decoder a fax uses. Polarity is returned
in JBIG2's own sense (1 = black, 6.2.2); the inversion belongs at the PDF
boundary beside `/ImageMask` and `/Decode`. A file whose page composited no
region — the symbol-dictionary lineage, see below — is refused rather than
returned as a blank white page that reads as a successful decode of a blank
scan.

**JPEG 2000** (JPXDecode, 7.4.9; T.800): the JP2/JPX box container of Annex I
and bare J2K codestreams, Annex A marker segments with COC and QCC overriding
per component, Annex B tier-2 — tag trees, packet headers, precincts and all
five progression orders (B.12) — Annex D tier-1 on the shared MQ coder,
Annex E dequantisation, both Annex F inverse wavelets (the reversible 5/3 and
the irreversible 9/7 in fixed point) and the Annex G and I colour pipeline,
palettes and `cdef` included. An opacity channel is carried out separately in
`JpxOpacity` because what it is *for* is `/SMaskInData`'s rule (8.9.5.4) and
that decision stays outside the crate. The decoder's stance is that a wrong
JPEG 2000 decode looks like a photograph — the inverse wavelet smooths wrong
coefficients into a plausible image — so everything not implemented is refused
by name, and two integrity checks (packet lengths, the D.5 segmentation
symbol) catch a mis-parse before any pixel exists. Measured against the
corpus's nineteen JPX files as of August 2026: 14 decode, 4 refuse by name,
and 1 is never asked for (a form-resources non-goal recorded in
[rendering](rendering.md) and the [ROADMAP](../ROADMAP.md), not a codec
limit).

**Container codecs, not `/Filter` names.** No PDF stream is a PNG file, but
the archive formats need one, so the crate also exports a PNG decoder
(ISO/IEC 15948): all fifteen legal colour-type/bit-depth pairs, Adam7
interlace, `tRNS` in all three forms, and palettes applied with a bounds
check — an out-of-range index becomes a black pixel with a warning rather
than a refusal of every pixel that was fine (ruling 2). `png_scan` serves the
pass-through path: it walks chunks, checks every CRC and hands back the
concatenated IDAT without inflating it. Beside it sit `inflate_raw` (RFC 1951
with no wrapper, for ZIP entries) and `crc32` — the reflected-polynomial
CRC-32 that ZIP (APPNOTE 4.4.7) and PNG (5.3) both carry, with a resumable
`Crc32` for checksums over non-adjacent slices.

## API

The filters never appear on the facade — ruling 11 makes `tinker_pdf` the
only public surface, and every filter here runs behind `Document::open`,
`Page::render` and stream reading without being named. Inside the workspace
the crate boundary is the API:

```rust
use tinker_pdf_filters::{apply_chain, ChainOutput, Filter, FilterSpec, Limits};

let chain = [FilterSpec::new(Filter::AsciiHex), FilterSpec::new(Filter::Flate)];
match apply_chain(&bytes, &chain, &Limits::new(64 << 20))? {
    ChainOutput::Bytes(decoded) => { /* decoded.data, .complete, .warnings */ }
    ChainOutput::EncodedImage { kind, data, .. } => { /* hand to jpeg_decode etc. */ }
}
```

Single-filter entry points mirror the `/Filter` names: `flate_decode`,
`lzw_decode`, `ascii_hex_decode`, `ascii85_decode`, `run_length_decode`,
`predictor_decode`. The image codecs are `jpeg_decode` (returns `JpegImage`),
`ccitt_decode` (takes `CcittParams`), `jbig2_decode` (takes `Jbig2Params`,
which carries the `/JBIG2Globals` bytes) and `jpx_decode` (returns
`JpxImage`). The encoder half is `deflate` and `zlib_compress`; the container
half is `png_decode`, `png_scan`, `inflate_raw` and `crc32`.

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| JBIG2 symbol dictionary + text region (T.88 6.4, 6.5) | `FilterError::Unsupported(Capability::Jbig2)`, reason in `Warning::Jbig2SegmentSkipped` | The common OCR-pipeline output; a page with no composited region draws the placeholder rather than a blank page reported as success | [ROADMAP](../ROADMAP.md) |
| JBIG2 region or page above the output ceiling | `Warning::Jbig2RegionTooLarge` | Width and height are attacker-controlled 32-bit values; refused before allocation (ruling 1) | [rulings](../rulings.md) |
| JPX markers RGN, POC, PPM, PPT, CRG (T.800 Table A.2) | `Warning::JpxMarkerUnsupported` | Never skipped: a skipped RGN draws a bright rectangle and a skipped POC mis-parses every packet after it | [ROADMAP](../ROADMAP.md) |
| JPX markers Table A.2 does not define (all of ISO/IEC 15444-2) | `Warning::JpxMarkerUnknown` | Part 2 is a non-goal; an unknown marker cannot be measured past | [ROADMAP](../ROADMAP.md) |
| JPX coding features: five of Table A.19's six code-block styles, unmappable `colr`, unequal channel depths | `Warning::JpxFeatureUnsupported` | A wrong JPEG 2000 decode is a plausible photograph; refusal beats a blur nobody can distinguish from a bad scan | [ROADMAP](../ROADMAP.md) |
| JPX component precision above 16 bits | `Warning::JpxPrecisionUnsupported` | T.800 allows 38 bits; the sample path carries 16, so this is refused rather than truncated | [ROADMAP](../ROADMAP.md) |
| JPX tile-parts out of order | `Warning::JpxStructureInvalid` | Decoding them would need buffering the whole codestream speculatively | [ROADMAP](../ROADMAP.md) |
| JPX work/sample/code-block budgets spent | `Warning::JpxBudgetSpent` | The budgets are totals, never refunded — a per-item cap is not a work cap once the structure branches (ruling 1) | [rulings](../rulings.md) |
| JPEG arithmetic coding | `JpegError::Arithmetic` (gate `Capability::JpegArithmetic`) | Reported rather than half-decoded | [ROADMAP](../ROADMAP.md) |
| JPEG precision other than 8 bits | `JpegError::UnsupportedPrecision` (gate `Capability::Jpeg12Bit`) | Same contract: named, not guessed at | [ROADMAP](../ROADMAP.md) |

Every JPX refusal also returns `FilterError::Unsupported(Capability::Jpx)`;
the warning names the reason, because the refusal is an `Err` and ruling 10
wants the reason to survive it.

## Verified

- `crates/tinker-pdf-filters/tests/vectors.rs` — byte-vector fixtures for
  every wave-1 decoder, each with recorded provenance: genuine zlib output,
  a libtiff-produced LZW strip, the `-----A---B` example of 7.4.4.2 verbatim.
- `crates/tinker-pdf-filters/tests/props.rs` — proptest: no panic on
  arbitrary bytes, and no decoder ever exceeds `Limits::max_output`.
- `crates/tinker-pdf-filters/tests/containers.rs` — `inflate_raw` and CRC-32
  as their container consumers use them.
- `crates/tinker-pdf-filters/tests/jpx_reference.rs` — the JPX decoder
  against committed reference decodes: codestreams made with `opj_compress`
  from this repository's own images, lossy references decoded once by
  `opj_decompress` (OpenJPEG 2.5.0, August 2026), byte-identity pinning
  container, tier-2, tier-1 context numbering, dequantisation, the 5/3 and
  the DC level shift in one comparison. **It invokes nothing** — it was named
  and described as an oracle and never was one, and the CI job that grepped
  it for a banner it does not print could not pass. Corrected with ruling 13,
  which keeps the committed decodes as a dated measurement and rules out ever
  regenerating them as a check.
- `crates/tinker-pdf-filters/tests/png_suite.rs` — the PNG decoder against
  PngSuite, 176 files as of August 2026: all fifteen legal colour-type/depth
  pairs, and fourteen broken-by-design files that must each be refused. Runs
  when `TINKER_PNGSUITE` points at the set, and prints `RAN`/`SKIPPED` so a
  missing corpus never reads as a pass.
- In-crate: `jbig2.rs` decodes T.88 Annex H.1's published datastream example
  byte for byte; `mq.rs` holds Annex H.2's test sequence as a permanent
  fixture, because the coder serves two codecs; `src/jpx/tests/refusals.rs`
  reaches every entry of the JPX refusal list, so "the refusals are the
  feature" is checked, not claimed.
- Fuzzing: eight of the 24 fuzz targets exercise this crate —
  `ascii_filters`, `lzw`, `inflate`, `ccitt`, `jpeg`, `jbig2`, `jpx`, `png`.
- Downstream: the `image`, `jbig2` and `jpx` render fingerprints among the
  15 in `crates/tinker-pdf/tests/determinism.rs` pin decoded pixels
  bit-for-bit across targets ([determinism](determinism.md)), and the whole
  workspace stands at 2 951 passed / 0 failed / 8 ignored
  (Windows x86_64, August 2026). See [verification](../verification.md) for
  the full harness.
