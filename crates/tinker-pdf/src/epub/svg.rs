//! An SVG spine item, drawn onto a page (gap 31's SVG lane, milestone 7).
//!
//! `tinker-pdf-svg` reads a document into a [`Scene`] — filled and stroked
//! outlines, gradients, clips, images and text runs, all in one coordinate
//! space, with everything it declined named in [`Scene::warnings`]. This file
//! is the other half: it turns that display list into content-stream
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
//! # Three things this file decides, and one it does not
//!
//! **The page mapping.** SVG's user space has `y` growing downward from the
//! top left and a PDF page has it growing upward from the bottom left, so one
//! `cm` at the top of the stream carries the flip, the scale and the centring
//! for the whole scene. Every coordinate written after it is the scene's own,
//! which is what makes the operators readable beside the fixture.
//!
//! **Where a gradient's matrix goes.** 8.7.3.1 makes pattern space *"the
//! default coordinate system of the page"* — it ignores whatever `cm` is in
//! force. So a shading pattern's `/Matrix` is composed with the page mapping
//! here rather than inherited from it, and that is the one place the flip has
//! to be written twice.
//!
//! **How text is set.** Through `paint::face_runs` and
//! [`crate::shaping::write_run`], which is the same `css-fonts-4` §5.3 matcher
//! and the same shaper that set the rest of the book. A second matcher here
//! would let SVG text and XHTML text in one book resolve `serif` differently,
//! and no page would say which was right.
//!
//! What it does **not** decide is what the document said: every property, every
//! transform and every refusal was settled in the leaf crate.

use tinker_pdf_cos::build::{
    DeviceSpace, DocumentBuilder, ExtGState, Function, ImageData, PageBuilder, Shading,
    ShadingPattern,
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
    /// evaluate, a glyph run the builder declined.
    pub refused: usize,
    /// `<image>` references this build did not resolve into a page.
    pub images_unresolved: usize,
    /// `<image>` references that resolved and were embedded.
    pub images_drawn: usize,
}

/// Every resource a scene needs, registered **before its page begins**.
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
/// So registration is a pass of its own, run before `begin_page`, and drawing
/// takes the names it produced rather than a builder it could add to. The type
/// is the enforcement: [`draw`] cannot register anything, because it is handed
/// a `&Registry`.
#[derive(Debug, Default)]
pub struct Registry {
    /// Per node: the `/ExtGState` carrying its fill and stroke alpha.
    alpha: Vec<(usize, Vec<u8>)>,
    /// Per node: the `/Pattern` its fill resolved to.
    fill_pattern: Vec<(usize, Vec<u8>)>,
    /// Per node: the `/Pattern` its stroke resolved to.
    stroke_pattern: Vec<(usize, Vec<u8>)>,
    /// Per node: the `/XObject` its `<image>` resolved to, and the picture's
    /// own pixel dimensions.
    images: Vec<(usize, Vec<u8>, (f64, f64))>,
    /// `<image>` references that named nothing this build could draw.
    images_unresolved: usize,
}

impl Registry {
    fn named(list: &[(usize, Vec<u8>)], at: usize) -> Option<&[u8]> {
        list.iter()
            .find(|(index, _)| *index == at)
            .map(|(_, name)| name.as_slice())
    }
}

/// Registers everything a scene will name, before its page is begun.
///
/// `resolve` answers an `<image>` href: a closure rather than a parameter
/// because what an href resolves against is the *container*, and the container
/// is the caller's — the same boundary `tinker-pdf-css`'s `ImportResolver`
/// draws for an `@import`.
pub fn register(
    builder: &mut DocumentBuilder,
    scene: &Scene,
    placement: Placement,
    mut resolve: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Registry {
    let base = placement.matrix(scene.size);
    let mut out = Registry::default();
    for (at, node) in scene.nodes.iter().enumerate() {
        match node {
            Node::Path {
                fill,
                fill_opacity,
                stroke,
                ..
            } => {
                let stroke_alpha = stroke.as_ref().map_or(1.0, |stroke| stroke.opacity);
                if *fill_opacity < 1.0 || stroke_alpha < 1.0 {
                    let name = format!("SvgA{at}").into_bytes();
                    if builder.add_ext_gstate(
                        &name,
                        &ExtGState {
                            fill_alpha: Some(*fill_opacity),
                            stroke_alpha: Some(stroke_alpha),
                            ..ExtGState::default()
                        },
                    ) {
                        out.alpha.push((at, name));
                    }
                }
                if let Some(name) = pattern(builder, fill, base, &format!("SvgF{at}")) {
                    out.fill_pattern.push((at, name));
                }
                if let Some(stroke) = stroke {
                    if let Some(name) = pattern(builder, &stroke.paint, base, &format!("SvgS{at}"))
                    {
                        out.stroke_pattern.push((at, name));
                    }
                }
            }
            Node::Text { fill_opacity, .. } => {
                if *fill_opacity < 1.0 {
                    let name = format!("SvgA{at}").into_bytes();
                    if builder.add_ext_gstate(
                        &name,
                        &ExtGState {
                            fill_alpha: Some(*fill_opacity),
                            ..ExtGState::default()
                        },
                    ) {
                        out.alpha.push((at, name));
                    }
                }
            }
            Node::Image { href, .. } => {
                let name = format!("SvgI{at}").into_bytes();
                match embed(builder, &name, href, placement.entry_limit, &mut resolve) {
                    Some(natural) => out.images.push((at, name, natural)),
                    None => out.images_unresolved += 1,
                }
            }
            _ => {}
        }
    }
    out
}

/// Registers a gradient as a `/Pattern`, or `None` for a paint that is not one.
fn pattern(
    builder: &mut DocumentBuilder,
    paint: &Paint,
    base: [f64; 6],
    name: &str,
) -> Option<Vec<u8>> {
    let shading = shading_of(paint)?;
    // 8.7.3.1: pattern space is the page's **default** coordinate system, so
    // the `cm` in force does not reach it and the page mapping has to be
    // composed in here. This is the one place the flip is written twice, and
    // it is the specification's rule rather than a shortcut.
    let matrix = match paint {
        Paint::Linear { matrix, .. } | Paint::Radial { matrix, .. } => {
            transform::concat(*matrix, base)
        }
        _ => base,
    };
    let name = name.as_bytes().to_vec();
    builder
        .add_shading_pattern(
            &name,
            &ShadingPattern {
                shading,
                matrix: Some(matrix),
            },
        )
        .then_some(name)
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

/// Draws a whole scene onto a page, naming what [`register`] already added.
pub fn draw(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    scene: &Scene,
    registry: &Registry,
    placement: Placement,
    fonts: &Fonts<'_>,
    metrics: &BookMetrics<'_>,
) -> Drawn {
    let mut out = Drawn {
        images_unresolved: registry.images_unresolved,
        ..Drawn::default()
    };
    let base = placement.matrix(scene.size);
    page.raw(
        format!(
            "q {} {} {} {} {} {} cm",
            base[0], base[1], base[2], base[3], base[4], base[5]
        )
        .as_bytes(),
    );

    // §10.9's chunks, resolved before anything is drawn: `text-anchor` needs a
    // whole chunk's width, and a width needs the metrics — so the pen is
    // walked once here and the drawing pass reads the answer.
    let placed = place_text(scene, metrics);

    for (index, node) in scene.nodes.iter().enumerate() {
        match node {
            Node::Path {
                outline,
                fill,
                rule,
                stroke,
                clip,
                ..
            } => {
                page.raw(b"q");
                if let Some(name) = Registry::named(&registry.alpha, index) {
                    page.set_ext_gstate(name);
                }
                if let Some(clip) = clip {
                    apply_clip(page, clip);
                }
                let filled = set_paint(
                    page,
                    fill,
                    Registry::named(&registry.fill_pattern, index),
                    false,
                );
                let stroked = stroke.as_ref().is_some_and(|stroke| {
                    let painted = set_paint(
                        page,
                        &stroke.paint,
                        Registry::named(&registry.stroke_pattern, index),
                        true,
                    );
                    if painted {
                        set_stroke_state(page, stroke);
                    }
                    painted
                });
                if filled || stroked {
                    write_outline(page, outline);
                    page.raw(operator(filled, stroked, *rule));
                } else if clip.is_some() {
                    // A shape that paints nothing still had a clip pushed, and
                    // `Q` below is what pops it.
                    out.refused += usize::from(!matches!(fill, Paint::None));
                }
                page.raw(b"Q");
            }
            Node::Image {
                rect,
                matrix,
                preserve,
                ..
            } => {
                if let Some((_, name, natural)) =
                    registry.images.iter().find(|(at, ..)| *at == index)
                {
                    place_image(page, name, *rect, *matrix, preserve.as_deref(), *natural);
                    out.images_drawn += 1;
                }
            }
            Node::Text { .. } => {
                let Some(run) = placed.get(index).copied().flatten() else {
                    continue;
                };
                out.refused += draw_text(
                    builder,
                    page,
                    node,
                    run,
                    Registry::named(&registry.alpha, index),
                    fonts,
                    metrics,
                );
            }
            // `Node` is `#[non_exhaustive]`: a variant added to the leaf crate
            // reaches here as ink nobody drew, so it is counted rather than
            // skipped.
            _ => out.refused += 1,
        }
    }
    page.raw(b"Q");
    out
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
fn write_outline(page: &mut PageBuilder, outline: &tinker_pdf_svg::path::Outline) {
    let mut stream = String::new();
    for segment in &outline.segments {
        match *segment {
            Segment::Move(p) => stream.push_str(&format!("{} {} m\n", p[0], p[1])),
            Segment::Line(p) => stream.push_str(&format!("{} {} l\n", p[0], p[1])),
            Segment::Cubic(a, b, c) => stream.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                a[0], a[1], b[0], b[1], c[0], c[1]
            )),
            Segment::Close => stream.push_str("h\n"),
            _ => {}
        }
    }
    page.raw(stream.as_bytes());
}

/// §14.3's clip, as 8.5.4's `W`/`W*` followed by `n`.
///
/// An **empty** clip path clips everything away, and it is written as a
/// degenerate rectangle rather than skipped: §14.3.5 says so, and a build that
/// wrote nothing would draw the element unclipped — which is the opposite
/// answer and looks like a document that has no clip in it.
fn apply_clip(page: &mut PageBuilder, clip: &Clip) {
    if clip.outline.segments.is_empty() {
        page.raw(b"0 0 0 0 re W n");
        return;
    }
    write_outline(page, &clip.outline);
    page.raw(match clip.rule {
        FillRule::NonZero => b"W n".as_slice(),
        FillRule::EvenOdd => b"W* n".as_slice(),
    });
}

/// Sets a fill or stroke colour, returning whether anything will paint.
///
/// A gradient's pattern name comes from [`register`] rather than being added
/// here: see [`Registry`]'s own note.
fn set_paint(
    page: &mut PageBuilder,
    paint: &Paint,
    pattern: Option<&[u8]>,
    stroking: bool,
) -> bool {
    match paint {
        Paint::None => false,
        Paint::Solid(colour) => {
            let [r, g, b] = colour.rgb;
            if stroking {
                page.set_stroke_rgb(r, g, b)
            } else {
                page.set_fill_rgb(r, g, b)
            }
        }
        Paint::Linear { .. } | Paint::Radial { .. } => match pattern {
            // A gradient whose pattern the builder refused paints **nothing**
            // rather than falling back to a colour. §13.2's fallback is for a
            // paint server the *document* did not supply; inventing one here
            // would put a flat colour where a reader would see a ramp, and
            // nothing would say the ramp had been lost.
            None => false,
            Some(name) => {
                if stroking {
                    page.set_stroke_pattern(name)
                } else {
                    page.set_fill_pattern(name)
                }
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
fn set_stroke_state(page: &mut PageBuilder, stroke: &tinker_pdf_svg::Stroke) {
    let mut out = format!("{} w {} M", stroke.width, stroke.miter_limit.max(1.0));
    out.push_str(match stroke.cap {
        LineCap::Butt => " 0 J",
        LineCap::Round => " 1 J",
        LineCap::Square => " 2 J",
    });
    out.push_str(match stroke.join {
        LineJoin::Miter => " 0 j",
        LineJoin::Round => " 1 j",
        LineJoin::Bevel => " 2 j",
    });
    let dashes: Vec<String> = stroke.dashes.iter().map(f64::to_string).collect();
    out.push_str(&format!(" [{}] {} d", dashes.join(" "), stroke.dash_offset));
    page.raw(out.as_bytes());
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
    page: &mut PageBuilder,
    name: &[u8],
    rect: [f64; 4],
    matrix: [f64; 6],
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
    let placed = transform::concat(placed, matrix);
    page.raw(
        format!(
            "q {} {} {} {} {} {} cm",
            placed[0], placed[1], placed[2], placed[3], placed[4], placed[5]
        )
        .as_bytes(),
    );
    page.image(name, 0.0, 0.0, 1.0, 1.0);
    page.raw(b"Q");
}

// ---- §10's text ----------------------------------------------------------------

/// Where one run's baseline starts, in scene coordinates.
type Origin = [f64; 2];

/// Resolves §10.9's chunks into one origin per text node.
///
/// **Two passes, and the first one is why this is not done inline.** A chunk's
/// `text-anchor` cannot be applied until the whole chunk's width is known, and
/// a run that states no position of its own begins where the one before it
/// ended — so both need a measurement, and a measurement needs the same
/// metrics the book was paginated with. Doing it here means SVG text and the
/// book's own prose are measured by one `Metrics`.
fn place_text(scene: &Scene, metrics: &BookMetrics<'_>) -> Vec<Option<Origin>> {
    let mut out: Vec<Option<Origin>> = vec![None; scene.nodes.len()];
    let mut chunk: Vec<(usize, f64)> = Vec::new();
    let mut pen = [0.0f64, 0.0];
    let mut anchor_kind = TextAnchor::Start;

    let flush = |chunk: &mut Vec<(usize, f64)>, out: &mut Vec<Option<Origin>>, kind| {
        let total: f64 = chunk.iter().map(|(_, width)| *width).sum();
        // §10.9's shift, applied to the whole chunk rather than to each run:
        // `middle` centres what the chunk holds, and a build that centred each
        // run would pile them on top of one another.
        let shift = match kind {
            TextAnchor::Start => 0.0,
            TextAnchor::Middle => -total / 2.0,
            TextAnchor::End => -total,
        };
        // **The shift alone.** Each run's origin already carries the pen at
        // the moment it was reached, so adding a running offset here as well
        // would advance every run twice -- which puts the second word of a
        // chunk two words along and is invisible in anything but a number.
        for (index, _) in chunk.drain(..) {
            if let Some(origin) = out[index].as_mut() {
                origin[0] += shift;
            }
        }
    };

    for (index, node) in scene.nodes.iter().enumerate() {
        let Node::Text {
            text, anchor, font, ..
        } = node
        else {
            continue;
        };
        let families = families_of(&font.families);
        let width = metrics.measure(text, &request_of(font, &families));
        if let Some(start) = anchor {
            flush(&mut chunk, &mut out, anchor_kind);
            pen = *start;
            anchor_kind = font.anchor;
        }
        out[index] = Some(pen);
        chunk.push((index, width));
        pen[0] += width;
    }
    flush(&mut chunk, &mut out, anchor_kind);
    out
}

/// One text run, as a text object under the run's own matrix.
///
/// Returns how many pieces the writer refused, which the caller turns into
/// [`crate::ArchiveWarning::UnwritableTextRun`] (ruling 10).
#[allow(clippy::too_many_arguments)]
fn draw_text(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    node: &Node,
    origin: Origin,
    alpha: Option<&[u8]>,
    fonts: &Fonts<'_>,
    metrics: &BookMetrics<'_>,
) -> usize {
    let Node::Text {
        text,
        matrix,
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
    let local = transform::concat([1.0, 0.0, 0.0, -1.0, origin[0], origin[1]], *matrix);

    page.raw(b"q");
    if let Some(name) = alpha {
        page.set_ext_gstate(name);
    }
    if let Paint::Solid(colour) = fill {
        page.set_fill_rgb(colour.rgb[0], colour.rgb[1], colour.rgb[2]);
    }
    page.raw(
        format!(
            "{} {} {} {} {} {} cm",
            local[0], local[1], local[2], local[3], local[4], local[5]
        )
        .as_bytes(),
    );

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
                    page.raw(&bytes);
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
                pen += draw_coded(page, fonts, metrics, &request, chosen, slice, pen);
            }
        }
    }
    page.raw(b"Q");
    refused
}

/// A slice in one of the standard 14, as bytes in a simple font.
///
/// Returns the slice's advance. Consecutive characters that resolve to the
/// same resource are written as one string: a character outside
/// `WinAnsiEncoding` lands in an overflow font with a resource of its own, and
/// splitting only where the resource changes is what keeps a word one text
/// object wherever it can be.
#[allow(clippy::too_many_arguments)]
fn draw_coded(
    page: &mut PageBuilder,
    fonts: &Fonts<'_>,
    metrics: &BookMetrics<'_>,
    request: &FontRequest<'_>,
    chosen: Chosen,
    text: &str,
    start: f64,
) -> f64 {
    let size = request.size;
    let mut pen = 0.0f64;
    let mut current: Option<(Vec<u8>, Vec<u8>, String)> = None;
    let mut at = 0.0f64;
    let flush = |page: &mut PageBuilder, held: Option<(Vec<u8>, Vec<u8>, String)>, x: f64| {
        if let Some((resource, codes, characters)) = held {
            page.encoded_text(
                &resource,
                size,
                start + x,
                0.0,
                (0.0, 0.0),
                &codes,
                &characters,
            );
        }
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
            Some((resource, codes, characters)) if resource == coded.resource() => {
                if let Coded::Simple { code, .. } = coded {
                    codes.push(code);
                    characters.push(ch);
                }
            }
            _ => {
                flush(page, current.take(), at);
                at = pen;
                if let Coded::Simple { resource, code } = coded {
                    current = Some((resource, vec![code], ch.to_string()));
                }
            }
        }
        pen += advance;
    }
    flush(page, current.take(), at);
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
    for node in &scene.nodes {
        let Node::Text { text, font, .. } = node else {
            continue;
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
