//! `css-syntax-3` §5's grammar, and the error recovery that is normative.
//!
//! # Recovery is the feature, not the tolerance
//!
//! §5.4.4's *"consume the remnants of a bad declaration"* discards to the next
//! top-level semicolon and §5.4.2 discards a malformed rule to the end of its
//! block. Both are `MUST`s, and gap 31's own scope section decided the
//! consequence: a stylesheet with one bad rule yields the rest, so a build that
//! refuses the sheet renders an unstyled book that looks entirely fine.
//!
//! **The counts are the point.** A build that silently discards has no way to
//! say how much it discarded, so [`Report::discarded_declarations`] and
//! [`Report::discarded_rules`] are separate numbers rather than one — a
//! stylesheet where every rule survives and half the declarations do not is a
//! different fact about a book from one where half the rules are gone.
//!
//! # The four at-rules that are answered rather than ignored
//!
//! `@media` is evaluated, because a build that ignores it applies every rule
//! inside every block or none, and **both are plausible and wrong**. `@import`
//! is resolved through a caller-supplied [`crate::ImportResolver`], with a
//! depth cap and a cycle guard that are two different facts. `@font-face` is
//! read into [`Stylesheet::font_faces`], because it is the one at-rule here
//! whose consumer is outside this crate.
//!
//! `@layer` is read, and what it yields is not a rule but a **position**: every
//! rule carries the layer it was written in ([`StyleRule::layer`], an index
//! into [`Stylesheet::layers`]) and `css-cascade-5` §6.1's fourth criterion
//! sorts on it. Reading the block as ordinary rules would invert that criterion
//! — a layered rule must lose to an unlayered one of *lower* specificity — and
//! dropping the block would lose the rules inside. The ordering is the feature;
//! the block is only where it is written down.
//!
//! **The nesting goes both ways**, which is why the at-rules are one recursion
//! rather than a shorter second list of the ones a block may hold:
//! `@layer a { @media print { … } }` and `@media print { @layer a { … } }` are
//! both books somebody writes, so [`Parse::nested_rules`] hands whatever it
//! finds back to [`Parse::apply_at_rule`].
//!
//! Every other at-rule is dropped with its name in a counted warning, which is
//! decision 5's `Unsupported` shape one level up from a property.

use crate::media::MediaContext;
use crate::property::{self, Declaration};
use crate::selector::{self, Selector};
use crate::tokenizer::{tokenize, Token};
use crate::{Budget, ImportResolver, Limits, Refusal, Warning};

/// Which bracket opened a simple block (§5.4.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// `{ … }`
    Curly,
    /// `( … )`
    Paren,
    /// `[ … ]`
    Square,
}

/// §5's component value: a preserved token, a function, or a simple block.
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    /// Any token that is not an opening bracket or a function token.
    Token(Token),
    /// `name( … )`.
    Function {
        /// The function's name, as written.
        name: String,
        /// Its arguments, still as component values.
        arguments: Vec<ComponentValue>,
    },
    /// A simple block.
    Block {
        /// Which bracket opened it.
        kind: BlockKind,
        /// Its contents.
        values: Vec<ComponentValue>,
    },
}

impl ComponentValue {
    /// The token this is, if it is a preserved one.
    pub fn token(&self) -> Option<&Token> {
        match self {
            ComponentValue::Token(token) => Some(token),
            _ => None,
        }
    }

    /// Whether this is a whitespace token, which every value parser skips.
    pub fn is_whitespace(&self) -> bool {
        matches!(self, ComponentValue::Token(Token::Whitespace))
    }
}

/// One declaration as the cascade sees it: what it says, and whether it shouts.
#[derive(Clone, Debug, PartialEq)]
pub struct Declared {
    /// The declaration, split three ways by decision 5.
    pub declaration: Declaration,
    /// `!important`.
    pub important: bool,
}

/// One part of a layer's name, `css-cascade-5` §6.4.2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerPart {
    /// A name the author wrote, and can write again to reach the same layer.
    Named(String),
    /// `@layer { … }`: a fresh layer every time, **never reachable by name
    /// again**. The number is its ordinal within the parse that met it, which
    /// is what keeps two of them distinct without making either nameable — an
    /// anonymous layer that could be found by a name would not be one.
    Anonymous(usize),
}

/// A layer's whole name, outermost part first.
///
/// `@layer a.b` and `@layer a { @layer b { … } }` are the same two parts and
/// the same layer, which is the point of resolving the name here rather than
/// keeping the spelling.
pub type LayerName = Vec<LayerPart>;

/// A qualified rule that parsed: a selector list and a declaration block.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRule {
    /// The selectors, each already carrying its specificity.
    pub selectors: Vec<Selector>,
    /// The declarations, in source order.
    pub declarations: Vec<Declared>,
    /// The layer this rule was written in, as an index into
    /// [`Stylesheet::layers`], or `None` for an unlayered rule.
    ///
    /// `None` is not "the first layer": `css-cascade-5` §6.4.2 puts unlayered
    /// styles in an implicit final layer, so a normal unlayered declaration
    /// beats **every** layered one at the same origin whatever its specificity,
    /// and an important unlayered declaration loses to every layered one.
    pub layer: Option<usize>,
}

/// A parsed stylesheet: rules in source order, and what was lost getting here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stylesheet {
    /// Every qualified rule that survived, in the order the cascade's last
    /// criterion needs — including rules pulled in by `@import`, spliced in at
    /// the position of the `@import` itself, which is what
    /// `css-cascade-5` §6.4.1 requires.
    pub rules: Vec<StyleRule>,
    /// Every `@font-face` that survived, in source order, including those
    /// pulled in by `@import` and those inside a `@media` block that matched
    /// (gap 31, milestone 9).
    ///
    /// A **separate list** rather than an entry in `rules`, because a
    /// `@font-face` is not a qualified rule: it has no selector, it matches no
    /// element, and the cascade never sees it. Splicing it into `rules` would
    /// give it a position in a sort whose criterion it has no value for.
    pub font_faces: Vec<crate::font_face::FontFace>,
    /// Every layer this sheet declared, **in the order each name was first
    /// seen**, including those declared by an `@import`ed sheet.
    ///
    /// First mention is what fixes a layer's position (`css-cascade-5` §6.4.2),
    /// so `@layer a, b;` before either block arrives orders them, and a second
    /// `@layer a { … }` further down appends to the `a` that is already there
    /// rather than moving it to the end.
    ///
    /// This is a list rather than an order: a name's **priority** is its
    /// position in the layer *tree* of its whole origin, which no single sheet
    /// can know — two author sheets share a layer called `a` and a UA sheet's
    /// `a` is a different layer entirely. [`crate::cascade`] builds that tree.
    pub layers: Vec<LayerName>,
    /// What was discarded, counted rather than swallowed.
    pub report: Report,
}

/// Everything a parse discarded, warned about, or could not implement.
///
/// Deduplicated with counts rather than one entry per occurrence — device 3 of
/// gap 31's honesty machinery: *"a book with `float: left` on four hundred
/// elements must produce one warning naming the property with a count"*.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Declarations discarded to the next semicolon (§5.4.4).
    pub discarded_declarations: usize,
    /// Rules discarded to the end of their block (§5.4.2), plus rules whose
    /// selector list would not parse at all (`selectors-4` §3.1).
    pub discarded_rules: usize,
    /// Warnings, deduplicated, each with the number of times it fired.
    pub warnings: Vec<(Warning, usize)>,
    /// Properties this build knows the name of and did not implement, or
    /// implemented at a value it does not — decision 5's `Unsupported`, per
    /// property name, with a count.
    pub unsupported: Vec<(&'static str, usize)>,
    /// Names no CSS specification this build cites defines: a typo, a vendor
    /// extension, or a custom property. Ordinary, and a different fact from
    /// the line above.
    pub unknown: Vec<(String, usize)>,
}

impl Report {
    /// Records a warning, or bumps the count of one already there.
    pub fn warn(&mut self, warning: Warning) {
        if let Some(slot) = self.warnings.iter_mut().find(|(w, _)| *w == warning) {
            slot.1 += 1;
        } else {
            self.warnings.push((warning, 1));
        }
    }

    fn note_unsupported(&mut self, property: &'static str) {
        if let Some(slot) = self.unsupported.iter_mut().find(|(p, _)| *p == property) {
            slot.1 += 1;
        } else {
            self.unsupported.push((property, 1));
        }
    }

    fn note_unknown(&mut self, property: &str) {
        if let Some(slot) = self.unknown.iter_mut().find(|(p, _)| p == property) {
            slot.1 += 1;
        } else {
            self.unknown.push((property.to_string(), 1));
        }
    }

    /// Merges another report into this one, keeping the counts additive. Used
    /// when an `@import` returns and when a caller sums a book's sheets.
    pub fn absorb(&mut self, other: Report) {
        self.discarded_declarations += other.discarded_declarations;
        self.discarded_rules += other.discarded_rules;
        for (warning, count) in other.warnings {
            if let Some(slot) = self.warnings.iter_mut().find(|(w, _)| *w == warning) {
                slot.1 += count;
            } else {
                self.warnings.push((warning, count));
            }
        }
        for (property, count) in other.unsupported {
            if let Some(slot) = self.unsupported.iter_mut().find(|(p, _)| *p == property) {
                slot.1 += count;
            } else {
                self.unsupported.push((property, count));
            }
        }
        for (property, count) in other.unknown {
            if let Some(slot) = self.unknown.iter_mut().find(|(p, _)| *p == property) {
                slot.1 += count;
            } else {
                self.unknown.push((property, count));
            }
        }
    }
}

/// A cursor over a token list, with §5's two consumption algorithms on it.
struct Stream {
    tokens: Vec<Token>,
    at: usize,
}

impl Stream {
    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        if token.is_some() {
            self.at += 1;
        }
        token
    }

    /// §5.4.7: consume a component value. The opening token is already read.
    fn consume_component_value(&mut self, token: Token, depth: usize) -> ComponentValue {
        // A block-nesting cap is not in gap 31's bounds table and does not need
        // to be: it bounds no allocation a token count does not already bound,
        // and this recursion is what would blow the stack. 256 is
        // `MAX_XML_DEPTH`'s number for the same reason — a stack, not a budget —
        // and past it the block is kept as its opening token, which is a
        // malformed construct the layer above already discards.
        if depth >= 256 {
            return ComponentValue::Token(token);
        }
        match token {
            Token::Function(name) => {
                let arguments = self.consume_until(Token::CloseParen, depth + 1);
                ComponentValue::Function { name, arguments }
            }
            Token::OpenParen => ComponentValue::Block {
                kind: BlockKind::Paren,
                values: self.consume_until(Token::CloseParen, depth + 1),
            },
            Token::OpenSquare => ComponentValue::Block {
                kind: BlockKind::Square,
                values: self.consume_until(Token::CloseSquare, depth + 1),
            },
            Token::OpenCurly => ComponentValue::Block {
                kind: BlockKind::Curly,
                values: self.consume_until(Token::CloseCurly, depth + 1),
            },
            other => ComponentValue::Token(other),
        }
    }

    /// Consumes component values until the matching closing token or EOF.
    fn consume_until(&mut self, closing: Token, depth: usize) -> Vec<ComponentValue> {
        let mut out = Vec::new();
        loop {
            let Some(token) = self.next() else { return out };
            if token == closing {
                return out;
            }
            out.push(self.consume_component_value(token, depth));
        }
    }
}

/// What a rule turned out to be.
enum RawRule {
    /// A qualified rule: prelude, then a `{}` block.
    Qualified {
        prelude: Vec<ComponentValue>,
        block: Vec<ComponentValue>,
    },
    /// An at-rule: name, prelude, and a block only if it had one.
    At {
        name: String,
        prelude: Vec<ComponentValue>,
        block: Option<Vec<ComponentValue>>,
    },
    /// A qualified rule that ran to EOF with no block. §5.4.2's parse error.
    Malformed,
}

/// The whole of one parse: source, options and the budget it spends.
struct Parse<'a> {
    limits: &'a Limits,
    media: &'a MediaContext,
    resolver: &'a dyn ImportResolver,
    report: Report,
    depth: usize,
    /// The `@import` hrefs on the stack, so a cycle is refused rather than
    /// recursed. A depth cap alone reads the same two files eight times.
    stack: Vec<String>,
    /// Every `@font-face` met, in source order across every sheet reached.
    ///
    /// On the parse rather than returned from [`Parse::sheet`] because an
    /// `@import`ed sheet's faces belong to the importing sheet's book: a
    /// caller gets one list and does not have to walk an import tree to find
    /// the faces a book actually declared.
    font_faces: Vec<crate::font_face::FontFace>,
    /// Every layer met, in first-mention order, for the same reason as the
    /// line above: an `@import`ed sheet's layers are the importing sheet's,
    /// because `css-cascade-5` §6.4.1 splices its rules in at the `@import`.
    layers: Vec<LayerName>,
    /// The layer whose block is being read, as an index into `layers`.
    ///
    /// Saved and restored around a nested block rather than passed down,
    /// because every route into [`Parse::build_rule`] needs it and only the two
    /// that open a layer change it.
    current_layer: Option<usize>,
    /// How many `@layer { … }` blocks have been met. An anonymous layer has no
    /// other identity, and this is it.
    anonymous: usize,
    /// How many at-rule blocks are open around the rule being read.
    nesting: usize,
}

/// How deep at-rule blocks may nest, and how many parts a layer's name may
/// have — one number, because for `@layer` they are the same depth written two
/// ways: `@layer a.b { … }` and `@layer a { @layer b { … } }` name one layer.
///
/// It is here beside the block-nesting cap rather than in [`crate::limits`],
/// and for that cap's own reason: **a stack, not a budget.** It bounds no
/// allocation something else does not already bound — every layer admitted is
/// charged one rule against [`Budget::spend_rule`], so the *number* of layers a
/// book may declare is [`crate::limits::MAX_CSS_RULES`] and needs no second
/// constant — and what it does bound is the recursion through
/// [`Parse::nested_rules`], which is what would blow the stack.
///
/// Sixteen rather than the block cap's 256 so that it can fire at all: a layer
/// nested 256 deep needs 257 nested `{`, which the block cap flattens into a
/// malformed construct first, and a cap that cannot fire is not a cap. The
/// deepest nesting either producer in the committed corpus writes is **zero**,
/// which `epub_css.rs` asserts rather than assumes.
pub(crate) const MAX_AT_RULE_DEPTH: usize = 16;

/// Parses one stylesheet, resolving `@import` through `resolver`.
///
/// `href` is this sheet's own address, used as the base for a relative
/// `@import` and as the cycle guard's key. `None` means the sheet came from a
/// `<style>` element and has no address of its own.
pub fn parse(
    bytes: &[u8],
    href: Option<&str>,
    resolver: &dyn ImportResolver,
    media: &MediaContext,
    limits: &Limits,
    budget: &mut Budget,
) -> Result<Stylesheet, Refusal> {
    let mut parse = Parse {
        limits,
        media,
        resolver,
        report: Report::default(),
        depth: 0,
        stack: href.map(|h| vec![h.to_string()]).unwrap_or_default(),
        font_faces: Vec::new(),
        layers: Vec::new(),
        current_layer: None,
        anonymous: 0,
        nesting: 0,
    };
    let rules = parse.sheet(bytes, href, budget)?;
    Ok(Stylesheet {
        rules,
        font_faces: parse.font_faces,
        layers: parse.layers,
        report: parse.report,
    })
}

impl Parse<'_> {
    /// One sheet's bytes to its rules, `@import`s spliced in place.
    fn sheet(
        &mut self,
        bytes: &[u8],
        href: Option<&str>,
        budget: &mut Budget,
    ) -> Result<Vec<StyleRule>, Refusal> {
        if bytes.len() > self.limits.max_bytes {
            return Err(Refusal::StylesheetTooLong { bytes: bytes.len() });
        }
        // §3.2's decode step. An EPUB's stylesheets are UTF-8 by OCF 3.3 §3.4,
        // and a byte that is not is a `Delim` rather than a refusal — losing a
        // book over one mis-encoded byte in a comment would be ruling 2 read
        // backwards.
        let text = String::from_utf8_lossy(bytes);
        let stripped = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let tokens = tokenize(stripped);
        budget.spend_tokens(tokens.len())?;
        let mut stream = Stream { tokens, at: 0 };
        let mut rules = Vec::new();
        // §3.3's `@import` ordering rule: an `@import` after any qualified rule
        // or any at-rule other than `@charset`/`@layer` is invalid. Tracked
        // rather than assumed, because a book that puts one at the bottom gets
        // a warning naming the fact instead of a stylesheet that silently
        // differs from every browser.
        let mut imports_still_allowed = true;
        while let Some(token) = stream.next() {
            match token {
                Token::Whitespace => {}
                // §5.4.1: CDO and CDC are dropped at the top level. They exist
                // so a 1996 stylesheet inside an HTML comment still parses.
                Token::Cdo | Token::Cdc => {}
                Token::AtKeyword(name) => {
                    let raw = self.consume_at_rule(&mut stream, name);
                    self.apply_at_rule(raw, href, &mut rules, &mut imports_still_allowed, budget)?;
                }
                other => {
                    imports_still_allowed = false;
                    match self.consume_qualified_rule(&mut stream, other) {
                        RawRule::Qualified { prelude, block } => {
                            if let Some(rule) = self.build_rule(&prelude, &block, budget)? {
                                budget.spend_rule()?;
                                rules.push(rule);
                            }
                        }
                        RawRule::Malformed => {
                            self.report.discarded_rules += 1;
                            break;
                        }
                        RawRule::At { .. } => unreachable!("only an at-keyword makes an at-rule"),
                    }
                }
            }
        }
        Ok(rules)
    }

    /// §5.4.2: prelude to the first top-level `{`, then the block.
    fn consume_qualified_rule(&mut self, stream: &mut Stream, first: Token) -> RawRule {
        let mut prelude = Vec::new();
        let mut token = first;
        loop {
            if token == Token::OpenCurly {
                let block = stream.consume_until(Token::CloseCurly, 1);
                return RawRule::Qualified { prelude, block };
            }
            prelude.push(stream.consume_component_value(token, 1));
            match stream.next() {
                Some(next) => token = next,
                // EOF before a block. §5.4.2 calls it a parse error and returns
                // nothing: everything read is discarded, which is the *only*
                // recovery available since there is no next block to resume at.
                None => return RawRule::Malformed,
            }
        }
    }

    /// §5.4.3: prelude to a `;` or a `{}` block, whichever comes first.
    fn consume_at_rule(&mut self, stream: &mut Stream, name: String) -> RawRule {
        let mut prelude = Vec::new();
        loop {
            match stream.next() {
                None => {
                    return RawRule::At {
                        name,
                        prelude,
                        block: None,
                    }
                }
                Some(Token::Semicolon) => {
                    return RawRule::At {
                        name,
                        prelude,
                        block: None,
                    }
                }
                Some(Token::OpenCurly) => {
                    let block = stream.consume_until(Token::CloseCurly, 1);
                    return RawRule::At {
                        name,
                        prelude,
                        block: Some(block),
                    };
                }
                Some(token) => prelude.push(stream.consume_component_value(token, 1)),
            }
        }
    }

    /// Turns an at-rule into rules, a warning, or nothing.
    fn apply_at_rule(
        &mut self,
        raw: RawRule,
        href: Option<&str>,
        rules: &mut Vec<StyleRule>,
        imports_still_allowed: &mut bool,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        let RawRule::At {
            name,
            prelude,
            block,
        } = raw
        else {
            unreachable!("apply_at_rule is only called with an at-rule")
        };
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "charset" => {}
            "media" => {
                *imports_still_allowed = false;
                let Some(block) = block else {
                    self.report.discarded_rules += 1;
                    return Ok(());
                };
                if crate::media::evaluate(&prelude, self.media) {
                    self.descend(block, href, rules, imports_still_allowed, budget)?;
                }
            }
            "import" => {
                if !*imports_still_allowed {
                    self.report.warn(Warning::ImportOutOfOrder);
                    return Ok(());
                }
                if block.is_some() {
                    // §3.3: `@import` takes no block. One with a block is
                    // invalid and is dropped whole.
                    self.report.discarded_rules += 1;
                    return Ok(());
                }
                self.import(&prelude, href, rules, budget)?;
            }
            // `css-cascade-5` §6.4.2. A statement (`@layer a;`, `@layer a, b;`)
            // declares order and nothing else; a block puts the rules inside it
            // in that layer. **The statement does not close the `@import`
            // window** and the block does — §3.3 lists `@layer` beside
            // `@charset` as the at-rules an `@import` may follow, and it means
            // the statement form, because a block holds rules.
            "layer" => match block {
                None => self.layer_statement(&prelude, budget)?,
                Some(block) => {
                    *imports_still_allowed = false;
                    self.layer_block(&prelude, block, href, rules, imports_still_allowed, budget)?;
                }
            },
            // `css-fonts-4` §4.1. Read rather than warned about, and the
            // difference from every other at-rule here is that this one has a
            // consumer: `tinker-pdf`'s EPUB reader opens the `url()` in the
            // container the sheet came out of.
            "font-face" => {
                *imports_still_allowed = false;
                let Some(block) = block else {
                    // §4.1 needs a block; `@font-face;` is a parse error.
                    self.report.discarded_rules += 1;
                    return Ok(());
                };
                self.font_face(&block, href, budget)?;
            }
            other => {
                *imports_still_allowed = false;
                self.report
                    .warn(Warning::AtRuleUnsupported(other.to_string()));
            }
        }
        Ok(())
    }

    /// One `@font-face` block to a face, or to a discarded rule.
    ///
    /// Charged against the **rule** budget rather than being unbounded, which
    /// is not an approximation: a `@font-face` costs a parse and a `Vec` the
    /// same way a style rule does, and a book that declared a hundred thousand
    /// of them would otherwise be refused for neither. It keeps the face list
    /// bounded by `MAX_CSS_RULES` with no second constant to justify.
    fn font_face(
        &mut self,
        block: &[ComponentValue],
        href: Option<&str>,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        budget.spend_rule()?;
        match crate::font_face::parse_rule(block, href) {
            Some(face) => self.font_faces.push(face),
            // §4.1: no `font-family` or no `src` makes the rule invalid, and an
            // invalid rule is a discarded one. Counted rather than silent,
            // because a producer that wrote `src` with a typo in the URL
            // function gets a book set in the fallback face and no reason why.
            None => self.report.discarded_rules += 1,
        }
        Ok(())
    }

    /// `@layer a;` and `@layer a, b, c;` — order declared before any block.
    ///
    /// §6.4.2's *first mention wins*: this is the form whose whole purpose is
    /// to fix positions ahead of the blocks, so that a book can order its
    /// layers at the top of a sheet and write them in whatever order suits it.
    fn layer_statement(
        &mut self,
        prelude: &[ComponentValue],
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        let Some(names) = layer_names(prelude) else {
            self.report.discarded_rules += 1;
            return Ok(());
        };
        // `@layer;` names nothing. There is no anonymous *statement* form —
        // an anonymous layer is a fresh layer holding rules, and a statement
        // holds none, so one would be a layer nothing could ever be put in.
        if names.is_empty() {
            self.report.discarded_rules += 1;
            return Ok(());
        }
        for name in names {
            self.declare_layer(name, budget)?;
        }
        Ok(())
    }

    /// `@layer name { … }`, `@layer a.b { … }` and `@layer { … }`.
    fn layer_block(
        &mut self,
        prelude: &[ComponentValue],
        block: Vec<ComponentValue>,
        href: Option<&str>,
        rules: &mut Vec<StyleRule>,
        imports_still_allowed: &mut bool,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        let Some(mut names) = layer_names(prelude) else {
            self.report.discarded_rules += 1;
            return Ok(());
        };
        // §6.4.2: the block form takes **one** name. `@layer a, b { … }` is
        // invalid rather than two layers with the same rules in both, and
        // reading it as either would be a guess about which the author meant.
        if names.len() > 1 {
            self.report.discarded_rules += 1;
            return Ok(());
        }
        let name = match names.pop() {
            Some(name) => name,
            // `@layer { … }`: a fresh layer, unreachable by name for ever.
            None => {
                let ordinal = self.anonymous;
                self.anonymous += 1;
                vec![LayerPart::Anonymous(ordinal)]
            }
        };
        let Some(at) = self.declare_layer(name, budget)? else {
            return Ok(());
        };
        let outer = self.current_layer;
        self.current_layer = Some(at);
        let result = self.descend(block, href, rules, imports_still_allowed, budget);
        self.current_layer = outer;
        result
    }

    /// Puts a layer in the order, or finds the position it is already in.
    ///
    /// **First mention wins** (§6.4.2), which is the clause that decides what a
    /// re-opened layer does: `@layer a, b;` then `@layer b { … } @layer a { … }`
    /// leaves `a` weaker than `b`, because the statement already placed them.
    /// A build that appended on every mention would order them by their blocks
    /// and get every stylesheet that declares its order up front backwards.
    ///
    /// The name is resolved against the layer being read, so `@layer b` inside
    /// `@layer a { … }` is `a.b` — the same layer `@layer a.b` names, which is
    /// why the two spellings cannot disagree.
    ///
    /// `None` comes back when the name is deeper than [`MAX_AT_RULE_DEPTH`],
    /// which is refused by name rather than flattened into its parent: a layer
    /// silently merged into the one above it inverts §6.1's fourth criterion
    /// exactly as reading the block as ordinary rules would.
    fn declare_layer(
        &mut self,
        name: LayerName,
        budget: &mut Budget,
    ) -> Result<Option<usize>, Refusal> {
        let mut full = match self.current_layer {
            Some(at) => self.layers[at].clone(),
            None => Vec::new(),
        };
        full.extend(name);
        if full.len() > MAX_AT_RULE_DEPTH {
            self.report.warn(Warning::AtRuleTooDeep);
            self.report.discarded_rules += 1;
            return Ok(None);
        }
        if let Some(at) = self.layers.iter().position(|seen| *seen == full) {
            return Ok(Some(at));
        }
        // Charged against the **rule** budget, for `@font-face`'s reason one
        // method up: a layer costs a parse and a `Vec` the way a style rule
        // does, so `MAX_CSS_RULES` bounds how many a book may declare and there
        // is no second constant to justify.
        budget.spend_rule()?;
        self.layers.push(full);
        Ok(Some(self.layers.len() - 1))
    }

    /// [`Parse::nested_rules`] with the recursion charged.
    ///
    /// The cap is on the **stack** rather than on the work: every layer this
    /// admits is already charged a rule, and what is left to bound is how many
    /// frames a book of nested blocks can open.
    fn descend(
        &mut self,
        block: Vec<ComponentValue>,
        href: Option<&str>,
        rules: &mut Vec<StyleRule>,
        imports_still_allowed: &mut bool,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        if self.nesting >= MAX_AT_RULE_DEPTH {
            self.report.warn(Warning::AtRuleTooDeep);
            self.report.discarded_rules += 1;
            return Ok(());
        }
        self.nesting += 1;
        let result = self.nested_rules(block, href, rules, imports_still_allowed, budget);
        self.nesting -= 1;
        result
    }

    /// The rules inside an at-rule's block — a `@media` that matched, or a
    /// `@layer`.
    fn nested_rules(
        &mut self,
        block: Vec<ComponentValue>,
        href: Option<&str>,
        rules: &mut Vec<StyleRule>,
        imports_still_allowed: &mut bool,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        let mut at = 0usize;
        while at < block.len() {
            // Whitespace before a rule is not part of it. Skipping it here is
            // what lets the at-keyword test below see `@page` at all: `@media
            // screen { @page … }` puts a whitespace token between the `{` and
            // the `@`, and a build that tested only the token it happened to be
            // standing on read the at-keyword as the first half of a selector
            // and discarded the construct as a malformed qualified rule —
            // which counts a discard and produces **no name**, so the warning
            // the branch below exists for never fired.
            if block[at].is_whitespace() {
                at += 1;
                continue;
            }
            // A nested at-rule — `@media print { @page {} }`,
            // `@layer a { @media print { … } }`, a `@font-face` inside either.
            // §5.4.3's shape is rebuilt from the component values and handed
            // **back** to `apply_at_rule`, so there is one list of at-rules
            // this build answers rather than a shorter second one for the ones
            // a block may hold. The second list is what dropped a `@layer`
            // nested in a `@media`, and it would have dropped a `@media` nested
            // in a `@layer` in the same way.
            if let ComponentValue::Token(Token::AtKeyword(name)) = &block[at] {
                let name = name.clone();
                at += 1;
                let start = at;
                let mut nested = None;
                while at < block.len() {
                    match &block[at] {
                        ComponentValue::Block {
                            kind: BlockKind::Curly,
                            values,
                        } => {
                            nested = Some(values.clone());
                            break;
                        }
                        ComponentValue::Token(Token::Semicolon) => break,
                        _ => at += 1,
                    }
                }
                let prelude: Vec<ComponentValue> = block[start..at].to_vec();
                at += 1;
                let raw = RawRule::At {
                    name,
                    prelude,
                    block: nested,
                };
                self.apply_at_rule(raw, href, rules, imports_still_allowed, budget)?;
                continue;
            }
            let start = at;
            while at < block.len()
                && !matches!(
                    block[at],
                    ComponentValue::Block {
                        kind: BlockKind::Curly,
                        ..
                    }
                )
            {
                at += 1;
            }
            if at == block.len() {
                // A prelude with no block, inside a `@media`. §5.4.2's parse
                // error again, and everything from `start` is discarded.
                if block[start..].iter().any(|v| !v.is_whitespace()) {
                    self.report.discarded_rules += 1;
                }
                return Ok(());
            }
            let prelude: Vec<ComponentValue> = block[start..at].to_vec();
            let ComponentValue::Block { values, .. } = &block[at] else {
                unreachable!("the loop above stopped at a curly block")
            };
            let declarations = values.clone();
            at += 1;
            if let Some(rule) = self.build_rule(&prelude, &declarations, budget)? {
                budget.spend_rule()?;
                rules.push(rule);
            }
        }
        Ok(())
    }

    /// `@import`, with the depth cap and the cycle guard that are two facts.
    fn import(
        &mut self,
        prelude: &[ComponentValue],
        base: Option<&str>,
        rules: &mut Vec<StyleRule>,
        budget: &mut Budget,
    ) -> Result<(), Refusal> {
        let mut values = prelude.iter().filter(|v| !v.is_whitespace());
        let Some(first) = values.next() else {
            self.report.discarded_rules += 1;
            return Ok(());
        };
        let target = match first {
            ComponentValue::Token(Token::Url(url)) => url.clone(),
            ComponentValue::Token(Token::Str(url)) => url.clone(),
            ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("url") => {
                match arguments.iter().find(|v| !v.is_whitespace()) {
                    Some(ComponentValue::Token(Token::Str(url))) => url.clone(),
                    _ => {
                        self.report.discarded_rules += 1;
                        return Ok(());
                    }
                }
            }
            _ => {
                self.report.discarded_rules += 1;
                return Ok(());
            }
        };
        let mut rest: Vec<ComponentValue> = values.cloned().collect();
        // §6.4.1's `layer` / `layer(<layer-name>)`, which puts the imported
        // sheet's rules in a layer and **must be taken off before the media
        // query list is read**. A build that left it there hands `layer(a)` to
        // the media evaluator, which cannot read it, calls the query false, and
        // drops the whole sheet without a word — a book that layers its imports
        // arriving unstyled and nothing anywhere saying why.
        //
        // `rest` carries no whitespace — the iterator above filtered it — so
        // the clause, if there is one, is the value the URL is followed by.
        let mut into_layer = None;
        let clause = match rest.first() {
            Some(ComponentValue::Token(Token::Ident(name)))
                if name.eq_ignore_ascii_case("layer") =>
            {
                // Bare `layer` is the anonymous form, exactly as `@layer { … }`
                // is: a fresh layer nothing can name afterwards.
                Some(Vec::new())
            }
            Some(ComponentValue::Function { name, arguments })
                if name.eq_ignore_ascii_case("layer") =>
            {
                match layer_names(arguments) {
                    // `layer()` names nothing and is a parse error, and so is a
                    // list: one `@import` reaches one layer.
                    Some(names) if names.len() == 1 => names.into_iter().next(),
                    _ => {
                        self.report.discarded_rules += 1;
                        return Ok(());
                    }
                }
            }
            _ => None,
        };
        if let Some(name) = clause {
            rest.remove(0);
            let name = if name.is_empty() {
                let ordinal = self.anonymous;
                self.anonymous += 1;
                vec![LayerPart::Anonymous(ordinal)]
            } else {
                name
            };
            match self.declare_layer(name, budget)? {
                Some(at) => into_layer = Some(at),
                None => return Ok(()),
            }
        }
        // The media query list on an `@import` is evaluated exactly as a
        // `@media` block's is; a build that honoured one and not the other
        // would apply a print-only sheet to a screen.
        if !rest.is_empty() && !crate::media::evaluate(&rest, self.media) {
            return Ok(());
        }
        if self.depth >= self.limits.max_import_depth {
            self.report.warn(Warning::ImportTooDeep);
            return Ok(());
        }
        let resolved = self.resolver.resolve(&target, base);
        let Some((address, bytes)) = resolved else {
            self.report.warn(Warning::ImportUnresolved);
            return Ok(());
        };
        // A cycle is two lines of CSS. Without this the depth cap turns an
        // infinite recursion into eight reads of the same pair of files, which
        // is not the same as refusing it and would not say so.
        if self.stack.iter().any(|seen| seen == &address) {
            self.report.warn(Warning::ImportCycle);
            return Ok(());
        }
        self.stack.push(address.clone());
        self.depth += 1;
        // Every rule the imported sheet yields is in the layer this `@import`
        // named — including the rules its own `@import`s bring in, which is
        // what saving and restoring rather than clearing gets right.
        let outer = self.current_layer;
        if into_layer.is_some() {
            self.current_layer = into_layer;
        }
        let nested = self.sheet(&bytes, Some(&address), budget);
        self.current_layer = outer;
        self.depth -= 1;
        self.stack.pop();
        rules.extend(nested?);
        Ok(())
    }

    /// A prelude and a block to a rule, or `None` if the selector list is
    /// invalid — `selectors-4` §3.1: one invalid selector invalidates the list,
    /// and the rule with it.
    fn build_rule(
        &mut self,
        prelude: &[ComponentValue],
        block: &[ComponentValue],
        budget: &mut Budget,
    ) -> Result<Option<StyleRule>, Refusal> {
        let parsed = selector::parse_list(prelude, self.limits.max_selector_parts);
        let selectors = match parsed {
            Ok(selectors) => selectors,
            Err(selector::Invalid::TooManyParts) => {
                self.report.warn(Warning::SelectorTooComplex);
                self.report.discarded_rules += 1;
                return Ok(None);
            }
            Err(selector::Invalid::UnknownPseudo(name)) => {
                self.report.warn(Warning::PseudoUnknown(name));
                self.report.discarded_rules += 1;
                return Ok(None);
            }
            Err(selector::Invalid::Malformed) => {
                self.report.discarded_rules += 1;
                return Ok(None);
            }
        };
        for warning in selector::warnings(&selectors) {
            self.report.warn(warning);
        }
        let declarations = self.declarations(block, budget)?;
        Ok(Some(StyleRule {
            selectors,
            declarations,
            // The layer whose block this rule is in, which is the whole of what
            // `@layer` leaves behind: §6.1's fourth criterion is a position,
            // and this is where the rule records the one it has.
            layer: self.current_layer,
        }))
    }

    /// §5.4.4 over a `{}` block's contents.
    fn declarations(
        &mut self,
        block: &[ComponentValue],
        budget: &mut Budget,
    ) -> Result<Vec<Declared>, Refusal> {
        declarations_from(block, &mut self.report, budget)
    }
}

/// Every declaration in a `{}` block or a `style=""` attribute.
///
/// §5.4.4: split at top-level semicolons and discard what is not
/// `ident : value`, counting each discard. It is a free function rather than a
/// method because a `style=""` attribute has exactly this grammar and no sheet
/// around it — and a second copy of this loop for the inline case is how the
/// two would come to disagree about what `!IMPORTANT` means.
fn declarations_from(
    block: &[ComponentValue],
    report: &mut Report,
    budget: &mut Budget,
) -> Result<Vec<Declared>, Refusal> {
    let mut out = Vec::new();
    for chunk in block.split(|v| matches!(v, ComponentValue::Token(Token::Semicolon))) {
        let mut values = chunk.iter().skip_while(|v| v.is_whitespace()).peekable();
        let Some(first) = values.next() else { continue };
        // An at-rule inside a declaration block — `@media` nested in a style
        // rule, which css-nesting allows and this build does not.
        if let ComponentValue::Token(Token::AtKeyword(name)) = first {
            report.warn(Warning::AtRuleUnsupported(name.to_ascii_lowercase()));
            continue;
        }
        let Some(Token::Ident(name)) = first.token() else {
            if chunk.iter().any(|v| !v.is_whitespace()) {
                report.discarded_declarations += 1;
            }
            continue;
        };
        while values.peek().is_some_and(|v| v.is_whitespace()) {
            values.next();
        }
        if values.next().and_then(|v| v.token()) != Some(&Token::Colon) {
            report.discarded_declarations += 1;
            continue;
        }
        let mut rest: Vec<ComponentValue> = values.cloned().collect();
        let important = strip_important(&mut rest);
        let name = name.to_ascii_lowercase();
        match property::parse_declaration(&name, &rest) {
            property::Parsed::Invalid => {
                // A declaration whose *value* will not parse at all is
                // discarded by §5.4.4 exactly as a syntactically malformed one
                // is, and it is counted in the same place. It is not
                // `Unsupported`: this build does implement the property.
                report.discarded_declarations += 1;
            }
            // A shorthand arrives here already expanded into its longhands,
            // and it is charged **once**: the budget counts what the author
            // wrote, so a book of `margin` shorthands and a book of four
            // longhands each cost what they look like.
            property::Parsed::Known(longhands) => {
                budget.spend_declaration()?;
                for property in longhands {
                    out.push(Declared {
                        declaration: Declaration::Known(property),
                        important,
                    });
                }
            }
            property::Parsed::Unsupported { property, value } => {
                budget.spend_declaration()?;
                report.note_unsupported(property);
                out.push(Declared {
                    declaration: Declaration::Unsupported { property, value },
                    important,
                });
            }
            property::Parsed::Unknown => {
                budget.spend_declaration()?;
                report.note_unknown(&name);
                out.push(Declared {
                    declaration: Declaration::Unknown { property: name },
                    important,
                });
            }
        }
    }
    Ok(out)
}

/// An `@layer` prelude to the names it declares, `css-cascade-5` §6.4.1.
///
/// `None` is an invalid prelude, which discards the whole rule — §5.4.2's
/// recovery, and the same answer for `@layer 3` as for `@layer a b`. An empty
/// list is the *absent* prelude, which is `@layer { … }`'s anonymous form and
/// `@layer;`'s parse error, and the two callers tell them apart because only
/// one of them has a block to put rules in.
///
/// The grammar is `<layer-name>#`, where a `<layer-name>` is
/// `<ident> ('.' <ident>)*`. The dots are `<delim-token>`s and the idents are
/// whole tokens, so `a.b` is three tokens and not one — which is what lets a
/// name be compared part by part rather than as a string two spellings could
/// disagree about.
///
/// **§6.4.1 forbids whitespace inside a layer name**, and that clause is not
/// decoration: `@layer a b` is two names with no comma between them, which is
/// invalid, and a reader that skipped whitespace would silently read it as the
/// one layer `a.b` — a layer the author never wrote, holding rules that were
/// meant for two.
fn layer_names(prelude: &[ComponentValue]) -> Option<Vec<LayerName>> {
    if prelude.iter().all(|value| value.is_whitespace()) {
        return Some(Vec::new());
    }
    let mut names = Vec::new();
    for chunk in prelude.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        let start = chunk.iter().position(|v| !v.is_whitespace())?;
        let end = chunk.iter().rposition(|v| !v.is_whitespace())? + 1;
        let mut name = LayerName::new();
        let mut expect_ident = true;
        for value in &chunk[start..end] {
            match value {
                ComponentValue::Token(Token::Ident(part)) if expect_ident => {
                    name.push(LayerPart::Named(part.clone()));
                    expect_ident = false;
                }
                ComponentValue::Token(Token::Delim('.')) if !expect_ident => expect_ident = true,
                _ => return None,
            }
        }
        if expect_ident {
            // A trailing dot: `@layer a.` names nothing after it.
            return None;
        }
        names.push(name);
    }
    Some(names)
}

/// Every component value in a token list, for a source with no rules around it.
///
/// **Public since `tinker-pdf-svg`**, and the reason is the one CONTRIBUTING
/// rule 1 is about. An SVG states its properties three ways — a presentation
/// attribute, a `style=""` attribute and a `<style>` element — and all three
/// are CSS values; but almost none of the *properties* are ones this crate
/// implements. `fill`, `stroke-linejoin` and `stop-color` are SVG's, so
/// [`crate::property::parse_declaration`] answers `Unknown` for each of them
/// and the value it could not use is gone. What that caller needs is §5.4.7's
/// structure and nothing above it, and the alternative to handing it over is a
/// **second component-value builder in this repository** — two readings of the
/// same clause, differing on an unterminated string or a stray bracket, with no
/// test that could see the difference.
///
/// What is deliberately *not* exported beside it is a raw rule reader.
/// Splitting a value list on `{ }` and `;` is structural walking over what this
/// returns rather than a second parse, and it belongs where the document
/// language's vocabulary is.
pub fn component_values(tokens: Vec<Token>) -> Vec<ComponentValue> {
    let mut stream = Stream { tokens, at: 0 };
    let mut out = Vec::new();
    while let Some(token) = stream.next() {
        out.push(stream.consume_component_value(token, 1));
    }
    out
}

/// Parses an element-attached declaration block — `style=""` in XHTML.
///
/// `css-cascade-5` §6.1's third criterion arrives through here, and it goes
/// through the **same** `declarations_from` as a rule's block rather than
/// through a second reader: an inline block that disagreed with a rule's about
/// `!important` or about error recovery would produce a cascade that is right
/// in a stylesheet and wrong on an attribute, and no page would say which.
pub fn parse_inline(
    source: &str,
    report: &mut Report,
    budget: &mut Budget,
) -> Result<Vec<Declared>, Refusal> {
    let tokens = tokenize(source);
    budget.spend_tokens(tokens.len())?;
    let values = component_values(tokens);
    declarations_from(&values, report, budget)
}

/// Removes a trailing `!important` and says whether it was there.
///
/// §5.4.4's rule is the last two non-whitespace values, and it is
/// case-insensitive: `!IMPORTANT` is important. A `!` followed by anything else
/// is an ordinary part of the value.
fn strip_important(values: &mut Vec<ComponentValue>) -> bool {
    let mut significant: Vec<usize> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_whitespace())
        .map(|(i, _)| i)
        .collect();
    let Some(&last) = significant.last() else {
        return false;
    };
    significant.pop();
    let Some(&before) = significant.last() else {
        return false;
    };
    let is_ident = matches!(&values[last], ComponentValue::Token(Token::Ident(name))
        if name.eq_ignore_ascii_case("important"));
    let is_bang = matches!(&values[before], ComponentValue::Token(Token::Delim('!')));
    if is_ident && is_bang {
        values.truncate(before);
        true
    } else {
        false
    }
}
