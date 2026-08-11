//! Attachments, XMP and `/Limits` descent (phase 04).
//!
//! A tree exists so a lookup does not have to read all of it; attachments are
//! content a document plainly carries; XMP holds the identity and rights that
//! `/Info` does not. None of the three was reachable.

use tinker_pdf_cos::{attachments, name_tree_lookup, xmp_metadata, CosDocument, ObjRef};

/// A document whose `/Dests` name tree has two levels and real `/Limits`, one
/// attachment, and an XMP stream.
fn document() -> CosDocument {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Metadata 20 0 R\n\
   /Names << /Dests 10 0 R /EmbeddedFiles 30 0 R >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>\nendobj\n\
10 0 obj\n<< /Kids [11 0 R 12 0 R] >>\nendobj\n\
11 0 obj\n<< /Limits [(alpha) (delta) ]\n\
   /Names [(alpha) [3 0 R /Fit] (delta) [3 0 R /Fit]] >>\nendobj\n\
12 0 obj\n<< /Limits [(mike) (zulu) ]\n\
   /Names [(mike) [3 0 R /Fit] (zulu) [3 0 R /Fit]] >>\nendobj\n\
20 0 obj\n<< /Type /Metadata /Subtype /XML /Length 44 >>\nstream\n\
<?xpacket?><rdf:RDF>identity here</rdf:RDF>\n\
endstream\nendobj\n\
30 0 obj\n<< /Names [(report.csv) 31 0 R] >>\nendobj\n\
31 0 obj\n<< /Type /Filespec /F (report.csv) /UF (report.csv)\n\
   /Desc (the numbers behind the report) /EF << /F 32 0 R >> >>\nendobj\n\
32 0 obj\n<< /Type /EmbeddedFile /Params << /Size 9 >> /Length 9 >>\nstream\n\
a,b,c\n1,2\n\
endstream\nendobj\n\
trailer\n<< /Size 33 /Root 1 0 R >>\n%%EOF\n";
    CosDocument::open(bytes).expect("it opens")
}

/// The lookup descends into the subtree whose limits admit the key, and only
/// that one.
#[test]
fn a_name_is_found_through_two_levels() {
    let doc = document();
    let root = ObjRef::new(10, 0);

    assert!(name_tree_lookup(&doc, root, b"alpha").is_some());
    assert!(name_tree_lookup(&doc, root, b"zulu").is_some());
    assert!(
        name_tree_lookup(&doc, root, b"nothing-here").is_none(),
        "a key in no subtree is not found"
    );
}

/// A key between the two subtrees belongs to neither, and the limits say so
/// without either being read.
#[test]
fn a_key_outside_every_limit_is_not_found() {
    let doc = document();
    assert!(name_tree_lookup(&doc, ObjRef::new(10, 0), b"golf").is_none());
}

/// A node with no `/Limits` is descended into anyway: the index being damaged
/// is no reason to conclude the entry is absent.
#[test]
fn a_node_without_limits_is_still_searched() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
10 0 obj\n<< /Kids [11 0 R] >>\nendobj\n\
11 0 obj\n<< /Names [(only) (value)] >>\nendobj\n\
trailer\n<< /Size 12 /Root 1 0 R >>\n%%EOF\n";
    let doc = CosDocument::open(bytes).expect("it opens");
    assert!(name_tree_lookup(&doc, ObjRef::new(10, 0), b"only").is_some());
}

/// A tree that refers back to itself must not recurse forever.
#[test]
fn a_cyclic_tree_terminates() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
10 0 obj\n<< /Kids [10 0 R] >>\nendobj\n\
trailer\n<< /Size 11 /Root 1 0 R >>\n%%EOF\n";
    let doc = CosDocument::open(bytes).expect("it opens");
    assert!(name_tree_lookup(&doc, ObjRef::new(10, 0), b"anything").is_none());
}

/// Attachments are content a document plainly carries — the spreadsheet next
/// to the report, the machine-readable half of an invoice — and a reader that
/// cannot list them hides it.
#[test]
fn attachments_are_listed_with_their_filenames() {
    let doc = document();
    let found = attachments(&doc);

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "report.csv");
    assert_eq!(found[0].filename, "report.csv");
    assert_eq!(
        found[0].description.as_deref(),
        Some("the numbers behind the report")
    );
    assert_eq!(found[0].size, Some(9));
}

/// The stream is handed over rather than read: listing what a document carries
/// should not cost what extracting it costs.
#[test]
fn an_attachment_hands_over_its_stream_to_read() {
    let doc = document();
    let found = attachments(&doc);
    let reference = found[0].stream.expect("the embedded stream");

    let bytes = doc.stream_decoded(reference).expect("it decodes");
    assert!(
        String::from_utf8_lossy(&bytes).contains("a,b,c"),
        "the caller can read the attachment itself"
    );
}

#[test]
fn a_document_with_no_attachments_lists_none() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n";
    let doc = CosDocument::open(bytes).expect("it opens");
    assert!(attachments(&doc).is_empty());
    assert!(xmp_metadata(&doc).is_none());
}

/// XMP comes back unparsed. It is RDF/XML, and parsing it would mean an XML
/// reader this engine does not have and should not grow.
#[test]
fn xmp_metadata_is_reachable_as_bytes() {
    let doc = document();
    let xmp = xmp_metadata(&doc).expect("the metadata stream");
    assert!(String::from_utf8_lossy(&xmp).contains("identity here"));
}

/// The same absent-versus-empty rule `/Info` obeys, on the other metadata
/// path: a catalog naming a `/Metadata` stream that happens to be empty has
/// one, and `None` is reserved for a catalog that names none at all. An
/// archival profile requiring the stream to exist cares about the difference.
#[test]
fn an_empty_xmp_stream_is_present_rather_than_absent() {
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Metadata 20 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
20 0 obj\n<< /Type /Metadata /Subtype /XML /Length 0 >>\nstream\n\
endstream\nendobj\n\
trailer\n<< /Size 21 /Root 1 0 R >>\n%%EOF\n";
    let doc = CosDocument::open(bytes).expect("it opens");
    assert_eq!(xmp_metadata(&doc).as_deref(), Some(&[][..]));
}
