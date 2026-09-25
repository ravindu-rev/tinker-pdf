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

**JPEG** (DCTDecode, 7.4.8; T.81): baseline (SOF0), extended sequential
(SOF1) and progressive (SOF2) — spectral selection, successive approximation
and EOB runs (T.81 G.1.1.1.1) all decode — and, since 20 September 2026, the
**arithmetic** frames beside them: **SOF9 and SOF10**, which are those same two
DCT processes with Annex D's entropy coder in place of Annex C's. **B.2.2's
twelve-bit precision decodes too**, outside the baseline frame where the clause allows it:
A.3.1's level shift is `2^(P-1)` rather than 128, and the samples are narrowed
to eight on the way out with `JpegPrecisionNarrowed` recorded, because every
`PixelFormat` this engine rasters into is eight bits deep and a wider sample
path is a change to the raster rather than to this codec. `JpegImage::precision`
says what the frame declared. Every mode
fills the same per-component coefficient buffer and is rendered once by a
single dequantise-and-transform pass with an integer separable IDCT.
`JpegColor` names what came out: greyscale, YCbCr already converted to RGB,
and CMYK with the Adobe transform undone — including the inverted-CMYK
convention, reported as its own variant.

**The arithmetic coder is `qm.rs`, and it is not `mq.rs`.** T.81 Annex D and
T.88 Annex E are cousins rather than the same coder: Table D.3 has 113 rows
against Table E.1's 47 and the two share exactly one `Qe` value, `X'0001'`,
where both bottom out; the MPS sub-interval sits at the base rather than the
LPS, so the comparison is `Cx < A` after `A = A - Qe` and the subtraction
happens on the other path; `Initdec` loads two whole bytes and starts `A` at
`X'10000'` rather than `X'8000'`; an `X'FF'` is followed by a stuffed **zero
byte** rather than a stuffed bit; and at a marker the decoder is fed **0-bits**
where T.88 feeds 1-bits. Each of those changes the decoded bits, so the two
modules are two implementations on purpose, and the one-value overlap is a test
rather than a sentence.

**What adjudicates it, and what does not — the two halves have different
answers.** The *coder* is adjudicated outright by published data: **T.81 K.4.1**
prints a 256-bit test sequence, the 32 bytes it encodes to, and Tables K.7 and
K.8's symbol-by-symbol traces of both directions. `qm.rs` decodes the published
bytes to the published decisions and encodes the published decisions to the
published bytes, which is the standing T.88 Annex H.2's fixture already has
here. The *statistical model* — F.1.4.4's Tables F.4 and F.5, and Table G.2 for
a refinement scan — has no such data behind it, because **T.81 publishes no
arithmetic-coded image**: Annex K was read section by section on 20 September
2026 and K.4.1 is the only data in it, T.83's own clause 4.4 says its test data
ships on diskettes rather than inside the document, and T.84 clause 4.2.1 says
compliance data is obtained from ISO and the ITU. So the model is transcribed
from its clauses, read against the document twice, and pinned by **hand-derived
decision sequences**: each test states the decisions T.81's figures say an
encoder emits for a given block, and asserts both the coefficients that come
back *and the statistics bin every decision was taken against* — because a
model that uses the wrong bin still decodes correctly while every bin is in its
initial state, and asserting coefficients alone would let it through.

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
followed it, 6 once transposed placement, clause 7.4.13's custom code tables,
7.2.7's unknown data length and the halftone lineage had landed, and **1 of 118
now** that 7.4.2's retained contexts, 6.5.8.2.2's reference offset, an
empty text region and Annex B's four mis-transcribed tables have followed. None
ever gained one. The one that is left is `pdfjs/issue3371.pdf`, a stream that
stops inside a segment — damage rather than a capability, and
`Jbig2Refusal::is_malformed` says so.
[`jbig2_attribution.rs`](../../crates/tinker-pdf/tests/jbig2_attribution.rs)
names it and pins the count.

Annex B's fifteen tables are **transcribed from ITU-T Rec. T.88 (02/2000)**,
which is published free of charge. Until September 2026 they were
*reconstructed* instead — the standard could not be fetched here — and four of
the fifteen were wrong. Three were known wrong from inside this repository, by
a property that needs no copy of it: B.3 assigns canonical codes from prefix
lengths alone, so a table decodes every input only if `sum(2^-len)` is one, and
B.7, B.10 and B.12 sat at 0.640, 0.945 and 0.921 of it. **B.15 summed to
exactly one and was wrong anyway**, which is why the check that found the
others could not have found it.

What holds them now: the transcription, the standard's own printed Encoding
column beside each line (`the_codes_b3_assigns_are_the_ones_the_standard_prints`),
Annex H coding the same two symbols twice — once with `SDHUFF`, once through
the MQ coder — which decode byte-identically, and
[`jbig2_lineages.rs`](../../crates/tinker-pdf/tests/jbig2_lineages.rs), which
requires every corpus file selecting one of them to draw the same picture as
the files that do not
([design/jbig2-symbol-text.md](../design/jbig2-symbol-text.md)).

Polarity is returned in JBIG2's own sense (1 = black, 6.2.2); the inversion
belongs at the PDF boundary beside `/ImageMask` and `/Decode`. A file whose
page composited no region at all is refused rather than returned as a blank
white page that reads as a successful decode of a blank scan.

**JPEG 2000** (JPXDecode, 7.4.9; T.800): the JP2/JPX box container of Annex I
and bare J2K codestreams, Annex A marker segments with COC and QCC overriding
per component, Annex B tier-2 — tag trees, packet headers, precincts, all
five progression orders (B.12) over B.12.2's **progression order volumes**,
and A.7.4's and A.7.5's packed packet headers
in both places they can live — Annex D tier-1 on the shared MQ coder,
Annex E dequantisation, both Annex F inverse wavelets (the reversible 5/3 and
the irreversible 9/7 in fixed point), **Annex H's region of interest** and the
Annex G and I colour pipeline, palettes and `cdef` included. **All six of
Table A.19's code-block styles decode**: segmentation symbols (D.5's integrity
check), `RESET`'s return to Table D.7's states at every pass boundary,
`VERTICALLY_CAUSAL`'s stripe that depends on nothing beneath it, `PREDICTABLE`,
which constrains an encoder and leaves a decoder's reading unchanged, and — the
last two, on 23 September 2026 — D.6's selective arithmetic coding bypass and
D.4's termination on each coding pass. An opacity channel is carried out separately in
`JpxOpacity` because what it is *for* is `/SMaskInData`'s rule (8.9.5.4) and
that decision stays outside the crate. The decoder's stance is that a wrong
JPEG 2000 decode looks like a photograph — the inverse wavelet smooths wrong
coefficients into a plausible image — so everything not implemented is refused
by name, and two integrity checks (packet lengths, the D.5 segmentation
symbol) catch a mis-parse before any pixel exists.

**`BYPASS` and `TERMALL` are two capabilities over one mechanism.** What
they share is B.10.7.2's *multiple codeword segments*: with either set, a
code-block's contribution to a packet is signalled as `K` lengths rather than
one, where `K` counts the coding passes Tables D.8 and D.9 terminate plus the
last pass the packet includes. Tier-2 therefore hands tier-1 a **list** of
segments rather than one byte range, and tier-1 opens a fresh reader on each —
an MQ decoder re-initialised per D.4.2, or D.6's raw one. That is not the same
thing as `RESET`: the decoder's registers are re-initialised and Table D.7's
context states are not. What they do *not* share is D.6's raw
reader. `TERMALL` adds no new way of reading a decision; `BYPASS` makes the
significance propagation and magnitude refinement passes from the fifth
bit-plane down carry raw bits, with their sign taken straight from the stream
by equation (D-2) rather than through D.2.2's context and XOR. The note this
decoder used to carry said both were "not a tier-1 change", and half of that
was wrong.

**The progression order change is a bound on the loops, not a second
sequencer.** A POC marker segment (A.6.6) is a list of progressions, and
B.12.2 says what each one is: B.12.1's "for loops" limited by (B-21)'s start
and end points — `CSpod <= i < CEpod`, `RSpod <= r < REpod`, `0 <= l < LEpod`
— with its own Table A.16 order. So tier-2 walks a list of *volumes*, and a
codestream with no POC is one volume covering everything, which is B.12.2's
own first sentence rather than a special case. Two rules of the clause are
easy to miss and both are implemented: every volume's layer loop starts at
zero and a packet already emitted is not emitted again (A.6.6 on `LYEpoc`),
which is what makes "the layer always starts with the next one"; and a tile's
own volumes may be spread across its tile-part headers (B.12.3's Figure
B.15b), joined in `TPsot` order, provided the first tile-part header carries
one.

*What a skipped POC costs is measured rather than argued.* T.800 J.10's own
two published packets, swapped and described by a two-volume POC, decode to
J.10.5's nine published samples; the identical bytes with the POC removed
decode **cleanly, with no warning**, to `128, 130, 132, 139, 128, 124, 143,
97, 153` instead of `101, 103, 104, 105, 96, 97, 96, 102, 109`
([`jpx_poc.rs`](../../crates/tinker-pdf-filters/tests/jpx_poc.rs)). That is
the "a wrong JPEG 2000 decode looks like a photograph" claim above, on the
standard's own bytes.

*One limit, named.* B.12.3 allows a POC to "describe more progression order
volumes than exist in the codestream" and lets "the last progression order
volume in each tile" be incomplete. A volume naming resolution levels or
components the tile does not have is clamped and contributes no packets, so
the common form of that is decoded. A codestream whose volumes describe more
*packets* than its tile data holds is refused on the exact-consumption check
— the same refusal a truncated codestream with no POC already earns here, so
this is one rule rather than two.

**Packed packet headers are one reader, not two.** PPM (A.7.4) moves every
tile's packet headers into the main header and PPT (A.7.5) moves one tile's
into its tile-part headers; B.10 names all three places a header can be. A
packed header is byte for byte the header that would have been in the bit
stream — A.7.4: "The contents are exactly the packet header which would
have been distributed in the bit stream as described in B.10" — so tier-2
parameterises *where the header bits come from* and leaves B.10's syntax in
one place. The bodies never move, so a packet's body still comes from the
bit stream; SOP stays with the body and EPH moves with the header, which is
A.8.1 and A.8.2 respectively. Both streams are then held to the same
exact-consumption check, and the seams the clauses name — each `Nppm` run's
end, each marker segment's end — must fall between two headers rather than
inside one.

**Measured over five corpora, September 2026**, by
[`jpx_attribution.rs`](../../crates/tinker-pdf/tests/jpx_attribution.rs), which
names the *internal* refusal rather than the coarse warning: **39 files carry a
JPX stream and 7 report a refusal, for 5 distinct reasons**, and **not one of
the five is a coding capability**. One file is a ruling 1 budget; two are
veraPDF fixtures whose `colr` box is deliberately non-conformant; two are
`/JPXDecode` streams whose bytes are not JPEG 2000 at all; and two are real
documents in which **no tile arrived whole**, which is the one truncation this
decoder refuses rather than draws around — a tile short of its declared parts
costs pixels and is drawn as far as it arrives, and only a codestream with no
complete tile has nothing to degrade to. So precision above sixteen bits —
the one coding-side entry left, and a limit rather than a gap — is reached by
**zero** corpus files, fixture or real, as `BYPASS` and `TERMALL` were until
they landed.

*The file count in that sentence read 5 until September 2026, against a test
that prints 7.* It was counting the five files whose reasons are not
truncation and taking the test's five **reasons** for its five **files**; the
two `a codestream with no complete tile` documents are refusals and are in the
census rows, where the old text had them among the truncations that decode.
The two numbers are now both given and both say which they are.

RGN, PPM and PPT were on the zero-reachability list until 20 September 2026,
POC until the 21st and `BYPASS` and `TERMALL` until the 23rd, and their leaving
moved nothing: the census was re-run after each and reports the same 39 bearing
files, the same 7 refusals and the same five reasons, because no corpus file
carried any of the six to begin with. Under ruling 3 that is a scheduling input
rather than a justification. The tier-2 roadmap row they belonged to is gone
with the last of them; what is left of the story is in that tier's preamble.

That paragraph used to read "the corpus's nineteen readable JPX files as of
August 2026: 16 decode and 3 refuse", which was a count over four corpora
taken before the attribution existed, and a coarse one.

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

**And a PNG *encoder*, the first of this crate's four image writers.**
`png_encode` takes an interleaved 8-bit raster and returns a whole file:
signature, IHDR, IDAT, IEND, each chunk's CRC over its type and its data. It
is here rather than in the tool that wanted it because a PNG file *is* the
three things that already lived here and nowhere else — a zlib stream (10.3 is
`zlib_compress`, RFC 1950), 9.2's row filters (`predictors.rs`, which PDF's
`/Predictor 15` adopted wholesale) and a chunk CRC-32 — so any other home
would have had to reach for all three or copy them.

All five of 9.2's filter types are emitted, chosen per row by **the minimum
sum of absolute differences** the specification itself recommends in 12.8,
with the filtered bytes read as *signed* (a byte of `0xFF` is a difference of
minus one, and the unsigned reading rejects exactly the rows a filter helped
most). Integer arithmetic throughout, ties to the lowest-numbered filter, so
the chosen filter and therefore every compressed byte is identical on every
target (ruling 4). Across PngSuite the tally is 115 rows on None, 679 on Sub,
1 313 on Up, 26 on Average and 2 813 on Paeth — measured, and asserted to be
non-zero for all five, because an encoder emitting type 0 everywhere would
round-trip perfectly and exercise 9.2's other four formulas on neither side.

Four colour types are written — 0, 2, 4 and 6 — at **eight bits a component**.
There is no indexed output, because a palette is a compression decision that
costs a second pass over the raster to sometimes save bytes and the row filters
already take the flat regions it would; no interlace, because Adam7 exists so
that a partial download shows a blurry whole image and nothing here writes to a
socket; and no 16-bit, because `tinker_pdf::Bitmap` — the thing the encoder
exists to serialise — stores a byte a channel. **A 16-bit PNG decoded and
re-encoded therefore comes back at 8-bit precision**, which is stated at the
assertion that compares them. All three are read by `png_decode`, which is the
direction that matters: a file somebody else wrote may be anything Table 11.1
permits; a file this engine writes is one of four things it chose.

One asymmetry is deliberate and worth knowing about: `png_encode` does **not**
charge `MAX_PNG_SAMPLES`. That cap is a reader's budget against a thirteen-byte
IHDR asking for 2^63 samples, and charging it on the way out would refuse a
legal picture — a page at `MAX_PAGE_PIXELS` (67.1 Mpx) is 268 million samples
as RGBA, four times the ceiling. So the largest page this engine renders writes
a PNG this crate's own decoder will not read back.

**Two bilevel encoders joined it on 15 September 2026**, both promoted from
code that already existed as a test fixture, and both shaped like `png_encode`:
a borrowed raster with an explicit stride, plain numbers beside it, a free
function returning `Result<Vec<u8>, _>`, and a refusal enum whose every variant
is a caller mis-describing its own buffer rather than damage in something read.

`ccitt_g4_encode` takes a `CcittSource` and writes **ITU-T T.6 two-dimensional
coding** — PDF's `/CCITTFaxDecode` at `/K` negative, and JBIG2's MMR (T.88
6.2.6) with `end_of_block` off. Pass, vertical and horizontal modes off T.6
§2.2.4's flow diagram; the full T.4 run tables, both make-up tables, Table 3b's
shared extension and the rule for a run past 2 623 pels; `/BlackIs1` either
way; EOFB on request. It writes G4 and nothing else — no `/K` zero, no `/K`
positive with its per-line tag, no TIFF compression 2 with byte-aligned rows,
no uncompressed mode — because each of those is a different framing around the
same row coder, G4 is smaller than all of them on every image, and a caller
that wants one should have to ask.

`jbig2_generic_encode` takes a `Jbig2GenericSource` and writes **T.88 6.2.5's
arithmetically coded generic region**: all four templates of Figures 8 to 11 at
any AT positions 7.4.6.3 can express, 6.2.5.7's typical prediction, over an
`MqEncoder` that stopped being `#[cfg(test)]` to carry it.
`jbig2_generic_region_segment` wraps that in 7.4.1's region information field
and 7.4.6.2's flags, which is one segment's *data* and not a JBIG2 file: no
page information segment, no file header, no D.3 embedded-stream assembly.
Symbol dictionaries, text regions, refinement, halftones, pattern dictionaries
and USESKIP are not written, each named in the module header with its reason.
MMR generic regions are not written *here* either, because `ccitt_g4_encode`
is the coder 6.2.6 defers to and a second copy would be a second thing to get
wrong.

**What adjudicates them is third-party data in both directions.** A round trip
through this crate's own decoder proves the two halves of one misunderstanding
agree, so it is never the claim:

- **ITU-T T.4 Tables 2, 3a, 3b and 4** — fetched 15 September 2026, read twice
  (once out of the text layer, once off the rendered pages at 170 dpi), and all
  195 run-length entries asserted against that transcription. No fixture in the
  tree has a run longer than 63, so this is the only guard on a make-up code.
- **ITU-T T.88 Annex H.1** publishes a 54 × 44 bitmap *and* the bytes its own
  encoder produced for it twice over: segment 4 is 26 bytes of MMR and segment
  11 is nine bytes of arithmetic generic region at template 0 with TPGDON.
  Encoding the published picture and comparing with the published bytes leaves
  an encoder nowhere to hide. Annex H.2's thirty bytes pin the MQ coder
  underneath independently.

One limit is recorded rather than glossed: **no published bitstream can pin the
context numbering.** A context index is a label into an array whose slots all
start identical, so any bijection of the numbering is invisible to a coder —
measured, by injecting two such permutations and watching Annex H.1 still match
byte for byte. T.88's Figures 8 to 11, transcribed pixel by pixel, are what pin
it, and they pin all four templates rather than the one the annex uses.

**Nothing in this repository calls either encoder outside the tests**, and the
writer's contract — it never re-encodes image bytes — is unchanged by their
existence ([creation](creation.md), [ROADMAP](../ROADMAP.md)).

**And a baseline JPEG encoder joined them on 16 September 2026**, which closes
the roadmap's image-encoder row with one gap named rather than papered over.
`jpeg_encode` takes a `JpegSource` — the same borrowed raster, explicit stride
and plain numbers as the other three — and writes **ITU-T T.81's baseline
process and only that**: sequential DCT, Huffman coding, 8-bit precision, one
SOF0 frame, one scan, `Ss = 0`, `Se = 63`, `Ah = Al = 0`, SOI to EOI with no
abbreviated form. Everything else T.81 defines is excluded rather than
half-built, each named in the module header with its reason: progressive and
extended sequential, arithmetic (Annex D's QM coder — which the decoder beside
it **no longer** refuses, and writing one would need an encoder-side model this
does not have), lossless, hierarchical and differential, 12-bit precision,
four-component CMYK and YCCK — whose meaning comes from Adobe's APP14 and not
from T.81 or T.871, so writing one would be inventing a convention — and K.2's
procedure for optimising a Huffman table from an image's own statistics.

Three things had to be settled, and each is settled against a published clause
rather than against what other encoders do:

- **Colour.** T.81 A.1 specifies no colour space at all; it codes numbered
  components. The published outside is **ITU-T T.871 clause 7**, the same
  YCbCr the decoder inverts, so exactly two source colours are written — one
  component with no transform, or three through clause 7's **exact** forward
  equations with the JFIF APP0 clause 10.1 requires, so a reader is *told*
  which YCbCr it is rather than guessing from the component count.
- **Sampling factors are named by the caller and never inferred.** 4:4:4 or
  4:2:0, defaulting to 4:4:4. T.81 recommends no factors; choosing them from
  the pixels would make the output depend on image content in a way the caller
  cannot predict; and subsampling is a lossy colour decision, so a caller who
  did not ask for it should not silently get chroma back through a box filter
  and a replicate.
- **Quantisation, and the caller who wants a quality number.** T.81 publishes
  exactly two settings — Tables K.1 and K.2, and K.1's own "if these
  quantization values are divided by 2" — and this offers exactly those two
  plus the caller's own tables. There is deliberately **no `quality: u8` from
  1 to 100**: every such scale in circulation is some program's private
  convention, none of them is in T.81, T.83 or T.871, and shipping one would
  put a number in this crate's public API that no published document
  adjudicates. What the format carries is the table, so a caller who wants a
  specific quality supplies the table and gets exactly it.

**An image whose dimensions are not a multiple of the MCU size** is completed
by **A.2.4 and its NOTE**: the right-most column and the bottom line are
replicated into the partial MCU, and A.2.4's last sentence — "any sample added
by an encoding process to complete partial MCUs shall be removed by the
decoding process" — is why nothing else is needed. Zero-fill would put a step
of up to 128 levels inside the edge block, costing bits in every AC coefficient
and ringing back across the boundary into pixels the caller can see; mirroring
costs the same as replication and is a different picture from the one the
standard recommends, for no gain.

**What adjudicates it, and — said plainly — what does not.** `T-REC-T.81` (the
W3C's copy of CCITT Rec. T.81 (1992), ISO/IEC 10918-1 : 1993) was fetched and
read **twice**, once out of the text layer and once off `tpdf render`'s pages at
200 dpi, because T.81's tables are typeset with column rules that the text layer
emits as a literal `1`: Table K.1's first row arrives as `16111016124140151161`
and is only resolvable into `16 11 10 16 24 40 51 61` against the picture. The
two readings agree on all 604 entries behind Figure A.6's zig-zag, Tables K.1
and K.2, and K.3.3's four byte lists. Two of those readings then appear in the
**output**: every DHT segment this writes is K.3.3's published byte list
verbatim, because B.2.4.2's payload after `Tc`/`Th` is exactly BITS then
HUFFVAL; and two whole entropy-coded segments are derived from Tables K.3's and
K.5's *printed code words* with no implementation in the middle — a flat
mid-grey block is `DIFF = 0` then EOB, which K.3 category 0 (`00`) and K.5 0/0
(`1010`) fix at `001010`, padded per B.1.1.5 NOTE 1 to `X'2B'`.

**But no published DCT vector set adjudicates the coefficients.** ITU-T T.83
(ISO/IEC 10918-2) is the compliance data published for exactly this, and its own
clause 4.4 says the data ships *on three diskettes* accompanying the document
rather than inside it. The ITU's copy returns HTTP 500 and the Recommendation's own
page says it "is only available through payment"; ISO's returns HTTP 403; the
one reachable copy is the standards-preview extract, which carries the numbered
pages 1 to 11 and stops mid-sentence in clause 5.2.1 — and whose own contents
list puts clause 6's encoder compliance tests on p. 19, Annex B's compliance
quantisation tables on p. 28 and Annex C's compressed test data on p. 30, every
one of them past where it stops. The Internet Archive holds nothing. So the
forward transform is held to **A.3.3's equation recomputed in `f64` in the
test** — the standard's formula, not the standard's numbers — and that is what
the test's own doc comment says. Nor does "held to this crate's decoder" help:
that decoder is adjudicated by nothing third-party either — every fixture in
`jpeg.rs` is built by that module's own bit writer, and `jpeg_census.rs` counts
frame types without comparing a pixel — so the round-trip tests here say "round
trip" and claim nothing else.

**Nothing in this repository calls `jpeg_encode` outside the tests**, on the
same contract as the two bilevel encoders.

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
`JpxImage`). The encoder half is `deflate`, `zlib_compress`, `png_encode`
(takes a `PngSource`, returns the file or a `PngEncodeError`), `ccitt_g4_encode`
(takes a `CcittSource`, returns the coded bits or a `CcittEncodeError`),
`jbig2_generic_encode` / `jbig2_generic_region_segment` (take a
`Jbig2GenericSource`, return the MQ bytes or the whole segment data, or a
`Jbig2EncodeError`), with `MqEncoder` public beneath those two, and
`jpeg_encode` (takes a `JpegSource` and a `JpegOptions`, returns the whole
interchange datastream or a `JpegEncodeError`); the container
half is `png_decode`, `png_scan`, `tiff_decode`, `tiff_scan`, `packbits_decode`,
`inflate_raw`, `crc32` and `jxr_decode` (returns `JxrImage`).

Every one of those six takes plain numbers and a borrowed byte slice and
returns bytes, which is all ruling 8 asks of a leaf. The parameter names
`columns`, `rows`, `black_is_1` and `end_of_block` are PDF's, and deliberately:
`CcittParams` has been public with those names since the decoder landed, and one
name per concept in a crate beats two.

`png_encode` is the one filter entry point with a visible counterpart on the
facade, and the shape is a projection rather than a re-export: ruling 11 keeps
`tinker_pdf` the public surface for a *document*, and what a caller has is a
rendered page, so `tinker_pdf::Bitmap::to_png` maps a `PixelFormat` onto one of
PNG's colour types and calls this. Two of the six formats have no colour type
to map onto and are converted there rather than here — `CmykA8` through
8.6.4.4's device relation and `LabA8` back out of `L*a*b*` — because this crate
holds no PDF colour and ruling 8 keeps it that way.

`ccitt_g4_encode`, `jbig2_generic_encode` and `jpeg_encode` have **no** facade
counterpart, and
that is the same ruling read the same way rather than an omission. Ruling 11
makes `tinker_pdf` the surface for a *document*; a bilevel raster is not one,
and nothing the facade hands a caller is one. `Bitmap::to_png` exists because a
caller of the facade holds a rendered page; there is no equivalent thing to
project here, and inventing an entry point to satisfy a ruling that does not ask
for one would be the actual mistake. A caller who wants G4, JBIG2 or JPEG bytes
depends on the leaf crate, which is what leaf crates are for. If a facade caller
ever wants a JPEG *of a rendered page*, that is a projection beside
`Bitmap::to_png` and not a re-export of this one.

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
| JBIG2 reference that does not resolve: a retained bitmap-coding context (7.4.2) no referred-to segment left behind, a retained context of the wrong shape for the template consuming it, or a selector naming a custom table the segment did not refer to | `Warning::Jbig2VariantSkipped` | **Every one of these is now a file contradicting itself rather than a lineage nobody has started.** SDHUFF, SBHUFF, SDREFAGG, SBREFINE, segment types 40/42/43, `TRANSPOSED`, clause 7.4.13's custom code tables and 7.4.2's retained contexts have all left this row. **The whole symbol lineage decodes** — arithmetic and Huffman, with and without refinement, in either combination, either placement orientation, standard tables or the file's own, adaptive state carried across dictionaries or started fresh, and a non-zero refinement delta through **B.14 and B.15** on both the text region's road and the dictionary's | T.88 7.4.2, 7.4.3.1.6 |
| JBIG2 text region whose referred-to dictionary is absent or refused | `Warning::Jbig2VariantSkipped` | 7.4.3 numbers symbols across every referred-to dictionary, so drawing it renumbered says something else — refused whole instead | T.88 7.4.3 |
| JBIG2 dictionary past its symbol or instance budget | `Warning::Jbig2SymbolLimitHit` | `SDNUMNEWSYMS`, `SDNUMEXSYMS` and `SBNUMINSTANCES` are attacker-controlled 32-bit counts; capped before allocation (ruling 1) | [rulings](../rulings.md) |
| JBIG2 region or page above the output ceiling | `Warning::Jbig2RegionTooLarge` | Width and height are attacker-controlled 32-bit values; refused before allocation (ruling 1) | [rulings](../rulings.md) |
| JPX markers SOP and EPH in a header (T.800 A.8) | `Warning::JpxMarkerUnsupported` | **No Table A.2 marker is refused as a capability any more, and these two are refused for where they are rather than for what they are.** A.8 puts both inside the bit stream and tier-2 reads them there; a header is the one place neither has a meaning. **Five markers have left this row, each for its own reason**: CRG, because A.9.1 says it "has no effect on decoding the codestream", so it is parsed, carried and not applied; RGN, because Annex H was implemented; PPM and PPT, because A.7.4 and A.7.5 were; and POC last, on 21 September 2026, because A.6.6's progressions are B.12.2's progression order volumes and tier-2 sequences the packets from them | T.800 A.8.1, A.8.2 |
| JPX POC field values: a `Ppoc` Table A.16 does not define, a bound outside Table A.32, an `Lpoc` that is not equation (A-6)'s, two POC segments in one header, a tile-part POC with none in the tile's first tile-part header | `Warning::JpxFeatureUnsupported`, `Warning::JpxStructureInvalid` | What is left of POC after the marker was implemented, and the same shape RGN's refusal took: a *value inside* the segment rather than the segment. A volume whose bounds run backwards is not a volume, and clamping one into shape would decode a packet sequence the codestream never described — the same failure as skipping the marker, reached from the other side | T.800 A.6.6, Table A.32, B.12.3 |
| JPX ROI style: an `Srgn` T.800 Table A.25 reserves | `Warning::JpxFeatureUnsupported` | Table A.25 defines one ROI style — 0, "Implicit ROI (maximum shift)" — and reserves the rest. A reserved style is some other realignment of the coefficients, so running H.1's Maxshift arithmetic over it would put the background at the wrong magnitude and draw a plausible picture. Refused by name rather than stepped over, which is the SOF3/SOF5/SOF6/SOF7 lesson on this page | T.800 A.6.3, Table A.25 |
| JPX markers Table A.2 does not define (all of ISO/IEC 15444-2) | `Warning::JpxMarkerUnknown` | Part 2 is a non-goal; an unknown marker cannot be measured past | [ROADMAP](../ROADMAP.md) |
| JPX coding features: a code-block style **bit** T.800 Table A.19 does not define, an unmappable `colr`, unequal channel depths | `Warning::JpxFeatureUnsupported` | A wrong JPEG 2000 decode is a plausible photograph; refusal beats a blur nobody can distinguish from a bad scan. **Table A.19 has left this row as a capability**: all six code-block styles decode as of 23 September 2026, and what fires for A.19 now is bit 6 or bit 7, which the table reserves — a *value* the standard does not define rather than a capability this build lacks, the same shape as the `Srgn` row above | [ROADMAP](../ROADMAP.md) |
| JPX component precision above 16 bits | `Warning::JpxPrecisionUnsupported` | **A limit, not a gap.** T.800 Table A.11 allows 38; E.1 clamps a coefficient to `2^(R_b + 2)` sample units and a coefficient plane is a Q12 `i32`, so 17 bits is where the plane format runs out — and ISO 32000-1 Table 89 has no `/BitsPerComponent` above 16 to hand a widened sample to. Argued in ROADMAP's Named non-goals | [ROADMAP](../ROADMAP.md) Named non-goals |
| JPX tile-parts out of order, or a codestream with no complete tile | `Warning::JpxStructureInvalid` | Out of order is a codestream contradicting itself, and reassembling in stream order would produce a picture wrong in a way that looks like compression. A tile *short* of its declared parts is a different failure — a file that stopped — so it is left blank and reported as `JpxTruncated` wherever any tile survives, which is `JxrWarning::TileDroppedAsZero`'s bargain; only a codestream with no whole tile at all is refused | [ROADMAP](../ROADMAP.md) |
| JPX work/sample/code-block budgets spent | `Warning::JpxBudgetSpent` | The budgets are totals, never refunded — a per-item cap is not a work cap once the structure branches (ruling 1) | [rulings](../rulings.md) |
| JPEG lossless frames: SOF3, SOF7, SOF11, SOF15 | `JpegError::Lossless` | Annex H's predictive coder shares nothing with the DCT path — no quantisation, no blocks, no transform. Both entropy coders are on this row because the predictor is what is missing either way: `qm.rs` decodes SOF11's and SOF15's decisions perfectly well with nothing to hand them to. Zero in the corpus | [ROADMAP](../ROADMAP.md) |
| JPEG differential frames: SOF5, SOF6, SOF13, SOF14 | `JpegError::Differential` | Annex J's hierarchical progression, where a frame codes the difference from an upsampled earlier one. Same shape as the row above: the entropy coder is not the gap. Zero in the corpus | [ROADMAP](../ROADMAP.md) |
| JPEG precision other than 8 or 12 bits | `JpegError::UnsupportedPrecision` | B.2.2 allows 8 in a baseline frame and 8 or 12 elsewhere; anything else is a header this build will not guess at | [ROADMAP](../ROADMAP.md) |
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
- `crates/tinker-pdf-filters/tests/jpx_annex_j.rs` — T.800 Annex J.10's
  published 100-byte codestream, decoded to the nine samples J.10.5 states,
  and **the same codestream with its packet headers relocated into a PPM and
  into a PPT**. That second pair is evidence rather than self-agreement
  because J.10.3 and J.10.4 publish where each packet's header ends: Table
  J.20 lists the first header's three bytes, J.10.4 gives its body's offset
  as octal 0125, Table J.21 lists the second header's four bytes and J.10.4
  gives its body's offset as octal 0137. The relocation moves published bytes
  across a published boundary and is judged by published samples; only the
  marker segments around them are this repository's, and they are asserted
  against Tables A.38 and A.39 field by field before any decode runs. The
  file also records a J.10 **erratum**: J.10.3's prose says the second packet
  header begins at octal 0134, while Table J.21 immediately below it lists
  `0xC0`, which is the byte at 0133 — and J.10.4's 0137 for the second body
  agrees with 0133.
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
- `crates/tinker-pdf-filters/tests/jpx_annex_j.rs` and
  `jpx_annex_h.rs` — **the two JPX checks the standard itself
  adjudicates.** T.800 Annex J.10 publishes a complete 100-byte codestream,
  annotated field by field, with its intermediate coefficients in J.10.4 and
  its nine decoded samples in J.10.5; `jpx_annex_j.rs` transcribes the
  codestream from the annotated field listings rather than from the hex dump
  and asserts each named field sits at J.10's own octal offset *before* any
  decode runs. `jpx_annex_h.rs` builds Annex H's region of interest on top of
  it: T.800 publishes no ROI test data anywhere, but H.1 rewrites exactly the
  coefficients J.10.4 prints, so the fixtures are J.10's codestream with
  `SPqcd` exponents lowered and an RGN inserted, and H.1's own arithmetic
  says what comes out. The link from coefficients to samples is checked by a
  second transcription of (F-3) to (F-6) and G.1 living in that file, which
  the standard adjudicates — it must reproduce J.10.5 from J.10.4 — before it
  is used to predict anything.
- `crates/tinker-pdf-filters/tests/png_suite.rs` — the PNG decoder **and the
  encoder** against PngSuite, 176 files in the 2017jul19 release, re-fetched
  and run in September 2026: all fifteen legal
  colour-type/depth pairs, and fourteen broken-by-design files that must each
  be refused. Runs when `TINKER_PNGSUITE` points at the set, and prints
  `RAN`/`SKIPPED` so a missing corpus never reads as a pass.

  The encoder's two legs are there because one of them is not enough.
  **Leg one** decodes each of the 162 readable files, encodes the raster,
  decodes that and requires the same pixels — which means the raster going in
  was produced by an encoder nobody here wrote, at every legal pairing,
  interlaced and not. **Leg two** transcribes 9.2's five *reconstruction*
  formulas into the test and rebuilds the raster from the encoder's own IDAT
  without calling `png_decode` at all. Leg two exists because `png/encode.rs`
  filters with the same `predictors.rs` Paeth predictor `png.rs` unfilters
  with, so a defect in it moves both directions together and leg one stays
  green; the injection matrix measures exactly that, and reversing the
  tie-break is the case — **all 162 round trips pass**, and what catches it is
  leg two plus the two decoder tests that compare two third-party files against
  each other rather than against our own output (the Adam7 twins and the
  published equivalence classes). Three of 2 014, not one of them a round trip.
  The container is asserted against clause 5 rather than against the
  reader: the signature, IHDR first with 11.2.2's thirteen bytes, IEND last and
  empty, every chunk's CRC recomputed over its type and its data, and the IDAT
  payload equal to `zlib_compress` of the filtered stream.
- `crates/tinker-pdf/tests/png_output.rs` — `Bitmap::to_png` over all six
  `PixelFormat`s, which is the half PngSuite cannot see because PngSuite has no
  `Bitmap`. It owns the format mapping: the colour type in IHDR for each, alpha
  surviving `Rgba8` and `GrayA8`, a padded stride not being written as pixels,
  and the two converted formats named by colour rather than by comparison —
  pure cyan is `(0, 255, 255)` because 8.6.4.4 says so, and sRGB's green
  primary at its published CIELAB coordinates comes back green rather than the
  mauve its encoded bytes are when read as RGB.
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
  fixture, because the coder serves two codecs; **`qm.rs` holds T.81 K.4.1's,
  in both directions** — the published bytes decode to the published decisions
  and the published decisions encode to the published bytes — and that fixture
  walks **26 of Table D.3's 113 rows**, counted rather than claimed, so what it
  does *not* reach is visible too; `src/jpx/tests/refusals.rs` reaches every
  entry of the JPX refusal list, so "the refusals are the feature" is checked,
  not claimed.
- Fuzzing: nine of the 25 fuzz targets exercise this crate —
  `ascii_filters`, `lzw`, `inflate`, `ccitt`, `jpeg`, `jbig2`, `jpx`, `png`,
  `tiff`. The last is the first target that reaches five other decoders
  through one parser, because a two-byte `Compression` field is what chooses
  between them, and it carries six committed seeds written by an `#[ignore]`d
  test in this crate so the seeds and the fixtures cannot drift.
- Downstream: the `image`, `jbig2` and `jpx` render fingerprints among the
  15 in `crates/tinker-pdf/tests/determinism.rs` pin decoded pixels
  bit-for-bit across targets ([determinism](determinism.md)), and the whole
  workspace stands at 4 903 passed / 0 failed / 59 ignored
  (Windows x86_64, 20 September 2026). See [verification](../verification.md) for
  the full harness.
