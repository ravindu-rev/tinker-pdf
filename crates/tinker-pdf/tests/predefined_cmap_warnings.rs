//! A CMap this build cannot resolve is named, not guessed (gap 03, ruling 10).
//!
//! The gap plan's own words: "The warning matters as much as the tables.
//! Whatever subset ships, a document using a CMap outside it must say so. A
//! wrong CID is invisible; a warning naming `90ms-RKSJ-H` is actionable."
//!
//! Before this, `/Encoding /90ms-RKSJ-Q` — a name no registry defines —
//! matched the prefix `90ms` and produced a blanket two-byte codespace with
//! code-as-CID. The page came out full of the wrong characters and nothing in
//! `Document::warnings()` mentioned a CMap at all. The failure mode is what
//! makes it worth a test rather than a sentence: text that lays out and reads
//! as gibberish looks like a missing font, so nobody files it against the
//! CMap.
//!
//! Two warnings, because two different things go wrong and they have
//! different fixes.
//!
//! - `PredefinedCMapUnknown` — no such CMap. Either the file is wrong or the
//!   registry has moved on; re-vendoring is the fix.
//! - `PredefinedCMapApproximate` — the registry has it and this build left the
//!   table out. Turning `cmap-predefined` back on is the fix. Reachable only
//!   with the feature off, which is why the assertions below branch on it
//!   rather than being compiled out: both builds run this file.

use tinker_pdf::{Document, Warning, WarningKind};

/// A one-page document whose only font is a Type 0 with `/Encoding` set to
/// `encoding`, written verbatim.
///
/// No font program and no descendant glyphs: nothing here is about drawing,
/// only about what reading the dictionary reports.
fn document(encoding: &str) -> Vec<u8> {
    let content = "BT /F0 12 Tf 20 30 Td <4181400041> Tj ET";
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
          /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    out.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes());
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Fixture {encoding}\n\
             /DescendantFonts [6 0 R] >>\nendobj\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(
        b"6 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture /DW 1000\n\
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >>\n\
          /CIDToGIDMap /Identity >>\nendobj\n",
    );
    out.extend_from_slice(b"trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");
    out
}

/// Reads the document's text, which is what makes the font dictionary load,
/// and returns everything the engine tolerated.
fn warnings_after_reading(bytes: Vec<u8>) -> (Document, Vec<Warning>) {
    let doc = Document::open(bytes).expect("it opens");
    let _ = doc.page(0).expect("a page").text();
    let warnings = doc.warnings();
    (doc, warnings)
}

/// Every CMap name the warnings mention, resolved back to bytes.
///
/// The warning carries an interned [`tinker_pdf::Name`], which is only
/// meaningful against the document that issued it — so this is also the
/// worked example of how a caller turns one into something to show a user.
fn named_cmaps(
    doc: &Document,
    warnings: &[Warning],
    want: fn(&WarningKind) -> bool,
) -> Vec<String> {
    warnings
        .iter()
        .filter(|w| want(&w.kind))
        .filter_map(|w| match w.kind {
            WarningKind::PredefinedCMapUnknown(n) | WarningKind::PredefinedCMapApproximate(n) => {
                doc.cos().name_bytes(n)
            }
            _ => None,
        })
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .collect()
}

fn is_unknown(kind: &WarningKind) -> bool {
    matches!(kind, WarningKind::PredefinedCMapUnknown(_))
}

fn is_approximate(kind: &WarningKind) -> bool {
    matches!(kind, WarningKind::PredefinedCMapApproximate(_))
}

/// The exit criterion: a document using a CMap this build does not have says
/// which one.
#[test]
fn an_unknown_predefined_cmap_is_reported_by_name() {
    let (doc, warnings) = warnings_after_reading(document("/Encoding /90ms-RKSJ-Q"));
    assert_eq!(
        named_cmaps(&doc, &warnings, is_unknown),
        vec!["90ms-RKSJ-Q".to_string()],
        "the name is the actionable part; {warnings:?}"
    );
}

/// And a CMap this build *does* have says nothing, which is what makes the
/// warning above worth anything.
#[test]
fn a_cmap_this_build_has_is_silent() {
    let (doc, warnings) = warnings_after_reading(document("/Encoding /90ms-RKSJ-H"));
    assert!(
        named_cmaps(&doc, &warnings, is_unknown).is_empty(),
        "the registry defines it; {warnings:?}"
    );

    // With `cmap-predefined` off the registry still defines it and the table
    // is absent, which is a different report: the codespaces are known, the
    // CIDs are not, and `is_approximate` is what says so.
    let approximate = named_cmaps(&doc, &warnings, is_approximate);
    if cfg!(feature = "cmap-predefined") {
        assert!(approximate.is_empty(), "{warnings:?}");
    } else {
        assert_eq!(approximate, vec!["90ms-RKSJ-H".to_string()], "{warnings:?}");
    }
}

/// `Identity-H` is never either, in any build. It is the one name every
/// composite font in circulation is likeliest to use, and a warning on it
/// would be noise on almost every file with a subset font in it.
#[test]
fn the_identity_pair_is_never_reported() {
    for name in ["Identity-H", "Identity-V"] {
        let (doc, warnings) = warnings_after_reading(document(&format!("/Encoding /{name}")));
        assert!(
            named_cmaps(&doc, &warnings, is_unknown).is_empty()
                && named_cmaps(&doc, &warnings, is_approximate).is_empty(),
            "{name}: {warnings:?}"
        );
    }
}

/// A `usecmap` naming a CMap nobody has is reported by name too.
///
/// Gap 04 already warned `ParentUnresolved` here, and that warning cannot say
/// *which* parent — which matters more for this case than for the `/Encoding`
/// one, because a differential CMap that loses its parent loses its codespace
/// ranges and then splits every string one byte at a time.
#[test]
fn an_unresolvable_usecmap_parent_is_reported_by_name() {
    let cmap = b"/CIDInit /ProcSet findresource begin\n\
                 begincmap\n\
                 /90ms-RKSJ-Q usecmap\n\
                 1 begincidrange <8140> <817E> 3 endcidrange\n\
                 endcmap end";
    let mut bytes = document("/Encoding 7 0 R");
    let tail = format!(
        "7 0 obj\n<< /Type /CMap /Length {} >>\nstream\n",
        cmap.len()
    );
    // Rewrite the trailer so the new object is inside the document.
    let at = bytes
        .windows(7)
        .position(|w| w == b"trailer")
        .expect("the fixture has one");
    bytes.truncate(at);
    bytes.extend_from_slice(tail.as_bytes());
    bytes.extend_from_slice(cmap);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(b"trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");

    let (doc, warnings) = warnings_after_reading(bytes);
    assert_eq!(
        named_cmaps(&doc, &warnings, is_unknown),
        vec!["90ms-RKSJ-Q".to_string()],
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.kind.as_str() == "cmap-parent-unresolved"),
        "gap 04's warning still fires; this one says which parent: {warnings:?}"
    );
}

/// A real registry parent resolves and reports nothing, which is the same
/// fixture with one character changed.
#[test]
fn a_registry_usecmap_parent_is_silent() {
    let cmap = b"/CIDInit /ProcSet findresource begin\n\
                 begincmap\n\
                 /90ms-RKSJ-H usecmap\n\
                 1 begincidrange <8140> <817E> 3 endcidrange\n\
                 endcmap end";
    let mut bytes = document("/Encoding 7 0 R");
    let tail = format!(
        "7 0 obj\n<< /Type /CMap /Length {} >>\nstream\n",
        cmap.len()
    );
    let at = bytes
        .windows(7)
        .position(|w| w == b"trailer")
        .expect("the fixture has one");
    bytes.truncate(at);
    bytes.extend_from_slice(tail.as_bytes());
    bytes.extend_from_slice(cmap);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(b"trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");

    let (doc, warnings) = warnings_after_reading(bytes);
    assert!(
        named_cmaps(&doc, &warnings, is_unknown).is_empty(),
        "{warnings:?}"
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.kind.as_str() == "cmap-parent-unresolved"),
        "the parent is the registry's own and it resolved: {warnings:?}"
    );
}
