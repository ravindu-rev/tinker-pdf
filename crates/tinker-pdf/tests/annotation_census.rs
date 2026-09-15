//! What the read surface makes of the fetched corpora: every annotation on
//! every page, and every font every page can reach.
//!
//! Two censuses, both `#[ignore]`d because they walk a fetched tree:
//!
//! ```sh
//! cargo test -p tinker-pdf --test annotation_census -- --ignored --nocapture
//! ```
//!
//! A census is not a unit test and does not pretend to be one. The fixtures
//! beside `src/annotations.rs`, `src/fontlist.rs` and in `tests/facade_read.rs`
//! answer "does the model say the right thing about a shape I chose"; only a
//! corpus answers "is the shape I chose the shape real producers emit". The
//! roadmap row's exit criterion — *every corpus annotation read and the refused
//! ones counted by subtype* — is a question of the second kind, and this file
//! is the answer to it.
//!
//! **`RAN`/`SKIPPED` is printed on the first line of each**, because a census
//! whose corpus is missing would otherwise be a passing test that measured
//! nothing. Point it at the tree with `TINKER_CORPUS`; the default is
//! `corpus/files` beside the workspace.
//!
//! # What adjudicates what
//!
//! The corpus is third-party data: the counts below are facts about files this
//! project did not write, which is what makes them worth taking. They are not
//! a third-party *verdict* — nothing here asks another program whether an
//! answer is right (ruling 13). Where two readers in this tree are compared
//! (`Page::links` against `Page::annotations`, below) the comparison is named
//! **self-consistency** and proves only that two independently written walks of
//! one array agree; what pins either of them to the standard is the
//! clause-cited fixtures elsewhere.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{AnnotationKind, Document, FontKind, ProgramKey};

// ---- finding the corpus ---------------------------------------------------

fn corpus_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/files");
    root.canonicalize().ok().filter(|path| path.is_dir())
}

fn pdfs_under(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, into);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        {
            into.push(path);
        }
    }
}

fn all_pdfs(root: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    pdfs_under(root, &mut all);
    all.sort();
    all
}

/// Whether the raw bytes contain `needle`, found without the reader.
fn bytes_name(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Opens a corpus file the way a caller reading annotations would.
///
/// Most encrypted documents in these corpora open on the empty user password,
/// and a signature, a font or an annotation living in an object stream is
/// invisible until the file key exists — so a census that skipped them would
/// report a smaller corpus rather than a different one.
fn open(bytes: Vec<u8>) -> Option<Document> {
    let document = Document::open(bytes).ok()?;
    if document.is_encrypted() {
        let _ = document.authenticate("");
    }
    Some(document)
}

/// Adds one to `key`'s tally.
fn tally(counts: &mut BTreeMap<String, usize>, key: &str) {
    *counts.entry(key.to_string()).or_default() += 1;
}

/// Prints a tally, largest first, ties by name.
fn print_tally(heading: &str, counts: &BTreeMap<String, usize>) {
    let mut rows: Vec<(&String, &usize)> = counts.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    println!("{heading} ({} distinct)", rows.len());
    for (name, count) in rows {
        println!("  {count:>8}  {name}");
    }
}

// ---- the annotation census ------------------------------------------------

/// How the refusals are labelled when the subtype gave no name to use.
const NO_SUBTYPE: &str = "(no /Subtype)";
const NOT_A_DICTIONARY: &str = "(not a dictionary)";

/// Every annotation on every page of every corpus file, counted by subtype,
/// with the refused ones counted by subtype beside them.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_annotations() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };

    // Found without the reader: a file whose bytes never say `/Annots` cannot
    // have a page that carries one, and skipping it keeps the census over the
    // files the question is about. (Bytes inside an object stream are
    // compressed, so this under-selects rather than over-selects — a file it
    // drops would have contributed nothing but zeros.)
    let mut candidates = Vec::new();
    for path in all_pdfs(&root) {
        if std::fs::read(&path).is_ok_and(|bytes| bytes_name(&bytes, b"/Annots")) {
            candidates.push(path);
        }
    }
    println!("RAN over {} files naming /Annots", candidates.len());
    assert!(
        candidates.len() >= 1_000,
        "the fetched corpora carried 1 012 when this was written; found {}",
        candidates.len()
    );

    let mut opened = 0usize;
    let mut unopenable = 0usize;
    let mut with_annotations = 0usize;
    let mut total = 0usize;
    let mut covered_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut refused_counts: BTreeMap<String, usize> = BTreeMap::new();
    // Where each refused subtype came from, so a count is attributable: the
    // number alone cannot say whether the same names are still the same names.
    let mut refused_from: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let mut markup = 0usize;
    let mut with_contents = 0usize;
    let mut with_title = 0usize;
    let mut with_modified = 0usize;
    let mut modified_parses = 0usize;
    let mut with_appearance = 0usize;
    let mut popups = 0usize;
    let mut popups_with_parent = 0usize;
    let mut popups_taking_parent_text = 0usize;
    let mut hidden = 0usize;
    let mut biggest_page = (0usize, String::new());

    // Self-consistency, named as such: `Page::links` walks the same `/Annots`
    // array through `cos::dest`, independently of this model. Every `/Link` one
    // finds the other should find. This adjudicates nothing about the
    // standard — both readers are ours.
    let mut link_disagreements: Vec<String> = Vec::new();

    for path in &candidates {
        let name = relative(&root, path);
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(document) = open(bytes) else {
            unopenable += 1;
            continue;
        };
        opened += 1;

        let mut in_this_file = 0usize;
        let mut links_here = 0usize;
        let mut link_annotations_here = 0usize;

        for page in document.pages() {
            let annotations = page.annotations();
            if annotations.len() > biggest_page.0 {
                biggest_page = (annotations.len(), name.clone());
            }
            links_here += page.links().len();
            in_this_file += annotations.len();

            for annotation in &annotations {
                total += 1;
                match &annotation.kind {
                    kind if kind.is_covered() => tally(&mut covered_counts, kind.as_name()),
                    AnnotationKind::Other(subtype) => {
                        tally(&mut refused_counts, subtype);
                        let from = refused_from.entry(subtype.clone()).or_default();
                        if from.len() < 3 && !from.contains(&name) {
                            from.push(name.clone());
                        }
                    }
                    AnnotationKind::Unnamed => {
                        tally(&mut refused_counts, NO_SUBTYPE);
                        let from = refused_from.entry(NO_SUBTYPE.to_string()).or_default();
                        if from.len() < 3 && !from.contains(&name) {
                            from.push(name.clone());
                        }
                    }
                    AnnotationKind::Unreadable => {
                        tally(&mut refused_counts, NOT_A_DICTIONARY);
                        let from = refused_from
                            .entry(NOT_A_DICTIONARY.to_string())
                            .or_default();
                        if from.len() < 3 && !from.contains(&name) {
                            from.push(name.clone());
                        }
                    }
                    // Covered is the first arm; nothing else reaches here.
                    _ => unreachable!("a covered kind is tallied above"),
                }

                if annotation.kind.is_markup() {
                    markup += 1;
                }
                if annotation.contents.is_some() {
                    with_contents += 1;
                }
                if annotation.title.is_some() {
                    with_title += 1;
                }
                if annotation.modified.is_some() {
                    with_modified += 1;
                    if annotation.modified_date.is_some() {
                        modified_parses += 1;
                    }
                }
                if annotation.has_appearance {
                    with_appearance += 1;
                }
                if annotation.flags.hidden() {
                    hidden += 1;
                }
                if annotation.kind == AnnotationKind::Link {
                    link_annotations_here += 1;
                }
                if annotation.kind == AnnotationKind::Popup {
                    popups += 1;
                    if annotation.parent.is_some() {
                        popups_with_parent += 1;
                        if annotation.contents.is_some() {
                            popups_taking_parent_text += 1;
                        }
                    }
                }
            }
        }

        if in_this_file > 0 {
            with_annotations += 1;
        }
        // `links()` resolves a destination and drops a `/Link` whose target
        // will not resolve, so it may find fewer. Finding *more* would mean
        // this model lost an entry of the array it walks.
        if links_here > link_annotations_here {
            link_disagreements.push(format!(
                "{name}: links() {links_here} > /Link annotations {link_annotations_here}"
            ));
        }
    }

    println!("opened {opened}, unopenable {unopenable}, with annotations {with_annotations}");
    println!("annotations read: {total}");
    println!(
        "largest page: {} annotations in {}",
        biggest_page.0, biggest_page.1
    );

    let covered_total: usize = covered_counts.values().sum();
    let refused_total: usize = refused_counts.values().sum();
    print_tally("covered subtypes", &covered_counts);
    print_tally("refused subtypes", &refused_counts);
    for (subtype, from) in &refused_from {
        println!("  {subtype} first seen in {}", from.join(", "));
    }
    println!(
        "covered {covered_total}, refused {refused_total}, \
         refused share {:.4}%",
        100.0 * refused_total as f64 / total.max(1) as f64
    );

    println!(
        "markup {markup}; /Contents {with_contents}, /T {with_title}, \
         /M {with_modified} of which {modified_parses} parse as 7.9.4 dates"
    );
    println!("with a normal appearance {with_appearance}; /F hidden {hidden}");
    println!(
        "pop-ups {popups}, of which {popups_with_parent} have a /Parent \
         and {popups_taking_parent_text} report text through it (12.5.6.14)"
    );

    // **The totality claim, on real files.** Nothing is dropped, so the two
    // tallies account for every annotation read.
    assert_eq!(
        covered_total + refused_total,
        total,
        "every annotation is counted either as covered or by its refused subtype"
    );

    // The corpus carried these when this was written. A floor rather than an
    // equality: files get added, and a census that failed on a larger corpus
    // would be a census nobody runs.
    assert!(
        total >= 14_000,
        "the corpus carried 14 996 annotations when this was written; found {total}"
    );
    // **All twenty-eight** modelled subtypes appear in the fetched corpora,
    // which is the fact that makes the covered half of the model measured
    // rather than merely transcribed. A floor, so adding files cannot fail it;
    // removing the only file carrying `/Projection` is meant to.
    assert!(
        covered_counts.len() >= 28,
        "every subtype the model covers appeared in the corpus when this was          written; found {} distinct: {:?}",
        covered_counts.len(),
        covered_counts.keys().collect::<Vec<_>>()
    );

    if !link_disagreements.is_empty() {
        for line in &link_disagreements {
            println!("  LINK DISAGREEMENT {line}");
        }
    }
    assert!(
        link_disagreements.is_empty(),
        "{} files where links() found a /Link this model did not",
        link_disagreements.len()
    );
}

// ---- the font census ------------------------------------------------------

/// Every font every corpus page can reach, counted by family, by embedding and
/// by `/FontFile*` key.
///
/// This is the half of the corpus that adjudicates `Document::fonts`: the
/// subset tags here were written by real producers, and 9.6.4's shape either
/// describes them or does not.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_fonts() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let files = all_pdfs(&root);
    println!("RAN over {} corpus files", files.len());
    assert!(
        files.len() >= 5_000,
        "the fetched corpora carried 5 605 files when this was written; found {}",
        files.len()
    );

    let mut opened = 0usize;
    let mut unopenable = 0usize;
    let mut with_fonts = 0usize;
    let mut total = 0usize;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    let mut embedded = 0usize;
    let mut tagged = 0usize;
    let mut nameless = 0usize;
    let mut inline_dictionaries = 0usize;
    let mut many_names = 0usize;
    let mut program_bytes = 0u64;
    let mut undecodable: Vec<String> = Vec::new();
    // Files whose raw bytes name a `/FontFile*` and whose listing reaches no
    // embedded program at all. Not an assertion: the bytes may be in an
    // unreferenced object, on a page outside the tree, or in a descriptor
    // nothing uses. Printed so the number is looked at rather than assumed.
    let mut claims_a_program_but_lists_none: Vec<String> = Vec::new();

    for path in &files {
        let name = relative(&root, path);
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let names_a_font_file = bytes_name(&bytes, b"/FontFile");
        let Some(document) = open(bytes) else {
            unopenable += 1;
            continue;
        };
        opened += 1;

        let fonts = document.fonts();
        if !fonts.is_empty() {
            with_fonts += 1;
        }
        let mut embedded_here = 0usize;

        for font in &fonts {
            total += 1;
            tally(
                &mut kinds,
                match font.kind {
                    FontKind::Type1 => "Type1",
                    FontKind::TrueType => "TrueType",
                    FontKind::Type3 => "Type3",
                    FontKind::Type0 => "Type0",
                },
            );
            if font.name.is_empty() {
                nameless += 1;
            }
            if font.reference.is_none() {
                inline_dictionaries += 1;
            }
            if font.resource_names.len() > 1 {
                many_names += 1;
            }
            if let Some(tag) = &font.subset_tag {
                tagged += 1;
                // 9.6.4, checked against what producers actually wrote: six
                // upper-case ASCII letters. A tag that failed this would mean
                // the splitter stripped something that is not a tag, and the
                // reported name would be a name no other tool uses.
                assert_eq!(tag.len(), 6, "{name}: subset tag {tag:?} is not six long");
                assert!(
                    tag.bytes().all(|b| b.is_ascii_uppercase()),
                    "{name}: subset tag {tag:?} is not upper-case letters"
                );
                assert!(
                    font.base_font.starts_with(tag.as_str()),
                    "{name}: {:?} does not start with its own tag",
                    font.base_font
                );
                assert!(
                    !font.name.contains('+'),
                    "{name}: the reported name {:?} still carries a tag",
                    font.name
                );
            }
            match font.program {
                Some(program) => {
                    embedded += 1;
                    embedded_here += 1;
                    tally(
                        &mut keys,
                        match program.key {
                            ProgramKey::FontFile => "FontFile",
                            ProgramKey::FontFile2 => "FontFile2",
                            ProgramKey::FontFile3 => "FontFile3",
                        },
                    );
                    assert!(font.is_embedded(), "{name}: a program but not embedded");
                    match font.program_bytes() {
                        Some(bytes) => program_bytes += bytes.len() as u64,
                        // Documented: a stream that will not decode and an
                        // absent one are both "no bytes" to a caller, and the
                        // reason is already on `Document::warnings`. Counted
                        // rather than asserted away.
                        None if undecodable.len() < 20 => {
                            undecodable.push(format!("{name}: {:?}", font.base_font));
                        }
                        None => {}
                    }
                }
                None => assert!(!font.is_embedded(), "{name}: embedded with no program",),
            }
        }

        if names_a_font_file && embedded_here == 0 && claims_a_program_but_lists_none.len() < 20 {
            claims_a_program_but_lists_none.push(name);
        }
    }

    println!("opened {opened}, unopenable {unopenable}, with fonts {with_fonts}");
    println!("distinct fonts listed: {total}");
    print_tally("by family (9.6/9.7)", &kinds);
    print_tally("by /FontFile* key (9.9 Table 126)", &keys);
    println!(
        "embedded {embedded} ({:.1}%), subset-tagged {tagged}, \
         no /BaseFont {nameless}, written inline {inline_dictionaries}, \
         reached under more than one resource name {many_names}",
        100.0 * embedded as f64 / total.max(1) as f64
    );
    println!(
        "program bytes decoded: {program_bytes} across {} embedded programs",
        embedded - undecodable.len()
    );
    for line in &undecodable {
        println!("  undecodable program {line}");
    }
    for line in &claims_a_program_but_lists_none {
        println!("  bytes name /FontFile, listing reaches none: {line}");
    }

    assert!(
        total >= 22_000,
        "the corpus listed 22 769 fonts when this was written; found {total}"
    );
    assert!(
        kinds.len() == 4,
        "all four of 9.6/9.7's families appear in the corpus; found {:?}",
        kinds.keys().collect::<Vec<_>>()
    );
}
