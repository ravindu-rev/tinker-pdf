//! The typed annotation model (12.5), behind [`Page::annotations`].
//!
//! [`Page::annotations`]: crate::Page::annotations
//!
//! `annots.rs` beside this one is the *drawing* half — it runs an
//! annotation's `/AP` through the content pipeline. This is the *reading*
//! half: what the annotation says, in types a caller can name.
//!
//! The projection boundary is argued in [`crate::fontlist`]. Here it lands
//! on the owned side for a third reason, different from the other two: there
//! is no internal type to project. `cos::dest::links` returns `Link`, which
//! is navigation and not annotation — a `/Rect` and a resolved target, with
//! every other subtype's entries absent by design — and `cos::form::Field` is
//! the *form* view of a widget, keyed by field name rather than by page. Both
//! stay exactly as they are; [`Page::annotations`] is the third view and it
//! is the one that answers "what is on this page".
//!
//! # What "refused" means on a read surface
//!
//! Nothing here is refused, and that is a deliberate answer to ruling 10
//! rather than an absence of one. A leniency must name what it touched; the
//! cheapest way to satisfy that for a list is to make the list **total**:
//!
//! > `page.annotations()` has exactly as many entries as the page's
//! > `/Annots` array, up to the [`MAX_ANNOTS`] entries ruling 1 bounds it to.
//!
//! Every entry comes back. What the model could not type, it says so by
//! [`AnnotationKind`]:
//!
//! * [`AnnotationKind::Other`] — a `/Subtype` no edition of ISO 32000
//!   defines, carrying the name the file used. This is the "refused ones
//!   counted **by subtype**" the roadmap row asks for: the name is what makes
//!   the count actionable, and dropping the entry would make the count
//!   impossible.
//! * [`AnnotationKind::Unnamed`] — a dictionary with no `/Subtype` at all.
//! * [`AnnotationKind::Unreadable`] — an `/Annots` entry that is not a
//!   dictionary: a number, a null, a reference to a free object.
//!
//! A caller that wants only the annotations it understands filters; a caller
//! auditing a file counts. Neither has to guess what went missing, because
//! nothing does.
//!
//! # The subtype table
//!
//! [`AnnotationKind`]'s covered variants are ISO 32000-1 Table 169's
//! twenty-six subtypes plus the two ISO 32000-2 adds. The table is
//! transcribed in [`AnnotationKind::from_name`] with the clause that defines
//! each, and it agrees name-for-name with the independent transcription in
//! `pdfa::annotations`, which was made for a different purpose (ISO 19005's
//! exclusions) from the same table.
//!
//! # The one trap
//!
//! 12.5.6.14 Table 183, on a pop-up's `/Parent`: *"the parent annotation's
//! `Contents`, `M`, `C` and `T` entries shall override those of the pop-up
//! annotation itself."* It runs **from the parent into the pop-up**, and only
//! that way. The mirror-image mistake — a markup annotation reading its
//! `/Contents` through its `/Popup` — is not in the specification at all and
//! would report a note's text as whatever its pop-up window happened to
//! carry, usually nothing. Both directions have a test.

use std::sync::Arc;

use tinker_pdf_cos::{decode_text_string, parse_date, CosDocument, Date, Dict, ObjRef, Object};

/// How many `/Annots` entries one page reports.
///
/// Ruling 1: a hostile `/Annots` must not be able to make this allocate
/// without limit. Four thousand is past any real page — the largest in the
/// corpus census is 122, three orders of magnitude below — so nothing a
/// producer writes is truncated by it.
///
/// **Past the cap the list is shortened and says nothing.** There is no
/// warning, and that is forced rather than chosen: a warning would have to go
/// on [`crate::Document::warnings`], and appending to it here would make a
/// read change what the document reports about itself, which is the module
/// comment's second argument against building this on `cos::font::read`. The
/// two honest options left are to say so — which this comment, the method's
/// own documentation and the roadmap row all now do — and to pin the bound so
/// it cannot quietly go away: `a_hostile_annots_array_is_capped` does that,
/// and before it existed raising this constant to `usize::MAX` passed every
/// other test in the crate.
const MAX_ANNOTS: usize = 4096;

/// How far a pop-up's `/Parent` chain is followed before it is abandoned.
///
/// One step is the whole of 12.5.6.14: a pop-up's parent is a markup
/// annotation, and a markup annotation is not a pop-up. The bound exists for
/// the file that disagrees.
const MAX_PARENT_HOPS: u32 = 4;

/// Which of 12.5.6's subtypes an annotation is.
///
/// The covered variants are ISO 32000-1 Table 169 and ISO 32000-2's two
/// additions. The last three are how the model says what it could not type;
/// see the module comment.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnnotationKind {
    /// `/Text`, a sticky note (12.5.6.4).
    Text,
    /// `/Link`, a hypertext link (12.5.6.5). Also read by
    /// [`crate::Page::links`], which resolves its target.
    Link,
    /// `/FreeText`, text drawn on the page itself (12.5.6.6).
    FreeText,
    /// `/Line` (12.5.6.7).
    Line,
    /// `/Square` (12.5.6.8).
    Square,
    /// `/Circle` (12.5.6.8).
    Circle,
    /// `/Polygon` (12.5.6.9).
    Polygon,
    /// `/PolyLine` (12.5.6.9).
    PolyLine,
    /// `/Highlight`, a text markup annotation (12.5.6.10).
    Highlight,
    /// `/Underline`, a text markup annotation (12.5.6.10).
    Underline,
    /// `/Squiggly`, a text markup annotation (12.5.6.10).
    Squiggly,
    /// `/StrikeOut`, a text markup annotation (12.5.6.10).
    StrikeOut,
    /// `/Caret`, an insertion point (12.5.6.11).
    Caret,
    /// `/Stamp`, a rubber stamp (12.5.6.12).
    Stamp,
    /// `/Ink`, a freehand scrawl (12.5.6.13).
    Ink,
    /// `/Popup`, the window a markup annotation opens (12.5.6.14).
    Popup,
    /// `/FileAttachment` (12.5.6.15).
    FileAttachment,
    /// `/Sound` (12.5.6.16).
    Sound,
    /// `/Movie` (12.5.6.17).
    Movie,
    /// `/Screen`, a media clip's playing surface (12.5.6.18).
    Screen,
    /// `/Widget`, a form field's appearance on a page (12.5.6.19).
    Widget,
    /// `/PrinterMark`, a mark added at print time (12.5.6.20).
    PrinterMark,
    /// `/TrapNet`, a trap network (12.5.6.21).
    TrapNet,
    /// `/Watermark` (12.5.6.22).
    Watermark,
    /// `/Redact`, a redaction to be applied (12.5.6.23).
    Redact,
    /// `/3D`, a three-dimensional artwork (13.6.2, listed in Table 169).
    ///
    /// Spelled `ThreeD` because `3D` is not a Rust identifier;
    /// [`AnnotationKind::as_name`] gives back the `/Subtype` spelling.
    ThreeD,
    /// `/Projection`, added by ISO 32000-2 (12.5.6.24).
    Projection,
    /// `/RichMedia`, added by ISO 32000-2 (13.7).
    RichMedia,
    /// A `/Subtype` no edition of ISO 32000 defines, named as the file wrote
    /// it.
    ///
    /// Named rather than dropped: this is what makes "the refused ones
    /// counted by subtype" answerable.
    Other(String),
    /// A dictionary with no `/Subtype` (12.5.2 Table 164 requires one).
    Unnamed,
    /// An `/Annots` entry that is not a dictionary at all.
    Unreadable,
}

impl AnnotationKind {
    /// The `/Subtype` name, as a file would spell it.
    ///
    /// `""` for [`Self::Unnamed`] and [`Self::Unreadable`], which had no name
    /// to give back.
    #[must_use]
    pub fn as_name(&self) -> &str {
        match self {
            AnnotationKind::Text => "Text",
            AnnotationKind::Link => "Link",
            AnnotationKind::FreeText => "FreeText",
            AnnotationKind::Line => "Line",
            AnnotationKind::Square => "Square",
            AnnotationKind::Circle => "Circle",
            AnnotationKind::Polygon => "Polygon",
            AnnotationKind::PolyLine => "PolyLine",
            AnnotationKind::Highlight => "Highlight",
            AnnotationKind::Underline => "Underline",
            AnnotationKind::Squiggly => "Squiggly",
            AnnotationKind::StrikeOut => "StrikeOut",
            AnnotationKind::Caret => "Caret",
            AnnotationKind::Stamp => "Stamp",
            AnnotationKind::Ink => "Ink",
            AnnotationKind::Popup => "Popup",
            AnnotationKind::FileAttachment => "FileAttachment",
            AnnotationKind::Sound => "Sound",
            AnnotationKind::Movie => "Movie",
            AnnotationKind::Screen => "Screen",
            AnnotationKind::Widget => "Widget",
            AnnotationKind::PrinterMark => "PrinterMark",
            AnnotationKind::TrapNet => "TrapNet",
            AnnotationKind::Watermark => "Watermark",
            AnnotationKind::Redact => "Redact",
            AnnotationKind::ThreeD => "3D",
            AnnotationKind::Projection => "Projection",
            AnnotationKind::RichMedia => "RichMedia",
            AnnotationKind::Other(name) => name,
            AnnotationKind::Unnamed | AnnotationKind::Unreadable => "",
        }
    }

    /// Whether the model covers this subtype, rather than merely naming it.
    ///
    /// The word the roadmap row uses for the complement is *refused*, and the
    /// three variants this returns `false` for are exactly the three the
    /// census counts under that heading.
    #[must_use]
    pub fn is_covered(&self) -> bool {
        !matches!(
            self,
            AnnotationKind::Other(_) | AnnotationKind::Unnamed | AnnotationKind::Unreadable
        )
    }

    /// Whether 12.5.6.2 makes this a *markup* annotation.
    ///
    /// Markup annotations are the ones that carry `/T`, `/Popup`, `/CA`,
    /// `/RC`, `/CreationDate` and `/Subj` (Table 170). The list is the
    /// clause's own and is not derived from anything: a widget has a `/T`
    /// too, and it is the *field's* partial name, which is why a widget is
    /// not on it.
    #[must_use]
    pub fn is_markup(&self) -> bool {
        matches!(
            self,
            AnnotationKind::Text
                | AnnotationKind::FreeText
                | AnnotationKind::Line
                | AnnotationKind::Square
                | AnnotationKind::Circle
                | AnnotationKind::Polygon
                | AnnotationKind::PolyLine
                | AnnotationKind::Highlight
                | AnnotationKind::Underline
                | AnnotationKind::Squiggly
                | AnnotationKind::StrikeOut
                | AnnotationKind::Caret
                | AnnotationKind::Stamp
                | AnnotationKind::Ink
                | AnnotationKind::FileAttachment
                | AnnotationKind::Sound
                | AnnotationKind::Redact
        )
    }

    /// ISO 32000-1 Table 169's subtypes, plus ISO 32000-2's two.
    ///
    /// Transcribed from the table with the clause that defines each. The
    /// spellings are case-sensitive: 7.3.5 makes a name's bytes its identity,
    /// so `/widget` is not `/Widget` and is reported as
    /// [`AnnotationKind::Other`].
    fn from_name(name: &[u8]) -> AnnotationKind {
        match name {
            b"Text" => AnnotationKind::Text,                     // 12.5.6.4
            b"Link" => AnnotationKind::Link,                     // 12.5.6.5
            b"FreeText" => AnnotationKind::FreeText,             // 12.5.6.6
            b"Line" => AnnotationKind::Line,                     // 12.5.6.7
            b"Square" => AnnotationKind::Square,                 // 12.5.6.8
            b"Circle" => AnnotationKind::Circle,                 // 12.5.6.8
            b"Polygon" => AnnotationKind::Polygon,               // 12.5.6.9
            b"PolyLine" => AnnotationKind::PolyLine,             // 12.5.6.9
            b"Highlight" => AnnotationKind::Highlight,           // 12.5.6.10
            b"Underline" => AnnotationKind::Underline,           // 12.5.6.10
            b"Squiggly" => AnnotationKind::Squiggly,             // 12.5.6.10
            b"StrikeOut" => AnnotationKind::StrikeOut,           // 12.5.6.10
            b"Caret" => AnnotationKind::Caret,                   // 12.5.6.11
            b"Stamp" => AnnotationKind::Stamp,                   // 12.5.6.12
            b"Ink" => AnnotationKind::Ink,                       // 12.5.6.13
            b"Popup" => AnnotationKind::Popup,                   // 12.5.6.14
            b"FileAttachment" => AnnotationKind::FileAttachment, // 12.5.6.15
            b"Sound" => AnnotationKind::Sound,                   // 12.5.6.16
            b"Movie" => AnnotationKind::Movie,                   // 12.5.6.17
            b"Screen" => AnnotationKind::Screen,                 // 12.5.6.18
            b"Widget" => AnnotationKind::Widget,                 // 12.5.6.19
            b"PrinterMark" => AnnotationKind::PrinterMark,       // 12.5.6.20
            b"TrapNet" => AnnotationKind::TrapNet,               // 12.5.6.21
            b"Watermark" => AnnotationKind::Watermark,           // 12.5.6.22
            b"Redact" => AnnotationKind::Redact,                 // 12.5.6.23
            b"3D" => AnnotationKind::ThreeD,                     // 13.6.2
            // ISO 32000-2.
            b"Projection" => AnnotationKind::Projection, // 12.5.6.24
            b"RichMedia" => AnnotationKind::RichMedia,   // 13.7
            other => AnnotationKind::Other(String::from_utf8_lossy(other).into_owned()),
        }
    }
}

/// `/F`, the annotation flags (12.5.3, Table 165).
///
/// The raw integer with bit accessors, which is the shape
/// [`crate::Permissions`] already uses in this tree and for the same reason:
/// a file may set bits this build does not know, and a struct of booleans
/// loses them on the way through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnnotationFlags {
    raw: i64,
}

impl AnnotationFlags {
    /// Wraps an `/F` value.
    #[must_use]
    pub const fn from_raw(raw: i64) -> AnnotationFlags {
        AnnotationFlags { raw }
    }

    /// `/F` exactly as stored.
    #[must_use]
    pub const fn raw(self) -> i64 {
        self.raw
    }

    /// Bit `n`, counting from 1 as Table 165 does.
    const fn bit(self, n: u32) -> bool {
        self.raw & (1 << (n - 1)) != 0
    }

    /// Bit 1: do not render an annotation whose subtype this reader does not
    /// know.
    #[must_use]
    pub const fn invisible(self) -> bool {
        self.bit(1)
    }

    /// Bit 2: do not render or print this annotation at all.
    #[must_use]
    pub const fn hidden(self) -> bool {
        self.bit(2)
    }

    /// Bit 3: print this annotation.
    #[must_use]
    pub const fn print(self) -> bool {
        self.bit(3)
    }

    /// Bit 4: do not scale the annotation with the page.
    #[must_use]
    pub const fn no_zoom(self) -> bool {
        self.bit(4)
    }

    /// Bit 5: do not rotate the annotation with the page.
    #[must_use]
    pub const fn no_rotate(self) -> bool {
        self.bit(5)
    }

    /// Bit 6: do not display on screen, but print if bit 3 says so.
    #[must_use]
    pub const fn no_view(self) -> bool {
        self.bit(6)
    }

    /// Bit 7: no user interaction, which is not the same as `/Ff` read-only.
    #[must_use]
    pub const fn read_only(self) -> bool {
        self.bit(7)
    }

    /// Bit 8: the annotation's properties may not be edited.
    #[must_use]
    pub const fn locked(self) -> bool {
        self.bit(8)
    }

    /// Bit 9: toggle bit 6 when the annotation is selected.
    #[must_use]
    pub const fn toggle_no_view(self) -> bool {
        self.bit(9)
    }

    /// Bit 10: the annotation's contents may not be edited.
    #[must_use]
    pub const fn locked_contents(self) -> bool {
        self.bit(10)
    }
}

/// One entry of a page's `/Annots` array, read (12.5.2).
///
/// The entries every annotation dictionary may carry (Table 164), plus the
/// markup entries (Table 170) the roadmap row named. **No per-subtype
/// geometry**: `/QuadPoints`, `/InkList`, `/Vertices`, `/L` and their
/// relatives are one payload per family and belong to their own commit — see
/// the roadmap row this one left behind.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    /// The annotation dictionary's own reference, or `None` when `/Annots`
    /// wrote it inline.
    pub reference: Option<ObjRef>,
    /// Which of 12.5.6's subtypes this is, or how it could not be typed.
    pub kind: AnnotationKind,
    /// `/Rect` as `(x0, y0, x1, y1)`, ordered so `x0 <= x1` and `y0 <= y1`.
    ///
    /// All zeros when `/Rect` is absent or unreadable, which 12.5.2 makes a
    /// malformed annotation — reported as a degenerate rectangle rather than
    /// by dropping the entry, so the list stays total.
    pub rect: (f64, f64, f64, f64),
    /// `/Contents`: the text of a markup annotation, or the alternate
    /// description of one that has no text of its own (Table 164).
    ///
    /// For a pop-up whose `/Parent` is present this is the **parent's**
    /// `/Contents`, per 12.5.6.14.
    pub contents: Option<String>,
    /// `/T`: the markup annotation's author (Table 170).
    ///
    /// For a widget this is the field's partial name, which is why the field
    /// tree and not this is where a form is read from. Overridden by the
    /// parent for a pop-up, as `/Contents` is.
    pub title: Option<String>,
    /// `/M` exactly as the file wrote it, decoded as a text string.
    ///
    /// Text and not a date because Table 164 says so: `/M` *should* be a date
    /// string, and a reader "should be prepared to accept and display a
    /// string in any format". Carrying the text is the only lossless answer.
    /// Overridden by the parent for a pop-up.
    pub modified: Option<String>,
    /// `/M` parsed as a 7.9.4 date, when it is one.
    ///
    /// The convenience beside the fidelity. `None` where [`Self::modified`]
    /// is `Some` means the producer wrote something that is not a date, which
    /// is a fact about the file worth being able to see.
    pub modified_date: Option<Date>,
    /// `/F` (12.5.3, Table 165).
    pub flags: AnnotationFlags,
    /// `/Popup`: the pop-up window this markup annotation opens (Table 170).
    ///
    /// The *reference*, not the pop-up's contents. Reading `/Contents`
    /// through this entry is the mistake 12.5.6.14 does not licence; see the
    /// module comment.
    pub popup: Option<ObjRef>,
    /// `/Parent`: for a pop-up, the markup annotation it belongs to
    /// (12.5.6.14); for a widget, the field dictionary it inherits from.
    pub parent: Option<ObjRef>,
    /// Whether `/AP` carries a normal appearance (12.5.5).
    ///
    /// Whether one exists, not what it draws — drawing is `annots.rs`'s job.
    pub has_appearance: bool,
}

/// Reads one page's `/Annots`, in the array's own order.
///
/// The returned list is the same length as the array, capped at
/// [`MAX_ANNOTS`]; see the module comment for why nothing is dropped.
pub(crate) fn of_page(doc: &Arc<CosDocument>, page: ObjRef) -> Vec<Annotation> {
    let Ok(object) = doc.get(page) else {
        return Vec::new();
    };
    let Some(dict) = object.as_dict() else {
        return Vec::new();
    };
    let annots = doc.resolve_key(dict, doc.intern(b"Annots"));
    let Some(entries) = annots.as_array() else {
        return Vec::new();
    };

    entries
        .iter()
        .take(MAX_ANNOTS)
        .map(|entry| read_one(doc, entry))
        .collect()
}

fn read_one(doc: &CosDocument, entry: &Object) -> Annotation {
    let reference = entry.as_objref();
    let resolved = doc.resolve(entry);
    let Some(dict) = resolved.as_dict() else {
        return Annotation {
            reference,
            kind: AnnotationKind::Unreadable,
            rect: (0.0, 0.0, 0.0, 0.0),
            contents: None,
            title: None,
            modified: None,
            modified_date: None,
            flags: AnnotationFlags::default(),
            popup: None,
            parent: None,
            has_appearance: false,
        };
    };

    let kind = match doc
        .resolve_key(dict, doc.intern(b"Subtype"))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
    {
        Some(name) => AnnotationKind::from_name(&name),
        None => AnnotationKind::Unnamed,
    };

    // 12.5.6.14 Table 183: a pop-up takes /Contents, /M and /T from its
    // parent. **Only a pop-up, and only through /Parent.** The `/Popup` entry
    // below points the other way and is never followed for text.
    let text_source = match kind {
        AnnotationKind::Popup => parent_dict(doc, dict).unwrap_or_else(|| dict.clone()),
        _ => dict.clone(),
    };

    let modified = text_of(doc, &text_source, b"M");
    Annotation {
        reference,
        kind,
        rect: rect_of(doc, dict),
        contents: text_of(doc, &text_source, b"Contents"),
        title: text_of(doc, &text_source, b"T"),
        modified_date: modified.as_deref().and_then(parse_date),
        modified,
        flags: AnnotationFlags::from_raw(
            doc.resolve_key(dict, doc.intern(b"F"))
                .as_int()
                .unwrap_or(0),
        ),
        popup: dict.get_ref(doc.intern(b"Popup")),
        parent: dict.get_ref(doc.intern(b"Parent")),
        has_appearance: has_normal_appearance(doc, dict),
    }
}

/// The dictionary a pop-up's `/Parent` names, if it names one.
///
/// Bounded and cycle-guarded: a file whose pop-up is its own parent, or whose
/// parent chain runs in a circle, costs a few hops and then falls back to the
/// pop-up's own entries.
fn parent_dict(doc: &CosDocument, popup: &Dict) -> Option<Dict> {
    let mut seen: Vec<ObjRef> = Vec::new();
    let mut at = popup.get_ref(doc.intern(b"Parent"))?;
    for _ in 0..MAX_PARENT_HOPS {
        if seen.contains(&at) {
            return None;
        }
        seen.push(at);
        let object = doc.get(at).ok()?;
        let dict = object.as_dict()?;
        // The parent of a pop-up is a markup annotation, which is not itself
        // a pop-up. A chain of pop-ups is a malformation; following it to the
        // end is the lenient reading and it terminates either way.
        let is_popup = doc
            .resolve_key(dict, doc.intern(b"Subtype"))
            .as_name()
            .and_then(|n| doc.name_bytes(n))
            .is_some_and(|n| n.as_ref() == b"Popup");
        if !is_popup {
            return Some(dict.clone());
        }
        at = dict.get_ref(doc.intern(b"Parent"))?;
    }
    None
}

/// A text-string entry, decoded (7.9.2.2).
///
/// An entry that is present but empty comes back as `Some("")`: a file that
/// wrote an empty `/Contents` said something different from one that wrote
/// none, and collapsing the two would lose it.
fn text_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<String> {
    let value = doc.resolve_key(dict, doc.intern(key));
    let string = value.as_string()?;
    Some(decode_text_string(&string.bytes))
}

/// `/Rect`, ordered (12.5.2 Table 164).
///
/// The clause says the rectangle "shall" be written with the lower-left
/// corner first and immediately adds that a reader should normalise it, which
/// is the same instruction 7.9.5 gives for every rectangle. Producers do get
/// it backwards.
fn rect_of(doc: &CosDocument, dict: &Dict) -> (f64, f64, f64, f64) {
    let value = doc.resolve_key(dict, doc.intern(b"Rect"));
    let Some(items) = value.as_array() else {
        return (0.0, 0.0, 0.0, 0.0);
    };
    let numbers: Vec<f64> = items
        .iter()
        .map(|item| doc.resolve(item).as_number().unwrap_or(f64::NAN))
        .collect();
    match numbers.as_slice() {
        [a, b, c, d] if numbers.iter().all(|n| n.is_finite()) => {
            (a.min(*c), b.min(*d), a.max(*c), b.max(*d))
        }
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

/// Whether `/AP` has an `/N` entry, following a dictionary of states one
/// level (12.5.5).
fn has_normal_appearance(doc: &CosDocument, dict: &Dict) -> bool {
    let ap = doc.resolve_key(dict, doc.intern(b"AP"));
    let Some(ap) = ap.as_dict() else {
        return false;
    };
    let Some(normal) = ap.get(doc.intern(b"N")) else {
        return false;
    };
    let resolved = doc.resolve(normal);
    if resolved.as_stream().is_some() {
        return true;
    }
    // A dictionary of substates: a checkbox's on and off appearances.
    resolved.as_dict().is_some_and(|states| {
        states
            .iter()
            .any(|(_, v)| doc.resolve(v).as_stream().is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **ISO 32000-1 Table 169, transcribed a second time**, from the clause
    /// rather than from the code above, and compared against it.
    ///
    /// Twenty-six subtypes, plus the two ISO 32000-2 adds. The same
    /// twenty-six appear in `pdfa::annotations::STANDARD_TYPES`, transcribed
    /// independently for ISO 19005's exclusion lists; the two agreeing is the
    /// check the campaign's "transcribe every table twice" rule asks for.
    ///
    /// Adjudicated by the **standard's own table**, not by a round trip.
    #[test]
    fn every_subtype_the_standard_defines_is_covered() {
        let iso_32000_1 = [
            "Text",
            "Link",
            "FreeText",
            "Line",
            "Square",
            "Circle",
            "Polygon",
            "PolyLine",
            "Highlight",
            "Underline",
            "Squiggly",
            "StrikeOut",
            "Caret",
            "Stamp",
            "Ink",
            "Popup",
            "FileAttachment",
            "Sound",
            "Movie",
            "Screen",
            "Widget",
            "PrinterMark",
            "TrapNet",
            "Watermark",
            "3D",
            "Redact",
        ];
        assert_eq!(iso_32000_1.len(), 26, "Table 169 has twenty-six rows");

        let iso_32000_2 = ["Projection", "RichMedia"];

        for name in iso_32000_1.iter().chain(iso_32000_2.iter()) {
            let kind = AnnotationKind::from_name(name.as_bytes());
            assert!(kind.is_covered(), "/{name} is a subtype the model covers");
            assert_eq!(
                kind.as_name(),
                *name,
                "/{name} round-trips through the enum's own spelling"
            );
        }
    }

    /// A subtype no edition defines is **named**, not dropped, and reports
    /// itself as not covered.
    #[test]
    fn an_unknown_subtype_is_named_and_counted() {
        let kind = AnnotationKind::from_name(b"XfaWidget");
        assert_eq!(kind, AnnotationKind::Other("XfaWidget".to_string()));
        assert!(!kind.is_covered());
        assert_eq!(kind.as_name(), "XfaWidget");

        // 7.3.5: a name's identity is its bytes, so case matters.
        assert!(!AnnotationKind::from_name(b"widget").is_covered());
    }

    /// The list is total: as many entries out as `/Annots` had in, whatever
    /// was in them.
    ///
    /// Three entries, three answers, none of them dropped: a subtype the
    /// model does not cover, a dictionary with no subtype, and a reference to
    /// an object that is not there.
    #[test]
    fn nothing_in_annots_is_dropped() {
        let doc = page_with(
            "/Annots [20 0 R 21 0 R 99 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Fancy /Rect [0 0 1 1] >>\nendobj\n\
             21 0 obj\n<< /Type /Annot /Rect [0 0 1 1] >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots.len(), 3, "one entry out per entry in");
        assert_eq!(annots[0].kind, AnnotationKind::Other("Fancy".to_string()));
        assert_eq!(annots[1].kind, AnnotationKind::Unnamed);
        assert_eq!(annots[2].kind, AnnotationKind::Unreadable);
        assert!(annots.iter().all(|a| !a.kind.is_covered()));
    }

    /// The roadmap row's own exit criterion: `/Contents`, `/T` and `/M` come
    /// back.
    #[test]
    fn contents_title_and_modified_are_read() {
        let doc = page_with(
            "/Annots [20 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Text /Rect [10 20 30 40] \
             /Contents (a note) /T (Ada) /M (D:20260915120000Z) /F 4 >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots.len(), 1);
        let note = &annots[0];

        assert_eq!(note.kind, AnnotationKind::Text);
        assert!(note.kind.is_markup());
        assert_eq!(note.contents.as_deref(), Some("a note"));
        assert_eq!(note.title.as_deref(), Some("Ada"));
        assert_eq!(note.modified.as_deref(), Some("D:20260915120000Z"));
        assert_eq!(note.modified_date.expect("a date").year, 2026);
        assert_eq!(note.modified_date.expect("a date").month, 9);
        assert_eq!(note.rect, (10.0, 20.0, 30.0, 40.0));
        assert!(note.flags.print(), "/F 4 is bit 3, print");
        assert!(!note.flags.hidden());
    }

    /// **12.5.6.14, both directions.**
    ///
    /// The parent's `/Contents`, `/T` and `/M` override the pop-up's own; and
    /// the markup annotation's own `/Contents` is its own, never read through
    /// its `/Popup`.
    ///
    /// The second half is the injection: a reader that followed `/Popup` to
    /// find text would report the note's contents as `(the popup's own)` —
    /// which is why the fixture deliberately puts different text in both.
    #[test]
    fn a_popup_takes_its_text_from_its_parent_and_never_the_other_way() {
        let doc = page_with(
            "/Annots [20 0 R 21 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] \
             /Contents (the parent's text) /T (Ada) /M (D:20260101) /Popup 21 0 R >>\nendobj\n\
             21 0 obj\n<< /Type /Annot /Subtype /Popup /Rect [10 0 20 10] \
             /Contents (the popup's own) /T (Nobody) /M (D:19990101) /Parent 20 0 R >>\n\
             endobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots.len(), 2);

        let (note, popup) = (&annots[0], &annots[1]);
        assert_eq!(note.kind, AnnotationKind::Text);
        assert_eq!(popup.kind, AnnotationKind::Popup);

        // The markup annotation keeps its own text. Following /Popup would
        // give "the popup's own" here.
        assert_eq!(note.contents.as_deref(), Some("the parent's text"));
        assert_eq!(note.title.as_deref(), Some("Ada"));
        assert_eq!(note.modified.as_deref(), Some("D:20260101"));
        assert_eq!(note.popup.map(|r| r.num), Some(21));

        // The pop-up takes the parent's three entries, overriding its own.
        assert_eq!(popup.contents.as_deref(), Some("the parent's text"));
        assert_eq!(popup.title.as_deref(), Some("Ada"));
        assert_eq!(popup.modified.as_deref(), Some("D:20260101"));
        assert_eq!(popup.parent.map(|r| r.num), Some(20));
    }

    /// A pop-up with no `/Parent` keeps its own entries: the override is
    /// conditional on the entry being there.
    #[test]
    fn a_parentless_popup_keeps_its_own_text() {
        let doc = page_with(
            "/Annots [21 0 R]",
            "21 0 obj\n<< /Type /Annot /Subtype /Popup /Rect [0 0 10 10] \
             /Contents (orphaned) >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots[0].contents.as_deref(), Some("orphaned"));
    }

    /// A pop-up whose parent chain runs in a circle terminates (ruling 1).
    #[test]
    fn a_popup_parent_cycle_terminates() {
        let doc = page_with(
            "/Annots [20 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Popup /Rect [0 0 10 10] \
             /Contents (mine) /Parent 21 0 R >>\nendobj\n\
             21 0 obj\n<< /Type /Annot /Subtype /Popup /Parent 20 0 R >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].contents.as_deref(), Some("mine"));
    }

    /// A widget is one annotation among the rest, not the only one that comes
    /// back.
    ///
    /// Before this model the page-level view was `links()` and the form
    /// field tree, so a `/Square` on a page with a form was invisible from
    /// the facade. That is the injection `annotations_are_not_only_widgets`
    /// in `tests/facade_read.rs` re-creates.
    #[test]
    fn widgets_are_listed_beside_every_other_subtype() {
        let doc = page_with(
            "/Annots [20 0 R 21 0 R 22 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Widget /Rect [0 0 1 1] /T (field) >>\nendobj\n\
             21 0 obj\n<< /Type /Annot /Subtype /Square /Rect [0 0 1 1] >>\nendobj\n\
             22 0 obj\n<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] >>\nendobj\n",
        );
        let kinds: Vec<AnnotationKind> = doc
            .page(0)
            .expect("a page")
            .annotations()
            .into_iter()
            .map(|a| a.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                AnnotationKind::Widget,
                AnnotationKind::Square,
                AnnotationKind::Link
            ]
        );
    }

    /// 12.5.2 / 7.9.5: a rectangle written the wrong way round is normalised
    /// rather than reported as negative.
    #[test]
    fn a_reversed_rectangle_is_ordered() {
        let doc = page_with(
            "/Annots [20 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Square /Rect [30 40 10 20] >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots[0].rect, (10.0, 20.0, 30.0, 40.0));
    }

    /// `/M` that is not a date is carried as text, and says so by parsing to
    /// `None`.
    #[test]
    fn a_modified_entry_that_is_not_a_date_is_still_text() {
        let doc = page_with(
            "/Annots [20 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Text /Rect [0 0 1 1] \
             /M (last Tuesday) >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots[0].modified.as_deref(), Some("last Tuesday"));
        assert_eq!(annots[0].modified_date, None);
    }

    /// 12.5.3 Table 165, bit by bit.
    #[test]
    fn the_flag_bits_are_table_165s() {
        let hidden = AnnotationFlags::from_raw(2);
        assert!(hidden.hidden() && !hidden.print() && !hidden.invisible());

        let print_no_view = AnnotationFlags::from_raw(4 | 32);
        assert!(print_no_view.print() && print_no_view.no_view());
        assert!(!print_no_view.hidden());

        let locked = AnnotationFlags::from_raw(1 << 9);
        assert!(locked.locked_contents(), "bit 10");
        assert_eq!(locked.raw(), 512, "the raw value survives");
    }

    /// Ruling 1: a hostile `/Annots` cannot make the list grow without bound.
    ///
    /// **This fixture exists because its absence was a hole.** Raising
    /// [`MAX_ANNOTS`] to `usize::MAX` — the whole of ruling 1's bound on this
    /// path, gone — failed nothing in the crate: every other fixture here
    /// carries five annotations or fewer, and none of them is large enough to
    /// reach the cap. The census cannot reach it either, the corpus's largest
    /// page being 122.
    ///
    /// It pins the truncation as well as the bound. The entries past the cap
    /// are dropped and nothing says so — see [`MAX_ANNOTS`] for why a warning
    /// is not available to a read surface — so this is the only place that
    /// records what a page over the cap actually gets back.
    ///
    /// **The sizes are literals on purpose.** A fixture sized from the
    /// constant it is meant to pin grows along with it: written as
    /// `repeat(MAX_ANNOTS + 1)` and `assert_eq!(len, MAX_ANNOTS)` this test
    /// passed with the cap raised to a hundred thousand, because the array
    /// grew to a hundred thousand and one. The bound's *value* is the
    /// contract, so it is spelled out and the constant is checked against it.
    #[test]
    fn a_hostile_annots_array_is_capped() {
        let entries = "<< /Subtype /Square >> ".repeat(4097);
        let doc = page_with(&format!("/Annots [{entries}]"), "");
        let annotations = doc.page(0).expect("a page").annotations();

        assert_eq!(
            annotations.len(),
            4096,
            "4 097 entries went in and ruling 1's bound is 4 096"
        );
        assert_eq!(MAX_ANNOTS, 4096, "and that bound is this constant");
        assert!(
            annotations.iter().all(|a| a.kind == AnnotationKind::Square),
            "the entries kept are the array's own, read normally"
        );
    }

    /// A page with no `/Annots` has no annotations, which is an ordinary
    /// answer and not an error.
    #[test]
    fn a_page_without_annots_has_none() {
        let doc = page_with("", "");
        assert!(doc.page(0).expect("a page").annotations().is_empty());
    }

    fn page_with(page_extra: &str, objects: &str) -> crate::Document {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] {page_extra} >>\nendobj\n\
{objects}\
trailer\n<< /Size 60 /Root 1 0 R >>\n%%EOF\n"
        );
        crate::Document::open(bytes.into_bytes()).expect("it opens")
    }
}
