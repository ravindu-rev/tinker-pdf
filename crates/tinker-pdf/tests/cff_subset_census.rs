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

use tinker_pdf::subset::UntouchedReason;
use tinker_pdf::Document;
use tinker_pdf_cos::{CosDocument, ObjRef, Object, XrefEntry};
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

/// Whether a census that finds no corpus should fail rather than skip.
///
/// A skip exits 0 and reads exactly like a pass (CONTRIBUTING, "the `RAN` /
/// `SKIPPED` discipline"), so a job that means to walk the corpora sets this
/// and gets a failure when there is nothing under them. `0` is an explicit
/// off, matching `cmap_census.rs` and the rest.
fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
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
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus at \
             TINKER_CORPUS: this census would have passed over nothing"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!("cff-subset-census: RAN over {} files", files.len());
    assert!(
        files.len() >= 4500,
        "the fetched corpora carry 5 605 files at the lock this was last \
         re-pinned against; found {}",
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

// ---------------------------------------------------------------------------
// The rewrite path.
//
// Everything above drives `tinker_pdf_font::subset` on a program pulled out of
// a document. That answers "does the subsetter survive the shapes real
// producers emit". It does not touch `tinker_pdf::subset::apply`, which is the
// pass a *save* runs (`tinker_pdf::write::save`), and which has a whole second
// half the census above cannot see: the glyph walk that decides which glyphs a
// document draws, and the dictionaries around the program that have to still
// describe it afterwards.
//
// That half is where a self-built fixture is weakest, and it is exactly what
// the fixtures in `crates/tinker-pdf/src/subset.rs` are built on. A document
// this project wrote has an encoding this project chose: `/Encoding` absent or
// `/WinAnsiEncoding`, `/FirstChar` at 32, no `/Differences`, no CID-keyed
// descendant with a `/W` array, no `/CIDToGIDMap` stream. Real producers emit
// all of those, and the pass's central claim — **the encoding is correct after
// the cut because it is unchanged** — is a claim about exactly those
// dictionaries.
//
// So this census asserts that claim over the corpus rather than over a
// fixture: every font dictionary comes out of the pass identical except
// `/BaseFont`, every descriptor except `/FontName`, and every program that was
// cut keeps its glyph count, so no identifier any of those dictionaries names
// has moved.
//
// This file's name is CFF's and this test is not CFF-only — the rewrite path
// subsets TrueType too, and most of the corpus is TrueType. The name is kept
// rather than corrected because the first census is what it says, and renaming
// a test file loses the name every commit that touched it cites.
// ---------------------------------------------------------------------------

/// What one document's fonts looked like before the pass.
struct Snapshot {
    /// Every `/Type /Font` dictionary, by object number.
    fonts: BTreeMap<u32, Object>,
    /// Every `/Type /FontDescriptor` dictionary, by object number.
    descriptors: BTreeMap<u32, Object>,
    /// Every embedded program a descriptor named, and its decoded bytes.
    programs: BTreeMap<u32, Vec<u8>>,
}

/// The number of glyphs a program declares, which is the number that must not
/// move.
///
/// `maxp`'s `numGlyphs` for an sfnt — its offset 4, a `u16` big-endian — and
/// the CharStrings INDEX count for a bare CFF. Both are what every glyph
/// identifier in the document is an index into: `/FirstChar`, `/Widths`,
/// `/Differences`, `/W` and `/CIDToGIDMap` are correct after a subset only
/// because this number, and the meaning of every index below it, is unchanged.
fn declared_glyphs(program: &[u8]) -> Option<usize> {
    if let Some(sfnt) = tinker_pdf_font::Sfnt::parse(program) {
        let maxp = sfnt.table(0x6d61_7870)?;
        let count = maxp.get(4..6)?;
        return Some(usize::from(u16::from_be_bytes([count[0], count[1]])));
    }
    Cff::parse(program).map(|cff| cff.glyph_count())
}

/// The eight reasons a program is written through whole, in declaration order.
///
/// An array rather than a map because `UntouchedReason` is deliberately not
/// `Ord` — it is a reason, not a rank — and because a new variant should break
/// this rather than land in an "other" bucket nobody reads.
const REASONS: [UntouchedReason; 8] = [
    UntouchedReason::ProgramNotRebuildable,
    UntouchedReason::SubsetNotSmaller,
    UntouchedReason::CodeNotMapped,
    UntouchedReason::FieldResource,
    UntouchedReason::ScopeNotWalked,
    UntouchedReason::Type3Resource,
    UntouchedReason::NotAnObject,
    UntouchedReason::NoFontNamesIt,
];

/// Every font dictionary, descriptor and embedded program in one document.
fn snapshot(doc: &CosDocument) -> Snapshot {
    let mut out = Snapshot {
        fonts: BTreeMap::new(),
        descriptors: BTreeMap::new(),
        programs: BTreeMap::new(),
    };
    let font = doc.intern(b"Font");
    let descriptor = doc.intern(b"FontDescriptor");
    let type_key = doc.intern(b"Type");

    for (number, entry) in doc.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let Ok(object) = doc.get(ObjRef::new(number, 0)) else {
            continue;
        };
        // A plain dictionary, never a stream — and this is not pedantry. A
        // `/FontFile2` stream in safedocs/0000215.pdf carries `/Type /Font`
        // on the *stream*, and `Object::as_dict` hands back a stream's
        // dictionary as readily as a dictionary. Six of them were compared
        // against the program stream the pass had just rewritten, and reported
        // as six font dictionaries whose `/Filter` and `/Length` had changed.
        // A font dictionary is a dictionary (9.5) and a descriptor is a
        // dictionary (9.8.1); neither is ever a stream.
        let Object::Dict(dict) = object.as_ref() else {
            continue;
        };
        let kind = dict.get(type_key);
        if kind == Some(&Object::Name(font)) {
            out.fonts.insert(number, (*object).clone());
            continue;
        }
        // A descriptor by `/Type` (Table 122 requires it) **or** by carrying a
        // program, because the second is the set the pass can touch and real
        // producers do omit the first. Taking only the typed ones would have
        // made every check below quietly skip the files most worth checking.
        let mut programs = Vec::new();
        for key in [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"] {
            if let Some(reference) = dict.get_ref(doc.intern(key)) {
                programs.push(reference);
            }
        }
        if kind != Some(&Object::Name(descriptor)) && programs.is_empty() {
            continue;
        }
        out.descriptors.insert(number, (*object).clone());
        for reference in programs {
            if let Ok(bytes) = doc.stream_decoded(reference) {
                out.programs.insert(reference.num, bytes);
            }
        }
    }
    out
}

/// A dictionary's entries, with the keys that are allowed to move dropped from
/// both sides.
///
/// Compared as a whole rather than key by key so that an *added* or *removed*
/// key counts as a difference too — a pass that dropped `/ToUnicode` would
/// otherwise slip through a loop over the keys that are still there.
fn without(doc: &CosDocument, object: &Object, allowed: &[&[u8]]) -> Vec<(Vec<u8>, Object)> {
    let Some(dict) = object.as_dict() else {
        return Vec::new();
    };
    let allowed: Vec<_> = allowed.iter().map(|k| doc.intern(k)).collect();
    dict.iter()
        .filter(|(key, _)| !allowed.contains(key))
        .map(|(key, value)| {
            (
                doc.name_bytes(*key)
                    .map(|bytes| bytes.to_vec())
                    .unwrap_or_default(),
                value.clone(),
            )
        })
        .collect()
}

/// How one document's fonts came through the pass.
#[derive(Default)]
struct Divergences {
    /// A font dictionary changed in a key that is not `/BaseFont`.
    font_dicts: Vec<String>,
    /// A descriptor changed in a key that is not `/FontName`.
    descriptors: Vec<String>,
    /// A cut program no longer declares the same number of glyphs, so every
    /// identifier the dictionaries name may point elsewhere.
    glyph_counts: Vec<String>,
    /// A cut program does not parse at all.
    unreadable: Vec<String>,
    /// A cut program came out no smaller, which the pass is supposed to
    /// decline rather than write.
    grew: Vec<String>,
    /// A `/FontFile2` stream's `/Length1` is not the decoded length of the
    /// program it now carries (Table 126).
    length1: Vec<String>,
    /// A program a descriptor embeds that the pass reported in neither list.
    ///
    /// Ruling 10: leniency names what it touched. A program neither cut nor
    /// named in `untouched` is a program a caller checking for disclosure
    /// cannot see at all, which is the one outcome the report is supposed to
    /// make impossible.
    unreported: Vec<String>,
}

#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_rewrite_subsets() {
    let Some(root) = corpus_root() else {
        println!("rewrite-subset-census: SKIPPED (no corpus; set TINKER_CORPUS)");
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus at \
             TINKER_CORPUS: this census would have passed over nothing"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!("rewrite-subset-census: RAN over {} files", files.len());
    assert!(
        files.len() >= 4500,
        "the fetched corpora carry 5 605 files at the lock this was pinned \
         against; found {}",
        files.len()
    );

    let mut documents = 0u32;
    let mut carriers = 0u32;
    let mut cut = 0u32;
    let mut whole = 0u32;
    let mut reasons = [0u32; REASONS.len()];
    let (mut before_bytes, mut after_bytes) = (0u64, 0u64);
    let mut divergences = Divergences::default();
    // How many times two of the checks below actually compared something.
    //
    // Pinned with the rest, because an empty divergence list has two causes
    // and only one of them is good news: a check that never ran reports
    // nothing exactly as loudly as a check that ran and found nothing. Both
    // of these read a value that may be absent — a program that does not
    // parse, a stream with no `/Length1` — so both can go quiet without
    // anybody noticing.
    let mut glyph_counts_compared = 0u32;
    let mut length1_checked = 0u32;
    // Printed, never asserted. The pass costs a full interpretation of every
    // page and `docs/features/editing.md` says so; this is where that sentence
    // gets a number, measured rather than guessed. A clock is not a property.
    let mut walking = std::time::Duration::ZERO;

    for path in &files {
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(doc) = Document::open(bytes) else {
            continue;
        };
        if doc.is_encrypted() {
            let _ = doc.authenticate("");
        }
        documents += 1;

        let before = snapshot(doc.cos());
        if before.programs.is_empty() {
            continue;
        }
        carriers += 1;

        let mut editor = doc.editor();
        let started = std::time::Instant::now();
        let report = tinker_pdf::subset::apply(&mut editor);
        walking += started.elapsed();

        // The dictionaries. This is the claim the module makes, and the one a
        // self-built fixture cannot support: every key the encoding rests on
        // survives the cut *because nothing touches it*.
        for (&number, original) in &before.fonts {
            let Some(after) = editor.get(ObjRef::new(number, 0)) else {
                divergences
                    .font_dicts
                    .push(format!("{name}: font {number} vanished"));
                continue;
            };
            if without(doc.cos(), original, &[b"BaseFont"])
                != without(doc.cos(), &after, &[b"BaseFont"])
            {
                divergences
                    .font_dicts
                    .push(format!("{name}: font {number}"));
            }
        }
        for (&number, original) in &before.descriptors {
            let Some(after) = editor.get(ObjRef::new(number, 0)) else {
                divergences
                    .descriptors
                    .push(format!("{name}: descriptor {number} vanished"));
                continue;
            };
            if without(doc.cos(), original, &[b"FontName"])
                != without(doc.cos(), &after, &[b"FontName"])
            {
                divergences
                    .descriptors
                    .push(format!("{name}: descriptor {number}"));
            }
        }

        // The programs.
        for subsetted in &report.subsetted {
            cut += 1;
            before_bytes += subsetted.before as u64;
            after_bytes += subsetted.after as u64;
            let Some(after) = editor.stream_bytes(subsetted.program) else {
                divergences
                    .unreadable
                    .push(format!("{name}: {} has no bytes", subsetted.program.num));
                continue;
            };
            if after.len() >= subsetted.before {
                divergences.grew.push(format!(
                    "{name}: {} {} -> {}",
                    subsetted.program.num,
                    subsetted.before,
                    after.len()
                ));
            }
            let was = before
                .programs
                .get(&subsetted.program.num)
                .and_then(|bytes| declared_glyphs(bytes));
            match (was, declared_glyphs(&after)) {
                (Some(was), Some(now)) => {
                    glyph_counts_compared += 1;
                    if was != now {
                        divergences.glyph_counts.push(format!(
                            "{name}: {} declared {was} glyphs, now {now}",
                            subsetted.program.num
                        ));
                    }
                }
                (_, None) => divergences
                    .unreadable
                    .push(format!("{name}: {} does not parse", subsetted.program.num)),
                // The subset parses and the original did not. That is not a
                // defect — the pass reads the original through
                // `tinker_pdf_font::subset`, which has its own opinion of
                // what it can rebuild — but it is a comparison not made, and
                // `glyph_counts_compared` is what keeps it from reading as
                // one that was.
                (None, Some(_)) => {}
            }
            // Table 126: `/Length1` is the decoded length of a `/FontFile2`.
            //
            // Through `as_dict` rather than a `Object::Dict(_)` pattern: the
            // editor hands back a *written* stream as its dictionary alone
            // and an untouched one as an `Object::Stream`, so matching the
            // first shape only would make this check silently dead the day a
            // program reached here without having been rewritten.
            if let Some(dict) = editor
                .get(subsetted.program)
                .as_ref()
                .and_then(Object::as_dict)
            {
                if let Some(declared) = dict.get_int(doc.cos().intern(b"Length1")) {
                    length1_checked += 1;
                    if declared != after.len() as i64 {
                        divergences.length1.push(format!(
                            "{name}: {} declares /Length1 {declared} over {} bytes",
                            subsetted.program.num,
                            after.len()
                        ));
                    }
                }
            }
        }
        for untouched in &report.untouched {
            whole += 1;
            before_bytes += untouched.bytes as u64;
            after_bytes += untouched.bytes as u64;
            if let Some(index) = REASONS.iter().position(|r| *r == untouched.reason) {
                reasons[index] += 1;
            }
        }

        // Ruling 10, over documents nobody here wrote: every program a
        // descriptor embeds is in one list or the other.
        let reported: BTreeSet<u32> = report
            .subsetted
            .iter()
            .map(|s| s.program.num)
            .chain(report.untouched.iter().map(|u| u.program.num))
            .collect();
        for number in before.programs.keys() {
            if !reported.contains(number) {
                divergences
                    .unreported
                    .push(format!("{name}: {number} is in neither list"));
            }
        }
    }

    println!("\n{documents} documents opened; {carriers} carry an embedded program\n");
    println!("  {cut} programs cut down, {whole} written through whole");
    println!(
        "  the pass itself took {:.1} s over those {carriers} documents, \
         {:.0} ms each\n",
        walking.as_secs_f64(),
        walking.as_secs_f64() * 1000.0 / f64::from(carriers.max(1)),
    );
    println!("  {:<52} {:>6}", "reason a program was left whole", "count");
    for (reason, count) in REASONS.iter().zip(reasons.iter()) {
        println!("  {:<52} {count:>6}", reason.to_string());
    }
    println!(
        "\n{before_bytes} bytes of font program became {after_bytes} ({}%)",
        after_bytes
            .saturating_mul(100)
            .checked_div(before_bytes)
            .unwrap_or(0)
    );
    println!(
        "\n{glyph_counts_compared} glyph counts compared, \
         {length1_checked} /Length1 declarations checked"
    );

    for (what, rows) in [
        ("font dictionaries changed", &divergences.font_dicts),
        ("descriptors changed", &divergences.descriptors),
        ("glyph counts moved", &divergences.glyph_counts),
        ("programs unreadable", &divergences.unreadable),
        ("programs that grew", &divergences.grew),
        ("/Length1 wrong", &divergences.length1),
        ("programs in neither list", &divergences.unreported),
    ] {
        if rows.is_empty() {
            continue;
        }
        println!("\n{what}: {}", rows.len());
        for row in rows.iter().take(20) {
            println!("  {row}");
        }
    }

    // Seven properties, over documents nobody here wrote. Each is a statement
    // about *their* encoding surviving *our* pass; none of them is this engine
    // agreeing with itself about a document it also built.
    assert!(
        divergences.font_dicts.is_empty(),
        "a font dictionary changed in a key that is not /BaseFont"
    );
    assert!(
        divergences.descriptors.is_empty(),
        "a descriptor changed in a key that is not /FontName"
    );
    assert!(
        divergences.glyph_counts.is_empty(),
        "a cut program declares a different number of glyphs, so identifiers moved"
    );
    assert!(
        divergences.unreadable.is_empty(),
        "a cut program is not a font"
    );
    assert!(
        divergences.grew.is_empty(),
        "a cut program is no smaller, which the pass declines rather than writes"
    );
    assert!(
        divergences.length1.is_empty(),
        "/Length1 does not describe the program it is on (Table 126)"
    );
    assert!(
        divergences.unreported.is_empty(),
        "a program is in neither list, so the report cannot be read for disclosure"
    );

    // Recorded against the corpora `corpus/corpora.lock` pins, so a shrinking
    // result cannot read as a passing one.
    assert_eq!(documents, DOCUMENTS, "documents that opened");
    assert_eq!(carriers, PROGRAM_CARRIERS, "documents carrying a program");
    assert_eq!(cut, CUT, "programs cut down");
    assert_eq!(whole, LEFT_WHOLE, "programs written through whole");
    assert_eq!(reasons, REASON_COUNTS, "why programs were left whole");
    assert_eq!(before_bytes, PROGRAM_BYTES_BEFORE, "font program bytes in");
    assert_eq!(after_bytes, PROGRAM_BYTES_AFTER, "font program bytes out");
    assert_eq!(
        glyph_counts_compared, GLYPH_COUNTS_COMPARED,
        "glyph counts actually compared, so an empty divergence list is a \
         measurement rather than a check that stopped running"
    );
    assert_eq!(
        length1_checked, LENGTH1_CHECKED,
        "/Length1 declarations actually read, for the same reason"
    );
}

// The numbers this stood at when it was written; see the assertions above.
//
// Measured 23 September 2026 over the 5 605 files `corpus/corpora.lock` pins,
// with `--release`, because a debug walk of them costs the better part of an
// hour. The counts are the profile's to be independent of and are: the same
// harness over the 976 pdf.js documents gives 455 carriers, 565 cut, 681
// whole and 71 032 402 -> 27 311 032 bytes in both profiles, byte for byte.
//
// **Three of the seven properties above failed on the first run**, which is
// what this census was written to find out and what no fixture in the tree
// could have told anybody:
//
// - Six "font dictionaries" changed in a key that is not `/BaseFont`, in
//   safedocs/0000215.pdf. That one was the census's own fault and `snapshot`
//   says what it was.
// - Two cut programs did not parse: pdfjs issue13193 and issue9262_reduced
//   embed a **TrueType Collection** as a `/FontFile2`, and
//   `tinker_pdf_font::subset` was writing the subset's `sfntVersion` from byte
//   zero of the original — so the output said `ttcf` while being a single flat
//   directory, and no reader on earth could open it. Fixed in
//   `crates/tinker-pdf-font/src/subset.rs`; `assemble`'s doc comment carries
//   the argument.
// - Seventy-three programs were in neither list: a `/FontDescriptor` embeds
//   them and no font dictionary names the descriptor, so the pass never saw
//   them and the report never mentioned them — while a `Rewrite` kept every
//   one of their outlines in the output. `UntouchedReason::NoFontNamesIt` now
//   names them.
//
// The pass took 93 seconds over the 2 180 documents that carry a program,
// which is the number `docs/features/editing.md` means by "a full
// interpretation of every page".
const DOCUMENTS: u32 = 5597;
const PROGRAM_CARRIERS: u32 = 2180;
const CUT: u32 = 5882;
const LEFT_WHOLE: u32 = 4950;
const REASON_COUNTS: [u32; 8] = [553, 3406, 581, 176, 151, 0, 10, 73];
const PROGRAM_BYTES_BEFORE: u64 = 669_086_159;
const PROGRAM_BYTES_AFTER: u64 = 401_630_511;
const GLYPH_COUNTS_COMPARED: u32 = 5882;
const LENGTH1_CHECKED: u32 = 5481;
