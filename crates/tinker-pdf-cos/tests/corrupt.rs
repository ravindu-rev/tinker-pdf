//! The leniency ladder, exercised on damage constructed byte by byte.
//!
//! Every fixture here is built in this file rather than committed, so the
//! damage is legible next to the assertion about it: the point of each test is
//! the exact rung it lands on and the exact warning it emits, and a checked-in
//! blob would hide both.

use std::collections::BTreeSet;
use std::sync::Arc;

use tinker_pdf_cos::{
    CosDocument, Decryptor, LadderLevel, Name, ObjRef, Object, Warning, WarningKind, XrefEntry,
};

// ---- fixture construction -----------------------------------------------

/// A PDF assembled with real offsets, so damage can be introduced deliberately
/// rather than by accident.
struct Builder {
    buf: Vec<u8>,
    offsets: Vec<(u32, u64)>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            buf: b"%PDF-1.7\n".to_vec(),
            offsets: Vec::new(),
        }
    }

    fn at(&self) -> u64 {
        self.buf.len() as u64
    }

    fn raw(&mut self, bytes: &[u8]) -> &mut Builder {
        self.buf.extend_from_slice(bytes);
        self
    }

    fn obj(&mut self, num: u32, body: &[u8]) -> &mut Builder {
        self.offsets.push((num, self.at()));
        self.buf
            .extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        self.buf.extend_from_slice(body);
        self.buf.extend_from_slice(b"\nendobj\n");
        self
    }

    /// A stream whose `/Length` is correct by construction.
    fn stream(&mut self, num: u32, dict: &str, data: &[u8]) -> &mut Builder {
        let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.obj(num, &body)
    }

    /// A stream whose `/Length` is whatever the caller says, right or wrong.
    fn stream_len(&mut self, num: u32, dict: &str, data: &[u8], length: &str) -> &mut Builder {
        let mut body = format!("<< {dict} /Length {length} >>\nstream\n").into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        self.obj(num, &body)
    }

    fn max_num(&self) -> u32 {
        self.offsets.iter().map(|(n, _)| *n).max().unwrap_or(0)
    }

    fn offset_of(&self, num: u32) -> u64 {
        self.offsets
            .iter()
            .find(|(n, _)| *n == num)
            .map(|(_, o)| *o)
            .unwrap_or(0)
    }

    /// A classic cross-reference table plus trailer and `startxref`.
    fn finish(&mut self, trailer_extra: &str) -> Vec<u8> {
        let xref_at = self.at();
        let size = self.max_num() + 1;
        self.buf
            .extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
        self.buf.extend_from_slice(b"0000000000 65535 f \n");
        for num in 1..size {
            let offset = self.offset_of(num);
            self.buf
                .extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        self.buf.extend_from_slice(
            format!("trailer\n<< /Size {size} {trailer_extra} >>\nstartxref\n{xref_at}\n%%EOF\n")
                .as_bytes(),
        );
        self.buf.clone()
    }

    /// A cross-reference *stream* with `/W [1 4 2]`, so type-2 entries can be
    /// expressed at all.
    fn finish_xref_stream(
        &mut self,
        xref_num: u32,
        type2: &[(u32, u32, u32)],
        trailer_extra: &str,
    ) -> Vec<u8> {
        let xref_at = self.at();
        let size = self
            .max_num()
            .max(xref_num)
            .max(type2.iter().map(|(num, _, _)| *num).max().unwrap_or(0))
            + 1;
        let mut rows = Vec::with_capacity(size as usize * 7);
        for num in 0..size {
            let (kind, f2, f3): (u8, u32, u16) = if num == 0 {
                (0, 0, 65535)
            } else if let Some((_, stream_num, idx)) = type2.iter().find(|(n, _, _)| *n == num) {
                (2, *stream_num, u16::try_from(*idx).unwrap_or(0))
            } else if num == xref_num {
                (1, u32::try_from(xref_at).unwrap_or(0), 0)
            } else if self.offsets.iter().any(|(n, _)| *n == num) {
                (1, u32::try_from(self.offset_of(num)).unwrap_or(0), 0)
            } else {
                (0, 0, 0)
            };
            rows.push(kind);
            rows.extend_from_slice(&f2.to_be_bytes());
            rows.extend_from_slice(&f3.to_be_bytes());
        }
        let dict = format!(
            "<< /Type /XRef /Size {size} /W [1 4 2] {trailer_extra} /Length {} >>",
            rows.len()
        );
        self.buf
            .extend_from_slice(format!("{xref_num} 0 obj\n{dict}\nstream\n").as_bytes());
        self.buf.extend_from_slice(&rows);
        self.buf.extend_from_slice(b"\nendstream\nendobj\n");
        self.buf
            .extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
        self.buf.clone()
    }
}

/// A three-page document with a classic table, used as the honest baseline
/// that each damaged fixture is a mutation of.
fn baseline() -> Vec<u8> {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 3 /Kids [3 0 R 4 0 R 5 0 R] >>")
        .obj(3, b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>")
        .obj(4, b"<< /Type /Page /Parent 2 0 R >>")
        .obj(5, b"<< /Type /Page /Parent 2 0 R >>")
        .stream(6, "", b"BT /F0 12 Tf (hi) Tj ET");
    b.finish("/Root 1 0 R")
}

/// A zlib stream of stored deflate blocks: no compressor needed, and the
/// inflater's stored path is exactly what an xref stream exercises.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.push(0x01);
    let len = u16::try_from(data.len()).expect("fixture blocks stay under 64 KiB");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

fn kinds(doc: &CosDocument) -> Vec<WarningKind> {
    doc.warnings().iter().map(|w| w.kind).collect()
}

fn count_pages(doc: &CosDocument) -> usize {
    let root = doc.resolve_key(doc.trailer(), Name::ROOT);
    let Some(catalog) = root.as_dict() else {
        return 0;
    };
    let mut visited: BTreeSet<ObjRef> = BTreeSet::new();
    let mut stack = vec![doc.resolve_key(catalog, Name::PAGES)];
    let mut leaves = 0usize;
    while let Some(node) = stack.pop() {
        let Some(dict) = node.as_dict() else { continue };
        match dict.get(Name::KIDS) {
            Some(kids) => {
                let kids = doc.resolve(kids);
                for kid in kids.as_array().unwrap_or(&[]) {
                    if let Some(r) = kid.as_objref() {
                        if !visited.insert(r) {
                            continue;
                        }
                    }
                    stack.push(doc.resolve(kid));
                }
            }
            None => leaves += 1,
        }
    }
    leaves
}

// ---- the baseline is honest ---------------------------------------------

#[test]
fn the_baseline_opens_at_trust() {
    let doc = CosDocument::open(baseline()).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(kinds(&doc), Vec::new());
    assert_eq!(count_pages(&doc), 3);
}

// ---- level 1: leading junk shifts every offset ---------------------------

#[test]
fn junk_before_the_header_shifts_every_offset() {
    let mut bytes = b"GARBAGE BEFORE THE HEADER\n".to_vec();
    let shift = bytes.len();
    bytes.extend_from_slice(&baseline());
    let doc = CosDocument::open(bytes).expect("opens");

    assert_eq!(kinds(&doc), [WarningKind::HeaderNotAtStart]);
    // One uniform shift is enough: nothing needed the repair scanner.
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(count_pages(&doc), 3);
    let object = doc.get(ObjRef::new(1, 0)).expect("a catalog");
    assert!(object.as_dict().is_some());
    match doc.xref().get(3) {
        Some(XrefEntry::Offset { offset, .. }) => assert!(offset > shift as u64),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_with_no_header_still_opens() {
    let bytes = baseline();
    let headerless = bytes.get(9..).expect("past the header").to_vec();
    // Every offset is now wrong by the header's length, so this is level 3.
    let doc = CosDocument::open(headerless).expect("opens");
    assert!(kinds(&doc).contains(&WarningKind::HeaderMissing));
    assert_eq!(doc.ladder_level(), LadderLevel::Rescan);
    assert_eq!(count_pages(&doc), 3);
}

// ---- level 2: a lying offset --------------------------------------------

#[test]
fn a_lying_xref_offset_is_patched_from_the_scan() {
    let bytes = baseline();
    let mut damaged = bytes.clone();
    // Object 3's entry is the fourth line of the table; point it at nothing.
    let table = find(&damaged, b"xref\n0 7\n");
    let entry = table + b"xref\n0 7\n".len() + 3 * 20;
    damaged
        .get_mut(entry..entry + 10)
        .expect("an entry")
        .copy_from_slice(b"0000000004");

    let doc = CosDocument::open(damaged).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Patch);
    assert_eq!(
        kinds(&doc),
        [
            WarningKind::ObjectHeaderMismatch,
            WarningKind::ObjectRepaired
        ]
    );
    assert_eq!(
        doc.warnings()[1].object,
        Some(ObjRef::new(3, 0)),
        "the repair is addressed to the object it repaired"
    );

    // And the object that comes back is the right one.
    let page = doc.get(ObjRef::new(3, 0)).expect("a page");
    let dict = page.as_dict().expect("a dictionary");
    assert_eq!(dict.get_ref(Name::CONTENTS), Some(ObjRef::new(6, 0)));
    assert_eq!(count_pages(&doc), 3);
}

#[test]
fn an_offset_pointing_at_another_object_is_patched_too() {
    let bytes = baseline();
    let mut damaged = bytes.clone();
    let table = find(&damaged, b"xref\n0 7\n");
    let entry = table + b"xref\n0 7\n".len() + 3 * 20;
    // Point object 3's entry at object 1's header: a valid header, wrong
    // object, which is exactly what the number check exists to catch.
    damaged
        .get_mut(entry..entry + 10)
        .expect("an entry")
        .copy_from_slice(b"0000000009");
    let doc = CosDocument::open(damaged).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Patch);
    let page = doc.get(ObjRef::new(3, 0)).expect("a page");
    assert_eq!(
        page.as_dict().and_then(|d| d.get_ref(Name::PARENT)),
        Some(ObjRef::new(2, 0))
    );
}

// ---- level 3: nothing to trust ------------------------------------------

#[test]
fn a_dead_startxref_forces_a_rescan() {
    let bytes = baseline();
    let mut damaged = bytes.clone();
    let at = rfind(&damaged, b"startxref\n");
    let value = at + b"startxref\n".len();
    damaged
        .get_mut(value..value + 3)
        .expect("the offset digits")
        .copy_from_slice(b"999");

    let doc = CosDocument::open(damaged).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Rescan);
    assert!(kinds(&doc).contains(&WarningKind::DocumentRescanned));
    assert_eq!(count_pages(&doc), 3);
    assert_eq!(doc.trailer().get_ref(Name::ROOT), Some(ObjRef::new(1, 0)));
}

#[test]
fn a_missing_startxref_forces_a_rescan() {
    let bytes = baseline();
    let at = rfind(&bytes, b"startxref\n");
    let doc = CosDocument::open(bytes.get(..at).expect("the head").to_vec()).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Rescan);
    assert!(kinds(&doc).contains(&WarningKind::StartxrefMissing));
    assert_eq!(count_pages(&doc), 3);
}

#[test]
fn a_truncated_tail_keeps_the_objects_that_survived() {
    let bytes = baseline();
    // Cut in the middle of the cross-reference table.
    let at = find(&bytes, b"xref\n0 7\n") + 20;
    let doc = CosDocument::open(bytes.get(..at).expect("the head").to_vec()).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Rescan);
    assert_eq!(count_pages(&doc), 3);
    // The file's own trailer is gone, so /Root came from the catalog.
    assert!(kinds(&doc).contains(&WarningKind::RootSynthesized));
    assert_eq!(doc.trailer().get_ref(Name::ROOT), Some(ObjRef::new(1, 0)));
}

#[test]
fn a_file_with_no_objects_at_all_fails_to_open() {
    assert!(CosDocument::open(b"%PDF-1.7\nnothing here\n%%EOF".to_vec()).is_err());
    assert!(CosDocument::open(Vec::new()).is_err());
    assert!(CosDocument::open(vec![0u8; 4096]).is_err());
}

#[test]
fn obj_lookalikes_inside_stream_data_do_not_poison_the_index() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>")
        // The stream body spells a header for an object that also exists for
        // real, further on. The scanner must skip the body, not index it.
        .stream(
            3,
            "/Type /Page /Parent 2 0 R",
            b"4 0 obj\n<< /Trap true >>\nendobj\n",
        )
        .obj(4, b"<< /Real true >>");
    let mut bytes = b.finish("/Root 1 0 R");
    let at = rfind(&bytes, b"startxref\n") + b"startxref\n".len();
    bytes
        .get_mut(at..at + 3)
        .expect("the offset digits")
        .copy_from_slice(b"999");

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Rescan);
    let four = doc.get(ObjRef::new(4, 0)).expect("object 4");
    let dict = four.as_dict().expect("a dictionary");
    assert!(
        dict.contains_key(doc_name(&doc, "Real")),
        "the real object 4 won, not the one inside the stream"
    );
    assert_eq!(dict.len(), 1);
}

// ---- /Length policy ------------------------------------------------------

#[test]
fn a_wrong_length_is_recovered_and_reported() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        .stream_len(3, "", b"BT (recovered) Tj ET", "4");
    let bytes = b.finish("/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    let data = doc.stream_decoded(ObjRef::new(3, 0)).expect("a stream");
    assert_eq!(data, b"BT (recovered) Tj ET");
    assert_eq!(
        kinds(&doc),
        [WarningKind::StreamLengthRecovered {
            declared: Some(4),
            actual: 20
        }]
    );
    assert_eq!(doc.warnings()[0].object, Some(ObjRef::new(3, 0)));

    // The recovery happens once, however many times the stream is read.
    let _ = doc.stream_raw(ObjRef::new(3, 0)).expect("a stream");
    let _ = doc
        .stream_raw_encrypted(ObjRef::new(3, 0))
        .expect("a stream");
    assert_eq!(doc.warnings().len(), 1);
}

#[test]
fn a_missing_endstream_truncates_at_the_next_object() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>");
    // Written by hand: no `endstream` anywhere after the data.
    b.offsets.push((3, b.at()));
    b.raw(b"3 0 obj\n<< /Length 900 >>\nstream\nDATA DATA\nendobj\n");
    b.obj(4, b"<< /After true >>");
    let bytes = b.finish("/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    let data = doc.stream_raw(ObjRef::new(3, 0)).expect("a stream");
    assert!(kinds(&doc).contains(&WarningKind::StreamEndstreamMissing));
    assert!(
        data.starts_with(b"DATA DATA"),
        "{:?}",
        String::from_utf8_lossy(&data)
    );
    assert!(
        !data.windows(7).any(|w| w == b"4 0 obj"),
        "truncation stopped at the next header"
    );
}

#[test]
fn an_indirect_length_resolves_through_the_store() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        .stream_len(3, "", b"BT (indirect) Tj ET", "4 0 R")
        .obj(4, b"19");
    let bytes = b.finish("/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(
        doc.stream_decoded(ObjRef::new(3, 0)).expect("a stream"),
        b"BT (indirect) Tj ET"
    );
    // An indirect /Length is legal and common: nothing to warn about.
    assert_eq!(kinds(&doc), Vec::new());
    let object = doc.get(ObjRef::new(3, 0)).expect("a stream");
    assert_eq!(object.as_stream().and_then(|s| s.len_hint), Some(19));
}

#[test]
fn a_cyclic_length_yields_null_and_a_warning_without_hanging() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        // /Length points at the very object it is the length of.
        .stream_len(3, "", b"BT (cyclic) Tj ET", "3 0 R");
    let bytes = b.finish("/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    let object = doc.get(ObjRef::new(3, 0)).expect("a stream");
    let stream = object.as_stream().expect("a stream");
    assert_eq!(stream.len_hint, None, "the cycle resolved to null");
    let warnings = kinds(&doc);
    assert!(warnings.contains(&WarningKind::ObjectCycle), "{warnings:?}");
    assert_eq!(
        doc.warnings()
            .iter()
            .find(|w| w.kind == WarningKind::ObjectCycle)
            .and_then(|w| w.object),
        Some(ObjRef::new(3, 0))
    );
    // Recovery still gets the data out.
    assert_eq!(
        doc.stream_raw(ObjRef::new(3, 0)).expect("a stream"),
        b"BT (cyclic) Tj ET"
    );
}

#[test]
fn a_self_referential_object_terminates() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        .obj(3, b"3 0 R");
    let bytes = b.finish("/Root 1 0 R");
    let doc = CosDocument::open(bytes).expect("opens");
    let resolved = doc.resolve(&Object::Ref(ObjRef::new(3, 0)));
    assert!(resolved.is_null());
    assert!(kinds(&doc).contains(&WarningKind::ResolveDepthCapHit));
}

// ---- filters -------------------------------------------------------------

#[test]
fn a_filter_chain_runs_in_order() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        // "BT ET" hex-encoded, then run-length is a no-op wrapper here, so
        // just the one stage plus its abbreviation spelling.
        .stream(3, "/Filter /AHx", b"425420455421>");
    let bytes = b.finish("/Root 1 0 R");
    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(
        doc.stream_decoded(ObjRef::new(3, 0)).expect("a stream"),
        b"BT ET!"
    );
    // The undecoded tier hands back exactly what the file holds.
    assert_eq!(
        doc.stream_raw(ObjRef::new(3, 0)).expect("a stream"),
        b"425420455421>"
    );
}

#[test]
fn an_unknown_filter_stops_the_chain_and_warns() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        .stream(3, "/Filter [/AHx /MadeUpDecode]", b"4142>");
    let bytes = b.finish("/Root 1 0 R");
    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(
        doc.stream_decoded(ObjRef::new(3, 0)).expect("a stream"),
        b"AB"
    );
    assert!(kinds(&doc).contains(&WarningKind::FilterUnknown));
}

#[test]
fn an_image_codec_terminates_the_chain() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        .stream(3, "/Filter /DCTDecode", b"\xff\xd8\xff\xe0jpeg");
    let bytes = b.finish("/Root 1 0 R");
    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(
        doc.stream_decoded(ObjRef::new(3, 0)).expect("a stream"),
        b"\xff\xd8\xff\xe0jpeg"
    );
    assert!(kinds(&doc).contains(&WarningKind::ImageCodecNotDecoded));
}

// ---- cross-reference streams and object streams --------------------------

#[test]
fn an_xref_stream_document_opens() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>")
        .obj(3, b"<< /Type /Page /Parent 2 0 R >>");
    let bytes = b.finish_xref_stream(4, &[], "/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(kinds(&doc), Vec::new());
    assert_eq!(count_pages(&doc), 1);
    assert_eq!(doc.revisions().len(), 1);
}

#[test]
fn an_xref_stream_decodes_through_flate_and_a_png_predictor() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>");
    let xref_at = b.at();

    // Rows of 7 bytes each, PNG-filtered with tag 0 (None) so the predictor
    // path runs without needing a real encoder.
    let mut rows = Vec::new();
    let entries: [(u8, u32, u16); 4] = [
        (0, 0, 65535),
        (1, u32::try_from(b.offset_of(1)).expect("fits"), 0),
        (1, u32::try_from(b.offset_of(2)).expect("fits"), 0),
        (1, u32::try_from(xref_at).expect("fits"), 0),
    ];
    for (kind, f2, f3) in entries {
        rows.push(0u8);
        rows.push(kind);
        rows.extend_from_slice(&f2.to_be_bytes());
        rows.extend_from_slice(&f3.to_be_bytes());
    }
    let data = zlib_stored(&rows);
    let dict = format!(
        "<< /Type /XRef /Size 4 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode \
         /DecodeParms << /Predictor 12 /Columns 7 >> /Length {} >>",
        data.len()
    );
    b.raw(format!("3 0 obj\n{dict}\nstream\n").as_bytes());
    b.raw(&data);
    b.raw(b"\nendstream\nendobj\n");
    let bytes = b
        .raw(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes())
        .buf
        .clone();

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(kinds(&doc), Vec::new());
    assert_eq!(
        doc.get(ObjRef::new(1, 0))
            .expect("a catalog")
            .as_dict()
            .and_then(|d| d.get_ref(Name::PAGES)),
        Some(ObjRef::new(2, 0))
    );
}

#[test]
fn an_unknown_xref_stream_type_reads_as_null() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>");
    let xref_at = b.at();
    let mut rows = Vec::new();
    // Object 0 free, 1 and 2 type 1, object 3 an invented type 7.
    for (kind, f2, f3) in [
        (0u8, 0u32, 65535u16),
        (1, u32::try_from(b.offset_of(1)).expect("fits"), 0),
        (1, u32::try_from(b.offset_of(2)).expect("fits"), 0),
        (7, 12345, 0),
        (1, u32::try_from(xref_at).expect("fits"), 0),
    ] {
        rows.push(kind);
        rows.extend_from_slice(&f2.to_be_bytes());
        rows.extend_from_slice(&f3.to_be_bytes());
    }
    let dict = format!(
        "<< /Type /XRef /Size 5 /W [1 4 2] /Root 1 0 R /Length {} >>",
        rows.len()
    );
    b.raw(format!("4 0 obj\n{dict}\nstream\n").as_bytes());
    b.raw(&rows);
    b.raw(b"\nendstream\nendobj\n");
    let bytes = b
        .raw(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes())
        .buf
        .clone();

    let doc = CosDocument::open(bytes).expect("opens");
    assert!(kinds(&doc).contains(&WarningKind::XrefStreamUnknownType));
    // 7.5.8.3: any other type is a reference to the null object.
    assert!(doc.get(ObjRef::new(3, 0)).expect("a value").is_null());
}

#[test]
fn an_object_stream_is_decompressed_exactly_once() {
    let mut b = Builder::new();
    // Three objects packed into one container.
    let payload_objects: [&[u8]; 3] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R /Note (packed) >>",
    ];
    let mut bodies = Vec::new();
    let mut pairs = String::new();
    for (i, body) in payload_objects.iter().enumerate() {
        pairs.push_str(&format!("{} {} ", i + 1, bodies.len()));
        bodies.extend_from_slice(body);
        bodies.push(b' ');
    }
    let first = pairs.len();
    let mut data = pairs.into_bytes();
    data.extend_from_slice(&bodies);
    b.stream(4, &format!("/Type /ObjStm /N 3 /First {first}"), &data);
    let bytes = b.finish_xref_stream(5, &[(1, 4, 0), (2, 4, 1), (3, 4, 2)], "/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(kinds(&doc), Vec::new());
    assert_eq!(count_pages(&doc), 1);

    for num in 1..=3 {
        assert!(!doc.get(ObjRef::new(num, 0)).expect("a value").is_null());
    }
    assert_eq!(
        doc.object_stream_decodes(),
        1,
        "three objects out of one container is one inflate"
    );
    assert_eq!(
        doc.xref().get(2),
        Some(XrefEntry::InStream {
            stream_num: 4,
            idx: 1
        })
    );
}

#[test]
fn object_streams_are_found_again_after_a_rescan() {
    let mut b = Builder::new();
    let mut bodies = Vec::new();
    let mut pairs = String::new();
    for (i, body) in [
        &b"<< /Type /Catalog /Pages 2 0 R >>"[..],
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R >>",
    ]
    .iter()
    .enumerate()
    {
        pairs.push_str(&format!("{} {} ", i + 1, bodies.len()));
        bodies.extend_from_slice(body);
        bodies.push(b' ');
    }
    let first = pairs.len();
    let mut data = pairs.into_bytes();
    data.extend_from_slice(&bodies);
    b.stream(4, &format!("/Type /ObjStm /N 3 /First {first}"), &data);
    let mut bytes = b.finish_xref_stream(5, &[(1, 4, 0), (2, 4, 1), (3, 4, 2)], "/Root 1 0 R");

    let at = rfind(&bytes, b"startxref\n") + b"startxref\n".len();
    bytes
        .get_mut(at..at + 3)
        .expect("the offset digits")
        .copy_from_slice(b"999");

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Rescan);
    // The scanner cannot see objects inside a container, so the containers it
    // did find are expanded into type-2 entries.
    assert_eq!(
        doc.xref().get(3),
        Some(XrefEntry::InStream {
            stream_num: 4,
            idx: 2
        })
    );
    assert_eq!(count_pages(&doc), 1);
}

#[test]
fn a_broken_object_stream_header_still_yields_its_objects() {
    let mut b = Builder::new();
    let data = b"1 0 2 34 << /Type /Catalog /Pages 2 0 R >> << /Type /Pages /Count 0 /Kids [] >>";
    // /First is a lie and /N is missing entirely.
    b.stream(4, "/Type /ObjStm /First 999", data);
    let bytes = b.finish_xref_stream(5, &[(1, 4, 0), (2, 4, 1)], "/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    let catalog = doc.get(ObjRef::new(1, 0)).expect("a catalog");
    assert_eq!(
        catalog.as_dict().and_then(|d| d.get_ref(Name::PAGES)),
        Some(ObjRef::new(2, 0))
    );
    assert!(kinds(&doc).contains(&WarningKind::ObjStmHeaderBad));
}

// ---- incremental updates, hybrids and /Prev ------------------------------

#[test]
fn a_prev_chain_shadows_older_revisions() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>")
        .obj(3, b"<< /Type /Page /Parent 2 0 R /Rev 1 >>");
    let first_xref = b.at();
    b.finish("/Root 1 0 R");

    // A second revision that replaces object 3 in place.
    let new_offset = b.at();
    b.raw(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /Rev 2 >>\nendobj\n");
    let second_xref = b.at();
    b.raw(
        format!(
            "xref\n0 1\n0000000000 65535 f \n3 1\n{new_offset:010} 00000 n \n\
             trailer\n<< /Size 4 /Root 1 0 R /Prev {first_xref} >>\n\
             startxref\n{second_xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    let bytes = b.buf.clone();

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(kinds(&doc), Vec::new());
    assert_eq!(doc.revisions().len(), 2, "newest first");
    assert!(doc.revisions()[0].byte_range.end > doc.revisions()[1].byte_range.end);

    let page = doc.get(ObjRef::new(3, 0)).expect("a page");
    let rev = page
        .as_dict()
        .and_then(|d| d.get_int(doc_name(&doc, "Rev")));
    assert_eq!(rev, Some(2), "the newer revision shadows the older");
    // Objects only the older revision defined are still reachable.
    assert!(doc
        .get(ObjRef::new(1, 0))
        .expect("a catalog")
        .as_dict()
        .is_some());
}

#[test]
fn a_hybrid_file_consults_the_xrefstm() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>")
        .obj(3, b"<< /Type /Page /Parent 2 0 R >>");
    let first_xref = b.at();
    b.finish("/Root 1 0 R");

    // Revision two adds object 5, findable only through the /XRefStm.
    let five_at = b.at();
    b.raw(b"5 0 obj\n<< /HybridOnly true >>\nendobj\n");
    let stm_at = b.at();
    let mut rows = Vec::new();
    for (kind, f2, f3) in [
        (0u8, 0u32, 65535u16),
        (1, u32::try_from(five_at).expect("fits"), 0),
    ] {
        rows.push(kind);
        rows.extend_from_slice(&f2.to_be_bytes());
        rows.extend_from_slice(&f3.to_be_bytes());
    }
    let dict = format!(
        "<< /Type /XRef /Size 6 /W [1 4 2] /Index [0 1 5 1] /Root 1 0 R /Length {} >>",
        rows.len()
    );
    b.raw(format!("4 0 obj\n{dict}\nstream\n").as_bytes());
    b.raw(&rows);
    b.raw(b"\nendstream\nendobj\n");

    let second_xref = b.at();
    b.raw(
        format!(
            "xref\n0 1\n0000000000 65535 f \n4 1\n{stm_at:010} 00000 n \n\
             trailer\n<< /Size 6 /Root 1 0 R /XRefStm {stm_at} /Prev {first_xref} >>\n\
             startxref\n{second_xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    let bytes = b.buf.clone();

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert_eq!(kinds(&doc), Vec::new());
    let five = doc.get(ObjRef::new(5, 0)).expect("the hybrid-only object");
    assert_eq!(five.as_dict().map(tinker_pdf_cos::Dict::len), Some(1));
    assert_eq!(count_pages(&doc), 1);
}

#[test]
fn a_prev_cycle_stops_the_chain() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>");
    let xref_at = b.at();
    // The section's /Prev points at itself.
    b.raw(
        format!(
            "xref\n0 3\n0000000000 65535 f \n{:010} 00000 n \n{:010} 00000 n \n\
             trailer\n<< /Size 3 /Root 1 0 R /Prev {xref_at} >>\nstartxref\n{xref_at}\n%%EOF\n",
            b.offset_of(1),
            b.offset_of(2)
        )
        .as_bytes(),
    );
    let bytes = b.buf.clone();

    let doc = CosDocument::open(bytes).expect("opens");
    assert_eq!(kinds(&doc), [WarningKind::XrefPrevCycle]);
    assert_eq!(doc.ladder_level(), LadderLevel::Trust);
    assert!(doc
        .get(ObjRef::new(1, 0))
        .expect("a catalog")
        .as_dict()
        .is_some());
}

// ---- encryption seam -----------------------------------------------------

struct ReverseDecryptor;

impl Decryptor for ReverseDecryptor {
    fn decrypt_string(&self, _containing: ObjRef, data: &[u8]) -> Vec<u8> {
        data.iter().rev().copied().collect()
    }
    fn decrypt_stream(&self, _r: ObjRef, data: &[u8]) -> Vec<u8> {
        data.iter().rev().copied().collect()
    }
}

/// Reverses strings and leaves streams alone, so an object stream's own bytes
/// survive the round trip and the only visible effect is on strings.
struct ReverseStringsOnly;

impl Decryptor for ReverseStringsOnly {
    fn decrypt_string(&self, _containing: ObjRef, data: &[u8]) -> Vec<u8> {
        data.iter().rev().copied().collect()
    }
    fn decrypt_stream(&self, _r: ObjRef, data: &[u8]) -> Vec<u8> {
        data.to_vec()
    }
}

#[test]
fn a_decryptor_reaches_strings_and_streams_but_not_the_exemptions() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R /Note (abc) >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        .obj(7, b"<< /Filter /Standard /V 1 /R 2 /O (owner) /U (user) >>")
        .stream(3, "", b"plain");
    let bytes = b.finish("/Root 1 0 R /Encrypt 7 0 R");

    let mut doc = CosDocument::open(bytes).expect("opens");
    let params = doc
        .encrypt_params()
        .expect("an /Encrypt dictionary")
        .clone();
    assert_eq!(params.encrypt_ref, Some(ObjRef::new(7, 0)));
    // 7.6.2: the /Encrypt dictionary's own strings are never decrypted.
    assert_eq!(params.o.as_deref(), Some(&b"owner"[..]));

    doc.set_decryptor(Arc::new(ReverseDecryptor));
    let catalog = doc.get(ObjRef::new(1, 0)).expect("a catalog");
    let note = catalog
        .as_dict()
        .and_then(|d| d.get_string(doc_name(&doc, "Note")))
        .map(|s| s.bytes.clone());
    assert_eq!(note, Some(b"cba".to_vec()));

    // The forensic tier is still the file's own bytes.
    assert_eq!(
        doc.stream_raw_encrypted(ObjRef::new(3, 0))
            .expect("a stream"),
        b"plain"
    );
    assert_eq!(
        doc.stream_raw(ObjRef::new(3, 0)).expect("a stream"),
        b"nialp"
    );

    // And the /Encrypt object itself, reloaded, is still plaintext.
    let encrypt = doc.get(ObjRef::new(7, 0)).expect("the /Encrypt dictionary");
    assert_eq!(
        encrypt
            .as_dict()
            .and_then(|d| d.get_string(doc_name(&doc, "O")))
            .map(|s| s.bytes.clone()),
        Some(b"owner".to_vec())
    );
}

#[test]
fn strings_inside_an_object_stream_are_not_decrypted_again() {
    let mut b = Builder::new();
    let bodies: &[&[u8]] = &[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 0 /Kids [] /Note (abc) >>",
    ];
    let mut payload = Vec::new();
    let mut pairs = String::new();
    for (i, body) in bodies.iter().enumerate() {
        pairs.push_str(&format!("{} {} ", i + 1, payload.len()));
        payload.extend_from_slice(body);
        payload.push(b' ');
    }
    let first = pairs.len();
    let mut data = pairs.into_bytes();
    data.extend_from_slice(&payload);
    b.stream(4, &format!("/Type /ObjStm /N 2 /First {first}"), &data);
    b.obj(7, b"<< /Filter /Standard /V 1 /R 2 >>");
    b.obj(8, b"<< /Note (abc) >>");
    let bytes = b.finish_xref_stream(5, &[(1, 4, 0), (2, 4, 1)], "/Root 1 0 R /Encrypt 7 0 R");

    let mut doc = CosDocument::open(bytes).expect("opens");
    doc.set_decryptor(Arc::new(ReverseStringsOnly));
    let note = doc_name(&doc, "Note");

    // A string in an ordinary indirect object goes through the decryptor.
    let plain = doc.get(ObjRef::new(8, 0)).expect("an ordinary object");
    assert_eq!(
        plain
            .as_dict()
            .and_then(|d| d.get_string(note))
            .map(|s| s.bytes.clone()),
        Some(b"cba".to_vec())
    );

    // The container stream was decrypted as a whole; its strings must not be
    // put through the decryptor a second time (7.6.2).
    let pages = doc.get(ObjRef::new(2, 0)).expect("the pages node");
    assert_eq!(
        pages
            .as_dict()
            .and_then(|d| d.get_string(note))
            .map(|s| s.bytes.clone()),
        Some(b"abc".to_vec())
    );
}

// ---- determinism and concurrency ----------------------------------------

#[test]
fn the_same_bytes_always_produce_the_same_warnings() {
    let bytes = {
        let mut damaged = baseline();
        let table = find(&damaged, b"xref\n0 7\n");
        let entry = table + b"xref\n0 7\n".len() + 3 * 20;
        damaged
            .get_mut(entry..entry + 10)
            .expect("an entry")
            .copy_from_slice(b"0000000004");
        // Plus a wrong /Length, so the warning list is not trivially short.
        damaged
    };

    let run = || -> (LadderLevel, Vec<Warning>) {
        let doc = CosDocument::open(bytes.clone()).expect("opens");
        for (num, _) in doc.xref().iter() {
            let _ = doc.get(ObjRef::new(num, 0));
        }
        let _ = doc.stream_decoded(ObjRef::new(6, 0));
        let _ = count_pages(&doc);
        (doc.ladder_level(), doc.warnings())
    };

    let expected = run();
    assert!(
        !expected.1.is_empty(),
        "the fixture must warn about something"
    );
    for i in 0..100 {
        assert_eq!(run(), expected, "run {i} diverged");
    }
}

#[test]
fn eight_threads_resolving_a_reference_cycle_do_not_deadlock() {
    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] /Peer 4 0 R >>")
        .obj(3, b"<< /Type /Page /Parent 2 0 R /Peer 4 0 R >>")
        // 4 and 5 point at each other, and both point back at 2.
        .obj(4, b"<< /Peer 5 0 R /Up 2 0 R >>")
        .obj(5, b"<< /Peer 4 0 R /Up 3 0 R >>");
    let bytes = b.finish("/Root 1 0 R");

    let doc = Arc::new(CosDocument::open(bytes).expect("opens"));
    let mut handles = Vec::new();
    for t in 0..8u32 {
        let doc = Arc::clone(&doc);
        handles.push(std::thread::spawn(move || {
            for i in 0..500u32 {
                // Different threads start at different objects on purpose, so
                // the mutually referencing pair is entered from both sides.
                let start = 1 + ((t + i) % 5);
                let object = doc.get(ObjRef::new(start, 0)).expect("a value");
                let Some(dict) = object.as_dict() else {
                    continue;
                };
                for (_, value) in dict.iter() {
                    let _ = doc.resolve(value);
                }
            }
        }));
    }
    for handle in handles {
        handle.join().expect("no thread may panic");
    }
    assert_eq!(doc.warnings(), Vec::new());
    assert_eq!(count_pages(&doc), 1);
}

// ---- structural edges ----------------------------------------------------

#[test]
fn integer_nesting_and_dictionary_damage_survive_the_document_layer() {
    let mut deep = Vec::new();
    for _ in 0..500 {
        deep.extend_from_slice(b"[");
    }
    for _ in 0..500 {
        deep.extend_from_slice(b"]");
    }

    let mut b = Builder::new();
    b.obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
        .obj(2, b"<< /Type /Pages /Count 0 /Kids [] >>")
        // Beyond i64 in both directions, plus a doubled sign.
        .obj(
            3,
            b"[99999999999999999999 -99999999999999999999 --5 6.02e23]",
        )
        .obj(4, &deep)
        // A duplicate key, then a key with no value at all.
        .obj(5, b"<< /A 1 /B 2 /A 3 /C >>");
    let bytes = b.finish("/Root 1 0 R");

    let doc = CosDocument::open(bytes).expect("opens");
    let numbers = doc.get(ObjRef::new(3, 0)).expect("an array");
    let items = numbers.as_array().expect("an array");
    assert_eq!(items[0], Object::Int(i64::MAX));
    assert_eq!(items[1], Object::Int(i64::MIN));
    assert_eq!(items[2], Object::Int(-5));
    assert_eq!(items[3], Object::Real(6.02e23));

    assert!(doc
        .get(ObjRef::new(4, 0))
        .expect("an array")
        .as_array()
        .is_some());
    let recovered = doc.get(ObjRef::new(5, 0)).expect("a dictionary");
    let dict = recovered.as_dict().expect("a dictionary");
    assert_eq!(dict.len(), 3);
    assert_eq!(dict.get_int(doc_name(&doc, "A")), Some(3));
    assert_eq!(dict.get(doc_name(&doc, "C")), Some(&Object::Null));

    // Every warning names the object it happened in.
    for warning in doc.warnings() {
        assert!(warning.object.is_some(), "{warning}");
    }
    let warned: BTreeSet<u32> = doc
        .warnings()
        .iter()
        .filter_map(|w| w.object.map(|r| r.num))
        .collect();
    assert!(warned.contains(&3));
    assert!(warned.contains(&4));
    assert!(warned.contains(&5));
}

#[test]
fn a_reference_to_nothing_reads_as_null() {
    let doc = CosDocument::open(baseline()).expect("opens");
    assert!(doc.get(ObjRef::new(4242, 0)).expect("a value").is_null());
    // 7.3.10 makes this legal, so it is not a warning.
    assert_eq!(doc.warnings(), Vec::new());
}

// ---- helpers -------------------------------------------------------------

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or_else(|| panic!("{:?} not in the fixture", String::from_utf8_lossy(needle)))
}

fn rfind(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len())
        .rposition(|w| w == needle)
        .unwrap_or_else(|| panic!("{:?} not in the fixture", String::from_utf8_lossy(needle)))
}

/// Interns a name against the document's own table, which is the only table
/// its [`Name`]s are valid against.
fn doc_name(doc: &CosDocument, name: &str) -> Name {
    doc.intern(name.as_bytes())
}
