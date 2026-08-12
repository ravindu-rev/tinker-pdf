//! Reading font dictionaries (9.5–9.8).
//!
//! This is the join between a PDF font dictionary and the leaf font crate: it
//! decides, for every byte a content stream shows, which code it is, how wide
//! that code is, and what character it means. Nothing here parses a font
//! program — that is `tinker-pdf-font`'s job, and it never sees a COS type.

use std::collections::HashMap;
use std::sync::Arc;

use tinker_pdf_font::cmap::CMap;
use tinker_pdf_font::{base_char, base_glyph_name, glyph_name_to_char, BaseEncoding, Standard14};

use crate::doc::CosDocument;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};

/// Which of 9.6/9.7's font families a dictionary belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontKind {
    /// Type 1, including the standard 14 (9.6.2).
    Type1,
    /// TrueType (9.6.3).
    TrueType,
    /// Type 3, whose glyphs are content streams (9.6.5).
    Type3,
    /// Type 0, composite, with a descendant CIDFont (9.7).
    Type0,
}

/// One code decoded from a string, ready to lay out.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedCode {
    /// The character code, as the CMap or byte gave it.
    pub code: u32,
    /// How many bytes the code occupied.
    pub bytes: u8,
    /// The advance in text-space units (1/1000 em).
    pub width: f64,
    /// The text this code stands for, empty when nothing maps it.
    pub text: String,
    /// Whether the width came from the document rather than a fallback.
    pub width_is_exact: bool,
}

/// A font dictionary, read.
pub struct Font {
    kind: FontKind,
    /// `/BaseFont`, for diagnostics and standard-14 matching.
    base_font: String,
    /// Simple fonts: the width of each code, from `/Widths`.
    widths: HashMap<u32, f64>,
    /// `/MissingWidth` from the descriptor, or zero.
    missing_width: f64,
    /// Simple fonts: the glyph name each code maps to, after `/Differences`.
    differences: HashMap<u32, String>,
    base_encoding: BaseEncoding,
    /// Whether the dictionary *named* a base encoding, rather than
    /// `base_encoding` being this struct's default.
    ///
    /// 9.6.6 turns on the difference: where the document names an encoding it
    /// wins, and where it does not the font program's own built-in encoding
    /// is what applies. A `/Differences` array with no `/BaseEncoding` names
    /// the codes it lists and leaves every other code to the font.
    base_encoding_named: bool,
    /// Composite fonts: how bytes become codes, and codes become CIDs.
    encoding_cmap: Option<CMap>,
    /// `/ToUnicode`, when the document supplies one.
    to_unicode: Option<CMap>,
    /// Composite fonts: `/W` widths by CID, and `/DW`'s default.
    cid_widths: HashMap<u32, f64>,
    default_width: f64,
    /// A standard face's built-in metrics, when the font names one and gives
    /// no widths of its own.
    standard: Option<Standard14>,
    /// Vertical writing, from the encoding CMap (9.7.4.3).
    vertical: bool,
    /// True when the font is symbolic, which changes how codes are read.
    symbolic: bool,
}

impl Font {
    /// Which family this font belongs to.
    #[must_use]
    pub fn kind(&self) -> FontKind {
        self.kind
    }

    /// `/BaseFont`, as written.
    #[must_use]
    pub fn base_font(&self) -> &str {
        &self.base_font
    }

    /// Whether the writing mode is vertical.
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        self.vertical
    }

    /// Whether the font is marked symbolic in its descriptor (9.8.2).
    #[must_use]
    pub fn is_symbolic(&self) -> bool {
        self.symbolic
    }

    /// Splits a string into codes and resolves each one.
    ///
    /// A simple font's codes are single bytes (9.6.6); a composite font's come
    /// from its encoding CMap, which is why a two-byte font cannot be read
    /// byte at a time.
    #[must_use]
    pub fn decode(&self, bytes: &[u8]) -> Vec<DecodedCode> {
        let raw: Vec<(u32, u8)> = match (&self.encoding_cmap, self.kind) {
            (Some(cmap), FontKind::Type0) => cmap.decode_codes(bytes),
            _ => bytes.iter().map(|&b| (u32::from(b), 1u8)).collect(),
        };

        raw.into_iter()
            .map(|(code, len)| {
                let (width, exact) = self.width_of(code);
                DecodedCode {
                    code,
                    bytes: len,
                    width,
                    text: self.text_of(code),
                    width_is_exact: exact,
                }
            })
            .collect()
    }

    /// The CID a code selects, through the encoding CMap (9.7.4).
    ///
    /// A code the CMap does not map is its own CID, which is what
    /// `Identity-H` says and what a broken CMap degrades to.
    #[must_use]
    pub fn cid_of(&self, code: u32) -> u32 {
        self.encoding_cmap
            .as_ref()
            .and_then(|c| c.cid(code))
            .unwrap_or(code)
    }

    /// The glyph name the *document* gives a code, where it gives one.
    ///
    /// `/Differences` first, then the base encoding the dictionary named
    /// (9.6.6). `None` means the document named nothing for this code, which
    /// is not the same as naming a glyph the font turns out not to have: the
    /// first sends a caller to the font program's own encoding, the second
    /// does not.
    ///
    /// This is a glyph *name* rather than a character on purpose. Type 1 and
    /// CFF fonts address their glyphs by name and by nothing else, so going
    /// through a character — which is what [`Font::text_of`] gives — loses
    /// every glyph whose name has no Unicode meaning, and that is most of a
    /// subset font's.
    #[must_use]
    pub fn glyph_name(&self, code: u32) -> Option<&str> {
        if let Some(name) = self.differences.get(&code) {
            return Some(name);
        }
        if !self.base_encoding_named {
            return None;
        }
        base_glyph_name(self.base_encoding, u8::try_from(code).ok()?)
    }

    /// The advance of one code, in 1/1000 em, and whether it is the
    /// document's own number.
    #[must_use]
    pub fn width_of(&self, code: u32) -> (f64, bool) {
        if self.kind == FontKind::Type0 {
            let cid = self.cid_of(code);
            if let Some(w) = self.cid_widths.get(&cid) {
                return (*w, true);
            }
            // 9.7.4.3: /DW defaults to 1000 when absent.
            return (self.default_width, false);
        }

        if let Some(w) = self.widths.get(&code) {
            return (*w, true);
        }

        // A standard face supplies its own metrics when the document does not.
        if let Some(std) = self.standard {
            if let Some(c) = self.char_of(code) {
                let (w, exact) = std.advance(c);
                return (f64::from(w), exact);
            }
        }

        (self.missing_width, false)
    }

    /// The character a code stands for, before `/ToUnicode` is consulted.
    fn char_of(&self, code: u32) -> Option<char> {
        let byte = u8::try_from(code).ok()?;
        if let Some(name) = self.differences.get(&code) {
            return glyph_name_to_char(name);
        }
        base_char(self.base_encoding, byte)
    }

    /// The text a code stands for.
    ///
    /// `/ToUnicode` wins where it exists, because it is the producer's own
    /// statement of what the glyph means; the encoding is the fallback. An
    /// empty result means nothing could map the code, which a caller reports
    /// rather than papering over with a replacement character.
    #[must_use]
    pub fn text_of(&self, code: u32) -> String {
        if let Some(cmap) = &self.to_unicode {
            if let Some(text) = cmap.to_unicode_string(code) {
                if !text.is_empty() {
                    return text;
                }
            }
        }
        self.char_of(code).map(String::from).unwrap_or_default()
    }
}

/// Reads a font dictionary.
#[must_use]
pub fn read(doc: &CosDocument, dict: &Dict) -> Font {
    let subtype = doc
        .resolve_key(dict, doc.intern(b"Subtype"))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
        .map(|b| b.to_vec())
        .unwrap_or_default();

    let kind = match subtype.as_slice() {
        b"Type0" => FontKind::Type0,
        b"TrueType" => FontKind::TrueType,
        b"Type3" => FontKind::Type3,
        _ => FontKind::Type1,
    };

    let base_font = doc
        .resolve_key(dict, doc.intern(b"BaseFont"))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();

    let mut font = Font {
        kind,
        base_font: base_font.clone(),
        widths: HashMap::new(),
        missing_width: 0.0,
        differences: HashMap::new(),
        base_encoding: BaseEncoding::Standard,
        base_encoding_named: false,
        encoding_cmap: None,
        to_unicode: read_to_unicode(doc, dict),
        cid_widths: HashMap::new(),
        default_width: 1000.0,
        standard: None,
        vertical: false,
        symbolic: false,
    };

    read_encoding(doc, dict, &mut font);

    match kind {
        FontKind::Type0 => read_composite(doc, dict, &mut font),
        _ => read_simple(doc, dict, &mut font, &base_font),
    }

    font
}

fn read_to_unicode(doc: &CosDocument, dict: &Dict) -> Option<CMap> {
    let r = dict.get_ref(doc.intern(b"ToUnicode"))?;
    let bytes = doc.stream_decoded(r).ok()?;
    Some(tinker_pdf_font::cmap::parse(&bytes))
}

/// `/Encoding` is a name, or a dictionary with `/BaseEncoding` and
/// `/Differences` (9.6.6), or for a composite font a predefined CMap name or
/// an embedded CMap stream (9.7.5).
fn read_encoding(doc: &CosDocument, dict: &Dict, font: &mut Font) {
    let key = doc.intern(b"Encoding");

    let named = |bytes: &[u8], font: &mut Font| match bytes {
        b"WinAnsiEncoding" => {
            font.base_encoding = BaseEncoding::WinAnsi;
            font.base_encoding_named = true;
        }
        b"MacRomanEncoding" => {
            font.base_encoding = BaseEncoding::MacRoman;
            font.base_encoding_named = true;
        }
        b"StandardEncoding" | b"MacExpertEncoding" => {
            // MacExpert is a distinct set this engine does not carry; falling
            // back to Standard keeps ASCII right, which is most of it.
            font.base_encoding = BaseEncoding::Standard;
            font.base_encoding_named = true;
        }
        other => {
            if let Some(cmap) = CMap::predefined(other) {
                font.vertical = cmap.is_vertical();
                font.encoding_cmap = Some(cmap);
            }
        }
    };

    // An embedded CMap stream, for a composite font.
    if let Some(r) = dict.get_ref(key) {
        if let Ok(bytes) = doc.stream_decoded(r) {
            let cmap = tinker_pdf_font::cmap::parse(&bytes);
            font.vertical = cmap.is_vertical();
            font.encoding_cmap = Some(cmap);
            return;
        }
    }

    let value = doc.resolve_key(dict, key);
    if let Some(n) = value.as_name() {
        if let Some(bytes) = doc.name_bytes(n) {
            named(&bytes, font);
        }
        return;
    }

    let Some(enc) = value.as_dict() else { return };

    if let Some(base) = doc
        .resolve_key(enc, doc.intern(b"BaseEncoding"))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
    {
        named(&base, font);
    }

    // 9.6.6.1: /Differences is a flat array of numbers and names, where each
    // number restarts the code counter.
    let diffs = doc.resolve_key(enc, doc.intern(b"Differences"));
    if let Some(items) = diffs.as_array() {
        let mut code = 0u32;
        for item in items {
            match item {
                Object::Int(n) => code = u32::try_from(*n).unwrap_or(code),
                Object::Real(r) => code = (*r).max(0.0) as u32,
                Object::Name(n) => {
                    if let Some(bytes) = doc.name_bytes(*n) {
                        font.differences
                            .insert(code, String::from_utf8_lossy(&bytes).into_owned());
                    }
                    code = code.saturating_add(1);
                }
                _ => {}
            }
        }
    }
}

fn read_simple(doc: &CosDocument, dict: &Dict, font: &mut Font, base_font: &str) {
    let first = doc
        .resolve_key(dict, doc.intern(b"FirstChar"))
        .as_int()
        .unwrap_or(0);

    let widths = doc.resolve_key(dict, doc.intern(b"Widths"));
    if let Some(items) = widths.as_array() {
        for (i, item) in items.iter().enumerate() {
            let resolved = doc.resolve(item);
            if let Some(w) = resolved.as_number() {
                if let Ok(code) = u32::try_from(first + i as i64) {
                    font.widths.insert(code, w);
                }
            }
        }
    }

    if let Some(desc) = descriptor(doc, dict) {
        font.missing_width = doc
            .resolve_key(&desc, doc.intern(b"MissingWidth"))
            .as_number()
            .unwrap_or(0.0);
        // 9.8.2 Table 121: bit 3 is the symbolic flag.
        if let Some(flags) = doc.resolve_key(&desc, doc.intern(b"Flags")).as_int() {
            font.symbolic = flags & 0b100 != 0;
        }
    }

    // Standard-14 metrics only matter when the document gave none of its own.
    if font.widths.is_empty() {
        font.standard = Standard14::from_base_font(base_font);
        // 9.6.2.2: a standard font with no /Encoding is Standard-encoded, but
        // a non-symbolic one is almost always intended as WinAnsi by
        // producers that omit it. Standard stays the default; only the
        // symbolic faces need their own handling, which they get by name.
    }
}

fn read_composite(doc: &CosDocument, dict: &Dict, font: &mut Font) {
    // 9.7.1: exactly one descendant, in an array.
    let descendants = doc.resolve_key(dict, doc.intern(b"DescendantFonts"));
    let Some(first) = descendants.as_array().and_then(<[Object]>::first).cloned() else {
        return;
    };
    let descendant = doc.resolve(&first);
    let Some(cid_font) = descendant.as_dict() else {
        return;
    };

    font.default_width = doc
        .resolve_key(cid_font, doc.intern(b"DW"))
        .as_number()
        .unwrap_or(1000.0);

    // 9.7.4.3: /W is [ c [w1 w2 ...] ] or [ cfirst clast w ], mixed freely.
    let w = doc.resolve_key(cid_font, doc.intern(b"W"));
    if let Some(items) = w.as_array() {
        let values: Vec<Arc<Object>> = items.iter().map(|o| doc.resolve(o)).collect();
        let mut i = 0usize;
        while i < values.len() {
            let Some(start) = values.get(i).and_then(|o| o.as_int()) else {
                break;
            };
            match values.get(i + 1).map(|o| o.as_ref()) {
                Some(Object::Array(list)) => {
                    for (k, item) in list.iter().enumerate() {
                        if let Some(width) = doc.resolve(item).as_number() {
                            if let Ok(cid) = u32::try_from(start + k as i64) {
                                font.cid_widths.insert(cid, width);
                            }
                        }
                    }
                    i += 2;
                }
                Some(_) => {
                    let (Some(end), Some(width)) = (
                        values.get(i + 1).and_then(|o| o.as_int()),
                        values.get(i + 2).and_then(|o| o.as_number()),
                    ) else {
                        break;
                    };
                    // A hostile range would fill memory; bound it to the CID
                    // space, which is what a real font can address anyway.
                    let end = end.min(start + 65_535);
                    for cid in start..=end {
                        if let Ok(cid) = u32::try_from(cid) {
                            font.cid_widths.insert(cid, width);
                        }
                    }
                    i += 3;
                }
                None => break,
            }
        }
    }

    if let Some(desc) = descriptor(doc, cid_font) {
        if let Some(flags) = doc.resolve_key(&desc, doc.intern(b"Flags")).as_int() {
            font.symbolic = flags & 0b100 != 0;
        }
    }
}

fn descriptor(doc: &CosDocument, dict: &Dict) -> Option<Arc<Dict>> {
    let value = doc.resolve_key(dict, doc.intern(b"FontDescriptor"));
    value.as_dict().map(|d| Arc::new(d.clone()))
}

/// The font dictionaries in one resource dictionary, by resource name.
#[must_use]
pub fn from_resources(doc: &CosDocument, resources: &Dict) -> HashMap<Name, Arc<Font>> {
    let mut out = HashMap::new();
    let value = doc.resolve_key(resources, doc.intern(b"Font"));
    let Some(fonts) = value.as_dict() else {
        return out;
    };

    for (key, entry) in fonts.iter() {
        let resolved = doc.resolve(entry);
        if let Some(dict) = resolved.as_dict() {
            out.insert(*key, Arc::new(read(doc, dict)));
        }
    }
    out
}

/// Reads the font dictionary at `r`.
#[must_use]
pub fn at(doc: &CosDocument, r: ObjRef) -> Option<Font> {
    let object = doc.get(r).ok()?;
    let dict = object.as_dict()?;
    Some(read(doc, dict))
}
