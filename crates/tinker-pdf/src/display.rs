//! A retained page: [`Page::display_list`] interprets a page once, and
//! [`DisplayList::render`] draws it at any scale without interpreting it
//! again.
//!
//! # What is kept, and why that is enough
//!
//! The calls the interpreter made — every fill, stroke, clip, glyph, image,
//! shading, `q`, `Q`, form, group, soft mask and marked-content scope, each
//! with the graphics state it saw — recorded by `tinker-pdf-content`'s
//! recording device, in the space the interpreter runs in. The renderer holds
//! the transform to pixels, not the events, which is what makes one recording
//! drawable at any scale: nothing in it is a pixel.
//!
//! And the page's **resources**, kept alive beside it. A replay draws through
//! the same `Renderer` a direct render does, and that renderer resolves
//! glyph outlines, images, shadings, patterns and form scopes by name as it
//! meets them; the resource object it asks is the display list's own, so
//! what it decodes once — an image, an outline, a nested scope — is decoded
//! once for every render the list is asked for.
//!
//! # Why a replay is the direct render, byte for byte
//!
//! Three of the calls are questions the interpreter acts on (`begin_form`,
//! `begin_group`, `begin_soft_mask`), so a recording is only the renderer's
//! picture if it was made with the renderer's answers. It is:
//! `tinker_pdf_render::DisplayRecorder` asks the renderer's own `Admission`,
//! and the renderer's answers depend on nothing but the content stream — no
//! pixel, no scale and no rectangle — since ruling 5's September 2026 change.
//! Everything else is the same code: a replay goes through
//! [`Page::render_layer_with`], which is [`Page::render`]'s pipeline with the
//! interpretation swapped for [`tinker_pdf_content::replay`].
//! `determinism.rs`'s `a_display_list_replays_every_fingerprinted_page_byte_for_byte`
//! holds it over every fingerprinted page at the fingerprints' scale and three
//! others.
//!
//! Annotations are recorded too, each with the resource scope its appearance
//! resolves in, and replayed only when [`RenderOptions::annotations`] asks —
//! after the content, as `Page::render` draws them.
//!
//! # Why a replay's warnings are its own
//!
//! The resources a list keeps are also where a render's tolerances are
//! written down — a font no glyph resolved in, an image decoded with damage,
//! what a pattern cell met. A direct render builds its resources fresh, so its
//! warnings are what *it* met. Each replay is given resources of its own over
//! the list's caches (`PageResources::for_one_render`): a cached image or
//! outline brings back what its first decode reported, so a replay that meets
//! it says so as its own decode would, and a replay that does not meet it — a
//! region that misses a patterned fill, a render cancelled before anything was
//! drawn — says nothing about it. The interpretation itself ran once, when the
//! list was recorded, so what *it* could not resolve (a font name the resource
//! dictionary does not define), and the colour spaces it read with a repair, are
//! kept beside the events and reported by every replay that runs to the end. A cancelled replay reports none of it: a
//! cancelled direct render reports what its interpreter reached before it
//! stopped, which depends on when it stopped, so the replay's answer is the
//! one a render cancelled before the first such font would give — less, and
//! never more.
//!
//! # A page in bands
//!
//! With threads asked for, [`DisplayList::render`] draws the page in
//! horizontal bands, each a region of the page in its one frame (ruling 5) on
//! a thread of the engine's pool, into one bitmap
//! (`docs/design/parallelism.md`, "Bands of one page"). The cuts keep each
//! run of image draws — the one decision ruling 5 leaves to a canvas — inside
//! one band ([`DisplayList::image_runs`], [`cuts`]), and the bands' warnings,
//! each stamped with the ordinal of the call it was met at, merge into the
//! page's list ([`merged`]).
//!
//! # A page too large to retain
//!
//! A recording holds every call with its own state and path, so what it costs
//! is the interpreter's work, and work multiplies through forms: ten forms
//! each invoking the next four times are a million calls from two kilobytes.
//! So the recording has a budget, `tinker_pdf_render::MAX_DISPLAY_LIST_BYTES`
//! (64 MiB), and a page that passes it is **not retained**: the list keeps no
//! calls ([`DisplayList::is_retained`] says so), and every render interprets
//! the page again exactly as [`Page::render`] does — the same bitmap and the
//! same warnings, at a direct render's cost and in a direct render's memory.
//! [`DisplayList::to_svg`] interprets it into the SVG writer the same way. A
//! list that cannot be cheaper than the page is the page; it is never a
//! partial picture.

use std::sync::Arc;

use std::sync::{Mutex, PoisonError};

use tinker_pdf_content::{
    interpret, replay, Device, Event, Glyph, GraphicsState, Group, ImageRef, MarkedProps,
    MaskGroup, Matrix, PathSegment,
};
use tinker_pdf_cos::pages as cos_pages;
use tinker_pdf_pool::{on_worker, Executor, MAX_THREADS};
use tinker_pdf_raster::image::{image_bounds_in, Transform};
use tinker_pdf_render::{DisplayRecorder, Renderer};

use crate::annots::{self, Recorded};
use crate::resources::PageResources;
use crate::stats::Counter;
use crate::{
    handed_back, Bitmap, Met, Page, Parallelism, PixelRegion, RenderOptions, RenderWarning, View,
};

/// A page interpreted once, to be drawn as often and at as many scales as a
/// caller likes. See [`Page::display_list`].
///
/// Holds the page (which shares the document), the calls its content made,
/// its annotations' appearances, and the resources a replay resolves names
/// in — so it is as large as the page's drawing, not as large as a bitmap of
/// it, and it does not borrow the [`crate::Document`].
pub struct DisplayList {
    page: Page,
    resources: PageResources,
    content: Vec<Event>,
    annotations: Vec<Recorded>,
    /// Font names the recording's interpretation could not resolve — what a
    /// direct render's interpreter reports, met once rather than per render.
    interpreted_missing: Vec<String>,
    /// Colour spaces the recording's interpretation read with a repair — a
    /// `cs` resolves its space while the content is interpreted, which a
    /// replay does not do again.
    interpreted_repairs: Vec<(String, String)>,
    /// Whether the calls are kept: false when recording them passed
    /// `MAX_DISPLAY_LIST_BYTES`, and every render is then a direct one.
    retained: bool,
}

impl Page {
    /// Interprets the page once and keeps what it drew.
    ///
    /// The returned [`DisplayList`] renders with [`DisplayList::render`] to
    /// the same bitmap [`Page::render`] returns for the same options — the
    /// same pixels, the same size and the same warnings — at any scale,
    /// without tokenizing the content stream again. Worth it for a caller that
    /// draws one page more than once: a viewer zooming, a tiler asking for
    /// many regions, a thumbnail and a full view of the same page.
    ///
    /// The warnings are each render's own, not the list's history: see the
    /// module documentation, which also says the one place a cancelled
    /// replay's can be fewer than a cancelled direct render's.
    ///
    /// Annotations are recorded whatever a later render asks, and drawn only
    /// by a render that asks for them.
    ///
    /// A page whose recording would hold more than
    /// `tinker_pdf_render::MAX_DISPLAY_LIST_BYTES` is not retained: the list
    /// keeps nothing and every render of it is a direct one, which is the
    /// same bitmap ([`DisplayList::is_retained`]).
    #[must_use]
    pub fn display_list(&self) -> DisplayList {
        let content = cos_pages::content_bytes(&self.doc, &self.inner);
        // The recording is an operation of its own; the resources it reads
        // through are kept past it, and serve none afterwards.
        let resources = PageResources::new(&self.doc, &self.inner, self.fonts.as_ref());
        let mut recorder = DisplayRecorder::new();
        interpret(&content, Matrix::IDENTITY, &mut recorder, &resources);
        // Nothing has been drawn through these resources yet, so everything
        // they list is the interpretation's.
        let interpreted_missing = resources.missing_fonts();
        let interpreted_repairs = resources.repaired_spaces();
        let content = recorder.take();
        let annotations = if recorder.overflowed() {
            Vec::new()
        } else {
            annots::record(
                &self.doc,
                &self.inner,
                self.fonts.as_ref(),
                &mut recorder,
                resources.operation(),
            )
        };
        let resources = resources.kept();
        if recorder.overflowed() {
            // Everything recorded is dropped here, the content's calls too:
            // a list that kept part of a page would replay part of a page.
            return DisplayList {
                page: self.clone(),
                resources,
                content: Vec::new(),
                annotations: Vec::new(),
                interpreted_missing: Vec::new(),
                interpreted_repairs: Vec::new(),
                retained: false,
            };
        }
        DisplayList {
            page: self.clone(),
            resources,
            content,
            annotations,
            interpreted_missing,
            interpreted_repairs,
            retained: true,
        }
    }
}

impl DisplayList {
    /// Draws the page as [`Page::render`] would with the same `options`.
    ///
    /// Every option applies exactly as it does there — the scale and its
    /// clamp, the region, the format, the anti-aliasing, a transparent or
    /// premultiplied page, cancellation, and whether annotations are drawn —
    /// because this is the same pipeline with the interpretation replaced by
    /// a replay of what the interpretation produced.
    ///
    /// With [`RenderOptions::parallelism`] asking for two threads or more,
    /// the page is drawn in horizontal bands on that many threads of the
    /// engine's pool (the module documentation): the same bitmap and, for a
    /// render that runs to its end, the same warnings.
    ///
    /// A list that is not retained ([`DisplayList::is_retained`]) draws by
    /// calling [`Page::render`], on the caller.
    #[must_use]
    pub fn render(&self, options: &RenderOptions) -> Bitmap {
        if !self.retained {
            // On the caller: a page asked for in bands asks its list for
            // them, and a list that keeps nothing draws it the direct way.
            return self.page.render(&RenderOptions {
                parallelism: Parallelism::Serial,
                ..options.clone()
            });
        }
        if let Some(bitmap) = self.render_in_bands(options) {
            return bitmap;
        }
        let resources = self
            .resources
            .for_one_render_counted(crate::stats::tally(options.stats));
        self.page
            .render_layer_with(options, None, &resources, |renderer, resources| {
                self.replay_onto(options, renderer, resources);
            })
    }

    /// The list's calls onto `target`: the content, what its interpretation
    /// could not resolve noted by a replay that ran to the end, and the
    /// annotations after them where `options` draws them, each in the scope
    /// its appearance resolves in. See the module documentation for why a
    /// cancelled replay notes none of what the interpretation met.
    fn replay_onto(
        &self,
        options: &RenderOptions,
        target: &mut impl Target,
        resources: &PageResources,
    ) {
        let replayed = replay(&self.content, target);
        if !replayed.cancelled {
            resources.note_missing_fonts(&self.interpreted_missing);
            resources.note_repaired_spaces(&self.interpreted_repairs);
        }
        target.settle();
        if options.annotations {
            for annotation in &self.annotations {
                target.push(resources.bind(&annotation.scope));
                replay(&annotation.events, target);
                target.pop();
            }
        }
    }

    /// The list drawn in horizontal bands on the threads
    /// `options.parallelism` names (`docs/design/parallelism.md`, "Bands of
    /// one page"), or `None` where it is drawn whole: asked for no threads, or
    /// for one, a view of fewer than two rows, or a page whose runs of images
    /// leave no row to cut at.
    ///
    /// Each band is the view's own rows drawn as a region of the page in its
    /// one frame (ruling 5), so it is byte-equal to those rows of the page
    /// drawn whole; the cuts keep each run of images inside one band
    /// ([`DisplayList::image_runs`], [`cuts`]), which is the decision ruling 5
    /// leaves to the canvas. Every band replays the whole list over the list's
    /// caches, which fill once however many bands miss a key at once, and
    /// writes its rows into one bitmap sized to the view. What the bands met
    /// is merged into the list the page drawn whole reports ([`merged`]). On
    /// the pool's workers where they start, and on the caller where none does
    /// — wasm32, a host at its thread limit — with the same output; each band
    /// counts where it ran into the render's one tally.
    fn render_in_bands(&self, options: &RenderOptions) -> Option<Bitmap> {
        let threads = match options.parallelism {
            Parallelism::Threads(threads) if threads >= 2 => threads.min(MAX_THREADS),
            _ => return None,
        };
        let page = &self.page;
        let View {
            scale,
            applied,
            full_width,
            full_height,
            view,
            asked,
        } = page.view(options, None);
        if view.width == 0 || view.height < 2 {
            return None;
        }
        let base = page.base_at(applied);
        let runs = self.image_runs(options, &base, (full_width, full_height));
        let count = threads.min(usize::try_from(view.height).unwrap_or(usize::MAX));
        let bands = bands_of(view, &cuts(&runs, view, count));
        if bands.len() < 2 {
            return None;
        }

        let work = crate::stats::tally(options.stats);
        let format = handed_back(options);
        let stride = (view.width as usize).saturating_mul(format.components());
        let mut data = vec![0u8; stride.saturating_mul(view.height as usize)];
        // Each band's rows of the one bitmap, its own to write.
        let mut slots: Vec<Mutex<&mut [u8]>> = Vec::with_capacity(bands.len());
        let mut rest = data.as_mut_slice();
        for band in &bands {
            let (rows, after) = rest.split_at_mut(stride.saturating_mul(band.height as usize));
            slots.push(Mutex::new(rows));
            rest = after;
        }
        let parts: Vec<Mutex<Option<BandMet>>> = bands.iter().map(|_| Mutex::new(None)).collect();
        let premultiplied = Mutex::new(false);
        options.parallelism.run(bands.len(), &|index| {
            let (Some(band), Some(slot), Some(part)) =
                (bands.get(index), slots.get(index), parts.get(index))
            else {
                return;
            };
            if let Some(work) = &work {
                let place = if on_worker() {
                    Counter::PoolItemsOnWorkers
                } else {
                    Counter::PoolItemsOnCaller
                };
                work.add(place, 1);
            }
            let resources = self.resources.for_one_render_counted(work.clone());
            let band_options = RenderOptions {
                region: Some(*band),
                parallelism: Parallelism::Serial,
                ..options.clone()
            };
            let mut stamps = Stamps::default();
            let layer = page.draw_layer(&band_options, None, &resources, |renderer, resources| {
                let mut stamping = Stamping {
                    renderer,
                    resources,
                    ordinal: 0,
                    stamps: Stamps::default(),
                };
                self.replay_onto(options, &mut stamping, resources);
                stamps = stamping.stamps;
            });
            let mut rows = slot.lock().unwrap_or_else(PoisonError::into_inner);
            let rows: &mut [u8] = &mut rows;
            let length = rows.len().min(layer.pixels.data.len());
            if let (Some(to), Some(from)) =
                (rows.get_mut(..length), layer.pixels.data.get(..length))
            {
                to.copy_from_slice(from);
            }
            *premultiplied.lock().unwrap_or_else(PoisonError::into_inner) =
                layer.pixels.premultiplied;
            *part.lock().unwrap_or_else(PoisonError::into_inner) = Some(BandMet {
                met: layer.met,
                stamps,
            });
        });

        let parts: Vec<BandMet> = parts
            .into_iter()
            .filter_map(|part| part.into_inner().unwrap_or_else(PoisonError::into_inner))
            .collect();
        let warnings = merged(
            &parts,
            (applied < scale).then_some((scale, applied)),
            asked
                .filter(|requested| *requested != view)
                .map(|requested| (requested, view)),
        );
        drop(slots);
        Some(Bitmap {
            width: view.width,
            height: view.height,
            format,
            stride,
            data,
            warnings,
            premultiplied: premultiplied
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner),
            stats: work.as_ref().map(|work| work.stats()),
        })
    }

    /// The rows of the page each run of image draws covers, at the render's
    /// scale: each maximal sequence of two image draws or more with no call
    /// between them the renderer ends a run on
    /// ([`tinker_pdf_render::ends_image_run`]) — in the content, its forms,
    /// groups and soft masks, and the annotations after it where `options`
    /// draws them, in the order a replay delivers them — as the rows of the
    /// page its draws' unit squares reach (`image_bounds_in`'s rectangle,
    /// under each draw's transform and the page's `base`).
    ///
    /// A superset of the runs the renderer holds: a draw a hidden layer hides,
    /// or that a change of state or an overlap makes the renderer start a run
    /// of its own at, is counted into the sequence it stands in, and only
    /// costs a cut a row it could have had. A single draw is a run of one and
    /// constrains nothing. A tiling cell is drawn on a canvas of its own,
    /// the same in a band as on the page, so its runs are not here.
    fn image_runs(
        &self,
        options: &RenderOptions,
        base: &Matrix,
        page: (u32, u32),
    ) -> Vec<(u32, u32)> {
        let annotations = self
            .annotations
            .iter()
            .filter(|_| options.annotations)
            .flat_map(|annotation| annotation.events.iter());
        let mut runs = Vec::new();
        // The open sequence's first row, its row past the last, and its draws.
        let mut open: Option<(u32, u32, usize)> = None;
        let close = |open: &mut Option<(u32, u32, usize)>, runs: &mut Vec<(u32, u32)>| {
            if let Some((top, bottom, draws)) = open.take() {
                if draws >= 2 && top < bottom {
                    runs.push((top, bottom));
                }
            }
        };
        for event in self.content.iter().chain(annotations) {
            match event {
                Event::DrawImage { state, .. } => {
                    // A call recorded with no state is replayed in the
                    // initial one, and its square is placed by that.
                    let ctm = state.as_deref().map_or(Matrix::IDENTITY, |state| state.ctm);
                    let sequence = open.get_or_insert((u32::MAX, 0, 0));
                    if let Some((top, bottom)) = rows_of(&ctm.then(base), page) {
                        sequence.0 = sequence.0.min(top);
                        sequence.1 = sequence.1.max(bottom);
                    }
                    sequence.2 += 1;
                }
                other if tinker_pdf_render::ends_image_run(other.kind()) => {
                    close(&mut open, &mut runs);
                }
                _ => {}
            }
        }
        close(&mut open, &mut runs);
        runs
    }

    /// The page the list was recorded from.
    pub(crate) fn page(&self) -> &Page {
        &self.page
    }

    /// The resources a replay resolves names in.
    pub(crate) fn resources(&self) -> &PageResources {
        &self.resources
    }

    /// What the recording's interpretation could not resolve.
    pub(crate) fn interpreted_missing(&self) -> &[String] {
        &self.interpreted_missing
    }

    /// The colour spaces the recording's interpretation read with a repair.
    pub(crate) fn interpreted_repairs(&self) -> &[(String, String)] {
        &self.interpreted_repairs
    }

    /// The page's own calls.
    pub(crate) fn content(&self) -> &[Event] {
        &self.content
    }

    /// Each annotation's resource scope and calls, in `/Annots` order.
    pub(crate) fn annotation_layers(
        &self,
    ) -> impl Iterator<Item = (&Arc<PageResources>, &[Event])> {
        self.annotations
            .iter()
            .map(|annotation| (&annotation.scope, annotation.events.as_slice()))
    }

    /// Which page of its document this is.
    #[must_use]
    pub fn page_index(&self) -> u32 {
        self.page.index()
    }

    /// How many of the page's content's calls the list holds, not counting
    /// its annotations — all of them, or none for a list that is not
    /// retained.
    ///
    /// A measure of what the list holds rather than of what it draws: a glyph
    /// is one call and so is a full-page image.
    #[must_use]
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// Whether the list holds no calls: a page whose content made none, or a
    /// list that is not retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Whether the list holds the page's calls. False when recording them
    /// would have held more than `tinker_pdf_render::MAX_DISPLAY_LIST_BYTES`
    /// (64 MiB) — a page whose forms fan out into millions of calls — in
    /// which case it holds none, and every [`DisplayList::render`] and
    /// [`DisplayList::to_svg`] interprets the page again, exactly as
    /// [`Page::render`] and [`Page::to_svg`] do: the same output, at a direct
    /// render's cost.
    #[must_use]
    pub fn is_retained(&self) -> bool {
        self.retained
    }
}

/// The rows of a page `page` pixels in size an image's unit square reaches
/// under `to_device`, as its first row and the row past its last; `None` for
/// a square that reaches none.
fn rows_of(to_device: &Matrix, page: (u32, u32)) -> Option<(u32, u32)> {
    let t = Transform {
        a: to_device.a,
        b: to_device.b,
        c: to_device.c,
        d: to_device.d,
        e: to_device.e,
        f: to_device.f,
    };
    let (_, y, _, height) = image_bounds_in(&t, (0, 0, page.0, page.1))?;
    // Clamped into the page, so inside `0..page.1`.
    let top = u32::try_from(y).ok()?;
    Some((top, top.saturating_add(height)))
}

/// Where `count` bands of `view` are cut: the page rows
/// `view.y + round(k × height ÷ count)` for `k` from 1, each moved to the
/// nearest row no run of `runs` has rows on both sides of — the lower of two
/// as near — and kept between the cut before it and the next cut's own row;
/// a cut with no such row is dropped, and the view is drawn in fewer bands
/// (`docs/design/parallelism.md`, "Bands of one page"). A cut is the first
/// row of the band below it.
fn cuts(runs: &[(u32, u32)], view: PixelRegion, count: usize) -> Vec<u32> {
    // The rows no cut may be, as inclusive ranges, merged where they meet:
    // every row strictly inside a run.
    let mut blocked: Vec<(u32, u32)> = runs
        .iter()
        .filter(|(top, bottom)| bottom.saturating_sub(*top) >= 2)
        .map(|&(top, bottom)| (top + 1, bottom - 1))
        .collect();
    blocked.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(blocked.len());
    for (from, to) in blocked {
        match merged.last_mut() {
            Some(last) if from <= last.1.saturating_add(1) => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    let inside = |row: u32| {
        let at = merged.partition_point(|&(from, _)| from <= row);
        at.checked_sub(1)
            .and_then(|at| merged.get(at))
            .filter(|&&(_, to)| row <= to)
            .copied()
    };
    let (top, height) = (u64::from(view.y), u64::from(view.height));
    let n = u64::try_from(count).unwrap_or(u64::MAX).max(1);
    // round(k × height ÷ n), half up, in integers.
    let ideal = |k: u64| {
        let offset = (2 * k * height + n) / (2 * n);
        u32::try_from(top + offset).unwrap_or(u32::MAX)
    };
    let end = u32::try_from(top + height).unwrap_or(u32::MAX);
    let mut placed: Vec<u32> = Vec::new();
    for k in 1..n {
        let lower = placed.last().copied().unwrap_or(view.y);
        let upper = if k + 1 < n { ideal(k + 1) } else { end };
        let want = ideal(k);
        let candidates = match inside(want) {
            None => [Some(want), None],
            Some((from, to)) => {
                let (below, above) = (from.checked_sub(1), to.checked_add(1));
                let near = |row: Option<u32>| row.map_or(u32::MAX, |row| row.abs_diff(want));
                if near(above) < near(below) {
                    [above, below]
                } else {
                    [below, above]
                }
            }
        };
        if let Some(cut) = candidates
            .into_iter()
            .flatten()
            .find(|&row| lower < row && row < upper)
        {
            placed.push(cut);
        }
    }
    placed
}

/// The bands of `view` its `cuts` make, top to bottom, the whole width each.
fn bands_of(view: PixelRegion, cuts: &[u32]) -> Vec<PixelRegion> {
    let end = view.y.saturating_add(view.height);
    let mut edges = Vec::with_capacity(cuts.len() + 2);
    edges.push(view.y);
    edges.extend_from_slice(cuts);
    edges.push(end);
    edges
        .windows(2)
        .filter_map(|pair| match pair {
            [from, to] if from < to => Some(PixelRegion::new(view.x, *from, view.width, to - from)),
            _ => None,
        })
        .collect()
}

/// The ordinal of the call a warning was first met at among the calls a
/// replay delivers, and the warning's place among those that call met.
type Stamp = (u64, u32);

/// The stamps of what one band's render met: one for each warning of the
/// renderer's list met during the calls, each damaged image and each
/// repaired space of the resources', in their orders.
#[derive(Default)]
struct Stamps {
    met: Vec<Stamp>,
    damaged: Vec<Stamp>,
    repaired: Vec<Stamp>,
}

/// Stamps the entries a list has gained since `stamps` last saw it, `now` long
/// after the call `ordinal`.
fn stamp(stamps: &mut Vec<Stamp>, now: usize, ordinal: u64) {
    for place in 0..now.saturating_sub(stamps.len()) {
        stamps.push((ordinal, u32::try_from(place).unwrap_or(u32::MAX)));
    }
}

/// One band's render: what it met and the calls it met each at.
struct BandMet {
    met: Met,
    stamps: Stamps,
}

/// The warnings of a page drawn in bands, as the page drawn whole reports
/// them (`docs/design/parallelism.md`, "Warnings equal the serial render's").
///
/// Every band replays the same calls, so a call's ordinal is the same in
/// each, and the page met a warning first at the first call that produced it
/// on its canvas — a call some band's canvas holds. So each warning the
/// renderer met during the calls is kept once, at the least stamp any band
/// met it at, and ordered by that; then what the renderers added at the end,
/// in the fixed order a render adds it; then the facade's, each damaged image
/// and repaired space at its least stamp likewise; the page scaled down and
/// the caller's region clamped, which belong to the whole. A band lies inside
/// the page, so none is clamped itself.
fn merged(
    parts: &[BandMet],
    scaled_down: Option<(f64, f64)>,
    clamped: Option<(PixelRegion, PixelRegion)>,
) -> Vec<RenderWarning> {
    let mut calls: Vec<(Stamp, RenderWarning)> = Vec::new();
    let mut ending: Vec<RenderWarning> = Vec::new();
    for part in parts {
        for (index, warning) in part.met.renderer.iter().enumerate() {
            match part.stamps.met.get(index) {
                Some(&stamp) => keep_least(&mut calls, stamp, warning.clone()),
                None => {
                    if !ending.contains(warning) {
                        ending.push(warning.clone());
                    }
                }
            }
        }
    }
    calls.sort_by(|a, b| a.0.cmp(&b.0));
    // `Renderer::finish`'s order; a sort that keeps the order of the rest.
    ending.sort_by_key(|warning| match warning {
        RenderWarning::UnreadableFont => 0,
        RenderWarning::GroupBudgetSpent { .. } => 1,
        RenderWarning::RecursiveContent => 2,
        RenderWarning::NestedBudgetSpent => 3,
        RenderWarning::Cancelled => 4,
        _ => 5,
    });
    let mut renderer: Vec<RenderWarning> = calls.into_iter().map(|(_, warning)| warning).collect();
    renderer.extend(ending);
    let listed = |pick: &dyn Fn(&BandMet) -> (&[(String, String)], &[Stamp])| {
        let mut least: Vec<(Stamp, (String, String))> = Vec::new();
        for part in parts {
            let (entries, stamps) = pick(part);
            for (index, entry) in entries.iter().enumerate() {
                let stamp = stamps
                    .get(index)
                    .copied()
                    .unwrap_or((u64::MAX, u32::try_from(index).unwrap_or(u32::MAX)));
                keep_least(&mut least, stamp, entry.clone());
            }
        }
        least.sort_by(|a, b| a.0.cmp(&b.0));
        // The cap a render's own list keeps (`PageResources`).
        least.truncate(64);
        least
            .into_iter()
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>()
    };
    let damaged = listed(&|part| (&part.met.damaged, &part.stamps.damaged));
    let repaired = listed(&|part| (&part.met.repaired, &part.stamps.repaired));
    Met {
        renderer,
        missing_fonts: parts.iter().any(|part| part.met.missing_fonts),
        damaged,
        repaired,
        scaled_down,
        clamped,
    }
    .warnings()
}

/// Keeps `value` in `kept` once, at the least `stamp` it has been met at.
fn keep_least<T: PartialEq>(kept: &mut Vec<(Stamp, T)>, stamp: Stamp, value: T) {
    match kept.iter_mut().find(|(_, held)| *held == value) {
        Some(held) => held.0 = held.0.min(stamp),
        None => kept.push((stamp, value)),
    }
}

/// Where a list's calls are replayed: the renderer, or the renderer behind a
/// [`Stamping`].
trait Target: Device {
    /// [`Renderer::push_resources`].
    fn push(&mut self, scope: Arc<PageResources>);
    /// [`Renderer::pop_resources`].
    fn pop(&mut self);
    /// Marks the end of the content's calls, after what the recording's
    /// interpretation met is noted.
    fn settle(&mut self) {}
}

impl Target for Renderer<'_, PageResources> {
    fn push(&mut self, scope: Arc<PageResources>) {
        self.push_resources(scope);
    }

    fn pop(&mut self) {
        self.pop_resources();
    }
}

/// A renderer handed each recorded call in turn, stamping what each call made
/// the render meet — the renderer's warnings and the resources' damaged
/// images and repaired spaces — with the call's ordinal, for [`merged`].
struct Stamping<'a, 'g> {
    renderer: &'a mut Renderer<'g, PageResources>,
    resources: &'a PageResources,
    ordinal: u64,
    stamps: Stamps,
}

impl Stamping<'_, '_> {
    /// Stamps what the call just delivered made the render meet, and counts
    /// the call.
    fn after(&mut self) {
        let ordinal = self.ordinal;
        stamp(&mut self.stamps.met, self.renderer.met().len(), ordinal);
        let (damaged, repaired) = self.resources.tolerated();
        stamp(&mut self.stamps.damaged, damaged, ordinal);
        stamp(&mut self.stamps.repaired, repaired, ordinal);
        self.ordinal = self.ordinal.saturating_add(1);
    }
}

impl Target for Stamping<'_, '_> {
    fn push(&mut self, scope: Arc<PageResources>) {
        self.renderer.push_resources(scope);
    }

    fn pop(&mut self) {
        self.renderer.pop_resources();
    }

    fn settle(&mut self) {
        self.after();
    }
}

impl Device for Stamping<'_, '_> {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.renderer.show_glyph(glyph, state);
        self.after();
    }

    fn begin_text(&mut self) {
        self.renderer.begin_text();
        self.after();
    }

    fn end_text(&mut self) {
        self.renderer.end_text();
        self.after();
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.renderer.fill_path(path, state, even_odd);
        self.after();
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        self.renderer.stroke_path(path, state);
        self.after();
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.renderer.clip_path(path, state, even_odd);
        self.after();
    }

    fn save_state(&mut self) {
        self.renderer.save_state();
        self.after();
    }

    fn restore_state(&mut self) {
        self.renderer.restore_state();
        self.after();
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        self.renderer.draw_image(image, state);
        self.after();
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        self.renderer.draw_shading(name, state);
        self.after();
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        self.renderer
            .begin_marked_content(tag, visible, hidden_layer, props);
        self.after();
    }

    fn end_marked_content(&mut self) {
        self.renderer.end_marked_content();
        self.after();
    }

    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        let entered = self.renderer.begin_form(id, name);
        self.after();
        entered
    }

    fn end_form(&mut self, id: u64) {
        self.renderer.end_form(id);
        self.after();
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        let entered = self.renderer.begin_group(group, state);
        self.after();
        entered
    }

    fn end_group(&mut self) {
        self.renderer.end_group();
        self.after();
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        let entered = self.renderer.begin_soft_mask(mask, bbox, state);
        self.after();
        entered
    }

    fn end_soft_mask(&mut self) {
        self.renderer.end_soft_mask();
        self.after();
    }

    fn clear_soft_mask(&mut self) {
        self.renderer.clear_soft_mask();
        self.after();
    }

    fn group_budget_spent(&mut self) {
        self.renderer.group_budget_spent();
        self.after();
    }

    fn recursion_refused(&mut self) {
        self.renderer.recursion_refused();
        self.after();
    }

    fn nested_budget_spent(&mut self) {
        self.renderer.nested_budget_spent();
        self.after();
    }

    fn is_cancelled(&self) -> bool {
        self.renderer.is_cancelled()
    }
}

impl core::fmt::Debug for DisplayList {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DisplayList")
            .field("page", &self.page.index())
            .field("retained", &self.retained)
            .field("calls", &self.content.len())
            .field("annotations", &self.annotations.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tinker_pdf_color::Function;
    use tinker_pdf_content::{
        BlackPointCompensation, BlendMode, Event, GraphicsState, Halftone, RenderingIntent,
        SpotFunction, StoredParameters, Transfer,
    };

    use crate::Document;

    /// A document whose pages share one indirect `/Resources` (object 3)
    /// holding `states` as its `/ExtGState`, and a transparency group form
    /// (object 4) a soft mask can name. Page `i` is 40 x 40 and draws
    /// `contents[i]`.
    fn document(states: &str, extra: &[String], contents: &[&str]) -> Vec<u8> {
        document_in(&format!("<< /ExtGState << {states} >> >>"), extra, contents)
    }

    /// [`document`], with `resources` as object 3 as written.
    fn document_in(resources: &str, extra: &[String], contents: &[&str]) -> Vec<u8> {
        let first = 5 + extra.len();
        let kids: Vec<String> = (0..contents.len())
            .map(|i| format!("{} 0 R", first + 2 * i))
            .collect();
        let mask = "1 g 0 0 20 40 re f";
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!(
                "<< /Type /Pages /Count {} /Kids [{}] >>",
                contents.len(),
                kids.join(" ")
            ),
            resources.to_string(),
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] \
                 /Group << /S /Transparency /CS /DeviceGray >> /Length {} >>\n\
                 stream\n{mask}\nendstream",
                mask.len()
            ),
        ];
        objects.extend(extra.iter().cloned());
        for (i, content) in contents.iter().enumerate() {
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Resources 3 0 R \
                 /Contents {} 0 R >>",
                first + 1 + 2 * i
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ));
        }
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// Each page's recording.
    fn recordings(states: &str, contents: &[&str]) -> Vec<Vec<Event>> {
        recordings_with(states, &[], contents)
    }

    /// Each page's recording, with `extra` as objects 5 onwards.
    fn recordings_with(states: &str, extra: &[String], contents: &[&str]) -> Vec<Vec<Event>> {
        recordings_of(document(states, extra, contents))
    }

    /// Each page's recording of `bytes`.
    fn recordings_of(bytes: Vec<u8>) -> Vec<Vec<Event>> {
        let doc = Document::open(bytes).expect("the document opens");
        (0..doc.page_count())
            .map(|i| {
                let list = doc.page(i).expect("a page").display_list();
                assert!(list.is_retained(), "page {i} is retained");
                list.content().to_vec()
            })
            .collect()
    }

    /// Where two recordings part, with both events, or `None` when they are
    /// equal.
    fn parting(a: &[Event], b: &[Event]) -> Option<String> {
        if a == b {
            return None;
        }
        let at = a
            .iter()
            .zip(b)
            .position(|(x, y)| x != y)
            .unwrap_or(a.len().min(b.len()));
        Some(format!(
            "they part at event {at} of {} and {}: {:?} against {:?}",
            a.len(),
            b.len(),
            a.get(at),
            b.get(at)
        ))
    }

    /// The states of the painting events, in order.
    fn painted(events: &[Event]) -> Vec<&GraphicsState> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::FillPath { state, .. } | Event::StrokePath { state, .. } => state.as_deref(),
                _ => None,
            })
            .collect()
    }

    /// **A `gs` and its operator twin record the same events**, on two pages
    /// of one document sharing one `/Resources`; `/CA`, `/ca`, `/BM` and
    /// `/SMask` record their pinned values; and each stored key's recorded
    /// value is the dictionary's, kept through a later `gs` that omits it and
    /// restored by `Q`.
    #[test]
    fn gs_twins_record_the_same_events() {
        let twins = [
            ("/LW 3", "3 w"),
            ("/LW -1", "-1 w"),
            ("/LC 1", "1 J"),
            ("/LC 7", "7 J"),
            ("/LJ 2", "2 j"),
            ("/ML 1", "1 M"),
            ("/ML 0.5", "0.5 M"),
            ("/D [[3 2] 1]", "[3 2] 1 d"),
            ("/D [[0 0] 0]", "[0 0] 0 d"),
            ("/D [[-1 2] 0]", "[-1 2] 0 d"),
            ("/RI /Perceptual", "/Perceptual ri"),
            ("/RI /Vivid", "/Vivid ri"),
            ("/FL 5", "5 i"),
        ];
        let stroke = "0 0 1 RG 6 6 m 20 34 l 34 6 l S";
        let mut states = String::new();
        let mut pages = Vec::new();
        for (index, (entry, operator)) in twins.iter().enumerate() {
            states.push_str(&format!("/S{index} << {entry} >> "));
            pages.push(format!("q /S{index} gs {stroke} Q {stroke}"));
            pages.push(format!("q {operator} {stroke} Q {stroke}"));
        }
        // Table 58: an absent `/op` takes `/OP`'s value.
        states.push_str("/OPOnly << /OP true >> /OPBoth << /OP true /op true >> ");
        pages.push(format!("/OPOnly gs {stroke}"));
        pages.push(format!("/OPBoth gs {stroke}"));
        let contents: Vec<&str> = pages.iter().map(String::as_str).collect();
        let recorded = recordings(&states, &contents);
        for (index, pair) in recorded.chunks(2).enumerate() {
            let label = twins.get(index).map_or("/OP alone", |(entry, _)| entry);
            assert!(
                parting(&pair[0], &pair[1]).is_none(),
                "{label}: {}",
                parting(&pair[0], &pair[1]).unwrap_or_default()
            );
        }

        // A font set by `gs` records the events its `Tf` twin records: the
        // state gives the object the name `/Font` binds it to, so every glyph
        // carries the same id and every state the same name. A page whose
        // `/Font` does not name the object differs in exactly those two: its
        // glyphs carry the reserved name's id and its states that name.
        let helvetica = ["<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string()];
        let show = "5 5 Td (Hi) Tj ET";
        let named = recordings_of(document_in(
            "<< /Font << /F1 5 0 R >> /ExtGState << /G << /Font [5 0 R 12] >> >> >>",
            &helvetica,
            &[&format!("BT /F1 12 Tf {show}"), &format!("/G gs BT {show}")],
        ));
        assert!(
            parting(&named[0], &named[1]).is_none(),
            "/Font: {}",
            parting(&named[0], &named[1]).unwrap_or_default()
        );
        let glyphs = |events: &[Event]| -> Vec<(u64, Option<Vec<u8>>, f64)> {
            events
                .iter()
                .filter_map(|event| match event {
                    Event::ShowGlyph { glyph, state } => state
                        .as_deref()
                        .map(|state| (glyph.font_id, state.text.font.clone(), state.text.size)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(glyphs(&named[1]).len(), 2, "both glyphs are shown");
        let unnamed = recordings_of(document_in(
            "<< /ExtGState << /G << /Font [5 0 R 12] >> >> >>",
            &helvetica,
            &[&format!("/G gs BT {show}")],
        ));
        let reserved = 1u64 << 63 | 5 << 16;
        assert_eq!(
            glyphs(&unnamed[0]),
            vec![(reserved, Some(b"\x005 0".to_vec()), 12.0); 2],
            "the reserved name and its id"
        );
        let normalise = |events: &[Event]| -> Vec<Event> {
            events
                .iter()
                .cloned()
                .map(|mut event| {
                    if let Event::ShowGlyph { glyph, state } = &mut event {
                        glyph.font_id = 0;
                        if let Some(state) = state.as_deref_mut() {
                            state.text.font = None;
                        }
                    }
                    event
                })
                .collect()
        };
        assert_eq!(
            normalise(&unnamed[0]),
            normalise(&named[0]),
            "the reserved name's page differs from its twin in the id and the name alone"
        );

        // The four keys with no operator, against their pins.
        let states = "/StrokeHalf << /CA 0.5 >> /FillHalf << /ca 0.5 >> \
                      /Multiply << /BM /Multiply >> \
                      /Mask << /SMask << /S /Luminosity /G 4 0 R >> >> /Unmask << /SMask /None >>";
        let pages = [
            "/StrokeHalf gs 0 0 1 RG 5 20 m 35 20 l S",
            "/FillHalf gs 0 0 1 rg 10 10 20 20 re f",
            "/Multiply gs 0.5 g 10 10 20 20 re f",
            "/Mask gs 0 0 1 rg 0 0 40 40 re f /Unmask gs 0 0 40 40 re f",
        ];
        let recorded = recordings(states, &pages);
        let stroked = painted(&recorded[0]);
        assert_eq!((stroked[0].stroke_alpha, stroked[0].fill_alpha), (0.5, 1.0));
        let filled = painted(&recorded[1]);
        assert_eq!((filled[0].fill_alpha, filled[0].stroke_alpha), (0.5, 1.0));
        assert_eq!(painted(&recorded[2])[0].blend, BlendMode::Multiply);
        let masked = &recorded[3];
        let mask = masked.iter().find_map(|event| match event {
            Event::BeginSoftMask { mask, entered, .. } => Some((mask, *entered)),
            _ => None,
        });
        let (mask, entered) = mask.expect("the `gs` offered its mask group");
        assert!(entered && mask.luminosity);
        assert_eq!((mask.backdrop, mask.transfer), (None, None));
        assert_eq!(mask.form.bbox, Some([0.0, 0.0, 40.0, 40.0]));
        assert_eq!(mask.form.stream, 4 << 16, "`/G` is object 4");
        assert!(
            masked
                .iter()
                .any(|event| matches!(event, Event::ClearSoftMask)),
            "`/SMask /None` clears the mask"
        );

        // Every stored key, kept through a `gs` that omits it and restored by
        // `Q`.
        let states = "/Stored << /SM 0.5 /SA true /OP true /OPM 1 /AIS true /TK false \
                      /UseBlackPtComp /ON /RI /Saturation /FL 3 >> /Width << /LW 2 >>";
        let fill = "0 0 1 1 re f";
        let page = format!("q /Stored gs {fill} /Width gs {fill} Q {fill}");
        let recorded = recordings(states, &[&page]);
        let fills = painted(&recorded[0]);
        let expected = StoredParameters {
            rendering_intent: RenderingIntent::Saturation,
            black_point_compensation: BlackPointCompensation::On,
            stroke_overprint: true,
            fill_overprint: true,
            overprint_mode: 1,
            alpha_is_shape: true,
            text_knockout: false,
            flatness: Some(3.0),
            smoothness: Some(0.5),
            stroke_adjustment: Some(true),
            ..StoredParameters::INITIAL
        };
        assert_eq!(fills[0].stored_parameters(), &expected);
        assert_eq!(
            fills[1].stored_parameters(),
            &expected,
            "a `gs` that omits a key leaves it"
        );
        assert_eq!(fills[1].line_width, 2.0);
        assert!(fills[2].stored.is_none(), "`Q` restores the initial values");
        assert_eq!(fills[2].stored_parameters(), &StoredParameters::INITIAL);
    }

    /// **Every function key is stored as the dictionary writes it**: each
    /// recorded function evaluates as the test evaluates the dictionary's, a
    /// sampled one included; the second spelling wins when both are written;
    /// `/Default` restores the initial value; a key the table does not admit
    /// is absent; and a `gs` that omits a key and a `Q` behave as for the
    /// scalar keys. Four references to one function share one reading.
    #[test]
    fn every_function_key_is_stored() {
        // Object 5 samples a curve through 0, 64 and 255; object 6 is
        // `1 - x` as a calculator.
        let samples = "0040FF>";
        let calculator = "{ 1 exch sub }";
        let extra = [
            format!(
                "<< /FunctionType 0 /Domain [0 1] /Range [0 1] /Size [3] /BitsPerSample 8 \
                 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{samples}\nendstream",
                samples.len()
            ),
            format!(
                "<< /FunctionType 4 /Domain [0 1] /Range [0 1] /Length {} >>\n\
                 stream\n{calculator}\nendstream",
                calculator.len()
            ),
        ];
        let squared = "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >>";
        let cubed = "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 3 >>";
        let states = format!(
            "/TR << /TR 5 0 R >> /TRFour << /TR [6 0 R 6 0 R 6 0 R 6 0 R] >> \
             /TRIdentity << /TR /Identity >> /TRBoth << /TR 6 0 R /TR2 5 0 R >> \
             /TR2Default << /TR2 /Default >> /TRWrong << /TR /Default >> \
             /BG << /BG {squared} >> /BGBoth << /BG {squared} /BG2 {cubed} >> \
             /BG2Default << /BG2 /Default >> /UCR << /UCR 6 0 R >> \
             /UCRBoth << /UCR 6 0 R /UCR2 5 0 R >> /UCR2Default << /UCR2 /Default >> \
             /Width << /LW 2 >>"
        );
        let f = "0 0 1 1 re f";
        let page = format!(
            "q /TR gs /BG gs /UCR gs {f} /Width gs {f} /TRWrong gs {f} Q {f} \
             /TRFour gs {f} /TRIdentity gs {f} /TRBoth gs /BGBoth gs /UCRBoth gs {f} \
             /TR2Default gs /BG2Default gs /UCR2Default gs {f}"
        );
        let recorded = recordings_with(&states, &extra, &[&page]);
        let fills = painted(&recorded[0]);
        assert_eq!(fills.len(), 8);

        // The dictionary's functions, as the test evaluates them.
        let at = [0.0, 0.25, 0.5, 0.75, 1.0];
        let sampled = |x: f64| {
            let s = [0.0, 64.0, 255.0];
            let e = x * 2.0;
            let i = (e.floor() as usize).min(1);
            (s[i] + (e - i as f64) * (s[i + 1] - s[i])) / 255.0
        };
        let holds = |function: &Function, expected: &dyn Fn(f64) -> f64| {
            at.iter().all(|&x| {
                let got = function.eval(&[x]).first().copied().unwrap_or(f64::NAN);
                (got - expected(x)).abs() < 1e-9
            })
        };
        let one = |transfer: &Option<Transfer>| match transfer {
            Some(Transfer::One(function)) => Arc::clone(function),
            other => panic!("one transfer function was written, and {other:?} recorded"),
        };

        let set = fills[0].stored_parameters();
        assert!(holds(&one(&set.transfer), &sampled), "/TR, sampled");
        let generation = set.black_generation.as_deref().expect("/BG is stored");
        assert!(holds(generation, &|x| x * x), "/BG");
        let removal = set.undercolor_removal.as_deref().expect("/UCR is stored");
        assert!(holds(removal, &|x| 1.0 - x), "/UCR, a calculator");
        assert_eq!(fills[1].stored_parameters(), set, "a `gs` that omits them");
        assert_eq!(
            fills[2].stored_parameters(),
            set,
            "`/TR /Default` is not a value `/TR` admits, so it is absent"
        );
        assert!(fills[3].stored.is_none(), "`Q` restores the initial values");

        match &fills[4].stored_parameters().transfer {
            Some(Transfer::Four(four)) => {
                assert!(holds(&four[0], &|x| 1.0 - x));
                assert!(
                    four.iter().all(|function| Arc::ptr_eq(function, &four[0])),
                    "one object, read once"
                );
            }
            other => panic!("four functions were written, and {other:?} recorded"),
        }
        assert_eq!(
            fills[5].stored_parameters().transfer,
            Some(Transfer::Identity)
        );

        let both = fills[6].stored_parameters();
        assert!(holds(&one(&both.transfer), &sampled), "/TR2 over /TR");
        let generation = both.black_generation.as_deref().expect("/BG2 is stored");
        assert!(holds(generation, &|x| x * x * x), "/BG2 over /BG");
        let removal = both.undercolor_removal.as_deref().expect("/UCR2 is stored");
        assert!(holds(removal, &sampled), "/UCR2 over /UCR");

        assert_eq!(
            fills[7].stored_parameters(),
            &StoredParameters::INITIAL,
            "`/Default` restores each"
        );
    }

    /// **Every halftone key is stored as the dictionary or stream writes it**:
    /// a halftone of each type records its type and parameters, a threshold
    /// array by its stream, a type 5's entries per colorant; a halftone of the
    /// wrong shape is absent; `/Default` restores the initial value; `/HTO`
    /// records its origin; and a `gs` that omits a key and a `Q` behave as
    /// for the scalar keys.
    #[test]
    fn every_halftone_key_is_stored() {
        let stream = |dict: &str, data: &str| {
            format!(
                "<< {dict} /Filter /ASCIIHexDecode /Length {} >>\nstream\n{data}\nendstream",
                data.len()
            )
        };
        // Objects 5 to 9: a type 6, a type 10 and a type 16 halftone, a
        // transfer function and a spot function.
        let extra = [
            stream("/HalftoneType 6 /Width 2 /Height 2", "004080C0>"),
            stream("/HalftoneType 10 /Xsquare 1 /Ysquare 1", "0080>"),
            stream(
                "/HalftoneType 16 /Width 1 /Height 1 /Width2 1 /Height2 1 /TransferFunction 8 0 R",
                "00008000>",
            ),
            "<< /FunctionType 4 /Domain [0 1] /Range [0 1] /Length 14 >>\n\
             stream\n{ 1 exch sub }\nendstream"
                .to_string(),
            "<< /FunctionType 4 /Domain [-1 1 -1 1] /Range [-1 1] /Length 39 >>\n\
             stream\n{ dup mul exch dup mul add 1 exch sub }\nendstream"
                .to_string(),
        ];
        let screen = "<< /HalftoneType 1 /Frequency 60 /Angle 45 /SpotFunction /Round >>";
        let states = format!(
            "/Screen << /HT {screen} >> \
             /SpotFunction << /HT << /HalftoneType 1 /Frequency 85 /Angle 15 \
               /SpotFunction 9 0 R /AccurateScreens true /TransferFunction /Identity >> >> \
             /SpotNames << /HT << /HalftoneType 1 /Frequency 60 /Angle 0 \
               /SpotFunction [/Vivid /Ellipse] >> >> \
             /Type6 << /HT 5 0 R >> /Type10 << /HT 6 0 R >> /Type16 << /HT 7 0 R >> \
             /Type5 << /HT << /HalftoneType 5 /Default {screen} /Cyan 5 0 R >> >> \
             /NoDefault << /HT << /HalftoneType 5 /Cyan 5 0 R >> >> \
             /Nested << /HT << /HalftoneType 5 /Default << /HalftoneType 5 /Default {screen} >> >> >> \
             /NoFrequency << /HT << /HalftoneType 1 /Angle 45 /SpotFunction /Round >> >> \
             /Default << /HT /Default >> /Origin << /HTO [3 4] >> /Width << /LW 2 >>"
        );
        let f = "0 0 1 1 re f";
        let page = format!(
            "q /Screen gs /Origin gs {f} /Width gs {f} /NoFrequency gs {f} /NoDefault gs {f} \
             /Nested gs {f} Q {f} /SpotFunction gs {f} /SpotNames gs {f} /Type6 gs {f} \
             /Type10 gs {f} /Type16 gs {f} /Type5 gs {f} /Default gs {f}"
        );
        let recorded = recordings_with(&states, &extra, &[&page]);
        let fills = painted(&recorded[0]);
        assert_eq!(fills.len(), 13);
        let halftone = |at: usize| fills[at].stored_parameters().halftone.clone();
        let round = Halftone::Screen {
            frequency: 60.0,
            angle: 45.0,
            spot: SpotFunction::Names(vec![b"Round".to_vec()]),
            accurate: false,
            transfer: None,
        };

        assert_eq!(halftone(0).as_deref(), Some(&round), "type 1, by name");
        assert_eq!(
            fills[0].stored_parameters().halftone_origin,
            Some([3.0, 4.0])
        );
        for (at, what) in [
            (1, "a `gs` that omits it"),
            (2, "a type 1 with no /Frequency, absent"),
            (3, "a type 5 with no /Default, absent"),
            (4, "a type 5 inside a type 5, absent"),
        ] {
            assert_eq!(
                fills[at].stored_parameters(),
                fills[0].stored_parameters(),
                "{what}"
            );
        }
        assert!(fills[5].stored.is_none(), "`Q` restores the initial values");

        match halftone(6).as_deref() {
            Some(Halftone::Screen {
                frequency,
                angle,
                spot: SpotFunction::Function(spot),
                accurate,
                transfer,
            }) => {
                assert_eq!((*frequency, *angle, *accurate), (85.0, 15.0, true));
                assert_eq!(transfer, &Some(Transfer::Identity));
                assert_eq!(spot.eval(&[0.5, 0.5]), vec![0.5], "1 - x^2 - y^2");
            }
            other => panic!("a type 1 with a spot function, and {other:?} recorded"),
        }
        assert_eq!(
            halftone(7).as_deref(),
            Some(&Halftone::Screen {
                frequency: 60.0,
                angle: 0.0,
                spot: SpotFunction::Names(vec![b"Vivid".to_vec(), b"Ellipse".to_vec()]),
                accurate: false,
                transfer: None,
            }),
            "names in order of preference, as written"
        );
        assert_eq!(
            halftone(8).as_deref(),
            Some(&Halftone::Threshold {
                width: 2,
                height: 2,
                thresholds: 5 << 16,
                transfer: None,
            })
        );
        assert_eq!(
            halftone(9).as_deref(),
            Some(&Halftone::Squares {
                xsquare: 1,
                ysquare: 1,
                thresholds: 6 << 16,
                transfer: None,
            })
        );
        match halftone(10).as_deref() {
            Some(Halftone::Threshold16 {
                width: 1,
                height: 1,
                second: Some((1, 1)),
                thresholds,
                transfer: Some(Transfer::One(transfer)),
            }) => {
                assert_eq!(*thresholds, 7 << 16);
                assert_eq!(transfer.eval(&[0.25]), vec![0.75]);
            }
            other => panic!("a type 16 of two rectangles, and {other:?} recorded"),
        }
        assert_eq!(
            halftone(11).as_deref(),
            Some(&Halftone::Colorants(vec![
                (b"Default".to_vec(), round.clone()),
                (
                    b"Cyan".to_vec(),
                    Halftone::Threshold {
                        width: 2,
                        height: 2,
                        thresholds: 5 << 16,
                        transfer: None,
                    }
                ),
            ]))
        );
        assert_eq!(
            fills[12].stored_parameters(),
            &StoredParameters::INITIAL,
            "`/HT /Default` restores the initial value"
        );
    }
}

#[cfg(test)]
mod band_tests {
    use super::*;
    use crate::Document;

    /// A view of `height` rows at row `y`, 50 wide.
    fn view(y: u32, height: u32) -> PixelRegion {
        PixelRegion::new(0, y, 50, height)
    }

    /// With no run anywhere, the cuts are the even rows `round(k × height ÷ n)`
    /// from the view's top, half up, and a run of one row, which no cut can
    /// fall strictly inside, moves none of them.
    #[test]
    fn cuts_fall_evenly_where_no_run_stands() {
        assert_eq!(cuts(&[], view(0, 100), 4), vec![25, 50, 75]);
        // round(10 / 3) and round(20 / 3), from row 7.
        assert_eq!(cuts(&[], view(7, 10), 3), vec![10, 14]);
        // round(5 / 2) is 3, half up.
        assert_eq!(cuts(&[], view(0, 5), 2), vec![3]);
        assert_eq!(cuts(&[], view(0, 4), 4), vec![1, 2, 3], "a band a row");
        assert!(cuts(&[], view(0, 9), 1).is_empty(), "one band, no cut");
        // Row 24 alone holds no cut inside it; rows 50 and 51 hold only 51.
        assert_eq!(
            cuts(&[(24, 25), (50, 52)], view(0, 100), 4),
            vec![25, 50, 75]
        );
    }

    /// A cut that would fall strictly inside a run moves to the nearer of the
    /// rows just outside it — the run's first row, or the row past its last —
    /// the lower of two as near; the others stay where they were.
    #[test]
    fn a_cut_moves_to_the_nearest_row_no_run_spans() {
        // Rows 20 to 29: 25 is inside, 20 and 30 are five away each.
        assert_eq!(cuts(&[(20, 30)], view(0, 100), 4), vec![20, 50, 75]);
        // Rows 22 to 29: 22 is three away, 30 five.
        assert_eq!(cuts(&[(22, 30)], view(0, 100), 4), vec![22, 50, 75]);
        // Rows 18 to 27: 28 is three away, 18 seven.
        assert_eq!(cuts(&[(18, 28)], view(0, 100), 4), vec![28, 50, 75]);
        // A view starting below the page's top: its rows are the page's.
        assert_eq!(cuts(&[(60, 70)], view(40, 40), 2), vec![60]);
    }

    /// Runs that overlap or abut block their rows as one; a cut keeps between
    /// the cut before it and the next cut's own row, taking the farther free
    /// row where the nearer is outside them, and is dropped where both are;
    /// and a run over every row leaves no cut at all — the view drawn whole.
    #[test]
    fn a_cut_keeps_between_its_neighbours_or_is_dropped() {
        // Rows 10 to 39 and 39 to 69 block 11 to 68 as one: 25 moves to 10,
        // 50 to 70, and 75 is free.
        assert_eq!(
            cuts(&[(10, 40), (39, 70)], view(0, 100), 4),
            vec![10, 70, 75]
        );
        // 50 inside rows 45 to 79 moves to 45, the nearer; 75 inside them
        // moves to 80, the nearer, which is past 45.
        assert_eq!(cuts(&[(45, 80)], view(0, 100), 4), vec![25, 45, 80]);
        // 25 inside rows 20 to 79 moves to 20; 50 has 20, not past the cut
        // before it, and 80, past 75, its neighbour: dropped; 75 moves to 80.
        assert_eq!(cuts(&[(20, 80)], view(0, 100), 4), vec![20, 80]);
        assert_eq!(
            bands_of(view(0, 100), &[20, 80]),
            vec![view(0, 20), view(20, 60), view(80, 20)],
            "the run in a band of its own"
        );
        // A run over every row: each cut's free rows are the view's own
        // edges, which no cut may be.
        assert!(cuts(&[(0, 100)], view(0, 100), 4).is_empty());
        assert_eq!(bands_of(view(0, 100), &[]), vec![view(0, 100)]);
    }

    /// The bands of a view tile it, top to bottom, the whole width each.
    #[test]
    fn the_bands_tile_the_view() {
        let region = PixelRegion::new(13, 40, 21, 60);
        let bands = bands_of(region, &[55, 61, 99]);
        assert_eq!(
            bands,
            vec![
                PixelRegion::new(13, 40, 21, 15),
                PixelRegion::new(13, 55, 21, 6),
                PixelRegion::new(13, 61, 21, 38),
                PixelRegion::new(13, 99, 21, 1),
            ]
        );
        let rows: u32 = bands.iter().map(|band| band.height).sum();
        assert_eq!(rows, region.height);
    }

    /// A document of one 100 by 100 page drawing `content`, whose `/Im` is a
    /// one-sample grey image.
    fn page_drawing(content: &str) -> Vec<u8> {
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
   /Resources << /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
   /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length 3 >>\nstream\n40>\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
            content.len()
        )
        .into_bytes()
    }

    /// The runs a list's planner reads: image draws one after another, a `q`
    /// and a `Q` between them, are one sequence over their rows; a fill ends
    /// one; a single draw is no run; and the rows are the page's at the
    /// render's scale.
    #[test]
    fn the_runs_are_what_the_renderer_holds() {
        let content = "q 20 0 0 10 0 80 cm /Im Do Q q 20 0 0 10 30 60 cm /Im Do Q \
                       0 0 1 1 re f \
                       q 20 0 0 10 0 30 cm /Im Do Q \
                       0 0 1 1 re f \
                       q 20 0 0 5 0 10 cm /Im Do Q q 20 0 0 5 30 0 cm /Im Do Q \
                       q 20 0 0 5 60 20 cm /Im Do Q";
        let document = Document::open(page_drawing(content)).expect("it opens");
        let page = document.page(0).expect("a page");
        let list = page.display_list();
        let runs = |scale: f64| {
            let options = RenderOptions {
                scale,
                ..RenderOptions::default()
            };
            let View {
                applied,
                full_width,
                full_height,
                ..
            } = page.view(&options, None);
            list.image_runs(&options, &page.base_at(applied), (full_width, full_height))
        };
        // Rows from the top: the first two images over rows 10 to 39, the
        // lone one none, the last three over rows 75 to 99.
        assert_eq!(runs(1.0), vec![(10, 40), (75, 100)]);
        assert_eq!(runs(2.0), vec![(20, 80), (150, 200)]);
    }

    /// A band's render met with its stamps, for the merge.
    fn band(
        renderer: Vec<RenderWarning>,
        met: Vec<Stamp>,
        damaged: Vec<((&str, &str), Stamp)>,
    ) -> BandMet {
        BandMet {
            met: Met {
                renderer,
                missing_fonts: false,
                damaged: damaged
                    .iter()
                    .map(|((name, reason), _)| (name.to_string(), reason.to_string()))
                    .collect(),
                repaired: Vec::new(),
                scaled_down: None,
                clamped: None,
            },
            stamps: Stamps {
                met,
                damaged: damaged.iter().map(|(_, stamp)| *stamp).collect(),
                repaired: Vec::new(),
            },
        }
    }

    fn unsupported(codec: &str) -> RenderWarning {
        RenderWarning::UnsupportedImage {
            codec: codec.to_string(),
        }
    }

    /// **The bands' warnings are the page's.** A warning two bands met is
    /// kept once, at the earlier call, and the warnings are ordered by the
    /// call each was first met at — so a band below that met the page's first
    /// warning, and a band above that met only its second, merge into the
    /// page's order; what the renderers added at the end follows, in the
    /// order a render adds it, each once; then the facade's, by the same
    /// rule, and the page's own scale and region.
    #[test]
    fn warnings_merge_in_the_order_the_page_met_them() {
        let top = band(
            vec![unsupported("NotJpeg"), RenderWarning::RecursiveContent],
            vec![(9, 0)],
            vec![(("HeadCut", "truncated"), (9, 0))],
        );
        let bottom = band(
            vec![
                unsupported("JBIG2Decode"),
                unsupported("NotJpeg"),
                RenderWarning::UnreadableFont,
                RenderWarning::RecursiveContent,
            ],
            vec![(4, 0), (9, 0)],
            vec![
                (("FootCut", "truncated"), (4, 0)),
                (("HeadCut", "truncated"), (9, 0)),
            ],
        );
        let region = PixelRegion::new(0, 0, 500, 500);
        let applied = PixelRegion::new(0, 0, 100, 120);
        let warnings = merged(&[top, bottom], Some((8.0, 4.0)), Some((region, applied)));
        assert_eq!(
            warnings,
            vec![
                unsupported("JBIG2Decode"),
                unsupported("NotJpeg"),
                RenderWarning::UnreadableFont,
                RenderWarning::RecursiveContent,
                RenderWarning::DamagedImage {
                    name: "FootCut".to_string(),
                    reason: "truncated".to_string(),
                },
                RenderWarning::DamagedImage {
                    name: "HeadCut".to_string(),
                    reason: "truncated".to_string(),
                },
                RenderWarning::PageScaledDown {
                    requested: 8.0,
                    applied: 4.0,
                },
                RenderWarning::RegionClamped {
                    requested: region,
                    applied,
                },
            ]
        );

        // Two warnings one call met keep that call's order; one a band met at
        // an earlier call keeps the earlier; and a font the resources missed,
        // said by one band, is said once where no renderer said it.
        let one = band(
            vec![unsupported("a"), unsupported("b")],
            vec![(3, 0), (3, 1)],
            vec![],
        );
        let mut other = band(
            vec![unsupported("a"), unsupported("b")],
            vec![(1, 0), (3, 0)],
            vec![],
        );
        other.met.missing_fonts = true;
        assert_eq!(
            merged(&[one, other], None, None),
            vec![
                unsupported("a"),
                unsupported("b"),
                RenderWarning::UnreadableFont
            ]
        );
    }
}
