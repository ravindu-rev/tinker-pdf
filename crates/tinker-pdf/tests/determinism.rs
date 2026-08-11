//! Ruling 4: the same document renders to the same bytes, on every target.
//!
//! This is the test that turns "achievable" into "demonstrated". Nothing on a
//! pixel path calls the platform's `libm` any more, and `cargo xtask libm`
//! fails the build if something starts to — but that guards the *mechanism*.
//! The property itself is only proved by rendering on Linux, Windows, macOS
//! and wasm and getting identical bytes, which is what CI runs this file for.
//!
//! # Why hashes rather than committed images
//!
//! A golden PNM for each of these would be a few hundred kilobytes of binary
//! in the repository that no reviewer can read, and a diff nobody can assess.
//! A hash is thirty-two bytes and fails just as loudly. What it cannot do is
//! tell you *how* the output changed — so when one of these fails, render the
//! page and compare it with `pdfcmp` against a build that passes. The failure
//! message says so, because that is the question the person reading it will
//! have.
//!
//! # When one of these fails
//!
//! It means one of two things, and they need opposite responses:
//!
//! - **The same target now renders differently.** A deliberate change to
//!   rendering — then update the hash in the same commit that caused it, and
//!   say in the message what moved.
//! - **Two targets disagree.** A determinism bug. Do not update the hash;
//!   find the arithmetic that is not target-stable. That is the failure this
//!   file exists for, and it is the one that is silent everywhere else.

use tinker_pdf::{Document, DocumentBuilder, RenderOptions};

/// The rendered bytes of the first page, hashed.
fn fingerprint(bytes: Vec<u8>) -> String {
    let bitmap = Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    // The dimensions go into the hash as well: two renders that differ only
    // in size would otherwise have to differ in content to be caught, and a
    // rounding change at the page-size boundary is exactly the kind of thing
    // that does not.
    let mut input = Vec::with_capacity(bitmap.data.len() + 8);
    input.extend_from_slice(&bitmap.width.to_be_bytes());
    input.extend_from_slice(&bitmap.height.to_be_bytes());
    input.extend_from_slice(&bitmap.data);

    tinker_pdf_crypto::sha2::sha256(&input)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Text, which exercises glyph rasterisation — the densest source of
/// coverage arithmetic in the engine.
fn text_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(200.0, 100.0, |page| {
        page.text(
            b"F0",
            14.0,
            10.0,
            60.0,
            "Determinism, and the quick brown fox.",
        );
        page.text(b"F0", 9.0, 10.0, 40.0, "jumps over the lazy dog 0123456789");
    });
    builder.finish()
}

/// Curves and strokes at an angle, where flattening tolerance and the
/// stroker's joins decide individual pixels.
fn curves_page() -> Vec<u8> {
    let content = "0.2 0.4 0.9 RG 3 w 1 J 1 j\n\
                   10 10 m 40 90 80 10 110 60 c S\n\
                   0.9 0.2 0.2 rg\n\
                   20 20 m 60 75 l 100 25 l f\n\
                   0 0 0 RG 0.7 w\n\
                   15 85 m 115 15 l S";
    page_with(content, 130.0, 100.0)
}

/// A shading, which is evaluated per pixel and so multiplies any instability
/// by the area it covers.
fn shading_page() -> Vec<u8> {
    let content = "q 0 0 120 80 re W n /Sh0 sh Q";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /Shading << /Sh0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 120 80]\n\
   /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >>\n\
   /Extend [true true] >>\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// Blend modes, whose integer arithmetic is new and whose whole reason for
/// being integer is this property.
fn blend_page() -> Vec<u8> {
    let content = "0.3 0.6 0.2 rg 0 0 60 60 re f\n\
                   /GS0 gs\n\
                   0.8 0.2 0.5 rg 20 20 60 60 re f";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 80 80]\n\
   /Resources << /ExtGState << /GS0 << /BM /SoftLight /ca 0.7 >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

fn page_with(content: &str, width: f64, height: f64) -> Vec<u8> {
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// The committed fingerprints.
///
/// Every entry is a claim that this page renders to these exact bytes on
/// every supported target. Changing one is a deliberate act; see the module
/// documentation for which of the two failures you are looking at.
/// A named page, and the function that builds it.
type Page = (&'static str, fn() -> Vec<u8>);

const GOLDEN: &[Page] = &[
    ("text", text_page as fn() -> Vec<u8>),
    ("curves", curves_page),
    ("shading", shading_page),
    ("blend", blend_page),
];

#[test]
fn rendering_is_stable_across_targets() {
    // A mismatch prints the replacement lines ready to paste. The length
    // check below is there because an empty table would make this test pass
    // by not looking at anything.
    let expected: &[(&str, &str)] = &[
        (
            "text",
            "0a04158f6a3ed3a7bf9d12ce14188de5ff82fcda4205cd85d7e6ed024729b8bf",
        ),
        (
            "curves",
            "7924b1b282589efa4bbfc39055af40d9f29c9405d0c95381420706b97163968b",
        ),
        (
            "shading",
            "813a28f7b119418e76ae52f96f69047b5dec5100a26375294e9de41ed9cc90b5",
        ),
        (
            "blend",
            "759840c7df7bad4fc49a2d94f763e8b5eca6d9edb64f3af1cdfcd635b2512258",
        ),
    ];
    assert_eq!(
        expected.len(),
        GOLDEN.len(),
        "every page has a fingerprint, or the test passes by not looking"
    );

    let mut wrong = Vec::new();
    for (name, build) in GOLDEN {
        let actual = fingerprint(build());
        let want = expected
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, h)| *h)
            .unwrap_or_default();
        if actual != want {
            wrong.push(format!("        (\"{name}\", \"{actual}\"),"));
        }
    }

    assert!(
        wrong.is_empty(),
        "rendering does not match the committed fingerprints.\n\n\
         If two targets disagree, this is a determinism bug — find the \
         arithmetic that is not target-stable, do not update the table.\n\
         If this is a deliberate rendering change, render the page and \
         compare it with `pdfcmp` first, then paste these in:\n\n{}\n",
        wrong.join("\n")
    );
}

/// The same document rendered twice in one process must agree, which is the
/// weaker property but catches anything reading uninitialised memory or
/// iterating a hash map.
#[test]
fn rendering_is_stable_within_one_process() {
    for (name, build) in GOLDEN {
        assert_eq!(
            fingerprint(build()),
            fingerprint(build()),
            "{name} renders the same twice"
        );
    }
}
