//! OCF: the container half of an EPUB (3.3 §4.2, §4.3).
//!
//! Gap 30 put `xps/opc.rs` in the facade with the argument that it *"would have
//! exactly one consumer forever: EPUB is **not** OPC"*. That was right and it is
//! confirmed from this side — **0 of 26 measured books** carries
//! `[Content_Types].xml` or `_rels/.rels`, and not one line of `opc.rs` is
//! reused. This is OCF's own layer, in the facade for gap 30's own reason: what
//! is left after `tinker-pdf-zip` and `tinker-pdf-xml` is name arithmetic over
//! already-validated input.
//!
//! # What OCF needs that OPC did not
//!
//! **A `mimetype` rule that is not the check a first implementation writes.**
//! §4.3.2 wants a file named `mimetype`, holding exactly the twenty ASCII bytes
//! `application/epub+zip`, that is the **first file in the ZIP archive**, is
//! **uncompressed**, and has **no extra field**. Three traps live in that one
//! sentence and all three are answered here:
//!
//! - *"First file in the archive" is not `entries()[0]`.* `tinker-pdf-zip`'s
//!   `entries()` is central-directory order, and APPNOTE puts **no** ordering
//!   requirement on the central directory — gap 29's natural-sort section says
//!   so in as many words. The check is [`Entry::header_offset`] `== 0`, which
//!   is physical order and is what §4.3.2 means; it is also what puts
//!   `application/epub+zip` at offset 38 of every conforming container, which
//!   is the whole point of the clause. All twenty-six books milestone 1
//!   measured put `mimetype` first in **both** orders, so a check written
//!   against `index == 0` passes every real book and is still wrong, and the
//!   fixtures that separate them had to be built rather than found.
//! - *"No extra field" was not readable at all.* It is now:
//!   [`tinker_pdf_zip::Archive::local_extra`] parses the one local header a
//!   caller asks about. The field is the **local** header's, which 4.4.11 lets
//!   differ from the directory's — so a field on `Entry` would have answered a
//!   different question — and it is computed on demand, because parsing every
//!   local header at `Archive::open` is exactly what that reader refuses to do.
//! - *The rule is a `MUST` and this engine degrades rather than fails.* Every
//!   one of the five clauses is a **warning**, never a refusal. The signature
//!   that decides the format is `META-INF/container.xml`; see
//!   [`super`]'s header for why the asymmetry is deliberate.
//!
//! **`META-INF`'s six reserved names** (§4.2.6.3) are recognised by name — and
//! recognising a name and *acting* on it are two different rules, so both are
//! here and both are tested. `container.xml` is parsed; `encryption.xml` is
//! parsed far enough to refuse anything that is not one of OCF's two font
//! obfuscations; `signatures.xml`, `rights.xml`, `manifest.xml` and
//! `metadata.xml` are recognised **and ignored**, which is the act — a build
//! that parsed all six would refuse a book over a `signatures.xml` it has no
//! opinion about, and a build that parsed none would read encrypted bytes as a
//! font.
//!
//! A seventh file there is neither refused nor warned about. Milestone 1
//! measured that every book one producer writes carries
//! `META-INF/com.apple.ibooks.display-options.xml`, and **zero** of the twenty
//! fetched books carries anything unreserved there at all: a refusal would have
//! lost every pandoc book while passing the entire downloaded corpus, and a
//! warning every book of one producer trips is not a warning.
//!
//! **Path restrictions (§4.2.3) that are not OPC's part-name grammar.** No
//! leading `/`, no scheme, no dot segment surviving resolution, no climb above
//! the container root, no empty segment, none of §4.2.3's forbidden characters,
//! and a length limit — and comparison is **case-sensitive**, where OPC 6.2.2.3
//! folds ASCII case. Those two rules disagree about the same bytes, which is
//! why `PartName` is not reused for a container path.
//!
//! **Relative-reference resolution (§4.2.5) against the referring document**,
//! with one exception that is the whole of what a first implementation gets
//! wrong. See [`resolve_reference`].
//!
//! # Two constraints OPC imposed that OCF does not
//!
//! OPC forbade encryption outright and every compression method but DEFLATE.
//! OCF permits neither restriction to be assumed: §4.3.2 allows Stored and
//! Deflated, and §4.2.6.3's `encryption.xml` exists precisely because a
//! container may hold encrypted resources. In practice `tinker-pdf-zip` refuses
//! an encrypted entry and refuses `Method::Other` anyway, so the *behaviour* is
//! identical and the *reason* is different — which is worth a comment rather
//! than an inherited assumption.

use std::borrow::Cow;

use tinker_pdf_xml::{Doctype, Event, Limits as XmlLimits, Source};
use tinker_pdf_zip::{Archive, Entry, EntryError, Method};

use super::Limits;
use crate::cbz::ArchiveRefusal;

// ---- The names OCF fixes ----------------------------------------------------

/// §4.3.2's entry, spelled exactly.
pub const MIMETYPE_ITEM: &str = "mimetype";

/// The twenty bytes §4.3.2 requires that entry to hold.
pub const OCF_MEDIA_TYPE: &[u8] = b"application/epub+zip";

/// §4.2.6's directory, with its separator, so a `starts_with` cannot match a
/// file named `META-INFORMATION.txt`.
pub const META_INF: &str = "META-INF/";

/// §4.2.6.3.1's required file: **the signature of the whole format.**
pub const CONTAINER_ITEM: &str = "META-INF/container.xml";

/// §4.2.6.3's optional file describing obfuscated or encrypted resources.
pub const ENCRYPTION_ITEM: &str = "META-INF/encryption.xml";

/// The namespace `container.xml` and `encryption.xml` are written in.
pub const CONTAINER_NAMESPACE: &str = "urn:oasis:names:tc:opendocument:xmlns:container";

/// XML Encryption's namespace, which is where `encryption.xml`'s payload lives.
pub const XMLENC_NAMESPACE: &str = "http://www.w3.org/2001/04/xmlenc#";

/// §5.1's media type for a package document, which is what makes one
/// `<rootfile>` the default rendition rather than another.
pub const PACKAGE_MEDIA_TYPE: &str = "application/oebps-package+xml";

/// OCF's own font obfuscation (§4.4.4): SHA-1 over the unique identifier,
/// XORed across the first 1 040 bytes.
pub const IDPF_OBFUSCATION: &str = "http://www.idpf.org/2008/embedding";

/// Adobe's, which predates it: a 16-byte UUID key across the first 1 024.
pub const ADOBE_OBFUSCATION: &str = "http://ns.adobe.com/pdf/enc#RC";

/// The base a reference resolves against when it is stated relative to the
/// container itself rather than to a document inside it.
///
/// The empty string, because a container path has no leading separator — see
/// [`resolve_reference`], where this is the argument rather than the constant.
pub const OCF_ROOT: &str = "";

// ---- What an entry is -------------------------------------------------------

/// One of §4.2.6.3's six reserved names inside `META-INF`.
///
/// The set is **closed**. A seventh file there is [`Item::UnreservedMetaInf`]
/// and is ignored, which is a measurement rather than a preference: see this
/// module's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reserved {
    /// `container.xml`. Required, and parsed.
    Container,
    /// `encryption.xml`. Optional, and parsed — the only other one that is.
    Encryption,
    /// `manifest.xml`. Recognised and ignored: OCF's own abstract-container
    /// manifest, which nothing here needs, since the archive *is* the manifest.
    Manifest,
    /// `metadata.xml`. Recognised and ignored: container-level metadata, where
    /// this engine reads the package document's instead (§5.5).
    Metadata,
    /// `rights.xml`. Recognised and ignored: digital-rights information, whose
    /// format OCF deliberately does not specify.
    Rights,
    /// `signatures.xml`. Recognised and ignored: XML digital signatures over
    /// the container, which gap 31 names as a non-goal — this engine has no
    /// certificate store and verifying nothing is worse than saying nothing.
    Signatures,
}

impl Reserved {
    /// The reserved name an item is, or `None`.
    ///
    /// **Case-sensitive**, per §4.2.3: `META-INF/Container.xml` is not
    /// `META-INF/container.xml`, and a build that folded case would take a
    /// comic archive holding the first for a book.
    #[must_use]
    pub fn from_item(item: &str) -> Option<Reserved> {
        match item {
            CONTAINER_ITEM => Some(Reserved::Container),
            ENCRYPTION_ITEM => Some(Reserved::Encryption),
            "META-INF/manifest.xml" => Some(Reserved::Manifest),
            "META-INF/metadata.xml" => Some(Reserved::Metadata),
            "META-INF/rights.xml" => Some(Reserved::Rights),
            "META-INF/signatures.xml" => Some(Reserved::Signatures),
            _ => None,
        }
    }
}

/// What one archive entry turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    /// §4.3.2's `mimetype`. An entry and never a publication resource.
    Mimetype,
    /// One of the six.
    Reserved(Reserved),
    /// A file under `META-INF/` that is not one of the six. Ignored.
    UnreservedMetaInf,
    /// A directory record (APPNOTE 4.4.17.1). Holds no bytes.
    ///
    /// Listed rather than folded into the entry it looks like, because one of
    /// milestone 1's two producers writes a **deflated** `META-INF/` directory
    /// record into every book: a `META-INF` walk that took every entry there
    /// for a file meets an empty one first.
    Directory,
    /// Anything else: a publication resource, which is what milestone 4 reads.
    Resource,
}

/// Which of §4.3.2's five clauses a container broke.
///
/// Five names and not one boolean, for milestone 1's stated reason: a book
/// whose `mimetype` is deflated and a book whose `mimetype` is second are two
/// different bugs in two different producers, and a caller that can only be
/// told "the mimetype is wrong" cannot tell them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MimetypeDefect {
    /// There is no entry named `mimetype` at all.
    Missing,
    /// It is not the first file **physically** — `header_offset != 0`.
    ///
    /// Not `index != 0`. See this module's header for why the two are different
    /// claims and why no real book can tell them apart.
    NotFirst,
    /// It is not stored: method 8, or a method this build does not read.
    Compressed,
    /// Its **local** header carries an extra field, which §4.3.2 forbids
    /// because it moves the media type off its fixed offset.
    ExtraField,
    /// Its bytes are not exactly [`OCF_MEDIA_TYPE`].
    WrongMediaType,
}

/// What reading an OCF container tolerated (ruling 10).
///
/// One variant, and `#[non_exhaustive]` because milestone 4 adds the spine's.
/// Every one of §4.3.2's clauses lands here rather than in a refusal: the whole
/// asymmetry of this layer is that the format is decided by
/// [`CONTAINER_ITEM`]'s presence and reported on by everything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OcfWarning {
    /// §4.3.2, per clause.
    Mimetype(MimetypeDefect),
}

// ---- Paths ------------------------------------------------------------------

/// Why a reference is not a container path.
///
/// Seven names rather than one, because a caller acts differently on two of
/// them — [`PathDefect::TooLong`] is a bound this build enforces and the rest
/// are the container being wrong — and because a test asserting only "it
/// failed" cannot tell a working check from a deleted one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PathDefect {
    /// Nothing, or nothing left after resolution.
    Empty,
    /// Past [`super::MAX_OCF_PATH_LEN`].
    TooLong,
    /// A leading `/`. §4.2.3's paths are relative to the container root and a
    /// container has no absolute one.
    Absolute,
    /// The first segment carries a `:`, which is how a scheme is spelled — an
    /// `http:` or a `data:` reference names something outside the container,
    /// and this engine performs no I/O of any kind.
    Scheme,
    /// A `..` that climbs above the container root.
    ///
    /// **Refused rather than clamped**, and the difference from
    /// [`crate::xps`]'s resolver is deliberate. RFC 3986 §5.2.4 discards an
    /// over-climbing segment, so `../../a` from the root becomes `a` — which
    /// inside a container silently renames a reference to a *different*
    /// resource that may well exist. gap 30's milestone 8 recorded the clamp as
    /// the standard's behaviour; §4.2.3 forbids `..` in a container path at
    /// all, so here there is nothing to clamp to.
    ClimbsOut,
    /// An empty segment: `a//b`, or a trailing `/` naming a directory.
    EmptySegment,
    /// One of §4.2.3's forbidden characters: `" * : < > ? \ |`, a C0 or C1
    /// control, `DEL`, a `.` as a name's last character, or a `/` that arrived
    /// by percent-escape and would otherwise become a separator after the fact.
    RestrictedCharacter,
}

/// RFC 3986 §5.2 for the path-only references a container holds, with §4.2.3's
/// restrictions applied to the result.
///
/// `referring` is the **path of the document the reference was written in**,
/// not its directory: the merge cuts at that path's last `/`, which is RFC 3986
/// §5.2.3's own rule and is what makes an `href` in `EPUB/text/ch1.xhtml`
/// resolve `../images/a.png` to `EPUB/images/a.png`.
///
/// # The one reference in this format whose base is not its referring document
///
/// `container.xml`'s `full-path` is defined by §4.2.6.3.1 as a path **from the
/// container root**, so it is resolved against [`OCF_ROOT`] and not against
/// `META-INF/container.xml`. Every real book makes the difference visible:
/// milestone 1 measured one producer putting its package document at
/// `EPUB/package.opf` and the other at `content.opf`, and resolving either
/// against the referring document yields `META-INF/EPUB/package.opf` or
/// `META-INF/content.opf` — a name no book holds.
///
/// This is the same off-by-one-segment gap 30's milestone 3 had to get right
/// for `.rels`, where a relationships part's targets resolve against its
/// *owner* rather than against the `_rels` directory it sits in. It is worth a
/// paragraph rather than a line because the wrong answer fails in the direction
/// that looks like a missing file.
///
/// # Order of operations, which is not interchangeable
///
/// 1. The fragment is dropped. It is never part of a path, and milestone 4's
///    cross-references carry one on nearly every `href`.
/// 2. **The length is charged before any work**, against the reference as
///    written — `tinker-pdf-zip`'s posture, where a permit is what has been
///    promised. Charging after the merge would mean allocating the 128 MiB path
///    first and refusing it second.
/// 3. Scheme and leading `/` are refused, on the reference as written.
/// 4. The merge, then RFC 3986 §5.2.4's dot-segment removal — **before**
///    percent-decoding, because `%2E` is not a dot segment and treating it as
///    one would let a reference climb by escape.
/// 5. Percent-decoding, per segment, so a decoded `/` stays inside the segment
///    it was written in rather than becoming a separator; such a segment is
///    then refused, since §4.2.3 makes `/` the separator and nothing else.
/// 6. §4.2.3's character restrictions, on the decoded segments.
///
/// # Errors
/// [`PathDefect`], one variant per rule.
pub fn resolve_reference(
    referring: &str,
    reference: &str,
    limits: &Limits,
) -> Result<String, PathDefect> {
    // 1.
    let reference = reference.split('#').next().unwrap_or("");
    if reference.is_empty() {
        return Err(PathDefect::Empty);
    }
    // 2.
    if reference.len() > limits.max_path_len {
        return Err(PathDefect::TooLong);
    }
    // 3. A `//` opens an authority, which is a host this engine will not visit;
    // a single `/` is the absolute form §4.2.3 says a container has none of.
    if reference.starts_with('/') {
        return Err(PathDefect::Absolute);
    }
    if reference.split('/').next().unwrap_or("").contains(':') {
        return Err(PathDefect::Scheme);
    }

    // 4. §5.2.3's merge: everything up to and including the base's last `/`.
    let cut = referring.rfind('/').map_or(0, |at| at + 1);
    let merged = format!("{}{reference}", referring.get(..cut).unwrap_or(OCF_ROOT));

    let mut segments: Vec<&str> = Vec::new();
    for segment in merged.split('/') {
        match segment {
            "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(PathDefect::ClimbsOut);
                }
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return Err(PathDefect::Empty);
    }

    // 5 and 6.
    let mut out = String::with_capacity(merged.len());
    for (at, segment) in segments.iter().enumerate() {
        let decoded = percent_decode(segment);
        if let Some(defect) = segment_defect(&decoded) {
            return Err(defect);
        }
        if at > 0 {
            out.push('/');
        }
        out.push_str(&decoded);
    }
    if out.len() > limits.max_path_len {
        return Err(PathDefect::TooLong);
    }
    Ok(out)
}

/// §4.2.3's restrictions on one file or directory name.
///
/// **What is deliberately not here is the 255-byte limit** the same clause puts
/// on a file name. It is an interoperability rule about file systems this
/// engine never writes to, and enforcing it would refuse a path that names an
/// entry the container actually holds — which loses a book over somebody else's
/// `PATH_MAX`. [`super::MAX_OCF_PATH_LEN`] is the bound; this is the grammar.
fn segment_defect(segment: &str) -> Option<PathDefect> {
    if segment.is_empty() {
        return Some(PathDefect::EmptySegment);
    }
    for c in segment.chars() {
        let code = u32::from(c);
        if matches!(c, '/' | '"' | '*' | ':' | '<' | '>' | '?' | '\\' | '|')
            || code < 0x20
            || (0x7F..=0x9F).contains(&code)
        {
            return Some(PathDefect::RestrictedCharacter);
        }
    }
    // "FULL STOP as the last character" — `.` and `..` are gone by now, so what
    // this catches is `chapter.` and nothing else.
    if segment.ends_with('.') {
        return Some(PathDefect::RestrictedCharacter);
    }
    None
}

/// Percent-decoding for one segment, UTF-8 or not at all.
///
/// A ZIP name under general-purpose bit 11 is UTF-8, so the decoded octets are
/// compared as UTF-8 or the escapes are left standing. An escape sequence that
/// spells no valid UTF-8 is **kept as written** rather than replaced, which is
/// `opc.rs`'s answer to the same question and for the same reason: a name that
/// has been lossily rewritten resolves to a part nobody wrote.
fn percent_decode(segment: &str) -> Cow<'_, str> {
    if !segment.contains('%') {
        return Cow::Borrowed(segment);
    }
    let bytes = segment.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0usize;
    while at < bytes.len() {
        match escape_at(bytes, at) {
            Some(octet) => {
                out.push(octet);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    match String::from_utf8(out) {
        Ok(text) => Cow::Owned(text),
        Err(_) => Cow::Borrowed(segment),
    }
}

/// The octet a `%XX` at `at` stands for, or `None` when there is not one there.
fn escape_at(bytes: &[u8], at: usize) -> Option<u8> {
    if bytes.get(at).copied()? != b'%' {
        return None;
    }
    let hi = hex_value(bytes.get(at + 1).copied()?)?;
    let lo = hex_value(bytes.get(at + 2).copied()?)?;
    Some(hi * 16 + lo)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ---- container.xml ----------------------------------------------------------

/// One `<rootfile>`, as `container.xml` wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootfileRef {
    /// The `full-path` attribute, unresolved.
    pub full_path: String,
    /// The `media-type` attribute.
    pub media_type: String,
}

/// The default rendition, resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rootfile {
    /// What `container.xml` said.
    pub declared: String,
    /// What that resolves to against the container root.
    pub path: String,
    /// The media type, which is [`PACKAGE_MEDIA_TYPE`] by construction.
    pub media_type: String,
    /// The archive entry it names.
    pub index: usize,
}

/// Parses `META-INF/container.xml` (§4.2.6.3.1).
///
/// Every `<rootfile>` under `<rootfiles>` comes back, in document order,
/// including the ones naming media types this build has never heard of —
/// §4.2.6.3.1 permits multiple renditions and the *first* whose media type is
/// [`PACKAGE_MEDIA_TYPE`] is the default one. Choosing here would throw away
/// the evidence a later milestone needs to pick a different rendition.
///
/// **Elements this build does not know are ignored rather than refused.**
/// §4.2.6.3.1's own schema allows a `<links>` element beside `<rootfiles>`, and
/// milestone 1 found a producer writing a seventh file into `META-INF` that no
/// version of the specification names — a reader that refuses what it has not
/// been introduced to refuses the future. What is *not* tolerated is the root:
/// a file at this name whose root element is not `container` in
/// [`CONTAINER_NAMESPACE`] is not this file.
///
/// The doctype mode is [`Doctype::Refuse`], which is deliberate and is not
/// milestone 2's relaxation being forgotten. Milestone 1 measured **zero**
/// document type declarations on any `container.xml` in either corpus; the
/// relaxation exists for XHTML content documents, where 100 % of one producer's
/// output carries one, and extending it to a file that has never had one would
/// widen the attack surface for nothing.
///
/// # Errors
/// [`ArchiveRefusal::UnreadableContainer`] for markup that is not well formed,
/// whose root is not `container`, or that holds no `<rootfile>` at all.
pub fn parse_container(
    bytes: &[u8],
    limits: &XmlLimits,
) -> Result<Vec<RootfileRef>, ArchiveRefusal> {
    let source = Source::new(bytes).map_err(|_| ArchiveRefusal::UnreadableContainer)?;
    let mut out: Vec<RootfileRef> = Vec::new();
    let mut depth = 0usize;
    let mut root = false;
    for event in source.reader_with(limits, Doctype::Refuse) {
        let element = match event.map_err(|_| ArchiveRefusal::UnreadableContainer)? {
            Event::Start(element) => element,
            // An empty-element tag produces a `Start` and then an `End`, so the
            // two are counted against each other — three `<rootfile/>`s are
            // three elements at depth three and not one nest three deep. Gap
            // 30's milestone 3 recorded forgetting this as the whole of its
            // first failing run.
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => continue,
        };
        depth += 1;
        if depth == 1 {
            root =
                element.namespace() == Some(CONTAINER_NAMESPACE) && element.local() == "container";
            if !root {
                return Err(ArchiveRefusal::UnreadableContainer);
            }
            continue;
        }
        if depth != 3
            || element.local() != "rootfile"
            || element.namespace() != Some(CONTAINER_NAMESPACE)
        {
            continue;
        }
        let (Some(full_path), Some(media_type)) = (
            element.attribute(None, "full-path"),
            element.attribute(None, "media-type"),
        ) else {
            // §4.2.6.3.1 makes both attributes required, and a `<rootfile>`
            // missing one names nothing. Skipped rather than refused, so a
            // container carrying one broken rendition beside a good one is the
            // book it is.
            continue;
        };
        out.push(RootfileRef {
            full_path: full_path.to_owned(),
            media_type: media_type.to_owned(),
        });
    }
    if !root || out.is_empty() {
        return Err(ArchiveRefusal::UnreadableContainer);
    }
    Ok(out)
}

// ---- encryption.xml ---------------------------------------------------------

/// Which of the two obfuscations covers a resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Obfuscation {
    /// OCF §4.4.4: SHA-1 of the package's unique identifier, 1 040 bytes.
    Idpf,
    /// Adobe's: a 16-byte UUID key, 1 024 bytes.
    Adobe,
}

/// One `<EncryptedData>` this build is willing to see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obfuscated {
    /// The container path its `<CipherReference URI>` resolves to.
    pub path: String,
    /// Which algorithm.
    pub algorithm: Obfuscation,
}

/// What `encryption.xml` said, once it has been agreed that all of it is
/// obfuscation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Encryption {
    entries: Vec<Obfuscated>,
}

impl Encryption {
    /// The obfuscated resources, in document order.
    ///
    /// Empty for a book with no `encryption.xml`, which is almost every book:
    /// milestone 1 found two in twenty-six.
    #[must_use]
    pub fn entries(&self) -> &[Obfuscated] {
        &self.entries
    }
}

/// Parses `META-INF/encryption.xml` far enough to know whether anything real is
/// encrypted.
///
/// The whole question this answers is *may the resources be read at all*. OCF's
/// two font obfuscations are not encryption — they are a XOR over the first
/// kilobyte, done so a font's licence terms are not trivially violated by
/// dragging the file out of the archive — and a book using either is a book
/// this engine will read. **Anything else means a key this engine does not
/// have**, and a book whose resources cannot be read is refused by that name
/// rather than paginated into a complete-looking book of placeholders.
///
/// The `<CipherReference>` URIs are collected as well as the algorithms, and
/// that is not decoration: without them this function could be a substring
/// search for two URLs, and a substring search cannot tell an
/// `<EncryptedData>` that names an algorithm from one that does not. An
/// `<EncryptedData>` with no `<EncryptionMethod>` is refused for exactly that
/// reason — XML Encryption allows the algorithm to be implied by the key, and
/// an algorithm this build cannot see is one it cannot agree to.
///
/// # Errors
/// [`ArchiveRefusal::EncryptedResources`] for an algorithm that is not one of
/// the two, and [`ArchiveRefusal::UnreadableContainer`] for markup that will
/// not read, whose root is not `encryption`, or whose `URI` is not a container
/// path.
pub fn parse_encryption(bytes: &[u8], limits: &Limits) -> Result<Encryption, ArchiveRefusal> {
    let source = Source::new(bytes).map_err(|_| ArchiveRefusal::UnreadableContainer)?;
    let mut out = Encryption::default();
    let mut depth = 0usize;
    let mut root = false;
    let mut algorithm: Option<String> = None;
    let mut uris: Vec<String> = Vec::new();
    let mut inside = false;
    for event in source.reader_with(&limits.xml, Doctype::Refuse) {
        match event.map_err(|_| ArchiveRefusal::UnreadableContainer)? {
            Event::Start(element) => {
                depth += 1;
                if depth == 1 {
                    root = element.namespace() == Some(CONTAINER_NAMESPACE)
                        && element.local() == "encryption";
                    if !root {
                        return Err(ArchiveRefusal::UnreadableContainer);
                    }
                    continue;
                }
                if element.namespace() != Some(XMLENC_NAMESPACE) {
                    continue;
                }
                match element.local() {
                    "EncryptedData" => {
                        inside = true;
                        algorithm = None;
                        uris.clear();
                    }
                    "EncryptionMethod" if inside => {
                        algorithm = element.attribute(None, "Algorithm").map(str::to_owned);
                    }
                    "CipherReference" if inside => {
                        if let Some(uri) = element.attribute(None, "URI") {
                            uris.push(uri.to_owned());
                        }
                    }
                    _ => {}
                }
            }
            Event::End(name) => {
                depth = depth.saturating_sub(1);
                if !inside
                    || name.namespace() != Some(XMLENC_NAMESPACE)
                    || name.local() != "EncryptedData"
                {
                    continue;
                }
                inside = false;
                let algorithm = match algorithm.take().as_deref() {
                    Some(IDPF_OBFUSCATION) => Obfuscation::Idpf,
                    Some(ADOBE_OBFUSCATION) => Obfuscation::Adobe,
                    // Both arms are the same refusal and they are two
                    // different facts: a named algorithm this build will not
                    // accept, and no named algorithm at all.
                    _ => return Err(ArchiveRefusal::EncryptedResources),
                };
                for uri in uris.drain(..) {
                    // §4.2.5 again, and the base is the container root: a
                    // `CipherReference` in `META-INF/encryption.xml` names a
                    // publication resource, not a sibling of the file it is
                    // written in.
                    let path = resolve_reference(OCF_ROOT, &uri, limits)
                        .map_err(|_| ArchiveRefusal::UnreadableContainer)?;
                    out.entries.push(Obfuscated { path, algorithm });
                }
            }
            _ => {}
        }
    }
    if !root {
        return Err(ArchiveRefusal::UnreadableContainer);
    }
    Ok(out)
}

// ---- The container ----------------------------------------------------------

/// Whether an archive is an OCF container.
///
/// **The presence of `META-INF/container.xml`, and nothing else.** Byte-exact
/// and case-sensitive per §4.2.3, and no read at all: a comic archive stops
/// here having cost one comparison per entry.
///
/// The near miss that decides this is not hypothetical. One of milestone 1's
/// two producers writes a `META-INF/` **directory record** into every book, so
/// a comic archive that acquired one — from an archiver that preserves empty
/// directories, or from a repack of a book's images — carries the directory and
/// no file in it. That archive is a comic, and this is the line that says so.
#[must_use]
pub fn is_container(archive: &Archive<'_>) -> bool {
    archive.entries().iter().any(|e| e.name == CONTAINER_ITEM)
}

/// A ZIP read as an OCF container: what each entry is, what §4.3.2 made of it,
/// and a read-once cache.
pub struct Ocf<'a> {
    archive: Archive<'a>,
    items: Vec<Item>,
    /// One slot per archive entry. `Some` once the entry has been read, whether
    /// it produced bytes or a refusal — so a second resolution of the same file
    /// spends no more of [`Archive`]'s inflation budget, which
    /// `resolving_a_file_twice_does_not_inflate_it_twice` asserts through
    /// [`Archive::inflated`].
    ///
    /// gap 30's `opc.rs` states the underlying fact and it is worth restating
    /// where the second caller is: `Archive::read` charges the budget on every
    /// call and never refunds it, so reading `container.xml` to route the file
    /// and again to resolve its rootfile would spend it twice. That is a
    /// correctness requirement rather than an optimisation — a book whose files
    /// are large enough would be refused for a bound it never actually
    /// exceeded.
    cache: Vec<Option<Result<Cow<'a, [u8]>, EntryError>>>,
    warnings: Vec<OcfWarning>,
    limits: Limits,
    /// The default rendition, resolved at most once.
    rendition: Option<Result<Rootfile, ArchiveRefusal>>,
    /// `encryption.xml`, read at most once. The outer `Option` is "not tried
    /// yet" and the inner is "tried and refused".
    encryption: Option<Result<Encryption, ArchiveRefusal>>,
}

impl<'a> Ocf<'a> {
    /// Indexes an archive as a container and checks §4.3.2.
    ///
    /// **Nothing is refused here**, which is the same posture `opc::Package`
    /// takes for the same reason: the discrimination has already happened, and
    /// every one of §4.3.2's clauses is a warning by decision.
    #[must_use]
    pub fn open(archive: Archive<'a>, limits: &Limits) -> Ocf<'a> {
        let items: Vec<Item> = archive.entries().iter().map(classify).collect();
        let cache = vec![None; items.len()];
        let mut ocf = Ocf {
            archive,
            items,
            cache,
            warnings: Vec::new(),
            limits: *limits,
            rendition: None,
            encryption: None,
        };
        ocf.check_mimetype();
        ocf
    }

    /// The archive underneath, for a caller that wants its warnings or its
    /// spend.
    #[must_use]
    pub fn archive(&self) -> &Archive<'a> {
        &self.archive
    }

    /// What each archive entry turned out to be, in the archive's own order.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Everything §4.3.2 tolerated.
    #[must_use]
    pub fn warnings(&self) -> &[OcfWarning] {
        &self.warnings
    }

    /// The entry index of an item, by the name the archive spelled.
    ///
    /// Byte-exact and case-sensitive (§4.2.3), and the **first** entry of that
    /// name. A ZIP may legally hold two entries with one name and §4.2.3
    /// forbids a container from doing so; this build does not detect it, which
    /// is written down in gap 31's milestone 3 progress as owed rather than
    /// left to be discovered.
    #[must_use]
    pub fn index_of(&self, item: &str) -> Option<usize> {
        self.archive.entries().iter().position(|e| e.name == item)
    }

    /// Reads one entry, **once**.
    ///
    /// # Errors
    /// Whatever the archive said, or [`EntryError::NoSuchEntry`] for an index
    /// it does not have.
    pub fn read(&mut self, index: usize) -> Result<&[u8], EntryError> {
        if !matches!(self.cache.get(index), Some(Some(_))) {
            let read = self.archive.read(index);
            if let Some(slot) = self.cache.get_mut(index) {
                *slot = Some(read);
            }
        }
        match self.cache.get(index) {
            Some(Some(Ok(data))) => Ok(data),
            Some(Some(Err(why))) => Err(*why),
            _ => Err(EntryError::NoSuchEntry),
        }
    }

    /// The default rendition's package document (§4.2.6.3.1), resolved to an
    /// entry this container holds.
    ///
    /// Resolved as well as parsed, because the two failures are different
    /// sentences with different names — and because which of them comes back is
    /// the **only** observable difference between resolving `full-path` against
    /// the container root and resolving it against `META-INF/`. A build that
    /// got that wrong answers [`ArchiveRefusal::RootfileMissing`] on every real
    /// book, and one that never resolved at all could not tell.
    ///
    /// # Errors
    /// [`ArchiveRefusal::UnreadableContainer`] when `container.xml` will not
    /// read or names no package document, [`ArchiveRefusal::TooLarge`] when its
    /// `full-path` is past [`super::MAX_OCF_PATH_LEN`], and
    /// [`ArchiveRefusal::RootfileMissing`] when the path it resolves to names
    /// no entry.
    pub fn default_rendition(&mut self) -> Result<&Rootfile, ArchiveRefusal> {
        if self.rendition.is_none() {
            let found = self.find_rendition();
            self.rendition = Some(found);
        }
        match &self.rendition {
            Some(Ok(rootfile)) => Ok(rootfile),
            Some(Err(why)) => Err(*why),
            None => Err(ArchiveRefusal::UnreadableContainer),
        }
    }

    /// What `encryption.xml` said, or an empty answer when there is none.
    ///
    /// # Errors
    /// [`ArchiveRefusal::EncryptedResources`] for an algorithm that is not one
    /// of the two obfuscations, and
    /// [`ArchiveRefusal::UnreadableContainer`] when the file will not read.
    pub fn encryption(&mut self) -> Result<&Encryption, ArchiveRefusal> {
        if self.encryption.is_none() {
            let read = self.read_encryption();
            self.encryption = Some(read);
        }
        match &self.encryption {
            Some(Ok(encryption)) => Ok(encryption),
            Some(Err(why)) => Err(*why),
            None => Err(ArchiveRefusal::UnreadableContainer),
        }
    }

    fn find_rendition(&mut self) -> Result<Rootfile, ArchiveRefusal> {
        let limits = self.limits;
        let index = self
            .index_of(CONTAINER_ITEM)
            .ok_or(ArchiveRefusal::UnreadableContainer)?;
        let parsed = match self.read(index) {
            Ok(bytes) => parse_container(bytes, &limits.xml),
            Err(_) => Err(ArchiveRefusal::UnreadableContainer),
        };
        let rootfile = parsed?
            .into_iter()
            .find(|r| r.media_type == PACKAGE_MEDIA_TYPE)
            .ok_or(ArchiveRefusal::UnreadableContainer)?;
        // The base is the container root and **not** `META-INF/container.xml`.
        // See `resolve_reference`.
        let path =
            resolve_reference(OCF_ROOT, &rootfile.full_path, &limits).map_err(
                |defect| match defect {
                    PathDefect::TooLong => ArchiveRefusal::TooLarge,
                    _ => ArchiveRefusal::UnreadableContainer,
                },
            )?;
        let index = self
            .index_of(&path)
            .ok_or(ArchiveRefusal::RootfileMissing)?;
        Ok(Rootfile {
            declared: rootfile.full_path,
            path,
            media_type: rootfile.media_type,
            index,
        })
    }

    fn read_encryption(&mut self) -> Result<Encryption, ArchiveRefusal> {
        let limits = self.limits;
        let Some(index) = self.index_of(ENCRYPTION_ITEM) else {
            return Ok(Encryption::default());
        };
        match self.read(index) {
            Ok(bytes) => parse_encryption(bytes, &limits),
            Err(_) => Err(ArchiveRefusal::UnreadableContainer),
        }
    }

    /// §4.3.2's five clauses, each recorded on its own.
    ///
    /// The order is the specification's own sentence — present, first,
    /// uncompressed, no extra field, and holding the media type — so a
    /// container that breaks several reads its warnings in the order the clause
    /// states them.
    fn check_mimetype(&mut self) {
        let Some(index) = self.index_of(MIMETYPE_ITEM) else {
            self.warnings
                .push(OcfWarning::Mimetype(MimetypeDefect::Missing));
            return;
        };
        let Some(entry) = self.archive.entries().get(index).cloned() else {
            return;
        };
        // **`header_offset == 0`, not `index == 0`.** The first local file
        // header of a ZIP sits at offset zero, and §4.3.2's clause is what puts
        // `application/epub+zip` at offset 38 of a conforming container — a
        // property the central directory's order cannot express at all.
        if entry.header_offset != 0 {
            self.warnings
                .push(OcfWarning::Mimetype(MimetypeDefect::NotFirst));
        }
        if entry.method != Method::Stored {
            self.warnings
                .push(OcfWarning::Mimetype(MimetypeDefect::Compressed));
        }
        if self
            .archive
            .local_extra(index)
            .is_some_and(|e| !e.is_empty())
        {
            self.warnings
                .push(OcfWarning::Mimetype(MimetypeDefect::ExtraField));
        }
        let right = matches!(self.read(index), Ok(bytes) if bytes == OCF_MEDIA_TYPE);
        if !right {
            self.warnings
                .push(OcfWarning::Mimetype(MimetypeDefect::WrongMediaType));
        }
    }
}

/// What one entry is, by its stored name alone.
#[must_use]
pub fn classify(entry: &Entry) -> Item {
    // 4.4.17.1: a stored path ending in `/` is a directory record. Checked
    // first, because `META-INF/` is one and is not a file in `META-INF`.
    if entry.is_directory() {
        return Item::Directory;
    }
    if entry.name == MIMETYPE_ITEM {
        return Item::Mimetype;
    }
    if entry.name.starts_with(META_INF) {
        return match Reserved::from_item(&entry.name) {
            Some(reserved) => Item::Reserved(reserved),
            None => Item::UnreservedMetaInf,
        };
    }
    Item::Resource
}
