//! Annex F's own numbering rule, applied to this writer's output.
//!
//! F.3.1 divides every indirect object in a linearized file into two groups.
//! The second — the remaining pages, the shared objects, everything no page
//! reaches — is *numbered sequentially starting at 1*. The first — the
//! catalogue, the document-level objects and the first page's — is numbered
//! after it. Table F.4 item 1 states the consequence from the reader's side:
//! the first object of the second page has object number 1.
//!
//! That is not decoration. F.4.1's per-page entry hands a reader a *count* and
//! nothing else, and the reader takes that many consecutive object numbers
//! beginning with the page's own page object. So a reader can derive every
//! page's first object number from `/O` and the counts alone, and this file is
//! that derivation: `annex_f_first_objects` is the rule, and
//! `names_the_page` asks the document whether the number it produced is the
//! page it claims to be.
//!
//! # Where each half lives
//!
//! The counts come from Table F.4's page-offset hint table, and the decoder
//! for it is `crate::validate::hints`, which is `pub(crate)` — one reader for
//! the format, not two (`hints.rs`'s own header says why). Widening it for a
//! test's convenience would put Annex F's bit-table internals on the public
//! surface of a crate that routes through the facade (ruling 11), so it stays
//! shut: the sweep that needs decoded counts is a unit test at the bottom of
//! that module, and what lives here is the half the public API answers.
//!
//! The two halves are not circular. This file derives each page's run from the
//! *gaps* between the page objects the document walk finds, and then asserts
//! that the strict validator reports no `hint-value-wrong` for a page's object
//! count — which is the public statement that the counts the file *declares*
//! are those gaps. The validator ties the declared counts to the file's own
//! object extents; this ties the gaps to the pages.

use std::sync::Arc;

use tinker_pdf_cos::{
    pages, CosDocument, Defect, DefectKind, DocumentBuilder, DocumentEditor, Encryption, ObjRef,
    Object, WriteMode, WriteOptions,
};

fn document(page_count: usize) -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.set_info(b"Title", "linearized");
    for index in 0..page_count {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

/// The same forty-eight deterministic bytes `linearized.rs` uses, so the
/// encrypted fixtures are the same documents run after run.
fn entropy() -> [u8; 48] {
    let mut bytes = [0u8; 48];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    bytes
}

fn save(page_count: usize, encryption: Option<Encryption>) -> Vec<u8> {
    DocumentEditor::new(document(page_count)).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        encryption,
        ..WriteOptions::default()
    })
}

fn linearized(page_count: usize) -> Vec<u8> {
    save(page_count, None)
}

fn encrypted_linearized(page_count: usize) -> Vec<u8> {
    save(
        page_count,
        Some(Encryption {
            user_password: "open-me".to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy: entropy(),
        }),
    )
}

/// One integer out of the linearization parameter dictionary.
fn parameter(bytes: &[u8], key: &str) -> u64 {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).into_owned();
    let at = text
        .find(&format!("/{key} "))
        .unwrap_or_else(|| panic!("no /{key} in the parameter dictionary: {text}"));
    text[at + key.len() + 2..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("/{key} is not a number"))
}

/// **Annex F's rule, and nothing else.**
///
/// - `first[0]` is `/O` (F.3.3, Table F.1 item 3).
/// - `first[1]` is 1 (Table F.4 item 1, F.3.1's second group).
/// - `first[k]` is `first[k - 1] + objects_in_page(k - 1)` (Table F.4 item 1's
///   per-page count, which is what a reader accumulates).
///
/// A one-page document makes the rule vacuous rather than false: there is no
/// second page for item 1 to speak about.
fn annex_f_first_objects(first_page_object: u32, counts: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(counts.len());
    for index in 0..counts.len() {
        let number = match index {
            0 => first_page_object,
            1 => 1,
            _ => out[index - 1] + counts[index - 1],
        };
        out.push(number);
    }
    out
}

/// Whether the object `number` names is a page, and is the page the document
/// walk puts at `index`.
///
/// Both halves matter. A number that lands on some other page object is still
/// a `/Type /Page`, and a derivation that produced it would pass a test that
/// only asked what kind of object it was.
fn names_the_page(doc: &CosDocument, number: u32, index: usize) {
    let object = doc
        .get(ObjRef::new(number, 0))
        .unwrap_or_else(|_| panic!("object {number} is in the file"));
    let kind = match object.as_ref() {
        Object::Dict(dict) => dict.get_name(tinker_pdf_cos::Name::TYPE),
        _ => None,
    };
    assert_eq!(
        kind,
        Some(doc.intern(b"Page")),
        "Annex F's arithmetic names object {number} for page {index}, which is not a page"
    );
    assert_eq!(
        pages::collect(doc)
            .get(index)
            .map(|page| page.reference.num),
        Some(number),
        "object {number} is a page, but not page {index}"
    );
}

/// Each page's run, taken as the gap to the next page's first object.
///
/// Page one's run is the head group's last, so it reaches the top of the
/// numbering — which is the hint stream, F.3.6 having given that *the last
/// object number in the file*. Every later page's run ends where the next
/// begins, and the last of them ends where the head group does.
fn runs_from_the_gaps(doc: &CosDocument, bytes: &[u8]) -> Vec<u32> {
    let numbers: Vec<u32> = pages::collect(doc)
        .iter()
        .map(|page| page.reference.num)
        .collect();
    let top = doc.max_object_number();
    let head_first = head_group_first(bytes);

    numbers
        .iter()
        .enumerate()
        .map(|(index, number)| {
            let end = match index {
                0 => top,
                _ => numbers.get(index + 1).copied().unwrap_or(head_first),
            };
            end - number
        })
        .collect()
}

/// The first object number the front cross-reference section covers, read off
/// its own subsection header (7.5.4). That is where the head group starts, so
/// it is where the last of the remaining pages' runs stops.
fn head_group_first(bytes: &[u8]) -> u32 {
    let tail_at = bytes.len().saturating_sub(64);
    let tail = String::from_utf8_lossy(&bytes[tail_at..]).into_owned();
    let at = tail.rfind("startxref\n").expect("a startxref");
    let section: usize = tail[at + 10..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("an offset");
    let head = bytes.get(section..).unwrap_or_default();
    assert!(head.starts_with(b"xref\n"), "a section at startxref");
    let line: String = head[5..]
        .iter()
        .copied()
        .take_while(|b| *b != b'\n')
        .map(char::from)
        .collect();
    line.split(' ')
        .next()
        .and_then(|t| t.parse().ok())
        .expect("a subsection header")
}

/// The strict validator's word that the counts the file *declares* are the
/// gaps this test measured.
fn the_declared_counts_agree(doc: &CosDocument) {
    let complaints: Vec<String> = tinker_pdf_cos::validate(doc)
        .iter()
        .filter(|defect| {
            matches!(
                defect.kind,
                DefectKind::HintValueWrong {
                    entry: "page object count"
                        | "last page's object count"
                        | "page's own objects, which no table carries"
                }
            )
        })
        .map(Defect::to_string)
        .collect();
    assert!(
        complaints.is_empty(),
        "the counts the file declares are not the runs this test measured: {complaints:?}"
    );
}

/// Table F.4 item 1, verbatim: *the first object of the second page shall have
/// an object number of 1*.
///
/// Over one, two, three and six pages, plain and encrypted, because the
/// encrypted layout carries one object the plain one does not — 7.6.1's
/// `/Encrypt` dictionary — and that object is in the head group, so a writer
/// that let it shift the tail group would fail here and nowhere else.
#[test]
fn the_second_pages_first_object_is_object_one() {
    for count in [1usize, 2, 3, 6] {
        for (case, bytes) in [
            ("plain", linearized(count)),
            ("encrypted", encrypted_linearized(count)),
        ] {
            let doc = CosDocument::open(bytes.clone()).expect("it opens");
            if doc.is_encrypted() {
                assert!(doc.authenticate("open-me").is_ok(), "the fixture opens");
            }
            let collected = pages::collect(&doc);
            assert_eq!(collected.len(), count, "{count} pages ({case})");

            let numbers: Vec<u32> = collected.iter().map(|page| page.reference.num).collect();
            assert_eq!(
                u64::from(numbers[0]),
                parameter(&bytes, "O"),
                "/O leads the first page's run ({count} pages, {case})"
            );

            if count == 1 {
                // The rule is vacuous, not violated: there is no second page.
                // Asserted so that a one-page file cannot be read as evidence
                // either way, which is how the defect this test exists for
                // stayed invisible.
                assert_eq!(numbers.len(), 1);
                continue;
            }
            assert_eq!(
                numbers[1], 1,
                "the second page's first object is object 1 ({count} pages, {case}): {numbers:?}"
            );
            names_the_page(&doc, 1, 1);
        }
    }
}

/// The whole chain, over the writer's output: `/O`, then 1, then each page's
/// run added to the one before, lands on every page in turn.
#[test]
fn every_page_is_found_by_annex_fs_own_arithmetic() {
    for count in [2usize, 3, 6] {
        for (case, bytes) in [
            ("plain", linearized(count)),
            ("encrypted", encrypted_linearized(count)),
        ] {
            let doc = CosDocument::open(bytes.clone()).expect("it opens");
            if doc.is_encrypted() {
                assert!(doc.authenticate("open-me").is_ok(), "the fixture opens");
            }
            let counts = runs_from_the_gaps(&doc, &bytes);
            assert_eq!(counts.len(), count, "a run for every page ({case})");
            assert!(
                counts.iter().all(|run| *run > 0),
                "every page owns at least its page object ({case}): {counts:?}"
            );

            let firsts = annex_f_first_objects(parameter(&bytes, "O") as u32, &counts);
            for (index, number) in firsts.iter().enumerate() {
                names_the_page(&doc, *number, index);
            }

            // And the runs this derivation used are the runs the file declares
            // (see this file's header for why that is not circular).
            the_declared_counts_agree(&doc);
        }
    }
}
