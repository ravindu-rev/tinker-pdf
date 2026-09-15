//! Listing a document's fonts (9.5 to 9.9), behind [`Document::fonts`].
//!
//! [`Document::fonts`]: crate::Document::fonts
//!
//! # The projection boundary, stated once for the whole read surface
//!
//! This module, [`crate::layers`] and [`crate::annotations`] are the same
//! decision made three times, so the argument lives here and the other two
//! cite it.
//!
//! Ruling 11 makes this facade the only public surface, so every type a
//! caller can *reach* has to be one it can *name* from this crate. That is
//! satisfied two ways, and which one applies is a judgement per type rather
//! than a policy:
//!
//! * **Re-export** an internal type that is already the right shape and costs
//!   nothing to hold. [`FontKind`] and [`ProgramKey`] are `Copy` enums over
//!   9.6/9.7's families and 9.9's `/FontFile*` keys; a facade twin would be
//!   two enums that must be kept in step and one `match` that silently stops
//!   being exhaustive.
//! * **Own a new type** where the internal one is the wrong shape *or* where
//!   handing it out would force a cost the caller did not ask for.
//!   [`DocumentFont`] is owned for both reasons.
//!
//! ## Why `tinker_pdf_cos::Font` is not what `fonts()` returns
//!
//! `cos::font::Font` is the *interpreter's* reader. Holding one costs the
//! decoded `/ToUnicode` CMap, the encoding CMap, every `/Widths` entry and a
//! `OnceLock` for the inverted `/CIDToGIDMap`; building one runs
//! `cos::font::read`, which decodes CMap streams. Two things follow, and the
//! second is the decisive one:
//!
//! 1. A caller who wanted the *name* of each font would pay for every CMap in
//!    the document to get it.
//! 2. `cos::font::read` ends in `doc.absorb(sink)` — the leniencies it
//!    tolerates become entries on [`Document::warnings`]. Listing fonts would
//!    then *change what the document reports about itself*, so asking twice
//!    and asking once would give different answers. A read surface that
//!    mutates the thing it reads is not a read surface.
//!
//! [`Document::warnings`]: crate::Document::warnings
//!
//! So the listing reads only what a listing needs — `/Subtype`, `/BaseFont`,
//! and the descriptor's `/FontFile*` key — and never touches a CMap. The
//! cost of the second reader is a cross-check test
//! (`the_listing_and_the_interpreter_agree_about_kind_and_embedding`), and
//! that test is **self-consistency**: it pins two readers in this repository
//! against each other and adjudicates nothing about the standard. What the
//! standard adjudicates here is the subset tag's shape (9.6.4) and the
//! `/FontFile*` table (9.9 Table 126); what third-party data adjudicates is
//! the corpus, through `tests/annotation_census.rs`'s font half.
//!
//! ## The program bytes are lazy, and that is the whole reason for the method
//!
//! A font program is routinely a megabyte, and `/FontFile*` is a reference —
//! `cos::font::EmbeddedProgram`'s own doc comment already argues this for the
//! interpreter. [`DocumentFont`] keeps the same discipline for the facade: it
//! carries the *address* of the program (eight bytes) and decodes the stream
//! only in [`DocumentFont::program_bytes`]. A document with forty embedded
//! faces costs forty names to list and nothing else, which is what makes
//! `tpdf fonts` cheap on a corpus.

use std::collections::BTreeSet;
use std::sync::Arc;

use tinker_pdf_cos::{
    pages as cos_pages, CosDocument, Dict, FontKind, Name, ObjRef, Object, ProgramKey,
};

/// How deep the resource walk follows a nested `/Resources`.
///
/// Form XObjects nest, tiling patterns carry resources, and a Type 3 font's
/// `/Resources` can name another Type 3 font. Sixteen is past anything a
/// producer emits and bounded for anything that does not.
const MAX_RESOURCE_DEPTH: u32 = 16;

/// How many distinct fonts a listing returns before it stops.
///
/// A bound rather than a budget: ruling 1 says a hostile file must not be
/// able to make this allocate without limit, and a document with more than
/// this many distinct faces is not one a caller is listing by hand.
const MAX_FONTS: usize = 4096;

/// Where an embedded font program is, and which `/FontFile*` key named it
/// (9.9, Table 126).
///
/// A reference, never the bytes — see the module comment. The stream is read
/// by [`DocumentFont::program_bytes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontProgram {
    /// The stream object holding the program.
    pub stream: ObjRef,
    /// Which descriptor key pointed at it.
    pub key: ProgramKey,
}

/// One font a document's pages can reach.
///
/// Cheap to hold: a few strings and a reference. The program bytes are not in
/// here and are read by [`DocumentFont::program_bytes`] when a caller wants
/// them.
#[derive(Clone)]
pub struct DocumentFont {
    /// Kept so [`DocumentFont::program_bytes`] can decode the stream without
    /// the caller having to hand the document back. Shared, so this costs a
    /// refcount rather than a copy.
    doc: Arc<CosDocument>,
    /// The font dictionary's own reference, or `None` when the resource
    /// dictionary wrote it inline.
    ///
    /// 7.7.3.3 allows a direct dictionary and producers use it, so the
    /// listing does not pretend every font has an address.
    pub reference: Option<ObjRef>,
    /// Which of 9.6/9.7's families the dictionary belongs to.
    pub kind: FontKind,
    /// `/BaseFont` exactly as the file wrote it, subset tag and all.
    ///
    /// Empty for a Type 3 font, which 9.6.5 gives no `/BaseFont`.
    pub base_font: String,
    /// `/BaseFont` with the subset tag removed, which is the name a person
    /// means by "which font is this".
    pub name: String,
    /// The six upper-case letters of 9.6.4's subset tag, without the `+`.
    ///
    /// `None` when the name carries no tag, which is the ordinary case for a
    /// fully embedded or non-embedded face.
    pub subset_tag: Option<String>,
    /// The embedded program, or `None` when the document embeds none.
    pub program: Option<FontProgram>,
    /// The resource names this font is reachable under, sorted and
    /// deduplicated.
    ///
    /// A font is usually `/F1` on every page that uses it, and occasionally
    /// several names in one file. Sorted because ruling 4 makes iteration
    /// order a correctness question.
    pub resource_names: Vec<String>,
}

impl DocumentFont {
    /// Whether the document carries the font's program (9.9).
    ///
    /// This is exactly "the descriptor named a `/FontFile*` that resolves to
    /// a stream". It is **not** "the glyphs can be drawn": a program this
    /// build cannot parse is still embedded, and a standard-14 face with no
    /// program still draws. Conflating the two is the injection this method's
    /// test exists to catch.
    #[must_use]
    pub fn is_embedded(&self) -> bool {
        self.program.is_some()
    }

    /// The embedded program's bytes, decoded through its filter chain.
    ///
    /// `None` when nothing is embedded; `None` too when the stream will not
    /// decode, because a truncated program and an absent one are both "no
    /// bytes here" to a caller who wanted to write a file out. Which of the
    /// two it was is on [`crate::Document::warnings`] already.
    ///
    /// **This is where the cost is.** Every other field was read when the
    /// listing was built; this one reads and decompresses a stream each time
    /// it is called, and nothing caches it.
    #[must_use]
    pub fn program_bytes(&self) -> Option<Vec<u8>> {
        let program = self.program?;
        self.doc.stream_decoded(program.stream).ok()
    }
}

impl core::fmt::Debug for DocumentFont {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DocumentFont")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("subset_tag", &self.subset_tag)
            .field("embedded", &self.is_embedded())
            .finish()
    }
}

/// Splits 9.6.4's subset tag off a `/BaseFont` value.
///
/// 9.6.4 gives the shape exactly: six upper-case ASCII letters, a `+`, then
/// the font's own name. Anything else is not a tag and the whole string is
/// the name — `AB+Arial` keeps its `AB+`, because a listing that guessed
/// would report a font by a name no other tool uses.
///
/// This is the *reading* half of the shape `pdfa::fonts::subset_tag`
/// validates; that one reports a malformed tag as an ISO 19005 finding and
/// this one declines to strip it. The two agree about what a tag is, and a
/// test pins them to the same six strings.
fn split_subset_tag(base_font: &str) -> (Option<String>, String) {
    let bytes = base_font.as_bytes();
    let shaped = bytes.len() > 7
        && bytes[6] == b'+'
        && bytes[..6].iter().all(u8::is_ascii_uppercase)
        // A second `+` means the producer wrote something other than a tag,
        // and the rest is not a font name this code should invent.
        && !bytes[7..].contains(&b'+');
    if shaped {
        (Some(base_font[..6].to_string()), base_font[7..].to_string())
    } else {
        (None, base_font.to_string())
    }
}

/// How a font is identified for deduplication.
///
/// An indirect font is its reference; a direct one has no address, so it is
/// identified by what it says. Without the second case a page that writes its
/// fonts inline — which producers do — would report the same face once per
/// page it appears on.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Identity {
    At(ObjRef),
    Inline(String, u8, Option<(u32, u16)>),
}

fn identity(reference: Option<ObjRef>, font: &DocumentFont) -> Identity {
    match reference {
        Some(r) => Identity::At(r),
        None => Identity::Inline(
            font.base_font.clone(),
            match font.kind {
                FontKind::Type1 => 0,
                FontKind::TrueType => 1,
                FontKind::Type3 => 2,
                FontKind::Type0 => 3,
            },
            font.program.map(|p| (p.stream.num, p.stream.gen)),
        ),
    }
}

/// Reads one font dictionary into a listing entry.
///
/// Deliberately shallow: `/Subtype`, `/BaseFont`, and the descriptor. No
/// CMap is decoded and no warning is raised, which is the module comment's
/// whole point.
fn read_one(doc: &Arc<CosDocument>, dict: &Dict, reference: Option<ObjRef>) -> DocumentFont {
    let subtype = doc
        .resolve_key(dict, doc.intern(b"Subtype"))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
        .map(|b| b.to_vec())
        .unwrap_or_default();

    // The same four-way split `cos::font::read` makes, and it has to stay the
    // same four-way split: a listing that called a Type 0 font a Type 1 would
    // send a caller to the wrong descriptor.
    let kind = match subtype.as_slice() {
        b"Type0" => FontKind::Type0,
        b"TrueType" => FontKind::TrueType,
        b"Type3" => FontKind::Type3,
        _ => FontKind::Type1,
    };

    let base_font = doc
        .resolve_key(dict, doc.intern(b"BaseFont"))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let (subset_tag, name) = split_subset_tag(&base_font);

    DocumentFont {
        doc: Arc::clone(doc),
        reference,
        kind,
        base_font,
        name,
        subset_tag,
        program: program_of(doc, dict, kind),
        resource_names: Vec::new(),
    }
}

/// The descriptor's embedded program, following 9.7.4's descendant for a
/// composite font.
///
/// A Type 0 dictionary has no `/FontDescriptor` of its own — the descendant
/// CIDFont carries it — so a caller never has to know which of the two
/// dictionaries the listing started from.
fn program_of(doc: &CosDocument, dict: &Dict, kind: FontKind) -> Option<FontProgram> {
    let descriptor_holder = if kind == FontKind::Type0 {
        let descendants = doc.resolve_key(dict, doc.intern(b"DescendantFonts"));
        let first = descendants.as_array()?.first()?.clone();
        doc.resolve(&first).as_dict()?.clone()
    } else {
        dict.clone()
    };

    let descriptor = doc.resolve_key(&descriptor_holder, doc.intern(b"FontDescriptor"));
    let descriptor = descriptor.as_dict()?;

    // 9.9 Table 126, in the order a descriptor may carry them. At most one is
    // meant to be present; the first found wins, which is the same rule
    // `cos::font` applies.
    for (key, program) in [
        (&b"FontFile"[..], ProgramKey::FontFile),
        (&b"FontFile2"[..], ProgramKey::FontFile2),
        (&b"FontFile3"[..], ProgramKey::FontFile3),
    ] {
        let Some(stream) = descriptor.get_ref(doc.intern(key)) else {
            continue;
        };
        // **Embedded means there is a stream there.** A `/FontFile2` pointing
        // at a free object, a number, or a dictionary that is not a stream is
        // a descriptor claiming a program it does not have, and reporting it
        // as embedded is the defect `a_dangling_font_file_is_not_embedded`
        // pins.
        let Ok(object) = doc.get(stream) else {
            continue;
        };
        if object.as_stream().is_none() {
            continue;
        }
        return Some(FontProgram {
            stream,
            key: program,
        });
    }
    None
}

/// Every font the document's pages can reach, in page then resource-name
/// order.
///
/// # What "can reach" means
///
/// A page's `/Resources /Font`, and then every `/Resources` that page's
/// content can enter: a form XObject's (8.10.1), a tiling pattern's (8.7.3),
/// a Type 3 font's (9.6.5), and the normal appearance of each of its
/// annotations (12.5.5). That is the set an interpreter could bind, which is
/// the set a caller means.
///
/// **Descendant CIDFonts are not listed separately.** 9.7.4 makes the
/// descendant part of the Type 0 font rather than a font of its own, and a
/// listing that showed both would report every CJK face twice.
pub(crate) fn of_document(doc: &Arc<CosDocument>) -> Vec<DocumentFont> {
    let mut out: Vec<DocumentFont> = Vec::new();
    let mut seen: BTreeSet<Identity> = BTreeSet::new();
    let mut scopes: BTreeSet<ObjRef> = BTreeSet::new();

    for page in cos_pages::collect(doc) {
        if let Some(resources) = page.resources.clone() {
            walk(doc, &resources, 0, &mut scopes, &mut seen, &mut out);
        }
        annotation_resources(doc, &page, &mut scopes, &mut seen, &mut out);
    }
    // Last, so a font a page names keeps the page's own resource name at the
    // head of the list.
    form_default_resources(doc, &mut scopes, &mut seen, &mut out);
    out
}

/// The `/AcroForm /DR` dictionary (12.7.3.3).
///
/// **Found by the corpus census, not by reading the clause.** The font in
/// `verapdf/Isartor test files/PDFA-1b/6.9 Interactive Forms/isartor-6-9-t01-fail-a.pdf`
/// is `/LuciduxSans-Oblique`, embedded, and reachable from no page: the page's
/// `/Resources` is `<< /ProcSet … >>` and nothing else, and the face is named
/// only by the field's `/DA` string, whose names 12.7.3.3 resolves in `/DR`.
/// A listing that walked pages alone reported that document as carrying no
/// fonts at all, which is how the census's "the bytes name `/FontFile` and the
/// listing reaches none" line came to have seventeen entries in it.
///
/// It is the same *kind* of scope as an appearance stream's: a resource
/// dictionary an interpreter binds, reached by a route the content walk cannot
/// take.
///
/// Only the form's `/DR`, which is the one 12.7.3.3 defines. The Isartor file
/// repeats it on the field dictionary as well, and plenty of producers do, but
/// a per-field `/DR` is not a structure the clause gives meaning to — and the
/// fonts in one are the fonts in the form's, since a `/DA` that named anything
/// else would not resolve for the viewer that wrote it.
fn form_default_resources(
    doc: &Arc<CosDocument>,
    scopes: &mut BTreeSet<ObjRef>,
    seen: &mut BTreeSet<Identity>,
    out: &mut Vec<DocumentFont>,
) {
    let Some(catalog) = doc.catalog() else {
        return;
    };
    let form = doc.resolve_key(&catalog, doc.intern(b"AcroForm"));
    let Some(form) = form.as_dict() else {
        return;
    };
    let Some(resources) = doc.resolve_key(form, doc.intern(b"DR")).as_dict().cloned() else {
        return;
    };
    walk(doc, &resources, 0, scopes, seen, out);
}

/// The `/Resources` of each of a page's annotations' normal appearances.
///
/// An appearance stream is a form XObject reached by reference rather than by
/// a resource name (12.5.5), so the content walk never sees it — which is why
/// `annots::draw` pushes its resources explicitly and why the listing has to
/// reach them the same way. A signature's appearance is where a whole face
/// hides in plenty of real files.
fn annotation_resources(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    scopes: &mut BTreeSet<ObjRef>,
    seen: &mut BTreeSet<Identity>,
    out: &mut Vec<DocumentFont>,
) {
    let Ok(object) = doc.get(page.reference) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };
    let annots = doc.resolve_key(dict, doc.intern(b"Annots"));
    let Some(entries) = annots.as_array() else {
        return;
    };

    for entry in entries.iter().take(MAX_ANNOTS) {
        let resolved = doc.resolve(entry);
        let Some(annot) = resolved.as_dict() else {
            continue;
        };
        let ap = doc.resolve_key(annot, doc.intern(b"AP"));
        let Some(ap) = ap.as_dict() else {
            continue;
        };
        // Every state of every appearance, not just the one `/AS` selects: a
        // checkbox's off state is as embedded as its on state, and a listing
        // that followed `/AS` would report a different set of fonts depending
        // on what the form was filled in with.
        for (_, value) in ap.iter() {
            collect_appearance(doc, value, 0, scopes, seen, out);
        }
    }
}

/// How many `/Annots` entries the appearance walk examines per page.
const MAX_ANNOTS: usize = 4096;

fn collect_appearance(
    doc: &Arc<CosDocument>,
    value: &Object,
    depth: u32,
    scopes: &mut BTreeSet<ObjRef>,
    seen: &mut BTreeSet<Identity>,
    out: &mut Vec<DocumentFont>,
) {
    if depth > 2 {
        // `/AP` is at most `/N` → state → stream. Anything deeper is not a
        // shape 12.5.5 describes.
        return;
    }
    let resolved = doc.resolve(value);
    let Some(dict) = resolved.as_dict() else {
        return;
    };
    // A stream is the appearance itself; a plain dictionary here is the
    // dictionary of states a checkbox writes (12.5.5).
    if resolved.as_stream().is_some() {
        if let Some(resources) = doc.resolve_key(dict, Name::RESOURCES).as_dict().cloned() {
            walk(doc, &resources, 0, scopes, seen, out);
        }
        return;
    }
    for (_, nested) in dict.iter() {
        collect_appearance(doc, nested, depth + 1, scopes, seen, out);
    }
}

/// Adds every font in one `/Resources` dictionary, then follows the scopes it
/// opens.
fn walk(
    doc: &Arc<CosDocument>,
    resources: &Dict,
    depth: u32,
    scopes: &mut BTreeSet<ObjRef>,
    seen: &mut BTreeSet<Identity>,
    out: &mut Vec<DocumentFont>,
) {
    if depth > MAX_RESOURCE_DEPTH || out.len() >= MAX_FONTS {
        return;
    }

    let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
    if let Some(table) = fonts.as_dict() {
        for (key, entry) in table.iter() {
            if out.len() >= MAX_FONTS {
                return;
            }
            let reference = entry.as_objref();
            let resolved = doc.resolve(entry);
            let Some(dict) = resolved.as_dict() else {
                continue;
            };
            let label = doc
                .name_bytes(*key)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();

            let font = read_one(doc, dict, reference);
            let id = identity(reference, &font);
            match seen.contains(&id) {
                // Already listed: record the further name it answers to and
                // do not list it twice.
                true => {
                    for existing in out.iter_mut() {
                        if identity(existing.reference, existing) != id {
                            continue;
                        }
                        if !existing.resource_names.contains(&label) {
                            existing.resource_names.push(label);
                            existing.resource_names.sort();
                        }
                        break;
                    }
                }
                false => {
                    seen.insert(id);
                    let mut font = font;
                    font.resource_names.push(label);
                    // A Type 3 font's glyphs are content streams with
                    // resources of their own (9.6.5), and those resources may
                    // name further fonts.
                    if font.kind == FontKind::Type3 {
                        if let Some(nested) =
                            doc.resolve_key(dict, Name::RESOURCES).as_dict().cloned()
                        {
                            out.push(font);
                            walk(doc, &nested, depth + 1, scopes, seen, out);
                            continue;
                        }
                    }
                    out.push(font);
                }
            }
        }
    }

    for key in [&b"XObject"[..], &b"Pattern"[..]] {
        let table = doc.resolve_key(resources, doc.intern(key));
        let Some(table) = table.as_dict() else {
            continue;
        };
        for (_, entry) in table.iter() {
            // A scope is visited once however many names reach it, which is
            // both the cycle guard and what keeps a form used on every page
            // from being walked once per page.
            if let Some(reference) = entry.as_objref() {
                if !scopes.insert(reference) {
                    continue;
                }
            }
            let resolved = doc.resolve(entry);
            let Some(dict) = resolved.as_dict() else {
                continue;
            };
            let Some(nested) = doc.resolve_key(dict, Name::RESOURCES).as_dict().cloned() else {
                continue;
            };
            walk(doc, &nested, depth + 1, scopes, seen, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 9.6.4's shape, both directions, transcribed from the clause rather
    /// than from the code that reads it.
    ///
    /// Adjudicated by **ISO 32000-1 9.6.4**, which states the tag as six
    /// upper-case letters followed by `+`. Nothing here is a round trip.
    #[test]
    fn the_subset_tag_is_six_upper_case_letters_and_a_plus() {
        // Tagged.
        assert_eq!(
            split_subset_tag("ABCDEF+Arial"),
            (Some("ABCDEF".to_string()), "Arial".to_string())
        );
        assert_eq!(
            split_subset_tag("QWERTY+Times-Roman"),
            (Some("QWERTY".to_string()), "Times-Roman".to_string())
        );

        // Not tagged, each for its own reason.
        for untagged in [
            "Arial",            // no plus at all
            "AB+Arial",         // too few letters
            "ABCDEFG+Arial",    // too many before the plus
            "abcdef+Arial",     // not upper case
            "ABC1EF+Arial",     // not letters
            "ABCDEF+",          // nothing left to name
            "ABCDEF+GHIJKL+Ar", // a second plus: not a tag's shape
        ] {
            let (tag, name) = split_subset_tag(untagged);
            assert_eq!(tag, None, "{untagged} is not a subset tag");
            assert_eq!(name, untagged, "{untagged} keeps its whole name");
        }
    }

    /// The tag is *stripped* from the reported name, and the untouched
    /// `/BaseFont` is still there beside it.
    ///
    /// Both halves matter: a caller matching against a font on the host needs
    /// the stripped name, and a caller writing the file back out needs the
    /// exact bytes the file had.
    #[test]
    fn a_tagged_name_reports_both_spellings() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Arial >>\nendobj\n",
            "/Font << /F1 6 0 R >>",
        );
        let fonts = of_document(&doc);
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].name, "Arial");
        assert_eq!(fonts[0].base_font, "ABCDEF+Arial");
        assert_eq!(fonts[0].subset_tag.as_deref(), Some("ABCDEF"));
    }

    /// 9.9: a descriptor naming `/FontFile2` has an embedded program, and one
    /// naming none does not.
    #[test]
    fn a_descriptor_decides_whether_a_font_is_embedded() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Boxy \
             /FontDescriptor 7 0 R >>\nendobj\n\
             7 0 obj\n<< /Type /FontDescriptor /FontName /Boxy /FontFile2 8 0 R >>\nendobj\n\
             8 0 obj\n<< /Length 4 >>\nstream\nabcd\nendstream\nendobj\n\
             9 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
            "/Font << /F1 6 0 R /F2 9 0 R >>",
        );
        let fonts = of_document(&doc);
        assert_eq!(fonts.len(), 2);

        let boxy = fonts.iter().find(|f| f.name == "Boxy").expect("Boxy");
        assert!(boxy.is_embedded());
        assert_eq!(boxy.program.expect("a program").key, ProgramKey::FontFile2);
        assert_eq!(boxy.program_bytes().as_deref(), Some(&b"abcd"[..]));

        let helvetica = fonts
            .iter()
            .find(|f| f.name == "Helvetica")
            .expect("Helvetica");
        assert!(!helvetica.is_embedded());
        assert_eq!(helvetica.program_bytes(), None);
    }

    /// A `/FontFile2` pointing at something that is not a stream is a
    /// descriptor claiming a program it does not carry.
    ///
    /// Reporting it as embedded is worse than useless: a caller extracting
    /// every embedded face would get an entry whose bytes are always `None`,
    /// and a PDF/A pipeline would believe the file self-contained.
    #[test]
    fn a_dangling_font_file_is_not_embedded() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Boxy \
             /FontDescriptor 7 0 R >>\nendobj\n\
             7 0 obj\n<< /Type /FontDescriptor /FontName /Boxy /FontFile2 40 0 R >>\nendobj\n",
            "/Font << /F1 6 0 R >>",
        );
        let fonts = of_document(&doc);
        assert_eq!(fonts.len(), 1);
        assert!(
            !fonts[0].is_embedded(),
            "a /FontFile2 naming no stream is not an embedded program"
        );
    }

    /// 9.7.4: the descendant CIDFont carries the descriptor, and the listing
    /// reports **one** font, not two.
    #[test]
    fn a_composite_font_is_one_entry_with_its_descendants_program() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /GHIJKL+Song \
             /Encoding /Identity-H /DescendantFonts [7 0 R] >>\nendobj\n\
             7 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /GHIJKL+Song \
             /FontDescriptor 8 0 R >>\nendobj\n\
             8 0 obj\n<< /Type /FontDescriptor /FontName /GHIJKL+Song /FontFile2 9 0 R >>\n\
             endobj\n\
             9 0 obj\n<< /Length 2 >>\nstream\nhi\nendstream\nendobj\n",
            "/Font << /F1 6 0 R >>",
        );
        let fonts = of_document(&doc);
        assert_eq!(fonts.len(), 1, "the descendant is not a font of its own");
        assert_eq!(fonts[0].kind, FontKind::Type0);
        assert_eq!(fonts[0].name, "Song");
        assert!(fonts[0].is_embedded());
        assert_eq!(fonts[0].program_bytes().as_deref(), Some(&b"hi"[..]));
    }

    /// A font used on three pages under two names is one font with both
    /// names, not three entries.
    #[test]
    fn one_font_reached_many_ways_is_listed_once() {
        let doc = pages(
            "6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
            &[
                "/Font << /F1 6 0 R >>",
                "/Font << /F1 6 0 R >>",
                "/Font << /Zed 6 0 R >>",
            ],
        );
        let fonts = of_document(&doc);
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].resource_names, vec!["F1", "Zed"]);
    }

    /// A form XObject's own `/Resources` is a scope the content can enter
    /// (8.10.1), so the fonts in it are the page's fonts.
    #[test]
    fn a_form_xobjects_fonts_are_reached() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Inner >>\nendobj\n\
             7 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] \
             /Resources << /Font << /I1 6 0 R >> >> /Length 0 >>\nstream\n\nendstream\nendobj\n",
            "/XObject << /X1 7 0 R >>",
        );
        let fonts = of_document(&doc);
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].name, "Inner");
    }

    /// A form XObject naming itself must not walk forever (ruling 1).
    #[test]
    fn a_resource_cycle_terminates() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Loop >>\nendobj\n\
             7 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] \
             /Resources << /Font << /I1 6 0 R >> /XObject << /X1 7 0 R >> >> \
             /Length 0 >>\nstream\n\nendstream\nendobj\n",
            "/XObject << /X1 7 0 R >>",
        );
        let fonts = of_document(&doc);
        assert_eq!(fonts.len(), 1);
        assert_eq!(fonts[0].name, "Loop");
    }

    /// 12.5.5: an appearance stream is reached by reference, so the content
    /// walk never sees its resources and the listing has to.
    #[test]
    fn an_annotation_appearances_fonts_are_reached() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Signed >>\nendobj\n\
             7 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] \
             /Resources << /Font << /A1 6 0 R >> >> /Length 0 >>\nstream\n\nendstream\nendobj\n\
             8 0 obj\n<< /Type /Annot /Subtype /Widget /Rect [0 0 10 10] \
             /AP << /N 7 0 R >> >>\nendobj\n",
            "",
        );
        let fonts = of_document(&doc);
        assert_eq!(
            fonts.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            vec!["Signed"]
        );
    }

    /// 12.7.3.3: a variable-text field's `/DA` names its font in the form's
    /// `/DR`, which no page's `/Resources` has to mention.
    ///
    /// The fixture is the shape of
    /// `verapdf/Isartor test files/PDFA-1b/6.9 Interactive Forms/isartor-6-9-t01-fail-a.pdf`,
    /// which the corpus census found: one page whose `/Resources` is a
    /// `/ProcSet` and nothing else, one text field, and an embedded face
    /// reachable only through `/DR`. Before this the listing called that
    /// document fontless.
    #[test]
    fn a_form_default_resources_font_is_reached() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [8 0 R] \
/DA (/Lucidux 0 Tf) /DR << /Font << /Lucidux 6 0 R >> >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
/Resources << /ProcSet [/PDF /Text] >> /Annots [8 0 R] >>\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /LuciduxSans-Oblique \
/FontDescriptor 7 0 R >>\nendobj\n\
7 0 obj\n<< /Type /FontDescriptor /FontName /LuciduxSans-Oblique \
/FontFile 9 0 R >>\nendobj\n\
9 0 obj\n<< /Length 4 >>\nstream\npfb!\nendstream\nendobj\n\
8 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (field1) \
/DA (/Lucidux 0 Tf) /Rect [0 0 10 10] >>\nendobj\n\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n"
            .to_vec();
        let document = crate::Document::open(bytes).expect("it opens");
        let fonts = document.fonts();

        assert_eq!(fonts.len(), 1, "the /DR face, reachable from no page");
        assert_eq!(fonts[0].name, "LuciduxSans-Oblique");
        assert_eq!(fonts[0].resource_names, vec!["Lucidux"]);
        assert!(fonts[0].is_embedded(), "/FontFile names a stream");
        assert_eq!(fonts[0].program_bytes().as_deref(), Some(&b"pfb!"[..]));
    }

    /// **Self-consistency, named as such.** The listing's second reader and
    /// `cos::font::read` must agree about the two things the listing claims:
    /// which family the dictionary is, and whether a program is embedded.
    ///
    /// This adjudicates nothing about the standard — both readers are ours
    /// (ruling 13, and the "self round trips cannot adjudicate" rule). It
    /// exists because the listing deliberately does *not* call
    /// `cos::font::read`, and two readers that drift are worse than one slow
    /// one. What pins the behaviour to the specification is the clause-cited
    /// tests above and the corpus census.
    #[test]
    fn the_listing_and_the_interpreter_agree_about_kind_and_embedding() {
        let doc = document(
            "6 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Boxy \
             /FontDescriptor 7 0 R >>\nendobj\n\
             7 0 obj\n<< /Type /FontDescriptor /FontName /Boxy /FontFile2 8 0 R >>\nendobj\n\
             8 0 obj\n<< /Length 4 >>\nstream\nabcd\nendstream\nendobj\n\
             9 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /Song \
             /Encoding /Identity-H /DescendantFonts [10 0 R] >>\nendobj\n\
             10 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Song >>\nendobj\n\
             11 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
            "/Font << /F1 6 0 R /F2 9 0 R /F3 11 0 R >>",
        );

        for listed in of_document(&doc) {
            let reference = listed.reference.expect("every font here is indirect");
            let read = tinker_pdf_cos::font::at(&doc, reference).expect("the interpreter reads it");
            assert_eq!(listed.kind, read.kind(), "{} kind", listed.base_font);
            assert_eq!(
                listed.program.map(|p| (p.stream, p.key)),
                read.program().map(|p| (p.stream, p.key)),
                "{} program",
                listed.base_font
            );
            assert_eq!(listed.base_font, read.base_font());
        }
    }

    /// Listing fonts must not change what the document says about itself.
    ///
    /// `cos::font::read` absorbs its leniencies into the document's warnings.
    /// If `fonts()` were built on it, a caller who listed the fonts and then
    /// asked for the warnings would get a different answer from one who asked
    /// in the other order — which is the module comment's second argument,
    /// made executable.
    #[test]
    fn listing_fonts_adds_no_warnings() {
        // A `/ToUnicode` naming a missing object is exactly the leniency
        // `cos::font::read` would report.
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
/Resources << /Font << /F1 6 0 R >> >> >>\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 99 0 R >>\n\
endobj\n\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n"
            .to_vec();
        let doc = crate::Document::open(bytes).expect("it opens");

        let before = doc.warnings().len();
        let fonts = doc.fonts();
        assert_eq!(fonts.len(), 1);
        let after = doc.warnings().len();
        assert_eq!(
            before,
            after,
            "listing fonts added {} warnings to the document",
            after - before
        );
    }

    // ---- fixtures ---------------------------------------------------------

    /// A one-page document whose page carries `resources` and whose object
    /// numbers 6 upward are `extra`.
    fn document(extra: &str, resources: &str) -> Arc<CosDocument> {
        pages(extra, &[resources])
    }

    /// The same, with one page per entry of `resources`.
    fn pages(extra: &str, resources: &[&str]) -> Arc<CosDocument> {
        let kids: String = (0..resources.len())
            .map(|i| format!("{} 0 R ", i + 3))
            .collect();
        let bodies: String = resources
            .iter()
            .enumerate()
            .map(|(i, r)| {
                format!(
                    "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
                     /Resources << {r} >> /Annots [8 0 R] >>\nendobj\n",
                    i + 3
                )
            })
            .collect();
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count {} /Kids [{kids}] >>\nendobj\n\
{bodies}{extra}\
trailer\n<< /Size 60 /Root 1 0 R >>\n%%EOF\n",
            resources.len()
        );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        page.doc.clone()
    }
}
