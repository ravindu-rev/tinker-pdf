//! The command stream on the C ABI is pinned by equality with the facade:
//! every record the cursor reads, every field as plain values and every table
//! an accessor hands back is the record `CommandStream::records` holds, and
//! the commands read, pushed back through the encoder, are the facade's
//! bytes.

use std::collections::BTreeSet;
use std::ffi::{c_int, CStr};
use std::ptr;

use super::*;
use crate::{
    tpdf_bitmap_data, tpdf_bitmap_free, tpdf_bitmap_height, tpdf_bitmap_width, tpdf_buffer_data,
    tpdf_buffer_free, tpdf_document_free, tpdf_document_open, tpdf_last_error_message,
    tpdf_page_render, tpdf_page_size,
};
use tinker_pdf::{Document, COMMAND_STREAM_MAJOR, COMMAND_STREAM_MINOR};

/// Liberation Serif, the face the page's text is embedded in.
const SERIF: &[u8] =
    include_bytes!("../../../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf");

/// An RGB profile on gamma curves, from the profiles the ICC fuzz target's
/// corpus commits: the space of the page's shading.
const RGB_PROFILE: &[u8] =
    include_bytes!("../../../../fuzz/corpus/icc_profile/rgb-gamma-curve.icc");

fn last_error() -> String {
    let pointer = unsafe { tpdf_last_error_message() };
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

/// Asserts a call was refused as a bad argument whose message holds `name`.
#[track_caller]
fn refused_naming(status: TpdfStatus, name: &str, what: &str) {
    assert_eq!(status, TpdfStatus::BadArgument, "{what}");
    let message = last_error();
    assert!(
        message.contains(name),
        "{what}: the message names {name}: {message:?}"
    );
}

/// A stream object of `dict` holding `data`.
fn stream_object(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A file of `objects`, numbered from 1, the first the catalog.
fn file(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A two by two grey inline image, drawn into the unit square of `at`.
fn inline_image(at: &str) -> Vec<u8> {
    let mut out = format!("q {at} cm BI /W 2 /H 2 /CS /G /BPC 8 ID ").into_bytes();
    out.extend_from_slice(&[0x00, 0x55, 0xaa, 0xff]);
    out.extend_from_slice(b" EI Q\n");
    out
}

/// A 200 x 160 page whose stream holds every kind of record a page's
/// stream is written with:
///
/// - a state with stored parameters (`/RI`), a black generation and a
///   transfer function and a halftone (`Stored`, `Function`, `Halftone`),
///   stroking a line and filling a box;
/// - a clip and an axial shading in an ICC space (`ClipPath`, `DrawShading`,
///   `Profile`, `ColorSpace`, `ShadingDefinition`, `Shading`);
/// - a fill in a `/Separation` space (`Source`), and one in a Lab space whose
///   range is written backwards, which is read with a repair
///   (`RepairedSpace`);
/// - text in an embedded face (`BeginText`, `GlyphRun`, `EndText`,
///   `Outline`, `FontIdentity`) and in a font no name resolves
///   (`MissingFont`);
/// - an image XObject and an inline image (`DrawImage`, `Image`,
///   `InlineImage`, `ImageDecode`);
/// - marked content, a transparency group under a luminosity soft mask then
///   cleared (`BeginMarkedContent`, `BeginForm`, `BeginGroup`,
///   `BeginSoftMask`, `ClearSoftMask` and their ends);
/// - a tiling pattern (`Pattern`, `Cell`), a form with resources of its own
///   (`Scope`), a form that draws itself (`RecursionRefused`), and a square
///   annotation with an appearance (`Annotation`).
///
/// The three kinds it does not hold are named where they are pushed by hand
/// ([`the_command_stream_round_trips_through_the_c_abi`]).
fn every_kind_page() -> Vec<u8> {
    let widths = vec!["500"; 95].join(" ");
    let mut content = b"q /Fancy gs 0 0 1 RG 2 w 10 10 m 100 10 l S 1 0 0 rg 10 20 30 20 re f Q\n\
          q 10 40 40 30 re W n /Sh0 sh Q\n\
          q /CS0 cs 0.7 scn 60 20 20 20 re f Q\n\
          q /Lab0 cs 50 10 10 sc 90 20 10 10 re f Q\n\
          BT /F0 12 Tf 10 120 Td (Glyphs) Tj ET\n\
          BT /Nope 12 Tf 10 140 Td (x) Tj ET\n\
          q 20 0 0 20 120 20 cm /Im0 Do Q\n"
        .to_vec();
    content.extend(inline_image("10 0 0 10 150 20"));
    content.extend_from_slice(
        b"/Span << /MCID 0 >> BDC 0 g 120 60 5 5 re f EMC\n\
          q /Masked gs 1 0 0 1 120 80 cm /G Do /NoMask gs 0 1 0 rg 0 0 5 5 re f Q\n\
          q /Pattern cs /P0 scn 150 60 30 30 re f Q\n\
          q 1 0 0 1 150 100 cm /Fm0 Do Q\n\
          /Rec Do\n",
    );
    let rgb: Vec<u8> = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0];
    file(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] /Resources 4 0 R \
          /Contents 5 0 R /Annots [17 0 R] >>"
            .to_vec(),
        b"<< /Font << /F0 6 0 R >> \
          /XObject << /Im0 9 0 R /G 10 0 R /Fm0 11 0 R /Rec 12 0 R >> \
          /Shading << /Sh0 13 0 R >> /Pattern << /P0 15 0 R >> \
          /ColorSpace << \
            /CS0 [/Separation /Spot /DeviceRGB \
                  << /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>] \
            /Lab0 [/Lab << /WhitePoint [0.9505 1 1.089] /Range [100 -100 -100 100] >>] >> \
          /ExtGState << \
            /Fancy << /RI /Saturation \
                      /BG << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
                      /TR << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >> \
                      /HT << /Type /Halftone /HalftoneType 1 /Frequency 60 /Angle 45 \
                             /SpotFunction /Round >> >> \
            /Masked << /SMask << /Type /Mask /S /Luminosity /G 16 0 R >> >> \
            /NoMask << /SMask /None >> >> >>"
            .to_vec(),
        stream_object("", &content),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 \
             /LastChar 126 /Widths [{widths}] /FontDescriptor 7 0 R /Encoding /WinAnsiEncoding >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 \
          /FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 \
          /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"
            .to_vec(),
        stream_object(&format!("/Length1 {}", SERIF.len()), SERIF),
        stream_object(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8",
            &rgb,
        ),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >>",
            b"0 0.8 0 rg 0 0 20 20 re f",
        ),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 40] \
             /Resources << /XObject << /ImF 9 0 R >> >>",
            b"q 10 0 0 10 0 0 cm /ImF Do Q",
        ),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] \
             /Resources << /XObject << /Rec 12 0 R >> >>",
            b"0 0 1 rg 0 0 2 2 re f /Rec Do",
        ),
        b"<< /ShadingType 2 /ColorSpace [/ICCBased 14 0 R] /Coords [10 0 50 0] \
          /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
          /Extend [true true] >>"
            .to_vec(),
        stream_object("/N 3", RGB_PROFILE),
        stream_object(
            "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 \
             /YStep 10 /Resources << >>",
            b"1 0 0 rg 0 0 5 5 re f",
        ),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 160] \
             /Group << /S /Transparency /CS /DeviceGray >>",
            b"0.5 g 0 0 200 160 re f",
        ),
        b"<< /Type /Annot /Subtype /Square /Rect [150 120 190 150] /AP << /N 18 0 R >> >>".to_vec(),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 30]",
            b"0 0 1 RG 2 w 1 1 38 28 re S",
        ),
    ])
}

fn open(bytes: &[u8]) -> *mut TpdfDocument {
    let mut doc = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_document_open(bytes.as_ptr(), bytes.len(), &mut doc) },
        TpdfStatus::Ok
    );
    doc
}

/// A buffer's bytes, copied, and the buffer freed.
fn taken(buffer: *mut TpdfBuffer) -> Vec<u8> {
    let mut len = 0;
    let data = unsafe { tpdf_buffer_data(buffer, &mut len) };
    let bytes = unsafe { values(data, len) }.to_vec();
    unsafe { tpdf_buffer_free(buffer) };
    bytes
}

/// Page `index`'s stream, through the C ABI.
fn page_stream(doc: *const TpdfDocument, index: u32) -> Vec<u8> {
    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_page_command_stream(doc, index, &mut buffer) },
        TpdfStatus::Ok
    );
    taken(buffer)
}

fn parse(bytes: &[u8]) -> *mut TpdfCommandStream {
    let mut stream = ptr::null_mut();
    let status = unsafe { tpdf_command_stream_parse(bytes.as_ptr(), bytes.len(), &mut stream) };
    assert_eq!(status, TpdfStatus::Ok, "{}", last_error());
    stream
}

/// A field read through the C ABI and copied out, reals by their bits.
#[derive(Clone, Debug, PartialEq)]
enum Owned {
    Nothing,
    Unsigned(u64),
    Signed(i64),
    Real(u64),
    Bytes(Vec<u8>),
    Reals(Vec<u64>),
    Naturals(Vec<u64>),
    Strings(Vec<Vec<u8>>),
}

/// A field with nothing in it, for a call to fill.
fn blank() -> TpdfCommandField {
    TpdfCommandField {
        tag: -1,
        natural: 0,
        integer: 0,
        real: 0.0,
        data: ptr::null(),
        count: 0,
    }
}

/// Field `field` of record `index`, through the C ABI.
fn read_field(stream: *const TpdfCommandStream, index: u32, field: u32) -> Owned {
    let mut out = blank();
    assert_eq!(
        unsafe { tpdf_command_stream_field(stream, index, field, &mut out) },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
    match out.tag {
        0 => Owned::Nothing,
        1 => Owned::Unsigned(out.natural),
        2 => Owned::Signed(out.integer),
        3 => Owned::Real(out.real.to_bits()),
        4 => Owned::Bytes(unsafe { values(out.data.cast::<u8>(), out.count) }.to_vec()),
        5 => Owned::Reals(
            unsafe { values(out.data.cast::<f64>(), out.count) }
                .iter()
                .map(|v| v.to_bits())
                .collect(),
        ),
        6 => Owned::Naturals(unsafe { values(out.data.cast::<u64>(), out.count) }.to_vec()),
        7 => Owned::Strings(
            unsafe { values(out.data.cast::<TpdfCommandBytes>(), out.count) }
                .iter()
                .map(|s| unsafe { values(s.data, s.len) }.to_vec())
                .collect(),
        ),
        other => panic!("a field of tag {other}"),
    }
}

/// Every record the cursor reads, each with its fields, and the end it
/// reads after them.
fn read_all(stream: *mut TpdfCommandStream) -> (Vec<(TpdfCommand, Vec<Owned>)>, TpdfCommand) {
    assert_eq!(
        unsafe { tpdf_command_stream_rewind(stream) },
        TpdfStatus::Ok
    );
    let total = unsafe { tpdf_command_stream_count(stream) };
    let mut records = Vec::new();
    loop {
        let mut command = TpdfCommand {
            kind: -2,
            number: 0,
            index: 0,
            field_count: 0,
        };
        assert_eq!(
            unsafe { tpdf_command_stream_next(stream, &mut command) },
            TpdfStatus::Ok
        );
        if command.index == total {
            return (records, command);
        }
        let fields = (0..command.field_count)
            .map(|field| read_field(stream, command.index, field))
            .collect();
        records.push((command, fields));
    }
}

/// The facade's field an owned read spells.
fn facade_field(owned: &Owned) -> CommandField {
    match owned {
        Owned::Nothing => CommandField::Nothing,
        Owned::Unsigned(v) => CommandField::Unsigned(*v),
        Owned::Signed(v) => CommandField::Signed(*v),
        Owned::Real(bits) => CommandField::Real(f64::from_bits(*bits)),
        Owned::Bytes(bytes) => CommandField::bytes(bytes),
        Owned::Reals(bits) => {
            CommandField::Reals(bits.iter().map(|b| f64::from_bits(*b)).collect())
        }
        Owned::Naturals(naturals) => CommandField::Naturals(naturals.clone()),
        Owned::Strings(strings) => CommandField::Strings(strings.clone()),
    }
}

/// Pushes a record of `kind` holding `fields` through the C ABI.
fn push(encoder: *mut TpdfCommandEncoder, kind: c_int, fields: &[Owned]) -> TpdfStatus {
    let reals: Vec<Vec<f64>> = fields
        .iter()
        .map(|field| match field {
            Owned::Reals(bits) => bits.iter().map(|b| f64::from_bits(*b)).collect(),
            _ => Vec::new(),
        })
        .collect();
    let strings: Vec<Vec<TpdfCommandBytes>> = fields
        .iter()
        .map(|field| match field {
            Owned::Strings(list) => list
                .iter()
                .map(|s| TpdfCommandBytes {
                    data: s.as_ptr(),
                    len: s.len(),
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    let plain: Vec<TpdfCommandField> = fields
        .iter()
        .enumerate()
        .map(|(at, field)| {
            let mut out = blank();
            match field {
                Owned::Nothing => out.tag = TpdfCommandFieldTag::Nothing as c_int,
                Owned::Unsigned(v) => {
                    out.tag = TpdfCommandFieldTag::Unsigned as c_int;
                    out.natural = *v;
                }
                Owned::Signed(v) => {
                    out.tag = TpdfCommandFieldTag::Signed as c_int;
                    out.integer = *v;
                }
                Owned::Real(bits) => {
                    out.tag = TpdfCommandFieldTag::Real as c_int;
                    out.real = f64::from_bits(*bits);
                }
                Owned::Bytes(bytes) => {
                    out.tag = TpdfCommandFieldTag::Bytes as c_int;
                    out.data = bytes.as_ptr().cast();
                    out.count = bytes.len();
                }
                Owned::Reals(_) => {
                    out.tag = TpdfCommandFieldTag::Reals as c_int;
                    out.data = reals[at].as_ptr().cast();
                    out.count = reals[at].len();
                }
                Owned::Naturals(naturals) => {
                    out.tag = TpdfCommandFieldTag::Naturals as c_int;
                    out.data = naturals.as_ptr().cast();
                    out.count = naturals.len();
                }
                Owned::Strings(_) => {
                    out.tag = TpdfCommandFieldTag::Strings as c_int;
                    out.data = strings[at].as_ptr().cast();
                    out.count = strings[at].len();
                }
            }
            out
        })
        .collect();
    unsafe { tpdf_command_encoder_push(encoder, kind, plain.as_ptr(), plain.len()) }
}

/// `records` pushed through a new encoder and finished.
fn encode(records: &[(TpdfCommand, Vec<Owned>)]) -> Vec<u8> {
    let mut encoder = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_encoder_new(&mut encoder) },
        TpdfStatus::Ok
    );
    for (command, fields) in records {
        assert_eq!(
            push(encoder, command.kind, fields),
            TpdfStatus::Ok,
            "record {}: {}",
            command.index,
            last_error()
        );
    }
    let mut buffer = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_encoder_finish(encoder, &mut buffer) },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
    unsafe { tpdf_command_encoder_free(encoder) };
    taken(buffer)
}

/// A command of `kind` holding `fields`, as the cursor would read it.
fn of_kind(kind: TpdfCommandKind, fields: Vec<Owned>) -> (TpdfCommand, Vec<Owned>) {
    let command = TpdfCommand {
        kind: kind as c_int,
        number: kind.kind().number(),
        index: 0,
        field_count: count(fields.len()),
    };
    (command, fields)
}

/// What a read says apart from where it stood: its kind, its number and its
/// fields.
fn spelled(records: &[(TpdfCommand, Vec<Owned>)]) -> Vec<(c_int, u32, Vec<Owned>)> {
    records
        .iter()
        .map(|(command, fields)| (command.kind, command.number, fields.clone()))
        .collect()
}

/// **The command stream round-trips through the C ABI** (RC-18.5): the
/// stream of a page holding every kind a page's stream is written with,
/// handed back by `tpdf_page_command_stream` as the facade writes it, reads
/// through the cursor and the fields as the facade's records — every kind,
/// number and field, the reals to the bit — and those commands, pushed back
/// through the encoder, are the facade's bytes.
///
/// The three kinds no page here writes are pushed by hand after the page's
/// own record, and that stream round-trips the same way: `Colorants`, which
/// a stream holds only where its recording composited colorants, which no
/// display list does; and the two budgets' notices, `GroupBudgetSpent` and
/// `NestedBudgetSpent`, which a page reaches only past two thousand groups
/// or thousands of nested runs. Between the two streams, every kind but the
/// end, which the cursor reads after each.
#[test]
fn the_command_stream_round_trips_through_the_c_abi() {
    let bytes = every_kind_page();
    let doc = open(&bytes);
    let written = page_stream(doc, 0);
    unsafe { tpdf_document_free(doc) };
    let facade = Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .command_stream();
    assert_eq!(written, facade, "the C ABI hands back the page's stream");

    let stream = parse(&written);
    let (records, end) = read_all(stream);
    let parsed = CommandStream::parse(&facade).expect("the stream reads");
    assert_eq!(records.len(), parsed.records().len(), "every record, once");
    for ((command, fields), record) in records.iter().zip(parsed.records()) {
        let kind = record
            .command_kind()
            .and_then(TpdfCommandKind::of)
            .map(|kind| kind as c_int);
        assert_eq!(
            (
                Some(command.kind),
                command.number,
                command.field_count as usize
            ),
            (kind, record.kind, record.fields.len()),
            "record {}",
            command.index
        );
        let read: Vec<CommandField> = fields.iter().map(facade_field).collect();
        assert_eq!(
            read, record.fields,
            "record {} of kind {}",
            command.index, record.kind
        );
    }
    assert_eq!(
        (end.kind, end.number, end.index, end.field_count),
        (TpdfCommandKind::End as c_int, 0, count(records.len()), 0),
        "the end, after the last record"
    );
    assert_eq!(encode(&records), facade, "the commands read, encoded back");
    unsafe { tpdf_command_stream_free(stream) };

    let on_the_page: BTreeSet<c_int> = records.iter().map(|(command, _)| command.kind).collect();
    let by_hand = [
        TpdfCommandKind::Colorants,
        TpdfCommandKind::GroupBudgetSpent,
        TpdfCommandKind::NestedBudgetSpent,
    ];
    for kind in TpdfCommandKind::ALL {
        if *kind != TpdfCommandKind::End && !by_hand.contains(kind) {
            assert!(
                on_the_page.contains(&(*kind as c_int)),
                "the page's stream holds a {kind:?} record"
            );
        }
    }

    let colorants = vec![
        Owned::Unsigned(1),
        Owned::Strings(vec![b"Spot".to_vec(), b"Other".to_vec()]),
        Owned::Bytes(vec![128, 64]),
        Owned::Naturals(vec![1]),
        Owned::Bytes(vec![0, 0, 0, 255]),
        Owned::Nothing,
        Owned::Bytes(vec![10, 20, 30, 40]),
    ];
    let hand = vec![
        records[0].clone(),
        of_kind(TpdfCommandKind::Colorants, colorants),
        of_kind(TpdfCommandKind::GroupBudgetSpent, Vec::new()),
        of_kind(TpdfCommandKind::NestedBudgetSpent, Vec::new()),
    ];
    let encoded = encode(&hand);
    let facade_records: Vec<CommandRecord> = hand
        .iter()
        .map(|(command, fields)| CommandRecord {
            kind: command.number,
            fields: fields.iter().map(facade_field).collect(),
        })
        .collect();
    let facade_hand = CommandStream::from_records(&facade_records)
        .expect("the facade reads the commands")
        .to_bytes();
    assert_eq!(encoded, facade_hand, "the commands pushed by hand");
    let stream = parse(&encoded);
    let (read_back, _) = read_all(stream);
    assert_eq!(spelled(&read_back), spelled(&hand), "read back as pushed");
    assert_eq!(encode(&read_back), encoded, "and encoded back");
    unsafe { tpdf_command_stream_free(stream) };
}

/// **The cursor ends on the end, stays there, and rewinds**: after the last
/// record it reads `End` at the number of records with no fields, as often
/// as it is asked, and a rewind puts it back on the page record; a record
/// read by its place is the cursor's read of it.
#[test]
fn the_cursor_ends_on_the_end_and_rewinds() {
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    unsafe { tpdf_document_free(doc) };
    let stream = parse(&bytes);
    let total = unsafe { tpdf_command_stream_count(stream) };
    assert!(total > 30, "a rich page holds many records: {total}");
    let (records, end) = read_all(stream);
    assert_eq!(records.len() as u32, total);
    for _ in 0..3 {
        let mut again = blank_command();
        assert_eq!(
            unsafe { tpdf_command_stream_next(stream, &mut again) },
            TpdfStatus::Ok
        );
        assert_eq!(again, end, "the end, again");
    }
    assert_eq!(
        unsafe { tpdf_command_stream_rewind(stream) },
        TpdfStatus::Ok
    );
    let mut first = blank_command();
    assert_eq!(
        unsafe { tpdf_command_stream_next(stream, &mut first) },
        TpdfStatus::Ok
    );
    assert_eq!(
        (first.kind, first.index),
        (TpdfCommandKind::Page as c_int, 0)
    );
    for (command, _) in &records {
        let mut read = blank_command();
        assert_eq!(
            unsafe { tpdf_command_stream_record(stream, command.index, &mut read) },
            TpdfStatus::Ok
        );
        assert_eq!(read, *command);
    }
    let mut past = blank_command();
    refused_naming(
        unsafe { tpdf_command_stream_record(stream, total, &mut past) },
        "no such record",
        "a record past the last",
    );
    unsafe { tpdf_command_stream_free(stream) };
}

fn blank_command() -> TpdfCommand {
    TpdfCommand {
        kind: -2,
        number: u32::MAX,
        index: u32::MAX,
        field_count: u32::MAX,
    }
}

/// **A field reads as plain values**: the page record's crop box is the
/// page's, its rotation and index are unsigned, and a field or a record past
/// the end is refused naming what was asked.
#[test]
fn a_field_reads_as_plain_values() {
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    let (mut width, mut height) = (0.0, 0.0);
    assert_eq!(
        unsafe { tpdf_page_size(doc, 0, &mut width, &mut height) },
        TpdfStatus::Ok
    );
    unsafe { tpdf_document_free(doc) };
    let stream = parse(&bytes);

    let crop = read_field(stream, 0, 0);
    assert_eq!(
        crop,
        Owned::Reals([0.0f64, 0.0, 200.0, 160.0].map(f64::to_bits).to_vec()),
        "the crop box"
    );
    assert_eq!(read_field(stream, 0, 1), Owned::Unsigned(0), "no rotation");
    assert_eq!(
        read_field(stream, 0, 4),
        Owned::Unsigned(0),
        "the first page"
    );
    assert_eq!((width, height), (200.0, 160.0));

    let mut out = blank();
    refused_naming(
        unsafe { tpdf_command_stream_field(stream, 0, 5, &mut out) },
        "no such field",
        "a field past the page record's five",
    );
    let total = unsafe { tpdf_command_stream_count(stream) };
    refused_naming(
        unsafe { tpdf_command_stream_field(stream, total, 0, &mut out) },
        "no such record",
        "a record past the last",
    );
    refused_naming(
        unsafe { tpdf_command_stream_field(stream, 0, 0, ptr::null_mut()) },
        "null",
        "no place to write",
    );
    refused_naming(
        unsafe { tpdf_command_stream_field(ptr::null(), 0, 0, &mut out) },
        "null",
        "no stream",
    );
    unsafe { tpdf_command_stream_free(stream) };
}

/// **The accessors hand back each outline and each decode as its fields
/// hold it**: every outline record's scope, font, code, found flag,
/// operators and numbers, and every image decode's answer, size, samples and
/// flags, are the record's own fields, borrowed; the page's glyphs have
/// outlines and its two images decodes. An accessor asked about a record of
/// another kind is refused naming both kinds.
#[test]
fn the_accessors_hand_back_each_outline_and_each_decode() {
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    unsafe { tpdf_document_free(doc) };
    let stream = parse(&bytes);
    let (records, _) = read_all(stream);
    let (mut outlines, mut drawn, mut images) = (0, 0, 0);
    for (command, fields) in &records {
        if command.kind == TpdfCommandKind::Outline as c_int {
            outlines += 1;
            let mut outline = TpdfCommandOutline {
                scope: u64::MAX,
                font_id: u64::MAX,
                code: u32::MAX,
                found: -1,
                operators: TpdfCommandBytes::NONE,
                numbers: ptr::null(),
                number_count: 0,
            };
            assert_eq!(
                unsafe { tpdf_command_stream_outline(stream, command.index, &mut outline) },
                TpdfStatus::Ok,
                "{}",
                last_error()
            );
            assert_eq!(
                (
                    Owned::Unsigned(outline.scope),
                    Owned::Unsigned(outline.font_id),
                    Owned::Unsigned(u64::from(outline.code)),
                    Owned::Unsigned(outline.found as u64),
                ),
                (
                    fields[0].clone(),
                    fields[1].clone(),
                    fields[2].clone(),
                    fields[3].clone()
                ),
                "outline {}",
                command.index
            );
            let operators = unsafe { values(outline.operators.data, outline.operators.len) };
            let numbers = unsafe { values(outline.numbers, outline.number_count) };
            assert_eq!(Owned::Bytes(operators.to_vec()), fields[4]);
            assert_eq!(
                Owned::Reals(numbers.iter().map(|v| v.to_bits()).collect()),
                fields[5]
            );
            if outline.found == 1 && !operators.is_empty() {
                drawn += 1;
            }
        }
        if command.kind == TpdfCommandKind::ImageDecode as c_int {
            let mut image = std::mem::MaybeUninit::<TpdfCommandImage>::zeroed();
            assert_eq!(
                unsafe { tpdf_command_stream_image(stream, command.index, image.as_mut_ptr()) },
                TpdfStatus::Ok,
                "{}",
                last_error()
            );
            let image = unsafe { image.assume_init() };
            assert_eq!(
                (
                    Owned::Unsigned(image.image),
                    Owned::Unsigned(u64::from(image.reduction))
                ),
                (fields[0].clone(), fields[1].clone())
            );
            if image.answer == TpdfImageAnswer::Image {
                images += 1;
                assert_eq!(fields[2], Owned::Unsigned(1));
                assert_eq!(Owned::Unsigned(u64::from(image.width)), fields[4]);
                assert_eq!(Owned::Unsigned(u64::from(image.height)), fields[5]);
                let rgb = unsafe { values(image.rgb.data, image.rgb.len) };
                let alpha = unsafe { values(image.alpha.data, image.alpha.len) };
                assert_eq!(Owned::Bytes(rgb.to_vec()), fields[6]);
                assert_eq!(Owned::Bytes(alpha.to_vec()), fields[7]);
                let (held_width, held_height) = if image.has_window == 1 {
                    (image.window_width, image.window_height)
                } else {
                    (image.width, image.height)
                };
                assert_eq!(
                    rgb.len(),
                    3 * held_width as usize * held_height as usize,
                    "three bytes a sample held"
                );
                assert_eq!(Owned::Unsigned(image.stencil as u64), fields[10]);
                assert_eq!(Owned::Unsigned(image.interpolate as u64), fields[11]);
            }
        }
    }
    assert!(outlines > 0 && drawn > 0, "the page's glyphs have outlines");
    assert!(
        images >= 2,
        "the image and the inline image decode: {images}"
    );

    let mut outline = std::mem::MaybeUninit::<TpdfCommandOutline>::zeroed();
    refused_naming(
        unsafe { tpdf_command_stream_outline(stream, 0, outline.as_mut_ptr()) },
        "record 0 is Page, not Outline",
        "the page record as an outline",
    );
    let mut image = std::mem::MaybeUninit::<TpdfCommandImage>::zeroed();
    refused_naming(
        unsafe { tpdf_command_stream_image(stream, 0, image.as_mut_ptr()) },
        "record 0 is Page, not ImageDecode",
        "the page record as a decode",
    );
    unsafe { tpdf_command_stream_free(stream) };
}

/// **The encoder refuses what its enums and the parse refuse**: a kind or a
/// tag its enum does not declare, the end pushed, null data with a count and
/// a null string are refused at the push, naming them, and appended nothing;
/// commands that are no stream — no page record — are refused at the finish
/// with the reader's sentence; and the same encoder, given a page record,
/// finishes, and finishes again to the same bytes.
#[test]
fn the_encoder_refuses_what_its_enums_and_the_parse_refuse() {
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    unsafe { tpdf_document_free(doc) };
    let stream = parse(&bytes);
    let (records, _) = read_all(stream);
    unsafe { tpdf_command_stream_free(stream) };
    let (page, page_fields) = &records[0];

    let mut encoder = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_encoder_new(&mut encoder) },
        TpdfStatus::Ok
    );
    let past = c_int::try_from(TpdfCommandKind::ALL.len()).expect("a handful");
    for kind in [-1, past, 64, c_int::MAX] {
        refused_naming(
            push(encoder, kind, &[]),
            "TpdfCommandKind",
            &format!("a push of kind {kind}"),
        );
    }
    refused_naming(
        push(encoder, TpdfCommandKind::End as c_int, &[]),
        "the end is no command to push",
        "the end pushed",
    );
    let mut wrong_tag = blank();
    wrong_tag.tag = 8;
    refused_naming(
        unsafe { tpdf_command_encoder_push(encoder, page.kind, &wrong_tag, 1) },
        "TpdfCommandFieldTag",
        "a field of tag 8",
    );
    let mut null_data = blank();
    null_data.tag = TpdfCommandFieldTag::Reals as c_int;
    null_data.count = 4;
    refused_naming(
        unsafe { tpdf_command_encoder_push(encoder, page.kind, &null_data, 1) },
        "its data is null",
        "reals with no data",
    );
    let strings = [TpdfCommandBytes {
        data: ptr::null(),
        len: 3,
    }];
    let mut null_string = blank();
    null_string.tag = TpdfCommandFieldTag::Strings as c_int;
    null_string.data = strings.as_ptr().cast();
    null_string.count = 1;
    refused_naming(
        unsafe { tpdf_command_encoder_push(encoder, page.kind, &null_string, 1) },
        "its string 0 is null",
        "a null string",
    );
    refused_naming(
        unsafe { tpdf_command_encoder_push(encoder, page.kind, ptr::null(), 2) },
        "the fields are null",
        "no fields with a count",
    );

    // Nothing was appended, so a finish now is a stream of no page.
    let mut buffer = ptr::null_mut();
    refused_naming(
        unsafe { tpdf_command_encoder_finish(encoder, &mut buffer) },
        "no page record",
        "a finish with nothing pushed",
    );
    assert!(buffer.is_null(), "and no bytes handed over");

    assert_eq!(push(encoder, page.kind, page_fields), TpdfStatus::Ok);
    let mut first = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_encoder_finish(encoder, &mut first) },
        TpdfStatus::Ok,
        "{}",
        last_error()
    );
    let mut second = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_encoder_finish(encoder, &mut second) },
        TpdfStatus::Ok
    );
    let first = taken(first);
    assert_eq!(first, taken(second), "a finish keeps the commands");
    let facade = CommandStream::from_records(&[CommandRecord {
        kind: page.number,
        fields: page_fields.iter().map(facade_field).collect(),
    }])
    .expect("a page record alone is a stream")
    .to_bytes();
    assert_eq!(first, facade);

    refused_naming(
        unsafe { tpdf_command_encoder_push(ptr::null_mut(), page.kind, ptr::null(), 0) },
        "null",
        "no encoder",
    );
    refused_naming(
        unsafe { tpdf_command_encoder_finish(encoder, ptr::null_mut()) },
        "null",
        "no place to write",
    );
    unsafe { tpdf_command_encoder_free(encoder) };
}

/// **Bytes the reader refuses are a bad argument naming why**: bytes that are
/// no command stream, a stream cut short and one with bytes after its end
/// are refused with the reader's sentence, and nothing is handed over; null
/// bytes and a null place to write are refused as null pointers.
#[test]
fn bytes_the_reader_refuses_are_a_bad_argument_naming_why() {
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    unsafe { tpdf_document_free(doc) };
    let refused = |bytes: &[u8], name: &str, what: &str| {
        let mut stream = ptr::null_mut();
        refused_naming(
            unsafe { tpdf_command_stream_parse(bytes.as_ptr(), bytes.len(), &mut stream) },
            name,
            what,
        );
        assert!(stream.is_null(), "{what}: nothing handed over");
    };
    refused(b"%PDF-1.7\n", "not a command stream", "a PDF");
    refused(&bytes[..bytes.len() - 1], "cut short", "a stream cut short");
    let mut running_on = bytes.clone();
    running_on.push(0);
    refused(&running_on, "bytes follow", "a stream running on");
    let mut newer = bytes.clone();
    newer[8..10].copy_from_slice(&(COMMAND_STREAM_MAJOR + 1).to_le_bytes());
    refused(&newer, "major version", "a newer major");

    let mut stream = ptr::null_mut();
    refused_naming(
        unsafe { tpdf_command_stream_parse(ptr::null(), 4, &mut stream) },
        "null",
        "no bytes",
    );
    refused_naming(
        unsafe { tpdf_command_stream_parse(bytes.as_ptr(), bytes.len(), ptr::null_mut()) },
        "null",
        "no place to write",
    );
}

/// **A stream says what its page is and draws it**: its version is this
/// build's, its page index and size the page's, its bytes written back the
/// bytes it was read from, and its render at 1.5 times in RGB the page's own
/// render through the C ABI, byte for byte; a format `TpdfPixelFormat` does
/// not declare is refused naming it.
#[test]
fn a_stream_says_what_its_page_is_and_draws_it() {
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    let stream = parse(&bytes);

    let (mut major, mut minor) = (0, 0);
    assert_eq!(
        unsafe { tpdf_command_stream_version(stream, &mut major, &mut minor) },
        TpdfStatus::Ok
    );
    assert_eq!((major, minor), (COMMAND_STREAM_MAJOR, COMMAND_STREAM_MINOR));
    let mut index = u32::MAX;
    assert_eq!(
        unsafe { tpdf_command_stream_page_index(stream, &mut index) },
        TpdfStatus::Ok
    );
    assert_eq!(index, 0);
    let (mut width, mut height) = (0.0, 0.0);
    assert_eq!(
        unsafe { tpdf_command_stream_size(stream, &mut width, &mut height) },
        TpdfStatus::Ok
    );
    let (mut page_width, mut page_height) = (0.0, 0.0);
    assert_eq!(
        unsafe { tpdf_page_size(doc, 0, &mut page_width, &mut page_height) },
        TpdfStatus::Ok
    );
    assert_eq!((width, height), (page_width, page_height));
    let mut written = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_stream_to_bytes(stream, &mut written) },
        TpdfStatus::Ok
    );
    assert_eq!(taken(written), bytes, "written back, the bytes read");

    let rgb = TpdfPixelFormat::Rgb8 as c_int;
    let (mut drawn, mut direct) = (ptr::null_mut(), ptr::null_mut());
    assert_eq!(
        unsafe { tpdf_command_stream_render(stream, 1.5, rgb, &mut drawn) },
        TpdfStatus::Ok
    );
    assert_eq!(
        unsafe { tpdf_page_render(doc, 0, 1.5, rgb, &mut direct) },
        TpdfStatus::Ok
    );
    assert_eq!(
        unsafe { (tpdf_bitmap_width(drawn), tpdf_bitmap_height(drawn)) },
        unsafe { (tpdf_bitmap_width(direct), tpdf_bitmap_height(direct)) }
    );
    let pixels = |bitmap: *const TpdfBitmap| {
        let mut len = 0;
        let data = unsafe { tpdf_bitmap_data(bitmap, &mut len) };
        unsafe { values(data, len) }.to_vec()
    };
    assert!(pixels(drawn) == pixels(direct), "the stream draws the page");
    unsafe { tpdf_bitmap_free(drawn) };
    unsafe { tpdf_bitmap_free(direct) };

    let mut refused = ptr::null_mut();
    refused_naming(
        unsafe { tpdf_command_stream_render(stream, 1.0, 4, &mut refused) },
        "TpdfPixelFormat",
        "a format of 4",
    );
    assert!(refused.is_null());
    unsafe { tpdf_command_stream_free(stream) };
    unsafe { tpdf_document_free(doc) };
}

/// **A newer minor's record reads as a kind this build does not know**: the
/// page's stream marked one minor newer with a record of kind 63 before its
/// end reads, the cursor gives that record as -1 with its number and no
/// fields, and the stream writes back to those bytes; the encoder, which
/// writes this build's version, takes no kind it does not declare.
#[test]
fn a_newer_minor_s_record_reads_as_a_kind_this_build_does_not_know() {
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    unsafe { tpdf_document_free(doc) };
    // The end is kind 0 and length 0, the stream's last two bytes; the new
    // record is kind 63 and length 0 before it.
    let mut newer = bytes[..bytes.len() - 2].to_vec();
    newer.extend_from_slice(&[63, 0, 0, 0]);
    newer[10..12].copy_from_slice(&(COMMAND_STREAM_MINOR + 1).to_le_bytes());

    let stream = parse(&newer);
    let total = unsafe { tpdf_command_stream_count(stream) };
    let mut last = blank_command();
    assert_eq!(
        unsafe { tpdf_command_stream_record(stream, total - 1, &mut last) },
        TpdfStatus::Ok
    );
    assert_eq!(
        (last.kind, last.number, last.field_count),
        (-1, 63, 0),
        "a kind this build does not know, by its number"
    );
    let mut written = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_stream_to_bytes(stream, &mut written) },
        TpdfStatus::Ok
    );
    assert_eq!(taken(written), newer, "written back at its own version");
    unsafe { tpdf_command_stream_free(stream) };

    let mut encoder = ptr::null_mut();
    assert_eq!(
        unsafe { tpdf_command_encoder_new(&mut encoder) },
        TpdfStatus::Ok
    );
    refused_naming(push(encoder, -1, &[]), "TpdfCommandKind", "a push of -1");
    unsafe { tpdf_command_encoder_free(encoder) };
}

/// **Null is refused on every entry point and accepted by every free**.
#[test]
fn null_is_refused_on_every_entry_point_and_freed_as_nothing() {
    let mut buffer = ptr::null_mut();
    refused_naming(
        unsafe { tpdf_page_command_stream(ptr::null(), 0, &mut buffer) },
        "null",
        "no document",
    );
    let doc = open(&every_kind_page());
    assert_eq!(
        unsafe { tpdf_page_command_stream(doc, 1, &mut buffer) },
        TpdfStatus::NoSuchPage,
        "a page past the last"
    );
    refused_naming(
        unsafe { tpdf_page_command_stream(doc, 0, ptr::null_mut()) },
        "null",
        "no place to write",
    );
    unsafe { tpdf_document_free(doc) };

    let null = ptr::null::<TpdfCommandStream>();
    let mut command = blank_command();
    let mut outline = std::mem::MaybeUninit::<TpdfCommandOutline>::zeroed();
    let mut image = std::mem::MaybeUninit::<TpdfCommandImage>::zeroed();
    let (mut major, mut minor, mut index) = (0, 0, 0);
    let (mut width, mut height) = (0.0, 0.0);
    let mut bitmap = ptr::null_mut();
    assert_eq!(unsafe { tpdf_command_stream_count(null) }, 0);
    let null_refused = |status: TpdfStatus, what: &str| refused_naming(status, "null", what);
    null_refused(
        unsafe { tpdf_command_stream_next(ptr::null_mut(), &mut command) },
        "next",
    );
    null_refused(
        unsafe { tpdf_command_stream_rewind(ptr::null_mut()) },
        "rewind",
    );
    null_refused(
        unsafe { tpdf_command_stream_record(null, 0, &mut command) },
        "record",
    );
    null_refused(
        unsafe { tpdf_command_stream_outline(null, 0, outline.as_mut_ptr()) },
        "outline",
    );
    null_refused(
        unsafe { tpdf_command_stream_image(null, 0, image.as_mut_ptr()) },
        "image",
    );
    null_refused(
        unsafe { tpdf_command_stream_to_bytes(null, &mut buffer) },
        "to_bytes",
    );
    null_refused(
        unsafe { tpdf_command_stream_version(null, &mut major, &mut minor) },
        "version",
    );
    null_refused(
        unsafe { tpdf_command_stream_page_index(null, &mut index) },
        "page_index",
    );
    null_refused(
        unsafe { tpdf_command_stream_size(null, &mut width, &mut height) },
        "size",
    );
    null_refused(
        unsafe { tpdf_command_stream_render(null, 1.0, 0, &mut bitmap) },
        "render",
    );
    null_refused(
        unsafe { tpdf_command_encoder_new(ptr::null_mut()) },
        "encoder_new",
    );
    null_refused(
        unsafe { tpdf_command_encoder_finish(ptr::null(), &mut buffer) },
        "encoder_finish",
    );

    // A live stream, with each out pointer null.
    let doc = open(&every_kind_page());
    let bytes = page_stream(doc, 0);
    unsafe { tpdf_document_free(doc) };
    let stream = parse(&bytes);
    null_refused(
        unsafe { tpdf_command_stream_next(stream, ptr::null_mut()) },
        "next into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_record(stream, 0, ptr::null_mut()) },
        "record into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_outline(stream, 0, ptr::null_mut()) },
        "outline into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_image(stream, 0, ptr::null_mut()) },
        "image into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_to_bytes(stream, ptr::null_mut()) },
        "to_bytes into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_version(stream, ptr::null_mut(), &mut minor) },
        "version into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_page_index(stream, ptr::null_mut()) },
        "page_index into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_size(stream, &mut width, ptr::null_mut()) },
        "size into nothing",
    );
    null_refused(
        unsafe { tpdf_command_stream_render(stream, 1.0, 0, ptr::null_mut()) },
        "render into nothing",
    );
    unsafe { tpdf_command_stream_free(stream) };
    unsafe { tpdf_command_stream_free(ptr::null_mut()) };
    unsafe { tpdf_command_encoder_free(ptr::null_mut()) };
}
