//! XML documents, generated: one element tree written well formed or with
//! one defect, read back in both document type modes.
//!
//! `xml` hands mutated text to the reader, and most mutations of a document
//! break it at the first seam they touch. This target reads its input as
//! choices (`tinker_pdf_fuzz::Choices`) and writes a document with
//! `tinker_pdf_fuzz::markup`: an element tree with names with and without a
//! prefix, attributes in no namespace, in a bound one and in `xml`'s, prefix
//! and default namespace declarations among the attributes, character data
//! with the five predefined entities, decimal and hexadecimal character
//! references across §2.2's ranges and CR LF line ends, CDATA sections,
//! comments and processing instructions inside and outside the root, and an
//! XML declaration or none. A document may carry one defect the generator
//! names — an undeclared prefix, an end tag closing the wrong element, a
//! duplicate attribute, a bare `&` — and a document type declaration before
//! the root: bare, with an external identifier, or with an internal subset.
//!
//! The caps — 40 elements, eight deep, five children and four attributes an
//! element — are `tinker_pdf_fuzz::markup`'s constants.
//!
//! What is asserted beyond "it did not panic", in both `Doctype` modes:
//!
//! - **A well-formed document reads back as written.** Every element, in
//!   document order, by its qualified name, with its attributes in order by
//!   theirs and their values as §3.3.3 normalises them — so the reader's count
//!   of elements is the generator's — the character data whole, references
//!   resolved and line ends normalised (§2.11), and every comment and
//!   instruction; no namespace declaration is read as an attribute, every
//!   start has its end, and nothing is refused.
//! - **A declaration is never parsed.** `Doctype::Refuse` refuses any one as
//!   `DoctypeUnsupported` before the root opens; `Doctype::SkipExternalId`
//!   refuses one with an internal subset as `InternalSubset`, before the root
//!   opens, and reads past one without, as though it were not there.
//! - **A defect is refused as itself**: `UndeclaredPrefix`, `MismatchedEndTag`,
//!   `DuplicateAttribute`, or an unterminated reference.
//!
//! # What this target cannot find, and what covers it instead
//!
//! The generator and the reader are both this repository's, so a clause both
//! read the same wrong way passes. `crates/tinker-pdf-xml`'s own tests hold
//! the reader to the specification's examples, and the XPS and EPUB suites
//! hold it to documents other producers wrote.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_fuzz::markup::{self, Declaration, Defect, Expected};
use tinker_pdf_fuzz::Choices;
use tinker_pdf_xml::{Construct, Doctype, Error, Event, Limits, Source};

/// What one mode read: what a well-formed document would be compared with,
/// whether the root had opened, and the refusal it stopped at.
fn read(bytes: &[u8], doctype: Doctype) -> (Expected, bool, Option<Error>) {
    let source =
        Source::new(bytes).unwrap_or_else(|e| panic!("a generated document did not decode: {e}"));
    let mut got = Expected::default();
    let mut depth = 0usize;
    let mut opened = false;
    let mut reader = source.reader_with(&Limits::DEFAULT, doctype);
    for event in reader.by_ref() {
        match event {
            Err(error) => return (got, opened, Some(error)),
            Ok(Event::Start(element)) => {
                opened = true;
                depth += 1;
                let attributes = element
                    .attributes()
                    .iter()
                    .map(|a| {
                        let name = a.name().qualified();
                        assert!(
                            name != "xmlns" && !name.starts_with("xmlns:"),
                            "a namespace declaration was read as an attribute"
                        );
                        (name.to_string(), a.value().to_string())
                    })
                    .collect();
                got.elements
                    .push((element.name().qualified().to_string(), attributes));
            }
            Ok(Event::End(_)) => {
                assert!(depth > 0, "an end with no start");
                depth -= 1;
            }
            Ok(Event::Text(text) | Event::Cdata(text)) => got.text.push_str(&text),
            Ok(Event::Comment(text)) => got.comments.push(text.to_string()),
            Ok(Event::Instruction { target, value }) => {
                got.instructions
                    .push((target.to_string(), value.to_string()));
            }
        }
    }
    assert_eq!(depth, 0, "a start with no end");
    (got, opened, None)
}

fuzz_target!(|data: &[u8]| {
    let doc = markup::xml(&mut Choices::new(data));
    for doctype in [Doctype::Refuse, Doctype::SkipExternalId] {
        let (got, opened, refusal) = read(&doc.bytes, doctype);
        let declared = match (doc.declaration, doctype) {
            (Some(_), Doctype::Refuse) => Some(Error::DoctypeUnsupported),
            (Some(Declaration::Subset), Doctype::SkipExternalId) => Some(Error::InternalSubset),
            _ => None,
        };
        if let Some(want) = declared {
            assert_eq!(refusal, Some(want), "{doctype:?} read a declaration");
            assert!(
                !opened,
                "{doctype:?} opened the root past a refused declaration"
            );
            continue;
        }
        let defect = doc.defect.map(|defect| match defect {
            Defect::UndeclaredPrefix => Error::UndeclaredPrefix,
            Defect::MismatchedEnd => Error::MismatchedEndTag,
            Defect::DuplicateAttribute => Error::DuplicateAttribute,
            Defect::BareAmpersand => Error::Unterminated(Construct::Reference),
        });
        match defect {
            Some(want) => assert_eq!(
                refusal,
                Some(want),
                "{doctype:?} did not refuse the defect {:?} as itself",
                doc.defect
            ),
            None => {
                assert_eq!(refusal, None, "{doctype:?} refused a well-formed document");
                assert_eq!(
                    got.elements.len(),
                    doc.expected.elements.len(),
                    "{doctype:?} read another number of elements"
                );
                assert_eq!(got, doc.expected, "{doctype:?} read otherwise than written");
            }
        }
    }
});
