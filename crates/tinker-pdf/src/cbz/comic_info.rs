//! `ComicInfo.xml`: the one entry in a comic archive that is metadata
//! (tier 4, W-ARCHIVE milestone 1).
//!
//! Feature documentation: `docs/features/cbz.md`; the design is
//! `docs/design/comic-archives.md`.
//!
//! # What changed, and what deliberately did not
//!
//! Gap 29 skipped this file by name and was right to, three times over: an
//! entry that is not an image is not a page, warning about it would bury the
//! warnings that matter, and the comic path had no consumer for a title. The
//! third of those stopped being true when `DocumentBuilder::set_info` got a
//! caller — `epub.rs` writes `dc:title` and `dc:creator` into a synthesised
//! book's `/Info` — so a comic archive that carries a title and throws it away
//! is now the odd one out rather than the consistent one.
//!
//! **It is still not a page.** `extension_claims_image("ComicInfo.xml")` stays
//! false and the entry still produces no [`super::PageOrigin`]; what this
//! module adds is a second thing an entry can be, beside *page* and *ignored*,
//! and the distinction between the three is the part that must not blur.
//!
//! # The schema, and how much of it is read
//!
//! `ComicInfo.xml` has no standard behind it. It is ComicRack's, its
//! `ComicInfo.xsd` is what every other reader copied, and the elements are
//! unprefixed and in no namespace — the two `xsi:` attributes a real file
//! carries name a schema this build never fetches and never validates against.
//! Fifty-odd elements are defined and **six** are read here, because six is
//! what `/Info` has anywhere sensible to put (Table 14.3.3's eight keys, less
//! the two dates and the two this engine writes for itself):
//!
//! | `ComicInfo` | `/Info` | Why |
//! | --- | --- | --- |
//! | `Title` | `/Title` | The same field under two names. |
//! | `Series` + `Number` | `/Title`, as `Series #Number`, **only when `Title` is absent** | Most issues carry no `Title` at all: the series and the number *are* the name of the book, and a viewer with an empty title bar shows a filename instead. |
//! | `Series` + `Number` | `/Keywords` | Always, so the series identity survives whichever branch above won and nothing measured is thrown away. |
//! | `Writer`, then `Penciller` | `/Author` | 14.3.3: "the name of the person who created the document". A comic has no single one, and these are the two credits every file carries; joined with `, ` in that order, duplicates dropped, because one person often did both. |
//! | `Summary` | `/Subject` | 14.3.3: "the subject of the document". A summary is exactly that, and it is the only long field here. |
//!
//! `Publisher` is deliberately **not** `/Creator` or `/Producer`: those two
//! name the application that made the file (14.3.3), this engine is that
//! application, and a comic's publisher is a fact about the story rather than
//! about the bytes. Writing it there would make `/Producer` a field a caller
//! cannot trust for the one thing it is for.
//!
//! # Untrusted, and bounded as such
//!
//! The entry is archive bytes, so ruling 1 binds: [`MAX_COMIC_INFO_BYTES`]
//! decides whether the parse is attempted at all, `tinker-pdf-xml`'s four caps
//! bound the parse itself, and [`Doctype::Refuse`] is passed explicitly — a
//! comic archive has never needed a document type declaration and gap 31's
//! relaxation exists for XHTML content documents, so widening it here would be
//! attack surface bought for nothing.
//!
//! Everything that goes wrong degrades (ruling 2) and is named (ruling 10):
//! the document is still every page the archive holds, with
//! [`super::ArchiveWarning::ComicInfo`] saying what the metadata did.

use tinker_pdf_xml::{Doctype, Event, Limits as XmlLimits, Source};

/// The most bytes of `ComicInfo.xml` this build will parse.
///
/// A cap on the *entry*, decided before one byte is handed to the XML reader,
/// rather than four caps inside the parse and a `/Info` string of whatever
/// survived them. The reason it is here at all is that
/// [`super::MAX_SYNTHESISED_PDF`] is charged **per page**: a `/Info`
/// dictionary is not a page, so nothing else in the comic path would have
/// bounded a 128 MiB `<Summary>` on its way into the document.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends | 88 (`the_metadata_a_comic_carries_reaches_the_document`) |
/// | A 200-page comic's own `ComicInfo.xml` | ~16 KB — the credits are a few hundred bytes and the rest is `<Pages>`, one `<Page>` element of about sixty bytes per page |
/// | **This cap** | **64 KiB** |
///
/// A margin of four, and the thing being bounded is a metadata file rather
/// than a picture: an entry past this is not a plausible `ComicInfo.xml` and
/// is refused as [`ComicInfoDefect::TooLarge`] rather than truncated, because
/// half a title is a wrong title.
pub const MAX_COMIC_INFO_BYTES: usize = 64 << 10;

/// The stored path a comic's metadata is read from, and the only one.
///
/// Matched case-insensitively against the **whole** stored path, so a
/// `chapter1/ComicInfo.xml` is not read. That is narrow on purpose: ComicRack
/// writes it at the archive root, a document has exactly one `/Info`
/// dictionary, and a nested file describes something that is not this
/// document — picking one of several by directory order would make the
/// document's title depend on what a packing tool happened to walk first.
const COMIC_INFO_PATH: &str = "comicinfo.xml";

/// Why a `ComicInfo.xml` that is present did not become metadata.
///
/// Named rather than collapsed, for [`super::PageDefect`]'s reason: a
/// host that can say *the metadata was too large* has something to show, and
/// "no title" is what an archive with no `ComicInfo.xml` at all looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ComicInfoDefect {
    /// Past [`MAX_COMIC_INFO_BYTES`], so the parse was never attempted.
    TooLarge,
    /// The archive would not hand the entry's bytes over: encrypted, a
    /// checksum failure, a compression method not read here.
    EntryRefused(super::ZipEntryError),
    /// Markup that is not well formed, an encoding that will not decode, a
    /// document type declaration, or one of `tinker-pdf-xml`'s four caps.
    Unreadable,
    /// Well-formed markup whose root element is not `ComicInfo`.
    ///
    /// Distinct from [`ComicInfoDefect::Unreadable`] because the two blame
    /// different things: one says the file is broken and this one says the
    /// file is *something else* wearing the name.
    NotComicInfo,
}

impl core::fmt::Display for ComicInfoDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ComicInfoDefect::TooLarge => f.write_str("a ComicInfo.xml past the size this reads"),
            ComicInfoDefect::EntryRefused(e) => {
                write!(f, "a ComicInfo.xml the archive refused: {e}")
            }
            ComicInfoDefect::Unreadable => f.write_str("a ComicInfo.xml that could not be read"),
            ComicInfoDefect::NotComicInfo => {
                f.write_str("an XML entry named ComicInfo.xml that is not one")
            }
        }
    }
}

/// The six fields of `ComicInfo.xml` this build reads.
///
/// Plain owned strings, trimmed, and every one optional — the schema requires
/// nothing, and a file that names one element is as ordinary as one that names
/// forty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComicInfo {
    title: Option<String>,
    series: Option<String>,
    number: Option<String>,
    writer: Option<String>,
    penciller: Option<String>,
    summary: Option<String>,
}

impl ComicInfo {
    /// `<Title>`, the issue's own name where it has one.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// `<Series>`.
    #[must_use]
    pub fn series(&self) -> Option<&str> {
        self.series.as_deref()
    }

    /// `<Number>`. A string rather than a number: `12`, `12.5`, `0`, `Annual`
    /// and `1 of 6` are all real values, and parsing it would refuse four of
    /// those five for no gain.
    #[must_use]
    pub fn number(&self) -> Option<&str> {
        self.number.as_deref()
    }

    /// `<Writer>`.
    #[must_use]
    pub fn writer(&self) -> Option<&str> {
        self.writer.as_deref()
    }

    /// `<Penciller>`.
    #[must_use]
    pub fn penciller(&self) -> Option<&str> {
        self.penciller.as_deref()
    }

    /// `<Summary>`.
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// Whether none of the six was named.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == ComicInfo::default()
    }

    /// The `/Info` entries this becomes, in write order.
    ///
    /// The mapping is *here* rather than at the call site so that a caller and
    /// a test read it from the same place: `the_metadata_a_comic_carries_reaches_the_document`
    /// asserts against the document's own `/Info`, and this is the only thing
    /// that decides what is in it. The module header's table is this function
    /// in prose.
    #[must_use]
    pub fn info_entries(&self) -> Vec<(&'static [u8], String)> {
        let mut out: Vec<(&'static [u8], String)> = Vec::new();
        let issue = self.issue_name();
        if let Some(title) = self.title.clone().or_else(|| issue.clone()) {
            out.push((b"Title".as_slice(), title));
        }
        if let Some(author) = self.author() {
            out.push((b"Author".as_slice(), author));
        }
        if let Some(summary) = self.summary.clone() {
            out.push((b"Subject".as_slice(), summary));
        }
        if let Some(issue) = issue {
            out.push((b"Keywords".as_slice(), issue));
        }
        out
    }

    /// `Series #Number`, or whichever half exists.
    fn issue_name(&self) -> Option<String> {
        match (self.series.as_deref(), self.number.as_deref()) {
            (Some(series), Some(number)) => Some(format!("{series} #{number}")),
            (Some(series), None) => Some(series.to_owned()),
            // A number with no series names nothing a reader could use, and
            // `#7` in a title bar is worse than the filename it replaces.
            (None, _) => None,
        }
    }

    /// `Writer`, then `Penciller`, joined — and one name when they are one
    /// person, which is common enough that the duplicate would be noticed.
    fn author(&self) -> Option<String> {
        match (self.writer.as_deref(), self.penciller.as_deref()) {
            (Some(w), Some(p)) if w == p => Some(w.to_owned()),
            (Some(w), Some(p)) => Some(format!("{w}, {p}")),
            (Some(one), None) | (None, Some(one)) => Some(one.to_owned()),
            (None, None) => None,
        }
    }
}

/// Whether a stored path is the metadata entry.
///
/// ASCII case folding and nothing else. A ZIP name is bytes decoded as UTF-8
/// or CP437 (APPNOTE D.1) and a Unicode case fold is a per-locale answer this
/// engine may not have (ruling 4); every writer that emits this file emits it
/// in ASCII.
#[must_use]
pub fn is_comic_info(name: &str) -> bool {
    name.len() == COMIC_INFO_PATH.len() && name.eq_ignore_ascii_case(COMIC_INFO_PATH)
}

/// Reads a `ComicInfo.xml`.
///
/// # Errors
/// [`ComicInfoDefect`], one variant per way it did not become metadata. Never
/// an error the caller must act on: the document is every page the archive
/// holds either way, and this decides only whether it carries a title.
pub fn parse(bytes: &[u8], limits: &XmlLimits) -> Result<ComicInfo, ComicInfoDefect> {
    if bytes.len() > MAX_COMIC_INFO_BYTES {
        return Err(ComicInfoDefect::TooLarge);
    }
    let source = Source::new(bytes).map_err(|_| ComicInfoDefect::Unreadable)?;

    let mut info = ComicInfo::default();
    let mut depth = 0usize;
    // The element whose characters are being gathered, and what has been
    // gathered so far. `Event::Text` arrives in as many pieces as the reader
    // chooses, and `<Title>A &amp; B</Title>` is three of them.
    let mut gathering: Option<(Field, String)> = None;

    for event in source.reader_with(limits, Doctype::Refuse) {
        match event.map_err(|_| ComicInfoDefect::Unreadable)? {
            Event::Text(text) | Event::Cdata(text) => {
                if let Some((_, gathered)) = &mut gathering {
                    gathered.push_str(&text);
                }
            }
            Event::Start(element) => {
                depth += 1;
                match depth {
                    1 => {
                        // No namespace, because `ComicInfo.xsd` declares none:
                        // a file whose root is in one is a different format
                        // that has borrowed the name.
                        if element.namespace().is_some() || element.local() != "ComicInfo" {
                            return Err(ComicInfoDefect::NotComicInfo);
                        }
                    }
                    2 => gathering = Field::of(element.local()).map(|f| (f, String::new())),
                    // A mapped element's children are markup the schema does
                    // not define. Their text is not gathered and they are not
                    // an error: `<Summary><p>…</p></Summary>` is a file some
                    // tool wrote and the summary it carries is not this one.
                    _ => {}
                }
            }
            Event::End(_) => {
                if let Some((field, gathered)) = gathering.take() {
                    let value = gathered.trim();
                    if !value.is_empty() {
                        // First occurrence wins. The schema allows one of each
                        // and a file with two `<Title>`s has not said which,
                        // so the rule is the one `epub::package` already
                        // takes for `dc:title`: document order, first.
                        let slot = field.slot(&mut info);
                        if slot.is_none() {
                            *slot = Some(value.to_owned());
                        }
                    }
                }
                depth = depth.saturating_sub(1);
            }
            Event::Comment(_) | Event::Instruction { .. } => {}
        }
    }

    // A well-formed `<ComicInfo/>` that names none of the six is **not** a
    // defect and produces no warning. Nothing was tolerated, repaired or lost:
    // the file was read exactly as written and it said nothing this build maps,
    // which is gap 29's "warning about each of them would bury the warnings
    // that matter" holding in its own narrow case. The empty value is still a
    // value, so `ArchiveReport::comic_info` tells *an empty ComicInfo.xml* from
    // *no ComicInfo.xml at all* without spending a warning on it.
    Ok(info)
}

/// One of the six, as a value rather than six booleans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Title,
    Series,
    Number,
    Writer,
    Penciller,
    Summary,
}

impl Field {
    /// The element names, exactly as `ComicInfo.xsd` spells them. Case
    /// sensitive, because XML names are.
    fn of(local: &str) -> Option<Field> {
        Some(match local {
            "Title" => Field::Title,
            "Series" => Field::Series,
            "Number" => Field::Number,
            "Writer" => Field::Writer,
            "Penciller" => Field::Penciller,
            "Summary" => Field::Summary,
            _ => return None,
        })
    }

    fn slot(self, info: &mut ComicInfo) -> &mut Option<String> {
        match self {
            Field::Title => &mut info.title,
            Field::Series => &mut info.series,
            Field::Number => &mut info.number,
            Field::Writer => &mut info.writer,
            Field::Penciller => &mut info.penciller,
            Field::Summary => &mut info.summary,
        }
    }
}
