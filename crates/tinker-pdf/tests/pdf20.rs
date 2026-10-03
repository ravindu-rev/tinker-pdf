//! ISO 32000-2's additions this engine reads and writes beyond the structure
//! namespaces (`tagged_writer.rs`, `tagged_pdf.rs`): page-level output
//! intents, and associated files.
//!
//! The 2.0 text is not readable here (`docs/pdf20-deltas.md` says why), so
//! each test is held to what a reachable source states — the PDF Association's
//! approved errata, the Arlington model, its `pdf20examples` — and each
//! fixture written by hand is the shape such a source shows.

mod pdfa_support;

use pdfa_support::{cmyk_like, srgb_like};
use tinker_pdf::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, Document, DocumentBuilder, NewOutputIntent,
    ObjRef, OutputIntent, Tier,
};
use tinker_pdf_cos::DeviceSpace;

/// The strict structural validator finds nothing (ruling 13's third axis).
fn structurally_clean(doc: &Document) {
    let defects: Vec<_> = doc
        .validate()
        .into_iter()
        .filter(|defect| defect.kind.tier() == Tier::Structure)
        .collect();
    assert!(defects.is_empty(), "{defects:?}");
}

// ---- page-level output intents --------------------------------------------

/// An `eciRGB`-style intent, as the PDF Association's example writes its
/// page-level one.
fn eci(profile: Vec<u8>) -> NewOutputIntent {
    NewOutputIntent::new(b"GTS_PDFX", "eciRGB")
        .registry("http://www.color.org")
        .info("European Color Initiative RGB")
        .condition("eciRGB v2")
        .profile(profile, DeviceSpace::Rgb)
}

/// Written on the pages that ask, read back from each page as written, with
/// the catalog's left as it was — empty, in a document with no archival
/// profile — and one profile stream shared by every page that names it.
#[test]
fn a_page_level_output_intent_is_written_and_read_back() {
    let mut builder = DocumentBuilder::with_version(2, 0);
    builder.add_base_font(b"F1", b"Helvetica");
    let profile = srgb_like();
    builder.add_page(200.0, 200.0, |page| {
        assert!(page.output_intent(eci(profile.clone())));
        assert!(page.output_intent(
            NewOutputIntent::new(b"ISO_PDFE1", "Custom").profile(cmyk_like(), DeviceSpace::Cmyk)
        ));
        page.text(b"F1", 12.0, 20.0, 150.0, "first");
    });
    builder.add_page(200.0, 200.0, |page| {
        assert!(page.output_intent(eci(profile.clone())));
    });
    builder.add_page(200.0, 200.0, |page| {
        page.text(b"F1", 12.0, 20.0, 150.0, "the catalog's");
    });
    let doc = Document::open(builder.finish()).expect("opens");
    structurally_clean(&doc);

    assert!(doc.output_intents().is_empty(), "the catalog states none");
    let first = doc.page(0).expect("page 1").output_intents();
    assert_eq!(first.len(), 2, "both, in the order given");
    let eci_read = &first[0];
    assert_eq!(eci_read.subtype.as_deref(), Some("GTS_PDFX"));
    assert_eq!(
        eci_read.output_condition_identifier.as_deref(),
        Some("eciRGB")
    );
    assert_eq!(
        eci_read.registry_name.as_deref(),
        Some("http://www.color.org")
    );
    assert_eq!(
        eci_read.info.as_deref(),
        Some("European Color Initiative RGB")
    );
    assert_eq!(eci_read.output_condition.as_deref(), Some("eciRGB v2"));
    assert_eq!(eci_read.components, Some(3));
    let profile_ref = eci_read.destination_profile.expect("a profile stream");
    assert_eq!(
        doc.cos().stream_decoded(profile_ref).expect("decodes"),
        profile,
        "the profile's bytes, as given"
    );
    assert_eq!(first[1].subtype.as_deref(), Some("ISO_PDFE1"));
    assert_eq!(first[1].components, Some(4));
    assert_ne!(first[1].destination_profile, Some(profile_ref));

    let second = doc.page(1).expect("page 2").output_intents();
    assert_eq!(second.len(), 1);
    assert_eq!(
        second[0].destination_profile,
        Some(profile_ref),
        "one stream for one profile, however many pages name it"
    );
    assert!(doc.page(2).expect("page 3").output_intents().is_empty());
}

/// What `PageBuilder::output_intent` refuses, each for the reason its
/// documentation gives.
#[test]
fn a_page_level_output_intent_is_refused_where_it_cannot_be_written() {
    // Before 2.0 a page has no such entry.
    let mut old = DocumentBuilder::new();
    old.add_page(100.0, 100.0, |page| {
        assert!(!page.output_intent(eci(srgb_like())));
    });
    let doc = Document::open(old.finish()).expect("opens");
    assert!(doc.page(0).expect("a page").output_intents().is_empty());

    // An archival profile writes the catalog's intent and judges every
    // device colour against it — part 4 is on 2.0 and is refused all the same.
    let mut archival = DocumentBuilder::archival(ArchivalProfile {
        part: ArchivalPart::Four,
        level: Some(ArchivalLevel::F),
        destination_profile: srgb_like(),
        destination_space: DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    });
    archival.add_page(100.0, 100.0, |page| {
        assert!(!page.output_intent(eci(srgb_like())));
    });

    let mut builder = DocumentBuilder::with_version(2, 0);
    builder.add_page(100.0, 100.0, |page| {
        assert!(
            !page.output_intent(NewOutputIntent::new(b"", "Custom")),
            "no /S"
        );
        assert!(
            !page.output_intent(NewOutputIntent::new(b"GTS_PDFX", "")),
            "no /OutputConditionIdentifier"
        );
        assert!(
            !page.output_intent(eci(Vec::new())),
            "a profile with no bytes"
        );
        assert!(
            page.output_intent(NewOutputIntent::new(b"GTS_PDFX", "Custom")),
            "no profile is fine"
        );
    });
    let doc = Document::open(builder.finish()).expect("opens");
    let read = doc.page(0).expect("a page").output_intents();
    assert_eq!(read.len(), 1, "only the one accepted");
    assert_eq!(read[0].destination_profile, None);
    assert_eq!(read[0].components, None);
}

/// The reader on a file shaped like the PDF Association's PDF 2.0 example:
/// a catalog intent, a page overriding it with its own, a page with none —
/// and, beside the example's shape, what a reader has to tolerate: an entry
/// that is not a dictionary, an intent with no `/S`, and an intent on a
/// `/Pages` node, which the Arlington model does not make inheritable.
#[test]
fn page_level_intents_are_read_from_the_page_alone_beside_the_catalogs() {
    let profile = "0123456789";
    let bytes = format!(
        "%PDF-2.0\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R\n\
   /OutputIntents [ << /Type /OutputIntent /S /GTS_PDFX /DestOutputProfile 9 0 R\n\
      /Info (Adobe RGB \\(1998\\)) /OutputConditionIdentifier (Adobe RGB \\(1998\\))\n\
      /RegistryName (http://www.color.org) >> ] >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2\n\
   /OutputIntents [ << /S /GTS_PDFX /OutputConditionIdentifier (inherited?) >> ] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 396]\n\
   /OutputIntents [ << /Type /OutputIntent /S /GTS_PDFX /DestOutputProfile 10 0 R\n\
      /Info (European Color Initiative RGB) /OutputConditionIdentifier (eciRGB)\n\
      /RegistryName (http://www.color.org) >>\n\
      42\n\
      << /OutputConditionIdentifier (no subtype) /DestOutputProfile << /N 3 >> >> ] >>\nendobj\n\
4 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 396] >>\nendobj\n\
9 0 obj\n<< /N 3 /Length {len} >>\nstream\n{profile}\nendstream\nendobj\n\
10 0 obj\n<< /N 3 /Length {len} >>\nstream\n{profile}\nendstream\nendobj\n\
trailer\n<< /Size 11 /Root 1 0 R >>\n%%EOF\n",
        len = profile.len()
    );
    let doc = Document::open(bytes.into_bytes()).expect("opens");

    let catalog = doc.output_intents();
    assert_eq!(catalog.len(), 1);
    assert_eq!(
        catalog[0].output_condition_identifier.as_deref(),
        Some("Adobe RGB (1998)")
    );
    assert_eq!(catalog[0].destination_profile, Some(ObjRef::new(9, 0)));

    let first = doc.page(0).expect("page 1").output_intents();
    let expected_first = OutputIntentView {
        subtype: Some("GTS_PDFX"),
        identifier: Some("eciRGB"),
        profile: Some(ObjRef::new(10, 0)),
        components: Some(3),
    };
    assert_eq!(first.len(), 2, "the integer is skipped, the rest read");
    assert_eq!(OutputIntentView::of(&first[0]), expected_first);
    assert_eq!(
        OutputIntentView::of(&first[1]),
        OutputIntentView {
            subtype: None,
            identifier: Some("no subtype"),
            profile: None,
            components: None,
        },
        "no /S is said, not defaulted; a direct profile names no stream"
    );
    assert!(
        doc.page(1).expect("page 2").output_intents().is_empty(),
        "nothing inherited from /Pages, and the catalog's is not copied in"
    );
}

/// The fields a test compares, borrowed.
#[derive(Debug, PartialEq)]
struct OutputIntentView<'a> {
    subtype: Option<&'a str>,
    identifier: Option<&'a str>,
    profile: Option<ObjRef>,
    components: Option<u32>,
}

impl<'a> OutputIntentView<'a> {
    fn of(intent: &'a OutputIntent) -> Self {
        OutputIntentView {
            subtype: intent.subtype.as_deref(),
            identifier: intent.output_condition_identifier.as_deref(),
            profile: intent.destination_profile,
            components: intent.components,
        }
    }
}

/// The PDF/A writer's catalog intent reads back through the same reader, and
/// the archival handling around it is untouched: no page of an archival
/// document has an intent of its own.
#[test]
fn an_archival_documents_catalog_intent_reads_back() {
    let mut builder = DocumentBuilder::archival(ArchivalProfile {
        part: ArchivalPart::Two,
        level: Some(ArchivalLevel::B),
        destination_profile: srgb_like(),
        destination_space: DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    });
    builder.add_page(100.0, 100.0, |page| page.fill_rect(1.0, 1.0, 2.0, 2.0, 0.0));
    let doc = Document::open(builder.finish_archival().expect("written")).expect("opens");
    let intents = doc.output_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].subtype.as_deref(), Some("GTS_PDFA1"));
    assert_eq!(
        intents[0].output_condition_identifier.as_deref(),
        Some("Custom")
    );
    assert_eq!(intents[0].components, Some(3));
    assert!(doc.page(0).expect("a page").output_intents().is_empty());
}

// ---- associated files ------------------------------------------------------

use tinker_pdf::{
    ArchivalRefusal, AssociatedFile, FileRelationship, NewAssociatedFile, StructElement, Tag,
};

/// An RGB part `part` profile, as `pdfa_writer.rs` builds one.
fn archival(part: ArchivalPart, level: Option<ArchivalLevel>) -> ArchivalProfile {
    ArchivalProfile {
        part,
        level,
        destination_profile: srgb_like(),
        destination_space: DeviceSpace::Rgb,
        output_condition: "Custom".to_string(),
        language: None,
    }
}

/// Every element in the tree, depth-first.
fn elements(doc: &Document) -> Vec<StructElement> {
    doc.structure()
        .expect("a tree")
        .elements()
        .into_iter()
        .cloned()
        .collect()
}

/// The fields a test compares, borrowed.
#[derive(Debug, PartialEq)]
struct FileView<'a> {
    filename: Option<&'a str>,
    description: Option<&'a str>,
    relationship: Option<FileRelationship>,
    relationship_name: Option<&'a str>,
    mime_type: Option<&'a str>,
    size: Option<i64>,
}

impl<'a> FileView<'a> {
    fn of(file: &'a AssociatedFile) -> Self {
        FileView {
            filename: file.filename.as_deref(),
            description: file.description.as_deref(),
            relationship: file.relationship,
            relationship_name: file.relationship_name.as_deref(),
            mime_type: file.mime_type.as_deref(),
            size: file.size,
        }
    }
}

/// Written on the catalog, a page and a structure element, and read back from
/// each as given: the filename (a non-ASCII one through `/UF`), the
/// description, the relationship, the MIME type and the bytes — and an
/// element holding nothing but an associated file is kept, since the file is
/// something it says.
#[test]
fn associated_files_are_written_on_the_catalog_a_page_and_an_element_and_read_back() {
    let csv = b"a,b\n1,2\n".to_vec();
    let mut builder = DocumentBuilder::with_version(2, 0);
    builder.add_base_font(b"F1", b"Helvetica");
    assert!(builder.associate_file(
        NewAssociatedFile::new(
            "donn\u{e9}es.csv",
            "text/csv",
            FileRelationship::Data,
            csv.clone()
        )
        .description("The table's data")
    ));
    builder.add_page(200.0, 200.0, |page| {
        assert!(page.associate_file(NewAssociatedFile::new(
            "page.svg",
            "image/svg+xml",
            FileRelationship::Source,
            b"<svg/>".to_vec(),
        )));
        let table = Tag::new(b"Table").associated_file(NewAssociatedFile::new(
            "table.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            FileRelationship::Alternative,
            b"PK".to_vec(),
        ));
        page.tagged_with(&table, |page| {
            page.tagged(b"P", |page| page.text(b"F1", 12.0, 20.0, 150.0, "1 2"));
        });
        let formula = Tag::new(b"Formula").associated_file(NewAssociatedFile::new(
            "formula.mml",
            "application/mathml+xml",
            FileRelationship::Source,
            b"<math/>".to_vec(),
        ));
        page.tagged_with(&formula, |_| {});
    });
    let doc = Document::open(builder.finish()).expect("opens");
    structurally_clean(&doc);

    let catalog = doc.associated_files();
    assert_eq!(catalog.len(), 1);
    assert_eq!(
        FileView::of(&catalog[0]),
        FileView {
            filename: Some("donn\u{e9}es.csv"),
            description: Some("The table's data"),
            relationship: Some(FileRelationship::Data),
            relationship_name: Some("Data"),
            mime_type: Some("text/csv"),
            size: Some(csv.len() as i64),
        }
    );
    let stream = catalog[0].stream.expect("embedded");
    assert_eq!(doc.cos().stream_decoded(stream).expect("decodes"), csv);
    assert!(
        catalog[0].specification.is_some(),
        "an indirect specification"
    );

    let page = doc.page(0).expect("a page").associated_files();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].filename.as_deref(), Some("page.svg"));
    assert_eq!(page[0].relationship, Some(FileRelationship::Source));
    assert_eq!(page[0].mime_type.as_deref(), Some("image/svg+xml"));

    let tree = elements(&doc);
    let table = tree
        .iter()
        .find(|element| element.standard_type == "Table")
        .expect("the table");
    assert_eq!(table.associated_files.len(), 1);
    assert_eq!(
        table.associated_files[0].relationship,
        Some(FileRelationship::Alternative)
    );
    let formula = tree
        .iter()
        .find(|element| element.standard_type == "Formula")
        .expect("an element holding only a file is kept");
    assert_eq!(
        formula.associated_files[0].filename.as_deref(),
        Some("formula.mml")
    );
    assert!(
        tree.iter()
            .filter(|element| element.standard_type == "P")
            .all(|element| element.associated_files.is_empty()),
        "a file is the element's own, not its kids'"
    );
}

/// Each relationship is written as the name it reads back as.
#[test]
fn every_relationship_reads_back_as_itself() {
    for relationship in FileRelationship::ALL {
        assert_eq!(
            FileRelationship::from_name(relationship.name()),
            Some(relationship)
        );
    }
    assert_eq!(FileRelationship::from_name(b"C2PA_Manifest"), None);
}

/// What the writer refuses: a file it cannot write well, and a document that
/// may not carry one.
#[test]
fn an_associated_file_is_refused_where_it_cannot_be_written() {
    let file = |name: &str, mime: &str, relationship| {
        NewAssociatedFile::new(name, mime, relationship, b"x".to_vec())
    };
    for (bad, why) in [
        (file("", "text/csv", FileRelationship::Data), "no filename"),
        (file("a", "text", FileRelationship::Data), "no subtype"),
        (file("a", "/csv", FileRelationship::Data), "no type"),
        (
            file("a", "text/csv; charset=utf-8", FileRelationship::Data),
            "parameters",
        ),
        (
            file("a", "text/c#v", FileRelationship::Data),
            "a number sign",
        ),
        (
            file("a", "text/csv/x", FileRelationship::Data),
            "two solidi",
        ),
        (
            file("a", "text/csv", FileRelationship::EncryptedPayload),
            "an encrypted payload needs /EP",
        ),
    ] {
        assert!(!bad.is_writable(), "{why}");
        let mut builder = DocumentBuilder::with_version(2, 0);
        assert!(!builder.associate_file(bad), "{why}");
    }
    assert!(file(
        "a.bin",
        "application/octet-stream",
        FileRelationship::Unspecified
    )
    .is_writable());

    // Before 2.0, with no ISO 19005-3 profile: nowhere, and an element's file
    // is dropped where its element is still written.
    let mut old = DocumentBuilder::new();
    assert!(!old.associate_file(file("a.csv", "text/csv", FileRelationship::Data)));
    old.add_base_font(b"F1", b"Helvetica");
    old.add_page(100.0, 100.0, |page| {
        assert!(!page.associate_file(file("a.csv", "text/csv", FileRelationship::Data)));
        let tag = Tag::new(b"P").associated_file(file("a.csv", "text/csv", FileRelationship::Data));
        page.tagged_with(&tag, |page| page.text(b"F1", 12.0, 20.0, 50.0, "x"));
    });
    assert!(
        old.refusals().is_empty(),
        "no profile, so nothing to refuse under"
    );
    let bytes = old.finish();
    assert!(!bytes.windows(14).any(|window| window == b"AFRelationship"));
    let doc = Document::open(bytes).expect("opens");
    assert_eq!(
        elements(&doc).len(),
        2,
        "the /P is written, without its file"
    );

    // Under part 2, each refusal is recorded.
    let mut part2 = DocumentBuilder::archival(archival(ArchivalPart::Two, Some(ArchivalLevel::B)));
    assert!(!part2.associate_file(file("a.csv", "text/csv", FileRelationship::Data)));
    part2.add_page(100.0, 100.0, |page| {
        assert!(!page.associate_file(file("a.csv", "text/csv", FileRelationship::Data)));
        let tag =
            Tag::new(b"Figure").associated_file(file("a.csv", "text/csv", FileRelationship::Data));
        page.tagged_with(&tag, |page| page.fill_rect(1.0, 1.0, 2.0, 2.0, 0.0));
    });
    assert_eq!(
        part2.refusals(),
        &[
            ArchivalRefusal::AssociatedFile,
            ArchivalRefusal::AssociatedFile,
            ArchivalRefusal::AssociatedFile
        ]
    );
}

/// ISO 19005-3 carried associated files on 1.7 before 2.0 did, and a part 3
/// document carrying one validates with no finding — the repository's own
/// part 3 rules ask for `/AFRelationship`, and it is there.
#[test]
fn a_part_3_document_carries_an_associated_file_and_validates() {
    let mut builder =
        DocumentBuilder::archival(archival(ArchivalPart::Three, Some(ArchivalLevel::B)));
    assert!(builder.associate_file(NewAssociatedFile::new(
        "invoice.xml",
        "text/xml",
        FileRelationship::Alternative,
        b"<invoice/>".to_vec(),
    )));
    builder.add_page(100.0, 100.0, |page| {
        assert!(page.set_fill_rgb(1.0, 0.0, 0.0));
        page.fill_rect(10.0, 10.0, 50.0, 50.0, 0.5);
    });
    assert!(builder.refusals().is_empty(), "{:?}", builder.refusals());
    let doc = Document::open(builder.finish_archival().expect("written")).expect("opens");
    assert_eq!(doc.pdf_version(), "PDF 1.7", "part 3 is on 1.7");
    assert_eq!(doc.associated_files().len(), 1);
    let verdict = doc.validate_pdfa();
    assert!(verdict.coverage.is_complete());
    assert!(verdict.findings.is_empty(), "{:?}", verdict.findings);
}

/// The reader on what another producer may write: an entry that is not a
/// dictionary, a specification with no `/AFRelationship`, a relationship an
/// extension defines, a file outside the document, and an `/AF` on a `/Pages`
/// node — which the Arlington model does not make inheritable.
#[test]
fn associated_files_another_producer_wrote_are_read_as_written() {
    let bytes = "%PDF-2.0\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R\n\
   /AF [ 10 0 R 7\n\
         << /Type /Filespec /F (bare.txt) /EF << /F 12 0 R >> >>\n\
         << /Type /Filespec /UF (c2pa.json) /AFRelationship /C2PA_Manifest >> ] >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /AF [10 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>\nendobj\n\
10 0 obj\n<< /Type /Filespec /F (http://example.com/x.csv) /FS /URL\n\
   /AFRelationship /Supplement >>\nendobj\n\
12 0 obj\n<< /Type /EmbeddedFile /Length 2 >>\nstream\nhi\nendstream\nendobj\n\
trailer\n<< /Size 13 /Root 1 0 R >>\n%%EOF\n";
    let doc = Document::open(bytes.as_bytes().to_vec()).expect("opens");
    let files = doc.associated_files();
    assert_eq!(files.len(), 3, "the integer is skipped");

    assert_eq!(files[0].specification, Some(ObjRef::new(10, 0)));
    assert_eq!(files[0].relationship, Some(FileRelationship::Supplement));
    assert_eq!(files[0].stream, None, "a file outside the document");

    assert_eq!(
        FileView::of(&files[1]),
        FileView {
            filename: Some("bare.txt"),
            description: None,
            relationship: None,
            relationship_name: None,
            mime_type: None,
            size: None,
        },
        "nothing defaulted: no relationship, no /Subtype, no /Params"
    );
    assert_eq!(files[1].stream, Some(ObjRef::new(12, 0)));
    assert_eq!(files[1].specification, None, "a direct specification");

    assert_eq!(files[2].relationship, None);
    assert_eq!(files[2].relationship_name.as_deref(), Some("C2PA_Manifest"));

    assert!(
        doc.page(0).expect("a page").associated_files().is_empty(),
        "nothing inherited from /Pages"
    );
}

/// An element opened on one page and closed on the next is one element, and
/// its file is written once rather than once per half.
#[test]
fn an_element_carried_over_a_page_break_holds_its_file_once() {
    let mut builder = DocumentBuilder::with_version(2, 0);
    builder.add_base_font(b"F1", b"Helvetica");
    let tag = Tag::new(b"P").associated_file(NewAssociatedFile::new(
        "p.txt",
        "text/plain",
        FileRelationship::Source,
        b"p".to_vec(),
    ));
    let mut first = builder.begin_page(100.0, 100.0);
    assert!(first.open_tag(&tag));
    first.text(b"F1", 12.0, 20.0, 50.0, "first half");
    builder.push_page(first);
    let mut second = builder.begin_page(100.0, 100.0);
    second.text(b"F1", 12.0, 20.0, 50.0, "second half");
    assert!(second.close_tag());
    builder.push_page(second);
    let doc = Document::open(builder.finish()).expect("opens");
    let paragraphs: Vec<StructElement> = elements(&doc)
        .into_iter()
        .filter(|element| element.standard_type == "P")
        .collect();
    assert_eq!(paragraphs.len(), 1, "one element");
    assert_eq!(paragraphs[0].associated_files.len(), 1, "one file");
}

/// Two halves of one keyed element that state different files: the first
/// half's statement stands, as for every other property of a merged element.
#[test]
fn keyed_halves_keep_the_first_halfs_files() {
    let mut builder = DocumentBuilder::with_version(2, 0);
    builder.add_base_font(b"F1", b"Helvetica");
    let half = |name: &str| {
        Tag::new(b"P")
            .keyed(7, 0)
            .associated_file(NewAssociatedFile::new(
                name,
                "text/plain",
                FileRelationship::Source,
                name.as_bytes().to_vec(),
            ))
    };
    builder.add_page(100.0, 100.0, |page| {
        page.tagged_with(&half("first.txt"), |page| {
            page.text(b"F1", 12.0, 20.0, 50.0, "first half");
        });
    });
    builder.add_page(100.0, 100.0, |page| {
        page.tagged_with(&half("second.txt"), |page| {
            page.text(b"F1", 12.0, 20.0, 50.0, "second half");
        });
    });
    let doc = Document::open(builder.finish()).expect("opens");
    let paragraphs: Vec<StructElement> = elements(&doc)
        .into_iter()
        .filter(|element| element.standard_type == "P")
        .collect();
    assert_eq!(paragraphs.len(), 1, "one element");
    let names: Vec<Option<&str>> = paragraphs[0]
        .associated_files
        .iter()
        .map(|file| file.filename.as_deref())
        .collect();
    assert_eq!(names, [Some("first.txt")]);
}
