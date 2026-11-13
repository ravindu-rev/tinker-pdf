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
//! Three, because there are three things to pin. The shaping one drives the
//! committed text-rendering-tests faces through [`Shaper::shape_text`], the
//! entry point a consumer calls, and folds in every field of every glyph. The
//! bidi one drives paragraphs of mixed direction through
//! [`Paragraph::new`] and folds in the levels and the visual order. The
//! vertical one drives paragraphs of mixed orientation through
//! [`Shaper::shape_text_vertical`] over faces with vertical metrics and
//! features, and folds in each run's direction, how it stands and every
//! field of every glyph (FT-07b.8).
//!
//! The first two are `include_str!`/`include_bytes!` rather than read from
//! disk, and the third's faces are built here byte by byte (`column_face`,
//! which `vertical.rs` shares), because the fourth target is `wasm32-wasip1`
//! under wasmtime and it has no filesystem unless somebody remembers to grant
//! one.

use std::collections::BTreeMap;

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{BaseDirection, Paragraph};
use tinker_pdf_shape::{Orientation, Plans, Shaper, Tag};

// The face assembler `vertical.rs` writes its columns' faces with.
mod column_face;
use column_face::{
    cursive_pos, gdef, layout_table, ligature_subst, lookup, mark_pos, pair_pos, single_pos,
    single_subst, Face,
};

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
        // The one right-to-left face in the corpus, and the only one whose
        // glyphs reach cursive attachment, the joining forms and the
        // direction-dependent half of attachment propagation. Without it a
        // `f32` creeping into a cursive chain would not move any number here.
        "TestShapeAran.ttf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestShapeAran.ttf").as_slice()
        }
        "TestCMAP14.otf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestCMAP14.otf").as_slice()
        }
        // Two Brahmic faces, for the same reason the Arabic one is here: the
        // Universal Shaping Engine's syllables and its reordering pause are
        // reached by no other case in this corpus, so without them a defect in
        // either would move no number.
        "NotoSansBalinese-Regular.ttf" => {
            include_bytes!("../data/text-rendering-tests/fonts/NotoSansBalinese-Regular.ttf")
                .as_slice()
        }
        "TestShapeLana.ttf" => {
            include_bytes!("../data/text-rendering-tests/fonts/TestShapeLana.ttf").as_slice()
        }
        // Kannada, added after milestone 5's last commits found this corpus
        // blind to three of its own behaviours: the reph, the joiner deleted
        // after `GSUB`, and a syllable-final base plus halant appear in no
        // other section here. A fingerprint that cannot see a behaviour cannot
        // report that two targets disagree about it.
        "NotoSansKannada-Regular.ttf" => {
            include_bytes!("../data/text-rendering-tests/fonts/NotoSansKannada-Regular.ttf")
                .as_slice()
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
    include_str!("../data/text-rendering-tests/testcases/SHARAN-1.html"),
    include_str!("../data/text-rendering-tests/testcases/SHBALI-1.html"),
    include_str!("../data/text-rendering-tests/testcases/SHLANA-3.html"),
    include_str!("../data/text-rendering-tests/testcases/SHKNDA-2.html"),
    include_str!("../data/text-rendering-tests/testcases/SHKNDA-3.html"),
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
    shaping_with(None)
}

/// [`shaping`], with every shaper over a face made with that face's entry
/// in `kept` where one is given — what a caller that keeps a face's
/// [`Plans`] does.
fn shaping_with(kept: Option<&BTreeMap<String, Plans>>) -> (String, usize) {
    let mut digest = Digest::new();
    let mut glyphs = 0usize;
    for fixture in FIXTURES {
        for (font, render) in cases(fixture) {
            let Some(bytes) = face_bytes(&font) else {
                continue;
            };
            let face = Sfnt::parse(bytes).expect("a fixture font is a valid sfnt");
            let shaper = Shaper::new(&face);
            let shaper = match kept.and_then(|kept| kept.get(&font)) {
                Some(plans) => shaper.with_plans(plans),
                None => shaper,
            };
            // The font name and the text go into the digest as well, so that a
            // corpus that lost a case is a different fingerprint rather than a
            // shorter one that happens to collide.
            digest.bytes(font.as_bytes());
            digest.bytes(render.as_bytes());
            digest.bytes(&face.units_per_em.to_be_bytes());
            // `Auto` rather than a fixed direction, so that the Arabic
            // fixture resolves to a right-to-left paragraph the way a
            // consumer's would. Every other case in the corpus is Latin,
            // Ethiopic or Han and resolves to base level zero either way, so
            // this changes nothing about them.
            let (_, runs) = shaper.shape_text(&render, BaseDirection::Auto);
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

// --- the vertical corpus ---------------------------------------------------

/// Ideographs upright: 漢, 字, 一 and 二, the ideographic comma and full stop,
/// each with a `vmtx` entry and a `glyf` top of its own; the comma and stop
/// take `vert` forms (glyphs 7, 8) and `vrt2` ones (9, 10), and 一 joins 二
/// down the column under `curs`.
fn vertical_ideographs() -> Face {
    let cmap = [
        ('\u{6F22}', 1),
        ('\u{5B57}', 2),
        ('\u{4E00}', 3),
        ('\u{4E8C}', 4),
        ('\u{3001}', 5),
        ('\u{3002}', 6),
    ];
    let mut face = Face::new(11, &cmap).with_vertical(
        &[
            (1000, 100),
            (1000, 90),
            (980, 120),
            (1000, 110),
            (1100, 60),
            (1000, 700),
            (1000, 650),
            (1000, 120),
            (1000, 130),
            (990, 140),
            (1010, 150),
        ],
        &[0, 820, 830, 760, 840, 150, 160, 790, 800, 770, 780],
    );
    face.advances = vec![
        500, 1000, 1000, 1000, 900, 1000, 1000, 1000, 1000, 1000, 1000,
    ];
    face.gsub = Some(layout_table(
        &[*b"DFLT", *b"hani"],
        &[(*b"vert", vec![0]), (*b"vrt2", vec![1])],
        &[
            lookup(1, 0, &[single_subst(&[(5, 7), (6, 8)])]),
            lookup(1, 0, &[single_subst(&[(5, 9), (6, 10)])]),
        ],
    ));
    face.gpos = Some(layout_table(
        &[*b"DFLT", *b"hani"],
        &[(*b"curs", vec![0])],
        &[lookup(3, 0, &[cursive_pos(3, (520, -100), 4, (450, 900))])],
    ));
    face
}

/// Katakana ス and ト kerned down the column under `vkrn` — a `YAdvance` on
/// the first, a `YPlacement` on the second, and an `XAdvance` a column does
/// not take — and か with the combining voiced sound mark attached under
/// `mark`, `GDEF` naming the mark.
fn vertical_kana() -> Face {
    let cmap = [
        ('\u{30B9}', 1),
        ('\u{30C8}', 2),
        ('\u{304B}', 3),
        ('\u{3099}', 4),
    ];
    let mut face = Face::new(5, &cmap).with_vertical(
        &[
            (1000, 120),
            (1000, 120),
            (1000, 110),
            (1000, 120),
            (1000, 0),
        ],
        &[760, 760, 750, 770, 900],
    );
    face.advances[4] = 600;
    face.gdef = Some(gdef(&[(1, 1), (2, 1), (3, 1), (4, 3)]));
    face.gpos = Some(layout_table(
        &[*b"DFLT", *b"kana"],
        &[(*b"vkrn", vec![0]), (*b"mark", vec![1])],
        &[
            lookup(
                2,
                0,
                &[pair_pos(1, 2, 0x000C, &[-300, -200], 0x0002, &[-40])],
            ),
            lookup(4, 0, &[mark_pos(4, (300, 900), 3, (900, 800))]),
        ],
    ));
    face
}

/// A paragraph's other orientations: an ideograph and a full-width digit
/// upright, kerned under `vkrn`; Latin and digits sideways along a line,
/// under `liga` (`fi`, glyph 16), `kern` (`a` then `b`) and `vrtr` (`v`,
/// glyph 18), with a `vert` form for `v` (glyph 17) a sideways run must not
/// take; and a Hebrew word sideways at its right-to-left level.
fn vertical_mixed() -> Face {
    let cmap = [
        ('\u{6F22}', 1),
        ('a', 2),
        ('b', 3),
        ('v', 4),
        (' ', 5),
        ('f', 6),
        ('i', 7),
        ('2', 8),
        ('0', 9),
        ('6', 10),
        ('\u{FF12}', 11),
        ('\u{05E9}', 12),
        ('\u{05DC}', 13),
        ('\u{05D5}', 14),
        ('\u{05DD}', 15),
        ('\u{5B57}', 19),
    ];
    let mut face = Face::new(20, &cmap);
    face.advances = (0..20u16).map(|glyph| 400 + glyph * 13).collect();
    let metrics: Vec<(u16, i16)> = (0..20u16)
        .map(|glyph| (1000 - glyph * 3, 100 + glyph as i16))
        .collect();
    let tops: Vec<i16> = (0..20i16).map(|glyph| 700 + glyph).collect();
    face = face.with_vertical(&metrics, &tops);
    face.gsub = Some(layout_table(
        &[*b"DFLT", *b"latn", *b"hani"],
        &[
            (*b"liga", vec![0]),
            (*b"vert", vec![1]),
            (*b"vrtr", vec![2]),
        ],
        &[
            lookup(4, 0, &[ligature_subst(6, &[7], 16)]),
            lookup(1, 0, &[single_subst(&[(4, 17)])]),
            lookup(1, 0, &[single_subst(&[(4, 18)])]),
        ],
    ));
    face.gpos = Some(layout_table(
        &[*b"DFLT", *b"latn", *b"hani"],
        &[(*b"kern", vec![0]), (*b"vkrn", vec![1])],
        &[
            lookup(2, 0, &[pair_pos(2, 3, 0x0004, &[-50], 0, &[])]),
            lookup(2, 0, &[pair_pos(1, 11, 0x0008, &[-90], 0, &[])]),
        ],
    ));
    face
}

/// Mongolian letters with an initial, a medial and a final form each, under
/// `mong`'s `init`, `medi` and `fina`, set sideways and joined in a column,
/// and an ideograph upright before them.
fn vertical_mongolian() -> Face {
    let cmap = [
        ('\u{182E}', 1),
        ('\u{1823}', 2),
        ('\u{1829}', 3),
        ('\u{6F22}', 13),
    ];
    let mut face = Face::new(14, &cmap);
    face.advances = (0..14u16).map(|glyph| 400 + glyph * 10).collect();
    face = face.with_vertical(&[(1000, 100); 14], &[700; 14]);
    let forms = |block: u16| -> Vec<(u16, u16)> { (1..=3).map(|g| (g, g + block)).collect() };
    face.gsub = Some(layout_table(
        &[*b"mong"],
        &[
            (*b"init", vec![0]),
            (*b"medi", vec![1]),
            (*b"fina", vec![2]),
        ],
        &[
            lookup(1, 0, &[single_subst(&forms(3))]),
            lookup(1, 0, &[single_subst(&forms(6))]),
            lookup(1, 0, &[single_subst(&forms(9))]),
        ],
    ));
    face
}

/// The ideographic comma's heights: `vhal`, `vpal` and `valt` as single
/// adjustments of its placement and height, and `vkrn` kerning 漢 before it,
/// each of which a setting below switches on or off.
fn vertical_heights() -> Face {
    let mut face = Face::new(3, &[('\u{3001}', 1), ('\u{6F22}', 2)])
        .with_vertical(&[(1000, 120), (1000, 690), (1000, 110)], &[760, 140, 770]);
    face.gpos = Some(layout_table(
        &[*b"DFLT", *b"hani"],
        &[
            (*b"vhal", vec![0]),
            (*b"vpal", vec![1]),
            (*b"vkrn", vec![2]),
            (*b"valt", vec![3]),
        ],
        &[
            lookup(1, 0, &[single_pos(&[1], 0x000A, &[240, -510])]),
            lookup(1, 0, &[single_pos(&[1], 0x000A, &[110, -290])]),
            lookup(2, 0, &[pair_pos(2, 1, 0x0008, &[-115], 0, &[])]),
            lookup(1, 0, &[single_pos(&[1], 0x0002, &[-65])]),
        ],
    ));
    face
}

/// A face with no `glyf` whose vertical origins are its `VORG`'s: a default
/// and two glyphs' own.
fn vertical_origins() -> Face {
    let mut face = Face::new(4, &[('\u{6F22}', 1), ('\u{5B57}', 2), ('\u{3002}', 3)]);
    face.vertical = Some(vec![(1000, 0), (1100, 0), (900, 0), (1000, 0)]);
    face.vorg = Some((880, vec![(2, 950), (3, 300)]));
    face
}

/// A face with no vertical metrics at all, set on the synthesis a column
/// makes, and saying so.
fn vertical_synthesized() -> Face {
    let mut face = Face::new(3, &[('\u{6F22}', 1), ('\u{5B57}', 2)]);
    face.em = 2048;
    face.advances = vec![1024, 2048, 1900];
    face
}

/// The vertical corpus: each face by name, and the paragraphs shaped through
/// it as a column, each under the feature settings beside it.
///
/// Mixed orientation throughout: upright ideographs and kana, their `vert`
/// and `vrt2` forms, a cursive join, `vkrn`, a mark on its base; Latin,
/// digits, a Hebrew word and Mongolian sideways beside them, along a line's
/// plan; the comma's heights under each setting that changes them; a `VORG`
/// face; and a face whose metrics are synthesized.
#[allow(clippy::type_complexity, reason = "a table of cases, read once")]
fn vertical_cases() -> Vec<(&'static str, Face, Vec<(&'static str, Vec<(Tag, u32)>)>)> {
    let on = |name: &[u8; 4], value: u32| (Tag::new(name), value);
    vec![
        (
            "ideographs",
            vertical_ideographs(),
            vec![
                ("\u{6F22}\u{3001}\u{5B57}\u{3002}", vec![]),
                ("\u{6F22}\u{3001}\u{5B57}\u{3002}", vec![on(b"vrt2", 1)]),
                ("\u{4E00}\u{4E8C}", vec![]),
            ],
        ),
        (
            "kana",
            vertical_kana(),
            vec![("\u{30B9}\u{30C8}\u{304B}\u{3099}\u{30B9}", vec![])],
        ),
        (
            "mixed",
            vertical_mixed(),
            vec![
                (
                    "\u{6F22}abv fi\u{5B57}2026\u{FF12}\u{05E9}\u{05DC}\u{05D5}\u{05DD}",
                    vec![],
                ),
                ("\u{6F22}\u{FF12}", vec![on(b"vkrn", 0)]),
            ],
        ),
        (
            "mongolian",
            vertical_mongolian(),
            vec![("\u{6F22}\u{182E}\u{1823}\u{1829}", vec![])],
        ),
        (
            "heights",
            vertical_heights(),
            vec![
                ("\u{6F22}\u{3001}", vec![]),
                ("\u{6F22}\u{3001}", vec![on(b"vhal", 1)]),
                ("\u{6F22}\u{3001}", vec![on(b"vpal", 1)]),
                ("\u{6F22}\u{3001}", vec![on(b"valt", 0)]),
                ("\u{6F22}\u{3001}", vec![on(b"vhal", 1), on(b"vkrn", 1)]),
            ],
        ),
        (
            "origins",
            vertical_origins(),
            vec![("\u{6F22}\u{5B57}\u{3002}", vec![])],
        ),
        (
            "synthesized",
            vertical_synthesized(),
            vec![("\u{6F22}\u{5B57}", vec![])],
        ),
    ]
}

/// The vertical fingerprint, how many glyphs went into it, and how many runs
/// stood upright and sideways: every run of every paragraph of
/// [`vertical_cases`], shaped as a column — its direction, how it stands, the
/// text it covers and how many warnings it carries, and every field of every
/// glyph.
fn vertical() -> (String, usize, usize, usize) {
    let mut digest = Digest::new();
    let (mut glyphs, mut upright, mut sideways) = (0usize, 0usize, 0usize);
    for (name, face, paragraphs) in vertical_cases() {
        let bytes = face.build();
        let sfnt = Sfnt::parse(&bytes).expect("a face built here is a valid sfnt");
        for (text, settings) in &paragraphs {
            let shaper = Shaper::new(&sfnt).with_settings(settings);
            // The case goes into the digest as well, so that a corpus that
            // lost one is a different fingerprint rather than a shorter one
            // that happens to collide.
            digest.bytes(name.as_bytes());
            digest.bytes(text.as_bytes());
            for (tag, value) in settings {
                digest.bytes(&tag.to_bytes());
                digest.bytes(&value.to_be_bytes());
            }
            digest.bytes(&sfnt.units_per_em.to_be_bytes());
            let (paragraph, runs) = shaper.shape_text_vertical(text, BaseDirection::Auto);
            digest.byte(paragraph.base_level().number());
            for run in &runs {
                let direction = run.direction();
                digest.byte(u8::from(direction.is_vertical()));
                digest.byte(u8::from(direction.is_forward()));
                digest.byte(match run.orientation() {
                    Orientation::Horizontal => 0,
                    Orientation::Upright => {
                        upright += 1;
                        1
                    }
                    Orientation::Sideways => {
                        sideways += 1;
                        2
                    }
                });
                let span = run.text();
                for at in [span.start, span.end, run.warnings().len()] {
                    digest.bytes(&u32::try_from(at).unwrap_or(u32::MAX).to_be_bytes());
                }
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
    (digest.finish(), glyphs, upright, sideways)
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
///
/// `SHAPING` moved once, in the commit that finished what milestone 5 could
/// reach of the Universal Shaping Engine. Three things moved it and each is a
/// different Brahmic cluster coming back in a different order: the reordering
/// permutation is computed over the **glyphs** and no longer skipped where
/// substitution changed their number; the halant no longer moves the insertion
/// point, so a pre-base vowel goes in front of a whole conjunct; and a Brahmic
/// character with a canonical decomposition is decomposed before it is mapped.
/// Thirty-five text-rendering-tests cases across seven sections started passing
/// in the same commit, which is what says the new number is the better one.
/// `BIDI` did not move, which is the pair of facts that places the change in
/// the cluster model and not in UAX #9.
///
/// It moved a **second** time, and this one is a position rather than an
/// order: a Brahmic run no longer zeroes the advance a face gave a spacing
/// matra. Thirteen of `NotoSansKannada`'s fourteen `GDEF` marks are spacing
/// matras carrying real `hmtx` advances — U+0CC2 UU is 1526 units at 2048 per
/// em — and zeroing them stacked every glyph of a syllable at one x.
/// text-rendering-tests SHKNDA-3's expected positions are exactly the
/// cumulative `hmtx` sum with those advances intact, which is what says this
/// number is the better one: SHKNDA-3 went 0/31 to 30/31 and SHKNDA-2 4/16 to
/// 8/16 in the same commit, and SHKNDA-1 and SHBALI-2 became whole.
///
/// `BIDI` did not move again, for the same reason as before: the answer is in
/// the plan the run gets, not in how its levels resolve.
///
/// # The third move is not a behaviour change, and that is the point
///
/// It moved a third time because the **corpus grew**, not because shaping did.
/// Milestone 5's last three commits — the joiner deleted after `GSUB`, `rphf`
/// offered only where a base follows, the reph moved to the end of its
/// syllable — changed the shaper and moved this number **not at all**, because
/// no case here contained a joiner, a reph, or a syllable-final base and
/// halant. Three behaviours this file exists to pin were invisible to it.
///
/// A fingerprint that cannot see a behaviour cannot report that two targets
/// disagree about it, so SHKNDA-2 and SHKNDA-3 and their face joined the
/// corpus and the number moved once, deliberately, for that reason alone. The
/// glyph floor went 230 to 480 against a measured 493 in the same commit,
/// which is the other half of the claim: a corpus that grew and a floor that
/// did not would be a corpus that could quietly shrink back.
///
/// The general lesson, since this is the second shape of it in this
/// repository: ask what a check actually covers, not what its name says it
/// covers. Two tests named `..._on_every_target` had exactly one target for
/// the same class of reason.
const SHAPING: &str = "d07a70e4ccd9c31c";
const BIDI: &str = "d77c9e938eb9c996";

/// The vertical fingerprint (FT-07b.8): [`vertical_cases`]' columns. A run's
/// output, recorded where Linux, Windows and `wasm32-wasip1` agree on it;
/// until it is, `vertical_shaping_is_stable_across_targets` fails on it alone
/// and prints the value it computed.
const VERTICAL: &str = "";

/// The least each corpus may produce before its fingerprint means anything.
///
/// `determinism.rs`'s `least_ink`, transposed: a corpus that shaped nothing
/// hashes perfectly stably on every target and proves nothing at all.
const LEAST_GLYPHS: usize = 480;
const LEAST_CHARACTERS: usize = 240;
/// The vertical corpus's floors: the glyphs its paragraphs shape to, and the
/// runs standing upright and sideways that their orientations cut them
/// into, each paragraph's cut counted once however its scripts split it.
const LEAST_VERTICAL_GLYPHS: usize = 48;
/// See [`LEAST_VERTICAL_GLYPHS`].
const LEAST_UPRIGHT_RUNS: usize = 16;
/// See [`LEAST_VERTICAL_GLYPHS`].
const LEAST_SIDEWAYS_RUNS: usize = 4;

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

/// **Vertical shaping is stable across targets** (FT-07b.8): columns of mixed
/// orientation shaped through the faces [`vertical_cases`] builds hash to
/// [`VERTICAL`], after checking that they shaped what they claim — the glyphs
/// and the upright and sideways runs — since a column that stopped shaping
/// hashes as stably as one that shapes. [`SHAPING`] and [`BIDI`] are not
/// moved by it: a move in either is a finding.
#[test]
fn vertical_shaping_is_stable_across_targets() {
    let (fingerprint, glyphs, upright, sideways) = vertical();
    assert!(
        glyphs >= LEAST_VERTICAL_GLYPHS,
        "the vertical corpus produced {glyphs} glyphs, fewer than the \n         {LEAST_VERTICAL_GLYPHS} it is supposed to: it is measuring less than it \n         claims, and its fingerprint is not evidence about anything until that is \n         fixed"
    );
    assert!(
        upright >= LEAST_UPRIGHT_RUNS && sideways >= LEAST_SIDEWAYS_RUNS,
        "the vertical corpus stood {upright} runs upright and {sideways} sideways, \n         fewer than the {LEAST_UPRIGHT_RUNS} and {LEAST_SIDEWAYS_RUNS} its paragraphs' \n         orientations cut them into"
    );
    assert_eq!(
        fingerprint, VERTICAL,
        "vertical shaping moved. If that was deliberate, update this constant in \n         the same commit and say what changed; if two targets disagree, the \n         constant is not the thing to change"
    );
}

/// The vertical corpus shaped twice in one process gives the answer it gave
/// once: nothing a column leaves behind reaches the next.
#[test]
fn vertical_shaping_twice_gives_the_same_answer() {
    assert_eq!(vertical(), vertical());
}

/// Nothing in the shaped output depends on how many times it was shaped.
///
/// A cheap check for the class of bug a fingerprint alone cannot see: state
/// left over between runs. Shaping the same corpus twice in one process has to
/// give the same answer as shaping it once.
///
/// It is also the kept plans' determinism check: the corpus is shaped again
/// through one [`Plans`] per face, which the first pass builds and the second
/// reads back, and both give the answer shaping with none gives — so a plan
/// or an index carried from one case to the next moves nothing.
#[test]
fn shaping_twice_gives_the_same_answer() {
    assert_eq!(shaping(), shaping());
    assert_eq!(bidi(), bidi());

    let kept: BTreeMap<String, Plans> = FIXTURES
        .iter()
        .flat_map(|fixture| cases(fixture))
        .map(|(font, _)| font)
        .filter(|font| face_bytes(font).is_some())
        .map(|font| (font, Plans::new()))
        .collect();
    let plain = shaping();
    assert_eq!(shaping_with(Some(&kept)), plain, "a Plans being built");
    let built: u64 = kept.values().map(Plans::plans_built).sum();
    assert!(built > 0, "no face was shaped through its Plans");
    assert_eq!(shaping_with(Some(&kept)), plain, "a kept Plans read back");
    assert_eq!(
        kept.values().map(Plans::plans_built).sum::<u64>(),
        built,
        "the second pass built a plan again"
    );
}
