//! Building documents (phase 12).
//!
//! A thin authoring layer over the writer: pages, content, fonts, metadata and
//! an outline. It deliberately does no layout — placing what a caller already
//! positioned is this crate's business, and composing paragraphs is not.

use std::collections::{BTreeMap, BTreeSet};

use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::write::{rewrite, ObjectSet, StreamData, WriteOptions};

/// Image data to embed.
///
/// **`#[non_exhaustive]`**, from gap 29 milestone 4. A caller outside this
/// workspace matches with a wildcard arm, so the next image shape is an
/// addition rather than a break. It is marked here rather than when it is next
/// needed because marking it later costs the same break again for nothing, and
/// gaps 30 and 31 each expect to add one.
#[non_exhaustive]
pub enum ImageData<'a> {
    /// JPEG bytes, placed **as they are**.
    ///
    /// Never re-encoded: recompression is generational quality loss the
    /// caller cannot undo, and one who wants different quality can decode and
    /// re-add.
    Jpeg(&'a [u8]),
    /// Eight-bit RGB, three bytes per pixel, row-major from the top.
    Rgb8 {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// The samples.
        data: &'a [u8],
    },
    /// Eight-bit greyscale, one byte per pixel.
    Gray8 {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// The samples.
        data: &'a [u8],
    },
    /// Bytes that are **already** in the encoding their dictionary declares.
    ///
    /// [`ImageData::Jpeg`] is this idea for one codec: place the bytes, name
    /// the filter, do no pixel work. This generalises it, and gap 29 is why —
    /// a CBZ synthesises every page at open, so whatever a page holds is held
    /// for the whole document. Decoding each PNG to [`ImageData::Rgb8`] would
    /// cost *w x h x 3* a page, about 3.6 GB for a 200-page archive at
    /// 2000 x 3000; passing the compressed bytes through keeps the peak a
    /// small multiple of the archive's own size, and that multiple is a
    /// constant rather than a function of the pixel count.
    ///
    /// It works at all because a non-interlaced PNG's IDAT **is** a
    /// `/FlateDecode` stream with `/Predictor 15`, byte for byte: PDF's
    /// `/Predictor` is PNG 9.2's row-filter specification adopted wholesale,
    /// down to the per-row tag and the `ceil(colors x bpc / 8)` left-neighbour
    /// offset floored at one.
    ///
    /// The writer never re-encodes these bytes, and that is a contract rather
    /// than an accident: `maybe_compress` declines any stream whose dictionary
    /// already declares a `/Filter`, and
    /// `a_stream_that_already_declares_a_filter_is_handed_through_untouched`
    /// holds it there.
    Compressed(CompressedImage<'a>),
}

/// A device colour space, which is all an `/Indexed` base may be here.
///
/// 8.6.6.3 forbids an `/Indexed` whose base is itself `/Indexed`, and this
/// writer emits no CIE-based, `/Separation` or `/DeviceN` space — so the three
/// device families are the whole of it, and the restriction is the
/// specification's rather than an invention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceSpace {
    /// `/DeviceGray`.
    Gray,
    /// `/DeviceRGB`.
    Rgb,
    /// `/DeviceCMYK`.
    Cmyk,
}

impl DeviceSpace {
    /// Colour components per sample: 8.6.4's own counts.
    #[must_use]
    pub const fn components(self) -> u32 {
        match self {
            DeviceSpace::Gray => 1,
            DeviceSpace::Rgb => 3,
            DeviceSpace::Cmyk => 4,
        }
    }

    const fn pdf_name(self) -> &'static [u8] {
        match self {
            DeviceSpace::Gray => b"DeviceGray",
            DeviceSpace::Rgb => b"DeviceRGB",
            DeviceSpace::Cmyk => b"DeviceCMYK",
        }
    }
}

/// The `/ColorSpace` of a [`CompressedImage`] — the set this writer can emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageColorSpace<'a> {
    /// `/DeviceGray`.
    DeviceGray,
    /// `/DeviceRGB`.
    DeviceRgb,
    /// `/DeviceCMYK`.
    DeviceCmyk,
    /// `[/Indexed base hival lookup]` (8.6.6.3).
    Indexed {
        /// The space each table entry is expressed in.
        base: DeviceSpace,
        /// The table, `base.components()` bytes an entry, from index zero.
        ///
        /// `/hival` is **derived from this length** rather than carried
        /// separately. A `/hival` that disagrees with the table it describes is
        /// exactly how a reader ends up indexing past the end of one, and there
        /// is no legitimate document in which the two differ.
        lookup: &'a [u8],
    },
}

impl ImageColorSpace<'_> {
    /// Components in one *sample* of this space.
    ///
    /// An `/Indexed` sample is a single index whatever its base is (8.6.6.3),
    /// which is also what `/DecodeParms /Colors` must say for it.
    #[must_use]
    pub const fn components(&self) -> u32 {
        match self {
            ImageColorSpace::DeviceGray | ImageColorSpace::Indexed { .. } => 1,
            ImageColorSpace::DeviceRgb => 3,
            ImageColorSpace::DeviceCmyk => 4,
        }
    }
}

/// A filter **already applied** to the bytes handed over with it.
///
/// Not a request to encode: the `/Filter` and `/DecodeParms` this names are
/// written into the dictionary and the bytes are placed unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFilter {
    /// `/DCTDecode`, which carries no `/DecodeParms`.
    Dct,
    /// `/FlateDecode` over the samples directly, with no `/DecodeParms`.
    Flate,
    /// `/FlateDecode` with
    /// `/DecodeParms << /Predictor 15 /Colors c /BitsPerComponent b /Columns w >>`.
    ///
    /// 7.4.4.4's predictor 15 is "PNG optimum": every row carries its own
    /// filter tag, which is what PNG 9.2 writes and what makes a PNG's IDAT
    /// legible to a PDF reader without a byte being touched.
    ///
    /// The three parameters describe **the bytes**, not the image, which is why
    /// they are carried rather than derived — but a set that disagrees with the
    /// image's own geometry describes a different raster from the one the
    /// dictionary declares, so [`DocumentBuilder::add_image`] refuses it.
    FlatePngPredictor {
        /// `/Colors`: components per sample in the *encoded* data.
        colors: u32,
        /// `/BitsPerComponent`, as the predictor saw them.
        bits_per_component: u32,
        /// `/Columns`: samples per row.
        columns: u32,
    },
}

/// Per-sample opacity, as the `/DeviceGray` sub-image 11.6.5.3 asks for.
///
/// Its colour space is not a field: `/SMask` *is* `/DeviceGray`, and a mask in
/// any other space is not a mask.
pub struct SoftMask<'a> {
    /// Width in samples. Need not match the image's; a reader scales it.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// 1, 2, 4, 8 or 16.
    pub bits_per_component: u8,
    /// The filter already applied to `data`, or `None` for raw samples.
    pub filter: Option<ImageFilter>,
    /// The bytes, encoded as `filter` says.
    pub data: &'a [u8],
}

/// An image whose bytes are already encoded — see [`ImageData::Compressed`].
pub struct CompressedImage<'a> {
    /// Width in samples.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// 1, 2, 4, 8 or 16 (Table 89). An `/Indexed` image may not use 16,
    /// because 8.6.6.3 caps `/hival` at 255.
    pub bits_per_component: u8,
    /// The `/ColorSpace`.
    pub color_space: ImageColorSpace<'a>,
    /// The filter already applied to `data`, or `None` for raw samples — in
    /// which case the writer's ordinary compression applies.
    pub filter: Option<ImageFilter>,
    /// The bytes, encoded as `filter` says.
    pub data: &'a [u8],
    /// 8.9.6.4 colour-key masking: one inclusive `(min, max)` range per colour
    /// component, compared against **raw sample values**, before `/Decode`.
    ///
    /// A sample inside every one of its ranges is not painted. This is a range
    /// test rather than a lookup, which is precisely what a PNG `tRNS` naming
    /// one fully transparent colour or index needs and precisely what a `tRNS`
    /// giving partial alpha to a palette cannot use.
    pub color_key_mask: Option<&'a [(u32, u32)]>,
    /// 11.6.5.3 `/SMask`, written as its own image XObject.
    pub soft_mask: Option<SoftMask<'a>>,
}

/// Table 89: the five depths an image sample may have.
const fn is_legal_depth(bits: u8) -> bool {
    matches!(bits, 1 | 2 | 4 | 8 | 16)
}

/// The largest raw value a sample of this depth can hold.
const fn max_sample(bits: u8) -> u32 {
    // `bits` is one of Table 89's five, so the shift is at most 16.
    (1u32 << bits) - 1
}

/// Whether a filter's declared `/DecodeParms` describe the image carrying it.
///
/// A `/Columns` that is not the width unfilters every row at the wrong stride
/// and produces a picture that is scrambled rather than absent — the shape of
/// failure that reads as a decoder bug and gets found late. There is no
/// document in which these three legitimately differ from the image's own
/// geometry, so a disagreement is refused rather than written out.
fn filter_describes(filter: Option<ImageFilter>, components: u32, bits: u8, width: u32) -> bool {
    match filter {
        Some(ImageFilter::FlatePngPredictor {
            colors,
            bits_per_component,
            columns,
        }) => colors == components && bits_per_component == u32::from(bits) && columns == width,
        _ => true,
    }
}

/// A font whose program is embedded, held until `finish`.
///
/// The subset depends on what the document draws, which is not known when the
/// font is registered — so registration reserves object numbers and nothing
/// else.
struct Embedded {
    resource: Vec<u8>,
    base_font: Vec<u8>,
    program: Vec<u8>,
    file_ref: ObjRef,
    descriptor_ref: ObjRef,
    font_ref: ObjRef,
}

/// The six-letter tag and plus sign a subset font name carries (9.6.4).
///
/// Derived from the subset's own bytes rather than from a counter or a clock,
/// so the same document written twice produces the same name (ruling 4). Two
/// different subsets of the same face collide only if their bytes hash the
/// same, and a collision would merely give two fonts the same name, which is
/// legal.
fn subset_tag(program: &[u8]) -> Vec<u8> {
    // FNV-1a, for no reason beyond being short and well spread.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in program {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let mut tag = Vec::with_capacity(7);
    for _ in 0..6 {
        tag.push(b'A' + (hash % 26) as u8);
        hash /= 26;
    }
    tag.push(b'+');
    tag
}

/// A page being assembled.
pub struct PageBuilder {
    width: f64,
    height: f64,
    content: Vec<u8>,
    fonts: Vec<(Vec<u8>, ObjRef)>,
    images: Vec<(Vec<u8>, ObjRef)>,
    /// Characters drawn with each font resource, for subsetting.
    used: BTreeMap<Vec<u8>, BTreeSet<char>>,
}

impl PageBuilder {
    /// Writes text at a position, in points from the bottom-left.
    ///
    /// `font` names a font registered with [`DocumentBuilder::add_base_font`].
    pub fn text(&mut self, font: &[u8], size: f64, x: f64, y: f64, text: &str) {
        // Recorded so `finish` can subset an embedded font to what the
        // document actually draws. The builder is the only way content gets
        // written, so this sees everything.
        self.used
            .entry(font.to_vec())
            .or_default()
            .extend(text.chars());

        self.content.extend_from_slice(b"BT /");
        self.content.extend_from_slice(font);
        self.content
            .extend_from_slice(format!(" {size} Tf {x} {y} Td (").as_bytes());

        // 7.3.4.2: the three characters that must be escaped in a literal.
        for byte in text.bytes() {
            if matches!(byte, b'(' | b')' | b'\\') {
                self.content.push(b'\\');
            }
            self.content.push(byte);
        }
        self.content.extend_from_slice(b") Tj ET\n");
    }

    /// Fills a rectangle in device grey, from black (0) to white (1).
    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, grey: f64) {
        let grey = grey.clamp(0.0, 1.0);
        self.content
            .extend_from_slice(format!("{grey} g {x} {y} {w} {h} re f\n").as_bytes());
    }

    /// Draws a registered image into the given rectangle.
    ///
    /// 8.9.5.2: an image occupies the unit square, so placing it is entirely
    /// a matter of the transform — which is what this writes.
    pub fn image(&mut self, resource: &[u8], x: f64, y: f64, w: f64, h: f64) {
        self.content
            .extend_from_slice(format!("q {w} 0 0 {h} {x} {y} cm /").as_bytes());
        self.content.extend_from_slice(resource);
        self.content.extend_from_slice(b" Do Q\n");
    }

    /// Sets the non-stroking colour, as red, green and blue from zero to one.
    pub fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) {
        let c = |v: f64| v.clamp(0.0, 1.0);
        self.content
            .extend_from_slice(format!("{} {} {} rg\n", c(r), c(g), c(b)).as_bytes());
    }

    /// Appends raw content-stream operators.
    ///
    /// An escape hatch for callers that know the operator set; nothing checks
    /// what goes in, so a malformed sequence produces a malformed page.
    pub fn raw(&mut self, operators: &[u8]) {
        self.content.extend_from_slice(operators);
        self.content.push(b'\n');
    }
}

/// Reads a JPEG's dimensions and component count from its frame header.
fn jpeg_shape(data: &[u8]) -> Option<(u32, u32, u8)> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2usize;
    while at + 3 < data.len() {
        if data.get(at) != Some(&0xFF) {
            at += 1;
            continue;
        }
        let marker = *data.get(at + 1)?;
        at += 2;
        // Standalone markers carry no length.
        if matches!(marker, 0x01 | 0xD0..=0xD9 | 0xFF) {
            continue;
        }
        let length = data
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))?;

        // Any SOF marker; 0xC4, 0xC8 and 0xCC are tables, not frames.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = data.get(at + 3..at + 5)?;
            let w = data.get(at + 5..at + 7)?;
            let components = *data.get(at + 7)?;
            return Some((
                u32::from(u16::from_be_bytes([w[0], w[1]])),
                u32::from(u16::from_be_bytes([h[0], h[1]])),
                components,
            ));
        }
        at += length.max(2);
    }
    None
}

/// One outline entry to write.
pub struct OutlineEntry {
    /// The visible text.
    pub title: String,
    /// Zero-based page index the entry goes to.
    pub page: u32,
    /// Nested entries.
    pub children: Vec<OutlineEntry>,
}

/// Assembles a document.
pub struct DocumentBuilder {
    names: NameTable,
    objects: ObjectSet,
    next: u32,
    pages: Vec<PageBuilder>,
    fonts: Vec<(Vec<u8>, ObjRef)>,
    images: Vec<(Vec<u8>, ObjRef)>,
    /// Fonts whose programs are embedded, written at `finish` once the
    /// characters they are asked to draw are known.
    embedded: Vec<Embedded>,
    subset_fonts: bool,
    info: Dict,
    outline: Vec<OutlineEntry>,
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentBuilder {
    /// An empty document.
    #[must_use]
    pub fn new() -> DocumentBuilder {
        DocumentBuilder {
            names: NameTable::new(),
            objects: ObjectSet::new(),
            // 1 and 2 are reserved for the catalog and the page tree.
            next: 3,
            pages: Vec::new(),
            fonts: Vec::new(),
            images: Vec::new(),
            embedded: Vec::new(),
            subset_fonts: true,
            info: Dict::new(),
            outline: Vec::new(),
        }
    }

    fn allocate(&mut self) -> ObjRef {
        let r = ObjRef::new(self.next, 0);
        self.next = self.next.saturating_add(1);
        r
    }

    /// Registers one of the standard 14 fonts under a resource name.
    ///
    /// A standard font needs no `/Widths` and no embedded program, which is
    /// what makes it the right choice for a fixture: the reader supplies the
    /// metrics.
    pub fn add_base_font(&mut self, resource: &[u8], base_font: &[u8]) {
        let r = self.allocate();
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Font")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"Type1")),
        );
        dict.insert(
            self.names.intern(b"BaseFont"),
            Object::Name(self.names.intern(base_font)),
        );
        dict.insert(
            self.names.intern(b"Encoding"),
            Object::Name(self.names.intern(b"WinAnsiEncoding")),
        );
        self.objects.insert(r.num, Object::Dict(dict));
        self.fonts.push((resource.to_vec(), r));
    }

    /// Registers a TrueType font, embedding its program (9.6.6, 9.9).
    ///
    /// A document using only the standard 14 relies on the reader having them;
    /// one that embeds its font carries everything it needs, which is the
    /// difference between a file that renders the same everywhere and one that
    /// renders the same where the fonts happen to match.
    ///
    /// Widths come from the font program's own `hmtx`, scaled into the
    /// thousandths PDF measures in, so they agree with the outlines a renderer
    /// will draw — a `/Widths` array that disagrees with the glyphs is how
    /// text ends up overlapping itself.
    ///
    /// Returns false when the bytes are not a TrueType program this can read,
    /// rather than writing a font dictionary pointing at nothing.
    ///
    /// The whole program is embedded. Subsetting needs a glyph-set analysis
    /// and a table rewriter, and shipping the entire face is correct — merely
    /// larger — where a broken subset is neither.
    pub fn add_embedded_font(&mut self, resource: &[u8], base_font: &[u8], program: &[u8]) -> bool {
        if tinker_pdf_font::Sfnt::parse(program).is_none() {
            return false;
        }

        // Object numbers are reserved now, so page resources can name the
        // font; nothing is written until `finish`, because the subset depends
        // on what the document ends up drawing and that is not known yet.
        let file_ref = self.allocate();
        let descriptor_ref = self.allocate();
        let font_ref = self.allocate();

        self.embedded.push(Embedded {
            resource: resource.to_vec(),
            base_font: base_font.to_vec(),
            program: program.to_vec(),
            file_ref,
            descriptor_ref,
            font_ref,
        });
        self.fonts.push((resource.to_vec(), font_ref));
        true
    }

    /// Whether embedded fonts are cut down to the glyphs the document draws.
    ///
    /// On by default: a whole CJK face to set a line of Latin text is tens of
    /// megabytes, and subsetting is what makes embedding practical at all.
    /// Turn it off for a file that will be edited afterwards by something
    /// needing the other glyphs, which is the one case where the full face
    /// earns its size.
    pub fn set_subset_fonts(&mut self, subset: bool) {
        self.subset_fonts = subset;
    }

    /// Writes the font dictionaries, cutting each program down to the
    /// characters the pages drew with it.
    fn write_embedded_fonts(&mut self, used: &BTreeMap<Vec<u8>, BTreeSet<char>>) {
        let embedded = std::mem::take(&mut self.embedded);
        let empty = BTreeSet::new();

        for font in embedded {
            let characters = used.get(&font.resource).unwrap_or(&empty);
            let text: String = characters.iter().copied().collect();

            // A subset that cannot be built is not a reason to fail the
            // document: the whole face is larger and correct, which is the
            // right way round (ruling 2).
            let (program, subsetted) = if self.subset_fonts {
                let glyphs = tinker_pdf_font::glyphs_for(&font.program, &text);
                if !text.is_empty() && glyphs.is_empty() {
                    // The document drew text with this font and the font
                    // claims none of it: a missing or unreadable `cmap`, most
                    // likely, or a symbolic font addressed some other way.
                    // Subsetting on that evidence would keep `.notdef` alone
                    // and every letter would come out blank — which reads as
                    // a rendering bug rather than a subsetting one, and so
                    // gets found far too late.
                    (font.program.clone(), false)
                } else {
                    match tinker_pdf_font::subset(&font.program, &glyphs) {
                        Some(reduced) => (reduced, true),
                        None => (font.program.clone(), false),
                    }
                }
            } else {
                (font.program.clone(), false)
            };

            // 9.6.4: a subset font name carries a six-letter tag and a plus
            // sign, which is how a reader knows the embedded program is not
            // the whole face.
            let base_font = if subsetted {
                let mut tagged = subset_tag(&program);
                tagged.extend_from_slice(&font.base_font);
                tagged
            } else {
                font.base_font.clone()
            };

            self.write_font_objects(&font, &program, &base_font);
        }
    }

    fn write_font_objects(&mut self, font: &Embedded, program: &[u8], base_font: &[u8]) {
        // Widths come from the *original* program. Subsetting never moves a
        // glyph identifier, so the `hmtx` entry is still the right one, and
        // reading them from the subset would be no different — but reading
        // them from the original says plainly that it does not depend on
        // which glyphs survived.
        let Some(sfnt) = tinker_pdf_font::Sfnt::parse(&font.program) else {
            return;
        };
        let units = f64::from(sfnt.units_per_em.max(1));

        // 9.6.6.4: /FirstChar../LastChar with one width each, in glyph space
        // thousandths. WinAnsi is assumed because that is what the encoding
        // below declares; a font used with another encoding needs the widths
        // that encoding implies.
        const FIRST: u8 = 32;
        const LAST: u8 = 255;
        let mut widths = Vec::with_capacity(usize::from(LAST - FIRST) + 1);
        for code in FIRST..=LAST {
            let width = sfnt
                .glyph_for_char(char::from(code))
                .and_then(|glyph| sfnt.advance(glyph))
                .map_or(500.0, |advance| f64::from(advance) * 1000.0 / units);
            widths.push(Object::Real(width.round()));
        }

        let mut file_dict = Dict::new();
        // 9.9: /Length1 is the embedded program's length — the subset's, not
        // the original's, since the subset is what the stream contains.
        file_dict.insert(
            self.names.intern(b"Length1"),
            Object::Int(program.len() as i64),
        );
        self.objects.insert_stream(
            font.file_ref.num,
            StreamData {
                dict: file_dict,
                data: program.to_vec(),
            },
        );

        let mut descriptor = Dict::new();
        descriptor.insert(
            Name::TYPE,
            Object::Name(self.names.intern(b"FontDescriptor")),
        );
        descriptor.insert(
            self.names.intern(b"FontName"),
            Object::Name(self.names.intern(base_font)),
        );
        // 9.8.2 Table 123: bit 6 marks a non-symbolic font, which is what an
        // explicit /Encoding requires; a symbolic one is read through its own
        // cmap and must not carry one.
        descriptor.insert(self.names.intern(b"Flags"), Object::Int(32));
        descriptor.insert(
            self.names.intern(b"FontBBox"),
            Object::Array(vec![
                Object::Int(-500),
                Object::Int(-300),
                Object::Int(1500),
                Object::Int(1000),
            ]),
        );
        descriptor.insert(self.names.intern(b"ItalicAngle"), Object::Int(0));
        descriptor.insert(self.names.intern(b"Ascent"), Object::Int(750));
        descriptor.insert(self.names.intern(b"Descent"), Object::Int(-250));
        descriptor.insert(self.names.intern(b"CapHeight"), Object::Int(700));
        descriptor.insert(self.names.intern(b"StemV"), Object::Int(80));
        descriptor.insert(self.names.intern(b"FontFile2"), Object::Ref(font.file_ref));
        self.objects
            .insert(font.descriptor_ref.num, Object::Dict(descriptor));

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Font")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"TrueType")),
        );
        dict.insert(
            self.names.intern(b"BaseFont"),
            Object::Name(self.names.intern(base_font)),
        );
        dict.insert(
            self.names.intern(b"Encoding"),
            Object::Name(self.names.intern(b"WinAnsiEncoding")),
        );
        dict.insert(
            self.names.intern(b"FirstChar"),
            Object::Int(i64::from(FIRST)),
        );
        dict.insert(self.names.intern(b"LastChar"), Object::Int(i64::from(LAST)));
        dict.insert(self.names.intern(b"Widths"), Object::Array(widths));
        dict.insert(
            self.names.intern(b"FontDescriptor"),
            Object::Ref(font.descriptor_ref),
        );

        self.objects.insert(font.font_ref.num, Object::Dict(dict));
    }

    /// Registers an image under a resource name.
    ///
    /// Returns false when the data does not describe an image of the size it
    /// claims, rather than writing a stream a reader would choke on.
    pub fn add_image(&mut self, resource: &[u8], image: &ImageData<'_>) -> bool {
        let r = self.allocate();
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"XObject")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"Image")),
        );

        let data = match image {
            ImageData::Jpeg(bytes) => {
                // The dimensions live in the JPEG's own SOF marker and must
                // agree with /Width and /Height, so they are read out of it
                // rather than taken on trust.
                let Some((width, height, components)) = jpeg_shape(bytes) else {
                    return false;
                };
                dict.insert(self.names.intern(b"Width"), Object::Int(i64::from(width)));
                dict.insert(self.names.intern(b"Height"), Object::Int(i64::from(height)));
                dict.insert(self.names.intern(b"BitsPerComponent"), Object::Int(8));
                dict.insert(
                    self.names.intern(b"ColorSpace"),
                    Object::Name(self.names.intern(match components {
                        1 => b"DeviceGray".as_slice(),
                        4 => b"DeviceCMYK",
                        _ => b"DeviceRGB",
                    })),
                );
                dict.insert(Name::FILTER, Object::Name(self.names.intern(b"DCTDecode")));
                bytes.to_vec()
            }
            ImageData::Rgb8 {
                width,
                height,
                data,
            }
            | ImageData::Gray8 {
                width,
                height,
                data,
            } => {
                let gray = matches!(image, ImageData::Gray8 { .. });
                let n = if gray { 1 } else { 3 };
                let expected = (*width as usize)
                    .saturating_mul(*height as usize)
                    .saturating_mul(n);
                if *width == 0 || *height == 0 || data.len() < expected {
                    return false;
                }
                dict.insert(self.names.intern(b"Width"), Object::Int(i64::from(*width)));
                dict.insert(
                    self.names.intern(b"Height"),
                    Object::Int(i64::from(*height)),
                );
                dict.insert(self.names.intern(b"BitsPerComponent"), Object::Int(8));
                dict.insert(
                    self.names.intern(b"ColorSpace"),
                    Object::Name(self.names.intern(if gray {
                        b"DeviceGray".as_slice()
                    } else {
                        b"DeviceRGB"
                    })),
                );
                data.get(..expected).unwrap_or(data).to_vec()
            }
            ImageData::Compressed(image) => {
                let Some(data) = self.compressed_image(&mut dict, image) else {
                    return false;
                };
                data
            }
        };

        self.objects.insert_stream(r.num, StreamData { dict, data });
        self.images.push((resource.to_vec(), r));
        true
    }

    /// Fills an image dictionary for bytes that are already encoded.
    ///
    /// Everything is checked **before** anything is written: a refused image
    /// that had already inserted its `/SMask` would leave an unreferenced
    /// stream in the file, and `add_image` returning false would stop being the
    /// whole story.
    fn compressed_image(
        &mut self,
        dict: &mut Dict,
        image: &CompressedImage<'_>,
    ) -> Option<Vec<u8>> {
        if image.width == 0 || image.height == 0 || image.data.is_empty() {
            return None;
        }
        if !is_legal_depth(image.bits_per_component) {
            return None;
        }
        let components = image.color_space.components();
        if !filter_describes(
            image.filter,
            components,
            image.bits_per_component,
            image.width,
        ) {
            return None;
        }

        // 8.6.6.3: `/hival` is at most 255, and every index the samples can
        // name has to be one the table holds — so a 4-bit indexed image may
        // carry sixteen entries and a 1-bit one may carry two.
        let mut entries = 0usize;
        if let ImageColorSpace::Indexed { base, lookup } = image.color_space {
            let per = base.components() as usize;
            if lookup.is_empty() || lookup.len() % per != 0 {
                return None;
            }
            entries = lookup.len() / per;
            if image.bits_per_component > 8 || entries > 1usize << image.bits_per_component {
                return None;
            }
        }

        if let Some(ranges) = image.color_key_mask {
            // 8.9.6.4: 2 x n integers, "each in the range 0 to
            // 2^BitsPerComponent - 1", and a min above its max names an empty
            // range that would mask nothing while looking as though it did.
            if ranges.len() != components as usize {
                return None;
            }
            let ceiling = max_sample(image.bits_per_component);
            if ranges.iter().any(|&(lo, hi)| lo > hi || hi > ceiling) {
                return None;
            }
        }

        if let Some(mask) = &image.soft_mask {
            if mask.width == 0 || mask.height == 0 || mask.data.is_empty() {
                return None;
            }
            if !is_legal_depth(mask.bits_per_component) {
                return None;
            }
            // A soft mask is one-component `/DeviceGray`, so its own predictor
            // parameters are checked against that rather than against the
            // image's.
            if !filter_describes(mask.filter, 1, mask.bits_per_component, mask.width) {
                return None;
            }
        }

        dict.insert(
            self.names.intern(b"Width"),
            Object::Int(i64::from(image.width)),
        );
        dict.insert(
            self.names.intern(b"Height"),
            Object::Int(i64::from(image.height)),
        );
        dict.insert(
            self.names.intern(b"BitsPerComponent"),
            Object::Int(i64::from(image.bits_per_component)),
        );
        let space = match image.color_space {
            ImageColorSpace::DeviceGray => Object::Name(self.names.intern(b"DeviceGray")),
            ImageColorSpace::DeviceRgb => Object::Name(self.names.intern(b"DeviceRGB")),
            ImageColorSpace::DeviceCmyk => Object::Name(self.names.intern(b"DeviceCMYK")),
            ImageColorSpace::Indexed { base, lookup } => Object::Array(vec![
                Object::Name(self.names.intern(b"Indexed")),
                Object::Name(self.names.intern(base.pdf_name())),
                // Checked above to be at least one, so this cannot wrap.
                Object::Int(entries as i64 - 1),
                // A hex string rather than a literal: a palette is arbitrary
                // bytes, and hex needs no escaping decisions at all.
                Object::String(PdfString::hex(lookup.to_vec())),
            ]),
        };
        dict.insert(self.names.intern(b"ColorSpace"), space);
        if let Some(filter) = image.filter {
            self.insert_filter(dict, filter);
        }
        if let Some(ranges) = image.color_key_mask {
            dict.insert(
                self.names.intern(b"Mask"),
                Object::Array(
                    ranges
                        .iter()
                        .flat_map(|&(lo, hi)| {
                            [Object::Int(i64::from(lo)), Object::Int(i64::from(hi))]
                        })
                        .collect(),
                ),
            );
        }
        if let Some(mask) = &image.soft_mask {
            let reference = self.allocate();
            let mut md = Dict::new();
            md.insert(Name::TYPE, Object::Name(self.names.intern(b"XObject")));
            md.insert(
                self.names.intern(b"Subtype"),
                Object::Name(self.names.intern(b"Image")),
            );
            md.insert(
                self.names.intern(b"Width"),
                Object::Int(i64::from(mask.width)),
            );
            md.insert(
                self.names.intern(b"Height"),
                Object::Int(i64::from(mask.height)),
            );
            md.insert(
                self.names.intern(b"BitsPerComponent"),
                Object::Int(i64::from(mask.bits_per_component)),
            );
            md.insert(
                self.names.intern(b"ColorSpace"),
                Object::Name(self.names.intern(b"DeviceGray")),
            );
            if let Some(filter) = mask.filter {
                self.insert_filter(&mut md, filter);
            }
            self.objects.insert_stream(
                reference.num,
                StreamData {
                    dict: md,
                    data: mask.data.to_vec(),
                },
            );
            dict.insert(self.names.intern(b"SMask"), Object::Ref(reference));
        }

        Some(image.data.to_vec())
    }

    /// Writes `/Filter` and, where the filter has any, `/DecodeParms`.
    fn insert_filter(&mut self, dict: &mut Dict, filter: ImageFilter) {
        let name = match filter {
            ImageFilter::Dct => b"DCTDecode".as_slice(),
            _ => b"FlateDecode",
        };
        dict.insert(Name::FILTER, Object::Name(self.names.intern(name)));

        if let ImageFilter::FlatePngPredictor {
            colors,
            bits_per_component,
            columns,
        } = filter
        {
            let mut parms = Dict::new();
            // 7.4.4.4 Table 10, in the table's own order.
            parms.insert(self.names.intern(b"Predictor"), Object::Int(15));
            parms.insert(self.names.intern(b"Colors"), Object::Int(i64::from(colors)));
            parms.insert(
                self.names.intern(b"BitsPerComponent"),
                Object::Int(i64::from(bits_per_component)),
            );
            parms.insert(
                self.names.intern(b"Columns"),
                Object::Int(i64::from(columns)),
            );
            dict.insert(Name::DECODE_PARMS, Object::Dict(parms));
        }
    }

    /// Adds a page, drawing it with the given closure.
    ///
    /// A closure rather than a returned reference so the API stays infallible:
    /// there is no borrow to fumble and no case where "the page just pushed"
    /// has to be recovered from an `Option`.
    pub fn add_page(&mut self, width: f64, height: f64, draw: impl FnOnce(&mut PageBuilder)) {
        let mut page = PageBuilder {
            width,
            height,
            content: Vec::new(),
            fonts: self.fonts.clone(),
            images: self.images.clone(),
            used: BTreeMap::new(),
        };
        draw(&mut page);
        self.pages.push(page);
    }

    /// Sets an `/Info` field.
    pub fn set_info(&mut self, key: &[u8], value: &str) {
        let name = self.names.intern(key);
        self.info.insert(
            name,
            Object::String(PdfString::literal(value.as_bytes().to_vec())),
        );
    }

    /// Sets the outline.
    pub fn set_outline(&mut self, entries: Vec<OutlineEntry>) {
        self.outline = entries;
    }

    /// Serializes the document.
    #[must_use]
    pub fn finish(mut self) -> Vec<u8> {
        let page_refs: Vec<ObjRef> = (0..self.pages.len()).map(|_| self.allocate()).collect();
        let pages_ref = ObjRef::new(2, 0);

        let pages = std::mem::take(&mut self.pages);

        // Every character every page drew, per font resource. Gathered before
        // anything is written because the embedded programs are subset to it.
        let mut used: BTreeMap<Vec<u8>, BTreeSet<char>> = BTreeMap::new();
        for page in &pages {
            for (resource, characters) in &page.used {
                used.entry(resource.clone())
                    .or_default()
                    .extend(characters.iter().copied());
            }
        }
        self.write_embedded_fonts(&used);

        for (page, reference) in pages.iter().zip(page_refs.iter()) {
            let content_ref = self.allocate();
            self.objects.insert_stream(
                content_ref.num,
                StreamData {
                    dict: Dict::new(),
                    data: page.content.clone(),
                },
            );

            let mut font_dict = Dict::new();
            for (resource, font_ref) in &page.fonts {
                let name = self.names.intern(resource);
                font_dict.insert(name, Object::Ref(*font_ref));
            }
            let mut xobjects = Dict::new();
            for (resource, image_ref) in &page.images {
                let name = self.names.intern(resource);
                xobjects.insert(name, Object::Ref(*image_ref));
            }

            let mut resources = Dict::new();
            if !font_dict.is_empty() {
                resources.insert(self.names.intern(b"Font"), Object::Dict(font_dict));
            }
            if !xobjects.is_empty() {
                resources.insert(self.names.intern(b"XObject"), Object::Dict(xobjects));
            }

            let mut dict = Dict::new();
            dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Page")));
            dict.insert(Name::PARENT, Object::Ref(pages_ref));
            dict.insert(
                Name::MEDIA_BOX,
                Object::Array(vec![
                    Object::Int(0),
                    Object::Int(0),
                    Object::Real(page.width),
                    Object::Real(page.height),
                ]),
            );
            dict.insert(Name::RESOURCES, Object::Dict(resources));
            dict.insert(Name::CONTENTS, Object::Ref(content_ref));
            self.objects.insert(reference.num, Object::Dict(dict));
        }

        // The page tree.
        let mut tree = Dict::new();
        tree.insert(Name::TYPE, Object::Name(self.names.intern(b"Pages")));
        tree.insert(
            Name::KIDS,
            Object::Array(page_refs.iter().map(|r| Object::Ref(*r)).collect()),
        );
        tree.insert(Name::COUNT, Object::Int(page_refs.len() as i64));
        self.objects.insert(2, Object::Dict(tree));

        // The catalog, and the outline if there is one.
        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(self.names.intern(b"Catalog")));
        catalog.insert(Name::PAGES, Object::Ref(pages_ref));

        let outline = std::mem::take(&mut self.outline);
        if !outline.is_empty() {
            let root = self.allocate();
            let children = self.write_outline(&outline, &page_refs, root);
            let mut dict = Dict::new();
            dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Outlines")));
            if let Some((first, last, count)) = children {
                dict.insert(self.names.intern(b"First"), Object::Ref(first));
                dict.insert(self.names.intern(b"Last"), Object::Ref(last));
                dict.insert(Name::COUNT, Object::Int(count));
            }
            self.objects.insert(root.num, Object::Dict(dict));
            catalog.insert(self.names.intern(b"Outlines"), Object::Ref(root));
        }
        self.objects.insert(1, Object::Dict(catalog));

        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));
        if !self.info.is_empty() {
            let info_ref = self.allocate();
            let info = std::mem::take(&mut self.info);
            self.objects.insert(info_ref.num, Object::Dict(info));
            trailer.insert(Name::INFO, Object::Ref(info_ref));
        }

        rewrite(
            &self.objects,
            &trailer,
            &WriteOptions::default(),
            &self.names,
        )
    }

    /// Writes one level of outline entries, returning `(first, last, count)`.
    fn write_outline(
        &mut self,
        entries: &[OutlineEntry],
        pages: &[ObjRef],
        parent: ObjRef,
    ) -> Option<(ObjRef, ObjRef, i64)> {
        if entries.is_empty() {
            return None;
        }

        let refs: Vec<ObjRef> = entries.iter().map(|_| self.allocate()).collect();
        let mut total = refs.len() as i64;

        for (index, entry) in entries.iter().enumerate() {
            let Some(&reference) = refs.get(index) else {
                continue;
            };
            let children = self.write_outline(&entry.children, pages, reference);

            let mut dict = Dict::new();
            dict.insert(
                self.names.intern(b"Title"),
                Object::String(PdfString::literal(entry.title.as_bytes().to_vec())),
            );
            dict.insert(Name::PARENT, Object::Ref(parent));

            // Ruling 6: an explicit destination, never a name that looks like
            // one. This is the writer side of the defect that made the engine
            // being replaced turn "#page=2" into a dead named destination.
            if let Some(&page) = pages.get(entry.page as usize) {
                dict.insert(
                    self.names.intern(b"Dest"),
                    Object::Array(vec![
                        Object::Ref(page),
                        Object::Name(self.names.intern(b"Fit")),
                    ]),
                );
            }

            if let Some(&previous) = index.checked_sub(1).and_then(|i| refs.get(i)) {
                dict.insert(self.names.intern(b"Prev"), Object::Ref(previous));
            }
            if let Some(&next) = refs.get(index + 1) {
                dict.insert(self.names.intern(b"Next"), Object::Ref(next));
            }
            if let Some((first, last, count)) = children {
                dict.insert(self.names.intern(b"First"), Object::Ref(first));
                dict.insert(self.names.intern(b"Last"), Object::Ref(last));
                // A positive count means the entry is open when the document
                // is opened (12.3.3).
                dict.insert(Name::COUNT, Object::Int(count));
                total += count;
            }

            self.objects.insert(reference.num, Object::Dict(dict));
        }

        match (refs.first(), refs.last()) {
            (Some(&first), Some(&last)) => Some((first, last, total)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CosDocument;

    #[test]
    fn a_built_document_opens_and_reports_its_pages() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for i in 1..=3 {
            builder.add_page(595.0, 842.0, |page| {
                page.text(b"F0", 18.0, 72.0, 742.0, &format!("Built page {i} of 3"));
            });
        }
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("the built document opens");
        assert_eq!(crate::pages::count(&doc), 3);
        assert_eq!(
            doc.ladder_level(),
            crate::LadderLevel::Trust,
            "our own output should need no repair"
        );
        assert!(
            doc.warnings().is_empty(),
            "nor provoke any leniency: {:?}",
            doc.warnings()
        );
    }

    #[test]
    fn built_pages_carry_their_geometry_and_content() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "Hello");
        });
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let pages = crate::pages::collect(&doc);
        let first = pages.first().expect("a page");

        assert_eq!(first.media_box.width(), 200.0);
        assert_eq!(first.media_box.height(), 100.0);

        let content = crate::pages::content_bytes(&doc, first);
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("BT"), "text operators: {text}");
        assert!(text.contains("(Hello)"), "the string: {text}");
    }

    #[test]
    fn strings_with_parentheses_survive_the_round_trip() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(300.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, r"a (nested) string\here");
        });
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let pages = crate::pages::collect(&doc);
        let content = crate::pages::content_bytes(&doc, pages.first().expect("a page"));
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains(r"\(nested\)"), "escaped: {text}");
    }

    /// Ruling 6 on the writer side: an outline destination round-trips as an
    /// explicit one.
    #[test]
    fn a_built_outline_round_trips_with_explicit_destinations() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for _ in 0..6 {
            builder.add_page(595.0, 842.0, |_| {});
        }
        builder.set_outline(vec![
            OutlineEntry {
                title: "Part One".to_string(),
                page: 0,
                children: vec![OutlineEntry {
                    title: "Chapter 1".to_string(),
                    page: 1,
                    children: vec![OutlineEntry {
                        title: "Section 1.1".to_string(),
                        page: 2,
                        children: Vec::new(),
                    }],
                }],
            },
            OutlineEntry {
                title: "Part Two".to_string(),
                page: 4,
                children: Vec::new(),
            },
        ]);
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let items = crate::outline::outline(&doc);
        let flat = crate::OutlineItem::flatten(&items);

        let seen: Vec<(u32, String, Option<u32>)> = flat
            .iter()
            .map(|(depth, item)| {
                let page = match &item.destination {
                    Some(crate::Destination::Explicit { page_index, .. }) => *page_index,
                    other => panic!("expected an explicit destination, got {other:?}"),
                };
                (*depth, item.title.clone(), page)
            })
            .collect();

        // The same shape the MuPDF-generated fixture has.
        for want in [
            (0u32, "Part One", Some(0u32)),
            (1, "Chapter 1", Some(1)),
            (2, "Section 1.1", Some(2)),
            (0, "Part Two", Some(4)),
        ] {
            assert!(
                seen.iter()
                    .any(|(d, t, p)| *d == want.0 && t == want.1 && *p == want.2),
                "expected {want:?}; got {seen:#?}"
            );
        }
    }

    #[test]
    fn metadata_round_trips() {
        let mut builder = DocumentBuilder::new();
        builder.add_page(100.0, 100.0, |_| {});
        builder.set_info(b"Title", "A Built Document");
        builder.set_info(b"Producer", "tinker-pdf");
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let meta = crate::outline::metadata(&doc);
        assert_eq!(meta.title.as_deref(), Some("A Built Document"));
        assert_eq!(meta.producer.as_deref(), Some("tinker-pdf"));
    }

    #[test]
    fn an_empty_document_is_still_a_document() {
        let bytes = DocumentBuilder::new().finish();
        let doc = CosDocument::open(bytes).expect("even an empty one opens");
        assert_eq!(crate::pages::count(&doc), 0);
        assert!(doc.catalog().is_some());
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::CosDocument;

    #[test]
    fn a_raw_image_round_trips_through_the_reader() {
        let mut builder = DocumentBuilder::new();
        // Two by two: red, green, blue, white.
        let pixels = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 2,
                height: 2,
                data: &pixels,
            }
        ));
        builder.add_page(100.0, 100.0, |page| {
            page.image(b"Im0", 10.0, 10.0, 50.0, 50.0);
        });

        let doc = CosDocument::open(builder.finish()).expect("it opens");
        let pages = crate::pages::collect(&doc);
        let content = crate::pages::content_bytes(&doc, pages.first().expect("a page"));
        let text = String::from_utf8_lossy(&content);

        assert!(text.contains("/Im0 Do"), "the image is drawn: {text}");
        assert!(text.contains("50 0 0 50 10 10 cm"), "and placed: {text}");
    }

    #[test]
    fn an_images_samples_survive_the_round_trip() {
        let mut builder = DocumentBuilder::new();
        let pixels = [1u8, 2, 3, 4, 5, 6];
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 2,
                height: 1,
                data: &pixels,
            }
        ));
        builder.add_page(10.0, 10.0, |_| {});

        let doc = CosDocument::open(builder.finish()).expect("it opens");
        // The image is the only stream with /Subtype /Image.
        let subtype = doc.intern(b"Subtype");
        let found = (1..20u32).find_map(|num| {
            let r = ObjRef::new(num, 0);
            let object = doc.get(r).ok()?;
            let dict = object.as_dict()?;
            let is_image = dict
                .get(subtype)
                .and_then(Object::as_name)
                .and_then(|n| doc.name_bytes(n))
                .is_some_and(|b| b.as_ref() == b"Image");
            is_image.then_some(r)
        });

        let reference = found.expect("an image object");
        assert_eq!(
            doc.stream_decoded(reference).ok(),
            Some(pixels.to_vec()),
            "the samples come back unchanged"
        );
    }

    #[test]
    fn a_jpeg_is_embedded_without_being_re_encoded() {
        // A minimal JPEG header declaring 4x3, three components.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        jpeg.extend_from_slice(&3u16.to_be_bytes()); // height
        jpeg.extend_from_slice(&4u16.to_be_bytes()); // width
        jpeg.push(3); // components
        jpeg.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);

        let mut builder = DocumentBuilder::new();
        assert!(builder.add_image(b"Im0", &ImageData::Jpeg(&jpeg)));
        builder.add_page(10.0, 10.0, |_| {});

        let bytes = builder.finish();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/DCTDecode"), "kept as JPEG");
        assert!(
            text.contains("/Width 4") && text.contains("/Height 3"),
            "dimensions read from the frame header, not guessed"
        );

        // And the exact bytes are still in the file.
        assert!(
            bytes.windows(jpeg.len()).any(|w| w == jpeg),
            "the JPEG data is embedded byte for byte"
        );
    }

    /// A compressed image with nothing wrong with it, for the refusals below to
    /// be measured against.
    fn sound() -> CompressedImage<'static> {
        CompressedImage {
            width: 4,
            height: 2,
            bits_per_component: 8,
            color_space: ImageColorSpace::DeviceRgb,
            filter: Some(ImageFilter::FlatePngPredictor {
                colors: 3,
                bits_per_component: 8,
                columns: 4,
            }),
            data: b"encoded",
            color_key_mask: None,
            soft_mask: None,
        }
    }

    /// Every way a pre-compressed image can fail to describe an image, refused
    /// rather than written out as a dictionary a reader will choke on.
    ///
    /// The control comes first on purpose: a `compressed_image` that returned
    /// `None` unconditionally would satisfy every other line here.
    #[test]
    fn a_compressed_image_that_does_not_describe_an_image_is_refused() {
        let mut builder = DocumentBuilder::new();
        assert!(
            builder.add_image(b"Ok", &ImageData::Compressed(sound())),
            "the control is accepted"
        );

        const LOOKUP: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let cases: Vec<(&str, CompressedImage<'_>)> = vec![
            (
                "no width",
                CompressedImage {
                    width: 0,
                    ..sound()
                },
            ),
            (
                "no height",
                CompressedImage {
                    height: 0,
                    ..sound()
                },
            ),
            (
                "no data",
                CompressedImage {
                    data: b"",
                    ..sound()
                },
            ),
            // Table 89 admits 1, 2, 4, 8 and 16 and nothing else. A depth of 3
            // would be read as 3 by the sample loop and produce a row stride
            // nothing in the file agrees with.
            (
                "a depth outside Table 89",
                CompressedImage {
                    bits_per_component: 3,
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 3,
                        bits_per_component: 3,
                        columns: 4,
                    }),
                    ..sound()
                },
            ),
            (
                "a depth of zero",
                CompressedImage {
                    bits_per_component: 0,
                    filter: None,
                    ..sound()
                },
            ),
            // 7.4.4.4: the three parameters describe the bytes, so a set that
            // disagrees with the image's own geometry unfilters at a stride the
            // data never had.
            (
                "/Columns that is not the width",
                CompressedImage {
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 3,
                        bits_per_component: 8,
                        columns: 3,
                    }),
                    ..sound()
                },
            ),
            (
                "/Colors that is not the component count",
                CompressedImage {
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 1,
                        bits_per_component: 8,
                        columns: 4,
                    }),
                    ..sound()
                },
            ),
            (
                "/BitsPerComponent that is not the depth",
                CompressedImage {
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 3,
                        bits_per_component: 4,
                        columns: 4,
                    }),
                    ..sound()
                },
            ),
            // 8.6.6.3.
            (
                "an empty palette",
                CompressedImage {
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: b"",
                    },
                    filter: None,
                    ..sound()
                },
            ),
            (
                "a palette that is not whole entries",
                CompressedImage {
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: &LOOKUP[..8],
                    },
                    filter: None,
                    ..sound()
                },
            ),
            (
                "a palette larger than the depth can index",
                CompressedImage {
                    bits_per_component: 1,
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: &LOOKUP,
                    },
                    filter: None,
                    ..sound()
                },
            ),
            (
                "an indexed image at sixteen bits",
                CompressedImage {
                    bits_per_component: 16,
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: &LOOKUP,
                    },
                    filter: None,
                    ..sound()
                },
            ),
            // 8.9.6.4.
            (
                "a colour key of the wrong length",
                CompressedImage {
                    color_key_mask: Some(&[(0, 0)]),
                    ..sound()
                },
            ),
            (
                "a colour key past the depth's range",
                CompressedImage {
                    color_key_mask: Some(&[(0, 0), (0, 0), (0, 256)]),
                    ..sound()
                },
            ),
            (
                "a colour key whose minimum is above its maximum",
                CompressedImage {
                    color_key_mask: Some(&[(0, 0), (0, 0), (9, 8)]),
                    ..sound()
                },
            ),
            // 11.6.5.3.
            (
                "a soft mask with no width",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 0,
                        height: 2,
                        bits_per_component: 8,
                        filter: None,
                        data: b"xx",
                    }),
                    ..sound()
                },
            ),
            (
                "a soft mask with no data",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 4,
                        height: 2,
                        bits_per_component: 8,
                        filter: None,
                        data: b"",
                    }),
                    ..sound()
                },
            ),
            (
                "a soft mask at a depth outside Table 89",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 4,
                        height: 2,
                        bits_per_component: 7,
                        filter: None,
                        data: b"xx",
                    }),
                    ..sound()
                },
            ),
            (
                "a soft mask whose predictor claims three colours",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 4,
                        height: 2,
                        bits_per_component: 8,
                        filter: Some(ImageFilter::FlatePngPredictor {
                            colors: 3,
                            bits_per_component: 8,
                            columns: 4,
                        }),
                        data: b"xx",
                    }),
                    ..sound()
                },
            ),
        ];

        for (what, image) in cases {
            assert!(
                !builder.add_image(b"Im", &ImageData::Compressed(image)),
                "{what} was accepted"
            );
        }
    }

    /// A refusal writes nothing at all — not even the part that was checked
    /// before the part that failed.
    ///
    /// The soft mask is the last thing validated and the only thing that gets
    /// an object of its own, so it is the one place an ordering mistake would
    /// leave an unreferenced stream behind in every file that hit it.
    #[test]
    fn a_refused_image_leaves_no_orphan_stream_behind() {
        let mut builder = DocumentBuilder::new();
        assert!(!builder.add_image(
            b"Im0",
            &ImageData::Compressed(CompressedImage {
                // Sound in every respect except its mask.
                soft_mask: Some(SoftMask {
                    width: 4,
                    height: 0,
                    bits_per_component: 8,
                    filter: None,
                    data: b"the mask that must not be written",
                }),
                ..sound()
            })
        ));
        builder.add_page(10.0, 10.0, |_| {});

        let bytes = builder.finish();
        assert!(
            !bytes
                .windows(33)
                .any(|w| w == b"the mask that must not be written"),
            "the mask was written despite the image being refused"
        );
        assert!(
            !String::from_utf8_lossy(&bytes).contains("/Subtype /Image"),
            "and no image XObject exists at all"
        );
    }

    /// A pre-compressed image with **no** filter is placed as raw samples and
    /// declares none, which is the other half of the `maybe_compress` contract:
    /// the rule is "a `/Filter` key means hands off", not "this variant means
    /// hands off".
    ///
    /// It also records something a reader of this variant needs to know.
    /// `WriteOptions::default()` has `compress: false`, and `finish` uses the
    /// default — so the writer will *not* deflate these bytes on the way out.
    /// Anything handing over a raster has to compress it itself or the samples
    /// land in the file at full size, which is `png_embed`'s reason for
    /// re-deflating the decoded route rather than leaving it to the writer.
    #[test]
    fn a_compressed_image_with_no_filter_is_placed_as_raw_samples() {
        let samples: Vec<u8> = (0..3 * 64 * 64).map(|i| (i % 251) as u8).collect();
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Compressed(CompressedImage {
                width: 64,
                height: 64,
                bits_per_component: 8,
                color_space: ImageColorSpace::DeviceRgb,
                filter: None,
                data: &samples,
                color_key_mask: None,
                soft_mask: None,
            })
        ));
        builder.add_page(64.0, 64.0, |page| page.image(b"Im0", 0.0, 0.0, 64.0, 64.0));

        let bytes = builder.finish();
        assert!(
            bytes.windows(samples.len()).any(|w| w == samples),
            "the samples are in the file verbatim"
        );
        let doc = CosDocument::open(bytes).expect("it opens");
        let found = (1..20u32)
            .map(|num| ObjRef::new(num, 0))
            .find(|r| doc.stream_decoded(*r).is_ok_and(|d| d == samples))
            .expect("the image object");
        let dict = doc
            .get(found)
            .ok()
            .and_then(|o| o.as_dict().cloned())
            .expect("a stream dictionary");
        assert!(
            !dict.contains_key(Name::FILTER),
            "and no filter is claimed for them"
        );
    }

    #[test]
    fn an_image_that_does_not_match_its_size_is_refused() {
        let mut builder = DocumentBuilder::new();
        // Three bytes cannot be a 2x2 RGB image.
        assert!(!builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 2,
                height: 2,
                data: &[1, 2, 3],
            }
        ));
        assert!(!builder.add_image(b"Im1", &ImageData::Jpeg(b"not a jpeg")));
        assert!(!builder.add_image(
            b"Im2",
            &ImageData::Gray8 {
                width: 0,
                height: 5,
                data: &[],
            }
        ));
    }
}
