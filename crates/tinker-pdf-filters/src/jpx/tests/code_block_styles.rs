//! Table A.19's last two code-block styles, against what T.800 publishes.
//!
//! `BYPASS` (bit 0, D.6) and `TERMALL` (bit 2, D.4) landed together. They are
//! **two capabilities over one mechanism**: both put more than one codeword
//! segment in a code-block's contribution to a packet (B.10.7.2), and only
//! `BYPASS` adds a second way of reading a coding pass. [`super::super::passes`]
//! carries the split.
//!
//! # Where the standard's evidence stops and ours begins
//!
//! Ruling 13: nothing outside this repository adjudicates. So the question
//! for each link is whether T.800 states the answer, and it is worth being
//! exact, because the answer differs from link to link.
//!
//! **Adjudicated by T.800, with its own bytes in and its own numbers out:**
//!
//! - *B.10.7.1's length coding.* NOTE 1 prints four successive layers'
//!   lengths, their pass counts and a valid bit sequence for all four,
//!   including two `Lblock` increments. [`b10_7_1s_worked_example`].
//! - *B.10.7.2's multiple codeword segments.* Its NOTE prints a code-block's
//!   five included passes, the set `T` they produce **under D.6's bypass**,
//!   the four lengths that are signalled and a valid bit sequence coding
//!   them. [`b10_7_2s_worked_example`] feeds that bit sequence to the shipped
//!   reader and requires the standard's four lengths back, with `T` derived
//!   from this build's Table D.9 transcription rather than written in.
//! - *D.6's boundary, from below.* J.10's second code-block carries seven
//!   coding passes, and D.6 makes no pass before the tenth raw. So J.10's
//!   published second packet header, read **with `BYPASS` set**, must still
//!   signal Table J.21's one length of three bytes, and those three published
//!   bytes must still give J.10.4's published "1, 5, 1, 0".
//!   [`j10s_second_packet_is_unmoved_by_bypass`].
//! - *D.6's boundary, from above.* J.10's first code-block carries sixteen,
//!   so under `BYPASS` B.10.7.2 wants five lengths where J.10 prints one, and
//!   the published three-byte header stops being readable.
//!   [`j10s_first_packet_header_is_not_a_bypass_header`].
//!
//! **Transcribed and not adjudicated**, said plainly because the roadmap row
//! this closes is the honest-entry row:
//!
//! - *The exact value of D.6's boundary.* The two J.10 tests bracket it into
//!   7..=15 — below 7 a pass of the seven-pass block would terminate or go
//!   raw, from 16 up the sixteen-pass block's header would read as one
//!   length. Ten is transcribed from D.6 and Table D.9, and inside that range
//!   nothing in T.800 that this repository can run distinguishes it.
//! - *Raw pass decoding itself.* T.800 publishes no codestream with a raw
//!   pass in it — J.10's is the only codestream in the 231 pages and its COD
//!   sets none of Table A.19's bits. The bit-unstuffing rule, D.4.1's 0xFF
//!   extension, (D-2)'s raw sign and the pass schedule are transcribed from
//!   D.6 and asserted as data. Two tests then run them:
//!   [`a_raw_pass_takes_every_decision_and_its_sign_from_the_stream`] decodes
//!   a raw pair on top of J.10's own six bytes and requires what D.4.1 and
//!   (D-2) *say* should come out, which is the clause checked against the
//!   code rather than the standard checked against it, and
//!   [`a_segment_past_the_boundary_is_read_raw`] only checks that the flag
//!   arrives at all. Nothing here round-trips an encoder we wrote through a
//!   decoder we wrote, because that would prove nothing about the standard.
//! - *`TERMALL`'s per-segment re-initialisation.* Same: Table D.8 publishes
//!   the pattern and no bytes. A `TERMALL` codestream cannot be made out of
//!   J.10's by flipping its style byte, and the reason is measurable rather
//!   than assumed — [`termall_cannot_be_flipped_onto_j10`] states it. What
//!   `TERMALL` does share with `BYPASS` is [`read_lengths`], and that
//!   function is adjudicated by B.10.7.2's worked example. That a termination
//!   re-initialises the decoder and **not** Table D.7's context states is
//!   [`a_codeword_segment_boundary_does_not_reset_the_context_states`], which
//!   is a statement about two Table A.19 bits staying two.
//! - *B.10.7.2's multi-layer merge.* A signalled length is not a codeword
//!   segment, and only a codestream with more than one layer reaches the
//!   difference. T.800 states the rule in prose and publishes no bytes for
//!   it, so [`a_contribution_split_across_two_layers_decodes_as_one`] builds
//!   both codestreams here and claims only that layering is transparent.
//!
//! # What the injection campaign asked for
//!
//! Sixteen defects were put back one at a time. Twelve fired against the
//! tests above; **four fired nothing**, and all four were about paths a
//! single-layer, single-segment fixture cannot reach — the merge rule, the
//! schedule's pass numbering, (D-2)'s raw sign and the context states at a
//! segment boundary. The last three tests in this file exist because of that
//! result, and it is recorded here because a plausible defect that fires
//! nothing is a fact about the suite rather than about the defect.
//!
//! # Why this file is in-crate
//!
//! The pattern it copies is `tests/jpx_annex_j.rs`, `tests/jpx_annex_h.rs`
//! and `tests/jpx_poc.rs`: transcribe from the clause's own field listings,
//! assert every field before any decode runs, and say which link the standard
//! adjudicates. What it cannot copy is their *location*. The evidence T.800
//! offers for these two capabilities is a packet header's bit sequence and
//! one code-block's coefficients, and both sit below `jpx_decode`. Putting
//! the tests where the evidence is keeps them from having to approach a
//! five-byte bit string through a whole container.

use crate::jpx::codestream::cb_style;
use crate::jpx::jpx_decode;
use crate::jpx::passes::Schedule;
use crate::jpx::tier1::{decode_code_block, initial_contexts, CodingStyle};
use crate::jpx::tier2::{
    read_lengths, read_pass_count, Orientation, PacketBits, Segment, SignalledLength,
};
use crate::Limits;

use super::writer::Spec;

// --- B.10.7.1 and B.10.7.2's worked examples ----------------------------

/// Packs a bit string written as `'0'`/`'1'` (spaces ignored) into bytes,
/// most significant bit first, zero-padding the last byte.
///
/// The standard prints its sequences in groups of four, so they are kept in
/// that shape and grouped here rather than pre-packed: a hand-packed byte is
/// a transcription with an extra step in it, and the extra step is where the
/// mistake goes.
fn pack(bits: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut byte = 0u8;
    let mut n = 0u32;
    for c in bits.chars().filter(|c| *c == '0' || *c == '1') {
        byte = (byte << 1) | u8::from(c == '1');
        n += 1;
        if n % 8 == 0 {
            out.push(byte);
            byte = 0;
        }
    }
    if n % 8 != 0 {
        out.push(byte << (8 - n % 8));
    }
    out
}

/// B.10.7.1's NOTE 1, all four layers, as the standard prints it.
///
/// "For example, say that in successive layers a code-block has 6 bytes, 31
/// bytes, 44 bytes and 134 bytes respectively. Further assume that the number
/// of coding passes is 1, 9, 2 and 5. The code for each would be 0 110 (0
/// delimits and 110 = 6), 0011111 (0 delimits, log2 9 = 3 bits for the 9
/// coding passes, 011111 = 31), 11 0 101100 (110 adds two bits to Lblock,
/// log2 2 = 1, 101100 = 44), and 1 0 10000110 (10 adds one bit to Lblock,
/// log2 5 = 2, 10000110 = 134)."
///
/// This is the single-codeword-segment path, which every codestream with
/// neither Table A.19 bit uses — so it is a check on what was already here as
/// much as on what is new, and it is here because the reader it addresses was
/// rewritten to carry B.10.7.2 as well.
#[test]
fn b10_7_1s_worked_example() {
    // (the standard's bit sequence, its coding passes, its length, the
    // Lblock the standard says it leaves behind)
    let layers: &[(&str, u32, usize, u32)] = &[
        ("0 110", 1, 6, 3),
        ("0 011111", 9, 31, 3),
        ("11 0 101100", 2, 44, 5),
        ("1 0 10000110", 5, 134, 6),
    ];

    // B.10.7.1: "The value of Lblock is initially set to three."
    let mut lblock = 3;
    // No Table A.19 bit: Tables D.8's left column, so no pass terminates and
    // every contribution is one segment closed by the packet ending.
    let schedule = Schedule::default();
    let mut first = 0;
    for &(bits, passes, length, after) in layers {
        let bytes = pack(bits);
        let mut reader = PacketBits::new(&bytes);
        let got = read_lengths(&mut reader, &mut lblock, schedule, first, passes)
            .expect("B.10.7.1's own bit sequence");
        assert_eq!(
            got,
            vec![SignalledLength {
                length,
                passes,
                terminated: false,
            }],
            "B.10.7.1 NOTE 1: {passes} coding passes, {length} bytes"
        );
        assert_eq!(lblock, after, "Lblock after the layer coding {length}");
        first += passes;
    }
}

/// B.10.7.2's NOTE, which is the only place T.800 publishes a **multiple**
/// codeword segment length signal.
///
/// "Consider the selective arithmetic coding bypass (see D.6). Say that the
/// passes included in a packet for a given code-block are the cleanup pass of
/// bit-plane number 4 through the significance propagation pass of bit-plane
/// number 6 (see Table D.9). These passes are indexed as {0, 1, 2, 3, 4} and
/// the lengths are given as {6, 31, 44, 134, 192} respectively. Then
/// T = {0, 2, 3, 4} and K = 4 lengths are signalled. The set of lengths to be
/// signalled is {6, 75, 134, 192} and the corresponding number of coding
/// passes that are added is {1, 2, 1, 1}. A valid code bit sequence is 11
/// 1110 (Lblock increased to 8), 0000 0110 (log21 = 0, 8 bits used to code
/// length of 6), 0 0100 1011 (log22 = 1, 9 bits used to code the length of
/// 75), 1000 0110 (log21 = 0, 8 bits used to code the length of 134), and
/// 1100 0000 (log21 = 0, 8 bits used to code the length of 192). Notice that
/// the value of Lblock is incremented only at the start of the sequence."
///
/// **`T` is derived here and not written in.** The NOTE's indices are
/// packet-local; the absolute pass indices are 9 to 13, because "the cleanup
/// pass of bit-plane number 4" is pass 9. Feeding the reader `first = 9` and
/// five passes makes it consult this build's own Table D.9 transcription, and
/// the four lengths only come out in the standard's order if that
/// transcription puts terminations exactly where Table D.9 does. Writing
/// `T = {0, 2, 3, 4}` into the test instead would have checked the length
/// coding and nothing else.
#[test]
fn b10_7_2s_worked_example() {
    // "the cleanup pass of bit-plane number 4 through the significance
    // propagation pass of bit-plane number 6". Pass 0 is bit-plane 1's
    // cleanup pass, so bit-plane 4's is pass 9 and bit-plane 6's significance
    // propagation pass is pass 13 — five passes, the NOTE's {0, 1, 2, 3, 4}.
    const FIRST: u32 = 9;
    const PASSES: u32 = 5;

    // The NOTE's own sequence, in the groups the standard prints it in.
    let bytes = pack("11 1110 0000 0110 0 0100 1011 1000 0110 1100 0000");
    // 6 + 8 + 9 + 8 + 8 = 39 bits. The transcription is checked before it is
    // trusted, the way `jpx_annex_j.rs` checks J.10's offsets: a dropped or
    // doubled digit changes this and nothing else would notice.
    assert_eq!(bytes.len(), 5, "39 bits, padded to five bytes");
    assert_eq!(
        bytes,
        vec![0xF8, 0x18, 0x97, 0x0D, 0x80],
        "B.10.7.2's NOTE, packed most significant bit first"
    );

    let mut lblock = 3;
    let mut reader = PacketBits::new(&bytes);
    let got = read_lengths(
        &mut reader,
        &mut lblock,
        Schedule::new(true, false),
        FIRST,
        PASSES,
    )
    .expect("B.10.7.2's own bit sequence");

    // "The set of lengths to be signalled is {6, 75, 134, 192} and the
    // corresponding number of coding passes that are added is {1, 2, 1, 1}."
    assert_eq!(
        got.iter().map(|s| s.length).collect::<Vec<_>>(),
        vec![6, 75, 134, 192],
        "B.10.7.2's four signalled lengths"
    );
    assert_eq!(
        got.iter().map(|s| s.passes).collect::<Vec<_>>(),
        vec![1, 2, 1, 1],
        "B.10.7.2's added coding passes per length"
    );
    // "K = 4 lengths are signalled."
    assert_eq!(got.len(), 4, "K");
    // "Lblock increased to 8", once, at the start.
    assert_eq!(lblock, 8);

    // T = {0, 2, 3, 4} packet-locally is {9, 11, 12} absolutely, plus pass 13
    // added only because it is the final pass included. So three of the four
    // segments really terminated and the last did not — which is what tells
    // [`super::super::tier2::CodeBlock::append`] to leave it open for the
    // next layer. The distinction has no effect on the bits and every effect
    // on the decode.
    assert_eq!(
        got.iter().map(|s| s.terminated).collect::<Vec<_>>(),
        vec![true, true, true, false],
        "T is {{9, 11, 12}}; pass 13 is B.10.7.2's \"final coding pass included\""
    );

    // And the whole sequence was consumed: 39 bits of a 40-bit buffer.
    assert_eq!(reader.consumed(), 5, "39 bits rounds up to five bytes");
}

/// The same five passes without `BYPASS` signal **one** length, not four.
///
/// The discriminating half of the test above. B.10.7.2's example is only a
/// statement about bypass if the same reader over the same pass range does
/// something else when bypass is off — otherwise it would pass against a
/// build that split at every pass, or at none.
#[test]
fn without_bypass_the_same_passes_are_one_segment() {
    let bytes = pack("11 1110 0000 0110 0 0100 1011 1000 0110 1100 0000");
    let mut lblock = 3;
    let mut reader = PacketBits::new(&bytes);
    let got = read_lengths(&mut reader, &mut lblock, Schedule::default(), 9, 5)
        .expect("a single codeword segment");
    assert_eq!(got.len(), 1, "Table D.8's left column terminates no pass");
    assert_eq!(got[0].passes, 5);
    assert!(!got[0].terminated);
}

/// With `TERMALL` the same five passes signal **five** lengths.
///
/// The third of the three answers B.10.7.2 gives for one pass range, and the
/// bit sequence is this repository's rather than T.800's — deliberately, and
/// it is worth saying why the standard's cannot be reused. B.10.7.2's NOTE
/// codes four lengths in 39 bits with `Lblock` at 8; five lengths at that
/// width need 46, so feeding the NOTE's own bytes to a `TERMALL` reader
/// truncates rather than disagreeing, which would have been a test of the
/// buffer length. So the sequence below is built to the rule instead: one
/// `0` leaving `Lblock` at its initial three, then five three-bit lengths,
/// each `Lblock + floor(log2(1)) = 3` wide.
///
/// What is T.800's here is the *shape* — Table D.8's right column terminates
/// every pass, so `K` is the pass count — and that is what the assertions
/// are on.
#[test]
fn with_termall_the_same_passes_are_five_segments() {
    // "0" (Lblock sufficient), then 001, 010, 011, 100, 101.
    let bytes = pack("0 001 010 011 100 101");
    assert_eq!(bytes, vec![0x14, 0xE5], "16 bits, packed");

    let mut lblock = 3;
    let mut reader = PacketBits::new(&bytes);
    let got = read_lengths(&mut reader, &mut lblock, Schedule::new(false, true), 9, 5)
        .expect("five codeword segments");
    assert_eq!(
        got.len(),
        5,
        "Table D.8's right column terminates every pass"
    );
    assert_eq!(lblock, 3, "B.10.7.1: a single 0 leaves Lblock alone");
    assert_eq!(
        got.iter().map(|s| s.length).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5],
    );
    assert!(
        got.iter().all(|s| s.terminated && s.passes == 1),
        "every pass its own terminated segment"
    );
    assert_eq!(reader.consumed(), 2);
}

// --- J.10's two published code-blocks -----------------------------------
//
// T.800 J.10.4 prints, for each of the codestream's two code-blocks, the
// bytes handed to the decoder and the coefficients that come out. Those are
// the standard's numbers on both sides, which is what makes them usable here;
// `tests/jpx_annex_j.rs` uses the same two spans for the packet boundary and
// decodes the whole codestream to J.10.5's samples.
//
// The geometry is J.10.1's and J.10.3's. The image is one component, 1 sample
// wide and 9 tall, with one decomposition level, so B.5 gives resolution 0 a
// 1x5 LL band and resolution 1 a 1x4 LH band — J.10.3's "5 low-pass wavelet
// coefficients, and 4 horizontal low-pass vertical high-pass coefficients".
// The HL and HH bands are empty, because a 1-sample-wide image has no
// horizontally high-pass coefficients at all.

/// J.10.4: "The bytes provided to the arithmetic coder are those beginning at
/// offset 0125", printed as `01 8F0D C875 5D`.
const J10_FIRST_BLOCK: [u8; 6] = [0x01, 0x8F, 0x0D, 0xC8, 0x75, 0x5D];

/// Table J.20: "16 coding passes for this code-block".
const J10_FIRST_PASSES: u32 = 16;

/// J.10.4: "Thus the decoded coefficients are: -26, -22, -30, -32, -19".
const J10_FIRST_COEFFICIENTS: [i32; 5] = [-26, -22, -30, -32, -19];

/// J.10.4: "The compressed data for the only code-block in the second packet,
/// representing the vertical high pass horizontal lowpass sub-band begins at
/// offset 0137 octal", printed as `0F B176`.
const J10_SECOND_BLOCK: [u8; 3] = [0x0F, 0xB1, 0x76];

/// Table J.21: "7 coding passes for this code-block".
const J10_SECOND_PASSES: u32 = 7;

/// J.10.4: "The decoded vertical high pass horizontal low pass coefficients
/// are: 1, 5, 1, 0".
const J10_SECOND_COEFFICIENTS: [i32; 4] = [1, 5, 1, 0];

fn decode(
    bytes: &[u8],
    w: u32,
    h: u32,
    passes: u32,
    o: Orientation,
    style: CodingStyle,
) -> Vec<i32> {
    let mut contexts = initial_contexts();
    let mut work = u64::MAX;
    decode_code_block(
        &Segment::single(bytes, passes),
        w,
        h,
        passes,
        o,
        style,
        &mut contexts,
        &mut work,
    )
    .expect("J.10's own code-block bytes")
    .coefficients
}

/// Both of J.10's code-blocks, decoded in isolation, give J.10.4's published
/// coefficients.
///
/// The basis for everything below it, and it runs first for the reason
/// `jpx_annex_j.rs` checks J.10's offsets before decoding: if this build read
/// these two spans differently from the standard, the bypass tests after it
/// would be experiments on ourselves.
#[test]
fn j10s_published_code_blocks_decode_to_its_published_coefficients() {
    assert_eq!(
        decode(
            &J10_FIRST_BLOCK,
            1,
            5,
            J10_FIRST_PASSES,
            Orientation::Ll,
            CodingStyle::default()
        ),
        J10_FIRST_COEFFICIENTS,
        "J.10.4's first code-block"
    );
    assert_eq!(
        decode(
            &J10_SECOND_BLOCK,
            1,
            4,
            J10_SECOND_PASSES,
            Orientation::Lh,
            CodingStyle::default()
        ),
        J10_SECOND_COEFFICIENTS,
        "J.10.4's second code-block"
    );
}

/// Table J.20's "Codestream bytes" column: J.10's first packet header.
///
/// Three bytes at octal 00122. `jpx_annex_j.rs`'s `J10_SPANS` carries the
/// same three at the same offset inside the whole codestream, and its
/// `j10_publishes_where_each_packet_header_ends` is where the offsets are
/// checked; here they are the standard's bits and nothing else.
const J10_FIRST_HEADER: [u8; 3] = [0xC7, 0xD4, 0x0C];

/// Table J.21's "Codestream bytes" column: J.10's second packet header, four
/// bytes at octal 00133.
const J10_SECOND_HEADER: [u8; 4] = [0xC0, 0x7C, 0x21, 0x80];

/// B.10.7.2's `K`, counted from a schedule rather than read from a header.
///
/// "Let T be the set of indices of terminated coding passes included for the
/// code-block in the packet as indicated in Tables D.8 and D.9. If the index
/// final coding pass included in the packet is not a member of T, then it is
/// added to T ... K lengths are signalled."
fn k(schedule: Schedule, first: u32, passes: u32) -> usize {
    (first..first + passes)
        .filter(|&p| schedule.terminates(p))
        .count()
        + usize::from(!schedule.terminates(first + passes - 1))
}

/// B.10.3 and B.10.8's leading fields of a packet header carrying exactly one
/// included code-block, as Tables J.20 and J.21 annotate them.
///
/// Returns the reader positioned on B.10.7's first `Lblock` signalling bit,
/// with `(zero bit-planes, coding passes)` behind it. Everything it reads is
/// a field the two tables name and give a value for, so the caller can assert
/// the standard's own numbers before any length is signalled — the discipline
/// `jpx_annex_j.rs` uses on J.10's offsets, applied to its bits.
fn j10_packet_header_prefix<'a>(header: &'a [u8]) -> (PacketBits<'a>, u32, u32) {
    let mut bits = PacketBits::new(header);
    assert_eq!(
        bits.bit().expect("J.10"),
        1,
        "B.10.3: non-zero length packet"
    );
    assert_eq!(
        bits.bit().expect("J.10"),
        1,
        "the only code-block is included"
    );
    // B.10.2's tag tree over a single node, which emits one 0 per unit of the
    // value and then a 1. Both of J.10's precincts hold one code-block, so
    // there is no tree above the leaf to read.
    let mut zero_planes = 0;
    while bits.bit().expect("J.10") == 0 {
        zero_planes += 1;
    }
    let passes = read_pass_count(&mut bits).expect("B.10.6");
    (bits, zero_planes, passes)
}

/// **D.6's boundary, adjudicated from below, through J.10's own packet
/// header.** J.10's second code-block has seven coding passes, and D.6
/// bypasses nothing before the tenth — so setting `BYPASS` must leave both
/// halves of that packet alone: `K` is still 1, the one signalled length is
/// still Table J.21's 3 bytes, and the decode is still J.10.4's "1, 5, 1, 0".
///
/// **This is the test the capability turns on, and its first version was
/// vacuous.** That version handed tier-1 a single [`Segment`] and asserted
/// the coefficients were unchanged — which they could not help being, since
/// with one segment there is nothing for `BYPASS` to do and the pass count
/// never reaches the code that consults `passes::FIRST_BYPASS_PASS`. A decoder with
/// the boundary at 1 would have passed it. The boundary is only observable
/// where it is *used*, which is B.10.7.2's split, so the header is where the
/// test has to start.
///
/// What it now discriminates: at any boundary of 6 or less, pass 5 or pass 6
/// terminates, `K` becomes 2 or more, J.10's published 25-bit header parses
/// as something else, and the raw reader takes passes this code-block has.
#[test]
fn j10s_second_packet_is_unmoved_by_bypass() {
    let bypass = Schedule::new(true, false);
    let (mut bits, zero_planes, passes) = j10_packet_header_prefix(&J10_SECOND_HEADER);
    // Table J.21: "7 zero bit-planes", "7 coding passes for this code-block".
    assert_eq!(zero_planes, 7, "Table J.21's zero bit-planes");
    assert_eq!(passes, J10_SECOND_PASSES, "Table J.21's coding passes");

    // D.6 reaches no pass of this code-block, so B.10.7.2 signals one length.
    assert_eq!(
        k(bypass, 0, passes),
        1,
        "T is empty; only the final pass is added"
    );
    let mut lblock = 3;
    let got = read_lengths(&mut bits, &mut lblock, bypass, 0, passes).expect("Table J.21's bits");
    // Table J.21: "0 — LBlock remains 3", "0001 1 — 3 bytes of compressed
    // data", "0000000 — unused padding bits".
    assert_eq!(lblock, 3, "Table J.21: LBlock remains 3");
    assert_eq!(
        got,
        vec![SignalledLength {
            length: 3,
            passes: 7,
            terminated: false,
        }],
        "Table J.21: 3 bytes of compressed data, in one codeword segment"
    );
    // 25 bits of a 32-bit header, and the seven that are left are the
    // table's "unused padding bits".
    assert_eq!(bits.consumed(), 4);

    // And with neither bit set — the codestream J.10 actually publishes — the
    // same header reads identically. That is the claim: `BYPASS` is inert
    // here, rather than merely surviving.
    let (mut plain, _, _) = j10_packet_header_prefix(&J10_SECOND_HEADER);
    let mut lblock = 3;
    assert_eq!(
        read_lengths(&mut plain, &mut lblock, Schedule::default(), 0, passes).expect("J.10"),
        got,
        "D.6 changes nothing about a seven-pass code-block"
    );

    // The published bytes, cut to the published length, decode to the
    // published coefficients — with `BYPASS` set.
    assert_eq!(
        decode(
            J10_SECOND_BLOCK.get(..got[0].length).expect("3 of 3 bytes"),
            1,
            4,
            passes,
            Orientation::Lh,
            CodingStyle {
                bypass: true,
                ..CodingStyle::default()
            }
        ),
        J10_SECOND_COEFFICIENTS,
        "J.10.4's \"1, 5, 1, 0\", decoded under D.6"
    );
}

/// **D.6's boundary, bracketed from above.** J.10's first code-block has
/// sixteen coding passes, so `BYPASS` reaches it — and J.10's published first
/// packet header then stops being readable, because B.10.7.2 wants five
/// lengths where the standard printed one.
///
/// The pair brackets the boundary into 7..=15: below 7 the test above breaks,
/// from 16 up this one does, and ten is transcribed from D.6 and Table D.9.
/// Nothing in T.800 that this repository can run distinguishes the eight
/// values inside that range, and saying so is the point of writing the
/// bracket down rather than claiming the clause was proved.
#[test]
fn j10s_first_packet_header_is_not_a_bypass_header() {
    let plain = Schedule::default();
    let bypass = Schedule::new(true, false);

    // Table J.20's own numbers first, read out of Table J.20's own bytes.
    let (mut bits, zero_planes, passes) = j10_packet_header_prefix(&J10_FIRST_HEADER);
    assert_eq!(zero_planes, 3, "Table J.20: 3 zero bit-planes");
    assert_eq!(passes, J10_FIRST_PASSES, "Table J.20: 16 coding passes");
    let mut lblock = 3;
    let got = read_lengths(&mut bits, &mut lblock, plain, 0, passes).expect("Table J.20's bits");
    assert_eq!(lblock, 3, "Table J.20: LBlock remains 3");
    assert_eq!(
        got,
        vec![SignalledLength {
            length: 6,
            passes: 16,
            terminated: false,
        }],
        "Table J.20: 6 bytes of compressed data"
    );
    assert_eq!(bits.consumed(), 3, "J.10.3: \"requires 3 bytes\"");

    // With `BYPASS` the same three bytes are no longer a whole header: five
    // lengths at Lblock 3 need 6 + 4 + 3 + 4 + 3 = 20 bits and eight are
    // left. The refusal is the shape of the evidence — J.10's header is one
    // length long, which sixteen coding passes only permit with D.6 off.
    assert_eq!(k(bypass, 0, passes), 5, "T = {{9, 11, 12, 14, 15}}");
    let (mut bits, _, _) = j10_packet_header_prefix(&J10_FIRST_HEADER);
    let mut lblock = 3;
    assert!(
        read_lengths(&mut bits, &mut lblock, bypass, 0, passes).is_err(),
        "three published bytes cannot hold B.10.7.2's five lengths"
    );
}

/// `TERMALL` cannot be flipped onto J.10, and the reason is a number rather
/// than a shrug.
///
/// The lever that made ROI and POC adjudicable was that J.10's own bytes
/// could be rearranged and still be required to produce J.10.5's samples. It
/// does not reach here, and this test records why so that the next reader
/// does not re-derive it: with `TERMALL` set, B.10.7.2 signals one length per
/// *terminated* pass, and J.10's two code-blocks carry sixteen and seven
/// coding passes. Setting the code-block style byte in J.10's COD from `0x00`
/// to `0x04` would therefore require sixteen lengths in the first packet
/// header and seven in the second, where J.10 prints one and one. The bytes
/// would have to be re-encoded, and an encoder this repository wrote agreeing
/// with a decoder this repository wrote is a statement about this repository.
///
/// So `TERMALL` has **no** link in this file whose expected output is
/// T.800's. What holds it up instead is Table D.8's two columns transcribed
/// and asserted as data in [`super::super::passes`], B.10.7.2's rule for
/// turning those columns into `K`, and the fact that the same rule — the same
/// function, on the same inputs — is what B.10.7.2's own worked example
/// adjudicates for `BYPASS` in [`b10_7_2s_worked_example`].
#[test]
fn termall_cannot_be_flipped_onto_j10() {
    for (style, first, second) in [
        (Schedule::new(false, true), 16, 7),
        (Schedule::new(true, true), 16, 7),
        (Schedule::new(true, false), 5, 1),
        (Schedule::default(), 1, 1),
    ] {
        assert_eq!(
            k(style, 0, J10_FIRST_PASSES),
            first,
            "{style:?}, J.10's first code-block"
        );
        assert_eq!(
            k(style, 0, J10_SECOND_PASSES),
            second,
            "{style:?}, J.10's second code-block"
        );
    }
    // Tables J.20 and J.21 print exactly one length each, so only the last
    // row above describes the codestream J.10 publishes — and the
    // second-to-last describes its second packet as well, which is what
    // [`j10s_second_packet_is_unmoved_by_bypass`] rests on.
    let (_, _, first_passes) = j10_packet_header_prefix(&J10_FIRST_HEADER);
    let (_, _, second_passes) = j10_packet_header_prefix(&J10_SECOND_HEADER);
    assert_eq!((first_passes, second_passes), (16, 7));
}

// --- the raw reader is reached, which is wiring rather than evidence -----

/// `BYPASS` selects [`super::super::tier1`]'s raw reader for a segment whose
/// first pass is past D.6's boundary.
///
/// **Self-consistency, and labelled as such.** T.800 publishes no codestream
/// with a raw coding pass in it — J.10's is the only codestream in the 231
/// pages and its COD sets none of Table A.19's bits — so there is no
/// published byte sequence whose raw decode has a published answer. What this
/// checks is only that the flag arrives: the same segment list decoded with
/// `bypass` set and clear must differ, because one reads the second segment
/// as MQ decisions and the other as raw bits. A build that computed the
/// schedule correctly and then never consulted it would pass every other test
/// in this file.
#[test]
fn a_segment_past_the_boundary_is_read_raw() {
    // Ten passes, then two: D.6's first raw segment starts at pass 10.
    let segments = [
        Segment {
            bytes: J10_FIRST_BLOCK.to_vec(),
            passes: 10,
            terminated: true,
        },
        Segment {
            bytes: vec![0x5A, 0xC3],
            passes: 2,
            terminated: true,
        },
    ];
    let run = |bypass: bool| {
        let mut contexts = initial_contexts();
        let mut work = u64::MAX;
        decode_code_block(
            &segments,
            1,
            5,
            12,
            Orientation::Ll,
            CodingStyle {
                bypass,
                ..CodingStyle::default()
            },
            &mut contexts,
            &mut work,
        )
        .expect("a well-formed segment list")
        .coefficients
    };
    assert_ne!(
        run(true),
        run(false),
        "passes 10 and 11 are D.6's first raw pair"
    );
}

// --- the two stuffed bit readers ----------------------------------------

/// D.6's unstuffing rule and B.10.1's are the same rule in two clauses, and
/// this pins that they stay the same.
///
/// D.6: "this routine throws out the first bit after an 0xFF byte value".
/// B.10.1 says it of packet header bits. Two implementations exist because
/// they read different streams and fail differently at the end; two *rules*
/// would be a bug waiting for a code-block whose bytes happen to contain an
/// 0xFF.
#[test]
fn the_two_stuffed_bit_readers_agree() {
    // 0xFF in every position that matters: first, middle, last, and twice in
    // a row.
    let data: &[u8] = &[0xFF, 0x00, 0x5A, 0xFF, 0xFF, 0x7F, 0xA5, 0xFF, 0x01];
    let mut packet = PacketBits::new(data);
    let mut raw = crate::jpx::tier1::raw_bits_for_test(data);
    // Fewer bits than the buffer holds, so neither reader is asked what it
    // does past the end — where they deliberately differ.
    for i in 0..60 {
        let a = packet.bit().expect("inside the buffer");
        let b = raw();
        assert_eq!(a, b, "bit {i}");
    }
}

/// D.4.1's extension, which D.6's NOTE 2 applies to the raw stream: past the
/// end of a codeword segment the decoder appends 0xFF.
///
/// The bit that comes back is therefore a **one**, and it stays one however
/// far past the end the caller reads — an 0xFF after an 0xFF unstuffs to
/// seven ones rather than to something else. This is the standard's answer
/// and not a convenience: NOTE 2 says truncation of a raw bit stream "may be
/// possible" *because* the decoder appends, so a short raw segment is a thing
/// a conforming encoder may emit, not only a thing a damaged file has.
///
/// The two readers part company here, which is why
/// [`the_two_stuffed_bit_readers_agree`] stays inside its buffer: a packet
/// header that runs out is a `Truncated` refusal, because B.10 gives a
/// packet header an explicit end and reading past it means the parse was
/// already wrong.
#[test]
fn a_raw_segment_past_its_end_reads_as_the_appended_0xff() {
    // One byte, and then nothing. Its own eight bits come out first.
    let mut raw = crate::jpx::tier1::raw_bits_for_test(&[0b1010_0110]);
    for expected in [1, 0, 1, 0, 0, 1, 1, 0] {
        assert_eq!(raw(), expected, "the byte that is there");
    }
    for i in 0..64 {
        assert_eq!(raw(), 1, "0xFF, appended, bit {i}");
    }

    // And when the last real byte *is* an 0xFF — which D.6 forbids an encoder
    // to emit, so this is the damaged case — the appended byte is unstuffed
    // to seven bits and every one of them is still a one.
    let mut raw = crate::jpx::tier1::raw_bits_for_test(&[0xFF]);
    for i in 0..32 {
        assert_eq!(raw(), 1, "bit {i}");
    }

    // An empty segment is the same rule with nothing before it.
    let mut raw = crate::jpx::tier1::raw_bits_for_test(&[]);
    for i in 0..32 {
        assert_eq!(raw(), 1, "bit {i}");
    }
}

// --- the style bits reach tier-1 and tier-2 at all ----------------------

/// The two bits are read out of `SPcod` at the positions Table A.19 gives
/// them, and A.10's Profile-0 mnemonic is an independent check on that.
///
/// Table A.45 constrains Profile-0's code-block style to
/// `SPcod, SPcoc = 00sp vtra` with `a = r = v = 0` and NOTE 1 "t = 1 for
/// termination on each coding pass, p = 1 for predictive termination, s = 1
/// for segmentation symbols". Read most significant bit first that spells out
/// bit 0 = a (arithmetic coding bypass), bit 1 = r (reset), bit 2 = t
/// (termination on each pass), bit 3 = v (vertically causal), bit 4 = p, bit
/// 5 = s — the same assignment as Table A.19, printed in a different clause
/// in a different form. Two independent statements of one table is exactly
/// what a transcription wants, and this asserts against both.
#[test]
fn table_a19s_bit_positions_agree_with_table_a45s_mnemonic() {
    assert_eq!(cb_style::BYPASS, 0b0000_0001, "a, Table A.45's 00sp vtra");
    assert_eq!(cb_style::RESET, 0b0000_0010, "r");
    assert_eq!(cb_style::TERMALL, 0b0000_0100, "t");
    assert_eq!(cb_style::VERTICALLY_CAUSAL, 0b0000_1000, "v");
    assert_eq!(cb_style::PREDICTABLE, 0b0001_0000, "p");
    assert_eq!(cb_style::SEGMENTATION_SYMBOLS, 0b0010_0000, "s");
    // "00sp vtra": bits 6 and 7 are the two zeros, which is Table A.19's "All
    // other values reserved" said a second way.
    assert_eq!(cb_style::DEFINED, 0b0011_1111);

    // Table A.45's Profile-0 row in full: "where a = r = v = 0, and
    // t, p, s = 0 or 1". So Profile-0 permits `t` while forbidding `a`, which
    // is the one place T.800 treats these two bits differently and a useful
    // reminder that they are two capabilities rather than one: `TERMALL` is
    // legal in the most restrictive profile the standard defines, and
    // `BYPASS` is not.
    let forbidden = cb_style::BYPASS | cb_style::RESET | cb_style::VERTICALLY_CAUSAL;
    let permitted = cb_style::TERMALL | cb_style::PREDICTABLE | cb_style::SEGMENTATION_SYMBOLS;
    assert_eq!(forbidden, 0b0000_1011, "a, r and v");
    assert_eq!(permitted, 0b0011_0100, "t, p and s");
    assert_eq!(
        forbidden | permitted,
        cb_style::DEFINED,
        "Table A.45 accounts for all six of Table A.19's bits and no others"
    );
    assert_eq!(forbidden & permitted, 0, "no bit is on both sides");
}

// --- what a raw pass actually reads -------------------------------------

/// D.4.1's extension and (D-2)'s sign, in one decode: a raw segment with no
/// bytes left makes every decision a 1, and every sign a **minus**.
///
/// The cleanest case D.6 offers, and it needs no encoder. Past the end of a
/// codeword segment the decoder appends 0xFF (D.4.1, restated for the raw
/// stream by D.6's NOTE 2), so every bit the raw reader returns is a one.
/// In a significance propagation pass that means every coefficient the pass
/// visits becomes significant, and (D-2) — "signbit = raw_value, where
/// raw_value = 1 is a negative sign bit" — makes every one of their signs
/// negative.
///
/// So the expected output is not a pinned blob: the three coefficients J.10's
/// first six bytes leave at zero become **-1**, the last bit-plane's worth of
/// magnitude with a negative sign, and nothing else moves. It is the
/// discriminating case for the one line of D.6 easiest to miss, which is that
/// a raw sign is *not* run through D.2.2's context and XOR. Running it there
/// inverts about half of these signs, which is a picture rather than an
/// error.
///
/// **Self-consistency about the first ten passes, adjudicated about the
/// last two.** J.10's six bytes and the coefficients they give through ten
/// arithmetic passes are the standard's; that a `BYPASS` codestream would
/// ever present them in this shape is this file's construction.
#[test]
fn a_raw_pass_takes_every_decision_and_its_sign_from_the_stream() {
    // Passes 0..=9 arithmetic out of J.10's published bytes, then D.6's first
    // raw pair — with no bytes at all, so the appended 0xFF supplies them.
    let segments = [
        Segment {
            bytes: J10_FIRST_BLOCK.to_vec(),
            passes: 10,
            terminated: true,
        },
        Segment {
            bytes: Vec::new(),
            passes: 2,
            terminated: true,
        },
    ];
    let run = |bypass: bool| {
        let mut contexts = initial_contexts();
        let mut work = u64::MAX;
        decode_code_block(
            &segments,
            4,
            4,
            12,
            Orientation::Ll,
            CodingStyle {
                bypass,
                ..CodingStyle::default()
            },
            &mut contexts,
            &mut work,
        )
        .expect("a well-formed segment list")
        .coefficients
    };

    let without = run(false);
    let with = run(true);
    assert_eq!(without.len(), 16);

    // Every coefficient the arithmetic passes left at zero is now -1. The
    // ones they left non-zero are untouched, which is an observation rather
    // than a rule: the magnitude refinement pass's raw bits are ones too, and
    // every magnitude J.10's six bytes produce here is already odd.
    for (i, (&before, &after)) in without.iter().zip(with.iter()).enumerate() {
        if before == 0 {
            assert_eq!(after, -1, "(D-2): coefficient {i} is newly significant");
        } else {
            assert_eq!(after, before, "coefficient {i} was already significant");
        }
    }
    // And the case is not vacuous: some coefficient really was left at zero.
    assert!(
        without.contains(&0),
        "the raw significance propagation pass must have something to find"
    );
}

/// A codeword segment boundary re-initialises the decoder and **not** the
/// context states.
///
/// `TERMALL` and `RESET` are Table A.19 bits 2 and 1 and they are not the same
/// bit. D.4 terminates the arithmetic coder between passes, which is a
/// statement about where bytes begin; Table D.7's reset is a statement about
/// the nineteen adaptive states. A decoder that did both at every termination
/// would decode a `TERMALL` stream's second pass as noise, and the
/// [`super::super::tier1`] comment that says so had nothing behind it.
///
/// The discriminator: with one coding pass per codeword segment — which is
/// exactly what `TERMALL` produces — "reset the contexts at every segment
/// boundary" and "reset them at every pass boundary" are the same rule. So a
/// build that conflated the two would make these two decodes equal. They must
/// not be.
///
/// Wiring rather than evidence, and the bytes are arbitrary: what is checked
/// is that two Table A.19 bits stay two.
#[test]
fn a_codeword_segment_boundary_does_not_reset_the_context_states() {
    let segments: Vec<Segment> = (0..12)
        .map(|i| Segment {
            bytes: vec![J10_FIRST_BLOCK[i % J10_FIRST_BLOCK.len()]],
            passes: 1,
            terminated: true,
        })
        .collect();
    let run = |reset_contexts: bool| {
        let mut contexts = initial_contexts();
        let mut work = u64::MAX;
        decode_code_block(
            &segments,
            4,
            4,
            12,
            Orientation::Ll,
            CodingStyle {
                terminate_all: true,
                reset_contexts,
                ..CodingStyle::default()
            },
            &mut contexts,
            &mut work,
        )
        .expect("a well-formed segment list")
        .coefficients
    };
    assert_ne!(
        run(false),
        run(true),
        "Table A.19 bit 2 is not Table A.19 bit 1"
    );
}

// --- B.10.7.2 across two layers -----------------------------------------

/// A code-block's contribution split across two layers decodes exactly as the
/// same bytes contributed in one.
///
/// **The one rule in B.10.7.2 that only a multi-layer codestream reaches.** A
/// signalled length is not a codeword segment: the clause signals a length for
/// the final coding pass included in a packet whether or not that pass
/// terminated, so a code-block whose passes span two layers gets two lengths
/// for one segment. The second contribution must *continue* the first —
/// same segment, same bytes, same reader — and only a contribution arriving
/// against a genuinely terminated segment starts a new one. A decoder that
/// started a new one would open a fresh MQ decoder part way through a segment
/// and get coefficients rather than an error.
///
/// It also pins the other half of the same rule: the schedule is consulted at
/// the code-block's **own** pass numbering, not at packet-local indices. Under
/// `BYPASS` the second layer here contributes passes 5 to 11, which Tables D.8
/// and D.9 split at 9 and 11; read as passes 0 to 6 they would not split at
/// all, the packet's bodies would sit at different offsets, and tier-2's own
/// "a tile's packets consume it exactly" check would refuse the file.
///
/// **Both codestreams are this file's**, and the claim is only that layering
/// is transparent — the twelve bytes are arbitrary and no published number is
/// involved. That is what this rule is: a statement about bookkeeping, which
/// T.800 makes in prose and publishes no bytes for.
#[test]
fn a_contribution_split_across_two_layers_decodes_as_one() {
    // Twelve bytes of code-block data, split 9 + 3 at D.6's termination after
    // pass 9 — and the first nine split again, 4 + 5, at the layer boundary.
    const DATA: [u8; 12] = [
        0x01, 0x8F, 0x0D, 0xC8, 0x75, 0x5D, 0x9A, 0x3C, 0x41, 0x6E, 0xB2, 0x07,
    ];

    // One layer: one packet for resolution 0 carrying all twelve coding
    // passes, and an empty packet for resolution 1.
    //
    //   1          B.10.3: a non-zero length packet
    //   1          B.10.4: the inclusion tag tree over one code-block, value 0
    //   1          B.10.5: the zero bit-plane tag tree, value 0
    //   111100110  B.10.6: 12 coding passes
    //   0          B.10.7.1: Lblock stays at three
    //   001001     passes 0..=9 are one segment: 3 + floor(log2 10) = 6 bits
    //   0011       passes 10 and 11 are the next: 3 + floor(log2 2) = 4 bits
    let mut one = pack("1 1 1 111100110 0 001001 0011");
    assert_eq!(one, vec![0xFE, 0x61, 0x26], "23 bits in three bytes");
    one.extend_from_slice(&DATA);
    one.push(0x00); // resolution 1's empty packet.

    // Two layers, LRCP: layer 0 resolution 0, layer 0 resolution 1, layer 1
    // resolution 0, layer 1 resolution 1.
    let mut two = pack("1 1 1 1110 0 00100");
    assert_eq!(two, vec![0xFC, 0x20], "13 bits in two bytes");
    two.extend_from_slice(&DATA[..4]); // five coding passes, four bytes
    two.push(0x00);
    //   1          a non-zero length packet
    //   1          B.10.4: one bit, because this code-block is already included
    //   111100001  B.10.6: 7 coding passes
    //   0          Lblock stays at three
    //   00101      passes 5..=9 close the segment pass 9 terminates: 5 bits
    //   0011       passes 10 and 11: 4 bits
    let header = pack("1 1 111100001 0 00101 0011");
    assert_eq!(header, vec![0xFC, 0x22, 0x98], "21 bits in three bytes");
    two.extend_from_slice(&header);
    two.extend_from_slice(&DATA[4..]);
    two.push(0x00);

    let decode = |layers: u16, packets: &[u8]| {
        let spec = Spec {
            layers,
            cb_style: cb_style::BYPASS,
            ..Spec::default()
        };
        let mut warnings = Vec::new();
        jpx_decode(
            &spec.codestream(&[(0, packets)]),
            &Limits::new(1 << 20),
            &mut warnings,
        )
        .map(|image| (image.samples, warnings))
    };

    let (one_layer, one_warnings) = decode(1, &one).expect("the one-layer codestream");
    let (two_layer, two_warnings) = decode(2, &two).expect("the two-layer codestream");
    assert!(one_warnings.is_empty(), "{one_warnings:?}");
    assert!(two_warnings.is_empty(), "{two_warnings:?}");
    assert_eq!(
        one_layer, two_layer,
        "B.10.7.2: the layer boundary is not a codeword segment boundary"
    );
    // Not vacuous: the bytes decoded to something rather than to a blank.
    assert_eq!(one_layer.len(), 16, "a 4 x 4 image, one component, 8 bits");
    assert!(
        one_layer.iter().any(|&s| s != one_layer[0]),
        "the twelve bytes produced a picture rather than a flat field"
    );
}
