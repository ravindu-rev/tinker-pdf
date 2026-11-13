//! The coverage check held to small trees whose answers are written out here,
//! and its readers to the repository's own surfaces.

use super::*;

fn tree(files: &[(&str, &str)]) -> Tree {
    files
        .iter()
        .map(|(path, text)| ((*path).to_string(), (*text).to_string()))
        .collect()
}

/// A facade of every shape the walk meets: a definition at the root with
/// public, restricted, private and hidden methods and a trait impl; a free
/// function and a constant; a private module re-exported by name and under
/// an alias, and again from a public module; another crate's type through a
/// glob, with methods in a second file; another crate's module by path; and
/// test and hidden items, which are not the surface.
fn facade_tree() -> Tree {
    tree(&[
        (
            "crates/tinker-pdf/src/lib.rs",
            "//! The facade.\n\
             pub mod pdfa;\n\
             mod annotations;\n\
             pub use annotations::{Annotation, Kind as AnnotationKind};\n\
             pub use tinker_pdf_cos::{DocumentBuilder, pages::PageTree};\n\
             /// An open document.\n\
             #[derive(Clone)]\n\
             pub struct Document { inner: u8 }\n\
             impl Document {\n\
                 /// Opens one.\n\
                 pub fn open(bytes: &[u8]) -> Result<Document, String> { Ok(Document { inner: 0 }) }\n\
                 pub fn page_count(&self) -> u32 { if self.inner > 0 { 1 } else { 0 } }\n\
                 pub(crate) fn internal(&self) {}\n\
                 fn private(&self) {}\n\
                 #[doc(hidden)]\n\
                 pub fn hidden(&self) {}\n\
             }\n\
             impl core::fmt::Debug for Document {\n\
                 fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { Ok(()) }\n\
             }\n\
             pub fn sniff(bytes: &[u8]) -> bool { bytes.starts_with(b\"%PDF\") }\n\
             pub const MAX: usize = [1, 2][0];\n\
             #[cfg(test)]\n\
             mod tests { pub struct Hidden; }\n\
             #[doc(hidden)]\n\
             pub struct Secret;\n\
             pub(crate) struct Inner;\n",
        ),
        (
            "crates/tinker-pdf/src/annotations.rs",
            "pub struct Annotation { pub kind: Kind, hidden: u8 }\n\
             impl Annotation { pub fn contents(&self) -> &str { \"{\" } }\n\
             pub enum Kind {\n    \
                 /// Text.\n    \
                 Text,\n    \
                 #[allow(dead_code)]\n    \
                 Link { uri: String },\n    \
                 Other(Vec<(u8, u8)>),\n\
             }\n",
        ),
        (
            "crates/tinker-pdf/src/pdfa.rs",
            "pub use crate::annotations::Annotation;\n\
             pub struct Profile;\n\
             impl Profile { pub fn name(&self) -> &'static str { \"a-2b\" } }\n",
        ),
        (
            "crates/tinker-pdf-cos/src/lib.rs",
            "mod build;\npub mod pages;\npub use build::*;\n",
        ),
        (
            "crates/tinker-pdf-cos/src/build.rs",
            "mod more;\n\
             pub struct DocumentBuilder;\n\
             impl DocumentBuilder {\n    \
                 pub fn new() -> Self { DocumentBuilder }\n    \
                 pub fn finish(self) -> Vec<u8> { Vec::new() }\n\
             }\n",
        ),
        (
            "crates/tinker-pdf-cos/src/build/more.rs",
            "impl super::DocumentBuilder {\n    pub fn add_page(&mut self) {}\n}\n",
        ),
        (
            "crates/tinker-pdf-cos/src/build/tests.rs",
            "impl super::DocumentBuilder { pub fn only_in_tests(&self) {} }\n",
        ),
        (
            "crates/tinker-pdf-cos/src/pages.rs",
            "pub struct PageTree<'a> { pub root: &'a str }\n\
             impl<'a> PageTree<'a> where 'a: 'static { pub fn count(&self) -> u32 { 0 } }\n",
        ),
    ])
}

/// **The walk finds every public item, and only those**: definitions and
/// their public inherent methods, re-exports followed through a private
/// module, an alias, a glob and another crate's module, methods from a second
/// file of the defining crate; one item reached by two paths under the
/// shorter; and never a restricted, private, hidden or test item, a trait's
/// method or a method in a test file. The variants and public fields come
/// with their types, for the ledger to name.
#[test]
fn the_walk_finds_every_public_item_and_only_those() {
    let found = facade(&facade_tree());
    let paths: Vec<&str> = found.items.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        vec![
            "Annotation",
            "Annotation::contents",
            "AnnotationKind",
            "Document",
            "Document::open",
            "Document::page_count",
            "DocumentBuilder",
            "DocumentBuilder::add_page",
            "DocumentBuilder::finish",
            "DocumentBuilder::new",
            "MAX",
            "PageTree",
            "PageTree::count",
            "pdfa::Profile",
            "pdfa::Profile::name",
            "sniff",
        ]
    );
    assert_eq!(found.items["Document"].kind, Kind::Type);
    assert_eq!(found.items["Document::open"].kind, Kind::Method);
    assert_eq!(
        found.items["Document::open"].owner.as_deref(),
        Some("Document")
    );
    assert_eq!(found.items["sniff"].kind, Kind::Function);
    assert_eq!(found.items["MAX"].kind, Kind::Constant);
    assert_eq!(found.items["AnnotationKind"].name, "Kind");
    let parts: Vec<&str> = found.parts.iter().map(String::as_str).collect();
    assert_eq!(
        parts,
        vec![
            "Annotation::kind",
            "AnnotationKind::Link",
            "AnnotationKind::Other",
            "AnnotationKind::Text",
            "PageTree::root",
        ]
    );
}

/// **A re-export the walk cannot follow is still an item**, under the name
/// it is brought in as, so a surface is held to it rather than it being
/// dropped.
#[test]
fn an_unfollowed_reexport_is_still_an_item() {
    let found = facade(&tree(&[(
        "crates/tinker-pdf/src/lib.rs",
        "pub use somewhere_else::Mystery;\npub use std::sync::Arc as Shared;\n",
    )]));
    let paths: Vec<&str> = found.items.keys().map(String::as_str).collect();
    assert_eq!(paths, vec!["Mystery", "Shared"]);
}

/// **An `impl` block's type**: its own generics and the type's skipped, a
/// path's last segment, a `where` clause ignored, and a trait's impl none.
#[test]
fn an_impl_block_names_its_type() {
    for (head, want) in [
        ("impl Foo {}", Some("Foo")),
        ("impl<'a> Foo<'a> {}", Some("Foo")),
        (
            "impl<T: Into<String>> Bar<T> where T: Clone {}",
            Some("Bar"),
        ),
        ("impl super::Foo {}", Some("Foo")),
        ("impl<F: Fn() -> u8> Baz<F> {}", Some("Baz")),
        ("impl core::fmt::Display for Foo {}", None),
        ("impl<T> From<T> for Foo {}", None),
    ] {
        let src = Source::new(head);
        let item = items(&src, 0, src.toks.len())
            .into_iter()
            .next()
            .expect("an item");
        assert_eq!(item.keyword, "impl", "{head}");
        assert_eq!(impl_self(&src, item.head), want, "{head}");
    }
}

/// **A `use` tree's leaves**: nested braces, aliases, globs and `self`.
#[test]
fn a_use_tree_has_its_leaves() {
    let src = Source::new("use a::{b, c as d, e::{f, g::*}, self};");
    let item = items(&src, 0, src.toks.len())
        .into_iter()
        .next()
        .expect("the use");
    let leaves = use_leaves(&src, item.head.0, item.head.1);
    let shown: Vec<String> = leaves
        .iter()
        .map(|leaf| {
            let mut text = leaf.path.join("::");
            if leaf.glob {
                text.push_str("::*");
            }
            if let Some(alias) = &leaf.alias {
                text.push_str(&format!(" as {alias}"));
            }
            text
        })
        .collect();
    assert_eq!(
        shown,
        vec!["a::b", "a::c as d", "a::e::f", "a::e::g::*", "a::self"]
    );
}

const HEADER_TEXT: &str = "#include <stdint.h>\n\
    typedef struct TpdfDocument TpdfDocument;\n\
    typedef struct TpdfBitmap TpdfBitmap;\n\
    typedef enum TpdfPixelFormat { TpdfPixelFormat_Rgb8 = 2 } TpdfPixelFormat;\n\
    /* TpdfStatus tpdf_commented(void); */\n\
    TpdfStatus tpdf_document_open(const uint8_t *bytes, uintptr_t len, TpdfDocument **out);\n\
    // a line comment naming tpdf_page_size(\n\
    TpdfStatus tpdf_page_render(const TpdfDocument *doc,\n    \
        uint32_t index,\n    \
        double scale,\n    \
        TpdfPixelFormat format,\n    \
        TpdfBitmap **out);\n\
    uint32_t tpdf_bitmap_width(const TpdfBitmap *bitmap);\n";

/// **The header's functions, with the types their declarations name**: a
/// declaration across lines whole, its return type included, and nothing
/// from a comment.
#[test]
fn the_header_has_its_functions_and_the_types_they_take() {
    let (functions, types) = header_exports(HEADER_TEXT);
    let names: Vec<&str> = functions.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        vec![
            "tpdf_bitmap_width",
            "tpdf_document_open",
            "tpdf_page_render"
        ]
    );
    let render: Vec<&str> = functions["tpdf_page_render"]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        render,
        vec![
            "TpdfBitmap",
            "TpdfDocument",
            "TpdfPixelFormat",
            "TpdfStatus"
        ]
    );
    for wanted in [
        "TpdfDocument",
        "TpdfBitmap",
        "TpdfPixelFormat",
        "TpdfStatus",
    ] {
        assert!(types.contains(wanted), "{wanted}");
    }
}

/// **A wrapper has what its own code calls**: a C# and a Ruby call through
/// `Native`, a Go call through `C`, a Java call through `Native` from a
/// public class; not a declaration transcribed from the header (C#'s and
/// Ruby's `extern`, Java's `Native.java`), a comment, a test, or Go's
/// commands.
#[test]
fn a_wrapper_has_what_its_code_calls() {
    let files = tree(&[
        (
            "bindings/dotnet/TinkerPdf.cs",
            "static class Native {\n\
             [DllImport(Lib)] internal static extern int tpdf_document_open(byte[] b, nuint n, out IntPtr d);\n\
             [DllImport(Lib)] internal static extern int tpdf_page_render(IntPtr d);\n\
             }\n\
             // Native.tpdf_page_render(doc) is not called\n\
             class Document { void Open() { Native.Check(Native.tpdf_document_open(b, n, out var d)); } }\n",
        ),
        (
            "bindings/dotnet/tests/Smoke.cs",
            "Native.tpdf_bitmap_width(b);\n",
        ),
        (
            "bindings/go/read.go",
            "/*\n#include \"tinker_pdf.h\"\n*/\nimport \"C\"\n\
             func Open() { C.tpdf_document_open(nil, 0, nil) }\n",
        ),
        (
            "bindings/go/read_test.go",
            "func TestX() { C.tpdf_page_render(nil) }\n",
        ),
        (
            "bindings/go/cmd/parity/main.go",
            "func main() { C.tpdf_bitmap_width(nil) }\n",
        ),
        (
            "bindings/ruby/lib/tinker_pdf.rb",
            "module Native\n  \
               extern 'int tpdf_document_open(const uint8_t *b, size_t n, void **out)'\n  \
               extern 'int tpdf_page_render(const void *doc)'\n\
             end\n\
             # Native.tpdf_page_render(doc) in a comment\n\
             def open(b) Native.tpdf_document_open(b, b.bytesize, out) end # and '#' here\n",
        ),
        (
            "bindings/java/src/io/x/Native.java",
            "final class Native { static final MethodHandle tpdf_page_render = handle(\"tpdf_page_render\"); }\n",
        ),
        (
            "bindings/java/src/io/x/Document.java",
            "class Document { void open() { Native.callInt(Native.tpdf_document_open, a); } }\n",
        ),
    ]);
    for surface in WRAPPERS {
        let mut calls = BTreeSet::new();
        for (path, text) in &files {
            if let Some(found) = wrapper_calls(surface, path, text) {
                calls.extend(found);
            }
        }
        let calls: Vec<&str> = calls.iter().map(String::as_str).collect();
        assert_eq!(calls, vec!["tpdf_document_open"], "{}", surface.name());
    }
}

/// **Python and JavaScript have their classes, methods and functions**: a
/// PyO3 class by its Python name with its methods from another file, a
/// rename and a constructor; a wasm-bindgen class by its Rust name and its
/// `js_name`, its public methods by their Rust names and `js_name`s, and an
/// exported function.
#[test]
fn python_and_javascript_have_their_classes_methods_and_functions() {
    let mut python = Binding::default();
    python_exports(
        &[
            "#[pyclass(name = \"Document\")]\npub struct PyDocument { inner: u8 }\n\
             #[pyfunction]\nfn version() -> &'static str { \"1\" }\n",
            "#[pymethods]\nimpl PyDocument {\n    \
                 #[new]\n    fn new(data: &[u8]) -> Self { todo!() }\n    \
                 #[getter]\n    fn page_count(&self) -> u32 { 0 }\n    \
                 #[pyo3(name = \"text_of\")]\n    fn page_text(&self) -> String { String::new() }\n\
             }\n",
        ],
        &mut python,
    );
    let methods: Vec<&str> = python.classes["Document"]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        methods,
        vec!["__new__", "new", "page_count", "page_text", "text_of"]
    );
    assert!(!python.classes.contains_key("PyDocument"));
    assert!(python.functions.contains("version"));

    let mut javascript = Binding::default();
    javascript_exports(
        "#[wasm_bindgen(js_name = PdfDoc)]\npub struct PdfDocument { inner: u8 }\n\
         #[wasm_bindgen]\nimpl PdfDocument {\n    \
             #[wasm_bindgen(constructor)]\n    pub fn new() -> PdfDocument { todo!() }\n    \
             #[wasm_bindgen(js_name = pageCount)]\n    pub fn page_count(&self) -> u32 { 0 }\n    \
             fn private(&self) {}\n\
         }\n\
         #[wasm_bindgen]\npub fn version() -> String { String::new() }\n",
        &mut javascript,
    );
    let methods: Vec<&str> = javascript.classes["PdfDocument"]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(methods, vec!["new", "pageCount", "page_count"]);
    assert!(javascript.classes.contains_key("PdfDoc"));
    assert!(javascript.functions.contains("version"));
}

const LEDGER_TEXT: &str = "# Bindings\n\
    \n\
    ## Projections\n\
    \n\
    | Facade | C ABI | Python | JavaScript |\n\
    | --- | --- | --- | --- |\n\
    | `DocumentEditor` | `TpdfEditor` | `Editor` | `PdfEditor` |\n\
    | `Page::render` | `tpdf_page_render` | `Document.render` | `PdfDocument.render_page` |\n\
    | `Document::page_count` | | — | |\n\
    \n\
    ## Owed\n\
    \n\
    | Step | Items | Surfaces |\n\
    | --- | --- | --- |\n\
    | SU-12.3 | `PageBuilder::glyphs`, `Target::Named` | every |\n\
    | SU-12.14 | `Signature::*` | C ABI, .NET, Go | each a shape of its own |\n\
    \n\
    ## Refused by name\n\
    \n\
    | What | How it shows | Why | See |\n\
    | --- | --- | --- | --- |\n\
    | `ImageData::Compressed` | not projected | a sub-surface | — |\n\
    | Holding a wasm `view()` | detached | memory grows | — |\n";

/// **The ledger reads its three tables**: projection cells empty, absent or
/// named; owed rows with their step, items and surfaces; and every
/// backticked name in the refusal table's What column.
#[test]
fn the_ledger_reads_its_three_tables() {
    let read = ledger(LEDGER_TEXT);
    assert!(read.problems.is_empty(), "{:?}", read.problems);
    assert_eq!(read.projections.len(), 3);
    assert_eq!(read.projections[0].item, "DocumentEditor");
    assert_eq!(
        read.projections[0].c,
        Cell::Names(vec!["TpdfEditor".into()])
    );
    assert_eq!(read.projections[2].c, Cell::Join);
    assert_eq!(read.projections[2].python, Cell::Absent);
    assert_eq!(read.owed.len(), 2);
    assert_eq!(read.owed[0].step, "SU-12.3");
    assert_eq!(
        read.owed[0].items,
        vec!["PageBuilder::glyphs", "Target::Named"]
    );
    assert_eq!(read.owed[0].surfaces, SURFACES.to_vec());
    assert_eq!(
        read.owed[1].surfaces,
        vec![Surface::CAbi, Surface::DotNet, Surface::Go]
    );
    assert_eq!(read.refused, vec!["ImageData::Compressed", "view()"]);

    let bad = ledger(
        "## Owed\n\n| Step | Items | Surfaces |\n| --- | --- | --- |\n\
         | SU-12.3 | `X` | Fortran |\n| SU-12.4 | nothing | every |\n\
         ## Projections\n\n| Facade | C ABI |\n| --- | --- |\n| `X` | `y` |\n",
    );
    assert_eq!(bad.problems.len(), 3, "{:?}", bad.problems);
}

/// Items for the join's tests, each named as the walk names them.
fn join_items() -> Items {
    let mut out = Items::default();
    let mut add = |path: &str, kind: Kind, owner: Option<&str>| {
        let name = path.rsplit("::").next().unwrap_or(path).to_string();
        out.items.insert(
            path.to_string(),
            Item {
                path: path.to_string(),
                kind,
                name,
                owner: owner.map(str::to_string),
            },
        );
    };
    add("Document", Kind::Type, None);
    add("Document::open", Kind::Method, Some("Document"));
    add("Document::page_count", Kind::Method, Some("Document"));
    add("DocumentEditor", Kind::Type, None);
    add("DocumentEditor::save", Kind::Method, Some("DocumentEditor"));
    add("Page", Kind::Type, None);
    add("Page::render", Kind::Method, Some("Page"));
    add("PixelFormat", Kind::Type, None);
    add("sniff", Kind::Function, None);
    out.parts.insert("Target::Named".to_string());
    out
}

/// Surfaces for the join's tests: every one has `Document::open`; the C ABI
/// and the wrappers have the editor's save as `tpdf_editor_save` and the
/// pixel format through `tpdf_page_render`'s declaration.
fn join_surfaces() -> Surfaces {
    let mut out = Surfaces::default();
    let (functions, types) = header_exports(
        "TpdfStatus tpdf_document_open(const uint8_t *b, TpdfDocument **out);\n\
         uint32_t tpdf_document_page_count(const TpdfDocument *d);\n\
         TpdfStatus tpdf_editor_save(TpdfEditor *e, TpdfBuffer **out);\n\
         TpdfStatus tpdf_page_render(const TpdfDocument *d, TpdfPixelFormat f, TpdfBitmap **out);\n",
    );
    out.functions = functions;
    out.types = types;
    let every: BTreeSet<String> = [
        "tpdf_document_open",
        "tpdf_document_page_count",
        "tpdf_editor_save",
        "tpdf_page_render",
    ]
    .iter()
    .map(|name| (*name).to_string())
    .collect();
    for surface in WRAPPERS {
        out.wrappers.insert(surface, every.clone());
    }
    let class = |methods: &[&str]| -> BTreeSet<String> {
        methods.iter().map(|name| (*name).to_string()).collect()
    };
    out.python
        .classes
        .insert("Document".into(), class(&["open", "render", "page_count"]));
    out.python.classes.insert("Editor".into(), class(&["save"]));
    out.python.functions.insert("sniff".into());
    out.javascript.classes.insert(
        "PdfDocument".into(),
        class(&["open", "render_page", "page_count"]),
    );
    out.javascript
        .classes
        .insert("PdfEditor".into(), class(&["save"]));
    out.javascript.functions.insert("sniff".into());
    out
}

fn open_steps_of(steps: &[&str]) -> BTreeSet<String> {
    steps.iter().map(|step| (*step).to_string()).collect()
}

/// **The join by name and by the table**: a method by `tpdf_<type>_<name>`
/// and on the class of its type's name; a type crossing as another name; a
/// method a row maps; a type by its methods, or by its `Tpdf` type alone and,
/// on a wrapper, by a call whose declaration names it; a row's `—` taking a
/// surface away; and a function by its name.
#[test]
fn the_join_reads_names_and_the_table() {
    let items = join_items();
    let surfaces = join_surfaces();
    let mut read = ledger(LEDGER_TEXT);
    read.owed.clear();
    let joined = join(&items, &surfaces, &read, &open_steps_of(&["SU-12.3"]));
    let standing = |path: &str, surface: Surface| joined.standings[path][&surface].clone();

    for surface in SURFACES {
        assert!(
            matches!(standing("Document::open", surface), Standing::Projected(_)),
            "Document::open on the {}",
            surface.name()
        );
        assert!(
            matches!(
                standing("DocumentEditor::save", surface),
                Standing::Projected(_)
            ),
            "the editor's save on the {}",
            surface.name()
        );
        assert!(
            matches!(standing("Page::render", surface), Standing::Projected(_)),
            "the mapped render on the {}",
            surface.name()
        );
        assert!(
            matches!(standing("PixelFormat", surface), Standing::Projected(_))
                || matches!(surface, Surface::Python | Surface::JavaScript),
            "the pixel format on the {}",
            surface.name()
        );
    }
    assert_eq!(
        standing("Document::open", Surface::CAbi),
        Standing::Projected(vec!["tpdf_document_open".into()])
    );
    assert_eq!(
        standing("DocumentEditor::save", Surface::CAbi),
        Standing::Projected(vec!["tpdf_editor_save".into()])
    );
    assert_eq!(
        standing("Page::render", Surface::JavaScript),
        Standing::Projected(vec!["PdfDocument.render_page".into()])
    );
    assert_eq!(
        standing("PixelFormat", Surface::Go),
        Standing::Projected(vec!["TpdfPixelFormat".into()])
    );
    assert_eq!(
        standing("Document::page_count", Surface::Python),
        Standing::Missing,
        "the row's dash takes Python away"
    );
    assert_eq!(
        standing("sniff", Surface::Python),
        Standing::Projected(vec!["sniff".into()])
    );
    assert_eq!(standing("sniff", Surface::CAbi), Standing::Missing);
    // What is missing is named, with its surfaces.
    assert!(joined
        .problems
        .iter()
        .any(|problem| problem.starts_with("`sniff` is not on the C ABI, .NET, Go, Ruby, Java,")));
    assert!(joined
        .problems
        .iter()
        .any(|problem| problem.starts_with("`Document::page_count` is not on the Python,")));
}

/// **An item added with no projection, owed row or refusal fails**, naming
/// every surface that lacks it; owed on every surface for an open step, or
/// refused by name, it passes.
#[test]
fn an_item_with_no_projection_owed_row_or_refusal_fails() {
    let mut items = join_items();
    items.items.insert(
        "PageBuilder::glyphs".to_string(),
        Item {
            path: "PageBuilder::glyphs".to_string(),
            kind: Kind::Method,
            name: "glyphs".to_string(),
            owner: Some("PageBuilder".to_string()),
        },
    );
    let surfaces = join_surfaces();
    let mut bare = ledger(LEDGER_TEXT);
    bare.owed.clear();
    let joined = join(&items, &surfaces, &bare, &open_steps_of(&["SU-12.3"]));
    assert!(
        joined.problems.iter().any(|problem| problem
            == "`PageBuilder::glyphs` is not on the C ABI, .NET, Go, Ruby, Java, Python, \
                JavaScript, and is neither owed nor refused"),
        "{:?}",
        joined.problems
    );

    let owing = ledger(LEDGER_TEXT);
    let owed = join(
        &items,
        &surfaces,
        &owing,
        &open_steps_of(&["SU-12.3", "SU-12.14"]),
    );
    assert_eq!(
        owed.standings["PageBuilder::glyphs"][&Surface::Ruby],
        Standing::Owed("SU-12.3".into())
    );
    assert!(
        !owed
            .problems
            .iter()
            .any(|problem| problem.contains("PageBuilder::glyphs")),
        "{:?}",
        owed.problems
    );

    let mut refusing = ledger(LEDGER_TEXT);
    refusing.owed.clear();
    refusing.refused.push("PageBuilder::*".into());
    let refused = join(&items, &surfaces, &refusing, &BTreeSet::new());
    assert_eq!(
        refused.standings["PageBuilder::glyphs"][&Surface::Java],
        Standing::Refused
    );
}

/// **A ledger that has drifted from the tree fails**: an owed row whose step
/// is no longer on the roadmap or is not one of SU-12's, naming no item, or
/// naming an item projected everywhere it is owed; a projection row naming no
/// item or a name its surface lacks. A variant an owed row names is held to
/// its step alone.
#[test]
fn a_ledger_that_has_drifted_fails() {
    let items = join_items();
    let surfaces = join_surfaces();
    let mut read = ledger(LEDGER_TEXT);
    read.owed = vec![
        Owed {
            step: "SU-12.9".into(),
            items: vec!["Document::open".into()],
            surfaces: SURFACES.to_vec(),
        },
        Owed {
            step: "SU-12.3".into(),
            items: vec!["Nowhere::thing".into(), "Target::Named".into()],
            surfaces: SURFACES.to_vec(),
        },
        Owed {
            step: "FJ-01.1".into(),
            items: vec!["sniff".into()],
            surfaces: vec![Surface::CAbi],
        },
    ];
    read.projections.push(Projection {
        item: "Ghost".into(),
        c: Cell::Join,
        python: Cell::Join,
        javascript: Cell::Join,
    });
    read.projections.push(Projection {
        item: "sniff".into(),
        c: Cell::Names(vec!["tpdf_sniff".into()]),
        python: Cell::Join,
        javascript: Cell::Join,
    });
    let joined = join(&items, &surfaces, &read, &open_steps_of(&["SU-12.3"]));
    let wanted = [
        "Owed row for Document::open names SU-12.9, which is no step of SU-12 still in \
         docs/roadmap/SU.md",
        "Owed row for sniff names FJ-01.1, which is no step of SU-12 still in docs/roadmap/SU.md",
        "Owed row SU-12.3: `Nowhere::thing` names no facade item",
        "Owed row SU-12.9: `Document::open` is projected on every surface it is owed on: take it \
         off",
        "Projections row `Ghost` names no facade item",
        "Projections row `sniff`: `tpdf_sniff` is not on the C ABI",
    ];
    for problem in wanted {
        assert!(
            joined.problems.iter().any(|found| found == problem),
            "missing: {problem}\nhad: {:#?}",
            joined.problems
        );
    }
    assert!(
        !joined
            .problems
            .iter()
            .any(|problem| problem.contains("Target::Named")),
        "a variant owed to an open step is held to the step alone: {:#?}",
        joined.problems
    );
}

/// **The small helpers**: snake case as the C ABI spells a type, a ledger
/// name's match, and the steps a step file still holds.
#[test]
fn the_helpers_read_as_the_check_reads() {
    for (name, want) in [
        ("Document", "document"),
        ("DocumentEditor", "document_editor"),
        ("PageBuilder", "page_builder"),
        ("XMLParser", "xml_parser"),
        ("SvgOptions", "svg_options"),
    ] {
        assert_eq!(snake(name), want, "{name}");
    }
    assert!(matches("Document::*", "Document"));
    assert!(matches("Document::*", "Document::open"));
    assert!(!matches("Document::*", "DocumentEditor"));
    assert!(matches("Document", "Document"));
    assert!(!matches("Document", "Document::open"));
    let steps = open_steps(
        "| SU-12.3 | build | Builder text |\n| SU-12.24 | ci | Nothing owed |\n\
         | SU-14.1 | build | Exceptions |\n",
    );
    assert_eq!(steps, open_steps_of(&["SU-12.24", "SU-12.3"]));
    assert_eq!(Surface::parse(" .net "), Some(Surface::DotNet));
    assert_eq!(Surface::parse("c abi"), Some(Surface::CAbi));
    assert_eq!(Surface::parse("Fortran"), None);
}

/// **The repository's own surfaces read**: the facade's walk reaches its
/// open, a page's render and the builder its crate re-exports; the header
/// declares the open and render; each wrapper calls the open; Python's
/// `Document` and JavaScript's `PdfDocument` are classes with methods; and
/// the ledger's tables parse. A reader broken by a change of layout fails
/// here rather than reporting a whole surface missing.
#[test]
fn the_repository_s_own_surfaces_read() {
    let tree = read_tree(&crate::repo_root()).expect("the repository reads");
    let items = facade(&tree);
    for path in ["Document::open", "Page::render", "DocumentBuilder::finish"] {
        assert!(items.items.contains_key(path), "the walk lost {path}");
    }
    let surfaces = surfaces(&tree);
    for function in ["tpdf_document_open", "tpdf_page_render"] {
        assert!(surfaces.functions.contains_key(function), "{function}");
    }
    for surface in WRAPPERS {
        assert!(
            surfaces.wrappers[&surface].contains("tpdf_document_open"),
            "{} calls no open",
            surface.name()
        );
    }
    assert!(surfaces
        .python
        .classes
        .get("Document")
        .is_some_and(|methods| methods.contains("page_count")));
    assert!(surfaces
        .javascript
        .classes
        .get("PdfDocument")
        .is_some_and(|methods| !methods.is_empty()));
    let read = ledger(tree.get(LEDGER).expect("the ledger"));
    assert!(read.problems.is_empty(), "{:#?}", read.problems);
    assert!(!read.projections.is_empty() && !read.owed.is_empty());
    println!(
        "bindings-coverage-readers: RAN {} items, {} C functions",
        items.items.len(),
        surfaces.functions.len()
    );
}
