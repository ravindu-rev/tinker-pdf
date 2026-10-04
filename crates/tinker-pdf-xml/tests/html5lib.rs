//! The HTML parser, held to html5lib's tree-construction tests.
//!
//! `data/html5lib-tests/tree-construction/*.dat` is the suite every HTML
//! parser is measured against: each test is an input and the tree the
//! standard builds from it, written one node a line. **That is the
//! adjudicator, and nobody here wrote it.** This file parses every test the
//! suite runs with scripting disabled through `tinker_pdf_xml::html`,
//! serialises the tree the suite's way, compares the two **exactly**, and
//! holds a counted floor — the way `commonmark_spec.rs` holds the Markdown
//! reader, except that the suite is MIT and so is vendored rather than
//! fetched.
//!
//! # What is not attempted, by name
//!
//! - **`#script-on` tests**, which describe the tree a parser with scripting
//!   enabled builds — `<noscript>` as raw text. This parser never runs a
//!   script, and a renderer with no script engine must show `<noscript>`'s
//!   content, so scripting is disabled always. A test marked neither way is
//!   run once, disabled.
//! - **`scripted/`**, the suite's tests that need `document.write` to run.
//!   They are not vendored.
//! - **Parse errors are not counted against the suite's.** A test passes on
//!   its tree; the `#errors` lines are kept by the suite to grade a
//!   conformance *checker*, and this is a parser.

use std::path::{Path, PathBuf};

use tinker_pdf_xml::html::{self, AttributeNamespace, Document, Namespace, NodeData};
use tinker_pdf_xml::Limits;

/// Tests the vendored files hold, all of them, and how many run with
/// scripting disabled.
const TESTS: usize = 1_792;
const RUN: usize = 1_784;

/// **The floor**: how many of the [`RUN`] tests build exactly the tree the
/// suite gives, measured 3 October 2026. A change that passes more raises it
/// in the same commit; one that passes fewer fails here.
///
/// **1 779 of 1 784**, and every one of the five that do not is named in
/// [`NOT_PASSING`] with its reason.
const PASSING: usize = 1_779;

/// The tests that do not pass, by file and number, each for a reason that is
/// a decision rather than a defect. Held as a list, so a change that fixes one
/// and breaks another is seen even though the count would not move.
///
/// - `tests1.dat#77` is a start tag whose one attribute's name is 1 100
///   characters long: past `Limits::max_name_len`, 1 024 bytes, which stops
///   the parse — the XML reader's cap and the XML reader's answer, kept for
///   HTML because truncating a name changes which attribute it is.
/// - `webkit02.dat#45` to `#48` are customizable `<select>`'s
///   `<selectedcontent>`, which takes a copy of the selected option's
///   content when an `<option>` is popped. That copy is a DOM behaviour the
///   parser triggers — it needs the option *selectedness* algorithm, the
///   `selected` attribute's dirtiness and the select's list of options — and
///   not tree construction; the `<selectedcontent>` element itself is built,
///   empty.
const NOT_PASSING: [&str; 5] = [
    "tests1.dat#77",
    "webkit02.dat#45",
    "webkit02.dat#46",
    "webkit02.dat#47",
    "webkit02.dat#48",
];

struct Test {
    file: String,
    number: usize,
    data: String,
    fragment: Option<String>,
    script: Option<bool>,
    document: String,
}

fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("data/html5lib-tests/tree-construction")
}

/// The suite's own format, from its README: a test starts at `#data`, each
/// heading starts a section, a section's lines run to the next heading, and
/// one blank line separates two tests.
fn tests() -> Vec<Test> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(directory())
        .expect("the vendored suite is in the tree")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "dat"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        let text = std::fs::read_to_string(&path).expect("a .dat file is UTF-8");
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        let mut number = 0;
        let mut flush = |sections: &mut Vec<(String, Vec<String>)>, out: &mut Vec<Test>| {
            if sections.is_empty() {
                return;
            }
            number += 1;
            let mut test = Test {
                file: file.clone(),
                number,
                data: String::new(),
                fragment: None,
                script: None,
                document: String::new(),
            };
            for (heading, lines) in sections.drain(..) {
                match heading.as_str() {
                    "#data" => test.data = lines.join("\n"),
                    "#document-fragment" => {
                        test.fragment = lines.first().cloned();
                    }
                    "#script-on" => test.script = Some(true),
                    "#script-off" => test.script = Some(false),
                    "#document" => {
                        let mut lines = lines;
                        // The blank line that separates this test from the next.
                        if lines.last().is_some_and(String::is_empty) {
                            lines.pop();
                        }
                        test.document = lines.join("\n");
                    }
                    _ => {}
                }
            }
            out.push(test);
        };
        for line in text.split('\n') {
            let heading = matches!(
                line,
                "#data"
                    | "#errors"
                    | "#new-errors"
                    | "#document-fragment"
                    | "#script-off"
                    | "#script-on"
                    | "#document"
            );
            if line == "#data" {
                flush(&mut sections, &mut out);
            }
            if heading {
                sections.push((line.to_owned(), Vec::new()));
            } else if let Some((_, lines)) = sections.last_mut() {
                lines.push(line.to_owned());
            }
        }
        // The file's final newline leaves one empty line after the last test.
        if let Some((heading, lines)) = sections.last_mut() {
            if heading == "#document" && lines.last().is_some_and(String::is_empty) {
                lines.pop();
            }
        }
        flush(&mut sections, &mut out);
    }
    out
}

/// The tree, written the way the suite writes it.
fn serialise(document: &Document) -> String {
    let mut lines = Vec::new();
    children(document, document.root(), 0, &mut lines);
    lines.join("\n")
}

fn children(document: &Document, parent: usize, depth: usize, lines: &mut Vec<String>) {
    let Some(node) = document.node(parent) else {
        return;
    };
    for &child in &node.children {
        let Some(child_node) = document.node(child) else {
            continue;
        };
        let indent = format!("| {}", "  ".repeat(depth));
        match &child_node.data {
            NodeData::Doctype {
                name,
                public_id,
                system_id,
            } => {
                if public_id.is_empty() && system_id.is_empty() {
                    lines.push(format!("{indent}<!DOCTYPE {name}>"));
                } else {
                    lines.push(format!(
                        "{indent}<!DOCTYPE {name} \"{public_id}\" \"{system_id}\">"
                    ));
                }
            }
            NodeData::Element(element) => {
                let prefix = match element.namespace {
                    Namespace::Html => "",
                    Namespace::Svg => "svg ",
                    Namespace::MathMl => "math ",
                };
                lines.push(format!("{indent}<{prefix}{}>", element.name));
                let mut attributes: Vec<(String, &str)> = element
                    .attributes
                    .iter()
                    .map(|a| {
                        let prefix = match a.namespace {
                            None => "",
                            Some(AttributeNamespace::XLink) => "xlink ",
                            Some(AttributeNamespace::Xml) => "xml ",
                            Some(AttributeNamespace::Xmlns) => "xmlns ",
                        };
                        (format!("{prefix}{}", a.name), a.value.as_str())
                    })
                    .collect();
                attributes.sort_by(|a, b| {
                    a.0.encode_utf16()
                        .collect::<Vec<_>>()
                        .cmp(&b.0.encode_utf16().collect::<Vec<_>>())
                });
                let inner = format!("| {}", "  ".repeat(depth + 1));
                for (name, value) in attributes {
                    lines.push(format!("{inner}{name}=\"{value}\""));
                }
                if let Some(contents) = element.template_contents {
                    lines.push(format!("{inner}content"));
                    self::children(document, contents, depth + 2, lines);
                }
                self::children(document, child, depth + 1, lines);
            }
            NodeData::Text(text) => lines.push(format!("{indent}\"{text}\"")),
            NodeData::Comment(text) => lines.push(format!("{indent}<!-- {text} -->")),
            NodeData::Document | NodeData::Fragment => {}
        }
    }
}

fn parse(test: &Test) -> Document {
    match &test.fragment {
        None => html::parse(&test.data, &Limits::DEFAULT),
        Some(context) => {
            let (namespace, name) = if let Some(name) = context.strip_prefix("svg ") {
                (Namespace::Svg, name)
            } else if let Some(name) = context.strip_prefix("math ") {
                (Namespace::MathMl, name)
            } else {
                (Namespace::Html, context.as_str())
            };
            html::parse_fragment(&test.data, (namespace, name), &Limits::DEFAULT)
        }
    }
}

/// **The counted floor over html5lib's tree-construction tests.**
#[test]
fn the_html_parser_builds_its_counted_share_of_html5libs_trees() {
    let all = tests();
    assert_eq!(all.len(), TESTS, "the vendored files hold {TESTS} tests");
    let run: Vec<&Test> = all.iter().filter(|t| t.script != Some(true)).collect();
    assert_eq!(run.len(), RUN, "{RUN} of them run with scripting disabled");

    let mut passed = 0;
    let mut by_file: Vec<(String, usize, usize)> = Vec::new();
    let mut failures = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for test in &run {
        let got = serialise(&parse(test));
        let ok = got == test.document;
        if ok {
            passed += 1;
        } else {
            failed.push(format!("{}#{}", test.file, test.number));
            failures.push(format!(
                "{}#{}: {:?}\n--- expected\n{}\n--- got\n{}",
                test.file, test.number, test.data, test.document, got
            ));
        }
        match by_file.iter_mut().find(|(f, _, _)| *f == test.file) {
            Some(entry) => {
                entry.1 += 1;
                entry.2 += usize::from(ok);
            }
            None => by_file.push((test.file.clone(), 1, usize::from(ok))),
        }
    }
    if std::env::var_os("HTML5LIB_SHOW").is_some() {
        for failure in &failures {
            println!("{failure}\n");
        }
    }
    for (file, total, ok) in &by_file {
        println!("html5lib: {file}: {ok} of {total}");
    }
    println!("html5lib: {passed} of {RUN} pass; not passing: {failed:?}");
    assert_eq!(
        passed, PASSING,
        "the floor is {PASSING} of {RUN}: a change that moves it says so in the same commit"
    );
    assert_eq!(
        failed, NOT_PASSING,
        "the tests that do not pass are not the five named"
    );
}
