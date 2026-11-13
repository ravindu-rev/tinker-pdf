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
//! `what_the_parser_makes_of_the_corpus_profiles` then asks the parser and
//! holds its answer as a ratchet: [`STREAMS`] exact, being the lock file's
//! population, [`COMPILED`] a floor and [`REFUSED_SHAPES`] a ceiling. A run
//! that compiles more, or refuses no profile in one of the listed shapes,
//! prints the gain to record; one that compiles fewer, or refuses a profile
//! in a shape the list does not hold, fails.
//! `a_census_that_compiles_more_is_a_gain_and_less_a_failure` holds that
//! comparison on every run, with a corpus or without one.
//!
//! ```sh
//! cargo test -p tinker-pdf --test icc_census -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, XrefEntry};

/// A four-byte ICC signature, as text: empty where the four bytes are not
/// all inside `bytes`.
fn sig(bytes: &[u8], at: usize) -> String {
    bytes
        .get(at..at.saturating_add(4))
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
    /// The type signature each tag's data starts with, in `tags`' order: the
    /// structure a table tag holds (`mft2`, `mAB`, `mpet`, …), empty where the
    /// tag's offset falls outside the profile.
    kinds: Vec<String>,
    size: usize,
}

impl Profile {
    /// The version as ICC.1 7.2.4 writes it: the major revision, then the
    /// minor and bug-fix revisions a nibble each, `4.3.0`.
    fn version_text(&self) -> String {
        format!(
            "{}.{}.{}",
            self.version >> 24,
            (self.version >> 20) & 0xF,
            (self.version >> 16) & 0xF
        )
    }

    /// Each tag with the structure its data holds, `A2B0=mft2`.
    fn tag_kinds(&self) -> Vec<String> {
        self.tags
            .iter()
            .zip(&self.kinds)
            .map(|(tag, kind)| format!("{tag}={kind}"))
            .collect()
    }

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

    /// The `A2B*` tags it carries, which a source colour converts through.
    fn a2b_tags(&self) -> Vec<&str> {
        self.lut_tags()
            .into_iter()
            .filter(|tag| tag.starts_with("A2B"))
            .collect()
    }

    /// Whether it carries a model — the matrix/TRC tags, or a grey `kTRC` —
    /// beside `A2B*` tables: the profiles ICC.1 8.10.2 converts through their
    /// tables first, and the model only for an intent whose tables it does
    /// not carry (`docs/design/icc.md`, "The table each intent compiles").
    fn carries_a_model_and_tables(&self) -> bool {
        let grey = self.tags.iter().any(|have| have == "kTRC");
        (self.is_matrix_trc() || grey) && !self.a2b_tags().is_empty()
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
    let mut kinds = Vec::with_capacity(count);
    for i in 0..count {
        let at = 132 + i * 12;
        if bytes.len() < at + 12 {
            break;
        }
        tags.push(sig(bytes, at));
        // A tag table entry is its signature, its data's offset and its size
        // (ICC.1 7.3, Table 24); the data starts with its type's signature.
        let kind =
            u32_at(bytes, at + 4).map_or_else(String::new, |offset| sig(bytes, offset as usize));
        kinds.push(kind);
    }
    Some(Profile {
        class: sig(bytes, 12),
        space: sig(bytes, 16),
        pcs: sig(bytes, 20),
        version: u32_at(bytes, 8)?,
        tags,
        kinds,
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
        assert!(
            !required(),
            "icc-census: TINKER_CORPUS_REQUIRED is set and there is no corpus: \
             this census would have passed over nothing"
        );
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

    // The profiles whose rendering ICC.1 8.10.2's order moved: a model and
    // tables both, converted through the tables an intent names before the
    // model. Which `A2B*` tags they carry says which intents moved — all of
    // them where `A2B0` is there, and only the intents whose own tag is
    // otherwise. Roadmap step RC-01.9b commits the count.
    let both: Vec<&Profile> = all
        .iter()
        .filter(|profile| profile.carries_a_model_and_tables())
        .collect();
    let mut by_tables: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, u32> = BTreeMap::new();
    for profile in &both {
        *by_tables.entry(profile.a2b_tags().join(" ")).or_default() += 1;
        let model = if profile.is_matrix_trc() {
            "matrix/TRC"
        } else {
            "grey"
        };
        let kind = format!(
            "{} v{} {} -> {}, {model}",
            profile.class,
            profile.version >> 24,
            profile.space,
            profile.pcs
        );
        *by_kind.entry(kind).or_default() += 1;
    }
    println!("\n--- a model beside tables (ICC.1 8.10.2) ---");
    println!(
        "profiles carrying a model and A2B* tables  {:>6}",
        both.len()
    );
    println!("by the A2B* tags they carry:");
    for (tables, count) in &by_tables {
        println!("  {count:>6}  {tables}");
    }
    println!("by class, version, spaces and model:");
    for (kind, count) in &by_kind {
        println!("  {count:>6}  {kind}");
    }

    // What the rest of the format brings the corpus: the classes, tags and
    // version the output direction, links, the gamut tag, named colours and
    // ICC.2 add (`docs/design/icc.md`, "ICC engine completion").
    let of_class = |class: &str| all.iter().filter(|p| p.class == class).count();
    let carrying = |tag: &str| {
        all.iter()
            .filter(|p| p.tags.iter().any(|have| have == tag))
            .count()
    };
    let carrying_one_of = |prefix: &str| {
        all.iter()
            .filter(|p| p.tags.iter().any(|have| have.starts_with(prefix)))
            .count()
    };
    println!("\n--- beyond the input direction ---");
    println!(
        "profiles of class link                {:>6}",
        of_class("link")
    );
    println!(
        "profiles of class nmcl                {:>6}",
        of_class("nmcl")
    );
    println!(
        "profiles carrying a B2A* tag          {:>6}",
        carrying_one_of("B2A")
    );
    println!(
        "profiles carrying a D2B* tag          {:>6}",
        carrying_one_of("D2B")
    );
    println!(
        "profiles carrying a B2D* tag          {:>6}",
        carrying_one_of("B2D")
    );
    println!(
        "profiles carrying gamt                {:>6}",
        carrying("gamt")
    );
    println!(
        "profiles carrying ncl2                {:>6}",
        carrying("ncl2")
    );
    let v5: Vec<&Profile> = all.iter().filter(|p| p.version >> 24 == 5).collect();
    println!("version 5 profiles                    {:>6}", v5.len());
    let mut v5_tags: BTreeMap<String, u32> = BTreeMap::new();
    for profile in &v5 {
        for tag in profile.tag_kinds() {
            *v5_tags.entry(tag).or_default() += 1;
        }
    }
    if !v5_tags.is_empty() {
        println!("their tags, each with the structure it holds:");
        for (tag, count) in &v5_tags {
            println!("  {count:>6}  {tag}");
        }
    }

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
    use tinker_pdf_color::icc::{IccError, Intent, Profile as IccProfile, Transform};

    let Some(root) = corpus_root() else {
        println!("icc-reality: SKIPPED (no corpus; set TINKER_CORPUS)");
        assert!(
            !required(),
            "icc-reality: TINKER_CORPUS_REQUIRED is set and there is no corpus: \
             this census would have passed over nothing"
        );
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
    let mut shapes: Vec<String> = Vec::new();
    let mut refused_profiles: BTreeMap<String, u32> = BTreeMap::new();
    for bytes in &streams {
        match IccProfile::parse(bytes) {
            Ok(profile) => {
                if Transform::compile(&profile, Intent::RelativeColorimetric).is_some() {
                    compiled += 1;
                } else {
                    parsed_no_transform += 1;
                }
            }
            Err(error) => {
                // What the refused profile *is*, so a row that says "refused
                // by name with its count" can also say what the file did.
                let shape = format!(
                    "{} {} -> {}",
                    sig(bytes, 12),
                    sig(bytes, 16),
                    sig(bytes, 20)
                );
                shapes.push(format!("{error:?}: {shape}"));
                // And the whole of it, read from the bytes as the first census
                // reads them: its version, and each tag with the structure its
                // data holds, so a table tag says which of `mft1`, `mft2`,
                // `mAB`, `mBA` or `mpet` it is.
                let detail = match read_profile(bytes) {
                    Some(found) => format!(
                        "{error:?}: v{} {} {} -> {}: {}",
                        found.version_text(),
                        found.class,
                        found.space,
                        found.pcs,
                        found.tag_kinds().join(" ")
                    ),
                    None => format!("{error:?}: {shape}: no tag table to read"),
                };
                *refused_profiles.entry(detail).or_default() += 1;
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
                    IccError::UnsupportedElement(_) => "UnsupportedElement",
                    // The output direction's refusal, which a parse never
                    // makes; named all the same, so a parse that began to
                    // make it would be counted as what it is.
                    IccError::NoInverse => "NoInverse",
                    // A calculator past a cap, which only a version 5
                    // profile's elements hold.
                    IccError::CalculatorLimit(_) => "CalculatorLimit",
                    // A spectral connection space or viewing conditions not
                    // reduced or connected, which only a version 5 profile
                    // names.
                    IccError::Spectral(_) => "Spectral",
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
    println!(
        "
what each refused profile is (class, data space, PCS):"
    );
    shapes.sort();
    shapes.dedup();
    for shape in &shapes {
        println!("  {shape}");
    }
    println!(
        "
each refused profile (version, class, data space -> PCS, each tag=its structure):"
    );
    for (detail, count) in &refused_profiles {
        println!("  {count:>6}  {detail}");
    }

    // The population is the lock file's, so it is exact; what the parser
    // makes of it is the ratchet.
    assert_eq!(total, STREAMS, "the corpus's profile population moved");
    match ratchet(compiled, &shapes, COMPILED, &REFUSED_SHAPES) {
        Ok(gain) => {
            if gain.compiled > 0 {
                println!(
                    "\ngain: {compiled} compile, {} more than COMPILED's {COMPILED}; \
                     raise it to record them",
                    gain.compiled
                );
            }
            for shape in &gain.cleared {
                println!("gain: no profile is refused as `{shape}`; take it out of REFUSED_SHAPES");
            }
        }
        Err(losses) => panic!("icc-reality: {}", losses.join("; ")),
    }
}

/// What a census run gained against the pins: [`ratchet`]'s answer when it
/// lost nothing.
#[derive(Debug, PartialEq, Eq)]
struct Gain {
    /// How many more profiles compiled than the floor.
    compiled: u32,
    /// The ceiling's shapes no profile was refused in, in the ceiling's order.
    cleared: Vec<String>,
}

/// **The ratchet.** A run's compiled count against a floor and its refused
/// shapes against a ceiling: what it gained, or else each thing it lost — a
/// compiled count under the floor, and every refused shape the ceiling does
/// not hold. A gain is never a failure, so a parser that reads one more
/// corpus profile keeps the census green, and the commit that records the run
/// raises the floor to it and narrows the ceiling.
fn ratchet(
    compiled: u32,
    shapes: &[String],
    floor: u32,
    ceiling: &[&str],
) -> Result<Gain, Vec<String>> {
    let mut losses = Vec::new();
    if compiled < floor {
        losses.push(format!(
            "the compiled count fell from {floor} to {compiled}"
        ));
    }
    for shape in shapes {
        if !ceiling.contains(&shape.as_str()) {
            losses.push(format!(
                "a profile is refused as `{shape}`, a shape REFUSED_SHAPES does not hold"
            ));
        }
    }
    if !losses.is_empty() {
        return Err(losses);
    }
    let cleared = ceiling
        .iter()
        .filter(|pinned| !shapes.iter().any(|shape| shape.as_str() == **pinned))
        .map(|pinned| (*pinned).to_string())
        .collect();
    Ok(Gain {
        compiled: compiled - floor,
        cleared,
    })
}

/// **The ratchet, on counts and shapes made up here.** It needs no corpus, so
/// the comparison the parser census rests on is held on every run of the
/// suite, where the census itself is ignored.
#[test]
fn a_census_that_compiles_more_is_a_gain_and_less_a_failure() {
    fn owned(list: &[&str]) -> Vec<String> {
        list.iter().map(|shape| (*shape).to_string()).collect()
    }
    const LAB: &str = "UnsupportedSpace: mntr LAB -> XYZ";
    const INK: &str = "UnsupportedSpace: prtr CMYK -> Lab";
    const NEW: &str = "MissingTags: scnr RGB -> XYZ";
    let ceiling = [LAB, INK];
    let held = Gain {
        compiled: 0,
        cleared: Vec::new(),
    };

    // As pinned: nothing gained and nothing lost.
    assert_eq!(ratchet(100, &owned(&[LAB, INK]), 100, &ceiling), Ok(held));
    // More compiled is a gain, and so is a listed shape no profile has now.
    assert_eq!(
        ratchet(103, &owned(&[LAB]), 100, &ceiling),
        Ok(Gain {
            compiled: 3,
            cleared: owned(&[INK]),
        })
    );
    assert_eq!(
        ratchet(100, &[], 100, &ceiling),
        Ok(Gain {
            compiled: 0,
            cleared: owned(&[LAB, INK]),
        })
    );
    // Fewer compiled fails, whatever else improved.
    assert_eq!(
        ratchet(99, &[], 100, &ceiling),
        Err(owned(&["the compiled count fell from 100 to 99"]))
    );
    // A refused shape the ceiling does not hold fails, even beside a gain.
    let refused = ratchet(120, &owned(&[LAB, NEW]), 100, &ceiling).unwrap_err();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains(NEW), "{refused:?}");
    // Both at once: each loss is named.
    let both = ratchet(98, &owned(&[NEW, INK]), 100, &ceiling).unwrap_err();
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(both[0].contains("fell from 100 to 98"), "{both:?}");
    assert!(both[1].contains(NEW), "{both:?}");
    // The file's own pins hold against themselves.
    assert_eq!(
        ratchet(COMPILED, &owned(&REFUSED_SHAPES), COMPILED, &REFUSED_SHAPES),
        Ok(Gain {
            compiled: 0,
            cleared: Vec::new(),
        })
    );
}

/// **The censuses' byte reader, on a profile made up here**: the version as
/// ICC writes it, each tag with the structure its data starts with — none for
/// a tag whose offset is past the end — and the families the LUT census groups
/// table tags into. These are what the refused profiles' lines and the counts
/// beyond the input direction are made of, so they are held without a corpus.
#[test]
fn the_byte_reader_names_each_tag_and_the_structure_it_holds() {
    let table = 132 + 3 * 12;
    let mut bytes = vec![0u8; table + 8];
    bytes[8..12].copy_from_slice(&[0x04, 0x30, 0x00, 0x00]);
    bytes[12..16].copy_from_slice(b"prtr");
    bytes[16..20].copy_from_slice(b"CMYK");
    bytes[20..24].copy_from_slice(b"Lab ");
    bytes[36..40].copy_from_slice(b"acsp");
    bytes[128..132].copy_from_slice(&3u32.to_be_bytes());
    let entries: [(&[u8; 4], usize); 3] =
        [(b"A2B0", table), (b"B2A0", table + 4), (b"gamt", 10_000)];
    for (i, (name, offset)) in entries.iter().enumerate() {
        let at = 132 + i * 12;
        bytes[at..at + 4].copy_from_slice(*name);
        bytes[at + 4..at + 8].copy_from_slice(&(*offset as u32).to_be_bytes());
        bytes[at + 8..at + 12].copy_from_slice(&4u32.to_be_bytes());
    }
    bytes[table..table + 4].copy_from_slice(b"mft2");
    bytes[table + 4..table + 8].copy_from_slice(b"mBA ");

    let profile = read_profile(&bytes).expect("the header and tag table read");
    assert_eq!(profile.version_text(), "4.3.0");
    assert_eq!(
        (
            profile.class.as_str(),
            profile.space.as_str(),
            profile.pcs.as_str()
        ),
        ("prtr", "CMYK", "Lab")
    );
    assert_eq!(profile.tag_kinds(), ["A2B0=mft2", "B2A0=mBA", "gamt="]);
    let families: Vec<Option<&str>> = profile
        .tags
        .iter()
        .map(|tag| table_family(tag.as_str()))
        .collect();
    assert_eq!(families, [Some("A2B*"), Some("B2A*"), Some("gamt")]);
    assert_eq!(table_family("D2B3"), Some("D2B*"));
    assert_eq!(table_family("B2D0"), Some("B2D*"));
    assert_eq!(table_family("rXYZ"), None);
    assert_eq!(table_family("A2"), None);
    // A version 2 header, its minor and bug-fix nibbles, and a stream without
    // the profile magic.
    bytes[8..12].copy_from_slice(&[0x02, 0x10, 0x00, 0x00]);
    assert_eq!(
        read_profile(&bytes).expect("a profile").version_text(),
        "2.1.0"
    );
    bytes[9] = 0x42;
    assert_eq!(
        read_profile(&bytes).expect("a profile").version_text(),
        "2.4.2"
    );
    bytes[36..40].copy_from_slice(b"none");
    assert!(read_profile(&bytes).is_none(), "no acsp, no profile");
}

/// Profile streams across the five corpora: the population
/// `corpus/corpora.lock` fetches, so a census that finds another number has
/// read another corpus.
const STREAMS: u32 = 3235;

/// Of those, the fewest that may compile to a transform: a floor. A run that
/// compiles more prints the gain, and the commit recording that run raises
/// the floor to it; one that compiles fewer fails.
const COMPILED: u32 = 3229;

/// **The shapes a refused profile may have**: a ceiling. A run in which no
/// profile is refused in one of these prints that as a gain, and the commit
/// recording it takes the shape out; a profile refused in any other shape
/// fails the census.
///
/// All three shapes are profiles that contradict themselves rather than
/// capabilities this build lacks, which is the distinction the roadmap row
/// wanted and the reason this list is an exit criterion rather than a count:
///
/// - `mntr LAB -> XYZ` carries `rXYZ`/`gXYZ`/`bXYZ` columns, and a matrix
///   applies to linear RGB. Lab is not that, and multiplying it by one
///   produces a colour rather than an error.
/// - `mntr YYY -> XYZ` names a three-channel data space no registry defines,
///   so nothing says what its components mean.
/// - `prtr CMYK -> Lab` reaches the grey branch, which means it carries a
///   single `kTRC` and no matrix — one curve for four channels of ink.
///
/// A table beside such a model is what the profile converts through
/// (`docs/design/icc.md`, "The table each intent compiles"), so a profile of
/// one of these shapes carrying an `A2B*` table that reads compiles, and its
/// shape clears as a gain.
const REFUSED_SHAPES: [&str; 3] = [
    "UnsupportedSpace: mntr LAB -> XYZ",
    "UnsupportedSpace: mntr YYY -> XYZ",
    "UnsupportedSpace: prtr CMYK -> Lab",
];

/// **What the LUT profiles are made of.** The scoping question for
/// `design/icc.md`'s milestone 5, LUT profiles.
///
/// `A2B0` is a *tag name*; what sits at it is one of four different structures.
/// `mft1` and `mft2` are v2's, a matrix and three stages of table; `mAB ` and
/// `mBA ` are v4's, which add per-channel curves either side and carry their
/// own offsets. Which of the four the corpus actually uses decides whether this
/// is one structure or four.
///
/// Every table tag is counted so, by [`table_family`]: the forward `A2B*`, the
/// inverse `B2A*` an output transform reads, the multi-process `D2B*` and
/// `B2D*`, and the gamut tag `gamt` (`docs/design/icc.md`, "ICC engine
/// completion").
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_lut_profiles() {
    let Some(root) = corpus_root() else {
        println!("lut-census: SKIPPED (no corpus; set TINKER_CORPUS)");
        assert!(
            !required(),
            "lut-census: TINKER_CORPUS_REQUIRED is set and there is no corpus: \
             this census would have passed over nothing"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();

    let mut tags: BTreeMap<&str, u32> = BTreeMap::new();
    let mut kinds: BTreeMap<(&str, String), u32> = BTreeMap::new();
    let mut shapes: BTreeMap<String, u32> = BTreeMap::new();
    let mut grids: BTreeMap<(&str, u8), u32> = BTreeMap::new();

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
            if data.len() < 132 || data.get(36..40) != Some(b"acsp") {
                continue;
            }
            let Some(count) = u32_at(&data, 128) else {
                continue;
            };
            if count > 1024 {
                continue;
            }
            for i in 0..count as usize {
                let at = 132 + i * 12;
                if data.len() < at + 12 {
                    break;
                }
                let name = sig(&data, at);
                let Some(family) = table_family(&name) else {
                    continue;
                };
                let Some(offset) = u32_at(&data, at + 4).map(|v| v as usize) else {
                    continue;
                };
                if data.len().saturating_sub(12) < offset {
                    continue;
                }
                *tags.entry(family).or_default() += 1;
                let kind = sig(&data, offset);
                if kind == "mft1" || kind == "mft2" {
                    let inputs = data[offset + 8];
                    let outputs = data[offset + 9];
                    let grid = data[offset + 10];
                    *shapes
                        .entry(format!("{family} {kind} {inputs}->{outputs}"))
                        .or_default() += 1;
                    *grids.entry((family, grid)).or_default() += 1;
                }
                *kinds.entry((family, kind)).or_default() += 1;
            }
        }
    }

    println!("lut-census: RAN");
    for (family, count) in &tags {
        println!("{count} {family} tags");
    }
    println!("\nstructure at the tag:");
    for ((family, kind), count) in &kinds {
        println!("  {count:>5}  {family} {kind}");
    }
    println!("\nshape (v2 tags only):");
    let mut rows: Vec<(&String, &u32)> = shapes.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1));
    for (shape, count) in rows.iter().take(16) {
        println!("  {count:>5}  {shape}");
    }
    println!("\nCLUT grid points per axis (v2 tags only):");
    for ((family, grid), count) in &grids {
        println!("  {count:>5}  {family} {grid}");
    }
}

/// The table tags [`census_of_the_lut_profiles`] reads the structure of, by
/// family: the forward and inverse tables of ICC.1 9.2, the multi-process
/// tags beside them, and the gamut tag.
fn table_family(tag: &str) -> Option<&'static str> {
    if tag == "gamt" {
        return Some("gamt");
    }
    match tag.get(..3)? {
        "A2B" => Some("A2B*"),
        "B2A" => Some("B2A*"),
        "D2B" => Some("D2B*"),
        "B2D" => Some("B2D*"),
        _ => None,
    }
}

/// **Does the lookup-table path produce the colours ink makes?**
///
/// There is no second CMM to ask (ruling 13) and no published table for a
/// printer profile, so the check is a property every CMYK profile must have
/// whatever its make: paper is light, black ink is dark, and each primary is
/// its own hue. Over hundreds of real profiles from real producers, those hold
/// or the decoding is wrong — a shifted Lab axis or a mis-strided grid moves
/// hues, and a hue that has moved fails these.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn what_the_lookup_tables_make_of_ink() {
    use tinker_pdf_color::icc::{Intent, Profile as IccProfile, Transform};

    let Some(root) = corpus_root() else {
        println!("lut-reality: SKIPPED (no corpus; set TINKER_CORPUS)");
        assert!(
            !required(),
            "lut-reality: TINKER_CORPUS_REQUIRED is set and there is no corpus: \
             this census would have passed over nothing"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();

    // One of each distinct profile, by its bytes, so a profile embedded in a
    // hundred files is judged once.
    let mut seen: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
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
            if data.len() > 132
                && data.get(36..40) == Some(b"acsp")
                && data.get(16..20) == Some(b"CMYK")
            {
                seen.insert(data);
            }
        }
    }

    let (mut judged, mut paper, mut ink, mut cyan, mut magenta, mut yellow) = (0, 0, 0, 0, 0, 0);
    let mut failures: Vec<String> = Vec::new();
    for bytes in &seen {
        let Ok(profile) = IccProfile::parse(bytes) else {
            continue;
        };
        let Some(transform) = Transform::compile(&profile, Intent::RelativeColorimetric) else {
            continue;
        };
        judged += 1;
        let at = |c: [f64; 4]| transform.apply(&c);

        let white = at([0.0, 0.0, 0.0, 0.0]);
        let black = at([0.0, 0.0, 0.0, 1.0]);
        let c = at([1.0, 0.0, 0.0, 0.0]);
        let m = at([0.0, 1.0, 0.0, 0.0]);
        let y = at([0.0, 0.0, 1.0, 0.0]);

        let luma = |p: (u8, u8, u8)| u32::from(p.0) + u32::from(p.1) + u32::from(p.2);
        if luma(white) > 600 {
            paper += 1;
        } else if failures.len() < 5 {
            failures.push(format!("no ink is {white:?}, which is not paper"));
        }
        if luma(black) < 300 {
            ink += 1;
        } else if failures.len() < 5 {
            failures.push(format!("full black is {black:?}, which is not ink"));
        }
        // Cyan absorbs red; magenta absorbs green; yellow absorbs blue.
        if c.0 < c.1 && c.0 < c.2 {
            cyan += 1;
        } else if failures.len() < 5 {
            failures.push(format!("cyan is {c:?}"));
        }
        if m.1 < m.0 && m.1 < m.2 {
            magenta += 1;
        } else if failures.len() < 5 {
            failures.push(format!("magenta is {m:?}"));
        }
        if y.2 < y.0 && y.2 < y.1 {
            yellow += 1;
        } else if failures.len() < 5 {
            failures.push(format!("yellow is {y:?}"));
        }
    }

    println!(
        "lut-reality: RAN over {} distinct CMYK profiles",
        seen.len()
    );
    println!("judged (parsed and compiled)  {judged:>5}");
    println!("no ink is paper               {paper:>5}");
    println!("full black is ink             {ink:>5}");
    println!("cyan absorbs red              {cyan:>5}");
    println!("magenta absorbs green         {magenta:>5}");
    println!("yellow absorbs blue           {yellow:>5}");
    for line in &failures {
        println!("  ! {line}");
    }

    assert!(
        judged > 0,
        "no CMYK profile compiled, so this measured nothing"
    );
    // Every one of the five, on every profile. A single exception is a
    // decoding defect rather than an unusual profile: these are properties of
    // ink, not of a vendor's rendering.
    for (name, count) in [
        ("paper", paper),
        ("ink", ink),
        ("cyan", cyan),
        ("magenta", magenta),
        ("yellow", yellow),
    ] {
        assert_eq!(
            count, judged,
            "{name}: {count} of {judged} profiles agree, so the table is being \
             read wrongly rather than a profile being unusual"
        );
    }
}

/// Whether a missing corpus is a failure rather than a skip: the corpus job
/// sets `TINKER_CORPUS_REQUIRED`, so a census there cannot pass over nothing.
fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
}
