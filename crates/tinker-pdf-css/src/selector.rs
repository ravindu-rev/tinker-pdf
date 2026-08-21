//! `selectors-4`: §6.1–§6.4's simple selectors, §14's four combinators, §15's
//! specificity, and the matching that uses them.
//!
//! # Nothing here knows what XHTML is
//!
//! Ruling 8 says a leaf takes bytes and plain parameters. The element side of
//! matching arrives through [`crate::Element`], a five-method trait the caller
//! implements, and the three methods that look like HTML are the ones that make
//! the crate *not* know about it: `selectors-4` §6.6 says *"the ID attribute is
//! defined by the document language"* and §6.5 says the same of class, so
//! asking the element for its id and its classes is what keeps `id=` and
//! `class=` out of this crate. A matcher that read `attribute("class")` would
//! have hard-coded XHTML into a crate whose whole argument is that it has not.
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

/// A pseudo-class, split by whether this build evaluates it.
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
    /// `:not(…)`
    Not(Vec<Selector>),
    /// `:is(…)`, and `:matches()`/`:any()` under their old names.
    Is(Vec<Selector>),
    /// `:where(…)`, which is `:is()` at zero specificity.
    Where(Vec<Selector>),
    /// A pseudo-class `selectors-4` defines that this build does not evaluate.
    ///
    /// **It never matches, and it is counted and named.** That is decision 5's
    /// shape applied to a selector: `a:hover` genuinely cannot match in a
    /// paginated render and `tr:nth-child(2n)` genuinely could and does not, and
    /// a build that quietly dropped either would style a book differently from
    /// every reading system with nothing anywhere saying so. The alternative —
    /// treating it as an invalid selector — is worse in a specific way: it
    /// would take `a:hover, a:link { … }` down with it, because §3.1
    /// invalidates a whole list for one bad member.
    Inert(&'static str),
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
/// The four at the top are evaluated. The rest are `selectors-4`'s and are
/// **inert**: recognised, never matching, and counted by name. The list is
/// closed on purpose — a name that is not here is `Invalid::UnknownPseudo` and
/// takes its rule down, because a selector nothing recognises is a selector
/// from a specification this build does not cite, and guessing at it is how a
/// cascade acquires a rule its author never wrote.
fn simple_pseudo_class(name: &str) -> Option<PseudoClass> {
    Some(match name {
        "root" => PseudoClass::Root,
        "first-child" => PseudoClass::FirstChild,
        "last-child" => PseudoClass::LastChild,
        "only-child" => PseudoClass::OnlyChild,
        "hover" => PseudoClass::Inert(":hover"),
        "focus" => PseudoClass::Inert(":focus"),
        "focus-within" => PseudoClass::Inert(":focus-within"),
        "focus-visible" => PseudoClass::Inert(":focus-visible"),
        "active" => PseudoClass::Inert(":active"),
        "link" => PseudoClass::Inert(":link"),
        "visited" => PseudoClass::Inert(":visited"),
        "any-link" => PseudoClass::Inert(":any-link"),
        "target" => PseudoClass::Inert(":target"),
        "empty" => PseudoClass::Inert(":empty"),
        "checked" => PseudoClass::Inert(":checked"),
        "disabled" => PseudoClass::Inert(":disabled"),
        "enabled" => PseudoClass::Inert(":enabled"),
        "required" => PseudoClass::Inert(":required"),
        "optional" => PseudoClass::Inert(":optional"),
        "read-only" => PseudoClass::Inert(":read-only"),
        "read-write" => PseudoClass::Inert(":read-write"),
        "first-of-type" => PseudoClass::Inert(":first-of-type"),
        "last-of-type" => PseudoClass::Inert(":last-of-type"),
        "only-of-type" => PseudoClass::Inert(":only-of-type"),
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
        // §15 gives `:has()` its most specific argument too, and this build
        // does not evaluate it — a relational selector needs a descendant
        // search the `Element` trait deliberately cannot do.
        "has" => {
            let list = parse_list(arguments, max_parts)?;
            Ok((PseudoClass::Inert(":has"), most_specific(&list)))
        }
        "nth-child" => inert_function(":nth-child"),
        "nth-last-child" => inert_function(":nth-last-child"),
        "nth-of-type" => inert_function(":nth-of-type"),
        "nth-last-of-type" => inert_function(":nth-last-of-type"),
        "lang" => inert_function(":lang"),
        "dir" => inert_function(":dir"),
        other => Err(Invalid::UnknownPseudo(format!(":{other}()"))),
    }
}

fn inert_function(name: &'static str) -> Result<(PseudoClass, Specificity), Invalid> {
    Ok((PseudoClass::Inert(name), Specificity { a: 0, b: 1, c: 0 }))
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

/// The warnings a parsed selector list owes: one per inert pseudo-class and one
/// per pseudo-element, each named.
pub fn warnings(selectors: &[Selector]) -> Vec<Warning> {
    let mut out = Vec::new();
    for selector in selectors {
        if let Some(element) = selector.pseudo_element {
            out.push(Warning::PseudoElementUnsupported(element.name()));
        }
        for compound in &selector.compounds {
            collect_inert(&compound.pseudo_classes, &mut out);
        }
    }
    out
}

fn collect_inert(classes: &[PseudoClass], out: &mut Vec<Warning>) {
    for class in classes {
        match class {
            PseudoClass::Inert(name) => out.push(Warning::PseudoClassUnsupported(name)),
            PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
                for selector in list {
                    for compound in &selector.compounds {
                        collect_inert(&compound.pseudo_classes, out);
                    }
                }
            }
            PseudoClass::Root
            | PseudoClass::FirstChild
            | PseudoClass::LastChild
            | PseudoClass::OnlyChild => {}
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
        budget,
    )
}

fn match_from<E: Element>(
    selector: &Selector,
    compound: usize,
    elements: &[E],
    index: usize,
    budget: &mut Budget,
) -> Result<bool, Refusal> {
    budget.spend_match()?;
    if !matches_compound(&selector.compounds[compound], elements, index, budget)? {
        return Ok(false);
    }
    if compound == 0 {
        return Ok(true);
    }
    let element = &elements[index];
    match selector.combinators[compound - 1] {
        Combinator::Child => {
            let Some(parent) = element.parent() else {
                return Ok(false);
            };
            match_from(selector, compound - 1, elements, parent, budget)
        }
        Combinator::NextSibling => {
            let Some(previous) = element.previous_sibling() else {
                return Ok(false);
            };
            match_from(selector, compound - 1, elements, previous, budget)
        }
        Combinator::Descendant => {
            let mut at = element.parent();
            while let Some(ancestor) = at {
                if match_from(selector, compound - 1, elements, ancestor, budget)? {
                    return Ok(true);
                }
                at = elements[ancestor].parent();
            }
            Ok(false)
        }
        Combinator::SubsequentSibling => {
            let mut at = element.previous_sibling();
            while let Some(sibling) = at {
                if match_from(selector, compound - 1, elements, sibling, budget)? {
                    return Ok(true);
                }
                at = elements[sibling].previous_sibling();
            }
            Ok(false)
        }
    }
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
                    budget,
                )? {
                    return Ok(true);
                }
            }
            false
        }
        PseudoClass::Inert(_) => false,
    })
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
