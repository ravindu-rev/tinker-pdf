//! The write-parity scripts, run against the facade itself (gap 32).
//!
//! ```text
//! cargo run -p tinker-pdf --example write_parity -- testdata/form-fields.pdf
//! ```
//!
//! This is the **reference surface**. The bindings run the same scripts --
//! `bindings/python/tests/write_parity.py`,
//! `bindings/js/tests/write_parity.mjs`, the write leg of
//! `bindings/dotnet/tests/Smoke`, and the parity programs of the Go, Java and
//! Ruby bindings -- and `cargo xtask bindings-parity` requires every surface
//! to print the same hashes. Ruling 11 is what makes that the right test: a
//! binding projects the facade 1:1 and adds no logic, so surfaces disagreeing
//! means one of them added something.
//!
//! Two scripts write, both with every input pinned:
//!
//! - **fill-and-save** opens the committed form fixture, fills its damaged
//!   field (one `SkippedWidget`, which must be *reported* rather than
//!   flattened into failure), fills its undamaged control field (no skipped
//!   widgets, which is what keeps "the report was non-empty" distinguishable
//!   from "the report is always non-empty"), ticks a checkbox whose on state
//!   is `/On` rather than `/Yes`, selects a radio option, and saves
//!   incrementally (7.5.6, so the original bytes survive as a prefix);
//! - **build-a-document** registers a base font and an image, draws two pages
//!   through `begin_page`/`push_page`, sets `/Info` and an outline, and
//!   finishes.
//!
//! The image is computed from a formula rather than read from a file, so four
//! languages produce the same 64 bytes with no fixture between them -- a
//! parity suite whose surfaces read the same *file* proves they can read a
//! file.
//!
//! A third script reads rather than writes:
//!
//! - **read-surface** opens two documents -- the outline fixture with five
//!   bytes in front of its header, which the reader tolerates and reports as a
//!   warning, and a two-page document it builds with links, an outline and
//!   `/Info` -- and writes down everything the read surface says about each:
//!   the version, the page count, every `/Info` entry, `/Trapped`, the page
//!   labels, the outline flattened with its destinations, every link with its
//!   action, every attachment with a hash of its bytes, the XMP packet's hash,
//!   and, last, every warning. It prints `READ sha256=` of that text.
//!
//! The text is the contract, so it is written down once, here, and every
//! surface reproduces it byte for byte. Each line is space-separated tokens
//! ending in a line feed; a string is `s:` and the lower-case hex of its
//! UTF-8, a byte string `b:` and its hex, a number `f:` and the sixteen hex
//! digits of its IEEE 754 bits, and an absent value `-`. Hex rather than
//! quoting because every language escapes differently; IEEE bits rather than
//! decimals because every language formats `1.5` and `1e21` differently, and a
//! parity check that tolerated "close" in a coordinate would be measuring the
//! formatters rather than the engine.
//!
//! ```text
//! document <name>
//! version <s>
//! pages <count>
//! info <key> <s|->     for title author subject keywords creator producer
//!                      creation-date modification-date, in that order
//! trapped <absent|true|false|unknown>
//! label <page> <s>     one per page, or none when the document has no labels
//! outline <depth> <open 0|1> <title s> <dest>
//! link <page> <x0 f> <y0 f> <x1 f> <y1 f> <num.gen|-> <action>
//! attachment <name s> <filename s> <description s|-> <size|-> <sha256|->
//! xmp <sha256|->
//! warning <offset> <num.gen|-> <kind> <message s>
//!
//! dest   = - | explicit <page|-> <num.gen|-> <view> | named <b> | uri <b>
//! view   = xyz <f|-> <f|-> <f|-> | fit | fith <f|-> | fitv <f|->
//!        | fitr <f> <f> <f> <f> | fitb | fitbh <f|-> | fitbv <f|->
//! action = - | goto <dest> | gotor <b|-> <dest> | uri <b> | named <b>
//!        | launch <b|-> | other <b>
//! ```
//!
//! An attachment's hash is of its decoded bytes, `-` when it names no stream
//! or the stream does not read. The warnings are read last on purpose:
//! reading a page can tolerate more, so the order of the reads is part of the
//! contract. `TINKER_PARITY_DUMP=1` prints the text itself, which is how a
//! disagreement is found once the hashes say there is one.
//!
//! Every written artefact is put through this engine's strict structural
//! validator before its hash is printed. Under ruling 13 that check is
//! first-party, which is exactly why it can be a gate here rather than an
//! external step that might be skipped: byte-identical outputs agreeing tells
//! you nothing if all of them are wrong, and this is what rules that out.

use std::path::PathBuf;

use tinker_pdf::{
    DestKind, Document, DocumentBuilder, ImageData, OutlineEntry, OutlineItem, Target, Trapped,
    WriteMode, WriteOptions,
};

/// The eight-by-eight grey image both the Rust and the binding scripts build,
/// from the same formula.
fn parity_image() -> Vec<u8> {
    (0..64u32).map(|i| ((i * 7) % 256) as u8).collect()
}

/// Hex of a SHA-256, the one hash this repository already owns.
fn sha256_hex(bytes: &[u8]) -> String {
    tinker_pdf_crypto::sha2::sha256(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Script one: open a form, fill it, save incrementally.
fn fill_and_save(fixture: &[u8]) -> Vec<u8> {
    let document = Document::open(fixture.to_vec()).expect("the form fixture opens");
    let mut editor = document.editor();

    let skipped = editor
        .fill_field("name", "Ada Lovelace")
        .expect("the value is taken -- ruling 2 degrades rather than failing");
    assert_eq!(
        skipped.len(),
        1,
        "the fixture's /Rect-less widget must be reported, not swallowed"
    );
    assert_eq!(skipped[0].to_string(), "7 0 R: no usable /Rect (12.5.2)");

    let clean = editor
        .fill_field("notes", "every surface writes this")
        .expect("the value is taken");
    assert!(
        clean.is_empty(),
        "the control field's widget is well formed, so nothing is skipped"
    );

    assert!(editor.set_checkbox("agree", true));
    assert!(editor.select_radio("colour", "red"));

    editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    })
}

/// Script two: build a document from pages, a font and an image.
fn build_a_document() -> Vec<u8> {
    let samples = parity_image();
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    assert!(builder.add_image(
        b"Im1",
        &ImageData::Gray8 {
            width: 8,
            height: 8,
            data: &samples,
        }
    ));

    let mut one = builder.begin_page(200.0, 200.0);
    one.text(b"F1", 14.0, 20.0, 170.0, "Page one");
    one.fill_rect(20.0, 40.0, 60.0, 60.0, 0.25);
    one.image(b"Im1", 100.0, 40.0, 60.0, 60.0);
    builder.push_page(one);

    let mut two = builder.begin_page(200.0, 200.0);
    two.text(b"F1", 14.0, 20.0, 170.0, "Page two");
    builder.push_page(two);

    builder.set_info(b"Title", "tinker-pdf write parity");
    assert!(builder.set_outline(
        [(0u32, "Page one"), (1, "Page two")]
            .into_iter()
            .map(|(index, title)| OutlineEntry {
                title: title.to_string(),
                target: Some(Target::Page {
                    index,
                    view: DestKind::Fit,
                }),
                open: false,
                children: Vec::new(),
            })
            .collect()
    ));
    builder.finish()
}

/// Script three's second document: links, an outline and `/Info`, built.
///
/// Only what every surface can already write goes in -- a URI link, a page
/// link with a view whose top is `null` and a rectangle with fractions in it,
/// an outline with a heading that points nowhere and a child that does, an
/// `/Info` title outside ASCII and an author that is empty rather than absent
/// -- so a surface reads back what it wrote.
fn linked_document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    let mut one = builder.begin_page(200.0, 200.0);
    one.text(b"F1", 12.0, 20.0, 170.0, "Links");
    assert!(one.link(
        10.0,
        10.0,
        60.0,
        30.0,
        &Target::Uri("https://example.org/parity".to_string())
    ));
    assert!(one.link(
        70.0,
        10.0,
        120.5,
        30.25,
        &Target::Page {
            index: 1,
            view: DestKind::Xyz {
                left: Some(10.0),
                top: None,
                zoom: Some(1.5),
            },
        }
    ));
    builder.push_page(one);
    let two = builder.begin_page(200.0, 200.0);
    builder.push_page(two);
    builder.set_info(b"Title", "Read surface \u{2014} parity");
    builder.set_info(b"Author", "");
    assert!(builder.set_outline(vec![
        OutlineEntry {
            title: "Part one".to_string(),
            target: None,
            open: true,
            children: vec![OutlineEntry {
                title: "Chapter one".to_string(),
                target: Some(Target::Page {
                    index: 1,
                    view: DestKind::FitH { top: Some(150.0) },
                }),
                open: false,
                children: Vec::new(),
            }],
        },
        OutlineEntry {
            title: "Elsewhere".to_string(),
            target: Some(Target::Uri("https://example.org/".to_string())),
            open: false,
            children: Vec::new(),
        },
    ]));
    builder.finish()
}

/// The canonical spelling of each token, as the module documentation gives it.
mod dump {
    use super::sha256_hex;
    use tinker_pdf::{Action, DestKind, Destination, ObjRef};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn text(value: Option<&str>) -> String {
        value.map_or_else(|| "-".to_string(), |v| format!("s:{}", hex(v.as_bytes())))
    }

    pub fn bytes(value: Option<&[u8]>) -> String {
        value.map_or_else(|| "-".to_string(), |v| format!("b:{}", hex(v)))
    }

    pub fn number(value: Option<f64>) -> String {
        value.map_or_else(|| "-".to_string(), |v| format!("f:{:016x}", v.to_bits()))
    }

    pub fn reference(value: Option<ObjRef>) -> String {
        value.map_or_else(|| "-".to_string(), |r| format!("{}.{}", r.num, r.gen))
    }

    pub fn digest(value: Option<&[u8]>) -> String {
        value.map_or_else(|| "-".to_string(), sha256_hex)
    }

    fn view(kind: &DestKind) -> String {
        match *kind {
            DestKind::Xyz { left, top, zoom } => {
                format!("xyz {} {} {}", number(left), number(top), number(zoom))
            }
            DestKind::Fit => "fit".to_string(),
            DestKind::FitH { top } => format!("fith {}", number(top)),
            DestKind::FitV { left } => format!("fitv {}", number(left)),
            DestKind::FitR {
                left,
                bottom,
                right,
                top,
            } => format!(
                "fitr {} {} {} {}",
                number(Some(left)),
                number(Some(bottom)),
                number(Some(right)),
                number(Some(top))
            ),
            DestKind::FitB => "fitb".to_string(),
            DestKind::FitBH { top } => format!("fitbh {}", number(top)),
            DestKind::FitBV { left } => format!("fitbv {}", number(left)),
        }
    }

    pub fn destination(value: Option<&Destination>) -> String {
        match value {
            None => "-".to_string(),
            Some(Destination::Explicit {
                page_index,
                page_ref,
                kind,
            }) => format!(
                "explicit {} {} {}",
                page_index.map_or_else(|| "-".to_string(), |i| i.to_string()),
                reference(*page_ref),
                view(kind)
            ),
            Some(Destination::Named(name)) => format!("named {}", bytes(Some(name))),
            Some(Destination::Uri(uri)) => format!("uri {}", bytes(Some(uri))),
        }
    }

    pub fn action(value: Option<&Action>) -> String {
        match value {
            None => "-".to_string(),
            Some(Action::GoTo(dest)) => format!("goto {}", destination(Some(dest))),
            Some(Action::GoToR { file, dest }) => format!(
                "gotor {} {}",
                bytes(file.as_deref()),
                destination(dest.as_ref())
            ),
            Some(Action::Uri(uri)) => format!("uri {}", bytes(Some(uri))),
            Some(Action::Named(name)) => format!("named {}", bytes(Some(name))),
            Some(Action::Launch { file }) => format!("launch {}", bytes(file.as_deref())),
            Some(Action::Other { subtype }) => format!("other {}", bytes(Some(subtype))),
        }
    }
}

/// Everything the read surface says about one document, in the contract's
/// order.
fn read_dump(name: &str, document: &Document, out: &mut Vec<String>) {
    out.push(format!("document {name}"));
    out.push(format!(
        "version {}",
        dump::text(Some(&document.pdf_version()))
    ));
    out.push(format!("pages {}", document.page_count()));
    let metadata = document.metadata();
    for (key, value) in [
        ("title", &metadata.title),
        ("author", &metadata.author),
        ("subject", &metadata.subject),
        ("keywords", &metadata.keywords),
        ("creator", &metadata.creator),
        ("producer", &metadata.producer),
        ("creation-date", &metadata.creation_date),
        ("modification-date", &metadata.modification_date),
    ] {
        out.push(format!("info {key} {}", dump::text(value.as_deref())));
    }
    out.push(format!(
        "trapped {}",
        match metadata.trapped {
            None => "absent",
            Some(Trapped::True) => "true",
            Some(Trapped::False) => "false",
            Some(Trapped::Unknown) => "unknown",
        }
    ));
    for (index, label) in document.page_labels().iter().enumerate() {
        out.push(format!("label {index} {}", dump::text(Some(label))));
    }
    for (depth, item) in OutlineItem::flatten(&document.outline()) {
        out.push(format!(
            "outline {depth} {} {} {}",
            u8::from(item.open),
            dump::text(Some(&item.title)),
            dump::destination(item.destination.as_ref())
        ));
    }
    for index in 0..document.page_count() {
        let Some(page) = document.page(index) else {
            continue;
        };
        for link in page.links() {
            out.push(format!(
                "link {index} {} {} {} {} {} {}",
                dump::number(Some(link.rect.x0)),
                dump::number(Some(link.rect.y0)),
                dump::number(Some(link.rect.x1)),
                dump::number(Some(link.rect.y1)),
                dump::reference(link.reference),
                dump::action(link.target.as_ref())
            ));
        }
    }
    for attachment in document.attachments() {
        let data = attachment
            .stream
            .and_then(|stream| document.cos().stream_decoded(stream).ok());
        out.push(format!(
            "attachment {} {} {} {} {}",
            dump::text(Some(&attachment.name)),
            dump::text(Some(&attachment.filename)),
            dump::text(attachment.description.as_deref()),
            attachment
                .size
                .map_or_else(|| "-".to_string(), |s| s.to_string()),
            dump::digest(data.as_deref())
        ));
    }
    out.push(format!(
        "xmp {}",
        dump::digest(document.xmp_metadata().as_deref())
    ));
    for warning in document.warnings() {
        out.push(format!(
            "warning {} {} {} {}",
            warning.offset,
            dump::reference(warning.object),
            warning.kind.as_str(),
            dump::text(Some(&warning.kind.to_string()))
        ));
    }
}

/// Script three: everything the read surface says about two documents.
fn read_surface(outline_fixture: &[u8]) -> String {
    let mut shifted = b"JUNK\n".to_vec();
    shifted.extend_from_slice(outline_fixture);
    let mut lines = Vec::new();
    let document = Document::open(shifted).expect("a shifted header is tolerated");
    read_dump("shifted", &document, &mut lines);
    let document = Document::open(linked_document()).expect("the built document opens");
    read_dump("linked", &document, &mut lines);
    let mut text = String::new();
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// Validates, then prints the line `cargo xtask bindings-parity` reads.
///
/// The validation is not decoration and not optional. A surface that printed a
/// hash without it would be claiming agreement about bytes nobody checked were
/// a document.
fn report(script: &str, bytes: &[u8]) {
    let document = Document::open(bytes.to_vec())
        .unwrap_or_else(|e| panic!("{script}: the artefact does not reopen: {e}"));
    let defects = document.validate();
    assert!(
        defects.is_empty(),
        "{script}: the artefact does not pass the strict validator: {:?}",
        defects.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>()
    );
    println!(
        "WROTE sha256={} surface=facade script={script} bytes={}",
        sha256_hex(bytes),
        bytes.len()
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let fixture = args.next().map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: write_parity <form-fields.pdf>");
        std::process::exit(2);
    });
    let bytes =
        std::fs::read(&fixture).unwrap_or_else(|e| panic!("reading {}: {e}", fixture.display()));
    // The outline fixture sits beside the form one in `testdata/`, so the
    // command line stays the one argument every surface already takes.
    let outline_path = fixture.with_file_name("outline-3level.pdf");
    let outline = std::fs::read(&outline_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", outline_path.display()));

    report("fill-and-save", &fill_and_save(&bytes));
    report("build-a-document", &build_a_document());

    let dumped = read_surface(&outline);
    if std::env::var_os("TINKER_PARITY_DUMP").is_some() {
        print!("{dumped}");
    }
    println!(
        "READ sha256={} surface=facade script=read-surface bytes={}",
        sha256_hex(dumped.as_bytes()),
        dumped.len()
    );
    println!("FACADE-PARITY: RAN");
}
