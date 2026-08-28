//! Ruling 4 for shaping: the same text shapes to the same integers, on every
//! target.
//!
//! `docs/design/shaping.md`'s milestone 2 asks for *"shaping fingerprints
//! committed and reproduced by the determinism CI legs on all four targets"*,
//! and this is that file. `crates/tinker-pdf/tests/determinism.rs` does the
//! same job for rendered pages and this one is deliberately in its register,
//! including the two things that file learned the hard way:
//!
//! - **A fingerprint of nothing is extremely stable.** That file's `text`
//!   fixture hashed a blank page for months because the face never loaded.
//!   So every corpus here is checked for how much it produced *before* it is
//!   hashed, and a run that stopped shaping fails rather than becoming the
//!   new baseline.
//! - **When one of these fails it means one of two opposite things.** Either
//!   shaping deliberately changed — update the constant in the same commit and
//!   say what moved — or two targets disagree, which is a determinism bug and
//!   the constant is not the thing to change.
//!
//! # Why this is not SHA-256
//!
//! `determinism.rs` hashes with `tinker_pdf_crypto::sha2`, and this crate does
//! not depend on `tinker-pdf-crypto`. Adding the edge to fingerprint six
//! numbers per glyph would be a dependency bought for a test, so the digest
//! below is FNV-1a, written out in nine lines. Nothing here is defending
//! against an adversary choosing the input to collide: it is defending against
//! a `f32` creeping into an advance and rounding differently on aarch64, and
//! for that any avalanching function over the bytes does the job. What matters
//! is that the arithmetic is integer and the input is canonical, and both are
//! visible in this file.
//!
//! # What the corpora are
//!
//! Two, because there are two things to pin. The shaping one drives the
//! committed text-rendering-tests faces through [`Shaper::shape_text`], the
//! entry point a consumer calls, and folds in every field of every glyph. The
//! bidi one drives paragraphs of mixed direction through
//! [`Paragraph::new`] and folds in the levels and the visual order.
//!
//! Both are `include_str!`/`include_bytes!` rather than read from disk,
//! because the fourth target is `wasm32-wasip1` under wasmtime and it has no
//! filesystem unless somebody remembers to grant one.

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{BaseDirection, Paragraph};
use tinker_pdf_shape::Shaper;

/// FNV-1a, 64 bits. Integer, associative over a byte stream, and identical on
/// every target because it is `u64` multiplication and exclusive-or.
struct Digest(u64);

impl Digest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.byte(*byte);
        }
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_be_bytes());
    }

    fn finish(&self) -> String {
        format!("{:016x}", self.0)
    }
}

/// The faces the shaping corpus drives, embedded so the wasm leg can read
/// them.
fn face_bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "TestGSUBOne.otf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestGSUBOne.otf").as_slice()
        }
        "TestGPOSOne.ttf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestGPOSOne.ttf").as_slice()
        }
        "TestGPOSTwo.otf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestGPOSTwo.otf").as_slice()
        }
        "TestGPOSThree.ttf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestGPOSThree.ttf").as_slice()
        }
        "TestShapeEthi.ttf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestShapeEthi.ttf").as_slice()
        }
        "TestCMAP14.otf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestCMAP14.otf").as_slice()
        }
        // The declined sections' faces, and the billion-laughs one, are not
        // fingerprinted: two of them shape to `.notdef` today and one is a
        // budget test whose output is a refusal.
        _ => return None,
    })
}

/// The fixtures the render strings come from.
const FIXTURES: &[&str] = &[
    include_str!("../data/text-rendering-tests/testcases/CMAP-1.html"),
    include_str!("../data/text-rendering-tests/testcases/CMAP-2.html"),
    include_str!("../data/text-rendering-tests/testcases/GSUB-1.html"),
    include_str!("../data/text-rendering-tests/testcases/GSUB-2.html"),
    include_str!("../data/text-rendering-tests/testcases/GPOS-1.html"),
    include_str!("../data/text-rendering-tests/testcases/GPOS-2.html"),
    include_str!("../data/text-rendering-tests/testcases/GPOS-3.html"),
    include_str!("../data/text-rendering-tests/testcases/GPOS-4.html"),
];

/// The paragraphs the bidi corpus resolves.
///
/// Chosen to reach every branch of UAX #9 that has one: a strong right-to-left
/// run, a number inside it (I1's two-level bump), a bracket pair whose
/// contents decide its direction (N0), an isolate, an overflowing embedding,
/// and trailing whitespace (L1).
const PARAGRAPHS: &[&str] = &[
    "hello world",
    "\u{05D0}\u{05D1}\u{05D2}",
    "abc \u{05D0}\u{05D1} 123 def",
    "\u{05D0}\u{05D1} (\u{05D2} 42) \u{05D0}",
    "abc (\u{0627}\u{0628} def) ghi",
    "a\u{2067}bc\u{05D0}\u{2069}d",
    "a\u{202B}\u{05D0}1\u{202C}b   ",
    "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064A}\u{0629} 2026",
    "\u{05D0}\u{05D1}\u{05D2} ",
];

/// The shaping fingerprint, and how many glyphs went into it.
fn shaping() -> (String, usize) {
    let mut digest = Digest::new();
    let mut glyphs = 0usize;
    for fixture in FIXTURES {
        for (font, render) in cases(fixture) {
            let Some(bytes) = face_bytes(&font) else {
                continue;
            };
            let face = Sfnt::parse(bytes).expect("a fixture font is a valid sfnt");
            let shaper = Shaper::new(&face);
            // The font name and the text go into the digest as well, so that a
            // corpus that lost a case is a different fingerprint rather than a
            // shorter one that happens to collide.
            digest.bytes(font.as_bytes());
            digest.bytes(render.as_bytes());
            digest.bytes(&face.units_per_em.to_be_bytes());
            let (_, runs) = shaper.shape_text(&render, BaseDirection::LeftToRight);
            for run in &runs {
                digest.byte(u8::from(run.direction().is_forward()));
                for glyph in run.glyphs() {
                    digest.bytes(&glyph.glyph.to_be_bytes());
                    digest.bytes(&glyph.cluster.to_be_bytes());
                    digest.i32(glyph.x_advance);
                    digest.i32(glyph.y_advance);
                    digest.i32(glyph.x_offset);
                    digest.i32(glyph.y_offset);
                    glyphs += 1;
                }
            }
        }
    }
    (digest.finish(), glyphs)
}

/// The bidi fingerprint, and how many characters went into it.
fn bidi() -> (String, usize) {
    let mut digest = Digest::new();
    let mut characters = 0usize;
    for text in PARAGRAPHS {
        for direction in [
            BaseDirection::Auto,
            BaseDirection::LeftToRight,
            BaseDirection::RightToLeft,
        ] {
            let paragraph = Paragraph::new(text, direction);
            digest.bytes(text.as_bytes());
            digest.byte(paragraph.base_level().number());
            let line = paragraph.line(0..paragraph.len());
            for level in line.levels() {
                digest.byte(level.number());
            }
            for at in line.visual_order() {
                digest.bytes(&u32::try_from(*at).unwrap_or(u32::MAX).to_be_bytes());
                characters += 1;
            }
        }
    }
    (digest.finish(), characters)
}

/// Every `(font, text)` a fixture names.
fn cases(fixture: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = fixture;
    while let Some(at) = rest.find("class=\"expected\"") {
        rest = &rest[at..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if let (Some(font), Some(render)) = (attribute(tag, "ft:font"), attribute(tag, "ft:render"))
        {
            out.push((font, render));
        }
        rest = &rest[end..];
    }
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let at = tag.find(&needle)? + needle.len();
    let rest = &tag[at..];
    let end = rest.find('"')?;
    Some(unescape(&rest[..end]))
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("&#x") {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find(';') else {
            return out + &rest[at..];
        };
        let code = u32::from_str_radix(&rest[at + 3..at + end], 16).expect("a reference");
        out.push(char::from_u32(code).expect("a character"));
        rest = &rest[at + end + 1..];
    }
    out.push_str(rest);
    out
}

/// The committed fingerprints.
///
/// Reproduced by the determinism CI legs on linux, windows, macos and
/// `wasm32-wasip1`. Two targets disagreeing here is a determinism bug and not
/// a reason to move these numbers.
const SHAPING: &str = "5bb275af438f2314";
const BIDI: &str = "d77c9e938eb9c996";

/// The least each corpus may produce before its fingerprint means anything.
///
/// `determinism.rs`'s `least_ink`, transposed: a corpus that shaped nothing
/// hashes perfectly stably on every target and proves nothing at all.
const LEAST_GLYPHS: usize = 90;
const LEAST_CHARACTERS: usize = 240;

#[test]
fn shaping_is_stable_across_targets() {
    let (fingerprint, glyphs) = shaping();
    assert!(
        glyphs >= LEAST_GLYPHS,
        "the shaping corpus produced {glyphs} glyphs, fewer than the \
         {LEAST_GLYPHS} it is supposed to: it is measuring less than it \
         claims, and its fingerprint is not evidence about anything until \
         that is fixed"
    );
    assert_eq!(
        fingerprint, SHAPING,
        "shaping moved. If that was deliberate, update this constant in the \
         same commit and say what changed; if two targets disagree, the \
         constant is not the thing to change"
    );
}

#[test]
fn bidi_is_stable_across_targets() {
    let (fingerprint, characters) = bidi();
    assert!(
        characters >= LEAST_CHARACTERS,
        "the bidi corpus ordered {characters} characters, fewer than the \
         {LEAST_CHARACTERS} it is supposed to"
    );
    assert_eq!(
        fingerprint, BIDI,
        "level resolution moved. If that was deliberate, update this constant \
         in the same commit; if two targets disagree, it is a determinism bug"
    );
}

/// Nothing in the shaped output depends on how many times it was shaped.
///
/// A cheap check for the class of bug a fingerprint alone cannot see: state
/// left over between runs. Shaping the same corpus twice in one process has to
/// give the same answer as shaping it once.
#[test]
fn shaping_twice_gives_the_same_answer() {
    assert_eq!(shaping(), shaping());
    assert_eq!(bidi(), bidi());
}
