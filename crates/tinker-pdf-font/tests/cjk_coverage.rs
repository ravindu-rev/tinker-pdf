//! **The census H-116 and H-04 are ruled with** (`docs/design/fallback-faces.md`,
//! FT-01.4): for each candidate face the design names, the CIDs of each of the
//! registry's five CJK collections, supplement by supplement, that a
//! substituted face's routes reach in it, in either writing mode and split by
//! the route that reached each; its bytes as published, deflated and as WOFF2;
//! the time each of those forms takes to be made ready on first use; and the
//! time each collection takes to be inverted.
//!
//! The census rules nothing: the face is H-116's and the footprint H-04's, each
//! decided with these figures in hand. It counts by the routes the facade's
//! `substituted_glyph` (`crates/tinker-pdf/src/resources.rs`) takes for a
//! composite font, in its order and through the functions it calls — the
//! face's own charset by `Cff::registry_ordering` and `Cff::gid_for_cid`, the
//! collection's character by `cmap::collection_character` and
//! `Sfnt::glyph_for_char` — so its figures are the drawing's. `/ToUnicode` is
//! a document's statement and not a face's, so the third route is not counted;
//! a CID no other route reaches is counted as unreached.
//!
//! # Fetched, never committed
//!
//! The candidates are fetched once into a directory of the operator's, from
//! the upstreams `fallback-faces.md` names, and `TINKER_CJK_FACES` names it;
//! nothing of them reaches the tree (ruling 18). A WOFF2 form of a candidate,
//! made once by a compressor outside the engine, is read from the same
//! directory under the candidate's stem with `.woff2`, and is absent where the
//! directory holds none. Without the variable, or with one that names no
//! directory, the census prints [`SKIPPED`] and does nothing; with it,
//! [`RAN`] and its three tables, a candidate the directory lacks marked absent.
//!
//! ```text
//! TINKER_CJK_FACES=<directory> cargo test -p tinker-pdf-font --release --test cjk_coverage -- --ignored --nocapture
//! TINKER_CJK_FACES=<directory> cargo test -p tinker-pdf-font --test cjk_coverage -- --ignored --nocapture
//! ```
//!
//! The first run is the release build's and the second the debug build's: the
//! counts are the same in both, and the times are each build's own. The census
//! is this file's one ignored test, so `--ignored` runs it alone, and a
//! collection is inverted by its first use in the process, which the census
//! times before any face asks for it.
//!
//! Its counts and sizes are committed as `cjk_coverage.tsv` beside this file:
//! every row it prints, less the columns that hold a time and the profile a
//! time was taken in ([`TIMED`]), since a count or a size is the same in
//! either build and a time is the named machine's (ruling 15). The run's
//! revision, machine and day go in the message of the commit that records it
//! (ruling 16).
//!
//! The tests below the census hold its instruments — the supplements read from
//! the registry, the routes, a collection's faces, the storage forms, the
//! `name` table reading and the candidate list — to what they state, over
//! inputs built here or committed, and the committed record whole and adding
//! up, and run in every `cargo test`.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use tinker_pdf_filters::{flate_decode, zlib_compress_at, DeflateLevel, Limits};
use tinker_pdf_font::cmap::collection_character;
use tinker_pdf_font::encoding::{base_char, BaseEncoding};
use tinker_pdf_font::{woff, CMap, Cff, Sfnt};

/// Printed when the census ran: a skipped census exits 0 and reads like a
/// pass, so a run without this line is no measurement.
const RAN: &str = "cjk-coverage: RAN";

/// Printed when there was nothing to measure.
const SKIPPED: &str = "cjk-coverage: SKIPPED";

/// The head of the census's `inversion` table, as it prints it.
const INVERSION_HEAD: &str = "inversion\tcollection\twriting_mode\tcids_reached\tmicros\tprofile";

/// The head of the census's `coverage` table, as it prints it.
const COVERAGE_HEAD: &str = "coverage\tfile\tface\tname\tcollection\tsupplement\tcids\t\
     horizontal_by_charset\thorizontal_by_character\thorizontal_by_index\thorizontal_unreached\t\
     vertical_by_charset\tvertical_by_character\tvertical_by_index\tvertical_unreached";

/// The head of the census's `storage` table, as it prints it.
const STORAGE_HEAD: &str = "storage\tfile\tbytes\tdeflated_bytes\twoff2_bytes\tready_raw_micros\t\
     ready_deflated_micros\tready_woff2_micros\tprofile\tupstream\tspdx";

/// The columns of the census's tables that hold a time, or the build profile
/// a time was taken in, which the committed record leaves out.
const TIMED: [&str; 5] = [
    "micros",
    "ready_raw_micros",
    "ready_deflated_micros",
    "ready_woff2_micros",
    "profile",
];

/// One candidate file, as `fallback-faces.md`'s candidate table names it.
struct Candidate {
    /// The file the census reads from `TINKER_CJK_FACES`.
    file: &'static str,
    /// The face, or the collection of faces, it holds.
    face: &'static str,
    /// Where it is fetched from.
    upstream: &'static str,
    /// Its licence's SPDX expression.
    spdx: &'static str,
}

const SOURCE_HAN_SUBSET: &str =
    "https://github.com/adobe-fonts/source-han-sans release 2.005R SubsetOTF/";
const SOURCE_HAN_OTC: &str = "https://github.com/adobe-fonts/source-han-sans release 2.005R OTC/";
const NOTO_SUBSET: &str = "https://github.com/notofonts/noto-cjk release Sans2.004 Sans/SubsetOTF/";
const NOTO_OTC: &str = "https://github.com/notofonts/noto-cjk release Sans2.004 Sans/OTC/";
const DROID: &str = "https://android.googlesource.com/platform/frameworks/base data/fonts/";

/// The candidates, in the design's order.
const CANDIDATES: [Candidate; 16] = [
    Candidate {
        file: "SourceHanSansJP-Regular.otf",
        face: "Source Han Sans JP Regular",
        upstream: SOURCE_HAN_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "SourceHanSansKR-Regular.otf",
        face: "Source Han Sans KR Regular",
        upstream: SOURCE_HAN_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "SourceHanSansCN-Regular.otf",
        face: "Source Han Sans CN Regular",
        upstream: SOURCE_HAN_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "SourceHanSansTW-Regular.otf",
        face: "Source Han Sans TW Regular",
        upstream: SOURCE_HAN_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "SourceHanSansHK-Regular.otf",
        face: "Source Han Sans HK Regular",
        upstream: SOURCE_HAN_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "SourceHanSans-Regular.ttc",
        face: "Source Han Sans Regular, the Pan-CJK collection",
        upstream: SOURCE_HAN_OTC,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansJP-Regular.otf",
        face: "Noto Sans JP Regular",
        upstream: NOTO_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansKR-Regular.otf",
        face: "Noto Sans KR Regular",
        upstream: NOTO_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansSC-Regular.otf",
        face: "Noto Sans SC Regular",
        upstream: NOTO_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansTC-Regular.otf",
        face: "Noto Sans TC Regular",
        upstream: NOTO_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansHK-Regular.otf",
        face: "Noto Sans HK Regular",
        upstream: NOTO_SUBSET,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansCJK-Regular.ttc",
        face: "Noto Sans CJK Regular, the collection",
        upstream: NOTO_OTC,
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "BIZUDGothic-Regular.ttf",
        face: "BIZ UDGothic Regular",
        upstream: "https://github.com/googlefonts/morisawa-biz-ud-gothic release v1.051 fonts/ttf/",
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NanumGothic-Regular.ttf",
        face: "Nanum Gothic Regular",
        upstream: "https://github.com/google/fonts ofl/nanumgothic/",
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "DroidSansFallback.ttf",
        face: "Droid Sans Fallback",
        upstream: DROID,
        spdx: "Apache-2.0",
    },
    Candidate {
        file: "DroidSansFallbackFull.ttf",
        face: "Droid Sans Fallback Full",
        upstream: DROID,
        spdx: "Apache-2.0",
    },
];

/// The registry's five CJK collections, by `/Ordering`, in the design's
/// order: each is `Adobe`'s.
const ORDERINGS: [&str; 5] = ["Japan1", "GB1", "CNS1", "Korea1", "KR"];

/// The most supplements a collection is read to: the vendored registry's
/// longest run is Adobe-KR's ten, and a re-vendoring that went past this
/// would be cut here rather than read without end.
const MAX_SUPPLEMENTS: u32 = 64;

/// The most faces a collection file is read to. The candidate collections
/// hold ten each; a `numFonts` past this is a damaged header.
const MAX_FACES: usize = 64;

/// The largest program a stored form is unpacked to. The candidates are tens
/// of megabytes at most; a form that asks for more is reported unreadable.
const MAX_PROGRAM: usize = 256 << 20;

/// How many times each storage form is made ready; the least is the figure,
/// so that an interruption by the scheduler is not.
const READY_RUNS: usize = 5;

const TAG_CFF: u32 = u32::from_be_bytes(*b"CFF ");
const TAG_CMAP: u32 = u32::from_be_bytes(*b"cmap");
const TAG_GLYF: u32 = u32::from_be_bytes(*b"glyf");
const TAG_MAXP: u32 = u32::from_be_bytes(*b"maxp");
const TAG_NAME: u32 = u32::from_be_bytes(*b"name");

/// The directory `TINKER_CJK_FACES` names, when it names one.
fn faces_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TINKER_CJK_FACES")?);
    dir.is_dir().then_some(dir)
}

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

// --- the supplements --------------------------------------------------------

/// The CIDs each supplement of `ordering` adds, in supplement order, read
/// from the registry's own `Adobe-<ordering>-<n>` CMaps: supplement `n`'s are
/// the CIDs its CMap reaches that no earlier supplement's does, ascending, and
/// CID 0 — `.notdef` by definition, which no route is asked for — is left
/// out. Empty in a build without `cmap-predefined`, which holds no CID tables
/// to read.
fn supplements(ordering: &str) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::from([0u32]);
    for supplement in 0..MAX_SUPPLEMENTS {
        let name = format!("Adobe-{ordering}-{supplement}");
        let Some(cmap) = CMap::predefined(name.as_bytes()) else {
            break;
        };
        if cmap.is_approximate() {
            break;
        }
        let mut added: Vec<u32> = (0..=0xFFFF_u32)
            .filter_map(|code| cmap.cid(code))
            .filter(|cid| seen.insert(*cid))
            .collect();
        added.sort_unstable();
        out.push(added);
    }
    out
}

// --- the routes -------------------------------------------------------------

/// Which route reached a CID's glyph, in the facade's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// The face's own charset: a CID-keyed CFF whose `ROS` is the font's
    /// collection.
    Charset,
    /// The collection's character, through the face's `cmap`.
    Character,
    /// The CID as a glyph index: a face with neither a `cmap` nor a `ROS`,
    /// which has no characters to be read by, keeps the reading an embedded
    /// program gets, counted here as the CIDs below its glyph count.
    Index,
}

/// What a face offers the routes, as the facade reads a substituted program:
/// an sfnt's table directory where it is one, its CFF outlines where they are
/// CFF — an OpenType face's `CFF ` table, or a bare CFF program — and its
/// glyph count.
struct Face<'a> {
    sfnt: Option<Sfnt<'a>>,
    cff: Option<Cff<'a>>,
    glyphs: usize,
}

impl<'a> Face<'a> {
    /// `program` read as the facade reads a substituted one: an sfnt, its
    /// outlines CFF where it has no `glyf`; else a bare CFF program; else
    /// nothing.
    fn read(program: &'a [u8]) -> Option<Face<'a>> {
        if let Some(sfnt) = Sfnt::parse(program) {
            let cff = if sfnt.table(TAG_GLYF).is_none() {
                sfnt.table(TAG_CFF).and_then(Cff::parse)
            } else {
                None
            };
            let glyphs = match &cff {
                Some(cff) => cff.glyph_count(),
                None => sfnt
                    .table(TAG_MAXP)
                    .and_then(|maxp| be16(maxp, 4))
                    .map_or(0, usize::from),
            };
            return Some(Face {
                sfnt: Some(sfnt),
                cff,
                glyphs,
            });
        }
        let cff = Cff::parse(program)?;
        let glyphs = cff.glyph_count();
        Some(Face {
            sfnt: None,
            cff: Some(cff),
            glyphs,
        })
    }

    /// The route by which CID `cid` of Adobe's collection `ordering`, in a
    /// font that writes vertically or not, reaches a glyph of this face other
    /// than glyph 0: the face's own charset, where it is CID-keyed in that
    /// collection; else the collection's character — the better of the two
    /// that reach the CID first — through its `cmap`; and `None` where neither
    /// reaches one. A face with neither a `cmap` nor a `ROS` keeps the index
    /// reading instead.
    fn route(&self, ordering: &str, vertical: bool, cid: u32) -> Option<Route> {
        let mapped = self
            .sfnt
            .as_ref()
            .filter(|sfnt| sfnt.table(TAG_CMAP).is_some());
        let keyed = self.cff.as_ref().filter(|cff| cff.is_cid());
        if mapped.is_none() && keyed.is_none() {
            let below = usize::try_from(cid).is_ok_and(|cid| cid < self.glyphs);
            return below.then_some(Route::Index);
        }
        if let Some(cff) = keyed {
            let same = cff.registry_ordering() == Some(("Adobe", ordering));
            if same && cff.gid_for_cid(cid).is_some_and(|glyph| glyph != 0) {
                return Some(Route::Charset);
            }
        }
        let sfnt = mapped?;
        let found = collection_character("Adobe", ordering, vertical, cid)?;
        found
            .characters
            .into_iter()
            .flatten()
            .any(|c| sfnt.glyph_for_char(c).is_some_and(|glyph| glyph != 0))
            .then_some(Route::Character)
    }
}

/// How many CIDs each route reached, and how many none did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    charset: usize,
    character: usize,
    index: usize,
    unreached: usize,
}

impl Tally {
    fn add(&mut self, route: Option<Route>) {
        match route {
            Some(Route::Charset) => self.charset += 1,
            Some(Route::Character) => self.character += 1,
            Some(Route::Index) => self.index += 1,
            None => self.unreached += 1,
        }
    }

    fn plus(self, other: Tally) -> Tally {
        Tally {
            charset: self.charset + other.charset,
            character: self.character + other.character,
            index: self.index + other.index,
            unreached: self.unreached + other.unreached,
        }
    }

    /// The four counts as TSV cells.
    fn cells(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.charset, self.character, self.index, self.unreached
        )
    }
}

/// Each supplement's tally over `face`, horizontal then vertical.
fn coverage(face: &Face<'_>, ordering: &str, supplements: &[Vec<u32>]) -> Vec<[Tally; 2]> {
    supplements
        .iter()
        .map(|cids| {
            let mut modes = [Tally::default(); 2];
            for cid in cids {
                modes[0].add(face.route(ordering, false, *cid));
                modes[1].add(face.route(ordering, true, *cid));
            }
            modes
        })
        .collect()
}

// --- a collection's faces ---------------------------------------------------

/// How many faces `program` holds: a collection's (`ttcf`) `numFonts`, and one
/// for anything else.
fn face_count(program: &[u8]) -> usize {
    if program.get(..4) != Some(b"ttcf") {
        return 1;
    }
    be32(program, 8)
        .and_then(|count| usize::try_from(count).ok())
        .map_or(0, |count| count.min(MAX_FACES))
}

/// Face `index` of `program` as `Sfnt::parse` reads a face: the program
/// itself where it is not a collection, and for a collection a copy whose
/// first table-directory offset is face `index`'s, since `Sfnt::parse` reads
/// a collection's first face and its tables' offsets are from the file's
/// start. `None` past the faces the header lists.
fn face_at(program: &[u8], index: usize) -> Option<Cow<'_, [u8]>> {
    if program.get(..4) != Some(b"ttcf") {
        return (index == 0).then_some(Cow::Borrowed(program));
    }
    if index >= face_count(program) {
        return None;
    }
    let at = 12 + index * 4;
    let offset = program.get(at..at + 4)?;
    let mut copy = program.to_vec();
    copy.get_mut(12..16)?.copy_from_slice(offset);
    Some(Cow::Owned(copy))
}

// --- the storage forms ------------------------------------------------------

/// A candidate's bytes in each storage form `fallback-faces.md` weighs, and
/// the least time of [`READY_RUNS`] each takes to be made ready, in
/// microseconds: `Err` naming why a form would not be.
#[derive(Debug)]
struct Stored {
    bytes: usize,
    deflated: usize,
    /// The WOFF2 form's bytes, where the directory holds one.
    woff2: Option<usize>,
    ready_raw: Result<u128, String>,
    ready_deflated: Result<u128, String>,
    /// Where the directory holds no WOFF2 form, `None`.
    ready_woff2: Option<Result<u128, String>>,
}

/// The glyph count `program`'s first face states: its CFF's where its
/// outlines are CFF, else `maxp`'s.
fn glyph_total(program: &[u8]) -> Option<usize> {
    Face::read(program).map(|face| face.glyphs)
}

/// How long `make` takes, the least of [`READY_RUNS`] in microseconds, to
/// make a stored form ready: to turn it into the program — nothing at all for
/// the form as published, which is read where it lies — parse the program
/// and look its first character up, which chooses the `cmap` subtable every
/// later lookup reads: the work before a face's first glyph is reached. The
/// program it makes must hold `glyphs` glyphs, as the published one does.
fn ready<'p>(
    glyphs: usize,
    make: impl Fn() -> Result<Cow<'p, [u8]>, String>,
) -> Result<u128, String> {
    let mut least: Option<u128> = None;
    for _ in 0..READY_RUNS {
        let clock = Instant::now();
        let program = make()?;
        let face = Sfnt::parse(&program).ok_or("the program it makes is no sfnt")?;
        std::hint::black_box(face.glyph_for_char('\u{4E00}'));
        let micros = clock.elapsed().as_micros();
        let made = glyph_total(&program);
        if made != Some(glyphs) {
            return Err(format!(
                "the program it makes holds {made:?} glyphs, not {glyphs}"
            ));
        }
        least = Some(least.map_or(micros, |held| held.min(micros)));
    }
    least.ok_or_else(|| "no run".to_string())
}

/// `program` in each storage form, `woff2` its WOFF2 form where there is one.
fn stored(program: &[u8], woff2: Option<&[u8]>) -> Stored {
    let glyphs = glyph_total(program).unwrap_or(0);
    let deflated = zlib_compress_at(program, DeflateLevel::Ratio);
    let limits = Limits::new(MAX_PROGRAM);
    let ready_raw = ready(glyphs, || Ok(Cow::Borrowed(program)));
    let ready_deflated = ready(glyphs, || {
        let decoded =
            flate_decode(&deflated, &limits, None).map_err(|error| format!("{error:?}"))?;
        if !decoded.complete || decoded.data != program {
            return Err("it does not inflate to the program".to_string());
        }
        Ok(Cow::Owned(decoded.data))
    });
    let ready_woff2 = woff2.map(|packed| {
        ready(glyphs, || {
            woff::decode(packed, MAX_PROGRAM)
                .map(Cow::Owned)
                .map_err(|error| error.to_string())
        })
    });
    Stored {
        bytes: program.len(),
        deflated: deflated.len(),
        woff2: woff2.map(<[u8]>::len),
        ready_raw,
        ready_deflated,
        ready_woff2,
    }
}

/// The WOFF2 form of `file` in `dir`: the file of its stem with `.woff2`.
fn woff2_beside(dir: &Path, file: &str) -> Option<Vec<u8>> {
    let stem = Path::new(file).file_stem()?.to_str()?;
    std::fs::read(dir.join(format!("{stem}.woff2"))).ok()
}

/// A ready time or why there is none, as a TSV cell.
fn ready_cell(ready: &Result<u128, String>) -> String {
    match ready {
        Ok(micros) => micros.to_string(),
        Err(why) => cell(&format!("unready: {why}")),
    }
}

// --- the `name` table --------------------------------------------------------

/// The `name` table record `id` as text: a Windows Unicode record first
/// (platform 3, encoding 1 or 10), American English before any other
/// language, then a Unicode platform record, then a Macintosh Roman one.
/// The text has its white space made single spaces, so it fits a TSV cell.
fn name_record(program: &[u8], id: u16) -> Option<String> {
    let table = Sfnt::parse(program)?.table(TAG_NAME)?;
    let count = usize::from(be16(table, 2)?);
    let strings = usize::from(be16(table, 4)?);
    let mut best: Option<(u8, String)> = None;
    for i in 0..count.min(1 << 12) {
        let at = 6 + i * 12;
        let (Some(platform), Some(encoding), Some(language), Some(name), Some(len), Some(off)) = (
            be16(table, at),
            be16(table, at + 2),
            be16(table, at + 4),
            be16(table, at + 6),
            be16(table, at + 8),
            be16(table, at + 10),
        ) else {
            break;
        };
        if name != id {
            continue;
        }
        let start = strings + usize::from(off);
        let Some(bytes) = table.get(start..start + usize::from(len)) else {
            continue;
        };
        let rank = match (platform, encoding, language) {
            (3, 1 | 10, 0x0409) => 0,
            (3, 1 | 10, _) => 1,
            (0, _, _) => 2,
            (1, 0, _) => 3,
            _ => continue,
        };
        if best.as_ref().is_some_and(|(held, _)| *held <= rank) {
            continue;
        }
        let text: String = if platform == 1 {
            bytes
                .iter()
                .filter_map(|byte| base_char(BaseEncoding::MacRoman, *byte))
                .collect()
        } else {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            char::decode_utf16(units)
                .map(|unit| unit.unwrap_or('\u{FFFD}'))
                .collect()
        };
        best = Some((rank, text.split_whitespace().collect::<Vec<_>>().join(" ")));
    }
    best.map(|(_, text)| text)
}

/// A TSV cell: no tab or line break inside it.
fn cell(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

// --- the census --------------------------------------------------------------

/// The `coverage` rows of one face: a row per collection and supplement, then
/// the collection's `all` row, each with its CIDs and, in each writing mode,
/// the CIDs its charset, its characters and the index reading reached and
/// the CIDs nothing reached.
fn coverage_rows(
    file: &str,
    index: usize,
    program: &[u8],
    collections: &[(&str, Vec<Vec<u32>>)],
) -> Vec<String> {
    let name = cell(&name_record(program, 4).unwrap_or_default());
    let Some(face) = Face::read(program) else {
        return vec![format!("coverage\t{file}\t{index}\t{name}\tunreadable")];
    };
    let mut rows = Vec::new();
    for (ordering, supplements) in collections {
        let tallies = coverage(&face, ordering, supplements);
        let mut whole = [Tally::default(); 2];
        for (supplement, (cids, modes)) in supplements.iter().zip(&tallies).enumerate() {
            whole = [whole[0].plus(modes[0]), whole[1].plus(modes[1])];
            rows.push(format!(
                "coverage\t{file}\t{index}\t{name}\tAdobe-{ordering}\t{supplement}\t{}\t{}\t{}",
                cids.len(),
                modes[0].cells(),
                modes[1].cells()
            ));
        }
        let cids: usize = supplements.iter().map(Vec::len).sum();
        rows.push(format!(
            "coverage\t{file}\t{index}\t{name}\tAdobe-{ordering}\tall\t{cids}\t{}\t{}",
            whole[0].cells(),
            whole[1].cells()
        ));
    }
    rows
}

/// **The census.** Prints, after [`RAN`], three tables, each row led by its
/// table's name: `inversion`, each collection's two tables' first-use time
/// and the CIDs each reaches; `coverage`, a row per candidate face, collection
/// and supplement and the collection's `all` row; and `storage`, a row per
/// candidate file, its bytes in each form and the time each takes to be made
/// ready. Holds nothing.
#[test]
#[ignore = "a census for the owner: H-116 and H-04 are ruled with its rows, and it holds nothing"]
fn cjk_face_coverage_census() {
    let Some(dir) = faces_dir() else {
        println!(
            "{SKIPPED} cjk_face_coverage_census (set TINKER_CJK_FACES to a directory holding \
             the candidates docs/design/fallback-faces.md names)"
        );
        return;
    };
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };

    // Each collection is inverted by its first use, so its two tables are
    // timed before anything else in the process asks for them: CID 1, the
    // space, which every collection's horizontal table reaches, so the
    // horizontal lookup builds the horizontal table alone.
    let mut inversions = Vec::new();
    for ordering in ORDERINGS {
        for vertical in [false, true] {
            let clock = Instant::now();
            std::hint::black_box(collection_character("Adobe", ordering, vertical, 1));
            let micros = clock.elapsed().as_micros();
            let reached = (0..=0xFFFF_u32)
                .filter(|cid| collection_character("Adobe", ordering, vertical, *cid).is_some())
                .count();
            inversions.push((ordering, vertical, reached, micros));
        }
    }

    let collections: Vec<(&str, Vec<Vec<u32>>)> = ORDERINGS
        .iter()
        .map(|ordering| (*ordering, supplements(ordering)))
        .collect();
    if collections
        .iter()
        .any(|(_, supplements)| supplements.is_empty())
    {
        println!(
            "{SKIPPED} cjk_face_coverage_census (this build holds no CID tables: \
             cmap-predefined is off)"
        );
        return;
    }

    println!(
        "{RAN} cjk_face_coverage_census ({}, {profile})",
        dir.display()
    );

    println!("{INVERSION_HEAD}");
    for (ordering, vertical, reached, micros) in &inversions {
        let mode = if *vertical { "vertical" } else { "horizontal" };
        println!("inversion\tAdobe-{ordering}\t{mode}\t{reached}\t{micros}\t{profile}");
    }

    println!("{COVERAGE_HEAD}");
    for candidate in &CANDIDATES {
        let Ok(program) = std::fs::read(dir.join(candidate.file)) else {
            println!(
                "coverage\t{}\t\t{}\tabsent\t\t\t\t\t\t\t\t\t\t\t",
                candidate.file,
                cell(candidate.face)
            );
            continue;
        };
        let count = face_count(&program);
        if count == 0 {
            println!("coverage\t{}\t\t\tunreadable", candidate.file);
        }
        for index in 0..count {
            match face_at(&program, index) {
                Some(face) => {
                    for row in coverage_rows(candidate.file, index, &face, &collections) {
                        println!("{row}");
                    }
                }
                None => println!("coverage\t{}\t{index}\t\tunreadable", candidate.file),
            }
        }
    }

    println!("{STORAGE_HEAD}");
    for candidate in &CANDIDATES {
        let Ok(program) = std::fs::read(dir.join(candidate.file)) else {
            println!(
                "storage\t{}\tabsent\t\t\t\t\t\t{profile}\t{}\t{}",
                candidate.file,
                cell(candidate.upstream),
                candidate.spdx
            );
            continue;
        };
        let woff2 = woff2_beside(&dir, candidate.file);
        let form = stored(&program, woff2.as_deref());
        println!(
            "storage\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{profile}\t{}\t{}",
            candidate.file,
            form.bytes,
            form.deflated,
            form.woff2
                .map_or_else(|| "absent".to_string(), |bytes| bytes.to_string()),
            ready_cell(&form.ready_raw),
            ready_cell(&form.ready_deflated),
            form.ready_woff2
                .as_ref()
                .map_or_else(|| "absent".to_string(), ready_cell),
            cell(candidate.upstream),
            candidate.spdx,
        );
    }
}

// --- the instruments ---------------------------------------------------------

/// The `/CIDCount` a vendored CMap file states, read from its text.
#[cfg(feature = "cmap-predefined")]
fn stated_cid_count(path: &Path) -> u32 {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("/CIDCount"))
        .unwrap_or_else(|| panic!("{} states no /CIDCount", path.display()));
    line.split_whitespace()
        .nth(1)
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("{}: {line}", path.display()))
}

/// The vendored `Adobe-<ordering>-<n>` CMap files, by supplement, as the
/// registry's directory for the collection holds them.
#[cfg(feature = "cmap-predefined")]
fn supplement_files(ordering: &str) -> Vec<PathBuf> {
    let registry = repo_file("crates/tinker-pdf-font/data/cmap-resources");
    let prefix = format!("Adobe-{ordering}-");
    let dirs: Vec<PathBuf> = std::fs::read_dir(&registry)
        .expect("the vendored registry")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.strip_prefix(&prefix)
                        .is_some_and(|rest| rest.parse::<u32>().is_ok())
                })
        })
        .collect();
    assert_eq!(dirs.len(), 1, "one registry directory for {ordering}");
    let mut files = Vec::new();
    for supplement in 0.. {
        let path = dirs[0].join("CMap").join(format!("{prefix}{supplement}"));
        if !path.is_file() {
            break;
        }
        files.push(path);
    }
    files
}

/// **The supplements are the registry's.** Each collection's supplements, read
/// through the compiled `Adobe-<ordering>-<n>` CMaps, are the CIDs from the
/// last supplement's `/CIDCount` up to their own, as a second reading — the
/// vendored files' text — states it, one for each such file, CID 0 left out of
/// the first.
#[cfg(feature = "cmap-predefined")]
#[test]
fn each_collections_supplements_are_the_registrys() {
    for ordering in ORDERINGS {
        let read = supplements(ordering);
        let files = supplement_files(ordering);
        assert!(!files.is_empty(), "{ordering} has supplement CMaps");
        assert_eq!(read.len(), files.len(), "{ordering}'s supplements");
        let mut from = 1u32;
        for (supplement, (cids, file)) in read.iter().zip(&files).enumerate() {
            let count = stated_cid_count(file);
            let want: Vec<u32> = (from..count).collect();
            assert_eq!(
                cids, &want,
                "Adobe-{ordering}-{supplement}: the CIDs {from} to {count}"
            );
            from = count;
        }
    }
    // The figures the files state, held here as well, so that a re-vendored
    // registry that moved them moves this test.
    let last = |ordering: &str| {
        supplements(ordering)
            .last()
            .and_then(|cids| cids.last().copied())
    };
    assert_eq!(last("Japan1"), Some(23_059));
    assert_eq!(last("GB1"), Some(30_571));
    assert_eq!(last("CNS1"), Some(19_178));
    assert_eq!(last("Korea1"), Some(18_351));
    assert_eq!(last("KR"), Some(22_896));
    assert_eq!(supplements("Japan1").len(), 8);
    assert_eq!(supplements("KR").len(), 10);
    assert!(
        supplements("Identity").is_empty(),
        "Adobe-Identity-0 has no supplement CMap"
    );
    assert!(
        supplements("Nothing").is_empty(),
        "an ordering the registry lacks"
    );
}

/// **Without the tables no supplement is read**: a build without
/// `cmap-predefined` holds no CID ranges, so the census reads no collection
/// and skips, rather than counting faces against nothing.
#[cfg(not(feature = "cmap-predefined"))]
#[test]
fn without_the_tables_no_supplement_is_read() {
    for ordering in ORDERINGS {
        assert!(supplements(ordering).is_empty(), "{ordering}");
    }
}

/// A CFF INDEX of `entries`, its offsets as wide as they need.
fn index(entries: &[Vec<u8>]) -> Vec<u8> {
    let count = u16::try_from(entries.len()).expect("a small INDEX");
    let mut out = count.to_be_bytes().to_vec();
    if entries.is_empty() {
        return out;
    }
    let total: usize = entries.iter().map(Vec::len).sum::<usize>() + 1;
    let size: usize = match total {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };
    out.push(u8::try_from(size).expect("an offset size"));
    let mut offset = 1usize;
    let push = |value: usize, out: &mut Vec<u8>| {
        let bytes = u32::try_from(value).expect("a small INDEX").to_be_bytes();
        out.extend_from_slice(&bytes[4 - size..]);
    };
    push(offset, &mut out);
    for entry in entries {
        offset += entry.len();
        push(offset, &mut out);
    }
    for entry in entries {
        out.extend_from_slice(entry);
    }
    out
}

/// A DICT integer in its five-byte form, so a DICT's length does not depend
/// on its offsets.
fn int(value: i32) -> Vec<u8> {
    let mut out = vec![29];
    out.extend_from_slice(&value.to_be_bytes());
    out
}

/// A CID-keyed CFF program whose `ROS` names `registry` and `ordering`, with
/// glyph 0 and a glyph for each of `cids`, in order, each an empty charstring.
fn cid_keyed(registry: &str, ordering: &str, cids: &[u16]) -> Vec<u8> {
    let header = [1u8, 0, 4, 4];
    let names = index(&[b"CJKCensus".to_vec()]);
    let strings = index(&[registry.as_bytes().to_vec(), ordering.as_bytes().to_vec()]);
    let globals = index(&[]);
    let charstrings = index(&vec![vec![14u8]; cids.len() + 1]);
    let mut charset = vec![0u8];
    for cid in cids {
        charset.extend_from_slice(&cid.to_be_bytes());
    }
    let top = |charset_at: i32, charstrings_at: i32| {
        let mut dict = Vec::new();
        dict.extend(int(391));
        dict.extend(int(392));
        dict.extend(int(0));
        dict.extend([12, 30]);
        dict.extend(int(charset_at));
        dict.push(15);
        dict.extend(int(charstrings_at));
        dict.push(17);
        dict
    };
    let top_len = index(&[top(0, 0)]).len();
    let charset_at = header.len() + names.len() + top_len + strings.len() + globals.len();
    let charstrings_at = charset_at + charset.len();
    let top_index = index(&[top(
        i32::try_from(charset_at).expect("a small program"),
        i32::try_from(charstrings_at).expect("a small program"),
    )]);
    assert_eq!(top_index.len(), top_len, "the Top DICT's length is fixed");
    let mut out = header.to_vec();
    for part in [names, top_index, strings, globals, charset, charstrings] {
        out.extend(part);
    }
    out
}

/// A `cmap` of one (3,10) format 12 subtable mapping each character to its
/// glyph.
fn cmap_of(pairs: &[(char, u16)]) -> Vec<u8> {
    let mut sorted = pairs.to_vec();
    sorted.sort_unstable();
    let groups = u32::try_from(sorted.len()).expect("a small cmap");
    let mut sub = Vec::new();
    sub.extend_from_slice(&12u16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    sub.extend_from_slice(&(16 + groups * 12).to_be_bytes());
    sub.extend_from_slice(&0u32.to_be_bytes());
    sub.extend_from_slice(&groups.to_be_bytes());
    for (c, glyph) in &sorted {
        sub.extend_from_slice(&u32::from(*c).to_be_bytes());
        sub.extend_from_slice(&u32::from(*c).to_be_bytes());
        sub.extend_from_slice(&u32::from(*glyph).to_be_bytes());
    }
    let mut out = Vec::new();
    for value in [0u16, 1, 3, 10] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend(sub);
    out
}

/// A `maxp` table, version 0.5, stating `glyphs`.
#[cfg(feature = "cmap-predefined")]
fn maxp(glyphs: u16) -> Vec<u8> {
    let mut out = 0x0000_5000u32.to_be_bytes().to_vec();
    out.extend_from_slice(&glyphs.to_be_bytes());
    out
}

/// A table directory of `version` over `tables`, and the tables it points
/// at, each padded to four bytes: a table's offset is `first_table` plus
/// where it lands among them.
fn directory(
    version: u32,
    tables: &[([u8; 4], Vec<u8>)],
    first_table: usize,
) -> (Vec<u8>, Vec<u8>) {
    let mut head = version.to_be_bytes().to_vec();
    head.extend_from_slice(
        &u16::try_from(tables.len())
            .expect("few tables")
            .to_be_bytes(),
    );
    head.extend_from_slice(&[0; 6]);
    let mut body = Vec::new();
    for (tag, table) in tables {
        head.extend_from_slice(tag);
        head.extend_from_slice(&0u32.to_be_bytes());
        let at = u32::try_from(first_table + body.len()).expect("a small face");
        head.extend_from_slice(&at.to_be_bytes());
        head.extend_from_slice(
            &u32::try_from(table.len())
                .expect("a small table")
                .to_be_bytes(),
        );
        body.extend_from_slice(table);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    (head, body)
}

/// An sfnt of `version` holding `tables`.
fn sfnt(version: u32, tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let (head, body) = directory(version, tables, 12 + 16 * tables.len());
    let mut out = head;
    out.extend(body);
    out
}

/// A collection (`ttcf`) of `faces`, each an OpenType CFF face's tables: the
/// header, every face's directory, then every face's tables.
fn collection(faces: &[Vec<([u8; 4], Vec<u8>)>]) -> Vec<u8> {
    let header_len = 12 + 4 * faces.len();
    let directories: usize = faces.iter().map(|tables| 12 + 16 * tables.len()).sum();
    let mut heads = Vec::new();
    let mut bodies = Vec::new();
    let mut offsets = Vec::new();
    for tables in faces {
        offsets.push(header_len + heads.len());
        let (head, body) = directory(0x4F54_544F, tables, header_len + directories + bodies.len());
        heads.extend(head);
        bodies.extend(body);
    }
    let mut out = b"ttcf".to_vec();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&u32::try_from(faces.len()).expect("few faces").to_be_bytes());
    for offset in offsets {
        out.extend_from_slice(&u32::try_from(offset).expect("a small file").to_be_bytes());
    }
    out.extend(heads);
    out.extend(bodies);
    out
}

/// The OpenType face the route tests read: a CFF keyed by `ros` holding
/// CIDs 3284 (`日` in Adobe-Japan1) and 10 as glyphs 1 and 2, and glyph 3
/// with CID 99, and a `cmap` mapping `A` to glyph 3 and `、` to glyph 2.
fn keyed_face(ros: (&str, &str)) -> Vec<u8> {
    let cff = cid_keyed(ros.0, ros.1, &[3284, 10, 99]);
    sfnt(
        0x4F54_544F,
        &[
            (*b"CFF ", cff),
            (*b"cmap", cmap_of(&[('A', 3), ('\u{3001}', 2)])),
        ],
    )
}

/// **The routes are the facade's, in its order.** A face keyed by Adobe-Japan1
/// reaches a CID its charset holds by the charset, `日`'s among them, which its
/// `cmap` does not map; reaches Adobe-Japan1's `A` by its character; reaches
/// the vertical ideographic comma, CID 7887, by its character only in a
/// vertical font — a horizontal font reads it as the presentation form
/// U+FE11, which the face does not map, and a vertical one as `、` — and
/// reaches no CID neither route reaches. Read for Adobe-GB1, the charset
/// keyed by Adobe-Japan1 is closed — GB1's CID 10 is `)`, which the face does
/// not map — and GB1's `中` is unreached and its `A` reached by character.
/// Keyed by another collection, the charset is closed and `日` is unreached.
/// A face with neither a `cmap` nor a `ROS` keeps the index reading, the CIDs
/// below its glyph count, and a program that is neither an sfnt nor a CFF is
/// no face.
#[cfg(feature = "cmap-predefined")]
#[test]
fn the_census_counts_each_cid_by_its_route() {
    let program = keyed_face(("Adobe", "Japan1"));
    let face = Face::read(&program).expect("an OpenType CFF face");
    assert_eq!(face.glyphs, 4);
    for vertical in [false, true] {
        assert_eq!(face.route("Japan1", vertical, 3284), Some(Route::Charset));
        assert_eq!(face.route("Japan1", vertical, 10), Some(Route::Charset));
        assert_eq!(face.route("Japan1", vertical, 34), Some(Route::Character));
        assert_eq!(face.route("Japan1", vertical, 50_000), None);
        assert_eq!(face.route("GB1", vertical, 10), None, "Japan1's charset");
        assert_eq!(face.route("GB1", vertical, 4559), None, "GB1's 中");
        assert_eq!(face.route("GB1", vertical, 34), Some(Route::Character));
    }
    assert_eq!(face.route("Japan1", false, 7887), None, "U+FE11");
    assert_eq!(face.route("Japan1", true, 7887), Some(Route::Character));

    let program = keyed_face(("Adobe", "Identity"));
    let face = Face::read(&program).expect("an OpenType CFF face");
    assert_eq!(
        face.route("Japan1", false, 3284),
        None,
        "the charset is closed"
    );
    assert_eq!(face.route("Japan1", false, 34), Some(Route::Character));

    let program = sfnt(0x0001_0000, &[(*b"maxp", maxp(40))]);
    let face = Face::read(&program).expect("a face of a maxp alone");
    assert_eq!(face.glyphs, 40);
    assert_eq!(face.route("Japan1", false, 39), Some(Route::Index));
    assert_eq!(face.route("Japan1", false, 40), None);

    let bare = cid_keyed("Adobe", "Japan1", &[3284]);
    let face = Face::read(&bare).expect("a bare CID-keyed CFF");
    assert!(face.sfnt.is_none());
    assert_eq!(face.route("Japan1", false, 3284), Some(Route::Charset));
    assert_eq!(face.route("Japan1", false, 34), None, "no cmap to read");

    assert!(Face::read(b"not a font").is_none());
}

/// **A face's tallies are its supplements' and their sum.** The keyed face
/// over two supplements of its own — CIDs 3284 and 34 in the first, 7887 and
/// 50 000 in the second — counts the first's two in both writing modes, the
/// second's comma vertically alone, and its CID past the collection as
/// unreached in both; the rows the census prints for it are a row a
/// supplement and the collection's `all` row, which sums them.
#[cfg(feature = "cmap-predefined")]
#[test]
fn a_face_is_tallied_supplement_by_supplement() {
    let program = keyed_face(("Adobe", "Japan1"));
    let face = Face::read(&program).expect("an OpenType CFF face");
    let supplements = vec![vec![34, 3284], vec![7887, 50_000]];
    let tallies = coverage(&face, "Japan1", &supplements);
    let first = Tally {
        charset: 1,
        character: 1,
        ..Tally::default()
    };
    assert_eq!(tallies[0], [first, first]);
    assert_eq!(
        tallies[1],
        [
            Tally {
                unreached: 2,
                ..Tally::default()
            },
            Tally {
                character: 1,
                unreached: 1,
                ..Tally::default()
            },
        ]
    );
    assert_eq!(
        tallies[0][1].plus(tallies[1][1]),
        Tally {
            charset: 1,
            character: 2,
            index: 0,
            unreached: 1,
        }
    );

    let rows = coverage_rows("keyed.otf", 0, &program, &[("Japan1", supplements.clone())]);
    assert_eq!(
        rows,
        [
            "coverage\tkeyed.otf\t0\t\tAdobe-Japan1\t0\t2\t1\t1\t0\t0\t1\t1\t0\t0",
            "coverage\tkeyed.otf\t0\t\tAdobe-Japan1\t1\t2\t0\t0\t0\t2\t0\t1\t0\t1",
            "coverage\tkeyed.otf\t0\t\tAdobe-Japan1\tall\t4\t1\t1\t0\t2\t1\t2\t0\t1",
        ]
    );
    assert_eq!(
        coverage_rows("junk.otf", 0, b"junk", &[("Japan1", supplements)]),
        ["coverage\tjunk.otf\t0\t\tunreadable"]
    );
}

/// **A collection's faces are each read as its own.** A collection of two
/// faces whose `cmap`s map `A` to different glyphs holds two faces, each read
/// with its own `cmap`; a plain face is one face, read as it stands; and a
/// face past the header's count is none.
#[test]
fn each_face_of_a_collection_is_read_as_its_own() {
    let face = |glyph: u16| {
        vec![
            (*b"CFF ", cid_keyed("Adobe", "Identity", &[1, 2, 3])),
            (*b"cmap", cmap_of(&[('A', glyph)])),
        ]
    };
    let both = collection(&[face(1), face(2)]);
    assert_eq!(face_count(&both), 2);
    for (index, glyph) in [(0usize, 1u16), (1, 2)] {
        let bytes = face_at(&both, index).expect("a face the header lists");
        let sfnt = Sfnt::parse(&bytes).expect("each face parses");
        assert_eq!(sfnt.glyph_for_char('A'), Some(glyph), "face {index}");
        assert_eq!(glyph_total(&bytes), Some(4));
    }
    assert!(face_at(&both, 2).is_none());

    let plain = keyed_face(("Adobe", "Japan1"));
    assert_eq!(face_count(&plain), 1);
    assert!(matches!(face_at(&plain, 0), Some(Cow::Borrowed(_))));
    assert!(face_at(&plain, 1).is_none());

    let mut damaged = both.clone();
    damaged[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(face_count(&damaged), MAX_FACES, "a damaged count is capped");
    assert_eq!(face_count(b"ttcf"), 0, "a header cut short holds none");
}

/// A committed file of the WOFF fixtures, by name.
fn woff_fixture(name: &str) -> Vec<u8> {
    let path = repo_file("crates/tinker-pdf-font/tests/woff").join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// **Each storage form is measured as it is made ready.** The committed WOFF
/// fixtures' source face and its WOFF2 form: the face's bytes as published,
/// deflated to fewer, and the WOFF2 file's own; each form made ready — the
/// deflated one inflated back to the face byte for byte, the WOFF2 one
/// unpacked to a face of the same glyphs — in a time of its own. A WOFF2
/// form that is no WOFF is unready and says why, and a directory with no
/// WOFF2 form beside a file has none.
#[test]
fn each_storage_form_is_measured_as_it_is_made_ready() {
    let program = woff_fixture("synthetic-2.ttf");
    let packed = woff_fixture("synthetic-2.woff2");
    let form = stored(&program, Some(&packed));
    assert_eq!(form.bytes, program.len());
    assert!(form.deflated > 0, "{form:?}");
    assert_eq!(form.woff2, Some(packed.len()));
    assert!(form.ready_raw.is_ok(), "{form:?}");
    assert!(form.ready_deflated.is_ok(), "{form:?}");
    assert!(
        form.ready_woff2.as_ref().is_some_and(Result::is_ok),
        "{form:?}"
    );

    let refused = stored(&program, Some(b"not a woff"));
    assert!(
        refused
            .ready_woff2
            .as_ref()
            .is_some_and(|ready| ready.as_ref().is_err_and(|why| !why.is_empty())),
        "{refused:?}"
    );
    assert!(ready_cell(&Err("why".to_string())).starts_with("unready"));

    let dir = repo_file("crates/tinker-pdf-font/tests/woff");
    assert_eq!(woff2_beside(&dir, "synthetic-2.ttf"), Some(packed));
    assert_eq!(
        woff2_beside(&dir, "synthetic-2-aligned.ttf"),
        None,
        "no synthetic-2-aligned.woff2 is committed"
    );
    assert!(stored(&program, None).ready_woff2.is_none());
}

/// A `name` table with `records`, each `(platform, encoding, language, id,
/// bytes)`, in a face of its own.
fn face_with_names(records: &[(u16, u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes());
    table.extend_from_slice(&u16::try_from(records.len()).expect("few").to_be_bytes());
    table.extend_from_slice(
        &u16::try_from(6 + 12 * records.len())
            .expect("a small table")
            .to_be_bytes(),
    );
    let mut offset = 0usize;
    for (platform, encoding, language, id, bytes) in records {
        for value in [
            *platform,
            *encoding,
            *language,
            *id,
            u16::try_from(bytes.len()).expect("a short record"),
            u16::try_from(offset).expect("a small table"),
        ] {
            table.extend_from_slice(&value.to_be_bytes());
        }
        offset += bytes.len();
    }
    for (.., bytes) in records {
        table.extend_from_slice(bytes);
    }
    sfnt(0x0001_0000, &[(*b"name", table)])
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// A face's name is its Windows English full name where it has one, then any
/// Windows record, then a Unicode one, then a Macintosh Roman one, its white
/// space made single spaces; a face with no such record has none.
#[test]
fn a_faces_name_is_read_windows_english_first() {
    let face = face_with_names(&[
        (1, 0, 0, 4, b"Mac name".to_vec()),
        (3, 1, 0x0411, 4, utf16("\u{6E90}\u{30CE}\u{89D2}")),
        (3, 1, 0x0409, 4, utf16("Source Han Sans\nJP Regular")),
    ]);
    assert_eq!(
        name_record(&face, 4).as_deref(),
        Some("Source Han Sans JP Regular")
    );
    assert_eq!(name_record(&face, 1), None, "no record for the ID");
    let face = face_with_names(&[(3, 1, 0x0411, 4, utf16("\u{6E90}\u{30CE}\u{89D2}"))]);
    assert_eq!(
        name_record(&face, 4).as_deref(),
        Some("\u{6E90}\u{30CE}\u{89D2}")
    );
    let face = face_with_names(&[(1, 0, 0, 4, b"Mac \xA9 name".to_vec())]);
    assert_eq!(name_record(&face, 4).as_deref(), Some("Mac \u{A9} name"));
    assert_eq!(name_record(b"not a font", 4), None);
    assert_eq!(cell("a\tb\nc"), "a b c");
}

/// The candidate list is the design's: every file and SPDX expression the
/// census reads is in `fallback-faces.md`'s candidate table, and every file
/// that table names is a candidate.
#[test]
fn every_candidate_is_the_designs() {
    let doc =
        std::fs::read_to_string(repo_file("docs/design/fallback-faces.md")).expect("the design");
    let header = "| Candidate | Regions | Files | Upstream | SPDX |";
    let table = doc
        .split(header)
        .nth(1)
        .and_then(|rest| rest.split("\n\n").next())
        .expect("the candidate table");
    let mut named = BTreeSet::new();
    for row in table.lines().filter(|line| line.starts_with("| ")) {
        let cells: Vec<&str> = row.split('|').collect();
        let (Some(files), Some(spdx)) = (cells.get(3), cells.get(5)) else {
            continue;
        };
        if files.trim().starts_with("---") {
            continue;
        }
        let listed: Vec<&str> = files.split('`').skip(1).step_by(2).collect();
        assert!(!listed.is_empty(), "a row names its files: {row}");
        for file in listed {
            named.insert(file.to_string());
            let candidate = CANDIDATES
                .iter()
                .find(|candidate| candidate.file == file)
                .unwrap_or_else(|| panic!("{file} is no candidate of the census"));
            assert!(
                spdx.contains(&format!("`{}`", candidate.spdx)),
                "{file} is {} in the design: {row}",
                candidate.spdx
            );
        }
    }
    let read: BTreeSet<String> = CANDIDATES
        .iter()
        .map(|candidate| candidate.file.to_string())
        .collect();
    assert_eq!(named, read, "the design's files and the census's");
}

// --- the committed record -----------------------------------------------------

/// The committed record, `cjk_coverage.tsv` beside this file, as its rows'
/// cells: its `#` head and blank lines left out.
fn record() -> Vec<Vec<String>> {
    let path = repo_file("crates/tinker-pdf-font/tests/cjk_coverage.tsv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect()
}

/// A table's head as the record carries it: the census's, less [`TIMED`].
fn committed_head(head: &str) -> Vec<String> {
    head.split('\t')
        .filter(|column| !TIMED.contains(column))
        .map(str::to_string)
        .collect()
}

/// A count cell of the record, as a number.
fn count(cell: &str, row: &[String]) -> usize {
    cell.parse()
        .unwrap_or_else(|_| panic!("{cell:?} is no count: {row:?}"))
}

/// **The committed record is whole and adds up.** `cjk_coverage.tsv` holds
/// the census's three tables in its order and nothing else, each headed by the
/// census's own head less [`TIMED`]: an `inversion` row for each collection in
/// each writing mode; then, for each candidate in turn, its faces in order —
/// one for a file that is not a collection, more for one that is — each with
/// every collection's supplements from 0 and then its `all` row, every face
/// giving a collection the same supplements and a supplement the same CIDs,
/// each writing mode's four counts adding up to the row's CIDs, each `all` row
/// the sum of its supplements, and no face reaching more of a collection by
/// its characters than that collection's table in that writing mode reaches;
/// then a `storage` row for each candidate, with its bytes in all three forms
/// and the upstream and SPDX expression the candidate list gives it. A
/// candidate the census found absent or unreadable has no such rows, so the
/// record is no whole census.
#[test]
fn the_committed_census_is_whole() {
    let rows = record();
    let ranks: Vec<u8> = rows
        .iter()
        .map(|row| match row[0].as_str() {
            "inversion" => 0,
            "coverage" => 1,
            "storage" => 2,
            other => panic!("a row of no census table, {other:?}: {row:?}"),
        })
        .collect();
    assert!(
        ranks.windows(2).all(|pair| pair[0] <= pair[1]),
        "the three tables, in the census's order"
    );
    let table =
        |name: &str| -> Vec<&Vec<String>> { rows.iter().filter(|row| row[0] == name).collect() };

    // The inversion: each collection's two tables, horizontal first.
    let inversion = table("inversion");
    assert_eq!(*inversion[0], committed_head(INVERSION_HEAD));
    let mut reached = BTreeMap::new();
    let mut rows_of = inversion[1..].iter();
    for ordering in ORDERINGS {
        for mode in ["horizontal", "vertical"] {
            let row = rows_of.next().expect("an inversion row for each table");
            assert_eq!(row.len(), 4, "{row:?}");
            assert_eq!(row[1], format!("Adobe-{ordering}"), "{row:?}");
            assert_eq!(row[2], mode, "{row:?}");
            reached.insert((ordering, mode), count(&row[3], row));
        }
    }
    assert!(rows_of.next().is_none(), "an inversion row of no table");

    // The coverage: every face of every candidate, in order.
    let coverage = table("coverage");
    assert_eq!(*coverage[0], committed_head(COVERAGE_HEAD));
    let mut data = coverage[1..].iter().peekable();
    let mut supplement_cids: BTreeMap<(&str, usize), usize> = BTreeMap::new();
    let mut supplement_count: BTreeMap<&str, usize> = BTreeMap::new();
    for candidate in &CANDIDATES {
        let mut faces = 0usize;
        while data.peek().is_some_and(|row| row[1] == candidate.file) {
            for ordering in ORDERINGS {
                let mut sums = [0usize; 9];
                let mut supplement = 0usize;
                loop {
                    let row = data
                        .next()
                        .unwrap_or_else(|| panic!("{} stops short", candidate.file));
                    assert_eq!(row.len(), 15, "{row:?}");
                    assert_eq!(row[1], candidate.file, "{row:?}");
                    assert_eq!(row[2], faces.to_string(), "{row:?}");
                    assert!(!row[3].is_empty(), "every face names itself: {row:?}");
                    assert_eq!(row[4], format!("Adobe-{ordering}"), "{row:?}");
                    let numbers: Vec<usize> =
                        row[6..].iter().map(|value| count(value, row)).collect();
                    assert_eq!(
                        numbers[1..5].iter().sum::<usize>(),
                        numbers[0],
                        "the horizontal counts add up to the CIDs: {row:?}"
                    );
                    assert_eq!(
                        numbers[5..9].iter().sum::<usize>(),
                        numbers[0],
                        "the vertical counts add up to the CIDs: {row:?}"
                    );
                    if row[5] == "all" {
                        assert!(supplement > 0, "a collection with no supplement: {row:?}");
                        assert_eq!(numbers, sums, "the all row sums its supplements: {row:?}");
                        assert!(
                            numbers[2] <= reached[&(ordering, "horizontal")]
                                && numbers[6] <= reached[&(ordering, "vertical")],
                            "no face reaches by character a CID its table does not: {row:?}"
                        );
                        break;
                    }
                    assert_eq!(row[5], supplement.to_string(), "{row:?}");
                    let held = supplement_cids
                        .entry((ordering, supplement))
                        .or_insert(numbers[0]);
                    assert_eq!(*held, numbers[0], "every face's supplement: {row:?}");
                    for (sum, number) in sums.iter_mut().zip(&numbers) {
                        *sum += number;
                    }
                    supplement += 1;
                }
                let held = supplement_count.entry(ordering).or_insert(supplement);
                assert_eq!(*held, supplement, "{ordering}'s supplements, face {faces}");
            }
            faces += 1;
        }
        if candidate.file.ends_with(".ttc") {
            assert!(faces > 1, "{} is a collection of faces", candidate.file);
        } else {
            assert_eq!(faces, 1, "{} is one face", candidate.file);
        }
    }
    assert!(
        data.next().is_none(),
        "a coverage row of no candidate, or out of order"
    );

    // The storage: each candidate's three forms.
    let storage = table("storage");
    assert_eq!(*storage[0], committed_head(STORAGE_HEAD));
    assert_eq!(storage.len(), CANDIDATES.len() + 1, "a storage row each");
    for (row, candidate) in storage[1..].iter().zip(&CANDIDATES) {
        assert_eq!(row.len(), 7, "{row:?}");
        assert_eq!(row[1], candidate.file, "{row:?}");
        for bytes in &row[2..5] {
            assert!(count(bytes, row) > 0, "every form has bytes: {row:?}");
        }
        assert_eq!(row[5], cell(candidate.upstream), "{row:?}");
        assert_eq!(row[6], candidate.spdx, "{row:?}");
    }
}
