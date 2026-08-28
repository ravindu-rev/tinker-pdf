//! **Clause 6.3's refinement, held to a picture the corpus codes twelve ways.**
//!
//! The pdf.js JBIG2 fixtures are one 399 by 400 bitmap encoded over and over
//! with a different coding tool each time: four generic templates, MMR, typical
//! prediction, custom AT pixels, a symbol dictionary and text region — and, for
//! this file's purposes, eight refinement variants. Every one of them is a
//! lossless encoding of the same picture, so **the files that do not refine are
//! ground truth for the files that do**, and no external program is asked
//! anything (ruling 13).
//!
//! That matters more here than it would elsewhere. T.88's own Annex H does
//! carry a refinement page, and `tinker-pdf-filters` pins it — but its
//! refinement is a single 6 by 6 symbol, thirty-six coded decisions, and that
//! is not enough information to tell one candidate context template from
//! another. Five distinct templates decode Annex H's page 3 into the same
//! plausible-looking line of text, and all five are wrong: they render the
//! third glyph without its descender. What separates them is exactly this —
//! a whole picture, coded both with refinement and without it, asserted equal.
//!
//! These are corpus tests, so they are `#[ignore]` like the census beside them:
//!
//! ```sh
//! cargo test -p tinker-pdf --test jbig2_refinement -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};
use tinker_pdf_filters::{jbig2_decode, Jbig2Params};

/// The file every assertion below compares against: the same picture, coded as
/// a plain generic region with no refinement anywhere in it.
const GROUND_TRUTH: &str = "bitmap-template1.pdf";

/// The refinement variants, and what each one is for.
///
/// Read as a list of what would still be wrong if any single one were dropped:
/// the two templates, the two roads a reference arrives by, typical prediction
/// on each template, a non-nominal adaptive pair, and refinement reached
/// through a symbol dictionary rather than a region.
const VARIANTS: [(&str, &str); 15] = [
    (
        "bitmap-refine.pdf",
        "template 0, over an intermediate region",
    ),
    ("bitmap-refine-page.pdf", "template 0, over the page itself"),
    (
        "bitmap-refine-page-subrect.pdf",
        "the reference is part of the page, not all of it",
    ),
    (
        "bitmap-refine-customat.pdf",
        "template 0 with a non-nominal adaptive pair (7.4.7.3)",
    ),
    (
        "bitmap-refine-template1.pdf",
        "template 1, which has no adaptive pair",
    ),
    (
        "bitmap-refine-tpgron.pdf",
        "6.3.5.6 typical prediction, template 0",
    ),
    (
        "bitmap-refine-customat-tpgron.pdf",
        "typical prediction and a custom adaptive pair together",
    ),
    (
        "bitmap-refine-template1-tpgron.pdf",
        "6.3.5.6 typical prediction, template 1",
    ),
    ("bitmap-refine-refine.pdf", "a refinement of a refinement"),
    (
        "bitmap-symbol-refine.pdf",
        "6.4.11: a text region refining its own symbols",
    ),
    (
        "bitmap-symbol-texthuffrefine.pdf",
        "6.4.11 over Huffman, with the deltas through table B.14",
    ),
    (
        "bitmap-symbol-texthuffrefineB15.pdf",
        "the same, through B.15 — the two tables the selector chooses between",
    ),
    (
        "bitmap-symbol-symhuffrefineone.pdf",
        "6.5.8.2.2 over Huffman: a dictionary refining one symbol",
    ),
    (
        "bitmap-symbol-symhuffrefineseveral.pdf",
        "6.5.8.2.1 over Huffman: a symbol that is itself a text region",
    ),
    (
        "bitmap-symbol-symhuffrefine-textrefine.pdf",
        "both at once, and the only file that shows 6.3's states must persist          across a dictionary's symbols",
    ),
];

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files/pdfjs/test/pdfs")
        .join(name)
}

/// The first JBIG2 image in a document: its stream, its globals, and its size.
///
/// `stream_decoded` stops at an image filter rather than running it, so what
/// comes back here is the embedded JBIG2 organisation of D.3 — which is what
/// the decoder under test takes.
fn jbig2_image(name: &str) -> Option<(Vec<u8>, Vec<u8>, u32, u32)> {
    let bytes = std::fs::read(corpus(name)).ok()?;
    let doc = Document::open(bytes).ok()?;
    let cos = doc.cos();
    let filter = cos.intern(b"Filter");
    let parms = cos.intern(b"DecodeParms");
    let globals_key = cos.intern(b"JBIG2Globals");
    let width_key = cos.intern(b"Width");
    let height_key = cos.intern(b"Height");

    let named = |object: &Object| {
        object
            .as_name()
            .and_then(|name| cos.name_bytes(name))
            .is_some_and(|bytes| bytes.as_ref() == b"JBIG2Decode")
    };

    for (number, entry) in cos.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let generation = match entry {
            XrefEntry::Offset { gen, .. } => gen,
            _ => 0,
        };
        let reference = ObjRef::new(number, generation);
        let Ok(object) = cos.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let filters = cos.resolve_key(dict, filter);
        let is_jbig2 = match &*filters {
            Object::Array(items) => items.iter().any(named),
            other => named(other),
        };
        if !is_jbig2 {
            continue;
        }
        let width = u32::try_from(cos.resolve_key(dict, width_key).as_int()?).ok()?;
        let height = u32::try_from(cos.resolve_key(dict, height_key).as_int()?).ok()?;
        let mut globals = Vec::new();
        let parms_value = cos.resolve_key(dict, parms);
        if let Some(parms_dict) = parms_value.as_dict() {
            if let Some(reference) = parms_dict.get_ref(globals_key) {
                if let Ok(data) = cos.stream_decoded(reference) {
                    globals = data;
                }
            }
        }
        return Some((cos.stream_decoded(reference).ok()?, globals, width, height));
    }
    None
}

/// The decoded page, and whatever the decoder said about it.
fn decoded(name: &str) -> Option<(Vec<u8>, Vec<String>, u32, u32)> {
    let (data, globals, width, height) = jbig2_image(name)?;
    let params = Jbig2Params {
        globals: &globals,
        width,
        height,
    };
    let mut warnings = Vec::new();
    let bits = jbig2_decode(&data, &params, 1 << 22, &mut warnings).ok()?;
    let warnings = warnings.iter().map(|w| format!("{w:?}")).collect();
    Some((bits, warnings, width, height))
}

/// How many pixels two decodes of the same size disagree on.
fn disagreements(left: &[u8], right: &[u8], width: u32, height: u32) -> usize {
    let stride = (width as usize).div_ceil(8);
    let mut count = 0;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let bit = |bits: &[u8]| {
                bits.get(y * stride + x / 8)
                    .copied()
                    .unwrap_or(0)
                    .wrapping_shr(7 - (x % 8) as u32)
                    & 1
            };
            if bit(left) != bit(right) {
                count += 1;
            }
        }
    }
    count
}

/// **Every refinement variant decodes to the picture the corpus codes without
/// refinement**, exactly, and warns about nothing.
///
/// The pixel count is asserted at zero rather than under a budget on purpose.
/// These are lossless encodings of one bitmap, so any disagreement at all is a
/// decoder defect and not a tolerance to be widened — and a refinement that is
/// nearly right is precisely the failure this whole lineage risks.
#[test]
#[ignore = "reads the fetched corpus"]
fn every_refinement_variant_reproduces_the_unrefined_picture() {
    let Some((truth, truth_warnings, width, height)) = decoded(GROUND_TRUTH) else {
        panic!("{GROUND_TRUTH} is not in the fetched corpus; run `cargo xtask corpus-fetch`");
    };
    assert!(
        truth_warnings.is_empty(),
        "the ground truth itself warned: {truth_warnings:?}"
    );

    let mut failures = Vec::new();
    for (name, purpose) in VARIANTS {
        let Some((bits, warnings, w, h)) = decoded(name) else {
            failures.push(format!("{name} ({purpose}): did not decode at all"));
            continue;
        };
        if (w, h) != (width, height) {
            failures.push(format!("{name} ({purpose}): {w}x{h}, not {width}x{height}"));
            continue;
        }
        let differing = disagreements(&truth, &bits, width, height);
        if differing != 0 || !warnings.is_empty() {
            failures.push(format!(
                "{name} ({purpose}): {differing} pixels differ, warnings {warnings:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// **The unrefined encodings agree with each other too**, which is what earns
/// the file above the right to call any one of them ground truth.
///
/// Without this the test above would be comparing refinement against a single
/// unverified decode; with it, the picture is the fixed point of six coding
/// tools that share almost no code.
#[test]
#[ignore = "reads the fetched corpus"]
fn the_unrefined_encodings_agree_on_one_picture() {
    let Some((truth, _, width, height)) = decoded(GROUND_TRUTH) else {
        panic!("{GROUND_TRUTH} is not in the fetched corpus; run `cargo xtask corpus-fetch`");
    };
    for name in [
        "bitmap-template2.pdf",
        "bitmap-tpgdon.pdf",
        "bitmap-customat.pdf",
        "bitmap-mmr.pdf",
        "bitmap-symbol.pdf",
    ] {
        let Some((bits, warnings, w, h)) = decoded(name) else {
            panic!("{name} did not decode");
        };
        assert_eq!((w, h), (width, height), "{name} is a different size");
        assert_eq!(
            disagreements(&truth, &bits, width, height),
            0,
            "{name} disagrees with {GROUND_TRUTH}, warnings {warnings:?}"
        );
    }
}
