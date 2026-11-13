//! The external graphics state (8.4.5): every key a `gs` sets draws as the
//! operator that sets the same parameter.
//!
//! A **twin** is two pages of one document, sharing one indirect `/Resources`,
//! that differ only in their content streams: one sets a parameter through a
//! named `/ExtGState`, the other through its operator (`w` `J` `j` `M` `d` `ri`
//! `i`). Each twin renders byte-identical, degenerate operands and a `Q`
//! included, and each applied key's page also differs from the page without
//! it, so a pair that agrees because neither applies anything fails. A key the
//! engine stores for the row that applies it (`/SM`, `/OP`, ...) moves no
//! pixel yet, so its page draws as the page without it.
//!
//! `/CA`, `/ca`, `/BM` and `/SMask` have no operator twin; their pages are held
//! to pinned pixels.

mod written_support;

use tinker_pdf::write::{save, SaveOptions};
use tinker_pdf::{Bitmap, Document, RenderOptions};

/// A document whose pages share one indirect `/Resources` (object 3) holding
/// `states` as its `/ExtGState`, and a transparency group form (object 4) a
/// soft mask can name: its left half white and its right half black. Page
/// `i` is 40 x 40 and draws `contents[i]`.
fn document(states: &str, contents: &[&str]) -> Vec<u8> {
    document_with(states, &[], contents)
}

/// [`document`], with `extra` as objects 5 onwards, for `states` to name.
fn document_with(states: &str, extra: &[String], contents: &[&str]) -> Vec<u8> {
    let first = 5 + extra.len();
    let kids: Vec<String> = (0..contents.len())
        .map(|i| format!("{} 0 R", first + 2 * i))
        .collect();
    let mask = "1 g 0 0 20 40 re f 0 g 20 0 20 40 re f";
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            contents.len(),
            kids.join(" ")
        ),
        format!("<< /ExtGState << {states} >> >>"),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] \
             /Group << /S /Transparency /CS /DeviceGray >> /Length {} >>\n\
             stream\n{mask}\nendstream",
            mask.len()
        ),
    ];
    objects.extend(extra.iter().cloned());
    for (i, content) in contents.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Resources 3 0 R \
             /Contents {} 0 R >>",
            first + 1 + 2 * i
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
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

/// Every page of `bytes`, rendered with the default options.
fn render_all(bytes: Vec<u8>) -> Vec<Bitmap> {
    let doc = Document::open(bytes).expect("the document opens");
    (0..doc.page_count())
        .map(|i| {
            doc.page(i)
                .expect("a page")
                .render(&RenderOptions::default())
        })
        .collect()
}

/// FNV-1a over a bitmap's size and pixels.
fn pin(bitmap: &Bitmap) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let size = [bitmap.width, bitmap.height];
    for byte in size
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .chain(bitmap.data.iter().copied())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// **`/CA`, `/ca`, `/BM` and `/SMask` draw as pinned**: a half-transparent
/// stroke, a half-transparent fill, a multiplied square and a page masked to
/// the group's white half, each unlike its page without the key.
#[test]
fn the_four_keys_the_old_methods_read_draw_as_they_did() {
    let states = "/StrokeHalf << /CA 0.5 >> /FillHalf << /ca 0.5 >> \
                  /Multiply << /BM /Multiply >> \
                  /Mask << /SMask << /S /Luminosity /G 4 0 R >> >>";
    let stroke = "0 0 1 RG 4 w 5 20 m 35 20 l S";
    let fill = "0 0 1 rg 10 10 20 20 re f";
    let grey = "0.5 g 0 0 40 40 re f";
    let pages = [
        format!("/StrokeHalf gs {stroke}"),
        stroke.to_string(),
        format!("/FillHalf gs {fill}"),
        fill.to_string(),
        format!("{grey} /Multiply gs 0.5 g 10 10 20 20 re f"),
        format!("{grey} 0.5 g 10 10 20 20 re f"),
        "/Mask gs 0 0 1 rg 0 0 40 40 re f".to_string(),
        "0 0 1 rg 0 0 40 40 re f".to_string(),
    ];
    let contents: Vec<&str> = pages.iter().map(String::as_str).collect();
    let bitmaps = render_all(document(states, &contents));
    let pins = [
        ("/CA", PIN_CA),
        ("/ca", PIN_FILL_CA),
        ("/BM", PIN_BM),
        ("/SMask", PIN_SMASK),
    ];
    let mut measured = Vec::new();
    let mut expected = Vec::new();
    for (index, (key, pinned)) in pins.iter().enumerate() {
        let with = &bitmaps[2 * index];
        let without = &bitmaps[2 * index + 1];
        assert_ne!(
            with.data, without.data,
            "{key}: the page draws as the page without it"
        );
        measured.push(format!("{key} {:#018x}", pin(with)));
        expected.push(format!("{key} {pinned:#018x}"));
    }
    assert_eq!(
        measured, expected,
        "a key no longer draws as the replaced method drew it"
    );
    println!("ext-gstate-pins: RAN {}", measured.join(", "));
}

/// A stroke under `/CA 0.5`.
const PIN_CA: u64 = 0x3f86_d4eb_8ca9_de85;
/// A fill under `/ca 0.5`.
const PIN_FILL_CA: u64 = 0x39ca_1c92_dd14_1b85;
/// A grey square multiplied over grey.
const PIN_BM: u64 = 0xdebf_72d1_0004_6245;
/// A blue page under a luminosity mask white on its left half.
const PIN_SMASK: u64 = 0x2034_c5ed_028d_d645;

/// The stroke every twin draws: two ends, a sharp corner and a long run, so a
/// width, a cap, a join, a miter limit and a dash each show.
const STROKE: &str = "0 0 1 RG 6 6 m 20 34 l 34 6 l S";

/// A second stroke, drawn after a `Q`.
const AFTER: &str = "0 1 0 RG 4 20 m 36 20 l S";

/// **Every scalar key a `gs` sets draws as its operator twin**, degenerate
/// operands and a `Q` included, and each applied key's page differs from the
/// page without it. A stored key with no operator draws as the page without
/// it: nothing applies it yet.
#[test]
fn every_scalar_key_set_by_gs_draws_as_its_twin() {
    // The entry, the operator that sets the same, and whether the page
    // differs from the page without it.
    let twins = [
        ("/LW 3", "3 w", true),
        ("/LW -1", "-1 w", false),
        ("/LC 1", "1 J", true),
        ("/LC 7", "7 J", false),
        ("/LJ 1", "1 j", true),
        ("/LJ 9", "9 j", false),
        ("/ML 1", "1 M", true),
        ("/ML 0.5", "0.5 M", false),
        ("/D [[3 2] 1]", "[3 2] 1 d", true),
        ("/D [[0 0] 0]", "[0 0] 0 d", false),
        ("/D [[-1 2] 0]", "[-1 2] 0 d", false),
        ("/RI /Perceptual", "/Perceptual ri", false),
        ("/RI /Vivid", "/Vivid ri", false),
        ("/FL 5", "5 i", false),
    ];
    let stored = [
        "/SM 0.5",
        "/SA true",
        "/OP true",
        "/op false",
        "/OPM 1",
        "/AIS true",
        "/TK false",
        "/UseBlackPtComp /ON",
    ];

    let mut states = String::new();
    let mut pages = Vec::new();
    for (index, (entry, operator, _)) in twins.iter().enumerate() {
        states.push_str(&format!("/S{index} << {entry} >> "));
        // A width the caps and joins show at, unless the width is the key.
        let width = if entry.starts_with("/LW") { "" } else { "4 w " };
        pages.push(format!("{width}/S{index} gs {STROKE}"));
        pages.push(format!("{width}{operator} {STROKE}"));
        pages.push(format!("{width}{STROKE}"));
    }
    // A `Q` restores what the `gs` set, as it restores what the operator set.
    for (index, operator) in [(0, "3 w"), (8, "[3 2] 1 d")] {
        pages.push(format!("4 w q /S{index} gs {STROKE} Q {AFTER}"));
        pages.push(format!("4 w q {operator} {STROKE} Q {AFTER}"));
        pages.push(format!("4 w q {STROKE} Q {AFTER}"));
    }
    for (index, entry) in stored.iter().enumerate() {
        states.push_str(&format!("/T{index} << {entry} >> "));
        pages.push(format!("4 w /T{index} gs {STROKE}"));
        pages.push(format!("4 w {STROKE}"));
    }

    let contents: Vec<&str> = pages.iter().map(String::as_str).collect();
    let bitmaps = render_all(document(&states, &contents));
    let mut at = 0;
    let labels = twins
        .iter()
        .map(|(entry, operator, applied)| (entry.to_string(), *operator, *applied))
        .chain([
            ("/LW 3 under q Q".to_string(), "3 w", true),
            ("/D [[3 2] 1] under q Q".to_string(), "[3 2] 1 d", true),
        ]);
    for (label, operator, applied) in labels {
        let (with, twin, without) = (&bitmaps[at], &bitmaps[at + 1], &bitmaps[at + 2]);
        at += 3;
        assert_eq!(
            with.data, twin.data,
            "{label}: the `gs` page does not draw as `{operator}` does"
        );
        assert_eq!(
            with.data != without.data,
            applied,
            "{label}: the page {} the page without the key",
            if applied { "draws as" } else { "differs from" }
        );
    }
    for entry in stored {
        let (with, without) = (&bitmaps[at], &bitmaps[at + 1]);
        at += 2;
        assert_eq!(
            with.data, without.data,
            "{entry}: a stored key moved a pixel before its row applies it"
        );
    }
    assert_eq!(at, bitmaps.len());
    println!(
        "ext-gstate-twins: RAN {} twins and {} stored keys",
        twins.len() + 2,
        stored.len()
    );
}

/// Objects 5 and 6 for [`document_with`]: `1 - x` sampled at its two ends, and
/// the same curve as a calculator.
fn inverting_streams() -> Vec<String> {
    let sampled = "FF00>";
    let calculator = "{ 1 exch sub }";
    vec![
        format!(
            "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [2] /BitsPerSample 8 \
             /Filter /ASCIIHexDecode /Length {} >>\nstream\n{sampled}\nendstream",
            sampled.len()
        ),
        format!(
            "<< /FunctionType 4 /Domain [0 1] /Range [0 1] /Length {} >>\n\
             stream\n{calculator}\nendstream",
            calculator.len()
        ),
    ]
}

/// **Every function key draws as the page without it**, until the row that
/// applies it does; and **a soft mask's `/TR` is read as written**: a sampled
/// and a calculator function, both streams, draw as the type 2 function of the
/// same curve, and unlike the mask with no `/TR`.
#[test]
fn every_function_key_draws_as_the_page_without_it() {
    let inverse = "<< /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>";
    let states = format!(
        "/TR << /TR 5 0 R >> /TRFour << /TR [6 0 R 6 0 R 6 0 R 6 0 R] >> \
         /TRIdentity << /TR /Identity >> /TR2 << /TR2 {inverse} >> \
         /TR2Default << /TR2 /Default >> /BG << /BG 6 0 R >> /BG2 << /BG2 {inverse} >> \
         /BG2Default << /BG2 /Default >> /UCR << /UCR 5 0 R >> \
         /UCR2 << /UCR2 {inverse} >> /UCR2Default << /UCR2 /Default >> \
         /Mask << /SMask << /S /Luminosity /G 4 0 R >> >> \
         /MaskT2 << /SMask << /S /Luminosity /G 4 0 R /TR {inverse} >> >> \
         /MaskT0 << /SMask << /S /Luminosity /G 4 0 R /TR 5 0 R >> >> \
         /MaskT4 << /SMask << /S /Luminosity /G 4 0 R /TR 6 0 R >> >>"
    );
    let keys = [
        "TR",
        "TRFour",
        "TRIdentity",
        "TR2",
        "TR2Default",
        "BG",
        "BG2",
        "BG2Default",
        "UCR",
        "UCR2",
        "UCR2Default",
    ];
    let fill = "0.2 0.4 0.6 0.1 k 5 5 30 30 re f";
    let mut pages: Vec<String> = keys.iter().map(|key| format!("/{key} gs {fill}")).collect();
    pages.push(fill.to_string());
    let masked = "0 0 1 rg 0 0 40 40 re f";
    for mask in ["MaskT2", "MaskT0", "MaskT4", "Mask"] {
        pages.push(format!("/{mask} gs {masked}"));
    }
    let contents: Vec<&str> = pages.iter().map(String::as_str).collect();
    let bitmaps = render_all(document_with(&states, &inverting_streams(), &contents));

    let without = &bitmaps[keys.len()];
    for (key, page) in keys.iter().zip(&bitmaps) {
        assert_eq!(
            page.data, without.data,
            "/{key}: a stored function moved a pixel before its row applies it"
        );
    }
    let [type2, type0, type4, untransferred] = [1, 2, 3, 4].map(|at| &bitmaps[keys.len() + at]);
    assert_ne!(
        type2.data, untransferred.data,
        "the mask's type 2 `/TR` changed nothing"
    );
    assert_eq!(
        type0.data, type2.data,
        "a sampled `/TR`, a stream, does not draw as the type 2 function of its curve"
    );
    assert_eq!(
        type4.data, type2.data,
        "a calculator `/TR`, a stream, does not draw as the type 2 function of its curve"
    );
    println!(
        "ext-gstate-functions: RAN {} keys and three mask transfers",
        keys.len()
    );
}

/// **Every halftone key draws as the page without it**, until the row that
/// applies it does: a halftone of each type, `/Default` and `/HTO`.
#[test]
fn every_halftone_key_draws_as_the_page_without_it() {
    let stream = |dict: &str, data: &str| {
        format!(
            "<< {dict} /Filter /ASCIIHexDecode /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
    };
    let extra = [
        stream("/HalftoneType 6 /Width 2 /Height 2", "004080C0>"),
        stream("/HalftoneType 10 /Xsquare 1 /Ysquare 1", "0080>"),
        stream("/HalftoneType 16 /Width 1 /Height 1", "0000>"),
    ];
    let screen = "<< /HalftoneType 1 /Frequency 60 /Angle 45 /SpotFunction /Round >>";
    let states = format!(
        "/Type1 << /HT {screen} >> /Type5 << /HT << /HalftoneType 5 /Default {screen} \
         /Cyan 5 0 R >> >> /Type6 << /HT 5 0 R >> /Type10 << /HT 6 0 R >> \
         /Type16 << /HT 7 0 R >> /Default << /HT /Default >> /Origin << /HTO [3 4] >>"
    );
    let keys = [
        "Type1", "Type5", "Type6", "Type10", "Type16", "Default", "Origin",
    ];
    let fill = "0.2 0.4 0.6 0.1 k 5 5 30 30 re f 0.5 g 10 10 m 30 30 l S";
    let mut pages: Vec<String> = keys.iter().map(|key| format!("/{key} gs {fill}")).collect();
    pages.push(fill.to_string());
    let contents: Vec<&str> = pages.iter().map(String::as_str).collect();
    let bitmaps = render_all(document_with(&states, &extra, &contents));
    let without = &bitmaps[keys.len()];
    for (key, page) in keys.iter().zip(&bitmaps) {
        assert_eq!(
            page.data, without.data,
            "/{key}: a stored halftone moved a pixel before its row applies it"
        );
    }
    println!("ext-gstate-halftones: RAN {} keys", keys.len());
}

/// A face whose glyphs draw, embedded whole.
const SANS: &[u8] =
    include_bytes!("../../tinker-pdf-font/data/liberation/LiberationSans-Regular.ttf");

/// One page of 120 x 40 drawing `content` under `resources`, with
/// Liberation Sans embedded as object 4: a TrueType font of WinAnsi codes
/// 72 to 111 (`H` to `o`).
fn font_document(resources: &str, content: &str) -> Vec<u8> {
    let widths = vec!["600"; 40].join(" ");
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [7 0 R] >>".to_vec(),
        resources.as_bytes().to_vec(),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSans \
             /Encoding /WinAnsiEncoding /FirstChar 72 /LastChar 111 /Widths [{widths}] \
             /FontDescriptor 5 0 R >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 \
           /FontBBox [-203 -303 1050 910] /ItalicAngle 0 /Ascent 905 /Descent -212 \
           /CapHeight 716 /StemV 80 /FontFile2 6 0 R >>"
            .to_vec(),
    ];
    let mut program = format!(
        "<< /Length {} /Length1 {} >>\nstream\n",
        SANS.len(),
        SANS.len()
    )
    .into_bytes();
    program.extend_from_slice(SANS);
    program.extend_from_slice(b"\nendstream");
    objects.push(program);
    objects.push(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 40] /Resources 3 0 R \
           /Contents 8 0 R >>"
            .to_vec(),
    );
    objects.push(
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
    );
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
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

/// What one page of a document gives every reader of its text font: its
/// pixels, its text, the program a subsetting save keeps of object 4, and how
/// `Document::fonts` lists object 4.
fn readings(bytes: Vec<u8>) -> (Vec<u8>, String, Vec<u8>, (String, bool, bool)) {
    let doc = Document::open(bytes).expect("the document opens");
    let page = doc.page(0).expect("a page");
    let pixels = page.render(&RenderOptions::default()).data;
    let text = page.text().plain_text();
    let saved = save(&mut doc.editor(), &SaveOptions::default());
    written_support::rewritten("readings", doc.cos().bytes(), &saved.bytes);
    let reopened = Document::open(saved.bytes).expect("the saved document opens");
    let program = reopened
        .fonts()
        .into_iter()
        .find(|font| font.reference.is_some_and(|r| r.num == 4))
        .and_then(|font| font.program_bytes())
        .expect("object 4 keeps a program");
    let listed = doc
        .fonts()
        .into_iter()
        .find(|font| font.reference.is_some_and(|r| r.num == 4))
        .map(|font| {
            (
                font.base_font.clone(),
                font.is_embedded(),
                font.resource_names.is_empty(),
            )
        })
        .expect("object 4 is listed");
    (pixels, text, program, listed)
}

/// **A font a `gs` selects draws and reads as the `Tf` that selects it**:
/// rendered, extracted, subset on save and listed by `Document::fonts` alike,
/// whether the scope's `/Font` names the object or not.
#[test]
fn a_font_set_by_gs_draws_and_reads_as_tf() {
    let text = "5 15 Td (Hello) Tj ET";
    let tf = readings(font_document(
        "<< /Font << /F1 4 0 R >> >>",
        &format!("BT /F1 18 Tf {text}"),
    ));
    let named = readings(font_document(
        "<< /Font << /F1 4 0 R >> /ExtGState << /G << /Font [4 0 R 18] >> >> >>",
        &format!("/G gs BT {text}"),
    ));
    let unnamed = readings(font_document(
        "<< /ExtGState << /G << /Font [4 0 R 18] >> >> >>",
        &format!("/G gs BT {text}"),
    ));
    let blank = readings(font_document(
        "<< /Font << /F1 4 0 R >> >>",
        "BT /F1 18 Tf ET",
    ));

    assert_ne!(tf.0, blank.0, "the `Tf` page draws its text");
    assert_eq!(tf.1.trim(), "Hello");
    for (label, page) in [
        ("named in /Font", &named),
        ("named only by the state", &unnamed),
    ] {
        assert_eq!(
            page.0, tf.0,
            "{label}: the page does not draw as its `Tf` twin"
        );
        assert_eq!(
            page.1, tf.1,
            "{label}: the text does not read as its `Tf` twin's"
        );
        assert_eq!(
            page.2, tf.2,
            "{label}: a subsetting save keeps another program than its `Tf` twin's"
        );
        assert_eq!(
            (&page.3 .0, page.3 .1),
            (&tf.3 .0, tf.3 .1),
            "{label}: listed unlike its `Tf` twin"
        );
    }
    assert!(
        tf.2.len() < SANS.len(),
        "the save subsets the program it keeps"
    );
    assert!(!tf.3 .2 && !named.3 .2, "`/Font` names it: F1");
    assert!(
        unnamed.3 .2,
        "a font only a state names has no resource name of its own"
    );
    println!("ext-gstate-font: RAN rendered, extracted, subset and listed as `Tf`");
}
