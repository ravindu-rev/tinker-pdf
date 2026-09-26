//! Link annotations and the document outline, round-tripped through this
//! repository's own reader (gap 31, milestone 5).
//!
//! # Why the round trip is the test
//!
//! Every other milestone in this gap checks a writer by reading its bytes back
//! with an assertion written beside the writer. This one can do better, and it
//! is the only one that can: the facade **already reads** outlines
//! (`Document::outline`) and link annotations (`Page::links`), built by gap 21
//! and the annotation work, so a document can be written, saved, opened through
//! the public API and compared against what was asked for.
//!
//! That is a stronger check than any assertion on a dictionary, for the reason
//! gap 30 milestone 5 gave about `MAX_FUNCTION_DEPTH`: **a writer whose output
//! its own reader cannot read is not a writer.** A `/Count` whose sign is wrong,
//! an `/A` dictionary the reader skips, a `/Parent` pointing at the wrong node —
//! none of those is visible in a dictionary comparison written by the same
//! person who wrote the dictionary.
//!
//! # Nothing here mentions EPUB
//!
//! Row 5's last clause is that nothing in the milestone may, and that is the
//! test of whether the work belongs in the writer rather than in a format. These
//! are PDF documents built from PDF vocabulary and held to ISO 32000-1's clause
//! numbers; the word appears in this file exactly once, in this paragraph,
//! saying why it does not appear anywhere else.

use tinker_pdf::{Document, WriteMode, WriteOptions};
use tinker_pdf_cos::dest::{Action, DestKind, Destination};
use tinker_pdf_cos::{DocumentBuilder, OutlineEntry, Target};

/// Builds a document, saves it, and opens it through the facade.
///
/// The save is a real one — `DocumentEditor::save`, the same path a caller
/// takes — so what is read back is a file rather than a builder's own tables.
fn round_trip(build: impl FnOnce(&mut DocumentBuilder)) -> Document {
    let mut builder = DocumentBuilder::new();
    build(&mut builder);
    let bytes = builder.finish();
    let document = Document::open(bytes).expect("the builder's own output opens");
    let saved = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Document::open(saved).expect("and it survives a rewrite")
}

/// Three pages, so a destination can name one that is not the first.
fn three_pages(builder: &mut DocumentBuilder) {
    for _ in 0..3 {
        builder.add_page(200.0, 300.0, |_| {});
    }
}

// ---- links ---------------------------------------------------------------

/// A link to a page in this document comes back as a `/GoTo` at the page it
/// named.
#[test]
fn an_internal_link_round_trips_to_the_page_it_names() {
    let document = round_trip(|builder| {
        three_pages(builder);
        builder.add_page(200.0, 300.0, |page| {
            assert!(page.link(
                10.0,
                20.0,
                90.0,
                40.0,
                &Target::Page {
                    index: 2,
                    view: DestKind::Fit,
                },
            ));
        });
    });

    let page = document.page(3).expect("the page carrying the link");
    let links = page.links();
    assert_eq!(links.len(), 1, "one link");
    let link = &links[0];

    // The rectangle, corners ordered as the reader orders them.
    assert_eq!(
        (link.rect.x0, link.rect.y0, link.rect.x1, link.rect.y1),
        (10.0, 20.0, 90.0, 40.0)
    );

    match &link.target {
        Some(Action::GoTo(Destination::Explicit {
            page_index, kind, ..
        })) => {
            assert_eq!(*page_index, Some(2), "the third page, zero-based");
            assert_eq!(*kind, DestKind::Fit);
        }
        other => panic!("expected a /GoTo to page 2, got {other:?}"),
    }
}

/// A link to a URI comes back as a `/URI` action, which is a different entry in
/// a different dictionary from the one above.
///
/// 12.5.6.5 makes `/Dest` and `/A` **alternatives**, so these are two ways of
/// saying where a link goes and not two spellings of one. A writer that emitted
/// only the first would round-trip an internal link perfectly and lose every
/// external one — which is why both have their own test rather than sharing a
/// fixture with two links in it.
#[test]
fn an_external_link_round_trips_as_a_uri_action() {
    let document = round_trip(|builder| {
        builder.add_page(200.0, 300.0, |page| {
            assert!(page.link(
                0.0,
                0.0,
                50.0,
                10.0,
                &Target::Uri("https://example.org/a".to_string()),
            ));
        });
    });

    let links = document.page(0).expect("a page").links();
    assert_eq!(links.len(), 1);
    match &links[0].target {
        Some(Action::Uri(uri)) => assert_eq!(uri.as_slice(), b"https://example.org/a"),
        other => panic!("expected a /URI action, got {other:?}"),
    }
}

/// Both kinds on one page, in `/Annots` order, because a page usually has both.
#[test]
fn a_page_carries_both_kinds_of_link_in_order() {
    let document = round_trip(|builder| {
        three_pages(builder);
        builder.add_page(200.0, 300.0, |page| {
            assert!(page.link(
                0.0,
                0.0,
                10.0,
                10.0,
                &Target::Page {
                    index: 1,
                    view: DestKind::Fit,
                },
            ));
            assert!(page.link(
                20.0,
                0.0,
                30.0,
                10.0,
                &Target::Uri("https://example.org/b".to_string()),
            ));
        });
    });

    let links = document.page(3).expect("a page").links();
    assert_eq!(links.len(), 2, "both, and in the order they were added");
    assert!(matches!(links[0].target, Some(Action::GoTo(_))));
    assert!(matches!(links[1].target, Some(Action::Uri(_))));
}

/// A target the writer cannot express is refused, and nothing is written.
///
/// Two consequences of one refusal and both are asserted: `link` answers false,
/// **and** the page comes back with no annotation at all. A build that returned
/// false after writing a half-formed `/Annots` entry would pass the first.
#[test]
fn a_target_that_cannot_be_written_is_refused_and_leaves_nothing_behind() {
    let document = round_trip(|builder| {
        builder.add_page(200.0, 300.0, |page| {
            // 12.6.4.7 makes a URI 7-bit ASCII; this one is not.
            assert!(
                !page.link(
                    0.0,
                    0.0,
                    10.0,
                    10.0,
                    &Target::Uri("https://exa\u{2014}ple".into())
                ),
                "a URI outside 7-bit ASCII is refused"
            );
        });
    });
    assert!(
        document.page(0).expect("a page").links().is_empty(),
        "and no annotation was left behind"
    );
}

/// A rectangle given with its corners the other way round comes back ordered.
///
/// 12.5.2 says a `/Rect` is stated with its corners in either order and *read*
/// normalised, so a writer that passed them through unchanged produces a
/// rectangle every conforming reader still understands — and one that this
/// engine's own reader hands back with `x0 > x1`. The injection matrix found no
/// test cared: every fixture until now gave its corners already in order, which
/// is the shape a writer's own author naturally writes.
#[test]
fn a_rectangle_given_backwards_is_written_normalised() {
    let document = round_trip(|builder| {
        builder.add_page(200.0, 300.0, |page| {
            // Bottom-right first, top-left second.
            assert!(page.link(
                90.0,
                40.0,
                10.0,
                20.0,
                &Target::Uri("https://example.org/".to_string()),
            ));
        });
    });

    let links = document.page(0).expect("a page").links();
    let rect = links[0].rect;
    assert_eq!(
        (rect.x0, rect.y0, rect.x1, rect.y1),
        (10.0, 20.0, 90.0, 40.0),
        "the corners are ordered, whichever way they arrived"
    );
    assert!(rect.x0 < rect.x1 && rect.y0 < rect.y1);
}

/// A rectangle with no area is refused, and nothing is written.
///
/// Two consequences again: `link` answers false **and** the page carries no
/// annotation. A zero-width link is one a reader cannot click and a viewer
/// draws nothing for, so writing it would be writing a lie about where the page
/// can be pressed.
#[test]
fn a_rectangle_with_no_area_is_refused_and_leaves_nothing_behind() {
    for (x0, y0, x1, y1, what) in [
        (10.0, 20.0, 10.0, 40.0, "no width"),
        (10.0, 20.0, 90.0, 20.0, "no height"),
    ] {
        let document = round_trip(|builder| {
            builder.add_page(200.0, 300.0, |page| {
                assert!(
                    !page.link(x0, y0, x1, y1, &Target::Uri("https://example.org/".into())),
                    "a rectangle with {what} is refused"
                );
            });
        });
        assert!(
            document.page(0).expect("a page").links().is_empty(),
            "and {what} left no annotation behind"
        );
    }
}

// ---- the outline ---------------------------------------------------------

/// The tree comes back with its titles, its nesting and its destinations.
#[test]
fn an_outline_round_trips_with_its_shape_and_its_destinations() {
    let document = round_trip(|builder| {
        three_pages(builder);
        builder.set_outline(vec![
            OutlineEntry {
                title: "Part One".to_string(),
                // 12.3.3 makes `/Dest` optional; a part title often points
                // nowhere itself.
                target: None,
                open: true,
                children: vec![
                    OutlineEntry {
                        title: "Chapter I".to_string(),
                        target: Some(Target::Page {
                            index: 0,
                            view: DestKind::Fit,
                        }),
                        open: true,
                        children: Vec::new(),
                    },
                    OutlineEntry {
                        title: "Chapter II".to_string(),
                        target: Some(Target::Page {
                            index: 1,
                            view: DestKind::Fit,
                        }),
                        open: true,
                        children: Vec::new(),
                    },
                ],
            },
            OutlineEntry {
                title: "Part Two".to_string(),
                target: Some(Target::Page {
                    index: 2,
                    view: DestKind::Fit,
                }),
                open: true,
                children: Vec::new(),
            },
        ]);
    });

    let outline = document.outline();
    assert_eq!(outline.len(), 2, "two top-level entries");
    assert_eq!(outline[0].title, "Part One");
    assert_eq!(outline[1].title, "Part Two");
    assert_eq!(outline[0].children.len(), 2, "and two beneath the first");
    assert_eq!(outline[0].children[0].title, "Chapter I");
    assert_eq!(outline[0].children[1].title, "Chapter II");

    // A heading with no destination is a real shape rather than a degraded one.
    assert!(outline[0].destination.is_none());
    match &outline[0].children[1].destination {
        Some(Destination::Explicit { page_index, .. }) => assert_eq!(*page_index, Some(1)),
        other => panic!("expected page 1, got {other:?}"),
    }
}

/// **`/Count`'s sign is what says open or closed (12.3.3), and the magnitude is
/// a separate claim.**
///
/// A closed entry carries the *negative* of its visible descendant count. Get
/// the sign wrong and the tree opens the wrong way in every viewer while
/// round-tripping perfectly through any reader that ignores it; get the
/// magnitude wrong and a viewer that trusts it draws the wrong number of rows.
/// So both are asserted, and from **both directions** — an open parent and a
/// closed one in one document, because a build that wrote one sign for
/// everything satisfies a test that only has the other.
#[test]
fn the_outlines_open_flag_is_the_sign_of_its_count_in_both_directions() {
    let child = |title: &str, index: u32| OutlineEntry {
        title: title.to_string(),
        target: Some(Target::Page {
            index,
            view: DestKind::Fit,
        }),
        open: true,
        children: Vec::new(),
    };
    let document = round_trip(|builder| {
        three_pages(builder);
        builder.set_outline(vec![
            OutlineEntry {
                title: "Open".to_string(),
                target: None,
                open: true,
                children: vec![child("A", 0), child("B", 1)],
            },
            OutlineEntry {
                title: "Closed".to_string(),
                target: None,
                open: false,
                children: vec![child("C", 2)],
            },
        ]);
    });

    let outline = document.outline();
    assert!(outline[0].open, "the first was written expanded");
    assert!(!outline[1].open, "and the second collapsed");

    // Both keep their children whichever way they were saved: `open` is a view
    // state, not a truncation.
    assert_eq!(outline[0].children.len(), 2);
    assert_eq!(outline[1].children.len(), 1);

    // And the magnitude, read out of the saved bytes, because the reader turns
    // the sign into a `bool` and throws the number away. `/Count 2` for the
    // open parent, `/Count -1` for the closed one.
    let saved = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    let text = String::from_utf8_lossy(&saved);
    assert!(
        text.contains("/Count 2"),
        "the open parent states two visible descendants"
    );
    assert!(
        text.contains("/Count -1"),
        "and the closed one states minus its own: {}",
        &text[..text.len().min(400)]
    );
}

/// An empty vector **clears** the outline, and the document then has none at
/// all rather than an empty one.
///
/// `set_outline`'s contract says so in as many words, and the distinction is
/// real: a document with no `/Outlines` and a document with an `/Outlines`
/// whose `/First` is absent are different files, and gap 21 is the whole gap
/// about telling absent from empty.
///
/// The clearing is what needed the test. Passing an empty vector to a builder
/// that never had an outline proves nothing — every implementation passes that,
/// including one whose empty case is `return true` and nothing else. So the
/// outline is set first, and then cleared.
#[test]
fn an_empty_outline_clears_one_that_was_already_set() {
    let document = round_trip(|builder| {
        three_pages(builder);
        assert!(builder.set_outline(vec![OutlineEntry {
            title: "Gone".to_string(),
            target: Some(Target::Page {
                index: 0,
                view: DestKind::Fit,
            }),
            open: true,
            children: Vec::new(),
        }]));
        assert!(
            builder.set_outline(Vec::new()),
            "an empty vector is accepted rather than refused"
        );
    });

    assert!(document.outline().is_empty(), "the entry is gone");
    let saved = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    let text = String::from_utf8_lossy(&saved);
    assert!(
        !text.contains("/Outlines"),
        "and no outline dictionary was written at all"
    );
    assert!(
        !text.contains("Gone"),
        "nor the title of the entry that was cleared"
    );
}

/// Nesting deeper than one level, so `/Parent` and the sibling chain are
/// exercised in a tree that has both.
#[test]
fn a_three_level_outline_keeps_every_level() {
    let document = round_trip(|builder| {
        three_pages(builder);
        builder.set_outline(vec![OutlineEntry {
            title: "One".to_string(),
            target: None,
            open: true,
            children: vec![OutlineEntry {
                title: "Two".to_string(),
                target: None,
                open: true,
                children: vec![OutlineEntry {
                    title: "Three".to_string(),
                    target: Some(Target::Page {
                        index: 2,
                        view: DestKind::Fit,
                    }),
                    open: true,
                    children: Vec::new(),
                }],
            }],
        }]);
    });

    let outline = document.outline();
    let flat = tinker_pdf_cos::OutlineItem::flatten(&outline);
    let seen: Vec<(u32, &str)> = flat
        .iter()
        .map(|(depth, item)| (*depth, item.title.as_str()))
        .collect();
    assert_eq!(seen, [(0, "One"), (1, "Two"), (2, "Three")]);
}

/// An outline the writer cannot express **leaves the one already set
/// standing**.
///
/// `set_outline`'s contract says so and the injection matrix found nothing
/// checked it: every fixture set one outline once, so a build that cleared the
/// document before validating the new tree passed. The distinction matters
/// because the failure is silent — a caller that ignores the `false` gets a
/// document with no navigation at all rather than the navigation it had.
#[test]
fn an_unwritable_outline_leaves_the_previous_one_standing() {
    let document = round_trip(|builder| {
        three_pages(builder);
        assert!(builder.set_outline(vec![OutlineEntry {
            title: "Kept".to_string(),
            target: Some(Target::Page {
                index: 0,
                view: DestKind::Fit,
            }),
            open: true,
            children: Vec::new(),
        }]));

        // A tree carrying a target that cannot be written: 12.6.4.7 makes a URI
        // 7-bit ASCII and this one is not.
        assert!(
            !builder.set_outline(vec![OutlineEntry {
                title: "Rejected".to_string(),
                target: Some(Target::Uri("https://exa\u{2014}ple".to_string())),
                open: true,
                children: Vec::new(),
            }]),
            "the new tree is refused"
        );
    });

    let outline = document.outline();
    assert_eq!(outline.len(), 1, "the old outline is still there");
    assert_eq!(outline[0].title, "Kept");
}

// ---- named destinations ---------------------------------------------------

/// Follows a name through the document's own `/Names /Dests` tree.
fn resolve(document: &Document, name: &[u8]) -> Option<Destination> {
    tinker_pdf_cos::dest::Resolver::new(document.cos()).resolve_named(name)
}

/// A link and an outline entry that use named destinations read back **as
/// names** (ruling 6), and each name resolves through the tree the builder
/// wrote to the page it was registered for.
///
/// Both halves are asserted because either alone is satisfied by a build that
/// flattened the name: the link reading back as the explicit array would
/// resolve to the right page and lose the name, and the name surviving with no
/// tree behind it would be a dead link that still reads as a name.
#[test]
fn a_named_link_and_outline_entry_read_back_as_names_that_resolve() {
    let document = round_trip(|builder| {
        assert!(builder.add_named_destination(b"chapter-2", 1, DestKind::Fit));
        assert!(builder.add_named_destination(
            b"the end",
            2,
            DestKind::Xyz {
                left: Some(10.0),
                top: Some(250.0),
                zoom: None,
            },
        ));
        builder.add_page(200.0, 300.0, |page| {
            assert!(page.link(
                10.0,
                10.0,
                90.0,
                30.0,
                &Target::Named(b"chapter-2".to_vec()),
            ));
        });
        builder.add_page(200.0, 300.0, |_| {});
        builder.add_page(200.0, 300.0, |_| {});
        assert!(builder.set_outline(vec![OutlineEntry {
            title: "End".to_string(),
            target: Some(Target::Named(b"the end".to_vec())),
            open: true,
            children: Vec::new(),
        }]));
        assert!(builder.dangling_destinations().is_empty());
    });

    let links = document.page(0).expect("the first page").links();
    assert_eq!(links.len(), 1);
    assert_eq!(
        links[0].target,
        Some(Action::GoTo(Destination::Named(b"chapter-2".to_vec()))),
        "the link keeps its name"
    );
    let outline = document.outline();
    assert_eq!(
        outline[0].destination,
        Some(Destination::Named(b"the end".to_vec())),
        "the outline entry keeps its name"
    );

    match resolve(&document, b"chapter-2") {
        Some(Destination::Explicit {
            page_index, kind, ..
        }) => {
            assert_eq!(page_index, Some(1), "chapter-2 is the second page");
            assert_eq!(kind, DestKind::Fit);
        }
        other => panic!("chapter-2 resolves to an explicit destination, got {other:?}"),
    }
    match resolve(&document, b"the end") {
        Some(Destination::Explicit {
            page_index, kind, ..
        }) => {
            assert_eq!(page_index, Some(2), "the end is the third page");
            assert_eq!(
                kind,
                DestKind::Xyz {
                    left: Some(10.0),
                    top: Some(250.0),
                    zoom: None,
                }
            );
        }
        other => panic!("the end resolves to an explicit destination, got {other:?}"),
    }
    assert!(
        resolve(&document, b"nowhere").is_none(),
        "and a name nobody registered resolves to nothing"
    );
    assert!(
        document.validate().is_empty(),
        "the strict validator is clean: {:?}",
        document.validate()
    );
}

/// A name nothing registers, and one registered for a page that never
/// arrives, are **dangling**: `dangling_destinations` names both before the
/// document is finished, and `finish` refuses both rather than writing a name
/// that resolves to nothing — the link is not written, and the outline entry
/// is written as a heading.
#[test]
fn a_dangling_name_is_refused_at_write_time_by_name() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_named_destination(b"late", 7, DestKind::Fit));
    assert!(builder.add_named_destination(b"here", 0, DestKind::Fit));
    builder.add_page(200.0, 300.0, |page| {
        assert!(page.link(0.0, 0.0, 50.0, 10.0, &Target::Named(b"nowhere".to_vec())));
        assert!(page.link(0.0, 20.0, 50.0, 30.0, &Target::Named(b"here".to_vec())));
    });
    assert!(builder.set_outline(vec![OutlineEntry {
        title: "Late".to_string(),
        target: Some(Target::Named(b"late".to_vec())),
        open: true,
        children: Vec::new(),
    }]));

    assert_eq!(
        builder.dangling_destinations(),
        vec![b"late".to_vec(), b"nowhere".to_vec()],
        "both dangling names, sorted, and not the one that resolves"
    );

    let document = Document::open(builder.finish()).expect("it opens");
    let links = document.page(0).expect("the page").links();
    assert_eq!(links.len(), 1, "the dangling link is not written at all");
    assert_eq!(
        links[0].target,
        Some(Action::GoTo(Destination::Named(b"here".to_vec()))),
        "the live one keeps its name"
    );
    assert_eq!(links[0].rect.y0, 20.0, "and it is the live one's rectangle");
    assert_eq!(
        document.outline()[0].destination,
        None,
        "the entry whose page never arrived is a heading"
    );
    assert!(
        resolve(&document, b"late").is_none(),
        "no tree entry for it"
    );
    assert!(document.validate().is_empty(), "{:?}", document.validate());
}

/// A name registered twice, an empty name and an unwritable view are refused
/// at registration, and an empty name cannot be linked to at all.
#[test]
fn a_name_that_cannot_be_one_is_refused() {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_named_destination(b"once", 0, DestKind::Fit));
    assert!(
        !builder.add_named_destination(b"once", 0, DestKind::FitB),
        "7.9.6 maps each key to one value"
    );
    assert!(!builder.add_named_destination(b"", 0, DestKind::Fit));
    assert!(!builder.add_named_destination(
        b"nan",
        0,
        DestKind::FitH {
            top: Some(f64::NAN)
        }
    ));
    builder.add_page(200.0, 300.0, |page| {
        assert!(!page.link(0.0, 0.0, 10.0, 10.0, &Target::Named(Vec::new())));
    });
    let document = Document::open(builder.finish()).expect("it opens");
    assert!(document.page(0).expect("the page").links().is_empty());
    match resolve(&document, b"once") {
        Some(Destination::Explicit { kind, .. }) => {
            assert_eq!(kind, DestKind::Fit, "the first registration stands");
        }
        other => panic!("once resolves, got {other:?}"),
    }
}

/// Enough names that the tree has to split into leaves under `/Kids`, and
/// every one of them still resolves to its own page.
#[test]
fn a_tree_of_many_names_resolves_every_one() {
    let document = round_trip(|builder| {
        three_pages(builder);
        for at in 0..200u32 {
            let name = format!("n{at:03}");
            assert!(builder.add_named_destination(name.as_bytes(), at % 3, DestKind::Fit));
        }
    });
    for at in 0..200u32 {
        let name = format!("n{at:03}");
        match resolve(&document, name.as_bytes()) {
            Some(Destination::Explicit { page_index, .. }) => {
                assert_eq!(page_index, Some(at % 3), "{name}");
            }
            other => panic!("{name} resolves, got {other:?}"),
        }
    }
}

/// The editor adds a name to a document that has a tree, and to one that has
/// none, and the old names survive the addition.
#[test]
fn the_editor_adds_a_named_destination_beside_the_ones_already_there() {
    let mut builder = DocumentBuilder::new();
    three_pages(&mut builder);
    assert!(builder.add_named_destination(b"old", 2, DestKind::Fit));
    let with_tree = builder.finish();

    let mut plain = DocumentBuilder::new();
    three_pages(&mut plain);
    let without_tree = plain.finish();

    for (bytes, had) in [(with_tree, true), (without_tree, false)] {
        let document = Document::open(bytes).expect("it opens");
        let mut editor = document.editor();
        assert!(editor.add_named_destination(b"new", 1, DestKind::FitB));
        assert!(
            !editor.add_named_destination(b"new", 0, DestKind::Fit),
            "a second registration of one name is refused"
        );
        assert!(!editor.add_named_destination(b"past", 9, DestKind::Fit));
        let saved = editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        });
        let reopened = Document::open(saved).expect("the update opens");
        match resolve(&reopened, b"new") {
            Some(Destination::Explicit {
                page_index, kind, ..
            }) => {
                assert_eq!(page_index, Some(1));
                assert_eq!(kind, DestKind::FitB);
            }
            other => panic!("new resolves, got {other:?}"),
        }
        match (had, resolve(&reopened, b"old")) {
            (true, Some(Destination::Explicit { page_index, .. })) => {
                assert_eq!(page_index, Some(2), "the old name survives");
            }
            (false, None) => {}
            (_, other) => panic!("old: expected it only where it was, got {other:?}"),
        }
        assert!(reopened.validate().is_empty(), "{:?}", reopened.validate());
    }
}
