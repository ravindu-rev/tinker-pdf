//! PDF/UA (ISO 14289) measured against the veraPDF corpus's own annotations.
//!
//! ```text
//! cargo xtask corpus-fetch
//! TINKER_CORPUS=<abs path to corpus/files> \
//!   cargo test --release -p tinker-pdf --test pdfua -- --ignored --nocapture
//! ```
//!
//! # What is claimed, and what is not
//!
//! This engine implements a **small** part of ISO 14289 — the part reachable
//! from a structure tree, a `/MarkInfo` dictionary and an XMP packet. Most of
//! the standard is about whether tagging is *correct*, which is a judgement
//! about meaning that no reader can make: whether a `/P` is really a
//! paragraph, whether the reading order is the author's, whether an `/Alt`
//! describes the picture. Those clauses are not implemented and never will be
//! by a reader alone.
//!
//! So the measurement here is deliberately asymmetric, and the asymmetry is
//! the honest part:
//!
//! - **Caught** — a `-fail-` file where a rule fired. Every one is a real
//!   detection.
//! - **False alarm** — a `-pass-` file where a rule fired. **This must be
//!   zero.** A conforming document that trips one of these rules means the
//!   rule is wrong, and the test fails on any.
//! - **Abstained** — no rule fired. This is *not* a pass. It is this engine
//!   saying nothing, and it is the majority answer.
//!
//! A test that reported "agreement" over all 434 files would be reporting
//! mostly abstentions as successes, which is how a validator comes to claim a
//! conformance level it has not earned.
//!
//! # Ruling 13
//!
//! The corpus supplies **bytes and one annotation each** — `-pass-` or
//! `-fail-` in the filename, and the clause in the directory path. Nothing
//! outside this repository runs, and no outside program adjudicates: every
//! finding below is this engine reading the file. The annotation is the thing
//! being *compared against*, which is what a third party is allowed to be.
//!
//! # Skipped, not silently passed
//!
//! A test over bytes that are not in the repository can fail to run for a
//! reason that looks exactly like a pass, so it prints [`RAN`] or [`SKIPPED`]
//! and a job depending on it greps its own output for the second.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{Document, Object, StructElement, StructKid, StructureTree};

/// Printed once when the census ran. CI greps for it.
const RAN: &str = "pdfua-census: RAN";

/// Printed once when it could not. CI greps for this one and fails.
const SKIPPED: &str = "pdfua-census: SKIPPED";

/// One rule this engine implements, named by the clause it comes from.
///
/// A closed list, because the point of the measurement is what is *not* here
/// as much as what is: a rule added silently would move the caught count
/// without anybody deciding it should.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rule {
    /// UA-1 7.1, UA-2 8.2: the document shall be tagged.
    NoStructureTree,
    /// UA-1 7.1: `/MarkInfo /Marked` shall be true.
    NotMarked,
    /// UA-1 7.1: `/MarkInfo /Suspects` shall not be true.
    Suspects,
    /// UA-1 5, UA-2 5: the XMP shall carry `pdfuaid:part`.
    NoIdentifier,
    /// UA-1 7.3, UA-2 8.4: a `/Figure` shall have `/Alt` or `/ActualText`
    /// (ISO 32000-1 14.9.3).
    FigureWithoutAlt,
    /// UA-1 7.4: heading levels shall not skip a level.
    HeadingLevelSkipped,
    /// UA-1 7.2, UA-2 8.4: the natural language shall be stated somewhere —
    /// `/Lang` on the catalog, or on the structure elements that need it.
    NoNaturalLanguage,
    /// UA-1 7.21.4.1: every font used shall be embedded.
    FontNotEmbedded,
    /// The `/K` walk could not be completed as written — a cycle, a role-map
    /// loop, an unreadable kid. Not a clause of ISO 14289; a document whose
    /// structure tree cannot be walked cannot satisfy any of them.
    TreeNotWalkable,
}

impl Rule {
    fn label(self) -> &'static str {
        match self {
            Rule::NoStructureTree => "no-structure-tree",
            Rule::NotMarked => "not-marked",
            Rule::Suspects => "suspects-true",
            Rule::NoIdentifier => "no-pdfuaid-part",
            Rule::FigureWithoutAlt => "figure-without-alt",
            Rule::HeadingLevelSkipped => "heading-level-skipped",
            Rule::NoNaturalLanguage => "no-natural-language",
            Rule::FontNotEmbedded => "font-not-embedded",
            Rule::TreeNotWalkable => "tree-not-walkable",
        }
    }
}

/// Every rule that fired on one document.
fn findings(path: &Path) -> Result<Vec<Rule>, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let doc = Document::open(bytes).map_err(|error| format!("{error:?}"))?;

    let mut out = Vec::new();
    if uaid_part(doc.xmp_metadata().as_deref()).is_none() {
        out.push(Rule::NoIdentifier);
    }

    let Some(tree) = doc.structure() else {
        out.push(Rule::NoStructureTree);
        return Ok(out);
    };
    if !tree.marked {
        out.push(Rule::NotMarked);
    }
    if tree.suspects {
        out.push(Rule::Suspects);
    }
    if !tree.warnings.is_empty() {
        out.push(Rule::TreeNotWalkable);
    }
    if figure_without_alt(&tree) {
        out.push(Rule::FigureWithoutAlt);
    }
    if heading_level_skipped(&tree) {
        out.push(Rule::HeadingLevelSkipped);
    }
    if !states_a_language(&doc, &tree) {
        out.push(Rule::NoNaturalLanguage);
    }
    if let Some(unembedded) = unembedded_font(&doc) {
        let _ = unembedded;
        out.push(Rule::FontNotEmbedded);
    }
    Ok(out)
}

/// 14.9.3: a `/Figure` stands for content that is not text, so something has
/// to say what it is. `/ActualText` counts as well as `/Alt` — a figure that
/// *is* a word, which is what a dropped capital is, says so with 14.9.4.
fn figure_without_alt(tree: &StructureTree) -> bool {
    tree.elements().into_iter().any(|element| {
        element.standard_type == "Figure" && element.alt.is_none() && element.actual_text.is_none()
    })
}

/// ISO 14289-1 7.4.4: heading levels descend one at a time.
///
/// Measured over each root-to-leaf path rather than over the document as a
/// flat sequence, because a section's first heading is compared against the
/// section it is in and not against whatever the previous section ended on.
fn heading_level_skipped(tree: &StructureTree) -> bool {
    let mut previous = 0u8;
    let mut skipped = false;
    visit_headings(&tree.kids, &mut |level| {
        if level > previous + 1 {
            skipped = true;
        }
        previous = level;
    });
    skipped
}

/// A resource name as text, for a message.
fn name_text(cos: &tinker_pdf_cos::CosDocument, name: tinker_pdf_cos::Name) -> String {
    cos.name_bytes(name)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|| "<unnamed>".to_string())
}

/// The first font a page names that has no embedded program.
///
/// ISO 14289-1 7.21.4.1 requires every font to be embedded, including the
/// standard 14 — a viewer substituting Helvetica is choosing glyphs the
/// author did not, which is the thing the clause exists to stop.
///
/// Scoped to the fonts **pages name**, not to every `/Type /Font` object in
/// the file. An unreferenced font left behind by an editor is not a font the
/// document uses, and counting it would fail a conforming file for something
/// no reader ever draws.
fn unembedded_font(doc: &Document) -> Option<String> {
    let cos = doc.cos();
    let (font_key, subtype, descendants) = (
        cos.intern(b"Font"),
        cos.intern(b"Subtype"),
        cos.intern(b"DescendantFonts"),
    );
    let descriptor = cos.intern(b"FontDescriptor");

    for page in tinker_pdf_cos::pages::collect(cos) {
        let Some(resources) = page.resources.as_ref() else {
            continue;
        };
        let fonts = cos.resolve_key(resources, font_key);
        let Object::Dict(fonts) = &*fonts else {
            continue;
        };
        for (name, entry) in fonts.iter() {
            let font = cos.resolve(entry);
            let Object::Dict(font) = &*font else {
                continue;
            };
            // 9.7.1: a composite font's program hangs off its descendant, so
            // asking the Type0 dictionary alone answers `None` for every one
            // of them — which would report every CID font as unembedded.
            let carrier = match &*cos.resolve_key(font, subtype) {
                Object::Name(name) if cos.name_bytes(*name).as_deref() == Some(&b"Type0"[..]) => {
                    match &*cos.resolve_key(font, descendants) {
                        Object::Array(kids) => match kids.first().map(|kid| cos.resolve(kid)) {
                            Some(kid) => match &*kid {
                                Object::Dict(kid) => kid.clone(),
                                _ => continue,
                            },
                            None => continue,
                        },
                        _ => continue,
                    }
                }
                _ => font.clone(),
            };

            let described = cos.resolve_key(&carrier, descriptor);
            let Object::Dict(described) = &*described else {
                // No descriptor at all is one of the standard 14, which
                // 7.21.4.1 does not exempt.
                return Some(name_text(cos, *name));
            };
            let embedded = [&b"FontFile"[..], b"FontFile2", b"FontFile3"]
                .iter()
                .any(|key| {
                    let key = cos.intern(key);
                    !matches!(&*cos.resolve_key(described, key), Object::Null)
                });
            if !embedded {
                return Some(name_text(cos, *name));
            }
        }
    }
    None
}

/// Whether the document says what language it is in, anywhere.
///
/// ISO 14289-1 7.2 wants the natural language stated for all text, which in
/// general is per-element and not decidable by a reader: an element with no
/// `/Lang` inherits one, and whether the inherited one is *right* for its
/// text is a judgement about meaning. What is decidable is the weakest form
/// of the clause — a document that states no language at all, anywhere,
/// cannot have stated the right one.
///
/// So this is deliberately the loosest reading that is still a rule. It
/// catches the fixtures that say nothing and abstains on every document that
/// says something, which is why it produces no false alarm on 195 conforming
/// files.
fn states_a_language(doc: &Document, tree: &StructureTree) -> bool {
    let cos = doc.cos();
    if let Some(catalog) = cos.catalog() {
        let key = cos.intern(b"Lang");
        if !matches!(&*cos.resolve_key(&catalog, key), Object::Null) {
            return true;
        }
    }
    let mut found = false;
    visit_elements(&tree.kids, &mut |element| {
        if element.lang.is_some() {
            found = true;
        }
    });
    found
}

/// Every structure element in the tree, in reading order, to a visitor.
fn visit_elements(kids: &[StructKid], visit: &mut impl FnMut(&StructElement)) {
    for kid in kids {
        let StructKid::Element(element) = kid else {
            continue;
        };
        visit(element);
        visit_elements(&element.kids, visit);
    }
}

/// Every `Hn` in the tree, in reading order, to a visitor.
///
/// Pre-order, which is 14.8's reading order: an element is read before its
/// children, and its children before its next sibling.
fn visit_headings(kids: &[StructKid], visit: &mut impl FnMut(u8)) {
    for kid in kids {
        let StructKid::Element(element) = kid else {
            continue;
        };
        if let Some(level) = heading_level(element) {
            visit(level);
        }
        visit_headings(&element.kids, visit);
    }
}

/// `H1`…`H6` (ISO 32000-1 Table 335), and PDF 2.0's unbounded `Hn`.
///
/// Two digits at most: `H99` is already past anything a document means, and
/// an unbounded parse would let `H4294967296` decide the answer.
fn heading_level(element: &StructElement) -> Option<u8> {
    let rest = element.standard_type.strip_prefix('H')?;
    if rest.is_empty() || rest.len() > 2 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let level: u8 = rest.parse().ok()?;
    (level > 0).then_some(level)
}

/// `pdfuaid:part` out of an XMP packet.
///
/// A local reader rather than a call into `pdfa.rs`: that module answers about
/// the *PDF/A* identification schema, and the two share a shape and nothing
/// else. Reading one through the other is exactly the confusion that made 434
/// PDF/UA files read as PDF/A claims once already.
fn uaid_part(packet: Option<&[u8]>) -> Option<u32> {
    const UA_ID_NAMESPACE: &str = "http://www.aiim.org/pdfua/ns/id/";
    let packet = packet?;
    let source = tinker_pdf_xml::Source::new(packet).ok()?;
    let limits = tinker_pdf_xml::Limits::default();

    let is_uaid = |name: &tinker_pdf_xml::Name<'_>| {
        name.prefix() == Some("pdfuaid") || name.namespace() == Some(UA_ID_NAMESPACE)
    };

    let mut collecting = false;
    for event in source.reader(&limits) {
        let Ok(event) = event else {
            break;
        };
        match event {
            tinker_pdf_xml::Event::Start(element) => {
                for attribute in element.attributes() {
                    if is_uaid(attribute.name()) && attribute.name().local() == "part" {
                        if let Ok(part) = attribute.value().trim().parse() {
                            return Some(part);
                        }
                    }
                }
                collecting = is_uaid(element.name()) && element.local() == "part";
            }
            tinker_pdf_xml::Event::Text(text) | tinker_pdf_xml::Event::Cdata(text) => {
                if collecting {
                    if let Ok(part) = text.trim().parse() {
                        return Some(part);
                    }
                }
            }
            _ => collecting = false,
        }
    }
    None
}

/// Every PDF/UA fixture under `TINKER_CORPUS`, with its part, clause and the
/// verdict its name carries.
fn fixtures() -> Option<Vec<(&'static str, String, bool, PathBuf)>> {
    let root = PathBuf::from(std::env::var_os("TINKER_CORPUS")?).join("verapdf");
    let mut out = Vec::new();
    for part in ["PDF_UA-1", "PDF_UA-2"] {
        let base = root.join(part);
        let mut stack = vec![base.clone()];
        while let Some(current) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "pdf") {
                    continue;
                }
                let name = path.file_name()?.to_string_lossy().to_string();
                // Only the annotated ones. A fixture with neither word in its
                // name states no verdict, and guessing one would put this
                // engine's answer on both sides of the comparison.
                let expected = if name.contains("-pass-") {
                    true
                } else if name.contains("-fail-") {
                    false
                } else {
                    continue;
                };
                let clause = path
                    .strip_prefix(&base)
                    .ok()?
                    .components()
                    .next()?
                    .as_os_str()
                    .to_string_lossy()
                    .to_string();
                out.push((part, clause, expected, path));
            }
        }
    }
    if out.is_empty() {
        return None;
    }
    out.sort();
    Some(out)
}

/// The census, and the assertion that no conforming file trips a rule.
#[test]
#[ignore = "reads the fetched corpora; set TINKER_CORPUS=corpus/files"]
fn the_implemented_clauses_catch_failures_and_never_conforming_files() {
    let Some(fixtures) = fixtures() else {
        println!(
            "{SKIPPED} the PDF/UA census -- TINKER_CORPUS is unset or holds no \
             annotated PDF_UA-* fixtures; fetch with `cargo xtask corpus-fetch` \
             and point it at corpus/files"
        );
        return;
    };
    println!(
        "{RAN} the PDF/UA census ({} annotated files)",
        fixtures.len()
    );

    // (caught, abstained) per clause, over the `-fail-` files only.
    let mut by_clause: BTreeMap<(&str, String), (usize, usize)> = BTreeMap::new();
    let mut by_rule: BTreeMap<Rule, usize> = BTreeMap::new();
    let mut false_alarms: Vec<(PathBuf, Vec<Rule>)> = Vec::new();
    let mut unopened: Vec<PathBuf> = Vec::new();
    let (mut caught, mut abstained, mut conforming) = (0usize, 0usize, 0usize);

    for (part, clause, expected, path) in &fixtures {
        let fired = match findings(path) {
            Ok(fired) => fired,
            Err(_) => {
                // A file this engine cannot open says nothing about a rule.
                // Counted, named, and left out of both sides.
                unopened.push(path.clone());
                continue;
            }
        };
        for rule in &fired {
            *by_rule.entry(*rule).or_default() += 1;
        }
        if *expected {
            conforming += 1;
            if !fired.is_empty() {
                false_alarms.push((path.clone(), fired));
            }
            continue;
        }
        let slot = by_clause.entry((part, clause.clone())).or_default();
        if fired.is_empty() {
            abstained += 1;
            slot.1 += 1;
        } else {
            caught += 1;
            slot.0 += 1;
        }
    }

    println!();
    println!("non-conforming fixtures, by clause:");
    println!(
        "{:<10} {:<38} {:>7} {:>10}",
        "part", "clause", "caught", "abstained"
    );
    for ((part, clause), (hit, miss)) in &by_clause {
        println!("{part:<10} {clause:<38} {hit:>7} {miss:>10}");
    }

    println!();
    println!("rules that fired, over every fixture:");
    for (rule, count) in &by_rule {
        println!("  {:<24} {count}", rule.label());
    }

    println!();
    println!("annotated files             {}", fixtures.len());
    println!("  conforming                {conforming}");
    println!("  non-conforming            {}", caught + abstained);
    println!("    caught                  {caught}");
    println!("    abstained               {abstained}");
    println!("could not be opened         {}", unopened.len());
    println!("false alarms                {}", false_alarms.len());
    for (path, fired) in &false_alarms {
        let names: Vec<&str> = fired.iter().map(|rule| rule.label()).collect();
        println!("  {} — {}", path.display(), names.join(", "));
    }

    // The assertion that matters, and the only symmetric one available: a
    // document the corpus calls conforming must trip no rule this engine
    // implements. A rule that fires on a conforming file is wrong, whatever
    // it catches elsewhere, because the cost of a false accusation is a
    // caller who stops believing the true ones.
    assert!(
        false_alarms.is_empty(),
        "{} conforming documents tripped a rule; see the list above",
        false_alarms.len()
    );

    // Floors, not equalities. They move upward when a rule is added and are
    // what says the census did not quietly stop finding things.
    //
    // 29 is measured, not aimed at. The first number written here was 40,
    // guessed before the census had been run, and it was wrong in the
    // flattering direction — which is the reason a floor is recorded from a
    // run rather than from an intention.
    assert!(caught >= 29, "caught {caught}, and the floor is 29");
    assert!(
        by_rule.len() >= 8,
        "only {} rules ever fired",
        by_rule.len()
    );
}

/// The heading walk, on trees small enough to reason about, so the corpus
/// census is not the only thing standing behind it.
#[test]
fn heading_levels_are_an_outline_over_the_whole_document_in_reading_order() {
    // Built through the writer, because a hand-rolled `StructureTree` would
    // be this test asserting against its own construction.
    use tinker_pdf::DocumentBuilder;

    let tree_of = |tags: &[&[u8]]| {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F1", b"Helvetica");
        let tags: Vec<Vec<u8>> = tags.iter().map(|tag| tag.to_vec()).collect();
        builder.add_page(300.0, 200.0, |page| {
            for (index, tag) in tags.iter().enumerate() {
                page.tagged(tag, |page| {
                    page.text(b"F1", 12.0, 20.0, 180.0 - index as f64 * 20.0, "x");
                });
            }
        });
        Document::open(builder.finish())
            .expect("opens")
            .structure()
            .expect("a tree")
    };

    assert!(!heading_level_skipped(&tree_of(&[b"H1", b"H2", b"H3"])));
    assert!(!heading_level_skipped(&tree_of(&[
        b"H1", b"H2", b"H1", b"H2"
    ])));
    assert!(heading_level_skipped(&tree_of(&[b"H1", b"H3"])));
    assert!(
        heading_level_skipped(&tree_of(&[b"H2"])),
        "H2 first skips H1"
    );
    assert!(!heading_level_skipped(&tree_of(&[b"P", b"H1", b"P"])));

    // The case that overturned the first reading of this rule. `7.4.2-t01-
    // pass-d.pdf` is a conforming file whose H1, H2 and H3 sit in three
    // *sibling* `Sect`s, one heading each. A rule that carried the deepest
    // level only downwards saw each `Sect` start from nothing and called the
    // H2 a skip — a false alarm on a document the corpus says conforms.
    //
    // 7.4.2 is about the outline a reader hears, and that outline is the
    // headings in reading order (14.8) regardless of what contains them. So
    // the level carries across siblings and out of containers, and only the
    // *previous heading* decides whether the next one skips.
    let nested = {
        let mut builder = tinker_pdf::DocumentBuilder::new();
        builder.add_base_font(b"F1", b"Helvetica");
        builder.add_page(300.0, 200.0, |page| {
            for (index, level) in [&b"H1"[..], b"H2", b"H3"].iter().enumerate() {
                page.tagged(b"Sect", |page| {
                    page.tagged(level, |page| {
                        page.text(b"F1", 12.0, 20.0, 180.0 - index as f64 * 20.0, "x");
                    });
                });
            }
        });
        Document::open(builder.finish())
            .expect("opens")
            .structure()
            .expect("a tree")
    };
    assert!(
        !heading_level_skipped(&nested),
        "three sibling sections holding H1, H2, H3 are one outline"
    );
}

/// `pdfuaid:part` in both spellings a real packet uses, and never
/// `pdfaid:part` — the confusion that read 434 PDF/UA files as PDF/A claims.
#[test]
fn the_ua_identifier_is_read_and_is_not_the_pdfa_one() {
    let attribute = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
 pdfuaid:part="1"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    assert_eq!(uaid_part(Some(attribute)), Some(1));

    let element = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/">
<pdfuaid:part>2</pdfuaid:part></rdf:Description></rdf:RDF></x:xmpmeta>"#;
    assert_eq!(uaid_part(Some(element)), Some(2));

    let pdfa = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="2" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#;
    assert_eq!(
        uaid_part(Some(pdfa)),
        None,
        "a PDF/A claim is not a PDF/UA one"
    );
    assert_eq!(uaid_part(None), None);
}
