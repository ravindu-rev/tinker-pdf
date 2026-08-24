//! Layout whose right answer is arithmetic (ruling 13, roadmap step 5).
//!
//! Half of what replaces `epub_browser.rs`. That oracle asked a headless
//! Chromium where the boxes went and compared. Ruling 13 retires it, and the
//! honest statement of the trade is in `docs/verification.md`: a browser is the
//! reference implementation of CSS and nothing here is. What this file can do
//! instead is narrower and harder to argue with — **every expected number below
//! is computed in the test, from the clause, and not read back from the
//! engine.**
//!
//! # The one thing that makes it possible
//!
//! `BookMetrics::STANDARD` measures with the standard 14, and Courier's advance
//! is exactly 600/1000 of the em for every character it has. So a document set
//! in `monospace` has a line breaker with a closed form: a measure of `W` at
//! `font-size: S` fits `floor(W / 0.6S)` characters, and `css-text-3` §5 breaks
//! at the last opportunity that still fits. [`greedy`] below is that rule
//! written out, and every line-breaking expectation in this file comes from it
//! rather than from `tinker-pdf-layout`.
//!
//! Everything else is the box model as `css2` §8 and §10 state it: adjoining
//! margins collapse to the larger, padding and border move the content edge by
//! their own widths, a float shortens the line boxes beside it and none below
//! it, and a line box's baselines are `line-height` apart.
//!
//! # What this cannot reach, said here rather than discovered
//!
//! It covers pages simple enough to have a closed form. A real book is not one,
//! which is why the corpus sweep in `epub_conservation.rs` stays and why
//! `epub_reftest.rs` sits beside this file: reftests cover shapes whose
//! *absolute* answer nobody can compute but whose *equality* is required. And
//! none of the three notices a rule this engine and this test read the same
//! wrong way, which is the property that left with the browser.

mod epub_support;

use epub_support::layout::{column, document, fits, lay_out, written, Line, MONO_ADVANCE};

/// The measure every test below lays out at, with `body { margin: 0 }` so the
/// flow's content edge is the page's.
const MEASURE: f64 = 240.0;

/// The sheet every document here starts from: no margins anywhere this test did
/// not ask for, so an expectation is about the rule under test and not about
/// the user-agent sheet.
const RESET: &str = "body { margin: 0 } p, div, h1 { margin: 0; padding: 0; border: 0 }";

fn sheet(extra: &str) -> String {
    format!("{RESET} {extra}")
}

/// `css-text-3` §5, written out: break at the last opportunity that still fits.
///
/// The reference line breaker for this file. It is deliberately the crudest
/// thing that implements the rule — words separated by single spaces, no
/// hyphenation, no East Asian rules — and every fixture below is written to
/// stay inside what it can say.
fn greedy(text: &str, measure: f64, size: f64) -> Vec<String> {
    let room = fits(measure, size);
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match out.last_mut() {
            // The space counts: a line is the characters *and* the separators.
            Some(line) if line.chars().count() + 1 + word.chars().count() <= room => {
                line.push(' ');
                line.push_str(word);
            }
            _ => out.push(word.to_owned()),
        }
    }
    out
}

fn texts(lines: &[Line]) -> Vec<String> {
    lines.iter().map(|line| line.text.clone()).collect()
}

/// The gaps between consecutive baselines, which is what every vertical rule in
/// this file is stated in.
///
/// Differences rather than absolutes on purpose: the first baseline of a flow
/// is half-leading plus the face's ascent, and an expectation carrying those
/// would be a test that had copied two numbers out of the font it is measuring
/// with. Every rule under test — `line-height`, margin collapsing,
/// fragmentation — is about a *difference*, so that is what is asserted.
fn gaps(lines: &[Line]) -> Vec<f64> {
    lines.windows(2).map(|pair| pair[1].y - pair[0].y).collect()
}

#[track_caller]
fn close(actual: f64, wanted: f64, what: &str) {
    assert!(
        (actual - wanted).abs() < 1e-9,
        "{what}: {actual} is not {wanted}"
    );
}

// ---- the line box -----------------------------------------------------------

/// **Baselines are `line-height` apart, exactly.**
///
/// Three heights rather than one, because a build that used the font's own line
/// box and ignored the property would agree with a fixture whose `line-height`
/// happened to be the natural one.
#[test]
fn consecutive_baselines_are_one_line_height_apart() {
    for (size, height) in [(20.0, 30.0), (16.0, 16.0), (12.0, 25.5)] {
        let laid = column(
            // Eight words of ten characters, which is three lines or more at
            // every size below: twenty advances at 20px is one word a line,
            // twenty-five at 16px is two, and thirty-three at 12px is three.
            &document(
                "<p>aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd                  eeeeeeeeee ffffffffff gggggggggg hhhhhhhhhh</p>",
            ),
            &sheet(&format!(
                "p {{ font-size: {size}px; line-height: {height}px }}"
            )),
            MEASURE,
        );
        assert!(laid.len() >= 3, "{size}/{height}: {laid:?}");
        for gap in gaps(&laid) {
            close(gap, height, &format!("at {size}px/{height}px"));
        }
    }
}

/// **A run's advance is the character count times the advance of the face.**
///
/// Courier is 600/1000 of the em and this is the only place that number is
/// used as an assertion rather than as an input — everything else in the file
/// depends on it through [`greedy`], so a build that measured with a different
/// face would fail here first and legibly.
#[test]
fn a_runs_width_is_its_characters_times_the_faces_advance() {
    let laid = column(
        &document("<p>aaaaaaaaaa</p>"),
        &sheet("p { font-size: 20px; line-height: 30px }"),
        MEASURE,
    );
    assert_eq!(texts(&laid), ["aaaaaaaaaa"]);
    close(laid[0].width, 10.0 * MONO_ADVANCE * 20.0, "ten characters");
    close(laid[0].size, 20.0, "the size the sheet states");
}

/// **A line holds what the measure divided by the advance allows**, at four
/// measures.
///
/// The measures are chosen to land on and either side of a boundary: 240 points
/// at 20px fits twenty characters exactly, so 239 fits nineteen and the
/// nineteen-character word is the last that goes on the line. A single measure
/// would not tell a correct breaker from one that is off by a character.
#[test]
fn a_line_breaks_at_the_last_word_that_fits_the_measure() {
    const TEXT: &str = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj";
    for measure in [240.0, 239.0, 120.0, 61.0] {
        let laid = column(
            &document(&format!("<p>{TEXT}</p>")),
            &sheet("p { font-size: 20px; line-height: 30px }"),
            measure,
        );
        assert_eq!(
            texts(&laid),
            greedy(TEXT, measure, 20.0),
            "at a measure of {measure}"
        );
    }
}

/// And the arithmetic behind it is stated on its own, both sides of a boundary.
///
/// [`fits`] is what every expectation above rests on, so it is asserted rather
/// than trusted: a build in which it were off by one would make the test above
/// agree with a line breaker that was off by one too.
#[test]
fn the_measure_divided_by_the_advance_is_the_room_a_line_has() {
    // 20px monospace is a 12-point advance.
    assert_eq!(fits(240.0, 20.0), 20, "twenty advances exactly");
    assert_eq!(fits(239.99, 20.0), 19, "a hair under is one fewer");
    assert_eq!(fits(252.0, 20.0), 21);
    assert_eq!(fits(0.0, 20.0), 0, "a measure of nothing holds nothing");
}

// ---- the box model ----------------------------------------------------------

/// **Adjoining margins collapse to the larger** (`css2` §8.3.1).
///
/// Four pairs, and the two that matter are the ones where the answer is not the
/// sum: a build that added them would pass a fixture whose two margins were `0`
/// and `n`, because `max(0, n)` and `0 + n` are the same number.
#[test]
fn adjoining_margins_collapse_to_the_larger_and_are_not_added() {
    for (above, below) in [
        (30.0_f64, 10.0_f64),
        (10.0, 30.0),
        (20.0, 20.0),
        (0.0, 25.0),
    ] {
        let laid = column(
            &document(r#"<p class="a">one</p><p class="b">two</p>"#),
            &sheet(&format!(
                "p {{ font-size: 20px; line-height: 30px }} \
                 .a {{ margin-bottom: {above}px }} .b {{ margin-top: {below}px }}"
            )),
            MEASURE,
        );
        assert_eq!(texts(&laid), ["one", "two"]);
        close(
            laid[1].y - laid[0].y,
            30.0 + above.max(below),
            &format!("a {above}px margin against a {below}px one"),
        );
    }
}

/// **Padding and border move the content edge by their own widths** (`css2`
/// §8.1), and the two are separate declarations that add.
#[test]
fn padding_and_border_each_move_the_text_by_their_own_width() {
    for (padding, border) in [(40.0, 0.0), (0.0, 12.0), (15.0, 5.0)] {
        let laid = column(
            &document("<p>one</p>"),
            &sheet(&format!(
                "p {{ font-size: 20px; line-height: 30px; \
                 padding-left: {padding}px; border-left: {border}px solid #000 }}"
            )),
            MEASURE,
        );
        close(
            laid[0].x,
            padding + border,
            &format!("{padding}px of padding and {border}px of border"),
        );
    }
}

/// **`text-indent` moves the first line and no other** (`css2` §16.1).
///
/// The pair the house rule asks for: a build that indented every line would
/// pass an assertion about the first one alone.
#[test]
fn text_indent_moves_the_first_line_and_leaves_the_rest() {
    let laid = column(
        &document("<p>aaaa bbbb cccc dddd eeee ffff gggg hhhh</p>"),
        &sheet("p { font-size: 20px; line-height: 30px; text-indent: 36px }"),
        MEASURE,
    );
    assert!(laid.len() >= 2, "{laid:?}");
    close(laid[0].x, 36.0, "the first line is indented");
    for line in &laid[1..] {
        close(line.x, 0.0, "and no other line is");
    }
    // And the indent costs the first line its room: 240 less 36 is 204, which
    // is seventeen advances of twelve.
    assert_eq!(
        laid[0].text.chars().count(),
        greedy("aaaa bbbb cccc dddd eeee ffff gggg hhhh", 204.0, 20.0)[0]
            .chars()
            .count()
    );
}

// ---- floats -----------------------------------------------------------------

/// **A float shortens the line boxes beside it and none below it** (`css2`
/// §9.5).
///
/// Both halves, because they fail separately: a build that never shortened
/// anything and a build that shortened the whole block both draw a page that
/// looks like text beside a picture.
#[test]
fn a_float_shortens_the_lines_beside_it_and_not_the_ones_below() {
    const WIDTH: f64 = 60.0;
    const TEXT: &str = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk";
    let laid = column(
        &document(&format!(r#"<div class="f">f</div><p>{TEXT}</p>"#)),
        &sheet(&format!(
            "p {{ font-size: 20px; line-height: 30px }} \
             .f {{ float: left; width: {WIDTH}px; height: 30px; font-size: 20px; line-height: 30px }}"
        )),
        MEASURE,
    );
    let text: Vec<&Line> = laid.iter().filter(|line| line.text != "f").collect();
    assert!(text.len() >= 3, "{laid:?}");

    // Beside it: the line starts past the float and holds what the shortened
    // measure allows.
    close(text[0].x, WIDTH, "the first line clears the float");
    assert_eq!(
        text[0].text,
        greedy(TEXT, MEASURE - WIDTH, 20.0)[0],
        "and is broken to the shortened measure"
    );

    // Below it: the float is one line tall, so from the second line the whole
    // measure is back.
    for line in &text[1..] {
        close(line.x, 0.0, "a line below the float starts at the edge");
    }
    let rest: String = text[1..]
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let wanted: String = TEXT
        .strip_prefix(&format!("{} ", text[0].text))
        .expect("the first line is a prefix of the text")
        .to_owned();
    assert_eq!(
        rest,
        greedy(&wanted, MEASURE, 20.0).join(" "),
        "the lines below the float are broken to the full measure"
    );
}

// ---- fragmentation ----------------------------------------------------------

/// **With one line allowed alone, a page holds the lines that fit it and no
/// more** — `floor(height / line-height)`, at four heights.
///
/// The pure geometric rule, which is only pure once `orphans` and `widows` are
/// out of the way: CSS 2.2 §13.3.2 defaults both to 2, and this fixture sets
/// them to 1 so that the break is decided by the page box alone. The next test
/// is the other half, where they are not.
///
/// The heights straddle a boundary on purpose. 90 and 100 both hold three lines
/// of 30, so a build that divided and rounded rather than flooring would put
/// four on the taller one.
#[test]
fn a_page_holds_the_lines_that_fit_it_when_one_may_stand_alone() {
    const LINE: f64 = 30.0;
    let words: Vec<String> = (0..40).map(|n| format!("w{n:03}")).collect();
    let text = words.join(" ");
    for height in [90.0, 100.0, 130.0, 150.0] {
        let (_, _, laid) = lay_out(
            &document(&format!("<p>{text}</p>")),
            &sheet("p { font-size: 20px; line-height: 30px; orphans: 1; widows: 1 }"),
            MEASURE,
            height,
        );
        let laid = written(&laid);
        assert_eq!(
            texts(&laid),
            greedy(&text, MEASURE, 20.0),
            "at a page {height} tall, fragmentation moved lines and created none"
        );

        // The closed form, computed here: every page but the last is full, and
        // the last holds the remainder.
        let per_page = (height / LINE).floor() as usize;
        let total = laid.len();
        let mut wanted: Vec<usize> = std::iter::repeat_n(per_page, total / per_page).collect();
        if total % per_page != 0 {
            wanted.push(total % per_page);
        }
        let pages = laid.last().expect("lines").page + 1;
        let measured: Vec<usize> = (0..pages)
            .map(|page| laid.iter().filter(|line| line.page == page).count())
            .collect();
        assert_eq!(measured, wanted, "at a page {height} tall");

        // And within a page the baselines are a line apart, and every page
        // starts at the same offset — so the split above is the only thing
        // that moved.
        let first = laid[0].y;
        for page in 0..pages {
            let on: Vec<&Line> = laid.iter().filter(|line| line.page == page).collect();
            close(
                on[0].y,
                first,
                &format!("page {page} starts where page 0 does"),
            );
            for pair in on.windows(2) {
                close(pair[1].y - pair[0].y, LINE, "lines within a page");
            }
        }
    }
}

/// **And a line may not be left alone when `widows` says two** (§13.3.2).
///
/// The same document at the same page box, twice, differing only in the two
/// properties — which is what makes this an assertion about the rule rather
/// than about the fixture. Ten lines at three a page divide as `3, 3, 3, 1`,
/// and the fourth page's single line is the widow §13.3.2 forbids: the break
/// moves back to leave two.
///
/// **The pair is the point.** A build that ignored both properties passes the
/// test above and fails here; a build that applied them always would fail the
/// test above. Neither half alone says the properties are read.
#[test]
fn a_last_line_is_not_left_alone_when_widows_forbids_it() {
    let words: Vec<String> = (0..40).map(|n| format!("w{n:03}")).collect();
    let text = words.join(" ");
    let split = |extra: &str| {
        let (_, _, laid) = lay_out(
            &document(&format!("<p>{text}</p>")),
            &sheet(&format!(
                "p {{ font-size: 20px; line-height: 30px; {extra} }}"
            )),
            MEASURE,
            100.0,
        );
        let laid = written(&laid);
        let pages = laid.last().expect("lines").page + 1;
        (0..pages)
            .map(|page| laid.iter().filter(|line| line.page == page).count())
            .collect::<Vec<usize>>()
    };
    assert_eq!(
        split("orphans: 1; widows: 1"),
        [3, 3, 3, 1],
        "with one line allowed alone the split is geometric"
    );
    assert_eq!(
        split(""),
        [3, 3, 2, 2],
        "and the default of two moves the break back rather than widowing a line"
    );
}

/// **A page box that holds one line holds exactly one.**
///
/// The degenerate end of the same rule, and the one a build that always emitted
/// two lines per page would fail. It is separate because a fixture with room
/// for six cannot tell "the last that fits" from "all but one".
#[test]
fn a_page_with_room_for_one_line_holds_one_line() {
    let words: Vec<String> = (0..6).map(|n| format!("w{n:03}")).collect();
    let text = words.join(" ");
    let (_, _, laid) = lay_out(
        &document(&format!("<p>{text}</p>")),
        &sheet("p { font-size: 20px; line-height: 30px }"),
        // Wide enough for one word per line, tall enough for one line.
        40.0,
        40.0,
    );
    let laid = written(&laid);
    assert_eq!(texts(&laid), words, "one word per line");
    for (index, line) in laid.iter().enumerate() {
        assert_eq!(line.page, index, "one line per page: {laid:?}");
    }
}

// ---- what the tests themselves rest on --------------------------------------

/// The reference breaker agrees with itself about the shapes the file uses.
///
/// [`greedy`] is the expectation for every line-breaking assertion above, so a
/// defect in it would be a defect in all of them at once. These are the cases
/// the fixtures actually exercise, computed by hand.
#[test]
fn the_reference_breaker_breaks_where_the_arithmetic_says() {
    // 20px monospace is 12 points an advance; 240 points is twenty of them.
    assert_eq!(
        greedy("aaaa bbbb cccc dddd eeee", 240.0, 20.0),
        ["aaaa bbbb cccc dddd", "eeee"],
        "nineteen characters fit and twenty-four do not"
    );
    assert_eq!(
        greedy("aaaa bbbb cccc dddd eeee", 60.0, 20.0),
        ["aaaa", "bbbb", "cccc", "dddd", "eeee"],
        "five advances is one word a line"
    );
    // A word longer than the measure still occupies a line of its own: §5 has
    // no opportunity inside it and this build does not break one.
    assert_eq!(greedy("aaaaaaaaaa", 24.0, 20.0), ["aaaaaaaaaa"]);
    assert_eq!(greedy("", 240.0, 20.0), Vec::<String>::new());
}

/// And the engine is what is being measured, not the page box.
///
/// The same document at two measures produces different lines, so a build that
/// ignored the measure entirely — which would make every assertion above about
/// one number — fails here.
#[test]
fn the_same_document_at_two_measures_is_two_columns() {
    let doc = document("<p>aaaa bbbb cccc dddd eeee ffff</p>");
    let style = sheet("p { font-size: 20px; line-height: 30px }");
    let wide = texts(&column(&doc, &style, 240.0));
    let narrow = texts(&column(&doc, &style, 120.0));
    assert_ne!(wide, narrow, "the measure decides the lines");
    assert_eq!(wide.join(" "), narrow.join(" "), "and not the words");
}
