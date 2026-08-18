//! What a fixed page's elements are made of: a budget, a small subtree, and
//! the four value grammars every attribute in 11 to 15 is written in (gap 30,
//! milestone 6).
//!
//! # Why there is a subtree at all, when the plan says there is not
//!
//! Gap 30's design section says *"a pull parser, not a tree … a tree would
//! allocate the whole of a fixed page before anything looked at it, and a
//! fixed page is the one part of this format whose size is chosen by the
//! file"*, and that is exactly right and is what [`super::paint`] does: a page
//! is streamed, and the drawing for an element is emitted when its end tag
//! arrives.
//!
//! What cannot be streamed is a **property element**. XAML writes a value that
//! will not fit in an attribute as a child named `Owner.Property` —
//! `<Path.Data>`, `<Canvas.Resources>`, `<LinearGradientBrush.GradientStops>`
//! — and a value is a value: a `GradientStop` list has to be complete before a
//! gradient can be built from it, and a `ResourceDictionary` has to be
//! complete before a `{StaticResource}` can be looked up in it. So this module
//! materialises **those** subtrees and nothing else, each one charged against
//! [`Budget::element`], and the drawable tree — `Canvas` inside `Canvas`
//! inside `FixedPage` — is never built.
//!
//! # The budget is two totals and both are the file's to choose
//!
//! Ruling 1's own sentence, from `5adf502`: *"depth is not work once the
//! recursion branches"*, and gap 30's version of it — **a per-item cap is not
//! a total once the file chooses how many items there are**. A page may hold a
//! million elements and one `Data` attribute may hold twenty million segments,
//! so both are counted across the **whole document**, spent and never
//! refunded. The constants and their ledgers are in [`super`].

use tinker_pdf_xml::{Element, Error as XmlError, Event, Reader};

// ---- The budget -------------------------------------------------------------

/// What is left of the document's two work totals.
///
/// One value rather than two counters, because both are spent from the same
/// places and a caller that had to remember to thread the second would
/// eventually not.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    elements: usize,
    segments: usize,
    glyphs: usize,
}

/// A total ran out. The package is refused rather than truncated: a document
/// that quietly drew nine tenths of itself is this gap's own failure mode, and
/// a page count that is right while the pages are not is worse than a refusal
/// that says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exhausted;

impl Budget {
    /// A budget with the three totals set.
    #[must_use]
    pub fn new(elements: usize, segments: usize, glyphs: usize) -> Budget {
        Budget {
            elements,
            segments,
            glyphs,
        }
    }

    /// Charges one markup element — one materialised into a subtree, or one
    /// visited by the drawing walk. Both are work and both are counted.
    ///
    /// # Errors
    /// [`Exhausted`] when the document's element total is spent.
    pub fn element(&mut self) -> Result<(), Exhausted> {
        self.elements = self.elements.checked_sub(1).ok_or(Exhausted)?;
        Ok(())
    }

    /// Charges `count` path segments.
    ///
    /// # Errors
    /// [`Exhausted`] when the document's segment total is spent.
    pub fn segments(&mut self, count: usize) -> Result<(), Exhausted> {
        self.segments = self.segments.checked_sub(count).ok_or(Exhausted)?;
        Ok(())
    }

    /// Charges `count` glyphs placed by a `Glyphs` run.
    ///
    /// A third total, and it is one for [`Budget::segments`]'s reason rather
    /// than by analogy: one `Indices` attribute in a part this build already
    /// admits holds millions of glyph mappings, and neither the element count
    /// nor the segment count sees a single one of them.
    ///
    /// **Charged before the mappings are materialised, not after**, which is
    /// `tinker-pdf-zip`'s `Budget` posture — a permit is what has been
    /// promised rather than what happened to arrive. One `GlyphMapping` is
    /// eighty bytes of value out of one byte of markup, so a cap checked after
    /// the parse would be a cap checked after the allocation it exists to
    /// stop.
    ///
    /// # Errors
    /// [`Exhausted`] when the document's glyph total is spent.
    pub fn glyphs(&mut self, count: usize) -> Result<(), Exhausted> {
        self.glyphs = self.glyphs.checked_sub(count).ok_or(Exhausted)?;
        Ok(())
    }

    /// How many elements are left, for the tests that prove the total is a
    /// total and not a per-page cap.
    #[must_use]
    pub fn elements_left(&self) -> usize {
        self.elements
    }

    /// How many segments are left.
    #[must_use]
    pub fn segments_left(&self) -> usize {
        self.segments
    }

    /// How many glyphs are left.
    #[must_use]
    pub fn glyphs_left(&self) -> usize {
        self.glyphs
    }
}

// ---- A materialised property-element subtree --------------------------------

/// One element and its children, with the attributes a value grammar needs.
///
/// Namespaces are collapsed to two questions — *is this element in one of the
/// two dialects* and *what is its `x:Key`* — because those are the only two a
/// fixed page asks. Everything else in XPS markup is an unprefixed attribute
/// in no namespace at all, which is what [`Node::attr`] looks up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    /// The local name, with no prefix.
    pub local: String,
    /// Whether the element's namespace is one of the two dialects. An element
    /// from somewhere else is kept in the tree — dropping it would silently
    /// change a `GradientStop` list's length — and never acted on.
    pub xps: bool,
    /// `x:Key`, in either dialect's resource-dictionary-key namespace.
    pub key: Option<String>,
    /// Attributes in no namespace, in document order.
    pub attrs: Vec<(String, String)>,
    /// Children, in document order.
    pub children: Vec<Node>,
}

impl Node {
    /// An unprefixed attribute's value.
    #[must_use]
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| name == local)
            .map(|(_, value)| value.as_str())
    }

    /// The first child in a dialect namespace with this local name.
    #[must_use]
    pub fn child(&self, local: &str) -> Option<&Node> {
        self.children
            .iter()
            .find(|node| node.xps && node.local == local)
    }

    /// The part after the dot of `Owner.Property` — XAML's property-element
    /// spelling (14.1), which is a value and never a thing to draw — when the
    /// owner is exactly `owner`.
    ///
    /// Matched on the whole segment rather than as a prefix, so a producer's
    /// `CanvasThing.Value` is not read as a `Canvas`'s property.
    #[must_use]
    pub fn property_of(&self, owner: &str) -> Option<&str> {
        let (found, property) = self.local.split_once('.')?;
        (found == owner && !property.is_empty()).then_some(property)
    }
}

/// What reading a subtree can go wrong with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trouble {
    /// The markup itself is not well formed, or a bound inside
    /// `tinker-pdf-xml` fired. The page is unreadable from here on.
    Markup,
    /// One of gap 30's two work totals is spent. The **package** is refused.
    Exhausted,
}

impl From<Exhausted> for Trouble {
    fn from(_: Exhausted) -> Trouble {
        Trouble::Exhausted
    }
}

impl From<XmlError> for Trouble {
    fn from(_: XmlError) -> Trouble {
        Trouble::Markup
    }
}

/// One element on its own, with no children read.
///
/// What a streamed element looks like when only its attributes matter — a
/// `Canvas`'s transform and opacity are on the start tag, and its children are
/// the page's own drawing rather than part of its value.
#[must_use]
pub fn leaf(element: &Element<'_>) -> Node {
    node_of(element)
}

/// Turns one already-started element and everything under it into a [`Node`].
///
/// The caller has taken the [`Event::Start`] and hands it over; this consumes
/// events up to and including the matching [`Event::End`]. Text, comments and
/// processing instructions are dropped — no value grammar in 11 to 15 is
/// carried in character data, and the corpus proves the whitespace is real:
/// WPF indents inside `FixedPage.Resources` with no `xml:space` at all.
///
/// **The element itself is not charged**, only what is under it: every caller
/// has already charged the start tag it is holding, and charging here as well
/// would make one element cost two.
///
/// # Errors
/// [`Trouble::Markup`] for markup that will not read, [`Trouble::Exhausted`]
/// when the document's element total is spent.
pub fn subtree(
    reader: &mut Reader<'_>,
    start: &Element<'_>,
    budget: &mut Budget,
) -> Result<Node, Trouble> {
    let mut root = node_of(start);
    // An empty-element tag still produces its own `End`, so the loop below
    // consumes it and never needs to know which spelling the file used —
    // which is `tinker-pdf-xml`'s own contract and the one milestone 3
    // misread, reading `<Types><Default/><Default/></Types>` as a nest six
    // deep.
    let mut stack: Vec<Node> = Vec::new();
    loop {
        let Some(event) = reader.next() else {
            // The reader ran out before the element closed. A well-formed
            // document cannot do that, so this is markup that will not read.
            return Err(Trouble::Markup);
        };
        match event? {
            Event::Start(element) => {
                budget.element()?;
                stack.push(node_of(&element));
            }
            Event::End(_) => match stack.pop() {
                Some(done) => match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => root.children.push(done),
                },
                None => return Ok(root),
            },
            _ => {}
        }
    }
}

/// Skips one already-started element and everything under it, charging what it
/// costs.
///
/// The same walk as [`subtree`] and it keeps nothing. Used where an element is
/// recognised and not drawn — a `Glyphs` run, whose milestone is 7 — because
/// the alternative is letting its children fall through to the drawing walk
/// and be read as siblings of the element that contains them.
///
/// # Errors
/// As [`subtree`].
pub fn skip_subtree(reader: &mut Reader<'_>, budget: &mut Budget) -> Result<(), Trouble> {
    let mut depth = 0usize;
    loop {
        let Some(event) = reader.next() else {
            return Err(Trouble::Markup);
        };
        match event? {
            Event::Start(_) => {
                budget.element()?;
                depth += 1;
            }
            Event::End(_) => {
                if depth == 0 {
                    return Ok(());
                }
                depth -= 1;
            }
            _ => {}
        }
    }
}

/// The resource-dictionary-key namespace, per dialect.
///
/// Both are `<dialect namespace>/resourcedictionary-key`, which is what the
/// corpus carries on `xmlns:x` in all eight packages.
fn is_key_namespace(namespace: &str) -> bool {
    namespace
        == concat!(
            "http://schemas.microsoft.com/xps/2005/06",
            "/resourcedictionary-key"
        )
        || namespace
            == concat!(
                "http://schemas.openxps.org/oxps/v1.0",
                "/resourcedictionary-key"
            )
}

fn node_of(element: &Element<'_>) -> Node {
    let mut node = Node {
        local: element.local().to_string(),
        xps: super::dialect_of(element.namespace()).is_some(),
        ..Node::default()
    };
    for attribute in element.attributes() {
        match attribute.namespace() {
            None => node
                .attrs
                .push((attribute.local().to_string(), attribute.value().to_string())),
            Some(namespace) if is_key_namespace(namespace) && attribute.local() == "Key" => {
                node.key = Some(attribute.value().to_string());
            }
            // `xml:lang`, `xmlns:x` and anything else a producer hangs on an
            // element: carried past rather than refused, because nothing in
            // 11 to 15 reads one and refusing an attribute a file is allowed
            // to write would refuse every real package.
            Some(_) => {}
        }
    }
    node
}

// ---- The value grammars -----------------------------------------------------

/// The largest magnitude any coordinate, radius or matrix entry may have.
///
/// **Not a resource bound, and it stays out of `bounds_ledger.rs` for
/// [`super::MAX_PAGE_UNITS`]'s reason**: it allocates nothing and refuses
/// nothing a document could want. What it is instead is the point past which
/// a number stops being a coordinate — 10⁹ XPS units is about 165 miles — and
/// past which the fixed-point rasteriser ruling 4 fixes would be doing
/// arithmetic on a number no page could hold. A value past it makes its
/// element's geometry unreadable, which is a named warning rather than a
/// silent clamp.
pub const MAX_COORDINATE: f64 = 1.0e9;

/// Whether a number is one this reader will carry into a content stream.
#[must_use]
pub fn usable(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_COORDINATE
}

/// One number, over the whole string.
#[must_use]
pub fn number(text: &str) -> Option<f64> {
    let value = text.trim().parse::<f64>().ok()?;
    usable(value).then_some(value)
}

/// `x,y`, or `x y` — both spellings appear in one corpus, and neither is
/// ambiguous.
#[must_use]
pub fn pair(text: &str) -> Option<(f64, f64)> {
    let values = numbers(text)?;
    match values.as_slice() {
        [x, y] => Some((*x, *y)),
        _ => None,
    }
}

/// Every number in a comma-and-whitespace-separated list.
#[must_use]
pub fn numbers(text: &str) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for field in text.split([',', ' ', '\t', '\r', '\n']) {
        if field.is_empty() {
            continue;
        }
        out.push(number(field)?);
    }
    Some(out)
}

/// A `Points` list: an even count of numbers, read as points.
#[must_use]
pub fn points(text: &str) -> Option<Vec<(f64, f64)>> {
    let values = numbers(text)?;
    if values.is_empty() || values.len() % 2 != 0 {
        return None;
    }
    Some(values.chunks_exact(2).map(|p| (p[0], p[1])).collect())
}

/// 14.4.1's six-number matrix, in the order `m11 m12 m21 m22 offsetX offsetY`.
///
/// That is PDF's `a b c d e f` in the same order, which is why the numbers in
/// the synthesised content stream are the numbers in the markup — the property
/// gap 30's geometry section asks not to be thrown away by pre-multiplying.
#[must_use]
pub fn matrix(text: &str) -> Option<[f64; 6]> {
    let values = numbers(text)?;
    match values.as_slice() {
        [a, b, c, d, e, f] => Some([*a, *b, *c, *d, *e, *f]),
        _ => None,
    }
}

/// `true` or `false`, ASCII case-insensitively.
#[must_use]
pub fn boolean(text: &str) -> Option<bool> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

// ---- Matrices ---------------------------------------------------------------

/// The identity, in the six-number form.
pub const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `first` then `second`, in the order a `cm` composes them.
#[must_use]
pub fn concat(first: [f64; 6], second: [f64; 6]) -> [f64; 6] {
    let [a1, b1, c1, d1, e1, f1] = first;
    let [a2, b2, c2, d2, e2, f2] = second;
    [
        a1 * a2 + b1 * c2,
        a1 * b2 + b1 * d2,
        c1 * a2 + d1 * c2,
        c1 * b2 + d1 * d2,
        e1 * a2 + f1 * c2 + e2,
        e1 * b2 + f1 * d2 + f2,
    ]
}

/// A point through a matrix.
#[must_use]
pub fn apply(m: [f64; 6], point: (f64, f64)) -> (f64, f64) {
    let (x, y) = point;
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

// ---- Numbers on the way out -------------------------------------------------

/// How many decimal places a coordinate reaches the content stream with.
///
/// One ten-thousandth of a PDF point is 1/720 000 inch, which is two orders
/// below what any rasteriser resolves, and rounding here rather than letting
/// `f64`'s shortest round-trip spelling through is what keeps `0.1 + 0.2` from
/// arriving as `0.30000000000000004` — a content stream whose length depends
/// on floating-point accident is one the fourteenth determinism fingerprint
/// would have to hash.
const PLACES: f64 = 1.0e4;

/// One number, as PDF spells it.
///
/// 7.3.3: a PDF real has no exponent form, so a value this reader would print
/// as `1e-7` has to arrive as `0.0000001`. Rust's `Display` for `f64` never
/// uses an exponent, and rounding first keeps the digit count bounded.
#[must_use]
pub fn num(value: f64) -> String {
    let rounded = (value * PLACES).round() / PLACES;
    // `-0` is a PDF number and a pointless one; it also makes two spellings of
    // the same content stream.
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded}")
}

/// Appends `values` as PDF numbers, space separated, followed by `operator`.
pub fn op(out: &mut Vec<u8>, values: &[f64], operator: &str) {
    for value in values {
        out.extend_from_slice(num(*value).as_bytes());
        out.push(b' ');
    }
    out.extend_from_slice(operator.as_bytes());
    out.push(b'\n');
}
