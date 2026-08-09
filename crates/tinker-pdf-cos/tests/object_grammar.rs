//! Walks a small well-formed file through the public API only — the same way
//! the cross-reference and store layers will use it.

use tinker_pdf_cos::{
    parse_indirect_at, parse_object_at, Name, NameTable, ObjRef, Object, WarningSink,
};

const FILE: &[u8] = b"%PDF-1.7\n\
1 0 obj\n\
<< /Type /Catalog /Pages 2 0 R >>\n\
endobj\n\
2 0 obj\n\
<< /Type /Pages /Kids [3 0 R] /Count 1 >>\n\
endobj\n\
3 0 obj\n\
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.276 841.89] /Contents 4 0 R >>\n\
endobj\n\
4 0 obj\n\
<< /Length 47 >>\n\
stream\n\
BT /F1 12 Tf 72 712 Td (A stream of text) Tj ET\n\
endstream\n\
endobj\n\
trailer\n\
<< /Size 5 /Root 1 0 R >>\n";

fn offset_of(needle: &[u8]) -> u64 {
    FILE.windows(needle.len())
        .position(|w| w == needle)
        .expect("fixture contains the marker") as u64
}

#[test]
fn every_object_in_a_clean_file_parses_without_warnings() {
    let names = NameTable::new();
    let mut sink = WarningSink::new();

    let catalog = parse_indirect_at(FILE, offset_of(b"1 0 obj"), &names, &mut sink).expect("1 0");
    assert_eq!(catalog.reference, ObjRef::new(1, 0));
    let dict = catalog.object.as_dict().expect("a dictionary");
    assert_eq!(dict.get_name(Name::TYPE), Some(names.intern(b"Catalog")));
    assert_eq!(dict.get_ref(Name::PAGES), Some(ObjRef::new(2, 0)));

    let pages = parse_indirect_at(FILE, offset_of(b"2 0 obj"), &names, &mut sink).expect("2 0");
    let dict = pages.object.as_dict().expect("a dictionary");
    assert_eq!(dict.get_int(Name::COUNT), Some(1));
    assert_eq!(
        dict.get_array(Name::KIDS),
        Some(&[Object::Ref(ObjRef::new(3, 0))][..])
    );

    let page = parse_indirect_at(FILE, offset_of(b"3 0 obj"), &names, &mut sink).expect("3 0");
    let dict = page.object.as_dict().expect("a dictionary");
    assert_eq!(dict.get_ref(Name::PARENT), Some(ObjRef::new(2, 0)));
    let media_box: Vec<f64> = dict
        .get_array(Name::MEDIA_BOX)
        .expect("a media box")
        .iter()
        .filter_map(Object::as_number)
        .collect();
    assert_eq!(media_box, [0.0, 0.0, 595.276, 841.89]);

    let content = parse_indirect_at(FILE, offset_of(b"4 0 obj"), &names, &mut sink).expect("4 0");
    let stream = content.object.as_stream().expect("a stream");
    assert_eq!(stream.len_hint, Some(47));
    let range = stream.data_range();
    let data = &FILE[range.start as usize..range.end as usize];
    assert_eq!(data, b"BT /F1 12 Tf 72 712 Td (A stream of text) Tj ET");
    assert_eq!(&FILE[content.end_offset as usize..][..10], b"\nendstream");

    let trailer_at = offset_of(b"trailer") + b"trailer".len() as u64;
    let trailer = parse_object_at(FILE, trailer_at, &names, &mut sink);
    let dict = trailer.object.as_dict().expect("a dictionary");
    assert_eq!(dict.get_int(Name::SIZE), Some(5));
    assert_eq!(dict.get_ref(Name::ROOT), Some(ObjRef::new(1, 0)));

    // A clean file produces no warnings at all: that is what makes an empty
    // sink usable as the "this file was honest" signal.
    assert!(sink.is_empty(), "{:?}", sink.warnings());
}

#[test]
fn one_name_table_is_shared_across_objects() {
    let names = NameTable::new();
    let mut sink = WarningSink::new();
    let first = parse_indirect_at(FILE, offset_of(b"1 0 obj"), &names, &mut sink).expect("1 0");
    let second = parse_indirect_at(FILE, offset_of(b"2 0 obj"), &names, &mut sink).expect("2 0");
    let type_of = |o: &Object| o.as_dict().and_then(|d| d.get(Name::TYPE)).cloned();
    assert_eq!(
        type_of(&first.object),
        Some(Object::Name(names.intern(b"Catalog")))
    );
    assert_eq!(
        type_of(&second.object),
        Some(Object::Name(Name::PAGES)),
        "a pre-interned name keeps its constant id"
    );
}
