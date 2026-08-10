//! Building documents (phase 12).
//!
//! A thin authoring layer over the writer: pages, content, fonts, metadata and
//! an outline. It deliberately does no layout — placing what a caller already
//! positioned is this crate's business, and composing paragraphs is not.

use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::write::{rewrite, ObjectSet, StreamData, WriteOptions};

/// A page being assembled.
pub struct PageBuilder {
    width: f64,
    height: f64,
    content: Vec<u8>,
    fonts: Vec<(Vec<u8>, ObjRef)>,
}

impl PageBuilder {
    /// Writes text at a position, in points from the bottom-left.
    ///
    /// `font` names a font registered with [`DocumentBuilder::add_base_font`].
    pub fn text(&mut self, font: &[u8], size: f64, x: f64, y: f64, text: &str) {
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

    /// Appends raw content-stream operators.
    ///
    /// An escape hatch for callers that know the operator set; nothing checks
    /// what goes in, so a malformed sequence produces a malformed page.
    pub fn raw(&mut self, operators: &[u8]) {
        self.content.extend_from_slice(operators);
        self.content.push(b'\n');
    }
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
            let mut resources = Dict::new();
            if !font_dict.is_empty() {
                resources.insert(self.names.intern(b"Font"), Object::Dict(font_dict));
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
