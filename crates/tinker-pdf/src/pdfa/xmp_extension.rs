//! The PDF/A extension schema description: ISO 19005-1 6.7.8 and ISO 19005-2
//! 6.6.2.3.2 and 6.6.2.3.3.
//!
//! This is the **exception** to the predefined-schema rule in
//! [`super::xmp_schemas`], and it exists in this build only because the rule
//! does. A packet may carry a property no predefined schema of the cited
//! revision defines, on one condition: the packet describes that property
//! itself, in the `pdfaExtension` markup this file reads. A membership rule
//! written without the exception would report every conforming file that uses
//! one, which is why neither half landed before the other.
//!
//! # What the markup is
//!
//! One top-level property, `pdfaExtension:schemas`, holding an unordered array
//! of **schema descriptions**. Each description names a namespace and carries
//! arrays of **property descriptions** and of **value type descriptions**; a
//! value type description carries an array of **field descriptions**. Four
//! vocabularies, one per level, and ISO 19005 fixes the prefix of every one of
//! them.
//!
//! ```text
//! pdfaExtension:schemas   rdf:Bag of schema descriptions
//!   pdfaSchema:schema        a human-readable name        required
//!   pdfaSchema:namespaceURI  the namespace described      required
//!   pdfaSchema:prefix        its preferred prefix         required
//!   pdfaSchema:property      rdf:Seq of property descriptions
//!     pdfaProperty:name        required
//!     pdfaProperty:valueType   required
//!     pdfaProperty:category    required
//!     pdfaProperty:description required
//!   pdfaSchema:valueType     rdf:Seq of value type descriptions
//!     pdfaType:type            required
//!     pdfaType:namespaceURI    required
//!     pdfaType:prefix          required
//!     pdfaType:description     required
//!     pdfaType:field           rdf:Seq of field descriptions, required
//!       pdfaField:name           required
//!       pdfaField:valueType      required
//!       pdfaField:description    required
//! ```
//!
//! # Which entries are required, and how that was settled
//!
//! By the conformance suite's own statements, one fixture per entry. Every
//! veraPDF fixture states its expected message in its outline, so each row
//! below is a published claim rather than a reading of the clause table:
//!
//! | fixture | says | so |
//! | --- | --- | --- |
//! | `6-6-2-3-3-t01-fail-a` | `pdfaSchema:namespaceURI` missing, **fail** | required |
//! | `6-6-2-3-3-t01-fail-b` | `pdfaSchema:prefix` missing, **fail** | required |
//! | `6-6-2-3-3-t01-fail-c` | `pdfaSchema:schema` missing, **fail** | required |
//! | `6-6-2-3-3-t01-pass-e` | `pdfaSchema:valueType` missing, **pass** | optional |
//! | `6-6-2-3-3-t05-pass-a` | `pdfaSchema:property` missing, **pass** | optional |
//! | `6-6-2-3-3-t02-fail-a`…`d` | a `pdfaProperty` entry missing, **fail** | all four required |
//! | `6-6-2-3-3-t03-fail-a`,`b`,`c`,`e`,`g` | a `pdfaType` entry missing, **fail** | all five required |
//! | `6-6-2-3-3-t04-fail-a`,`b`,`c` | a `pdfaField` entry missing, **fail** | all three required |
//!
//! The two `pass` rows are the ones that shape the code. `pdfaSchema:property`
//! and `pdfaSchema:valueType` are **not** required entries: a description that
//! carries neither describes no property, and the consequence — that a property
//! it would have described is not a member — is the membership rule's to
//! report, not this file's. Treating either as required would report
//! `6-6-2-3-3-t05-pass-a`, which the suite annotates conforming.
//!
//! # The prefixes are matched, not just the namespaces
//!
//! ISO 19005 fixes the five prefixes, and the suite pins all four of the inner
//! ones from the failing side: `6-6-2-3-3-t01-fail-f`, `t02-fail-e`,
//! `t03-fail-f` and `t04-fail-d` under part 2, and `6-7-8-t06/t05/t07/t04-fail-a`
//! under part 1. **In every one of those eight files the offending prefix is
//! bound to the correct namespace URI** — `xmlns:nonpdfaSchema="http://www.aiim
//! .org/pdfa/ns/schema#"` — so a reader that matched on the resolved namespace
//! alone, which is what XML means by the name, would pass all eight. The
//! requirement is genuinely about the spelling.
//!
//! So an element is *recognised* by its namespace, which is what says what it
//! means, and then its prefix is *checked*, which is what the clause requires.
//! Recognising it first matters: a description written with the wrong prefix is
//! still read, so the file gets one finding about the prefix rather than that
//! finding plus a cascade of "this entry is missing" about entries that are
//! plainly there.

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_xml::{Element, Event, Name, Source};

use super::FindingKind;

/// The container schema's namespace: `pdfaExtension:schemas` lives here.
const EXTENSION: &str = "http://www.aiim.org/pdfa/ns/extension/";
/// The schema description vocabulary.
const SCHEMA: &str = "http://www.aiim.org/pdfa/ns/schema#";
/// The property description vocabulary.
const PROPERTY: &str = "http://www.aiim.org/pdfa/ns/property#";
/// The value type description vocabulary.
const TYPE: &str = "http://www.aiim.org/pdfa/ns/type#";
/// The field description vocabulary.
const FIELD: &str = "http://www.aiim.org/pdfa/ns/field#";

/// The PDF/A identification schema, which is not described here and is not
/// judged by the membership rule either.
///
/// ISO 19005 defines `pdfaid` itself and requires a conforming file to carry
/// it, so it is not a property the packet invented and it is not one a
/// predefined XMP schema could ever name. The corpus settles the consequence:
/// **1 646 `pdfaid` properties across the 831 files the suite annotates
/// conforming, and only 7 of those files describe any extension schema at
/// all** — a membership rule that judged `pdfaid` would report nearly every
/// conforming file in the suite. Whether the packet describes `pdfaid` where
/// the part asks it to is clause 6.7.11's question, and `super::STAGED` still
/// carries that row.
const IDENTIFICATION: &str = "http://www.aiim.org/pdfa/ns/id/";

/// The RDF namespace.
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// Whether a namespace is one ISO 19005 defines for its own use, and which the
/// membership rule therefore does not judge.
pub(super) fn is_an_iso_19005_namespace(uri: &str) -> bool {
    matches!(
        uri,
        IDENTIFICATION | EXTENSION | SCHEMA | PROPERTY | TYPE | FIELD
    )
}

/// Which of the four description vocabularies an element belongs to.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Vocabulary {
    Schema,
    Property,
    Type,
    Field,
}

impl Vocabulary {
    /// The vocabulary a namespace names, if it names one.
    fn of(uri: &str) -> Option<Vocabulary> {
        match uri {
            SCHEMA => Some(Vocabulary::Schema),
            PROPERTY => Some(Vocabulary::Property),
            TYPE => Some(Vocabulary::Type),
            FIELD => Some(Vocabulary::Field),
            _ => None,
        }
    }

    /// The prefix ISO 19005 fixes for it.
    fn prefix(self) -> &'static str {
        match self {
            Vocabulary::Schema => "pdfaSchema",
            Vocabulary::Property => "pdfaProperty",
            Vocabulary::Type => "pdfaType",
            Vocabulary::Field => "pdfaField",
        }
    }

    /// The entries a description in this vocabulary must carry, in the order a
    /// finding about them is reported.
    fn required(self) -> &'static [&'static str] {
        match self {
            // `property` and `valueType` are deliberately absent: see the
            // module doc's table, and the two `pass` fixtures in it.
            Vocabulary::Schema => &["namespaceURI", "prefix", "schema"],
            Vocabulary::Property => &["category", "description", "name", "valueType"],
            Vocabulary::Type => &["description", "field", "namespaceURI", "prefix", "type"],
            Vocabulary::Field => &["description", "name", "valueType"],
        }
    }

    /// How a finding names an entry of this vocabulary: `pdfaSchema:prefix`.
    fn qualify(self, entry: &str) -> String {
        format!("{}:{entry}", self.prefix())
    }
}

/// One description node — an `rdf:li` or `rdf:Description` inside the
/// extension markup — and the entries it carries.
struct Node {
    /// The node it sits inside, so a property description can find the schema
    /// description that owns it.
    parent: Option<usize>,
    /// The path depth its start tag sat at, which is what closes it.
    depth: usize,
    /// Which vocabulary its entries came from. `None` until the first entry is
    /// seen, and still `None` for a node that carries none.
    vocabulary: Option<Vocabulary>,
    /// The entries, by local name. The value is the text where there is text
    /// and empty where the entry is itself a container; presence is what the
    /// required-entry check reads, and the text is what names a property.
    entries: BTreeMap<String, String>,
}

/// What the packet's extension schemas describe, and what is wrong with them.
pub(super) struct Extensions {
    /// The `(namespace URI, property name)` pairs the packet describes.
    described: BTreeSet<(String, String)>,
    /// Findings about the descriptions themselves, deduplicated and ordered.
    pub(super) defects: Vec<FindingKind>,
}

impl Extensions {
    /// Nothing described and nothing wrong: the answer for a packet that is
    /// not there.
    pub(super) fn none() -> Extensions {
        Extensions {
            described: BTreeSet::new(),
            defects: Vec::new(),
        }
    }

    /// Whether this packet describes the property `local` in `namespace`.
    pub(super) fn describes(&self, namespace: &str, local: &str) -> bool {
        // A scan rather than a lookup, to answer the question without
        // allocating a pair to ask it with. The set is a packet's own
        // extension schemas, which is nothing or a handful.
        self.described
            .iter()
            .any(|(uri, name)| uri == namespace && name == local)
    }
}

/// Reads a packet's `pdfaExtension:schemas`.
///
/// The walk is one pass over the element stream, the same shape
/// `super::xmp::top_level_properties` uses and for the same reason: RDF/XML is
/// a graph serialisation and this is the part of it the clause talks about.
/// A packet that stops part way keeps what it got through, because a truncated
/// packet has already been reported as unreadable by the time this matters.
pub(super) fn read(packet: &[u8]) -> Extensions {
    let packet = super::readable(packet);
    let Ok(source) = Source::new(&packet) else {
        return Extensions::none();
    };
    let limits = tinker_pdf_xml::Limits::default();
    let reader = source.reader(&limits);

    let mut depth = 0usize;
    // The depth `pdfaExtension:schemas` opened at. Everything this file reads
    // is inside it.
    let mut schemas_at: Option<usize> = None;
    let mut nodes: Vec<Node> = Vec::new();
    // The nodes currently open, innermost last.
    let mut open: Vec<usize> = Vec::new();
    // The entry whose text is being gathered: which node, which local name,
    // and the depth that ends it.
    let mut capturing: Option<(usize, String, usize)> = None;
    let mut text = String::new();
    // Wrong prefixes, as `(vocabulary, what the packet wrote)`. A set, so a
    // packet that misspells the same prefix on ten entries reports it once.
    let mut misspelled: BTreeSet<(Vocabulary, String)> = BTreeSet::new();
    let mut container_misspelled: BTreeSet<String> = BTreeSet::new();

    for event in reader {
        let Ok(event) = event else {
            break;
        };
        match event {
            Event::Start(element) => {
                let name = element.name();
                let namespace = name.namespace().unwrap_or_default().to_string();
                let local = name.local().to_string();

                if schemas_at.is_none() {
                    if namespace == EXTENSION && local == "schemas" {
                        schemas_at = Some(depth);
                        if name.prefix() != Some("pdfaExtension") {
                            container_misspelled
                                .insert(name.prefix().unwrap_or_default().to_string());
                        }
                    }
                } else if namespace == RDF && (local == "li" || local == "Description") {
                    nodes.push(Node {
                        parent: open.last().copied(),
                        depth,
                        vocabulary: None,
                        entries: BTreeMap::new(),
                    });
                    let index = nodes.len() - 1;
                    open.push(index);
                    // The shorthand form writes a description's entries as
                    // attributes of the `rdf:li` itself. A file that used it
                    // and was read only for child elements would be reported
                    // as missing every entry it has.
                    attributes_into(&element, &mut nodes[index], &mut misspelled);
                } else if let Some(vocabulary) = Vocabulary::of(&namespace) {
                    if name.prefix() != Some(vocabulary.prefix()) {
                        misspelled
                            .insert((vocabulary, name.prefix().unwrap_or_default().to_string()));
                    }
                    if let Some(&index) = open.last() {
                        let node = &mut nodes[index];
                        node.vocabulary.get_or_insert(vocabulary);
                        node.entries.insert(local.clone(), String::new());
                        capturing = Some((index, local.clone(), depth));
                        text.clear();
                    }
                }

                depth += 1;
            }
            Event::Text(chunk) | Event::Cdata(chunk) => {
                if capturing.is_some() {
                    text.push_str(&chunk);
                }
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if let Some((index, entry, at)) = capturing.as_ref() {
                    if *at == depth {
                        nodes[*index].entries.insert(entry.clone(), text.clone());
                        capturing = None;
                        text.clear();
                    }
                }
                if open
                    .last()
                    .is_some_and(|&index| nodes[index].depth == depth)
                {
                    open.pop();
                }
                if schemas_at == Some(depth) {
                    schemas_at = None;
                    open.clear();
                }
            }
            _ => {}
        }
    }

    finish(&nodes, &misspelled, &container_misspelled)
}

/// Reads a description's entries from the attribute shorthand.
fn attributes_into(
    element: &Element<'_>,
    node: &mut Node,
    misspelled: &mut BTreeSet<(Vocabulary, String)>,
) {
    for attribute in element.attributes() {
        let name: &Name<'_> = attribute.name();
        let Some(vocabulary) = name.namespace().and_then(Vocabulary::of) else {
            continue;
        };
        if name.prefix() != Some(vocabulary.prefix()) {
            misspelled.insert((vocabulary, name.prefix().unwrap_or_default().to_string()));
        }
        node.vocabulary.get_or_insert(vocabulary);
        node.entries
            .insert(name.local().to_string(), attribute.value().to_string());
    }
}

/// Turns the walked nodes into the described set and the findings.
fn finish(
    nodes: &[Node],
    misspelled: &BTreeSet<(Vocabulary, String)>,
    container_misspelled: &BTreeSet<String>,
) -> Extensions {
    let mut described = BTreeSet::new();
    // A set rather than a list, because one packet often repeats the same
    // omission in every description it carries and a finding per repetition
    // says nothing the first one did not.
    let mut missing: BTreeSet<String> = BTreeSet::new();

    for node in nodes {
        let Some(vocabulary) = node.vocabulary else {
            continue;
        };
        for entry in vocabulary.required() {
            if !node.entries.contains_key(*entry) {
                missing.insert(vocabulary.qualify(entry));
            }
        }
        if vocabulary != Vocabulary::Property {
            continue;
        }
        // A described property is named by its own `pdfaProperty:name` and by
        // the `pdfaSchema:namespaceURI` of the description that owns it. Both
        // halves, which is what makes `is_an_extension_property` a statement
        // about a namespace and not just about a name: two schemas can
        // describe the same name.
        let Some(name) = node.entries.get("name") else {
            continue;
        };
        let Some(namespace) = owning_namespace(nodes, node) else {
            continue;
        };
        described.insert((namespace, name.trim().to_string()));
    }

    let mut defects = Vec::new();
    for prefix in container_misspelled {
        defects.push(FindingKind::XmpExtensionPrefix {
            expected: "pdfaExtension",
            found: prefix.clone(),
        });
    }
    for (vocabulary, prefix) in misspelled {
        defects.push(FindingKind::XmpExtensionPrefix {
            expected: vocabulary.prefix(),
            found: prefix.clone(),
        });
    }
    for entry in missing {
        defects.push(FindingKind::XmpExtensionEntryMissing { entry });
    }
    Extensions { described, defects }
}

/// The `pdfaSchema:namespaceURI` of the schema description that owns `node`.
fn owning_namespace(nodes: &[Node], node: &Node) -> Option<String> {
    let mut cursor = node.parent;
    while let Some(index) = cursor {
        let candidate = &nodes[index];
        if candidate.vocabulary == Some(Vocabulary::Schema) {
            return candidate
                .entries
                .get("namespaceURI")
                .map(|uri| uri.trim().to_string());
        }
        cursor = candidate.parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packet these tests vary, with `body` dropped in as the extension
    /// markup's schema descriptions.
    fn packet(descriptions: &str) -> String {
        format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
             <rdf:Description rdf:about=\"\" \
             xmlns:pdfaExtension=\"{EXTENSION}\" xmlns:pdfaSchema=\"{SCHEMA}\" \
             xmlns:pdfaProperty=\"{PROPERTY}\" xmlns:pdfaType=\"{TYPE}\" \
             xmlns:pdfaField=\"{FIELD}\">\
             <pdfaExtension:schemas><rdf:Bag>{descriptions}</rdf:Bag>\
             </pdfaExtension:schemas></rdf:Description>\
             </rdf:RDF></x:xmpmeta>"
        )
    }

    /// A complete schema description for `http://example.invalid/ns/`,
    /// describing one property `Machine`.
    const COMPLETE: &str = "<rdf:li rdf:parseType=\"Resource\">\
        <pdfaSchema:schema>An example schema</pdfaSchema:schema>\
        <pdfaSchema:namespaceURI>http://example.invalid/ns/</pdfaSchema:namespaceURI>\
        <pdfaSchema:prefix>ex</pdfaSchema:prefix>\
        <pdfaSchema:property><rdf:Seq><rdf:li rdf:parseType=\"Resource\">\
        <pdfaProperty:name>Machine</pdfaProperty:name>\
        <pdfaProperty:valueType>Text</pdfaProperty:valueType>\
        <pdfaProperty:category>external</pdfaProperty:category>\
        <pdfaProperty:description>the machine</pdfaProperty:description>\
        </rdf:li></rdf:Seq></pdfaSchema:property></rdf:li>";

    /// The names of the missing entries a body reports, in order.
    fn missing(descriptions: &str) -> Vec<String> {
        read(packet(descriptions).as_bytes())
            .defects
            .into_iter()
            .filter_map(|kind| match kind {
                FindingKind::XmpExtensionEntryMissing { entry } => Some(entry),
                _ => None,
            })
            .collect()
    }

    /// The wrong prefixes a body reports, as `expected/found`.
    fn prefixes(descriptions: &str) -> Vec<String> {
        read(packet(descriptions).as_bytes())
            .defects
            .into_iter()
            .filter_map(|kind| match kind {
                FindingKind::XmpExtensionPrefix { expected, found } => {
                    Some(format!("{expected}/{found}"))
                }
                _ => None,
            })
            .collect()
    }

    /// The twin that matters most: a complete description is silent, and the
    /// property it describes is described.
    #[test]
    fn a_complete_description_is_silent_and_describes_its_property() {
        let extensions = read(packet(COMPLETE).as_bytes());
        assert!(extensions.defects.is_empty(), "{:?}", extensions.defects);
        assert!(extensions.describes("http://example.invalid/ns/", "Machine"));
    }

    /// A described property is matched on **both** halves of its name.
    ///
    /// The name alone would make one packet's `ex:Machine` describe another
    /// namespace's `Machine`, which is the shape of a rule that lets a
    /// non-conforming file through by accident. Injected, this is the check
    /// that fires.
    #[test]
    fn a_name_alone_does_not_describe_a_property() {
        let extensions = read(packet(COMPLETE).as_bytes());
        assert!(extensions.describes("http://example.invalid/ns/", "Machine"));
        assert!(!extensions.describes("http://other.invalid/ns/", "Machine"));
        assert!(!extensions.describes("http://example.invalid/ns/", "Engine"));
    }

    /// Each entry the suite calls required, one at a time, against the
    /// complete description that is its own twin.
    ///
    /// The `pass` fixtures are asserted in the same shape as the `fail` ones:
    /// `pdfaSchema:property` and `pdfaSchema:valueType` can both be absent and
    /// nothing is reported, because `6-6-2-3-3-t05-pass-a` and `t01-pass-e`
    /// say so.
    #[test]
    fn every_required_entry_is_required_and_the_two_optional_ones_are_not() {
        for (entry, without) in [
            (
                "pdfaSchema:schema",
                "<pdfaSchema:schema>An example schema</pdfaSchema:schema>",
            ),
            (
                "pdfaSchema:namespaceURI",
                "<pdfaSchema:namespaceURI>http://example.invalid/ns/</pdfaSchema:namespaceURI>",
            ),
            (
                "pdfaSchema:prefix",
                "<pdfaSchema:prefix>ex</pdfaSchema:prefix>",
            ),
            (
                "pdfaProperty:name",
                "<pdfaProperty:name>Machine</pdfaProperty:name>",
            ),
            (
                "pdfaProperty:valueType",
                "<pdfaProperty:valueType>Text</pdfaProperty:valueType>",
            ),
            (
                "pdfaProperty:category",
                "<pdfaProperty:category>external</pdfaProperty:category>",
            ),
            (
                "pdfaProperty:description",
                "<pdfaProperty:description>the machine</pdfaProperty:description>",
            ),
        ] {
            let body = COMPLETE.replace(without, "");
            assert_ne!(body, COMPLETE, "{entry}: the fixture did not change");
            assert_eq!(missing(&body), [entry.to_string()], "{body}");
        }

        // And the twin: the whole description, untouched, reports nothing.
        assert!(missing(COMPLETE).is_empty());

        // The two the suite annotates `pass`. A description with no
        // `pdfaSchema:property` describes nothing, and that is all it does.
        let bare = "<rdf:li rdf:parseType=\"Resource\">\
             <pdfaSchema:schema>An example schema</pdfaSchema:schema>\
             <pdfaSchema:namespaceURI>http://example.invalid/ns/</pdfaSchema:namespaceURI>\
             <pdfaSchema:prefix>ex</pdfaSchema:prefix></rdf:li>";
        assert!(missing(bare).is_empty());
        assert!(!read(packet(bare).as_bytes()).describes("http://example.invalid/ns/", "Machine"));
    }

    /// A value type description and its fields, each entry required.
    #[test]
    fn a_value_type_description_carries_five_entries_and_its_fields_carry_three() {
        const WITH_TYPE: &str = "<rdf:li rdf:parseType=\"Resource\">\
            <pdfaSchema:schema>An example schema</pdfaSchema:schema>\
            <pdfaSchema:namespaceURI>http://example.invalid/ns/</pdfaSchema:namespaceURI>\
            <pdfaSchema:prefix>ex</pdfaSchema:prefix>\
            <pdfaSchema:valueType><rdf:Seq><rdf:li rdf:parseType=\"Resource\">\
            <pdfaType:type>Part</pdfaType:type>\
            <pdfaType:namespaceURI>http://example.invalid/part/</pdfaType:namespaceURI>\
            <pdfaType:prefix>pt</pdfaType:prefix>\
            <pdfaType:description>a part</pdfaType:description>\
            <pdfaType:field><rdf:Seq><rdf:li rdf:parseType=\"Resource\">\
            <pdfaField:name>Number</pdfaField:name>\
            <pdfaField:valueType>Text</pdfaField:valueType>\
            <pdfaField:description>its number</pdfaField:description>\
            </rdf:li></rdf:Seq></pdfaType:field>\
            </rdf:li></rdf:Seq></pdfaSchema:valueType></rdf:li>";

        assert!(missing(WITH_TYPE).is_empty(), "{:?}", missing(WITH_TYPE));

        for (entry, without) in [
            ("pdfaType:type", "<pdfaType:type>Part</pdfaType:type>"),
            (
                "pdfaType:namespaceURI",
                "<pdfaType:namespaceURI>http://example.invalid/part/</pdfaType:namespaceURI>",
            ),
            ("pdfaType:prefix", "<pdfaType:prefix>pt</pdfaType:prefix>"),
            (
                "pdfaType:description",
                "<pdfaType:description>a part</pdfaType:description>",
            ),
            ("pdfaField:name", "<pdfaField:name>Number</pdfaField:name>"),
            (
                "pdfaField:valueType",
                "<pdfaField:valueType>Text</pdfaField:valueType>",
            ),
            (
                "pdfaField:description",
                "<pdfaField:description>its number</pdfaField:description>",
            ),
        ] {
            let body = WITH_TYPE.replace(without, "");
            assert_ne!(body, WITH_TYPE, "{entry}: the fixture did not change");
            assert_eq!(missing(&body), [entry.to_string()], "{body}");
        }

        // `pdfaType:field` is required too, and taking the whole array out
        // takes the field description with it.
        let no_field = "<rdf:li rdf:parseType=\"Resource\">\
            <pdfaSchema:schema>An example schema</pdfaSchema:schema>\
            <pdfaSchema:namespaceURI>http://example.invalid/ns/</pdfaSchema:namespaceURI>\
            <pdfaSchema:prefix>ex</pdfaSchema:prefix>\
            <pdfaSchema:valueType><rdf:Seq><rdf:li rdf:parseType=\"Resource\">\
            <pdfaType:type>Part</pdfaType:type>\
            <pdfaType:namespaceURI>http://example.invalid/part/</pdfaType:namespaceURI>\
            <pdfaType:prefix>pt</pdfaType:prefix>\
            <pdfaType:description>a part</pdfaType:description>\
            </rdf:li></rdf:Seq></pdfaSchema:valueType></rdf:li>";
        assert_eq!(missing(no_field), ["pdfaType:field".to_string()]);
    }

    /// The prefix requirement, from both sides, on the namespace the suite
    /// binds it to.
    ///
    /// The wrong prefix here is bound to the **right** namespace, exactly as
    /// the eight corpus fixtures bind it, so this fails if the check ever
    /// becomes a namespace comparison.
    #[test]
    fn the_four_prefixes_are_the_ones_iso_19005_fixes() {
        assert!(prefixes(COMPLETE).is_empty());

        let body = COMPLETE.replace("pdfaSchema:", "nonpdfaSchema:");
        let packet = format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
             <rdf:Description rdf:about=\"\" \
             xmlns:pdfaExtension=\"{EXTENSION}\" xmlns:nonpdfaSchema=\"{SCHEMA}\" \
             xmlns:pdfaProperty=\"{PROPERTY}\">\
             <pdfaExtension:schemas><rdf:Bag>{body}</rdf:Bag>\
             </pdfaExtension:schemas></rdf:Description>\
             </rdf:RDF></x:xmpmeta>"
        );
        let extensions = read(packet.as_bytes());
        let reported: Vec<String> = extensions
            .defects
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect();
        assert!(
            reported
                .iter()
                .any(|line| line.contains("pdfaSchema") && line.contains("nonpdfaSchema")),
            "{reported:?}"
        );
        // And the description is still read, so the file is not also told that
        // three entries it plainly carries are missing.
        assert!(
            !reported.iter().any(|line| line.contains("EntryMissing")),
            "{reported:?}"
        );
        assert!(extensions.describes("http://example.invalid/ns/", "Machine"));
    }

    /// The attribute shorthand says the same thing as the element form.
    ///
    /// XMP lets a structure's fields be written as attributes, and a producer
    /// that does it writes a conforming file. A reader that only looked at
    /// child elements would report every entry of it as missing — a
    /// conforming file reported, which is the failure mode this rule group is
    /// held to avoid.
    #[test]
    fn the_attribute_shorthand_is_read_as_the_element_form() {
        let body = "<rdf:li rdf:parseType=\"Resource\" \
             pdfaSchema:schema=\"An example schema\" \
             pdfaSchema:namespaceURI=\"http://example.invalid/ns/\" \
             pdfaSchema:prefix=\"ex\">\
             <pdfaSchema:property><rdf:Seq>\
             <rdf:li rdf:parseType=\"Resource\" pdfaProperty:name=\"Machine\" \
             pdfaProperty:valueType=\"Text\" pdfaProperty:category=\"external\" \
             pdfaProperty:description=\"the machine\"/>\
             </rdf:Seq></pdfaSchema:property></rdf:li>";
        let extensions = read(packet(body).as_bytes());
        assert!(extensions.defects.is_empty(), "{:?}", extensions.defects);
        assert!(extensions.describes("http://example.invalid/ns/", "Machine"));
    }

    /// A packet with no extension markup describes nothing and reports
    /// nothing, which is the common case and the one a defect here would be
    /// loudest in.
    #[test]
    fn a_packet_with_no_extension_markup_is_silent() {
        let plain = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
             <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
             <rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\" rdf:about=\"\">\
             <dc:format>application/pdf</dc:format></rdf:Description>\
             </rdf:RDF></x:xmpmeta>";
        let extensions = read(plain.as_bytes());
        assert!(extensions.defects.is_empty());
        assert!(!extensions.describes("http://purl.org/dc/elements/1.1/", "format"));
    }

    /// The namespaces ISO 19005 defines for itself, named rather than
    /// described.
    #[test]
    fn the_standards_own_namespaces_are_known() {
        for uri in [IDENTIFICATION, EXTENSION, SCHEMA, PROPERTY, TYPE, FIELD] {
            assert!(is_an_iso_19005_namespace(uri), "{uri}");
        }
        for uri in [
            "http://purl.org/dc/elements/1.1/",
            "http://ns.adobe.com/xap/1.0/",
            "http://example.invalid/ns/",
        ] {
            assert!(!is_an_iso_19005_namespace(uri), "{uri}");
        }
    }
}
