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
    CalculatorOp, DeviceSpace, DocumentBuilder, ExtGState, FormXObject, Function, ImageData,
    MaskKind, PageBuilder, Shading, ShadingPattern, StateMask, TilingPattern, TilingType,
    TransparencyGroup,
};
use tinker_pdf_css::property::{
    Color, FontFamily, FontKerning, FontStyle, FontVariant, TextDecoration,
};
use tinker_pdf_filters::Limits as FilterLimits;
use tinker_pdf_font::Sfnt;
use tinker_pdf_layout::metrics::{FontRequest, Metrics};
use tinker_pdf_layout::TextRun;
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{
    transform, Clip, FillRule, LineCap, LineJoin, Node, Paint, Scene, Spread, Stop, TextAnchor,
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
        kerning: FontKerning::Auto,
        features: &[],
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
        grey: false,
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
///
/// `grey` is set inside a §14.4 mask's content, where every flat colour and
/// every stop is written as its own luminance — see [`luminance`].
#[derive(Clone, Copy, Debug)]
struct Space {
    base: [f64; 6],
    extent: [f64; 4],
    grey: bool,
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
                let filled = set_paint(out, fill, fill_pattern.as_deref(), false, space.grey);
                let stroked = stroke.as_ref().is_some_and(|stroke| {
                    let painted = set_paint(
                        out,
                        &stroke.paint,
                        stroke_pattern.as_deref(),
                        true,
                        space.grey,
                    );
                    if painted {
                        set_stroke_state(out, stroke);
                    }
                    painted
                });
                // §11.4 strokes in the element's user space, and the outline
                // is already in the scene's: so a stroke under any matrix but
                // the identity is painted under that matrix — the `w` and `d`
                // set above are read in the user space in force when `S` is,
                // 8.4.3.2 — with the outline taken back through its inverse.
                // A space with no area has no stroke either.
                let user = stroke
                    .as_ref()
                    .filter(|_| stroked)
                    .map(|stroke| stroke.matrix)
                    .filter(|m| *m != transform::IDENTITY);
                let (local, stroked) = match user {
                    None => (None, stroked),
                    Some(m) => match transform::invert(m)
                        .map(|inverse| outline.transformed(inverse))
                        .filter(outline_finite)
                    {
                        Some(local) => (Some((m, local)), stroked),
                        None => (None, false),
                    },
                };
                if filled || stroked {
                    if let Some((m, local)) = &local {
                        out.extend_from_slice(b"q ");
                        matrix(out, *m);
                        out.extend_from_slice(b" cm\n");
                        write_outline(out, local);
                    } else {
                        write_outline(out, outline);
                    }
                    out.extend_from_slice(operator(filled, stroked, *rule));
                    out.push(b'\n');
                    if local.is_some() {
                        out.extend_from_slice(b"Q\n");
                    }
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
            Node::Text {
                fill,
                fill_opacity,
                stroke,
                hidden,
                ..
            } => {
                let Some(origin) = cursor.take() else {
                    return;
                };
                // §11.5: laid out — `place_text` moved the pen past it, and
                // its origin is spent here — and not painted, not even as
                // 9.3.6's invisible text a reader would extract.
                if *hidden {
                    return;
                }
                let stroke_alpha = stroke.as_ref().map_or(1.0, |stroke| stroke.opacity);
                let alpha = if *fill_opacity < 1.0 || stroke_alpha < 1.0 {
                    self.alpha(*fill_opacity, stroke_alpha)
                } else {
                    None
                };
                // A run's gradient or pattern is the shape's: registered here,
                // before the page is begun, and named in the run's paint.
                let patterns = Patterns {
                    fill: self.pattern(fill, space),
                    stroke: stroke
                        .as_ref()
                        .and_then(|stroke| self.pattern(&stroke.paint, space)),
                };
                self.drawn.refused += draw_text(
                    self.builder,
                    out,
                    node,
                    origin,
                    alpha.as_deref(),
                    &patterns,
                    space.grey,
                    self.fonts,
                    self.metrics,
                );
            }
            Node::Group {
                nodes,
                opacity,
                clip,
                mask,
            } => self.group(
                out,
                nodes,
                *opacity,
                clip.as_ref(),
                mask.as_deref(),
                space,
                cursor,
            ),
            // `Node` is `#[non_exhaustive]`: a variant added to the leaf crate
            // reaches here as ink nobody drew, so it is counted rather than
            // skipped.
            _ => self.drawn.refused += 1,
        }
    }

    /// §14.5's group, §14.3.5's clip of one, and §14.4's mask of one.
    ///
    /// A group at full opacity with no mask is only a clip, which a `q`/`Q`
    /// pair around its nodes says exactly. Otherwise it is 11.6.6's
    /// transparency group: the nodes are composited in a form of their own,
    /// and that form is painted once under 11.6.4.4's constant alpha — which
    /// 11.6.6 applies to the group's result as a whole, and which is §14.5
    /// word for word — and under 11.6.5.2's `/Luminosity` soft mask, whose
    /// group is the mask's content drawn in a form of its own.
    #[allow(clippy::too_many_arguments)]
    fn group(
        &mut self,
        out: &mut Vec<u8>,
        nodes: &[Node],
        opacity: f64,
        clip: Option<&Clip>,
        mask: Option<&tinker_pdf_svg::Mask>,
        space: Space,
        cursor: &mut Cursor,
    ) {
        if opacity >= 1.0 && mask.is_none() {
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
        let state = match mask {
            None => self.alpha(opacity, opacity),
            Some(mask) => self.masking(mask, opacity, space),
        };
        let (true, Some(state)) = (registered, state) else {
            self.drawn.refused += 1;
            return;
        };
        out.extend_from_slice(b"q\n");
        if let Some(clip) = clip {
            apply_clip(out, clip);
        }
        // The state is set **after** the page mapping is undone: 11.6.5.2
        // places a soft mask's group in the coordinate system in force when
        // the `gs` is executed, and the mask's form, like every form here,
        // puts the mapping back as its first operator.
        out.extend_from_slice(b"q ");
        matrix(out, undo);
        out.extend_from_slice(b" cm ");
        gs(out, &state);
        name(out, &form);
        out.extend_from_slice(b" Do Q\nQ\n");
    }

    /// §14.4's mask as 11.6.5.2's soft mask: its content in a transparency
    /// group of its own, read for `/Luminosity`, clipped to the mask region —
    /// outside which the group's backdrop, black, masks everything away.
    ///
    /// The content is written with every flat colour and every stop as its
    /// own luminance ([`luminance`]), because 11.6.5.3 derives a luminosity
    /// from an RGB group by its own weights and §14.4 by CSS Masking's; a
    /// grey is the one colour on which the two agree.
    fn masking(
        &mut self,
        mask: &tinker_pdf_svg::Mask,
        opacity: f64,
        space: Space,
    ) -> Option<Vec<u8>> {
        let inside = Space {
            grey: true,
            ..space
        };
        let mut cursor = Cursor {
            origins: place_text(&mask.nodes, self.metrics),
            next: 0,
        };
        let mut content = Vec::new();
        content.extend_from_slice(b"q ");
        matrix(&mut content, space.base);
        content.extend_from_slice(b" cm\n");
        // A mask with no region of its own — a clip's silhouettes — is
        // bounded by its form's `/BBox` and the black backdrop alone.
        if let Some(region) = &mask.region {
            apply_clip(
                &mut content,
                &Clip {
                    outline: region.clone(),
                    rule: FillRule::NonZero,
                },
            );
        }
        self.nodes(&mut content, &mask.nodes, inside, &mut cursor);
        content.extend_from_slice(b"Q\n");
        let form = self.name("M");
        let registered = self.builder.add_form(
            &form,
            &FormXObject {
                bbox: space.extent,
                matrix: None,
                group: Some(TransparencyGroup {
                    color_space: DeviceSpace::Rgb,
                    isolated: true,
                    knockout: false,
                }),
                content: &content,
            },
        );
        if !registered {
            return None;
        }
        let alpha = (opacity < 1.0).then_some(opacity);
        let state = self.name("K");
        self.builder
            .add_ext_gstate(
                &state,
                &ExtGState {
                    fill_alpha: alpha,
                    stroke_alpha: alpha,
                    soft_mask: Some(StateMask::Group {
                        kind: MaskKind::Luminosity,
                        form: &form,
                        backdrop: None,
                    }),
                    ..ExtGState::default()
                },
            )
            .then_some(state)
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
        if let Paint::Pattern(tile) = paint {
            return self.tiling(tile, space);
        }
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
        let toned;
        let paint = if space.grey {
            toned = grey_stops(paint);
            &toned
        } else {
            paint
        };
        let shading = shading_of(paint, matrix, space.extent)?;
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

    /// §13.3's pattern as 8.7.3's tiling pattern: one cell, the tile's nodes
    /// drawn in pattern space, repeated at the tile's width and height.
    ///
    /// The cell's content stream is pattern space with no mapping of its
    /// own — the nodes are already in it — and its `/BBox` is the tile, which
    /// is the clip §13.3's `overflow: hidden` asks for. The pattern's
    /// `/Matrix` carries pattern space into the stream's default space,
    /// 8.7.3.1's rule and the gradient's.
    fn tiling(&mut self, tile: &tinker_pdf_svg::Tile, space: Space) -> Option<Vec<u8>> {
        let [x, y, width, height] = tile.cell;
        let cell = Space {
            base: transform::IDENTITY,
            extent: [x, y, x + width, y + height],
            grey: space.grey,
        };
        let content = self.stream(&tile.nodes, cell);
        let name = self.name("T");
        self.builder
            .add_tiling_pattern(
                &name,
                &TilingPattern {
                    bbox: cell.extent,
                    x_step: width,
                    y_step: height,
                    matrix: Some(transform::concat(tile.matrix, space.base)),
                    tiling_type: TilingType::ConstantSpacing,
                    content: &content,
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
/// Whether every point of an outline is a number a content stream can carry —
/// which one taken back through a nearly singular matrix may not be.
fn outline_finite(outline: &tinker_pdf_svg::path::Outline) -> bool {
    outline.segments.iter().all(|segment| match *segment {
        Segment::Move(p) | Segment::Line(p) => p.iter().all(|v| v.is_finite()),
        Segment::Cubic(a, b, c) => [a, b, c].iter().flatten().all(|v| v.is_finite()),
        _ => true,
    })
}

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
fn set_paint(
    out: &mut Vec<u8>,
    paint: &Paint,
    pattern: Option<&[u8]>,
    stroking: bool,
    grey: bool,
) -> bool {
    match paint {
        Paint::None => false,
        Paint::Solid(colour) => {
            let c = |v: f64| number(v.clamp(0.0, 1.0));
            let [r, g, b] = if grey {
                [luminance(colour.rgb); 3]
            } else {
                colour.rgb
            };
            let op = if stroking { "RG" } else { "rg" };
            out.extend_from_slice(format!("{} {} {} {op}\n", c(r), c(g), c(b)).as_bytes());
            true
        }
        Paint::Linear { .. } | Paint::Radial { .. } | Paint::Pattern(_) => match pattern {
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

/// CSS Masking 1's luminance of a colour: `feColorMatrix`'s
/// `luminanceToAlpha` weights, in `color-interpolation`'s initial sRGB.
///
/// SVG 1.1 §14.4 asked for linearRGB first; CSS Masking replaced that text and
/// every reading system follows it. Written out as a grey so that 11.6.5.3's
/// own luminosity of it — 0.30, 0.59 and 0.11 of an RGB group, which sum to
/// one — is exactly this number.
fn luminance(rgb: [f64; 3]) -> f64 {
    (0.2125 * rgb[0] + 0.7154 * rgb[1] + 0.0721 * rgb[2]).clamp(0.0, 1.0)
}

/// A gradient whose every stop is its own [`luminance`], for a mask.
fn grey_stops(paint: &Paint) -> Paint {
    let mut out = paint.clone();
    if let Paint::Linear { stops, .. } | Paint::Radial { stops, .. } = &mut out {
        for stop in stops.iter_mut() {
            stop.colour.rgb = [luminance(stop.colour.rgb); 3];
        }
    }
    out
}

/// A gradient as 8.7.4.5's shading.
///
/// `to_default` is the pattern matrix — gradient space to the stream's
/// default space — and `extent` the default space's visible box; together they
/// say how far past its axis a `reflect` or `repeat` gradient has to reach.
///
/// The stops become 7.10.4's stitching function over 7.10.3's exponential
/// pieces, which is how a PDF spells a multi-stop ramp: `k` stops are `k - 1`
/// linear segments, and the bounds between them are the stops' own offsets.
/// A `reflect` or `repeat` gradient is [`spread_shading`]'s instead.
fn shading_of(paint: &Paint, to_default: [f64; 6], extent: [f64; 4]) -> Option<Shading> {
    let (stops, spread) = match paint {
        Paint::Linear { stops, spread, .. } | Paint::Radial { stops, spread, .. } => {
            (stops, *spread)
        }
        _ => return None,
    };
    if stops.len() < 2 {
        return None;
    }
    if spread != Spread::Pad {
        return spread_shading(paint, stops, spread, to_default, extent);
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
            // §13.2.3's `pad`: the end stops' colours past the axis, which is
            // 8.7.4.5.3's `/Extend` exactly. `reflect` and `repeat` never reach
            // here; `spread_shading` writes those.
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

/// How many periods past its axis a `reflect` or `repeat` gradient is drawn.
///
/// **Not a resource bound**, and so not in `bounds_ledger.rs`: the function
/// that tiles the ramp is the same size whatever its domain, so a gradient
/// whose period is a millionth of the page costs what one the size of the
/// page does. It is a sanity bound on a *number*: a domain this wide is a
/// period below a millionth of the visible box across it, finer than any
/// pixel, and past it the shading pads with whichever stop it reached.
const SPREAD_PERIODS: f64 = (1u64 << 20) as f64;

/// §13.2.3's `reflect` and `repeat`, as one shading over the visible box.
///
/// # Why a calculator and not more stitched stops
///
/// The obvious spelling is the pad shading with its axis stretched and its
/// stitching function repeated once per period — which is right, and costs a
/// sub-function per period, so a gradient whose period is a hair across a
/// page is a function of a million pieces. 7.10.5's calculator tiles the ramp
/// in a dozen operators however many periods there are: `t − ⌊t⌋` for
/// `repeat`, and its reflection about one for `reflect`, followed by the ramp
/// itself as a binary search over the stops. The number of periods is still
/// derived — from the visible box taken back into gradient space, which is the
/// furthest any part of the shape can be — but only as the domain's two ends.
///
/// The axis's parameter `s` is SVG's: 0 at the first stop's end of the axis
/// and 1 at the last's, so the shading's coordinates are the axis stretched to
/// `[s0, s1]` and the function maps 8.7.4.5.3's `t ∈ [0, 1]` back onto it.
/// For a radial gradient `s` is the circle's: the focus at 0, the stated
/// circle at 1, and the shading's second circle at `s1`.
fn spread_shading(
    paint: &Paint,
    stops: &[Stop],
    spread: Spread,
    to_default: [f64; 6],
    extent: [f64; 4],
) -> Option<Shading> {
    let inverse = transform::invert(to_default)?;
    let [x0, y0, x1, y1] = extent;
    let corners: Vec<[f64; 2]> = [[x0, y0], [x1, y0], [x0, y1], [x1, y1]]
        .iter()
        .map(|corner| transform::apply(inverse, *corner))
        .collect();
    match paint {
        Paint::Linear { from, to, .. } => {
            let d = [to[0] - from[0], to[1] - from[1]];
            let length = d[0] * d[0] + d[1] * d[1];
            if !(length > 0.0 && length.is_finite()) {
                return None;
            }
            let along = |p: &[f64; 2]| ((p[0] - from[0]) * d[0] + (p[1] - from[1]) * d[1]) / length;
            let low = corners.iter().map(along).fold(0.0f64, f64::min).floor();
            let high = corners.iter().map(along).fold(1.0f64, f64::max).ceil();
            let low = low.max(-SPREAD_PERIODS);
            let high = high.min(SPREAD_PERIODS);
            let at = |s: f64| [from[0] + s * d[0], from[1] + s * d[1]];
            let (start, end) = (at(low), at(high));
            Some(Shading::Axial {
                color_space: DeviceSpace::Rgb,
                coords: [start[0], start[1], end[0], end[1]],
                function: spread_function(stops, spread, low, high)?,
                extend: (true, true),
            })
        }
        Paint::Radial {
            centre,
            radius,
            focus,
            ..
        } => {
            // §13.2.3 puts the focus inside the circle; on it or past it the
            // cone of circles never covers what lies behind the focus, so it
            // is drawn in to 99% of the radius — the nudge every renderer
            // makes — rather than leaving half the plane unpainted.
            let mut d = [centre[0] - focus[0], centre[1] - focus[1]];
            let mut focus = *focus;
            let offset = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if offset >= 0.99 * radius {
                let keep = 0.99 * radius / offset;
                d = [d[0] * keep, d[1] * keep];
                focus = [centre[0] - d[0], centre[1] - d[1]];
            }
            // A point p is inside the circle at s when |q − s·d| ≤ s·r, with
            // q = p − focus. Squared, that is a quadratic in s whose leading
            // coefficient r² − |d|² is positive while the focus is inside, and
            // its larger root is the smallest s whose circle holds p.
            let a = radius * radius - (d[0] * d[0] + d[1] * d[1]);
            if !(a > 0.0 && a.is_finite()) {
                return None;
            }
            let reach = |p: &[f64; 2]| {
                let q = [p[0] - focus[0], p[1] - focus[1]];
                let qd = q[0] * d[0] + q[1] * d[1];
                let qq = q[0] * q[0] + q[1] * q[1];
                (-qd + (qd * qd + a * qq).sqrt()) / a
            };
            let high = corners
                .iter()
                .map(reach)
                .fold(1.0f64, f64::max)
                .ceil()
                .min(SPREAD_PERIODS);
            if !high.is_finite() {
                return None;
            }
            Some(Shading::Radial {
                color_space: DeviceSpace::Rgb,
                coords: [
                    focus[0],
                    focus[1],
                    0.0,
                    focus[0] + high * d[0],
                    focus[1] + high * d[1],
                    high * radius,
                ],
                function: spread_function(stops, spread, 0.0, high)?,
                extend: (true, true),
            })
        }
        _ => None,
    }
}

/// The calculator that tiles a ramp: 8.7.4.5's `t` onto `[low, high]`, the
/// period folded back into `[0, 1]`, and the stops evaluated there.
fn spread_function(stops: &[Stop], spread: Spread, low: f64, high: f64) -> Option<Function> {
    use CalculatorOp::{Number, Operator};
    let first = stops.first()?;
    let last = stops.last()?;
    let mut program = vec![
        // t ∈ [0, 1] to s ∈ [low, high].
        Number(high - low),
        Operator("mul"),
        Number(low),
        Operator("add"),
    ];
    match spread {
        // u = s − ⌊s⌋, in [0, 1).
        Spread::Repeat => program.extend([Operator("dup"), Operator("floor"), Operator("sub")]),
        // u = s − 2⌊s/2⌋, in [0, 2), and then 2 − u past one: the ramp
        // forwards on even periods and backwards on odd ones.
        Spread::Reflect => program.extend([
            Operator("dup"),
            Number(2.0),
            Operator("div"),
            Operator("floor"),
            Number(2.0),
            Operator("mul"),
            Operator("sub"),
            Operator("dup"),
            Number(1.0),
            Operator("gt"),
            CalculatorOp::If(vec![Number(2.0), Operator("exch"), Operator("sub")]),
        ]),
        Spread::Pad => {}
    }
    // Before the first stop's offset the first stop's colour, after the last
    // the last's: §13.2.4's own rule inside one period.
    program.extend([
        Operator("dup"),
        Number(first.offset),
        Operator("lt"),
        CalculatorOp::If(vec![Operator("pop"), Number(first.offset)]),
        Operator("dup"),
        Number(last.offset),
        Operator("gt"),
        CalculatorOp::If(vec![Operator("pop"), Number(last.offset)]),
    ]);
    program.extend(ramp(stops, 0, stops.len() - 1));
    Some(Function::Calculator {
        domain: vec![[0.0, 1.0]],
        range: vec![[0.0, 1.0]; 3],
        program,
    })
}

/// The stops' piecewise-linear ramp over segments `[lo, hi)`, as a binary
/// search: one comparison per halving, so a ramp of a thousand stops nests ten
/// deep rather than a thousand — 7.10.5 has no `min`, no `max` and no table,
/// and a chain of `ifelse`s one per stop would pass this writer's nesting
/// limit at seventeen.
///
/// Enters with `u` on the stack and leaves `r g b`.
fn ramp(stops: &[Stop], lo: usize, hi: usize) -> Vec<CalculatorOp> {
    use CalculatorOp::{IfElse, Number, Operator};
    if hi <= lo + 1 {
        let (Some(a), Some(b)) = (stops.get(lo), stops.get(lo + 1)) else {
            return Vec::new();
        };
        let width = b.offset - a.offset;
        // A hard stop — two stops at one offset — is a segment of no width,
        // which the search above never lands inside; it is the second
        // stop's colour should it ever be reached.
        let (base, slope): ([f64; 3], [f64; 3]) = if width > 0.0 {
            let mut slope = [0.0; 3];
            for (channel, value) in slope.iter_mut().enumerate() {
                *value = (b.colour.rgb[channel] - a.colour.rgb[channel]) / width;
            }
            (a.colour.rgb, slope)
        } else {
            (b.colour.rgb, [0.0; 3])
        };
        // d = u − offset, kept three times: d d d → d d r → r d d → r d g →
        // r g d → r g b.
        return vec![
            Number(a.offset),
            Operator("sub"),
            Operator("dup"),
            Operator("dup"),
            Number(slope[0]),
            Operator("mul"),
            Number(base[0]),
            Operator("add"),
            Number(3.0),
            Number(1.0),
            Operator("roll"),
            Number(slope[1]),
            Operator("mul"),
            Number(base[1]),
            Operator("add"),
            Operator("exch"),
            Number(slope[2]),
            Operator("mul"),
            Number(base[2]),
            Operator("add"),
        ];
    }
    let middle = lo + (hi - lo) / 2;
    let split = stops.get(middle).map_or(0.0, |stop| stop.offset);
    vec![
        Operator("dup"),
        Number(split),
        Operator("lt"),
        IfElse(ramp(stops, lo, middle), ramp(stops, middle, hi)),
    ]
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
                    text,
                    anchor,
                    continues_x,
                    font,
                    ..
                } => {
                    let families = families_of(&font.families);
                    let width = metrics.measure(text, &request_of(font, &families));
                    if let Some(start) = anchor {
                        flush(state);
                        // §10.5's rule (b): a chunk opened by a `y` alone
                        // starts where the pen is, plus the `dx` it carries.
                        let x = if *continues_x {
                            state.pen[0] + start[0]
                        } else {
                            start[0]
                        };
                        state.pen = [x, start[1]];
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

/// A run measured as [`place_text`] places it, for the leaf's box of text.
///
/// `tinker-pdf-svg` resolves a `mask`, a `clip-path` or a paint server in
/// `objectBoundingBox` units on text against the text's glyph cells, which
/// are a font's (ruling 8), so the facade lends it these: the advance is
/// [`Metrics::measure`] exactly as `place_text` and `draw_text` move the pen
/// by it — the leaf replays `place_text` with it, so the box is where the ink
/// is — and the ascent and descent are those of the face the run's request
/// resolves to, which is SVG 2 §8.10's glyph cell. A run whose characters
/// fall to more than one face is measured by the first face's, the one
/// `Metrics::vertical` answers for.
impl tinker_pdf_svg::MeasureText for BookMetrics<'_> {
    fn measure(&self, text: &str, font: &tinker_pdf_svg::TextStyle) -> tinker_pdf_svg::RunMetrics {
        let families = families_of(&font.families);
        let request = request_of(font, &families);
        let vertical = Metrics::vertical(self, &request);
        tinker_pdf_svg::RunMetrics {
            advance: Metrics::measure(self, text, &request),
            ascent: vertical.ascent,
            descent: vertical.descent,
        }
    }
}

/// Whether a scene has text a bounding-box effect needed the box of, which a
/// read with [`tinker_pdf_svg::Context::with_measure`] can give it.
#[must_use]
pub fn unmeasured(scene: &Scene) -> bool {
    scene
        .warnings
        .contains(&tinker_pdf_svg::Warning::TextBoxUnmeasured)
}

/// The pattern resources a run's fill and stroke are painted with, where
/// either is a gradient or a pattern the builder took.
struct Patterns {
    fill: Option<Vec<u8>>,
    stroke: Option<Vec<u8>>,
}

/// One text run, as a text object under the run's own matrix.
///
/// Painted as its `fill` and `stroke` say, through 9.3.6's rendering modes,
/// which are SVG's four combinations of the two exactly: a fill alone is the
/// initial mode 0, a stroke alone 1, both 2, and neither 3 — invisible, and
/// still text a reader extracts and searches, which is what `fill="none"` on
/// a label is. `Tr` is graphics state, so the run's `q`/`Q` scopes it. Until
/// this the run's solid fill was set and nothing else: a gradient, a pattern
/// or `none` drew in whatever colour the state held, and a stroke was never
/// drawn.
///
/// Returns how many pieces the writer refused, which the caller turns into
/// [`crate::ArchiveWarning::UnwritableTextRun`] (ruling 10).
#[allow(clippy::too_many_arguments)]
fn draw_text(
    builder: &mut DocumentBuilder,
    out: &mut Vec<u8>,
    node: &Node,
    origin: Origin,
    alpha: Option<&[u8]>,
    patterns: &Patterns,
    grey: bool,
    fonts: &Fonts<'_>,
    metrics: &BookMetrics<'_>,
) -> usize {
    let Node::Text {
        text,
        matrix: element,
        font,
        fill,
        stroke,
        rotate,
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
    // force supplies. §10.5's `rotate` turns the glyph about its own origin,
    // so it sits between the flip and the move to the origin: clockwise in
    // the downward space, which is what a positive angle means there.
    let turned = if *rotate == 0.0 {
        [1.0, 0.0, 0.0, -1.0, 0.0, 0.0]
    } else {
        transform::concat(
            [1.0, 0.0, 0.0, -1.0, 0.0, 0.0],
            transform::rotation(*rotate),
        )
    };
    let local = transform::concat(
        transform::concat(turned, [1.0, 0.0, 0.0, 1.0, origin[0], origin[1]]),
        *element,
    );

    out.extend_from_slice(b"q\n");
    if let Some(resource) = alpha {
        gs(out, resource);
    }
    // Set before the run's `cm`, as a shape's are: a line width is read in
    // the user space in force when the glyphs are stroked, which is the run's,
    // and a pattern's space is the stream's default whatever the `cm`.
    let filled = set_paint(out, fill, patterns.fill.as_deref(), false, grey);
    let stroked = stroke.as_ref().is_some_and(|stroke| {
        let painted = set_paint(out, &stroke.paint, patterns.stroke.as_deref(), true, grey);
        if painted {
            set_stroke_state(out, stroke);
        }
        painted
    });
    match (filled, stroked) {
        (true, false) => {}
        (false, true) => out.extend_from_slice(b"1 Tr\n"),
        (true, true) => out.extend_from_slice(b"2 Tr\n"),
        (false, false) => out.extend_from_slice(b"3 Tr\n"),
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

/// The runs a pattern's tile draws, which are drawn in its cell's stream and
/// need their faces as much as the page's do.
fn note_paint(paint: &Paint, fonts: &mut Fonts<'_>) {
    if let Paint::Pattern(tile) = paint {
        note_nodes(&tile.nodes, fonts);
    }
}

/// Every run a node list draws — at every depth of a group, inside a mask,
/// and inside a tile of any paint — noted, so that every face a stream names
/// is one the registry writes.
fn note_nodes(nodes: &[Node], fonts: &mut Fonts<'_>) {
    for node in nodes {
        let (text, font) = match node {
            Node::Group { nodes, mask, .. } => {
                note_nodes(nodes, fonts);
                if let Some(mask) = mask {
                    note_nodes(&mask.nodes, fonts);
                }
                continue;
            }
            Node::Path { fill, stroke, .. } => {
                note_paint(fill, fonts);
                if let Some(stroke) = stroke {
                    note_paint(&stroke.paint, fonts);
                }
                continue;
            }
            // A hidden run is measured and never drawn, so it needs no code.
            Node::Text { hidden: true, .. } => continue,
            Node::Text {
                text,
                font,
                fill,
                stroke,
                ..
            } => {
                note_paint(fill, fonts);
                if let Some(stroke) = stroke {
                    note_paint(&stroke.paint, fonts);
                }
                (text, font)
            }
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
            kerning: FontKerning::Auto,
            features: Vec::new(),
            paragraph_rtl: Some(false),
            embeddings: Vec::new(),
            bidi_level: None,
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
