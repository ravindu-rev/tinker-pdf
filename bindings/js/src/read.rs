//! The read surface beyond pages: `/Info`, the outline, links, attachments,
//! XMP and warnings, as JavaScript classes.
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

use wasm_bindgen::prelude::*;

use tinker_pdf::{Action, DestKind, Destination, Trapped};

use crate::PdfDocument;

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

    /// `/Params /Size`. Advisory: the stream is the truth.
    #[wasm_bindgen(getter)]
    pub fn size(&self) -> Option<f64> {
        // A JavaScript number holds every size a document can declare and
        // still be held in memory; the facade's i64 is exact below 2^53.
        self.inner.size.map(|size| size as f64)
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
