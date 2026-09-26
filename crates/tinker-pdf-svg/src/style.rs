//! SVG 1.1 §6's properties, from the three places a document states them.
//!
//! # Three sources, one order, and it is `css-cascade-5`'s
//!
//! An SVG says `fill: red` three ways, and **all three are in the fetched
//! corpus**: Illustrator writes a `<style>` element of `.st0 { fill: … }`
//! classes, Inkscape writes `style="fill:…"`, and a hand-written file writes
//! `fill="red"`. §6.4 puts them in one order:
//!
//! 1. **Presentation attributes**, which *"are considered to participate in the
//!    cascade with a specificity of zero, as if they were at the start of the
//!    author style sheet"* — so every `<style>` rule beats every presentation
//!    attribute, whatever the rule's selector.
//! 2. **`<style>` rules**, by `selectors-4` specificity and then source order.
//! 3. **`style=""`**, which beats both.
//!
//! `!important` inverts each comparison, which is `css-cascade-5` §6.1 and is
//! not re-derived here.
//!
//! # What is taken from `tinker-pdf-css` and what is not
//!
//! Taken: the tokenizer, §5.4.7's component values, the selector grammar,
//! `Specificity`, the matcher, and the `<color>` grammar — so `rebeccapurple`
//! and `rgb(0 128 0 / 40%)` are read by the crate that already knows how, and
//! this one holds no colour table.
//!
//! Not taken: `ComputedStyle`. Its properties are HTML's and the fifteen below
//! are SVG's, and there is almost no overlap — `fill` is not `color`,
//! `stroke-linejoin` has no CSS 2.1 equivalent, and half of `ComputedStyle` is
//! about boxes SVG does not have. The manifest says the edge *"buys the parsing
//! rather than the model"*, and this is that sentence in code.
//!
//! # Inheritance is a value, not a lookup
//!
//! Every resolved [`Style`] is complete: a child is resolved from its parent's
//! resolved style, once, on the way down. Nothing walks back up. §11's
//! inherited set is the whole of the painting properties *except* `opacity`,
//! `display`, `clip-path`, `mask` and `filter`, and getting that split wrong is
//! invisible in a flat document and wrong in every real one.

use tinker_pdf_css::parser::{component_values, BlockKind, ComponentValue};
use tinker_pdf_css::property::{self, Parsed, Property};
use tinker_pdf_css::selector::{self, Selector, Specificity, UiState};
use tinker_pdf_css::tokenizer::{tokenize, Token};
use tinker_pdf_css::{Budget, Element as CssElement, Refusal as CssRefusal};

use crate::document::{Node, Tree};
use crate::{Colour, FillRule, LineCap, LineJoin, TextAnchor};

// ---- the element side of a selector match ------------------------------------

/// The seven methods `tinker_pdf_css::Element` requires, answered for SVG.
///
/// Everything a document language gets to decide is decided here and nowhere
/// else — which is the whole value of that trait's boundary. Two of the
/// answers are SVG's rather than HTML's and are worth naming: an element's
/// classes come from `class` **case-sensitively**, because an SVG is XML; and
/// [`CssElement::is_link`] is `false` for every element, because `<a>` in SVG
/// is a container rather than a state a reading session can have visited.
impl CssElement for Node {
    fn local_name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    fn classes(&self) -> &[String] {
        &self.classes
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attr(name)
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
        // **Deliberately `None`.** `style=""` is §6.4's *third* criterion and
        // is applied by [`Style::resolve`] directly; answering it here would
        // put it into a matcher that has no place to rank it, and the two
        // would then disagree about a declaration nobody could see.
        None
    }

    fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    fn language(&self) -> Option<&str> {
        // XML's own attribute, which is the only one SVG has: there is no
        // bare `lang` in the SVG namespace.
        self.attr("xml:lang")
    }

    fn ui_state(&self) -> UiState {
        // A paginated drawing has no pointer, no focus and no form control.
        UiState::NONE
    }
}

// ---- a `<style>` element, read ------------------------------------------------

/// One declaration, still as component values.
///
/// Kept as values rather than as a string so the `<color>` grammar can be asked
/// for by [`property::parse_declaration`] without re-tokenizing — and so that
/// `fill: rgb(1, 2, 3)` and `fill:rgb(1,2,3)` cannot become two different
/// answers by way of two different re-readings.
#[derive(Clone, Debug)]
pub struct Declaration {
    /// The property name, lower-cased. **SVG property names are ASCII
    /// lower-case in the specification and case-insensitive in CSS**, and an
    /// author who writes `Fill:` means `fill`.
    pub name: String,
    /// The value.
    pub values: Vec<ComponentValue>,
    /// `!important`.
    pub important: bool,
}

/// One qualified rule of a `<style>` element.
struct Rule {
    selector: Selector,
    /// An index into [`Sheet::blocks`], so two selectors in one list share one
    /// declaration block rather than cloning it.
    block: usize,
    /// Position in the document, for `css-cascade-5` §6.1's last criterion.
    order: usize,
}

/// Every `<style>` element of a document, read once.
#[derive(Default)]
pub struct Sheet {
    rules: Vec<Rule>,
    blocks: Vec<Vec<Declaration>>,
    /// Whether an at-rule was skipped, so a caller can say so (ruling 10).
    pub at_rules: usize,
}

impl Sheet {
    /// Whether the document styled anything at all, which decides whether the
    /// matcher runs per element.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Splits a declaration block into declarations.
///
/// §5.4.4's recovery is the whole of the error handling and it is one line: a
/// malformed declaration is discarded **to the next semicolon**, which is what
/// splitting on `;` does. A build that refused the block would lose the rules
/// after one typo.
fn declarations(values: &[ComponentValue]) -> Vec<Declaration> {
    let mut out = Vec::new();
    for piece in values.split(|v| matches!(v, ComponentValue::Token(Token::Semicolon))) {
        let mut significant = piece.iter().skip_while(|v| v.is_whitespace());
        let Some(ComponentValue::Token(Token::Ident(name))) = significant.next() else {
            continue;
        };
        let rest: Vec<&ComponentValue> = significant.skip_while(|v| v.is_whitespace()).collect();
        let Some((ComponentValue::Token(Token::Colon), value)) = rest.split_first() else {
            continue;
        };
        let mut value: Vec<ComponentValue> = value.iter().map(|v| (*v).clone()).collect();
        // §5.4.4's `!important`: the last two non-whitespace values, and the
        // identifier is compared case-insensitively.
        let important = strip_important(&mut value);
        if value.iter().all(ComponentValue::is_whitespace) {
            continue;
        }
        out.push(Declaration {
            name: name.to_ascii_lowercase(),
            values: value,
            important,
        });
    }
    out
}

/// Removes a trailing `!important` and says whether it was there.
fn strip_important(values: &mut Vec<ComponentValue>) -> bool {
    let mut significant: Vec<usize> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_whitespace())
        .map(|(index, _)| index)
        .collect();
    let Some(&last) = significant.last() else {
        return false;
    };
    significant.pop();
    let Some(&before) = significant.last() else {
        return false;
    };
    let word = matches!(&values[last], ComponentValue::Token(Token::Ident(name))
        if name.eq_ignore_ascii_case("important"));
    let bang = matches!(&values[before], ComponentValue::Token(Token::Delim('!')));
    if word && bang {
        values.truncate(before);
        true
    } else {
        false
    }
}

/// One `style=""` attribute's declarations.
///
/// Public because a `<stop>`'s `stop-color` is as likely to be there as in an
/// attribute, and [`resolve`] is the wrong tool for one: a `<stop>` inherits
/// from the *gradient*, not from wherever the gradient is referenced.
#[must_use]
pub fn inline_declarations(text: &str) -> Vec<Declaration> {
    declarations(&component_values(tokenize(&format!("{text};"))))
}

/// Reads every `<style>` element in a tree into one sheet.
///
/// **One sheet for the document, not one per element.** §6.2 lets a `<style>`
/// sit anywhere, and two of them are one author stylesheet in source order —
/// which is what `css-cascade-5` §6.1's last criterion compares, so keeping
/// them apart would make the tie-break depend on which element a rule came
/// from.
#[must_use]
pub fn sheet(tree: &Tree, max_parts: usize) -> Sheet {
    let mut out = Sheet::default();
    for node in &tree.nodes {
        if !node.is_svg() || node.name != "style" {
            continue;
        }
        // §6.2: `type` other than `text/css` is a language this build does not
        // read. An absent one is CSS, which is SVG 2's default and what every
        // file that omits it means.
        if node
            .attr("type")
            .is_some_and(|kind| !kind.trim().eq_ignore_ascii_case("text/css"))
        {
            continue;
        }
        read_into(&mut out, &node.text(), max_parts);
    }
    out
}

/// One stylesheet's text, appended to a sheet.
fn read_into(sheet: &mut Sheet, text: &str, max_parts: usize) {
    let values = component_values(tokenize(text));
    let mut prelude: Vec<ComponentValue> = Vec::new();
    for value in values {
        match value {
            ComponentValue::Block {
                kind: BlockKind::Curly,
                values,
            } => {
                // An at-rule's block — `@media { … }` — is skipped whole
                // rather than read as a qualified rule, because its prelude is
                // not a selector list and `parse_list` would refuse it anyway.
                // Counted, so a caller can say a sheet was not read whole.
                if prelude
                    .iter()
                    .any(|v| matches!(v, ComponentValue::Token(Token::AtKeyword(_))))
                {
                    sheet.at_rules += 1;
                    prelude.clear();
                    continue;
                }
                let Ok(selectors) = selector::parse_list(&prelude, max_parts) else {
                    // §3.1: an invalid selector list invalidates the rule, and
                    // §5.4.2 discards it to the end of its block — which is
                    // where we already are.
                    prelude.clear();
                    continue;
                };
                let block = declarations(&values);
                prelude.clear();
                if block.is_empty() {
                    continue;
                }
                let at = sheet.blocks.len();
                sheet.blocks.push(block);
                for selector in selectors {
                    let order = sheet.rules.len();
                    sheet.rules.push(Rule {
                        selector,
                        block: at,
                        order,
                    });
                }
            }
            // A statement at-rule — `@import url(…);` — ends at its semicolon
            // and has no block.
            ComponentValue::Token(Token::Semicolon) => {
                if prelude
                    .iter()
                    .any(|v| matches!(v, ComponentValue::Token(Token::AtKeyword(_))))
                {
                    sheet.at_rules += 1;
                }
                prelude.clear();
            }
            other => prelude.push(other),
        }
    }
}

// ---- the property set ---------------------------------------------------------

/// SVG's `<paint>`, before a reference has been resolved.
///
/// §13.2's grammar is `none | currentColor | <color> | <funciri> [ none |
/// currentColor | <color> ]`, and the **fallback after a reference is not
/// decoration**: it is what a file says to use when the paint server is not
/// there, and a build that dropped it would draw nothing where the author had
/// written what to draw instead.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintSpec {
    /// `none`.
    None,
    /// A colour, already read.
    Solid(Colour),
    /// `currentColor`, which resolves against [`Style::colour`].
    Current,
    /// `url(#name)`, with the fallback that follows it.
    Reference(String, Box<PaintSpec>),
}

/// The properties this build resolves, all of them, for one element.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// `fill`.
    pub fill: PaintSpec,
    /// `fill-rule`.
    pub fill_rule: FillRule,
    /// `fill-opacity`, in `[0, 1]`.
    pub fill_opacity: f64,
    /// `stroke`.
    pub stroke: PaintSpec,
    /// `stroke-width`, in user units.
    pub stroke_width: f64,
    /// `stroke-linecap`.
    pub cap: LineCap,
    /// `stroke-linejoin`.
    pub join: LineJoin,
    /// `stroke-miterlimit`.
    pub miter_limit: f64,
    /// `stroke-dasharray`, empty for a solid line.
    pub dashes: Vec<f64>,
    /// `stroke-dashoffset`.
    pub dash_offset: f64,
    /// `stroke-opacity`, in `[0, 1]`.
    pub stroke_opacity: f64,
    /// `color`, which is what `currentColor` means.
    pub colour: Colour,
    /// `clip-rule`, which is `fill-rule` for a `<clipPath>`'s children and is
    /// a **separate property** — a shape used as a clip and as a fill can want
    /// two different rules.
    pub clip_rule: FillRule,
    /// `visibility`. §11.5 lays the element out and does not paint it, which
    /// is why it is not `display`.
    pub visible: bool,
    /// The product of every `opacity` from the root down to this element.
    ///
    /// **A product rather than a group**, and the flattening is named where it
    /// is observable — see [`crate::Warning::GroupOpacityFlattened`].
    pub opacity: f64,
    /// §13.2.4's `stop-color`, which only a `<stop>` reads.
    pub stop_colour: Colour,
    /// §13.2.4's `stop-opacity`.
    pub stop_opacity: f64,
    /// §14.3's `clip-path`, as the bare fragment name it referenced.
    pub clip_path: Option<String>,
    /// `font-family`, in the author's order, generics left in.
    pub families: Vec<String>,
    /// `font-size`, in user units, already resolved through `em` and `%`.
    ///
    /// Resolved on the way down rather than carried specified, and this is the
    /// one place this crate can do what `tinker-pdf-css` deliberately cannot:
    /// an `em` here is relative to the **parent's** computed size, and because
    /// a style is resolved from its parent's resolved style, the parent's size
    /// is sitting in this field when the child's declaration is applied.
    pub font_size: f64,
    /// `font-weight`, as a number in `[100, 900]`.
    pub font_weight: u16,
    /// Whether `font-style` is `italic` or `oblique`.
    pub font_italic: bool,
    /// §10.9's `text-anchor`.
    pub text_anchor: TextAnchor,
}

impl Default for Style {
    /// §11's initial values, every one of them.
    fn default() -> Self {
        Self {
            fill: PaintSpec::Solid(Colour {
                rgb: [0.0, 0.0, 0.0],
            }),
            fill_rule: FillRule::NonZero,
            fill_opacity: 1.0,
            stroke: PaintSpec::None,
            stroke_width: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
            stroke_opacity: 1.0,
            colour: Colour {
                rgb: [0.0, 0.0, 0.0],
            },
            clip_rule: FillRule::NonZero,
            visible: true,
            opacity: 1.0,
            stop_colour: Colour {
                rgb: [0.0, 0.0, 0.0],
            },
            stop_opacity: 1.0,
            clip_path: None,
            // §10.10's initial `font-family` is the user agent's, and CSS 2.1
            // §15.7 makes the initial `font-size` `medium`. The first is the
            // caller's to decide and is named rather than guessed: an empty
            // family list means *"whatever you have"*, which is exactly what a
            // reading system answers.
            families: Vec::new(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            text_anchor: TextAnchor::Start,
        }
    }
}

/// The properties this build reads, so a presentation attribute that is not one
/// is left alone rather than read as a property nobody consumes.
pub const PROPERTIES: [&str; 23] = [
    "fill",
    "fill-rule",
    "fill-opacity",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    "color",
    "clip-rule",
    "visibility",
    "opacity",
    "stop-color",
    "stop-opacity",
    "clip-path",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "text-anchor",
];

/// What resolving one declaration did.
pub enum Applied {
    /// The property was set.
    Set,
    /// The name is one of [`PROPERTIES`] and the value is not its grammar.
    Unreadable,
    /// Not a property this build reads.
    Ignored,
}

impl Style {
    /// This style as a child's starting point.
    ///
    /// **The whole of §11's inheritance split lives in this function**, and it
    /// is written as the exceptions rather than as a `fn inherited(name)`
    /// consulted from somewhere else — a rule enforced in two places hides the
    /// reachable half, which is the injection matrix's standing complaint.
    ///
    /// Every painting property inherits. Three do not, and each for its own
    /// reason:
    ///
    /// - `stop-color` and `stop-opacity` describe a `<stop>` and nothing else,
    ///   so a `<g stop-color="red">` around a gradient must not colour it.
    /// - `clip-path` clips **the element that states it**; inherited, every
    ///   descendant would be clipped again by the same path, which is the same
    ///   picture until a descendant moves.
    ///
    /// `opacity` is the fourth exception and it is not reset either: it is held
    /// here as the *product* from the root down, because §14.5 composes a
    /// group's opacity with everything under it.
    #[must_use]
    pub fn inherit(&self) -> Style {
        let initial = Style::default();
        Style {
            stop_colour: initial.stop_colour,
            stop_opacity: initial.stop_opacity,
            clip_path: None,
            ..self.clone()
        }
    }

    /// Applies one declaration.
    ///
    /// Returns whether the value was the property's grammar, so the caller can
    /// name what it dropped (ruling 10). **A value that is not the grammar
    /// leaves the inherited value standing**, which is `css-cascade-5` §5.2's
    /// answer and not this crate's: a `stroke-width: 3px3` must draw the width
    /// the parent said, not zero and not one.
    pub fn apply(&mut self, name: &str, values: &[ComponentValue]) -> Applied {
        let significant: Vec<&ComponentValue> =
            values.iter().filter(|v| !v.is_whitespace()).collect();
        if significant.is_empty() {
            return Applied::Unreadable;
        }
        let word = || match significant.first() {
            Some(ComponentValue::Token(Token::Ident(name))) if significant.len() == 1 => {
                Some(name.to_ascii_lowercase())
            }
            _ => None,
        };
        let ok = match name {
            "fill" | "stroke" => match paint(&significant) {
                Some(spec) => {
                    if name == "fill" {
                        self.fill = spec;
                    } else {
                        self.stroke = spec;
                    }
                    true
                }
                None => false,
            },
            "stop-color" => match colour(values) {
                Some(read) => {
                    self.stop_colour = read;
                    true
                }
                // §13.2.4 allows `currentColor` here too, and it means the
                // `color` in force on the `<stop>` — which is the one the
                // gradient element inherited, since a `<stop>` sets none.
                None => match significant.first() {
                    Some(ComponentValue::Token(Token::Ident(word)))
                        if word.eq_ignore_ascii_case("currentColor") =>
                    {
                        self.stop_colour = self.colour;
                        true
                    }
                    _ => false,
                },
            },
            "stop-opacity" => match alpha(&significant) {
                Some(value) => {
                    self.stop_opacity = value;
                    true
                }
                None => false,
            },
            "clip-path" => match significant.first() {
                Some(ComponentValue::Token(Token::Ident(word)))
                    if significant.len() == 1 && word.eq_ignore_ascii_case("none") =>
                {
                    self.clip_path = None;
                    true
                }
                Some(value) => match reference(value) {
                    Some(name) => {
                        self.clip_path = Some(name);
                        true
                    }
                    None => false,
                },
                None => false,
            },
            "color" => match colour(values) {
                // §11.2 makes `color`'s own `inherit` the only way to reach a
                // parent's, and this build implements no CSS-wide keyword —
                // so an unreadable `color` keeps the inherited one, which is
                // the same answer.
                Some(read) => {
                    self.colour = read;
                    true
                }
                None => false,
            },
            "fill-rule" | "clip-rule" => match word().as_deref() {
                Some("nonzero") => {
                    *rule_of(self, name) = FillRule::NonZero;
                    true
                }
                Some("evenodd") => {
                    *rule_of(self, name) = FillRule::EvenOdd;
                    true
                }
                _ => false,
            },
            "fill-opacity" | "stroke-opacity" | "opacity" => match alpha(&significant) {
                Some(value) => {
                    match name {
                        "fill-opacity" => self.fill_opacity = value,
                        "stroke-opacity" => self.stroke_opacity = value,
                        // §14.5's group opacity **multiplies** down the tree;
                        // it is the one property that is neither inherited nor
                        // reset, because it composes.
                        _ => self.opacity *= value,
                    }
                    true
                }
                None => false,
            },
            "stroke-width" => match length(&significant) {
                // §11.4: a negative width is an error and a zero one disables
                // the stroke. Both are kept as-is and answered where the
                // stroke is built, so the value a document stated survives to
                // whoever reads it.
                Some(value) if value >= 0.0 => {
                    self.stroke_width = value;
                    true
                }
                _ => false,
            },
            "stroke-dashoffset" => match length(&significant) {
                Some(value) => {
                    self.dash_offset = value;
                    true
                }
                None => false,
            },
            "stroke-miterlimit" => match number(&significant) {
                // §11.4: *"a value of less than one is an error"*.
                Some(value) if value >= 1.0 => {
                    self.miter_limit = value;
                    true
                }
                _ => false,
            },
            "stroke-linecap" => match word().as_deref() {
                Some("butt") => set(&mut self.cap, LineCap::Butt),
                Some("round") => set(&mut self.cap, LineCap::Round),
                Some("square") => set(&mut self.cap, LineCap::Square),
                _ => false,
            },
            "stroke-linejoin" => match word().as_deref() {
                Some("miter") => set(&mut self.join, LineJoin::Miter),
                Some("round") => set(&mut self.join, LineJoin::Round),
                Some("bevel") => set(&mut self.join, LineJoin::Bevel),
                _ => false,
            },
            "stroke-dasharray" => match dashes(&significant) {
                Some(list) => {
                    self.dashes = list;
                    true
                }
                None => false,
            },
            "font-family" => match families(&significant) {
                Some(list) => {
                    self.families = list;
                    true
                }
                None => false,
            },
            "font-size" => match font_size(&significant, self.font_size) {
                Some(value) if value > 0.0 => {
                    self.font_size = value;
                    true
                }
                _ => false,
            },
            "font-weight" => match weight(&significant, self.font_weight) {
                Some(value) => {
                    self.font_weight = value;
                    true
                }
                None => false,
            },
            "font-style" => match word().as_deref() {
                Some("normal") => set(&mut self.font_italic, false),
                // `css-fonts-4` §5.2 makes an italic and an oblique acceptable
                // matches for each other, and no caller in this repository has
                // both for one family — so they are one question here.
                Some("italic" | "oblique") => set(&mut self.font_italic, true),
                _ => false,
            },
            "text-anchor" => match word().as_deref() {
                Some("start") => set(&mut self.text_anchor, TextAnchor::Start),
                Some("middle") => set(&mut self.text_anchor, TextAnchor::Middle),
                Some("end") => set(&mut self.text_anchor, TextAnchor::End),
                _ => false,
            },
            "visibility" => match word().as_deref() {
                Some("visible") => set(&mut self.visible, true),
                // §11.5's `collapse` is `hidden` for everything that is not a
                // table row or column, and SVG has neither.
                Some("hidden" | "collapse") => set(&mut self.visible, false),
                _ => false,
            },
            _ => return Applied::Ignored,
        };
        if ok {
            Applied::Set
        } else {
            Applied::Unreadable
        }
    }
}

fn set<T>(slot: &mut T, value: T) -> bool {
    *slot = value;
    true
}

fn rule_of<'a>(style: &'a mut Style, name: &str) -> &'a mut FillRule {
    if name == "clip-rule" {
        &mut style.clip_rule
    } else {
        &mut style.fill_rule
    }
}

/// §13.2's `<paint>`.
fn paint(values: &[&ComponentValue]) -> Option<PaintSpec> {
    let first = values.first()?;
    if let ComponentValue::Token(Token::Ident(word)) = first {
        if values.len() == 1 {
            let lower = word.to_ascii_lowercase();
            if lower == "none" {
                return Some(PaintSpec::None);
            }
            // `currentColor` is spelled with a capital C in the specification
            // and lower-cased by half the files that use it, and CSS makes a
            // keyword case-insensitive either way.
            if lower == "currentcolor" {
                return Some(PaintSpec::Current);
            }
        }
    }
    if let Some(name) = reference(first) {
        // §13.2's fallback: what to paint with when the server is not there.
        let rest: Vec<&ComponentValue> = values[1..].to_vec();
        let fallback = if rest.is_empty() {
            PaintSpec::None
        } else {
            paint(&rest)?
        };
        return Some(PaintSpec::Reference(name, Box::new(fallback)));
    }
    let owned: Vec<ComponentValue> = values.iter().map(|v| (*v).clone()).collect();
    colour(&owned).map(PaintSpec::Solid)
}

/// A `<funciri>` naming a fragment in this document, as the bare name.
///
/// **Only a same-document reference is read.** `url(other.svg#g)` names a
/// resource this crate cannot fetch — it has no container, no filesystem and
/// no network — so it is not a reference here and falls through to the
/// fallback, which is what a file that supplied one asked for.
fn reference(value: &ComponentValue) -> Option<String> {
    let text = match value {
        ComponentValue::Token(Token::Url(text)) => text.clone(),
        ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("url") => {
            match arguments.iter().find(|v| !v.is_whitespace())? {
                ComponentValue::Token(Token::Str(text)) => text.clone(),
                _ => return None,
            }
        }
        _ => return None,
    };
    text.trim().strip_prefix('#').map(str::to_owned)
}

/// A `<color>`, through `tinker-pdf-css`'s own grammar.
///
/// Asked for as the `color` property rather than reimplemented, so
/// `rebeccapurple`, `#abc`, `rgb()` and `hsl()` are read once in this
/// repository and this crate holds no colour table.
fn colour(values: &[ComponentValue]) -> Option<Colour> {
    let Parsed::Known(properties) = property::parse_declaration("color", values) else {
        return None;
    };
    properties.iter().find_map(|property| match property {
        Property::Color(read) => Some(Colour {
            rgb: [
                f64::from(read.r) / 255.0,
                f64::from(read.g) / 255.0,
                f64::from(read.b) / 255.0,
            ],
        }),
        _ => None,
    })
}

/// An `<opacity-value>`: §11.6's number or a percentage, clamped to `[0, 1]`.
fn alpha(values: &[&ComponentValue]) -> Option<f64> {
    if values.len() != 1 {
        return None;
    }
    let value = match values[0] {
        ComponentValue::Token(Token::Number { value, .. }) => *value,
        ComponentValue::Token(Token::Percentage(percent)) => percent / 100.0,
        _ => return None,
    };
    // §11.6: *"any values outside the range … are clamped"* — a clamp, not an
    // error, so `fill-opacity: 2` is opaque rather than dropped.
    value.is_finite().then(|| value.clamp(0.0, 1.0))
}

/// A plain `<number>`.
fn number(values: &[&ComponentValue]) -> Option<f64> {
    if values.len() != 1 {
        return None;
    }
    match values[0] {
        ComponentValue::Token(Token::Number { value, .. }) if value.is_finite() => Some(*value),
        _ => None,
    }
}

/// A `<length>` in a property value, where the unit is a CSS dimension token
/// rather than the tail of an attribute string.
fn length(values: &[&ComponentValue]) -> Option<f64> {
    if values.len() != 1 {
        return None;
    }
    match values[0] {
        ComponentValue::Token(Token::Number { value, .. }) if value.is_finite() => Some(*value),
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            crate::document::length(&format!("{value}{unit}"), None)
        }
        _ => None,
    }
}

/// §10.10's `font-family`: a comma-separated list of names.
///
/// A quoted name is one family however many spaces it holds, and an unquoted
/// one is *"a sequence of identifiers"* joined by single spaces — which is
/// `css-fonts-4` §2.2's rule and the reason `font-family: Times New Roman`
/// with no quotes is one family and not three.
fn families(values: &[&ComponentValue]) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for value in values {
        match value {
            ComponentValue::Token(Token::Comma) => {
                if !current.is_empty() {
                    out.push(current.join(" "));
                    current.clear();
                }
            }
            ComponentValue::Token(Token::Str(name) | Token::Ident(name)) => {
                current.push(name.clone());
            }
            _ => return None,
        }
    }
    if !current.is_empty() {
        out.push(current.join(" "));
    }
    (!out.is_empty()).then_some(out)
}

/// §10.10's `font-size`, against the **parent's** computed size.
///
/// `em`, `ex` and a percentage are all relative to it, which is why the basis
/// is a parameter: [`crate::document::length`] resolves an `em` against the
/// initial sixteen because the lengths *it* reads are on the root, and
/// `font-size` is the one property where that is the wrong answer.
fn font_size(values: &[&ComponentValue], parent: f64) -> Option<f64> {
    if values.len() != 1 {
        return None;
    }
    let finite = |value: f64| value.is_finite().then_some(value);
    match values[0] {
        ComponentValue::Token(Token::Number { value, .. }) if value.is_finite() => Some(*value),
        ComponentValue::Token(Token::Percentage(percent)) => finite(parent * percent / 100.0),
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            let lower = unit.to_ascii_lowercase();
            match lower.as_str() {
                "em" => finite(value * parent),
                // CSS 2.1 §4.3.2: half an `em` where the face does not say,
                // and no face is visible from here.
                "ex" => finite(value * parent / 2.0),
                _ => crate::document::length(&format!("{value}{lower}"), None),
            }
        }
        // CSS 2.1 §15.7's absolute keywords, on the specification's own 1.2
        // ratio from `medium`. Written out rather than computed from a ratio,
        // so a reader can check each against the table.
        ComponentValue::Token(Token::Ident(word)) => match word.to_ascii_lowercase().as_str() {
            "xx-small" => Some(9.0),
            "x-small" => Some(10.0),
            "small" => Some(13.0),
            "medium" => Some(16.0),
            "large" => Some(18.0),
            "x-large" => Some(24.0),
            "xx-large" => Some(32.0),
            "larger" => finite(parent * 1.2),
            "smaller" => finite(parent / 1.2),
            _ => None,
        },
        _ => None,
    }
}

/// `css-fonts-4` §2.2's `font-weight`.
fn weight(values: &[&ComponentValue], parent: u16) -> Option<u16> {
    if values.len() != 1 {
        return None;
    }
    match values[0] {
        ComponentValue::Token(Token::Number { value, .. }) if *value >= 1.0 && *value <= 1000.0 => {
            Some(*value as u16)
        }
        ComponentValue::Token(Token::Ident(word)) => match word.to_ascii_lowercase().as_str() {
            "normal" => Some(400),
            "bold" => Some(700),
            // §2.2's relative keywords are a **table** rather than an addition,
            // and the difference shows at the ends: `bolder` than 900 is still
            // 900, and a build that added a hundred would ask for a weight no
            // face has.
            "bolder" => Some(match parent {
                0..=300 => 400,
                301..=500 => 700,
                _ => 900,
            }),
            "lighter" => Some(match parent {
                0..=500 => 100,
                501..=700 => 400,
                _ => 700,
            }),
            _ => None,
        },
        _ => None,
    }
}

/// §11.4's `stroke-dasharray`.
///
/// Three rules, and each is a real file. `none` is the empty list. **An odd
/// number of dashes is repeated to make it even**, so `stroke-dasharray: 5`
/// is five on, five off — a build that took it literally would draw a solid
/// line. And a list that is all zeroes is `none`, because a dash pattern that
/// never advances is a line a rasterizer cannot draw.
fn dashes(values: &[&ComponentValue]) -> Option<Vec<f64>> {
    if let [ComponentValue::Token(Token::Ident(word))] = values {
        if word.eq_ignore_ascii_case("none") {
            return Some(Vec::new());
        }
    }
    let mut out = Vec::new();
    for value in values {
        if matches!(value, ComponentValue::Token(Token::Comma)) {
            continue;
        }
        let one = length(&[value])?;
        // §11.4: *"a negative value is an error"*, and one bad dash
        // invalidates the whole list rather than being dropped from it. The
        // comparison is written the positive way round because a `NaN` must
        // refuse the list too, and `!(x >= 0.0)` and `x < 0.0` differ there.
        if one < 0.0 || one.is_nan() {
            return None;
        }
        out.push(one);
    }
    if out.is_empty() || out.iter().all(|dash| *dash == 0.0) {
        return Some(Vec::new());
    }
    if out.len() % 2 == 1 {
        let doubled = out.clone();
        out.extend(doubled);
    }
    Some(out)
}

// ---- resolution ---------------------------------------------------------------

/// Where a ranked declaration lives.
///
/// An index pair rather than a clone: `cover.svg` in the fetched corpus is
/// fifty-eight rules against three hundred and thirty-nine paths, and cloning a
/// matched block per element would copy the same component values twenty
/// thousand times over just to sort them.
enum Source {
    /// `sheet.blocks[block][at]`.
    Rule { block: usize, at: usize },
    /// `own[at]` — a presentation attribute or a `style=""` declaration, which
    /// belong to the element and are tokenized once here.
    Own { at: usize },
}

/// What a resolution had to say about itself.
pub struct Resolved {
    /// The style.
    pub style: Style,
    /// Property names whose value was not the property's grammar, deduplicated.
    pub unreadable: Vec<String>,
}

/// Resolves one element's style from its parent's.
///
/// §6.4's three sources in §6.4's order, with `!important` inverting each
/// comparison. `parent` is the **resolved** style of the element above, so
/// inheritance costs one clone rather than a walk up the tree.
///
/// The ranking is one sort rather than three passes, and that is not a
/// refactor: a build that applied each source in turn is right only while no
/// `!important` exists anywhere, because an important presentation attribute
/// beats a normal `style=""` — which is the half of `css-cascade-5` §6.1 that a
/// first implementation drops.
///
/// # Errors
/// [`CssRefusal`] when selector matching crosses the CSS budget, which is a
/// document with more style than the caller agreed to spend on it.
pub fn resolve(
    tree: &Tree,
    index: usize,
    sheet: &Sheet,
    parent: &Style,
    budget: &mut Budget,
) -> Result<Resolved, CssRefusal> {
    let node = &tree.nodes[index];
    let mut style = parent.inherit();
    let mut unreadable: Vec<String> = Vec::new();
    let mut own: Vec<Declaration> = Vec::new();
    // (important, specificity, order, source). `Specificity` derives `Ord` as
    // the lexicographic tuple `selectors-4` §15 asks for, so no amount of class
    // beats one id.
    let mut ranked: Vec<(bool, Specificity, usize, Source)> = Vec::new();

    // 1. §6.4's presentation attributes: specificity zero, before every rule.
    for (name, value) in &node.attributes {
        let lower = name.to_ascii_lowercase();
        if !PROPERTIES.contains(&lower.as_str()) {
            continue;
        }
        // The trailing `;` is what makes a one-declaration block out of a bare
        // value, so the same splitter reads an attribute and a rule. Two
        // readers here would disagree about `!important` on an attribute, and
        // §6.4 says an attribute may carry one.
        let values = component_values(tokenize(&format!("{lower}:{value};")));
        for declaration in declarations(&values) {
            ranked.push((
                declaration.important,
                Specificity::ZERO,
                0,
                Source::Own { at: own.len() },
            ));
            own.push(declaration);
        }
    }

    // 2. `<style>` rules.
    for rule in &sheet.rules {
        if !selector::matches(&rule.selector, &tree.nodes, index, budget)? {
            continue;
        }
        for at in 0..sheet.blocks[rule.block].len() {
            ranked.push((
                sheet.blocks[rule.block][at].important,
                rule.selector.specificity,
                rule.order + 1,
                Source::Rule {
                    block: rule.block,
                    at,
                },
            ));
        }
    }

    // 3. `style=""`, which §6.4 puts above every selector whatever its
    // specificity. Ranked with an unreachable specificity rather than applied
    // afterwards, so that an important rule still beats a normal inline
    // declaration — §6.1's reversal, and the reason there is one sort.
    if let Some(text) = node.style.as_deref() {
        let values = component_values(tokenize(&format!("{text};")));
        for declaration in declarations(&values) {
            ranked.push((
                declaration.important,
                INLINE,
                usize::MAX,
                Source::Own { at: own.len() },
            ));
            own.push(declaration);
        }
    }

    // A stable sort, so declarations that tie on all three keys keep the order
    // they were pushed in — which is source order, which is §6.1's own last
    // criterion.
    ranked.sort_by_key(|(important, specificity, order, _)| (*important, *specificity, *order));
    for (.., source) in &ranked {
        let declaration = match source {
            Source::Rule { block, at } => &sheet.blocks[*block][*at],
            Source::Own { at } => &own[*at],
        };
        if let Applied::Unreadable = style.apply(&declaration.name, &declaration.values) {
            if !unreadable.contains(&declaration.name) {
                unreadable.push(declaration.name.clone());
            }
        }
    }

    Ok(Resolved { style, unreadable })
}

/// The rank a `style=""` declaration sorts at.
///
/// Above every selector `selectors-4` can spell, which is what §6.4 asks for,
/// and it is a constant rather than a fourth pass so that `!important` still
/// orders around it. An id selector contributes one to `a` and a selector list
/// is capped at `MAX_CSS_SELECTOR_PARTS` compounds, so this is unreachable from
/// markup by four orders of magnitude.
const INLINE: Specificity = Specificity {
    a: u32::MAX,
    b: u32::MAX,
    c: u32::MAX,
};
