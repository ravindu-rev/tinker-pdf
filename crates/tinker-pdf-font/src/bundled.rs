//! The faces this build carries: the twelve Liberation faces behind
//! `bundled-fonts`, and the four CJK fallback faces behind `cjk-fonts`, which
//! `bundled-fonts` turns on (`docs/design/fallback-faces.md`).
//!
//! ## Why there are any
//!
//! 9.6.2.2 requires a reader to have the standard 14 fonts, so most documents
//! do not embed them. This engine had none, and the cost of that was measured
//! rather than argued: `corpus/ratchet-fonts.json` renders the same 4 525
//! corpus files with a face supplied and `corpus/ratchet.json` without one,
//! and **52 % of all reported degradation was the absence of a face** — 1 045
//! files down to 506, and in qpdf's corpus 530 down to 130. A conforming file
//! that names Helvetica and embeds nothing is one this engine could not draw.
//!
//! A composite font in one of the registry's CJK collections that embeds no
//! program is the same gap in another script: its CIDs stand for characters a
//! face the build carries can draw by character, as a host's face draws them
//! (`docs/design/fallback-faces.md`, "The routes, in order").
//!
//! ## Why they are off by default
//!
//! Because [`crate`]'s callers are not all the same. A desktop application, a
//! server with a font package, a web page with a face already loaded — each
//! has better faces than these and a way to supply them, and none of them
//! should carry 4.2 MB of ours. `FontProvider` remains the seam whether this
//! feature is on or off; what the feature adds is an answer for the host that
//! has none.
//!
//! ## What is here and what is not
//!
//! Liberation Sans, Serif and Mono, four styles each. They are
//! metric-compatible with Arial, Times New Roman and Courier New, which are in
//! turn what every reader substitutes for Helvetica, Times and Courier — so
//! twelve of the standard 14 are covered by faces whose advance widths match
//! what the document's own `/Widths` array already says.
//!
//! Symbol and ZapfDingbats are **not** here and have no Liberation
//! equivalent. Substituting a text face for a symbolic font draws confidently
//! wrong glyphs, which is the failure `FontProvider::substitute` names as the
//! reason declining is a legitimate answer, so `family_for` declines instead.
//!
//! ## The CJK faces
//!
//! Source Han Sans Regular, one face a region, as ruling 23 answers FT-01.8:
//! JP for Adobe-Japan1, CN for Adobe-GB1, TW for Adobe-CNS1, and KR for
//! Adobe-Korea1 and Adobe-KR. A collection is drawn with its region's face,
//! because the same Han character is drawn one way for a Japanese reader and
//! another for a Chinese one, and [`Region::of`] says which region that is.
//! Each face is the only data of a crate of its own, `tinker-pdf-font-cjk-jp`
//! and its three siblings: together they pass crates.io's package ceiling.
//!
//! Each is stored as WOFF2, the smallest of the forms FT-01.4 measured, and
//! made ready once a process: [`cjk_face`] decodes a region's file through
//! [`crate::woff`] on the first request for it and keeps the program behind
//! a `OnceLock`, so every request after, from any thread, shares one copy.
//! The WOFF2 files were made by the reference encoder, and the tests beside
//! the facade hold every table each one decodes to against the published
//! face's (`crates/tinker-pdf/tests/cjk_faces.rs`).
//!
//! Nothing reads a font directory: that is an operating-system dependency and
//! `wasm32-unknown-unknown` has no filesystem at all.

#[cfg(feature = "cjk-fonts")]
use std::sync::{Arc, OnceLock};

/// Which of the three families a request wants.
#[cfg(feature = "bundled-fonts")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Liberation Sans — Helvetica and Arial.
    Sans,
    /// Liberation Serif — Times and Times New Roman.
    Serif,
    /// Liberation Mono — Courier and Courier New.
    Mono,
}

#[cfg(feature = "bundled-fonts")]
macro_rules! face_bytes {
    ($name:literal) => {
        include_bytes!(concat!("../data/liberation/", $name, ".ttf"))
    };
}

// Statics, not consts. A const is a value, and the bytes it points at are
// emitted again in each crate that compiles a use of it, so a compiler that
// inlines `face` into a caller's crate carries every face once more. A static
// has one address, so the faces are carried once whatever the compiler
// inlines.
#[cfg(feature = "bundled-fonts")]
static SANS: [&[u8]; 4] = [
    face_bytes!("LiberationSans-Regular"),
    face_bytes!("LiberationSans-Bold"),
    face_bytes!("LiberationSans-Italic"),
    face_bytes!("LiberationSans-BoldItalic"),
];
#[cfg(feature = "bundled-fonts")]
static SERIF: [&[u8]; 4] = [
    face_bytes!("LiberationSerif-Regular"),
    face_bytes!("LiberationSerif-Bold"),
    face_bytes!("LiberationSerif-Italic"),
    face_bytes!("LiberationSerif-BoldItalic"),
];
#[cfg(feature = "bundled-fonts")]
static MONO: [&[u8]; 4] = [
    face_bytes!("LiberationMono-Regular"),
    face_bytes!("LiberationMono-Bold"),
    face_bytes!("LiberationMono-Italic"),
    face_bytes!("LiberationMono-BoldItalic"),
];

/// The face for a family and style, as the TrueType bytes.
#[cfg(feature = "bundled-fonts")]
#[must_use]
pub fn face(family: Family, bold: bool, italic: bool) -> &'static [u8] {
    let style = usize::from(bold) | (usize::from(italic) << 1);
    let family = match family {
        Family::Sans => &SANS,
        Family::Serif => &SERIF,
        Family::Mono => &MONO,
    };
    // The index is two bits and the array is four long, so this cannot miss;
    // the fallback is the regular face rather than a panic all the same,
    // because ruling 1 does not make exceptions for arithmetic that is
    // obviously right.
    family.get(style).copied().unwrap_or(family[0])
}

/// Which family a font's name and flags ask for, or `None` to decline.
///
/// **The name is consulted before the flags**, and that ordering is the whole
/// of the interesting judgement here. `/Flags` bit 2 is the serif bit and
/// producers set it wrongly all the time — a great many emit zero for every
/// font they write — whereas a base-14 document names `Helvetica`, `Times-
/// Roman` or `Courier` exactly, and those three names are the case this
/// feature exists for. So a recognised name decides, and the flags decide only
/// what the name does not.
///
/// A symbolic font is declined outright. Its codes address a private glyph
/// order, so a text face drawn for one produces letters where the document
/// meant arrows — legible, plausible and wrong, which is worse than the
/// missing-glyph gap it would be replacing.
#[cfg(feature = "bundled-fonts")]
#[must_use]
pub fn family_for(
    base_font: &str,
    serif: bool,
    fixed_pitch: bool,
    symbolic: bool,
) -> Option<Family> {
    let lowered = base_font.to_ascii_lowercase();
    // **Declined by name as well as by flag**, for the reason the families are
    // chosen by name: a base-14 document writes no `/FontDescriptor` at all,
    // so `/Flags` is zero and the symbolic bit says nothing. Symbol and
    // ZapfDingbats are two of the standard 14 — precisely the case this
    // feature exists for — and a text face drawn for either puts letters where
    // the document meant arrows. Measured: without this line a `/BaseFont
    // /Symbol` page renders `abgd` as Latin letters, confidently.
    if symbolic
        || ["symbol", "dingbat", "wingding", "webding", "marlett"]
            .iter()
            .any(|name| lowered.contains(name))
    {
        return None;
    }
    // **The order is load-bearing**, because these are substrings and real
    // font names nest them: `sans-serif` contains `serif`, and `DejaVu Sans
    // Mono` contains `sans`. Narrowest claim first, so monospace beats sans
    // and sans beats serif, and each of those three sentences is one of the
    // tests below.
    for (needle, family) in [
        ("courier", Family::Mono),
        ("mono", Family::Mono),
        ("consol", Family::Mono),
        ("sans", Family::Sans),
        ("times", Family::Serif),
        ("serif", Family::Serif),
        ("georgia", Family::Serif),
        ("garamond", Family::Serif),
        ("book", Family::Serif),
        ("roman", Family::Serif),
        ("helvetica", Family::Sans),
        ("arial", Family::Sans),
        ("verdana", Family::Sans),
        ("tahoma", Family::Sans),
        ("calibri", Family::Sans),
    ] {
        if lowered.contains(needle) {
            return Some(family);
        }
    }
    if fixed_pitch {
        return Some(Family::Mono);
    }
    Some(if serif { Family::Serif } else { Family::Sans })
}

// ---- the CJK faces ------------------------------------------------------------

/// The region a CJK face is drawn for: one face a region, in one style, a
/// sans-serif Regular (`docs/design/fallback-faces.md`, "One face per region,
/// one style").
#[cfg(feature = "cjk-fonts")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    /// Japan: Source Han Sans JP, for Adobe-Japan1.
    Japan,
    /// Simplified Chinese: Source Han Sans CN, for Adobe-GB1.
    SimplifiedChinese,
    /// Traditional Chinese: Source Han Sans TW, for Adobe-CNS1.
    TraditionalChinese,
    /// Korea: Source Han Sans KR, for Adobe-Korea1 and Adobe-KR.
    Korea,
}

#[cfg(feature = "cjk-fonts")]
impl Region {
    /// Every region, in the design's order.
    pub const ALL: [Region; 4] = [
        Region::Japan,
        Region::SimplifiedChinese,
        Region::TraditionalChinese,
        Region::Korea,
    ];

    /// The region whose face draws a character collection's CIDs, by the
    /// collection's registry and ordering as a CIDFont's `/CIDSystemInfo`
    /// writes them: Adobe's Japan1, GB1, CNS1, Korea1 and KR. `None` for any
    /// other collection — `Identity`, whose CIDs stand for no character,
    /// among them — and for any other registry. The names are compared as
    /// written, as [`crate::cmap::collection_character`] compares them, so a
    /// collection that function reads no character of is drawn by no face.
    #[must_use]
    pub fn of(registry: &str, ordering: &str) -> Option<Region> {
        if registry != "Adobe" {
            return None;
        }
        match ordering {
            "Japan1" => Some(Region::Japan),
            "GB1" => Some(Region::SimplifiedChinese),
            "CNS1" => Some(Region::TraditionalChinese),
            "Korea1" | "KR" => Some(Region::Korea),
            _ => None,
        }
    }

    /// The orderings of Adobe's registry this region's face draws, each one
    /// [`Region::of`] answers with this region.
    #[must_use]
    pub fn orderings(self) -> &'static [&'static str] {
        match self {
            Region::Japan => &["Japan1"],
            Region::SimplifiedChinese => &["GB1"],
            Region::TraditionalChinese => &["CNS1"],
            Region::Korea => &["Korea1", "KR"],
        }
    }

    /// The published face's file, as its release names it, which is the name
    /// `THIRDPARTY.md` and the census's record (`tests/cjk_coverage.tsv`)
    /// give it.
    #[must_use]
    pub fn published_file(self) -> &'static str {
        match self {
            Region::Japan => "SourceHanSansJP-Regular.otf",
            Region::SimplifiedChinese => "SourceHanSansCN-Regular.otf",
            Region::TraditionalChinese => "SourceHanSansTW-Regular.otf",
            Region::Korea => "SourceHanSansKR-Regular.otf",
        }
    }

    /// The face as this build stores it: its WOFF2 file, the one static of
    /// the region's data crate.
    #[must_use]
    pub fn stored(self) -> &'static [u8] {
        match self {
            Region::Japan => tinker_pdf_font_cjk_jp::WOFF2,
            Region::SimplifiedChinese => tinker_pdf_font_cjk_cn::WOFF2,
            Region::TraditionalChinese => tinker_pdf_font_cjk_tw::WOFF2,
            Region::Korea => tinker_pdf_font_cjk_kr::WOFF2,
        }
    }

    /// Its place in [`Region::ALL`].
    fn index(self) -> usize {
        match self {
            Region::Japan => 0,
            Region::SimplifiedChinese => 1,
            Region::TraditionalChinese => 2,
            Region::Korea => 3,
        }
    }
}

/// The most bytes a stored face is decoded to: a bound above every face this
/// build carries, which `the_cjk_face_is_compiled_in_and_reads` holds, and
/// the bound [`crate::woff::decode`] needs, because a WOFF2 states its lengths
/// itself.
#[cfg(feature = "cjk-fonts")]
const MAX_FACE: usize = 16 << 20;

/// A region's face, its program made ready: the region's WOFF2 file decoded
/// on the first request for it in the process, kept behind a `OnceLock` and
/// shared by every request after, from any thread, so a document of many
/// pages, or many documents, decode it once.
///
/// `None` where the file would not become a face, which the build's own test
/// rules out for the files it carries: a caller has no face to draw with,
/// and the renderer reports the font it could not draw.
#[cfg(feature = "cjk-fonts")]
#[must_use]
pub fn cjk_face(region: Region) -> Option<Arc<Vec<u8>>> {
    static READY: [OnceLock<Option<Arc<Vec<u8>>>>; 4] = [const { OnceLock::new() }; 4];
    READY
        .get(region.index())?
        .get_or_init(|| {
            #[cfg(test)]
            count_made_ready(region);
            made_ready(region.stored())
        })
        .clone()
}

/// `stored`, a WOFF2 file, made into the program it packs: unpacked through
/// [`crate::woff::decode`] within [`MAX_FACE`] and read back as an sfnt.
/// `None` for a file that is no WOFF, that will not unpack, that unpacks past
/// the bound, or that unpacks to something no sfnt reader reads.
#[cfg(feature = "cjk-fonts")]
fn made_ready(stored: &[u8]) -> Option<Arc<Vec<u8>>> {
    let program = crate::woff::decode(stored, MAX_FACE).ok()?;
    crate::Sfnt::parse(&program)?;
    Some(Arc::new(program))
}

/// How many times each region's face has been made ready in this test
/// process, by [`Region::ALL`]'s order.
#[cfg(all(test, feature = "cjk-fonts"))]
static MADE_READY: [std::sync::atomic::AtomicUsize; 4] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; 4];

/// Counts one making-ready of `region`'s face.
#[cfg(all(test, feature = "cjk-fonts"))]
fn count_made_ready(region: Region) {
    if let Some(count) = MADE_READY.get(region.index()) {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(all(test, feature = "cjk-fonts"))]
mod cjk_tests {
    use super::*;
    use crate::woff::{packaging, Packaging};
    use crate::{Cff, Sfnt};

    /// `OTTO`: an sfnt whose outlines are CFF.
    const OTTO: u32 = u32::from_be_bytes(*b"OTTO");
    /// `CFF `.
    const CFF: u32 = u32::from_be_bytes(*b"CFF ");
    /// `glyf`.
    const GLYF: u32 = u32::from_be_bytes(*b"glyf");
    /// `cmap`.
    const CMAP: u32 = u32::from_be_bytes(*b"cmap");
    /// `maxp`.
    const MAXP: u32 = u32::from_be_bytes(*b"maxp");

    /// The census FT-01.4 committed (`tests/cjk_coverage.tsv`): the CIDs of
    /// each collection each candidate face reaches, by route and writing mode.
    #[cfg(feature = "cmap-predefined")]
    const CENSUS: &str = include_str!("../tests/cjk_coverage.tsv");

    /// The census's `all` row for the published `file`'s face 0 over
    /// `Adobe-<ordering>`, in one writing mode: the CIDs its charset, its
    /// characters and the index reading reached.
    #[cfg(feature = "cmap-predefined")]
    fn census_reach(file: &str, ordering: &str, vertical: bool) -> (usize, usize, usize) {
        let collection = format!("Adobe-{ordering}");
        let row: Vec<&str> = CENSUS
            .lines()
            .map(|line| line.split('\t').collect::<Vec<&str>>())
            .find(|cells| {
                cells.first() == Some(&"coverage")
                    && cells.get(1) == Some(&file)
                    && cells.get(2) == Some(&"0")
                    && cells.get(4) == Some(&collection.as_str())
                    && cells.get(5) == Some(&"all")
            })
            .unwrap_or_else(|| panic!("the census records {file} over {collection}"));
        let at = if vertical { 11 } else { 7 };
        let count = |offset: usize| -> usize {
            row.get(at + offset)
                .and_then(|cell| cell.parse().ok())
                .unwrap_or_else(|| panic!("{file} over {collection}: a count at {}", at + offset))
        };
        (count(0), count(1), count(2))
    }

    /// The CIDs of `Adobe-<ordering>`, as the census counts them: those each
    /// supplement's `Adobe-<ordering>-<n>` CMap reaches, CID 0 — `.notdef`,
    /// which no route is asked for — left out.
    #[cfg(feature = "cmap-predefined")]
    fn collection_cids(ordering: &str) -> Vec<u32> {
        let mut seen = std::collections::BTreeSet::from([0u32]);
        let mut out = Vec::new();
        for supplement in 0..64u32 {
            let name = format!("Adobe-{ordering}-{supplement}");
            let Some(cmap) = crate::CMap::predefined(name.as_bytes()) else {
                break;
            };
            if cmap.is_approximate() {
                break;
            }
            for code in 0..=0xFFFF_u32 {
                if let Some(cid) = cmap.cid(code) {
                    if seen.insert(cid) {
                        out.push(cid);
                    }
                }
            }
        }
        out
    }

    /// How many of `Adobe-<ordering>`'s CIDs `face` draws by character in a
    /// writing mode: those the collection's inverted `Uni*-UTF32` CMaps give
    /// a character of, either of the two kept, that the face's `cmap` maps to
    /// a glyph other than `.notdef` — the second route, as the census and the
    /// facade read it.
    #[cfg(feature = "cmap-predefined")]
    fn reached_by_character(face: &Sfnt<'_>, ordering: &str, vertical: bool) -> usize {
        collection_cids(ordering)
            .into_iter()
            .filter(|cid| {
                crate::cmap::collection_character("Adobe", ordering, vertical, *cid).is_some_and(
                    |found| {
                        found
                            .characters
                            .into_iter()
                            .flatten()
                            .any(|c| face.glyph_for_char(c).is_some_and(|glyph| glyph != 0))
                    },
                )
            })
            .count()
    }

    /// **The faces are compiled in, and each reads** (FT-01.8): each region's
    /// stored bytes are a WOFF2 file no other region's repeats; made ready,
    /// each is an sfnt with CFF outlines, its CFF's glyph count `maxp`'s, a
    /// `cmap`, and an outline for `中`, which every region writes; and each
    /// covers the collections FT-01.4 counted — in each writing mode its
    /// characters reach exactly the CIDs of its region's collections the
    /// census counted for the published face, by character alone, since the
    /// census found the face keyed by none of them. So the WOFF2 file holds
    /// the `cmap` the published face does, and draws what it drew.
    #[test]
    fn the_cjk_face_is_compiled_in_and_reads() {
        let mut seen: Vec<&[u8]> = Vec::new();
        for region in Region::ALL {
            let stored = region.stored();
            assert_eq!(
                packaging(stored),
                Some(Packaging::Woff2),
                "{region:?} is stored as WOFF2"
            );
            assert!(
                !seen.iter().any(|other| other.as_ptr() == stored.as_ptr()),
                "{region:?} repeats another region's file"
            );
            seen.push(stored);

            let program = cjk_face(region).unwrap_or_else(|| panic!("{region:?} is made ready"));
            assert!(program.len() <= MAX_FACE, "{region:?} is within the bound");
            let face = Sfnt::parse(&program).unwrap_or_else(|| panic!("{region:?} is an sfnt"));
            assert_eq!(face.version(), OTTO, "{region:?}'s outlines are CFF");
            assert!(face.table(GLYF).is_none(), "{region:?} has no glyf");
            assert!(face.table(CMAP).is_some(), "{region:?} has a cmap");
            let cff = face
                .table(CFF)
                .and_then(Cff::parse)
                .unwrap_or_else(|| panic!("{region:?}'s CFF parses"));
            let glyphs = face
                .table(MAXP)
                .and_then(|maxp| maxp.get(4..6))
                .map(|count| usize::from(u16::from_be_bytes([count[0], count[1]])));
            assert_eq!(
                Some(cff.glyph_count()),
                glyphs,
                "{region:?}: the CFF holds the glyphs maxp counts"
            );
            let middle = face
                .glyph_for_char('\u{4e2d}')
                .filter(|glyph| *glyph != 0)
                .unwrap_or_else(|| panic!("{region:?} draws 中"));
            assert!(
                cff.outline(middle)
                    .is_some_and(|outline| !outline.is_empty()),
                "{region:?}: 中 has an outline"
            );

            #[cfg(feature = "cmap-predefined")]
            for ordering in region.orderings() {
                for vertical in [false, true] {
                    let (charset, character, index) =
                        census_reach(region.published_file(), ordering, vertical);
                    assert_eq!(
                        (charset, index),
                        (0, 0),
                        "{region:?} over Adobe-{ordering}: the census reached nothing but by \
                         character"
                    );
                    assert_eq!(
                        reached_by_character(&face, ordering, vertical),
                        character,
                        "{region:?} over Adobe-{ordering}, vertical {vertical}: the CIDs the \
                         census counted for the published face"
                    );
                }
            }
        }
        assert_eq!(seen.len(), 4, "a face a region");
    }

    /// **Each face is made ready once** (FT-01.8): many requests for each
    /// region, from several threads at once, make each face ready once in
    /// the process, and every one is handed the same program.
    #[test]
    fn the_cjk_face_is_made_ready_once() {
        let handed: Vec<Vec<Arc<Vec<u8>>>> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let mut handed = Vec::new();
                        for _ in 0..16 {
                            for region in Region::ALL {
                                handed.push(cjk_face(region).expect("the face is made ready"));
                            }
                        }
                        handed
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|thread| thread.join().expect("a thread asks"))
                .collect()
        });
        for (at, region) in Region::ALL.into_iter().enumerate() {
            let first = cjk_face(region).expect("the face is made ready");
            for answers in &handed {
                for answer in answers.iter().skip(at).step_by(Region::ALL.len()) {
                    assert!(Arc::ptr_eq(answer, &first), "{region:?}: one program");
                }
            }
            let made = MADE_READY
                .get(region.index())
                .map(|count| count.load(std::sync::atomic::Ordering::Relaxed));
            assert_eq!(made, Some(1), "{region:?} is made ready once a process");
        }
    }

    /// Each of the registry's CJK collections is drawn by its region's face,
    /// and each region draws the collections it names.
    #[test]
    fn each_collection_is_drawn_by_its_regions_face() {
        for (ordering, region) in [
            ("Japan1", Region::Japan),
            ("GB1", Region::SimplifiedChinese),
            ("CNS1", Region::TraditionalChinese),
            ("Korea1", Region::Korea),
            ("KR", Region::Korea),
        ] {
            assert_eq!(Region::of("Adobe", ordering), Some(region), "{ordering}");
            assert!(
                region.orderings().contains(&ordering),
                "{region:?}: {ordering}"
            );
        }
        for region in Region::ALL {
            for ordering in region.orderings() {
                assert_eq!(Region::of("Adobe", ordering), Some(region));
            }
            assert!(
                region.published_file().starts_with("SourceHanSans")
                    && region.published_file().ends_with("-Regular.otf"),
                "{region:?}"
            );
        }
    }

    /// A collection of no region is drawn by none: `Identity`, whose CIDs
    /// stand for no character, the registry's Manga1, which is no collection
    /// of FT-01.1's, an ordering of another registry, and a name spelled
    /// otherwise than the registry writes it.
    #[test]
    fn a_collection_of_no_region_is_drawn_by_no_face() {
        for (registry, ordering) in [
            ("Adobe", "Identity"),
            ("Adobe", "Manga1"),
            ("Adobe", ""),
            ("", "Japan1"),
            ("Acme", "Japan1"),
            ("adobe", "Japan1"),
            ("Adobe", "japan1"),
            ("Adobe", "Japan1 "),
            ("Adobe", "Japan2"),
            ("Adobe", "Korea"),
        ] {
            assert_eq!(
                Region::of(registry, ordering),
                None,
                "{registry} {ordering}"
            );
        }
    }

    /// `n` as WOFF2's `UIntBase128` (§3.1): seven bits a byte, the high bit
    /// set on every byte but the last.
    fn base128(mut n: u32) -> Vec<u8> {
        let mut out = vec![(n & 0x7f) as u8];
        n >>= 7;
        while n > 0 {
            out.insert(0, 0x80 | (n & 0x7f) as u8);
            n >>= 7;
        }
        out
    }

    /// **A file that is no face is not made ready**: nothing; bytes that are
    /// no WOFF; the published-face form a WOFF is not, an sfnt; a carried
    /// file cut short; and a WOFF2 whose one table claims more than the
    /// bound, refused before anything is unpacked.
    #[test]
    fn a_file_that_is_no_face_is_not_made_ready() {
        let japan = Region::Japan.stored();
        let mut oversized = Vec::new();
        oversized.extend_from_slice(b"wOF2");
        oversized.extend_from_slice(&OTTO.to_be_bytes());
        oversized.extend_from_slice(&64u32.to_be_bytes()); // length
        oversized.extend_from_slice(&1u16.to_be_bytes()); // numTables
        oversized.extend_from_slice(&0u16.to_be_bytes()); // reserved
        oversized.extend_from_slice(&64u32.to_be_bytes()); // totalSfntSize
        oversized.extend_from_slice(&1u32.to_be_bytes()); // totalCompressedSize
        oversized.resize(48, 0);
        oversized.push(1); // `head`, the null transform
        oversized.extend_from_slice(&base128(u32::try_from(MAX_FACE + 1).expect("a u32")));
        oversized.push(0);
        let program = made_ready(japan).expect("the carried file is a face");
        for (what, bytes) in [
            ("nothing", &[][..]),
            ("bytes that are no WOFF", &b"not a font at all"[..]),
            ("an sfnt, which is no WOFF", &program[..]),
            ("the carried file cut short", &japan[..japan.len() / 2]),
            ("a table past the bound", &oversized[..]),
        ] {
            assert!(made_ready(bytes).is_none(), "{what}");
        }
    }
}

#[cfg(all(test, feature = "bundled-fonts"))]
mod tests {
    use super::*;

    /// Every face is a TrueType font, and no two of them are the same one.
    ///
    /// Both halves matter. The first is that `include_bytes!` reached a font
    /// rather than a README; the second is that twelve `include_bytes!` paths
    /// naming twelve files did not, through a copy-paste, name one of them
    /// four times — which compiles, passes any test that only checks the
    /// bytes are a font, and silently draws every weight as regular.
    #[test]
    fn twelve_distinct_truetype_faces_are_compiled_in() {
        let mut seen: Vec<&[u8]> = Vec::new();
        for family in [Family::Sans, Family::Serif, Family::Mono] {
            for bold in [false, true] {
                for italic in [false, true] {
                    let bytes = face(family, bold, italic);
                    assert_eq!(
                        &bytes[0..4],
                        &0x0001_0000u32.to_be_bytes(),
                        "{family:?} bold={bold} italic={italic} is not an sfnt"
                    );
                    assert!(bytes.len() > 100_000, "and it is a whole face");
                    assert!(
                        !seen.iter().any(|other| other.as_ptr() == bytes.as_ptr()),
                        "{family:?} bold={bold} italic={italic} repeats another face"
                    );
                    seen.push(bytes);
                }
            }
        }
        assert_eq!(seen.len(), 12);
    }

    /// The standard 14's own names pick the right family, whatever the flags
    /// say — which is the case this feature exists for.
    #[test]
    fn the_standard_names_decide_before_the_flags_do() {
        // Every one of these is asked with the *wrong* flags: serif claimed
        // for the sans faces and cleared for the serif ones, fixed pitch never
        // set. A reading that trusted the flags would get all six wrong.
        for (name, want) in [
            ("Helvetica", Family::Sans),
            ("Helvetica-BoldOblique", Family::Sans),
            ("Arial,Bold", Family::Sans),
            ("Times-Roman", Family::Serif),
            ("TimesNewRomanPSMT", Family::Serif),
            ("Courier", Family::Mono),
            ("Courier-BoldOblique", Family::Mono),
            ("CourierNewPS-BoldMT", Family::Mono),
        ] {
            let serif = want != Family::Serif;
            assert_eq!(
                family_for(name, serif, false, false),
                Some(want),
                "{name} with serif={serif}"
            );
        }
    }

    /// The substring search survives its own counter-examples.
    ///
    /// `sans-serif` contains `serif` and `DejaVu Sans Mono` contains `sans`,
    /// so the three families nest inside each other's names. Each row here is
    /// asked with flags that point the *other* way, so a reading that fell
    /// through to them would get it wrong.
    #[test]
    fn a_nested_family_name_resolves_to_the_narrowest_claim() {
        for name in ["sans-serif", "DejaVu Sans", "Open Sans", "PT Sans-Serif"] {
            assert_eq!(
                family_for(name, true, false, false),
                Some(Family::Sans),
                "{name} is sans, not serif"
            );
        }
        for name in ["DejaVu Sans Mono", "Liberation Mono", "Courier Sans"] {
            assert_eq!(
                family_for(name, true, false, false),
                Some(Family::Mono),
                "{name} is monospace, not sans"
            );
        }
    }

    /// A name nothing recognises falls to the flags, which is what they are
    /// for.
    #[test]
    fn an_unknown_name_falls_to_the_flags() {
        assert_eq!(
            family_for("Whatever-Regular", true, false, false),
            Some(Family::Serif)
        );
        assert_eq!(
            family_for("Whatever-Regular", false, false, false),
            Some(Family::Sans)
        );
        assert_eq!(
            family_for("Whatever-Regular", true, true, false),
            Some(Family::Mono)
        );
    }

    /// A symbolic font is declined rather than substituted.
    ///
    /// Symbol and ZapfDingbats are two of the standard 14 and neither has a
    /// Liberation equivalent. Drawing Liberation Sans for one puts letters
    /// where the document meant arrows — legible, plausible and wrong, which
    /// is a worse outcome than the gap it replaces.
    #[test]
    fn a_symbolic_font_is_declined() {
        for name in ["Symbol", "ZapfDingbats", "Wingdings", "Helvetica"] {
            assert_eq!(family_for(name, false, false, true), None, "{name}");
        }
    }

    /// And declined **by name**, with the flag clear.
    ///
    /// This is not belt and braces. A base-14 font dictionary carries no
    /// `/FontDescriptor`, so `/Flags` is zero and the symbolic bit is absent
    /// for exactly the two faces that need it most — which is how a
    /// `/BaseFont /Symbol` page came to render its text as Latin letters
    /// before this existed.
    #[test]
    fn the_two_symbolic_standard_faces_are_declined_by_name() {
        for name in ["Symbol", "ZapfDingbats", "Wingdings-Regular", "Webdings"] {
            assert_eq!(family_for(name, false, false, false), None, "{name}");
        }
        // And a text face with a similar-looking name is not caught by it.
        assert_eq!(
            family_for("SymbolaText", false, false, false),
            None,
            "a name containing `symbol` is declined, which is the safe side"
        );
        assert_eq!(
            family_for("Helvetica", false, false, false),
            Some(Family::Sans)
        );
    }
}
