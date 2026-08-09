# Phase 02 — Filters

When this phase is done, `tinker-pdf-filters` decodes every mainstream PDF
stream filter with hand-rolled code — inflate, LZW, the ASCII and run-length
codings, baseline and progressive JPEG, CCITT G3/G4 — and encodes deflate for
the writer. It ships in two waves because its consumers arrive in two waves:
wave 1 (the byte filters) is what [01-cos-and-object-model](01-cos-and-object-model.md)
needs to read compressed xref and object streams and what
[09-writing](09-writing.md) needs to start at all; wave 2 (the image codecs)
feeds [08-rendering-device](08-rendering-device.md) and nothing earlier, so it
runs later without blocking the semantics lane. The crate is a leaf under
ruling 8 of [99-consistency](99-consistency.md): bytes and plain parameter
structs in, bytes and typed warnings out, zero COS types, independently
fuzzable and publishable on its own.

## Scope

Wave 1 (M):

- Own inflate (RFC 1951): stored, fixed-Huffman and dynamic-Huffman blocks;
  zlib wrapper (RFC 1950) with raw-deflate fallback when the header is absent
  or invalid, which real PDFs require.
- Own deflate encoder for the writer: zlib-wrapped, one fixed strategy, no
  levels.
- Predictors (ISO 32000-1 7.4.4.4, Table 10): `/Predictor 2` TIFF horizontal
  differencing; `/Predictor 10–15` PNG prediction where each row's tag byte
  selects among the five PNG row filters (None/Sub/Up/Average/Paeth)
  regardless of the declared value; `/Colors`, `/BitsPerComponent`,
  `/Columns` honored including sub-byte components.
- ASCIIHexDecode (7.4.2), ASCII85Decode (7.4.3), RunLengthDecode (7.4.5).
- LZWDecode (7.4.4): 9→12-bit variable codes, Clear/EOD, `/EarlyChange` 0
  and 1 (default 1). Decode only — the writer always prefers deflate.
- The chain driver: `/Filter` arrays applied in order with per-entry decode
  params, null entries, image codecs terminal.
- The capability surface for deferred codecs (see Non-goals) and the
  leniency policy: truncated or corrupt streams decode what is there and
  warn; never fail the page.
- A cargo-fuzz target per decoder, in `fuzz/`, from the first commit of each
  decoder.

Wave 2 (L):

- Own JPEG (DCTDecode, 7.4.8; ITU-T T.81): baseline (SOF0) and extended
  sequential (SOF1) at 8-bit; progressive (SOF2) with spectral selection and
  successive approximation; restart markers with resync; grayscale, YCbCr,
  CMYK and YCCK; Adobe APP14 transform handling including the
  inverted-Photoshop-CMYK convention.
- Own CCITTFaxDecode (7.4.6; ITU-T T.4/T.6): G3 1D (`/K 0`), G3 mixed 2D
  (`/K > 0`), G4 (`/K < 0`); `/Columns`, `/Rows`, `/EncodedByteAlign`,
  `/BlackIs1`, `/EndOfLine`, `/EndOfBlock` and the off-spec quirks around
  them.

## Non-goals

- **JBIG2Decode (7.4.7), JPXDecode (7.4.9), arithmetic-coded JPEG,
  12-bit JPEG.** Deferred behind capability flags with corpus hit-rate gates
  per ruling 3 — implemented when the nightly hit-rate report from
  [14-testing-and-corpora](14-testing-and-corpora.md) shows real documents
  need them, not before. This crate returns a typed `Unsupported` value; the
  rendering device substitutes the neutral placeholder and appends the
  `Bitmap` warning per ruling 2. Never a hard failure, and never this
  crate's job to draw the placeholder.
- **The Crypt filter (7.4.10).** Decryption happens in
  [01-cos-and-object-model](01-cos-and-object-model.md) +
  [03-encryption](03-encryption.md) before bytes reach this crate. The name
  never appears in this crate's API.
- **Parsing `/DecodeParms`.** COS translates PDF dictionaries into the plain
  parameter structs below (ruling 8). This crate never sees a name object.
- **`/Decode` array application and color management.** Sample remapping
  belongs to [06-content-and-text](06-content-and-text.md); CMYK→render-space
  conversion belongs to `tinker-pdf-color` under
  [08-rendering-device](08-rendering-device.md). The JPEG decoder stops at
  Gray/RGB/CMYK samples.
- **Encoders other than deflate.** The writer emits FlateDecode for
  everything it compresses; ASCIIHex/ASCII85/RLE/LZW/JPEG/CCITT encoding has
  no consumer and is not built.
- **A streaming pull API.** Wave 1 is one-shot slices-in/`Vec`-out. The
  inflate core is an incremental state machine internally, so a streaming
  surface can be exposed later if profiling in 06 or 08 demands it — but it
  is not built on speculation.

## Design

### Shape of the crate

No I/O, no threads, no platform intrinsics — which is the whole of the
wasm32-unknown-unknown story for this crate: it is pure computation and
compiles for the target from day one, with CI proving it. All arithmetic in
the image decoders is integer/fixed-point so output is bit-identical across
linux/windows/macos/wasm; ruling 4 formally binds the rasterizer, but goldens
for this crate get the same property for free and CI compares them across all
four targets too.

### Leniency and the output contract

The policy, stated once: **corrupt input truncates and warns; it never
errors.** Real-world PDFs contain streams cut short by broken `/Length`
values, interrupted downloads, and encoders that never read the spec.
MuPDF's tolerance here is a large part of its moat, and matching it is
cheaper at the decoder layer than anywhere above. Errors are reserved for
the caller holding the API wrong (`BadParams`) and for deferred capabilities
(`Unsupported`), both of which are decisions, not data conditions.

```rust
pub struct Limits {
    /// Hard ceiling on produced bytes. Mandatory — a 1 KB flate stream can
    /// legally expand to gigabytes, and a lenient decoder without a ceiling
    /// is a denial-of-service primitive. Hitting it truncates and warns.
    pub max_output: usize,
}

pub struct Decoded {
    pub data: Vec<u8>,
    /// false: input ended early, was damaged, or hit `max_output`.
    pub complete: bool,
    /// Typed leniency records (ruling 10). This crate reports *what* it
    /// tolerated; the caller attaches *which object* it happened in.
    pub warnings: Vec<Warning>,
}

pub enum FilterError {
    BadParams(&'static str),
    Unsupported(Capability),
}

pub enum Capability { Jbig2, Jpx, JpegArithmetic, Jpeg12Bit }
```

`complete: false` plus warnings is what keeps leniency debuggable: "it
decoded" and "it decoded cleanly" stay distinguishable, and oracle-diff can
flag divergence even on streams we claim to have handled.

### Inflate and the zlib wrapper

Canonical-Huffman decode with a two-level table: a 9-bit primary lookup
covering the overwhelming majority of symbols, secondary subtables for longer
codes. Because decoding is to memory, the output `Vec` doubles as the 32 KB
back-reference window — no ring buffer, no copy. The core is a resumable
state machine (push bytes, get progress), wrapped by the one-shot API; that
is what makes the any-prefix property testable and leaves the streaming door
open without committing to it.

Wrapper leniency, each case seen in the wild: invalid CMF/FLG → retry the
same bytes as raw deflate; FDICT set → warn and attempt raw (preset
dictionaries do not occur in PDFs); Adler-32 mismatch → warning, never an
error; trailing garbage after the final block → ignored. Truncation mid-block
returns everything produced so far with `complete: false`.

### Deflate for the writer

Hash-chain LZ77 over a 32 KB window, minimum match 3, maximum 258, lazy
matching, dynamic Huffman trees per block with fixed-tree and stored-block
fallbacks when they win on size — roughly zlib level 6 behavior, as one fixed
strategy with no tuning knobs. The stored-block fallback is the guarantee
that output is never meaningfully worse than uncompressed. Correctness gate
is the round-trip property (own deflate → own inflate == identity, proptest)
plus an external zlib inflating our output as a CI subprocess per ruling 9;
the size gate keeps us honest against the reference implementation without
chasing it forever.

### Predictors

Applied as a post-pass after Flate/LZW, on rows of
`ceil(colors × bpc × columns / 8)` bytes. PNG rows carry a leading tag byte
0–4; Paeth per the PNG spec; a short final row is processed to the bytes
available and warned. TIFF predictor 2 adds left-neighbor components with
wrap, honoring `bpc` 1/2/4/8/16 — the sub-byte cases take a slow bit-level
path that is correct rather than fast, which matches how rarely they occur.

### LZW

Table of 4096 entries, codes widen 9→12 bits; with `/EarlyChange 1` (the
default) the width bumps one code early — getting this wrong corrupts every
stream from Acrobat-era encoders, so both settings are pinned by fixtures.
Leniency: an out-of-range code stops decode with a warning; a full table
without a Clear keeps decoding with the table frozen, which is what shipping
readers do.

### ASCII and run-length codings

ASCIIHex: whitespace skipped, `>` ends, an odd final digit implies a trailing
zero, invalid characters are skipped with a warning. ASCII85: `~>` ends,
`z` is a zero group (warned if mid-group), whitespace skipped, a final
partial group of n chars yields n−1 bytes, a group decoding above 2³²−1 is
clamped and warned, missing EOD consumes to end-of-input. RunLength: literal
runs, repeat runs, `128` ends; missing EOD at input exhaustion is tolerated.
All three are trivial; they are listed because their leniency cases are
exactly where cheap corpus wins live.

### The chain driver

```rust
pub enum Filter { Flate, Lzw, AsciiHex, Ascii85, RunLength, Dct, Ccitt, Jbig2, Jpx }

pub struct FilterSpec {
    pub filter: Filter,
    pub predictor: Option<PredictorParams>,  // Flate and LZW only
    pub early_change: bool,                  // LZW only, default true
}

pub enum ChainOutput {
    Bytes(Decoded),
    EncodedImage { kind: ImageCodec, data: Vec<u8>, warnings: Vec<Warning> },
}

pub fn apply_chain(
    input: &[u8],
    chain: &[FilterSpec],
    limits: &Limits,
) -> Result<ChainOutput, FilterError>;
```

COS maps `/Filter` name-or-array plus `/DecodeParms` null-or-dict-or-array
into this, in order. Image codecs (`jpeg_decode`, `fax_decode`) keep their own
entry points, because their output is an image, not bytes — but they are
*named* in the chain enum so the driver can be the one place that knows a
codec terminates a chain. It applies the byte prefix, then returns the
still-encoded payload tagged with its codec for the caller to forward; that
keeps "which codec" from being re-derived by every caller. A filter name
appearing *after* an image codec is nonsense: the tail is dropped with a
warning. Sub-crate parameters (predictor, `/EarlyChange`) ride on the spec
rather than on the variant so COS can forward a `/DecodeParms` entry without
first deciding whether it is meaningful for the named filter.

### JPEG (DCTDecode)

```rust
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// 1 = gray, 3 = RGB, 4 = CMYK. Interleaved 8-bit samples for JPEG;
    /// packed 1-bpp MSB-first rows, byte-padded, for fax.
    pub components: u8,
    pub data: Vec<u8>,
    pub complete: bool,
    pub warnings: Vec<Warning>,
}

pub fn jpeg_decode(input: &[u8], limits: &Limits)
    -> Result<DecodedImage, FilterError>;
```

Decisions and reasons:

- **IDCT is the AAN scaled-integer 8×8** — the same family libjpeg uses.
  Bit-exact parity with libjpeg-turbo is explicitly not the goal (the spec
  itself defines conformance statistically, and IDCT/upsampling choices
  differ legitimately between correct decoders); staying in the same
  algorithm family keeps the perceptual delta small, and `pdfcmp` tolerance
  is the gate.
- **Chroma upsampling is triangle ("fancy") for 4:2:0/4:2:2 from the
  start**, not pixel replication — the parity oracle is libjpeg-turbo output
  and replication visibly diverges on chroma edges, so building the cheap
  version first would just be building it twice.
- **Progressive** accumulates coefficients in per-component `i16` planes
  across scans (spectral selection Ss/Se, successive approximation Ah/Al, DC
  and AC first/refinement passes, EOB runs), with one IDCT at the end. This
  is the memory-heavy path — planes for a large image dwarf the output — so
  `Limits` is checked against plane allocation too, not just output bytes.
- **Color identification:** 1 component = gray; 3 = YCbCr unless APP14
  declares transform 0 or the component IDs are literally `R`,`G`,`B`;
  4 = CMYK for transform 0/absent, YCCK for transform 2. YCbCr→RGB and
  YCCK→CMYK conversion happens inside the decoder with fixed-point JFIF
  constants, because every consumer wants samples, not coefficients. CMYK
  stays CMYK — rendering conversion is tinker-pdf-color's problem.
- **Adobe CMYK inversion:** APP14-marked CMYK/YCCK JPEGs (the Photoshop
  lineage) store inverted values; when the Adobe marker is present the
  decoder inverts, matching the convention of every mainstream renderer, and
  records a warning-level note so the pipeline can see it happened.
- **Leniency:** corrupt entropy data → scan forward to the next marker
  (RSTn or otherwise), fill the skipped MCUs with neutral gray, continue,
  warn. Missing or out-of-order restart markers → resync by marker scan.
  Height 0 in the SOF fixed up by a later DNL marker. Truncated scan →
  partial image, `complete: false`. SOF9/SOF10 (arithmetic) and 12-bit
  precision → `Err(Unsupported(..))`, placeholder upstream.

### CCITTFaxDecode

Modified-Huffman white/black run tables for 1D; the 2D coding modes
(vertical −3…+3, horizontal, pass) over reference-line changing elements
a0/a1/a2/b1/b2 for `/K > 0` lines tagged 2D and for all of G4. Defaults per
Table 11: `/Columns 1728`, `/K 0`, `/BlackIs1 false` (0 bits are black),
`/EncodedByteAlign false`, `/EndOfBlock true`.

```rust
pub struct FaxParams {
    pub k: i32,
    pub columns: u32,
    pub rows: Option<u32>,
    pub encoded_byte_align: bool,
    pub black_is_1: bool,
    pub end_of_line: bool,
    pub end_of_block: bool,
}

pub fn fax_decode(input: &[u8], params: &FaxParams, limits: &Limits)
    -> DecodedImage;
```

The quirks are the actual work: encoders that byte-align G4 rows off-spec
(honored when `/EncodedByteAlign` says so, and probed for when a row fails
to decode at the expected bit position); missing EOFB/RTC; `/Rows` absent
(decode until data or `max_output` is exhausted) or wrong (trust the data,
warn). Damaged-row recovery follows fax practice: replicate the previous
row, warn, continue — a visibly plausible scan beats a failed page, and this
is precisely the "decode what is there" policy applied to 1-bpp data.

### Fuzzing

Every decoder gets a `cargo fuzz` target the day it exists (ruling 1 makes a
fuzz crash a release blocker): no panic, no allocation beyond `Limits`, no
runaway time on pathological input. Corpora are seeded from the fixture
streams and committed per [14-testing-and-corpora](14-testing-and-corpora.md).
Inflate additionally gets differential fuzzing once the encoder lands: mutate
own-deflate output, require decode-without-panic and, on the unmutated half,
identity.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Inflate + zlib wrapper + raw fallback | RFC 1950/1951 vectors pass (stored/fixed/dynamic); corpus of streams extracted from real PDFs decodes byte-equal to oracle output; any-prefix property holds under proptest (no panic, output is a prefix); `fuzz_inflate` running in CI; crate builds and tests green on wasm32-unknown-unknown | S |
| 2 | Deflate encoder | proptest round-trip own deflate → own inflate == identity on arbitrary and structured inputs; external zlib (CI subprocess, ruling 9) inflates our output byte-equal; total corpus-stream size ≤ 1.2× zlib level 6; stored fallback proven on incompressible input | S |
| 3 | Predictors + ASCIIHex/ASCII85/RunLength + LZW | Decode parity vs `mutool` stream extraction on fixture streams; PNG all five row filters and TIFF at bpc 1/2/4/8/16 pinned by vectors; `/EarlyChange` 0 and 1 both fixture-pinned; every leniency case in Design has a fixture; fuzz targets per decoder | S |
| 4 | Chain driver + capability surface + hardening | `decode_chain` drives filter/params arrays incl. null entries and post-codec tails; `Unsupported(Capability)` returned for JBIG2/JPX/arithmetic/12-bit probes; 01-cos decodes compressed xref (7.5.8) and object streams (7.5.7) through this crate on real files; 24h aggregate fuzz, zero findings — **wave 1 done, 09-writing unblocked** | S |
| 5 | JPEG baseline | SOF0/SOF1 8-bit gray + YCbCr with all common subsamplings; restart handling incl. corrupt-resync fixture; `pdfcmp` within per-fixture perceptual tolerance of libjpeg-turbo (`djpeg` subprocess) across the JPEG fixture set; `fuzz_jpeg` in CI | M |
| 6 | JPEG progressive + CMYK/YCCK/APP14 | Progressive matrix (spectral × successive-approximation) fixtures pass; APP14 transform cases 0/1/2 and RGB-component-ID case decode correctly; inverted-Photoshop-CMYK fixture matches oracle; plane allocation respects `Limits`; truncated-scan fixture yields partial image + warning | M |
| 7 | CCITT G3 1D/2D + G4 | T.4/T.6 reference images decode exact; quirk matrix (`/K` sign, `/EncodedByteAlign`, `/BlackIs1`, absent `/Rows`, missing EOFB) fixture-pinned; damaged-row fixture recovers by replication with warning; parity vs `mutool` on scanned fixtures; `fuzz_fax` in CI — **wave 2 done, 08's image milestones unblocked** | M |

Wave 1 (1–4) sums to the M in [PLAN.md](../PLAN.md); wave 2 (5–7) sums to
the top of its L band — milestone 7 shares nothing with 5–6 and can proceed
in parallel if a second pair of hands exists.

## Dependencies

- **Upstream: none.** This is a leaf crate (ruling 8) — no COS types, no
  other workspace crates. Wave 1 can start on day one; the semantics-lane
  ordering in [PLAN.md](../PLAN.md) only says COS's compressed-xref
  milestone waits on milestone 4 here, and COS lexes uncompressed fixtures
  in the meantime.
- **Infrastructure:** fuzz harness layout and corpus/fixture conventions
  from [14-testing-and-corpora](14-testing-and-corpora.md); oracle
  subprocess policy is ruling 9.
- **Unblocks:** [01-cos-and-object-model](01-cos-and-object-model.md) xref
  streams (7.5.8) and object streams (7.5.7) after milestone 4;
  [09-writing](09-writing.md) entirely (deflate is its first hard
  dependency); [08-rendering-device](08-rendering-device.md) image drawing
  after milestone 7; [03-encryption](03-encryption.md) indirectly, since
  encrypted files are near-universally also compressed.

## Risks

| Risk | Mitigation |
| --- | --- |
| Zip bombs and memory blowups — leniency plus unbounded expansion is a DoS primitive | `Limits.max_output` is a mandatory parameter, checked on output *and* on JPEG coefficient-plane allocation; fuzzers assert the bound; hitting it truncates and warns, never aborts the page |
| Leniency masks real decoder bugs (silent truncation looks like success) | `complete` flag + typed warnings (ruling 10) on every tolerated condition; oracle-diff compares even "successful" decodes, so divergence surfaces in CI rather than in a viewer |
| JPEG perceptual gate mis-set — too strict fails legitimate IDCT variance, too loose hides bugs | Same algorithm family as the oracle (AAN integer IDCT, triangle upsampling) keeps true deltas tiny; per-fixture `pdfcmp` thresholds set from measured baseline-decoder deltas, ratcheted down, never up |
| Progressive JPEG complexity blowout — historically the most underestimated decoder | Baseline ships first as its own milestone and already unblocks most of 08's corpus; progressive is separately gated; arithmetic and 12-bit stay behind capability flags scheduled by hit-rate evidence (ruling 3), not ambition |
| CCITT quirk zoo is open-ended (off-spec alignment, wrong `/Rows`, missing terminators) | Quirk matrix built from corpus failures, not imagination — every wild-file failure becomes a fixture; row-replication recovery bounds the damage of the quirks we have not met yet |
| Own deflate compresses poorly, inflating every file the writer produces | 1.2× zlib-6 size gate on real PDF streams in CI; stored-block fallback caps the worst case; the strategy is fixed and the gate is the stopping rule — no open-ended ratio chasing |
| Cross-target determinism drift (wasm vs native) breaks shared goldens | Integer/fixed-point arithmetic only in decode paths; CI runs the golden comparison on all four targets from milestone 1, so drift is caught the week it is introduced |

---

Rulings in [99-consistency](99-consistency.md) that bind this phase: 1
(never panic), 2 (degrade, don't fail), 3 (evidence-driven capabilities), 8
(leaf crates stay PDF-free), 9 (oracles are subprocesses), 10 (warnings
carry provenance). See [PLAN.md](../PLAN.md) for lanes and checkpoints.
