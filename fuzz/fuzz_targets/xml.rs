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
//! **Both [`Doctype`] modes run on every input, and neither takes a control
//! bit.** All eight bits of the control byte are already spent, and taking a
//! second byte would shift the body by one and change the meaning of every
//! corpus entry ever written. Running both modes costs one extra walk and buys
//! something a bit could not: the two runs are *compared*, and the comparison
//! is the strongest assertion in this file.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **No document type declaration is ever parsed, in either mode.** The
//!   refusal gap 30 exists for and the relaxation gap 31 added beside it: if the
//!   reader stopped at a declaration it stopped *at* it, and if it stopped at an
//!   internal subset it stopped at the `[` — with nothing inside read. A mutator
//!   that found a way past either would be finding the thing the whole crate is
//!   about.
//! - **The strict mode's events are a prefix of the relaxed mode's.** The two
//!   are the same reader until the first `<!DOCTYPE`, where one stops and the
//!   other may continue; anything else means the mode changed the parser rather
//!   than the construct. This is what would catch a `SkipExternalId` that ended
//!   a declaration inside a string literal and re-entered the document
//!   mid-declaration — the one way relaxing `<!DOCTYPE` could reintroduce what
//!   refusing it closed.
//! - **The identifier warning is a biconditional.** It is recorded exactly when
//!   an external identifier was read and was outside EPUB 3.3 Appendix B's set.
//!   A build that warned about everything and a build that warned about nothing
//!   both fail, and a one-sided assertion would catch neither.
//! - **Each mode's refusals are its own.** `DoctypeUnsupported` never reaches
//!   the relaxed mode and the three names the relaxed mode added never reach the
//!   strict one.
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

use tinker_pdf_xml::{
    Construct, Doctype, Encoding, Error, Event, ExternalId, Limits, Source, Warning,
};

/// What one mode made of the input.
struct Run<'a> {
    events: Vec<Event<'a>>,
    refusal: Option<Error>,
    offset: usize,
    warnings: Vec<Warning>,
    identifier: Option<ExternalId<'a>>,
}

/// One mode's walk, with every invariant that is a property of a single run.
///
/// The cross-mode comparison is the caller's, because it needs both.
fn walk<'s>(source: &'s Source<'_>, limits: &Limits, doctype: Doctype, text: &str) -> Run<'s> {
    let mut reader = source.reader_with(limits, doctype);
    let mut open: Vec<&'s str> = Vec::new();
    let mut events: Vec<Event<'s>> = Vec::new();
    let mut refusal = None;

    while let Some(event) = reader.next() {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                refusal = Some(error);
                break;
            }
        };
        events.push(event);
        assert!(
            events.len() <= limits.max_tokens,
            "the event total was exceeded rather than refused"
        );
        assert_eq!(
            events.len(),
            reader.events(),
            "the reader miscounted its own work"
        );

        match events.last().expect("one was just pushed") {
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
                open.push(element.name().qualified());
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

    let offset = reader.offset();

    // The refusal gap 30 exists for, and the two gap 31 added beside it.
    //
    // Compared against the text **at the offset** rather than against the first
    // `<!DOCTYPE` in the document, which is not necessarily the one that
    // refused. That is a defect this target carried until gap 31's milestone 2:
    // `<!--<!DOCTYPE--><!DOCTYPE a><a/>` holds two, only the second is a
    // declaration, and the old form asserted the reader had stopped at the one
    // inside the comment. Gap 30's milestone 9 found this target's own
    // invariant wrong once already; this is the second time.
    let at = text.get(offset..).unwrap_or("");
    match refusal {
        Some(Error::DoctypeUnsupported | Error::MisplacedDoctype) => assert!(
            at.starts_with("<!DOCTYPE"),
            "the reader read past a document type declaration before refusing it"
        ),
        Some(Error::InternalSubset) => assert!(
            at.starts_with('['),
            "the reader read into an internal subset before refusing it"
        ),
        _ => {}
    }
    if !text.contains("<!DOCTYPE") {
        assert!(
            !matches!(
                refusal,
                Some(
                    Error::DoctypeUnsupported
                        | Error::MisplacedDoctype
                        | Error::InternalSubset
                        | Error::MalformedDoctype
                        | Error::Unterminated(Construct::Doctype)
                )
            ),
            "a declaration was refused in text that holds none"
        );
    }

    // Each mode answers by its own names and never by the other's.
    match doctype {
        Doctype::Refuse => {
            assert!(
                !matches!(
                    refusal,
                    Some(
                        Error::InternalSubset
                            | Error::MalformedDoctype
                            | Error::MisplacedDoctype
                            | Error::Unterminated(Construct::Doctype)
                    )
                ),
                "the strict mode refused by a name only the relaxed mode has"
            );
            assert!(
                reader.external_identifier().is_none(),
                "the strict mode read an external identifier"
            );
        }
        Doctype::SkipExternalId => assert_ne!(
            refusal,
            Some(Error::DoctypeUnsupported),
            "the relaxed mode refused by the name it exists to stop using"
        ),
    }

    // The warning and the identifier are one fact told twice, and they must
    // agree: recorded exactly when an identifier was read and was outside the
    // set. A build that warned about everything and one that warned about
    // nothing both fail here.
    assert_eq!(
        reader
            .warnings()
            .contains(&Warning::ExternalIdentifierNotAllowed),
        reader
            .external_identifier()
            .is_some_and(|identifier| !identifier.is_allowed()),
        "the identifier warning and the identifier disagree"
    );

    if refusal.is_some() {
        // Fused: a caller that keeps asking cannot walk into a half-parsed
        // document.
        assert!(
            reader.next().is_none(),
            "the reader restarted after refusing"
        );
    } else {
        assert!(open.is_empty(), "the document ended with elements open");
    }

    Run {
        events,
        refusal,
        offset,
        warnings: reader.warnings().to_vec(),
        identifier: reader.external_identifier(),
    }
}

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
    assert!(stripped <= 3, "more than one byte order mark was consumed");
    if source.encoding() == Encoding::Utf8 {
        assert!(
            body.ends_with(text.as_bytes()),
            "UTF-8 text is not a suffix of the bytes it came from"
        );
    }

    let strict = walk(&source, &limits, Doctype::Refuse, &text);
    let relaxed = walk(&source, &limits, Doctype::SkipExternalId, &text);

    // **The comparison.** The two modes are the same reader until the first
    // `<!DOCTYPE` the reader reaches: there the strict one always stops, and
    // the relaxed one either skips it and carries on or stops for a reason of
    // its own. So the strict run's events are a prefix of the relaxed run's,
    // always — and a `SkipExternalId` that ended a declaration inside a string
    // literal and resumed parsing mid-declaration would produce events out of a
    // literal, which is a *different* continuation rather than a longer one
    // only when the literal spells markup. What this catches unconditionally is
    // the other half: a relaxed mode that lost, reordered or altered anything
    // the strict mode had already produced.
    assert!(
        relaxed.events.starts_with(&strict.events),
        "the relaxed mode changed something the strict mode had already produced"
    );
    assert!(
        relaxed.warnings.starts_with(&strict.warnings),
        "the relaxed mode changed a warning the strict mode had already recorded"
    );
    assert!(
        relaxed.offset >= strict.offset,
        "the relaxed mode stopped earlier than the strict one"
    );
    // An identifier is only ever read by the mode that reads one.
    assert!(strict.identifier.is_none());
    if relaxed.identifier.is_some() {
        assert!(
            text.contains("<!DOCTYPE"),
            "an identifier was read out of text with no declaration in it"
        );
    }
    // A declaration the strict mode refused is the only thing that can make the
    // two disagree at all.
    if strict.refusal != relaxed.refusal {
        assert_eq!(
            strict.refusal,
            Some(Error::DoctypeUnsupported),
            "the two modes disagreed about something that is not a declaration"
        );
    }

    // The same bytes under the shipped bounds, in both modes. Nothing about a
    // wider cap may turn a refusal into a panic, and this is the configuration
    // that ships.
    for doctype in [Doctype::Refuse, Doctype::SkipExternalId] {
        let mut shipped = source.reader_with(&Limits::DEFAULT, doctype);
        while let Some(event) = shipped.next() {
            if event.is_err() {
                break;
            }
        }
    }
});
