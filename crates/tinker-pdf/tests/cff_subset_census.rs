//! Every CFF face in the fetched corpora, cut down to a handful of glyphs
//! (roadmap Tier 3, "CFF subsetting").
//!
//! The fixtures in `tinker-pdf-font`'s own tests answer "does the subsetter do
//! the right thing to a shape I chose". This answers the question no fixture
//! can: **are the shapes I chose the shapes real producers emit** — and it is
//! the only thing in the suite that meets a CID-keyed face with two hundred
//! Font DICTs, a charset in format 2, or a global subroutine INDEX that is not
//! empty.
//!
//! ```sh
//! cargo test -p tinker-pdf --test cff_subset_census -- --ignored --nocapture
//! ```
//!
//! **What this checks that the subsetter does not check itself.** `subset_cff`
//! verifies its own output by outlining every retained glyph in both programs,
//! so for a face it *accepts*, the outline equality below is a second reading
//! of a property the writer already refused to ship without. That is stated
//! here rather than hidden. What the census adds is the **declined** count
//! over real files, the size reduction, and four properties the writer does
//! not look at at all — every retained glyph's advance, every glyph's name
//! through the charset, every CID's glyph through the inverted charset, and
//! every one of the 256 codes through the font's own encoding. Those are what
//! `/Widths`, `/W` and a simple font without `/Differences` rest on, and a
//! subsetter that rebuilt the charset instead of copying it would break them
//! while leaving every outline exactly right.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};
use tinker_pdf_font::Cff;

// ---------------------------------------------------------------------------
// Finding the faces.
// ---------------------------------------------------------------------------

/// What shape a font program turned out to be.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Shape {
    /// A bare CFF that is not CID-keyed: `/FontFile3 /Subtype /Type1C`.
    BareSimple,
    /// A bare CFF carrying `ROS`: `/FontFile3 /Subtype /CIDFontType0C`.
    BareCidKeyed,
    /// An sfnt whose outlines are in a `CFF ` table.
    OpenType,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Shape::BareSimple => "bare CFF, simple",
            Shape::BareCidKeyed => "bare CFF, CID-keyed",
            Shape::OpenType => "OpenType/CFF",
        }
    }
}

/// One embedded font program.
struct Face {
    shape: Shape,
    program: Vec<u8>,
}

/// The `CFF ` table of an sfnt, or the bytes themselves when they are bare.
fn inner_cff(program: &[u8]) -> Option<Vec<u8>> {
    match tinker_pdf_font::Sfnt::parse(program) {
        Some(sfnt) => sfnt.table(0x4346_4620).map(<[u8]>::to_vec),
        None => Some(program.to_vec()),
    }
}

/// Every embedded program in one document whose outlines are CFF charstrings.
///
/// Reached through the font descriptors rather than by decoding every stream:
/// a corpus of four and a half thousand files carries a great many images, and
/// `/FontFile`, `/FontFile2` and `/FontFile3` are the only three keys 9.9 lets
/// a program hang off.
fn cff_faces_in(bytes: Vec<u8>) -> Vec<Face> {
    let Ok(doc) = Document::open(bytes) else {
        return Vec::new();
    };
    // An encrypted document's streams do not decode until the file key exists,
    // and most open on the empty user password.
    if doc.is_encrypted() {
        let _ = doc.authenticate("");
    }
    let cos = doc.cos();

    let mut refs: BTreeSet<(u32, u16)> = BTreeSet::new();
    for (number, entry) in cos.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let generation = match entry {
            XrefEntry::Offset { gen, .. } => gen,
            _ => 0,
        };
        let object = cos.resolve(&Object::Ref(ObjRef::new(number, generation)));
        let Some(dict) = object.as_dict() else {
            continue;
        };
        for key in [b"FontFile3".as_slice(), b"FontFile2", b"FontFile"] {
            if let Some(reference) = dict.get_ref(cos.intern(key)) {
                refs.insert((reference.num, reference.gen));
            }
        }
    }

    let mut out = Vec::new();
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    for (num, gen) in refs {
        let Ok(program) = cos.stream_decoded(ObjRef::new(num, gen)) else {
            continue;
        };
        let shape = match tinker_pdf_font::Sfnt::parse(&program) {
            Some(sfnt) => {
                if sfnt.table(0x4346_4620).is_none() {
                    continue; // a TrueType, which the other subsetter owns
                }
                Shape::OpenType
            }
            None => match Cff::parse(&program) {
                Some(cff) if cff.is_cid() => Shape::BareCidKeyed,
                Some(_) => Shape::BareSimple,
                None => continue,
            },
        };
        // The same face embedded twice in one file is one face for counting.
        if seen.insert(program.clone()) {
            out.push(Face { shape, program });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Comparing a face with its subset.
// ---------------------------------------------------------------------------

/// What went wrong, if anything.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Divergence {
    /// The subset would not parse at all.
    Unreadable,
    /// The glyph count moved, so identifiers moved with it.
    GlyphCount(usize, usize),
    /// A retained glyph draws something else.
    Outline(u16),
    /// A retained glyph's advance changed, which moves every glyph after it.
    Advance(u16),
    /// A retained glyph's font matrix changed, which draws it at another size.
    Matrix(u16),
    /// The charset no longer names the same glyph, so `/Differences` and every
    /// name lookup resolve elsewhere.
    Name(u16),
    /// The charset no longer maps a CID onto the same glyph.
    Cid(u32),
    /// The font's own encoding no longer selects the same glyph for a code.
    Code(u8),
}

/// Compares a face with its subset on every property a caller can observe.
fn compare(original: &[u8], subset: &[u8], kept: &BTreeSet<u16>) -> Vec<Divergence> {
    let mut out = Vec::new();
    let (Some(before), Some(after)) = (Cff::parse(original), Cff::parse(subset)) else {
        return vec![Divergence::Unreadable];
    };
    if before.glyph_count() != after.glyph_count() {
        out.push(Divergence::GlyphCount(
            before.glyph_count(),
            after.glyph_count(),
        ));
        return out;
    }

    for &glyph in kept {
        if before.outline(glyph).map(|o| o.segments) != after.outline(glyph).map(|o| o.segments) {
            out.push(Divergence::Outline(glyph));
        }
        if before.advance(glyph) != after.advance(glyph) {
            out.push(Divergence::Advance(glyph));
        }
        if before.font_matrix_for(glyph) != after.font_matrix_for(glyph) {
            out.push(Divergence::Matrix(glyph));
        }
    }

    // The charset, read from both sides. This is the table a renumbering
    // subset would have had to rewrite, and the one this copies through.
    if before.is_cid() {
        for cid in 0..=u32::from(u16::MAX) {
            if before.gid_for_cid(cid) != after.gid_for_cid(cid) {
                out.push(Divergence::Cid(cid));
                break;
            }
        }
    } else {
        for glyph in 0..before.glyph_count() {
            let Ok(glyph) = u16::try_from(glyph) else {
                break;
            };
            if before.glyph_name(glyph) != after.glyph_name(glyph) {
                out.push(Divergence::Name(glyph));
                break;
            }
        }
        for code in 0..=u8::MAX {
            if before.gid_for_code(code) != after.gid_for_code(code) {
                out.push(Divergence::Code(code));
                break;
            }
        }
    }

    out
}

/// A handful of glyphs spread across the face, which is what a page of text
/// asks of a CJK font and what a subsetter has to survive.
fn sample(glyphs: usize) -> BTreeSet<u16> {
    let mut out: BTreeSet<u16> = BTreeSet::new();
    out.insert(0);
    if glyphs <= 1 {
        return out;
    }
    for step in 1..=8usize {
        let gid = (step * glyphs) / 9;
        if let Ok(gid) = u16::try_from(gid) {
            if usize::from(gid) < glyphs {
                out.insert(gid);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The corpus.
// ---------------------------------------------------------------------------

fn corpus_root() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("TINKER_CORPUS") {
        let path = PathBuf::from(named);
        return path.is_dir().then_some(path);
    }
    let guess = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()?;
    guess.is_dir().then_some(guess)
}

fn pdfs_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_cff_subsets() {
    let Some(root) = corpus_root() else {
        println!("cff-subset-census: SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!("cff-subset-census: RAN over {} files", files.len());
    assert!(
        files.len() >= 4500,
        "the fetched corpora carried 4 605 files when this was written; found {}",
        files.len()
    );

    let mut carriers = 0u32;
    let mut faces: BTreeMap<Shape, u32> = BTreeMap::new();
    let mut subsetted: BTreeMap<Shape, u32> = BTreeMap::new();
    let mut declined: BTreeMap<Shape, u32> = BTreeMap::new();
    let mut divergences: Vec<(String, Divergence)> = Vec::new();
    let mut refused: Vec<(String, Shape, usize)> = Vec::new();
    let mut grew: Vec<(String, usize, usize)> = Vec::new();
    let (mut before_bytes, mut after_bytes) = (0u64, 0u64);

    for path in &files {
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let found = cff_faces_in(bytes);
        if found.is_empty() {
            continue;
        }
        carriers += 1;

        for face in &found {
            *faces.entry(face.shape).or_insert(0) += 1;
            let Some(original) = inner_cff(&face.program) else {
                continue;
            };
            let Some(cff) = Cff::parse(&original) else {
                continue;
            };
            let kept = sample(cff.glyph_count());

            let Some(reduced) = tinker_pdf_font::subset(&face.program, &kept) else {
                *declined.entry(face.shape).or_insert(0) += 1;
                refused.push((name.clone(), face.shape, cff.glyph_count()));
                continue;
            };
            *subsetted.entry(face.shape).or_insert(0) += 1;
            before_bytes += face.program.len() as u64;
            after_bytes += reduced.len() as u64;
            if reduced.len() >= face.program.len() {
                grew.push((name.clone(), face.program.len(), reduced.len()));
            }

            let Some(inner) = inner_cff(&reduced) else {
                divergences.push((name.clone(), Divergence::Unreadable));
                continue;
            };
            for divergence in compare(&original, &inner, &kept) {
                divergences.push((name.clone(), divergence));
            }
        }
    }

    let total: u32 = faces.values().sum();
    let ok: u32 = subsetted.values().sum();
    let no: u32 = declined.values().sum();

    println!("\n{carriers} files carry a CFF face; {total} distinct faces\n");
    println!(
        "  {:<24} {:>6} {:>10} {:>10}",
        "shape", "faces", "subsetted", "declined"
    );
    for shape in [Shape::BareSimple, Shape::BareCidKeyed, Shape::OpenType] {
        println!(
            "  {:<24} {:>6} {:>10} {:>10}",
            shape.name(),
            faces.get(&shape).copied().unwrap_or(0),
            subsetted.get(&shape).copied().unwrap_or(0),
            declined.get(&shape).copied().unwrap_or(0),
        );
    }
    println!("  {:<24} {total:>6} {ok:>10} {no:>10}", "all");
    println!(
        "\n{before_bytes} bytes of font program became {after_bytes} ({}%)",
        after_bytes
            .saturating_mul(100)
            .checked_div(before_bytes)
            .unwrap_or(0)
    );
    if !refused.is_empty() {
        println!("\nfaces the subsetter refused: {}", refused.len());
        for (name, shape, glyphs) in &refused {
            println!("  {name}: {} with {glyphs} glyphs", shape.name());
        }
    }
    if !grew.is_empty() {
        // Not a failure. `DocumentBuilder` compares the two sizes and keeps
        // the face when the rebuild is not smaller, which is what
        // `SubsetRefusal::SubsetNotSmaller` reports; the number is here
        // because it is what says that rule earns its place.
        println!(
            "\nfaces the rebuild did not shrink, which the writer declines: {}",
            grew.len()
        );
        for (name, before, after) in grew.iter().take(10) {
            println!("  {name}: {before} -> {after}");
        }
    }
    if !divergences.is_empty() {
        println!("\ndivergences: {}", divergences.len());
        for (name, divergence) in divergences.iter().take(40) {
            println!("  {name}: {divergence:?}");
        }
    }

    // Recorded against the corpora `corpus/corpora.lock` pins, so a shrinking
    // result cannot read as a passing one. Re-pinning a corpus moves these,
    // and moving them is a deliberate act with its own commit and its own
    // reason.
    assert_eq!(divergences, Vec::new(), "a retained glyph changed");
    assert_eq!(carriers, CARRIERS, "files carrying a CFF face");
    assert_eq!(total, FACES, "distinct CFF faces");
    assert_eq!(ok, SUBSETTED, "faces cut down");
    assert_eq!(no, DECLINED, "faces the subsetter refused");
    assert_eq!(
        grew.len(),
        NOT_SMALLER,
        "faces the rebuild did not shrink, which the writer declines"
    );
    assert_eq!(
        faces.get(&Shape::BareCidKeyed).copied().unwrap_or(0),
        CID_KEYED,
        "CID-keyed faces, which is what makes FDArray and FDSelect load-bearing"
    );
    // An eighth until 6 September 2026, when the corpus gained a thousand
    // documents off the open web and the ratio became **19 %**: 25 264 014
    // bytes of font program become 5 046 791. Real-world faces are larger and
    // more of them are already producer-made subsets that cannot shrink
    // further -- 1 936 of 3 313, against 212 of 441 in the fixture corpora --
    // so the smaller aggregate saving is the population's and not the
    // subsetter's.
    assert!(
        after_bytes * 4 < before_bytes,
        "the surviving subsets were a fifth of the bytes when this was \
         written: {after_bytes} of {before_bytes}"
    );
}

// The numbers this stood at when it was written; see the assertions above.
//
// **Re-recorded 6 September 2026 for a corpus that grew rather than a
// subsetter that changed.** `corpus/corpora.lock` gained the SafeDocs shard --
// a thousand documents nobody wrote to test a reader -- and it carries seven
// times the CFF faces the four fixture corpora do. What did *not* move is the
// assertion this file exists for: `divergences` is still empty, so every
// retained glyph draws the same outline after subsetting as before. The four
// fixture corpora alone still give 297 / 441 / 439 / 2 / 212 / 222, measured
// by moving the shard out of `corpus/files` and running this again.
const CARRIERS: u32 = 480;
const FACES: u32 = 3313;
const SUBSETTED: u32 = 3311;
const DECLINED: u32 = 2;
const NOT_SMALLER: usize = 1936;
const CID_KEYED: u32 = 551;
