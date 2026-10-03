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
//! # Which encodings, and which are still refused
//!
//! Milestone 8 landed shaped filling for `/Identity-H` and nothing else,
//! because going from a glyph back to a *code* means reading an encoding CMap
//! backwards and 9.7.5's are written to be read forwards. "Written to be read
//! forwards" turned out not to be "not invertible": `CMap::code_for_cid`
//! gathers every code the CMap's own tables could have meant by a CID and
//! returns the first that maps **back**, so the round trip is checked rather
//! than assumed. Four encodings now fill:
//!
//! - `/Identity-H`, where the code is the CID outright;
//! - an **embedded CMap stream**, whose tables are the document's own and
//!   need nothing this build might not have compiled in;
//! - a **predefined registry CMap** of 9.7.5.2, where this build compiled its
//!   table in;
//! - and any of those over a non-identity `/CIDToGIDMap`, which is what every
//!   subset font in the wild has.
//!
//! # Three refusals that closed in October 2026
//!
//! They were named by the ROADMAP's shaping-consumers row, and each now has a
//! fixture whose expected operators are computed in the test:
//!
//! - **A vertical CMap** is written as a column down the box's centre line,
//!   each glyph where its own `/DW2` displacement puts it
//!   ([`a_vertical_encoding_is_written_as_a_column`],
//!   [`quadding_runs_down_a_column`]).
//! - **A bare CFF** under a `CIDFontType0` is wrapped, per line, in the
//!   smallest sfnt the shaper can read — a `cmap` from the font's own
//!   `/ToUnicode` read backwards and an `hmtx` from `/W` — and drawn at `/W`'s
//!   advances ([`a_bare_cff_is_shaped_through_a_wrapper_and_drawn`] and the
//!   three beside it). Nothing joins: a CFF carries nothing to join with.
//! - **A simple TrueType `/DA` font** is shaped, each glyph written as the
//!   lowest byte its encoding reaches it by, so `GPOS` positions reach the
//!   field ([`a_simple_font_carries_gpos_positions_into_the_field`]); a line
//!   needing a glyph no byte reaches keeps the single-byte path whole.
//!
//! Still refused, each with a typed warning naming the field and the
//! character (ruling 10): a vertical CMap over a CFF, a CFF with no
//! `/ToUnicode`, a vertical comb field, a symbolic or non-TrueType simple
//! font, and — in a build without `cmap-predefined` — every registry CMap,
//! whose refusal names the missing table rather than writing a silent `?`.
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
//! # The defects, reintroduced and counted
//!
//! Each was put back into the source and the suites below run against it, so
//! these are measurements rather than expectations. Twelve tests compile here
//! in either build — the registry pair swap places. The other two columns are
//! where the leaf crate's own guards live: `cmap.rs`'s thirty unit tests, and
//! the thirteen in `tinker-pdf-font/tests/predefined_cmaps.rs` that read
//! Adobe's vendored text rather than this engine's output.
//!
//! | Defect reintroduced | Here | `cmap.rs` | registry |
//! | --- | --- | --- | --- |
//! | `Composite::of` always answers `Shaping::No`, so the shaped path is never taken | 7 of 12 | 0 of 30 | 0 of 13 |
//! | A right-to-left run's glyphs are not reversed for drawing | 6 of 12 | 0 of 30 | 0 of 13 |
//! | `escape` writes `?` and names nothing, as it did before milestone 8 | 2 of 12 | 0 of 30 | 0 of 13 |
//! | `Composite::of` shapes a registry CMap whose table is absent instead of refusing | 1 of 12, `--no-default-features` | 0 of 30 | — |
//! | `report` drops the refusal and emits only the characters | 1 of 12, `--no-default-features` | 0 of 30 | — |
//! | `CMap::code_for_cid` answers its first candidate without checking it maps back | 1 of 12 | 1 of 30 | 0 of 13 |
//! | `CMap::code_width` always answers two bytes | 1 of 12 | 1 of 30 | 1 of 13 |
//! | `Font::cid_for_gid` answers the glyph as its own CID, ignoring `/CIDToGIDMap` | 1 of 12 | 0 of 30 | 0 of 13 |
//! | `Composite::of` shapes a vertical CMap as though it were horizontal | 1 of 12 | 0 of 30 | 0 of 13 |
//!
//! The table above was measured over the twelve tests milestone 8 left, when
//! the type was `Composite`; it is `Shapeable` since it stopped being only
//! that. The October 2026 widening's own campaign, over all nineteen, is in
//! its commit message, row by row, and is not repeated here.
//!
//! The sixth row's zero in the last column is worth reading rather than
//! skipping: `90ms-RKSJ-H` states no single that overrides a range it also
//! covers, so the registry suite cannot see an unverified inverse. That guard
//! is held up by the two tests that build a CMap which does.
//!
//! The rows of one are the interesting ones: each names a guard that exactly
//! one test stands behind, so removing that test removes the property. Row
//! two is broad now for a reason worth stating — [`assert_draws`] compares
//! the whole ordered list of codes, so every test that uses it reads order as
//! well as content, where before milestone 8's widening only
//! [`an_arabic_value_fills_a_field_with_joined_forms_in_visual_order`] did.

mod epub_support;

use std::sync::Arc;

use epub_support::typeface::{Face, Form, Joining, Ligature, Placement};
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

/// The three glyphs [`WORD`] shapes to, in the order they are **drawn**:
/// meem's final form first, because a right-to-left run is drawn from its
/// last letter leftward.
fn drawn_glyphs(face: &Face) -> [u16; 3] {
    [
        face.form_glyph('\u{645}', Form::Final)
            .expect("meem has a final form"),
        face.form_glyph('\u{62D}', Form::Medial)
            .expect("hah has a medial form"),
        face.form_glyph('\u{628}', Form::Initial)
            .expect("beh has an initial form"),
    ]
}

/// An embedded CMap: two-byte codes, CID *n* at code `0x0FFF + n`.
///
/// The document's own table, so a build that compiled no registry in still
/// has everything it needs to write through it. The codes start at `0x1000`
/// so that no expected code is also a plausible CID, glyph index or ASCII
/// byte — a test whose expected number could arrive three ways proves the
/// least of them.
const EMBEDDED_CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
     begincmap\n\
     /CMapName /Fixture-H def\n\
     1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
     1 begincidrange <1000> <10FF> 1 endcidrange\n\
     endcmap end";

/// The same, in a **one-byte** codespace: CID *n* at code `0x0F + n`.
///
/// 9.7.6.2's widths are the half of a CMap a writer cannot guess. A code
/// written two bytes wide where the codespace says one does not merely draw
/// the wrong glyph — it mis-splits every code after it.
const NARROW_CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
     begincmap\n\
     /CMapName /Fixture-Narrow def\n\
     1 begincodespacerange <00> <FF> endcodespacerange\n\
     1 begincidrange <10> <3F> 1 endcidrange\n\
     endcmap end";

/// [`EMBEDDED_CMAP`] with one code **stolen back**: `<1003>` was CID 4 by the
/// range and is CID 999 by the single that follows it.
///
/// 9.7.5.3 makes the single win, so after it nothing means CID 4 any more.
/// The point is what a writer does about that: a naive inverse walks the
/// range, produces `<1003>` for CID 4 and draws CID 999's glyph — a
/// *different* wrong glyph, which is the failure a question mark is better
/// than.
const OVERRIDING_CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
     begincmap\n\
     /CMapName /Fixture-Stolen def\n\
     1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
     1 begincidrange <1000> <10FF> 1 endcidrange\n\
     1 begincidchar <1003> 999 endcidchar\n\
     endcmap end";

/// A `/CIDToGIDMap` stream sending CID `100 + g` to glyph `g`, for every
/// glyph up to `glyphs`.
///
/// 9.7.4.2: a subsetter writes this form because subsetting renumbers glyphs
/// and the CIDs must not move with them, which is why almost every embedded
/// CJK font in the wild has one. Every CID below 100 maps to `.notdef`, so a
/// writer that ignored the table and handed back the glyph as its own CID
/// would draw nothing rather than draw something slightly wrong.
fn shifted_cid_to_gid(glyphs: u16) -> Vec<u8> {
    let mut out = Vec::new();
    for cid in 0..=(100 + glyphs) {
        let gid = cid.saturating_sub(100);
        out.extend_from_slice(&gid.to_be_bytes());
    }
    out
}

/// The composite font a fixture document's `/DA` names.
struct Fixture<'a> {
    /// The embedded program.
    program: &'a [u8],
    /// The descriptor key it hangs from: `FontFile2` for an sfnt,
    /// `FontFile3` for the bare CFF this build refuses to shape against.
    program_key: &'a str,
    /// The `/Encoding` value, written verbatim — a name like `/Identity-H`,
    /// or `20 0 R` when `cmap` supplies a stream.
    encoding: &'a str,
    /// An embedded CMap stream, placed at object 20.
    cmap: Option<&'a [u8]>,
    /// The `/CIDToGIDMap` value, written verbatim — `/Identity`, or `21 0 R`
    /// when `cid_to_gid` supplies a stream.
    cid_to_gid_map: &'a str,
    /// A `/CIDToGIDMap` stream, placed at object 21.
    cid_to_gid: Option<Vec<u8>>,
    /// The descendant's `/Subtype`: `CIDFontType2` over an sfnt,
    /// `CIDFontType0` over a CFF.
    descendant: &'a str,
    /// The descendant's widths, written verbatim — `/DW 500`, or a `/W`.
    widths: &'a str,
    /// The program stream's own `/Subtype`, which `/FontFile3` requires.
    program_subtype: Option<&'a str>,
    /// A `/ToUnicode` CMap stream for the Type 0 font, placed at object 22.
    to_unicode: Option<&'a [u8]>,
    /// The field's `/Q`: 0 left, 1 centred, 2 right (12.7.4.3).
    quadding: i64,
}

impl<'a> Fixture<'a> {
    /// The plain case: an sfnt under a named encoding, `/CIDToGIDMap
    /// /Identity`.
    fn named(program: &'a [u8], encoding: &'a str) -> Fixture<'a> {
        Fixture {
            program,
            program_key: "FontFile2",
            encoding,
            cmap: None,
            cid_to_gid_map: "/Identity",
            cid_to_gid: None,
            descendant: "CIDFontType2",
            widths: "/DW 500",
            program_subtype: None,
            to_unicode: None,
            quadding: 0,
        }
    }

    /// An sfnt under the embedded CMap stream `cmap`.
    fn embedded(program: &'a [u8], cmap: &'a [u8]) -> Fixture<'a> {
        Fixture {
            cmap: Some(cmap),
            encoding: "20 0 R",
            ..Fixture::named(program, "")
        }
    }
}

/// A one-page document with one text field whose `/DA` names `fixture`.
///
/// Written out by hand rather than through `DocumentBuilder`, for the reason
/// `crates/tinker-pdf-cos/tests/form_transactions.rs` gives for the same
/// choice: the builder has no AcroForm, and a fixture whose object graph is
/// visible in the test is one whose failure is readable.
fn form_document(fixture: &Fixture<'_>) -> Arc<CosDocument> {
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
             /Encoding {} /DescendantFonts [6 0 R]{} >>\nendobj\n",
            fixture.encoding,
            if fixture.to_unicode.is_some() {
                " /ToUnicode 22 0 R"
            } else {
                ""
            }
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /Font /Subtype /{} /BaseFont /Fixture\n   \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n   \
             /FontDescriptor 7 0 R {} /CIDToGIDMap {} >>\nendobj\n",
            fixture.descendant, fixture.widths, fixture.cid_to_gid_map
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(
        format!(
            "7 0 obj\n<< /Type /FontDescriptor /FontName /Fixture /Flags 4\n   \
             /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200\n   \
             /CapHeight 700 /StemV 80 /{} 8 0 R >>\nendobj\n",
            fixture.program_key
        )
        .as_bytes(),
    );
    let length = fixture.program.len();
    let subtype = fixture
        .program_subtype
        .map(|name| format!(" /Subtype /{name}"))
        .unwrap_or_default();
    pdf.extend_from_slice(
        format!("8 0 obj\n<< /Length {length} /Length1 {length}{subtype} >>\nstream\n").as_bytes(),
    );
    pdf.extend_from_slice(fixture.program);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf.extend_from_slice(
        format!(
            "10 0 obj\n<< /FT /Tx /T (name) /Rect [10 150 190 175] /Q {}\n   \
             /Subtype /Widget /Type /Annot >>\nendobj\n",
            fixture.quadding
        )
        .as_bytes(),
    );
    if let Some(cmap) = fixture.cmap {
        pdf.extend_from_slice(
            format!("20 0 obj\n<< /Length {} >>\nstream\n", cmap.len()).as_bytes(),
        );
        pdf.extend_from_slice(cmap);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }
    if let Some(table) = &fixture.cid_to_gid {
        pdf.extend_from_slice(
            format!("21 0 obj\n<< /Length {} >>\nstream\n", table.len()).as_bytes(),
        );
        pdf.extend_from_slice(table);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }
    if let Some(cmap) = fixture.to_unicode {
        pdf.extend_from_slice(
            format!("22 0 obj\n<< /Length {} >>\nstream\n", cmap.len()).as_bytes(),
        );
        pdf.extend_from_slice(cmap);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }
    pdf.extend_from_slice(b"trailer\n<< /Size 23 /Root 1 0 R >>\n%%EOF\n");

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
/// the pattern `edit/tests.rs`'s own appearance tests use: what a viewer will see is
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

/// Every hex string the appearance draws, as `(value, byte width)`, in the
/// order it draws them.
///
/// The width is half the digit count, which is what 9.7.6.2 makes it: a code
/// is as many bytes as its codespace says and the string carries no
/// separators, so `<41>` and `<0041>` are different strings that mean
/// different things.
fn codes(content: &str) -> Vec<(u32, u8)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(open) = content[at..].find('<') {
        let start = at + open + 1;
        let Some(close) = content[start..].find('>') else {
            break;
        };
        let digits = &content[start..start + close];
        at = start + close + 1;
        if digits.is_empty()
            || digits.len() % 2 != 0
            || !digits.bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        let value = u32::from_str_radix(digits, 16).expect("hex digits");
        out.push((value, (digits.len() / 2) as u8));
    }
    out
}

/// Asserts that `content` draws exactly `expected`, in that order.
fn assert_draws(content: &str, expected: &[(u32, u8)]) {
    assert_eq!(
        codes(content),
        expected,
        "the appearance draws the wrong codes:\n{content}"
    );
    assert!(
        !content.contains('?'),
        "the appearance still writes a question mark:\n{content}"
    );
}

/// The `/DA` font of the fixture form embeds a program, and the object model
/// can say where it is.
///
/// The blocker, closed, asserted on its own: everything below depends on this
/// and would fail confusingly if the walk were wrong.
#[test]
fn the_default_appearance_font_reaches_its_embedded_program() {
    let program = joining_face().build();
    let doc = form_document(&Fixture::named(&program, "/Identity-H"));
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
    let doc = form_document(&Fixture::named(&face.build(), "/Identity-H"));
    let (_, content) = filled(&doc, WORD);

    let drawn: Vec<String> = drawn_glyphs(&face)
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
    let doc = form_document(&Fixture::named(&face.build(), "/Identity-H"));
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

/// A font whose `/Encoding` is an **embedded CMap stream** fills with joined
/// forms, and does it in every build.
///
/// This is the half of the widening that owes nothing to a cargo feature. The
/// registry's code-to-CID tables are a megabyte behind `cmap-predefined`; a
/// document's own CMap is in the document, so inverting it needs only the
/// inverter. The expected codes are exactly the ones [`EMBEDDED_CMAP`]
/// declares, so this test names its numbers rather than deriving them from
/// the machinery under test.
#[test]
fn an_embedded_cmap_fills_with_joined_forms_and_needs_no_feature() {
    let face = joining_face();
    let program = face.build();
    let doc = form_document(&Fixture::embedded(&program, EMBEDDED_CMAP));
    let (_, content) = filled(&doc, WORD);

    // CID *n* sits at code 0x0FFF + n, and `/CIDToGIDMap /Identity` makes the
    // CID the glyph.
    let expected: Vec<(u32, u8)> = drawn_glyphs(&face)
        .iter()
        .map(|glyph| (0x0FFF + u32::from(*glyph), 2u8))
        .collect();
    assert_draws(&content, &expected);
}

/// A CMap whose codespace is **one byte** gets one-byte codes.
///
/// 9.7.6.2 is what decides where one code ends and the next begins, and a
/// writer that always emitted two bytes would produce a string this same
/// engine reads back as half as many codes, every one of them wrong. The
/// codespace tables are compiled into every build — it is the code-to-CID
/// megabyte that `cmap-predefined` gates — so this holds either way.
#[test]
fn a_one_byte_codespace_is_written_one_byte_wide() {
    let face = joining_face();
    let program = face.build();
    let doc = form_document(&Fixture::embedded(&program, NARROW_CMAP));
    let (_, content) = filled(&doc, WORD);

    let expected: Vec<(u32, u8)> = drawn_glyphs(&face)
        .iter()
        .map(|glyph| (0x0F + u32::from(*glyph), 1u8))
        .collect();
    assert_draws(&content, &expected);
    assert!(
        !content.contains("<00"),
        "a one-byte code was padded to two:\n{content}"
    );
}

/// A code another mapping took over is **refused**, not written.
///
/// The inverse of a CMap is not a function, and this is the case that makes
/// the difference visible: `<1003>` is CID 4 by [`OVERRIDING_CMAP`]'s range
/// and CID 999 by the single that overrides it, so after the override nothing
/// means CID 4. `code_for_cid` checks every candidate **forwards** before
/// answering, so beh — whose initial form is glyph 4 — comes back as a
/// character this appearance cannot draw and is named, while the other two
/// letters draw normally. A writer that inverted the range and stopped would
/// emit `<1003>` and silently draw CID 999's glyph instead.
#[test]
fn a_code_another_mapping_took_over_is_refused_rather_than_written() {
    let face = joining_face();
    let program = face.build();
    let doc = form_document(&Fixture::embedded(&program, OVERRIDING_CMAP));
    let (_, content) = filled(&doc, WORD);

    let [meem, hah, beh] = drawn_glyphs(&face);
    assert_eq!(beh, 4, "the fixture's beh is the glyph the CMap steals");
    assert_eq!(
        codes(&content),
        vec![(0x0FFF + u32::from(meem), 2), (0x0FFF + u32::from(hah), 2)],
        "the two letters whose codes survive are the two that are drawn:\n{content}"
    );
    assert!(
        !content.contains("<1003>"),
        "the stolen code was written anyway, and draws CID 999:\n{content}"
    );
    assert_eq!(
        unrepresentable(&doc),
        vec!['\u{628}'],
        "the letter with no code left was not named"
    );
}

/// A non-identity `/CIDToGIDMap` is inverted on the way out (9.7.4.2).
///
/// Almost every embedded CJK font in the wild has one, because subsetting
/// renumbers glyphs and the CIDs must not move with them. Nothing drove this
/// path through a fill before: the fixture's map sends CID `100 + g` to glyph
/// `g`, so a writer that handed the shaped glyph back as its own CID would
/// name a CID that maps to `.notdef` and draw nothing at all.
#[test]
fn a_non_identity_cid_to_gid_map_is_inverted_on_the_way_out() {
    let face = joining_face();
    let program = face.build();
    let glyphs = drawn_glyphs(&face);
    let doc = form_document(&Fixture {
        cid_to_gid_map: "21 0 R",
        cid_to_gid: Some(shifted_cid_to_gid(
            *glyphs.iter().max().expect("three glyphs"),
        )),
        ..Fixture::named(&program, "/Identity-H")
    });
    let (_, content) = filled(&doc, WORD);

    // `/Identity-H` makes the code the CID, and the map makes the CID the
    // glyph plus a hundred.
    let expected: Vec<(u32, u8)> = glyphs
        .iter()
        .map(|glyph| (100 + u32::from(*glyph), 2u8))
        .collect();
    assert_draws(&content, &expected);
    for glyph in glyphs {
        assert!(
            !content.contains(&format!("<{glyph:04X}>")),
            "the glyph was written as its own CID, ignoring /CIDToGIDMap:\n{content}"
        );
    }
}

/// A **vertical** CMap is written as a column, down the middle of the box.
///
/// 9.7.4.3: a vertical writing mode advances the pen downward by each CID's
/// `w1`, and draws each glyph displaced by its position vector, whose
/// horizontal half is half the glyph's width. So the column's pen goes on the
/// box's vertical centre line, starting at the top less the padding, and the
/// glyphs are written in the order the value was typed — top to bottom — with
/// no `TJ` numbers, because every glyph sits where a reader's own `/DW2`
/// displacement puts it.
///
/// Everything asserted is computed here rather than read back: the codes are
/// the face's own glyph indices (`/Identity-V` and `/CIDToGIDMap /Identity`
/// make code, CID and glyph one number), and the origin is the widget's
/// rectangle — 180 by 25 — halved across and less two points of padding down.
#[test]
fn a_vertical_encoding_is_written_as_a_column() {
    let face = Face::new("Fixture Latin", "ABC");
    let doc = form_document(&Fixture::named(&face.build(), "/Identity-V"));
    let (_, content) = filled(&doc, "ABC");

    let expected: Vec<(u32, u8)> = "ABC"
        .chars()
        .map(|ch| (u32::from(face.glyph_of(ch).expect("covered")), 2))
        .collect();
    assert_draws(&content, &expected);
    assert!(
        content.contains("1 0 0 1 90.00 23.00 Tm"),
        "the column does not start at the top of the box's centre line:\n{content}"
    );
    let run: String = expected
        .iter()
        .map(|(code, _)| format!("<{code:04X}>"))
        .collect();
    assert!(
        content.contains(&format!("[{run}] TJ")),
        "the column carries a position a reader's own metrics would not give:\n{content}"
    );
    assert!(
        unrepresentable(&doc).is_empty(),
        "a column that drew every character named one as undrawable"
    );
}

/// Quadding reads **down** a column: `/Q 1` centres it in the box and `/Q 2`
/// ends it at the bottom.
///
/// One glyph at twelve points under `/DW2`'s default `w1` of −1000 is twelve
/// points of column. In a box 25 high with two points of padding, a column
/// that starts at the top starts at 23; a centred one at `(25 + 12) / 2 =
/// 18.5`; one that ends at the bottom at `2 + 12 = 14`.
#[test]
fn quadding_runs_down_a_column() {
    let face = Face::new("Fixture Latin", "ABC");
    let program = face.build();
    for (quadding, top) in [(0, "23.00"), (1, "18.50"), (2, "14.00")] {
        let doc = form_document(&Fixture {
            quadding,
            ..Fixture::named(&program, "/Identity-V")
        });
        let (_, content) = filled(&doc, "A");
        assert!(
            content.contains(&format!("1 0 0 1 90.00 {top} Tm")),
            "/Q {quadding} does not put the column's top at {top}:\n{content}"
        );
    }
}

/// A program that is neither an sfnt nor a CFF is refused, and every
/// character is named.
///
/// Eight bytes of CFF header and nothing after it: the shape of a `/FontFile3`
/// that is damaged rather than one that is merely a CFF. Neither `Sfnt::parse`
/// nor `Cff::parse` may read past its end, and with nothing to shape against
/// the refusal has to reach the caller as characters rather than as silence,
/// which is the whole of ruling 10 here.
#[test]
fn a_program_that_is_neither_sfnt_nor_cff_is_refused_and_every_character_is_named() {
    let program: Vec<u8> = vec![0x01, 0x00, 0x04, 0x01, 0x00, 0x01, 0x01, 0x01];
    let doc = form_document(&Fixture {
        program_key: "FontFile3",
        ..Fixture::named(&program, "/Identity-H")
    });
    let (_, content) = filled(&doc, WORD);

    assert!(
        content.contains("(???)"),
        "a program that is no font was shaped against:\n{content}"
    );
    assert_eq!(
        unrepresentable(&doc),
        WORD.chars().collect::<Vec<_>>(),
        "the refusal named no characters"
    );
    let against: Vec<Option<u32>> = doc
        .warnings()
        .iter()
        .filter(|w| matches!(w.kind, WarningKind::FieldCharacterUnrepresentable { .. }))
        .map(|w| w.object.map(|r| r.num))
        .collect();
    assert_eq!(
        against,
        vec![Some(10); 3],
        "the warnings do not name the field they happened to"
    );
}

// ---- a bare CFF ----------------------------------------------------------------

/// A CFF INDEX with four-byte offsets.
fn cff_index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(4);
    let mut offset = 1u32;
    out.extend_from_slice(&offset.to_be_bytes());
    for item in items {
        offset += item.len() as u32;
        out.extend_from_slice(&offset.to_be_bytes());
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// One DICT entry, operands in the fixed five-byte form so offsets can be
/// computed before they are known.
fn cff_entry(op: u16, operands: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in operands {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }
    if op > 0xFF {
        out.push(12);
        out.push((op & 0xFF) as u8);
    } else {
        out.push(op as u8);
    }
    out
}

/// A **CID-keyed** CFF whose glyphs 1, 2 and 3 carry CIDs 10, 11 and 12, each
/// an empty charstring: `ROS`, a format 0 charset, a format 3 FDSelect with
/// every glyph in Font DICT 0, and one Private DICT. The shape is
/// `crates/tinker-pdf/tests/cff_fonts.rs`'s `Program` with `cid: true`,
/// trimmed to what this file needs.
fn cid_keyed_cff() -> Vec<u8> {
    let private = cff_entry(20, &[600]);
    let header = [1u8, 0, 4, 4];
    let names = cff_index(&[b"Fixture".to_vec()]);
    let strings = cff_index(&[b"Adobe".to_vec(), b"Identity".to_vec()]);
    let gsubrs = cff_index(&[]);
    let charstrings = cff_index(&[vec![14], vec![14], vec![14], vec![14]]);
    let mut charset = vec![0u8];
    for cid in [10u16, 11, 12] {
        charset.extend_from_slice(&cid.to_be_bytes());
    }
    let fd_select = vec![3u8, 0, 1, 0, 0, 0, 0, 4];
    let top = |at: &[i32; 5]| {
        let mut out = cff_entry(0x0C1E, &[391, 392, 0]);
        out.extend(cff_entry(15, &[at[0]]));
        out.extend(cff_entry(17, &[at[1]]));
        out.extend(cff_entry(18, &[at[2], at[3]]));
        out.extend(cff_entry(0x0C24, &[at[4]]));
        out.extend(cff_entry(0x0C25, &[at[0] + charset.len() as i32]));
        out
    };
    let top_len = top(&[0; 5]).len();
    let mut cursor =
        header.len() + names.len() + (2 + 1 + 8 + top_len) + strings.len() + gsubrs.len();
    let charset_at = cursor;
    cursor += charset.len() + fd_select.len();
    let charstrings_at = cursor;
    cursor += charstrings.len();
    let private_at = cursor;
    cursor += private.len();
    let fd_array_at = cursor;
    let at = [
        charset_at as i32,
        charstrings_at as i32,
        private.len() as i32,
        private_at as i32,
        fd_array_at as i32,
    ];
    let mut out = header.to_vec();
    out.extend_from_slice(&names);
    out.extend_from_slice(&cff_index(&[top(&at)]));
    out.extend_from_slice(&strings);
    out.extend_from_slice(&gsubrs);
    out.extend_from_slice(&charset);
    out.extend_from_slice(&fd_select);
    out.extend_from_slice(&charstrings);
    out.extend_from_slice(&private);
    let font_dict = cff_entry(18, &[private.len() as i32, private_at as i32]);
    out.extend_from_slice(&cff_index(&[font_dict]));
    out
}

/// What the CFF font's codes mean: `A`, `B` and `C` at the CIDs the program
/// carries, and `D` at CID 13, which it does **not**.
const CFF_TO_UNICODE: &[u8] = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 begincodespacerange <0000> <FFFF> endcodespacerange
4 beginbfchar
<000A> <0041>
<000B> <0042>
<000C> <0043>
<000D> <0044>
endbfchar
endcmap
end end";

/// The CFF fixture: a CIDFontType0 over [`cid_keyed_cff`], `/Identity-H`,
/// widths 600, 300 and 450 for the three CIDs it carries.
fn cff_fixture(program: &[u8]) -> Fixture<'_> {
    Fixture {
        program_key: "FontFile3",
        program_subtype: Some("CIDFontType0C"),
        descendant: "CIDFontType0",
        widths: "/W [10 [600 300 450]]",
        to_unicode: Some(CFF_TO_UNICODE),
        ..Fixture::named(program, "/Identity-H")
    }
}

/// **A bare CFF is shaped through a wrapper**, and the value is drawn rather
/// than written as question marks.
///
/// `Shaper::new` takes an sfnt and a CFF is not one, so `fill.rs` wraps it:
/// each character of the line goes to the code its `/ToUnicode` gives it, the
/// code to a CID, and the CID is kept where the program's charset carries it.
/// The codes expected here are read off the fixture's own `/ToUnicode` table,
/// in the order the value was typed — the value reads left to right — and the
/// run carries no `TJ` numbers, because the wrapper's advances are `/W`'s and
/// a reader advances by `/W` too.
#[test]
fn a_bare_cff_is_shaped_through_a_wrapper_and_drawn() {
    let program = cid_keyed_cff();
    let doc = form_document(&cff_fixture(&program));
    let (_, content) = filled(&doc, "CAB");

    assert_draws(&content, &[(0x000C, 2), (0x000A, 2), (0x000B, 2)]);
    assert!(
        content.contains("[<000C><000A><000B>] TJ"),
        "the run moved a glyph from where /W puts it:\n{content}"
    );
    assert!(unrepresentable(&doc).is_empty());
}

/// **Positioned at `/W`'s advances**, which differ per glyph: in a box 180
/// wide with two points of padding, `/Q 1` and a twelve-point line of `C`,
/// `A` and `B`, the line is `(450 + 600 + 300) × 12 / 1000 = 16.2` points
/// wide, so it starts at `2 + (176 − 16.2) / 2 = 81.9`. A line measured at
/// any uniform advance would start somewhere else.
#[test]
fn a_bare_cff_line_is_measured_by_its_own_widths() {
    let program = cid_keyed_cff();
    let doc = form_document(&Fixture {
        quadding: 1,
        ..cff_fixture(&program)
    });
    let (_, content) = filled(&doc, "CAB");
    assert!(
        content.contains("1 0 0 1 81.90 "),
        "the line was not measured by the widths it is drawn at:\n{content}"
    );
}

/// What the wrapper cannot draw is **named**: a character the `/ToUnicode`
/// never mentions, and one it maps to a CID the program does not carry.
#[test]
fn a_bare_cff_names_what_it_cannot_draw() {
    let program = cid_keyed_cff();
    let doc = form_document(&cff_fixture(&program));
    let (_, content) = filled(&doc, "AEDB");
    assert_draws(&content, &[(0x000A, 2), (0x000B, 2)]);
    assert_eq!(
        unrepresentable(&doc),
        vec!['E', 'D'],
        "a character with no code and one whose CID the program lacks are \
         both named, in the order the value meets them"
    );
}

/// A `/ToUnicode` whose `bfrange` says `<000B>` is `B` and whose `bfchar`
/// says it is `X`: the `bfchar` wins when the code is read (9.10.3), so
/// `<000B>` means `X`.
const OVERRIDDEN_TO_UNICODE: &[u8] = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 beginbfchar
<000B> <0058>
endbfchar
1 beginbfrange
<000A> <000C> <0041>
endbfrange
endcmap
end end";

/// **A code another entry took over is not written for the character it no
/// longer means** (review of lane 6C).
///
/// Read backwards, the range alone answers `B` with `<000B>`; read forwards,
/// `<000B>` is `X`, so the field would have drawn and extracted `AX` for a
/// value of `AB` and said nothing. `B` has no code in this font and is named;
/// `X` is written with `<000B>`, because that is what it means.
#[test]
fn a_bare_cff_code_another_entry_took_over_is_not_written() {
    let program = cid_keyed_cff();
    let doc = form_document(&Fixture {
        to_unicode: Some(OVERRIDDEN_TO_UNICODE),
        ..cff_fixture(&program)
    });
    let (_, content) = filled(&doc, "ABX");
    assert_draws(&content, &[(0x000A, 2), (0x000B, 2)]);
    assert_eq!(unrepresentable(&doc), vec!['B']);
}

/// A bare CFF with **no** `/ToUnicode` keeps the single-byte path: nothing in
/// the document says which code means which character.
#[test]
fn a_bare_cff_without_a_to_unicode_is_refused() {
    let program = cid_keyed_cff();
    let doc = form_document(&Fixture {
        to_unicode: None,
        ..cff_fixture(&program)
    });
    let (_, content) = filled(&doc, "CAB");
    assert!(
        content.contains("(CAB) Tj"),
        "a CFF with nothing to read backwards was shaped:\n{content}"
    );
}

// ---- a simple TrueType font ------------------------------------------------------

/// A one-page form whose `/DA` font is a **simple** TrueType font over
/// `program`, under `/WinAnsiEncoding`, every code 500 wide — the advance the
/// synthesised faces give every glyph.
fn simple_form_document(program: &[u8]) -> Arc<CosDocument> {
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
    let widths: String = (32..=126).map(|_| "500 ").collect();
    pdf.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Fixture\n   \
             /FirstChar 32 /LastChar 126 /Widths [{widths}]\n   \
             /Encoding /WinAnsiEncoding /FontDescriptor 7 0 R >>\nendobj\n"
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(
        b"7 0 obj\n<< /Type /FontDescriptor /FontName /Fixture /Flags 32\n   \
          /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200\n   \
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
    let raw = Arc::new(CosDocument::open(pdf).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// **A simple `/DA` font is shaped, and `GPOS` positions reach the field.**
///
/// The face displaces `B` by 120 units along the baseline and 80 up, under
/// `kern`, without moving the pen. At a thousand units to the em both are
/// already thousandths of text space, so — computed here, not read back — `B`
/// is drawn 120 past where `A`'s advance leaves the pen (a `TJ` number of
/// −120) and raised 0.08 em (`Ts`), and `C` comes back 120 (a number of 120)
/// with the rise cleared. Each code is one byte, the character's
/// `WinAnsiEncoding` code.
#[test]
fn a_simple_font_carries_gpos_positions_into_the_field() {
    let face = Face::new("Fixture Latin", "ABC").with_placement(Placement {
        ch: 'B',
        script: *b"DFLT",
        feature: *b"kern",
        x: 120,
        y: 80,
    });
    let doc = simple_form_document(&face.build());
    let (_, content) = filled(&doc, "ABC");

    assert_draws(&content, &[(0x41, 1), (0x42, 1), (0x43, 1)]);
    for operator in ["0.08 Ts", "[-120 <42>] TJ", "0 Ts", "[120 <43>] TJ"] {
        assert!(
            content.contains(operator),
            "the positioned run lacks {operator:?}:\n{content}"
        );
    }
    assert!(unrepresentable(&doc).is_empty());
}

/// **A line that needs a glyph no byte reaches keeps the single-byte path.**
///
/// The face ligates `A` and `B` under `liga`, and a ligature has no code in
/// `WinAnsiEncoding`; so the shaped line would lose two letters. It is not
/// taken, and the field draws exactly what it drew before simple fonts were
/// shaped at all.
#[test]
fn a_simple_font_line_with_an_unreachable_glyph_keeps_the_byte_path() {
    let face = Face::new("Fixture Latin", "ABC").with_ligature(Ligature {
        first: 'A',
        second: 'B',
        script: *b"DFLT",
        feature: *b"liga",
    });
    let doc = simple_form_document(&face.build());
    let (_, content) = filled(&doc, "ABC");
    assert!(
        content.contains("(ABC) Tj"),
        "a line whose ligature no byte can name was written shaped:\n{content}"
    );
    assert!(unrepresentable(&doc).is_empty());
}

/// A **predefined registry CMap** fills with joined Arabic rather than `?`.
///
/// The refusal this replaces was milestone 8's honest half: glyph-to-code
/// needs the encoding CMap read backwards, and a build that guessed would
/// draw a different wrong glyph. Nothing here guesses — the codes are checked
/// **forwards**, through the same `CMap::cid` every reading path uses, and
/// that is what makes this a round trip rather than a restatement of the
/// inverter's own opinion.
///
/// `UniJIS-UCS2-H` is a real registry CMap with a real table, so the codes are
/// Adobe's rather than the fixture's; the test asserts what they mean rather
/// than what they are.
#[cfg(feature = "cmap-predefined")]
#[test]
fn a_registry_cmap_fills_with_joined_forms_rather_than_question_marks() {
    let face = joining_face();
    let doc = form_document(&Fixture::named(&face.build(), "/UniJIS-UCS2-H"));
    let (_, content) = filled(&doc, WORD);

    assert!(
        !content.contains('?'),
        "a registry CMap still falls back to question marks:\n{content}"
    );
    let cmap =
        tinker_pdf_font::CMap::predefined(b"UniJIS-UCS2-H").expect("the registry defines it");
    let written = codes(&content);
    for (_, bytes) in &written {
        assert_eq!(*bytes, 2, "UniJIS-UCS2-H is a two-byte codespace");
    }
    // `/CIDToGIDMap /Identity` makes the CID the glyph, so reading each code
    // forwards has to give back the glyph the shaper chose.
    let round_tripped: Vec<u32> = written
        .iter()
        .map(|(code, _)| {
            cmap.cid(*code)
                .unwrap_or_else(|| panic!("the CMap maps no CID to code {code:#06X}"))
        })
        .collect();
    let expected: Vec<u32> = drawn_glyphs(&face).iter().map(|g| u32::from(*g)).collect();
    assert_eq!(
        round_tripped, expected,
        "the codes written do not read back as the glyphs that were shaped:\n{content}"
    );
}

/// With the registry's tables left out, the refusal **names the missing
/// table** rather than writing a silent `?`.
///
/// A capability that quietly depends on a cargo feature is the failure this
/// repository already named once in PDF/A: a verdict that depends on a feature
/// is not a verdict, and neither is a fill. So a `--no-default-features` build
/// says which CMap it could not invert, against the field it could not fill,
/// and names every character it dropped — three sentences a caller can act on
/// instead of a row of question marks.
///
/// `font::read` emits the same typed kind when the font is merely *read*, with
/// no object attached. This asserts the one attached to the field, which is
/// the one that means a fill was refused.
#[cfg(not(feature = "cmap-predefined"))]
#[test]
fn a_registry_cmap_without_its_table_names_the_table_it_wanted() {
    let face = joining_face();
    let doc = form_document(&Fixture::named(&face.build(), "/UniJIS-UCS2-H"));
    let (_, content) = filled(&doc, WORD);

    assert!(
        content.contains("(???)"),
        "a CMap with no table was written as though it had one:\n{content}"
    );
    let named: Vec<Vec<u8>> = doc
        .warnings()
        .iter()
        .filter(|w| w.object == Some(ObjRef { num: 10, gen: 0 }))
        .filter_map(|w| match w.kind {
            WarningKind::PredefinedCMapApproximate(name) => {
                Some(doc.name_bytes(name).map(|b| b.to_vec()).unwrap_or_default())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        named,
        vec![b"UniJIS-UCS2-H".to_vec()],
        "the refusal did not name the CMap whose table is missing, against the field"
    );
    assert_eq!(
        unrepresentable(&doc),
        WORD.chars().collect::<Vec<_>>(),
        "the refusal named no characters"
    );
}

/// The document the fill produced is clean under the strict structural
/// validator.
///
/// A shaped appearance is a content stream this engine writes by hand, and a
/// producer that emitted a `TJ` array with an unbalanced bracket or a hex
/// string of odd length would still draw *something* in a lenient viewer. Both
/// encodings that reach the shaped path in every build are checked, because a
/// one-byte code is where an odd-length hex string would come from.
#[test]
fn the_filled_document_is_clean_under_the_strict_validator() {
    let face = joining_face();
    let program = face.build();
    for fixture in [
        Fixture::named(&program, "/Identity-H"),
        Fixture::embedded(&program, EMBEDDED_CMAP),
        Fixture::embedded(&program, NARROW_CMAP),
    ] {
        let doc = form_document(&fixture);
        let (saved, _) = filled(&doc, WORD);
        let problems = tinker_pdf_cos::validate(&saved);
        assert!(
            problems.is_empty(),
            "the filled document is not structurally clean: {problems:?}"
        );
    }
}

/// `/V` survives the fill and comes back as the string that was written.
///
/// The other half of the exit criterion, and the one a shaped appearance can
/// silently destroy: a producer that wrote the glyphs and lost the text would
/// draw a document that reads correctly and searches as nothing.
#[test]
fn the_value_round_trips_through_the_saved_document() {
    let face = joining_face();
    let program = face.build();
    for fixture in [
        Fixture::named(&program, "/Identity-H"),
        Fixture::embedded(&program, EMBEDDED_CMAP),
    ] {
        let doc = form_document(&fixture);
        let (saved, _) = filled(&doc, WORD);
        let value = tinker_pdf_cos::fields(&saved)
            .into_iter()
            .find(|field| field.name == "name")
            .map(|field| field.value.as_text())
            .expect("the field is still there");
        assert_eq!(value, WORD, "the value did not survive the fill");
    }
}
