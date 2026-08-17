//! Entry names: CP437 by default, UTF-8 when the archive says so.
//!
//! APPNOTE 4.4.4 gives general-purpose bit 11 as the only thing that
//! distinguishes them, and D.1 says the default is "the original IBM PC
//! character encoding set, commonly referred to as IBM Code Page 437". That
//! default is not a fiction to be replaced with Latin-1: every byte from 0x80
//! up differs between the two, so a German archive written by a DOS-era tool
//! reads `Bücher` as `B�cher` under Latin-1 and correctly under CP437, and
//! the two are indistinguishable without the table.
//!
//! The table below is the upper half only. The lower half is ASCII in both
//! encodings — CP437 does assign glyphs to 0x00..=0x1F, but those are the
//! control-picture aliases, and a filename holding a raw control byte means
//! the control byte.
//!
//! The table is written as `\u{...}` escapes rather than as the characters
//! themselves, so a reviewer checks code points against the published page
//! rather than trusting a terminal font to distinguish U+2561 from U+2562.

/// CP437, 0x80..=0xFF.
const HIGH: [char; 128] = [
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}',
    '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}',
    '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}',
    '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
    '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
    '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}',
    '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}',
    '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}',
    '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
    '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}',
    '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
    '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
    '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];

/// Every byte maps to a character, so this is total and cannot fail.
#[must_use]
pub(crate) fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else {
            out.push(HIGH[usize::from(b) - 0x80]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six code points chosen because each one is wrong under a plausible
    /// substitute. `0xE1` is `ß` in CP437 and `á` in Latin-1; `0xFF` is a
    /// non-breaking space here and `ÿ` there; `0x9E` is the peseta sign, which
    /// no Latin-1 has at all. A table transcribed from the wrong page passes a
    /// test that only checks ASCII.
    #[test]
    fn the_high_half_is_cp437_and_not_latin_1() {
        assert_eq!(decode(&[0x81]), "\u{00FC}"); // u-umlaut
        assert_eq!(decode(&[0x9E]), "\u{20A7}"); // peseta
        assert_eq!(decode(&[0xE1]), "\u{00DF}"); // sharp s, not a-acute
        assert_eq!(decode(&[0xFF]), "\u{00A0}"); // no-break space, not y-umlaut
        assert_eq!(decode(&[0xB3]), "\u{2502}"); // box drawing
        assert_eq!(decode(&[0xFE]), "\u{25A0}"); // black square
    }

    /// The lower half is ASCII, including the bytes CP437 also draws glyphs
    /// for. A name holding `0x01` means `0x01`, not a smiling face.
    #[test]
    fn the_low_half_is_ascii_including_the_control_bytes() {
        assert_eq!(decode(b"page01.jpg"), "page01.jpg");
        assert_eq!(decode(&[0x01, 0x1f, 0x7f]), "\u{0001}\u{001f}\u{007f}");
    }

    /// Total over every byte: 256 inputs, 256 characters, no panic and no
    /// replacement character invented along the way.
    #[test]
    fn every_byte_decodes_to_exactly_one_character() {
        for b in 0u16..=255 {
            let s = decode(&[b as u8]);
            assert_eq!(s.chars().count(), 1, "{b:#04x}");
            assert_ne!(s, "\u{FFFD}", "{b:#04x}");
        }
        let all: Vec<u8> = (0u16..=255).map(|b| b as u8).collect();
        assert_eq!(decode(&all).chars().count(), 256);
    }

    /// Distinctness, which a transcription error breaks without breaking any
    /// single-value assertion: CP437 is a bijection onto 256 code points, so
    /// two entries that ended up the same are a copy-paste slip that spot
    /// checks cannot see.
    #[test]
    fn the_table_maps_no_two_bytes_to_one_character() {
        let mut seen: Vec<char> = (0u16..=255).map(|b| decode(&[b as u8]).remove(0)).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), before, "two bytes decode to the same character");
    }
}
