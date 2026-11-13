//! The writer's compression on the engine's own pool (row P-15,
//! `docs/design/parallelism.md`): a save that compresses its streams side by
//! side is the serial save, byte for byte, in every shape a save takes.
//!
//! Each stream deflates alone into a slot of its own and the objects are
//! serialised and encrypted in their order after, so nothing about the
//! schedule can reach the file: these tests hold that over a rewrite, an
//! incremental update, the object-stream container and the linearized
//! layout, encrypted and not, at 2, 3, 4 and 7 threads.

use std::sync::Arc;

use tinker_pdf_cos::{
    pages, AuthLevel, CosDocument, Dict, DocumentBuilder, DocumentEditor, Encryption, Name, Object,
    Parallelism, StreamData, WriteMode, WriteOptions,
};

/// Forty-eight deterministic bytes: the same key, salts and IVs every run.
fn entropy() -> [u8; 48] {
    let mut bytes = [0u8; 48];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(13).wrapping_add(5);
    }
    bytes
}

/// Bytes from a linear congruential generator, which deflate does not
/// shrink.
fn noise(count: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(17);
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

/// Content-stream-like bytes that deflate well, different for each seed.
fn compressible(count: usize, seed: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(count + 64);
    let mut line = seed;
    while out.len() < count {
        line = line.wrapping_mul(31).wrapping_add(7);
        out.extend_from_slice(
            format!(
                "BT /F0 {} Tf {} {} Td (line {line} of stream {seed}) Tj ET\n",
                8 + line % 9,
                line % 180,
                line % 90
            )
            .as_bytes(),
        );
    }
    out
}

/// Twelve pages of text, as the builder writes them.
fn source() -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for page in 0..12 {
        builder.add_page(200.0, 100.0, |draw| {
            draw.text(
                b"F0",
                12.0,
                10.0,
                50.0,
                &format!("Page {} of twelve", page + 1),
            );
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("the source opens"))
}

/// `editor` with forty-one streams more, each named from the first page so
/// that a save collecting garbage keeps them: most of them deflate well; one
/// in eight is noise deflate does not shrink, which goes in as it stands; one
/// is empty, one already declares a filter, which the writer hands through,
/// and one is an XMP packet, which compression never reaches (14.3.2).
fn with_streams(editor: &mut DocumentEditor) {
    let filter = Name::FILTER;
    let hex = editor.intern(b"ASCIIHexDecode");
    let metadata = editor.intern(b"Metadata");
    let subtype = editor.intern(b"Subtype");
    let xml = editor.intern(b"XML");
    let mut named = Vec::new();
    for index in 0..41u32 {
        let r = editor.allocate();
        named.push(Object::Ref(r));
        let mut dict = Dict::new();
        let data = match index % 8 {
            0 => noise(3_000, index),
            1 => Vec::new(),
            2 => {
                dict.insert(filter, Object::Name(hex));
                b"48656c6c6f2c20776f726c64>".to_vec()
            }
            3 => {
                dict.insert(Name::TYPE, Object::Name(metadata));
                dict.insert(subtype, Object::Name(xml));
                b"<?xpacket begin=\"\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/><?xpacket end=\"w\"?>"
                    .to_vec()
            }
            _ => compressible(1_500 + 400 * index as usize, index),
        };
        editor.put_stream(r, StreamData { dict, data });
    }
    let first = editor.page_refs()[0];
    let Some(Object::Dict(mut page)) = editor.get(first) else {
        panic!("the first page is a dictionary");
    };
    page.insert(editor.intern(b"TestStreams"), Object::Array(named));
    editor.put(first, Object::Dict(page));
}

/// The password a sealed save is written under.
const PASSWORD: &str = "open-sesame";

fn encryption() -> Option<Encryption> {
    Some(Encryption {
        user_password: PASSWORD.to_string(),
        owner_password: "the-owner".to_string(),
        permissions: -4,
        entropy: entropy(),
    })
}

/// One shape of save: its name, how to make the editor it writes, and the
/// options it writes with, every one compressing.
struct Shape {
    name: &'static str,
    editor: fn() -> DocumentEditor,
    options: WriteOptions,
    password: Option<&'static str>,
}

/// An editor over the source, with the streams added.
fn plain_editor() -> DocumentEditor {
    let mut editor = DocumentEditor::new(source());
    with_streams(&mut editor);
    editor
}

/// An editor over the source saved encrypted and opened again with its
/// password, with the streams added: what an incremental update appends to
/// under the key the file was opened with.
fn sealed_editor() -> DocumentEditor {
    let sealed = DocumentEditor::new(source()).save(&WriteOptions {
        encryption: encryption(),
        ..WriteOptions::default()
    });
    let doc = CosDocument::open(sealed).expect("the sealed source opens");
    assert_eq!(doc.authenticate(PASSWORD), Ok(AuthLevel::User));
    let mut editor = DocumentEditor::new(Arc::new(doc));
    with_streams(&mut editor);
    editor
}

fn shapes() -> Vec<Shape> {
    let compressing = WriteOptions {
        compress: true,
        ..WriteOptions::default()
    };
    let shape = |name, editor: fn() -> DocumentEditor, options: WriteOptions, password| Shape {
        name,
        editor,
        options,
        password,
    };
    vec![
        shape("a rewrite", plain_editor, compressing.clone(), None),
        shape(
            "an encrypted rewrite",
            plain_editor,
            WriteOptions {
                encryption: encryption(),
                ..compressing.clone()
            },
            Some(PASSWORD),
        ),
        shape(
            "a rewrite packing objects",
            plain_editor,
            WriteOptions {
                object_streams: true,
                ..compressing.clone()
            },
            None,
        ),
        shape(
            "an encrypted rewrite packing objects",
            plain_editor,
            WriteOptions {
                object_streams: true,
                encryption: encryption(),
                ..compressing.clone()
            },
            Some(PASSWORD),
        ),
        shape(
            "a linearized rewrite",
            plain_editor,
            WriteOptions {
                linearize: true,
                ..compressing.clone()
            },
            None,
        ),
        shape(
            "an encrypted linearized rewrite",
            plain_editor,
            WriteOptions {
                linearize: true,
                encryption: encryption(),
                ..compressing.clone()
            },
            Some(PASSWORD),
        ),
        shape(
            "an incremental update",
            plain_editor,
            WriteOptions {
                mode: WriteMode::Incremental,
                ..compressing.clone()
            },
            None,
        ),
        shape(
            "an incremental update under the inherited key",
            sealed_editor,
            WriteOptions {
                mode: WriteMode::Incremental,
                ..compressing.clone()
            },
            Some(PASSWORD),
        ),
        shape(
            "a rewrite collecting garbage and merging streams",
            plain_editor,
            WriteOptions {
                garbage_collect: true,
                deduplicate_streams: true,
                ..compressing
            },
            None,
        ),
    ]
}

/// **Saves on 2, 3, 4 and 7 threads are the serial bytes.** Every shape —
/// a rewrite, one packing objects into a stream, a linearized one, each
/// encrypted and not, an incremental update to a plain file and to one
/// sealed under its own key, and a rewrite that collects garbage and merges
/// streams — compressing forty-one streams of every kind, saved serially and
/// on the pool: the files are one, and each reopens to its twelve pages, its
/// password opening the sealed ones. The serial file is smaller than the
/// same save uncompressed, so there was compression for the pool to do.
#[test]
fn saves_on_2_3_4_and_7_threads_are_the_serial_bytes() {
    for shape in shapes() {
        let serial = (shape.editor)().save(&shape.options);
        let uncompressed = (shape.editor)().save(&WriteOptions {
            compress: false,
            ..shape.options.clone()
        });
        assert!(
            serial.len() < uncompressed.len(),
            "{}: compression shrank the file, {} bytes against {}",
            shape.name,
            serial.len(),
            uncompressed.len()
        );
        for threads in [2usize, 3, 4, 7] {
            let pooled = (shape.editor)().save(&WriteOptions {
                parallelism: Parallelism::Threads(threads),
                ..shape.options.clone()
            });
            assert!(
                pooled == serial,
                "{} on {threads} threads: {} bytes against the serial {}",
                shape.name,
                pooled.len(),
                serial.len()
            );
        }
        let reopened = CosDocument::open(serial).expect("the save reopens");
        if let Some(password) = shape.password {
            assert_eq!(
                reopened.authenticate(password),
                Ok(AuthLevel::User),
                "{}",
                shape.name
            );
        }
        assert_eq!(pages::collect(&reopened).len(), 12, "{}", shape.name);
    }
}

/// A save asked for no threads, and one asked for threads that compresses
/// nothing, write the bytes the default writes.
#[test]
fn a_serial_or_uncompressed_save_is_the_default_writer() {
    let default = plain_editor().save(&WriteOptions::default());
    for parallelism in [
        Parallelism::Serial,
        Parallelism::Threads(0),
        Parallelism::Threads(4),
    ] {
        let saved = plain_editor().save(&WriteOptions {
            parallelism,
            ..WriteOptions::default()
        });
        assert!(saved == default, "{parallelism:?}, compressing nothing");
    }
}
