//! What the corpus's ICC profiles actually are (roadmap Tier 2, `design/icc.md`).
//!
//! `ColorSpace::Approximated` reads an `ICCBased` space by its component count
//! and nothing else, which is 8.6.5.5's alternate-space fallback stated on the
//! type. Replacing it with a real transform is a large capability, and ruling 3
//! wants it scheduled by what real documents carry rather than by what the
//! specification permits. The question this answers is the one that decides the
//! size of the work: **are the corpus's profiles matrix/TRC, or do they need
//! the LUT machinery?**
//!
//! A matrix/TRC profile is three `XYZ` tags and three tone curves — a few
//! hundred bytes of arithmetic. A LUT profile (`mft1`, `mft2`, `mAB `, `mBA `)
//! is a multi-dimensional interpolation table, and it is the milestone the
//! design doc sizes at L on its own.
//!
//! **This reads bytes and nothing else.** It does not use the colour crate, so
//! it cannot agree with a parser that is wrong; a census taken with the
//! decoder's own reader would confirm whatever that reader believed.
//!
//! ```sh
//! cargo test -p tinker-pdf --test icc_census -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, XrefEntry};

/// A four-byte ICC signature, as text.
fn sig(bytes: &[u8], at: usize) -> String {
    bytes
        .get(at..at + 4)
        .map(|s| String::from_utf8_lossy(s).trim_end().to_string())
        .unwrap_or_default()
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let s = bytes.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// What one profile is.
#[derive(Debug, Clone)]
struct Profile {
    class: String,
    space: String,
    pcs: String,
    version: u32,
    tags: Vec<String>,
    size: usize,
}

impl Profile {
    /// Whether the three matrix columns and three tone curves are all present,
    /// which is what a transform can be built from without any LUT.
    fn is_matrix_trc(&self) -> bool {
        ["rXYZ", "gXYZ", "bXYZ", "rTRC", "gTRC", "bTRC"]
            .iter()
            .all(|t| self.tags.iter().any(|have| have == t))
    }

    /// Whether it carries any of the four LUT tag types.
    fn lut_tags(&self) -> Vec<&str> {
        ["A2B0", "A2B1", "A2B2", "B2A0", "B2A1", "B2A2"]
            .iter()
            .filter(|t| self.tags.iter().any(|have| have == *t))
            .copied()
            .collect()
    }
}

/// Reads a profile's header and tag table. Bytes only — see the module note.
fn read_profile(bytes: &[u8]) -> Option<Profile> {
    // The magic at offset 36 is what makes this a profile rather than a stream
    // that happens to be long enough.
    if sig(bytes, 36) != "acsp" {
        return None;
    }
    let count = u32_at(bytes, 128)? as usize;
    if count > 1024 {
        return None;
    }
    let mut tags = Vec::with_capacity(count);
    for i in 0..count {
        let at = 132 + i * 12;
        if bytes.len() < at + 12 {
            break;
        }
        tags.push(sig(bytes, at));
    }
    Some(Profile {
        class: sig(bytes, 12),
        space: sig(bytes, 16),
        pcs: sig(bytes, 20),
        version: u32_at(bytes, 8)?,
        tags,
        size: bytes.len(),
    })
}

/// Every stream in one document that looks like an ICC profile.
fn profiles_in(bytes: Vec<u8>) -> Vec<Profile> {
    let Ok(doc) = Document::open(bytes) else {
        return Vec::new();
    };
    let cos = doc.cos();
    let mut out = Vec::new();
    for (number, entry) in cos.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let generation = match entry {
            XrefEntry::Offset { gen, .. } => gen,
            _ => 0,
        };
        let Ok(data) = cos.stream_decoded(ObjRef::new(number, generation)) else {
            continue;
        };
        if let Some(profile) = read_profile(&data) {
            out.push(profile);
        }
    }
    out
}

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

/// **What the corpus's ICC profiles are.** Ignored by default: it reads
/// thousands of fetched files and answers a scheduling question.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_icc_profiles() {
    let Some(root) = corpus_root() else {
        println!("icc-census: SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!("icc-census: RAN over {} files", files.len());

    let mut carriers = 0u32;
    let mut all: Vec<Profile> = Vec::new();
    let mut matrix_trc_files = 0u32;
    let mut lut_only_files = 0u32;
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let profiles = profiles_in(bytes);
        if profiles.is_empty() {
            continue;
        }
        carriers += 1;
        if profiles.iter().all(Profile::is_matrix_trc) {
            matrix_trc_files += 1;
        } else if profiles.iter().any(|p| !p.lut_tags().is_empty()) {
            lut_only_files += 1;
        }
        all.extend(profiles);
    }

    println!(
        "\n{carriers} files carry an ICC profile; {} profiles in all\n",
        all.len()
    );

    let mut classes: BTreeMap<String, u32> = BTreeMap::new();
    let mut spaces: BTreeMap<String, u32> = BTreeMap::new();
    let mut pcses: BTreeMap<String, u32> = BTreeMap::new();
    let mut tags: BTreeMap<String, u32> = BTreeMap::new();
    let (mut matrix, mut lut, mut neither) = (0u32, 0u32, 0u32);
    for profile in &all {
        *classes.entry(profile.class.clone()).or_default() += 1;
        *spaces.entry(profile.space.clone()).or_default() += 1;
        *pcses.entry(profile.pcs.clone()).or_default() += 1;
        for tag in &profile.tags {
            *tags.entry(tag.clone()).or_default() += 1;
        }
        if profile.is_matrix_trc() {
            matrix += 1;
        } else if !profile.lut_tags().is_empty() {
            lut += 1;
        } else {
            neither += 1;
        }
    }

    println!("--- the scheduling answer ---");
    println!("profiles that are matrix/TRC          {matrix:>6}");
    println!("profiles that need a LUT              {lut:>6}");
    println!("profiles that are neither             {neither:>6}");
    println!("files whose profiles are all matrix   {matrix_trc_files:>6}");
    println!("files carrying any LUT profile        {lut_only_files:>6}");

    let show = |name: &str, map: &BTreeMap<String, u32>| {
        println!("\n{name}:");
        let mut rows: Vec<(&String, &u32)> = map.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for (key, count) in rows.iter().take(12) {
            println!("  {count:>6}  {key}");
        }
    };
    show("device class", &classes);
    show("data colour space", &spaces);
    show("PCS", &pcses);
    show("tags", &tags);

    let versions: BTreeMap<u32, u32> = all.iter().fold(BTreeMap::new(), |mut acc, p| {
        *acc.entry(p.version >> 24).or_default() += 1;
        acc
    });
    println!("\nmajor version:");
    for (major, count) in &versions {
        println!("  {count:>6}  v{major}");
    }

    let largest = all.iter().map(|p| p.size).max().unwrap_or(0);
    println!("\nlargest profile: {largest} bytes");
}

/// **What the parser makes of them.** The other half of the census: the first
/// walks the bytes independently, this one asks the real
/// [`tinker_pdf_color::icc::Profile`] and counts what it says.
///
/// The two are deliberately separate. The census above establishes what is out
/// there without consulting the parser, so it cannot be wrong in the same way;
/// this one is the capability's own report card, and the interesting number is
/// how far the two disagree.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn what_the_parser_makes_of_the_corpus_profiles() {
    use tinker_pdf_color::icc::{IccError, Profile as IccProfile, Transform};

    let Some(root) = corpus_root() else {
        println!("icc-reality: SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!("icc-reality: RAN over {} files", files.len());

    // The raw profile bytes, not the census's summary of them.
    let mut streams: Vec<Vec<u8>> = Vec::new();
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(doc) = Document::open(bytes) else {
            continue;
        };
        let cos = doc.cos();
        for (number, entry) in cos.xref().iter() {
            if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
                continue;
            }
            let generation = match entry {
                XrefEntry::Offset { gen, .. } => gen,
                _ => 0,
            };
            let Ok(data) = cos.stream_decoded(ObjRef::new(number, generation)) else {
                continue;
            };
            if data.len() > 132 && data.get(36..40) == Some(b"acsp") {
                streams.push(data);
            }
        }
    }

    let mut compiled = 0u32;
    let mut parsed_no_transform = 0u32;
    let mut refused: BTreeMap<String, u32> = BTreeMap::new();
    for bytes in &streams {
        match IccProfile::parse(bytes) {
            Ok(profile) => {
                if Transform::compile(&profile).is_some() {
                    compiled += 1;
                } else {
                    parsed_no_transform += 1;
                }
            }
            Err(error) => {
                let name = match error {
                    IccError::TooShort => "TooShort",
                    IccError::NotAProfile => "NotAProfile",
                    IccError::SizeMismatch => "SizeMismatch",
                    IccError::TagOutOfBounds => "TagOutOfBounds",
                    IccError::TooLarge => "TooLarge",
                    IccError::NeedsLut => "NeedsLut",
                    IccError::UnsupportedPcs => "UnsupportedPcs",
                    IccError::UnsupportedSpace => "UnsupportedSpace",
                    IccError::MissingTags => "MissingTags",
                    IccError::MalformedTag => "MalformedTag",
                };
                *refused.entry(name.to_string()).or_default() += 1;
            }
        }
    }

    let total = streams.len() as u32;
    println!("\n{total} profile streams\n");
    println!(
        "compiled to a transform     {compiled:>6}  ({:.1} %)",
        f64::from(compiled) * 100.0 / f64::from(total.max(1))
    );
    println!("parsed, no transform        {parsed_no_transform:>6}");
    println!(
        "refused by name             {:>6}",
        total - compiled - parsed_no_transform
    );
    println!("\nrefusals, by reason:");
    let mut rows: Vec<(&String, &u32)> = refused.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in rows {
        println!("  {count:>6}  {name}");
    }
}
