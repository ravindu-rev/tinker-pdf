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

use tinker_pdf::{Bitmap, Document, DocumentBuilder, RenderOptions, RenderWarning};

/// A named page: how to build it, and the least ink it may draw.
struct Fixture {
    name: &'static str,
    build: fn() -> Vec<u8>,
    /// The fewest non-background pixels this page may paint.
    ///
    /// A floor, not a measurement — roughly half of what the page draws
    /// today, so an ordinary rendering change moves the hash and leaves this
    /// alone, while a fixture that has stopped drawing trips it. The point is
    /// that "the fingerprints did not move" cannot again be evidence about a
    /// path the fixture never exercised.
    least_ink: usize,
}

/// Pixels that are not the white the page started as.
fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel.iter().any(|value| *value != 255))
        .count()
}

/// The rendered bytes of the first page, hashed.
fn fingerprint(fixture: &Fixture) -> String {
    let bitmap = Document::open((fixture.build)())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    let drawn = ink(&bitmap);
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
    builder.finish()
}

/// One outline point: its position in font units, and whether it lies on the
/// curve.
type Point = (i16, i16, bool);
/// A closed contour.
type Contour = &'static [Point];
/// One glyph, as its contours.
type Shape = &'static [Contour];

/// The six outlines of [`curvy_font`], glyph 1 upward; glyph 0 is `.notdef`
/// and empty.
///
/// Chosen for what they make the rasteriser do, not for looking like letters.
/// A box outline — four axis-aligned edges — exercises almost nothing: every
/// span is full or empty and no coverage value between 0 and 1 ever arises.
/// These do, in six different ways:
///
/// 1. a chevron: long diagonals meeting at a thin apex, with a notch;
/// 2. a ring: two curved contours wound in opposite directions, so the hole
///    depends on the fill rule as well as on the arithmetic;
/// 3. a wedge: one quadratic spanning the whole em against two straight
///    edges, which is flattening tolerance on its own;
/// 4. a slash: a parallelogram at a shallow angle, nothing but partial
///    coverage down both sides;
/// 5. a ribbon: consecutive off-curve points, so the implied on-curve
///    midpoint rule decides where the curve actually goes;
/// 6. a dot over a stem: two contours of very different size in one glyph,
///    the small one curved and the thin one diagonal.
const SHAPES: &[Shape] = &[
    // 1. Chevron.
    &[&[
        (20, 0, true),
        (240, 700, true),
        (320, 700, true),
        (540, 0, true),
        (420, 0, true),
        (280, 380, true),
        (140, 0, true),
    ]],
    // 2. Ring: the outer contour runs clockwise and the inner one
    // anticlockwise, which is what makes the middle a hole.
    &[
        &[
            (280, 630, true),
            (560, 630, false),
            (560, 350, true),
            (560, 70, false),
            (280, 70, true),
            (0, 70, false),
            (0, 350, true),
            (0, 630, false),
        ],
        &[
            (280, 500, true),
            (130, 500, false),
            (130, 350, true),
            (130, 200, false),
            (280, 200, true),
            (430, 200, false),
            (430, 350, true),
            (430, 500, false),
        ],
    ],
    // 3. Wedge.
    &[&[
        (0, 0, true),
        (560, 0, true),
        (560, 700, false),
        (0, 700, true),
    ]],
    // 4. Slash.
    &[&[
        (0, 0, true),
        (200, 0, true),
        (560, 700, true),
        (360, 700, true),
    ]],
    // 5. Ribbon. Each edge is two quadratics meeting at a point the font
    // never states — halfway between the two off-curve points.
    &[&[
        (40, 0, true),
        (40, 340, false),
        (520, 360, false),
        (520, 700, true),
        (400, 700, true),
        (360, 300, false),
        (200, 260, false),
        (160, 0, true),
    ]],
    // 6. Dot over a stem.
    &[
        &[
            (140, 680, true),
            (260, 680, false),
            (260, 560, true),
            (260, 440, false),
            (140, 440, true),
            (20, 440, false),
            (20, 560, true),
            (20, 680, false),
        ],
        &[
            (240, 0, true),
            (380, 0, true),
            (560, 420, true),
            (420, 420, true),
        ],
    ],
];

/// How many glyphs the face has, `.notdef` included.
const GLYPHS: u16 = SHAPES.len() as u16 + 1;
/// The advance of every shape, in font units, and of the space.
const ADVANCE: u16 = 640;
const SPACE_ADVANCE: u16 = 320;

/// Which glyph a character code selects: the space is empty, and every other
/// printable code takes the six shapes in turn.
fn glyph_for(code: u16) -> u16 {
    if code == 0x20 {
        return 0;
    }
    1 + (code - 0x21) % (GLYPHS - 1)
}

/// A synthetic TrueType face of curves and diagonals.
///
/// Built here rather than read from the system, because ruling 4 is a claim
/// about every target — including `wasm32-unknown-unknown`, where there are
/// no font directories to read — and because a repository that carries no
/// font carries nobody's licence.
fn curvy_font() -> Vec<u8> {
    // Glyph 0 is `.notdef` and has no outline: an empty `loca` range, which
    // is how a font says "no shape" (a repeated offset, rather than a zero
    // one).
    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0];
    for shape in SHAPES {
        glyf.extend_from_slice(&glyph_data(shape));
        loca.push(glyf.len() as u32);
    }

    let mut loca_bytes = Vec::new();
    for offset in &loca {
        loca_bytes.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca offsets

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&GLYPHS.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&GLYPHS.to_be_bytes()); // numberOfHMetrics

    // The advances the builder reads out to write /Widths with, so the text
    // is spaced by the same numbers the outlines are drawn from.
    let mut hmtx = Vec::new();
    for glyph in 0..GLYPHS {
        let advance = if glyph == 0 { SPACE_ADVANCE } else { ADVANCE };
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // left side bearing
    }

    let cmap = cmap();
    let tables: [(&[u8; 4], &[u8]); 7] = [
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca_bytes),
        (b"maxp", &maxp),
    ];

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]); // search hints, unread

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// One glyph's `glyf` entry.
fn glyph_data(shape: Shape) -> Vec<u8> {
    let points: Vec<Point> = shape.iter().flat_map(|c| c.iter().copied()).collect();
    let xs = || points.iter().map(|p| p.0);
    let ys = || points.iter().map(|p| p.1);

    let mut out = Vec::new();
    out.extend_from_slice(&(shape.len() as i16).to_be_bytes());
    out.extend_from_slice(&xs().min().unwrap_or(0).to_be_bytes()); // xMin
    out.extend_from_slice(&ys().min().unwrap_or(0).to_be_bytes()); // yMin
    out.extend_from_slice(&xs().max().unwrap_or(0).to_be_bytes()); // xMax
    out.extend_from_slice(&ys().max().unwrap_or(0).to_be_bytes()); // yMax

    let mut end = 0usize;
    for contour in shape {
        end += contour.len();
        out.extend_from_slice(&((end - 1) as u16).to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes()); // no hinting instructions

    // Bit 0 is the on-curve flag. None of the short-coordinate or repeat bits
    // are set, so every delta below is a signed 16-bit word — larger than a
    // real font would write, and far easier to read.
    for (_, _, on_curve) in &points {
        out.push(u8::from(*on_curve));
    }
    let mut previous = 0i16;
    for (x, _, _) in &points {
        out.extend_from_slice(&(x - previous).to_be_bytes());
        previous = *x;
    }
    let mut previous = 0i16;
    for (_, y, _) in &points {
        out.extend_from_slice(&(y - previous).to_be_bytes());
        previous = *y;
    }

    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// A `cmap` covering printable ASCII (9.6.6.4).
///
/// Format 4 through its `idRangeOffset` branch — the one where the segment
/// points into a glyph index array at an offset measured from its own slot,
/// which is the awkward part of the format and the part a real font uses.
/// Going through the array rather than a plain delta is what lets every
/// character in the fixture's text draw, out of a face with six shapes.
fn cmap() -> Vec<u8> {
    const FIRST: u16 = 0x20;
    const LAST: u16 = 0x7E;
    // The real segment, and the terminating one at 0xFFFF the format requires.
    const SEGMENTS: u16 = 2;

    let mut sub = Vec::new();
    for value in [4u16, 0, 0, SEGMENTS * 2, 0, 0, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    sub.extend_from_slice(&LAST.to_be_bytes()); // endCode
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    sub.extend_from_slice(&FIRST.to_be_bytes()); // startCode
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes()); // idDelta: the array is absolute
    sub.extend_from_slice(&1u16.to_be_bytes());
    // idRangeOffset: the glyph array begins immediately after this array, and
    // the offset is counted from this slot, so it is the distance to the end
    // of the array — two bytes for each segment from this one on.
    sub.extend_from_slice(&(SEGMENTS * 2).to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    for code in FIRST..=LAST {
        sub.extend_from_slice(&glyph_for(code).to_be_bytes());
    }

    let mut cmap = Vec::new();
    // One (3,1) Windows Unicode BMP subtable, which is the one a reader
    // prefers and the one a Latin face would carry.
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);
    cmap
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
const GOLDEN: &[Fixture] = &[
    // The floors are about half of what each page paints today: 1486, 2363,
    // 9600, 3600 and 3230 pixels.
    Fixture {
        name: "text",
        build: text_page,
        least_ink: 700,
    },
    Fixture {
        name: "curves",
        build: curves_page,
        least_ink: 1100,
    },
    Fixture {
        name: "shading",
        build: shading_page,
        least_ink: 4500,
    },
    Fixture {
        name: "blend",
        build: blend_page,
        least_ink: 1700,
    },
    Fixture {
        name: "pattern",
        build: pattern_page,
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
        least_ink: 2400,
    },
];

#[test]
fn rendering_is_stable_across_targets() {
    // A mismatch prints the replacement lines ready to paste. The length
    // check below is there because an empty table would make this test pass
    // by not looking at anything.
    let expected: &[(&str, &str)] = &[
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
            "b0bc9383d116d84d7a104afc67b3d5dc8e727323ba30262f67121a32b89004c2",
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
        // Added August 2026 with gap 07; no existing fixture reached
        // `fill_with_pattern` at all.
        (
            "pattern",
            "18765f39455bc173f00fc6272449402d0c5db445963b5334e3d511a766199af2",
        ),
        // Added August 2026 with gap 06. No existing fixture has an
        // `/OCProperties`, so none of them would move if 8.11 stopped working.
        (
            "optional",
            "e0f2bc33f56dcb85beb7a1770f9cb33e22a1a2cdba1cbb4b838be656370035a1",
        ),
    ];
    assert_eq!(
        expected.len(),
        GOLDEN.len(),
        "every page has a fingerprint, or the test passes by not looking"
    );

    let mut wrong = Vec::new();
    for fixture in GOLDEN {
        let actual = fingerprint(fixture);
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
    for fixture in GOLDEN {
        let name = fixture.name;
        assert_eq!(
            fingerprint(fixture),
            fingerprint(fixture),
            "{name} renders the same twice"
        );
    }
}
