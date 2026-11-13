//! One part of a page rendered on its own: a form XObject the page names, or
//! one of its annotations.
//!
//! # Through the page's own pipeline, and nothing beside it
//!
//! Ruling 5 is about tiles, and what it says generalises: a render of part of
//! a page is the page's pipeline with less in it, never a second
//! implementation. So both entry points here go through `Page::render_layer`,
//! which is the whole of what `Page::render` does — the same scale clamp, the
//! same view transform, the same canvas, the same `Renderer`, the same warnings
//! and the same conversion at the end — and differ from a page only in *what
//! is painted* and *which rectangle of the page is the default viewport*:
//!
//! - a form is painted by interpreting `/Name Do` with the identity matrix, so
//!   the interpreter reaches the form exactly as the page's own content does —
//!   `/Matrix`, the `/BBox` clip, a transparency group, `/OC`, its own
//!   `/Resources` — and the viewport is the form's bounding box on the page;
//! - an annotation is painted by the code `Page::render` draws every
//!   annotation with (`annots.rs`), and the viewport is its `/Rect`.
//!
//! Which is why each is **byte-equal to its rectangle of a page that draws it
//! and nothing else there**: `render_parts.rs` holds both to that.
//!
//! # How a part is named
//!
//! A form by its **resource name in the page's `/XObject` dictionary** — the
//! name the page's own content would `Do` it by, and the only name a form has
//! that means "as this page places it". A form reachable only from inside
//! another form's resources has no placement on the page to render it at, so
//! it is not addressable here. An annotation by its **index in `/Annots`**,
//! which is the index [`Page::annotations`] reports it at: `Annotation`'s
//! `reference` is `None` for a direct dictionary, so the index is the one
//! handle every entry has.
//!
//! # What comes back when a part cannot be drawn
//!
//! A typed [`RenderPartError`] rather than a blank bitmap. A page renders
//! whatever it can because a reader wants the page (ruling 2); a caller asking
//! for one part asked a question about that part, and a white rectangle is a
//! wrong answer that looks like a right one. Every reason `Page::render` has
//! for silently drawing nothing — a hidden flag, a pop-up, no appearance, a
//! collapsed box — is a named [`NotDrawn`] here.

use tinker_pdf_content::{interpret, FontSource, Matrix};
use tinker_pdf_cos::Object;

use crate::annotations::MAX_ANNOTS;
use crate::{annots, Bitmap, Page, RenderOptions};

/// Why [`Page::render_form`] or [`Page::render_annotation`] drew nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderPartError {
    /// The page's `/Resources` have no `/XObject` of this name.
    NoSuchXObject {
        /// The name asked for, as text (lossily, if it was not UTF-8).
        name: String,
    },
    /// The XObject is not a form: an image, a PostScript XObject, or a stream
    /// with no `/Subtype` (8.8, Table 87).
    NotAForm {
        /// The name asked for.
        name: String,
        /// Its `/Subtype`, or `None` when it has none.
        subtype: Option<String>,
    },
    /// A form whose stream could not be read.
    UnreadableForm {
        /// The name asked for.
        name: String,
    },
    /// There is no entry at this index of `/Annots`.
    NoSuchAnnotation {
        /// The index asked for.
        index: usize,
        /// How many entries [`Page::annotations`] reports.
        count: usize,
    },
    /// The annotation exists and has nothing to draw.
    AnnotationNotDrawn {
        /// The index asked for.
        index: usize,
        /// Why.
        why: NotDrawn,
    },
}

/// Why an annotation that exists draws nothing — each a reason `Page::render`
/// skips one silently, named here because a caller asked for this one.
///
/// The flags and an annotation's `/OC` are read for the render's usage
/// ([`crate::RenderOptions::usage`], `docs/design/render-usage.md`): `Hidden`
/// in every usage, `Invisible` on a subtype ISO 32000 does not define in every
/// usage, `NotPrinted` in a print render, `NoView` in any other, and
/// `OptionalContentOff` under the states the render's usage gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotDrawn {
    /// The `/Annots` entry is not a dictionary.
    NotADictionary,
    /// `/F` sets Hidden (12.5.3): never drawn, in any usage.
    Hidden,
    /// `/F` sets Invisible on an annotation whose `/Subtype` is none ISO
    /// 32000 defines, or that has none (12.5.3): an unknown annotation,
    /// drawn in no usage.
    Invisible,
    /// `/F` sets NoView in a render for the screen — no usage, view or
    /// export (12.5.3); a print render draws it where `/F` sets Print.
    NoView,
    /// `/F` does not set Print, in a print render (12.5.3).
    NotPrinted,
    /// The annotation's `/OC` is off under the render's optional-content
    /// states (12.5.2 Table 166, 8.11.3.3).
    OptionalContentOff,
    /// A `/Popup`: the window its parent opens, not a mark on the page.
    Popup,
    /// No `/Rect` that is four finite numbers enclosing some area.
    NoRect,
    /// No normal appearance: no `/AP /N`, or an `/AS` naming a state the
    /// appearance dictionary does not have (12.5.5).
    NoAppearance,
    /// An appearance stream that could not be decoded.
    UnreadableAppearance,
    /// A `/BBox` and `/Matrix` that collapse to nothing, so there is no box to
    /// fit onto `/Rect` (12.5.5).
    Degenerate,
}

impl core::fmt::Display for NotDrawn {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            NotDrawn::NotADictionary => "the entry is not a dictionary",
            NotDrawn::Hidden => "its Hidden flag hides it",
            NotDrawn::Invisible => "its Invisible flag hides an annotation of an unknown type",
            NotDrawn::NoView => "its NoView flag keeps it off the screen",
            NotDrawn::NotPrinted => "its Print flag is clear and the render is for print",
            NotDrawn::OptionalContentOff => "its optional content is off",
            NotDrawn::Popup => "it is a pop-up window",
            NotDrawn::NoRect => "it has no usable /Rect",
            NotDrawn::NoAppearance => "it has no normal appearance",
            NotDrawn::UnreadableAppearance => "its appearance stream cannot be read",
            NotDrawn::Degenerate => "its appearance box collapses to nothing",
        })
    }
}

impl core::fmt::Display for RenderPartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RenderPartError::NoSuchXObject { name } => {
                write!(f, "the page has no XObject named /{name}")
            }
            RenderPartError::NotAForm { name, subtype } => match subtype {
                Some(subtype) => write!(f, "/{name} is a /{subtype} XObject, not a form"),
                None => write!(f, "/{name} has no /Subtype, so it is not a form"),
            },
            RenderPartError::UnreadableForm { name } => {
                write!(f, "the form /{name} could not be read")
            }
            RenderPartError::NoSuchAnnotation { index, count } => {
                write!(f, "no annotation at index {index}: the page has {count}")
            }
            RenderPartError::AnnotationNotDrawn { index, why } => {
                write!(f, "annotation {index} draws nothing: {why}")
            }
        }
    }
}

impl std::error::Error for RenderPartError {}

impl Page {
    /// One form XObject the page names, rendered on its own, where the page
    /// places it.
    ///
    /// `name` is the form's resource name in the page's `/XObject` dictionary,
    /// without the slash and with any `#xx` escapes already decoded — `b"Fm0"`
    /// for `/Fm0`. The form is drawn with the identity matrix, which is where a
    /// page that says `/Fm0 Do` with no `cm` in front of it draws it: its own
    /// `/Matrix`, its `/BBox` clip, a transparency group and `/OC` all apply,
    /// because the page's interpreter is what draws it.
    ///
    /// The bitmap is the form's bounding box on the page — `/BBox` through
    /// `/Matrix`, in the page's pixels, rounded outward and trimmed to the page
    /// — unless `options.region` names a rectangle, which then wins exactly as
    /// it does for [`Page::render`]. A form with no `/BBox` gets the whole page.
    /// Everything else in `options` means what it means for a page;
    /// `annotations` has nothing to draw.
    ///
    /// **It is byte-equal to the same rectangle of the page** when the page
    /// draws that form there and nothing else, which is the test that holds it.
    ///
    /// # Errors
    ///
    /// [`RenderPartError::NoSuchXObject`], [`RenderPartError::NotAForm`] with
    /// its `/Subtype`, or [`RenderPartError::UnreadableForm`].
    pub fn render_form(
        &self,
        name: &[u8],
        options: &RenderOptions,
    ) -> Result<Bitmap, RenderPartError> {
        let text = || String::from_utf8_lossy(name).into_owned();
        // One operation for the whole door, this scope's reading of the name
        // included: it is work the door does before the render starts, and
        // the form it reads here is the one the render's `Do` draws.
        let op = self.render_operation(options, options.usage);
        let resources = crate::resources::PageResources::new_in(
            &self.doc,
            &self.inner,
            self.fonts.as_ref(),
            op.clone(),
        );
        let subtype = resources
            .xobject_subtype(name)
            .ok_or_else(|| RenderPartError::NoSuchXObject { name: text() })?;
        if subtype.as_deref() != Some(b"Form".as_slice()) {
            return Err(RenderPartError::NotAForm {
                name: text(),
                subtype: subtype.map(|s| String::from_utf8_lossy(&s).into_owned()),
            });
        }
        let form = resources
            .form(name)
            .ok_or_else(|| RenderPartError::UnreadableForm { name: text() })?;

        // `/BBox` in form space, through `/Matrix` into the page's default
        // space — which is user space here, the form being placed with the
        // identity — as the axis-aligned rectangle of its four corners.
        let frame = form.bbox.map(|[x0, y0, x1, y1]| {
            bounds([(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| form.matrix.apply(x, y)))
        });

        let mut content = b"/".to_vec();
        content.extend_from_slice(&escaped(name));
        content.extend_from_slice(b" Do");
        Ok(
            self.render_layer(options, frame, op, |renderer, resources| {
                interpret(&content, Matrix::IDENTITY, renderer, resources);
            }),
        )
    }

    /// One of the page's annotations, rendered on its own over the page's
    /// white.
    ///
    /// `index` is its position in `/Annots`, which is its position in
    /// [`Page::annotations`]. It is drawn by the code [`Page::render`] draws
    /// every annotation with — its flags and `/OC` read for `options.usage`
    /// (12.5.3), 12.5.5's fit of the normal appearance onto `/Rect`, the
    /// `/BBox` clip, the appearance's own resources, and `/NoZoom` and
    /// `/NoRotate` placed for `options.magnification` and the page's `/Rotate`
    /// — and the bitmap is `/Rect`, placed as the appearance is, in the
    /// page's pixels, rounded outward and trimmed to the page, unless
    /// `options.region` names a rectangle. `options.annotations`, `widgets`,
    /// `other_annotations` and `content` are not consulted: asking for one
    /// annotation is asking for it drawn alone, where the usage draws it.
    ///
    /// **It is byte-equal to the same rectangle of the page rendered with
    /// annotations on**, where the page draws nothing else there.
    ///
    /// # Errors
    ///
    /// [`RenderPartError::NoSuchAnnotation`] for an index past the list, and
    /// [`RenderPartError::AnnotationNotDrawn`] with the [`NotDrawn`] reason for
    /// an entry that exists and draws nothing.
    pub fn render_annotation(
        &self,
        index: usize,
        options: &RenderOptions,
    ) -> Result<Bitmap, RenderPartError> {
        let entries = self.annots_entries();
        let count = entries.len().min(MAX_ANNOTS);
        let entry = entries
            .get(index)
            .filter(|_| index < count)
            .ok_or(RenderPartError::NoSuchAnnotation { index, count })?;
        let not_drawn = |why| RenderPartError::AnnotationNotDrawn { index, why };

        let dict = match entry {
            Object::Ref(r) => self.doc.get(*r).ok().and_then(|o| o.as_dict().cloned()),
            Object::Dict(d) => Some(d.clone()),
            _ => None,
        }
        .ok_or(not_drawn(NotDrawn::NotADictionary))?;

        // The operation is made before the appearance is decoded, so the
        // decode counts too.
        let op = self.render_operation(options, options.usage);
        let appearance = annots::prepare(&self.doc, &dict, &op).map_err(not_drawn)?;
        // 12.5.3's `/NoZoom` and `/NoRotate` move the box with the appearance.
        let placement = annots::Placement::new(options.magnification, self.inner.rotation);
        let rect = appearance.placed_rect(placement);
        Ok(
            self.render_layer(options, Some(rect), op, |renderer, resources| {
                annots::draw_prepared(
                    &self.doc,
                    &appearance,
                    self.fonts.as_ref(),
                    renderer,
                    resources.operation(),
                    placement,
                );
            }),
        )
    }

    /// The page's `/Annots` array, resolved, or nothing.
    fn annots_entries(&self) -> Vec<Object> {
        let Ok(object) = self.doc.get(self.inner.reference) else {
            return Vec::new();
        };
        let Some(dict) = object.as_dict() else {
            return Vec::new();
        };
        self.doc
            .resolve_key(dict, self.doc.intern(b"Annots"))
            .as_array()
            .map(<[Object]>::to_vec)
            .unwrap_or_default()
    }
}

/// The axis-aligned rectangle of four points, as `(x0, y0, x1, y1)`.
fn bounds(corners: [(f64, f64); 4]) -> (f64, f64, f64, f64) {
    let mut out = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (x, y) in corners {
        out = (out.0.min(x), out.1.min(y), out.2.max(x), out.3.max(y));
    }
    out
}

/// A name's bytes as a content stream spells them (7.3.5): regular characters
/// as they are, and everything else — white space, a delimiter, `#` itself,
/// and any byte outside `!`..`~` — as `#` and two hex digits, so that the name
/// the interpreter reads back is the name the caller gave.
fn escaped(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    for &byte in name {
        let regular = (0x21..=0x7E).contains(&byte)
            && !matches!(
                byte,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
            );
        if regular {
            out.push(byte);
        } else {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_escaped_so_the_interpreter_reads_it_back() {
        assert_eq!(escaped(b"Fm0"), b"Fm0");
        assert_eq!(escaped(b"a b"), b"a#20b");
        assert_eq!(escaped(b"x#y/z"), b"x#23y#2Fz");
        assert_eq!(escaped(&[0x00, 0xFF]), b"#00#FF");
    }
}
