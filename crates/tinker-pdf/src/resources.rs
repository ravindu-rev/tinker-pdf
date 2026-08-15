//! Resolving a page's resources for the interpreter and the renderer.
//!
//! The two seams — `FontSource` for the interpreter, `GlyphSource` for the
//! renderer — exist so those crates never see a PDF dictionary. This is where
//! the dictionaries actually get read.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_content::{Device, FontSource, Layer, Matrix, PathSegment, Rgb};
use tinker_pdf_cos::{
    font as cos_font, limits, pages as cos_pages, CosDocument, Dict, Name, ObjRef, Object,
};
use tinker_pdf_filters::{
    ccitt_decode, jbig2_decode, jpeg_decode, CcittParams, Jbig2Params, JpegColor,
    Warning as FilterWarning,
};
use tinker_pdf_font::{base_glyph_name, cff::Cff, glyf, BaseEncoding, Outline, Sfnt, Type1};
use tinker_pdf_raster::canvas::{Canvas, Color};
use tinker_pdf_render::{DecodedImage, GlyphSource, PatternPaint, Shading, TilingPattern};

use crate::fonts::{self, FontProvider, FontRequest};
use crate::optional::OptionalContent;

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
    /// Resource names that named no font this build could resolve, and — when
    /// glyphs were being drawn — names whose font resolved no glyph for a
    /// code the page used.
    missing_fonts: Mutex<Vec<String>>,
    /// Images that decoded with something tolerated, as `(resource name,
    /// what the decoder tolerated)`. Ruling 10: the leaf crate says what it
    /// forgave, and this is where the object it happened in gets attached.
    damaged_images: Mutex<Vec<(String, String)>>,
    /// The host's substitute faces, kept so a resource dictionary *inside*
    /// this one can be read with the same configuration.
    ///
    /// A tiling pattern's `/Resources` (8.7.3.2) is read into a
    /// `PageResources` of its own, and without this the cell would be the one
    /// place in a document where a `FontProvider` the caller installed does
    /// not apply — text inside a pattern would silently lose its substitute
    /// face while the same text beside it kept one.
    provider: Option<Arc<dyn FontProvider>>,
    /// Which optional-content layers the document's default configuration
    /// shows (8.11).
    ///
    /// Bound here, once, rather than looked up per `BDC`: the catalog is
    /// already in reach through `doc`, the answer cannot change during a
    /// render — this build has no layer-toggle API to change it with — and a
    /// page that marks every drawing operator would otherwise walk
    /// `/OCProperties` thousands of times for one constant.
    optional: OptionalContent,
}

/// Applies `/Decode [1 0]` to already-decoded samples.
///
/// The general `/Decode` machinery works on raw samples before colour
/// conversion, which the JPEG and CCITT paths have already done by the time
/// they return. Reversal is the only form either of them meets in practice —
/// `/Decode [1 0]` on a fax, `[1 0 1 0 1 0]` on a JPEG — and inverting the
/// output is exactly equivalent for those. Any other range is left alone
/// rather than approximated.
fn invert_if_decode_reverses(decode: &[(f64, f64)], rgb: &mut [u8]) {
    if decode.is_empty() || !decode.iter().all(|(a, b)| *a > *b) {
        return;
    }
    for value in rgb.iter_mut() {
        *value = 255 - *value;
    }
}

/// What an inline image is called in a warning. It has no resource name to be
/// called anything else by, which is also why it is not cached.
const INLINE_NAME: &str = "inline";

/// A JPEG's pixels, ready to blit (7.4.8).
///
/// Shared by the XObject path and the inline one so the two differ in nothing
/// but where the still-coded bytes came from — the ceiling, the colour
/// conversion and the `/Decode` reversal included.
///
/// A JPEG carries its own dimensions and its own colour, so it never joins the
/// generic sample loop; `/Decode` therefore has to be applied to the converted
/// output rather than to raw samples, which is what
/// [`invert_if_decode_reverses`] is for.
fn jpeg_image(
    raw: &[u8],
    decode: &[(f64, f64)],
    interpolate: bool,
) -> Result<DecodedImage, String> {
    let image = jpeg_decode(raw, 1 << 28).map_err(|e| format!("{e:?}"))?;
    let mut rgb = jpeg_to_rgb(&image);
    invert_if_decode_reverses(decode, &mut rgb);
    Ok(DecodedImage {
        width: image.width,
        height: image.height,
        rgb,
        alpha: Vec::new(),
        stencil: false,
        interpolate,
    })
}

/// Rewrites an inline image's dictionary into the long spellings the shared
/// image path reads (8.9.7, Table 93).
///
/// Name by name rather than by text substitution. A search for `"/Fl "` finds
/// neither `/F/Fl` nor `[/AHx/Fl]`, and both are ordinary PDF — 7.2.2 makes
/// the `/` its own delimiter, so no producer is obliged to write the space
/// that search depends on. The consequence was silent and total: the whole
/// `/Filter` entry stayed a two-letter name nothing matched, so a compressed
/// inline image was sampled straight out of its compressed bytes, and `/DP`
/// written tight against its dictionary took the predictor down with it.
fn expand_inline_abbreviations(dict: &[u8]) -> Vec<u8> {
    // Every abbreviation of Table 93 and of Table 6, keys and values both:
    // `/CS /G` is a colour space named the short way, and `/F /AHx` a filter.
    const ABBREVIATIONS: &[(&[u8], &[u8])] = &[
        (b"BPC", b"BitsPerComponent"),
        (b"CS", b"ColorSpace"),
        (b"D", b"Decode"),
        (b"DP", b"DecodeParms"),
        (b"F", b"Filter"),
        (b"H", b"Height"),
        (b"IM", b"ImageMask"),
        (b"I", b"Interpolate"),
        (b"W", b"Width"),
        (b"G", b"DeviceGray"),
        (b"RGB", b"DeviceRGB"),
        (b"CMYK", b"DeviceCMYK"),
        (b"AHx", b"ASCIIHexDecode"),
        (b"A85", b"ASCII85Decode"),
        (b"LZW", b"LZWDecode"),
        (b"Fl", b"FlateDecode"),
        (b"RL", b"RunLengthDecode"),
        (b"CCF", b"CCITTFaxDecode"),
        (b"DCT", b"DCTDecode"),
    ];
    // 7.2.2: everything that ends a name. A regular character is anything
    // else — `#` included, so an escaped name is read whole, matches no
    // abbreviation and comes back out exactly as it went in.
    const DELIMITERS: &[u8] = b"()<>[]{}/%";
    let is_regular = |b: u8| !b.is_ascii_whitespace() && b != 0 && !DELIMITERS.contains(&b);

    let mut out: Vec<u8> = Vec::with_capacity(dict.len() + 32);
    out.extend_from_slice(b"<< ");
    let mut i = 0usize;
    while let Some(&byte) = dict.get(i) {
        if byte != b'/' {
            out.push(byte);
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while dict.get(end).copied().is_some_and(&is_regular) {
            end += 1;
        }
        let name = dict.get(start..end).unwrap_or_default();
        let long = ABBREVIATIONS
            .iter()
            .find(|(short, _)| *short == name)
            .map_or(name, |(_, long)| *long);
        out.push(b'/');
        out.extend_from_slice(long);
        // No separator is added: whatever ended the short name — white space,
        // a delimiter or the end of the dictionary — ends the long one too.
        i = end;
    }
    out.extend_from_slice(b" >>");
    out
}

impl PageResources {
    /// The font resource names that could not be resolved.
    #[must_use]
    pub fn missing_fonts(&self) -> Vec<String> {
        self.missing_fonts
            .lock()
            .map(|names| names.clone())
            .unwrap_or_default()
    }

    /// Images that decoded with a leniency, and which leniency (ruling 10).
    #[must_use]
    pub fn damaged_images(&self) -> Vec<(String, String)> {
        self.damaged_images
            .lock()
            .map(|images| images.clone())
            .unwrap_or_default()
    }

    /// Reads a page's resource dictionary.
    #[must_use]
    pub fn new(
        doc: &Arc<CosDocument>,
        page: &cos_pages::Page,
        provider: Option<&Arc<dyn FontProvider>>,
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
                    if let Some(program) =
                        program_for(doc, &key, dict, &font, provider.map(|p| &**p))
                    {
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
            missing_fonts: Mutex::new(Vec::new()),
            damaged_images: Mutex::new(Vec::new()),
            provider: provider.cloned(),
            optional: OptionalContent::bind(doc),
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
        provider: Option<&Arc<dyn FontProvider>>,
    ) -> PageResources {
        let mut fonts = HashMap::new();
        let mut font_ids = HashMap::new();
        let mut programs = HashMap::new();

        for (name, font) in cos_font::from_resources(doc, &dict) {
            if let Some(bytes) = doc.name_bytes(name) {
                let key = bytes.to_vec();
                let id = u64::from(name.id());
                if let Some(program) = program_for(doc, &key, &dict, &font, provider.map(|p| &**p))
                {
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
            missing_fonts: Mutex::new(Vec::new()),
            damaged_images: Mutex::new(Vec::new()),
            provider: provider.cloned(),
            optional: OptionalContent::bind(doc),
        }
    }

    /// A pattern resource: its dictionary, and the object it lives in when it
    /// is an indirect one (8.7.3).
    ///
    /// The reference is what a *tiling* pattern needs and a shading pattern
    /// does not — type 1 is a stream and its cell is the stream's bytes, type 2
    /// is a dictionary that may perfectly well be written inline. So both come
    /// back, and both routes read the dictionary through the same lookup:
    /// `/PatternType` deciding the kind is the one thing they must agree on.
    fn pattern_entry(&self, name: &[u8]) -> Option<(Dict, Option<ObjRef>)> {
        let resources = self.resources.as_ref()?;
        let table = self.doc.resolve_key(resources, self.doc.intern(b"Pattern"));
        let table = table.as_dict()?;
        let key = self.doc.intern(name);
        let reference = table.get_ref(key);
        let entry = self.doc.resolve_key(table, key);
        Some((entry.as_dict()?.clone(), reference))
    }

    /// A pattern's `/Matrix`, which maps pattern space to the parent content
    /// stream's default space (8.7.3.1).
    fn pattern_matrix(&self, dict: &Dict) -> Matrix {
        self.doc
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
            .unwrap_or(Matrix::IDENTITY)
    }

    /// A tiling pattern's geometry (8.7.3.2, Table 75).
    ///
    /// The cell's pixels are not read here: they are a content stream, and the
    /// renderer decides how large a buffer to draw it into and whether the
    /// lattice is affordable before anything is rasterized.
    fn tiling_pattern(&self, dict: &Dict, matrix: Matrix) -> PatternPaint {
        let number = |key: &[u8]| {
            self.doc
                .resolve_key(dict, self.doc.intern(key))
                .as_number()
                .filter(|v| v.is_finite())
        };

        // Required, and the cell has no extent without it. The corners may be
        // given in either order (7.9.5), as they may on a form's `/BBox`.
        let bbox = self
            .doc
            .resolve_key(dict, self.doc.intern(b"BBox"))
            .as_array()
            .and_then(|values| {
                let n = |i: usize| {
                    values
                        .get(i)
                        .and_then(|v| self.doc.resolve(v).as_number())
                        .filter(|v| v.is_finite())
                };
                let (x0, y0, x1, y1) = (n(0)?, n(1)?, n(2)?, n(3)?);
                Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
            });
        let Some(bbox) = bbox.filter(|b| b[2] > b[0] && b[3] > b[1]) else {
            return PatternPaint::Unsupported;
        };

        // 8.7.3.2: `/PaintType` 2 is the uncoloured pattern, which takes the
        // colour its `SCN` operands supplied. Anything that is not 2 is
        // coloured -- including a value the file got wrong, because a coloured
        // cell painted in its own colours is the reading that loses nothing.
        let uncolored = self
            .doc
            .resolve_key(dict, self.doc.intern(b"PaintType"))
            .as_int()
            .unwrap_or(1)
            == 2;

        PatternPaint::Tiling(Box::new(TilingPattern {
            matrix,
            bbox,
            // Absent or zero is invalid; the renderer falls back to the cell's
            // own size, which is the only reading that tiles.
            xstep: number(b"XStep").unwrap_or(0.0),
            ystep: number(b"YStep").unwrap_or(0.0),
            uncolored,
        }))
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
            b"Pattern" => return Some(ColorSpace::Pattern { base: None }),
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
                b"Pattern" => Some(ColorSpace::Pattern { base: None }),
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
            // 8.7.3.2: `[/Pattern base]` is the *uncoloured* pattern space.
            // The second element is where the operands of an `scn` that names
            // a `PaintType 2` pattern are measured; a bare `/Pattern` has no
            // second element and no colour. Without this branch the array form
            // resolved to nothing at all, so `cs` left the slot in whatever
            // space preceded it and the operands were read against that.
            b"Pattern" => Some(ColorSpace::Pattern {
                base: items
                    .get(1)
                    .and_then(|o| self.parse_space(o, depth + 1))
                    .map(Box::new),
            }),
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
        let Some(found) = self.fonts.get(font) else {
            // A name the resource dictionary does not define. Recorded so a
            // caller can tell "this page has no text" from "this page has text
            // whose font this build could not resolve".
            if let Ok(mut missing) = self.missing_fonts.lock() {
                let name = String::from_utf8_lossy(font).into_owned();
                if missing.len() < 64 && !missing.contains(&name) {
                    missing.push(name);
                }
            }
            return Vec::new();
        };
        let font = found;
        font.decode(bytes)
            .into_iter()
            .map(|d| (d.code, d.text, d.width))
            .collect()
    }

    fn is_vertical(&self, font: &[u8]) -> bool {
        self.fonts.get(font).is_some_and(|f| f.is_vertical())
    }

    fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64) {
        let Some(found) = self.fonts.get(font) else {
            // Unreachable in practice: an unresolved font decodes to no codes
            // at all, so nothing is ever shown to ask about. 9.7.4.3's default
            // rather than a zero displacement, so that a future caller getting
            // here still advances.
            return (0.0, 880.0, -1000.0);
        };
        // 9.7.4.3 keys /W2 by CID, and this is the same `cid_of` the advance
        // goes through — so the displacement and the position vector cannot
        // end up describing a different CID from the glyph that is drawn.
        found.vertical_metrics(found.cid_of(code))
    }

    fn font_id(&self, font: &[u8]) -> u64 {
        self.font_ids.get(font).copied().unwrap_or(0)
    }

    fn type3_glyph(&self, font: &[u8], code: u32) -> Option<(Vec<u8>, Matrix)> {
        // 9.6.5: only a Type 3 font has glyph procedures. Every other kind
        // returns None here, which leaves the ordinary outline path untouched.
        let dict = fonts::font_dict(&self.doc, self.resources.as_ref()?, font)?;
        let subtype = dict
            .get_name(self.doc.intern(b"Subtype"))
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Type3" {
            return None;
        }

        // A Type 3 font addresses its procedures by name, and the only way to
        // get one is the /Encoding /Differences array — there is no built-in
        // encoding for a font whose glyphs the document invented.
        let encoding = self.doc.resolve_key(&dict, self.doc.intern(b"Encoding"));
        let differences = self
            .doc
            .resolve_key(encoding.as_dict()?, self.doc.intern(b"Differences"));
        let differences = differences.as_array()?;

        let mut current = 0u32;
        let mut name: Option<Name> = None;
        for item in differences {
            match self.doc.resolve(item).as_ref() {
                Object::Int(v) => current = u32::try_from(*v).unwrap_or(0),
                Object::Real(v) => current = *v as u32,
                Object::Name(n) => {
                    if current == code {
                        name = Some(*n);
                        break;
                    }
                    current = current.saturating_add(1);
                }
                _ => {}
            }
        }
        let name = name?;

        let procs = self.doc.resolve_key(&dict, self.doc.intern(b"CharProcs"));
        let reference = procs.as_dict()?.get_ref(name)?;
        let content = self.doc.stream_decoded(reference).ok()?;

        // 9.6.5: /FontMatrix maps glyph space to text space. Unlike every
        // other font it is not 1/1000 by convention — a Type 3 font chooses
        // its own units, and the matrix is required for that reason.
        let matrix = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"FontMatrix"))
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
            })?;

        Some((content, matrix))
    }

    fn form(&self, name: &[u8]) -> Option<tinker_pdf_content::Form> {
        let (dict, reference) = self.xobject(name)?;
        let subtype = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"Subtype"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Form" {
            return None;
        }
        self.form_from(&dict, reference)
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

    fn ext_g_state_blend(&self, name: &[u8]) -> Option<tinker_pdf_content::BlendMode> {
        let resources = self.resources.as_ref()?;
        let table = self
            .doc
            .resolve_key(resources, self.doc.intern(b"ExtGState"));
        let entry = self
            .doc
            .resolve_key(table.as_dict()?, self.doc.intern(name));
        let dict = entry.as_dict()?;

        // 11.3.5: /BM is a name, or an array of names offering fallbacks.
        let value = self.doc.resolve_key(dict, self.doc.intern(b"BM"));
        if let Some(name) = value.as_name().and_then(|n| self.doc.name_bytes(n)) {
            return Some(tinker_pdf_content::BlendMode::from_name(&name));
        }
        let names: Vec<Vec<u8>> = value
            .as_array()?
            .iter()
            .filter_map(|item| self.doc.resolve(item).as_name())
            .filter_map(|n| self.doc.name_bytes(n))
            .map(|n| n.to_vec())
            .collect();
        (!names.is_empty())
            .then(|| tinker_pdf_content::BlendMode::from_names(names.iter().map(Vec::as_slice)))
    }

    fn ext_g_state_soft_mask(&self, name: &[u8]) -> Option<tinker_pdf_content::SoftMask> {
        let resources = self.resources.as_ref()?;
        let table = self
            .doc
            .resolve_key(resources, self.doc.intern(b"ExtGState"));
        let entry = self
            .doc
            .resolve_key(table.as_dict()?, self.doc.intern(name));
        let dict = entry.as_dict()?;

        // 11.6.5.1: `/SMask` is either the name `/None` or a mask dictionary.
        // The key being absent is *not* the same as `/None` — the first
        // leaves whatever mask is in force alone, the second removes it —
        // which is why the whole method returns an `Option` of an enum rather
        // than an `Option` of a mask.
        let value = self.doc.resolve_key(dict, self.doc.intern(b"SMask"));
        if let Some(name) = value.as_name().and_then(|n| self.doc.name_bytes(n)) {
            return (name.as_ref() == b"None").then_some(tinker_pdf_content::SoftMask::None);
        }
        let mask = value.as_dict()?;

        // `/G` is required, and is a form XObject with a transparency group.
        let reference = mask.get_ref(self.doc.intern(b"G"))?;
        let group_dict = self.doc.get(reference).ok()?.as_dict()?.clone();
        let form = self.form_from(&group_dict, reference)?;

        // 11.6.5.2: `/S` selects what the rendered group is read as. A name
        // this build does not know is `/Alpha`'s opposite rather than an
        // error, because `/Luminosity` is overwhelmingly the one in the wild
        // and a mask that fails to parse is a mask that hides nothing.
        let luminosity = self
            .doc
            .resolve_key(mask, self.doc.intern(b"S"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))
            .is_none_or(|s| s.as_ref() != b"Alpha");

        // `/BC` is in the group's own colour space, which is `/Group /CS` on
        // `/G`. Without one the components are read by count, which is what
        // every other unnamed space in this engine does.
        let backdrop = self
            .doc
            .resolve_key(mask, self.doc.intern(b"BC"))
            .as_array()
            .map(|values| {
                let components: Vec<f64> = values
                    .iter()
                    .filter_map(|v| self.doc.resolve(v).as_number())
                    .collect();
                let space = self
                    .doc
                    .resolve_key(&group_dict, self.doc.intern(b"Group"))
                    .as_dict()
                    .and_then(|g| g.get(self.doc.intern(b"CS")).cloned())
                    .and_then(|cs| self.parse_space(&cs, 0));
                let (r, g, b) = match space {
                    Some(space) => space.to_rgb(&components),
                    None => by_component_count(&components),
                };
                Rgb { r, g, b }
            });

        // 11.6.5.2: `/TR` is a function, or the name `/Identity`. A name is
        // the identity whatever it says, because the only alternative a
        // reader has for an unknown one is to mask nothing.
        let transfer = self
            .doc
            .resolve_key(mask, self.doc.intern(b"TR"))
            .as_dict()
            .and_then(|_| {
                let entry = self.doc.resolve_key(mask, self.doc.intern(b"TR"));
                self.parse_function(&entry, 0)
            })
            .map(|function| sample_transfer(&function));

        Some(tinker_pdf_content::SoftMask::Group(Box::new(
            tinker_pdf_content::MaskGroup {
                form,
                luminosity,
                backdrop,
                transfer,
            },
        )))
    }

    fn optional_content(&self, name: &[u8]) -> Option<Layer> {
        // 8.11.3.2: `/OC /MC0 BDC` names an entry in the /Properties
        // sub-dictionary of the current resource dictionary. The entry is an
        // optional content group or a membership dictionary; anything else is
        // an ordinary property list and hides nothing.
        let resources = self.resources.as_ref()?;
        let table = self
            .doc
            .resolve_key(resources, self.doc.intern(b"Properties"));
        let entry = table.as_dict()?.get(self.doc.intern(name))?.clone();
        let label = String::from_utf8_lossy(name).into_owned();
        Some(self.optional.layer_of(&self.doc, &entry, &label))
    }

    fn xobject_optional_content(&self, name: &[u8]) -> Option<Layer> {
        // 8.11.4.4: an /OC entry on a form or image XObject hides the whole
        // XObject, and is read from the XObject's own dictionary rather than
        // from /Properties.
        let (dict, _) = self.xobject(name)?;
        let entry = dict.get(self.doc.intern(b"OC"))?.clone();
        let label = String::from_utf8_lossy(name).into_owned();
        Some(self.optional.layer_of(&self.doc, &entry, &label))
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
        let (dict, _) = self.pattern_entry(name)?;

        // 8.7.3.3: type 1 is a tiling pattern, type 2 a shading pattern.
        let kind = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"PatternType"))
            .as_int()
            .unwrap_or(0);
        // 8.7.3.1: the pattern matrix maps pattern space to the *default*
        // space of the parent content stream, so the CTM in force at fill time
        // is not part of it. Shared by both kinds, because two readers of one
        // key is how the anchoring comes to be right on one route and wrong on
        // the other.
        let matrix = self.pattern_matrix(&dict);

        match kind {
            1 => Some(self.tiling_pattern(&dict, matrix)),
            2 => {
                let shading = self.doc.resolve_key(&dict, self.doc.intern(b"Shading"));
                let shading = shading.as_dict()?;
                // A mesh inside a pattern is as unpaintable as a mesh anywhere
                // else, and reports as an unpainted pattern rather than as a
                // missing one.
                let Ok(Some(shading)) = self.read_shading(shading) else {
                    return Some(PatternPaint::Unsupported);
                };
                Some(PatternPaint::Shading(Box::new(shading), matrix))
            }
            _ => Some(PatternPaint::Unsupported),
        }
    }

    fn tile(
        &self,
        name: &[u8],
        request: &tinker_pdf_render::TileRequest,
    ) -> Option<tinker_pdf_render::Tile> {
        let (dict, reference) = self.pattern_entry(name)?;
        // A tiling pattern is a *stream* (8.7.3.2, Table 75): its cell is a
        // content stream. A direct dictionary carrying the keys but no stream
        // has no cell to draw, and declining here is what turns it back into
        // the reported gap it was.
        let content = self.doc.stream_decoded(reference?).ok()?;

        // 8.7.3.2: the cell's own `/Resources`. Gap 11 recorded that a *form*
        // XObject's `/Resources` are consulted nowhere in this engine; a
        // pattern's are, because a cell that names `/F0` almost always means a
        // font the page never heard of — a pattern is a self-contained
        // drawing, where a form is usually written beside the page that
        // invokes it.
        let own = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"Resources"))
            .as_dict()
            .cloned();
        let nested;
        let resources: &PageResources = match own {
            Some(dict) => {
                nested = PageResources::from_dict(&self.doc, dict, self.provider.as_ref());
                &nested
            }
            // Falling back to the page's is what a producer that omits the key
            // is relying on, and it is what every reader does.
            None => self,
        };

        let canvas = Canvas::new(
            request.width,
            request.height,
            request.format,
            Color::TRANSPARENT,
        );
        let mut renderer = tinker_pdf_render::Renderer::new(canvas, request.to_pixels, resources)
            .with_cancel(request.cancel.clone())
            .with_pattern_depth(request.depth);

        // 8.7.3.2: the cell is clipped to its `/BBox`. The buffer is already
        // that box rounded outward, so this only takes back the part of a
        // pixel the rounding gave away -- but a hatch is normally drawn with a
        // line width that deliberately runs past the box so neighbouring cells
        // join, and without the clip that overshoot lands in the next cell's
        // gap.
        let [x0, y0, x1, y1] = request.bbox;
        let box_path = [
            PathSegment::MoveTo { x: x0, y: y0 },
            PathSegment::LineTo { x: x1, y: y0 },
            PathSegment::LineTo { x: x1, y: y1 },
            PathSegment::LineTo { x: x0, y: y1 },
            PathSegment::Close,
        ];
        // Pattern space, because `to_pixels` is the renderer's `base` and the
        // interpreter's CTM starts at the identity below.
        let state = tinker_pdf_content::GraphicsState::new(Matrix::IDENTITY);
        renderer.clip_path(&box_path, &state, false);

        tinker_pdf_content::interpret(&content, Matrix::IDENTITY, &mut renderer, resources);
        let (mut canvas, warnings) = renderer.finish();

        // 8.7.3.3: `PaintType 2`. The cell is a shape and the colour comes
        // from the `SCN` operands, so whatever the content stream said about
        // colour is discarded here rather than intercepted at every paint --
        // which is what makes "colour operators inside it are ignored" true of
        // operators this engine has not implemented yet as well.
        if let Some((r, g, b)) = request.color {
            canvas.recolor(Color::rgb(r, g, b));
        }

        // Ruling 10, and only reachable when the cell had resources of its
        // own: a font the cell could not resolve, or an image it had to
        // tolerate, is a fact about the page and the page is the only place a
        // caller looks.
        if !std::ptr::eq(resources, self) {
            if let (Ok(mut mine), names) = (self.missing_fonts.lock(), resources.missing_fonts()) {
                for name in names {
                    if mine.len() < 64 && !mine.contains(&name) {
                        mine.push(name);
                    }
                }
            }
            if let Ok(mut mine) = self.damaged_images.lock() {
                for entry in resources.damaged_images() {
                    if !mine.contains(&entry) {
                        mine.push(entry);
                    }
                }
            }
        }

        Some(tinker_pdf_render::Tile { canvas, warnings })
    }

    fn inline_image(&self, dict: &[u8], data: &[u8]) -> Result<Option<DecodedImage>, String> {
        let text = expand_inline_abbreviations(dict);

        let mut sink = tinker_pdf_cos::WarningSink::new();
        let parsed = tinker_pdf_cos::parse_object_at(&text, 0, self.doc.names_table(), &mut sink);
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
    /// A form XObject read from its own dictionary.
    ///
    /// Split out of [`FontSource::form`] because an ExtGState `/SMask`
    /// reaches a form through `/G` rather than through the `/XObject`
    /// resource table, and a second reader for the same dictionary is how
    /// `/Matrix` or `/BBox` comes to be honoured on one route and not the
    /// other.
    fn form_from(&self, dict: &Dict, reference: ObjRef) -> Option<tinker_pdf_content::Form> {
        let content = self.doc.stream_decoded(reference).ok()?;
        // 8.10.2: /Matrix maps the form's space into the one that invoked it.
        let matrix = self
            .doc
            .resolve_key(dict, self.doc.intern(b"Matrix"))
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

        // 8.10.2 requires it; a file that omits it gets an unclipped form
        // rather than no form.
        let bbox = self
            .doc
            .resolve_key(dict, self.doc.intern(b"BBox"))
            .as_array()
            .and_then(|values| {
                let n = |i: usize| values.get(i).and_then(|v| self.doc.resolve(v).as_number());
                // The corners may be given in either order (7.9.5).
                let (x0, y0, x1, y1) = (n(0)?, n(1)?, n(2)?, n(3)?);
                Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
            });

        // 11.6.6: `/Group` with `/S /Transparency` makes the form a
        // transparency group, which is composited as a unit rather than
        // element by element. Any other subtype — 8.10.3's `/S /Reference`
        // group — is not one, so the name is checked rather than the key's
        // presence.
        let group = self
            .doc
            .resolve_key(dict, self.doc.intern(b"Group"))
            .as_dict()
            .and_then(|group| {
                let subtype = self
                    .doc
                    .resolve_key(group, self.doc.intern(b"S"))
                    .as_name()
                    .and_then(|n| self.doc.name_bytes(n))?;
                if subtype.as_ref() != b"Transparency" {
                    return None;
                }
                // 11.6.6 Table 147: both default to false, and a value that is
                // not a boolean is not a value.
                Some(tinker_pdf_content::Group {
                    isolated: self
                        .doc
                        .resolve_key(group, self.doc.intern(b"I"))
                        .as_bool()
                        .unwrap_or(false),
                    knockout: self
                        .doc
                        .resolve_key(group, self.doc.intern(b"K"))
                        .as_bool()
                        .unwrap_or(false),
                })
            });

        Some(tinker_pdf_content::Form {
            content,
            matrix,
            bbox,
            group,
        })
    }

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

    /// Records that a font resolved no glyph for a code it was asked for.
    ///
    /// Step 4 of the fallback order in `docs/plans/gaps/01-cff-glyph-selection
    /// .md`: the glyph drawn is `.notdef`, and the page says so rather than
    /// silently drawing something else. Only rendering reaches this — text
    /// extraction never asks for an outline — so the same list stays exactly
    /// what its name says for the extraction path, which turns it into
    /// `TextWarning::UnknownFont`.
    fn report_unresolved_glyph(&self, font: &[u8]) {
        if let Ok(mut missing) = self.missing_fonts.lock() {
            let name = String::from_utf8_lossy(font).into_owned();
            if missing.len() < 64 && !missing.contains(&name) {
                missing.push(name);
            }
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
            // An OpenType font may carry its outlines in a `CFF ` table
            // instead of `glyf` — that is what the `OTTO` tag means. The sfnt
            // parser accepts `OTTO`, so such a font took this branch, found
            // no `glyf`, and `glyf::outline` returned an *empty* outline,
            // which the renderer reads as a legitimate space. The whole face
            // drew as blank with nothing reported.
            if sfnt.table(0x676C_7966).is_none() {
                if let Some(table) = sfnt.table(0x4346_4620) {
                    if let Some(cff) = Cff::parse(table) {
                        return self.cff_outline(&cff, font, &name, code);
                    }
                }
                return None;
            }

            let glyph = if font.kind() == cos_font::FontKind::Type0 {
                self.cid_glyph(font, &name, code)
            } else {
                match ch {
                    Some(c) => sfnt.glyph_for_char(c).filter(|g| *g != 0),
                    None => None,
                }
                // 9.6.6.4: a symbolic font's codes go through its `cmap`
                // directly rather than through a character — the (3,0)
                // subtable maps the code into the F0xx private-use block,
                // which is what `glyph_for_char` does with a character it is
                // handed. Without this a symbolic face fell straight to the
                // last resort below.
                .or_else(|| {
                    char::from_u32(code)
                        .and_then(|c| sfnt.glyph_for_char(c))
                        .filter(|g| *g != 0)
                })
                // The last resort: a subset font whose `cmap` was dropped,
                // where the code is the only glyph number on offer, and the
                // guess every reader makes for it. It used to be correct for
                // a composite font too — an identity `/CIDToGIDMap` makes the
                // code the glyph — but that reading only held while every
                // composite font's CMap was the identity as well. The branch
                // above owns that case now and indexes by CID, which is the
                // same number whenever the old reading was right and the
                // right one whenever it was not.
                .or_else(|| u16::try_from(code).ok())?
            };

            let units = f64::from(sfnt.units_per_em.max(1));
            let outline = glyf::outline(&sfnt, glyph)?;
            return Some(scale(&outline, 1.0 / units));
        }

        if let Some(cff) = Cff::parse(program) {
            return self.cff_outline(&cff, font, &name, code);
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

    /// Which glyph of a TrueType program a composite font's code selects
    /// (9.7.4.2).
    ///
    /// A CIDFontType2 addresses its descendant by CID, and `/CIDToGIDMap`
    /// turns that CID into a glyph index. The font program's own `cmap` is
    /// **not** consulted on this path and must not be: a `cmap` answers "which
    /// glyph draws this character", and a composite font's code is not a
    /// character. Until August 2026 it was the only thing consulted here — the
    /// code went through `/ToUnicode` to a character and the character through
    /// the `cmap` — so a document whose CMap was not the identity drew a page
    /// of real glyphs, correctly spaced, that were not the ones it named.
    ///
    /// A CID the map does not name draws `.notdef` and reports, the same way
    /// an unresolvable CFF glyph does. Without the report it is a space, which
    /// is the failure that looks like nothing happened.
    fn cid_glyph(&self, font: &cos_font::Font, resource: &[u8], code: u32) -> u16 {
        let cid = font.cid_of(code);
        let glyph = font.gid_for_cid(cid);
        // CID 0 is `.notdef` by definition and reports nothing; any other CID
        // resolving to glyph 0 is a CID this font does not carry.
        if glyph == 0 && cid != 0 {
            self.report_unresolved_glyph(resource);
        }
        glyph
    }

    /// One glyph of a CFF program, chosen the way 9.6.6 says to choose it.
    fn cff_outline(
        &self,
        cff: &Cff<'_>,
        font: &cos_font::Font,
        resource: &[u8],
        code: u32,
    ) -> Option<Outline> {
        let glyph = cff_glyph(cff, font, code).unwrap_or_else(|| {
            // Nothing named this glyph. `.notdef` is glyph 0 in every CFF
            // font — usually empty, sometimes a box — and the page reports
            // rather than drawing whichever glyph the code happened to number.
            self.report_unresolved_glyph(resource);
            0
        });
        let outline = cff.outline(glyph)?;
        // A CFF font matrix is usually 1/1000 but need not be, and a
        // CID-keyed font may carry a different one per Font DICT.
        let factor = cff.font_matrix_for(glyph).first().copied().unwrap_or(0.001);
        Some(scale(&outline, factor))
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

    /// Records that an image decoded with something tolerated (ruling 10).
    fn report_damaged_image(&self, name: &[u8], warning: FilterWarning) {
        if let Ok(mut damaged) = self.damaged_images.lock() {
            let entry = (
                String::from_utf8_lossy(name).into_owned(),
                warning.as_str().to_string(),
            );
            if damaged.len() < 64 && !damaged.contains(&entry) {
                damaged.push(entry);
            }
        }
    }

    /// Decodes a CCITT stream into the packed one-bit samples the sample loop
    /// in [`Self::decode_image_at`] reads (7.4.6).
    ///
    /// **The single entry point for fax data in this file.** The XObject path
    /// reaches it above; gap 08 routes `decode_inline` to this same function
    /// rather than growing a second copy of Table 11's defaults. That matters
    /// more than it looks: `ccitt_decode`'s signature does not change when the
    /// meaning of its bytes changes, so a second call site would be invisible
    /// to the compiler and would go on producing the old shape in silence.
    ///
    /// `/Columns` is the *coding* width and need not be `/Width` — Table 11
    /// defaults it to 1728 whatever the image is — so the decoded rows are
    /// re-laid at the image's own width here. The bits past `/Columns` in a
    /// decoded row are white, so a wider image gains white rather than an edge
    /// that is not in the fax.
    fn ccitt_samples(
        &self,
        dict: &Dict,
        coded: &[u8],
        width: u32,
        height: u32,
        name: &[u8],
    ) -> Vec<u8> {
        let parms = self.decode_parms(dict);
        let int = |key: &[u8]| parms.as_ref().and_then(|p| p.get_int(self.doc.intern(key)));
        let flag = |key: &[u8]| {
            parms
                .as_ref()
                .and_then(|p| p.get_bool(self.doc.intern(key)))
        };

        // Table 11: `/Rows` is the height of the coded image. It is optional,
        // and the image's own `/Height` is what it means when it is absent —
        // which is what this used to pass unconditionally, so a file whose two
        // heights disagreed was decoded at the wrong one.
        let rows = int(b"Rows")
            .filter(|rows| *rows > 0)
            .unwrap_or(i64::from(height))
            .clamp(1, 1 << 16) as u32;
        let params = CcittParams {
            k: int(b"K").unwrap_or(0) as i32,
            columns: int(b"Columns").unwrap_or(1728).clamp(1, 1 << 16) as u32,
            rows,
            black_is_1: flag(b"BlackIs1").unwrap_or(false),
            byte_align: flag(b"EncodedByteAlign").unwrap_or(false),
            // Table 11's other two flags, neither of which was consulted.
            // `/EndOfLine` says an EOL is required before every line, so a
            // missing one is a leniency worth naming rather than a shape the
            // decoder silently absorbs. `/EndOfBlock false` says the data
            // carries no EOFB, which is what makes `/Rows` above the authority
            // on where the image ends instead of a hint the decoder may run
            // past.
            end_of_line: flag(b"EndOfLine").unwrap_or(false),
            end_of_block: flag(b"EndOfBlock").unwrap_or(true),
        };

        // The same ceiling every other stream in the document decodes under.
        // Every row costs at least one bit of input, so the decode is bounded
        // by the coded length long before it reaches this.
        let (packed, warnings) = ccitt_decode(coded, &params, limits::MAX_DECODED_STREAM);
        for warning in warnings {
            // Ruling 10. These were discarded, so a fax with half its rows
            // missing was reported as a clean render of a mostly blank page.
            self.report_damaged_image(name, warning);
        }

        let coded_stride = (params.columns as usize).div_ceil(8);
        let stride = (width as usize).div_ceil(8);
        let white = if params.black_is_1 { 0x00 } else { 0xFF };
        let mut samples = vec![white; (height as usize).saturating_mul(stride)];
        let run = stride.min(coded_stride);
        for y in 0..height as usize {
            let from = y * coded_stride;
            let to = y * stride;
            let (Some(source), Some(target)) =
                (packed.get(from..from + run), samples.get_mut(to..to + run))
            else {
                break;
            };
            target.copy_from_slice(source);
        }
        samples
    }

    /// Decodes a JBIG2 stream into the packed one-bit samples the sample loop
    /// in [`Self::decode_image_at`] reads (ISO 32000-1 7.4.7).
    ///
    /// **The polarity inversion lives here, and nowhere else.** T.88 6.2.2
    /// codes 1 for black; a 1-bit DeviceGray sample is 0 for black. The
    /// decoder returns JBIG2's own sense unconverted — as [`T6Rows`] does for
    /// a fax — so exactly one place in the build knows which convention it is
    /// translating between, and it is the place that also reads `/Decode`,
    /// `/ImageMask` and `/ColorSpace`. A decoder that inverted for itself
    /// would be guessing what its caller wanted, and a second caller would
    /// invert again.
    ///
    /// **It joins the generic sample path deliberately**, for the reason gap
    /// 16 gave when it moved the fax onto it: an image that returns its own
    /// pixels has to reimplement `/Decode`, `/ImageMask` and the colour space
    /// to compose correctly, and a JBIG2 scan is an image mask about as often
    /// as it is a DeviceGray image. Coming out as *samples* means those three
    /// keys are read once, where they have always been read.
    ///
    /// `None` is the refusal: no region decoded, so the caller draws the
    /// placeholder rather than a blank page (ruling 2).
    fn jbig2_samples(
        &self,
        dict: &Dict,
        coded: &[u8],
        width: u32,
        height: u32,
        name: &[u8],
    ) -> Option<Vec<u8>> {
        // 7.4.7: `/JBIG2Globals` is a *stream*, in this image's own
        // `/DecodeParms`, holding the segments shared by every image that
        // names it. It is the ordinary shape for the symbol dictionary of a
        // scanned book, so a build that ignored it would refuse a file whose
        // whole payload is in one place and say nothing about why.
        let globals = self
            .decode_parms(dict)
            .and_then(|parms| match parms.get(self.doc.intern(b"JBIG2Globals")) {
                Some(Object::Ref(r)) => self.doc.stream_decoded(*r).ok(),
                _ => None,
            })
            .unwrap_or_default();

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &globals,
            width,
            height,
        };
        let decoded = jbig2_decode(coded, &params, limits::MAX_DECODED_STREAM, &mut warnings);
        for warning in &warnings {
            // Ruling 10, and it carries most of the information here: the
            // refusal below is one error with no detail, and "no region,
            // because this is a symbol dictionary" and "no region, because
            // the stream was truncated" are different failures.
            self.report_damaged_image(name, *warning);
        }

        let mut samples = decoded.ok()?;
        // The inversion, stated once. Padding bits past the image's width sit
        // in the same byte and flip with it, which is correct: they were 0 for
        // white in JBIG2 and become 1 for white here.
        for byte in &mut samples {
            *byte = !*byte;
        }
        Some(samples)
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

        // Table 89: /Interpolate asks for the samples to be smoothed when the
        // image is magnified. Read here rather than in either codec branch,
        // because both of those return before the end of this function — which
        // is how /Decode came to be ignored on exactly the two formats whose
        // producers use it most.
        let interpolate = self
            .doc
            .resolve_key(&dict, self.doc.intern(b"Interpolate"))
            .as_bool()
            .unwrap_or(false);

        // 8.9.5.2: /Decode remaps each component's sample range. Read here
        // rather than after the codec branches, because both of those used to
        // return before reaching it — so `/Decode [1 0]` on a JPEG or a fax
        // did nothing at all, and the image rendered as its own negative
        // without a word.
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
            // Every filter before the codec, and no further: a producer
            // that deflates its JPEG bytes writes `[/FlateDecode /DCTDecode]`,
            // and handing the undecoded stream to the JPEG decoder made it
            // refuse a perfectly good image.
            let raw = self
                .doc
                .stream_image_input(reference)
                .map_err(|_| "DCTDecode".to_string())?;
            return jpeg_image(&raw, &decode, interpolate);
        }
        if let Some(filter) = last_filter.as_deref() {
            if matches!(filter, b"JPXDecode") {
                return Err(String::from_utf8_lossy(filter).into_owned());
            }
        }

        // CCITT data likewise arrives still coded, and carries its own
        // parameters in /DecodeParms — but unlike a JPEG it decodes to
        // *samples*, one bit per pixel, which is what the image dictionary
        // says it is. So it joins the path below rather than returning its own
        // pixels: `/ImageMask`, `/Decode` and `/ColorSpace` are read once,
        // where they have always been read, and apply to a fax because a fax
        // now arrives there like everything else.
        let fax = matches!(
            last_filter.as_deref(),
            Some(b"CCITTFaxDecode") | Some(b"CCF")
        );
        // JBIG2 takes the same road, and for the same reason (gap 17). Both
        // are bilevel codecs that produce samples rather than pixels, and a
        // scanned page is an `/ImageMask` about as often as it is a DeviceGray
        // image — so composing correctly means arriving where those keys are
        // read, not reimplementing them.
        let jbig2 = matches!(last_filter.as_deref(), Some(b"JBIG2Decode"));
        let data = if jbig2 {
            let raw = self
                .doc
                .stream_image_input(reference)
                .map_err(|_| "JBIG2Decode".to_string())?;
            // The refusal. A stream whose regions this build cannot decode —
            // the symbol-dictionary lineage an OCR pipeline emits — comes back
            // `None`, and the caller draws the neutral placeholder. Returning
            // the blank page it was composited onto would be indistinguishable
            // from a correct decode of a blank scan.
            self.jbig2_samples(&dict, &raw, width, height, name)
                .ok_or_else(|| "JBIG2Decode".to_string())?
        } else if fax {
            let raw = self
                .doc
                .stream_image_input(reference)
                .map_err(|_| "CCITTFaxDecode".to_string())?;
            self.ccitt_samples(&dict, &raw, width, height, name)
        } else {
            // Everything else decodes to raw samples.
            self.doc
                .stream_decoded(reference)
                .map_err(|_| "undecodable".to_string())?
        };
        // 7.4.6 and 7.4.7: both bilevel codecs produce one bit per pixel,
        // whatever the dictionary claims. Producers that omit
        // `/BitsPerComponent` are common and the clamp already reads an absent
        // key as 1; one that writes 8 would otherwise read each row eight
        // times too wide.
        let bpc = if fax || jbig2 { 1 } else { bpc };

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
            interpolate,
        })
    }

    /// Decodes an inline image's samples (8.9.7).
    ///
    /// Separate from [`Self::decode_image_at`] because an inline image has no
    /// object number: its bytes are in hand rather than behind a stream tier,
    /// so the filter chain is run here instead of by the document.
    fn decode_inline(&self, dict: &Dict, data: &[u8]) -> Result<DecodedImage, String> {
        use tinker_pdf_filters::{apply_chain, ChainOutput, ImageCodec, Limits};

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

        // Table 93 abbreviates /Interpolate to /I, and the caller has already
        // rewritten the short form, so only the long one is looked for here.
        let interpolate = self
            .doc
            .resolve_key(dict, self.doc.intern(b"Interpolate"))
            .as_bool()
            .unwrap_or(false);

        // 8.9.5.2. Read before the chain rather than after it, because the DCT
        // branch below returns its own pixels and would otherwise never see
        // this — which is the mistake the XObject path made until gap 16.
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

        // The filters, in the order they were applied — read here only to name
        // one in a warning, since the chain itself is built by the document.
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
        let named = |at: usize| -> String {
            filters.get(at).or_else(|| filters.last()).map_or_else(
                || INLINE_NAME.to_string(),
                |n| String::from_utf8_lossy(n).into_owned(),
            )
        };

        // The chain the document would have built had these bytes been an
        // object. Built there rather than here on purpose: Table 10's
        // predictor defaults, `/EarlyChange` and the per-filter
        // `/DecodeParms` array form then have one implementation instead of
        // two that can drift. This path used to carry the second one, and it
        // passed no predictor at all and hardcoded `/EarlyChange` true.
        let mut chain_warnings = tinker_pdf_cos::WarningSink::new();
        let chain = self.doc.filter_chain(dict, &mut chain_warnings);
        if chain_warnings
            .warnings()
            .iter()
            .any(|w| w.kind == tinker_pdf_cos::WarningKind::FilterUnknown)
        {
            // A filter name this build does not know ends the chain where it
            // appears, so everything after it is still encoded. Reported,
            // rather than sampled as though it were pixels.
            return Err(named(chain.len()));
        }

        // The ceiling every other stream in the document decodes under, not a
        // second number that happens to be smaller. An inline image is
        // attacker-controlled bytes inside a content stream, and a page may
        // hold any number of them, so the bound here is a denial-of-service
        // question rather than a tidiness one — and one that had drifted to
        // half the shared value with nothing naming the relationship.
        let limits = Limits::new(limits::MAX_DECODED_STREAM);
        let report = |warnings: Vec<FilterWarning>| {
            // Ruling 10: what the chain forgave is named, the same way the
            // XObject path names it. An inline image is one object's worth of
            // attacker-controlled bytes inside a content stream, so "it drew"
            // and "it drew cleanly" have to stay apart here too.
            for warning in warnings {
                self.report_damaged_image(INLINE_NAME.as_bytes(), warning);
            }
        };
        // 8.9.7 permits DCT and CCITT inline, and both are ordinary there — a
        // scanned page pasted into a content stream is a fax, and a photograph
        // is a JPEG. Both used to hit a catch-all that returned `Err`, so both
        // drew the grey placeholder and reported a codec this build has in
        // fact had all along.
        let (bytes, fax) = match apply_chain(data, &chain, &limits) {
            Ok(ChainOutput::Bytes(decoded)) => {
                report(decoded.warnings);
                (decoded.data, false)
            }
            Ok(ChainOutput::EncodedImage {
                kind: ImageCodec::Dct,
                data,
                warnings,
            }) => {
                report(warnings);
                // The same decoder, through the same helper, as an image
                // XObject: a JPEG returns its own dimensions and its own
                // colour, so it does not join the sample loop below.
                return jpeg_image(&data, &decode, interpolate);
            }
            Ok(ChainOutput::EncodedImage {
                kind: ImageCodec::Ccitt,
                data,
                warnings,
            }) => {
                report(warnings);
                // Gap 16's single entry point, not a second call site. A fax
                // decodes to packed one-bit *samples*, so it joins the loop
                // below and gains `/ImageMask`, `/Decode` and `/ColorSpace`
                // with it. `ccitt_decode`'s signature says nothing about the
                // shape of what it returns, so a copy of this made here would
                // have gone on producing the old byte-per-pixel shape with
                // nothing in the build able to notice.
                (
                    self.ccitt_samples(dict, &data, width, height, INLINE_NAME.as_bytes()),
                    true,
                )
            }
            // JBIG2 and JPX are gated capabilities wherever they appear
            // (ruling 2), inline included.
            Ok(ChainOutput::EncodedImage { .. }) => {
                return Err(named(chain.len().saturating_sub(1)))
            }
            Err(_) => return Err(named(chain.len())),
        };
        // 7.4.6: fax output is one bit per pixel whatever the dictionary
        // claims, exactly as on the XObject path.
        let bpc = if fax { 1 } else { bpc };

        let space = self.doc.resolve_key(dict, self.doc.intern(b"ColorSpace"));
        let space = self
            .parse_space(&space, 0)
            .unwrap_or(ColorSpace::DeviceGray);
        let n = if is_mask { 1 } else { space.components() };

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
            interpolate,
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

/// Components read as a colour by how many of them there are.
///
/// 11.6.5.2 puts `/BC` in the mask group's own colour space, which a file may
/// simply not declare. One component is grey, three RGB, four CMYK — the same
/// reading the interpreter applies to an `sc` whose space it could not
/// resolve, and the only one available without a declaration.
fn by_component_count(components: &[f64]) -> (u8, u8, u8) {
    let space = match components.len() {
        1 => ColorSpace::DeviceGray,
        4 => ColorSpace::DeviceCmyk,
        _ => ColorSpace::DeviceRgb,
    };
    space.to_rgb(components)
}

/// A soft mask's `/TR` sampled to 256 entries (11.6.5.2).
///
/// Evaluated once here rather than per pixel: the function may be a Type 4
/// PostScript calculator, and running one for every pixel of a page-sized
/// mask is minutes of work. Its domain and range are both `[0 1]` by
/// definition, so 256 samples reproduce every value an 8-bit mask can hold
/// with no interpolation at all — the table is exact, not an approximation.
fn sample_transfer(function: &Function) -> [u8; 256] {
    let mut table = [0u8; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let input = f64::from(i as u16) / 255.0;
        let value = function.eval(&[input]).first().copied().unwrap_or(input);
        // A function may return anything; the mask cannot.
        let value = if value.is_finite() { value } else { input };
        *slot = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    table
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

/// Which glyph of a CFF program a character code selects.
///
/// The character code is not a glyph index and never was. Until August 2026
/// this function did not exist and its two callers used the code as one, so a
/// document embedding a Type 1 face as `/FontFile3 /Subtype /Type1C` — how
/// they have been embedded since PDF 1.2 — drew whatever glyph sat at that
/// position, or nothing at all when the font was subsetted small enough that
/// the position did not exist.
///
/// The order below is the one real files need, because real files omit
/// things. Each step is tried in turn and the first that names a glyph in
/// this font wins:
///
/// 1. the document's `/Encoding` gives a name, and the charset gives the
///    glyph — 9.6.6 makes the PDF's encoding win over the font's;
/// 2. the font program's own built-in encoding gives the glyph directly;
/// 3. the standard encoding's name for the code, through the charset;
/// 4. nothing, which the caller turns into `.notdef` plus a report.
///
/// Step 1 is tried whenever the dictionary named an encoding *and* that name
/// reaches a glyph. A name that reaches nothing falls through to step 2
/// rather than stopping there: 9.6.6 is about which encoding is authoritative,
/// not about refusing to look at a font that plainly disagrees with it, and a
/// subset font whose `/Differences` were written against the unsubsetted
/// original is a file that exists.
fn cff_glyph(cff: &Cff<'_>, font: &cos_font::Font, code: u32) -> Option<u16> {
    if font.kind() == cos_font::FontKind::Type0 {
        // 9.7.4.2: a composite font addresses its descendant by CID. A
        // CID-keyed program maps the CID through its charset; a plain CFF
        // inside a CIDFontType0 has no such map and the CID is the glyph.
        let cid = font.cid_of(code);
        return if cff.is_cid() {
            cff.gid_for_cid(cid)
        } else {
            u16::try_from(cid).ok()
        };
    }
    // A CID-keyed program under a simple font dictionary is malformed, and
    // the only reading of a one-byte code left is as a CID.
    if cff.is_cid() {
        return cff.gid_for_cid(code);
    }

    let byte = u8::try_from(code).ok();
    font.glyph_name(code)
        .and_then(|name| cff.gid_for_name(name))
        .or_else(|| byte.and_then(|c| cff.gid_for_code(c)))
        .or_else(|| {
            let name = base_glyph_name(BaseEncoding::Standard, byte?)?;
            cff.gid_for_name(name)
        })
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

#[cfg(test)]
mod tests {
    use super::{expand_inline_abbreviations, PageResources};
    use tinker_pdf_content::FontSource;

    fn expand(dict: &str) -> String {
        String::from_utf8(expand_inline_abbreviations(dict.as_bytes()))
            .expect("ascii in, ascii out")
    }

    /// Table 93's short keys expand whatever follows them.
    ///
    /// 7.2.2 makes `/` a delimiter in its own right, so a producer is free to
    /// write `/F/Fl` with nothing between the two names — and the substitution
    /// this replaced searched for `"/Fl "`, which is not in that string. The
    /// whole `/Filter` entry then stayed the name `/F`, matched no branch, and
    /// the compressed bytes were sampled as though they were pixels.
    #[test]
    fn an_abbreviation_expands_against_a_delimiter_as_well_as_a_space() {
        assert_eq!(
            expand("/W 4/H 4/CS/RGB/BPC 8/F/Fl"),
            "<< /Width 4/Height 4/ColorSpace/DeviceRGB/BitsPerComponent 8/Filter/FlateDecode >>"
        );
        assert_eq!(
            expand("/F[/AHx/Fl]/DP[null<</Predictor 15>>]"),
            "<< /Filter[/ASCIIHexDecode/FlateDecode]/DecodeParms[null<</Predictor 15>>] >>"
        );
    }

    /// A name that is not an abbreviation comes back exactly as it went in,
    /// including one carrying a `#` escape — which is a regular character in a
    /// name (7.3.5) and must not end it.
    #[test]
    fn a_name_that_is_not_an_abbreviation_is_untouched() {
        assert_eq!(expand("/Whirlpool"), "<< /Whirlpool >>");
        assert_eq!(expand("/CS /Sep#20A"), "<< /ColorSpace /Sep#20A >>");
        // `/D` is `/Decode` and `/DP` is `/DecodeParms`: the longer name is
        // read whole rather than as the shorter one plus a stray `P`.
        assert_eq!(
            expand("/D[1 0]/DP<<>>"),
            "<< /Decode[1 0]/DecodeParms<<>> >>"
        );
    }

    /// A one-page document whose `/ColorSpace` dictionary is given verbatim.
    fn resources_of(color_spaces: &str) -> PageResources {
        let content = "0 0 1 1 re f";
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10]\n\
   /Resources << /ColorSpace << {color_spaces} >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
            content.len() + 1
        );

        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        PageResources::new(&page.doc, &page.inner, None)
    }

    /// 8.7.3.2: `[/Pattern base]` is the uncoloured pattern space, and `base`
    /// is where the operands of an `scn` that names a `PaintType 2` pattern
    /// are measured.
    ///
    /// The array form resolved to nothing at all before this, so those
    /// operands could not be read in any space and an uncoloured pattern had
    /// no colour to take. The two spaces below read the same three numbers
    /// differently, so a binder that fell back to RGB for both fails on the
    /// second.
    #[test]
    fn an_uncoloured_pattern_space_resolves_through_its_underlying_space() {
        let res = resources_of(
            "/Prgb [ /Pattern /DeviceRGB ] \
             /Pcmyk [ /Pattern /DeviceCMYK ] \
             /Pbare /Pattern",
        );

        assert_eq!(res.color_components(b"Prgb"), Some(3));
        assert_eq!(
            res.resolve_color(b"Prgb", &[1.0, 0.0, 0.0]),
            Some(tinker_pdf_content::Rgb { r: 255, g: 0, b: 0 }),
        );

        assert_eq!(res.color_components(b"Pcmyk"), Some(4));
        assert_eq!(
            res.resolve_color(b"Pcmyk", &[1.0, 0.0, 0.0, 0.0]),
            Some(tinker_pdf_content::Rgb {
                r: 0,
                g: 255,
                b: 255
            }),
            "cyan, not red — the operands are inks here"
        );

        // 8.7.3.1: a *coloured* pattern space has no underlying space and no
        // colour of its own, which is the black every pattern used to paint.
        assert_eq!(
            res.resolve_color(b"Pbare", &[1.0, 0.0, 0.0]),
            Some(tinker_pdf_content::Rgb { r: 0, g: 0, b: 0 }),
        );
    }
}
