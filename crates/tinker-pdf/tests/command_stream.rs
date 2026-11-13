//! A page's command stream drawn with no document behind it
//! (`docs/design/command-stream.md`): what `CommandStream::render` draws is
//! what the page's own render draws, under every option a render reads, and
//! what each render of a stream reports is what that render met.
//!
//! `determinism.rs`'s `a_command_stream_replays_every_fingerprinted_page_byte_for_byte`
//! holds the replay to every fingerprinted page at four scales. The pages here
//! are the ones where the answer depends on more than the calls: a render's
//! options, a renderer that declines a group, a note the interpretation left
//! where only some renders reach it, the usage a stream was written for — and
//! the bytes a reader refuses.

use tinker_pdf::{
    Bitmap, CancelToken, CommandBudgets, CommandField, CommandKind, CommandRecord, CommandStream,
    CommandStreamError, Document, ForcedColours, HardEdge, Parallelism, PixelFormat, PixelRegion,
    RenderOptions, RenderWarning, Usage, COMMAND_STREAM_MAJOR, COMMAND_STREAM_MINOR,
};

mod render_support;
use render_support::{curvy_font, raw_pdf, raw_stream};

/// Liberation Serif, the face the option page's text is embedded in.
const SERIF: &[u8] =
    include_bytes!("../../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf");

/// Holds a stream's render to the page's render of the same options: the
/// same shape, pixels and warnings.
#[track_caller]
fn same(got: &Bitmap, want: &Bitmap, what: &str) {
    assert_eq!(
        (
            got.width,
            got.height,
            got.format,
            got.stride,
            got.premultiplied
        ),
        (
            want.width,
            want.height,
            want.format,
            want.stride,
            want.premultiplied
        ),
        "{what}: a different bitmap"
    );
    if got.data != want.data {
        let differing = got
            .data
            .iter()
            .zip(want.data.iter())
            .filter(|(a, b)| a != b)
            .count();
        panic!(
            "{what}: the stream is not the page -- {differing} of {} bytes differ",
            want.data.len()
        );
    }
    assert_eq!(
        got.warnings, want.warnings,
        "{what}: the stream reported differently"
    );
}

/// The first page of `bytes` and its command stream, read back.
fn page_and_stream(bytes: Vec<u8>) -> (Document, CommandStream) {
    let document = Document::open(bytes).expect("it opens");
    let stream = CommandStream::parse(&document.page(0).expect("a page").command_stream())
        .expect("the stream reads");
    (document, stream)
}

// ---- every option a render reads ------------------------------------------------

/// RGB samples of a `w` by `h` image, by formula.
fn rgb(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [
                (x * 255 / w) as u8,
                (y * 255 / h) as u8,
                ((x + y) * 40) as u8,
            ]
        })
        .collect()
}

/// A two by two grey inline image, drawn into the unit square of `at`.
fn inline_image(at: &str) -> Vec<u8> {
    let mut out = format!("q {at} cm BI /W 2 /H 2 /CS /G /BPC 8 ID ").into_bytes();
    out.extend_from_slice(&[0x00, 0x55, 0xaa, 0xff]);
    out.extend_from_slice(b" EI Q\n");
    out
}

/// A 120 x 90 page drawing what a render's options act on: text in an
/// embedded face, a dashed stroke and a hairline, a DeviceCMYK fill, an image
/// XObject and one inline image drawn twice, an axial shading, a tiling
/// pattern whose cell draws text from resources of its own, a transparency
/// group under a luminosity soft mask, and a square annotation whose
/// appearance strokes its border and draws the same inline image.
fn every_option_page() -> Vec<u8> {
    let widths = vec!["500"; 95].join(" ");
    let mut content = b"q 0 0 1 RG 1.5 w [3 2] 0 d 5 5 m 115 5 l S Q\n\
          q 0 w 5 8 m 115 12 l S Q\n\
          BT /F0 14 Tf 6 72 Td (Stream) Tj ET\n\
          q 0.1 0.6 0.2 0 k 72 62 18 18 re f Q\n\
          q 24 0 0 24 6 36 cm /Im0 Do Q\n\
          q 34 36 30 24 re W n /Sh0 sh Q\n\
          q /Pattern cs /P0 scn 68 30 46 26 re f Q\n\
          q /Masked gs 1 0 0 1 94 64 cm /G Do Q\n"
        .to_vec();
    content.extend(inline_image("8 0 0 8 36 16"));
    content.extend(inline_image("8 0 0 8 46 16"));
    let mut appearance = b"1 0 0 RG 2 w 1 1 28 13 re S\n".to_vec();
    appearance.extend(inline_image("6 0 0 6 20 4"));
    raw_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 90] /Resources 4 0 R \
          /Contents 5 0 R /Annots [14 0 R] >>"
            .to_vec(),
        b"<< /Font << /F0 6 0 R >> /XObject << /Im0 9 0 R /G 10 0 R >> \
          /Shading << /Sh0 11 0 R >> /Pattern << /P0 12 0 R >> \
          /ExtGState << /Masked << /SMask << /Type /Mask /S /Luminosity /G 13 0 R >> >> >> >>"
            .to_vec(),
        raw_stream("", &content),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 \
             /LastChar 126 /Widths [{widths}] /FontDescriptor 7 0 R /Encoding /WinAnsiEncoding >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 \
          /FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 \
          /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"
            .to_vec(),
        raw_stream(&format!("/Length1 {}", SERIF.len()), SERIF),
        raw_stream(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8",
            &rgb(4, 4),
        ),
        raw_stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >> \
             /Resources << /ExtGState << /Half << /ca 0.5 >> >> >>",
            b"/Half gs 0.8 0.1 0.1 rg 0 0 20 20 re f 0 0 0.8 rg 10 10 10 10 re f",
        ),
        b"<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [34 0 64 0] \
          /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0.5 0] /C1 [0 0.5 1] /N 1 >> \
          /Extend [true true] >>"
            .to_vec(),
        raw_stream(
            "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 12 12] /XStep 12 \
             /YStep 12 /Resources << /Font << /F0 6 0 R >> >>",
            b"0 0.5 0 rg 0 0 6 6 re f BT /F0 8 Tf 6 6 Td (a) Tj ET",
        ),
        raw_stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 120 90] \
             /Group << /S /Transparency /CS /DeviceGray >>",
            b"0.6 g 0 0 120 90 re f 1 g 98 68 8 8 re f",
        ),
        b"<< /Type /Annot /Subtype /Square /Rect [80 8 110 23] /AP << /N 15 0 R >> >>".to_vec(),
        raw_stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 30 15]",
            &appearance,
        ),
    ])
}

/// Every option a render reads, each by what it asks.
fn every_option() -> Vec<(&'static str, RenderOptions)> {
    let cancelled = CancelToken::new();
    cancelled.cancel();
    let base = RenderOptions::default;
    vec![
        ("the defaults", base()),
        (
            "half the size",
            RenderOptions {
                scale: 0.5,
                ..base()
            },
        ),
        (
            "2.25 times",
            RenderOptions {
                scale: 2.25,
                ..base()
            },
        ),
        (
            "a scale that is no number",
            RenderOptions {
                scale: f64::NAN,
                ..base()
            },
        ),
        (
            "a region",
            RenderOptions {
                scale: 1.5,
                region: Some(PixelRegion::new(13, 7, 61, 45)),
                ..base()
            },
        ),
        (
            "a region off the page",
            RenderOptions {
                region: Some(PixelRegion::new(500, 500, 10, 10)),
                ..base()
            },
        ),
        (
            "grey",
            RenderOptions {
                format: PixelFormat::Gray8,
                ..base()
            },
        ),
        (
            "grey and alpha on nothing",
            RenderOptions {
                format: PixelFormat::GrayA8,
                transparent: true,
                ..base()
            },
        ),
        (
            "RGBA on nothing, premultiplied",
            RenderOptions {
                format: PixelFormat::Rgba8,
                transparent: true,
                premultiplied: true,
                ..base()
            },
        ),
        (
            "ink",
            RenderOptions {
                format: PixelFormat::CmykA8,
                allow_cmyk: true,
                ..base()
            },
        ),
        (
            "no anti-aliasing",
            RenderOptions {
                antialias: false,
                ..base()
            },
        ),
        (
            "no anti-aliasing, by the centres",
            RenderOptions {
                antialias: false,
                hard_edge: HardEdge::Centre,
                ..base()
            },
        ),
        (
            "two bits of graphics and one of text",
            RenderOptions {
                graphics_bits: 2,
                text_bits: 1,
                ..base()
            },
        ),
        (
            "graphics at level 0, by any part",
            RenderOptions {
                graphics_bits: 0,
                hard_edge: HardEdge::AnyPart,
                ..base()
            },
        ),
        (
            "a thinnest line",
            RenderOptions {
                thinnest_line: Some(2.0),
                ..base()
            },
        ),
        (
            "forced colours",
            RenderOptions {
                forced_colours: Some(ForcedColours::default()),
                ..base()
            },
        ),
        (
            "no annotations",
            RenderOptions {
                annotations: false,
                ..base()
            },
        ),
        (
            "the annotations alone",
            RenderOptions {
                content: false,
                ..base()
            },
        ),
        (
            "no widgets",
            RenderOptions {
                widgets: false,
                ..base()
            },
        ),
        (
            "no annotation but widgets",
            RenderOptions {
                other_annotations: false,
                ..base()
            },
        ),
        (
            "counted",
            RenderOptions {
                stats: true,
                ..base()
            },
        ),
        (
            "cancelled",
            RenderOptions {
                cancel: Some(cancelled),
                ..base()
            },
        ),
        (
            "bands on three threads",
            RenderOptions {
                scale: 2.0,
                parallelism: Parallelism::Threads(3),
                ..base()
            },
        ),
        (
            "bands on seven threads, of a region",
            RenderOptions {
                scale: 2.0,
                region: Some(PixelRegion::new(20, 30, 150, 100)),
                parallelism: Parallelism::Threads(7),
                ..base()
            },
        ),
    ]
}

/// **A stream draws what its page draws, under every option**: the scale
/// and its clamp, a region on the page and off it, every format, a page on
/// nothing and premultiplied, ink, anti-aliasing off under two rules and its
/// levels, a thinnest line, forced colours, the content and the appearances
/// by their kind, statistics, cancellation and threads asked for, each the
/// page's own render of the same options in shape, pixels and warnings —
/// one stream, read once, for all of them.
#[test]
fn a_stream_draws_what_its_page_draws_under_every_option() {
    let (document, stream) = page_and_stream(every_option_page());
    let page = document.page(0).expect("a page");
    let plain = page.render(&RenderOptions {
        annotations: false,
        ..RenderOptions::default()
    });
    assert!(
        plain.data != page.render(&RenderOptions::default()).data,
        "the fixture's annotation draws"
    );
    for (what, options) in every_option() {
        let drawn = stream.render(&options);
        same(&drawn, &page.render(&options), what);
        assert_eq!(
            drawn.stats.is_some(),
            options.stats,
            "{what}: counted when asked, and only then"
        );
        assert_eq!(
            stream.pixel_size(&options),
            page.pixel_size(&options),
            "{what}: the size it gives"
        );
    }
}

// ---- what a stream says about itself ----------------------------------------------

/// A two-page document whose second page is a crop of its media box, turned
/// a quarter and composited in a transparency group of its own.
fn two_pages() -> Vec<u8> {
    raw_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Contents 4 0 R >>".to_vec(),
        raw_stream("", b"0 0 1 rg 5 5 30 30 re f"),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 60] /CropBox [10 5 90 55] \
          /Rotate 90 /Group << /S /Transparency /CS /DeviceRGB >> /Contents 6 0 R >>"
            .to_vec(),
        raw_stream(
            "",
            b"1 0 0 rg 12 8 40 20 re f 0 0 1 RG 3 w 20 10 m 80 50 l S",
        ),
    ])
}

/// **A stream says which page it is and reads back to its bytes**: the
/// second page's stream, written by the page and by its list alike, is of
/// this build's version and no usage, names its page, gives the page's size
/// turned and cropped, writes back to the bytes it was read from — its
/// records led by the page's and with no end among them, and those records
/// taken back write the same bytes — and draws the turned page.
#[test]
fn a_stream_reads_back_to_its_bytes_and_names_its_page() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<CommandStream>();

    let document = Document::open(two_pages()).expect("it opens");
    let page = document.page(1).expect("a second page");
    let bytes = page.command_stream();
    assert_eq!(
        page.display_list().command_stream(),
        bytes,
        "one stream from the page and from its list"
    );
    let stream = CommandStream::parse(&bytes).expect("the stream reads");
    assert_eq!(
        stream.version(),
        (COMMAND_STREAM_MAJOR, COMMAND_STREAM_MINOR)
    );
    assert_eq!(stream.page_index(), 1, "the second page");
    assert_eq!(stream.usage(), None, "written for no usage");
    assert_eq!(stream.size(), page.size(), "the page's size");
    assert_eq!(stream.size(), (50.0, 80.0), "the crop box, turned");
    assert_eq!(stream.to_bytes(), bytes, "read and written back");

    let records = stream.records();
    assert_eq!(
        records.first().map(|record| record.kind),
        Some(CommandKind::Page.number()),
        "the page record leads"
    );
    assert!(
        records
            .iter()
            .all(|record| record.kind != CommandKind::End.number()),
        "the end is no record of the stream's"
    );
    let taken_back = CommandStream::from_records(records).expect("the records read");
    assert_eq!(taken_back.to_bytes(), bytes, "the records write the stream");
    assert!(format!("{stream:?}").contains("CommandStream"));

    for scale in [1.0, 1.5] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        assert_eq!(stream.pixel_size(&options), page.pixel_size(&options));
        same(
            &stream.render(&options),
            &page.render(&options),
            &format!("the turned page at {scale}x"),
        );
    }
}

/// **What is not a page's stream is refused by name**: bytes that are no
/// command stream, a newer major, a stream cut short and one running on, a
/// kind this minor does not define, a record whose fields are not its kind's,
/// a stream with no page record, and a stream past each budget a page can
/// reach — its bytes, its records and its cells' depth.
#[test]
fn what_is_not_a_page_stream_is_refused_by_name() {
    let document = Document::open(every_option_page()).expect("it opens");
    let bytes = document.page(0).expect("a page").command_stream();
    let records = CommandStream::parse(&bytes)
        .expect("the stream reads")
        .records()
        .to_vec();
    let refused = |bytes: &[u8]| CommandStream::parse(bytes).err();

    assert_eq!(refused(b""), Some(CommandStreamError::NotACommandStream));
    assert_eq!(
        refused(b"%PDF-1.7\n"),
        Some(CommandStreamError::NotACommandStream)
    );

    let mut newer = bytes.clone();
    newer[8..10].copy_from_slice(&(COMMAND_STREAM_MAJOR + 1).to_le_bytes());
    assert_eq!(
        refused(&newer),
        Some(CommandStreamError::NewerMajor {
            found: COMMAND_STREAM_MAJOR + 1,
            read: COMMAND_STREAM_MAJOR,
        })
    );

    assert!(
        matches!(
            refused(&bytes[..bytes.len() - 1]),
            Some(CommandStreamError::Truncated { .. })
        ),
        "a stream cut short"
    );
    let mut running_on = bytes.clone();
    running_on.push(0);
    assert_eq!(
        refused(&running_on),
        Some(CommandStreamError::TrailingBytes { at: bytes.len() })
    );

    // Kind 63 is in the codec's range and no kind of this minor.
    assert!(CommandKind::from_number(63).is_none());
    let mut unknown = records.clone();
    unknown.push(CommandRecord {
        kind: 63,
        fields: vec![CommandField::Unsigned(7)],
    });
    assert_eq!(
        CommandStream::from_records(&unknown).err(),
        Some(CommandStreamError::UnknownRecord { kind: 63 })
    );

    let mut turned = records.clone();
    turned[0].fields[1] = CommandField::Unsigned(45);
    assert!(
        matches!(
            CommandStream::from_records(&turned).err(),
            Some(CommandStreamError::Malformed { kind, .. }) if kind == CommandKind::Page.number()
        ),
        "a page turned by 45 degrees"
    );
    assert!(
        matches!(
            CommandStream::from_records(&[]).err(),
            Some(CommandStreamError::Malformed { kind, .. }) if kind == CommandKind::Page.number()
        ),
        "a page stream with no page"
    );

    let under = |budgets: CommandBudgets| CommandStream::parse_with(&bytes, &budgets).err();
    for (budgets, budget) in [
        (
            CommandBudgets {
                bytes: 64,
                ..CommandBudgets::DEFAULT
            },
            "bytes",
        ),
        (
            CommandBudgets {
                records: 1,
                ..CommandBudgets::DEFAULT
            },
            "records",
        ),
        // The pattern's cell is a stream one deep.
        (
            CommandBudgets {
                depth: 0,
                ..CommandBudgets::DEFAULT
            },
            "depth",
        ),
    ] {
        assert_eq!(
            under(budgets),
            Some(CommandStreamError::OverBudget { budget }),
            "past the {budget} budget"
        );
    }
    assert!(
        CommandStream::parse_with(&bytes, &CommandBudgets::DEFAULT).is_ok(),
        "and inside the defaults it reads"
    );
}

/// **A newer minor's kind is stepped over and kept**: the stream with a
/// record of a kind this build does not define, at the next minor version,
/// reads, keeps the record by its number, writes back to its own bytes at
/// its own version, and draws what the page draws.
#[test]
fn a_newer_minor_s_record_is_kept_and_stepped_over() {
    let (document, stream) = page_and_stream(every_option_page());
    let page = document.page(0).expect("a page");
    let mut records = stream.records().to_vec();
    records.push(CommandRecord {
        kind: 63,
        fields: vec![
            CommandField::Unsigned(7),
            CommandField::Reals(vec![0.5, -0.0]),
        ],
    });
    let minor = COMMAND_STREAM_MINOR + 1;
    let newer =
        tinker_pdf_content::commands::write_stream_at(COMMAND_STREAM_MAJOR, minor, &records);
    let read = CommandStream::parse(&newer).expect("a newer minor reads");
    assert_eq!(read.version(), (COMMAND_STREAM_MAJOR, minor));
    assert_eq!(read.records(), records.as_slice(), "the record is kept");
    assert_eq!(read.to_bytes(), newer, "and written back at its version");
    let options = RenderOptions::default();
    same(
        &read.render(&options),
        &page.render(&options),
        "the newer minor",
    );
}

/// A 40 x 40 page whose group `/P` is on in `/D` and off in print, under an
/// `/AS` entry for print: a red square in the group, a blue one outside it.
fn print_usage_page() -> Vec<u8> {
    let content = "/OC /P BDC 1 0 0 rg 0 0 20 20 re f EMC 0 0 1 rg 20 0 20 20 re f";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << /ON [5 0 R] \
/AS [<< /Event /Print /Category [/Print] /OCGs [5 0 R] >>] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /Properties << /P 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (Screen only) /Usage << /Print << /PrintState /OFF >> >> >>\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// **A stream is the drawing of the usage it was written for**: written for
/// print it hides what print hides whatever usage a render names, and
/// written for none it shows it to a render for print — there is no document
/// behind a stream to interpret for another usage.
#[test]
fn a_stream_draws_the_usage_it_was_written_for() {
    let document = Document::open(print_usage_page()).expect("it opens");
    let page = document.page(0).expect("a page");
    let usage = |usage| RenderOptions {
        usage,
        ..RenderOptions::default()
    };
    let (none, view, print) = (
        usage(None),
        usage(Some(Usage::View)),
        usage(Some(Usage::Print)),
    );
    let printed = page.render(&print);
    let plain = page.render(&none);
    assert!(
        printed.data != plain.data,
        "the print state hides the red square"
    );

    let for_print =
        CommandStream::parse(&page.display_list_for(Some(Usage::Print)).command_stream())
            .expect("the print stream reads");
    assert_eq!(for_print.usage(), Some(Usage::Print));
    let for_none = CommandStream::parse(&page.command_stream()).expect("the stream reads");
    assert_eq!(for_none.usage(), None);
    for (what, options) in [("no usage", &none), ("view", &view), ("print", &print)] {
        same(
            &for_print.render(options),
            &printed,
            &format!("the print stream, rendered for {what}"),
        );
        same(
            &for_none.render(options),
            &plain,
            &format!("the stream for no usage, rendered for {what}"),
        );
    }
}

// ---- where the renderer declines ----------------------------------------------

/// A one-page document of `width` x `height` points around `content`, with
/// `/G` a transparency-group form drawing a green square at half opacity and
/// `/Off` an optional-content group the default configuration hides.
fn pdf(content: &str, width: u32, height: u32) -> Vec<u8> {
    let form = "/GS0 gs 0 0.6 0 rg 2 2 6 6 re f";
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [6 0 R] /D << /OFF [6 0 R] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
   /Resources << /XObject << /G 5 0 R >> /Properties << /Off 6 0 R >>\n\
                 /ExtGState << /GS0 << /ca 0.5 >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 10]\n\
   /Group << /S /Transparency >> /Resources << /ExtGState << /GS0 << /ca 0.5 >> >> >>\n\
   /Length {} >>\nstream\n{form}\nendstream\nendobj\n\
6 0 obj\n<< /Type /OCG /Name (Hidden) >>\nendobj\n",
        content.len(),
        form.len()
    );
    out.push_str("trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");
    out.into_bytes()
}

/// Asserts the stream of `bytes`' first page draws what the page renders, at
/// two scales, and returns the page's warnings.
#[track_caller]
fn streams_as_it_renders(bytes: Vec<u8>, what: &str) -> Vec<RenderWarning> {
    let (document, stream) = page_and_stream(bytes);
    let page = document.page(0).expect("a page");
    let mut warnings = Vec::new();
    for scale in [1.0, 2.5] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        let direct = page.render(&options);
        same(
            &stream.render(&options),
            &direct,
            &format!("{what} at {scale}x"),
        );
        warnings = direct.warnings;
    }
    warnings
}

/// **Past the group-buffer budget** the renderer declines every further
/// group, and the stream holds the calls the interpretation made with the
/// renderer's answers: the green square past the budget, at `x = 20`, is
/// drawn as the page draws it.
#[test]
fn a_page_past_the_group_budget_streams_as_it_renders() {
    // 2 000 is `MAX_GROUP_BUFFERS`; the page spends them all, then asks once
    // more somewhere that shows.
    let mut content = String::new();
    for _ in 0..2_000 {
        content.push_str("/G Do\n");
    }
    content.push_str("q 1 0 0 1 20 0 cm /G Do Q\n");
    let warnings = streams_as_it_renders(pdf(&content, 40, 12), "the budget page");
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::GroupBudgetSpent { .. })),
        "the page really is past the budget: {warnings:?}"
    );
}

/// **Inside hidden optional content** the renderer declines a group and not
/// a soft mask; the page draws after the scope, where a bracket the stream
/// held askew would show.
#[test]
fn a_group_inside_hidden_content_streams_as_it_renders() {
    let content = "/OC /Off BDC /G Do /G Do EMC \
                   0.9 0.1 0.1 rg 11 1 8 8 re f /G Do";
    streams_as_it_renders(pdf(content, 30, 12), "the hidden-group page");
}

// ---- what each render of a stream reports ----------------------------------------
//
// A stream holds what the page's interpretation and its resources noted — a
// font no name resolved, an image decoded with damage, what a pattern cell met
// — each with the answer or the call it was met at, so a render reports what
// it reaches, as a render of the page does, and nothing an earlier render or
// the writing met.

/// A one-page document of `width` x `height` points around `content`, with
/// `resources` and further objects numbered from 5.
fn page_with(
    content: &str,
    width: u32,
    height: u32,
    resources: &str,
    objects: &[&[u8]],
) -> Vec<u8> {
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
   /Resources {resources} /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len()
    )
    .into_bytes();
    for (index, object) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", index + 5).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            objects.len() + 5
        )
        .as_bytes(),
    );
    out
}

/// Packs `0` and `1` into bytes, most significant bit first, anything else
/// ignored.
fn bits(pattern: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut byte, mut count) = (0u8, 0u32);
    for c in pattern.chars().filter(|c| *c == '0' || *c == '1') {
        byte = (byte << 1) | u8::from(c == '1');
        count += 1;
        if count % 8 == 0 {
            out.push(byte);
            byte = 0;
        }
    }
    if count % 8 != 0 {
        out.push(byte << (8 - count % 8));
    }
    out
}

/// Asserts a stream's render reports what the page's render reports, for
/// each set of options in turn and in that order — so an earlier render of
/// the stream is in front of every later one.
#[track_caller]
fn reports_as_it_renders(bytes: Vec<u8>, asks: &[RenderOptions], what: &str) {
    let (document, stream) = page_and_stream(bytes);
    let page = document.page(0).expect("a page");
    for (index, options) in asks.iter().enumerate() {
        same(
            &stream.render(options),
            &page.render(options),
            &format!("{what}, render {index}"),
        );
    }
}

/// **A font no name resolved**, noted where the interpretation met it: a
/// cancelled render of the stream draws nothing and reports nothing of it,
/// and an uncancelled one reports it.
#[test]
fn a_cancelled_stream_reports_no_font_it_never_reached() {
    let bytes = page_with(
        "BT /Nope 12 Tf 10 10 Td (Hi) Tj ET 0 0 1 rg 1 1 8 8 re f",
        40,
        40,
        "<< >>",
        &[],
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    let cancelled = RenderOptions {
        cancel: Some(cancel),
        ..RenderOptions::default()
    };
    reports_as_it_renders(
        bytes,
        &[cancelled.clone(), RenderOptions::default(), cancelled],
        "the missing-font page",
    );
}

/// **What a pattern cell met** is reported by the render that draws the
/// cell: a region that misses the patterned fill draws no cell and reports
/// nothing of the font the cell's stream could not resolve, before a full
/// render of the stream and after one.
#[test]
fn a_region_that_draws_no_cell_of_a_stream_reports_nothing_a_cell_met() {
    let cell = raw_stream(
        "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 \
         /BBox [0 0 10 10] /XStep 10 /YStep 10 /Resources << >>",
        b"BT /Nope 6 Tf 1 1 Td (x) Tj ET 1 0 0 rg 0 0 5 5 re f",
    );
    let bytes = page_with(
        "/Pattern cs /P scn 0 60 40 40 re f",
        100,
        100,
        "<< /Pattern << /P 5 0 R >> >>",
        &[&cell],
    );
    let region = RenderOptions {
        region: Some(PixelRegion::new(50, 0, 50, 50)),
        ..RenderOptions::default()
    };
    let document = Document::open(bytes.clone()).expect("it opens");
    let page = document.page(0).expect("a page");
    assert!(
        page.render(&RenderOptions::default())
            .warnings
            .contains(&RenderWarning::UnreadableFont)
            && page.render(&region).warnings.is_empty(),
        "the fixture is what it says"
    );
    reports_as_it_renders(
        bytes,
        &[
            region.clone(),
            RenderOptions::default(),
            region,
            RenderOptions::default(),
        ],
        "the patterned page",
    );
}

/// **An image that decoded with damage**, and one this build cannot decode
/// at all, are each answered with what their decode noted, and every render
/// of the stream that draws them says so in the words the page's render
/// uses.
#[test]
fn every_render_of_a_stream_names_the_images_the_page_draws() {
    // `ccitt.rs`'s damaged row: a fax whose second row will not decode.
    let fax = raw_stream(
        "/Type /XObject /Subtype /Image /Width 8 /Height 4 \
         /ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode \
         /DecodeParms << /K -1 /Columns 8 /Rows 4 >>",
        &bits(concat!("001 00110101 011 1 ", "111 ", "00000001")),
    );
    // `images.rs`'s placeholder: a JPX codestream of four zero bytes.
    let odd = raw_stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode",
        &[0, 0, 0, 0],
    );
    let bytes = page_with(
        "q 20 0 0 20 0 0 cm /Fax Do Q q 10 0 0 10 25 5 cm /Odd Do Q \
         q 5 0 0 5 25 20 cm /Odd Do Q",
        40,
        30,
        "<< /XObject << /Fax 5 0 R /Odd 6 0 R >> >>",
        &[&fax, &odd],
    );
    let document = Document::open(bytes.clone()).expect("it opens");
    let direct = document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(
        direct
            .warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::DamagedImage { .. }))
            && direct
                .warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::UnsupportedImage { .. })),
        "the fixture is what it says: {:?}",
        direct.warnings
    );
    let twice = RenderOptions {
        scale: 2.0,
        ..RenderOptions::default()
    };
    reports_as_it_renders(
        bytes,
        &[RenderOptions::default(), twice, RenderOptions::default()],
        "the damaged-image page",
    );
}

/// **A glyph its font resolves to `.notdef`** is answered with the outline
/// and the note its extraction made, and every render of the stream that
/// draws it reports it.
///
/// A composite font over `render_support`'s TrueType face whose
/// `/CIDToGIDMap` sends CID 1 to glyph 0, which 9.7.4.2 makes a CID the font
/// does not carry, and CID 2 to a glyph it does.
#[test]
fn every_render_of_a_stream_names_the_glyph_its_font_could_not_resolve() {
    let face = curvy_font();
    let type0: &[u8] = b"<< /Type /Font /Subtype /Type0 /BaseFont /Curvy /Encoding /Identity-H \
                         /DescendantFonts [6 0 R] >>";
    let cid_font: &[u8] = b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Curvy \
                            /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                            /FontDescriptor 7 0 R /CIDToGIDMap 8 0 R /DW 600 >>";
    let descriptor: &[u8] = b"<< /Type /FontDescriptor /FontName /Curvy /Flags 4 \
                              /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
                              /CapHeight 700 /StemV 80 /FontFile2 9 0 R >>";
    let map = raw_stream("", &[0, 0, 0, 0, 0, 3]);
    let program = raw_stream("", &face);
    let bytes = page_with(
        "BT /F0 20 Tf 5 10 Td <00010002> Tj ET",
        60,
        40,
        "<< /Font << /F0 5 0 R >> >>",
        &[type0, cid_font, descriptor, &map, &program],
    );
    let document = Document::open(bytes.clone()).expect("it opens");
    let direct = document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(
        direct.warnings.contains(&RenderWarning::UnreadableFont),
        "the fixture is what it says: {:?}",
        direct.warnings
    );
    let twice = RenderOptions {
        scale: 2.0,
        ..RenderOptions::default()
    };
    reports_as_it_renders(
        bytes,
        &[RenderOptions::default(), twice, RenderOptions::default()],
        "the unresolved-glyph page",
    );
}
