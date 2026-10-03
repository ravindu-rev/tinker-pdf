//! The WHATWG Encoding Standard's labels, and its single-byte decoders.
//!
//! Two jobs, both of them tables: which encoding a label names (§4.2's *get an
//! encoding*), and what each byte of a single-byte encoding is (§9's *single-
//! byte decoder* over the encoding's index). Both tables are vendored verbatim
//! from `whatwg/encoding` under `data/encoding-indexes` and compiled by
//! `build.rs`; nothing under `data/` is opened at run time.
//!
//! **Why the Encoding Standard and not the IANA registry or ISO 8859 itself.**
//! The standard is what every reader of real documents decodes by, and its
//! tables are the measured ones: `iso-8859-1`, `latin1` and `us-ascii` are
//! labels of **windows-1252** there, because a document that says Latin-1 and
//! holds a byte in 0x80–0x9F means a curly quote or a dash by it and never the
//! C1 control ISO 8859-1 puts there. A reader that took the label at its word
//! would set every such byte as an invisible control.
//!
//! **Only the single-byte encodings decode.** The multi-byte ones — GBK,
//! gb18030, Big5, EUC-JP, ISO-2022-JP, Shift_JIS, EUC-KR — are labels this
//! module recognises and a caller is told it does not decode
//! ([`Label::Unsupported`]), because each is a state machine over an index of
//! thousands of rows and none of them is what the rows that asked for this
//! module (FB2 in `windows-1251` and `koi8-r`, HTML's windows-1252 default)
//! need.
//!
//! Decoding cannot expand: one byte in is one `char` out, three bytes of UTF-8
//! at most, and an unmapped byte is U+FFFD — the standard's own *replacement*
//! error mode — counted so the caller can say how many there were.

include!(concat!(env!("OUT_DIR"), "/single_byte.rs"));

/// One of the Encoding Standard's single-byte encodings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SingleByte {
    Ibm866,
    Iso8859_2,
    Iso8859_3,
    Iso8859_4,
    Iso8859_5,
    Iso8859_6,
    Iso8859_7,
    Iso8859_8,
    /// ISO-8859-8 in logical order: the same bytes to the same characters.
    Iso8859_8I,
    Iso8859_10,
    Iso8859_13,
    Iso8859_14,
    Iso8859_15,
    Iso8859_16,
    Koi8R,
    Koi8U,
    Macintosh,
    Windows874,
    Windows1250,
    Windows1251,
    /// Also what `iso-8859-1`, `latin1`, `ascii` and `us-ascii` name.
    Windows1252,
    Windows1253,
    Windows1254,
    Windows1255,
    Windows1256,
    Windows1257,
    Windows1258,
    XMacCyrillic,
}

impl SingleByte {
    /// Every single-byte encoding, in the standard's order.
    pub const ALL: [SingleByte; 28] = [
        SingleByte::Ibm866,
        SingleByte::Iso8859_2,
        SingleByte::Iso8859_3,
        SingleByte::Iso8859_4,
        SingleByte::Iso8859_5,
        SingleByte::Iso8859_6,
        SingleByte::Iso8859_7,
        SingleByte::Iso8859_8,
        SingleByte::Iso8859_8I,
        SingleByte::Iso8859_10,
        SingleByte::Iso8859_13,
        SingleByte::Iso8859_14,
        SingleByte::Iso8859_15,
        SingleByte::Iso8859_16,
        SingleByte::Koi8R,
        SingleByte::Koi8U,
        SingleByte::Macintosh,
        SingleByte::Windows874,
        SingleByte::Windows1250,
        SingleByte::Windows1251,
        SingleByte::Windows1252,
        SingleByte::Windows1253,
        SingleByte::Windows1254,
        SingleByte::Windows1255,
        SingleByte::Windows1256,
        SingleByte::Windows1257,
        SingleByte::Windows1258,
        SingleByte::XMacCyrillic,
    ];

    /// The name the Encoding Standard gives this encoding.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            SingleByte::Ibm866 => "IBM866",
            SingleByte::Iso8859_2 => "ISO-8859-2",
            SingleByte::Iso8859_3 => "ISO-8859-3",
            SingleByte::Iso8859_4 => "ISO-8859-4",
            SingleByte::Iso8859_5 => "ISO-8859-5",
            SingleByte::Iso8859_6 => "ISO-8859-6",
            SingleByte::Iso8859_7 => "ISO-8859-7",
            SingleByte::Iso8859_8 => "ISO-8859-8",
            SingleByte::Iso8859_8I => "ISO-8859-8-I",
            SingleByte::Iso8859_10 => "ISO-8859-10",
            SingleByte::Iso8859_13 => "ISO-8859-13",
            SingleByte::Iso8859_14 => "ISO-8859-14",
            SingleByte::Iso8859_15 => "ISO-8859-15",
            SingleByte::Iso8859_16 => "ISO-8859-16",
            SingleByte::Koi8R => "KOI8-R",
            SingleByte::Koi8U => "KOI8-U",
            SingleByte::Macintosh => "macintosh",
            SingleByte::Windows874 => "windows-874",
            SingleByte::Windows1250 => "windows-1250",
            SingleByte::Windows1251 => "windows-1251",
            SingleByte::Windows1252 => "windows-1252",
            SingleByte::Windows1253 => "windows-1253",
            SingleByte::Windows1254 => "windows-1254",
            SingleByte::Windows1255 => "windows-1255",
            SingleByte::Windows1256 => "windows-1256",
            SingleByte::Windows1257 => "windows-1257",
            SingleByte::Windows1258 => "windows-1258",
            SingleByte::XMacCyrillic => "x-mac-cyrillic",
        }
    }

    /// The index's 128 entries for bytes 0x80 to 0xFF, 0 for one it leaves
    /// unmapped.
    fn table(self) -> &'static [u16; 128] {
        let name = self.name();
        SINGLE_BYTE_TABLES
            .iter()
            .find(|(entry, _)| *entry == name)
            .map_or(&EMPTY, |(_, table)| table)
    }

    /// One byte as the character it is, or `None` for a byte the index leaves
    /// unmapped. §9's single-byte decoder: below 0x80 a byte is the ASCII
    /// character it is, in every one of these encodings.
    #[must_use]
    pub fn char(self, byte: u8) -> Option<char> {
        if byte < 0x80 {
            return Some(char::from(byte));
        }
        let scalar = *self.table().get(usize::from(byte - 0x80))?;
        if scalar == 0 {
            return None;
        }
        char::from_u32(u32::from(scalar))
    }

    /// The bytes as text, each unmapped byte read as U+FFFD, and how many
    /// there were.
    #[must_use]
    pub fn decode(self, bytes: &[u8]) -> (String, usize) {
        let mut out = String::with_capacity(bytes.len());
        let mut unmapped = 0;
        for &byte in bytes {
            match self.char(byte) {
                Some(c) => out.push(c),
                None => {
                    out.push('\u{FFFD}');
                    unmapped += 1;
                }
            }
        }
        (out, unmapped)
    }
}

/// Every pointer unmapped: what a table lookup falls back to, which no
/// variant reaches — `single_byte_tables_cover_every_variant` holds that.
static EMPTY: [u16; 128] = [0; 128];

/// What a label names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Label {
    /// UTF-8.
    Utf8,
    /// UTF-16LE — which is also what the bare label `utf-16` names.
    Utf16LittleEndian,
    /// UTF-16BE.
    Utf16BigEndian,
    /// A single-byte encoding, which [`SingleByte::decode`] reads.
    SingleByte(SingleByte),
    /// An encoding the standard defines and this crate does not decode, by the
    /// standard's name for it: one of the multi-byte encodings,
    /// `replacement`, or `x-user-defined`.
    Unsupported(&'static str),
}

/// §4.2's *get an encoding*: the label with ASCII white space trimmed from
/// both ends, compared case-insensitively against every label the standard
/// gives. `None` for a label the standard does not give at all.
#[must_use]
pub fn lookup(label: &str) -> Option<Label> {
    let trimmed = label.trim_matches(|c: char| matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' '));
    let lower = trimmed.to_ascii_lowercase();
    let at = ENCODING_LABELS
        .binary_search_by(|(entry, _)| entry.as_bytes().cmp(lower.as_bytes()))
        .ok()?;
    let (_, name) = ENCODING_LABELS.get(at)?;
    Some(match *name {
        "UTF-8" => Label::Utf8,
        "UTF-16LE" => Label::Utf16LittleEndian,
        "UTF-16BE" => Label::Utf16BigEndian,
        other => match SingleByte::ALL.iter().find(|s| s.name() == other) {
            Some(single) => Label::SingleByte(*single),
            None => Label::Unsupported(name),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant finds its table, and every table belongs to a variant —
    /// so `EMPTY` is a fallback nothing reaches.
    #[test]
    fn single_byte_tables_cover_every_variant() {
        assert_eq!(SINGLE_BYTE_TABLES.len(), SingleByte::ALL.len());
        for single in SingleByte::ALL {
            assert!(
                SINGLE_BYTE_TABLES
                    .iter()
                    .any(|(name, _)| *name == single.name()),
                "{single:?}"
            );
            assert!(!std::ptr::eq(single.table(), &EMPTY), "{single:?}");
        }
    }

    /// The word for *hello* in Russian, in the two encodings Russian books are
    /// written in, against the bytes each gives it — written out by hand from
    /// the code charts rather than by running a decoder.
    #[test]
    fn cyrillic_decodes_from_both_of_its_encodings() {
        let windows = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
        let koi = [0xF0, 0xD2, 0xC9, 0xD7, 0xC5, 0xD4];
        assert_eq!(
            SingleByte::Windows1251.decode(&windows),
            ("Привет".to_owned(), 0)
        );
        assert_eq!(SingleByte::Koi8R.decode(&koi), ("Привет".to_owned(), 0));
        assert_eq!(
            SingleByte::Windows1251.decode(b"<p>ok</p>"),
            ("<p>ok</p>".to_owned(), 0),
            "ASCII is itself"
        );
        // Ё and ё sit outside the contiguous block in both.
        assert_eq!(SingleByte::Windows1251.char(0xA8), Some('Ё'));
        assert_eq!(SingleByte::Koi8R.char(0xA3), Some('ё'));
    }

    /// windows-1252's five holes are U+FFFD and counted, and the C1 range it
    /// fills is the typography that range means in real documents.
    #[test]
    fn an_unmapped_byte_is_a_replacement_and_counted() {
        assert_eq!(SingleByte::Windows1252.char(0x80), Some('€'));
        assert_eq!(SingleByte::Windows1252.char(0x93), Some('\u{201C}'));
        // windows-1253's 0xAA is one of the bytes its index leaves out.
        assert_eq!(SingleByte::Windows1253.char(0xAA), None);
        assert_eq!(
            SingleByte::Windows1253.decode(&[b'a', 0xAA, b'b']),
            ("a\u{FFFD}b".to_owned(), 1)
        );
    }

    #[test]
    fn a_label_is_trimmed_and_compared_without_case() {
        assert_eq!(
            lookup(" Windows-1251\n"),
            Some(Label::SingleByte(SingleByte::Windows1251))
        );
        assert_eq!(lookup("koi8-r"), Some(Label::SingleByte(SingleByte::Koi8R)));
        assert_eq!(
            lookup("cp1251"),
            Some(Label::SingleByte(SingleByte::Windows1251))
        );
        assert_eq!(
            lookup("ISO-8859-1"),
            Some(Label::SingleByte(SingleByte::Windows1252)),
            "Latin-1 is windows-1252 by the standard"
        );
        assert_eq!(lookup("utf8"), Some(Label::Utf8));
        assert_eq!(lookup("utf-16"), Some(Label::Utf16LittleEndian));
        assert_eq!(lookup("shift_jis"), Some(Label::Unsupported("Shift_JIS")));
        assert_eq!(lookup("gb2312"), Some(Label::Unsupported("GBK")));
        assert_eq!(lookup("no-such-encoding"), None);
        assert_eq!(lookup("windows-1251x"), None);
    }
}
