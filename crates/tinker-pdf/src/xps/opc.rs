//! Open Packaging Conventions: a ZIP becomes a package of named parts (gap 30,
//! milestone 3).
//!
//! ECMA-388's normative reference is ECMA-376 1st edition Part 2; **the clause
//! numbers here are the 5th edition's (December 2021)**, which is what Ecma
//! publishes today and what was read to write this. The rules are unchanged and
//! the numbering is not, so a reader chasing E.3's "§9.2" will find it at 7.3.
//!
//! # Why this is a module and not a crate
//!
//! It is close to a leaf by ruling 8's definition — bytes and names in, names
//! and byte ranges out — and the reason it is not one is that it would have
//! exactly one consumer forever: EPUB is **not** OPC. Gap 31's container is OCF
//! with `META-INF/container.xml`, so it reuses `tinker-pdf-xml` and reuses
//! nothing here. The byte-level risk lives in `tinker-pdf-zip` and
//! `tinker-pdf-xml`, which are leaves and are independently fuzzed; what is
//! left here is name arithmetic and graph resolution over already-validated
//! input.
//!
//! # The four things a naive reader gets wrong on the first real file
//!
//! **A part name is not a ZIP item name.** 7.3.4 and 7.3.5 give the mapping in
//! both directions and it is not "prepend a slash": it is RFC 3987's
//! URI-to-IRI conversion, which decodes the percent-escapes that stand for
//! characters *outside* US-ASCII and leaves every other escape alone. That is
//! what makes the mapping a round trip — `%41` decoded to `A` would re-encode
//! to `A`, and the name would have changed.
//!
//! **`[Content_Types].xml` is an item and never a part.** 7.3.7 chose the
//! brackets *because* they violate 6.2.2.2's part-name grammar, so it can never
//! collide with one. [`PartName::from_item`] refuses it for that reason rather
//! than by a special case, and its position in the archive is unconstrained: in
//! all eight packages milestone 1 committed it is the **last** of five to eight
//! items.
//!
//! **Media types are resolved by 7.2.3.5's ordered algorithm, not by
//! extension.** `Override` beats `Default`, and both compare ASCII
//! case-insensitively — which is not decoration: every package in milestone 1's
//! corpus carries `<Default Extension="ODTTF" …>` in upper case against a part
//! named `….ODTTF`, in **both** dialects, and a byte comparison against
//! `odttf` finds nothing.
//!
//! **Targets are relative references and must be resolved against the source
//! part.** Milestone 1 measured both directions in one corpus: WPF writes
//! `Target="../../../Resources/….png"` in a page's relationships part beside
//! `ImageSource="/Resources/….png"` in the page's own markup, and the XPS
//! object model writes `Target="FixedDocumentSequence.fdseq"` — relative — in
//! the *package* relationships part, where WPF writes it absolute. A resolver
//! that handled only one form refuses one of the two producers.
//!
//! # What `tinker-pdf-zip` owes the caller, and what is done about it here
//!
//! That crate's doc comment says it *"neither normalises the path nor resolves
//! `..`, because it never touches a filesystem and inventing a canonical form
//! would be a claim about one"*, which is the right posture for a leaf and puts
//! all four of these here:
//!
//! 1. **There is no lookup by name** — [`Package`] builds its own index, over
//!    normalised names.
//! 2. **`Archive::read` charges the inflation budget on every call and never
//!    refunds it**, so reading `[Content_Types].xml` to decide the format and
//!    again to use it spends it twice. [`Package::read`] caches, and that is a
//!    correctness requirement rather than an optimisation.
//! 3. **A truncated name is kept, not dropped.** A name past
//!    `MAX_ZIP_NAME_LEN` survives with a `Warning::NameTruncated`, which for a
//!    comic is the right leniency and for a package produces a name that
//!    resolves to the wrong part or to none. Such an entry becomes
//!    [`Item::Unresolvable`] — see [`Package::open`] for how it is attributed.
//! 4. **Duplicate names are neither detected nor deduplicated**, and 6.2.2.3
//!    makes that invalid. [`Package::validate`] refuses rather than picking
//!    one, because "whichever came first" is directory order and directory
//!    order is whatever the producing tool walked.

use std::borrow::Cow;
use std::collections::HashSet;

use tinker_pdf_xml::{Event, Limits as XmlLimits, Source};
use tinker_pdf_zip::{Archive, EntryError, Warning as ZipWarning};

// ---- The names OPC fixes ----------------------------------------------------

/// The content-types stream's ZIP item name (7.3.7). An item, never a part.
pub const CONTENT_TYPES_ITEM: &str = "[Content_Types].xml";

/// The package relationships part's ZIP item name (6.5.2).
pub const PACKAGE_RELATIONSHIPS_ITEM: &str = "_rels/.rels";

/// The package relationships part's part name.
pub const PACKAGE_RELATIONSHIPS_PART: &str = "/_rels/.rels";

/// The base a relative target in the *package* relationships part resolves
/// against: the package root, which is not itself a part name.
pub const PACKAGE_ROOT: &str = "/";

/// The namespace `[Content_Types].xml` is written in.
pub const CONTENT_TYPES_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/content-types";

/// The namespace every `.rels` part is written in.
pub const RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";

// ---- Part names -------------------------------------------------------------

/// A part name: 6.2.2.2's grammar, normalised, with 6.2.2.3's equivalence.
///
/// Equality and hashing are **ASCII case-insensitive**, because 6.2.2.3 says a
/// package holding `/a` may not also hold `/A` — the two are one name. The
/// spelling the package used is kept as well, because a report that renamed a
/// part to talk about it would be reporting about a part the package does not
/// have.
#[derive(Clone, Debug, Eq)]
pub struct PartName {
    text: String,
    folded: String,
}

impl PartialEq for PartName {
    fn eq(&self, other: &Self) -> bool {
        self.folded == other.folded
    }
}

impl std::hash::Hash for PartName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.folded.hash(state);
    }
}

impl core::fmt::Display for PartName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartName {
    /// 7.3.5: a ZIP item name becomes a part name.
    ///
    /// A leading slash is added and RFC 3987 §3.2's URI-to-IRI conversion is
    /// applied — only the escapes standing for characters outside US-ASCII are
    /// decoded. The result is checked against 6.2.2.2, so an item name that is
    /// not a part name (`[Content_Types].xml`, a directory record, anything
    /// carrying a backslash) answers `None` rather than becoming a part whose
    /// name no package could hold.
    #[must_use]
    pub fn from_item(item: &str) -> Option<PartName> {
        if item.is_empty() || item.starts_with('/') || item.contains('\\') {
            return None;
        }
        PartName::from_absolute(&format!("/{}", percent_decode_non_ascii(item)))
    }

    /// A part name that is already absolute — an `Override`'s `PartName`, or a
    /// resolved relationship target — checked against 6.2.2.2.
    #[must_use]
    pub fn from_absolute(name: &str) -> Option<PartName> {
        if !valid_part_name(name) {
            return None;
        }
        Some(PartName {
            text: name.to_string(),
            folded: name.to_ascii_lowercase(),
        })
    }

    /// 7.3.4: the part name becomes a ZIP item name.
    ///
    /// Every character outside US-ASCII is percent-encoded as its UTF-8 octets
    /// and the leading slash is removed. Composed with [`PartName::from_item`]
    /// in either order this is the identity, which is what
    /// `every_item_name_in_the_inventory_round_trips` asserts over all
    /// fifty-two parts of milestone 1's corpus.
    #[must_use]
    pub fn item_name(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut out = String::with_capacity(self.text.len());
        for c in self.text.chars().skip(1) {
            if c.is_ascii() {
                out.push(c);
                continue;
            }
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).as_bytes() {
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 0x0F)]));
            }
        }
        out
    }

    /// The name as the package spells it, leading slash and all.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The name folded for 6.2.2.3's comparison. ASCII only — the clause says
    /// ASCII, and a Unicode case fold would be a different rule.
    #[must_use]
    pub fn folded(&self) -> &str {
        &self.folded
    }

    /// 7.2.3.5's extension: everything right of the rightmost dot in the
    /// rightmost segment, or `None` when that segment has no dot.
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        let last = self.text.rsplit('/').next()?;
        last.rsplit_once('.').map(|(_, extension)| extension)
    }

    /// 6.5.2: `/a/b.xml`'s relationships part is `/a/_rels/b.xml.rels`.
    ///
    /// The `_rels` segment goes *before* the last one and `.rels` is appended
    /// to it — which is the off-by-one-segment a first implementation gets
    /// wrong, and the reason relative targets in this part resolve against the
    /// **source** part rather than against this one.
    #[must_use]
    pub fn relationships_part(&self) -> Option<PartName> {
        let (parent, last) = self.text.rsplit_once('/')?;
        PartName::from_absolute(&format!("{parent}/_rels/{last}.rels"))
    }

    /// Resolves a relative reference against this part name (RFC 3986 §5.2).
    ///
    /// This is what a relationship `Target` and a markup `ImageSource` or
    /// `FontUri` each need, and milestone 1 measured both forms of each in one
    /// corpus.
    #[must_use]
    pub fn resolve(&self, reference: &str) -> Option<PartName> {
        PartName::from_absolute(&resolve_reference(&self.text, reference)?)
    }

    /// Whether this name is *derivable* from `other` under 6.2.2.3: `/a` is
    /// derivable from `/a/b`, and a package may not hold both.
    #[must_use]
    pub fn is_derivable_from(&self, other: &PartName) -> bool {
        other
            .folded
            .strip_prefix(&self.folded)
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// RFC 3986 §5.2, for the path-only references a package can hold.
///
/// A reference carrying a scheme, an authority or a query is **not** a part of
/// this package and answers `None` rather than being coerced into one — which
/// is also what makes an `http://` `ImageSource` a refusal rather than an
/// attempt at I/O this engine could not perform anyway.
#[must_use]
pub fn resolve_reference(base: &str, reference: &str) -> Option<String> {
    // A fragment is 9.1.7's face selector on a `FontUri` and is never part of
    // the name; a query has no meaning inside a package at all.
    let reference = reference.split('#').next().unwrap_or("");
    if reference.contains('?') || reference.starts_with("//") {
        return None;
    }
    // §4.2: a relative-path reference may not have a colon in its first
    // segment, because that is how a scheme is spelled.
    let first = reference.split('/').next().unwrap_or("");
    if first.contains(':') {
        return None;
    }

    let merged = if reference.starts_with('/') {
        reference.to_string()
    } else if reference.is_empty() {
        base.to_string()
    } else {
        // §5.2.3's merge: everything up to and including the base's last `/`.
        let cut = base.rfind('/').map_or(0, |at| at + 1);
        format!("{}{reference}", base.get(..cut).unwrap_or(PACKAGE_ROOT))
    };
    Some(remove_dot_segments(&merged))
}

/// RFC 3986 §5.2.4.
///
/// An empty segment other than the leading one is **kept** rather than
/// collapsed: `/a//b` is not a part name, and silently turning it into `/a/b`
/// would resolve a reference to a part the package never named.
fn remove_dot_segments(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for (at, segment) in path.split('/').enumerate() {
        match segment {
            "." => {}
            ".." => {
                out.pop();
            }
            "" if at == 0 && absolute => {}
            other => out.push(other),
        }
    }
    let mut text = String::new();
    if absolute {
        text.push('/');
    }
    text.push_str(&out.join("/"));
    text
}

/// 6.2.2.2: `part_name = 1*( "/" isegment-nz )`, with the three prohibitions.
fn valid_part_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('/') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    rest.split('/').all(valid_segment)
}

/// One `isegment-nz`, plus 6.2.2.2's prohibitions on what a segment may encode.
fn valid_segment(segment: &str) -> bool {
    // "A segment shall not end with a dot", and shall not be empty.
    if segment.is_empty() || segment.ends_with('.') {
        return false;
    }
    let mut chars = segment.chars();
    while let Some(c) = chars.next() {
        match c {
            '%' => {
                let (Some(hi), Some(lo)) = (
                    chars.next().and_then(hex_value),
                    chars.next().and_then(hex_value),
                ) else {
                    return false;
                };
                let octet = hi * 16 + lo;
                // "shall not contain a percent-encoded unreserved character",
                // and "shall not contain a percent-encoded forward slash or
                // backward slash".
                if is_unreserved(octet) || octet == b'/' || octet == b'\\' {
                    return false;
                }
            }
            // `pchar = unreserved / pct-encoded / sub-delims / ":" / "@"`.
            // This is what refuses `[Content_Types].xml`: `[` is a gen-delim
            // and belongs to no path segment, which is 7.3.7's whole point.
            c if c.is_ascii() => {
                if !is_pchar(c) {
                    return false;
                }
            }
            // RFC 3987's `ucschar`, admitted whole. A control character is not
            // one, and neither is anything a percent-decode produced by
            // accident.
            c => {
                if c.is_control() {
                    return false;
                }
            }
        }
    }
    true
}

fn is_pchar(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~!$&'()*+,;=:@".contains(c)
}

fn is_unreserved(octet: u8) -> bool {
    octet.is_ascii_alphanumeric() || matches!(octet, b'-' | b'.' | b'_' | b'~')
}

fn hex_value(c: char) -> Option<u8> {
    c.to_digit(16).and_then(|v| u8::try_from(v).ok())
}

/// RFC 3987 §3.2's URI-to-IRI conversion, restricted to a path.
///
/// Only the escapes that spell a character **outside** US-ASCII are decoded,
/// and only when the run of them is valid UTF-8. `%41` stays `%41`, which is
/// what makes [`PartName::item_name`] the exact inverse — and is separately
/// what lets 6.2.2.2's "no percent-encoded unreserved character" be checked at
/// all, since a decoder that turned `%41` into `A` would have destroyed the
/// evidence.
fn percent_decode_non_ascii(item: &str) -> String {
    let bytes = item.as_bytes();
    let mut out = String::with_capacity(item.len());
    let mut at = 0usize;
    while at < bytes.len() {
        let Some(octet) = escape_at(bytes, at) else {
            let Some(c) = item.get(at..).and_then(|rest| rest.chars().next()) else {
                break;
            };
            out.push(c);
            at += c.len_utf8();
            continue;
        };
        if octet < 0x80 {
            out.push_str(item.get(at..at + 3).unwrap_or(""));
            at += 3;
            continue;
        }
        let start = at;
        let mut raw: Vec<u8> = Vec::new();
        while let Some(octet) = escape_at(bytes, at) {
            if octet < 0x80 {
                break;
            }
            raw.push(octet);
            at += 3;
        }
        match String::from_utf8(raw) {
            Ok(text) => out.push_str(&text),
            // Not UTF-8, so it spells nothing: the escapes stay escapes, and
            // 6.2.2.2 will decide whether the name survives them.
            Err(_) => out.push_str(item.get(start..at).unwrap_or("")),
        }
    }
    out
}

/// The octet a `%XX` at `at` stands for, or `None` when there is not one there.
fn escape_at(bytes: &[u8], at: usize) -> Option<u8> {
    if bytes.get(at).copied()? != b'%' {
        return None;
    }
    let hi = hex_value(char::from(bytes.get(at + 1).copied()?))?;
    let lo = hex_value(char::from(bytes.get(at + 2).copied()?))?;
    Some(hi * 16 + lo)
}

// ---- Content types ----------------------------------------------------------

/// `[Content_Types].xml`, as 7.2.3.5 needs to read it.
#[derive(Clone, Debug, Default)]
pub struct ContentTypes {
    /// `(lower-cased extension, media type)`.
    defaults: Vec<(String, String)>,
    /// `(folded part name, media type)`.
    overrides: Vec<(String, String)>,
}

impl ContentTypes {
    /// Parses the content-types stream.
    ///
    /// # Errors
    /// [`PackageDefect::Unreadable`] for markup that is not well formed, is not
    /// in [`CONTENT_TYPES_NAMESPACE`], holds an element 7.2.3.2 does not define,
    /// or declares one extension or one part name twice — which that clause
    /// forbids and which a reader taking "whichever came first" would resolve
    /// by directory order.
    pub fn parse(bytes: &[u8], limits: &XmlLimits) -> Result<ContentTypes, PackageDefect> {
        let source = Source::new(bytes).map_err(|_| PackageDefect::Unreadable)?;
        let mut types = ContentTypes::default();
        let mut depth = 0usize;
        let mut root = false;
        for event in source.reader(limits) {
            let element = match event.map_err(|_| PackageDefect::Unreadable)? {
                Event::Start(element) => element,
                // An empty-element tag produces a `Start` and then an `End`, so
                // the two are counted against each other and a package with six
                // `Default`s is six elements at depth two rather than one nest
                // six deep. Forgetting this is the whole of gap 30 milestone
                // 3's first failing run.
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    continue;
                }
                _ => continue,
            };
            depth += 1;
            root |= depth == 1;
            if element.namespace() != Some(CONTENT_TYPES_NAMESPACE) {
                return Err(PackageDefect::Unreadable);
            }
            match (depth, element.local()) {
                (1, "Types") => {}
                (2, "Default") => {
                    let (Some(extension), Some(media)) = (
                        element.attribute(None, "Extension"),
                        element.attribute(None, "ContentType"),
                    ) else {
                        return Err(PackageDefect::Unreadable);
                    };
                    let folded = extension.to_ascii_lowercase();
                    if types.defaults.iter().any(|(e, _)| *e == folded) {
                        return Err(PackageDefect::Unreadable);
                    }
                    types.defaults.push((folded, media.to_string()));
                }
                (2, "Override") => {
                    let (Some(part), Some(media)) = (
                        element.attribute(None, "PartName"),
                        element.attribute(None, "ContentType"),
                    ) else {
                        return Err(PackageDefect::Unreadable);
                    };
                    let Some(name) = PartName::from_absolute(part) else {
                        return Err(PackageDefect::Unreadable);
                    };
                    if types.overrides.iter().any(|(p, _)| p == name.folded()) {
                        return Err(PackageDefect::Unreadable);
                    }
                    types
                        .overrides
                        .push((name.folded().to_string(), media.to_string()));
                }
                _ => return Err(PackageDefect::Unreadable),
            }
        }
        if !root {
            return Err(PackageDefect::Unreadable);
        }
        Ok(types)
    }

    /// 7.2.3.5's ordered algorithm, in its own order.
    ///
    /// 1. the part name against every `Override`, ASCII case-insensitively;
    /// 2. failing that, the extension against every `Default`, likewise;
    /// 3. failing that, the part has no media type — which is a real answer and
    ///    not a failure.
    #[must_use]
    pub fn media_type(&self, part: &PartName) -> Option<&str> {
        if let Some((_, media)) = self
            .overrides
            .iter()
            .find(|(name, _)| name == part.folded())
        {
            return Some(media);
        }
        let extension = part.extension()?.to_ascii_lowercase();
        self.defaults
            .iter()
            .find(|(e, _)| *e == extension)
            .map(|(_, media)| media.as_str())
    }
}

// ---- Relationships ----------------------------------------------------------

/// One `<Relationship>` of a `.rels` part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    /// The `Id` attribute, unique within its part (6.5.2).
    pub id: String,
    /// The `Type` attribute. **This is where the dialect is read from**: the
    /// content types are byte-identical across XPS 1.0 and OpenXPS, which
    /// milestone 1 measured, so the namespace here and on the markup's elements
    /// is the only discriminator there is.
    pub kind: String,
    /// The `Target` attribute, exactly as written — absolute or relative, and
    /// milestone 1's corpus carries both in one package.
    pub target: String,
    /// `TargetMode="External"`. Such a target is not a part of this package and
    /// is never resolved against it.
    pub external: bool,
}

/// Parses a `.rels` part.
///
/// # Errors
/// [`PackageDefect::Unreadable`] for markup that is not well formed, is not in
/// [`RELATIONSHIPS_NAMESPACE`], is missing one of the three required
/// attributes, or repeats an `Id`.
pub fn parse_relationships(
    bytes: &[u8],
    limits: &XmlLimits,
) -> Result<Vec<Relationship>, PackageDefect> {
    let source = Source::new(bytes).map_err(|_| PackageDefect::Unreadable)?;
    let mut out: Vec<Relationship> = Vec::new();
    let mut depth = 0usize;
    let mut root = false;
    for event in source.reader(limits) {
        let element = match event.map_err(|_| PackageDefect::Unreadable)? {
            Event::Start(element) => element,
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => continue,
        };
        depth += 1;
        root |= depth == 1;
        if element.namespace() != Some(RELATIONSHIPS_NAMESPACE) {
            return Err(PackageDefect::Unreadable);
        }
        match (depth, element.local()) {
            (1, "Relationships") => {}
            (2, "Relationship") => {
                let (Some(id), Some(kind), Some(target)) = (
                    element.attribute(None, "Id"),
                    element.attribute(None, "Type"),
                    element.attribute(None, "Target"),
                ) else {
                    return Err(PackageDefect::Unreadable);
                };
                if out.iter().any(|r| r.id == id) {
                    return Err(PackageDefect::Unreadable);
                }
                out.push(Relationship {
                    id: id.to_string(),
                    kind: kind.to_string(),
                    target: target.to_string(),
                    external: element
                        .attribute(None, "TargetMode")
                        .is_some_and(|mode| mode.eq_ignore_ascii_case("external")),
                });
            }
            _ => return Err(PackageDefect::Unreadable),
        }
    }
    if !root {
        return Err(PackageDefect::Unreadable);
    }
    Ok(out)
}

// ---- The package ------------------------------------------------------------

/// Why one part would not read.
///
/// Three variants rather than one, for gap 29 milestone 2's stated reason: a
/// part that is not in the package, a part the archive would not hand over and
/// a part whose markup is broken are three different sentences, and a test
/// asserting only "it failed" cannot tell a working check from a deleted one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PackageDefect {
    /// No part of that name — or the name was truncated by the archive reader,
    /// which makes it a different name from the one the package holds.
    Missing,
    /// The archive refused the entry: encrypted, a checksum failure, a
    /// truncation, a method OPC 7.3.6 forbids anyway.
    Entry(EntryError),
    /// The bytes are not the markup this part must hold.
    Unreadable,
}

impl core::fmt::Display for PackageDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PackageDefect::Missing => f.write_str("no part of that name"),
            PackageDefect::Entry(why) => write!(f, "the archive refused the entry: {why}"),
            PackageDefect::Unreadable => f.write_str("the part is not the markup it must be"),
        }
    }
}

/// Why a whole package is refused, before anything is read from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PackageProblem {
    /// An interleaved package (7.2.4, 7.3.7): items named `…/[0].piece`.
    /// Recognised and refused rather than half-assembled — reassembling them is
    /// a second addressing model layered on the first, and no package in
    /// milestone 1's corpus uses one.
    Interleaved,
    /// An item that is not a part name and is not the content-types item.
    InvalidPartName,
    /// 6.2.2.3: two names equal under ASCII case folding, or one derivable from
    /// another. Refused rather than resolved to whichever came first, because
    /// "whichever came first" is directory order.
    AmbiguousPartNames,
    /// More parts than the cap allows.
    TooManyParts,
}

/// What one archive entry turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A part, with its normalised name.
    Part(PartName),
    /// `[Content_Types].xml`: an item and never a part (7.3.7).
    ContentTypes,
    /// A directory record (APPNOTE 4.4.17.1). Holds no bytes and is no part.
    Directory,
    /// A piece of an interleaved part (7.2.4).
    Piece,
    /// A part whose name the archive reader **truncated**, so the name in hand
    /// is not the name the package holds. Unresolvable by construction: a
    /// lookup that matched it would be matching a name nobody wrote.
    Unresolvable,
    /// An item name that is not a part name at all.
    NotAPart,
}

/// A ZIP read as an OPC package: an index of parts, and a read-once cache.
pub struct Package<'a> {
    archive: Archive<'a>,
    items: Vec<Item>,
    /// One slot per archive entry. `Some` once the entry has been read, whether
    /// it produced bytes or a refusal — so a second resolution of the same part
    /// spends no more of `Archive`'s inflation budget, which
    /// `resolving_a_part_twice_does_not_inflate_it_twice` asserts through
    /// [`Archive::inflated`].
    cache: Vec<Option<Result<Cow<'a, [u8]>, EntryError>>>,
    /// `[Content_Types].xml`, parsed at most once. The outer `Option` is "not
    /// tried yet" and the inner is "tried and failed".
    types: Option<Result<ContentTypes, PackageDefect>>,
    /// One slot per archive entry, for the entries that are `.rels` parts
    /// (gap 30, milestone 4).
    ///
    /// Milestone 3 cached the *bytes* and not the markup, so asking a part for
    /// its relationships twice parsed twice — recorded there as a decision
    /// rather than left as an oversight, and closed here because the payload
    /// walk asks the same part more than once as soon as anything but the spine
    /// is read.
    ///
    /// **No cap of its own, and the argument is the one `limits.rs` asks
    /// for.** What is retained is a re-shaping of bytes the archive has already
    /// charged against `MAX_ZIP_INFLATED`: the shortest `<Relationship>` an OPC
    /// package can hold is about forty bytes and it becomes three `String`s and
    /// a `bool`, so the cache is a small constant times an already-bounded
    /// number. A constant over it could only ever fire after that one did,
    /// which is gap 18a milestone 8's failure written the other way round.
    relationships: Vec<Option<Result<Vec<Relationship>, PackageDefect>>>,
    /// How many times a part's markup has been parsed.
    ///
    /// The cache above has no other observable: the bytes are already cached,
    /// so `Archive::inflated` does not move whether the markup is parsed once
    /// or a thousand times, and a test written against it would assert
    /// `n == n` and pass with the cache deleted. That is gap 29's
    /// milestone-6 survivor exactly — a loop nothing checked was there — so the
    /// count is published for the same reason `Archive::inflated` is.
    parses: usize,
    xml: XmlLimits,
}

impl<'a> Package<'a> {
    /// Indexes an archive as a package. **Nothing is refused here**, on
    /// purpose.
    ///
    /// The whole point of milestone 3 is that a ZIP is not a format, and the
    /// discrimination has to run *before* any package rule is enforced —
    /// otherwise a comic archive with two entries of one name, or one carrying
    /// a `.piece` in its filename, would be refused as a broken package rather
    /// than paged as the comic it is. [`Package::validate`] is the strict half
    /// and runs only once E.3's three steps have said this is an XPS.
    ///
    /// `zip_name_len` is the archive reader's own [`tinker_pdf_zip::Limits`]
    /// name cap, and it is here for one reason: a truncated name has to be
    /// attributed to the entry it happened to, and `Warning::NameTruncated` is
    /// recorded once per **archive**. A name is truncated on a char boundary at
    /// or below the cap, so a truncated one is always within three bytes of it;
    /// when the archive reports the warning, every entry in that window is
    /// taken as [`Item::Unresolvable`]. That is conservative in the safe
    /// direction — an untruncated 1 024-byte name in an archive that truncated
    /// some *other* name becomes unresolvable too, and a part that resolves to
    /// nothing is visible where a part that resolves to the wrong bytes is not.
    #[must_use]
    pub fn open(archive: Archive<'a>, zip_name_len: usize, xml: XmlLimits) -> Package<'a> {
        let truncated = archive.warnings().contains(&ZipWarning::NameTruncated);
        let items: Vec<Item> = archive
            .entries()
            .iter()
            .map(|entry| classify(&entry.name, entry.is_directory(), truncated, zip_name_len))
            .collect();
        let cache = vec![None; items.len()];
        let relationships = vec![None; items.len()];
        Package {
            archive,
            items,
            cache,
            types: None,
            relationships,
            parses: 0,
            xml,
        }
    }

    /// The archive back, unread, for a caller that decided this is not a
    /// package after all.
    ///
    /// This is what makes `Document::open` open the archive **once**: the sniff
    /// and the read share one [`Archive`], so there is no window in which two
    /// reads of the same bytes could disagree and no second central directory
    /// walk.
    #[must_use]
    pub fn into_archive(self) -> Archive<'a> {
        self.archive
    }

    /// What each archive entry turned out to be, in the archive's own order.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The archive underneath, for a caller that wants its warnings or its
    /// spend.
    #[must_use]
    pub fn archive(&self) -> &Archive<'a> {
        &self.archive
    }

    /// How many parts the package holds. Not the entry count: the content-types
    /// item, directory records and unresolvable names are not parts.
    #[must_use]
    pub fn part_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, Item::Part(_)))
            .count()
    }

    /// Every part name, in the archive's own order.
    pub fn parts(&self) -> impl Iterator<Item = &PartName> {
        self.items.iter().filter_map(|item| match item {
            Item::Part(name) => Some(name),
            _ => None,
        })
    }

    /// The entry index of an item, by the name the archive spelled.
    #[must_use]
    pub fn item_index(&self, item: &str) -> Option<usize> {
        self.archive
            .entries()
            .iter()
            .position(|entry| entry.name == item)
    }

    /// The entry index of a part, by 6.2.2.3's equivalence.
    #[must_use]
    pub fn index_of(&self, name: &PartName) -> Option<usize> {
        self.items.iter().position(|item| match item {
            Item::Part(part) => part == name,
            _ => false,
        })
    }

    /// Whether a part of that name is in the package.
    #[must_use]
    pub fn has(&self, name: &PartName) -> bool {
        self.index_of(name).is_some()
    }

    /// Reads one entry, **once**.
    ///
    /// # Errors
    /// [`PackageDefect::Entry`] for whatever the archive said, or
    /// [`PackageDefect::Missing`] for an index the archive does not have.
    pub fn read(&mut self, index: usize) -> Result<&[u8], PackageDefect> {
        if !matches!(self.cache.get(index), Some(Some(_))) {
            let read = self.archive.read(index);
            if let Some(slot) = self.cache.get_mut(index) {
                *slot = Some(read);
            }
        }
        match self.cache.get(index) {
            Some(Some(Ok(data))) => Ok(data),
            Some(Some(Err(why))) => Err(PackageDefect::Entry(*why)),
            _ => Err(PackageDefect::Missing),
        }
    }

    /// Reads one part by name, once.
    ///
    /// # Errors
    /// [`PackageDefect::Missing`] when no part has that name, otherwise
    /// whatever [`Package::read`] said.
    pub fn read_part(&mut self, name: &PartName) -> Result<&[u8], PackageDefect> {
        let index = self.index_of(name).ok_or(PackageDefect::Missing)?;
        self.read(index)
    }

    /// The media type of a part, by 7.2.3.5.
    ///
    /// # Errors
    /// [`PackageDefect`] when `[Content_Types].xml` is absent or will not
    /// parse. `Ok(None)` is a different answer and a legitimate one: 7.2.3.5's
    /// third step is that a part may simply have no media type.
    pub fn media_type(&mut self, name: &PartName) -> Result<Option<&str>, PackageDefect> {
        self.parse_content_types()?;
        match &self.types {
            Some(Ok(types)) => Ok(types.media_type(name)),
            Some(Err(why)) => Err(*why),
            None => Err(PackageDefect::Missing),
        }
    }

    /// How many times this package has parsed a part's markup.
    ///
    /// Public because a cache nobody can measure is a cache nobody can check —
    /// `Archive::inflated`'s argument, one layer up. See the field's own
    /// comment for why the archive's budget cannot stand in for it.
    #[must_use]
    pub fn parses(&self) -> usize {
        self.parses
    }

    /// Parses `[Content_Types].xml` if it has not been parsed yet.
    fn parse_content_types(&mut self) -> Result<(), PackageDefect> {
        if let Some(done) = &self.types {
            return done.as_ref().map(|_| ()).map_err(|why| *why);
        }
        let xml = self.xml;
        let parsed = match self.item_index(CONTENT_TYPES_ITEM) {
            None => Err(PackageDefect::Missing),
            Some(index) => match self.read(index) {
                Ok(bytes) => ContentTypes::parse(bytes, &xml),
                Err(why) => Err(why),
            },
        };
        self.parses = self.parses.saturating_add(1);
        let answer = parsed.as_ref().map(|_| ()).map_err(|why| *why);
        self.types = Some(parsed);
        answer
    }

    /// The relationships of a part, or of the package itself, parsed **once**.
    ///
    /// `None` for the package relationships part `/_rels/.rels`; `Some(part)`
    /// for that part's derived `_rels` name. An **absent** relationships part
    /// is `Ok(vec![])` rather than an error: 6.5.2 does not require one, and
    /// most parts of a real package have none.
    ///
    /// The answer is cached against the entry it came from, so a second ask
    /// costs a clone rather than a parse — see the `relationships` field and
    /// [`Package::parses`], which is how a test can tell the two apart at all.
    ///
    /// # Errors
    /// [`PackageDefect`] when the part is there and will not read.
    pub fn relationships(
        &mut self,
        source: Option<&PartName>,
    ) -> Result<Vec<Relationship>, PackageDefect> {
        let name = match source {
            None => {
                PartName::from_absolute(PACKAGE_RELATIONSHIPS_PART).ok_or(PackageDefect::Missing)?
            }
            Some(part) => part.relationships_part().ok_or(PackageDefect::Missing)?,
        };
        let Some(index) = self.index_of(&name) else {
            return Ok(Vec::new());
        };
        if !matches!(self.relationships.get(index), Some(Some(_))) {
            let xml = self.xml;
            let parsed = match self.read(index) {
                Ok(bytes) => parse_relationships(bytes, &xml),
                Err(why) => Err(why),
            };
            self.parses = self.parses.saturating_add(1);
            if let Some(slot) = self.relationships.get_mut(index) {
                *slot = Some(parsed);
            }
        }
        match self.relationships.get(index) {
            Some(Some(Ok(found))) => Ok(found.clone()),
            Some(Some(Err(why))) => Err(*why),
            _ => Err(PackageDefect::Missing),
        }
    }

    /// The strict half of OPC, run **after** the format has been decided.
    ///
    /// # Errors
    /// [`PackageProblem`], one variant per rule, each of them by name.
    pub fn validate(&self, max_parts: usize) -> Result<(), PackageProblem> {
        if self.items.contains(&Item::Piece) {
            return Err(PackageProblem::Interleaved);
        }
        if self.items.contains(&Item::NotAPart) {
            return Err(PackageProblem::InvalidPartName);
        }
        if self.part_count() > max_parts {
            return Err(PackageProblem::TooManyParts);
        }

        // 6.2.2.3, both halves. A set rather than a pairwise sweep: 8 192 parts
        // is 33 million comparisons the other way, and the derivability check
        // walks each name's own segment boundaries rather than every other
        // name.
        let mut folded: HashSet<&str> = HashSet::with_capacity(self.items.len());
        for part in self.parts() {
            if !folded.insert(part.folded()) {
                return Err(PackageProblem::AmbiguousPartNames);
            }
        }
        for part in self.parts() {
            let name = part.folded();
            for (at, _) in name.match_indices('/').skip(1) {
                if folded.contains(name.get(..at).unwrap_or("")) {
                    return Err(PackageProblem::AmbiguousPartNames);
                }
            }
        }
        Ok(())
    }
}

/// What one archive entry is, before anything has been read from it.
fn classify(name: &str, directory: bool, truncated: bool, zip_name_len: usize) -> Item {
    if name == CONTENT_TYPES_ITEM {
        return Item::ContentTypes;
    }
    if directory {
        return Item::Directory;
    }
    if is_piece(name) {
        return Item::Piece;
    }
    // See `Package::open` for why a window rather than an exact answer.
    if truncated && name.len().saturating_add(3) >= zip_name_len {
        return Item::Unresolvable;
    }
    match PartName::from_item(name) {
        Some(part) => Item::Part(part),
        None => Item::NotAPart,
    }
}

/// 7.2.4's piece names: `…/[0].piece`, `…/[3].last.piece`.
fn is_piece(name: &str) -> bool {
    let last = name.rsplit('/').next().unwrap_or(name);
    last.starts_with('[') && last.ends_with(".piece")
}
