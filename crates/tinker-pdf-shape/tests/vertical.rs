//! Vertical shaping: `docs/design/shaping.md`, *Vertical shaping*.
//!
//! # What these tests can and cannot say
//!
//! Every face below is synthesised here, and every expected glyph and
//! position is computed in the test from the tables the test wrote. That is
//! the same standing `tests/synthetic.rs` states for itself: the author of
//! the table also wrote the reader, so a test here catches a disagreement
//! between the code and the reading `docs/design/shaping.md` settles — the
//! registry's plan, the `vmtx` clause's origin, the `GPOS` clause's sign of
//! a vertical advance — and cannot catch a misreading the two share. The
//! fixtures that can, expectations a person wrote for real faces, are H-64's
//! and FT-07b.10's.
//!
//! The orientation half is different in kind: `VerticalOrientation.txt` is
//! the Unicode Consortium's data, so
//! [`vertical_orientation_matches_the_vendored_file`] holds the compiled
//! table to the file a second parser reads, code point by code point.

use std::ops::Range;
use std::path::PathBuf;

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{BaseDirection, Level, Paragraph};
use tinker_pdf_shape::shape::{itemize, itemize_vertical, Run};
use tinker_pdf_shape::unicode::{self, Script, VerticalOrientation};
use tinker_pdf_shape::{Direction, Orientation, ShapedGlyph, ShapedRun, Shaper, Tag, Warning};

// The face assembler, shared with `fingerprints.rs`: every OpenType Layout
// table a test writes, and the face around them.
mod column_face;
use column_face::*;

/// The runs `text` shapes to as a column, through `shaper`.
fn column_runs(shaper: &Shaper<'_>, text: &str) -> Vec<ShapedRun> {
    shaper.shape_text_vertical(text, BaseDirection::Auto).1
}

/// `text`'s byte offset of each character, as the clusters a run carries.
fn clusters(text: &str) -> Vec<u32> {
    text.char_indices()
        .map(|(at, _)| u32::try_from(at).expect("a short text"))
        .collect()
}

// --- orientation ---------------------------------------------------------

/// The vendored `VerticalOrientation.txt`, as one value per code point, read
/// by this test's own parser: the `@missing` line first, then every row.
fn vendored_orientations() -> Vec<VerticalOrientation> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("ucd")
        .join("VerticalOrientation.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} could not be read ({error}); this test cannot be skipped, \
             because a skipped check exits 0 and reads exactly like a pass",
            path.display()
        )
    });
    let value = |name: &str| match name.trim() {
        "U" => VerticalOrientation::U,
        "R" => VerticalOrientation::R,
        "Tu" => VerticalOrientation::Tu,
        "Tr" => VerticalOrientation::Tr,
        other => panic!("VerticalOrientation.txt has a value {other:?} UAX #50 does not"),
    };
    let range = |codes: &str| -> (usize, usize) {
        let codes = codes.trim();
        let parse = |hex: &str| usize::from_str_radix(hex.trim(), 16).expect("a code point");
        match codes.split_once("..") {
            Some((first, last)) => (parse(first), parse(last)),
            None => (parse(codes), parse(codes)),
        }
    };

    let mut defaults: Vec<(usize, usize, VerticalOrientation)> = Vec::new();
    let mut rows: Vec<(usize, usize, VerticalOrientation)> = Vec::new();
    for line in text.lines() {
        if let Some(body) = line.strip_prefix("# @missing:") {
            let (codes, name) = body
                .split_once(';')
                .expect("a @missing line has two fields");
            let (first, last) = range(codes);
            defaults.push((first, last, value(name)));
            continue;
        }
        let body = line.split('#').next().unwrap_or_default();
        if body.trim().is_empty() {
            continue;
        }
        let (codes, name) = body.split_once(';').expect("a data line has two fields");
        let (first, last) = range(codes);
        rows.push((first, last, value(name)));
    }
    assert_eq!(
        defaults,
        vec![(0, 0x10_FFFF, VerticalOrientation::R)],
        "the file's one @missing line"
    );
    assert!(rows.len() > 2_000, "only {} data rows", rows.len());

    let mut table = vec![VerticalOrientation::R; 0x11_0000];
    for (first, last, value) in defaults.into_iter().chain(rows) {
        for slot in table
            .get_mut(first..=last)
            .expect("a range inside the code space")
        {
            *slot = value;
        }
    }
    table
}

/// **The compiled table is the file**, for every code point there is.
///
/// `build.rs` compiles the property and this reads it again with a parser of
/// its own, so a row the build dropped, a range it merged wrongly or a
/// default it forgot is a code point named here. Surrogates are not
/// characters and are the only code points not asked about.
#[test]
fn vertical_orientation_matches_the_vendored_file() {
    let table = vendored_orientations();
    let mut asked = 0usize;
    let mut wrong: Vec<String> = Vec::new();
    let mut seen = [0usize; 4];
    for (code, expected) in table.iter().enumerate() {
        let Some(c) = u32::try_from(code).ok().and_then(char::from_u32) else {
            continue;
        };
        asked += 1;
        let got = unicode::vertical_orientation(c);
        seen[match expected {
            VerticalOrientation::U => 0,
            VerticalOrientation::R => 1,
            VerticalOrientation::Tu => 2,
            VerticalOrientation::Tr => 3,
        }] += 1;
        if got != *expected && wrong.len() < 8 {
            wrong.push(format!("U+{code:04X}: {got:?}, the file says {expected:?}"));
        }
    }
    assert_eq!(asked, 0x11_0000 - 0x800, "every scalar value was asked");
    assert!(
        wrong.is_empty(),
        "the compiled table disagrees:\n{}",
        wrong.join("\n")
    );
    assert!(
        seen.iter().all(|count| *count > 0),
        "a value the file uses never occurred: {seen:?}"
    );
}

/// The runs of `text` as a column itemizes them: each run's text, script,
/// level and orientation.
fn column(text: &str) -> Vec<(String, Script, u8, Orientation)> {
    let paragraph = Paragraph::new(text, BaseDirection::Auto);
    itemize_vertical(text, &paragraph)
        .into_iter()
        .map(|run| {
            (
                text[run.text.clone()].to_string(),
                run.script,
                run.level.number(),
                run.orientation,
            )
        })
        .collect()
}

/// **A column of CJK, Latin and Mongolian, cut by orientation.**
///
/// The paragraph is written piece by piece below, each piece one run with
/// the script and orientation UAX #50 gives it, so the expected runs are the
/// pieces and not numbers read back from the itemizer:
///
/// - Han is `U` and stands upright; Latin is `R` and stands sideways.
/// - `b` in a combining enclosing circle is one grapheme cluster holding an
///   `Me`, so the whole cluster stands upright (UAX #50 §3.2.1) and is cut
///   from the sideways `a` before it and `c` after it.
/// - `d` and its combining acute are one cluster, sideways as `d` is.
/// - Mongolian, `R`, stands sideways — a run of its own script beside the
///   Latin one — and its free variation selector stays in it.
/// - The ideographic comma is `Tu` and the fullwidth parentheses `Tr`: all
///   three stand upright, and each takes the script of what it follows, so
///   the comma and the left parenthesis join the Han run before them and the
///   right parenthesis stands upright after a sideways `a`.
#[test]
fn itemize_splits_a_vertical_paragraph_by_orientation() {
    let pieces: [(&str, Script, Orientation); 8] = [
        ("\u{65E5}\u{672C}", Script::Han, Orientation::Upright),
        ("a", Script::Latin, Orientation::Sideways),
        ("b\u{20DD}", Script::Latin, Orientation::Upright),
        ("cd\u{0301}", Script::Latin, Orientation::Sideways),
        (
            "\u{182E}\u{1823}\u{1829}\u{180B}",
            Script::Mongolian,
            Orientation::Sideways,
        ),
        (
            "\u{8A9E}\u{3001}\u{FF08}",
            Script::Han,
            Orientation::Upright,
        ),
        ("a", Script::Latin, Orientation::Sideways),
        ("\u{FF09}", Script::Latin, Orientation::Upright),
    ];
    let text: String = pieces.iter().map(|(piece, _, _)| *piece).collect();
    let expected: Vec<(String, Script, u8, Orientation)> = pieces
        .iter()
        .map(|(piece, script, orientation)| ((*piece).to_string(), *script, 0, *orientation))
        .collect();
    assert_eq!(column(&text), expected);

    // The same text along a line is cut by script and level alone, and every
    // run is horizontal: a line itemizes as it did before columns.
    let paragraph = Paragraph::new(&text, BaseDirection::Auto);
    let line: Vec<(Range<usize>, Script, Orientation)> = itemize(&text, &paragraph)
        .into_iter()
        .map(|run| (run.text, run.script, run.orientation))
        .collect();
    let mut at = 0usize;
    let mut edges: Vec<(Range<usize>, Script, Orientation)> = Vec::new();
    for (piece, script, _) in &pieces {
        let end = at + piece.len();
        match edges.last_mut() {
            Some(last) if last.1 == *script => last.0.end = end,
            _ => edges.push((at..end, *script, Orientation::Horizontal)),
        }
        at = end;
    }
    assert_eq!(line, edges);
}

/// A mark that opens a column has nothing to continue, so it stands as its
/// own value says; a lone enclosing mark is a cluster holding one, upright.
#[test]
fn a_mark_opening_a_column_stands_as_its_own_value_says() {
    assert_eq!(
        column("\u{0301}\u{65E5}"),
        vec![
            (
                "\u{0301}".to_string(),
                Script::Han,
                0,
                Orientation::Sideways
            ),
            ("\u{65E5}".to_string(), Script::Han, 0, Orientation::Upright),
        ]
    );
    assert_eq!(
        column("\u{20DD}a"),
        vec![
            (
                "\u{20DD}".to_string(),
                Script::Latin,
                0,
                Orientation::Upright
            ),
            ("a".to_string(), Script::Latin, 0, Orientation::Sideways),
        ]
    );
}

/// The joiners and the variation selectors continue a cluster too: an
/// ideograph with a variation selector after it, and two upright emoji
/// joined by `ZWJ`, stay one upright run though the selector and the joiner
/// are `R` on their own.
#[test]
fn the_joiners_and_the_selectors_stay_with_their_cluster() {
    assert_eq!(
        column("\u{845B}\u{E0100}\u{8449}"),
        vec![(
            "\u{845B}\u{E0100}\u{8449}".to_string(),
            Script::Han,
            0,
            Orientation::Upright
        )]
    );
    // The joiner is `BN`, so how its level is cut is UAX #9's business and
    // not this test's: what is asserted is that nothing in the sequence
    // stands sideways.
    let family = "\u{1F468}\u{200D}\u{1F469}";
    let runs = column(family);
    let whole: String = runs.iter().map(|run| run.0.as_str()).collect();
    assert_eq!(whole, family);
    assert!(
        runs.iter().all(|run| run.3 == Orientation::Upright),
        "{runs:?}"
    );
}

/// A right-to-left word in a column stands sideways at its own level, which
/// is what a consumer reads its direction from: UAX #50 §3.2.3 leaves
/// right-to-left scripts in vertical text unspecified, and the run is shaped
/// right to left along a line and turned like any other sideways run.
#[test]
fn a_right_to_left_word_in_a_column_is_sideways_at_its_level() {
    let runs = column("\u{65E5}\u{05E9}\u{05DC}\u{05D5}\u{05DD}\u{672C}");
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert_eq!(runs[1].1, Script::Hebrew);
    assert_eq!(runs[1].2, 1, "the Hebrew word is at an odd level");
    assert_eq!(runs[1].3, Orientation::Sideways);
    assert_eq!(runs[0].3, Orientation::Upright);
    assert_eq!(runs[2].3, Orientation::Upright);
}

// --- the upright plan and its metrics --------------------------------------

/// `漢`, `、` and `字`, with a vertical form of the comma that only `vert`
/// reaches, and `vmtx` and `glyf` entries distinct enough that a glyph given
/// another's metrics shows: the comma's horizontal form sits low in the em
/// and its vertical form high, as a real face's do.
fn ideographs() -> Face {
    let face = Face::new(5, &[('\u{6F22}', 1), ('\u{3001}', 2), ('\u{5B57}', 4)]);
    let mut face = face.with_vertical(
        &[(1000, 100), (1100, 80), (1000, 700), (950, 50), (1050, 90)],
        &[0, 800, 150, 850, 790],
    );
    face.advances[0] = 500;
    face.advances[3] = 1001;
    face.gsub = Some(layout_table(
        &[*b"DFLT"],
        &[(*b"vert", vec![0])],
        &[lookup(1, 0, &[single_subst(&[(2, 3)])])],
    ));
    face
}

/// **An upright run takes `vert` and the face's `vmtx` advances.**
///
/// `漢、字` stands upright, one run top to bottom: the comma takes the
/// vertical form `vert` maps it to, and every glyph its own advance height
/// and the vertical origin the `vmtx` clause gives it — its `glyf` top plus
/// its top side bearing, half its width in — read from the tables this test
/// wrote. The vertical form is measured as itself, after the substitution,
/// as a line's glyphs are. Along a line the same text takes neither `vert`
/// nor a vertical metric.
#[test]
fn an_upright_run_takes_vert_and_its_vmtx_advances() {
    let face = ideographs();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);
    let text = "\u{6F22}\u{3001}\u{5B57}";
    let at = clusters(text);

    let runs = column_runs(&shaper, text);
    assert_eq!(runs.len(), 1, "one upright run");
    let run = &runs[0];
    assert_eq!(run.direction(), Direction::TopToBottom);
    assert_eq!(run.orientation(), Orientation::Upright);
    let expected = vec![
        face.upright(1, at[0]),
        face.upright(3, at[1]),
        face.upright(4, at[2]),
    ];
    assert_eq!(run.glyphs(), expected.as_slice());
    assert_eq!(run.advance(), 1100 + 950 + 1050, "the column's length");
    assert!(run.warnings().is_empty(), "{:?}", run.warnings());
    for glyph in [1u16, 2, 3, 4] {
        assert_eq!(
            shaper.vertical_metrics(glyph),
            face.expected(glyph),
            "glyph {glyph}"
        );
    }

    let line = shaper.shape_text(text, BaseDirection::Auto).1;
    assert_eq!(line.len(), 1);
    assert_eq!(line[0].direction(), Direction::LeftToRight);
    assert_eq!(line[0].orientation(), Orientation::Horizontal);
    let along: Vec<ShapedGlyph> = [1u16, 2, 4]
        .iter()
        .zip(&at)
        .map(|(glyph, cluster)| face.along(*glyph, *cluster))
        .collect();
    assert_eq!(line[0].glyphs(), along.as_slice());
}

/// A face with no vertical metrics stands upright on synthesized ones — one
/// em down, the origin 880/1000 of an em above the baseline, half the width
/// in — and the run says so once, however many glyphs took them.
#[test]
fn a_face_without_vertical_metrics_is_set_on_a_synthesis_and_says_so() {
    let mut face = Face::new(3, &[('\u{6F22}', 1), ('\u{5B57}', 2)]);
    face.em = 2048;
    face.advances = vec![1024, 2048, 1900];
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);
    let text = "\u{6F22}\u{5B57}";
    let at = clusters(text);
    let runs = column_runs(&shaper, text);
    assert_eq!(runs.len(), 1);
    let expected = vec![face.upright(1, at[0]), face.upright(2, at[1])];
    assert_eq!(runs[0].glyphs(), expected.as_slice());
    assert_eq!(expected[0].y_advance, -2048);
    assert_eq!(expected[0].y_offset, -(2048 * 880 / 1000));
    assert_eq!(expected[1].x_offset, -950);
    assert_eq!(
        runs[0].warnings(),
        &[Warning::VerticalMetricsSynthesized],
        "one warning for the run"
    );
    assert!(shaper.vertical_metrics(1).synthesized);
}

/// The ideographs again, with a second vertical form only `vrt2` reaches,
/// `a` (glyph 6) with a form under each of `vert`, `vrt2` and `vrtr`.
///
/// `vert` is lookup 0 and `vrt2` lookup 1, both from the horizontal comma, so
/// a plan that ran both would end on `vert`'s form — `vrt2`'s lookup no
/// longer finds the glyph it covers — and only a plan that runs `vrt2` *in
/// place of* `vert` ends on `vrt2`'s.
fn ideographs_with_vrt2(carries_vert: bool) -> Face {
    let mut face = ideographs();
    face.cmap.push(('a', 6));
    face.advances.extend([1000, 500, 500, 500, 500]);
    let metrics: Vec<(u16, i16)> = face
        .vertical
        .take()
        .expect("vertical metrics")
        .into_iter()
        .chain([
            (980, 40),
            (1000, 100),
            (1000, 100),
            (1000, 100),
            (1000, 100),
        ])
        .collect();
    let tops: Vec<i16> = face
        .tops
        .take()
        .expect("glyf tops")
        .into_iter()
        .chain([860, 500, 500, 500, 500])
        .collect();
    face = face.with_vertical(&metrics, &tops);
    let mut features = vec![(*b"vrt2", vec![1]), (*b"vrtr", vec![2])];
    if carries_vert {
        features.insert(0, (*b"vert", vec![0]));
    }
    face.gsub = Some(layout_table(
        &[*b"DFLT"],
        &features,
        &[
            // vert: the comma, and `a`, which a sideways run must not take.
            lookup(1, 0, &[single_subst(&[(2, 3), (6, 7)])]),
            // vrt2: the comma to its own form, and `a` likewise.
            lookup(1, 0, &[single_subst(&[(2, 5), (6, 8)])]),
            // vrtr: `a` to the form a sideways run takes.
            lookup(1, 0, &[single_subst(&[(6, 9)])]),
        ],
    ));
    face
}

/// **`vrt2` is applied as the OpenType feature registry says.**
///
/// The registry gives `vert` to an engine that turns sideways runs itself and
/// `vrt2` "in lieu of" it to one that depends on the face for pre-rotated
/// glyphs, `vert` being "a subset" of `vrt2`, which "overrides" it:
///
/// - by default an upright run takes `vert`, and not `vrt2`;
/// - with `vrt2` turned on it takes `vrt2` *in place of* `vert`, and a face
///   that carries `vrt2` and no `vert` takes `vrt2` too;
/// - turned on over a face that carries no `vrt2`, `vert` stays, because
///   nothing overrides it;
/// - `vert` switched off leaves the comma horizontal;
/// - a sideways run takes neither, whatever the settings — `vrtr` is its
///   feature — since its consumer turns it and a vertical form would be
///   turned twice.
#[test]
fn vrt2_is_applied_as_the_registry_says() {
    let comma = |face: &Face, settings: &[(Tag, u32)]| -> u16 {
        let bytes = face.build();
        let sfnt = Sfnt::parse(&bytes).expect("the face parses");
        let shaper = Shaper::new(&sfnt).with_settings(settings);
        let runs = column_runs(&shaper, "\u{6F22}\u{3001}");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].orientation(), Orientation::Upright);
        runs[0].glyphs()[1].glyph
    };
    let on = [(Tag::new(b"vrt2"), 1)];
    let both = ideographs_with_vrt2(true);
    assert_eq!(comma(&both, &[]), 3, "vert by default");
    assert_eq!(comma(&both, &on), 5, "vrt2 in vert's place");
    assert_eq!(
        comma(&ideographs_with_vrt2(false), &on),
        5,
        "a face with vrt2 and no vert"
    );
    assert_eq!(
        comma(&ideographs_with_vrt2(false), &[]),
        2,
        "and without the setting, nothing"
    );
    assert_eq!(
        comma(&ideographs(), &on),
        3,
        "no vrt2 to override vert with"
    );
    assert_eq!(comma(&both, &[(Tag::new(b"vert"), 0)]), 2, "vert off");

    // A sideways `a`: neither vertical alternate, with or without the
    // settings, and `vrtr` always.
    let bytes = both.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let vert_on = [(Tag::new(b"vert"), 1)];
    for settings in [&[][..], &on[..], &vert_on[..]] {
        let shaper = Shaper::new(&sfnt).with_settings(settings);
        let runs = column_runs(&shaper, "\u{6F22}a");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[1].orientation(), Orientation::Sideways);
        assert_eq!(runs[1].glyphs()[0].glyph, 9, "vrtr, under {settings:?}");
    }
}

/// Latin and the features a line gives it: an `f`+`i` ligature under `liga`,
/// a `kern` pair, and a `vert` and a `vrtr` substitution for `v`.
fn latin() -> Face {
    let cmap = [('a', 1), ('b', 2), ('v', 3), (' ', 4), ('f', 5), ('i', 6)];
    let mut face = Face::new(10, &cmap);
    face.advances = vec![500, 510, 520, 530, 250, 300, 260, 560, 540, 600];
    face = face.with_vertical(&[(1000, 100); 10], &[700; 10]);
    let kern = {
        // PairPosFormat1: `a` then `b`, the first glyph's XAdvance -50.
        let pair_set = u16s(&[1, 2, (-50i16) as u16]);
        let head = u16s(&[1, 0, 0x0004, 0, 1, 0]);
        assemble(head, &[2, 10], &[coverage(&[1]), pair_set])
    };
    face.gsub = Some(layout_table(
        &[*b"DFLT"],
        &[
            (*b"liga", vec![0]),
            (*b"vert", vec![1]),
            (*b"vrtr", vec![2]),
        ],
        &[
            lookup(4, 0, &[ligature_subst(5, &[6], 7)]),
            lookup(1, 0, &[single_subst(&[(1, 8), (3, 8)])]),
            lookup(1, 0, &[single_subst(&[(3, 9)])]),
        ],
    ));
    face.gpos = Some(layout_table(
        &[*b"DFLT"],
        &[(*b"kern", vec![0])],
        &[lookup(2, 0, &[kern])],
    ));
    face
}

/// **A sideways run keeps the horizontal plan.**
///
/// In a column, Latin stands sideways: shaped along a line — `liga`, `kern`
/// and the `hmtx` advances — with `vrtr` added and `vert` kept out, its
/// direction its level's, and marked for its consumer to turn. The upright
/// ideograph before it is a run of its own.
#[test]
fn a_sideways_run_keeps_the_horizontal_plan() {
    let mut face = latin();
    face.cmap.push(('\u{6F22}', 4));
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);
    let text = "\u{6F22}abv fi";
    let at = clusters(text);
    let runs = column_runs(&shaper, text);
    assert_eq!(runs.len(), 2, "an upright ideograph and a sideways word");
    assert_eq!(runs[0].orientation(), Orientation::Upright);
    let run = &runs[1];
    assert_eq!(run.orientation(), Orientation::Sideways);
    assert_eq!(run.direction(), Direction::LeftToRight);

    let mut a = face.along(face.glyph('a'), at[1]);
    a.x_advance -= 50;
    let expected = vec![
        a,
        face.along(face.glyph('b'), at[2]),
        face.along(9, at[3]),
        face.along(face.glyph(' '), at[4]),
        face.along(7, at[5]),
    ];
    assert_eq!(run.glyphs(), expected.as_slice());
    assert_eq!(
        run.advance(),
        expected.iter().map(|glyph| glyph.x_advance).sum::<i32>(),
        "a sideways run's length is its line's"
    );
}

/// Mongolian letters with an initial, a medial and a final form each, under
/// `mong`'s `init`, `medi` and `fina`.
fn mongolian() -> Face {
    let letters = [('\u{182E}', 1), ('\u{1823}', 2), ('\u{1829}', 3)];
    let mut face = Face::new(13, &letters);
    face.advances = (0..13).map(|glyph| 400 + glyph * 10).collect();
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

/// **A Mongolian run in a vertical paragraph joins.**
///
/// Mongolian is `R`: in a column it stands sideways, shaped along a line by
/// the joining plan — its first letter initial, its middle medial, its last
/// final — and turned by its consumer. Forced upright, the same word takes
/// the default plan, which joins nothing: CSS Writing Modes 3 §5.1.1 shapes a
/// cursive script set upright in its isolated forms.
#[test]
fn a_mongolian_run_in_a_vertical_paragraph_joins() {
    let face = mongolian();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);
    let text = "\u{182E}\u{1823}\u{1829}";
    let at = clusters(text);
    let runs = column_runs(&shaper, text);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].orientation(), Orientation::Sideways);
    assert_eq!(runs[0].direction(), Direction::LeftToRight);
    let joined = vec![
        face.along(1 + 3, at[0]),
        face.along(2 + 6, at[1]),
        face.along(3 + 9, at[2]),
    ];
    assert_eq!(runs[0].glyphs(), joined.as_slice());

    let upright = Run {
        text: 0..text.len(),
        script: Script::Mongolian,
        level: Level::LTR,
        orientation: Orientation::Upright,
    };
    let shaped = shaper.shape(text, &upright);
    let isolated: Vec<u16> = shaped.glyphs().iter().map(|glyph| glyph.glyph).collect();
    assert_eq!(isolated, [1, 2, 3], "upright, nothing joins");
    assert_eq!(shaped.direction(), Direction::TopToBottom);
}

/// A caller who names its own features is taken at its word in a column:
/// no vertical alternate is added to what it asked for.
#[test]
fn named_features_are_taken_at_their_word_in_a_column() {
    let face = ideographs();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let liga = [Tag::new(b"liga")];
    let shaper = Shaper::new(&sfnt).with_features(&liga, &[]);
    let runs = column_runs(&shaper, "\u{6F22}\u{3001}");
    assert_eq!(runs[0].glyphs()[1].glyph, 2, "vert was not asked for");
    let vert = [Tag::new(b"vert")];
    let shaper = Shaper::new(&sfnt).with_features(&vert, &[]);
    let runs = column_runs(&shaper, "\u{6F22}\u{3001}");
    assert_eq!(runs[0].glyphs()[1].glyph, 3, "and is applied where it is");
}

// --- GPOS down a column ----------------------------------------------------

/// Where each glyph of a run is drawn — the pen plus its offset — walking
/// the pen by every advance, which is how a consumer draws a column.
fn drawn(glyphs: &[ShapedGlyph]) -> Vec<(i32, i32)> {
    let (mut x, mut y) = (0i32, 0i32);
    glyphs
        .iter()
        .map(|glyph| {
            let at = (x + glyph.x_offset, y + glyph.y_offset);
            x += glyph.x_advance;
            y += glyph.y_advance;
            at
        })
        .collect()
}

/// Katakana ス and ト at one em each, their vertical origins 880 above the
/// baseline.
fn katakana() -> Face {
    Face::new(3, &[('\u{30B9}', 1), ('\u{30C8}', 2)]).with_vertical(&[(1000, 120); 3], &[760; 3])
}

/// **`vkrn` adjusts a pair down the column.**
///
/// The registry's own example pair, ス then ト, kerned under `vkrn` with a
/// first record of `XAdvance` −300 and `YAdvance` −200 and a second of
/// `YPlacement` −40; and the same pair under `kern` with a `YPlacement` of
/// −33 and an `XAdvance` of −77. Down a column, per the `GPOS` clause:
///
/// - the `YAdvance` lengthens the advance *height* — "to increase the
///   advance height of a glyph by 100, the YAdvance = 100" — so −200 takes
///   200 off ス's, and its `y_advance` is 200 nearer zero;
/// - the `XAdvance`, "only used for horizontal layout", moves nothing, and no
///   glyph of the column advances across it;
/// - the placement adds to ト's offset as written;
/// - and `kern`, "not intended for use in vertical text layout", does not
///   run at all.
///
/// Along a line it is the other way round: `kern` runs and `vkrn` does not.
#[test]
fn vkrn_adjusts_a_pair_down_the_column() {
    let mut face = katakana();
    face.gpos = Some(layout_table(
        &[*b"DFLT"],
        &[(*b"vkrn", vec![0]), (*b"kern", vec![1])],
        &[
            lookup(
                2,
                0,
                &[pair_pos(1, 2, 0x000C, &[-300, -200], 0x0002, &[-40])],
            ),
            lookup(2, 0, &[pair_pos(1, 2, 0x0006, &[-33, -77], 0, &[])]),
        ],
    ));
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);
    // ス ト ス: the pair kerns once, and the last ス, with nothing after it,
    // stands as it would alone.
    let text = "\u{30B9}\u{30C8}\u{30B9}";
    let at = clusters(text);

    let runs = column_runs(&shaper, text);
    assert_eq!(runs.len(), 1);
    let mut su = face.upright(1, at[0]);
    su.y_advance -= -200;
    let mut to = face.upright(2, at[1]);
    to.y_offset += -40;
    let expected = vec![su, to, face.upright(1, at[2])];
    assert_eq!(runs[0].glyphs(), expected.as_slice());
    assert_eq!(runs[0].advance(), 800 + 1000 + 1000);
    assert!(runs[0].glyphs().iter().all(|glyph| glyph.x_advance == 0));

    let line = shaper.shape_text(text, BaseDirection::Auto).1;
    let mut su = face.along(1, at[0]);
    su.y_offset = -33;
    su.x_advance -= 77;
    let expected = vec![su, face.along(2, at[1]), face.along(1, at[2])];
    assert_eq!(line[0].glyphs(), expected.as_slice());
}

/// 漢 and the ideographic comma, with three positioning features the comma
/// may take — `vhal` and `vpal` as single adjustments of its placement and
/// its height, `valt` as a placement alone where it is carried — and `vkrn`
/// kerning 漢 before it.
fn comma_heights(with_valt: bool) -> Face {
    let mut face = Face::new(3, &[('\u{3001}', 1), ('\u{6F22}', 2)])
        .with_vertical(&[(1000, 120), (1000, 700), (1000, 120)], &[760, 150, 760]);
    let mut features = vec![
        (*b"vhal", vec![0]),
        (*b"vpal", vec![1]),
        (*b"vkrn", vec![2]),
    ];
    let mut lookups = vec![
        lookup(1, 0, &[single_pos(&[1], 0x000A, &[250, -500])]),
        lookup(1, 0, &[single_pos(&[1], 0x000A, &[100, -300])]),
        lookup(2, 0, &[pair_pos(2, 1, 0x0008, &[-111], 0, &[])]),
    ];
    if with_valt {
        features.push((*b"valt", vec![3]));
        lookups.push(lookup(1, 0, &[single_pos(&[1], 0x0002, &[-60])]));
    }
    face.gpos = Some(layout_table(&[*b"DFLT"], &features, &lookups));
    face
}

/// `漢、` as a column through `face` under `settings`: 漢's advance height
/// and the comma's offset down the column and advance height.
fn comma_column(face: &Face, settings: &[(Tag, u32)]) -> (i32, i32, i32) {
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt).with_settings(settings);
    let runs = column_runs(&shaper, "\u{6F22}\u{3001}");
    assert_eq!(runs.len(), 1);
    let glyphs = runs[0].glyphs();
    (
        -glyphs[0].y_advance,
        glyphs[1].y_offset,
        -glyphs[1].y_advance,
    )
}

/// **`vhal` and `vpal` take their adjustments when switched on**, and only
/// then: the registry has both off by default.
///
/// The comma's vertical origin is 150 + 700 = 850 above its baseline, so its
/// offset down the column starts at −850 and each feature's `YPlacement` is
/// added to it as written, its `YAdvance` taken off its height (−500 is a
/// height of 500). And the registry's interactions: `vhal` gives fixed half
/// heights, under which `vkrn` "should be disabled for that run" unless the
/// caller asks for it, while `vpal` gives proportional heights that `vkrn` is
/// meant to kern, so it stays.
#[test]
fn vhal_and_vpal_take_their_adjustments_when_switched_on() {
    let face = comma_heights(false);
    let tag = |name: &[u8; 4], value: u32| (Tag::new(name), value);
    let kerned = 1000 - 111;
    assert_eq!(
        comma_column(&face, &[]),
        (kerned, -850, 1000),
        "neither by default"
    );
    assert_eq!(
        comma_column(&face, &[tag(b"vhal", 1)]),
        (1000, -850 + 250, 1000 - 500),
        "vhal's, and vkrn withdrawn"
    );
    assert_eq!(
        comma_column(&face, &[tag(b"vpal", 1)]),
        (kerned, -850 + 100, 1000 - 300),
        "vpal's, and vkrn kept"
    );
    assert_eq!(
        comma_column(&face, &[tag(b"vhal", 1), tag(b"vkrn", 1)]),
        (kerned, -850 + 250, 1000 - 500),
        "vkrn put back by a caller who asks for it"
    );
    assert_eq!(
        comma_column(&face, &[tag(b"vkrn", 0)]),
        (1000, -850, 1000),
        "vkrn switched off"
    );
}

/// **`valt` is on by default where the face carries it, and withdraws
/// `vkrn`.**
///
/// The registry has `valt` "active by default in vertical-setting contexts"
/// and `vkrn` "disabled" for a run it applies to: with a face that carries
/// it, the comma takes `valt`'s placement and 漢 no kerning. A face that does
/// not carry it keeps `vkrn`; so does a caller who switches `valt` off, or
/// turns `vkrn` on; and a caller who turns on `vpal` has chosen another
/// glyph-height feature, which withdraws `valt`.
#[test]
fn valt_is_on_where_the_face_carries_it_and_withdraws_vkrn() {
    let tag = |name: &[u8; 4], value: u32| (Tag::new(name), value);
    let with = comma_heights(true);
    let kerned = 1000 - 111;
    assert_eq!(
        comma_column(&with, &[]),
        (1000, -850 - 60, 1000),
        "valt, no vkrn"
    );
    assert_eq!(
        comma_column(&comma_heights(false), &[]),
        (kerned, -850, 1000),
        "a face without valt keeps vkrn"
    );
    assert_eq!(
        comma_column(&with, &[tag(b"valt", 0)]),
        (kerned, -850, 1000),
        "valt off"
    );
    assert_eq!(
        comma_column(&with, &[tag(b"vkrn", 1)]),
        (kerned, -850 - 60, 1000),
        "vkrn asked for"
    );
    assert_eq!(
        comma_column(&with, &[tag(b"vpal", 1)]),
        (kerned, -850 + 100, 1000 - 300),
        "vpal chosen: valt withdrawn, vkrn kept"
    );
}

/// か and the combining voiced sound mark, which is `Mn`, so the two are one
/// grapheme cluster standing upright; the mark is a `GDEF` mark with a full
/// advance height of its own in `vmtx`, which a column zeroes.
fn kana_with_mark() -> Face {
    let mut face = Face::new(3, &[('\u{304B}', 1), ('\u{3099}', 2)])
        .with_vertical(&[(1000, 120), (1000, 120), (1000, 0)], &[760, 760, 900]);
    face.advances[2] = 600;
    face.gdef = Some(gdef(&[(1, 1), (2, 3)]));
    face.gpos = Some(layout_table(
        &[*b"DFLT"],
        &[(*b"mark", vec![0])],
        &[lookup(4, 0, &[mark_pos(2, (300, 900), 1, (900, 800))])],
    ));
    face
}

/// **A mark attaches to its base in a column.**
///
/// The mark's anchor (300, 900) must meet the base's (900, 800), each in its
/// own glyph's outline coordinates. Down a column the pen has moved past the
/// base by its advance height when the mark is drawn, so the mark's offset is
/// the base's, plus the base's height back up the column, plus the anchors'
/// difference — `Buffer::propagate_attachments`'s forward arithmetic on both
/// axes — and its own advance is zeroed, as `GDEF` says it is a mark. The
/// test computes the offset from the tables, and then checks the geometry it
/// stands for: drawn where the pen and the offsets put them, the two anchors
/// are one point.
#[test]
fn a_mark_attaches_to_its_base_in_a_column() {
    let face = kana_with_mark();
    let bytes = face.build();
    let sfnt = Sfnt::parse(&bytes).expect("the face parses");
    let shaper = Shaper::new(&sfnt);
    let text = "\u{304B}\u{3099}";
    let at = clusters(text);
    let runs = column_runs(&shaper, text);
    assert_eq!(runs.len(), 1, "a base and its mark stand as one cluster");

    let base = face.upright(1, at[0]);
    let mark = ShapedGlyph {
        glyph: 2,
        cluster: at[1],
        x_advance: 0,
        y_advance: 0,
        x_offset: base.x_offset + (900 - 300),
        y_offset: base.y_offset - base.y_advance + (800 - 900),
    };
    assert_eq!(runs[0].glyphs(), &[base, mark]);

    let points = drawn(runs[0].glyphs());
    assert_eq!(
        (points[0].0 + 900, points[0].1 + 800),
        (points[1].0 + 300, points[1].1 + 900),
        "the base's anchor and the mark's are one point"
    );
}

/// Two ideographs joined by a cursive lookup down the column: 一's exit at
/// (520, −100) and 二's entry at (450, 900), the second's advance width
/// narrower so the two vertical origins sit at different x, and the anchors
/// 70 apart across the column so that the glyph that moves across moves.
fn cursive_pair(flags: u16) -> Face {
    let mut face = Face::new(3, &[('\u{4E00}', 1), ('\u{4E8C}', 2)])
        .with_vertical(&[(1000, 120), (1000, 120), (1100, 60)], &[760, 760, 840]);
    face.advances[2] = 900;
    face.gpos = Some(layout_table(
        &[*b"DFLT"],
        &[(*b"curs", vec![0])],
        &[lookup(
            3,
            flags,
            &[cursive_pos(1, (520, -100), 2, (450, 900))],
        )],
    ));
    face
}

/// **A cursive join closes the gap down a column**, the column being the
/// join's third case.
///
/// Along the column the first glyph's advance carries the pen to its exit
/// and the second gives up the distance from its origin to its entry; across
/// it one glyph's placement makes the anchors meet. Whichever way
/// `RIGHT_TO_LEFT` chooses the glyph that moves across, the exit and the
/// entry are drawn at one point, and the glyph that does not move keeps its
/// place across the column.
#[test]
fn a_cursive_join_closes_the_gap_down_a_column() {
    for (flags, keeps) in [(0u16, 0usize), (1, 1)] {
        let face = cursive_pair(flags);
        let bytes = face.build();
        let sfnt = Sfnt::parse(&bytes).expect("the face parses");
        let shaper = Shaper::new(&sfnt);
        let text = "\u{4E00}\u{4E8C}";
        let runs = column_runs(&shaper, text);
        assert_eq!(runs.len(), 1);
        let glyphs = runs[0].glyphs();
        let points = drawn(glyphs);
        assert_eq!(
            (points[0].0 + 520, points[0].1 - 100),
            (points[1].0 + 450, points[1].1 + 900),
            "the exit and the entry are one point, under flags {flags}"
        );
        let stays = face.upright(glyphs[keeps].glyph, glyphs[keeps].cluster);
        assert_eq!(
            glyphs[keeps].x_offset, stays.x_offset,
            "the glyph that does not move across keeps its place, under flags {flags}"
        );
        let moves = face.upright(glyphs[1 - keeps].glyph, glyphs[1 - keeps].cluster);
        assert_ne!(
            glyphs[1 - keeps].x_offset,
            moves.x_offset,
            "the other glyph moved across, under flags {flags}"
        );
        assert!(glyphs.iter().all(|glyph| glyph.x_advance == 0));
    }
}
