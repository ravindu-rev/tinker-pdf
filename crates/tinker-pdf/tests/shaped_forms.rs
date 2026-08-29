//! Milestone 8 of `docs/design/shaping.md`: a form field's value, shaped.
//!
//! Its exit criterion is that `crates/tinker-pdf-cos/src/fill.rs`'s `escape`
//! *"no longer writes `?` for characters above the single-byte range"*, and
//! that an Arabic value fills a text field with **joined** forms. Both are
//! asserted here, against a face this file synthesises, so nothing depends on
//! a font anybody has to licence and the expected glyph indices are ones the
//! test can name rather than read back out of the thing it is testing.
//!
//! # Why the blocker was where it was
//!
//! `text_appearance` reaches its font through the AcroForm `/DR`, and until
//! this milestone `tinker_pdf_cos::Font` exposed every width and no outline —
//! so there was nothing for `tinker-pdf-shape` to shape *against*.
//! `Font::program` is the entry that closed it: it walks
//! `/DescendantFonts` → `/FontDescriptor` → `/FontFile2` and returns the
//! stream's address, which the fill path decodes when it has a reason to.
//!
//! # What the fixture face proves, and what it cannot
//!
//! Its `GSUB` gives every covered letter an initial, a medial and a final
//! glyph, in three contiguous blocks whose indices `Face::form_glyph`
//! predicts. So a word of three letters shaped through it produces three
//! glyphs that are **not** the three the `cmap` alone would give, and the
//! appearance stream can be checked against exact numbers. What it does not
//! prove is that a real Nasta‘līq face joins correctly — that is
//! `crates/tinker-pdf-shape/tests/text_rendering.rs`'s SHARAN-1, on the real
//! font, and `docs/features/fonts.md` keeps the two claims apart.
//!
//! # The three defects, reintroduced and counted
//!
//! Each was put back into `crates/tinker-pdf-cos/src/fill.rs` and this file
//! run against it, so the numbers below are measured rather than expected:
//!
//! | Defect reintroduced | Tests that caught it |
//! | --- | --- |
//! | `Composite::of` always answers `None`, so the shaped path is never taken | 2 of 6 |
//! | A right-to-left run's glyphs are not reversed for drawing | 1 of 6 |
//! | `escape` writes `?` and names nothing, as it did before milestone 8 | 2 of 6 |
//!
//! The middle row is the thin one, and deliberately: only
//! [`an_arabic_value_fills_a_field_with_joined_forms_in_visual_order`] reads
//! the *order* of the codes, because it is the only test here that can — the
//! others are about which glyph, or about what was said, and a word drawn
//! backwards is neither.

mod epub_support;

use std::sync::Arc;

use epub_support::typeface::{Face, Form, Joining};
use tinker_pdf_cos::{
    CosDocument, DocumentEditor, ObjRef, ProgramKey, WarningKind, WriteMode, WriteOptions,
};

/// Three dual-joining Arabic letters: beh, hah, meem. Every one of them joins
/// on both sides, so the word takes an initial, a medial and a final form and
/// exercises all three lookups.
const WORD: &str = "\u{628}\u{62D}\u{645}";

/// A character the fixture face deliberately does **not** cover, so the
/// warning path has something to fire on.
const UNCOVERED: char = '\u{6A9}';

/// The fixture face: the three letters of [`WORD`], joining under `arab`.
fn joining_face() -> Face {
    Face::new("Fixture Arabic", WORD).with_joining(Joining { script: *b"arab" })
}

/// A one-page document with one text field whose `/DA` names a composite font
/// embedding `program`.
///
/// Written out by hand rather than through `DocumentBuilder`, for the reason
/// `crates/tinker-pdf-cos/tests/form_transactions.rs` gives for the same
/// choice: the builder has no AcroForm, and a fixture whose object graph is
/// visible in the test is one whose failure is readable.
fn form_document(program: &[u8], encoding: &str) -> Arc<CosDocument> {
    let mut pdf: Vec<u8> = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.7\n");
    pdf.extend_from_slice(
        b"1 0 obj\n\
<< /Type /Catalog /Pages 2 0 R /AcroForm\n\
   << /Fields [10 0 R] /DA (/F0 12 Tf 0 g)\n\
      /DR << /Font << /F0 5 0 R >> >> >> >>\nendobj\n",
    );
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
   /Annots [10 0 R] >>\nendobj\n",
    );
    pdf.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Fixture\n   \
             /Encoding /{encoding} /DescendantFonts [6 0 R] >>\nendobj\n"
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(
        b"6 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture\n\
   /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
   /FontDescriptor 7 0 R /DW 500 /CIDToGIDMap /Identity >>\nendobj\n",
    );
    pdf.extend_from_slice(
        b"7 0 obj\n<< /Type /FontDescriptor /FontName /Fixture /Flags 4\n\
   /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200\n\
   /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>\nendobj\n",
    );
    let length = program.len();
    pdf.extend_from_slice(
        format!("8 0 obj\n<< /Length {length} /Length1 {length} >>\nstream\n").as_bytes(),
    );
    pdf.extend_from_slice(program);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf.extend_from_slice(
        b"10 0 obj\n<< /FT /Tx /T (name) /Rect [10 150 190 175]\n\
   /Subtype /Widget /Type /Annot >>\nendobj\n",
    );
    pdf.extend_from_slice(b"trailer\n<< /Size 11 /Root 1 0 R >>\n%%EOF\n");

    // Hand-written bytes carry no cross-reference table, so they open through
    // the repair scanner; a rewrite puts the same object graph inside a
    // well-formed file, which is what the fill path then edits.
    let raw = Arc::new(CosDocument::open(pdf).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// Fills the field and returns the widget's regenerated appearance stream.
///
/// Saved and reopened rather than read out of the editor's overlay, which is
/// the pattern `edit.rs`'s own appearance tests use: what a viewer will see is
/// what the writer emitted, and a stream still in the overlay has no encoded
/// bytes to decode.
fn filled(doc: &Arc<CosDocument>, value: &str) -> (Arc<CosDocument>, String) {
    let mut editor = DocumentEditor::new(Arc::clone(doc));
    assert!(editor.set_field_value("name", value), "the field filled");
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    let saved = Arc::new(CosDocument::open(saved).expect("the filled document reopens"));
    let widget = ObjRef { num: 10, gen: 0 };
    let form = saved
        .get(widget)
        .ok()
        .and_then(|o| o.as_dict().cloned())
        .and_then(|d| d.get_dict(saved.intern(b"AP")).cloned())
        .and_then(|ap| ap.get_ref(saved.intern(b"N")))
        .expect("the widget has a normal appearance");
    let content = saved
        .stream_decoded(form)
        .expect("the appearance stream decodes");
    let text = String::from_utf8_lossy(&content).into_owned();
    (saved, text)
}

/// Every character one filled document named as undrawable, in order.
fn unrepresentable(doc: &CosDocument) -> Vec<char> {
    doc.warnings()
        .iter()
        .filter_map(|warning| match warning.kind {
            WarningKind::FieldCharacterUnrepresentable { character } => Some(character),
            _ => None,
        })
        .collect()
}

/// The `/DA` font of the fixture form embeds a program, and the object model
/// can say where it is.
///
/// The blocker, closed, asserted on its own: everything below depends on this
/// and would fail confusingly if the walk were wrong.
#[test]
fn the_default_appearance_font_reaches_its_embedded_program() {
    let program = joining_face().build();
    let doc = form_document(&program, "Identity-H");
    let font =
        tinker_pdf_cos::font::at(&doc, ObjRef { num: 5, gen: 0 }).expect("the /DA font reads");
    let embedded = font
        .program()
        .expect("the composite font's descendant embeds a program");
    assert_eq!(
        embedded.key,
        ProgramKey::FontFile2,
        "the walk found the wrong descriptor key"
    );
    let bytes = doc
        .stream_decoded(embedded.stream)
        .expect("the program stream decodes");
    assert_eq!(
        bytes, program,
        "the bytes that came back are not the ones that went in"
    );
}

/// The criterion, in one assertion: an Arabic value fills with joined forms,
/// in the order they are drawn.
///
/// Beh is the first letter of the word and takes its **initial** form, hah is
/// enclosed and takes its **medial** one, meem is last and takes its **final**
/// one. The stream lists them right to left, because that is the order a
/// right-to-left run is drawn in and `fill.rs` applies UAX #9's rule L2 before
/// it writes anything.
#[test]
fn an_arabic_value_fills_a_field_with_joined_forms_in_visual_order() {
    let face = joining_face();
    let doc = form_document(&face.build(), "Identity-H");
    let (_, content) = filled(&doc, WORD);

    let beh = face
        .form_glyph('\u{628}', Form::Initial)
        .expect("beh has an initial form");
    let hah = face
        .form_glyph('\u{62D}', Form::Medial)
        .expect("hah has a medial form");
    let meem = face
        .form_glyph('\u{645}', Form::Final)
        .expect("meem has a final form");
    let drawn: Vec<String> = [meem, hah, beh]
        .iter()
        .map(|glyph| format!("<{glyph:04X}>"))
        .collect();

    for code in &drawn {
        assert!(
            content.contains(code.as_str()),
            "the appearance does not draw {code}:\n{content}"
        );
    }
    // In that order, and not merely present: a shaper that produced the right
    // three glyphs logically and never reordered them would satisfy the loop
    // above and draw the word backwards.
    let mut at = 0usize;
    for code in &drawn {
        let found = content[at..]
            .find(code.as_str())
            .unwrap_or_else(|| panic!("{code} is out of visual order:\n{content}"));
        at += found + code.len();
    }
    assert!(
        !content.contains('?'),
        "the appearance still writes a question mark:\n{content}"
    );
    // And the isolated glyphs the `cmap` alone would give are **not** there,
    // which is what makes this a test of shaping rather than of encoding.
    for ch in WORD.chars() {
        let plain = face.glyph_of(ch).expect("the face covers the word");
        assert!(
            !content.contains(&format!("<{plain:04X}>")),
            "the appearance drew the unjoined form of {ch:?}:\n{content}"
        );
    }
}

/// A character the face has no glyph for is **named**, not silently replaced.
///
/// Ruling 10: "it filled" and "it filled cleanly" have to be different
/// sentences. Before milestone 8 this path wrote `b'?'` and said nothing at
/// all, so a form whose Arabic value had become question marks was
/// indistinguishable from one that had been filled correctly.
#[test]
fn a_character_the_face_cannot_draw_is_named_against_its_field() {
    let face = joining_face();
    let doc = form_document(&face.build(), "Identity-H");
    let (saved, _) = filled(&doc, &format!("{WORD}{UNCOVERED}"));

    assert_eq!(
        unrepresentable(&doc),
        vec![UNCOVERED],
        "the character the face could not draw was not reported"
    );
    let against: Vec<Option<u32>> = doc
        .warnings()
        .iter()
        .filter(|w| matches!(w.kind, WarningKind::FieldCharacterUnrepresentable { .. }))
        .map(|w| w.object.map(|r| r.num))
        .collect();
    assert_eq!(
        against,
        vec![Some(10)],
        "the warning does not name the field it happened to"
    );
    // The document that came *back* is a different one and carries none of
    // this: a warning is about the fill that happened, not a property of the
    // file, and asserting it on the saved copy would be asserting nothing.
    assert!(unrepresentable(&saved).is_empty());
}

/// A font whose `/Encoding` is not `/Identity-H` keeps the single-byte path,
/// and says which characters it could not write.
///
/// The refusal is deliberate and is the honest half of milestone 8: going from
/// a glyph back to a *code* needs the encoding CMap read backwards, and
/// 9.7.5's CMaps are written to be read forwards. A build that guessed would
/// draw a different wrong glyph, which is worse than a question mark that
/// announces itself.
#[test]
fn a_composite_font_under_another_cmap_declines_and_says_so() {
    let face = joining_face();
    let doc = form_document(&face.build(), "UniJIS-UCS2-H");
    let (_, content) = filled(&doc, WORD);

    assert!(
        content.contains("(???)"),
        "the declined path did not fall back to the single-byte one:\n{content}"
    );
    assert_eq!(
        unrepresentable(&doc),
        WORD.chars().collect::<Vec<_>>(),
        "the single-byte fallback wrote question marks and named nothing"
    );
}

/// The document the fill produced is clean under the strict structural
/// validator.
///
/// A shaped appearance is a content stream this engine writes by hand, and a
/// producer that emitted a `TJ` array with an unbalanced bracket or a hex
/// string of odd length would still draw *something* in a lenient viewer.
#[test]
fn the_filled_document_is_clean_under_the_strict_validator() {
    let face = joining_face();
    let doc = form_document(&face.build(), "Identity-H");
    let (saved, _) = filled(&doc, WORD);
    let problems = tinker_pdf_cos::validate(&saved);
    assert!(
        problems.is_empty(),
        "the filled document is not structurally clean: {problems:?}"
    );
}

/// `/V` survives the fill and comes back as the string that was written.
///
/// The other half of the exit criterion, and the one a shaped appearance can
/// silently destroy: a producer that wrote the glyphs and lost the text would
/// draw a document that reads correctly and searches as nothing.
#[test]
fn the_value_round_trips_through_the_saved_document() {
    let face = joining_face();
    let doc = form_document(&face.build(), "Identity-H");
    let (saved, _) = filled(&doc, WORD);
    let value = tinker_pdf_cos::fields(&saved)
        .into_iter()
        .find(|field| field.name == "name")
        .map(|field| field.value.as_text())
        .expect("the field is still there");
    assert_eq!(value, WORD, "the value did not survive the fill");
}
