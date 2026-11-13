//! One generated element tree, written two ways: as XML 1.0 with namespaces,
//! well formed or with one defect it names, for `xml_structured`; and as HTML
//! tag soup aimed at the tree builder's insertion modes, for
//! `html_structured`.
//!
//! The tree's element names are HTML's — tables and their parts, `template`,
//! `select`, the formatting elements the adoption agency reorders, raw-text
//! elements, `svg` and `math` — so one tree reaches the HTML tree builder's
//! modes and is an ordinary XML document too. Its namespace declarations,
//! prefixes, CDATA sections and processing instructions are XML's, and the
//! HTML writer spells them as HTML reads them: a prefix is part of no name,
//! and CDATA and a processing instruction are bogus comments outside foreign
//! content.
//!
//! The caps — 40 elements, eight deep, five children and four attributes an
//! element — are this module's constants.

use crate::Choices;

/// The most elements a tree holds.
pub const MAX_ELEMENTS: usize = 40;
/// How deep elements nest.
pub const MAX_DEPTH: usize = 8;
/// The most children an element holds.
pub const MAX_CHILDREN: usize = 5;
/// The most attributes an element holds, namespace declarations aside.
pub const MAX_ATTRIBUTES: usize = 4;
/// The most characters a run of generated text holds.
pub const MAX_RUN: usize = 12;

/// Element names: HTML's, chosen for the insertion modes they reach.
const ELEMENTS: &[&str] = &[
    "div",
    "span",
    "p",
    "b",
    "i",
    "a",
    "em",
    "strong",
    "table",
    "tbody",
    "thead",
    "tfoot",
    "tr",
    "td",
    "th",
    "caption",
    "colgroup",
    "col",
    "template",
    "select",
    "option",
    "optgroup",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "form",
    "button",
    "font",
    "nobr",
    "u",
    "s",
    "small",
    "big",
    "code",
    "tt",
    "h1",
    "h2",
    "section",
    "article",
    "aside",
    "nav",
    "header",
    "footer",
    "main",
    "figure",
    "blockquote",
    "pre",
    "listing",
    "textarea",
    "title",
    "style",
    "script",
    "noscript",
    "iframe",
    "svg",
    "math",
    "mi",
    "mtext",
    "foreignObject",
    "desc",
    "annotation-xml",
    "img",
    "br",
    "hr",
    "input",
    "meta",
    "link",
    "head",
    "body",
    "html",
    "frameset",
    "frame",
    "object",
    "applet",
    "marquee",
    "ruby",
    "rt",
    "rp",
    "image",
    "xmp",
    "noembed",
    "noframes",
    "address",
    "center",
    "details",
    "summary",
    "dialog",
    "menu",
    "search",
    "hgroup",
    "fieldset",
    "legend",
    "label",
    "keygen",
    "embed",
    "area",
    "param",
    "source",
    "track",
    "wbr",
    "basefont",
    "plaintext",
];

/// The formatting elements of §13.2.4.3, which the adoption agency reorders.
const FORMATTING: &[&str] = &[
    "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u",
];

/// Elements whose content HTML reads as text (§13.1.2).
const RAW: &[&str] = &[
    "script",
    "style",
    "textarea",
    "title",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "plaintext",
];

/// Attribute names.
const ATTRIBUTES: &[&str] = &[
    "id",
    "class",
    "href",
    "style",
    "title",
    "lang",
    "type",
    "name",
    "value",
    "src",
    "width",
    "colspan",
    "rowspan",
    "data-x",
    "encoding",
    "definitionURL",
    "viewBox",
    "space",
    "base",
];

/// Prefixes the generator binds, and the namespaces it binds them to — none
/// of them the two Namespaces §3 reserves.
const PREFIXES: &[&str] = &["p", "q", "svg", "x-y", "m"];
const URIS: &[&str] = &[
    "urn:a",
    "urn:b",
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1999/xhtml",
    "http://example.com/n",
];

/// The five entities XML predefines (§4.6), and the characters they stand
/// for.
const ENTITIES: &[(&str, char)] = &[
    ("lt", '<'),
    ("gt", '>'),
    ("amp", '&'),
    ("apos", '\''),
    ("quot", '"'),
];

/// Characters a character reference may name, the ones XML says are
/// characters (§2.2) and both sides of the ranges.
const REFERENCED: &[char] = &[
    'A',
    '\t',
    '\n',
    '\r',
    '<',
    '&',
    '"',
    ']',
    'é',
    '\u{D7FF}',
    '\u{E000}',
    '\u{FFFD}',
    '中',
    '\u{10000}',
    '😀',
    '\u{10FFFF}',
];

/// Literal characters for text: no `<`, `&` or `]`, so no markup and no
/// `]]>` is spelled by accident.
const TEXT: &[char] = &[
    'a', 'b', 'c', 'x', 'y', 'z', ' ', ' ', '0', '7', '.', ',', ';', ':', '!', '?', '-', '_', '(',
    ')', '>', '"', '\'', '=', '/', 'é', 'ß', '中', 'ア', '😀', '\n',
];

/// What one piece of character data or of an attribute value is.
#[derive(Clone, Debug)]
pub enum Piece {
    /// Literal characters.
    Literal(String),
    /// A carriage return and a line feed, which §2.11 reads as one line feed.
    CrLf,
    /// A predefined entity (§4.6), by its index in the table.
    Entity(usize),
    /// A character reference (§4.1), decimal or hexadecimal.
    Char(char, bool),
}

/// An attribute: its name, its prefix, its value.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: &'static str,
    /// An index into the prefixes, `Some(usize::MAX)` for `xml`.
    pub prefix: Option<usize>,
    pub value: Vec<Piece>,
}

/// One element.
#[derive(Clone, Debug)]
pub struct Element {
    pub name: &'static str,
    /// An index into the prefixes bound where the element is.
    pub prefix: Option<usize>,
    /// The declarations it carries: a prefix (or the default namespace) and
    /// the namespace it binds.
    pub declares: Vec<(Option<usize>, usize)>,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
}

/// One node of the tree.
#[derive(Clone, Debug)]
pub enum Node {
    Element(Element),
    Text(Vec<Piece>),
    Cdata(String),
    Comment(String),
    Instruction(&'static str, String),
}

/// The tree, and how many elements it holds.
pub struct Tree {
    pub root: Element,
    pub elements: usize,
}

fn run(c: &mut Choices, alphabet: &[char]) -> String {
    let len = 1 + c.below(MAX_RUN);
    (0..len).map(|_| *c.pick(alphabet)).collect()
}

/// Character data: literal runs, references and line ends.
fn pieces(c: &mut Choices, attribute: bool) -> Vec<Piece> {
    let count = 1 + c.below(4);
    (0..count)
        .map(|_| match c.below(8) {
            0 => Piece::Entity(c.below(ENTITIES.len())),
            1 => Piece::Char(*c.pick(REFERENCED), c.flag()),
            2 => Piece::CrLf,
            _ => {
                let mut text = run(c, TEXT);
                if attribute && c.below(4) == 3 {
                    // §3.3.3's literal white space, which becomes a space.
                    text.push(if c.flag() { '\t' } else { '\n' });
                }
                Piece::Literal(text)
            }
        })
        .collect()
}

/// What the scope binds: each prefix's namespace, and the default one.
#[derive(Clone, Default)]
struct Scope {
    prefixes: Vec<(usize, usize)>,
}

impl Scope {
    fn bound(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.prefixes.iter().map(|(p, _)| *p).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn uri(&self, prefix: usize) -> Option<usize> {
        self.prefixes
            .iter()
            .rev()
            .find(|(p, _)| *p == prefix)
            .map(|(_, u)| *u)
    }
}

fn element(c: &mut Choices, depth: usize, scope: &Scope, count: &mut usize) -> Element {
    *count += 1;
    let name = *c.pick(ELEMENTS);
    let mut declares: Vec<(Option<usize>, usize)> = Vec::new();
    for _ in 0..c.below(3) {
        let prefix = if c.flag() {
            Some(c.below(PREFIXES.len()))
        } else {
            None
        };
        let uri = c.below(URIS.len());
        if !declares.iter().any(|(p, _)| *p == prefix) {
            declares.push((prefix, uri));
        }
    }
    let mut inner = scope.clone();
    for &(prefix, uri) in &declares {
        if let Some(p) = prefix {
            inner.prefixes.push((p, uri));
        }
    }
    let bound = inner.bound();
    let prefix = if !bound.is_empty() && c.below(3) == 2 {
        Some(bound[c.below(bound.len())])
    } else {
        None
    };
    // Namespaces §6.3: no two attributes share an expanded name.
    let mut expanded: Vec<(Option<usize>, &str)> = Vec::new();
    let mut attributes = Vec::new();
    for _ in 0..c.below(MAX_ATTRIBUTES + 1) {
        let name = *c.pick(ATTRIBUTES);
        let prefix = match c.below(6) {
            5 if name == "lang" || name == "space" || name == "base" => Some(usize::MAX),
            4 if !bound.is_empty() => Some(bound[c.below(bound.len())]),
            _ => None,
        };
        let namespace = match prefix {
            Some(usize::MAX) => Some(usize::MAX),
            Some(p) => inner.uri(p),
            None => None,
        };
        let value = pieces(c, true);
        if expanded.contains(&(namespace, name)) {
            continue;
        }
        expanded.push((namespace, name));
        attributes.push(Attribute {
            name,
            prefix,
            value,
        });
    }
    let mut children = Vec::new();
    if depth + 1 < MAX_DEPTH {
        for _ in 0..c.below(MAX_CHILDREN + 1) {
            let node = match c.below(10) {
                0..=3 if *count < MAX_ELEMENTS => {
                    Node::Element(element(c, depth + 1, &inner, count))
                }
                0..=5 => Node::Text(pieces(c, false)),
                6 => {
                    let mut body = run(c, &['a', ' ', '<', '&', '>', ']', 'é', '\n']);
                    while body.contains("]]>") {
                        body = body.replace("]]>", "]>");
                    }
                    Node::Cdata(body)
                }
                7 => {
                    let mut body = run(c, &['a', ' ', '-', '<', '&', 'é']);
                    while body.contains("--") {
                        body = body.replace("--", "-");
                    }
                    if body.ends_with('-') {
                        body.push('x');
                    }
                    Node::Comment(body)
                }
                8 => {
                    let target = *c.pick(&["pi", "php", "target", "a-b"]);
                    let value = if c.flag() {
                        run(c, &['a', ' ', '>', '<', 'é']).trim_start().to_string()
                    } else {
                        String::new()
                    };
                    Node::Instruction(target, value)
                }
                _ => Node::Text(pieces(c, false)),
            };
            children.push(node);
        }
    }
    Element {
        name,
        prefix,
        declares,
        attributes,
        children,
    }
}

/// A tree built from `c`.
pub fn tree(c: &mut Choices) -> Tree {
    let mut count = 0;
    let root = element(c, 0, &Scope::default(), &mut count);
    Tree {
        root,
        elements: count,
    }
}

// --- XML -----------------------------------------------------------------

/// A defect the XML writer may put in a document, which the reader must
/// refuse as the error it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defect {
    /// A prefix no declaration in scope binds (Namespaces §5.1).
    UndeclaredPrefix,
    /// An end tag naming another element than the one it closes (§3, the
    /// Element Type Match constraint).
    MismatchedEnd,
    /// Two attributes with one name, or with one expanded name (§3.1,
    /// Namespaces §6.3).
    DuplicateAttribute,
    /// A `&` that begins no reference (§4.1).
    BareAmpersand,
}

/// A document type declaration before the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Declaration {
    /// `<!DOCTYPE name>`.
    Bare,
    /// With an external identifier.
    External,
    /// With an internal subset, which declares an entity.
    Subset,
}

/// What a reader must read out of a well-formed document: each element's
/// qualified name and attributes in document order, every character of its
/// character data, and its comments and instructions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expected {
    pub elements: Vec<(String, Vec<(String, String)>)>,
    pub text: String,
    pub comments: Vec<String>,
    pub instructions: Vec<(String, String)>,
}

/// A written XML document and what the generator knows of it.
pub struct XmlDocument {
    pub bytes: Vec<u8>,
    pub expected: Expected,
    pub defect: Option<Defect>,
    pub declaration: Option<Declaration>,
    /// Elements with a prefix, attributes with one, and default namespace
    /// declarations.
    pub prefixed: usize,
    pub defaults: usize,
    pub cdata: usize,
    pub references: usize,
    pub instructions: usize,
    pub depth: usize,
    pub line_ends: usize,
}

fn qualified(name: &str, prefix: Option<usize>) -> String {
    match prefix {
        Some(usize::MAX) => format!("xml:{name}"),
        Some(p) => format!("{}:{name}", PREFIXES[p]),
        None => name.to_string(),
    }
}

/// Character data as XML spells it, and what it reads as.
fn xml_pieces(
    pieces: &[Piece],
    quote: Option<char>,
    out: &mut String,
    read: &mut String,
    doc: &mut XmlDocument,
) {
    for piece in pieces {
        match piece {
            Piece::Literal(text) => {
                for ch in text.chars() {
                    if Some(ch) == quote {
                        out.push_str(if ch == '"' { "&quot;" } else { "&apos;" });
                        read.push(ch);
                        continue;
                    }
                    out.push(ch);
                    // §3.3.3: a literal tab or line end in a value is a space.
                    read.push(if quote.is_some() && (ch == '\t' || ch == '\n') {
                        ' '
                    } else {
                        ch
                    });
                }
            }
            Piece::CrLf => {
                out.push_str("\r\n");
                doc.line_ends += 1;
                read.push(if quote.is_some() { ' ' } else { '\n' });
            }
            Piece::Entity(i) => {
                let (name, ch) = ENTITIES[*i];
                out.push('&');
                out.push_str(name);
                out.push(';');
                read.push(ch);
                doc.references += 1;
            }
            Piece::Char(ch, hex) => {
                if *hex {
                    out.push_str(&format!("&#x{:X};", u32::from(*ch)));
                } else {
                    out.push_str(&format!("&#{};", u32::from(*ch)));
                }
                read.push(*ch);
                doc.references += 1;
            }
        }
    }
}

struct XmlWriter<'c, 'a> {
    c: &'c mut Choices<'a>,
    out: String,
    doc: XmlDocument,
    /// The preorder index of the element a defect goes in, and how many have
    /// been written.
    target: usize,
    seen: usize,
}

impl XmlWriter<'_, '_> {
    fn space(&mut self) {
        let s = *self.c.pick(&[" ", "  ", "\n", "\t", " \r\n "]);
        self.out.push_str(s);
    }

    fn element(&mut self, el: &Element, depth: usize) {
        let here = self.seen;
        self.seen += 1;
        self.doc.depth = self.doc.depth.max(depth + 1);
        let defect_here = self.target == here;
        let mut prefix = el.prefix;
        if defect_here && self.doc.defect == Some(Defect::UndeclaredPrefix) {
            // A prefix nothing binds: one of the generator's own, written
            // with a letter no prefix of theirs has.
            prefix = None;
            self.out.push('<');
            self.out.push_str("undeclared:");
            self.out.push_str(el.name);
        } else {
            self.out.push('<');
            self.out.push_str(&qualified(el.name, prefix));
        }
        if prefix.is_some() {
            self.doc.prefixed += 1;
        }
        let qname = qualified(el.name, prefix);
        let mut attributes = Vec::new();
        // The declarations and the attributes, the declarations among them
        // where the input puts them.
        let mut order: Vec<Option<usize>> = (0..el.attributes.len()).map(Some).collect();
        for _ in &el.declares {
            let at = self.c.below(order.len() + 1);
            order.insert(at, None);
        }
        let mut declarations = el.declares.iter();
        for slot in order {
            self.space();
            match slot {
                None => {
                    if let Some(&(p, uri)) = declarations.next() {
                        match p {
                            Some(p) => {
                                self.out.push_str("xmlns:");
                                self.out.push_str(PREFIXES[p]);
                            }
                            None => {
                                self.out.push_str("xmlns");
                                self.doc.defaults += 1;
                            }
                        }
                        self.out.push_str("=\"");
                        self.out.push_str(URIS[uri]);
                        self.out.push('"');
                    }
                }
                Some(i) => {
                    let attribute = &el.attributes[i];
                    let name = qualified(attribute.name, attribute.prefix);
                    if attribute.prefix.is_some() {
                        self.doc.prefixed += 1;
                    }
                    let quote = if self.c.flag() { '"' } else { '\'' };
                    let mut spelled = String::new();
                    let mut read = String::new();
                    xml_pieces(
                        &attribute.value,
                        Some(quote),
                        &mut spelled,
                        &mut read,
                        &mut self.doc,
                    );
                    self.out.push_str(&name);
                    if self.c.below(4) == 3 {
                        self.out.push_str(" = ");
                    } else {
                        self.out.push('=');
                    }
                    self.out.push(quote);
                    self.out.push_str(&spelled);
                    self.out.push(quote);
                    attributes.push((name, read));
                }
            }
        }
        if defect_here {
            match self.doc.defect {
                Some(Defect::DuplicateAttribute) => {
                    // The first attribute again, or a new one twice.
                    let name = attributes
                        .first()
                        .map_or_else(|| "dup".to_string(), |(n, _)| n.clone());
                    if attributes.is_empty() {
                        self.out.push_str(" dup=\"1\"");
                    }
                    self.out.push(' ');
                    self.out.push_str(&name);
                    self.out.push_str("=\"2\"");
                }
                Some(Defect::BareAmpersand) => {
                    self.out.push_str(" bare=\"a & b\"");
                }
                _ => {}
            }
        }
        self.doc.expected.elements.push((qname.clone(), attributes));
        if self.c.below(4) == 3 {
            self.space();
        }
        let closes = defect_here && self.doc.defect == Some(Defect::MismatchedEnd);
        if el.children.is_empty() && !closes && self.c.flag() {
            self.out.push_str("/>");
            return;
        }
        self.out.push('>');
        for child in &el.children {
            match child {
                Node::Element(e) => self.element(e, depth + 1),
                Node::Text(pieces) => {
                    let mut spelled = String::new();
                    let mut read = String::new();
                    xml_pieces(pieces, None, &mut spelled, &mut read, &mut self.doc);
                    self.out.push_str(&spelled);
                    self.doc.expected.text.push_str(&read);
                }
                Node::Cdata(body) => {
                    self.out.push_str("<![CDATA[");
                    self.out.push_str(body);
                    self.out.push_str("]]>");
                    self.doc.expected.text.push_str(body);
                    self.doc.cdata += 1;
                }
                Node::Comment(body) => {
                    self.out.push_str("<!--");
                    self.out.push_str(body);
                    self.out.push_str("-->");
                    self.doc.expected.comments.push(body.clone());
                }
                Node::Instruction(target, value) => {
                    self.instruction(target, value);
                }
            }
        }
        self.out.push_str("</");
        if closes {
            self.out
                .push_str(if el.name == "div" { "span" } else { "div" });
        } else {
            self.out.push_str(&qname);
        }
        if self.c.below(4) == 3 {
            self.space();
        }
        self.out.push('>');
    }

    fn instruction(&mut self, target: &str, value: &str) {
        self.out.push_str("<?");
        self.out.push_str(target);
        if !value.is_empty() {
            self.out.push(' ');
            self.out.push_str(value);
        }
        self.out.push_str("?>");
        self.doc
            .expected
            .instructions
            .push((target.to_string(), value.to_string()));
        self.doc.instructions += 1;
    }

    /// A comment or an instruction outside the root, where the input wants
    /// one.
    fn misc(&mut self) {
        match self.c.below(4) {
            1 => {
                self.out.push_str("<!--outside-->");
                self.doc.expected.comments.push("outside".to_string());
            }
            2 => self.instruction("pi", "outside"),
            _ => {}
        }
    }
}

/// The tree written as an XML document, with a defect or a document type
/// declaration where the input asks for one.
pub fn xml(c: &mut Choices) -> XmlDocument {
    let defect = match c.below(8) {
        4 => Some(Defect::UndeclaredPrefix),
        5 => Some(Defect::MismatchedEnd),
        6 => Some(Defect::DuplicateAttribute),
        7 => Some(Defect::BareAmpersand),
        _ => None,
    };
    let declaration = match c.below(8) {
        5 => Some(Declaration::Bare),
        6 => Some(Declaration::External),
        7 => Some(Declaration::Subset),
        _ => None,
    };
    let tree = tree(c);
    let target = c.below(tree.elements);
    let mut w = XmlWriter {
        c,
        out: String::new(),
        doc: XmlDocument {
            bytes: Vec::new(),
            expected: Expected::default(),
            defect,
            declaration,
            prefixed: 0,
            defaults: 0,
            cdata: 0,
            references: 0,
            instructions: 0,
            depth: 0,
            line_ends: 0,
        },
        target,
        seen: 0,
    };
    match w.c.below(3) {
        1 => w.out.push_str("<?xml version=\"1.0\"?>"),
        2 => w
            .out
            .push_str("<?xml version='1.0' encoding='UTF-8' standalone='yes'?>"),
        _ => {}
    }
    w.misc();
    let root = qualified(tree.root.name, tree.root.prefix);
    match declaration {
        Some(Declaration::Bare) => w.out.push_str(&format!("<!DOCTYPE {root}>")),
        Some(Declaration::External) => w.out.push_str(&format!(
            "<!DOCTYPE {root} PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\">"
        )),
        Some(Declaration::Subset) => w.out.push_str(&format!(
            "<!DOCTYPE {root} [<!ENTITY e \"&#38;e;&#38;e;\">]>"
        )),
        None => {}
    }
    w.misc();
    w.element(&tree.root, 0);
    w.misc();
    let mut doc = w.doc;
    doc.bytes = w.out.into_bytes();
    doc
}

// --- HTML ----------------------------------------------------------------

/// A written HTML document and what the generator knows of it.
pub struct HtmlDocument {
    pub bytes: Vec<u8>,
    /// The four bounds the document is parsed under.
    pub limits: tinker_pdf_xml::Limits,
    /// End tags left out, stray end tags, and end tags of formatting
    /// elements written out of order.
    pub omitted: usize,
    pub stray: usize,
    pub misnested: usize,
    /// Text or an element written straight inside a table, which foster
    /// parenting moves out of it.
    pub fostered: usize,
    /// Elements by the mode they open: tables and their parts, templates,
    /// selects, foreign content and raw text.
    pub tables: usize,
    pub templates: usize,
    pub selects: usize,
    pub foreign: usize,
    pub raw: usize,
    /// Which declaration opens the document: none, standards mode, quirks.
    pub doctype: u8,
}

/// HTML's escaping of character data, sometimes left raw as soup is, and
/// references with and without their semicolons.
fn html_text(pieces: &[Piece], c: &mut Choices, out: &mut String) {
    for piece in pieces {
        match piece {
            Piece::Literal(text) => out.push_str(text),
            Piece::CrLf => out.push_str("\r\n"),
            Piece::Entity(i) => {
                let (name, _) = ENTITIES[*i];
                match c.below(4) {
                    0 => out.push_str(&format!("&{name}")),
                    1 => out.push_str(*c.pick(&["&nbsp", "&notin;", "&not", "&amp;", "& ", "<"])),
                    _ => out.push_str(&format!("&{name};")),
                }
            }
            Piece::Char(ch, hex) => match c.below(5) {
                0 => out.push_str(*c.pick(&["&#0;", "&#x110000;", "&#128;", "&#xD800;", "&#"])),
                _ if *hex => out.push_str(&format!("&#x{:x};", u32::from(*ch))),
                _ => out.push_str(&format!("&#{}", u32::from(*ch))),
            },
        }
    }
}

struct HtmlWriter<'c, 'a> {
    c: &'c mut Choices<'a>,
    out: String,
    doc: HtmlDocument,
}

impl HtmlWriter<'_, '_> {
    fn attributes(&mut self, el: &Element) {
        for attribute in &el.attributes {
            self.out.push(' ');
            self.out.push_str(attribute.name);
            let mut value = String::new();
            html_text(&attribute.value, self.c, &mut value);
            match self.c.below(5) {
                0 => {}
                1 if !value.contains(|ch: char| ch.is_whitespace() || "\"'=<>`".contains(ch)) => {
                    self.out.push('=');
                    self.out.push_str(&value);
                }
                2 => {
                    self.out.push_str("='");
                    self.out.push_str(&value.replace('\'', "&#39;"));
                    self.out.push('\'');
                }
                _ => {
                    self.out.push_str("=\"");
                    self.out.push_str(&value.replace('"', "&quot;"));
                    self.out.push('"');
                }
            }
            if self.c.below(8) == 7 {
                // A duplicate, which the tokenizer drops.
                self.out.push(' ');
                self.out.push_str(attribute.name);
                self.out.push_str("=dup");
            }
        }
    }

    fn stray(&mut self) {
        if self.c.below(8) == 7 {
            let name = *self.c.pick(&[
                "p", "br", "td", "tr", "table", "body", "html", "div", "b", "a", "select",
                "template", "li", "form", "svg", "option", "head", "caption",
            ]);
            self.out.push_str(&format!("</{name}>"));
            self.doc.stray += 1;
        }
    }

    /// One element; `then` is a formatting ancestor's end tag to write
    /// before this element's own, which mis-nests the two.
    fn element(&mut self, el: &Element, parent: &str, then: Option<&str>) {
        let name = el.name;
        match name {
            "table" | "tbody" | "thead" | "tfoot" | "tr" | "td" | "th" | "caption" | "colgroup"
            | "col" => self.doc.tables += 1,
            "template" => self.doc.templates += 1,
            "select" | "option" | "optgroup" => self.doc.selects += 1,
            "svg" | "math" => self.doc.foreign += 1,
            _ => {}
        }
        if matches!(parent, "table" | "tbody" | "thead" | "tfoot" | "tr")
            && !matches!(
                name,
                "tbody"
                    | "thead"
                    | "tfoot"
                    | "tr"
                    | "td"
                    | "th"
                    | "caption"
                    | "colgroup"
                    | "col"
                    | "template"
                    | "script"
                    | "style"
            )
        {
            self.doc.fostered += 1;
        }
        self.out.push('<');
        self.out.push_str(name);
        self.attributes(el);
        if el.children.is_empty() && self.c.below(4) == 3 {
            self.out.push_str("/>");
            if let Some(outer) = then {
                self.out.push_str(&format!("</{outer}>"));
            }
            return;
        }
        self.out.push('>');
        let raw = RAW.contains(&name);
        if raw {
            self.doc.raw += 1;
        }
        let count = el.children.len();
        let mut handed = false;
        for (k, child) in el.children.iter().enumerate() {
            self.stray();
            match child {
                Node::Element(e) if raw => {
                    // Markup inside raw text is text to HTML.
                    self.out.push('<');
                    self.out.push_str(e.name);
                    self.out.push('>');
                }
                Node::Element(e) => {
                    // A formatting element inside another, last: close the
                    // outer one first, so the adoption agency reorders them.
                    let swap = k + 1 == count
                        && FORMATTING.contains(&name)
                        && FORMATTING.contains(&e.name)
                        && self.c.below(3) == 2;
                    if swap {
                        self.doc.misnested += 1;
                        handed = true;
                        self.element(e, name, Some(name));
                    } else {
                        self.element(e, name, None);
                    }
                }
                Node::Text(pieces) => {
                    if matches!(name, "table" | "tbody" | "thead" | "tfoot" | "tr") {
                        self.doc.fostered += 1;
                    }
                    html_text(pieces, self.c, &mut self.out);
                }
                Node::Cdata(body) => {
                    self.out.push_str("<![CDATA[");
                    self.out.push_str(body);
                    self.out.push_str("]]>");
                }
                Node::Comment(body) => {
                    self.out.push_str("<!--");
                    self.out.push_str(body);
                    self.out.push_str("-->");
                }
                Node::Instruction(target, value) => {
                    self.out.push_str(&format!("<?{target} {value}>"));
                }
            }
        }
        self.stray();
        if let Some(outer) = then {
            self.out.push_str(&format!("</{outer}>"));
        }
        if handed {
            // The end tag went before the last child's.
            return;
        }
        if self.c.below(5) == 4 {
            self.doc.omitted += 1;
            return;
        }
        self.out.push_str(&format!("</{name}>"));
    }
}

/// The tree written as HTML tag soup, under bounds the input chooses.
pub fn html(c: &mut Choices) -> HtmlDocument {
    let limits = tinker_pdf_xml::Limits {
        max_depth: *c.pick(&[256, 8, 32, 4]),
        max_attributes: *c.pick(&[256, 2, 16, 0]),
        max_name_len: *c.pick(&[1024, 4, 64, 1]),
        max_tokens: *c.pick(&[1 << 20, 64, 512, 16]),
    };
    let doctype = c.below(3) as u8;
    let tree = tree(c);
    let mut w = HtmlWriter {
        c,
        out: String::new(),
        doc: HtmlDocument {
            bytes: Vec::new(),
            limits,
            omitted: 0,
            stray: 0,
            misnested: 0,
            fostered: 0,
            tables: 0,
            templates: 0,
            selects: 0,
            foreign: 0,
            raw: 0,
            doctype,
        },
    };
    match doctype {
        1 => w.out.push_str("<!DOCTYPE html>"),
        2 => w
            .out
            .push_str("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\">"),
        _ => {}
    }
    w.element(&tree.root, "body", None);
    let mut doc = w.doc;
    doc.bytes = w.out.into_bytes();
    doc
}

#[cfg(test)]
mod tests;
