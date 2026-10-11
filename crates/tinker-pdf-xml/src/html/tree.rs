//! HTML §13.2.6, the tree construction stage.
//!
//! One method per insertion mode, named as the standard names it, each a
//! transcription of that mode's list in order: a reader holding §13.2.6.4
//! beside this file should find every entry here, in the same place. What is
//! not here is what a parser with scripting disabled and no browsing context
//! never does — running a script, attaching a shadow root, patching a
//! template's content in place — and the module comment of `html` says so.
//!
//! The tree is an arena of [`Node`]s addressed by index, and the stack of open
//! elements and the list of active formatting elements are lists of indices
//! into it, so the adoption agency can move a node by editing two `Vec`s and
//! nothing is ever reference-counted.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::tokenizer::{DoctypeToken, State, Tag, Token, Tokenizer};
use super::{Attribute, AttributeNamespace, Document, Element, Namespace, Node, NodeData, Quirks};
use crate::encoding::{self, Label};
use crate::limits::{MAX_HTML_ACTIVE_FORMATTING, MAX_HTML_CLONE_BYTES};
use crate::{Error, Limits};

/// §13.2.4.1's insertion modes, by the names the standard gives them. "In
/// select" and "in select in table" are gone from the standard since its
/// customizable-`<select>` change, and from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

/// An entry of the list of active formatting elements.
#[derive(Clone, Debug)]
enum Entry {
    Marker,
    /// The element, and the token it was made for — which the standard keeps
    /// so that a clone can be made of it later. Shared, so that taking an
    /// entry out of the list to clone from is a count and not a copy of every
    /// attribute it carries.
    Element(usize, Rc<Formatting>),
}

/// A formatting element's token, as the list of active formatting elements
/// keeps it.
#[derive(Debug)]
struct Formatting {
    tag: Tag,
    /// The attributes' indices in name order. A tag's names are distinct —
    /// the tokenizer keeps the first of two — so this order is canonical, and
    /// Noah's Ark compares two lists in one pass rather than looking every
    /// name of one up in the other.
    order: Vec<usize>,
    /// A hash of the name and of the attributes in that order: two entries
    /// whose keys differ are not the same, and only two whose keys agree are
    /// compared attribute by attribute.
    key: u64,
}

impl Formatting {
    fn new(tag: Tag) -> Self {
        let mut order: Vec<usize> = (0..tag.attributes.len()).collect();
        order.sort_by(|&a, &b| {
            let name = |at: usize| tag.attributes.get(at).map(|(name, _)| name);
            name(a).cmp(&name(b))
        });
        // SipHash with its fixed keys: the same input hashes the same on every
        // run and every target, and nothing the parser builds depends on the
        // value beyond the equality it guards.
        let mut hasher = DefaultHasher::new();
        tag.name.hash(&mut hasher);
        for &at in &order {
            if let Some((name, value)) = tag.attributes.get(at) {
                name.hash(&mut hasher);
                value.hash(&mut hasher);
            }
        }
        Formatting {
            key: hasher.finish(),
            order,
            tag,
        }
    }

    /// Noah's Ark's comparison: the same tag name, and the same attributes
    /// with the same values, in any order — linear in the attributes.
    fn same_as(&self, other: &Formatting) -> bool {
        self.key == other.key
            && self.tag.name == other.tag.name
            && self.order.len() == other.order.len()
            && self.order.iter().zip(&other.order).all(|(&a, &b)| {
                super::step();
                self.tag.attributes.get(a) == other.tag.attributes.get(b)
            })
    }
}

/// What handling a token in a mode came to.
enum Step {
    Done,
    /// The token again, through the dispatcher, in whatever mode is now set.
    Reprocess(Token),
}

const SPACE: [char; 5] = ['\t', '\n', '\x0C', '\r', ' '];

fn is_space_text(text: &str) -> bool {
    text.chars().all(|c| SPACE.contains(&c))
}

fn start_named(token: &Token, names: &[&str]) -> bool {
    matches!(token, Token::StartTag(tag) if names.contains(&tag.name.as_str()))
}

/// The elements §13.2.4.2 calls *special*, in the HTML namespace.
const SPECIAL_HTML: [&str; 83] = [
    "address",
    "applet",
    "area",
    "article",
    "aside",
    "base",
    "basefont",
    "bgsound",
    "blockquote",
    "body",
    "br",
    "button",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "iframe",
    "img",
    "input",
    "keygen",
    "li",
    "link",
    "listing",
    "main",
    "marquee",
    "menu",
    "meta",
    "nav",
    "noembed",
    "noframes",
    "noscript",
    "object",
    "ol",
    "p",
    "param",
    "plaintext",
    "pre",
    "script",
    "search",
    "section",
    "select",
    "source",
    "style",
    "summary",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
    "wbr",
    "xmp",
];

const IMPLIED_END: [&str; 10] = [
    "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc",
];

const IMPLIED_END_THOROUGH: [&str; 18] = [
    "caption", "colgroup", "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc",
    "tbody", "td", "tfoot", "th", "thead", "tr",
];

/// The elements an end of `<body>` or of the file may leave open without a
/// parse error.
const MAY_STAY_OPEN: [&str; 18] = [
    "dd", "dt", "li", "optgroup", "option", "p", "rb", "rp", "rt", "rtc", "tbody", "td", "tfoot",
    "th", "thead", "tr", "body", "html",
];

/// §13.2.6.5's start tags that break out of foreign content.
const BREAKS_FOREIGN: [&str; 44] = [
    "b",
    "big",
    "blockquote",
    "body",
    "br",
    "center",
    "code",
    "dd",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "hr",
    "i",
    "img",
    "li",
    "listing",
    "menu",
    "meta",
    "nobr",
    "ol",
    "p",
    "pre",
    "ruby",
    "s",
    "small",
    "span",
    "strong",
    "strike",
    "sub",
    "sup",
    "table",
    "tt",
    "u",
    "ul",
    "var",
];

/// §13.2.6.5's table of SVG element names that are not all lowercase.
const SVG_TAG_NAMES: [(&str, &str); 37] = [
    ("altglyph", "altGlyph"),
    ("altglyphdef", "altGlyphDef"),
    ("altglyphitem", "altGlyphItem"),
    ("animatecolor", "animateColor"),
    ("animatemotion", "animateMotion"),
    ("animatetransform", "animateTransform"),
    ("clippath", "clipPath"),
    ("feblend", "feBlend"),
    ("fecolormatrix", "feColorMatrix"),
    ("fecomponenttransfer", "feComponentTransfer"),
    ("fecomposite", "feComposite"),
    ("feconvolvematrix", "feConvolveMatrix"),
    ("fediffuselighting", "feDiffuseLighting"),
    ("fedisplacementmap", "feDisplacementMap"),
    ("fedistantlight", "feDistantLight"),
    ("fedropshadow", "feDropShadow"),
    ("feflood", "feFlood"),
    ("fefunca", "feFuncA"),
    ("fefuncb", "feFuncB"),
    ("fefuncg", "feFuncG"),
    ("fefuncr", "feFuncR"),
    ("fegaussianblur", "feGaussianBlur"),
    ("feimage", "feImage"),
    ("femerge", "feMerge"),
    ("femergenode", "feMergeNode"),
    ("femorphology", "feMorphology"),
    ("feoffset", "feOffset"),
    ("fepointlight", "fePointLight"),
    ("fespecularlighting", "feSpecularLighting"),
    ("fespotlight", "feSpotLight"),
    ("fetile", "feTile"),
    ("feturbulence", "feTurbulence"),
    ("foreignobject", "foreignObject"),
    ("glyphref", "glyphRef"),
    ("lineargradient", "linearGradient"),
    ("radialgradient", "radialGradient"),
    ("textpath", "textPath"),
];

/// §13.2.6.1's *adjust SVG attributes* table.
const SVG_ATTRIBUTES: [(&str, &str); 58] = [
    ("attributename", "attributeName"),
    ("attributetype", "attributeType"),
    ("basefrequency", "baseFrequency"),
    ("baseprofile", "baseProfile"),
    ("calcmode", "calcMode"),
    ("clippathunits", "clipPathUnits"),
    ("diffuseconstant", "diffuseConstant"),
    ("edgemode", "edgeMode"),
    ("filterunits", "filterUnits"),
    ("glyphref", "glyphRef"),
    ("gradienttransform", "gradientTransform"),
    ("gradientunits", "gradientUnits"),
    ("kernelmatrix", "kernelMatrix"),
    ("kernelunitlength", "kernelUnitLength"),
    ("keypoints", "keyPoints"),
    ("keysplines", "keySplines"),
    ("keytimes", "keyTimes"),
    ("lengthadjust", "lengthAdjust"),
    ("limitingconeangle", "limitingConeAngle"),
    ("markerheight", "markerHeight"),
    ("markerunits", "markerUnits"),
    ("markerwidth", "markerWidth"),
    ("maskcontentunits", "maskContentUnits"),
    ("maskunits", "maskUnits"),
    ("numoctaves", "numOctaves"),
    ("pathlength", "pathLength"),
    ("patterncontentunits", "patternContentUnits"),
    ("patterntransform", "patternTransform"),
    ("patternunits", "patternUnits"),
    ("pointsatx", "pointsAtX"),
    ("pointsaty", "pointsAtY"),
    ("pointsatz", "pointsAtZ"),
    ("preservealpha", "preserveAlpha"),
    ("preserveaspectratio", "preserveAspectRatio"),
    ("primitiveunits", "primitiveUnits"),
    ("refx", "refX"),
    ("refy", "refY"),
    ("repeatcount", "repeatCount"),
    ("repeatdur", "repeatDur"),
    ("requiredextensions", "requiredExtensions"),
    ("requiredfeatures", "requiredFeatures"),
    ("specularconstant", "specularConstant"),
    ("specularexponent", "specularExponent"),
    ("spreadmethod", "spreadMethod"),
    ("startoffset", "startOffset"),
    ("stddeviation", "stdDeviation"),
    ("stitchtiles", "stitchTiles"),
    ("surfacescale", "surfaceScale"),
    ("systemlanguage", "systemLanguage"),
    ("tablevalues", "tableValues"),
    ("targetx", "targetX"),
    ("targety", "targetY"),
    ("textlength", "textLength"),
    ("viewbox", "viewBox"),
    ("viewtarget", "viewTarget"),
    ("xchannelselector", "xChannelSelector"),
    ("ychannelselector", "yChannelSelector"),
    ("zoomandpan", "zoomAndPan"),
];

/// The DOCTYPE public identifiers that set quirks mode by prefix.
const QUIRKS_PREFIXES: [&str; 55] = [
    "+//silmaril//dtd html pro v0r11 19970101//",
    "-//as//dtd html 3.0 aswedit + extensions//",
    "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
    "-//ietf//dtd html 2.0 level 1//",
    "-//ietf//dtd html 2.0 level 2//",
    "-//ietf//dtd html 2.0 strict level 1//",
    "-//ietf//dtd html 2.0 strict level 2//",
    "-//ietf//dtd html 2.0 strict//",
    "-//ietf//dtd html 2.0//",
    "-//ietf//dtd html 2.1e//",
    "-//ietf//dtd html 3.0//",
    "-//ietf//dtd html 3.2 final//",
    "-//ietf//dtd html 3.2//",
    "-//ietf//dtd html 3//",
    "-//ietf//dtd html level 0//",
    "-//ietf//dtd html level 1//",
    "-//ietf//dtd html level 2//",
    "-//ietf//dtd html level 3//",
    "-//ietf//dtd html strict level 0//",
    "-//ietf//dtd html strict level 1//",
    "-//ietf//dtd html strict level 2//",
    "-//ietf//dtd html strict level 3//",
    "-//ietf//dtd html strict//",
    "-//ietf//dtd html//",
    "-//metrius//dtd metrius presentational//",
    "-//microsoft//dtd internet explorer 2.0 html strict//",
    "-//microsoft//dtd internet explorer 2.0 html//",
    "-//microsoft//dtd internet explorer 2.0 tables//",
    "-//microsoft//dtd internet explorer 3.0 html strict//",
    "-//microsoft//dtd internet explorer 3.0 html//",
    "-//microsoft//dtd internet explorer 3.0 tables//",
    "-//netscape comm. corp.//dtd html//",
    "-//netscape comm. corp.//dtd strict html//",
    "-//o'reilly and associates//dtd html 2.0//",
    "-//o'reilly and associates//dtd html extended 1.0//",
    "-//o'reilly and associates//dtd html extended relaxed 1.0//",
    "-//sq//dtd html 2.0 hotmetal + extensions//",
    "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
    "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
    "-//spyglass//dtd html 2.0 extended//",
    "-//sun microsystems corp.//dtd hotjava html//",
    "-//sun microsystems corp.//dtd hotjava strict html//",
    "-//w3c//dtd html 3 1995-03-24//",
    "-//w3c//dtd html 3.2 draft//",
    "-//w3c//dtd html 3.2 final//",
    "-//w3c//dtd html 3.2//",
    "-//w3c//dtd html 3.2s draft//",
    "-//w3c//dtd html 4.0 frameset//",
    "-//w3c//dtd html 4.0 transitional//",
    "-//w3c//dtd html experimental 19960712//",
    "-//w3c//dtd html experimental 970421//",
    "-//w3c//dtd w3 html//",
    "-//w3o//dtd w3 html 3.0//",
    "-//webtechs//dtd mozilla html 2.0//",
    "-//webtechs//dtd mozilla html//",
];

pub(crate) struct TreeBuilder<'a> {
    tokenizer: Tokenizer<'a>,
    nodes: Vec<Node>,
    open: Vec<usize>,
    active: Vec<Entry>,
    head: Option<usize>,
    form: Option<usize>,
    mode: Mode,
    original_mode: Mode,
    template_modes: Vec<Mode>,
    frameset_ok: bool,
    foster: bool,
    quirks: Quirks,
    pending_table_text: Vec<String>,
    context: Option<usize>,
    ignore_lf: bool,
    errors: usize,
    spent: usize,
    /// Bytes of attribute names and values copied onto clones, against
    /// [`MAX_HTML_CLONE_BYTES`].
    cloned: usize,
    limits: Limits,
    stopped: Option<Error>,
    halt: bool,
    /// The encoding the first `<meta>` that names one names (§13.2.6.4.4).
    meta_encoding: Option<Label>,
    /// The names of the attributes of each element [`TreeBuilder::merge_attributes`]
    /// has merged into — the root and the `<body>` — kept from its first
    /// merge on, so that a merge costs its own tag's attributes.
    merged: Vec<(usize, BTreeSet<String>)>,
}

/// The encoding a `<meta>` names, as §13.2.6.4.4 reads it: a `charset` that
/// is an encoding's label, or else an `http-equiv` of `Content-Type` and a
/// `content` holding `charset=`.
fn meta_encoding(tag: &Tag) -> Option<Label> {
    if let Some(found) = tag.attribute("charset").and_then(encoding::lookup) {
        return Some(found);
    }
    let pragma = tag
        .attribute("http-equiv")
        .is_some_and(|v| v.eq_ignore_ascii_case("content-type"));
    if !pragma {
        return None;
    }
    super::charset_from_content(tag.attribute("content")?.as_bytes())
}

pub(crate) fn parse_document(text: &str, limits: &Limits) -> Document {
    let mut builder = TreeBuilder::new(text, limits);
    builder.run();
    builder.finish(0)
}

pub(crate) fn parse_fragment(text: &str, context: (Namespace, &str), limits: &Limits) -> Document {
    let mut builder = TreeBuilder::new(text, limits);
    let (namespace, name) = context;
    let context_tag = Tag {
        name: name.to_owned(),
        ..Tag::default()
    };
    let context_id = builder.create_element(&context_tag, namespace);
    builder.context = Some(context_id);
    let root = builder.create_element(
        &Tag {
            name: "html".to_owned(),
            ..Tag::default()
        },
        Namespace::Html,
    );
    builder.append(0, root);
    builder.open.push(root);
    if namespace == Namespace::Html {
        if name == "template" {
            builder.template_modes.push(Mode::InTemplate);
        }
        builder.tokenizer.state = match name {
            "title" | "textarea" => State::Rcdata,
            "style" | "xmp" | "iframe" | "noembed" | "noframes" => State::Rawtext,
            "script" => State::ScriptData,
            "plaintext" => State::Plaintext,
            _ => State::Data,
        };
        // No start tag has been emitted by this tokenizer, so no end tag is
        // appropriate: `</script>` inside a `script` context is text.
        if name == "form" {
            builder.form = Some(context_id);
        }
    }
    builder.reset_insertion_mode();
    builder.run();
    builder.finish(root)
}

impl<'a> TreeBuilder<'a> {
    fn new(text: &'a str, limits: &Limits) -> Self {
        TreeBuilder {
            tokenizer: Tokenizer::new(text, limits),
            nodes: vec![Node {
                parent: None,
                children: Vec::new(),
                data: NodeData::Document,
            }],
            open: Vec::new(),
            active: Vec::new(),
            head: None,
            form: None,
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            template_modes: Vec::new(),
            frameset_ok: true,
            foster: false,
            quirks: Quirks::NoQuirks,
            pending_table_text: Vec::new(),
            context: None,
            ignore_lf: false,
            errors: 0,
            spent: 0,
            cloned: 0,
            limits: *limits,
            stopped: None,
            halt: false,
            meta_encoding: None,
            merged: Vec::new(),
        }
    }

    fn finish(self, root: usize) -> Document {
        Document {
            nodes: self.nodes,
            root,
            quirks: self.quirks,
            errors: self.errors.saturating_add(self.tokenizer.errors),
            stopped: self.stopped.or(self.tokenizer.stop),
            decoding: None,
            meta_encoding: self.meta_encoding,
        }
    }

    fn run(&mut self) {
        loop {
            self.tokenizer.allow_cdata = self
                .adjusted_current()
                .is_some_and(|node| self.namespace(node) != Some(Namespace::Html));
            let token = self.tokenizer.next_token();
            let eof = token == Token::Eof;
            if !eof {
                self.spend();
            }
            if self.halt || self.tokenizer.stop.is_some() {
                return;
            }
            self.process(token);
            if eof || self.halt {
                return;
            }
        }
    }

    fn stop(&mut self, error: Error) {
        if self.stopped.is_none() {
            self.stopped = Some(error);
        }
        self.halt = true;
    }

    /// One unit of [`Limits::max_tokens`], spent by every token and every
    /// node created.
    fn spend(&mut self) {
        self.spend_many(1);
    }

    fn spend_many(&mut self, units: usize) {
        self.spent = self.spent.saturating_add(units);
        if self.spent > self.limits.max_tokens {
            self.stop(Error::TokenCap);
        }
    }

    /// What a clone of a formatting element costs, charged before it is made:
    /// one unit of [`Limits::max_tokens`] for every attribute it copies — the
    /// node itself is spent as every node is — and the attributes' bytes from
    /// [`MAX_HTML_CLONE_BYTES`]. A clone is the one place the tree builder
    /// copies what the input said once, as often as the input reopens it, so
    /// these two are what keep the tree's size a function of the input's.
    /// `false` when the clone would cross either, which stops the parse
    /// without making it.
    fn charge_clone(&mut self, tag: &Tag) -> bool {
        let bytes = tag.attributes.iter().fold(0usize, |sum, (name, value)| {
            sum.saturating_add(name.len()).saturating_add(value.len())
        });
        let cloned = self.cloned.saturating_add(bytes);
        if cloned > MAX_HTML_CLONE_BYTES {
            self.stop(Error::CloneCap);
            return false;
        }
        self.cloned = cloned;
        self.spend_many(tag.attributes.len());
        !self.halt
    }

    fn error(&mut self) {
        self.errors = self.errors.saturating_add(1);
    }

    // ---- The tree -------------------------------------------------------

    fn element(&self, node: usize) -> Option<&Element> {
        self.nodes.get(node).and_then(Node::element)
    }

    fn namespace(&self, node: usize) -> Option<Namespace> {
        self.element(node).map(|e| e.namespace)
    }

    fn is_html(&self, node: usize, name: &str) -> bool {
        self.element(node)
            .is_some_and(|e| e.namespace == Namespace::Html && e.name == name)
    }

    fn is_html_one_of(&self, node: usize, names: &[&str]) -> bool {
        self.element(node)
            .is_some_and(|e| e.namespace == Namespace::Html && names.contains(&e.name.as_str()))
    }

    fn is_in(&self, node: usize, namespace: Namespace, names: &[&str]) -> bool {
        self.element(node)
            .is_some_and(|e| e.namespace == namespace && names.contains(&e.name.as_str()))
    }

    fn is_special(&self, node: usize) -> bool {
        self.is_html_one_of(node, &SPECIAL_HTML)
            || self.is_in(
                node,
                Namespace::MathMl,
                &["mi", "mo", "mn", "ms", "mtext", "annotation-xml"],
            )
            || self.is_in(node, Namespace::Svg, &["foreignObject", "desc", "title"])
    }

    fn current(&self) -> Option<usize> {
        self.open.last().copied()
    }

    fn adjusted_current(&self) -> Option<usize> {
        match self.context {
            Some(context) if self.open.len() == 1 => Some(context),
            _ => self.current(),
        }
    }

    fn current_is(&self, name: &str) -> bool {
        self.current().is_some_and(|node| self.is_html(node, name))
    }

    fn current_is_one_of(&self, names: &[&str]) -> bool {
        self.current()
            .is_some_and(|node| self.is_html_one_of(node, names))
    }

    fn new_node(&mut self, data: NodeData) -> usize {
        self.spend();
        self.nodes.push(Node {
            parent: None,
            children: Vec::new(),
            data,
        });
        self.nodes.len() - 1
    }

    /// §13.2.6.1's *create an element for a token*, unattached.
    fn create_element(&mut self, tag: &Tag, namespace: Namespace) -> usize {
        let integration_point = namespace == Namespace::MathMl
            && tag.name == "annotation-xml"
            && tag.attribute("encoding").is_some_and(|encoding| {
                encoding.eq_ignore_ascii_case("text/html")
                    || encoding.eq_ignore_ascii_case("application/xhtml+xml")
            });
        let attributes = tag
            .attributes
            .iter()
            .map(|(name, value)| attribute(namespace, name, value))
            .collect();
        let template = namespace == Namespace::Html && tag.name == "template";
        let element = self.new_node(NodeData::Element(Element {
            name: tag.name.clone(),
            namespace,
            attributes,
            template_contents: None,
            integration_point,
        }));
        if template {
            let contents = self.new_node(NodeData::Fragment);
            if let Some(Node {
                data: NodeData::Element(e),
                ..
            }) = self.nodes.get_mut(element)
            {
                e.template_contents = Some(contents);
            }
        }
        element
    }

    fn detach(&mut self, node: usize) {
        let Some(parent) = self.nodes.get(node).and_then(|n| n.parent) else {
            return;
        };
        // Searched from the end: what the tree builder moves is almost always a
        // parent's last child, and a search from the front made a misnested
        // tag after a hundred thousand siblings a hundred thousand steps.
        if let Some(p) = self.nodes.get_mut(parent) {
            if let Some(at) = p.children.iter().rposition(|&c| {
                super::step();
                c == node
            }) {
                p.children.remove(at);
            }
        }
        if let Some(n) = self.nodes.get_mut(node) {
            n.parent = None;
        }
    }

    fn insert_before(&mut self, parent: usize, node: usize, before: Option<usize>) {
        self.detach(node);
        let Some(p) = self.nodes.get_mut(parent) else {
            return;
        };
        // From the end for the same reason: a foster parent's reference child
        // is the open table, which is its parent's last child.
        let at = before
            .and_then(|b| {
                p.children.iter().rposition(|&c| {
                    super::step();
                    c == b
                })
            })
            .unwrap_or(p.children.len());
        p.children.insert(at, node);
        if let Some(n) = self.nodes.get_mut(node) {
            n.parent = Some(parent);
        }
    }

    fn append(&mut self, parent: usize, node: usize) {
        self.insert_before(parent, node, None);
    }

    /// §13.2.6.1's *appropriate place for inserting a node*.
    fn appropriate_place(&self, target: Option<usize>) -> (usize, Option<usize>) {
        let mut parent = match target.or_else(|| self.current()) {
            Some(parent) => parent,
            None => return (0, None),
        };
        let mut before = None;
        if self.foster && self.is_html_one_of(parent, &["table", "tbody", "tfoot", "thead", "tr"]) {
            let last_template = self.open.iter().rposition(|&n| self.is_html(n, "template"));
            let last_table = self.open.iter().rposition(|&n| self.is_html(n, "table"));
            match (last_template, last_table) {
                (Some(t), table) if table.is_none_or(|table| t > table) => {
                    parent = self.open.get(t).copied().unwrap_or(parent);
                }
                (_, None) => {
                    return (self.open.first().copied().unwrap_or(0), None);
                }
                (_, Some(table_at)) => {
                    let table = self.open.get(table_at).copied().unwrap_or(parent);
                    match self.nodes.get(table).and_then(|n| n.parent) {
                        Some(table_parent) => {
                            parent = table_parent;
                            before = Some(table);
                        }
                        None => {
                            parent = table_at
                                .checked_sub(1)
                                .and_then(|above| self.open.get(above))
                                .copied()
                                .unwrap_or(parent);
                        }
                    }
                }
            }
        }
        if let Some(contents) = self.element(parent).and_then(|e| {
            if e.namespace == Namespace::Html && e.name == "template" {
                e.template_contents
            } else {
                None
            }
        }) {
            return (contents, None);
        }
        (parent, before)
    }

    fn push_open(&mut self, node: usize) {
        self.open.push(node);
        if self.open.len() > self.limits.max_depth {
            self.stop(Error::DepthCap);
        }
    }

    /// §13.2.6.1's *insert a foreign element*.
    fn insert_element(&mut self, tag: &Tag, namespace: Namespace) -> usize {
        let (parent, before) = self.appropriate_place(None);
        let element = self.create_element(tag, namespace);
        if !matches!(
            self.nodes.get(parent).map(|n| &n.data),
            Some(NodeData::Document)
        ) || !self.has_element_child(parent)
        {
            self.insert_before(parent, element, before);
        }
        self.push_open(element);
        element
    }

    fn has_element_child(&self, parent: usize) -> bool {
        self.nodes.get(parent).is_some_and(|p| {
            p.children
                .iter()
                .any(|&c| self.nodes.get(c).is_some_and(|n| n.element().is_some()))
        })
    }

    fn insert_html(&mut self, tag: &Tag) -> usize {
        self.insert_element(tag, Namespace::Html)
    }

    fn insert_html_named(&mut self, name: &str) -> usize {
        self.insert_html(&Tag {
            name: name.to_owned(),
            ..Tag::default()
        })
    }

    /// §13.2.6.1's *insert a character*, for a run.
    fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let (parent, before) = self.appropriate_place(None);
        if matches!(
            self.nodes.get(parent).map(|n| &n.data),
            Some(NodeData::Document)
        ) {
            return;
        }
        let previous = match before {
            // From the end, as `insert_before` finds it: the reference node is
            // the open table, its parent's last child, and every fostered node
            // goes in front of it.
            Some(b) => self.nodes.get(parent).and_then(|p| {
                let at = p.children.iter().rposition(|&c| {
                    super::step();
                    c == b
                })?;
                at.checked_sub(1).and_then(|i| p.children.get(i)).copied()
            }),
            None => self
                .nodes
                .get(parent)
                .and_then(|p| p.children.last().copied()),
        };
        if let Some(previous) = previous {
            if let Some(Node {
                data: NodeData::Text(existing),
                ..
            }) = self.nodes.get_mut(previous)
            {
                existing.push_str(text);
                return;
            }
        }
        let node = self.new_node(NodeData::Text(text.to_owned()));
        self.insert_before(parent, node, before);
    }

    fn insert_comment(&mut self, text: String, at: Option<(usize, Option<usize>)>) {
        let (parent, before) = at.unwrap_or_else(|| self.appropriate_place(None));
        let node = self.new_node(NodeData::Comment(text));
        self.insert_before(parent, node, before);
    }

    fn pop(&mut self) -> Option<usize> {
        self.open.pop()
    }

    fn pop_until(&mut self, name: &str) {
        while let Some(node) = self.pop() {
            if self.is_html(node, name) {
                return;
            }
        }
    }

    fn pop_until_one_of(&mut self, names: &[&str]) {
        while let Some(node) = self.pop() {
            if self.is_html_one_of(node, names) {
                return;
            }
        }
    }

    fn remove_from_open(&mut self, node: usize) {
        if let Some(at) = self.open.iter().rposition(|&n| n == node) {
            self.open.remove(at);
        }
    }

    // ---- Scopes ---------------------------------------------------------

    fn is_scope_boundary(&self, node: usize) -> bool {
        self.is_html_one_of(
            node,
            &[
                "applet", "caption", "html", "table", "td", "th", "marquee", "object", "select",
                "template",
            ],
        ) || self.is_in(
            node,
            Namespace::MathMl,
            &["mi", "mo", "mn", "ms", "mtext", "annotation-xml"],
        ) || self.is_in(node, Namespace::Svg, &["foreignObject", "desc", "title"])
    }

    fn in_scope_where(
        &self,
        target: impl Fn(&Self, usize) -> bool,
        boundary: impl Fn(&Self, usize) -> bool,
    ) -> bool {
        for &node in self.open.iter().rev() {
            if target(self, node) {
                return true;
            }
            if boundary(self, node) {
                return false;
            }
        }
        false
    }

    fn in_scope(&self, name: &str) -> bool {
        self.in_scope_where(|b, n| b.is_html(n, name), Self::is_scope_boundary)
    }

    fn in_scope_one_of(&self, names: &[&str]) -> bool {
        self.in_scope_where(|b, n| b.is_html_one_of(n, names), Self::is_scope_boundary)
    }

    fn node_in_scope(&self, node: usize) -> bool {
        self.in_scope_where(|_, n| n == node, Self::is_scope_boundary)
    }

    fn in_list_item_scope(&self, name: &str) -> bool {
        self.in_scope_where(
            |b, n| b.is_html(n, name),
            |b, n| b.is_scope_boundary(n) || b.is_html_one_of(n, &["ol", "ul"]),
        )
    }

    fn in_button_scope(&self, name: &str) -> bool {
        self.in_scope_where(
            |b, n| b.is_html(n, name),
            |b, n| b.is_scope_boundary(n) || b.is_html(n, "button"),
        )
    }

    fn in_table_scope(&self, name: &str) -> bool {
        self.in_scope_where(
            |b, n| b.is_html(n, name),
            |b, n| b.is_html_one_of(n, &["html", "table", "template"]),
        )
    }

    fn in_table_scope_one_of(&self, names: &[&str]) -> bool {
        self.in_scope_where(
            |b, n| b.is_html_one_of(n, names),
            |b, n| b.is_html_one_of(n, &["html", "table", "template"]),
        )
    }

    fn generate_implied_end_tags(&mut self, except: Option<&str>) {
        while let Some(current) = self.current() {
            let implied = self.is_html_one_of(current, &IMPLIED_END)
                && except.is_none_or(|except| !self.is_html(current, except));
            if !implied {
                return;
            }
            self.pop();
        }
    }

    fn generate_implied_end_tags_thoroughly(&mut self) {
        while self.current_is_one_of(&IMPLIED_END_THOROUGH) {
            self.pop();
        }
    }

    fn close_p(&mut self) {
        self.generate_implied_end_tags(Some("p"));
        if !self.current_is("p") {
            self.error();
        }
        self.pop_until("p");
    }

    fn close_p_in_button_scope(&mut self) {
        if self.in_button_scope("p") {
            self.close_p();
        }
    }

    fn template_on_stack(&self) -> bool {
        self.open.iter().any(|&n| self.is_html(n, "template"))
    }

    /// §13.2.4.4's *parsing template contents*: a template on the stack, or
    /// one as the fragment's context.
    fn in_template_contents(&self) -> bool {
        self.template_on_stack()
            || self
                .context
                .is_some_and(|context| self.is_html(context, "template"))
    }

    // ---- The list of active formatting elements -------------------------

    fn push_formatting(&mut self, node: usize, tag: Tag) {
        let formatting = Formatting::new(tag);
        // Noah's Ark: of three entries after the last marker that are the same
        // as this one, the earliest goes.
        let mut same = 0;
        let mut earliest = None;
        for (at, entry) in self.active.iter().enumerate().rev() {
            match entry {
                Entry::Marker => break,
                Entry::Element(_, other) => {
                    if other.same_as(&formatting) {
                        same += 1;
                        earliest = Some(at);
                    }
                }
            }
        }
        if same >= 3 {
            if let Some(earliest) = earliest {
                self.active.remove(earliest);
            }
        }
        self.push_active(Entry::Element(node, Rc::new(formatting)));
    }

    /// Appends to the list of active formatting elements, which is held to
    /// [`MAX_HTML_ACTIVE_FORMATTING`] entries, markers included: past it the
    /// parse stops with [`Error::FormattingCap`].
    fn push_active(&mut self, entry: Entry) {
        self.active.push(entry);
        if self.active.len() > MAX_HTML_ACTIVE_FORMATTING {
            self.stop(Error::FormattingCap);
        }
    }

    fn active_position(&self, node: usize) -> Option<usize> {
        self.active
            .iter()
            .position(|e| matches!(e, Entry::Element(n, _) if *n == node))
    }

    fn reconstruct_active_formatting(&mut self) {
        let Some(last) = self.active.len().checked_sub(1) else {
            return;
        };
        let on_stack = |b: &Self, at: usize| match b.active.get(at) {
            Some(Entry::Marker) | None => true,
            Some(Entry::Element(n, _)) => b.open.contains(n),
        };
        if on_stack(self, last) {
            return;
        }
        let mut at = last;
        // Rewind.
        while at > 0 {
            if on_stack(self, at - 1) {
                break;
            }
            at -= 1;
        }
        // Advance and create.
        while at < self.active.len() {
            if self.halt {
                return;
            }
            let Some(Entry::Element(_, formatting)) = self.active.get(at).cloned() else {
                at += 1;
                continue;
            };
            if !self.charge_clone(&formatting.tag) {
                return;
            }
            let element = self.insert_html(&formatting.tag);
            if let Some(slot) = self.active.get_mut(at) {
                *slot = Entry::Element(element, formatting);
            }
            at += 1;
        }
    }

    fn clear_to_last_marker(&mut self) {
        while let Some(entry) = self.active.pop() {
            if matches!(entry, Entry::Marker) {
                return;
            }
        }
    }

    // ---- The insertion mode ---------------------------------------------

    fn reset_insertion_mode(&mut self) {
        let mut index = self.open.len();
        while index > 0 {
            index -= 1;
            let mut node = self.open.get(index).copied().unwrap_or(0);
            let last = index == 0;
            if last {
                if let Some(context) = self.context {
                    node = context;
                }
            }
            let Some(element) = self.element(node) else {
                continue;
            };
            if element.namespace != Namespace::Html {
                if last {
                    self.mode = Mode::InBody;
                    return;
                }
                continue;
            }
            let mode = match element.name.as_str() {
                "td" | "th" if !last => Some(Mode::InCell),
                "tr" => Some(Mode::InRow),
                "tbody" | "thead" | "tfoot" => Some(Mode::InTableBody),
                "caption" => Some(Mode::InCaption),
                "colgroup" => Some(Mode::InColumnGroup),
                "table" => Some(Mode::InTable),
                "template" => Some(self.template_modes.last().copied().unwrap_or(Mode::InBody)),
                "head" if !last => Some(Mode::InHead),
                "body" => Some(Mode::InBody),
                "frameset" => Some(Mode::InFrameset),
                "html" => Some(if self.head.is_none() {
                    Mode::BeforeHead
                } else {
                    Mode::AfterHead
                }),
                _ => None,
            };
            if let Some(mode) = mode {
                self.mode = mode;
                return;
            }
            if last {
                self.mode = Mode::InBody;
                return;
            }
        }
        self.mode = Mode::InBody;
    }

    // ---- The dispatcher -------------------------------------------------

    fn process(&mut self, token: Token) {
        let token = if std::mem::take(&mut self.ignore_lf) {
            match token {
                Token::Characters(text) => match text.strip_prefix('\n') {
                    Some("") => return,
                    Some(rest) => Token::Characters(rest.to_owned()),
                    None => Token::Characters(text),
                },
                other => other,
            }
        } else {
            token
        };
        // A run is split where white space meets anything else, because
        // several modes treat the two differently and the standard decides one
        // character at a time.
        if let Token::Characters(text) = &token {
            let mut runs: Vec<&str> = Vec::new();
            let mut start = 0;
            let mut space = None;
            for (at, c) in text.char_indices() {
                let is = SPACE.contains(&c);
                if space.is_some_and(|s| s != is) {
                    runs.push(text.get(start..at).unwrap_or(""));
                    start = at;
                }
                space = Some(is);
            }
            if runs.is_empty() {
                self.dispatch(token);
                return;
            }
            runs.push(text.get(start..).unwrap_or(""));
            let runs: Vec<String> = runs.into_iter().map(str::to_owned).collect();
            for run in runs {
                self.dispatch(Token::Characters(run));
                if self.halt {
                    return;
                }
            }
            return;
        }
        self.dispatch(token);
    }

    fn dispatch(&mut self, mut token: Token) {
        // Every reprocess goes through the dispatcher again, and a mode
        // switch per pass is what bounds this loop; the cap is a backstop.
        for _ in 0..64 {
            let step = if self.use_html_rules(&token) {
                self.handle(self.mode, token)
            } else {
                self.in_foreign_content(token)
            };
            match step {
                Step::Done => return,
                Step::Reprocess(again) => token = again,
            }
            if self.halt {
                return;
            }
        }
    }

    /// §13.2.6's tree construction dispatcher: whether a token goes to the
    /// insertion mode or to the rules for foreign content.
    fn use_html_rules(&self, token: &Token) -> bool {
        let Some(node) = self.adjusted_current() else {
            return true;
        };
        let Some(element) = self.element(node) else {
            return true;
        };
        if element.namespace == Namespace::Html {
            return true;
        }
        let mathml_text = element.namespace == Namespace::MathMl
            && ["mi", "mo", "mn", "ms", "mtext"].contains(&element.name.as_str());
        if mathml_text {
            match token {
                Token::StartTag(tag) if tag.name != "mglyph" && tag.name != "malignmark" => {
                    return true;
                }
                Token::Characters(_) | Token::Null => return true,
                _ => {}
            }
        }
        if element.namespace == Namespace::MathMl
            && element.name == "annotation-xml"
            && start_named(token, &["svg"])
        {
            return true;
        }
        let html_integration = element.integration_point
            || (element.namespace == Namespace::Svg
                && ["foreignObject", "desc", "title"].contains(&element.name.as_str()));
        if html_integration
            && matches!(
                token,
                Token::StartTag(_) | Token::Characters(_) | Token::Null
            )
        {
            return true;
        }
        matches!(token, Token::Eof)
    }

    fn handle(&mut self, mode: Mode, token: Token) -> Step {
        match mode {
            Mode::Initial => self.initial(token),
            Mode::BeforeHtml => self.before_html(token),
            Mode::BeforeHead => self.before_head(token),
            Mode::InHead => self.in_head(token),
            Mode::InHeadNoscript => self.in_head_noscript(token),
            Mode::AfterHead => self.after_head(token),
            Mode::InBody => self.in_body(token),
            Mode::Text => self.text(token),
            Mode::InTable => self.in_table(token),
            Mode::InTableText => self.in_table_text(token),
            Mode::InCaption => self.in_caption(token),
            Mode::InColumnGroup => self.in_column_group(token),
            Mode::InTableBody => self.in_table_body(token),
            Mode::InRow => self.in_row(token),
            Mode::InCell => self.in_cell(token),
            Mode::InTemplate => self.in_template(token),
            Mode::AfterBody => self.after_body(token),
            Mode::InFrameset => self.in_frameset(token),
            Mode::AfterFrameset => self.after_frameset(token),
            Mode::AfterAfterBody => self.after_after_body(token),
            Mode::AfterAfterFrameset => self.after_after_frameset(token),
        }
    }

    // ---- §13.2.6.4.1 to §13.2.6.4.6 --------------------------------------

    fn initial(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) if is_space_text(&text) => Step::Done,
            Token::Comment(text) => {
                self.insert_comment(text, Some((0, None)));
                Step::Done
            }
            Token::Doctype(doctype) => {
                let name = doctype.name.as_deref().unwrap_or("");
                if name != "html"
                    || doctype.public_id.is_some()
                    || doctype
                        .system_id
                        .as_deref()
                        .is_some_and(|s| s != "about:legacy-compat")
                {
                    self.error();
                }
                self.quirks = quirks_of(&doctype);
                let node = self.new_node(NodeData::Doctype {
                    name: doctype.name.clone().unwrap_or_default(),
                    public_id: doctype.public_id.clone().unwrap_or_default(),
                    system_id: doctype.system_id.clone().unwrap_or_default(),
                });
                self.append(0, node);
                self.mode = Mode::BeforeHtml;
                Step::Done
            }
            other => {
                self.error();
                self.quirks = Quirks::Quirks;
                self.mode = Mode::BeforeHtml;
                Step::Reprocess(other)
            }
        }
    }

    fn before_html(&mut self, token: Token) -> Step {
        match token {
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, Some((0, None)));
                Step::Done
            }
            Token::Characters(text) if is_space_text(&text) => Step::Done,
            Token::StartTag(tag) if tag.name == "html" => {
                let element = self.create_element(&tag, Namespace::Html);
                self.append(0, element);
                self.push_open(element);
                self.mode = Mode::BeforeHead;
                Step::Done
            }
            Token::EndTag(tag) if !["head", "body", "html", "br"].contains(&tag.name.as_str()) => {
                self.error();
                Step::Done
            }
            other => {
                let element = self.create_element(
                    &Tag {
                        name: "html".to_owned(),
                        ..Tag::default()
                    },
                    Namespace::Html,
                );
                self.append(0, element);
                self.push_open(element);
                self.mode = Mode::BeforeHead;
                Step::Reprocess(other)
            }
        }
    }

    fn before_head(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) if is_space_text(&text) => Step::Done,
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::StartTag(tag) if tag.name == "head" => {
                let head = self.insert_html(&tag);
                self.head = Some(head);
                self.mode = Mode::InHead;
                Step::Done
            }
            Token::EndTag(tag) if !["head", "body", "html", "br"].contains(&tag.name.as_str()) => {
                self.error();
                Step::Done
            }
            other => {
                let head = self.insert_html_named("head");
                self.head = Some(head);
                self.mode = Mode::InHead;
                Step::Reprocess(other)
            }
        }
    }

    fn in_head(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) if is_space_text(&text) => {
                self.insert_text(&text);
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::StartTag(tag)
                if ["base", "basefont", "bgsound", "link", "meta"].contains(&tag.name.as_str()) =>
            {
                self.insert_html(&tag);
                self.pop();
                // §13.2.6.4.4: a `<meta>` naming an encoding asks to change
                // to it while the encoding is tentative. Only the first can:
                // changing the encoding makes it certain, whichever way it
                // goes, and `parse_bytes` is the one that knows whether it was.
                if tag.name == "meta" && self.meta_encoding.is_none() {
                    self.meta_encoding = meta_encoding(&tag);
                }
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "title" => {
                self.generic_text(&tag, State::Rcdata);
                Step::Done
            }
            // Scripting is disabled, so `<noscript>` is markup (below).
            Token::StartTag(tag) if tag.name == "noframes" || tag.name == "style" => {
                self.generic_text(&tag, State::Rawtext);
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "noscript" => {
                self.insert_html(&tag);
                self.mode = Mode::InHeadNoscript;
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "script" => {
                let (parent, before) = self.appropriate_place(None);
                let element = self.create_element(&tag, Namespace::Html);
                self.insert_before(parent, element, before);
                self.push_open(element);
                self.tokenizer.state = State::ScriptData;
                self.original_mode = self.mode;
                self.mode = Mode::Text;
                Step::Done
            }
            Token::EndTag(tag) if tag.name == "head" => {
                self.pop();
                self.mode = Mode::AfterHead;
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "template" => {
                self.push_active(Entry::Marker);
                self.frameset_ok = false;
                self.mode = Mode::InTemplate;
                self.template_modes.push(Mode::InTemplate);
                self.insert_html(&tag);
                Step::Done
            }
            Token::EndTag(tag) if tag.name == "template" => {
                if !self.template_on_stack() {
                    self.error();
                    return Step::Done;
                }
                self.generate_implied_end_tags_thoroughly();
                if !self.current_is("template") {
                    self.error();
                }
                self.pop_until("template");
                self.clear_to_last_marker();
                self.template_modes.pop();
                self.reset_insertion_mode();
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "head" => {
                self.error();
                Step::Done
            }
            Token::EndTag(tag) if !["body", "html", "br"].contains(&tag.name.as_str()) => {
                self.error();
                Step::Done
            }
            other => {
                self.pop();
                self.mode = Mode::AfterHead;
                Step::Reprocess(other)
            }
        }
    }

    fn generic_text(&mut self, tag: &Tag, state: State) {
        self.insert_html(tag);
        self.tokenizer.state = state;
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    fn in_head_noscript(&mut self, token: Token) -> Step {
        match token {
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::EndTag(tag) if tag.name == "noscript" => {
                self.pop();
                self.mode = Mode::InHead;
                Step::Done
            }
            Token::Characters(ref text) if is_space_text(text) => self.in_head(token),
            Token::Comment(_) => self.in_head(token),
            Token::StartTag(ref tag)
                if ["basefont", "bgsound", "link", "meta", "noframes", "style"]
                    .contains(&tag.name.as_str()) =>
            {
                self.in_head(token)
            }
            Token::StartTag(tag) if tag.name == "head" || tag.name == "noscript" => {
                self.error();
                Step::Done
            }
            Token::EndTag(tag) if tag.name != "br" => {
                self.error();
                Step::Done
            }
            other => {
                self.error();
                self.pop();
                self.mode = Mode::InHead;
                Step::Reprocess(other)
            }
        }
    }

    fn after_head(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) if is_space_text(&text) => {
                self.insert_text(&text);
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::StartTag(tag) if tag.name == "body" => {
                self.insert_html(&tag);
                self.frameset_ok = false;
                self.mode = Mode::InBody;
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "frameset" => {
                self.insert_html(&tag);
                self.mode = Mode::InFrameset;
                Step::Done
            }
            Token::StartTag(ref tag)
                if [
                    "base", "basefont", "bgsound", "link", "meta", "noframes", "script", "style",
                    "template", "title",
                ]
                .contains(&tag.name.as_str()) =>
            {
                self.error();
                let Some(head) = self.head else {
                    return Step::Done;
                };
                self.push_open(head);
                let step = self.in_head(token);
                self.remove_from_open(head);
                step
            }
            Token::EndTag(ref tag) if tag.name == "template" => self.in_head(token),
            Token::StartTag(tag) if tag.name == "head" => {
                self.error();
                Step::Done
            }
            Token::EndTag(tag) if !["body", "html", "br"].contains(&tag.name.as_str()) => {
                self.error();
                Step::Done
            }
            other => {
                self.insert_html_named("body");
                self.frameset_ok = true;
                self.mode = Mode::InBody;
                Step::Reprocess(other)
            }
        }
    }

    // ---- §13.2.6.4.7, "in body" ------------------------------------------

    fn in_body(&mut self, token: Token) -> Step {
        match token {
            Token::Null => {
                self.error();
                Step::Done
            }
            Token::Characters(text) => {
                self.reconstruct_active_formatting();
                self.insert_text(&text);
                if !is_space_text(&text) {
                    self.frameset_ok = false;
                }
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(tag) => self.in_body_start(tag),
            Token::EndTag(tag) => self.in_body_end(tag),
            Token::Eof => {
                if !self.template_modes.is_empty() {
                    return self.in_template(Token::Eof);
                }
                if self
                    .open
                    .iter()
                    .any(|&n| !self.is_html_one_of(n, &MAY_STAY_OPEN))
                {
                    self.error();
                }
                Step::Done
            }
        }
    }

    fn in_body_start(&mut self, mut tag: Tag) -> Step {
        let name = tag.name.clone();
        match name.as_str() {
            "html" => {
                self.error();
                if self.template_on_stack() {
                    return Step::Done;
                }
                if let Some(&root) = self.open.first() {
                    self.merge_attributes(root, &tag);
                }
            }
            "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script" | "style"
            | "template" | "title" => return self.in_head(Token::StartTag(tag)),
            "body" => {
                self.error();
                let body = self.open.get(1).copied();
                if self.open.len() == 1
                    || !body.is_some_and(|b| self.is_html(b, "body"))
                    || self.template_on_stack()
                {
                    return Step::Done;
                }
                self.frameset_ok = false;
                if let Some(body) = body {
                    self.merge_attributes(body, &tag);
                }
            }
            "frameset" => {
                self.error();
                let body = self.open.get(1).copied();
                if self.open.len() == 1 || !body.is_some_and(|b| self.is_html(b, "body")) {
                    return Step::Done;
                }
                if !self.frameset_ok {
                    return Step::Done;
                }
                if let Some(body) = body {
                    self.detach(body);
                }
                self.open.truncate(1);
                self.insert_html(&tag);
                self.mode = Mode::InFrameset;
            }
            "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog"
            | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "header"
            | "hgroup" | "main" | "menu" | "nav" | "ol" | "p" | "search" | "section"
            | "summary" | "ul" => {
                self.close_p_in_button_scope();
                self.insert_html(&tag);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.close_p_in_button_scope();
                if self.current_is_one_of(&["h1", "h2", "h3", "h4", "h5", "h6"]) {
                    self.error();
                    self.pop();
                }
                self.insert_html(&tag);
            }
            "pre" | "listing" => {
                self.close_p_in_button_scope();
                self.insert_html(&tag);
                self.ignore_lf = true;
                self.frameset_ok = false;
            }
            "form" => {
                let in_template = self.in_template_contents();
                if self.form.is_some() && !in_template {
                    self.error();
                    return Step::Done;
                }
                self.close_p_in_button_scope();
                let form = self.insert_html(&tag);
                if !in_template {
                    self.form = Some(form);
                }
            }
            "li" | "dd" | "dt" => {
                self.frameset_ok = false;
                let closes: &[&str] = if name == "li" { &["li"] } else { &["dd", "dt"] };
                for index in (0..self.open.len()).rev() {
                    let Some(&node) = self.open.get(index) else {
                        break;
                    };
                    if let Some(found) = closes.iter().find(|&&c| self.is_html(node, c)) {
                        let found = *found;
                        self.generate_implied_end_tags(Some(found));
                        if !self.current_is(found) {
                            self.error();
                        }
                        self.pop_until(found);
                        break;
                    }
                    if self.is_special(node) && !self.is_html_one_of(node, &["address", "div", "p"])
                    {
                        break;
                    }
                }
                self.close_p_in_button_scope();
                self.insert_html(&tag);
            }
            "plaintext" => {
                self.close_p_in_button_scope();
                self.insert_html(&tag);
                self.tokenizer.state = State::Plaintext;
            }
            "button" => {
                if self.in_scope("button") {
                    self.error();
                    self.generate_implied_end_tags(None);
                    self.pop_until("button");
                }
                self.reconstruct_active_formatting();
                self.insert_html(&tag);
                self.frameset_ok = false;
            }
            "a" => {
                let existing = self.active.iter().rev().find_map(|entry| match entry {
                    Entry::Marker => Some(None),
                    Entry::Element(node, f) if f.tag.name == "a" => Some(Some(*node)),
                    Entry::Element(..) => None,
                });
                if let Some(Some(node)) = existing {
                    self.error();
                    self.adoption_agency("a");
                    if let Some(at) = self.active_position(node) {
                        self.active.remove(at);
                    }
                    self.remove_from_open(node);
                }
                self.reconstruct_active_formatting();
                let element = self.insert_html(&tag);
                self.push_formatting(element, tag);
            }
            "b" | "big" | "code" | "em" | "font" | "i" | "s" | "small" | "strike" | "strong"
            | "tt" | "u" => {
                self.reconstruct_active_formatting();
                let element = self.insert_html(&tag);
                self.push_formatting(element, tag);
            }
            "nobr" => {
                self.reconstruct_active_formatting();
                if self.in_scope("nobr") {
                    self.error();
                    self.adoption_agency("nobr");
                    self.reconstruct_active_formatting();
                }
                let element = self.insert_html(&tag);
                self.push_formatting(element, tag);
            }
            "applet" | "marquee" | "object" => {
                self.reconstruct_active_formatting();
                self.insert_html(&tag);
                self.push_active(Entry::Marker);
                self.frameset_ok = false;
            }
            "table" => {
                if self.quirks != Quirks::Quirks {
                    self.close_p_in_button_scope();
                }
                self.insert_html(&tag);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct_active_formatting();
                self.insert_html(&tag);
                self.pop();
                self.frameset_ok = false;
            }
            "input" => {
                if self
                    .context
                    .is_some_and(|context| self.is_html(context, "select"))
                {
                    self.error();
                    return Step::Done;
                }
                if self.in_scope("select") {
                    self.error();
                    self.pop_until("select");
                }
                self.reconstruct_active_formatting();
                self.insert_html(&tag);
                self.pop();
                let hidden = tag
                    .attribute("type")
                    .is_some_and(|t| t.eq_ignore_ascii_case("hidden"));
                if !hidden {
                    self.frameset_ok = false;
                }
            }
            "param" | "source" | "track" => {
                self.insert_html(&tag);
                self.pop();
            }
            "hr" => {
                self.close_p_in_button_scope();
                if self.in_scope("select") {
                    self.generate_implied_end_tags(None);
                    if self.in_scope("option") || self.in_scope("optgroup") {
                        self.error();
                    }
                }
                self.insert_html(&tag);
                self.pop();
                self.frameset_ok = false;
            }
            "image" => {
                self.error();
                tag.name = "img".to_owned();
                return Step::Reprocess(Token::StartTag(tag));
            }
            "textarea" => {
                self.insert_html(&tag);
                self.ignore_lf = true;
                self.tokenizer.state = State::Rcdata;
                self.original_mode = self.mode;
                self.frameset_ok = false;
                self.mode = Mode::Text;
            }
            "xmp" => {
                self.close_p_in_button_scope();
                self.reconstruct_active_formatting();
                self.frameset_ok = false;
                self.generic_text(&tag, State::Rawtext);
            }
            "iframe" => {
                self.frameset_ok = false;
                self.generic_text(&tag, State::Rawtext);
            }
            "noembed" => self.generic_text(&tag, State::Rawtext),
            "select" => {
                if self
                    .context
                    .is_some_and(|context| self.is_html(context, "select"))
                {
                    self.error();
                } else if self.in_scope("select") {
                    self.error();
                    self.pop_until("select");
                } else {
                    self.reconstruct_active_formatting();
                    self.insert_html(&tag);
                    self.frameset_ok = false;
                }
            }
            "option" => {
                if self.in_scope("select") {
                    self.generate_implied_end_tags(Some("optgroup"));
                    if self.in_scope("option") {
                        self.error();
                    }
                } else if self.current_is("option") {
                    self.pop();
                }
                self.reconstruct_active_formatting();
                self.insert_html(&tag);
            }
            "optgroup" => {
                if self.in_scope("select") {
                    self.generate_implied_end_tags(None);
                    if self.in_scope("option") || self.in_scope("optgroup") {
                        self.error();
                    }
                } else if self.current_is("option") {
                    self.pop();
                }
                self.reconstruct_active_formatting();
                self.insert_html(&tag);
            }
            "rb" | "rtc" => {
                if self.in_scope("ruby") {
                    self.generate_implied_end_tags(None);
                    if !self.current_is("ruby") {
                        self.error();
                    }
                }
                self.insert_html(&tag);
            }
            "rp" | "rt" => {
                if self.in_scope("ruby") {
                    self.generate_implied_end_tags(Some("rtc"));
                    if !self.current_is_one_of(&["rtc", "ruby"]) {
                        self.error();
                    }
                }
                self.insert_html(&tag);
            }
            "math" => {
                self.reconstruct_active_formatting();
                adjust_mathml_attributes(&mut tag);
                self.insert_element(&tag, Namespace::MathMl);
                if tag.self_closing {
                    self.pop();
                }
            }
            "svg" => {
                self.reconstruct_active_formatting();
                adjust_svg_attributes(&mut tag);
                self.insert_element(&tag, Namespace::Svg);
                if tag.self_closing {
                    self.pop();
                }
            }
            "caption" | "col" | "colgroup" | "frame" | "head" | "tbody" | "td" | "tfoot" | "th"
            | "thead" | "tr" => {
                self.error();
            }
            _ => {
                self.reconstruct_active_formatting();
                self.insert_html(&tag);
            }
        }
        Step::Done
    }

    /// The in-body `<html>` and `<body>` rule: each of the token's attributes
    /// the element does not already carry is added to it.
    ///
    /// **Held to [`Limits::max_attributes`] per element**, as the tokenizer
    /// holds it per tag: every `<body>` may bring that many new ones, so an
    /// element merged into by a file's worth of them would carry as many as
    /// the file liked. A token whose new attributes would take the element
    /// past the cap merges none of them and stops the parse with
    /// [`Error::AttributeCap`].
    ///
    /// **A merge costs the token's attributes, and a tag with none costs
    /// nothing.** The names the element carries are kept in a set from its
    /// first merge on ([`TreeBuilder::merged`]), so a token looks each of its
    /// own names up once. The review of the lane's fixes found the set built
    /// again for every `<html>` and `<body>` — two hundred and fifty-six names
    /// for each six-byte `<html>`, seventy-five seconds of a debug build at the
    /// token cap.
    fn merge_attributes(&mut self, node: usize, tag: &Tag) {
        if tag.attributes.is_empty() {
            return;
        }
        let max = self.limits.max_attributes;
        let Some(Node {
            data: NodeData::Element(element),
            ..
        }) = self.nodes.get_mut(node)
        else {
            return;
        };
        let at = match self.merged.iter().position(|(n, _)| *n == node) {
            Some(at) => at,
            None => {
                let names = element
                    .attributes
                    .iter()
                    .filter(|a| a.namespace.is_none())
                    .map(|a| {
                        super::step();
                        a.name.clone()
                    })
                    .collect();
                self.merged.push((node, names));
                self.merged.len() - 1
            }
        };
        let Some((_, names)) = self.merged.get_mut(at) else {
            return;
        };
        let added: Vec<Attribute> = tag
            .attributes
            .iter()
            .filter(|(name, _)| {
                super::step();
                !names.contains(name.as_str())
            })
            .map(|(name, value)| Attribute {
                name: name.clone(),
                namespace: None,
                value: value.clone(),
            })
            .collect();
        if element.attributes.len().saturating_add(added.len()) > max {
            self.stop(Error::AttributeCap);
            return;
        }
        names.extend(added.iter().map(|a| a.name.clone()));
        element.attributes.extend(added);
    }

    fn in_body_end(&mut self, tag: Tag) -> Step {
        let name = tag.name.as_str();
        match name {
            "template" => return self.in_head(Token::EndTag(tag)),
            "body" | "html" => {
                if !self.in_scope("body") {
                    self.error();
                    return Step::Done;
                }
                if self
                    .open
                    .iter()
                    .any(|&n| !self.is_html_one_of(n, &MAY_STAY_OPEN))
                {
                    self.error();
                }
                self.mode = Mode::AfterBody;
                if name == "html" {
                    return Step::Reprocess(Token::EndTag(tag));
                }
            }
            "address" | "article" | "aside" | "blockquote" | "button" | "center" | "details"
            | "dialog" | "dir" | "div" | "dl" | "fieldset" | "figcaption" | "figure" | "footer"
            | "header" | "hgroup" | "listing" | "main" | "menu" | "nav" | "ol" | "pre"
            | "search" | "section" | "select" | "summary" | "ul" => {
                if !self.in_scope(name) {
                    self.error();
                    return Step::Done;
                }
                self.generate_implied_end_tags(None);
                if !self.current_is(name) {
                    self.error();
                }
                self.pop_until(name);
            }
            "form" => {
                if self.in_template_contents() {
                    if !self.in_scope("form") {
                        self.error();
                        return Step::Done;
                    }
                    self.generate_implied_end_tags(None);
                    if !self.current_is("form") {
                        self.error();
                    }
                    self.pop_until("form");
                } else {
                    let node = self.form.take();
                    let Some(node) = node.filter(|&n| self.node_in_scope(n)) else {
                        self.error();
                        return Step::Done;
                    };
                    self.generate_implied_end_tags(None);
                    if self.current() != Some(node) {
                        self.error();
                    }
                    self.remove_from_open(node);
                }
            }
            "p" => {
                if !self.in_button_scope("p") {
                    self.error();
                    self.insert_html_named("p");
                }
                self.close_p();
            }
            "li" => {
                if !self.in_list_item_scope("li") {
                    self.error();
                    return Step::Done;
                }
                self.generate_implied_end_tags(Some("li"));
                if !self.current_is("li") {
                    self.error();
                }
                self.pop_until("li");
            }
            "dd" | "dt" => {
                if !self.in_scope(name) {
                    self.error();
                    return Step::Done;
                }
                self.generate_implied_end_tags(Some(name));
                if !self.current_is(name) {
                    self.error();
                }
                self.pop_until(name);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let headings = ["h1", "h2", "h3", "h4", "h5", "h6"];
                if !self.in_scope_one_of(&headings) {
                    self.error();
                    return Step::Done;
                }
                self.generate_implied_end_tags(None);
                if !self.current_is(name) {
                    self.error();
                }
                self.pop_until_one_of(&headings);
            }
            "a" | "b" | "big" | "code" | "em" | "font" | "i" | "nobr" | "s" | "small"
            | "strike" | "strong" | "tt" | "u" => {
                self.adoption_agency(name);
            }
            "applet" | "marquee" | "object" => {
                if !self.in_scope(name) {
                    self.error();
                    return Step::Done;
                }
                self.generate_implied_end_tags(None);
                if !self.current_is(name) {
                    self.error();
                }
                self.pop_until(name);
                self.clear_to_last_marker();
            }
            "br" => {
                self.error();
                return self.in_body_start(Tag {
                    name: "br".to_owned(),
                    ..Tag::default()
                });
            }
            _ => self.any_other_end_tag(name),
        }
        Step::Done
    }

    fn any_other_end_tag(&mut self, name: &str) {
        for index in (0..self.open.len()).rev() {
            let Some(&node) = self.open.get(index) else {
                return;
            };
            if self.is_html(node, name) {
                self.generate_implied_end_tags(Some(name));
                if self.current() != Some(node) {
                    self.error();
                }
                self.open.truncate(index);
                return;
            }
            if self.is_special(node) {
                self.error();
                return;
            }
        }
    }

    /// §13.2.6.4.7's adoption agency algorithm.
    fn adoption_agency(&mut self, subject: &str) {
        if let Some(current) = self.current() {
            if self.is_html(current, subject) && self.active_position(current).is_none() {
                self.pop();
                return;
            }
        }
        for _ in 0..8 {
            if self.halt {
                return;
            }
            // The last element in the list, after the last marker, named the
            // subject.
            let mut formatting = None;
            for (at, entry) in self.active.iter().enumerate().rev() {
                match entry {
                    Entry::Marker => break,
                    Entry::Element(node, f) if f.tag.name == subject => {
                        formatting = Some((at, *node));
                        break;
                    }
                    Entry::Element(..) => {}
                }
            }
            let Some((formatting_at, formatting)) = formatting else {
                self.any_other_end_tag(subject);
                return;
            };
            let Some(formatting_open) = self.open.iter().position(|&n| n == formatting) else {
                self.error();
                self.active.remove(formatting_at);
                return;
            };
            if !self.node_in_scope(formatting) {
                self.error();
                return;
            }
            if self.current() != Some(formatting) {
                self.error();
            }
            let furthest = self
                .open
                .iter()
                .enumerate()
                .skip(formatting_open + 1)
                .find(|&(_, &n)| self.is_special(n))
                .map(|(at, &n)| (at, n));
            let Some((_, furthest_block)) = furthest else {
                self.open.truncate(formatting_open);
                if let Some(at) = self.active_position(formatting) {
                    self.active.remove(at);
                }
                return;
            };
            let Some(common_ancestor) = formatting_open
                .checked_sub(1)
                .and_then(|at| self.open.get(at))
                .copied()
            else {
                return;
            };
            let mut bookmark = formatting_at;
            let mut node = furthest_block;
            let mut last_node = furthest_block;
            let mut inner = 0;
            let mut node_index = self.open.iter().position(|&n| n == node).unwrap_or(0);
            loop {
                inner += 1;
                // The element above `node` on the stack, as it was.
                node_index = match node_index.checked_sub(1) {
                    Some(above) => above,
                    None => break,
                };
                node = match self.open.get(node_index) {
                    Some(&n) => n,
                    None => break,
                };
                if node == formatting {
                    break;
                }
                let mut in_list = self.active_position(node);
                if inner > 3 {
                    if let Some(at) = in_list {
                        self.active.remove(at);
                        if at < bookmark {
                            bookmark -= 1;
                        }
                        in_list = None;
                    }
                }
                let Some(list_at) = in_list else {
                    self.open.remove(node_index);
                    continue;
                };
                let Some(Entry::Element(_, formatting)) = self.active.get(list_at).cloned() else {
                    break;
                };
                if !self.charge_clone(&formatting.tag) {
                    return;
                }
                let replacement = self.create_element(&formatting.tag, Namespace::Html);
                if let Some(slot) = self.active.get_mut(list_at) {
                    *slot = Entry::Element(replacement, formatting);
                }
                if let Some(slot) = self.open.get_mut(node_index) {
                    *slot = replacement;
                }
                node = replacement;
                if last_node == furthest_block {
                    bookmark = list_at + 1;
                }
                self.append(node, last_node);
                last_node = node;
            }
            let (target, before) = self.appropriate_place(Some(common_ancestor));
            self.detach(last_node);
            if !self.is_inclusive_ancestor(last_node, target) {
                self.insert_before(target, last_node, before);
            }
            let Some(Entry::Element(_, formatting_entry)) = self
                .active_position(formatting)
                .and_then(|at| self.active.get(at))
                .cloned()
            else {
                return;
            };
            if !self.charge_clone(&formatting_entry.tag) {
                return;
            }
            let replacement = self.create_element(&formatting_entry.tag, Namespace::Html);
            // Every child moves at once: one at a time was a removal from the
            // front of the furthest block's list per child.
            let children = self
                .nodes
                .get_mut(furthest_block)
                .map(|n| std::mem::take(&mut n.children))
                .unwrap_or_default();
            for &child in &children {
                if let Some(n) = self.nodes.get_mut(child) {
                    n.parent = Some(replacement);
                }
            }
            if let Some(n) = self.nodes.get_mut(replacement) {
                n.children = children;
            }
            self.append(furthest_block, replacement);
            if let Some(at) = self.active_position(formatting) {
                self.active.remove(at);
                if at < bookmark {
                    bookmark -= 1;
                }
            }
            let bookmark = bookmark.min(self.active.len());
            self.active
                .insert(bookmark, Entry::Element(replacement, formatting_entry));
            self.remove_from_open(formatting);
            if let Some(at) = self.open.iter().position(|&n| n == furthest_block) {
                self.open.insert(at + 1, replacement);
            }
        }
    }

    fn is_inclusive_ancestor(&self, ancestor: usize, node: usize) -> bool {
        let mut cursor = Some(node);
        while let Some(at) = cursor {
            if at == ancestor {
                return true;
            }
            cursor = self.nodes.get(at).and_then(|n| n.parent);
        }
        false
    }

    // ---- §13.2.6.4.8 to §13.2.6.4.23 -------------------------------------

    fn text(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) => {
                self.insert_text(&text);
                Step::Done
            }
            Token::Null => {
                self.insert_text("\u{FFFD}");
                Step::Done
            }
            Token::Eof => {
                self.error();
                self.pop();
                self.mode = self.original_mode;
                Step::Reprocess(Token::Eof)
            }
            _ => {
                self.pop();
                self.mode = self.original_mode;
                Step::Done
            }
        }
    }

    fn in_table(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(_) | Token::Null
                if self
                    .current_is_one_of(&["table", "tbody", "template", "tfoot", "thead", "tr"]) =>
            {
                self.pending_table_text.clear();
                self.original_mode = self.mode;
                self.mode = Mode::InTableText;
                Step::Reprocess(token)
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "caption" => {
                self.clear_to_table_context();
                self.push_active(Entry::Marker);
                self.insert_html(&tag);
                self.mode = Mode::InCaption;
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "colgroup" => {
                self.clear_to_table_context();
                self.insert_html(&tag);
                self.mode = Mode::InColumnGroup;
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "col" => {
                self.clear_to_table_context();
                self.insert_html_named("colgroup");
                self.mode = Mode::InColumnGroup;
                Step::Reprocess(token)
            }
            Token::StartTag(tag) if ["tbody", "tfoot", "thead"].contains(&tag.name.as_str()) => {
                self.clear_to_table_context();
                self.insert_html(&tag);
                self.mode = Mode::InTableBody;
                Step::Done
            }
            Token::StartTag(ref tag) if ["td", "th", "tr"].contains(&tag.name.as_str()) => {
                self.clear_to_table_context();
                self.insert_html_named("tbody");
                self.mode = Mode::InTableBody;
                Step::Reprocess(token)
            }
            Token::StartTag(ref tag) if tag.name == "table" => {
                self.error();
                if !self.in_table_scope("table") {
                    return Step::Done;
                }
                self.pop_until("table");
                self.reset_insertion_mode();
                Step::Reprocess(token)
            }
            Token::EndTag(tag) if tag.name == "table" => {
                if !self.in_table_scope("table") {
                    self.error();
                    return Step::Done;
                }
                self.pop_until("table");
                self.reset_insertion_mode();
                Step::Done
            }
            Token::EndTag(tag)
                if [
                    "body", "caption", "col", "colgroup", "html", "tbody", "td", "tfoot", "th",
                    "thead", "tr",
                ]
                .contains(&tag.name.as_str()) =>
            {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag)
                if ["style", "script", "template"].contains(&tag.name.as_str()) =>
            {
                self.in_head(token)
            }
            Token::EndTag(ref tag) if tag.name == "template" => self.in_head(token),
            Token::StartTag(ref tag)
                if tag.name == "input"
                    && tag
                        .attribute("type")
                        .is_some_and(|t| t.eq_ignore_ascii_case("hidden")) =>
            {
                self.error();
                self.insert_html(tag);
                self.pop();
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "form" => {
                self.error();
                if self.form.is_some() && !self.in_template_contents() {
                    return Step::Done;
                }
                let form = self.insert_html(&tag);
                if !self.in_template_contents() {
                    self.form = Some(form);
                }
                self.pop();
                Step::Done
            }
            Token::Eof => self.in_body(Token::Eof),
            other => {
                self.error();
                self.foster = true;
                let step = self.in_body(other);
                self.foster = false;
                step
            }
        }
    }

    fn clear_to_table_context(&mut self) {
        while !self.current_is_one_of(&["table", "template", "html"]) {
            if self.pop().is_none() {
                return;
            }
        }
    }

    fn in_table_text(&mut self, token: Token) -> Step {
        match token {
            Token::Null => {
                self.error();
                Step::Done
            }
            Token::Characters(text) => {
                self.pending_table_text.push(text);
                Step::Done
            }
            other => {
                let pending = std::mem::take(&mut self.pending_table_text);
                if pending.iter().any(|t| !is_space_text(t)) {
                    self.error();
                    for text in pending {
                        self.foster = true;
                        self.in_body(Token::Characters(text));
                        self.foster = false;
                    }
                } else {
                    for text in pending {
                        self.insert_text(&text);
                    }
                }
                self.mode = self.original_mode;
                Step::Reprocess(other)
            }
        }
    }

    fn in_caption(&mut self, token: Token) -> Step {
        match token {
            Token::EndTag(ref tag) if tag.name == "caption" => {
                if self.close_caption() {
                    self.mode = Mode::InTable;
                }
                Step::Done
            }
            Token::StartTag(ref tag)
                if [
                    "caption", "col", "colgroup", "tbody", "td", "tfoot", "th", "thead", "tr",
                ]
                .contains(&tag.name.as_str()) =>
            {
                if self.close_caption() {
                    self.mode = Mode::InTable;
                    Step::Reprocess(token)
                } else {
                    Step::Done
                }
            }
            Token::EndTag(ref tag) if tag.name == "table" => {
                if self.close_caption() {
                    self.mode = Mode::InTable;
                    Step::Reprocess(token)
                } else {
                    Step::Done
                }
            }
            Token::EndTag(ref tag)
                if [
                    "body", "col", "colgroup", "html", "tbody", "td", "tfoot", "th", "thead", "tr",
                ]
                .contains(&tag.name.as_str()) =>
            {
                self.error();
                Step::Done
            }
            other => self.in_body(other),
        }
    }

    /// The shared steps of ending a caption; `false` when there was none in
    /// table scope (the fragment case).
    fn close_caption(&mut self) -> bool {
        if !self.in_table_scope("caption") {
            self.error();
            return false;
        }
        self.generate_implied_end_tags(None);
        if !self.current_is("caption") {
            self.error();
        }
        self.pop_until("caption");
        self.clear_to_last_marker();
        true
    }

    fn in_column_group(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) if is_space_text(&text) => {
                self.insert_text(&text);
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::StartTag(tag) if tag.name == "col" => {
                self.insert_html(&tag);
                self.pop();
                Step::Done
            }
            Token::EndTag(tag) if tag.name == "colgroup" => {
                if !self.current_is("colgroup") {
                    self.error();
                    return Step::Done;
                }
                self.pop();
                self.mode = Mode::InTable;
                Step::Done
            }
            Token::EndTag(tag) if tag.name == "col" => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "template" => self.in_head(token),
            Token::EndTag(ref tag) if tag.name == "template" => self.in_head(token),
            Token::Eof => self.in_body(Token::Eof),
            other => {
                if !self.current_is("colgroup") {
                    self.error();
                    return Step::Done;
                }
                self.pop();
                self.mode = Mode::InTable;
                Step::Reprocess(other)
            }
        }
    }

    fn clear_to_table_body_context(&mut self) {
        while !self.current_is_one_of(&["tbody", "tfoot", "thead", "template", "html"]) {
            if self.pop().is_none() {
                return;
            }
        }
    }

    fn in_table_body(&mut self, token: Token) -> Step {
        match token {
            Token::StartTag(tag) if tag.name == "tr" => {
                self.clear_to_table_body_context();
                self.insert_html(&tag);
                self.mode = Mode::InRow;
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "th" || tag.name == "td" => {
                self.error();
                self.clear_to_table_body_context();
                self.insert_html_named("tr");
                self.mode = Mode::InRow;
                Step::Reprocess(token)
            }
            Token::EndTag(tag) if ["tbody", "tfoot", "thead"].contains(&tag.name.as_str()) => {
                if !self.in_table_scope(&tag.name) {
                    self.error();
                    return Step::Done;
                }
                self.clear_to_table_body_context();
                self.pop();
                self.mode = Mode::InTable;
                Step::Done
            }
            Token::StartTag(ref tag)
                if ["caption", "col", "colgroup", "tbody", "tfoot", "thead"]
                    .contains(&tag.name.as_str()) =>
            {
                self.leave_table_body(token)
            }
            Token::EndTag(ref tag) if tag.name == "table" => self.leave_table_body(token),
            Token::EndTag(tag)
                if [
                    "body", "caption", "col", "colgroup", "html", "td", "th", "tr",
                ]
                .contains(&tag.name.as_str()) =>
            {
                self.error();
                Step::Done
            }
            other => self.in_table(other),
        }
    }

    fn leave_table_body(&mut self, token: Token) -> Step {
        if !self.in_table_scope_one_of(&["tbody", "thead", "tfoot"]) {
            self.error();
            return Step::Done;
        }
        self.clear_to_table_body_context();
        self.pop();
        self.mode = Mode::InTable;
        Step::Reprocess(token)
    }

    fn clear_to_table_row_context(&mut self) {
        while !self.current_is_one_of(&["tr", "template", "html"]) {
            if self.pop().is_none() {
                return;
            }
        }
    }

    fn in_row(&mut self, token: Token) -> Step {
        match token {
            Token::StartTag(tag) if tag.name == "th" || tag.name == "td" => {
                self.clear_to_table_row_context();
                self.insert_html(&tag);
                self.mode = Mode::InCell;
                self.push_active(Entry::Marker);
                Step::Done
            }
            Token::EndTag(ref tag) if tag.name == "tr" => {
                if self.close_row() {
                    self.mode = Mode::InTableBody;
                }
                Step::Done
            }
            Token::StartTag(ref tag)
                if [
                    "caption", "col", "colgroup", "tbody", "tfoot", "thead", "tr",
                ]
                .contains(&tag.name.as_str()) =>
            {
                if self.close_row() {
                    self.mode = Mode::InTableBody;
                    Step::Reprocess(token)
                } else {
                    Step::Done
                }
            }
            Token::EndTag(ref tag) if tag.name == "table" => {
                if self.close_row() {
                    self.mode = Mode::InTableBody;
                    Step::Reprocess(token)
                } else {
                    Step::Done
                }
            }
            Token::EndTag(ref tag) if ["tbody", "tfoot", "thead"].contains(&tag.name.as_str()) => {
                if !self.in_table_scope(&tag.name) {
                    self.error();
                    return Step::Done;
                }
                if !self.in_table_scope("tr") {
                    return Step::Done;
                }
                self.clear_to_table_row_context();
                self.pop();
                self.mode = Mode::InTableBody;
                Step::Reprocess(token)
            }
            Token::EndTag(tag)
                if ["body", "caption", "col", "colgroup", "html", "td", "th"]
                    .contains(&tag.name.as_str()) =>
            {
                self.error();
                Step::Done
            }
            other => self.in_table(other),
        }
    }

    /// The shared steps of ending a row; `false` when there was no `tr` in
    /// table scope.
    fn close_row(&mut self) -> bool {
        if !self.in_table_scope("tr") {
            self.error();
            return false;
        }
        self.clear_to_table_row_context();
        self.pop();
        true
    }

    fn in_cell(&mut self, token: Token) -> Step {
        match token {
            Token::EndTag(tag) if tag.name == "td" || tag.name == "th" => {
                if !self.in_table_scope(&tag.name) {
                    self.error();
                    return Step::Done;
                }
                self.generate_implied_end_tags(None);
                if !self.current_is(&tag.name) {
                    self.error();
                }
                self.pop_until(&tag.name);
                self.clear_to_last_marker();
                self.mode = Mode::InRow;
                Step::Done
            }
            Token::StartTag(ref tag)
                if [
                    "caption", "col", "colgroup", "tbody", "td", "tfoot", "th", "thead", "tr",
                ]
                .contains(&tag.name.as_str()) =>
            {
                if !self.in_table_scope_one_of(&["td", "th"]) {
                    // Not reachable outside the fragment case; the standard
                    // asserts it, and ignoring the token is the safe reading.
                    self.error();
                    return Step::Done;
                }
                self.close_cell();
                Step::Reprocess(token)
            }
            Token::EndTag(tag)
                if ["body", "caption", "col", "colgroup", "html"].contains(&tag.name.as_str()) =>
            {
                self.error();
                Step::Done
            }
            Token::EndTag(ref tag)
                if ["table", "tbody", "tfoot", "thead", "tr"].contains(&tag.name.as_str()) =>
            {
                if !self.in_table_scope(&tag.name) {
                    self.error();
                    return Step::Done;
                }
                self.close_cell();
                Step::Reprocess(token)
            }
            other => self.in_body(other),
        }
    }

    fn close_cell(&mut self) {
        self.generate_implied_end_tags(None);
        if !self.current_is_one_of(&["td", "th"]) {
            self.error();
        }
        self.pop_until_one_of(&["td", "th"]);
        self.clear_to_last_marker();
        self.mode = Mode::InRow;
    }

    fn in_template(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(_) | Token::Null | Token::Comment(_) | Token::Doctype(_) => {
                self.in_body(token)
            }
            Token::StartTag(ref tag)
                if [
                    "base", "basefont", "bgsound", "link", "meta", "noframes", "script", "style",
                    "template", "title",
                ]
                .contains(&tag.name.as_str()) =>
            {
                self.in_head(token)
            }
            Token::EndTag(ref tag) if tag.name == "template" => self.in_head(token),
            Token::StartTag(ref tag) => {
                let mode = match tag.name.as_str() {
                    "caption" | "colgroup" | "tbody" | "tfoot" | "thead" => Mode::InTable,
                    "col" => Mode::InColumnGroup,
                    "tr" => Mode::InTableBody,
                    "td" | "th" => Mode::InRow,
                    _ => Mode::InBody,
                };
                self.template_modes.pop();
                self.template_modes.push(mode);
                self.mode = mode;
                Step::Reprocess(token)
            }
            Token::EndTag(_) => {
                self.error();
                Step::Done
            }
            Token::Eof => {
                if !self.template_on_stack() {
                    return Step::Done;
                }
                self.error();
                self.pop_until("template");
                self.clear_to_last_marker();
                self.template_modes.pop();
                self.reset_insertion_mode();
                Step::Reprocess(Token::Eof)
            }
        }
    }

    fn after_body(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(ref text) if is_space_text(text) => self.in_body(token),
            Token::Comment(text) => {
                let html = self.open.first().copied().unwrap_or(0);
                self.insert_comment(text, Some((html, None)));
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::EndTag(ref tag) if tag.name == "html" => {
                if self.context.is_some() {
                    self.error();
                } else {
                    self.mode = Mode::AfterAfterBody;
                }
                Step::Done
            }
            Token::Eof => Step::Done,
            other => {
                self.error();
                self.mode = Mode::InBody;
                Step::Reprocess(other)
            }
        }
    }

    fn in_frameset(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) if is_space_text(&text) => {
                self.insert_text(&text);
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::StartTag(tag) if tag.name == "frameset" => {
                self.insert_html(&tag);
                Step::Done
            }
            Token::EndTag(tag) if tag.name == "frameset" => {
                if self.open.len() <= 1 {
                    self.error();
                    return Step::Done;
                }
                self.pop();
                if self.context.is_none() && !self.current_is("frameset") {
                    self.mode = Mode::AfterFrameset;
                }
                Step::Done
            }
            Token::StartTag(tag) if tag.name == "frame" => {
                self.insert_html(&tag);
                self.pop();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "noframes" => self.in_head(token),
            Token::Eof => {
                if self.open.len() > 1 {
                    self.error();
                }
                Step::Done
            }
            _ => {
                self.error();
                Step::Done
            }
        }
    }

    fn after_frameset(&mut self, token: Token) -> Step {
        match token {
            Token::Characters(text) if is_space_text(&text) => {
                self.insert_text(&text);
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::EndTag(tag) if tag.name == "html" => {
                self.mode = Mode::AfterAfterFrameset;
                Step::Done
            }
            Token::StartTag(ref tag) if tag.name == "noframes" => self.in_head(token),
            Token::Eof => Step::Done,
            _ => {
                self.error();
                Step::Done
            }
        }
    }

    fn after_after_body(&mut self, token: Token) -> Step {
        match token {
            Token::Comment(text) => {
                self.insert_comment(text, Some((0, None)));
                Step::Done
            }
            Token::Doctype(_) => self.in_body(token),
            Token::Characters(ref text) if is_space_text(text) => self.in_body(token),
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::Eof => Step::Done,
            other => {
                self.error();
                self.mode = Mode::InBody;
                Step::Reprocess(other)
            }
        }
    }

    fn after_after_frameset(&mut self, token: Token) -> Step {
        match token {
            Token::Comment(text) => {
                self.insert_comment(text, Some((0, None)));
                Step::Done
            }
            Token::Doctype(_) => self.in_body(token),
            Token::Characters(ref text) if is_space_text(text) => self.in_body(token),
            Token::StartTag(ref tag) if tag.name == "html" => self.in_body(token),
            Token::Eof => Step::Done,
            Token::StartTag(ref tag) if tag.name == "noframes" => self.in_head(token),
            _ => {
                self.error();
                Step::Done
            }
        }
    }

    // ---- §13.2.6.5, foreign content --------------------------------------

    fn in_foreign_content(&mut self, token: Token) -> Step {
        match token {
            Token::Null => {
                self.error();
                self.insert_text("\u{FFFD}");
                Step::Done
            }
            Token::Characters(text) => {
                self.insert_text(&text);
                if !is_space_text(&text) {
                    self.frameset_ok = false;
                }
                Step::Done
            }
            Token::Comment(text) => {
                self.insert_comment(text, None);
                Step::Done
            }
            Token::Doctype(_) => {
                self.error();
                Step::Done
            }
            // Reprocessed by the insertion mode's rules directly, not through
            // the dispatcher: a MathML text integration point would send an
            // end tag straight back here.
            Token::StartTag(ref tag)
                if BREAKS_FOREIGN.contains(&tag.name.as_str())
                    || (tag.name == "font"
                        && ["color", "face", "size"]
                            .iter()
                            .any(|a| tag.attribute(a).is_some())) =>
            {
                self.error();
                self.pop_out_of_foreign();
                self.handle(self.mode, token)
            }
            Token::EndTag(ref tag) if tag.name == "br" || tag.name == "p" => {
                self.error();
                self.pop_out_of_foreign();
                self.handle(self.mode, token)
            }
            Token::StartTag(mut tag) => {
                let namespace = self
                    .adjusted_current()
                    .and_then(|n| self.namespace(n))
                    .unwrap_or(Namespace::Html);
                if namespace == Namespace::MathMl {
                    adjust_mathml_attributes(&mut tag);
                }
                if namespace == Namespace::Svg {
                    if let Some((_, fixed)) = SVG_TAG_NAMES.iter().find(|(l, _)| *l == tag.name) {
                        tag.name = (*fixed).to_owned();
                    }
                    adjust_svg_attributes(&mut tag);
                }
                self.insert_element(&tag, namespace);
                if tag.self_closing {
                    self.pop();
                }
                Step::Done
            }
            Token::EndTag(tag) => {
                let Some(mut index) = self.open.len().checked_sub(1) else {
                    return Step::Done;
                };
                let lower = |b: &Self, node: usize| {
                    b.element(node)
                        .is_some_and(|e| e.name.eq_ignore_ascii_case(&tag.name))
                };
                if self.open.get(index).is_some_and(|&n| !lower(self, n)) {
                    self.error();
                }
                loop {
                    if index == 0 {
                        return Step::Done;
                    }
                    let Some(&node) = self.open.get(index) else {
                        return Step::Done;
                    };
                    if lower(self, node) {
                        self.open.truncate(index);
                        return Step::Done;
                    }
                    index -= 1;
                    let Some(&above) = self.open.get(index) else {
                        return Step::Done;
                    };
                    if self.namespace(above) == Some(Namespace::Html) {
                        let mode = self.mode;
                        return self.handle(mode, Token::EndTag(tag));
                    }
                }
            }
            Token::Eof => self.handle(self.mode, Token::Eof),
        }
    }

    fn pop_out_of_foreign(&mut self) {
        while let Some(current) = self.current() {
            let Some(element) = self.element(current) else {
                return;
            };
            let mathml_text = element.namespace == Namespace::MathMl
                && ["mi", "mo", "mn", "ms", "mtext"].contains(&element.name.as_str());
            let html_integration = element.integration_point
                || (element.namespace == Namespace::Svg
                    && ["foreignObject", "desc", "title"].contains(&element.name.as_str()));
            if element.namespace == Namespace::Html || mathml_text || html_integration {
                return;
            }
            self.pop();
        }
    }
}

/// An attribute on an element in `namespace`, with §13.2.6.1's foreign
/// attribute adjustment applied there.
fn attribute(namespace: Namespace, name: &str, value: &str) -> Attribute {
    let adjusted = if namespace == Namespace::Html {
        None
    } else {
        match name {
            "xlink:actuate" | "xlink:arcrole" | "xlink:href" | "xlink:role" | "xlink:show"
            | "xlink:title" | "xlink:type" => {
                Some((AttributeNamespace::XLink, name.get(6..).unwrap_or("")))
            }
            "xml:lang" | "xml:space" => {
                Some((AttributeNamespace::Xml, name.get(4..).unwrap_or("")))
            }
            "xmlns" => Some((AttributeNamespace::Xmlns, "xmlns")),
            "xmlns:xlink" => Some((AttributeNamespace::Xmlns, "xlink")),
            _ => None,
        }
    };
    match adjusted {
        Some((namespace, local)) => Attribute {
            name: local.to_owned(),
            namespace: Some(namespace),
            value: value.to_owned(),
        },
        None => Attribute {
            name: name.to_owned(),
            namespace: None,
            value: value.to_owned(),
        },
    }
}

fn adjust_svg_attributes(tag: &mut Tag) {
    for (name, _) in &mut tag.attributes {
        if let Some((_, fixed)) = SVG_ATTRIBUTES.iter().find(|(l, _)| l == name) {
            *name = (*fixed).to_owned();
        }
    }
}

fn adjust_mathml_attributes(tag: &mut Tag) {
    for (name, _) in &mut tag.attributes {
        if name == "definitionurl" {
            *name = "definitionURL".to_owned();
        }
    }
}

/// §13.2.6.4.1's quirks-mode decision for a DOCTYPE token.
fn quirks_of(doctype: &DoctypeToken) -> Quirks {
    let public = doctype.public_id.as_deref().map(str::to_ascii_lowercase);
    let system = doctype.system_id.as_deref().map(str::to_ascii_lowercase);
    let public_is = |s: &str| public.as_deref() == Some(s);
    let public_starts = |s: &str| public.as_deref().is_some_and(|p| p.starts_with(s));
    // "Missing or the empty string", as the two HTML 4.01 rows now say.
    let system_missing = system.as_deref().is_none_or(str::is_empty);
    if doctype.force_quirks
        || doctype.name.as_deref() != Some("html")
        || public_is("-//w3o//dtd w3 html strict 3.0//en//")
        || public_is("-/w3c/dtd html 4.0 transitional/en")
        || public_is("html")
        || system.as_deref() == Some("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
        || QUIRKS_PREFIXES.iter().any(|p| public_starts(p))
        || (system_missing && public_starts("-//w3c//dtd html 4.01 frameset//"))
        || (system_missing && public_starts("-//w3c//dtd html 4.01 transitional//"))
    {
        return Quirks::Quirks;
    }
    if public_starts("-//w3c//dtd xhtml 1.0 frameset//")
        || public_starts("-//w3c//dtd xhtml 1.0 transitional//")
        || (!system_missing && public_starts("-//w3c//dtd html 4.01 frameset//"))
        || (!system_missing && public_starts("-//w3c//dtd html 4.01 transitional//"))
    {
        return Quirks::Limited;
    }
    Quirks::NoQuirks
}

/// **The loops the review made linear, held linear by count.** Each case is
/// an input the review timed — a slow test is not a failing one, since
/// `cargo test` has no timeout — and each asserts that the steps
/// [`super::step`] counts in it stay under the input's length in bytes: a
/// child looked at from the end of its parent's list, two attributes Noah's
/// Ark compares, a name a merge looks up. Put back the scan from the front, a
/// name lookup per attribute or a set built per merge, and the count is the
/// square of the input rather than a fraction of it.
#[cfg(test)]
mod tests {
    use super::super::{parse, STEPS};
    use crate::Limits;

    /// The steps parsing `markup` takes, and that it parsed to its end.
    fn steps(markup: &str) -> usize {
        STEPS.with(|steps| steps.set(0));
        let document = parse(markup, &Limits::DEFAULT);
        assert_eq!(document.stopped(), None);
        STEPS.with(std::cell::Cell::get)
    }

    fn assert_linear(markup: &str, what: &str) {
        let taken = steps(markup);
        assert!(
            taken <= markup.len(),
            "{what}: {taken} steps for {} bytes",
            markup.len()
        );
    }

    fn names(count: usize) -> String {
        (0..count).map(|i| format!(" a{i}")).collect()
    }

    #[test]
    fn a_merge_costs_its_own_tags_attributes() {
        let many = names(255);
        let tokens = 20_000;
        assert_linear(
            &format!("<html{many}>{}", "<html>".repeat(tokens)),
            "bare <html>s",
        );
        assert_linear(
            &format!("<html{many}>{}", "<html a0>".repeat(tokens)),
            "<html a0>s",
        );
        assert_linear(
            &format!("<body{many}>{}", "<body>".repeat(tokens)),
            "bare <body>s",
        );
        assert_linear(
            &format!("<body{many}>{}", "<body a0 b>".repeat(tokens)),
            "<body a0 b>s",
        );
    }

    #[test]
    fn noahs_ark_compares_each_attribute_once() {
        let many = names(254);
        let mut markup = String::new();
        for i in 0..60 {
            markup.push_str(&format!("<b{many} z={i}>"));
        }
        for j in 0..10 {
            markup.push_str(&format!("<b{many} z=n{j}></b>"));
        }
        // And four the same, of which the earliest goes.
        markup.push_str(&format!("<p>{}</p>x", format!("<b{many}>").repeat(4)));
        assert_linear(&markup, "Noah's Ark");
    }

    #[test]
    fn a_child_is_found_from_the_end_of_its_parents_list() {
        let many = 5_000;
        assert_linear(
            &format!("<b><div>{}</b>", "<i></i>".repeat(many)),
            "the adoption agency's move",
        );
        assert_linear(
            &format!("<table>{}</table>", "<span></span>".repeat(many)),
            "fostered elements",
        );
        assert_linear(
            &format!("<table>{}</table>", "<b></b>x".repeat(many)),
            "fostered text",
        );
    }
}
