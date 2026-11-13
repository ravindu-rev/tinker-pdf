//! Every input the benchmark set times, and nothing that times it.
//!
//! This file holds no criterion type, no clock and no spawn, so a test can
//! reach a generator by path (`#[path = "../benches/support/inputs.rs"]`)
//! without compiling the harness. Every input is written here, in memory, or
//! read by path from what the tree already commits; a run needs no corpus.
//!
//! The four first below are the seven operations' inputs, moved out of the
//! single `benches/engine.rs` they were written in with their bodies
//! unchanged, so each of the seven still times the bytes it timed there. The
//! rest are the benchmark set's (`docs/design/performance.md`, "The inputs"),
//! written by formula where the bytes are pinned: seeded integer arithmetic
//! and no float, so every target computes the same bytes.
//!
//! The encode group's two deflate inputs and the level-6 byte counts tier P
//! holds their deflate to ([`LevelSix`]) are in `samples.rs`, beside the
//! seeded samples, and are re-exported here; a test that reads the counts takes
//! that file alone, by the same kind of path.
//!
//! # The inputs a paired run hands every build
//!
//! A generator not pinned by formula writes through the engine's own
//! builder, encoders, editor or renderer, so two builds that each generate
//! "one" input time two; and a committed file is read from each build's own
//! checkout. So every group file takes each input through a [`Store`]
//! (`docs/design/performance.md`, MS-02, "The inputs both sides are timed
//! on"): with [`SHARED`] naming a directory, an input is read from the file
//! there of its name, and generated — or read from where the tree commits it
//! — and written there first when the directory holds none, so the first
//! process of a run to need an input writes it for every build the run times.
//! Without it, an input is generated in memory, and a file a child reads is
//! this process's own. A name says which generator wrote the input and with
//! what: `dense-5.pdf` is [`dense`]'s five pages at seed 0, `dense-50-3.pdf`
//! its fifty at seed 3, `dense-quarter-8.pdf` [`dense_part`]'s eight
//! quarter-size pages.
//!
//! The generators themselves never read the directory: an input built from
//! another is built from the other's generator, so a process writes the
//! inputs its operation reads and no other's, and the calibration operation,
//! which calls [`dense`] itself, keeps its own build's bytes.
//!
//! # Where the committed files are read
//!
//! Every file the tree commits is read through [`in_tree`], under the
//! crate's directory as the process sees it: where cargo built the crate,
//! or the directory [`CRATE_DIR`] names, which a wasm leg sets to the path
//! its runtime's directory grant gives the crate.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tinker_pdf::write::{self, SaveOptions};
use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, BilevelCodec, Bitmap, CcittParams,
    CompressedImage, ContinuousCodec, DeviceSpace, Document, DocumentBuilder, Encryption, Function,
    ImageColorSpace, ImageData, ImageFilter, ImagePolicy, ImageRecoding, OpenOptions, PageBox,
    PageBuilder, PixelFormat, RenderOptions, Shading, TilingPattern, TilingType, WriteMode,
    WriteOptions,
};
use tinker_pdf_cos::edit::annot::{self, Color};
use tinker_pdf_cos::pages::Rect;
use tinker_pdf_cos::write::StreamData;
use tinker_pdf_cos::{Dict, ObjRef, Object};
use tinker_pdf_crypto::bignum::{Modulus, Uint};
use tinker_pdf_crypto::rsa::{DigestAlgorithm, RSA_LIMBS};
use tinker_pdf_crypto::sha2::sha256;
use tinker_pdf_font::Sfnt;

/// The raw byte inputs and the level-6 counts beside them, which a test
/// that needs no document takes alone.
#[path = "samples.rs"]
mod samples;

use samples::hex;
#[allow(unused_imports, reason = "each group reads a different few")]
pub use samples::{
    page_like, photo, photo_like, LevelSix, Seeded, PAGE_BYTES, PAGE_LIKE_LEVEL_SIX,
    PHOTO_LIKE_LEVEL_SIX,
};

/// The separations inputs, pages written byte by byte that `separations.rs`
/// takes alone to hold their plates: the render group's separations rows
/// draw them.
#[path = "plates.rs"]
mod plates;

#[allow(unused_imports, reason = "the render group reads them and no other")]
pub use plates::{separations_page, spots_page};

/// The tests' EPUB support, for its OCF writer: the generated books go
/// through the same writer as the tests' own.
#[path = "../../tests/epub_support/mod.rs"]
mod epub_support;

use epub_support::{ocf_zip, OcfEntry, OCF_MEDIA_TYPE};

/// The variable naming the directory every process of a paired run reads
/// its inputs from. The run sets it, to a directory that starts empty, for
/// every process of all three builds.
pub const SHARED: &str = "TINKER_BENCH_FIXTURES";

/// Where a group file's inputs come from: the directory [`SHARED`] names,
/// or none.
pub struct Store {
    dir: Option<PathBuf>,
}

/// The store this process's environment names: the directory [`SHARED`]
/// names, or none when it is unset. Set to nothing, or to a path relative to
/// the directory a process runs in, which differs from one checkout to the
/// next, it is refused, since a run that meant to share and did not would
/// time two inputs in silence.
pub fn store() -> Store {
    let Some(dir) = std::env::var_os(SHARED) else {
        return Store { dir: None };
    };
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_absolute(),
        "{SHARED} is {:?}, and names no directory every build's processes share: \
         set it to an absolute path",
        dir
    );
    Store { dir: Some(dir) }
}

/// The variable naming this crate's directory as the bench process sees it,
/// where that is not where cargo built it: under a wasm runtime, the guest
/// path the runtime's directory grant puts the crate at
/// (`docs/verification.md`, "The wasm legs").
pub const CRATE_DIR: &str = "TINKER_BENCH_CRATE_DIR";

/// `relative`, a path under this crate's directory, as this process reads
/// it: under the directory [`CRATE_DIR`] names, or where cargo built the
/// crate when it is unset. A wasm runtime reaches the host's files only
/// through the directories it grants, at paths of its own, and a Windows
/// host's drive path means nothing to it, so on a wasm target the variable
/// is required; set to a relative path, which would resolve in whatever
/// directory a process runs in, it is refused.
pub fn in_tree(relative: &str) -> PathBuf {
    let dir = match std::env::var_os(CRATE_DIR) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            assert!(
                dir.is_absolute(),
                "{CRATE_DIR} is {dir:?}, and names no directory a process reads the same \
                 wherever it runs: set it to an absolute path"
            );
            dir
        }
        None => {
            assert!(
                !cfg!(target_family = "wasm"),
                "{CRATE_DIR} is unset, and a wasm runtime cannot read the crate where cargo \
                 built it: set it to the path the runtime's directory grant gives the crate"
            );
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        }
    };
    dir.join(relative)
}

impl Store {
    /// The store over `dir`, as a run that sets [`SHARED`] to it has.
    pub fn shared(dir: impl Into<PathBuf>) -> Store {
        Store {
            dir: Some(dir.into()),
        }
    }

    /// The input `name`: the file of that name in the shared directory, or,
    /// when the directory holds none, `generate`d and written there first,
    /// so every later call, in this process or another, reads it back.
    /// Without a shared directory, `generate`d.
    pub fn bytes(&self, name: &str, generate: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
        let Some(dir) = &self.dir else {
            return generate();
        };
        let path = dir.join(name);
        match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let bytes = generate();
                write_whole(&path, &bytes);
                bytes
            }
            Err(error) => panic!("the input {}: {error}", path.display()),
        }
    }

    /// The input `name` as a file a child reads, and its path: the shared
    /// directory's, `generate`d and written there first when it holds none;
    /// without one, this process's own, written into `bench-fixtures/` under
    /// the directory cargo gives every bench target each time it is asked
    /// for, so no earlier build's bytes are read in its place.
    pub fn file(&self, name: &str, generate: impl FnOnce() -> Vec<u8>) -> PathBuf {
        match &self.dir {
            Some(dir) => {
                let path = dir.join(name);
                if !path.is_file() {
                    write_whole(&path, &generate());
                }
                path
            }
            None => {
                let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
                    .join("bench-fixtures")
                    .join(name);
                write_whole(&path, &generate());
                path
            }
        }
    }

    /// [`Store::bytes`] for text: the input `name` as UTF-8.
    pub fn text(&self, name: &str, generate: impl FnOnce() -> String) -> String {
        String::from_utf8(self.bytes(name, || generate().into_bytes()))
            .unwrap_or_else(|_| panic!("the input {name} is not UTF-8"))
    }

    /// [`Store::bytes`] for a set of files: the input `name` holds every
    /// file of the set, by name and in order, as one [`bundle`].
    pub fn set(&self, name: &str, generate: impl FnOnce() -> Vec<Fixture>) -> Vec<Fixture> {
        let bytes = self.bytes(name, || {
            let set = generate();
            let parts: Vec<(&str, &[u8])> = set
                .iter()
                .map(|fixture| (fixture.name.as_str(), &fixture.bytes[..]))
                .collect();
            bundle(&parts)
        });
        unbundle(&bytes, name)
            .into_iter()
            .map(|(name, bytes)| Fixture {
                name,
                bytes: bytes.into(),
            })
            .collect()
    }

    /// [`Store::bytes`] for a [`scan`]: its width, its height and its bits,
    /// as one [`bundle`].
    pub fn scan(
        &self,
        name: &str,
        generate: impl FnOnce() -> (u32, u32, Arc<[u8]>),
    ) -> (u32, u32, Arc<[u8]>) {
        let bytes = self.bytes(name, || {
            let (width, height, bits) = generate();
            bundle(&[
                ("width", &width.to_le_bytes()),
                ("height", &height.to_le_bytes()),
                ("bits", &bits),
            ])
        });
        let [width, height, bits] = parts(&bytes, name, ["width", "height", "bits"]);
        let (width, height) = (
            u32::from_le_bytes(fixed(&width, name)),
            u32::from_le_bytes(fixed(&height, name)),
        );
        assert_eq!(
            bits.len(),
            width.div_ceil(8) as usize * height as usize,
            "the scan {name} holds every row of its bits"
        );
        (width, height, bits.into())
    }

    /// [`Store::bytes`] for a bitmap a render draws, which crosses as its
    /// pixels: its size, format, stride and alpha, and its rows, as one
    /// [`bundle`]. What comes back is a bitmap of those pixels with no
    /// warnings, whether this process rendered it or read it.
    pub fn bitmap(&self, name: &str, render: impl FnOnce() -> Bitmap) -> Bitmap {
        let bytes = self.bytes(name, || {
            let drawn = render();
            let format = format!("{:?}", drawn.format);
            bundle(&[
                ("width", &drawn.width.to_le_bytes()),
                ("height", &drawn.height.to_le_bytes()),
                ("format", format.as_bytes()),
                ("stride", &(drawn.stride as u64).to_le_bytes()),
                ("premultiplied", &[u8::from(drawn.premultiplied)]),
                ("pixels", &drawn.data),
            ])
        });
        let [width, height, format, stride, premultiplied, pixels] = parts(
            &bytes,
            name,
            [
                "width",
                "height",
                "format",
                "stride",
                "premultiplied",
                "pixels",
            ],
        );
        let format = FORMATS
            .into_iter()
            .find(|known| format!("{known:?}").as_bytes() == format.as_slice())
            .unwrap_or_else(|| panic!("the bitmap {name} names no pixel format the facade has"));
        let [premultiplied] = fixed(&premultiplied, name);
        bitmap(
            u32::from_le_bytes(fixed(&width, name)),
            u32::from_le_bytes(fixed(&height, name)),
            format,
            usize::try_from(u64::from_le_bytes(fixed(&stride, name)))
                .unwrap_or_else(|_| panic!("the bitmap {name}'s stride fits no address")),
            premultiplied != 0,
            pixels,
        )
    }
}

/// `bytes` written as the file `path`, whole or not at all: into a file of
/// this process's own beside it, then renamed onto it, so no process reads
/// half an input another is writing.
fn write_whole(path: &Path, bytes: &[u8]) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        panic!("the input {} names no file in a directory", path.display());
    };
    // A process's partial file carries its id, so two writing the same input
    // at once each write their own; `wasm32-wasip1` has no process ids, and
    // `std::process::id` panics there.
    let writer = if cfg!(target_os = "wasi") {
        "wasi".to_string()
    } else {
        std::process::id().to_string()
    };
    let part = dir.join(format!(".{}.{writer}.part", name.to_string_lossy()));
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&part, bytes))
        .and_then(|()| std::fs::rename(&part, path))
        .unwrap_or_else(|error| panic!("the input {}: {error}", path.display()));
}

/// What every bundle starts with, so a file that is none is refused by name.
const BUNDLE: &[u8] = b"tinker-pdf bench bundle\n";

/// Named parts as one file: [`BUNDLE`], then each part's name and its bytes,
/// each after its length as eight little-endian bytes.
pub fn bundle(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = BUNDLE.to_vec();
    for (name, bytes) in parts {
        for piece in [name.as_bytes(), *bytes] {
            out.extend_from_slice(&(piece.len() as u64).to_le_bytes());
            out.extend_from_slice(piece);
        }
    }
    out
}

/// A [`bundle`]'s parts, by name and in order; one that is not a whole
/// bundle panics, naming `what`.
pub fn unbundle(bytes: &[u8], what: &str) -> Vec<(String, Vec<u8>)> {
    let mut rest = bytes
        .strip_prefix(BUNDLE)
        .unwrap_or_else(|| panic!("the input {what} is no bundle"));
    let mut out = Vec::new();
    while !rest.is_empty() {
        let name = String::from_utf8(piece(&mut rest, what).to_vec())
            .unwrap_or_else(|_| panic!("the bundle {what} names a part in other than UTF-8"));
        out.push((name, piece(&mut rest, what).to_vec()));
    }
    out
}

/// The next piece of a bundle, after its length, `rest` moved past it.
fn piece<'b>(rest: &mut &'b [u8], what: &str) -> &'b [u8] {
    let Some((length, after)) = rest.split_first_chunk::<8>() else {
        panic!("the bundle {what} ends inside a length");
    };
    let Some((piece, after)) = usize::try_from(u64::from_le_bytes(*length))
        .ok()
        .and_then(|length| after.split_at_checked(length))
    else {
        panic!("the bundle {what} ends inside a part");
    };
    *rest = after;
    piece
}

/// The parts of the bundle `bytes` when they are `names`, in that order.
fn parts<const N: usize>(bytes: &[u8], what: &str, names: [&str; N]) -> [Vec<u8>; N] {
    let found = unbundle(bytes, what);
    let named: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(named, names, "the parts of the bundle {what}");
    let parts: Vec<Vec<u8>> = found.into_iter().map(|(_, bytes)| bytes).collect();
    parts
        .try_into()
        .unwrap_or_else(|_| unreachable!("the names were counted"))
}

/// A part that is a number of `B` bytes, as `B` bytes.
fn fixed<const B: usize>(part: &[u8], what: &str) -> [u8; B] {
    part.try_into()
        .unwrap_or_else(|_| panic!("a part of {what} is {} bytes, not {B}", part.len()))
}

/// Every pixel format the facade has, which a stored bitmap names by its
/// `Debug` name.
const FORMATS: [PixelFormat; 8] = [
    PixelFormat::Gray8,
    PixelFormat::GrayA8,
    PixelFormat::Rgb8,
    PixelFormat::Rgba8,
    PixelFormat::CmykA8,
    PixelFormat::LabA8,
    PixelFormat::Bgr8,
    PixelFormat::Bgra8,
];

/// A bitmap of `data`, with no warnings and no statistics, as a caller
/// builds one by hand.
#[cfg(not(tinker_planning_baseline))]
fn bitmap(
    width: u32,
    height: u32,
    format: PixelFormat,
    stride: usize,
    premultiplied: bool,
    data: Vec<u8>,
) -> Bitmap {
    Bitmap {
        width,
        height,
        format,
        stride,
        data,
        warnings: Vec::new(),
        premultiplied,
        stats: None,
    }
}

/// The same bitmap on the planning baseline, whose `Bitmap` carries no
/// statistics.
#[cfg(tinker_planning_baseline)]
fn bitmap(
    width: u32,
    height: u32,
    format: PixelFormat,
    stride: usize,
    premultiplied: bool,
    data: Vec<u8>,
) -> Bitmap {
    Bitmap {
        width,
        height,
        format,
        stride,
        data,
        warnings: Vec::new(),
        premultiplied,
    }
}

/// Three A4 pages of forty lines each in standard-14 Helvetica, unembedded.
///
/// The seven's text page, and the document their open and rewrite take. No
/// glyph of it is drawn in the default build: Helvetica names no program, so
/// the renderer reports `UnreadableFont` and paints nothing (see
/// [`triangles`]).
pub fn three_pages() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for page in 0..3 {
        builder.add_page(595.0, 842.0, |p| {
            for line in 0..40 {
                p.text(
                    b"F0",
                    11.0,
                    72.0,
                    780.0 - f64::from(line) * 16.0,
                    &format!("Page {page}, line {line}: the quick brown fox jumps over it."),
                );
            }
        });
    }
    builder.finish()
}

/// One page carrying an axial shading over its whole area.
pub fn axial() -> Vec<u8> {
    axial_part(1)
}

/// [`axial`] on a page a `divisor`th of A4 each way, the shading's axis
/// with it, so a render draws a `divisor`²th of the pixels the same ramp
/// takes.
pub fn axial_part(divisor: u32) -> Vec<u8> {
    let (width, height) = page_part(divisor);
    let mut builder = DocumentBuilder::new();
    let shading = Shading::Axial {
        color_space: DeviceSpace::Rgb,
        coords: [0.0, 0.0, width, height],
        function: Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.0, 0.1, 0.6],
            c1: vec![1.0, 0.9, 0.2],
            n: 1.0,
        },
        extend: (true, true),
    };
    builder.add_shading(b"S0", &shading);
    builder.add_page(width, height, |page| {
        page.shading(b"S0");
    });
    builder.finish()
}

/// A page a `divisor`th of A4 each way, in points.
pub fn page_part(divisor: u32) -> (f64, f64) {
    (595.0 / f64::from(divisor), 842.0 / f64::from(divisor))
}

/// Three hundred filled paths with edges on no axis, over a whole page.
///
/// **The only one of the seven that spends its time in the scanline
/// rasteriser, and it was added because the roadmap's vectorisation row could
/// not be measured without one.** That row's exit criterion said "a measured
/// speedup on the six benchmarks", and of the six: four never enter
/// `tinker-pdf-raster` at all, the shading is its own sampler, and *"render
/// text at 150 dpi" draws no glyphs* — its fixture names base-14 Helvetica,
/// embeds no program, `bundled-fonts` is off by default, so the renderer
/// pushes `UnreadableFont` and paints nothing. A speedup in the span filler
/// would have moved none of them.
///
/// Triangles rather than rectangles, and at fractional coordinates, because
/// the loops in question are the ones that accumulate *partial* coverage: an
/// axis-aligned rectangle on integers is the one shape that costs a span
/// filler nothing to anti-alias.
pub fn triangles() -> Vec<u8> {
    let mut content = String::with_capacity(24 * 1024);
    for index in 0..300 {
        let step = f64::from(index);
        let x = 20.0 + (step * 37.0) % 520.0;
        let y = 20.0 + (step * 61.0) % 760.0;
        let size = 18.0 + (step * 7.0) % 44.0;
        let shade = 0.15 + (step % 17.0) / 20.0;
        content.push_str(&format!(
            "{shade:.3} {:.3} {:.3} rg {x:.2} {y:.2} m {:.2} {:.2} l {:.2} {:.2} l h f\n",
            1.0 - shade,
            (step % 11.0) / 11.0,
            x + size * 0.93,
            y + size * 0.37,
            x + size * 0.41,
            y + size,
        ));
    }
    let mut builder = DocumentBuilder::new();
    builder.add_page(595.0, 842.0, |page| page.raw(content.as_bytes()));
    builder.finish()
}

/// One of the books committed under `crates/tinker-pdf/tests/epub/`, by its
/// file name.
pub fn committed_book(name: &str) -> Vec<u8> {
    let path = in_tree("tests/epub").join(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed book {}: {error}", path.display()))
}

/// One of the fixtures committed under `fuzz/corpus/render_page/`, by its
/// file name.
pub fn render_fixture(name: &str) -> Vec<u8> {
    let path = in_tree("../../fuzz/corpus/render_page").join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("the fixture {}: {error}", path.display()))
}

/// The phrase every dense page holds once, on a line of its own choosing:
/// what the text group's searches find.
pub const PHRASE: &str = "quick brown fox";

/// The word the whole-word, diacritic-insensitive search asks for, unaccented:
/// every dense page holds it accented, `crème`, once beside [`PHRASE`].
pub const UNACCENTED: &str = "creme";

/// The words a dense page is drawn from: about eighty distinct glyphs with
/// a few accented letters, each in `WinAnsiEncoding`, so the standard-14
/// pages [`producer`] writes draw the same text.
const WORDS: [&str; 64] = [
    "the", "of", "and", "a", "to", "in", "is", "that", "it", "was", "for", "on", "are", "as",
    "with", "his", "they", "at", "be", "this", "from", "have", "or", "by", "one", "had", "not",
    "but", "what", "all", "were", "when", "we", "there", "can", "an", "your", "which", "their",
    "said", "if", "do", "will", "each", "about", "how", "up", "out", "page", "line", "glyph",
    "reader", "order", "café", "déjà", "naïve", "façade", "über", "señor", "élan", "1984", "2026",
    "(see", "it)",
];

/// The 70 lines of one dense page, each about 80 characters, words drawn
/// from `words` and the page's sentence holding [`PHRASE`] on one of them.
fn dense_lines(words: &mut Seeded) -> Vec<String> {
    let at = words.below(70);
    (0..70)
        .map(|line| {
            let mut text = if line == at {
                format!("The {PHRASE} jumps over the crème brûlée;")
            } else {
                String::new()
            };
            let mut start = text.is_empty();
            while text.chars().count() < 78 {
                if !text.is_empty() {
                    text.push(' ');
                }
                let word = WORDS[words.below(WORDS.len())];
                if start {
                    let mut chars = word.chars();
                    if let Some(first) = chars.next() {
                        text.extend(first.to_uppercase());
                        text.push_str(chars.as_str());
                    }
                    start = false;
                } else {
                    text.push_str(word);
                }
                match words.below(12) {
                    0 => {
                        text.push('.');
                        start = true;
                    }
                    1 => text.push(','),
                    2 => text.push(';'),
                    _ => {}
                }
            }
            text
        })
        .collect()
}

/// `text` in `WinAnsiEncoding`: every character [`WORDS`] and [`PHRASE`]'s
/// line use is ASCII or Latin-1, which that encoding places at its own code.
fn win_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).expect("a Latin-1 character"))
        .collect()
}

/// The pages of [`dense`] and [`producer`], each its 70 lines.
fn dense_text(pages: usize, seed: u64) -> Vec<Vec<String>> {
    let mut words = Seeded::new(seed);
    (0..pages).map(|_| dense_lines(&mut words)).collect()
}

/// Liberation Serif Regular, the face [`dense`] embeds.
pub fn serif() -> Vec<u8> {
    liberation("LiberationSerif-Regular.ttf")
}

/// `pages` A4 pages of 70 lines each, about 5 600 characters a page, in
/// Liberation Serif Regular embedded whole, so subset-on-save has a program
/// to cut.
pub fn dense(pages: usize, seed: u64) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(
        builder.add_embedded_font(b"F0", b"LiberationSerif", &serif()),
        "the face is embedded"
    );
    for lines in dense_text(pages, seed) {
        builder.add_page(595.0, 842.0, |page| {
            for (at, line) in lines.iter().enumerate() {
                let y = 806.0 - 11.0 * at as f64;
                page.encoded_text(b"F0", 9.0, 36.0, y, (0.0, 0.0), &win_ansi(line), line);
            }
        });
    }
    builder.finish()
}

/// [`dense`]'s pages a `divisor`th of A4 each way, each holding what of its
/// page's text fits the same type there: its first `70 / divisor` lines,
/// each its words up to `78 / divisor` characters, at [`dense`]'s size and
/// leading, the margins a `divisor`th of its. A render of one draws about a
/// `divisor`²th of the glyphs and the pixels a dense page does.
pub fn dense_part(pages: usize, seed: u64, divisor: u32) -> Vec<u8> {
    let (width, height) = page_part(divisor);
    let lines = 70 / divisor as usize;
    let characters = 78 / divisor as usize;
    let margin = 36.0 / f64::from(divisor);
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(
        builder.add_embedded_font(b"F0", b"LiberationSerif", &serif()),
        "the face is embedded"
    );
    for text in dense_text(pages, seed) {
        builder.add_page(width, height, |page| {
            for (at, line) in text.iter().take(lines).enumerate() {
                let line = words_within(line, characters);
                let y = height - margin - 11.0 * at as f64;
                page.encoded_text(b"F0", 9.0, margin, y, (0.0, 0.0), &win_ansi(&line), &line);
            }
        });
    }
    builder.finish()
}

/// `line`'s words, as many from its start as `characters` holds, and its
/// first word whatever its length.
fn words_within(line: &str, characters: usize) -> String {
    let mut out = String::new();
    for word in line.split(' ') {
        let wanted = out.chars().count() + usize::from(!out.is_empty()) + word.chars().count();
        if !out.is_empty() && wanted > characters {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// [`dense`]'s text, seed 0, in standard-14 Helvetica, unembedded, as a
/// producer writes it: one `TJ` a line, its words kerned apart.
pub fn producer(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for lines in dense_text(pages, 0) {
        let mut content = Vec::with_capacity(70 * 120);
        for (at, line) in lines.iter().enumerate() {
            let y = 806 - 11 * at;
            content.extend_from_slice(format!("BT /F0 9 Tf 36 {y} Td [(").as_bytes());
            for (index, word) in line.split(' ').enumerate() {
                if index > 0 {
                    content.extend_from_slice(format!(") -{} (", 20 + index % 7).as_bytes());
                }
                for byte in win_ansi(word) {
                    if matches!(byte, b'(' | b')' | b'\\') {
                        content.push(b'\\');
                    }
                    content.push(byte);
                }
                content.push(b' ');
            }
            content.extend_from_slice(b")] TJ ET\n");
        }
        builder.add_page(595.0, 842.0, |page| page.raw(&content));
    }
    builder.finish()
}

/// `pages` pages of one Helvetica line each, the line [`three_pages`] writes
/// first on each of its pages, in the builder's flat page tree.
pub fn small(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for page in 0..pages {
        builder.add_page(595.0, 842.0, |p| {
            p.text(
                b"F0",
                11.0,
                72.0,
                780.0,
                &format!("Page {page}, line 0: the quick brown fox jumps over it."),
            );
        });
    }
    builder.finish()
}

/// The 60-page linearized fixture of `crates/tinker-pdf/tests/streaming_open.rs`
/// (`a_linearized_document`): pages of tens of kilobytes of filled paths,
/// saved linearized with object streams off.
pub fn linearized(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for index in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            let mut ops = Vec::with_capacity(24 * 1024);
            let shade = (index % 7) as f64 / 7.0;
            for step in 0..400 {
                let x = (step % 40) as f64 * 4.0;
                let y = (step / 40) as f64 * 6.0;
                ops.extend_from_slice(
                    format!(
                        "{shade:.3} 0.400 0.600 rg {x:.2} {y:.2} m {:.2} {:.2} l {:.2} {:.2} l h f ",
                        x + 3.5,
                        y + 5.5,
                        x + 1.25,
                        y + 2.75
                    )
                    .as_bytes(),
                );
            }
            page.raw(&ops);
        });
    }
    let base = Document::open(builder.finish()).expect("the pages open");
    base.editor().save(&tinker_pdf::WriteOptions {
        mode: tinker_pdf::WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        ..tinker_pdf::WriteOptions::default()
    })
}

/// At least 23 000 000 bytes of [`image_pages`]. Written once a process.
pub fn image_file() -> Arc<[u8]> {
    static FILE: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(FILE.get_or_init(|| image_pages(23_000_000).into()))
}

/// At least 2 400 000 bytes of [`image_pages`], its first page, about a
/// tenth of [`image_file`]: the file the operations take that [`image_file`]
/// holds outside the set's window. Written once a process.
pub fn small_image_file() -> Arc<[u8]> {
    static FILE: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(FILE.get_or_init(|| image_pages(2_400_000).into()))
}

/// At least `at_least` bytes of A4 pages, each covered by an image of its
/// own, `photo(2480, 3508, page)` coded as a baseline JPEG, so no two pages
/// share an image a writer could deduplicate.
pub fn image_pages(at_least: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    let mut held = 0usize;
    let mut page = 0u64;
    while held < at_least {
        let samples = photo(2480, 3508, page);
        let jpeg = tinker_pdf_filters::jpeg_encode(
            &tinker_pdf_filters::JpegSource {
                width: 2480,
                height: 3508,
                colour: tinker_pdf_filters::JpegSourceColour::Rgb,
                stride: 2480 * 3,
                data: &samples,
            },
            &tinker_pdf_filters::JpegOptions::default(),
        )
        .expect("a baseline JPEG");
        held += jpeg.len();
        let name = format!("Im{page}");
        assert!(builder.add_image(name.as_bytes(), &ImageData::Jpeg(&jpeg)));
        builder.add_page(595.0, 842.0, |p| {
            p.image(name.as_bytes(), 0.0, 0.0, 595.0, 842.0)
        });
        page += 1;
    }
    let file = builder.finish();
    assert!(
        file.len() >= at_least,
        "the image file is {} bytes",
        file.len()
    );
    file
}

/// [`image_file`] saved at R6 with both passwords empty and fixed entropy,
/// the file `tpdf encrypt` writes with neither password. Written once a
/// process.
pub fn aes_file() -> Arc<[u8]> {
    static FILE: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(FILE.get_or_init(|| {
        let plain = Document::open(image_file()).expect("the image file opens");
        let options = WriteOptions {
            encryption: Some(Encryption {
                user_password: String::new(),
                owner_password: String::new(),
                permissions: -4,
                entropy: [0x5A; 48],
            }),
            ..WriteOptions::default()
        };
        let file: Arc<[u8]> = plain.editor().save(&options).into();
        let doc = Document::open(Arc::clone(&file)).expect("the AES-256 file opens");
        doc.authenticate("")
            .expect("the AES-256 file authenticates with the empty password");
        assert_eq!(doc.page_count(), plain.page_count());
        file
    }))
}

/// The text of one column of a report page: `lines` lines of about 44
/// characters, drawn from [`WORDS`] as [`dense`]'s lines are.
fn column_lines(words: &mut Seeded, lines: usize) -> Vec<String> {
    (0..lines)
        .map(|_| {
            let mut text = String::new();
            while text.chars().count() < 44 {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(WORDS[words.below(WORDS.len())]);
            }
            text
        })
        .collect()
}

/// The cells of a report page's one table: a header row and four rows of
/// three cells, the figures by seed.
fn table_cells(words: &mut Seeded, page: usize) -> [[String; 3]; 5] {
    let mut row = |at: usize| {
        [
            format!("Item {}", page * 4 + at),
            format!("{}", words.below(9000) + 1000),
            format!("{}.{:02}", words.below(90) + 10, words.below(100)),
        ]
    };
    let rows = [row(1), row(2), row(3), row(4)];
    let [a, b, c, d] = rows;
    [
        ["Item".to_string(), "Units".to_string(), "Share".to_string()],
        a,
        b,
        c,
        d,
    ]
}

/// The operators that rule a table of `rows` rows of 18 pt and three
/// columns of `widths`, its top left corner at `(x, top)`: every row's and
/// every column's edge, so the table reads off its rules.
fn table_rules(x: f64, top: f64, widths: [f64; 3], rows: usize) -> Vec<u8> {
    let width: f64 = widths.iter().sum();
    let bottom = top - 18.0 * rows as f64;
    let mut rules = b"q 0.5 w\n".to_vec();
    for row in 0..=rows {
        let y = top - 18.0 * row as f64;
        rules.extend_from_slice(format!("{x} {y} m {} {y} l S\n", x + width).as_bytes());
    }
    let mut at = x;
    for edge in 0..=3 {
        rules.extend_from_slice(format!("{at} {top} m {at} {bottom} l S\n").as_bytes());
        if let Some(step) = widths.get(edge) {
            at += step;
        }
    }
    rules.extend_from_slice(b"Q");
    rules
}

/// One line of `text` in the embedded face `F0`.
fn line(page: &mut PageBuilder, size: f64, x: f64, y: f64, text: &str) {
    page.encoded_text(b"F0", size, x, y, (0.0, 0.0), &win_ansi(text), text);
}

/// One report page: a running head, two columns of 52 lines of body text,
/// one ruled table of a header and four rows below them, a running foot
/// and the page number, in the embedded face `F0`. A tagged page puts each
/// piece in its element — the head an `H1`, each column and the foot a
/// `P`, the table a `Table` — and an untagged one draws the same.
fn report_page(page: &mut PageBuilder, number: usize, words: &mut Seeded, tagged: bool) {
    let put = |page: &mut PageBuilder, tag: &[u8], draw: &dyn Fn(&mut PageBuilder)| {
        if tagged {
            page.tagged(tag, |page| draw(page));
        } else {
            draw(page);
        }
    };
    let head = format!("Annual report, section {number}");
    put(page, b"H1", &|page| line(page, 11.0, 50.0, 806.0, &head));
    for x in [50.0, 310.0] {
        let lines = column_lines(words, 52);
        put(page, b"P", &|page| {
            for (at, text) in lines.iter().enumerate() {
                line(page, 9.0, x, 780.0 - 11.0 * at as f64, text);
            }
        });
    }
    let cells = table_cells(words, number);
    let widths = [180.0, 120.0, 120.0];
    put(page, b"Table", &|page| {
        page.raw(&table_rules(80.0, 190.0, widths, cells.len()));
        for (row, cells) in cells.iter().enumerate() {
            let y = 190.0 - 18.0 * row as f64 - 13.0;
            let mut x = 84.0;
            for (cell, width) in cells.iter().zip(widths) {
                line(page, 9.0, x, y, cell);
                x += width;
            }
        }
    });
    let foot = format!("Bench report, page {number}");
    put(page, b"P", &|page| line(page, 8.0, 50.0, 30.0, &foot));
}

/// `pages` report pages, untagged: two columns of body text in Liberation
/// Serif Regular embedded, a running head and foot, a page number and one
/// ruled table each.
pub fn report(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_embedded_font(b"F0", b"LiberationSerif", &serif()),
        "the face is embedded"
    );
    let mut words = Seeded::new(7);
    for number in 1..=pages {
        builder.add_page(595.0, 842.0, |page| {
            report_page(page, number, &mut words, false)
        });
    }
    builder.finish()
}

/// The CMYK output profile the fuzz corpus commits, which the claiming
/// pages name as their destination.
fn cmyk_profile() -> Vec<u8> {
    let path = in_tree("../../fuzz/corpus/icc_profile/cmyk-lut.icc");
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed profile {}: {error}", path.display()))
}

/// [`report`]'s pages tagged under PDF/A-2b, and claiming PDF/UA-1 and
/// PDF/X-1a:2003 beside it, so each validator runs its rules rather than
/// reporting no claim.
///
/// The archival builder writes the PDF/A identification and its
/// `GTS_PDFA1` intent over `cmyk-lut.icc`; the editor then adds what no
/// profile writes: a `GTS_PDFX` intent naming the same destination, and
/// `pdfuaid:part` in the metadata packet.
pub fn claiming(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::archival(ArchivalProfile {
        part: ArchivalPart::Two,
        level: Some(ArchivalLevel::B),
        destination_profile: cmyk_profile(),
        destination_space: DeviceSpace::Cmyk,
        output_condition: "Custom".to_string(),
        language: Some("en".to_string()),
    });
    assert!(
        builder.add_embedded_font(b"F0", b"LiberationSerif", &serif()),
        "the face is embedded"
    );
    assert!(builder.set_info(b"Title", "Bench report"));
    assert!(builder.set_info(b"GTS_PDFXVersion", "PDF/X-1a:2003"));
    let mut words = Seeded::new(7);
    for number in 1..=pages {
        builder.add_page(595.0, 842.0, |page| {
            report_page(page, number, &mut words, true)
        });
    }
    let archived = builder
        .finish_archival()
        .unwrap_or_else(|refusal| panic!("the archival profile refused it: {refusal:?}"));

    let doc = Document::open(archived).expect("the archived file opens");
    let mut editor = doc.editor();
    let root = doc
        .cos()
        .trailer()
        .get_ref(editor.intern(b"Root"))
        .expect("a catalog");
    let Some(Object::Dict(mut catalog)) = editor.get(root) else {
        panic!("the catalog is a dictionary");
    };
    let key = editor.intern(b"OutputIntents");
    let Some(Object::Array(mut intents)) = catalog.get(key).cloned() else {
        panic!("the archival builder writes an output intent");
    };
    let first = match intents.first() {
        Some(Object::Ref(at)) => editor.get(*at),
        other => other.cloned(),
    };
    let Some(Object::Dict(mut pdfx)) = first else {
        panic!("the output intent is a dictionary");
    };
    pdfx.insert(
        editor.intern(b"S"),
        Object::Name(editor.intern(b"GTS_PDFX")),
    );
    intents.push(Object::Dict(pdfx));
    catalog.insert(key, Object::Array(intents));
    editor.put(root, Object::Dict(catalog));

    let packet = String::from_utf8(doc.xmp_metadata().expect("the archival packet"))
        .expect("the packet is UTF-8");
    let claimed = packet.replacen(
        "<rdf:Description ",
        "<rdf:Description xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\" \
         pdfuaid:part=\"1\" ",
        1,
    );
    assert_ne!(claimed, packet, "the packet has a description to claim in");
    // The packet is not checked against `/Info`, whose `GTS_PDFXVersion`
    // it has no property for, which is what the sync answer says.
    let _unchecked = editor
        .set_xmp_metadata(claimed.as_bytes())
        .expect("the catalog takes the packet");
    editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    })
}

/// `base` with one 100 by 100 pt square annotation on its first page, its
/// appearance synthesized, saved as a rewrite.
pub fn annotated(base: &[u8]) -> Vec<u8> {
    let doc = Document::open(base.to_vec()).expect("the base opens");
    let mut editor = doc.editor();
    let square = annot::square(
        editor.document(),
        Rect {
            x0: 100.0,
            y0: 600.0,
            x1: 200.0,
            y1: 700.0,
        },
        Color {
            r: 0.8,
            g: 0.1,
            b: 0.1,
        },
        2.0,
    );
    editor
        .add_annotation(0, square)
        .expect("the first page takes the annotation");
    editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    })
}

/// The normal appearance stream of the first annotation on `doc`'s first
/// page, which [`annotated`] gave its square.
pub fn first_appearance(doc: &Document) -> ObjRef {
    let editor = doc.editor();
    let dict = |object: Option<Object>| match object {
        Some(Object::Dict(dict)) => dict,
        Some(Object::Ref(at)) => match editor.get(at) {
            Some(Object::Dict(dict)) => dict,
            _ => panic!("a dictionary"),
        },
        _ => panic!("a dictionary"),
    };
    let page = dict(editor.get(editor.page_refs()[0]));
    let annots = match page.get(editor.intern(b"Annots")) {
        Some(Object::Array(annots)) => annots.clone(),
        Some(Object::Ref(at)) => match editor.get(*at) {
            Some(Object::Array(annots)) => annots,
            _ => panic!("an array of annotations"),
        },
        _ => panic!("the first page has annotations"),
    };
    let square = dict(annots.first().cloned());
    let appearance = dict(square.get(editor.intern(b"AP")).cloned());
    appearance
        .get_ref(editor.intern(b"N"))
        .expect("a normal appearance stream")
}

/// A replacement for [`annotated`]'s square's appearance: its 100 by 100 pt
/// box filled in `grey`, named in `doc`'s own name table.
pub fn square_appearance(doc: &Document, grey: f64) -> StreamData {
    let names = doc.cos();
    let mut dict = Dict::new();
    dict.insert(
        names.intern(b"Type"),
        Object::Name(names.intern(b"XObject")),
    );
    dict.insert(
        names.intern(b"Subtype"),
        Object::Name(names.intern(b"Form")),
    );
    dict.insert(
        names.intern(b"BBox"),
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Int(100),
            Object::Int(100),
        ]),
    );
    StreamData {
        dict,
        data: format!("{grey} g 0 0 100 100 re f").into_bytes(),
    }
}

/// `count` filled shapes at fractional coordinates, as [`triangles`] draws
/// them: a triangle and a closed cubic curve in turn, each its own colour.
fn filled(count: usize) -> String {
    let mut content = String::with_capacity(count * 96);
    for index in 0..count {
        let step = index as f64;
        let x = 20.0 + (step * 37.0) % 520.0;
        let y = 20.0 + (step * 61.0) % 760.0;
        let size = 18.0 + (step * 7.0) % 44.0;
        let shade = 0.15 + (step % 17.0) / 20.0;
        content.push_str(&format!(
            "{shade:.3} {:.3} {:.3} rg {x:.2} {y:.2} m ",
            1.0 - shade,
            (step % 11.0) / 11.0
        ));
        if index % 2 == 0 {
            content.push_str(&format!(
                "{:.2} {:.2} l {:.2} {:.2} l h f\n",
                x + size * 0.93,
                y + size * 0.37,
                x + size * 0.41,
                y + size,
            ));
        } else {
            content.push_str(&format!(
                "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {x:.2} {y:.2} c h f\n",
                x + size * 0.81,
                y - size * 0.23,
                x + size * 1.07,
                y + size * 0.69,
                x + size * 0.47,
                y + size * 0.93,
                x - size * 0.19,
                y + size * 1.11,
                x - size * 0.31,
                y + size * 0.29,
            ));
        }
    }
    content
}

/// `count` stroked cubic curves, 0.5 to 4 pt wide, through each of the nine
/// pairs of line cap and line join in turn, every fifth one dashed (8.4.3).
fn stroked(count: usize) -> String {
    let mut content = String::with_capacity(count * 128);
    for index in 0..count {
        let step = index as f64;
        let x = 30.0 + (step * 53.0) % 500.0;
        let y = 30.0 + (step * 29.0) % 760.0;
        let reach = 24.0 + (step * 13.0) % 60.0;
        let width = 0.5 * (1 + index % 8) as f64;
        let (cap, join) = ((index % 9) / 3, index % 3);
        let dash = if index % 5 == 0 { "[6 3] 0" } else { "[] 0" };
        content.push_str(&format!(
            "{width:.1} w {cap} J {join} j {dash} d {:.3} {:.3} {:.3} RG \
             {x:.2} {y:.2} m {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c S\n",
            (step % 13.0) / 13.0,
            0.2 + (step % 7.0) / 9.0,
            1.0 - (step % 5.0) / 6.0,
            x + reach * 0.31,
            y + reach * 0.97,
            x + reach * 0.83,
            y - reach * 0.41,
            x + reach,
            y + reach * 0.53,
        ));
    }
    content
}

/// One A4 page of raw content.
fn raw_page(content: &str) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(595.0, 842.0, |page| page.raw(content.as_bytes()));
    builder.finish()
}

/// `count` paths over one A4 page: three fifths filled polygons and closed
/// curves, as [`triangles`] draws them, and two fifths stroked cubic curves,
/// as [`curves`] draws them.
pub fn paths(count: usize) -> Vec<u8> {
    let curves = count * 2 / 5;
    raw_page(&(filled(count - curves) + &stroked(curves)))
}

/// `count` stroked cubic curves alone over one A4 page, 0.5 to 4 pt wide,
/// each cap and join in turn and every fifth dashed: [`paths`]'s curves.
pub fn curves(count: usize) -> Vec<u8> {
    raw_page(&stroked(count))
}

/// One tiling pattern filling an A4 page, in one rectangle or in `fills`
/// strips that together cover what the one rectangle covers.
pub fn tiling(fills: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    let cell = b"0.9 0.3 0.1 rg 2 2 12 12 re f 0.1 0.4 0.8 rg 10 10 m 18 10 l 14 18 l h f";
    assert!(builder.add_tiling_pattern(
        b"P0",
        &TilingPattern {
            bbox: [0.0, 0.0, 20.0, 20.0],
            x_step: 20.0,
            y_step: 20.0,
            matrix: None,
            tiling_type: TilingType::ConstantSpacing,
            content: cell,
        }
    ));
    builder.add_page(595.0, 842.0, |page| {
        assert!(page.set_fill_pattern(b"P0"));
        let strip = 842.0 / fills as f64;
        let mut rectangles = String::new();
        for at in 0..fills {
            rectangles.push_str(&format!("0 {:.4} 595 {strip:.4} re f\n", strip * at as f64));
        }
        page.raw(rectangles.as_bytes());
    });
    builder.finish()
}

/// The width and height of the pages the decode group and the image pages
/// decode, in pixels: A4 at 300 dpi, as a scan is made.
pub const SCAN: (u32, u32) = (2480, 3508);

/// [`PAGE_BYTES`] seeded bytes that deflate to between 90 % and 100 % of
/// their length: literals drawn from the 144 the fixed code writes in eight
/// bits, with a four-byte repeat every 32 bytes, so inflate decodes coded
/// literals and a few matches rather than stored blocks.
pub fn noise() -> Vec<u8> {
    let mut seed = Seeded::new(13);
    let mut out = Vec::with_capacity(PAGE_BYTES);
    while out.len() < PAGE_BYTES {
        if out.len() % 32 == 28 && out.len() >= 64 {
            let from = out.len() - 64;
            for at in from..from + 4 {
                out.push(out[at]);
            }
        } else {
            out.push((seed.next() % 144) as u8);
        }
    }
    out.truncate(PAGE_BYTES);
    out
}

/// `bytes` coded as 7.4.4's LZW: 9- to 12-bit codes, `/EarlyChange 1`, a
/// clear code first and again before the table passes 4 096 entries, and
/// the end-of-data code last. The tree writes no LZW of its own, so this is
/// the one, round-tripped through `lzw_decode` by its caller.
pub fn lzw(bytes: &[u8]) -> Vec<u8> {
    const CLEAR: u32 = 256;
    const END: u32 = 257;
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let (mut held, mut held_bits) = (0u64, 0u32);
    let mut put = |code: u32, width: u32, out: &mut Vec<u8>| {
        held = held << width | u64::from(code);
        held_bits += width;
        while held_bits >= 8 {
            held_bits -= 8;
            out.push((held >> held_bits) as u8);
        }
    };
    let mut table: std::collections::HashMap<(u32, u8), u32> = std::collections::HashMap::new();
    let (mut next, mut width) = (258u32, 9u32);
    put(CLEAR, width, &mut out);
    let mut iter = bytes.iter();
    let Some(&first) = iter.next() else {
        put(END, width, &mut out);
        if held_bits > 0 {
            out.push((held << (8 - held_bits)) as u8);
        }
        return out;
    };
    let mut string = u32::from(first);
    for &byte in iter {
        if let Some(&code) = table.get(&(string, byte)) {
            string = code;
            continue;
        }
        put(string, width, &mut out);
        table.insert((string, byte), next);
        next += 1;
        // The reader adds each entry one code behind the writer, and with
        // `/EarlyChange 1` widens when its next entry would reach the width's
        // last code, which is when this side's count reaches it.
        if next >= 1 << width && width < 12 {
            width += 1;
        }
        if next >= 4094 {
            put(CLEAR, width, &mut out);
            table.clear();
            next = 258;
            width = 9;
        }
        string = u32::from(byte);
    }
    put(string, width, &mut out);
    put(END, width, &mut out);
    if held_bits > 0 {
        out.push((held << (8 - held_bits)) as u8);
    }
    out
}

/// [`photo`] at [`SCAN`]'s size, seed 0, as a baseline JPEG under Annex K's
/// tables. Written once a process.
pub fn baseline_jpeg() -> Arc<[u8]> {
    static JPEG: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(JPEG.get_or_init(|| {
        let samples = photo(SCAN.0, SCAN.1, 0);
        let jpeg = tinker_pdf_filters::jpeg_encode(
            &tinker_pdf_filters::JpegSource {
                width: SCAN.0,
                height: SCAN.1,
                colour: tinker_pdf_filters::JpegSourceColour::Rgb,
                stride: SCAN.0 as usize * 3,
                data: &samples,
            },
            &tinker_pdf_filters::JpegOptions::default(),
        )
        .expect("a baseline JPEG");
        // The JPEG page `tests/peak_memory.rs` holds the peak of is made of
        // these bytes: a changed encoder fails here, before it moves a peak.
        assert_eq!(
            hex(&sha256(&jpeg)),
            "754c262d187b8459117b897f5d79c97d71062c58ed6ff494b82438d8ed8dcbbc",
            "baseline_jpeg() is the peak test's JPEG"
        );
        jpeg.into()
    }))
}

/// [`SCAN`]'s size a `divisor`th each way: the image of a page
/// [`page_part`] makes at a scan's resolution.
pub fn scan_part(divisor: u32) -> (u32, u32) {
    (SCAN.0 / divisor, SCAN.1 / divisor)
}

/// [`photo`] at [`scan_part`]'s size, seed 0, as a baseline JPEG under Annex
/// K's tables. At a divisor of 2 it is the source
/// `data/progressive-1240x1754.jpg` was transcoded from, as `data/README.md`
/// records it, and is held to that digest: a changed encoder fails here,
/// before the decode group compares the two.
pub fn baseline_jpeg_part(divisor: u32) -> Vec<u8> {
    let (width, height) = scan_part(divisor);
    let samples = photo(width, height, 0);
    let jpeg = tinker_pdf_filters::jpeg_encode(
        &tinker_pdf_filters::JpegSource {
            width,
            height,
            colour: tinker_pdf_filters::JpegSourceColour::Rgb,
            stride: width as usize * 3,
            data: &samples,
        },
        &tinker_pdf_filters::JpegOptions::default(),
    )
    .expect("a baseline JPEG");
    if divisor == 2 {
        assert_eq!(
            hex(&sha256(&jpeg)),
            "2dd7f148015119516d79e4ea8b97096f99d20fd3a848e361abc895248ee2475f",
            "baseline_jpeg_part(2) is the progressive file's source"
        );
    }
    jpeg
}

/// One of the codestreams committed under `benches/data/`, by its name.
pub fn committed_data(name: &str) -> Vec<u8> {
    let path = in_tree("benches/data").join(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed file {}: {error}", path.display()))
}

/// One of the documents committed under the repository's `testdata/`, by
/// its file name.
pub fn testdata(name: &str) -> Vec<u8> {
    let path = in_tree("../../testdata").join(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed document {}: {error}", path.display()))
}

/// A scan: the dense text page rendered at 300 dpi and thresholded to one
/// bit a pixel, rows packed from the most significant bit, **0 for black**
/// as a one-bit `/DeviceGray` sample is. Its width, height and bytes.
pub fn scan() -> (u32, u32, Arc<[u8]>) {
    static SCAN_PAGE: OnceLock<(u32, u32, Arc<[u8]>)> = OnceLock::new();
    let (width, height, bytes) = SCAN_PAGE.get_or_init(|| scan_at(300.0));
    (*width, *height, Arc::clone(bytes))
}

/// [`scan`] at `dpi`: the dense text page rendered there and thresholded the
/// same way.
pub fn scan_at(dpi: f64) -> (u32, u32, Arc<[u8]>) {
    let doc = Document::open(dense(1, 0)).expect("the text page opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(dpi));
    let stride = bitmap.width.div_ceil(8) as usize;
    let n = bitmap.components();
    let mut bits = vec![0xFFu8; stride * bitmap.height as usize];
    for (y, row) in bitmap.data.chunks(bitmap.stride).enumerate() {
        for (x, pixel) in row.chunks(n).take(bitmap.width as usize).enumerate() {
            if pixel.iter().take(3).map(|&v| u32::from(v)).sum::<u32>() < 3 * 128 {
                bits[y * stride + x / 8] &= !(0x80 >> (x % 8));
            }
        }
    }
    (bitmap.width, bitmap.height, bits.into())
}

/// [`scan`] coded as ITU-T T.6, as `/CCITTFaxDecode /K -1` reads it.
pub fn g4(width: u32, height: u32, bits: &[u8]) -> Vec<u8> {
    tinker_pdf_filters::ccitt_g4_encode(&tinker_pdf_filters::CcittSource {
        columns: width,
        rows: height,
        black_is_1: false,
        stride: width.div_ceil(8) as usize,
        end_of_block: true,
        data: bits,
    })
    .expect("a G4 page")
}

/// [`scan`] as a T.88 immediate lossless generic region, in the embedded
/// organisation `/JBIG2Decode` reads (Annex D.3): a page information
/// segment, the region and an end of page, as the image pass writes them.
pub fn jbig2(width: u32, height: u32, bits: &[u8]) -> Vec<u8> {
    // T.88 codes 1 for black.
    let inverted: Vec<u8> = bits.iter().map(|b| !b).collect();
    let region = tinker_pdf_filters::jbig2_generic_region_segment(
        &tinker_pdf_filters::Jbig2GenericSource {
            width,
            height,
            template: 0,
            tpgdon: true,
            at: [(3, -1), (-3, -1), (2, -2), (-2, -2)],
            stride: width.div_ceil(8) as usize,
            data: &inverted,
        },
        0,
        0,
        0,
    )
    .expect("a generic region");
    let segment = |number: u32, kind: u8, data: &[u8]| -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() + 11);
        out.extend_from_slice(&number.to_be_bytes());
        out.push(kind & 0x3F);
        out.push(0);
        out.push(1);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        out
    };
    let mut page = Vec::with_capacity(19);
    page.extend_from_slice(&width.to_be_bytes());
    page.extend_from_slice(&height.to_be_bytes());
    page.extend_from_slice(&[0; 8]);
    page.push(0x01);
    page.extend_from_slice(&[0, 0]);
    let mut out = segment(0, 48, &page);
    out.extend_from_slice(&segment(1, 39, &region));
    out.extend_from_slice(&segment(2, 49, &[]));
    out
}

/// An A4 page covered by one image: `image` placed as `Im0` over the page.
fn image_page(image: &ImageData<'_>) -> Vec<u8> {
    image_page_part(image, 1)
}

/// [`image_page`] on a page a `divisor`th of A4 each way ([`page_part`]).
fn image_page_part(image: &ImageData<'_>, divisor: u32) -> Vec<u8> {
    let (width, height) = page_part(divisor);
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", image), "the image is placed");
    builder.add_page(width, height, |page| {
        page.image(b"Im0", 0.0, 0.0, width, height)
    });
    builder.finish()
}

/// An A4 page covered by [`baseline_jpeg`].
pub fn jpeg_page() -> Vec<u8> {
    image_page(&ImageData::Jpeg(&baseline_jpeg()))
}

/// A page a `divisor`th of A4 each way covered by [`baseline_jpeg_part`].
pub fn jpeg_page_part(divisor: u32) -> Vec<u8> {
    image_page_part(&ImageData::Jpeg(&baseline_jpeg_part(divisor)), divisor)
}

/// A page a `divisor`th of A4 each way covered by [`photo`] at
/// [`scan_part`]'s size, seed 1, Flate-coded.
pub fn flate_page(divisor: u32) -> Vec<u8> {
    let (width, height) = scan_part(divisor);
    let coded = tinker_pdf_filters::zlib_compress(&photo(width, height, 1));
    image_page_part(
        &ImageData::Compressed(CompressedImage {
            width,
            height,
            bits_per_component: 8,
            color_space: ImageColorSpace::DeviceRgb,
            filter: Some(ImageFilter::Flate),
            data: &coded,
            color_key_mask: None,
            soft_mask: None,
        }),
        divisor,
    )
}

/// A page a `divisor`th of A4 each way covered by [`photo`] at
/// [`scan_part`]'s size, seed 2, its rows run through PNG's five filters in
/// turn — None, Sub, Up, Average and Paeth, each row tagged with its own
/// (PNG 9.2) — and Flate-coded under predictor 15 (7.4.4.4), so a decode
/// unfilters every kind of row.
pub fn predicted_page(divisor: u32) -> Vec<u8> {
    let (width, height) = scan_part(divisor);
    let rows = png_filtered(&photo(width, height, 2), width as usize * 3, 3);
    let coded = tinker_pdf_filters::zlib_compress(&rows);
    image_page_part(
        &ImageData::Compressed(CompressedImage {
            width,
            height,
            bits_per_component: 8,
            color_space: ImageColorSpace::DeviceRgb,
            filter: Some(ImageFilter::FlatePngPredictor {
                colors: 3,
                bits_per_component: 8,
                columns: width,
            }),
            data: &coded,
            color_key_mask: None,
            soft_mask: None,
        }),
        divisor,
    )
}

/// `samples`, rows of `stride` bytes at `bpp` bytes a pixel, each row
/// filtered by PNG 9.2's filter `row % 5` and prefixed with its type: the
/// difference, modulo 256, from nothing, the byte a pixel to the left (`a`),
/// the byte above (`b`), their mean rounded down, or Paeth's pick of `a`, `b`
/// and the byte above and to the left (`c`), each `0` off the image.
fn png_filtered(samples: &[u8], stride: usize, bpp: usize) -> Vec<u8> {
    let zeros = vec![0u8; stride];
    let mut out = Vec::with_capacity(samples.len() + samples.len() / stride.max(1));
    for (index, row) in samples.chunks(stride).enumerate() {
        let above = match index {
            0 => &zeros[..],
            _ => &samples[(index - 1) * stride..index * stride],
        };
        let kind = (index % 5) as u8;
        out.push(kind);
        for (at, &x) in row.iter().enumerate() {
            let a = if at >= bpp { row[at - bpp] } else { 0 };
            let b = above[at];
            let c = if at >= bpp { above[at - bpp] } else { 0 };
            let predicted = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                _ => {
                    let p = i16::from(a) + i16::from(b) - i16::from(c);
                    let (pa, pb, pc) = (
                        (p - i16::from(a)).abs(),
                        (p - i16::from(b)).abs(),
                        (p - i16::from(c)).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
            };
            out.push(x.wrapping_sub(predicted));
        }
    }
    out
}

/// A page a `divisor`th of A4 each way covered by [`photo`] at
/// [`scan_part`]'s size, seed 0, turned into ink by formula — black the
/// darkest of the three, each colour the rest of its complement —
/// Flate-coded under `/ICCBased` over `cmyk-lut.icc`.
pub fn cmyk_page(divisor: u32) -> Vec<u8> {
    let (width, height) = scan_part(divisor);
    let (page_width, page_height) = page_part(divisor);
    let rgb = photo(width, height, 0);
    let mut ink = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in rgb.chunks(3) {
        let (c, m, y) = (255 - pixel[0], 255 - pixel[1], 255 - pixel[2]);
        let k = c.min(m).min(y);
        ink.extend_from_slice(&[c - k, m - k, y - k, k]);
    }
    let coded = tinker_pdf_filters::zlib_compress(&ink);
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_icc_color_space(b"CS0", &cmyk_profile(), 4));
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Compressed(CompressedImage {
            width,
            height,
            bits_per_component: 8,
            color_space: ImageColorSpace::Icc {
                resource: b"CS0",
                components: 4,
            },
            filter: Some(ImageFilter::Flate),
            data: &coded,
            color_key_mask: None,
            soft_mask: None,
        })
    ));
    builder.add_page(page_width, page_height, |page| {
        page.image(b"Im0", 0.0, 0.0, page_width, page_height)
    });
    builder.finish()
}

/// The ICC profiles the tests write byte by byte, `tests/icc_support/`'s:
/// [`v5_page`] converts through its version 5 profile of multi-process
/// elements, the bytes `determinism.rs`'s version 5 page draws a picture
/// through. The planning baseline reads no version 5 profile and builds no
/// such page, so its build leaves the file out.
#[cfg(not(tinker_planning_baseline))]
#[path = "../../tests/icc_support/transforms.rs"]
mod icc_transforms;

/// A page a `divisor`th of A4 each way covered by [`photo`] at
/// [`scan_part`]'s size, seed 0 — [`cmyk_page`]'s samples before they are
/// turned into ink — Flate-coded under `/ICCBased` over a version 5 display
/// profile whose `A2B0` is multi-process elements: ICC.2's formula segments
/// and a single sampled curve, a matrix and an extended grid
/// (`icc_transforms::v5_elements_profile`, RC-20.10). With `profiled` false
/// the same samples are `/DeviceRGB`, as a picture whose profile the parser
/// refused would draw them.
#[cfg(not(tinker_planning_baseline))]
pub fn v5_page(divisor: u32, profiled: bool) -> Vec<u8> {
    let (width, height) = scan_part(divisor);
    let (page_width, page_height) = page_part(divisor);
    let coded = tinker_pdf_filters::zlib_compress(&photo(width, height, 0));
    let mut builder = DocumentBuilder::new();
    let color_space = if profiled {
        assert!(builder.add_icc_color_space(b"CS0", &icc_transforms::v5_elements_profile(), 3));
        ImageColorSpace::Icc {
            resource: b"CS0",
            components: 3,
        }
    } else {
        ImageColorSpace::DeviceRgb
    };
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Compressed(CompressedImage {
            width,
            height,
            bits_per_component: 8,
            color_space,
            filter: Some(ImageFilter::Flate),
            data: &coded,
            color_key_mask: None,
            soft_mask: None,
        })
    ));
    builder.add_page(page_width, page_height, |page| {
        page.image(b"Im0", 0.0, 0.0, page_width, page_height)
    });
    builder.finish()
}

/// The rendering intents Table 69 names, in [`intents_page`]'s column order.
pub const INTENTS: [&str; 4] = [
    "Perceptual",
    "RelativeColorimetric",
    "Saturation",
    "AbsoluteColorimetric",
];

/// [`intents_page`]'s profile: RGB into Lab, carrying a two-point `mft2`
/// table for each intent — `A2B0`, `A2B1` and `A2B2`, three different grids
/// (ICC.1 8.10.2) — and a media white other than D50, so ICC-absolute
/// colorimetry is a transform of its own (6.2.3) and each of the four intents
/// converts through its own. In integers throughout: each node's `L*` is a
/// whole number, encoded in v2's legacy 16-bit Lab as `L* × 652.8` rounded,
/// and its `a*` and `b*` as `(v + 128) × 256` exactly.
fn intents_profile() -> Vec<u8> {
    let identity = |out: &mut Vec<u8>| {
        for _ in 0..3 {
            out.extend_from_slice(&0u16.to_be_bytes());
            out.extend_from_slice(&u16::MAX.to_be_bytes());
        }
    };
    let table = |intent: i32| {
        let mut out = b"mft2".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[3, 3, 2, 0]);
        for row in 0..3 {
            for column in 0..3 {
                let value: u32 = if row == column { 0x0001_0000 } else { 0 };
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&2u16.to_be_bytes());
        identity(&mut out);
        for node in 0..8i32 {
            let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
            let l = 18 + 30 * i + 25 * j + 15 * k + 4 * intent;
            let a = -40 + 70 * j + 10 * k - 30 * intent;
            let b = -10 + 50 * k - 30 * i + 35 * intent;
            for value in [(l * 6528 + 5) / 10, (a + 128) * 256, (b + 128) * 256] {
                let value = u16::try_from(value).expect("a node within the encoding");
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        identity(&mut out);
        out
    };
    // 0.88, 0.91 and 0.72 in s15Fixed16: a warm paper, darker than D50.
    let mut white = b"XYZ ".to_vec();
    white.extend_from_slice(&[0; 4]);
    for value in [57_672u32, 59_638, 47_186] {
        white.extend_from_slice(&value.to_be_bytes());
    }
    let tags = [
        (*b"A2B0", table(0)),
        (*b"A2B1", table(1)),
        (*b"A2B2", table(2)),
        (*b"wtpt", white),
    ];
    let mut out = vec![0u8; 128];
    out[8..12].copy_from_slice(&0x0200_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(b"RGB ");
    out[20..24].copy_from_slice(b"Lab ");
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    let start = 132 + 12 * tags.len();
    let mut body = Vec::new();
    for (signature, data) in &tags {
        out.extend_from_slice(signature);
        out.extend_from_slice(&((start + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    let size = out.len() as u32;
    out[0..4].copy_from_slice(&size.to_be_bytes());
    out
}

/// RC-01's page at A4, for `render an ICC page under each rendering intent
/// at 150 dpi`: in each of four columns, under one of [`INTENTS`] by `ri`, a
/// fill along the top third, [`photo`] at [`scan_part`]`(8)`'s size along the
/// middle and an axial shading `sh` paints across the column along the
/// bottom, all in an `ICCBased` space over [`intents_profile`]. Each intent's
/// transform is compiled, the image decoded once in each, and every pixel of
/// each shading converted through its column's. Written as the bytes of the
/// file, which every revision reads alike.
pub fn intents_page() -> Vec<u8> {
    let (width, height) = scan_part(8);
    let samples = tinker_pdf_filters::zlib_compress(&photo(width, height, 0));
    let mut content = String::new();
    for (column, intent) in INTENTS.iter().enumerate() {
        let x = 148.75 * column as f64;
        content.push_str(&format!(
            "q /{intent} ri\n\
             /L cs 0.25 0.5 0.75 scn {x} 561 148.75 281 re f\n\
             q 148.75 0 0 281 {x} 280 cm /Im0 Do Q\n\
             q {x} 0 148.75 280 re W n 1 0 0 1 {x} 0 cm /Sh0 sh Q\n\
             Q\n"
        ));
    }
    let stream = |entries: &str, data: &[u8]| {
        let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    };
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842]\n   \
          /Resources << /ColorSpace << /L [/ICCBased 5 0 R] >>\n     \
          /XObject << /Im0 6 0 R >> /Shading << /Sh0 7 0 R >> >>\n   \
          /Contents 4 0 R >>"
            .to_vec(),
        stream("", content.as_bytes()),
        stream("/N 3", &intents_profile()),
        stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width {width} /Height {height} \
                 /ColorSpace [/ICCBased 5 0 R] /BitsPerComponent 8 /Filter /FlateDecode"
            ),
            &samples,
        ),
        b"<< /ShadingType 2 /ColorSpace [/ICCBased 5 0 R] /Coords [0 0 148.75 0]\n   \
          /Function << /FunctionType 2 /Domain [0 1] /C0 [0 1 0] /C1 [1 0 1] /N 1 >>\n   \
          /Extend [true true] >>"
            .to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The characters [`cjk_page`] sets, in turn: the hiragana and katakana
/// syllables and the commonest kanji, each a character JIS X 0208 holds, so
/// Adobe-Japan1 reaches it through `UniJIS-UTF16-H` and Source Han Sans JP
/// draws it.
const CJK_TEXT: &str = "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをん\
     アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン\
     日本語文字中国人大小山川田上下左右月火水木金土年時分前後東西南北学校先生名今何見行来出入言話読書食飲";

/// The lines [`cjk_page`] sets, and the characters a line.
const CJK_LINES: usize = 50;
/// See [`CJK_LINES`].
const CJK_COLUMNS: usize = 38;

/// An A4 page of unembedded CJK text, for the render group's two
/// bundled-face operations (FT-01.11): [`CJK_LINES`] lines of
/// [`CJK_COLUMNS`] characters of [`CJK_TEXT`] at 12 pt, in an Adobe-Japan1
/// Type 0 font under `UniJIS-UTF16-H` whose descendant embeds no program and
/// whose descriptor is symbolic — the shape of a CJK document that leaves its
/// font to the reader — so a `bundled-fonts` build draws every glyph through
/// Source Han Sans JP, read by character. Written as the bytes of the file,
/// which every revision reads alike.
pub fn cjk_page() -> Vec<u8> {
    let characters: Vec<char> = CJK_TEXT.chars().filter(|c| !c.is_whitespace()).collect();
    let mut content = String::from("BT /F0 12 Tf 14.4 TL 70 792 Td\n");
    for line in 0..CJK_LINES {
        content.push('<');
        for column in 0..CJK_COLUMNS {
            let c = characters[(line * CJK_COLUMNS + column) % characters.len()];
            content.push_str(&format!("{:04X}", u32::from(c)));
        }
        content.push_str("> Tj T*\n");
    }
    content.push_str("ET\n");
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842]\n   \
          /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        {
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(content.as_bytes());
            stream.extend_from_slice(b"\nendstream");
            stream
        },
        b"<< /Type /Font /Subtype /Type0 /BaseFont /KozGoPr6N-Regular\n   \
          /Encoding /UniJIS-UTF16-H /DescendantFonts [6 0 R] >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /KozGoPr6N-Regular\n   \
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 6 >>\n   \
          /FontDescriptor 7 0 R /DW 1000 >>"
            .to_vec(),
        b"<< /Type /FontDescriptor /FontName /KozGoPr6N-Regular /Flags 4\n   \
          /FontBBox [0 -120 1000 880] /ItalicAngle 0 /Ascent 880 /Descent -120\n   \
          /CapHeight 740 /StemV 80 >>"
            .to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A page a quarter of A4 each way covered by the committed 620 × 877
/// irreversible JPEG 2000 codestream.
pub fn jpx_page_part() -> Vec<u8> {
    let codestream = committed_data("irreversible-620x877.jp2");
    let (width, height) = scan_part(4);
    image_page_part(
        &ImageData::Compressed(CompressedImage {
            width,
            height,
            // Neither is written for a codestream, which states both itself.
            bits_per_component: 8,
            color_space: ImageColorSpace::DeviceRgb,
            filter: Some(ImageFilter::Jpx),
            data: &codestream,
            color_key_mask: None,
            soft_mask: None,
        }),
        4,
    )
}

/// An A4 page covered by the committed irreversible JPEG 2000 codestream.
pub fn jpx_page() -> Vec<u8> {
    let codestream = committed_data("irreversible-2480x3508.jp2");
    image_page(&ImageData::Compressed(CompressedImage {
        width: SCAN.0,
        height: SCAN.1,
        // Neither is written for a codestream, which states both itself.
        bits_per_component: 8,
        color_space: ImageColorSpace::DeviceRgb,
        filter: Some(ImageFilter::Jpx),
        data: &codestream,
        color_key_mask: None,
        soft_mask: None,
    }))
}

/// A page a `divisor`th of A4 each way covered by [`scan_at`] a `divisor`th
/// of a scan's resolution, G4-coded.
pub fn g4_page(divisor: u32) -> Vec<u8> {
    let (width, height, bits) = scan_at(300.0 / f64::from(divisor));
    let coded = g4(width, height, &bits);
    image_page_part(
        &ImageData::Compressed(CompressedImage {
            width,
            height,
            bits_per_component: 1,
            color_space: ImageColorSpace::DeviceGray,
            filter: Some(ImageFilter::CcittFax(CcittParams {
                k: -1,
                columns: width,
                rows: height,
                ..CcittParams::default()
            })),
            data: &coded,
            color_key_mask: None,
            soft_mask: None,
        }),
        divisor,
    )
}

/// A page a `divisor`th of A4 each way covered by [`scan_at`] a `divisor`th
/// of a scan's resolution, saved through the image pass's JBIG2 generic
/// coding, the builder having no JBIG2 filter of its own.
///
/// The scan is placed as raw one-bit samples rather than as [`g4_page`]'s G4,
/// because the pass keeps a new coding only when it is smaller than the
/// stored one, and a generic region need not beat G4.
pub fn jbig2_page(divisor: u32) -> Vec<u8> {
    let (width, height, bits) = scan_at(300.0 / f64::from(divisor));
    let raw = image_page_part(
        &ImageData::Compressed(CompressedImage {
            width,
            height,
            bits_per_component: 1,
            color_space: ImageColorSpace::DeviceGray,
            filter: None,
            data: &bits,
            color_key_mask: None,
            soft_mask: None,
        }),
        divisor,
    );
    let doc = Document::open(raw).expect("the raw scan page opens");
    let options = SaveOptions {
        write: WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        },
        images: ImagePolicy::Recode(ImageRecoding::new(
            ContinuousCodec::Keep,
            BilevelCodec::Jbig2Generic,
        )),
        ..SaveOptions::default()
    };
    write::save(&mut doc.editor(), &options).bytes
}

/// A page holding one line of Helvetica, which renders to paper and little
/// else.
pub fn near_blank() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(595.0, 842.0, |page| {
        page.text(b"F0", 11.0, 72.0, 780.0, "A page with one line on it.");
    });
    builder.finish()
}

/// A 2048-bit RSA key, big-endian: `pss-vect.txt`'s Example 10, from the
/// copy `tinker-pdf-crypto`'s own tests read
/// (`crates/tinker-pdf-crypto/tests/data/pkcs1/pss-vect.txt`).
pub struct RsaKey {
    /// `n`.
    pub modulus: Vec<u8>,
    /// `e`.
    pub public_exponent: Vec<u8>,
    /// `d`.
    pub private_exponent: Vec<u8>,
}

/// `pss-vect.txt`, as the crypto crate's tests commit it.
pub fn pss_vectors() -> String {
    let path = in_tree("../tinker-pdf-crypto/tests/data/pkcs1/pss-vect.txt");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the committed vectors {}: {error}", path.display()))
}

/// Example 10's key, read from its block of `vectors`, [`pss_vectors`]'s
/// text: its private-key section, where `Modulus` is `n`, `Public exponent`
/// is `e` and `Exponent` is `d` — the label the public-key section above it
/// gives `e`.
pub fn example_10(vectors: &str) -> RsaKey {
    let block = vectors.split("# Example 10:").nth(1).expect("Example 10");
    let private = block
        .split("# Private key")
        .nth(1)
        .expect("its private key");
    let field = |label: &str| -> Vec<u8> {
        let after = private
            .split(&format!("# {label}:"))
            .nth(1)
            .unwrap_or_else(|| panic!("Example 10's {label}"));
        let digits: String = after
            .lines()
            .skip(1)
            .take_while(|line| !line.trim_start().starts_with('#'))
            .flat_map(str::split_whitespace)
            .collect();
        (0..digits.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).expect("hex octets"))
            .collect()
    };
    RsaKey {
        modulus: field("Modulus"),
        public_exponent: field("Public exponent"),
        private_exponent: field("Exponent"),
    }
}

/// The bytes [`message`] signs: 10 MB.
pub const MESSAGE_BYTES: usize = 10_000_000;

/// [`MESSAGE_BYTES`] seeded bytes.
pub fn message() -> Vec<u8> {
    let mut seed = Seeded::new(17);
    let mut bytes = Vec::with_capacity(MESSAGE_BYTES + 8);
    while bytes.len() < MESSAGE_BYTES {
        bytes.extend_from_slice(&seed.next().to_le_bytes());
    }
    bytes.truncate(MESSAGE_BYTES);
    bytes
}

/// `message`'s RSA-2048 PKCS #1 v1.5 signature under `key` with SHA-256,
/// made here: the crypto crate has no signing path, so the signature is
/// [`pkcs1_encoded`]'s representative through [`rsa_private`].
pub fn signature(key: &RsaKey, message: &[u8]) -> Vec<u8> {
    rsa_private(key, &pkcs1_encoded(key, message))
}

/// RFC 8017 §9.2's EMSA-PKCS1-v1_5 encoding of `message`'s SHA-256 digest at
/// the key's length: `0x00 0x01`, `0xff` padding, `0x00`, then the digest's
/// `DigestInfo`.
pub fn pkcs1_encoded(key: &RsaKey, message: &[u8]) -> Vec<u8> {
    let prefix = DigestAlgorithm::Sha256.digest_info_prefix();
    let digest = sha256(message);
    let length = key.modulus.len();
    let at = length - prefix.len() - digest.len();
    let mut encoded = vec![0xff; length];
    encoded[0] = 0x00;
    encoded[1] = 0x01;
    encoded[at - 1] = 0x00;
    encoded[at..at + prefix.len()].copy_from_slice(prefix);
    encoded[at + prefix.len()..].copy_from_slice(&digest);
    encoded
}

/// The key's modulus, its private exponent and `representative` as the
/// crypto crate's integers: what the private-key operation, `Modulus::pow`
/// with `d`, takes.
pub fn private_operands(
    key: &RsaKey,
    representative: &[u8],
) -> (Modulus<RSA_LIMBS>, Uint<RSA_LIMBS>, Uint<RSA_LIMBS>) {
    let n = Uint::from_be_bytes(&key.modulus).expect("n fits");
    let modulus = Modulus::new(n).expect("n is an odd modulus");
    let d = Uint::from_be_bytes(&key.private_exponent).expect("d fits");
    let m = Uint::from_be_bytes(representative).expect("the representative fits");
    (modulus, d, m)
}

/// `representative ^ d mod n`, written out at the modulus's length: the
/// private-key operation, through the one modular power the crypto crate has.
pub fn rsa_private(key: &RsaKey, representative: &[u8]) -> Vec<u8> {
    let (modulus, d, m) = private_operands(key, representative);
    let mut out = vec![0u8; key.modulus.len()];
    assert!(modulus.pow(&m, &d).to_be_bytes(&mut out), "s is below n");
    out
}

/// `META-INF/container.xml` for a book whose package document is
/// `EPUB/content.opf`.
const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

/// About 600 characters of [`WORDS`] in sentences: one chapter's paragraph,
/// which a 432 × 648 pt page holds whole.
fn paragraph(words: &mut Seeded) -> String {
    let mut text = String::new();
    let mut start = true;
    while text.chars().count() < 600 {
        if !text.is_empty() {
            text.push(' ');
        }
        let word = WORDS[words.below(WORDS.len())];
        if start {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                text.extend(first.to_uppercase());
                text.push_str(chars.as_str());
            }
            start = false;
        } else {
            text.push_str(word);
        }
        if words.below(10) == 0 {
            text.push('.');
            start = true;
        }
    }
    text.push('.');
    text
}

/// A book of `chapters` chapters, each one XHTML file holding a heading and
/// one [`paragraph`], all set in Liberation Serif Regular embedded through
/// `@font-face` in the one stylesheet every chapter links, so its glyphs are
/// drawn in both builds. Each chapter starts a page and fits on it, so the
/// book is `chapters` pages long at the default box. Written through
/// `tests/epub_support`'s OCF writer, the one the tests' generated books use.
pub fn book(chapters: usize) -> Vec<u8> {
    let mut words = Seeded::new(chapters as u64);
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut entries = vec![
        OcfEntry::stored("mimetype", OCF_MEDIA_TYPE),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
    ];
    for chapter in 1..=chapters {
        manifest.push_str(&format!(
            r#"<item id="c{chapter}" href="c{chapter}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{chapter}"/>"#));
    }
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000000b0</dc:identifier>"#,
            r#"<dc:title>A Book of {chapters} Chapters</dc:title>"#,
            r#"<dc:language>en</dc:language>"#,
            r#"</metadata><manifest>"#,
            r#"<item id="css" href="book.css" media-type="text/css"/>"#,
            r#"<item id="serif" href="serif.ttf" media-type="font/ttf"/>"#,
            r#"{manifest}</manifest><spine>{spine}</spine></package>"#
        ),
        chapters = chapters,
        manifest = manifest,
        spine = spine
    );
    entries.push(OcfEntry::deflated("EPUB/content.opf", package.as_bytes()));
    let stylesheet = concat!(
        r#"@font-face { font-family: "Book Serif"; src: url(serif.ttf) format("truetype"); } "#,
        r#"body { font-family: "Book Serif"; }"#
    );
    entries.push(OcfEntry::deflated("EPUB/book.css", stylesheet.as_bytes()));
    entries.push(OcfEntry::deflated("EPUB/serif.ttf", &serif()));
    for chapter in 1..=chapters {
        let body = paragraph(&mut words);
        let xhtml = format!(
            concat!(
                r#"<?xml version="1.0" encoding="utf-8"?>"#,
                r#"<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en"><head>"#,
                r#"<title>Chapter {chapter}</title>"#,
                r#"<link rel="stylesheet" type="text/css" href="book.css"/>"#,
                r#"</head><body><h2>Chapter {chapter}</h2><p>{body}</p></body></html>"#
            ),
            chapter = chapter,
            body = body
        );
        entries.push(OcfEntry::deflated(
            &format!("EPUB/c{chapter}.xhtml"),
            xhtml.as_bytes(),
        ));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// The long words [`hyphenated_paragraph`] draws from: British English of
/// twelve letters and more, ASCII and none holding a hyphen, so a line whose
/// last word does not fit has letters enough to break inside it.
const LONG_WORDS: [&str; 16] = [
    "accommodation",
    "administration",
    "characteristic",
    "communication",
    "consideration",
    "environmental",
    "establishment",
    "extraordinary",
    "independently",
    "international",
    "investigation",
    "neighbourhood",
    "organisation",
    "particularly",
    "responsibility",
    "understanding",
];

/// The short words [`hyphenated_paragraph`] sets between the long ones.
const LINKING_WORDS: [&str; 8] = ["the", "of", "and", "in", "with", "for", "an", "by"];

/// About 600 characters of [`LONG_WORDS`] and [`LINKING_WORDS`], a long word
/// and a short one in turn, in sentences: one chapter's paragraph of
/// [`hyphenated_book`], which a 432 × 648 pt page holds whole, as it holds
/// [`paragraph`]'s. It holds no hyphen and no soft hyphen.
fn hyphenated_paragraph(words: &mut Seeded) -> String {
    let mut text = String::new();
    let mut start = true;
    while text.chars().count() < 600 {
        if !text.is_empty() {
            text.push(' ');
        }
        let long = LONG_WORDS[words.below(LONG_WORDS.len())];
        if start {
            let mut letters = long.chars();
            if let Some(first) = letters.next() {
                text.extend(first.to_uppercase());
                text.push_str(letters.as_str());
            }
            start = false;
        } else {
            text.push_str(long);
        }
        text.push(' ');
        text.push_str(LINKING_WORDS[words.below(LINKING_WORDS.len())]);
        if words.below(6) == 0 {
            text.push('.');
            start = true;
        }
    }
    text.push('.');
    text
}

/// A book of `chapters` chapters in British English set in `hyphens: auto`
/// (`docs/design/hyphenation.md`): [`book`]'s shape, each chapter one XHTML
/// file holding a heading and one paragraph in Liberation Serif Regular
/// embedded through `@font-face`, every chapter `xml:lang="en-GB"`, the one
/// stylesheet setting every paragraph in `hyphens: auto`, and each paragraph
/// [`hyphenated_paragraph`]'s long words. A build with the `hyphenation`
/// feature breaks its lines at the vendored `en-GB` dictionary's points; one
/// without, or a revision with no `hyphens: auto`, sets it as `manual`. Each
/// chapter starts a page and fits on it either way, so the book is
/// `chapters` pages long at the default box.
pub fn hyphenated_book(chapters: usize) -> Vec<u8> {
    let mut words = Seeded::new(chapters as u64 + 1_000);
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut entries = vec![
        OcfEntry::stored("mimetype", OCF_MEDIA_TYPE),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
    ];
    for chapter in 1..=chapters {
        manifest.push_str(&format!(
            r#"<item id="c{chapter}" href="c{chapter}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        spine.push_str(&format!(r#"<itemref idref="c{chapter}"/>"#));
    }
    let package = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000000b1</dc:identifier>"#,
            r#"<dc:title>A Hyphenated Book of {chapters} Chapters</dc:title>"#,
            r#"<dc:language>en-GB</dc:language>"#,
            r#"</metadata><manifest>"#,
            r#"<item id="css" href="book.css" media-type="text/css"/>"#,
            r#"<item id="serif" href="serif.ttf" media-type="font/ttf"/>"#,
            r#"{manifest}</manifest><spine>{spine}</spine></package>"#
        ),
        chapters = chapters,
        manifest = manifest,
        spine = spine
    );
    entries.push(OcfEntry::deflated("EPUB/content.opf", package.as_bytes()));
    let stylesheet = concat!(
        r#"@font-face { font-family: "Book Serif"; src: url(serif.ttf) format("truetype"); } "#,
        r#"body { font-family: "Book Serif"; } p { hyphens: auto; }"#
    );
    entries.push(OcfEntry::deflated("EPUB/book.css", stylesheet.as_bytes()));
    entries.push(OcfEntry::deflated("EPUB/serif.ttf", &serif()));
    for chapter in 1..=chapters {
        let body = hyphenated_paragraph(&mut words);
        let xhtml = format!(
            concat!(
                r#"<?xml version="1.0" encoding="utf-8"?>"#,
                r#"<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-GB"><head>"#,
                r#"<title>Chapter {chapter}</title>"#,
                r#"<link rel="stylesheet" type="text/css" href="book.css"/>"#,
                r#"</head><body><h2>Chapter {chapter}</h2><p>{body}</p></body></html>"#
            ),
            chapter = chapter,
            body = body
        );
        entries.push(OcfEntry::deflated(
            &format!("EPUB/c{chapter}.xhtml"),
            xhtml.as_bytes(),
        ));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// How many sections [`html_report`] and [`markdown`] hold: what lays out
/// into their 20 pages at the default box.
const REPORT_SECTIONS: usize = 20;

/// A report of [`REPORT_SECTIONS`] sections in XHTML, each a heading, two
/// paragraphs and a ruled table of six rows on a page of its own — a forced
/// break before every section but the first, and each about four fifths of a
/// page long — so it lays out into 20 pages at the default box: the
/// HTML-to-PDF operation's input.
pub fn html_report() -> String {
    let mut words = Seeded::new(20);
    let mut body = String::new();
    for section in 1..=REPORT_SECTIONS {
        body.push_str(&format!(
            r#"<div class="section"><h2>Section {section}</h2>"#
        ));
        for _ in 0..2 {
            body.push_str(&format!("<p>{}</p>", paragraph(&mut words)));
        }
        body.push_str("<table><tr><th>Item</th><th>Count</th><th>Note</th></tr>");
        for row in 1..=5 {
            let note = WORDS[words.below(WORDS.len())];
            let count = words.below(1000);
            body.push_str(&format!(
                "<tr><td>{section}.{row}</td><td>{count}</td><td>{note}</td></tr>"
            ));
        }
        body.push_str("</table></div>");
    }
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en"><head><title>A Report</title>"#,
            r#"<style>.section + .section {{ page-break-before: always }} "#,
            r#"table {{ border-collapse: collapse; margin: 6pt 0 }} "#,
            r#"td, th {{ border: 0.5pt solid black; padding: 2pt 6pt }}</style>"#,
            r#"</head><body>{body}</body></html>"#
        ),
        body = body
    )
}

/// The page box the HTML report is laid out in: the default box a book opens
/// at, which is the one Markdown is laid out in too.
pub fn default_box() -> PageBox {
    let (width, height) = OpenOptions::default().page;
    PageBox::new(width, height)
}

/// A CommonMark document of [`REPORT_SECTIONS`] sections, each a heading,
/// three paragraphs and a list of five items, which lays out into 20 pages
/// at the default box. CommonMark has no table, so a list stands where the
/// report's table does.
pub fn markdown() -> String {
    let mut words = Seeded::new(21);
    let mut text = String::new();
    for section in 1..=REPORT_SECTIONS {
        text.push_str(&format!("## Section {section}\n\n"));
        for _ in 0..3 {
            text.push_str(&paragraph(&mut words));
            text.push_str("\n\n");
        }
        for item in 1..=5 {
            let note = WORDS[words.below(WORDS.len())];
            text.push_str(&format!("- Item {section}.{item}: {note}\n"));
        }
        text.push('\n');
    }
    text
}

/// One committed fixture: its file name and its bytes.
pub struct Fixture {
    /// The file name, which the operations' assertions name it by.
    pub name: String,
    /// Its bytes.
    pub bytes: Arc<[u8]>,
}

/// Every file under `dir` (relative to this crate, searched to every depth)
/// whose extension is one of `extensions`, in name order, except those
/// `left_out` names: a fixture the facade refuses is left out by name,
/// never timed as a failure.
fn fixtures(dir: &str, extensions: &[&str], left_out: &[&str]) -> Vec<Fixture> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("the fixture directory {}: {error}", dir.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    walk(&in_tree(dir), &mut paths);
    let mut out: Vec<Fixture> = paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extensions.contains(&extension))
        })
        .map(|path| Fixture {
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a UTF-8 file name")
                .to_string(),
            bytes: std::fs::read(&path)
                .unwrap_or_else(|error| panic!("the fixture {}: {error}", path.display()))
                .into(),
        })
        .filter(|fixture| !left_out.contains(&fixture.name.as_str()))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Every committed SVG fixture the facade opens.
pub fn svg_fixtures() -> Vec<Fixture> {
    fixtures("../tinker-pdf-svg/tests/fixtures", &["svg"], &[])
}

/// Every committed XPS and OpenXPS package.
pub fn xps_fixtures() -> Vec<Fixture> {
    fixtures("tests", &["xps", "oxps"], &[])
}

/// Every committed `.cbz`.
pub fn cbz_fixtures() -> Vec<Fixture> {
    fixtures("tests/cbz", &["cbz"], &[])
}

/// One of the three Liberation faces the fonts group outlines, by file name.
pub fn liberation(name: &str) -> Vec<u8> {
    let path = in_tree("../tinker-pdf-font/data/liberation").join(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed face {}: {error}", path.display()))
}

/// One of the two variable faces `tinker-pdf-font` commits for its variation
/// tests (`tests/variable/`, written by `tests/variable_fixtures.rs` from the
/// statements in `tests/variable_support/mod.rs`), by file name: the `glyf`
/// face and the CFF2 face the fonts group outlines and instances.
pub fn variable_face(name: &str) -> Vec<u8> {
    let path = in_tree("../tinker-pdf-font/tests/variable").join(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed face {}: {error}", path.display()))
}

/// A face under `tinker-pdf-shape/data/`, by its path there.
pub fn shape_face(path: &str) -> Vec<u8> {
    let path = in_tree("../tinker-pdf-shape/data").join(path);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed face {}: {error}", path.display()))
}

/// Every committed CFF program, by file name: the four bare ones the `cff`
/// fuzz target seeds with, and the `CFF ` table of every committed OpenType
/// face — aots's 185, text-rendering-tests' three and the `sfnt` target's
/// `opentype-cff.otf`.
pub fn cff_programs() -> Vec<Fixture> {
    let mut out = Vec::new();
    let mut read = |dir: &str, extension: &str, whole: bool| {
        let mut names: Vec<std::path::PathBuf> = std::fs::read_dir(in_tree(dir))
            .unwrap_or_else(|error| panic!("the font directory {dir}: {error}"))
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|e| e == extension))
            .collect();
        names.sort();
        for path in names {
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("the font {}: {error}", path.display()));
            let program = if whole {
                bytes
            } else {
                let face = Sfnt::parse(&bytes)
                    .unwrap_or_else(|| panic!("{} is an OpenType face", path.display()));
                face.table(u32::from_be_bytes(*b"CFF "))
                    .unwrap_or_else(|| panic!("{} has a CFF table", path.display()))
                    .to_vec()
            };
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a UTF-8 file name")
                .to_string();
            out.push(Fixture {
                name,
                bytes: program.into(),
            });
        }
    };
    read("../../fuzz/corpus/cff", "cff", true);
    read("../tinker-pdf-shape/data/aots/fonts", "otf", false);
    read(
        "../tinker-pdf-shape/data/text-rendering-tests/fonts",
        "otf",
        false,
    );
    read("../../fuzz/corpus/sfnt", "otf", false);
    out
}

/// Urdu words in the seventeen letters `TestShapeAran.ttf`, a Nasta'liq test
/// face, maps: with the space between them, every character it maps.
const URDU: [&str; 36] = [
    "نستعلیق",
    "خط",
    "فن",
    "طوطی",
    "تختی",
    "نقل",
    "قسط",
    "کل",
    "پتا",
    "ٹکٹ",
    "ڈاک",
    "سال",
    "نیا",
    "تو",
    "کا",
    "کی",
    "کو",
    "تک",
    "خوف",
    "عقل",
    "طاقت",
    "قانون",
    "انسان",
    "سوال",
    "خیال",
    "نئی",
    "کئی",
    "پانی",
    "تیل",
    "ستون",
    "عینک",
    "لوٹا",
    "ڈنڈا",
    "پیاس",
    "لیکن",
    "فائل",
];

/// Hindi words with the script's conjuncts, a reph, nuktas, candrabindus and
/// every vowel sign's position.
const HINDI: [&str; 67] = [
    "भारत",
    "हिन्दी",
    "भाषा",
    "नमस्ते",
    "दुनिया",
    "किताब",
    "पानी",
    "विद्यालय",
    "प्रश्न",
    "उत्तर",
    "स्वतंत्रता",
    "क्षेत्र",
    "ज्ञान",
    "श्रम",
    "राष्ट्र",
    "संस्कृति",
    "धर्म",
    "कर्म",
    "सूर्य",
    "चंद्रमा",
    "पृथ्वी",
    "आकाश",
    "समुद्र",
    "पर्वत",
    "नदी",
    "वृक्ष",
    "फूल",
    "पत्ता",
    "हवा",
    "आग",
    "शिक्षा",
    "विज्ञान",
    "इतिहास",
    "भूगोल",
    "गणित",
    "साहित्य",
    "कविता",
    "कहानी",
    "उपन्यास",
    "लेखक",
    "पाठक",
    "मित्र",
    "परिवार",
    "माँ",
    "पिता",
    "बच्चा",
    "घर",
    "गाँव",
    "शहर",
    "सड़क",
    "ज़मीन",
    "क़लम",
    "ख़बर",
    "और",
    "है",
    "में",
    "से",
    "का",
    "की",
    "के",
    "को",
    "पर",
    "यह",
    "वह",
    "हम",
    "तुम",
    "आप",
];

/// 1 000 characters of `words` drawn from `seed`, a space between each and,
/// every eighth word or so, `stop` after one.
fn thousand(words: &[&str], stop: Option<char>, seed: u64) -> String {
    let mut seeded = Seeded::new(seed);
    let mut text = String::new();
    while text.chars().count() < 1000 {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(words[seeded.below(words.len())]);
        if let Some(stop) = stop.filter(|_| seeded.below(8) == 0) {
            text.push(stop);
        }
    }
    text.chars().take(1000).collect()
}

/// 1 000 characters of Latin, from [`WORDS`].
pub fn latin_paragraph() -> String {
    thousand(&WORDS, Some('.'), 1)
}

/// 1 000 characters of Urdu in Arabic script, from [`URDU`]: no punctuation,
/// since the face maps none.
pub fn arabic_paragraph() -> String {
    thousand(&URDU, None, 2)
}

/// 1 000 characters of Hindi in Devanagari, from [`HINDI`], with danda.
pub fn devanagari_paragraph() -> String {
    thousand(&HINDI, Some('।'), 3)
}

/// What [`vertical_paragraph`] draws from: Japanese words, which a column sets
/// upright and runs together; the ideographic comma and full stop, upright in
/// their vertical forms; Latin words and a number between spaces, which a
/// column sets sideways; and full-width digits, upright.
const COLUMN_WORDS: [&str; 26] = [
    "縦書き",
    "日本語",
    "文字",
    "組版",
    "漢字",
    "東京",
    "学校",
    "時間",
    "ひらがな",
    "カタカナ",
    "です",
    "ます",
    "の",
    "は",
    "を",
    "スト",
    "、",
    "。",
    "、",
    "。",
    " PDF ",
    " fine ",
    " office ",
    " 2026 ",
    "１２３",
    "縦組",
];

/// 1 000 characters of a vertical paragraph of mixed orientation, from
/// [`COLUMN_WORDS`] run together, every character of it one
/// [`column_face`] maps.
pub fn vertical_paragraph() -> String {
    let mut seeded = Seeded::new(4);
    let mut text = String::new();
    while text.chars().count() < 1000 {
        text.push_str(COLUMN_WORDS[seeded.below(COLUMN_WORDS.len())]);
    }
    text.chars().take(1000).collect()
}

/// Big-endian `u16`s.
fn be16s(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// An OpenType Layout header, with `slots[i]` patched to the offset of
/// `parts[i]`, which follow it in order.
fn assembled(mut head: Vec<u8>, slots: &[usize], parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for (slot, part) in slots.iter().zip(parts) {
        let offset = u16::try_from(head.len() + body.len()).expect("a table under 64 KB");
        head[*slot..*slot + 2].copy_from_slice(&offset.to_be_bytes());
        body.extend_from_slice(part);
    }
    head.extend(body);
    head
}

/// Coverage format 1 over `glyphs`, sorted.
fn covering(glyphs: &[u16]) -> Vec<u8> {
    let mut sorted = glyphs.to_vec();
    sorted.sort_unstable();
    let mut out = be16s(&[1, u16::try_from(sorted.len()).expect("a small coverage")]);
    out.extend(be16s(&sorted));
    out
}

/// One lookup of `kind` with its one subtable.
fn one_lookup(kind: u16, subtable: Vec<u8>) -> Vec<u8> {
    assembled(be16s(&[kind, 0, 1, 0]), &[6], &[subtable])
}

/// A `GSUB` or `GPOS` whose scripts' default language systems each name
/// every feature, and each feature its lookup, by index.
fn layout(scripts: &[[u8; 4]], features: &[([u8; 4], u16)], lookups: &[Vec<u8>]) -> Vec<u8> {
    let count = u16::try_from(features.len()).expect("a small feature list");
    let mut lang_sys = be16s(&[0, 0xFFFF, count]);
    lang_sys.extend(be16s(&(0..count).collect::<Vec<u16>>()));
    let script = assembled(be16s(&[0, 0]), &[0], &[lang_sys]);
    let mut script_head = be16s(&[u16::try_from(scripts.len()).expect("a small script list")]);
    let mut script_slots = Vec::new();
    for tag in scripts {
        script_head.extend_from_slice(tag);
        script_slots.push(script_head.len());
        script_head.extend(be16s(&[0]));
    }
    let script_list = assembled(script_head, &script_slots, &vec![script; scripts.len()]);

    let mut feature_head = be16s(&[count]);
    let mut feature_slots = Vec::new();
    let mut feature_tables = Vec::new();
    for (tag, index) in features {
        feature_head.extend_from_slice(tag);
        feature_slots.push(feature_head.len());
        feature_head.extend(be16s(&[0]));
        feature_tables.push(be16s(&[0, 1, *index]));
    }
    let feature_list = assembled(feature_head, &feature_slots, &feature_tables);

    let mut list_head = be16s(&[u16::try_from(lookups.len()).expect("a small lookup list")]);
    list_head.extend(vec![0u8; lookups.len() * 2]);
    let slots: Vec<usize> = (0..lookups.len()).map(|i| 2 + i * 2).collect();
    let lookup_list = assembled(list_head, &slots, lookups);

    assembled(
        be16s(&[1, 0, 0, 0, 0]),
        &[4, 6, 8],
        &[script_list, feature_list, lookup_list],
    )
}

/// A face for [`vertical_paragraph`] set as a column, generated as every
/// input here is: a `glyf` box, an `hmtx` advance and a `vmtx` advance and
/// top side bearing for each character the paragraph holds, all by formula;
/// a `vert` form for the comma and the full stop; `vkrn` between `ス` and
/// `ト`; and, for the sideways runs, `liga` for `fi` and `kern` between `P`
/// and `D`. Every number is an integer, so every target writes the same
/// bytes.
pub fn column_face() -> Vec<u8> {
    let mut chars: Vec<char> = COLUMN_WORDS.iter().flat_map(|word| word.chars()).collect();
    chars.sort_unstable();
    chars.dedup();
    let glyph_of = |c: char| -> u16 {
        let at = chars.binary_search(&c).expect("a character the words hold");
        u16::try_from(at + 1).expect("a small face")
    };
    let mapped = u16::try_from(chars.len()).expect("a small face");
    // After the characters: the comma's and the stop's vertical forms, and
    // the `fi` ligature.
    let (comma, stop, fi) = (mapped + 1, mapped + 2, mapped + 3);
    let glyphs = mapped + 4;

    // Each glyph's horizontal advance, its box, and its vertical advance and
    // top side bearing, by what it stands for.
    let shape = |glyph: u16| -> (u16, [i16; 4], u16, i16) {
        let c = (1..=mapped)
            .contains(&glyph)
            .then(|| chars[usize::from(glyph - 1)]);
        let step = i16::try_from(glyph % 7).expect("under seven") * 10;
        match c {
            Some(' ') => (250, [0, 0, 0, 0], 1000, 0),
            Some('、' | '。') => (1000, [80, -60, 380, 240], 1000, 600),
            Some(c) if c.is_ascii_alphanumeric() => {
                let width = 480 + u16::try_from(glyph % 9).expect("under nine") * 15;
                let right = i16::try_from(width).expect("under 1000") - 40;
                (width, [40, 0, right, 700 + step], 1000, 100 + step)
            }
            Some(_) => (
                1000,
                [60, -80, 940, 820 + step],
                1000 + u16::try_from(step).expect("small"),
                80 + step,
            ),
            None if glyph == comma || glyph == stop => (1000, [620, 560, 920, 860], 1000, 140),
            None if glyph == fi => (900, [40, 0, 860, 700], 1000, 100),
            None => (500, [50, 0, 450, 700], 1000, 100),
        }
    };

    let mut hmtx = Vec::new();
    let mut vmtx = Vec::new();
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for glyph in 0..glyphs {
        let (advance, [x0, y0, x1, y1], height, bearing) = shape(glyph);
        hmtx.extend(be16s(&[advance, 0]));
        vmtx.extend_from_slice(&height.to_be_bytes());
        vmtx.extend_from_slice(&bearing.to_be_bytes());
        loca.extend_from_slice(
            &u32::try_from(glyf.len())
                .expect("a small glyf")
                .to_be_bytes(),
        );
        if x1 > x0 && y1 > y0 {
            // One contour of four on-curve points, word deltas.
            for value in [1i16, x0, y0, x1, y1] {
                glyf.extend_from_slice(&value.to_be_bytes());
            }
            glyf.extend(be16s(&[3, 0]));
            glyf.extend_from_slice(&[0x01; 4]);
            for dx in [x0, x1 - x0, 0, x0 - x1] {
                glyf.extend_from_slice(&dx.to_be_bytes());
            }
            for dy in [y0, 0, y1 - y0, 0] {
                glyf.extend_from_slice(&dy.to_be_bytes());
            }
        }
    }
    loca.extend_from_slice(
        &u32::try_from(glyf.len())
            .expect("a small glyf")
            .to_be_bytes(),
    );

    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[34..36].copy_from_slice(&glyphs.to_be_bytes());
    let mut vhea = vec![0u8; 36];
    vhea[0..4].copy_from_slice(&0x0001_1000u32.to_be_bytes());
    vhea[34..36].copy_from_slice(&glyphs.to_be_bytes());
    let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
    maxp.extend_from_slice(&glyphs.to_be_bytes());

    let groups = u32::try_from(chars.len()).expect("a small cmap");
    let mut cmap = be16s(&[0, 1, 3, 10]);
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend(be16s(&[12, 0]));
    cmap.extend_from_slice(&(16 + groups * 12).to_be_bytes());
    cmap.extend_from_slice(&0u32.to_be_bytes());
    cmap.extend_from_slice(&groups.to_be_bytes());
    for c in &chars {
        let code = u32::from(*c);
        cmap.extend_from_slice(&code.to_be_bytes());
        cmap.extend_from_slice(&code.to_be_bytes());
        cmap.extend_from_slice(&u32::from(glyph_of(*c)).to_be_bytes());
    }

    let scripts = [*b"DFLT", *b"hani", *b"kana", *b"latn"];
    // SingleSubstFormat2: the comma and the stop to their vertical forms.
    let mut forms = [(glyph_of('、'), comma), (glyph_of('。'), stop)];
    forms.sort_unstable();
    let mut vert = be16s(&[2, 0, 2, forms[0].1, forms[1].1]);
    vert = assembled(vert, &[2], &[covering(&[forms[0].0, forms[1].0])]);
    // LigatureSubstFormat1: `f` then `i`.
    let ligature = be16s(&[fi, 2, glyph_of('i')]);
    let set = assembled(be16s(&[1, 0]), &[2], &[ligature]);
    let liga = assembled(
        be16s(&[1, 0, 1, 0]),
        &[2, 6],
        &[covering(&[glyph_of('f')]), set],
    );
    let gsub = layout(
        &scripts,
        &[(*b"vert", 0), (*b"liga", 1)],
        &[one_lookup(1, vert), one_lookup(4, liga)],
    );
    // PairPosFormat1, one pair each: `ス` then `ト` a YAdvance of −150 down
    // a column, and `P` then `D` an XAdvance of −40 along a line.
    let pair = |first: u16, second: u16, format: u16, value: i16| {
        let mut set = be16s(&[1, second]);
        set.extend_from_slice(&value.to_be_bytes());
        assembled(
            be16s(&[1, 0, format, 0, 1, 0]),
            &[2, 10],
            &[covering(&[first]), set],
        )
    };
    let gpos = layout(
        &scripts,
        &[(*b"vkrn", 0), (*b"kern", 1)],
        &[
            one_lookup(2, pair(glyph_of('ス'), glyph_of('ト'), 0x0008, -150)),
            one_lookup(2, pair(glyph_of('P'), glyph_of('D'), 0x0004, -40)),
        ],
    );

    sfnt_of(vec![
        (*b"GPOS", gpos),
        (*b"GSUB", gsub),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"vhea", vhea),
        (*b"vmtx", vmtx),
    ])
}

/// A TrueType face of `tables`: its directory, the records in tag order with
/// no checksum, and the tables after it in the same order, unpadded.
fn sfnt_of(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend(be16s(&[
        u16::try_from(tables.len()).expect("a small directory"),
        0,
        0,
        0,
    ]));
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, table) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).expect("a small face").to_be_bytes());
        out.extend_from_slice(
            &u32::try_from(table.len())
                .expect("a small table")
                .to_be_bytes(),
        );
        offset += table.len();
        body.extend_from_slice(table);
    }
    out.extend(body);
    out
}

// ---- A page of colour glyphs (FT-06.17a) -----------------------------------

/// The palette [`colour_face`]'s graphs paint in, each entry RGBA: a colour
/// emoji's yellow and orange, a dark brown for its features, and five others
/// the capitals take in turn.
const COLOUR_PALETTE: [[u8; 4]; 8] = [
    [255, 214, 82, 255],
    [247, 152, 36, 255],
    [92, 54, 24, 255],
    [232, 80, 64, 255],
    [64, 140, 222, 255],
    [84, 182, 96, 255],
    [146, 92, 204, 255],
    [255, 255, 255, 255],
];

/// How many capitals [`colour_face`] draws in colour: `A` to `Z`, glyphs 1 to
/// 26.
const CAPITALS: u16 = 26;

/// The type a page of colour glyphs is set in, in points: a glyph an em wide.
pub const COLOUR_SIZE: f64 = 24.0;

/// How many lines a page of colour glyphs sets, and how many glyphs a line:
/// 21 glyphs at 24 points fill A4's width inside 36-point margins, and 25
/// lines 30 points apart its height.
const COLOUR_LINES: usize = 25;
const COLOUR_COLUMNS: usize = 21;

/// The lines of a page of colour glyphs: the `j`th glyph of line `k` the
/// `(7k + 11j) mod 26`th capital, so that a line holds 21 different capitals
/// and each of the 26 stands on 20 or 21 of the lines.
pub fn colour_lines() -> Vec<String> {
    (0..COLOUR_LINES)
        .map(|line| {
            (0..COLOUR_COLUMNS)
                .map(|column| {
                    let n = u8::try_from((7 * line + 11 * column) % 26).expect("under 26");
                    char::from(b'A' + n)
                })
                .collect()
        })
        .collect()
}

/// Where line `line` of [`colour_lines`] stands on an A4 page, 595 by 842
/// points: its text matrix, 36 points in from the left edge, the first
/// baseline 60 points under the top edge and each 30 under the one before.
pub fn colour_line_matrix(line: usize) -> [f64; 6] {
    let down = f64::from(u32::try_from(line).expect("a line of the page"));
    [1.0, 0.0, 0.0, 1.0, 36.0, 782.0 - 30.0 * down]
}

/// Big-endian `i16`s.
fn words(values: &[i16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// A simple TrueType glyph of `contours`, each a closed run of points
/// `(x, y, on the curve)`: its contour count and box, the last point of each
/// contour, no instructions, a flag a point, and every coordinate a word
/// delta from the point before.
fn simple_glyph(contours: &[Vec<(i16, i16, bool)>]) -> Vec<u8> {
    let points: Vec<(i16, i16, bool)> = contours.iter().flatten().copied().collect();
    let xs = points.iter().map(|&(x, _, _)| x);
    let ys = points.iter().map(|&(_, y, _)| y);
    let mut out = words(&[
        i16::try_from(contours.len()).expect("few contours"),
        xs.clone().min().expect("a point"),
        ys.clone().min().expect("a point"),
        xs.max().expect("a point"),
        ys.max().expect("a point"),
    ]);
    let mut last = 0usize;
    for contour in contours {
        last += contour.len();
        out.extend(be16s(&[u16::try_from(last - 1).expect("few points")]));
    }
    out.extend(be16s(&[0]));
    out.extend(points.iter().map(|&(_, _, on)| u8::from(on)));
    let mut previous = 0i16;
    for &(x, _, _) in &points {
        out.extend_from_slice(&(x - previous).to_be_bytes());
        previous = x;
    }
    previous = 0;
    for &(_, y, _) in &points {
        out.extend_from_slice(&(y - previous).to_be_bytes());
        previous = y;
    }
    out
}

/// The three outlines capital `n`'s graph fills, in font units at a thousand
/// to the em: a disc of four quadratic arcs, clockwise from its top, its
/// on-curve points at the compass points and its off-curve ones at its box's
/// corners, its radius smaller with `n`; two eyes, higher with `n`; and a
/// mouth, lower with it.
fn capital_shapes(n: u16) -> [Vec<Vec<(i16, i16, bool)>>; 3] {
    let n = i16::try_from(n).expect("a capital");
    let (x, y, r) = (500, 450, 400 - n % 4 * 20);
    let disc = vec![vec![
        (x, y + r, true),
        (x + r, y + r, false),
        (x + r, y, true),
        (x + r, y - r, false),
        (x, y - r, true),
        (x - r, y - r, false),
        (x - r, y, true),
        (x - r, y + r, false),
    ]];
    let rect = |x0: i16, y0: i16, x1: i16, y1: i16| {
        vec![
            (x0, y0, true),
            (x0, y1, true),
            (x1, y1, true),
            (x1, y0, true),
        ]
    };
    let lift = n % 3 * 20;
    let eyes = vec![
        rect(320, 480 + lift, 420, 640 + lift),
        rect(580, 480 + lift, 680, 640 + lift),
    ];
    let low = n % 5 * 10;
    [disc, eyes, vec![rect(300, 220 + low, 700, 300 + low)]]
}

/// `head`, a `COLR` version 1 paint table whose `Offset24` at `slot` names
/// `child`, with the child written after it: *Paint tables* counts the
/// offset from the naming table's own start.
fn paint_naming(mut head: Vec<u8>, slot: usize, child: Vec<u8>) -> Vec<u8> {
    let offset = u32::try_from(head.len())
        .expect("a small paint")
        .to_be_bytes();
    head[slot..slot + 3].copy_from_slice(&offset[1..]);
    head.extend(child);
    head
}

/// A `ColorLine` padded at both ends (`EXTEND_PAD`) from palette entry `from`
/// at offset 0 to `to` at 1, both opaque: an `F2DOT14` of 1 is `0x4000`.
fn colour_line(from: u16, to: u16) -> Vec<u8> {
    let mut out = vec![0];
    out.extend(be16s(&[2, 0, from, 0x4000, 0x4000, to, 0x4000]));
    out
}

/// `PaintGlyph` (format 10): `glyph`'s outline filled with `paint`.
fn paint_glyph(glyph: u16, paint: Vec<u8>) -> Vec<u8> {
    let mut head = vec![10, 0, 0, 0];
    head.extend(be16s(&[glyph]));
    paint_naming(head, 1, paint)
}

/// Capital `n`'s three layers, bottom first, over the glyphs `shapes` names:
/// the disc under a radial gradient (format 6) from a highlight above its
/// centre to its rim; the eyes in a solid (format 2) of the brown, opaque for
/// an even capital and at three quarters for an odd one; and the mouth under
/// a linear gradient (format 4) across it. The palette entries turn with `n`.
fn capital_layers(n: u16, shapes: [u16; 3]) -> [Vec<u8>; 3] {
    let k = i16::try_from(n).expect("a capital");
    let mut radial = vec![6, 0, 0, 0];
    radial.extend(words(&[420, 560]));
    radial.extend(be16s(&[0]));
    radial.extend(words(&[500, 450]));
    radial.extend(be16s(&[u16::try_from(400 - k % 4 * 20).expect("a radius")]));
    let radial = paint_naming(radial, 1, colour_line(n % 8, (n + 1) % 8));
    let mut solid = vec![2];
    solid.extend(be16s(&[2, if n % 2 == 0 { 0x4000 } else { 0x3000 }]));
    let low = 220 + k % 5 * 10;
    let mut linear = vec![4, 0, 0, 0];
    linear.extend(words(&[300, low, 700, low, 300, low + 100]));
    let linear = paint_naming(linear, 1, colour_line((n + 3) % 8, (n + 5) % 8));
    [
        paint_glyph(shapes[0], radial),
        paint_glyph(shapes[1], solid),
        paint_glyph(shapes[2], linear),
    ]
}

/// A face that draws each capital as a colour glyph, generated as every
/// input here is, by integer formula: capital `n` glyph `1 + n`, its outline
/// its disc ([`capital_shapes`]), and its eyes and mouth glyphs `27 + 2n` and
/// `28 + 2n`; every advance an em of a thousand units; a `cmap` from `A`–`Z`
/// onto the capitals; and a `COLR` version 1 whose `BaseGlyphList` gives each
/// capital a `PaintColrLayers` of its three layers in the `LayerList`
/// ([`capital_layers`]), over a `CPAL` of one palette, [`COLOUR_PALETTE`].
/// The capitals' own outlines are what a writer without colour glyphs draws.
pub fn colour_face() -> Vec<u8> {
    let glyphs = 1 + CAPITALS * 3;
    let mut outlines: Vec<Vec<u8>> = vec![Vec::new(); usize::from(glyphs)];
    for n in 0..CAPITALS {
        let [disc, eyes, mouth] = capital_shapes(n);
        outlines[usize::from(1 + n)] = simple_glyph(&disc);
        outlines[usize::from(1 + CAPITALS + 2 * n)] = simple_glyph(&eyes);
        outlines[usize::from(2 + CAPITALS + 2 * n)] = simple_glyph(&mouth);
    }
    let at = |glyf: &[u8]| {
        u32::try_from(glyf.len())
            .expect("a small glyf")
            .to_be_bytes()
    };
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for outline in &outlines {
        loca.extend_from_slice(&at(&glyf));
        glyf.extend_from_slice(outline);
    }
    loca.extend_from_slice(&at(&glyf));
    let hmtx = be16s(&[1000, 0].repeat(usize::from(glyphs)));

    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[36..44].copy_from_slice(&words(&[100, 50, 900, 850]));
    head[50..52].copy_from_slice(&1i16.to_be_bytes());
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[4..8].copy_from_slice(&words(&[900, -100]));
    hhea[10..12].copy_from_slice(&1000u16.to_be_bytes());
    hhea[34..36].copy_from_slice(&glyphs.to_be_bytes());
    let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
    maxp.extend_from_slice(&glyphs.to_be_bytes());
    // Format 12 under (3, 10): one group, `A` to `Z` onto glyphs 1 to 26.
    let mut cmap = be16s(&[0, 1, 3, 10]);
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend(be16s(&[12, 0]));
    for value in [28u32, 0, 1, u32::from('A'), u32::from('Z'), 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }

    // The paints, each capital's `PaintColrLayers` and then its layers.
    let mut area = Vec::new();
    let mut roots = Vec::new();
    let mut layers = Vec::new();
    for n in 0..CAPITALS {
        roots.push(area.len());
        area.extend_from_slice(&[1, 3]);
        area.extend_from_slice(&(3 * u32::from(n)).to_be_bytes());
        let shapes = [1 + n, 1 + CAPITALS + 2 * n, 2 + CAPITALS + 2 * n];
        for layer in capital_layers(n, shapes) {
            layers.push(area.len());
            area.extend(layer);
        }
    }
    let base_list_at = 34usize;
    let layer_list_at = base_list_at + 4 + 6 * roots.len();
    let area_at = layer_list_at + 4 + 4 * layers.len();
    let offset = |value: usize| u32::try_from(value).expect("a small COLR").to_be_bytes();
    // *COLR header*: version 1, no version 0 records, the two lists, and no
    // clip list, variation index map or variation store.
    let mut colr = be16s(&[1, 0]);
    colr.extend_from_slice(&[0; 8]);
    colr.extend(be16s(&[0]));
    colr.extend_from_slice(&offset(base_list_at));
    colr.extend_from_slice(&offset(layer_list_at));
    colr.extend_from_slice(&[0; 12]);
    colr.extend_from_slice(&offset(roots.len()));
    for (glyph, root) in (1u16..).zip(&roots) {
        colr.extend(be16s(&[glyph]));
        colr.extend_from_slice(&offset(area_at + root - base_list_at));
    }
    colr.extend_from_slice(&offset(layers.len()));
    for layer in &layers {
        colr.extend_from_slice(&offset(area_at + layer - layer_list_at));
    }
    colr.extend(area);

    let entries = u16::try_from(COLOUR_PALETTE.len()).expect("a small palette");
    let mut cpal = be16s(&[0, entries, 1, entries]);
    cpal.extend_from_slice(&14u32.to_be_bytes());
    cpal.extend(be16s(&[0]));
    for [r, g, b, a] in COLOUR_PALETTE {
        cpal.extend_from_slice(&[b, g, r, a]);
    }

    sfnt_of(vec![
        (*b"COLR", colr),
        (*b"CPAL", cpal),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
    ])
}

// ---------------------------------------------------------------------------
// Fonts on save: a Type 1 program's pages, and pages of fonts no program
// embeds.
// ---------------------------------------------------------------------------

/// A document of `objects`, numbered from one, the first its catalog, with
/// the cross-reference table that states where each is.
fn raw_document(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A stream object: `entries`, its `/Length`, then `data`.
fn raw_stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// One line's bytes as a hexadecimal string operand, so no byte needs an
/// escape.
fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2 + 2);
    out.push('<');
    for byte in bytes {
        out.push_str(&format!("{byte:02X}"));
    }
    out.push('>');
    out
}

/// A page's lines in `/F0`, 9 points on 11, from the top of an A4 page.
fn lines_content(lines: &[Vec<u8>]) -> Vec<u8> {
    let mut content = b"BT /F0 9 Tf 11 TL 36 806 Td\n".to_vec();
    for (at, line) in lines.iter().enumerate() {
        if at > 0 {
            content.extend_from_slice(b"T* ");
        }
        content.extend_from_slice(hex_string(line).as_bytes());
        content.extend_from_slice(b" Tj\n");
    }
    content.extend_from_slice(b"ET\n");
    content
}

/// The page objects of `contents`, each an A4 page over `resources`, its
/// content stream after it, numbered from `first`: the page tree's `/Kids`
/// and the objects.
fn raw_pages(first: usize, resources: &str, contents: &[Vec<u8>]) -> (String, Vec<Vec<u8>>) {
    let mut kids = Vec::with_capacity(contents.len());
    let mut objects = Vec::with_capacity(contents.len() * 2);
    for (at, content) in contents.iter().enumerate() {
        let page = first + 2 * at;
        kids.push(format!("{page} 0 R"));
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
                 /Resources << {resources} >> /Contents {} 0 R >>",
                page + 1
            )
            .into_bytes(),
        );
        objects.push(raw_stream("", content));
    }
    (kids.join(" "), objects)
}

/// The Standard Encoding's name for an ASCII code, 32 to 126 (ISO 32000-2
/// Annex D): the letters and digits as themselves, the rest by name.
fn standard_name(code: u8) -> String {
    const PUNCTUATION: [(u8, &str); 33] = [
        (32, "space"),
        (33, "exclam"),
        (34, "quotedbl"),
        (35, "numbersign"),
        (36, "dollar"),
        (37, "percent"),
        (38, "ampersand"),
        (39, "quoteright"),
        (40, "parenleft"),
        (41, "parenright"),
        (42, "asterisk"),
        (43, "plus"),
        (44, "comma"),
        (45, "hyphen"),
        (46, "period"),
        (47, "slash"),
        (58, "colon"),
        (59, "semicolon"),
        (60, "less"),
        (61, "equal"),
        (62, "greater"),
        (63, "question"),
        (64, "at"),
        (91, "bracketleft"),
        (92, "backslash"),
        (93, "bracketright"),
        (94, "asciicircum"),
        (95, "underscore"),
        (96, "quoteleft"),
        (123, "braceleft"),
        (124, "bar"),
        (125, "braceright"),
        (126, "asciitilde"),
    ];
    const DIGITS: [&str; 10] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    ];
    if code.is_ascii_alphabetic() {
        return char::from(code).to_string();
    }
    if code.is_ascii_digit() {
        return DIGITS[usize::from(code - b'0')].to_string();
    }
    PUNCTUATION
        .iter()
        .find(|(at, _)| *at == code)
        .map(|(_, name)| (*name).to_string())
        .expect("an ASCII code from 32 to 126")
}

/// Adobe's cipher encrypting (Type 1 Font Format, chapter 7.1): `lead`, the
/// bytes a reader discards, and then `plain`, under `key`.
fn type1_cipher(plain: &[u8], key: u16, lead: &[u8]) -> Vec<u8> {
    let mut r = key;
    lead.iter()
        .chain(plain)
        .map(|&byte| {
            let cipher = byte ^ (r >> 8) as u8;
            r = u16::from(cipher)
                .wrapping_add(r)
                .wrapping_mul(52_845)
                .wrapping_add(22_719);
            cipher
        })
        .collect()
}

/// Type 1 charstring numbers in their shortest forms (chapter 6.2).
fn charstring_numbers(values: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &v in values {
        if (-107..=107).contains(&v) {
            out.push((v + 139) as u8);
        } else if (108..=1131).contains(&v) {
            let v = v - 108;
            out.push((v / 256 + 247) as u8);
            out.push((v % 256) as u8);
        } else if (-1131..=-108).contains(&v) {
            let v = -v - 108;
            out.push((v / 256 + 251) as u8);
            out.push((v % 256) as u8);
        } else {
            out.push(255);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out
}

/// A Type 1 program written here byte for byte, its three ISO 32000-2 Table
/// 125 lengths, and the width of each code from 32 to 126.
///
/// `.notdef` and the 95 glyphs the Standard Encoding names from 32 to 126,
/// each a width and a contour by formula from its code: `hsbw`, a stem each
/// way, a move, one of eight shared subroutines of lines and a curve, and a
/// line of its own; every seventh replaces its hints through OtherSubrs 3 and
/// every eleventh draws a chapter 8.3 flex. Under `/Encoding
/// StandardEncoding`, its subroutines 0 to 3 chapter 8.4's.
pub fn type1_program() -> (Vec<u8>, [usize; 3], Vec<u32>) {
    let n = charstring_numbers;
    let piece = |k: i32| -> Vec<u8> {
        [
            n(&[200 + 10 * k]),
            vec![6], // hlineto
            n(&[400 + 15 * k]),
            vec![7], // vlineto
            n(&[-40, 30, -60, 40, -80, 0]),
            vec![8], // rrcurveto
            n(&[-(20 + 5 * k), -(300 + 15 * k)]),
            vec![5, 11], // rlineto, return
        ]
        .concat()
    };
    let mut subrs: Vec<Vec<u8>> = vec![
        [n(&[3, 0]), vec![12, 16, 12, 17, 12, 17, 12, 33, 11]].concat(),
        [n(&[0, 1]), vec![12, 16, 11]].concat(),
        [n(&[0, 2]), vec![12, 16, 11]].concat(),
        vec![11],
    ];
    for k in 0..8 {
        subrs.push(piece(k));
    }
    // Subroutine 12: the stems a hint replacement declares.
    subrs.push([n(&[0, 30]), vec![1], n(&[120, 40]), vec![3, 11]].concat());

    let mut widths = Vec::with_capacity(95);
    let mut glyphs: Vec<(String, Vec<u8>)> =
        vec![(".notdef".to_string(), [n(&[0, 500]), vec![13, 14]].concat())];
    for code in 32u8..=126 {
        let i = i32::from(code - 32);
        if code == 32 {
            widths.push(250);
            glyphs.push((standard_name(code), [n(&[0, 250]), vec![13, 14]].concat()));
            continue;
        }
        let width = 400 + (i * 37) % 300;
        widths.push(width as u32);
        let (sx, sy) = (40 + i % 30, 0);
        let mut code_bytes = [
            n(&[0, width]),
            vec![13], // hsbw
            n(&[0, 20]),
            vec![1], // hstem
            n(&[60 + i % 40, 50]),
            vec![3], // vstem
            n(&[sx, sy]),
            vec![21], // rmoveto
        ]
        .concat();
        if i % 11 == 5 {
            // 8.3: the reference point and six more, from the pen, which
            // ends 80 units on.
            code_bytes.extend(n(&[1]));
            code_bytes.push(10);
            for (dx, dy) in [
                (30, 0),
                (-20, 0),
                (10, 10),
                (20, 0),
                (20, 0),
                (10, -10),
                (10, 0),
            ] {
                code_bytes.extend(n(&[dx, dy]));
                code_bytes.push(21);
                code_bytes.extend(n(&[2]));
                code_bytes.push(10);
            }
            code_bytes.extend(n(&[50, sx + 80, sy, 0]));
            code_bytes.push(10);
        }
        code_bytes.extend(n(&[4 + i % 8]));
        code_bytes.push(10); // callsubr
        if i % 7 == 3 {
            // 8.1: subr# 1 3 callothersubr pop callsubr.
            code_bytes.extend(n(&[12, 1, 3]));
            code_bytes.extend_from_slice(&[12, 16, 12, 17, 10]);
        }
        code_bytes.extend(n(&[i % 23 + 1, i % 17 + 1]));
        code_bytes.extend_from_slice(&[5, 9, 14]); // rlineto, closepath, endchar
        glyphs.push((standard_name(code), code_bytes));
    }

    let clear = b"%!PS-AdobeFont-1.0: BenchType1 001.000\n\
        12 dict begin\n\
        /FontInfo 5 dict dup begin\n\
        /version (001.000) readonly def\n\
        /FullName (Bench Type 1) readonly def\n\
        /FamilyName (Bench) readonly def\n\
        /Weight (Regular) readonly def\n\
        /ItalicAngle 0 def\n\
        end readonly def\n\
        /FontName /BenchType1 def\n\
        /PaintType 0 def\n\
        /FontType 1 def\n\
        /FontMatrix [0.001 0 0 0.001 0 0] readonly def\n\
        /FontBBox {0 -100 1000 900} readonly def\n\
        /Encoding StandardEncoding def\n\
        currentdict end\n\
        currentfile eexec\n"
        .to_vec();
    let lead = [0x55u8; 4];
    let mut private = b"dup /Private 9 dict dup begin\n\
        /RD{string currentfile exch readstring pop}executeonly def\n\
        /ND{noaccess def}executeonly def\n\
        /NP{noaccess put}executeonly def\n\
        /BlueValues [-15 0 700 715] def\n\
        /StdHW [30] def\n\
        /StdVW [50] def\n\
        /lenIV 4 def\n"
        .to_vec();
    private.extend_from_slice(format!("/Subrs {} array\n", subrs.len()).as_bytes());
    for (index, code) in subrs.iter().enumerate() {
        let encrypted = type1_cipher(code, 4330, &lead);
        private.extend_from_slice(format!("dup {index} {} RD ", encrypted.len()).as_bytes());
        private.extend_from_slice(&encrypted);
        private.extend_from_slice(b" NP\n");
    }
    private.extend_from_slice(b"ND\n");
    private.extend_from_slice(
        format!("2 index /CharStrings {} dict dup begin\n", glyphs.len()).as_bytes(),
    );
    for (name, code) in &glyphs {
        let encrypted = type1_cipher(code, 4330, &lead);
        private.extend_from_slice(format!("/{name} {} RD ", encrypted.len()).as_bytes());
        private.extend_from_slice(&encrypted);
        private.extend_from_slice(b" ND\n");
    }
    private.extend_from_slice(
        b"end\nend\nreadonly put\nnoaccess put\n\
          dup /FontName get exch definefont pop\nmark currentfile closefile\n",
    );
    let encrypted = type1_cipher(&private, 55_665, &lead);
    let mut trailer = b"\n".to_vec();
    for _ in 0..8 {
        trailer.extend_from_slice(&[b'0'; 64]);
        trailer.push(b'\n');
    }
    trailer.extend_from_slice(b"cleartomark\n");

    let lengths = [clear.len(), encrypted.len(), trailer.len()];
    let mut program = clear;
    program.extend(encrypted);
    program.extend(trailer);
    (program, lengths, widths)
}

/// [`dense`]'s seeded lines, each its characters from 32 to 126 alone.
fn ascii_lines(pages: usize, seed: u64) -> Vec<Vec<Vec<u8>>> {
    dense_text(pages, seed)
        .into_iter()
        .map(|lines| {
            lines
                .iter()
                .map(|line| {
                    line.chars()
                        .filter(|c| (' '..='~').contains(c))
                        .map(|c| c as u8)
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// `pages` A4 pages of [`dense`]'s seeded lines, their ASCII characters
/// alone, in [`type1_program`] embedded whole under `/FontFile`, so
/// subset-on-save has a Type 1 program to cut, and to convert.
pub fn type1_pages(pages: usize, seed: u64) -> Vec<u8> {
    let (program, [one, two, three], widths) = type1_program();
    let widths: Vec<String> = widths.iter().map(u32::to_string).collect();
    let contents: Vec<Vec<u8>> = ascii_lines(pages, seed)
        .iter()
        .map(|lines| lines_content(lines))
        .collect();
    let (kids, page_objects) = raw_pages(6, "/Font << /F0 3 0 R >>", &contents);
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Count {pages} /Kids [{kids}] >>").into_bytes(),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /BenchType1 /FirstChar 32 \
             /LastChar 126 /Widths [{}] /FontDescriptor 4 0 R >>",
            widths.join(" ")
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /BenchType1 /Flags 32 \
          /FontBBox [0 -100 1000 900] /ItalicAngle 0 /Ascent 900 /Descent -100 \
          /CapHeight 700 /StemV 50 /FontFile 5 0 R >>"
            .to_vec(),
        raw_stream(
            &format!("/Length1 {one} /Length2 {two} /Length3 {three}"),
            &program,
        ),
    ];
    objects.extend(page_objects);
    raw_document(&objects)
}

/// `pages` A4 pages whose lines alternate between standard-14 Helvetica and
/// `/Arial`, an unembedded non-symbolic TrueType font under
/// `WinAnsiEncoding` whose every width is 600: the fonts no program embeds,
/// which the save door's embedding pass asks a provider for. Each line is
/// [`dense`]'s seeded words that are letters alone.
pub fn embed_pages(pages: usize, seed: u64) -> Vec<u8> {
    let contents: Vec<Vec<u8>> = dense_text(pages, seed)
        .iter()
        .map(|lines| {
            let mut content = b"BT 9 TL 36 806 Td\n".to_vec();
            for (at, line) in lines.iter().enumerate() {
                let words: Vec<&str> = line
                    .split(' ')
                    .filter(|word| {
                        !word.is_empty() && word.chars().all(|c| c.is_ascii_alphabetic())
                    })
                    .collect();
                let font = if at % 2 == 0 { "F0" } else { "F1" };
                content.extend_from_slice(
                    format!(
                        "/{font} 9 Tf {} Tj T*\n",
                        hex_string(words.join(" ").as_bytes())
                    )
                    .as_bytes(),
                );
            }
            content.extend_from_slice(b"ET\n");
            content
        })
        .collect();
    let (kids, page_objects) = raw_pages(6, "/Font << /F0 3 0 R /F1 4 0 R >>", &contents);
    let widths = vec!["600"; 122 - 32 + 1].join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Count {pages} /Kids [{kids}] >>").into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Arial /Encoding /WinAnsiEncoding \
             /FirstChar 32 /LastChar 122 /Widths [{widths}] /FontDescriptor 5 0 R >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /Arial /Flags 32 /FontBBox [0 -200 1000 800] \
          /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 >>"
            .to_vec(),
    ];
    objects.extend(page_objects);
    raw_document(&objects)
}
