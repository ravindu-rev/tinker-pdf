//! **Every JBIG2 coding variant decodes to the one picture the corpus codes
//! a dozen ways** (roadmap Tier 2).
//!
//! `jbig2_refinement.rs` is this test for clause 6.3, and its header carries
//! the argument in full: the pdf.js fixtures are one 399 by 400 bitmap encoded
//! many ways, each losslessly, so **the encodings that do not use a tool are
//! ground truth for the ones that do**, and the comparison is between two of
//! this engine's own decodes — ruling 13 untouched.
//!
//! This file is the same instrument pointed at the lineages Tier 2 closes,
//! which arrive one at a time: transposed text regions first. The reason it is
//! not folded into `jbig2_refinement.rs` is that a refinement variant and a
//! placement variant fail for different reasons and a reader should be able to
//! run one without the other.
//!
//! What makes this worth more than a round trip against this repository's own
//! encoder: these are documents nobody here authored, written by somebody else
//! to exercise exactly the clauses that had to be derived rather than read.
//!
//! ```sh
//! cargo test -p tinker-pdf --test jbig2_lineages -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};
use tinker_pdf_filters::{jbig2_decode, Jbig2Params};

/// The file every assertion below compares against: the same picture, coded as
/// a plain generic region with no symbol dictionary and no placement variant.
const GROUND_TRUTH: &str = "bitmap-template1.pdf";

/// 6.4.5's `TRANSPOSED` = 1, and what each file is for.
///
/// The census found four such regions in four files, and they are all four of
/// the reference corners bar one — which is the point: transposed placement
/// branches on left-versus-right where ordinary placement branches on
/// top-versus-bottom, so a build that had the pair the wrong way round would
/// pass on some of these and fail on others.
const TRANSPOSED: [(&str, &str); 4] = [
    ("bitmap-symbol-texttranspose.pdf", "TRANSPOSED with TOPLEFT"),
    (
        "bitmap-symbol-textbottomlefttranspose.pdf",
        "TRANSPOSED with BOTTOMLEFT, where the coordinate names the bottom edge",
    ),
    (
        "bitmap-symbol-textbottomrighttranspose.pdf",
        "TRANSPOSED with BOTTOMRIGHT, both coordinates on the far edges",
    ),
    (
        "bitmap-symbol-texttoprighttranspose.pdf",
        "TRANSPOSED with TOPRIGHT, where the coordinate names the right edge",
    ),
];

/// Clause 7.4.13's custom code tables, and which selector each file drives.
///
/// The census counted 21 Tables segments in 6 files, and five of the six reach
/// them through a *refining* Huffman text region -- which is the interesting
/// half, because 7.4.4.1.2 hands the referred-to tables to the selectors by
/// position, so a build that consumed them in the wrong order would take the
/// right number of tables and give each one to the wrong field.
const CUSTOM_TABLES: [(&str, &str); 6] = [
    (
        "bitmap-symbol-symhuffcustom-texthuffcustom.pdf",
        "a custom table on the dictionary's SDHUFFDH and on the region's own selectors",
    ),
    (
        "bitmap-symbol-texthuffrefinecustom.pdf",
        "a refining region whose RDW/RDH/RDX/RDY come from custom tables",
    ),
    (
        "bitmap-symbol-texthuffrefinecustomdims.pdf",
        "custom tables for the refinement's two size deltas",
    ),
    (
        "bitmap-symbol-texthuffrefinecustompos.pdf",
        "custom tables for the refinement's two position deltas",
    ),
    (
        "bitmap-symbol-texthuffrefinecustomposdims.pdf",
        "custom tables for all four refinement deltas at once",
    ),
    (
        "bitmap-symbol-texthuffrefinecustomsize.pdf",
        "a custom SBHUFFRSIZE, the one selector that is a single bit",
    ),
];

/// 7.2.7's unknown segment data length.
///
/// One fixture, and two real documents off the open web that the corpus report
/// names -- so this is the rare Tier 2 row whose reachability is a *scanner*
/// rather than a test suite. The picture is the same one, which is what makes
/// it checkable here at all.
const UNKNOWN_LENGTH: [(&str, &str); 1] = [(
    "bitmap-initially-unknown-size.pdf",
    "a segment whose length is 0xFFFFFFFF, ended by the row terminator",
)];

/// **Annex B's standard tables, as the corpus selects them.**
///
/// The Huffman symbol lineage coded with the *standard* tables rather than the
/// file's own — the custom-table family below covers those. Four files, and
/// between them they drive every `SBHUFFFS`, `SBHUFFDS` and `SBHUFFDT`
/// selector the corpus uses: B.6 and B.7, B.8 and B.10, B.11, B.12 and B.13.
///
/// **Why this family exists at all.** Four of the fifteen tables were
/// mis-transcribed, and only three of the four could be *known* wrong from
/// inside this repository: they were not complete prefix codes, so some bit
/// patterns decoded to nothing and the file was refused. The fourth, B.15, was
/// complete and wrong, and the difference matters here — a table whose lines
/// are wrong but whose lengths sum to one does not refuse. It assigns
/// different codes to the same lines and draws a different picture. Until this
/// family existed, "the file reported no warning" was all the corpus said
/// about any of them, and that is not the same claim.
const ANNEX_B_TABLES: [(&str, &str); 4] = [
    (
        "bitmap-symbol-symhuff-texthuff.pdf",
        "the standard tables at every selector's default",
    ),
    (
        "bitmap-symbol-symhuff-texthuffB10B13.pdf",
        "SBHUFFDS through B.10 and SBHUFFDT through B.13",
    ),
    (
        "bitmap-symbol-symhuffB5B3-texthuffB7B9B12.pdf",
        "SDHUFFDH through B.5, SDHUFFDW through B.3, and the text          region through B.7, B.9 and B.12 — the file that refused until          the tables were transcribed from T.88 rather than reconstructed",
    ),
    (
        "bitmap-symbol-symhuffuncompressed-texthuff.pdf",
        "6.5.9's collective bitmap stored uncompressed rather than as MMR",
    ),
];

/// Clause 7.4.2's retained bitmap-coding contexts.
///
/// One file, and it is the whole reachability of the row: three of its symbol
/// dictionaries consume the adaptive state a previous one retained, and two
/// retain. Nothing else in the corpus or in this repository's fixtures asks
/// for it, which is why the assertion is against the picture rather than
/// against the contexts themselves -- a consumer that started from E.3.6's
/// initial state instead decodes symbols that are *nearly* right, so the only
/// thing that separates the two readings is every pixel of the page.
const RETAINED_CONTEXT: [(&str, &str); 1] = [(
    "bitmap-symbol-context-reuse.pdf",
    "three dictionaries consuming a retained context, two retaining one",
)];

/// Clauses 6.6 and 6.7: the halftone lineage.
///
/// Thirteen of the sixteen declare the *lossless* region type (23), so they can
/// be held to the picture exactly. The two `10bpp` files are type 22 and the
/// refining one is an intermediate region plus a type 42, so all three are
/// listed here too and any of them that is not lossless will say so by failing
/// rather than by being left out.
const HALFTONE: [(&str, &str); 16] = [
    ("bitmap-halftone.pdf", "the plain case"),
    ("bitmap-halftone-template1.pdf", "grey-scale template 1"),
    ("bitmap-halftone-template2.pdf", "grey-scale template 2"),
    ("bitmap-halftone-template3.pdf", "grey-scale template 3"),
    (
        "bitmap-halftone-grid.pdf",
        "a grid vector that is not axis-aligned",
    ),
    (
        "bitmap-halftone-composite.pdf",
        "a combination operator other than OR",
    ),
    (
        "bitmap-composite-and-xnor-halftone.pdf",
        "AND and XNOR as the cell operator",
    ),
    (
        "bitmap-composite-or-xor-replace-halftone.pdf",
        "OR, XOR and REPLACE, beside a generic region",
    ),
    ("bitmap-halftone-skip-grid.pdf", "6.6.5.1's HENABLESKIP"),
    (
        "bitmap-halftone-skip-grid-template1.pdf",
        "skip with grey-scale template 1",
    ),
    (
        "bitmap-halftone-skip-grid-template2.pdf",
        "skip with grey-scale template 2",
    ),
    (
        "bitmap-halftone-skip-grid-template3.pdf",
        "skip with grey-scale template 3",
    ),
    (
        "bitmap-halftone-skip-dummy.pdf",
        "HENABLESKIP where nothing is actually skipped",
    ),
    ("bitmap-halftone-10bpp.pdf", "ten bitplanes"),
    ("bitmap-halftone-10bpp-mmr.pdf", "ten bitplanes, MMR-coded"),
    (
        "bitmap-halftone-refine.pdf",
        "an intermediate halftone region refined by a type 42 segment",
    ),
];

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files/pdfjs/test/pdfs")
        .join(name)
}

/// The first JBIG2 image in a document: its stream, its globals, and its size.
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

/// Holds a family of variants to the ground truth, at zero pixels different.
fn every_variant_reproduces(family: &str, variants: &[(&str, &str)]) {
    let Some((truth, truth_warnings, width, height)) = decoded(GROUND_TRUTH) else {
        panic!("{GROUND_TRUTH} is not in the fetched corpus; run `cargo xtask corpus-fetch`");
    };
    assert!(
        truth_warnings.is_empty(),
        "the ground truth itself warned: {truth_warnings:?}"
    );

    let mut failures = Vec::new();
    for (name, purpose) in variants {
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
    assert!(failures.is_empty(), "{family}:\n{}", failures.join("\n"));
    println!("jbig2-lineages: RAN {family}, {} variants", variants.len());
}

/// **Every transposed text region draws the same picture as an ordinary one.**
///
/// Asserted at zero pixels rather than under a budget, for the reason
/// `jbig2_refinement.rs` gives: these are lossless encodings of one bitmap, so
/// any disagreement at all is a decoder defect rather than a tolerance to be
/// widened. A transposed region that placed its symbols on the wrong edge
/// would still produce a picture, which is exactly why the comparison has to
/// be against a whole one.
#[test]
#[ignore = "reads the fetched corpus"]
fn every_transposed_text_region_reproduces_the_ordinary_picture() {
    every_variant_reproduces("transposed text regions", &TRANSPOSED);
}

/// **Every custom-code-table file draws the same picture as a standard one.**
///
/// This is what says the 7.4.13 reader is right, and it says something a round
/// trip could not. The tables are handed to the selectors **by position** in
/// the order the clause lists them, so a reader that consumed one too many or
/// too few, or read `HTLOW` and `HTHIGH` the wrong way round, would still
/// produce a table of the right shape and would place every symbol after the
/// mistake somewhere else. Only a whole picture notices that.
#[test]
#[ignore = "reads the fetched corpus"]
fn every_custom_code_table_file_reproduces_the_standard_picture() {
    every_variant_reproduces("clause 7.4.13 custom code tables", &CUSTOM_TABLES);
}

/// **A segment of unknown data length draws the same picture as one that
/// declares its length.**
///
/// 7.2.7 gives the layout instead of a length, so the end is found by scanning
/// for the row terminator and the height comes from the four bytes after it.
/// Both of those are places to be off by a small amount and still produce a
/// picture -- a height one row short, or a terminator found inside the
/// adaptive pixels -- which is why the comparison is a whole page.
#[test]
#[ignore = "reads the fetched corpus"]
fn an_unknown_length_segment_reproduces_the_declared_picture() {
    every_variant_reproduces("7.2.7 unknown data length", &UNKNOWN_LENGTH);
}

/// **The standard Annex B tables decode to the picture, not merely without a
/// warning.**
///
/// A prefix table with the wrong lengths assigns different codes to the same
/// lines. Some bit patterns then decode to *a* value rather than to none, so
/// the file completes and draws something else — which is why this assertion
/// is the picture and why "no warning" was never evidence about the tables.
///
/// `bitmap-symbol-symhuffB5B3-texthuffB7B9B12.pdf` is deliberately absent: it
/// is the file that does not decode, and it is the roadmap row.
#[test]
#[ignore = "reads the fetched corpus"]
fn the_standard_annex_b_tables_reproduce_the_picture() {
    every_variant_reproduces("Annex B standard tables", &ANNEX_B_TABLES);
}

/// **A dictionary that starts where another finished draws the same picture.**
///
/// The adaptive contexts are a probability model, not data: a decoder reading
/// the *same bits* through the wrong probabilities gets a different answer,
/// and nothing about the bit stream says so. This file is the only thing in
/// reach that asks.
///
/// It also settles a choice T.88 leaves to be read carefully: a dictionary
/// referring to several segments that retained takes the **last** of them in
/// reference order. Both wrong readings were run -- starting from E.3.6's
/// initial state, and taking the first retainer instead of the last -- and
/// this file catches each of them, in both cases by desynchronising the
/// dictionary badly enough that its export count disagrees with its header
/// rather than by drawing a wrong picture. That is luck rather than design:
/// a probability model is wrong by degrees, and the assertion is against
/// every pixel because a smaller desynchronisation would draw.
#[test]
#[ignore = "reads the fetched corpus"]
fn a_retained_bitmap_coding_context_reproduces_the_standard_picture() {
    every_variant_reproduces("7.4.2 retained contexts", &RETAINED_CONTEXT);
}

/// **Every halftone file draws the picture the corpus codes without one.**
///
/// The third lineage, and the one with the most places to be subtly wrong: the
/// grid vector is 8.8 fixed point and may be sheared, the grey values are Gray
/// coded across bitplanes that share one coder, and 6.6.5.1's skip decides
/// which cells are coded at all. Every one of those produces *a* picture when
/// it is wrong, which is why the assertion is zero pixels against a whole one.
#[test]
#[ignore = "reads the fetched corpus"]
fn every_halftone_file_reproduces_the_generic_picture() {
    every_variant_reproduces("halftone regions and pattern dictionaries", &HALFTONE);
}
