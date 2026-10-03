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
