//! ASCIIHexDecode (7.4.2) and ASCII85Decode (7.4.3).

use crate::{Limits, Warning, Warnings};

/// 7.2.3, Table 1: the six white-space characters.
const fn is_white(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}

const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn hex_bytes(input: &[u8], limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut half: Option<u8> = None;
    let mut complete = false;

    for (i, &b) in input.iter().enumerate() {
        if b == b'>' {
            // 7.4.2: a final odd digit is taken as if followed by a zero.
            if let Some(h) = half.take() {
                w.push(Warning::OddHexDigit);
                if !push_capped(&mut out, h << 4, limits, w) {
                    return (out, false);
                }
            }
            if i + 1 < input.len() {
                w.push(Warning::TrailingGarbage);
            }
            complete = true;
            break;
        }
        if is_white(b) {
            continue;
        }
        match hex_val(b) {
            Some(v) => match half {
                Some(h) => {
                    half = None;
                    if !push_capped(&mut out, (h << 4) | v, limits, w) {
                        return (out, false);
                    }
                }
                None => half = Some(v),
            },
            None => w.push(Warning::NonHexByte),
        }
    }

    if !complete {
        if let Some(h) = half {
            w.push(Warning::OddHexDigit);
            if !push_capped(&mut out, h << 4, limits, w) {
                return (out, false);
            }
        }
        w.push(Warning::EarlyEod);
    }
    (out, complete)
}

fn push_capped(out: &mut Vec<u8>, b: u8, limits: &Limits, w: &mut Warnings) -> bool {
    if out.len() >= limits.max_output {
        w.push(Warning::OutputCapHit);
        return false;
    }
    out.push(b);
    true
}

pub(crate) fn a85_bytes(input: &[u8], limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut n = 0usize;
    let mut saw_eod = false;
    let mut truncated = false;

    let mut i = 0usize;
    // Adobe's leading "<~" is not part of 7.4.3 but some producers emit it.
    if input.first() == Some(&b'<') && input.get(1) == Some(&b'~') {
        i = 2;
    }

    while let Some(&b) = input.get(i) {
        i += 1;
        if b == b'~' {
            // 7.4.3: the EOD is "~>"; a lone '~' still ends the data.
            if input.get(i).is_some_and(|&c| c == b'>') {
                i += 1;
            }
            if i < input.len() {
                w.push(Warning::TrailingGarbage);
            }
            saw_eod = true;
            break;
        }
        if is_white(b) {
            continue;
        }
        if b == b'z' {
            if n == 0 {
                if !extend_capped(&mut out, &[0, 0, 0, 0], limits, w) {
                    return (out, false);
                }
            } else {
                w.push(Warning::Ascii85ZInGroup);
            }
            continue;
        }
        // 7.4.3 has no 'y' shortcut; it is btoa's, and lands here as invalid.
        if !(b'!'..=b'u').contains(&b) {
            w.push(Warning::BadAscii85Byte);
            continue;
        }
        if let Some(slot) = group.get_mut(n) {
            *slot = b - b'!';
        }
        n += 1;
        if n == 5 {
            n = 0;
            let (bytes, len) = decode_group(&group, 5, w);
            if !extend_capped(&mut out, bytes.get(..len).unwrap_or(&bytes), limits, w) {
                return (out, false);
            }
        }
    }

    if n > 0 {
        // 7.4.3: a final group of n characters yields n-1 bytes.
        if n == 1 {
            w.push(Warning::TruncatedInput);
            truncated = true;
        } else {
            for slot in group.iter_mut().skip(n) {
                *slot = 84;
            }
            let (bytes, len) = decode_group(&group, n, w);
            if !extend_capped(&mut out, bytes.get(..len).unwrap_or(&bytes), limits, w) {
                return (out, false);
            }
        }
    }
    if !saw_eod {
        w.push(Warning::EarlyEod);
    }
    (out, saw_eod && !truncated)
}

fn decode_group(group: &[u8; 5], n: usize, w: &mut Warnings) -> ([u8; 4], usize) {
    let mut v: u64 = 0;
    for &d in group {
        v = v * 85 + u64::from(d);
    }
    if v > u64::from(u32::MAX) {
        w.push(Warning::Ascii85Overflow);
        v = u64::from(u32::MAX);
    }
    ((v as u32).to_be_bytes(), n.saturating_sub(1).min(4))
}

fn extend_capped(out: &mut Vec<u8>, bytes: &[u8], limits: &Limits, w: &mut Warnings) -> bool {
    let room = limits.max_output.saturating_sub(out.len());
    let n = bytes.len().min(room);
    if let Some(s) = bytes.get(..n) {
        out.extend_from_slice(s);
    }
    if n < bytes.len() {
        w.push(Warning::OutputCapHit);
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Limits = Limits::new(1 << 20);

    fn hex(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = hex_bytes(input, &CAP, &mut w);
        (d, c, w.into_vec())
    }

    fn a85(input: &[u8]) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = a85_bytes(input, &CAP, &mut w);
        (d, c, w.into_vec())
    }

    #[test]
    fn hex_decodes_pairs() {
        let (d, c, w) = hex(b"48656C6C6F>");
        assert_eq!(d, b"Hello");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn hex_is_case_insensitive_and_skips_white_space() {
        let (d, c, w) = hex(b" 4 8 65\n6c\r6C\t6f >");
        assert_eq!(d, b"Hello");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn hex_odd_final_digit_implies_a_zero() {
        let (d, c, w) = hex(b"4865C>");
        assert_eq!(d, vec![0x48, 0x65, 0xc0]);
        assert!(c);
        assert_eq!(w, vec![Warning::OddHexDigit]);
    }

    #[test]
    fn hex_skips_invalid_bytes_with_one_warning() {
        let (d, c, w) = hex(b"48zz65!!6C6C6F>");
        assert_eq!(d, b"Hello");
        assert!(c);
        assert_eq!(w, vec![Warning::NonHexByte]);
    }

    #[test]
    fn hex_missing_eod_is_tolerated() {
        let (d, c, w) = hex(b"48656C6C6F");
        assert_eq!(d, b"Hello");
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn hex_missing_eod_with_an_odd_digit() {
        let (d, c, w) = hex(b"486");
        assert_eq!(d, vec![0x48, 0x60]);
        assert!(!c);
        assert_eq!(w, vec![Warning::OddHexDigit, Warning::EarlyEod]);
    }

    #[test]
    fn hex_ignores_bytes_after_the_eod() {
        let (d, c, w) = hex(b"48>656C");
        assert_eq!(d, vec![0x48]);
        assert!(c);
        assert_eq!(w, vec![Warning::TrailingGarbage]);
    }

    #[test]
    fn hex_empty_and_eod_only() {
        let (d, c, w) = hex(b">");
        assert!(d.is_empty());
        assert!(c);
        assert!(w.is_empty());
        let (d, c, w) = hex(b"");
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn hex_respects_the_output_cap() {
        let limits = Limits::new(2);
        let mut w = Warnings::default();
        let (d, c) = hex_bytes(b"41424344>", &limits, &mut w);
        assert_eq!(d, b"AB");
        assert!(!c);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }

    #[test]
    fn a85_decodes_a_full_group() {
        // "Man " is the canonical group: 0x4d616e20 -> "9jqo^" prefix.
        let (d, c, w) = a85(b"9jqo^~>");
        assert_eq!(d, b"Man ");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn a85_partial_group_yields_one_byte_less() {
        let (d, c, _) = a85(b"9jqo~>");
        assert_eq!(d, b"Man");
        assert!(c);
        let (d, c, _) = a85(b"9jq~>");
        assert_eq!(d, b"Ma");
        assert!(c);
        let (d, c, _) = a85(b"9j~>");
        assert_eq!(d, b"M");
        assert!(c);
    }

    #[test]
    fn a85_single_leftover_character_is_dropped() {
        let (d, c, w) = a85(b"9jqo^9~>");
        assert_eq!(d, b"Man ");
        assert!(!c);
        assert!(w.contains(&Warning::TruncatedInput));
    }

    #[test]
    fn a85_z_is_four_zero_bytes() {
        let (d, c, w) = a85(b"z~>");
        assert_eq!(d, vec![0, 0, 0, 0]);
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn a85_z_inside_a_group_is_skipped_with_a_warning() {
        let (d, c, w) = a85(b"9jzqo^~>");
        assert_eq!(d, b"Man ");
        assert!(c);
        assert_eq!(w, vec![Warning::Ascii85ZInGroup]);
    }

    #[test]
    fn a85_has_no_y_shortcut() {
        // 'y' is btoa's four-space shortcut, not 7.4.3's.
        let (d, c, w) = a85(b"y9jqo^~>");
        assert_eq!(d, b"Man ");
        assert!(c);
        assert_eq!(w, vec![Warning::BadAscii85Byte]);
    }

    #[test]
    fn a85_white_space_is_skipped() {
        let (d, c, w) = a85(b"9j\nq o\t^\r~>");
        assert_eq!(d, b"Man ");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn a85_overflowing_group_is_clamped() {
        // "uuuuu" is 85^4*84... well above 2^32-1.
        let (d, c, w) = a85(b"uuuuu~>");
        assert_eq!(d, vec![0xff, 0xff, 0xff, 0xff]);
        assert!(c);
        assert_eq!(w, vec![Warning::Ascii85Overflow]);
    }

    #[test]
    fn a85_missing_eod_decodes_to_the_end() {
        let (d, c, w) = a85(b"9jqo^");
        assert_eq!(d, b"Man ");
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
    }

    #[test]
    fn a85_accepts_the_adobe_prefix_and_ignores_the_tail() {
        let (d, c, w) = a85(b"<~9jqo^~>rest");
        assert_eq!(d, b"Man ");
        assert!(c);
        assert_eq!(w, vec![Warning::TrailingGarbage]);
    }

    #[test]
    fn a85_respects_the_output_cap() {
        let limits = Limits::new(2);
        let mut w = Warnings::default();
        let (d, c) = a85_bytes(b"9jqo^9jqo^~>", &limits, &mut w);
        assert_eq!(d, b"Ma");
        assert!(!c);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }

    #[test]
    fn a85_empty_input() {
        let (d, c, w) = a85(b"");
        assert!(d.is_empty());
        assert!(!c);
        assert_eq!(w, vec![Warning::EarlyEod]);
        let (d, c, w) = a85(b"~>");
        assert!(d.is_empty());
        assert!(c);
        assert!(w.is_empty());
    }
}
