//! An EPUB is told from a comic, and stops being its cover (gap 31, milestone 3).
//!
//! `PK\x03\x04` is one signature over five formats. Gap 29 read it as a comic
//! archive; gap 30 taught [`crate::Document::open`] to recognise an XPS first,
//! by ECMA-388 E.3's three steps, and named the rest of the list in the same
//! sentence — *"CBZ, XPS, EPUB, ODF, OOXML and every JAR ever built"*. This
//! module is the entry nobody went back for.
//!
//! # The route, and why gap 30's is untouched
//!
//! E.3's step 2 asks whether the archive holds `[Content_Types].xml` **and**
//! `_rels/.rels`. Gap 31's milestone 1 measured twenty-six real books and
//! **none** of them holds either, so an EPUB fails E.3 at step 2's first check
//! having read nothing, and the comic fallthrough is exactly what E.3's own
//! text asks for. [`ArchiveRefusal::UnreadablePackage`] is therefore
//! unreachable for an EPUB. **Gap 30 is not wrong; EPUB is a different question
//! it did not ask**, and the order in [`crate::Document::open`] says so: XPS
//! first, then this, then the comic path.
//!
//! # What the signature is, and what it deliberately is not
//!
//! **The signature is `META-INF/container.xml`.** OCF 3.3 §4.2.6.3 makes it the
//! one file every container must hold, and it is what tells this format from
//! every other ZIP: an ODF has `META-INF/manifest.xml` and no `container.xml`,
//! a JAR has `META-INF/MANIFEST.MF`, and a comic archive that happens to carry
//! a `META-INF/` directory record — which one of milestone 1's two producers
//! writes into every book — has no file in it at all.
//!
//! **The signature is not the `mimetype` rule.** §4.3.2 requires an entry
//! named `mimetype`, first in the archive, uncompressed and with no extra
//! field, holding exactly `application/epub+zip`. That is a `MUST` and this
//! engine degrades rather than fails (ruling 2), so a book that breaks it is
//! **warned about and read anyway**: refusing one would lose a book over a ZIP
//! field that changes nothing about its contents. The asymmetry — one clause
//! decides the format and the other only reports on it — is decided here rather
//! than by whoever writes the next layer.
//!
//! The comparison is byte-exact and case-sensitive, because §4.2.3 says
//! container paths are compared case-sensitively. `META-INF/Container.xml` is
//! not `META-INF/container.xml`, and a build that folded case would route a
//! comic archive holding the first into this module.
//!
//! # What this milestone does with a book, and what it does not
//!
//! It refuses it, **by a name that is true of it**. The reflowable engine is
//! milestones 4 to 12; until the spine is read there are no pages, and gap 31's
//! plan puts this milestone third for exactly that reason: *"it is the only one
//! of the thirteen that improves matters on its own — after it, an EPUB is
//! refused by a name that is true instead of opening as its cover."*
//!
//! So the four refusals below are what a caller sees today, and
//! [`ArchiveRefusal::UnpaginatedBook`] is the one that goes away when milestone
//! 4 lands. The other three are permanent.

pub mod ocf;

use tinker_pdf_xml::Limits as XmlLimits;
use tinker_pdf_zip::Archive;

use crate::cbz::ArchiveRefusal;

// ---- Bounds -----------------------------------------------------------------

/// The most bytes one container path may be.
///
/// OCF 3.3 §4.2.3's own number, taken rather than invented: *"content paths
/// MUST NOT exceed 65535 bytes"*. It is a bound in ruling 1's sense as well as
/// a validity rule, because a path is a `String` this engine builds out of an
/// attacker-chosen XML attribute value — and an attribute value may be as long
/// as the part that holds it.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends | 65 535 (the container built *past* this cap names a path of 65 536; the longest path in any real book here is 39) |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **65 535** |
///
/// Charged against the reference **before** it is merged with a base and before
/// its dot segments are removed, which is `tinker-pdf-zip`'s own posture: a
/// permit is what has been promised, not what happened to arrive. Charging
/// afterwards would mean allocating the 128 MiB path first and refusing it
/// second.
///
/// **Not** a cap on a container path's *depth*, and that absence is argued
/// rather than overlooked: nothing here touches a filesystem, so depth bounds
/// no allocation and no recursion — `remove_dot_segments` is a loop over a
/// `Vec` — and length is what costs. It is `tinker-pdf-zip`'s stated reason for
/// having no such cap either.
///
/// **Not** a cap on one *segment*, and that is a different absence with a
/// different argument. §4.2.3 also says a file name must not exceed 255 bytes,
/// and that clause is enforced by nothing here: it is an interoperability rule
/// about somebody else's file system, this engine never writes a file, and
/// refusing a path that names an entry the container actually holds would lose
/// a book over it. See [`ocf::resolve_reference`].
///
/// Reachable: `a_content_path_past_the_length_cap_is_refused_by_name` builds
/// the real 65 536-byte reference rather than lowering the constant, because a
/// cap proved only against a lowered copy of itself has not been proved to
/// fire.
pub const MAX_OCF_PATH_LEN: usize = 65_535;

/// Resource ceilings for reading an OCF container.
///
/// Separate from [`crate::cbz::Limits`] and from [`crate::xps::Limits`] for
/// their stated reason: they bound different things, and a caller that lowered
/// one should not silently lower another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of one container path. See [`MAX_OCF_PATH_LEN`].
    pub max_path_len: usize,
    /// What the markup reader is allowed to spend, per file.
    pub xml: XmlLimits,
}

impl Limits {
    /// The constants above, which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_path_len: MAX_OCF_PATH_LEN,
        xml: XmlLimits::DEFAULT,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---- Routing ----------------------------------------------------------------

/// What one already-open archive turned out to be.
///
/// The archive travels **inside** this enum on the way out rather than being
/// opened twice, which is gap 30 milestone 3's exit criterion inherited whole:
/// opening it to sniff and again to read doubles the central-directory walk and
/// creates a window in which the two reads could disagree about the same bytes.
///
/// Two variants and not three. Milestone 4 adds the third — a book that
/// paginates — and this enum is `#[non_exhaustive]` so that costs an addition
/// rather than a break.
#[non_exhaustive]
pub enum Routing<'a> {
    /// An OCF container, and this build will not lay it out yet. **Refused by
    /// name**, which is the feature rather than the absence of one.
    Refused(ArchiveRefusal),
    /// Not an EPUB. The archive is handed back **unread** for the comic path.
    NotEpub(Archive<'a>),
}

/// Decides whether an open archive is an EPUB, and reads it as far as this
/// build goes.
///
/// The cheap half runs first and costs no read at all: a ZIP with no
/// `META-INF/container.xml` item is not an OCF container, which is every comic
/// archive there has ever been, and it comes straight back as
/// [`Routing::NotEpub`] having spent nothing.
#[must_use]
pub fn route<'a>(archive: Archive<'a>, limits: &Limits) -> Routing<'a> {
    if !ocf::is_container(&archive) {
        return Routing::NotEpub(archive);
    }
    let mut book = ocf::Ocf::open(archive, limits);
    Routing::Refused(read(&mut book))
}

/// How far milestone 3 reads a book, and what it says when it stops.
///
/// The order is `container.xml` first and `encryption.xml` second, and it is a
/// decision rather than an accident. `container.xml` is the file §4.2.6.3
/// **requires**, and a container whose required file will not read has failed
/// before the question of what is encrypted arises; `encryption.xml` is
/// optional and, when it is there, is about the publication's resources rather
/// than about the container. A book that breaks both is reported by the first,
/// which is the more basic failure.
fn read(book: &mut ocf::Ocf<'_>) -> ArchiveRefusal {
    // The rootfile is resolved as well as parsed: a container naming a package
    // document the book does not hold is a different sentence from one whose
    // markup will not read, and the two have different names. Which of them a
    // build reports is also the only observable difference between resolving
    // `full-path` against the container root and resolving it against
    // `META-INF/` — see `ocf::Ocf::default_rendition`.
    if let Err(why) = book.default_rendition() {
        return why;
    }
    // Read far enough to know whether the two font obfuscations are all that is
    // going on. Anything else is real encryption, this engine holds no key, and
    // a book whose resources it cannot read is refused by that name rather than
    // paginated into placeholders — which would be a complete-looking book of
    // nothing.
    if let Err(why) = book.encryption() {
        return why;
    }
    // Everything OCF has to say about this book has been read and none of it
    // refused. What is left is the package document, the spine and a layout
    // engine, which is milestones 4 to 12.
    ArchiveRefusal::UnpaginatedBook
}

#[cfg(test)]
mod tests;
