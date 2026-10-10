//! Appearance synthesis for an annotation without `/AP` (12.5.5, 12.5.6),
//! over arbitrary bytes read as one COS object.
//!
//! `DocumentEditor::add_annotation` hands the dictionary a caller supplies —
//! often one lifted out of another file — to `synthesize_appearance`, which
//! reads its geometry arrays, its colours, border and dash, its `/DA` string
//! (lexed as the content stream it is) and its `/Contents`, and writes a
//! content stream from them. Every byte of that dictionary is the document's.
//!
//! The input is parsed with `parse_object_at` against a fixed one-page file
//! whose interactive form's `/DR` holds a WinAnsi Helvetica as `/Helv`, a
//! StandardEncoding one as `/Std`, a `/Differences` font as `/Dif` and a
//! composite font as `/Cmp`, so a seed's `/DA` reaches every branch of the
//! free text path. An input that is not a dictionary is skipped.
//!
//! # What this target checks
//!
//! **No panic and no hang** (ruling 1), and two properties of what is
//! written:
//!
//! - **It is bounded by the input.** A squiggly underline is a constant
//!   number of operators per quad however long the quad, and a free text
//!   annotation's layout stops at its box and at `MAX_DECODED_STREAM`; a
//!   path drawn a vertex per tooth, or a layout that ran on below the box,
//!   is output that grows with a number in the input rather than with the
//!   input, and fails here.
//! - **It lexes cleanly.** This crate's own lexer reads the stream to its
//!   end without a single leniency — no number 7.3.3 disallows or that an
//!   integer token would clamp, no unescaped name, no unterminated string,
//!   no stray delimiter — so what is written is what a reader reads.
//!
//! # What this target cannot find
//!
//! Whether an appearance is *right*: an arrowhead on the wrong end, a box
//! drawn a unit too large or text in the wrong glyphs passes here. That is
//! `appearance.rs`'s unit tests, which pin each subtype's operators, and
//! `crates/tinker-pdf/tests/appearance_synthesis.rs`, which renders them and
//! reads the page at points 12.5.6 puts inside and outside each shape.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_cos::limits::MAX_DECODED_STREAM;
use tinker_pdf_cos::{
    parse_object_at, synthesize_appearance, CosDocument, Lexer, Object, TokenKind, WarningSink,
};

/// The one-page form every input is synthesised against.
fn form() -> CosDocument {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /DR << /Font << \
         /Helv 4 0 R /Std 5 0 R /Dif 6 0 R /Cmp 7 0 R >> >> >> >>",
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman \
         /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /B /C /quoteright] >> >>",
        "<< /Type /Font /Subtype /Type0 /BaseFont /Cmp /Encoding /Identity-H \
         /DescendantFonts [] >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    // The bytes above are this file's own and open: a failure here is the
    // target's, not the input's.
    CosDocument::open(out).expect("the target's own form opens")
}

thread_local! {
    static FORM: CosDocument = form();
}

/// Whether the lexer reads `data` to its end with no leniency and no stray
/// delimiter.
fn lexes_cleanly(data: &[u8]) -> bool {
    let mut sink = WarningSink::new();
    let mut lexer = Lexer::new(data);
    loop {
        let token = lexer.next_token(&mut sink);
        if token.is_eof() {
            return sink.is_empty();
        }
        let stray = matches!(token.kind, TokenKind::Unknown)
            && usize::try_from(token.start)
                .ok()
                .and_then(|at| data.get(at))
                .is_none_or(|b| tinker_pdf_cos::lexer::is_delimiter(*b));
        if stray {
            return false;
        }
    }
}

fuzz_target!(|data: &[u8]| {
    FORM.with(|doc| {
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(data, 0, doc.names_table(), &mut sink);
        let Object::Dict(annotation) = parsed.object else {
            return;
        };
        let Some(stream) = synthesize_appearance(doc, &annotation) else {
            return;
        };
        // A number is at most 309 digits, and an input byte buys a bounded
        // number of them; a stream past this grew with a value, not a length.
        let bound = MAX_DECODED_STREAM + 4096 + 4096 * data.len();
        assert!(
            stream.data.len() <= bound,
            "{} bytes of appearance from {} of input",
            stream.data.len(),
            data.len()
        );
        assert!(
            lexes_cleanly(&stream.data),
            "the appearance does not lex cleanly: {}",
            String::from_utf8_lossy(&stream.data)
        );
    });
});
