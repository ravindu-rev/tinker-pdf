//! A whole document searched (`docs/design/document-search.md`): the page
//! text joined line to line, block to block and page to page, the three keep-
//! options, a hit across a page break forwards and backwards, and a
//! right-to-left phrase found as it is read.
//!
//! Every document here is built with `DocumentBuilder` and held to the
//! validate-on-save helper. Where a hit's quads are asserted they are held to
//! the quads the page's own search reports over the same glyphs, so the
//! cursor maps a hit to glyphs as `search_with` maps its own.

mod epub_support;
mod written_support;

use epub_support::typeface::Face;
use tinker_pdf::{
    Document, DocumentSearchOptions, HitPlace, PageQuads, Quad, SearchHit, SearchOptions,
    SearchPosition, UnsearchedReason,
};
use tinker_pdf_cos::build::{DocumentBuilder, Glyph, PlacedGlyph};

/// `line` as a PDF literal string's body in `WinAnsiEncoding`: ASCII as it
/// is, a Latin-1 letter by its code as an octal escape (`\351` is `é`).
fn escape(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        match c {
            '\\' | '(' | ')' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            '\u{A0}'..='\u{FF}' => out.push_str(&format!("\\{:03o}", u32::from(c))),
            other => panic!("{other:?} is no character this fixture's encoding draws"),
        }
    }
    out
}

/// A document of Helvetica pages: each page its blocks, each block its
/// lines, the lines of a block 14 points apart at 12 points and the blocks
/// 60 apart, far enough for the reader to part them.
fn helvetica(label: &str, pages: &[&[&[&str]]]) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for blocks in pages {
        let mut content = String::from("BT /F0 12 Tf ");
        let mut y = 760.0;
        for block in *blocks {
            for line in *block {
                content.push_str(&format!("1 0 0 1 40 {y} Tm ({}) Tj ", escape(line)));
                y -= 14.0;
            }
            y -= 46.0;
        }
        content.push_str("ET");
        builder.add_page(612.0, 792.0, |page| page.raw(content.as_bytes()));
    }
    let bytes = builder.finish();
    written_support::authored(label, &bytes)
}

/// A document of Helvetica pages of one block each.
fn pages(label: &str, pages: &[&[&str]]) -> Document {
    let blocks: Vec<[&[&str]; 1]> = pages.iter().map(|lines| [*lines]).collect();
    let pages: Vec<&[&[&str]]> = blocks.iter().map(|block| &block[..]).collect();
    helvetica(label, &pages)
}

/// Every hit, forwards from the start.
fn forwards(doc: &Document, needle: &str, options: &DocumentSearchOptions) -> Vec<SearchHit> {
    doc.search(needle, options).collect()
}

/// Every hit, backwards from the end, in the order found.
fn backwards(doc: &Document, needle: &str, options: &DocumentSearchOptions) -> Vec<SearchHit> {
    let mut search = doc.search(needle, options);
    search.seek(SearchPosition::end());
    let mut hits = Vec::new();
    while let Some(hit) = search.previous() {
        hits.push(hit);
    }
    hits
}

/// A hit's pages and their quads.
fn quads(hit: &SearchHit) -> &[PageQuads] {
    match &hit.place {
        HitPlace::Pages(pages) => pages,
        other => panic!("a page hit, not {other:?}"),
    }
}

/// The pages a hit covers.
fn pages_of(hit: &SearchHit) -> Vec<u32> {
    quads(hit).iter().map(|page| page.page).collect()
}

/// The quads page `page`'s own search reports for `needle` under `options`.
fn page_search(doc: &Document, page: u32, needle: &str, options: &SearchOptions) -> Vec<Quad> {
    doc.page(page)
        .expect("a page")
        .text()
        .search_with(needle, options)
}

fn options(search: SearchOptions) -> DocumentSearchOptions {
    DocumentSearchOptions {
        search,
        ..DocumentSearchOptions::default()
    }
}

const KEEP_LINES: DocumentSearchOptions = DocumentSearchOptions {
    search: SearchOptions {
        case_sensitive: false,
        whole_word: false,
        diacritic_insensitive: false,
    },
    keep_lines: true,
    keep_paragraphs: false,
    keep_hyphens: false,
};

/// **A phrase broken across a page break is one hit, forwards and
/// backwards**: `brown fox`, `brown` the last word of the first page and
/// `fox` the first of the second, is found by `next` from the start and by
/// `previous` from the end, as the same hit — two pages, the quad on each the
/// one the page's own search reports for its half — and its text is the
/// phrase.
#[test]
fn a_match_across_a_page_break_is_found_forwards_and_backwards() {
    let doc = pages(
        "a phrase across a page break",
        &[&["The quick", "brown"], &["fox jumps over", "the lazy dog"]],
    );
    let default = DocumentSearchOptions::default();
    let mut search = doc.search("brown fox", &default);
    let forward = search.next().expect("found forwards");
    assert!(search.next().is_none(), "found once");
    assert_eq!(pages_of(&forward), [0, 1]);
    assert_eq!(forward.text, "brown fox");
    let defaults = SearchOptions::default();
    assert_eq!(
        quads(&forward)[0].quads,
        page_search(&doc, 0, "brown", &defaults)
    );
    assert_eq!(
        quads(&forward)[1].quads,
        page_search(&doc, 1, "fox", &defaults)
    );
    assert_eq!(forward.after, " jumps over");
    assert_eq!(forward.before, "");

    let backward = backwards(&doc, "brown fox", &default);
    assert_eq!(backward, vec![forward.clone()]);

    // A phrase that runs over the break and a line beyond.
    let long = forwards(&doc, "quick brown fox jumps over the", &default);
    assert_eq!(long.len(), 1);
    assert_eq!(pages_of(&long[0]), [0, 1]);
    assert_eq!(quads(&long[0])[0].quads.len(), 2, "two lines of page 1");
    assert_eq!(quads(&long[0])[1].quads.len(), 2, "two lines of page 2");
    assert_eq!(long[0].text, "quick brown fox jumps over the");
}

/// **Backwards is forwards in reverse**, under every option and over
/// needles whose occurrences overlap, in chains that cross a line and a page
/// break: `a a` in `a a a a` is two hits, the first and the third
/// occurrence, whichever way the cursor walks.
#[test]
fn backwards_finds_the_forward_hits_in_reverse() {
    let doc = pages(
        "overlapping hits across a page break",
        &[
            &["x a a", "a a b"],
            &["a a a y", "z a a"],
            &["a a", "abab abab ab ab"],
        ],
    );
    let every = [
        DocumentSearchOptions::default(),
        KEEP_LINES,
        DocumentSearchOptions {
            keep_paragraphs: true,
            ..DocumentSearchOptions::default()
        },
        options(SearchOptions {
            whole_word: true,
            ..SearchOptions::default()
        }),
        DocumentSearchOptions {
            keep_hyphens: true,
            ..options(SearchOptions {
                case_sensitive: true,
                ..SearchOptions::default()
            })
        },
    ];
    for options in &every {
        for needle in [
            "a a", "a", "a a a", "abab", "ab ab", "b a", "y z", "x", "absent",
        ] {
            let ahead = forwards(&doc, needle, options);
            let mut behind = backwards(&doc, needle, options);
            behind.reverse();
            assert_eq!(behind, ahead, "{needle:?} under {options:?}");
            // Each hit ends at or before the next one starts.
            for pair in ahead.windows(2) {
                assert!(
                    pair[0].end() <= pair[1].start(),
                    "{needle:?}: overlapping hits"
                );
            }
        }
    }

    // `a a` over `x a`, `a a b a` and, past the page break, `a y`: the
    // occurrences start at `x a`'s a, at the second line's first a and at its
    // last. The second begins inside the first and is passed over, so one hit
    // crosses the line break and one the page break.
    let chains = pages(
        "chains across a line and a page break",
        &[&["x a", "a a b a"], &["a y"]],
    );
    let default = DocumentSearchOptions::default();
    let hits = forwards(&chains, "a a", &default);
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert!(hits.iter().all(|hit| hit.text == "a a"));
    assert_eq!(pages_of(&hits[0]), [0], "a hit across the line break");
    assert_eq!(quads(&hits[0])[0].quads.len(), 2);
    assert_eq!(pages_of(&hits[1]), [0, 1], "a hit across the page break");
    let mut behind = backwards(&chains, "a a", &default);
    behind.reverse();
    assert_eq!(behind, hits);
}

/// **`keep_lines` keeps every hit inside one line**: a phrase across a line
/// break is not found, nor one across a page break; a hit inside a line is;
/// and with `keep_hyphens` beside it each page's hits are the page's own
/// search's, quad for quad, needle for needle and option for option.
#[test]
fn keep_lines_keeps_every_hit_inside_one_line() {
    let doc = pages(
        "keep lines",
        &[
            &["The quick", "brown fox", "R\u{e9}sum\u{e9} resume  "],
            &["fox jumps", "  over the hyphen-", "ation of words"],
        ],
    );
    let default = DocumentSearchOptions::default();
    assert_eq!(forwards(&doc, "quick brown", &default).len(), 1);
    assert!(forwards(&doc, "quick brown", &KEEP_LINES).is_empty());
    assert!(forwards(&doc, "resume fox", &KEEP_LINES).is_empty());
    assert_eq!(forwards(&doc, "brown fox", &KEEP_LINES).len(), 1);
    assert!(forwards(&doc, "hyphenation", &KEEP_LINES).is_empty());

    let as_drawn = DocumentSearchOptions {
        keep_hyphens: true,
        ..KEEP_LINES
    };
    let searches = [
        SearchOptions::default(),
        SearchOptions {
            case_sensitive: true,
            ..SearchOptions::default()
        },
        SearchOptions {
            whole_word: true,
            ..SearchOptions::default()
        },
        SearchOptions {
            diacritic_insensitive: true,
            ..SearchOptions::default()
        },
    ];
    for search in searches {
        let cursor = DocumentSearchOptions {
            search,
            ..as_drawn.clone()
        };
        for needle in [
            "fox",
            "o",
            "resume",
            "R\u{e9}sum\u{e9}",
            "  ",
            "hyphen-",
            "e",
            "the",
        ] {
            let hits = forwards(&doc, needle, &cursor);
            for hit in &hits {
                assert_eq!(quads(hit).len(), 1, "{needle:?}: one page");
                assert_eq!(quads(hit)[0].quads.len(), 1, "{needle:?}: one line");
            }
            for page in 0..2 {
                let ours: Vec<Quad> = hits
                    .iter()
                    .filter(|hit| pages_of(hit) == [page])
                    .flat_map(|hit| quads(hit)[0].quads.clone())
                    .collect();
                assert_eq!(
                    ours,
                    page_search(&doc, page, needle, &search),
                    "{needle:?} on page {page} under {search:?}"
                );
            }
        }
    }
}

/// **`keep_paragraphs` stops a hit at a block**: two blocks on a page, a
/// phrase across their boundary found by default and not with the option; a
/// phrase across two lines of one block found with it; and a page break,
/// which ends a block, crossed by default only.
#[test]
fn keep_paragraphs_stops_a_hit_at_a_block() {
    let doc = helvetica(
        "keep paragraphs",
        &[
            &[
                &["The first", "paragraph ends"],
                &["here the second", "begins"],
            ],
            &[&["and goes on"]],
        ],
    );
    let lines = doc.page(0).expect("a page").text();
    assert_eq!(
        lines.blocks().len(),
        2,
        "two blocks: {:?}",
        lines.plain_text()
    );
    let default = DocumentSearchOptions::default();
    let paragraphs = DocumentSearchOptions {
        keep_paragraphs: true,
        ..DocumentSearchOptions::default()
    };
    assert_eq!(forwards(&doc, "ends here", &default).len(), 1);
    assert!(forwards(&doc, "ends here", &paragraphs).is_empty());
    let within = forwards(&doc, "first paragraph", &paragraphs);
    assert_eq!(within.len(), 1, "two lines of one block");
    assert_eq!(quads(&within[0])[0].quads.len(), 2);
    assert_eq!(forwards(&doc, "second begins", &paragraphs).len(), 1);
    assert_eq!(forwards(&doc, "begins and", &default).len(), 1);
    assert!(forwards(&doc, "begins and", &paragraphs).is_empty());
    // And backwards alike.
    assert!(backwards(&doc, "ends here", &paragraphs).is_empty());
    assert_eq!(backwards(&doc, "first paragraph", &paragraphs), within);
}

/// **`keep_hyphens` leaves a broken word broken**: `hyphen-` ending a line
/// before `ation` is `hyphenation` by default — on one page and across a page
/// break — and with the option it is `hyphen- ation`; a hyphen before a
/// capital is never rejoined, as plain text's rule has it.
#[test]
fn keep_hyphens_leaves_a_broken_word_broken() {
    let doc = pages(
        "keep hyphens",
        &[
            &[
                "the hyphen-",
                "ation of a Franco-",
                "Prussian war, and a page-",
            ],
            &["break here"],
        ],
    );
    let default = DocumentSearchOptions::default();
    let kept = DocumentSearchOptions {
        keep_hyphens: true,
        ..DocumentSearchOptions::default()
    };
    let joined = forwards(&doc, "hyphenation", &default);
    assert_eq!(joined.len(), 1);
    assert_eq!(
        joined[0].text, "hyphenation",
        "the hyphen is not in its text"
    );
    assert_eq!(quads(&joined[0])[0].quads.len(), 2, "both lines");
    // The hyphen's glyph reads as nothing and lies inside the hit, so the
    // first line's quad is the broken half as drawn, its hyphen with it.
    assert_eq!(
        quads(&joined[0])[0].quads[0],
        page_search(&doc, 0, "hyphen-", &SearchOptions::default())[0]
    );
    assert_eq!(
        quads(&joined[0])[0].quads[1],
        page_search(&doc, 0, "ation", &SearchOptions::default())[0]
    );
    assert!(forwards(&doc, "hyphenation", &kept).is_empty());
    assert_eq!(forwards(&doc, "hyphen- ation", &kept).len(), 1);
    assert!(forwards(&doc, "hyphen- ation", &default).is_empty());

    // Across the page break.
    let across = forwards(&doc, "pagebreak", &default);
    assert_eq!(across.len(), 1);
    assert_eq!(pages_of(&across[0]), [0, 1]);
    assert!(forwards(&doc, "pagebreak", &kept).is_empty());
    assert_eq!(forwards(&doc, "page- break", &kept).len(), 1);

    // A capital after the hyphen: never joined.
    assert!(forwards(&doc, "FrancoPrussian", &default).is_empty());
    assert_eq!(forwards(&doc, "Franco- Prussian", &default).len(), 1);
    assert_eq!(forwards(&doc, "Franco- Prussian", &kept).len(), 1);

    // Backwards alike.
    assert_eq!(backwards(&doc, "hyphenation", &default), joined);
    assert_eq!(backwards(&doc, "pagebreak", &default), across);
}

/// Shin, lamed, vav, final mem: `shalom`, as it is read.
const SHALOM: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}";

/// Ayin, vav, lamed, final mem: `olam`, as it is read.
const OLAM: &str = "\u{5E2}\u{5D5}\u{5DC}\u{5DD}";

/// Everything the synthesised face's pages draw.
const COVERS: &str = "\u{5E9}\u{5DC}\u{5D5}\u{5DD}\u{5E2} abcdefghijklmnopqrstuvwxyzDFT-.,'\u{e9}";

/// The face's advance at the size the pages draw it, twenty points.
const ADVANCE: f64 = 10.0;

/// A document of pages drawn in the synthesised face: each page's lines, each
/// a run of the characters given drawn left to right in that order, with the
/// text each glyph stands for — a character, or `\u{AD}` for a soft hyphen
/// drawn with the hyphen's glyph.
fn drawn(label: &str, pages: &[&[&str]]) -> Document {
    let face = Face::new("Fixture Search", COVERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureSearch", &program));
    let mut contents = Vec::new();
    for lines in pages {
        let mut content = Vec::new();
        for (row, line) in lines.iter().enumerate() {
            let texts: Vec<String> = line.chars().map(|ch| ch.to_string()).collect();
            let glyphs: Vec<PlacedGlyph<'_>> = line
                .chars()
                .zip(texts.iter())
                .enumerate()
                .map(|(i, (ch, text))| PlacedGlyph {
                    glyph: Glyph {
                        id: face
                            .glyph_of(if ch == '\u{AD}' { '-' } else { ch })
                            .expect("the face covers what the page draws"),
                        text,
                    },
                    x: i as f64 * ADVANCE,
                    rise: 0.0,
                })
                .collect();
            let baseline = 700.0 - 30.0 * row as f64;
            assert!(builder.glyph_run(
                &mut content,
                b"F0",
                20.0,
                [1.0, 0.0, 0.0, 1.0, 40.0, baseline],
                &glyphs
            ));
        }
        contents.push(content);
    }
    for content in &contents {
        builder.add_page(612.0, 792.0, |page| page.raw(content));
    }
    let bytes = builder.finish();
    written_support::authored(label, &bytes)
}

fn reversed(text: &str) -> String {
    text.chars().rev().collect()
}

/// **A right-to-left line continued across a page break is found as it is
/// read**: `shalom olam`, `shalom` the last line of one page and `olam` the
/// first of the next, each drawn in visual order. The page text is in logical
/// order (ruling 14), so the phrase is found in the order it is read, as one
/// hit over two pages, each page's quad the one its own search reports — the
/// right way round — and the phrase drawn is not found.
#[test]
fn a_right_to_left_line_continued_across_a_page_break_is_found_as_it_is_read() {
    let doc = drawn(
        "a right-to-left line across a page break",
        &[
            &["see the", &reversed(SHALOM)],
            &[&reversed(OLAM), "and more"],
        ],
    );
    let phrase = format!("{SHALOM} {OLAM}");
    let default = DocumentSearchOptions::default();
    let hits = forwards(&doc, &phrase, &default);
    assert_eq!(hits.len(), 1, "the phrase as read");
    assert_eq!(pages_of(&hits[0]), [0, 1]);
    assert_eq!(hits[0].text, phrase);
    let defaults = SearchOptions::default();
    let first = &quads(&hits[0])[0].quads;
    assert_eq!(*first, page_search(&doc, 0, SHALOM, &defaults));
    assert!(first[0].ul.0 < first[0].ur.0, "boxed the right way round");
    assert_eq!(
        quads(&hits[0])[1].quads,
        page_search(&doc, 1, OLAM, &defaults)
    );
    assert_eq!(backwards(&doc, &phrase, &default), hits);
    let drawn_order = format!("{} {}", reversed(SHALOM), reversed(OLAM));
    assert!(forwards(&doc, &drawn_order, &default).is_empty());
}

/// **A soft hyphen ending a page rejoins its word**, always, whatever starts
/// the next page, and every soft hyphen is removed from what a search reads
/// unless `keep_hyphens` keeps it.
#[test]
fn a_soft_hyphen_at_a_page_end_rejoins_the_word() {
    let doc = drawn(
        "a soft hyphen at a page end",
        &[&["the Donau\u{AD}"], &["Dampfschiff and sof\u{AD}t"]],
    );
    let default = DocumentSearchOptions::default();
    let joined = forwards(&doc, "DonauDampfschiff", &default);
    assert_eq!(joined.len(), 1);
    assert_eq!(pages_of(&joined[0]), [0, 1]);
    assert_eq!(joined[0].text, "DonauDampfschiff");
    assert_eq!(
        forwards(&doc, "soft", &default).len(),
        1,
        "removed mid-line"
    );
    let kept = DocumentSearchOptions {
        keep_hyphens: true,
        ..DocumentSearchOptions::default()
    };
    assert!(forwards(&doc, "DonauDampfschiff", &kept).is_empty());
    assert!(forwards(&doc, "soft", &kept).is_empty());
    assert_eq!(forwards(&doc, "sof\u{AD}t", &kept).len(), 1);
    assert_eq!(forwards(&doc, "Donau\u{AD} Dampfschiff", &kept).len(), 1);
}

/// **The page search's options behave as on one page**, across a break:
/// case, whole word and diacritics.
#[test]
fn the_search_options_behave_across_a_page_break_as_on_one_page() {
    let doc = pages(
        "options across a page break",
        &[&["a R\u{e9}sum\u{e9}"], &["Review concat cat"]],
    );
    let found =
        |needle: &str, search: SearchOptions| forwards(&doc, needle, &options(search)).len();
    let default = SearchOptions::default();
    assert_eq!(found("r\u{e9}sum\u{e9} review", default), 1);
    assert_eq!(
        found("resume review", default),
        0,
        "accents are significant"
    );
    let unaccented = SearchOptions {
        diacritic_insensitive: true,
        ..default
    };
    assert_eq!(found("resume review", unaccented), 1);
    let case = SearchOptions {
        case_sensitive: true,
        ..default
    };
    assert_eq!(found("R\u{e9}sum\u{e9} Review", case), 1);
    assert_eq!(found("r\u{e9}sum\u{e9} review", case), 0);
    let whole = SearchOptions {
        whole_word: true,
        ..default
    };
    assert_eq!(found("cat", default), 2);
    assert_eq!(found("cat", whole), 1, "not inside concat");
    assert_eq!(found("sum\u{e9} rev", whole), 0, "a part of each word");
    assert_eq!(found("r\u{e9}sum\u{e9} review", whole), 1);
}

/// **Seeking a page finds the hits that start there**: a hit that starts on
/// the page before and runs onto it is neither after the page's start nor
/// before it; `next` after `previous` returns the hit again; the cursor's
/// position is a hit's end after `next` and its start after `previous`.
#[test]
fn seeking_and_alternating_move_through_the_hits_as_a_list_iterator() {
    let doc = pages(
        "seeking",
        &[&["one cat", "two"], &["cat three cat"], &["four cat"]],
    );
    let default = DocumentSearchOptions::default();
    let all = forwards(&doc, "cat", &default);
    assert_eq!(all.len(), 4);
    let across = forwards(&doc, "two cat", &default);
    assert_eq!(pages_of(&across[0]), [0, 1]);

    let mut search = doc.search("cat", &default);
    search.seek(SearchPosition::page(1));
    let hit = search.next().expect("a hit on page 2");
    assert_eq!(hit, all[1]);
    assert_eq!(search.position(), *hit.end());
    let back = search.previous().expect("the same hit");
    assert_eq!(back, all[1], "previous after next returns it again");
    assert_eq!(search.position(), *hit.start());
    assert_eq!(search.next(), Some(all[1].clone()), "and next again");

    let mut crossing = doc.search("two cat", &default);
    crossing.seek(SearchPosition::page(1));
    assert!(crossing.next().is_none(), "it starts on the page before");
    crossing.seek(SearchPosition::page(1));
    assert!(
        crossing.previous().is_none(),
        "and ends after the page's start"
    );
    crossing.seek(SearchPosition::start());
    assert_eq!(crossing.next(), Some(across[0].clone()));

    // Past the end, then back.
    let mut search = doc.search("cat", &default);
    assert_eq!(search.by_ref().count(), 4);
    assert_eq!(search.position(), SearchPosition::end());
    assert_eq!(search.previous(), Some(all[3].clone()));
    search.seek(SearchPosition::page(9));
    assert!(search.next().is_none(), "a page past the last is the end");
    search.seek(SearchPosition::page(9));
    assert_eq!(search.previous(), Some(all[3].clone()));
    search.seek(SearchPosition::start());
    assert!(search.previous().is_none());
    assert_eq!(search.position(), SearchPosition::start());
}

/// **Nothing is found for a needle that is empty or that folding empties**,
/// in a document with no text, or past pages with none; a page with no text
/// between two joins nothing, and the phrase across it is one hit.
#[test]
fn empty_needles_documents_and_pages_find_what_they_hold() {
    let doc = pages(
        "pages with no text",
        &[&["the end of"], &[], &["chapter one"]],
    );
    let default = DocumentSearchOptions::default();
    assert!(forwards(&doc, "", &default).is_empty());
    let unaccented = options(SearchOptions {
        diacritic_insensitive: true,
        ..SearchOptions::default()
    });
    assert!(forwards(&doc, "\u{301}", &unaccented).is_empty());
    let across = forwards(&doc, "of chapter", &default);
    assert_eq!(across.len(), 1);
    assert_eq!(
        pages_of(&across[0]),
        [0, 2],
        "the empty page between joins nothing"
    );
    assert!(
        forwards(&doc, "of  chapter", &default).is_empty(),
        "one space"
    );
    assert_eq!(backwards(&doc, "of chapter", &default), across);

    let blank = pages("no text at all", &[&[], &[]]);
    assert!(forwards(&blank, "a", &default).is_empty());
    assert!(backwards(&blank, "a", &default).is_empty());
}

/// **White space at a line's ends is set aside where lines join**, so a
/// phrase broken across a line reads with one space in it whatever the
/// producer drew; and a needle of one space matches only a join, which covers
/// no glyph, so it is no hit.
#[test]
fn white_space_at_a_join_is_set_aside() {
    let doc = pages(
        "white space at line ends",
        &[&["   ends with spaces   ", "   starts with them"]],
    );
    let default = DocumentSearchOptions::default();
    let hits = forwards(&doc, "spaces starts", &default);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "spaces starts");
    assert!(forwards(&doc, "spaces  starts", &default).is_empty());
    // The spaces between words are glyphs: each is a hit; the join is not,
    // and neither is the white space set aside at the lines' ends.
    let spaces = forwards(&doc, " ", &default);
    assert_eq!(spaces.len(), 4, "the word spaces, and not the join");
    // As drawn, each line is read whole.
    let as_drawn = DocumentSearchOptions {
        keep_hyphens: true,
        ..KEEP_LINES
    };
    assert_eq!(forwards(&doc, "   ends", &as_drawn).len(), 1);
    assert!(forwards(&doc, "   ends", &default).is_empty());
}

/// **A document no password has opened is named, not searched** (ruling 10):
/// the committed AES-256 fixture finds nothing until its password is given,
/// and says why in `unsearched`.
#[test]
fn an_encrypted_document_no_password_opened_is_named_not_searched() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/encrypted-aes256.pdf"),
    )
    .expect("the committed fixture");
    let doc = Document::open(bytes).expect("it opens");
    let default = DocumentSearchOptions::default();
    let mut locked = doc.search("AES-256", &default);
    assert!(locked.next().is_none());
    assert_eq!(locked.unsearched().len(), 1);
    assert!(locked.unsearched()[0].attachment_path.is_empty());
    assert_eq!(
        locked.unsearched()[0].reason,
        UnsearchedReason::PasswordRequired
    );
    doc.authenticate("open-sesame").expect("the user password");
    let mut open = doc.search("AES-256", &default);
    assert!(open.next().is_some(), "read once it is open");
    assert!(open.unsearched().is_empty());
}
