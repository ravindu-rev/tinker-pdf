//! PDF/UA (ISO 14289) measured against the veraPDF corpus's own annotations,
//! through `Document::validate_pdfua`.
//!
//! ```text
//! cargo xtask corpus-fetch
//! TINKER_CORPUS=<abs path to corpus/files> TINKER_CORPUS_REQUIRED=1 \
//!   cargo test --release -p tinker-pdf --test pdfua -- --ignored --nocapture
//! ```
//!
//! With `TINKER_CORPUS` unset the census looks for `corpus/files` beside the
//! workspace, which is where `cargo xtask corpus-fetch` writes. **It did not,
//! until the validator landed**:
//! it read `TINKER_CORPUS` and nothing else, the nightly job sets no such
//! variable, and so the step that lists this census ran it as a skip every
//! night. A census nobody runs is a sentence in a file.
//!
//! # What is claimed, and what is not
//!
//! Everything below goes through `Document::validate_pdfua`, which implements
//! a **part** of ISO 14289 — the part reachable from a structure tree, the
//! catalog, the fonts the pages draw with and an XMP packet. Most of the
//! standard is about whether tagging is *correct*, which is a judgement about
//! meaning that no reader can make: whether a `/P` is really a paragraph,
//! whether the reading order is the author's, whether an `/Alt` describes the
//! picture. The verdict names those clauses in `abstained`, and the census
//! prints them with the word *undecidable* or *staged* beside them — never as
//! a rate.
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
//! and a job depending on it greps its own output for the second; with
//! `TINKER_CORPUS_REQUIRED` set, a skip is a failure.

mod written_support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{
    ConformanceFinding, Document, FindingKind, PdfUaAbstentionClass, PdfUaCoverage, PdfUaPart,
};

#[path = "epub_support/mod.rs"]
mod epub_support;

#[path = "cbz_support/mod.rs"]
mod cbz_support;

/// Printed once when the census ran. CI greps for it.
const RAN: &str = "pdfua-census: RAN";

/// Printed once when it could not. CI greps for this one and fails.
const SKIPPED: &str = "pdfua-census: SKIPPED";

/// A finding kind's name, without its fields: what the census counts by.
fn label(kind: &FindingKind) -> String {
    let debug = format!("{kind:?}");
    debug
        .split([' ', '{', '('])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// One false alarm as the census prints it: the file, and beside it every
/// finding reported on it as a finding displays itself -- its clause, its
/// object and its kind with the kind's fields -- so the line says which code
/// and which widths, not only which rule. `pdfa_ledger.rs` prints a false
/// positive's findings the same way.
fn false_alarm_line(path: &Path, findings: &[ConformanceFinding]) -> String {
    let findings: Vec<String> = findings.iter().map(ToString::to_string).collect();
    format!("{} — {}", path.display(), findings.join("; "))
}

/// What the font a `GlyphWidthInconsistent` finding names states about a
/// code's width, appended to its false alarm: the keys a simple font takes
/// a width from, the `/Widths` entry the code falls on, the descriptor's
/// `/MissingWidth`, and a Type 0 font's descendant's `/W` and `/DW` (MS-14).
/// A false alarm's fields say only that the dictionary and the program
/// disagree; these say which width the dictionary stated.
fn width_sources(doc: &Document, findings: &[ConformanceFinding]) -> String {
    let cos = doc.cos();
    let mut out = String::new();
    for finding in findings {
        let (FindingKind::GlyphWidthInconsistent { code, .. }, Some(object)) =
            (&finding.kind, finding.object)
        else {
            continue;
        };
        let Ok(font) = cos.get(object) else {
            continue;
        };
        let Some(dict) = font.as_dict() else {
            continue;
        };
        let mut said = vec![format!("font {object}")];
        for key in ["Subtype", "BaseFont", "FirstChar", "LastChar", "Encoding"] {
            if let Some(value) = entry(cos, dict, key) {
                said.push(format!("/{key} {}", shown(cos, &value, 0)));
            }
        }
        let first = entry(cos, dict, "FirstChar").and_then(|v| v.as_int());
        if let (Some(first), Some(widths)) = (first, entry(cos, dict, "Widths")) {
            if let Some(widths) = widths.as_array() {
                let at = i64::from(*code) - first;
                let width = usize::try_from(at)
                    .ok()
                    .and_then(|at| widths.get(at))
                    .map_or_else(|| "none".to_string(), |w| shown(cos, w, 0));
                said.push(format!(
                    "/Widths has {} entries, [{at}] {width}",
                    widths.len()
                ));
            }
        }
        if let Some(descriptor) = entry(cos, dict, "FontDescriptor") {
            if let Some(missing) = descriptor
                .as_dict()
                .and_then(|descriptor| entry(cos, descriptor, "MissingWidth"))
            {
                said.push(format!("/MissingWidth {}", shown(cos, &missing, 0)));
            }
        }
        if let Some(descendants) = entry(cos, dict, "DescendantFonts") {
            if let Some(Some(descendant)) = descendants.as_array().map(|all| all.first()) {
                let descendant = resolved(cos, descendant);
                if let Some(descendant) = descendant.as_dict() {
                    for key in ["Subtype", "DW", "W", "CIDToGIDMap"] {
                        if let Some(value) = entry(cos, descendant, key) {
                            said.push(format!("descendant /{key} {}", shown(cos, &value, 0)));
                        }
                    }
                }
            }
        }
        out.push_str(&format!(" [code {code}: {}]", said.join(", ")));
    }
    out
}

/// `key`'s value in `dict`, followed through one reference.
fn entry(
    cos: &tinker_pdf_cos::CosDocument,
    dict: &tinker_pdf_cos::Dict,
    key: &str,
) -> Option<tinker_pdf_cos::Object> {
    dict.iter()
        .find(|(name, _)| cos.name_bytes(*name).as_deref() == Some(key.as_bytes()))
        .map(|(_, value)| resolved(cos, value))
}

/// An object, or what a reference names.
fn resolved(
    cos: &tinker_pdf_cos::CosDocument,
    value: &tinker_pdf_cos::Object,
) -> tinker_pdf_cos::Object {
    match value {
        tinker_pdf_cos::Object::Ref(target) => cos
            .get(*target)
            .map(|object| (*object).clone())
            .unwrap_or(tinker_pdf_cos::Object::Null),
        other => other.clone(),
    }
}

/// An object as short text: names spelled, arrays cut after twenty-four
/// entries, a dictionary or a stream named by its kind.
fn shown(cos: &tinker_pdf_cos::CosDocument, value: &tinker_pdf_cos::Object, depth: u32) -> String {
    use tinker_pdf_cos::Object;
    match value {
        Object::Null => "null".to_string(),
        Object::Bool(b) => b.to_string(),
        Object::Int(i) => i.to_string(),
        Object::Real(r) => r.to_string(),
        Object::Name(name) => format!(
            "/{}",
            String::from_utf8_lossy(&cos.name_bytes(*name).unwrap_or_default())
        ),
        Object::String(_) => "(string)".to_string(),
        Object::Array(items) if depth < 2 => {
            let shown_items: Vec<String> = items
                .iter()
                .take(24)
                .map(|item| shown(cos, item, depth + 1))
                .collect();
            let more = if items.len() > 24 { " ..." } else { "" };
            format!("[{}{more}]", shown_items.join(" "))
        }
        Object::Array(items) => format!("[{} entries]", items.len()),
        Object::Dict(_) => "<<dictionary>>".to_string(),
        Object::Stream(_) => "stream".to_string(),
        Object::Ref(target) => format!("{target} R"),
    }
}

/// Every finding kind the validator reported on one opened document, in the
/// order it reported them, each once.
fn findings_of(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for finding in doc.validate_pdfua().findings {
        let name = label(&finding.kind);
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Whether a missing corpus is a failure rather than a skip.
fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
}

/// The veraPDF corpus's own directory: under `TINKER_CORPUS`, or under the
/// fetch directory beside the workspace.
fn corpus_root() -> Option<PathBuf> {
    let base = match std::env::var("TINKER_CORPUS") {
        Ok(path) => PathBuf::from(path),
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/files")
            .canonicalize()
            .ok()?,
    };
    let root = base.join("verapdf");
    root.is_dir().then_some(root)
}

/// Every PDF/UA fixture under the corpus, with its part, clause and the
/// verdict its name carries.
fn fixtures() -> Option<Vec<(&'static str, String, bool, PathBuf)>> {
    let root = corpus_root()?;
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
            "{SKIPPED} the PDF/UA census -- no annotated PDF_UA-* fixtures under \
             TINKER_CORPUS or corpus/files; fetch with `cargo xtask corpus-fetch`"
        );
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus: this census \
             would have passed over nothing"
        );
        return;
    };
    println!(
        "{RAN} the PDF/UA census ({} annotated files)",
        fixtures.len()
    );

    // (caught, abstained) per clause, over the `-fail-` files only.
    let mut by_clause: BTreeMap<(&str, String), (usize, usize)> = BTreeMap::new();
    let mut by_rule: BTreeMap<String, usize> = BTreeMap::new();
    let mut false_alarms: Vec<String> = Vec::new();
    let mut unopened: Vec<PathBuf> = Vec::new();
    let mut named: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    let (mut caught, mut abstained, mut conforming) = (0usize, 0usize, 0usize);

    for (part, clause, expected, path) in &fixtures {
        let Some(doc) = std::fs::read(path)
            .ok()
            .and_then(|bytes| Document::open(bytes).ok())
        else {
            // A file this engine cannot open says nothing about a rule.
            // Counted, named, and left out of both sides.
            unopened.push(path.clone());
            continue;
        };
        let verdict = doc.validate_pdfua();
        let mut fired: Vec<String> = Vec::new();
        for finding in &verdict.findings {
            let name = label(&finding.kind);
            if !fired.contains(&name) {
                fired.push(name);
            }
        }
        for rule in &fired {
            *by_rule.entry(rule.clone()).or_default() += 1;
        }
        if *expected {
            conforming += 1;
            if !fired.is_empty() {
                false_alarms.push(format!(
                    "{}{}",
                    false_alarm_line(path, &verdict.findings),
                    width_sources(&doc, &verdict.findings)
                ));
            }
            continue;
        }
        let slot = by_clause.entry((part, clause.clone())).or_default();
        if fired.is_empty() {
            abstained += 1;
            slot.1 += 1;
            // What the verdict itself says it did not decide, for the clause
            // directory this file sits in: the abstention, by name.
            let directory = clause.split(' ').next().unwrap_or_default();
            for abstention in &verdict.abstained {
                let gap = abstention.gap.clause;
                if directory.starts_with(gap) || gap.starts_with(directory) {
                    *named
                        .entry((part, gap, abstention.class.word()))
                        .or_default() += 1;
                }
            }
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
    println!("what the abstaining verdicts named, by clause and class:");
    for ((part, clause, word), count) in &named {
        println!("  {part:<10} {clause:<12} {word:<12} {count}");
    }

    println!();
    println!("rules that fired, over every fixture:");
    for (rule, count) in &by_rule {
        println!("  {rule:<36} {count}");
    }

    println!();
    println!("annotated files             {}", fixtures.len());
    println!("  conforming                {conforming}");
    println!("  non-conforming            {}", caught + abstained);
    println!("    caught                  {caught}");
    println!("    abstained               {abstained}");
    println!("could not be opened         {}", unopened.len());
    println!("false alarms                {}", false_alarms.len());
    for line in &false_alarms {
        println!("  {line}");
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
    // 29 is measured, not aimed at — on 16 September 2026, by the census as
    // it stood before the validator existed. The first number written here
    // was 40, guessed before the census had been run, and it was wrong in the
    // flattering direction, which is the reason a floor is recorded from a
    // run rather than from an intention. The rules added since were not
    // measured against the corpus, which was not reachable where they were
    // written, so the floor is not raised for them: the next run records it.
    assert!(caught >= 29, "caught {caught}, and the floor is 29");
    assert!(
        by_rule.len() >= 8,
        "only {} rules ever fired",
        by_rule.len()
    );
}

// ---- the rules on documents small enough to reason about -------------------

/// A one-page tagged document whose elements are `tags`, each around one
/// word, built through the writer so the test is not asserting against its
/// own construction of a `StructureTree`.
fn tagged(tags: &[&[u8]]) -> Document {
    use tinker_pdf::DocumentBuilder;
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
    converted(
        "tagged",
        Document::open(built("tagged", builder)).expect("opens"),
    )
}

/// The heading skips the structure group reports, as `(previous, level)`.
fn skips(doc: &Document) -> Vec<(u8, u8)> {
    doc.validate_pdfua_with(PdfUaCoverage::STRUCTURE)
        .findings
        .into_iter()
        .filter_map(|finding| match finding.kind {
            FindingKind::HeadingLevelSkipped { previous, level } => Some((previous, level)),
            _ => None,
        })
        .collect()
}

/// A false alarm's line names each finding by its clause and with its kind's
/// fields, so a run says which levels, codes or widths each one is.
#[test]
fn a_false_alarm_line_names_its_clause_object_and_fields() {
    let findings = tagged(&[b"H1", b"H3"]).validate_pdfua().findings;
    let line = false_alarm_line(Path::new("PDF_UA-1/a-conforming-pass-a.pdf"), &findings);
    let (path, said) = line.split_once(" — ").expect("the path, then the findings");
    assert_eq!(path, "PDF_UA-1/a-conforming-pass-a.pdf");
    assert!(
        said.contains("7.4.2") && said.contains("HeadingLevelSkipped { previous: 1, level: 3 }"),
        "{line}"
    );
    // Every finding is on it, each as it displays itself.
    for finding in &findings {
        assert!(
            line.contains(&finding.to_string()),
            "{finding} not in {line}"
        );
    }
}

/// The heading walk, on trees small enough to reason about, so the corpus
/// census is not the only thing standing behind it.
#[test]
fn heading_levels_are_an_outline_over_the_whole_document_in_reading_order() {
    assert!(skips(&tagged(&[b"H1", b"H2", b"H3"])).is_empty());
    assert!(skips(&tagged(&[b"H1", b"H2", b"H1", b"H2"])).is_empty());
    assert_eq!(skips(&tagged(&[b"H1", b"H3"])), [(1, 3)]);
    assert_eq!(skips(&tagged(&[b"H2"])), [(0, 2)], "H2 first skips H1");
    assert!(skips(&tagged(&[b"P", b"H1", b"P"])).is_empty());

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
        converted(
            "heading levels are an outline over the whole document in reading order",
            Document::open(built(
                "heading levels are an outline over the whole document in reading order",
                builder,
            ))
            .expect("opens"),
        )
    };
    assert!(
        skips(&nested).is_empty(),
        "three sibling sections holding H1, H2, H3 are one outline"
    );
}

/// No claim through the facade: numbered as part 1 numbers it, told so under
/// clause 5, and the abstentions are part 1's in both classes.
#[test]
fn a_file_claiming_nothing_is_told_so_and_numbered_as_part_one() {
    let verdict = tagged(&[b"P"]).validate_pdfua();
    assert_eq!(verdict.part, None);
    assert!(verdict
        .findings
        .iter()
        .any(|f| f.kind == FindingKind::PdfUaIdentifierMissing && f.clause.0 == "5"));
    for class in [
        PdfUaAbstentionClass::Staged,
        PdfUaAbstentionClass::Undecidable,
    ] {
        assert!(
            verdict
                .abstained
                .iter()
                .any(|a| a.class == class && a.gap.part == PdfUaPart::One),
            "{} missing",
            class.word()
        );
    }
    assert!(verdict.coverage.is_complete());
}

// ---- the census over this engine's own output ------------------------------
//
// The corpus census above measures this engine as a *reader* of other
// producers' files. These two run the same validator over files this engine
// *wrote* — an EPUB converted by the EPUB path, and a document built with
// `DocumentBuilder`'s tagging API — and assert exactly which rules still fire,
// each named with the reason it does. A rule that starts or stops firing on
// our own output is a change to what this engine writes, and has to be
// decided rather than discovered.

/// An EPUB that says everything ISO 14289-1's decidable clauses ask about:
/// a language, headings in order, a described picture, a link, a table with
/// its headers.
fn a_book_that_says_what_it_is() -> Vec<u8> {
    use epub_support::{ocf_zip, OcfEntry};
    const CONTAINER: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
        r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
        r#"</rootfiles></container>"#
    );
    const PACKAGE: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
        r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000057a8</dc:identifier>"#,
        r#"<dc:title>Censused</dc:title><dc:language>en</dc:language>"#,
        r#"</metadata><manifest>"#,
        r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"<item id="i1" href="red.png" media-type="image/png"/>"#,
        r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
    );
    const CHAPTER: &str = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<html xmlns="http://www.w3.org/1999/xhtml" lang="en" xml:lang="en">"#,
        r#"<head><title>T</title></head><body>"#,
        r##"<h1>A title</h1><p>Some <em>words</em> and a <a href="#t">link</a>.</p>"##,
        r#"<h2>A section</h2><p><img src="red.png" alt="A red square"/></p>"#,
        r#"<table summary="Two numbers"><tr><th id="k" scope="col">Key</th></tr>"#,
        r#"<tr><td headers="k">One</td></tr></table><p id="t">The end.</p>"#,
        r#"</body></html>"#
    );
    let picture = cbz_support::rgb_png(4, 4, &[200, 0, 0].repeat(16));
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", CHAPTER.as_bytes()),
        OcfEntry::stored("EPUB/red.png", &picture),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// **The census over this engine's own EPUB output**, with what remains
/// named.
///
/// Before the tagged-writing row this output stated no `/Lang` anywhere, which
/// is `NaturalLanguageMissing`, and a book with a `<figure>` in it wrote a
/// `/Figure` with no description, which is `AlternativeDescriptionMissing`.
/// Removing either half of what closed them puts it back: the catalog's
/// `/Lang` unwritten fires this test and the next, and a picture's `/Alt`
/// unwritten fires this one. What still fires, and why each is not this
/// row's to close:
///
/// - `PdfUaIdentifierMissing`: the output claims no PDF/UA conformance, and
///   should not — a structure tree is necessary for the claim and nowhere
///   near sufficient (`docs/features/epub.md`). Writing `pdfuaid:part` is the
///   PDF/UA design's ledger milestone, not a tagging question.
/// - `FontNotEmbedded`: a book with no `@font-face` is set in the standard
///   14, which the writer does not embed; ISO 14289-1 7.21.4.1 exempts none.
///   That is a font-provision question (`FontProvider`, bundled faces).
/// - `MetadataMissing` and `DisplayDocTitleNotSet`, since milestone 2 of the
///   PDF/UA design: ISO 14289-1 7.1 asks for a metadata stream in the catalog
///   and for `/ViewerPreferences /DisplayDocTitle true`, and the writer, which
///   claims no PDF/UA conformance, writes neither outside the archival
///   profile. Both are the same ledger milestone as the claim itself.
/// - `TabOrderNotStructure`, `AnnotationDescriptionMissing` and
///   `LinkContentsMissing`, since milestone 4's part 1 half of the PDF/UA
///   design (October 2026): the book's one link is an annotation inside a
///   `Link` element, as 7.18.5-1 asks, and the writer gives the page no
///   `/Tabs /S` (7.18.3-1), the annotation no `/Contents` and its element no
///   `/Alt` (7.18.1-2, 7.18.5-2). All three are the tagged writer's to close
///   — it knows the page carries a link and what the link's text says —
///   and none is this validator's.
#[test]
fn this_engines_own_epub_output_is_censused_and_what_remains_is_named() {
    let doc = converted(
        "this engines own epub output is censused and what remains is named",
        Document::open(a_book_that_says_what_it_is()).expect("a book"),
    );
    assert_eq!(
        findings_of(&doc),
        [
            "PdfUaIdentifierMissing",
            "MetadataMissing",
            "DisplayDocTitleNotSet",
            "TabOrderNotStructure",
            "AnnotationDescriptionMissing",
            "LinkContentsMissing",
            "FontNotEmbedded"
        ]
    );
}

/// **The census over a document built with the tagging API**, with what
/// remains named — the same seven, for the same reasons: the builder writes no
/// packet (so no `pdfuaid` claim and no metadata stream) and no viewer
/// preferences, this document is set in an unembedded standard font, and its
/// link, inside its `Link` element as 7.18.5-1 asks, has no `/Contents`, no
/// `/Alt` on the element and no `/Tabs /S` on its page.
/// Everything the tagging API decides — a tree, `/Marked`, described figures,
/// heading order, a language — trips nothing.
#[test]
fn a_tagged_document_builder_document_is_censused_and_what_remains_is_named() {
    use tinker_pdf::{DocumentBuilder, Tag, Target};

    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.set_language("en");
    builder.add_page(300.0, 300.0, |page| {
        page.tagged(b"H1", |page| page.text(b"F1", 18.0, 20.0, 270.0, "Title"));
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 250.0, "See ");
            page.tagged(b"Link", |page| {
                page.text(b"F1", 12.0, 45.0, 250.0, "this");
                page.link(
                    45.0,
                    245.0,
                    70.0,
                    262.0,
                    &Target::Uri("https://example.org/".into()),
                );
            });
        });
        page.tagged(b"H2", |page| page.text(b"F1", 14.0, 20.0, 220.0, "Part"));
        page.tagged_with(&Tag::new(b"Figure").alt("A grey square"), |page| {
            page.fill_rect(20.0, 150.0, 40.0, 40.0, 0.5);
        });
    });
    let doc = converted(
        "a tagged document builder document is censused and what remains is named",
        Document::open(built(
            "a tagged document builder document is censused and what remains is named",
            builder,
        ))
        .expect("opens"),
    );
    assert_eq!(
        findings_of(&doc),
        [
            "PdfUaIdentifierMissing",
            "MetadataMissing",
            "DisplayDocTitleNotSet",
            "TabOrderNotStructure",
            "AnnotationDescriptionMissing",
            "LinkContentsMissing",
            "FontNotEmbedded"
        ]
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
