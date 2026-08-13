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

## As built — August 2026

Five commits, one per milestone. All three defects were live, and reading the
call site turned up a fourth the plan does not describe: the warnings the
decoder produced were discarded on the floor -- `let (gray, _) =
ccitt_decode(...)` -- so every leniency it took was invisible. They are now
collected per image and reported through `damaged_images`, which ruling 10
asks for.

**The fixture came first**, in `72e127c`, before anything changed. It asserts
specific black and white pixels by position rather than that the page is not
blank, because a negative fax is a plausible image, and it pins the
damaged-row recovery -- repeat the row above -- which had to survive the
representation change.

**Packed output and the rewire landed together**, in `5d4dd9e`, as the plan
insisted. `decode`'s signature did not change, only the meaning of the bytes
it returns, so a caller left on the old contract would have rendered every fax
as a negative at one eighth width with nothing in the repository failing.

The consequence worth naming: a fax used to be the one kind of image that
could not be a stencil, because the branch hardcoded `stencil: false` and
returned before the generic sample path. That is the kind of image a fax most
often is. `/ImageMask`, `/Decode` and `/ColorSpace` now all reach one.

### Two seams later gaps depend on

**`ccitt_samples(...)` in `crates/tinker-pdf/src/resources.rs`** is the single
entry point. [08](08-inline-image-filters.md) routes the inline-image CCITT
path into *this function* rather than building a parallel call site — that is
why 08 was reordered to run after this gap, since a second caller on the old
contract would be invisible to the compiler.

**`T6Rows::new(data: &[u8], bit_offset: usize, columns: u32)`** in
`crates/tinker-pdf-filters/src/ccitt.rs` decodes two-dimensional rows resumably
from an arbitrary bit offset, with `next_row(&mut [u8]) -> bool`,
`row_bytes()` and `bit_position()`. [17](17-jbig2-generic-region.md)'s MMR path
uses it instead of duplicating T.6. It shares `decode_row` with the
whole-stream `decode`, so the mode codes keep one implementation and one set of
tests.

### Three rulings taken where the specifications do not agree

**`/Rows` stays a ceiling whatever `/EndOfBlock` says.** Table 11's wording is
that an EOFB terminates the image "overriding the Rows parameter", and read at
its most literal that lets a stream with trailing bytes and no EOFB decode past
its own declared height, bounded only by the output cap — a quarter of a
gigabyte of rows a caller who knows the image is `/Height` tall will discard.
Ruling 1 asks for bounded work on untrusted input, so the override runs the
other direction: an EOFB *before* `/Rows` ends the image early, which is the
case the sentence is actually describing. A pre-existing bounds test caught the
literal reading.

**The mixed-mode tag bit follows T.4, not Table 11.** ISO 32000 Table 11 says
the tag "shall precede each encoded line"; ITU-T T.4 4.2.1.3.1 attaches it to
the EOL. The two diverge only when EOLs are absent, which PDF permits and T.4
does not. This plan rules for T.4 and CONTRIBUTING rule 4 makes the plan
binding, but the repository has no `/K > 0` fixture from the wild, so a corpus
counter-example is the thing to re-litigate it with — [23](23-corpus-runner.md)
is where one would come from.

**An untagged row keeps the mode last announced.** Neither specification
answers this, because neither admits an untagged row. It reduces to
one-dimensional for a stream that never announces otherwise, which is
4.2.1.3.1's rule for the first line of a page.

### What the mixed-mode fix was worth

Measured before and after on four rows with one EOL at the head and none
after: `[0xF0, 0xF0, 0xFF, 0xFF]` became `[0xF0, 0xC0, 0xFC, 0xFF]` — row 0
right, rows 1 and 2 noise, row 3 accidentally right. With no EOL anywhere the
old decoder produced `[0xFF, 0xFF, 0xFF, 0xFF]` and a spurious `TruncatedInput`
— a blank white page.

The test that looks like the guard is not the evidence.
`mixed_mode_rows_are_coded_the_way_their_tag_bit_says` **passes against the
old behaviour**, because with an EOL before every row both readings consume the
same bit; only the fixtures whose rows are unseparated catch it. Its doc
comment says so, so nobody later mistakes one for the other.

RTC fell out of the same seam: in mixed mode it is six EOL-and-tag pairs, so
the second EOL is only visible after the first tag is consumed. It used to be
decoded as a row, fail, and report a truncation that had not happened.

### The session

2 189 612 executions in 301 s under WSL2/Ubuntu-24.04 and cargo-fuzz 0.13.2,
7 274 exec/s, peak RSS 497 MB, no crash and no artifact. Coverage over the
committed corpus alone: `T6Rows::new`, `bit_position` and `row_bytes` at 100
per cent of regions, `next_row` at 96.30, `decode_row` at 84.56 — so the paths
these five commits added were reached rather than merely present. The target
now drives both entry points and asserts three invariants rather than only not
crashing: no partial row comes back, a row cannot decode out of no bits, and
nothing is written past a row's own width.

Not done: no CCITT fixture joined `tests/determinism.rs`. The seven there are
unmoved and none decodes a fax, so this gap's pixels are covered by the
end-to-end tests in `crates/tinker-pdf/tests/ccitt.rs` rather than by a
fingerprint. Gap 25's milestone 4 remains owed by 09, 10 and 11.
