//! CSS: bytes in, a stylesheet out; an element and a selector in, a match and a
//! specificity out; declarations and a tree in, computed styles out.
//!
//! Scope, design and exit criteria: `docs/plans/gaps/31-epub.md`, milestone 6.
//!
//! **The ninth leaf**, and the fourth crate in this workspace with no internal
//! dependency at all, beside `filters`, `crypto` and `xml`. Ruling 8's August
//! 2026 amendment makes the test of a leaf the definition rather than the list:
//! *a leaf is any crate that takes bytes and plain parameters and returns bytes
//! and values, whatever the list says.* Nothing below knows what a PDF, a page,
//! an EPUB or an XHTML element is — the document tree lives in the facade and
//! reaches this crate through [`Element`], five methods wide.
//!
//! # The five things worth knowing before reading further
//!
//! **A property is a parser variant only when a consumer exists, and the
//! compiler enforces it.** [`property::Property`] has three exhaustive
//! consumers with no `_` arm between them — [`cascade::apply`],
//! [`property::Property::name`] and [`property::Property::inherited`] — so a
//! property parsed and then ignored **does not compile**. That is gap 31's
//! central device and the reason the whole reflowable scope was accepted; a
//! partially-implemented CSS property does not fail, it lays the page out
//! slightly differently and nobody can tell by looking.
//! `tests/unimplemented_property_does_not_build.rs` injects the defect and
//! asserts the **build** fails, in both directions.
//!
//! **The implemented set is keyed by (property, value).** `float: inline-start`
//! is not `float: left` and `display: flex` is not `display: block`. A value
//! outside a property's set is [`property::Declaration::Unsupported`], counted
//! and named, even though the property is "supported" — because a build that
//! maps an unhandled value onto its nearest handled one is producing gap 07's
//! solid-black gradient in a stylesheet.
//!
//! **Error recovery is normative and its counts are the deliverable.**
//! `css-syntax-3` §5.4.4 discards a malformed declaration to the next semicolon
//! and §5.4.2 a malformed rule to the end of its block, so a stylesheet with
//! one bad rule yields the rest. [`parser::Report`] says how many of each were
//! lost: a build that refuses the sheet renders an unstyled book that looks
//! entirely fine, and one that discards silently cannot say how much it
//! discarded.
//!
//! **The cascade is `css-cascade-5` §6.1 whole, including the `!important`
//! origin reversal.** An `!important` author rule **loses** to an `!important`
//! UA rule. That is backwards from the normal case, it is how a reading system
//! keeps control of what it must, and it is the half a first implementation
//! drops.
//!
//! **`@media` is evaluated as `screen`.** The argument is in
//! [`media`]'s header and it is a decision rather than a default: an EPUB is
//! authored for a reading system, and that the *output* here is a PDF is an
//! implementation fact about synthesis rather than a statement about the
//! medium.
//!
//! # Using it
//!
//! ```
//! use tinker_pdf_css::{cascade, media::MediaContext, Budget, Element, Limits, NoImports};
//!
//! struct Node {
//!     name: String,
//!     classes: Vec<String>,
//!     parent: Option<usize>,
//! }
//!
//! impl Element for Node {
//!     fn local_name(&self) -> &str {
//!         &self.name
//!     }
//!     fn id(&self) -> Option<&str> {
//!         None
//!     }
//!     fn classes(&self) -> &[String] {
//!         &self.classes
//!     }
//!     fn attribute(&self, _: &str) -> Option<&str> {
//!         None
//!     }
//!     fn parent(&self) -> Option<usize> {
//!         self.parent
//!     }
//!     fn previous_sibling(&self) -> Option<usize> {
//!         None
//!     }
//!     fn next_sibling(&self) -> Option<usize> {
//!         None
//!     }
//! }
//!
//! let limits = Limits::DEFAULT;
//! let mut budget = Budget::new(&limits);
//! let context = MediaContext::screen(432.0, 648.0);
//! let sheet = tinker_pdf_css::parse(
//!     b"p .lead { font-weight: bold }",
//!     None,
//!     &NoImports,
//!     &context,
//!     &limits,
//!     &mut budget,
//! )?;
//!
//! let tree = [
//!     Node { name: "p".into(), classes: vec![], parent: None },
//!     Node { name: "span".into(), classes: vec!["lead".into()], parent: Some(0) },
//! ];
//! let styled = cascade::cascade(
//!     &[(cascade::Origin::Author, &sheet)],
//!     &tree,
//!     &limits,
//!     &mut budget,
//! )?;
//! assert_eq!(styled.styles[1].font_weight, 700);
//! assert_eq!(styled.styles[0].font_weight, 400);
//! # Ok::<(), tinker_pdf_css::Refusal>(())
//! ```

#![forbid(unsafe_code)]

pub mod cascade;
pub mod font_face;
pub mod limits;
pub mod media;
pub mod parser;
pub mod property;
pub mod selector;
pub mod tokenizer;

#[cfg(test)]
mod tests;

use std::fmt;

pub use parser::{parse, parse_inline, Declared, Report, StyleRule, Stylesheet};

/// The element side of matching, and the whole of what this crate knows about
/// a document.
///
/// Five required methods and two provided ones, and the shape is what keeps
/// XHTML out of a CSS crate. [`Element::id`] and [`Element::classes`] exist
/// rather than `attribute("id")` and `attribute("class")` because
/// `selectors-4` §6.5 and §6.6 say the document language defines both — a
/// matcher that read those two attribute names would have hard-coded HTML into
/// a crate whose entire argument is that it has not.
///
/// **Indices, not references.** A tree of borrowed nodes would put a lifetime
/// on every signature here and make a cyclic parent link a compile error the
/// caller has to design around. The caller hands a slice in **document order**
/// instead, and every link is an index into it; [`cascade::cascade`] refuses a
/// slice whose parents do not precede their children, by name, rather than
/// reading an uninitialised style.
pub trait Element {
    /// The element's local name, without a prefix. Compared case-sensitively,
    /// which is XML's rule and therefore XHTML's.
    fn local_name(&self) -> &str;

    /// The element's ID, whatever the document language says that is.
    fn id(&self) -> Option<&str>;

    /// The element's classes, whatever the document language says those are.
    fn classes(&self) -> &[String];

    /// An attribute's value, for `[…]` selectors.
    fn attribute(&self, name: &str) -> Option<&str>;

    /// The parent's index, which **must** be less than this element's own.
    fn parent(&self) -> Option<usize>;

    /// The previous element sibling's index.
    fn previous_sibling(&self) -> Option<usize>;

    /// The next element sibling's index.
    fn next_sibling(&self) -> Option<usize>;

    /// An element-attached declaration block — `style=""` in XHTML.
    ///
    /// `css-cascade-5` §6.1's third criterion, and it beats every selector at
    /// the same origin and importance. The default is `None` so a caller with
    /// no such concept says nothing rather than inventing one.
    fn inline_style(&self) -> Option<&str> {
        None
    }

    /// Whether the element carries a class. Provided in terms of
    /// [`Element::classes`] so the two can never disagree — which they could,
    /// and silently, if both were the caller's to write.
    fn has_class(&self, name: &str) -> bool {
        self.classes().iter().any(|class| class == name)
    }
}

/// Where the bytes of an `@import` come from.
///
/// The crate cannot fetch: it has no container, no filesystem and no network,
/// which is what makes it a leaf. A caller that has an OCF container
/// implements this; one that does not passes [`NoImports`] and every `@import`
/// warns by name rather than being silently ignored.
pub trait ImportResolver {
    /// Resolves `href` against `base` — the importing sheet's own address —
    /// and returns the resolved absolute address **and** the bytes.
    ///
    /// The address comes back as well as the bytes because it is the cycle
    /// guard's key: two sheets that import each other by different relative
    /// spellings are the same pair of files, and only the resolver knows that.
    fn resolve(&self, href: &str, base: Option<&str>) -> Option<(String, Vec<u8>)>;
}

/// A resolver that resolves nothing, for a caller with no container.
pub struct NoImports;

impl ImportResolver for NoImports {
    fn resolve(&self, _href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
        None
    }
}

/// Resource ceilings, defaulting to the constants in [`limits`].
///
/// Separate fields rather than one number because they bound different things,
/// and **four of them are not here at all**: the totals live in [`Budget`],
/// which is threaded through a whole book rather than through one sheet.
/// Reading a per-item cap as the total is the mistake `MAX_SCRIPT_TOTAL`,
/// `MAX_TILE_WORK` and `tinker_pdf_xml::Limits::max_tokens` each exist to name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// One stylesheet's source. See [`limits::MAX_CSS_BYTES`].
    pub max_bytes: usize,
    /// Tokens across the book. See [`limits::MAX_CSS_TOKENS`].
    pub max_tokens: usize,
    /// Qualified rules across the book. See [`limits::MAX_CSS_RULES`].
    pub max_rules: usize,
    /// Declarations across the book. See [`limits::MAX_CSS_DECLARATIONS`].
    pub max_declarations: usize,
    /// Compounds in one complex selector. See
    /// [`limits::MAX_CSS_SELECTOR_PARTS`].
    pub max_selector_parts: usize,
    /// `@import` nesting. See [`limits::MAX_CSS_IMPORT_DEPTH`].
    pub max_import_depth: usize,
    /// Elements in one cascade pass. See [`limits::MAX_DOM_NODES`].
    pub max_elements: usize,
    /// Compound-against-element tests across the book. See
    /// [`limits::MAX_SELECTOR_MATCHES`].
    pub max_matches: usize,
}

impl Limits {
    /// The shipped ceilings.
    pub const DEFAULT: Self = Self {
        max_bytes: limits::MAX_CSS_BYTES,
        max_tokens: limits::MAX_CSS_TOKENS,
        max_rules: limits::MAX_CSS_RULES,
        max_declarations: limits::MAX_CSS_DECLARATIONS,
        max_selector_parts: limits::MAX_CSS_SELECTOR_PARTS,
        max_import_depth: limits::MAX_CSS_IMPORT_DEPTH,
        max_elements: limits::MAX_DOM_NODES,
        max_matches: limits::MAX_SELECTOR_MATCHES,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The four totals, spent across a whole book and never refunded.
///
/// It is one object rather than four counters passed around, because that is
/// what makes "across the book" structural instead of conventional: a caller
/// that parses forty stylesheets and cascades forty content documents makes
/// **one** of these, and the fortieth sheet cannot start from zero.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    limits: Limits,
    tokens: usize,
    rules: usize,
    declarations: usize,
    matches: usize,
}

impl Budget {
    /// A fresh budget under the given ceilings.
    pub fn new(limits: &Limits) -> Self {
        Self {
            limits: *limits,
            tokens: 0,
            rules: 0,
            declarations: 0,
            matches: 0,
        }
    }

    /// Charges tokens, all at once — a tokenizer knows its whole answer.
    pub fn spend_tokens(&mut self, count: usize) -> Result<(), Refusal> {
        self.tokens = self.tokens.saturating_add(count);
        if self.tokens > self.limits.max_tokens {
            return Err(Refusal::TooManyTokens {
                tokens: self.tokens,
            });
        }
        Ok(())
    }

    /// Charges one qualified rule.
    pub fn spend_rule(&mut self) -> Result<(), Refusal> {
        self.rules += 1;
        if self.rules > self.limits.max_rules {
            return Err(Refusal::TooManyRules { rules: self.rules });
        }
        Ok(())
    }

    /// Charges one declaration.
    pub fn spend_declaration(&mut self) -> Result<(), Refusal> {
        self.declarations += 1;
        if self.declarations > self.limits.max_declarations {
            return Err(Refusal::TooManyDeclarations {
                declarations: self.declarations,
            });
        }
        Ok(())
    }

    /// Charges one compound-against-element test.
    pub fn spend_match(&mut self) -> Result<(), Refusal> {
        self.matches += 1;
        if self.matches > self.limits.max_matches {
            return Err(Refusal::TooManySelectorMatches {
                matches: self.matches,
            });
        }
        Ok(())
    }

    /// Tokens spent so far.
    pub fn tokens(&self) -> usize {
        self.tokens
    }

    /// Rules admitted so far.
    pub fn rules(&self) -> usize {
        self.rules
    }

    /// Declarations admitted so far.
    pub fn declarations(&self) -> usize {
        self.declarations
    }

    /// Compound-against-element tests spent so far.
    pub fn matches(&self) -> usize {
        self.matches
    }
}

/// What this crate refuses, each by its own name.
///
/// `#[non_exhaustive]` for gap 29's reason: milestones 7 to 12 will add to it,
/// and a new variant should cost an addition rather than a breaking change.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// One stylesheet past [`limits::MAX_CSS_BYTES`].
    StylesheetTooLong {
        /// How long it was.
        bytes: usize,
    },
    /// The book's token total is spent.
    TooManyTokens {
        /// How many had been produced when it went over.
        tokens: usize,
    },
    /// The book's rule total is spent.
    TooManyRules {
        /// How many had been admitted when it went over.
        rules: usize,
    },
    /// The book's declaration total is spent.
    TooManyDeclarations {
        /// How many had been admitted when it went over.
        declarations: usize,
    },
    /// One content document past [`limits::MAX_DOM_NODES`].
    TooManyElements {
        /// How many were handed over.
        elements: usize,
    },
    /// The book's selector-matching total is spent.
    TooManySelectorMatches {
        /// How many tests had been made when it went over.
        matches: usize,
    },
    /// An element's parent does not precede it. A caller error rather than a
    /// hostile file, and refused by name because the alternative is a computed
    /// style read before it was written.
    NotInDocumentOrder {
        /// Which element.
        element: usize,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::StylesheetTooLong { bytes } => {
                write!(f, "a stylesheet of {bytes} bytes is past the cap")
            }
            Refusal::TooManyTokens { tokens } => {
                write!(f, "{tokens} CSS tokens is past the book's total")
            }
            Refusal::TooManyRules { rules } => {
                write!(f, "{rules} CSS rules is past the book's total")
            }
            Refusal::TooManyDeclarations { declarations } => {
                write!(f, "{declarations} declarations is past the book's total")
            }
            Refusal::TooManyElements { elements } => {
                write!(f, "{elements} elements in one document is past the cap")
            }
            Refusal::TooManySelectorMatches { matches } => {
                write!(f, "{matches} selector tests is past the book's total")
            }
            Refusal::NotInDocumentOrder { element } => {
                write!(f, "element {element} has a parent that does not precede it")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// What this crate could not honour, said out loud.
///
/// Ruling 10's shape: every one names the construct rather than saying
/// something went wrong, and [`parser::Report`] deduplicates them with a count
/// — a book with `float: left` on four hundred elements produces **one**
/// warning with the number four hundred beside it, not four hundred warnings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Warning {
    /// `@layer`, refused by name. `css-cascade-5` §6.1 sorts layers above
    /// specificity, so reading the block as ordinary rules would invert the
    /// cascade and dropping it silently would lose the rules inside.
    LayerRefused,
    /// An at-rule this build does not implement, by name.
    AtRuleUnsupported(String),
    /// An `@import` chain that comes back to a sheet already on the stack.
    /// **Refused rather than recursed**, which is not the same fact as
    /// [`Warning::ImportTooDeep`].
    ImportCycle,
    /// An `@import` past [`limits::MAX_CSS_IMPORT_DEPTH`].
    ImportTooDeep,
    /// An `@import` whose target the caller's resolver could not find.
    ImportUnresolved,
    /// An `@import` after a qualified rule, which `css-syntax-3` §3.3 makes
    /// invalid. The rules it names are not read.
    ImportOutOfOrder,
    /// A selector past [`limits::MAX_CSS_SELECTOR_PARTS`]; its rule is dropped.
    SelectorTooComplex,
    /// A pseudo-class or pseudo-element no specification this build cites
    /// defines. Its rule is dropped, per `selectors-4` §3.1.
    PseudoUnknown(String),
    /// A pseudo-class `selectors-4` defines that this build never matches —
    /// `:hover`, `:nth-child()` and their relatives — by name.
    PseudoClassUnsupported(&'static str),
    /// A pseudo-element this build parses and does not generate a box for. The
    /// rule matches nothing, which is the honest answer: applying it to the
    /// originating element would colour the paragraph instead of its marker.
    PseudoElementUnsupported(&'static str),
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::LayerRefused => f.write_str("@layer is refused by name"),
            Warning::AtRuleUnsupported(name) => write!(f, "@{name} is not implemented"),
            Warning::ImportCycle => f.write_str("an @import chain returns to a sheet it came from"),
            Warning::ImportTooDeep => f.write_str("an @import chain is past the depth cap"),
            Warning::ImportUnresolved => f.write_str("an @import names a sheet that is not there"),
            Warning::ImportOutOfOrder => {
                f.write_str("an @import follows a rule, which makes it invalid")
            }
            Warning::SelectorTooComplex => f.write_str("a selector is past the compound cap"),
            Warning::PseudoUnknown(name) => write!(f, "{name} is not a pseudo this build cites"),
            Warning::PseudoClassUnsupported(name) => write!(f, "{name} never matches here"),
            Warning::PseudoElementUnsupported(name) => write!(f, "{name} generates no box here"),
        }
    }
}
