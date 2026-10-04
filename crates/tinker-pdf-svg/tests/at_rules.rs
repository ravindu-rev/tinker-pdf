//! At-rules inside an SVG's `<style>` (the roadmap's SVG-in-the-spine row):
//! `@media` evaluated as print, `@import` fetched through the caller's
//! resolver, `@font-face` handed to the caller, and the rest skipped and
//! named.
//!
//! The resolver here is a table of sheets by address, which is all
//! `tinker_pdf_css::ImportResolver` asks for: the facade's is the container a
//! book came out of, and `crates/tinker-pdf/tests/epub_svg.rs` holds that end.

use std::cell::Cell;

use tinker_pdf_css::font_face::FontSource;
use tinker_pdf_css::ImportResolver;
use tinker_pdf_svg::{Colour, Context, Limits, Node, Paint, Scene, Warning};

/// Sheets by address, resolved against nothing — a flat directory — and
/// counting how often each was asked for.
struct Sheets {
    sheets: Vec<(&'static str, &'static str)>,
    asked: Cell<usize>,
}

impl ImportResolver for Sheets {
    fn resolve(&self, href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
        self.asked.set(self.asked.get() + 1);
        self.sheets
            .iter()
            .find(|(address, _)| *address == href)
            .map(|(address, text)| ((*address).to_owned(), text.as_bytes().to_vec()))
    }
}

fn sheets(sheets: &[(&'static str, &'static str)]) -> Sheets {
    Sheets {
        sheets: sheets.to_vec(),
        asked: Cell::new(0),
    }
}

fn read(style: &str, resolver: &Sheets) -> Scene {
    let markup = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">\
         <style>{style}</style><rect class=\"a\" width=\"1\" height=\"1\"/></svg>"
    );
    tinker_pdf_svg::read_with(
        markup.as_bytes(),
        Some((100.0, 100.0)),
        &Limits::DEFAULT,
        &Context::new(resolver),
    )
    .expect("reads")
}

fn fill(scene: &Scene) -> Option<[f64; 3]> {
    scene.nodes.iter().find_map(|node| match node {
        Node::Path {
            fill: Paint::Solid(Colour { rgb }),
            ..
        } => Some(*rgb),
        _ => None,
    })
}

const GREEN: [f64; 3] = [0.0, 1.0, 0.0];

/// **`@media` is evaluated as print**: a page is paper, so a `print` block
/// applies and a `screen` block does not, and a query on the page's width is
/// asked about the viewport. Neither is a skipped at-rule.
#[test]
fn media_is_evaluated_as_print() {
    let none = sheets(&[]);
    for (style, wanted) in [
        (".a { fill: red } @media print { .a { fill: lime } }", GREEN),
        (".a { fill: lime } @media screen { .a { fill: red } }", GREEN),
        (".a { fill: red } @media all and (min-width: 50px) { .a { fill: lime } }", GREEN),
        (".a { fill: lime } @media (min-width: 500px) { .a { fill: red } }", GREEN),
        (".a { fill: red } @media not screen { .a { fill: lime } }", GREEN),
        (
            ".a { fill: red } @media print { @media (orientation: portrait) { .a { fill: lime } } }",
            GREEN,
        ),
    ] {
        let scene = read(style, &none);
        assert_eq!(fill(&scene), Some(wanted), "{style}");
        assert!(scene.warnings.is_empty(), "{style}: {:?}", scene.warnings);
    }
}

/// **`@import` is fetched through the resolver and read in place**, so a
/// rule after it still beats it on source order; one for another medium is
/// not fetched at all; one the resolver does not have is `ImportUnresolved`
/// and the rest of the sheet applies.
#[test]
fn import_is_read_through_the_resolver_in_place() {
    let resolver = sheets(&[
        ("green.css", ".a { fill: lime }"),
        ("red.css", ".a { fill: red }"),
        ("nested.css", "@import 'green.css';"),
    ]);
    let scene = read("@import url(green.css);", &resolver);
    assert_eq!(fill(&scene), Some(GREEN));
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);

    let scene = read("@import 'red.css'; .a { fill: lime }", &resolver);
    assert_eq!(
        fill(&scene),
        Some(GREEN),
        "the sheet's own rule comes after"
    );

    let scene = read("@import 'nested.css';", &resolver);
    assert_eq!(fill(&scene), Some(GREEN), "an import inside an import");

    let asked = resolver.asked.get();
    let scene = read(".a { fill: lime } @import 'red.css';", &resolver);
    assert_eq!(
        fill(&scene),
        Some(GREEN),
        "an import after a rule is invalid"
    );
    assert_eq!(scene.warnings, [Warning::AtRuleIgnored]);
    assert_eq!(resolver.asked.get(), asked, "and is not fetched");

    let scene = read("@import 'red.css' screen; .a { fill: lime }", &resolver);
    assert_eq!(fill(&scene), Some(GREEN));
    assert_eq!(
        resolver.asked.get(),
        asked,
        "another medium's sheet is not fetched"
    );

    let scene = read("@import 'missing.css'; .a { fill: lime }", &resolver);
    assert_eq!(fill(&scene), Some(GREEN));
    assert_eq!(scene.warnings, [Warning::ImportUnresolved]);

    // Without a resolver every import is unresolved, and named.
    let plain = tinker_pdf_svg::read(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><style>@import 'x.css';</style></svg>",
        None,
        &Limits::DEFAULT,
    )
    .expect("reads");
    assert_eq!(plain.warnings, [Warning::ImportUnresolved]);
}

/// **An import cycle is read once, and depth, the token budget and the bytes
/// imported together are bounds**: a sheet importing itself, two importing
/// each other, a chain past `MAX_CSS_IMPORT_DEPTH`, and a sheet that imports
/// one large sheet a thousand times — of rules, or of one comment — each stop
/// rather than recurse or multiply.
#[test]
fn imports_are_bounded() {
    let resolver = sheets(&[
        ("self.css", "@import 'self.css'; .a { fill: lime }"),
        ("ping.css", "@import 'pong.css'; .a { fill: lime }"),
        ("pong.css", "@import 'ping.css';"),
        ("d1.css", "@import 'd2.css';"),
        ("d2.css", "@import 'd3.css';"),
        ("d3.css", "@import 'd4.css';"),
        ("d4.css", "@import 'd5.css';"),
        ("d5.css", "@import 'd6.css';"),
        ("d6.css", "@import 'd7.css';"),
        ("d7.css", "@import 'd8.css';"),
        ("d8.css", "@import 'd9.css';"),
        ("d9.css", "@import 'd10.css';"),
        ("d10.css", ".a { fill: red }"),
    ]);
    // Each sheet of the cycle is read once and the import that closes it is
    // fetched and refused: two fetches for a sheet naming itself, three for a
    // pair. The depth cap would stop a cycle too, and say the same thing —
    // after reading the cycle eight times, which is what the count is for.
    for (start, fetches) in [("self.css", 2), ("ping.css", 3)] {
        let before = resolver.asked.get();
        let scene = read(&format!("@import '{start}';"), &resolver);
        assert_eq!(fill(&scene), Some(GREEN), "{start}");
        assert_eq!(scene.warnings, [Warning::AtRuleIgnored], "{start}");
        assert_eq!(resolver.asked.get() - before, fetches, "{start}");
    }
    let scene = read("@import 'd1.css'; .a { fill: lime }", &resolver);
    assert_eq!(fill(&scene), Some(GREEN));
    assert_eq!(
        scene.warnings,
        [Warning::AtRuleIgnored],
        "past the depth cap"
    );

    // One rule of 1 000 001 tokens imported a thousand times would be a
    // billion; the budget refuses the fourth, past `MAX_CSS_TOKENS`, and
    // nothing after it is fetched, so the work is four sheets' and not a
    // thousand. The rule after the imports still applies.
    let big: &'static str =
        Box::leak(format!(".b {{ fill:{} }}", " r".repeat(500_000)).into_boxed_str());
    let resolver = sheets(&[("big.css", big)]);
    let imports = "@import 'big.css';".repeat(1_000);
    let scene = read(&format!("{imports} .a {{ fill: lime }}"), &resolver);
    assert_eq!(fill(&scene), Some(GREEN));
    assert_eq!(scene.warnings, [Warning::AtRuleIgnored]);
    assert_eq!(
        resolver.asked.get(),
        4,
        "nothing is fetched past the budget"
    );

    // A comment is no tokens, so the token budget cannot see a sheet of one:
    // three megabytes of it imported a thousand times are held by the bytes
    // every import comes to together, `MAX_CSS_BYTES`, which the third
    // crosses. Nothing after it is fetched.
    let quiet: &'static str =
        Box::leak(format!("/*{}*/ .a {{ fill: red }}", "x".repeat(3 << 20)).into_boxed_str());
    let resolver = sheets(&[("quiet.css", quiet)]);
    let imports = "@import 'quiet.css';".repeat(1_000);
    let scene = read(&format!("{imports} .a {{ fill: lime }}"), &resolver);
    assert_eq!(fill(&scene), Some(GREEN));
    assert_eq!(scene.warnings, [Warning::AtRuleIgnored]);
    assert_eq!(resolver.asked.get(), 3, "nothing is fetched past the bytes");
}

/// **`@font-face` is handed to the caller**, with the base it resolves
/// against: `None` for a `<style>` element — the document's own address —
/// and the importing sheet's for one in an import. One with no source is
/// invalid, skipped and named.
#[test]
fn font_face_is_handed_to_the_caller() {
    let resolver = sheets(&[(
        "fonts/faces.css",
        "@font-face { font-family: Imported; src: url(f.woff2) format('woff2') }",
    )]);
    let scene = read(
        "@import 'fonts/faces.css'; \
         @font-face { font-family: 'Local Face'; src: url(face.ttf); font-weight: 700 } \
         @media print { @font-face { font-family: Printed; src: url(p.otf) } } \
         @media screen { @font-face { font-family: Screened; src: url(s.otf) } } \
         @font-face { font-family: NoSource } \
         .a { fill: lime }",
        &resolver,
    );
    let faces: Vec<(&str, Option<&str>)> = scene
        .font_faces
        .iter()
        .map(|f| (f.family.as_str(), f.base.as_deref()))
        .collect();
    assert_eq!(
        faces,
        [
            ("imported", Some("fonts/faces.css")),
            ("local face", None),
            ("printed", None),
        ]
    );
    assert_eq!(scene.font_faces[1].weight, (700, 700));
    assert!(matches!(
        scene.font_faces[0].sources.first(),
        Some(FontSource::Url { url, .. }) if url == "f.woff2"
    ));
    assert_eq!(
        scene.warnings,
        [Warning::AtRuleIgnored],
        "the face with no source"
    );
}

/// **The at-rules that remain are skipped and named**, and the rules around
/// them apply.
#[test]
fn other_at_rules_are_skipped_and_named() {
    let none = sheets(&[]);
    for style in [
        "@keyframes spin { from { opacity: 0 } } .a { fill: lime }",
        "@page { margin: 0 } .a { fill: lime }",
        "@layer base { .a { fill: red } } .a { fill: lime }",
        "@namespace svg url(http://www.w3.org/2000/svg); .a { fill: lime }",
    ] {
        let scene = read(style, &none);
        assert_eq!(fill(&scene), Some(GREEN), "{style}");
        assert_eq!(scene.warnings, [Warning::AtRuleIgnored], "{style}");
    }
    let scene = read("@charset \"utf-8\"; .a { fill: lime }", &none);
    assert!(scene.warnings.is_empty(), "@charset is not a loss");
}
