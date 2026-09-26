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
//!
//! A CMap may also inherit (9.7.5.3). `cmap::parse` follows a `usecmap` chain
//! only as far as the predefined set, so the resolver-driven half — a parent
//! that is a stream, the depth cap, the cycle guard — is unreachable through
//! it. `parse_embedded` is called with three resolvers that answer out of the
//! input itself, which is how a hostile chain gets built without a document.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** All
//! eighteen calls below discard their result. A CMap that mapped every code
//! to the wrong glyph — or to nothing — is indistinguishable here from one
//! that mapped them correctly. So a run that returned the *wrong* answer
//! passes this target exactly as a correct one does, and a green `cargo fuzz`
//! here is evidence about ruling 1 and about nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf/tests/cmap_inheritance.rs` and the composite-font
//! tests, which check extracted *text* rather than that a parse returned.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_font::cmap::{ParentRef, ParentSource};
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

    // A parent that is always the child: the cycle guard, whatever the CMap
    // called the link and whichever spelling asked for it.
    let looped = cmap::parse_embedded(data, &mut |_| Some(ParentSource::Source(data.to_vec())));
    check(&looped);

    // A parent that is a fresh source every time and never repeats, so only
    // the depth cap can stop it. Dropping one byte per link keeps the chain
    // finite even if the cap were removed, which is what makes a hang here a
    // report rather than a timeout in the harness.
    let mut link = 0usize;
    let growing = cmap::parse_embedded(data, &mut |_| {
        link += 1;
        Some(ParentSource::Source(data.get(link..)?.to_vec()))
    });
    check(&growing);

    // Names resolving to names: the indirection loop, and the reason it is a
    // loop rather than a recursion.
    let named = cmap::parse_embedded(data, &mut |want| match want {
        ParentRef::Named(n) => Some(ParentSource::Named(n.to_vec())),
        ParentRef::Dictionary(_) => Some(ParentSource::Named(name.to_vec())),
    });
    check(&named);
});

/// Everything a caller does with the result, so a merged CMap is queried the
/// same way a plain one is.
fn check(parsed: &cmap::Parsed) {
    for warning in &parsed.warnings {
        let _ = warning.as_str();
    }
    let _ = parsed.cmap.is_vertical();
    let _ = parsed.cmap.is_approximate();
    for code in [0u32, 0x20, 0xFF, 0x100, 0xFFFF, 0x1_0000, u32::MAX] {
        let _ = parsed.cmap.to_unicode(code);
        let _ = parsed.cmap.to_unicode_string(code);
        let _ = parsed.cmap.cid(code);
    }
}
