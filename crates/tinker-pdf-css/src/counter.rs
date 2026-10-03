//! `css-lists-3` §4's counters, walked over the element tree once the cascade
//! is done, and `css-counter-styles-3`'s predefined styles they are drawn in.
//!
//! # Why a second walk and not part of the cascade
//!
//! A counter's value at an element depends on every element **before** it in
//! the tree that generated a box — and whether an element generates a box is
//! its computed `display`, and its ancestors'. The cascade computes styles in
//! document order too, but a counter reset on an element is visible to that
//! element's *following siblings*, so the value at one element is not a
//! function of its ancestors' styles alone, which is all the cascade's one
//! pass has in hand. Doing it afterwards, over finished styles, is what keeps
//! the two questions apart.
//!
//! # The scope rule, in the shape it is implemented
//!
//! §4.5 states it as sets of counters copied from a parent, a preceding sibling
//! and the element before in tree order. It is implemented as the equivalent
//! stack: a counter instantiated on an element is **owned by that element's
//! parent**, is visible to the element, its descendants and its following
//! siblings, and is popped when its owner closes. §4.5's *"if innermost
//! counter's originating element is element or a previous sibling of element,
//! remove innermost counter"* is the case where the innermost instance of that
//! name has the same owner, and it is overwritten rather than nested — which
//! is why two sibling `<ol>`s each number from one rather than the second
//! nesting inside the first.
//!
//! `::before` is the originating element's first child and `::after` its last,
//! for §4.5's purposes as for layout's, so a `::before`'s `counter-increment`
//! is seen by the element's children and an `::after`'s by nothing after it.
//! An element that generates no box — `display: none`, or inside one — neither
//! changes a counter nor reads one (§4.3).
//!
//! # What it costs
//!
//! Every operation — an instantiation, an increment, a set, a value read — is
//! charged to the cascade's own work cap, [`crate::limits::MAX_SELECTOR_MATCHES`].
//! A stylesheet can name thousands of counters on every element; charging the
//! same budget the selectors spend bounds both the time and the instances live
//! at once (one at most per operation), and refuses such a book exactly as a
//! book whose selectors defeat the index is refused.

use std::collections::BTreeMap;

use crate::cascade::{ComputedStyle, Generated};
use crate::property::{ContentItem, Display, ListStyleType};
use crate::{Budget, Element, Refusal};

/// The counter `display: list-item` increments without being asked,
/// `css-lists-3` §4.6.
pub const LIST_ITEM: &str = "list-item";

/// The live counters at one point of the walk.
#[derive(Default)]
struct Scope {
    /// Per name, the instances in scope, outermost first, each with its owner
    /// (the parent of the element that instantiated it; `None` for the root).
    by_name: BTreeMap<String, Vec<(Option<usize>, i64)>>,
    /// Every instance pushed, in order, so that closing an element pops what it
    /// owns without searching.
    log: Vec<(String, Option<usize>)>,
}

impl Scope {
    /// §4.5's *instantiate a counter*.
    fn instantiate(
        &mut self,
        name: &str,
        value: i64,
        owner: Option<usize>,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        budget.spend_match()?;
        let stack = self.by_name.entry(name.to_owned()).or_default();
        match stack.last_mut() {
            // The innermost instance came from this element or a previous
            // sibling: §4.5 removes it, and a new one at the same depth with
            // the same owner is that instance overwritten.
            Some((at, held)) if *at == owner => *held = value,
            _ => {
                stack.push((owner, value));
                self.log.push((name.to_owned(), owner));
            }
        }
        Ok(())
    }

    /// The innermost instance, instantiated at zero where there is none —
    /// §4.3's and §4.8's shared fallback.
    fn innermost(
        &mut self,
        name: &str,
        owner: Option<usize>,
        budget: &mut Budget,
    ) -> Result<&mut i64, Refusal> {
        budget.spend_match()?;
        let stack = self.by_name.entry(name.to_owned()).or_default();
        if stack.is_empty() {
            stack.push((owner, 0));
            self.log.push((name.to_owned(), owner));
        }
        // At least one: the branch above pushed one where there was none.
        let last = stack.len() - 1;
        Ok(&mut stack[last].1)
    }

    /// Every instance in scope, outermost first, for `counters()`.
    fn all(
        &mut self,
        name: &str,
        owner: Option<usize>,
        budget: &mut Budget,
    ) -> Result<Vec<i64>, Refusal> {
        self.innermost(name, owner, budget)?;
        let values: Vec<i64> = self
            .by_name
            .get(name)
            .map(|stack| stack.iter().map(|(_, value)| *value).collect())
            .unwrap_or_default();
        for _ in 1..values.len() {
            budget.spend_match()?;
        }
        Ok(values)
    }

    /// An element's subtree has ended: what its children instantiated goes.
    fn close(&mut self, element: usize) {
        while let Some((name, owner)) = self.log.last() {
            if *owner != Some(element) {
                break;
            }
            if let Some(stack) = self.by_name.get_mut(name) {
                stack.pop();
            }
            self.log.pop();
        }
    }

    /// One box's `counter-reset`, `counter-increment` and `counter-set`, in
    /// that order (§4.2 to §4.4), with §4.6's implicit `list-item` increment.
    fn apply(
        &mut self,
        style: &ComputedStyle,
        owner: Option<usize>,
        list_item: bool,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        for change in &style.counter_reset {
            self.instantiate(&change.name, i64::from(change.value), owner, budget)?;
        }
        let mut list_item_named = false;
        for change in &style.counter_increment {
            list_item_named |= change.name == LIST_ITEM;
            let value = self.innermost(&change.name, owner, budget)?;
            *value = value.saturating_add(i64::from(change.value));
        }
        if list_item && !list_item_named {
            let value = self.innermost(LIST_ITEM, owner, budget)?;
            *value = value.saturating_add(1);
        }
        for change in &style.counter_set {
            *self.innermost(&change.name, owner, budget)? = i64::from(change.value);
        }
        Ok(())
    }

    /// A `content` value's items, as text.
    fn text<E: Element>(
        &mut self,
        items: &[ContentItem],
        element: &E,
        owner: Option<usize>,
        budget: &mut Budget,
    ) -> Result<String, Refusal> {
        let mut text = String::new();
        for item in items {
            match item {
                ContentItem::Text(literal) => text.push_str(literal),
                // §2.4: an attribute the element does not carry contributes
                // the empty string, which is the specification's own answer.
                ContentItem::Attr(name) => text.push_str(element.attribute(name).unwrap_or("")),
                ContentItem::Counter { name, style } => {
                    let value = *self.innermost(name, owner, budget)?;
                    text.push_str(&represent(value, *style));
                }
                ContentItem::Counters {
                    name,
                    separator,
                    style,
                } => {
                    let values = self.all(name, owner, budget)?;
                    for (index, value) in values.iter().enumerate() {
                        if index > 0 {
                            text.push_str(separator);
                        }
                        text.push_str(&represent(*value, *style));
                    }
                }
            }
        }
        Ok(text)
    }
}

/// Walks the tree, writing every generated box's text and every list item's
/// marker into `generated`.
///
/// `elements` is in document order, parents before children, which the cascade
/// has already checked; `styles` and `generated` are parallel to it.
pub(crate) fn resolve<E: Element>(
    elements: &[E],
    styles: &[ComputedStyle],
    generated: &mut [Generated],
    budget: &mut Budget,
) -> Result<(), Refusal> {
    let mut scope = Scope::default();
    // The open elements, outermost first, and whether each generates a box.
    let mut open: Vec<(usize, bool)> = Vec::new();
    for at in 0..elements.len() {
        let parent = elements[at].parent();
        while let Some(&(top, boxed)) = open.last() {
            if Some(top) == parent {
                break;
            }
            open.pop();
            finish(elements, generated, &mut scope, top, boxed, budget)?;
        }
        let Some(style) = styles.get(at) else {
            continue;
        };
        let parent_boxed = open.last().is_none_or(|(_, boxed)| *boxed);
        let boxed = parent_boxed && style.display != Display::None;
        open.push((at, boxed));
        if !boxed {
            continue;
        }
        let list_item = style.display == Display::ListItem;
        scope.apply(style, parent, list_item, budget)?;
        let marker = if list_item {
            let value = *scope.innermost(LIST_ITEM, parent, budget)?;
            Some(marker_text(style.list_style_type, value))
        } else {
            None
        };
        let Some(slot) = generated.get_mut(at) else {
            continue;
        };
        slot.marker = marker;
        if let Some(before) = slot.before.as_mut() {
            if before.style.display != Display::None {
                scope.apply(&before.style, Some(at), false, budget)?;
                before.text = scope.text(&before.content, &elements[at], Some(at), budget)?;
            }
        }
    }
    while let Some((top, boxed)) = open.pop() {
        finish(elements, generated, &mut scope, top, boxed, budget)?;
    }
    Ok(())
}

/// An element's subtree has ended: its `::after`, then its scope.
fn finish<E: Element>(
    elements: &[E],
    generated: &mut [Generated],
    scope: &mut Scope,
    at: usize,
    boxed: bool,
    budget: &mut Budget,
) -> Result<(), Refusal> {
    if boxed {
        if let Some(after) = generated.get_mut(at).and_then(|slot| slot.after.as_mut()) {
            if after.style.display != Display::None {
                scope.apply(&after.style, Some(at), false, budget)?;
                after.text = scope.text(&after.content, &elements[at], Some(at), budget)?;
            }
        }
    }
    scope.close(at);
    Ok(())
}

/// A counter value in a predefined counter style, `css-counter-styles-3` §6,
/// without a suffix — what `counter()` produces.
///
/// The alphabetic and additive styles have a **range**, and a value outside it
/// is drawn in the fallback style, `decimal` (§2.2): `lower-alpha` has no
/// zero and `lower-roman` stops at 3 999. A cyclic style draws every value,
/// negative ones included, as its one symbol.
#[must_use]
pub fn represent(value: i64, style: ListStyleType) -> String {
    match style {
        ListStyleType::None => String::new(),
        ListStyleType::Disc => "\u{2022}".to_string(),
        ListStyleType::Circle => "\u{25e6}".to_string(),
        ListStyleType::Square => "\u{25aa}".to_string(),
        ListStyleType::Decimal => value.to_string(),
        ListStyleType::LowerAlpha => alphabetic(value, b'a'),
        ListStyleType::UpperAlpha => alphabetic(value, b'A'),
        ListStyleType::LowerRoman => roman(value).to_lowercase(),
        ListStyleType::UpperRoman => roman(value),
    }
}

/// A list item's marker text: [`represent`] with the style's suffix.
///
/// §6's suffix is `". "` for the numeric, alphabetic and additive styles and
/// `" "` for the cyclic ones; the space is left to whoever places the marker,
/// because an `outside` marker is set clear of the box by a gap of its own and
/// an `inside` one by the space, and a marker carrying both would be set twice
/// as far from its text.
#[must_use]
pub fn marker_text(style: ListStyleType, value: i64) -> String {
    match style {
        ListStyleType::None => String::new(),
        ListStyleType::Disc | ListStyleType::Circle | ListStyleType::Square => {
            represent(value, style)
        }
        ListStyleType::Decimal
        | ListStyleType::LowerAlpha
        | ListStyleType::UpperAlpha
        | ListStyleType::LowerRoman
        | ListStyleType::UpperRoman => format!("{}.", represent(value, style)),
    }
}

/// Bijective base 26: 1 is `a`, 26 is `z`, 27 is `aa`. **Not** ordinary base
/// 26: there is no digit for zero, which is why zero and the negatives fall
/// back to decimal.
fn alphabetic(value: i64, first: u8) -> String {
    if value < 1 {
        return value.to_string();
    }
    let mut out = Vec::new();
    let mut n = value;
    while n > 0 {
        let digit = (n - 1) % 26;
        // In 0..26 by the line above, so the addition stays a letter.
        out.push(first + digit as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Additive Roman numerals over §6's range, 1 to 3 999; decimal outside it.
fn roman(value: i64) -> String {
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    if !(1..=3_999).contains(&value) {
        return value.to_string();
    }
    let mut out = String::new();
    let mut n = value;
    for (step, sign) in TABLE {
        while n >= step {
            out.push_str(sign);
            n -= step;
        }
    }
    out
}
