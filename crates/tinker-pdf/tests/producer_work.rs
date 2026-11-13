//! The producers' linearity census (`docs/verification.md`, "The producers'
//! linearity census"; tier P's P-18).
//!
//! Six producers turn an input into a PDF: a generated book of chapters, an
//! HTML report laid out through `FromHtml`, a Markdown document, an SVG, an
//! XPS package and a CBZ. Each is run at two sizes of its input ten times
//! apart, end to end as a reader meets it — produced, its pages counted and
//! looked up one by one, and each page rendered at 72 dpi — and its work is
//! counted in the three places this engine counts it: what the cascade and the
//! layout spent (`BookCost`), what the synthesis wrote and parsed (the archive
//! report, or the HTML report and the bytes its builder wrote), and what the
//! page walk and the renders took (MS-01's `Stats`). A producer's work is every
//! one of those counts summed; nothing is timed.
//!
//! `producer_work_census` is ignored, and writes `producer_work.tsv`: each
//! producer's work at both sizes, their ratio against 11 for ten times the
//! input — tier P's "1 000 chapters at most 11 times 100" — and the counter
//! whose own ratio is the largest. Run it with
//!
//! ```text
//! cargo test -p tinker-pdf --test producer_work -- --ignored producer_work_census
//! ```
//!
//! Every input is written here or in `epub_support/book.rs`, and every glyph is
//! drawn in `render_support::curvy_font`, supplied when each document opens,
//! so both builds count alike and nothing is read from disk. What the HTML
//! report's builder writes is held to the validate-on-save helper on every
//! run, outside what is counted, and each synthesis is held there once, at a
//! small size, by the test that holds every producer to its shape.

mod epub_support;
mod render_support;
mod written_support;
mod xps_support;

use std::sync::Arc;

use epub_support::book::{generated_book, prose};
use render_support::curvy_font;
use tinker_pdf::epub::BookCost;
use tinker_pdf::{
    ArchiveReport, Document, DocumentBuilder, FontProvider, FromHtml, OpenOptions, PageBox,
    RenderOptions, SimpleFontProvider, Stats,
};
use written_support::{authored, synthesised};
use xps_support::{
    archive, content_types_with, document, grey_jpeg, package_rels, part, sequence, zip, Damage,
    ZipFile, XPS_NS,
};

// ---- the producers ----------------------------------------------------------

/// One producer: its name, what its input is counted in, its two sizes ten
/// times apart, and what makes its document and counts reading it back.
struct Producer {
    name: &'static str,
    unit: &'static str,
    sizes: [usize; 2],
    run: fn(usize) -> Counted,
}

/// The six, in the order the census writes them.
const PRODUCERS: [Producer; 6] = [
    Producer {
        name: "book",
        unit: "chapters",
        sizes: [100, 1_000],
        run: book,
    },
    Producer {
        name: "HTML report",
        unit: "sections",
        sizes: [20, 200],
        run: html_report,
    },
    Producer {
        name: "Markdown document",
        unit: "sections",
        sizes: [20, 200],
        run: markdown_document,
    },
    Producer {
        name: "SVG",
        unit: "shapes",
        sizes: [100, 1_000],
        run: svg,
    },
    Producer {
        name: "XPS package",
        unit: "pages",
        sizes: [10, 100],
        run: xps,
    },
    Producer {
        name: "CBZ",
        unit: "pages",
        sizes: [10, 100],
        run: cbz,
    },
];

/// Ten times the input may cost this much more: tier P's 11 for 1 000
/// chapters against 100, held to every producer.
const BOUND: f64 = 11.0;

/// The one face every glyph is drawn in.
fn face() -> Arc<dyn FontProvider> {
    Arc::new(SimpleFontProvider::new(curvy_font()))
}

/// The options every document opens with: the default box and font size, and
/// [`face`].
fn options() -> OpenOptions {
    OpenOptions::default().with_fonts(face())
}

/// A generated book of `chapters` chapters, opened, paginated and rendered.
fn book(chapters: usize) -> Counted {
    let document =
        Document::open_with(generated_book(chapters), &options()).expect("the book opens");
    counted_synthesis(&document)
}

/// A report of `sections` sections in XHTML, each a heading, two paragraphs
/// and a ruled table of six rows on a page of its own: MS-04's HTML report
/// (`benches/support/inputs.rs`'s `html_report`) at any length.
fn report_markup(sections: usize) -> String {
    let mut body = String::new();
    for section in 1..=sections {
        body.push_str(&format!(
            r#"<div class="section"><h2>Section {section}</h2>"#
        ));
        for paragraph in 0..2 {
            let text = prose((section * 2 + paragraph) as u64, 600);
            body.push_str(&format!("<p>{text}</p>"));
        }
        body.push_str("<table><tr><th>Item</th><th>Count</th><th>Note</th></tr>");
        for row in 1..=5 {
            let note = prose((section * 10 + row) as u64, 8);
            let count = (section * 37 + row * 11) % 1000;
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

/// The report laid out through `FromHtml` at the default box, the PDF its
/// builder wrote held to the validate-on-save helper and opened, and every
/// page rendered.
fn html_report(sections: usize) -> Counted {
    let (width, height) = OpenOptions::default().page;
    let (builder, report) =
        DocumentBuilder::from_html(report_markup(sections), "", PageBox::new(width, height))
            .expect("the report lays out");
    let pdf = builder.finish();
    let mut counted = Counted::default();
    counted.cost(report.cost());
    counted.add("pages laid out", report.pages());
    counted.add("bytes written", pdf.len());
    let document = authored("the HTML report", pdf).with_fonts(face());
    counted.read_back(&document);
    counted
}

/// A CommonMark document of `sections` sections, each a heading, three
/// paragraphs and a list of five items: MS-04's Markdown document
/// (`benches/support/inputs.rs`'s `markdown`) at any length.
fn markdown_text(sections: usize) -> String {
    let mut text = String::new();
    for section in 1..=sections {
        text.push_str(&format!("## Section {section}\n\n"));
        for paragraph in 0..3 {
            text.push_str(&prose((section * 3 + paragraph) as u64, 600));
            text.push_str("\n\n");
        }
        for item in 1..=5 {
            let note = prose((section * 10 + item) as u64, 8);
            text.push_str(&format!("- Item {section}.{item}: {note}\n"));
        }
        text.push('\n');
    }
    text
}

/// The Markdown document opened, paginated and rendered.
fn markdown_document(sections: usize) -> Counted {
    let document = Document::open_markdown(markdown_text(sections).into_bytes(), &options())
        .expect("the Markdown document lays out");
    counted_synthesis(&document)
}

/// An SVG of 500 × 500 px holding `shapes` squares in a grid of fifty a row,
/// each its own colour, so ten times the shapes is ten times the elements on
/// one page of one size.
fn svg_markup(shapes: usize) -> String {
    let mut body = String::new();
    for shape in 0..shapes {
        let (column, row) = (shape % 50, shape / 50 % 50);
        let colour = (shape * 2_654_435_761) & 0xFF_FFFF;
        body.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="8" height="8" fill="#{colour:06x}"/>"##,
            x = column * 10,
            y = row * 10
        ));
    }
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="500" height="500" viewBox="0 0 500 500">{body}</svg>"#
    )
}

/// The SVG opened as its one page and rendered.
fn svg(shapes: usize) -> Counted {
    let document =
        Document::open_with(svg_markup(shapes).into_bytes(), &options()).expect("the SVG opens");
    counted_synthesis(&document)
}

/// An XPS package of `pages` fixed pages of 816 × 1056, each drawing three
/// filled paths and a stroked one (ECMA-388 §11.2): one document, one page
/// part a page, every part deflated.
fn xps_package(pages: usize) -> Vec<u8> {
    let names: Vec<String> = (1..=pages)
        .map(|page| format!("Pages/{page}.fpage"))
        .collect();
    let sources: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut parts = vec![
        part("_rels/.rels", &package_rels("/FixedDocumentSequence.fdseq")),
        part(
            "FixedDocumentSequence.fdseq",
            &sequence(&["Documents/1/FixedDocument.fdoc"]),
        ),
        part("Documents/1/FixedDocument.fdoc", &document(&sources)),
    ];
    for page in 1..=pages {
        let inset = 40 + page % 7 * 10;
        let markup = format!(
            concat!(
                r#"<FixedPage xmlns="{ns}" Width="816" Height="1056">"#,
                r##"<Path Data="M {inset},{inset} L 400,{inset} 400,400 {inset},400 Z" Fill="#FF336699"/>"##,
                r##"<Path Data="M 420,60 L 760,60 760,300 Z" Fill="#FF993366"/>"##,
                r##"<Path Data="M 60,500 L 760,500 760,1000 60,1000 Z" Fill="#FF669933"/>"##,
                r##"<Path Data="M 60,1020 L 760,1020" Stroke="#FF000000" StrokeThickness="2"/>"##,
                r#"</FixedPage>"#
            ),
            ns = XPS_NS,
            inset = inset
        );
        parts.push(part(&format!("Documents/1/Pages/{page}.fpage"), &markup));
    }
    parts.push(part("[Content_Types].xml", &content_types_with("")));
    archive(parts)
}

/// The XPS package opened, its pages synthesised, and every page rendered.
fn xps(pages: usize) -> Counted {
    let document = Document::open_with(xps_package(pages), &options()).expect("the package opens");
    counted_synthesis(&document)
}

/// A comic archive of `pages` pages, each a 16 × 16 grey JPEG of its own size
/// in the archive's natural order.
fn comic(pages: usize) -> Vec<u8> {
    let files: Vec<ZipFile> = (1..=pages)
        .map(|page| {
            let side = 16 + (page % 4) as u16 * 8;
            ZipFile::stored(&format!("page{page:04}.jpg"), &grey_jpeg(side, side))
        })
        .collect();
    zip(&files, Damage::None)
}

/// The CBZ opened, its pages synthesised, and every page rendered.
fn cbz(pages: usize) -> Counted {
    let document = Document::open_with(comic(pages), &options()).expect("the comic opens");
    counted_synthesis(&document)
}

// ---- what a run counted ------------------------------------------------------

/// Every count one run took, by name, in the order it took them.
#[derive(Default)]
struct Counted {
    counters: Vec<(&'static str, u64)>,
}

impl Counted {
    /// `value` more of `name`.
    fn add(&mut self, name: &'static str, value: usize) {
        let value = u64::try_from(value).unwrap_or(u64::MAX);
        match self.counters.iter_mut().find(|(at, _)| *at == name) {
            Some((_, total)) => *total = total.saturating_add(value),
            None => self.counters.push((name, value)),
        }
    }

    /// What the cascade and the layout spent.
    fn cost(&mut self, cost: BookCost) {
        self.add("manifest items", cost.manifest_items);
        self.add("spine items", cost.spine_items);
        self.add("CSS tokens", cost.css_tokens);
        self.add("CSS rules", cost.css_rules);
        self.add("CSS declarations", cost.css_declarations);
        self.add("selector matches", cost.selector_matches);
        self.add("boxes", cost.boxes);
        self.add("break work", cost.break_work);
        self.add("layout work", cost.layout_work);
        self.add("pages fragmented", cost.pages);
    }

    /// What a synthesis's report says it wrote and parsed.
    fn report(&mut self, report: &ArchiveReport) {
        if let Some(cost) = report.book_cost() {
            self.cost(cost);
        }
        self.add("pages synthesised", report.pages().len());
        self.add("parts parsed", report.parsed_parts());
        self.add("bytes written", report.synthesised_bytes());
    }

    /// Every step counter of MS-01's `Stats` — the ones `Stats::work` sums.
    fn stats(&mut self, stats: &Stats) {
        let steps: [(&'static str, u64); 32] = [
            ("glyphs shown", stats.glyphs_shown),
            ("outlines filled", stats.outlines_filled),
            ("glyph masks rasterised", stats.glyph_masks_rasterised),
            ("glyph mask hits", stats.glyph_mask_hits),
            (
                "font dictionaries resolved",
                stats.font_dictionaries_resolved,
            ),
            ("images decoded", stats.images_decoded),
            ("font program hits", stats.font_program_hits),
            ("font program misses", stats.font_program_misses),
            ("image hits", stats.image_hits),
            ("image misses", stats.image_misses),
            ("ICC transform hits", stats.icc_transform_hits),
            ("ICC transform misses", stats.icc_transform_misses),
            ("glyph outline hits", stats.glyph_outline_hits),
            ("glyph outline misses", stats.glyph_outline_misses),
            ("form scope hits", stats.form_scope_hits),
            ("form scope misses", stats.form_scope_misses),
            ("page walks", stats.page_walks),
            ("page nodes visited", stats.page_nodes_visited),
            ("page entries copied", stats.page_entries_copied),
            ("keys looked up", stats.keys_looked_up),
            ("colour spaces parsed", stats.color_spaces_parsed),
            ("functions read", stats.functions_read),
            ("graphics states read", stats.graphics_states_read),
            ("pages interpreted", stats.pages_interpreted),
            ("streams decoded", stats.streams_decoded),
            ("tokens lexed", stats.tokens_lexed),
            ("cross-reference entries read", stats.xref_entries_read),
            ("objects parsed", stats.objects_parsed),
            ("bytes scanned", stats.bytes_scanned),
            ("content tokens read", stats.content_tokens_read),
            ("operators run", stats.operators_run),
            (
                "shading functions evaluated",
                stats.shading_functions_evaluated,
            ),
        ];
        debug_assert_eq!(
            steps.iter().map(|(_, value)| value).sum::<u64>(),
            stats.work(),
            "every counter Stats::work sums, and no other"
        );
        for (name, value) in steps {
            self.add(name, usize::try_from(value).unwrap_or(usize::MAX));
        }
    }

    /// Reading `document` back as a reader does: its page count, each page
    /// looked up by its index, and each rendered at 72 dpi, every door's
    /// tally summed.
    fn read_back(&mut self, document: &Document) {
        let (pages, mut total) = document.page_count_counted();
        assert!(pages > 0, "the producer made no page");
        let options = RenderOptions {
            stats: true,
            ..RenderOptions::at_dpi(72.0)
        };
        for index in 0..pages {
            let (page, lookup) = document.page_counted(index);
            total += lookup;
            let bitmap = page.expect("a counted page is there").render(&options);
            assert!(
                bitmap.width > 0 && bitmap.height > 0,
                "page {index} rendered"
            );
            total += bitmap.stats.expect("a render asked to count counts");
        }
        self.stats(&total);
    }

    /// Every count summed: the producer's work.
    fn work(&self) -> u64 {
        self.counters
            .iter()
            .map(|(_, value)| *value)
            .fold(0, u64::saturating_add)
    }

    /// `name`'s count, zero where this run took none.
    fn get(&self, name: &str) -> u64 {
        self.counters
            .iter()
            .find(|(at, _)| *at == name)
            .map_or(0, |(_, value)| *value)
    }
}

/// A synthesised document's report read, and the document read back.
fn counted_synthesis(document: &Document) -> Counted {
    let mut counted = Counted::default();
    counted.report(
        document
            .archive()
            .expect("a synthesised document has a report"),
    );
    counted.read_back(document);
    counted
}

// ---- the comparison ------------------------------------------------------------

/// One producer at both sizes: the work at each, their ratio, and the counter
/// that grew fastest with its own ratio — a counter none of at the smaller
/// size and some at the larger first, as having grown from nothing.
struct Measured {
    small: u64,
    large: u64,
    ratio: f64,
    fastest: &'static str,
    fastest_ratio: Option<f64>,
}

fn measure(producer: &Producer) -> Measured {
    let [n, ten_n] = producer.sizes;
    assert_eq!(
        ten_n,
        10 * n,
        "{}: the sizes are ten times apart",
        producer.name
    );
    let (small, large) = ((producer.run)(n), (producer.run)(ten_n));
    assert!(
        small.work() > 0,
        "{}: nothing was counted at {n}",
        producer.name
    );
    let mut fastest: Option<(&'static str, Option<f64>)> = None;
    for &(name, value) in &large.counters {
        let before = small.get(name);
        let grew = match (before, value) {
            (_, 0) => continue,
            (0, _) => None,
            (before, value) => Some(value as f64 / before as f64),
        };
        let faster = match (fastest, grew) {
            (None, _) => true,
            (Some((_, None)), _) => false,
            (Some((_, Some(_))), None) => true,
            (Some((_, Some(best))), Some(ratio)) => ratio > best,
        };
        if faster {
            fastest = Some((name, grew));
        }
    }
    let (fastest, fastest_ratio) = fastest.unwrap_or(("none", Some(0.0)));
    Measured {
        small: small.work(),
        large: large.work(),
        ratio: large.work() as f64 / small.work() as f64,
        fastest,
        fastest_ratio,
    }
}

/// The committed table's first line, and the shape of every row after it.
const HEADER: &str = "producer\tunit\tsmall\tlarge\twork at small\twork at large\tratio\tbound\tfastest counter\tits ratio";

/// One row of the table.
fn row(producer: &Producer, measured: &Measured) -> String {
    let [n, ten_n] = producer.sizes;
    let fastest_ratio = measured
        .fastest_ratio
        .map_or_else(|| "from 0".to_owned(), |ratio| format!("{ratio:.2}"));
    format!(
        "{}\t{}\t{n}\t{ten_n}\t{}\t{}\t{:.2}\t{BOUND}\t{}\t{fastest_ratio}",
        producer.name,
        producer.unit,
        measured.small,
        measured.large,
        measured.ratio,
        measured.fastest
    )
}

/// The committed table, which [`producer_work_census`] writes.
const TABLE: &str = include_str!("producer_work.tsv");

/// **The census**: every producer at both sizes, one line each, and the table
/// they make written to `producer_work.tsv`, where it is committed.
#[test]
#[ignore = "the census: writes producer_work.tsv, which is committed"]
fn producer_work_census() {
    let mut table = format!("{HEADER}\n");
    for producer in &PRODUCERS {
        let measured = measure(producer);
        let [n, ten_n] = producer.sizes;
        println!(
            "producer-work {}: {} at {n} {}, {} at {ten_n}, {:.2}x against {BOUND}; fastest {} {}",
            producer.name,
            measured.small,
            producer.unit,
            measured.large,
            measured.ratio,
            measured.fastest,
            measured
                .fastest_ratio
                .map_or_else(|| "from 0".to_owned(), |ratio| format!("{ratio:.2}x")),
        );
        table.push_str(&row(producer, &measured));
        table.push('\n');
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/producer_work.tsv");
    std::fs::write(&path, table).expect("the table is written beside this file");
    println!("producer-work-census: RAN, {} producers", PRODUCERS.len());
}

/// **Every producer is linear in its input** (P-18): each one's work at ten
/// times its input is at most [`BOUND`] times its work at the input, with
/// every counter's own growth named when it is not.
///
/// The book's guard is A2's, `a2_a_thousand_chapter_book_costs_at_most_eleven_times_a_hundred`
/// in `guards.rs`, which reads the same book by the same counts, so its
/// thousand chapters are read once a run rather than twice; this holds the
/// other five.
#[test]
fn every_producer_is_linear_in_its_input() {
    for producer in PRODUCERS.iter().filter(|producer| producer.name != "book") {
        let measured = measure(producer);
        let [n, ten_n] = producer.sizes;
        assert!(
            measured.ratio <= BOUND,
            "{}: work {} at {ten_n} {} against {} at {n}, {:.2}x, over {BOUND}; \
             the counter that grew fastest is {} at {}",
            producer.name,
            measured.large,
            producer.unit,
            measured.small,
            measured.ratio,
            measured.fastest,
            measured
                .fastest_ratio
                .map_or_else(|| "from 0".to_owned(), |ratio| format!("{ratio:.2}x")),
        );
        println!(
            "producer-guard {}: RAN {:.2}x within {BOUND}",
            producer.name, measured.ratio
        );
    }
}

/// The committed table is the census's: its header, and a row of ten fields
/// for a producer the census runs, each at most once, in the census's order.
/// A table the census has not written yet holds its header alone.
#[test]
fn the_committed_table_is_the_censuss_shape() {
    let mut lines = TABLE.lines();
    assert_eq!(
        lines.next(),
        Some(HEADER),
        "the table's header is the census's"
    );
    let mut order = PRODUCERS.iter().map(|producer| producer.name);
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 10, "{line:?}: ten fields");
        assert!(
            order.any(|name| name == fields[0]),
            "{line:?}: a producer the census runs, in its order, once"
        );
        let producer = PRODUCERS
            .iter()
            .find(|producer| producer.name == fields[0])
            .expect("found above");
        assert_eq!(fields[1], producer.unit);
        assert_eq!(fields[2], producer.sizes[0].to_string());
        assert_eq!(fields[3], producer.sizes[1].to_string());
        for count in &fields[4..6] {
            assert!(count.parse::<u64>().is_ok(), "{line:?}: a count");
        }
        assert!(fields[6].parse::<f64>().is_ok(), "{line:?}: a ratio");
        assert_eq!(fields[7], BOUND.to_string());
    }
}

/// Each generator makes what it says: a book of a page a chapter, a report of
/// a page a section, a Markdown document of more than one page, an SVG of one
/// page, and an XPS package and a CBZ of a page a page — each synthesis held
/// to the validate-on-save helper.
#[test]
fn every_producer_makes_the_pages_its_input_says() {
    let pages = |label: &str, bytes: Vec<u8>| {
        let document = Document::open_with(bytes, &options()).expect("it opens");
        synthesised(label, document).page_count() as usize
    };
    assert_eq!(pages("a book", generated_book(5)), 5, "a page a chapter");
    assert_eq!(pages("an SVG", svg_markup(120).into_bytes()), 1, "one page");
    assert_eq!(pages("an XPS package", xps_package(3)), 3, "a page a page");
    assert_eq!(pages("a CBZ", comic(3)), 3, "a page an image");
    let markdown =
        Document::open_markdown(markdown_text(3).into_bytes(), &options()).expect("it lays out");
    let markdown = synthesised("a Markdown document", markdown).page_count();
    assert!(
        markdown >= 2,
        "{markdown} pages for three sections of a page or so"
    );
    let (width, height) = OpenOptions::default().page;
    let (_, report) = DocumentBuilder::from_html(report_markup(3), "", PageBox::new(width, height))
        .expect("it lays out");
    assert_eq!(
        report.pages(),
        3,
        "a page a section, each forced to start one"
    );
    // And the counts each producer's run reads are there to read: what it
    // wrote, and the walk its pages were read back by.
    for producer in &PRODUCERS {
        let counted = (producer.run)(2);
        assert!(counted.work() > 0, "{}: nothing counted", producer.name);
        assert!(
            counted.get("bytes written") > 0,
            "{}: no synthesis was counted",
            producer.name
        );
        assert!(
            counted.get("page walks") > 0,
            "{}: no page was read back",
            producer.name
        );
    }
}
