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

use tinker_pdf_shape::bidi::{BaseDirection, Paragraph};
use tinker_pdf_shape::shape::{itemize, itemize_vertical};
use tinker_pdf_shape::unicode::{self, Script, VerticalOrientation};
use tinker_pdf_shape::Orientation;

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
