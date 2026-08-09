//! Text strings and dates (7.9.2, 7.9.4).
//!
//! A PDF string is bytes. Turning it into text needs the document's own rules:
//! UTF-16BE when a byte-order mark says so, UTF-8 when PDF 2.0's mark does,
//! and otherwise PDFDocEncoding — which is Latin-1 with a different set of
//! characters in the 0x18..0x20 range and a handful of substitutions above
//! 0x7F.

/// PDFDocEncoding for the bytes that differ from Latin-1 (Annex D.2).
///
/// Codes 0x00..0x17, 0x20..0x7E and 0xA0..0xFF map to themselves as Latin-1
/// does; only these two ranges need a table.
const PDF_DOC_18_1F: [char; 8] = [
    '\u{02D8}', // 0x18 breve
    '\u{02C7}', // 0x19 caron
    '\u{02C6}', // 0x1A modifier circumflex
    '\u{02D9}', // 0x1B dot above
    '\u{02DD}', // 0x1C double acute
    '\u{02DB}', // 0x1D ogonek
    '\u{02DA}', // 0x1E ring above
    '\u{02DC}', // 0x1F small tilde
];

const PDF_DOC_80_9F: [char; 32] = [
    '\u{2022}', // 0x80 bullet
    '\u{2020}', // 0x81 dagger
    '\u{2021}', // 0x82 double dagger
    '\u{2026}', // 0x83 ellipsis
    '\u{2014}', // 0x84 em dash
    '\u{2013}', // 0x85 en dash
    '\u{0192}', // 0x86 florin
    '\u{2044}', // 0x87 fraction slash
    '\u{2039}', // 0x88 single left guillemet
    '\u{203A}', // 0x89 single right guillemet
    '\u{2212}', // 0x8A minus
    '\u{2030}', // 0x8B per mille
    '\u{201E}', // 0x8C double low quote
    '\u{201C}', // 0x8D left double quote
    '\u{201D}', // 0x8E right double quote
    '\u{2018}', // 0x8F left single quote
    '\u{2019}', // 0x90 right single quote
    '\u{201A}', // 0x91 single low quote
    '\u{2122}', // 0x92 trade mark
    '\u{FB01}', // 0x93 fi ligature
    '\u{FB02}', // 0x94 fl ligature
    '\u{0141}', // 0x95 L with stroke
    '\u{0152}', // 0x96 OE
    '\u{0160}', // 0x97 S with caron
    '\u{0178}', // 0x98 Y with diaeresis
    '\u{017D}', // 0x99 Z with caron
    '\u{0131}', // 0x9A dotless i
    '\u{0142}', // 0x9B l with stroke
    '\u{0153}', // 0x9C oe
    '\u{0161}', // 0x9D s with caron
    '\u{017E}', // 0x9E z with caron
    '\u{FFFD}', // 0x9F undefined
];

/// Decodes a text string (7.9.2.2).
///
/// Never fails: an unpaired surrogate or a truncated code unit becomes
/// U+FFFD, because a document's title is worth showing imperfectly.
#[must_use]
pub fn decode_text_string(bytes: &[u8]) -> String {
    // 7.9.2.2: UTF-16BE is announced by FE FF, and PDF 2.0's UTF-8 by
    // EF BB BF. Anything else is PDFDocEncoded.
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16be(rest);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    decode_pdf_doc(bytes)
}

fn decode_utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let mut out = String::from_utf16_lossy(&units);
    if bytes.len() % 2 != 0 {
        out.push('\u{FFFD}');
    }
    out
}

/// Decodes PDFDocEncoding (Annex D.2).
#[must_use]
pub fn decode_pdf_doc(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x18..=0x1F => PDF_DOC_18_1F
                .get(usize::from(b - 0x18))
                .copied()
                .unwrap_or('\u{FFFD}'),
            0x80..=0x9F => PDF_DOC_80_9F
                .get(usize::from(b - 0x80))
                .copied()
                .unwrap_or('\u{FFFD}'),
            // The rest coincides with Latin-1.
            _ => char::from(b),
        })
        .collect()
}

/// A date from a `D:` string (7.9.4), with everything after the year optional
/// because producers truncate wherever they please.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Date {
    /// Four-digit year.
    pub year: i32,
    /// 1..=12; 1 when absent, as the specification's defaults say.
    pub month: u8,
    /// 1..=31.
    pub day: u8,
    /// 0..=23.
    pub hour: u8,
    /// 0..=59.
    pub minute: u8,
    /// 0..=59.
    pub second: u8,
    /// Offset from UT in minutes, or `None` for an unspecified zone.
    pub utc_offset_minutes: Option<i32>,
}

/// Parses a PDF date string (7.9.4).
///
/// Leniency, all of it seen in real files: the `D:` prefix is optional, any
/// trailing component may be missing, the apostrophes in the zone offset may
/// be absent or doubled, and trailing junk is ignored. Returns `None` only
/// when there is not even a year.
#[must_use]
pub fn parse_date(text: &str) -> Option<Date> {
    let s = text.trim();
    let s = s.strip_prefix("D:").unwrap_or(s);
    let digits: Vec<u8> = s.bytes().collect();

    let num = |start: usize, len: usize| -> Option<i32> {
        let slice = digits.get(start..start + len)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(slice).ok()?.parse().ok()
    };

    let year = num(0, 4)?;
    let month = num(4, 2).unwrap_or(1).clamp(1, 12) as u8;
    let day = num(6, 2).unwrap_or(1).clamp(1, 31) as u8;
    let hour = num(8, 2).unwrap_or(0).clamp(0, 23) as u8;
    let minute = num(10, 2).unwrap_or(0).clamp(0, 59) as u8;
    // 60 occurs: a leap second, or a producer that cannot count.
    let second = num(12, 2).unwrap_or(0).clamp(0, 60).min(59) as u8;

    let utc_offset_minutes = digits.get(14).and_then(|&sign| {
        let signum = match sign {
            b'+' => 1,
            b'-' => -1,
            // 'Z' means UT exactly; anything else is not an offset.
            b'Z' => return Some(0),
            _ => return None,
        };
        let h = num(15, 2).unwrap_or(0).clamp(0, 23);
        // The apostrophe is required by the specification and routinely
        // omitted, so look for the minutes at either position.
        let m = num(18, 2).or_else(|| num(17, 2)).unwrap_or(0).clamp(0, 59);
        Some(signum * (h * 60 + m))
    });

    Some(Date {
        year,
        month,
        day,
        hour,
        minute,
        second,
        utc_offset_minutes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16be_with_a_byte_order_mark() {
        let bytes = [0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69];
        assert_eq!(decode_text_string(&bytes), "Hi");
    }

    #[test]
    fn utf16be_surrogate_pairs_survive() {
        // U+1F600, as a surrogate pair.
        let bytes = [0xFE, 0xFF, 0xD8, 0x3D, 0xDE, 0x00];
        assert_eq!(decode_text_string(&bytes), "\u{1F600}");
    }

    #[test]
    fn a_truncated_code_unit_becomes_a_replacement() {
        let bytes = [0xFE, 0xFF, 0x00, 0x48, 0x00];
        assert_eq!(decode_text_string(&bytes), "H\u{FFFD}");
    }

    #[test]
    fn pdf_doc_encoding_differs_from_latin1_where_it_should() {
        assert_eq!(decode_text_string(b"Plain ASCII"), "Plain ASCII");
        // 0x92 is a trade mark sign here, not Latin-1's control character.
        assert_eq!(decode_text_string(&[0x92]), "\u{2122}");
        assert_eq!(decode_text_string(&[0x18]), "\u{02D8}");
        // 0xE9 coincides with Latin-1.
        assert_eq!(decode_text_string(&[0xE9]), "é");
    }

    #[test]
    fn utf8_with_pdf2_mark() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("héllo".as_bytes());
        assert_eq!(decode_text_string(&bytes), "héllo");
    }

    #[test]
    fn a_full_date_parses() {
        let d = parse_date("D:20260809143000+05'30'").expect("a date");
        assert_eq!(
            (d.year, d.month, d.day, d.hour, d.minute, d.second),
            (2026, 8, 9, 14, 30, 0)
        );
        assert_eq!(d.utc_offset_minutes, Some(5 * 60 + 30));
    }

    #[test]
    fn truncated_dates_take_the_specifications_defaults() {
        assert_eq!(
            parse_date("D:2026"),
            Some(Date {
                year: 2026,
                month: 1,
                day: 1,
                ..Date::default()
            })
        );
        let d = parse_date("D:202608").expect("year and month");
        assert_eq!((d.year, d.month, d.day), (2026, 8, 1));
    }

    #[test]
    fn zone_leniency_covers_what_producers_actually_write() {
        assert_eq!(
            parse_date("D:20260809120000Z").and_then(|d| d.utc_offset_minutes),
            Some(0)
        );
        // Apostrophes omitted entirely.
        assert_eq!(
            parse_date("D:20260809120000-0800").and_then(|d| d.utc_offset_minutes),
            Some(-8 * 60)
        );
        // No zone at all.
        assert_eq!(
            parse_date("20260809120000").and_then(|d| d.utc_offset_minutes),
            None
        );
    }

    #[test]
    fn nonsense_is_rejected_or_clamped_but_never_panics() {
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("D:"), None);
        assert_eq!(parse_date("not a date"), None);
        let d = parse_date("D:20269999999999").expect("a year is enough");
        assert_eq!((d.month, d.day, d.hour), (12, 31, 23));
        for text in ["D:2026080", "D:20260809+", "D:20260809120000+9"] {
            let _ = parse_date(text);
        }
    }
}
