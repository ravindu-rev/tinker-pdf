//! Word boxes and their TSV, held to the published definition
//! `docs/design/word-boxes.md` transcribes (ROADMAP row FT-24).
//!
//! The pages are built the way `text_logical_order.rs` builds its pages: a
//! face synthesised to cover exactly what they draw (`epub_support::typeface`)
//! and `DocumentBuilder::glyph_run`, which places every glyph, so a
//! right-to-left line drawn in visual order and a line turned by its run's
//! matrix are what the test says they are.
//!
//! Every expectation is computed here from what [`tinker_pdf::Page::text`]
//! reports, never from the writer: a word is the run of a line's characters
//! between spaces — which on these lines is the design's word, each of
//! `TextLine::words` with the punctuation it touches — its box the union of
//! its characters' quads along the line's baseline, carried into the frame by
//! the page's crop box and `/Rotate` with the arithmetic written out below,
//! enclosed, clipped to the page and rounded outward. At 72 and 144 dpi that
//! arithmetic is the transform's to the last bit, since it scales by one or
//! two, so a whole-pixel box agrees exactly or not at all.

mod epub_support;
mod written_support;

use epub_support::typeface::Face;
use tinker_pdf::{
    Document, LineDirection, Quad, TextLine, WordBlock, WordBox, WordBoxOptions, WordBoxes,
    WordLine, WordPage, WordParagraph, WriteOptions,
};
use tinker_pdf_cos::build::{DocumentBuilder, Glyph, PlacedGlyph};

/// The TSV header as the published definition writes it: these twelve names,
/// in this order, one tab between each two.
const PUBLISHED_HEADER: [&str; 12] = [
    "level",
    "page_num",
    "block_num",
    "par_num",
    "line_num",
    "word_num",
    "left",
    "top",
    "width",
    "height",
    "conf",
    "text",
];

/// Shin, lamed, vav, final mem: `shalom`, as it is read.
const SHALOM: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
/// Ayin, vav, lamed, final mem: `olam`, as it is read.
const OLAM: &str = "\u{5E2}\u{5D5}\u{5DC}\u{5DD}";

/// Everything any page here draws.
const COVERS: &str =
    "Tabcdefghijklmnopqrstuvwxyz ().,!0123456789\u{5E9}\u{5DC}\u{5D5}\u{5DD}\u{5E2}";

/// The font size, and so the advance of every glyph: the face's 500 units of
/// a 1000-unit em at twenty points is ten.
const SIZE: f64 = 20.0;
const ADVANCE: f64 = 10.0;

/// The page every fixture draws on, in points.
const PAGE: (f64, f64) = (400.0, 300.0);

/// The rotated page's crop box: offset from the media box on both axes.
const CROP: (f64, f64, f64, f64) = (10.0, 20.0, 390.0, 290.0);

/// One run: `drawn`'s characters laid left to right one advance apart, in
/// the space `matrix` places.
struct Run {
    drawn: String,
    matrix: [f64; 6],
}

/// An upright run whose baseline starts at `(x, y)`.
fn upright_run(drawn: &str, x: f64, y: f64) -> Run {
    Run {
        drawn: drawn.to_owned(),
        matrix: [1.0, 0.0, 0.0, 1.0, x, y],
    }
}

/// The left-to-right page: two lines of words that carry their punctuation,
/// and a third turned thirty degrees.
fn latin_runs() -> Vec<Run> {
    let (sin, cos) = (0.5, 0.75f64.sqrt());
    vec![
        upright_run("The (quick) fox, jumps!", 30.0, 250.0),
        upright_run("over 43.5 dogs.", 30.0, 225.0),
        Run {
            drawn: "turned, words!".to_owned(),
            matrix: [cos, sin, -sin, cos, 60.0, 60.0],
        },
    ]
}

/// The right-to-left page: `shalom olam.` drawn in visual order, the full
/// stop leftmost.
fn hebrew_runs() -> Vec<Run> {
    let logical = format!("{SHALOM} {OLAM}.");
    let visual: String = logical.chars().rev().collect();
    vec![upright_run(&visual, 40.0, 150.0)]
}

/// A document of one page per entry of `pages`, each drawing its runs.
fn document(pages: &[Vec<Run>]) -> Vec<u8> {
    let face = Face::new("Fixture Words", COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureWords", &program));
    assert!(builder.set_info(b"CreationDate", "D:20260102030405Z"));
    for runs in pages {
        let mut content = Vec::new();
        for run in runs {
            let texts: Vec<String> = run.drawn.chars().map(String::from).collect();
            let glyphs: Vec<PlacedGlyph<'_>> = run
                .drawn
                .chars()
                .zip(texts.iter())
                .enumerate()
                .map(|(i, (ch, text))| PlacedGlyph {
                    glyph: Glyph {
                        id: face
                            .glyph_of(ch)
                            .expect("the face covers what the page draws"),
                        text,
                    },
                    x: i as f64 * ADVANCE,
                    rise: 0.0,
                })
                .collect();
            assert!(builder.glyph_run(&mut content, b"F0", SIZE, run.matrix, &glyphs));
        }
        builder.add_page(PAGE.0, PAGE.1, |page| page.raw(&content));
    }
    built("document", builder)
}

/// Three pages: the left-to-right page, the right-to-left page, and the
/// left-to-right page again under `/Rotate 90` and a crop box.
fn fixture() -> Document {
    let bytes = document(&[latin_runs(), hebrew_runs(), latin_runs()]);
    let built = converted("fixture", Document::open(bytes).expect("the fixture opens"));
    let mut editor = built.editor();
    assert!(editor.rotate_page(2, 90), "the third page turns");
    assert!(
        editor.set_crop_box(2, CROP.0, CROP.1, CROP.2, CROP.3),
        "the third page crops"
    );
    let saved = editor.save(&WriteOptions::default());
    written_support::rewritten(
        "the turned and cropped fixture",
        built.cos().bytes(),
        &saved,
    );
    Document::open(saved).expect("the edited fixture opens")
}

// ---- the expectation, from the text model --------------------------------

/// Where a point of user space lands in the frame: the page as displayed —
/// crop box and `/Rotate`, turning clockwise (7.7.3.3) — from its top-left
/// corner with `y` down, at `scale` pixels a point.
fn to_frame(
    (x0, y0, x1, y1): (f64, f64, f64, f64),
    rotation: u16,
    scale: f64,
    (x, y): (f64, f64),
) -> (f64, f64) {
    let (u, v) = match rotation {
        0 => (x - x0, y1 - y),
        90 => (y - y0, x - x0),
        180 => (x1 - x, y - y0),
        270 => (y1 - y, x1 - x),
        other => panic!("no page here turns {other} degrees"),
    };
    (u * scale, v * scale)
}

/// The smallest quad holding `quads` with sides along and across the first
/// one's baseline.
fn union_along_baseline(quads: &[Quad]) -> Quad {
    let first = quads.first().expect("a character");
    let (dx, dy) = (first.lr.0 - first.ll.0, first.lr.1 - first.ll.1);
    let length = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / length, dy / length);
    let (vx, vy) = (-uy, ux);
    let (mut s0, mut s1, mut t0, mut t1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for quad in quads {
        for (x, y) in [quad.ll, quad.lr, quad.ul, quad.ur] {
            let (s, t) = (x * ux + y * uy, x * vx + y * vy);
            s0 = s0.min(s);
            s1 = s1.max(s);
            t0 = t0.min(t);
            t1 = t1.max(t);
        }
    }
    let point = |s: f64, t: f64| (s * ux + t * vx, s * uy + t * vy);
    Quad {
        ll: point(s0, t0),
        lr: point(s1, t0),
        ul: point(s0, t1),
        ur: point(s1, t1),
    }
}

/// `quad`'s TSV box in a frame of `width` by `height`: its corners carried by
/// `carry`, enclosed, clipped to the page and rounded outward, as `left`,
/// `top`, `width`, `height`.
fn tsv_box(quad: &Quad, carry: &dyn Fn((f64, f64)) -> (f64, f64), (w, h): (u32, u32)) -> [u32; 4] {
    let corners = [quad.ul, quad.ur, quad.ll, quad.lr].map(carry);
    let xs = corners.map(|(x, _)| x);
    let ys = corners.map(|(_, y)| y);
    let min = |v: [f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: [f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (wf, hf) = (f64::from(w), f64::from(h));
    let left = min(xs).clamp(0.0, wf).floor() as u32;
    let top = min(ys).clamp(0.0, hf).floor() as u32;
    let right = max(xs).clamp(0.0, wf).ceil() as u32;
    let bottom = max(ys).clamp(0.0, hf).ceil() as u32;
    [left, top, right - left, bottom - top]
}

/// A line's words, each the run of its characters between spaces, with the
/// union of their quads.
fn words_between_spaces(line: &TextLine) -> Vec<(String, Quad)> {
    let mut words = Vec::new();
    let mut run: Vec<&tinker_pdf::TextChar> = Vec::new();
    for c in line.chars.iter().chain(std::iter::once(&space())) {
        if c.text.chars().all(char::is_whitespace) {
            if !run.is_empty() {
                let text: String = run.iter().map(|c| c.text.as_str()).collect();
                let quads: Vec<Quad> = run.iter().map(|c| c.quad).collect();
                words.push((text, union_along_baseline(&quads)));
                run.clear();
            }
        } else {
            run.push(c);
        }
    }
    words
}

/// A space, to end the last word of a line.
fn space() -> tinker_pdf::TextChar {
    let nowhere = (0.0, 0.0);
    tinker_pdf::TextChar {
        text: " ".to_owned(),
        quad: Quad {
            ul: nowhere,
            ur: nowhere,
            ll: nowhere,
            lr: nowhere,
        },
        size: 0.0,
        origin: nowhere,
        mcid: None,
        stream: 0,
        font: None,
        stated: None,
        artifact: false,
    }
}

/// The TSV records the published definition gives `document`'s text at
/// `scale` pixels a point, each its twelve fields.
fn expected_records(document: &Document, scale: f64) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let record = |level: u8, numbers: [u64; 5], edges: [u32; 4], text: &str| {
        let mut fields = vec![level.to_string()];
        fields.extend(numbers.iter().map(u64::to_string));
        fields.extend(edges.iter().map(u32::to_string));
        fields.push("-1".to_owned());
        fields.push(text.to_owned());
        fields
    };
    for page in document.pages() {
        let (crop, rotation) = (page.crop_box(), page.rotation());
        let (displayed_w, displayed_h) = page.size();
        let size = (
            (displayed_w * scale).ceil() as u32,
            (displayed_h * scale).ceil() as u32,
        );
        let carry = |point: (f64, f64)| to_frame(crop, rotation, scale, point);
        let p = u64::from(page.index()) + 1;
        records.push(record(1, [p, 0, 0, 0, 0], [0, 0, size.0, size.1], ""));
        for (b, block) in (1u64..).zip(page.text().blocks()) {
            let edges = tsv_box(&block.quad, &carry, size);
            records.push(record(2, [p, b, 0, 0, 0], edges, ""));
            // A born-digital block is one paragraph, its box the block's.
            records.push(record(3, [p, b, 1, 0, 0], edges, ""));
            for (l, line) in (1u64..).zip(&block.lines) {
                let edges = tsv_box(&line.quad, &carry, size);
                records.push(record(4, [p, b, 1, l, 0], edges, ""));
                for (w, (text, quad)) in (1u64..).zip(words_between_spaces(line)) {
                    let edges = tsv_box(&quad, &carry, size);
                    records.push(record(5, [p, b, 1, l, w], edges, &text));
                }
            }
        }
    }
    records
}

// ---- reading TSV back, by the published definition --------------------------

/// The records of `tsv` past its header, each its fields, after holding the
/// whole file to the published definition: the header is the twelve names,
/// every record a line of twelve tab-separated fields, the levels nest page,
/// block, paragraph, line and word, each number counts from 1 within its
/// parent and is 0 at the levels above it, every box is four whole numbers,
/// and only a word's row has text.
fn read_tsv(tsv: &str) -> Vec<Vec<String>> {
    let body = tsv
        .strip_suffix('\n')
        .expect("every record ends in a line feed");
    let mut lines = body.split('\n');
    let header = lines.next().expect("a header");
    assert_eq!(header, PUBLISHED_HEADER.join("\t"), "the published header");

    let mut records = Vec::new();
    // The numbers of the last row of each level, to hold the counting.
    let mut last: [u64; 5] = [0; 5];
    let mut depth = 0usize;
    for line in lines {
        let fields: Vec<String> = line.split('\t').map(str::to_owned).collect();
        assert_eq!(fields.len(), 12, "twelve fields: {line:?}");
        let level: usize = fields[0].parse().expect("a level");
        assert!((1..=5).contains(&level), "a level from 1 to 5: {line:?}");
        assert!(level <= depth + 1, "a level under its parent: {line:?}");
        let numbers: Vec<u64> = fields[1..6]
            .iter()
            .map(|n| n.parse().expect("a number"))
            .collect();
        for (at, number) in numbers.iter().enumerate() {
            if at + 1 > level {
                assert_eq!(*number, 0, "0 at the levels below: {line:?}");
            }
        }
        // The levels above keep their numbers, and this one counts on from
        // its last sibling, or from 1 under a new parent; a page is numbered
        // by its place in its document.
        for at in 0..level - 1 {
            assert_eq!(numbers[at], last[at], "the parent's number: {line:?}");
        }
        let at = level - 1;
        if level == 1 {
            assert!(numbers[0] >= 1, "a page counts from 1: {line:?}");
        } else {
            let follows = if depth >= level { last[at] + 1 } else { 1 };
            assert_eq!(numbers[at], follows, "counted within its parent: {line:?}");
        }
        last[at] = numbers[at];
        for below in last.iter_mut().skip(level) {
            *below = 0;
        }
        depth = level;
        for edge in &fields[6..10] {
            edge.parse::<u32>().expect("a whole number of pixels");
        }
        if level < 5 {
            assert_eq!(fields[10], "-1", "no confidence but a word's: {line:?}");
            assert_eq!(fields[11], "", "no text but a word's: {line:?}");
        }
        records.push(fields);
    }
    records
}

// ---- the tests ---------------------------------------------------------------

/// **TSV is its published column definition**, on a left-to-right page with
/// a turned line, a right-to-left page and a page under `/Rotate 90` and a
/// crop box: the header, twelve fields a record, the levels and their
/// numbering, `-1` and no text on every row but a word's, and every box the
/// one the design derives from the line's characters — in the default frame
/// and at 144 dpi.
#[test]
fn tsv_holds_its_published_column_definition() {
    let document = fixture();
    for resolution in [72.0, 144.0] {
        let boxes = document.word_boxes(&WordBoxOptions::at_dpi(resolution));
        let records = read_tsv(&boxes.tsv());
        let expected = expected_records(&document, resolution / 72.0);
        assert_eq!(records.len(), expected.len(), "at {resolution} dpi");
        for (got, want) in records.iter().zip(&expected) {
            assert_eq!(got, want, "at {resolution} dpi");
        }

        // The words are the design's, punctuation carried, a right-to-left
        // line's in the order it is read.
        let words: Vec<&str> = records
            .iter()
            .filter(|fields| fields[0] == "5" && fields[1] == "1")
            .map(|fields| fields[11].as_str())
            .collect();
        assert_eq!(
            words,
            ["The", "(quick)", "fox,", "jumps!", "over", "43.5", "dogs.", "turned,", "words!"]
        );
        let hebrew: Vec<&str> = records
            .iter()
            .filter(|fields| fields[0] == "5" && fields[1] == "2")
            .map(|fields| fields[11].as_str())
            .collect();
        let olam = format!("{OLAM}.");
        assert_eq!(hebrew, [SHALOM, olam.as_str()]);

        // The model says the same: each page its size in the frame, the
        // right-to-left line read so, every word without a confidence.
        let [latin, rtl, turned] = &boxes.pages[..] else {
            panic!("three pages");
        };
        let at = |points: f64| (points * resolution / 72.0).ceil() as u32;
        assert_eq!((latin.width, latin.height), (at(PAGE.0), at(PAGE.1)));
        assert_eq!(
            (turned.width, turned.height),
            (at(CROP.3 - CROP.1), at(CROP.2 - CROP.0)),
            "a quarter turn swaps the displayed sides"
        );
        let lines: Vec<&WordLine> = rtl
            .blocks
            .iter()
            .flat_map(|b| &b.paragraphs)
            .flat_map(|p| &p.lines)
            .collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].direction, LineDirection::RightToLeft);
        for page in [latin, rtl, turned] {
            assert_eq!(page.resolution, resolution);
            for block in &page.blocks {
                assert_eq!(block.paragraphs.len(), 1, "a paragraph is its block");
                assert_eq!(block.paragraphs[0].quad, block.quad);
                for line in &block.paragraphs[0].lines {
                    assert!(line.words.iter().all(|w| w.confidence.is_none()));
                }
            }
        }
        // The document's one date stands for both.
        assert_eq!(boxes.created.as_deref(), Some("2026-01-02T03:04:05Z"));
        assert_eq!(boxes.modified, boxes.created);
    }
}

/// A model a recogniser fills, with no page at all, writes its TSV: two
/// paragraphs in a block, a turned word enclosed, a word off the page's edge
/// clipped, confidences written to six places, clamped, or `-1` where there
/// is none or it is not a number, and a page with nothing on it one row.
#[test]
fn a_model_filled_without_a_page_writes_tsv() {
    let rect = tinker_pdf_content::word_boxes::upright;
    let word = |text: &str, quad: Quad, confidence: Option<f32>| WordBox {
        text: text.to_owned(),
        quad,
        confidence,
    };
    let line = |quad: Quad, words: Vec<WordBox>| WordLine {
        quad,
        direction: LineDirection::LeftToRight,
        words,
    };
    let turned = Quad {
        ul: (100.0, 150.0),
        ur: (180.0, 110.0),
        ll: (110.0, 170.0),
        lr: (190.0, 130.0),
    };
    let boxes = WordBoxes {
        pages: vec![
            WordPage {
                index: 0,
                width: 640,
                height: 500,
                resolution: 300.0,
                image: Some("scan-1.png".to_owned()),
                blocks: vec![
                    WordBlock {
                        quad: rect(61.0, 41.0, 574.0, 413.0),
                        paragraphs: vec![
                            WordParagraph {
                                quad: rect(61.0, 41.0, 574.0, 180.0),
                                lines: vec![
                                    line(
                                        rect(65.0, 41.0, 515.0, 71.0),
                                        vec![
                                            word(
                                                "The",
                                                rect(65.0, 41.0, 111.0, 61.0),
                                                Some(96.063_48),
                                            ),
                                            word(
                                                "(quick)",
                                                rect(128.4, 42.2, 217.6, 66.9),
                                                Some(95.965_7),
                                            ),
                                        ],
                                    ),
                                    line(turned, vec![word("turned", turned, None)]),
                                ],
                            },
                            WordParagraph {
                                quad: rect(61.0, 200.0, 640.0, 500.0),
                                lines: vec![line(
                                    rect(61.0, 400.0, 640.0, 500.0),
                                    vec![
                                        word("100%", rect(61.0, 400.0, 120.0, 430.0), Some(150.0)),
                                        word(
                                            "nan",
                                            rect(130.0, 400.0, 160.0, 430.0),
                                            Some(f32::NAN),
                                        ),
                                        word("clipped", rect(600.5, 480.2, 660.0, 520.0), None),
                                    ],
                                )],
                            },
                        ],
                    },
                    WordBlock {
                        quad: rect(-10.0, -10.0, 20.0, 20.0),
                        paragraphs: vec![WordParagraph {
                            quad: rect(0.0, 0.0, 20.0, 20.0),
                            lines: vec![line(
                                rect(0.0, 0.0, 20.0, 20.0),
                                vec![word("edge", rect(0.0, 0.0, 20.0, 20.0), Some(0.0))],
                            )],
                        }],
                    },
                ],
            },
            WordPage {
                index: 4,
                width: 100,
                height: 100,
                resolution: 72.0,
                image: None,
                blocks: Vec::new(),
            },
        ],
        created: None,
        modified: None,
    };
    let rows = [
        "1 1 0 0 0 0 0 0 640 500 -1 ",
        "2 1 1 0 0 0 61 41 513 372 -1 ",
        "3 1 1 1 0 0 61 41 513 139 -1 ",
        "4 1 1 1 1 0 65 41 450 30 -1 ",
        "5 1 1 1 1 1 65 41 46 20 96.063477 The",
        "5 1 1 1 1 2 128 42 90 25 95.965698 (quick)",
        "4 1 1 1 2 0 100 110 90 60 -1 ",
        "5 1 1 1 2 1 100 110 90 60 -1 turned",
        "3 1 1 2 0 0 61 200 579 300 -1 ",
        "4 1 1 2 1 0 61 400 579 100 -1 ",
        "5 1 1 2 1 1 61 400 59 30 100.000000 100%",
        "5 1 1 2 1 2 130 400 30 30 -1 nan",
        "5 1 1 2 1 3 600 480 40 20 -1 clipped",
        "2 1 2 0 0 0 0 0 20 20 -1 ",
        "3 1 2 1 0 0 0 0 20 20 -1 ",
        "4 1 2 1 1 0 0 0 20 20 -1 ",
        "5 1 2 1 1 1 0 0 20 20 0.000000 edge",
        "1 5 0 0 0 0 0 0 100 100 -1 ",
    ];
    let mut expected = PUBLISHED_HEADER.join("\t");
    expected.push('\n');
    for row in rows {
        // The rows are written with a space between fields for reading here;
        // a row that is not a word's ends in its empty text field.
        expected.push_str(&row.replace(' ', "\t"));
        expected.push('\n');
    }
    let tsv = boxes.tsv();
    assert_eq!(tsv, expected);
    assert_eq!(read_tsv(&tsv).len(), rows.len());
}

/// A word holding a tab, or any character that ends a line, is written with
/// a space in its place, so it stays one field of one record.
#[test]
fn a_word_holding_a_tab_or_line_break_stays_in_its_row() {
    let rect = tinker_pdf_content::word_boxes::upright;
    let breakers = [
        "a\tb",
        "c\nd",
        "e\r\nf",
        "g\u{b}h",
        "i\u{c}j",
        "k\u{85}l",
        "m\u{2028}n",
        "o\u{2029}p",
    ];
    let words = breakers
        .iter()
        .enumerate()
        .map(|(i, text)| WordBox {
            text: (*text).to_owned(),
            quad: rect(i as f64 * 10.0, 0.0, i as f64 * 10.0 + 8.0, 10.0),
            confidence: None,
        })
        .collect();
    let boxes = WordBoxes {
        pages: vec![WordPage {
            index: 0,
            width: 100,
            height: 20,
            resolution: 72.0,
            image: None,
            blocks: vec![WordBlock {
                quad: rect(0.0, 0.0, 80.0, 10.0),
                paragraphs: vec![WordParagraph {
                    quad: rect(0.0, 0.0, 80.0, 10.0),
                    lines: vec![WordLine {
                        quad: rect(0.0, 0.0, 80.0, 10.0),
                        direction: LineDirection::LeftToRight,
                        words,
                    }],
                }],
            }],
        }],
        ..WordBoxes::default()
    };
    let tsv = boxes.tsv();
    // Nothing that ends a line is left but each record's own line feed, and
    // no tab but the eleven between a record's fields.
    for breaker in ['\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'] {
        assert!(!tsv.contains(breaker), "{breaker:?} is left in the file");
    }
    for line in tsv.split_terminator('\n') {
        assert_eq!(line.matches('\t').count(), 11, "{line:?}");
    }
    let records = read_tsv(&tsv);
    let texts: Vec<&str> = records
        .iter()
        .filter(|fields| fields[0] == "5")
        .map(|fields| fields[11].as_str())
        .collect();
    assert_eq!(
        texts,
        ["a b", "c d", "e  f", "g h", "i j", "k l", "m n", "o p"]
    );
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}

/// `document`, and what `Document::open` synthesised for it held to the
/// validate-on-save helper. A PDF is opened, not written, and is handed back
/// as it is.
#[track_caller]
fn converted(label: &str, document: tinker_pdf::Document) -> tinker_pdf::Document {
    if document.archive().is_some() {
        written_support::synthesised(label, document)
    } else {
        document
    }
}
