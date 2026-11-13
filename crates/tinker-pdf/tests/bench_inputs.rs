//! The benchmark set's inputs reach every build a paired run times as the
//! bytes the first process to need them wrote, and its harness reads the
//! lists `baseline.json` commits.
//!
//! A paired run sets `TINKER_BENCH_FIXTURES` to one directory for every
//! process of all three builds (`docs/design/performance.md`, MS-02, "The
//! inputs both sides are timed on"), and `benches/support/inputs.rs` reads
//! each input from the file of its name there, writing it first when the
//! directory holds none. The tests reach that file by path, as the peak and
//! level-6 tests do, and hand its store a directory of their own, so no test
//! sets the variable the whole process shares. They reach the harness's list
//! reader, `benches/support/list.rs`, the same way.

use std::cell::Cell;
use std::path::{Path, PathBuf};

/// The benchmark set's generators and the store they are read through.
#[allow(
    dead_code,
    reason = "the store is held here, and a few of the generators feed it"
)]
#[path = "../benches/support/inputs.rs"]
mod inputs;

/// The reader of `baseline.json`'s lists the harness hands criterion.
#[allow(dead_code, reason = "its reader is held here, the committed file read")]
#[path = "../benches/support/list.rs"]
mod list;

use inputs::{Fixture, Store};
use tinker_pdf::{Document, RenderOptions};

/// An empty directory of this test's own.
fn empty(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("bench-inputs")
        .join(test);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("the test's old directory goes");
    }
    std::fs::create_dir_all(&dir).expect("the test's directory");
    dir
}

/// The names `dir` holds, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the directory reads")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("a UTF-8 name")
        })
        .collect();
    names.sort();
    names
}

#[test]
fn an_input_is_written_into_the_directory_once_and_read_back_by_every_later_call() {
    let dir = empty("written-once");
    let store = Store::shared(&dir);
    let generated = Cell::new(0);
    let generate = || {
        generated.set(generated.get() + 1);
        inputs::three_pages()
    };

    let first = store.bytes("three-pages.pdf", generate);
    assert_eq!(generated.get(), 1, "the first call generates the input");
    assert_eq!(
        first,
        inputs::three_pages(),
        "and is handed what it generated"
    );
    assert_eq!(
        std::fs::read(dir.join("three-pages.pdf")).expect("the input was written"),
        first,
        "the directory holds the bytes the first call generated"
    );

    for _ in 0..3 {
        let again = Store::shared(&dir).bytes("three-pages.pdf", || {
            panic!("an input the directory holds is never generated again")
        });
        assert_eq!(
            again, first,
            "a later call reads the first call's bytes back"
        );
    }

    // What a later call hands back is the file, whoever wrote it: a base or
    // the planning baseline reads what the head's process wrote.
    std::fs::write(dir.join("three-pages.pdf"), b"the head's bytes").expect("rewritten");
    assert_eq!(
        store.bytes("three-pages.pdf", || panic!("never generated again")),
        b"the head's bytes",
        "a later call reads the file the directory holds now"
    );
    assert_eq!(generated.get(), 1, "the input was generated once in all");
    assert_eq!(
        names(&dir),
        ["three-pages.pdf"],
        "the input alone was written, whole, under its own name"
    );
}

#[test]
fn a_file_a_child_reads_is_the_one_the_first_call_wrote() {
    let dir = empty("child-file");
    let store = Store::shared(&dir);
    let path = store.file("small-3.pdf", || inputs::small(3));
    assert_eq!(path, dir.join("small-3.pdf"), "the file is the directory's");
    assert_eq!(
        std::fs::read(&path).expect("it was written"),
        inputs::small(3)
    );
    let again = store.file("small-3.pdf", || {
        panic!("a file there is never written again")
    });
    assert_eq!(again, path, "a later call hands back the same file");

    // A file the in-process store wrote is the one a child reads, and the
    // other way round, so an operation and its CLI twin time one input.
    let read = store.bytes("small-3.pdf", || panic!("the file is there"));
    assert_eq!(read, inputs::small(3));
    let bytes = store.bytes("dense-1.pdf", || inputs::dense(1, 0));
    let file = store.file("dense-1.pdf", || panic!("the bytes were written"));
    assert_eq!(std::fs::read(file).expect("it reads"), bytes);
    assert_eq!(names(&dir), ["dense-1.pdf", "small-3.pdf"]);
}

#[test]
fn a_set_a_scan_text_and_a_bitmap_come_back_as_they_went_in() {
    let dir = empty("shapes");
    let store = Store::shared(&dir);

    let set = || {
        vec![
            Fixture {
                name: "b.svg".to_string(),
                bytes: b"<svg/>".to_vec().into(),
            },
            Fixture {
                name: "a.svg".to_string(),
                bytes: Vec::new().into(),
            },
        ]
    };
    for _ in 0..2 {
        let read = store.set("svg.bundle", set);
        let read: Vec<(&str, &[u8])> = read
            .iter()
            .map(|fixture| (fixture.name.as_str(), &fixture.bytes[..]))
            .collect();
        assert_eq!(
            read,
            [("b.svg", &b"<svg/>"[..]), ("a.svg", &b""[..])],
            "the set's files, by name and in its order"
        );
    }

    let bits: Vec<u8> = (0u8..6).collect();
    for _ in 0..2 {
        let (width, height, read) = store.scan("scan.bits", || (10, 3, bits.clone().into()));
        assert_eq!((width, height, &read[..]), (10, 3, &bits[..]));
    }

    for _ in 0..2 {
        assert_eq!(
            store.text("words.txt", || "crème brûlée".to_string()),
            "crème brûlée"
        );
    }

    let render = || {
        Document::open(inputs::near_blank())
            .expect("the page opens")
            .page(0)
            .expect("a page")
            .render(&RenderOptions::at_dpi(36.0))
    };
    let drawn = render();
    for _ in 0..2 {
        let read = store.bitmap("near-blank.pixels", render);
        assert_eq!(
            (
                read.width,
                read.height,
                read.format,
                read.stride,
                read.premultiplied
            ),
            (
                drawn.width,
                drawn.height,
                drawn.format,
                drawn.stride,
                drawn.premultiplied
            ),
            "the bitmap's shape"
        );
        assert!(read.data == drawn.data, "the bitmap's pixels");
        assert!(
            read.warnings.is_empty(),
            "a stored bitmap carries no warnings"
        );
        assert_eq!(
            read.to_png(),
            drawn.to_png(),
            "and encodes as the render does"
        );
    }
    assert_eq!(
        names(&dir),
        ["near-blank.pixels", "scan.bits", "svg.bundle", "words.txt"]
    );
}

#[test]
fn a_bundle_holds_its_parts_and_nothing_else() {
    let bundle = inputs::bundle(&[("width", &7u32.to_le_bytes()), ("", b"")]);
    assert_eq!(
        inputs::unbundle(&bundle, "two parts"),
        [
            ("width".to_string(), 7u32.to_le_bytes().to_vec()),
            (String::new(), Vec::new())
        ]
    );
    assert!(inputs::unbundle(&inputs::bundle(&[]), "none").is_empty());
}

#[test]
#[should_panic(expected = "is no bundle")]
fn a_file_that_is_no_bundle_is_refused() {
    inputs::unbundle(b"%PDF-1.7", "a PDF");
}

#[test]
#[should_panic(expected = "ends inside a part")]
fn a_bundle_cut_short_is_refused() {
    let bundle = inputs::bundle(&[("bits", &[1, 2, 3])]);
    inputs::unbundle(&bundle[..bundle.len() - 1], "a short bundle");
}

#[test]
#[should_panic(expected = "ends inside a length")]
fn a_bundle_cut_inside_a_length_is_refused() {
    let bundle = inputs::bundle(&[("bits", &[1, 2, 3])]);
    inputs::unbundle(&bundle[..bundle.len() - 9], "a short bundle");
}

#[test]
#[should_panic(expected = "holds every row of its bits")]
fn a_scan_short_of_its_rows_is_refused() {
    let dir = empty("short-scan");
    Store::shared(&dir).scan("scan.bits", || (10, 3, vec![0; 5].into()));
}

#[test]
#[should_panic(expected = "the parts of the bundle")]
fn a_bitmap_of_other_parts_is_refused() {
    let dir = empty("other-bitmap");
    std::fs::write(
        dir.join("page.pixels"),
        inputs::bundle(&[("width", &1u32.to_le_bytes())]),
    )
    .expect("written");
    Store::shared(&dir).bitmap("page.pixels", || panic!("the file is there"));
}

/// A list file with every shape of value the reader passes over.
const LISTS: &str = r#"{
  "note": ["a \"quoted\" line\twith escapes é 😀", "x"],
  "calibration": { "operation": "calibration/text of 5 dense pages",
                   "planning_ms": 37.6 },
  "groups": [
    { "group": "open", "operations": [
        { "name": "open the AES-256 file", "unit": "none",
          "per_iteration": 6, "calls": 6, "gate": { "floor": 1 } },
        { "name": "open a 3-page document", "unit": "bytes",
          "per_iteration": 10985, "calls": 1,
          "gate": { "held": true, "relations": [ { "to": "x", "at_most": 3.6e0 } ] },
          "paired": null }
    ] },
    { "group": "text", "operations": [] }
  ],
  "machines": [ { "machine": "m", "id": "open/x", "nanos": -1.5E+3 } ]
}"#;

#[test]
fn a_list_is_read_by_its_group_whatever_surrounds_it() {
    assert_eq!(
        list::list(LISTS, "open"),
        Ok(vec![
            list::Listed {
                name: "open the AES-256 file".to_string(),
                unit: "none".to_string(),
                per_iteration: 6,
            },
            list::Listed {
                name: "open a 3-page document".to_string(),
                unit: "bytes".to_string(),
                per_iteration: 10985,
            },
        ])
    );
    assert_eq!(list::list(LISTS, "text"), Ok(Vec::new()));
}

#[test]
fn a_list_the_reader_cannot_read_is_refused_by_what_is_wrong() {
    let refused =
        |text: &str, group: &str| list::list(text, group).expect_err("the reader refuses it");
    assert!(refused(LISTS, "render").contains("no `render` list"));
    assert!(refused("{}", "open").contains("no `groups` list"));
    assert!(refused(&format!("{LISTS} x"), "open").contains("goes on past its value"));
    assert!(refused(&LISTS.replace("10985", "10985.5"), "open").contains("no count"));
    assert!(refused(&LISTS.replace("\"unit\": \"bytes\",", ""), "open").contains("no `unit`"));
    assert!(refused(&LISTS.replace("\"x\"]", "\"x\""), "open").contains("in an array"));
    assert!(refused(&LISTS[..LISTS.len() / 2], "open").contains("at byte"));
}

/// What a wasm build of the harness leaves out is read from the committed
/// `wasm.json` by the harness's own reader, and every id it reads is an
/// operation of a committed list, so a leg leaves out what the file names
/// and nothing a slip in it would hide.
#[test]
fn the_wasm_exclusions_read_as_operations_of_the_lists() {
    let excluded = list::excluded_on_wasm();
    assert!(!excluded.is_empty(), "the committed exclusions");
    for id in &excluded {
        let (group, name) = id.split_once('/').expect("an id is <group>/<name>");
        assert!(
            list::committed(group)
                .iter()
                .any(|listed| listed.name == name),
            "{id} is an operation of the `{group}` list"
        );
    }
    assert!(list::excluded("{}")
        .expect_err("no list")
        .contains("no `excluded` list"));
    assert!(
        list::excluded(r#"{ "excluded": [ { "why": "threads" } ] }"#)
            .expect_err("no id")
            .contains("entry 0 has no `id` string")
    );
}

/// The committed files are read where the process finds the crate: where
/// cargo built it unless `TINKER_BENCH_CRATE_DIR` says otherwise, as a wasm
/// leg does. Unset here, so every read is the checkout's own.
#[test]
fn a_committed_file_is_read_under_the_crate_where_cargo_built_it() {
    assert!(std::env::var_os(inputs::CRATE_DIR).is_none());
    assert_eq!(
        inputs::in_tree("benches/baseline.json"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/baseline.json")
    );
    assert!(inputs::in_tree("benches/wasm.json").is_file());
}

#[test]
fn every_group_file_has_a_committed_list() {
    let benches = Path::new(env!("CARGO_MANIFEST_DIR")).join("benches");
    let mut groups: Vec<String> = std::fs::read_dir(&benches)
        .expect("the bench directory")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .filter_map(|path| Some(path.file_stem()?.to_str()?.to_string()))
        .collect();
    groups.sort();
    assert!(groups.len() > 10, "the group files: {groups:?}");
    for group in &groups {
        let listed = list::committed(group);
        assert!(!listed.is_empty(), "the `{group}` list holds operations");
        for operation in &listed {
            assert!(
                ["none", "pages", "pixels", "bytes", "outlines"].contains(&operation.unit.as_str()),
                "{group}/{}: the unit `{}`",
                operation.name,
                operation.unit
            );
            assert!(operation.per_iteration > 0, "{group}/{}", operation.name);
        }
    }
}
