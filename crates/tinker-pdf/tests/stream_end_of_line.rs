//! A stream whose `/Length` counts the end-of-line before `endstream` reads
//! with no repair, through each of the five byte filters (7.3.8.1, 7.4).
//!
//! 7.3.8.1 puts an end-of-line marker between a stream's data and
//! `endstream` and keeps it out of `/Length`. A producer that counts it in
//! hands the filter reading the stream that marker after the filter's own
//! end of data: two files of the reported-breakage corpus write their ICC
//! profiles so, a zlib stream's checksum followed by a line feed and an
//! ASCII85 stream's `~>` followed by one. White space there holds no data,
//! so reading past it is no leniency and raises no warning, where a byte
//! that is not white space is still `Filter(TrailingGarbage)` on its stream.
//!
//! Every document here is generated: a page filled through an `ICCBased`
//! space, its content stream and its profile each coded with one filter and
//! counted through a tail of white space. The fill's pixel says the profile
//! was read: its curves are the identity, so half of each component is half
//! the light, which sRGB encodes near 188, where a profile left unread draws
//! the alternate space's 128.

mod pdfa_support;

use tinker_pdf::{Bitmap, Document, LadderLevel, RenderOptions, Warning, WarningKind};
use tinker_pdf_filters::Warning as FilterWarning;

/// The content stream: the whole page filled at half of each component.
const FILL: &[u8] = b"/CS0 cs 0.5 0.5 0.5 scn 0 0 40 40 re f";

/// A byte filter of 7.4, by what its stream's `/Filter` names.
#[derive(Clone, Copy, Debug)]
enum Coding {
    Flate,
    Lzw,
    AsciiHex,
    Ascii85,
    RunLength,
}

const CODINGS: [Coding; 5] = [
    Coding::Flate,
    Coding::Lzw,
    Coding::AsciiHex,
    Coding::Ascii85,
    Coding::RunLength,
];

impl Coding {
    fn name(self) -> &'static str {
        match self {
            Self::Flate => "/FlateDecode",
            Self::Lzw => "/LZWDecode",
            Self::AsciiHex => "/ASCIIHexDecode",
            Self::Ascii85 => "/ASCII85Decode",
            Self::RunLength => "/RunLengthDecode",
        }
    }

    /// `data` coded, through its end-of-data marker and no further.
    fn encode(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Flate => tinker_pdf_filters::zlib_compress(data),
            Self::Lzw => lzw(data),
            Self::AsciiHex => {
                let mut out: Vec<u8> = data
                    .iter()
                    .flat_map(|b| format!("{b:02X}").into_bytes())
                    .collect();
                out.push(b'>');
                out
            }
            Self::Ascii85 => ascii85(data),
            Self::RunLength => run_length(data),
        }
    }
}

/// 7.4.4.2: each byte its own nine-bit code, a Clear code before every run
/// of 200 so that no code widens, and EOD (257) last, packed high bit first.
fn lzw(data: &[u8]) -> Vec<u8> {
    let mut codes: Vec<u16> = Vec::new();
    for run in data.chunks(200) {
        codes.push(256);
        codes.extend(run.iter().map(|&b| u16::from(b)));
    }
    codes.push(257);
    let mut out = Vec::new();
    let (mut bits, mut held) = (0u32, 0u32);
    for code in codes {
        bits = (bits << 9) | u32::from(code);
        held += 9;
        while held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
        }
        bits &= (1 << held) - 1;
    }
    if held > 0 {
        out.push((bits << (8 - held)) as u8);
    }
    out
}

/// 7.4.3: four bytes to five digits from `!`, `z` for four zero bytes, a
/// last group of n bytes as n + 1 digits, and `~>`.
fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for group in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..group.len()].copy_from_slice(group);
        let mut value = u32::from_be_bytes(word);
        if group.len() == 4 && value == 0 {
            out.push(b'z');
            continue;
        }
        let mut digits = [0u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        out.extend_from_slice(&digits[..group.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// 7.4.5: literal runs of at most 128 bytes, and EOD (128).
fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for run in data.chunks(128) {
        out.push((run.len() - 1) as u8);
        out.extend_from_slice(run);
    }
    out.push(128);
    out
}

/// What follows a stream's coded data: `counted`, which its `/Length`
/// counts, then `eol`, which it does not, then `endstream`.
#[derive(Clone, Copy, Debug)]
struct Tail {
    counted: &'static [u8],
    eol: &'static [u8],
}

/// The end-of-line a `/Length` counts in each of its three forms, alone
/// before `endstream` and before a marker of its own, and after blanks.
const TAILS: [Tail; 6] = [
    Tail {
        counted: b"\n",
        eol: b"",
    },
    Tail {
        counted: b"\n",
        eol: b"\r\n",
    },
    Tail {
        counted: b"\r\n",
        eol: b"",
    },
    Tail {
        counted: b"\r",
        eol: b"\n",
    },
    Tail {
        counted: b" \t\r\n",
        eol: b"",
    },
    Tail {
        counted: b"\n\n",
        eol: b"\n",
    },
];

/// A stream object: `dict`'s entries, `coded` and `tail`.
fn stream(dict: &str, coded: &[u8], tail: Tail) -> Vec<u8> {
    let length = coded.len() + tail.counted.len();
    let mut out = format!("<< {dict} /Length {length} >>\nstream\n").into_bytes();
    out.extend_from_slice(coded);
    out.extend_from_slice(tail.counted);
    out.extend_from_slice(tail.eol);
    out.extend_from_slice(b"endstream");
    out
}

/// A PDF of `objects`, numbered from 1 in order, with a cross-reference
/// table, so that it opens without repair.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The page, its content stream (object 4) and its profile (object 5) each
/// given whole: their dictionary's `/Filter`, their coded bytes and what
/// follows them.
fn document(content: (&str, &[u8], Tail), profile: (&str, &[u8], Tail)) -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n   \
          /Resources << /ColorSpace << /CS0 [/ICCBased 5 0 R] >> >> /Contents 4 0 R >>"
            .to_vec(),
        stream(content.0, content.1, content.2),
        stream(&format!("/N 3 {}", profile.0), profile.1, profile.2),
    ])
}

/// Both streams coded with `coding` and followed by `tail`.
fn coded(coding: Coding, tail: Tail) -> Vec<u8> {
    let filter = format!("/Filter {}", coding.name());
    document(
        (&filter, &coding.encode(FILL), tail),
        (&filter, &coding.encode(&pdfa_support::srgb_like()), tail),
    )
}

/// The page drawn, and every warning the document raised by the end of it.
fn drawn(bytes: Vec<u8>) -> (Bitmap, Vec<Warning>, LadderLevel) {
    let doc = Document::open(bytes).expect("it opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    (bitmap, doc.warnings(), doc.ladder_level())
}

/// The fill's first component, at the middle of the page.
fn middle(bitmap: &Bitmap) -> u8 {
    let at = (bitmap.height / 2) as usize * bitmap.stride
        + (bitmap.width / 2) as usize * bitmap.components();
    bitmap.data[at]
}

/// **The corpus's shape, through every filter.** Each stream's `/Length`
/// counts the white space after its filter's end of data, and the document
/// opens, draws and reads its profile with no warning at all.
#[test]
fn a_stream_whose_length_counts_its_end_of_line_draws_with_no_warning() {
    for coding in CODINGS {
        for tail in TAILS {
            let (bitmap, warnings, ladder) = drawn(coded(coding, tail));
            let case = format!("{coding:?} {tail:?}");
            assert_eq!(ladder, LadderLevel::Trust, "{case}");
            assert!(warnings.is_empty(), "{case}: {warnings:?}");
            assert!(bitmap.warnings.is_empty(), "{case}: {:?}", bitmap.warnings);
            let got = middle(&bitmap);
            assert!(
                (186..=190).contains(&got),
                "{case}: the profile's linear half encodes near 188, got {got}"
            );
        }
    }
}

/// A stream coded twice, the end-of-line its `/Length` counted coded with
/// it — a stream's bytes re-encoded whole — and its own after: each stage
/// of the chain meets white space after its end of data, and neither is a
/// repair.
#[test]
fn a_stream_coded_twice_with_its_end_of_line_inside_draws_with_no_warning() {
    let mut inner = Coding::Flate.encode(&pdfa_support::srgb_like());
    inner.push(b'\n');
    let filter = "/Filter [/ASCII85Decode /FlateDecode]";
    let tail = TAILS[0];
    let bytes = document(
        (
            filter,
            &ascii85(&tinker_pdf_filters::zlib_compress(FILL)),
            tail,
        ),
        (filter, &ascii85(&inner), tail),
    );
    let (bitmap, warnings, ladder) = drawn(bytes);
    assert_eq!(ladder, LadderLevel::Trust);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
    let got = middle(&bitmap);
    assert!((186..=190).contains(&got), "got {got}");
}

/// **What is not white space stays a repair**, named on the stream that
/// needed it: a byte after the end-of-line is garbage through every filter,
/// and the profile reads as it did.
#[test]
fn a_byte_after_the_end_of_data_that_is_not_white_space_is_still_repaired() {
    let garbage = Tail {
        counted: b"\nx",
        eol: b"\n",
    };
    for coding in CODINGS {
        let filter = format!("/Filter {}", coding.name());
        let bytes = document(
            (&filter, &coding.encode(FILL), TAILS[0]),
            (&filter, &coding.encode(&pdfa_support::srgb_like()), garbage),
        );
        let (bitmap, warnings, ladder) = drawn(bytes);
        assert_eq!(ladder, LadderLevel::Trust, "{coding:?}");
        let repairs: Vec<(Option<(u32, u16)>, WarningKind)> = warnings
            .iter()
            .map(|w| (w.object.map(|r| (r.num, r.gen)), w.kind))
            .collect();
        assert_eq!(
            repairs,
            vec![(
                Some((5, 0)),
                WarningKind::Filter(FilterWarning::TrailingGarbage)
            )],
            "{coding:?}"
        );
        let got = middle(&bitmap);
        assert!((186..=190).contains(&got), "{coding:?}: got {got}");
    }
}
