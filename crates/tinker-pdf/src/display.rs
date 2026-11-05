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

use tinker_pdf_content::{interpret, replay, Event, Matrix};
use tinker_pdf_cos::pages as cos_pages;
use tinker_pdf_render::DisplayRecorder;

use crate::annots::{self, Recorded};
use crate::resources::PageResources;
use crate::{Bitmap, Page, RenderOptions};

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
            annots::record(&self.doc, &self.inner, self.fonts.as_ref(), &mut recorder)
        };
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
    /// A list that is not retained ([`DisplayList::is_retained`]) draws by
    /// calling [`Page::render`].
    #[must_use]
    pub fn render(&self, options: &RenderOptions) -> Bitmap {
        if !self.retained {
            return self.page.render(options);
        }
        let resources = self
            .resources
            .for_one_render_counted(crate::stats::tally(options.stats));
        self.page
            .render_layer_with(options, None, &resources, |renderer, resources| {
                let replayed = replay(&self.content, renderer);
                // What the interpretation could not resolve, reported by a
                // replay that ran to the end. See the module documentation for
                // why a cancelled one reports none of it.
                if !replayed.cancelled {
                    resources.note_missing_fonts(&self.interpreted_missing);
                    resources.note_repaired_spaces(&self.interpreted_repairs);
                }
                if options.annotations {
                    for annotation in &self.annotations {
                        renderer.push_resources(resources.bind(&annotation.scope));
                        replay(&annotation.events, renderer);
                        renderer.pop_resources();
                    }
                }
            })
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
    use tinker_pdf_content::{
        BlackPointCompensation, BlendMode, Event, GraphicsState, RenderingIntent, StoredParameters,
    };

    use crate::Document;

    /// A document whose pages share one indirect `/Resources` (object 3)
    /// holding `states` as its `/ExtGState`, and a transparency group form
    /// (object 4) a soft mask can name. Page `i` is 40 x 40 and draws
    /// `contents[i]`.
    fn document(states: &str, contents: &[&str]) -> Vec<u8> {
        let kids: Vec<String> = (0..contents.len())
            .map(|i| format!("{} 0 R", 5 + 2 * i))
            .collect();
        let mask = "1 g 0 0 20 40 re f";
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!(
                "<< /Type /Pages /Count {} /Kids [{}] >>",
                contents.len(),
                kids.join(" ")
            ),
            format!("<< /ExtGState << {states} >> >>"),
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] \
                 /Group << /S /Transparency /CS /DeviceGray >> /Length {} >>\n\
                 stream\n{mask}\nendstream",
                mask.len()
            ),
        ];
        for (i, content) in contents.iter().enumerate() {
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Resources 3 0 R \
                 /Contents {} 0 R >>",
                6 + 2 * i
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
        let doc = Document::open(document(states, contents)).expect("the document opens");
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
}
