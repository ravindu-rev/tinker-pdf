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

**JBIG2** (JBIG2Decode, 7.4.7; T.88): the generic-region and symbol lineages.
The MQ arithmetic coder (T.88 Annex E) lives in its own module, `mq.rs`, shared
with the JPEG 2000 tier-1 coder — T.88 Annex E and T.800 Annex C are the same
coder — with `MqContexts::set_state` covering the one place the two callers
differ (T.88 E.3.6 against T.800 Table D.7). Around it: clause 7 segment
headers with their referred-to lists, the embedded organisation of D.3 with
`/JBIG2Globals` read first, generic regions under templates 0–3 with AT pixels
and typical prediction (TPGDON, 6.2.5.7), and MMR (6.2.6) through the same T.6
decoder a fax uses. 7.2.7's **unknown segment data length** is read too: a
header may decline to say how long an immediate generic region is, and the end
is then found by scanning for the row terminator the region's own coding uses —
`FF AC` for the arithmetic coder, `00 00` for MMR — with the real row count in
the four bytes after it. The scan starts past the adaptive pixels, because a
nominal AT pair is the bytes `FF FE` and a template-0 region carries four of
them.

**Symbol dictionaries (6.5) and text regions (6.4) decode, arithmetically**:
Annex A's integer procedures and A.3's symbol-index procedure over the shared
coder; 6.5's height classes with one coder and one adaptive context set carried
across every symbol in a dictionary, as 6.5.8.1 requires; 6.5.10's export runs
selecting across imported and new symbols; and 6.4.5's strip decoding — the
strip coordinate accumulating, the out-of-band value ending a strip, gaps
measured from the previous symbol's far edge, `SBDSOFFSET`, multi-strip regions
and all four reference corners. What a dictionary exports is keyed by segment
number, and a region's symbols are the concatenation of its referred-to
dictionaries' exports *in reference order* (7.4.3).

**Halftone regions and pattern dictionaries decode** (6.6, 6.7, Annex C), the
third lineage. A halftone region does not code pixels: it codes a grid of grey
values and stamps a pattern from its dictionary at each cell, which is how a
dithered photograph is coded compactly. The dictionary is one collective bitmap
with every pattern side by side, and the grid is Gray-coded across bitplanes
that share one coder and one context set — 6.5.8.1's rule met again in another
clause. `HRX` and `HRY` are an 8.8 fixed-point vector, so the lattice may be
sheared, and 6.6.5.1's `HENABLESKIP` leaves the cells that fall outside the
region uncoded. All four grey-scale templates, both codings and the skip path
decode; the MMR variant carries every plane in one datastream, each ended by
T.6's EOFB and **byte aligned** after it (6.2.6).

**Refinement (6.3) decodes too**, in all three of the shapes T.88 gives it:
6.5.8.2.2's single refinement and 6.5.8.2.1's aggregate inside a symbol
dictionary — where an aggregate symbol is itself a text region — 6.4.11's
per-instance refinement inside a text region, and the generic refinement region
segments of 7.4.7 (types 40, 42 and 43). Both of 6.3.5.3's context templates
are implemented, with 7.4.7.3's adaptive pair and 6.3.5.6's typical prediction.
With them come 7.4.6.1's intermediate regions, which are decoded and kept
rather than drawn: a refinement region's reference is either one of those or,
failing that, whatever the page already holds under the region's own box
(6.3.2).

The refinement templates are **derived rather than transcribed**, and the
reason that is sound is worth stating where a reader will meet it. A context
index only labels an adaptive state slot — the decoder reads and writes
`state[cx]`, every slot starts identical, and the arithmetic coder's registers
are global — so relabelling every context through a bijection cannot change a
single decision, and the bit order in 6.3.5.3's figures is unobservable. Only
the *set* of positions is a fact about the format, and that set is held to the
pdf.js corpus, which codes one 399 by 400 picture a dozen ways: the encodings
that do not refine are ground truth for the ten that do, and all ten reproduce
it with **0 pixels different**
([`jbig2_refinement.rs`](../../crates/tinker-pdf/tests/jbig2_refinement.rs)).
6.3.5.6's TPGRON slot does not survive the relabelling, so it was recovered the
same way — one value in 8 192 reproduces the fixture, and one in 1 024 for the
narrower template.

That last rule is why a region whose referred-to dictionary is **absent or
refused is refused whole** rather than drawn from what did arrive: the
numbering is shared, so a missing dictionary does not cost its own symbols, it
renumbers all of them and every instance draws a different symbol at the right
place — a page that looks like text and says something else. T.88 Annex H.1's
own page 2 is that case, its arithmetic text region referring to page 1's
Huffman dictionary.

Measured over the corpus's JBIG2-bearing files, counting those whose decode
reports any refusal: **65 before this lineage landed, 52 after the arithmetic
variant, 49 after the Huffman one, 35 after refinement, 30 once the Huffman road
followed it, and 6 of 118 now** that transposed placement, clause 7.4.13's
custom code tables, 7.2.7's unknown data length and the halftone lineage have
all landed. None ever gained one.
[`jbig2_attribution.rs`](../../crates/tinker-pdf/tests/jbig2_attribution.rs)
names every one of the six and pins the counts.

Annex B's tables are reconstructed rather than transcribed, and what holds them
to the standard is Annex H coding the same two symbols twice — once with
`SDHUFF`, once through the MQ coder — which decode byte-identically
([design/jbig2-symbol-text.md](../design/jbig2-symbol-text.md)). **Three of the
fifteen are nonetheless short**: B.3 assigns canonical codes from prefix lengths
alone, so a table decodes every input only if `sum(2^-len)` is one, and B.7,
B.10 and B.12 sit at 0.640, 0.945 and 0.921 of it. A file selecting one of them
is refused rather than mis-decoded, which is the trade this module makes
everywhere; the three are pinned by
`annex_b_tables_that_are_not_complete_prefix_codes_are_named` and are a roadmap
row rather than a guess.

Polarity is returned in JBIG2's own sense (1 = black, 6.2.2); the inversion
belongs at the PDF boundary beside `/ImageMask` and `/Decode`. A file whose
page composited no region at all is refused rather than returned as a blank
white page that reads as a successful decode of a blank scan.

**JPEG 2000** (JPXDecode, 7.4.9; T.800): the JP2/JPX box container of Annex I
and bare J2K codestreams, Annex A marker segments with COC and QCC overriding
per component, Annex B tier-2 — tag trees, packet headers, precincts and all
five progression orders (B.12) — Annex D tier-1 on the shared MQ coder,
Annex E dequantisation, both Annex F inverse wavelets (the reversible 5/3 and
the irreversible 9/7 in fixed point) and the Annex G and I colour pipeline,
palettes and `cdef` included. Four of Table A.19's six code-block styles
decode: segmentation symbols (D.5's integrity check), `RESET`'s return to
Table D.7's states at every pass boundary, `VERTICALLY_CAUSAL`'s stripe that
depends on nothing beneath it, and `PREDICTABLE`, which constrains an encoder
and leaves a decoder's reading unchanged. An opacity channel is carried out separately in
`JpxOpacity` because what it is *for* is `/SMaskInData`'s rule (8.9.5.4) and
that decision stays outside the crate. The decoder's stance is that a wrong
JPEG 2000 decode looks like a photograph — the inverse wavelet smooths wrong
coefficients into a plausible image — so everything not implemented is refused
by name, and two integrity checks (packet lengths, the D.5 segmentation
symbol) catch a mis-parse before any pixel exists. Measured against the
corpus's nineteen readable JPX files as of August 2026: **16 decode and 3
refuse by name**, and **none of the three is a code-block style**. One is a
budget (`jpx-budget-spent`, a ruling 1 hardening limit rather than a
capability gap) and two are veraPDF fixtures that are deliberately
non-conformant. It was 15 and 4: the file that moved is `jp2k-resetprob.pdf`,
whose only unusual bit is `RESET`. The fifteenth had been a case of its own —
never asked for at all, because its image sits two form XObjects deep and a
form's own `/Resources` were consulted nowhere; that one decodes now too
([rendering](rendering.md)).

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

**TIFF** (TIFF 6.0) is the second container decoder, and the argument for it
being here rather than in a reader is arithmetic: of the seven codings a TIFF
strip can be in, **six were already written for a `/Filter` name**. Compression
2, 3 and 4 are `ccitt.rs` (the same T.4/T.6 decoder `/CCITTFaxDecode` uses),
5 is `lzw.rs`, 7 is `jpeg.rs`, 8 and 32946 are `inflate.rs`, and `Predictor` 2
is `predictors.rs`'s `/Predictor 2` — Table 10 calls it "TIFF horizontal
differencing" because that is where PDF took it from. Only compression 32773,
PackBits (§9), is new, and it is forty lines. What the module adds beyond the
codecs is the part that is actually TIFF: both byte orders, the image file
directory with a cycle guard on the `NextIFD` chain, strips and tiles
(including edge tiles stored full size and padded), `PlanarConfiguration` 2,
`PhotometricInterpretation` 0 through 3 with the inversion 0 asks for, and a
`ColorMap` transposed out of p.23's three consecutive arrays into the RGB
triples every other palette in this engine is. Output is
[`PngImage`]'s shape deliberately — grey, grey+alpha, RGB or RGBA at 8 or 16
bits — so a consumer that splits an alpha channel into an `/SMask` learns one
layout rather than two. `tiff_scan` is `png_scan`'s counterpart: it walks the
directory, locates every strip and hands them back **without decompressing
one**, which is what the embed door's pass-through needs.

That door is `tinker_pdf_cos::tiff_image`, `png_image`'s sibling, and it is
where four of TIFF's codings stop being TIFF at all: compressions 2, 3 and 4
become `/CCITTFaxDecode` with Table 11 filled in from the directory's own
`T4Options`, 5 becomes `/LZWDecode`, 7 becomes `/DCTDecode` with `JPEGTables`
spliced in front, and 8 and 32946 become `/FlateDecode` — with `/Predictor 2`
where the file used one. A single-strip file of any of those reaches the page
as its own bytes and no raster is built. `TiffRoute` says which of the three
routes a file took, because "the picture is right" and "the pass-through
happened" are different claims and a build that quietly decoded everything
would satisfy the first.

Two things real TIFFs do that TIFF 6.0 does not describe are handled by name.
**Old-style LZW** — codes packed least significant bit first and widened one
code late, which is what encoders wrote before 1993 — is detected from the two
bytes that can tell it apart (a stream opening with Clear reads `0x80` one way
and `0x00` with the next bit set the other) and *transcoded* into the ordinary
packing, so `lzw.rs` decodes both and the dictionary exists once. And a
`ColorMap` whose every value is at or below 255 was written at 8 bits by a
encoder that forgot p.23's scaling; it is read as one, with
`Warning::TiffColorMapIsEightBit`, because the alternative is a palette image
that is uniformly almost black and reads as a decoder bug rather than as the
file's.

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
half is `png_decode`, `png_scan`, `tiff_decode`, `tiff_scan`, `packbits_decode`,
`inflate_raw`, `crc32` and `jxr_decode` (returns `JxrImage`).

`jxr_decode` is deliberately **not** a `Filter` or an `ImageCodec` variant.
Those two enums are PDF `/Filter` dispatch — what a `/Filter` *name* resolves
to — and no `/Filter` in ISO 32000-2 Table 6 reaches ITU-T T.832;
`/JPXDecode` is JPEG 2000, a different format. JPEG XR arrives only as an XPS
image part, so it is a free function beside `png_decode` and carries its own
`JxrWarning` set rather than widening `Warning`, which is the closed list of
leniencies a *PDF stream filter* performs. `docs/design/jpeg-xr.md` argues
this at length; the short version is that an entry nothing can name would
make both enums wrong.

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| JBIG2 retained bitmap-coding contexts (7.4.2), and a selector naming a custom table the segment did not refer to | `Warning::Jbig2VariantSkipped` | Variants of a segment this build *does* decode, named apart from a segment type it does not, so a file needing one is distinguishable from one needing a lineage nobody has started. **SDHUFF, SBHUFF, SDREFAGG, SBREFINE, segment types 40/42/43, `TRANSPOSED` and clause 7.4.13's custom code tables have all left this row.** What is left is a retained context, which is one corpus file and below ruling 3's line. **The whole symbol lineage decodes otherwise** — arithmetic and Huffman, with and without refinement, in either combination, either placement orientation, standard tables or the file's own, and a non-zero refinement delta through **B.14 and B.15** on both the text region's road and the dictionary's | [ROADMAP](../ROADMAP.md) |
| JBIG2 text region whose referred-to dictionary is absent or refused | `Warning::Jbig2VariantSkipped` | 7.4.3 numbers symbols across every referred-to dictionary, so drawing it renumbered says something else — refused whole instead | T.88 7.4.3 |
| JBIG2 dictionary past its symbol or instance budget | `Warning::Jbig2SymbolLimitHit` | `SDNUMNEWSYMS`, `SDNUMEXSYMS` and `SBNUMINSTANCES` are attacker-controlled 32-bit counts; capped before allocation (ruling 1) | [rulings](../rulings.md) |
| JBIG2 region or page above the output ceiling | `Warning::Jbig2RegionTooLarge` | Width and height are attacker-controlled 32-bit values; refused before allocation (ruling 1) | [rulings](../rulings.md) |
| JPX markers RGN, POC, PPM, PPT, CRG (T.800 Table A.2) | `Warning::JpxMarkerUnsupported` | Never skipped: a skipped RGN draws a bright rectangle and a skipped POC mis-parses every packet after it | [ROADMAP](../ROADMAP.md) |
| JPX markers Table A.2 does not define (all of ISO/IEC 15444-2) | `Warning::JpxMarkerUnknown` | Part 2 is a non-goal; an unknown marker cannot be measured past | [ROADMAP](../ROADMAP.md) |
| JPX coding features: two of Table A.19's six code-block styles — `BYPASS` and `TERMALL` — plus unmappable `colr` and unequal channel depths | `Warning::JpxFeatureUnsupported` | A wrong JPEG 2000 decode is a plausible photograph; refusal beats a blur nobody can distinguish from a bad scan. The two left both move where a coding pass's *bytes* start, so they need a length per pass out of the packet header (B.10.7) rather than anything tier-1 can do | [ROADMAP](../ROADMAP.md) |
| JPX component precision above 16 bits | `Warning::JpxPrecisionUnsupported` | T.800 allows 38 bits; the sample path carries 16, so this is refused rather than truncated | [ROADMAP](../ROADMAP.md) |
| JPX tile-parts out of order, or a codestream with no complete tile | `Warning::JpxStructureInvalid` | Out of order is a codestream contradicting itself, and reassembling in stream order would produce a picture wrong in a way that looks like compression. A tile *short* of its declared parts is a different failure — a file that stopped — so it is left blank and reported as `JpxTruncated` wherever any tile survives, which is `JxrWarning::TileDroppedAsZero`'s bargain; only a codestream with no whole tile at all is refused | [ROADMAP](../ROADMAP.md) |
| JPX work/sample/code-block budgets spent | `Warning::JpxBudgetSpent` | The budgets are totals, never refunded — a per-item cap is not a work cap once the structure branches (ruling 1) | [rulings](../rulings.md) |
| JPEG arithmetic coding | `JpegError::Arithmetic` (gate `Capability::JpegArithmetic`) | Reported rather than half-decoded | [ROADMAP](../ROADMAP.md) |
| JPEG precision other than 8 bits | `JpegError::UnsupportedPrecision` (gate `Capability::Jpeg12Bit`) | Same contract: named, not guessed at | [ROADMAP](../ROADMAP.md) |
| TIFF `PhotometricInterpretation` 4, 5, 8, 32803, and 6 outside compression 7 | `TiffError::UnsupportedPhotometric` | A CMYK or CIELab image read as RGB is not a degraded picture, it is a different one; YCbCr is read only where a JPEG has already undone it | — |
| TIFF `Compression` 6 (old-style JPEG), 34712 (JPEG 2000) and the rest | `TiffError::UnsupportedCompression` | Named by code, so a refusal says which | — |
| TIFF `SampleFormat` 2 or 3 (signed, IEEE float) | `TiffError::UnsupportedSampleFormat` | A different number line; reading it as unsigned produces a picture rather than a refusal | — |
| TIFF `Predictor` 3, `PlanarConfiguration` past 2, `BitsPerSample` outside {1,2,4,8,16}, two depths in one image | `TiffError::UnsupportedPredictor`, `UnsupportedPlanarConfiguration`, `UnsupportedBitDepth`, `UnequalBitDepths` | Nothing in the sample path carries two depths at once, and half-expanding one is worse than saying so | — |
| BigTIFF (magic 43) | `TiffError::BigTiff` | Eight-byte offsets and a different directory layout wearing the same two order bytes | — |
| TIFF past `MAX_TIFF_SAMPLES`, `MAX_TIFF_SEGMENTS` or the caller's ceiling | `TiffError::TooManySamples`, `TooManySegments`, `ExceedsOutputLimit` | Width, height and `StripOffsets`'s count are attacker-controlled 32-bit values; refused before allocation (ruling 1) | [rulings](../rulings.md) |
| JPEG XR fixed-point, half-float and 32-bit float pixel formats (Table A.6's SINT and Float rows) | `JxrRefusal::FloatOrFixedPointFormat` | 9.10.7's postscaling makes those numbers mean something `JxrImage`'s 8- and 16-bit unsigned samples cannot say; reinterpreting them returns a picture whose values are a different quantity | [design](../design/jpeg-xr.md) |
| JPEG XR CMYK, CMYKDIRECT, NCOMPONENT and RGBE output formats | `JxrRefusal::UnsupportedColourFormat` | A colour pipeline with no consumer in this engine; a CMYK image read as RGB is a different picture, not a degraded one | [design](../design/jpeg-xr.md) |
| A Table A.6 GUID this build has no row for | `JxrRefusal::UnknownPixelFormat` | The GUID is what names the channel order, so an unknown one cannot be guessed at | [design](../design/jpeg-xr.md) |
| JPEG XR packed output depths BD1WHITE1, BD1BLACK1, BD5, BD565, BD10 | `JxrRefusal::PackedOutputBitdepth` | 9.10.8.3 to 9.10.8.6's sub-byte and cross-byte packings | [design](../design/jpeg-xr.md) |
| JPEG XR interleaved alpha image plane (8.3.18) | `JxrRefusal::InterleavedAlphaPlane` | A.3.2's *separate* plane is what the encoder on this machine emits and what decodes here; the interleaved form has no fixture, and building it against nothing is how a plausible wrong decode gets shipped | [design](../design/jpeg-xr.md) |
| JPEG XR YUV420, YUV422 and YUVK internal colour formats | `JxrRefusal::SubsampledInternalFormat` | 9.10.3's chroma upsampling and a macroblock geometry that is not the 4:4:4 one this build implements — three whole branches of clauses 8.7, 9.5, 9.6 and 9.8, absent rather than written and untested | [design](../design/jpeg-xr.md) |
| JPEG XR windowing with a non-zero top or left margin (8.3.13) | `JxrRefusal::WindowedOrigin` | It shifts the whole sample grid off the macroblock origin; the bottom and right margins are ordinary padding and are always handled | [design](../design/jpeg-xr.md) |
| JPEG XR past `MAX_JXR_SAMPLES`, `MAX_JXR_TILES`, `MAX_JXR_COMPONENTS`, `MAX_JXR_MACROBLOCKS` or the caller's ceiling | `JxrError::TooManySamples`, `TooManyTiles`, `TooManyComponents`, `TooManyMacroblocks`, `ExceedsOutputLimit` | 8.3.23 permits 4096 tile columns, 8.4.12 permits 4111 components and 8.3.21's dimensions are 32-bit, and the standard bounds each factor without bounding the product; the budgets are **totals**, checked before any buffer exists (ruling 1) | [rulings](../rulings.md) |

Every `JxrRefusal` variant is reached by a test in
`src/jxr/tests/refusals.rs`, for the reason that file's header gives: a
published refusal list nothing reaches is a claim rather than a check, and a
refusal whose condition a later milestone made unreachable should fail a test
rather than quietly become decoration.

**JPEG XR damage is a warning, not a refusal.** A tile that will not parse is
dropped to zero and reported as `JxrWarning::TileDroppedAsZero`, which is
8.7.10.1's own NOTE 1 and ruling 2's degrade path; an alpha plane that will
not decode leaves an opaque image and `JxrWarning::AlphaPlaneDropped`. Both
set `complete` to false. The distinction is deliberate: damage that costs
*pixels* leaves a partial image, and damage that costs *meaning* is refused.

### JPEG XR: decoded but unadjudicated

The refusals above are decisions. This is the other list — what decodes with
**nothing checking that the result is right** — and it exists because JPEG XR
is the only codec here with no second implementation available to this
repository. Ruling 13 forbids asking one: a third party may generate an input
and may never adjudicate an output. The list, the three first-party evidence
legs that stand in place of an oracle, and what each of them cannot reach are
in [design/jpeg-xr.md](../design/jpeg-xr.md) and
[features/xps.md](xps.md); the largest single gap is the **quantised lossy
path**, which the lossless identity does not touch at any quantizer but 1.

The list is a **limit rather than owed work** — [ROADMAP](../ROADMAP.md)
carries it under Named non-goals for that reason. Only T.832's own
conformance bitstreams would shrink it, and they are not freely licensed.

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
- In-crate: `tiff.rs` is held to files this repository writes byte by byte
  from TIFF 6.0's own field layouts, and to coded strips written from the
  coding specification that owns each — a real T.4/T.6 coder for compressions
  2, 3, 3-with-2D and 4, both LZW bit orders from one code stream, and a
  baseline T.81 datastream split the way Technical Note 2 splits one. Both
  byte orders, strip and tile layout, all four photometric interpretations and
  every compression above decode to a raster asserted against literal expected
  bytes. `packbits.rs` decodes §9's own worked example, both halves
  transcribed from the specification text. `mutated_fixtures_never_panic` puts
  the same fixtures through seeded xorshift damage aimed at the directory's
  entry array, so ruling 1 is enforced on stable and not only under
  `cargo-fuzz`. All **seven** of the TIFF warnings are reached by a test that
  asserts them, for the reason `jpx`'s refusal suite exists: a variant nothing
  can reach is a claim rather than a check.
- In-crate: `jbig2.rs` decodes T.88 Annex H.1's published datastream example
  byte for byte; `mq.rs` holds Annex H.2's test sequence as a permanent
  fixture, because the coder serves two codecs; `src/jpx/tests/refusals.rs`
  reaches every entry of the JPX refusal list, so "the refusals are the
  feature" is checked, not claimed.
- Fuzzing: nine of the 25 fuzz targets exercise this crate —
  `ascii_filters`, `lzw`, `inflate`, `ccitt`, `jpeg`, `jbig2`, `jpx`, `png`,
  `tiff`. The last is the first target that reaches five other decoders
  through one parser, because a two-byte `Compression` field is what chooses
  between them, and it carries six committed seeds written by an `#[ignore]`d
  test in this crate so the seeds and the fixtures cannot drift.
- Downstream: the `image`, `jbig2` and `jpx` render fingerprints among the
  15 in `crates/tinker-pdf/tests/determinism.rs` pin decoded pixels
  bit-for-bit across targets ([determinism](determinism.md)), and the whole
  workspace stands at 4 483 passed / 0 failed / 52 ignored
  (Windows x86_64, September 2026). See [verification](../verification.md) for
  the full harness.
