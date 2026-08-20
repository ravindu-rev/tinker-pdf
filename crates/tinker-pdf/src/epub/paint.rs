//! Measurement, encoding and the page a laid-out book is drawn on (gap 31,
//! milestone 8).
//!
//! Two things live here because they are one decision seen from either end:
//! **which face a run is measured with** and **which font resource it is drawn
//! with** have to be the same answer, and a build with two of them sets a page
//! whose glyphs do not fit the boxes it computed. [`Face::of`] is that one
//! answer, and [`BookMetrics`] and [`draw_page`] both go through it.
//!
//! # The metrics are real, and they are not a font file
//!
//! Nothing here parses an sfnt. The advances are Adobe's published AFM numbers
//! for the standard 14, which `tinker-pdf-font` already holds because a PDF may
//! omit `/Widths` for those faces and this repository's reader has to lay such
//! a document out. So a book set in Times is measured with **Times's own
//! advances** and set with **Times**, and the pagination is the pagination a
//! reader will see. Milestone 9 is where `@font-face` and a real embedded face
//! arrive; until then this is a correct answer for a restricted set of faces
//! rather than an approximation of a general one.
//!
//! # A character `WinAnsiEncoding` cannot spell
//!
//! Milestone 1's corpus contains a line of Japanese in five of its six books,
//! placed there so that a space-only line breaker would be caught. It catches
//! something else too: a simple font maps one byte to one glyph, and 25 kanji
//! and kana are not in Windows code page 1252. A build that wrote them as
//! UTF-8 bytes into a `WinAnsiEncoding` string would put mojibake on the page
//! **and** lose the characters from `Page::text()`, which is text conservation
//! failing on the one sentence the corpus was built around.
//!
//! So a character outside the encoding is given a code in an **overflow font**
//! — the same base face under an `/Encoding` whose `/Differences` names the
//! glyph by the Adobe Glyph List's algorithmic `uniXXXX` form. 9.10.2's second
//! step resolves that back to the character, so the text extracts correctly;
//! the standard face has no such glyph, so the page shows a notdef. **That
//! asymmetry is stated rather than hidden**: [`Fonts::unrepresented`] counts
//! every character drawn that way, and the caller warns by name.
//!
//! **One overflow font per face, and no more**, which is 224 codes and is
//! `/Differences`'s own size rather than a cap invented here — the array is
//! allocated at that size whatever the input says, so it is not a bound in
//! ruling 1's sense and does not join `bounds_ledger.rs`. A book with more
//! than 224 distinct characters outside the encoding for one face loses the
//! excess and says so, and milestone 9's `@font-face` is where that stops
//! being true.

use tinker_pdf_cos::build::{DocumentBuilder, PageBuilder, Target};
use tinker_pdf_css::property::{BorderStyle, Color, FontFamily, FontStyle, Side, TextDecoration};
use tinker_pdf_font::base14::Standard14;
use tinker_pdf_font::encoding::{base_char, glyph_name_for_char, BaseEncoding};
use tinker_pdf_layout::metrics::{FontRequest, Metrics, Vertical};
use tinker_pdf_layout::{BoxFragment, Page as LayoutPage, TextRun};

use super::read::PX_TO_PT;

/// How many codes one overflow font holds: 32 through 255.
///
/// `/Differences` may start at any code and a simple font has 256 of them;
/// starting at 32 keeps every code out of the range a PDF lexer would have to
/// escape twice and leaves the largest contiguous run available.
pub const OVERFLOW_CODES: usize = 224;

/// The first code an overflow font uses.
pub const OVERFLOW_FIRST: u8 = 32;

/// Which of the three generic families a run resolved to.
///
/// `cursive` and `fantasy` are `css-fonts-4` generic families this build has
/// no face for, and they resolve to `serif` — **which is what a reading system
/// with no such face does**, and is the one place here where a value is mapped
/// onto a neighbour. It is recorded rather than silent: the resolution is a
/// property of having only the standard 14, and milestone 9 is where a
/// provider can answer differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Generic {
    /// `serif`, and the initial value.
    Serif,
    /// `sans-serif`.
    SansSerif,
    /// `monospace`.
    Monospace,
}

/// One of the twelve text faces of the standard 14.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Face {
    /// The generic family.
    pub generic: Generic,
    /// `font-weight` at or above 600, which is `css-fonts-4` §2.2's own
    /// threshold for a face that has only two weights.
    pub bold: bool,
    /// `font-style` other than `normal`. Times has an italic and Helvetica an
    /// oblique, and `css-fonts-4` §5.2 makes either an acceptable match for
    /// the other.
    pub italic: bool,
}

impl Face {
    /// Which face a request resolves to.
    ///
    /// The family list is walked **in the author's order** and the first entry
    /// this build can answer wins, which is `css-fonts-4` §5's rule read down
    /// to three faces. A named family goes through
    /// [`Standard14::from_base_font`], so `Georgia` — which pandoc sets on
    /// every book it produced here — resolves through its `serif` substitution
    /// rather than falling off the end of the list.
    #[must_use]
    pub fn of(font: &FontRequest<'_>) -> Face {
        let bold = font.weight >= 600;
        let italic = font.style != FontStyle::Normal;
        for family in font.families {
            let generic = match family {
                FontFamily::Serif | FontFamily::Cursive | FontFamily::Fantasy => Generic::Serif,
                FontFamily::SansSerif => Generic::SansSerif,
                FontFamily::Monospace => Generic::Monospace,
                FontFamily::Named(name) => match Standard14::from_base_font(name) {
                    Some(Standard14::Courier) => Generic::Monospace,
                    Some(Standard14::Helvetica | Standard14::HelveticaBold) => Generic::SansSerif,
                    Some(
                        Standard14::TimesRoman
                        | Standard14::TimesBold
                        | Standard14::TimesItalic
                        | Standard14::TimesBoldItalic,
                    ) => Generic::Serif,
                    // Symbol and ZapfDingbats are not text faces, and a family
                    // this build has never heard of is not a match at all:
                    // §5's algorithm moves to the next entry rather than
                    // stopping, which is what makes `Georgia, serif` fall
                    // through to `serif` when `Georgia` is absent.
                    _ => continue,
                },
            };
            return Face {
                generic,
                bold,
                italic,
            };
        }
        Face {
            generic: Generic::Serif,
            bold,
            italic,
        }
    }

    /// Every face, in a stable order, so the resource names a document uses do
    /// not depend on the order a book happened to need them in.
    #[must_use]
    pub fn all() -> Vec<Face> {
        let mut out = Vec::with_capacity(12);
        for generic in [Generic::Serif, Generic::SansSerif, Generic::Monospace] {
            for bold in [false, true] {
                for italic in [false, true] {
                    out.push(Face {
                        generic,
                        bold,
                        italic,
                    });
                }
            }
        }
        out
    }

    /// This face's index among [`Face::all`].
    #[must_use]
    pub fn index(self) -> usize {
        let generic = match self.generic {
            Generic::Serif => 0,
            Generic::SansSerif => 1,
            Generic::Monospace => 2,
        };
        generic * 4 + usize::from(self.bold) * 2 + usize::from(self.italic)
    }

    /// The `/BaseFont` name, from Annex D.1's table.
    #[must_use]
    pub fn base_font(self) -> &'static [u8] {
        match (self.generic, self.bold, self.italic) {
            (Generic::Serif, false, false) => b"Times-Roman",
            (Generic::Serif, false, true) => b"Times-Italic",
            (Generic::Serif, true, false) => b"Times-Bold",
            (Generic::Serif, true, true) => b"Times-BoldItalic",
            (Generic::SansSerif, false, false) => b"Helvetica",
            (Generic::SansSerif, false, true) => b"Helvetica-Oblique",
            (Generic::SansSerif, true, false) => b"Helvetica-Bold",
            (Generic::SansSerif, true, true) => b"Helvetica-BoldOblique",
            (Generic::Monospace, false, false) => b"Courier",
            (Generic::Monospace, false, true) => b"Courier-Oblique",
            (Generic::Monospace, true, false) => b"Courier-Bold",
            (Generic::Monospace, true, true) => b"Courier-BoldOblique",
        }
    }

    /// The face whose **advances** this one shares.
    ///
    /// Helvetica and Helvetica-Oblique publish the same widths and so do the
    /// four Couriers, which is why [`Standard14`] has nine variants for twelve
    /// faces rather than twelve.
    #[must_use]
    pub fn standard(self) -> Standard14 {
        match (self.generic, self.bold, self.italic) {
            (Generic::Serif, false, false) => Standard14::TimesRoman,
            (Generic::Serif, false, true) => Standard14::TimesItalic,
            (Generic::Serif, true, false) => Standard14::TimesBold,
            (Generic::Serif, true, true) => Standard14::TimesBoldItalic,
            (Generic::SansSerif, false, _) => Standard14::Helvetica,
            (Generic::SansSerif, true, _) => Standard14::HelveticaBold,
            (Generic::Monospace, _, _) => Standard14::Courier,
        }
    }

    /// The ascent and descent, as fractions of the em.
    ///
    /// Adobe's published AFM `Ascender` and `Descender` for the three families,
    /// with the descender's sign flipped because
    /// [`tinker_pdf_layout::metrics::Vertical`] measures a depth below the
    /// baseline as a positive number — a provider that returned the sfnt's own
    /// convention and one that returned the absolute value would both look
    /// plausible and one of them would set every line on top of the next.
    #[must_use]
    pub fn vertical_fractions(self) -> (f64, f64) {
        match self.generic {
            Generic::Serif => (0.683, 0.217),
            Generic::SansSerif => (0.718, 0.207),
            Generic::Monospace => (0.629, 0.157),
        }
    }

    /// The resource name the primary, `WinAnsiEncoding` font is registered
    /// under.
    #[must_use]
    pub fn resource(self) -> Vec<u8> {
        format!("Bk{}", self.index()).into_bytes()
    }

    /// The resource name of this face's overflow font.
    #[must_use]
    pub fn overflow_resource(self) -> Vec<u8> {
        format!("Bx{}", self.index()).into_bytes()
    }
}

/// The character a code stands for in `WinAnsiEncoding`, backwards.
///
/// Three ranges rather than a table, because `tinker-pdf-font`'s own table for
/// 0x80–0x9F is private and duplicating it here would be two tables that could
/// disagree. Below 0x80 and at or above 0xA0 the encoding **is** Latin-1 by
/// construction, and the thirty-two codes in between are found by asking the
/// one table there is.
#[must_use]
pub fn winansi_code(c: char) -> Option<u8> {
    let code = u32::from(c);
    if code < 0x80 {
        return u8::try_from(code).ok();
    }
    if (0xA0..=0xFF).contains(&code) {
        return u8::try_from(code).ok();
    }
    (0x80..=0x9F).find(|code| base_char(BaseEncoding::WinAnsi, *code) == Some(c))
}

/// Advances and line heights for a book set in the standard 14.
#[derive(Clone, Copy, Debug, Default)]
pub struct BookMetrics;

impl Metrics for BookMetrics {
    fn advance(&self, ch: char, font: &FontRequest<'_>) -> f64 {
        // An East Asian character is one em wide in every face that has one,
        // and the standard 14 have none at all — so the number cannot come
        // from `Standard14`, which would answer with a Latin space's advance
        // and set a Japanese line at a third of its width. UAX #11's own
        // classification is what decides, through the table
        // `tinker-pdf-layout` already vendors for UAX #14.
        if tinker_pdf_layout::unicode::is_east_asian(ch) {
            return font.size;
        }
        let (advance, _) = Face::of(font).standard().advance(ch);
        f64::from(advance) / 1000.0 * font.size
    }

    fn vertical(&self, font: &FontRequest<'_>) -> Vertical {
        let (ascent, descent) = Face::of(font).vertical_fractions();
        Vertical {
            ascent: ascent * font.size,
            descent: descent * font.size,
        }
    }
}

/// The font resources a book needs, and the codes its out-of-encoding
/// characters were given.
#[derive(Clone, Debug, Default)]
pub struct Fonts {
    /// Per face, in [`Face::all`] order, the characters that needed an
    /// overflow code, in the order they were first met.
    overflow: Vec<Vec<char>>,
    /// Which faces drew anything at all, so a book of Times does not carry
    /// twelve font dictionaries.
    used: Vec<bool>,
    /// Characters that could not be given a code at all.
    unrepresented: usize,
}

impl Fonts {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Fonts {
        Fonts {
            overflow: vec![Vec::new(); 12],
            used: vec![false; 12],
            unrepresented: 0,
        }
    }

    /// Records every character one run will draw.
    pub fn note(&mut self, run: &TextRun) {
        let font = request(run);
        let face = Face::of(&font);
        let index = face.index();
        self.used[index] = true;
        for ch in run.text.chars() {
            if winansi_code(ch).is_some() {
                continue;
            }
            if self.overflow[index].contains(&ch) {
                continue;
            }
            if self.overflow[index].len() >= OVERFLOW_CODES {
                self.unrepresented += 1;
                continue;
            }
            self.overflow[index].push(ch);
        }
    }

    /// How many characters had no code, and are therefore not on any page.
    #[must_use]
    pub fn unrepresented(&self) -> usize {
        self.unrepresented
    }

    /// How many overflow fonts the document carries.
    #[must_use]
    pub fn overflow_fonts(&self) -> usize {
        self.overflow.iter().filter(|set| !set.is_empty()).count()
    }

    /// How many codes one face's overflow font spends.
    ///
    /// Not the same number as how many characters were *met*, and the
    /// difference is the whole of [`Fonts::note`]'s duplicate check: a build
    /// that pushed a code per occurrence rather than per character would draw
    /// every book identically — `encode` finds the first entry either way —
    /// and would run out of the 224 on the first paragraph of Japanese. The
    /// injection matrix is what found that nothing could see it.
    #[must_use]
    pub fn codes(&self, face: Face) -> usize {
        self.overflow[face.index()].len()
    }

    /// Registers every face this book used on the document.
    pub fn register(&self, builder: &mut DocumentBuilder) {
        for face in Face::all() {
            let index = face.index();
            if !self.used[index] {
                continue;
            }
            builder.add_base_font(&face.resource(), face.base_font());
            if self.overflow[index].is_empty() {
                continue;
            }
            let names: Vec<String> = self.overflow[index]
                .iter()
                .map(|c| glyph_name_for_char(*c).unwrap_or_else(|| "space".to_owned()))
                .collect();
            let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
            // The width a code is written with is the width this build
            // **measured** it at, not the width the standard face publishes for
            // whatever glyph it has at that code. They differ for every
            // character in this array by construction, and a `/Widths` that
            // disagreed with the layout would put a viewer's text cursor
            // somewhere the text is not.
            let widths: Vec<u16> = self.overflow[index]
                .iter()
                .map(|c| {
                    let em = if tinker_pdf_layout::unicode::is_east_asian(*c) {
                        1000.0
                    } else {
                        f64::from(face.standard().advance(*c).0)
                    };
                    em.round().clamp(0.0, 65535.0) as u16
                })
                .collect();
            builder.add_named_font(
                &face.overflow_resource(),
                face.base_font(),
                OVERFLOW_FIRST,
                &borrowed,
                &widths,
            );
        }
    }

    /// The resource and the code one character is drawn with.
    ///
    /// `None` for a character that got neither, which is the only case a page
    /// silently loses text in — and it is counted, not silent.
    #[must_use]
    pub fn encode(&self, face: Face, ch: char) -> Option<(Vec<u8>, u8)> {
        if let Some(code) = winansi_code(ch) {
            return Some((face.resource(), code));
        }
        let at = self.overflow[face.index()].iter().position(|c| *c == ch)?;
        let code = OVERFLOW_FIRST.checked_add(u8::try_from(at).ok()?)?;
        Some((face.overflow_resource(), code))
    }
}

/// The font a run asks for, rebuilt from what the run carries.
#[must_use]
pub fn request(run: &TextRun) -> FontRequest<'_> {
    FontRequest {
        families: &run.families,
        weight: run.weight,
        style: run.style,
        size: run.font_size,
    }
}

/// Where a laid-out page's coordinates land on a PDF page.
///
/// `tinker-pdf-layout` measures in CSS pixels with `y` growing **downward**
/// from the top of the content area; a PDF page is points with `y` growing
/// upward from the bottom. The flip and the scale happen here, once, which is
/// the reason this is a struct rather than four arguments passed around.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The page box, in points.
    pub page: (f64, f64),
    /// The margin around the content area, in points.
    pub margin: f64,
}

impl Frame {
    /// The content area, in CSS pixels, which is what `Options` takes.
    #[must_use]
    pub fn content_px(&self) -> (f64, f64) {
        (
            (self.page.0 - self.margin * 2.0).max(1.0) / PX_TO_PT,
            (self.page.1 - self.margin * 2.0).max(1.0) / PX_TO_PT,
        )
    }

    /// A horizontal offset in CSS pixels, as a PDF x coordinate.
    #[must_use]
    pub fn x(&self, px: f64) -> f64 {
        self.margin + px * PX_TO_PT
    }

    /// A downward offset in CSS pixels, as a PDF y coordinate.
    #[must_use]
    pub fn y(&self, px: f64) -> f64 {
        self.page.1 - self.margin - px * PX_TO_PT
    }
}

/// Draws one laid-out page.
///
/// Decorations first, in the order `tinker-pdf-layout` produced them — an
/// ancestor before its descendants, so a child's background covers its
/// parent's — and then the text, in **reading order**, which is what makes
/// `Page::text()` return the words in the order the book wrote them rather
/// than in the order a painter found convenient.
pub fn draw_page(page: &mut PageBuilder, laid: &LayoutPage, frame: &Frame, fonts: &Fonts) {
    for fragment in &laid.boxes {
        draw_box(page, fragment, frame);
    }
    for run in &laid.runs {
        if !run.painted {
            continue;
        }
        // 14.8.2.2: a list marker is *"a graphics object that is not part of
        // the author's original content"*, which is what 14.8.2 calls an
        // artifact and what `TextRun::generated` already says one crate down.
        // Marking it is what lets a bullet be **drawn and not extracted**, and
        // it is the only reason text conservation can stay an equality: a
        // marker on the page and not in the spine would be one extra character
        // per list item, on every book with a list in it.
        if run.generated {
            page.raw(b"/Artifact BMC");
        }
        draw_run(page, run, frame, fonts);
        if run.generated {
            page.raw(b"EMC");
        }
    }
}

fn set_fill(page: &mut PageBuilder, colour: Color) {
    page.set_fill_rgb(
        f64::from(colour.r) / 255.0,
        f64::from(colour.g) / 255.0,
        f64::from(colour.b) / 255.0,
    );
}

/// Fills a rectangle in the current colour.
///
/// `PageBuilder::fill_rect` takes a grey and would overwrite the colour set
/// above it, so the operators are written out — which is what
/// `PageBuilder::raw` is for and what its documentation says it is for.
fn fill(page: &mut PageBuilder, x: f64, y: f64, width: f64, height: f64) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    page.raw(format!("{x} {y} {width} {height} re f").as_bytes());
}

fn draw_box(page: &mut PageBuilder, fragment: &BoxFragment, frame: &Frame) {
    let x = frame.x(fragment.x);
    let top = frame.y(fragment.y);
    let width = fragment.width * PX_TO_PT;
    let height = fragment.height * PX_TO_PT;
    if fragment.background.a != 0 {
        set_fill(page, fragment.background);
        fill(page, x, top - height, width, height);
    }
    // A border is drawn as four filled rectangles rather than as a stroked
    // path, because CSS's border box is defined by its **edges** and a stroke
    // is centred on a path: a one-pixel stroke round the border box would put
    // half a pixel outside it on all four sides.
    let widths = &fragment.border_width;
    let styles = &fragment.border_style;
    let colours = &fragment.border_color;
    for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
        let thickness = widths.get(side) * PX_TO_PT;
        if thickness <= 0.0 || !drawable(styles.get(side)) {
            continue;
        }
        set_fill(page, colours.get(side));
        let (bx, by, bw, bh) = match side {
            Side::Top => (x, top - thickness, width, thickness),
            Side::Bottom => (x, top - height, width, thickness),
            Side::Left => (x, top - height, thickness, height),
            Side::Right => (x + width - thickness, top - height, thickness, height),
        };
        fill(page, bx, by, bw, bh);
    }
}

/// Whether a border style puts ink on the page at all.
///
/// `dashed`, `dotted` and `double` are drawn **solid**, and that is a
/// partiality worth naming rather than hiding: the border is in the right
/// place at the right width in the right colour and the pattern is wrong.
/// `tinker-pdf-layout` already warns for the properties it does not honour;
/// this one is honoured approximately, which is a third thing, and milestone
/// 13's `As built` is where it is recorded.
fn drawable(style: BorderStyle) -> bool {
    !matches!(style, BorderStyle::None | BorderStyle::Hidden)
}

fn draw_run(page: &mut PageBuilder, run: &TextRun, frame: &Frame, fonts: &Fonts) {
    let font = request(run);
    let face = Face::of(&font);
    let metrics = BookMetrics;
    let size = run.font_size * PX_TO_PT;
    let baseline = frame.y(run.y);
    let mut x = run.x;

    set_fill(page, run.color);
    // One `Tj` per contiguous stretch of characters sharing a font resource,
    // because a PDF string is bytes in **one** font: a run that mixes an
    // encodable character with an unencodable one is two show operations and
    // not one, and the second's origin is wherever the first's advance left it.
    let mut codes: Vec<u8> = Vec::new();
    let mut characters = String::new();
    let mut resource: Option<Vec<u8>> = None;
    let mut start = x;

    for ch in run.text.chars() {
        let encoded = fonts.encode(face, ch);
        let Some((next, code)) = encoded else {
            // No code at all: the character is not drawn. Counted by
            // `Fonts::note` when the run was walked, so the page is short of
            // exactly as many characters as the report says.
            continue;
        };
        if resource.as_ref() != Some(&next) {
            flush(
                page,
                &resource,
                size,
                frame.x(start),
                baseline,
                run,
                &codes,
                &characters,
            );
            codes.clear();
            characters.clear();
            start = x;
            resource = Some(next);
        }
        codes.push(code);
        characters.push(ch);
        x += metrics.advance(ch, &font) + run.letter_spacing;
        if ch == ' ' {
            x += run.word_spacing;
        }
    }
    flush(
        page,
        &resource,
        size,
        frame.x(start),
        baseline,
        run,
        &codes,
        &characters,
    );

    decorate(page, run, frame, x);
}

#[allow(clippy::too_many_arguments)]
fn flush(
    page: &mut PageBuilder,
    resource: &Option<Vec<u8>>,
    size: f64,
    x: f64,
    y: f64,
    run: &TextRun,
    codes: &[u8],
    characters: &str,
) {
    let Some(resource) = resource else {
        return;
    };
    if codes.is_empty() {
        return;
    }
    page.encoded_text(
        resource,
        size,
        x,
        y,
        (run.letter_spacing * PX_TO_PT, run.word_spacing * PX_TO_PT),
        codes,
        characters,
    );
}

/// `text-decoration`, as a filled rectangle at the position CSS 2.2 §16.3.1
/// leaves to the user agent.
fn decorate(page: &mut PageBuilder, run: &TextRun, frame: &Frame, end_px: f64) {
    if run.decoration == TextDecoration::None {
        return;
    }
    let width = (end_px - run.x).max(0.0) * PX_TO_PT;
    if width <= 0.0 {
        return;
    }
    let size = run.font_size * PX_TO_PT;
    let thickness = (size / 14.0).max(0.4);
    let baseline = frame.y(run.y);
    let y = match run.decoration {
        // A tenth of an em below the baseline clears a descender's stem
        // without crossing it, which is what a reading system's own underline
        // does.
        TextDecoration::Underline => baseline - size * 0.1 - thickness,
        TextDecoration::Overline => baseline + size * 0.75,
        TextDecoration::LineThrough => baseline + size * 0.25,
        TextDecoration::None => return,
    };
    set_fill(page, run.color);
    fill(page, frame.x(run.x), y, width, thickness);
}

/// A rectangle a link annotation covers, in PDF points.
///
/// Padded by a fifth of the font size above the baseline and a tenth below,
/// which is the box a reader expects to be able to click: a rectangle exactly
/// on the baseline has no height at all, and `DocumentBuilder::link` refuses
/// one that encloses no area.
#[must_use]
pub fn run_rect(run: &TextRun, frame: &Frame) -> (f64, f64, f64, f64) {
    let size = run.font_size * PX_TO_PT;
    let baseline = frame.y(run.y);
    (
        frame.x(run.x),
        baseline - size * 0.25,
        frame.x(run.x) + run.width * PX_TO_PT,
        baseline + size * 0.85,
    )
}

/// Where a link goes, from an `href` that has already been classified.
#[must_use]
pub fn page_target(index: u32) -> Target {
    Target::Page {
        index,
        // `/Fit` rather than `/XYZ`: an EPUB cross-reference names a chapter
        // and this build has one chapter's worth of page, so scrolling to a
        // coordinate inside it would be a precision the source does not have.
        view: tinker_pdf_cos::dest::DestKind::Fit,
    }
}
