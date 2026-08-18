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
//! Five test binaries include this module — `xps_opc.rs`, `xps_spine.rs`,
//! `xps_markup.rs`, `xps_glyphs.rs` and `xps_qpdf.rs` — and each compiles its
//! own copy and uses a different subset. The ZIP writer comes from
//! `cbz_support` by path rather than by a second `mod` declaration in each
//! binary, so an XPS test never has to know that the archive builder it needs
//! was written for comics.

#![allow(
    dead_code,
    unused_imports,
    reason = "shared by five test binaries; each uses a different subset"
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

// ---- fonts, and the parts that carry them -------------------------------

/// The GUID `tests/xps/`'s real ODTTF part carries, reused so a fixture and
/// the corpus exercise the same key arithmetic.
pub const FONT_GUID: &str = "595c31af-dbe8-48a5-a032-c677a052f501";

/// A font part named the way 9.1.7.3 wants one named.
pub fn font_part_name(extension: &str) -> String {
    format!("Resources/{FONT_GUID}.{extension}")
}

/// 9.1.7.3, applied. The XOR is its own inverse, so obfuscating and
/// de-obfuscating are the same thirty-two operations — which is why a fixture
/// can be built with the code under test's own arithmetic **only** if that
/// arithmetic is not what is being tested. It is not: this is written out
/// here, from the clause, so a defect in `font::deobfuscate` cannot cancel
/// itself out against the fixture that feeds it.
pub fn obfuscate(guid: &str, bytes: &[u8]) -> Vec<u8> {
    let digits: String = guid.chars().filter(|c| *c != '-').collect();
    let mut key = [0u8; 16];
    for (at, slot) in key.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&digits[at * 2..at * 2 + 2], 16).expect("a GUID");
    }
    key.reverse();
    let mut out = bytes.to_vec();
    for (at, byte) in out.iter_mut().take(32).enumerate() {
        *byte ^= key[at % 16];
    }
    out
}

/// A TrueType program whose `cmap` disagrees with the glyph a run names, and
/// whose four glyphs have four different advances.
///
/// Four glyphs at 1000 units per em. Glyph 0 is empty; glyph 1 is a square in
/// the **bottom-left** quarter of the em, 0 to 300; glyph 2 is a square in the
/// **top-right**, 400 to 700; glyph 3 fills 0 to 700. The advances are 1000,
/// **500**, 1000 and **250**, so a build that used a nominal width rather than
/// the font's own puts every glyph after the first in the wrong place.
///
/// The `cmap` maps `A` to glyph **1** and `B` to glyph **3**, and nothing
/// else. That is the disagreement gap 30's row 7 asks a fixture to prove:
/// `Indices="2"` over `UnicodeString="A"` must draw the top-right square, and
/// a build that reached the glyph through the character draws the bottom-left
/// one — correct, plausible, and not what the file said.
pub fn disagreeing_font() -> Vec<u8> {
    let square = |low: i16, high: i16| -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1i16.to_be_bytes()); // one contour
        for value in [low, low, high, high] {
            out.extend_from_slice(&value.to_be_bytes()); // bounding box
        }
        out.extend_from_slice(&3u16.to_be_bytes()); // last point index
        out.extend_from_slice(&0u16.to_be_bytes()); // no instructions
        out.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on curve, i16 deltas
        let side = high - low;
        for dx in [low, side, 0, -side] {
            out.extend_from_slice(&dx.to_be_bytes());
        }
        for dy in [low, 0, side, 0] {
            out.extend_from_slice(&dy.to_be_bytes());
        }
        out
    };

    let mut glyf = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0]; // glyph 0 is empty
    for (low, high) in [(0i16, 300i16), (400, 700), (0, 700)] {
        glyf.extend_from_slice(&square(low, high));
        loca.push(u32::try_from(glyf.len()).expect("a small font"));
    }
    let mut loca_bytes = Vec::new();
    for offset in &loca {
        loca_bytes.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&4u16.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&4u16.to_be_bytes());

    let mut hmtx = Vec::new();
    for advance in [1000u16, 500, 1000, 250] {
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    // cmap format 4, two segments: `A` to glyph 1 and `B` to glyph 3.
    let mut sub = Vec::new();
    for value in [4u16, 40, 0, 6, 4, 1, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    let a = u16::from(b'A');
    let b = u16::from(b'B');
    for value in [
        a,
        b,
        0xFFFF, // endCode
        0,      // reservedPad
        a,
        b,
        0xFFFF, // startCode
        1u16.wrapping_sub(a),
        3u16.wrapping_sub(b),
        1, // idDelta
        0,
        0,
        0, // idRangeOffset
    ] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);

    sfnt(&[
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca_bytes),
        (b"maxp", &maxp),
    ])
}

/// A table directory over the tables given, in the order given.
pub fn sfnt(tables: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(
        &u16::try_from(tables.len())
            .expect("few tables")
            .to_be_bytes(),
    );
    // searchRange, entrySelector and rangeShift, which are what milestone 1's
    // transposed key got wrong while every table tag still read correctly.
    let entry_selector = u16::try_from(usize::BITS - 1 - tables.len().leading_zeros()).unwrap_or(0);
    let search_range = 16u16 << entry_selector;
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(
        &(u16::try_from(tables.len()).expect("few tables") * 16 - search_range).to_be_bytes(),
    );
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).expect("a small font").to_be_bytes());
        out.extend_from_slice(
            &u32::try_from(data.len())
                .expect("a small table")
                .to_be_bytes(),
        );
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// A TrueType Collection over the faces given.
///
/// The header is 9.1.7's own case: a `FontUri` may carry a `#n` fragment
/// naming one of these, and `Sfnt::parse` takes the first whatever the
/// fragment says.
pub fn collection(faces: &[&[u8]]) -> Vec<u8> {
    let header = 12 + faces.len() * 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&u32::try_from(faces.len()).expect("few faces").to_be_bytes());
    let mut offset = header;
    for face in faces {
        out.extend_from_slice(&u32::try_from(offset).expect("a small file").to_be_bytes());
        offset += face.len();
    }
    for face in faces {
        // A collection's table offsets are from the **file's** start and not
        // the face's, so a face copied in verbatim would point every table at
        // whatever happened to be that far into the collection. Getting this
        // wrong builds a fixture that is not a font, and the test that reads
        // it then measures the fixture rather than the code.
        let base = u32::try_from(out.len()).expect("a small file");
        let mut copy = face.to_vec();
        let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
        for at in 0..count {
            let entry = 12 + at * 16 + 8;
            let was = u32::from_be_bytes([
                face[entry],
                face[entry + 1],
                face[entry + 2],
                face[entry + 3],
            ]);
            copy[entry..entry + 4].copy_from_slice(&(was + base).to_be_bytes());
        }
        out.extend_from_slice(&copy);
    }
    out
}
