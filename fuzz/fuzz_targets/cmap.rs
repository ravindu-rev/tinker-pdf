//! CMap syntax: an embedded CMap stream, and the predefined name that may
//! stand in its place.
//!
//! An embedded CMap is a PostScript-ish text stream whose bytes reach this
//! parser after `tinker-pdf-cos` and `tinker-pdf-filters` have decoded them,
//! so the input here is exactly a decoded `/ToUnicode` or `/Encoding` stream.
//! Parsing it is only half the target: the codespace ranges it declares are
//! what decide where one code ends and the next begins, so the same bytes are
//! also split by the CMap they came from and every code that falls out is
//! queried.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::{cmap, CMap};

fuzz_target!(|data: &[u8]| {
    let map = cmap::parse(data);
    let _ = map.is_vertical();
    let _ = map.is_approximate();

    // Splitting a string is where a codespace range that overlaps, inverts,
    // or claims four bytes for a one-byte code does its damage.
    for (code, bytes) in map.decode_codes(data).iter().take(4096) {
        let _ = bytes;
        let _ = map.to_unicode(*code);
        let _ = map.to_unicode_string(*code);
        let _ = map.cid(*code);
    }

    // The boundaries a range lookup gets wrong: nothing, one past the top of
    // the BMP, and the value a four-byte code cannot exceed.
    for code in [0u32, 0x20, 0xFF, 0x100, 0xFFFF, 0x1_0000, u32::MAX] {
        let _ = map.to_unicode(code);
        let _ = map.cid(code);
    }

    // `/Encoding` may name a predefined CMap rather than embed one, and the
    // name comes out of the file just as unchecked as the stream does. The
    // first line of the input is that name.
    let name = data.split(|b| *b == b'\n').next().unwrap_or_default();
    if let Some(predefined) = CMap::predefined(name) {
        let _ = predefined.is_vertical();
        let _ = predefined.is_approximate();
        for (code, _) in predefined.decode_codes(data).iter().take(4096) {
            let _ = predefined.cid(*code);
            let _ = predefined.to_unicode(*code);
        }
    }
});
