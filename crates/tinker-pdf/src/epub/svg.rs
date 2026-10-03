//! An SVG spine item, drawn onto a page (gap 31's SVG lane, milestone 7).
//!
//! `tinker-pdf-svg` reads a document into a [`Scene`] — filled and stroked
//! outlines, gradients, clips, groups, images and text runs, all in one
//! coordinate space, with everything it declined named in [`Scene::warnings`].
//! This file is the other half: it turns that display list into content-stream
//! operators, and it is the **only** place in this repository that knows both
//! what an SVG node is and what a PDF operator is.
//!
//! # Ruling 7, and where the `Device` seam actually is
//!
//! Nothing here rasterizes. What this writes is a page of a synthesised
//! document, exactly as `paint::draw_page` writes a chapter's boxes and text —
//! and that document is then read back through `tinker-pdf-content`'s
//! interpreter and out through the `Device` trait like any other. The seam
//! ruling 7 protects is between *interpreting a content stream* and its
//! consumers, and there is no interpretation on this path: an SVG is not a
//! content stream, and the scene reaches paper by being **written**.
//!
//! The alternative — rasterizing a scene here and placing the bitmap — is what
//! ruling 7 forbids in spirit as well as in letter: it would put a second
//! renderer in the tree, produce a page whose text does not extract, and make
//! the output resolution-dependent.
//!
//! # Four things this file decides, and one it does not
//!
//! **The page mapping.** SVG's user space has `y` growing downward from the
//! top left and a PDF page has it growing upward from the bottom left, so one
//! `cm` at the top of every content stream carries the flip, the scale and the
//! centring for the whole scene. Every coordinate written after it is the
//! scene's own, which is what makes the operators readable beside the fixture.
//!
//! **Where a gradient's matrix goes.** 8.7.3.1 makes pattern space *"the
//! default coordinate system"* of the pattern's parent content stream — it
//! ignores whatever `cm` is in force. So a shading pattern's `/Matrix` is
//! composed with the page mapping here rather than inherited from it, and that
//! is the one place the flip has to be written twice.
//!
//! **Where a group's form is drawn.** §14.5's group is a transparency group
//! form XObject, and 8.7.3.1 reads a pattern used *inside* a form against the
//! form's default space at the moment it is painted. Every form here is
//! therefore painted with the stream's own default space in force — the page
//! mapping undone around the `Do`, and put back as the form's first operator —
//! so that a form's default space **is** the page's, and a gradient inside a
//! group is anchored by the same matrix as one outside it, whichever of the
//! two readings of 8.7.3.1 a reader takes.
//!
//! **How text is set.** Through `paint::face_runs` and
//! [`crate::shaping::write_run`], which is the same `css-fonts-4` §5.3 matcher
//! and the same shaper that set the rest of the book. A second matcher here
//! would let SVG text and XHTML text in one book resolve `serif` differently,
//! and no page would say which was right.
//!
//! What it does **not** decide is what the document said: every property, every
//! transform and every refusal was settled in the leaf crate.

use std::collections::HashMap;

use tinker_pdf_cos::build::{
    DeviceSpace, DocumentBuilder, ExtGState, FormXObject, Function, ImageData, PageBuilder,
    Shading, ShadingPattern, TransparencyGroup,
};
use tinker_pdf_css::property::{Color, FontFamily, FontStyle, FontVariant, TextDecoration};
use tinker_pdf_filters::Limits as FilterLimits;
use tinker_pdf_font::Sfnt;
use tinker_pdf_layout::metrics::{FontRequest, Metrics};
use tinker_pdf_layout::TextRun;
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{
    transform, Clip, FillRule, LineCap, LineJoin, Node, Paint, Scene, TextAnchor,
};

use super::paint::{self, BookMetrics, Chosen, Coded, Fonts};

/// An SVG's `font-family` list as the one `css-fonts-4` §5.3 matcher takes.
///
/// The generic names are recognised here rather than in `tinker-pdf-svg`,
/// because *which* names are generic is CSS's question and that crate holds no
/// CSS model — it carries the author's strings and this turns them into the
/// vocabulary `paint::choose` already speaks.
fn families_of(names: &[String]) -> Vec<FontFamily> {
    names
        .iter()
        .map(|name| match name.to_ascii_lowercase().as_str() {
            "serif" => FontFamily::Serif,
            "sans-serif" => FontFamily::SansSerif,
            "monospace" => FontFamily::Monospace,
            "cursive" => FontFamily::Cursive,
            "fantasy" => FontFamily::Fantasy,
            _ => FontFamily::Named(name.clone()),
        })
        .collect()
}

/// The font request one run is set with.
fn request_of<'a>(font: &tinker_pdf_svg::TextStyle, families: &'a [FontFamily]) -> FontRequest<'a> {
    FontRequest {
        families,
        weight: font.weight,
        style: if font.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        size: font.size,
    }
}

/// What one SVG page cost and what it could not draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drawn {
    /// Nodes whose paint the writer refused — a shading a reader could not
    /// evaluate, a glyph run the builder declined, a group whose form or
    /// alpha the builder would not register.
    pub refused: usize,
    /// `<image>` references this build did not resolve into a page.
    pub images_unresolved: usize,
    /// `<image>` references that resolved and were embedded.
    pub images_drawn: usize,
}

/// A scene's whole page, written **before its page begins**.
///
/// # The bug this type exists to make impossible
///
/// `DocumentBuilder::begin_page` *snapshots* the document's resource set, so a
/// `/Pattern`, an `/ExtGState` or an `/XObject` added after that call is
/// invisible to the page that wanted it — the operator naming it is written,
/// the reader cannot resolve the name, and the page draws **without** it. That
/// is silent in the worst way: a gradient becomes nothing, a translucent shape
/// becomes nothing, and a photograph becomes a grey rectangle, while the page
/// still has ink on it from every solid stroke. The first draft of this file
/// registered as it drew and every gradient in the fetched corpus was dropped
/// — the corpus test still passed, because `cover.svg` strokes its paths black
/// and a page with strokes and no fills is a page with more than one colour.
///
/// Since §14.5's groups the rule reaches further: a group is a form XObject,
/// and a form's operators have to exist before the form can be registered —
/// so the content of every group, text included, is written in [`register`],
/// and by the time a page is begun the whole of it is bytes. [`draw`] cannot
/// register anything, because it is handed a `&Registry` and no builder.
#[derive(Debug, Default)]
pub struct Registry {
    /// The page's operators, complete.
    content: Vec<u8>,
    /// What writing them cost.
    drawn: Drawn,
}

/// Writes a scene's page, registering everything it names, before the page is
/// begun.
///
/// `resolve` answers an `<image>` href: a closure rather than a parameter
/// because what an href resolves against is the *container*, and the container
/// is the caller's — the same boundary `tinker-pdf-css`'s `ImportResolver`
/// draws for an `@import`.
pub fn register(
    builder: &mut DocumentBuilder,
    scene: &Scene,
    placement: Placement,
    fonts: &Fonts<'_>,
    metrics: &BookMetrics<'_>,
    resolve: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Registry {
    let space = Space {
        base: placement.matrix(scene.size),
        extent: [0.0, 0.0, placement.page.0, placement.page.1],
    };
    let mut writer = Writer {
        builder,
        fonts,
        metrics,
        resolve,
        entry_limit: placement.entry_limit,
        next: 0,
        alphas: HashMap::new(),
        images: HashMap::new(),
        drawn: Drawn::default(),
    };
    let content = writer.stream(&scene.nodes, space);
    Registry {
        content,
        drawn: writer.drawn,
    }
}

/// Where a scene goes on a page.
///
/// A struct rather than four numbers, because two of them are lengths in
/// different spaces and a caller that swapped them would still compile.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// The page box, in points.
    pub page: (f64, f64),
    /// The largest `<image>` this page may embed, in bytes.
    ///
    /// The **host's** number rather than the decoder's, which is `cbz.rs`'s
    /// posture: a raster bigger than the biggest file the container may hold
    /// is not a picture, and a host that lowered the ceiling can tell its own
    /// decision from `MAX_PNG_SAMPLES`.
    pub entry_limit: usize,
}

impl Placement {
    /// §7.7's `meet` mapping of a scene onto a page: the whole picture fits,
    /// scaled uniformly, centred in whatever is left over.
    ///
    /// The same rule `transform::view_box` applies inside a document, applied
    /// once more at the outside — because a spine item's SVG states its own
    /// size and the reading system states the page, and neither gets to
    /// distort the other. A build that stretched to fill would make every
    /// cover that is not the page's shape slightly the wrong one.
    #[must_use]
    pub fn matrix(&self, scene: (f64, f64)) -> [f64; 6] {
        let (page_width, page_height) = self.page;
        if !(scene.0 > 0.0 && scene.1 > 0.0) {
            return [1.0, 0.0, 0.0, -1.0, 0.0, page_height];
        }
        let scale = (page_width / scene.0).min(page_height / scene.1);
        let left = (page_width - scene.0 * scale) / 2.0;
        let top = (page_height - scene.1 * scale) / 2.0;
        // `y` down becomes `y` up: the `d` term is negative and the `f` term
        // is the page's top edge, so a scene point at `y = 0` lands there.
        [scale, 0.0, 0.0, -scale, left, page_height - top]
    }
}

/// Draws a scene's page, whose operators [`register`] already wrote.
pub fn draw(page: &mut PageBuilder, registry: &Registry) -> Drawn {
    page.raw(&registry.content);
    registry.drawn.clone()
}

/// The space one content stream's nodes are drawn in.
///
/// `base` maps the nodes' coordinates — the scene's — into the stream's
/// **default** space, which is the page's for the page and for every group's
/// form (see this module's header for why those are one space), and
/// `extent` is that default space's visible box, which a form's `/BBox` is.
#[derive(Clone, Copy, Debug)]
struct Space {
    base: [f64; 6],
    extent: [f64; 4],
}

/// Where each text run of one content stream starts, consumed in paint order.
///
/// §10.9's chunks cross group boundaries — a `<tspan opacity="0.5">` is a
/// group around its run and still continues the pen of the run before it — so
/// the origins are resolved once for the whole stream, groups looked through,
/// and the drawing pass takes them in the order it meets the runs.
struct Cursor {
    origins: Vec<Option<Origin>>,
    next: usize,
}

impl Cursor {
    fn take(&mut self) -> Option<Origin> {
        let origin = self.origins.get(self.next).copied().flatten();
        self.next += 1;
        origin
    }
}

/// An embedded picture: its `/XObject` name and its natural size in pixels.
type Embedded = (Vec<u8>, (f64, f64));

/// The state of one [`register`] call.
struct Writer<'b, 'f, 'm, R> {
    builder: &'b mut DocumentBuilder,
    fonts: &'b Fonts<'f>,
    metrics: &'b BookMetrics<'m>,
    resolve: R,
    entry_limit: usize,
    /// The next resource number this page hands out.
    next: usize,
    /// `(fill alpha, stroke alpha)` to the `/ExtGState` already registered
    /// for it, so a page of a thousand translucent shapes writes one.
    alphas: HashMap<(u64, u64), Vec<u8>>,
    /// Each href to the `/XObject` it resolved to and the picture's natural
    /// size, or `None` for one that did not resolve — so a `<use>` of a
    /// photograph sixteen times embeds it once.
    images: HashMap<String, Option<Embedded>>,
    drawn: Drawn,
}

impl<R: FnMut(&str) -> Option<Vec<u8>>> Writer<'_, '_, '_, R> {
    /// A resource name no other resource on this page has.
    fn name(&mut self, kind: &str) -> Vec<u8> {
        let name = format!("Svg{kind}{}", self.next);
        self.next += 1;
        name.into_bytes()
    }

    /// One whole content stream: the space's mapping, the nodes, and the `Q`
    /// that closes it.
    fn stream(&mut self, nodes: &[Node], space: Space) -> Vec<u8> {
        let mut cursor = Cursor {
            origins: place_text(nodes, self.metrics),
            next: 0,
        };
        let mut out = Vec::new();
        out.extend_from_slice(b"q ");
        matrix(&mut out, space.base);
        out.extend_from_slice(b" cm\n");
        self.nodes(&mut out, nodes, space, &mut cursor);
        out.extend_from_slice(b"Q\n");
        out
    }

    fn nodes(&mut self, out: &mut Vec<u8>, nodes: &[Node], space: Space, cursor: &mut Cursor) {
        for node in nodes {
            self.node(out, node, space, cursor);
        }
    }

    fn node(&mut self, out: &mut Vec<u8>, node: &Node, space: Space, cursor: &mut Cursor) {
        match node {
            Node::Path {
                outline,
                fill,
                rule,
                fill_opacity,
                stroke,
                clip,
            } => {
                let stroke_alpha = stroke.as_ref().map_or(1.0, |stroke| stroke.opacity);
                let alpha = if *fill_opacity < 1.0 || stroke_alpha < 1.0 {
                    self.alpha(*fill_opacity, stroke_alpha)
                } else {
                    None
                };
                let fill_pattern = self.pattern(fill, space);
                let stroke_pattern = stroke
                    .as_ref()
                    .and_then(|stroke| self.pattern(&stroke.paint, space));
                out.extend_from_slice(b"q\n");
                if let Some(name) = &alpha {
                    gs(out, name);
                }
                if let Some(clip) = clip {
                    apply_clip(out, clip);
                }
                let filled = set_paint(out, fill, fill_pattern.as_deref(), false);
                let stroked = stroke.as_ref().is_some_and(|stroke| {
                    let painted = set_paint(out, &stroke.paint, stroke_pattern.as_deref(), true);
                    if painted {
                        set_stroke_state(out, stroke);
                    }
                    painted
                });
                if filled || stroked {
                    write_outline(out, outline);
                    out.extend_from_slice(operator(filled, stroked, *rule));
                    out.push(b'\n');
                } else if clip.is_some() {
                    // A shape that paints nothing still had a clip pushed, and
                    // `Q` below is what pops it.
                    self.drawn.refused += usize::from(!matches!(fill, Paint::None));
                }
                out.extend_from_slice(b"Q\n");
            }
            Node::Image {
                href,
                rect,
                matrix,
                preserve,
            } => match self.image(href) {
                Some((name, natural)) => {
                    place_image(out, &name, *rect, *matrix, preserve.as_deref(), natural);
                    self.drawn.images_drawn += 1;
                }
                None => self.drawn.images_unresolved += 1,
            },
            Node::Text { fill_opacity, .. } => {
                let Some(origin) = cursor.take() else {
                    return;
                };
                let alpha = if *fill_opacity < 1.0 {
                    self.alpha(*fill_opacity, 1.0)
                } else {
                    None
                };
                self.drawn.refused += draw_text(
                    self.builder,
                    out,
                    node,
                    origin,
                    alpha.as_deref(),
                    self.fonts,
                    self.metrics,
                );
            }
            Node::Group {
                nodes,
                opacity,
                clip,
            } => self.group(out, nodes, *opacity, clip.as_ref(), space, cursor),
            // `Node` is `#[non_exhaustive]`: a variant added to the leaf crate
            // reaches here as ink nobody drew, so it is counted rather than
            // skipped.
            _ => self.drawn.refused += 1,
        }
    }

    /// §14.5's group, and §14.3.5's clip of one.
    ///
    /// A group at full opacity is only a clip, which a `q`/`Q` pair around
    /// its nodes says exactly. Below full opacity it is 11.6.6's transparency
    /// group: the nodes are composited in a form of their own, and that form
    /// is painted once under 11.6.4.4's constant alpha — which 11.6.6 applies
    /// to the group's result as a whole, and which is §14.5 word for word.
    fn group(
        &mut self,
        out: &mut Vec<u8>,
        nodes: &[Node],
        opacity: f64,
        clip: Option<&Clip>,
        space: Space,
        cursor: &mut Cursor,
    ) {
        if opacity >= 1.0 {
            out.extend_from_slice(b"q\n");
            if let Some(clip) = clip {
                apply_clip(out, clip);
            }
            self.nodes(out, nodes, space, cursor);
            out.extend_from_slice(b"Q\n");
            return;
        }
        // The form's operators, in the scene's coordinates under the same
        // mapping as the stream around it: see this module's header.
        let mut inner = Vec::new();
        inner.extend_from_slice(b"q ");
        matrix(&mut inner, space.base);
        inner.extend_from_slice(b" cm\n");
        self.nodes(&mut inner, nodes, space, cursor);
        inner.extend_from_slice(b"Q\n");

        let Some(undo) = transform::invert(space.base) else {
            self.drawn.refused += 1;
            return;
        };
        let form = self.name("G");
        let registered = self.builder.add_form(
            &form,
            &FormXObject {
                bbox: space.extent,
                matrix: None,
                // Isolated, as XPS's canvas groups are: §14.5 composites the
                // group's offscreen image *onto* what is behind it, which is an
                // isolated group's definition, and for the normal blend mode
                // it is the same picture as a non-isolated one.
                group: Some(TransparencyGroup {
                    color_space: DeviceSpace::Rgb,
                    isolated: true,
                    knockout: false,
                }),
                content: &inner,
            },
        );
        let alpha = self.alpha(opacity, opacity);
        let (true, Some(alpha)) = (registered, alpha) else {
            self.drawn.refused += 1;
            return;
        };
        out.extend_from_slice(b"q\n");
        if let Some(clip) = clip {
            apply_clip(out, clip);
        }
        gs(out, &alpha);
        out.extend_from_slice(b"q ");
        matrix(out, undo);
        out.extend_from_slice(b" cm ");
        name(out, &form);
        out.extend_from_slice(b" Do Q\nQ\n");
    }

    /// The `/ExtGState` for one pair of alphas, registered once per page.
    fn alpha(&mut self, fill: f64, stroke: f64) -> Option<Vec<u8>> {
        let key = (fill.to_bits(), stroke.to_bits());
        if let Some(name) = self.alphas.get(&key) {
            return Some(name.clone());
        }
        let name = self.name("A");
        let registered = self.builder.add_ext_gstate(
            &name,
            &ExtGState {
                fill_alpha: Some(fill),
                stroke_alpha: Some(stroke),
                ..ExtGState::default()
            },
        );
        if !registered {
            return None;
        }
        self.alphas.insert(key, name.clone());
        Some(name)
    }

    /// Registers a gradient as a `/Pattern`, or `None` for a paint that is not
    /// one.
    fn pattern(&mut self, paint: &Paint, space: Space) -> Option<Vec<u8>> {
        let shading = shading_of(paint)?;
        // 8.7.3.1: pattern space is the stream's **default** coordinate
        // system, so the `cm` in force does not reach it and the space's
        // mapping has to be composed in here. This is the one place the flip
        // is written twice, and it is the specification's rule rather than a
        // shortcut.
        let matrix = match paint {
            Paint::Linear { matrix, .. } | Paint::Radial { matrix, .. } => {
                transform::concat(*matrix, space.base)
            }
            _ => space.base,
        };
        let name = self.name("P");
        self.builder
            .add_shading_pattern(
                &name,
                &ShadingPattern {
                    shading,
                    matrix: Some(matrix),
                },
            )
            .then_some(name)
    }

    /// An `<image>`'s `/XObject` and natural size, embedded on first use.
    fn image(&mut self, href: &str) -> Option<Embedded> {
        if let Some(known) = self.images.get(href) {
            return known.clone();
        }
        let name = self.name("I");
        let embedded = embed(
            self.builder,
            &name,
            href,
            self.entry_limit,
            &mut self.resolve,
        )
        .map(|natural| (name, natural));
        self.images.insert(href.to_owned(), embedded.clone());
        embedded
    }
}

/// Six numbers, space-separated, as a content stream spells a matrix.
fn matrix(out: &mut Vec<u8>, m: [f64; 6]) {
    let text: Vec<String> = m.iter().map(|v| number(*v)).collect();
    out.extend_from_slice(text.join(" ").as_bytes());
}

/// A number as a content stream may carry it.
///
/// A value that is not finite has no PDF spelling at all — `NaN` and `inf`
/// are not tokens a reader can lex — so it is written as zero, which is what
/// the leaf crate's own finite-number discipline already makes unreachable
/// from a document.
fn number(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "0".to_owned()
    }
}

/// A resource name, escaped as 7.3.5 requires.
fn name(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'/');
    for &byte in bytes {
        let escape = byte <= b' '
            || byte >= 0x7F
            || matches!(
                byte,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
            );
        if escape {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        } else {
            out.push(byte);
        }
    }
}

/// 8.4.5's `gs`.
fn gs(out: &mut Vec<u8>, resource: &[u8]) {
    name(out, resource);
    out.extend_from_slice(b" gs\n");
}

/// The painting operator for what was set: 8.5.3.3's four, by rule.
fn operator(filled: bool, stroked: bool, rule: FillRule) -> &'static [u8] {
    match (filled, stroked, rule) {
        (true, true, FillRule::NonZero) => b"B",
        (true, true, FillRule::EvenOdd) => b"B*",
        (true, false, FillRule::NonZero) => b"f",
        (true, false, FillRule::EvenOdd) => b"f*",
        (false, _, _) => b"S",
    }
}

/// One outline, as `m`/`l`/`c`/`h`.
fn write_outline(out: &mut Vec<u8>, outline: &tinker_pdf_svg::path::Outline) {
    for segment in &outline.segments {
        match *segment {
            Segment::Move(p) => {
                out.extend_from_slice(format!("{} {} m\n", number(p[0]), number(p[1])).as_bytes());
            }
            Segment::Line(p) => {
                out.extend_from_slice(format!("{} {} l\n", number(p[0]), number(p[1])).as_bytes());
            }
            Segment::Cubic(a, b, c) => out.extend_from_slice(
                format!(
                    "{} {} {} {} {} {} c\n",
                    number(a[0]),
                    number(a[1]),
                    number(b[0]),
                    number(b[1]),
                    number(c[0]),
                    number(c[1])
                )
                .as_bytes(),
            ),
            Segment::Close => out.extend_from_slice(b"h\n"),
            _ => {}
        }
    }
}

/// §14.3's clip, as 8.5.4's `W`/`W*` followed by `n`.
///
/// An **empty** clip path clips everything away, and it is written as a
/// degenerate rectangle rather than skipped: §14.3.5 says so, and a build that
/// wrote nothing would draw the element unclipped — which is the opposite
/// answer and looks like a document that has no clip in it.
fn apply_clip(out: &mut Vec<u8>, clip: &Clip) {
    if clip.outline.segments.is_empty() {
        out.extend_from_slice(b"0 0 0 0 re W n\n");
        return;
    }
    write_outline(out, &clip.outline);
    out.extend_from_slice(match clip.rule {
        FillRule::NonZero => b"W n\n".as_slice(),
        FillRule::EvenOdd => b"W* n\n".as_slice(),
    });
}

/// Sets a fill or stroke colour, returning whether anything will paint.
///
/// A gradient's pattern name comes from [`Writer::pattern`] rather than being
/// added here, which keeps this a function of what was already registered.
fn set_paint(out: &mut Vec<u8>, paint: &Paint, pattern: Option<&[u8]>, stroking: bool) -> bool {
    match paint {
        Paint::None => false,
        Paint::Solid(colour) => {
            let c = |v: f64| number(v.clamp(0.0, 1.0));
            let [r, g, b] = colour.rgb;
            let op = if stroking { "RG" } else { "rg" };
            out.extend_from_slice(format!("{} {} {} {op}\n", c(r), c(g), c(b)).as_bytes());
            true
        }
        Paint::Linear { .. } | Paint::Radial { .. } => match pattern {
            // A gradient whose pattern the builder refused paints **nothing**
            // rather than falling back to a colour. §13.2's fallback is for a
            // paint server the *document* did not supply; inventing one here
            // would put a flat colour where a reader would see a ramp, and
            // nothing would say the ramp had been lost.
            None => false,
            Some(resource) => {
                // 8.7.3.2: the colour space becomes `/Pattern` before a
                // pattern name can be an operand.
                out.extend_from_slice(if stroking {
                    b"/Pattern CS "
                } else {
                    b"/Pattern cs "
                });
                name(out, resource);
                out.extend_from_slice(if stroking { b" SCN\n" } else { b" scn\n" });
                true
            }
        },
        _ => false,
    }
}

/// A gradient as 8.7.4.5's shading.
///
/// The stops become 7.10.4's stitching function over 7.10.3's exponential
/// pieces, which is how a PDF spells a multi-stop ramp: `k` stops are `k - 1`
/// linear segments, and the bounds between them are the stops' own offsets.
fn shading_of(paint: &Paint) -> Option<Shading> {
    let stops = match paint {
        Paint::Linear { stops, .. } | Paint::Radial { stops, .. } => stops,
        _ => return None,
    };
    if stops.len() < 2 {
        return None;
    }
    let colour = |at: usize| -> Vec<f64> { stops[at].colour.rgb.to_vec() };
    let mut functions = Vec::new();
    let mut bounds = Vec::new();
    let mut encode = Vec::new();
    for pair in 0..stops.len() - 1 {
        functions.push(Function::Exponential {
            domain: [0.0, 1.0],
            c0: colour(pair),
            c1: colour(pair + 1),
            n: 1.0,
        });
        encode.push([0.0, 1.0]);
        if pair + 2 < stops.len() {
            // 7.10.4 requires the bounds strictly increasing and strictly
            // inside the domain. §13.2.4 lets two stops share an offset — a
            // hard colour change — so a bound that has not advanced is nudged
            // by the smallest step that keeps the function legal rather than
            // dropping the stop.
            let previous = bounds.last().copied().unwrap_or(0.0);
            let next = stops[pair + 1].offset.clamp(0.0, 1.0);
            let next = if next > previous {
                next
            } else {
                next_after(previous)
            };
            if next >= 1.0 {
                // No room left for the remaining stops; the ramp ends here
                // rather than producing a function a reader refuses.
                functions.truncate(bounds.len() + 1);
                encode.truncate(bounds.len() + 1);
                break;
            }
            bounds.push(next);
        }
    }
    let function = if functions.len() == 1 {
        functions.remove(0)
    } else {
        Function::Stitching {
            domain: [0.0, 1.0],
            functions,
            bounds,
            encode,
        }
    };
    match paint {
        Paint::Linear { from, to, .. } => Some(Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [from[0], from[1], to[0], to[1]],
            function,
            // §13.2.3's `pad`, which is the initial `spreadMethod` and the one
            // the leaf crate has already narrowed every document to.
            extend: (true, true),
        }),
        Paint::Radial {
            centre,
            radius,
            focus,
            ..
        } => Some(Shading::Radial {
            color_space: DeviceSpace::Rgb,
            // 8.7.4.5.4 blends between two circles; SVG's focal point is the
            // first one with a radius of zero, which is exactly §13.2.3's
            // shape.
            coords: [focus[0], focus[1], 0.0, centre[0], centre[1], *radius],
            function,
            extend: (true, true),
        }),
        _ => None,
    }
}

/// The next representable value above `x`, for the stitching bound above.
///
/// `f64::next_up` is unstable, so it is written out — and it is exact
/// arithmetic on the bit pattern rather than an epsilon, because an epsilon
/// chosen here would be too large near zero and too small near one.
fn next_after(value: f64) -> f64 {
    if value.is_nan() || value >= 1.0 {
        return value;
    }
    f64::from_bits(value.to_bits() + 1)
}

/// §11.4's stroke parameters, as 8.4.3's operators.
fn set_stroke_state(out: &mut Vec<u8>, stroke: &tinker_pdf_svg::Stroke) {
    let mut text = format!(
        "{} w {} M",
        number(stroke.width),
        number(stroke.miter_limit.max(1.0))
    );
    text.push_str(match stroke.cap {
        LineCap::Butt => " 0 J",
        LineCap::Round => " 1 J",
        LineCap::Square => " 2 J",
    });
    text.push_str(match stroke.join {
        LineJoin::Miter => " 0 j",
        LineJoin::Round => " 1 j",
        LineJoin::Bevel => " 2 j",
    });
    let dashes: Vec<String> = stroke.dashes.iter().map(|v| number(*v)).collect();
    text.push_str(&format!(
        " [{}] {} d\n",
        dashes.join(" "),
        number(stroke.dash_offset)
    ));
    out.extend_from_slice(text.as_bytes());
}

// ---- §5.7's images ------------------------------------------------------------

/// Registers an image resource for an href, returning its **natural size**.
///
/// The size comes back because §7.8's fit needs it and nothing else has it:
/// `preserveAspectRatio` says how a picture sits in a box, and the answer
/// depends on the picture's own proportions, which are inside the bytes.
fn embed(
    builder: &mut DocumentBuilder,
    name: &[u8],
    href: &str,
    limit: usize,
    resolve: &mut impl FnMut(&str) -> Option<Vec<u8>>,
) -> Option<(f64, f64)> {
    let bytes = resolve(href)?;
    // The same two routes `cbz.rs` takes, and for its reasons: a JPEG is
    // placed verbatim because re-encoding is generational loss the caller
    // cannot undo, and a PNG goes through the reader that decides between
    // passing its `IDAT` through and decoding it.
    if let Some((wide, high, _)) = tinker_pdf_cos::jpeg_shape(&bytes) {
        return builder
            .add_image(name, &ImageData::Jpeg(&bytes))
            .then(|| (f64::from(wide), f64::from(high)));
    }
    // The caller's own entry ceiling, which is `cbz.rs`'s posture exactly: a
    // raster bigger than the biggest file the container may hold is not a
    // picture, and the number is the *host's* rather than the decoder's so a
    // host that lowered it can tell its decision from `MAX_PNG_SAMPLES`.
    let png = tinker_pdf_cos::png_image(&bytes, &FilterLimits::new(limit)).ok()?;
    let image = png.image();
    let shape = image_shape(&image)?;
    builder.add_image(name, &image).then_some(shape)
}

/// An image's pixel dimensions, whichever variant it arrived as.
///
/// Asked of the [`ImageData`] rather than of the reader that produced it,
/// which is `cbz.rs`'s `embedded_len` doctrine: one function over the type the
/// builder actually takes cannot disagree with what the builder writes.
fn image_shape(image: &ImageData<'_>) -> Option<(f64, f64)> {
    let (wide, high) = match image {
        ImageData::Rgb8 { width, height, .. } | ImageData::Gray8 { width, height, .. } => {
            (*width, *height)
        }
        ImageData::Compressed(compressed) => (compressed.width, compressed.height),
        _ => return None,
    };
    Some((f64::from(wide), f64::from(high)))
}

/// Places a registered image into an `<image>`'s rectangle.
///
/// 8.9.5.2 puts an image in the unit square with its **first row at the top**,
/// which is the same orientation an SVG `<image>` has — so the mapping is the
/// element's rectangle with a `y` flip inside it, and nothing here has to know
/// which way round the page is.
///
/// `preserveAspectRatio` is applied through the leaf crate's own
/// `transform::view_box`, given the rectangle as the viewport and the image's
/// natural box as the view box — which is exactly what §7.8 says the attribute
/// means, read by the code that already reads it for `<svg>`.
fn place_image(
    out: &mut Vec<u8>,
    resource: &[u8],
    rect: [f64; 4],
    element: [f64; 6],
    preserve: Option<&str>,
    natural: (f64, f64),
) {
    let [x, y, width, height] = rect;
    let fit = transform::view_box([0.0, 0.0, natural.0, natural.1], width, height, preserve);
    // A degenerate natural box cannot be fitted to anything, and the honest
    // answer is the one `preserveAspectRatio="none"` gives: fill the
    // rectangle the element stated.
    let (unit, natural) = match fit {
        Some(map) => (map, natural),
        None => ([width, 0.0, 0.0, height, 0.0, 0.0], (1.0, 1.0)),
    };
    // Unit square to the image's natural box, then §7.8's fit, then the
    // element's own position, then everything above it.
    let into_natural = [natural.0, 0.0, 0.0, natural.1, 0.0, 0.0];
    let placed = transform::concat(into_natural, unit);
    let placed = transform::concat(placed, [1.0, 0.0, 0.0, 1.0, x, y]);
    // 8.9.5.2's unit square has `y` up and an SVG rectangle has it down, so the
    // image is flipped inside its own box before anything else applies.
    let flip = [1.0, 0.0, 0.0, -1.0, 0.0, 1.0];
    let placed = transform::concat(flip, placed);
    let placed = transform::concat(placed, element);
    out.extend_from_slice(b"q ");
    matrix(out, placed);
    out.extend_from_slice(b" cm ");
    name(out, resource);
    out.extend_from_slice(b" Do Q\n");
}

// ---- §10's text ----------------------------------------------------------------

/// Where one run's baseline starts, in scene coordinates.
type Origin = [f64; 2];

/// Resolves §10.9's chunks into one origin per text node, in paint order,
/// groups looked through.
///
/// **Two passes, and the first one is why this is not done inline.** A chunk's
/// `text-anchor` cannot be applied until the whole chunk's width is known, and
/// a run that states no position of its own begins where the one before it
/// ended — so both need a measurement, and a measurement needs the same
/// metrics the book was paginated with. Doing it here means SVG text and the
/// book's own prose are measured by one `Metrics`.
fn place_text(nodes: &[Node], metrics: &BookMetrics<'_>) -> Vec<Option<Origin>> {
    struct Chunking {
        out: Vec<Option<Origin>>,
        chunk: Vec<(usize, f64)>,
        pen: [f64; 2],
        kind: TextAnchor,
    }

    fn flush(state: &mut Chunking) {
        let total: f64 = state.chunk.iter().map(|(_, width)| *width).sum();
        // §10.9's shift, applied to the whole chunk rather than to each run:
        // `middle` centres what the chunk holds, and a build that centred each
        // run would pile them on top of one another.
        let shift = match state.kind {
            TextAnchor::Start => 0.0,
            TextAnchor::Middle => -total / 2.0,
            TextAnchor::End => -total,
        };
        // **The shift alone.** Each run's origin already carries the pen at
        // the moment it was reached, so adding a running offset here as well
        // would advance every run twice -- which puts the second word of a
        // chunk two words along and is invisible in anything but a number.
        for (index, _) in state.chunk.drain(..) {
            if let Some(Some(origin)) = state.out.get_mut(index) {
                origin[0] += shift;
            }
        }
    }

    fn walk(nodes: &[Node], metrics: &BookMetrics<'_>, state: &mut Chunking) {
        for node in nodes {
            match node {
                Node::Group { nodes, .. } => walk(nodes, metrics, state),
                Node::Text {
                    text, anchor, font, ..
                } => {
                    let families = families_of(&font.families);
                    let width = metrics.measure(text, &request_of(font, &families));
                    if let Some(start) = anchor {
                        flush(state);
                        state.pen = *start;
                        state.kind = font.anchor;
                    }
                    let index = state.out.len();
                    state.out.push(Some(state.pen));
                    state.chunk.push((index, width));
                    state.pen[0] += width;
                }
                _ => {}
            }
        }
    }

    let mut state = Chunking {
        out: Vec::new(),
        chunk: Vec::new(),
        pen: [0.0, 0.0],
        kind: TextAnchor::Start,
    };
    walk(nodes, metrics, &mut state);
    flush(&mut state);
    state.out
}

/// One text run, as a text object under the run's own matrix.
///
/// Returns how many pieces the writer refused, which the caller turns into
/// [`crate::ArchiveWarning::UnwritableTextRun`] (ruling 10).
fn draw_text(
    builder: &mut DocumentBuilder,
    out: &mut Vec<u8>,
    node: &Node,
    origin: Origin,
    alpha: Option<&[u8]>,
    fonts: &Fonts<'_>,
    metrics: &BookMetrics<'_>,
) -> usize {
    let Node::Text {
        text,
        matrix: element,
        font,
        fill,
        ..
    } = node
    else {
        return 0;
    };
    let families = families_of(&font.families);
    let request = request_of(font, &families);
    // Glyph space is `y` up and SVG's run space is `y` down, so the run's own
    // matrix carries a flip at the baseline. Composed with the element's
    // matrix — and *not* with the page mapping, which the `cm` already in
    // force supplies.
    let local = transform::concat([1.0, 0.0, 0.0, -1.0, origin[0], origin[1]], *element);

    out.extend_from_slice(b"q\n");
    if let Some(resource) = alpha {
        gs(out, resource);
    }
    if let Paint::Solid(colour) = fill {
        set_paint(out, &Paint::Solid(*colour), None, false);
    }
    matrix(out, local);
    out.extend_from_slice(b" cm\n");

    let mut refused = 0usize;
    let mut pen = 0.0f64;
    for (range, chosen) in paint::face_runs(fonts.faces(), &request, text) {
        let slice = text.get(range).unwrap_or("");
        match chosen {
            Chosen::Embedded(index) => {
                let Some(face) = fonts.faces().faces().get(index) else {
                    refused += 1;
                    continue;
                };
                let Some(sfnt) = Sfnt::parse(&face.program) else {
                    refused += 1;
                    continue;
                };
                let mut bytes = Vec::new();
                let written = crate::shaping::write_run(
                    builder,
                    &mut bytes,
                    &crate::shaping::Run {
                        font: &face.resource,
                        face: &sfnt,
                        size: font.size,
                        matrix: [1.0, 0.0, 0.0, 1.0, pen, 0.0],
                        text: slice,
                        direction: BaseDirection::Auto,
                    },
                );
                if written {
                    out.extend_from_slice(&bytes);
                    out.push(b'\n');
                } else {
                    refused += 1;
                }
                // The pen moves by what the **metrics** say, not by what the
                // shaper's own advance came to: the book was paginated with
                // these numbers and a second answer here would put SVG text
                // and book text on two different grids.
                pen += metrics.measure(slice, &request);
            }
            Chosen::Standard(_) => {
                pen += draw_coded(out, fonts, metrics, &request, chosen, slice, pen);
            }
        }
    }
    out.extend_from_slice(b"Q\n");
    refused
}

/// A slice in one of the standard 14, as bytes in a simple font.
///
/// Returns the slice's advance. Consecutive characters that resolve to the
/// same resource are written as one string: a character outside
/// `WinAnsiEncoding` lands in an overflow font with a resource of its own, and
/// splitting only where the resource changes is what keeps a word one text
/// object wherever it can be.
///
/// Written as `PageBuilder::encoded_text` writes it, byte for byte: the
/// standard 14 are registered without a program, so there is no subset for the
/// characters to be recorded against, and the operators are the same whether
/// they land on a page or in a group's form.
fn draw_coded(
    out: &mut Vec<u8>,
    fonts: &Fonts<'_>,
    metrics: &BookMetrics<'_>,
    request: &FontRequest<'_>,
    chosen: Chosen,
    text: &str,
    start: f64,
) -> f64 {
    let size = request.size;
    let mut pen = 0.0f64;
    let mut current: Option<(Vec<u8>, Vec<u8>)> = None;
    let mut at = 0.0f64;
    let flush = |out: &mut Vec<u8>, held: Option<(Vec<u8>, Vec<u8>)>, x: f64| {
        let Some((resource, codes)) = held else {
            return;
        };
        out.extend_from_slice(b"BT ");
        name(out, &resource);
        out.extend_from_slice(
            format!(
                " {} Tf 0 Tc 0 Tw {} 0 Td (",
                number(size),
                number(start + x)
            )
            .as_bytes(),
        );
        for byte in codes {
            // 7.3.4.2's three, plus the two that a viewer would read as an
            // end-of-line inside a literal string and fold away.
            match byte {
                b'(' | b')' | b'\\' => {
                    out.push(b'\\');
                    out.push(byte);
                }
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\n' => out.extend_from_slice(b"\\n"),
                _ => out.push(byte),
            }
        }
        out.extend_from_slice(b") Tj ET\n");
    };
    for ch in text.chars() {
        let advance = metrics.advance(ch, request);
        let Some(coded) = fonts.encode(chosen, ch) else {
            // No code anywhere for this character; the run keeps its width so
            // that what follows stays where the document put it, and the
            // caller already counts it through `Fonts::unrepresented`.
            pen += advance;
            continue;
        };
        match &mut current {
            Some((resource, codes)) if resource == coded.resource() => {
                if let Coded::Simple { code, .. } = coded {
                    codes.push(code);
                }
            }
            _ => {
                flush(out, current.take(), at);
                at = pen;
                if let Coded::Simple { resource, code } = coded {
                    current = Some((resource, vec![code]));
                }
            }
        }
        pen += advance;
    }
    flush(out, current.take(), at);
    pen
}

/// Records every character an SVG's text runs need a code for.
///
/// The same `Fonts::note` a chapter's runs go through, reached with a
/// `TextRun` built from the scene — which is a translation rather than a second
/// encoder: a build that gave SVG text its own code assignment would let one
/// character land in two different overflow fonts in one document, and no page
/// would say which was which.
pub fn note(scene: &Scene, fonts: &mut Fonts<'_>) {
    note_nodes(&scene.nodes, fonts);
}

fn note_nodes(nodes: &[Node], fonts: &mut Fonts<'_>) {
    for node in nodes {
        let (text, font) = match node {
            Node::Group { nodes, .. } => {
                note_nodes(nodes, fonts);
                continue;
            }
            Node::Text { text, font, .. } => (text, font),
            _ => continue,
        };
        fonts.note(&TextRun {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            text: text.clone(),
            font_size: font.size,
            families: families_of(&font.families),
            weight: font.weight,
            style: if font.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            variant: FontVariant::Normal,
            color: Color::BLACK,
            decoration: TextDecoration::None,
            painted: true,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            // **Not generated.** 14.8.2.2's artifact marking is for content the
            // source did not contain, and every character here is in the
            // document — an SVG label extracts as text, exactly as a paragraph
            // does.
            generated: false,
            anchor: None,
            order: 0,
        });
    }
}
