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
    // 5262 today, out of 19 200. The five placements are disjoint, so this
    // floor is also a guard against one of them silently drawing nothing:
    // losing the rotated placement or either of the two magnified ones takes
    // the page below it.
    Fixture {
        name: "image",
        build: image_page,
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
        least_ink: 3600,
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
        // Added August 2026 with gap 12. No existing fixture drew an image,
        // so the whole of image sampling — every row of the policy matrix,
        // the bilinear weights and the pyramid — was outside what this file
        // measured.
        (
            "image",
            "8cca4e2c1380f630e1c85da93b3a6add4349156d704adbffca7d45d917244f38",
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
            "aa7b2df6bd7613fb53c696ed4b9018a00d1aa4dece2ffe82775c40bfaa1a5011",
        ),
        // Added August 2026 with gap 17. Nothing above decodes a JBIG2
        // stream, and the MQ arithmetic coder underneath it is shared with
        // whatever gap 18 decides about JPEG 2000 -- so a change made for one
        // codec that shifted a row of the Qe table would, until now, have had
        // only the filter crate's own tests standing in front of it.
        (
            "jbig2",
            "cd20bc1e5c786e245402ba94d700f2a91a267c36e0922d2bc98be5e897839abd",
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
            "546f7f9e61572460b1b76610719e772b69625651d6a6b3b820ab30538be7d693",
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
            "d9d0a1f733de50ca06fae32655bc240854d573679698ce7a8e8095640972ef4d",
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
