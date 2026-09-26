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

use tinker_pdf_cos::{parse_date, CosDocument, Date, Dict, ObjRef, Object};

mod payload;

use payload::Read;
pub use payload::{AnnotationPayload, BorderEffect, FileSpec, Linked, MAX_ANNOTATION_BYTES};

/// How many `/Annots` entries one page reports.
///
/// Ruling 1: a hostile `/Annots` must not be able to make this allocate
/// without limit. Four thousand is past any real page — the largest in the
/// corpus census is 122, three orders of magnitude below — so nothing a
/// producer writes is truncated by it.
///
/// **Past the cap the list is shortened, and the returned value says by how
/// much**: [`AnnotationList::dropped`], from [`crate::Page::annotation_list`].
/// Not a warning: a warning would have to go on
/// [`crate::Document::warnings`], and appending to it here would make a read
/// change what the document reports about itself, which is the module
/// comment's second argument against building this on `cos::font::read`.
/// Until September 2026 that argument ended at "so it says nothing", and the
/// roadmap carried the gap; the count in the returned value is the answer that
/// does not mutate anything. `a_hostile_annots_array_is_capped` pins the bound
/// and the count — before it existed raising this constant to `usize::MAX`
/// passed every other test in the crate.
pub(crate) const MAX_ANNOTS: usize = 4096;

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

/// The border an annotation is drawn with (12.5.4): `/BS` (Table 166) where
/// the annotation has one, and the older `/Border` array (Table 164) where it
/// does not — 12.5.2 has `/BS` supersede it.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Border {
    /// The width in points: `/BS /W`, or `/Border`'s third number. 1 by
    /// default, and 0 for no border.
    pub width: f64,
    /// `/BS /S`: `S` solid (the default), `D` dashed, `B` beveled, `I` inset,
    /// `U` underline. `/Border` has no style and reads as `S`, or `D` with a
    /// dash array.
    pub style: String,
    /// The dash array: `/BS /D`, or `/Border`'s optional fourth element.
    pub dash: Vec<f64>,
    /// `/Border`'s horizontal and vertical corner radii. `/BS` has none.
    pub corner_radii: Option<(f64, f64)>,
}

/// `/RC`, a markup annotation's rich text (Table 170): an XHTML body, as the
/// file carries it.
#[derive(Clone, Debug, PartialEq)]
pub enum RichText {
    /// A text string, decoded (7.9.2.2).
    Text(String),
    /// A text stream, by reference and **not decoded**: decoding it would add
    /// its warnings to [`crate::Document::warnings`], and a read must not
    /// change what the document reports. `Document::cos` reads it.
    Stream(ObjRef),
}

/// A page's annotations, and what the listing could not read (ruling 10).
///
/// [`crate::Page::annotation_list`]'s answer. The counts are how a cap names
/// what it dropped **in the value it returns** — the read changes nothing,
/// and the same page read twice says the same thing twice.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct AnnotationList {
    /// One entry per `/Annots` entry read, in the array's order.
    pub annotations: Vec<Annotation>,
    /// How many `/Annots` entries past the listing's 4 096 were not read at all.
    /// Zero for every page the corpus held when it was last measured, whose
    /// busiest carried 122.
    pub dropped: usize,
    /// How many of [`AnnotationList::annotations`] are
    /// [`Annotation::incomplete`]: listed, with an entry left unread because
    /// the page spent [`MAX_ANNOTATION_BYTES`].
    pub incomplete: usize,
}

impl AnnotationList {
    /// Whether every entry of `/Annots` was read whole.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.dropped == 0 && self.incomplete == 0
    }
}

/// One entry of a page's `/Annots` array, read (12.5.2).
///
/// The entries every annotation dictionary may carry (Table 164), the markup
/// entries (Table 170), and the family's own in [`Annotation::payload`]
/// (12.5.6's tables, one variant per family).
///
/// `#[non_exhaustive]` since the payloads arrived: a struct a caller only
/// reads should be able to grow a field without breaking them.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
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
    /// `/AS`: which of the appearance's states is shown (12.5.5).
    pub appearance_state: Option<String>,
    /// `/NM`: a name unique among the page's annotations (Table 164).
    pub unique_name: Option<String>,
    /// `/C` (Table 164): the colour of the background when closed, the title
    /// bar of a pop-up, a link's border. Zero components (transparent), one
    /// (grey), three (RGB) or four (CMYK), as the file wrote them.
    pub colour: Option<Vec<f64>>,
    /// `/BS` or `/Border` (12.5.4).
    pub border: Option<Border>,
    /// `/CA` (Table 170): the opacity a markup annotation is drawn at.
    /// `None` when the file says nothing, which 12.5.6.2 makes 1.0; only read
    /// on a markup annotation.
    pub opacity: Option<f64>,
    /// `/RC` (Table 170): rich text. Markup annotations only.
    pub rich_text: Option<RichText>,
    /// `/Subj` (Table 170): the subject. Markup annotations only.
    pub subject: Option<String>,
    /// `/CreationDate` (Table 170), as the file wrote it. Markup annotations
    /// only.
    pub created: Option<String>,
    /// [`Annotation::created`] parsed as a 7.9.4 date, when it is one.
    pub created_date: Option<Date>,
    /// `/IRT` (Table 170): the annotation this one replies to.
    pub in_reply_to: Option<ObjRef>,
    /// `/RT` (Table 170): `R` for a reply (the default when `/IRT` is
    /// present) or `Group`.
    pub reply_type: Option<String>,
    /// `/IT` (Table 170): the intent — `FreeTextCallout`, `LineArrow`,
    /// `PolygonCloud`, … Markup annotations only.
    pub intent: Option<String>,
    /// The family's own entries (12.5.6).
    pub payload: AnnotationPayload,
    /// Whether an entry was left unread because this page's listing spent
    /// [`MAX_ANNOTATION_BYTES`]. Every entry of an incomplete annotation that
    /// reads as absent may have been present; the rest were read whole.
    pub incomplete: bool,
}

/// Reads one page's `/Annots`, in the array's own order.
///
/// The returned list is the same length as the array, capped at
/// [`MAX_ANNOTS`]; the list says how many entries the cap left unread, and
/// how many it read with an entry left out.
pub(crate) fn of_page(doc: &Arc<CosDocument>, page: ObjRef) -> AnnotationList {
    let mut list = AnnotationList {
        annotations: Vec::new(),
        dropped: 0,
        incomplete: 0,
    };
    let Ok(object) = doc.get(page) else {
        return list;
    };
    let Some(dict) = object.as_dict() else {
        return list;
    };
    let annots = doc.resolve_key(dict, doc.intern(b"Annots"));
    let Some(entries) = annots.as_array() else {
        return list;
    };

    let mut read = Read::new(doc, MAX_ANNOTATION_BYTES);
    list.annotations = entries
        .iter()
        .take(MAX_ANNOTS)
        .map(|entry| read_one(&mut read, entry))
        .collect();
    list.dropped = entries.len().saturating_sub(MAX_ANNOTS);
    list.incomplete = list.annotations.iter().filter(|a| a.incomplete).count();
    list
}

fn read_one(read: &mut Read<'_>, entry: &Object) -> Annotation {
    let doc = read.doc;
    read.cut = false;
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
            appearance_state: None,
            unique_name: None,
            colour: None,
            border: None,
            opacity: None,
            rich_text: None,
            subject: None,
            created: None,
            created_date: None,
            in_reply_to: None,
            reply_type: None,
            intent: None,
            payload: AnnotationPayload::None,
            incomplete: false,
        };
    };

    let subtype = doc
        .resolve_key(dict, doc.intern(b"Subtype"))
        .as_name()
        .and_then(|n| doc.name_bytes(n));
    let kind = match &subtype {
        Some(name) => AnnotationKind::from_name(name),
        None => AnnotationKind::Unnamed,
    };

    // 12.5.6.14 Table 183: a pop-up takes /Contents, /M and /T from its
    // parent. **Only a pop-up, and only through /Parent.** The `/Popup` entry
    // below points the other way and is never followed for text.
    let text_source = match kind {
        AnnotationKind::Popup => parent_dict(doc, dict).unwrap_or_else(|| dict.clone()),
        _ => dict.clone(),
    };

    let modified = read.text(&text_source, b"M");
    let markup = kind.is_markup();
    let created = if markup {
        read.text(dict, b"CreationDate")
    } else {
        None
    };
    let payload = match (&subtype, kind.is_covered()) {
        (Some(name), true) => read.payload(name, dict),
        _ => AnnotationPayload::None,
    };
    let mut annotation = Annotation {
        reference,
        rect: rect_of(doc, dict),
        contents: read.text(&text_source, b"Contents"),
        title: read.text(&text_source, b"T"),
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
        appearance_state: read.name(dict, b"AS"),
        unique_name: read.text(dict, b"NM"),
        // 12.5.6.14 names `/C` among the entries a parent overrides.
        colour: read.colour(&text_source, b"C"),
        border: border_of(read, dict),
        opacity: if markup {
            doc.resolve_key(dict, doc.intern(b"CA"))
                .as_number()
                .filter(|n| n.is_finite())
        } else {
            None
        },
        rich_text: if markup {
            rich_text_of(read, dict)
        } else {
            None
        },
        subject: if markup {
            read.text(dict, b"Subj")
        } else {
            None
        },
        created_date: created.as_deref().and_then(parse_date),
        created,
        in_reply_to: if markup {
            dict.get_ref(doc.intern(b"IRT"))
        } else {
            None
        },
        reply_type: if markup { read.name(dict, b"RT") } else { None },
        intent: if markup { read.name(dict, b"IT") } else { None },
        payload,
        kind,
        incomplete: false,
    };
    annotation.incomplete = read.cut;
    annotation
}

/// `/BS`, or `/Border` where there is no `/BS` (12.5.4).
fn border_of(read: &mut Read<'_>, dict: &Dict) -> Option<Border> {
    let doc = read.doc;
    let style = doc.resolve_key(dict, doc.intern(b"BS"));
    if let Some(bs) = style.as_dict() {
        return Some(Border {
            width: doc
                .resolve_key(bs, doc.intern(b"W"))
                .as_number()
                .filter(|w| w.is_finite())
                .unwrap_or(1.0),
            style: read.name(bs, b"S").unwrap_or_else(|| "S".to_string()),
            dash: read.numbers(bs, b"D").unwrap_or_default(),
            corner_radii: None,
        });
    }
    let border = dict.get(doc.intern(b"Border"))?.clone();
    let resolved = doc.resolve(&border);
    let items = resolved.as_array()?;
    let number = |i: usize| {
        items
            .get(i)
            .and_then(|v| doc.resolve(v).as_number())
            .filter(|n| n.is_finite())
    };
    let (h, v, w) = (number(0)?, number(1)?, number(2)?);
    let dash = match items.get(3) {
        Some(dash) => read.numbers_of(dash).unwrap_or_default(),
        None => Vec::new(),
    };
    Some(Border {
        width: w,
        style: if dash.is_empty() { "S" } else { "D" }.to_string(),
        dash,
        corner_radii: Some((h, v)),
    })
}

/// `/RC`: a text string, decoded, or a stream, named (Table 170).
fn rich_text_of(read: &mut Read<'_>, dict: &Dict) -> Option<RichText> {
    let doc = read.doc;
    let entry = dict.get(doc.intern(b"RC"))?;
    if let Some(r) = entry.as_objref() {
        if doc.resolve(entry).as_stream().is_some() {
            return Some(RichText::Stream(r));
        }
    }
    read.text(dict, b"RC").map(RichText::Text)
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

/// `/Rect`, ordered (12.5.2 Table 164).
///
/// The clause says the rectangle "shall" be written with the lower-left
/// corner first and immediately adds that a reader should normalise it, which
/// is the same instruction 7.9.5 gives for every rectangle. Producers do get
/// it backwards.
fn rect_of(doc: &CosDocument, dict: &Dict) -> (f64, f64, f64, f64) {
    let value = doc.resolve_key(dict, doc.intern(b"Rect"));
    // Four or it is not a rectangle, and checked before anything is copied:
    // a `/Rect` of a million numbers shared by every annotation on the page
    // would otherwise be copied once per annotation.
    let Some(items) = value.as_array().filter(|items| items.len() == 4) else {
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

    /// 12.5.6.14 names `/C` beside `/Contents`, `/M` and `/T`: a pop-up's
    /// colour is its parent's too.
    #[test]
    fn a_popup_takes_its_colour_from_its_parent() {
        let doc = page_with(
            "/Annots [21 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /C [0 0 1] >>\nendobj\n\
             21 0 obj\n<< /Type /Annot /Subtype /Popup /Rect [10 0 20 10] /C [1 0 0] \
             /Parent 20 0 R >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots[0].colour, Some(vec![0.0, 0.0, 1.0]));
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
    /// It pins the truncation as well as the bound, and since September 2026
    /// the count: the entries past the cap are dropped, and
    /// [`AnnotationList::dropped`] says how many — see [`MAX_ANNOTS`] for why
    /// that is in the returned value rather than on `Document::warnings`,
    /// which this test also holds unchanged.
    ///
    /// **The sizes are literals on purpose.** A fixture sized from the
    /// constant it is meant to pin grows along with it: written as
    /// `repeat(MAX_ANNOTS + 1)` and `assert_eq!(len, MAX_ANNOTS)` this test
    /// passed with the cap raised to a hundred thousand, because the array
    /// grew to a hundred thousand and one. The bound's *value* is the
    /// contract, so it is spelled out and the constant is checked against it.
    #[test]
    fn a_hostile_annots_array_is_capped() {
        let entries = "<< /Subtype /Square >> ".repeat(4099);
        let doc = page_with(&format!("/Annots [{entries}]"), "");
        let warnings = doc.warnings().len();
        let list = doc.page(0).expect("a page").annotation_list();
        let annotations = &list.annotations;

        assert_eq!(
            annotations.len(),
            4096,
            "4 099 entries went in and ruling 1's bound is 4 096"
        );
        assert_eq!(MAX_ANNOTS, 4096, "and that bound is this constant");
        assert_eq!(list.dropped, 3, "and the list says how many it left out");
        assert!(!list.is_complete());
        assert!(
            annotations.iter().all(|a| a.kind == AnnotationKind::Square),
            "the entries kept are the array's own, read normally"
        );
        assert_eq!(
            doc.page(0).expect("a page").annotations().len(),
            4096,
            "`annotations()` is the same list"
        );
        assert_eq!(
            doc.warnings().len(),
            warnings,
            "and reading it, twice, told the document nothing"
        );
    }

    /// Ruling 1 for the copies: four thousand annotations naming one shared
    /// `/InkList` of 4 096 numbers ask for 128 MiB of copies, past
    /// [`MAX_ANNOTATION_BYTES`]. The listing stops copying when the budget is
    /// spent, and says so on each annotation it cut and in the count.
    ///
    /// The sizes are literals for `a_hostile_annots_array_is_capped`'s
    /// reason, and the constant is checked against the one this is built to
    /// exceed.
    #[test]
    fn a_listing_past_its_copy_budget_says_what_it_cut() {
        let path = "1 2 ".repeat(2048);
        let entries = "<< /Subtype /Ink /Rect [0 0 1 1] /InkList [20 0 R] >> ".repeat(4096);
        let doc = page_with(
            &format!("/Annots [{entries}]"),
            &format!("20 0 obj\n[{path}]\nendobj\n"),
        );
        assert_eq!(
            MAX_ANNOTATION_BYTES,
            64 << 20,
            "the cap this is sized against"
        );
        let warnings = doc.warnings().len();
        let list = doc.page(0).expect("a page").annotation_list();

        assert_eq!(list.annotations.len(), 4096, "every entry is still listed");
        assert_eq!(list.dropped, 0);
        // 2 048 annotations' strokes fit in 64 MiB at 32 KiB each (4 096
        // numbers of eight bytes, and eight for the one path), less the one
        // the outer arrays' charges push over.
        assert!(
            list.incomplete > 2000 && list.incomplete < 2100,
            "about half were cut: {}",
            list.incomplete
        );
        let first = &list.annotations[0];
        assert!(!first.incomplete);
        assert!(matches!(
            &first.payload,
            AnnotationPayload::Ink { strokes } if strokes.len() == 1 && strokes[0].len() == 2048
        ));
        let last = &list.annotations[4095];
        assert!(last.incomplete, "the last one lost its strokes and says so");
        assert!(matches!(
            &last.payload,
            AnnotationPayload::Ink { strokes } if strokes.is_empty()
        ));
        assert_eq!(doc.warnings().len(), warnings);
    }

    /// The common entries this model reads beside the payload (Table 164,
    /// Table 170, 12.5.4).
    #[test]
    fn the_common_and_markup_entries_are_read() {
        let doc = page_with(
            "/Annots [20 0 R 21 0 R 22 0 R]",
            "20 0 obj\n<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] /NM (sq-1) \
             /C [1 0 0] /BS << /W 2 /S /D /D [3 1] >> /CA 0.5 /Subj (Review) \
             /RC (<body>rich</body>) /CreationDate (D:20260102030405Z) \
             /IRT 21 0 R /RT /Group /IT /SquareCloud /AS /N >>\nendobj\n\
             21 0 obj\n<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] \
             /Border [2 3 1.5 [4 2]] /C [] /CA 0.25 >>\nendobj\n\
             22 0 obj\n<< /Type /Annot /Subtype /FreeText /Rect [0 0 1 1] /DA (/Helv 9 Tf) \
             /RC 23 0 R >>\nendobj\n\
             23 0 obj\n<< /Length 4 >>\nstream\n<p/>\nendstream\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        let square = &annots[0];
        assert_eq!(square.unique_name.as_deref(), Some("sq-1"));
        assert_eq!(square.appearance_state.as_deref(), Some("N"));
        assert_eq!(square.colour, Some(vec![1.0, 0.0, 0.0]));
        assert_eq!(
            square.border,
            Some(Border {
                width: 2.0,
                style: "D".into(),
                dash: vec![3.0, 1.0],
                corner_radii: None,
            })
        );
        assert_eq!(square.opacity, Some(0.5));
        assert_eq!(square.subject.as_deref(), Some("Review"));
        assert_eq!(
            square.rich_text,
            Some(RichText::Text("<body>rich</body>".into()))
        );
        assert_eq!(square.created.as_deref(), Some("D:20260102030405Z"));
        assert_eq!(square.created_date.map(|d| d.year), Some(2026));
        assert_eq!(square.in_reply_to.map(|r| r.num), Some(21));
        assert_eq!(square.reply_type.as_deref(), Some("Group"));
        assert_eq!(square.intent.as_deref(), Some("SquareCloud"));
        assert!(!square.incomplete);

        // A link is not a markup annotation: `/CA` is not read on it.
        let link = &annots[1];
        assert_eq!(link.colour, Some(Vec::new()), "an empty /C is transparent");
        assert_eq!(
            link.border,
            Some(Border {
                width: 1.5,
                style: "D".into(),
                dash: vec![4.0, 2.0],
                corner_radii: Some((2.0, 3.0)),
            }),
            "the legacy /Border, with its dash array"
        );
        assert_eq!(link.opacity, None, "Table 170 is markup-only");

        // A stream `/RC` is named, not decoded.
        assert_eq!(
            annots[2].rich_text,
            Some(RichText::Stream(ObjRef::new(23, 0)))
        );
    }

    /// **Every family's payload**, one annotation each, against its table.
    ///
    /// The values are chosen so that a default and a read cannot be confused
    /// — every entry the table defaults is given something else here — and
    /// a second page of each family with nothing but `/Subtype` and `/Rect`
    /// checks the defaults themselves.
    #[test]
    fn every_family_payload_is_read() {
        let objects = "\
20 0 obj\n<< /Subtype /Text /Rect [0 0 1 1] /Open true /Name /Key /State (Accepted) /StateModel (Review) >>\nendobj\n\
21 0 obj\n<< /Subtype /Link /Rect [0 0 1 1] /H /P /QuadPoints [0 10 20 10 0 0 20 0] >>\nendobj\n\
22 0 obj\n<< /Subtype /FreeText /Rect [0 0 1 1] /DA (/Helv 12 Tf 0 g) /Q 2 /DS (font: 12pt) /CL [1 2 3 4 5 6] /BE << /S /C /I 1 >> /RD [1 2 3 4] /LE /OpenArrow >>\nendobj\n\
23 0 obj\n<< /Subtype /Line /Rect [0 0 1 1] /L [1 2 30 40] /LE [/Circle /ClosedArrow] /IC [0 1 0] /LL 5 /LLE 2 /LLO 1 /Cap true /CP /Top /CO [3 4] >>\nendobj\n\
24 0 obj\n<< /Subtype /Circle /Rect [0 0 1 1] /IC [0.5] /BE << /S /C >> /RD [1 1 1 1] >>\nendobj\n\
25 0 obj\n<< /Subtype /PolyLine /Rect [0 0 1 1] /Vertices [0 0 10 10 20 0] /LE [/Butt /Slash] /IC [0 0 0 1] >>\nendobj\n\
26 0 obj\n<< /Subtype /Squiggly /Rect [0 0 1 1] /QuadPoints [0 10 20 10 0 0 20 0 30 10 40 10 30 0 40 0] >>\nendobj\n\
27 0 obj\n<< /Subtype /Caret /Rect [0 0 1 1] /RD [0 1 0 1] /Sy /P >>\nendobj\n\
28 0 obj\n<< /Subtype /Stamp /Rect [0 0 1 1] /Name /Approved >>\nendobj\n\
29 0 obj\n<< /Subtype /Ink /Rect [0 0 1 1] /InkList [[0 0 1 1 2 2] [5 5 6 6]] >>\nendobj\n\
30 0 obj\n<< /Subtype /Popup /Rect [0 0 1 1] /Open true >>\nendobj\n\
31 0 obj\n<< /Subtype /FileAttachment /Rect [0 0 1 1] /Name /Paperclip /FS << /Type /Filespec /F (data.csv) /UF (data \\(1\\).csv) /Desc (the data) /EF << /F 50 0 R >> >> >>\nendobj\n\
32 0 obj\n<< /Subtype /Sound /Rect [0 0 1 1] /Sound 51 0 R /Name /Mic >>\nendobj\n\
33 0 obj\n<< /Subtype /Movie /Rect [0 0 1 1] /T (Trailer) /Movie << /F (clip.mov) >> >>\nendobj\n\
34 0 obj\n<< /Subtype /Screen /Rect [0 0 1 1] /T (Player) /A << /S /Rendition >> >>\nendobj\n\
35 0 obj\n<< /Subtype /Widget /Rect [0 0 1 1] /H /O /MK << /BC [0] >> >>\nendobj\n\
36 0 obj\n<< /Subtype /PrinterMark /Rect [0 0 1 1] /MN /ColorBar >>\nendobj\n\
37 0 obj\n<< /Subtype /TrapNet /Rect [0 0 1 1] /LastModified (D:20260101) >>\nendobj\n\
38 0 obj\n<< /Subtype /Watermark /Rect [0 0 1 1] /FixedPrint << /Type /FixedPrint >> >>\nendobj\n\
39 0 obj\n<< /Subtype /Redact /Rect [0 0 1 1] /QuadPoints [0 10 20 10 0 0 20 0] /IC [0 0 0] /RO 52 0 R /OverlayText (REDACTED) /Repeat true /DA (/Helv 8 Tf) /Q 1 >>\nendobj\n\
40 0 obj\n<< /Subtype /3D /Rect [0 0 1 1] /3DD 53 0 R >>\nendobj\n\
41 0 obj\n<< /Subtype /Projection /Rect [0 0 1 1] >>\nendobj\n\
42 0 obj\n<< /Subtype /RichMedia /Rect [0 0 1 1] /RichMediaContent << /Assets << >> >> /RichMediaSettings 54 0 R >>\nendobj\n\
43 0 obj\n<< /Subtype /Polygon /Rect [0 0 1 1] /Vertices [0 0 10 0 5 5] /LE [/Slash /Slash] >>\nendobj\n\
50 0 obj\n<< /Type /EmbeddedFile /Length 3 >>\nstream\na,b\nendstream\nendobj\n\
51 0 obj\n<< /Type /Sound /R 8000 /Length 1 >>\nstream\n\x00\nendstream\nendobj\n\
52 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] /Length 0 >>\nstream\n\nendstream\nendobj\n\
53 0 obj\n<< /Type /3D /Subtype /U3D /Length 0 >>\nstream\n\nendstream\nendobj\n\
54 0 obj\n<< /Type /RichMediaSettings >>\nendobj\n";
        let refs: String = (20..=43).map(|n| format!("{n} 0 R ")).collect();
        let doc = page_with(&format!("/Annots [{refs}]"), objects);
        let annots = doc.page(0).expect("a page").annotations();
        assert_eq!(annots.len(), 24);
        assert!(annots.iter().all(|a| !a.incomplete));
        let payload = |i: usize| annots[i].payload.clone();

        assert_eq!(
            payload(0),
            AnnotationPayload::Text {
                open: true,
                icon: "Key".into(),
                state: Some("Accepted".into()),
                state_model: Some("Review".into()),
            }
        );
        assert_eq!(
            payload(1),
            AnnotationPayload::Link {
                highlight: "P".into(),
                quads: vec![[0.0, 10.0, 20.0, 10.0, 0.0, 0.0, 20.0, 0.0]],
            }
        );
        assert_eq!(
            payload(2),
            AnnotationPayload::FreeText {
                default_appearance: Some("/Helv 12 Tf 0 g".into()),
                quadding: 2,
                default_style: Some("font: 12pt".into()),
                callout: vec![(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)],
                border_effect: Some(BorderEffect {
                    style: "C".into(),
                    intensity: 1.0,
                }),
                rect_differences: Some([1.0, 2.0, 3.0, 4.0]),
                line_ending: "OpenArrow".into(),
            }
        );
        assert_eq!(
            payload(3),
            AnnotationPayload::Line {
                line: Some(((1.0, 2.0), (30.0, 40.0))),
                endings: ("Circle".into(), "ClosedArrow".into()),
                interior_colour: Some(vec![0.0, 1.0, 0.0]),
                leader_length: 5.0,
                leader_extension: 2.0,
                leader_offset: 1.0,
                caption: true,
                caption_position: "Top".into(),
                caption_offset: Some((3.0, 4.0)),
            }
        );
        assert_eq!(
            payload(4),
            AnnotationPayload::Shape {
                interior_colour: Some(vec![0.5]),
                border_effect: Some(BorderEffect {
                    style: "C".into(),
                    intensity: 0.0,
                }),
                rect_differences: Some([1.0; 4]),
            }
        );
        assert_eq!(
            payload(5),
            AnnotationPayload::Polygon {
                vertices: vec![(0.0, 0.0), (10.0, 10.0), (20.0, 0.0)],
                endings: ("Butt".into(), "Slash".into()),
                interior_colour: Some(vec![0.0, 0.0, 0.0, 1.0]),
                border_effect: None,
            }
        );
        assert_eq!(
            payload(6),
            AnnotationPayload::TextMarkup {
                quads: vec![
                    [0.0, 10.0, 20.0, 10.0, 0.0, 0.0, 20.0, 0.0],
                    [30.0, 10.0, 40.0, 10.0, 30.0, 0.0, 40.0, 0.0],
                ],
            }
        );
        assert_eq!(
            payload(7),
            AnnotationPayload::Caret {
                rect_differences: Some([0.0, 1.0, 0.0, 1.0]),
                symbol: "P".into(),
            }
        );
        assert_eq!(
            payload(8),
            AnnotationPayload::Stamp {
                icon: "Approved".into()
            }
        );
        assert_eq!(
            payload(9),
            AnnotationPayload::Ink {
                strokes: vec![
                    vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
                    vec![(5.0, 5.0), (6.0, 6.0)],
                ],
            }
        );
        assert_eq!(payload(10), AnnotationPayload::Popup { open: true });
        assert_eq!(
            payload(11),
            AnnotationPayload::FileAttachment {
                file: Some(FileSpec {
                    name: Some("data (1).csv".into()),
                    description: Some("the data".into()),
                    embedded: Some(ObjRef::new(50, 0)),
                }),
                icon: "Paperclip".into(),
            },
            "/UF before /F (7.11.3)"
        );
        assert_eq!(
            payload(12),
            AnnotationPayload::Sound {
                sound: Some(Linked::Object(ObjRef::new(51, 0))),
                icon: "Mic".into(),
            }
        );
        assert_eq!(
            payload(13),
            AnnotationPayload::Movie {
                title: Some("Trailer".into()),
                movie: Some(Linked::Direct),
                file: Some(FileSpec {
                    name: Some("clip.mov".into()),
                    description: None,
                    embedded: None,
                }),
            }
        );
        assert_eq!(
            payload(14),
            AnnotationPayload::Screen {
                title: Some("Player".into()),
                has_action: true,
            }
        );
        assert_eq!(
            payload(15),
            AnnotationPayload::Widget {
                highlight: "O".into(),
                has_characteristics: true,
                has_action: false,
            }
        );
        assert_eq!(
            payload(16),
            AnnotationPayload::PrinterMark {
                name: Some("ColorBar".into())
            }
        );
        assert_eq!(
            payload(17),
            AnnotationPayload::TrapNet {
                last_modified: Some("D:20260101".into()),
                has_version: false,
            }
        );
        assert_eq!(
            payload(18),
            AnnotationPayload::Watermark { fixed_print: true }
        );
        assert_eq!(
            payload(19),
            AnnotationPayload::Redact {
                quads: vec![[0.0, 10.0, 20.0, 10.0, 0.0, 0.0, 20.0, 0.0]],
                interior_colour: Some(vec![0.0, 0.0, 0.0]),
                overlay: Some(Linked::Object(ObjRef::new(52, 0))),
                overlay_text: Some("REDACTED".into()),
                repeat: true,
                default_appearance: Some("/Helv 8 Tf".into()),
                quadding: 1,
            }
        );
        assert_eq!(
            payload(20),
            AnnotationPayload::ThreeD {
                artwork: Some(Linked::Object(ObjRef::new(53, 0)))
            }
        );
        assert_eq!(payload(21), AnnotationPayload::Projection);
        assert_eq!(
            payload(22),
            AnnotationPayload::RichMedia {
                content: Some(Linked::Direct),
                settings: Some(Linked::Object(ObjRef::new(54, 0))),
            }
        );
        assert_eq!(
            payload(23),
            AnnotationPayload::Polygon {
                vertices: vec![(0.0, 0.0), (10.0, 0.0), (5.0, 5.0)],
                endings: ("None".into(), "None".into()),
                interior_colour: None,
                border_effect: None,
            },
            "a polygon has no ends, whatever /LE says"
        );

        // Every family whose table requires something carries it here.
        for a in &annots {
            assert_ne!(
                a.payload.carries_required(),
                Some(false),
                "{}",
                a.payload.family()
            );
        }
        let families: std::collections::BTreeSet<&str> =
            annots.iter().map(|a| a.payload.family()).collect();
        assert_eq!(families.len(), 23, "one variant per 12.5.6 family");
    }

    /// Each table's defaults, read off annotations that carry nothing but
    /// `/Subtype` and `/Rect` — and the required entries' absence, which
    /// [`AnnotationPayload::carries_required`] reports.
    #[test]
    fn each_family_reads_its_tables_defaults() {
        let subtypes = [
            "Text",
            "Link",
            "FreeText",
            "Line",
            "Square",
            "Polygon",
            "PolyLine",
            "Highlight",
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
            "Redact",
            "3D",
            "Projection",
            "RichMedia",
        ];
        let entries: String = subtypes
            .iter()
            .map(|s| format!("<< /Subtype /{s} /Rect [0 0 1 1] >> "))
            .collect();
        let doc = page_with(&format!("/Annots [{entries}]"), "");
        let annots = doc.page(0).expect("a page").annotations();
        let by = |subtype: &str| {
            annots
                .iter()
                .find(|a| a.kind.as_name() == subtype)
                .map(|a| a.payload.clone())
                .expect("listed")
        };
        assert!(matches!(
            by("Text"),
            AnnotationPayload::Text { open: false, ref icon, state: None, .. } if icon == "Note"
        ));
        assert!(
            matches!(by("Link"), AnnotationPayload::Link { ref highlight, .. } if highlight == "I")
        );
        assert!(matches!(
            by("FreeText"),
            AnnotationPayload::FreeText { quadding: 0, ref line_ending, .. } if line_ending == "None"
        ));
        assert!(matches!(
            by("Line"),
            AnnotationPayload::Line { ref endings, ref caption_position, caption: false, .. }
                if endings.0 == "None" && endings.1 == "None" && caption_position == "Inline"
        ));
        assert!(
            matches!(by("Caret"), AnnotationPayload::Caret { ref symbol, .. } if symbol == "None")
        );
        assert!(matches!(by("Stamp"), AnnotationPayload::Stamp { ref icon } if icon == "Draft"));
        assert!(matches!(
            by("FileAttachment"),
            AnnotationPayload::FileAttachment { file: None, ref icon } if icon == "PushPin"
        ));
        assert!(matches!(
            by("Sound"),
            AnnotationPayload::Sound { sound: None, ref icon } if icon == "Speaker"
        ));
        assert!(
            matches!(by("Widget"), AnnotationPayload::Widget { ref highlight, .. } if highlight == "I")
        );
        assert!(matches!(
            by("Redact"),
            AnnotationPayload::Redact {
                repeat: false,
                quadding: 0,
                ..
            }
        ));

        let missing: Vec<&str> = annots
            .iter()
            .filter(|a| a.payload.carries_required() == Some(false))
            .map(|a| a.kind.as_name())
            .collect();
        assert_eq!(
            missing,
            vec![
                "FreeText",
                "Line",
                "Polygon",
                "PolyLine",
                "Highlight",
                "Ink",
                "FileAttachment",
                "Sound",
                "Movie",
                "TrapNet",
                "3D",
                "RichMedia",
            ],
            "the families whose tables require an entry, and only those"
        );
    }

    /// Geometry that is not what its table says is not half read: a partial
    /// quad, an odd vertex, a non-number in an ink path and a `/L` of three
    /// numbers each come back as nothing rather than as a guess.
    #[test]
    fn malformed_geometry_is_not_guessed_at() {
        let doc = page_with(
            "/Annots [20 0 R 21 0 R 22 0 R 23 0 R]",
            "20 0 obj\n<< /Subtype /Highlight /Rect [0 0 1 1] /QuadPoints [0 1 2 3 4 5 6 7 8 9] >>\nendobj\n\
             21 0 obj\n<< /Subtype /Polygon /Rect [0 0 1 1] /Vertices [0 0 1 1 2] >>\nendobj\n\
             22 0 obj\n<< /Subtype /Ink /Rect [0 0 1 1] /InkList [[0 0 /x 1] [1 1 2 2]] >>\nendobj\n\
             23 0 obj\n<< /Subtype /Line /Rect [0 0 1 1] /L [0 0 1] >>\nendobj\n",
        );
        let annots = doc.page(0).expect("a page").annotations();
        assert!(
            matches!(
                &annots[0].payload,
                AnnotationPayload::TextMarkup { quads } if quads.len() == 1
            ),
            "the whole quad, and not the two numbers after it"
        );
        assert!(matches!(
            &annots[1].payload,
            AnnotationPayload::Polygon { vertices, .. } if vertices.len() == 2
        ));
        assert!(
            matches!(
                &annots[2].payload,
                AnnotationPayload::Ink { strokes } if strokes == &vec![vec![(1.0, 1.0), (2.0, 2.0)]]
            ),
            "the path with a name in it is not a path"
        );
        assert!(matches!(
            &annots[3].payload,
            AnnotationPayload::Line { line: None, .. }
        ));
        assert_eq!(annots[3].payload.carries_required(), Some(false));
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
