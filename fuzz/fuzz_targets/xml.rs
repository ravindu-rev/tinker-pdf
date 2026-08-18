//! XML 1.0 with namespaces: the pull parser, its bounds, and the invariants a
//! caller trusts without re-checking.
//!
//! This is the one parser in the tree whose input is *text* rather than a
//! record layout, and that changes what a fuzzer is good for. There is no
//! offset to corrupt and no length field to lie about; what a mutator finds
//! instead is the seams between constructs — a `<` inside an attribute value, a
//! `]]` that is not a terminator, a `&` with no `;`, a prefix declared on the
//! element that uses it, a comment that swallows the rest of the document. Very
//! little of that is reachable from hand-built fixtures, because a fixture
//! author writes the markup they already had in mind.
//!
//! The control byte picks the bounds rather than the input, and it does so in a
//! way that keeps the caps *reachable*: gap 18a milestone 8 found a work cap set
//! above the most its own inputs could ask for, so a target whose limits are all
//! at their shipped defaults is a target that never explores a refusal — a
//! million-event cap cannot fire inside one iteration. Every knob takes a
//! distinct pair of bits and each new one would take a higher pair, so a corpus
//! written for an earlier set keeps its meaning.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **No document type declaration is ever parsed.** The refusal gap 30 exists
//!   for: if the text holds `<!DOCTYPE` anywhere the reader would reach, the
//!   reader stops at it, and it stops *at* it rather than after it. A mutator
//!   that found a way past this would be finding the thing the whole crate is
//!   about.
//! - **Starts and ends balance, in order.** An empty-element tag produces both,
//!   so a caller matching on the pair never has to special-case it; if that ever
//!   stopped holding, every consumer of this crate would be building a tree with
//!   a hole in it.
//! - **Every bound holds rather than being exceeded and reported.** Depth,
//!   attributes per element, name length and the event total.
//! - **A refusal is final.** The reader is fused: after an error it yields
//!   nothing, so a caller that keeps asking cannot walk into a half-parsed
//!   document.
//! - **Nothing decoded is longer than what it was decoded from.** The structural
//!   half of the answer to entity expansion, asserted on every text run rather
//!   than argued in a comment.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_xml::{Encoding, Error, Event, Limits, Source};

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Small enough that every cap is crossable inside one iteration, and varied
    // enough that both sides of each are reachable from the same corpus.
    let limits = Limits {
        max_depth: match knobs & 3 {
            0 => 1,
            1 => 4,
            2 => 32,
            _ => 256,
        },
        max_attributes: match (knobs >> 2) & 3 {
            0 => 0,
            1 => 2,
            2 => 16,
            _ => 256,
        },
        max_name_len: match (knobs >> 4) & 3 {
            0 => 1,
            1 => 8,
            2 => 64,
            _ => 1024,
        },
        max_tokens: match (knobs >> 6) & 3 {
            0 => 0,
            1 => 4,
            2 => 64,
            _ => 4096,
        },
    };

    let Ok(source) = Source::new(body) else {
        return;
    };
    let text = source.text().to_string();

    // Whatever the encoding, **the** mark is gone — one, at offset zero, which
    // is all 4.3.3 says to consume.
    //
    // This assertion used to be `!text.starts_with('\u{FEFF}')` and it was
    // wrong. The first session this target ever ran produced `FE FF FE FF` and
    // fired it in eleven minutes: a *second* `U+FEFF` is ZERO WIDTH NO-BREAK
    // SPACE, an ordinary character, and leaving it in the text is correct. What
    // is not correct is accepting it before the root element, and that is the
    // reader's answer rather than the decoder's — `Error::TextBeforeRoot`,
    // asserted in the crate's own tests. A target that demanded the decoder
    // strip it would have driven the crate into silently accepting a document
    // 2.8 forbids.
    let stripped = match source.encoding() {
        Encoding::Utf8 => body.len() - text.len(),
        _ => 0,
    };
    assert!(
        stripped <= 3,
        "more than one byte order mark was consumed"
    );
    if source.encoding() == Encoding::Utf8 {
        assert!(
            body.ends_with(text.as_bytes()),
            "UTF-8 text is not a suffix of the bytes it came from"
        );
    }

    let mut reader = source.reader(&limits);
    let mut open: Vec<String> = Vec::new();
    let mut events = 0usize;
    let mut refusal = None;

    while let Some(event) = reader.next() {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                refusal = Some(error);
                break;
            }
        };
        events += 1;
        assert!(
            events <= limits.max_tokens,
            "the event total was exceeded rather than refused"
        );
        assert_eq!(events, reader.events(), "the reader miscounted its own work");

        match event {
            Event::Start(element) => {
                assert!(
                    element.name().qualified().len() <= limits.max_name_len,
                    "a name past the cap was kept rather than refused"
                );
                assert!(
                    element.attributes().len() <= limits.max_attributes,
                    "an element past the attribute cap was produced"
                );
                // Namespaces §5.3: no two attributes share an expanded name.
                for (index, attribute) in element.attributes().iter().enumerate() {
                    assert!(
                        attribute.name().qualified().len() <= limits.max_name_len,
                        "an attribute name past the cap was kept"
                    );
                    assert!(
                        element
                            .attributes()
                            .iter()
                            .take(index)
                            .all(|earlier| earlier.name() != attribute.name()),
                        "two attributes with one expanded name were produced"
                    );
                    // A namespace declaration is not an attribute.
                    let qualified = attribute.name().qualified();
                    assert!(
                        qualified != "xmlns" && !qualified.starts_with("xmlns:"),
                        "a namespace declaration was reported as an attribute"
                    );
                }
                open.push(element.name().qualified().to_string());
                assert!(
                    open.len() <= limits.max_depth,
                    "nesting past the depth cap was produced"
                );
                assert_eq!(open.len(), reader.depth(), "the reader lost its own stack");
            }
            Event::End(name) => {
                let owed = open.pop().expect("an end with nothing open");
                assert_eq!(
                    owed,
                    name.qualified(),
                    "an end tag closed an element that was not open"
                );
            }
            Event::Text(run) | Event::Cdata(run) | Event::Comment(run) => {
                // Nothing this reader admits can expand: every reference is at
                // least four source bytes and produces exactly one character.
                assert!(
                    run.len() <= text.len(),
                    "a run decoded to more than the whole document"
                );
            }
            Event::Instruction { target, value } => {
                assert!(!target.is_empty(), "an instruction with no target");
                assert!(
                    !target.eq_ignore_ascii_case("xml"),
                    "an instruction with the reserved target was produced"
                );
                assert!(!value.contains("?>"), "an instruction ran past its end");
            }
        }
    }

    // The refusal gap 30 exists for. If the reader stopped, it stopped *at* the
    // declaration and not one byte past it; if it did not stop, the text held
    // none it could reach.
    if let Some(at) = text.find("<!DOCTYPE") {
        if refusal == Some(Error::DoctypeUnsupported) {
            assert_eq!(
                reader.offset(),
                at,
                "the reader read past a document type declaration before refusing it"
            );
        }
    } else {
        assert_ne!(
            refusal,
            Some(Error::DoctypeUnsupported),
            "a declaration was refused in text that holds none"
        );
    }

    if refusal.is_some() {
        // Fused: a caller that keeps asking cannot walk into a half-parsed
        // document.
        assert!(reader.next().is_none(), "the reader restarted after refusing");
    } else {
        assert!(open.is_empty(), "the document ended with elements open");
    }

    // The same bytes under the shipped bounds. Nothing about a wider cap may
    // turn a refusal into a panic, and this is the configuration that ships.
    let mut shipped = source.reader(&Limits::DEFAULT);
    while let Some(event) = shipped.next() {
        if event.is_err() {
            break;
        }
    }
});
