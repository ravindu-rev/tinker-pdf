//! The OCR import seam (ROADMAP row OC-01, `docs/design/ocr.md` milestone 1):
//! hOCR, ALTO and PAGE XML read into FT-24's word-box model.
//!
//! Every file here is written from its format's own definition — hOCR 1.2's
//! classes and properties, ALTO's and PAGE XML's schemas — and none is a
//! recogniser's output (ruling 13: a recogniser's file is an input, never a
//! verdict). Each test states the words, boxes and order its file states and
//! holds the import to them. The three whole files are the `ocr_import` fuzz
//! target's seeds, and `the_fuzz_seeds_are_the_files_these_tests_read` holds
//! the committed seeds to them.
//!
//! The text layer (milestone 2) is held to the same files, written over pages
//! this file builds, saved, reopened and held to the strict validator: the
//! layer extracts as the imported text in the imported order, every word's
//! box comes back as the file's through the frame, and the page draws what it
//! drew before.

use std::path::PathBuf;

mod written_support;

use proptest::prelude::*;
use tinker_pdf::ocr::{
    add_text_layer, import, Direction, Extent, ImportError, ImportFormat, ImportWarning, Imported,
    ImportedPage, LayerError, LayerWarning, Layered, LineSetting, Located, Unit,
    MAX_IMPORTED_WORDS, MAX_LAYER_CHARACTERS,
};
use tinker_pdf::{
    Document, DocumentBuilder, LineDirection, Quad, Rect, RenderOptions, WordBlock, WordBox,
    WordBoxOptions, WordLine, WordPage, WordParagraph, WriteMode, WriteOptions,
};

// ---- the files --------------------------------------------------------------

/// hOCR 1.2, two pages: two blocks whose `order` reverses their document
/// order, paragraphs, a line holding text and no word, an `alternatives` span,
/// and a second page whose block states no paragraph.
const HOCR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en" lang="en">
<head>
<title>A hand-written hOCR file</title>
<meta http-equiv="Content-Type" content="text/html;charset=utf-8"/>
<meta name="ocr-system" content="written by hand"/>
<meta name="ocr-capabilities" content="ocr_page ocr_carea ocr_par ocr_line ocrx_word ocrp_wconf"/>
<meta name="ocr-number-of-pages" content="2"/>
</head>
<body>
<div class="ocr_page" id="page_1" title='image "scan-1.png"; bbox 0 0 1000 1400; ppageno 0; scan_res 300 300'>
<div class="ocr_carea" id="block_1_2" title="bbox 100 700 900 800; order 2">
<p class="ocr_par" id="par_1_2_1" title="bbox 100 700 900 800">
<span class="ocr_line" id="line_1_2_1_1" title="bbox 100 700 520 760">
<span class="ocrx_word" id="word_second" title="bbox 100 700 300 760; x_wconf 88">second</span>
<span class="ocrx_word" id="word_block" title="bbox 320 700 520 760; x_wconf 79">block</span>
</span>
</p>
</div>
<div class="ocr_carea" id="block_1_1" title="bbox 100 100 900 400; order 1">
<p class="ocr_par" id="par_1_1_1" title="bbox 100 100 900 250">
<span class="ocr_line" id="line_1_1_1_1" title="bbox 100 100 600 160">
<span class="ocrx_word" id="word_hello" title="bbox 100 100 300 160; x_wconf 96">Hello</span>
<span class="ocrx_word" id="word_world" title="bbox 320 100 600 160; x_wconf 91"><span class="alternatives"><ins class="alt" title="nlp 0.2">world</ins><del class="alt" title="nlp 1.3">word</del></span></span>
</span>
<span class="ocr_line" id="line_1_1_1_2" title="bbox 100 180 400 240">
<span class="ocrx_word" id="word_again" title="bbox 100 180 400 240">again</span>
</span>
</p>
<p class="ocr_par" id="par_1_1_2" title="bbox 100 300 900 400">
<span class="ocr_line" id="line_1_1_2_1" title="bbox 100 300 700 360">A line   of
 text</span>
</p>
</div>
</div>
<div class="ocr_page" id="page_2" title="bbox 0 0 800 600; ppageno 1">
<div class="ocr_carea" id="block_2_1" title="bbox 50 50 750 150">
<span class="ocr_line" id="line_2_1_1" title="bbox 50 50 300 100">
<span class="ocrx_word" id="word_page" title="bbox 50 50 200 100">Page</span>
<span class="ocrx_word" id="word_two" title="bbox 220 50 300 100">two</span>
</span>
</div>
</div>
</body>
</html>
"#;

/// ALTO v4: a `ReadingOrder` (4.3) that names a line of the second block
/// first and the first block after it, a `HYP` ending a line, a `String` in a
/// `ComposedBlock`, and a block the order does not name.
const ALTO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<alto xmlns="http://www.loc.gov/standards/alto/ns-v4#">
<Description>
<MeasurementUnit>pixel</MeasurementUnit>
<sourceImageInformation><fileName>scan-2.tif</fileName></sourceImageInformation>
</Description>
<ReadingOrder>
<OrderedGroup ID="ro_1">
<ElementRef ID="ref_1" REF="TL_2"/>
<ElementRef ID="ref_2" REF="TB_1"/>
</OrderedGroup>
</ReadingOrder>
<Layout>
<Page ID="P_1" PHYSICAL_IMG_NR="3" WIDTH="2000" HEIGHT="3000">
<PrintSpace HPOS="0" VPOS="0" WIDTH="2000" HEIGHT="3000">
<TextBlock ID="TB_1" HPOS="100" VPOS="100" WIDTH="1800" HEIGHT="200">
<TextLine ID="TL_1" HPOS="100" VPOS="100" WIDTH="890" HEIGHT="80">
<String ID="S_1" CONTENT="First" HPOS="100" VPOS="100" WIDTH="300" HEIGHT="80" WC="0.97"/>
<SP WIDTH="20" HPOS="400" VPOS="100"/>
<String ID="S_2" CONTENT="block" HPOS="420" VPOS="100" WIDTH="300" HEIGHT="80" WC="0.9"/>
<SP WIDTH="20" HPOS="720" VPOS="100"/>
<String ID="S_3" CONTENT="hyph" SUBS_CONTENT="hyphenated" SUBS_TYPE="HypPart1" HPOS="740" VPOS="100" WIDTH="220" HEIGHT="80"/>
<HYP CONTENT="-" HPOS="960" VPOS="100" WIDTH="30"/>
</TextLine>
</TextBlock>
<ComposedBlock ID="CB_1" HPOS="100" VPOS="400" WIDTH="1800" HEIGHT="300">
<TextBlock ID="TB_2" HPOS="100" VPOS="400" WIDTH="1800" HEIGHT="100">
<TextLine ID="TL_2" HPOS="100" VPOS="400" WIDTH="600" HEIGHT="80">
<String ID="S_4" CONTENT="Composed" HPOS="100" VPOS="400" WIDTH="400" HEIGHT="80"/>
</TextLine>
</TextBlock>
</ComposedBlock>
<TextBlock ID="TB_3" HPOS="100" VPOS="800" WIDTH="1800" HEIGHT="100">
<TextLine ID="TL_3" HPOS="100" VPOS="800" WIDTH="500" HEIGHT="80">
<String ID="S_5" CONTENT="Unnamed" HPOS="100" VPOS="800" WIDTH="500" HEIGHT="80"/>
</TextLine>
</TextBlock>
</PrintSpace>
</Page>
</Layout>
</alto>
"#;

/// ALTO with no `ReadingOrder`: an `IDNEXT` chain from the first block to the
/// third and back to the second, and a fourth block no chain reaches. The
/// namespace is a placeholder, `{NS}`, so the same page is read in each of
/// ALTO's namespaces.
const ALTO_CHAINED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<alto{NS}>
<Description><MeasurementUnit>pixel</MeasurementUnit></Description>
<Layout>
<Page ID="P_1" WIDTH="1000" HEIGHT="1000">
<PrintSpace HPOS="0" VPOS="0" WIDTH="1000" HEIGHT="1000">
<TextBlock ID="A" IDNEXT="C" HPOS="0" VPOS="0" WIDTH="400" HEIGHT="100">
<TextLine HPOS="0" VPOS="0" WIDTH="400" HEIGHT="100"><String CONTENT="one" HPOS="0" VPOS="0" WIDTH="400" HEIGHT="100"/></TextLine>
</TextBlock>
<TextBlock ID="B" HPOS="0" VPOS="500" WIDTH="400" HEIGHT="100">
<TextLine HPOS="0" VPOS="500" WIDTH="400" HEIGHT="100"><String CONTENT="three" HPOS="0" VPOS="500" WIDTH="400" HEIGHT="100"/></TextLine>
</TextBlock>
<TextBlock ID="C" IDNEXT="B" HPOS="500" VPOS="0" WIDTH="400" HEIGHT="100">
<TextLine HPOS="500" VPOS="0" WIDTH="400" HEIGHT="100"><String CONTENT="two" HPOS="500" VPOS="0" WIDTH="400" HEIGHT="100"/></TextLine>
</TextBlock>
<TextBlock ID="D" HPOS="500" VPOS="500" WIDTH="400" HEIGHT="100">
<TextLine HPOS="500" VPOS="500" WIDTH="400" HEIGHT="100"><String CONTENT="four" HPOS="500" VPOS="500" WIDTH="400" HEIGHT="100"/></TextLine>
</TextBlock>
</PrintSpace>
</Page>
</Layout>
</alto>
"#;

/// One word a page ten inches by twelve, an inch in from the left and two
/// down, in ALTO's two units that are lengths: `{UNIT}`, and the page's and
/// the word's numbers in it.
const ALTO_UNITS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<alto xmlns="http://www.loc.gov/standards/alto/ns-v4#">
<Description><MeasurementUnit>{UNIT}</MeasurementUnit></Description>
<Layout><Page ID="P" WIDTH="{W}" HEIGHT="{H}"><PrintSpace>
<TextBlock ID="B"><TextLine ID="L" HPOS="{X}" VPOS="{Y}" WIDTH="{WW}" HEIGHT="{WH}">
<String ID="S" CONTENT="inch" HPOS="{X}" VPOS="{Y}" WIDTH="{WW}" HEIGHT="{WH}"/>
</TextLine></TextBlock>
</PrintSpace></Page></Layout>
</alto>
"#;

/// PAGE XML 2019-07-15: a `ReadingOrder` that puts the second region first,
/// lines ordered by `index` against their document order, a word whose
/// `Coords` is a polygon, a word with two readings, and a `TextLine` with no
/// `Word`.
const PAGE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<PcGts xmlns="http://schema.primaresearch.org/PAGE/gts/pagecontent/2019-07-15">
<Metadata><Creator>written by hand</Creator><Created>2026-11-13T00:00:00</Created><LastChange>2026-11-13T00:00:00</LastChange></Metadata>
<Page imageFilename="scan-3.png" imageWidth="1200" imageHeight="1600" imageXResolution="300" imageYResolution="300">
<ReadingOrder>
<OrderedGroup id="ro_1" caption="Regions">
<RegionRefIndexed index="1" regionRef="r1"/>
<RegionRefIndexed index="0" regionRef="r2"/>
</OrderedGroup>
</ReadingOrder>
<TextRegion id="r1" type="paragraph">
<Coords points="100,100 1100,100 1100,400 100,400"/>
<TextLine id="l2" index="1">
<Coords points="100,250 1100,250 1100,330 100,330"/>
<Word id="w3"><Coords points="100,250 400,250 400,330 100,330"/><TextEquiv conf="0.8"><Unicode>second</Unicode></TextEquiv></Word>
<TextEquiv><Unicode>second</Unicode></TextEquiv>
</TextLine>
<TextLine id="l1" index="0">
<Coords points="100,100 1100,100 1100,180 100,180"/>
<Word id="w1"><Coords points="100,110 300,100 310,180 105,175"/><TextEquiv index="2"><Unicode>Frist</Unicode></TextEquiv><TextEquiv index="1" conf="0.9"><Unicode>First</Unicode></TextEquiv></Word>
<Word id="w2"><Coords points="320,100 600,100 600,180 320,180"/><TextEquiv><Unicode>line</Unicode></TextEquiv></Word>
<TextEquiv><Unicode>First line</Unicode></TextEquiv>
</TextLine>
</TextRegion>
<TextRegion id="r2" type="paragraph">
<Coords points="100,500 1100,500 1100,700 100,700"/>
<TextLine id="l3">
<Coords points="100,500 900,500 900,580 100,580"/>
<TextEquiv><Unicode>A line with no words</Unicode></TextEquiv>
</TextLine>
</TextRegion>
</Page>
</PcGts>
"#;

/// The fuzz target's seeds: these files, by the names they are committed
/// under in `fuzz/corpus/ocr_import/`.
const SEEDS: [(&str, &str); 3] = [
    ("hocr-two-pages", HOCR),
    ("alto-reading-order", ALTO),
    ("page-xml-reading-order", PAGE_XML),
];

// ---- reading them back ---------------------------------------------------------

fn read(file: &str) -> Imported {
    import(file.as_bytes()).unwrap_or_else(|e| panic!("the file imports: {e}"))
}

fn rect(quad: &Quad) -> (f64, f64, f64, f64) {
    quad.bounds()
}

/// A page as lines of text: each block, paragraph and line with its box, and
/// each word with its box and its confidence to two decimals.
fn shape(page: &WordPage) -> Vec<String> {
    let mut out = Vec::new();
    for block in &page.blocks {
        out.push(format!("block {:?}", rect(&block.quad)));
        for paragraph in &block.paragraphs {
            out.push(format!(" paragraph {:?}", rect(&paragraph.quad)));
            for line in &paragraph.lines {
                out.push(format!("  line {:?}", rect(&line.quad)));
                for word in &line.words {
                    let confidence = word
                        .confidence
                        .map_or_else(|| "-".to_string(), |c| format!("{c:.2}"));
                    out.push(format!(
                        "   {} {:?} {confidence}",
                        word.text,
                        rect(&word.quad)
                    ));
                }
            }
        }
    }
    out
}

/// The page's words, in reading order.
fn words(page: &ImportedPage) -> Vec<String> {
    page.page
        .blocks
        .iter()
        .flat_map(|b| &b.paragraphs)
        .flat_map(|p| &p.lines)
        .flat_map(|l| &l.words)
        .map(|w| w.text.clone())
        .collect()
}

fn lines_of(page: &WordPage) -> usize {
    page.blocks
        .iter()
        .flat_map(|b| &b.paragraphs)
        .map(|p| p.lines.len())
        .sum()
}

// ---- the three formats ---------------------------------------------------------

/// hOCR 1.2: blocks, paragraphs, lines and words with the boxes their `bbox`
/// states (4.2); `order` on sibling blocks (4.11) putting the second before
/// the first; a line holding text and no word read as one word over the
/// line's box, its white space collapsed; an `alternatives` span giving its
/// first `ins` (5.7); and two pages, each with its `ppageno`, the first with
/// its `image` and `scan_res`, the second's block stating no paragraph and so
/// being its own.
#[test]
fn an_hocr_file_reads_to_the_words_boxes_and_order_it_states() {
    let imported = read(HOCR);
    assert_eq!(imported.format, ImportFormat::Hocr);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.pages.len(), 2);

    let first = &imported.pages[0];
    assert_eq!(
        (first.page.index, first.page.width, first.page.height),
        (0, 1000, 1400)
    );
    assert_eq!(first.page.resolution, 300.0);
    assert_eq!(first.resolution, Some((300.0, 300.0)));
    assert_eq!(first.page.image.as_deref(), Some("scan-1.png"));
    assert_eq!(first.unit, Unit::Pixel);
    assert_eq!(
        shape(&first.page),
        [
            "block (100.0, 100.0, 900.0, 400.0)",
            " paragraph (100.0, 100.0, 900.0, 250.0)",
            "  line (100.0, 100.0, 600.0, 160.0)",
            "   Hello (100.0, 100.0, 300.0, 160.0) 96.00",
            "   world (320.0, 100.0, 600.0, 160.0) 91.00",
            "  line (100.0, 180.0, 400.0, 240.0)",
            "   again (100.0, 180.0, 400.0, 240.0) -",
            " paragraph (100.0, 300.0, 900.0, 400.0)",
            "  line (100.0, 300.0, 700.0, 360.0)",
            "   A line of text (100.0, 300.0, 700.0, 360.0) -",
            "block (100.0, 700.0, 900.0, 800.0)",
            " paragraph (100.0, 700.0, 900.0, 800.0)",
            "  line (100.0, 700.0, 520.0, 760.0)",
            "   second (100.0, 700.0, 300.0, 760.0) 88.00",
            "   block (320.0, 700.0, 520.0, 760.0) 79.00",
        ]
    );
    assert_eq!(first.lines.len(), lines_of(&first.page));
    assert!(first
        .lines
        .iter()
        .all(|line| line.turn == 0.0 && line.direction.is_none()));

    let second = &imported.pages[1];
    assert_eq!(
        (second.page.index, second.page.width, second.page.height),
        (1, 800, 600)
    );
    assert_eq!(second.page.resolution, 72.0, "the model's own, stated none");
    assert_eq!(second.resolution, None);
    assert_eq!(second.page.image, None);
    assert_eq!(
        shape(&second.page),
        [
            "block (50.0, 50.0, 750.0, 150.0)",
            " paragraph (50.0, 50.0, 750.0, 150.0)",
            "  line (50.0, 50.0, 300.0, 100.0)",
            "   Page (50.0, 50.0, 200.0, 100.0) -",
            "   two (220.0, 50.0, 300.0, 100.0) -",
        ]
    );
}

/// ALTO: a `ReadingOrder` that differs from document order — an
/// `ElementRef` to a line placing its block at that mention — with the block
/// it does not name after; a `String` inside a `ComposedBlock`; a `HYP`
/// joined to its line's last word, content and box; `SUBS_CONTENT` not read
/// over `CONTENT`; an `IDNEXT` chain; the same page in `mm10` and in
/// `inch1200`, each in its own unit; and the same page in the v2 and v3
/// namespaces and in none, read to the same pages as in v4.
#[test]
fn an_alto_file_reads_to_the_words_boxes_and_order_it_states() {
    let imported = read(ALTO);
    assert_eq!(imported.format, ImportFormat::Alto);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let [page] = imported.pages.as_slice() else {
        panic!("one page")
    };
    assert_eq!(
        (page.page.index, page.page.width, page.page.height),
        (2, 2000, 3000),
        "the index is PHYSICAL_IMG_NR less one"
    );
    assert_eq!(page.page.image.as_deref(), Some("scan-2.tif"));
    assert_eq!((page.unit, page.resolution), (Unit::Pixel, None));
    assert_eq!(
        shape(&page.page),
        [
            "block (100.0, 400.0, 1900.0, 500.0)",
            " paragraph (100.0, 400.0, 1900.0, 500.0)",
            "  line (100.0, 400.0, 700.0, 480.0)",
            "   Composed (100.0, 400.0, 500.0, 480.0) -",
            "block (100.0, 100.0, 1900.0, 300.0)",
            " paragraph (100.0, 100.0, 1900.0, 300.0)",
            "  line (100.0, 100.0, 990.0, 180.0)",
            "   First (100.0, 100.0, 400.0, 180.0) 97.00",
            "   block (420.0, 100.0, 720.0, 180.0) 90.00",
            "   hyph- (740.0, 100.0, 990.0, 180.0) -",
            "block (100.0, 800.0, 1900.0, 900.0)",
            " paragraph (100.0, 800.0, 1900.0, 900.0)",
            "  line (100.0, 800.0, 600.0, 880.0)",
            "   Unnamed (100.0, 800.0, 600.0, 880.0) -",
        ]
    );

    // The chain: A to C to B, and D, which nothing names, after it.
    let chained = read(&ALTO_CHAINED.replace(
        "{NS}",
        " xmlns=\"http://www.loc.gov/standards/alto/ns-v4#\"",
    ));
    assert!(chained.warnings.is_empty(), "{:?}", chained.warnings);
    assert_eq!(words(&chained.pages[0]), ["one", "two", "three", "four"]);

    // The same page in the two earlier namespaces and in none.
    for namespace in [
        " xmlns=\"http://www.loc.gov/standards/alto/ns-v2#\"",
        " xmlns=\"http://www.loc.gov/standards/alto/ns-v3#\"",
        "",
    ] {
        let again = read(&ALTO_CHAINED.replace("{NS}", namespace));
        assert_eq!(again, chained, "{namespace:?} reads as v4 does");
    }

    // One word an inch in and two down, an inch and a half wide, half an inch
    // high, on a page ten inches by twelve: in each unit, its own numbers.
    let in_unit = |unit: &str, numbers: [&str; 6]| {
        let [w, h, x, y, ww, wh] = numbers;
        read(
            &ALTO_UNITS
                .replace("{UNIT}", unit)
                .replace("{WW}", ww)
                .replace("{WH}", wh)
                .replace("{W}", w)
                .replace("{H}", h)
                .replace("{X}", x)
                .replace("{Y}", y),
        )
    };
    let mm10 = in_unit("mm10", ["2540", "3048", "254", "508", "381", "127"]);
    let inch1200 = in_unit(
        "inch1200",
        ["12000", "14400", "1200", "2400", "1800", "600"],
    );
    for (imported, unit, per_inch, word) in [
        (&mm10, Unit::Mm10, 254.0, (254.0, 508.0, 635.0, 635.0)),
        (
            &inch1200,
            Unit::Inch1200,
            1200.0,
            (1200.0, 2400.0, 3000.0, 3000.0),
        ),
    ] {
        let page = &imported.pages[0];
        assert_eq!(page.unit, unit);
        assert_eq!(unit.per_inch(), Some(per_inch));
        assert_eq!(page.resolution, Some((per_inch, per_inch)));
        assert_eq!(page.page.resolution, per_inch);
        let quad = &page.page.blocks[0].paragraphs[0].lines[0].words[0].quad;
        assert_eq!(rect(quad), word, "the file's own numbers, unrescaled");
        // And the two are one page, an inch being an inch.
        let (left, top, right, bottom) = rect(quad);
        assert_eq!(
            (
                left / per_inch,
                top / per_inch,
                right / per_inch,
                bottom / per_inch
            ),
            (1.0, 2.0, 2.5, 2.5)
        );
        assert_eq!(
            (page.extent.right / per_inch, page.extent.bottom / per_inch),
            (10.0, 12.0)
        );
    }
}

/// `PAGE_XML` with every `Coords` written as `Point` children, as the dated
/// schemas before 2013-07-15 write them, in the 2010-03-19 namespace.
fn page_xml_with_points() -> String {
    let mut out = String::new();
    let mut rest = PAGE_XML.replace("2019-07-15", "2010-03-19");
    while let Some(at) = rest.find("<Coords points=\"") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "<Coords points=\"".len()..];
        let end = after.find("\"/>").expect("a closed Coords");
        out.push_str("<Coords>");
        for pair in after[..end].split(' ') {
            let (x, y) = pair.split_once(',').expect("x,y");
            out.push_str(&format!("<Point x=\"{x}\" y=\"{y}\"/>"));
        }
        out.push_str("</Coords>");
        rest = after[end + "\"/>".len()..].to_string();
    }
    out.push_str(&rest);
    out
}

/// PAGE XML: polygon `Coords` read as their bounds; a `ReadingOrder` whose
/// `index` puts the second region first; lines ordered by their `index`
/// against document order; the `TextEquiv` with the lowest `index` chosen
/// over the first; a `TextLine` with no `Word` read as one word over the
/// line's box; and the same page under the 2013-07-15 namespace, and with
/// `Point` children under 2010-03-19, read to the same page.
#[test]
fn a_page_xml_file_reads_to_the_words_boxes_and_order_it_states() {
    let imported = read(PAGE_XML);
    assert_eq!(imported.format, ImportFormat::PageXml);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let [page] = imported.pages.as_slice() else {
        panic!("one page")
    };
    assert_eq!(
        (page.page.index, page.page.width, page.page.height),
        (0, 1200, 1600)
    );
    assert_eq!(page.resolution, Some((300.0, 300.0)));
    assert_eq!(page.page.image.as_deref(), Some("scan-3.png"));
    assert_eq!(
        shape(&page.page),
        [
            "block (100.0, 500.0, 1100.0, 700.0)",
            " paragraph (100.0, 500.0, 1100.0, 700.0)",
            "  line (100.0, 500.0, 900.0, 580.0)",
            "   A line with no words (100.0, 500.0, 900.0, 580.0) -",
            "block (100.0, 100.0, 1100.0, 400.0)",
            " paragraph (100.0, 100.0, 1100.0, 400.0)",
            "  line (100.0, 100.0, 1100.0, 180.0)",
            "   First (100.0, 100.0, 310.0, 180.0) 90.00",
            "   line (320.0, 100.0, 600.0, 180.0) -",
            "  line (100.0, 250.0, 1100.0, 330.0)",
            "   second (100.0, 250.0, 400.0, 330.0) 80.00",
        ]
    );

    let thirteen = read(&PAGE_XML.replace("2019-07-15", "2013-07-15"));
    assert_eq!(thirteen, imported, "2013-07-15 reads as 2019-07-15 does");
    let points = page_xml_with_points();
    assert!(points.contains("<Point x=\"310\" y=\"180\"/>"));
    assert_eq!(
        read(&points),
        imported,
        "Point children are the same polygon"
    );
}

/// Each format's confidence reaches the model's 0 to 100 from its own
/// scale, its ends included: hOCR's `x_wconf` a percentage already, ALTO's
/// `WC` and PAGE XML's `conf` from 0 to 1.
#[test]
fn confidences_reach_the_models_scale() {
    let confidences = |imported: &Imported| -> Vec<Option<f32>> {
        imported.pages[0]
            .page
            .blocks
            .iter()
            .flat_map(|b| &b.paragraphs)
            .flat_map(|p| &p.lines)
            .flat_map(|l| &l.words)
            .map(|w| w.confidence)
            .collect()
    };
    assert_eq!(
        confidences(&read(HOCR)),
        [Some(96.0), Some(91.0), None, None, Some(88.0), Some(79.0)]
    );
    assert_eq!(
        confidences(&read(ALTO)),
        [None, Some(97.0), Some(90.0), None, None]
    );
    assert_eq!(
        confidences(&read(PAGE_XML)),
        [None, Some(90.0), None, Some(80.0)]
    );

    let hocr = hocr_page(
        "<span class=\"ocr_line\" title=\"bbox 0 0 300 50\">\
         <span class=\"ocrx_word\" title=\"bbox 0 0 100 50; x_wconf 0\">none</span>\
         <span class=\"ocrx_word\" title=\"bbox 100 0 200 50; x_wconf 100\">all</span>\
         <span class=\"ocrx_word\" title=\"bbox 200 0 300 50; x_wconf 37.5\">some</span></span>",
    );
    assert_eq!(
        confidences(&read(&hocr)),
        [Some(0.0), Some(100.0), Some(37.5)]
    );
    let alto = alto_page(
        "<String CONTENT=\"none\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"10\" HEIGHT=\"10\" WC=\"0\"/>\
         <String CONTENT=\"all\" HPOS=\"10\" VPOS=\"0\" WIDTH=\"10\" HEIGHT=\"10\" WC=\"1\"/>\
         <String CONTENT=\"some\" HPOS=\"20\" VPOS=\"0\" WIDTH=\"10\" HEIGHT=\"10\" WC=\"0.375\"/>",
    );
    assert_eq!(
        confidences(&read(&alto)),
        [Some(0.0), Some(100.0), Some(37.5)]
    );
    let page = page_xml_line(
        "<Word><Coords points=\"0,0 10,10\"/><TextEquiv conf=\"0\"><Unicode>none</Unicode></TextEquiv></Word>\
         <Word><Coords points=\"10,0 20,10\"/><TextEquiv conf=\"1\"><Unicode>all</Unicode></TextEquiv></Word>\
         <Word><Coords points=\"20,0 30,10\"/><TextEquiv conf=\"0.375\"><Unicode>some</Unicode></TextEquiv></Word>",
    );
    assert_eq!(
        confidences(&read(&page)),
        [Some(0.0), Some(100.0), Some(37.5)]
    );
}

// ---- small files for the defects ---------------------------------------------

/// An hOCR file of one page, 1000 by 1400, around `body`.
fn hocr_page(body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html><head><title>t</title></head><body>\
         <div class=\"ocr_page\" title=\"bbox 0 0 1000 1400\">{body}</div></body></html>\n"
    )
}

/// An ALTO file of one page, 1000 by 1000, its one line holding `strings`.
fn alto_page(strings: &str) -> String {
    format!(
        "<alto xmlns=\"http://www.loc.gov/standards/alto/ns-v4#\"><Layout>\
         <Page ID=\"P\" WIDTH=\"1000\" HEIGHT=\"1000\"><PrintSpace>\
         <TextBlock ID=\"B\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"1000\" HEIGHT=\"1000\">\
         <TextLine ID=\"L\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"1000\" HEIGHT=\"100\">{strings}</TextLine>\
         </TextBlock></PrintSpace></Page></Layout></alto>"
    )
}

/// A PAGE XML file of one page, 1000 by 1000, its one line holding `words`.
fn page_xml_line(words: &str) -> String {
    format!(
        "<PcGts xmlns=\"http://schema.primaresearch.org/PAGE/gts/pagecontent/2019-07-15\">\
         <Page imageFilename=\"p.png\" imageWidth=\"1000\" imageHeight=\"1000\">\
         <TextRegion id=\"r\"><Coords points=\"0,0 1000,100\"/>\
         <TextLine id=\"l\"><Coords points=\"0,0 1000,100\"/>{words}</TextLine>\
         </TextRegion></Page></PcGts>"
    )
}

/// Where a defect on the `n`th word of a one-line page is named.
fn word_at(id: Option<&str>, word: usize) -> Located {
    Located {
        page: 0,
        id: id.map(str::to_string),
        line: 0,
        word,
    }
}

/// One case per row of the design's table of what the import tolerates: each
/// leniency a warning naming the element by its `id` or `ID` where it has one
/// and by its page, line and word otherwise, and each refusal its error. The
/// word cap is held at `MAX_IMPORTED_WORDS`: a page of one word more keeps
/// that many and says so once, `TooManyWords` (`bounds_ledger.rs`'s row).
#[test]
fn each_tolerated_defect_is_a_named_warning() {
    let line = |words: &str| hocr_page(&format!("<span class=\"ocr_line\">{words}</span>"));

    // A word with no box, and one whose far edge is before its near one.
    let imported = read(&line(
        "<span class=\"ocrx_word\" id=\"w0\">unboxed</span>\
         <span class=\"ocrx_word\" title=\"bbox 300 100 100 160\">inverted</span>\
         <span class=\"ocrx_word\" title=\"bbox 0 0 10 10\">kept</span>",
    ));
    assert_eq!(
        imported.warnings,
        [
            ImportWarning::WordWithoutBox(word_at(Some("w0"), 0)),
            ImportWarning::InvertedBox(word_at(None, 1)),
        ]
    );
    assert_eq!(words(&imported.pages[0]), ["kept"]);
    let imported = read(&alto_page(
        "<String ID=\"S\" CONTENT=\"short\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"10\"/>\
         <String CONTENT=\"negative\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"-10\" HEIGHT=\"10\"/>",
    ));
    assert_eq!(
        imported.warnings,
        [
            ImportWarning::WordWithoutBox(word_at(Some("S"), 0)),
            ImportWarning::InvertedBox(word_at(None, 1)),
        ]
    );

    // A word whose text is empty once read.
    let imported = read(&line(
        "<span class=\"ocrx_word\" title=\"bbox 0 0 10 10\"> \t\n </span>",
    ));
    assert_eq!(
        imported.warnings,
        [ImportWarning::EmptyWord(word_at(None, 0))]
    );
    assert!(imported.pages[0].page.blocks.is_empty(), "nothing is left");

    // Characters that are no text, each named, and the word kept without them.
    let imported = read(&line(
        "<span class=\"ocrx_word\" title=\"bbox 0 0 10 10\">a&#x1;b</span>",
    ));
    assert_eq!(
        imported.warnings,
        [ImportWarning::NotText {
            at: word_at(None, 0),
            character: '\u{1}'
        }]
    );
    assert_eq!(words(&imported.pages[0]), ["ab"]);
    let imported = read(&alto_page(
        "<String ID=\"S\" CONTENT=\"&#xFEFF;x&#xFDD0;\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"10\" HEIGHT=\"10\"/>",
    ));
    assert_eq!(
        imported.warnings,
        [
            ImportWarning::NotText {
                at: word_at(Some("S"), 0),
                character: '\u{FEFF}'
            },
            ImportWarning::NotText {
                at: word_at(Some("S"), 0),
                character: '\u{FDD0}'
            },
        ]
    );
    assert_eq!(words(&imported.pages[0]), ["x"]);

    // A confidence outside its format's scale, or no number: the word kept
    // with none.
    let imported = read(&line(
        "<span class=\"ocrx_word\" title=\"bbox 0 0 10 10; x_wconf 140\">high</span>\
         <span class=\"ocrx_word\" title=\"bbox 0 0 10 10; x_wconf sure\">word</span>",
    ));
    assert_eq!(
        imported.warnings,
        [
            ImportWarning::ConfidenceOutOfRange {
                at: word_at(None, 0),
                stated: "140".into()
            },
            ImportWarning::ConfidenceOutOfRange {
                at: word_at(None, 1),
                stated: "sure".into()
            },
        ]
    );
    let kept = &imported.pages[0].page.blocks[0].paragraphs[0].lines[0].words;
    assert_eq!(kept.len(), 2);
    assert!(kept.iter().all(|w| w.confidence.is_none()));
    let imported = read(&alto_page(
        "<String CONTENT=\"high\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"10\" HEIGHT=\"10\" WC=\"1.5\"/>",
    ));
    assert_eq!(
        imported.warnings,
        [ImportWarning::ConfidenceOutOfRange {
            at: word_at(None, 0),
            stated: "1.5".into()
        }]
    );
    let imported = read(&page_xml_line(
        "<Word id=\"w\"><Coords points=\"0,0 10,10\"/><TextEquiv conf=\"-0.2\"><Unicode>low</Unicode></TextEquiv></Word>",
    ));
    assert_eq!(
        imported.warnings,
        [ImportWarning::ConfidenceOutOfRange {
            at: word_at(Some("w"), 0),
            stated: "-0.2".into()
        }]
    );

    // A box reaching outside its page, kept as stated.
    let imported = read(&line(
        "<span class=\"ocrx_word\" title=\"bbox 950 100 1100 160\">over</span>",
    ));
    assert_eq!(
        imported.warnings,
        [ImportWarning::OutsidePage(word_at(None, 0))]
    );
    assert_eq!(
        rect(&imported.pages[0].page.blocks[0].paragraphs[0].lines[0].words[0].quad),
        (950.0, 100.0, 1100.0, 160.0)
    );

    // A reading order naming nothing, or naming one region twice: PAGE XML's,
    // the page's; ALTO's, the document's; and an `IDNEXT` naming nothing.
    let page = PAGE_XML.replace(
        "<RegionRefIndexed index=\"0\" regionRef=\"r2\"/>",
        "<RegionRefIndexed index=\"0\" regionRef=\"r2\"/>\
         <RegionRefIndexed index=\"2\" regionRef=\"nowhere\"/>\
         <RegionRefIndexed index=\"3\" regionRef=\"r1\"/>",
    );
    let imported = read(&page);
    assert_eq!(
        imported.warnings,
        [
            ImportWarning::OrderNamesNothing {
                page: Some(0),
                name: "nowhere".into()
            },
            ImportWarning::OrderNamesNothing {
                page: Some(0),
                name: "r1".into()
            },
        ]
    );
    assert_eq!(imported.pages, read(PAGE_XML).pages, "the order stands");
    let imported = read(&ALTO.replace("REF=\"TB_1\"", "REF=\"TB_9\""));
    assert_eq!(
        imported.warnings,
        [ImportWarning::OrderNamesNothing {
            page: None,
            name: "TB_9".into()
        }]
    );
    assert_eq!(
        words(&imported.pages[0]),
        ["Composed", "First", "block", "hyph-", "Unnamed"],
        "the blocks the order does not name follow in document order"
    );
    let chained = ALTO_CHAINED
        .replace("{NS}", "")
        .replace("IDNEXT=\"B\"", "IDNEXT=\"Z\"");
    let imported = read(&chained);
    assert_eq!(
        imported.warnings,
        [ImportWarning::OrderNamesNothing {
            page: Some(0),
            name: "Z".into()
        }]
    );
    assert_eq!(words(&imported.pages[0]), ["one", "two", "three", "four"]);

    // A page past the word cap keeps the cap and says so once.
    let one = "<String CONTENT=\"w\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"1\" HEIGHT=\"1\"/>";
    let imported = read(&alto_page(&one.repeat(MAX_IMPORTED_WORDS + 1)));
    assert_eq!(
        imported.warnings,
        [ImportWarning::TooManyWords {
            page: 0,
            limit: MAX_IMPORTED_WORDS
        }]
    );
    assert_eq!(words(&imported.pages[0]).len(), MAX_IMPORTED_WORDS);

    // The refusals: a unit ALTO does not name, pages with no extent, and
    // markup the reader refuses.
    assert_eq!(
        import(ALTO.replace(">pixel<", ">furlong<").as_bytes()),
        Err(ImportError::UnknownUnit("furlong".into()))
    );
    assert_eq!(
        import(ALTO.replace(" HEIGHT=\"3000\">", ">").as_bytes()),
        Err(ImportError::PageWithoutExtent { page: 0 })
    );
    assert_eq!(
        import(
            HOCR.replace("title=\"bbox 0 0 800 600; ppageno 1\"", "")
                .as_bytes()
        ),
        Err(ImportError::PageWithoutExtent { page: 1 })
    );
    assert_eq!(
        import(PAGE_XML.replace(" imageHeight=\"1600\"", "").as_bytes()),
        Err(ImportError::PageWithoutExtent { page: 0 })
    );
    assert_eq!(
        import(
            ALTO.replace(
                "</TextBlock>\n<ComposedBlock",
                "</TextLine>\n<ComposedBlock"
            )
            .as_bytes()
        ),
        Err(ImportError::Markup(tinker_pdf_xml::Error::MismatchedEndTag))
    );
    let subset = "<?xml version=\"1.0\"?>\n<!DOCTYPE alto [<!ENTITY e \"x\">]>\n<alto/>";
    assert_eq!(
        import(subset.as_bytes()),
        Err(ImportError::Markup(tinker_pdf_xml::Error::InternalSubset)),
        "refused at the subset, before anything in it is read"
    );
}

/// What is none of the three formats is refused as that, by name: a PDF, an
/// SVG, XHTML with no `ocr_page`, an `alto` root in another namespace, a
/// `PcGts` root in no namespace, text, and no bytes at all.
#[test]
fn a_file_that_is_none_of_the_three_is_refused_by_name() {
    let cases: [(&str, &[u8]); 7] = [
        ("a PDF", b"%PDF-1.7\n1 0 obj\n<< >>\nendobj\n%%EOF\n"),
        (
            "an SVG",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><text>ocr_page</text></svg>",
        ),
        (
            "XHTML with no ocr_page",
            b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p class=\"ocr_par\">x</p></body></html>",
        ),
        (
            "alto in another namespace",
            b"<alto xmlns=\"http://example.com/alto\"><Layout/></alto>",
        ),
        ("PcGts in no namespace", b"<PcGts><Page/></PcGts>"),
        ("text", b"Hello, world.\n"),
        ("nothing", b""),
    ];
    for (what, bytes) in cases {
        assert_eq!(import(bytes), Err(ImportError::NotRecognised), "{what}");
    }
}

/// The turns and directions each format states reach the page's line
/// settings: hOCR's `textangle` and `dir`, inherited; ALTO's `ROTATION` and
/// `BASEDIRECTION`; PAGE XML's `orientation` and `readingDirection`.
#[test]
fn turns_and_directions_reach_the_line_settings() {
    let hocr = hocr_page(
        "<div class=\"ocr_carea\" title=\"bbox 0 0 1000 1000\" dir=\"rtl\">\
         <span class=\"ocr_line\" title=\"bbox 0 0 100 100; textangle 90\">\
         <span class=\"ocrx_word\" title=\"bbox 0 0 100 100\">a</span></span>\
         <span class=\"ocr_line\" title=\"bbox 0 200 100 300\" dir=\"ltr\">\
         <span class=\"ocrx_word\" title=\"bbox 0 200 100 300\">b</span></span></div>",
    );
    let page = &read(&hocr).pages[0];
    let settings: Vec<(f64, Option<Direction>)> =
        page.lines.iter().map(|l| (l.turn, l.direction)).collect();
    assert_eq!(
        settings,
        [
            (90.0, Some(Direction::RightToLeft)),
            (0.0, Some(Direction::LeftToRight))
        ]
    );
    let directions: Vec<LineDirection> = page.page.blocks[0].paragraphs[0]
        .lines
        .iter()
        .map(|l| l.direction)
        .collect();
    assert_eq!(
        directions,
        [LineDirection::RightToLeft, LineDirection::LeftToRight]
    );

    let alto = "<alto><Layout><Page ID=\"P\" WIDTH=\"100\" HEIGHT=\"100\"><PrintSpace>\
                <TextBlock ID=\"B\" ROTATION=\"-90\">\
                <TextLine BASEDIRECTION=\"btt\"><String CONTENT=\"up\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"1\" HEIGHT=\"1\"/></TextLine>\
                <TextLine BASEDIRECTION=\"rtl\"><String CONTENT=\"left\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"1\" HEIGHT=\"1\"/></TextLine>\
                </TextBlock><TextBlock ID=\"C\">\
                <TextLine BASEDIRECTION=\"ttb\"><String CONTENT=\"down\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"1\" HEIGHT=\"1\"/></TextLine>\
                </TextBlock></PrintSpace></Page></Layout></alto>";
    let page = &read(alto).pages[0];
    let settings: Vec<(f64, Option<Direction>)> =
        page.lines.iter().map(|l| (l.turn, l.direction)).collect();
    assert_eq!(
        settings,
        [
            (0.0, None),
            (270.0, Some(Direction::RightToLeft)),
            (270.0, None)
        ]
    );
    let vertical = &page.page.blocks[1].paragraphs[0].lines[0];
    assert_eq!(vertical.direction, LineDirection::TopToBottom);

    let page_xml = "<PcGts xmlns=\"http://schema.primaresearch.org/PAGE/gts/pagecontent/2019-07-15\">\
         <Page imageFilename=\"p\" imageWidth=\"100\" imageHeight=\"100\" readingDirection=\"right-to-left\">\
         <TextRegion id=\"r\" orientation=\"180\">\
         <TextLine id=\"a\"><Coords points=\"0,0 1,1\"/><TextEquiv><Unicode>a</Unicode></TextEquiv></TextLine>\
         <TextLine id=\"b\" readingDirection=\"top-to-bottom\"><Coords points=\"0,0 1,1\"/><TextEquiv><Unicode>b</Unicode></TextEquiv></TextLine>\
         </TextRegion></Page></PcGts>";
    let page = &read(page_xml).pages[0];
    let settings: Vec<(f64, Option<Direction>)> =
        page.lines.iter().map(|l| (l.turn, l.direction)).collect();
    assert_eq!(
        settings,
        [(180.0, Some(Direction::RightToLeft)), (90.0, None)]
    );
}

// ---- what a file leaves out, and the orders' edges -----------------------------

/// hOCR that states less than the model holds: lines outside any block are
/// one block a run, a block's lines outside any `ocr_par` one paragraph — the
/// block's box — and words outside any line one line a run; `ocr_column`, the
/// obsolete name, is a block, and an `ocrx_block` inside it is not a second
/// one; an `ocr_header` holding words is a line, and an `ocr_caption` holding
/// a line is not.
#[test]
fn hocr_structure_a_file_leaves_out_is_the_models_rule() {
    let file = hocr_page(
        "<span class=\"ocr_line\" title=\"bbox 10 10 200 50\">\
         <span class=\"ocrx_word\" title=\"bbox 10 10 200 50\">loose</span></span>\
         <span class=\"ocr_line\" title=\"bbox 10 60 200 100\">\
         <span class=\"ocrx_word\" title=\"bbox 10 60 200 100\">lines</span></span>\
         <div class=\"ocr_column\" title=\"bbox 300 0 1000 400\">\
         <div class=\"ocrx_block\" title=\"bbox 300 0 600 100\">\
         <span class=\"ocrx_word\" title=\"bbox 300 10 400 50\">bare</span>\
         <span class=\"ocrx_word\" title=\"bbox 410 10 500 50\">words</span></div>\
         <span class=\"ocr_header\" title=\"bbox 300 200 700 250\">\
         <span class=\"ocrx_word\" title=\"bbox 300 200 500 250\">header</span></span>\
         <span class=\"ocr_caption\" title=\"bbox 300 300 900 400\">\
         <span class=\"ocr_line\" title=\"bbox 300 300 900 350\">\
         <span class=\"ocrx_word\" title=\"bbox 300 300 600 350\">caption</span></span></span>\
         </div>",
    );
    let imported = read(&file);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        shape(&imported.pages[0].page),
        [
            "block (10.0, 10.0, 200.0, 100.0)",
            " paragraph (10.0, 10.0, 200.0, 100.0)",
            "  line (10.0, 10.0, 200.0, 50.0)",
            "   loose (10.0, 10.0, 200.0, 50.0) -",
            "  line (10.0, 60.0, 200.0, 100.0)",
            "   lines (10.0, 60.0, 200.0, 100.0) -",
            "block (300.0, 0.0, 1000.0, 400.0)",
            " paragraph (300.0, 0.0, 1000.0, 400.0)",
            "  line (300.0, 10.0, 500.0, 50.0)",
            "   bare (300.0, 10.0, 400.0, 50.0) -",
            "   words (410.0, 10.0, 500.0, 50.0) -",
            "  line (300.0, 200.0, 700.0, 250.0)",
            "   header (300.0, 200.0, 500.0, 250.0) -",
            "  line (300.0, 300.0, 900.0, 350.0)",
            "   caption (300.0, 300.0, 600.0, 350.0) -",
        ]
    );
    assert_eq!(imported.pages[0].page.index, 0, "no ppageno: its position");
}

/// hOCR's `order` puts sibling lines in its order when every one carries it,
/// and leaves siblings in document order when one does not.
#[test]
fn hocr_order_binds_only_siblings_that_all_carry_it() {
    let file = hocr_page(
        "<div class=\"ocr_carea\" title=\"bbox 0 0 500 500; order 2\"><p class=\"ocr_par\">\
         <span class=\"ocr_line\" title=\"bbox 0 200 100 250; order 3\">\
         <span class=\"ocrx_word\" title=\"bbox 0 200 100 250\">third</span></span>\
         <span class=\"ocr_line\" title=\"bbox 0 0 100 50; order 1\">\
         <span class=\"ocrx_word\" title=\"bbox 0 0 100 50\">first</span></span>\
         <span class=\"ocr_line\" title=\"bbox 0 100 100 150; order 2\">\
         <span class=\"ocrx_word\" title=\"bbox 0 100 100 150\">second</span></span>\
         </p></div>\
         <div class=\"ocr_carea\" title=\"bbox 0 600 500 700\">\
         <span class=\"ocr_line\" title=\"bbox 0 600 100 650\">\
         <span class=\"ocrx_word\" title=\"bbox 0 600 100 650\">last</span></span></div>",
    );
    let imported = read(&file);
    assert_eq!(
        words(&imported.pages[0]),
        ["first", "second", "third", "last"]
    );
}

/// An hOCR file whose markup the HTML parser stops at is refused with the
/// parser's own refusal, rather than read as far as it went.
#[test]
fn hocr_past_the_html_parsers_caps_is_refused_with_its_refusal() {
    let deep = format!(
        "<span class=\"ocr_line\" title=\"bbox 0 0 10 10\">\
         <span class=\"ocrx_word\" title=\"bbox 0 0 10 10\">kept</span></span>{}{}",
        "<div>".repeat(300),
        "</div>".repeat(300)
    );
    assert_eq!(
        import(hocr_page(&deep).as_bytes()),
        Err(ImportError::Markup(tinker_pdf_xml::Error::DepthCap))
    );
}

/// ALTO's `ReadingOrder` at its edges: an unordered group's members in
/// document order among themselves, an `ElementRef` to a `String` placing its
/// block, one to a `ComposedBlock` placing its blocks, one naming a block a
/// second time named and skipped, and the blocks it never names after.
#[test]
fn alto_reading_orders_edges_are_the_designs() {
    let block = |id: &str, word: &str, y: u32| {
        format!(
            "<TextBlock ID=\"{id}\" HPOS=\"0\" VPOS=\"{y}\" WIDTH=\"100\" HEIGHT=\"10\">\
             <TextLine HPOS=\"0\" VPOS=\"{y}\" WIDTH=\"100\" HEIGHT=\"10\">\
             <String ID=\"S_{id}\" CONTENT=\"{word}\" HPOS=\"0\" VPOS=\"{y}\" WIDTH=\"100\" HEIGHT=\"10\"/>\
             </TextLine></TextBlock>"
        )
    };
    let file = format!(
        "<alto xmlns=\"http://www.loc.gov/standards/alto/ns-v4#\"><ReadingOrder>\
         <OrderedGroup ID=\"g\"><UnorderedGroup ID=\"u\">\
         <ElementRef ID=\"e1\" REF=\"S_D\"/><ElementRef ID=\"e2\" REF=\"C\"/></UnorderedGroup>\
         <ElementRef ID=\"e3\" REF=\"CB\"/><ElementRef ID=\"e4\" REF=\"C\"/></OrderedGroup>\
         </ReadingOrder><Layout><Page ID=\"P\" WIDTH=\"1000\" HEIGHT=\"1000\"><PrintSpace>\
         {a}{c}{d}<ComposedBlock ID=\"CB\">{e}{f}</ComposedBlock>{g}\
         </PrintSpace></Page></Layout></alto>",
        a = block("A", "a", 0),
        c = block("C", "c", 100),
        d = block("D", "d", 200),
        e = block("E", "e", 300),
        f = block("F", "f", 400),
        g = block("G", "g", 500),
    );
    let imported = read(&file);
    assert_eq!(
        imported.warnings,
        [ImportWarning::OrderNamesNothing {
            page: None,
            name: "C".into()
        }]
    );
    assert_eq!(words(&imported.pages[0]), ["c", "d", "e", "f", "a", "g"]);
}

/// ALTO's `IDNEXT` at its edges: a cycle no block starts leaves its blocks to
/// document order, after the chains; two chains naming one block name it a
/// second time; and a chain reaching a block on another page ends there, the
/// next page's own.
#[test]
fn alto_idnext_chains_at_their_edges() {
    let block = |id: &str, next: &str, word: &str| {
        let next = if next.is_empty() {
            String::new()
        } else {
            format!(" IDNEXT=\"{next}\"")
        };
        format!(
            "<TextBlock ID=\"{id}\"{next}><TextLine>\
             <String CONTENT=\"{word}\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"1\" HEIGHT=\"1\"/>\
             </TextLine></TextBlock>"
        )
    };
    let page = |id: &str, blocks: &str| {
        format!(
            "<Page ID=\"{id}\" WIDTH=\"100\" HEIGHT=\"100\"><PrintSpace>{blocks}</PrintSpace></Page>"
        )
    };
    let alto = |pages: &str| format!("<alto><Layout>{pages}</Layout></alto>");

    let cycle = read(&alto(&page(
        "P",
        &[
            block("A", "B", "a"),
            block("B", "A", "b"),
            block("C", "", "c"),
        ]
        .concat(),
    )));
    assert!(cycle.warnings.is_empty(), "{:?}", cycle.warnings);
    assert_eq!(words(&cycle.pages[0]), ["c", "a", "b"]);

    let merged = read(&alto(&page(
        "P",
        &[
            block("A", "C", "a"),
            block("B", "C", "b"),
            block("C", "", "c"),
        ]
        .concat(),
    )));
    assert_eq!(
        merged.warnings,
        [ImportWarning::OrderNamesNothing {
            page: Some(0),
            name: "C".into()
        }]
    );
    assert_eq!(words(&merged.pages[0]), ["a", "c", "b"]);

    let two_pages = read(&alto(
        &[
            page(
                "P1",
                &[block("A", "B2", "a"), block("A2", "", "a2")].concat(),
            ),
            page("P2", &block("B2", "", "b2")),
        ]
        .concat(),
    ));
    assert!(two_pages.warnings.is_empty(), "{:?}", two_pages.warnings);
    assert_eq!(two_pages.pages.len(), 2);
    assert_eq!(words(&two_pages.pages[0]), ["a", "a2"]);
    assert_eq!(words(&two_pages.pages[1]), ["b2"]);
    let indices: Vec<u32> = two_pages.pages.iter().map(|p| p.page.index).collect();
    assert_eq!(indices, [0, 1], "no PHYSICAL_IMG_NR: each page's position");
}

/// ALTO's `HYP` with no `String` before it is a word of its own, and one that
/// states its `HEIGHT` stretches its word's box both ways.
#[test]
fn an_alto_hyphen_is_joined_where_it_can_be() {
    let imported = read(&alto_page(
        "<HYP CONTENT=\"-\" HPOS=\"0\" VPOS=\"0\" WIDTH=\"10\" HEIGHT=\"20\"/>\
         <SP HPOS=\"10\" VPOS=\"0\" WIDTH=\"5\"/>\
         <String CONTENT=\"ex\" HPOS=\"20\" VPOS=\"100\" WIDTH=\"100\" HEIGHT=\"50\"/>\
         <HYP CONTENT=\"-\" HPOS=\"120\" VPOS=\"95\" WIDTH=\"20\" HEIGHT=\"60\"/>",
    ));
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let line = &imported.pages[0].page.blocks[0].paragraphs[0].lines[0];
    let read: Vec<(&str, (f64, f64, f64, f64))> = line
        .words
        .iter()
        .map(|w| (w.text.as_str(), rect(&w.quad)))
        .collect();
    assert_eq!(
        read,
        [
            ("-", (0.0, 0.0, 10.0, 20.0)),
            ("ex-", (20.0, 95.0, 140.0, 155.0))
        ]
    );
}

/// PAGE XML's `ReadingOrder` at its edges: an unordered group's members in
/// document order among themselves, a group's own `regionRef` naming a region
/// that holds no text — there, and nothing to place — and a region that holds
/// only regions placing the regions of lines inside it.
#[test]
fn page_xml_reading_orders_edges_are_the_designs() {
    let region = |id: &str, word: &str| {
        format!(
            "<TextRegion id=\"{id}\"><Coords points=\"0,0 10,10\"/>\
             <TextLine id=\"l_{id}\"><Coords points=\"0,0 10,10\"/>\
             <TextEquiv><Unicode>{word}</Unicode></TextEquiv></TextLine></TextRegion>"
        )
    };
    let file = format!(
        "<PcGts xmlns=\"http://schema.primaresearch.org/PAGE/gts/pagecontent/2019-07-15\">\
         <Page imageFilename=\"p\" imageWidth=\"100\" imageHeight=\"100\">\
         <ReadingOrder><OrderedGroup id=\"o\">\
         <UnorderedGroupIndexed id=\"u\" index=\"0\">\
         <RegionRef regionRef=\"r3\"/><RegionRef regionRef=\"r2\"/></UnorderedGroupIndexed>\
         <OrderedGroupIndexed id=\"g\" index=\"1\" regionRef=\"picture\">\
         <RegionRefIndexed index=\"0\" regionRef=\"outer\"/></OrderedGroupIndexed>\
         </OrderedGroup></ReadingOrder>\
         {r1}{r2}{r3}<ImageRegion id=\"picture\"><Coords points=\"0,0 10,10\"/></ImageRegion>\
         <TextRegion id=\"outer\"><Coords points=\"0,0 10,10\"/>{r4}{r5}</TextRegion>\
         </Page></PcGts>",
        r1 = region("r1", "one"),
        r2 = region("r2", "two"),
        r3 = region("r3", "three"),
        r4 = region("r4", "four"),
        r5 = region("r5", "five"),
    );
    let imported = read(&file);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        words(&imported.pages[0]),
        ["two", "three", "four", "five", "one"]
    );
}

/// PAGE XML's details: a resolution in pixels a centimetre carried to the
/// inch, lines not every one of which carries an `index` in document order, a
/// `Word` with no `TextEquiv` empty, and a `Coords` that is not points a word
/// with no box.
#[test]
fn page_xml_details_are_read_as_stated() {
    let file = "<PcGts xmlns=\"http://schema.primaresearch.org/PAGE/gts/pagecontent/2019-07-15\">\
         <Page imageFilename=\"p\" imageWidth=\"1000\" imageHeight=\"1000\" \
         imageXResolution=\"100\" imageYResolution=\"50\" imageResolutionUnit=\"PPCM\">\
         <TextRegion id=\"r\"><Coords points=\"0,0 1000,1000\"/>\
         <TextLine id=\"b\" index=\"1\"><Coords points=\"0,0 100,10\"/>\
         <Word id=\"w0\"><Coords points=\"0,0 10,10\"/><TextEquiv><Unicode>before</Unicode></TextEquiv></Word>\
         <Word id=\"w1\"><Coords points=\"20,0 30,10\"/></Word>\
         <Word id=\"w2\"><Coords points=\"40,0,7 50,10\"/><TextEquiv><Unicode>unboxed</Unicode></TextEquiv></Word>\
         </TextLine>\
         <TextLine id=\"a\"><Coords points=\"0,20 100,30\"/>\
         <TextEquiv><Unicode>after</Unicode></TextEquiv></TextLine>\
         </TextRegion></Page></PcGts>";
    let imported = read(file);
    let page = &imported.pages[0];
    let (x, y) = page.resolution.expect("a resolution");
    assert!(
        (x - 254.0).abs() < 1e-9 && (y - 127.0).abs() < 1e-9,
        "{x} {y}"
    );
    assert!((page.page.resolution - 254.0).abs() < 1e-9);
    assert_eq!(words(page), ["before", "after"], "document order");
    assert_eq!(
        imported.warnings,
        [
            ImportWarning::EmptyWord(Located {
                page: 0,
                id: Some("w1".into()),
                line: 0,
                word: 1
            }),
            ImportWarning::WordWithoutBox(Located {
                page: 0,
                id: Some("w2".into()),
                line: 0,
                word: 2
            }),
        ]
    );
}

/// The page refusals at their edges: an extent with no area, and a `bbox`
/// whose far corner is before its near one; and files whose pages are absent
/// import to no pages.
#[test]
fn pages_without_area_are_refused_and_absent_pages_are_none() {
    assert_eq!(
        import(
            ALTO.replace(
                "WIDTH=\"2000\" HEIGHT=\"3000\">\n<PrintSpace",
                "WIDTH=\"0\" HEIGHT=\"3000\">\n<PrintSpace"
            )
            .as_bytes()
        ),
        Err(ImportError::PageWithoutExtent { page: 0 })
    );
    assert_eq!(
        import(
            HOCR.replace("bbox 0 0 1000 1400", "bbox 1000 1400 0 0")
                .as_bytes()
        ),
        Err(ImportError::PageWithoutExtent { page: 0 })
    );
    for (what, file) in [
        (
            "ALTO with no layout",
            "<alto xmlns=\"http://www.loc.gov/standards/alto/ns-v4#\"><Description/></alto>",
        ),
        (
            "PAGE XML with no page",
            "<PcGts xmlns=\"http://schema.primaresearch.org/PAGE/gts/pagecontent/2019-07-15\"/>",
        ),
    ] {
        let imported = read(file);
        assert!(imported.pages.is_empty(), "{what}");
        assert!(imported.warnings.is_empty(), "{what}");
    }
    let empty = read(&hocr_page(""));
    assert_eq!(empty.pages.len(), 1);
    assert!(empty.pages[0].page.blocks.is_empty());
    assert!(empty.pages[0].lines.is_empty());
    assert_eq!(
        read(&ALTO.replace(">pixel<", "> mm10 <")).pages[0].unit,
        Unit::Mm10,
        "a unit is read with its white space trimmed"
    );
}

// ---- the text layer --------------------------------------------------------------

/// A one-page document `width` by `height` points, its crop box `crop`, turned
/// by `rotate`, drawing `own` lines of text in Helvetica: `(x, y, text)`.
fn base(
    (width, height): (f64, f64),
    crop: Option<[f64; 4]>,
    rotate: i64,
    own: &[(f64, f64, &str)],
) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(width, height, |page| {
        if let Some([x0, y0, x1, y1]) = crop {
            page.set_crop_box(x0, y0, x1, y1);
        }
        for &(x, y, text) in own {
            page.text(b"F0", 10.0, x, y, text);
        }
    });
    let document = written_support::authored("the page", builder.finish());
    if rotate == 0 {
        return document;
    }
    let mut editor = document.editor();
    assert!(editor.rotate_page(0, rotate), "the page turns");
    let turned = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    written_support::authored("the turned page", turned)
}

/// `imported` written over page one of `document`, onto `onto`, the file
/// saved as an incremental update, and reopened through the validate-on-save
/// helper, which holds it to the strict validator as every document this
/// engine authors is held.
fn layered(
    document: &Document,
    imported: &ImportedPage,
    onto: Option<Rect>,
) -> (Document, Layered) {
    let mut editor = document.editor();
    let report = add_text_layer(&mut editor, 0, imported, onto).expect("the layer is written");
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    (written_support::authored("the layered page", saved), report)
}

/// Each line of a model, its words joined by single spaces.
fn model_lines(page: &WordPage) -> Vec<String> {
    page.blocks
        .iter()
        .flat_map(|b| &b.paragraphs)
        .flat_map(|p| &p.lines)
        .map(|l| {
            l.words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// Each line page one's text holds, in order.
fn extracted_lines(document: &Document) -> Vec<String> {
    document
        .page(0)
        .expect("page one")
        .text()
        .lines()
        .iter()
        .map(|line| line.text.clone())
        .collect()
}

/// The imported pages of the three files, each its first.
fn first_pages() -> Vec<(&'static str, ImportedPage)> {
    [("hOCR", HOCR), ("ALTO", ALTO), ("PAGE XML", PAGE_XML)]
        .into_iter()
        .map(|(what, file)| (what, read(file).pages.remove(0)))
        .collect()
}

/// The layer reads back as the file: each format's page written over a page
/// with no text extracts as the imported lines, each its words with one space
/// between each two, in the imported order — the hOCR file's `order` and the
/// ALTO and PAGE XML files' reading orders included — and over a page with
/// text of its own, that text's lines come first and the layer's after.
#[test]
fn the_layer_extracts_as_the_imported_text_in_the_imported_order() {
    for (what, page) in first_pages() {
        let expected = model_lines(&page.page);
        assert!(!expected.is_empty(), "{what}: the file states lines");

        let blank = base((612.0, 792.0), None, 0, &[]);
        let (document, report) = layered(&blank, &page, None);
        assert!(report.warnings.is_empty(), "{what}: {:?}", report.warnings);
        assert!(report.form.is_some(), "{what}: a form was stamped");
        assert_eq!(
            extracted_lines(&document),
            expected,
            "{what} over a blank page"
        );

        let own = base(
            (612.0, 792.0),
            None,
            0,
            &[
                (40.0, 30.0, "Text of the page itself"),
                (40.0, 15.0, "and its second line"),
            ],
        );
        let (document, _) = layered(&own, &page, None);
        let mut with_own = vec![
            "Text of the page itself".to_string(),
            "and its second line".to_string(),
        ];
        with_own.extend(expected);
        assert_eq!(
            extracted_lines(&document),
            with_own,
            "{what} over a page of text"
        );
    }
}

/// The frame a test carries a box back through: the target rectangle on the
/// displayed page (left, top, width and height in points, `y` up), the file's
/// extent, and the displayed page's height.
struct Back {
    left: f64,
    top: f64,
    sx: f64,
    sy: f64,
    from: Extent,
    displayed_height: f64,
}

impl Back {
    fn new(page: &ImportedPage, onto: Option<Rect>, displayed: (f64, f64)) -> Back {
        let (left, top, width, height) = match onto {
            None => (0.0, displayed.1, displayed.0, displayed.1),
            Some(r) => (r.x0, r.y1, r.x1 - r.x0, r.y1 - r.y0),
        };
        Back {
            left,
            top,
            sx: width / page.extent.width(),
            sy: height / page.extent.height(),
            from: page.extent,
            displayed_height: displayed.1,
        }
    }

    /// A box in the render's frame at 72 dpi — points, `y` down from the
    /// displayed page's top — as the file's.
    fn file_box(&self, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        let x = |x: f64| self.from.left + (x - self.left) / self.sx;
        let y = |y: f64| self.from.top + (self.top - (self.displayed_height - y)) / self.sy;
        (x(x0), y(y0), x(x1), y(y1))
    }
}

/// Every word's box comes back as the file states it: the union of the
/// quads of each word's characters, as `Page::word_boxes` takes it, carried
/// back through the frame, is within a thousandth of the file's unit on every
/// edge — on a page turned by each of the four `/Rotate`s with a crop box
/// offset from its media box, and on a turned page placed onto a rectangle
/// the caller names.
#[test]
fn every_word_s_box_equals_the_file_s() {
    let page = read(ALTO).pages.remove(0);
    let expected: Vec<(String, (f64, f64, f64, f64))> = page
        .page
        .blocks
        .iter()
        .flat_map(|b| &b.paragraphs)
        .flat_map(|p| &p.lines)
        .flat_map(|l| &l.words)
        .map(|w| (w.text.clone(), w.quad.bounds()))
        .collect();
    let crop = [36.0, 48.0, 576.0, 744.0];
    let cases: [(i64, Option<Rect>); 5] = [
        (0, None),
        (90, None),
        (180, None),
        (270, None),
        (
            90,
            Some(Rect {
                x0: 50.0,
                y0: 60.0,
                x1: 450.0,
                y1: 300.0,
            }),
        ),
    ];
    for (rotate, onto) in cases {
        let document = base((612.0, 792.0), Some(crop), rotate, &[]);
        let (layered, report) = layered(&document, &page, onto);
        assert_eq!(report.drawn, expected.len());
        let shown = layered.page(0).expect("page one");
        let back = Back::new(&page, onto, shown.size());
        let boxes = shown.word_boxes(&WordBoxOptions::default());
        let found: Vec<(String, (f64, f64, f64, f64))> = boxes
            .blocks
            .iter()
            .flat_map(|b| &b.paragraphs)
            .flat_map(|p| &p.lines)
            .flat_map(|l| &l.words)
            .map(|w| (w.text.clone(), back.file_box(w.quad.bounds())))
            .collect();
        assert_eq!(found.len(), expected.len(), "/Rotate {rotate} {onto:?}");
        for ((text, at), (want, file)) in found.iter().zip(&expected) {
            assert_eq!(text, want, "/Rotate {rotate} {onto:?}");
            let edges = [
                (at.0, file.0),
                (at.1, file.1),
                (at.2, file.2),
                (at.3, file.3),
            ];
            assert!(
                edges.iter().all(|(a, b)| (a - b).abs() < 1e-3),
                "/Rotate {rotate} {onto:?}: {text} came back at {at:?}, the file states {file:?}"
            );
        }
    }
}

/// The layer paints nothing: a page with text of its own renders to the same
/// pixels with the layer as without it, byte for byte, and raises no warning
/// it did not raise before.
#[test]
fn the_page_renders_to_the_same_pixels_as_before() {
    for (what, page) in first_pages() {
        let document = base(
            (300.0, 400.0),
            Some([10.0, 10.0, 290.0, 390.0]),
            0,
            &[
                (30.0, 300.0, "Text the page draws"),
                (30.0, 100.0, "and more of it"),
            ],
        );
        let before = document
            .page(0)
            .expect("page one")
            .render(&RenderOptions::default());
        let (layered, _) = layered(&document, &page, None);
        let after = layered
            .page(0)
            .expect("page one")
            .render(&RenderOptions::default());
        assert_eq!(
            (after.width, after.height),
            (before.width, before.height),
            "{what}"
        );
        assert!(after.data == before.data, "{what}: the layer painted");
        assert!(
            after.warnings.iter().all(|w| before.warnings.contains(w)),
            "{what}: the layer raised {:?} beyond {:?}",
            after.warnings,
            before.warnings
        );
    }
}

/// A line turned by a quarter turn, a half or three quarters is shown along
/// that turn and extracts as one line of its words in order — the run up the
/// page, upside down along it, and down it.
#[test]
fn a_turned_line_extracts_as_one_line() {
    // Each line's words laid along its run: up the page from its foot for a
    // quarter turn, leftwards from the right for a half, down from the top
    // for three quarters.
    let file = hocr_page(
        "<span class=\"ocr_line\" title=\"bbox 100 100 160 700; textangle 90\">\
         <span class=\"ocrx_word\" title=\"bbox 100 500 160 700\">up</span>\
         <span class=\"ocrx_word\" title=\"bbox 100 300 160 480\">the</span>\
         <span class=\"ocrx_word\" title=\"bbox 100 100 160 280\">page</span></span>\
         <span class=\"ocr_line\" title=\"bbox 300 1000 900 1060; textangle 180\">\
         <span class=\"ocrx_word\" title=\"bbox 700 1000 900 1060\">upside</span>\
         <span class=\"ocrx_word\" title=\"bbox 300 1000 680 1060\">down</span></span>\
         <span class=\"ocr_line\" title=\"bbox 850 100 910 700; textangle 270\">\
         <span class=\"ocrx_word\" title=\"bbox 850 100 910 300\">down</span>\
         <span class=\"ocrx_word\" title=\"bbox 850 320 910 500\">the</span>\
         <span class=\"ocrx_word\" title=\"bbox 850 520 910 700\">page</span></span>",
    );
    let page = read(&file).pages.remove(0);
    let turns: Vec<f64> = page.lines.iter().map(|l| l.turn).collect();
    assert_eq!(turns, [90.0, 180.0, 270.0]);
    let blank = base((500.0, 700.0), None, 0, &[]);
    let (document, report) = layered(&blank, &page, None);
    assert_eq!(report.drawn, 8);
    assert_eq!(
        extracted_lines(&document),
        ["up the page", "upside down", "down the page"]
    );
}

/// A line holding right-to-left characters is drawn in the visual order UAX
/// #9 gives its text, each word over its own box, inside a span stating the
/// imported text, and so extracts as imported; a line holding none carries
/// no span.
#[test]
fn a_right_to_left_line_extracts_as_imported() {
    let file = hocr_page(
        "<span class=\"ocr_line\" title=\"bbox 100 100 900 160\" dir=\"rtl\">\
         <span class=\"ocrx_word\" title=\"bbox 700 100 900 160\">\u{5E9}\u{5DC}\u{5D5}\u{5DD}</span>\
         <span class=\"ocrx_word\" title=\"bbox 500 100 680 160\">\u{5E2}\u{5D5}\u{5DC}\u{5DD}</span>\
         <span class=\"ocrx_word\" title=\"bbox 300 100 480 160\">2026</span></span>\
         <span class=\"ocr_line\" title=\"bbox 100 300 900 360\">\
         <span class=\"ocrx_word\" title=\"bbox 100 300 400 360\">plain</span>\
         <span class=\"ocrx_word\" title=\"bbox 420 300 700 360\">text</span></span>",
    );
    let page = read(&file).pages.remove(0);
    let blank = base((612.0, 792.0), None, 0, &[]);
    let (document, report) = layered(&blank, &page, None);
    assert_eq!(report.drawn, 5);
    assert_eq!(
        extracted_lines(&document),
        [
            "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD} 2026",
            "plain text"
        ]
    );
}

/// A word whose box has no width, or no height, cannot be fitted: it is
/// skipped and named by its line and its place in it, and the rest of the
/// page is drawn.
#[test]
fn a_word_without_area_is_skipped_and_named() {
    let file = hocr_page(
        "<span class=\"ocr_line\" title=\"bbox 100 100 900 160\">\
         <span class=\"ocrx_word\" title=\"bbox 100 100 300 160\">kept</span>\
         <span class=\"ocrx_word\" title=\"bbox 320 100 320 160\">thin</span>\
         <span class=\"ocrx_word\" title=\"bbox 340 100 500 100\">flat</span>\
         <span class=\"ocrx_word\" title=\"bbox 520 100 700 160\">also</span></span>",
    );
    let page = read(&file).pages.remove(0);
    assert!(read(&file).warnings.is_empty(), "the import keeps all four");
    let blank = base((612.0, 792.0), None, 0, &[]);
    let (document, report) = layered(&blank, &page, None);
    assert_eq!(
        report.warnings,
        [
            LayerWarning::WordWithoutArea { line: 0, word: 1 },
            LayerWarning::WordWithoutArea { line: 0, word: 2 },
        ]
    );
    assert_eq!(report.drawn, 2);
    assert_eq!(extracted_lines(&document), ["kept also"]);
}

/// The refusals: a page the document does not have, a frame with no area,
/// and more distinct characters than a layer's two-byte codes name; and a
/// page none of whose words can be drawn adds nothing at all.
#[test]
fn a_layer_is_refused_by_name() {
    let page = read(ALTO).pages.remove(0);
    let blank = base((612.0, 792.0), None, 0, &[]);
    let mut editor = blank.editor();
    assert_eq!(
        add_text_layer(&mut editor, 3, &page, None),
        Err(LayerError::NoSuchPage { page: 3 })
    );
    let flat = Rect {
        x0: 10.0,
        y0: 10.0,
        x1: 200.0,
        y1: 10.0,
    };
    assert_eq!(
        add_text_layer(&mut editor, 0, &page, Some(flat)),
        Err(LayerError::NoArea)
    );
    assert!(!editor.is_dirty(), "a refused layer writes nothing");

    // One word of every character from U+20000 on, one more than a layer
    // names.
    let many: String = (0..=MAX_LAYER_CHARACTERS as u32)
        .filter_map(|n| char::from_u32(0x20000 + n))
        .collect();
    assert_eq!(many.chars().count(), MAX_LAYER_CHARACTERS + 1);
    let crowded = one_word_page(&many);
    assert_eq!(
        add_text_layer(&mut editor, 0, &crowded, None),
        Err(LayerError::TooManyCharacters {
            limit: MAX_LAYER_CHARACTERS
        })
    );

    let nothing = one_word_page("");
    let report = add_text_layer(&mut editor, 0, &nothing, None).expect("nothing to draw");
    assert_eq!(
        report,
        Layered {
            form: None,
            drawn: 0,
            warnings: vec![LayerWarning::EmptyWord { line: 0, word: 0 }],
        }
    );
    assert!(!editor.is_dirty(), "an empty layer writes nothing");
}

/// A model page of one word, `text`, filling a 1000 by 100 extent, built
/// directly rather than imported, as a caller may build one.
fn one_word_page(text: &str) -> ImportedPage {
    let quad = |l: f64, t: f64, r: f64, b: f64| Quad {
        ul: (l, t),
        ur: (r, t),
        ll: (l, b),
        lr: (r, b),
    };
    ImportedPage {
        page: WordPage {
            index: 0,
            width: 1000,
            height: 100,
            resolution: 72.0,
            image: None,
            blocks: vec![WordBlock {
                quad: quad(0.0, 0.0, 1000.0, 100.0),
                paragraphs: vec![WordParagraph {
                    quad: quad(0.0, 0.0, 1000.0, 100.0),
                    lines: vec![WordLine {
                        quad: quad(0.0, 0.0, 1000.0, 100.0),
                        direction: LineDirection::LeftToRight,
                        words: vec![WordBox {
                            text: text.to_string(),
                            quad: quad(0.0, 0.0, 1000.0, 100.0),
                            confidence: None,
                        }],
                    }],
                }],
            }],
        },
        extent: Extent {
            left: 0.0,
            top: 0.0,
            right: 1000.0,
            bottom: 100.0,
        },
        unit: Unit::Pixel,
        resolution: None,
        lines: vec![LineSetting {
            turn: 0.0,
            direction: None,
        }],
    }
}

// ---- the property, and the seeds ------------------------------------------------

/// What every import that succeeds holds to, whatever it read: a word with
/// text that is trimmed and holds no character that is no text, every box
/// finite, no page past the cap, a setting for every line, every turn from 0
/// up to a whole turn — and the same answer twice.
fn holds(imported: &Imported, bytes: &[u8]) {
    for page in &imported.pages {
        let mut count = 0;
        for block in &page.page.blocks {
            assert!(block.quad.is_finite());
            for paragraph in &block.paragraphs {
                assert!(paragraph.quad.is_finite());
                for line in &paragraph.lines {
                    assert!(line.quad.is_finite());
                    for word in &line.words {
                        count += 1;
                        assert!(word.quad.is_finite());
                        assert!(!word.text.is_empty());
                        assert_eq!(
                            word.text.trim_matches(|c: char| c.is_ascii_whitespace()),
                            word.text
                        );
                        assert!(!word.text.chars().any(|c| c.is_control() || c == '\u{FEFF}'));
                        if let Some(confidence) = word.confidence {
                            assert!((0.0..=100.0).contains(&confidence));
                        }
                    }
                }
            }
        }
        assert!(count <= MAX_IMPORTED_WORDS);
        assert_eq!(page.lines.len(), lines_of(&page.page));
        assert!(page.lines.iter().all(|l| (0.0..360.0).contains(&l.turn)));
        assert!(page.extent.width() > 0.0 && page.extent.height() > 0.0);
    }
    assert_eq!(
        import(bytes).as_ref(),
        Ok(imported),
        "the same answer twice"
    );
}

/// Bytes, or one of the three files with bytes changed and cut short.
fn inputs() -> impl Strategy<Value = Vec<u8>> {
    let arbitrary = proptest::collection::vec(any::<u8>(), 0..1024);
    let mutated = (
        0usize..SEEDS.len(),
        proptest::collection::vec((any::<usize>(), any::<u8>()), 0..12),
        any::<usize>(),
    )
        .prop_map(|(which, edits, cut)| {
            let mut bytes = SEEDS[which].1.as_bytes().to_vec();
            for (at, byte) in edits {
                let at = at % bytes.len();
                bytes[at] = byte;
            }
            let keep = bytes.len() - cut % (bytes.len() / 4 + 1);
            bytes.truncate(keep);
            bytes
        });
    prop_oneof![arbitrary, mutated]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Ruling 1: no input panics the import — arbitrary bytes, and the three
    /// files mutated and cut short — and what imports holds to `holds`.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in inputs()) {
        if let Ok(imported) = import(&bytes) {
            holds(&imported, &bytes);
        }
    }
}

fn seeds_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/ocr_import")
}

/// The `ocr_import` fuzz target's committed seeds are the three files these
/// tests read, byte for byte, and nothing else: a seed that drifted from its
/// test would fuzz a file nothing states the reading of.
#[test]
fn the_fuzz_seeds_are_the_files_these_tests_read() {
    let dir = seeds_dir();
    if !dir.exists() {
        println!("ocr-import-seeds: SKIPPED, fuzz/corpus/ocr_import is not in this tree");
        return;
    }
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .expect("the corpus reads")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    on_disk.sort();
    let mut named: Vec<String> = SEEDS.iter().map(|(name, _)| name.to_string()).collect();
    named.sort();
    assert_eq!(on_disk, named, "the seeds on disk are the files named");
    for (name, file) in SEEDS {
        let committed = std::fs::read(dir.join(name)).expect("the seed reads");
        assert_eq!(committed, file.as_bytes(), "{name} is its test's file");
        read(file);
    }
    println!("ocr-import-seeds: RAN, {} seeds", SEEDS.len());
}

/// Writes the fuzz target's seeds from the files above. Run with `--ignored`
/// when a file changes.
#[test]
#[ignore = "writes the committed fuzz seeds"]
fn write_the_fuzz_seeds() {
    let dir = seeds_dir();
    std::fs::create_dir_all(&dir).expect("the corpus directory");
    for (name, file) in SEEDS {
        std::fs::write(dir.join(name), file).expect("the seed writes");
    }
}
