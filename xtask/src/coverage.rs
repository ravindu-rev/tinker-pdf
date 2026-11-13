//! `cargo xtask bindings-coverage`: every public facade item, listed against
//! the C ABI and each binding (row SU-12, ruling 11).
//!
//! Ruling 11 makes the facade the only public surface and every binding a
//! projection of it, so an item the facade has and a surface lacks is a debt.
//! This check finds the debts: it reads the facade's public items from its
//! source, each surface's exports from its own, joins the two, and fails on an
//! item a surface lacks that `docs/features/bindings.md` neither refuses by
//! name nor owes to a step of SU-12. It reads files and spawns nothing.
//!
//! # The items
//!
//! An item is a public type (struct, enum, union, trait or alias), a public
//! function, a public constant or static, or a public inherent method of a
//! public struct, enum or union, each named by its path from the facade's
//! root: `Document`, `Document::open`, `pdfa::Profile`. They are found by
//! walking `crates/tinker-pdf/src/lib.rs` and its public modules: a definition
//! is an item where it stands, and a `pub use` is followed — into the facade's
//! own modules and into the engine crates it re-exports from, through their
//! own re-exports and globs — to the definition, whose inherent `impl` blocks
//! in that crate give its methods. An item reached by two paths is one item,
//! under the shorter. A variant or a field crosses with its type and is not an
//! item; a ledger entry may still name one (`Target::Named`), and must name
//! one that exists. Test modules, `#[doc(hidden)]` items and restricted
//! visibility are not the surface.
//!
//! # The surfaces
//!
//! - **The C ABI**: the `tpdf_*` functions and `Tpdf*` types of
//!   `crates/tinker-pdf-ffi/include/tinker_pdf.h`, each function with the
//!   `Tpdf*` types its declaration names.
//! - **.NET, Go, Ruby and Java**, each over the C ABI: the `tpdf_*` functions
//!   its own code calls — `Native.tpdf_*` in C#, Ruby and Java, `C.tpdf_*` in
//!   Go — outside comments. The declarations each transcribes from the header
//!   (C#'s and Ruby's `extern` lines, Java's `Native.java`) are not calls.
//! - **Python**: the classes of `bindings/python/src` (`#[pyclass]`, by their
//!   Python name), the methods of their `#[pymethods]` blocks, by their Rust
//!   name and any `#[pyo3(name = …)]`, and the `#[pyfunction]`s.
//! - **JavaScript**: the classes of `bindings/js/src` (`#[wasm_bindgen]`
//!   structs, by their Rust name and any `js_name`), the methods of their
//!   `#[wasm_bindgen]` `impl` blocks, by their Rust name and any `js_name`, and
//!   the exported functions.
//!
//! # The join
//!
//! `docs/features/bindings.md`'s **Projections** table names how an item
//! crosses where the names do not say it; everything else is joined by name.
//! On the C ABI a method `T::m` is `tpdf_<t>_m`, `<t>` the type's name in
//! snake case or a prefix the table gives the type (`DocumentEditor` crosses
//! as `TpdfEditor`, so its methods as `tpdf_editor_*`); a function `f` is
//! `tpdf_f`; a type is its `Tpdf` type or any of its methods. A wrapper over
//! the C ABI has an item when it calls every function the C ABI crosses it by,
//! or, for an item the C ABI crosses as a type alone, a function whose
//! declaration names that type. Python and JavaScript have a method when the
//! class the type crosses as has it — the type's own name, `Pdf` and the name
//! on JavaScript, or the classes the table gives — and a function when the
//! module exports it. A table row's cell overrides the join for its surface:
//! the names it lists must all be there, and `—` says the item is not.
//!
//! # The ledger, and the outcomes
//!
//! An item a surface lacks is **refused** when a backticked name in the What
//! column of the **Refused by name** table matches it, on every surface, and
//! **owed** when a row of the **Owed** table does — its step, its items, its
//! surfaces (`every`, or names), and a reason the check does not read — for
//! the surfaces the row names, to the step of SU-12 the row names. A name matches an item exactly,
//! and `T::*` matches `T` and everything under it. Anything else fails, named
//! with the surfaces that lack it. So does a ledger that has drifted from the
//! tree: a projection row naming no item or a name its surface lacks, an owed
//! row naming no item or a step no longer on the roadmap, and an owed row
//! none of whose items is still missing where it says — an allowance that
//! outlives its need is how the next one gets in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::written::lex;

/// The header the C ABI is read from.
pub const HEADER: &str = "crates/tinker-pdf-ffi/include/tinker_pdf.h";

/// The feature document that holds the ledger.
pub const LEDGER: &str = "docs/features/bindings.md";

/// The step file whose SU-12 steps an owed row may name.
pub const STEPS: &str = "docs/roadmap/SU.md";

/// How deep a chain of re-exports is followed before it is given up on.
const MAX_HOPS: usize = 24;

/// The seven surfaces, in the order a listing gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Surface {
    CAbi,
    DotNet,
    Go,
    Ruby,
    Java,
    Python,
    JavaScript,
}

/// Every surface, in listing order.
pub const SURFACES: [Surface; 7] = [
    Surface::CAbi,
    Surface::DotNet,
    Surface::Go,
    Surface::Ruby,
    Surface::Java,
    Surface::Python,
    Surface::JavaScript,
];

/// The four surfaces over the C ABI.
const WRAPPERS: [Surface; 4] = [Surface::DotNet, Surface::Go, Surface::Ruby, Surface::Java];

impl Surface {
    /// The name the ledger and the listing use.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Surface::CAbi => "C ABI",
            Surface::DotNet => ".NET",
            Surface::Go => "Go",
            Surface::Ruby => "Ruby",
            Surface::Java => "Java",
            Surface::Python => "Python",
            Surface::JavaScript => "JavaScript",
        }
    }

    /// The surface a ledger cell names, case aside.
    #[must_use]
    pub fn parse(text: &str) -> Option<Surface> {
        let text = text.trim();
        SURFACES
            .into_iter()
            .find(|surface| surface.name().eq_ignore_ascii_case(text))
    }
}

// ---- the tree ------------------------------------------------------------------

/// The files the check reads, by path from the repository root with forward
/// slashes, and their text.
pub type Tree = BTreeMap<String, String>;

/// Reads every file the check needs: the engine crates' sources, the header,
/// the bindings' sources, the ledger and the step file.
pub fn read_tree(root: &Path) -> Result<Tree, String> {
    let mut tree = Tree::new();
    let crates =
        std::fs::read_dir(root.join("crates")).map_err(|error| format!("crates/: {error}"))?;
    for entry in crates.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("tinker-pdf") {
            collect(root, &format!("crates/{name}/src"), &["rs"], &mut tree);
        }
    }
    for (dir, extensions) in [
        ("bindings/dotnet", &["cs"][..]),
        ("bindings/go", &["go"][..]),
        ("bindings/ruby/lib", &["rb"][..]),
        ("bindings/java/src", &["java"][..]),
        ("bindings/python/src", &["rs"][..]),
        ("bindings/js/src", &["rs"][..]),
    ] {
        collect(root, dir, extensions, &mut tree);
    }
    for file in [HEADER, LEDGER, STEPS] {
        let text =
            std::fs::read_to_string(root.join(file)).map_err(|error| format!("{file}: {error}"))?;
        tree.insert(file.to_string(), text);
    }
    Ok(tree)
}

/// Every file under `dir` with one of `extensions`, into `tree`.
fn collect(root: &Path, dir: &str, extensions: &[&str], tree: &mut Tree) {
    let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{dir}/{name}");
        if entry.path().is_dir() {
            collect(root, &path, extensions, tree);
        } else if extensions
            .iter()
            .any(|extension| name.ends_with(&format!(".{extension}")))
        {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                tree.insert(path, text);
            }
        }
    }
}

// ---- tokens --------------------------------------------------------------------

/// A token of code: an identifier or number, or one punctuation byte.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Punct(char),
}

/// A file lexed (comments and the insides of literals blanked) and cut into
/// tokens, each with its byte offset, beside its string literals by offset.
struct Source {
    toks: Vec<(usize, Tok)>,
    strings: Vec<(usize, String)>,
}

impl Source {
    fn new(text: &str) -> Source {
        let (code, strings) = lex(text);
        Source {
            toks: tokens(&code),
            strings,
        }
    }

    fn ident(&self, at: usize) -> Option<&str> {
        match self.toks.get(at) {
            Some((_, Tok::Ident(name))) => Some(name),
            _ => None,
        }
    }

    fn is(&self, at: usize, punct: char) -> bool {
        matches!(self.toks.get(at), Some((_, Tok::Punct(p))) if *p == punct)
    }

    /// The string literal that begins inside token range `from..to`, if any:
    /// a lexed literal keeps its quotes as code, so its contents start one
    /// byte after a `"` token's offset.
    fn string_in(&self, from: usize, to: usize) -> Option<&str> {
        let start = self.toks.get(from)?.0;
        let end = self.toks.get(to).map_or(usize::MAX, |(offset, _)| *offset);
        self.strings
            .iter()
            .find(|(offset, _)| *offset > start && *offset < end)
            .map(|(_, text)| text.as_str())
    }

    /// The token after the bracket that matches the opening one at `open`,
    /// counting `()`, `[]` and `{}` together.
    fn close(&self, open: usize, to: usize) -> usize {
        let mut depth = 0usize;
        let mut at = open;
        while at < to {
            match self.toks.get(at) {
                Some((_, Tok::Punct('(' | '[' | '{'))) => depth += 1,
                Some((_, Tok::Punct(')' | ']' | '}'))) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return at + 1;
                    }
                }
                _ => {}
            }
            at += 1;
        }
        to
    }

    /// The first token at or after `from` that is `punct` outside every
    /// bracket, or `to`.
    fn find_outside(&self, from: usize, to: usize, punct: &[char]) -> usize {
        let mut at = from;
        while at < to {
            match self.toks.get(at) {
                Some((_, Tok::Punct(p))) if punct.contains(p) => return at,
                Some((_, Tok::Punct('(' | '[' | '{'))) => at = self.close(at, to),
                _ => at += 1,
            }
        }
        to
    }
}

/// `code`'s tokens: identifiers and numbers whole, every other printable
/// ASCII byte alone.
fn tokens(code: &str) -> Vec<(usize, Tok)> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            out.push((start, Tok::Ident(code[start..at].to_string())));
        } else {
            if byte.is_ascii_graphic() {
                out.push((at, Tok::Punct(char::from(byte))));
            }
            at += 1;
        }
    }
    out
}

/// One item of a module, an `impl` block or a trait, as its tokens give it.
#[derive(Clone, Debug)]
struct RawItem {
    /// The attributes' token ranges, inside their brackets.
    attrs: Vec<(usize, usize)>,
    /// Exactly `pub`: not private and not `pub(crate)` or the like.
    public: bool,
    keyword: String,
    name: Option<String>,
    /// The tokens inside the item's braces, for a module, a type, a trait or
    /// an `impl` block.
    body: Option<(usize, usize)>,
    /// The tokens after the keyword, up to the body or the `;`.
    head: (usize, usize),
}

impl RawItem {
    /// Whether an attribute says the item is not the surface: `#[cfg(test)]`
    /// (and not `not(test)`) or `#[doc(hidden)]`.
    fn hidden(&self, src: &Source) -> bool {
        self.attrs.iter().any(|&(from, to)| {
            let words: Vec<&str> = (from..to).filter_map(|at| src.ident(at)).collect();
            match words.first() {
                Some(&"cfg") => words.contains(&"test") && !words.contains(&"not"),
                Some(&"doc") => words.contains(&"hidden"),
                _ => false,
            }
        })
    }

    /// Whether an attribute's first word is `name`.
    fn has_attr(&self, src: &Source, name: &str) -> bool {
        self.attr(src, name).is_some()
    }

    /// The token range of the attribute whose first word is `name`.
    fn attr(&self, src: &Source, name: &str) -> Option<(usize, usize)> {
        self.attrs
            .iter()
            .copied()
            .find(|&(from, _)| src.ident(from) == Some(name))
    }
}

/// The items of `src` between tokens `from` and `to`, at that level.
fn items(src: &Source, from: usize, to: usize) -> Vec<RawItem> {
    let mut out = Vec::new();
    let mut at = from;
    while at < to {
        let mut attrs = Vec::new();
        while src.is(at, '#') {
            let mut open = at + 1;
            if src.is(open, '!') {
                open += 1;
            }
            if !src.is(open, '[') {
                break;
            }
            let close = src.close(open, to);
            attrs.push((open + 1, close.saturating_sub(1)));
            at = close;
        }
        if at >= to {
            break;
        }
        let mut public = false;
        if src.ident(at) == Some("pub") {
            if src.is(at + 1, '(') {
                at = src.close(at + 1, to);
            } else {
                public = true;
                at += 1;
            }
        }
        // Qualifiers before the keyword: `const fn`, `unsafe fn`, `async fn`,
        // `extern "C" fn`, `default fn`.
        loop {
            match src.ident(at) {
                Some("unsafe" | "async" | "default") => at += 1,
                Some("const") if src.ident(at + 1) == Some("fn") => at += 1,
                // `extern "C"`: the literal is its two quotes once lexed.
                Some("extern") if src.is(at + 1, '"') && src.is(at + 2, '"') => at += 3,
                _ => break,
            }
        }
        let keyword = src.ident(at).unwrap_or_default().to_string();
        let start = at + 1;
        let (name, body, head, next) = match keyword.as_str() {
            "mod" => {
                let name = src.ident(start).map(str::to_string);
                if src.is(start + 1, '{') {
                    let close = src.close(start + 1, to);
                    (
                        name,
                        Some((start + 2, close - 1)),
                        (start, start + 1),
                        close,
                    )
                } else {
                    let end = src.find_outside(start, to, &[';']);
                    (name, None, (start, end), end + 1)
                }
            }
            "use" | "const" | "static" | "type" => {
                let end = src.find_outside(start, to, &[';']);
                let name = match keyword.as_str() {
                    "use" => None,
                    "static" if src.ident(start) == Some("mut") => {
                        src.ident(start + 1).map(str::to_string)
                    }
                    _ => src.ident(start).map(str::to_string),
                };
                (name, None, (start, end), end + 1)
            }
            "struct" | "enum" | "union" | "trait" | "fn" | "impl" => {
                let name = if keyword == "impl" {
                    None
                } else {
                    src.ident(start).map(str::to_string)
                };
                // The body is the first `{` outside brackets, unless a `;`
                // comes first: a tuple or unit struct, a declared method.
                let stop = src.find_outside(start, to, &['{', ';']);
                if src.is(stop, '{') {
                    let close = src.close(stop, to);
                    (name, Some((stop + 1, close - 1)), (start, stop), close)
                } else {
                    (name, None, (start, stop), stop + 1)
                }
            }
            _ => {
                // A macro, `extern crate`, or anything else: to its `;` or
                // through its braces.
                let stop = src.find_outside(at, to, &['{', ';']);
                let next = if src.is(stop, '{') {
                    let close = src.close(stop, to);
                    if src.is(close, ';') {
                        close + 1
                    } else {
                        close
                    }
                } else {
                    stop + 1
                };
                (None, None, (start, stop), next)
            }
        };
        out.push(RawItem {
            attrs,
            public,
            keyword,
            name,
            body,
            head,
        });
        at = next.max(at + 1);
    }
    out
}

/// One leaf of a `use` tree: its path and the name it is brought in as, or a
/// glob.
#[derive(Clone, Debug, PartialEq, Eq)]
struct UseLeaf {
    path: Vec<String>,
    alias: Option<String>,
    glob: bool,
}

impl UseLeaf {
    /// The name the leaf is brought in as.
    fn name(&self) -> Option<&str> {
        self.alias
            .as_deref()
            .or_else(|| self.path.last().map(String::as_str))
    }
}

/// The leaves of the `use` tree in tokens `from..to`.
fn use_leaves(src: &Source, from: usize, to: usize) -> Vec<UseLeaf> {
    let mut out = Vec::new();
    use_tree(src, from, to, &[], &mut out);
    out
}

fn use_tree(src: &Source, from: usize, to: usize, prefix: &[String], out: &mut Vec<UseLeaf>) {
    let mut path = prefix.to_vec();
    let mut at = from;
    // A leading `::`.
    while src.is(at, ':') {
        at += 1;
    }
    while at < to {
        if src.is(at, '{') {
            let close = src.close(at, to);
            let mut part = at + 1;
            while part < close - 1 {
                let end = src.find_outside(part, close - 1, &[',']);
                if end > part {
                    use_tree(src, part, end, &path, out);
                }
                part = end + 1;
            }
            return;
        }
        if src.is(at, '*') {
            out.push(UseLeaf {
                path,
                alias: None,
                glob: true,
            });
            return;
        }
        match src.ident(at) {
            Some("as") => {
                let alias = src.ident(at + 1).map(str::to_string);
                out.push(UseLeaf {
                    path,
                    alias,
                    glob: false,
                });
                return;
            }
            Some(segment) => {
                path.push(segment.to_string());
                at += 1;
            }
            None => at += 1,
        }
    }
    if path.len() > prefix.len() {
        out.push(UseLeaf {
            path,
            alias: None,
            glob: false,
        });
    }
}

// ---- the facade ------------------------------------------------------------------

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Type,
    Function,
    Method,
    Constant,
}

/// One public item of the facade.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Its path from the facade's root.
    pub path: String,
    pub kind: Kind,
    /// Its own name: the type's, the function's, the method's.
    pub name: String,
    /// For a method, its type's last name, which the join reads.
    pub owner: Option<String>,
}

/// Where an item is defined: its crate, its file and its name, which is what
/// makes two paths to it one item.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Def {
    krate: String,
    file: String,
    name: String,
    keyword: String,
    /// The tokens inside its braces, for an enum or a struct.
    body: Option<(usize, usize)>,
}

/// A module: the file it is in and the tokens that are its contents.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Loc {
    file: String,
    range: (usize, usize),
}

/// The facade and the crates it re-exports from, as the walk reads them.
struct Facade<'a> {
    tree: &'a Tree,
    sources: BTreeMap<String, Source>,
    /// The engine crates the tree holds, by the name a path names them by.
    crates: BTreeSet<String>,
    /// Each crate's definitions and `impl` blocks, read once a crate.
    indexes: BTreeMap<String, CrateIndex>,
}

/// One crate's public definitions by name, and the public methods of its
/// inherent `impl` blocks by the type they name, with the file of each.
#[derive(Default)]
struct CrateIndex {
    defs: BTreeMap<String, Vec<Def>>,
    impls: BTreeMap<String, Vec<(String, BTreeSet<String>)>>,
}

/// The public items of the facade, by path, and the variants and fields of
/// its types a ledger entry may name.
#[derive(Debug, Default)]
pub struct Items {
    pub items: BTreeMap<String, Item>,
    pub parts: BTreeSet<String>,
}

impl<'a> Facade<'a> {
    fn new(tree: &'a Tree) -> Facade<'a> {
        let crates = tree
            .keys()
            .filter_map(|path| path.strip_prefix("crates/"))
            .filter_map(|rest| rest.split('/').next())
            .map(|dir| dir.replace('-', "_"))
            .collect();
        Facade {
            tree,
            sources: BTreeMap::new(),
            crates,
            indexes: BTreeMap::new(),
        }
    }

    /// `krate`'s index, read from its non-test files the first time.
    fn index(&mut self, krate: &str) -> &CrateIndex {
        if !self.indexes.contains_key(krate) {
            let dir = format!("{}/", Facade::src_dir(krate));
            let files: Vec<String> = self
                .tree
                .keys()
                .filter(|path| {
                    path.starts_with(&dir) && path.ends_with(".rs") && !is_test_file(path)
                })
                .cloned()
                .collect();
            let mut index = CrateIndex::default();
            for file in files {
                let Some(src) = self.source(&file) else {
                    continue;
                };
                for (item, _) in module_items(src) {
                    if item.hidden(src) {
                        continue;
                    }
                    match item.keyword.as_str() {
                        "struct" | "enum" | "union" | "trait" | "type" | "fn" | "const"
                        | "static"
                            if item.public =>
                        {
                            if let Some(name) = &item.name {
                                index.defs.entry(name.clone()).or_default().push(Def {
                                    krate: krate.to_string(),
                                    file: file.clone(),
                                    name: name.clone(),
                                    keyword: item.keyword.clone(),
                                    body: item.body,
                                });
                            }
                        }
                        "impl" => {
                            let (Some(body), Some(owner)) = (item.body, impl_self(src, item.head))
                            else {
                                continue;
                            };
                            let methods: BTreeSet<String> = items(src, body.0, body.1)
                                .into_iter()
                                .filter(|inner| {
                                    inner.keyword == "fn" && inner.public && !inner.hidden(src)
                                })
                                .filter_map(|inner| inner.name)
                                .collect();
                            index
                                .impls
                                .entry(owner.to_string())
                                .or_default()
                                .push((file.clone(), methods));
                        }
                        _ => {}
                    }
                }
            }
            self.indexes.insert(krate.to_string(), index);
        }
        self.indexes.entry(krate.to_string()).or_default()
    }

    fn source(&mut self, file: &str) -> Option<&Source> {
        if !self.sources.contains_key(file) {
            let text = self.tree.get(file)?;
            self.sources.insert(file.to_string(), Source::new(text));
        }
        self.sources.get(file)
    }

    /// The directory a crate's sources are in.
    fn src_dir(krate: &str) -> String {
        format!("crates/{}/src", krate.replace('_', "-"))
    }

    /// Where module `module` of `krate` is: a file of its own, or a `mod`
    /// with braces inside its parent's.
    fn locate(&mut self, krate: &str, module: &[String]) -> Option<Loc> {
        let dir = Facade::src_dir(krate);
        if module.is_empty() {
            let file = format!("{dir}/lib.rs");
            let len = self.source(&file)?.toks.len();
            return Some(Loc {
                file,
                range: (0, len),
            });
        }
        let joined = module.join("/");
        for file in [
            format!("{dir}/{joined}.rs"),
            format!("{dir}/{joined}/mod.rs"),
        ] {
            if let Some(src) = self.source(&file) {
                let len = src.toks.len();
                return Some(Loc {
                    file,
                    range: (0, len),
                });
            }
        }
        let (last, parent) = module.split_last()?;
        let parent = self.locate(krate, parent)?;
        let src = self.source(&parent.file)?;
        items(src, parent.range.0, parent.range.1)
            .into_iter()
            .find(|item| item.keyword == "mod" && item.name.as_deref() == Some(last.as_str()))
            .and_then(|item| item.body)
            .map(|range| Loc {
                file: parent.file.clone(),
                range,
            })
    }

    /// The crate and module a `use` path's leading segments name, from
    /// module `module` of `krate`, and the segments left after them.
    fn start(
        &mut self,
        krate: &str,
        module: &[String],
        path: &[String],
    ) -> Option<(String, Vec<String>, usize)> {
        let first = path.first()?;
        match first.as_str() {
            "crate" => Some((krate.to_string(), Vec::new(), 1)),
            "self" => Some((krate.to_string(), module.to_vec(), 1)),
            "super" => {
                let mut parent = module.to_vec();
                let mut used = 0;
                while path.get(used).map(String::as_str) == Some("super") {
                    parent.pop();
                    used += 1;
                }
                Some((krate.to_string(), parent, used))
            }
            name if self.crates.contains(name) => Some((name.to_string(), Vec::new(), 1)),
            name => {
                // A child module of this one, as a path in a `use` reads.
                let mut child = module.to_vec();
                child.push(name.to_string());
                self.locate(krate, &child)
                    .map(|_| (krate.to_string(), child, 1))
            }
        }
    }

    /// The definitions `path` names from module `module` of `krate`.
    fn resolve(
        &mut self,
        krate: &str,
        module: &[String],
        path: &[String],
        hops: usize,
    ) -> Vec<Def> {
        if hops > MAX_HOPS {
            return Vec::new();
        }
        let Some((target, mut at_module, used)) = self.start(krate, module, path) else {
            return Vec::new();
        };
        let rest = &path[used..];
        let Some((name, modules)) = rest.split_last() else {
            return Vec::new();
        };
        at_module.extend(modules.iter().cloned());
        let found = self.find(&target, &at_module, name, hops + 1);
        if found.is_empty() {
            self.anywhere(&target, name)
        } else {
            found
        }
    }

    /// The definitions of `name` in module `module` of `krate`: one written
    /// there, or one its `pub use`s bring in.
    fn find(&mut self, krate: &str, module: &[String], name: &str, hops: usize) -> Vec<Def> {
        if hops > MAX_HOPS {
            return Vec::new();
        }
        let Some(loc) = self.locate(krate, module) else {
            return Vec::new();
        };
        let Some(src) = self.source(&loc.file) else {
            return Vec::new();
        };
        let listed = items(src, loc.range.0, loc.range.1);
        let mut uses = Vec::new();
        for item in &listed {
            if !item.public || item.hidden(src) {
                continue;
            }
            match item.keyword.as_str() {
                "struct" | "enum" | "union" | "trait" | "type" | "fn" | "const" | "static"
                    if item.name.as_deref() == Some(name) =>
                {
                    return vec![Def {
                        krate: krate.to_string(),
                        file: loc.file.clone(),
                        name: name.to_string(),
                        keyword: item.keyword.clone(),
                        body: item.body,
                    }];
                }
                "use" => uses.extend(use_leaves(src, item.head.0, item.head.1)),
                _ => {}
            }
        }
        for leaf in uses {
            if leaf.glob {
                let mut path = leaf.path.clone();
                path.push(name.to_string());
                let found = self.resolve(krate, module, &path, hops + 1);
                if !found.is_empty() {
                    return found;
                }
            } else if leaf.name() == Some(name) {
                return self.resolve(krate, module, &leaf.path, hops + 1);
            }
        }
        Vec::new()
    }

    /// Every public definition of `name` anywhere in `krate`'s non-test
    /// files: the last resort for a re-export the walk could not follow.
    fn anywhere(&mut self, krate: &str, name: &str) -> Vec<Def> {
        self.index(krate)
            .defs
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// The public inherent methods of `def`, from the `impl` blocks of its
    /// crate: those in its own file where the crate defines its name more
    /// than once, all of them otherwise.
    fn methods(&mut self, def: &Def) -> BTreeSet<String> {
        let index = self.index(&def.krate);
        let defined = index.defs.get(&def.name).map_or(0, Vec::len);
        let mut out = BTreeSet::new();
        for (file, methods) in index.impls.get(&def.name).into_iter().flatten() {
            if defined > 1 && *file != def.file {
                continue;
            }
            out.extend(methods.iter().cloned());
        }
        out
    }

    /// The variants of an enum, or the public fields of a struct.
    fn parts(&mut self, def: &Def) -> Vec<String> {
        let Some((from, to)) = def.body else {
            return Vec::new();
        };
        let Some(src) = self.source(&def.file) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut at = from;
        while at < to {
            // One variant or field: attributes, then its name.
            while src.is(at, '#') && src.is(at + 1, '[') {
                at = src.close(at + 1, to);
            }
            let end = src.find_outside(at, to, &[',']);
            match def.keyword.as_str() {
                "enum" => {
                    if let Some(name) = src.ident(at) {
                        out.push(name.to_string());
                    }
                }
                _ => {
                    if src.ident(at) == Some("pub") && !src.is(at + 1, '(') {
                        if let Some(name) = src.ident(at + 1) {
                            out.push(name.to_string());
                        }
                    }
                }
            }
            at = end + 1;
        }
        out
    }
}

/// Whether a file is a test's, which is never the surface: a `tests.rs`, a
/// `*_tests.rs`, or anything under a `tests` directory.
fn is_test_file(path: &str) -> bool {
    path.ends_with("/tests.rs") || path.ends_with("_tests.rs") || path.contains("/tests/")
}

/// Every item of a file at its top level and inside its non-test modules with
/// braces, each with the module path inside the file.
fn module_items(src: &Source) -> Vec<(RawItem, Vec<String>)> {
    let mut out = Vec::new();
    let mut pending = vec![((0, src.toks.len()), Vec::new())];
    while let Some(((from, to), path)) = pending.pop() {
        for item in items(src, from, to) {
            if item.keyword == "mod" && !item.hidden(src) {
                if let (Some(body), Some(name)) = (item.body, &item.name) {
                    let mut inner = path.clone();
                    inner.push(name.clone());
                    pending.push((body, inner));
                }
            }
            out.push((item, path.clone()));
        }
    }
    out
}

/// The type an inherent `impl` block's head names, or `None` for a trait's
/// (`impl Trait for Type`): the last segment of the path, its generics aside.
fn impl_self(src: &Source, head: (usize, usize)) -> Option<&str> {
    let (mut at, to) = head;
    // The block's own generics.
    if src.is(at, '<') {
        let mut depth = 0usize;
        while at < to {
            if src.is(at, '<') {
                depth += 1;
            } else if src.is(at, '>') && !src.is(at.saturating_sub(1), '-') {
                depth -= 1;
                if depth == 0 {
                    at += 1;
                    break;
                }
            }
            at += 1;
        }
    }
    let mut last = None;
    let mut depth = 0usize;
    while at < to {
        if src.is(at, '<') {
            depth += 1;
        } else if src.is(at, '>') && !src.is(at.saturating_sub(1), '-') {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            match src.ident(at) {
                Some("for") => return None,
                Some("where") => break,
                Some("dyn" | "mut") => {}
                Some(name) => last = Some(name),
                None => {}
            }
        }
        at += 1;
    }
    last
}

/// The facade's public items: walked from its root, every `pub use` followed
/// to its definition, each type's inherent methods with it.
pub fn facade(tree: &Tree) -> Items {
    let mut facade = Facade::new(tree);
    // Each definition under the shortest path that reaches it.
    let mut defs: BTreeMap<Def, String> = BTreeMap::new();
    let mut unresolved: BTreeMap<String, ()> = BTreeMap::new();
    let mut pending: Vec<(String, Vec<String>, String)> =
        vec![("tinker_pdf".to_string(), Vec::new(), String::new())];
    let mut walked: BTreeSet<(String, Vec<String>)> = BTreeSet::new();
    let place = |defs: &mut BTreeMap<Def, String>, def: Def, path: String| {
        let better = defs.get(&def).is_none_or(|known| {
            (known.matches("::").count(), known.len(), known.as_str())
                > (path.matches("::").count(), path.len(), path.as_str())
        });
        if better {
            defs.insert(def, path);
        }
    };
    while let Some((krate, module, prefix)) = pending.pop() {
        if !walked.insert((krate.clone(), module.clone())) {
            continue;
        }
        let Some(loc) = facade.locate(&krate, &module) else {
            continue;
        };
        let Some(src) = facade.source(&loc.file) else {
            continue;
        };
        let listed = items(src, loc.range.0, loc.range.1);
        let mut uses = Vec::new();
        for item in &listed {
            if !item.public || item.hidden(src) {
                continue;
            }
            match item.keyword.as_str() {
                "mod" => {
                    if let Some(name) = &item.name {
                        let mut inner = module.clone();
                        inner.push(name.clone());
                        pending.push((krate.clone(), inner, format!("{prefix}{name}::")));
                    }
                }
                "use" => uses.extend(use_leaves(src, item.head.0, item.head.1)),
                "struct" | "enum" | "union" | "trait" | "type" | "fn" | "const" | "static" => {
                    if let Some(name) = &item.name {
                        let def = Def {
                            krate: krate.clone(),
                            file: loc.file.clone(),
                            name: name.clone(),
                            keyword: item.keyword.clone(),
                            body: item.body,
                        };
                        place(&mut defs, def, format!("{prefix}{name}"));
                    }
                }
                _ => {}
            }
        }
        for leaf in uses {
            if leaf.glob {
                // Everything the glob's module holds, at this level.
                if let Some((target, mut at_module, used)) =
                    facade.start(&krate, &module, &leaf.path)
                {
                    at_module.extend(leaf.path[used..].iter().cloned());
                    pending.push((target, at_module, prefix.clone()));
                }
                continue;
            }
            let Some(name) = leaf.name().map(str::to_string) else {
                continue;
            };
            let found = facade.resolve(&krate, &module, &leaf.path, 0);
            if found.is_empty() {
                // A module brought in by name is walked under that name.
                if let Some((target, mut at_module, used)) =
                    facade.start(&krate, &module, &leaf.path)
                {
                    at_module.extend(leaf.path[used..].iter().cloned());
                    if facade.locate(&target, &at_module).is_some() {
                        pending.push((target, at_module, format!("{prefix}{name}::")));
                        continue;
                    }
                }
                unresolved.insert(format!("{prefix}{name}"), ());
            }
            for def in found {
                place(&mut defs, def, format!("{prefix}{name}"));
            }
        }
    }

    let mut out = Items::default();
    for path in unresolved.into_keys() {
        if !defs.values().any(|known| *known == path) {
            let name = path.rsplit("::").next().unwrap_or(&path).to_string();
            out.items.insert(
                path.clone(),
                Item {
                    path,
                    kind: Kind::Type,
                    name,
                    owner: None,
                },
            );
        }
    }
    for (def, path) in defs {
        let kind = match def.keyword.as_str() {
            "fn" => Kind::Function,
            "const" | "static" => Kind::Constant,
            _ => Kind::Type,
        };
        if matches!(def.keyword.as_str(), "struct" | "enum" | "union") {
            for method in facade.methods(&def) {
                let method_path = format!("{path}::{method}");
                out.items.insert(
                    method_path.clone(),
                    Item {
                        path: method_path,
                        kind: Kind::Method,
                        name: method,
                        owner: Some(def.name.clone()),
                    },
                );
            }
            for part in facade.parts(&def) {
                out.parts.insert(format!("{path}::{part}"));
            }
        }
        out.items.insert(
            path.clone(),
            Item {
                path,
                kind,
                name: def.name.clone(),
                owner: None,
            },
        );
    }
    out
}

// ---- the surfaces ----------------------------------------------------------------

/// A binding's classes and module functions, by the names the join reads.
#[derive(Debug, Default, Clone)]
pub struct Binding {
    /// Each class, under every name it is known by, with its methods' names.
    pub classes: BTreeMap<String, BTreeSet<String>>,
    pub functions: BTreeSet<String>,
}

/// What each surface exports.
#[derive(Debug, Default)]
pub struct Surfaces {
    /// The C ABI's functions, each with the `Tpdf` types its declaration
    /// names.
    pub functions: BTreeMap<String, BTreeSet<String>>,
    /// The C ABI's `Tpdf` types.
    pub types: BTreeSet<String>,
    /// The functions of the C ABI each wrapper calls.
    pub wrappers: BTreeMap<Surface, BTreeSet<String>>,
    pub python: Binding,
    pub javascript: Binding,
}

/// Every surface, read from `tree`.
pub fn surfaces(tree: &Tree) -> Surfaces {
    let mut out = Surfaces::default();
    if let Some(header) = tree.get(HEADER) {
        let (functions, types) = header_exports(header);
        out.functions = functions;
        out.types = types;
    }
    for surface in WRAPPERS {
        let mut calls = BTreeSet::new();
        for (path, text) in tree {
            if let Some(found) = wrapper_calls(surface, path, text) {
                calls.extend(found);
            }
        }
        out.wrappers.insert(surface, calls);
    }
    let python: Vec<&str> = tree
        .iter()
        .filter(|(path, _)| path.starts_with("bindings/python/src/") && path.ends_with(".rs"))
        .map(|(_, text)| text.as_str())
        .collect();
    python_exports(&python, &mut out.python);
    for (path, text) in tree {
        if path.starts_with("bindings/js/src/") && path.ends_with(".rs") {
            javascript_exports(text, &mut out.javascript);
        }
    }
    out
}

/// The header's functions, each with the `Tpdf` types its declaration names,
/// and every `Tpdf` type it names.
fn header_exports(header: &str) -> (BTreeMap<String, BTreeSet<String>>, BTreeSet<String>) {
    let src = Source::new(header);
    let mut functions = BTreeMap::new();
    let mut types = BTreeSet::new();
    let mut statement_start = 0;
    for at in 0..src.toks.len() {
        match &src.toks[at].1 {
            Tok::Punct(';' | '{' | '}') => statement_start = at + 1,
            Tok::Ident(name) if name.starts_with("Tpdf") => {
                types.insert(name.clone());
            }
            Tok::Ident(name) if name.starts_with("tpdf_") && src.is(at + 1, '(') => {
                let end = src.close(at + 1, src.toks.len());
                let named: BTreeSet<String> = (statement_start..end)
                    .filter_map(|i| src.ident(i))
                    .filter(|word| word.starts_with("Tpdf"))
                    .map(str::to_string)
                    .collect();
                functions
                    .entry(name.clone())
                    .or_insert_with(BTreeSet::new)
                    .extend(named);
            }
            _ => {}
        }
    }
    (functions, types)
}

/// The C functions a wrapper's file calls, or `None` for a file that is no
/// part of that wrapper's library.
fn wrapper_calls(surface: Surface, path: &str, text: &str) -> Option<BTreeSet<String>> {
    let (prefix, qualifier) = match surface {
        Surface::DotNet => ("bindings/dotnet/", "Native"),
        Surface::Go => ("bindings/go/", "C"),
        Surface::Ruby => ("bindings/ruby/lib/", "Native"),
        Surface::Java => ("bindings/java/src/", "Native"),
        _ => return None,
    };
    if !path.starts_with(prefix) {
        return None;
    }
    let rest = &path[prefix.len()..];
    let own = match surface {
        // The library's own files: not its tests, which sit in a directory.
        Surface::DotNet => rest.ends_with(".cs") && !rest.contains('/'),
        Surface::Go => rest.ends_with(".go") && !rest.contains('/') && !rest.ends_with("_test.go"),
        Surface::Ruby => rest.ends_with(".rb"),
        Surface::Java => rest.ends_with(".java"),
        _ => false,
    };
    if !own {
        return None;
    }
    let code = match surface {
        Surface::Ruby => ruby_code(text),
        _ => lex(text).0,
    };
    let src = Source {
        toks: tokens(&code),
        strings: Vec::new(),
    };
    let mut calls = BTreeSet::new();
    for at in 2..src.toks.len() {
        if let Some(name) = src.ident(at) {
            if name.starts_with("tpdf_")
                && src.is(at - 1, '.')
                && src.ident(at - 2) == Some(qualifier)
            {
                calls.insert(name.to_string());
            }
        }
    }
    Some(calls)
}

/// Ruby source with its `#` comments blanked: a `#` outside a string to the
/// end of its line.
fn ruby_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut quote: Option<char> = None;
        let mut escaped = false;
        let mut cut = line.len();
        for (at, ch) in line.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match (quote, ch) {
                (Some(_), '\\') => escaped = true,
                (Some(open), c) if c == open => quote = None,
                (None, '\'' | '"') => quote = Some(ch),
                (None, '#') => {
                    cut = at;
                    break;
                }
                _ => {}
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// A PyO3 module's classes, methods and functions, over all its files: a
/// class's methods may be in another file than the class.
fn python_exports(texts: &[&str], binding: &mut Binding) {
    let sources: Vec<Source> = texts.iter().map(|text| Source::new(text)).collect();
    // Each struct's class name: `#[pyclass(name = "X")]`, else its own.
    let mut classes: BTreeMap<String, String> = BTreeMap::new();
    for src in &sources {
        for (item, _) in module_items(src) {
            if item.keyword == "struct" {
                if let (Some(name), Some((from, to))) = (&item.name, item.attr(src, "pyclass")) {
                    let python = named(src, from, to, "name").unwrap_or_else(|| name.clone());
                    classes.insert(name.clone(), python);
                }
            }
        }
    }
    for src in &sources {
        python_items(src, &classes, binding);
    }
    for python in classes.into_values() {
        binding.classes.entry(python).or_default();
    }
}

/// One PyO3 file's methods and functions, its classes named by `classes`.
fn python_items(src: &Source, classes: &BTreeMap<String, String>, binding: &mut Binding) {
    let listed = module_items(src);
    for (item, _) in &listed {
        match item.keyword.as_str() {
            "impl" if item.has_attr(src, "pymethods") => {
                let Some(owner) = impl_self(src, item.head) else {
                    continue;
                };
                let class = classes
                    .get(owner)
                    .cloned()
                    .unwrap_or_else(|| owner.to_string());
                let Some(body) = item.body else { continue };
                let methods = binding.classes.entry(class).or_default();
                for inner in items(src, body.0, body.1) {
                    if inner.keyword != "fn" {
                        continue;
                    }
                    if let Some(name) = &inner.name {
                        methods.insert(name.clone());
                    }
                    if let Some((from, to)) = inner.attr(src, "pyo3") {
                        if let Some(renamed) = named(src, from, to, "name") {
                            methods.insert(renamed);
                        }
                    }
                    if inner.has_attr(src, "new") {
                        methods.insert("__new__".to_string());
                    }
                }
            }
            "fn" if item.has_attr(src, "pyfunction") => {
                if let Some(name) = &item.name {
                    binding.functions.insert(name.clone());
                }
                if let Some((from, to)) = item.attr(src, "pyo3") {
                    if let Some(renamed) = named(src, from, to, "name") {
                        binding.functions.insert(renamed);
                    }
                }
            }
            _ => {}
        }
    }
}

/// A wasm-bindgen module's classes, methods and functions.
fn javascript_exports(text: &str, binding: &mut Binding) {
    let src = Source::new(text);
    let listed = module_items(&src);
    for (item, _) in &listed {
        let Some(attr) = item.attr(&src, "wasm_bindgen") else {
            continue;
        };
        match item.keyword.as_str() {
            "struct" => {
                if let Some(name) = &item.name {
                    binding.classes.entry(name.clone()).or_default();
                    if let Some(js) = js_name(&src, attr) {
                        binding.classes.entry(js).or_default();
                    }
                }
            }
            "impl" => {
                let Some(owner) = impl_self(&src, item.head) else {
                    continue;
                };
                let Some(body) = item.body else { continue };
                let mut found = BTreeSet::new();
                for inner in items(&src, body.0, body.1) {
                    if inner.keyword != "fn" || !inner.public {
                        continue;
                    }
                    if let Some(name) = &inner.name {
                        found.insert(name.clone());
                    }
                    if let Some(js) = inner
                        .attr(&src, "wasm_bindgen")
                        .and_then(|a| js_name(&src, a))
                    {
                        found.insert(js);
                    }
                }
                binding
                    .classes
                    .entry(owner.to_string())
                    .or_default()
                    .extend(found);
            }
            "fn" if item.public => {
                if let Some(name) = &item.name {
                    binding.functions.insert(name.clone());
                }
                if let Some(js) = js_name(&src, attr) {
                    binding.functions.insert(js);
                }
            }
            _ => {}
        }
    }
}

/// The string an attribute gives `key` (`key = "value"`).
fn named(src: &Source, from: usize, to: usize, key: &str) -> Option<String> {
    (from..to)
        .find(|&at| src.ident(at) == Some(key) && src.is(at + 1, '='))
        .and_then(|at| src.string_in(at + 1, to.min(at + 5)))
        .map(str::to_string)
}

/// The `js_name` of a `#[wasm_bindgen(...)]` attribute: an identifier or a
/// string.
fn js_name(src: &Source, (from, to): (usize, usize)) -> Option<String> {
    let at = (from..to).find(|&at| src.ident(at) == Some("js_name") && src.is(at + 1, '='))?;
    if let Some(ident) = src.ident(at + 2) {
        return Some(ident.to_string());
    }
    src.string_in(at + 1, to.min(at + 5)).map(str::to_string)
}

// ---- the ledger --------------------------------------------------------------------

/// How a projection row's cell says an item crosses one surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Empty: the join by name decides.
    Join,
    /// `—`: not on this surface.
    Absent,
    /// These names, every one of which the surface must have.
    Names(Vec<String>),
}

/// One row of the Projections table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    pub item: String,
    pub c: Cell,
    pub python: Cell,
    pub javascript: Cell,
}

/// One row of the Owed table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owed {
    pub step: String,
    pub items: Vec<String>,
    pub surfaces: Vec<Surface>,
}

/// `docs/features/bindings.md`'s three tables, and what in them would not
/// parse.
#[derive(Debug, Default)]
pub struct Ledger {
    pub projections: Vec<Projection>,
    pub owed: Vec<Owed>,
    /// The names in the refusal table's What column.
    pub refused: Vec<String>,
    pub problems: Vec<String>,
}

/// The ledger, read from the feature document's text.
pub fn ledger(text: &str) -> Ledger {
    let mut out = Ledger::default();
    for (section, rows) in tables(text) {
        match section.as_str() {
            "Projections" => {
                for (line, cells) in rows {
                    if cells.len() != 4 {
                        out.problems.push(format!(
                            "{LEDGER}:{line}: a Projections row has {} cells, not 4",
                            cells.len()
                        ));
                        continue;
                    }
                    let item = backticked(&cells[0]);
                    let [one] = item.as_slice() else {
                        out.problems.push(format!(
                            "{LEDGER}:{line}: a Projections row names {} facade items, not one",
                            item.len()
                        ));
                        continue;
                    };
                    out.projections.push(Projection {
                        item: one.clone(),
                        c: cell(&cells[1]),
                        python: cell(&cells[2]),
                        javascript: cell(&cells[3]),
                    });
                }
            }
            "Owed" => {
                for (line, cells) in rows {
                    // The step, the items, the surfaces, and a reason, which
                    // the check does not read.
                    if !matches!(cells.len(), 3 | 4) {
                        out.problems.push(format!(
                            "{LEDGER}:{line}: an Owed row has {} cells, not 3 or 4",
                            cells.len()
                        ));
                        continue;
                    }
                    let step = cells[0].trim().to_string();
                    let items = backticked(&cells[1]);
                    let surfaces = if cells[2].trim().eq_ignore_ascii_case("every") {
                        SURFACES.to_vec()
                    } else {
                        let mut surfaces = Vec::new();
                        for name in cells[2].split(',') {
                            match Surface::parse(name) {
                                Some(surface) => surfaces.push(surface),
                                None => out.problems.push(format!(
                                    "{LEDGER}:{line}: `{}` is no surface",
                                    name.trim()
                                )),
                            }
                        }
                        surfaces
                    };
                    if items.is_empty() {
                        out.problems
                            .push(format!("{LEDGER}:{line}: an Owed row names no item"));
                    }
                    out.owed.push(Owed {
                        step,
                        items,
                        surfaces,
                    });
                }
            }
            "Refused by name" => {
                for (_, cells) in rows {
                    if let Some(what) = cells.first() {
                        out.refused.extend(backticked(what));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Each `## ` section's table rows, header and rule aside, with each row's
/// line number and cells.
fn tables(text: &str) -> Vec<(String, Vec<(usize, Vec<String>)>)> {
    let mut out: Vec<(String, Vec<(usize, Vec<String>)>)> = Vec::new();
    let mut header_seen = false;
    for (index, line) in text.lines().enumerate() {
        if let Some(title) = line.strip_prefix("## ") {
            out.push((title.trim().to_string(), Vec::new()));
            header_seen = false;
            continue;
        }
        let Some((_, rows)) = out.last_mut() else {
            continue;
        };
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            header_seen = false;
            continue;
        }
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect();
        if cells
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'))
        {
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue;
        }
        rows.push((index + 1, cells));
    }
    out
}

/// The backticked spans of `text`, in order.
fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(|span| span.trim().to_string())
        .filter(|span| !span.is_empty())
        .collect()
}

/// A projection cell: empty, `—`, or backticked names.
fn cell(text: &str) -> Cell {
    let text = text.trim();
    if text.is_empty() {
        Cell::Join
    } else if text == "—" || text == "-" {
        Cell::Absent
    } else {
        Cell::Names(backticked(text))
    }
}

/// Whether ledger name `pattern` matches item `path`: exactly, or `T::*`
/// matching `T` and everything under it.
fn matches(pattern: &str, path: &str) -> bool {
    match pattern.strip_suffix("::*") {
        Some(base) => path == base || path.starts_with(&format!("{base}::")),
        None => pattern == path,
    }
}

/// The steps of SU-12 a step file still holds.
fn open_steps(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("| SU-12."))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(|number| format!("SU-12.{number}"))
        .collect()
}

// ---- the join ------------------------------------------------------------------------

/// How one item stands on one surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Projected, by these names of the surface's.
    Projected(Vec<String>),
    /// Owed, to this step.
    Owed(String),
    Refused,
    /// None of the three: a gap the check fails on.
    Missing,
}

impl Standing {
    fn label(&self) -> String {
        match self {
            Standing::Projected(_) => "yes".to_string(),
            Standing::Owed(step) => format!("owed {step}"),
            Standing::Refused => "refused".to_string(),
            Standing::Missing => "MISSING".to_string(),
        }
    }
}

/// The snake-case spelling of a type's name: `DocumentEditor` →
/// `document_editor`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (at, ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            let after_lower = at > 0 && !chars[at - 1].is_ascii_uppercase();
            let before_lower = chars.get(at + 1).is_some_and(|c| c.is_ascii_lowercase());
            if at > 0 && (after_lower || before_lower) {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(*ch);
        }
    }
    out
}

/// The join of the facade, the surfaces and the ledger.
pub struct Coverage {
    /// Every item, and how it stands on each surface.
    pub standings: BTreeMap<String, BTreeMap<Surface, Standing>>,
    pub problems: Vec<String>,
}

/// Joins `items` with what `surfaces` export, through `ledger`, holding owed
/// rows to the steps `open` still lists.
pub fn join(
    items: &Items,
    surfaces: &Surfaces,
    ledger: &Ledger,
    open: &BTreeSet<String>,
) -> Coverage {
    let mut problems = ledger.problems.clone();
    let rows: BTreeMap<&str, &Projection> = ledger
        .projections
        .iter()
        .map(|row| (row.item.as_str(), row))
        .collect();

    // The ledger's names, held to the tree.
    let known = |name: &str| {
        items.items.contains_key(name)
            || items.parts.contains(name)
            || (name.ends_with("::*") && items.items.keys().any(|path| matches(name, path)))
    };
    for row in &ledger.projections {
        if !known(&row.item) {
            problems.push(format!(
                "Projections row `{}` names no facade item",
                row.item
            ));
        }
        for (surface, cell) in [
            (Surface::CAbi, &row.c),
            (Surface::Python, &row.python),
            (Surface::JavaScript, &row.javascript),
        ] {
            if let Cell::Names(names) = cell {
                for name in names {
                    if !has_name(surfaces, surface, name) {
                        problems.push(format!(
                            "Projections row `{}`: `{name}` is not on the {}",
                            row.item,
                            surface.name()
                        ));
                    }
                }
            }
        }
    }
    for row in &ledger.owed {
        if !row.step.starts_with("SU-12.") || !open.contains(&row.step) {
            problems.push(format!(
                "Owed row for {} names {}, which is no step of SU-12 still in {STEPS}",
                row.items.join(", "),
                row.step
            ));
        }
        for name in &row.items {
            if !known(name) {
                problems.push(format!(
                    "Owed row {}: `{name}` names no facade item",
                    row.step
                ));
            }
        }
    }

    // The names the Projections table gives a type, by surface and the
    // type's own name, for the join by name.
    let mut given: BTreeMap<(Surface, String), Vec<String>> = BTreeMap::new();
    for item in items.items.values().filter(|item| item.kind == Kind::Type) {
        let Some(row) = rows.get(item.path.as_str()) else {
            continue;
        };
        for (surface, cell) in [
            (Surface::CAbi, &row.c),
            (Surface::Python, &row.python),
            (Surface::JavaScript, &row.javascript),
        ] {
            if let Cell::Names(names) = cell {
                given
                    .entry((surface, item.name.clone()))
                    .or_default()
                    .extend(names.iter().cloned());
            }
        }
    }
    let aliases = |surface: Surface, type_name: &str| -> Vec<String> {
        given
            .get(&(surface, type_name.to_string()))
            .cloned()
            .unwrap_or_default()
    };

    let mut standings = BTreeMap::new();
    let mut owed_used: BTreeSet<(usize, usize)> = BTreeSet::new();
    for item in items.items.values() {
        let mut by_surface = BTreeMap::new();
        let c = c_projection(item, &rows, surfaces, items, &aliases);
        for surface in SURFACES {
            let projected = match surface {
                Surface::CAbi => c.clone(),
                Surface::Python | Surface::JavaScript => {
                    binding_projection(item, &rows, surface, surfaces, items, &aliases)
                }
                wrapper => c.as_ref().and_then(|names| {
                    let calls = surfaces.wrappers.get(&wrapper)?;
                    let any = item.kind == Kind::Type;
                    wrapper_has(names, calls, surfaces, any).then(|| names.clone())
                }),
            };
            let standing = match projected {
                Some(names) => Standing::Projected(names),
                None => {
                    let owed = ledger.owed.iter().enumerate().find_map(|(row_at, row)| {
                        if !row.surfaces.contains(&surface) {
                            return None;
                        }
                        row.items
                            .iter()
                            .position(|pattern| matches(pattern, &item.path))
                            .map(|pattern_at| (row_at, pattern_at, row.step.clone()))
                    });
                    if let Some((row_at, pattern_at, step)) = owed {
                        owed_used.insert((row_at, pattern_at));
                        Standing::Owed(step)
                    } else if ledger
                        .refused
                        .iter()
                        .any(|pattern| matches(pattern, &item.path))
                    {
                        Standing::Refused
                    } else {
                        Standing::Missing
                    }
                }
            };
            by_surface.insert(surface, standing);
        }
        let missing: Vec<&str> = by_surface
            .iter()
            .filter(|(_, standing)| **standing == Standing::Missing)
            .map(|(surface, _)| surface.name())
            .collect();
        if !missing.is_empty() {
            problems.push(format!(
                "`{}` is not on the {}, and is neither owed nor refused",
                item.path,
                missing.join(", ")
            ));
        }
        standings.insert(item.path.clone(), by_surface);
    }

    // An owed name that stands for no missing projection outlived its need.
    // A variant or a field crosses with its type, so its row is held only to
    // its step.
    for (row_at, row) in ledger.owed.iter().enumerate() {
        for (pattern_at, pattern) in row.items.iter().enumerate() {
            let is_part = items.parts.contains(pattern) && !items.items.contains_key(pattern);
            if !is_part && known(pattern) && !owed_used.contains(&(row_at, pattern_at)) {
                problems.push(format!(
                    "Owed row {}: `{pattern}` is projected on every surface it is owed on: take it off",
                    row.step
                ));
            }
        }
    }
    problems.sort();
    problems.dedup();
    Coverage {
        standings,
        problems,
    }
}

/// Whether `surface` has `name` as a projection row may name it.
fn has_name(surfaces: &Surfaces, surface: Surface, name: &str) -> bool {
    match surface {
        Surface::CAbi => surfaces.functions.contains_key(name) || surfaces.types.contains(name),
        Surface::Python => binding_has(&surfaces.python, name),
        Surface::JavaScript => binding_has(&surfaces.javascript, name),
        _ => false,
    }
}

/// Whether a binding has `Class.method`, a function, or a class.
fn binding_has(binding: &Binding, name: &str) -> bool {
    match name.split_once('.') {
        Some((class, method)) => binding
            .classes
            .get(class)
            .is_some_and(|methods| methods.contains(method)),
        None => binding.functions.contains(name) || binding.classes.contains_key(name),
    }
}

/// The rows of the Projections table, by the item each names.
type Rows<'a> = BTreeMap<&'a str, &'a Projection>;

/// The methods of the type at `path`: the items just after it in path order
/// whose paths it begins.
fn methods_of<'a>(items: &'a Items, path: &str) -> impl Iterator<Item = &'a Item> + 'a {
    let prefix = format!("{path}::");
    items
        .items
        .range(prefix.clone()..)
        .take_while(move |(other, _)| other.starts_with(&prefix))
        .map(|(_, item)| item)
        .filter(|item| item.kind == Kind::Method)
}

/// The C ABI's names for `item`, or `None` where it has none.
fn c_projection(
    item: &Item,
    rows: &Rows<'_>,
    surfaces: &Surfaces,
    items: &Items,
    aliases: &dyn Fn(Surface, &str) -> Vec<String>,
) -> Option<Vec<String>> {
    match rows.get(item.path.as_str()).map(|row| &row.c) {
        Some(Cell::Absent) => return None,
        Some(Cell::Names(names)) => {
            return names
                .iter()
                .all(|name| has_name(surfaces, Surface::CAbi, name))
                .then(|| names.clone());
        }
        Some(Cell::Join) | None => {}
    }
    match item.kind {
        Kind::Method => {
            let owner = item.owner.as_deref()?;
            let mut prefixes = vec![snake(owner)];
            for alias in aliases(Surface::CAbi, owner) {
                if let Some(rest) = alias.strip_prefix("Tpdf") {
                    prefixes.push(snake(rest));
                }
            }
            prefixes.into_iter().find_map(|prefix| {
                let name = format!("tpdf_{prefix}_{}", item.name);
                surfaces.functions.contains_key(&name).then(|| vec![name])
            })
        }
        Kind::Function => {
            let name = format!("tpdf_{}", item.name);
            surfaces.functions.contains_key(&name).then(|| vec![name])
        }
        Kind::Type => {
            // Its methods the C ABI crosses, else its own type.
            let methods: Vec<String> = methods_of(items, &item.path)
                .filter_map(|method| c_projection(method, rows, surfaces, items, aliases))
                .flatten()
                .collect();
            if !methods.is_empty() {
                return Some(methods);
            }
            let mut names = vec![format!("Tpdf{}", item.name)];
            names.extend(aliases(Surface::CAbi, &item.name));
            names
                .into_iter()
                .find(|name| surfaces.types.contains(name))
                .map(|name| vec![name])
        }
        Kind::Constant => None,
    }
}

/// Whether a wrapper that calls `calls` has an item the C ABI crosses by
/// `names`: every function among them called — any one, for a type, which
/// crosses by its methods — and for a type the C ABI crosses alone, a call to
/// a function whose declaration names it.
fn wrapper_has(names: &[String], calls: &BTreeSet<String>, surfaces: &Surfaces, any: bool) -> bool {
    let has = |name: &String| {
        if surfaces.functions.contains_key(name) {
            calls.contains(name)
        } else {
            surfaces
                .functions
                .iter()
                .any(|(function, types)| types.contains(name) && calls.contains(function))
        }
    };
    if any {
        names.iter().any(has)
    } else {
        names.iter().all(has)
    }
}

/// Python's or JavaScript's names for `item`, or `None` where it has none.
fn binding_projection(
    item: &Item,
    rows: &Rows<'_>,
    surface: Surface,
    surfaces: &Surfaces,
    items: &Items,
    aliases: &dyn Fn(Surface, &str) -> Vec<String>,
) -> Option<Vec<String>> {
    let row = rows.get(item.path.as_str()).copied();
    let (binding, cell) = match surface {
        Surface::Python => (&surfaces.python, row.map(|row| &row.python)),
        _ => (&surfaces.javascript, row.map(|row| &row.javascript)),
    };
    match cell {
        Some(Cell::Absent) => return None,
        Some(Cell::Names(names)) => {
            return names
                .iter()
                .all(|name| binding_has(binding, name))
                .then(|| names.clone());
        }
        Some(Cell::Join) | None => {}
    }
    let classes = |type_name: &str| -> Vec<String> {
        let mut names = vec![type_name.to_string()];
        if surface == Surface::JavaScript {
            names.push(format!("Pdf{type_name}"));
        }
        names.extend(aliases(surface, type_name));
        names
    };
    match item.kind {
        Kind::Method => {
            let owner = item.owner.as_deref()?;
            classes(owner).into_iter().find_map(|class| {
                let methods = binding.classes.get(&class)?;
                methods
                    .contains(&item.name)
                    .then(|| vec![format!("{class}.{}", item.name)])
            })
        }
        Kind::Function => binding
            .functions
            .contains(&item.name)
            .then(|| vec![item.name.clone()]),
        Kind::Type => {
            if let Some(class) = classes(&item.name)
                .into_iter()
                .find(|class| binding.classes.contains_key(class))
            {
                return Some(vec![class]);
            }
            let methods: Vec<String> = methods_of(items, &item.path)
                .filter_map(|method| {
                    binding_projection(method, rows, surface, surfaces, items, aliases)
                })
                .flatten()
                .collect();
            (!methods.is_empty()).then_some(methods)
        }
        Kind::Constant => None,
    }
}

// ---- the task ----------------------------------------------------------------------

/// The check's outcome over a tree: the coverage, with every problem.
pub fn coverage(tree: &Tree) -> Coverage {
    let items = facade(tree);
    let surfaces = surfaces(tree);
    let ledger = tree
        .get(LEDGER)
        .map_or_else(Ledger::default, |text| ledger(text));
    let open = tree
        .get(STEPS)
        .map_or_else(BTreeSet::new, |text| open_steps(text));
    join(&items, &surfaces, &ledger, &open)
}

/// `cargo xtask check`'s part: the problems, none printed.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let tree = read_tree(root).map_err(|problem| vec![problem])?;
    let outcome = coverage(&tree);
    if outcome.problems.is_empty() {
        Ok(())
    } else {
        Err(outcome.problems)
    }
}

/// `cargo xtask bindings-coverage`: every item listed against every surface,
/// one line each, then the tally, then the problems.
pub fn run(root: &Path) -> Result<(), Vec<String>> {
    let tree = read_tree(root).map_err(|problem| vec![problem])?;
    let outcome = coverage(&tree);
    let header: Vec<&str> = SURFACES.iter().map(|surface| surface.name()).collect();
    println!("item\t{}", header.join("\t"));
    let mut everywhere = 0usize;
    let mut owed = 0usize;
    let mut refused = 0usize;
    for (path, standings) in &outcome.standings {
        let cells: Vec<String> = SURFACES
            .iter()
            .map(|surface| {
                standings
                    .get(surface)
                    .map_or_else(String::new, Standing::label)
            })
            .collect();
        println!("{path}\t{}", cells.join("\t"));
        if standings
            .values()
            .all(|standing| matches!(standing, Standing::Projected(_)))
        {
            everywhere += 1;
        }
        if standings
            .values()
            .any(|standing| matches!(standing, Standing::Owed(_)))
        {
            owed += 1;
        }
        if standings
            .values()
            .any(|standing| *standing == Standing::Refused)
        {
            refused += 1;
        }
    }
    println!(
        "bindings-coverage: RAN {} facade items: {everywhere} on every surface, {owed} owed \
         somewhere, {refused} refused somewhere, {} problem(s)",
        outcome.standings.len(),
        outcome.problems.len()
    );
    if outcome.problems.is_empty() {
        Ok(())
    } else {
        Err(outcome.problems)
    }
}

#[cfg(test)]
mod tests;
