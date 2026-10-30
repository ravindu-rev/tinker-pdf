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

use std::sync::{Arc, OnceLock};

use tinker_pdf::write::{self, SaveOptions};
use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, BilevelCodec, CcittParams, CompressedImage,
    ContinuousCodec, DeviceSpace, Document, DocumentBuilder, Encryption, Function, ImageColorSpace,
    ImageData, ImageFilter, ImagePolicy, ImageRecoding, OpenOptions, PageBox, PageBuilder,
    RenderOptions, Shading, TilingPattern, TilingType, WriteMode, WriteOptions,
};
use tinker_pdf_cos::edit::annot::{self, Color};
use tinker_pdf_cos::pages::Rect;
use tinker_pdf_cos::write::StreamData;
use tinker_pdf_cos::{Dict, ObjRef, Object};
use tinker_pdf_crypto::bignum::{Modulus, Uint};
use tinker_pdf_crypto::rsa::{DigestAlgorithm, RSA_LIMBS};
use tinker_pdf_crypto::sha2::sha256;
use tinker_pdf_font::Sfnt;

/// The tests' EPUB support, for its OCF writer: the generated books go
/// through the same writer as the tests' own.
#[path = "../../tests/epub_support/mod.rs"]
mod epub_support;

use epub_support::{ocf_zip, OcfEntry, OCF_MEDIA_TYPE};

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
    let mut builder = DocumentBuilder::new();
    let shading = Shading::Axial {
        color_space: DeviceSpace::Rgb,
        coords: [0.0, 0.0, 595.0, 842.0],
        function: Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.0, 0.1, 0.6],
            c1: vec![1.0, 0.9, 0.2],
            n: 1.0,
        },
        extend: (true, true),
    };
    builder.add_shading(b"S0", &shading);
    builder.add_page(595.0, 842.0, |page| {
        page.shading(b"S0");
    });
    builder.finish()
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

/// Where one of the books committed under `crates/tinker-pdf/tests/epub/`
/// is, by its file name.
pub fn committed_book_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/epub")
        .join(name)
}

/// One of the books committed under `crates/tinker-pdf/tests/epub/`, by its
/// file name.
pub fn committed_book(name: &str) -> Vec<u8> {
    let path = committed_book_path(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("the committed book {}: {error}", path.display()))
}

/// The integer stream every seeded input draws from: SplitMix64, so an input
/// is the same bytes on every target.
pub struct Seeded(u64);

impl Seeded {
    /// The stream for `seed`.
    pub fn new(seed: u64) -> Seeded {
        Seeded(seed)
    }

    /// The next 64 bits.
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `bound`, which is at least one.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
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
    let path = format!(
        "{}/../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("the committed face {path}: {error}"))
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

/// `width` by `height` RGB samples by formula: smooth fields, hard-edged
/// shapes and a few levels of seeded noise, the photographic content of
/// every image input.
pub fn photo(width: u32, height: u32, seed: u64) -> Vec<u8> {
    let mut noise = Seeded::new(seed);
    let (w, h) = (u64::from(width), u64::from(height));
    // Eight rectangles and discs, placed and coloured by the seed.
    let shapes: Vec<(u64, u64, u64, [u8; 3], bool)> = (0..8)
        .map(|_| {
            let cx = noise.next() % w;
            let cy = noise.next() % h;
            let r = w / 16 + noise.next() % (w / 5);
            let colour = [noise.next() as u8, noise.next() as u8, noise.next() as u8];
            (cx, cy, r, colour, noise.next() % 2 == 0)
        })
        .collect();
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut pixel = [
                (x * 255 / w) as u8,
                (y * 255 / h) as u8,
                ((x + y) * 255 / (w + h)) as u8,
            ];
            for (cx, cy, r, colour, disc) in &shapes {
                let (dx, dy) = (x.abs_diff(*cx), y.abs_diff(*cy));
                let inside = if *disc {
                    dx * dx + dy * dy <= r * r
                } else {
                    dx <= *r && dy <= *r / 2
                };
                if inside {
                    pixel = *colour;
                }
            }
            let grain = noise.next();
            for (at, sample) in pixel.iter_mut().enumerate() {
                let level = ((grain >> (8 * at)) & 7) as u8;
                *sample = sample.saturating_add(level).saturating_sub(4);
            }
            out.extend_from_slice(&pixel);
        }
    }
    out
}

/// At least 23 000 000 bytes of A4 pages, each covered by an image of its
/// own, `photo(2480, 3508, page)` coded as a baseline JPEG, so no two pages
/// share an image a writer could deduplicate. Written once a process.
pub fn image_file() -> Arc<[u8]> {
    static FILE: OnceLock<Arc<[u8]>> = OnceLock::new();
    Arc::clone(FILE.get_or_init(|| {
        let mut builder = DocumentBuilder::new();
        let mut held = 0usize;
        let mut page = 0u64;
        while held < 23_000_000 {
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
            file.len() >= 23_000_000,
            "the image file is {} bytes",
            file.len()
        );
        file.into()
    }))
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
    let path = format!(
        "{}/../../fuzz/corpus/icc_profile/cmyk-lut.icc",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("the committed profile {path}: {error}"))
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

/// The bytes of [`page_like`] and [`photo_like`]: 1240 by 1755 RGB.
pub const PAGE_BYTES: usize = 1240 * 1755 * 3;

/// 1240 by 1755 RGB of text-like marks on white, about a tenth of the
/// pixels inked: lines of glyph-sized boxes in words, each box drawn in a
/// stroke pattern, by formula — never a render, so it cannot move with the
/// renderer.
pub fn page_like() -> Vec<u8> {
    let (width, height) = (1240usize, 1755usize);
    let mut out = vec![255u8; PAGE_BYTES];
    let mut seed = Seeded::new(11);
    let mut top = 60;
    while top + 24 < height - 60 {
        let mut x = 80;
        while x + 12 < width - 80 {
            for _ in 0..2 + seed.below(8) {
                let (glyph, tall) = (5 + seed.below(6), 10 + seed.below(6));
                for y in top + 16 - tall..top + 16 {
                    for column in x..(x + glyph).min(width - 80) {
                        if (column + 2 * y) % 4 == 0 {
                            let at = (y * width + column) * 3;
                            out[at..at + 3].copy_from_slice(&[24, 24, 32]);
                        }
                    }
                }
                x += glyph + 2;
            }
            x += 8;
        }
        top += 24;
    }
    out
}

/// [`photo`] at 1240 by 1755, seed 0: the photo-like data the encode and
/// decode groups compress.
pub fn photo_like() -> Vec<u8> {
    photo(1240, 1755, 0)
}

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
        // The source `data/progressive-2480x3508.jpg` was transcoded from, as
        // `data/README.md` records it: a changed encoder fails here, before
        // the decode group compares the two.
        assert_eq!(
            hex(&sha256(&jpeg)),
            "754c262d187b8459117b897f5d79c97d71062c58ed6ff494b82438d8ed8dcbbc",
            "baseline_jpeg() is the progressive file's source"
        );
        jpeg.into()
    }))
}

/// `bytes` as lower-case hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One of the codestreams committed under `benches/data/`, by its name.
pub fn committed_data(name: &str) -> Vec<u8> {
    let path = format!("{}/benches/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("the committed file {path}: {error}"))
}

/// A scan: the dense text page rendered at 300 dpi and thresholded to one
/// bit a pixel, rows packed from the most significant bit, **0 for black**
/// as a one-bit `/DeviceGray` sample is. Its width, height and bytes.
pub fn scan() -> (u32, u32, Arc<[u8]>) {
    static SCAN_PAGE: OnceLock<(u32, u32, Arc<[u8]>)> = OnceLock::new();
    let (width, height, bytes) = SCAN_PAGE.get_or_init(|| {
        let doc = Document::open(dense(1, 0)).expect("the text page opens");
        let bitmap = doc
            .page(0)
            .expect("a page")
            .render(&RenderOptions::at_dpi(300.0));
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
    });
    (*width, *height, Arc::clone(bytes))
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
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", image), "the image is placed");
    builder.add_page(595.0, 842.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 595.0, 842.0)
    });
    builder.finish()
}

/// An A4 page covered by [`baseline_jpeg`].
pub fn jpeg_page() -> Vec<u8> {
    image_page(&ImageData::Jpeg(&baseline_jpeg()))
}

/// An A4 page covered by [`photo`] at [`SCAN`]'s size, seed 1, Flate-coded.
pub fn flate_page() -> Vec<u8> {
    let coded = tinker_pdf_filters::zlib_compress(&photo(SCAN.0, SCAN.1, 1));
    image_page(&ImageData::Compressed(CompressedImage {
        width: SCAN.0,
        height: SCAN.1,
        bits_per_component: 8,
        color_space: ImageColorSpace::DeviceRgb,
        filter: Some(ImageFilter::Flate),
        data: &coded,
        color_key_mask: None,
        soft_mask: None,
    }))
}

/// An A4 page covered by [`photo`] at [`SCAN`]'s size, seed 0, turned into
/// ink by formula — black the darkest of the three, each colour the rest of
/// its complement — Flate-coded under `/ICCBased` over `cmyk-lut.icc`.
pub fn cmyk_page() -> Vec<u8> {
    let rgb = photo(SCAN.0, SCAN.1, 0);
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
            width: SCAN.0,
            height: SCAN.1,
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
    builder.add_page(595.0, 842.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 595.0, 842.0)
    });
    builder.finish()
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

/// An A4 page covered by [`scan`], G4-coded.
pub fn g4_page() -> Vec<u8> {
    let (width, height, bits) = scan();
    let coded = g4(width, height, &bits);
    image_page(&ImageData::Compressed(CompressedImage {
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
    }))
}

/// An A4 page covered by [`scan`], saved through the image pass's JBIG2
/// generic coding, the builder having no JBIG2 filter of its own.
///
/// The scan is placed as raw one-bit samples rather than as [`g4_page`]'s G4,
/// because the pass keeps a new coding only when it is smaller than the
/// stored one, and a generic region need not beat G4.
pub fn jbig2_page() -> Vec<u8> {
    let (width, height, bits) = scan();
    let raw = image_page(&ImageData::Compressed(CompressedImage {
        width,
        height,
        bits_per_component: 1,
        color_space: ImageColorSpace::DeviceGray,
        filter: None,
        data: &bits,
        color_key_mask: None,
        soft_mask: None,
    }));
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

/// Example 10's key, read from its block's private-key section, where
/// `Modulus` is `n`, `Public exponent` is `e` and `Exponent` is `d` — the
/// label the public-key section above it gives `e`.
pub fn example_10() -> RsaKey {
    let path = format!(
        "{}/../tinker-pdf-crypto/tests/data/pkcs1/pss-vect.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the committed vectors {path}: {error}"));
    let block = text.split("# Example 10:").nth(1).expect("Example 10");
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

/// [`MESSAGE_BYTES`] seeded bytes, and their RSA-2048 PKCS #1 v1.5 signature
/// under [`example_10`] with SHA-256, made here: the crypto crate has no
/// signing path, so the signature is [`pkcs1_encoded`]'s representative
/// through [`rsa_private`].
pub fn message() -> (Vec<u8>, Vec<u8>) {
    let mut seed = Seeded::new(17);
    let mut bytes = Vec::with_capacity(MESSAGE_BYTES + 8);
    while bytes.len() < MESSAGE_BYTES {
        bytes.extend_from_slice(&seed.next().to_le_bytes());
    }
    bytes.truncate(MESSAGE_BYTES);
    let key = example_10();
    let signature = rsa_private(&key, &pkcs1_encoded(&key, &bytes));
    (bytes, signature)
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
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir),
        &mut paths,
    );
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
    let path = format!(
        "{}/../tinker-pdf-font/data/liberation/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("the committed face {path}: {error}"))
}

/// A face under `tinker-pdf-shape/data/`, by its path there.
pub fn shape_face(path: &str) -> Vec<u8> {
    let path = format!(
        "{}/../tinker-pdf-shape/data/{path}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("the committed face {path}: {error}"))
}

/// Every committed CFF program, by file name: the four bare ones the `cff`
/// fuzz target seeds with, and the `CFF ` table of every committed OpenType
/// face — aots's 185, text-rendering-tests' three and the `sfnt` target's
/// `opentype-cff.otf`.
pub fn cff_programs() -> Vec<(String, Vec<u8>)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    let mut read = |dir: &str, extension: &str, whole: bool| {
        let mut names: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(dir))
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
            out.push((name, program));
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
