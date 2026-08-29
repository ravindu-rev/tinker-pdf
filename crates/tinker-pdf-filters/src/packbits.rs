//! PackBits (TIFF 6.0 §9), the run-length scheme TIFF compression 32773 names.
//!
//! # Why this is not `runlength.rs`
//!
//! PDF's RunLengthDecode (7.4.5) and TIFF's PackBits are the same Macintosh
//! scheme read by two specifications that disagree about one byte and about
//! when to stop, and the disagreement is not cosmetic:
//!
//! | | `runlength.rs` (7.4.5) | here (TIFF 6.0 §9) |
//! | --- | --- | --- |
//! | tag 128 | **EOD**: the stream ends | **a no-op**: skip it and read on |
//! | end of data | the EOD byte | the caller's expected byte count |
//! | a stream with no 128 | [`Warning::EarlyEod`] | ordinary and complete |
//!
//! Sharing one routine would mean a flag that changes what 128 means, and a
//! decoder in which a single byte silently switches between "stop here" and
//! "ignore me" is one whose two callers can never be reasoned about
//! separately. A PDF stream that ends without its EOD is damaged; a TIFF strip
//! that ends without a 128 is every TIFF strip ever written.
//!
//! # The stop condition is the caller's
//!
//! §9's own decoder is "loop until you get the number of decompressed bytes
//! you expect", and a TIFF strip knows that number before it starts: rows in
//! the strip times bytes in a row. So `expected` is a parameter rather than a
//! thing this module derives, and it is what bounds the output — the caller's
//! [`Limits`] bounds it a second time, from outside, for a caller that got its
//! own arithmetic wrong.

use crate::{Limits, Warning, Warnings};

/// Decodes one PackBits-compressed run of `expected` bytes.
///
/// Returns the bytes and whether they came out whole. Output stops at
/// `expected`, or at `limits.max_output`, whichever is smaller — a strip that
/// decodes long is truncated to the geometry the directory declared rather
/// than trusted to redefine it.
pub(crate) fn packbits_bytes(
    input: &[u8],
    expected: usize,
    limits: &Limits,
    w: &mut Warnings,
) -> (Vec<u8>, bool) {
    let ceiling = expected.min(limits.max_output);
    let mut out: Vec<u8> = Vec::with_capacity(ceiling.min(1 << 16));
    let mut i = 0usize;

    while out.len() < ceiling {
        let Some(&tag) = input.get(i) else {
            // §9 stops on the byte count, so running out of input first means
            // the strip is short. The rows decoded so far are kept (ruling 2).
            w.push(Warning::TruncatedInput);
            return (out, false);
        };
        i += 1;

        // TIFF 6.0 p.42: "if n is between 0 and 127 inclusive, copy the next
        // n+1 bytes literally"; "else if n is between -1 and -127 inclusive,
        // copy the next byte -n+1 times"; "else if n is -128, noop". The three
        // arms are written in the specification's own order.
        if tag < 128 {
            let want = tag as usize + 1;
            let take = want.min(input.len().saturating_sub(i));
            let room = ceiling - out.len();
            let n = take.min(room);
            if let Some(s) = input.get(i..i + n) {
                out.extend_from_slice(s);
            }
            i += take;
            if n < take {
                // The literal ran past the geometry; the excess is dropped and
                // the loop's own condition ends the strip.
                w.push(Warning::PackBitsRunOverruns);
            }
            // A literal that outran the input is truncation **only** if the
            // strip is still short. A run cut off by the geometry it had
            // already filled is the overrun above and nothing else — reporting
            // both would make `complete` false for a strip that is exactly as
            // long as it was declared to be.
            if take < want && out.len() < ceiling {
                w.push(Warning::TruncatedInput);
                return (out, false);
            }
        } else if tag > 128 {
            let Some(&b) = input.get(i) else {
                w.push(Warning::TruncatedInput);
                return (out, false);
            };
            i += 1;
            // -n + 1 with n = tag - 256, which is 257 - tag: 255 repeats
            // twice and 129 repeats 128 times.
            let want = 257 - tag as usize;
            let room = ceiling - out.len();
            let n = want.min(room);
            out.resize(out.len() + n, b);
            if n < want {
                w.push(Warning::PackBitsRunOverruns);
            }
        } else {
            // -128. §9: "noop". Never written by an encoder this repository
            // would call reasonable, and skipped rather than treated as an end
            // marker, which is what 7.4.5 would have done with it.
            w.push(Warning::PackBitsNoOp);
        }
    }

    // Whether anything is left over is not damage: §9's decoder stops on the
    // count, and an encoder is free to pad a strip.
    let whole = out.len() == expected;
    (out, whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Limits = Limits::new(1 << 20);

    fn unpack(input: &[u8], expected: usize) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = packbits_bytes(input, expected, &CAP, &mut w);
        (d, c, w.into_vec())
    }

    /// TIFF 6.0 p.42's own worked example, both halves transcribed from the
    /// specification text: the compressed string and what it decodes to.
    #[test]
    fn the_specifications_worked_example_decodes() {
        const COMPRESSED: [u8; 15] = [
            0xFE, 0xAA, 0x02, 0x80, 0x00, 0x2A, 0xFD, 0xAA, 0x03, 0x80, 0x00, 0x2A, 0x22, 0xF7,
            0xAA,
        ];
        const DECODED: [u8; 24] = [
            0xAA, 0xAA, 0xAA, 0x80, 0x00, 0x2A, 0xAA, 0xAA, 0xAA, 0xAA, 0x80, 0x00, 0x2A, 0x22,
            0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
        ];

        let (d, c, w) = unpack(&COMPRESSED, DECODED.len());
        assert_eq!(d, DECODED, "TIFF 6.0 p.42");
        assert!(c);
        assert!(w.is_empty());
    }

    /// The encoder half of §9, written here so the round-trip below is a
    /// round-trip rather than a re-run of the decoder's own arithmetic.
    ///
    /// It is the naive rule the specification describes — a run of three or
    /// more identical bytes becomes a replicate run, everything else
    /// accumulates into a literal — and it is deliberately *not* shipped: this
    /// engine reads TIFF and does not write it.
    fn pack(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < input.len() {
            let b = input[i];
            let mut run = 1usize;
            while i + run < input.len() && input[i + run] == b && run < 128 {
                run += 1;
            }
            if run >= 3 {
                out.push((257 - run) as u8);
                out.push(b);
                i += run;
            } else {
                let start = i;
                let mut lit = 0usize;
                while i < input.len() && lit < 128 {
                    // Stop the literal where a replicate run of three begins.
                    if i + 2 < input.len() && input[i] == input[i + 1] && input[i] == input[i + 2] {
                        break;
                    }
                    i += 1;
                    lit += 1;
                }
                out.push((lit - 1) as u8);
                out.extend_from_slice(&input[start..start + lit]);
            }
        }
        out
    }

    #[test]
    fn round_trips_every_shape_a_strip_takes() {
        let cases: [Vec<u8>; 6] = [
            b"the quick brown fox".to_vec(),
            vec![0x55; 300],
            (0..=255u8).collect(),
            // Alternating runs and literals, which is where the encoder's
            // lookahead and the decoder's tag arithmetic have to agree.
            [vec![1u8; 5], vec![2, 3, 4], vec![9u8; 200], vec![7]].concat(),
            vec![0u8; 1],
            vec![0xFFu8; 128],
        ];
        for raw in cases {
            let (d, c, w) = unpack(&pack(&raw), raw.len());
            assert_eq!(d, raw, "round trip");
            assert!(c);
            assert!(w.is_empty());
        }
    }

    #[test]
    fn the_no_op_tag_is_skipped_rather_than_ending_the_strip() {
        // 7.4.5 would have stopped at the 0x80 and returned "ab".
        let (d, c, w) = unpack(&[0x01, b'a', b'b', 0x80, 0x01, b'c', b'd'], 4);
        assert_eq!(d, b"abcd");
        assert!(c);
        assert_eq!(w, vec![Warning::PackBitsNoOp]);
    }

    #[test]
    fn a_strip_with_no_terminator_is_ordinary() {
        let (d, c, w) = unpack(&[0x02, b'x', b'y', b'z'], 3);
        assert_eq!(d, b"xyz");
        assert!(c, "PackBits has no EOD to be missing");
        assert!(w.is_empty());
    }

    #[test]
    fn a_literal_that_runs_off_the_end_keeps_what_is_there() {
        let (d, c, w) = unpack(&[0x09, b'a', b'b'], 10);
        assert_eq!(d, b"ab");
        assert!(!c);
        assert_eq!(w, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn a_replicate_run_with_no_byte_to_repeat_stops() {
        let (d, c, w) = unpack(&[0x00, b'a', 0xF0], 20);
        assert_eq!(d, b"a");
        assert!(!c);
        assert_eq!(w, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn a_run_past_the_declared_geometry_is_cut_to_it() {
        // 0x81 asks for 128 copies where the strip is four bytes wide.
        let (d, c, w) = unpack(&[0x81, b'q'], 4);
        assert_eq!(d, b"qqqq");
        assert!(c, "the strip is exactly as long as it was declared");
        assert_eq!(w, vec![Warning::PackBitsRunOverruns]);
    }

    #[test]
    fn the_callers_ceiling_bounds_a_bomb_below_the_geometry() {
        // Every byte of input asks for 128 more bytes out, so 64 KiB of strip
        // asks for 8 MiB. The ceiling stops it at two.
        let bomb: Vec<u8> = (0..1 << 16).flat_map(|_| [0x81u8, 0x00]).collect();
        let mut w = Warnings::default();
        let (d, c) = packbits_bytes(&bomb, 1 << 30, &Limits::new(2), &mut w);
        assert_eq!(d.len(), 2);
        assert!(!c);
    }

    #[test]
    fn empty_input_for_a_nonempty_strip_is_truncation() {
        let (d, c, w) = unpack(&[], 8);
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn an_empty_strip_asks_for_nothing_and_reads_nothing() {
        let (d, c, w) = unpack(&[0x02, b'x', b'y', b'z'], 0);
        assert!(d.is_empty());
        assert!(c);
        assert!(w.is_empty());
    }
}
