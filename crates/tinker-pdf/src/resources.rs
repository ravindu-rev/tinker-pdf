//! Resolving a page's resources for the interpreter and the renderer.
//!
//! The two seams — `FontSource` for the interpreter, `GlyphSource` for the
//! renderer — exist so those crates never see a PDF dictionary. This is where
//! the dictionaries actually get read.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_content::{FontSource, Matrix, Rgb};
use tinker_pdf_cos::{font as cos_font, pages as cos_pages, CosDocument, Dict, Name, Object};
use tinker_pdf_filters::{ccitt_decode, jpeg_decode, CcittParams, JpegColor};
use tinker_pdf_font::{cff::Cff, glyf, Outline, Sfnt, Type1};
use tinker_pdf_render::{DecodedImage, GlyphSource, PatternPaint, Shading};

use crate::fonts::{self, FontProvider, FontRequest};

/// Extracted glyph outlines, keyed by font identity and character code.
///
/// `None` records that a glyph was looked for and is not there, so a missing
/// one costs the extraction attempt once rather than on every occurrence.
type OutlineCache = HashMap<(u64, u32), Option<Arc<Outline>>>;

/// Everything one page's rendering needs from its resources.
pub struct PageResources {
    doc: Arc<CosDocument>,
    fonts: HashMap<Vec<u8>, Arc<cos_font::Font>>,
    font_ids: HashMap<Vec<u8>, u64>,
    /// The embedded font program of each font, by the id the interpreter uses.
    programs: HashMap<u64, Arc<Vec<u8>>>,
    /// Which glyph a code selects, per font, resolved lazily.
    resources: Option<Dict>,
    /// Decoded images, kept because a page may draw one many times.
    images: Mutex<HashMap<Vec<u8>, Option<Arc<DecodedImage>>>>,
    /// Outlines already extracted, keyed by font and code.
    outlines: RwLock<OutlineCache>,
}

impl PageResources {
    /// Reads a page's resource dictionary.
    #[must_use]
    pub fn new(
        doc: &Arc<CosDocument>,
        page: &cos_pages::Page,
        provider: Option<&dyn FontProvider>,
    ) -> PageResources {
        let mut fonts = HashMap::new();
        let mut font_ids = HashMap::new();
        let mut programs = HashMap::new();
        let mut resources = None;

        if let Some(dict) = page.resources.as_ref() {
            resources = Some(dict.clone());
            for (name, font) in cos_font::from_resources(doc, dict) {
                if let Some(bytes) = doc.name_bytes(name) {
                    let key = bytes.to_vec();
                    let id = u64::from(name.id());
                    if let Some(program) = program_for(doc, &key, dict, &font, provider) {
                        programs.insert(id, program);
                    }
                    font_ids.insert(key.clone(), id);
                    fonts.insert(key, font);
                }
            }
        }

        PageResources {
            doc: doc.clone(),
            fonts,
            font_ids,
            programs,
            resources,
            images: Mutex::new(HashMap::new()),
            outlines: RwLock::new(HashMap::new()),
        }
    }

    /// Reads a resource dictionary that is not a page's.
    ///
    /// An annotation's appearance stream carries its own resources, and
    /// resolving its fonts against the page's would find the wrong ones or
    /// none at all.
    #[must_use]
    pub fn from_dict(
        doc: &Arc<CosDocument>,
        dict: Dict,
        provider: Option<&dyn FontProvider>,
    ) -> PageResources {
        let mut fonts = HashMap::new();
        let mut font_ids = HashMap::new();
        let mut programs = HashMap::new();

        for (name, font) in cos_font::from_resources(doc, &dict) {
            if let Some(bytes) = doc.name_bytes(name) {
                let key = bytes.to_vec();
                let id = u64::from(name.id());
                if let Some(program) = program_for(doc, &key, &dict, &font, provider) {
                    programs.insert(id, program);
                }
                font_ids.insert(key.clone(), id);
                fonts.insert(key, font);
            }
        }

        PageResources {
            doc: doc.clone(),
            fonts,
            font_ids,
            programs,
            resources: Some(dict),
            images: Mutex::new(HashMap::new()),
            outlines: RwLock::new(HashMap::new()),
        }
    }

    fn xobject(&self, name: &[u8]) -> Option<(Dict, tinker_pdf_cos::ObjRef)> {
        let resources = self.resources.as_ref()?;
        let value = self.doc.resolve_key(resources, self.doc.intern(b"XObject"));
        let dict = value.as_dict()?;
        let reference = dict.get_ref(self.doc.intern(name))?;
        let object = self.doc.get(reference).ok()?;
        let stream = object.as_dict()?.clone();
        Some((stream, reference))
    }

    /// The colour space a resource name selects (8.6.6).
    fn color_space(&self, name: &[u8]) -> Option<ColorSpace> {
        // The device spaces may be named directly without appearing in
        // /ColorSpace at all.
        match name {
            b"DeviceGray" | b"G" | b"CalGray" => return Some(ColorSpace::DeviceGray),
            b"DeviceRGB" | b"RGB" | b"CalRGB" => return Some(ColorSpace::DeviceRgb),
            b"DeviceCMYK" | b"CMYK" => return Some(ColorSpace::DeviceCmyk),
            b"Pattern" => return Some(ColorSpace::Pattern),
            _ => {}
        }

        let resources = self.resources.as_ref()?;
        let table = self
            .doc
            .resolve_key(resources, self.doc.intern(b"ColorSpace"));
        let dict = table.as_dict()?;
        let entry = dict.get(self.doc.intern(name))?.clone();
        self.parse_space(&entry, 0)
    }

    fn parse_space(&self, object: &Object, depth: u32) -> Option<ColorSpace> {
        if depth > 8 {
            return None;
        }
        let resolved = self.doc.resolve(object);

        if let Some(name) = resolved.as_name() {
            let bytes = self.doc.name_bytes(name)?;
            return match bytes.as_ref() {
                b"DeviceGray" | b"G" | b"CalGray" => Some(ColorSpace::DeviceGray),
                b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(ColorSpace::DeviceRgb),
                b"DeviceCMYK" | b"CMYK" => Some(ColorSpace::DeviceCmyk),
                b"Pattern" => Some(ColorSpace::Pattern),
                _ => None,
            };
        }

        let items = resolved.as_array()?;
        let family = items.first().and_then(Object::as_name)?;
        let family = self.doc.name_bytes(family)?;

        match family.as_ref() {
            b"ICCBased" => {
                // 8.6.5.5: a reader may use the alternate space, and the
                // component count is what the data's shape actually is.
                let stream = items.get(1).map(|o| self.doc.resolve(o))?;
                let n = stream
                    .as_dict()
                    .and_then(|d| d.get_int(self.doc.intern(b"N")))
                    .unwrap_or(3);
                Some(ColorSpace::Approximated {
                    components: n.clamp(1, 4) as usize,
                })
            }
            b"Indexed" | b"I" => {
                let base = self.parse_space(items.get(1)?, depth + 1)?;
                let high = items.get(2).and_then(|o| self.doc.resolve(o).as_int())?;
                let lookup = match items.get(3).map(|o| self.doc.resolve(o)) {
                    Some(value) => match value.as_string() {
                        Some(s) => s.bytes.clone(),
                        None => items
                            .get(3)
                            .and_then(Object::as_objref)
                            .and_then(|r| self.doc.stream_decoded(r).ok())
                            .unwrap_or_default(),
                    },
                    None => Vec::new(),
                };
                Some(ColorSpace::Indexed {
                    base: Box::new(base),
                    lookup,
                    high: high.clamp(0, 255) as u32,
                })
            }
            b"Separation" | b"DeviceN" => {
                let components = if family.as_ref() == b"Separation" {
                    1
                } else {
                    self.doc
                        .resolve(items.get(1)?)
                        .as_array()
                        .map_or(1, <[Object]>::len)
                };
                let alternate = self.parse_space(items.get(2)?, depth + 1)?;

                // 8.6.6.4: the fourth element converts tint values into the
                // alternate space. Left as the identity, a one-ink Separation
                // feeds its tint straight into a CMYK alternate as cyan, so
                // full-tint black prints cyan — and nothing about the result
                // looks like a bug. Spot colours are everywhere in
                // print-origin files.
                let tint = items
                    .get(3)
                    .and_then(|o| self.parse_function(o, depth + 1))
                    .unwrap_or(Function::Identity);

                Some(ColorSpace::Separation {
                    components,
                    alternate: Box::new(alternate),
                    tint: Box::new(tint),
                })
            }
            b"CalGray" => Some(ColorSpace::DeviceGray),
            b"CalRGB" => Some(ColorSpace::DeviceRgb),
            // 8.6.5.4: L runs 0..100 and a/b roughly -128..127. Aliasing Lab
            // to RGB clamps every component into 0..1, which renders almost
            // the whole space black.
            b"Lab" => {
                let params = items.get(1).map(|o| self.doc.resolve(o));
                let range = params
                    .as_ref()
                    .and_then(|p| p.as_dict())
                    .map(|d| self.doc.resolve_key(d, self.doc.intern(b"Range")))
                    .and_then(|r| {
                        let values: Vec<f64> =
                            r.as_array()?.iter().filter_map(Object::as_number).collect();
                        (values.len() >= 4).then(|| [values[0], values[1], values[2], values[3]])
                    })
                    .unwrap_or([-100.0, 100.0, -100.0, 100.0]);
                Some(ColorSpace::Lab { range })
            }
            _ => None,
        }
    }

    /// Reads a shading dictionary, wherever it was found.
    ///
    /// Split out of [`GlyphSource::shading`] so a shading *pattern* can reuse
    /// it on the dictionary it resolved itself, rather than duplicating the
    /// nine entries a shading is made of.
    fn read_shading(&self, dict: &Dict) -> Result<Option<Shading>, i64> {
        let kind = self
            .doc
            .resolve_key(dict, self.doc.intern(b"ShadingType"))
            .as_int()
            .unwrap_or(0);

        let space = self.doc.resolve_key(dict, self.doc.intern(b"ColorSpace"));
        let space = self.parse_space(&space, 0).unwrap_or(ColorSpace::DeviceRgb);
        let function = self.function(dict).unwrap_or(Function::Identity);

        let coords = self.doc.resolve_key(dict, self.doc.intern(b"Coords"));
        let coords: Vec<f64> = coords
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect()
            })
            .unwrap_or_default();

        let extend = self.doc.resolve_key(dict, self.doc.intern(b"Extend"));
        let extend = extend
            .as_array()
            .map(|a| {
                (
                    a.first().and_then(Object::as_bool).unwrap_or(false),
                    a.get(1).and_then(Object::as_bool).unwrap_or(false),
                )
            })
            .unwrap_or((false, false));

        match kind {
            1 => {
                let domain = self.doc.resolve_key(dict, self.doc.intern(b"Domain"));
                let domain: Vec<f64> = domain
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|o| self.doc.resolve(o).as_number())
                            .collect()
                    })
                    .unwrap_or_default();
                let d = |i: usize, fallback: f64| domain.get(i).copied().unwrap_or(fallback);
                Ok(Some(Shading::FunctionBased {
                    space,
                    function,
                    domain: [d(0, 0.0), d(1, 1.0), d(2, 0.0), d(3, 1.0)],
                }))
            }
            2 if coords.len() >= 4 => Ok(Some(Shading::Axial {
                space,
                function,
                coords: [coords[0], coords[1], coords[2], coords[3]],
                extend,
            })),
            3 if coords.len() >= 6 => Ok(Some(Shading::Radial {
                space,
                function,
                coords: [
                    coords[0], coords[1], coords[2], coords[3], coords[4], coords[5],
                ],
                extend,
            })),
            // The mesh types are behind a capability (ruling 3).
            4..=7 => Err(kind),
            _ => Ok(None),
        }
    }
}

/// The embedded font program of a font resource, if it has one (9.9).
/// The font program to draw a font's glyphs with: the embedded one, or a
/// substitute the host supplied.
///
/// The document's own program always wins. A substitute is a different face
/// with different outlines, and preferring it over what the file carries
/// would change how a correctly embedded document looks.
fn program_for(
    doc: &CosDocument,
    name: &[u8],
    resources: &Dict,
    font: &cos_font::Font,
    provider: Option<&dyn FontProvider>,
) -> Option<Arc<Vec<u8>>> {
    if let Some(embedded) = embedded_program(doc, name, resources) {
        return Some(Arc::new(embedded));
    }

    let provider = provider?;
    if !fonts::is_substitutable(font) {
        return None;
    }
    let dict = fonts::font_dict(doc, resources, name)?;
    provider.substitute(&FontRequest::read(doc, &dict))
}

fn embedded_program(doc: &CosDocument, name: &[u8], resources: &Dict) -> Option<Vec<u8>> {
    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    let dict = fonts.as_dict()?;
    let font = doc.resolve_key(dict, doc.intern(name));
    let mut font = font.as_dict()?.clone();

    // A composite font keeps its program on the descendant.
    let descendants = doc.resolve_key(&font, doc.intern(b"DescendantFonts"));
    if let Some(first) = descendants.as_array().and_then(<[Object]>::first) {
        if let Some(dict) = doc.resolve(first).as_dict() {
            font = dict.clone();
        }
    }

    let descriptor = doc.resolve_key(&font, doc.intern(b"FontDescriptor"));
    let descriptor = descriptor.as_dict()?;

    for key in [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"] {
        if let Some(reference) = descriptor.get_ref(doc.intern(key)) {
            if let Ok(bytes) = doc.stream_decoded(reference) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

impl FontSource for PageResources {
    fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
        let Some(font) = self.fonts.get(font) else {
            return Vec::new();
        };
        font.decode(bytes)
            .into_iter()
            .map(|d| (d.code, d.text, d.width))
            .collect()
    }

    fn is_vertical(&self, font: &[u8]) -> bool {
        self.fonts.get(font).is_some_and(|f| f.is_vertical())
    }

    fn font_id(&self, font: &[u8]) -> u64 {
        self.font_ids.get(font).copied().unwrap_or(0)
    }

    fn form(&self, name: &[u8]) -> Option<(Vec<u8>, Matrix)> {
        let (dict, reference) = self.xobject(name)?;
        let subtype = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"Subtype"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Form" {
            return None;
        }

        let content = self.doc.stream_decoded(reference).ok()?;
        // 8.10.2: /Matrix maps the form's space into the one that invoked it.
        let matrix = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"Matrix"))
            .as_array()
            .and_then(|values| {
                let n = |i: usize| values.get(i).and_then(Object::as_number);
                Some(Matrix {
                    a: n(0)?,
                    b: n(1)?,
                    c: n(2)?,
                    d: n(3)?,
                    e: n(4)?,
                    f: n(5)?,
                })
            })
            .unwrap_or(Matrix::IDENTITY);

        Some((content, matrix))
    }

    fn resolve_color(&self, space: &[u8], components: &[f64]) -> Option<Rgb> {
        let space = self.color_space(space)?;
        let (r, g, b) = space.to_rgb(components);
        Some(Rgb { r, g, b })
    }

    fn color_components(&self, space: &[u8]) -> Option<usize> {
        Some(self.color_space(space)?.components())
    }

    fn ext_g_state_alpha(&self, name: &[u8]) -> Option<(Option<f64>, Option<f64>)> {
        let resources = self.resources.as_ref()?;
        let table = self
            .doc
            .resolve_key(resources, self.doc.intern(b"ExtGState"));
        let entry = self
            .doc
            .resolve_key(table.as_dict()?, self.doc.intern(name));
        let dict = entry.as_dict()?;

        // 8.4.5 Table 58: `ca` is the non-stroking alpha, `CA` the stroking.
        let fill = self
            .doc
            .resolve_key(dict, self.doc.intern(b"ca"))
            .as_number();
        let stroke = self
            .doc
            .resolve_key(dict, self.doc.intern(b"CA"))
            .as_number();
        (fill.is_some() || stroke.is_some()).then_some((fill, stroke))
    }
}

impl GlyphSource for PageResources {
    fn outline(&self, font_id: u64, code: u32) -> Option<Outline> {
        if let Ok(cache) = self.outlines.read() {
            if let Some(hit) = cache.get(&(font_id, code)) {
                return hit.as_ref().map(|o| (**o).clone());
            }
        }

        let outline = self.extract_outline(font_id, code).map(Arc::new);
        if let Ok(mut cache) = self.outlines.write() {
            // Bounded: a hostile document could ask for millions of codes.
            if cache.len() < 1 << 16 {
                cache.insert((font_id, code), outline.clone());
            }
        }
        outline.map(|o| (*o).clone())
    }

    fn shading(&self, name: &[u8]) -> Result<Option<Shading>, i64> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(None);
        };
        let table = self.doc.resolve_key(resources, self.doc.intern(b"Shading"));
        let Some(table) = table.as_dict() else {
            return Ok(None);
        };
        let entry = self.doc.resolve_key(table, self.doc.intern(name));
        let Some(dict) = entry.as_dict() else {
            return Ok(None);
        };

        self.read_shading(dict)
    }

    fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
        let resources = self.resources.as_ref()?;
        let table = self.doc.resolve_key(resources, self.doc.intern(b"Pattern"));
        let table = table.as_dict()?;
        let entry = self.doc.resolve_key(table, self.doc.intern(name));
        let dict = entry.as_dict()?;

        // 8.7.3.3: type 2 is a shading pattern, type 1 a tiling pattern. A
        // tiling pattern needs its content stream replayed into a tile and
        // repeated, which this build does not do — reported, not painted.
        let kind = self
            .doc
            .resolve_key(dict, self.doc.intern(b"PatternType"))
            .as_int()
            .unwrap_or(0);
        if kind != 2 {
            return Some(PatternPaint::Unsupported);
        }

        let shading = self.doc.resolve_key(dict, self.doc.intern(b"Shading"));
        let shading = shading.as_dict()?;
        // A mesh inside a pattern is as unpaintable as a mesh anywhere else,
        // and reports as an unpainted pattern rather than as a missing one.
        let Ok(Some(shading)) = self.read_shading(shading) else {
            return Some(PatternPaint::Unsupported);
        };

        // 8.7.3.1: the pattern matrix maps pattern space to the page's
        // *default* space, so the CTM in force at fill time is not part of it.
        let matrix = self
            .doc
            .resolve_key(dict, self.doc.intern(b"Matrix"))
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect::<Vec<f64>>()
            })
            .and_then(|v| {
                (v.len() >= 6 && v.iter().all(|x| x.is_finite())).then(|| Matrix {
                    a: v[0],
                    b: v[1],
                    c: v[2],
                    d: v[3],
                    e: v[4],
                    f: v[5],
                })
            })
            .unwrap_or(Matrix::IDENTITY);

        Some(PatternPaint::Shading(Box::new(shading), matrix))
    }

    fn inline_image(&self, dict: &[u8], data: &[u8]) -> Result<Option<DecodedImage>, String> {
        // 8.9.7 Table 93: an inline image's keys have short forms. They are
        // rewritten to the long ones so the shared decoder sees an ordinary
        // image dictionary rather than learning a second vocabulary.
        const ABBREVIATIONS: &[(&[u8], &[u8])] = &[
            (b"/BPC", b"/BitsPerComponent"),
            (b"/CS", b"/ColorSpace"),
            (b"/D", b"/Decode"),
            (b"/DP", b"/DecodeParms"),
            (b"/F", b"/Filter"),
            (b"/H", b"/Height"),
            (b"/IM", b"/ImageMask"),
            (b"/I", b"/Interpolate"),
            (b"/W", b"/Width"),
            (b"/G", b"/DeviceGray"),
            (b"/RGB", b"/DeviceRGB"),
            (b"/CMYK", b"/DeviceCMYK"),
            (b"/AHx", b"/ASCIIHexDecode"),
            (b"/A85", b"/ASCII85Decode"),
            (b"/LZW", b"/LZWDecode"),
            (b"/Fl", b"/FlateDecode"),
            (b"/RL", b"/RunLengthDecode"),
            (b"/CCF", b"/CCITTFaxDecode"),
            (b"/DCT", b"/DCTDecode"),
        ];

        let mut text = format!("<< {} >>", String::from_utf8_lossy(dict));
        for (short, long) in ABBREVIATIONS {
            let short = format!("{} ", String::from_utf8_lossy(short));
            let long = format!("{} ", String::from_utf8_lossy(long));
            text = text.replace(&short, &long);
        }

        let mut sink = tinker_pdf_cos::WarningSink::new();
        let parsed =
            tinker_pdf_cos::parse_object_at(text.as_bytes(), 0, self.doc.names_table(), &mut sink);
        let Some(dict) = parsed.object.as_dict() else {
            return Ok(None);
        };

        self.decode_inline(dict, data).map(Some)
    }

    fn image(&self, name: &[u8]) -> Result<Option<DecodedImage>, String> {
        if let Ok(cache) = self.images.lock() {
            if let Some(hit) = cache.get(name) {
                return Ok(hit.as_ref().map(|i| (**i).clone()));
            }
        }

        let decoded = self.decode_image(name);
        let stored = decoded.as_ref().ok().cloned().map(Arc::new);
        if let Ok(mut cache) = self.images.lock() {
            if cache.len() < 256 {
                cache.insert(name.to_vec(), stored);
            }
        }
        decoded.map(Some)
    }
}

impl PageResources {
    /// The `/DecodeParms` of a stream, which may be one dictionary or an
    /// array with one entry per filter (7.4.1).
    fn decode_parms(&self, dict: &Dict) -> Option<Dict> {
        let value = self.doc.resolve_key(dict, Name::DECODE_PARMS);
        if let Some(d) = value.as_dict() {
            return Some(d.clone());
        }
        // The last filter's parameters are the ones an image codec wants.
        let items = value.as_array()?;
        items
            .iter()
            .rev()
            .find_map(|o| self.doc.resolve(o).as_dict().cloned())
    }

    /// Reads a `/Function` entry, which is one function or an array of them,
    /// one per output component (7.10).
    fn function(&self, dict: &Dict) -> Option<Function> {
        let value = self.doc.resolve_key(dict, self.doc.intern(b"Function"));
        if let Some(items) = value.as_array() {
            // 7.10.1: every member supplies one output component. Reading only
            // the first turns an RGB gradient into a red ramp on black, and
            // does it silently — a one-output function is valid on its own, so
            // nothing downstream can tell the difference.
            let parsed: Vec<Function> = items
                .iter()
                .filter_map(|o| self.parse_function(o, 0))
                .collect();
            return match parsed.len() {
                0 => None,
                1 => parsed.into_iter().next(),
                _ => Some(Function::Array(parsed)),
            };
        }
        self.parse_function(&value, 0)
    }

    fn parse_function(&self, object: &Object, depth: u32) -> Option<Function> {
        if depth > 8 {
            return None;
        }
        let resolved = self.doc.resolve(object);
        let dict = resolved.as_dict()?;

        let numbers = |key: &[u8]| -> Vec<f64> {
            let value = self.doc.resolve_key(dict, self.doc.intern(key));
            value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| self.doc.resolve(o).as_number())
                        .collect()
                })
                .unwrap_or_default()
        };
        let pairs = |values: &[f64]| -> Vec<(f64, f64)> {
            values.chunks_exact(2).map(|p| (p[0], p[1])).collect()
        };

        let kind = self
            .doc
            .resolve_key(dict, self.doc.intern(b"FunctionType"))
            .as_int()?;

        match kind {
            2 => {
                let c0 = numbers(b"C0");
                let c1 = numbers(b"C1");
                let domain = numbers(b"Domain");
                Some(Function::Exponential {
                    domain: (
                        domain.first().copied().unwrap_or(0.0),
                        domain.get(1).copied().unwrap_or(1.0),
                    ),
                    c0: if c0.is_empty() { vec![0.0] } else { c0 },
                    c1: if c1.is_empty() { vec![1.0] } else { c1 },
                    n: self
                        .doc
                        .resolve_key(dict, self.doc.intern(b"N"))
                        .as_number()
                        .unwrap_or(1.0),
                })
            }
            3 => {
                let value = self.doc.resolve_key(dict, self.doc.intern(b"Functions"));
                let functions: Vec<Function> = value
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|o| self.parse_function(o, depth + 1))
                            .collect()
                    })
                    .unwrap_or_default();
                let domain = numbers(b"Domain");
                Some(Function::Stitching {
                    domain: (
                        domain.first().copied().unwrap_or(0.0),
                        domain.get(1).copied().unwrap_or(1.0),
                    ),
                    functions,
                    bounds: numbers(b"Bounds"),
                    encode: pairs(&numbers(b"Encode")),
                })
            }
            0 | 4 => {
                // Both are streams, so the object handed in is the reference.
                let reference = object.as_objref()?;
                let data = self.doc.stream_decoded(reference).ok()?;
                if kind == 4 {
                    return Some(Function::PostScript {
                        domain: pairs(&numbers(b"Domain")),
                        range: pairs(&numbers(b"Range")),
                        program: tinker_pdf_color::function::parse_postscript(&data),
                    });
                }
                Some(Function::Sampled {
                    domain: pairs(&numbers(b"Domain")),
                    range: pairs(&numbers(b"Range")),
                    size: numbers(b"Size").iter().map(|v| *v as usize).collect(),
                    bits: self
                        .doc
                        .resolve_key(dict, self.doc.intern(b"BitsPerSample"))
                        .as_int()
                        .unwrap_or(8) as u32,
                    encode: pairs(&numbers(b"Encode")),
                    decode: pairs(&numbers(b"Decode")),
                    samples: data,
                })
            }
            _ => None,
        }
    }

    /// Pulls one glyph's outline out of an embedded font program.
    fn extract_outline(&self, font_id: u64, code: u32) -> Option<Outline> {
        let program = self.programs.get(&font_id)?;

        // The font's own character mapping decides which glyph a code means.
        let name = self
            .font_ids
            .iter()
            .find(|(_, id)| **id == font_id)
            .map(|(name, _)| name.clone())?;
        let font = self.fonts.get(&name)?;
        let text = font.text_of(code);
        let ch = text.chars().next();

        if let Some(sfnt) = Sfnt::parse(program) {
            let glyph = match ch {
                Some(c) => sfnt.glyph_for_char(c).filter(|g| *g != 0),
                None => None,
            }
            // A composite font addresses glyphs directly, and a symbolic one
            // often has no usable character mapping at all.
            .or_else(|| u16::try_from(code).ok())?;

            let units = f64::from(sfnt.units_per_em.max(1));
            let outline = glyf::outline(&sfnt, glyph)?;
            return Some(scale(&outline, 1.0 / units));
        }

        if let Some(cff) = Cff::parse(program) {
            let glyph = u16::try_from(code).ok()?;
            let outline = cff.outline(glyph)?;
            // A CFF font matrix is usually 1/1000 but need not be.
            let scale_factor = cff.font_matrix.first().copied().unwrap_or(0.001);
            return Some(scale(&outline, scale_factor));
        }

        // 9.9: a `/FontFile` is a Type 1 program. Until this existed the bytes
        // reached the two parsers above, both declined them correctly, and the
        // glyph was silently absent — an embedded Type 1 font drew nothing and
        // said nothing about why.
        if let Some(type1) = Type1::parse(program) {
            // Type 1 addresses glyphs by *name*: through the encoding the PDF
            // font dictionary specifies, or through the font's own built-in
            // one. The index is not a glyph id.
            let glyph = ch
                .and_then(|c| {
                    let name = tinker_pdf_font::glyph_name_for_char(c)?;
                    type1.glyph_for_name(name.as_bytes())
                })
                .or_else(|| {
                    u8::try_from(code)
                        .ok()
                        .and_then(|b| type1.glyph_for_code(b))
                })?;

            let outline = type1.outline(glyph)?;
            let scale_factor = type1.font_matrix.first().copied().unwrap_or(0.001);
            return Some(scale(&outline, scale_factor));
        }

        None
    }

    /// Decodes one image XObject to RGB (8.9).
    fn decode_image(&self, name: &[u8]) -> Result<DecodedImage, String> {
        let Some((dict, reference)) = self.xobject(name) else {
            return Err(String::from_utf8_lossy(name).into_owned());
        };
        let label = String::from_utf8_lossy(name).into_owned();
        let mut image = self.decode_image_at(&dict, reference, &label)?;

        // 11.6.5.3: /SMask is a greyscale image whose samples are this one's
        // opacity. Without it every soft-masked image — a drop shadow, a
        // feathered edge, anything composited in a design tool — paints as an
        // opaque rectangle over the page.
        if let Some(mask) = self.soft_mask(&dict) {
            apply_soft_mask(&mut image, &mask);
        }
        Ok(image)
    }

    /// Decodes an image from its dictionary, wherever that came from.
    fn decode_image_at(
        &self,
        dict: &Dict,
        reference: tinker_pdf_cos::ObjRef,
        name: &str,
    ) -> Result<DecodedImage, String> {
        let dict = dict.clone();
        let name = name.as_bytes();

        let subtype = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"Subtype"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n));
        if subtype.as_deref() != Some(b"Image".as_slice()) {
            return Err(String::from_utf8_lossy(name).into_owned());
        }

        let int = |key: &[u8]| {
            self.doc
                .resolve_key(&dict, self.doc.intern(key))
                .as_int()
                .unwrap_or(0)
        };
        let width = int(b"Width").clamp(0, 1 << 16) as u32;
        let height = int(b"Height").clamp(0, 1 << 16) as u32;
        if width == 0 || height == 0 {
            return Err("empty".to_string());
        }
        let bpc = int(b"BitsPerComponent").clamp(1, 16) as u32;

        // The final filter decides how the bytes are read.
        let filters = self.doc.resolve_key(&dict, Name::FILTER);
        let last_filter = match filters.as_name() {
            Some(n) => self.doc.name_bytes(n).map(|b| b.to_vec()),
            None => filters
                .as_array()
                .and_then(|a| a.last())
                .and_then(Object::as_name)
                .and_then(|n| self.doc.name_bytes(n))
                .map(|b| b.to_vec()),
        };

        // DCTDecode data comes out of the stream tier still encoded, which is
        // exactly what the JPEG decoder wants.
        if matches!(last_filter.as_deref(), Some(b"DCTDecode") | Some(b"DCT")) {
            let raw = self
                .doc
                .stream_raw(reference)
                .map_err(|_| "DCTDecode".to_string())?;
            let image = jpeg_decode(&raw, 1 << 28).map_err(|e| format!("{e:?}"))?;
            let rgb = jpeg_to_rgb(&image);
            return Ok(DecodedImage {
                width: image.width,
                height: image.height,
                rgb,
                alpha: Vec::new(),
                stencil: false,
            });
        }
        // CCITT data likewise arrives still coded, and carries its own
        // parameters in /DecodeParms.
        if matches!(
            last_filter.as_deref(),
            Some(b"CCITTFaxDecode") | Some(b"CCF")
        ) {
            let raw = self
                .doc
                .stream_raw(reference)
                .map_err(|_| "CCITTFaxDecode".to_string())?;
            let parms = self.decode_parms(&dict);
            let params = CcittParams {
                k: parms
                    .as_ref()
                    .and_then(|p| p.get_int(self.doc.intern(b"K")))
                    .unwrap_or(0) as i32,
                columns: parms
                    .as_ref()
                    .and_then(|p| p.get_int(self.doc.intern(b"Columns")))
                    .unwrap_or(1728)
                    .clamp(1, 1 << 16) as u32,
                rows: height,
                black_is_1: parms
                    .as_ref()
                    .and_then(|p| p.get_bool(self.doc.intern(b"BlackIs1")))
                    .unwrap_or(false),
                byte_align: parms
                    .as_ref()
                    .and_then(|p| p.get_bool(self.doc.intern(b"EncodedByteAlign")))
                    .unwrap_or(false),
            };

            let (gray, _) = ccitt_decode(&raw, &params, 1 << 28);
            let mut rgb = Vec::with_capacity(gray.len() * 3);
            for value in &gray {
                rgb.extend_from_slice(&[*value, *value, *value]);
            }
            rgb.resize((width as usize) * (height as usize) * 3, 255);
            return Ok(DecodedImage {
                width,
                height,
                rgb,
                alpha: Vec::new(),
                stencil: false,
            });
        }
        if let Some(filter) = last_filter.as_deref() {
            if matches!(filter, b"JPXDecode" | b"JBIG2Decode") {
                return Err(String::from_utf8_lossy(filter).into_owned());
            }
        }

        // Everything else decodes to raw samples.
        let data = self
            .doc
            .stream_decoded(reference)
            .map_err(|_| "undecodable".to_string())?;

        let space = self.doc.resolve_key(&dict, self.doc.intern(b"ColorSpace"));
        let space = self
            .parse_space(&space, 0)
            .unwrap_or(ColorSpace::DeviceGray);
        let n = space.components();

        // 8.9.5.2: an image mask is one bit per sample, painted in the fill
        // colour. Without the fill colour here it reads as black-on-nothing.
        let is_mask = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"ImageMask"))
            .as_bool()
            .unwrap_or(false);

        // 8.9.5.2: /Decode remaps each component's sample range. `/Decode
        // [1 0]` on a stencil or a greyscale image inverts it, and ignoring it
        // renders the image as its own negative.
        let decode: Vec<(f64, f64)> = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"Decode"))
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect::<Vec<f64>>()
            })
            .map(|v| v.chunks_exact(2).map(|c| (c[0], c[1])).collect())
            .unwrap_or_default();

        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        let mut alpha = Vec::new();
        let row_bits = (width as usize) * n * (bpc as usize);
        let row_bytes = row_bits.div_ceil(8);
        let max = ((1u32 << bpc.min(16)) - 1) as f64;

        for y in 0..height as usize {
            for x in 0..width as usize {
                let mut components = Vec::with_capacity(n);
                for c in 0..n {
                    let bit = y * row_bytes * 8 + (x * n + c) * bpc as usize;
                    let value = read_bits(&data, bit, bpc);
                    let raw = match &space {
                        // An indexed space's component is the index itself.
                        ColorSpace::Indexed { .. } => f64::from(value),
                        _ => f64::from(value) / max,
                    };
                    components.push(match decode.get(c) {
                        // The interpolation is over the *sample* range, so an
                        // indexed image maps its index rather than a fraction.
                        Some((dmin, dmax)) => match &space {
                            ColorSpace::Indexed { .. } => dmin + raw * (dmax - dmin) / max.max(1.0),
                            _ => dmin + raw * (dmax - dmin),
                        },
                        None => raw,
                    });
                }

                if is_mask {
                    // A zero sample paints, a one does not (8.9.6.2).
                    let paints = components.first().copied().unwrap_or(0.0) < 0.5;
                    rgb.extend_from_slice(&[0, 0, 0]);
                    alpha.push(if paints { 255 } else { 0 });
                } else {
                    let (r, g, b) = space.to_rgb(&components);
                    rgb.extend_from_slice(&[r, g, b]);
                }
            }
        }

        Ok(DecodedImage {
            width,
            height,
            rgb,
            alpha,
            stencil: is_mask,
        })
    }

    /// Decodes an inline image's samples (8.9.7).
    ///
    /// Separate from [`Self::decode_image_at`] because an inline image has no
    /// object number: its bytes are in hand rather than behind a stream tier,
    /// so the filter chain is run here instead of by the document.
    fn decode_inline(&self, dict: &Dict, data: &[u8]) -> Result<DecodedImage, String> {
        use tinker_pdf_filters::{
            ascii85_decode, ascii_hex_decode, flate_decode, lzw_decode, run_length_decode, Limits,
        };

        let int = |key: &[u8]| {
            self.doc
                .resolve_key(dict, self.doc.intern(key))
                .as_int()
                .unwrap_or(0)
        };
        let width = int(b"Width").clamp(0, 1 << 16) as u32;
        let height = int(b"Height").clamp(0, 1 << 16) as u32;
        if width == 0 || height == 0 {
            return Err("inline".to_string());
        }

        let is_mask = self
            .doc
            .resolve_key(dict, self.doc.intern(b"ImageMask"))
            .as_bool()
            .unwrap_or(false);
        // 8.9.6.2: a mask is one bit per sample whatever /BPC claims.
        let bpc = if is_mask {
            1
        } else {
            int(b"BitsPerComponent").clamp(1, 16) as u32
        };

        // The filters, in the order they were applied.
        let filters_value = self.doc.resolve_key(dict, Name::FILTER);
        let mut filters: Vec<Vec<u8>> = Vec::new();
        if let Some(name) = filters_value.as_name() {
            if let Some(bytes) = self.doc.name_bytes(name) {
                filters.push(bytes.to_vec());
            }
        } else if let Some(items) = filters_value.as_array() {
            for item in items {
                if let Some(bytes) = item.as_name().and_then(|n| self.doc.name_bytes(n)) {
                    filters.push(bytes.to_vec());
                }
            }
        }

        let limits = Limits::new(1 << 26);
        let mut bytes = data.to_vec();
        for filter in &filters {
            bytes = match filter.as_slice() {
                b"FlateDecode" | b"Fl" => {
                    flate_decode(&bytes, &limits, None)
                        .map_err(|_| "FlateDecode".to_string())?
                        .data
                }
                b"LZWDecode" | b"LZW" => {
                    lzw_decode(&bytes, &limits, true, None)
                        .map_err(|_| "LZWDecode".to_string())?
                        .data
                }
                b"ASCIIHexDecode" | b"AHx" => ascii_hex_decode(&bytes, &limits).data,
                b"ASCII85Decode" | b"A85" => ascii85_decode(&bytes, &limits).data,
                b"RunLengthDecode" | b"RL" => run_length_decode(&bytes, &limits).data,
                // 8.9.7 permits DCT and CCITT inline too; they arrive still
                // coded and are reported rather than half-decoded.
                other => return Err(String::from_utf8_lossy(other).into_owned()),
            };
        }

        let space = self.doc.resolve_key(dict, self.doc.intern(b"ColorSpace"));
        let space = self
            .parse_space(&space, 0)
            .unwrap_or(ColorSpace::DeviceGray);
        let n = if is_mask { 1 } else { space.components() };

        let decode: Vec<(f64, f64)> = self
            .doc
            .resolve_key(dict, self.doc.intern(b"Decode"))
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect::<Vec<f64>>()
            })
            .map(|v| v.chunks_exact(2).map(|c| (c[0], c[1])).collect())
            .unwrap_or_default();

        let row_bytes = ((width as usize) * n * (bpc as usize)).div_ceil(8);
        let max = ((1u32 << bpc.min(16)) - 1) as f64;
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        let mut alpha = Vec::new();

        for y in 0..height as usize {
            for x in 0..width as usize {
                let mut components = Vec::with_capacity(n);
                for c in 0..n {
                    let bit = y * row_bytes * 8 + (x * n + c) * bpc as usize;
                    let value = read_bits(&bytes, bit, bpc);
                    let raw = match &space {
                        ColorSpace::Indexed { .. } => f64::from(value),
                        _ => f64::from(value) / max,
                    };
                    components.push(match decode.get(c) {
                        Some((dmin, dmax)) => match &space {
                            ColorSpace::Indexed { .. } => dmin + raw * (dmax - dmin) / max.max(1.0),
                            _ => dmin + raw * (dmax - dmin),
                        },
                        None => raw,
                    });
                }

                if is_mask {
                    let paints = components.first().copied().unwrap_or(0.0) < 0.5;
                    rgb.extend_from_slice(&[0, 0, 0]);
                    alpha.push(if paints { 255 } else { 0 });
                } else {
                    let (r, g, b) = space.to_rgb(&components);
                    rgb.extend_from_slice(&[r, g, b]);
                }
            }
        }

        Ok(DecodedImage {
            width,
            height,
            rgb,
            alpha,
            stencil: is_mask,
        })
    }

    /// The `/SMask` of an image, decoded (11.6.5.3).
    fn soft_mask(&self, dict: &Dict) -> Option<DecodedImage> {
        let reference = dict.get_ref(self.doc.intern(b"SMask"))?;
        let object = self.doc.get(reference).ok()?;
        let mask = object.as_dict()?.clone();
        // A mask that is itself masked is not a thing the spec allows, and
        // decoding it through the top-level entry point would recurse.
        self.decode_image_at(&mask, reference, "SMask").ok()
    }
}

/// Applies a soft mask's luminance as the image's per-sample opacity
/// (11.6.5.3).
///
/// The mask has its own dimensions and need not match the image's, so it is
/// sampled by position rather than by index — a 2x2 mask over a 512x512 image
/// is legal and common, because a mask only has to carry as much detail as its
/// gradient needs.
fn apply_soft_mask(image: &mut DecodedImage, mask: &DecodedImage) {
    if image.width == 0 || image.height == 0 || mask.width == 0 || mask.height == 0 {
        return;
    }

    let samples = (image.width as usize).saturating_mul(image.height as usize);
    let mut alpha = Vec::with_capacity(samples);

    for y in 0..image.height as usize {
        // Nearest-neighbour: a mask is a smooth ramp far more often than not,
        // and interpolating it would cost more than it buys.
        let my = y * mask.height as usize / image.height as usize;
        for x in 0..image.width as usize {
            let mx = x * mask.width as usize / image.width as usize;
            let at = (my * mask.width as usize + mx) * 3;
            // The mask is greyscale, so any channel is its value.
            let value = mask.rgb.get(at).copied().unwrap_or(255);

            // An image that already had alpha keeps the more opaque
            // constraint of the two rather than losing one of them.
            let existing = image
                .alpha
                .get(y * image.width as usize + x)
                .copied()
                .unwrap_or(255);
            alpha.push(((u16::from(existing) * u16::from(value)) / 255) as u8);
        }
    }

    image.alpha = alpha;
}

/// Reads `bits` bits starting at bit offset `at`, big-endian.
fn read_bits(data: &[u8], at: usize, bits: u32) -> u32 {
    let mut value = 0u32;
    for i in 0..bits.min(16) {
        let index = at + i as usize;
        let byte = data.get(index / 8).copied().unwrap_or(0);
        let bit = (byte >> (7 - (index % 8))) & 1;
        value = (value << 1) | u32::from(bit);
    }
    value
}

fn jpeg_to_rgb(image: &tinker_pdf_filters::JpegImage) -> Vec<u8> {
    let count = (image.width as usize) * (image.height as usize);
    let mut out = Vec::with_capacity(count * 3);

    match image.color {
        JpegColor::Gray => {
            for i in 0..count {
                let v = image.data.get(i).copied().unwrap_or(0);
                out.extend_from_slice(&[v, v, v]);
            }
        }
        JpegColor::Rgb => out.extend_from_slice(&image.data),
        JpegColor::Cmyk | JpegColor::CmykInverted => {
            let space = ColorSpace::DeviceCmyk;
            for i in 0..count {
                let at =
                    |c: usize| f64::from(image.data.get(i * 4 + c).copied().unwrap_or(0)) / 255.0;
                let (r, g, b) = space.to_rgb(&[at(0), at(1), at(2), at(3)]);
                out.extend_from_slice(&[r, g, b]);
            }
        }
    }

    out.resize(count * 3, 255);
    out
}

/// Scales an outline from font units into em units.
fn scale(outline: &Outline, factor: f64) -> Outline {
    use tinker_pdf_font::Segment;
    let s = |v: f64| v * factor;
    Outline {
        segments: outline
            .segments
            .iter()
            .map(|segment| match *segment {
                Segment::MoveTo { x, y } => Segment::MoveTo { x: s(x), y: s(y) },
                Segment::LineTo { x, y } => Segment::LineTo { x: s(x), y: s(y) },
                Segment::QuadTo { cx, cy, x, y } => Segment::QuadTo {
                    cx: s(cx),
                    cy: s(cy),
                    x: s(x),
                    y: s(y),
                },
                Segment::CurveTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => Segment::CurveTo {
                    c1x: s(c1x),
                    c1y: s(c1y),
                    c2x: s(c2x),
                    c2y: s(c2y),
                    x: s(x),
                    y: s(y),
                },
                Segment::Close => Segment::Close,
            })
            .collect(),
    }
}
