//! Packages built for the tests, as against the eight nobody here wrote.
//!
//! `tests/xps/` is the corpus milestone 1 obtained from two Microsoft
//! serialisers, and it is what says the reader works on real files. This module
//! is the other half, and gap 29's milestone 5 wrote down why it has to exist:
//! **a suite of positive assertions cannot catch a weakened check.** The
//! fixtures worth having are the near misses and the disagreements, and no
//! producer will ever hand you one — a package whose document order and
//! filename order differ, a `PageContent` naming an image part, a `ContentBox`
//! of three numbers.
//!
//! Everything here builds *conforming* packages by default and lets a test
//! break exactly one thing, through [`with`], so a fixture's defect is the one
//! line the test wrote rather than something it inherited.
//!
//! # Why the allow, and why the path attribute
//!
//! Three test binaries include this module — `xps_opc.rs`, `xps_spine.rs` and
//! `xps_qpdf.rs` — and each compiles its own copy and uses a different subset.
//! The ZIP writer comes from `cbz_support` by path rather than by a second
//! `mod` declaration in each binary, so an XPS test never has to know that the
//! archive builder it needs was written for comics.

#![allow(
    dead_code,
    unused_imports,
    reason = "shared by three test binaries; each uses a different subset"
)]

#[path = "../cbz_support/mod.rs"]
mod archive_writer;

pub use archive_writer::{distinct_pixels, rgb_png, zip, Damage, ZipFile};

use tinker_pdf::cbz;
use tinker_pdf::xps::opc;
use tinker_pdf::{ArchiveRefusal, Document, OpenError};
use tinker_pdf_zip::{limits as zip_limits, Limits as ZipLimits};

// ---- the two dialects ---------------------------------------------------

/// XPS 1.0's namespace, which is what every Microsoft serialiser writes.
pub const XPS_NS: &str = "http://schemas.microsoft.com/xps/2005/06";

/// ECMA-388 Table D-2's OpenXPS namespace.
pub const OXPS_NS: &str = "http://schemas.openxps.org/oxps/v1.0";

// ---- the parts a package is made of -------------------------------------

/// A content-types item that resolves every part of [`one_page_package`].
///
/// The three payload types are `Default`s over the extensions Windows uses, so
/// a package built here resolves its media types the way a real one does — and
/// a test that wants routing to *fail* has to say so, rather than getting it
/// for free from a fixture that never declared anything.
pub const CONTENT_TYPES: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />"#,
    r#"<Default Extension="fdseq" ContentType="application/vnd.ms-package.xps-fixeddocumentsequence+xml" />"#,
    r#"<Default Extension="fdoc" ContentType="application/vnd.ms-package.xps-fixeddocument+xml" />"#,
    r#"<Default Extension="fpage" ContentType="application/vnd.ms-package.xps-fixedpage+xml" />"#,
    r#"<Default Extension="png" ContentType="image/png" />"#,
    r#"</Types>"#
);

/// [`CONTENT_TYPES`] with `extra` elements before the closing tag.
///
/// 7.2.3.5 makes an `Override` beat a `Default`, so this is how a test says
/// "this part is a fixed page whatever its name ends in" — or the reverse.
pub fn content_types_with(extra: &str) -> String {
    CONTENT_TYPES.replace("</Types>", &format!("{extra}</Types>"))
}

/// The package relationships part, naming one fixed representation.
pub fn package_rels(target: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="{XPS_NS}/fixedrepresentation" Target="{target}" Id="R0" /></Relationships>"#
    )
}

/// A `FixedDocumentSequence` naming documents **in the order given**.
pub fn sequence(documents: &[&str]) -> String {
    let refs: String = documents
        .iter()
        .map(|source| format!(r#"<DocumentReference Source="{source}" />"#))
        .collect();
    format!(r#"<FixedDocumentSequence xmlns="{XPS_NS}">{refs}</FixedDocumentSequence>"#)
}

/// A `FixedDocument` naming pages **in the order given**, which is markup
/// order and is the only order 12.3.1 defines.
pub fn document(pages: &[&str]) -> String {
    let refs: String = pages
        .iter()
        .map(|source| format!(r#"<PageContent Source="{source}" />"#))
        .collect();
    format!(r#"<FixedDocument xmlns="{XPS_NS}">{refs}</FixedDocument>"#)
}

/// A `FixedPage` of the stated size, drawing nothing.
pub fn fixed_page(width: &str, height: &str) -> String {
    fixed_page_with(width, height, "")
}

/// A `FixedPage` of the stated size carrying `extra` attributes verbatim.
pub fn fixed_page_with(width: &str, height: &str, extra: &str) -> String {
    format!(r#"<FixedPage xmlns="{XPS_NS}" Width="{width}" Height="{height}" {extra}/>"#)
}

// ---- building a package -------------------------------------------------

/// One item of a package to build.
pub struct Part {
    pub name: String,
    pub data: Vec<u8>,
}

/// A text part.
pub fn part(name: &str, data: &str) -> Part {
    Part {
        name: name.to_string(),
        data: data.as_bytes().to_vec(),
    }
}

/// A part whose bytes are not text.
pub fn binary_part(name: &str, data: Vec<u8>) -> Part {
    Part {
        name: name.to_string(),
        data,
    }
}

/// Builds an archive out of parts, deflated, with no damage.
///
/// Deflated rather than stored, and it matters for one test: `Archive::inflated`
/// is the only public measure of the archive's budget and a **stored** entry
/// spends none of it, so a read-once cache checked over a stored part asserts
/// `0 == 0` and passes with the cache deleted. Milestone 1 measured both — WPF
/// stores `_rels/.rels` and the XPS object model deflates it — so either would
/// have been a defensible fixture and only one of them can see the defect.
pub fn archive(parts: Vec<Part>) -> Vec<u8> {
    let files: Vec<ZipFile> = parts
        .iter()
        .map(|p| {
            // A directory record holds no bytes, and deflating none of them is
            // not what an archiver writes.
            if p.data.is_empty() {
                ZipFile::stored(&p.name, &p.data)
            } else {
                ZipFile::deflated(&p.name, &p.data)
            }
        })
        .collect();
    zip(&files, Damage::None)
}

/// The smallest package that is an XPS: one document, one page, 816 x 1056.
///
/// `[Content_Types].xml` goes **last**, as it does in all eight of milestone
/// 1's real packages — OPC 7.3.7 leaves its position unconstrained and a reader
/// that assumed it was first would be wrong on the first real file.
pub fn one_page_package() -> Vec<Part> {
    vec![
        part("_rels/.rels", &package_rels("/FixedDocumentSequence.fdseq")),
        part(
            "FixedDocumentSequence.fdseq",
            &sequence(&["Documents/1/FixedDocument.fdoc"]),
        ),
        part(
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage"]),
        ),
        part("Documents/1/Pages/1.fpage", &fixed_page("816", "1056")),
        part("[Content_Types].xml", CONTENT_TYPES),
    ]
}

/// Replaces one part's bytes in place, keeping its position.
pub fn with(mut parts: Vec<Part>, name: &str, data: &str) -> Vec<Part> {
    for slot in &mut parts {
        if slot.name == name {
            slot.data = data.as_bytes().to_vec();
            return parts;
        }
    }
    parts.push(part(name, data));
    parts
}

/// Inserts a part **before** the content-types item, so that item stays last.
pub fn before_content_types(mut parts: Vec<Part>, new: Part) -> Vec<Part> {
    let at = parts.len().saturating_sub(1);
    parts.insert(at, new);
    parts
}

// ---- opening one --------------------------------------------------------

/// Through the whole facade, which is where a defect costs a page.
pub fn open(bytes: &[u8]) -> Result<Document, OpenError> {
    Document::open(bytes.to_vec())
}

/// The refusal a package came back with, if it is one.
pub fn refusal(bytes: &[u8]) -> Option<ArchiveRefusal> {
    match Document::open(bytes.to_vec()) {
        Err(OpenError::UnsupportedArchive(why)) => Some(why),
        _ => None,
    }
}

/// A package read through the OPC layer directly, for the tests that are about
/// the layer rather than about the facade.
pub fn opened(bytes: &[u8]) -> opc::Package<'_> {
    let archive =
        cbz::open_archive(bytes, &ZipLimits::DEFAULT).expect("the fixture is a readable ZIP");
    opc::Package::open(
        archive,
        zip_limits::MAX_ZIP_NAME_LEN,
        tinker_pdf_xml::Limits::DEFAULT,
    )
}
