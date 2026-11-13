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

use tinker_pdf_font::hint::{Face, Hinted, Program, Unhinted};
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
/// paste.
const HINTED: [(&str, &str); 12] = [
    ("LiberationMono-Bold", ""),
    ("LiberationMono-BoldItalic", ""),
    ("LiberationMono-Italic", ""),
    ("LiberationMono-Regular", ""),
    ("LiberationSans-Bold", ""),
    ("LiberationSans-BoldItalic", ""),
    ("LiberationSans-Italic", ""),
    ("LiberationSans-Regular", ""),
    ("LiberationSerif-Bold", ""),
    ("LiberationSerif-BoldItalic", ""),
    ("LiberationSerif-Italic", ""),
    ("LiberationSerif-Regular", ""),
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

/// Each face's record, a face to a thread where the target starts threads —
/// the faces are independent, so the order they finish in moves nothing —
/// and one after another on a wasm target, which starts none.
fn records() -> Vec<(String, u64)> {
    if cfg!(target_family = "wasm") {
        return FACES.iter().map(|(_, bytes)| hint_face(bytes)).collect();
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = FACES
            .iter()
            .map(|(_, bytes)| scope.spawn(move || hint_face(bytes)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a face hints without a panic"))
            .collect()
    })
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
