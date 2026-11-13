//! A stylesheet, generated (`css-syntax-3`, `selectors-4`, `css-cascade-5`),
//! and the element tree it cascades over, for `css_structured`.
//!
//! The stylesheet holds rule sets whose selectors take every combinator and
//! the pseudo-classes and pseudo-elements `selectors-4` defines that the
//! matcher implements, attribute selectors of every matcher and case flag,
//! and declarations of implemented, unsupported and unknown properties with
//! `!important` and the defaulting keywords; `@import`s with layers and media,
//! some naming each other or the sheet that imports them; `@media`,
//! `@supports` and `@layer` blocks nested inside one another, `@layer`
//! statements, `@font-face`, and an at-rule nothing defines.
//!
//! **It is written twice.** The second writing is the first with every
//! selector `S` written `:is(S)`, its pseudo-element kept outside — the same
//! elements matched at the same specificity (`selectors-4` §4.2 and §17),
//! every rule in the same place. The rule index buckets a selector by its
//! rightmost compound's id, class or type, and `:is(S)` has none of them, so
//! the second writing's every selector is a candidate for every element: its
//! cascade is the brute-force one.
//!
//! The caps — 16 items a sheet, three imported sheets, two levels of block and of
//! functional pseudo-class, four compounds a selector, six declarations a
//! block and 24 elements — are this module's constants.

use crate::Choices;

/// The most items a sheet's top level holds.
pub const MAX_RULES: usize = 16;
/// The most sheets `@import` names.
pub const MAX_IMPORTS: usize = 3;
/// How deep blocks nest, and functional pseudo-classes.
pub const MAX_NESTING: u32 = 2;
/// The most compounds a selector holds.
pub const MAX_COMPOUNDS: usize = 4;
/// The most declarations a block holds.
pub const MAX_DECLARATIONS: usize = 6;
/// The most elements the tree holds.
pub const MAX_ELEMENTS: usize = 24;

const NAMES: &[&str] = &[
    "p", "div", "span", "a", "li", "em", "ul", "h1", "section", "img", "input", "body",
];
const CLASSES: &[&str] = &["lead", "a", "b", "note", "x-y"];
const IDS: &[&str] = &["top", "x", "main"];
const LANGUAGES: &[&str] = &["en", "en-GB", "fr", "de-CH", "", "ar"];

/// Pseudo-classes with no argument, the seven of a reading session among
/// them.
const SIMPLE: &[&str] = &[
    "root",
    "first-child",
    "last-child",
    "only-child",
    "empty",
    "first-of-type",
    "last-of-type",
    "only-of-type",
    "link",
    "any-link",
    "checked",
    "disabled",
    "enabled",
    "required",
    "optional",
    "read-only",
    "read-write",
    "hover",
    "focus",
    "focus-within",
    "focus-visible",
    "active",
    "target",
    "visited",
];

const NTH: &[&str] = &[
    "odd", "even", "2n+1", "-n+3", "3", "0n+0", "n", "-2n+5", "2n - 1",
];

const PSEUDO_ELEMENTS: &[&str] = &["::before", "::after", "::first-letter", "::first-line"];

/// Attribute selectors of every matcher (§6.1 to §6.3) and both case flags.
const ATTRIBUTES: &[&str] = &[
    "[href]",
    "[lang|=en]",
    "[class~=a]",
    "[href^=\"ch\"]",
    "[href$=\".xhtml\"]",
    "[href*=apt]",
    "[lang=\"EN-gb\" i]",
    "[title=x s]",
    "[id=top]",
    "[dir=rtl]",
];

/// Declarations of implemented properties, each value one the property
/// takes.
const KNOWN: &[(&str, &[&str])] = &[
    (
        "color",
        &[
            "red",
            "#0a0",
            "rgb(1 2 3 / 50%)",
            "currentcolor",
            "transparent",
        ],
    ),
    (
        "background-color",
        &["#fff", "hsl(120 50% 50%)", "transparent"],
    ),
    (
        "display",
        &[
            "block",
            "none",
            "inline",
            "inline-block",
            "list-item",
            "table",
            "flex",
        ],
    ),
    (
        "font-size",
        &[
            "12pt",
            "120%",
            "larger",
            "2em",
            "1.5rem",
            "small",
            "calc(1em + 2px)",
        ],
    ),
    ("font-weight", &["bold", "400", "bolder", "lighter"]),
    ("font-style", &["italic", "normal", "oblique"]),
    (
        "font-family",
        &["serif", "\"Gentium\", sans-serif", "monospace"],
    ),
    ("line-height", &["1.5", "normal", "20px", "150%"]),
    (
        "margin",
        &["1em 2px", "0", "auto", "1em 2em 3em 4em", "-1px"],
    ),
    ("padding", &["2px", "1em 0", "5%"]),
    (
        "text-align",
        &["center", "justify", "start", "end", "right"],
    ),
    ("text-indent", &["2em", "-1em", "10%"]),
    ("text-transform", &["uppercase", "capitalize", "none"]),
    ("white-space", &["pre", "nowrap", "pre-wrap", "normal"]),
    ("float", &["left", "right", "none"]),
    ("clear", &["both", "left"]),
    ("width", &["50%", "10em", "auto", "max-content"]),
    ("height", &["2em", "auto"]),
    ("border", &["1px solid red", "thin dotted", "none"]),
    ("border-radius", &["4px", "50%"]),
    (
        "list-style-type",
        &["disc", "decimal", "lower-roman", "none", "\"-\""],
    ),
    ("counter-reset", &["section", "item 3", "none"]),
    ("counter-increment", &["section", "item 2"]),
    (
        "content",
        &[
            "\"x\"",
            "counter(section)",
            "attr(title)",
            "none",
            "\"a\" counter(item, upper-alpha)",
        ],
    ),
    ("visibility", &["hidden", "visible", "collapse"]),
    ("direction", &["rtl", "ltr"]),
    ("unicode-bidi", &["embed", "isolate", "bidi-override"]),
    ("letter-spacing", &["0.1em", "normal"]),
    ("word-spacing", &["2px", "normal"]),
    ("vertical-align", &["super", "middle", "-2px"]),
    ("opacity", &["0.5", "1"]),
    ("page-break-before", &["always", "avoid"]),
    ("break-inside", &["avoid", "auto"]),
    ("position", &["relative", "absolute", "static"]),
    ("z-index", &["2", "auto"]),
    ("hyphens", &["auto", "manual"]),
];

/// §7.1's defaulting keywords, which every property takes.
const DEFAULTING: &[&str] = &["inherit", "initial", "unset", "revert", "revert-layer"];

/// Names no table holds, and names the unsupported table does.
const OTHER: &[&str] = &[
    "x-foo",
    "colour",
    "-webkit-box",
    "grid-area",
    "cursor",
    "filter",
];

const MEDIA: &[&str] = &[
    "screen",
    "print",
    "all",
    "not print",
    "(min-width: 100px)",
    "screen and (max-width: 1px)",
    "(orientation: portrait)",
    "only screen and (color)",
];

const SUPPORTS: &[&str] = &[
    "(display: flex)",
    "(display: frobnicate)",
    "not (color: red)",
    "((float: left) or (x-foo: 1))",
    "selector(:is(p))",
];

const LAYERS: &[&str] = &["base", "theme", "base.inner", "x"];

/// One element of the tree, in document order.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: &'static str,
    pub id: Option<&'static str>,
    pub classes: Vec<String>,
    pub attributes: Vec<(&'static str, String)>,
    pub style: Option<String>,
    pub parent: Option<usize>,
    pub previous: Option<usize>,
    pub next: Option<usize>,
    pub children: usize,
}

impl tinker_pdf_css::Element for Node {
    fn local_name(&self) -> &str {
        self.name
    }
    fn id(&self) -> Option<&str> {
        self.id
    }
    fn classes(&self) -> &[String] {
        &self.classes
    }
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
    fn parent(&self) -> Option<usize> {
        self.parent
    }
    fn previous_sibling(&self) -> Option<usize> {
        self.previous
    }
    fn next_sibling(&self) -> Option<usize> {
        self.next
    }
    fn inline_style(&self) -> Option<&str> {
        self.style.as_deref()
    }
    fn is_empty(&self) -> bool {
        self.children == 0
    }
    fn language(&self) -> Option<&str> {
        self.attribute("lang")
    }
    fn direction(&self) -> Option<&str> {
        self.attribute("dir")
    }
    fn is_link(&self) -> bool {
        self.name == "a" && self.attribute("href").is_some()
    }
}

/// A generated stylesheet, its imported sheets, and the tree.
pub struct Generated {
    /// The stylesheet, and the same written with every selector in `:is()`.
    pub sheet: Vec<u8>,
    pub wrapped: Vec<u8>,
    /// Each sheet an `@import` may name: its address, and both writings.
    pub imports: Vec<(String, Vec<u8>, Vec<u8>)>,
    pub nodes: Vec<Node>,
    /// Combinators written, one bit each: descendant, child, next sibling,
    /// subsequent sibling.
    pub combinators: u8,
    /// Functional pseudo-classes written, simple ones, pseudo-elements,
    /// attribute selectors.
    pub functional: usize,
    pub simple: usize,
    pub pseudo_elements: usize,
    pub attribute_selectors: usize,
    /// At-rules written, one bit each: `@import`, `@media`, `@supports`,
    /// `@layer` blocks, `@layer` statements, `@font-face`, unknown.
    pub at_rules: u8,
    /// `@import`s that name a sheet already being read.
    pub cycles: usize,
    pub important: usize,
    pub defaulted: usize,
    pub unknown: usize,
    pub inline: usize,
}

/// Both writings of one piece: as written, and with its selectors wrapped.
#[derive(Default)]
struct Two {
    plain: String,
    wrapped: String,
}

impl Two {
    fn both(&mut self, text: &str) {
        self.plain.push_str(text);
        self.wrapped.push_str(text);
    }
}

struct Writer<'c, 'a> {
    c: &'c mut Choices<'a>,
    out: Generated,
}

impl Writer<'_, '_> {
    /// One compound: a type or `*` first, then ids, classes, attributes and
    /// pseudo-classes, and `*` where nothing else was written.
    fn compound(&mut self, depth: u32, out: &mut String) {
        let start = out.len();
        match self.c.below(4) {
            0 => out.push_str(self.c.pick(NAMES)),
            1 => out.push('*'),
            _ => {}
        }
        for _ in 0..self.c.below(4) {
            match self.c.below(6) {
                0 => {
                    out.push('#');
                    out.push_str(self.c.pick(IDS));
                }
                1 | 2 => {
                    out.push('.');
                    out.push_str(self.c.pick(CLASSES));
                }
                3 => {
                    out.push_str(self.c.pick(ATTRIBUTES));
                    self.out.attribute_selectors += 1;
                }
                _ => self.pseudo_class(depth, out),
            }
        }
        if out.len() == start {
            out.push('*');
        }
    }

    fn pseudo_class(&mut self, depth: u32, out: &mut String) {
        if depth >= MAX_NESTING || self.c.flag() {
            out.push(':');
            out.push_str(self.c.pick(SIMPLE));
            self.out.simple += 1;
            return;
        }
        self.out.functional += 1;
        match self.c.below(9) {
            0 => out.push_str(&format!(":nth-child({})", self.c.pick(NTH))),
            1 => out.push_str(&format!(":nth-last-child({})", self.c.pick(NTH))),
            2 => out.push_str(&format!(":nth-of-type({})", self.c.pick(NTH))),
            3 => out.push_str(&format!(":nth-last-of-type({})", self.c.pick(NTH))),
            4 => {
                let of = self.list(depth + 1);
                out.push_str(&format!(":nth-child({} of {of})", self.c.pick(NTH)));
            }
            5 => {
                let name = *self.c.pick(&["not", "is", "where", "matches"]);
                let list = self.list(depth + 1);
                out.push_str(&format!(":{name}({list})"));
            }
            6 => {
                // §4.5's relative selectors, and no `:has()` inside one.
                let mut relative = String::new();
                for k in 0..1 + self.c.below(2) {
                    if k > 0 {
                        relative.push_str(", ");
                    }
                    match self.c.below(4) {
                        0 => relative.push_str("> "),
                        1 => relative.push_str("+ "),
                        2 => relative.push_str("~ "),
                        _ => {}
                    }
                    let inner = self.complex(MAX_NESTING);
                    relative.push_str(&inner);
                }
                out.push_str(&format!(":has({relative})"));
            }
            7 => {
                let range = *self.c.pick(&["en", "\"fr\"", "\"*-CH\"", "en, ar"]);
                out.push_str(&format!(":lang({range})"));
            }
            _ => {
                let which = *self.c.pick(&["ltr", "rtl"]);
                out.push_str(&format!(":dir({which})"));
            }
        }
    }

    /// A complex selector: compounds joined by combinators.
    fn complex(&mut self, depth: u32) -> String {
        let mut out = String::new();
        let count = 1 + self.c.below(MAX_COMPOUNDS);
        for k in 0..count {
            if k > 0 {
                let (text, bit) = match self.c.below(4) {
                    0 => (" ", 1u8),
                    1 => (" > ", 2),
                    2 => (" + ", 4),
                    _ => (" ~ ", 8),
                };
                out.push_str(text);
                self.out.combinators |= bit;
            }
            self.compound(depth, &mut out);
        }
        out
    }

    /// A selector list inside a functional pseudo-class.
    fn list(&mut self, depth: u32) -> String {
        let count = 1 + self.c.below(2);
        (0..count)
            .map(|_| self.complex(depth))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// A rule's selector list, both writings: each selector `S` and its
    /// pseudo-element, and `:is(S)` before the same pseudo-element.
    fn selectors(&mut self, out: &mut Two) {
        let count = 1 + self.c.below(3);
        for k in 0..count {
            if k > 0 {
                out.both(", ");
            }
            let selector = self.complex(0);
            let pseudo = if self.c.below(5) == 4 {
                self.out.pseudo_elements += 1;
                *self.c.pick(PSEUDO_ELEMENTS)
            } else {
                ""
            };
            out.plain.push_str(&selector);
            out.plain.push_str(pseudo);
            out.wrapped.push_str(":is(");
            out.wrapped.push_str(&selector);
            out.wrapped.push(')');
            out.wrapped.push_str(pseudo);
        }
    }

    /// A declaration block's contents.
    fn declarations(&mut self) -> String {
        let mut out = String::new();
        for _ in 0..self.c.below(MAX_DECLARATIONS + 1) {
            let (name, value) = match self.c.below(10) {
                0 => {
                    self.out.defaulted += 1;
                    let (name, _) = *self.c.pick(KNOWN);
                    (name, *self.c.pick(DEFAULTING))
                }
                1 => {
                    self.out.unknown += 1;
                    (*self.c.pick(OTHER), "1")
                }
                _ => {
                    let (name, values) = *self.c.pick(KNOWN);
                    (name, *self.c.pick(values))
                }
            };
            out.push_str(name);
            out.push_str(": ");
            out.push_str(value);
            if self.c.below(6) == 5 {
                out.push_str(" !important");
                self.out.important += 1;
            }
            out.push_str("; ");
        }
        out
    }

    /// One item of a sheet or a block.
    fn item(&mut self, depth: u32, out: &mut Two) {
        match self.c.below(12) {
            8 if depth < MAX_NESTING => {
                self.out.at_rules |= 2;
                let query = *self.c.pick(MEDIA);
                out.both(&format!("@media {query} {{ "));
                self.block(depth + 1, out);
                out.both("} ");
            }
            9 if depth < MAX_NESTING => {
                self.out.at_rules |= 4;
                let condition = *self.c.pick(SUPPORTS);
                out.both(&format!("@supports {condition} {{ "));
                self.block(depth + 1, out);
                out.both("} ");
            }
            10 if depth < MAX_NESTING => {
                self.out.at_rules |= 8;
                if self.c.flag() {
                    let name = *self.c.pick(LAYERS);
                    out.both(&format!("@layer {name} {{ "));
                } else {
                    out.both("@layer { ");
                }
                self.block(depth + 1, out);
                out.both("} ");
            }
            11 => match self.c.below(3) {
                0 => {
                    self.out.at_rules |= 32;
                    let family = *self.c.pick(&["\"Gentium\"", "X", "\"Noto Serif\""]);
                    let weight = *self.c.pick(&["400", "bold", "100 900"]);
                    out.both(&format!(
                        "@font-face {{ font-family: {family}; src: url(f.woff2) format(\"woff2\"), local(\"X\"); font-weight: {weight}; }} "
                    ));
                }
                1 => {
                    self.out.at_rules |= 64;
                    out.both("@frobnicate x { p { color: red } } ");
                }
                _ => {
                    self.out.at_rules |= 16;
                    let a = *self.c.pick(LAYERS);
                    let b = *self.c.pick(LAYERS);
                    out.both(&format!("@layer {a}, {b}; "));
                }
            },
            _ => {
                self.selectors(out);
                out.both(" { ");
                let block = self.declarations();
                out.both(&block);
                out.both("} ");
            }
        }
    }

    fn block(&mut self, depth: u32, out: &mut Two) {
        for _ in 0..1 + self.c.below(4) {
            self.item(depth, out);
        }
    }

    /// A whole sheet: its imports first, as §6.4.1 places them, then its
    /// items. `addresses` are the sheets an `@import` may name.
    fn sheet(&mut self, addresses: &[String], reading: &[String]) -> Two {
        let mut out = Two::default();
        for _ in 0..self.c.below(3) {
            if addresses.is_empty() {
                break;
            }
            let href = addresses[self.c.below(addresses.len())].clone();
            if reading.contains(&href) {
                self.out.cycles += 1;
            }
            self.out.at_rules |= 1;
            let tail = match self.c.below(4) {
                0 => " layer(base)".to_string(),
                1 => " layer".to_string(),
                2 => format!(" {}", self.c.pick(MEDIA)),
                _ => String::new(),
            };
            if self.c.flag() {
                out.both(&format!("@import url({href}){tail}; "));
            } else {
                out.both(&format!("@import \"{href}\"{tail}; "));
            }
        }
        for _ in 0..self.c.below(MAX_RULES + 1) {
            self.item(0, &mut out);
        }
        out
    }

    /// The tree: elements in document order, each one's children after it.
    fn tree(&mut self) {
        let mut stack: Vec<usize> = Vec::new();
        let count = 1 + self.c.below(MAX_ELEMENTS);
        for index in 0..count {
            // The parent is an open element; closing elements walks back up.
            let pops = if index == 0 {
                0
            } else {
                self.c.below(stack.len() + 1)
            };
            for _ in 0..pops.min(stack.len().saturating_sub(1)) {
                stack.pop();
            }
            let parent = stack.last().copied();
            let mut classes = Vec::new();
            for _ in 0..self.c.below(3) {
                classes.push((*self.c.pick(CLASSES)).to_string());
            }
            let id = self.c.flag().then(|| *self.c.pick(IDS));
            let mut attributes: Vec<(&'static str, String)> = Vec::new();
            if self.c.flag() {
                let href = *self.c.pick(&["chapter.xhtml", "#n1", "apt.xhtml"]);
                attributes.push(("href", href.to_string()));
            }
            if self.c.below(3) == 2 {
                attributes.push(("lang", (*self.c.pick(LANGUAGES)).to_string()));
            }
            if self.c.below(4) == 3 {
                attributes.push(("dir", (*self.c.pick(&["rtl", "ltr", "auto"])).to_string()));
            }
            if self.c.below(4) == 3 {
                attributes.push(("title", (*self.c.pick(&["x", "X", "y"])).to_string()));
            }
            if !classes.is_empty() {
                attributes.push(("class", classes.join(" ")));
            }
            if let Some(id) = id {
                attributes.push(("id", id.to_string()));
            }
            let style = (self.c.below(4) == 3).then(|| {
                self.out.inline += 1;
                self.declarations()
            });
            let previous = (0..index)
                .rev()
                .find(|e| self.out.nodes[*e].parent == parent);
            if let Some(previous) = previous {
                self.out.nodes[previous].next = Some(index);
            }
            if let Some(parent) = parent {
                self.out.nodes[parent].children += 1;
            }
            self.out.nodes.push(Node {
                name: *self.c.pick(NAMES),
                id,
                classes,
                attributes,
                style,
                parent,
                previous,
                next: None,
                children: 0,
            });
            stack.push(index);
        }
    }
}

/// A stylesheet, the sheets it may import, and a tree, built from `c`.
pub fn generate(c: &mut Choices) -> Generated {
    let mut w = Writer {
        c,
        out: Generated {
            sheet: Vec::new(),
            wrapped: Vec::new(),
            imports: Vec::new(),
            nodes: Vec::new(),
            combinators: 0,
            functional: 0,
            simple: 0,
            pseudo_elements: 0,
            attribute_selectors: 0,
            at_rules: 0,
            cycles: 0,
            important: 0,
            defaulted: 0,
            unknown: 0,
            inline: 0,
        },
    };
    let count = w.c.below(MAX_IMPORTS + 1);
    let addresses: Vec<String> = (0..count).map(|k| format!("i{k}.css")).collect();
    let mut every = addresses.clone();
    every.push("root.css".to_string());
    let root = w.sheet(&addresses, &["root.css".to_string()]);
    for href in &addresses {
        let sheet = w.sheet(&every, &[href.clone(), "root.css".to_string()]);
        w.out.imports.push((
            href.clone(),
            sheet.plain.into_bytes(),
            sheet.wrapped.into_bytes(),
        ));
    }
    w.tree();
    let mut out = w.out;
    out.sheet = root.plain.into_bytes();
    out.wrapped = root.wrapped.into_bytes();
    out
}

/// The sheets a generated stylesheet imports, as the parser asks for them:
/// one writing or the other.
pub struct Imports<'a> {
    pub sheets: &'a [(String, Vec<u8>, Vec<u8>)],
    pub wrapped: bool,
    /// The root sheet's own bytes, for an `@import` that names it.
    pub root: &'a [u8],
}

impl tinker_pdf_css::ImportResolver for Imports<'_> {
    fn resolve(&self, href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
        if href == "root.css" {
            return Some(("root.css".to_string(), self.root.to_vec()));
        }
        self.sheets
            .iter()
            .find(|(address, _, _)| address == href)
            .map(|(address, plain, wrapped)| {
                let body = if self.wrapped { wrapped } else { plain };
                (address.clone(), body.clone())
            })
    }
}

#[cfg(test)]
mod tests;
