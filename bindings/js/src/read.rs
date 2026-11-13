//! The read surface beyond pages: `/Info`, the outline, links, attachments,
//! XMP and warnings, and a page's command stream, as JavaScript classes.
//!
//! Each class is the facade's own type with read-only getters (ruling 11):
//! an absent value is `undefined` and an empty one is an empty string, an
//! outline item keeps its children, and a destination keeps its three arms
//! apart (ruling 6). A byte string the file stores as bytes — a destination
//! name, a URI — is a `Uint8Array`, because 12.3.2.3 compares names byte for
//! byte and decoding one would be a guess.
//!
//! [`PdfView`] is the one class both directions share: it is what an explicit
//! destination reads back as, and what `linkToPageView` and
//! `setPageTargetView` take, so a view written and read back is the same
//! object shape.

use js_sys::{Array, BigUint64Array, Float64Array, Function, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use tinker_pdf::{
    Action, CommandBudgets, CommandField, CommandKind, CommandRecord, CommandStream, DestKind,
    Destination, Trapped, Usage,
};

use crate::{PdfBitmap, PdfDocument};

/// The `/Info` dictionary (14.3.3), decoded.
#[wasm_bindgen]
pub struct PdfMetadata {
    inner: tinker_pdf::Metadata,
}

#[wasm_bindgen]
impl PdfMetadata {
    /// `/Title`.
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> Option<String> {
        self.inner.title.clone()
    }

    /// `/Author`.
    #[wasm_bindgen(getter)]
    pub fn author(&self) -> Option<String> {
        self.inner.author.clone()
    }

    /// `/Subject`.
    #[wasm_bindgen(getter)]
    pub fn subject(&self) -> Option<String> {
        self.inner.subject.clone()
    }

    /// `/Keywords`.
    #[wasm_bindgen(getter)]
    pub fn keywords(&self) -> Option<String> {
        self.inner.keywords.clone()
    }

    /// `/Creator`.
    #[wasm_bindgen(getter)]
    pub fn creator(&self) -> Option<String> {
        self.inner.creator.clone()
    }

    /// `/Producer`.
    #[wasm_bindgen(getter)]
    pub fn producer(&self) -> Option<String> {
        self.inner.producer.clone()
    }

    /// `/CreationDate`, as written.
    #[wasm_bindgen(getter, js_name = creationDate)]
    pub fn creation_date(&self) -> Option<String> {
        self.inner.creation_date.clone()
    }

    /// `/ModDate`, as written.
    #[wasm_bindgen(getter, js_name = modificationDate)]
    pub fn modification_date(&self) -> Option<String> {
        self.inner.modification_date.clone()
    }

    /// `"true"`, `"false"` or `"unknown"`; `undefined` when `/Trapped` is
    /// absent, which is not the same answer as `"unknown"`.
    #[wasm_bindgen(getter)]
    pub fn trapped(&self) -> Option<String> {
        self.inner.trapped.map(|trapped| {
            match trapped {
                Trapped::True => "true",
                Trapped::False => "false",
                Trapped::Unknown => "unknown",
            }
            .to_string()
        })
    }
}

/// How a destination positions its page (12.3.2.2, Table 151).
///
/// `kind` is one of `"xyz"`, `"fit"`, `"fith"`, `"fitv"`, `"fitr"`,
/// `"fitb"`, `"fitbh"` and `"fitbv"`; a number the kind does not use is
/// `undefined`, and so is one the file wrote as `null` ("retain the current
/// value"). The static constructors take `undefined` or `null` for `null`.
#[wasm_bindgen]
#[derive(Clone)]
pub struct PdfView {
    inner: DestKind,
}

impl PdfView {
    pub(crate) fn facade(&self) -> DestKind {
        self.inner
    }
}

#[wasm_bindgen]
impl PdfView {
    /// `/XYZ left top zoom`.
    pub fn xyz(left: Option<f64>, top: Option<f64>, zoom: Option<f64>) -> PdfView {
        PdfView {
            inner: DestKind::Xyz { left, top, zoom },
        }
    }

    /// `/Fit`.
    pub fn fit() -> PdfView {
        PdfView {
            inner: DestKind::Fit,
        }
    }

    /// `/FitH top`.
    #[wasm_bindgen(js_name = fitH)]
    pub fn fit_h(top: Option<f64>) -> PdfView {
        PdfView {
            inner: DestKind::FitH { top },
        }
    }

    /// `/FitV left`.
    #[wasm_bindgen(js_name = fitV)]
    pub fn fit_v(left: Option<f64>) -> PdfView {
        PdfView {
            inner: DestKind::FitV { left },
        }
    }

    /// `/FitR left bottom right top`: four numbers, none of them `null`.
    #[wasm_bindgen(js_name = fitR)]
    pub fn fit_r(left: f64, bottom: f64, right: f64, top: f64) -> PdfView {
        PdfView {
            inner: DestKind::FitR {
                left,
                bottom,
                right,
                top,
            },
        }
    }

    /// `/FitB`.
    #[wasm_bindgen(js_name = fitB)]
    pub fn fit_b() -> PdfView {
        PdfView {
            inner: DestKind::FitB,
        }
    }

    /// `/FitBH top`.
    #[wasm_bindgen(js_name = fitBH)]
    pub fn fit_bh(top: Option<f64>) -> PdfView {
        PdfView {
            inner: DestKind::FitBH { top },
        }
    }

    /// `/FitBV left`.
    #[wasm_bindgen(js_name = fitBV)]
    pub fn fit_bv(left: Option<f64>) -> PdfView {
        PdfView {
            inner: DestKind::FitBV { left },
        }
    }

    /// Which of the eight.
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        match self.inner {
            DestKind::Xyz { .. } => "xyz",
            DestKind::Fit => "fit",
            DestKind::FitH { .. } => "fith",
            DestKind::FitV { .. } => "fitv",
            DestKind::FitR { .. } => "fitr",
            DestKind::FitB => "fitb",
            DestKind::FitBH { .. } => "fitbh",
            DestKind::FitBV { .. } => "fitbv",
        }
        .to_string()
    }

    /// The left edge, for `xyz`, `fitv`, `fitbv` and `fitr`.
    #[wasm_bindgen(getter)]
    pub fn left(&self) -> Option<f64> {
        match self.inner {
            DestKind::Xyz { left, .. } | DestKind::FitV { left } | DestKind::FitBV { left } => left,
            DestKind::FitR { left, .. } => Some(left),
            _ => None,
        }
    }

    /// The bottom edge, for `fitr`.
    #[wasm_bindgen(getter)]
    pub fn bottom(&self) -> Option<f64> {
        match self.inner {
            DestKind::FitR { bottom, .. } => Some(bottom),
            _ => None,
        }
    }

    /// The right edge, for `fitr`.
    #[wasm_bindgen(getter)]
    pub fn right(&self) -> Option<f64> {
        match self.inner {
            DestKind::FitR { right, .. } => Some(right),
            _ => None,
        }
    }

    /// The top edge, for `xyz`, `fith`, `fitbh` and `fitr`.
    #[wasm_bindgen(getter)]
    pub fn top(&self) -> Option<f64> {
        match self.inner {
            DestKind::Xyz { top, .. } | DestKind::FitH { top } | DestKind::FitBH { top } => top,
            DestKind::FitR { top, .. } => Some(top),
            _ => None,
        }
    }

    /// The magnification, for `xyz`.
    #[wasm_bindgen(getter)]
    pub fn zoom(&self) -> Option<f64> {
        match self.inner {
            DestKind::Xyz { zoom, .. } => zoom,
            _ => None,
        }
    }
}

/// Where an outline entry or a link goes (12.3.2): `kind` is `"explicit"`
/// (with `pageIndex`, `pageRef` and `view`), `"named"` (with `name`) or
/// `"uri"` (with `uri`). The three are never collapsed (ruling 6).
#[wasm_bindgen]
#[derive(Clone)]
pub struct PdfDestination {
    inner: Destination,
}

#[wasm_bindgen]
impl PdfDestination {
    /// Which of the three.
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        match self.inner {
            Destination::Explicit { .. } => "explicit",
            Destination::Named(_) => "named",
            Destination::Uri(_) => "uri",
        }
        .to_string()
    }

    /// The zero-based page, when the page reference resolved.
    #[wasm_bindgen(getter, js_name = pageIndex)]
    pub fn page_index(&self) -> Option<u32> {
        match self.inner {
            Destination::Explicit { page_index, .. } => page_index,
            _ => None,
        }
    }

    /// `[objectNumber, generation]` of the page the file named, kept whether
    /// or not it resolved.
    #[wasm_bindgen(getter, js_name = pageRef)]
    pub fn page_ref(&self) -> Option<Vec<u32>> {
        match self.inner {
            Destination::Explicit { page_ref, .. } => {
                page_ref.map(|r| vec![r.num, u32::from(r.gen)])
            }
            _ => None,
        }
    }

    /// How the page is positioned, for an explicit destination.
    #[wasm_bindgen(getter)]
    pub fn view(&self) -> Option<PdfView> {
        match self.inner {
            Destination::Explicit { kind, .. } => Some(PdfView { inner: kind }),
            _ => None,
        }
    }

    /// A named destination's name.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<Vec<u8>> {
        match &self.inner {
            Destination::Named(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// A URI destination's URI.
    #[wasm_bindgen(getter)]
    pub fn uri(&self) -> Option<Vec<u8>> {
        match &self.inner {
            Destination::Uri(uri) => Some(uri.clone()),
            _ => None,
        }
    }
}

fn destination(value: Option<&Destination>) -> Option<PdfDestination> {
    value.map(|inner| PdfDestination {
        inner: inner.clone(),
    })
}

/// What a link does (12.6.4): `kind` is `"goto"`, `"gotor"`, `"uri"`,
/// `"named"`, `"launch"` or `"other"`. A `/Launch` is reported, never
/// executed.
#[wasm_bindgen]
pub struct PdfAction {
    inner: Action,
}

#[wasm_bindgen]
impl PdfAction {
    /// Which of the six.
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        match self.inner {
            Action::GoTo(_) => "goto",
            Action::GoToR { .. } => "gotor",
            Action::Uri(_) => "uri",
            Action::Named(_) => "named",
            Action::Launch { .. } => "launch",
            Action::Other { .. } => "other",
        }
        .to_string()
    }

    /// The destination of a `/GoTo`, and of a `/GoToR` that carries one.
    #[wasm_bindgen(getter)]
    pub fn destination(&self) -> Option<PdfDestination> {
        match &self.inner {
            Action::GoTo(dest) => destination(Some(dest)),
            Action::GoToR { dest, .. } => destination(dest.as_ref()),
            _ => None,
        }
    }

    /// A `/URI` action's URI.
    #[wasm_bindgen(getter)]
    pub fn uri(&self) -> Option<Vec<u8>> {
        match &self.inner {
            Action::Uri(uri) => Some(uri.clone()),
            _ => None,
        }
    }

    /// A `/Named` action's viewer command.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<Vec<u8>> {
        match &self.inner {
            Action::Named(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// The file a `/GoToR` or `/Launch` names.
    #[wasm_bindgen(getter)]
    pub fn file(&self) -> Option<Vec<u8>> {
        match &self.inner {
            Action::GoToR { file, .. } | Action::Launch { file } => file.clone(),
            _ => None,
        }
    }

    /// The `/S` of an action type this engine does not model.
    #[wasm_bindgen(getter)]
    pub fn subtype(&self) -> Option<Vec<u8>> {
        match &self.inner {
            Action::Other { subtype } => Some(subtype.clone()),
            _ => None,
        }
    }
}

/// One entry of the outline tree (12.3.3), with its children.
#[wasm_bindgen]
pub struct PdfOutlineItem {
    inner: tinker_pdf::OutlineItem,
}

#[wasm_bindgen]
impl PdfOutlineItem {
    /// The visible text, decoded.
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.inner.title.clone()
    }

    /// Whether the entry was saved expanded (`/Count` positive).
    #[wasm_bindgen(getter)]
    pub fn open(&self) -> bool {
        self.inner.open
    }

    /// Where it goes; `undefined` for an entry that is only a heading.
    #[wasm_bindgen(getter)]
    pub fn destination(&self) -> Option<PdfDestination> {
        destination(self.inner.destination.as_ref())
    }

    /// Nested entries.
    #[wasm_bindgen(getter)]
    pub fn children(&self) -> Vec<PdfOutlineItem> {
        self.inner
            .children
            .iter()
            .map(|child| PdfOutlineItem {
                inner: child.clone(),
            })
            .collect()
    }
}

/// One link annotation (12.5.6.5).
#[wasm_bindgen]
pub struct PdfLink {
    inner: tinker_pdf::Link,
}

#[wasm_bindgen]
impl PdfLink {
    /// `[x0, y0, x1, y1]`, corners ordered.
    #[wasm_bindgen(getter)]
    pub fn rect(&self) -> Vec<f64> {
        let r = self.inner.rect;
        vec![r.x0, r.y0, r.x1, r.y1]
    }

    /// `[objectNumber, generation]` when `/Annots` named it indirectly.
    #[wasm_bindgen(getter)]
    pub fn reference(&self) -> Option<Vec<u32>> {
        self.inner.reference.map(|r| vec![r.num, u32::from(r.gen)])
    }

    /// What it does; `undefined` for a link with neither `/Dest` nor a
    /// usable `/A`.
    #[wasm_bindgen(getter)]
    pub fn action(&self) -> Option<PdfAction> {
        self.inner.target.clone().map(|inner| PdfAction { inner })
    }
}

/// One file attached to the document (7.11.4).
///
/// Listing reads no bytes; `data()` reads them, through the stream reference
/// the facade hands back and `CosDocument::stream_decoded`, which is the
/// route the facade documents.
#[wasm_bindgen]
pub struct PdfAttachment {
    document: tinker_pdf::Document,
    inner: tinker_pdf::Attachment,
}

#[wasm_bindgen]
impl PdfAttachment {
    /// The name it is filed under in `/Names /EmbeddedFiles`.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.inner.name.clone()
    }

    /// `/UF` or `/F`: the filename to offer when saving it out.
    #[wasm_bindgen(getter)]
    pub fn filename(&self) -> String {
        self.inner.filename.clone()
    }

    /// `/Desc`.
    #[wasm_bindgen(getter)]
    pub fn description(&self) -> Option<String> {
        self.inner.description.clone()
    }

    /// `/Params /Size`, a `BigInt`. Advisory: the stream is the truth.
    ///
    /// The facade's `i64`, as every `i64` here crosses: the number is the one
    /// the document wrote, which nothing bounds by what memory holds, and a
    /// JavaScript number would round one past 2^53 without a word where
    /// every other surface returns it exactly (review of lane 7C).
    #[wasm_bindgen(getter)]
    pub fn size(&self) -> Option<i64> {
        self.inner.size
    }

    /// `[objectNumber, generation]` of the embedded file stream.
    #[wasm_bindgen(getter)]
    pub fn stream(&self) -> Option<Vec<u32>> {
        self.inner.stream.map(|r| vec![r.num, u32::from(r.gen)])
    }

    /// The file's bytes, decoded; `undefined` when the specification names no
    /// stream. Throws when the stream is there and cannot be read — "nothing
    /// here" and "something here, unreadable" differ.
    pub fn data(&self) -> Result<Option<Vec<u8>>, JsError> {
        let Some(stream) = self.inner.stream else {
            return Ok(None);
        };
        self.document
            .cos()
            .stream_decoded(stream)
            .map(Some)
            .map_err(|error| {
                JsError::new(&format!(
                    "attachment {:?}: {} {} R: {error}",
                    self.inner.name, stream.num, stream.gen
                ))
            })
    }
}

/// One thing the engine tolerated (ruling 10).
#[wasm_bindgen]
pub struct PdfWarning {
    inner: tinker_pdf::Warning,
}

#[wasm_bindgen]
impl PdfWarning {
    /// The byte offset that triggered it.
    #[wasm_bindgen(getter)]
    pub fn offset(&self) -> f64 {
        // Exact below 2^53 bytes, which no document held in wasm reaches.
        self.inner.offset as f64
    }

    /// `[objectNumber, generation]` being read when it happened, if known.
    #[wasm_bindgen(getter)]
    pub fn object(&self) -> Option<Vec<u32>> {
        self.inner.object.map(|r| vec![r.num, u32::from(r.gen)])
    }

    /// The stable identifier, such as `"header-not-at-start"`.
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.inner.kind.as_str().to_string()
    }

    /// The facade's own sentence for it.
    #[wasm_bindgen(getter)]
    pub fn message(&self) -> String {
        self.inner.kind.to_string()
    }
}

#[wasm_bindgen]
impl PdfDocument {
    /// The `/Info` dictionary (14.3.3), decoded: absent entries are
    /// `undefined`, empty ones `""`.
    #[wasm_bindgen(getter)]
    pub fn metadata(&self) -> PdfMetadata {
        PdfMetadata {
            inner: self.inner.metadata(),
        }
    }

    /// The version, as `"PDF 1.7"`: the later of the header's and the
    /// catalog's, never absent.
    #[wasm_bindgen(getter, js_name = pdfVersion)]
    pub fn pdf_version(&self) -> String {
        self.inner.pdf_version()
    }

    /// Every page's label (12.4.2), or an empty array when the document
    /// defines none.
    #[wasm_bindgen(js_name = pageLabels)]
    pub fn page_labels(&self) -> Vec<String> {
        self.inner.page_labels()
    }

    /// The outline tree (12.3.3); empty when the document has none.
    #[wasm_bindgen]
    pub fn outline(&self) -> Vec<PdfOutlineItem> {
        self.inner
            .outline()
            .into_iter()
            .map(|inner| PdfOutlineItem { inner })
            .collect()
    }

    /// A page's link annotations, in `/Annots` order (12.5.6.5).
    #[wasm_bindgen]
    pub fn links(&self, index: u32) -> Result<Vec<PdfLink>, JsError> {
        let page = self
            .inner
            .page(index)
            .ok_or_else(|| JsError::new("no such page"))?;
        Ok(page
            .links()
            .into_iter()
            .map(|inner| PdfLink { inner })
            .collect())
    }

    /// Every file attached to the document (7.11.4), in name order.
    #[wasm_bindgen]
    pub fn attachments(&self) -> Vec<PdfAttachment> {
        self.inner
            .attachments()
            .into_iter()
            .map(|inner| PdfAttachment {
                document: self.inner.clone(),
                inner,
            })
            .collect()
    }

    /// The XMP packet (14.3.2), unparsed, or `undefined`.
    #[wasm_bindgen(js_name = xmpMetadata)]
    pub fn xmp_metadata(&self) -> Option<Vec<u8>> {
        self.inner.xmp_metadata()
    }

    /// Everything the engine has tolerated so far, in order (ruling 10).
    /// Reading a page can tolerate more, so asking again later may answer
    /// with more.
    #[wasm_bindgen]
    pub fn warnings(&self) -> Vec<PdfWarning> {
        self.inner
            .warnings()
            .into_iter()
            .map(|inner| PdfWarning { inner })
            .collect()
    }
}

// ---- the command stream (`docs/design/command-stream.md`, RC-18.6) ------------

/// A kind of command-stream record (`docs/design/command-stream.md`, "The
/// records"): its number on the wire, its name, and what each of its fields
/// holds, in their places.
#[wasm_bindgen]
#[derive(Clone)]
pub struct PdfCommandKind {
    inner: CommandKind,
}

#[wasm_bindgen]
impl PdfCommandKind {
    /// Every kind this build knows, in the order of their numbers.
    #[wasm_bindgen]
    pub fn all() -> Vec<PdfCommandKind> {
        CommandKind::ALL
            .iter()
            .map(|&inner| PdfCommandKind { inner })
            .collect()
    }

    /// The kind a wire number names, or `undefined` for a number this build
    /// does not know.
    #[wasm_bindgen(js_name = fromNumber)]
    pub fn from_number(number: u32) -> Option<PdfCommandKind> {
        CommandKind::from_number(number).map(|inner| PdfCommandKind { inner })
    }

    /// The kind's number on the wire.
    #[wasm_bindgen(getter)]
    pub fn number(&self) -> u32 {
        self.inner.number()
    }

    /// The kind's name, as the design names it: `"GlyphRun"`.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.inner.name().to_string()
    }

    /// What each field holds, in their order.
    #[wasm_bindgen(getter)]
    pub fn fields(&self) -> Vec<String> {
        self.inner
            .fields()
            .iter()
            .map(|field| (*field).to_string())
            .collect()
    }

    /// Whether the kind is the codec's — a command, or a definition a state
    /// is made of — rather than the facade's page and tables.
    #[wasm_bindgen(getter, js_name = isCodec)]
    pub fn is_codec(&self) -> bool {
        self.inner.is_codec()
    }

    /// Whether the kind is one of a page's recorded calls.
    #[wasm_bindgen(getter, js_name = isCommand)]
    pub fn is_command(&self) -> bool {
        self.inner.is_command()
    }

    /// The device method a replay calls for a record of this kind: its name
    /// in lower camel case, `"glyphRun"`.
    #[wasm_bindgen(getter)]
    pub fn method(&self) -> String {
        camel_case(self.inner.name())
    }
}

/// `name`, which is in upper camel case, in lower camel case.
fn camel_case(name: &str) -> String {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}

/// One field of a record (`docs/design/command-stream.md`, "The form"): its
/// tag, and its value as JavaScript holds it.
///
/// The tag is kept beside the value, because two fields JavaScript would hold
/// alike are written apart: an unsigned and a signed integer, each a
/// `BigInt`, and an absent value and an empty one. Each tag has its
/// constructor, so a record built here writes the bytes the one read was
/// written with. A real is a `number`, its bits as written.
#[wasm_bindgen]
#[derive(Clone)]
pub struct PdfCommandField {
    inner: CommandField,
}

#[wasm_bindgen]
impl PdfCommandField {
    /// An absent value, tag 0.
    #[wasm_bindgen]
    pub fn nothing() -> PdfCommandField {
        PdfCommandField {
            inner: CommandField::Nothing,
        }
    }

    /// An unsigned integer, tag 1: a `BigInt`.
    #[wasm_bindgen]
    pub fn unsigned(value: u64) -> PdfCommandField {
        PdfCommandField {
            inner: CommandField::Unsigned(value),
        }
    }

    /// A signed integer, tag 2: a `BigInt`.
    #[wasm_bindgen]
    pub fn signed(value: i64) -> PdfCommandField {
        PdfCommandField {
            inner: CommandField::Signed(value),
        }
    }

    /// A real, tag 3.
    #[wasm_bindgen]
    pub fn real(value: f64) -> PdfCommandField {
        PdfCommandField {
            inner: CommandField::Real(value),
        }
    }

    /// Bytes, tag 4: a `Uint8Array`.
    #[wasm_bindgen]
    pub fn bytes(value: &[u8]) -> PdfCommandField {
        PdfCommandField {
            inner: CommandField::bytes(value),
        }
    }

    /// Reals, tag 5: a `Float64Array`.
    #[wasm_bindgen]
    pub fn reals(value: Vec<f64>) -> PdfCommandField {
        PdfCommandField {
            inner: CommandField::Reals(value),
        }
    }

    /// Unsigned integers, tag 6: a `BigUint64Array`.
    #[wasm_bindgen]
    pub fn naturals(value: Vec<u64>) -> PdfCommandField {
        PdfCommandField {
            inner: CommandField::Naturals(value),
        }
    }

    /// Byte strings, tag 7: an array of `Uint8Array`s.
    ///
    /// # Errors
    ///
    /// An element that is no `Uint8Array`, named by its place.
    #[wasm_bindgen]
    pub fn strings(value: &Array) -> Result<PdfCommandField, JsError> {
        let mut strings = Vec::with_capacity(value.length() as usize);
        for (at, element) in value.iter().enumerate() {
            let Some(bytes) = element.dyn_ref::<Uint8Array>() else {
                return Err(JsError::new(&format!(
                    "string {at} of a strings field is no Uint8Array"
                )));
            };
            strings.push(bytes.to_vec());
        }
        Ok(PdfCommandField {
            inner: CommandField::Strings(strings),
        })
    }

    /// The tag's name, its constructor's: `"nothing"`, `"unsigned"`,
    /// `"signed"`, `"real"`, `"bytes"`, `"reals"`, `"naturals"` or
    /// `"strings"`.
    #[wasm_bindgen(getter, js_name = tagName)]
    pub fn tag_name(&self) -> String {
        match self.inner {
            CommandField::Nothing => "nothing",
            CommandField::Unsigned(_) => "unsigned",
            CommandField::Signed(_) => "signed",
            CommandField::Real(_) => "real",
            CommandField::Bytes(_) => "bytes",
            CommandField::Reals(_) => "reals",
            CommandField::Naturals(_) => "naturals",
            CommandField::Strings(_) => "strings",
        }
        .to_string()
    }

    /// The tag's number on the wire, 0 to 7.
    #[wasm_bindgen(getter)]
    pub fn tag(&self) -> u8 {
        self.inner.tag()
    }

    /// The value: `undefined`, a `BigInt`, a `number`, a `Uint8Array`, a
    /// `Float64Array`, a `BigUint64Array`, or an array of `Uint8Array`s.
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> JsValue {
        match &self.inner {
            CommandField::Nothing => JsValue::UNDEFINED,
            CommandField::Unsigned(value) => JsValue::from(*value),
            CommandField::Signed(value) => JsValue::from(*value),
            CommandField::Real(value) => JsValue::from_f64(*value),
            CommandField::Bytes(bytes) => Uint8Array::from(&bytes[..]).into(),
            CommandField::Reals(reals) => Float64Array::from(&reals[..]).into(),
            CommandField::Naturals(naturals) => BigUint64Array::from(&naturals[..]).into(),
            CommandField::Strings(strings) => strings
                .iter()
                .map(|string| JsValue::from(Uint8Array::from(&string[..])))
                .collect::<Array>()
                .into(),
        }
    }

    /// Whether the two encode to the same bytes: a real by its bits.
    #[wasm_bindgen]
    pub fn equals(&self, other: &PdfCommandField) -> bool {
        self.inner == other.inner
    }
}

/// One record of a command stream: its kind's number and its fields, in the
/// places its kind names. A record of a kind this build does not know — one
/// a newer minor version added — keeps its number and fields, and names no
/// kind.
#[wasm_bindgen]
#[derive(Clone)]
pub struct PdfCommandRecord {
    inner: CommandRecord,
}

#[wasm_bindgen]
impl PdfCommandRecord {
    /// A record of the kind numbered `kind` holding `fields`, in their
    /// places, for `PdfCommandStream.fromRecords`.
    #[wasm_bindgen(constructor)]
    pub fn new(kind: u32, fields: Vec<PdfCommandField>) -> PdfCommandRecord {
        PdfCommandRecord {
            inner: CommandRecord {
                kind,
                fields: fields.into_iter().map(|field| field.inner).collect(),
            },
        }
    }

    /// The kind's number on the wire.
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> u32 {
        self.inner.kind
    }

    /// The kind, or `undefined` for a number this build does not know.
    #[wasm_bindgen(getter, js_name = commandKind)]
    pub fn command_kind(&self) -> Option<PdfCommandKind> {
        self.inner
            .command_kind()
            .map(|inner| PdfCommandKind { inner })
    }

    /// The kind's name, or `undefined` for a number this build does not
    /// know.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<String> {
        self.inner
            .command_kind()
            .map(|kind| kind.name().to_string())
    }

    /// What each field holds, by its kind; empty for a kind this build does
    /// not know.
    #[wasm_bindgen(getter, js_name = fieldNames)]
    pub fn field_names(&self) -> Vec<String> {
        self.inner.command_kind().map_or_else(Vec::new, |kind| {
            kind.fields()
                .iter()
                .map(|field| (*field).to_string())
                .collect()
        })
    }

    /// The fields, in their places.
    #[wasm_bindgen(getter)]
    pub fn fields(&self) -> Vec<PdfCommandField> {
        self.inner
            .fields
            .iter()
            .map(|inner| PdfCommandField {
                inner: inner.clone(),
            })
            .collect()
    }

    /// The field in the place the kind names `name`, or `undefined` where the
    /// kind names no such place or the record holds no field there.
    #[wasm_bindgen]
    pub fn field(&self, name: &str) -> Option<PdfCommandField> {
        let kind = self.inner.command_kind()?;
        let place = kind.fields().iter().position(|field| *field == name)?;
        self.inner.fields.get(place).map(|inner| PdfCommandField {
            inner: inner.clone(),
        })
    }

    /// Whether the two write the same bytes.
    #[wasm_bindgen]
    pub fn equals(&self, other: &PdfCommandRecord) -> bool {
        self.inner == other.inner
    }
}

/// The most a command stream may make a reader hold
/// (`docs/design/command-stream.md`, "The reader's budgets"), each starting at
/// the design's default.
#[wasm_bindgen]
pub struct PdfCommandBudgets {
    inner: CommandBudgets,
}

#[wasm_bindgen]
impl PdfCommandBudgets {
    /// The design's defaults.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PdfCommandBudgets {
        PdfCommandBudgets {
            inner: CommandBudgets::DEFAULT,
        }
    }

    /// Records in a stream, nested streams' included: a `BigInt`.
    #[wasm_bindgen(getter)]
    pub fn records(&self) -> u64 {
        self.inner.records
    }

    #[wasm_bindgen(setter)]
    pub fn set_records(&mut self, value: u64) {
        self.inner.records = value;
    }

    /// Bytes the read holds: a `BigInt`.
    #[wasm_bindgen(getter)]
    pub fn bytes(&self) -> u64 {
        self.inner.bytes
    }

    #[wasm_bindgen(setter)]
    pub fn set_bytes(&mut self, value: u64) {
        self.inner.bytes = value;
    }

    /// Streams nested in streams: a tiling cell's.
    #[wasm_bindgen(getter)]
    pub fn depth(&self) -> u32 {
        self.inner.depth
    }

    #[wasm_bindgen(setter)]
    pub fn set_depth(&mut self, value: u32) {
        self.inner.depth = value;
    }

    /// Functions nested in functions, and a calculator's blocks in blocks.
    #[wasm_bindgen(getter)]
    pub fn functions(&self) -> u32 {
        self.inner.functions
    }

    #[wasm_bindgen(setter)]
    pub fn set_functions(&mut self, value: u32) {
        self.inner.functions = value;
    }

    /// Colour spaces nested in colour spaces, and halftones in halftones.
    #[wasm_bindgen(getter)]
    pub fn spaces(&self) -> u32 {
        self.inner.spaces
    }

    #[wasm_bindgen(setter)]
    pub fn set_spaces(&mut self, value: u32) {
        self.inner.spaces = value;
    }
}

impl Default for PdfCommandBudgets {
    fn default() -> Self {
        Self::new()
    }
}

/// A page's drawing read from its command stream
/// (`docs/design/command-stream.md`): its records in order, the bytes they
/// write back, and its replay through the engine's own renderer.
///
/// `replay(device)` hands every record, in the stream's order, to the
/// device's method named for its kind in lower camel case
/// (`PdfCommandKind.method`: `glyphRun`, `beginGroup`, `imageDecode`), a
/// record of a kind this build does not know to its `unknown`; a device
/// without the method for a kind is not called for it. A refusal of the
/// bytes is an `Error` with the reader's sentence.
#[wasm_bindgen]
pub struct PdfCommandStream {
    inner: CommandStream,
}

/// The reader's refusal, as JavaScript throws it.
fn unread(error: tinker_pdf::CommandStreamError) -> JsError {
    JsError::new(&error.to_string())
}

#[wasm_bindgen]
impl PdfCommandStream {
    /// Reads a stream under the design's budgets.
    #[wasm_bindgen]
    pub fn parse(bytes: &[u8]) -> Result<PdfCommandStream, JsError> {
        CommandStream::parse(bytes)
            .map(|inner| PdfCommandStream { inner })
            .map_err(unread)
    }

    /// Reads a stream under `budgets`.
    #[wasm_bindgen(js_name = parseWith)]
    pub fn parse_with(
        bytes: &[u8],
        budgets: &PdfCommandBudgets,
    ) -> Result<PdfCommandStream, JsError> {
        CommandStream::parse_with(bytes, &budgets.inner)
            .map(|inner| PdfCommandStream { inner })
            .map_err(unread)
    }

    /// The stream `records` write at this build's version, read back as a
    /// parse reads it: the encoder.
    #[wasm_bindgen(js_name = fromRecords)]
    pub fn from_records(records: Vec<PdfCommandRecord>) -> Result<PdfCommandStream, JsError> {
        let records: Vec<CommandRecord> = records.into_iter().map(|record| record.inner).collect();
        CommandStream::from_records(&records)
            .map(|inner| PdfCommandStream { inner })
            .map_err(unread)
    }

    /// Every record before the stream's end, in order.
    #[wasm_bindgen]
    pub fn records(&self) -> Vec<PdfCommandRecord> {
        self.inner
            .records()
            .iter()
            .map(|inner| PdfCommandRecord {
                inner: inner.clone(),
            })
            .collect()
    }

    /// How many records the stream holds before its end.
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> u32 {
        u32::try_from(self.inner.records().len()).unwrap_or(u32::MAX)
    }

    /// The stream's bytes, written back at its own version.
    #[wasm_bindgen(js_name = toBytes)]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes()
    }

    /// The stream's major and minor version, as `[major, minor]`.
    #[wasm_bindgen(getter)]
    pub fn version(&self) -> Vec<u16> {
        let (major, minor) = self.inner.version();
        vec![major, minor]
    }

    /// The usage whose optional-content states the stream's calls carry —
    /// `"view"`, `"print"` or `"export"` — or `undefined` for none.
    #[wasm_bindgen(getter)]
    pub fn usage(&self) -> Option<String> {
        self.inner.usage().map(|usage| {
            match usage {
                Usage::View => "view",
                Usage::Print => "print",
                Usage::Export => "export",
            }
            .to_string()
        })
    }

    /// Which page of its document the stream is of.
    #[wasm_bindgen(getter, js_name = pageIndex)]
    pub fn page_index(&self) -> u32 {
        self.inner.page_index()
    }

    /// The page's size in points after rotation, as `[width, height]`.
    #[wasm_bindgen]
    pub fn size(&self) -> Vec<f64> {
        let (width, height) = self.inner.size();
        vec![width, height]
    }

    /// The size in pixels a render at `scale` draws, where 1.0 is 72 dpi, as
    /// `[width, height]`.
    #[wasm_bindgen(js_name = pixelSize)]
    pub fn pixel_size(&self, scale: f64) -> Vec<u32> {
        let (width, height) = self.inner.pixel_size(&render_options(scale));
        vec![width, height]
    }

    /// Draws the page at a scale, where 1.0 is 72 dpi, as a render of it
    /// draws: the same pixels and the same warnings.
    #[wasm_bindgen]
    pub fn render(&self, scale: f64) -> PdfBitmap {
        PdfBitmap {
            inner: self.inner.render(&render_options(scale)),
        }
    }

    /// Hands every record, in the stream's order, to the method of `device`
    /// named for its kind (the class documentation), and a record of a kind
    /// this build does not know to its `unknown`.
    ///
    /// # Errors
    ///
    /// What a method throws, which ends the replay.
    #[wasm_bindgen]
    pub fn replay(&self, device: &JsValue) -> Result<(), JsValue> {
        for record in self.inner.records() {
            let method = record
                .command_kind()
                .map_or_else(|| "unknown".to_string(), |kind| camel_case(kind.name()));
            let handler = Reflect::get(device, &JsValue::from_str(&method))?;
            if let Some(handler) = handler.dyn_ref::<Function>() {
                let argument = JsValue::from(PdfCommandRecord {
                    inner: record.clone(),
                });
                handler.call1(device, &argument)?;
            }
        }
        Ok(())
    }
}

/// A render at `scale`, into RGBA as every render of this binding draws.
fn render_options(scale: f64) -> tinker_pdf::RenderOptions {
    tinker_pdf::RenderOptions {
        scale,
        format: tinker_pdf::PixelFormat::Rgba8,
        ..tinker_pdf::RenderOptions::default()
    }
}

#[wasm_bindgen]
impl PdfDocument {
    /// A page's drawing as a command stream (`Page::command_stream`,
    /// `docs/design/command-stream.md`): versioned bytes that
    /// `PdfCommandStream` reads back, draws and writes again.
    ///
    /// # Errors
    ///
    /// A page index past the document's last.
    #[wasm_bindgen(js_name = commandStream)]
    pub fn command_stream(&self, index: u32) -> Result<Vec<u8>, JsError> {
        let page = self
            .inner
            .page(index)
            .ok_or_else(|| JsError::new("no such page"))?;
        Ok(page.command_stream())
    }
}
