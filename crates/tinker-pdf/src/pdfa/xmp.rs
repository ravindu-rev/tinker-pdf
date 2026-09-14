//! The XMP rule group: packet well-formedness and the document information
//! dictionary's agreement with it (milestone 3 of `docs/design/pdfa.md`).
//!
//! # Why this lives in the facade
//!
//! `tinker-pdf-cos` hands the packet over as bytes and refuses to parse it —
//! the `xmp_metadata` amendment in `crates/tinker-pdf-cos/src/outline.rs` says
//! why — and `tinker-pdf-xml` is a leaf crate with no PDF vocabulary in its
//! API (ruling 8). The facade is the one crate that depends on both, so it is
//! the only place the two halves of this rule can meet.
//!
//! # What is read, and what is not
//!
//! XMP is RDF/XML and a pull parser yields tokens rather than a graph. The
//! design doc's own risk table says so, and the answer it gives is the one
//! taken here: parse **only the property shapes ISO 19005 checks** — eight
//! properties in three namespaces — and treat a packet the subset cannot read
//! as a finding rather than as a pass.
//!
//! The largest metadata rule in the standard is here in **half**, and which
//! half is a decision rather than an accident. ISO 19005-1 6.7.2 and ISO
//! 19005-2 6.6.2.3 require every property in the packet to belong to a
//! predefined schema or be described by an extension schema, **and** to carry
//! the value type that schema declares.
//!
//! The value-type half runs, over the tables in [`super::xmp_schemas`] — one
//! per revision of the XMP specification, because the parts cite different
//! ones: part 1 the January 2004 revision, parts 2 and 3 the September 2005
//! one. The membership half is staged and [`super::STAGED`] names it: reading
//! the same tables as lists of *permitted* properties would report every
//! conforming file whose packet declares an extension schema, and ISO 19005-1
//! 6.7.8 is what says those files are conforming. `xmp_schemas.rs` argues
//! both halves at length.

use tinker_pdf_cos::{decode_text_string, parse_date, Date, Dict};
use tinker_pdf_xml::{Event, Name, Source};

use super::xmp_schemas::ValueForm;
use super::{clauses, FindingKind, Flavour, Machinery, Part, Raw, RuleGroup};

use crate::Document;

/// The Dublin Core namespace.
const DC: &str = "http://purl.org/dc/elements/1.1/";
/// The XMP basic namespace.
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
/// The Adobe PDF namespace.
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";

/// The RDF namespace, whose `rdf:li` carries an array member's text.
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// The XML namespace, which binds `xml:lang` whether a packet declares it or
/// not — XML 1.0 §2.12 makes the prefix reserved and pre-bound.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// One `/Info` entry and the XMP property ISO 19005 pairs it with.
///
/// ISO 19005-1 6.7.3 (parts 2 and 3: 6.1.5) requires that where the document
/// information dictionary carries one of these, the packet carries the
/// analogous property with an equivalent value. The pairing is the standard's,
/// not this build's; the equivalence is the part that needs a reading, and
/// [`Pairing::kind`] is where that reading lives.
struct Pairing {
    /// The `/Info` key.
    info: &'static [u8],
    /// The XMP property's namespace.
    namespace: &'static str,
    /// The conventional prefix, for packets that never bind the namespace.
    prefix: &'static str,
    /// The XMP property's local name.
    local: &'static str,
    /// How the two values are compared.
    kind: Compare,
}

/// How an `/Info` value and its XMP property are compared.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Compare {
    /// A simple value: the two strings match after trimming.
    Text,
    /// An `rdf:Alt` or `rdf:Seq`: the array must hold **exactly one** member
    /// and it must match.
    ///
    /// A reading, and named as one. The `/Info` entry is a single string, so
    /// an array of two cannot be equivalent to it — there is no single value
    /// for it to equal. A build that wanted to be lenient here would have to
    /// decide which member wins, and nothing in the clause says which.
    Array,
    /// A date: `D:YYYYMMDD…` on one side and ISO 8601 on the other, compared
    /// as instants rather than as strings.
    Instant,
}

/// The eight pairings ISO 19005 defines.
const PAIRINGS: &[Pairing] = &[
    Pairing {
        info: b"Title",
        namespace: DC,
        prefix: "dc",
        local: "title",
        kind: Compare::Array,
    },
    Pairing {
        info: b"Author",
        namespace: DC,
        prefix: "dc",
        local: "creator",
        kind: Compare::Array,
    },
    Pairing {
        info: b"Subject",
        namespace: DC,
        prefix: "dc",
        local: "description",
        kind: Compare::Array,
    },
    Pairing {
        info: b"Keywords",
        namespace: PDF,
        prefix: "pdf",
        local: "Keywords",
        kind: Compare::Text,
    },
    Pairing {
        info: b"Creator",
        namespace: XMP,
        prefix: "xmp",
        local: "CreatorTool",
        kind: Compare::Text,
    },
    Pairing {
        info: b"Producer",
        namespace: PDF,
        prefix: "pdf",
        local: "Producer",
        kind: Compare::Text,
    },
    Pairing {
        info: b"CreationDate",
        namespace: XMP,
        prefix: "xmp",
        local: "CreateDate",
        kind: Compare::Instant,
    },
    Pairing {
        info: b"ModDate",
        namespace: XMP,
        prefix: "xmp",
        local: "ModifyDate",
        kind: Compare::Instant,
    },
];

/// Runs the metadata group.
pub(super) fn rules(
    document: &Document,
    machinery: &Machinery,
    flavour: Option<Flavour>,
    out: &mut Vec<Raw>,
) {
    // The ask is recorded before anything is built, which is what makes the
    // laziness requirement a counted property rather than a comment.
    if !machinery.reach(RuleGroup::Metadata) {
        return;
    }
    let Some(packet) = document.xmp_metadata() else {
        // `flavour_of` already reported the absence.
        return;
    };

    let Some(properties) = properties(&packet) else {
        out.push(Raw::file(
            clauses::METADATA,
            FindingKind::MetadataUnreadable,
        ));
        return;
    };

    // The value-type half of 6.7.2 / 6.6.2.3, on the three parts that carry
    // the requirement. It runs before the `/Info` agreement below because that
    // half is part 1's alone and returns early.
    if let Some(part) = flavour.map(|f| f.part) {
        if part_carries_the_predefined_schema_rule(part) {
            schema_value_types(&packet, part, out);
        }
    }

    // **Part 1 only, and the restriction is a refusal rather than a gap.**
    //
    // ISO 19005-1 6.7.3 is unambiguous: where the document information
    // dictionary carries one of the eight entries, the analogous XMP property
    // shall carry an equivalent value. What this build could not establish
    // from the clause text is whether ISO 19005-2 kept that requirement when
    // it moved the subject from clause 6.7 (metadata) to clause 6.1 (file
    // structure), and PDF/A-4's 6.1.3 all but forbids the dictionary outright
    // rather than constraining its contents.
    //
    // Running the rule anyway on a 60/40 reading would report conforming
    // part 2 files as broken, and a validator that reports a conforming file
    // is worse than one that stays quiet. So it runs where the clause is
    // certain, and `super::STAGED` names where it does not and why.
    if flavour.map(|f| f.part) != Some(Part::One) {
        return;
    }

    let doc = &document.inner;
    let info_ref = doc.trailer().get_ref(doc.intern(b"Info"));
    let info = doc.resolve_key(doc.trailer(), doc.intern(b"Info"));
    let Some(info) = info.as_dict() else {
        return;
    };

    for pairing in PAIRINGS {
        let agreed = match info_entry(doc, info, pairing.info) {
            InfoEntry::Absent => continue,
            // An empty value is no claim to be consistent with.
            InfoEntry::Text(value) if value.is_empty() => continue,
            InfoEntry::Text(value) => agrees(
                &value,
                properties.get(pairing.namespace, pairing.prefix, pairing.local),
                pairing.kind,
            ),
            InfoEntry::NotAString => false,
        };
        if !agreed {
            out.push(Raw {
                rule: clauses::INFO_XMP,
                object: info_ref,
                kind: FindingKind::InfoXmpMismatch {
                    key: String::from_utf8_lossy(pairing.info).into_owned(),
                },
            });
        }
    }
}

/// An `/Info` entry, resolved through an indirect reference.
///
/// Three answers rather than two, because the clause distinguishes three
/// cases and a rule that collapsed them missed a corpus fixture for each:
/// the entry is absent and there is nothing to require, the entry is a string
/// and it must match, or **the entry is there and is not a string** — which
/// is a mismatch by itself, since a value that is not text cannot be
/// equivalent to an XMP property. One fixture's `/Title` is an indirect
/// reference to a font program, and reading that as "absent" passed a file
/// the clause fails.
///
/// The value is **not trimmed**. Padding a value with spaces makes it a
/// different value, and trimming it here hid a fixture whose `/Author` is
/// ` veraPDF Consortium ` against an XMP `veraPDF Consortium`.
enum InfoEntry {
    Absent,
    Text(String),
    NotAString,
}

fn info_entry(doc: &tinker_pdf_cos::CosDocument, info: &Dict, key: &[u8]) -> InfoEntry {
    let value = doc.resolve_key(info, doc.intern(key));
    if value.is_null() {
        return InfoEntry::Absent;
    }
    match value.as_string() {
        Some(string) => InfoEntry::Text(decode_text_string(&string.bytes)),
        None => InfoEntry::NotAString,
    }
}

/// Whether an `/Info` value and the XMP members found for it agree.
fn agrees(info: &str, found: Option<&[String]>, kind: Compare) -> bool {
    let Some(found) = found else {
        // The clause requires the property to be there when the `/Info` entry
        // is. An absent property is a mismatch, not a pass.
        return false;
    };
    match kind {
        // Compared as written on both sides. Padding a value with spaces
        // makes it a different value, and trimming here hid a fixture whose
        // `/Author` is ` veraPDF Consortium ` against an XMP
        // `veraPDF Consortium`. What whitespace an XMP element's content
        // really carries is decided in `normalise`, once, where the packet is
        // read — not here, where the difference would be silently forgiven.
        Compare::Text | Compare::Array => found.len() == 1 && found[0] == info,
        Compare::Instant => {
            found.len() == 1
                && match (parse_date(info), parse_date(&normalise_iso8601(&found[0]))) {
                    (Some(a), Some(b)) => same_instant(a, b),
                    _ => false,
                }
        }
    }
}

/// Rewrites an ISO 8601 timestamp into the digit run `parse_date` reads.
///
/// XMP writes `2015-03-10T17:19:21+01:00` and a PDF date string writes
/// `D:20150310171921+01'00'`. The two are the same instant spelled twice, so
/// the punctuation is removed and the zone is left where `parse_date` looks
/// for it. A truncated XMP date (`2015`, `2015-03`) survives: the parser fills
/// the missing components with the defaults ISO 32000-1 7.9.4 gives them, and
/// so does the standard.
fn normalise_iso8601(text: &str) -> String {
    let text = text.trim();
    let mut out = String::with_capacity(text.len() + 2);
    let mut zone = String::new();
    let mut in_zone = false;
    for (i, ch) in text.chars().enumerate() {
        match ch {
            '-' | '+' if i >= 8 => {
                // A sign this far in is the zone offset, not a date separator.
                in_zone = true;
                zone.push(ch);
            }
            'Z' => {
                in_zone = true;
                zone.push('Z');
            }
            _ if in_zone => {
                if ch.is_ascii_digit() {
                    zone.push(ch);
                }
            }
            c if c.is_ascii_digit() => out.push(c),
            // `-`, `:`, `T` and the fractional-second `.` are punctuation.
            _ => {}
        }
    }
    // `parse_date` wants the minutes at offset 18 (after an apostrophe) or 17.
    // Pad the digit run to fourteen so the zone lands where it looks.
    while out.len() < 14 {
        out.push('0');
    }
    out.truncate(14);
    out.push_str(&zone);
    out
}

/// Whether two dates name the same instant.
///
/// Where both carry a zone the comparison is of instants, so `12:00+01:00` and
/// `11:00Z` agree — they are the same moment written twice, and a rule that
/// called them different would report a producer's timezone as a conformance
/// defect. Where either omits its zone there is nothing to reconcile with, so
/// the fields are compared as written.
fn same_instant(a: Date, b: Date) -> bool {
    match (a.utc_offset_minutes, b.utc_offset_minutes) {
        (Some(oa), Some(ob)) => {
            minutes(a) - i64::from(oa) == minutes(b) - i64::from(ob) && a.second == b.second
        }
        _ => {
            a.year == b.year
                && a.month == b.month
                && a.day == b.day
                && a.hour == b.hour
                && a.minute == b.minute
                && a.second == b.second
        }
    }
}

/// Minutes since the civil epoch, by the proleptic Gregorian calendar.
///
/// Integer arithmetic only, so the answer is the same on every target
/// (ruling 4). Days-from-civil is the standard shift-the-year-to-March form:
/// with March as month zero the leap day lands at the end of the year and the
/// month-length pattern becomes an arithmetic progression.
fn minutes(date: Date) -> i64 {
    let y = i64::from(date.year) - i64::from(date.month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(date.month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(date.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 1440 + i64::from(date.hour) * 60 + i64::from(date.minute)
}

// ---- reading the packet ---------------------------------------------------

/// The properties this build reads out of a packet.
///
/// One slot per pairing, each holding the array members found for it. An empty
/// slot and a slot holding one empty string are different: the first says the
/// property was absent and the second says it was there and blank.
struct Properties {
    values: Vec<Option<Vec<String>>>,
}

impl Properties {
    fn get(&self, namespace: &str, prefix: &str, local: &str) -> Option<&[String]> {
        let index = PAIRINGS
            .iter()
            .position(|p| p.namespace == namespace && p.prefix == prefix && p.local == local)?;
        self.values.get(index)?.as_deref()
    }
}

/// An XMP element's text, with a pretty-printer's layout removed and nothing
/// else.
///
/// A packet written on one line means what it says: `<pdf:Producer>Acme 1.0 </`
/// declares a trailing space, and a reader that trimmed it would forgive a
/// `/Info` entry that does not carry one. A packet written across lines has
/// indentation that belongs to the file's layout rather than to the value, and
/// a reader that kept it would report every pretty-printed conforming file.
///
/// A newline is what tells the two apart. It is a heuristic and it is named as
/// one; the alternative is to pick one of the two failures and always make it.
fn normalise(text: &str) -> String {
    if text.contains('\n') || text.contains('\r') {
        text.trim().to_string()
    } else {
        text.to_string()
    }
}

/// Whether an XML name is the property `pairing` wants.
///
/// The prefix **or** the resolved namespace, for the reason milestone 1 gives
/// for `pdfaid`: a packet that binds the URI to an unconventional prefix is
/// legal RDF, and a packet that uses the conventional prefix without a visible
/// binding is what real files do. Requiring both would reject half the corpus;
/// requiring neither would let another standard's `title` through.
fn matches(name: &Name<'_>, pairing: &Pairing) -> bool {
    name.local() == pairing.local
        && (name.prefix() == Some(pairing.prefix) || name.namespace() == Some(pairing.namespace))
}

/// Whether an XML name is `rdf:li`.
fn is_rdf_li(name: &Name<'_>) -> bool {
    name.local() == "li" && (name.prefix() == Some("rdf") || name.namespace() == Some(RDF))
}

/// Reads the eight properties out of `packet`, or `None` if it will not parse.
fn properties(packet: &[u8]) -> Option<Properties> {
    // UTF-32 is a legal XMP encoding and not one XML 1.0 asks a reader to
    // handle; [`super::readable`] says why the transcode lives in the facade.
    let packet = super::readable(packet);
    let source = Source::new(&packet).ok()?;
    let limits = tinker_pdf_xml::Limits::default();
    let reader = source.reader(&limits);

    let mut values: Vec<Option<Vec<String>>> = vec![None; PAIRINGS.len()];
    // Which pairing is being collected, and the depth its element opened at.
    let mut capture: Option<(usize, usize)> = None;
    let mut items: Vec<String> = Vec::new();
    let mut buffer = String::new();
    let mut in_li = false;
    let mut depth: usize = 0;
    let mut well_formed = false;

    for event in reader {
        let Ok(event) = event else {
            // A packet that stops parsing part way has been read as far as it
            // goes. Whether that is a finding is the caller's to decide, and
            // it decides on `well_formed`: a packet that never produced a
            // single element is unreadable, and one that produced elements and
            // then broke has been read for what it said.
            return well_formed.then_some(Properties { values });
        };
        match event {
            Event::Start(element) => {
                well_formed = true;
                depth += 1;

                // The attribute form: `<rdf:Description dc:title="x"/>`.
                for attribute in element.attributes() {
                    for (index, pairing) in PAIRINGS.iter().enumerate() {
                        if matches(attribute.name(), pairing) && values[index].is_none() {
                            // Verbatim: an attribute value carries no layout
                            // whitespace to strip, so a space in it is part of
                            // the value the packet declares.
                            values[index] = Some(vec![attribute.value().to_string()]);
                        }
                    }
                }

                if capture.is_none() {
                    for (index, pairing) in PAIRINGS.iter().enumerate() {
                        if matches(element.name(), pairing) && values[index].is_none() {
                            capture = Some((index, depth));
                            items.clear();
                            buffer.clear();
                            in_li = false;
                            break;
                        }
                    }
                } else if is_rdf_li(element.name()) {
                    in_li = true;
                    buffer.clear();
                }
            }
            Event::Text(text) | Event::Cdata(text) => {
                if capture.is_some() {
                    buffer.push_str(&text);
                }
            }
            Event::End(name) => {
                if let Some((index, at)) = capture {
                    if in_li && is_rdf_li(&name) {
                        items.push(normalise(&buffer));
                        buffer.clear();
                        in_li = false;
                    } else if depth == at {
                        if items.is_empty() {
                            let text = normalise(&buffer);
                            if !text.is_empty() {
                                items.push(text);
                            }
                        }
                        values[index] = Some(core::mem::take(&mut items));
                        capture = None;
                        buffer.clear();
                    }
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    well_formed.then_some(Properties { values })
}

// ---- the predefined schemas' value types -----------------------------------
//
// ISO 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 have two halves. The membership
// half — every property belongs to a predefined schema or to an extension
// schema — is still staged, and `xmp_schemas.rs` records at length why: the
// tables now say which properties each cited revision defined, and what is
// left is the rule itself and the extension-schema exception 6.7.8 owes it.
//
// This is the other half. A property the cited revision's table *does* name is
// one that revision printed a value type for. A packet that writes
// `xmpDM:projectRef` as a string where the schema declares a structure has not
// written that property; it has written something else under its name, which
// is precisely what the clause exists to stop.
//
// Part 4 is excluded, and the exclusion is evidence rather than caution: the
// conformance suite has `PDF_A-1b/6.7 Metadata/6.7.2 Properties` and
// `PDF_A-2b/6.6 Metadata/6.6.2 Metadata streams/6.6.2.3 Schemas`, and there is
// no counterpart anywhere under `PDF_A-4` — ISO 19005-4 dropped the
// restriction rather than renumbering it.

/// A top-level property found in the packet, and the shape its value took.
struct Found {
    /// The resolved namespace URI.
    namespace: String,
    /// The local name.
    local: String,
    /// What the serialisation says the value is.
    form: ValueForm,
}

/// The property currently being read, and the depth its element opened at.
struct Pending {
    /// The resolved namespace URI.
    namespace: String,
    /// The local name.
    local: String,
    /// The path depth its start tag sat at.
    depth: usize,
    /// The form settled so far.
    form: ValueForm,
    /// Whether a child element has settled the form. The **first** direct
    /// child decides; a second cannot change it, because a property with two
    /// container children is malformed RDF rather than a different type.
    settled: bool,
}

/// Whether `name` is the RDF element `local`.
fn is_rdf(name: &Name<'_>, local: &str) -> bool {
    name.local() == local && (name.prefix() == Some("rdf") || name.namespace() == Some(RDF))
}

/// Whether a path entry is the RDF element `local`.
fn is_rdf_element(entry: Option<&(Option<String>, String)>, local: &str) -> bool {
    entry.is_some_and(|(namespace, name)| name == local && namespace.as_deref() == Some(RDF))
}

/// Whether the element about to open is a child of an `rdf:Description` that
/// is itself a child of `rdf:RDF`.
///
/// Matched on the path's **tail** rather than at a fixed depth, because
/// `x:xmpmeta` is optional: a packet inside `/Metadata` normally carries it
/// and a bare `<rdf:RDF>` is equally legal, so a rule keyed to depth two reads
/// every property of the wrapped form as a structure field and reports
/// nothing. That is exactly what the first build of this rule did, and the
/// corpus said so by not moving at all.
fn parent_is_a_top_level_description(path: &[(Option<String>, String)]) -> bool {
    let depth = path.len();
    depth >= 2
        && is_rdf_element(path.get(depth - 1), "Description")
        && is_rdf_element(path.get(depth - 2), "RDF")
}

/// Every top-level property in the packet, with the value form it was written
/// in.
///
/// "Top-level" is the standard's own level and is matched structurally: a
/// child of an `rdf:Description` that is itself a child of `rdf:RDF`. Matching
/// on `rdf:Description` alone would count a structure's *fields* as
/// properties, since a structure value is serialised as a nested
/// `rdf:Description` — and `stRef:instanceID` inside an `xmpMM:DerivedFrom`
/// would then be judged as though the document declared it.
fn top_level_properties(packet: &[u8]) -> Vec<Found> {
    let packet = super::readable(packet);
    let Ok(source) = Source::new(&packet) else {
        return Vec::new();
    };
    let limits = tinker_pdf_xml::Limits::default();
    let reader = source.reader(&limits);

    let mut found = Vec::new();
    // The element names on the way down. Only the two above the cursor are
    // ever consulted, but the whole path is kept because depth is what says
    // which two those are.
    let mut path: Vec<(Option<String>, String)> = Vec::new();
    let mut pending: Option<Pending> = None;
    // Path depth of the `rdf:Alt` whose items are being inspected for
    // `xml:lang`.
    let mut alt_at: Option<usize> = None;

    for event in reader {
        let Ok(event) = event else {
            // Read for what it said, exactly as `properties` does: a packet
            // that stops part way has already reported the properties it got
            // through, and `MetadataUnreadable` is the finding for one that
            // never started.
            break;
        };
        match event {
            Event::Start(element) => {
                let name = element.name();
                let depth = path.len();
                let namespace = name.namespace().map(str::to_string);
                let local = name.local().to_string();

                if let Some(open) = pending.as_mut() {
                    if depth == open.depth + 1 && !open.settled {
                        open.settled = true;
                        open.form = if is_rdf(name, "Seq") || is_rdf(name, "Bag") {
                            ValueForm::Array
                        } else if is_rdf(name, "Alt") {
                            alt_at = Some(depth);
                            // Provisional. An `rdf:Alt` is a language
                            // alternative only if its items say so, and no
                            // item has been seen yet.
                            ValueForm::Array
                        } else {
                            ValueForm::Structure
                        };
                    } else if alt_at == Some(depth - 1) && is_rdf_li(name) && has_xml_lang(&element)
                    {
                        open.form = ValueForm::LangAlt;
                    }
                } else if parent_is_a_top_level_description(&path)
                    && namespace.as_deref() != Some(RDF)
                {
                    // A property element. Its own attributes can settle the
                    // form before any child is seen: `rdf:parseType="Resource"`
                    // and the shorthand in which a structure's fields are
                    // written as attributes are both structures.
                    let resource = element.attributes().iter().any(|attribute| {
                        is_rdf(attribute.name(), "parseType") && attribute.value() == "Resource"
                    });
                    let shorthand = element
                        .attributes()
                        .iter()
                        .any(|attribute| is_a_field_of_a_structure(attribute.name()));
                    let structure = resource || shorthand;
                    pending = Some(Pending {
                        namespace: namespace.clone().unwrap_or_default(),
                        local: local.clone(),
                        depth,
                        form: if structure {
                            ValueForm::Structure
                        } else {
                            ValueForm::Simple
                        },
                        settled: structure,
                    });
                }

                // The attribute form of a property, which is written on the
                // `rdf:Description` itself: `<rdf:Description dc:format="…"/>`.
                // An attribute value is one string, so the form is simple by
                // construction.
                if local == "Description"
                    && namespace.as_deref() == Some(RDF)
                    && is_rdf_element(path.last(), "RDF")
                {
                    for attribute in element.attributes() {
                        if !is_a_field_of_a_structure(attribute.name()) {
                            continue;
                        }
                        let Some(ns) = attribute.name().namespace() else {
                            continue;
                        };
                        found.push(Found {
                            namespace: ns.to_string(),
                            local: attribute.name().local().to_string(),
                            form: ValueForm::Simple,
                        });
                    }
                }

                path.push((namespace, local));
            }
            Event::End(_) => {
                let depth = path.len().saturating_sub(1);
                if pending.as_ref().is_some_and(|open| open.depth == depth) {
                    let open = pending.take().expect("checked on the line above");
                    found.push(Found {
                        namespace: open.namespace,
                        local: open.local,
                        form: open.form,
                    });
                    alt_at = None;
                }
                path.pop();
            }
            _ => {}
        }
    }
    found
}

/// Whether an attribute is a *field of a structure* rather than a statement
/// about the element that carries it.
///
/// XMP lets a structure be written three ways, and the third is the one this
/// answers: `<xmpMM:DerivedFrom stRef:instanceID="…" stRef:documentID="…"/>`
/// is the same value as a nested `rdf:Description` with two children. So an
/// attribute that is a field makes its element a structure — and one that is
/// not must not, because reporting a conforming file is worse than staying
/// quiet.
///
/// Three exclusions, each for its own reason:
///
/// - **The RDF namespace**, excluded as a namespace rather than by listing
///   `parseType`, `resource`, `about`, `ID`, `nodeID` and `datatype`: every one
///   of them describes the *statement* and none is part of the value.
///   `rdf:resource` is the one worth naming — it makes the value a reference,
///   and counting it would turn every referenced simple value into a structure.
/// - **The XML namespace**, which is `xml:lang` and also `xml:base` and
///   `xml:space`. `has_xml_lang` reads `xml:lang` for a different question.
/// - **An attribute that resolves to no namespace at all** — an unprefixed one,
///   or one whose prefix was never declared. XMP writes every structure field
///   qualified, so neither can be one, and the second is a packet this reader
///   cannot name the namespace of anyway. This is the one exclusion that is
///   *not* symmetric with `is_rdf` and `has_xml_lang`, which fall back to the
///   conventional prefix when a declaration is missing: those two answer "is
///   this the RDF element I am looking for", where guessing costs a rule that
///   does not fire, and this one answers "is this element a structure", where
///   guessing costs a conforming file reported.
///
/// There is deliberately no `xmlns` exclusion: `tinker-pdf-xml` resolves
/// namespace declarations rather than reporting them, so they never reach an
/// `attributes()` list at all. Adding one would be a guard against nothing, and
/// reading this function without knowing that invites exactly that edit.
fn is_a_field_of_a_structure(name: &Name<'_>) -> bool {
    name.namespace()
        .is_some_and(|namespace| namespace != RDF && namespace != XML_NAMESPACE)
}

/// Whether an element carries `xml:lang`, which is what makes an `rdf:Alt` a
/// language alternative rather than an ordinary alternative array.
fn has_xml_lang(element: &tinker_pdf_xml::Element<'_>) -> bool {
    element.attributes().iter().any(|attribute| {
        attribute.name().local() == "lang"
            && (attribute.name().prefix() == Some("xml")
                || attribute.name().namespace() == Some(XML_NAMESPACE))
    })
}

/// Whether a part requires every property to belong to a predefined schema.
///
/// Parts 1, 2 and 3 do; **part 4 does not**, and that is a change in the
/// standard rather than a gap here. The evidence is the conformance suite's
/// own tree: `PDF_A-1b/6.7 Metadata/6.7.2 Properties` and `PDF_A-2b/6.6
/// Metadata/6.6.2 Metadata streams/6.6.2.3 Schemas` are directories full of
/// annotated fixtures, and there is no counterpart anywhere under `PDF_A-4` —
/// ISO 19005-4 dropped the restriction rather than renumbering it, and a build
/// that ran the rule there would be inventing a requirement.
fn part_carries_the_predefined_schema_rule(part: Part) -> bool {
    match part {
        Part::One | Part::Two | Part::Three => true,
        Part::Four => false,
    }
}

/// Reports every top-level property whose value form disagrees with the one
/// its predefined schema declares.
fn schema_value_types(packet: &[u8], part: Part, out: &mut Vec<Raw>) {
    for property in top_level_properties(packet) {
        let Some(declared) =
            super::xmp_schemas::value_form(&property.namespace, &property.local, part)
        else {
            // Not a property this table names, which is not a statement that
            // the property is unknown — only that nothing here can judge it.
            continue;
        };
        if declared == property.form {
            continue;
        }
        let named = match super::xmp_schemas::prefix_of(&property.namespace) {
            Some(prefix) => format!("{prefix}:{}", property.local),
            None => property.local.clone(),
        };
        out.push(Raw::file(
            // One table for every part. `ClauseTable::of` routes by part
            // already, and `SCHEMA_TYPES` carries part 1's own number in its
            // `one` arm — which is `METADATA`'s number too, because part 1
            // gives the whole of 6.7.2 one clause and no sub-clause.
            clauses::SCHEMA_TYPES,
            FindingKind::XmpValueTypeMismatch {
                property: named,
                expected: declared.describe(),
                found: property.form.describe(),
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::super::xmp_schemas;
    use super::*;

    fn read(packet: &str) -> Option<Properties> {
        properties(packet.as_bytes())
    }

    const HEAD: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:xmp="http://ns.adobe.com/xap/1.0/"
 xmlns:pdf="http://ns.adobe.com/pdf/1.3/"><rdf:Description rdf:about="">"#;
    const TAIL: &str = "</rdf:Description></rdf:RDF></x:xmpmeta>";
    /// [`HEAD`] with the `rdf:Description` still open, so a test can hang
    /// attributes on it.
    const HEAD_OPEN: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:dc="http://purl.org/dc/elements/1.1/"
 xmlns:xmp="http://ns.adobe.com/xap/1.0/"
 xmlns:pdf="http://ns.adobe.com/pdf/1.3/"><rdf:Description rdf:about="""#;

    #[test]
    fn an_alt_array_yields_its_one_member() {
        let packet = format!(
            "{HEAD}<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">A book\
             </rdf:li></rdf:Alt></dc:title>{TAIL}"
        );
        let properties = read(&packet).expect("parses");
        assert_eq!(
            properties.get(DC, "dc", "title"),
            Some(&["A book".to_string()][..])
        );
    }

    /// Two members cannot be equivalent to one `/Info` string, and the rule
    /// says so by counting rather than by picking one.
    #[test]
    fn a_seq_with_two_members_is_two_members() {
        let packet = format!(
            "{HEAD}<dc:creator><rdf:Seq><rdf:li>Ada</rdf:li><rdf:li>Grace\
             </rdf:li></rdf:Seq></dc:creator>{TAIL}"
        );
        let properties = read(&packet).expect("parses");
        let found = properties.get(DC, "dc", "creator").expect("present");
        assert_eq!(found.len(), 2);
        assert!(!agrees("Ada", Some(found), Compare::Array));
    }

    /// RDF admits the same statement as an attribute or as a child element,
    /// and real packets use both. A reader that handled one would miss the
    /// property in half the corpus and report a mismatch against `/Info`.
    #[test]
    fn the_attribute_form_and_the_element_form_say_the_same_thing() {
        let attribute = read(&format!(
            "{HEAD_OPEN} pdf:Producer=\"Acme 1.0\"/></rdf:RDF></x:xmpmeta>"
        ))
        .expect("parses");
        assert_eq!(
            attribute.get(PDF, "pdf", "Producer"),
            Some(&["Acme 1.0".to_string()][..])
        );

        let element = read(&format!(
            "{HEAD}<pdf:Producer>Acme 1.0</pdf:Producer>{TAIL}"
        ))
        .expect("parses");
        assert_eq!(
            element.get(PDF, "pdf", "Producer"),
            Some(&["Acme 1.0".to_string()][..])
        );
    }

    #[test]
    fn a_packet_that_will_not_parse_at_all_is_unreadable() {
        assert!(read("").is_none());
        assert!(properties(&[0xFF, 0xFE, 0x00]).is_none());
    }

    /// The same moment written two ways is one moment.
    #[test]
    fn a_zone_offset_is_reconciled_rather_than_compared_as_text() {
        assert!(agrees(
            "D:20150310171921+01'00'",
            Some(&["2015-03-10T16:19:21Z".to_string()]),
            Compare::Instant
        ));
        assert!(!agrees(
            "D:20150310171921+01'00'",
            Some(&["2015-03-10T17:19:21Z".to_string()]),
            Compare::Instant
        ));
    }

    #[test]
    fn an_absent_property_does_not_agree_with_a_present_info_entry() {
        assert!(!agrees("A book", None, Compare::Array));
        assert!(!agrees("Acme", None, Compare::Text));
    }

    /// Days-from-civil, against dates whose answers are fixed points.
    #[test]
    fn the_civil_calendar_is_integer_arithmetic() {
        let at = |year, month, day| Date {
            year,
            month,
            day,
            ..Date::default()
        };
        assert_eq!(minutes(at(1970, 1, 1)), 0);
        assert_eq!(minutes(at(1970, 1, 2)), 1440);
        assert_eq!(minutes(at(1969, 12, 31)), -1440);
        // 2000 was a leap year and 1900 was not, which is the whole of the
        // Gregorian correction and the part a naive formula gets wrong.
        assert_eq!(minutes(at(2000, 3, 1)) - minutes(at(2000, 2, 28)), 2 * 1440);
        assert_eq!(minutes(at(1900, 3, 1)) - minutes(at(1900, 2, 28)), 1440);
    }

    // ---- the predefined schemas' value types ------------------------------

    /// The packet these tests vary, with `body` dropped in as the properties.
    fn packet(body: &str) -> String {
        format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
             {body}\
             </rdf:RDF></x:xmpmeta>"
        )
    }

    /// The findings the value-type rule makes about `body`, under `part`.
    fn types(body: &str, part: Part) -> Vec<String> {
        let mut out = Vec::new();
        schema_value_types(packet(body).as_bytes(), part, &mut out);
        out.into_iter()
            .map(|raw| match raw.kind {
                FindingKind::XmpValueTypeMismatch {
                    property,
                    expected,
                    found,
                } => format!("{property}: {expected} declared, {found} written"),
                other => format!("unexpected {other:?}"),
            })
            .collect()
    }

    /// A structure written as a string is the shape the corpus's largest
    /// family is made of, and it is what the rule exists to catch.
    ///
    /// `xmpDM:projectRef` is a `ProjectLink`. Written with
    /// `rdf:parseType="Resource"` it is that structure; written with text
    /// between its tags it is a different property wearing the name, which is
    /// the thing ISO 19005-1 6.7.2 forbids.
    #[test]
    fn a_structure_written_as_a_string_is_reported_and_the_structure_is_not() {
        const NS: &str = "xmlns:xmpDM=\"http://ns.adobe.com/xmp/1.0/DynamicMedia/\"";
        let wrong = format!(
            "<rdf:Description {NS} rdf:about=\"\">\
             <xmpDM:projectRef>http://example.invalid/</xmpDM:projectRef>\
             </rdf:Description>"
        );
        assert_eq!(
            types(&wrong, Part::Two),
            ["xmpDM:projectRef: a structure declared, a simple value written"],
        );

        let right = format!(
            "<rdf:Description {NS} rdf:about=\"\">\
             <xmpDM:projectRef rdf:parseType=\"Resource\">\
             <xmpDM:type>custom</xmpDM:type>\
             </xmpDM:projectRef></rdf:Description>"
        );
        assert!(types(&right, Part::Two).is_empty(), "{right}");
    }

    /// The attribute shorthand for a structure is a structure.
    ///
    /// `<xmpDM:projectRef xmpDM:type="custom"/>` serialises the same value as
    /// the `parseType` form. A rule that only knew `parseType` would report a
    /// conforming file, which is the failure mode this whole rule group is
    /// held to avoid.
    #[test]
    fn the_attribute_shorthand_for_a_structure_is_a_structure() {
        let body = "<rdf:Description \
                    xmlns:xmpDM=\"http://ns.adobe.com/xmp/1.0/DynamicMedia/\" rdf:about=\"\">\
                    <xmpDM:projectRef xmpDM:type=\"custom\"/></rdf:Description>";
        assert!(types(body, Part::Two).is_empty());
    }

    /// A language alternative is an `rdf:Alt` **whose items carry
    /// `xml:lang`**, and the corpus is what settled that it is strict.
    ///
    /// Reading a bare `rdf:Alt` as satisfying a Lang Alt agrees with ten fewer
    /// fixtures across seven suites and gains no conforming file, so the
    /// leniency was measured and declined rather than assumed.
    #[test]
    fn a_language_alternative_needs_a_language_on_its_items() {
        const NS: &str = "xmlns:dc=\"http://purl.org/dc/elements/1.1/\"";
        let bare = format!(
            "<rdf:Description {NS} rdf:about=\"\"><dc:title><rdf:Alt>\
             <rdf:li>A title</rdf:li></rdf:Alt></dc:title></rdf:Description>"
        );
        assert_eq!(
            types(&bare, Part::One),
            ["dc:title: a language alternative declared, an array written"],
        );

        let tagged = format!(
            "<rdf:Description {NS} rdf:about=\"\"><dc:title><rdf:Alt>\
             <rdf:li xml:lang=\"x-default\">A title</rdf:li></rdf:Alt></dc:title>\
             </rdf:Description>"
        );
        assert!(types(&tagged, Part::One).is_empty(), "{tagged}");
    }

    /// A structure's **fields** are not top-level properties.
    ///
    /// `stRef:instanceID` inside an `xmpMM:DerivedFrom` is a field of a
    /// `ResourceRef`, not a property of the document. A walk that matched on
    /// `rdf:Description` alone would judge it as one — and a structure value
    /// is serialised as a nested `rdf:Description`, so that walk would judge
    /// every field of every structure in the packet.
    #[test]
    fn a_structures_fields_are_not_judged_as_properties() {
        let body = "<rdf:Description \
                    xmlns:xmpMM=\"http://ns.adobe.com/xap/1.0/mm/\" \
                    xmlns:stRef=\"http://ns.adobe.com/xap/1.0/sType/ResourceRef#\" \
                    rdf:about=\"\"><xmpMM:DerivedFrom rdf:parseType=\"Resource\">\
                    <stRef:instanceID>uuid:1</stRef:instanceID>\
                    <stRef:documentID>uuid:2</stRef:documentID>\
                    </xmpMM:DerivedFrom></rdf:Description>";
        assert!(types(body, Part::One).is_empty());
        let found = top_level_properties(packet(body).as_bytes());
        assert_eq!(found.len(), 1, "only the one property is top-level");
        assert_eq!(found[0].local, "DerivedFrom");
    }

    /// A property the cited revision's table does not name is **not**
    /// reported.
    ///
    /// This is the membership half staying staged, asserted rather than
    /// described. `pdf:Trapped` is the sharp case: the veraPDF suite has a
    /// fixture saying in as many words that it is "not permitted in Adobe PDF
    /// Schema in XMP 2004", and the string "Trapped" appears in neither
    /// revision — so a membership rule would report it and the value-type rule
    /// says nothing about it at all.
    #[test]
    fn a_property_this_table_does_not_name_is_not_a_finding() {
        for body in [
            "<rdf:Description xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\" rdf:about=\"\">\
             <pdf:Trapped>False</pdf:Trapped></rdf:Description>",
            "<rdf:Description xmlns:zz=\"http://example.invalid/ns/\" rdf:about=\"\">\
             <zz:whatever>x</zz:whatever></rdf:Description>",
        ] {
            assert!(types(body, Part::One).is_empty(), "{body}");
        }
    }

    /// The packet's wrapper is optional, and a rule keyed to depth misses
    /// every property when it is present.
    ///
    /// `x:xmpmeta` is what a `/Metadata` stream normally carries and a bare
    /// `<rdf:RDF>` is equally legal. The first build of this rule matched a
    /// property at a fixed depth of two, which is the bare form's depth, and
    /// the whole corpus moved by **zero files** because every real packet has
    /// the wrapper. Both are asserted so it cannot happen the other way round.
    #[test]
    fn the_xmpmeta_wrapper_is_optional() {
        const INNER: &str = "<rdf:Description \
                             xmlns:xmpDM=\"http://ns.adobe.com/xmp/1.0/DynamicMedia/\" \
                             rdf:about=\"\"><xmpDM:projectRef>x</xmpDM:projectRef>\
                             </rdf:Description>";
        let wrapped = packet(INNER);
        let bare = format!(
            "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
             {INNER}</rdf:RDF>"
        );
        for form in [wrapped.as_str(), bare.as_str()] {
            let found = top_level_properties(form.as_bytes());
            assert_eq!(found.len(), 1, "{form}");
            assert_eq!(found[0].form, ValueForm::Simple);
        }
    }

    /// Part 4 dropped the requirement, so the rule does not run there — and
    /// the evidence is the conformance suite's own directory tree, which has
    /// `6.7.2 Properties` under part 1 and `6.6.2.3 Schemas` under part 2 and
    /// nothing of the kind anywhere under `PDF_A-4`.
    ///
    /// Asserted on the predicate rather than on the walk, because the part is
    /// decided before the walk is reached and handing the walk a `Part::Four`
    /// would assert the opposite of the thing that matters.
    #[test]
    fn part_four_is_not_judged_on_predefined_schemas() {
        assert!(!part_carries_the_predefined_schema_rule(Part::Four));
        for part in [Part::One, Part::Two, Part::Three] {
            assert!(part_carries_the_predefined_schema_rule(part), "{part:?}");
        }
        // And a part that does carry it reports, so the line above is a
        // restriction rather than a rule that never fires anywhere.
        let body = "<rdf:Description \
                    xmlns:xmpDM=\"http://ns.adobe.com/xmp/1.0/DynamicMedia/\" rdf:about=\"\">\
                    <xmpDM:projectRef>x</xmpDM:projectRef></rdf:Description>";
        assert_eq!(types(body, Part::Two).len(), 1);
    }

    /// And the table router gives part 4 **nothing**, rather than a revision
    /// part 4 does not cite.
    ///
    /// This exists because of a measured zero. The walk above never asks
    /// `value_form` for part 4, so routing part 4 to the September 2005 table
    /// failed **no test at all** when it was injected: the arm was a sentence
    /// in a comment and not a claim anything held. ISO 19005-4 is drafted
    /// against ISO 16684-1, which neither table transcribes, so `None` — "no
    /// table here can judge this" — is the only honest answer, and it is also
    /// the only one that cannot report a conforming file if some later rule
    /// does reach here.
    #[test]
    fn the_table_router_gives_part_four_nothing() {
        // `dc:title` is in both revisions, so this is the router answering and
        // not a name neither table carries.
        const DC: &str = "http://purl.org/dc/elements/1.1/";
        assert_eq!(
            xmp_schemas::value_form(DC, "title", Part::One),
            Some(ValueForm::LangAlt)
        );
        assert_eq!(
            xmp_schemas::value_form(DC, "title", Part::Two),
            Some(ValueForm::LangAlt)
        );
        assert_eq!(xmp_schemas::value_form(DC, "title", Part::Four), None);
    }

    /// The one property whose **form** the two revisions disagree about, from
    /// both sides and under both parts.
    ///
    /// `photoshop:SupplementalCategories` is Text in the revision PDF/A-1
    /// cites and an unordered array in the one parts 2 and 3 cite, and the
    /// veraPDF suite asserts the conforming and the non-conforming spelling
    /// under each. Four assertions, because a table applied to the wrong part
    /// reports a conforming file — which is exactly what the hand-written
    /// override these tables replaced did before the part was added to it.
    #[test]
    fn the_one_form_the_two_revisions_disagree_on_holds_under_both_parts() {
        const NS: &str = "xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\"";
        let text = format!(
            "<rdf:Description {NS} rdf:about=\"\">\
             <photoshop:SupplementalCategories>c</photoshop:SupplementalCategories>\
             </rdf:Description>"
        );
        let bag = format!(
            "<rdf:Description {NS} rdf:about=\"\">\
             <photoshop:SupplementalCategories><rdf:Bag><rdf:li>c</rdf:li></rdf:Bag>\
             </photoshop:SupplementalCategories></rdf:Description>"
        );
        // Part 1: text conforms, an array does not.
        assert!(types(&text, Part::One).is_empty());
        assert_eq!(types(&bag, Part::One).len(), 1);
        // Parts 2 and 3: exactly the other way round.
        assert!(types(&bag, Part::Two).is_empty());
        assert_eq!(types(&text, Part::Two).len(), 1);
        assert!(types(&bag, Part::Three).is_empty());
        assert_eq!(types(&text, Part::Three).len(), 1);
    }

    /// Both revision tables are sorted, which is what makes the lookup a
    /// binary search and the iteration one order on every target (ruling 4).
    ///
    /// A generator that emitted an unsorted schema would make `value_form`
    /// miss properties silently — a binary search over unsorted data returns
    /// `Err` rather than failing — so the property is checked rather than
    /// trusted to the script that emitted the rows.
    #[test]
    fn both_revision_tables_are_sorted_and_have_no_duplicate_property() {
        for (label, table, want_schemas, want_properties) in [
            ("January 2004", xmp_schemas::PREDEFINED_2004, 11, 169),
            ("September 2005", xmp_schemas::PREDEFINED_2005, 14, 274),
        ] {
            let mut schemas = 0;
            let mut properties = 0;
            let mut previous_uri = "";
            for schema in table {
                assert!(
                    previous_uri < schema.uri,
                    "{label}: {} is out of order after {previous_uri}",
                    schema.uri
                );
                previous_uri = schema.uri;
                schemas += 1;
                let mut previous = "";
                for (name, _) in schema.properties {
                    assert!(
                        previous < *name,
                        "{label}: {name} in {} is out of order after {previous}",
                        schema.uri
                    );
                    previous = name;
                    properties += 1;
                }
            }
            assert_eq!(schemas, want_schemas, "{label}: schemas");
            assert_eq!(properties, want_properties, "{label}: properties");
        }
    }

    /// The two tables differ **where the two specifications differ**, and
    /// agree everywhere else.
    ///
    /// A table emitted by a script needs a check that the script's output is
    /// what the specifications say, and a count alone does not give one: two
    /// tables of the right size could still be the same table twice, or the
    /// same table shifted. So the differences the transcription found are
    /// named one by one — the three schemas September 2005 added, the two
    /// `xmp` properties it added, the one `exif` property it dropped, and the
    /// one value form it changed — and everything else is asserted to be
    /// identical, property for property.
    #[test]
    fn the_two_tables_differ_exactly_where_the_two_specifications_do() {
        use xmp_schemas::Schema;

        fn form(table: &'static [Schema], uri: &str, local: &str) -> Option<ValueForm> {
            let schema = table.iter().find(|schema| schema.uri == uri)?;
            let index = schema
                .properties
                .binary_search_by(|(name, _)| (*name).cmp(local))
                .ok()?;
            Some(schema.properties[index].1)
        }
        fn has(table: &'static [Schema], uri: &str) -> bool {
            table.iter().any(|schema| schema.uri == uri)
        }
        const OLD: &[Schema] = xmp_schemas::PREDEFINED_2004;
        const NEW: &[Schema] = xmp_schemas::PREDEFINED_2005;

        // Three whole schemas, added in June 2005 by the 2005 document's own
        // changelog: Camera Raw, the additional Exif properties, and Dynamic
        // Media.
        for uri in [
            "http://ns.adobe.com/camera-raw-settings/1.0/",
            "http://ns.adobe.com/exif/1.0/aux/",
            "http://ns.adobe.com/xmp/1.0/DynamicMedia/",
        ] {
            assert!(!has(OLD, uri), "{uri} is not in the January 2004 revision");
            assert!(has(NEW, uri), "{uri} is in the September 2005 revision");
        }

        // `xmp:Label` and `xmp:Rating`, added to the XMP Basic schema.
        const XMP: &str = "http://ns.adobe.com/xap/1.0/";
        for local in ["Label", "Rating"] {
            assert_eq!(form(OLD, XMP, local), None, "xmp:{local} in 2004");
            assert_eq!(
                form(NEW, XMP, local),
                Some(ValueForm::Simple),
                "xmp:{local} in 2005"
            );
        }

        // And the other direction, so this is not simply "2005 has more".
        const EXIF: &str = "http://ns.adobe.com/exif/1.0/";
        assert_eq!(form(OLD, EXIF, "MakerNote"), Some(ValueForm::Simple));
        assert_eq!(form(NEW, EXIF, "MakerNote"), None);

        // The one property whose form moved.
        const PHOTOSHOP: &str = "http://ns.adobe.com/photoshop/1.0/";
        assert_eq!(
            form(OLD, PHOTOSHOP, "SupplementalCategories"),
            Some(ValueForm::Simple)
        );
        assert_eq!(
            form(NEW, PHOTOSHOP, "SupplementalCategories"),
            Some(ValueForm::Array)
        );

        // Everything else agrees. Exactly one shared property disagrees about
        // its form across the whole of both tables, and it is the one above —
        // which is why no corpus file the single table these replaced could
        // judge changed its verdict. The eleven that moved moved on names that
        // table did not carry at all.
        let mut disagreements = Vec::new();
        for schema in OLD {
            for (name, old) in schema.properties {
                if let Some(new) = form(NEW, schema.uri, name) {
                    if new != *old {
                        disagreements.push(format!("{}:{name}", schema.prefix));
                    }
                }
            }
        }
        assert_eq!(disagreements, ["photoshop:SupplementalCategories"]);
    }
}
