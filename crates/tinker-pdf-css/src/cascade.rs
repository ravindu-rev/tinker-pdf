//! `css-cascade-5` §6.1's sorting order, **whole**, and §7.2's inheritance as
//! one top-down pass.
//!
//! # The order, and the two shortcuts that are both wrong
//!
//! The tempting summary is *"specificity, then order"*. §6.1 has six criteria
//! and the first of them is where real books diverge from that summary:
//!
//! 1. **Origin and importance**, and the `!important` reversal is the half a
//!    first implementation drops. The order, weakest first, is *normal UA,
//!    normal user, normal author, animation, important author, important user,
//!    important UA, transition*. So an `!important` **author** rule loses to an
//!    `!important` **UA** rule — backwards from the normal case, and it is how
//!    a reading system keeps control of what it must. A build that treated
//!    `!important` as "add ten to the weight" gets every ordinary book right
//!    and this one case wrong.
//! 2. **Context** — shadow trees. Not applicable here, and named so its
//!    absence is a decision rather than an omission.
//! 3. **Element-attached styles.** `style=""` beats every selector at the same
//!    origin and importance, whatever its specificity. Real books use it
//!    constantly.
//! 4. **Layers.** `@layer` is refused at the at-rule, by name, because a build
//!    that read an unknown at-rule's block as ordinary rules would silently
//!    invert this criterion.
//! 5. **Specificity**, `selectors-4` §15's tuple.
//! 6. **Order of appearance**, last wins.
//!
//! Animations and transitions have no source in this engine, so criteria at
//! their ranks are unreachable; the ranks are still spelled out in [`rank`] so
//! that adding one later is a new arm rather than a renumbering.
//!
//! # Inheritance is one pass, and the alternative is quadratic
//!
//! §7.2 propagates the parent's **computed** value, so the cascade runs
//! top-down once: the caller hands elements in document order, every parent
//! precedes its children, and a child starts from a copy of its parent's
//! computed style with the non-inherited properties reset.
//!
//! The alternative — resolving an inherited property on demand by walking up
//! until somebody specifies it — gives the **same answer** and is quadratic in
//! tree depth, and giving the same answer is exactly why it is worth deciding
//! here rather than discovering later. [`resolve_lazily`] is that
//! implementation, written so `a_lazy_resolution_and_the_single_pass_agree` can
//! compare the two; it is not the shipped route and its doc comment says so.

use crate::parser::{Declared, Report, StyleRule, Stylesheet};
use crate::property::*;
use crate::selector::{self, Index, Specificity};
use crate::{Budget, Element, Limits, Refusal};

/// Where a declaration came from, `css-cascade-5` §6.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// This engine's own stylesheet.
    UserAgent,
    /// A user's, which nothing in this engine supplies yet and which is here
    /// because the ordering is meaningless without it.
    User,
    /// The book's.
    Author,
}

/// §6.1's first criterion as a number, higher winning.
///
/// The gaps are deliberate and are documented rather than closed up:
/// **4** is animation and **8** is transition, neither of which has a source in
/// this engine. Numbering around them means the day one arrives it is a new
/// arm rather than a renumbering of everything below it — and a renumbering is
/// exactly the edit that would quietly move `!important` back the right way up.
pub fn rank(origin: Origin, important: bool) -> u8 {
    match (origin, important) {
        // 8: transition — absent.
        (Origin::UserAgent, true) => 7,
        (Origin::User, true) => 6,
        (Origin::Author, true) => 5,
        // 4: animation — absent.
        (Origin::Author, false) => 3,
        (Origin::User, false) => 2,
        (Origin::UserAgent, false) => 1,
    }
}

/// The whole sort key of one declaration, in §6.1's order.
///
/// `Ord` is derived and the field order **is** the specification's order, which
/// is why the fields are in this order and not in a tidier one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CascadeKey {
    /// §6.1 criterion 1.
    rank: u8,
    /// §6.1 criterion 3: an element-attached declaration beats a selector-
    /// matched one at the same origin and importance.
    attached: bool,
    /// §6.1 criterion 5.
    specificity: Specificity,
    /// §6.1 criterion 6: later in the document order of the sheets wins.
    order: usize,
}

/// Every property this build computes, resolved.
///
/// A struct of typed fields rather than a map, so that a consumer reads
/// `style.float` and gets a [`Float`] — there is no string anywhere in it, and
/// no way for a layout engine to ask for a property that does not exist.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    /// `color`
    pub color: Color,
    /// `font-family`
    pub font_family: Vec<FontFamily>,
    /// `font-size`, in CSS pixels.
    pub font_size: f64,
    /// `font-style`
    pub font_style: FontStyle,
    /// `font-variant`
    pub font_variant: FontVariant,
    /// `font-weight`, 1 to 1000.
    pub font_weight: u16,
    /// `line-height`
    pub line_height: LineHeight,
    /// `letter-spacing`
    pub letter_spacing: Spacing,
    /// `word-spacing`
    pub word_spacing: Spacing,
    /// `text-align`
    pub text_align: TextAlign,
    /// `text-indent`
    pub text_indent: LengthPercentage,
    /// `white-space`
    pub white_space: WhiteSpace,
    /// `list-style-type`
    pub list_style_type: ListStyleType,
    /// `visibility`
    pub visibility: Visibility,
    /// `text-decoration`
    pub text_decoration: TextDecoration,
    /// `display`
    pub display: Display,
    /// `float`
    pub float: Float,
    /// `clear`
    pub clear: Clear,
    /// `box-sizing`
    pub box_sizing: BoxSizing,
    /// `width`
    pub width: Size,
    /// `height`
    pub height: Size,
    /// `margin-*`
    pub margin: Sides<MarginValue>,
    /// `padding-*`
    pub padding: Sides<LengthPercentage>,
    /// `border-*-width`, in CSS pixels.
    pub border_width: Sides<f64>,
    /// `border-*-style`
    pub border_style: Sides<BorderStyle>,
    /// `border-*-color`
    pub border_color: Sides<Color>,
    /// `background-color`
    pub background_color: Color,
    /// `page-break-before`
    pub page_break_before: PageBreak,
    /// `page-break-after`
    pub page_break_after: PageBreak,
    /// `page-break-inside`
    pub page_break_inside: PageBreakInside,
    /// `orphans`
    pub orphans: u16,
    /// `widows`
    pub widows: u16,
    /// `overflow-wrap`
    pub overflow_wrap: OverflowWrap,
    /// `line-break`
    pub line_break: LineBreakStrictness,
    /// `word-break`
    pub word_break: WordBreak,
    // <<< the layout proof injects a field directly above this line >>>
}

/// The initial font size, `css-fonts-4`'s `medium`.
pub const INITIAL_FONT_SIZE: f64 = 16.0;

impl ComputedStyle {
    /// Every property at its initial value, which is what the root inherits
    /// from.
    pub fn initial() -> Self {
        Self {
            color: Color::BLACK,
            font_family: vec![FontFamily::Serif],
            font_size: INITIAL_FONT_SIZE,
            font_style: FontStyle::Normal,
            font_variant: FontVariant::Normal,
            font_weight: 400,
            line_height: LineHeight::Normal,
            letter_spacing: Spacing::Normal,
            word_spacing: Spacing::Normal,
            text_align: TextAlign::Left,
            text_indent: LengthPercentage::ZERO,
            white_space: WhiteSpace::Normal,
            list_style_type: ListStyleType::Disc,
            visibility: Visibility::Visible,
            text_decoration: TextDecoration::None,
            display: Display::Inline,
            float: Float::None,
            clear: Clear::None,
            box_sizing: BoxSizing::ContentBox,
            width: Size::Auto,
            height: Size::Auto,
            margin: Sides::all(MarginValue::Length(LengthPercentage::ZERO)),
            padding: Sides::all(LengthPercentage::ZERO),
            border_width: Sides::all(0.0),
            border_style: Sides::all(BorderStyle::None),
            border_color: Sides::all(Color::BLACK),
            background_color: Color::TRANSPARENT,
            page_break_before: PageBreak::Auto,
            page_break_after: PageBreak::Auto,
            page_break_inside: PageBreakInside::Auto,
            // CSS 2.2 §13.3.2's own initial value for both, and the pair is
            // what makes them interact: a fragment must leave at least
            // `orphans` lines behind and carry at least `widows` lines
            // forward, so a two-line paragraph cannot be broken at all.
            orphans: 2,
            widows: 2,
            overflow_wrap: OverflowWrap::Normal,
            line_break: LineBreakStrictness::Auto,
            word_break: WordBreak::Normal,
            // <<< the layout proof's initial value goes here >>>
        }
    }

    /// §7.2: the inherited properties come from the parent's **computed**
    /// value, the rest go back to their initial one.
    ///
    /// Written as "start from initial, then copy the inherited fields across"
    /// rather than "start from the parent, then reset the rest", because the
    /// second shape leaks a new non-inherited property the day it is added and
    /// the first cannot.
    pub fn inherit_from(parent: &ComputedStyle) -> Self {
        let mut style = ComputedStyle::initial();
        style.color = parent.color;
        style.font_family = parent.font_family.clone();
        style.font_size = parent.font_size;
        style.font_style = parent.font_style;
        style.font_variant = parent.font_variant;
        style.font_weight = parent.font_weight;
        style.line_height = parent.line_height;
        style.letter_spacing = parent.letter_spacing;
        style.word_spacing = parent.word_spacing;
        style.text_align = parent.text_align;
        style.text_indent = parent.text_indent;
        style.white_space = parent.white_space;
        style.list_style_type = parent.list_style_type;
        style.visibility = parent.visibility;
        style.orphans = parent.orphans;
        style.widows = parent.widows;
        style.overflow_wrap = parent.overflow_wrap;
        style.line_break = parent.line_break;
        style.word_break = parent.word_break;
        style
    }
}

/// Writes one property into a computed style.
///
/// **The consumer decision 5 is about.** The `match` below is exhaustive with
/// no `_` arm, so a variant added to [`Property`] without an arm here is
/// `error[E0004]` — a property that is parsed and then ignored **does not
/// compile**. `tests/unimplemented_property_does_not_build.rs` injects that
/// defect and asserts the build fails.
///
/// `root_font_size` is the root element's computed `font-size`, for `rem`.
/// `style.font_size` is the element's own by the time anything relative to it
/// is applied, which is why [`cascade`] applies the `font-size` winner in a
/// pass of its own before this is called for anything else.
pub fn apply(property: &Property, style: &mut ComputedStyle, root_font_size: f64) {
    let font_size = style.font_size;
    match property {
        Property::Color(value) => style.color = *value,
        Property::FontFamily(value) => style.font_family = value.clone(),
        Property::FontSize(value) => {
            style.font_size = match value {
                SpecifiedFontSize::Absolute(px) => *px,
                // Relative to the parent's, which is what `style.font_size`
                // still holds: `inherit_from` has run and nothing has
                // overwritten it.
                SpecifiedFontSize::Relative(factor) => font_size * factor,
                SpecifiedFontSize::Root(factor) => root_font_size * factor,
                // CSS 2.1 §15.7's relative keywords, at the 1.2 ratio the
                // specification's own note gives.
                SpecifiedFontSize::Larger => font_size * 1.2,
                SpecifiedFontSize::Smaller => font_size / 1.2,
            }
        }
        Property::FontStyle(value) => style.font_style = *value,
        Property::FontVariant(value) => style.font_variant = *value,
        Property::FontWeight(value) => {
            style.font_weight = match value {
                SpecifiedWeight::Absolute(weight) => *weight,
                // `css-fonts-4` §2.2's table, which is not "add 100": from 400
                // `bolder` is 700, and from 700 it is 900.
                SpecifiedWeight::Bolder => match style.font_weight {
                    w if w < 350 => 400,
                    w if w < 550 => 700,
                    w if w < 900 => 900,
                    w => w,
                },
                SpecifiedWeight::Lighter => match style.font_weight {
                    w if w < 550 => 100,
                    w if w < 750 => 400,
                    w if w < 900 => 700,
                    _ => 700,
                },
            }
        }
        Property::LineHeight(value) => style.line_height = *value,
        Property::LetterSpacing(value) => {
            style.letter_spacing = spacing(value, font_size, root_font_size)
        }
        Property::WordSpacing(value) => {
            style.word_spacing = spacing(value, font_size, root_font_size)
        }
        Property::TextAlign(value) => style.text_align = *value,
        Property::TextIndent(value) => style.text_indent = value.compute(font_size, root_font_size),
        Property::TextDecoration(value) => style.text_decoration = *value,
        Property::WhiteSpace(value) => style.white_space = *value,
        Property::ListStyleType(value) => style.list_style_type = *value,
        Property::Visibility(value) => style.visibility = *value,
        Property::Display(value) => style.display = *value,
        Property::Float(value) => style.float = *value,
        Property::Clear(value) => style.clear = *value,
        Property::BoxSizing(value) => style.box_sizing = *value,
        Property::Width(value) => {
            style.width = match value {
                SpecifiedSize::Auto => Size::Auto,
                SpecifiedSize::Length(len) => Size::Length(len.compute(font_size, root_font_size)),
            }
        }
        Property::Height(value) => {
            style.height = match value {
                SpecifiedSize::Auto => Size::Auto,
                SpecifiedSize::Length(len) => Size::Length(len.compute(font_size, root_font_size)),
            }
        }
        Property::Margin(side, value) => {
            let computed = match value {
                SpecifiedMargin::Auto => MarginValue::Auto,
                SpecifiedMargin::Length(len) => {
                    MarginValue::Length(len.compute(font_size, root_font_size))
                }
            };
            style.margin.set(*side, computed);
        }
        Property::Padding(side, value) => {
            style
                .padding
                .set(*side, value.compute(font_size, root_font_size));
        }
        Property::BorderWidth(side, value) => {
            // A percentage is not a valid border width and the parser refuses
            // one, so anything arriving here resolves to pixels.
            let px = match value.compute(font_size, root_font_size) {
                LengthPercentage::Px(px) => px,
                LengthPercentage::Percent(_) => 0.0,
            };
            style.border_width.set(*side, px);
        }
        Property::BorderStyle(side, value) => style.border_style.set(*side, *value),
        Property::BorderColor(side, value) => style.border_color.set(*side, *value),
        Property::BackgroundColor(value) => style.background_color = *value,
        Property::PageBreakBefore(value) => style.page_break_before = *value,
        Property::PageBreakAfter(value) => style.page_break_after = *value,
        Property::PageBreakInside(value) => style.page_break_inside = *value,
        Property::Orphans(value) => style.orphans = *value,
        Property::Widows(value) => style.widows = *value,
        Property::OverflowWrap(value) => style.overflow_wrap = *value,
        Property::LineBreak(value) => style.line_break = *value,
        Property::WordBreak(value) => style.word_break = *value,
        // <<< the compile-time proof's fourth arm goes here >>>
    }
}

/// A specified spacing to a computed one.
///
/// A percentage is not a valid `letter-spacing` and the parser refuses one, so
/// anything arriving here resolves to pixels.
fn spacing(value: &SpecifiedSpacing, font_size: f64, root_font_size: f64) -> Spacing {
    match value {
        SpecifiedSpacing::Normal => Spacing::Normal,
        SpecifiedSpacing::Length(len) => match len.compute(font_size, root_font_size) {
            LengthPercentage::Px(px) => Spacing::Px(px),
            LengthPercentage::Percent(_) => Spacing::Normal,
        },
    }
}

/// What a cascade produced.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleTree {
    /// One computed style per element, in the order the elements were given.
    pub styles: Vec<ComputedStyle>,
    /// What could not be honoured, counted **by element** rather than by
    /// declaration: *"`float`, unimplemented, affected 412 elements"* is a
    /// sentence a host can show, and four hundred identical warnings is not.
    pub report: Report,
}

/// One sheet and where in the cascade it sits.
pub type Sheet<'a> = (Origin, &'a Stylesheet);

/// The whole cascade: match, sort, apply, inherit — one pass, in document
/// order.
///
/// `elements` **must** be in document order, parents before children. That is
/// not a convenience: §7.2 says a computed value inherits from the parent's
/// computed value, so the one-pass form is only correct if the parent is
/// already done. [`Element::parent`] returns an index into this same slice,
/// and an index that is not less than the child's is a caller error this
/// refuses by name rather than silently reading an uninitialised style.
pub fn cascade<E: Element>(
    sheets: &[Sheet<'_>],
    elements: &[E],
    limits: &Limits,
    budget: &mut Budget,
) -> Result<StyleTree, Refusal> {
    cascade_from(sheets, elements, limits, budget, &ComputedStyle::initial())
}

/// The same cascade, from a caller's own initial values.
///
/// **`css-cascade-5` §7.1's initial value is not a constant of the
/// specification: it is the user's.** `css-fonts-4` says `font-size: medium`,
/// and what `medium` *is* is a reading system's preference — which is why a
/// percentage on the root element resolves against it and why `html
/// { font-size: 100% }`, which one of gap 31's two measured producers writes
/// on every book, means *"whatever the reader chose"* rather than *"sixteen
/// pixels"*.
///
/// A build with no way to say so reads that rule as a reset to 16 and a host's
/// base font size stops mattering. So the initial style is a parameter, and
/// [`cascade`] is this function at [`ComputedStyle::initial`].
///
/// Only the **inherited** properties of `initial` can be observed: a
/// non-inherited one is reset by [`ComputedStyle::inherit_from`] for every
/// element below the root, and would be a value the root alone carried.
///
/// # Errors
/// Any [`Refusal`]: a cap, or a slice that is not in document order.
pub fn cascade_from<E: Element>(
    sheets: &[Sheet<'_>],
    elements: &[E],
    limits: &Limits,
    budget: &mut Budget,
    initial: &ComputedStyle,
) -> Result<StyleTree, Refusal> {
    if elements.len() > limits.max_elements {
        return Err(Refusal::TooManyElements {
            elements: elements.len(),
        });
    }
    for (index, element) in elements.iter().enumerate() {
        if let Some(parent) = element.parent() {
            if parent >= index {
                return Err(Refusal::NotInDocumentOrder { element: index });
            }
        }
    }

    let matcher = Matcher::build(sheets);
    let mut report = Report::default();
    let mut styles: Vec<ComputedStyle> = Vec::with_capacity(elements.len());
    let mut root_font_size = initial.font_size;

    for index in 0..elements.len() {
        let mut style = match elements[index].parent() {
            Some(parent) => ComputedStyle::inherit_from(&styles[parent]),
            None => initial.clone(),
        };
        let winners = matcher.winners(elements, index, &mut report, budget)?;
        apply_winners(&winners, &mut style, root_font_size);
        if elements[index].parent().is_none() {
            root_font_size = style.font_size;
        }
        styles.push(style);
    }

    Ok(StyleTree { styles, report })
}

/// Applies the winning declarations, `font-size` first.
///
/// The order is load-bearing and is not a tidiness choice: `text-indent: 2em`
/// is two of **this element's** ems, and this element's em is whatever
/// `font-size` won. A single pass in cascade order resolves the em against
/// whatever the parent had whenever `font-size` happens to sort later, which is
/// right about half the time and wrong silently the rest.
fn apply_winners(winners: &[Property], style: &mut ComputedStyle, root_font_size: f64) {
    for property in winners {
        if matches!(property, Property::FontSize(_)) {
            apply(property, style, root_font_size);
        }
    }
    for property in winners {
        if !matches!(property, Property::FontSize(_)) {
            apply(property, style, root_font_size);
        }
    }
}

/// The rules of every sheet, bucketed, with their origin and source order.
struct Matcher<'a> {
    /// `(origin, order, rule)` — `order` counts up across every sheet in the
    /// order the caller gave them, which is §6.1's sixth criterion.
    rules: Vec<(Origin, usize, &'a StyleRule)>,
    /// `handle` is an index into a flattened `(rule index, selector index)`
    /// list, so one bucket entry names one selector.
    selectors: Vec<(usize, usize)>,
    index: Index,
}

impl<'a> Matcher<'a> {
    fn build(sheets: &[Sheet<'a>]) -> Self {
        let mut rules = Vec::new();
        let mut selectors = Vec::new();
        let mut index = Index::default();
        let mut order = 0usize;
        for (origin, sheet) in sheets {
            for rule in &sheet.rules {
                let rule_at = rules.len();
                rules.push((*origin, order, rule));
                order += 1;
                for (selector_at, selector) in rule.selectors.iter().enumerate() {
                    let handle = selectors.len();
                    selectors.push((rule_at, selector_at));
                    index.insert(selector, handle);
                }
            }
        }
        Self {
            rules,
            selectors,
            index,
        }
    }

    /// The declarations that win for one element, one per property name.
    ///
    /// The winners come back **cloned** rather than borrowed, and that is not
    /// laziness: a `style=""` attribute is parsed inside this function and its
    /// declarations do not outlive it, so a borrowed return would tie every
    /// element's style to a vector that dies at the end of the call.
    fn winners<E: Element>(
        &self,
        elements: &[E],
        at: usize,
        report: &mut Report,
        budget: &mut Budget,
    ) -> Result<Vec<Property>, Refusal> {
        let mut matched: Vec<(CascadeKey, &Declared)> = Vec::new();
        for handle in self.index.candidates(&elements[at]) {
            let (rule_at, selector_at) = self.selectors[handle];
            let (origin, order, rule) = self.rules[rule_at];
            let selector = &rule.selectors[selector_at];
            if !selector::matches(selector, elements, at, budget)? {
                continue;
            }
            for declared in &rule.declarations {
                matched.push((
                    CascadeKey {
                        rank: rank(origin, declared.important),
                        attached: false,
                        specificity: selector.specificity,
                        order,
                    },
                    declared,
                ));
            }
        }

        // §6.1 criterion 3. The inline declarations are parsed here rather than
        // at open, because they belong to the element and not to a sheet, and
        // because a book with no `style=""` should pay nothing for the feature.
        let inline_owned: Vec<Declared> = match elements[at].inline_style() {
            Some(source) => crate::parse_inline(source, report, budget)?,
            None => Vec::new(),
        };
        for declared in &inline_owned {
            matched.push((
                CascadeKey {
                    rank: rank(Origin::Author, declared.important),
                    attached: true,
                    specificity: Specificity::ZERO,
                    order: usize::MAX,
                },
                declared,
            ));
        }

        // A stable sort, so two declarations with an identical key keep the
        // order they were pushed in — which for two declarations of the same
        // property inside one rule is the order the author wrote them.
        matched.sort_by_key(|(key, _)| *key);

        let mut winners: Vec<(&'static str, Property)> = Vec::new();
        for (_, declared) in &matched {
            match &declared.declaration {
                crate::property::Declaration::Known(property) => {
                    let name = property.name();
                    match winners.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = property.clone(),
                        None => winners.push((name, property.clone())),
                    }
                }
                // Counted **here**, where it is known to have reached an
                // element, rather than only at parse time. A `float: left` in
                // a rule that matches nothing is not a gap this book noticed.
                crate::property::Declaration::Unsupported { property, .. } => {
                    note(&mut report.unsupported, property);
                }
                crate::property::Declaration::Unknown { property } => {
                    note_owned(&mut report.unknown, property);
                }
            }
        }
        Ok(winners.into_iter().map(|(_, property)| property).collect())
    }
}

fn note(counts: &mut Vec<(&'static str, usize)>, name: &'static str) {
    match counts.iter_mut().find(|(n, _)| *n == name) {
        Some(slot) => slot.1 += 1,
        None => counts.push((name, 1)),
    }
}

fn note_owned(counts: &mut Vec<(String, usize)>, name: &str) {
    match counts.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => slot.1 += 1,
        None => counts.push((name.to_string(), 1)),
    }
}

/// The lazy alternative to the single pass, **for comparison only**.
///
/// It computes one element's style by recomputing every ancestor's from
/// scratch, which is what "resolve inheritance on demand" means once relative
/// units are in play: an `em` needs the parent's computed `font-size`, and that
/// needs the grandparent's, and so on to the root. The answer is the same and
/// the cost is **quadratic in tree depth** — a chain of 256 elements does 32 896
/// element-styles instead of 256.
///
/// It exists so `a_lazy_resolution_and_the_single_pass_agree` can assert the
/// two agree, which is what makes the choice between them a performance
/// decision rather than a correctness one. Nothing ships through here.
#[cfg(test)]
pub(crate) fn resolve_lazily<E: Element>(
    sheets: &[Sheet<'_>],
    elements: &[E],
    at: usize,
    budget: &mut Budget,
) -> Result<ComputedStyle, Refusal> {
    let matcher = Matcher::build(sheets);
    let mut report = Report::default();
    lazily(&matcher, elements, at, &mut report, budget)
}

#[cfg(test)]
fn lazily<E: Element>(
    matcher: &Matcher<'_>,
    elements: &[E],
    at: usize,
    report: &mut Report,
    budget: &mut Budget,
) -> Result<ComputedStyle, Refusal> {
    let mut style = match elements[at].parent() {
        Some(parent) => {
            let parent_style = lazily(matcher, elements, parent, report, budget)?;
            ComputedStyle::inherit_from(&parent_style)
        }
        None => ComputedStyle::initial(),
    };
    let root_font_size = {
        let mut root = at;
        while let Some(parent) = elements[root].parent() {
            root = parent;
        }
        if root == at {
            // The root's own `rem` is relative to its own computed size, which
            // is not known until it is computed — the initial value, which is
            // what a browser uses in the same situation.
            INITIAL_FONT_SIZE
        } else {
            lazily(matcher, elements, root, report, budget)?.font_size
        }
    };
    let winners = matcher.winners(elements, at, report, budget)?;
    apply_winners(&winners, &mut style, root_font_size);
    Ok(style)
}
