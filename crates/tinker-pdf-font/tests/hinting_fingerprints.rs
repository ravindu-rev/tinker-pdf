//! Ruling 4 for hinting: every glyph of the committed faces hints to the same
//! points on every target (`docs/design/hinting.md`, FT-04b.10).
//!
//! Each of the twelve Liberation faces in `data/liberation/` is read with
//! `include_bytes!`, so the test needs no feature and no filesystem — the
//! fourth target is `wasm32-wasip1` under wasmtime — and every one of its
//! glyphs is hinted at each whole size from 8 to 16 pixels an em and at 24,
//! 32, 48 and 72: the sizes small text is read at, and the larger ones the
//! hinting design names. What each glyph hints to — every segment of its
//! outline, its four phantom points, the dropout state its programs left and
//! whether a program ran, or why it is unhinted — is folded into one digest a
//! face, held to the committed value beside the count of glyphs at all sizes
//! that ran a program and of those whose program failed. A face whose
//! programs stopped reaching the interpreter would hint every glyph to its
//! scaled points, which is as stable as anything, so each face is first held
//! to having run programs at all.
//!
//! # When it fails
//!
//! As `crates/tinker-pdf/tests/determinism.rs` says of its fingerprints: a
//! deliberate change to the interpreter moves a digest, and the commit that
//! moves it says what moved and why; two targets that disagree are a
//! determinism bug, and the table is not the thing to change. A move in the
//! failure count is a rule this engine refuses that a committed face breaks,
//! or an interpreter that reads the face wrong: a finding either way.
//!
//! # Why this is not SHA-256
//!
//! This crate does not depend on `tinker-pdf-crypto`, and the shaper's
//! fingerprints give the reason not to buy the edge for a test: the digest is
//! FNV-1a, integer arithmetic over a canonical byte stream, which is what a
//! cross-target check needs.

use tinker_pdf_font::glyf;
use tinker_pdf_font::hint::{Error, Face, Hinted, Program, Unhinted};
use tinker_pdf_font::{Outline, Segment, Sfnt};

/// The twelve committed faces, by name.
const FACES: [(&str, &[u8]); 12] = [
    (
        "LiberationMono-Bold",
        include_bytes!("../data/liberation/LiberationMono-Bold.ttf"),
    ),
    (
        "LiberationMono-BoldItalic",
        include_bytes!("../data/liberation/LiberationMono-BoldItalic.ttf"),
    ),
    (
        "LiberationMono-Italic",
        include_bytes!("../data/liberation/LiberationMono-Italic.ttf"),
    ),
    (
        "LiberationMono-Regular",
        include_bytes!("../data/liberation/LiberationMono-Regular.ttf"),
    ),
    (
        "LiberationSans-Bold",
        include_bytes!("../data/liberation/LiberationSans-Bold.ttf"),
    ),
    (
        "LiberationSans-BoldItalic",
        include_bytes!("../data/liberation/LiberationSans-BoldItalic.ttf"),
    ),
    (
        "LiberationSans-Italic",
        include_bytes!("../data/liberation/LiberationSans-Italic.ttf"),
    ),
    (
        "LiberationSans-Regular",
        include_bytes!("../data/liberation/LiberationSans-Regular.ttf"),
    ),
    (
        "LiberationSerif-Bold",
        include_bytes!("../data/liberation/LiberationSerif-Bold.ttf"),
    ),
    (
        "LiberationSerif-BoldItalic",
        include_bytes!("../data/liberation/LiberationSerif-BoldItalic.ttf"),
    ),
    (
        "LiberationSerif-Italic",
        include_bytes!("../data/liberation/LiberationSerif-Italic.ttf"),
    ),
    (
        "LiberationSerif-Regular",
        include_bytes!("../data/liberation/LiberationSerif-Regular.ttf"),
    ),
];

/// The sizes every glyph is hinted at, in whole pixels per em.
const SIZES: [i32; 13] = [8, 9, 10, 11, 12, 13, 14, 15, 16, 24, 32, 48, 72];

/// Each face's committed record: its digest, then how many of its glyphs at
/// all the sizes ran a program, then how many failed. Every value is a run's
/// output, recorded with the targets agreeing; a mismatch prints the lines to
/// paste. Each Liberation Mono face's failures are [`REFUSED`]'s glyph at the
/// twelve of these sizes its `gasp` hints.
const HINTED: [(&str, &str); 12] = [
    ("LiberationMono-Bold", "2b143d26f8945b04 28668 12"),
    ("LiberationMono-BoldItalic", "31d2277cef7bc5ae 28644 12"),
    ("LiberationMono-Italic", "3593872ab6481631 28656 12"),
    ("LiberationMono-Regular", "3f16b23df6ce9cfe 28680 12"),
    ("LiberationSans-Bold", "4f11da73897337f5 30720 0"),
    ("LiberationSans-BoldItalic", "40ba616541ecf8fc 30600 0"),
    ("LiberationSans-Italic", "2b85704b3694b92a 30672 0"),
    ("LiberationSans-Regular", "58f301b4960188da 30696 0"),
    ("LiberationSerif-Bold", "5e9cd6571b2b0943 30720 0"),
    ("LiberationSerif-BoldItalic", "591cf8e328c583d2 30744 0"),
    ("LiberationSerif-Italic", "fb235872e05b0c4e 30744 0"),
    ("LiberationSerif-Regular", "f006b41fda311e68 25650 0"),
];

/// The programs of the committed faces this interpreter refuses, each its
/// face, its glyph and what the program broke: refused at every size the face
/// is hinted at, and nothing else refused at any.
///
/// One program, `uni266B`'s in each Liberation Mono face, and it is malformed
/// for its glyph. The glyph has 28 contour points, and "if the entire set of
/// contours for a glyph requires “n” points (i.e., contour points numbered from
/// 0 to n-1), then the scaler will add points n, n+1, n+2, and n+3"
/// (*Instructing TrueType Glyphs*, "Phantom points"; Apple's manual adds two),
/// so its zone is points 0 to 31. Its program sets rp1 to 33 and rp2 to 32 and
/// then interpolates between them, and `IP` "uses zp0 with rp1, zp1 with rp2",
/// both the glyph's zone here: neither point exists. The references fit an
/// outline of 32 points, on which 32 and 33 are the left sidebearing point and
/// the advance width point an interpolation across the glyph reads, and on
/// this one the program shifts the four phantom points as it would contour
/// points before it gets there.
/// Named rather than counted, so a refusal that stopped, or one that started,
/// fails here by name.
const REFUSED: [(&str, u16, Error); 4] = [
    ("LiberationMono-Bold", 2215, Error::PointIndex(33)),
    ("LiberationMono-BoldItalic", 2215, Error::PointIndex(33)),
    ("LiberationMono-Italic", 2215, Error::PointIndex(33)),
    ("LiberationMono-Regular", 2215, Error::PointIndex(33)),
];

/// FNV-1a, 64 bits: `u64` multiplication and exclusive-or, the same on every
/// target.
struct Digest(u64);

impl Digest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Digest {
        Digest(Digest::OFFSET)
    }

    fn byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(Digest::PRIME);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_be_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_be_bytes());
    }

    /// A coordinate by its bits: each is a multiple of 1/128, exact in an
    /// `f64`, so its bits are canonical.
    fn f64(&mut self, value: f64) {
        self.bytes(&value.to_bits().to_be_bytes());
    }
}

/// Every segment of `outline`, each its kind and its coordinates.
fn fold_outline(digest: &mut Digest, outline: &Outline) {
    digest.u32(outline.segments.len() as u32);
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                digest.byte(0);
                digest.f64(x);
                digest.f64(y);
            }
            Segment::LineTo { x, y } => {
                digest.byte(1);
                digest.f64(x);
                digest.f64(y);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                digest.byte(2);
                for v in [cx, cy, x, y] {
                    digest.f64(v);
                }
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                digest.byte(3);
                for v in [c1x, c1y, c2x, c2y, x, y] {
                    digest.f64(v);
                }
            }
            Segment::Close => digest.byte(4),
        }
    }
}

/// What one glyph hinted to.
fn fold_hinted(digest: &mut Digest, hinted: &Hinted) {
    digest.byte(1);
    fold_outline(digest, &hinted.outline);
    for point in hinted.phantom {
        digest.i32(point.x);
        digest.i32(point.y);
    }
    match hinted.dropout {
        None => digest.byte(0),
        Some(dropout) => {
            digest.byte(1);
            digest.byte(u8::from(dropout.smart));
            digest.byte(u8::from(dropout.stubs));
        }
    }
    digest.byte(u8::from(hinted.ran_program));
}

/// Why a glyph, a size or a face is unhinted: the reason, and for a failure
/// its program, its error and its steps.
fn fold_unhinted(digest: &mut Digest, why: &Unhinted) {
    digest.byte(0);
    digest.bytes(format!("{why:?}").as_bytes());
}

/// One face hinted at every size: its record as [`HINTED`] holds it, and how
/// many glyphs ran a program.
fn hint_face(bytes: &[u8]) -> (String, u64) {
    let sfnt = Sfnt::parse(bytes).expect("a committed face");
    let glyphs = sfnt.glyph_count().expect("a glyph count");
    let mut digest = Digest::new();
    let (mut ran, mut failed) = (0u64, 0u64);
    match Face::new(&sfnt, None) {
        Err(why) => fold_unhinted(&mut digest, &why),
        Ok(face) => {
            for pixels in SIZES {
                digest.i32(pixels);
                let ppem = pixels * 64;
                let at = match face.at_size(ppem, ppem, false, None) {
                    Ok(at) => at,
                    Err(why) => {
                        fold_unhinted(&mut digest, &why);
                        continue;
                    }
                };
                for glyph in 0..glyphs {
                    digest.u32(u32::from(glyph));
                    match at.glyph(&sfnt, glyph, None) {
                        Ok(hinted) => {
                            ran += u64::from(hinted.ran_program);
                            fold_hinted(&mut digest, &hinted);
                        }
                        Err(why) => {
                            if matches!(
                                why,
                                Unhinted::Failed {
                                    program: Program::Glyph,
                                    ..
                                }
                            ) {
                                failed += 1;
                            }
                            fold_unhinted(&mut digest, &why);
                        }
                    }
                }
            }
        }
    }
    (format!("{:016x} {ran} {failed}", digest.0), ran)
}

/// `each` over every face, in [`FACES`]' order: a face to a thread where the
/// target starts threads — the faces are independent, so the order they
/// finish in moves nothing — and one after another on a wasm target, which
/// starts none.
fn per_face<T: Send>(each: fn(&[u8]) -> T) -> Vec<T> {
    if cfg!(target_family = "wasm") {
        return FACES.iter().map(|(_, bytes)| each(bytes)).collect();
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = FACES
            .iter()
            .map(|(_, bytes)| scope.spawn(move || each(bytes)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a face hints without a panic"))
            .collect()
    })
}

/// Each face's record.
fn records() -> Vec<(String, u64)> {
    per_face(hint_face)
}

/// One face's refused glyph programs, as [`refusals`] finds them.
struct Refusals {
    /// How many of [`SIZES`] the face is hinted at.
    sizes: usize,
    /// Each glyph whose program failed, what it broke, and at how many of
    /// those sizes.
    failed: Vec<(u16, Error, usize)>,
}

/// One face's refused glyph programs at every size. A face whose `fpgm`
/// fails, or a size whose `prep` does, answers why instead.
fn refusals(bytes: &[u8]) -> Result<Refusals, Unhinted> {
    let sfnt = Sfnt::parse(bytes).expect("a committed face");
    let glyphs = sfnt.glyph_count().expect("a glyph count");
    let face = Face::new(&sfnt, None)?;
    let mut sizes = 0usize;
    let mut failed: Vec<(u16, Error, usize)> = Vec::new();
    for pixels in SIZES {
        let ppem = pixels * 64;
        let at = match face.at_size(ppem, ppem, false, None) {
            Ok(at) => at,
            Err(Unhinted::NotAtThisSize) => continue,
            Err(why) => return Err(why),
        };
        sizes += 1;
        for glyph in 0..glyphs {
            if let Err(Unhinted::Failed { error, .. }) = at.glyph(&sfnt, glyph, None) {
                match failed
                    .iter_mut()
                    .find(|(g, e, _)| (*g, *e) == (glyph, error))
                {
                    Some((_, _, at_sizes)) => *at_sizes += 1,
                    None => failed.push((glyph, error, 1)),
                }
            }
        }
    }
    Ok(Refusals { sizes, failed })
}

#[test]
fn hinted_outlines_of_the_committed_faces_are_stable_across_targets() {
    let mut wrong = Vec::new();
    for ((name, _), (record, ran)) in FACES.iter().zip(records()) {
        assert!(
            ran > 0,
            "{name} ran no program at any size, so its digest is of its scaled \
             points and says nothing about the interpreter: {record}"
        );
        let committed = HINTED
            .iter()
            .find(|(face, _)| face == name)
            .map(|(_, value)| *value)
            .unwrap_or_default();
        println!("RAN hinting {name}: {record}");
        if record != committed {
            wrong.push(format!("    (\"{name}\", \"{record}\"),"));
        }
    }
    assert!(
        wrong.is_empty(),
        "the hinted outlines do not match the committed records.\n\n\
         If two targets disagree, this is a determinism bug: do not update the \
         table. If the interpreter changed on purpose, say what moved and paste \
         these in:\n\n{}\n",
        wrong.join("\n")
    );
}

/// Every program of the committed faces runs at every size each face is
/// hinted at, its `fpgm`, its `prep` and each glyph's, but for [`REFUSED`]'s,
/// which are refused at every one of those sizes by the rule named there.
#[test]
fn the_committed_faces_refuse_the_programs_named_and_no_others() {
    let mut refused: Vec<(&str, u16, Error)> = Vec::new();
    for ((name, _), found) in FACES.iter().zip(per_face(refusals)) {
        let Refusals { sizes, failed } = found.unwrap_or_else(|why| panic!("{name}: {why}"));
        for (glyph, error, at_sizes) in failed {
            assert_eq!(
                at_sizes, sizes,
                "{name}'s glyph {glyph} is refused at some of the face's {sizes} hinted sizes and \
                 not others: {error:?}"
            );
            refused.push((*name, glyph, error));
        }
    }
    assert_eq!(
        refused, REFUSED,
        "these are the committed faces' refused programs, and no others"
    );

    // And why, for each: its glyph's zone ends at point 31, its fourth phantom
    // point, short of the point the program names.
    for (name, glyph, error) in REFUSED {
        let (_, bytes) = FACES
            .iter()
            .find(|(face, _)| *face == name)
            .expect("a committed face");
        let sfnt = Sfnt::parse(bytes).expect("a committed face");
        let simple = glyf::simple_glyph(&sfnt, glyph).expect("a simple glyph");
        assert_eq!(
            (simple.contour_points(), error),
            (28, Error::PointIndex(33)),
            "{name}'s glyph {glyph}: 28 contour points and four phantom points are points 0 \
             to 31"
        );
    }
}
