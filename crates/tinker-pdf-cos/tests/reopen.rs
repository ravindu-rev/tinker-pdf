//! A document over an original and the update appended to it, read without
//! joining them (`CosDocument::open_appended`), held to the document
//! `CosDocument::open` gives the joined bytes (`docs/design/performance.md`,
//! P-17).

mod written_support;

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf_cos::write::{write_object, WriteMode, WriteOptions};
use tinker_pdf_cos::{
    pages, CosDocument, Dict, DocumentBuilder, DocumentEditor, LadderLevel, Name, ObjRef, Object,
    StreamData, XrefEntry,
};

/// The options an incremental save takes.
fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

/// A document of `pages` pages of text, as the builder writes it.
fn built(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for i in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {i}"));
        });
    }
    builder.finish()
}

/// `bytes` rewritten with object streams and a cross-reference stream.
fn packed(bytes: Vec<u8>) -> Vec<u8> {
    let editor = DocumentEditor::new(Arc::new(CosDocument::open(bytes).expect("it opens")));
    editor.save(&WriteOptions {
        object_streams: true,
        ..WriteOptions::default()
    })
}

/// The committed AES-256 fixture, whose user password is `open-sesame`.
fn encrypted() -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/encrypted-aes256.pdf");
    std::fs::read(path).expect("the fixture")
}

/// A hybrid-reference file (7.5.8.4), written by hand: a classic table for
/// objects one to five, whose trailer's `/XRefStm` names a cross-reference
/// stream holding object six, which the table does not.
fn hybrid() -> Vec<u8> {
    fn object(out: &mut Vec<u8>, offsets: &mut Vec<(u32, usize)>, num: u32, body: &[u8]) {
        offsets.push((num, out.len()));
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    fn offset_of(offsets: &[(u32, usize)], num: u32) -> usize {
        offsets
            .iter()
            .find(|(n, _)| *n == num)
            .map_or(0, |(_, at)| *at)
    }
    let mut out = b"%PDF-1.5\n".to_vec();
    let mut offsets = Vec::new();
    object(
        &mut out,
        &mut offsets,
        1,
        b"<< /Type /Catalog /Pages 2 0 R >>",
    );
    object(
        &mut out,
        &mut offsets,
        2,
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
    );
    object(
        &mut out,
        &mut offsets,
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Resources << >> /Note 6 0 R >>",
    );
    let content = b"0 0 1 rg 10 10 50 50 re f";
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content);
    stream.extend_from_slice(b"\nendstream");
    object(&mut out, &mut offsets, 4, &stream);
    object(&mut out, &mut offsets, 6, b"<< /HybridOnly true >>");
    let six = offset_of(&offsets, 6);
    let mut row = vec![1u8];
    row.extend_from_slice(&u32::try_from(six).expect("small").to_be_bytes());
    row.extend_from_slice(&0u16.to_be_bytes());
    let mut xref_stream = format!(
        "<< /Type /XRef /Size 7 /W [1 4 2] /Index [6 1] /Length {} >>\nstream\n",
        row.len()
    )
    .into_bytes();
    xref_stream.extend_from_slice(&row);
    xref_stream.extend_from_slice(b"\nendstream");
    object(&mut out, &mut offsets, 5, &xref_stream);
    let stm = offset_of(&offsets, 5);
    let table = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for num in 1..=5u32 {
        let at = offset_of(&offsets, num);
        out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 7 /Root 1 0 R /XRefStm {stm} >>\nstartxref\n{table}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

/// A builder document whose table states its catalog's offset a byte past
/// the catalog's header, which an open repairs from a scan of the file
/// (`Patch`). The catalog is what [`edit`] leaves alone, so the table's lie
/// outlives every update.
fn misplaced() -> Vec<u8> {
    let mut bytes = built(3);
    let doc = CosDocument::open(bytes.clone()).expect("it opens");
    let catalog = doc.trailer().get_ref(Name::ROOT).expect("a catalog");
    let first = pages::collect(&doc)[0].reference;
    assert!(
        catalog.num != 6 && catalog != first,
        "the premise: no edit redefines the catalog"
    );
    let Some(XrefEntry::Offset { offset, .. }) = doc.xref().get(catalog.num) else {
        panic!("the catalog has an offset");
    };
    let row = format!("{offset:010} 00000 n").into_bytes();
    let wrong = format!("{:010} 00000 n", offset + 1).into_bytes();
    let hits: Vec<usize> = bytes
        .windows(row.len())
        .enumerate()
        .filter(|(_, window)| *window == row.as_slice())
        .map(|(at, _)| at)
        .collect();
    assert_eq!(hits.len(), 1, "the row is the table's alone");
    bytes[hits[0]..hits[0] + row.len()].copy_from_slice(&wrong);
    bytes
}

/// Three objects and no cross-reference data at all, which an open rebuilds
/// from a scan (`Rescan`).
fn tableless() -> Vec<u8> {
    concat!(
        "%PDF-1.7\n",
        "1 0 obj <</Type /Catalog /Pages 2 0 R>>\nendobj\n",
        "2 0 obj <</Type /Pages /Kids [3 0 R] /Count 1>>\nendobj\n",
        "3 0 obj <</Type /Page /Parent 2 0 R /MediaBox [0 0 10 10]>>\nendobj\n",
    )
    .as_bytes()
    .to_vec()
}

/// `bytes` with every line ending at its end taken off, so an update after
/// it starts with the newline 7.5.6 asks for and its last comment runs to
/// the update's first byte.
fn unended(mut bytes: Vec<u8>) -> Vec<u8> {
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    bytes
}

/// A document over `bytes`, authenticated when it is encrypted.
fn opened(bytes: impl Into<Arc<[u8]>>, password: Option<&str>) -> Arc<CosDocument> {
    let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
    if let Some(password) = password {
        doc.authenticate(password).expect("the password opens it");
    }
    doc
}

/// The edit every base takes: page one turned, a new object, the first
/// page's content replaced by a stream of its own, and an object the base
/// has redefined.
fn edit(base: &Arc<CosDocument>) -> DocumentEditor {
    let mut editor = DocumentEditor::new(Arc::clone(base));
    let _ = editor.rotate_page(0, 90);
    let note = editor.allocate();
    editor.put(note, Object::Int(7));
    let content = editor.allocate();
    editor.put_stream(
        content,
        StreamData {
            dict: Dict::new(),
            data: b"1 0 0 rg 0 0 5 5 re f".to_vec(),
        },
    );
    if let Some(page) = editor.page_refs().first().copied() {
        if let Some(Object::Dict(mut dict)) = editor.get(page) {
            dict.insert(Name::CONTENTS, Object::Ref(content));
            editor.put(page, Object::Dict(dict));
        }
    }
    if let Some(Object::Dict(mut dict)) = editor.get(ObjRef::new(6, 0)) {
        dict.insert(editor.intern(b"Edited"), Object::Bool(true));
        editor.put(ObjRef::new(6, 0), Object::Dict(dict));
    }
    editor
}

/// Everything a reader is told about a document, one line a fact, with every
/// name written out, so that two documents' symbol tables cannot part them:
/// the ladder, the trailer, the revisions, every entry, every object with its
/// stream's extent, raw bytes and decode, every page and its content, and the
/// warnings.
fn reading(doc: &CosDocument) -> Vec<String> {
    let text = |object: &Object| {
        let mut bytes = Vec::new();
        write_object(&mut bytes, object, doc.names_table());
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let mut out = vec![format!(
        "ladder {:?}, encrypted {}, version {:?}, highest {}, streamed {}, whole {}",
        doc.ladder_level(),
        doc.is_encrypted(),
        doc.header_version(),
        doc.max_object_number(),
        doc.is_streamed(),
        doc.whole_file_fetched(),
    )];
    out.push(format!(
        "trailer {}",
        text(&Object::Dict(doc.trailer().clone()))
    ));
    for revision in doc.revisions() {
        out.push(format!(
            "revision {:?} at {}: {}",
            revision.byte_range,
            revision.xref_at,
            text(&Object::Dict(revision.trailer.clone()))
        ));
    }
    let numbers: Vec<u32> = doc.xref().iter().map(|(num, _)| num).collect();
    for (num, entry) in doc.xref().iter() {
        out.push(format!("entry {num} {entry:?}"));
    }
    for num in numbers {
        let r = ObjRef::new(num, 0);
        let object = doc.get(r).expect("a read never fails");
        out.push(format!("object {num} {}", text(&object)));
        if let Object::Stream(stream) = object.as_ref() {
            out.push(format!(
                "stream {num} at {}, length {:?}",
                stream.data_start, stream.len_hint
            ));
            out.push(format!("stream {num} raw {:?}", doc.stream_raw(r).ok()));
            out.push(format!(
                "stream {num} decoded {:?}",
                doc.stream_decoded(r).ok()
            ));
        }
    }
    for page in pages::collect(doc) {
        out.push(format!(
            "page {} {:?} {:?} {:?} turned {}: {}",
            page.index,
            page.reference,
            page.media_box,
            page.crop_box,
            page.rotation,
            page.resources
                .as_ref()
                .map(|resources| text(&Object::Dict(resources.clone())))
                .unwrap_or_default()
        ));
        out.push(format!(
            "content {} {:?}",
            page.index,
            pages::content_bytes(doc, &page)
        ));
    }
    out.push(format!("warnings {:?}", doc.warnings()));
    out
}

/// The two readings, line for line, naming the first that parts them.
#[track_caller]
fn same_reading(got: &[String], want: &[String], what: &str) {
    for (index, (got, want)) in got.iter().zip(want).enumerate() {
        assert!(
            got == want,
            "{what}: line {index}\n  got  {got}\n  want {want}"
        );
    }
    assert_eq!(got.len(), want.len(), "{what}: how many lines");
}

/// One base: what it is called, its bytes, the password it opens with, and
/// whether it is held as this engine's own writing.
struct Base {
    name: &'static str,
    bytes: Vec<u8>,
    password: Option<&'static str>,
    authored: bool,
}

/// Every kind of base: the builder's classic table, a packed one with object
/// streams and a cross-reference stream, an encrypted one, a hybrid-reference
/// one, one whose last line has no ending, one whose table lies about an
/// offset, and one with no table at all.
fn bases() -> Vec<Base> {
    vec![
        Base {
            name: "a classic table",
            bytes: built(4),
            password: None,
            authored: true,
        },
        Base {
            name: "object streams",
            bytes: packed(built(4)),
            password: None,
            authored: true,
        },
        Base {
            name: "encrypted",
            bytes: encrypted(),
            password: Some("open-sesame"),
            authored: false,
        },
        Base {
            name: "hybrid references",
            bytes: hybrid(),
            password: None,
            authored: false,
        },
        Base {
            name: "no line ending at the end",
            bytes: unended(built(2)),
            password: None,
            authored: true,
        },
        Base {
            name: "a misplaced offset",
            bytes: misplaced(),
            password: None,
            authored: false,
        },
        Base {
            name: "no table",
            bytes: tableless(),
            password: None,
            authored: false,
        },
    ]
}

/// **Two segments read as their concatenation.** For every kind of base and
/// an edit of each, the document over the base and its update section reads,
/// fact for fact, as the document over the two joined: every entry, object,
/// stream extent, raw and decoded stream, page and content, the trailer and
/// revisions, the ladder and every warning — the patched and the rescanned
/// base's repairs among them, whose scans read the whole file, and the reads
/// that cross the seam, the tail `startxref` is found in and the last line of
/// a base with no line ending. A second update over the first reads the same
/// way, and the whole-file read is the joined bytes.
#[test]
fn two_segments_read_as_their_concatenation() {
    for base in bases() {
        let original: Arc<[u8]> = base.bytes.clone().into();
        let doc = opened(Arc::clone(&original), base.password);
        let editor = edit(&doc);
        let update: Arc<[u8]> = editor.save_update(&incremental()).into();
        let joined = [&original[..], &update[..]].concat();
        if base.authored {
            written_support::authored(base.name, joined.clone());
        }
        assert!(
            editor.save(&incremental()) == joined,
            "{}: the update is the incremental save's section",
            base.name
        );

        let segmented = CosDocument::open_appended(Arc::clone(&original), Arc::clone(&update))
            .expect("it opens over two segments");
        let whole = CosDocument::open(joined.clone()).expect("it opens joined");
        for doc in [&segmented, &whole] {
            if let Some(password) = base.password {
                doc.authenticate(password).expect("the password opens it");
            }
        }
        assert!(!segmented.is_streamed() && segmented.whole_file_fetched());
        same_reading(&reading(&segmented), &reading(&whole), base.name);
        assert!(segmented.bytes() == joined.as_slice(), "{}", base.name);
        assert_eq!(
            segmented.ladder_level() == LadderLevel::Trust,
            base.name != "a misplaced offset" && base.name != "no table",
            "{}: {:?}",
            base.name,
            segmented.warnings()
        );

        // A second update, over the first, as the first was over its base.
        let first = Arc::new(segmented);
        let editor = edit(&first);
        let second: Arc<[u8]> = editor.save_update(&incremental()).into();
        let both = [&joined[..], &second[..]].concat();
        if base.authored {
            written_support::authored(base.name, both.clone());
        }
        let segmented = CosDocument::open_appended(joined.clone(), Arc::clone(&second))
            .expect("it opens over the first update and the second");
        let whole = CosDocument::open(both.clone()).expect("it opens joined");
        for doc in [&segmented, &whole] {
            if let Some(password) = base.password {
                doc.authenticate(password).expect("the password opens it");
            }
        }
        same_reading(
            &reading(&segmented),
            &reading(&whole),
            &format!("{}, a second update", base.name),
        );
    }
}
