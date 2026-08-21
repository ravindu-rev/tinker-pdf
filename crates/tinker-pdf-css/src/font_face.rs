//! `@font-face` (`css-fonts-4` §4.1) and the `src` descriptor (§4.3) (gap 31,
//! milestone 9).
//!
//! # A descriptor is not a property
//!
//! Everything else this crate parses is a **property**: it has an initial
//! value, it cascades, it inherits, and [`crate::property::apply`] folds it
//! into a [`crate::cascade::ComputedStyle`]. A descriptor does none of those
//! things. `src` inside `@font-face` is not `src` on an element, `font-weight`
//! there names *which faces this file is* rather than which weight an element
//! wants, and `!important` is meaningless in the block. So the rule is parsed
//! into its own type here rather than through `Declaration`, and the cascade
//! never sees it.
//!
//! What the cascade sees is the **consequence**: a book that declares a family
//! and then sets `font-family: "That Family"` gets the file, and the join
//! between the two is made one crate up, where the container the `url()` names
//! can actually be opened. This crate has no file system and no archive and is
//! not acquiring one (ruling 8), so a [`FontFace`] carries the `src` list as
//! written and says nothing about whether any of it resolves.
//!
//! # The fallback list is a list, and its order is load-bearing
//!
//! §4.3 makes `src` a comma-separated list of alternatives *"in order of
//! preference"*, and a user agent takes the first one it can use. A build that
//! kept only the first entry would fail on every book that writes
//! `src: url(x.woff2) format("woff2"), url(x.otf) format("opentype")` — which
//! is what a modern producer writes, and which is exactly the shape where the
//! entry this build cannot use comes **first**. So the whole list is kept, and
//! choosing from it is the caller's.
//!
//! `format()` is a hint and not a promise: §4.3 says a user agent *"may"* use
//! it to avoid downloading a file it cannot use, and a file whose bytes say
//! something else is still whatever its bytes say. Both are recorded — the
//! hint here, the bytes above — because a `format()` that lies is a real book
//! and a build that trusted either one alone would be wrong about it.

use crate::parser::{BlockKind, ComponentValue};
use crate::property::FontStyle;
use crate::tokenizer::Token;

/// A `format()` hint on one `src` entry, §4.3's own keyword list.
///
/// [`FontFormat::Other`] keeps the string as written rather than folding every
/// unrecognised hint together, because a warning naming `"woff2"` and a warning
/// naming `"svg"` are two different facts about a book and a caller that reports
/// them as one cannot tell a producer which file to change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontFormat {
    /// `opentype`, and `opentype-variations`.
    OpenType,
    /// `truetype`, and `truetype-variations`.
    TrueType,
    /// `woff`, WOFF 1.0.
    Woff,
    /// `woff2`.
    Woff2,
    /// `embedded-opentype`, Microsoft's EOT.
    EmbeddedOpenType,
    /// `svg`, the SVG font format.
    Svg,
    /// `collection`, a TrueType collection.
    Collection,
    /// Anything else, as written.
    Other(String),
}

impl FontFormat {
    /// The keyword a specification spells, for a warning to name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            FontFormat::OpenType => "opentype",
            FontFormat::TrueType => "truetype",
            FontFormat::Woff => "woff",
            FontFormat::Woff2 => "woff2",
            FontFormat::EmbeddedOpenType => "embedded-opentype",
            FontFormat::Svg => "svg",
            FontFormat::Collection => "collection",
            FontFormat::Other(name) => name,
        }
    }

    /// Reads §4.3's keyword, which is written as a string or as an identifier.
    ///
    /// `css-fonts-4` moved `format()` from taking a `<string>` to taking a
    /// `<font-format>` keyword and browsers accept both, so both are read
    /// here. A build that took only the string would ignore the hint on every
    /// sheet written to the newer grammar and would then download a WOFF2 to
    /// discover what it already knew.
    #[must_use]
    pub fn parse(text: &str) -> FontFormat {
        // `opentype-variations` and its three relatives name the same
        // container with a variation axis in it: this build reads neither the
        // axis nor the default instance differently, so folding them is not a
        // loss of information.
        let base = text
            .trim()
            .trim_end_matches("-variations")
            .to_ascii_lowercase();
        match base.as_str() {
            "opentype" => FontFormat::OpenType,
            "truetype" => FontFormat::TrueType,
            "woff" => FontFormat::Woff,
            "woff2" => FontFormat::Woff2,
            "embedded-opentype" => FontFormat::EmbeddedOpenType,
            "svg" => FontFormat::Svg,
            "collection" | "truetype-collection" => FontFormat::Collection,
            _ => FontFormat::Other(text.trim().to_owned()),
        }
    }
}

/// One entry of a `src` list (§4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontSource {
    /// `url(<url>)`, with §4.3's optional `format()` hint.
    Url {
        /// The URL exactly as written, unresolved.
        url: String,
        /// The `format()` hint, if the sheet gave one.
        format: Option<FontFormat>,
    },
    /// `local(<name>)`: a face already installed on the reading system.
    ///
    /// Kept rather than dropped, because *"this book asked for a face from
    /// the system"* and *"this book's `src` was empty"* are different facts and
    /// a reading system with no installed faces has to be able to say which
    /// happened.
    Local(String),
}

/// One `@font-face` rule (§4.1), reduced to the four descriptors this build
/// reads.
///
/// `unicode-range`, `font-stretch`, `font-feature-settings`, `font-display`
/// and the rest are **not here**, and their absence is decision 5's rule
/// applied to a descriptor: a field nothing consumes is a field that lies.
/// `unicode-range` in particular would change which face covers a character
/// and this build's per-character fallback asks the font program itself, so
/// reading the descriptor and ignoring it would be worse than not reading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFace {
    /// The `font-family` descriptor: the name a `font-family` property has to
    /// mention for this face to be a candidate. Lower-cased, because §5's
    /// matching is ASCII case-insensitive on the family name.
    pub family: String,
    /// The `src` list, in the order the sheet wrote it.
    pub sources: Vec<FontSource>,
    /// The `font-weight` descriptor as a **range**, `css-fonts-4` §4.5.
    ///
    /// A single value is a range whose ends are equal, which is what the
    /// specification says a one-value descriptor means — so §5.2's matching
    /// has one shape to handle rather than two.
    pub weight: (u16, u16),
    /// The `font-style` descriptor (§4.6). An angle on `oblique` is read and
    /// discarded: this build has one oblique per family and no way to set a
    /// second at a different slant.
    pub style: FontStyle,
    /// The address of the sheet this rule was written in, so a relative `url()`
    /// can be resolved against it.
    ///
    /// `None` for a `<style>` element, whose base is the content document's.
    /// Carried on the rule rather than looked up later because an `@import`ed
    /// sheet's faces resolve against **that** sheet's address and not against
    /// the one that imported it — which is invisible until a book puts its
    /// fonts in a directory beside a nested stylesheet, and then it is every
    /// face in the book.
    pub base: Option<String>,
}

impl FontFace {
    /// Whether this face is a candidate for a request naming `family`.
    ///
    /// ASCII case-insensitive, per §5.2.
    #[must_use]
    pub fn matches_family(&self, family: &str) -> bool {
        self.family.eq_ignore_ascii_case(family)
    }

    /// How far this face is from the requested weight, §5.2's own rule read
    /// down to a distance.
    ///
    /// Zero when the request is inside the declared range. Otherwise the gap to
    /// the nearer end, **plus a penalty in the direction §5.2 deprioritises**:
    /// for a request at 400 or below, lighter weights are preferred over
    /// heavier; above 500, heavier over lighter. Without the penalty a request
    /// for 400 with faces at 300 and 500 would be a tie decided by declaration
    /// order, and a book whose bold face happened to be declared first would be
    /// set entirely in bold.
    #[must_use]
    pub fn weight_distance(&self, wanted: u16) -> u32 {
        let (low, high) = self.weight;
        if wanted >= low && wanted <= high {
            return 0;
        }
        let (distance, heavier) = if wanted < low {
            (u32::from(low - wanted), true)
        } else {
            (u32::from(wanted - high), false)
        };
        // §5.2: at 400 and below a lighter face wins, above 500 a heavier one.
        // 450 is the midpoint of the band the specification leaves to "check
        // weights below, then above" and putting the boundary there gives 400
        // the lighter-first rule and 500 the heavier-first one.
        let deprioritised = if wanted <= 450 { heavier } else { !heavier };
        distance + u32::from(deprioritised) * 10_000
    }

    /// How far this face is from the requested slope, §5.2.
    ///
    /// Italic and oblique are one step apart because §5.2 makes either an
    /// acceptable substitute for the other, and both are two steps from
    /// upright — so a request for italic with only an upright and an oblique
    /// available takes the oblique, which is what a reading system does.
    #[must_use]
    pub fn style_distance(&self, wanted: FontStyle) -> u32 {
        match (self.style, wanted) {
            (a, b) if a == b => 0,
            (FontStyle::Italic, FontStyle::Oblique) | (FontStyle::Oblique, FontStyle::Italic) => 1,
            _ => 2,
        }
    }
}

/// Parses one `@font-face` block into a rule, or `None` if §4.1 makes it
/// invalid.
///
/// §4.1: *"the `@font-face` rule is invalid if it does not contain a
/// `font-family` descriptor or a `src` descriptor"*, so a rule missing either
/// is dropped whole rather than kept with an empty field. A face with a family
/// and no `src` would otherwise shadow every later declaration of the same
/// family with a face that has no file.
///
/// The last declaration of a descriptor wins, which is the ordinary rule for a
/// declaration block and is what a browser does inside `@font-face` too.
#[must_use]
pub fn parse_rule(block: &[ComponentValue], base: Option<&str>) -> Option<FontFace> {
    let mut family: Option<String> = None;
    let mut sources: Option<Vec<FontSource>> = None;
    let mut weight = (400u16, 400u16);
    let mut style = FontStyle::Normal;

    for chunk in block.split(|v| matches!(v, ComponentValue::Token(Token::Semicolon))) {
        let mut values = chunk.iter().skip_while(|v| v.is_whitespace());
        let Some(ComponentValue::Token(Token::Ident(name))) = values.next() else {
            continue;
        };
        if !matches!(values.next(), Some(ComponentValue::Token(Token::Colon))) {
            continue;
        }
        let rest: Vec<ComponentValue> = values.cloned().collect();
        match name.to_ascii_lowercase().as_str() {
            "font-family" => {
                if let Some(name) = descriptor_family(&rest) {
                    family = Some(name);
                }
            }
            "src" => {
                let list = source_list(&rest);
                // An `src` that parses to nothing is not an `src`: §4.3 makes
                // a descriptor whose value is entirely invalid the same as one
                // that is absent, and §4.1 then makes the rule invalid. A
                // build that recorded `Some(vec![])` here would keep a face
                // with no file in it and lose the fallback the book meant.
                if !list.is_empty() {
                    sources = Some(list);
                }
            }
            "font-weight" => weight = descriptor_weight(&rest).unwrap_or(weight),
            "font-style" => style = descriptor_style(&rest).unwrap_or(style),
            _ => {}
        }
    }

    Some(FontFace {
        family: family?.to_ascii_lowercase(),
        sources: sources?,
        weight,
        style,
        base: base.map(str::to_owned),
    })
}

/// §4.2's `font-family` descriptor: one family name, a string or a run of
/// identifiers.
///
/// **One**, not a list. The property takes a comma-separated list and the
/// descriptor does not — a face is one family — and a build that reused the
/// property's parser here would silently accept `font-family: A, B` and
/// register the face under whichever one it happened to keep.
fn descriptor_family(values: &[ComponentValue]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for value in values {
        match value {
            ComponentValue::Token(Token::Whitespace) => {}
            ComponentValue::Token(Token::Str(text)) => {
                if !parts.is_empty() {
                    return None;
                }
                parts.push(text.clone());
            }
            ComponentValue::Token(Token::Ident(word)) => parts.push(word.clone()),
            // A comma means the sheet wrote a list, which the descriptor's
            // grammar does not allow.
            _ => return None,
        }
    }
    let joined = parts.join(" ");
    (!joined.trim().is_empty()).then(|| joined.trim().to_owned())
}

/// §4.3's `src`: a comma-separated list of `url()` or `local()` alternatives.
///
/// An entry that will not parse is **dropped and the rest of the list kept**,
/// which is §4.3's own recovery: *"if a component value is not recognised, the
/// entire src is invalid"* applies to the descriptor's grammar, but a browser
/// that dropped the whole list on one unreadable alternative would fail on
/// every sheet carrying a vendor-specific entry beside a usable one. The list
/// this build keeps is the alternatives it understood, in order.
fn source_list(values: &[ComponentValue]) -> Vec<FontSource> {
    let mut out = Vec::new();
    for entry in values.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        if let Some(source) = one_source(entry) {
            out.push(source);
        }
    }
    out
}

fn one_source(entry: &[ComponentValue]) -> Option<FontSource> {
    let mut values = entry.iter().filter(|v| !v.is_whitespace());
    let first = values.next()?;
    let url = match first {
        // The unquoted `url(x.otf)` form is one token; `url("x.otf")` is a
        // function with a string in it, and §4.3.4 of `css-syntax-3` is what
        // makes them two different shapes rather than one.
        ComponentValue::Token(Token::Url(url)) => url.clone(),
        ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("url") => {
            match arguments.iter().find(|v| !v.is_whitespace()) {
                Some(ComponentValue::Token(Token::Str(url))) => url.clone(),
                _ => return None,
            }
        }
        ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("local") => {
            let name = arguments
                .iter()
                .filter(|v| !v.is_whitespace())
                .map(|v| match v {
                    ComponentValue::Token(Token::Str(text) | Token::Ident(text)) => text.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<String>>()
                .join(" ");
            return Some(FontSource::Local(name.trim().to_owned()));
        }
        _ => return None,
    };

    let mut format = None;
    for value in values {
        match value {
            ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("format") => {
                for argument in arguments.iter().filter(|v| !v.is_whitespace()) {
                    if let ComponentValue::Token(Token::Str(text) | Token::Ident(text)) = argument {
                        format = Some(FontFormat::parse(text));
                        break;
                    }
                }
            }
            // `tech()` and any other modifier: read past rather than refused,
            // so a sheet using §4.3's newer grammar still yields its URL.
            _ => {}
        }
    }
    Some(FontSource::Url { url, format })
}

/// §4.5's `font-weight` descriptor: one value, or a range of two.
fn descriptor_weight(values: &[ComponentValue]) -> Option<(u16, u16)> {
    let mut numbers: Vec<u16> = Vec::new();
    for value in values.iter().filter(|v| !v.is_whitespace()) {
        match value.token()? {
            Token::Ident(word) if word.eq_ignore_ascii_case("normal") => numbers.push(400),
            Token::Ident(word) if word.eq_ignore_ascii_case("bold") => numbers.push(700),
            Token::Number { value, .. } => {
                // §4.5's own range. A descriptor outside it is invalid and the
                // whole declaration is dropped, rather than clamped: a face
                // declared at weight 5000 is a typo and giving it 1000 would
                // make it the heaviest face in the book.
                if !(1.0..=1000.0).contains(value) {
                    return None;
                }
                numbers.push(*value as u16);
            }
            _ => return None,
        }
    }
    match numbers.as_slice() {
        [single] => Some((*single, *single)),
        [low, high] if low <= high => Some((*low, *high)),
        _ => None,
    }
}

/// §4.6's `font-style` descriptor. An `oblique` angle is read past.
fn descriptor_style(values: &[ComponentValue]) -> Option<FontStyle> {
    let first = values.iter().find(|v| !v.is_whitespace())?;
    let Token::Ident(word) = first.token()? else {
        return None;
    };
    match word.to_ascii_lowercase().as_str() {
        "normal" => Some(FontStyle::Normal),
        "italic" => Some(FontStyle::Italic),
        "oblique" => Some(FontStyle::Oblique),
        _ => None,
    }
}

/// Whether a component value is a `{}` block, which `@font-face` needs and a
/// `@font-face;` with no block does not have.
#[must_use]
pub fn is_curly(value: &ComponentValue) -> bool {
    matches!(
        value,
        ComponentValue::Block {
            kind: BlockKind::Curly,
            ..
        }
    )
}
