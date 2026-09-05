//! **What `cmap` subtables the corpus's embedded faces actually carry**
//! (roadmap Tier 2).
//!
//! Two capabilities are refused in `tinker-pdf-font` rather than here: a
//! `cmap` of format 13, and a Macintosh subtable read in a non-Roman encoding.
//! Both were roadmap rows whose exit criterion was *a count first*, and
//! neither leaves a warning behind — `lookup_cmap` returns `None` and
//! `glyph_for_char` returns `None`, so no corpus run can count them. This is
//! the instrument that can.
//!
//! **The parser here is written from the OpenType `cmap` chapter and shares
//! nothing with `tinker_pdf_font`**, for `jbig2_census.rs`'s reason: a census
//! taken with the reader's own directory walk would agree with that reader by
//! construction, including wherever it is wrong.
//!
//! ```sh
//! cargo test -p tinker-pdf --test cmap_census -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};

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

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Every embedded font program in one document.
fn font_programs(bytes: Vec<u8>) -> Vec<Vec<u8>> {
    let Ok(doc) = Document::open(bytes) else {
        return Vec::new();
    };
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
        for key in [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"] {
            if let Some(reference) = dict.get_ref(cos.intern(key)) {
                refs.insert((reference.num, reference.gen));
            }
        }
    }

    refs.into_iter()
        .filter_map(|(num, gen)| cos.stream_decoded(ObjRef::new(num, gen)).ok())
        .collect()
}

/// The `cmap` subtables of one sfnt: `(platform, encoding, language, format)`.
///
/// Written from the table layout rather than from `tinker_pdf_font`: the sfnt
/// directory is a count at offset 4 with sixteen-byte records after it, and the
/// `cmap` table is a count at offset 2 with eight-byte records after that.
fn cmap_subtables(program: &[u8]) -> Vec<(u16, u16, u32, u16)> {
    let mut out = Vec::new();
    let Some(tables) = be16(program, 4) else {
        return out;
    };
    let mut offset = None;
    for i in 0..usize::from(tables).min(512) {
        let at = 12 + i * 16;
        let (Some(tag), Some(start)) = (be32(program, at), be32(program, at + 8)) else {
            break;
        };
        if tag == 0x636D_6170 {
            offset = Some(start as usize);
            break;
        }
    }
    let Some(cmap) = offset.and_then(|start| program.get(start..)) else {
        return out;
    };
    let Some(count) = be16(cmap, 2) else {
        return out;
    };
    for i in 0..usize::from(count).min(64) {
        let at = 4 + i * 8;
        let (Some(platform), Some(encoding), Some(start)) =
            (be16(cmap, at), be16(cmap, at + 2), be32(cmap, at + 4))
        else {
            break;
        };
        let Some(sub) = cmap.get(start as usize..) else {
            continue;
        };
        let Some(format) = be16(sub, 0) else {
            continue;
        };
        // The language field is two bytes at offset 4 for the sixteen-bit
        // subtables and four bytes at offset 8 for the thirty-two-bit ones,
        // which is the same split as their length fields.
        let language = match format {
            0 | 2 | 4 | 6 => be16(sub, 4).map(u32::from),
            _ => be32(sub, 8),
        }
        .unwrap_or(0);
        out.push((platform, encoding, language, format));
    }
    out
}

/// **What the corpus's faces carry, and what this build cannot read.**
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_cmap_subtables() {
    let Some(root) = corpus_root() else {
        println!("cmap-census: SKIPPED (no corpus; set TINKER_CORPUS)");
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
    println!("cmap-census: RAN over {} files", files.len());

    let mut faces = 0u32;
    let mut with_cmap = 0u32;
    let mut formats: BTreeMap<u16, u32> = BTreeMap::new();
    let mut pairs: BTreeMap<(u16, u16), u32> = BTreeMap::new();
    let mut mac_languages: BTreeMap<u32, u32> = BTreeMap::new();
    let mut format13_faces = 0u32;
    let mut mac_non_roman_faces = 0u32;
    let mut only_mac_non_roman = 0u32;
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        for program in font_programs(bytes) {
            if !seen.insert(program.clone()) {
                continue; // one face embedded in a hundred files is one face
            }
            faces += 1;
            let subtables = cmap_subtables(&program);
            if subtables.is_empty() {
                continue;
            }
            with_cmap += 1;
            for (platform, encoding, language, format) in &subtables {
                *formats.entry(*format).or_default() += 1;
                *pairs.entry((*platform, *encoding)).or_default() += 1;
                if *platform == 1 {
                    *mac_languages.entry(*language).or_default() += 1;
                }
            }
            if subtables.iter().any(|(_, _, _, format)| *format == 13) {
                format13_faces += 1;
            }
            // A Macintosh subtable whose language names an encoding other than
            // Roman: the byte map is in a legacy encoding, and 0 or 1 is the
            // language-independent Roman case.
            let mac_non_roman =
                |(platform, _, language, _): &(u16, u16, u32, u16)| *platform == 1 && *language > 1;
            if subtables.iter().any(mac_non_roman) {
                mac_non_roman_faces += 1;
                // The one that actually costs a glyph: no Unicode subtable to
                // fall back to.
                if subtables
                    .iter()
                    .all(|entry| mac_non_roman(entry) || entry.0 == 1)
                {
                    only_mac_non_roman += 1;
                }
            }
        }
    }

    println!("\n{faces} distinct embedded faces; {with_cmap} carry a `cmap`\n");
    println!("--- the scheduling answer ---");
    println!("faces with a format 13 subtable            {format13_faces:>6}");
    println!("faces with a Macintosh non-Roman subtable  {mac_non_roman_faces:>6}");
    println!("  ...and no Unicode subtable beside it     {only_mac_non_roman:>6}");

    println!("\nsubtable format:");
    for (format, count) in &formats {
        println!("  {count:>6}  format {format}");
    }
    println!("\nplatform and encoding:");
    let mut rows: Vec<(&(u16, u16), &u32)> = pairs.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1));
    for ((platform, encoding), count) in rows.iter().take(12) {
        println!("  {count:>6}  ({platform}, {encoding})");
    }
    println!("\nMacintosh subtable language:");
    for (language, count) in &mac_languages {
        println!("  {count:>6}  language {language}");
    }

    // Pinned against `corpus/corpora.lock` as it stands, 6 September 2026.
    assert_eq!(faces, FACES, "the corpus's embedded-face population moved");
    assert_eq!(with_cmap, WITH_CMAP, "the faces carrying a `cmap` moved");
    assert_eq!(format13_faces, FORMAT_13, "the format 13 count moved");
    assert_eq!(
        mac_non_roman_faces, MAC_NON_ROMAN,
        "the Macintosh non-Roman count moved"
    );
    assert_eq!(
        only_mac_non_roman, 0,
        "a face now depends on a Macintosh non-Roman subtable with no Unicode \
         subtable beside it, which is the case `glyph_for_char` returns `None` \
         for — it is a lost glyph rather than a harmless extra subtable, and \
         the roadmap row that says nothing is reached is no longer true"
    );
    assert_eq!(
        formats.get(&2).copied().unwrap_or(0),
        FORMAT_2,
        "the format 2 count moved"
    );
}

/// Distinct embedded font programs across the five corpora.
const FACES: u32 = 7905;

/// Of those, the ones carrying a `cmap` at all — the rest are bare CFF, which a
/// PDF indexes by its own encoding rather than by character.
const WITH_CMAP: u32 = 3631;

/// **Faces with a format 13 subtable: none.**
///
/// The many-to-one range format is read now, and this is the count the roadmap
/// row asked for before it was: zero. The capability was built anyway, on a
/// decision recorded in the roadmap, and what adjudicates it is the committed
/// `text-rendering-tests` section CMAP-4 rather than a corpus document.
const FORMAT_13: u32 = 0;

/// Faces with a Macintosh subtable whose language names a non-Roman encoding.
///
/// Twenty-eight, and **every one of them carries a Unicode subtable as well**,
/// which `glyph_for_char` scores above it. So no corpus document loses a glyph
/// to the byte map this build has no conversion table for — the assertion above
/// is what would notice if one arrived.
const MAC_NON_ROMAN: u32 = 28;

/// Format 2, the high-byte mapping the legacy CJK encodings use.
///
/// Twenty-five subtables, and `lookup_cmap` covers 0, 4, 6, 12 and 13 — so this
/// is a refusal nobody had counted, found by pointing the instrument at the
/// question the roadmap actually asked. Whether any of the twenty-five is a
/// face's *only* subtable is the number that would schedule it.
const FORMAT_2: u32 = 25;
