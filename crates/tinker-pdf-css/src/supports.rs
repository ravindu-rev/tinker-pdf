//! `css-conditional-3` §6's `@supports`, evaluated against what this build
//! implements.
//!
//! # What "supports" means here
//!
//! §6.1: a declaration test is true *"if the UA supports the CSS property and
//! value given"*. That is a question about **this build**, not about CSS, and
//! the answer is the one the declaration would get if it were written in a
//! style rule: true where [`property::parse_declaration`] accepts the property
//! **and the value** — an implemented longhand or shorthand, one of §7.1's
//! defaulting keywords, a `content` this build generates — and false for a
//! name in [`property::UNSUPPORTED_PROPERTIES`], a name nobody cites, a value
//! refused by value (`transform: rotateX(1deg)`, a blurred shadow) and a value
//! that is not CSS at all. So `@supports (display: flex)` applies its rules
//! and `@supports (display: grid)` does not, which is what an author who wrote
//! a fallback beside it meant: the block for the engine that can, and the
//! fallback for the one that cannot.
//!
//! A book's `@supports` is therefore no longer a block skipped by name: before
//! October 2026 every one was `AtRuleUnsupported`, and a book whose layout lived
//! inside `@supports (display: flex)` lost it.
//!
//! # The grammar, and what falls outside it
//!
//! `not`, `and` and `or` over parenthesised conditions, the declaration test,
//! and `css-conditional-4` §2's `selector()`, true where the selector parses
//! and this build neither refuses nor warns about anything in it — `:hover`
//! parses everywhere and matches nothing in a paginated document, so an author
//! asking whether it is supported is answered no. Anything else in
//! parentheses, or any other function, is §6.1's `<general-enclosed>`, false.
//! A prelude outside the grammar — `and` and `or` mixed without parentheses,
//! `not` with nothing after it — makes the whole rule invalid, and it is
//! discarded and counted as any malformed rule is.
//!
//! Recursion is bounded by the component-value tree it walks, whose nesting
//! the parser caps at 256 levels.

use crate::parser::{BlockKind, ComponentValue};
use crate::property::{self, Parsed};
use crate::selector;
use crate::tokenizer::Token;

/// Whether an `@supports` rule with this prelude applies: `None` for a
/// prelude outside §6.1's grammar, which invalidates the rule.
#[must_use]
pub fn evaluate(prelude: &[ComponentValue], max_selector_parts: usize) -> Option<bool> {
    let significant: Vec<&ComponentValue> = prelude
        .iter()
        .filter(|value| !value.is_whitespace())
        .collect();
    condition(&significant, max_selector_parts)
}

/// `<supports-condition>`: `not` one operand, or operands joined by one of
/// `and` and `or` throughout.
fn condition(values: &[&ComponentValue], parts: usize) -> Option<bool> {
    let (first, rest) = values.split_first()?;
    if keyword(first) == Some("not") {
        return match rest {
            [operand] => in_parens(operand, parts).map(|value| !value),
            _ => None,
        };
    }
    let mut value = in_parens(first, parts)?;
    let mut joiner: Option<&str> = None;
    for pair in rest.chunks(2) {
        let [word, operand] = pair else {
            return None;
        };
        let word = keyword(word).filter(|word| matches!(*word, "and" | "or"))?;
        if joiner.is_some_and(|joined| joined != word) {
            return None;
        }
        joiner = Some(word);
        let next = in_parens(operand, parts)?;
        value = if word == "and" {
            value && next
        } else {
            value || next
        };
    }
    Some(value)
}

/// `<supports-in-parens>`.
fn in_parens(value: &ComponentValue, parts: usize) -> Option<bool> {
    match value {
        ComponentValue::Block {
            kind: BlockKind::Paren,
            values,
        } => {
            let inner: Vec<&ComponentValue> = values
                .iter()
                .filter(|value| !value.is_whitespace())
                .collect();
            if let Some(supported) = declaration(values) {
                return Some(supported);
            }
            // A nested condition, or `<general-enclosed>` — which is false
            // rather than invalid, so an unknown test does not take its
            // neighbours' rules with it.
            Some(condition(&inner, parts).unwrap_or(false))
        }
        ComponentValue::Function { name, arguments } => {
            if name.eq_ignore_ascii_case("selector") {
                return Some(selector_supported(arguments, parts));
            }
            Some(false)
        }
        _ => None,
    }
}

/// `<supports-decl>`'s inside: `name : value`, or `None` where it is not
/// shaped like a declaration at all.
fn declaration(values: &[ComponentValue]) -> Option<bool> {
    let mut rest = values.iter().skip_while(|value| value.is_whitespace());
    let ComponentValue::Token(Token::Ident(name)) = rest.next()? else {
        return None;
    };
    let mut rest = rest.skip_while(|value| value.is_whitespace()).peekable();
    if !matches!(rest.next(), Some(ComponentValue::Token(Token::Colon))) {
        return None;
    }
    let mut value: Vec<ComponentValue> = rest.cloned().collect();
    crate::parser::strip_important(&mut value);
    if value.iter().all(ComponentValue::is_whitespace) {
        return Some(false);
    }
    Some(matches!(
        property::parse_declaration(&name.to_ascii_lowercase(), &value),
        Parsed::Known(_) | Parsed::Defaulted { .. } | Parsed::Content(_)
    ))
}

/// `selector()`: one complex selector this build parses and reports nothing
/// about.
fn selector_supported(arguments: &[ComponentValue], parts: usize) -> bool {
    match selector::parse_list(arguments, parts) {
        Ok(list) => list.len() == 1 && selector::warnings(&list).is_empty(),
        Err(_) => false,
    }
}

/// An identifier's lower-cased text.
fn keyword(value: &ComponentValue) -> Option<&'static str> {
    match value {
        ComponentValue::Token(Token::Ident(word)) => ["not", "and", "or"]
            .into_iter()
            .find(|known| word.eq_ignore_ascii_case(known)),
        _ => None,
    }
}
