//! The per-family payloads of 12.5.6: what each subtype's own table adds to
//! Table 164's common entries.
//!
//! One [`AnnotationPayload`] variant per family — a family being a clause of
//! 12.5.6, so `/Square` and `/Circle` share one (12.5.6.8), the four text
//! markup subtypes share one (12.5.6.10), and `/Polygon` and `/PolyLine`
//! share one (12.5.6.9). Each variant's fields are its table's entries,
//! transcribed with the table's own defaults where it states one, so a field
//! the file left out reads as what a conforming reader would take it to be
//! rather than as an absence the caller then has to default.
//!
//! What is *referenced* rather than read — a sound's samples, a movie, a 3D
//! artwork, rich media, a redaction's overlay form — comes back as a
//! [`Linked`]: the object's reference, or [`Linked::Direct`] for one written
//! inline. Decoding a stream here would add its warnings to
//! `Document::warnings`, and a read must not change what the document reports
//! about itself.
//!
//! [`AnnotationPayload::carries_required`] is the per-family measure the
//! census counts: whether the entries the family's table marks *required* are
//! there.

use tinker_pdf_cos::{decode_text_string, CosDocument, Dict, ObjRef, Object};

/// A point in default user space.
pub type Point = (f64, f64);

/// An entry this model points at rather than reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linked {
    /// The entry is an indirect reference to this object.
    Object(ObjRef),
    /// The entry is written inline, so there is no reference to give.
    Direct,
}

/// A border effect (12.5.4 Table 167): `/BE`.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderEffect {
    /// `/S`: `S` for none, `C` for a cloudy border. Defaults to `S`.
    pub style: String,
    /// `/I`: the cloudy border's intensity, 0 to 2. Defaults to 0.
    pub intensity: f64,
}

/// A file specification (7.11), as a file attachment's `/FS` or a movie's
/// `/F` gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct FileSpec {
    /// The file's name: a string specification, or a dictionary's `/UF`
    /// before its `/F` (7.11.3 Table 43).
    pub name: Option<String>,
    /// `/Desc`, the description (Table 43).
    pub description: Option<String>,
    /// The embedded file stream, `/EF /UF` or `/EF /F` (7.11.4), when the
    /// file is carried inside the document.
    pub embedded: Option<ObjRef>,
}

/// The per-family entries of an annotation (12.5.6).
///
/// Each variant is `#[non_exhaustive]` so that a table entry this model does
/// not read yet can be added without breaking a caller who matches on the
/// ones it does.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum AnnotationPayload {
    /// An entry whose family this model could not name:
    /// `AnnotationKind::Other`, `Unnamed` or `Unreadable`.
    None,
    /// `/Text`, 12.5.6.4 Table 172.
    #[non_exhaustive]
    Text {
        /// `/Open`: the note starts open. Defaults to false.
        open: bool,
        /// `/Name`: the icon — `Note` by default, or `Comment`, `Key`,
        /// `Help`, `NewParagraph`, `Paragraph`, `Insert`, or the producer's.
        icon: String,
        /// `/State` (12.5.6.3), for a note that is a state change of another.
        state: Option<String>,
        /// `/StateModel`: `Marked` or `Review`.
        state_model: Option<String>,
    },
    /// `/Link`, 12.5.6.5 Table 173. The target is `Page::links`'.
    #[non_exhaustive]
    Link {
        /// `/H`: `N`, `I` (the default), `O` or `P`.
        highlight: String,
        /// `/QuadPoints`: the regions that activate it, when narrower than
        /// the `/Rect`.
        quads: Vec<[f64; 8]>,
    },
    /// `/FreeText`, 12.5.6.6 Table 174.
    #[non_exhaustive]
    FreeText {
        /// `/DA`, required: the default appearance string.
        default_appearance: Option<String>,
        /// `/Q`: 0 left, 1 centred, 2 right. Defaults to 0.
        quadding: i64,
        /// `/DS`: a CSS2 default style string.
        default_style: Option<String>,
        /// `/CL`: a callout line of two or three points.
        callout: Vec<Point>,
        /// `/BE`.
        border_effect: Option<BorderEffect>,
        /// `/RD`: the inset of the drawn box within `/Rect`, as left, top,
        /// right, bottom.
        rect_differences: Option<[f64; 4]>,
        /// `/LE`: the callout's line ending. Defaults to `None`.
        line_ending: String,
    },
    /// `/Line`, 12.5.6.7 Table 175.
    #[non_exhaustive]
    Line {
        /// `/L`, required: the two end points.
        line: Option<(Point, Point)>,
        /// `/LE`: the endings at each end. Defaults to `None` and `None`.
        endings: (String, String),
        /// `/IC`: the colour the endings are filled with.
        interior_colour: Option<Vec<f64>>,
        /// `/LL`: the leader lines' length. Defaults to 0.
        leader_length: f64,
        /// `/LLE`: the leader line extensions' length. Defaults to 0.
        leader_extension: f64,
        /// `/LLO`: the leader line offset. Defaults to 0.
        leader_offset: f64,
        /// `/Cap`: the `/Contents` or `/RC` is shown as a caption. Defaults to
        /// false.
        caption: bool,
        /// `/CP`: `Inline` (the default) or `Top`.
        caption_position: String,
        /// `/CO`: the caption's offset from its default place.
        caption_offset: Option<Point>,
    },
    /// `/Square` and `/Circle`, 12.5.6.8 Table 177.
    #[non_exhaustive]
    Shape {
        /// `/IC`: the fill colour.
        interior_colour: Option<Vec<f64>>,
        /// `/BE`.
        border_effect: Option<BorderEffect>,
        /// `/RD`: the inset of the drawn shape within `/Rect`.
        rect_differences: Option<[f64; 4]>,
    },
    /// `/Polygon` and `/PolyLine`, 12.5.6.9 Table 178.
    #[non_exhaustive]
    Polygon {
        /// `/Vertices`, required.
        vertices: Vec<Point>,
        /// `/LE`, a polyline's endings. `None` and `None` by default, and
        /// for a polygon, which has no ends.
        endings: (String, String),
        /// `/IC`.
        interior_colour: Option<Vec<f64>>,
        /// `/BE`.
        border_effect: Option<BorderEffect>,
    },
    /// `/Highlight`, `/Underline`, `/Squiggly` and `/StrikeOut`, 12.5.6.10
    /// Table 179.
    #[non_exhaustive]
    TextMarkup {
        /// `/QuadPoints`, required: one quad per run of marked text, its
        /// corners in the order the file wrote them.
        quads: Vec<[f64; 8]>,
    },
    /// `/Caret`, 12.5.6.11 Table 180.
    #[non_exhaustive]
    Caret {
        /// `/RD`.
        rect_differences: Option<[f64; 4]>,
        /// `/Sy`: `P` for a paragraph symbol, or `None` (the default).
        symbol: String,
    },
    /// `/Stamp`, 12.5.6.12 Table 181.
    #[non_exhaustive]
    Stamp {
        /// `/Name`: the stamp's icon. Defaults to `Draft`.
        icon: String,
    },
    /// `/Ink`, 12.5.6.13 Table 182.
    #[non_exhaustive]
    Ink {
        /// `/InkList`, required: one path per stroke.
        strokes: Vec<Vec<Point>>,
    },
    /// `/Popup`, 12.5.6.14 Table 183.
    #[non_exhaustive]
    Popup {
        /// `/Open`: the window starts open. Defaults to false.
        open: bool,
    },
    /// `/FileAttachment`, 12.5.6.15 Table 184.
    #[non_exhaustive]
    FileAttachment {
        /// `/FS`, required.
        file: Option<FileSpec>,
        /// `/Name`: the icon. Defaults to `PushPin`.
        icon: String,
    },
    /// `/Sound`, 12.5.6.16 Table 185.
    #[non_exhaustive]
    Sound {
        /// `/Sound`, required: the sound object (13.3).
        sound: Option<Linked>,
        /// `/Name`: the icon. Defaults to `Speaker`.
        icon: String,
    },
    /// `/Movie`, 12.5.6.17 Table 186.
    #[non_exhaustive]
    Movie {
        /// `/T`: the movie's title.
        title: Option<String>,
        /// `/Movie`, required: the movie dictionary (13.4).
        movie: Option<Linked>,
        /// That dictionary's `/F`: the file the movie is.
        file: Option<FileSpec>,
    },
    /// `/Screen`, 12.5.6.18 Table 187.
    #[non_exhaustive]
    Screen {
        /// `/T`: the screen's title.
        title: Option<String>,
        /// Whether `/A` carries an action.
        has_action: bool,
    },
    /// `/Widget`, 12.5.6.19 Table 188. The field it draws is the form's.
    #[non_exhaustive]
    Widget {
        /// `/H`: `N`, `I` (the default), `O`, `P` or `T`.
        highlight: String,
        /// Whether `/MK` gives appearance characteristics (Table 189).
        has_characteristics: bool,
        /// Whether `/A` carries an action.
        has_action: bool,
    },
    /// `/PrinterMark`, 12.5.6.20.
    #[non_exhaustive]
    PrinterMark {
        /// `/MN`: which mark — `ColorBar`, `RegistrationTarget`, ….
        name: Option<String>,
    },
    /// `/TrapNet`, 12.5.6.21 Table 189a.
    #[non_exhaustive]
    TrapNet {
        /// `/LastModified`, required unless `/Version` and `/AnnotStates`
        /// are present.
        last_modified: Option<String>,
        /// Whether `/Version` and `/AnnotStates` are both present.
        has_version: bool,
    },
    /// `/Watermark`, 12.5.6.22 Table 190.
    #[non_exhaustive]
    Watermark {
        /// Whether `/FixedPrint` fixes the mark's printed size and place.
        fixed_print: bool,
    },
    /// `/Redact`, 12.5.6.23 Table 191.
    #[non_exhaustive]
    Redact {
        /// `/QuadPoints`: the regions to remove, when narrower than `/Rect`.
        quads: Vec<[f64; 8]>,
        /// `/IC`: the colour the removed region is filled with.
        interior_colour: Option<Vec<f64>>,
        /// `/RO`: the form drawn over the removed region.
        overlay: Option<Linked>,
        /// `/OverlayText`.
        overlay_text: Option<String>,
        /// `/Repeat`: the overlay text is repeated to fill the region.
        /// Defaults to false.
        repeat: bool,
        /// `/DA`: the overlay text's appearance.
        default_appearance: Option<String>,
        /// `/Q`. Defaults to 0.
        quadding: i64,
    },
    /// `/3D`, 13.6.2 Table 298.
    #[non_exhaustive]
    ThreeD {
        /// `/3DD`, required: the 3D stream or its reference dictionary.
        artwork: Option<Linked>,
    },
    /// `/Projection`, ISO 32000-2 12.5.6.24, which adds nothing to the markup
    /// entries.
    Projection,
    /// `/RichMedia`, ISO 32000-2 13.7.2.
    #[non_exhaustive]
    RichMedia {
        /// `/RichMediaContent`, required.
        content: Option<Linked>,
        /// `/RichMediaSettings`.
        settings: Option<Linked>,
    },
}

impl AnnotationPayload {
    /// Which 12.5.6 family this is, as the clause names it — the key the
    /// census counts by.
    #[must_use]
    pub fn family(&self) -> &'static str {
        match self {
            AnnotationPayload::None => "none",
            AnnotationPayload::Text { .. } => "text",
            AnnotationPayload::Link { .. } => "link",
            AnnotationPayload::FreeText { .. } => "free text",
            AnnotationPayload::Line { .. } => "line",
            AnnotationPayload::Shape { .. } => "square and circle",
            AnnotationPayload::Polygon { .. } => "polygon and polyline",
            AnnotationPayload::TextMarkup { .. } => "text markup",
            AnnotationPayload::Caret { .. } => "caret",
            AnnotationPayload::Stamp { .. } => "rubber stamp",
            AnnotationPayload::Ink { .. } => "ink",
            AnnotationPayload::Popup { .. } => "pop-up",
            AnnotationPayload::FileAttachment { .. } => "file attachment",
            AnnotationPayload::Sound { .. } => "sound",
            AnnotationPayload::Movie { .. } => "movie",
            AnnotationPayload::Screen { .. } => "screen",
            AnnotationPayload::Widget { .. } => "widget",
            AnnotationPayload::PrinterMark { .. } => "printer's mark",
            AnnotationPayload::TrapNet { .. } => "trap network",
            AnnotationPayload::Watermark { .. } => "watermark",
            AnnotationPayload::Redact { .. } => "redaction",
            AnnotationPayload::ThreeD { .. } => "3D",
            AnnotationPayload::Projection => "projection",
            AnnotationPayload::RichMedia { .. } => "rich media",
        }
    }

    /// Whether the entries the family's table marks **required** are here.
    ///
    /// `None` for a family whose table requires nothing beyond the common
    /// entries. The requirements, by table: 174 `/DA`, 175 `/L`, 178
    /// `/Vertices`, 179 `/QuadPoints`, 182 `/InkList`, 184 `/FS`, 185
    /// `/Sound`, 186 `/Movie`, 189a `/LastModified` or both of `/Version` and
    /// `/AnnotStates`, 298 `/3DD`, and ISO 32000-2's `/RichMediaContent`.
    #[must_use]
    pub fn carries_required(&self) -> Option<bool> {
        match self {
            AnnotationPayload::FreeText {
                default_appearance, ..
            } => Some(default_appearance.is_some()),
            AnnotationPayload::Line { line, .. } => Some(line.is_some()),
            AnnotationPayload::Polygon { vertices, .. } => Some(!vertices.is_empty()),
            AnnotationPayload::TextMarkup { quads } => Some(!quads.is_empty()),
            AnnotationPayload::Ink { strokes } => Some(!strokes.is_empty()),
            AnnotationPayload::FileAttachment { file, .. } => Some(file.is_some()),
            AnnotationPayload::Sound { sound, .. } => Some(sound.is_some()),
            AnnotationPayload::Movie { movie, .. } => Some(movie.is_some()),
            AnnotationPayload::TrapNet {
                last_modified,
                has_version,
            } => Some(last_modified.is_some() || *has_version),
            AnnotationPayload::ThreeD { artwork } => Some(artwork.is_some()),
            AnnotationPayload::RichMedia { content, .. } => Some(content.is_some()),
            _ => None,
        }
    }
}

/// How many bytes one page's annotation listing may copy out of the document.
///
/// Every string, name and number an [`crate::Annotation`] carries is a copy,
/// and a copy is where a small file becomes a large allocation: an indirect
/// object is parsed once and may be named by every one of the 4 096 entries
/// `/Annots` is read to, so one 9 MB `/Contents` string — or one `/InkList` of
/// a million numbers — named four thousand times asks for tens of gigabytes.
/// That was true of `/Contents`, `/T` and `/M` from the day the model landed;
/// the per-family payloads made it true of every array in 12.5.6's tables. So
/// the listing spends one budget, charged before each copy is made, and an
/// entry the budget cannot pay for reads as absent **and says so**:
/// [`crate::Annotation::incomplete`] on the annotation, and
/// [`crate::AnnotationList::incomplete`] on the list (ruling 10, without
/// writing to `Document::warnings`).
///
/// A name costs its bytes, a string its bytes before decoding, a number eight.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 64 MiB |
/// | The most any other fixture spends, measured over every test that lists annotations | 923 |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **64 MiB** |
///
/// The three zeros are facts about what those paths write: a comic page has no
/// annotations, and the `/Link`s the XPS and EPUB paths write carry a `/Rect`,
/// a `/Dest` and a three-zero `/Border`, none of which this listing copies at a
/// cost. The corpus's busiest page carries 122 annotations
/// (`docs/features/document-model.md`); at 64 MiB each of them could carry half
/// a megabyte of text and geometry before the cap was near.
/// `a_listing_past_its_copy_budget_says_what_it_cut` builds four thousand
/// annotations naming one shared array and is what fires it.
pub const MAX_ANNOTATION_BYTES: usize = 64 << 20;

/// One page's copy budget, and whether the annotation being read has been
/// cut by it.
pub(super) struct Read<'d> {
    pub(super) doc: &'d CosDocument,
    left: usize,
    /// Whether the annotation being read lost an entry to the budget.
    pub(super) cut: bool,
}

impl<'d> Read<'d> {
    pub(super) fn new(doc: &'d CosDocument, budget: usize) -> Read<'d> {
        Read {
            doc,
            left: budget,
            cut: false,
        }
    }

    /// Takes `bytes` from the budget, or refuses and takes nothing.
    fn charge(&mut self, bytes: usize) -> bool {
        if bytes > self.left {
            self.cut = true;
            return false;
        }
        self.left -= bytes;
        true
    }

    fn key(&self, dict: &Dict, key: &[u8]) -> std::sync::Arc<Object> {
        self.doc.resolve_key(dict, self.doc.intern(key))
    }

    /// A text string entry, decoded (7.9.2.2).
    pub(super) fn text(&mut self, dict: &Dict, key: &[u8]) -> Option<String> {
        let value = self.key(dict, key);
        let string = value.as_string()?;
        if !self.charge(string.bytes.len()) {
            return None;
        }
        Some(decode_text_string(&string.bytes))
    }

    /// A name entry's bytes, as text.
    pub(super) fn name(&mut self, dict: &Dict, key: &[u8]) -> Option<String> {
        let name = self.key(dict, key).as_name()?;
        let bytes = self.doc.name_bytes(name)?;
        if !self.charge(bytes.len()) {
            return None;
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn name_or(&mut self, dict: &Dict, key: &[u8], default: &str) -> String {
        self.name(dict, key).unwrap_or_else(|| default.to_string())
    }

    fn number(&self, dict: &Dict, key: &[u8], default: f64) -> f64 {
        self.key(dict, key)
            .as_number()
            .filter(|n| n.is_finite())
            .unwrap_or(default)
    }

    fn int(&self, dict: &Dict, key: &[u8], default: i64) -> i64 {
        self.key(dict, key).as_int().unwrap_or(default)
    }

    fn bool(&self, dict: &Dict, key: &[u8], default: bool) -> bool {
        self.key(dict, key).as_bool().unwrap_or(default)
    }

    /// An array of numbers, charged before it is copied. `None` when the
    /// entry is not an array, when any element is not a finite number, or
    /// when the budget cannot pay for it.
    pub(super) fn numbers_of(&mut self, value: &Object) -> Option<Vec<f64>> {
        let resolved = self.doc.resolve(value);
        let items = resolved.as_array()?;
        if !self.charge(items.len().saturating_mul(8)) {
            return None;
        }
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let n = self.doc.resolve(item).as_number()?;
            if !n.is_finite() {
                return None;
            }
            out.push(n);
        }
        Some(out)
    }

    pub(super) fn numbers(&mut self, dict: &Dict, key: &[u8]) -> Option<Vec<f64>> {
        let value = dict.get(self.doc.intern(key))?.clone();
        self.numbers_of(&value)
    }

    /// A colour array (Table 164 `/C`, and every `/IC`): zero, one, three or
    /// four components. An empty array is transparent and comes back empty.
    pub(super) fn colour(&mut self, dict: &Dict, key: &[u8]) -> Option<Vec<f64>> {
        self.numbers(dict, key)
            .filter(|c| matches!(c.len(), 0 | 1 | 3 | 4))
    }

    /// `/QuadPoints`: whole quads of eight numbers; a trailing partial quad
    /// is not one.
    fn quads(&mut self, dict: &Dict) -> Vec<[f64; 8]> {
        self.numbers(dict, b"QuadPoints")
            .map(|n| {
                n.chunks_exact(8)
                    .map(|q| [q[0], q[1], q[2], q[3], q[4], q[5], q[6], q[7]])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Pairs of numbers as points; a trailing odd number is not a point.
    fn points_of(&mut self, value: &Object) -> Vec<Point> {
        self.numbers_of(value)
            .map(|n| n.chunks_exact(2).map(|p| (p[0], p[1])).collect())
            .unwrap_or_default()
    }

    fn points(&mut self, dict: &Dict, key: &[u8]) -> Vec<Point> {
        match dict.get(self.doc.intern(key)).cloned() {
            Some(value) => self.points_of(&value),
            None => Vec::new(),
        }
    }

    /// `/RD`: four non-negative insets.
    fn rect_differences(&mut self, dict: &Dict) -> Option<[f64; 4]> {
        match self.numbers(dict, b"RD")?.as_slice() {
            [a, b, c, d] => Some([*a, *b, *c, *d]),
            _ => None,
        }
    }

    /// `/LE` as a pair of names.
    fn endings(&mut self, dict: &Dict) -> (String, String) {
        let value = self.key(dict, b"LE");
        let names: Vec<String> = value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .take(2)
                    .filter_map(|i| self.doc.resolve(i).as_name())
                    .filter_map(|n| self.doc.name_bytes(n))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        if !self.charge(names.iter().map(String::len).sum()) {
            return ("None".into(), "None".into());
        }
        let mut names = names.into_iter();
        (
            names.next().unwrap_or_else(|| "None".into()),
            names.next().unwrap_or_else(|| "None".into()),
        )
    }

    fn border_effect(&mut self, dict: &Dict) -> Option<BorderEffect> {
        let value = self.key(dict, b"BE");
        let be = value.as_dict()?;
        Some(BorderEffect {
            style: self.name_or(be, b"S", "S"),
            intensity: self.number(be, b"I", 0.0),
        })
    }

    fn linked(&self, dict: &Dict, key: &[u8]) -> Option<Linked> {
        match dict.get(self.doc.intern(key))? {
            Object::Ref(r) => {
                // A reference to nothing — a free object, or one the file
                // never had — is not an entry.
                (!self.doc.resolve(&Object::Ref(*r)).is_null()).then_some(Linked::Object(*r))
            }
            Object::Null => None,
            _ => Some(Linked::Direct),
        }
    }

    /// A file specification (7.11): a string, or a dictionary.
    fn file_spec(&mut self, dict: &Dict, key: &[u8]) -> Option<FileSpec> {
        let value = self.key(dict, key);
        match value.as_ref() {
            Object::String(text) => {
                if !self.charge(text.bytes.len()) {
                    return None;
                }
                Some(FileSpec {
                    name: Some(decode_text_string(&text.bytes)),
                    description: None,
                    embedded: None,
                })
            }
            Object::Dict(spec) => {
                let name = self.text(spec, b"UF").or_else(|| self.text(spec, b"F"));
                let embedded = self.key(spec, b"EF").as_dict().and_then(|ef| {
                    ef.get_ref(self.doc.intern(b"UF"))
                        .or_else(|| ef.get_ref(self.doc.intern(b"F")))
                });
                Some(FileSpec {
                    name,
                    description: self.text(spec, b"Desc"),
                    embedded,
                })
            }
            _ => None,
        }
    }

    fn has(&self, dict: &Dict, key: &[u8]) -> bool {
        !self.key(dict, key).is_null()
    }

    /// The family payload of an annotation of subtype `subtype`.
    pub(super) fn payload(&mut self, subtype: &[u8], dict: &Dict) -> AnnotationPayload {
        match subtype {
            b"Text" => AnnotationPayload::Text {
                open: self.bool(dict, b"Open", false),
                icon: self.name_or(dict, b"Name", "Note"),
                state: self.text(dict, b"State"),
                state_model: self.text(dict, b"StateModel"),
            },
            b"Link" => AnnotationPayload::Link {
                highlight: self.name_or(dict, b"H", "I"),
                quads: self.quads(dict),
            },
            b"FreeText" => AnnotationPayload::FreeText {
                default_appearance: self.text(dict, b"DA"),
                quadding: self.int(dict, b"Q", 0),
                default_style: self.text(dict, b"DS"),
                callout: self.points(dict, b"CL"),
                border_effect: self.border_effect(dict),
                rect_differences: self.rect_differences(dict),
                line_ending: self.name_or(dict, b"LE", "None"),
            },
            b"Line" => AnnotationPayload::Line {
                line: match self.numbers(dict, b"L").as_deref() {
                    Some([x1, y1, x2, y2]) => Some(((*x1, *y1), (*x2, *y2))),
                    _ => None,
                },
                endings: self.endings(dict),
                interior_colour: self.colour(dict, b"IC"),
                leader_length: self.number(dict, b"LL", 0.0),
                leader_extension: self.number(dict, b"LLE", 0.0),
                leader_offset: self.number(dict, b"LLO", 0.0),
                caption: self.bool(dict, b"Cap", false),
                caption_position: self.name_or(dict, b"CP", "Inline"),
                caption_offset: match self.numbers(dict, b"CO").as_deref() {
                    Some([h, v]) => Some((*h, *v)),
                    _ => None,
                },
            },
            b"Square" | b"Circle" => AnnotationPayload::Shape {
                interior_colour: self.colour(dict, b"IC"),
                border_effect: self.border_effect(dict),
                rect_differences: self.rect_differences(dict),
            },
            b"Polygon" | b"PolyLine" => AnnotationPayload::Polygon {
                vertices: self.points(dict, b"Vertices"),
                endings: if subtype == b"PolyLine" {
                    self.endings(dict)
                } else {
                    ("None".into(), "None".into())
                },
                interior_colour: self.colour(dict, b"IC"),
                border_effect: self.border_effect(dict),
            },
            b"Highlight" | b"Underline" | b"Squiggly" | b"StrikeOut" => {
                AnnotationPayload::TextMarkup {
                    quads: self.quads(dict),
                }
            }
            b"Caret" => AnnotationPayload::Caret {
                rect_differences: self.rect_differences(dict),
                symbol: self.name_or(dict, b"Sy", "None"),
            },
            b"Stamp" => AnnotationPayload::Stamp {
                icon: self.name_or(dict, b"Name", "Draft"),
            },
            b"Ink" => {
                let list = self.key(dict, b"InkList");
                let paths = list.as_array().unwrap_or_default();
                // The outer array is charged as well as each path: a list of
                // a million empty paths costs nothing per path and everything
                // in walking them.
                let strokes = if self.charge(paths.len().saturating_mul(8)) {
                    paths
                        .iter()
                        .map(|path| self.points_of(path))
                        .filter(|path| !path.is_empty())
                        .collect()
                } else {
                    Vec::new()
                };
                AnnotationPayload::Ink { strokes }
            }
            b"Popup" => AnnotationPayload::Popup {
                open: self.bool(dict, b"Open", false),
            },
            b"FileAttachment" => AnnotationPayload::FileAttachment {
                file: self.file_spec(dict, b"FS"),
                icon: self.name_or(dict, b"Name", "PushPin"),
            },
            b"Sound" => AnnotationPayload::Sound {
                sound: self.linked(dict, b"Sound"),
                icon: self.name_or(dict, b"Name", "Speaker"),
            },
            b"Movie" => {
                let movie = self.key(dict, b"Movie");
                let file = match movie.as_dict() {
                    Some(movie) => self.file_spec(movie, b"F"),
                    None => None,
                };
                AnnotationPayload::Movie {
                    title: self.text(dict, b"T"),
                    movie: self.linked(dict, b"Movie"),
                    file,
                }
            }
            b"Screen" => AnnotationPayload::Screen {
                title: self.text(dict, b"T"),
                has_action: self.has(dict, b"A"),
            },
            b"Widget" => AnnotationPayload::Widget {
                highlight: self.name_or(dict, b"H", "I"),
                has_characteristics: self.has(dict, b"MK"),
                has_action: self.has(dict, b"A"),
            },
            b"PrinterMark" => AnnotationPayload::PrinterMark {
                name: self.name(dict, b"MN"),
            },
            b"TrapNet" => AnnotationPayload::TrapNet {
                last_modified: self.text(dict, b"LastModified"),
                has_version: self.has(dict, b"Version") && self.has(dict, b"AnnotStates"),
            },
            b"Watermark" => AnnotationPayload::Watermark {
                fixed_print: self.has(dict, b"FixedPrint"),
            },
            b"Redact" => AnnotationPayload::Redact {
                quads: self.quads(dict),
                interior_colour: self.colour(dict, b"IC"),
                overlay: self.linked(dict, b"RO"),
                overlay_text: self.text(dict, b"OverlayText"),
                repeat: self.bool(dict, b"Repeat", false),
                default_appearance: self.text(dict, b"DA"),
                quadding: self.int(dict, b"Q", 0),
            },
            b"3D" => AnnotationPayload::ThreeD {
                artwork: self.linked(dict, b"3DD"),
            },
            b"Projection" => AnnotationPayload::Projection,
            b"RichMedia" => AnnotationPayload::RichMedia {
                content: self.linked(dict, b"RichMediaContent"),
                settings: self.linked(dict, b"RichMediaSettings"),
            },
            _ => AnnotationPayload::None,
        }
    }
}
