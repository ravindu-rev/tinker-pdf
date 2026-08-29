//! The source documents `tests/xps/make-gs-corpus.ps1` converts (Tier 4, the
//! XPS second-producer row).
//!
//! Every package under `tests/xps/` before this file was written by one of two
//! Microsoft serialisers, and `tests/xps/README.md` recorded the debt that this
//! was *a single vendor's* idea of the format. Closing it needs a second
//! producer, and a second producer needs something to convert — so these five
//! documents are written by `DocumentBuilder`, in this repository, and handed
//! to Ghostscript's `xpswrite` device. Our content through their tool, which is
//! the shape ruling 13 admits and the one `tests/xps/README.md` already argues
//! for over the WPF corpus.
//!
//! # Why an `#[ignore]`d test rather than a script
//!
//! The same reason `docs/verification.md` gives for the six fuzz seed corpora
//! *"written by an `#[ignore]`d test in the crate that owns the fixtures, so
//! the seeds and the fixtures cannot drift"*: the writer that produces a
//! fixture and the writer the suite tests have to be one writer. A PowerShell
//! script that assembled these PDFs by hand would be a second implementation of
//! `DocumentBuilder`, kept in step by nobody.
//!
//! ```text
//! cargo test -p tinker-pdf --test xps_corpus_source -- --ignored --nocapture
//! ```
//!
//! It writes into `tests/xps/source/`, which **is** committed, and the bytes it
//! writes are the same bytes every time — ruling 4's contract, applied to the
//! writer rather than to the rasterizer. That matters more here than it would
//! anywhere else in this tree: Ghostscript stamps a fixed timestamp on every
//! ZIP entry and stores rather than deflates, so a `gs-*.xps` is byte-identical
//! across runs, and with a byte-identical input the whole corpus regenerates
//! byte for byte. No other container corpus in this repository does — both EPUB
//! producers mint a fresh UUID per run and both XPS serialisers mint a fresh
//! GUID per resource part. See `tests/xps/README.md`.
//!
//! # What each one is for
//!
//! | Document | The question it asks of `xpswrite` |
//! | --- | --- |
//! | `paths.pdf` | do filled paths keep their colours, and what becomes of an even-odd fill |
//! | `gradients.pdf` | is an axial or radial shading carried as a gradient brush, or flattened |
//! | `images.pdf` | does a Flate image and a DCT image survive as PNG and JPEG parts, or is either re-encoded |
//! | `embedded-font.pdf` | does an embedded TrueType face reach the package as an **unobfuscated** `.ttf` part |
//! | `rasterised-text.pdf` | what a base-14 font — which Ghostscript substitutes from its own ROM — costs in markup |
//!
//! The last two are a pair on purpose. `[Content_Types].xml` in every package
//! Ghostscript writes declares `.ttf` as `application/vnd.ms-opentype`, the
//! **un**obfuscated media type, and no committed package had ever carried one:
//! all six WPF packages and both OpenXPS ones use 9.1.7.3's ODTTF obfuscation,
//! so `xps/font.rs`'s unobfuscated arm had never been taken by a real file.

use std::path::{Path, PathBuf};

use tinker_pdf::{DeviceSpace, DocumentBuilder, Function, ImageData, PageBuilder, Shading};

/// The page every source document is, in points.
///
/// Not US Letter, and deliberately: 400 pt is 533.333 XPS units and `xpswrite`
/// writes `Width="533"`, which is the truncation `tests/xps/README.md` records.
/// A page size that happened to be a whole number of units would have hidden
/// it.
const PAGE: (f64, f64) = (400.0, 300.0);

fn source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/xps/source")
}

/// A Liberation face, from the copy `tinker-pdf-font` vendors.
///
/// OFL-1.1, on `deny.toml`'s allowlist and in `THIRDPARTY.md`, and — the part
/// that decides it for a committed document — the OFL's own *"The requirement
/// for fonts to remain under this license does not apply to any document
/// created using the Font Software"*. That is the identical argument
/// `tests/xps/README.md` makes for Cascadia Mono in the WPF corpus.
fn liberation(face: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tinker-pdf-font/data/liberation")
        .join(face);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// A 32 x 32 RGB image: four coloured quadrants under a white diagonal.
///
/// The same subject the WPF corpus carries, drawn here instead of by
/// `RenderTargetBitmap`, so the two producers' packages hold the same picture
/// and a difference between them is the producer's.
fn quadrants() -> Vec<u8> {
    const SIZE: usize = 32;
    let colours = [
        [220u8, 20, 60], // crimson
        [70, 130, 180],  // steel blue
        [218, 165, 32],  // goldenrod
        [46, 139, 87],   // sea green
    ];
    let mut data = Vec::with_capacity(SIZE * SIZE * 3);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let quadrant = usize::from(y >= SIZE / 2) * 2 + usize::from(x >= SIZE / 2);
            // A diagonal two pixels wide, which survives any resampling a
            // converter might do on the way into a package.
            let pixel = if x.abs_diff(y) < 2 {
                [255u8, 255, 255]
            } else {
                colours[quadrant]
            };
            data.extend_from_slice(&pixel);
        }
    }
    data
}

/// A baseline grey JPEG, DC only, every block the same dark value.
///
/// Written out here rather than borrowed from `cbz_support`, because this file
/// asks what a *converter* does with a DCT stream and the stream has to be
/// visible in the same place as the question. `width` and `height` are
/// multiples of eight. Two DC codes are defined so a multi-block image can say
/// "no change" after the first.
fn grey_jpeg(width: u16, height: u16) -> Vec<u8> {
    struct Bits {
        out: Vec<u8>,
        byte: u8,
        used: u32,
    }
    impl Bits {
        fn push(&mut self, bits: &str) {
            for c in bits.chars() {
                self.byte = (self.byte << 1) | u8::from(c == '1');
                self.used += 1;
                if self.used == 8 {
                    self.out.push(self.byte);
                    if self.byte == 0xFF {
                        self.out.push(0x00);
                    }
                    self.byte = 0;
                    self.used = 0;
                }
            }
        }
    }

    let mut out = vec![0xFF, 0xD8];

    // DQT, table 0. A large DC quantiser, so the one coded coefficient is a
    // value nobody could mistake for a blank page.
    let mut quant = [1u8; 64];
    quant[0] = 255;
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend_from_slice(&quant);

    // SOF0: baseline, 8-bit, one component, no subsampling, table 0.
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);

    // DHT DC table 0: two two-bit codes, `00` for size 0 and `01` for size 2.
    let mut counts = [0u8; 16];
    counts[1] = 2;
    let mut dht = vec![0x00];
    dht.extend_from_slice(&counts);
    dht.extend_from_slice(&[0x00, 0x02]);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);

    // DHT AC table 0: one one-bit code, end-of-block.
    let mut counts = [0u8; 16];
    counts[0] = 1;
    let mut dht = vec![0x10];
    dht.extend_from_slice(&counts);
    dht.push(0x00);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);

    // SOS: one component, the whole spectral band, no successive
    // approximation, which is what makes it sequential rather than progressive.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);

    let blocks = (usize::from(width).div_ceil(8)) * (usize::from(height).div_ceil(8));
    let mut bits = Bits {
        out: Vec::new(),
        byte: 0,
        used: 0,
    };
    for block in 0..blocks {
        if block == 0 {
            // Size 2, then the two bits `00`, which F.1.2.1's table reads as
            // -3: a DC of -765 after dequantisation, which the level shift
            // puts near 32.
            bits.push("01");
            bits.push("00");
        } else {
            bits.push("00");
        }
        bits.push("0");
    }
    while bits.used != 0 {
        bits.push("1");
    }
    out.extend_from_slice(&bits.out);

    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

// ---- the five documents -----------------------------------------------------

/// Filled paths in five colours, one of them not a rectangle and one of them an
/// even-odd fill.
///
/// The even-odd fill is the interesting one: `f*` over two concentric subpaths
/// is an annulus, a shape with a hole in it, and 11.2.3's abbreviated geometry
/// spells a hole with `F 0`. The same two subpaths are drawn again beside it
/// under the nonzero rule, so the package states both and a reader that ignored
/// the difference makes the two shapes identical.
fn paths() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(PAGE.0, PAGE.1, |page: &mut PageBuilder| {
        page.set_fill_rgb(1.0, 1.0, 1.0);
        page.raw(b"0 0 400 300 re f");

        page.set_fill_rgb(0.86, 0.08, 0.24); // crimson
        page.raw(b"20 200 100 60 re f");
        page.set_fill_rgb(0.27, 0.51, 0.71); // steel blue
        page.raw(b"140 200 100 60 re f");

        // A triangle: three points, closed, non-rectangular, so the package
        // cannot express it as an axis-aligned box however hard it tries.
        page.set_fill_rgb(0.18, 0.55, 0.34); // sea green
        page.raw(b"260 200 m 380 200 l 320 270 l h f");

        page.set_fill_rgb(0.85, 0.65, 0.13); // goldenrod
        page.raw(b"40 40 m 160 40 l 160 160 l 40 160 l h 70 70 m 130 70 l 130 130 l 70 130 l h f*");

        page.set_fill_rgb(0.29, 0.0, 0.51); // indigo
        page.raw(
            b"220 40 m 340 40 l 340 160 l 220 160 l h 250 70 m 310 70 l 310 130 l 250 130 l h f",
        );
    });
    builder.finish()
}

/// An axial and a radial shading, each clipped to its own rectangle.
///
/// `sh` fills the clip rather than a shape, so each is preceded by a `W n` —
/// which is also the only way this writer paints a gradient at all.
///
/// **The two are deliberately different shapes of the same question**, and the
/// difference is what makes the answer a measurement rather than an anecdote.
/// The axial one runs **down** the page, so every scanline of it is one colour;
/// the radial one is not constant along any line at all. A converter that keeps
/// gradients as gradients cannot tell them apart. One that flattens them into
/// filled paths has to pay for the second in a way it does not for the first,
/// and the two path counts in `tests/xps/CONSERVATION.tsv` are that bill.
///
/// The radial's square is 24 pt on a side and not the 160 pt an eye would want,
/// for that reason and no other: at 160 pt this one package was 5.3 MB, which
/// is thirty times the whole corpus before it.
fn gradients() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    // Three stops, so the axial one is a type 3 stitching function over two
    // type 2 ramps and not a single interpolation. A converter that took only
    // the endpoints loses the middle colour.
    let axial = Shading::Axial {
        color_space: DeviceSpace::Rgb,
        coords: [0.0, 280.0, 0.0, 240.0],
        function: Function::Stitching {
            domain: [0.0, 1.0],
            functions: vec![
                Function::Exponential {
                    domain: [0.0, 1.0],
                    c0: vec![0.86, 0.08, 0.24],
                    c1: vec![1.0, 0.84, 0.0],
                    n: 1.0,
                },
                Function::Exponential {
                    domain: [0.0, 1.0],
                    c0: vec![1.0, 0.84, 0.0],
                    c1: vec![0.18, 0.55, 0.34],
                    n: 1.0,
                },
            ],
            bounds: vec![0.5],
            encode: vec![[0.0, 1.0], [0.0, 1.0]],
        },
        extend: (true, true),
    };
    assert!(builder.add_shading(b"Sh0", &axial), "the axial shading");
    let radial = Shading::Radial {
        color_space: DeviceSpace::Rgb,
        coords: [280.0, 100.0, 0.0, 280.0, 100.0, 8.0],
        function: Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![1.0, 1.0, 1.0],
            c1: vec![0.1, 0.1, 0.44],
            n: 1.0,
        },
        extend: (false, false),
    };
    assert!(builder.add_shading(b"Sh1", &radial), "the radial shading");
    builder.add_page(PAGE.0, PAGE.1, |page: &mut PageBuilder| {
        page.set_fill_rgb(1.0, 1.0, 1.0);
        page.raw(b"0 0 400 300 re f");
        page.raw(b"q 20 240 80 40 re W n");
        assert!(page.shading(b"Sh0"), "the axial shading is painted");
        page.raw(b"Q");
        page.raw(b"q 272 92 16 16 re W n");
        assert!(page.shading(b"Sh1"), "the radial shading is painted");
        page.raw(b"Q");
    });
    builder.finish()
}

/// One Flate image and one DCT image, on one page.
///
/// The question is what the parts come out as. A `/DCTDecode` stream can be
/// copied into a `.jpg` part untouched — WPF's is — and a `/FlateDecode` image
/// has no part media type of its own, so whatever the converter picks for it is
/// a decision this corpus records rather than one ECMA-388 makes.
fn images() -> Vec<u8> {
    let pixels = quadrants();
    let jpeg = grey_jpeg(32, 32);
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 32,
                height: 32,
                data: &pixels,
            },
        ),
        "the RGB image"
    );
    assert!(
        builder.add_image(b"Im1", &ImageData::Jpeg(&jpeg)),
        "the JPEG image"
    );
    builder.add_page(PAGE.0, PAGE.1, |page: &mut PageBuilder| {
        page.set_fill_rgb(1.0, 1.0, 1.0);
        page.raw(b"0 0 400 300 re f");
        page.image(b"Im0", 30.0, 60.0, 160.0, 160.0);
        page.image(b"Im1", 220.0, 60.0, 160.0, 160.0);
    });
    builder.finish()
}

/// The three lines both text documents set, character for character.
///
/// One constant rather than two literals, because the pair is a controlled
/// experiment and the control is the text: the *only* difference between
/// `embedded-font.pdf` and `rasterised-text.pdf` is whether a font program is
/// embedded, so any difference between the two packages is that difference's.
const LINES: [(f64, f64, &str); 3] = [
    (16.0, 230.0, "One face, three"),
    (16.0, 200.0, "lines, and the"),
    (16.0, 170.0, "same text twice."),
];

/// The three lines in an embedded Liberation Serif.
///
/// The face is embedded so the converter has one to carry, and the question the
/// package answers is what part it lands in: `[Content_Types].xml` declares
/// `.ttf` as `application/vnd.ms-opentype` — the **un**obfuscated media type —
/// and no package in this corpus had ever carried one, because all eight
/// Microsoft packages obfuscate under 9.1.7.3.
fn embedded_font() -> Vec<u8> {
    let program = liberation("LiberationSerif-Regular.ttf");
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_embedded_font(b"F0", b"LiberationSerif", &program),
        "Liberation Serif embeds"
    );
    builder.add_page(PAGE.0, PAGE.1, |page: &mut PageBuilder| {
        page.set_fill_rgb(1.0, 1.0, 1.0);
        page.raw(b"0 0 400 300 re f");
        page.set_fill_rgb(0.0, 0.0, 0.0);
        for (size, y, line) in LINES {
            page.text(b"F0", size, 30.0, y, line);
        }
    });
    builder.finish()
}

/// The same three lines in Helvetica, which is a name and not a program.
///
/// Nothing is embedded, so the converter substitutes from its own resources —
/// and what it does then is the finding. Kept as its own document rather than
/// folded into the one above, because the two differ in exactly one thing and
/// the pair is the measurement.
fn rasterised_text() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(PAGE.0, PAGE.1, |page: &mut PageBuilder| {
        page.set_fill_rgb(1.0, 1.0, 1.0);
        page.raw(b"0 0 400 300 re f");
        page.set_fill_rgb(0.0, 0.0, 0.0);
        for (size, y, line) in LINES {
            page.text(b"F0", size, 30.0, y, line);
        }
    });
    builder.finish()
}

/// Every source document, by the name `make-gs-corpus.ps1` looks for.
fn documents() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("paths.pdf", paths()),
        ("gradients.pdf", gradients()),
        ("images.pdf", images()),
        ("embedded-font.pdf", embedded_font()),
        ("rasterised-text.pdf", rasterised_text()),
    ]
}

// ---- the two things that are asserted on every run --------------------------

/// **The committed sources are the ones this writer produces.**
///
/// Not `#[ignore]`d, and that is the point: the fixture writer below can only
/// be run deliberately, so without this the sources in `tests/xps/source/`
/// could drift from `DocumentBuilder` and the first thing to notice would be a
/// regenerated package that no longer matched `INVENTORY.tsv`. This says so at
/// the source instead.
///
/// It is also the check behind the reproducibility claim in
/// `tests/xps/README.md`: a `gs-*.xps` regenerates byte for byte only if its
/// input does, and this is what says the input does.
#[test]
fn the_committed_sources_are_what_this_writer_writes_today() {
    for (name, bytes) in documents() {
        let path = source_dir().join(name);
        let committed = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e} — run the writer below", path.display()));
        assert_eq!(
            committed.len(),
            bytes.len(),
            "{name}: the committed source is {} bytes and this writer produces {}",
            committed.len(),
            bytes.len()
        );
        assert!(
            committed == bytes,
            "{name}: the committed source is not what `DocumentBuilder` writes today. \
             Re-run the writer, re-run `make-gs-corpus.ps1`, and re-measure \
             INVENTORY.tsv and CONSERVATION.tsv in the same commit."
        );
    }
}

/// Each source document is a document this repository can read back.
///
/// A source this engine could not open would make every finding about the
/// package downstream of it a finding about a broken PDF instead.
#[test]
fn every_source_document_opens() {
    for (name, bytes) in documents() {
        let document = tinker_pdf::Document::open(bytes)
            .unwrap_or_else(|e| panic!("{name} does not open: {e:?}"));
        assert_eq!(document.page_count(), 1, "{name}: one page");
        let page = document.page(0).unwrap_or_else(|| panic!("{name}: page 0"));
        assert_eq!(page.size(), PAGE, "{name}: the page size");
    }
}

/// Writes `tests/xps/source/*.pdf`, which is committed.
///
/// Run with `--ignored` when a source document changes. Everything downstream
/// changes with it: re-run `make-gs-corpus.ps1`, then `inventory.ps1`, then the
/// conservation sweep, and update the hashes in `tests/xps/README.md` — the
/// test above is what fails until all of that has happened.
#[test]
#[ignore = "writes into tests/xps/source, which is committed"]
fn write_the_source_documents() {
    let dir = source_dir();
    std::fs::create_dir_all(&dir).expect("a source directory");
    for (name, bytes) in documents() {
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
        println!("wrote {name:<24} {:>8} bytes", bytes.len());
    }
}
