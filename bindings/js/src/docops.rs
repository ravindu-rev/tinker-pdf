//! The editor's document operations: page labels, embedded files, the
//! outline, the typed `/Info` setters and the XMP packet, the page
//! boundaries, and `sanitise` with its report.
//!
//! One facade call each (ruling 11). A metadata write answers with what it
//! did to the other statement of the same metadata, `"alone"` or
//! `"other-half-unchanged"`. A date is an array `[year, month, day, hour,
//! minute, second]`, with a seventh element for the offset from UT in minutes
//! when the date states one — six elements is an unspecified zone, which is a
//! different date from one at UT.

use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use tinker_pdf::{
    Date, EntryHolder, LabelStyle, MetadataSync, PageBoundary, PageLabelRange, PathStep, Removal,
    SanitiseReport, Trapped,
};

use crate::{refused, PdfDocument, PdfEditor, PdfOutlineEntry};

/// One run of page labels (12.4.2), for `setPageLabels`.
#[wasm_bindgen]
pub struct PdfPageLabelRange {
    inner: PageLabelRange,
}

#[wasm_bindgen]
impl PdfPageLabelRange {
    /// `style` is `"decimal"`, `"roman-upper"`, `"roman-lower"`,
    /// `"letters-upper"`, `"letters-lower"` or `"none"`; `prefix` is
    /// `undefined` for no `/P`; `start` is `/St`, at least 1.
    #[wasm_bindgen(constructor)]
    pub fn new(
        first_page: u32,
        style: &str,
        prefix: Option<String>,
        start: u32,
    ) -> Result<PdfPageLabelRange, JsError> {
        let style = match style {
            "decimal" => LabelStyle::Decimal,
            "roman-upper" => LabelStyle::RomanUpper,
            "roman-lower" => LabelStyle::RomanLower,
            "letters-upper" => LabelStyle::LettersUpper,
            "letters-lower" => LabelStyle::LettersLower,
            "none" => LabelStyle::None,
            other => {
                return Err(JsError::new(&format!(
                    "style must be decimal, roman-upper, roman-lower, letters-upper, \
                     letters-lower or none, not {other:?}"
                )))
            }
        };
        Ok(PdfPageLabelRange {
            inner: PageLabelRange {
                first_page,
                style,
                prefix,
                start,
            },
        })
    }
}

fn date(fields: &[i32], what: &str) -> Result<Date, JsError> {
    let byte = |value: i32| u8::try_from(value).ok();
    let (year, rest, offset) = match fields {
        [year, rest @ ..] if rest.len() == 5 => (*year, rest, None),
        [year, rest @ .., offset] if rest.len() == 5 => (*year, rest, Some(*offset)),
        _ => {
            return Err(JsError::new(&format!(
                "{what} is [year, month, day, hour, minute, second] with an optional \
                 seventh element for the offset in minutes"
            )))
        }
    };
    let parts: Option<Vec<u8>> = rest.iter().map(|value| byte(*value)).collect();
    match parts.as_deref() {
        Some(&[month, day, hour, minute, second]) => Ok(Date {
            year,
            month,
            day,
            hour,
            minute,
            second,
            utc_offset_minutes: offset,
        }),
        _ => Err(JsError::new(&format!("{what} has a field out of range"))),
    }
}

fn sync(sync: MetadataSync) -> String {
    match sync {
        MetadataSync::Alone => "alone",
        MetadataSync::OtherHalfUnchanged => "other-half-unchanged",
    }
    .to_string()
}

fn boundary(name: &str) -> Result<PageBoundary, JsError> {
    Ok(match name {
        "media" => PageBoundary::MediaBox,
        "crop" => PageBoundary::CropBox,
        "bleed" => PageBoundary::BleedBox,
        "trim" => PageBoundary::TrimBox,
        "art" => PageBoundary::ArtBox,
        other => {
            return Err(JsError::new(&format!(
                "boundary must be media, crop, bleed, trim or art, not {other:?}"
            )))
        }
    })
}

fn removal(what: &Removal) -> &'static str {
    match what {
        Removal::JavaScript => "javascript",
        Removal::DocumentJavaScript => "document-javascript",
        Removal::CalculationOrder => "calculation-order",
        Removal::XfaForm => "xfa-form",
        Removal::Action(_) => "action",
        Removal::EmbeddedFileTree => "embedded-file-tree",
        Removal::EmbeddedFile => "embedded-file",
        Removal::Info => "info",
        Removal::Metadata => "metadata",
    }
}

fn reference(num: u32, gen: u16) -> JsValue {
    let pair = Array::new();
    pair.push(&JsValue::from(num));
    pair.push(&JsValue::from(gen));
    pair.into()
}

fn set(object: &Object, key: &str, value: &JsValue) -> Result<(), JsError> {
    Reflect::set(object, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|_| JsError::new("could not build the report object"))
}

fn action(what: &Removal) -> JsValue {
    match what {
        Removal::Action(subtype) => Uint8Array::from(subtype.as_slice()).into(),
        _ => JsValue::UNDEFINED,
    }
}

/// Everything a sanitise took out.
///
/// `removed` is an array of `{holder, path, what, action}`: `holder` is
/// `[objectNumber, generation]`, or `undefined` for the trailer; `path` the
/// keys (`Uint8Array`) and array positions (numbers, counted in the array as
/// it was) from the holder down to the removed value; `what` why —
/// `"javascript"`, `"document-javascript"`, `"calculation-order"`,
/// `"xfa-form"`, `"action"`, `"embedded-file-tree"`, `"embedded-file"`,
/// `"info"` or `"metadata"`; and `action` the `/S` of an `"action"`.
/// `deleted` is an array of `{object, what, action}`.
#[wasm_bindgen]
pub struct PdfSanitiseReport {
    inner: SanitiseReport,
}

#[wasm_bindgen]
impl PdfSanitiseReport {
    /// Entries removed from objects that stay, and from the trailer.
    #[wasm_bindgen(getter)]
    pub fn removed(&self) -> Result<Array, JsError> {
        let out = Array::new();
        for entry in &self.inner.removed {
            let object = Object::new();
            let holder = match entry.holder {
                EntryHolder::Trailer => JsValue::UNDEFINED,
                EntryHolder::Object(r) => reference(r.num, r.gen),
            };
            set(&object, "holder", &holder)?;
            let path = Array::new();
            for step in &entry.path {
                match step {
                    PathStep::Key(key) => path.push(&Uint8Array::from(key.as_slice()).into()),
                    // An array position: exact in a JavaScript number far
                    // beyond any array this engine reads.
                    PathStep::Index(at) => path.push(&JsValue::from_f64(*at as f64)),
                };
            }
            set(&object, "path", &path.into())?;
            set(&object, "what", &JsValue::from_str(removal(&entry.what)))?;
            set(&object, "action", &action(&entry.what))?;
            out.push(&object.into());
        }
        Ok(out)
    }

    /// Objects deleted because only removed entries reached them.
    #[wasm_bindgen(getter)]
    pub fn deleted(&self) -> Result<Array, JsError> {
        let out = Array::new();
        for entry in &self.inner.deleted {
            let object = Object::new();
            set(
                &object,
                "object",
                &reference(entry.object.num, entry.object.gen),
            )?;
            set(&object, "what", &JsValue::from_str(removal(&entry.what)))?;
            set(&object, "action", &action(&entry.what))?;
            out.push(&object.into());
        }
        Ok(out)
    }

    /// Whether the pass found nothing to take out.
    #[wasm_bindgen(js_name = isEmpty)]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[wasm_bindgen]
impl PdfEditor {
    /// Sets the page labels (12.4.2), replacing any, **consuming** each
    /// range. Throws with the facade's own reason, writing nothing, when the
    /// ranges are refused.
    #[wasm_bindgen(js_name = setPageLabels)]
    pub fn set_page_labels(&mut self, ranges: Vec<PdfPageLabelRange>) -> Result<(), JsError> {
        let ranges: Vec<PageLabelRange> = ranges.into_iter().map(|r| r.inner).collect();
        self.inner
            .set_page_labels(&ranges)
            .map_err(|e| refused("setPageLabels", &e.to_string()))
    }

    /// Embeds a file (7.11.4) and returns its file specification's
    /// `[objectNumber, generation]`.
    #[wasm_bindgen(js_name = attachFile)]
    #[allow(clippy::too_many_arguments)]
    pub fn attach_file(
        &mut self,
        name: String,
        filename: String,
        data: &[u8],
        description: Option<String>,
        mime_type: Option<String>,
        created: Option<Vec<i32>>,
        modified: Option<Vec<i32>>,
    ) -> Result<Vec<u32>, JsError> {
        let created = created.map(|d| date(&d, "created")).transpose()?;
        let modified = modified.map(|d| date(&d, "modified")).transpose()?;
        let file = tinker_pdf::EmbeddedFile {
            name,
            filename,
            description,
            mime_type,
            created,
            modified,
            data: data.to_vec(),
        };
        self.inner
            .attach_file(&file)
            .map(|r| vec![r.num, u32::from(r.gen)])
            .map_err(|e| refused("attachFile", &e.to_string()))
    }

    /// Replaces the outline (12.3.3), **consuming** each entry.
    #[wasm_bindgen(js_name = setOutline)]
    pub fn set_outline(&mut self, entries: Vec<PdfOutlineEntry>) -> Result<(), JsError> {
        let mut taken = Vec::with_capacity(entries.len());
        for mut entry in entries {
            let Some(one) = entry.inner.take() else {
                return Err(JsError::new(
                    "this outline entry was already added to a tree",
                ));
            };
            taken.push(one);
        }
        if self.inner.set_outline(&taken) {
            Ok(())
        } else {
            Err(refused(
                "setOutline",
                "the tree is deeper or wider than this engine's own reader walks",
            ))
        }
    }

    /// Sets `/Info /Title`; answers `"alone"` or `"other-half-unchanged"`.
    #[wasm_bindgen(js_name = setTitle)]
    pub fn set_title(&mut self, value: &str) -> String {
        sync(self.inner.set_title(value))
    }

    /// Sets `/Info /Author`.
    #[wasm_bindgen(js_name = setAuthor)]
    pub fn set_author(&mut self, value: &str) -> String {
        sync(self.inner.set_author(value))
    }

    /// Sets `/Info /Subject`.
    #[wasm_bindgen(js_name = setSubject)]
    pub fn set_subject(&mut self, value: &str) -> String {
        sync(self.inner.set_subject(value))
    }

    /// Sets `/Info /Keywords`.
    #[wasm_bindgen(js_name = setKeywords)]
    pub fn set_keywords(&mut self, value: &str) -> String {
        sync(self.inner.set_keywords(value))
    }

    /// Sets `/Info /Creator`.
    #[wasm_bindgen(js_name = setCreator)]
    pub fn set_creator(&mut self, value: &str) -> String {
        sync(self.inner.set_creator(value))
    }

    /// Sets `/Info /Producer`.
    #[wasm_bindgen(js_name = setProducer)]
    pub fn set_producer(&mut self, value: &str) -> String {
        sync(self.inner.set_producer(value))
    }

    /// Sets `/Info /CreationDate`; throws when the date cannot be spelled.
    #[wasm_bindgen(js_name = setCreationDate)]
    pub fn set_creation_date(&mut self, value: Vec<i32>) -> Result<String, JsError> {
        let value = date(&value, "the date")?;
        self.inner
            .set_creation_date(value)
            .map(sync)
            .ok_or_else(|| refused("setCreationDate", &format!("{value:?}")))
    }

    /// Sets `/Info /ModDate`; throws when the date cannot be spelled.
    #[wasm_bindgen(js_name = setModificationDate)]
    pub fn set_modification_date(&mut self, value: Vec<i32>) -> Result<String, JsError> {
        let value = date(&value, "the date")?;
        self.inner
            .set_modification_date(value)
            .map(sync)
            .ok_or_else(|| refused("setModificationDate", &format!("{value:?}")))
    }

    /// Sets `/Info /Trapped`: `"true"`, `"false"` or `"unknown"`.
    #[wasm_bindgen(js_name = setTrapped)]
    pub fn set_trapped(&mut self, value: &str) -> Result<String, JsError> {
        let value = match value {
            "true" => Trapped::True,
            "false" => Trapped::False,
            "unknown" => Trapped::Unknown,
            other => {
                return Err(JsError::new(&format!(
                    "trapped must be true, false or unknown, not {other:?}"
                )))
            }
        };
        Ok(sync(self.inner.set_trapped(value)))
    }

    /// Makes `packet` the XMP metadata (14.3.2), verbatim and uncompressed.
    #[wasm_bindgen(js_name = setXmpMetadata)]
    pub fn set_xmp_metadata(&mut self, packet: &[u8]) -> Result<String, JsError> {
        self.inner
            .set_xmp_metadata(packet)
            .map(sync)
            .ok_or_else(|| refused("setXmpMetadata", "the document has no catalog"))
    }

    /// Sets one of a page's boundaries (14.11.2): `"media"`, `"crop"`,
    /// `"bleed"`, `"trim"` or `"art"`.
    #[wasm_bindgen(js_name = setPageBoundary)]
    #[allow(clippy::too_many_arguments)]
    pub fn set_page_boundary(
        &mut self,
        index: u32,
        which: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<(), JsError> {
        let which = boundary(which)?;
        if self.inner.set_page_boundary(index, which, x0, y0, x1, y1) {
            Ok(())
        } else {
            Err(refused(
                "setPageBoundary",
                &format!("page {index}, [{x0} {y0} {x1} {y1}]"),
            ))
        }
    }

    /// Sets a page's `/BleedBox`.
    #[wasm_bindgen(js_name = setBleedBox)]
    pub fn set_bleed_box(
        &mut self,
        index: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<(), JsError> {
        self.set_page_boundary(index, "bleed", x0, y0, x1, y1)
    }

    /// Sets a page's `/TrimBox`.
    #[wasm_bindgen(js_name = setTrimBox)]
    pub fn set_trim_box(
        &mut self,
        index: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<(), JsError> {
        self.set_page_boundary(index, "trim", x0, y0, x1, y1)
    }

    /// Sets a page's `/ArtBox`.
    #[wasm_bindgen(js_name = setArtBox)]
    pub fn set_art_box(
        &mut self,
        index: u32,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> Result<(), JsError> {
        self.set_page_boundary(index, "art", x0, y0, x1, y1)
    }

    /// Takes out what the flags name and reports every change it made. All
    /// four `false` takes out nothing; all four `true` is `Sanitise::ALL`.
    #[wasm_bindgen]
    pub fn sanitise(
        &mut self,
        javascript: bool,
        actions: bool,
        embedded_files: bool,
        metadata: bool,
    ) -> PdfSanitiseReport {
        PdfSanitiseReport {
            inner: self.inner.sanitise(&tinker_pdf::Sanitise {
                javascript,
                actions,
                embedded_files,
                metadata,
            }),
        }
    }
}

#[wasm_bindgen]
impl PdfDocument {
    /// One of a page's boundaries (14.11.2) as `[x0, y0, x1, y1]`, resolved
    /// the way the reader resolves an absent one.
    #[wasm_bindgen(js_name = pageBox)]
    pub fn page_box(&self, index: u32, which: &str) -> Result<Vec<f64>, JsError> {
        let which = boundary(which)?;
        let page = self
            .inner
            .page(index)
            .ok_or_else(|| JsError::new("no such page"))?;
        let (x0, y0, x1, y1) = page.boundary(which);
        Ok(vec![x0, y0, x1, y1])
    }
}
