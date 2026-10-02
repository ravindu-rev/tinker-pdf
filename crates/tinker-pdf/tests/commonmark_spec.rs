//! The Markdown reader, held to CommonMark 0.31.2's own examples (tier 5's
//! formats row).
//!
//! `spec.txt` at the `0.31.2` tag of `commonmark/commonmark-spec` carries 652
//! examples, each a Markdown input and the HTML it must produce; the
//! repository's `test/spec_tests.py --dump-tests` writes the same 652 out as
//! the published `spec.json`, by the rule [`examples`] restates. **That is the
//! adjudicator, and nobody here wrote it.** This file runs every example
//! through `tinker_pdf::markdown::to_html`, compares the output **exactly** —
//! stricter than `spec_tests.py`, which normalises white space first — and
//! holds a counted floor: the total, and the sections that pass whole.
//!
//! # Fetched, never committed
//!
//! `spec.txt` is CC-BY-SA 4.0, and `deny.toml`'s "deliberately NO copyleft —
//! not even weak copyleft" rule bars share-alike material from this
//! repository, as it barred `epub3-samples` (`tests/epub/fetch-corpus.sh`).
//! So it is fetched by `tests/commonmark/fetch-spec.sh` into `target/`, its
//! SHA-256 is checked here against the one recorded below, and this test reads
//! `TINKER_COMMONMARK_SPEC` — an **absolute** path to the file — and prints
//! [`RAN`] or [`SKIPPED`]. `TINKER_COMMONMARK_SPEC_REQUIRED=1` makes a skip a
//! failure, which is what the CI job sets.

use std::path::PathBuf;

use tinker_pdf::markdown::to_html;

/// Printed when the examples were read. CI greps for it.
const RAN: &str = "commonmark-spec: RAN";

/// Printed when they could not be. CI greps for it too, and fails.
const SKIPPED: &str = "commonmark-spec: SKIPPED";

/// SHA-256 of `spec.txt` at the 0.31.2 tag, fetched 2 October 2026 from
/// `https://raw.githubusercontent.com/commonmark/commonmark-spec/0.31.2/spec.txt`.
const SPEC_SHA256: &str = "257c41ad946f7a1414a499aca402a1aa8fdac3678532266611348c1cf54f4b80";

/// The examples 0.31.2 carries.
const EXAMPLES: usize = 652;

/// SHA-256 over every example's input, a NUL, its output, a NUL, its section
/// and a `0x01`, in order — computed on 2 October 2026 over the 652 objects
/// `python3 test/spec_tests.py --dump-tests --spec spec.txt` (Python 3.11.2,
/// the 0.31.2 tag's own script) writes as `spec.json`. Recomputing it here
/// over what [`examples`] extracts is what says this file reads the same 652
/// examples as the published `spec.json`, not merely as many.
const EXAMPLES_SHA256: &str = "68a4c06ba14feec70062206e8176cd5079ceaad31ccc5b2ca86b0d52c8c6bc06";

/// The floor: how many examples pass exactly, measured 2 October 2026. A
/// change that passes more raises it in the same commit; one that passes fewer
/// fails here.
///
/// **651 of 652.** The one that does not is example 25, in *Entity and numeric
/// character references*: it names `&Dcaron;`, `&HilbertSpace;`,
/// `&DifferentialD;`, `&ClockwiseContourIntegral;` and `&ngE;`, which are in
/// HTML's 2 231 names and not in XHTML 1.0's 253 — the table this reader
/// resolves from, because it is the one this repository vendors. Its other
/// five references (`&nbsp;`, `&amp;`, `&copy;`, `&AElig;`, `&frac34;`)
/// resolve, so the example fails on the list and on nothing else.
const PASSING: usize = 651;

/// Sections every example of which passes, measured with [`PASSING`]: all
/// twenty-six of 0.31.2's but the entity section, which is 16 of 17. Each is a
/// claim of its own, because a section can lose an example while the total
/// holds by another gaining one.
const WHOLE_SECTIONS: &[&str] = &[
    "Tabs",
    "Backslash escapes",
    "Precedence",
    "Thematic breaks",
    "ATX headings",
    "Setext headings",
    "Indented code blocks",
    "Fenced code blocks",
    "HTML blocks",
    "Link reference definitions",
    "Paragraphs",
    "Blank lines",
    "Block quotes",
    "List items",
    "Lists",
    "Inlines",
    "Code spans",
    "Emphasis and strong emphasis",
    "Links",
    "Images",
    "Autolinks",
    "Raw HTML",
    "Hard line breaks",
    "Soft line breaks",
    "Textual content",
];

fn required() -> bool {
    std::env::var_os("TINKER_COMMONMARK_SPEC_REQUIRED").is_some_and(|value| value != "0")
}

/// One example: the section it is in, its number, its input and its output.
struct Example {
    section: String,
    number: usize,
    markdown: String,
    html: String,
}

/// `spec_tests.py`'s `get_tests`, restated: a line that is 32 backticks and
/// ` example` opens one, a line that is `.` separates input from output, a
/// line of 32 backticks closes it, `→` stands for a tab in both, and the
/// section is the text of the last ATX heading seen outside an example.
fn examples(spec: &str) -> Vec<Example> {
    let fence = "`".repeat(32);
    let open = format!("{fence} example");
    let mut out = Vec::new();
    let mut state = 0;
    let mut section = String::new();
    let (mut markdown, mut html) = (String::new(), String::new());
    for raw in spec.split_inclusive('\n') {
        let line = raw.trim();
        if line == open {
            state = 1;
        } else if state == 2 && line == fence {
            state = 0;
            out.push(Example {
                section: section.clone(),
                number: out.len() + 1,
                markdown: std::mem::take(&mut markdown).replace('→', "\t"),
                html: std::mem::take(&mut html).replace('→', "\t"),
            });
        } else if line == "." && state == 1 {
            state = 2;
        } else if state == 1 {
            markdown.push_str(raw);
        } else if state == 2 {
            html.push_str(raw);
        } else if state == 0 {
            let hashes = raw.bytes().take_while(|&b| b == b'#').count();
            if hashes > 0 && raw.as_bytes().get(hashes) == Some(&b' ') {
                section = raw[hashes..].trim().to_owned();
            }
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn spec() -> Option<String> {
    let set = std::env::var_os("TINKER_COMMONMARK_SPEC");
    if let Some(path) = &set {
        let path = PathBuf::from(path);
        assert!(
            path.is_absolute() || !required(),
            "TINKER_COMMONMARK_SPEC is {path:?}, which is relative: a test \
             binary runs from its crate directory. Pass an absolute path."
        );
    }
    let bytes = std::fs::read(PathBuf::from(set?)).ok()?;
    let digest = hex(&tinker_pdf_crypto::sha2::sha256(&bytes));
    assert_eq!(
        digest, SPEC_SHA256,
        "the spec.txt handed in is not 0.31.2's: a different revision's examples \
         would move the floor for a reason that is not this reader"
    );
    String::from_utf8(bytes).ok()
}

/// **The counted floor over CommonMark 0.31.2's 652 examples.**
#[test]
fn the_markdown_reader_passes_its_counted_share_of_the_commonmark_examples() {
    let Some(text) = spec() else {
        println!("{SKIPPED}: TINKER_COMMONMARK_SPEC names no readable spec.txt");
        assert!(
            !required(),
            "TINKER_COMMONMARK_SPEC_REQUIRED is set and there is no spec to read"
        );
        return;
    };
    let all = examples(&text);
    assert_eq!(all.len(), EXAMPLES, "0.31.2 carries {EXAMPLES} examples");
    let mut fingerprint = Vec::new();
    for example in &all {
        fingerprint.extend_from_slice(example.markdown.as_bytes());
        fingerprint.push(0);
        fingerprint.extend_from_slice(example.html.as_bytes());
        fingerprint.push(0);
        fingerprint.extend_from_slice(example.section.as_bytes());
        fingerprint.push(1);
    }
    assert_eq!(
        hex(&tinker_pdf_crypto::sha2::sha256(&fingerprint)),
        EXAMPLES_SHA256,
        "the examples read here are not the 652 spec_tests.py dumps as spec.json"
    );

    let mut sections: Vec<(String, usize, usize)> = Vec::new();
    let mut passed = 0;
    let mut failures = Vec::new();
    for example in &all {
        let got = to_html(&example.markdown);
        let ok = got == example.html;
        if ok {
            passed += 1;
        } else {
            failures.push(example.number);
        }
        match sections
            .iter_mut()
            .find(|(name, _, _)| *name == example.section)
        {
            Some(slot) => {
                slot.1 += usize::from(ok);
                slot.2 += 1;
            }
            None => sections.push((example.section.clone(), usize::from(ok), 1)),
        }
    }
    println!("{RAN}: {passed} of {} examples pass exactly", all.len());
    for (name, ok, total) in &sections {
        println!("  {ok:>4} / {total:<4} {name}");
    }
    if std::env::var_os("TINKER_COMMONMARK_SPEC_FAILURES").is_some() {
        for number in &failures {
            let example = &all[number - 1];
            println!(
                "--- example {number} ({})\n{:?}\nwant {:?}\ngot  {:?}",
                example.section,
                example.markdown,
                example.html,
                to_html(&example.markdown)
            );
        }
    }
    assert!(
        passed >= PASSING,
        "{passed} of {EXAMPLES} pass, below the recorded floor of {PASSING}"
    );
    for whole in WHOLE_SECTIONS {
        let (_, ok, total) = sections
            .iter()
            .find(|(name, _, _)| name == whole)
            .unwrap_or_else(|| panic!("0.31.2 has no section {whole:?}"));
        assert_eq!(
            ok, total,
            "{whole}: {ok} of {total}, where every one passed"
        );
    }
}
