//! Test-only shadows of the three ways `docs/design/image-edges.md` (RC-21)
//! gives of answering the run-membership question in the page's frame — the
//! run's coverage held over the frame, the members outside the canvas
//! recomputed over the band a draw meets, and a rule from the page-frame
//! geometry alone — each beside the canvas's own answer
//! (`Renderer::run_would_overlap`), and the probe that prices them for H-15.
//!
//! A shadow keeps its own run as the page would hold it: ended where its own
//! answer finds an overlap, and wherever the real run ends for anything else
//! — a change of state, a draw made directly, any call that flushes. It
//! answers every question the canvas is asked and counts what answering
//! costs: the pixels it reads, the members it scans, the bytes it holds
//! beyond today's run, and the answers it gives otherwise than the canvas. It
//! decides nothing the render uses, so no pin moves while one is on.

use tinker_pdf_raster::blend::BlendMode;
use tinker_pdf_raster::canvas::Color;
use tinker_pdf_raster::fill::{FillScratch, HardEdge};
use tinker_pdf_raster::fragments::Fragments;
use tinker_pdf_raster::image::{
    accumulate_image, image_coverage, ImageDraw, ImageSource, Pyramid, SourceWindow, Transform,
    TwoColour,
};

use super::{Bounds, DecodedImage};

/// `Fragments::would_overlap`'s allowance for the rounding of an exact
/// abutment, in levels of one pixel's coverage. Mechanism C states it as an
/// area: two squares overlap when they share more than `SLACK / 255` of a
/// pixel.
const SLACK: f64 = 24.0;

/// One of the three mechanisms RC-21.1 names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mechanism {
    /// A: the run's coverage held over the frame, each member accumulated
    /// over all of its bounds within the frame.
    WholeFrame = 0,
    /// B: the members outside the canvas resampled over the part of the band
    /// outside it.
    Recompute = 1,
    /// C: the members' squares met with the draw's, in the page's frame.
    Geometry = 2,
}

impl Mechanism {
    /// All three, in RC-21.1's order.
    pub(crate) const ALL: [Mechanism; 3] = [
        Mechanism::WholeFrame,
        Mechanism::Recompute,
        Mechanism::Geometry,
    ];

    /// The name the price table gives it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Mechanism::WholeFrame => "whole frame",
            Mechanism::Recompute => "recomputed band",
            Mechanism::Geometry => "geometry",
        }
    }
}

/// What a mechanism, or the canvas, did over one render.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// Questions answered.
    pub(crate) questions: u64,
    /// Pixels read answering them: each question's band once, and what the
    /// mechanism resamples or accumulates outside the canvas.
    pub(crate) pixels_read: u64,
    /// Run members scanned answering them.
    pub(crate) members_scanned: u64,
    /// The most held at once: today's run's fragments for the canvas, and
    /// what the mechanism keeps beyond them for the others.
    pub(crate) bytes_held: u64,
    /// Answers other than the canvas's.
    pub(crate) answers_changed: u64,
}

/// A run member as mechanism B keeps it: what recomputes its coverage at a
/// pixel. The shadow keeps its colour too, so as to recompute through the
/// one walk the run accumulates through; the mechanism needs only what
/// carries alpha, which is what [`Member::bytes`] counts.
struct Member {
    unit_to_device: Transform,
    cut: Option<Transform>,
    interpolate: bool,
    tint: Option<Color>,
    edge_bits: u8,
    edge_rule: HardEdge,
    /// Its bounds within the frame.
    framed: Bounds,
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    alpha: Vec<u8>,
    two_colour: Option<(Vec<u8>, [[u8; 4]; 2])>,
    window: Option<SourceWindow>,
}

impl Member {
    /// The member `draw` makes of `image`.
    fn of(draw: &ImageDraw<'_>, image: &DecodedImage, framed: Bounds) -> Member {
        Member {
            unit_to_device: draw.unit_to_device,
            cut: draw.cut,
            interpolate: draw.interpolate,
            tint: draw.tint,
            edge_bits: draw.edge_bits,
            edge_rule: draw.edge_rule,
            framed,
            width: image.width,
            height: image.height,
            rgb: image.rgb.clone(),
            alpha: image.alpha.clone(),
            two_colour: image
                .two_colour
                .as_ref()
                .map(|two| (two.samples.clone(), two.colours)),
            window: image.window,
        }
    }

    /// What mechanism B holds for this member: its two transforms and its
    /// rectangle, and the samples that carry alpha — a soft mask's, a colour
    /// key's, a stencil's or a two-colour image's, and none for an opaque
    /// image, whose coverage is its square's.
    fn bytes(&self) -> u64 {
        let carried = match &self.two_colour {
            Some((samples, _)) => samples.len(),
            None => self.alpha.len(),
        };
        2 * 48 + 32 + carried as u64
    }

    /// The draw that accumulates this member, as the run accumulated it.
    fn draw(&self) -> ImageDraw<'_> {
        ImageDraw {
            image: ImageSource {
                width: self.width,
                height: self.height,
                rgb: &self.rgb,
                alpha: &self.alpha,
                two_colour: self
                    .two_colour
                    .as_ref()
                    .map(|(samples, colours)| TwoColour {
                        samples,
                        colours: *colours,
                    }),
                window: self.window,
            },
            unit_to_device: self.unit_to_device,
            cut: self.cut,
            interpolate: self.interpolate,
            alpha: 1.0,
            blend: BlendMode::Normal,
            clip: None,
            tint: self.tint,
            ink: None,
            stop: None,
            edge_bits: self.edge_bits,
            edge_rule: self.edge_rule,
        }
    }
}

/// Mechanism A's run: fragments the frame's size, and the hull of what its
/// members put coverage in.
struct WholeRun {
    fragments: Fragments,
    covered: Bounds,
}

/// The shadows a renderer runs, and what each and the canvas counted.
pub(crate) struct Shadows {
    on: [bool; 3],
    whole: Option<WholeRun>,
    members: Vec<Member>,
    quads: Vec<[(f64, f64); 4]>,
    tallies: [Tally; 3],
    canvas: Tally,
    answers: [Vec<bool>; 3],
    canvas_answers: Vec<bool>,
}

impl Shadows {
    /// The shadows of `on`, each with an empty run.
    pub(crate) fn new(on: &[Mechanism]) -> Shadows {
        let mut switched = [false; 3];
        for mechanism in on {
            switched[*mechanism as usize] = true;
        }
        Shadows {
            on: switched,
            whole: None,
            members: Vec::new(),
            quads: Vec::new(),
            tallies: [Tally::default(); 3],
            canvas: Tally::default(),
            answers: [Vec::new(), Vec::new(), Vec::new()],
            canvas_answers: Vec::new(),
        }
    }

    /// Ends every shadow's run: the real run ended for something other than
    /// an overlap, which ends the page's too.
    pub(crate) fn end_runs(&mut self) {
        self.whole = None;
        self.members.clear();
        self.quads.clear();
    }

    /// Asks each shadow whether a draw through `t`, whose bounds within the
    /// frame are `framed`, overlaps its run, beside the canvas's `answer`,
    /// which read `band` pixels; `canvas` is the canvas's rectangle. A shadow
    /// that finds an overlap ends its run, as the page would.
    pub(crate) fn question(
        &mut self,
        t: &Transform,
        framed: Option<Bounds>,
        canvas: Bounds,
        answer: bool,
        band: u64,
        scratch: &mut FillScratch,
    ) {
        self.canvas.questions += 1;
        self.canvas.pixels_read += band;
        self.canvas_answers.push(answer);
        for mechanism in Mechanism::ALL {
            let slot = mechanism as usize;
            if !self.on[slot] {
                continue;
            }
            let own = match mechanism {
                Mechanism::WholeFrame => self.whole_answer(t, framed, scratch),
                Mechanism::Recompute => self.recomputed_answer(t, framed, canvas, scratch),
                Mechanism::Geometry => self.geometric_answer(t),
            };
            let tally = &mut self.tallies[slot];
            tally.questions += 1;
            if own != answer {
                tally.answers_changed += 1;
            }
            self.answers[slot].push(own);
            if own {
                match mechanism {
                    Mechanism::WholeFrame => self.whole = None,
                    Mechanism::Recompute => self.members.clear(),
                    Mechanism::Geometry => self.quads.clear(),
                }
            }
        }
    }

    /// Mechanism A: the draw's square over its band with the run's coverage
    /// on the frame, read from fragments the frame's size.
    fn whole_answer(
        &mut self,
        t: &Transform,
        framed: Option<Bounds>,
        scratch: &mut FillScratch,
    ) -> bool {
        let (Some(run), Some(framed)) = (&self.whole, framed) else {
            return false;
        };
        let band = framed.meet(run.covered);
        if band.is_empty() {
            return false;
        }
        let (x0, y0, width, height) = band.rect();
        let shape = image_coverage(t, x0, y0, width, height, None, scratch);
        let overlaps = run.fragments.would_overlap(&shape);
        self.tallies[Mechanism::WholeFrame as usize].pixels_read += band.area();
        overlaps
    }

    /// Mechanism B: the members that meet the band accumulated over it — the
    /// part inside the canvas standing for the fragments the real mechanism
    /// reads there, and only the part outside it counted as resampled — and
    /// the draw's square read against them.
    fn recomputed_answer(
        &mut self,
        t: &Transform,
        framed: Option<Bounds>,
        canvas: Bounds,
        scratch: &mut FillScratch,
    ) -> bool {
        let Some(framed) = framed else {
            return false;
        };
        if self.members.is_empty() {
            return false;
        }
        let tally = &mut self.tallies[Mechanism::Recompute as usize];
        tally.members_scanned += self.members.len() as u64;
        let hull = self
            .members
            .iter()
            .fold(Bounds::NONE, |hull, member| hull.hull(member.framed));
        let band = framed.meet(hull);
        if band.is_empty() {
            return false;
        }
        let (x0, y0, width, height) = band.rect();
        let mut fragments = Fragments::new(x0, y0, width, height);
        let inside = band.meet(canvas);
        let outside = outside_of(band, canvas);
        for member in &self.members {
            let draw = member.draw();
            if !inside.meet(member.framed).is_empty() {
                accumulate_image(
                    &mut fragments,
                    &draw,
                    &mut Pyramid::new(),
                    inside.rect(),
                    scratch,
                );
            }
            for part in &outside {
                let met = part.meet(member.framed);
                if met.is_empty() {
                    continue;
                }
                accumulate_image(
                    &mut fragments,
                    &draw,
                    &mut Pyramid::new(),
                    part.rect(),
                    scratch,
                );
                tally.pixels_read += met.area();
            }
        }
        tally.pixels_read += band.area();
        let shape = image_coverage(t, x0, y0, width, height, None, scratch);
        fragments.would_overlap(&shape)
    }

    /// Mechanism C: whether the draw's square shares more than `SLACK / 255`
    /// of a pixel with some member's.
    fn geometric_answer(&mut self, t: &Transform) -> bool {
        let quad = corners(t);
        self.tallies[Mechanism::Geometry as usize].members_scanned += self.quads.len() as u64;
        self.quads
            .iter()
            .any(|member| shared_area(member, &quad) > SLACK / 255.0)
    }

    /// Adds a draw that joined the real run to each shadow's run: `draw` as
    /// the run accumulated it, of `image`, its bounds within the frame
    /// `framed`, the frame `frame` and the canvas `canvas`.
    pub(crate) fn join(
        &mut self,
        draw: &ImageDraw<'_>,
        image: &DecodedImage,
        framed: Option<Bounds>,
        frame: Bounds,
        canvas: Bounds,
        scratch: &mut FillScratch,
    ) {
        self.canvas.bytes_held = self.canvas.bytes_held.max(canvas.area().saturating_mul(4));
        let framed = framed.unwrap_or(Bounds::NONE);
        if self.on[Mechanism::WholeFrame as usize] {
            let run = self.whole.get_or_insert_with(|| {
                let (x0, y0, width, height) = frame.rect();
                WholeRun {
                    fragments: Fragments::new(x0, y0, width, height),
                    covered: Bounds::NONE,
                }
            });
            accumulate_image(
                &mut run.fragments,
                draw,
                &mut Pyramid::new(),
                frame.rect(),
                scratch,
            );
            run.covered = run.covered.hull(framed);
            let tally = &mut self.tallies[Mechanism::WholeFrame as usize];
            tally.pixels_read += framed.area() - framed.meet(canvas).area();
            tally.bytes_held = tally.bytes_held.max(frame.area().saturating_mul(4));
        }
        if self.on[Mechanism::Recompute as usize] && !framed.is_empty() {
            self.members.push(Member::of(draw, image, framed));
            let held: u64 = self.members.iter().map(Member::bytes).sum();
            let tally = &mut self.tallies[Mechanism::Recompute as usize];
            tally.bytes_held = tally.bytes_held.max(held);
        }
        if self.on[Mechanism::Geometry as usize] {
            self.quads
                .push(corners(draw.cut.as_ref().unwrap_or(&draw.unit_to_device)));
            let tally = &mut self.tallies[Mechanism::Geometry as usize];
            tally.bytes_held = tally.bytes_held.max(self.quads.len() as u64 * 64);
        }
    }

    /// What `mechanism` counted.
    pub(crate) fn tally(&self, mechanism: Mechanism) -> Tally {
        self.tallies[mechanism as usize]
    }

    /// What the canvas counted.
    pub(crate) fn canvas_tally(&self) -> Tally {
        self.canvas
    }

    /// `mechanism`'s answers, question by question.
    pub(crate) fn answers(&self, mechanism: Mechanism) -> &[bool] {
        &self.answers[mechanism as usize]
    }

    /// The canvas's answers, question by question.
    pub(crate) fn canvas_answers(&self) -> &[bool] {
        &self.canvas_answers
    }
}

/// `band` less `canvas`, as at most four rectangles: the rows above and
/// below the canvas across the band, and the columns beside it in the
/// canvas's rows.
fn outside_of(band: Bounds, canvas: Bounds) -> Vec<Bounds> {
    let inner = band.meet(canvas);
    if inner.is_empty() {
        return vec![band];
    }
    let mut parts = vec![
        Bounds {
            left: band.left,
            top: band.top,
            right: band.right,
            bottom: inner.top,
        },
        Bounds {
            left: band.left,
            top: inner.bottom,
            right: band.right,
            bottom: band.bottom,
        },
        Bounds {
            left: band.left,
            top: inner.top,
            right: inner.left,
            bottom: inner.bottom,
        },
        Bounds {
            left: inner.right,
            top: inner.top,
            right: band.right,
            bottom: inner.bottom,
        },
    ];
    parts.retain(|part| !part.is_empty());
    parts
}

/// A unit square's four device corners through `t`, snapped to the 1/256
/// grid its coverage is filled on (`image_coverage`), in the coverage's
/// order.
fn corners(t: &Transform) -> [(f64, f64); 4] {
    let snap = |value: f64| (value * 256.0).round() / 256.0;
    [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(x, y)| {
        let (x, y) = t.apply(x, y);
        (snap(x), snap(y))
    })
}

/// Twice the signed area of a polygon, positive when its corners run
/// counter-clockwise in the plane's own sense.
fn twice_signed_area(polygon: &[(f64, f64)]) -> f64 {
    let mut sum = 0.0;
    for (index, &(x0, y0)) in polygon.iter().enumerate() {
        let (x1, y1) = polygon[(index + 1) % polygon.len()];
        sum += x0 * y1 - x1 * y0;
    }
    sum
}

/// Which side of the line from `p` to `q` the point `v` is on: positive to
/// its left.
fn side(p: (f64, f64), q: (f64, f64), v: (f64, f64)) -> f64 {
    (q.0 - p.0) * (v.1 - p.1) - (q.1 - p.1) * (v.0 - p.0)
}

/// The area two convex quadrilaterals share, in square device pixels: the
/// first clipped by each edge of the second (Sutherland and Hodgman), and the
/// shoelace over what is left. Either may run either way round; one with a
/// corner that is not finite shares nothing.
pub(crate) fn shared_area(a: &[(f64, f64); 4], b: &[(f64, f64); 4]) -> f64 {
    if a.iter()
        .chain(b.iter())
        .any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return 0.0;
    }
    let oriented = |quad: &[(f64, f64); 4]| {
        let mut quad = *quad;
        if twice_signed_area(&quad) < 0.0 {
            quad.reverse();
        }
        quad
    };
    let clipper = oriented(b);
    let mut polygon: Vec<(f64, f64)> = oriented(a).to_vec();
    for index in 0..4 {
        if polygon.is_empty() {
            return 0.0;
        }
        let (p, q) = (clipper[index], clipper[(index + 1) % 4]);
        let mut kept = Vec::with_capacity(polygon.len() + 1);
        for (at, &current) in polygon.iter().enumerate() {
            let previous = polygon[(at + polygon.len() - 1) % polygon.len()];
            let (now, before) = (side(p, q, current), side(p, q, previous));
            if now >= 0.0 {
                if before < 0.0 {
                    kept.push(crossing(previous, current, before, now));
                }
                kept.push(current);
            } else if before >= 0.0 {
                kept.push(crossing(previous, current, before, now));
            }
        }
        polygon = kept;
    }
    if polygon.len() < 3 {
        return 0.0;
    }
    (twice_signed_area(&polygon) / 2.0).abs()
}

/// Where the segment from `a` to `b` crosses a clipping line it meets, given
/// each end's [`side`] of it, which differ in sign.
fn crossing(a: (f64, f64), b: (f64, f64), at_a: f64, at_b: f64) -> (f64, f64) {
    let t = at_a / (at_a - at_b);
    (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, OnceLock};
    use std::time::Instant;

    use tinker_pdf_content::{
        interpret, ColorFamily, FontSource, Form, Group, GroupSpace, MaskGroup, Matrix,
        RenderingIntent, SoftMask, StateParameters,
    };
    use tinker_pdf_font::Outline;
    use tinker_pdf_raster::canvas::{Canvas, Color, PixelFormat};

    use super::{shared_area, Mechanism, Shadows, Tally};
    use crate::{
        page_pixels, page_view_transform, region_canvas_in, DecodedImage, GlyphSource,
        ImageRequest, PixelRegion, Renderer, TwoColourSamples,
    };

    /// The strips of [`strips`], top to bottom.
    const STRIPS: u32 = 41;

    /// An opaque RGB image of `rgb`.
    fn opaque(rgb: Vec<u8>, width: u32, height: u32) -> DecodedImage {
        DecodedImage {
            width,
            height,
            rgb,
            alpha: Vec::new(),
            two_colour: None,
            stencil: false,
            interpolate: false,
            reduced: None,
            family: ColorFamily::Rgb,
            soft_masked: false,
            window: None,
        }
    }

    /// A one-sample-wide stencil whose rows are `samples`, as a one-bit
    /// `/ImageMask` decodes: a sample of zero paints the fill colour, any
    /// other leaves the page alone (8.9.6.2).
    fn stencil(samples: Vec<u8>) -> DecodedImage {
        DecodedImage {
            height: samples.len() as u32,
            rgb: Vec::new(),
            two_colour: Some(TwoColourSamples {
                samples,
                colours: [[0, 0, 0, 255], [0, 0, 0, 0]],
            }),
            stencil: true,
            family: ColorFamily::Other,
            ..opaque(Vec::new(), 1, 1)
        }
    }

    /// Strip `k` of the 300-dpi page: 1200 by 44 samples of a pattern with
    /// no two strips alike, built once a process.
    fn strip(k: u32) -> Option<DecodedImage> {
        static BUILT: OnceLock<Vec<DecodedImage>> = OnceLock::new();
        let built = BUILT.get_or_init(|| {
            (0..STRIPS)
                .map(|k| {
                    let (width, height) = (1200u32, 44u32);
                    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
                    for y in 0..height {
                        for x in 0..width {
                            rgb.push(((x * 7 + k * 31) % 256) as u8);
                            rgb.push(((y * 13 + x / 3) % 256) as u8);
                            rgb.push(((k * 59 + y * 3 + x) % 256) as u8);
                        }
                    }
                    opaque(rgb, width, height)
                })
                .collect()
        });
        built.get(k as usize).cloned()
    }

    /// The probe's images, by name: `I` black and `W` white, one sample
    /// each; `S` black under a soft mask of 0.8; `K` a one-sample stencil
    /// and `H` a two-sample one whose upper sample alone paints; `R0` to
    /// `R40` the strips.
    struct Samples;

    impl GlyphSource for Samples {
        fn outline(&self, _font_id: u64, _code: u32) -> Option<Outline> {
            None
        }

        fn image(
            &self,
            name: &[u8],
            _request: &ImageRequest,
            _intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            Ok(match name {
                b"I" => Some(opaque(vec![0, 0, 0], 1, 1)),
                b"W" => Some(opaque(vec![255, 255, 255], 1, 1)),
                b"S" => Some(DecodedImage {
                    alpha: vec![204],
                    soft_masked: true,
                    ..opaque(vec![0, 0, 0], 1, 1)
                }),
                b"K" => Some(stencil(vec![0])),
                b"H" => Some(stencil(vec![0, 1])),
                [b'R', digits @ ..] => std::str::from_utf8(digits)
                    .ok()
                    .and_then(|digits| digits.parse().ok())
                    .and_then(strip),
                _ => None,
            })
        }
    }

    /// The three draws of ruling 5's first page, by name: `a` and `c` abut
    /// along `x = 40.5` across the top forty rows, `c` overlaps `b` in rows
    /// 60 to 69.
    fn three(a: &str, b: &str, c: &str) -> String {
        format!(
            "q 40.5 0 0 40 0 60 cm /{a} Do Q q 40 0 0 40 40.5 0 cm /{b} Do Q \
             q 40 0 0 70 40.5 30 cm /{c} Do Q"
        )
    }

    /// The probe's forms and graphics states: `G` the three in a
    /// non-isolated transparency group over `[5 5 95 95]`, `A` the three as
    /// an annotation's appearance draws them, its box the page, and `GM` a
    /// luminosity soft mask whose group draws the three white.
    struct Resources;

    impl FontSource for Resources {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }

        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }

        fn form(&self, name: &[u8]) -> Option<Form> {
            let (bbox, group) = match name {
                b"G" => ([5.0, 5.0, 95.0, 95.0], Some(Group::default())),
                b"A" => ([0.0, 0.0, 100.0, 100.0], None),
                _ => return None,
            };
            Some(Form {
                content: three("I", "I", "I").into_bytes(),
                matrix: Matrix::IDENTITY,
                bbox: Some(bbox),
                group,
                stream: 0,
            })
        }

        fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
            (name == b"GM").then(|| {
                Arc::new(StateParameters {
                    soft_mask: Some(SoftMask::Group(Box::new(MaskGroup {
                        form: Form {
                            content: three("W", "W", "W").into_bytes(),
                            matrix: Matrix::IDENTITY,
                            bbox: Some([0.0, 0.0, 100.0, 100.0]),
                            group: Some(Group {
                                space: Some(GroupSpace::Gray),
                                ..Group::default()
                            }),
                            stream: 0,
                        },
                        luminosity: true,
                        backdrop: None,
                        transfer: None,
                    }))),
                    ..StateParameters::default()
                })
            })
        }
    }

    /// One page the probe draws.
    struct Page {
        name: &'static str,
        content: String,
        crop: (f64, f64, f64, f64),
        rotation: u16,
        scale: f64,
    }

    impl Page {
        /// Its rendered size, turned as it is displayed.
        fn size(&self) -> (u32, u32) {
            let (x0, y0, x1, y1) = self.crop;
            let (width, height) = (x1 - x0, y1 - y0);
            let (width, height) = if self.rotation % 180 == 90 {
                (height, width)
            } else {
                (width, height)
            };
            page_pixels(width, height, self.scale)
        }

        /// Draws the page over `region`, or whole, with the shadows of `on`
        /// switched on, or none: the canvas, the shadows, and the pixels the
        /// canvas's questions read (`Renderer::run_pixels`).
        fn draw(
            &self,
            region: Option<PixelRegion>,
            on: Option<&[Mechanism]>,
        ) -> (Canvas, Option<Box<Shadows>>, u64) {
            let (width, height) = self.size();
            let canvas = match region {
                Some(region) => region_canvas_in(region, PixelFormat::Rgb8),
                None => Canvas::new(width, height, PixelFormat::Rgb8, Color::WHITE),
            };
            let base = page_view_transform(self.crop, self.rotation, self.scale);
            let mut renderer = Renderer::new(canvas, base, &Samples).with_page_size(width, height);
            renderer.shadows = on.map(|on| Box::new(Shadows::new(on)));
            interpret(
                self.content.as_bytes(),
                Matrix::IDENTITY,
                &mut renderer,
                &Resources,
            );
            let shadows = renderer.shadows.take();
            let read = renderer.run_pixels;
            let (canvas, _) = renderer.finish();
            (canvas, shadows, read)
        }
    }

    /// The media box of the class's pages.
    const FULL: (f64, f64, f64, f64) = (0.0, 0.0, 100.0, 100.0);
    /// The crop box of its cropped ones, offset on both axes.
    const CROP: (f64, f64, f64, f64) = (7.0, 3.0, 93.0, 97.0);

    /// The class RC-21.1 names, drawn by this crate's interpreter from the
    /// content `render_regions.rs`'s pages draw: the annotation's appearance
    /// as the form it is, drawn after an empty content stream.
    fn class() -> Vec<Page> {
        let page = |name, content: String, crop, rotation| Page {
            name,
            content,
            crop,
            rotation,
            scale: 1.0,
        };
        let first = || three("I", "I", "I");
        vec![
            page("overlap outside the tile in y", first(), FULL, 0),
            page(
                "overlap outside the tile in x",
                "q 40 0 0 40.5 0 59.5 cm /I Do Q q 40 0 0 40 60 19.5 cm /I Do Q \
                 q 70 0 0 40 0 19.5 cm /I Do Q"
                    .into(),
                FULL,
                0,
            ),
            page(
                "a draw wholly outside the tile overlaps a member",
                "q 40.5 0 0 100 0 0 cm /I Do Q q 40 0 0 30 20 0 cm /I Do Q \
                 q 40 0 0 40 40.5 60 cm /I Do Q"
                    .into(),
                FULL,
                0,
            ),
            page("soft-masked images", three("S", "S", "S"), FULL, 0),
            page(
                "stencil images",
                format!("0.1 0.3 0.8 rg {}", three("K", "H", "K")),
                FULL,
                0,
            ),
            page("in a group's canvas", "/G Do".into(), FULL, 0),
            page(
                "in a soft mask's canvas",
                "q /GM gs 0.9 0.2 0.1 rg 0 0 100 100 re f Q".into(),
                FULL,
                0,
            ),
            page("turned a quarter", first(), FULL, 90),
            page("cropped", first(), CROP, 0),
            page("cropped and turned three quarters", first(), CROP, 270),
            page("in an annotation's appearance", "/A Do".into(), FULL, 0),
        ]
    }

    /// A stencil drawn over another's unpainted half, a third abutting the
    /// first's painted half at row 10: coverage finds no overlap and keeps
    /// one run, geometry finds one and parts the seam into two.
    fn open_overlap() -> Page {
        Page {
            name: "a stencil over another's unpainted half",
            content: "0.2 0.5 0.3 rg q 80 0 0 40 10 49.5 cm /H Do Q \
                      q 80 0 0 25 10 40 cm /K Do Q q 80 0 0 10.5 10 89.5 cm /K Do Q"
                .into(),
            crop: FULL,
            rotation: 0,
            scale: 1.0,
        }
    }

    /// The 300-dpi page of abutting strips: 4 by 6 inches, 1200 by 1800
    /// pixels, of [`STRIPS`] full-width strips from 1200 by 44 samples, each
    /// meeting the next at a fractional row.
    fn strips() -> Page {
        let height = 432.0 / f64::from(STRIPS);
        let content = (0..STRIPS)
            .map(|k| {
                let y = 432.0 * f64::from(STRIPS - 1 - k) / f64::from(STRIPS);
                format!("q 288 0 0 {height} 0 {y} cm /R{k} Do Q\n")
            })
            .collect();
        Page {
            name: "a 300-dpi page of abutting strips",
            content,
            crop: (0.0, 0.0, 288.0, 432.0),
            rotation: 0,
            scale: 300.0 / 72.0,
        }
    }

    /// How a page is cut into tiles.
    #[derive(Clone, Copy)]
    enum Lattice {
        /// The page drawn whole.
        Whole,
        /// Horizontal bands, the page's whole width, cut at
        /// `height * i / count`.
        Bands(u32),
        /// Squares of a side, the last row and column partial.
        Squares(u32),
    }

    impl Lattice {
        fn name(self) -> String {
            match self {
                Lattice::Whole => "whole".into(),
                Lattice::Bands(count) => format!("{count} bands"),
                Lattice::Squares(side) => format!("{side} pixels"),
            }
        }

        /// Its tiles over a page `width` by `height`, `None` the page whole.
        fn tiles(self, (width, height): (u32, u32)) -> Vec<Option<PixelRegion>> {
            match self {
                Lattice::Whole => vec![None],
                Lattice::Bands(count) => (0..count)
                    .map(|band| (height * band / count, height * (band + 1) / count))
                    .filter(|(top, bottom)| bottom > top)
                    .map(|(top, bottom)| Some(PixelRegion::new(0, top, width, bottom - top)))
                    .collect(),
                Lattice::Squares(side) => {
                    let mut tiles = Vec::new();
                    for top in (0..height).step_by(side as usize) {
                        for left in (0..width).step_by(side as usize) {
                            tiles.push(Some(PixelRegion::new(
                                left,
                                top,
                                side.min(width - left),
                                side.min(height - top),
                            )));
                        }
                    }
                    tiles
                }
            }
        }
    }

    /// Two squares share their overlap, abutting ones nothing, whichever way
    /// round either runs; a square inside another shares itself, and one a
    /// diamond holds shares itself too.
    #[test]
    fn two_quadrilaterals_share_their_overlap() {
        let square = |x: f64, y: f64, side: f64| {
            [(x, y), (x + side, y), (x + side, y + side), (x, y + side)]
        };
        let near = |got: f64, want: f64, what: &str| {
            assert!((got - want).abs() < 1e-9, "{what}: {got}, not {want}");
        };
        let page = square(0.0, 0.0, 10.0);
        near(
            shared_area(&page, &square(5.0, 5.0, 10.0)),
            25.0,
            "a corner",
        );
        near(
            shared_area(&page, &square(10.0, 0.0, 10.0)),
            0.0,
            "abutting",
        );
        near(shared_area(&page, &square(20.0, 3.0, 10.0)), 0.0, "apart");
        near(shared_area(&page, &square(2.0, 3.0, 4.0)), 16.0, "inside");
        near(shared_area(&square(2.0, 3.0, 4.0), &page), 16.0, "around");
        let mut reversed = square(5.0, 5.0, 10.0);
        reversed.reverse();
        near(shared_area(&page, &reversed), 25.0, "clockwise");
        near(shared_area(&reversed, &page), 25.0, "clockwise first");
        let diamond = [(5.0, -5.0), (15.0, 5.0), (5.0, 15.0), (-5.0, 5.0)];
        near(shared_area(&page, &diamond), 100.0, "inside a diamond");
        let flat = [(0.0, 5.0), (10.0, 5.0), (10.0, 5.0), (0.0, 5.0)];
        near(shared_area(&page, &flat), 0.0, "a square of no area");
        let lost = [(f64::NAN, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        near(
            shared_area(&page, &lost),
            0.0,
            "a corner that is not a number",
        );
        // A sliver: two strips a hundred pixels long overlapping by a
        // twentieth of one, a twentieth of a pixel's coverage in each pixel
        // and five pixels in all.
        let upper = [(0.0, 0.0), (100.0, 0.0), (100.0, 10.05), (0.0, 10.05)];
        let lower = [(0.0, 10.0), (100.0, 10.0), (100.0, 20.0), (0.0, 20.0)];
        near(shared_area(&upper, &lower), 5.0, "a sliver");
    }

    /// **A run reads only the band where a draw meets it** — today's counted
    /// reads (`Renderer::run_pixels`), pinned. Eight strips 50 pixels wide and
    /// 12.5 tall down a 50 by 100 page: each after the first meets the run in
    /// the row its top edge crosses, fifty pixels where that edge falls inside
    /// a row, at 12.5, 37.5, 62.5 and 87.5, and none where it falls between
    /// two, at 25, 50 and 75. A tile reads the band cut at its canvas, and a
    /// strip with no rectangle on it reads nothing. A picture laid over the
    /// strips reads the rectangle it shares with the run, its own 20 by 20.
    #[test]
    fn a_run_reads_only_the_band_where_a_draw_meets_it() {
        let strips: String = (0..8)
            .map(|k| {
                format!(
                    "q 50 0 0 12.5 0 {} cm /I Do Q ",
                    100.0 - 12.5 * f64::from(k + 1)
                )
            })
            .collect();
        let page = Page {
            name: "eight strips",
            content: strips.clone(),
            crop: (0.0, 0.0, 50.0, 100.0),
            rotation: 0,
            scale: 1.0,
        };
        assert_eq!(page.draw(None, None).2, 4 * 50, "{}, whole", page.name);
        assert_eq!(
            page.draw(Some(PixelRegion::new(0, 0, 25, 100)), None).2,
            4 * 25,
            "the left half: each row cut at the canvas"
        );
        assert_eq!(
            page.draw(Some(PixelRegion::new(0, 0, 50, 40)), None).2,
            2 * 50,
            "the top forty rows: the strips below them have no rectangle there"
        );
        let over = Page {
            content: format!("{strips}q 20 0 0 20 15 40 cm /I Do Q"),
            ..page
        };
        assert_eq!(
            over.draw(None, None).2,
            4 * 50 + 20 * 20,
            "the strips and a picture laid over them"
        );
    }

    /// The canvas's answers on `page` over `region`, or whole.
    fn canvas_answers(page: &Page, region: Option<PixelRegion>) -> Vec<bool> {
        let (_, shadows, read) = page.draw(region, Some(&[][..]));
        let shadows = shadows.expect("switched on");
        assert_eq!(
            shadows.canvas_tally().pixels_read,
            read,
            "{}: the shadows saw every band the canvas read",
            page.name
        );
        shadows.canvas_answers().to_vec()
    }

    /// **Every page of the class parts some tile from the page**: on the
    /// 37-pixel lattice at least one tile's canvas answers a question
    /// otherwise than the page does, which is what puts it in the class.
    #[test]
    fn every_page_of_the_class_parts_some_tile_from_the_page() {
        for page in class() {
            let whole = canvas_answers(&page, None);
            let parted = Lattice::Squares(37)
                .tiles(page.size())
                .into_iter()
                .any(|tile| canvas_answers(&page, tile) != whole);
            assert!(parted, "{}: every tile answers as the page does", page.name);
        }
    }

    /// **The shadows answer in the page's frame.** On every page of the
    /// class, the open-overlap page and the strips' four bands, in every tile
    /// of the 64-, 37- and 23-pixel lattices and of four bands: the whole
    /// frame's and the recomputed band's answers are the page's, question by
    /// question, which is what each mechanism is for, and geometry's are the
    /// answers it gives on the page, a rule the page and the tile share.
    #[test]
    fn the_shadows_answer_in_the_pages_frame() {
        let mut pages = class();
        pages.push(open_overlap());
        for page in &pages {
            let (_, whole, _) = page.draw(None, Some(Mechanism::ALL.as_slice()));
            let whole = whole.expect("switched on");
            let answers = whole.canvas_answers().to_vec();
            assert_eq!(
                whole.answers(Mechanism::WholeFrame),
                answers,
                "{}",
                page.name
            );
            assert_eq!(
                whole.answers(Mechanism::Recompute),
                answers,
                "{}",
                page.name
            );
            let geometric = whole.answers(Mechanism::Geometry).to_vec();
            for lattice in [
                Lattice::Squares(64),
                Lattice::Squares(37),
                Lattice::Squares(23),
                Lattice::Bands(4),
            ] {
                for tile in lattice.tiles(page.size()) {
                    let (_, shadows, _) = page.draw(tile, Some(Mechanism::ALL.as_slice()));
                    let shadows = shadows.expect("switched on");
                    let what = format!("{} over {tile:?}", page.name);
                    assert_eq!(shadows.answers(Mechanism::WholeFrame), answers, "{what}");
                    assert_eq!(shadows.answers(Mechanism::Recompute), answers, "{what}");
                    assert_eq!(shadows.answers(Mechanism::Geometry), geometric, "{what}");
                }
            }
        }
        let page = strips();
        let (_, whole, _) = page.draw(None, Some(Mechanism::ALL.as_slice()));
        let whole = whole.expect("switched on");
        assert!(
            whole.canvas_answers().iter().all(|overlap| !overlap),
            "abutting strips never overlap"
        );
        for tile in Lattice::Bands(4).tiles(page.size()) {
            let (_, shadows, _) = page.draw(tile, Some(Mechanism::ALL.as_slice()));
            let shadows = shadows.expect("switched on");
            for mechanism in Mechanism::ALL {
                assert_eq!(
                    shadows.answers(mechanism),
                    whole.canvas_answers(),
                    "{:?} over {tile:?}",
                    mechanism
                );
            }
        }
    }

    /// **Geometry moves a page coverage keeps**: on the open-overlap page the
    /// first stencil is asked about an empty run, the second lands where the
    /// first paints nothing, so the page's own answer is no and the whole
    /// frame and the recomputed band agree, while geometry finds the squares
    /// overlapping and ends the run, and the third abuts the first and
    /// overlaps nothing — a render geometry moves, its price.
    #[test]
    fn geometry_ends_a_run_coverage_keeps() {
        let page = open_overlap();
        let (_, shadows, _) = page.draw(None, Some(Mechanism::ALL.as_slice()));
        let shadows = shadows.expect("switched on");
        assert_eq!(shadows.canvas_answers(), [false, false, false]);
        assert_eq!(
            shadows.answers(Mechanism::WholeFrame),
            [false, false, false]
        );
        assert_eq!(shadows.answers(Mechanism::Recompute), [false, false, false]);
        assert_eq!(shadows.answers(Mechanism::Geometry), [false, true, false]);
        assert_eq!(shadows.tally(Mechanism::Geometry).answers_changed, 1);
        assert_eq!(shadows.tally(Mechanism::WholeFrame).answers_changed, 0);
    }

    /// **A shadow changes no pixel**: every page of the class and the
    /// open-overlap page, whole and over a tile, draws the same canvas with
    /// every shadow on as with none.
    #[test]
    fn the_shadows_change_no_pixel() {
        let mut pages = class();
        pages.push(open_overlap());
        for page in &pages {
            for region in [None, Some(PixelRegion::new(37, 0, 37, 37))] {
                let (plain, _, _) = page.draw(region, None);
                let (shadowed, _, _) = page.draw(region, Some(Mechanism::ALL.as_slice()));
                assert!(
                    plain.data == shadowed.data,
                    "{} over {region:?}: a shadow changed the canvas",
                    page.name
                );
            }
        }
    }

    /// The head of `crates/tinker-pdf-render/data/run_prices.tsv`, which
    /// [`price_of_deciding_runs_in_the_page_frame`] writes before its rows.
    const RUN_PRICES_HEAD: &str = "\
# The price of deciding an image run in the page's frame: each mechanism
# docs/design/image-edges.md (RC-21) gives, shadowed beside today's answer,
# written by price_of_deciding_runs_in_the_page_frame in
# crates/tinker-pdf-render/src/run_shadows.rs:
#
#   cargo test -p tinker-pdf-render --release -- --ignored price_of_deciding_runs_in_the_page_frame
#   cargo test -p tinker-pdf-render -- --ignored price_of_deciding_runs_in_the_page_frame
#
# A run rewrites its own build's rows and keeps the other build's. Twelve
# tab-separated fields, a row for each build, page, lattice and mechanism:
#
#   build            release or debug
#   page             the page, as the probe draws it
#   lattice          whole, 4 bands, or 64 pixels
#   tiles            the tiles the lattice cuts the page into
#   mechanism        canvas (today's), whole frame (A), recomputed band (B)
#                    or geometry (C)
#   questions        run-membership questions a tile asks, on average
#   pixels           pixels a tile reads answering them, on average: each
#                    question's band, and what the mechanism resamples or
#                    accumulates outside the canvas
#   members          run members a tile scans answering them, on average
#   bytes            the most one tile holds: today's run's fragments for the
#                    canvas, and what the mechanism keeps beyond them
#   answers_changed  questions answered otherwise than the canvas answers
#                    them, over the lattice
#   tiles_changed    tiles holding such an answer
#   time             the lattice drawn with the mechanism's shadow beside
#                    today's answer, over the lattice drawn with none, the
#                    least of three draws each, on one quiet machine
build\tpage\tlattice\ttiles\tmechanism\tquestions\tpixels\tmembers\tbytes\tanswers_changed\ttiles_changed\ttime
";

    /// The least of three timings of `draw`, in seconds.
    fn least_time(mut draw: impl FnMut()) -> f64 {
        (0..3)
            .map(|_| {
                let started = Instant::now();
                draw();
                started.elapsed().as_secs_f64()
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// One row of the table: `tally` summed over `tiles` tiles, `peak` the
    /// most one tile held, `parted` the tiles holding a changed answer.
    #[allow(clippy::too_many_arguments)]
    fn price_row(
        build: &str,
        page: &Page,
        lattice: Lattice,
        tiles: usize,
        mechanism: &str,
        tally: Tally,
        peak: u64,
        parted: u64,
        time: f64,
    ) -> String {
        let each = |total: u64| total as f64 / tiles as f64;
        format!(
            "{build}\t{}\t{}\t{tiles}\t{mechanism}\t{:.1}\t{:.1}\t{:.1}\t{peak}\t{}\t{parted}\t{time:.3}\n",
            page.name,
            lattice.name(),
            each(tally.questions),
            each(tally.pixels_read),
            each(tally.members_scanned),
            tally.answers_changed,
        )
    }

    /// **The price of deciding runs in the page's frame**, the table H-15 is
    /// decided from (`docs/design/image-edges.md`, RC-21).
    ///
    /// The class's eleven pages, the open-overlap page and the 300-dpi page
    /// of strips, each drawn whole, in four bands and on a 64-pixel lattice:
    /// once with every shadow on, for what each mechanism and the canvas read,
    /// scanned, held and answered otherwise, tile by tile; then timed with no
    /// shadow and with each alone. The rows are printed after a `RAN` banner
    /// and written to the committed table, this build's replacing its last.
    #[test]
    #[ignore = "prices three mechanisms over thirteen pages, the 300-dpi page's 64-pixel \
                lattice paying the page's image work in each of its tiles; rewrites \
                crates/tinker-pdf-render/data/run_prices.tsv"]
    fn price_of_deciding_runs_in_the_page_frame() {
        let build = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let mut pages = class();
        pages.push(open_overlap());
        pages.push(strips());
        let mut rows = String::new();
        for page in &pages {
            for lattice in [Lattice::Whole, Lattice::Bands(4), Lattice::Squares(64)] {
                let tiles = lattice.tiles(page.size());
                let mut canvas = Tally::default();
                let mut canvas_peak = 0;
                let mut totals = [Tally::default(); 3];
                let mut peaks = [0u64; 3];
                let mut parted = [0u64; 3];
                for tile in &tiles {
                    let (_, shadows, read) = page.draw(*tile, Some(Mechanism::ALL.as_slice()));
                    let shadows = shadows.expect("switched on");
                    let seen = shadows.canvas_tally();
                    assert_eq!(seen.pixels_read, read, "{}: every band seen", page.name);
                    canvas.questions += seen.questions;
                    canvas.pixels_read += seen.pixels_read;
                    canvas_peak = canvas_peak.max(seen.bytes_held);
                    for mechanism in Mechanism::ALL {
                        let slot = mechanism as usize;
                        let tally = shadows.tally(mechanism);
                        let total = &mut totals[slot];
                        total.questions += tally.questions;
                        total.pixels_read += tally.pixels_read;
                        total.members_scanned += tally.members_scanned;
                        total.answers_changed += tally.answers_changed;
                        peaks[slot] = peaks[slot].max(tally.bytes_held);
                        if tally.answers_changed > 0 {
                            parted[slot] += 1;
                        }
                    }
                }
                let today = least_time(|| {
                    for tile in &tiles {
                        let _ = page.draw(*tile, None);
                    }
                });
                rows.push_str(&price_row(
                    build,
                    page,
                    lattice,
                    tiles.len(),
                    "canvas",
                    canvas,
                    canvas_peak,
                    0,
                    1.0,
                ));
                for mechanism in Mechanism::ALL {
                    let slot = mechanism as usize;
                    let time = least_time(|| {
                        for tile in &tiles {
                            let _ = page.draw(*tile, Some([mechanism].as_slice()));
                        }
                    });
                    rows.push_str(&price_row(
                        build,
                        page,
                        lattice,
                        tiles.len(),
                        mechanism.name(),
                        totals[slot],
                        peaks[slot],
                        parted[slot],
                        time / today,
                    ));
                }
            }
        }
        println!(
            "run-prices: RAN, {} rows, {build} build",
            rows.lines().count()
        );
        print!("{rows}");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("run_prices.tsv");
        let other: String = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|line| {
                !line.is_empty()
                    && !line.starts_with('#')
                    && !line.starts_with("build\t")
                    && !line.starts_with(&format!("{build}\t"))
            })
            .map(|line| format!("{line}\n"))
            .collect();
        // Release rows first, whichever build ran last.
        let body = if build == "release" {
            format!("{rows}{other}")
        } else {
            format!("{other}{rows}")
        };
        std::fs::write(&path, format!("{RUN_PRICES_HEAD}{body}"))
            .unwrap_or_else(|error| panic!("writing {path:?}: {error}"));
    }
}
