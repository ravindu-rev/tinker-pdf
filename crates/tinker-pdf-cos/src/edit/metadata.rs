//! The document's metadata: typed `/Info` entries (14.3.3) and a
//! caller-supplied XMP packet (14.3.2).
//!
//! # `/Info` and XMP are not kept in step here, and each setter says so
//!
//! A document can state its title twice — `/Info /Title` and the packet's
//! `dc:title` — and ISO 19005 requires the two to agree while ISO 32000-2
//! 14.3.3 deprecates most of `/Info` in favour of the packet. Keeping them in
//! step would mean parsing the packet and rewriting part of it, and this crate
//! neither parses nor rewrites XML: `tinker-pdf-cos` has no edge to
//! `tinker-pdf-xml` and [`crate::outline::xmp_metadata`] documents why it
//! should not grow one to edit a metadata stream. Deriving `/Info` from a
//! packet the caller hands over would be a second, smaller XMP reader.
//!
//! So neither side is derived from the other, and that is **not silent**:
//! every setter returns a [`MetadataSync`], `#[must_use]`, saying whether the
//! other half exists and was left as it was. A caller who gets
//! [`MetadataSync::OtherHalfUnchanged`] has a document that may now say two
//! different things, and is the one who can make them agree — by supplying a
//! packet that says what `/Info` says, which is what the builder's archival
//! profile does for the documents it creates.

use super::docops::pdf_date;
use super::DocumentEditor;
use crate::name::Name;
use crate::object::{Dict, Object, PdfString};
use crate::outline::Trapped;
use crate::text_string::Date;
use crate::write::StreamData;

/// What a metadata write did to the *other* statement of the same metadata.
///
/// After an `/Info` setter the other half is the catalog's XMP packet; after
/// [`DocumentEditor::set_xmp_metadata`] it is the `/Info` dictionary. See
/// [this module](self) for why the editor keeps neither in step with the
/// other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[must_use = "an /Info entry and an XMP packet that disagree are a document that says two things"]
pub enum MetadataSync {
    /// The other half does not exist, so nothing can disagree with what was
    /// written: no catalog `/Metadata` stream after an `/Info` write, no
    /// `/Info` entry after a packet write.
    Alone,
    /// The other half exists and was **not** changed. It may still state the
    /// old value, and a reader that prefers it — ISO 32000-2 14.3.3 points
    /// readers at the packet — shows that.
    OtherHalfUnchanged,
}

impl DocumentEditor {
    /// Whether the catalog names a metadata stream.
    fn has_xmp(&self) -> bool {
        self.catalog()
            .is_some_and(|c| c.get(self.intern(b"Metadata")).is_some())
    }

    /// Whether the document's `/Info` holds any entry.
    fn has_info_entries(&self) -> bool {
        match self.merged_trailer().get(Name::INFO) {
            Some(Object::Ref(r)) => matches!(self.get(*r), Some(Object::Dict(d)) if !d.is_empty()),
            Some(Object::Dict(d)) => !d.is_empty(),
            _ => false,
        }
    }

    fn info_text(&mut self, key: &[u8], value: &str) -> MetadataSync {
        self.set_info(key, value);
        self.xmp_sync()
    }

    fn info_date(&mut self, key: &[u8], date: Date) -> Option<MetadataSync> {
        let text = pdf_date(&date, self.text_version())?;
        let key = self.intern(key);
        // 7.9.4: a date is a string of ASCII digits and zone punctuation,
        // which PDFDocEncoding spells as itself.
        self.set_info_entry(key, Object::String(PdfString::literal(text.into_bytes())));
        Some(self.xmp_sync())
    }

    fn xmp_sync(&self) -> MetadataSync {
        if self.has_xmp() {
            MetadataSync::OtherHalfUnchanged
        } else {
            MetadataSync::Alone
        }
    }

    /// Sets `/Info /Title` (14.3.3 Table 349), creating `/Info` when there is
    /// none. See [`MetadataSync`] for what the result says about the XMP
    /// packet.
    pub fn set_title(&mut self, title: &str) -> MetadataSync {
        self.info_text(b"Title", title)
    }

    /// Sets `/Info /Author`.
    pub fn set_author(&mut self, author: &str) -> MetadataSync {
        self.info_text(b"Author", author)
    }

    /// Sets `/Info /Subject`.
    pub fn set_subject(&mut self, subject: &str) -> MetadataSync {
        self.info_text(b"Subject", subject)
    }

    /// Sets `/Info /Keywords`.
    pub fn set_keywords(&mut self, keywords: &str) -> MetadataSync {
        self.info_text(b"Keywords", keywords)
    }

    /// Sets `/Info /Creator`: the application the original document was
    /// authored in.
    pub fn set_creator(&mut self, creator: &str) -> MetadataSync {
        self.info_text(b"Creator", creator)
    }

    /// Sets `/Info /Producer`: the application that wrote the PDF.
    pub fn set_producer(&mut self, producer: &str) -> MetadataSync {
        self.info_text(b"Producer", producer)
    }

    /// Sets `/Info /CreationDate`, spelled as 7.9.4 spells a date.
    ///
    /// `None`, writing nothing, for a date the syntax has no digits for: a
    /// year outside 0 to 9999, a month outside 1 to 12, an hour past 23, an
    /// offset of a day or more.
    pub fn set_creation_date(&mut self, date: Date) -> Option<MetadataSync> {
        self.info_date(b"CreationDate", date)
    }

    /// Sets `/Info /ModDate`, under [`DocumentEditor::set_creation_date`]'s
    /// rules.
    pub fn set_modification_date(&mut self, date: Date) -> Option<MetadataSync> {
        self.info_date(b"ModDate", date)
    }

    /// Sets `/Info /Trapped` (14.3.3 Table 349), which is a name rather than
    /// a text string.
    pub fn set_trapped(&mut self, trapped: Trapped) -> MetadataSync {
        let value: &[u8] = match trapped {
            Trapped::True => b"True",
            Trapped::False => b"False",
            Trapped::Unknown => b"Unknown",
        };
        let key = self.intern(b"Trapped");
        let value = self.name_object(value);
        self.set_info_entry(key, value);
        self.xmp_sync()
    }

    /// Makes `packet` the document's XMP metadata (14.3.2): the catalog's
    /// `/Metadata`, a stream with `/Type /Metadata /Subtype /XML`.
    ///
    /// The bytes are the caller's and are written **verbatim** — not parsed,
    /// not checked, not re-encoded — and **never compressed**: the writer
    /// leaves any `/Type /Metadata` stream unfiltered whatever
    /// [`crate::write::WriteOptions::compress`] says, because a metadata
    /// stream must be readable by tools that do not decode PDF filters and
    /// ISO 19005 forbids a `/Filter` on one. A catalog that already names a
    /// metadata stream by reference has that object replaced, so anything
    /// else pointing at it sees the new packet; otherwise a new object is
    /// allocated.
    ///
    /// `None`, writing nothing, when there is no catalog. Otherwise the
    /// [`MetadataSync`] says whether `/Info` holds entries the packet was not
    /// checked against.
    pub fn set_xmp_metadata(&mut self, packet: &[u8]) -> Option<MetadataSync> {
        let key = self.intern(b"Metadata");
        let catalog = self.catalog()?;
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, self.name_object(b"Metadata"));
        dict.insert(self.intern(b"Subtype"), self.name_object(b"XML"));
        let stream = catalog.get_ref(key).unwrap_or_else(|| self.allocate());
        self.put_stream(
            stream,
            StreamData {
                dict,
                data: packet.to_vec(),
            },
        );
        self.update_catalog(|catalog| {
            catalog.insert(key, Object::Ref(stream));
        });
        Some(if self.has_info_entries() {
            MetadataSync::OtherHalfUnchanged
        } else {
            MetadataSync::Alone
        })
    }
}
