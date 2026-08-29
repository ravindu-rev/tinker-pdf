//! `selectors-4`: §6.1–§6.4's simple selectors, §14's four combinators, §15's
//! specificity, and the matching that uses them.
//!
//! # Nothing here knows what XHTML is
//!
//! Ruling 8 says a leaf takes bytes and plain parameters. The element side of
//! matching arrives through [`crate::Element`], a trait the caller implements,
//! and the methods that look like HTML are the ones that make the crate *not*
//! know about it: `selectors-4` §6.6 says *"the ID attribute is defined by the
//! document language"* and §6.5 says the same of class, so asking the element
//! for its id and its classes is what keeps `id=` and `class=` out of this
//! crate. A matcher that read `attribute("class")` would have hard-coded XHTML
//! into a crate whose whole argument is that it has not.
//!
//! The same sentence decides every pseudo-class below whose meaning is the
//! document language's rather than the tree's. `:link` is not *"an `<a>` with
//! an `href`"* here, `:lang()` does not read `lang=`, and `:checked` does not
//! read `checked` — each asks the element a question ([`crate::Element::is_link`],
//! [`crate::Element::language`], [`crate::Element::ui_state`]) and the caller
//! answers it in the vocabulary of the language it parsed. What this crate
//! keeps is what it can see: the **tree**. `:lang()` and `:dir()` inherit by
//! walking `parent()` from here, because inheritance is a fact about the tree
//! and the attribute name is a fact about the language.
//!
//! # The two kinds of pseudo-class, and why one of them is an answer
//!
//! `selectors-4` defines pseudo-classes this build **evaluates** and seven it
//! **decides against**, and the second list is not a list of things that are
//! missing. `:hover`, `:focus`, `:focus-within`, `:focus-visible`, `:active`,
//! `:target` and `:visited` are states of a *reading session* — a pointer, a
//! focus ring, a fragment the reader navigated to, a history. A paginated
//! document has none of them, at any point, for any element, so
//! [`PseudoClass::NoSuchState`] returning `false` is the **right answer** and
//! not a shortcut. Everything else `selectors-4` defines and this build parses
//! is decided from the tree, the attributes and the document language: a
//! static document knows perfectly well which of its rows is even.
//!
//! The distinction is load-bearing because the two failures are opposite. A
//! build that never matched `:nth-child(2n)` would silently drop a rule the
//! book's author can see the effect of; a build that *did* match `:hover`
//! would invent a state no reader is ever in.
//!
//! # Case sensitivity, which is a decision and not an oversight
//!
//! Type names, class names, id names and attribute names are compared
//! **case-sensitively**, which is XML's rule and therefore the rule for an
//! XHTML content document — EPUB 3.3 §3.2 makes every one of them XML. A
//! stylesheet written against `text/html`'s ASCII-case-insensitive matching
//! will behave differently here, and that is the specification's answer rather
//! than this build's shortcut. Attribute **values** honour §6.3.6's `i` and `s`
//! flags, which is the one place the author gets to choose.
//!
//! # The three specificity rules a naive A/B/C gets wrong
//!
//! §15's tuple is easy until it is not, and each of these is a rule a plausible
//! implementation gets wrong in a way no ordinary stylesheet reveals:
//!
//! 1. **`:not()` contributes its argument's specificity, not its own.** So
//!    `:not(.a)` and `.a` are the *same* specificity, and a build that counted
//!    `:not()` as a pseudo-class would make it one step stronger — which only
//!    shows up when the two are in the same cascade.
//! 2. **`:is()` takes its most specific argument.** `:is(#x, p)` is
//!    `(1, 0, 0)`, not `(0, 0, 1)` and not the sum. `:where()` is always zero,
//!    which is the whole reason it exists.
//! 3. **A pseudo-element contributes to C**, like a type selector. `p::before`
//!    is `(0, 0, 2)`, and a build that treated `::before` as a pseudo-*class*
//!    would put it in B and beat every type selector with it.

use std::collections::HashMap;

use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::{HashKind, Token};
use crate::{Budget, Element, Refusal, Warning};

/// `selectors-4` §15's A/B/C tuple.
///
/// `Ord` is derived, and the derivation is the comparison the specification
/// asks for: the tuple is compared lexicographically, so no amount of B beats
/// one A. A build that packed it into a single number with a base — the classic
/// `a * 100 + b * 10 + c` — is wrong for any book with eleven classes on one
/// selector, and no book announces that it has one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    /// Id selectors.
    pub a: u32,
    /// Class selectors, attribute selectors and pseudo-classes.
    pub b: u32,
    /// Type selectors and pseudo-elements.
    pub c: u32,
}

impl Specificity {
    /// The zero tuple, which is what `*` and `:where()` contribute.
    pub const ZERO: Self = Self { a: 0, b: 0, c: 0 };

    fn plus(self, other: Self) -> Self {
        Self {
            a: self.a + other.a,
            b: self.b + other.b,
            c: self.c + other.c,
        }
    }
}

/// §14's four combinators. There is no fifth in `selectors-4`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    /// `a b`
    Descendant,
    /// `a > b`
    Child,
    /// `a + b`
    NextSibling,
    /// `a ~ b`
    SubsequentSibling,
}

/// §6.3's attribute matchers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeMatch {
    /// `[href]`
    Exists,
    /// `[href="x"]`
    Equals(String),
    /// `[class~="x"]`, whitespace-separated word.
    Includes(String),
    /// `[lang|="en"]`, exact or followed by `-`.
    DashMatch(String),
    /// `[href^="x"]`
    Prefix(String),
    /// `[href$="x"]`
    Suffix(String),
    /// `[href*="x"]`
    Substring(String),
}

/// One `[…]` selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeSelector {
    /// The attribute's name, compared case-sensitively (XML's rule).
    pub name: String,
    /// What must be true of its value.
    pub matcher: AttributeMatch,
    /// §6.3.6's `i` flag. `s` is the default and is accepted explicitly.
    pub case_insensitive: bool,
}

/// `selectors-4` §6.6.2's `An+B`, parsed once at parse time.
///
/// `A` and `B` are held apart rather than folded into a modulus, because the
/// two edges of the microsyntax are `A == 0` — which is not a step at all but
/// the single position `B` — and a **negative** `A`, where `-n+3` counts the
/// first three and `3n-2` does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nth {
    /// The step. May be zero (a single position) or negative (a countdown).
    pub a: i64,
    /// The offset, which §6.6.2 counts **from one**.
    pub b: i64,
}

impl Nth {
    /// `:nth-child(odd)`, which §6.6.2 defines as `2n+1`.
    pub const ODD: Self = Self { a: 2, b: 1 };
    /// `:nth-child(even)`, which is `2n`.
    pub const EVEN: Self = Self { a: 2, b: 0 };

    /// Does a **one-based** position match?
    ///
    /// §6.6.2: `An+B` matches position `p` when `p = An + B` for some
    /// non-negative integer `n`. The non-negativity is the half a plain
    /// remainder test drops, and dropping it makes `:nth-child(-n+3)` match
    /// every element rather than the first three.
    #[must_use]
    pub fn contains(self, position: i64) -> bool {
        let offset = position - self.b;
        if self.a == 0 {
            return offset == 0;
        }
        offset % self.a == 0 && offset / self.a >= 0
    }
}

/// One member of a `:has()` argument: §4.2's *relative* selector.
///
/// A relative selector is a selector with a combinator in front of it, and the
/// combinator joins it to `:scope` — the element `:has()` is being tested
/// against. `:has(.a)` is `:has(:scope .a)`, which is why an argument written
/// without a combinator is a **descendant** relation and not a bare match:
/// `p:has(.a .b)` is false when the `.a` is outside the `p`, and a build that
/// simply matched `.a .b` against every descendant would say it is true.
#[derive(Clone, Debug, PartialEq)]
pub struct Relative {
    /// The combinator between `:scope` and the selector's leftmost compound.
    pub combinator: Combinator,
    /// The selector, whose **rightmost** compound is still the subject — it is
    /// the element `:has()` is looking for, not the element being styled.
    pub selector: Selector,
}

/// The document language's user-interface states for one element.
///
/// `selectors-4` §12 defines these pseudo-classes and defers every one of them
/// to the document language: HTML says which elements can be disabled, what
/// `checked` means and which of them are editable, and this crate must not.
/// So the whole of §12 arrives through one value the caller fills in.
///
/// **Three of the four fields are `Option<bool>` and that is §12's own shape,
/// not indecision.** `:enabled` is not the negation of `:disabled`: a `<p>` is
/// neither, because §12.3 scopes both to elements the document language says
/// can be activated at all. A build that made one the negation of the other
/// would match `p:enabled` against every paragraph in the book.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiState {
    /// §12.2's `:checked`.
    pub checked: bool,
    /// `Some(true)` for `:disabled`, `Some(false)` for `:enabled`, `None` for
    /// an element the document language says is neither — §12.3.
    pub disabled: Option<bool>,
    /// `Some(true)` for `:required`, `Some(false)` for `:optional`, `None` for
    /// an element that takes no value — §12.5.
    pub required: Option<bool>,
    /// `Some(true)` for `:read-only`, `Some(false)` for `:read-write`, `None`
    /// for an element the document language does not classify — §12.4.
    pub read_only: Option<bool>,
}

impl UiState {
    /// An element with none of §12's states, which is what a caller with no
    /// forms in its document language says about everything.
    pub const NONE: Self = Self {
        checked: false,
        disabled: None,
        required: None,
        read_only: None,
    };
}

/// A pseudo-class, split by whether the state it names exists here at all.
#[derive(Clone, Debug, PartialEq)]
pub enum PseudoClass {
    /// `:root`
    Root,
    /// `:first-child`
    FirstChild,
    /// `:last-child`
    LastChild,
    /// `:only-child`
    OnlyChild,
    /// `:empty`, §6.6.3 — answered by [`crate::Element::is_empty`], because
    /// what counts as a child node is the document language's question.
    Empty,
    /// `:first-of-type`
    FirstOfType,
    /// `:last-of-type`
    LastOfType,
    /// `:only-of-type`
    OnlyOfType,
    /// `:nth-child(…)`
    NthChild(Nth),
    /// `:nth-last-child(…)`
    NthLastChild(Nth),
    /// `:nth-of-type(…)`
    NthOfType(Nth),
    /// `:nth-last-of-type(…)`
    NthLastOfType(Nth),
    /// `:lang(…)`, §6.5.1: one or more language **ranges**, matched by
    /// RFC 4647 §3.3.2's extended filtering against the element's language.
    Lang(Vec<String>),
    /// `:dir(…)`, §6.6: a directionality keyword, lower-cased.
    Dir(String),
    /// `:not(…)`
    Not(Vec<Selector>),
    /// `:is(…)`, and `:matches()`/`:any()` under their old names.
    Is(Vec<Selector>),
    /// `:where(…)`, which is `:is()` at zero specificity.
    Where(Vec<Selector>),
    /// `:has(…)`, §4.2's relational pseudo-class.
    Has(Vec<Relative>),
    /// `:link` and `:any-link`, §6.6.1.
    ///
    /// **One variant for two names**, because in a document with no history
    /// they are the same set: §6.6.1 splits `:any-link` into `:link` and
    /// `:visited` on whether the reading system has been there before, and a
    /// PDF page has been nowhere. `:visited` is therefore [`Self::NoSuchState`]
    /// and `:link` is every hyperlink source.
    Link,
    /// `:checked`, §12.2.
    Checked,
    /// `:disabled`, §12.3.
    Disabled,
    /// `:enabled`, §12.3 — **not** the negation of `:disabled`.
    Enabled,
    /// `:required`, §12.5.
    Required,
    /// `:optional`, §12.5.
    Optional,
    /// `:read-only`, §12.4.
    ReadOnly,
    /// `:read-write`, §12.4.
    ReadWrite,
    /// A pseudo-class naming a state a paginated document does not have.
    ///
    /// **It never matches, and that is the answer rather than a refusal.**
    /// `:hover` needs a pointer, `:focus` and its two relatives need a focus
    /// ring, `:active` needs a press, `:target` needs a fragment the reader
    /// navigated to and `:visited` needs a history. None of the five things
    /// exists in a PDF page, for any element, ever — so `false` is what
    /// `selectors-4` says the answer is here, not what this build managed.
    ///
    /// It is still **counted and named**, because a book that styles `:hover`
    /// has said something that had no effect and ruling 10 says so out loud.
    /// The alternative — treating the name as invalid — is worse in a specific
    /// way: it would take `a:hover, a:link { … }` down with it, because §3.1
    /// invalidates a whole list for one bad member.
    NoSuchState(&'static str),
}

/// `selectors-4` §7's pseudo-elements, to the four a book uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PseudoElement {
    /// `::before`
    Before,
    /// `::after`
    After,
    /// `::first-line`
    FirstLine,
    /// `::first-letter`
    FirstLetter,
}

impl PseudoElement {
    /// The name, for a warning to carry.
    pub fn name(self) -> &'static str {
        match self {
            PseudoElement::Before => "::before",
            PseudoElement::After => "::after",
            PseudoElement::FirstLine => "::first-line",
            PseudoElement::FirstLetter => "::first-letter",
        }
    }
}

/// One compound selector: everything between two combinators.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compound {
    /// A type selector, or `None` for the universal selector or none at all.
    pub type_name: Option<String>,
    /// `#id`. §6.6 allows several and a build that kept only one would match
    /// `#a#b` against an element with `id="a"`.
    pub ids: Vec<String>,
    /// `.class`
    pub classes: Vec<String>,
    /// `[…]`
    pub attributes: Vec<AttributeSelector>,
    /// `:…`
    pub pseudo_classes: Vec<PseudoClass>,
}

/// A complex selector: compounds left to right, and the combinators between.
#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    /// Leftmost first. The rightmost is the subject.
    pub compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` to `compounds[i + 1]`, so there is
    /// always exactly one fewer of these than of those.
    pub combinators: Vec<Combinator>,
    /// A trailing pseudo-element, which makes this selector address something
    /// the element does not have yet.
    pub pseudo_element: Option<PseudoElement>,
    /// §15's tuple, computed once at parse time.
    pub specificity: Specificity,
}

/// Why a selector list would not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Past [`crate::limits::MAX_CSS_SELECTOR_PARTS`].
    TooManyParts,
    /// A pseudo-class or pseudo-element no specification this build cites
    /// defines. §3.1 makes the whole list invalid, and the rule with it.
    UnknownPseudo(String),
    /// Anything else the grammar rejects: a stray combinator, a namespace
    /// separator, an empty selector between two commas.
    Malformed,
}

/// Parses a selector list from a qualified rule's prelude.
///
/// §3.1: *"if any of the selectors in the list is invalid, the whole list is
/// invalid"* — so this returns `Err` for the rule rather than the selectors
/// that happened to parse. A build that kept the good half would apply a rule
/// its author scoped to something else.
pub fn parse_list(prelude: &[ComponentValue], max_parts: usize) -> Result<Vec<Selector>, Invalid> {
    let mut out = Vec::new();
    for group in prelude.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        out.push(parse_one(group, max_parts)?);
    }
    if out.is_empty() {
        return Err(Invalid::Malformed);
    }
    Ok(out)
}

/// One complex selector.
///
/// The shape is compound-then-combinator rather than one flat scan, because a
/// flat scan has to remember whether the whitespace it just passed was a
/// descendant combinator or the padding around a `>` — and that memory is
/// exactly where `a > b` becomes two combinators for one pair of compounds.
fn parse_one(values: &[ComponentValue], max_parts: usize) -> Result<Selector, Invalid> {
    let mut compounds: Vec<Compound> = Vec::new();
    let mut combinators: Vec<Combinator> = Vec::new();
    let mut pseudo_element: Option<PseudoElement> = None;
    let mut specificity = Specificity::ZERO;
    let mut at = 0usize;

    // Leading whitespace is not a descendant combinator.
    while at < values.len() && values[at].is_whitespace() {
        at += 1;
    }
    if at >= values.len() {
        return Err(Invalid::Malformed);
    }

    loop {
        let mut compound = Compound::default();
        let mut any = false;
        while at < values.len() && pseudo_element.is_none() {
            match &values[at] {
                ComponentValue::Token(Token::Whitespace) => break,
                ComponentValue::Token(Token::Delim('>' | '+' | '~')) => break,
                _ => {
                    let consumed = parse_simple(
                        values,
                        at,
                        &mut compound,
                        &mut specificity,
                        &mut pseudo_element,
                        max_parts,
                    )?;
                    at += consumed;
                    any = true;
                }
            }
        }
        if !any {
            // An empty compound: a leading or doubled combinator, which is
            // `selectors-4`'s relative-selector syntax and only `:has()` may
            // use it.
            return Err(Invalid::Malformed);
        }
        compounds.push(compound);

        let mut saw_space = false;
        while at < values.len() && values[at].is_whitespace() {
            saw_space = true;
            at += 1;
        }
        if at >= values.len() {
            break;
        }
        // Nothing may follow a pseudo-element: it is the subject and it is the
        // end. `p::before span` addresses nothing.
        if pseudo_element.is_some() {
            return Err(Invalid::Malformed);
        }
        let combinator = match &values[at] {
            ComponentValue::Token(Token::Delim('>')) => {
                at += 1;
                Combinator::Child
            }
            ComponentValue::Token(Token::Delim('+')) => {
                at += 1;
                Combinator::NextSibling
            }
            ComponentValue::Token(Token::Delim('~')) => {
                at += 1;
                Combinator::SubsequentSibling
            }
            // Whitespace on its own, and only then: an explicit combinator
            // *replaces* the whitespace around it rather than adding to it.
            _ if saw_space => Combinator::Descendant,
            _ => return Err(Invalid::Malformed),
        };
        combinators.push(combinator);
        while at < values.len() && values[at].is_whitespace() {
            at += 1;
        }
        if at >= values.len() {
            // A trailing combinator: `a >`.
            return Err(Invalid::Malformed);
        }
        if compounds.len() >= max_parts {
            return Err(Invalid::TooManyParts);
        }
    }

    if combinators.len() + 1 != compounds.len() {
        return Err(Invalid::Malformed);
    }
    if compounds.len() > max_parts {
        return Err(Invalid::TooManyParts);
    }
    Ok(Selector {
        compounds,
        combinators,
        pseudo_element,
        specificity,
    })
}

/// One simple selector, returning how many component values it ate.
fn parse_simple(
    values: &[ComponentValue],
    at: usize,
    compound: &mut Compound,
    specificity: &mut Specificity,
    pseudo_element: &mut Option<PseudoElement>,
    max_parts: usize,
) -> Result<usize, Invalid> {
    match &values[at] {
        ComponentValue::Token(Token::Ident(name)) => {
            if compound.type_name.is_some() {
                return Err(Invalid::Malformed);
            }
            compound.type_name = Some(name.clone());
            specificity.c += 1;
            Ok(1)
        }
        ComponentValue::Token(Token::Delim('*')) => {
            // The universal selector contributes nothing to specificity, which
            // is §15's own sentence and the one place a "count everything"
            // implementation is wrong in the harmless direction.
            Ok(1)
        }
        ComponentValue::Token(Token::Delim('|')) => Err(Invalid::Malformed),
        ComponentValue::Token(Token::Hash(name, HashKind::Id)) => {
            compound.ids.push(name.clone());
            specificity.a += 1;
            Ok(1)
        }
        ComponentValue::Token(Token::Hash(_, HashKind::Unrestricted)) => Err(Invalid::Malformed),
        ComponentValue::Token(Token::Delim('.')) => {
            let Some(ComponentValue::Token(Token::Ident(name))) = values.get(at + 1) else {
                return Err(Invalid::Malformed);
            };
            compound.classes.push(name.clone());
            specificity.b += 1;
            Ok(2)
        }
        ComponentValue::Block {
            kind: BlockKind::Square,
            values: inner,
        } => {
            compound.attributes.push(parse_attribute(inner)?);
            specificity.b += 1;
            Ok(1)
        }
        ComponentValue::Token(Token::Colon) => {
            let double = matches!(
                values.get(at + 1),
                Some(ComponentValue::Token(Token::Colon))
            );
            let name_at = if double { at + 2 } else { at + 1 };
            match values.get(name_at) {
                Some(ComponentValue::Token(Token::Ident(name))) => {
                    let lower = name.to_ascii_lowercase();
                    if let Some(element) = pseudo_element_named(&lower) {
                        // `:before` with one colon is CSS 2.1's spelling and
                        // real books use it. It is the same pseudo-element and
                        // it contributes to C either way.
                        *pseudo_element = Some(element);
                        specificity.c += 1;
                        return Ok(name_at - at + 1);
                    }
                    if double {
                        return Err(Invalid::UnknownPseudo(format!("::{lower}")));
                    }
                    let class = simple_pseudo_class(&lower)
                        .ok_or_else(|| Invalid::UnknownPseudo(format!(":{lower}")))?;
                    compound.pseudo_classes.push(class);
                    specificity.b += 1;
                    Ok(name_at - at + 1)
                }
                Some(ComponentValue::Function { name, arguments }) => {
                    if double {
                        return Err(Invalid::UnknownPseudo(format!(
                            "::{}",
                            name.to_ascii_lowercase()
                        )));
                    }
                    let lower = name.to_ascii_lowercase();
                    let (class, contribution) =
                        functional_pseudo_class(&lower, arguments, max_parts)?;
                    compound.pseudo_classes.push(class);
                    *specificity = specificity.plus(contribution);
                    Ok(name_at - at + 1)
                }
                _ => Err(Invalid::Malformed),
            }
        }
        _ => Err(Invalid::Malformed),
    }
}

fn pseudo_element_named(name: &str) -> Option<PseudoElement> {
    match name {
        "before" => Some(PseudoElement::Before),
        "after" => Some(PseudoElement::After),
        "first-line" => Some(PseudoElement::FirstLine),
        "first-letter" => Some(PseudoElement::FirstLetter),
        _ => None,
    }
}

/// Every pseudo-class this build recognises without arguments.
///
/// All but seven are **evaluated**. The seven are the states of a reading
/// session — a pointer, a focus ring, a press, a fragment, a history — and
/// [`PseudoClass::NoSuchState`] is where the argument for each of them lives.
///
/// The list is closed on purpose: a name that is not here is
/// [`Invalid::UnknownPseudo`] and takes its rule down, because a selector
/// nothing recognises is a selector from a specification this build does not
/// cite, and guessing at it is how a cascade acquires a rule its author never
/// wrote.
fn simple_pseudo_class(name: &str) -> Option<PseudoClass> {
    Some(match name {
        "root" => PseudoClass::Root,
        "first-child" => PseudoClass::FirstChild,
        "last-child" => PseudoClass::LastChild,
        "only-child" => PseudoClass::OnlyChild,
        "empty" => PseudoClass::Empty,
        "first-of-type" => PseudoClass::FirstOfType,
        "last-of-type" => PseudoClass::LastOfType,
        "only-of-type" => PseudoClass::OnlyOfType,
        // §6.6.1: with no history, `:link` and `:any-link` are one set.
        "link" | "any-link" => PseudoClass::Link,
        "checked" => PseudoClass::Checked,
        "disabled" => PseudoClass::Disabled,
        "enabled" => PseudoClass::Enabled,
        "required" => PseudoClass::Required,
        "optional" => PseudoClass::Optional,
        "read-only" => PseudoClass::ReadOnly,
        "read-write" => PseudoClass::ReadWrite,
        // The seven, and the whole of the seven.
        "hover" => PseudoClass::NoSuchState(":hover"),
        "focus" => PseudoClass::NoSuchState(":focus"),
        "focus-within" => PseudoClass::NoSuchState(":focus-within"),
        "focus-visible" => PseudoClass::NoSuchState(":focus-visible"),
        "active" => PseudoClass::NoSuchState(":active"),
        "target" => PseudoClass::NoSuchState(":target"),
        "visited" => PseudoClass::NoSuchState(":visited"),
        _ => return None,
    })
}

/// A functional pseudo-class, and what it contributes to specificity.
fn functional_pseudo_class(
    name: &str,
    arguments: &[ComponentValue],
    max_parts: usize,
) -> Result<(PseudoClass, Specificity), Invalid> {
    match name {
        // §15's rule, and the one a naive implementation gets wrong: the
        // *argument's* specificity, so `:not(.a)` and `.a` are equal.
        "not" => {
            let list = parse_list(arguments, max_parts)?;
            let most = most_specific(&list);
            Ok((PseudoClass::Not(list), most))
        }
        "is" | "matches" | "any" => {
            let list = parse_list(arguments, max_parts)?;
            let most = most_specific(&list);
            Ok((PseudoClass::Is(list), most))
        }
        // `:where()` is `:is()` at zero, which is the whole reason it exists.
        "where" => {
            let list = parse_list(arguments, max_parts)?;
            Ok((PseudoClass::Where(list), Specificity::ZERO))
        }
        // §15 gives `:has()` its most specific argument too.
        "has" => {
            let list = parse_relative_list(arguments, max_parts)?;
            let most = list
                .iter()
                .map(|relative| relative.selector.specificity)
                .max()
                .unwrap_or(Specificity::ZERO);
            Ok((PseudoClass::Has(list), most))
        }
        "nth-child" => nth_function(arguments, PseudoClass::NthChild),
        "nth-last-child" => nth_function(arguments, PseudoClass::NthLastChild),
        "nth-of-type" => nth_function(arguments, PseudoClass::NthOfType),
        "nth-last-of-type" => nth_function(arguments, PseudoClass::NthLastOfType),
        "lang" => Ok((PseudoClass::Lang(parse_lang(arguments)?), ONE_B)),
        "dir" => Ok((PseudoClass::Dir(parse_dir(arguments)?), ONE_B)),
        other => Err(Invalid::UnknownPseudo(format!(":{other}()"))),
    }
}

/// What one pseudo-class contributes to §15's B.
const ONE_B: Specificity = Specificity { a: 0, b: 1, c: 0 };

fn nth_function(
    arguments: &[ComponentValue],
    wrap: fn(Nth) -> PseudoClass,
) -> Result<(PseudoClass, Specificity), Invalid> {
    Ok((wrap(parse_nth(arguments)?), ONE_B))
}

/// §6.5.1's argument: one or more comma-separated language **ranges**.
///
/// A range is an identifier, a string, or a run of identifiers and `*` delims
/// that the tokenizer split — `*-CH` is `<delim *>` then `<ident -CH>`, and
/// gluing them back together here is the only way to read RFC 4647's wildcard
/// at all.
fn parse_lang(arguments: &[ComponentValue]) -> Result<Vec<String>, Invalid> {
    let mut out = Vec::new();
    for group in arguments.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        let mut range = String::new();
        for value in group.iter().filter(|v| !v.is_whitespace()) {
            match value {
                ComponentValue::Token(Token::Ident(text)) => range.push_str(text),
                // A quoted range is one token and cannot be glued to another.
                ComponentValue::Token(Token::Str(text)) if range.is_empty() => {
                    range.push_str(text);
                }
                ComponentValue::Token(Token::Delim('*')) => range.push('*'),
                _ => return Err(Invalid::Malformed),
            }
        }
        if range.is_empty() {
            return Err(Invalid::Malformed);
        }
        out.push(range);
    }
    if out.is_empty() {
        return Err(Invalid::Malformed);
    }
    Ok(out)
}

/// §6.6's argument: one directionality keyword, which CSS compares
/// ASCII-case-insensitively like every other keyword.
fn parse_dir(arguments: &[ComponentValue]) -> Result<String, Invalid> {
    let values: Vec<&ComponentValue> = arguments.iter().filter(|v| !v.is_whitespace()).collect();
    match values.as_slice() {
        [ComponentValue::Token(Token::Ident(name))] => Ok(name.to_ascii_lowercase()),
        _ => Err(Invalid::Malformed),
    }
}

/// What follows the `n` of an `An+B` identifier or dimension unit.
enum NTail {
    /// `n`: whatever `B` there is has not been tokenized yet.
    Bare,
    /// `n-`: a signless integer follows and `B` is its negation. `2n- 1` is
    /// `2n-1`, and it is a separate production because `n-` is a perfectly
    /// ordinary identifier and the tokenizer has already eaten the sign.
    DashPending,
    /// `n-3`: `B` arrived inside the identifier, because `-3` is ident code
    /// points. **This is where a hand-written `An+B` parser goes wrong**:
    /// `2n-3` is *one* dimension token whose unit is `n-3`, not three tokens.
    Complete(i64),
}

fn n_tail(rest: &str) -> Option<NTail> {
    if rest.is_empty() {
        return Some(NTail::Bare);
    }
    let digits = rest.strip_prefix('-')?;
    if digits.is_empty() {
        return Some(NTail::DashPending);
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // A range check rather than a wrap: the position this is compared against
    // is bounded by the element count, so anything past `i32` can only ever
    // fail to match, and holding it in `i64` keeps `position - b` from
    // overflowing (ruling 1 — a stylesheet is untrusted).
    let value: i64 = digits.parse().ok()?;
    if value > i64::from(i32::MAX) {
        return None;
    }
    Some(NTail::Complete(-value))
}

/// `css-syntax-3` §9's `An+B` microsyntax.
///
/// The whole difficulty is that the tokenizer has already made decisions: `2n`
/// is a `<dimension-token>`, `-n` is an `<ident-token>`, `2n-3` is a dimension
/// whose *unit* is `n-3`, and `2n - 3` is three tokens. Every one of those is
/// the same selector.
///
/// **One leniency, named rather than hidden.** This tokenizer's
/// `<number-token>` keeps its value and its integer flag but not whether the
/// source wrote a sign, so `:nth-child(2n 1)` — which §9 rejects, because a
/// signless integer may not follow a whitespace — is read here as `2n+1`. The
/// alternative is a token representation this crate has no other use for.
fn parse_nth(arguments: &[ComponentValue]) -> Result<Nth, Invalid> {
    let values: Vec<&ComponentValue> = arguments.iter().filter(|v| !v.is_whitespace()).collect();
    let (first, rest) = values.split_first().ok_or(Invalid::Malformed)?;
    // `+n+3`: a lone `+` in front, which is a delim because `+n` does not start
    // a number.
    if matches!(first, ComponentValue::Token(Token::Delim('+'))) {
        let (second, rest) = rest.split_first().ok_or(Invalid::Malformed)?;
        let ComponentValue::Token(Token::Ident(name)) = second else {
            return Err(Invalid::Malformed);
        };
        return finish_nth(1, ident_tail(&name.to_ascii_lowercase())?, rest);
    }
    match first {
        ComponentValue::Token(Token::Ident(name)) => {
            let lower = name.to_ascii_lowercase();
            // §9: `odd` and `even` are the microsyntax's own keywords, and
            // they are `2n+1` and `2n` rather than a special case downstream.
            if lower == "odd" && rest.is_empty() {
                return Ok(Nth::ODD);
            }
            if lower == "even" && rest.is_empty() {
                return Ok(Nth::EVEN);
            }
            let (sign, tail) = match lower.strip_prefix('-') {
                Some(after) => (-1, ident_tail(after)?),
                None => (1, ident_tail(&lower)?),
            };
            finish_nth(sign, tail, rest)
        }
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            let a = integer(*value).ok_or(Invalid::Malformed)?;
            let unit = unit.to_ascii_lowercase();
            let after = unit.strip_prefix('n').ok_or(Invalid::Malformed)?;
            finish_nth(a, n_tail(after).ok_or(Invalid::Malformed)?, rest)
        }
        // `:nth-child(3)`: no `n` at all, so `A` is zero and the selector
        // names one position.
        ComponentValue::Token(Token::Number {
            value,
            integer: true,
        }) if rest.is_empty() => Ok(Nth {
            a: 0,
            b: integer(*value).ok_or(Invalid::Malformed)?,
        }),
        _ => Err(Invalid::Malformed),
    }
}

/// The `n…` part of an identifier, with any leading `-` already taken.
fn ident_tail(lower: &str) -> Result<NTail, Invalid> {
    let after = lower.strip_prefix('n').ok_or(Invalid::Malformed)?;
    n_tail(after).ok_or(Invalid::Malformed)
}

/// Reads whatever `B` the tokens after the `n` still owe.
fn finish_nth(a: i64, tail: NTail, rest: &[&ComponentValue]) -> Result<Nth, Invalid> {
    match tail {
        NTail::Complete(b) => {
            if rest.is_empty() {
                Ok(Nth { a, b })
            } else {
                Err(Invalid::Malformed)
            }
        }
        NTail::DashPending => match rest {
            [ComponentValue::Token(Token::Number {
                value,
                integer: true,
            })] => {
                let b = integer(*value).ok_or(Invalid::Malformed)?;
                // `2n- -1` is not a thing: the sign was the `n-`.
                if b < 0 {
                    return Err(Invalid::Malformed);
                }
                Ok(Nth { a, b: -b })
            }
            _ => Err(Invalid::Malformed),
        },
        NTail::Bare => match rest {
            [] => Ok(Nth { a, b: 0 }),
            [ComponentValue::Token(Token::Number {
                value,
                integer: true,
            })] => Ok(Nth {
                a,
                b: integer(*value).ok_or(Invalid::Malformed)?,
            }),
            [ComponentValue::Token(Token::Delim(sign @ ('+' | '-'))), ComponentValue::Token(Token::Number {
                value,
                integer: true,
            })] => {
                let b = integer(*value).ok_or(Invalid::Malformed)?;
                // After an explicit sign the integer must be signless: `2n+-1`
                // is not `2n-1`.
                if b < 0 {
                    return Err(Invalid::Malformed);
                }
                Ok(Nth {
                    a,
                    b: if *sign == '-' { -b } else { b },
                })
            }
            _ => Err(Invalid::Malformed),
        },
    }
}

/// An integer `An+B` coefficient, bounded so the arithmetic downstream cannot
/// overflow whatever a stylesheet writes.
fn integer(value: f64) -> Option<i64> {
    if !value.is_finite() || value != value.trunc() {
        return None;
    }
    if value.abs() > f64::from(i32::MAX) {
        return None;
    }
    Some(value as i64)
}

/// §4.2's relative selector list, which is what `:has()` takes.
///
/// The one thing it does that [`parse_list`] must not is accept a **leading
/// combinator**: `:has(> li)` is legal and `> li` on its own is not.
fn parse_relative_list(
    arguments: &[ComponentValue],
    max_parts: usize,
) -> Result<Vec<Relative>, Invalid> {
    let mut out = Vec::new();
    for group in arguments.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        let mut at = 0usize;
        while at < group.len() && group[at].is_whitespace() {
            at += 1;
        }
        let combinator = match group.get(at) {
            Some(ComponentValue::Token(Token::Delim('>'))) => {
                at += 1;
                Combinator::Child
            }
            Some(ComponentValue::Token(Token::Delim('+'))) => {
                at += 1;
                Combinator::NextSibling
            }
            Some(ComponentValue::Token(Token::Delim('~'))) => {
                at += 1;
                Combinator::SubsequentSibling
            }
            // §4.2: an argument with no combinator is `:scope <descendant>`,
            // which is why `p:has(.a)` asks about the paragraph's subtree and
            // not about the document.
            _ => Combinator::Descendant,
        };
        out.push(Relative {
            combinator,
            selector: parse_one(&group[at..], max_parts)?,
        });
    }
    if out.is_empty() {
        return Err(Invalid::Malformed);
    }
    Ok(out)
}

/// The most specific of a selector list, which is what §15 says `:is()`,
/// `:not()` and `:has()` each contribute.
fn most_specific(list: &[Selector]) -> Specificity {
    list.iter()
        .map(|s| s.specificity)
        .max()
        .unwrap_or(Specificity::ZERO)
}

/// §6.3's grammar inside `[…]`.
fn parse_attribute(inner: &[ComponentValue]) -> Result<AttributeSelector, Invalid> {
    let values: Vec<&ComponentValue> = inner.iter().filter(|v| !v.is_whitespace()).collect();
    let Some(ComponentValue::Token(Token::Ident(name))) = values.first() else {
        return Err(Invalid::Malformed);
    };
    if values.len() == 1 {
        return Ok(AttributeSelector {
            name: name.clone(),
            matcher: AttributeMatch::Exists,
            case_insensitive: false,
        });
    }
    // `~=`, `|=`, `^=`, `$=` and `*=` are two tokens; `=` is one.
    let (operator, value_at) = match values.get(1) {
        Some(ComponentValue::Token(Token::Delim('='))) => (None, 2),
        Some(ComponentValue::Token(Token::Delim(c @ ('~' | '|' | '^' | '$' | '*')))) => {
            if !matches!(
                values.get(2),
                Some(ComponentValue::Token(Token::Delim('=')))
            ) {
                return Err(Invalid::Malformed);
            }
            (Some(*c), 3)
        }
        _ => return Err(Invalid::Malformed),
    };
    let value = match values.get(value_at) {
        Some(ComponentValue::Token(Token::Str(text))) => text.clone(),
        Some(ComponentValue::Token(Token::Ident(text))) => text.clone(),
        _ => return Err(Invalid::Malformed),
    };
    let case_insensitive = match values.get(value_at + 1) {
        None => false,
        Some(ComponentValue::Token(Token::Ident(flag))) if flag.eq_ignore_ascii_case("i") => true,
        Some(ComponentValue::Token(Token::Ident(flag))) if flag.eq_ignore_ascii_case("s") => false,
        Some(_) => return Err(Invalid::Malformed),
    };
    if values.len() > value_at + 2 {
        return Err(Invalid::Malformed);
    }
    let matcher = match operator {
        None => AttributeMatch::Equals(value),
        Some('~') => AttributeMatch::Includes(value),
        Some('|') => AttributeMatch::DashMatch(value),
        Some('^') => AttributeMatch::Prefix(value),
        Some('$') => AttributeMatch::Suffix(value),
        Some('*') => AttributeMatch::Substring(value),
        Some(_) => return Err(Invalid::Malformed),
    };
    Ok(AttributeSelector {
        name: name.clone(),
        matcher,
        case_insensitive,
    })
}

/// The warnings a parsed selector list owes: one per pseudo-class naming a
/// state this document does not have, and one per pseudo-element, each named.
pub fn warnings(selectors: &[Selector]) -> Vec<Warning> {
    let mut out = Vec::new();
    for selector in selectors {
        if let Some(element) = selector.pseudo_element {
            out.push(Warning::PseudoElementUnsupported(element.name()));
        }
        for compound in &selector.compounds {
            collect_stateless(&compound.pseudo_classes, &mut out);
        }
    }
    out
}

/// The arms are written out rather than swept up with a `_`, and that is the
/// device that keeps this honest: a pseudo-class added to the enum without a
/// decision about whether it warns **does not compile**.
fn collect_stateless(classes: &[PseudoClass], out: &mut Vec<Warning>) {
    for class in classes {
        match class {
            PseudoClass::NoSuchState(name) => out.push(Warning::PseudoClassUnsupported(name)),
            PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
                for selector in list {
                    for compound in &selector.compounds {
                        collect_stateless(&compound.pseudo_classes, out);
                    }
                }
            }
            PseudoClass::Has(list) => {
                for relative in list {
                    for compound in &relative.selector.compounds {
                        collect_stateless(&compound.pseudo_classes, out);
                    }
                }
            }
            PseudoClass::Root
            | PseudoClass::FirstChild
            | PseudoClass::LastChild
            | PseudoClass::OnlyChild
            | PseudoClass::Empty
            | PseudoClass::FirstOfType
            | PseudoClass::LastOfType
            | PseudoClass::OnlyOfType
            | PseudoClass::NthChild(_)
            | PseudoClass::NthLastChild(_)
            | PseudoClass::NthOfType(_)
            | PseudoClass::NthLastOfType(_)
            | PseudoClass::Lang(_)
            | PseudoClass::Dir(_)
            | PseudoClass::Link
            | PseudoClass::Checked
            | PseudoClass::Disabled
            | PseudoClass::Enabled
            | PseudoClass::Required
            | PseudoClass::Optional
            | PseudoClass::ReadOnly
            | PseudoClass::ReadWrite => {}
        }
    }
}

// ---- matching ---------------------------------------------------------------

/// Does this selector match `index` in a document-ordered element slice?
///
/// Every **compound**-against-element test is charged to the budget, not every
/// selector-against-element attempt: `a b c d` against a deep tree costs
/// `O(depth^3)` compound tests, so charging the outer loop would bound a number
/// that is not the work. See [`crate::limits::MAX_SELECTOR_MATCHES`].
///
/// # The amendment the structural pseudo-classes needed
///
/// A **tree step** is charged too: every sibling a `:nth-child()` counts past,
/// every ancestor a `:has()` walks, every element a `:has()` scans. The reason
/// is the sentence above one level further down — `tr:nth-child(2n)` against a
/// table of ten thousand rows costs ten thousand sibling steps *per row*, so a
/// cap that counted only the compound would bound a number that is not the
/// work. It doubles as the cycle guard: a caller that hands over a slice whose
/// `parent()` links loop gets a [`Refusal`] rather than a hang, which is
/// ruling 1 for a caller error instead of for a file.
pub fn matches<E: Element>(
    selector: &Selector,
    elements: &[E],
    index: usize,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    // A rule whose subject is a pseudo-element does not style the element it is
    // attached to. Applying it there is the plausible wrong answer: `p::before
    // { color: red }` would colour the paragraph.
    if selector.pseudo_element.is_some() {
        return Ok(false);
    }
    match_from(
        selector,
        selector.compounds.len() - 1,
        elements,
        index,
        None,
        budget,
    )
}

/// The `:scope` a relative selector is anchored to while `:has()` is being
/// evaluated, and the combinator that joins the two.
///
/// It reaches only the **leftmost** compound of the relative selector, which is
/// where `:scope` sits: a `:not()` nested inside a `:has()` argument starts
/// again with no scope, because its own subject is the element it is testing.
#[derive(Clone, Copy)]
struct Scope {
    at: usize,
    combinator: Combinator,
}

fn match_from<E: Element>(
    selector: &Selector,
    compound: usize,
    elements: &[E],
    index: usize,
    scope: Option<Scope>,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    budget.spend_match()?;
    if !matches_compound(&selector.compounds[compound], elements, index, budget)? {
        return Ok(false);
    }
    if compound == 0 {
        return match scope {
            None => Ok(true),
            Some(scope) => related(elements, scope, index, budget),
        };
    }
    let element = &elements[index];
    match selector.combinators[compound - 1] {
        Combinator::Child => {
            let Some(parent) = element.parent() else {
                return Ok(false);
            };
            match_from(selector, compound - 1, elements, parent, scope, budget)
        }
        Combinator::NextSibling => {
            let Some(previous) = element.previous_sibling() else {
                return Ok(false);
            };
            match_from(selector, compound - 1, elements, previous, scope, budget)
        }
        Combinator::Descendant => {
            let mut at = element.parent();
            while let Some(ancestor) = at {
                if match_from(selector, compound - 1, elements, ancestor, scope, budget)? {
                    return Ok(true);
                }
                at = elements[ancestor].parent();
            }
            Ok(false)
        }
        Combinator::SubsequentSibling => {
            let mut at = element.previous_sibling();
            while let Some(sibling) = at {
                if match_from(selector, compound - 1, elements, sibling, scope, budget)? {
                    return Ok(true);
                }
                at = elements[sibling].previous_sibling();
            }
            Ok(false)
        }
    }
}

/// Is `index` in the stated relation to the `:scope` element?
///
/// The four arms are §14's four combinators read the other way round: the
/// leftmost compound of a relative selector has matched, and what is left to
/// check is that the element it matched really is the child, descendant or
/// sibling of the element `:has()` was asked about.
fn related<E: Element>(
    elements: &[E],
    scope: Scope,
    index: usize,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    let element = &elements[index];
    Ok(match scope.combinator {
        Combinator::Child => element.parent() == Some(scope.at),
        Combinator::NextSibling => element.previous_sibling() == Some(scope.at),
        Combinator::Descendant => {
            let mut at = element.parent();
            loop {
                budget.spend_match()?;
                match at {
                    None => break false,
                    Some(ancestor) if ancestor == scope.at => break true,
                    Some(ancestor) => at = elements[ancestor].parent(),
                }
            }
        }
        Combinator::SubsequentSibling => {
            let mut at = element.previous_sibling();
            loop {
                budget.spend_match()?;
                match at {
                    None => break false,
                    Some(sibling) if sibling == scope.at => break true,
                    Some(sibling) => at = elements[sibling].previous_sibling(),
                }
            }
        }
    })
}

fn matches_compound<E: Element>(
    compound: &Compound,
    elements: &[E],
    index: usize,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    let element = &elements[index];
    if let Some(name) = &compound.type_name {
        if element.local_name() != name {
            return Ok(false);
        }
    }
    for id in &compound.ids {
        if element.id() != Some(id.as_str()) {
            return Ok(false);
        }
    }
    for class in &compound.classes {
        if !element.has_class(class) {
            return Ok(false);
        }
    }
    for attribute in &compound.attributes {
        if !matches_attribute(attribute, element) {
            return Ok(false);
        }
    }
    for class in &compound.pseudo_classes {
        if !matches_pseudo_class(class, elements, index, budget)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn matches_pseudo_class<E: Element>(
    class: &PseudoClass,
    elements: &[E],
    index: usize,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    let element = &elements[index];
    Ok(match class {
        PseudoClass::Root => element.parent().is_none(),
        PseudoClass::FirstChild => element.previous_sibling().is_none(),
        PseudoClass::LastChild => element.next_sibling().is_none(),
        PseudoClass::OnlyChild => {
            element.previous_sibling().is_none() && element.next_sibling().is_none()
        }
        // §6.6.3, and the one structural pseudo-class the tree here cannot
        // answer: an element's children are not links this trait carries, and
        // *what counts as a child* — a text node, a comment, a CDATA
        // section — is the document language's question anyway.
        PseudoClass::Empty => element.is_empty(),
        // §6.6.2's "of type" is the element's own type, and here that is its
        // local name: this build has no namespace syntax in selectors, so a
        // type selector already compares local names and `:first-of-type`
        // agreeing with it is the only self-consistent reading.
        PseudoClass::FirstOfType => position_of_type(elements, index, false, budget)? == 1,
        PseudoClass::LastOfType => position_of_type(elements, index, true, budget)? == 1,
        PseudoClass::OnlyOfType => {
            position_of_type(elements, index, false, budget)? == 1
                && position_of_type(elements, index, true, budget)? == 1
        }
        PseudoClass::NthChild(nth) => nth.contains(position(elements, index, false, budget)?),
        PseudoClass::NthLastChild(nth) => nth.contains(position(elements, index, true, budget)?),
        PseudoClass::NthOfType(nth) => {
            nth.contains(position_of_type(elements, index, false, budget)?)
        }
        PseudoClass::NthLastOfType(nth) => {
            nth.contains(position_of_type(elements, index, true, budget)?)
        }
        // §6.5.1: the element's language is the nearest one an ancestor
        // declares, which is a walk this crate can do because the tree is
        // this crate's; which attribute declared it is not.
        PseudoClass::Lang(ranges) => match inherited(elements, index, Element::language, budget)? {
            None => false,
            Some(language) => ranges.iter().any(|range| language_matches(range, language)),
        },
        PseudoClass::Dir(wanted) => match inherited(elements, index, Element::direction, budget)? {
            None => false,
            Some(direction) => direction.eq_ignore_ascii_case(wanted),
        },
        // §6.2's rule: `:not()` matches when **none** of its arguments does.
        // A build that negated each argument separately would make
        // `:not(a, b)` mean `:not(a)` or `:not(b)`, which is everything.
        PseudoClass::Not(list) => {
            for selector in list {
                if selector.pseudo_element.is_some() {
                    continue;
                }
                if match_from(
                    selector,
                    selector.compounds.len() - 1,
                    elements,
                    index,
                    None,
                    budget,
                )? {
                    return Ok(false);
                }
            }
            true
        }
        PseudoClass::Is(list) | PseudoClass::Where(list) => {
            for selector in list {
                if selector.pseudo_element.is_some() {
                    continue;
                }
                if match_from(
                    selector,
                    selector.compounds.len() - 1,
                    elements,
                    index,
                    None,
                    budget,
                )? {
                    return Ok(true);
                }
            }
            false
        }
        PseudoClass::Has(list) => matches_has(list, elements, index, budget)?,
        PseudoClass::Link => element.is_link(),
        PseudoClass::Checked => element.ui_state().checked,
        // Each of these is `Some(_)` only for an element the document language
        // classifies at all, so `p:enabled` and `p:optional` are false rather
        // than true-by-negation.
        PseudoClass::Disabled => element.ui_state().disabled == Some(true),
        PseudoClass::Enabled => element.ui_state().disabled == Some(false),
        PseudoClass::Required => element.ui_state().required == Some(true),
        PseudoClass::Optional => element.ui_state().required == Some(false),
        PseudoClass::ReadOnly => element.ui_state().read_only == Some(true),
        PseudoClass::ReadWrite => element.ui_state().read_only == Some(false),
        PseudoClass::NoSuchState(_) => false,
    })
}

/// The one-based position of an element among its siblings, from either end.
fn position<E: Element>(
    elements: &[E],
    index: usize,
    from_end: bool,
    budget: &mut Budget,
) -> Result<i64, Refusal> {
    count_siblings(elements, index, from_end, budget, |_| true)
}

/// The same, counting only siblings of the same type.
fn position_of_type<E: Element>(
    elements: &[E],
    index: usize,
    from_end: bool,
    budget: &mut Budget,
) -> Result<i64, Refusal> {
    let name = elements[index].local_name();
    count_siblings(elements, index, from_end, budget, |sibling: &E| {
        sibling.local_name() == name
    })
}

/// One plus the number of siblings before (or after) `index` that the
/// predicate admits — §6.6.2 counts **from one**, so an element with no
/// preceding sibling is at position 1 rather than 0, and an off-by-one here
/// turns every `:nth-child(odd)` into `:nth-child(even)`.
fn count_siblings<E: Element, F: Fn(&E) -> bool>(
    elements: &[E],
    index: usize,
    from_end: bool,
    budget: &mut Budget,
    admit: F,
) -> Result<i64, Refusal> {
    let mut position = 1i64;
    let mut at = if from_end {
        elements[index].next_sibling()
    } else {
        elements[index].previous_sibling()
    };
    while let Some(sibling) = at {
        budget.spend_match()?;
        if admit(&elements[sibling]) {
            position += 1;
        }
        at = if from_end {
            elements[sibling].next_sibling()
        } else {
            elements[sibling].previous_sibling()
        };
    }
    Ok(position)
}

/// The nearest value an element or one of its ancestors declares.
///
/// `:lang()` and `:dir()` both inherit, and both inherit the same way: the
/// document language says which attribute declares the value and says nothing
/// about the walk, because the walk is a fact about the tree.
fn inherited<'a, E: Element, F: Fn(&'a E) -> Option<&'a str>>(
    elements: &'a [E],
    index: usize,
    declared: F,
    budget: &mut Budget,
) -> Result<Option<&'a str>, Refusal> {
    let mut at = Some(index);
    while let Some(cursor) = at {
        budget.spend_match()?;
        if let Some(value) = declared(&elements[cursor]) {
            return Ok(Some(value));
        }
        at = elements[cursor].parent();
    }
    Ok(None)
}

/// RFC 4647 §3.3.2's extended filtering, which is what §6.5.1 cites.
///
/// The subtag loop is the whole of it, and the two rules that make it more
/// than a prefix test are that `*` in a range matches **any** subtag and that
/// a tag subtag the range does not name is skipped — so `de-*-DE` matches
/// `de-Latn-DE` — unless it is a singleton (one character), which is an
/// extension boundary and stops the skip.
fn language_matches(range: &str, tag: &str) -> bool {
    // A language stated to be *unknown* — which is what an empty declaration
    // means — is in no range at all, `*` included. It is a different answer
    // from having declared nothing, which inherits instead.
    if tag.is_empty() {
        return false;
    }
    let range: Vec<&str> = range.split('-').collect();
    let tag: Vec<&str> = tag.split('-').collect();
    let (Some(first_range), Some(first_tag)) = (range.first(), tag.first()) else {
        return false;
    };
    if *first_range != "*" && !first_range.eq_ignore_ascii_case(first_tag) {
        return false;
    }
    let (mut r, mut t) = (1usize, 1usize);
    while r < range.len() {
        if range[r] == "*" {
            r += 1;
            continue;
        }
        if t >= tag.len() {
            return false;
        }
        if range[r].eq_ignore_ascii_case(tag[t]) {
            r += 1;
            t += 1;
            continue;
        }
        if tag[t].len() == 1 {
            return false;
        }
        t += 1;
    }
    true
}

/// §4.2: does any relative selector find something, anchored at `index`?
fn matches_has<E: Element>(
    list: &[Relative],
    elements: &[E],
    index: usize,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    for relative in list {
        if relative.selector.pseudo_element.is_some() {
            continue;
        }
        let (start, end) = relative_range(elements, index, relative.combinator, budget)?;
        let scope = Scope {
            at: index,
            combinator: relative.combinator,
        };
        for candidate in start..end.min(elements.len()) {
            if match_from(
                &relative.selector,
                relative.selector.compounds.len() - 1,
                elements,
                candidate,
                Some(scope),
                budget,
            )? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The half-open range of indices a relative selector's subject can occupy.
///
/// **This is the bound, and it comes from the trait's own contract rather than
/// from a guess.** [`crate::Element`] is documented as a slice in document
/// order with every parent before its children, so an element's descendants
/// are the run that follows it and its later siblings' subtrees are the run
/// after that. Scanning to the end of the slice instead would be correct and
/// would make `:has()` quadratic in the document rather than in the subtree —
/// which for a book is the difference between a rule and a
/// [`Refusal::TooManySelectorMatches`].
///
/// A candidate inside the range is still tested in full, so a range that is
/// too wide costs time and never invents a match.
fn relative_range<E: Element>(
    elements: &[E],
    at: usize,
    combinator: Combinator,
    budget: &mut Budget,
) -> Result<(usize, usize), Refusal> {
    Ok(match combinator {
        Combinator::Descendant | Combinator::Child => (at + 1, subtree_end(elements, at, budget)?),
        Combinator::NextSibling | Combinator::SubsequentSibling => {
            let start = subtree_end(elements, at, budget)?;
            let end = match elements[at].parent() {
                Some(parent) => subtree_end(elements, parent, budget)?,
                None => elements.len(),
            };
            (start, end)
        }
    })
}

/// The first index after everything inside `at`, in document order.
fn subtree_end<E: Element>(
    elements: &[E],
    at: usize,
    budget: &mut Budget,
) -> Result<usize, Refusal> {
    let mut cursor = Some(at);
    while let Some(index) = cursor {
        budget.spend_match()?;
        if let Some(next) = elements[index].next_sibling() {
            return Ok(next);
        }
        cursor = elements[index].parent();
    }
    Ok(elements.len())
}

fn matches_attribute<E: Element>(selector: &AttributeSelector, element: &E) -> bool {
    let Some(value) = element.attribute(&selector.name) else {
        return false;
    };
    let fold = selector.case_insensitive;
    let same = |a: &str, b: &str| {
        if fold {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    match &selector.matcher {
        AttributeMatch::Exists => true,
        AttributeMatch::Equals(wanted) => same(value, wanted),
        // §6.3.2: an empty value or one containing whitespace matches nothing.
        AttributeMatch::Includes(wanted) => {
            !wanted.is_empty()
                && !wanted.chars().any(char::is_whitespace)
                && value
                    .split_ascii_whitespace()
                    .any(|word| same(word, wanted))
        }
        AttributeMatch::DashMatch(wanted) => {
            same(value, wanted)
                || (value.len() > wanted.len()
                    && value.as_bytes().get(wanted.len()) == Some(&b'-')
                    && same(&value[..wanted.len()], wanted))
        }
        // §6.3.3 to §6.3.5: an empty operand matches nothing at all, which is
        // the one case where "starts with the empty string" would be true.
        AttributeMatch::Prefix(wanted) => {
            !wanted.is_empty()
                && value.len() >= wanted.len()
                && same(&value[..wanted.len()], wanted)
        }
        AttributeMatch::Suffix(wanted) => {
            !wanted.is_empty()
                && value.len() >= wanted.len()
                && same(&value[value.len() - wanted.len()..], wanted)
        }
        AttributeMatch::Substring(wanted) => {
            if wanted.is_empty() {
                false
            } else if fold {
                value
                    .to_ascii_lowercase()
                    .contains(&wanted.to_ascii_lowercase())
            } else {
                value.contains(wanted.as_str())
            }
        }
    }
}

// ---- the index --------------------------------------------------------------

/// Which bucket a selector's rightmost compound belongs in.
///
/// One bucket per selector, never several, so a candidate list needs no
/// deduplication — which is what keeps the index from becoming its own quadratic
/// cost.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Bucket {
    Id(String),
    Class(String),
    Type(String),
    Universal,
}

fn bucket_for(selector: &Selector) -> Bucket {
    let last = selector
        .compounds
        .last()
        .expect("a selector has at least one compound");
    if let Some(id) = last.ids.first() {
        return Bucket::Id(id.clone());
    }
    if let Some(class) = last.classes.first() {
        return Bucket::Class(class.clone());
    }
    if let Some(name) = &last.type_name {
        return Bucket::Type(name.clone());
    }
    Bucket::Universal
}

/// Rules bucketed by their rightmost compound's most selective key.
///
/// **The index is an optimisation and is not a bound.** A stylesheet whose
/// every rule names the same class puts every rule in one bucket and gets the
/// full rules-times-elements product, which is exactly what
/// [`crate::limits::MAX_SELECTOR_MATCHES`] is for and exactly the input a
/// hostile book would write.
#[derive(Debug, Default)]
pub struct Index {
    by_id: HashMap<String, Vec<usize>>,
    by_class: HashMap<String, Vec<usize>>,
    by_type: HashMap<String, Vec<usize>>,
    universal: Vec<usize>,
}

impl Index {
    /// Adds one selector, identified by an opaque handle the caller chooses.
    pub fn insert(&mut self, selector: &Selector, handle: usize) {
        match bucket_for(selector) {
            Bucket::Id(id) => self.by_id.entry(id).or_default().push(handle),
            Bucket::Class(class) => self.by_class.entry(class).or_default().push(handle),
            Bucket::Type(name) => self.by_type.entry(name).or_default().push(handle),
            Bucket::Universal => self.universal.push(handle),
        }
    }

    /// Every handle that could possibly match this element.
    ///
    /// A superset, always: a handle this does not return **cannot** match, and
    /// one it does return still has to be tested. `an_indexed_cascade_and_a_
    /// brute_force_one_agree` is what says the first half is true, because a
    /// bucketing bug produces a book that is styled slightly less than it
    /// should be — which reads as a plain stylesheet rather than as a defect.
    ///
    /// # A superset **without repeats**, and gap 31 milestone 13's campaign
    /// found out why that matters
    ///
    /// [`bucket_for`] puts every selector in exactly one bucket, so the four
    /// lists below are disjoint and nothing here can return a handle twice —
    /// except through the loop over classes, because an element may carry the
    /// **same class twice**. `class="note note"` is valid HTML that real books
    /// write by accident, and this function used to return every rule in that
    /// bucket once per repetition.
    ///
    /// The visible half is small: applying one declaration twice lands on the
    /// same computed value, so the page is unchanged. The half that is not
    /// small is the budget. Every repeat is charged against
    /// [`crate::limits::MAX_SELECTOR_MATCHES`], and the element cap counts
    /// *elements* rather than class tokens — so `class="a a a a …"` with a
    /// thousand repetitions multiplies the whole cascade's cost by a thousand
    /// out of one attribute, which is a cap nothing was enforcing.
    ///
    /// `cargo fuzz run css` found it in 428 executions, as the index and brute
    /// force disagreeing; `an_index_does_not_return_a_rule_twice_for_a_repeated
    /// _class` is the reproducer as a test.
    ///
    /// The fix is here rather than in whatever builds the element, and
    /// deliberately: [`Element`] is a trait a caller implements, so a rule
    /// enforced in the caller is a rule enforced nowhere this crate can see.
    pub fn candidates<E: Element>(&self, element: &E) -> Vec<usize> {
        let mut out = self.universal.clone();
        if let Some(id) = element.id() {
            if let Some(handles) = self.by_id.get(id) {
                out.extend_from_slice(handles);
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for class in element.classes() {
            let class = class.as_str();
            if seen.contains(&class) {
                continue;
            }
            seen.push(class);
            if let Some(handles) = self.by_class.get(class) {
                out.extend_from_slice(handles);
            }
        }
        if let Some(handles) = self.by_type.get(element.local_name()) {
            out.extend_from_slice(handles);
        }
        out
    }
}
