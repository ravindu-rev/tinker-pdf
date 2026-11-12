//! What a page's resource scope looks up while its content runs
//! (`docs/design/performance.md`, P-02): the counts a scope makes, read off
//! MS-01's doors and held to what the page's content uses.
//!
//! Every input is written here, in memory, in the guard suite's embedded face
//! (`render_support::curvy_font`), so the default and `bundled-fonts` builds
//! count alike and the `wasm32-wasip1` leg, which has no filesystem, runs it.

mod guard_support;
mod render_support;

use guard_support::{deflated, face, object, pdf};
use tinker_pdf::{Document, TextOptions};

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
