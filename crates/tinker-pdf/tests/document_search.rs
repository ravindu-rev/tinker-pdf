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
    SearchPosition, SearchSources, UnsearchedReason,
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
    sources: SearchSources::ALL,
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

// ---- the sources beside the page text (FT-21.3) ------------------------------

use tinker_pdf::{
    AnnotationText, AttachmentText, DocumentEditor, EmbeddedFile, WriteMode, WriteOptions,
};
use tinker_pdf_cos::edit::annot;
use tinker_pdf_cos::pages::Rect;
use tinker_pdf_cos::{Dict, Object, PdfString};

/// A committed fixture under `testdata/`, by name.
fn testdata(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata")
            .join(name),
    )
    .expect("the committed fixture")
}

/// What `editor` writes, rewritten from `input` and held to the
/// validate-on-save helper, opened.
fn saved(label: &str, input: &[u8], editor: &DocumentEditor) -> Document {
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    written_support::rewritten(label, input, &bytes);
    Document::open(bytes).expect("the saved document opens")
}

/// A one-page Helvetica document's bytes, held as authored.
fn page_bytes(label: &str, lines: &[&str]) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    let mut content = String::from("BT /F0 12 Tf ");
    for (row, line) in lines.iter().enumerate() {
        let y = 760.0 - 14.0 * row as f64;
        content.push_str(&format!("1 0 0 1 40 {y} Tm ({}) Tj ", escape(line)));
    }
    content.push_str("ET");
    builder.add_page(612.0, 792.0, |page| page.raw(content.as_bytes()));
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}

/// The options with every source but the ones `off` turns off.
fn without(off: fn(&mut SearchSources)) -> DocumentSearchOptions {
    let mut sources = SearchSources::ALL;
    off(&mut sources);
    DocumentSearchOptions {
        sources,
        ..DocumentSearchOptions::default()
    }
}

/// **Each source is searched with a known hit** (FT-21.3): a built
/// annotation's `/Contents`, a value set through
/// `DocumentEditor::set_field_value`, `testdata/outline-3level.pdf`'s outline
/// title `Chapter 1`, and an attachment's name and description added through
/// `DocumentEditor::attach_file` — each found once, where it is, and not
/// found when its source is left out.
#[test]
fn each_source_is_searched_with_a_known_hit() {
    let default = DocumentSearchOptions::default();

    // An annotation's `/Contents`.
    let base = page_bytes("a page to annotate", &["the page text"]);
    let doc = Document::open(base.clone()).expect("it opens");
    let mut editor = doc.editor();
    let rect = Rect {
        x0: 20.0,
        y0: 30.0,
        x1: 40.0,
        y1: 50.0,
    };
    let note = annot::text_note(doc.cos(), rect, "A remark on the margin", false);
    editor.add_annotation(0, note).expect("page one is there");
    let annotated = saved("an annotated page", &base, &editor);
    let hits = forwards(&annotated, "remark", &default);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(
        hits[0].place,
        HitPlace::Annotation {
            page: 0,
            index: 0,
            rect: (20.0, 30.0, 40.0, 50.0),
            text: AnnotationText::Contents,
            range: 2..8,
        }
    );
    assert_eq!(hits[0].text, "remark");
    assert_eq!(hits[0].before, "A ");
    assert_eq!(hits[0].after, " on the margin");
    assert!(forwards(&annotated, "remark", &without(|s| s.annotations = false)).is_empty());

    // A field's value.
    let form = testdata("form-fields.pdf");
    let doc = Document::open(form.clone()).expect("the form opens");
    let mut editor = doc.editor();
    assert!(editor.set_field_value("notes", "Please call before noon"));
    let filled = saved("the form filled", &form, &editor);
    let hits = forwards(&filled, "call before", &default);
    assert_eq!(hits.len(), 1, "{hits:?}");
    let widgets = filled
        .form_fields()
        .into_iter()
        .find(|field| field.name == "notes")
        .expect("the field")
        .widgets;
    assert!(!widgets.is_empty());
    assert_eq!(
        hits[0].place,
        HitPlace::FormField {
            name: "notes".to_string(),
            widgets,
            value: 0,
            range: 7..18,
        }
    );
    assert!(forwards(&filled, "call before", &without(|s| s.form_fields = false)).is_empty());

    // An outline title, and on none of the pages.
    let outlined = Document::open(testdata("outline-3level.pdf")).expect("it opens");
    let hits = forwards(&outlined, "Chapter 1", &default);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(
        hits[0].place,
        HitPlace::Outline {
            path: vec![0, 0],
            titles: vec!["Part One".to_string(), "Chapter 1".to_string()],
            range: 0..9,
        }
    );
    let pages_only = DocumentSearchOptions {
        sources: SearchSources::PAGES,
        ..DocumentSearchOptions::default()
    };
    assert!(
        forwards(&outlined, "Chapter 1", &pages_only).is_empty(),
        "no page holds it"
    );
    assert!(forwards(&outlined, "Chapter 1", &without(|s| s.outline = false)).is_empty());

    // An attachment's name and description.
    let doc = Document::open(base.clone()).expect("it opens");
    let mut editor = doc.editor();
    editor
        .attach_file(&EmbeddedFile {
            name: "figures-2026.csv".to_string(),
            filename: "figures-2026.csv".to_string(),
            description: Some("The quarterly figures, as a table".to_string()),
            mime_type: Some("text/csv".to_string()),
            created: None,
            modified: None,
            data: b"quarter,value\nQ1,12\n".to_vec(),
        })
        .expect("attachable");
    let attached = saved("a page with an attachment", &base, &editor);
    let hits = forwards(&attached, "figures", &default);
    let places: Vec<&HitPlace> = hits.iter().map(|hit| &hit.place).collect();
    assert_eq!(
        places,
        [
            &HitPlace::Attachment {
                name: "figures-2026.csv".to_string(),
                text: AttachmentText::Name,
                range: 0..7,
            },
            &HitPlace::Attachment {
                name: "figures-2026.csv".to_string(),
                text: AttachmentText::Description,
                range: 14..21,
            },
        ],
        "the name and the description once each, the file name being the name"
    );
    assert!(forwards(&attached, "figures", &without(|s| s.attachments = false)).is_empty());
}

/// **An annotation's strings**: `/RC`'s character data — its paragraphs read
/// as lines, no tag ever found — `/Subj` and a markup annotation's author;
/// a pop-up, whose text is its parent's, passed over so the parent's is
/// found once.
#[test]
fn an_annotations_rich_text_subject_and_author_are_searched() {
    let base = page_bytes("a page to annotate richly", &["plain page"]);
    let doc = Document::open(base.clone()).expect("it opens");
    let cos = doc.cos();
    let mut editor = doc.editor();
    let rect = Rect {
        x0: 10.0,
        y0: 10.0,
        x1: 30.0,
        y1: 30.0,
    };
    let mut note = annot::text_note(cos, rect, "The note's own words", true);
    note.insert(
        cos.intern(b"RC"),
        Object::String(PdfString::literal(
            b"<?xml version=\"1.0\"?><body xmlns=\"http://www.w3.org/1999/xhtml\"><p>Some <b>bold words</b></p><p>a second paragraph</p></body>"
                .to_vec(),
        )),
    );
    note.insert(
        cos.intern(b"Subj"),
        Object::String(PdfString::literal(b"Margin query".to_vec())),
    );
    note.insert(
        cos.intern(b"T"),
        Object::String(PdfString::literal(b"A. Reviewer".to_vec())),
    );
    let parent = editor.add_annotation(0, note).expect("page one");
    let mut popup = Dict::new();
    popup.insert(cos.intern(b"Type"), Object::Name(cos.intern(b"Annot")));
    popup.insert(cos.intern(b"Subtype"), Object::Name(cos.intern(b"Popup")));
    popup.insert(
        cos.intern(b"Rect"),
        Object::Array(vec![
            Object::Real(40.0),
            Object::Real(40.0),
            Object::Real(140.0),
            Object::Real(90.0),
        ]),
    );
    popup.insert(cos.intern(b"Parent"), Object::Ref(parent));
    editor.add_annotation(0, popup).expect("page one");
    let annotated = saved("a richly annotated page", &base, &editor);

    let default = DocumentSearchOptions::default();
    let text_of = |needle: &str| -> Vec<(AnnotationText, std::ops::Range<usize>)> {
        forwards(&annotated, needle, &default)
            .into_iter()
            .map(|hit| match hit.place {
                HitPlace::Annotation {
                    index, text, range, ..
                } => {
                    assert_eq!(index, 0, "the note, never its pop-up");
                    (text, range)
                }
                other => panic!("an annotation hit, not {other:?}"),
            })
            .collect()
    };
    assert_eq!(text_of("own words"), [(AnnotationText::Contents, 11..20)]);
    assert_eq!(text_of("bold words"), [(AnnotationText::RichText, 5..15)]);
    assert_eq!(
        text_of("words a second"),
        [(AnnotationText::RichText, 10..24)],
        "two paragraphs of rich text are two lines, joined"
    );
    assert!(text_of("xmlns").is_empty(), "no markup is text");
    assert!(text_of("body").is_empty());
    assert_eq!(text_of("query"), [(AnnotationText::Subject, 7..12)]);
    assert_eq!(text_of("reviewer"), [(AnnotationText::Author, 3..11)]);
    let lines = DocumentSearchOptions {
        keep_lines: true,
        ..DocumentSearchOptions::default()
    };
    assert!(forwards(&annotated, "words a second", &lines).is_empty());
}

/// **The sources are read in their order**, the page text first and the
/// attachments last, so backwards is forwards in reverse across them, and a
/// file name that is not the name is searched.
#[test]
fn the_sources_are_read_in_order_and_backwards_in_reverse() {
    let form = testdata("form-fields.pdf");
    let doc = Document::open(form.clone()).expect("the form opens");
    let mut editor = doc.editor();
    assert!(editor.set_field_value("notes", "the fixture value"));
    let rect = Rect {
        x0: 20.0,
        y0: 20.0,
        x1: 40.0,
        y1: 40.0,
    };
    editor
        .add_annotation(
            0,
            annot::text_note(doc.cos(), rect, "a fixture remark", false),
        )
        .expect("page one");
    assert!(editor.set_outline(&[tinker_pdf::OutlineEntry {
        title: "The fixture".to_string(),
        target: None,
        open: false,
        children: Vec::new(),
    }]));
    editor
        .attach_file(&EmbeddedFile {
            name: "data".to_string(),
            filename: "fixture-data.csv".to_string(),
            description: None,
            mime_type: None,
            created: None,
            modified: None,
            data: b"a,b\n".to_vec(),
        })
        .expect("attachable");
    let all = saved("a form with every source", &form, &editor);

    let default = DocumentSearchOptions::default();
    let hits = forwards(&all, "fixture", &default);
    let kinds: Vec<&str> = hits
        .iter()
        .map(|hit| match &hit.place {
            HitPlace::Pages(_) => "page",
            HitPlace::Annotation { .. } => "annotation",
            HitPlace::FormField { .. } => "field",
            HitPlace::Outline { .. } => "outline",
            HitPlace::Attachment { text, .. } => {
                assert_eq!(*text, AttachmentText::FileName);
                "attachment"
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        ["page", "annotation", "field", "outline", "attachment"]
    );
    let mut behind = backwards(&all, "fixture", &default);
    behind.reverse();
    assert_eq!(behind, hits);
    for pair in hits.windows(2) {
        assert!(pair[0].end() <= pair[1].start());
    }

    // Seeking a page's start passes over the page text before it, never a
    // source after the pages.
    let mut search = all.search("fixture", &default);
    search.seek(SearchPosition::page(1));
    assert_eq!(
        search.next(),
        Some(hits[1].clone()),
        "the annotation, after every page"
    );
    assert_eq!(search.previous(), Some(hits[1].clone()));
    assert_eq!(search.previous(), Some(hits[0].clone()));
    assert!(search.previous().is_none());

    // The page text alone.
    let pages_only = DocumentSearchOptions {
        sources: SearchSources::PAGES,
        ..DocumentSearchOptions::default()
    };
    assert_eq!(
        forwards(&all, "fixture", &pages_only),
        vec![hits[0].clone()]
    );
    assert_eq!(SearchSources::default(), SearchSources::ALL);
}

// ---- attached documents (FT-21.4) --------------------------------------------

use tinker_pdf::{
    Encryption, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_DEPTH, MAX_SEARCHED_ATTACHMENTS,
};

/// `base` with `files` attached, each a name and its bytes, saved with its
/// streams compressed and held to the validate-on-save helper.
fn with_attachments(label: &str, base: &[u8], files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let doc = Document::open(base.to_vec()).expect("it opens");
    let mut editor = doc.editor();
    for (name, data) in files {
        editor
            .attach_file(&EmbeddedFile {
                name: name.clone(),
                filename: name.clone(),
                description: None,
                mime_type: None,
                created: None,
                modified: None,
                data: data.clone(),
            })
            .expect("attachable");
    }
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        compress: true,
        ..WriteOptions::default()
    });
    written_support::rewritten(label, base, &bytes);
    bytes
}

/// Every hit forwards, and what the search could not read once it had read
/// everything.
fn searched(
    doc: &Document,
    needle: &str,
    options: &DocumentSearchOptions,
) -> (Vec<SearchHit>, Vec<tinker_pdf::Unsearched>) {
    let mut search = doc.search(needle, options);
    let hits: Vec<SearchHit> = search.by_ref().collect();
    (hits, search.unsearched().to_vec())
}

fn path(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// **A hit inside an attached PDF names its attachment** (FT-21.4): one
/// level down, and a PDF attached inside an attached PDF two levels down,
/// each hit carrying the attachments it was found under, read after the
/// document's own sources; an attachment that is no PDF is neither searched
/// nor named; backwards is forwards in reverse across them; and with the
/// attached documents left out, only the document's own hit is found.
#[test]
fn a_hit_inside_an_attached_pdf_names_its_attachment() {
    let inner = page_bytes("the innermost document", &["an inner needle here"]);
    let middle_base = page_bytes("the middle document", &["the middle page"]);
    let middle = with_attachments(
        "the middle document, the inner attached",
        &middle_base,
        &[("inner.pdf".to_string(), inner)],
    );
    let outer_base = page_bytes("the outer document", &["the outer page, a needle too"]);
    let outer = with_attachments(
        "the outer document, the middle attached",
        &outer_base,
        &[
            ("middle.pdf".to_string(), middle),
            ("notes.txt".to_string(), b"a needle in a text file".to_vec()),
        ],
    );
    let doc = Document::open(outer).expect("it opens");
    let default = DocumentSearchOptions::default();

    let (hits, unsearched) = searched(&doc, "needle", &default);
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert!(hits[0].attachment_path.is_empty(), "the outer page first");
    assert_eq!(pages_of(&hits[0]), [0]);
    assert_eq!(hits[1].attachment_path, path(&["middle.pdf", "inner.pdf"]));
    assert_eq!(pages_of(&hits[1]), [0]);
    assert_eq!(hits[1].text, "needle");
    assert_eq!(hits[1].before, "an inner ");
    assert_eq!(hits[1].after, " here");
    assert!(
        unsearched.is_empty(),
        "a text file is no document: {unsearched:?}"
    );
    assert!(hits[0].end() <= hits[1].start());

    let (one_level, _) = searched(&doc, "middle page", &default);
    assert_eq!(one_level.len(), 1);
    assert_eq!(one_level[0].attachment_path, path(&["middle.pdf"]));

    let mut behind = backwards(&doc, "needle", &default);
    behind.reverse();
    assert_eq!(behind, hits);

    // Seeking into an attached document and back.
    let mut search = doc.search("needle", &default);
    search.seek(hits[1].start().clone());
    assert_eq!(search.next(), Some(hits[1].clone()));
    assert_eq!(search.previous(), Some(hits[1].clone()));
    assert_eq!(search.previous(), Some(hits[0].clone()));

    let own = DocumentSearchOptions {
        sources: SearchSources {
            attached_documents: false,
            ..SearchSources::ALL
        },
        ..DocumentSearchOptions::default()
    };
    let (alone, _) = searched(&doc, "needle", &own);
    assert_eq!(alone, vec![hits[0].clone()]);
}

/// **An attachment past the caps is reported, not searched** (ruling 10):
/// PDFs nested five deep, the fifth past `MAX_ATTACHMENT_DEPTH`, named with
/// its path while the four above it are searched; an attachment that
/// decodes to a byte more than `MAX_ATTACHMENT_BYTES`; and one more
/// attachment than `MAX_SEARCHED_ATTACHMENTS`, the one past it named.
#[test]
fn an_attachment_past_the_caps_is_reported_not_searched() {
    let default = DocumentSearchOptions::default();
    assert_eq!(MAX_ATTACHMENT_DEPTH, 4);

    // Five levels: each level's page says its number.
    let mut nested = page_bytes("level 5", &["deep needle 5"]);
    for level in (1..=4).rev() {
        let base = page_bytes(
            &format!("level {level}"),
            &[&format!("deep needle {level}")],
        );
        nested = with_attachments(
            &format!("level {level}, the next attached"),
            &base,
            &[(format!("level{}.pdf", level + 1), nested)],
        );
    }
    let outer_base = page_bytes("five levels down", &["the top"]);
    let outer = with_attachments(
        "five levels down, the first attached",
        &outer_base,
        &[("level1.pdf".to_string(), nested)],
    );
    let doc = Document::open(outer).expect("it opens");
    let (hits, unsearched) = searched(&doc, "deep needle", &default);
    let paths: Vec<Vec<String>> = hits.iter().map(|hit| hit.attachment_path.clone()).collect();
    assert_eq!(
        paths,
        [
            path(&["level1.pdf"]),
            path(&["level1.pdf", "level2.pdf"]),
            path(&["level1.pdf", "level2.pdf", "level3.pdf"]),
            path(&["level1.pdf", "level2.pdf", "level3.pdf", "level4.pdf"]),
        ],
        "four levels searched"
    );
    assert_eq!(
        unsearched,
        [tinker_pdf::Unsearched {
            attachment_path: path(&[
                "level1.pdf",
                "level2.pdf",
                "level3.pdf",
                "level4.pdf",
                "level5.pdf"
            ]),
            reason: UnsearchedReason::TooDeep,
        }],
        "the fifth named, not searched"
    );
    // Backwards, the same hits, and the same name once.
    let mut search = doc.search("deep needle", &default);
    search.seek(SearchPosition::end());
    let mut behind = Vec::new();
    while let Some(hit) = search.previous() {
        behind.push(hit);
    }
    behind.reverse();
    assert_eq!(behind, hits);
    assert_eq!(search.unsearched(), unsearched.as_slice());

    // A byte past the size cap.
    let base = page_bytes("a page with a large attachment", &["a large attachment"]);
    let large = with_attachments(
        "a large attachment",
        &base,
        &[("huge.bin".to_string(), vec![b' '; MAX_ATTACHMENT_BYTES + 1])],
    );
    let doc = Document::open(large).expect("it opens");
    let (hits, unsearched) = searched(&doc, "large", &default);
    assert_eq!(hits.len(), 1, "the page's own text");
    assert_eq!(
        unsearched,
        [tinker_pdf::Unsearched {
            attachment_path: path(&["huge.bin"]),
            reason: UnsearchedReason::TooLarge,
        }]
    );

    // One attachment more than the count cap: every one is read in name
    // order, and the last is named.
    let files: Vec<(String, Vec<u8>)> = (0..=MAX_SEARCHED_ATTACHMENTS)
        .map(|index| (format!("file-{index:03}"), b"plain".to_vec()))
        .collect();
    let base = page_bytes("a page with many attachments", &["many"]);
    let many = with_attachments("many attachments", &base, &files);
    let doc = Document::open(many).expect("it opens");
    assert_eq!(doc.attachments().len(), MAX_SEARCHED_ATTACHMENTS + 1);
    let (_, unsearched) = searched(&doc, "plain", &default);
    assert_eq!(
        unsearched,
        [tinker_pdf::Unsearched {
            attachment_path: path(&[&format!("file-{MAX_SEARCHED_ATTACHMENTS:03}")]),
            reason: UnsearchedReason::TooMany,
        }]
    );
}

/// **An encrypted attachment is reported by name**: the committed AES-256
/// fixture attached, which the empty password does not open, is named with
/// `PasswordRequired` and nothing in it is found; one encrypted only to
/// restrict permissions opens with the empty password and is searched; and
/// an attachment with no stream is named `Unreadable`.
#[test]
fn an_encrypted_attachment_is_reported_by_name() {
    let default = DocumentSearchOptions::default();
    let base = page_bytes("a page with encrypted attachments", &["the cover page"]);

    let plain = page_bytes("an attachment to encrypt", &["an open secret"]);
    let doc = Document::open(plain.clone()).expect("it opens");
    let encrypted = doc.editor().save(&WriteOptions {
        encryption: Some(Encryption {
            user_password: String::new(),
            owner_password: "owner".to_string(),
            permissions: -4,
            entropy: [0x5A; 48],
        }),
        ..WriteOptions::default()
    });
    written_support::rewritten(
        "an attachment encrypted to restrict permissions",
        plain.as_slice(),
        written_support::Locked {
            bytes: encrypted.clone(),
            key: written_support::Key::Password(""),
        },
    );

    let outer = with_attachments(
        "a page with encrypted attachments",
        &base,
        &[
            ("locked.pdf".to_string(), testdata("encrypted-aes256.pdf")),
            ("open.pdf".to_string(), encrypted),
        ],
    );
    let doc = Document::open(outer).expect("it opens");
    let (hits, unsearched) = searched(&doc, "AES-256", &default);
    assert!(
        hits.is_empty(),
        "nothing read from the locked one: {hits:?}"
    );
    assert_eq!(
        unsearched,
        [tinker_pdf::Unsearched {
            attachment_path: path(&["locked.pdf"]),
            reason: UnsearchedReason::PasswordRequired,
        }]
    );
    let (hits, _) = searched(&doc, "open secret", &default);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].attachment_path, path(&["open.pdf"]));

    // A file specification with no embedded file stream.
    let broken = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 5 0 R >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>\nendobj\n\
4 0 obj\n<< /Type /Filespec /F (broken.pdf) >>\nendobj\n\
5 0 obj\n<< /Names [(broken) 4 0 R] >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n";
    let doc = Document::open(broken.to_vec()).expect("it opens");
    assert_eq!(doc.attachments().len(), 1);
    let (hits, unsearched) = searched(&doc, "anything", &default);
    assert!(hits.is_empty());
    assert_eq!(
        unsearched,
        [tinker_pdf::Unsearched {
            attachment_path: path(&["broken"]),
            reason: UnsearchedReason::Unreadable,
        }]
    );
}
