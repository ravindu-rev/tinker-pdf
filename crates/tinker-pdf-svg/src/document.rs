//! An SVG document as an element tree, and SVG 1.1 §4.2's length grammar.
//!
//! The join between `tinker-pdf-xml`'s event stream and everything above it.
//! There is exactly one XML parser in this repository and this file does not
//! add a second: what it adds is the vocabulary — that `class` is a
//! space-separated token list, that `xlink:href` and `href` name the same
//! thing, and that a `<style>` element's body is character data.
//!
//! # The doctype mode is [`Doctype::SkipExternalId`], and that is a decision
//!
//! `tinker-pdf-xml` refuses `<!DOCTYPE` outright in its default mode, and an
//! SVG is the one document type in this tree most likely to carry one:
//! `-//W3C//DTD SVG 1.1//EN` is the first entry of
//! [`tinker_pdf_xml::ALLOWED_PUBLIC_IDENTIFIERS`], and Adobe Illustrator has
//! written it at the top of every file it has ever exported. Refusing it would
//! lose those documents whole. The relaxed mode splits the construct where the
//! danger is — an **internal subset** is [`tinker_pdf_xml::Error::InternalSubset`],
//! refused at the `[` with nothing inside it read — so billion laughs and its
//! three relatives are refused by name here for free, and
//! `tinker-pdf-xml/tests/bombs.rs` is what says so in *both* modes.
//!
//! # A partial tree is kept
//!
//! Ruling 2. A document that stops half way has still said most of a picture,
//! and an exporter that truncated a file at 64 KB should lose the bottom of the
//! drawing rather than all of it. [`Tree::truncated`] says it happened.

use tinker_pdf_xml::{Doctype, Event, Limits as XmlLimits, Source};

use crate::{Limits, Refusal};

/// The namespace an SVG element is in.
pub const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// The namespace `xlink:href` is in.
pub const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";

/// What sits inside an element.
///
/// Interleaved rather than one concatenated string, because `<text>Hello
/// <tspan>there</tspan></text>` sets two runs at two positions and a tree that
/// had flattened the character data could not say where the second begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Child {
    /// A child element, by index into [`Tree::nodes`].
    Element(usize),
    /// Character data, exactly as the source wrote it.
    Text(String),
}

/// One element of an SVG document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The local name, without a prefix.
    pub name: String,
    /// The namespace the name resolved in, or `None` for a name in no
    /// namespace at all.
    pub namespace: Option<String>,
    /// `id`, which SVG spells with no namespace.
    pub id: Option<String>,
    /// `class`, split on white space.
    pub classes: Vec<String>,
    /// Every attribute, under the qualified name the source spelled — so
    /// `xlink:href` stays `xlink:href` and an author's `[xlink|href]` would
    /// have had to be written the same way.
    pub attributes: Vec<(String, String)>,
    /// The parent's index, always less than this node's own.
    pub parent: Option<usize>,
    /// The previous element sibling.
    pub previous: Option<usize>,
    /// The next element sibling.
    pub next: Option<usize>,
    /// Children, in document order, elements and character data interleaved.
    pub children: Vec<Child>,
    /// `style=""`, unparsed.
    pub style: Option<String>,
    /// How deep this element sits, the document element being zero.
    pub depth: usize,
}

impl Node {
    /// An attribute's value, by the name the source spelled.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether this element is one SVG defines.
    ///
    /// A document with no `xmlns` at all is treated as SVG, for
    /// `tinker_pdf::epub::xhtml::Node::is_html`'s reason exactly: the media
    /// type already said what the document is, and refusing an unqualified
    /// `<svg>` would lose files whose only defect is a missing declaration.
    #[must_use]
    pub fn is_svg(&self) -> bool {
        match &self.namespace {
            None => true,
            Some(namespace) => namespace == SVG_NAMESPACE,
        }
    }

    /// `href`, in either of its two spellings.
    ///
    /// SVG 1.1 says `xlink:href` and SVG 2 says `href`; every renderer accepts
    /// both and every real file uses one or the other. Preferring the plain
    /// one matches SVG 2's own rule for an element carrying both.
    #[must_use]
    pub fn href(&self) -> Option<&str> {
        self.attr("href")
            .or_else(|| self.attr("xlink:href"))
            .or_else(|| {
                self.attributes
                    .iter()
                    .find(|(name, _)| name.ends_with(":href"))
                    .map(|(_, value)| value.as_str())
            })
    }

    /// Every character-data child, concatenated — a `<style>` element's body,
    /// or a `<title>`'s.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            if let Child::Text(text) = child {
                out.push_str(text);
            }
        }
        out
    }
}

/// A whole document, as elements.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    /// Every element, in document order, parents before children.
    pub nodes: Vec<Node>,
    /// The document element's index.
    pub root: usize,
    /// Whether the reader stopped before the end of the document.
    pub truncated: bool,
}

impl Tree {
    /// The first element with the given `id`, in document order.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.id.as_deref() == Some(id))
    }

    /// Whether `at` is `descendant` or an ancestor of it.
    ///
    /// Walking up from the descendant rather than down from `at`, so the cost
    /// is the depth rather than the subtree — which matters because the one
    /// caller is `<use>`'s cycle check and it asks on every expansion.
    #[must_use]
    pub fn contains(&self, at: usize, descendant: usize) -> bool {
        let mut cursor = Some(descendant);
        while let Some(index) = cursor {
            if index == at {
                return true;
            }
            cursor = self.nodes.get(index).and_then(|node| node.parent);
        }
        false
    }

    /// The child elements of `at`, in document order.
    pub fn element_children(&self, at: usize) -> impl Iterator<Item = usize> + '_ {
        self.nodes
            .get(at)
            .map(|node| node.children.as_slice())
            .unwrap_or_default()
            .iter()
            .filter_map(|child| match child {
                Child::Element(index) => Some(*index),
                Child::Text(_) => None,
            })
    }
}

/// Reads a document into a tree.
///
/// # Errors
/// [`Refusal::Unreadable`] when the bytes are not XML this build decodes, or
/// carry a construct `tinker-pdf-xml` refuses by name, or hold no element at
/// all; [`Refusal::NotAnSvg`] when the document element is not an `<svg>`;
/// [`Refusal::TooDeep`] and [`Refusal::TooManyNodes`] for the two ceilings.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<Tree, Refusal> {
    let Ok(source) = Source::new(bytes) else {
        return Err(Refusal::Unreadable);
    };
    // **The reader keeps its own 256 and this crate counts its own depth.**
    // Handing the reader the smaller of the two was the first draft and it is
    // wrong in a way only a test says: the reader refuses a level *before*
    // yielding the start tag that crossed it, so the caller's cap would fire
    // under the reader's name — [`Refusal::Unreadable`] on a truncated tree —
    // rather than under [`Refusal::TooDeep`]. The two caps now sit in series,
    // this one in front, and a caller that raises `max_depth` past 256 gets
    // the reader's answer because at that point the reader's is the smaller.
    let mut reader = source.reader_with(&XmlLimits::DEFAULT, Doctype::SkipExternalId);
    let mut tree = Tree::default();
    // The indices of the elements that are open, innermost last.
    let mut open: Vec<usize> = Vec::new();
    let mut too_deep = false;

    for event in &mut reader {
        let Ok(event) = event else {
            tree.truncated = true;
            break;
        };
        match event {
            Event::Start(element) => {
                if open.len() >= limits.max_depth {
                    too_deep = true;
                    break;
                }
                if tree.nodes.len() >= limits.max_nodes {
                    return Err(Refusal::TooManyNodes);
                }
                let index = tree.nodes.len();
                let mut node = Node {
                    name: element.local().to_owned(),
                    namespace: element.namespace().map(str::to_owned),
                    id: None,
                    classes: Vec::new(),
                    attributes: Vec::with_capacity(element.attributes().len()),
                    parent: open.last().copied(),
                    previous: None,
                    next: None,
                    children: Vec::new(),
                    style: None,
                    depth: open.len(),
                };
                for attribute in element.attributes() {
                    let name = attribute.name().qualified();
                    let value = attribute.value();
                    match name {
                        "id" => node.id = Some(value.to_owned()),
                        "class" => {
                            node.classes = value.split_whitespace().map(str::to_owned).collect();
                        }
                        "style" => node.style = Some(value.to_owned()),
                        _ => {}
                    }
                    node.attributes.push((name.to_owned(), value.to_owned()));
                }
                if let Some(&parent) = open.last() {
                    let previous =
                        tree.nodes[parent]
                            .children
                            .iter()
                            .rev()
                            .find_map(|child| match child {
                                Child::Element(at) => Some(*at),
                                Child::Text(_) => None,
                            });
                    node.previous = previous;
                    if let Some(previous) = previous {
                        tree.nodes[previous].next = Some(index);
                    }
                    tree.nodes[parent].children.push(Child::Element(index));
                }
                tree.nodes.push(node);
                open.push(index);
            }
            // `open` cannot be empty here: `tinker-pdf-xml` emits one `End` per
            // `Start` — an empty-element tag produces both — and refuses a
            // stray end tag before it reaches this loop.
            Event::End(_) => {
                open.pop();
            }
            Event::Text(text) | Event::Cdata(text) => {
                if let Some(&parent) = open.last() {
                    tree.nodes[parent]
                        .children
                        .push(Child::Text(text.into_owned()));
                }
            }
            Event::Comment(_) | Event::Instruction { .. } => {}
        }
    }

    // Checked after the loop rather than inside it, so a document that is too
    // deep in one branch is still reported as too deep rather than as the
    // truncation the `break` above also produces.
    if too_deep {
        return Err(Refusal::TooDeep);
    }
    if tree.nodes.is_empty() {
        return Err(Refusal::Unreadable);
    }
    if !tree.nodes[0].is_svg() || tree.nodes[0].name != "svg" {
        return Err(Refusal::NotAnSvg);
    }
    Ok(tree)
}

// ---- §4.2's lengths ---------------------------------------------------------

/// CSS pixels per inch, SVG 1.1 §7.10's number rather than CSS's.
///
/// **Ninety, not ninety-six.** §7.10 fixes `1in = 90px` and derives every other
/// absolute unit from it, and a file authored against that table has its
/// millimetres measured wrong by four per cent under CSS 2.1's ninety-six. The
/// difference is one part in twenty-four — invisible on a screen and a whole
/// millimetre across an A4 page.
const PX_PER_INCH: f64 = 90.0;

/// The `font-size` an `em` resolves against when nothing has set one.
///
/// `medium`, which CSS leaves to the user agent and every user agent makes
/// sixteen pixels. It is a constant here rather than a parameter because the
/// only lengths this crate resolves in `em` are on the root, where there is
/// nothing above to inherit from.
const INITIAL_FONT_SIZE: f64 = 16.0;

/// §4.2's `<length>`: a number, optionally a unit, and `%` of a basis.
///
/// `None` for anything that is not the grammar, which a caller turns into
/// [`crate::Warning::ValueUnreadable`] — a length silently read as zero
/// collapses a shape to nothing while looking like a file that drew nothing.
///
/// `percent_of` is the dimension a percentage is taken of: the viewport's
/// width for `x` and `width`, its height for `y` and `height`, and §7.10's
/// normalised diagonal for `r`. A caller with no basis passes `None`, and a
/// percentage then has no meaning and is refused rather than guessed at.
#[must_use]
pub fn length(text: &str, percent_of: Option<f64>) -> Option<f64> {
    let text = text.trim();
    let split = text
        .char_indices()
        .find(|(_, c)| !matches!(c, '0'..='9' | '+' | '-' | '.' | 'e' | 'E'))
        .map_or(text.len(), |(at, _)| at);
    let (number, unit) = text.split_at(split);
    // The number is read by the same grammar the transform list uses, so
    // `.5in` and `1e2` mean here what they mean there.
    let numbers = crate::transform::numbers(number)?;
    let [value] = numbers[..] else { return None };
    let scale = match unit.trim() {
        "" | "px" => 1.0,
        "pt" => PX_PER_INCH / 72.0,
        "pc" => PX_PER_INCH / 6.0,
        "in" => PX_PER_INCH,
        "mm" => PX_PER_INCH / 25.4,
        "cm" => PX_PER_INCH / 2.54,
        "em" => INITIAL_FONT_SIZE,
        // §4.2 defines `ex` as the font's x-height and CSS 2.1 §4.3.2 makes
        // half an `em` the value to use when the font does not say — which no
        // font this crate can see does, because it can see no font at all.
        "ex" => INITIAL_FONT_SIZE / 2.0,
        "%" => percent_of? / 100.0,
        _ => return None,
    };
    let out = value * scale;
    out.is_finite().then_some(out)
}

/// §7.10's normalised diagonal, which is what a percentage of a radius is of.
///
/// Written out rather than left as `hypot`: that method is not one of the
/// correctly-rounded five, so ruling 4 bars it from anything that decides
/// where ink lands.
#[must_use]
pub fn diagonal(width: f64, height: f64) -> f64 {
    ((width * width + height * height) / 2.0).sqrt()
}
