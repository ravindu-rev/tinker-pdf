//! What a page's resource scope looks up while its content runs
//! (`docs/design/performance.md`, P-02): the counts a scope makes, read off
//! MS-01's doors and held to what the page's content uses.
//!
//! Every input is written here, in memory, in the guard suite's embedded face
//! (`render_support::curvy_font`), so the default and `bundled-fonts` builds
//! count alike and the `wasm32-wasip1` leg, which has no filesystem, runs it.

mod guard_support;
mod render_support;

use guard_support::{deflated, face, object, pdf, stream};
use tinker_pdf::{Document, RenderOptions, RenderWarning, Stats, TextOptions};

/// A dense page's lines, as the benchmark set's `dense` lays them out.
const LINES: usize = 70;

/// Each line's characters.
const COLUMNS: usize = 80;

/// The dense page's text: seventy lines of eighty characters, words of
/// letters between single spaces, by formula so every target writes the
/// same bytes.
fn lines() -> Vec<String> {
    (0..LINES)
        .map(|line| {
            let mut text = String::with_capacity(COLUMNS);
            let mut word = 0;
            while text.len() < COLUMNS {
                // A word of three to nine letters, then a space.
                let length = 3 + (line * 7 + word * 5) % 7;
                for at in 0..length {
                    if text.len() == COLUMNS {
                        break;
                    }
                    let letter = (line * 11 + word * 13 + at * 3) % 26;
                    text.push(char::from(b'a' + letter as u8));
                }
                if text.len() < COLUMNS {
                    text.push(' ');
                }
                word += 1;
            }
            text
        })
        .collect()
}

/// A one-page document in the embedded face under the resource name `font`,
/// one `Tf` and one `Tj` a line, as the benchmark set's dense pages write
/// theirs. 1 the catalog, 2 the page tree, 3 the page, 4 its content, 5 to 7
/// the face.
fn dense_page(font: &str) -> Vec<u8> {
    let mut content = String::new();
    for (at, line) in lines().iter().enumerate() {
        let y = 806 - 11 * at;
        content.push_str(&format!("BT /{font} 9 Tf 36 {y} Td ({line}) Tj ET\n"));
    }
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842]\n\
             /Resources << /Font << /{font} 5 0 R >> >> /Contents 4 0 R >>"
        )),
        deflated("", content.as_bytes()),
    ];
    objects.extend(face(5));
    pdf(&objects)
}

/// **P-02.2**: extracting a dense page's text looks up no key the name table
/// holds by its bytes — every key it reads is a constant — so the lookups it
/// makes are the resource names its content uses. The same page with its font
/// named `/Font`, a resource name the table pre-interns, shows the count sees
/// a lookup where there is one.
#[test]
fn extracting_a_page_looks_up_no_well_known_key() {
    let letters: usize = lines()
        .iter()
        .map(|line| line.chars().filter(|c| !c.is_whitespace()).count())
        .sum();

    let document = Document::open(dense_page("F1")).expect("the dense page opens");
    let (text, stats) = document
        .page(0)
        .expect("its page")
        .text_counted(&TextOptions::default());
    let read = text
        .plain_text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .count();
    assert_eq!(read, letters, "the page reads every letter it shows");
    assert_eq!(
        stats.keys_looked_up, 0,
        "a key the table holds was looked up by its bytes: {stats:?}"
    );

    let twin = Document::open(dense_page("Font")).expect("the twin opens");
    let (twin_text, twin_stats) = twin
        .page(0)
        .expect("its page")
        .text_counted(&TextOptions::default());
    assert_eq!(
        twin_text.plain_text(),
        text.plain_text(),
        "the twin differs only in its font's name"
    );
    assert!(
        twin_stats.keys_looked_up >= 1,
        "a font named /Font is looked up by its bytes, and the count missed it: {twin_stats:?}"
    );
    println!(
        "resource-work: RAN {letters} letters on {LINES} lines, no key looked up; \
         the twin's /Font looked up {} times",
        twin_stats.keys_looked_up
    );
}

/// The rule the count holds, held at the source too: no reader on the path
/// spells out by its bytes a key the name table pre-interns, where its
/// constant stands for it. Read when the suite is built, so the wasm leg reads
/// it too; each file's own tests, which build documents, are left out.
#[test]
fn no_reader_spells_out_a_key_the_name_table_holds() {
    const SOURCES: [(&str, &str); 3] = [
        ("resources.rs", include_str!("../src/resources.rs")),
        ("fonts.rs", include_str!("../src/fonts.rs")),
        (
            "tinker-pdf-cos font.rs",
            include_str!("../../tinker-pdf-cos/src/font.rs"),
        ),
    ];
    let mut checked = 0;
    for (file, source) in SOURCES {
        let code = source
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or(source);
        for key in tinker_pdf_cos::NAMES {
            let key = core::str::from_utf8(key).expect("every pre-interned key is ASCII");
            let spelled = format!("intern(b\"{key}\")");
            assert!(
                !code.contains(&spelled),
                "{file} looks /{key} up by its bytes; its Name constant stands for it"
            );
            checked += 1;
        }
    }
    println!("resource-work: RAN {checked} keys against three readers");
}

/// The colour spaces the painting page selects by name, each with the
/// components its `scn` sets: a spot colour whose tint transform is an object
/// of its own, a Lab space whose `/Range` is written backwards — a repair,
/// named by every render that meets it — a calibrated RGB and a palette.
const SPACES: [(&str, &str, &str); 4] = [
    ("Spot", "[/Separation /Ink /DeviceCMYK 5 0 R]", "0.5"),
    (
        "Lb",
        "[/Lab << /WhitePoint [0.9642 1 0.8249] /Range [10 -10 -100 100] >>]",
        "50 10 10",
    ),
    (
        "Cal",
        "[/CalRGB << /WhitePoint [0.9505 1 1.089] /Gamma [2.2 2.2 2.2] >>]",
        "0.2 0.4 0.6",
    ),
    ("Ix", "[/Indexed /DeviceRGB 1 <FF00000000FF>]", "1"),
];

/// The shadings it paints: an axial one through a function of its own, a
/// radial one through a stitching function whose pieces are objects — the
/// first of them the axial one's function — and an axial one in the spot
/// colour's space, its tint transform the spot colour's, through a sampled
/// function, a stream.
const SHADINGS: [(&str, &str); 3] = [
    (
        "Sh0",
        "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 40 0] /Function 6 0 R >>",
    ),
    (
        "Sh1",
        "<< /ShadingType 3 /ColorSpace /DeviceRGB /Coords [20 20 0 20 20 20] /Function 7 0 R >>",
    ),
    (
        "Sh2",
        "<< /ShadingType 2 /ColorSpace [/Separation /Ink /DeviceCMYK 5 0 R]\n\
         /Coords [0 0 0 40] /Function 9 0 R >>",
    ),
];

/// The graphics states it sets: alphas alone, and two that share a transfer
/// function, a calculator — a stream.
const STATES: [(&str, &str); 3] = [
    ("GS0", "<< /CA 0.5 /ca 0.5 >>"),
    ("GS1", "<< /TR 10 0 R >>"),
    ("GS2", "<< /BM /Multiply /TR 10 0 R >>"),
];

/// The function objects the spaces, shadings and states name, 5 to 10: the
/// spot colour's tint, the axial shading's, the stitching function and its
/// second piece, the sampled function and the calculator.
const FUNCTIONS: u64 = 6;

/// `/name value` for each entry, as a dictionary's body.
fn entries<'a>(named: impl IntoIterator<Item = &'a (&'a str, &'a str)>) -> String {
    named
        .into_iter()
        .map(|(name, value)| format!("/{name} {value}\n"))
        .collect()
}

/// A page of `rounds` rounds, each a `gs`, a `cs`, an `scn` and an `sh`, the
/// state, the space and the shading taken in turn, with a small fill in the
/// colour set and the shading clipped to a square of its own. 1 the catalog,
/// 2 the page tree, 3 the page, 4 its content, 5 to 10 the functions.
fn painting_page(rounds: usize) -> Vec<u8> {
    let mut content = String::new();
    for round in 0..rounds {
        let (space, _, components) = SPACES[round % SPACES.len()];
        let (shading, _) = SHADINGS[round % SHADINGS.len()];
        let (state, _) = STATES[round % STATES.len()];
        let (x, y) = (round % 20 * 2, round / 20 % 20 * 2);
        content.push_str(&format!(
            "q /{state} gs /{space} cs {components} scn {x} {y} 1 1 re f \
             {x} {y} 2 2 re W n /{shading} sh Q\n"
        ));
    }
    let spaces: Vec<(&str, &str)> = SPACES
        .iter()
        .map(|(name, value, _)| (*name, *value))
        .collect();
    let (spaces, shadings, states) = (entries(&spaces), entries(&SHADINGS), entries(&STATES));
    pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
             /Resources << /ColorSpace << {spaces} >>\n\
             /Shading << {shadings} >>\n/ExtGState << {states} >> >>\n\
             /Contents 4 0 R >>"
        )),
        deflated("", content.as_bytes()),
        object("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 1 1 0] /N 1 >>"),
        object("<< /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >>"),
        object(
            "<< /FunctionType 3 /Domain [0 1] /Functions [6 0 R 8 0 R] /Bounds [0.5]\n\
             /Encode [0 1 0 1] >>",
        ),
        object("<< /FunctionType 2 /Domain [0 1] /C0 [0 1 0] /C1 [1 1 0] /N 1 >>"),
        stream(
            "/FunctionType 0 /Domain [0 1] /Range [0 1] /Size [2] /BitsPerSample 8",
            &[0x00, 0xFF],
        ),
        stream(
            "/FunctionType 4 /Domain [0 1] /Range [0 1]",
            b"{ 1 exch sub }",
        ),
    ])
}

/// The default render, counting.
fn asking() -> RenderOptions {
    RenderOptions {
        stats: true,
        ..RenderOptions::default()
    }
}

/// What a render of the painting page of `rounds` rounds counts, and warns.
fn painting_render(rounds: usize) -> (Stats, Vec<RenderWarning>) {
    let document = Document::open(painting_page(rounds)).expect("the painting page opens");
    let bitmap = document.page(0).expect("its page").render(&asking());
    (bitmap.stats.expect("asked"), bitmap.warnings)
}

/// The Lab space's repair, as a render names it.
fn lab_repaired() -> RenderWarning {
    RenderWarning::RepairedColorSpace {
        name: "Lb".to_string(),
        reason: "LabRangeUnordered".to_string(),
    }
}

/// **P-02.5**: a page's scope reads each colour space its content selects by
/// name once, both of its readings at once; each function an object holds
/// once, whether two shadings, two spaces, two states or a stitching
/// function and a shading share it; and each graphics state once — so a
/// render of a page of a thousand `cs`, `scn`, `sh` and `gs` operators reads
/// what a page of twelve rounds of them reads, every name and object it uses
/// once, and at most the distinct names and objects its content uses. What
/// reading the Lab space repaired is named once, as it is read once.
#[test]
fn a_scope_parses_each_named_space_and_state_once() {
    let (thousand, warnings) = painting_render(250);
    let (twelve, _) = painting_render(12);
    let read = |stats: &Stats| {
        (
            stats.color_spaces_parsed,
            stats.functions_read,
            stats.graphics_states_read,
        )
    };
    let distinct = (SPACES.len() as u64, FUNCTIONS, STATES.len() as u64);
    let (spaces, functions, states) = read(&thousand);
    assert!(
        spaces <= distinct.0 && functions <= distinct.1 && states <= distinct.2,
        "spaces, functions and states read over a thousand operators: {:?}, \
         past the {distinct:?} the content uses: {thousand:?}",
        read(&thousand)
    );
    assert_eq!(
        read(&thousand),
        distinct,
        "each name and object the content uses, read once: {thousand:?}"
    );
    assert_eq!(
        read(&thousand),
        read(&twelve),
        "a thousand operators read what forty-eight read"
    );
    let repaired: Vec<&RenderWarning> = warnings
        .iter()
        .filter(|warning| matches!(warning, RenderWarning::RepairedColorSpace { .. }))
        .collect();
    assert_eq!(repaired, [&lab_repaired()], "{warnings:?}");
    println!(
        "resource-work: RAN a thousand operators, {spaces} spaces, {functions} functions \
         and {states} states read"
    );
}

/// A page whose one fill is a tiling pattern with no resources of its own,
/// whose cell selects the Lab space by name: the cell runs in the page's
/// scope, each time a render draws it. 1 the catalog, 2 the page tree, 3 the
/// page, 4 its content, 5 the pattern.
fn patterned_page() -> Vec<u8> {
    pdf(&[
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
             /Resources << /ColorSpace << /Lb {} >> /Pattern << /P0 5 0 R >> >>\n\
             /Contents 4 0 R >>",
            SPACES[1].1
        )),
        stream("", b"/Pattern cs /P0 scn 0 0 40 40 re f"),
        stream(
            "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10]\n\
             /XStep 10 /YStep 10",
            b"/Lb cs 50 10 10 scn 0 0 5 5 re f",
        ),
    ])
}

/// **P-02.5**: a space a scope keeps says again, each time it is found, what
/// reading it said, so a render's warnings do not depend on whether it read
/// the space or found it. A display list's replays draw the pattern's cell
/// over the scope the list keeps: the first replay reads the cell's Lab space
/// and names its repair; the second finds the space kept, reads nothing, and
/// names the repair all the same — the direct render's warnings and pixels,
/// each time.
#[test]
fn a_space_found_kept_says_what_reading_it_said() {
    let document = Document::open(patterned_page()).expect("the patterned page opens");
    let page = document.page(0).expect("its page");
    let direct = page.render(&asking());
    let list = page.display_list();
    assert!(list.is_retained(), "the list keeps the page");
    let first = list.render(&asking());
    let second = list.render(&asking());
    let parsed = |bitmap: &tinker_pdf::Bitmap| bitmap.stats.expect("asked").color_spaces_parsed;
    assert_eq!(
        parsed(&direct),
        1,
        "the direct render reads the cell's space"
    );
    assert_eq!(parsed(&first), 1, "and so does the first replay");
    assert_eq!(parsed(&second), 0, "the second finds it kept");
    assert!(
        direct.warnings.contains(&lab_repaired()),
        "{:?}",
        direct.warnings
    );
    for (what, bitmap) in [("the first replay", &first), ("the second", &second)] {
        assert_eq!(bitmap.warnings, direct.warnings, "{what}'s warnings");
        assert!(bitmap.data == direct.data, "{what}'s pixels");
    }
    println!("resource-work: RAN a kept space named its repair on both replays");
}
