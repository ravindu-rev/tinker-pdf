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
