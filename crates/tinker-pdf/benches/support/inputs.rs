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

use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, DeviceSpace, Document, DocumentBuilder,
    Encryption, Function, ImageData, PageBuilder, Shading, WriteMode, WriteOptions,
};
use tinker_pdf_cos::edit::annot::{self, Color};
use tinker_pdf_cos::pages::Rect;
use tinker_pdf_cos::write::StreamData;
use tinker_pdf_cos::{Dict, ObjRef, Object};

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

/// One of the books committed under `crates/tinker-pdf/tests/epub/`, by its
/// file name.
pub fn committed_book(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/epub/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("the committed book {path}: {error}"))
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
