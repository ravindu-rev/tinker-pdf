//! LZWDecode (7.4.4): 9→12-bit variable codes, MSB first.

use crate::{Limits, Warning, Warnings};

const CLEAR: u16 = 256;
const EOD: u16 = 257;
const FIRST: u16 = 258;
const MAX_ENTRIES: usize = 4096;

pub(crate) fn lzw_bytes(
    input: &[u8],
    limits: &Limits,
    early_change: bool,
    w: &mut Warnings,
) -> (Vec<u8>, bool) {
    let mut prefix = vec![0u16; MAX_ENTRIES];
    let mut suffix = vec![0u8; MAX_ENTRIES];
    let mut scratch: Vec<u8> = Vec::new();
    let mut out = Vec::new();

    // 7.4.4.2: with /EarlyChange 1 (the default) the code width grows one
    // code before the table would otherwise require it.
    let early = u32::from(early_change);
    let total_bits = input.len() as u64 * 8;
    let mut bitpos = 0u64;
    let mut width = 9u32;
    let mut next = FIRST;
    let mut prev: Option<u16> = None;

    loop {
        let Some(code) = read_code(input, bitpos, total_bits, width) else {
            w.push(Warning::EarlyEod);
            return (out, false);
        };
        bitpos += u64::from(width);

        if code == CLEAR {
            next = FIRST;
            width = 9;
            prev = None;
            continue;
        }
        if code == EOD {
            if total_bits - bitpos >= 8 {
                w.push(Warning::TrailingGarbage);
            }
            return (out, true);
        }

        let known = code < 256 || (code >= FIRST && code < next);
        if known {
            if !expand(code, &prefix, &suffix, &mut scratch) {
                w.push(Warning::BadLzwCode);
                return (out, false);
            }
        } else if code == next {
            // KwKwK: the encoder used an entry it defined with this very
            // code, so the string is the previous one plus its first byte.
            let Some(p) = prev else {
                w.push(Warning::BadLzwCode);
                return (out, false);
            };
            if !expand(p, &prefix, &suffix, &mut scratch) {
                w.push(Warning::BadLzwCode);
                return (out, false);
            }
            match scratch.first().copied() {
                Some(f) => scratch.push(f),
                None => {
                    w.push(Warning::BadLzwCode);
                    return (out, false);
                }
            }
        } else {
            w.push(Warning::BadLzwCode);
            return (out, false);
        }

        let room = limits.max_output.saturating_sub(out.len());
        let n = scratch.len().min(room);
        if let Some(s) = scratch.get(..n) {
            out.extend_from_slice(s);
        }
        if n < scratch.len() {
            w.push(Warning::OutputCapHit);
            return (out, false);
        }

        if let (Some(p), Some(&f)) = (prev, scratch.first()) {
            if (next as usize) < MAX_ENTRIES {
                let i = next as usize;
                if let (Some(pe), Some(se)) = (prefix.get_mut(i), suffix.get_mut(i)) {
                    *pe = p;
                    *se = f;
                }
                next += 1;
                if width < 12 && u32::from(next) + early >= 1u32 << width {
                    width += 1;
                }
            } else {
                // A full table without a Clear: freeze it and keep going,
                // which is what shipping readers do.
                w.push(Warning::LzwTableFull);
            }
        }
        prev = Some(code);
    }
}

/// Walks a code's chain into `scratch`, in order. `false` means the chain is
/// broken or cyclic.
fn expand(code: u16, prefix: &[u16], suffix: &[u8], scratch: &mut Vec<u8>) -> bool {
    scratch.clear();
    let mut c = code;
    loop {
        if scratch.len() > MAX_ENTRIES {
            return false;
        }
        if c < 256 {
            scratch.push(c as u8);
            break;
        }
        if c < FIRST {
            return false;
        }
        let i = c as usize;
        match (suffix.get(i), prefix.get(i)) {
            (Some(&s), Some(&p)) => {
                scratch.push(s);
                c = p;
            }
            _ => return false,
        }
    }
    scratch.reverse();
    true
}

fn read_code(input: &[u8], bitpos: u64, total_bits: u64, width: u32) -> Option<u16> {
    if bitpos + u64::from(width) > total_bits {
        return None;
    }
    let byte = (bitpos / 8) as usize;
    let off = (bitpos % 8) as u32;
    let b0 = u32::from(*input.get(byte)?);
    let b1 = u32::from(input.get(byte + 1).copied().unwrap_or(0));
    let b2 = u32::from(input.get(byte + 2).copied().unwrap_or(0));
    // width <= 12 and off <= 7, so a code never spans more than three bytes.
    let chunk = (b0 << 16) | (b1 << 8) | b2;
    let shift = 24 - off - width;
    Some(((chunk >> shift) & ((1u32 << width) - 1)) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Limits = Limits::new(1 << 20);

    fn lzw(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = lzw_bytes(input, &CAP, true, &mut w);
        (d, c, w.into_vec())
    }

    /// Packs (code, width) pairs MSB-first, zero-padded to a byte.
    fn pack(codes: &[(u16, u32)]) -> Vec<u8> {
        let mut bits: Vec<u8> = Vec::new();
        for &(c, wd) in codes {
            for i in (0..wd).rev() {
                bits.push(((u32::from(c) >> i) & 1) as u8);
            }
        }
        while bits.len() % 8 != 0 {
            bits.push(0);
        }
        bits.chunks(8)
            .map(|c| c.iter().fold(0u8, |a, &b| (a << 1) | b))
            .collect()
    }

    #[test]
    fn spec_example_round_trips() {
        // 7.4.4.2, Table 8: "-----A---B" encodes to these nine bytes, and
        // its third code is the KwKwK case.
        let enc = [0x80, 0x0b, 0x60, 0x50, 0x22, 0x0c, 0x0c, 0x85, 0x01];
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"-----A---B");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn single_byte_codes_decode() {
        let enc = pack(&[
            (256, 9),
            (u16::from(b'A'), 9),
            (u16::from(b'B'), 9),
            (257, 9),
        ]);
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"AB");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn a_clear_code_resets_the_table() {
        let enc = pack(&[
            (256, 9),
            (u16::from(b'A'), 9),
            (u16::from(b'B'), 9),
            (256, 9),
            (u16::from(b'C'), 9),
            (257, 9),
        ]);
        let (d, c, _) = lzw(&enc);
        assert_eq!(d, b"ABC");
        assert!(c);
    }

    #[test]
    fn missing_leading_clear_is_tolerated() {
        let enc = pack(&[(u16::from(b'A'), 9), (u16::from(b'B'), 9), (257, 9)]);
        let (d, c, _) = lzw(&enc);
        assert_eq!(d, b"AB");
        assert!(c);
    }

    #[test]
    fn out_of_range_code_stops_with_a_warning() {
        let enc = pack(&[(256, 9), (u16::from(b'A'), 9), (300, 9), (257, 9)]);
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"A");
        assert!(!c);
        assert_eq!(w, vec![Warning::BadLzwCode]);
    }

    #[test]
    fn kwkwk_without_a_previous_code_is_rejected() {
        let enc = pack(&[(256, 9), (258, 9)]);
        let (d, c, w) = lzw(&enc);
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::BadLzwCode]);
    }

    #[test]
    fn missing_eod_is_reported() {
        let enc = pack(&[(256, 9), (u16::from(b'A'), 9)]);
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"A");
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn truncating_the_spec_stream_keeps_the_prefix() {
        let enc = [0x80, 0x0b, 0x60, 0x50];
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"---");
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn empty_input_warns_once() {
        let (d, c, w) = lzw(b"");
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn bytes_after_the_eod_are_ignored() {
        let mut enc = pack(&[(256, 9), (u16::from(b'A'), 9), (257, 9)]);
        enc.extend_from_slice(b"junk");
        let (d, c, w) = lzw(&enc);
        assert_eq!(d, b"A");
        assert!(c);
        assert_eq!(w, vec![Warning::TrailingGarbage]);
    }

    #[test]
    fn the_width_grows_one_code_early_by_default() {
        // 510 single-byte codes fill entries up to 511, which is where
        // /EarlyChange 1 switches to ten bits and 0 does not.
        let mut codes = vec![(256u16, 9u32)];
        for i in 0..254u16 {
            codes.push((i, 9));
        }
        // entries 258..=511 now exist: the next code is ten bits wide with
        // early change, still nine without.
        let early_on = {
            let mut c = codes.clone();
            c.push((u16::from(b'Z'), 10));
            c.push((257, 10));
            pack(&c)
        };
        let early_off = {
            let mut c = codes.clone();
            c.push((u16::from(b'Z'), 9));
            c.push((257, 10));
            pack(&c)
        };
        let mut w = Warnings::default();
        let (d, c) = lzw_bytes(&early_on, &CAP, true, &mut w);
        assert!(c, "{:?}", w.clone().into_vec());
        assert_eq!(d.last(), Some(&b'Z'));
        let mut w = Warnings::default();
        let (d, c) = lzw_bytes(&early_off, &CAP, false, &mut w);
        assert!(c);
        assert_eq!(d.last(), Some(&b'Z'));
    }

    #[test]
    fn a_full_table_without_a_clear_freezes_and_continues() {
        let mut codes = vec![(256u16, 9u32)];
        let mut width = 9u32;
        let mut next = FIRST;
        let mut first = true;
        for _ in 0..4000 {
            codes.push((0, width));
            if first {
                first = false;
            } else if (next as usize) < MAX_ENTRIES {
                next += 1;
                if width < 12 && u32::from(next) + 1 >= 1u32 << width {
                    width += 1;
                }
            }
        }
        codes.push((257, width));
        let (d, c, w) = lzw(&pack(&codes));
        assert_eq!(d.len(), 4000);
        assert!(d.iter().all(|&b| b == 0));
        assert!(c);
        assert_eq!(w, vec![Warning::LzwTableFull]);
    }

    #[test]
    fn output_cap_truncates() {
        let limits = Limits::new(3);
        let mut w = Warnings::default();
        let enc = [0x80, 0x0b, 0x60, 0x50, 0x22, 0x0c, 0x0c, 0x85, 0x01];
        let (d, c) = lzw_bytes(&enc, &limits, true, &mut w);
        assert_eq!(d, b"---");
        assert!(!c);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }
}
