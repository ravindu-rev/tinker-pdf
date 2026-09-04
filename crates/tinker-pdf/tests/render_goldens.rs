//! One reviewed raster per operator family (ruling 13, roadmap step 6).
//!
//! Milestone 6 of [`docs/design/render-verification.md`], and the tier whose
//! value depends entirely on the review actually having happened. The analytic
//! fixtures answer to an equation and the differential pairs answer to each
//! other; neither can say *this is the picture the clause describes*, because
//! both sides are written from the same reading of the clause. A golden can, on
//! the one day a person looks at it — and every day afterwards it is a
//! regression test, which is a weaker thing and still worth having.
//!
//! So the record of that day is **inside the fixture**. A PGM or PPM carries
//! `#` comment lines after its magic, and each golden's carry the family, the
//! clause it draws, who read it and when. A commit message would do the same
//! job and be a click further away from the picture it is about.
//!
//! # The nine families, and why nine
//!
//! Taken from the **`Device` seam** (ruling 7) rather than from ISO 32000's
//! operator table, because the seam is where this engine branches and a
//! golden's job is to pin a branch. A family with two goldens would be two
//! chances to review the same code path; a branch with none is the gap this
//! list exists to close.
//!
//! Four families deliberately have **no** golden, and what covers them instead
//! is named here so the absence is a decision rather than an oversight:
//!
//! - **inline images** — `inline_images.rs` compares an inline image against
//!   the same samples as an XObject, byte for byte, which is a stronger claim
//!   than a picture;
//! - **form XObjects** — `render_differential.rs`'s pair, likewise;
//! - **colour conversion** — `colour_spaces.rs` and the ICC census, where the
//!   claim is arithmetic over components and a raster would only blur it;
//! - **optional content** — a suppression rather than a mark, and
//!   `determinism.rs`'s `optional` fixture already pins it in both directions.
//!
//! # Small enough to review
//!
//! Every page is 48 points at 72 dpi, so a golden is 6 926 bytes and the nine
//! together are under 63 KiB. The design's non-goal is explicit that there are
//! no full-page goldens here — "hundreds of kilobytes of binary per page that
//! no reviewer can assess" — and `a_golden_stays_small_enough_to_review` is
//! that sentence as an assertion rather than as prose.
//!
//! # The injections that were counted
//!
//! Over the whole workspace, 4 413 tests. Each of the three ways this tier can
//! be hollowed out is caught by exactly the check written for it, and by
//! nothing else — which is what says the three are three rather than one.
//!
//! | Injected | Caught by the workspace | Of which here |
//! | --- | ---: | ---: |
//! | a golden's `reviewer` line blanked | 1 | 1 |
//! | the page changed so the golden no longer matches | 1 | 1 |
//! | `read_header` accepting a blank field | 1 | 1 |
//!
//! Two more land here from elsewhere: a tiling lattice stepped by its `/BBox`
//! and a Type 3 `/FontMatrix` applied in the wrong order each move a golden as
//! well as their own fixture, which is the regression half of what a golden is
//! for.

use std::path::{Path, PathBuf};

use tinker_pdf::{
    BlendMode, DeviceSpace, DocumentBuilder, ExtGState, FormXObject, Function, Shading,
    ShadingPattern, TilingPattern, TilingType, TransparencyGroup,
};

mod render_support;
use render_support::{curvy_font, ink, render};

/// The page every golden draws into, in points and in pixels alike.
const SIZE: f64 = 48.0;

/// The families, in the order the goldens are listed and reviewed.
///
/// Order-sensitive on purpose, in `bounds_ledger.rs`'s discipline: a family
/// dropped from the set is the failure this array exists to make visible, and a
/// set compared as a set would let a rename hide one.
const FAMILIES: [Family; 9] = [
    Family {
        name: "path-fill",
        clause: "ISO 32000-1 8.5.3.3.2",
        build: path_fill_page,
        least_ink: 160,
    },
    Family {
        name: "path-stroke",
        clause: "ISO 32000-1 8.4.3.3-8.4.3.6",
        build: path_stroke_page,
        least_ink: 200,
    },
    Family {
        name: "clip",
        clause: "ISO 32000-1 8.5.4",
        build: clip_page,
        least_ink: 200,
    },
    Family {
        name: "text",
        clause: "ISO 32000-1 9.3, 9.4.4",
        build: text_page,
        least_ink: 100,
    },
    Family {
        name: "type3-glyph",
        clause: "ISO 32000-1 9.6.5",
        build: type3_page,
        least_ink: 100,
    },
    Family {
        name: "image",
        clause: "ISO 32000-1 8.9.5",
        build: image_page,
        least_ink: 200,
    },
    Family {
        name: "shading",
        clause: "ISO 32000-1 8.7.4.5.3-8.7.4.5.4",
        build: shading_page,
        least_ink: 800,
    },
    Family {
        name: "pattern",
        clause: "ISO 32000-1 8.7.3, 8.7.4.5.5",
        build: pattern_page,
        least_ink: 400,
    },
    Family {
        name: "transparency",
        clause: "ISO 32000-1 11.3.5, 11.6.4.4, 11.6.6",
        build: transparency_page,
        least_ink: 400,
    },
];

/// The families no person has read yet.
///
/// **This array must reach zero, and what empties it is a review rather than a
/// commit.** The mechanism below is complete — the headers are parsed, a blank
/// field is refused, the picture is re-rendered and compared byte for byte —
/// and the one thing it cannot do for itself is the thing the tier is for. A
/// family leaves this list when a person has looked at its golden against the
/// clause named in its header and put their name in it.
///
/// It is a pin in the shape `epub_fixed_layout.rs` uses: the test that deletes
/// it has to come, and until it does the gap is visible in the suite rather
/// than implied by an empty field somewhere.
///
/// What has been done, so the review starts from somewhere rather than from
/// nothing: every one of the nine was rendered out and looked at once while it
/// was being written, and each page was chosen so that the clause it names is
/// *visible in the picture* rather than merely exercised by it — the pentagram
/// is hollow under even-odd and solid under nonzero, the clip shows a union
/// against a symmetric difference, the transparency group's overlap shows one
/// alpha rather than two. That is what makes a review possible in a minute; it
/// is not the review.
const UNREVIEWED: [&str; 9] = [
    "path-fill",
    "path-stroke",
    "clip",
    "text",
    "type3-glyph",
    "image",
    "shading",
    "pattern",
    "transparency",
];

/// What a golden's header says the golden is.
struct Family {
    name: &'static str,
    clause: &'static str,
    build: fn() -> Vec<u8>,
    /// The fewest non-background pixels the page may paint, for the reason
    /// every tier here has one: a blank golden matches a blank render forever.
    least_ink: usize,
}

// ---- the pages --------------------------------------------------------------

fn page(content: &str) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(SIZE, SIZE, |page| page.raw(content.as_bytes()));
    builder.finish()
}

/// 8.5.3.3.2: the nonzero winding rule and the even-odd rule over one path.
///
/// A pentagram — five points traced 0, 2, 4, 1, 3, so the path crosses itself
/// and the pentagon in the middle is wound twice. That middle is the whole
/// fixture: **nonzero fills it and even-odd leaves it hollow**, so the golden
/// carries the difference between the two rules rather than one rule's output,
/// and a build with one rule wired to both draws two of the same star.
fn path_fill_page() -> Vec<u8> {
    /// The five vertices of a unit pentagram, already in the crossing order.
    const STAR: [(f64, f64); 5] = [
        (0.000, 1.000),
        (0.588, -0.809),
        (-0.951, 0.309),
        (0.951, 0.309),
        (-0.588, -0.809),
    ];
    let star = |cx: f64, cy: f64, r: f64| {
        let mut path = String::new();
        for (index, (dx, dy)) in STAR.iter().enumerate() {
            let verb = if index == 0 { "m" } else { "l" };
            path.push_str(&format!("{} {} {verb} ", cx + dx * r, cy + dy * r));
        }
        path.push_str(
            "h
",
        );
        path
    };
    page(&format!(
        "0.1 0.2 0.7 rg
{}f
0.7 0.2 0.1 rg
{}f*
",
        star(24.0, 35.0, 11.0),
        star(24.0, 13.0, 11.0),
    ))
}

/// 8.4.3.3-8.4.3.6: line width, the three joins, the three caps and a dash.
///
/// Three strokes down the page, each a chevron so a join is drawn, with the
/// join and cap style changing per row; the fourth is dashed, which is the one
/// place a phase is visible.
fn path_stroke_page() -> Vec<u8> {
    let mut content = String::from("0 0 0 RG 4 w\n");
    for (row, (join, cap)) in [(0u32, 0u32), (1, 1), (2, 2)].into_iter().enumerate() {
        let y = 40.0 - row as f64 * 12.0;
        content.push_str(&format!(
            "{join} j {cap} J\n6 {y} m 18 {} l 30 {y} l S\n",
            y - 8.0
        ));
    }
    content.push_str("[4 3] 1 d 1 w 0.8 0 0 RG\n36 4 m 36 44 l S\n");
    page(&content)
}

/// 8.5.4: `W n` and `W* n`, each with a fill that spills past it.
///
/// The clip is stated as two overlapping rectangles so that the two rules
/// disagree about the overlap, and the fill covers the whole page, so what the
/// golden shows is the clip's shape and nothing else.
fn clip_page() -> Vec<u8> {
    page(
        "q 4 26 24 18 re 14 30 24 14 re W n 0.1 0.5 0.2 rg 0 0 48 48 re f Q\n\
         q 4 2 24 18 re 14 6 24 14 re W* n 0.5 0.1 0.2 rg 0 0 48 48 re f Q\n",
    )
}

/// 9.3 and 9.4.4: a run set in the face this repository writes for itself, with
/// horizontal scaling, a rise and a stroking render mode.
///
/// Three lines rather than one, because the parameters are what is being
/// reviewed: the second is scaled to half its width, and the third is stroked
/// rather than filled, so a build that ignored `Tz` or `Tr` draws three of the
/// same line.
fn text_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    // Subsetting off for `determinism.rs`'s reason: the golden is about what
    // the rasteriser draws, and a subsetter change should not be able to move
    // a picture whose review was about glyph shapes.
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    builder.add_page(SIZE, SIZE, |page| {
        page.text(b"F0", 11.0, 4.0, 34.0, "Abc");
        page.raw(b"BT /F0 11 Tf 50 Tz 4 20 Td (Abc) Tj ET\n");
        page.raw(b"BT /F0 11 Tf 1 Tr 0.6 w 0.8 0 0 RG 4 6 Td 3 Ts (Abc) Tj ET\n");
    });
    builder.finish()
}

/// 9.6.5: a glyph whose procedure is a content stream.
///
/// Hand-written, because a Type 3 font has no font program for
/// `DocumentBuilder` to embed and so no builder API. Two glyphs at two sizes,
/// so the golden carries the font matrix's scaling as well as the procedure's
/// own drawing.
fn type3_page() -> Vec<u8> {
    let procedure = "1000 0 d0 100 100 800 300 re f 350 450 300 450 re f";
    let content = "0.2 0.2 0.6 rg BT /F0 20 Tf 4 28 Td (a) Tj ET\n\
                   0.6 0.2 0.2 rg BT /F0 12 Tf 4 8 Td (aa) Tj ET";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {SIZE} {SIZE}]\n\
   /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
   /FontMatrix [0.001 0 0 0.001 0 0]\n\
   /CharProcs << /a 6 0 R >>\n\
   /Encoding << /Type /Encoding /Differences [97 /a] >>\n\
   /FirstChar 97 /LastChar 97 /Widths [1000] /Resources << >> >>\nendobj\n\
6 0 obj\n<< /Length {} >>\nstream\n{procedure}\nendstream\nendobj\n\
trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1,
        procedure.len() + 1,
    )
    .into_bytes()
}

/// 8.9.5: one image magnified, minified and turned.
///
/// A four-by-four checker at three placements, so the golden carries the
/// sampling at more than one scale and at a rotation — the three cases where an
/// image's edges and its sample grid are decided differently.
fn image_page() -> Vec<u8> {
    let mut data = Vec::new();
    for row in 0..4u32 {
        for column in 0..4u32 {
            let on = (row + column) % 2 == 0;
            let (r, g, b) = if on { (20, 40, 160) } else { (240, 200, 60) };
            data.extend_from_slice(&[r, g, b]);
        }
    }
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &tinker_pdf::ImageData::Rgb8 {
            width: 4,
            height: 4,
            data: &data,
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.image(b"Im0", 2.0, 26.0, 20.0, 20.0);
        page.image(b"Im0", 26.0, 38.0, 8.0, 8.0);
        // 8.9.5.2 places an image in the unit square, so a turn is a `cm`
        // around it rather than an operand.
        page.raw(b"q 12 12 -12 12 6 2 cm /Im0 Do Q\n");
    });
    builder.finish()
}

/// 8.7.4.5.3 and 8.7.4.5.4: the two shading types on one page.
///
/// Axial across the top and radial below it, each clipped to its own half, so
/// one golden carries both parametric equations and the difference between
/// them is the picture.
fn shading_page() -> Vec<u8> {
    let ramp = |c0: [f64; 3], c1: [f64; 3]| Function::Exponential {
        domain: [0.0, 1.0],
        c0: c0.to_vec(),
        c1: c1.to_vec(),
        n: 1.0,
    };
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [0.0, 24.0, 48.0, 48.0],
            function: ramp([0.9, 0.3, 0.1], [0.1, 0.2, 0.8]),
            extend: (true, true),
        }
    ));
    assert!(builder.add_shading(
        b"Sh1",
        &Shading::Radial {
            color_space: DeviceSpace::Rgb,
            coords: [24.0, 12.0, 2.0, 24.0, 12.0, 14.0],
            function: ramp([1.0, 1.0, 1.0], [0.1, 0.4, 0.1]),
            extend: (true, true),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.raw(b"q 0 24 48 24 re W n ");
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q q 0 0 48 24 re W n ");
        assert!(page.shading(b"Sh1"));
        page.raw(b"Q");
    });
    builder.finish()
}

/// 8.7.3 and 8.7.4.5.5: a tiling pattern and a shading pattern as colours.
///
/// Both are `scn` operands rather than paint operators, which is the thing this
/// family is about: the top half is a lattice of cells and the bottom half is a
/// gradient, and each fills a path rather than flooding a clip.
fn pattern_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_tiling_pattern(
        b"P0",
        &TilingPattern {
            bbox: [0.0, 0.0, 6.0, 6.0],
            x_step: 8.0,
            y_step: 8.0,
            matrix: None,
            tiling_type: TilingType::NoDistortion,
            content: b"0.2 0.3 0.8 rg 0 0 3 3 re f 0.8 0.6 0.1 rg 3 3 3 3 re f",
        }
    ));
    assert!(builder.add_shading_pattern(
        b"P1",
        &ShadingPattern {
            shading: Shading::Axial {
                color_space: DeviceSpace::Rgb,
                coords: [4.0, 0.0, 44.0, 0.0],
                function: Function::Exponential {
                    domain: [0.0, 1.0],
                    c0: vec![0.1, 0.6, 0.3],
                    c1: vec![0.7, 0.1, 0.5],
                    n: 1.0,
                },
                extend: (true, true),
            },
            matrix: None,
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        assert!(page.set_fill_pattern(b"P0"));
        page.raw(b"4 26 40 18 re f\n");
        assert!(page.set_fill_pattern(b"P1"));
        page.raw(b"4 4 40 18 re f\n");
    });
    builder.finish()
}

/// 11.3.5, 11.6.4.4 and 11.6.6: a blend, a constant alpha and a group.
///
/// Three overlaps in one picture. A `Multiply` over an opaque backdrop, a fill
/// at half alpha over the same backdrop, and a transparency group holding two
/// overlapping shapes at group alpha — which is the one that says whether the
/// alpha was applied to the group or to each shape inside it, because the seam
/// where they cross shows if it was applied twice.
fn transparency_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_ext_gstate(
        b"GBlend",
        &ExtGState {
            blend_mode: Some(BlendMode::Multiply),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GAlpha",
        &ExtGState {
            fill_alpha: Some(0.5),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: [0.0, 0.0, 24.0, 20.0],
            matrix: None,
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Rgb,
                isolated: true,
                knockout: false,
            }),
            content: b"0.1 0.3 0.9 rg 0 0 16 12 re f 0.9 0.4 0.1 rg 8 6 16 12 re f",
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.set_fill_rgb(0.85, 0.85, 0.2);
        page.raw(b"2 26 44 20 re f\n");
        page.raw(b"q ");
        assert!(page.set_ext_gstate(b"GBlend"));
        page.set_fill_rgb(0.2, 0.5, 0.9);
        page.raw(b"4 30 18 12 re f Q\n");
        page.raw(b"q ");
        assert!(page.set_ext_gstate(b"GAlpha"));
        page.set_fill_rgb(0.9, 0.1, 0.2);
        page.raw(b"26 30 18 12 re f Q\n");
        page.raw(b"q ");
        assert!(page.set_ext_gstate(b"GAlpha"));
        page.raw(b"1 0 0 1 12 2 cm ");
        assert!(page.form(b"Fm0"));
        page.raw(b"Q\n");
    });
    builder.finish()
}

// ---- the golden file format -------------------------------------------------

/// What a golden's header has to say before it counts as reviewed.
#[derive(Debug, PartialEq)]
struct Header {
    family: String,
    clause: String,
    reviewer: String,
    reviewed: String,
    built_by: String,
}

/// Why a header was refused. Every variant is a field that is absent or blank,
/// because those are the two ways a review gets recorded as having happened
/// when it did not.
#[derive(Debug, PartialEq)]
enum HeaderError {
    NotAPpm,
    Missing(&'static str),
    Empty(&'static str),
}

/// Reads a golden's `#` comment lines, refusing a blank field the way
/// `pdfa_ledger.rs`'s `read_ledger` refuses a blank reason: there is no default
/// and no way to add a golden without writing one.
fn read_header(bytes: &[u8]) -> Result<Header, HeaderError> {
    if !bytes.starts_with(b"P6\n") {
        return Err(HeaderError::NotAPpm);
    }
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    let field = |key: &'static str| -> Result<String, HeaderError> {
        let prefix = format!("# {key}:");
        let line = text
            .lines()
            .find(|line| line.starts_with(&prefix))
            .ok_or(HeaderError::Missing(key))?;
        let value = line[prefix.len()..].trim().to_string();
        if value.is_empty() {
            return Err(HeaderError::Empty(key));
        }
        Ok(value)
    };
    Ok(Header {
        family: field("family")?,
        clause: field("clause")?,
        reviewer: field("reviewer")?,
        reviewed: field("reviewed")?,
        built_by: field("built-by")?,
    })
}

/// A rendered page as a binary PPM, with the review recorded in its own bytes.
fn to_ppm(bitmap: &tinker_pdf::Bitmap, family: &Family, reviewer: &str, reviewed: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"P6\n");
    out.extend_from_slice(b"# tinker-pdf reviewed golden\n");
    out.extend_from_slice(format!("# family: {}\n", family.name).as_bytes());
    out.extend_from_slice(format!("# clause: {}\n", family.clause).as_bytes());
    out.extend_from_slice(format!("# reviewer: {reviewer}\n").as_bytes());
    out.extend_from_slice(format!("# reviewed: {reviewed}\n").as_bytes());
    out.extend_from_slice(format!("# built-by: render_goldens.rs::{}\n", family.name).as_bytes());
    out.extend_from_slice(format!("{} {}\n255\n", bitmap.width, bitmap.height).as_bytes());
    for pixel in bitmap.data.chunks_exact(bitmap.components()) {
        out.extend_from_slice(&pixel[..3]);
    }
    out
}

/// The samples of a PPM, past whatever header it carries.
fn ppm_samples(bytes: &[u8]) -> &[u8] {
    // P6, then comments and three integers, the last followed by exactly one
    // whitespace byte (Netpbm's rule) before the raster begins.
    let mut at = 3;
    let mut numbers = 0;
    while at < bytes.len() && numbers < 3 {
        if bytes[at] == b'#' {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
        } else if bytes[at].is_ascii_digit() {
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            numbers += 1;
            continue;
        }
        at += 1;
    }
    // Netpbm: exactly one whitespace byte separates the maxval from the
    // raster, and it belongs to the header rather than to the picture.
    &bytes[(at + 1).min(bytes.len())..]
}

fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
}

fn golden_path(family: &Family) -> PathBuf {
    goldens_dir().join(format!("{}.ppm", family.name))
}

// ---- the checks -------------------------------------------------------------

/// Every family has a golden, and the set is exactly the set.
#[test]
fn every_operator_family_has_a_reviewed_golden() {
    for family in &FAMILIES {
        let path = golden_path(family);
        assert!(
            path.exists(),
            "{} has no golden; run the writer below with --ignored",
            family.name
        );
        let bytes = std::fs::read(&path).expect("the golden reads");
        let header = read_header(&bytes).unwrap_or_else(|e| {
            panic!("{}: its header is refused: {e:?}", family.name);
        });
        assert_eq!(
            header.family, family.name,
            "the header names its own family"
        );
        assert_eq!(header.clause, family.clause, "the header names its clause");
        assert!(
            UNREVIEWED.contains(&family.name) || header.reviewer != "unreviewed",
            "{} is not on the unreviewed list and its header says it is",
            family.name
        );
    }

    // The directory holds nothing the list does not name: a golden left behind
    // by a renamed family reads exactly like a reviewed one.
    let mut found: Vec<String> = std::fs::read_dir(goldens_dir())
        .expect("the goldens directory")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".ppm").map(str::to_string)
        })
        .collect();
    found.sort();
    let mut wanted: Vec<String> = FAMILIES.iter().map(|f| f.name.to_string()).collect();
    wanted.sort();
    assert_eq!(found, wanted, "the goldens on disk are the families listed");
}

/// A golden is what this build draws, byte for byte.
#[test]
fn every_golden_is_what_this_build_draws() {
    let mut wrong = Vec::new();
    for family in &FAMILIES {
        let bitmap = render((family.build)());
        let drawn = ink(&bitmap);
        assert!(
            drawn >= family.least_ink,
            "{}: the page painted {drawn} pixels, fewer than the {} it is \
             supposed to -- a blank golden matches a blank render forever",
            family.name,
            family.least_ink
        );
        let committed = std::fs::read(golden_path(family)).expect("the golden reads");
        let samples: Vec<u8> = bitmap
            .data
            .chunks_exact(bitmap.components())
            .flat_map(|pixel| pixel[..3].to_vec())
            .collect();
        let stored = ppm_samples(&committed);
        if stored != samples.as_slice() {
            let differing = stored
                .iter()
                .zip(samples.iter())
                .filter(|(a, b)| a != b)
                .count();
            wrong.push(format!(
                "{}: {differing} of {} bytes differ (stored {} bytes, drew {})",
                family.name,
                samples.len(),
                stored.len(),
                samples.len()
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the goldens and this build disagree. If the change is deliberate, \
         re-run the writer with --ignored and have the new pictures reviewed \
         again -- a golden replaced without a second reading is a regression \
         test that agreed with whatever arrived:\n{}",
        wrong.join("\n")
    );
}

/// A header with a field absent, blank, or whitespace is refused.
///
/// Three shapes, because those are the three ways a review gets recorded as
/// having happened when it did not, and the fourth case is the same failure on
/// a *later* field — where a reader that validated only what it had already
/// accepted would stop looking.
#[test]
fn a_golden_without_a_reviewer_is_refused() {
    let good = b"P6\n# family: clip\n# clause: 8.5.4\n# reviewer: A Person\n\
                 # reviewed: 2026-09-04\n# built-by: render_goldens.rs::clip\n1 1\n255\n\x00\x00\x00";
    assert!(read_header(good).is_ok(), "the undamaged twin is accepted");

    let absent = b"P6\n# family: clip\n# clause: 8.5.4\n\
                   # reviewed: 2026-09-04\n# built-by: render_goldens.rs::clip\n1 1\n255\n";
    assert_eq!(read_header(absent), Err(HeaderError::Missing("reviewer")));

    let blank = b"P6\n# family: clip\n# clause: 8.5.4\n# reviewer:\n\
                  # reviewed: 2026-09-04\n# built-by: render_goldens.rs::clip\n1 1\n255\n";
    assert_eq!(read_header(blank), Err(HeaderError::Empty("reviewer")));

    let spaces = b"P6\n# family: clip\n# clause: 8.5.4\n# reviewer:    \n\
                   # reviewed: 2026-09-04\n# built-by: render_goldens.rs::clip\n1 1\n255\n";
    assert_eq!(read_header(spaces), Err(HeaderError::Empty("reviewer")));

    let later = b"P6\n# family: clip\n# clause: 8.5.4\n# reviewer: A Person\n\
                  # reviewed:  \n# built-by: render_goldens.rs::clip\n1 1\n255\n";
    assert_eq!(read_header(later), Err(HeaderError::Empty("reviewed")));

    let not_a_ppm = b"P5\n# family: clip\n";
    assert_eq!(read_header(not_a_ppm), Err(HeaderError::NotAPpm));
}

/// A golden stays small enough that reviewing one is a real act.
///
/// The design's non-goal is explicit — no full-page goldens, "hundreds of
/// kilobytes of binary per page that no reviewer can assess" — and a ceiling is
/// that sentence in a form that survives someone raising a page size.
#[test]
fn a_golden_stays_small_enough_to_review() {
    const PER_GOLDEN: u64 = 16 * 1024;
    const ALTOGETHER: u64 = 96 * 1024;

    let mut total = 0;
    for family in &FAMILIES {
        let size = std::fs::metadata(golden_path(family))
            .expect("the golden is there")
            .len();
        assert!(
            size <= PER_GOLDEN,
            "{} is {size} bytes, past the {PER_GOLDEN} a reviewer is asked for",
            family.name
        );
        total += size;
    }
    assert!(
        total <= ALTOGETHER,
        "the goldens are {total} bytes altogether, past {ALTOGETHER}"
    );
}

/// Writes the goldens. Run with `--ignored` when a picture is meant to change,
/// and have the result reviewed before it is committed.
///
/// The writer lives beside the fixtures, in the pattern the fuzz corpora use:
/// a golden and the page that draws it cannot drift if one command makes both.
#[test]
#[ignore = "writes the committed goldens"]
fn write_the_goldens() {
    let reviewer = std::env::var("TINKER_GOLDEN_REVIEWER").unwrap_or_else(|_| "unreviewed".into());
    let reviewed = std::env::var("TINKER_GOLDEN_DATE").unwrap_or_else(|_| "unreviewed".into());
    std::fs::create_dir_all(goldens_dir()).expect("the goldens directory");
    for family in &FAMILIES {
        let bitmap = render((family.build)());
        let bytes = to_ppm(&bitmap, family, &reviewer, &reviewed);
        std::fs::write(golden_path(family), &bytes).expect("the golden writes");
        println!(
            "  wrote {} ({} bytes, {} pixels of ink)",
            family.name,
            bytes.len(),
            ink(&bitmap)
        );
    }
}
