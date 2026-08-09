//! Text strings (7.9.2.2) and the PDFDocEncoding table of Annex D.
//!
//! Every feature in this layer decodes text strings — titles, labels,
//! filenames, `/Info` values — so the helper comes first and is written once.
//! Undefined code points become U+FFFD rather than an error: a mangled title
//! must not sink a document.

/// What an undefined code point or a broken surrogate decodes to.
pub const REPLACEMENT: char = '\u{FFFD}';

/// D.2: codes 24 to 31, where PDFDocEncoding puts accents that ISO 8859-1
/// leaves as control characters.
const ACCENTS: [char; 8] = [
    '\u{02D8}', // 24 breve
    '\u{02C7}', // 25 caron
    '\u{02C6}', // 26 circumflex
    '\u{02D9}', // 27 dotaccent
    '\u{02DD}', // 28 hungarumlaut
    '\u{02DB}', // 29 ogonek
    '\u{02DA}', // 30 ring
    '\u{02DC}', // 31 tilde
];

/// D.2: codes 127 to 160. [`REPLACEMENT`] marks the codes Annex D leaves
/// undefined, which is every entry with no character in the PDFDocEncoding
/// column.
const HIGH: [char; 34] = [
    REPLACEMENT, // 127 undefined
    '\u{2022}',  // 128 bullet
    '\u{2020}',  // 129 dagger
    '\u{2021}',  // 130 daggerdbl
    '\u{2026}',  // 131 ellipsis
    '\u{2014}',  // 132 emdash
    '\u{2013}',  // 133 endash
    '\u{0192}',  // 134 florin
    '\u{2044}',  // 135 fraction
    '\u{2039}',  // 136 guilsinglleft
    '\u{203A}',  // 137 guilsinglright
    '\u{2212}',  // 138 minus
    '\u{2030}',  // 139 perthousand
    '\u{201E}',  // 140 quotedblbase
    '\u{201C}',  // 141 quotedblleft
    '\u{201D}',  // 142 quotedblright
    '\u{2018}',  // 143 quoteleft
    '\u{2019}',  // 144 quoteright
    '\u{201A}',  // 145 quotesinglbase
    '\u{2122}',  // 146 trademark
    '\u{FB01}',  // 147 fi
    '\u{FB02}',  // 148 fl
    '\u{0141}',  // 149 Lslash
    '\u{0152}',  // 150 OE
    '\u{0160}',  // 151 Scaron
    '\u{0178}',  // 152 Ydieresis
    '\u{017D}',  // 153 Zcaron
    '\u{0131}',  // 154 dotlessi
    '\u{0142}',  // 155 lslash
    '\u{0153}',  // 156 oe
    '\u{0161}',  // 157 scaron
    '\u{017E}',  // 158 zcaron
    REPLACEMENT, // 159 undefined
    '\u{20AC}',  // 160 Euro
];

/// The character one PDFDocEncoding code stands for (Annex D.2).
///
/// Outside the three blocks the table redefines, PDFDocEncoding is ISO 8859-1,
/// whose code points are their own Unicode scalars.
pub fn pdf_doc_encoding_char(code: u8) -> char {
    match code {
        0x18..=0x1F => ACCENTS
            .get(usize::from(code.wrapping_sub(0x18)))
            .copied()
            .unwrap_or(REPLACEMENT),
        0x7F..=0xA0 => HIGH
            .get(usize::from(code.wrapping_sub(0x7F)))
            .copied()
            .unwrap_or(REPLACEMENT),
        // D.2 leaves 173 undefined, where ISO 8859-1 has a soft hyphen.
        0xAD => REPLACEMENT,
        _ => char::from(code),
    }
}

/// Decodes a text string (7.9.2.2).
///
/// `FE FF` selects UTF-16BE; `EF BB BF` selects UTF-8, which is a PDF 2.0
/// addition accepted on read; anything else is PDFDocEncoding. Nothing here
/// fails: an undecodable code becomes [`REPLACEMENT`].
pub fn decode_text_string(bytes: &[u8]) -> String {
    decode(bytes).0
}

/// [`decode_text_string`] plus whether anything had to be replaced, so the
/// document layer can attach a warning to the object it came from (ruling 10).
pub(crate) fn decode(bytes: &[u8]) -> (String, bool) {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16be(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return match core::str::from_utf8(rest) {
            Ok(text) => (text.to_owned(), false),
            Err(_) => (String::from_utf8_lossy(rest).into_owned(), true),
        };
    }
    decode_pdf_doc(bytes)
}

fn decode_utf16be(bytes: &[u8]) -> (String, bool) {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        if let [hi, lo] = pair {
            units.push(u16::from_be_bytes([*hi, *lo]));
        }
    }
    let mut replaced = bytes.len() % 2 != 0;
    let mut out = String::with_capacity(units.len());
    for unit in char::decode_utf16(units) {
        match unit {
            Ok(c) => out.push(c),
            // A lone surrogate is legal UTF-16 nowhere; it reads as U+FFFD.
            Err(_) => {
                out.push(REPLACEMENT);
                replaced = true;
            }
        }
    }
    (out, replaced)
}

fn decode_pdf_doc(bytes: &[u8]) -> (String, bool) {
    let mut out = String::with_capacity(bytes.len());
    let mut replaced = false;
    for &code in bytes {
        let c = pdf_doc_encoding_char(code);
        replaced |= c == REPLACEMENT;
        out.push(c);
    }
    (out, replaced)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_itself_in_pdfdocencoding() {
        assert_eq!(decode_text_string(b"Hello, world"), "Hello, world");
        for code in 0x20u8..0x7F {
            assert_eq!(pdf_doc_encoding_char(code), char::from(code));
        }
    }

    #[test]
    fn annex_d_accent_block() {
        let table = [
            (0x18u8, '\u{02D8}'),
            (0x19, '\u{02C7}'),
            (0x1A, '\u{02C6}'),
            (0x1B, '\u{02D9}'),
            (0x1C, '\u{02DD}'),
            (0x1D, '\u{02DB}'),
            (0x1E, '\u{02DA}'),
            (0x1F, '\u{02DC}'),
        ];
        for (code, expected) in table {
            assert_eq!(pdf_doc_encoding_char(code), expected, "code {code:#04x}");
        }
    }

    #[test]
    fn annex_d_high_block() {
        let table = [
            (0x7Fu8, REPLACEMENT),
            (0x80, '\u{2022}'),
            (0x85, '\u{2013}'),
            (0x86, '\u{0192}'),
            (0x8A, '\u{2212}'),
            (0x92, '\u{2122}'),
            (0x93, '\u{FB01}'),
            (0x96, '\u{0152}'),
            (0x9A, '\u{0131}'),
            (0x9E, '\u{017E}'),
            (0x9F, REPLACEMENT),
            (0xA0, '\u{20AC}'),
            (0xA1, '\u{00A1}'),
            (0xAD, REPLACEMENT),
            (0xAE, '\u{00AE}'),
            (0xFF, '\u{00FF}'),
        ];
        for (code, expected) in table {
            assert_eq!(pdf_doc_encoding_char(code), expected, "code {code:#04x}");
        }
    }

    #[test]
    fn latin1_holds_outside_the_redefined_blocks() {
        for code in 0u8..=0xFF {
            let redefined = matches!(code, 0x18..=0x1F | 0x7F..=0xA0 | 0xAD);
            if !redefined {
                assert_eq!(pdf_doc_encoding_char(code), char::from(code));
            }
        }
    }

    #[test]
    fn utf16be_needs_its_bom() {
        // Without the BOM the same bytes are PDFDocEncoding, not UTF-16.
        assert_eq!(decode_text_string(&[0x00, 0x41]), "\u{0}A");
        assert_eq!(decode_text_string(&[0xFE, 0xFF, 0x00, 0x41]), "A");
    }

    #[test]
    fn utf16be_decodes_surrogate_pairs() {
        let bytes = [0xFE, 0xFF, 0xD8, 0x3D, 0xDE, 0x00];
        assert_eq!(decode_text_string(&bytes), "\u{1F600}");
        assert!(!decode(&bytes).1);
    }

    #[test]
    fn a_lone_surrogate_is_replaced_not_fatal() {
        let (text, replaced) = decode(&[0xFE, 0xFF, 0xD8, 0x3D, 0x00, 0x41]);
        assert_eq!(text, "\u{FFFD}A");
        assert!(replaced);
    }

    #[test]
    fn an_odd_utf16_tail_is_dropped_and_reported() {
        let (text, replaced) = decode(&[0xFE, 0xFF, 0x00, 0x41, 0x00]);
        assert_eq!(text, "A");
        assert!(replaced);
    }

    #[test]
    fn an_empty_utf16_string_is_empty_not_absent() {
        assert_eq!(decode_text_string(&[0xFE, 0xFF]), "");
    }

    #[test]
    fn a_utf8_bom_is_honoured_as_a_pdf20_delta() {
        let bytes = [0xEF, 0xBB, 0xBF, 0xC3, 0xA9];
        assert_eq!(decode_text_string(&bytes), "é");
    }

    #[test]
    fn undefined_pdfdocencoding_codes_are_reported() {
        let (text, replaced) = decode(&[0x41, 0xAD, 0x42]);
        assert_eq!(text, "A\u{FFFD}B");
        assert!(replaced);
    }
}
