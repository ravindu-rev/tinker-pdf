//! RunLengthDecode (7.4.5).

use crate::{Limits, Warning, Warnings};

pub(crate) fn rle_bytes(input: &[u8], limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut i = 0usize;

    while let Some(&tag) = input.get(i) {
        i += 1;
        // 7.4.5: 128 is EOD, 0..=127 a literal run of tag+1, 129..=255 a
        // repeat of the next byte 257-tag times.
        if tag == 128 {
            if i < input.len() {
                w.push(Warning::TrailingGarbage);
            }
            return (out, true);
        }
        if tag < 128 {
            let want = tag as usize + 1;
            let avail = input.len() - i;
            let take = want.min(avail);
            let room = limits.max_output.saturating_sub(out.len());
            let n = take.min(room);
            if let Some(s) = input.get(i..i + n) {
                out.extend_from_slice(s);
            }
            i += take;
            if n < take {
                w.push(Warning::OutputCapHit);
                return (out, false);
            }
            if take < want {
                w.push(Warning::TruncatedInput);
                return (out, false);
            }
        } else {
            let Some(&b) = input.get(i) else {
                w.push(Warning::TruncatedInput);
                return (out, false);
            };
            i += 1;
            let want = 257 - tag as usize;
            let room = limits.max_output.saturating_sub(out.len());
            let n = want.min(room);
            out.resize(out.len() + n, b);
            if n < want {
                w.push(Warning::OutputCapHit);
                return (out, false);
            }
        }
    }

    w.push(Warning::EarlyEod);
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Limits = Limits::new(1 << 20);

    fn rle(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = rle_bytes(input, &CAP, &mut w);
        (d, c, w.into_vec())
    }

    #[test]
    fn literal_run_copies_length_plus_one() {
        let (d, c, w) = rle(&[2, b'a', b'b', b'c', 128]);
        assert_eq!(d, b"abc");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn repeat_run_expands_257_minus_length() {
        let (d, c, w) = rle(&[254, b'x', 128]);
        assert_eq!(d, b"xxx");
        assert!(c);
        assert!(w.is_empty());
        // 255 is the shortest repeat: two copies.
        let (d, _, _) = rle(&[255, b'y', 128]);
        assert_eq!(d, b"yy");
        // 129 is the longest: 128 copies.
        let (d, _, _) = rle(&[129, b'z', 128]);
        assert_eq!(d.len(), 128);
        assert!(d.iter().all(|&b| b == b'z'));
    }

    #[test]
    fn runs_mix() {
        let (d, c, _) = rle(&[0, b'A', 254, b'B', 1, b'C', b'D', 128]);
        assert_eq!(d, b"ABBBCD");
        assert!(c);
    }

    #[test]
    fn eod_only_and_empty() {
        let (d, c, w) = rle(&[128]);
        assert!(d.is_empty());
        assert!(c);
        assert!(w.is_empty());
        let (d, c, w) = rle(&[]);
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn missing_eod_is_tolerated() {
        let (d, c, w) = rle(&[2, b'a', b'b', b'c']);
        assert_eq!(d, b"abc");
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn truncated_literal_run_keeps_what_is_there() {
        let (d, c, w) = rle(&[5, b'a', b'b']);
        assert_eq!(d, b"ab");
        assert!(!c);
        assert_eq!(w, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn truncated_repeat_run_has_no_byte_to_repeat() {
        let (d, c, w) = rle(&[1, b'a', b'b', 200]);
        assert_eq!(d, b"ab");
        assert!(!c);
        assert_eq!(w, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn bytes_after_eod_are_ignored() {
        let (d, c, w) = rle(&[0, b'A', 128, b'j', b'u', b'n', b'k']);
        assert_eq!(d, b"A");
        assert!(c);
        assert_eq!(w, vec![Warning::TrailingGarbage]);
    }

    #[test]
    fn output_cap_stops_a_literal_run() {
        let limits = Limits::new(2);
        let mut w = Warnings::default();
        let (d, c) = rle_bytes(&[3, b'a', b'b', b'c', b'd', 128], &limits, &mut w);
        assert_eq!(d, b"ab");
        assert!(!c);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }

    #[test]
    fn output_cap_stops_a_repeat_run() {
        let limits = Limits::new(3);
        let mut w = Warnings::default();
        let (d, c) = rle_bytes(&[129, b'z', 128], &limits, &mut w);
        assert_eq!(d, b"zzz");
        assert!(!c);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }
}
