//! Embedding missing fonts on write (`docs/design/font-embedding.md`, FT-12):
//! a document whose fonts embed no program, rewritten with the faces a host's
//! provider drew it with, draws as that face drew it with no provider at all.
//!
//! The renders compared here are this engine's, before and after, which is
//! self-consistency and named as such: it is what sees a glyph that went
//! missing or another glyph drawn in its place, which is the failure this pass
//! can cause. Each face is assembled in `embed_support`, each glyph a box of
//! its own size, so another glyph is other pixels.

mod embed_support;
mod written_support;

use std::sync::{Arc, Mutex};

use embed_support::{
    fingerprint, font_page, ink, object, pdf, provider, sans, standard_page, stream, truetype_page,
    Face,
};
use tinker_pdf::write::{save, SaveOptions, Saved};
use tinker_pdf::{
    Document, EmbedReport, EmbeddedFormat, FontEmbedding, FontKind, FontPolicy, FontProvider,
    FontRequest, LeftFont, LeftReason, ObjRef, ProgramKey, RenderOptions, RenderWarning,
    SubsetOutcome,
};

/// What a page draws and reads as.
struct Drawn {
    fingerprint: String,
    ink: usize,
    text: String,
    warnings: Vec<RenderWarning>,
}

fn drawn(document: &Document) -> Drawn {
    let page = document.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions::default());
    Drawn {
        fingerprint: fingerprint(&bitmap),
        ink: ink(&bitmap),
        text: page.text().plain_text(),
        warnings: bitmap.warnings.clone(),
    }
}

/// `bytes` opened with `provider` as the host's.
fn opened_with(bytes: &[u8], provider: &Arc<dyn FontProvider>) -> Document {
    Document::open(bytes.to_vec())
        .expect("it opens")
        .with_fonts(Arc::clone(provider))
}

/// `source`, opened with `provider` and saved asking the pass for its faces,
/// under `fonts`.
fn embedded(source: &[u8], provider: &Arc<dyn FontProvider>, fonts: FontPolicy) -> Saved {
    let document = opened_with(source, provider);
    let mut editor = document.editor();
    save(
        &mut editor,
        &SaveOptions {
            embed: FontEmbedding::Embed(Arc::clone(provider)),
            fonts,
            ..SaveOptions::default()
        },
    )
}

/// The report of a save that asked for the pass.
#[track_caller]
fn report_of(saved: &Saved) -> EmbedReport {
    saved.embedded.clone().expect("the pass was asked for")
}

/// **The row's exit** (FT-12.3): a standard-14 page and an unembedded TrueType
/// page, rendered with the provider, rewritten, and rendered again with none —
/// the same picture, and the same text, from a font that now embeds a subset
/// of the face, named after it.
#[test]
fn a_rewritten_document_draws_as_its_provider_face_drew_it() {
    let face = sans();
    let provider = provider(face.truetype());
    for (label, source) in [
        ("a standard-14 page", standard_page("Hello World")),
        (
            "an unembedded TrueType page",
            truetype_page(&face, "Hello World"),
        ),
    ] {
        let before = drawn(&opened_with(&source, &provider));
        assert!(
            before.ink > 0 && !before.warnings.contains(&RenderWarning::UnreadableFont),
            "{label}: the provider face draws the page: {:?}",
            before.warnings
        );

        let saved = embedded(&source, &provider, FontPolicy::Subset);
        let report = report_of(&saved);
        assert_eq!(report.left, [], "{label}");
        assert_eq!(report.embedded.len(), 1, "{label}: {report:?}");
        assert_eq!(report.embedded[0].format, EmbeddedFormat::TrueType);

        // Opened with no provider, as the next reader opens it.
        let rewritten = written_support::rewritten(label, &source, &saved.bytes);
        let after = drawn(&rewritten);
        assert_eq!(
            after.fingerprint, before.fingerprint,
            "{label}: the rewritten page draws as the provider face drew it"
        );
        assert!(
            !after.warnings.contains(&RenderWarning::UnreadableFont),
            "{label}: and nothing is missing: {:?}",
            after.warnings
        );
        assert_eq!(after.text, before.text, "{label}: and it reads as it did");

        let fonts = rewritten.fonts();
        let font = fonts
            .iter()
            .find(|font| font.is_embedded())
            .expect("the font embeds its face");
        assert_eq!(font.kind, FontKind::TrueType);
        assert_eq!(font.program.map(|p| p.key), Some(ProgramKey::FontFile2));
        assert!(
            font.subset_tag.is_some(),
            "a subset, and named as one: {}",
            font.base_font
        );
        assert_eq!(
            font.name, "TinkerSans-Regular",
            "9.9.2: named after the face it was cut from"
        );
        assert_eq!(report.embedded[0].base_font, font.base_font);
    }
}

/// The face is cut to the glyphs the page drew, `.notdef` and nothing else
/// besides: a page drawing `AB` out of a face of 53 glyphs embeds two.
#[test]
fn the_embedded_face_is_cut_to_the_drawn_glyphs() {
    let face = sans();
    let program = face.truetype();
    let saved = embedded(
        &standard_page("AB"),
        &provider(program.clone()),
        FontPolicy::Keep,
    );
    let report = report_of(&saved);
    let only = report.embedded.first().expect("one font embedded");
    assert_eq!(only.glyphs, 2, "A and B");
    assert_eq!(only.face, program.len());
    assert!(
        only.bytes < only.face,
        "{} is not smaller than the face's {}",
        only.bytes,
        only.face
    );

    let rewritten = Document::open(saved.bytes).expect("the output opens");
    let font = rewritten
        .fonts()
        .into_iter()
        .find(|font| font.is_embedded())
        .expect("the font embeds its face");
    let bytes = font.program_bytes().expect("the program decodes");
    assert_eq!(bytes.len(), only.bytes);
    let sfnt = tinker_pdf_font::Sfnt::parse(&bytes).expect("the cut is an sfnt");
    for glyph in &face.glyphs {
        let id = face.glyph_of(glyph.ch).expect("the face has it");
        let outline = tinker_pdf_font::glyf::outline(&sfnt, id).unwrap_or_default();
        let drawn = matches!(glyph.ch, 'A' | 'B');
        assert_eq!(
            !outline.segments.is_empty(),
            drawn,
            "{:?}: the cut keeps exactly the drawn glyphs' outlines",
            glyph.ch
        );
    }
}

/// A provider that writes down every request and answers none.
struct Records(Mutex<Vec<String>>);

impl FontProvider for Records {
    fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(request.base_font.clone());
        }
        None
    }
}

/// A font that embeds its program and a Type 3 font are missing nothing:
/// neither is a candidate, neither is reported, the provider is asked
/// nothing, and both are written as they were.
#[test]
fn an_embedded_or_type3_font_is_left_as_it_is() {
    let face = sans().truetype();
    let source = pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 80] \
             /Resources << /Font << /F0 4 0 R /F1 7 0 R >> >> /Contents 9 0 R >>",
        ),
        object(
            "<< /Type /Font /Subtype /TrueType /BaseFont /TinkerSans-Regular \
             /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [600 600] \
             /FontDescriptor 5 0 R >>",
        ),
        object(
            "<< /Type /FontDescriptor /FontName /TinkerSans-Regular /Flags 32 \
             /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
             /CapHeight 700 /StemV 80 /FontFile2 6 0 R >>",
        ),
        stream(&format!("/Length1 {}", face.len()), &face),
        object(
            "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 750 750] \
             /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /square 8 0 R >> \
             /Encoding << /Type /Encoding /Differences [65 /square] >> \
             /FirstChar 65 /LastChar 65 /Widths [1000] >>",
        ),
        stream("", b"1000 0 0 0 750 750 d1 0 0 750 750 re f"),
        stream("", b"BT /F0 24 Tf 12 30 Td (AB) Tj /F1 24 Tf (A) Tj ET"),
    ]);
    let records = Arc::new(Records(Mutex::new(Vec::new())));
    let provider: Arc<dyn FontProvider> = records.clone();
    let document = opened_with(&source, &provider);
    let before = drawn(&document);
    assert!(before.ink > 0, "both fonts draw");

    let mut editor = document.editor();
    let saved = save(
        &mut editor,
        &SaveOptions {
            embed: FontEmbedding::Embed(Arc::clone(&provider)),
            fonts: FontPolicy::Keep,
            ..SaveOptions::default()
        },
    );
    assert_eq!(
        saved.embedded,
        Some(EmbedReport::default()),
        "neither font is missing its program, so neither is reported"
    );
    assert_eq!(
        records.0.lock().expect("not poisoned").len(),
        0,
        "and the provider is asked nothing"
    );

    let rewritten = written_support::rewritten("embedded and Type 3", &source, &saved.bytes);
    assert_eq!(drawn(&rewritten).fingerprint, before.fingerprint);
    let fonts = rewritten.fonts();
    let truetype = fonts
        .iter()
        .find(|font| font.kind == FontKind::TrueType)
        .expect("the TrueType font");
    assert_eq!(
        truetype.program_bytes().as_deref(),
        Some(face.as_slice()),
        "its own program, byte for byte"
    );
    assert_eq!(truetype.base_font, "TinkerSans-Regular", "and its own name");
    assert!(fonts.iter().any(|font| font.kind == FontKind::Type3));
}

/// A provider with a text face that declines a symbolic font, as
/// `SimpleFontProvider` does by default — and as the bundled faces do in a
/// `bundled-fonts` build, so the symbolic font is declined in both builds.
struct TextOnly(Vec<u8>);

impl FontProvider for TextOnly {
    fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
        (!request.symbolic).then(|| Arc::new(self.0.clone()))
    }
}

/// A request the provider declines leaves its font as it was, and says so by
/// name, beside a font of the same page the provider answers.
#[test]
fn a_declined_request_is_left_and_reported() {
    let source = pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 80] \
             /Resources << /Font << /F0 4 0 R /F1 5 0 R >> >> /Contents 7 0 R >>",
        ),
        object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"),
        object(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Wingdings /FirstChar 65 \
             /LastChar 66 /Widths [900 900] /FontDescriptor 6 0 R >>",
        ),
        object(
            "<< /Type /FontDescriptor /FontName /Wingdings /Flags 4 \
             /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 900 /Descent -100 \
             /CapHeight 700 /StemV 80 >>",
        ),
        stream("", b"BT /F0 24 Tf 12 30 Td (AB) Tj /F1 24 Tf (AB) Tj ET"),
    ]);
    let provider: Arc<dyn FontProvider> = Arc::new(TextOnly(sans().truetype()));
    let saved = embedded(&source, &provider, FontPolicy::Keep);
    let report = report_of(&saved);
    assert_eq!(
        report.embedded.len(),
        1,
        "Helvetica is answered: {report:?}"
    );
    assert_eq!(report.embedded[0].font, ObjRef::new(4, 0));
    assert_eq!(
        report.left,
        [LeftFont {
            font: Some(ObjRef::new(5, 0)),
            base_font: "Wingdings".to_string(),
            reason: LeftReason::Declined,
        }]
    );
    let rewritten = written_support::rewritten("a declined font", &source, &saved.bytes);
    let wingdings = rewritten
        .fonts()
        .into_iter()
        .find(|font| font.base_font == "Wingdings")
        .expect("Wingdings is still named as it was");
    assert!(!wingdings.is_embedded(), "and still embeds nothing");
}

/// 9.9.1: a face whose `fsType` asks for restricted-licence embedding or for
/// bitmap embedding only is not embedded, nor one that forbids the subset the
/// pass embeds; preview-and-print and editable faces, and installable ones,
/// are.
#[test]
fn a_face_whose_licence_forbids_it_is_left() {
    for (bits, refused) in [
        (0x0002, Some(LeftReason::EmbeddingForbidden)),
        (0x0200, Some(LeftReason::EmbeddingForbidden)),
        (0x0100, Some(LeftReason::SubsettingForbidden)),
        (0x0000, None),
        (0x0004, None),
        (0x0008, None),
    ] {
        let face = sans().with_fs_type(bits);
        let saved = embedded(
            &standard_page("AB"),
            &provider(face.truetype()),
            FontPolicy::Keep,
        );
        let report = report_of(&saved);
        match refused {
            Some(reason) => {
                assert_eq!(
                    report
                        .left
                        .iter()
                        .map(|left| left.reason)
                        .collect::<Vec<_>>(),
                    [reason],
                    "fsType {bits:04X}"
                );
                assert!(report.embedded.is_empty(), "fsType {bits:04X}");
                let output = Document::open(saved.bytes).expect("it opens");
                assert!(
                    output.fonts().iter().all(|font| !font.is_embedded()),
                    "fsType {bits:04X}: nothing was written"
                );
            }
            None => {
                assert_eq!(report.embedded.len(), 1, "fsType {bits:04X}: {report:?}");
                assert!(report.left.is_empty());
            }
        }
    }
}

/// A code the face reaches no glyph for but `.notdef` leaves the font, named
/// by the code, and the page is written as it was: embedding the face would
/// make the missing glyph permanent. So does a face with nothing to be read
/// by — no `cmap` — whose glyphs are reached only by the code as a number.
#[test]
fn a_code_the_face_has_no_glyph_for_leaves_the_font() {
    let partial = Face::covering("TinkerSans-Partial", "AB", 600);
    let source = standard_page("ABZ");
    let partial_provider = provider(partial.truetype());
    let before = drawn(&opened_with(&source, &partial_provider));

    let saved = embedded(&source, &partial_provider, FontPolicy::Keep);
    let report = report_of(&saved);
    assert!(report.embedded.is_empty());
    assert_eq!(
        report
            .left
            .iter()
            .map(|left| left.reason)
            .collect::<Vec<_>>(),
        [LeftReason::NoGlyph {
            code: u32::from(b'Z')
        }]
    );
    let output = Document::open(saved.bytes).expect("it opens");
    assert!(output.fonts().iter().all(|font| !font.is_embedded()));
    let output = output.with_fonts(Arc::clone(&partial_provider));
    assert_eq!(
        drawn(&output).fingerprint,
        before.fingerprint,
        "and the page draws as it did, through the provider still"
    );

    let unmapped = provider(sans().unmapped().truetype());
    let report = report_of(&embedded(&source, &unmapped, FontPolicy::Keep));
    assert_eq!(
        report
            .left
            .iter()
            .map(|left| left.reason)
            .collect::<Vec<_>>(),
        [LeftReason::NoGlyph {
            code: u32::from(b'A')
        }],
        "a face read by glyph number states no glyph for the first code"
    );
}

/// The faces the pass embeds are cut under either policy, to the same bytes;
/// the font pass passes over them, so its report — of the document's own
/// programs — names none of them, and a file whose every program is cut says
/// so.
#[test]
fn the_new_faces_are_cut_whatever_the_font_policy() {
    let face = sans();
    let provider = provider(face.truetype());
    let source = standard_page("Hello");
    let kept = embedded(&source, &provider, FontPolicy::Keep);
    let cut = embedded(&source, &provider, FontPolicy::Subset);
    let (kept_report, cut_report) = (report_of(&kept), report_of(&cut));
    assert_eq!(kept_report.embedded.len(), 1);
    assert_eq!(cut_report.embedded.len(), 1);
    assert_eq!(
        kept_report.embedded[0].bytes, cut_report.embedded[0].bytes,
        "one cut under either policy"
    );
    assert!(kept_report.embedded[0].bytes < face.truetype().len());
    assert_eq!(kept.fonts, SubsetOutcome::Kept);

    let program = cut_report.embedded[0].program;
    let subsetting = cut.fonts.report().expect("the font pass ran");
    assert!(
        subsetting.subsetted.iter().all(|s| s.program != program)
            && subsetting.untouched.iter().all(|u| u.program != program),
        "the font pass passes over the face the embedding pass cut: {subsetting:?}"
    );
    assert!(
        cut.fonts.removed(),
        "and a file whose every program is cut says so"
    );
}

/// A CFF answer — bare, or an `OpenType/CFF` face's `CFF ` table — is
/// embedded as `/FontFile3 /Subtype /Type1C` under a Type 1 font, a TrueType
/// font answered by one included, and draws and reads as it did.
#[test]
fn a_cff_answer_is_embedded_as_type1c() {
    let face = sans();
    for (label, program) in [
        ("a bare CFF", face.cff()),
        ("an OpenType CFF face", face.opentype_cff()),
    ] {
        let provider = provider(program);
        for (page, source) in [
            ("a standard-14 page", standard_page("Hello World")),
            ("a TrueType page", truetype_page(&face, "Hello World")),
        ] {
            let before = drawn(&opened_with(&source, &provider));
            assert!(before.ink > 0, "{label}, {page}: the face draws");
            let saved = embedded(&source, &provider, FontPolicy::Subset);
            let report = report_of(&saved);
            assert_eq!(report.embedded.len(), 1, "{label}, {page}: {report:?}");
            assert_eq!(report.embedded[0].format, EmbeddedFormat::Type1C);

            let rewritten = written_support::rewritten(label, &source, &saved.bytes);
            let after = drawn(&rewritten);
            assert_eq!(after.fingerprint, before.fingerprint, "{label}, {page}");
            assert_eq!(after.text, before.text, "{label}, {page}");
            let font = rewritten
                .fonts()
                .into_iter()
                .find(|font| font.is_embedded())
                .expect("the font embeds its face");
            assert_eq!(font.kind, FontKind::Type1, "{label}, {page}");
            assert_eq!(font.program.map(|p| p.key), Some(ProgramKey::FontFile3));
        }
    }
}

/// `SaveOptions::default()` runs no pass and reports none, and a document
/// that is missing no program is written byte for byte the same asked or
/// not: a pass that finds nothing writes nothing.
#[test]
fn a_save_that_asks_for_nothing_embeds_nothing() {
    let document = Document::open(standard_page("Hello")).expect("it opens");
    let mut editor = document.editor();
    let saved = save(&mut editor, &SaveOptions::default());
    assert_eq!(
        saved.embedded, None,
        "Leave runs nothing and reports nothing"
    );
    let output = Document::open(saved.bytes).expect("it opens");
    assert!(output.fonts().iter().all(|font| !font.is_embedded()));

    let face = sans().truetype();
    let complete = font_page(
        "/Type /Font /Subtype /TrueType /BaseFont /TinkerSans-Regular \
         /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [600 600] \
         /FontDescriptor 5 0 R",
        "/Type /FontDescriptor /FontName /TinkerSans-Regular /Flags 32 \
         /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
         /CapHeight 700 /StemV 80 /FontFile2 6 0 R",
        &[stream(&format!("/Length1 {}", face.len()), &face)],
        "BT /F0 24 Tf 12 30 Td (AB) Tj ET",
    );
    let left = {
        let mut editor = Document::open(complete.clone()).expect("it opens").editor();
        save(&mut editor, &SaveOptions::default())
    };
    let asked = {
        let mut editor = Document::open(complete).expect("it opens").editor();
        save(
            &mut editor,
            &SaveOptions {
                embed: FontEmbedding::Embed(provider(face)),
                ..SaveOptions::default()
            },
        )
    };
    assert_eq!(asked.embedded, Some(EmbedReport::default()));
    assert_eq!(
        asked.bytes, left.bytes,
        "a pass that finds nothing missing writes nothing"
    );
}

/// A page imported into another document brings its font's program with it
/// as a stream the editor wrote, and a font that embeds its program is no
/// candidate however it came to be in the editor: the pass asks the document
/// as the editor has it, read as a file, where that program is a stream.
#[test]
fn an_imported_font_keeps_its_own_program() {
    let face = sans().truetype();
    let imported = font_page(
        "/Type /Font /Subtype /TrueType /BaseFont /TinkerSans-Regular \
         /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [600 600] \
         /FontDescriptor 5 0 R",
        "/Type /FontDescriptor /FontName /TinkerSans-Regular /Flags 32 \
         /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
         /CapHeight 700 /StemV 80 /FontFile2 6 0 R",
        &[stream(&format!("/Length1 {}", face.len()), &face)],
        "BT /F0 24 Tf 12 30 Td (AB) Tj ET",
    );
    let source = Document::open(imported).expect("it opens");
    let mut builder = tinker_pdf::DocumentBuilder::new();
    builder.add_page(320.0, 80.0, |_| {});
    let target = Document::open(builder.finish()).expect("it opens");

    let records = Arc::new(Records(Mutex::new(Vec::new())));
    let provider: Arc<dyn FontProvider> = records.clone();
    let mut editor = target.editor();
    editor
        .import_page(source.cos(), 0, 1)
        .expect("the page imports");
    let saved = save(
        &mut editor,
        &SaveOptions {
            embed: FontEmbedding::Embed(Arc::clone(&provider)),
            fonts: FontPolicy::Keep,
            ..SaveOptions::default()
        },
    );
    assert_eq!(saved.embedded, Some(EmbedReport::default()));
    assert_eq!(records.0.lock().expect("not poisoned").len(), 0);
    let output = Document::open(saved.bytes).expect("it opens");
    let font = output
        .fonts()
        .into_iter()
        .find(|font| font.base_font == "TinkerSans-Regular")
        .expect("the imported font, by its own name");
    assert_eq!(font.program_bytes().as_deref(), Some(face.as_slice()));
}

/// A font written directly into a resource dictionary has no object for a
/// rewrite to address: it is left, and named by its `/BaseFont`.
#[test]
fn a_font_written_into_the_resources_is_left_and_named() {
    let source = pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 80] /Resources << /Font << \
             /F0 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> \
             /Contents 4 0 R >>",
        ),
        stream("", b"BT /F0 24 Tf 12 30 Td (AB) Tj ET"),
    ]);
    let report = report_of(&embedded(
        &source,
        &provider(sans().truetype()),
        FontPolicy::Keep,
    ));
    assert!(report.embedded.is_empty());
    assert_eq!(
        report.left,
        [LeftFont {
            font: None,
            base_font: "Helvetica".to_string(),
            reason: LeftReason::NotAnObject,
        }]
    );
}

/// A font a form field's `/DA` may draw with is left: the field's next value
/// may need any character, which a cut face does not have (12.7.3.3).
#[test]
fn a_font_a_field_may_draw_with_is_left_and_named() {
    let source = pdf(&[
        object(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] \
             /DR << /Font << /Helv 4 0 R >> >> /DA (/Helv 0 Tf 0 g) >> >>",
        ),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 80] \
             /Resources << /Font << /F0 4 0 R >> >> /Contents 5 0 R >>",
        ),
        object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"),
        stream("", b"BT /F0 24 Tf 12 30 Td (AB) Tj ET"),
    ]);
    let report = report_of(&embedded(
        &source,
        &provider(sans().truetype()),
        FontPolicy::Keep,
    ));
    assert!(report.embedded.is_empty());
    assert_eq!(
        report.left,
        [LeftFont {
            font: Some(ObjRef::new(4, 0)),
            base_font: "Helvetica".to_string(),
            reason: LeftReason::FieldResource,
        }]
    );
}

/// A font a Type 3 font's own `/Resources` names is left: the interpreter
/// runs a glyph procedure in the enclosing scope, so what it shows is
/// credited to another font, and a cut to the codes the walk saw could drop
/// one a procedure shows (9.6.5).
#[test]
fn a_font_a_type3_font_names_is_left_and_named() {
    let source = pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 80] \
             /Resources << /Font << /F0 4 0 R /F1 5 0 R >> >> /Contents 7 0 R >>",
        ),
        object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"),
        object(
            "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 750 750] \
             /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /square 6 0 R >> \
             /Encoding << /Type /Encoding /Differences [65 /square] >> \
             /FirstChar 65 /LastChar 65 /Widths [1000] \
             /Resources << /Font << /F0 4 0 R >> >> >>",
        ),
        stream("", b"1000 0 0 0 750 750 d1 0 0 750 750 re f"),
        stream("", b"BT /F0 24 Tf 12 30 Td (AB) Tj /F1 24 Tf (A) Tj ET"),
    ]);
    let report = report_of(&embedded(
        &source,
        &provider(sans().truetype()),
        FontPolicy::Keep,
    ));
    assert!(report.embedded.is_empty(), "{report:?}");
    assert_eq!(
        report.left,
        [LeftFont {
            font: Some(ObjRef::new(4, 0)),
            base_font: "Helvetica".to_string(),
            reason: LeftReason::Type3Resource,
        }]
    );
}

/// A composite font is left, named, by the pass as it embeds simple fonts.
#[test]
fn a_composite_font_is_left_and_named() {
    let source = pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 80] \
             /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>",
        ),
        object(
            "<< /Type /Font /Subtype /Type0 /BaseFont /KozMinPr6N-Regular \
             /Encoding /Identity-H /DescendantFonts [5 0 R] >>",
        ),
        object(
            "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /KozMinPr6N-Regular \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 6 >> \
             /FontDescriptor 6 0 R /DW 1000 >>",
        ),
        object(
            "<< /Type /FontDescriptor /FontName /KozMinPr6N-Regular /Flags 4 \
             /FontBBox [0 -120 1000 880] /ItalicAngle 0 /Ascent 880 /Descent -120 \
             /CapHeight 740 /StemV 80 >>",
        ),
        stream("", b"BT /F0 24 Tf 12 30 Td <0022> Tj ET"),
    ]);
    let report = report_of(&embedded(
        &source,
        &provider(sans().truetype()),
        FontPolicy::Keep,
    ));
    assert!(report.embedded.is_empty());
    assert_eq!(
        report.left,
        [LeftFont {
            font: Some(ObjRef::new(4, 0)),
            base_font: "KozMinPr6N-Regular".to_string(),
            reason: LeftReason::Composite,
        }]
    );
}
