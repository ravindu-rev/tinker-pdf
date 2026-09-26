//! Stream deduplication on a rewrite (`WriteOptions::deduplicate_streams`),
//! held to the property that makes it safe: **identical means identical**.
//!
//! The first test is the reason the row exists and the reason it is
//! dangerous: two font dictionaries each embedding the same program. Merged,
//! the file carries the program once and both fonts name it; wrongly merged,
//! one font draws with the other's outlines and nothing in the file says so.
//! So the proof is a render — the page before and after, byte for byte —
//! and not a count of objects.
//!
//! The program is the vendored Liberation Serif, third-party bytes (ruling
//! 13): what its glyphs look like is a fact about the face, not about a
//! fixture written to pass.
//!
//! The collision half — a digest that agrees over bytes that do not — cannot
//! be produced with SHA-256 and is not faked here: `dedup.rs`'s own
//! `a_colliding_digest_never_merges_different_bytes` injects a digest that
//! collides on everything and shows the byte comparison still decides.

use tinker_pdf::{
    Dict, Document, DocumentBuilder, Name, ObjRef, Object, RenderOptions, WriteMode, WriteOptions,
};

fn face() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf");
    std::fs::read(path).expect("the vendored Liberation face is readable")
}

/// One page drawing text in two fonts, each embedding the whole face — two
/// identical `/FontFile2` streams under two font dictionaries.
fn two_fonts_one_program() -> Document {
    let program = face();
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"LiberationSerif", &program));
    assert!(builder.add_embedded_font(b"F1", b"LiberationSerifTwin", &program));
    builder.add_page(300.0, 120.0, |page| {
        page.text(b"F0", 28.0, 12.0, 70.0, "Glyphs, once");
        page.text(b"F1", 28.0, 12.0, 20.0, "and twice: Qxyz");
    });
    Document::open(builder.finish()).expect("the builder's output opens")
}

fn rewrite(doc: &Document, deduplicate_streams: bool, mode: WriteMode) -> Vec<u8> {
    doc.editor().save(&WriteOptions {
        mode,
        deduplicate_streams,
        ..WriteOptions::default()
    })
}

/// The `/FontFile2` each font on page 0 names, by resource name.
fn programs(doc: &Document) -> Vec<(Vec<u8>, ObjRef)> {
    let cos = doc.cos();
    let page = tinker_pdf_cos::pages::collect(cos)
        .into_iter()
        .next()
        .expect("a page");
    let resources = page.resources.expect("resources");
    let fonts = cos.resolve_key(&resources, cos.intern(b"Font"));
    let fonts = fonts.as_dict().expect("a /Font dictionary");
    let mut out = Vec::new();
    for (name, value) in fonts.iter() {
        let font = cos.resolve(value);
        let descriptor = cos.resolve_key(
            font.as_dict().expect("a font"),
            cos.intern(b"FontDescriptor"),
        );
        let program = descriptor
            .as_dict()
            .and_then(|d| d.get_ref(cos.intern(b"FontFile2")))
            .expect("an embedded program");
        let bytes = cos.name_bytes(*name).expect("a name").to_vec();
        out.push((bytes, program));
    }
    out.sort();
    out
}

fn render(doc: &Document) -> Vec<u8> {
    let bitmap = doc
        .page(0)
        .expect("page 0")
        .render(&RenderOptions::at_dpi(96.0));
    assert!(bitmap.data.iter().any(|b| *b < 128), "the page draws ink");
    bitmap.data
}

#[track_caller]
fn validated(bytes: Vec<u8>) -> Document {
    let doc = Document::open(bytes).expect("the rewrite reopens");
    let defects = doc.validate();
    assert!(
        defects.is_empty(),
        "the strict validator refused it: {defects:?}"
    );
    doc
}

#[test]
fn two_copies_of_one_font_program_become_one_and_render_the_same() {
    let original = two_fonts_one_program();
    let before = programs(&original);
    assert_eq!(before.len(), 2);
    assert_ne!(
        before[0].1, before[1].1,
        "the fixture embeds the program twice"
    );
    let pixels = render(&original);

    let plain = validated(rewrite(&original, false, WriteMode::Rewrite));
    let merged_bytes = rewrite(&original, true, WriteMode::Rewrite);
    let saved = merged_bytes.len();
    let merged = validated(merged_bytes);

    let after = programs(&merged);
    assert_eq!(after.len(), 2, "both fonts are still there");
    assert_eq!(after[0].1, after[1].1, "and both name one program");
    assert_eq!(
        merged
            .cos()
            .stream_decoded(after[0].1)
            .expect("the program decodes"),
        face(),
        "the program kept is the face, whole"
    );
    assert_eq!(render(&merged), pixels, "byte-equal render after the merge");
    assert_eq!(render(&plain), pixels);

    let without = rewrite(&original, false, WriteMode::Rewrite).len();
    assert!(
        saved + face().len() <= without + 64,
        "one copy of {} bytes left the file: {without} -> {saved}",
        face().len()
    );
}

/// An incremental update appends and must not rewrite what an earlier
/// revision (and a signature over it) covers: the switch does nothing there.
#[test]
fn an_incremental_update_merges_nothing() {
    let original = two_fonts_one_program();
    let mut editor = original.editor();
    editor.set_info(b"Title", "touched");
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        deduplicate_streams: true,
        ..WriteOptions::default()
    });
    let reopened = validated(bytes);
    let after = programs(&reopened);
    assert_ne!(after[0].1, after[1].1);
}

/// Two streams, one catalog array naming both, and the rest of the file.
fn with_two_streams(first: (Dict, Vec<u8>), second: (Dict, Vec<u8>)) -> (Document, [ObjRef; 2]) {
    let mut builder = DocumentBuilder::new();
    builder.add_page(50.0, 50.0, |_| {});
    let doc = Document::open(builder.finish()).expect("opens");
    let mut editor = doc.editor();
    let mut refs = [ObjRef::new(0, 0); 2];
    for (slot, (dict, data)) in refs.iter_mut().zip([first, second]) {
        *slot = editor.allocate();
        editor.put_stream(*slot, tinker_pdf_cos::StreamData { dict, data });
    }
    let key = editor.intern(b"PieceInfoProbe");
    assert!(editor.update_catalog(|catalog| {
        catalog.insert(
            key,
            Object::Array(refs.iter().map(|r| Object::Ref(*r)).collect()),
        );
    }));
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    (Document::open(saved).expect("reopens"), refs)
}

/// The two references the probe array holds after a deduplicating rewrite.
fn probe(doc: &Document) -> Vec<ObjRef> {
    let cos = doc.cos();
    let catalog = cos.catalog().expect("a catalog");
    let array = cos.resolve_key(&catalog, cos.intern(b"PieceInfoProbe"));
    array
        .as_array()
        .expect("the probe array")
        .iter()
        .filter_map(Object::as_objref)
        .collect()
}

fn flate_dict(doc_names: &Document, parms: Option<i64>) -> Dict {
    let cos = doc_names.cos();
    let mut dict = Dict::new();
    dict.insert(Name::FILTER, Object::Name(cos.intern(b"FlateDecode")));
    if let Some(columns) = parms {
        let mut p = Dict::new();
        p.insert(cos.intern(b"Columns"), Object::Int(columns));
        dict.insert(Name::DECODE_PARMS, Object::Dict(p));
    }
    dict
}

/// The same bytes behind two different `/DecodeParms` are two streams: the
/// dictionary is half of what "identical" means, even where — as here, with
/// no predictor — the parameters change nothing about the decoded bytes.
#[test]
fn equal_bytes_under_different_decode_parameters_stay_apart() {
    let names = two_fonts_one_program();
    let data = tinker_pdf_filters::zlib_compress(b"the same samples, twice");
    let (doc, _) = with_two_streams(
        (flate_dict(&names, Some(4)), data.clone()),
        (flate_dict(&names, Some(8)), data),
    );
    let merged = validated(rewrite(&doc, true, WriteMode::Rewrite));
    let refs = probe(&merged);
    assert_eq!(refs.len(), 2);
    assert_ne!(refs[0], refs[1], "kept apart");
}

/// A zlib stream of stored blocks (RFC 1950/1951, `BTYPE` 00): the same
/// decoded bytes as the encoder's output, in different stored bytes.
fn stored_zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = if data.is_empty() {
        vec![&[][..]]
    } else {
        data.chunks(65_535).collect()
    };
    for (index, chunk) in chunks.iter().enumerate() {
        out.push(u8::from(index + 1 == chunks.len()));
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// Equal dictionaries and equal **decoded** bytes merge though the stored
/// bytes differ — two encodings of one content are one content.
#[test]
fn two_encodings_of_one_content_merge() {
    let names = two_fonts_one_program();
    let content = b"one content, two encodings, one object".repeat(20);
    let packed = tinker_pdf_filters::zlib_compress(&content);
    let stored = stored_zlib(&content);
    assert_ne!(packed, stored);
    let (doc, _) = with_two_streams(
        (flate_dict(&names, None), packed),
        (flate_dict(&names, None), stored),
    );
    assert_eq!(
        doc.cos().stream_decoded(probe(&doc)[1]).expect("decodes"),
        content,
        "the stored-block stream is a valid encoding"
    );
    let merged = validated(rewrite(&doc, true, WriteMode::Rewrite));
    let refs = probe(&merged);
    assert_eq!(refs[0], refs[1], "one object");
    assert_eq!(
        merged.cos().stream_decoded(refs[0]).expect("decodes"),
        content
    );
}

/// Two damaged streams that decode to the same **partial** bytes — a valid
/// stored block, then a block header 7.4.4's deflate reserves (`BTYPE` 11)
/// followed by different bytes — are not the same stream. The lenient
/// decoder hands back what it could read, which is equal; the stored bytes
/// are not, and a decode that warned is never compared as if it were whole.
#[test]
fn two_damaged_streams_that_decode_alike_stay_apart() {
    let names = two_fonts_one_program();
    let prefix = b"the part both streams share before the damage".repeat(4);
    let damaged = |tail: &[u8]| {
        let mut out = vec![0x78, 0x01, 0x00];
        let len = prefix.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&prefix);
        // BFINAL 1, BTYPE 11: an error in every conforming decoder.
        out.push(0x07);
        out.extend_from_slice(tail);
        out
    };
    let (doc, _) = with_two_streams(
        (flate_dict(&names, None), damaged(b"first tail")),
        (flate_dict(&names, None), damaged(b"other tail")),
    );
    let cos = doc.cos();
    let refs = probe(&doc);
    let (a, b) = (cos.stream_decoded(refs[0]), cos.stream_decoded(refs[1]));
    assert_eq!(
        a.as_ref().ok(),
        b.as_ref().ok(),
        "the two decode alike, which is what makes this the dangerous case"
    );
    // Not held to the strict validator: the damage is the fixture, and a
    // rewrite carries it faithfully.
    let merged = Document::open(rewrite(&doc, true, WriteMode::Rewrite)).expect("reopens");
    let refs = probe(&merged);
    assert_ne!(refs[0], refs[1], "kept apart");
}
