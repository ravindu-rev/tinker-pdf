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

- **Arithmetic-coded JPEG, 12-bit JPEG.** Deferred behind capability flags
  with corpus hit-rate gates per ruling 3 — implemented when the hit-rate
  report from [14-testing-and-corpora](14-testing-and-corpora.md) shows real
  documents need them, not before. This crate returns a typed `Unsupported`
  value; the rendering device substitutes the neutral placeholder and appends
  the `Bitmap` warning per ruling 2. Never a hard failure, and never this
  crate's job to draw the placeholder.

- **JPXDecode (7.4.9) — *half* a non-goal, since August 2026,** and unlike
  JBIG2 the gate did **not** open: gap 23 measured JPX at 0.4 % of 4 525
  files and [gaps/18](gaps/18-jpx-decision.md)'s own amendment reads that
  number and argues for the 150-line header probe. The owner chose the
  decoder anyway, and [gaps/18a](gaps/18a-jpx-decoder.md) records at its top
  that this was a choice and not a finding. What this crate decodes so far is
  T.800 Annex I's JP2 boxes and a bare J2K codestream, Annex A's marker
  segments with COC and QCC overriding per component, Annex B's tier-2 —
  tag trees, packet headers, precincts and all five progression orders — and
  Annex D's tier-1 on the shared MQ coder. What it still refuses is
  dequantisation, both inverse wavelets and the colour pipeline, which are
  milestones 4 to 6, plus everything on that plan's enumerated refusal list:
  RGN, POC, PPM and PPT, five of Table A.19's six code-block styles, Part 2,
  precision above 16 bits. All of it keeps the shape above exactly —
  `Unsupported(Capability::Jpx)` and the neutral placeholder — and the
  capability still answers `Some(Capability::Jpx)`. What changed is what that
  answer *means*: "this crate may refuse these bytes", not "this crate will
  never decode them". The distinction matters more here than for JBIG2,
  because the failure mode of a partly-built JPEG 2000 decoder is not a
  visible break but a plausible photograph.

- **JBIG2Decode (7.4.7) — *half* a non-goal, since August 2026.** The gate
  opened: gap 23 ran the corpora and JBIG2 came back at 2.3 % of 4 525 files,
  the highest of the three capabilities ruling 3 defers, and
  [gaps/17](gaps/17-jbig2-generic-region.md) built the generic-region lineage
  against it. What this crate now decodes is the MQ arithmetic coder (T.88
  Annex E, in its own module for gap 18's sake), clause 7's segment headers,
  Annex D.3's embedded organisation with `/JBIG2Globals`, generic regions on
  templates 0–3 with AT pixels and typical prediction, and MMR regions through
  `T6Rows`. What it still refuses is **symbol dictionaries and text regions**
  (6.4, 6.5) — roughly 2 500 further lines with the integer arithmetic
  decoders and the standard Huffman tables — plus halftone and refinement
  regions. Those keep the shape above exactly: `Unsupported(Capability::Jbig2)`
  and the neutral placeholder. The line between the two halves is not a detail
  a reader can skip, because the refused half is the *common* one: it is what
  an OCR pipeline emits.
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

*Amended, August 2026.* "COS maps `/Filter` … into this" turned out to have a
second, silent implementation. An inline image (8.9.7) has no object number, so
its bytes never reach a stream tier, and `decode_inline` in
`crates/tinker-pdf/src/resources.rs` had grown a chain of its own that passed
no predictor at all, hardcoded `/EarlyChange`, and refused DCT and CCITT. The
mapping is therefore no longer private to the COS crate:
`CosDocument::filter_chain(dict, sink)` exposes it, so Table 10's defaults exist
in one place and both paths reach the same `apply_chain`. Gap
[08](gaps/08-inline-image-filters.md) has the measurements. The rule the
episode leaves behind is that a *second* mapping is the failure mode to watch
for here, not a wrong one: the specs above are testable, and two callers
agreeing to differ are not.

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
| 4 | Chain driver + capability surface + hardening | `decode_chain` drives filter/params arrays incl. null entries and post-codec tails; `Unsupported(Capability)` returned for JPX/arithmetic/12-bit probes, and for a JBIG2 stream **whose regions this build cannot decode** (see the amendment below); 01-cos decodes compressed xref (7.5.8) and object streams (7.5.7) through this crate on real files; 24h aggregate fuzz, zero findings — **wave 1 done, 09-writing unblocked** | S |
| 5 | JPEG baseline | SOF0/SOF1 8-bit gray + YCbCr with all common subsamplings; restart handling incl. corrupt-resync fixture; `pdfcmp` within per-fixture perceptual tolerance of libjpeg-turbo (`djpeg` subprocess) across the JPEG fixture set; `fuzz_jpeg` in CI | M |
| 6 | JPEG progressive + CMYK/YCCK/APP14 | Progressive matrix (spectral × successive-approximation) fixtures pass; APP14 transform cases 0/1/2 and RGB-component-ID case decode correctly; inverted-Photoshop-CMYK fixture matches oracle; plane allocation respects `Limits`; truncated-scan fixture yields partial image + warning | M |
| 7 | CCITT G3 1D/2D + G4 | T.4/T.6 reference images decode exact; quirk matrix (`/K` sign, `/EncodedByteAlign`, `/BlackIs1`, absent `/Rows`, missing EOFB) fixture-pinned; damaged-row fixture recovers by replication with warning; parity vs `mutool` on scanned fixtures; `fuzz_fax` in CI — **wave 2 done, 08's image milestones unblocked** | M |

Wave 1 (1–4) sums to the M in [PLAN.md](../PLAN.md); wave 2 (5–7) sums to
the top of its L band — milestone 7 shares nothing with 5–6 and can proceed
in parallel if a second pair of hands exists.

### Evidence, August 2026: progressive JPEG is not a tail case

Milestone 6 was scheduled behind baseline on the assumption that progressive
is the rarer half. The first eight real-world files ever put through the
engine ([STATUS](../STATUS.md)) contained progressive JPEG in **four of
them**, while JBIG2, JPX and arithmetic JPEG appeared in none.

Eight files decide nothing on their own, and the sample is biased: they came
from one machine and mostly from web-to-PDF tooling, which is exactly the
software that emits progressive JPEG. What it does establish is that
milestone 6 is not a long-tail item to sit indefinitely behind the flags in
Non-goals, and that the codecs it was implicitly ranked alongside did not
appear at all.

Ruling 3 says schedule capabilities on hit-rates rather than on ambition. The
action that implies is not "build progressive immediately" but "measure before
ranking anything else against it" — run the pinned corpora through `tpdf
check` and read the distribution off a sample large enough to mean something.

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

## Amendment — August 2026: what the capability surface now asserts

Milestone 4's exit criterion said `Unsupported(Capability)` is returned for a
JBIG2 probe. That was a statement about a codec nobody had written, and it is
no longer true as written, so it is corrected here rather than quietly relaxed
in a test.

`ImageCodec::Jbig2.capability()` still answers `Some(Capability::Jbig2)`, and
deliberately. What changed is what the answer *means*. It used to mean "this
crate will never decode these bytes". It now means "this crate may refuse
these bytes, and when it does, that is the capability to report" — which is
the same contract every other gated codec has, and the same thing the caller
does with it: draw the neutral placeholder and name the codec.

The distinction is worth the paragraph because the failure it guards against
is specific. A JBIG2 stream carrying only a symbol dictionary and a text
region decodes its page information segment perfectly well, finds no region to
composite, and could hand back a blank white page as a success — which is
indistinguishable from a correct decode of a blank scan, and strictly worse
than the placeholder it replaced. So the refusal is not the absence of a
feature; it is a feature, tested as one, in both directions: a file with *no*
decodable region is refused, and a file with a generic region *beside* an
undecodable symbol dictionary draws the region and reports the rest.
