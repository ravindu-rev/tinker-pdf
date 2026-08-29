//! The font rule group: ISO 19005-1 6.3, ISO 19005-2/3 6.2.11 and
//! ISO 19005-4 6.2.10 (milestone 5 of `docs/design/pdfa.md`).
//!
//! # "Used for rendering" is the clause's own qualifier, and it is load-bearing
//!
//! Nearly every sentence in these clauses begins the same way: *for all fonts
//! **used for rendering** in a conforming file*. The first version of this
//! module read that as decoration and judged every dictionary in the
//! cross-reference table whose `/Type` was `/Font`. The corpus disagreed
//! within one run, in two different ways and both of them instructive:
//!
//! - an interactive form's `/DR` names `/Helvetica`, `/Helvetica-Bold` and
//!   `/ZapfDingbats` — three of the standard 14, none embedded, none drawn
//!   with — and the suite annotates those files `pass`. A default-resource
//!   font is a font a *future* appearance stream might use, not one this file
//!   renders;
//! - a fixture whose own title reads *"Type1 font that is used for rendering
//!   is not embedded; **the text rendering mode is 3**"* is annotated `pass`,
//!   and so is a CIDFont with no `/CIDToGIDMap` under the same qualifier.
//!   Mode 3 is 9.3.6's invisible mode: the glyphs are positioned and not
//!   painted, so nothing about the face reaches the page.
//!
//! So this group finds out what is drawn before it judges anything, and that
//! is what [`usage`] does: the page content streams, the form XObjects they
//! invoke and the annotation appearance streams they carry are tokenized, the
//! text rendering mode and the selected font are tracked through `q`/`Q`, and
//! a font enters the set the rules run over only when a text-showing operator
//! ran with it selected at a mode other than 3.
//!
//! # What this group reaches for
//!
//! Two machineries past the COS document, and both are counted as
//! [`super::RuleGroup::Fonts`]: `tinker-pdf-content`'s tokenizer, for the
//! usage scan above, and `tinker-pdf-font`, for the two rules that ask a
//! question only the font program can answer — whether the bytes behind
//! `/FontFile2` are an `sfnt` at all, and whether a `/FontFile3` marked
//! `/Type1C` really carries CFF. Both are the leaves the renderer already
//! trusts. A validator with a second font parser would be checking files
//! against a reading of the font formats nothing else in the engine shares.
//!
//! **Injection, counted.** Making rendering mode 3 count as rendering — one
//! comparison in [`Scan::stream`] — fails **2 of the workspace's 3 581 tests**:
//! `a_font_drawn_only_at_rendering_mode_three_is_not_used_for_rendering` and
//! `the_rendering_mode_is_restored_by_q_like_the_rest_of_the_graphics_state`.
//! It also puts two conforming corpus files back into the false-positive
//! column, which is the measurement the two tests stand in for.
//!
//! # What is staged, and the evidence for staging it
//!
//! The Unicode rule (6.3.8 / 6.2.11.7) is the sharpest limit here and
//! [`super::STAGED`] carries the reason: `6-3-8-t01-fail-b.pdf` and
//! `6-3-8-t01-pass-e.pdf` have **byte-identical font dictionaries** — the same
//! two Type 1 fonts, the same `/Encoding` dictionary, neither with a
//! `/ToUnicode` — and opposite annotations. Whatever separates them is not in
//! the font dictionary, so no rule over font dictionaries can find it. What
//! *is* determinable is checked; the rest is named rather than guessed.

use std::collections::BTreeSet;

use tinker_pdf_cos::{CosDocument, Dict, ObjRef, Object};

use super::content;

use super::{clauses, FindingKind, Flavour, Level, Machinery, Part, Raw, RuleGroup};

/// How many font programs are parsed in one document.
const MAX_PROGRAM_PARSES: usize = 4096;

/// The `/Subtype` values a `/FontFile3` stream may declare (ISO 32000-1 9.9
/// Table 126, as ISO 19005-1 6.3.4 admits them).
const FONT_FILE3_SUBTYPES: &[&[u8]] = &[b"Type1C", b"CIDFontType0C", b"OpenType"];

/// The two base encodings ISO 19005-1 6.3.7 admits on a non-symbolic TrueType
/// font.
const ADMITTED_ENCODINGS: &[&[u8]] = &[b"WinAnsiEncoding", b"MacRomanEncoding"];

/// The named encodings ISO 32000-1 Annex D defines, whose code-to-glyph-name
/// tables are published and whose glyph names are Unicode-mappable.
///
/// A font that names one of these has said what every code means without a
/// `/ToUnicode`, which is the exemption ISO 19005-1 6.3.8 turns on.
const PREDEFINED_ENCODINGS: &[&[u8]] = &[
    b"WinAnsiEncoding",
    b"MacRomanEncoding",
    b"MacExpertEncoding",
    b"StandardEncoding",
];

/// Runs every font rule that applies to `flavour`.
pub(super) fn rules(
    doc: &CosDocument,
    machinery: &Machinery,
    flavour: Option<Flavour>,
    out: &mut Vec<Raw>,
) {
    if !machinery.reach(RuleGroup::Fonts) {
        return;
    }
    let mut budget = MAX_PROGRAM_PARSES;
    for reference in usage(doc) {
        let Ok(object) = doc.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        font(doc, flavour, dict, reference, &mut budget, out);
    }
}

// ---- what the file actually draws with ------------------------------------

/// Every font a text-showing operator drew with at a visible rendering mode.
///
/// The walk itself is [`super::content`], shared with the colour group; this
/// is the visitor over it. Returned as an ordered set so the findings come out
/// in object-number order whatever order the pages happened to reach them in —
/// a verdict is compared by tests and read by people, and both want it stable.
fn usage(doc: &CosDocument) -> BTreeSet<ObjRef> {
    let mut rendered = BTreeSet::new();
    content::walk(doc, &mut |op| {
        if !matches!(op.operator, b"Tj" | b"TJ" | b"'" | b"\"") {
            return;
        }
        if op.mode == content::RENDER_MODE_INVISIBLE {
            return;
        }
        let (Some(name), Some(resources)) = (op.font, op.resources) else {
            return;
        };
        if let Some(reference) = content::lookup(doc, resources, b"Font", name) {
            rendered.insert(reference);
        }
    });
    rendered
}

// ---- the rules ------------------------------------------------------------

/// One font dictionary, dispatched on its `/Subtype`.
fn font(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    at: ObjRef,
    budget: &mut usize,
    out: &mut Vec<Raw>,
) {
    let subtype = name_of(doc, dict, b"Subtype").unwrap_or_default();
    subset_tag(doc, dict, at, out);
    unicode(doc, flavour, dict, &subtype, at, out);

    match subtype.as_slice() {
        b"Type0" => composite(doc, dict, at, budget, out),
        // 9.6.5: a Type 3 font's glyphs *are* content streams, so there is no
        // program to embed and no descriptor to embed it in. The embedding
        // clause has nothing to say about one, and a rule that reported every
        // Type 3 font as unembedded would be reporting the format rather than
        // a defect. Part 4 forbids Type 3 fonts outright; that is 6.2.10.2's
        // rule rather than this one's, and `super::STAGED` names it.
        b"Type3" => {}
        _ => simple(doc, flavour, dict, &subtype, at, budget, out),
    }
}

// ---- 6.3.4 / 6.2.11.4.1 Embedding -----------------------------------------

/// A simple font: Type 1, its multiple-master variant, or TrueType.
fn simple(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    subtype: &[u8],
    at: ObjRef,
    budget: &mut usize,
    out: &mut Vec<Raw>,
) {
    encoding(doc, flavour, dict, subtype, at, out);
    let descriptor = doc.resolve_key(dict, doc.intern(b"FontDescriptor"));
    let Some(descriptor) = descriptor.as_dict() else {
        // **The standard 14 land here, and that is the whole point of the
        // clause.** ISO 32000-1 9.6.2.2 lets a font dictionary for one of the
        // fourteen omit both the descriptor and the widths, because the
        // reader is expected to supply them. ISO 19005 has no such list: a
        // file whose appearance depends on a face the reader happens to own
        // is the thing the standard exists to prevent, so a font with no
        // descriptor has no embedded program and is reported as one.
        out.push(Raw {
            rule: clauses::FONT_EMBEDDING,
            object: Some(at),
            kind: FindingKind::FontNotEmbedded {
                subtype: String::from_utf8_lossy(subtype).into_owned(),
            },
        });
        return;
    };
    embedded_program(doc, descriptor, subtype, at, budget, out);
}

/// The descriptor's font-file keys, against the `/Subtype` that named it.
///
/// ISO 19005-1 6.3.4: every font used for rendering shall have an embedded
/// program, and the program shall be the kind the font dictionary says it is.
/// Three findings come out of that one sentence and they are different
/// problems: nothing is embedded at all, something is embedded under a key
/// this font subtype does not admit, or the right key holds bytes that are not
/// the format it names.
fn embedded_program(
    doc: &CosDocument,
    descriptor: &Dict,
    subtype: &[u8],
    at: ObjRef,
    budget: &mut usize,
    out: &mut Vec<Raw>,
) {
    let mut embedded = false;
    for key in [&b"FontFile"[..], b"FontFile2", b"FontFile3"] {
        // `get_ref` rather than `resolve_key`, and deliberately: a stream is
        // an indirect object by construction (7.3.8), so an entry that is not
        // a reference is not a program. One corpus fixture writes
        // `/FontFile3 null`, which is a key present and a program absent —
        // and reading that as "embedded" was the defect this line avoids.
        let Some(program_ref) = descriptor.get_ref(doc.intern(key)) else {
            continue;
        };
        let Some(program) = doc.get(program_ref).ok() else {
            continue;
        };
        let Some(stream) = program.as_stream() else {
            continue;
        };
        embedded = true;
        let program_subtype = name_of(doc, &stream.dict, b"Subtype").unwrap_or_default();

        // The stream's own `/Subtype` first, then the pairing. The order is
        // not arbitrary: a `/FontFile3 /Type42C` is wrong whatever font names
        // it, and reporting it as "a TrueType font may not use /FontFile3"
        // would name the font's fault rather than the stream's. Checked the
        // other way round, every bad stream subtype was reported as a bad
        // pairing and the finding pointed at the wrong object.
        if key == b"FontFile3" && !FONT_FILE3_SUBTYPES.contains(&program_subtype.as_slice()) {
            out.push(Raw {
                rule: clauses::FONT_EMBEDDING,
                object: Some(at),
                kind: FindingKind::FontProgramSubtypeMismatch {
                    key: "FontFile3".to_string(),
                    declared: String::from_utf8_lossy(&program_subtype).into_owned(),
                },
            });
            continue;
        }
        if !admits(subtype, key, &program_subtype) {
            out.push(Raw {
                rule: clauses::FONT_EMBEDDING,
                object: Some(at),
                kind: FindingKind::FontProgramSubtypeMismatch {
                    key: String::from_utf8_lossy(key).into_owned(),
                    declared: String::from_utf8_lossy(subtype).into_owned(),
                },
            });
            continue;
        }

        // The reach. Everything above this line is the object graph; this is
        // the font program, and it is half of why the group exists as a group.
        if *budget == 0 {
            continue;
        }
        *budget -= 1;
        let Ok(bytes) = doc.stream_decoded(program_ref) else {
            // A stream whose filter chain will not run is a defect the syntax
            // group and the reader's own warnings already speak about. Saying
            // it a third time under a font clause would be attributing a
            // filter's problem to a font.
            continue;
        };
        if !parses_as(key, &program_subtype, &bytes) {
            out.push(Raw {
                rule: clauses::FONT_EMBEDDING,
                object: Some(at),
                kind: FindingKind::FontProgramUnreadable {
                    key: String::from_utf8_lossy(key).into_owned(),
                },
            });
        }
    }

    if !embedded {
        out.push(Raw {
            rule: clauses::FONT_EMBEDDING,
            object: Some(at),
            kind: FindingKind::FontNotEmbedded {
                subtype: String::from_utf8_lossy(subtype).into_owned(),
            },
        });
    }
}

/// Whether a font of this `/Subtype` may carry its program under `key`.
///
/// ISO 32000-1 9.9 Table 126 pairs them: `/FontFile` is Type 1, `/FontFile2`
/// is TrueType, and `/FontFile3` is everything with its own `/Subtype`. A
/// `/FontFile3` marked `/OpenType` is admitted for every font kind, which is
/// Table 126's own note — an OpenType wrapper can hold either outline format
/// and the wrapper is what the key names.
fn admits(font_subtype: &[u8], key: &[u8], program_subtype: &[u8]) -> bool {
    if key == b"FontFile3" && program_subtype == b"OpenType" {
        return true;
    }
    match font_subtype {
        b"Type1" | b"MMType1" => key == b"FontFile" || program_subtype == b"Type1C",
        b"TrueType" | b"CIDFontType2" => key == b"FontFile2",
        b"CIDFontType0" => key == b"FontFile3" && program_subtype == b"CIDFontType0C",
        // A `/Subtype` this build does not know is not a font-file problem,
        // and reporting one would turn "we have not heard of this" into "the
        // file is wrong".
        _ => true,
    }
}

/// Whether the bytes are the format the key and the stream's `/Subtype` claim.
///
/// This is the only place in the validator where a document's bytes are handed
/// to a parser that could be slow, and every one of these returns `None`
/// rather than panicking on rubbish — they are the same entry points the
/// renderer uses on the same untrusted input (ruling 1).
fn parses_as(key: &[u8], program_subtype: &[u8], bytes: &[u8]) -> bool {
    match (key, program_subtype) {
        (b"FontFile", _) => tinker_pdf_font::Type1::parse(bytes).is_some(),
        (b"FontFile2", _) | (b"FontFile3", b"OpenType") => {
            tinker_pdf_font::Sfnt::parse(bytes).is_some()
        }
        (b"FontFile3", b"Type1C" | b"CIDFontType0C") => {
            tinker_pdf_font::Cff::parse(bytes).is_some()
        }
        // An unknown pairing has already been reported as one; parsing it
        // against a format nobody named would report the same defect twice.
        _ => true,
    }
}

// ---- 6.3.5 / 6.2.11.4.2 Font subsets --------------------------------------

/// The six-letter subset tag.
///
/// ISO 32000-1 9.6.4 gives the shape — six upper-case letters, a `+`, then the
/// font's own name — and ISO 19005-1 6.3.5 requires a subset to use it. Only
/// the *shape* is checked: whether two subsets of one face were given the same
/// tag is a document-wide question this rule does not ask.
///
/// **A reading.** A `/BaseFont` with no `+` in it is not a subset and is not
/// reported. A `+` anywhere in the name is taken as a subset tag's, which is
/// what makes `AB+Arial` a finding. The alternative reading — only a `+` at
/// index six counts — would pass it silently, and passing it silently is the
/// defect the clause is about.
fn subset_tag(doc: &CosDocument, dict: &Dict, at: ObjRef, out: &mut Vec<Raw>) {
    let Some(base) = name_of(doc, dict, b"BaseFont") else {
        return;
    };
    if !base.contains(&b'+') {
        return;
    }
    let shaped = base.len() > 7
        && base[6] == b'+'
        && base[..6].iter().all(u8::is_ascii_uppercase)
        && !base[7..].contains(&b'+');
    if !shaped {
        out.push(Raw {
            rule: clauses::FONT_SUBSETS,
            object: Some(at),
            kind: FindingKind::SubsetTagMalformed {
                declared: String::from_utf8_lossy(&base).into_owned(),
            },
        });
    }
}

// ---- 6.3.7 / 6.2.11.6 Character encodings ---------------------------------

/// The symbolic flag, ISO 32000-1 9.8.2 Table 123 bit position 3.
const FLAG_SYMBOLIC: i64 = 1 << 2;

/// ISO 19005-1 6.3.7, and its siblings.
///
/// The clause is about **TrueType** fonts and only TrueType fonts, which is
/// worth saying because the obvious generalisation — every simple font's
/// encoding must be one of the two — is not what it says and would report
/// every conforming Type 1 font in the corpus. A Type 1 font's encoding is
/// built into the program and 9.6.6.2 lets the dictionary override it by name
/// or by difference; the clause leaves that alone.
///
/// Two requirements, in opposite directions:
///
/// 1. a **symbolic** font's `/Encoding` shall not be present, because the
///    program's own `cmap` is the mapping and a dictionary entry would be a
///    second, disagreeing one;
/// 2. a **non-symbolic** font's `/Encoding` shall be present and shall be
///    `/WinAnsiEncoding` or `/MacRomanEncoding`.
///
/// **A reading, named as one.** Part 1 is read as forbidding `/Differences` on
/// a non-symbolic TrueType font and parts 2 to 4 as permitting it: parts 2 and
/// 3 moved the clause and softened its wording, and this build could not
/// establish from the text available that the prohibition survived the move.
/// The stricter half runs only where the reading is certain.
fn encoding(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    subtype: &[u8],
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    if subtype != b"TrueType" {
        return;
    }
    let flags = doc
        .resolve_key(dict, doc.intern(b"FontDescriptor"))
        .as_dict()
        .map_or(0, |descriptor| {
            doc.resolve_key(descriptor, doc.intern(b"Flags"))
                .as_int()
                .unwrap_or(0)
        });

    let value = doc.resolve_key(dict, doc.intern(b"Encoding"));
    if flags & FLAG_SYMBOLIC != 0 {
        if !value.is_null() {
            out.push(Raw {
                rule: clauses::FONT_ENCODINGS,
                object: Some(at),
                kind: FindingKind::SymbolicFontHasEncoding,
            });
        }
        return;
    }

    let (declared, differences) = match value.as_ref() {
        Object::Name(name) => (
            doc.name_bytes(*name)
                .map(|n| n.to_vec())
                .unwrap_or_default(),
            false,
        ),
        Object::Dict(encoding) => (
            name_of(doc, encoding, b"BaseEncoding").unwrap_or_default(),
            encoding.contains_key(doc.intern(b"Differences")),
        ),
        _ => (Vec::new(), false),
    };

    if !ADMITTED_ENCODINGS.contains(&declared.as_slice()) {
        out.push(Raw {
            rule: clauses::FONT_ENCODINGS,
            object: Some(at),
            kind: FindingKind::EncodingNotStandard {
                declared: String::from_utf8_lossy(&declared).into_owned(),
            },
        });
    } else if differences && flavour.map(|f| f.part) == Some(Part::One) {
        out.push(Raw {
            rule: clauses::FONT_ENCODINGS,
            object: Some(at),
            kind: FindingKind::EncodingDifferencesForbidden,
        });
    }
}

// ---- 6.3.8 / 6.2.11.7 Unicode character maps ------------------------------

/// The determinable half of ISO 19005-1 6.3.8.
///
/// The clause requires every glyph a level A or level U file renders to be
/// mappable to Unicode. `/ToUnicode` is one way to say so and not the only
/// one: a simple font naming one of Annex D's predefined encodings has already
/// said what every code means, and a composite font whose `/Encoding` names a
/// predefined CMap other than the two identity ones carries its mapping in the
/// character collection that CMap belongs to.
///
/// So this rule fires on a font that has **none** of the three, which is the
/// part of the clause a font dictionary can answer. The part it cannot is
/// staged, and [`super::STAGED`] carries the evidence: two fixtures with
/// identical font dictionaries and opposite annotations, differing only in
/// which glyph of a `/Differences` array the page draws.
///
/// **Part 4 is included, and that is a reading.** ISO 19005-4 has no level U —
/// it has E and F — and this build reads it as having absorbed the Unicode
/// requirement into the base part rather than dropped it, because the suite
/// files fixtures under `6.2.10.7 Unicode character maps` and annotates one of
/// them `pass`.
fn unicode(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    subtype: &[u8],
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    let required = matches!(
        flavour,
        Some(Flavour {
            part: Part::Four,
            ..
        }) | Some(Flavour {
            level: Some(Level::A | Level::U),
            ..
        })
    );
    if !required || dict.contains_key(doc.intern(b"ToUnicode")) {
        return;
    }
    let mapped = match subtype {
        // Two routes, and the corpus named the second one for us.
        //
        // The first is a predefined CMap that is not one of the two identity
        // ones: the identity CMaps map a code to itself and say nothing about
        // text, and a CID-keyed CFF program has no glyph names to fall back
        // on.
        //
        // The second is the **character collection** the descendant CIDFont
        // declares. Adobe publishes a `…-UCS2` CMap for each of the published
        // collections, so a CID in `Adobe-Japan1` has a Unicode value whether
        // or not the file spells it out — which is why three fixtures whose
        // own titles read *"Type 0 font whose descendant CIDFont uses the
        // Adobe-Korea1 character collection does not include a ToUnicode
        // entry"* are annotated `pass`. `Adobe-Identity` is not one of them:
        // it is the collection that means "these CIDs mean whatever this file
        // says they mean", and this file says nothing.
        b"Type0" => {
            name_of(doc, dict, b"Encoding")
                .is_some_and(|name| name != b"Identity-H" && name != b"Identity-V")
                || published_collection(doc, dict)
        }
        // A TrueType program maps codes to glyph *indices* through its own
        // `cmap`, and an index is not a character. So the dictionary is the
        // only place a TrueType font can say what its codes mean: a named
        // encoding from Annex D, or an encoding dictionary whose
        // `/Differences` are glyph names.
        b"TrueType" => match doc.resolve_key(dict, doc.intern(b"Encoding")).as_ref() {
            Object::Name(name) => doc
                .name_bytes(*name)
                .is_some_and(|n| PREDEFINED_ENCODINGS.contains(&n.as_ref())),
            Object::Dict(_) => true,
            _ => false,
        },
        // **Type 1, its multiple-master variant and Type 3 are exempt, and
        // that is a reading the corpus forced.** `6-3-8-t01-pass-e.pdf`
        // carries a symbolic Type 1 font with no `/Encoding` and no
        // `/ToUnicode` and is annotated `pass`; the first version of this rule
        // reported it. The reason it conforms is that a Type 1 program is
        // keyed by glyph *name*, and a glyph name is a route to Unicode
        // through the Adobe Glyph List. Whether a particular name is on that
        // list is the staged half of this clause — `super::STAGED` says so —
        // and the difference between the two fixtures that made this rule
        // wrong is exactly one such name.
        _ => true,
    };
    if !mapped {
        out.push(Raw {
            rule: clauses::FONT_UNICODE,
            object: Some(at),
            kind: FindingKind::ToUnicodeMissing,
        });
    }
}

/// Whether a Type 0 font's descendant declares one of the published Adobe
/// character collections.
///
/// The list is the set Adobe publishes a `…-UCS2` mapping for. It is data
/// about the registry rather than about any file, and `Adobe-Identity` is
/// deliberately absent: a CID in `Adobe-Identity` has no published meaning.
fn published_collection(doc: &CosDocument, dict: &Dict) -> bool {
    const COLLECTIONS: &[&[u8]] = &[b"Japan1", b"Japan2", b"GB1", b"CNS1", b"Korea1", b"KR"];
    let descendants = doc.resolve_key(dict, doc.intern(b"DescendantFonts"));
    let Some(first) = descendants.as_array().and_then(<[Object]>::first) else {
        return false;
    };
    let descendant = doc.resolve(first);
    let Some(descendant) = descendant.as_dict() else {
        return false;
    };
    let info = doc.resolve_key(descendant, doc.intern(b"CIDSystemInfo"));
    let Some(info) = info.as_dict() else {
        return false;
    };
    let registry = doc.resolve_key(info, doc.intern(b"Registry"));
    let ordering = doc.resolve_key(info, doc.intern(b"Ordering"));
    let (Some(registry), Some(ordering)) = (registry.as_string(), ordering.as_string()) else {
        return false;
    };
    registry.bytes.as_slice() == b"Adobe" && COLLECTIONS.contains(&ordering.bytes.as_slice())
}

// ---- 6.3.3.2 / 6.2.11.3.2 CIDFonts ----------------------------------------

/// A Type 0 font and the CIDFont under it.
///
/// ISO 32000-1 9.7.4 makes `/DescendantFonts` a one-element array, and every
/// rule below is about that element rather than about the Type 0 dictionary
/// that names it — which is why the findings carry the *descendant's* object
/// number where there is one.
fn composite(doc: &CosDocument, dict: &Dict, at: ObjRef, budget: &mut usize, out: &mut Vec<Raw>) {
    let descendants = doc.resolve_key(dict, doc.intern(b"DescendantFonts"));
    let Some(values) = descendants.as_array() else {
        return;
    };
    let Some(first) = values.first() else {
        return;
    };
    let reference = first.as_objref().unwrap_or(at);
    let resolved = doc.resolve(first);
    let Some(descendant) = resolved.as_dict() else {
        return;
    };
    let subtype = name_of(doc, descendant, b"Subtype").unwrap_or_default();

    cid_system_info(doc, descendant, reference, out);
    if subtype == b"CIDFontType2" {
        cid_to_gid(doc, descendant, reference, out);
    }
    subset_tag(doc, descendant, reference, out);

    let descriptor = doc.resolve_key(descendant, doc.intern(b"FontDescriptor"));
    match descriptor.as_dict() {
        Some(descriptor) => {
            embedded_program(doc, descriptor, &subtype, reference, budget, out);
        }
        None => out.push(Raw {
            rule: clauses::FONT_EMBEDDING,
            object: Some(reference),
            kind: FindingKind::FontNotEmbedded {
                subtype: String::from_utf8_lossy(&subtype).into_owned(),
            },
        }),
    }
}

/// ISO 32000-1 9.7.3 Table 114, as ISO 19005-1 6.3.3.2 requires it.
///
/// The three entries are checked by *type* and not only by presence, because a
/// `/Registry` that is a name rather than a string is the defect the clause is
/// about as much as a missing one is: a consumer looking the character
/// collection up in a registry has nothing to look up either way.
fn cid_system_info(doc: &CosDocument, descendant: &Dict, at: ObjRef, out: &mut Vec<Raw>) {
    let info = doc.resolve_key(descendant, doc.intern(b"CIDSystemInfo"));
    let Some(info) = info.as_dict() else {
        out.push(Raw {
            rule: clauses::CID_FONTS,
            object: Some(at),
            kind: FindingKind::CidSystemInfoIncomplete {
                key: "CIDSystemInfo".to_string(),
            },
        });
        return;
    };
    for (key, wants_string) in [
        (&b"Registry"[..], true),
        (b"Ordering", true),
        (b"Supplement", false),
    ] {
        let value = doc.resolve_key(info, doc.intern(key));
        let ok = if wants_string {
            value.as_string().is_some()
        } else {
            value.as_int().is_some()
        };
        if !ok {
            out.push(Raw {
                rule: clauses::CID_FONTS,
                object: Some(at),
                kind: FindingKind::CidSystemInfoIncomplete {
                    key: String::from_utf8_lossy(key).into_owned(),
                },
            });
        }
    }
}

/// ISO 19005-1 6.3.3.2's `/CIDToGIDMap` requirement.
///
/// ISO 32000-1 9.7.4.2 gives `/CIDToGIDMap` the default `/Identity` when it is
/// absent, so a CIDFontType2 without one is a legal PDF. ISO 19005-1 6.3.3.2
/// nonetheless requires the entry, and the corpus confirms the reading from
/// the other side: the fixture that omits it is annotated `pass` **only
/// because the font is drawn at text rendering mode 3**, and its own title
/// says so in those words. A font this scan sees drawn visibly has been drawn,
/// and the entry is required of it.
fn cid_to_gid(doc: &CosDocument, descendant: &Dict, at: ObjRef, out: &mut Vec<Raw>) {
    let value = doc.resolve_key(descendant, doc.intern(b"CIDToGIDMap"));
    let ok = match value.as_ref() {
        Object::Name(name) => doc
            .name_bytes(*name)
            .is_some_and(|n| n.as_ref() == b"Identity"),
        Object::Stream(_) => true,
        _ => false,
    };
    if ok {
        return;
    }
    let declared = match value.as_ref() {
        Object::Name(name) => doc
            .name_bytes(*name)
            .map(|n| String::from_utf8_lossy(&n).into_owned())
            .unwrap_or_default(),
        Object::Null => String::new(),
        other => format!("{other:?}"),
    };
    out.push(Raw {
        rule: clauses::CID_FONTS,
        object: Some(at),
        kind: FindingKind::CidToGidMapMalformed { declared },
    });
}

// ---- shared ---------------------------------------------------------------

/// A dictionary entry that is a name, as bytes.
fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    doc.resolve_key(dict, doc.intern(key))
        .as_name()
        .and_then(|name| doc.name_bytes(name))
        .map(|bytes| bytes.to_vec())
}
