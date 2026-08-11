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
        };

        self.objects.insert_stream(r.num, StreamData { dict, data });
        self.images.push((resource.to_vec(), r));
        true
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
