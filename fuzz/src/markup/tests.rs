use super::*;
use tinker_pdf_xml::html::{self as parser, Document};
use tinker_pdf_xml::{Construct, Doctype, Error, Event, Limits, Source};

fn inputs(count: usize, longest: usize) -> impl Iterator<Item = Vec<u8>> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..count).map(move |n| {
        let len = (n * 97) % longest;
        (0..len)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    })
}

/// What one mode reads, as `xml_structured` compares it: what was read,
/// whether the root opened, and the refusal it stopped at.
fn read(bytes: &[u8], doctype: Doctype) -> (Expected, bool, Option<Error>) {
    let source = Source::new(bytes).expect("a generated document decodes");
    let mut got = Expected::default();
    let mut opened = false;
    for event in source.reader_with(&Limits::DEFAULT, doctype) {
        match event {
            Err(error) => return (got, opened, Some(error)),
            Ok(Event::Start(element)) => {
                opened = true;
                let attributes = element
                    .attributes()
                    .iter()
                    .map(|a| (a.name().qualified().to_string(), a.value().to_string()))
                    .collect();
                got.elements
                    .push((element.name().qualified().to_string(), attributes));
            }
            Ok(Event::End(_)) => {}
            Ok(Event::Text(text) | Event::Cdata(text)) => got.text.push_str(&text),
            Ok(Event::Comment(text)) => got.comments.push(text.to_string()),
            Ok(Event::Instruction { target, value }) => {
                got.instructions
                    .push((target.to_string(), value.to_string()));
            }
        }
    }
    (got, opened, None)
}

/// `xml_structured`'s property for one document, on stable.
fn xml_reads_back(doc: &XmlDocument) -> Result<(), String> {
    for doctype in [Doctype::Refuse, Doctype::SkipExternalId] {
        let (got, opened, refusal) = read(&doc.bytes, doctype);
        let declared = match (doc.declaration, doctype) {
            (Some(_), Doctype::Refuse) => Some(Error::DoctypeUnsupported),
            (Some(Declaration::Subset), Doctype::SkipExternalId) => Some(Error::InternalSubset),
            _ => None,
        };
        if let Some(want) = declared {
            if refusal != Some(want) || opened {
                return Err(format!("{doctype:?}: {refusal:?} for a declaration"));
            }
            continue;
        }
        let want = doc.defect.map(|defect| match defect {
            Defect::UndeclaredPrefix => Error::UndeclaredPrefix,
            Defect::MismatchedEnd => Error::MismatchedEndTag,
            Defect::DuplicateAttribute => Error::DuplicateAttribute,
            Defect::BareAmpersand => Error::Unterminated(Construct::Reference),
        });
        if refusal != want {
            return Err(format!("{doctype:?}: {refusal:?} for {:?}", doc.defect));
        }
        if want.is_none() && got != doc.expected {
            return Err(format!(
                "{doctype:?} read {got:?}\nwritten {:?}",
                doc.expected
            ));
        }
    }
    Ok(())
}

/// `html_structured`'s property for one document, on stable.
fn html_builds_a_tree(doc: &HtmlDocument) -> Result<Document, String> {
    let document = parser::parse_bytes(&doc.bytes, &doc.limits);
    let nodes = document.nodes();
    if nodes.len() > doc.limits.max_tokens + 16 {
        return Err(format!(
            "{} nodes past {}",
            nodes.len(),
            doc.limits.max_tokens
        ));
    }
    let mut seen = vec![false; nodes.len()];
    let mut stack = vec![document.root()];
    while let Some(at) = stack.pop() {
        if seen[at] {
            return Err(format!("node {at} reached twice"));
        }
        seen[at] = true;
        for &child in &nodes[at].children {
            if nodes[child].parent != Some(at) {
                return Err(format!("node {child} names another parent"));
            }
            stack.push(child);
        }
        if let Some(contents) = nodes[at].element().and_then(|e| e.template_contents) {
            if nodes[contents].parent.is_some() {
                return Err("a template's content has a parent".into());
            }
            stack.push(contents);
        }
    }
    Ok(document)
}

#[test]
fn every_xml_document_reads_back_as_written_or_refuses_its_defect() {
    let mut whole = 0;
    for (n, input) in inputs(3000, 1500).enumerate() {
        let doc = xml(&mut Choices::new(&input));
        if let Err(why) = xml_reads_back(&doc) {
            panic!("input {n}: {why}\n{}", String::from_utf8_lossy(&doc.bytes));
        }
        whole += usize::from(doc.defect.is_none() && doc.declaration.is_none());
    }
    assert!(whole > 100, "too few documents were whole");
}

#[test]
fn every_soup_builds_a_tree() {
    for (n, input) in inputs(3000, 1500).enumerate() {
        let doc = html(&mut Choices::new(&input));
        if let Err(why) = html_builds_a_tree(&doc) {
            panic!("input {n}: {why}");
        }
    }
}

/// A defect the writer was asked for is written: each of the four is
/// refused as itself somewhere in a run of inputs.
#[test]
fn every_defect_and_declaration_is_written() {
    let mut defects = Vec::new();
    let mut declarations = Vec::new();
    for input in inputs(600, 800) {
        let doc = xml(&mut Choices::new(&input));
        defects.extend(doc.defect);
        declarations.extend(doc.declaration);
    }
    for want in [
        Defect::UndeclaredPrefix,
        Defect::MismatchedEnd,
        Defect::DuplicateAttribute,
        Defect::BareAmpersand,
    ] {
        assert!(defects.contains(&want), "{want:?} was never written");
    }
    for want in [
        Declaration::Bare,
        Declaration::External,
        Declaration::Subset,
    ] {
        assert!(declarations.contains(&want), "{want:?} was never written");
    }
}

/// The plainest input is the plainest document: the first element name,
/// empty, in both languages.
#[test]
fn the_empty_input_writes_one_empty_element() {
    let doc = xml(&mut Choices::new(&[]));
    assert!(
        doc.bytes == b"<div></div>" || doc.bytes == b"<div/>",
        "{:?}",
        String::from_utf8_lossy(&doc.bytes)
    );
    assert_eq!(doc.expected.elements, vec![("div".to_string(), Vec::new())]);
    let soup = html(&mut Choices::new(&[]));
    assert_eq!(soup.bytes, b"<div></div>");
}

/// A document with nothing written yet, for the spelling helpers.
fn blank() -> XmlDocument {
    XmlDocument {
        bytes: Vec::new(),
        expected: Expected::default(),
        defect: None,
        declaration: None,
        prefixed: 0,
        defaults: 0,
        cdata: 0,
        references: 0,
        instructions: 0,
        depth: 0,
        line_ends: 0,
    }
}

/// The reading the generator expects of what it spells is §2.11's and
/// §3.3.3's: in character data a CR LF is one line feed and every other
/// character is itself; in an attribute value a literal tab, line feed or
/// CR LF is one space, a character reference is the character it names
/// whatever it is, and the quote the value is in is written as a reference.
#[test]
fn the_expected_reading_is_line_end_and_attribute_value_normalisation() {
    let pieces = vec![
        Piece::Literal("a\tb\nc\"'".to_string()),
        Piece::CrLf,
        Piece::Char('\t', false),
        Piece::Char('\r', true),
        Piece::Entity(0),
    ];
    let mut doc = blank();
    let (mut spelled, mut read) = (String::new(), String::new());
    xml_pieces(&pieces, None, &mut spelled, &mut read, &mut doc);
    assert_eq!(spelled, "a\tb\nc\"'\r\n&#9;&#xD;&lt;");
    assert_eq!(read, "a\tb\nc\"'\n\t\r<");
    let (mut spelled, mut read) = (String::new(), String::new());
    xml_pieces(&pieces, Some('"'), &mut spelled, &mut read, &mut doc);
    assert_eq!(spelled, "a\tb\nc&quot;'\r\n&#9;&#xD;&lt;");
    assert_eq!(read, "a b c\"' \t\r<");
    assert_eq!((doc.line_ends, doc.references), (2, 6));
    // And the reader agrees: the value read out of a start tag is the one
    // the model gives.
    let document = format!("<a v=\"{spelled}\"/>");
    let source = Source::new(document.as_bytes()).expect("UTF-8");
    let events: Vec<Event<'_>> = source
        .reader(&Limits::DEFAULT)
        .collect::<Result<_, _>>()
        .expect("well formed");
    match &events[0] {
        Event::Start(element) => assert_eq!(element.attributes()[0].value(), read),
        other => panic!("{other:?}"),
    }
}

/// The first input of a run whose document `wanted` accepts.
fn first_xml(wanted: impl Fn(&XmlDocument) -> bool) -> XmlDocument {
    inputs(5000, 1500)
        .map(|input| xml(&mut Choices::new(&input)))
        .find(|doc| wanted(doc))
        .expect("some input writes it")
}

/// Each of the four defects, written into a document with no declaration,
/// is refused as the error that names it, in both modes.
#[test]
fn each_defect_is_refused_as_itself_in_both_modes() {
    for (defect, want) in [
        (Defect::UndeclaredPrefix, Error::UndeclaredPrefix),
        (Defect::MismatchedEnd, Error::MismatchedEndTag),
        (Defect::DuplicateAttribute, Error::DuplicateAttribute),
        (
            Defect::BareAmpersand,
            Error::Unterminated(Construct::Reference),
        ),
    ] {
        let doc = first_xml(|d| d.defect == Some(defect) && d.declaration.is_none());
        for doctype in [Doctype::Refuse, Doctype::SkipExternalId] {
            let (_, _, refusal) = read(&doc.bytes, doctype);
            assert_eq!(refusal, Some(want), "{defect:?} under {doctype:?}");
        }
    }
}

/// A declaration of each form: `Refuse` refuses it before the root opens;
/// `SkipExternalId` refuses an internal subset before the root opens and
/// reads past a bare declaration or one with an external identifier to the
/// document it heads.
#[test]
fn a_declaration_is_refused_or_read_past_as_the_mode_says() {
    for declaration in [
        Declaration::Bare,
        Declaration::External,
        Declaration::Subset,
    ] {
        let doc = first_xml(|d| d.declaration == Some(declaration) && d.defect.is_none());
        let (_, opened, refusal) = read(&doc.bytes, Doctype::Refuse);
        assert_eq!(refusal, Some(Error::DoctypeUnsupported), "{declaration:?}");
        assert!(!opened, "{declaration:?} under Refuse opened the root");
        let (got, opened, refusal) = read(&doc.bytes, Doctype::SkipExternalId);
        if declaration == Declaration::Subset {
            assert_eq!(refusal, Some(Error::InternalSubset));
            assert!(!opened, "an internal subset opened the root");
        } else {
            assert_eq!(refusal, None, "{declaration:?} under SkipExternalId");
            assert_eq!(
                got, doc.expected,
                "{declaration:?} read otherwise than written"
            );
        }
    }
}

/// Namespace declarations, of a prefix and of the default namespace, are
/// read as bindings and never as attributes, and a prefixed name reads back
/// with its prefix.
#[test]
fn namespace_declarations_bind_and_are_not_attributes() {
    let doc = first_xml(|d| {
        d.defect.is_none() && d.declaration.is_none() && d.defaults >= 1 && d.prefixed >= 2
    });
    let (got, _, refusal) = read(&doc.bytes, Doctype::Refuse);
    assert_eq!(refusal, None);
    assert_eq!(got, doc.expected);
    for (_, attributes) in &got.elements {
        for (name, _) in attributes {
            assert!(name != "xmlns" && !name.starts_with("xmlns:"), "{name}");
        }
    }
    assert!(got.elements.iter().any(|(name, attributes)| {
        name.contains(':') || attributes.iter().any(|(a, _)| a.contains(':'))
    }));
}

/// Every soup feature the writer counts is written by some input, under
/// every declaration and every token cap the input can choose.
#[test]
fn every_soup_feature_and_bound_is_written() {
    let docs: Vec<HtmlDocument> = inputs(1500, 1500)
        .map(|input| html(&mut Choices::new(&input)))
        .collect();
    let some = |what: &str, wanted: &dyn Fn(&HtmlDocument) -> bool| {
        assert!(docs.iter().any(wanted), "no soup has {what}");
    };
    some("an omitted end tag", &|d| d.omitted > 0);
    some("a stray end tag", &|d| d.stray > 0);
    some("mis-nested formatting elements", &|d| d.misnested > 0);
    some("fostered content", &|d| d.fostered > 0);
    some("a table", &|d| d.tables > 0);
    some("a template", &|d| d.templates > 0);
    some("a select", &|d| d.selects > 0);
    some("foreign content", &|d| d.foreign > 0);
    some("raw text", &|d| d.raw > 0);
    for doctype in 0..3u8 {
        some("each declaration", &|d| d.doctype == doctype);
    }
    for tokens in [1usize << 20, 64, 512, 16] {
        some("each token cap", &|d| d.limits.max_tokens == tokens);
    }
}

/// A soup parsed under a token cap it reaches stops there, says so, and
/// keeps the tree it built inside the cap.
#[test]
fn a_soup_past_its_token_cap_stops_inside_it() {
    let (doc, document) = inputs(5000, 1500)
        .map(|input| html(&mut Choices::new(&input)))
        .filter(|d| d.limits.max_tokens == 16)
        .map(|d| {
            let document = parser::parse_bytes(&d.bytes, &d.limits);
            (d, document)
        })
        .find(|(_, document)| document.stopped() == Some(Error::TokenCap))
        .expect("some soup reaches a cap of sixteen");
    assert!(document.nodes().len() <= doc.limits.max_tokens + 16);
    assert!(html_builds_a_tree(&doc).is_ok());
}

mod xml_seeds {
    use super::*;

    type Feature = fn(&XmlDocument) -> bool;

    fn whole(doc: &XmlDocument) -> bool {
        doc.defect.is_none() && doc.declaration.is_none()
    }

    /// Rewrites `fuzz/corpus/xml_structured/`: one seed per feature, each the
    /// first input of `first_input`'s sequence whose document has it.
    #[test]
    #[ignore = "writes fuzz/corpus/xml_structured"]
    fn write_the_fuzz_seeds() {
        let features: &[(&str, Feature)] = &[
            ("namespaces", |d| {
                whole(d) && d.prefixed >= 4 && d.defaults >= 1
            }),
            ("references", |d| whole(d) && d.references >= 8),
            ("cdata", |d| whole(d) && d.cdata >= 2),
            ("comments-and-instructions", |d| {
                whole(d) && d.instructions >= 3 && d.expected.comments.len() >= 2
            }),
            ("line-ends", |d| whole(d) && d.line_ends >= 4),
            ("deep", |d| whole(d) && d.depth >= 6),
            ("forty-elements", |d| {
                whole(d) && d.expected.elements.len() == MAX_ELEMENTS
            }),
            ("doctype-bare", |d| {
                d.declaration == Some(Declaration::Bare) && d.defect.is_none()
            }),
            ("doctype-external", |d| {
                d.declaration == Some(Declaration::External) && d.defect.is_none()
            }),
            ("doctype-subset", |d| {
                d.declaration == Some(Declaration::Subset)
            }),
            ("undeclared-prefix", |d| {
                d.defect == Some(Defect::UndeclaredPrefix) && d.declaration.is_none()
            }),
            ("mismatched-end", |d| {
                d.defect == Some(Defect::MismatchedEnd) && d.declaration.is_none()
            }),
            ("duplicate-attribute", |d| {
                d.defect == Some(Defect::DuplicateAttribute) && d.declaration.is_none()
            }),
            ("bare-ampersand", |d| {
                d.defect == Some(Defect::BareAmpersand) && d.declaration.is_none()
            }),
        ];
        let read = |data: &[u8]| {
            let mut choices = Choices::new(data);
            let _ = xml(&mut choices);
            choices.consumed()
        };
        let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
        for (name, has) in features {
            let taken: Vec<Vec<u8>> = seeds.iter().map(|(_, seed)| seed.clone()).collect();
            let seed = crate::first_input(
                2000,
                200_000,
                |data| {
                    let doc = xml(&mut Choices::new(data));
                    has(&doc) && xml_reads_back(&doc).is_ok()
                },
                read,
                &taken,
            )
            .unwrap_or_else(|| panic!("no input has {name}"));
            assert!(
                has(&xml(&mut Choices::new(&seed))),
                "{name} lost its feature when cut"
            );
            seeds.push((name, seed));
        }
        crate::write_seeds("xml_structured", &seeds);
    }
}

mod html_seeds {
    use super::*;

    type Feature = fn(&HtmlDocument) -> bool;

    /// Rewrites `fuzz/corpus/html_structured/`: one seed per feature, each
    /// the first input of `first_input`'s sequence whose soup has it.
    #[test]
    #[ignore = "writes fuzz/corpus/html_structured"]
    fn write_the_fuzz_seeds() {
        let features: &[(&str, Feature)] = &[
            ("foster-parenting", |d| {
                d.limits.max_tokens == 1 << 20 && d.fostered >= 3
            }),
            ("adoption-agency", |d| {
                d.limits.max_tokens == 1 << 20 && d.misnested >= 2
            }),
            ("tables", |d| {
                d.limits.max_tokens == 1 << 20 && d.tables >= 6
            }),
            ("template", |d| {
                d.limits.max_tokens == 1 << 20 && d.templates >= 2
            }),
            ("select", |d| {
                d.limits.max_tokens == 1 << 20 && d.selects >= 3
            }),
            ("foreign-content", |d| {
                d.limits.max_tokens == 1 << 20 && d.foreign >= 2
            }),
            ("raw-text", |d| d.limits.max_tokens == 1 << 20 && d.raw >= 2),
            ("omitted-end-tags", |d| {
                d.limits.max_tokens == 1 << 20 && d.omitted >= 4
            }),
            ("stray-end-tags", |d| {
                d.limits.max_tokens == 1 << 20 && d.stray >= 4
            }),
            ("quirks", |d| {
                d.limits.max_tokens == 1 << 20 && d.doctype == 2 && d.tables >= 2
            }),
            ("token-cap", |d| {
                d.limits.max_tokens == 64
                    && parser::parse_bytes(&d.bytes, &d.limits).stopped() == Some(Error::TokenCap)
            }),
        ];
        let read = |data: &[u8]| {
            let mut choices = Choices::new(data);
            let _ = html(&mut choices);
            choices.consumed()
        };
        let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
        for (name, has) in features {
            let taken: Vec<Vec<u8>> = seeds.iter().map(|(_, seed)| seed.clone()).collect();
            let seed = crate::first_input(
                2000,
                200_000,
                |data| {
                    let doc = html(&mut Choices::new(data));
                    has(&doc) && html_builds_a_tree(&doc).is_ok()
                },
                read,
                &taken,
            )
            .unwrap_or_else(|| panic!("no input has {name}"));
            assert!(
                has(&html(&mut Choices::new(&seed))),
                "{name} lost its feature when cut"
            );
            seeds.push((name, seed));
        }
        crate::write_seeds("html_structured", &seeds);
    }
}
