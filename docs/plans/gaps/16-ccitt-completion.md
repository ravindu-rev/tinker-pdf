# CCITT ignores two parameters and returns the wrong shape

The fax decoder handles T.4 and T.6 codes correctly and then gets the
surrounding contract wrong in three ways: two `/DecodeParms` entries are never
consulted, `/K > 0` is not really mixed mode, and the output is one byte per
pixel where the image dictionary says one *bit*. When this is done, a scanned
fax decodes the way its parameters describe it. (M)

## What is wrong

**`/EndOfLine` and `/EndOfBlock` are not consulted.** The EOL and EOFB codes
are recognised wherever they appear, which is lenient and right, but the
parameters that say whether they are *expected* are ignored. `/EndOfBlock
false` means the decoder must stop at `/Rows` rather than at an EOFB it will
never see.

**`/K > 0` is not true mixed mode.** In T.4 mixed mode each row carries a tag
bit saying whether it is one- or two-dimensionally coded. The decoder reads
that bit at the top of every row whether or not an EOL preceded it — so every
row that is not EOL-separated loses a data bit, and everything after it in
that row decodes to noise.

**The output is 8× too wide.** `ccitt_decode` returns one byte per pixel. The
image dictionary says `/BitsPerComponent 1`, and the caller in
`crates/tinker-pdf/src/resources.rs` compensates by treating the result as
greyscale bytes — which means `/ImageMask`, `/Decode` and `/ColorSpace` never
apply to a fax, because it never reaches the generic sample path.

## Scope

- Read `/EndOfLine` and `/EndOfBlock` and act on both.
- Correct `/K > 0`: the tag bit is present only where the encoding puts it.
- Emit packed 1-bit-per-pixel output.
- Rewire `resources.rs` so a decoded fax goes through the generic sample path,
  gaining `/ImageMask`, `/Decode` and `/ColorSpace` with it.

## Non-goals

- **T.6 changes.** `/K < 0` is correct.
- **Encoding CCITT.** Nothing needs to write a fax.

## Design

**The decoder flip and the caller rewire must land in one commit.** There is
no end-to-end CCITT test in the repository. Change the decoder's output shape
without changing the caller and every fax renders as a photographic negative
at one eighth width — with a green suite, because nothing tests the pair.
That is the whole risk in this document, and it is why the milestones below
put them together rather than in sequence.

Packed output also makes the polarity explicit. CCITT's natural sense is
0 = white; a 1-bit DeviceGray image has 0 = black. Today the byte-per-pixel
conversion buries that inversion inside the decoder. Once the samples reach
the generic path, `/BlackIs1` and `/Decode` compose the way the specification
says they do, and the polarity is visible rather than folded in.

## Where a half-implementation is worse than none

Shipping the packed output without the rewire, as above: a negative image at
the wrong width, silently, with tests passing. Shipping the rewire without the
packed output is merely broken and obvious.

The mixed-mode fix has its own version: correcting the tag bit for
EOL-separated rows only would fix the common case and leave the uncommon one
decoding noise from a slightly different offset — harder to spot than the
current uniform failure.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | An end-to-end CCITT fixture, before any change | A hand-built G4 image renders to known pixels through `Document::render` — the test that makes everything below safe | S |
| 2 | Packed 1-bpp output **and** the `resources.rs` rewire, one commit | The fixture from 1 renders identically; `/ImageMask`, `/Decode [1 0]` and an explicit `/ColorSpace` now each change the output | M |
| 3 | `/EndOfLine` and `/EndOfBlock` honoured | `/EndOfBlock false` with trailing bytes stops at `/Rows`; `/EndOfLine true` with a missing EOL is reported rather than silently absorbed | S |
| 4 | True T.4 mixed mode for `/K > 0` | A mixed-mode fixture whose rows are not EOL-separated decodes correctly, where it previously produced noise after the first row | M |
| 5 | Fuzz | `cargo fuzz run ccitt` survives a session with the new paths reachable | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** correct rendering of scanned documents, which is a large share
of any real corpus. Also a prerequisite for [17](17-jbig2-generic-region.md),
whose MMR path reuses the T.6 decoder.

## Risks

| Risk | Mitigation |
| --- | --- |
| No end-to-end test exists, so any change to the output contract is unverified | Milestone 1 is that test, written first and deliberately before the change it protects |
| Polarity inverts silently — a negative fax is a plausible image | The fixture asserts specific black and white pixels by position, not merely that the image is not blank |
| Row-length recovery on a damaged row changes with the packed representation | The existing behaviour — repeat the row above — is preserved and asserted in the fixture |
