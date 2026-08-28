//! PDF/A conformance (ISO 19005), read.
//!
//! The question is not "is this a PDF/A" but "**which** PDF/A does this file
//! claim to be, and which clauses of that flavour does it break". Those are
//! different questions and only the second is useful: a file that claims
//! PDF/A-2b and breaks one metadata rule is a different problem from a file
//! that claims nothing, and a validator that answers `false` to both has told
//! a caller nothing they can act on.
//!
//! So a verdict is a **list of findings**, each naming its clause and, where
//! there is one, the object it is about. Conformance is the list being empty.
//! There is no boolean that discards the list.
//!
//! # Why the flavour comes from the metadata and not from the structure
//!
//! ISO 19005 puts the claim in an XMP packet: `pdfaid:part` and, for parts 1
//! to 3, `pdfaid:conformance`. Nothing else in a PDF says which flavour it is
//! meant to be, so a validator with no claim to check against has nothing to
//! do — and saying so is the honest answer rather than picking a flavour and
//! reporting against it.
//!
//! Measured over the 4 605 files in the fetched corpora: 3 231 carry an XMP
//! packet and **2 481 declare a `pdfaid:part`** — 839 part 1, 1 075 part 2, 28
//! part 3, 516 part 4. Two do not declare any of those: one says part 9 and
//! one says part 0, neither of which exists, which is why an unknown part is a
//! *finding* rather than an absence.
//!
//! **Part 4 carries no conformance letter**, and the corpus confirms it: 1 941
//! files declare one against 2 459 that declare a part, and the difference is
//! very nearly the 516 part-4 files. So a level on a part-4 file and a missing
//! level on a part-1-to-3 file are both findings, in opposite directions.
//!
//! # What this module does not do yet
//!
//! Milestone 1 of `docs/design/pdfa.md`: the verdict types, the flavour, and
//! the rule table's shape. The rule *groups* — syntax, font, colour, XMP — are
//! milestones 2 to 5, and until they land a clean verdict means "nothing this
//! build checks was broken" rather than "this file conforms".
//! [`Verdict::coverage`] is what says which, and it is not decoration: a
//! validator that cannot say what it did not check is a validator whose empty
//! answer cannot be read.

use tinker_pdf_cos::ObjRef;
use tinker_pdf_xml::{Event, Source};

use crate::Document;

/// Which part of ISO 19005 a file claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// ISO 19005-1, on PDF 1.4.
    One,
    /// ISO 19005-2, on PDF 1.7.
    Two,
    /// ISO 19005-3, which is part 2 plus arbitrary embedded files.
    Three,
    /// ISO 19005-4, on PDF 2.0. It has no conformance levels.
    Four,
}

impl Part {
    /// The `pdfaid:part` value.
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Part::One => 1,
            Part::Two => 2,
            Part::Three => 3,
            Part::Four => 4,
        }
    }

    fn from_number(value: i64) -> Option<Part> {
        match value {
            1 => Some(Part::One),
            2 => Some(Part::Two),
            3 => Some(Part::Three),
            4 => Some(Part::Four),
            _ => None,
        }
    }

    /// Whether this part defines the level.
    ///
    /// Parts 1 to 3 have the accessibility and basic levels, parts 2 and 3 add
    /// Unicode, and **part 4 has its own two**: `E` for engineering and `F`
    /// for embedded files. Part 4 is also the only one where declaring no
    /// level at all is correct — plain PDF/A-4 is a flavour.
    ///
    /// This table was wrong when it was written. It said part 4 had no levels,
    /// on a reading of the part's name rather than of its clauses, and the
    /// corpus census disagreed immediately: 16 files declare `E` and 10
    /// declare `F`, in directories the veraPDF suite calls `PDF_A-4e` and
    /// `PDF_A-4f`.
    #[must_use]
    pub fn allows(self, level: Level) -> bool {
        matches!(
            (self, level),
            (Part::One, Level::A | Level::B)
                | (Part::Two | Part::Three, Level::A | Level::B | Level::U)
                | (Part::Four, Level::E | Level::F)
        )
    }

    /// Whether omitting the level is correct for this part.
    #[must_use]
    pub fn level_optional(self) -> bool {
        self == Part::Four
    }
}

/// The conformance level, for the parts that have one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Accessible: level B plus a tagged structure tree and Unicode mapping.
    A,
    /// Basic: the file renders the same way everywhere.
    B,
    /// Unicode: level B plus every glyph mapped to Unicode. Parts 2 and 3.
    U,
    /// Engineering: part 4 only, permitting 3D and rich media.
    E,
    /// Embedded files: part 4 only, permitting arbitrary attachments.
    F,
}

impl Level {
    /// The `pdfaid:conformance` letter.
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            Level::A => 'A',
            Level::B => 'B',
            Level::U => 'U',
            Level::E => 'E',
            Level::F => 'F',
        }
    }

    fn from_letter(text: &str) -> Option<Level> {
        match text.trim() {
            "A" | "a" => Some(Level::A),
            "B" | "b" => Some(Level::B),
            "U" | "u" => Some(Level::U),
            "E" | "e" => Some(Level::E),
            "F" | "f" => Some(Level::F),
            _ => None,
        }
    }
}

/// What a file claims to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flavour {
    /// The part.
    pub part: Part,
    /// The level, absent for part 4 and for a part 1-to-3 file that omitted
    /// it — [`FindingKind::LevelMissing`] is what distinguishes the two.
    pub level: Option<Level>,
}

impl core::fmt::Display for Flavour {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.level {
            Some(level) => write!(f, "PDF/A-{}{}", self.part.number(), level.letter()),
            None => write!(f, "PDF/A-{}", self.part.number()),
        }
    }
}

/// The clause a finding is about.
///
/// A string rather than an enum because the clause numbering differs between
/// parts for the same rule — a file-structure rule is 6.1.x in parts 1 to 3
/// and elsewhere in part 4 — so the number is data about the flavour being
/// checked rather than a variant of the check.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Clause(pub String);

impl core::fmt::Display for Clause {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What was wrong.
///
/// Closed, like `WarningKind`: a caller can match every variant and a new one
/// is a compile error where it is handled rather than a string nobody reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindingKind {
    /// No XMP packet at all, so there is no claim to check.
    MetadataMissing,
    /// An XMP packet with no `pdfaid:part`.
    NoFlavourClaimed,
    /// `pdfaid:part` names a part ISO 19005 does not define.
    PartUnknown {
        /// What the file said.
        declared: String,
    },
    /// `pdfaid:conformance` names a letter the part does not define.
    LevelUnknown {
        /// What the file said.
        declared: String,
    },
    /// A part 1-to-3 file with no conformance level.
    LevelMissing,
    /// A conformance level that exists, on a part that does not define it —
    /// `U` on part 1, or `B` on part 4.
    LevelNotInPart {
        /// What the file said.
        declared: String,
    },
    /// The XMP packet is there and will not parse.
    MetadataUnreadable,
    /// The document is encrypted. Every part of ISO 19005 forbids it: a file
    /// nobody can open without a key is not archival.
    Encrypted,
}

/// One thing wrong with the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceFinding {
    /// Which clause.
    pub clause: Clause,
    /// The object it is about, where there is one.
    pub object: Option<ObjRef>,
    /// What was wrong.
    pub kind: FindingKind,
}

/// Which rule groups a verdict actually ran.
///
/// Not decoration. A verdict with no findings from a validator that checked
/// one group of four means "nothing in that group was broken", and a caller
/// told only "no findings" would read it as "conforms". This is the difference,
/// made impossible to ignore by being in the same struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Coverage {
    /// The flavour claim and the XMP packet around it.
    pub metadata: bool,
    /// File structure: encryption, version, filters, actions, annotations.
    pub syntax: bool,
    /// Fonts: embedding, widths, Unicode mapping.
    pub fonts: bool,
    /// Colour: output intents and device spaces.
    pub colour: bool,
}

impl Coverage {
    /// Whether every group ran, which is the only state in which an empty
    /// finding list means the file conforms.
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.metadata && self.syntax && self.fonts && self.colour
    }
}

/// What the validator made of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// What the file claims to be, when it claims anything.
    pub flavour: Option<Flavour>,
    /// Everything wrong that this build looked for.
    pub findings: Vec<ConformanceFinding>,
    /// Which rule groups ran.
    pub coverage: Coverage,
}

impl Verdict {
    /// Whether this build found anything wrong.
    ///
    /// **Not** "the file conforms" — see [`Verdict::coverage`]. Named for what
    /// it is so that a caller reaching for a boolean gets the honest one.
    #[must_use]
    pub fn found_nothing(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Validates `document` against the flavour it claims.
pub(crate) fn validate(document: &Document) -> Verdict {
    let mut findings = Vec::new();
    let flavour = flavour_of(document, &mut findings);

    // 6.1.3 in parts 1 to 3, and part 4's equivalent: an encrypted file is not
    // archival, whatever else is right about it. Checked here rather than in
    // the syntax group because it needs no machinery and because a file nobody
    // can open is the one finding worth reporting before any other.
    if document.is_encrypted() {
        findings.push(ConformanceFinding {
            clause: clause_for(flavour, "6.1.3"),
            object: None,
            kind: FindingKind::Encrypted,
        });
    }

    Verdict {
        flavour,
        findings,
        coverage: Coverage {
            metadata: true,
            // Milestones 2 to 5 of docs/design/pdfa.md.
            syntax: false,
            fonts: false,
            colour: false,
        },
    }
}

/// The clause number for a rule, which differs by part.
///
/// Parts 1 to 3 number file structure at 6.1; part 4 renumbers. Where this
/// build does not know a part's number for a rule it uses the parts 1-to-3
/// one, because a wrong clause number in a finding is a smaller error than a
/// missing finding — and the `kind` is what a caller matches on.
fn clause_for(flavour: Option<Flavour>, parts_one_to_three: &str) -> Clause {
    match flavour.map(|f| f.part) {
        Some(Part::Four) if parts_one_to_three == "6.1.3" => Clause("6.1.2".to_string()),
        _ => Clause(parts_one_to_three.to_string()),
    }
}

/// Reads `pdfaid:part` and `pdfaid:conformance` out of the XMP packet.
fn flavour_of(document: &Document, findings: &mut Vec<ConformanceFinding>) -> Option<Flavour> {
    let Some(packet) = document.xmp_metadata() else {
        findings.push(ConformanceFinding {
            clause: Clause("6.6".to_string()),
            object: None,
            kind: FindingKind::MetadataMissing,
        });
        return None;
    };

    let Some((part_text, level_text)) = pdfaid(&packet) else {
        findings.push(ConformanceFinding {
            clause: Clause("6.6".to_string()),
            object: None,
            kind: if packet.is_empty() {
                FindingKind::MetadataMissing
            } else {
                FindingKind::NoFlavourClaimed
            },
        });
        return None;
    };

    let Some(part) = part_text
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(Part::from_number)
    else {
        findings.push(ConformanceFinding {
            clause: Clause("6.6".to_string()),
            object: None,
            kind: FindingKind::PartUnknown {
                declared: part_text,
            },
        });
        return None;
    };

    let level = match level_text {
        Some(text) => match Level::from_letter(&text) {
            Some(level) if part.allows(level) => Some(level),
            Some(_) => {
                findings.push(ConformanceFinding {
                    clause: Clause("6.6".to_string()),
                    object: None,
                    kind: FindingKind::LevelNotInPart { declared: text },
                });
                None
            }
            None => {
                findings.push(ConformanceFinding {
                    clause: Clause("6.6".to_string()),
                    object: None,
                    kind: FindingKind::LevelUnknown { declared: text },
                });
                None
            }
        },
        None if part.level_optional() => None,
        None => {
            findings.push(ConformanceFinding {
                clause: Clause("6.6".to_string()),
                object: None,
                kind: FindingKind::LevelMissing,
            });
            None
        }
    };

    Some(Flavour { part, level })
}

/// `pdfaid:part` and `pdfaid:conformance`, wherever RDF put them.
///
/// XMP is RDF/XML and RDF admits the same statement as an attribute or as a
/// child element — `<rdf:Description pdfaid:part="2">` and
/// `<rdf:Description><pdfaid:part>2</pdfaid:part></rdf:Description>` say the
/// same thing. Both forms are in the corpus, so both are read; a reader that
/// handled one would report thousands of files as claiming nothing.
///
/// # The namespace check, and why the first attempt was wrong
///
/// This matched on the **local name** alone, on the argument that real files
/// bind the `pdfaid` prefix in a parent element the packet does not always
/// carry, and that "nothing else in an XMP packet is called `part`".
///
/// The corpus census disagreed within a minute: **PDF/UA declares
/// `pdfuaid:part`**, an entirely different standard's version number in an
/// entirely different namespace, and 434 of the corpus's files were read as
/// claiming a PDF/A part they say nothing about. The local name was never
/// distinctive; it only looked that way from inside PDF/A.
///
/// So the prefix must be `pdfaid`, **or** the resolved namespace must be
/// `http://www.aiim.org/pdfa/ns/id/`. Either alone is too strict — a packet
/// that binds the URI to some other prefix is legal RDF, and a packet that
/// uses the conventional prefix without a visible binding is what real files
/// do — and together they reject `pdfuaid` while accepting both.
const PDFA_ID_NAMESPACE: &str = "http://www.aiim.org/pdfa/ns/id/";

/// Whether a name is in the PDF/A identification schema.
fn is_pdfaid(name: &tinker_pdf_xml::Name<'_>) -> bool {
    name.prefix() == Some("pdfaid") || name.namespace() == Some(PDFA_ID_NAMESPACE)
}

fn pdfaid(packet: &[u8]) -> Option<(String, Option<String>)> {
    let source = Source::new(packet).ok()?;
    // An XMP packet carries no doctype and has no business carrying one, so
    // the strict mode is right here — unlike an XHTML content document, which
    // is why `Doctype::SkipExternalId` exists elsewhere in this crate.
    let limits = tinker_pdf_xml::Limits::default();
    let reader = source.reader(&limits);

    let (mut part, mut level) = (None, None);
    // Which element's text is being collected, if any.
    let mut collecting: Option<&'static str> = None;

    for event in reader {
        let Ok(event) = event else {
            break;
        };
        match event {
            Event::Start(element) => {
                for attribute in element.attributes() {
                    if !is_pdfaid(attribute.name()) {
                        continue;
                    }
                    match attribute.name().local() {
                        "part" if part.is_none() => part = Some(attribute.value().to_string()),
                        "conformance" if level.is_none() => {
                            level = Some(attribute.value().to_string());
                        }
                        _ => {}
                    }
                }
                collecting = if is_pdfaid(element.name()) {
                    match element.local() {
                        "part" if part.is_none() => Some("part"),
                        "conformance" if level.is_none() => Some("conformance"),
                        _ => None,
                    }
                } else {
                    None
                };
            }
            Event::Text(text) | Event::Cdata(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match collecting {
                    Some("part") => part = Some(trimmed.to_string()),
                    Some("conformance") => level = Some(trimmed.to_string()),
                    _ => {}
                }
            }
            Event::End(_) => collecting = None,
            _ => {}
        }
    }

    part.map(|part| (part, level))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(packet: &str) -> Option<(String, Option<String>)> {
        pdfaid(packet.as_bytes())
    }

    #[test]
    fn the_attribute_form_and_the_element_form_say_the_same_thing() {
        let attributes = read(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r">
               <rdf:Description xmlns:pdfaid="p" pdfaid:part="2"
                                pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#,
        );
        assert_eq!(attributes, Some(("2".into(), Some("B".into()))));

        let elements = read(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r">
               <rdf:Description xmlns:pdfaid="p"><pdfaid:part>2</pdfaid:part>
               <pdfaid:conformance>B</pdfaid:conformance></rdf:Description>
               </rdf:RDF></x:xmpmeta>"#,
        );
        assert_eq!(elements, attributes, "RDF admits both and both are read");
    }

    #[test]
    fn a_packet_with_no_claim_reads_as_no_claim_rather_than_as_a_guess() {
        assert_eq!(
            read(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"/></x:xmpmeta>"#),
            None
        );
    }

    #[test]
    fn markup_that_will_not_parse_yields_nothing_rather_than_half_an_answer() {
        assert_eq!(read("<x:xmpmeta><unclosed>"), None);
        assert_eq!(read(""), None);
        assert_eq!(pdfaid(&[0xFF, 0xFE, 0x00]), None);
    }

    /// The level table, which was wrong when it was first written: it said
    /// part 4 had none, and the corpus census found 26 files declaring one.
    #[test]
    fn each_part_admits_exactly_the_levels_iso_19005_gives_it() {
        for (part, allowed) in [
            (Part::One, &[Level::A, Level::B][..]),
            (Part::Two, &[Level::A, Level::B, Level::U]),
            (Part::Three, &[Level::A, Level::B, Level::U]),
            (Part::Four, &[Level::E, Level::F]),
        ] {
            for level in [Level::A, Level::B, Level::U, Level::E, Level::F] {
                assert_eq!(
                    part.allows(level),
                    allowed.contains(&level),
                    "{part:?} and {level:?}"
                );
            }
        }
        // And part 4 is the only one where declaring none is correct.
        assert!(Part::Four.level_optional());
        for part in [Part::One, Part::Two, Part::Three] {
            assert!(!part.level_optional(), "{part:?}");
        }
    }

    #[test]
    fn a_flavour_renders_the_way_the_standard_names_it() {
        assert_eq!(
            Flavour {
                part: Part::Two,
                level: Some(Level::B)
            }
            .to_string(),
            "PDF/A-2B"
        );
        assert_eq!(
            Flavour {
                part: Part::Four,
                level: None
            }
            .to_string(),
            "PDF/A-4"
        );
    }

    #[test]
    fn every_part_number_round_trips() {
        for part in [Part::One, Part::Two, Part::Three, Part::Four] {
            assert_eq!(Part::from_number(i64::from(part.number())), Some(part));
        }
        // The two the corpus actually contains, and neither exists.
        assert_eq!(Part::from_number(9), None);
        assert_eq!(Part::from_number(0), None);
    }

    #[test]
    fn a_verdict_that_checked_one_group_does_not_claim_conformance() {
        let partial = Coverage {
            metadata: true,
            ..Coverage::default()
        };
        assert!(!partial.is_complete());
        assert!(Coverage {
            metadata: true,
            syntax: true,
            fonts: true,
            colour: true
        }
        .is_complete());
    }
}
