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
//!
//! # The three document hashes moved once, and this is why
//!
//! *August 2026.* Every file this engine writes now carries the trailer `/ID`
//! of 7.5.5 Table 15, which it never did on any path — an outside reader found
//! that, and the strict validator refuses it now. So the synthesised document,
//! the synthesised fixed document and the synthesised book are all seventy-odd
//! bytes longer than they were and hash differently. Nothing about the
//! *rendering* moved: the render fingerprints beside them did not change, which
//! is the pair of facts that says this was the writer rather than the
//! rasterizer. The identifier is a hash of the document rather than of the
//! moment, so these hashes stay stable across targets and runs, which is the
//! property ruling 4 needs and a timestamped identifier could not have given.
//!
//! # Why every fixture asserts that it painted something
//!
//! *Added August 2026.* The `text` fixture named Helvetica and embedded no
//! program. The engine bundles no faces by design, so every glyph resolved to
//! nothing, the page came out uniformly white, and its committed fingerprint
//! was the hash of a blank 200x100 page — bit for bit the same as a document
//! that draws nothing at all. It had been that from the day it was written,
//! and it passed the whole time, on all four targets, because a blank page is
//! extremely stable.
//!
//! That is the failure mode a hash cannot show you: a fixture measuring
//! nothing is indistinguishable from a fixture measuring something, right up
//! until you ask what it covers. So each fixture now carries the least ink it
//! may paint and is checked against it before it is hashed. A page that
//! stops drawing fails here rather than quietly becoming the new baseline.

mod written_support;

use std::sync::Arc;

use tinker_pdf::{
    Bitmap, CacheLimits, Document, DocumentBuilder, OpenOptions, PixelRegion, RenderOptions,
    RenderWarning, SimpleFontProvider, SvgOptions, SvgWarning,
};

mod render_support;
use render_support::{
    axial_page, bilevel_page, blend_grid_page, curvy_font, image_page as analytic_image_page, ink,
    intents_page, jpeg_minified_page, jpx_minified_page, pen_page, radial_page,
};

/// How a fixture's document is opened, and which of its pages is hashed.
///
/// *Added by milestone 13.* Every fixture above the fifteenth is a PDF, a comic
/// archive or a fixed document, and all three have one thing in common that
/// nothing in this file had to say out loud: **their page count is a property
/// of the file.** `Document::open` is the whole of what they need and page 0 is
/// the only page a caller could mean.
///
/// A reflowable book is not like that. Its page box is the *caller's* number,
/// its page count is a function of that number, and it has no face of its own
/// unless the book carries one — so opening it needs two decisions this enum
/// makes explicit rather than defaulting.
enum Open {
    /// `Document::open`, page 0. The first fourteen.
    Closed,
    /// A reflowable book at a page box, with this file's own synthetic face,
    /// hashing the page at `at`.
    Book {
        /// The page box in points, which decides the page count.
        page: (f64, f64),
        /// Which page is hashed.
        at: u32,
    },
}

/// A named page: how to build it, and the least ink it may draw.
struct Fixture {
    name: &'static str,
    build: fn() -> Vec<u8>,
    /// How it is opened. See [`Open`].
    open: Open,
    /// The fewest non-background pixels this page may paint.
    ///
    /// A floor, not a measurement — roughly half of what the page draws
    /// today, so an ordinary rendering change moves the hash and leaves this
    /// alone, while a fixture that has stopped drawing trips it. The point is
    /// that "the fingerprints did not move" cannot again be evidence about a
    /// path the fixture never exercised.
    least_ink: usize,
}

/// The face every reflowable fixture here is set in.
///
/// [`curvy_font`], the six-shape synthetic face this file already builds for
/// the `text` fixture, handed to the engine as a host would hand it a system
/// font. **A book that embeds no face has no closed answer without one**: the
/// engine bundles no faces by design, so every glyph of every committed book
/// resolves to nothing and its pages come out blank — which is the exact
/// failure this file's own documentation describes, a fixture measuring
/// nothing being indistinguishable from a fixture measuring something.
///
/// A provider is *usually* an arrangement made outside the document, and
/// `text_page`'s doc comment refuses one for that reason. This one is not: the
/// face is written byte by byte forty lines from here, so it is as much a part
/// of this file as the page is, and ruling 4 still holds over every target
/// including `wasm32`, where there are no font directories to read.
fn book_face() -> Arc<SimpleFontProvider> {
    Arc::new(SimpleFontProvider::new(curvy_font()))
}

/// A fixture's document, opened as [`Open`] says, and the page it hashes.
fn opened(fixture: &Fixture) -> (Document, u32) {
    match fixture.open {
        Open::Closed => (
            converted(
                "opened",
                Document::open((fixture.build)()).expect("it opens"),
            ),
            0,
        ),
        Open::Book { page, at } => (
            converted(
                "opened",
                Document::open_with(
                    (fixture.build)(),
                    &OpenOptions::at_page(page.0, page.1).with_fonts(book_face()),
                )
                .expect("it opens"),
            ),
            at,
        ),
    }
}

/// The rendered bytes of one page, hashed.
fn fingerprint(fixture: &Fixture) -> String {
    let (document, at) = opened(fixture);
    let bitmap = document
        .page(at)
        .expect("a page")
        .render(&RenderOptions::default());
    hashed(fixture, &bitmap)
}

/// A fixture's page as one bitmap, checked for ink and faces and hashed with
/// its dimensions.
fn hashed(fixture: &Fixture, bitmap: &Bitmap) -> String {
    let drawn = ink(bitmap);
    assert!(
        drawn >= fixture.least_ink,
        "the {} fixture painted {drawn} pixels, fewer than the {} it is \
         supposed to: it is measuring less than it claims, and its \
         fingerprint is not evidence about anything until that is fixed. \
         Warnings: {:?}",
        fixture.name,
        fixture.least_ink,
        bitmap.warnings,
    );
    assert!(
        !bitmap.warnings.contains(&RenderWarning::UnreadableFont),
        "the {} fixture named a font this build cannot draw, so its glyphs \
         are missing from the fingerprint",
        fixture.name,
    );

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
///
/// The face is embedded rather than named. A document naming one of the
/// standard 14 carries no outlines, and the engine bundles none, so every
/// glyph in the version of this fixture that stood here until August 2026
/// resolved to nothing and the page was blank. A host `FontProvider` would
/// also have put ink on it, but the provider is the *host's* configuration —
/// a fixture that depends on one is measuring an arrangement made outside the
/// document, and ruling 4 is a claim about documents. Embedding keeps this
/// page closed: everything it renders from is in the file.
fn text_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    // The whole face, so that the only thing between these bytes and the
    // pixels is the renderer. Subsetting is correct and tested elsewhere; if
    // it ran here, a change to the subsetter would move a fingerprint whose
    // failure message talks about rendering.
    builder.set_subset_fonts(false);
    assert!(
        builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()),
        "the synthetic face parses as a TrueType program"
    );
    // Both lines are sized to end inside the page: ink that falls off the
    // canvas is clipped away and contributes nothing to the fingerprint, so
    // an overlong line is coverage that looks like it is being measured and
    // is not. The second starts on a half-pixel so the same shapes land on a
    // different sub-pixel phase.
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 14.0, 10.0, 60.0, "Determinism, and the");
        page.text(b"F0", 9.0, 10.5, 40.0, "quick brown fox jumps 0123456789");
    });
    built("text page", builder)
}

/// The `text` page with each line's font selected by a graphics state
/// rather than by `Tf` (8.4.5's `/Font [font size]`), and the page's `/Font`
/// taken away, so every reader reaches the face by the reserved name the state
/// gives it.
///
/// Made from [`text_page`]'s own bytes, each `/F0 n Tf` moved into a state
/// `/Sn` naming the face's object at size `n`: the same face, lines and
/// positions, so its fingerprint is the `text` page's — a font a state selects
/// draws as its `Tf` twin on every target.
fn gs_text_page() -> Vec<u8> {
    use tinker_pdf_cos::{
        CosDocument, Dict, DocumentEditor, Object, Resolve, StreamData, WriteMode, WriteOptions,
    };
    let doc = Arc::new(CosDocument::open(text_page()).expect("the text page opens"));
    let key = |name: &[u8]| doc.intern(name);
    let catalog = Resolve::catalog(&*doc).expect("a catalog");
    let pages = doc.resolve_key(&catalog, key(b"Pages"));
    let kids = doc.resolve_key(pages.as_dict().expect("a page tree"), key(b"Kids"));
    let page_ref = kids
        .as_array()
        .and_then(<[Object]>::first)
        .and_then(Object::as_objref)
        .expect("one page");
    let page = doc.get(page_ref).expect("the page reads");
    let mut page = page.as_dict().expect("a page dictionary").clone();
    let resources = doc.resolve_key(&page, key(b"Resources"));
    let resources = resources.as_dict().expect("resources");
    let face = doc
        .resolve_key(resources, key(b"Font"))
        .as_dict()
        .and_then(|fonts| fonts.get_ref(key(b"F0")))
        .expect("the face is an object");
    let contents = page.get_ref(key(b"Contents")).expect("one content stream");
    let content = String::from_utf8(doc.stream_decoded(contents).expect("the content decodes"))
        .expect("the content is text");
    let mut moved = content.clone();
    for size in [14, 9] {
        let tf = format!("BT /F0 {size} Tf ");
        assert_eq!(
            moved.matches(&tf).count(),
            1,
            "the page sets size {size} once"
        );
        moved = moved.replace(&tf, &format!("/S{size} gs BT "));
    }

    let mut states = Dict::new();
    for size in [14, 9] {
        let mut state = Dict::new();
        state.insert(
            key(b"Font"),
            Object::Array(vec![Object::Ref(face), Object::Int(size)]),
        );
        states.insert(key(format!("S{size}").as_bytes()), Object::Dict(state));
    }
    let mut scope = Dict::new();
    for (name, value) in resources.iter() {
        if *name != key(b"Font") {
            scope.insert(*name, value.clone());
        }
    }
    scope.insert(key(b"ExtGState"), Object::Dict(states));
    page.insert(key(b"Resources"), Object::Dict(scope));

    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    editor.put(page_ref, Object::Dict(page));
    editor.put_stream(
        contents,
        StreamData {
            dict: Dict::new(),
            data: moved.into_bytes(),
        },
    );
    let out = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    written_support::rewritten("gs text page", doc.bytes(), &out);
    out
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

/// A shading pattern used as a fill *and* as a stroke.
///
/// *Added August 2026, with gap 07.* `fill_with_pattern` had no fingerprint
/// at all before this. The `shading` fixture above covers `sh`, which is a
/// different loop over the canvas — the pattern path has its own inverse
/// transform, its own coverage mask and, since gap 07, a stroked outline
/// feeding it. Three things here are in no other fixture:
///
/// - the pattern's `/Matrix`, whose inverse is what maps a device pixel back
///   into pattern space, at a rotation so that all four of its coefficients
///   matter;
/// - a stroked outline used as the shape a pattern fills, with a curve, so
///   the stroker's flattening lands inside the pattern's coverage arithmetic;
/// - a paint-time CTM that is *not* the identity, which is the anchoring
///   guarantee of 8.7.3.1 expressed as bytes: the fingerprint moves if the
///   pattern ever starts following it.
fn pattern_page() -> Vec<u8> {
    let content = "/Pattern cs /P0 scn\n\
                   10 10 m 40 70 70 10 110 60 c 110 10 l h f\n\
                   /Pattern CS /P0 SCN 6 w 1 J 1 j\n\
                   q 1.5 0 0 1.5 5 5 cm\n\
                   5 5 m 20 40 40 5 60 28 c S\n\
                   Q";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /Pattern << /P0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /PatternType 2 /Matrix [0.8 0.6 -0.6 0.8 15 -10]\n\
   /Shading << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 100 0]\n\
     /Function << /FunctionType 2 /Domain [0 1]\n\
                  /C0 [0.9 0.1 0] /C1 [0 0.2 0.9] /N 1 >>\n\
     /Extend [true true] >> >>\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// Optional content: what a page draws once its `/OCProperties` have been
/// read.
///
/// *Added August 2026, with gap 06.* Every other fixture here would render
/// identically on a build that had never heard of 8.11, so none of them is
/// evidence about it. This one is four claims at once, and each is a
/// different branch of the visibility decision:
///
/// - the opaque red rectangle covers the whole page and is in a group `/D`
///   turns **off**. If suppression ever regresses, this fixture does not
///   merely change — it becomes a flat red page, which no minimum-ink floor
///   would catch, because the ink goes *up*;
/// - the curved wedge is in a group that is **on**, so a build that hid
///   everything marked rather than everything hidden loses it;
/// - the stroked rule is behind an OCMD with `/P /AnyOff` over one group that
///   is off and one that is on, which is the policy a naive reading inverts —
///   it is visible *because* a group is off;
/// - the form XObject carries `/OC` on the XObject dictionary itself (8.11.4.4)
///   rather than in a `BDC`, and would otherwise paint the page black.
fn optional_content_page() -> Vec<u8> {
    // Most of the ink is inside layers that are *on*, so the minimum-ink
    // floor below is an instrument with something to measure: a build that
    // hid everything marked rather than everything hidden loses three
    // quarters of the page.
    let content = "0.2 0.5 0.8 rg 5 5 m 30 45 l 55 5 l h f\n\
                   /OC /Off BDC 0.9 0.1 0.1 rg 0 0 120 80 re f EMC\n\
                   /OC /On BDC 0.1 0.7 0.2 rg 8 50 m 40 78 52 46 20 52 c h f\n\
                     0.5 0.3 0.7 rg 62 8 52 62 re f EMC\n\
                   /OC /Mixed BDC 0 0 0 RG 3 w 1 J 5 74 m 115 70 l S EMC\n\
                   q 1 0 0 1 0 0 cm /Frm Do Q";
    let form = "0 0 0 rg 0 0 120 80 re f";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R\n\
   /OCProperties << /OCGs [5 0 R 6 0 R] /D << /OFF [5 0 R] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /Properties << /Off 5 0 R /On 6 0 R /Mixed 7 0 R >>\n\
                 /XObject << /Frm 8 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /OCG /Name (Construction lines) >>\nendobj\n\
6 0 obj\n<< /Type /OCG /Name (Base) >>\nendobj\n\
7 0 obj\n<< /Type /OCMD /OCGs [5 0 R 6 0 R] /P /AnyOff >>\nendobj\n\
8 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 120 80] /OC 5 0 R\n\
   /Length {} >>\nstream\n{form}\nendstream\nendobj\n\
trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        form.len()
    )
    .into_bytes()
}

/// The samples of the image fixture: 32 x 32, three bytes each.
///
/// A ramp across in red and down in green, and a quadratic scramble in blue.
/// The ramps are what a filter has to interpolate smoothly; the blue is what
/// it has to *average*, and it is neither periodic nor linear on purpose.
///
/// Both of the obvious choices are traps, and the first one was written here
/// before it was measured. A **linear** ramp is reproduced exactly by box
/// averaging and by bilinear taps alike, at any pyramid depth, so a page of
/// gradients cannot tell one depth from another. A **checkerboard** is worse:
/// every balanced two-tap average of one is its mean, so a pyramid one level
/// short lands on the same bytes as a correct one. Both were confirmed by
/// injecting exactly that defect and watching the fingerprint not move.
///
/// A quadratic has a different mean over every window, which is the property
/// that makes the depth visible. No sample is white, so every pixel the image
/// covers counts as ink.
fn image_samples() -> Vec<u8> {
    let mut rgb = Vec::with_capacity(32 * 32 * 3);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let blue = (x * x * 7 + y * y * 13 + x * y * 29) % 256;
            rgb.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, blue as u8]);
        }
    }
    rgb
}

/// Those samples as `ASCIIHexDecode` text, so the whole fixture is a `str`.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2 + 1);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out.push('>');
    out
}

/// Image sampling: every row of the policy matrix, on one page.
///
/// *Added August 2026, with gap 12.* No fixture here drew an image at all
/// before this, so none of them would have moved if sampling had changed —
/// and sampling had never been anything but one truncating tap per
/// destination pixel. That is the failure mode this file's own documentation
/// describes: a fingerprint is not evidence about a path no fixture reaches.
///
/// Six placements of the same 32 x 32 samples, one per branch the sampler can
/// take:
///
/// - **magnified without `/Interpolate`** — nearest, the row that looks wrong
///   and is not: 32 samples into 40 pixels, hard sample edges preserved;
/// - **magnified with it** — bilinear, the same placement of `/Im1`, which
///   differs from `/Im0` in that one key;
/// - **shrunk within 2:1** — bilinear whatever the flag says, 32 samples into
///   20 pixels;
/// - **shrunk 4:1** — one halving per axis, then the taps;
/// - **shrunk 8:1** — two, because one pyramid depth cannot demonstrate that
///   the depth is chosen rather than assumed;
/// - **rotated and magnified** — a placement whose inverse transform has all
///   four coefficients, so the `u`/`v` mapping cannot be right by accident of
///   an axis-aligned test.
fn image_page() -> Vec<u8> {
    let content = "q 40 0 0 40 5 75 cm /Im0 Do Q\n\
                   q 40 0 0 40 50 75 cm /Im1 Do Q\n\
                   q 20 0 0 20 95 90 cm /Im0 Do Q\n\
                   q 8 0 0 8 120 92 cm /Im0 Do Q\n\
                   q 4 0 0 4 140 100 cm /Im0 Do Q\n\
                   q 28.284271 28.284271 -28.284271 28.284271 40 15 cm /Im1 Do Q";
    let samples = hex(&image_samples());
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 120]\n\
   /Resources << /XObject << /Im0 5 0 R /Im1 6 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 32 /Height 32\n\
   /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode\n\
   /Length {} >>\nstream\n{samples}\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Image /Width 32 /Height 32\n\
   /ColorSpace /DeviceRGB /BitsPerComponent 8 /Interpolate true\n\
   /Filter /ASCIIHexDecode /Length {} >>\nstream\n{samples}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        samples.len(),
        samples.len()
    )
    .into_bytes()
}

/// A JBIG2 scan: the arithmetic coder, a generic region and the polarity
/// inversion at the PDF boundary.
///
/// *Added August 2026, with gap 17.* Not one fixture above decodes a JBIG2
/// stream — the codec was a capability gate until this gap — so this is the
/// first that would move if the MQ coder, T.88's templates or the 1-is-black
/// inversion changed. That matters more here than for most paths, because the
/// MQ decoder is *shared*: gap 18 would use the same coder for JPEG 2000, and
/// a change made there that shifted a row of the Qe table would otherwise
/// have to be caught by the filter crate alone.
///
/// The bytes are ITU-T T.88 Annex H.1's second page, embedded as Annex D.3's
/// organisation is — page information segment, an arithmetically coded generic
/// region using template 0 with typical prediction, and four segments this
/// build refuses. It draws a frame two pixels thick, and it is drawn three
/// ways so the fixture covers more than one row of the sampling policy: at
/// 1:1, at a non-integer magnification, and as an `/ImageMask` in colour,
/// which is the shape a scanned page is most often given.
fn jbig2_page() -> Vec<u8> {
    let content = "q 64 0 0 56 0 64 cm /Im0 Do Q\n\
                   0.85 0.1 0.2 rg\n\
                   q 64 0 0 56 64 64 cm /Im1 Do Q\n\
                   q 101 0 0 57 13 3 cm /Im0 Do Q";
    let coded = hex(&ANNEX_H_PAGE_2);
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 128 120]\n\
   /Resources << /XObject << /Im0 5 0 R /Im1 6 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 64 /Height 56\n\
   /ColorSpace /DeviceGray /BitsPerComponent 1\n\
   /Filter [/ASCIIHexDecode /JBIG2Decode] /Length {} >>\nstream\n{coded}\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Image /Width 64 /Height 56 /ImageMask true\n\
   /Filter [/ASCIIHexDecode /JBIG2Decode] /Length {} >>\nstream\n{coded}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        coded.len(),
        coded.len()
    )
    .into_bytes()
}

/// ITU-T T.88 Annex H.1's second page, bytes 400 to 681 of the annex's
/// datastream.
#[rustfmt::skip]
const ANNEX_H_PAGE_2: [u8; 282] = [
    0x00, 0x00, 0x00, 0x08, 0x30, 0x00, 0x02, 0x00, 0x00, 0x00, 0x13, 0x00,
    0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x01,
    0x02, 0x00, 0x00, 0x00, 0x1B, 0x08, 0x00, 0x02, 0xFF, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x02, 0x4F, 0xE7, 0x8C, 0x20, 0x0E, 0x1D, 0xC7,
    0xCF, 0x01, 0x11, 0xC4, 0xB2, 0x6F, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0A,
    0x07, 0x40, 0x00, 0x09, 0x02, 0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00,
    0x25, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x0C, 0x08, 0x00, 0x00, 0x00, 0x05, 0x8D, 0x6E, 0x5A, 0x12,
    0x40, 0x85, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0B, 0x27, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x23, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x2C, 0x00,
    0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x0B, 0x00, 0x08, 0x03, 0xFF, 0xFD,
    0xFF, 0x02, 0xFE, 0xFE, 0xFE, 0x04, 0xEE, 0xED, 0x87, 0xFB, 0xCB, 0x2B,
    0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0C, 0x10, 0x01, 0x02, 0x00, 0x00, 0x00,
    0x1C, 0x06, 0x04, 0x04, 0x00, 0x00, 0x00, 0x0F, 0x90, 0x71, 0x6B, 0x6D,
    0x99, 0xA7, 0xAA, 0x49, 0x7D, 0xF2, 0xE5, 0x48, 0x1F, 0xDC, 0x68, 0xBC,
    0x6E, 0x40, 0xBB, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0D, 0x17, 0x20, 0x0C,
    0x02, 0x00, 0x00, 0x00, 0x3E, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
    0x24, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x87, 0xCB, 0x82, 0x1E, 0x66,
    0xA4, 0x14, 0xEB, 0x3C, 0x4A, 0x15, 0xFA, 0xCC, 0xD6, 0xF3, 0xB1, 0x6F,
    0x4C, 0xED, 0xBF, 0xA7, 0xBF, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0E, 0x31,
    0x00, 0x02, 0x00, 0x00, 0x00, 0x00,
];

/// JPEG 2000: the fixed-point 9/7 wavelet, the fixed-point ICT, and the
/// integer path beside them.
///
/// *Added August 2026, with gap 18a milestone 8.* Nothing above decodes a JPX
/// stream — the codec was a capability gate until milestone 7 put one on a
/// page — so this is the first fingerprint that would move if any of four
/// thousand lines of JPEG 2000 changed its answer.
///
/// **It is the fixture ruling 4 most needs, because this decoder is the only
/// one in the engine whose arithmetic was designed around it.** T.800 Table
/// F.4 specifies the 9/7's six lifting constants as decimals and G.2.2
/// specifies the ICT's four the same way, and both are computed here in
/// integers — `i32` planes at Q12, `i64` constants at Q24, every product
/// formed in `i64` and rounded back with `(p + (1 << 23)) >> 24` — precisely
/// so that a 32-bit target and a 64-bit one cannot disagree. The whole of
/// that decision is a claim about this test.
///
/// Three codestreams, because they reach different arithmetic:
///
/// - **`/Im0`, reversible 5/3 with the RCT.** Exact integer arithmetic, so it
///   cannot drift by rounding — what it covers is tier-2's packet
///   arithmetic, tier-1's context formation over the MQ coder, the tag trees,
///   dequantisation and the DC level shift, all of which the other two share.
/// - **`/Im1`, irreversible 9/7 with the ICT.** The fixed point, in both of
///   the places it lives. A constant rounded differently, a product formed at
///   a different width, or a shift that truncates where it should round moves
///   pixels here and nowhere else in this file.
/// - **`/Im2`, the same image truncated to a twentieth.** E.1.1.2's
///   reconstruction point is per *coefficient*, and on a complete code-block
///   every coefficient is known to the same depth, so this is the only one of
///   the three where `half_planes` does anything at all. Milestone 6 found
///   that defect with `opj_decompress` alone — no unit test and no `f64`
///   reference could see it — and this is a second thing that would.
///
/// The bytes are `opj_compress`'s output on this repository's own 32 x 24
/// gradient, which is a tool's output on our input and ours to commit. No
/// ISO/IEC 15444-4 conformance material is here and none ever will be.
fn jpx_page() -> Vec<u8> {
    let content = "q 48 0 0 36 4 12 cm /Im0 Do Q
                   q 48 0 0 36 56 12 cm /Im1 Do Q
                   q 48 0 0 36 108 12 cm /Im2 Do Q";
    let rct = hex(&JPX_RCT_5_3);
    let ict = hex(&JPX_ICT_9_7);
    let truncated = hex(&JPX_TRUNCATED);
    format!(
        "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 60]
   /Resources << /XObject << /Im0 5 0 R /Im1 6 0 R /Im2 7 0 R >> >>
   /Contents 4 0 R >>
endobj
4 0 obj
<< /Length {} >>
stream
{content}
endstream
endobj
5 0 obj
<< /Type /XObject /Subtype /Image /Width 32 /Height 24
   /Filter [/ASCIIHexDecode /JPXDecode] /Length {} >>
stream
{rct}
endstream
endobj
6 0 obj
<< /Type /XObject /Subtype /Image /Width 32 /Height 24
   /Filter [/ASCIIHexDecode /JPXDecode] /Length {} >>
stream
{ict}
endstream
endobj
7 0 obj
<< /Type /XObject /Subtype /Image /Width 32 /Height 24
   /Filter [/ASCIIHexDecode /JPXDecode] /Length {} >>
stream
{truncated}
endstream
endobj
trailer
<< /Size 8 /Root 1 0 R >>
%%EOF
",
        content.len(),
        rct.len(),
        ict.len(),
        truncated.len()
    )
    .into_bytes()
}

/// `opj_compress -i tests/jpx/c1.ppm -o c1-2.jp2 -r 1 -n 2`: a 32 x 24 RGB
/// gradient, coded losslessly with the reversible 5/3 and the RCT.
#[rustfmt::skip]
const JPX_RCT_5_3: [u8; 673] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
    0x00, 0x00, 0x00, 0x14, 0x66, 0x74, 0x79, 0x70, 0x6A, 0x70, 0x32, 0x20,
    0x00, 0x00, 0x00, 0x00, 0x6A, 0x70, 0x32, 0x20, 0x00, 0x00, 0x00, 0x2D,
    0x6A, 0x70, 0x32, 0x68, 0x00, 0x00, 0x00, 0x16, 0x69, 0x68, 0x64, 0x72,
    0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x20, 0x00, 0x03, 0x07, 0x07,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x63, 0x6F, 0x6C, 0x72, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x02, 0x54, 0x6A, 0x70, 0x32,
    0x63, 0xFF, 0x4F, 0xFF, 0x51, 0x00, 0x2F, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x20, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x07, 0x01, 0x01, 0x07, 0x01,
    0x01, 0x07, 0x01, 0x01, 0xFF, 0x52, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x01,
    0x01, 0x01, 0x04, 0x04, 0x00, 0x01, 0xFF, 0x5C, 0x00, 0x07, 0x40, 0x40,
    0x48, 0x48, 0x50, 0xFF, 0x64, 0x00, 0x25, 0x00, 0x01, 0x43, 0x72, 0x65,
    0x61, 0x74, 0x65, 0x64, 0x20, 0x62, 0x79, 0x20, 0x4F, 0x70, 0x65, 0x6E,
    0x4A, 0x50, 0x45, 0x47, 0x20, 0x76, 0x65, 0x72, 0x73, 0x69, 0x6F, 0x6E,
    0x20, 0x32, 0x2E, 0x35, 0x2E, 0x30, 0xFF, 0x90, 0x00, 0x0A, 0x00, 0x00,
    0x00, 0x00, 0x01, 0xD9, 0x00, 0x01, 0xFF, 0x93, 0xDF, 0x85, 0x0E, 0x12,
    0x36, 0x57, 0xB0, 0x66, 0xFA, 0xA8, 0x6C, 0xA6, 0x70, 0xE2, 0xBF, 0xAA,
    0x89, 0xEF, 0x74, 0xEA, 0x4A, 0x97, 0x7A, 0x73, 0xDF, 0x62, 0xAC, 0x35,
    0xAA, 0xB0, 0x35, 0x4D, 0xE0, 0xE8, 0x0A, 0x2E, 0xFE, 0x7C, 0x73, 0x47,
    0x2C, 0x51, 0x5C, 0xAC, 0xDB, 0x2A, 0xD5, 0x14, 0xAB, 0x70, 0xC1, 0x80,
    0xEF, 0x6E, 0xB7, 0xA6, 0x16, 0x20, 0x69, 0x69, 0xC8, 0x9B, 0xA2, 0x97,
    0xBB, 0x3C, 0x7F, 0x4F, 0x0F, 0x68, 0x70, 0x30, 0x87, 0x70, 0x32, 0xF9,
    0x8E, 0x7E, 0x81, 0xCC, 0x58, 0x49, 0x55, 0x55, 0x55, 0x55, 0x55, 0x71,
    0x22, 0xA2, 0xDA, 0x29, 0x01, 0x4B, 0x94, 0xBB, 0x91, 0x03, 0xAD, 0x06,
    0xD4, 0x12, 0x7D, 0x26, 0xC6, 0x2F, 0x79, 0xBE, 0x6F, 0x9B, 0xE6, 0xF9,
    0xC0, 0x05, 0x40, 0x85, 0x6D, 0xDE, 0x2E, 0x65, 0xE4, 0x6A, 0xA9, 0x0C,
    0xB7, 0x88, 0xF2, 0xF2, 0xF3, 0x80, 0xE8, 0x50, 0xCB, 0xF0, 0xDE, 0x17,
    0xE1, 0x7F, 0xDF, 0x83, 0xE8, 0x8C, 0x91, 0x46, 0x66, 0x53, 0xE3, 0x3A,
    0xFA, 0x04, 0x13, 0xA6, 0xE5, 0x6A, 0x11, 0x4C, 0xB3, 0x6E, 0xDC, 0x4F,
    0xF7, 0x6F, 0x38, 0xDF, 0xB3, 0x5B, 0x0B, 0x8B, 0x02, 0xDD, 0xCB, 0xC8,
    0xED, 0x19, 0x82, 0xE1, 0xB7, 0xC8, 0x1A, 0xEB, 0xF0, 0x96, 0xF6, 0x79,
    0xC9, 0x2D, 0x90, 0xA0, 0x75, 0xBC, 0xE4, 0xA9, 0xB2, 0x53, 0x39, 0xF8,
    0xFC, 0x35, 0xF2, 0x46, 0x02, 0x87, 0xDE, 0xC5, 0x6D, 0xD7, 0xC2, 0x12,
    0x1F, 0x1E, 0x5F, 0xFC, 0x5B, 0x63, 0xC9, 0xF6, 0x75, 0x66, 0x1E, 0x3A,
    0xC0, 0xCB, 0xA9, 0x2C, 0x7F, 0xD2, 0x4D, 0x49, 0x90, 0x00, 0x00, 0x00,
    0x00, 0x2C, 0x83, 0xCC, 0x09, 0xA9, 0x32, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x90, 0x75, 0xF0, 0x32, 0xF7, 0xF9, 0x06, 0x62, 0x00, 0x12, 0xC0, 0x92,
    0x7B, 0xB7, 0xF3, 0x81, 0x9A, 0xCC, 0xCC, 0xC9, 0xF4, 0xBF, 0xDF, 0x85,
    0x14, 0x3C, 0x01, 0x79, 0x7B, 0x4C, 0xE1, 0x9F, 0xB0, 0x52, 0xDD, 0x13,
    0x83, 0xFB, 0x0D, 0xA8, 0xC0, 0x96, 0xEF, 0xC4, 0xD2, 0xCC, 0x6B, 0x1C,
    0x20, 0xA8, 0x65, 0xA3, 0x05, 0xF9, 0xB4, 0xD6, 0x5D, 0xAC, 0xA6, 0x68,
    0xBF, 0x8A, 0x09, 0xB7, 0x2F, 0x68, 0xFB, 0x5C, 0xA3, 0x52, 0x95, 0xFB,
    0xC3, 0x41, 0xF8, 0xAB, 0xE2, 0x4E, 0xA6, 0x5F, 0xAC, 0xF9, 0xEA, 0x1F,
    0x0D, 0xF8, 0x1B, 0x1A, 0x81, 0x8A, 0xBA, 0x95, 0xEA, 0x78, 0x12, 0x1B,
    0x7D, 0x6D, 0x7C, 0x52, 0x2C, 0x8D, 0xE6, 0x66, 0x66, 0x71, 0x33, 0x42,
    0x42, 0xF7, 0x54, 0xE3, 0x2C, 0x33, 0xBB, 0xF9, 0x99, 0x92, 0x60, 0x6B,
    0xDB, 0xD0, 0x90, 0xB1, 0x1E, 0xC3, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x60, 0x18, 0xFB, 0x0B, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x29, 0xA9,
    0x73, 0x52, 0x79, 0x33, 0x83, 0xC7, 0x83, 0xF2, 0x4E, 0x59, 0x35, 0x99,
    0x99, 0x94, 0xA3, 0x82, 0x7F, 0x01, 0x3D, 0xC0, 0x3A, 0x24, 0x07, 0xC2,
    0x28, 0x5F, 0xA7, 0xC7, 0xBF, 0x8C, 0xA4, 0x29, 0x5D, 0x9F, 0xC0, 0x7C,
    0x22, 0xC0, 0x7C, 0x22, 0x80, 0x5F, 0xA7, 0xC5, 0x46, 0x7F, 0x8C, 0x8E,
    0xA9, 0x1D, 0x9F, 0xC0, 0xF9, 0x02, 0xC0, 0xF9, 0x04, 0x00, 0x5F, 0xA7,
    0xC5, 0x46, 0xBF, 0x8C, 0x8E, 0xA8, 0xFF, 0x78, 0x8C, 0xCC, 0x1F, 0xFF,
    0xD9,
];

/// The same image with `-I`, so the irreversible 9/7 and the ICT — which is
/// the pair the fixed-point decision was made for.
#[rustfmt::skip]
const JPX_ICT_9_7: [u8; 725] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
    0x00, 0x00, 0x00, 0x14, 0x66, 0x74, 0x79, 0x70, 0x6A, 0x70, 0x32, 0x20,
    0x00, 0x00, 0x00, 0x00, 0x6A, 0x70, 0x32, 0x20, 0x00, 0x00, 0x00, 0x2D,
    0x6A, 0x70, 0x32, 0x68, 0x00, 0x00, 0x00, 0x16, 0x69, 0x68, 0x64, 0x72,
    0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x20, 0x00, 0x03, 0x07, 0x07,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x63, 0x6F, 0x6C, 0x72, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x02, 0x88, 0x6A, 0x70, 0x32,
    0x63, 0xFF, 0x4F, 0xFF, 0x51, 0x00, 0x2F, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x20, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x07, 0x01, 0x01, 0x07, 0x01,
    0x01, 0x07, 0x01, 0x01, 0xFF, 0x52, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x01,
    0x01, 0x01, 0x04, 0x04, 0x00, 0x00, 0xFF, 0x5C, 0x00, 0x0B, 0x42, 0x48,
    0x24, 0x57, 0xD3, 0x57, 0xD3, 0x57, 0x62, 0xFF, 0x64, 0x00, 0x25, 0x00,
    0x01, 0x43, 0x72, 0x65, 0x61, 0x74, 0x65, 0x64, 0x20, 0x62, 0x79, 0x20,
    0x4F, 0x70, 0x65, 0x6E, 0x4A, 0x50, 0x45, 0x47, 0x20, 0x76, 0x65, 0x72,
    0x73, 0x69, 0x6F, 0x6E, 0x20, 0x32, 0x2E, 0x35, 0x2E, 0x30, 0xFF, 0x90,
    0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x02, 0x09, 0x00, 0x01, 0xFF, 0x93,
    0xCF, 0xC2, 0xB1, 0x11, 0x50, 0x4F, 0x95, 0xA6, 0x9E, 0xA1, 0x94, 0x82,
    0x06, 0xD6, 0xDA, 0x20, 0x90, 0x05, 0xD5, 0x8E, 0x6C, 0xFA, 0x63, 0x4B,
    0x07, 0x13, 0xCB, 0x1C, 0x3B, 0x60, 0x6A, 0x8C, 0x1C, 0xA7, 0x0B, 0xFE,
    0x1C, 0x16, 0x00, 0x26, 0xFE, 0x29, 0x40, 0xCE, 0xE4, 0x34, 0x6C, 0x91,
    0xDF, 0xBB, 0xA3, 0x79, 0x19, 0xE2, 0x75, 0xCF, 0xEB, 0x31, 0xA9, 0xA6,
    0x50, 0xDC, 0xDE, 0xCF, 0xD2, 0x07, 0x7B, 0x56, 0x76, 0x7E, 0x94, 0x90,
    0x1A, 0xC6, 0xB6, 0x1D, 0xB9, 0xC9, 0x3D, 0x05, 0x82, 0x44, 0xDE, 0x7E,
    0xD1, 0xE1, 0xC9, 0x3C, 0x75, 0x62, 0xD4, 0x20, 0xD5, 0x87, 0xA1, 0xA9,
    0xE4, 0x15, 0x88, 0x45, 0x15, 0x29, 0xB2, 0x3F, 0x8F, 0x12, 0xBF, 0xF7,
    0x1A, 0x05, 0x7C, 0xF3, 0xD6, 0x7C, 0x1A, 0xDC, 0x05, 0xAD, 0x56, 0x4E,
    0x89, 0xDB, 0x06, 0x08, 0x78, 0x74, 0x61, 0x29, 0x12, 0x93, 0x24, 0x0C,
    0x56, 0xED, 0x83, 0x58, 0x34, 0x0E, 0x62, 0xC4, 0x2F, 0x8B, 0x2E, 0x64,
    0x62, 0x6F, 0x79, 0xD7, 0xF3, 0xCE, 0xA5, 0x5D, 0x25, 0x94, 0x20, 0x50,
    0x02, 0x8B, 0xFF, 0x7D, 0x60, 0x1F, 0xF4, 0xAE, 0x00, 0x77, 0x1E, 0xDB,
    0xEB, 0x03, 0xAD, 0x6E, 0x7A, 0x73, 0x09, 0xB2, 0x48, 0x56, 0x0B, 0x6B,
    0xC3, 0xEA, 0x71, 0x55, 0xA6, 0x47, 0x29, 0xF0, 0x78, 0x45, 0x8B, 0xEF,
    0xB5, 0x95, 0xC4, 0x8E, 0x86, 0xDA, 0xD2, 0xE7, 0xFC, 0x60, 0x63, 0x03,
    0x64, 0xBE, 0x15, 0x88, 0x34, 0x1C, 0x19, 0x6D, 0x96, 0x6F, 0xCC, 0xF9,
    0x47, 0x8C, 0x22, 0x89, 0xD2, 0x4F, 0x34, 0xDA, 0x77, 0x0A, 0xA9, 0x39,
    0xD5, 0x61, 0x39, 0xE8, 0x62, 0xA2, 0x35, 0x90, 0x3F, 0x97, 0xD7, 0x55,
    0xF1, 0x03, 0x5B, 0xF3, 0x60, 0x00, 0x60, 0x7A, 0xA5, 0x15, 0xBA, 0x43,
    0xD3, 0xE5, 0x68, 0xEA, 0x7C, 0x44, 0x90, 0x70, 0x1D, 0x33, 0x46, 0x23,
    0x6B, 0x50, 0x85, 0xF8, 0x60, 0x01, 0x9A, 0x2D, 0x18, 0x09, 0xA8, 0x37,
    0xA7, 0x7C, 0xF9, 0x56, 0x72, 0x6D, 0xC5, 0x40, 0xC5, 0xA4, 0x60, 0xDC,
    0x45, 0x18, 0x4B, 0x18, 0xE5, 0x53, 0x3B, 0x23, 0xCF, 0xC2, 0xA8, 0x46,
    0x3B, 0x66, 0xF3, 0xF4, 0x58, 0xC3, 0x51, 0x57, 0x5E, 0x31, 0x1D, 0xF1,
    0x71, 0x92, 0x11, 0x3C, 0xBB, 0xA1, 0x27, 0x04, 0xE1, 0x08, 0xFE, 0x37,
    0xED, 0x77, 0xBF, 0x6C, 0x67, 0xE3, 0xEB, 0x20, 0x49, 0x18, 0x17, 0xF5,
    0xD8, 0x40, 0x73, 0xF4, 0x2E, 0xFC, 0xBB, 0xF2, 0x2D, 0xB8, 0x13, 0xC7,
    0xD9, 0xD4, 0xAF, 0x3D, 0xE4, 0xB2, 0x98, 0x58, 0x9D, 0x7D, 0xFD, 0x86,
    0xD8, 0xFD, 0xA4, 0x4D, 0x9B, 0xAB, 0x1D, 0x2B, 0x1E, 0x78, 0x7D, 0x74,
    0x44, 0x12, 0x8B, 0x82, 0xE4, 0xA4, 0x59, 0x5B, 0xAC, 0x54, 0x90, 0x91,
    0xCC, 0x55, 0x60, 0x4D, 0xD7, 0x23, 0x95, 0x04, 0x4A, 0xD6, 0xC3, 0xC7,
    0xA1, 0x88, 0x63, 0x92, 0x84, 0x33, 0x3E, 0x10, 0xD4, 0xCA, 0x54, 0xB5,
    0x22, 0xF2, 0x9E, 0x97, 0xC8, 0xB1, 0x40, 0x6F, 0xF5, 0x67, 0xB3, 0x59,
    0x32, 0xB1, 0x87, 0xAE, 0x44, 0x87, 0x58, 0x43, 0xB7, 0x07, 0x12, 0x01,
    0xFD, 0x37, 0x05, 0x3D, 0x18, 0xF4, 0xD6, 0x5E, 0x41, 0x43, 0xF9, 0xC1,
    0x9F, 0x8F, 0x0A, 0x73, 0xFF, 0x11, 0x66, 0x38, 0x55, 0xED, 0x60, 0x49,
    0x25, 0x5F, 0x5B, 0x3A, 0x1D, 0x6C, 0x09, 0xBB, 0x24, 0xA6, 0x18, 0xC0,
    0x1D, 0x12, 0x01, 0xF0, 0x90, 0x5F, 0xA7, 0xC5, 0x3F, 0x8C, 0xA4, 0x29,
    0x3D, 0x99, 0x7B, 0x52, 0x0F, 0xA0, 0x04, 0x60, 0x8C, 0x8E, 0xA7, 0xC0,
    0x1D, 0x12, 0x00, 0xE8, 0xC0, 0x5F, 0xA7, 0xC7, 0xBF, 0x8C, 0x8E, 0xA9,
    0x19, 0x78, 0x3F, 0xFF, 0xD9,
];

/// The same image again at `-q 20` and truncated to a twentieth, so its
/// code-blocks stop part way through a bit-plane and E.1.1.2's reconstruction
/// point has to be spent per coefficient rather than per block.
#[rustfmt::skip]
const JPX_TRUNCATED: [u8; 246] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
    0x00, 0x00, 0x00, 0x14, 0x66, 0x74, 0x79, 0x70, 0x6A, 0x70, 0x32, 0x20,
    0x00, 0x00, 0x00, 0x00, 0x6A, 0x70, 0x32, 0x20, 0x00, 0x00, 0x00, 0x2D,
    0x6A, 0x70, 0x32, 0x68, 0x00, 0x00, 0x00, 0x16, 0x69, 0x68, 0x64, 0x72,
    0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x20, 0x00, 0x03, 0x07, 0x07,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x63, 0x6F, 0x6C, 0x72, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0xA9, 0x6A, 0x70, 0x32,
    0x63, 0xFF, 0x4F, 0xFF, 0x51, 0x00, 0x2F, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x20, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x07, 0x01, 0x01, 0x07, 0x01,
    0x01, 0x07, 0x01, 0x01, 0xFF, 0x52, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x01,
    0x01, 0x01, 0x04, 0x04, 0x00, 0x00, 0xFF, 0x5C, 0x00, 0x0B, 0x42, 0x48,
    0x24, 0x57, 0xD3, 0x57, 0xD3, 0x57, 0x62, 0xFF, 0x64, 0x00, 0x25, 0x00,
    0x01, 0x43, 0x72, 0x65, 0x61, 0x74, 0x65, 0x64, 0x20, 0x62, 0x79, 0x20,
    0x4F, 0x70, 0x65, 0x6E, 0x4A, 0x50, 0x45, 0x47, 0x20, 0x76, 0x65, 0x72,
    0x73, 0x69, 0x6F, 0x6E, 0x20, 0x32, 0x2E, 0x35, 0x2E, 0x30, 0xFF, 0x90,
    0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A, 0x00, 0x01, 0xFF, 0x93,
    0xCA, 0xB0, 0x11, 0x50, 0x4F, 0x95, 0xA6, 0x9E, 0xA1, 0x94, 0x82, 0x06,
    0xD6, 0x80, 0xCA, 0x90, 0x46, 0x3B, 0x66, 0xF3, 0xF4, 0x58, 0xC3, 0x51,
    0x57, 0x80, 0x80, 0x80, 0xFF, 0xD9,
];

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

/// Transparency groups and a soft mask: what clause 11 does beyond constant
/// alpha and the blend modes.
///
/// *Added August 2026, with gap 11.* Not one fixture above has a `/Group` or
/// an ExtGState `/SMask`, so every one of them renders identically on a build
/// that has never heard of 11.4 or 11.6 — which is the failure this file's own
/// documentation describes, and it applied to the whole of group compositing
/// while group compositing was being written.
///
/// Five things here are in no other fixture, and each is a different branch of
/// the compositing arithmetic:
///
/// - a **non-isolated** group at `ca 0.5` under `Multiply` over a coloured
///   bar, so the backdrop is composited in, blended against, and removed again
///   by 11.4.7.2 — the removal term is non-zero only because the group's own
///   alpha is partial, which is what makes this fixture able to see it;
/// - two overlapping shapes inside that group, so the seam that per-element
///   alpha produces would move the hash;
/// - a **knockout** group whose two half-opaque shapes overlap, so 11.4.5's
///   restore runs on real coverage rather than on a rectangle;
/// - a `/Luminosity` soft mask whose group is an axial **shading**, giving a
///   different mask value in almost every column — a mask made of one flat
///   grey would be reproduced by a great many wrong implementations;
/// - a `/TR` on that mask, applied to the backdrop as well as to the group.
///
/// The mask's `/BC` is deliberately absent, so the region outside its group's
/// bounding box is the black default and the red bar is *cut off* there. That
/// is ink the page does not have: under-suppression adds it back, which no
/// minimum-ink floor can catch and the hash catches immediately.
fn transparency_page() -> Vec<u8> {
    let content = "0.2 0.7 0.4 rg 0 0 120 26 re f\n\
                   q /GA gs /Grp Do Q\n\
                   q /GB gs /Knock Do Q\n\
                   q /GM gs 0.9 0.2 0.1 rg 0 30 120 46 re f Q";
    // `/GH` inside the group is what makes 11.4.7.2 visible here: the
    // removal term is `a0/agn - a0`, which is exactly zero when the group
    // painted opaquely, so a fixture whose contents are opaque cannot see
    // backdrop removal at all. Measured, not assumed -- the first draft of
    // this page had no `/GH` here and deleting the removal step did not move
    // its hash.
    let grouped = "/GH gs 0 0 0.9 rg 6 4 34 34 re f 0.9 0.6 0 rg 20 14 34 34 re f";
    let knocked = "/GH gs 0.1 0.1 0.1 rg 62 4 34 34 re f 78 14 34 34 re f";
    let mask = "q 0 30 120 46 re W n /Sh0 sh Q";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /XObject << /Grp 5 0 R /Knock 6 0 R >>\n\
                 /Shading << /Sh0 9 0 R >>\n\
                 /ExtGState << /GA << /ca 0.5 /BM /Multiply >>\n\
                               /GB << /ca 1 /BM /Normal /SMask /None >>\n\
                               /GH << /ca 0.5 >>\n\
                               /GM << /SMask << /S /Luminosity /G 7 0 R\n\
                                  /TR 8 0 R >> >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 58 56]\n\
   /Group << /S /Transparency /I false /K false >>\n\
   /Length {} >>\nstream\n{grouped}\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [58 0 118 56]\n\
   /Group << /S /Transparency /I true /K true >>\n\
   /Length {} >>\nstream\n{knocked}\nendstream\nendobj\n\
7 0 obj\n<< /Type /XObject /Subtype /Form /BBox [10 30 110 76]\n\
   /Group << /S /Transparency /CS /DeviceGray >>\n\
   /Length {} >>\nstream\n{mask}\nendstream\nendobj\n\
8 0 obj\n<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >>\nendobj\n\
9 0 obj\n<< /ShadingType 2 /ColorSpace /DeviceGray /Coords [8 0 112 0]\n\
   /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>\n\
   /Extend [true true] >>\nendobj\n\
trailer\n<< /Size 10 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        grouped.len(),
        knocked.len(),
        mask.len()
    )
    .into_bytes()
}

/// Tiling patterns: a lattice of cells rasterised once and blitted, as a fill
/// and as a stroke, coloured and uncoloured.
///
/// *Added August 2026, with gap 09.* The `pattern` fixture above is a
/// `PatternType 2` shading and reaches none of this — a shading pattern is
/// evaluated per pixel through an inverse transform, where a tiling pattern is
/// an offscreen buffer composited at integer offsets. Six things here are in
/// no other fixture:
///
/// - a **lattice**, so the range arithmetic and its rounding to device pixels
///   are in the hash rather than only the one cell;
/// - a rotated pattern `/Matrix`, so every lattice offset is a vector with all
///   four coefficients in it and the cell's own device rectangle is the hull
///   of a rotated quad;
/// - a `/XStep` and `/YStep` that are **not** the `/BBox`, one wider and one
///   narrower, so both the gap and the overlap are measured;
/// - the cell clipped to its `/BBox` while its content deliberately overshoots
///   it, which is how a real hatch joins;
/// - a **stroked** outline filled with a lattice, under a paint-time CTM that
///   is not the identity, so 8.7.3.1's anchoring is pinned as bytes on the
///   route gap 07 opened and could not test;
/// - a `PaintType 2` cell, taking its colour from the `SCN` operands on the
///   **stroking** slot — the one thing in this engine that had never been
///   rendered at all.
fn tiling_page() -> Vec<u8> {
    let content = "/Pattern cs /P0 scn\n\
                   6 6 m 108 6 l 108 50 l 40 50 l h f\n\
                   /Cs1 CS 0.1 0.3 0.9 /P1 SCN 7 w 1 J 1 j\n\
                   q 1.25 0 0 1.25 4 4 cm\n\
                   6 46 m 30 60 60 40 86 56 c S\n\
                   Q";
    // The cell overshoots its box on every side, which the `/BBox` clip takes
    // back: without it the overshoot lands in the gap the 11 pt step leaves.
    let coloured = "0.9 0.2 0.1 rg -2 -2 8 12 re f\n\
                    0 0.5 0.2 rg 4 4 8 8 re f";
    // Uncoloured: these colour operators mean nothing (8.7.3.3), and if they
    // ever start meaning something this hash moves.
    let uncoloured = "0 1 0 rg 0 0 3 7 re f 1 1 0 rg 3 3 4 4 re f";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /Pattern << /P0 5 0 R /P1 6 0 R >>\n\
                 /ColorSpace << /Cs1 [/Pattern /DeviceRGB] >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /PatternType 1 /PaintType 1 /TilingType 1\n\
   /BBox [0 0 10 10] /XStep 11 /YStep 8\n\
   /Matrix [0.9659 0.2588 -0.2588 0.9659 3 -5]\n\
   /Resources << >> /Length {} >>\nstream\n{coloured}\nendstream\nendobj\n\
6 0 obj\n<< /PatternType 1 /PaintType 2 /TilingType 1\n\
   /BBox [0 0 7 7] /XStep 5 /YStep 6\n\
   /Matrix [1 0 0 1 -2 1]\n\
   /Resources << >> /Length {} >>\nstream\n{uncoloured}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        coloured.len(),
        uncoloured.len()
    )
    .into_bytes()
}

/// Packs values of a fixed bit width, most significant bit first — the layout
/// 8.7.4.5.5 gives a mesh shading's vertex stream.
struct Packed {
    data: Vec<u8>,
    bit: u32,
}

impl Packed {
    fn new() -> Packed {
        Packed {
            data: Vec::new(),
            bit: 0,
        }
    }

    fn push(&mut self, value: u64, bits: u32) -> &mut Packed {
        for index in (0..bits).rev() {
            if self.bit == 0 {
                self.data.push(0);
            }
            if let Some(byte) = self.data.last_mut() {
                *byte |= (((value >> index) & 1) as u8) << (7 - self.bit);
            }
            self.bit = (self.bit + 1) % 8;
        }
        self
    }

    /// A fraction of the way through a `/Decode` range, at the given width.
    fn ratio(&mut self, value: f64, bits: u32) -> &mut Packed {
        let top = ((1u64 << bits) - 1) as f64;
        self.push((value.clamp(0.0, 1.0) * top).round() as u64, bits)
    }

    /// 8.7.4.5.5 pads each vertex of a type 4 stream to a byte boundary, and
    /// 8.7.4.5.7 each patch of a type 6 or 7 one.
    fn align(&mut self) -> &mut Packed {
        self.bit = 0;
        self
    }
}

/// The four mesh streams of [`mesh_page`], as `ASCIIHexDecode` text.
///
/// Each is a different corner of 8.7.4.5.5 to 8.7.4.5.8, and between them they
/// use every packed width class the spec allows: one that is a whole byte, one
/// that is two, one that is neither (12 bits), and flag widths of 8, 4 and 2.
struct MeshStreams {
    free_form: String,
    lattice: String,
    coons: String,
    tensor: String,
}

fn mesh_streams() -> MeshStreams {
    // Type 4, 16-bit coordinates, three `/DeviceRGB` components, and all three
    // edge flags. A fan: the first triangle by flag 0, then two continuations
    // by flag 2, which keep the *first* and third vertices and so sweep round
    // a shared corner, and finally one by flag 1, which keeps the second and
    // third and lands a triangle overlapping the fan. The fan covers its band
    // exactly; the overlap is there so the non-zero accumulation has somewhere
    // to reach a winding of two.
    let mut free_form = Packed::new();
    let vertices: [(u64, f64, f64, [f64; 3]); 6] = [
        (0, 0.0, 0.0, [0.90, 0.15, 0.10]),
        (0, 40.0, 0.0, [0.15, 0.85, 0.25]),
        (0, 40.0, 40.0, [0.20, 0.30, 0.95]),
        (2, 40.0, 80.0, [0.95, 0.85, 0.15]),
        (2, 0.0, 80.0, [0.10, 0.55, 0.60]),
        (1, 20.0, 60.0, [0.55, 0.10, 0.70]),
    ];
    for (flag, x, y, rgb) in vertices {
        free_form.push(flag, 8);
        free_form.ratio(x / 120.0, 16).ratio(y / 80.0, 16);
        for component in rgb {
            free_form.ratio(component, 8);
        }
        free_form.align();
    }

    // Type 5: no flags at all, 16-bit *components* through a `/Function`, so
    // each vertex carries one parametric value rather than a colour. The
    // interior row is deliberately off the grid, so the cells are not
    // rectangles and the barycentric weights are not axis-aligned.
    let mut lattice = Packed::new();
    let rows: [[(f64, f64); 4]; 3] = [
        [(40.0, 0.0), (53.0, 0.0), (67.0, 0.0), (80.0, 0.0)],
        [(40.0, 40.0), (56.0, 33.0), (64.0, 46.0), (80.0, 40.0)],
        [(40.0, 80.0), (53.0, 80.0), (67.0, 80.0), (80.0, 80.0)],
    ];
    for (row, columns) in rows.iter().enumerate() {
        for (column, (x, y)) in columns.iter().enumerate() {
            // Neither linear nor periodic across the lattice: a linear ramp is
            // reproduced by almost any wrong interpolation, which is what gap
            // 12 found the hard way in the image fixture.
            let t = ((column * 5 + row * 7) % 11) as f64 / 10.0;
            lattice.ratio(x / 120.0, 8).ratio(y / 80.0, 8).ratio(t, 16);
        }
    }

    // Type 6: a Coons patch with two bowed edges, in a `/Separation` whose
    // tint transform is cubic — so the colour at the middle of the patch is
    // 87 per cent of the way to one end rather than halfway, and interpolating
    // in RGB instead moves it a long way. Four-bit flags.
    let mut coons = Packed::new();
    coons.push(0, 4);
    let patch: [(f64, f64); 12] = [
        (80.0, 0.0),
        (76.0, 30.0),
        (84.0, 54.0),
        (80.0, 80.0),
        (93.0, 66.0),
        (107.0, 90.0),
        (120.0, 80.0),
        (116.0, 52.0),
        (124.0, 26.0),
        (120.0, 0.0),
        (107.0, 14.0),
        (93.0, -10.0),
    ];
    for (x, y) in patch {
        coons.ratio(x / 120.0, 8).ratio(y / 80.0, 8);
    }
    for tint in [0.05, 0.95, 0.40, 0.70] {
        coons.ratio(tint, 8);
    }
    coons.align();

    // Type 7: a tensor patch whose four internal control points are dragged
    // well off the flat grid, in `/DeviceCMYK` — four components — at twelve
    // bits per coordinate, which is the width class that is not a whole number
    // of bytes, with four-bit components and two-bit flags.
    let mut tensor = Packed::new();
    tensor.push(0, 2);
    let boundary: [(f64, f64); 12] = [
        (4.0, 4.0),
        (0.0, 28.0),
        (8.0, 52.0),
        (4.0, 76.0),
        (42.0, 80.0),
        (78.0, 72.0),
        (116.0, 76.0),
        (120.0, 52.0),
        (112.0, 28.0),
        (116.0, 4.0),
        (78.0, 8.0),
        (42.0, 0.0),
    ];
    let interior: [(f64, f64); 4] = [(20.0, 60.0), (36.0, 18.0), (96.0, 62.0), (88.0, 12.0)];
    for (x, y) in boundary.into_iter().chain(interior) {
        tensor.ratio(x / 120.0, 12).ratio(y / 80.0, 12);
    }
    for cmyk in [
        [0.10, 0.80, 0.90, 0.00],
        [0.90, 0.20, 0.10, 0.10],
        [0.20, 0.10, 0.85, 0.05],
        [0.75, 0.70, 0.00, 0.20],
    ] {
        for component in cmyk {
            tensor.ratio(component, 4);
        }
    }
    tensor.align();

    MeshStreams {
        free_form: hex(&free_form.data),
        lattice: hex(&lattice.data),
        coons: hex(&coons.data),
        tensor: hex(&tensor.data),
    }
}

/// Mesh shadings: all four types, on one page (8.7.4.5.5 to 8.7.4.5.8).
///
/// *Added August 2026, with gap 10.* Not one of the ten fixtures above draws a
/// mesh — the types were a capability gate until this gap — so every one of
/// them renders identically on a build that has never heard of a Gouraud
/// triangle. This file's own documentation names that failure: a fingerprint
/// is not evidence about a path no fixture reaches.
///
/// Seven things here are in no other fixture, and each is a different branch:
///
/// - a **type 4** free-form strip carrying all three edge flags, so the
///   continuation rule of 8.7.4.5.5 is in the hash rather than only the
///   vertices — a flag read as the other one still draws a mesh, hinged
///   elsewhere;
/// - a **type 5** lattice through a `/Function`, so the parametric form and
///   the direct-component form are both covered, with an interior row off the
///   grid so no cell is a rectangle;
/// - a **type 6** Coons patch with bowed boundaries, in a `/Separation` whose
///   tint transform is **cubic** — which is what makes this fixture able to
///   see colour interpolated in RGB instead of in the shading's own space. A
///   linear space could not: the two orders agree everywhere on a straight
///   line, which is the trap gap 12 recorded for the image fixture;
/// - a **type 7** tensor patch whose four internal control points are dragged
///   off the flat grid, so a build that recomputed them by 8.7.4.5.7's Coons
///   formula would move;
/// - the **subdivision count**, which is chosen from the device transform, so
///   the patch grid is in these bytes and a step count that came out one
///   different on another target would show here and nowhere else;
/// - a **shading pattern over a mesh**, filling a path under a rotated
///   `/Matrix`, which is the route an Illustrator gradient mesh actually takes
///   into a file;
/// - every packed width class the spec allows: 16 bits per coordinate, 12 bits
///   (which is not a whole number of bytes), 16 bits per component, 4 bits per
///   component, and flag widths of 8, 4 and 2.
fn mesh_page() -> Vec<u8> {
    let streams = mesh_streams();
    let content = "q 0 0 40 80 re W n /Sh4 sh Q\n\
                   q 40 0 40 80 re W n /Sh5 sh Q\n\
                   q 80 0 40 80 re W n /Sh6 sh Q\n\
                   /Pattern cs /P0 scn\n\
                   q 1.1 0 0 1.1 2 2 cm\n\
                   14 10 m 88 18 l 80 58 l 26 54 l h f\n\
                   Q";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /Shading << /Sh4 5 0 R /Sh5 6 0 R /Sh6 7 0 R >>\n\
                 /Pattern << /P0 8 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 16\n\
   /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 120 0 80 0 1 0 1 0 1]\n\
   /Filter /ASCIIHexDecode /Length {} >>\nstream\n{}\nendstream\nendobj\n\
6 0 obj\n<< /ShadingType 5 /ColorSpace /DeviceRGB /VerticesPerRow 4\n\
   /BitsPerCoordinate 8 /BitsPerComponent 16 /Decode [0 120 0 80 0 1]\n\
   /Function << /FunctionType 2 /Domain [0 1] /C0 [0.05 0.20 0.50]\n\
                /C1 [0.95 0.60 0.10] /N 2 >>\n\
   /Filter /ASCIIHexDecode /Length {} >>\nstream\n{}\nendstream\nendobj\n\
7 0 obj\n<< /ShadingType 6 /BitsPerCoordinate 8 /BitsPerComponent 8\n\
   /BitsPerFlag 4 /Decode [0 120 0 80 0 1]\n\
   /ColorSpace [/Separation /Ink /DeviceRGB\n\
     << /FunctionType 2 /Domain [0 1] /C0 [0.95 0.90 0.80]\n\
        /C1 [0.10 0.15 0.45] /N 3 >>]\n\
   /Filter /ASCIIHexDecode /Length {} >>\nstream\n{}\nendstream\nendobj\n\
8 0 obj\n<< /PatternType 2 /Matrix [0.866 0.5 -0.5 0.866 24 -14]\n\
   /Shading 9 0 R >>\nendobj\n\
9 0 obj\n<< /ShadingType 7 /ColorSpace /DeviceCMYK /BitsPerCoordinate 12\n\
   /BitsPerComponent 4 /BitsPerFlag 2\n\
   /Decode [0 120 0 80 0 1 0 1 0 1 0 1]\n\
   /Filter /ASCIIHexDecode /Length {} >>\nstream\n{}\nendstream\nendobj\n\
trailer\n<< /Size 10 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        streams.free_form.len(),
        streams.free_form,
        streams.lattice.len(),
        streams.lattice,
        streams.coons.len(),
        streams.coons,
        streams.tensor.len(),
        streams.tensor,
    )
    .into_bytes()
}

/// A comic archive, whose document is **synthesised at open** rather than
/// parsed from the bytes handed in.
///
/// *Added August 2026, with gap 29 milestone 6.* Every fixture above is a PDF:
/// `Document::open` parses it, and what the hash covers is the renderer. This
/// one is a ZIP of page images, and `Document::open` **builds** the document —
/// through [`tinker_pdf::DocumentBuilder`], out through the writer and back in
/// through `CosDocument::open` — so the hash covers the whole of that as well.
/// Nothing else in this file would move if the synthesiser started writing a
/// different document.
///
/// # What a move in this hash means
///
/// Wider than any other entry here, and worth reading before treating a
/// failure as a rendering change. This one moves if **any** of these changes
/// its answer:
///
/// - the ZIP reader — which entries exist, in what order, and what bytes they
///   hold (`tinker-pdf-zip`);
/// - the natural sort, because the first page is deliberately **not** first in
///   the archive and not first lexicographically either;
/// - the classification of an entry by its magic bytes, since a `.gif` here is
///   a placeholder page and a `.xml` is not a page at all;
/// - `png_scan`, `png_image`'s routing table, and the pass-through's
///   `/DecodeParms`;
/// - the PNG decoder and the `/SMask` split, for the one page that takes the
///   fallback route;
/// - `DocumentBuilder` — object numbering, dictionary key order, stream
///   framing — and `CosDocument`'s parse of what it wrote;
/// - the page geometry (one image pixel is one PDF point), and therefore the
///   bitmap's own dimensions, which are in the hash;
/// - and the renderer, as for every fixture above.
///
/// That breadth is the point rather than a defect: a fingerprint over a
/// synthesised document is the only thing in this repository that pins the
/// producer and the consumer to each other. But it means "the CBZ fingerprint
/// moved" is not by itself a rendering finding, and the failure has to be
/// attributed before the table is touched.
/// [`the_synthesised_document_is_the_same_bytes_on_every_target`] narrows it:
/// if that hash moved too, the synthesiser changed; if it did not, the renderer
/// did.
///
/// # The pages, and why each is here
///
/// Five entries, stored **back to front** so that the reading order is
/// something the fixture proves rather than something it inherits:
///
/// - `page2.png` is an **indexed** PNG at **four bits** a sample with a `tRNS`
///   naming a contiguous transparent run, so it passes through as
///   `/FlateDecode` with `/Predictor 15`, an `/Indexed` colour space whose
///   `/hival` is derived from the palette, and 8.9.6.4's colour-key `/Mask`.
///   It is page **0**, so it is the page the fingerprint above renders. A
///   sub-byte depth is deliberate: nothing else in this file packs samples
///   narrower than a byte except the JBIG2 stencil, and the `/Mask` compares
///   raw four-bit indices;
/// - `page3.png` is **RGBA**, which cannot pass through, so it takes the
///   decoder and is split into samples and an `/SMask`;
/// - `page4.gif` is a GIF, which this build refuses **by name at the page
///   level**: it becomes a placeholder page of the book's own size, keeping its
///   page number, which is gap 29's first "where a half-implementation is worse
///   than none";
/// - `page10.jpg` is a baseline JPEG, placed verbatim. No other fixture in this
///   file embeds one at all;
/// - `ComicInfo.xml` is metadata: not a page and not a warning, and **since
///   tier 4's W-ARCHIVE milestone 1 it is read**. Its `<Title>None</Title>`
///   reaches the synthesised document's `/Info`, which is why the document
///   hash below moved in that commit and the render fingerprint above did
///   not — the attribution this test's own doc comment asks for, made in the
///   direction it describes.
///
/// The names are **unpadded** on purpose. `page001` and `page010` sort
/// identically under lexicographic and natural order, so a padded fixture
/// proves nothing about ordering at all — gap 16's zero-padded-filename lesson,
/// which gap 29's design section restates for this format. Here the archive's
/// stored order, its lexicographic order and its natural order all disagree
/// about which entry is page 0: directory order and lexicographic order both
/// put `page10.jpg` first (16 x 16, a JPEG), and only the natural order this
/// build implements puts `page2.png` there (40 x 40, indexed). So a regression
/// to either wrong order changes the picture *and* the bitmap's dimensions.
fn cbz_page() -> Vec<u8> {
    zip_archive(&[
        ("page10.jpg", grey_jpeg(16, 16)),
        // Six bytes of signature and a screen descriptor: enough to be
        // recognised as a GIF, which is all this entry is for.
        ("page4.gif", b"GIF89a\x08\x00\x08\x00\x00\x00\x00".to_vec()),
        ("page3.png", rgba_png(24, 24)),
        ("page2.png", indexed_png(40, 40)),
        (
            "ComicInfo.xml",
            b"<ComicInfo><Title>None</Title></ComicInfo>".to_vec(),
        ),
    ])
}

/// A ZIP of stored entries, from APPNOTE 6.3.10's field layouts.
///
/// Stored rather than deflated, and it matters for what this fixture is: a
/// deflated entry would put `deflate`'s output in the archive, so a change to
/// the *encoder* would change the fixture's own bytes. Everything here is
/// either a literal or a checksum.
fn zip_archive(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = tinker_pdf_filters::crc32(data);
        let name = name.as_bytes();

        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&(1u16 << 11).to_le_bytes()); // bit 11: UTF-8
        out.extend_from_slice(&0u16.to_le_bytes()); // method 0: stored
        out.extend_from_slice(&0u32.to_le_bytes()); // time and date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&(1u16 << 11).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // method
        central.extend_from_slice(&0u32.to_le_bytes()); // time and date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes()); // extra, comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }

    let directory_at = out.len() as u32;
    let count = files.len() as u16;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u32.to_le_bytes()); // this disk, directory disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
    out
}

/// 15948 5.3: length, type, data, and a CRC over the type and the data.
fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut covered = kind.to_vec();
    covered.extend_from_slice(data);
    out.extend_from_slice(&tinker_pdf_filters::crc32(&covered).to_be_bytes());
    out
}

/// A zlib stream (RFC 1950) whose deflate blocks are all **stored**.
///
/// Written out rather than produced by `zlib_compress` for the same reason the
/// archive above stores its entries: the pass-through copies these bytes into
/// the PDF verbatim, so a compressed IDAT would put the *encoder's* output into
/// a fingerprint that is supposed to be about the decoder, the embedder and the
/// writer. `0x7801` passes the modulo-31 check with FDICT clear and FLEVEL 0.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(0xFFFF).peekable();
    // An empty input still needs one block, or there is no final block at all.
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none())); // BFINAL, BTYPE 00
        out.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// 15948 9.2's five row filters, applied over `stride` bytes a row with the
/// left-neighbour offset the format gives a sample of this width.
///
/// The tag cycles rather than being zero on every row, because `/Predictor 15`
/// exists precisely so that the filter may change from row to row, and a raster
/// filtered entirely with type 0 exercises none of it.
fn png_filter(rows: &[Vec<u8>], bpp: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut previous = vec![0u8; rows.first().map_or(0, Vec::len)];
    for (y, row) in rows.iter().enumerate() {
        let tag = (y % 5) as u8;
        out.push(tag);
        for i in 0..row.len() {
            let a = if i >= bpp { i32::from(row[i - bpp]) } else { 0 };
            let b = i32::from(previous[i]);
            let c = if i >= bpp {
                i32::from(previous[i - bpp])
            } else {
                0
            };
            let predictor = match tag {
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    // 9.4's Paeth predictor, written out so this encoder is
                    // the specification's rather than the engine's.
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => 0,
            };
            out.push((i32::from(row[i]) - predictor) as u8);
        }
        previous.clone_from(row);
    }
    out
}

/// A four-bit indexed PNG with a `tRNS` naming a transparent run.
///
/// Sixteen palette entries, none of them white, so every opaque pixel counts as
/// ink; indices 0 and 1 are fully transparent, which is the shape 8.9.6.4's
/// `/Mask` expresses as the single inclusive range `[0 1]`. Two samples share
/// each byte, so the predictor's left-neighbour offset is one byte rather than
/// one sample — `ceil(1 x 4 / 8)`, floored at one.
fn indexed_png(width: u32, height: u32) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[4, 3, 0, 0, 0]); // depth 4, colour type 3

    let mut palette = Vec::with_capacity(48);
    for i in 0..16u32 {
        palette.push((i * 16 + 8) as u8);
        palette.push((240 - i * 15) as u8);
        palette.push(((i * 37 + 11) % 251) as u8);
    }

    let stride = (width as usize).div_ceil(2);
    let mut rows = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut row = vec![0u8; stride];
        for x in 0..width {
            // Neither periodic in x nor in y, so a row or column dropped is
            // visible rather than absorbed by the pattern repeating.
            let index = ((x * 3 + y * 5 + (x * y) / 7) % 16) as u8;
            let at = (x / 2) as usize;
            if x % 2 == 0 {
                row[at] |= index << 4;
            } else {
                row[at] |= index;
            }
        }
        rows.push(row);
    }

    let mut out = Vec::from(PNG_SIGNATURE);
    out.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&png_chunk(b"PLTE", &palette));
    // Indices 0 and 1 transparent, and every index past the array opaque.
    out.extend_from_slice(&png_chunk(b"tRNS", &[0x00, 0x00]));
    out.extend_from_slice(&png_chunk(b"IDAT", &zlib_stored(&png_filter(&rows, 1))));
    out.extend_from_slice(&png_chunk(b"IEND", b""));
    out
}

/// An eight-bit RGBA PNG, which is the routing table's "cannot pass through".
///
/// The alpha is a function of both axes so the `/SMask` the embedder splits out
/// is a picture rather than a constant, and no pixel is fully transparent, so
/// the whole raster is ink.
fn rgba_png(width: u32, height: u32) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // depth 8, colour type 6

    let mut rows = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut row = Vec::with_capacity(width as usize * 4);
        for x in 0..width {
            row.push((x * 9 + 3) as u8);
            row.push((y * 9 + 5) as u8);
            row.push(((x * x + y * 3) % 256) as u8);
            row.push((64 + (x * 5 + y * 7) % 192) as u8);
        }
        rows.push(row);
    }

    let mut out = Vec::from(PNG_SIGNATURE);
    out.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&png_chunk(b"IDAT", &zlib_stored(&png_filter(&rows, 4))));
    out.extend_from_slice(&png_chunk(b"IEND", b""));
    out
}

/// 15948 5.2's eight-byte signature.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A baseline JPEG (ITU-T T.81), DC only, one grey component.
///
/// `width` and `height` must be multiples of eight. Two DC codes are defined so
/// that a multi-block image can say "no change" after the first, which is what
/// makes this a fixture rather than an 8 x 8 special case. It reaches the page
/// as `ImageData::Jpeg` — the bytes the archive holds, never re-encoded.
fn grey_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];

    // DQT, table 0, with a large DC quantiser so the one coded coefficient is
    // a value nobody could mistake for a blank page.
    let mut quant = [1u8; 64];
    quant[0] = 255;
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend_from_slice(&quant);

    // SOF0: baseline, 8-bit, one component, no subsampling, table 0.
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);

    // DHT DC table 0: two two-bit codes, `00` for size 0 and `01` for size 2.
    let mut dc = vec![0x00, 0, 2];
    dc.extend_from_slice(&[0u8; 14]);
    dc.extend_from_slice(&[0x00, 0x02]);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dc.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dc);

    // DHT AC table 0: one one-bit code, end-of-block.
    let mut ac = vec![0x10, 1];
    ac.extend_from_slice(&[0u8; 15]);
    ac.push(0x00);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((ac.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&ac);

    // SOS: one component, the whole spectral band, no successive
    // approximation, which is what makes it sequential rather than progressive.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);

    let blocks = usize::from(width).div_ceil(8) * usize::from(height).div_ceil(8);
    let mut coded: Vec<u8> = Vec::new();
    for block in 0..blocks {
        // Size 2 then `00`, which F.1.2.1's table reads as -3: a DC of -765
        // after dequantisation, which the level shift puts near 32. Every
        // block after the first codes size 0, which is "no change".
        coded.extend_from_slice(if block == 0 { &[0, 1, 0, 0] } else { &[0, 0] });
        coded.push(0); // the AC end-of-block
    }
    while coded.len() % 8 != 0 {
        coded.push(1);
    }
    for byte in coded.chunks(8) {
        let packed = byte.iter().fold(0u8, |acc, bit| (acc << 1) | bit);
        out.push(packed);
        // B.1.1.5: a 0xFF inside the entropy-coded segment is byte-stuffed.
        if packed == 0xFF {
            out.push(0x00);
        }
    }

    out.extend_from_slice(&[0xFF, 0xD9]);
    out
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

/// **The fourteenth fingerprint's page, and the first in this file whose input
/// this repository did not author.**
///
/// Every other fixture here is bytes assembled a few lines above it, from a
/// specification this repository read. That is the whole of gap 29's closing
/// limitation — *"the first real archive this meets may find something, and
/// nothing here would have"* — arriving in the one file where it matters most,
/// because a fingerprint over a document we wrote pins this engine to its own
/// reading of a format rather than to the format.
///
/// This one is `wpf-image-and-text.xps` from
/// [milestone 1's corpus](xps/README.md): every byte of it was written by
/// `System.Windows.Xps.Packaging.XpsDocument`, WPF's `ReachFramework`
/// serialiser, on Windows 11. `include_bytes!` rather than `std::fs`, because
/// this file runs on `wasm32-wasip1` under wasmtime with no preopened
/// directory and a fixture that cannot be read is a fixture that cannot be
/// hashed.
///
/// # Why this package and not one of the other seven
///
/// All eight render with nothing owed after milestone 8, so any of them would
/// produce a stable hash. This is the only one that puts **every** layer gap 30
/// built on one page:
///
/// - OPC — `[Content_Types].xml` with an `Override`, `/_rels/.rels`, a part's
///   derived `_rels` name, and a relative target resolved against its source
///   part;
/// - the spine — `FixedDocumentSequence` to `FixedDocument` to `FixedPage`,
///   through relationships and media types rather than extensions;
/// - `FixedPage.Resources`, a `ResourceDictionary` and a `{StaticResource}`,
///   which is 14.2.5's scoping and the reason milestone 6's row exists;
/// - an `ImageBrush` over a PNG part, **passed through** rather than decoded,
///   with 13.4.1's resolution deciding the viewbox's units, painted as a PDF
///   tiling pattern with the four-times cell flips do not need here;
/// - a `Glyphs` run with `Indices=",53"` — the empty-glyph-index form nothing
///   in ECMA-388 predicts — through 9.1.7.3's ODTTF de-obfuscation and out to
///   milestone 5's Type0/CIDFontType2 font with `/Identity-H`.
///
/// So a change anywhere from the ZIP reader to the glyph rasteriser moves it,
/// which is the same breadth `cbz_page` carries and more of it: the CBZ
/// fixture reaches no XML, no font and no pattern at all.
///
/// # What a move in this hash means
///
/// The same attribution problem `cbz_page`'s doc comment sets out, one format
/// wider. This moves if the ZIP reader, the XML parser, the OPC layer, the
/// spine, the markup walk, the brush grammar, the geometry parser, the ODTTF
/// key arithmetic, `DocumentBuilder`, `CosDocument`'s parse of what it wrote,
/// or the renderer changes its answer.
/// [`the_synthesised_fixed_document_is_the_same_bytes_on_every_target`] is what
/// narrows it: if that hash moved too, the synthesiser changed; if it did not,
/// the renderer did.
fn xps_package() -> Vec<u8> {
    include_bytes!("xps/wpf-image-and-text.xps").to_vec()
}

/// **The fifteenth fingerprint's book, and the first page here whose page
/// number is a caller's decision.**
///
/// `pandoc-book-cover.epub` from [milestone 1's corpus](epub/README.md): pandoc
/// 3.10.2 wrote every byte of it on 19 August 2026, from `source/book.md` in
/// that directory. It is the second real producer's file in this file — gap 30's
/// XPS was the first — and `include_bytes!` rather than `std::fs` for the same
/// reason, because this test runs on `wasm32-wasip1` under wasmtime with no
/// preopened directory.
///
/// # Why this book and not one of the other five
///
/// All six read with nothing owed, so any of them would hash stably. This is
/// the only one that puts the whole of gap 31 on one spine:
///
/// - OCF — `META-INF/container.xml`, a `full-path` resolved from the container
///   root, and pandoc's seventh `META-INF` entry that §4.2.6.3 does not reserve;
/// - the package document — EPUB 3.0, a manifest of eight, a spine of five, and
///   **both** a navigation document and an NCX, which milestone 1 found only
///   this producer writes;
/// - the doctype — `<!DOCTYPE html>` on every content document, which the
///   parser refused outright until milestone 2 gave it a mode;
/// - the cascade — 276 qualified rules and 582 declarations out of pandoc's own
///   stylesheet, over the user-agent sheet milestone 8 wrote;
/// - layout — 326 boxes, 3 845 charged break opportunities, UAX #14's line
///   breaking over an em dash, an ellipsis, a non-breaking hyphen and a
///   Japanese line, and pagination into **seven** pages from five itemrefs;
/// - the writer — eight `/Link` annotations and an outline, which is milestone
///   5's work and reaches no other fixture in this file.
///
/// So a change anywhere from the ZIP reader to the line breaker moves it, which
/// is a wider blast radius than `cbz_page`'s or `xps_package`'s: neither of
/// those reaches a stylesheet, a box tree or a line break at all.
///
/// # What a move in this hash means
///
/// The same attribution problem the other two carry, one format wider again.
/// [`the_synthesised_book_is_the_same_bytes_on_every_target`] is what narrows
/// it: if that hash moved too, the reader or the writer changed; if it did not,
/// the renderer did. And
/// [`a_book_is_stable_at_each_page_box_and_the_two_boxes_differ`] is what says
/// the page this hash is over is the page the caller's box produced.
fn epub_book() -> Vec<u8> {
    include_bytes!("epub/pandoc-book-cover.epub").to_vec()
}

/// Default colour spaces (8.6.5.6): a page whose device colours are drawn
/// through the defaults its `/ColorSpace` names.
///
/// *Added October 2026, with K-02.2.* No other fixture here names a default,
/// so a build that stopped substituting one would move nothing in this file.
/// This page names all three, each through different arithmetic:
/// `/DefaultGray` a CalGray, whose gamma is `tinker-pdf-math`'s `pow`;
/// `/DefaultRGB` a CalRGB, three gammas and a matrix; and `/DefaultCMYK` an
/// ICC printer profile, whose four-channel lookup table is integer
/// interpolation. It paints through them by the device operators, which ask
/// the resource seam only in a scope with a default, by a DeviceCMYK shading,
/// an `/Indexed` base and a sampled image, so the hash covers the operator
/// branch, the seam and the image decode at once.
fn default_spaces_page() -> Vec<u8> {
    let content = "q 0 0 120 30 re W n /Sh0 sh Q\n\
                   0.3 g 0 30 30 50 re f\n\
                   0.2 0.5 0.8 rg 30 30 30 50 re f\n\
                   0.1 0.6 0.3 0.2 k 60 30 30 50 re f\n\
                   /CS1 cs 1 sc 90 30 30 25 re f\n\
                   q 30 0 0 25 90 55 cm /Im0 Do Q";
    let samples = "3366991188DDCC4422FF8800>";
    let profile = default_cmyk_profile();
    let mut bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /ColorSpace <<\n\
       /DefaultGray [/CalGray << /WhitePoint [0.9505 1 1.089] /Gamma 2.2 >>]\n\
       /DefaultRGB [/CalRGB << /WhitePoint [0.9505 1 1.089] /Gamma [1.8 1.8 1.8]\n\
         /Matrix [0.4124 0.2126 0.0193 0.3576 0.7152 0.1192 0.1805 0.0722 0.9505] >>]\n\
       /DefaultCMYK [/ICCBased 6 0 R]\n\
       /CS1 [/Indexed /DeviceRGB 1 <4080C0C08040>] >>\n\
     /Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceCMYK /Coords [0 0 120 0]\n\
       /Function << /FunctionType 2 /Domain [0 1] /C0 [0.1 0.6 0.3 0.2]\n\
                    /C1 [0.5 0.1 0.2 0.3] /N 1 >> /Extend [true true] >> >>\n\
     /XObject << /Im0 5 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 1 /ColorSpace /DeviceRGB\n\
   /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{samples}\nendstream\nendobj\n\
6 0 obj\n<< /N 4 /Length {} >>\nstream\n",
        content.len(),
        samples.len(),
        profile.len()
    )
    .into_bytes();
    bytes.extend_from_slice(&profile);
    bytes.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");
    bytes
}

/// The `/DefaultCMYK` of [`default_spaces_page`]: a version 2 printer
/// profile whose one `mft2` table takes four inks to XYZ on a grid of two
/// points an axis, each corner the light of its inks squeezed into a tenth
/// to nine tenths, so no colour comes out as 8.6.4.4 converts it.
fn default_cmyk_profile() -> Vec<u8> {
    let s15 = |value: f64| (((value * 65536.0).round() as i32) as u32).to_be_bytes();
    let mut tag = b"mft2\0\0\0\0".to_vec();
    tag.extend_from_slice(&[4, 3, 2, 0]);
    for row in 0..3 {
        for column in 0..3 {
            tag.extend_from_slice(&s15(if row == column { 1.0 } else { 0.0 }));
        }
    }
    tag.extend_from_slice(&2u16.to_be_bytes());
    tag.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..4 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    // sRGB's primaries adapted to D50, the last ink (black) varying fastest.
    let primaries = [
        [0.436_065, 0.222_488, 0.013_916],
        [0.385_147, 0.716_873, 0.097_076],
        [0.143_066, 0.060_607, 0.714_096],
    ];
    for corner in 0..16u32 {
        let ink = |bit: u32| f64::from((corner >> bit) & 1);
        let (c, m, y, k) = (ink(3), ink(2), ink(1), ink(0));
        let light = [
            0.1 + 0.8 * (1.0 - c) * (1.0 - k),
            0.1 + 0.8 * (1.0 - m) * (1.0 - k),
            0.1 + 0.8 * (1.0 - y) * (1.0 - k) * 0.7,
        ];
        let xyz: [f64; 3] = std::array::from_fn(|axis| {
            primaries
                .iter()
                .zip(light)
                .map(|(primary, amount)| primary[axis] * amount)
                .sum()
        });
        for value in xyz {
            // 1.0 is 0x8000 in the connection space's sixteen-bit encoding.
            tag.extend_from_slice(&((value * 32768.0).round() as u16).to_be_bytes());
        }
    }
    for _ in 0..3 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    let offset = 128 + 4 + 12;
    let size = (offset + tag.len()).div_ceil(4) * 4;
    let mut profile = vec![0u8; 128];
    profile[0..4].copy_from_slice(&(size as u32).to_be_bytes());
    profile[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
    profile[12..16].copy_from_slice(b"prtr");
    profile[16..20].copy_from_slice(b"CMYK");
    profile[20..24].copy_from_slice(b"XYZ ");
    profile[36..40].copy_from_slice(b"acsp");
    profile.extend_from_slice(&1u32.to_be_bytes());
    profile.extend_from_slice(b"A2B0");
    profile.extend_from_slice(&(offset as u32).to_be_bytes());
    profile.extend_from_slice(&(tag.len() as u32).to_be_bytes());
    profile.extend_from_slice(&tag);
    profile.resize(size, 0);
    profile
}

/// ICC conversion (8.6.5.5): every model of profile the engine reads, on one
/// page, beside a device image as its control.
///
/// *Added with P-22.3*, so that reading a lookup-table transform through an
/// integer grid (P-22.4) moves a fingerprint where it moves pixels. Five
/// pieces, each covering its part of a 120 x 80 page whole:
///
/// - **an 8-bit CMYK image through a four-channel `mft2` profile over a Lab
///   connection space** ([`icc_cmyk_lab_profile`]), the one conversion here a
///   grid stands in for;
/// - **the same samples as `/DeviceCMYK`**, 8.6.4.4's arithmetic and no
///   profile: the control beside it;
/// - **an RGB image and a fill through one matrix/TRC profile**
///   ([`icc_rgb_profile`]), the profile object the two share, so a fill and an
///   image pixel go through one compiled transform;
/// - **a grey-profile image** ([`icc_grey_profile`]), one curve.
///
/// Each picture's samples are drawn ten by ten pixels at whole-pixel offsets,
/// magnified without `/Interpolate`, so every pixel is one sample's converted
/// colour and a move in the hash is a move in a conversion. No sample is
/// unprinted paper or white, so every pixel of the page is ink.
fn icc_page() -> Vec<u8> {
    let content = "q 60 0 0 40 0 40 cm /Im0 Do Q\n\
                   q 60 0 0 40 60 40 cm /Im1 Do Q\n\
                   q 40 0 0 40 0 0 cm /Im2 Do Q\n\
                   q 40 0 0 40 40 0 cm /Im3 Do Q\n\
                   /CS1 cs 0.2 0.55 0.8 scn 80 0 40 40 re f";
    // Six by four inks, each channel stepped through the byte range at its
    // own rate, so the picture holds light and rich colours, near-black and
    // nearly no ink; the black ink is full in one sample and absent in four.
    let ink: Vec<u8> = (0..24u32)
        .flat_map(|i| {
            [
                (i * 37 + 11) % 256,
                (i * 91 + 40) % 256,
                (i * 53 + 200) % 256,
                (i * 23) % 256,
            ]
        })
        .map(|v| v as u8)
        .collect();
    let rgb: Vec<u8> = (0..16u32)
        .flat_map(|i| [(i * 61) % 256, (i * 29 + 90) % 256, (i * 113 + 30) % 256])
        .map(|v| v as u8)
        .collect();
    let grey: Vec<u8> = (0..16u32).map(|i| (i * 15 + 5) as u8).collect();
    let (ink, rgb, grey) = (hex(&ink), hex(&rgb), hex(&grey));
    let image = |width: u32, height: u32, space: &str, samples: &str| {
        format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height}\n\
   /ColorSpace {space} /BitsPerComponent 8 /Filter /ASCIIHexDecode\n\
   /Length {} >>\nstream\n{samples}\nendstream",
            samples.len()
        )
    };
    let mut bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80]\n\
   /Resources << /ColorSpace << /CS1 [/ICCBased 8 0 R] >>\n\
     /XObject << /Im0 5 0 R /Im1 6 0 R /Im2 10 0 R /Im3 11 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n{}\nendobj\n\
6 0 obj\n{}\nendobj\n",
        content.len(),
        image(6, 4, "[/ICCBased 7 0 R]", &ink),
        image(6, 4, "/DeviceCMYK", &ink),
    )
    .into_bytes();
    for (number, components, profile) in [
        (7, 4, icc_cmyk_lab_profile()),
        (8, 3, icc_rgb_profile()),
        (9, 1, icc_grey_profile()),
    ] {
        bytes.extend_from_slice(
            format!(
                "{number} 0 obj\n<< /N {components} /Length {} >>\nstream\n",
                profile.len()
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(&profile);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");
    }
    bytes.extend_from_slice(
        format!(
            "10 0 obj\n{}\nendobj\n\
11 0 obj\n{}\nendobj\n\
trailer\n<< /Size 12 /Root 1 0 R >>\n%%EOF\n",
            image(4, 4, "[/ICCBased 8 0 R]", &rgb),
            image(4, 4, "[/ICCBased 9 0 R]", &grey),
        )
        .as_bytes(),
    );
    bytes
}

/// A version 2 ICC profile of `class`, data space `space` and connection space
/// `pcs`, carrying `tags` in the order given: the header and tag table ICC.1
/// 7.2 and 7.3 lay out, each tag's data padded to four bytes.
fn icc_profile_bytes(
    class: &[u8; 4],
    space: &[u8; 4],
    pcs: &[u8; 4],
    tags: &[([u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let mut profile = vec![0u8; 128];
    profile[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    profile[12..16].copy_from_slice(class);
    profile[16..20].copy_from_slice(space);
    profile[20..24].copy_from_slice(pcs);
    profile[36..40].copy_from_slice(b"acsp");
    profile.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    let mut offset = 128 + 4 + 12 * tags.len();
    let mut body = Vec::new();
    for (signature, data) in tags {
        profile.extend_from_slice(signature);
        profile.extend_from_slice(&(offset as u32).to_be_bytes());
        profile.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 128 + 4 + 12 * tags.len() + body.len();
    }
    profile.extend_from_slice(&body);
    let size = profile.len() as u32;
    profile[0..4].copy_from_slice(&size.to_be_bytes());
    profile
}

/// ICC.1's `s15Fixed16Number`, big-endian.
fn s15_16(value: f64) -> [u8; 4] {
    (((value * 65536.0).round() as i32) as u32).to_be_bytes()
}

/// An `XYZType` tag.
fn icc_xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut tag = b"XYZ \0\0\0\0".to_vec();
    for value in [x, y, z] {
        tag.extend_from_slice(&s15_16(value));
    }
    tag
}

/// A `curv` tag of one entry: a pure gamma, in u8Fixed8.
fn icc_gamma(gamma: f64) -> Vec<u8> {
    let mut tag = b"curv\0\0\0\0".to_vec();
    tag.extend_from_slice(&1u32.to_be_bytes());
    tag.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    tag
}

/// The CMYK image's profile in [`icc_page`]: a printer profile whose one
/// `mft2` table takes four inks to v2's legacy 16-bit Lab over a grid of five
/// points an axis, its input and output stages the identity.
///
/// Each node is a profile's numbers rather than a measured press: the light
/// each ink leaves, scaled by what the black ink leaves, then a lightness and
/// two opponent axes of it. Built from additions and products alone, which
/// IEEE 754 rounds alike on every target, so the profile is the same bytes
/// everywhere. Full ink of all four is lightness 1.5, near black, where sRGB's
/// encoding is steepest.
fn icc_cmyk_lab_profile() -> Vec<u8> {
    const GRID: usize = 5;
    let mut tag = b"mft2\0\0\0\0".to_vec();
    tag.extend_from_slice(&[4, 3, GRID as u8, 0]);
    // The matrix, which an `A2B*` tag never applies.
    for row in 0..3 {
        for column in 0..3 {
            tag.extend_from_slice(&s15_16(if row == column { 1.0 } else { 0.0 }));
        }
    }
    tag.extend_from_slice(&2u16.to_be_bytes());
    tag.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..4 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    // The grid, the last ink (black) varying fastest.
    for index in 0..GRID.pow(4) {
        let mut ink = [0.0f64; 4];
        let mut rest = index;
        for slot in ink.iter_mut().rev() {
            *slot = (rest % GRID) as f64 / (GRID - 1) as f64;
            rest /= GRID;
        }
        let [c, m, y, k] = ink;
        let black = 1.0 - 0.92 * k;
        let red = (1.0 - 0.85 * c) * black;
        let green = (1.0 - 0.8 * m) * black;
        let blue = (1.0 - 0.75 * y) * black;
        let l = 100.0 * (0.3 * red + 0.59 * green + 0.11 * blue);
        let a = 90.0 * (red - green);
        let b = 70.0 * (0.5 * (red + green) - blue);
        // L* over 0..100 and a*, b* plus 128 over 0..255, each to 0..0xFF00.
        let encode = |v: f64| (v.round().clamp(0.0, 65535.0) as u16).to_be_bytes();
        tag.extend_from_slice(&encode(l / 100.0 * 65280.0));
        tag.extend_from_slice(&encode((a + 128.0) / 255.0 * 65280.0));
        tag.extend_from_slice(&encode((b + 128.0) / 255.0 * 65280.0));
    }
    for _ in 0..3 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    icc_profile_bytes(
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[(*b"A2B0", tag), (*b"wtpt", icc_xyz(0.9642, 1.0, 0.8249))],
    )
}

/// The RGB image's and the fill's profile in [`icc_page`]: sRGB's primaries
/// adapted to D50 as `rXYZ`, `gXYZ` and `bXYZ`, under three gammas of 2.2.
fn icc_rgb_profile() -> Vec<u8> {
    icc_profile_bytes(
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (*b"rXYZ", icc_xyz(0.436_065, 0.222_488, 0.013_916)),
            (*b"gXYZ", icc_xyz(0.385_147, 0.716_873, 0.097_076)),
            (*b"bXYZ", icc_xyz(0.143_066, 0.060_607, 0.714_096)),
            (*b"rTRC", icc_gamma(2.2)),
            (*b"gTRC", icc_gamma(2.2)),
            (*b"bTRC", icc_gamma(2.2)),
            (*b"wtpt", icc_xyz(0.9642, 1.0, 0.8249)),
        ],
    )
}

/// The grey image's profile in [`icc_page`]: one `kTRC` of gamma 1.8 into XYZ.
fn icc_grey_profile() -> Vec<u8> {
    icc_profile_bytes(
        b"mntr",
        b"GRAY",
        b"XYZ ",
        &[
            (*b"kTRC", icc_gamma(1.8)),
            (*b"wtpt", icc_xyz(0.9642, 1.0, 0.8249)),
        ],
    )
}

/// The committed fingerprints.
///
/// Every entry is a claim that this page renders to these exact bytes on
/// every supported target. Changing one is a deliberate act; see the module
/// documentation for which of the two failures you are looking at.
///
/// # The book hash moved once, deliberately
///
/// `352adfa4…` became `ed6ba37c…` when EPUB output gained a **structure
/// tree**: `/MarkInfo`, `/StructTreeRoot`, a `/ParentTree`, `/StructParents`
/// on every page, and a `BDC`/`EMC` pair around every run. None of that is on
/// a rendered page — the raster fingerprints did not move and could not have —
/// which is exactly the class of change this hash exists to catch, and the
/// same reason milestone 5's annotations are named above it.
///
/// **Three hashes moved and no raster one did**, which is the shape that says
/// it was one change: the synthesised book and the 432x648 page box moved to
/// the *same* value, because they are the same bytes, and the 600x800 box to
/// its own. A structure tree that had touched the page content would have
/// moved a raster fingerprint too, and none of them shifted.
///
/// # Eight raster hashes moved once, deliberately, and eleven did not
///
/// *15 September 2026, with ruling 5's region row.* `fill` used to hold an
/// edge's slope as a whole number of 1/256 pixel per sub-scanline, which
/// truncated every slope below one pixel of x per sixteen of y to **zero** and
/// left every steeper edge walking `h/16` pixels off the line over an edge `h`
/// tall; and it truncated an edge's x toward zero rather than taking the
/// nearest unit. Both are fixed, `analytic_coverage.rs` adjudicates the fix
/// against the line's own equation rather than against a picture, and the
/// eight pages here that rasterise a path or a glyph moved.
///
/// The shape of the move is the evidence that it is the one change, so it is
/// recorded rather than described. Pixels differing, and the worst component
/// difference of 255, between the previous entry and this one:
///
/// | Page | Pixels | Of | Worst |
/// | --- | ---: | ---: | ---: |
/// | `curves` | 786 | 13 000 | 239 |
/// | `epub` | 22 042 | 279 936 | 109 |
/// | `text` | 814 | 20 000 | 74 |
/// | `tiling` | 1 621 | 9 600 | 182 |
/// | `mesh` | 305 | 9 600 | 53 |
/// | `pattern` | 103 | 9 600 | 20 |
/// | `optional` | 37 | 9 600 | 10 |
/// | `xps` | 154 | 484 704 | 24 |
///
/// *Re-measured on the committed tree before this entry was written, and the
/// eight rows all moved by a few pixels from the draft that first carried
/// them: 789, 21 612, 799, 1 601, 306, 101, 40, 149, and a worst of 183 for
/// `tiling`. The draft's numbers came from a working tree that still had a
/// clamp in `build_edges` that the committed one does not. A table nobody
/// re-runs is a dated measurement (ruling 13's own distinction), so the date
/// above is load-bearing and the numbers are the tree's, not a previous
/// tree's.*
///
/// The slope is almost all of it. Taking the nearest unit rather than the one
/// below accounts for the rest and never for more than **two** levels: on its
/// own it moves 139 pixels of `curves`, 8 825 of `epub`, 369 of `text`, 432 of
/// `tiling`, 85 of `mesh`, 26 of `pattern`, 14 of `optional` and 85 of `xps`.
///
/// **`shading`, `blend`, `image`, `transparency`, `jbig2`, `jpx`, `cbz` and the
/// four `analytic_` pages did not move at all**, which is the half that says it
/// was the scanline filler: those pages are sampled or composited per pixel and
/// never ask `fill` for a coverage. A change that had reached the compositor,
/// the colour path or a decoder could not have left them alone.
///
/// They moved a **second** time, in the same three places and again with no
/// raster movement, when the tree became document-level: an element's kids may
/// now name different pages, so a paragraph broken across a page break is one
/// `/P` rather than two and a kid on another page is written as an `/MCR`
/// dictionary. That is what closed the float reading-order row -- Beowulf
/// conserves exactly in logical order where content order still shows 2 182 --
/// and it changes which objects the file holds without changing one glyph.
///
/// *October 2026, the tagged-writing row.* They moved a **third** time, in the
/// same three places and again with no raster movement, when the EPUB path
/// began writing the book's `dc:language` as the catalog's `/Lang` (14.9.2):
/// `dcd5912d…` became `1517adfb…` and `51748067…` became `36a2c5e1…`. That one
/// entry is the whole of the move, and it was measured rather than argued:
/// with only the `set_language` call removed, the three hashes are the old
/// ones exactly. The book's chapters declare `lang="en"`, which is the book's
/// own language, so no element carries a `/Lang` of its own, and the book's
/// only picture is an SVG cover this build refuses, so no `/Figure` is drawn.
///
/// And a **fourth** time, the same way, when each `<a>` holding an annotation
/// became a `/Link` element with an `/OBJR` to it and the annotation a
/// `/StructParent`: `1517adfb…` became `e0f94b04…` and `36a2c5e1…` became
/// `df84fe61…`. The book's eight annotations are unchanged — counted as
/// `/Subtype /Link`, since the bare name also counts the seven `/S /Link`
/// elements that hold them and their marked content — and no raster hash
/// moved.
///
/// And a **fifth**, when the EPUB path began writing an element's XHTML name
/// where its standard type would lose it (`/S /section`, `/S /em`), with a
/// `/RoleMap` saying what each is: `e0f94b04…` became `bcc9bb59…` and
/// `df84fe61…` became `3b288a03…`. Measured as the third was: with only the
/// role registration disabled, the hashes are the fourth move's exactly.
///
/// The CSS and shaping work merged beside it in the same wave — paint
/// effects, counters, `text-transform`, right-to-left lines, CID-keyed
/// fallback text — moved neither hash: the merged tree produces the fifth
/// move's two values exactly, because this book uses none of those properties
/// and none of its text is right-to-left. (Its non-breaking hyphen and its
/// Japanese line are outside WinAnsi, and are drawn in a fallback font, but
/// the book sets no `word-spacing` and justifies nothing, so writing the
/// overflow font at `0 Tw` moved neither hash either: a word spacing of zero
/// is written as it always was.) The new values
/// were produced on `x86_64-unknown-linux-gnu` and reproduced under
/// `wasm32-wasip1` before they were committed.
const GOLDEN: &[Fixture] = &[
    // The floors are about half of what each page paints today: 1486, 2363,
    // 9600, 3600 and 3230 pixels.
    Fixture {
        name: "text",
        build: text_page,
        open: Open::Closed,
        least_ink: 700,
    },
    // The `text` page, its font selected by graphics states: the same ink.
    Fixture {
        name: "gs_text",
        build: gs_text_page,
        open: Open::Closed,
        least_ink: 700,
    },
    Fixture {
        name: "curves",
        build: curves_page,
        open: Open::Closed,
        least_ink: 1100,
    },
    Fixture {
        name: "shading",
        build: shading_page,
        open: Open::Closed,
        least_ink: 4500,
    },
    Fixture {
        name: "blend",
        build: blend_page,
        open: Open::Closed,
        least_ink: 1700,
    },
    Fixture {
        name: "pattern",
        build: pattern_page,
        open: Open::Closed,
        least_ink: 1600,
    },
    // 4922 today, and most of it is inside layers that are *on*, so this
    // floor is the guard against over-suppression: hiding the two visible
    // layers drops the page to about 1300. Under-suppression is the opposite
    // failure and adds ink — a flat red 9600-pixel page — which no floor can
    // catch and the hash catches immediately.
    Fixture {
        name: "optional",
        build: optional_content_page,
        open: Open::Closed,
        least_ink: 2400,
    },
    // 5262 today, out of 19 200. The five placements are disjoint, so this
    // floor is also a guard against one of them silently drawing nothing:
    // losing the rotated placement or either of the two magnified ones takes
    // the page below it.
    Fixture {
        name: "image",
        build: image_page,
        open: Open::Closed,
        least_ink: 2600,
    },
    // 8066 today, of 9600. The floor guards over-suppression: a soft mask
    // that hides everything, or a group that composites nothing back, takes
    // the masked band's 4600 pixels off this page and lands under it. The
    // opposite failure *adds* ink -- a `/BC` defaulted to white uncovers the
    // twenty columns outside the mask group's box -- which no floor can catch
    // and the hash catches at once (the same asymmetry gap 06 recorded).
    Fixture {
        name: "transparency",
        build: transparency_page,
        open: Open::Closed,
        least_ink: 4000,
    },
    // 4488 today, of 9600, and most of it is the wedge's lattice. The floor
    // is the guard against the fill's cells failing to repeat: one row and
    // one column of them is already under it. It does *not* reach the
    // uncoloured stroke, which is only about six hundred pixels — that half
    // is the hash's, and so is the opposite failure of a cell spilling past
    // its box, which adds ink rather than losing it.
    Fixture {
        name: "tiling",
        build: tiling_page,
        open: Open::Closed,
        least_ink: 2200,
    },
    // The frame is thin, so this page paints little relative to its area and
    // the floor sits close to what it draws. That is the point: a JBIG2
    // decode that lost typical prediction, or a refusal that started firing,
    // returns *no* image at all and this drops to zero. The opposite failure
    // — the polarity inverted — fills almost the whole page instead, and the
    // hash catches that at once.
    Fixture {
        name: "jbig2",
        build: jbig2_page,
        open: Open::Closed,
        least_ink: 900,
    },
    // 9 311 today, of 9 600: the three `sh` bands cover the page between them
    // and only the bowed edges of the Coons patch leave any white. The floor
    // guards the failure a hash cannot describe -- one of the four types
    // silently painting nothing, which takes a whole third of the page out and
    // lands well under it. The opposite failure, a mesh spilling past the clip
    // its `sh` was given, *adds* ink and is the hash's to catch.
    Fixture {
        name: "mesh",
        build: mesh_page,
        open: Open::Closed,
        least_ink: 4600,
    },
    // 5 180 today, of 9 600: three disjoint 1 728-pixel rectangles, less the
    // four corner pixels of the gradient that come out pure white.
    //
    // **This floor is weaker than the others here and it is worth saying why
    // rather than letting it look like the same guard.** Both of this codec's
    // failure modes paint. A refusal draws ruling 2's placeholder, which is
    // grey; a wrong decode draws a plausible photograph, which is the whole
    // hazard JPEG 2000 carries and has just as much ink in it as a right one.
    // So no ink count can see either, and this fixture leans on its hash
    // almost entirely -- which is the reason it exists.
    //
    // What the floor does catch is coarser and still worth having: one of the
    // three rectangles not being painted at all, which is where a resource or
    // a filter-chain change lands rather than a decoder one. Losing any one
    // takes the page to 3 452.
    Fixture {
        name: "jpx",
        build: jpx_page,
        open: Open::Closed,
        least_ink: 3600,
    },
    // 1 403 today, of 1 600: the indexed picture covers the whole page and the
    // 197 pixels on the two transparent palette indices are what leaves any
    // white at all.
    //
    // This floor is doing two jobs rather than one. It guards the ordinary
    // failure -- a page that stops drawing -- and it guards the one this
    // fixture is uniquely exposed to: **a page that became a placeholder**.
    // Every route into this document can fail into ruling 2's neutral grey,
    // which is 0xBF on all three channels and therefore counts as ink on every
    // pixel of the page. So the floor cannot see that one, and the direction
    // that *can* is the other way: 8.9.6.4's `/Mask` ignored paints the two
    // transparent indices as their palette colours and takes this page to a
    // full 1 600. The hash catches that, as it catches the grey.
    Fixture {
        name: "cbz",
        build: cbz_page,
        open: Open::Closed,
        least_ink: 700,
    },
    // 20 333 today, of 484 704: a 612 x 792 page, most of it white.
    //
    // The floor is doing three jobs here rather than the usual one, because
    // this page has three independent halves and a hash cannot say which of
    // them stopped.
    //
    // The picture the `ImageBrush` paints is the bulk of the ink. A pattern
    // whose matrix is wrong puts it off the sheet entirely — which is
    // milestone 8's own defect, and the page rendered *plausibly* with it,
    // because the text was still there. That failure is the one this floor is
    // set to catch: losing the picture takes the page under it.
    //
    // The glyph run is small in pixels and cannot be floored on its own, so
    // it is guarded the other way, by `fingerprint`'s `UnreadableFont` check:
    // the ODTTF key arithmetic failing does not paint grey, it paints nothing
    // and warns.
    //
    // And a page that became ruling 2's placeholder is grey rather than white,
    // so it counts as ink on every one of the 484 704 pixels and no floor can
    // see it. That direction is the hash's, as it is for `cbz`.
    Fixture {
        name: "xps",
        build: xps_package,
        open: Open::Closed,
        least_ink: 10_000,
    },
    // 44 483 today, of 279 936: page **3** of seven, which is the first page of
    // chapter one and the densest page of running text the book has. Pages 0
    // to 2 are a cover this build refuses as SVG, a title page and a navigation
    // document, and none of them would notice if line breaking stopped working.
    //
    // The floor is doing a job no other floor here does. A reflowable book has
    // two independent ways to come out empty and a hash cannot tell them
    // apart: **the text may fail to lay out**, which leaves the page white, and
    // **the face may fail to arrive**, which leaves it white as well. The
    // second is guarded by `fingerprint`'s `UnreadableFont` check and by
    // [`book_face`]; this floor guards the first, and it guards it in the
    // direction a hash cannot — losing the paragraphs after the heading takes
    // this page to about 3 000.
    //
    // The opposite failure adds ink: a page box ignored sets the whole chapter
    // on one page, and the hash catches that at once.
    Fixture {
        name: "epub",
        build: epub_book,
        open: Open::Book {
            page: (432.0, 648.0),
            at: 3,
        },
        least_ink: 22_000,
    },
    // ---- the analytic pages (milestone 2 of design/render-verification.md) --
    //
    // `render_analytic.rs` holds each of these four to the equation ISO 32000
    // publishes for it, per pixel, over the whole page. What it cannot say is
    // that the same page comes out the same on wasm32 and on x86_64 — that is
    // this file's claim, and these are the fixtures where the two tiers meet:
    // the bytes hashed here are the bytes that file evaluates, through
    // `render_support`, rather than a second page built to look like them.
    Fixture {
        name: "analytic_axial",
        build: axial_page,
        open: Open::Closed,
        least_ink: 128,
    },
    Fixture {
        name: "analytic_radial",
        build: radial_page,
        open: Open::Closed,
        least_ink: 100,
    },
    Fixture {
        name: "analytic_blend",
        build: blend_grid_page,
        open: Open::Closed,
        least_ink: 800,
    },
    Fixture {
        name: "analytic_image",
        build: analytic_image_page,
        open: Open::Closed,
        least_ink: 64,
    },
    // The page paints every one of its 9 600 pixels; the floor is half.
    Fixture {
        name: "default_spaces",
        build: default_spaces_page,
        open: Open::Closed,
        least_ink: 4800,
    },
    // The stroker's whole pen: every cap and join, a miter past its limit,
    // dashes at two phases along a curve, and an even-odd star. The page
    // paints about 3 000 of its 11 921 pixels; the floor is half.
    Fixture {
        name: "strokes",
        build: pen_page,
        open: Open::Closed,
        least_ink: 1500,
    },
    // 5 759 today, of 15 345: one raster of one-bit samples five ways, each at
    // four sizes (`render_support::bilevel_page`), and the floor is about
    // half. Every row reaches the sampler through the one-bit sample loop, so
    // a loop that stops drawing takes the page under it. A row drawn as its
    // own negative keeps its ink, and a refusal paints a grey placeholder
    // that counts as ink, so both of those are the hash's to catch.
    Fixture {
        name: "bilevel",
        build: bilevel_page,
        open: Open::Closed,
        least_ink: 2_800,
    },
    // Every pixel of the 9 600 is a converted sample or the fill, none of
    // them white, and the floor is about half: a page that loses its two
    // pictures of inks, or all three 40 x 40 pieces under them, lands under
    // it. A picture converted as the wrong space keeps its ink, so that is
    // the hash's.
    Fixture {
        name: "icc",
        build: icc_page,
        open: Open::Closed,
        least_ink: 5_000,
    },
    // A baseline and a progressive 4:2:0 JPEG, both odd-sized, each drawn at
    // exactly a half, a quarter and an eighth of its size — one flipped, one
    // quarter-turned, one clipped (`render_support::jpeg_minified_page`) — so
    // each is decoded at the scale it is drawn at. The six draws cover about
    // 4 300 of the page's 18 683 pixels; the floor is under half of that, and
    // a draw that stops decoding takes the page under it.
    Fixture {
        name: "jpeg_minified",
        build: jpeg_minified_page,
        open: Open::Closed,
        least_ink: 2_000,
    },
    // Six JPEG 2000 codestreams of both wavelets, two of them odd-sized and
    // one of subsampled components, each drawn at the sizes that drop one
    // level and two — one flipped, one quarter-turned, one sheared, two
    // clipped, one past the levels it has
    // (`render_support::jpx_minified_page`) — so each is decoded with the
    // levels its size drops. The ten draws cover about 1 200 of the page's
    // 7 680 pixels; the floor is under half of that, and a draw that stops
    // decoding takes the page under it.
    Fixture {
        name: "jpx_minified",
        build: jpx_minified_page,
        open: Open::Closed,
        least_ink: 550,
    },
    // Each rendering intent's transform (8.6.5.8): in each of four columns,
    // under one of Table 69's intents by `ri`, a fill, a 2 x 2 image and an
    // axial shading `sh` paints, all in an `ICCBased` space over a profile
    // carrying a table for each intent on a warm paper
    // (`render_support::intents_page`). Every one of the page's 9 600 pixels
    // is painted and the floor is half: a column that stops painting takes
    // the page under it, and one painted in the wrong intent keeps its ink,
    // which is the hash's to catch.
    Fixture {
        name: "intents",
        build: intents_page,
        open: Open::Closed,
        least_ink: 4_800,
    },
];

/// The committed render fingerprint of every page in [`GOLDEN`], by name.
///
/// `rendering_is_stable_across_targets` holds each page's render to it, and
/// `banded_rendering_reproduces_every_fingerprint` holds the same page drawn
/// in four bands to the same value.
const FINGERPRINTS: &[(&str, &str)] = &[
    // Moved twice in August 2026. First because the old value was the hash
    // of a blank page, the fixture having named a standard-14 font and
    // embedded no outlines for the engine to draw.
    //
    // Then with gap 13, which made the quadratic a path verb of its own:
    // four of this face's six shapes carry off-curve points, and every one
    // of them used to be raised to a cubic before it was flattened. The
    // curve is the same curve either way, so the move is small and it is
    // arithmetic only — 7 of 20 000 pixels, each by exactly one level of
    // 255 on all three channels, with the ink count and the ink bounding
    // box unchanged. wasm32 and x86_64 agreed on the new value, which is
    // what says this is a rendering change and not a determinism bug.
    (
        "text",
        "82510bb48a364a4bc92729cfcdcd1e14aa99fe817a323a78e48cfb51f76a181d",
    ),
    // Moved September 2026, deliberately, and not because two targets
    // disagreed. The fixture's red triangle is `20 20 m 60 75 l 100 25 l
    // f` -- an **open** subpath, filled -- and until then the filler built
    // only the two edges the path states, so the third side of the
    // triangle was missing and the old hash had that defect baked in.
    // ISO 32000-1 8.5.3.1 closes every open subpath before a fill (8.5.4
    // before a clip); `an_open_subpath_is_closed_before_it_is_filled_or_clipped`
    // in `tinker-pdf-render` holds the new behaviour by half-plane
    // arithmetic, and the new value agreed on x86_64 and wasm32-wasip1.
    // No other fingerprint here moved, and instrumenting the filler over
    // all nineteen pages -- tiling cells included -- found this triangle
    // the only open subpath any of them fills or clips.
    (
        "curves",
        "414e3f05ee05718fc243993506458dcb0f8aca243cdff7bf21bd059a6f1e9f55",
    ),
    (
        "shading",
        "813a28f7b119418e76ae52f96f69047b5dec5100a26375294e9de41ed9cc90b5",
    ),
    (
        "blend",
        "759840c7df7bad4fc49a2d94f763e8b5eca6d9edb64f3af1cdfcd635b2512258",
    ),
    // Added August 2026 with gap 07; no existing fixture reached
    // `fill_with_pattern` at all.
    (
        "pattern",
        "e02ff91587f4a82c2273f7980e42d953878eead57026dd863de757241217cd5c",
    ),
    // Added August 2026 with gap 06. No existing fixture has an
    // `/OCProperties`, so none of them would move if 8.11 stopped working.
    (
        "optional",
        "e2e801543a390458985e5f42aa3e7526ef8af413e24234dfbd0252904e65a6a8",
    ),
    // Added August 2026 with gap 12. No existing fixture drew an image,
    // so the whole of image sampling — every row of the policy matrix,
    // the bilinear weights and the pyramid — was outside what this file
    // measured.
    //
    // Re-recorded August 2026 when minification stopped interpolating and
    // started averaging: 0.77 % of this page's pixels moved, mean 0.001,
    // worst 0.24, all of it the one minified placement. The new value is
    // the *correct* one and the old was not, which this file could never
    // have said on its own — a fingerprint pins the engine against itself,
    // so it reports that an answer changed and never that it was wrong.
    // `tinker-pdf-raster/tests/analytic_sampling.rs` is what says which.
    //
    // Re-recorded again when the box-filter pyramid stopped engaging below
    // 128:1 and the destination pixel's true source rectangle began to be
    // integrated instead. `pdfcmp` between the two renders of this page:
    // **0.0833 % of pixels differ at all, worst 1 level of 255, none by
    // more than twelve** — the minified placement again, and only it. The
    // same caveat as above applies and the same file answers it, now with
    // `a_downscale_agrees_with_itself_at_twice_the_scale`, which fails by
    // eight levels if the pyramid is put back.
    (
        "image",
        "33c71f2d05f951f3bf604eab03d9968c6f7245a8f8f86209410ded99670574db",
    ),
    // Added August 2026 with gap 11. Groups, isolation, knockout and an
    // ExtGState soft mask reach no other fixture here at all.
    (
        "transparency",
        "c120574918fcfadb0b33f3f9faa4f0c10a10cc760cd9e9830bedf31463e3f059",
    ),
    // Added August 2026 with gap 09. The `pattern` fixture above is a
    // shading pattern, evaluated per pixel; nothing here reached a
    // rasterised cell, a lattice, or `PaintType 2`.
    (
        "tiling",
        "274c17e359636faacafe251c7f71ca3b48d85f98a7b2251642048ea8897b5bd6",
    ),
    // Added August 2026 with gap 17. Nothing above decodes a JBIG2
    // stream, and the MQ arithmetic coder underneath it is shared with
    // whatever gap 18 decides about JPEG 2000 -- so a change made for one
    // codec that shifted a row of the Qe table would, until now, have had
    // only the filter crate's own tests standing in front of it.
    (
        "jbig2",
        "2b430170b06618741ae1c6051f3fce687f716bc2655488a8cdf0956d173127c7",
    ),
    // Added August 2026 with gap 10. Nothing above draws a mesh, so the
    // whole of 8.7.4.5.5 to 8.7.4.5.8 -- the packed vertex stream, the
    // edge-flag continuation, patch subdivision and the Gouraud
    // rasteriser -- was outside what this file measured. The patch
    // subdivision is the part that most needs a cross-target claim: it
    // picks a *count* from a device-space measure, and a count that comes
    // out one different on another target is a different mesh rather than
    // a rounding difference.
    (
        "mesh",
        "7faa166696c1283b6fb7556185d6e2f2a73bc49365a19bcf6797ad60e47c8c27",
    ),
    // Added August 2026 with gap 18a. Nothing above decodes a JPEG 2000
    // stream, so the whole of T.800 -- the container, tier-2's packets,
    // tier-1's contexts over the MQ coder, dequantisation, both inverse
    // wavelets and Annex G's two colour transforms -- was outside what
    // this file measured. The 9/7 and the ICT are the reason it needs to
    // be: both are specified as decimal lifting constants and both are
    // computed here in `i32` planes at Q12 with `i64` constants at Q24,
    // a decision the plan made before any decoder code existed and
    // justified entirely by this property. A float on that path would
    // show up here and nowhere else.
    (
        "jpx",
        "4da27b74f85dbec9e9891499c2d082009cec5e838efeefc371d77e1bf0659de8",
    ),
    // Added August 2026 with gap 29 milestone 6, and the first entry here
    // whose document is *synthesised* rather than parsed -- so it is the
    // first that pins a producer and a consumer to each other rather than
    // only a renderer. Read `cbz_page`'s doc comment before treating a
    // move in this one as a rendering change: its blast radius is the ZIP
    // reader, the natural sort, the PNG routing table, the pass-through's
    // `/DecodeParms`, `DocumentBuilder` and `CosDocument`'s parse of what
    // it wrote, as well as the renderer every other row here covers.
    (
        "cbz",
        "9e92c73984cff79feef04dcc984c52f04beda91bf3087a50ce9c17b3fc275aea",
    ),
    // Added August 2026 with gap 30 milestone 9, and **the first entry here
    // whose input this repository did not author**: Microsoft's own XPS
    // serialiser wrote the package, and every other fixture above is bytes
    // assembled a few lines from its own entry. Read `xps_package`'s doc
    // comment before treating a move in this one as a rendering change --
    // its blast radius is the ZIP reader, the XML parser, the OPC layer,
    // the spine, the markup walk, the brush grammar, the ODTTF key
    // arithmetic, `DocumentBuilder` and `CosDocument`'s parse of what it
    // wrote, as well as the renderer every other row here covers.
    (
        "xps",
        "5dd016599f8b550f523f7faa189b0d2c9edfcaf35860ac286caec4d71da56b11",
    ),
    // Added August 2026 with gap 31 milestone 13, and **the first entry
    // here whose page number is not a property of the file**: this is page
    // 3 of the seven a 432 x 648 box paginates
    // `pandoc-book-cover.epub` into, and a caller who passed 600 x 800
    // would get six pages and a different page 3. Read `epub_book`'s doc
    // comment before treating a move in this one as a rendering change --
    // its blast radius is the ZIP reader, the XML parser with milestone 2's
    // doctype mode, the OCF layer, the package document, the cascade, the
    // box tree, UAX #14's line breaker, fragmentation,
    // `DocumentBuilder`'s links and outline, and `CosDocument`'s parse of
    // what it wrote, as well as the renderer every other row here covers.
    (
        "epub",
        "a0cc8fd3b519745300b0b3eba14cfc5c2fb516b6af9c692801b8031f5585fbd6",
    ),
    // Enrolled with the analytic tier's own pages (milestone 2). Their
    // right answer is an equation `render_analytic.rs` evaluates per pixel;
    // what a hash adds is that the equation comes out the same on every
    // target.
    (
        "analytic_axial",
        "487f59336860e7edeb1854aa566de4a5964f14d2ba67c464b990dee393e1f8a1",
    ),
    (
        "analytic_radial",
        "8966418e8e900c6aca4de8682702e615d5e68ea066cb3bc2c25352a535943204",
    ),
    (
        "analytic_blend",
        "e1d056a15ae8cb7f18fd1524dfcc419893f4a5283c2f56b3f7307380aee43560",
    ),
    (
        "analytic_image",
        "37360b0c61ca919d525b75922112580518965e5f4ad12d52ce75953f46061872",
    ),
    // Added October 2026 with K-02.2: the one page here that names a
    // default colour space (8.6.5.6), each of the three through its own
    // arithmetic — CalGray's `pow`, CalRGB's matrix and an ICC lookup
    // table. No other entry moved when it was added.
    (
        "default_spaces",
        "cfe787612cba9228dd2a089986d409a34f599f2000ea708f96b6278036967374",
    ),
    // The `text` page with its font selected by graphics states (8.4.5's
    // `/Font`): the same face, lines and pixels, so the `text` page's
    // fingerprint.
    (
        "gs_text",
        "82510bb48a364a4bc92729cfcdcd1e14aa99fe817a323a78e48cfb51f76a181d",
    ),
    // The pen: no other page here dashes a stroke or draws a miter, a bevel or
    // a square cap, so this is the one that pins them.
    (
        "strokes",
        "2fb79647b749afcb45cb9bd4aa78d84101b88ab4fb1bacfd98351a185b0e9771",
    ),
    // One-bit images: DeviceGray plain and CCITT G4, under `/Decode [1 0]`, a
    // two-colour `/Indexed` palette and an `/ImageMask` in a fill colour, each
    // magnified, at 1:1 and minified 2:1 and 3:1 at fractional offsets. Every
    // bilevel path through the sample loops and the sampler draws on it, and
    // `render_regions.rs` tiles it.
    (
        "bilevel",
        "79a059cfb544d0b0a1b64c6aedafe7e70a64a821372a6fd245eae49327676f60",
    ),
    // ICC conversion through every model of profile the engine reads: a CMYK
    // image through a four-channel `mft2` into Lab, beside the same samples as
    // `/DeviceCMYK`, an RGB image and a fill through one matrix/TRC profile
    // and a grey-profile image. Its value is the pipeline's that converted
    // every colour per pixel, taken at the commit that added the page, so that
    // reading the lookup table through a grid (P-22.4) shows as a move of the
    // CMYK picture's pixels and of nothing else; `default_spaces`, whose
    // `/DefaultCMYK` is an `mft2` table too, is the other page it may move.
    ("icc", ""),
    // The reduced JPEG decodes: every scale a draw asks for — a half, a
    // quarter and an eighth — over a baseline and a progressive 4:2:0 frame
    // whose last block column and row are partly past the image, and the
    // reduced grid placed flipped, turned and clipped.
    ("jpeg_minified", ""),
    // The reduced JPEG 2000 decodes: one level dropped and two, of the 5/3
    // and the 9/7, over images whose reduced grid's last sample stands for
    // less than a whole one and over subsampled components, placed flipped,
    // turned, sheared and clipped, and a draw whose size would drop a level
    // its codestream does not have.
    ("jpx_minified", ""),
    // A fill, an image and an axial shading through one table profile under
    // each of the four intents: each intent's table, and absolute
    // colorimetric's scaling by the media white, on every target.
    ("intents", ""),
];

#[test]
fn rendering_is_stable_across_targets() {
    // A mismatch prints the replacement lines ready to paste. The length
    // check below is there because an empty table would make this test pass
    // by not looking at anything.
    assert_eq!(
        FINGERPRINTS.len(),
        GOLDEN.len(),
        "every page has a fingerprint, or the test passes by not looking"
    );

    let mut wrong = Vec::new();
    for fixture in GOLDEN {
        let actual = fingerprint(fixture);
        let want = FINGERPRINTS
            .iter()
            .find(|(n, _)| *n == fixture.name)
            .map(|(_, h)| *h)
            .unwrap_or_default();
        if actual != want {
            let name = fixture.name;
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

/// The four horizontal bands a page `height` rows tall is cut into, each as
/// its first row and its number of rows: band `i` starts at row
/// `height * i / 4`, so the bands are as even as the rows allow and none is
/// empty on any page here.
fn bands(height: u32) -> [(u32, u32); 4] {
    let edge = |i: u32| u32::try_from(u64::from(height) * u64::from(i) / 4).expect("within height");
    [0, 1, 2, 3].map(|i| (edge(i), edge(i + 1) - edge(i)))
}

/// One fingerprinted page rendered as [`bands`] through
/// [`RenderOptions::region`], the bands stacked back into one bitmap, and that
/// bitmap hashed as [`fingerprint`] hashes a page.
fn banded_fingerprint(fixture: &Fixture) -> String {
    let (document, at) = opened(fixture);
    let page = document.page(at).expect("a page");
    let options = RenderOptions::default();
    let (width, height) = page.pixel_size(&options);

    let mut data = Vec::new();
    let mut warnings: Vec<RenderWarning> = Vec::new();
    let mut shape = None;
    for (top, rows) in bands(height) {
        let band = page.render(&RenderOptions {
            region: Some(PixelRegion::new(0, top, width, rows)),
            ..options.clone()
        });
        assert_eq!(
            (band.width, band.height),
            (width, rows),
            "{}: the band of {rows} rows at row {top} came back the wrong size",
            fixture.name,
        );
        let this = (band.format, band.premultiplied);
        assert_eq!(
            *shape.get_or_insert(this),
            this,
            "{}: the band at row {top} changed the pixel format",
            fixture.name,
        );
        let row = band.width as usize * band.components();
        for y in 0..band.height as usize {
            let start = y * band.stride;
            data.extend_from_slice(band.data.get(start..start + row).expect("a whole row"));
        }
        for warning in band.warnings {
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
    }
    let (format, premultiplied) = shape.expect("four bands");
    let stride = data.len() / height as usize;
    hashed(
        fixture,
        &Bitmap {
            width,
            height,
            format,
            stride,
            data,
            warnings,
            premultiplied,
            stats: None,
        },
    )
}

/// **A page drawn in four bands is the page**: every fingerprinted page
/// rendered as four horizontal bands through [`RenderOptions::region`], the
/// bands stacked in order, hashes to the page's committed fingerprint.
///
/// Ruling 5's guard, `render_regions.rs`, holds tiles to the page they come
/// from on the targets that run it; this holds the bands to the committed
/// value itself, so the same pixels come out on every target the roster's
/// tests run on, `wasm32-wasip1` among them, whichever way the page is cut.
/// It adds no hash: a band that disagrees with [`FINGERPRINTS`] is a finding
/// about the region path or about the target, never a new value.
#[test]
fn banded_rendering_reproduces_every_fingerprint() {
    let mut wrong = Vec::new();
    for fixture in GOLDEN {
        let actual = banded_fingerprint(fixture);
        let want = FINGERPRINTS
            .iter()
            .find(|(n, _)| *n == fixture.name)
            .map(|(_, h)| *h)
            .expect("every page has a fingerprint");
        if actual != want {
            wrong.push(format!(
                "{}: banded {actual}, committed {want}",
                fixture.name
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "a page drawn in four bands is not the page its fingerprint pins. \
         Ruling 5 makes a band byte-equal to its rows of the whole render, so \
         this is a finding about the region path, or, where only one target \
         disagrees, a determinism bug -- never a reason to edit the table:\n{}",
        wrong.join("\n")
    );
}

/// **The page walk and the bands are the fingerprints** (K-07's threaded
/// path, P-15): every fingerprinted page drawn on 2, 3, 4 and 7 threads —
/// named that many times in one `Document::render_pages` walk, so that many
/// workers draw it at once, and drawn in that many bands through
/// `DisplayList::render` and through `Page::render` — hashes to the page's
/// committed fingerprint, as [`fingerprint`] hashes a page.
///
/// It adds no hash: a walked or banded page that disagrees with
/// [`FINGERPRINTS`] is a finding about the pool, the bands or their merge,
/// or, where only one target disagrees, a determinism bug — never a reason to
/// edit the table. `banded_rendering_reproduces_every_fingerprint` is the
/// serial half, a caller's regions one after another; this is the threaded
/// one. On `wasm32-wasip1` no thread starts, and the walk and the bands are
/// cut, drawn and stitched on the caller, which must come to the same bytes.
#[test]
fn parallel_and_banded_renders_are_the_fingerprints() {
    use tinker_pdf::Parallelism;

    let mut wrong = Vec::new();
    let mut held = 0usize;
    for fixture in GOLDEN {
        let want = FINGERPRINTS
            .iter()
            .find(|(n, _)| *n == fixture.name)
            .map(|(_, h)| *h)
            .expect("every page has a fingerprint");
        let (document, at) = opened(fixture);
        let page = document.page(at).expect("a page");
        let list = page.display_list();
        for threads in [2usize, 3, 4, 7] {
            let options = RenderOptions {
                parallelism: Parallelism::Threads(threads),
                ..RenderOptions::default()
            };
            let mut walked = Vec::new();
            document.render_pages(vec![at; threads], &options, |index, bitmap| {
                walked.push((index, bitmap));
            });
            assert_eq!(
                walked.len(),
                threads,
                "{}: every page of the walk",
                fixture.name
            );
            for (place, (index, bitmap)) in walked.iter().enumerate() {
                assert_eq!(*index, at, "{}: the page named", fixture.name);
                let bitmap = bitmap.as_ref().expect("the page renders");
                let actual = hashed(fixture, bitmap);
                if actual != want {
                    wrong.push(format!(
                        "{}: walked on {threads} threads, the walk's page {place}, {actual}",
                        fixture.name
                    ));
                }
                held += 1;
            }
            for (door, bitmap) in [
                ("DisplayList::render", list.render(&options)),
                ("Page::render", page.render(&options)),
            ] {
                let actual = hashed(fixture, &bitmap);
                if actual != want {
                    wrong.push(format!(
                        "{}: {door} in {threads} bands, {actual}",
                        fixture.name
                    ));
                }
                held += 1;
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a page drawn on threads is not the page its fingerprint pins. The walk \
         renders each page serially on its worker and the bands are the page's \
         own regions in its one frame (ruling 5), so this is a finding about the \
         pool, the bands or their merge, or, where only one target disagrees, a \
         determinism bug -- never a reason to edit the table:\n{}",
        wrong.join("\n")
    );
    println!(
        "determinism-threads: RAN {held} walked and banded renders of {} fingerprinted \
         pages, each its committed fingerprint",
        GOLDEN.len()
    );
}

/// One fingerprinted page written as SVG with the default [`SvgOptions`], its
/// markup hashed.
///
/// The page is the one [`fingerprint`] renders, held to the same two guards
/// before it is hashed: it draws something, and nothing it draws is missing
/// for want of a face. An SVG that holds no drawing element is this format's
/// blank page, and as stable on every target as a blank render is. A third
/// guard is the format's own: a document cut off at its budget is a hash of
/// part of the page.
fn svg_fingerprint(fixture: &Fixture) -> String {
    let (document, at) = opened(fixture);
    let svg = document
        .page(at)
        .expect("a page")
        .to_svg(&SvgOptions::default());
    let drawn: usize = ["<path", "<image", "<use"]
        .iter()
        .map(|element| svg.markup.matches(element).count())
        .sum();
    assert!(
        drawn > 0,
        "the {} fixture's SVG holds no drawing element, so its hash is not \
         evidence about anything. Warnings: {:?}",
        fixture.name,
        svg.warnings,
    );
    assert!(
        !svg.warnings
            .contains(&SvgWarning::Render(RenderWarning::UnreadableFont)),
        "the {} fixture's SVG names a font this build cannot draw, so its \
         glyphs are missing from the hash",
        fixture.name,
    );
    assert!(
        !svg.warnings
            .iter()
            .any(|warning| matches!(warning, SvgWarning::Truncated { .. })),
        "the {} fixture's SVG reached its budget, so the hash is of part of \
         the page",
        fixture.name,
    );
    sha(svg.markup.as_bytes())
}

/// **`Page::to_svg` writes the same bytes on every target**: every page the
/// render fingerprints cover, written as SVG with the default [`SvgOptions`],
/// hashes to the value committed here.
///
/// What the markup is made of is arithmetic a render does not show: every
/// coordinate rounded to four places through `f64::round` and printed at its
/// shortest, each stroke's pen measured through the transform, and what the
/// writer rasterises — a mesh, a tiling pattern, a shading no gradient states
/// exactly — drawn by the renderer at `raster_scale` and written through the
/// PNG encoder into a `data:` URI. Text is written as the outlines of the
/// fixtures' own faces, so one table serves the default and `bundled-fonts`
/// builds.
#[test]
fn svg_output_is_stable_across_targets() {
    // A mismatch prints the replacement lines ready to paste; the length check
    // is there because an empty table would pass by not looking.
    let expected: &[(&str, &str)] = &[
        (
            "text",
            "8cc277d52c2fa5ac801cd27120f9adbd2943f71ed5b3d69bc30b0a6e046f1a1a",
        ),
        (
            "gs_text",
            "8cc277d52c2fa5ac801cd27120f9adbd2943f71ed5b3d69bc30b0a6e046f1a1a",
        ),
        (
            "curves",
            "ca9b075cb334ca5e8dd8401d395f06f7da5af0df021c31493fa9aac497344b27",
        ),
        (
            "shading",
            "34ca88c83c577afe0f14c9b38f2b2dc9c3155a10b004889a625df8854622b07b",
        ),
        (
            "blend",
            "06aa852116a67b121d1f7ccdbde7aea0aabdf8596d19a1c9afcd8afc28c65b50",
        ),
        (
            "pattern",
            "5494551e8a1e1dca71ab8ff7e94cd9e5c5ec260221ec8cddd8c8c6dae841e8fc",
        ),
        (
            "optional",
            "358782a7a63e63938294c0323e349d293a25d8c1d7303e5e099ac26aee8b8ace",
        ),
        (
            "image",
            "f3a3492d5d30bec7cb3b531e7e5241e504d55a86c6b668fe9951ac23e6e2315e",
        ),
        (
            "transparency",
            "510840230ffef1ffda567ce242181acd3b4082e851c97dcdbed84b7d9cf96070",
        ),
        (
            "tiling",
            "b61ee3c2e2fdf8d84b62f925bce7c910f5b942e77aaa4299b3b50837bafddbde",
        ),
        (
            "jbig2",
            "53f4ce1899c7b06c31a69c19f7efe663bffd5b4eaca430d4ef9e20d41d223ad6",
        ),
        (
            "mesh",
            "ac3f390525f659fd802f3b6f7002752f727fefc2cd5bb9cc5294a79316de1654",
        ),
        (
            "jpx",
            "ab8df2a73b15a4b922dd286b603cadcab7c6c137f0558584817140a31c4ab1ec",
        ),
        (
            "cbz",
            "77aad756a02f1f5431828bddcb50c1cdd555a406c099fa7e1735e0981cd23e92",
        ),
        (
            "xps",
            "7002837758fdfcf2bb8a3753f44a4c2f53f0385546704109d421986df6531840",
        ),
        (
            "epub",
            "ef3c3ae7ddc44f2d8a22b0b0589efde0d1d09894217ae9afeb6cbea3705f9a30",
        ),
        (
            "analytic_axial",
            "1e554b33f30b7f6ee657b6099ad1611567158a1114c3cced45d46db58fdbbe0c",
        ),
        (
            "analytic_radial",
            "1da7340a402730866089f55876f6542de0abbde845e8458692c473317fca84da",
        ),
        (
            "analytic_blend",
            "4c809ac3056a114c1d7004cac59ca93a3c4e6e925db3a82b1b2aa08fba8fa085",
        ),
        (
            "analytic_image",
            "5d845a5952a5c2bd8577dfc61e14765a8da52ae6bb921e0f3f77be096fa4d7b3",
        ),
        (
            "default_spaces",
            "25633d01d2416d190d121513c742c7e4ba02034096fbe9146639bd7473b93ee3",
        ),
        (
            "strokes",
            "36c441354931cc517d46b3d87070e3075d3664eeb05124ecb189d126180633a9",
        ),
        (
            "bilevel",
            "17704be079071402a99457f13912c81f590ee940ff3625c311ec39ee19ac9f7f",
        ),
        ("icc", ""),
        ("jpeg_minified", ""),
        ("jpx_minified", ""),
        ("intents", ""),
    ];
    assert_eq!(
        expected.len(),
        GOLDEN.len(),
        "every page has an SVG hash, or the test passes by not looking"
    );

    let mut wrong = Vec::new();
    for fixture in GOLDEN {
        let actual = svg_fingerprint(fixture);
        let want = expected
            .iter()
            .find(|(n, _)| *n == fixture.name)
            .map(|(_, h)| *h)
            .unwrap_or_default();
        if actual != want {
            let name = fixture.name;
            wrong.push(format!("        (\"{name}\", \"{actual}\"),"));
        }
    }

    assert!(
        wrong.is_empty(),
        "the SVG output does not match the committed hashes.\n\n\
         If two targets disagree, this is a determinism bug — find the \
         arithmetic that is not target-stable, do not update the table.\n\
         If the SVG writer or the renderer changed deliberately, write the \
         page out with `Page::to_svg` and read the difference first, then \
         paste these in:\n\n{}\n",
        wrong.join("\n")
    );
}

/// The engine's own sRGB ICC profile, `tinker_pdf::SRGB_PROFILE`, hashed
/// (`docs/design/icc.md`, RC-19).
///
/// The profile is computed when the engine is built, by
/// `crates/tinker-pdf-color/build.rs` from IEC 61966-2-1's published
/// numbers, and a build script runs on the build host whatever the target:
/// so this is ruling 4 held over the build host's arithmetic as well as the
/// target's. The script's four IEEE operations and `tinker_pdf_math::pow`
/// make one answer on every host, and this pin is what says so on each
/// target the suite runs on. **A hash that differs between build hosts is a
/// determinism bug in `build.rs`, never a table edit.**
///
/// The hash covers every byte — the colorants and the curve, and as much the
/// header's date and the `desc` and `cprt` texts — so a one-byte change to
/// the description is another profile, and the second half of the test
/// shows the pin sees one.
#[test]
fn the_first_party_srgb_profile_is_one_byte_string_on_every_target() {
    let profile = tinker_pdf::SRGB_PROFILE;
    // The profile this hash is about, asserted rather than assumed: an ICC
    // profile of a display, RGB into XYZ, of the size RC-19.1's layout gives.
    assert_eq!(&profile[36..40], b"acsp", "an ICC profile");
    assert_eq!(&profile[12..24], b"mntrRGB XYZ ", "a display, RGB into XYZ");
    assert_eq!(profile.len(), 2624, "RC-19.1's layout");
    assert_eq!(
        sha(profile),
        "25219b24748844890523c69184ba4f4f010edba7c692be2166b57e8aa3e863cd",
        "the first-party sRGB profile is not the bytes it was. If two targets \
         or two build hosts disagree, this is a determinism bug in \
         `crates/tinker-pdf-color/build.rs` -- find the arithmetic that is not \
         host-stable, do not update the pin. If the profile changed \
         deliberately, `crates/tinker-pdf-color/tests/srgb_profile.rs` says \
         what it holds now. The profile is {} bytes.",
        profile.len()
    );

    let description = b"sRGB IEC 61966-2-1 (tinker-pdf)";
    let at = profile
        .windows(description.len())
        .position(|window| window == description)
        .expect("the description is in the profile");
    let mut changed = profile.to_vec();
    changed[at] ^= 0x01;
    assert_ne!(
        sha(&changed),
        sha(profile),
        "one byte of `desc` is another profile"
    );
}

/// The bytes the CBZ synthesiser hands `CosDocument::open`, hashed.
///
/// *Added August 2026, with gap 29 milestone 6.* The `cbz` fingerprint above
/// covers the synthesis only as far as the synthesis reaches pixels, and that
/// is less far than it looks: **object numbering, dictionary key order and
/// stream framing are invisible to it**, because a document renumbered or
/// re-keyed parses to the same page tree and draws the same picture. Ruling 4
/// is a claim about rendering, and this is the one place in this repository
/// where a *written* document is an input to it — so the written bytes get a
/// claim of their own.
///
/// It also covers three things no rendered hash can:
///
/// - **the writer's deflate encoder.** The decoded route re-compresses its own
///   samples in `png_embed`, so `zlib_compress`'s output is in these bytes.
///   Every other fixture in this file renders a document that was *parsed*, so
///   the compressor is on no other path here at all;
/// - **the pages nobody renders.** This hash is over the whole document, so
///   the RGBA page's `/SMask`, the placeholder's grey rectangle and the JPEG's
///   verbatim bytes are in it. The fingerprint above renders page 0 and sees
///   none of them;
/// - **an unreferenced object.** A stream written and then not pointed at
///   changes the length and the numbering and changes nothing on any page.
///
/// The two hashes together are what attribute a failure. If both move, the
/// synthesiser changed; if only the fingerprint moved, the renderer did; if
/// only this one moved, the document changed in a way that draws the same —
/// which is a real change and worth a sentence in the commit either way.
#[test]
fn the_synthesised_document_is_the_same_bytes_on_every_target() {
    let archive = cbz_page();
    let (pdf, report) = tinker_pdf::cbz::synthesise(
        tinker_pdf::Container::Zip,
        &archive,
        &tinker_pdf::cbz::Limits::DEFAULT,
    )
    .expect("the fixture archive synthesises");

    // The document this hash is about, asserted rather than assumed: four
    // pages, in natural order, with the GIF a placeholder and nothing else
    // degraded. A hash over the wrong document is stable and worthless.
    assert_eq!(
        report
            .pages()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["page2.png", "page3.png", "page4.gif", "page10.jpg"],
        "the fixture's pages are not the ones this hash was taken over"
    );
    assert_eq!(
        report.pages().iter().filter(|p| p.defect.is_some()).count(),
        1,
        "exactly one page is a placeholder: the GIF"
    );
    // Ruling 10: "it opened" and "it opened cleanly" stay distinguishable, and
    // for this archive the difference is one warning and no others. Without
    // this the fixture could be quietly damaged — an IDAT with a wrong
    // Adler-32 takes the decode route rather than passing through, renders the
    // same picture and reports a leniency only this count reads.
    assert_eq!(
        report.warnings().len(),
        1,
        "a well-formed archive opens with exactly the placeholder warning: {:?}",
        report.warnings()
    );
    assert_eq!(report.synthesised_bytes(), pdf.len());

    // Gap 29's routing table, entry by entry, in the document it produced. A
    // hash alone cannot say *why* it is what it is, and the failure this
    // guards is the one that leaves every picture correct: a build that quietly
    // decoded everything would render all four pages identically and would have
    // abandoned the whole design — the pass-through is what keeps a 200-page
    // archive's peak memory a multiple of the archive rather than of its pixel
    // count.
    for (needle, count, what) in [
        (&b"/Predictor 15"[..], 1, "the indexed PNG passed through"),
        (b"/Mask [0 1]", 1, "8.9.6.4's colour key, from the tRNS run"),
        (
            b"/Indexed /DeviceRGB 15",
            1,
            "/hival from the palette's own length",
        ),
        (b"/SMask", 1, "the RGBA PNG was decoded and split"),
        (b"/DCTDecode", 1, "the JPEG reached the page as itself"),
        (b"/ImageMask", 0, "nothing here is a stencil"),
    ] {
        assert_eq!(
            pdf.windows(needle.len()).filter(|w| *w == needle).count(),
            count,
            "{what}: {} in the synthesised document",
            String::from_utf8_lossy(needle),
        );
    }

    // The two pass-throughs are byte-for-byte, which is the claim the design
    // rests on and the one a dictionary key cannot make: the JPEG's own bytes
    // and the PNG's own IDAT are *in* the document, not a re-encoding of them.
    let jpeg = grey_jpeg(16, 16);
    assert!(
        pdf.windows(jpeg.len()).any(|w| w == jpeg),
        "the JPEG was re-encoded rather than placed"
    );
    let indexed = indexed_png(40, 40);
    let find = |tag: &[u8]| {
        indexed
            .windows(4)
            .position(|w| w == tag)
            .expect("the fixture has this chunk")
    };
    // The IDAT's data begins after its type and ends four bytes (its own CRC)
    // before the IEND chunk's length field, which is four before its type.
    let idat = &indexed[find(b"IDAT") + 4..find(b"IEND") - 8];
    assert!(
        pdf.windows(idat.len()).any(|w| w == idat),
        "the IDAT was rebuilt rather than copied"
    );

    let hash: String = tinker_pdf_crypto::sha2::sha256(&pdf)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hash,
        "995ed489d626a4388fd117f7b4c61bfa7d46a22e5839b17bf16f602af69bfc88",
        "the synthesised document is not the bytes it was; see this test's doc \
         comment for what that means and how to tell it apart from a rendering \
         change. The document is {} bytes.",
        pdf.len()
    );
}

/// The bytes the XPS synthesiser hands `CosDocument::open`, hashed.
///
/// *Added August 2026, with gap 30 milestone 9,* in the pair gap 29's milestone
/// 6 established and for the reason it established it: **a rendered hash cannot
/// see object numbering, dictionary key order or stream framing**, because a
/// document renumbered or re-keyed parses to the same page tree and draws the
/// same picture. Ruling 4 is a claim about rendering; this is the second place
/// in this repository where a *written* document is an input to it, so the
/// written bytes get a claim of their own.
///
/// What it covers that the `xps` fingerprint above cannot:
///
/// - **milestone 5's writer, as structure rather than as pixels.** The Type0
///   font's five objects, the `/W` array built from the face's own `hmtx`, the
///   `/ToUnicode` CMap and the tiling pattern's `/Matrix` are all in these
///   bytes. The renderer reads them back and draws the same page whether the
///   `/W` array is written as one run or as forty;
/// - **the ODTTF part as it lands in the document.** 9.1.7.3's de-obfuscation
///   produces a face this build then *subsets*, and a subsetter that emitted
///   the whole 189 252-byte face would render identically;
/// - **the routing table, on a file this repository did not write.** WPF's
///   serialiser emits its images as **RGBA**, which is the one colour type gap
///   29's pass-through cannot take — no PDF filter produces an alpha channel —
///   so this package reaches the *decoder* and is split into samples and an
///   `/SMask`. That is the fallback route arriving from a real producer rather
///   than from a fixture written to exercise it, and the assertions below say
///   so in both directions: an `/SMask` is present and a `/Predictor` is not.
///
/// The two hashes together attribute a failure. If both move, the synthesiser
/// changed; if only the fingerprint moved, the renderer did; if only this one
/// moved, the document changed in a way that draws the same — which is a real
/// change and worth a sentence in the commit either way.
#[test]
fn the_synthesised_fixed_document_is_the_same_bytes_on_every_target() {
    let package = xps_package();
    let archive = tinker_pdf::cbz::open_archive(&package, &tinker_pdf_zip::Limits::DEFAULT)
        .expect("the package is a readable ZIP");
    let (pdf, report) = match tinker_pdf::xps::route(archive, &tinker_pdf::xps::Limits::DEFAULT) {
        tinker_pdf::xps::Routing::Document(pdf, report) => (pdf, report),
        tinker_pdf::xps::Routing::Refused(why) => panic!("the package was refused: {why}"),
        tinker_pdf::xps::Routing::NotXps(_) => panic!("the package is an XPS"),
    };

    // The document this hash is about, asserted rather than assumed. A hash
    // over the wrong document is stable and worthless, and for this package the
    // wrong document has a name: before milestone 3 these same bytes opened as
    // a one-page *comic* whose single page was the 32 x 32 PNG.
    assert_eq!(
        report.xps_dialect(),
        Some(tinker_pdf::xps::Dialect::Xps1),
        "WPF's serialiser writes the XPS 1.0 namespace, which is decision 3"
    );
    assert_eq!(report.pages().len(), 1, "one FixedPage");
    assert!(
        report.pages()[0].defect.is_none(),
        "the page is the markup's own, not a placeholder: {:?}",
        report.pages()[0].defect
    );
    // Ruling 10: "it opened" and "it opened cleanly" stay distinguishable.
    // Milestone 8 is where this package stopped owing anything, and the
    // assertion inverted then — while any element was unpainted the document
    // could not be silent, and now it must be.
    assert!(
        report.warnings().is_empty(),
        "this package renders with nothing owed: {:?}",
        report.warnings()
    );
    assert_eq!(report.synthesised_bytes(), pdf.len());

    // The structure only a byte hash pins, named entry by entry so a failure
    // says which half moved. A hash alone cannot say *why* it is what it is.
    for (needle, count, what) in [
        (&b"/PatternType 1"[..], 1, "the ImageBrush became a pattern"),
        (b"/Identity-H", 1, "9.7.6's Type0 encoding"),
        (b"/CIDToGIDMap", 1, "the CID is the glyph index"),
        (b"/ToUnicode", 1, "the run's UnicodeString is extractable"),
        (
            b"/SMask",
            1,
            "the RGBA part took gap 29's decoder and was split",
        ),
        (
            b"/Predictor",
            0,
            "and therefore did *not* pass through: RGBA is the one colour type \
             that cannot, because no PDF filter produces an alpha channel",
        ),
        (b"/DCTDecode", 0, "this package holds no JPEG"),
        (b"/ImageMask", 0, "nothing here is a stencil"),
    ] {
        assert_eq!(
            pdf.windows(needle.len()).filter(|w| *w == needle).count(),
            count,
            "{what}: {} in the synthesised document",
            String::from_utf8_lossy(needle),
        );
    }

    // The picture is the part's own 32 x 32 and not the page's, which is the
    // shape of the original defect: before milestone 3 this package opened as a
    // comic whose one page *was* this image, at 32 x 32 points.
    for needle in [&b"/Width 32"[..], b"/Height 32"] {
        assert!(
            pdf.windows(needle.len()).any(|w| w == needle),
            "the image is the part's own size: {}",
            String::from_utf8_lossy(needle),
        );
    }

    // The ODTTF is **subsetted**, and the cheapest true statement of that is
    // that the whole document is smaller than the face it draws with: 9.1.7.3
    // de-obfuscates 189 252 bytes of Cascadia Mono and the run needs eight
    // glyphs of it. A build that embedded the face whole renders this page
    // identically, which is exactly the class of change no rendered hash sees.
    const DEOBFUSCATED_FACE: usize = 189_252;
    assert!(
        report.synthesised_bytes() < DEOBFUSCATED_FACE,
        "the document is {} bytes and the face it draws with is {DEOBFUSCATED_FACE}: \
         the subsetter is not running",
        report.synthesised_bytes(),
    );

    let hash: String = tinker_pdf_crypto::sha2::sha256(&pdf)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hash,
        "5aa40f7b728dac39497eed83de5552c0aebde34d546370f56c02b3d348c6a02d",
        "the synthesised fixed document is not the bytes it was; see this \
         test's doc comment for what that means and how to tell it apart from \
         a rendering change. The document is {} bytes.",
        pdf.len()
    );
}

/// Every fingerprinted page's mark box (`docs/design/page-analysis.md`), as
/// [`the_mark_boxes_are_the_same_on_every_target`] prints it: the box in
/// points, each number at its shortest, and in pixels at 72 dpi as
/// `(x, y, width, height)`, or `none` for a page that paints nothing.
///
/// Every value is a run's output and is recorded from one, the four targets
/// agreeing, as a render fingerprint is.
const MARK_BOXES: &[(&str, &str)] = &[
    ("text", ""),
    ("gs_text", ""),
    ("curves", ""),
    ("shading", ""),
    ("blend", ""),
    ("pattern", ""),
    ("optional", ""),
    ("image", ""),
    ("transparency", ""),
    ("tiling", ""),
    ("jbig2", ""),
    ("mesh", ""),
    ("jpx", ""),
    ("cbz", ""),
    ("xps", ""),
    ("epub", ""),
    ("analytic_axial", ""),
    ("analytic_radial", ""),
    ("analytic_blend", ""),
    ("analytic_image", ""),
    ("default_spaces", ""),
    ("strokes", ""),
    ("bilevel", ""),
    ("icc", ""),
    ("jpeg_minified", ""),
    ("jpx_minified", ""),
    ("intents", ""),
];

/// **The page analysis's box is the same number on every target.** The box is
/// arithmetic over every path, glyph outline, image square and clip a page
/// draws — Bézier extremes found through a square root, a stroke's outline
/// from the stroker, the page's view transform rounded outward — so a target
/// whose arithmetic differs moves a box as it would move a pixel. Each
/// fingerprinted page is analysed with the default options and its box held
/// to [`MARK_BOXES`]; a mismatch prints the lines to paste, under the render
/// fingerprints' rule: two targets that disagree are a determinism bug, never
/// a table edit.
#[test]
fn the_mark_boxes_are_the_same_on_every_target() {
    assert_eq!(
        MARK_BOXES.len(),
        GOLDEN.len(),
        "every page has a box, or the test passes by not looking"
    );
    let mut wrong = Vec::new();
    for fixture in GOLDEN {
        let (document, at) = opened(fixture);
        let page = document.page(at).expect("a page");
        let actual = match page.analysis(&RenderOptions::default()).marks {
            Some(marks) => format!(
                "{:?} {:?}",
                marks.points,
                (
                    marks.pixels.x,
                    marks.pixels.y,
                    marks.pixels.width,
                    marks.pixels.height
                )
            ),
            None => "none".to_string(),
        };
        let want = MARK_BOXES
            .iter()
            .find(|(n, _)| *n == fixture.name)
            .map(|(_, b)| *b)
            .unwrap_or_default();
        if actual != want {
            let name = fixture.name;
            wrong.push(format!("    (\"{name}\", \"{actual}\"),"));
        }
    }
    assert!(
        wrong.is_empty(),
        "the mark boxes do not match the committed table.\n\n\
         If two targets disagree, this is a determinism bug: do not update the \
         table. If the analysis changed on purpose, paste these in:\n\n{}\n",
        wrong.join("\n")
    );
}

/// The same document rendered twice in one process must agree, which is the
/// weaker property but catches anything reading uninitialised memory or
/// iterating a hash map.
#[test]
fn rendering_is_stable_within_one_process() {
    for fixture in GOLDEN {
        let name = fixture.name;
        assert_eq!(
            fingerprint(fixture),
            fingerprint(fixture),
            "{name} renders the same twice"
        );
    }
}

/// **A retained page is the page**: every fingerprinted page, recorded once
/// with `Page::display_list` and replayed at the fingerprints' own scale and
/// at three others, is byte-equal to a direct render — pixels, size, format
/// and warnings — with and without its annotations.
///
/// Added with the retained-page row, September 2026, and **not a new
/// fingerprint**: the table above is untouched, and a hash here would be
/// wrong in kind. This is an equality between two paths through the same
/// renderer, which is `streaming_determinism.rs`'s shape, and it sits here
/// rather than there because the row's exit criterion names these pages —
/// every operator family this file has ever enrolled, groups, soft masks,
/// tiling cells, meshes, both image codecs and all three synthesised formats.
/// One recording serves all four scales, which is the property the row is
/// about: nothing recorded is a pixel.
#[test]
fn a_display_list_replays_every_fingerprinted_page_byte_for_byte() {
    for fixture in GOLDEN {
        let (document, at) = opened(fixture);
        let page = document.page(at).expect("a page");
        let list = page.display_list();
        assert!(
            !list.is_empty(),
            "{}: the page recorded no calls",
            fixture.name
        );
        // 1.0 is the scale every fingerprint above is taken at.
        for scale in [1.0, 0.5, 1.5, 2.25] {
            for annotations in [true, false] {
                let options = RenderOptions {
                    scale,
                    annotations,
                    ..RenderOptions::default()
                };
                let direct = page.render(&options);
                let replayed = list.render(&options);
                let what = format!("{} at {scale}x, annotations {annotations}", fixture.name);
                assert_eq!(
                    (replayed.width, replayed.height, replayed.format),
                    (direct.width, direct.height, direct.format),
                    "{what}: a different bitmap shape"
                );
                if replayed.data != direct.data {
                    let differing = replayed
                        .data
                        .iter()
                        .zip(direct.data.iter())
                        .filter(|(a, b)| a != b)
                        .count();
                    panic!(
                        "{what}: the replay is not the page -- {differing} of {} bytes differ",
                        direct.data.len()
                    );
                }
                assert_eq!(
                    replayed.warnings, direct.warnings,
                    "{what}: the replay reported differently"
                );
            }
        }
    }
}

/// **What a document keeps across its renders draws what computing it again
/// draws** (`docs/design/performance.md`, P-16): every fingerprinted page,
/// rendered twice through a document whose budget keeps everything its
/// renders compute — so the second render is served by the document's cache
/// — and twice through one whose budget of zero keeps nothing, so that every
/// request computes again, hashes to its committed fingerprint each time and
/// reports what a render with no budget reports.
///
/// **Not a new fingerprint**: the table above is untouched, and a page that
/// disagrees with it here is a finding about the cache — a value that is not
/// a function of its key — or, where only one target disagrees, a
/// determinism bug, never a reason to edit the table. Each kind the cache
/// keeps is under this test as it lands, since the page renders through every
/// kind the document keeps. So that it cannot stop exercising the cache
/// unnoticed, the warm second renders must find something kept between them,
/// and the starved ones nothing.
#[test]
fn warm_and_starved_caches_render_the_fingerprints() {
    let budgets = [
        ("warm", CacheLimits::new(256 << 20)),
        ("starved", CacheLimits::new(0)),
    ];
    let counting = RenderOptions {
        stats: true,
        ..RenderOptions::default()
    };
    let mut wrong = Vec::new();
    let mut hits = [0u64; 2];
    for fixture in GOLDEN {
        let want = FINGERPRINTS
            .iter()
            .find(|(n, _)| *n == fixture.name)
            .map(|(_, h)| *h)
            .expect("every page has a fingerprint");
        let (document, at) = opened(fixture);
        let plain = document
            .page(at)
            .expect("a page")
            .render(&RenderOptions::default());
        for (which, (label, limits)) in budgets.iter().enumerate() {
            let budgeted = document.clone().with_cache_limits(*limits);
            let page = budgeted.page(at).expect("a page");
            for pass in ["first", "second"] {
                let bitmap = page.render(&counting);
                let stats = bitmap.stats.expect("a counted render hands its stats back");
                if pass == "second" {
                    hits[which] += stats.cache_hits;
                }
                let actual = hashed(fixture, &bitmap);
                if actual != want {
                    wrong.push(format!(
                        "{} {label}, {pass} render: {actual}, committed {want}",
                        fixture.name
                    ));
                }
                if bitmap.warnings != plain.warnings {
                    wrong.push(format!(
                        "{} {label}, {pass} render: reported {:?} where a render with no \
                         budget reports {:?}",
                        fixture.name, bitmap.warnings, plain.warnings
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a render through the document's cache is not the page its fingerprint \
         pins. A kept entry is what computing it again computes, so this is a \
         finding about the cache, or, where only one target disagrees, a \
         determinism bug -- never a reason to edit the table:\n{}",
        wrong.join("\n")
    );
    assert!(
        hits[0] > 0,
        "the warm second renders found nothing kept, so this test measured no hit"
    );
    assert_eq!(hits[1], 0, "a budget of zero keeps nothing to find");
}

/// `fixture` opened as [`opened`] opens it, with `limits` stated at open, so
/// that the document's parsed objects are charged to the budget beside its
/// cache.
fn opened_under(fixture: &Fixture, limits: CacheLimits) -> (Document, u32) {
    match fixture.open {
        Open::Closed => (
            converted(
                "opened under a budget",
                Document::open_with(
                    (fixture.build)(),
                    &OpenOptions::default().with_cache_limits(limits),
                )
                .expect("it opens"),
            ),
            0,
        ),
        Open::Book { page, at } => (
            converted(
                "opened under a budget",
                Document::open_with(
                    (fixture.build)(),
                    &OpenOptions::at_page(page.0, page.1)
                        .with_fonts(book_face())
                        .with_cache_limits(limits),
                )
                .expect("it opens"),
            ),
            at,
        ),
    }
}

/// **A budget stated at open renders the fingerprints too**
/// (`docs/design/performance.md`, P-16.8): every fingerprinted page, opened
/// with a budget that keeps everything — its parsed objects charged beside
/// its cache — and with a budget of zero, under which no parsed object is
/// kept and every read parses again, rendered twice each, hashes to its
/// committed fingerprint every time and reports what a render with no budget
/// reports. A parse is a function of the document's immutable bytes, so an
/// object dropped and read again is the object it was.
///
/// **Not a new fingerprint**: the table is untouched, and a page that
/// disagrees with it here is a finding about the object store under a
/// budget, never a reason to edit the table.
#[test]
fn budgets_stated_at_open_render_the_fingerprints() {
    let budgets = [
        ("warm at open", CacheLimits::new(256 << 20)),
        ("starved at open", CacheLimits::new(0)),
    ];
    let mut wrong = Vec::new();
    for fixture in GOLDEN {
        let want = FINGERPRINTS
            .iter()
            .find(|(n, _)| *n == fixture.name)
            .map(|(_, h)| *h)
            .expect("every page has a fingerprint");
        let (document, at) = opened(fixture);
        let plain = document
            .page(at)
            .expect("a page")
            .render(&RenderOptions::default());
        for (label, limits) in &budgets {
            let (budgeted, page_at) = opened_under(fixture, *limits);
            assert_eq!(page_at, at);
            assert!(
                budgeted.cos().objects_charged(),
                "{} {label}: the parsed objects are charged",
                fixture.name
            );
            for pass in ["first", "second"] {
                let bitmap = budgeted
                    .page(at)
                    .expect("a page")
                    .render(&RenderOptions::default());
                let actual = hashed(fixture, &bitmap);
                if actual != want {
                    wrong.push(format!(
                        "{} {label}, {pass} render: {actual}, committed {want}",
                        fixture.name
                    ));
                }
                if bitmap.warnings != plain.warnings {
                    wrong.push(format!(
                        "{} {label}, {pass} render: reported {:?} where a render with no \
                         budget reports {:?}",
                        fixture.name, bitmap.warnings, plain.warnings
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a render under a budget stated at open is not the page its fingerprint \
         pins. A parsed object read again is the object it was, so this is a \
         finding about the object store, or, where only one target disagrees, a \
         determinism bug -- never a reason to edit the table:\n{}",
        wrong.join("\n")
    );
}

/// The bytes the EPUB reader hands `CosDocument::open`, hashed.
///
/// *Added August 2026, with gap 31 milestone 13,* in the pair gap 29's
/// milestone 6 established and gap 30's milestone 9 repeated, and for the
/// reason they established it: **a rendered hash cannot see object numbering,
/// dictionary key order or stream framing**, because a document renumbered or
/// re-keyed parses to the same page tree and draws the same picture. Ruling 4
/// is a claim about rendering; this is the third place in this repository where
/// a *written* document is an input to it.
///
/// What it covers that the `epub` fingerprint above cannot:
///
/// - **milestone 5's annotations and outline.** Eight `/Link` annotations and
///   an `/Outlines` tree are in these bytes and on **no** rendered page: an
///   annotation is not content, so a build that wrote none would render this
///   book identically. That is a whole milestone of gap 31 that no fingerprint
///   in this file can see;
/// - **the pages nobody renders.** This hash is over all seven, so the cover
///   this build refuses as SVG, the title page and the navigation document are
///   in it. The fingerprint renders page 3 and sees none of them;
/// - **the page count itself**, which for a book is a function of the caller's
///   argument rather than of the file, and is the subject of
///   [`a_book_is_stable_at_each_page_box_and_the_two_boxes_differ`].
///
/// The two hashes together attribute a failure. If both move, the reader or the
/// writer changed; if only the fingerprint moved, the renderer did; if only
/// this one moved, the document changed in a way that draws the same --- which
/// is a real change and worth a sentence in the commit either way.
#[test]
fn the_synthesised_book_is_the_same_bytes_on_every_target() {
    let (pdf, report) = synthesise_book(BOOK_BOX);

    // The document this hash is about, asserted rather than assumed. A hash
    // over the wrong document is stable and worthless, and for a book the wrong
    // document has two names: before milestone 3 these bytes opened as a
    // one-page *comic* whose page was the cover, and before milestone 8 they
    // opened as five grey placeholders.
    assert_eq!(
        report.layout(),
        Some(tinker_pdf::epub::BookLayout::default()),
        "the report says which box it was laid out at, and it is the default"
    );
    assert_eq!(
        report
            .pages()
            .iter()
            .map(|page| page.name.as_str())
            .collect::<Vec<_>>(),
        [
            "EPUB/text/cover.xhtml",
            "EPUB/text/title_page.xhtml",
            "EPUB/nav.xhtml",
            "EPUB/text/ch001.xhtml",
            "EPUB/text/ch001.xhtml",
            "EPUB/text/ch001.xhtml",
            "EPUB/text/ch002.xhtml",
        ],
        "seven pages from five itemrefs, and chapter one takes three of them"
    );
    assert!(
        report.pages().iter().all(|page| page.defect.is_none()),
        "no page is a placeholder: {:?}",
        report.pages().iter().find(|page| page.defect.is_some())
    );
    assert_eq!(report.synthesised_bytes(), pdf.len());

    // What the book cost, entry by entry. Milestone 13 published these so that
    // `bounds_ledger.rs`'s third yardstick could be checked against a real book
    // rather than argued, and pinning them here is what stops the yardstick's
    // input from drifting under it.
    let cost = report.book_cost().expect("a book has a cost");
    assert_eq!(
        [
            cost.manifest_items,
            cost.spine_items,
            cost.css_tokens,
            cost.css_rules,
            cost.css_declarations,
            cost.selector_matches,
            cost.boxes,
            cost.break_work,
            cost.layout_work,
            cost.pages,
        ],
        [8, 5, 8_344, 279, 585, 1_240, 326, 3_845, 0, 7],
        "the book did not cost what this hash was taken over"
    );

    // The structure only a byte hash pins, named entry by entry so a failure
    // says which half moved. The annotations are counted by their subtype:
    // since the book is tagged, `/Link` is also a structure type and the
    // marked-content tag of its sequences, and a bare `/Link` counted those.
    for (needle, count, what) in [
        (
            &b"/Subtype /Link"[..],
            8,
            "milestone 5's annotations, on no rendered page",
        ),
        (
            b"/S /Link",
            7,
            "the `<a>` elements holding them, as /Link structure elements",
        ),
        (b"/Outlines", 2, "the navigation document, as an outline"),
        (
            b"/FontFile2",
            0,
            "the book embeds no face, so the writer has none to subset: this is \
             the `FontProvider` handover, as a zero",
        ),
        (b"/DCTDecode", 0, "the cover is SVG and is refused by name"),
        (b"/ImageMask", 0, "nothing here is a stencil"),
    ] {
        assert_eq!(
            pdf.windows(needle.len()).filter(|w| *w == needle).count(),
            count,
            "{what}: {} in the synthesised book",
            String::from_utf8_lossy(needle),
        );
    }

    let hash = sha(&pdf);
    assert_eq!(
        hash,
        "bcc9bb59d4898bcf5ff43fab0578945473a5c3ac80325a0fb220f63e7be82868",
        "the synthesised book is not the bytes it was; see this test's doc \
         comment for what that means and how to tell it apart from a rendering \
         change. The document is {} bytes.",
        pdf.len()
    );
}

/// The default page box, which is [`tinker_pdf::epub::DEFAULT_PAGE`]'s.
const BOOK_BOX: (f64, f64) = (432.0, 648.0);
/// A second box, and the whole point of it is that it is a different one.
const OTHER_BOOK_BOX: (f64, f64) = (600.0, 800.0);

/// Hex of a SHA-256, which three tests here want.
fn sha(bytes: &[u8]) -> String {
    tinker_pdf_crypto::sha2::sha256(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The book, read and written at one page box.
fn synthesise_book(page: (f64, f64)) -> (Vec<u8>, tinker_pdf::cbz::ArchiveReport) {
    let bytes = epub_book();
    let archive = tinker_pdf::cbz::open_archive(&bytes, &tinker_pdf_zip::Limits::DEFAULT)
        .expect("the book is a readable ZIP");
    let layout = tinker_pdf::epub::BookLayout {
        page,
        font_size: tinker_pdf::epub::DEFAULT_FONT_SIZE,
    };
    match tinker_pdf::epub::route(archive, &tinker_pdf::epub::Limits::DEFAULT, &layout) {
        tinker_pdf::epub::Routing::Document(pdf, report) => (pdf, report),
        tinker_pdf::epub::Routing::Refused(why) => panic!("the book was refused: {why:?}"),
        tinker_pdf::epub::Routing::NotEpub(_) => panic!("the book is an EPUB"),
        // `Routing` is `#[non_exhaustive]` and a wildcard here would swallow a
        // variant a later gap added, so it says so.
        _ => panic!("a routing this test has never seen"),
    }
}

/// **This format's own determinism question, which no earlier one had.**
///
/// *Added August 2026, with gap 31 milestone 13.* Fourteen fingerprints stood
/// in this file before the fifteenth and not one of them could ask this,
/// because in every format before EPUB the page count is a property of the
/// file. A comic's pages are its entries; a fixed document's are its
/// `FixedPage` parts; a PDF's are its page tree. **A book's are a function of
/// an argument the caller passed**, which is what
/// [`tinker_pdf::epub::DEFAULT_PAGE`]'s doc comment says in as many words.
///
/// So ruling 4 has a second half here, and it takes **three** assertions rather
/// than one:
///
/// 1. the same book at 432 x 648 is the same bytes every time and on every
///    target;
/// 2. the same book at 600 x 800 is the same bytes every time and on every
///    target;
/// 3. **and the two are not the same bytes.**
///
/// The third is the one that carries the weight, and the rule eighteen
/// milestones have found is why it is written down: *when a thing has two
/// independent consequences, a test for one of them is not a test.* A build
/// that ignored the page box entirely --- that paginated every book at the
/// default and reported the caller's numbers back unchanged --- would satisfy
/// "stable" twice over and be exactly as wrong as a build that laid the book
/// out at random. Stability is not evidence that the argument was used.
///
/// What differs is asserted in three places rather than trusted to the hash,
/// because a hash that moved for some *other* reason would let this pass while
/// the page box was still ignored: the page count (seven against six), the
/// number of cross-reference annotations the writer emitted (eight against
/// seven, because a link's target page changes with the pagination), and the
/// rendered page itself.
#[test]
fn a_book_is_stable_at_each_page_box_and_the_two_boxes_differ() {
    let (first, report) = synthesise_book(BOOK_BOX);
    let (again, _) = synthesise_book(BOOK_BOX);
    let (other, other_report) = synthesise_book(OTHER_BOOK_BOX);
    let (other_again, _) = synthesise_book(OTHER_BOOK_BOX);

    // (1) and (2): each box is stable, in this process and --- through the
    // committed hashes below --- on every target.
    assert_eq!(sha(&first), sha(&again), "432 x 648 is not stable");
    assert_eq!(sha(&other), sha(&other_again), "600 x 800 is not stable");
    assert_eq!(
        sha(&first),
        "bcc9bb59d4898bcf5ff43fab0578945473a5c3ac80325a0fb220f63e7be82868",
        "the book at 432 x 648 is not the bytes it was"
    );
    assert_eq!(
        sha(&other),
        "3b288a033d40d0fd80d010901c4a79dbd8c45c68e3f79ffd5974e21678f19f9d",
        "the book at 600 x 800 is not the bytes it was"
    );

    // (3): and they differ. This is the assertion a build that ignored the box
    // fails, and it is the only one here that it fails.
    assert_ne!(
        sha(&first),
        sha(&other),
        "the same book at two page boxes produced the same bytes, which means \
         the box did not reach the pagination"
    );

    // Named differences, so a failure says *what* stopped depending on the box
    // rather than only that something did.
    assert_eq!(
        (report.pages().len(), other_report.pages().len()),
        (7, 6),
        "the page count is a function of the page box"
    );
    assert_eq!(
        report.layout().map(|layout| layout.page),
        Some(BOOK_BOX),
        "the report says which box it was"
    );
    assert_eq!(
        other_report.layout().map(|layout| layout.page),
        Some(OTHER_BOOK_BOX),
        "and so does the other"
    );
    // By subtype, for the reason the needle above gives.
    let links = |pdf: &[u8]| pdf.windows(14).filter(|w| *w == b"/Subtype /Link").count();
    assert_eq!(
        (links(&first), links(&other)),
        (8, 7),
        "a cross-reference names the page its target landed on, so the writer's \
         annotation count moves with the pagination"
    );

    // **Through the facade as well**, which the injection matrix had to find.
    // Everything above goes straight to `epub::route`, and on that path the
    // caller's two numbers are already a `BookLayout`. The path a *caller*
    // takes has one more step in it — `BookLayout::sanitised`, where an
    // arbitrary pair of `f64`s becomes a usable box — and the matrix injected
    // "the caller's page width is ignored" there and **nothing above failed**:
    // a build that replaced the width with the default still produced two
    // different documents, because the height still differed. Two independent
    // consequences, and a test for one of them is not a test.
    //
    // So the page a caller is handed is asserted to be the page a caller asked
    // for, in both directions, which is the assertion that defect fails.
    for (box_, pages) in [(BOOK_BOX, 7u32), (OTHER_BOOK_BOX, 6)] {
        let document = converted(
            "a book is stable at each page box and the two boxes differ",
            Document::open_with(
                epub_book(),
                &OpenOptions::at_page(box_.0, box_.1).with_fonts(book_face()),
            )
            .expect("it opens"),
        );
        assert_eq!(
            document.page_count(),
            pages,
            "a book opened at {box_:?} paginated into {} pages",
            document.page_count()
        );
        assert_eq!(
            document.page(0).expect("a page").size(),
            box_,
            "the page a caller is handed is not the box the caller asked for"
        );
        assert_eq!(
            document
                .archive()
                .and_then(|report| report.layout())
                .map(|layout| layout.page),
            Some(box_),
            "the report a caller is handed does not say which box it was"
        );
    }

    // And the rendered page, which is the other half of ruling 4: the same
    // page **index** at two boxes is two different pages, and both are stable.
    let render = |page: (f64, f64)| {
        let fixture = Fixture {
            name: "epub",
            build: epub_book,
            open: Open::Book { page, at: 3 },
            least_ink: 22_000,
        };
        fingerprint(&fixture)
    };
    let at_default = render(BOOK_BOX);
    assert_eq!(at_default, render(BOOK_BOX), "the render is not stable");
    let at_other = render(OTHER_BOOK_BOX);
    assert_eq!(
        at_other,
        render(OTHER_BOOK_BOX),
        "the render at the second box is not stable"
    );
    assert_ne!(
        at_default, at_other,
        "page 3 renders identically at two page boxes"
    );
}

/// A PDF of `objects`, numbered from 1 in order, with a cross-reference table,
/// so that it opens without repair and an update over it is read alone.
fn classic(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
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

/// A stream object's body: `dict`'s entries, its `/Length`, and `data`.
fn stream_object(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// **The page the patched render's fingerprint is of, before its edit**
/// (P-17.7): a 200 x 120 page of a wash, a box, two diagonals and a 2 x 2
/// image, under two square annotations with appearances of their own —
/// object 5, red over `[30 20 90 70]`, its appearance object 6, and object 7,
/// a green diamond over `[110 40 170 100]`, its appearance object 8. Shapes
/// and samples only, so both builds draw it alike.
fn annotated_page() -> Vec<u8> {
    classic(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 120]\n\
          /Resources << /XObject << /Im 9 0 R >> >> /Contents 4 0 R /Annots [5 0 R 7 0 R] >>"
            .to_vec(),
        stream_object(
            "",
            b"0.95 0.9 0.6 rg 10 10 180 100 re f\n\
              0.3 0.5 0.9 rg 60 40 80 40 re f\n\
              0 0 0 RG 1.5 w 0 0 m 200 120 l S 0 120 m 200 0 l S\n\
              q 30 0 0 20 150 15 cm /Im Do Q",
        ),
        b"<< /Type /Annot /Subtype /Square /Rect [30 20 90 70] /F 4 /AP << /N 6 0 R >> >>".to_vec(),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 50]",
            b"0.8 0.1 0.1 rg 0 0 60 50 re f 0 0 0 RG 2 w 4 4 52 42 re S",
        ),
        b"<< /Type /Annot /Subtype /Square /Rect [110 40 170 100] /F 4 /AP << /N 8 0 R >> >>"
            .to_vec(),
        stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 60 60]",
            b"0.1 0.6 0.2 rg 0 0 60 60 re f 0 0 1 RG 3 w 30 5 m 55 30 l 30 55 l 5 30 l h S",
        ),
        stream_object(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /ASCIIHexDecode",
            b"FF000000FF000000FFFFFFFF>",
        ),
    ])
}

/// **The edit the patched render's fingerprint is of**, fixed here: the red
/// square's appearance redrawn blue where it is, the green diamond moved 30 pt
/// left and 15 pt down onto the square's corner, and a purple square, its
/// appearance synthesized, added over `[140 10 190 50]` — a recolouring, a
/// move and an addition, every kind of rectangle a patch is made of.
fn edit_annotations(editor: &mut tinker_pdf::DocumentEditor, document: &Document) {
    use tinker_pdf::{Dict, ObjRef, Object};
    use tinker_pdf_cos::{annot, Name, Rect, StreamData};

    let look = ObjRef::new(6, 0);
    let mut dict = Dict::new();
    if let Some(Object::Stream(stream)) = editor.get(look) {
        for (key, value) in stream.dict.iter() {
            if *key != Name::LENGTH {
                dict.insert(*key, value.clone());
            }
        }
    }
    editor.put_stream(
        look,
        StreamData {
            dict,
            data: b"0.1 0.2 0.9 rg 0 0 60 50 re f 1 1 1 RG 2 w 4 4 52 42 re S".to_vec(),
        },
    );

    let diamond = ObjRef::new(7, 0);
    if let Some(Object::Dict(mut dict)) = editor.get(diamond) {
        dict.insert(
            editor.intern(b"Rect"),
            Object::Array(
                [80.0, 25.0, 140.0, 85.0]
                    .into_iter()
                    .map(Object::Real)
                    .collect(),
            ),
        );
        editor.put(diamond, Object::Dict(dict));
    }

    let square = annot::square(
        document.cos(),
        Rect {
            x0: 140.0,
            y0: 10.0,
            x1: 190.0,
            y1: 50.0,
        },
        annot::Color {
            r: 0.5,
            g: 0.0,
            b: 0.5,
        },
        3.0,
    );
    editor.add_annotation(0, square).expect("the page is there");
}

/// The committed fingerprint of [`annotated_page`] after [`edit_annotations`],
/// rendered at the default options, as [`hashed`] hashes a page (P-17.7).
///
/// A run's output, recorded from `cargo test` on the first target it runs
/// on, which prints the value to paste, and held there on every other.
const PATCHED_PAGE: &str = "";

/// **A patched render reproduces the edited page's fingerprint** (P-17.7).
/// [`annotated_page`], edited by [`edit_annotations`] and appended
/// (`write::append`), is rendered both ways: patched over its render before
/// the edit by the rectangle the edit changed (`Appended::patch`), and whole.
/// The patch takes the page rather than rendering it whole, and both bitmaps
/// hash to the one committed fingerprint, on every target.
#[test]
fn a_patched_render_reproduces_the_edited_pages_fingerprint() {
    use tinker_pdf::write::{append, AppendOptions};

    let original = annotated_page();
    let document = Document::open(original.clone()).expect("it opens");
    let options = RenderOptions::default();
    let mut previous = document.page(0).expect("a page").render(&options);
    assert!(previous.warnings.is_empty(), "{:?}", previous.warnings);

    let mut editor = document.editor();
    edit_annotations(&mut editor, &document);
    let appended = append(&document, &editor, &AppendOptions::default()).expect("it appends");
    written_support::rewritten(
        "the patched page's update",
        original.clone(),
        [&original[..], &appended.update[..]].concat(),
    );
    let patch = appended
        .patch(0, &mut previous, &options)
        .expect("the edited page");
    assert_eq!(patch.fallback, None, "the page is patched, not drawn whole");
    let region = patch.region.expect("the edit changed pixels");
    assert!(
        u64::from(region.width) * u64::from(region.height) < 200 * 120,
        "{region:?} is less than the page"
    );
    let whole = appended
        .document
        .page(0)
        .expect("the edited page")
        .render(&options);

    // The wash alone paints 18 000 of the page's 24 000 pixels.
    let fixture = Fixture {
        name: "patched",
        build: annotated_page,
        open: Open::Closed,
        least_ink: 9_000,
    };
    let (patched, full) = (hashed(&fixture, &previous), hashed(&fixture, &whole));
    assert_eq!(patched, full, "the patch is the full render after the edit");
    assert_eq!(
        full, PATCHED_PAGE,
        "the edited page does not render to its committed fingerprint. If two \
         targets disagree, this is a determinism bug; if this is a deliberate \
         rendering change, compare the page first, then paste:\n\n\
         const PATCHED_PAGE: &str = \"{full}\";\n"
    );
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}

/// `document`, and what `Document::open` synthesised for it held to the
/// validate-on-save helper. A PDF is opened, not written, and is handed back
/// as it is.
#[track_caller]
fn converted(label: &str, document: tinker_pdf::Document) -> tinker_pdf::Document {
    if document.archive().is_some() {
        written_support::synthesised(label, document)
    } else {
        document
    }
}
