//! What a build without `cmap-predefined` does instead (gap 03 milestone 4).
//!
//! Plan 05 makes the CJK tables a feature so that a wasm host supplying its
//! own encoding data, or rendering no CJK at all, need not carry a megabyte
//! of them. The question this file answers is what such a build *says* — and
//! the answer is not "nothing", because the codespace ranges are three orders
//! of magnitude smaller than the CID tables and they are what makes a
//! Shift-JIS string split into the right number of codes.
//!
//! So a build with the feature off keeps the registry's codespaces, has no
//! CIDs at all, and reports `is_approximate`. That is a deliberate line:
//! knowing where a code ends is a published fact worth four kilobytes;
//! guessing what it means is what the gap plan calls "a wrong CID [that] is
//! invisible", and this build does not do it.
//!
//! Run as `cargo test -p tinker-pdf-font --no-default-features`. Turning the
//! feature off across the whole workspace does not work and cannot be made
//! to: `tools/` and `tinker-pdf-ffi` depend on the facade with its defaults,
//! and cargo unifies features across everything it builds.
#![cfg(not(feature = "cmap-predefined"))]

use tinker_pdf_font::CMap;

/// The identity pair is a rule rather than a table, so it survives.
///
/// This is the milestone's own words -- "`Identity-H`/`-V` always compiled in
/// regardless" -- and it matters because the overwhelming majority of
/// composite fonts in circulation use one of the two.
#[test]
fn the_identity_pair_still_works() {
    let h = CMap::predefined(b"Identity-H").expect("always compiled in");
    assert_eq!(h.decode_codes(&[0x30, 0x42]), vec![(0x3042, 2)]);
    assert_eq!(h.cid(0x3042), Some(0x3042));
    assert!(!h.is_approximate(), "the identity is exact, not assumed");

    let v = CMap::predefined(b"Identity-V").expect("always compiled in");
    assert!(v.is_vertical());
    assert!(!v.is_approximate());
}

/// A registry CMap still splits its strings correctly, and says it cannot
/// map them.
///
/// The same bytes as the full build's exit criterion: `A`, a two-byte kanji,
/// a half-width katakana, a second kanji. Four codes at the right widths --
/// which is what keeps the *advances* right, since a wrong split changes how
/// many glyphs there are -- and no CIDs, because inventing one is what this
/// gap exists to stop.
#[test]
fn a_registry_cmap_keeps_its_codespaces_and_admits_it_has_no_cids() {
    let cmap = CMap::predefined(b"90ms-RKSJ-H").expect("the name is known");

    assert_eq!(
        cmap.decode_codes(&[0x41, 0x81, 0x40, 0xB1, 0xE0, 0x40]),
        vec![(0x41, 1), (0x8140, 2), (0xB1, 1), (0xE040, 2)],
        "the codespaces are four kilobytes and they are always compiled in"
    );
    assert_eq!(cmap.cid(0x8140), None, "and the CIDs are not");
    assert!(
        cmap.is_approximate(),
        "which is what `is_approximate` is for"
    );
}

/// The writing mode and the `usecmap` parent survive too, because both live
/// in the same always-compiled table.
#[test]
fn the_writing_mode_and_the_parent_survive() {
    let v = CMap::predefined(b"90ms-RKSJ-V").expect("the name is known");
    assert!(v.is_vertical(), "/WMode 1 def");
    assert!(v.is_approximate());
    // Inherited from `90ms-RKSJ-H`, which declares them; the child declares
    // no codespace of its own at all.
    assert_eq!(v.decode_codes(&[0x81, 0x40]), vec![(0x8140, 2)]);
}

/// A name outside the registry is still refused, in both builds. "Not
/// compiled in" and "does not exist" are different answers and a caller has
/// to be able to tell them apart, which is what the warning naming the CMap
/// is built on.
#[test]
fn a_name_outside_the_registry_is_still_refused() {
    assert!(CMap::predefined(b"90ms-RKSJ-Q").is_none());
    assert!(CMap::predefined(b"UniJIS-Nonsense").is_none());
}
