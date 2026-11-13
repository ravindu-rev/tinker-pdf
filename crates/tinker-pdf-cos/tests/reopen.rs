//! A document over an original and the update appended to it, read without
//! joining them (`CosDocument::open_appended`), and a document reopened over
//! the one an edit was made to, reading only the update
//! (`CosDocument::reopen`), each held to the document `CosDocument::open`
//! gives the joined bytes (`docs/design/performance.md`, P-17).

mod written_support;

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use proptest::prelude::*;
use tinker_pdf_cos::limits::MAX_REOPEN_DEPTH;
use tinker_pdf_cos::write::{write_object, WriteMode, WriteOptions};
use tinker_pdf_cos::{
    annot, pages, CosDocument, Dict, DocumentBuilder, DocumentEditor, FullOpen, LadderLevel, Name,
    ObjRef, Object, PdfString, Rect, SliceSource, StreamData, Warning, XrefEntry,
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

/// A classic-table file written by hand: `bodies` numbered from one, in
/// order, object one its catalog.
fn classic(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(bodies.len());
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let table = out.len();
    let size = bodies.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for at in offsets {
        out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n").as_bytes(),
    );
    out
}

/// A stream object's body: `dict`, then `data` between `stream` and
/// `endstream`.
fn stream_body(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("{dict}\nstream\n").into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// Two pages whose content streams state their `/Length` indirectly, written
/// by hand: page one's in object 5, and page two's through a chain, object
/// 7 naming object 8, which holds it.
fn lengths() -> Vec<u8> {
    let first: &[u8] = b"0 0 1 rg 10 10 50 50 re f";
    let second: &[u8] = b"1 0 0 rg 20 20 30 30 re f";
    classic(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 2 /Kids [3 0 R 9 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Resources << >> >>"
            .to_vec(),
        stream_body("<< /Length 5 0 R >>", first),
        first.len().to_string().into_bytes(),
        b"<< /Kind /Plain >>".to_vec(),
        b"8 0 R".to_vec(),
        second.len().to_string().into_bytes(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 10 0 R \
          /Resources << >> >>"
            .to_vec(),
        stream_body("<< /Length 7 0 R >>", second),
    ])
}

/// [`built`]'s three pages laid out for the first page to arrive first
/// (Annex F).
fn linearized() -> Vec<u8> {
    let editor = DocumentEditor::new(Arc::new(CosDocument::open(built(3)).expect("it opens")));
    editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        // A container would hold the first page's objects in the same blob
        // as everything else, and there would be no head to open from.
        object_streams: false,
        ..WriteOptions::default()
    })
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

/// The bases a reopen keeps, each kind of cross-reference data and security
/// among them: the builder's classic table, object streams with a
/// cross-reference stream, an encrypted file, a hybrid-reference one, and one
/// whose content streams state their lengths indirectly.
fn kept() -> Vec<Base> {
    vec![
        Base {
            name: "a classic table",
            bytes: built(3),
            password: None,
            authored: true,
        },
        Base {
            name: "object streams",
            bytes: packed(built(3)),
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
            name: "indirect lengths",
            bytes: lengths(),
            password: None,
            authored: false,
        },
    ]
}

/// One edit of a random sequence. What it edits is picked out of the
/// document as it stands, by an index taken modulo what there is to pick.
#[derive(Clone, Debug)]
enum Op {
    /// A new object, holding an integer.
    Add(i64),
    /// An object replaced by an integer, a string or a dictionary naming
    /// another object.
    Replace(usize, i64),
    /// An object replaced by a stream.
    Stream(usize),
    /// An object deleted.
    Delete(usize),
    /// A page turned.
    Rotate(usize),
    /// A square annotation, its appearance synthesized, added to a page.
    Annotate(usize),
    /// A page's content replaced by a new stream.
    Contents(usize),
    /// A trailer entry of the caller's own.
    Trailer(i64),
    /// An object a stream's `/Length` names, given the length it resolves
    /// to plus `delta`.
    Length(usize, i64),
    /// An object stream's container deleted, or replaced by a dictionary.
    Container(usize, bool),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        any::<i64>().prop_map(Op::Add),
        (any::<usize>(), any::<i64>()).prop_map(|(at, value)| Op::Replace(at, value)),
        any::<usize>().prop_map(Op::Stream),
        any::<usize>().prop_map(Op::Delete),
        any::<usize>().prop_map(Op::Rotate),
        any::<usize>().prop_map(Op::Annotate),
        any::<usize>().prop_map(Op::Contents),
        any::<i64>().prop_map(Op::Trailer),
        (any::<usize>(), -3i64..=3).prop_map(|(at, delta)| Op::Length(at, delta)),
        (any::<usize>(), any::<bool>()).prop_map(|(at, delete)| Op::Container(at, delete)),
    ]
}

/// The item of `from` that `at` picks, when there is one.
fn pick<T: Copy>(from: &[T], at: usize) -> Option<T> {
    from.get(at.checked_rem(from.len())?).copied()
}

/// The page index of `pages` that `at` picks, when there is one.
fn page_at(pages: &[ObjRef], at: usize) -> Option<u32> {
    u32::try_from(at.checked_rem(pages.len())?).ok()
}

/// `op` made through `editor` to `doc`, the document it edits.
fn apply(editor: &mut DocumentEditor, doc: &CosDocument, op: &Op) {
    // Every number with an entry but the free head and the `/Encrypt`
    // dictionary, which an update may not redefine and keep its base.
    let encrypt = doc.trailer().get_ref(Name::ENCRYPT).map(|r| r.num);
    let numbers: Vec<u32> = doc
        .xref()
        .iter()
        .map(|(num, _)| num)
        .filter(|num| *num != 0 && Some(*num) != encrypt)
        .collect();
    let pages = editor.page_refs();
    match *op {
        Op::Add(value) => {
            let r = editor.allocate();
            editor.put(r, Object::Int(value));
        }
        Op::Replace(at, value) => {
            let Some(num) = pick(&numbers, at) else {
                return;
            };
            let object = match value.rem_euclid(3) {
                0 => Object::Int(value),
                1 => Object::String(PdfString::literal(format!("edited {value}").into_bytes())),
                _ => {
                    let other = usize::try_from(value.unsigned_abs())
                        .ok()
                        .and_then(|at| pick(&numbers, at))
                        .unwrap_or(num);
                    let mut dict = Dict::new();
                    dict.insert(editor.intern(b"Other"), Object::Ref(ObjRef::new(other, 0)));
                    Object::Dict(dict)
                }
            };
            editor.put(ObjRef::new(num, 0), object);
        }
        Op::Stream(at) => {
            if let Some(num) = pick(&numbers, at) {
                editor.put_stream(
                    ObjRef::new(num, 0),
                    StreamData {
                        dict: Dict::new(),
                        data: format!("stream {num}").into_bytes(),
                    },
                );
            }
        }
        Op::Delete(at) => {
            if let Some(num) = pick(&numbers, at) {
                editor.delete(ObjRef::new(num, 0));
            }
        }
        Op::Rotate(at) => {
            if let Some(index) = page_at(&pages, at) {
                let _ = editor.rotate_page(index, 90);
            }
        }
        Op::Annotate(at) => {
            if let Some(index) = page_at(&pages, at) {
                let square = annot::square(
                    doc,
                    Rect {
                        x0: 10.0,
                        y0: 10.0,
                        x1: 60.0,
                        y1: 60.0,
                    },
                    annot::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                    },
                    1.0,
                );
                let _ = editor.add_annotation(index, square);
            }
        }
        Op::Contents(at) => {
            let Some(page) = pick(&pages, at) else {
                return;
            };
            let content = editor.allocate();
            editor.put_stream(
                content,
                StreamData {
                    dict: Dict::new(),
                    data: b"0 1 0 rg 0 0 8 8 re f".to_vec(),
                },
            );
            if let Some(Object::Dict(mut dict)) = editor.get(page) {
                dict.insert(Name::CONTENTS, Object::Ref(content));
                editor.put(page, Object::Dict(dict));
            }
        }
        Op::Trailer(value) => {
            let note = editor.intern(b"Note");
            let _ = editor.set_trailer_entry(note, Object::Int(value));
        }
        Op::Length(at, delta) => {
            let named: Vec<ObjRef> = numbers
                .iter()
                .filter_map(|num| {
                    let object = doc.get(ObjRef::new(*num, 0)).ok()?;
                    object.as_stream()?.dict.get(Name::LENGTH)?.as_objref()
                })
                .collect();
            let Some(target) = pick(&named, at) else {
                return;
            };
            let length = doc.resolve(&Object::Ref(target)).as_int().unwrap_or(0);
            editor.put(target, Object::Int(length.saturating_add(delta)));
        }
        Op::Container(at, delete) => {
            let containers: Vec<u32> = doc
                .xref()
                .iter()
                .filter_map(|(_, entry)| match entry {
                    XrefEntry::InStream { stream_num, .. } => Some(stream_num),
                    _ => None,
                })
                .collect::<BTreeSet<u32>>()
                .into_iter()
                .collect();
            let Some(num) = pick(&containers, at) else {
                return;
            };
            if delete {
                editor.delete(ObjRef::new(num, 0));
            } else {
                editor.put(ObjRef::new(num, 0), Object::Dict(Dict::new()));
            }
        }
    }
}

/// A reopen of `base` over `update` that `want` sends to a full open, held
/// to a fresh open of the joined bytes: `want` is the reason it gives, and
/// the document it opens reads, fact for fact and warning for warning, as
/// `CosDocument::open` reads `original` followed by `update`. The reopened
/// document and the joined bytes, for what comes after.
#[track_caller]
fn falls_back(
    what: &str,
    base: &Arc<CosDocument>,
    original: &[u8],
    update: Vec<u8>,
    password: Option<&str>,
    want: FullOpen,
) -> (Arc<CosDocument>, Vec<u8>) {
    let joined = [original, &update[..]].concat();
    let reopened = CosDocument::reopen(base, update).expect("it reopens");
    assert_eq!(reopened.full_open, Some(want), "{what}");
    let fresh = opened(joined.clone(), password);
    same_reading(&reading(&reopened.document), &reading(&fresh), what);
    (Arc::new(reopened.document), joined)
}

/// `update` with its trailer's `/Prev` naming `prev` instead.
fn with_prev(update: &[u8], prev: u64) -> Vec<u8> {
    let key = b"/Prev";
    let at = update
        .windows(key.len())
        .rposition(|window| window == key)
        .expect("the trailer has a /Prev")
        + key.len();
    let digits = at
        + update[at..]
            .iter()
            .take_while(|b| b.is_ascii_whitespace())
            .count();
    let end = digits
        + update[digits..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
    [
        &update[..digits],
        prev.to_string().as_bytes(),
        &update[end..],
    ]
    .concat()
}

/// `update` with the offsets its table states for its first two objects
/// swapped, so each row names where the other's header is.
fn swapped_rows(update: &[u8]) -> Vec<u8> {
    let table = update
        .windows(6)
        .position(|window| window == b"\nxref\n")
        .expect("a table")
        + 1;
    let mut rows = Vec::new();
    let mut at = table;
    for line in update[table..].split(|b| *b == b'\n') {
        if line.starts_with(b"trailer") {
            break;
        }
        // `nnnnnnnnnn ggggg n `, the twenty bytes of an in-use row (7.5.4).
        if line.len() == 19 && line.get(17) == Some(&b'n') {
            rows.push(at);
        }
        at += line.len() + 1;
    }
    assert!(rows.len() >= 2, "the premise: two objects in the table");
    let (first, second) = (rows[0], rows[1]);
    let mut out = update.to_vec();
    let one = out[first..first + 10].to_vec();
    let other = out[second..second + 10].to_vec();
    out[first..first + 10].copy_from_slice(&other);
    out[second..second + 10].copy_from_slice(&one);
    out
}

/// An update section written by hand after `base`: each of `objects`, a
/// number and its body, then a classic table placing them after object
/// zero's free head, and `base`'s trailer with the `/Prev` that chains the
/// section to it, as `trailer` then edits it.
fn appended(
    base: &CosDocument,
    objects: &[(u32, Vec<u8>)],
    trailer: impl FnOnce(&mut Dict),
) -> Vec<u8> {
    let start = base.bytes().len();
    let mut out = Vec::new();
    if !base.bytes().ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut rows = Vec::new();
    for (num, body) in objects {
        rows.push((*num, start + out.len()));
        out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let table = start + out.len();
    out.extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
    for (num, at) in rows {
        out.extend_from_slice(format!("{num} 1\n{at:010} 00000 n \n").as_bytes());
    }
    let mut dict = base.trailer().clone();
    dict.insert(
        Name::PREV,
        Object::Int(i64::try_from(base.last_startxref()).expect("a small offset")),
    );
    trailer(&mut dict);
    out.extend_from_slice(b"trailer\n");
    write_object(&mut out, &Object::Dict(dict), base.names_table());
    out.extend_from_slice(format!("\nstartxref\n{table}\n%%EOF\n").as_bytes());
    out
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    /// **A reopen reads as a fresh open does.** Random sequences of edits —
    /// objects added, replaced by values and by streams, and deleted; pages
    /// turned, annotated and given new content; a trailer entry set; the
    /// objects a stream's `/Length` names given other lengths; object
    /// streams' containers deleted and replaced — on a classic table, object streams,
    /// an encrypted file, a hybrid-reference one and one whose lengths are
    /// indirect, each update saved and reopened over the document before it,
    /// a chain of them. Every reopen reads the update alone, and reads, fact
    /// for fact, as `CosDocument::open` reads the joined bytes: every entry,
    /// object, stream extent, raw and decoded stream, page and content, the
    /// trailer, the revisions and the ladder. Its warnings are those a fresh
    /// open of the joined bytes raises and those its base had raised, and
    /// nothing else.
    #[test]
    fn a_reopen_reads_as_a_fresh_open_does(
        rounds in proptest::collection::vec(proptest::collection::vec(op(), 1..6), 1..4),
    ) {
        for base in kept() {
            let mut bytes = base.bytes.clone();
            let mut doc = opened(bytes.clone(), base.password);
            for (round, ops) in rounds.iter().enumerate() {
                let what = format!("{}, update {}", base.name, round + 1);
                let mut editor = DocumentEditor::new(Arc::clone(&doc));
                for op in ops {
                    apply(&mut editor, &doc, op);
                }
                let update = editor.save_update(&incremental());
                bytes.extend_from_slice(&update);
                let reopened = CosDocument::reopen(&doc, update).expect("it reopens");
                prop_assert_eq!(
                    reopened.full_open,
                    None,
                    "{}: read through the update alone",
                    &what
                );
                let reopened = Arc::new(reopened.document);
                let fresh = opened(bytes.clone(), base.password);

                let mut got = reading(&reopened);
                let mut want = reading(&fresh);
                // The warnings, the last line of each, are held below as the
                // set each document raised.
                got.pop();
                want.pop();
                same_reading(&got, &want, &what);
                let raised: HashSet<Warning> = reopened.warnings().into_iter().collect();
                let mut expected: HashSet<Warning> = fresh.warnings().into_iter().collect();
                expected.extend(doc.warnings());
                prop_assert!(
                    raised == expected,
                    "{}: the reopen raised {:?}; a fresh open {:?}, over its base's {:?}",
                    &what,
                    reopened.warnings(),
                    fresh.warnings(),
                    doc.warnings()
                );
                doc = reopened;
            }
        }
    }
}

/// **Every base a reopen cannot keep says why.** Each of `FullOpen`'s
/// reasons, met by the base or by the update that has it, sends the reopen to
/// a full open that names it, and the document it opens reads, fact for fact
/// and warning for warning, as a fresh open of the joined bytes: a linearized
/// base opened from its head, one streamed from a source, a patched and a
/// rescanned one, one whose header is not at byte zero, one already reopened
/// as deep as a chain goes — whose full open starts a chain of its own — and
/// the updates that do not chain: a `startxref` naming the base's own
/// section, a `/Prev` past the base's newest section, rows that state each
/// other's objects, an update after a base whose last revision has no
/// `%%EOF`, and sections that move an encrypted base's first `/ID` string or
/// redefine its `/Encrypt` dictionary.
#[test]
fn every_base_a_reopen_cannot_keep_says_why() {
    // The bases, by what they are.
    let original = linearized();
    let base = Arc::new(
        CosDocument::open_source(Arc::new(SliceSource::new(original.clone())))
            .expect("it opens from a source"),
    );
    assert!(
        base.first_page_end().is_some(),
        "the premise: the head-only path engages"
    );
    let update = edit(&base).save_update(&incremental());
    falls_back(
        "a linearized base",
        &base,
        &original,
        update,
        None,
        FullOpen::Linearized,
    );

    let original = built(3);
    let base = Arc::new(
        CosDocument::open_source(Arc::new(SliceSource::new(original.clone())))
            .expect("it opens from a source"),
    );
    assert!(base.is_streamed() && base.first_page_end().is_none());
    let update = edit(&base).save_update(&incremental());
    falls_back(
        "a streamed base",
        &base,
        &original,
        update,
        None,
        FullOpen::Streamed,
    );
    assert!(base.whole_file_fetched(), "its bytes were fetched whole");

    for (what, original) in [
        ("a patched base", misplaced()),
        ("a rescanned base", tableless()),
    ] {
        let base = opened(original.clone(), None);
        assert_ne!(base.ladder_level(), LadderLevel::Trust, "the premise");
        let update = edit(&base).save_update(&incremental());
        falls_back(what, &base, &original, update, None, FullOpen::Repaired);
    }

    let original = [&b"bytes before the header\n"[..], &built(2)[..]].concat();
    let base = opened(original.clone(), None);
    assert_eq!(
        base.ladder_level(),
        LadderLevel::Trust,
        "the premise: every offset is found through the shift"
    );
    let update = edit(&base).save_update(&incremental());
    falls_back(
        "a shifted base",
        &base,
        &original,
        update,
        None,
        FullOpen::Shifted,
    );

    // As deep as a chain of reopens goes, and one more.
    let mut bytes = built(2);
    let mut doc = opened(bytes.clone(), None);
    for depth in 1..=MAX_REOPEN_DEPTH + 1 {
        let mut editor = DocumentEditor::new(Arc::clone(&doc));
        let note = editor.allocate();
        editor.put(note, Object::Int(i64::from(depth)));
        let update = editor.save_update(&incremental());
        if depth <= MAX_REOPEN_DEPTH {
            bytes.extend_from_slice(&update);
            let reopened = CosDocument::reopen(&doc, update).expect("it reopens");
            assert_eq!(reopened.full_open, None, "reopen {depth} reads the update");
            doc = Arc::new(reopened.document);
        } else {
            (doc, bytes) = falls_back(
                "a base reopened as deep as a chain goes",
                &doc,
                &bytes,
                update,
                None,
                FullOpen::Deep,
            );
        }
    }
    written_support::authored("a chain of reopens", bytes.clone());
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let note = editor.allocate();
    editor.put(note, Object::Int(0));
    let reopened =
        CosDocument::reopen(&doc, editor.save_update(&incremental())).expect("it reopens");
    assert_eq!(
        reopened.full_open, None,
        "the full open started a chain of its own"
    );

    // The updates that do not chain.
    let original = built(2);
    let base = opened(original.clone(), None);
    let update = format!("startxref\n{}\n%%EOF\n", base.last_startxref()).into_bytes();
    falls_back(
        "a startxref naming the base's own section",
        &base,
        &original,
        update,
        None,
        FullOpen::Unchained,
    );

    let first = built(2);
    let one = edit(&opened(first.clone(), None)).save_update(&incremental());
    let original = [&first[..], &one[..]].concat();
    let base = opened(original.clone(), None);
    assert_eq!(base.revisions().len(), 2, "the premise: two revisions");
    let older = base.revisions()[1].xref_at;
    let update = with_prev(&edit(&base).save_update(&incremental()), older);
    falls_back(
        "a /Prev past the base's newest section",
        &base,
        &original,
        update,
        None,
        FullOpen::Unchained,
    );

    let original = built(2);
    let base = opened(original.clone(), None);
    let update = swapped_rows(&edit(&base).save_update(&incremental()));
    falls_back(
        "rows that state each other's objects",
        &base,
        &original,
        update,
        None,
        FullOpen::Unchained,
    );

    let mut original = built(2);
    let eof = original
        .windows(5)
        .rposition(|window| window == b"%%EOF")
        .expect("an end of file");
    original.truncate(eof);
    let base = opened(original.clone(), None);
    assert_eq!(
        base.ladder_level(),
        LadderLevel::Trust,
        "the premise: it opens on its table"
    );
    let update = edit(&base).save_update(&incremental());
    falls_back(
        "an update after a base with no %%EOF",
        &base,
        &original,
        update,
        None,
        FullOpen::Unchained,
    );

    // The committed encrypted file, whose `/Encrypt` dictionary is direct in
    // its trailer: an update that moves the first `/ID` string.
    let original = encrypted();
    let base = opened(original.clone(), Some("open-sesame"));
    let second = base
        .trailer()
        .get_array(Name::ID)
        .and_then(|id| id.get(1).cloned())
        .expect("the premise: a two-string /ID");
    let update = appended(&base, &[], |trailer| {
        trailer.insert(
            Name::ID,
            Object::Array(vec![
                Object::String(PdfString::literal(b"another document".to_vec())),
                second,
            ]),
        );
    });
    falls_back(
        "a section moving an encrypted base's first /ID string",
        &base,
        &original,
        update,
        Some("open-sesame"),
        FullOpen::Unchained,
    );

    // The same file with its `/Encrypt` dictionary moved into an object of
    // its own by an update, and a section after that redefining the object.
    let fixture = encrypted();
    let plain = opened(fixture.clone(), None);
    let mut body = Vec::new();
    write_object(
        &mut body,
        plain
            .trailer()
            .get(Name::ENCRYPT)
            .expect("the premise: an /Encrypt dictionary"),
        plain.names_table(),
    );
    let held = plain.max_object_number() + 1;
    let moved = appended(&plain, &[(held, body.clone())], |trailer| {
        trailer.insert(Name::ENCRYPT, Object::Ref(ObjRef::new(held, 0)));
        trailer.insert(Name::SIZE, Object::Int(i64::from(held) + 1));
    });
    let original = [&fixture[..], &moved[..]].concat();
    let base = opened(original.clone(), Some("open-sesame"));
    assert_eq!(
        base.trailer().get_ref(Name::ENCRYPT),
        Some(ObjRef::new(held, 0)),
        "the premise: an /Encrypt object"
    );
    let update = appended(&base, &[(held, body)], |_| {});
    falls_back(
        "a section redefining the /Encrypt dictionary",
        &base,
        &original,
        update,
        Some("open-sesame"),
        FullOpen::Unchained,
    );
}
