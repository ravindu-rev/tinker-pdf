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
//! The largest metadata rule in the standard is *not* here and its absence is
//! deliberate. ISO 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 require every
//! property in the packet to belong to a predefined schema or be described by
//! an extension schema, which needs the XMP specification's predefined-schema
//! property tables as vendored data. Guessing those tables produces false
//! positives on conforming files, which is worse than not checking, so the
//! rule is staged and [`super::STAGED`] names it.

use tinker_pdf_cos::{decode_text_string, parse_date, Date, Dict};
use tinker_pdf_xml::{Event, Name, Source};

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

    // ISO 19005-4 has no `/Info` consistency rule: 6.1.3 all but forbids the
    // dictionary, and `syntax::trailer` enforces that instead. Parts 1 to 3
    // require agreement, and a file that claimed nothing is checked as part 1
    // would check it — the clause exists in every part that has an `/Info`.
    if flavour.map(|f| f.part) == Some(Part::Four) {
        return;
    }

    let doc = &document.inner;
    let info_ref = doc.trailer().get_ref(doc.intern(b"Info"));
    let info = doc.resolve_key(doc.trailer(), doc.intern(b"Info"));
    let Some(info) = info.as_dict() else {
        return;
    };

    for pairing in PAIRINGS {
        let Some(value) = info_string(doc, info, pairing.info) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let found = properties.get(pairing.namespace, pairing.prefix, pairing.local);
        if !agrees(&value, found, pairing.kind) {
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

/// An `/Info` entry as text, resolved through an indirect reference.
fn info_string(doc: &tinker_pdf_cos::CosDocument, info: &Dict, key: &[u8]) -> Option<String> {
    let value = doc.resolve_key(info, doc.intern(key));
    let string = value.as_string()?;
    Some(decode_text_string(&string.bytes).trim().to_string())
}

/// Whether an `/Info` value and the XMP members found for it agree.
fn agrees(info: &str, found: Option<&[String]>, kind: Compare) -> bool {
    let Some(found) = found else {
        // The clause requires the property to be there when the `/Info` entry
        // is. An absent property is a mismatch, not a pass.
        return false;
    };
    match kind {
        Compare::Text | Compare::Array => found.len() == 1 && found[0].trim() == info,
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
    let source = Source::new(packet).ok()?;
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
                            values[index] = Some(vec![attribute.value().trim().to_string()]);
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
                        items.push(buffer.trim().to_string());
                        buffer.clear();
                        in_li = false;
                    } else if depth == at {
                        if items.is_empty() {
                            let text = buffer.trim();
                            if !text.is_empty() {
                                items.push(text.to_string());
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

#[cfg(test)]
mod tests {
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
}
