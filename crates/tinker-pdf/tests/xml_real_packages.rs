//! The XML parser against the only XML in this repository the repository did
//! not write (gap 30, milestone 2).
//!
//! Milestone 1 committed eight genuine XPS packages from two Microsoft
//! producers, and between them they hold **forty-six XML parts**: a
//! content-types item and a package relationships part each, a fixed document
//! sequence, a fixed document, one to three fixed pages, and a page
//! relationships part where there are resources. Every one of them goes through
//! `tinker-pdf-xml` here.
//!
//! This test lives in the facade rather than in the leaf, and the reason is in
//! `crates/tinker-pdf-xml/Cargo.toml`: getting at a part means reading a ZIP,
//! and a leaf that took a dev-dependency on another leaf to test itself would
//! be adding an edge to the graph for the sake of a fixture. Both crates are
//! already here.
//!
//! **What it asserts beyond "they parse".** Gap 29's milestone 6 found a
//! survivor of exactly this shape: `Archive::warnings` was read into a report
//! and *nothing asserted that it was*, so deleting the loop failed nothing. So
//! the numbers below are pinned rather than printed — the deepest nesting, the
//! widest element, the longest name and the largest event count across the
//! whole corpus — and they are the first column of `bounds_ledger.rs`'s table,
//! measured rather than remembered.

use std::path::PathBuf;

use tinker_pdf_xml::{Encoding, Event, Limits, Source, Warning};

// ---- the corpus ---------------------------------------------------------

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/xps")
}

/// Every package in the corpus, by name. Written out rather than globbed, for
/// milestone 1's stated reason: a file deleted from the directory must fail a
/// test rather than shrink a loop to nothing.
const PACKAGES: &[&str] = &[
    "wpf-image-and-text.xps",
    "wpf-shapes-only.xps",
    "wpf-three-pages.xps",
    "wpf-gradients.xps",
    "wpf-tiled-brush.xps",
    "wpf-jpeg-image.xps",
    "xpsom-image-and-text.oxps",
    "xpsom-gradients.oxps",
];

/// The extensions of the items that hold markup. `[Content_Types].xml` and the
/// `.rels` parts are XML; `.fdseq`, `.fdoc` and `.fpage` are the fixed payload.
/// A `.png`, a `.jpg` and an `.ODTTF` are not, and are skipped by name rather
/// than by sniffing.
const MARKUP: &[&str] = &["xml", "rels", "fdseq", "fdoc", "fpage"];

/// Every markup part of every package: package name, item name, bytes.
fn markup_parts() -> Vec<(String, String, Vec<u8>)> {
    let mut parts = Vec::new();
    for name in PACKAGES {
        let path = corpus_dir().join(name);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let mut archive = tinker_pdf_zip::Archive::open(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name} is not readable as a ZIP: {e:?}"));
        let items: Vec<(usize, String)> = archive
            .entries()
            .iter()
            .map(|e| (e.index, e.name.clone()))
            .collect();
        for (index, item) in items {
            let extension = item
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !MARKUP.contains(&extension.as_str()) {
                continue;
            }
            let data = archive
                .read(index)
                .unwrap_or_else(|e| panic!("{name}/{item}: {e:?}"));
            parts.push(((*name).to_string(), item, data.into_owned()));
        }
    }
    parts
}

// ---- what the parser does with them -------------------------------------

/// Every markup part in the corpus parses, cleanly, under the shipped bounds.
#[test]
fn every_markup_part_of_every_real_package_parses() {
    let parts = markup_parts();
    assert_eq!(
        parts.len(),
        46,
        "the corpus should hold forty-six markup parts"
    );
    for (package, item, bytes) in parts {
        let source =
            Source::new(&bytes).unwrap_or_else(|e| panic!("{package}/{item} did not decode: {e}"));
        let mut reader = source.reader(&Limits::DEFAULT);
        let mut events = 0usize;
        while let Some(event) = reader.next() {
            if let Err(error) = event {
                panic!("{package}/{item} at byte {}: {error}", reader.offset());
            }
            events += 1;
        }
        assert!(
            events >= 4,
            "{package}/{item} produced only {events} events"
        );
        assert!(
            reader.warnings().is_empty(),
            "{package}/{item} needed leniency: {:?}",
            reader.warnings()
        );
    }
}

/// **The finding this milestone would have got wrong from ECMA-388 alone.**
///
/// Milestone 1 measured it and this asserts it: WPF writes the UTF-8 signature
/// on every part and the XPS object model writes none. A reader that required a
/// byte order mark refuses every OpenXPS file Windows writes; one that required
/// its absence refuses every XPS file it writes. Both halves are here so that
/// neither mistake can be made silently.
#[test]
fn one_producer_writes_a_byte_order_mark_on_every_part_and_the_other_writes_none() {
    let mut marked = 0usize;
    let mut unmarked = 0usize;
    for (package, item, bytes) in markup_parts() {
        let has_mark = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        if package.starts_with("wpf-") {
            assert!(has_mark, "{package}/{item} has no byte order mark");
            marked += 1;
        } else {
            assert!(!has_mark, "{package}/{item} has a byte order mark");
            unmarked += 1;
        }
        // Either way it is UTF-8, and either way the mark does not reach the
        // text — which is the half a reader that merely *skipped* three bytes
        // when it saw them would also pass, so the encoding is asserted too.
        let source = Source::new(&bytes).expect("decodes");
        assert_eq!(source.encoding(), Encoding::Utf8);
        assert!(source.text().starts_with('<'), "{package}/{item}");
    }
    assert!(
        marked >= 30 && unmarked >= 8,
        "{marked} marked, {unmarked} not"
    );
}

/// Four spellings of the prolog across eight files, including none at all.
///
/// The count is pinned in both directions: a producer that started writing a
/// declaration where it wrote none would change these numbers, and so would a
/// reader that stopped recognising one.
#[test]
fn four_prolog_spellings_appear_across_the_corpus_and_one_of_them_is_no_prolog() {
    let mut spellings: Vec<String> = Vec::new();
    for (_, _, bytes) in markup_parts() {
        let source = Source::new(&bytes).expect("decodes");
        let text = source.text();
        let spelling = match text.find("?>") {
            Some(end) if text.starts_with("<?xml") => text.get(..end + 2).unwrap_or("").to_string(),
            _ => String::from("(no declaration)"),
        };
        if !spellings.contains(&spelling) {
            spellings.push(spelling);
        }
    }
    spellings.sort();
    assert_eq!(
        spellings,
        [
            "(no declaration)",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
            "<?xml version=\"1.0\"?>",
        ],
        "the corpus's prolog spellings changed"
    );
}

/// A comment sits **inside element content** on the first real file, between
/// `<FixedPage>` and `<FixedPage.Resources>`, rather than in the prolog where a
/// hand-written fixture would have put it.
#[test]
fn a_comment_arrives_inside_element_content_on_a_real_fixed_page() {
    let mut comments = Vec::new();
    for (package, item, bytes) in markup_parts() {
        let source = Source::new(&bytes).expect("decodes");
        let mut depth = 0usize;
        for event in source.reader(&Limits::DEFAULT) {
            match event.expect("well formed") {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                Event::Comment(text) => {
                    comments.push((
                        package.clone(),
                        item.clone(),
                        depth,
                        text.trim().to_string(),
                    ));
                }
                _ => {}
            }
        }
    }
    assert_eq!(comments.len(), 2, "{comments:?}");
    for (package, item, depth, text) in &comments {
        assert_eq!(*depth, 1, "{package}/{item}: the comment is not in content");
        assert!(
            text.starts_with("Generated by: Microsoft XPS Object Model"),
            "{package}/{item}: {text}"
        );
    }
}

/// Inter-element whitespace is real, and one producer writes CRLF inside it —
/// so §2.11's line-end normalisation runs on a real file rather than only on a
/// fixture written to exercise it.
#[test]
fn real_inter_element_whitespace_arrives_as_text_with_its_line_ends_normalised() {
    let mut whitespace_runs = 0usize;
    let mut normalised = 0usize;
    for (package, item, bytes) in markup_parts() {
        let source = Source::new(&bytes).expect("decodes");
        let had_crlf = source.text().contains("\r\n");
        for event in source.reader(&Limits::DEFAULT) {
            if let Event::Text(text) = event.expect("well formed") {
                assert!(
                    text.trim().is_empty(),
                    "{package}/{item} holds character data outside an element: {text:?}"
                );
                whitespace_runs += 1;
                if had_crlf {
                    assert!(
                        !text.contains('\r'),
                        "{package}/{item}: a carriage return survived normalisation"
                    );
                    if text.contains('\n') {
                        normalised += 1;
                    }
                }
            }
        }
    }
    assert!(whitespace_runs >= 8, "{whitespace_runs} whitespace runs");
    assert!(normalised >= 4, "{normalised} runs were normalised");
}

/// Both dialects' namespaces are read, resolved, and told apart — and the
/// `x:Key` prefix each producer declares resolves to its own dialect's
/// resource-dictionary-key namespace, which is the thing a reader keyed on the
/// content type could not see.
#[test]
fn both_dialects_namespaces_are_resolved_from_the_markup() {
    const MICROSOFT: &str = "http://schemas.microsoft.com/xps/2005/06";
    const OPENXPS: &str = "http://schemas.openxps.org/oxps/v1.0";

    let mut seen: Vec<(String, String)> = Vec::new();
    for (package, item, bytes) in markup_parts() {
        if !item.ends_with(".fpage") {
            continue;
        }
        let source = Source::new(&bytes).expect("decodes");
        for event in source.reader(&Limits::DEFAULT) {
            if let Event::Start(element) = event.expect("well formed") {
                if element.local() != "FixedPage" {
                    continue;
                }
                let namespace = element
                    .namespace()
                    .expect("a fixed page has one")
                    .to_string();
                assert!(
                    namespace == MICROSOFT || namespace == OPENXPS,
                    "{package}/{item}: {namespace}"
                );
                // `xml:lang` is an ordinary attribute and every real fixed page
                // carries one.
                assert_eq!(
                    element.attribute(Some(tinker_pdf_xml::XML_NAMESPACE), "lang"),
                    Some("en-us"),
                    "{package}/{item}"
                );
                seen.push((package.clone(), namespace));
            }
        }
    }
    assert!(
        seen.iter().any(|(_, ns)| ns == MICROSOFT),
        "no XPS 1.0 fixed page"
    );
    assert!(
        seen.iter().any(|(_, ns)| ns == OPENXPS),
        "no OpenXPS fixed page"
    );

    // And the key prefix, which each producer spells in its own dialect. Two
    // prefixes, two namespaces, one local name — resolved rather than compared
    // as `x:Key`.
    let mut keys = Vec::new();
    for (package, item, bytes) in markup_parts() {
        if !item.ends_with(".fpage") {
            continue;
        }
        let source = Source::new(&bytes).expect("decodes");
        for event in source.reader(&Limits::DEFAULT) {
            if let Event::Start(element) = event.expect("well formed") {
                for attribute in element.attributes() {
                    if attribute.local() == "Key" {
                        keys.push((
                            package.clone(),
                            attribute.namespace().unwrap_or_default().to_string(),
                            attribute.value().to_string(),
                        ));
                    }
                }
            }
        }
    }
    assert!(keys.len() >= 6, "{keys:?}");
    for (package, namespace, value) in &keys {
        let expected = if package.starts_with("wpf-") {
            format!("{MICROSOFT}/resourcedictionary-key")
        } else {
            format!("{OPENXPS}/resourcedictionary-key")
        };
        assert_eq!(namespace, &expected, "{package}");
        assert!(value.starts_with('b'), "{package}: {value}");
    }
}

/// The two spellings of everything, in one corpus, from two Microsoft
/// producers that do not agree with each other — measured through the parser
/// rather than by grepping the bytes.
#[test]
fn one_corpus_holds_two_spellings_of_the_geometry_and_two_of_the_colours() {
    let mut geometries = Vec::new();
    let mut colours = Vec::new();
    for (_, item, bytes) in markup_parts() {
        if !item.ends_with(".fpage") {
            continue;
        }
        let source = Source::new(&bytes).expect("decodes");
        for event in source.reader(&Limits::DEFAULT) {
            if let Event::Start(element) = event.expect("well formed") {
                if let Some(data) = element.attribute(None, "Data") {
                    geometries.push(data.to_string());
                }
                for name in ["Fill", "Color"] {
                    if let Some(value) = element.attribute(None, name) {
                        if value.starts_with('#') {
                            colours.push(value.to_string());
                        }
                    }
                }
            }
        }
    }
    assert!(
        geometries.iter().any(|g| g.starts_with("M0,0L")),
        "no packed abbreviated geometry"
    );
    assert!(
        geometries.iter().any(|g| g.starts_with("M 0,0 L ")),
        "no spaced abbreviated geometry"
    );
    assert!(colours.iter().any(|c| c.len() == 9), "no #AARRGGBB colour");
    assert!(colours.iter().any(|c| c.len() == 7), "no #RRGGBB colour");
}

/// A `.rels` part carries a relative target and a `.fpage` an absolute one for
/// the same part in XPS 1.0, and the object model reverses it. Read through the
/// parser, because that is what milestones 3 and 6 will resolve against.
#[test]
fn a_relative_reference_and_an_absolute_one_name_the_same_part_in_one_package() {
    let mut targets = Vec::new();
    let mut sources = Vec::new();
    for (package, item, bytes) in markup_parts() {
        if !package.contains("image-and-text") {
            continue;
        }
        let source = Source::new(&bytes).expect("decodes");
        for event in source.reader(&Limits::DEFAULT) {
            if let Event::Start(element) = event.expect("well formed") {
                // A part relationships part, whose targets resolve against the
                // *source part* rather than against itself — the off-by-one
                // segment gap 30's design says a first implementation gets
                // wrong. The package relationships part `_rels/.rels` is a
                // different case and writes its target absolutely.
                if item.ends_with(".fpage.rels") {
                    if let Some(target) = element.attribute(None, "Target") {
                        targets.push((package.clone(), target.to_string()));
                    }
                }
                if let Some(image) = element.attribute(None, "ImageSource") {
                    sources.push((package.clone(), image.to_string()));
                }
            }
        }
    }
    // Every target in a page's own relationships part is relative, in both
    // dialects, and climbs three segments to reach `/Resources`.
    assert!(targets.len() >= 4, "{targets:?}");
    for (package, target) in &targets {
        assert!(target.starts_with("../../../"), "{package}: {target}");
    }
    // And the markup reference is absolute in XPS 1.0 and relative in OpenXPS,
    // which is the combination gap 30's design predicted only one half of.
    let xps = sources
        .iter()
        .find(|(p, _)| p.starts_with("wpf-"))
        .expect("an XPS 1.0 image source");
    let oxps = sources
        .iter()
        .find(|(p, _)| p.starts_with("xpsom-"))
        .expect("an OpenXPS image source");
    assert!(xps.1.starts_with('/'), "{:?}", xps);
    assert!(oxps.1.starts_with("../"), "{:?}", oxps);
    assert!(
        xps.1.ends_with(oxps.1.trim_start_matches("../")),
        "the two references do not name the same part: {} and {}",
        xps.1,
        oxps.1
    );
}

// ---- the first column of the bounds ledger, measured --------------------

/// What the real corpus actually spends against each of the four bounds.
///
/// These are the numbers gap 30's bounds section calls *"the most any fixture
/// in this repository legitimately spends"*, and they are measured here rather
/// than remembered — the discipline `tinker-pdf-zip`'s
/// `the_fixtures_in_this_crate_spend_what_the_ledger_says` established. The
/// ledger's own first column is the cap for all four, because the tests that
/// prove those caps fire spend the cap; **this** is the honest figure for real
/// markup, and it is four orders of magnitude below three of them.
#[test]
fn the_real_corpus_spends_what_the_ledger_says_real_markup_spends() {
    let mut deepest = 0usize;
    let mut widest = 0usize;
    let mut longest_name = 0usize;
    let mut most_events = 0usize;

    for (package, item, bytes) in markup_parts() {
        let source = Source::new(&bytes).expect("decodes");
        let mut reader = source.reader(&Limits::DEFAULT);
        let mut depth = 0usize;
        for event in reader.by_ref() {
            match event.unwrap_or_else(|e| panic!("{package}/{item}: {e}")) {
                Event::Start(element) => {
                    depth += 1;
                    deepest = deepest.max(depth);
                    widest = widest.max(element.attributes().len());
                    longest_name = longest_name.max(element.name().qualified().len());
                    for attribute in element.attributes() {
                        longest_name = longest_name.max(attribute.name().qualified().len());
                    }
                }
                Event::End(_) => depth -= 1,
                _ => {}
            }
        }
        most_events = most_events.max(reader.events());
    }

    assert_eq!(deepest, 6, "the deepest nesting in the corpus moved");
    assert_eq!(widest, 8, "the widest element in the corpus moved");
    assert_eq!(longest_name, 33, "the longest name in the corpus moved");
    assert_eq!(most_events, 41, "the largest part in the corpus moved");
}

/// Not one part of the real corpus needs any leniency at all.
///
/// Written in both directions, because gap 29's milestone-6 survivor was a
/// warning list that was read and never asserted: an empty list proves nothing
/// unless something proves the list can be non-empty. The second half puts a
/// carriage return where §2.5 forbids `--` and checks the warning arrives.
#[test]
fn the_real_corpus_needs_no_leniency_and_the_warning_list_still_works() {
    for (package, item, bytes) in markup_parts() {
        let source = Source::new(&bytes).expect("decodes");
        let mut reader = source.reader(&Limits::DEFAULT);
        for event in reader.by_ref() {
            event.expect("well formed");
        }
        assert!(
            reader.warnings().is_empty(),
            "{package}/{item}: {:?}",
            reader.warnings()
        );
    }

    let damaged = b"<a><!-- x -- y --></a>";
    let source = Source::new(damaged).expect("decodes");
    let mut reader = source.reader(&Limits::DEFAULT);
    for event in reader.by_ref() {
        event.expect("well formed");
    }
    assert_eq!(
        reader.warnings(),
        [Warning::DoubleHyphenInComment],
        "the warning list reports nothing at all"
    );
}
