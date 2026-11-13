//! The facade's edit, save and reopen (`tinker_pdf::write::append`,
//! `docs/design/performance.md`, P-17): an editor's edits written as the
//! update section an incremental save appends, and the document reopened over
//! it, read on the public surface (ruling 11) — what the reopen parsed,
//! counted on `Stats`, and every page of the reopened document held to a fresh
//! open of the joined bytes, pixels, warnings and text.

mod written_support;

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tinker_pdf::write::{append, AppendError, AppendOptions, Appended};
use tinker_pdf::{
    ByteSource, CacheLimits, Document, DocumentBuilder, DocumentEditor, FontProvider, FontRequest,
    FullOpen, LadderLevel, OpenError, OpenOptions, Parallelism, RenderOptions, SliceSource,
    SourceMiss, Stats, Warning, WriteMode, WriteOptions, CHUNK_SIZE,
};
use tinker_pdf_cos::limits::MAX_REOPEN_DEPTH;
use tinker_pdf_cos::{annot, Dict, Name, ObjRef, Object, Rect, StreamData};
use written_support::{authored, rewritten, synthesised, Key, Locked};

/// The password of the committed AES-256 fixture.
const PASSWORD: &str = "open-sesame";

/// The options an incremental save takes.
fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

/// [`AppendOptions`] asking for the reopen's counts.
fn counted() -> AppendOptions {
    AppendOptions {
        stats: true,
        ..AppendOptions::default()
    }
}

/// The page of `pages` the annotation is on: the middle one, page 501 of a
/// thousand.
fn middle(pages: usize) -> u32 {
    u32::try_from(pages / 2).expect("a small count")
}

/// `pages` letter pages, each drawing its number in a face the document does
/// not embed, with a 100 by 100 pt square annotation on the middle page, its
/// appearance synthesized: a file this engine wrote, saved as a rewrite.
fn annotated(pages: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for i in 0..pages {
        builder.add_page(612.0, 792.0, |page| {
            page.text(b"F0", 12.0, 72.0, 720.0, &format!("page {i}"));
        });
    }
    let doc = Document::open(builder.finish()).expect("it opens");
    let mut editor = doc.editor();
    let square = annot::square(
        doc.cos(),
        Rect {
            x0: 100.0,
            y0: 100.0,
            x1: 200.0,
            y1: 200.0,
        },
        annot::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        },
        1.0,
    );
    editor
        .add_annotation(middle(pages), square)
        .expect("the middle page is there");
    let bytes = editor.save(&WriteOptions::default());
    authored(&format!("{pages} pages, annotated"), bytes.clone());
    bytes
}

/// The committed AES-256 fixture.
fn encrypted() -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/encrypted-aes256.pdf");
    std::fs::read(path).expect("the fixture")
}

/// The appearance stream page `index`'s first annotation draws: its `/AP`'s
/// `/N`.
fn appearance(doc: &Document, index: u32) -> ObjRef {
    let cos = doc.cos();
    let page = doc.editor().page_ref(index).expect("the page is there");
    let page = cos.get(page).expect("the page loads");
    let first = page
        .as_dict()
        .and_then(|dict| dict.get_array(cos.intern(b"Annots")))
        .and_then(<[Object]>::first)
        .and_then(Object::as_objref)
        .expect("the page names an annotation");
    let annotation = cos.get(first).expect("the annotation loads");
    let dict = annotation.as_dict().expect("a dictionary");
    cos.resolve_key(dict, cos.intern(b"AP"))
        .as_dict()
        .and_then(|ap| ap.get_ref(Name::N))
        .expect("a normal appearance of its own")
}

/// **One annotation edited**: the middle page's square drawn as a blue fill of
/// its whole box instead, its appearance stream replaced where it is
/// (`put_stream`) — the one object the update writes.
fn recolour(doc: &Document, pages: usize) -> DocumentEditor {
    let target = appearance(doc, middle(pages));
    let mut editor = doc.editor();
    let Some(Object::Stream(stream)) = editor.get(target) else {
        panic!("the appearance is a stream");
    };
    let bbox: Vec<f64> = stream
        .dict
        .get_array(doc.cos().intern(b"BBox"))
        .expect("a form has a /BBox")
        .iter()
        .filter_map(Object::as_number)
        .collect();
    assert_eq!(bbox.len(), 4, "the premise: a rectangle");
    let mut dict = Dict::new();
    for (key, value) in stream.dict.iter() {
        if ![Name::LENGTH, Name::FILTER, Name::DECODE_PARMS].contains(key) {
            dict.insert(*key, value.clone());
        }
    }
    let data = format!(
        "0 0 1 rg {} {} {} {} re f",
        bbox[0],
        bbox[1],
        bbox[2] - bbox[0],
        bbox[3] - bbox[1]
    );
    editor.put_stream(
        target,
        StreamData {
            dict,
            data: data.into_bytes(),
        },
    );
    editor
}

/// **One annotation added**: a green square on page `index`, its appearance
/// synthesized — the page, the annotation and its appearance written.
fn annotate(doc: &Document, index: u32) -> (DocumentEditor, ObjRef) {
    let mut editor = doc.editor();
    let square = annot::square(
        doc.cos(),
        Rect {
            x0: 300.0,
            y0: 300.0,
            x1: 400.0,
            y1: 400.0,
        },
        annot::Color {
            r: 0.0,
            g: 0.5,
            b: 0.0,
        },
        2.0,
    );
    let added = editor
        .add_annotation(index, square)
        .expect("the page is there");
    (editor, added)
}

/// `original` followed by each update.
fn joined(original: &[u8], updates: &[&Appended]) -> Vec<u8> {
    let mut bytes = original.to_vec();
    for appended in updates {
        bytes.extend_from_slice(&appended.update);
    }
    bytes
}

/// What the reopen counted, which every test here asks for.
#[track_caller]
fn stats(appended: &Appended) -> Stats {
    appended.stats.expect("the counts were asked for")
}

/// **The row's bound**: a reopen that read the section alone parsed at most
/// the changed objects and the cross-reference's own — its rows, one for each
/// changed object and one for object zero's free head, and of whole objects
/// only a cross-reference stream's, which the classic section an update is
/// written with has none of.
#[track_caller]
fn within_the_bound(appended: &Appended, what: &str) {
    let stats = stats(appended);
    let changed = appended.changed.len() as u64;
    assert_eq!(appended.full_open, None, "{what}: only the update was read");
    assert_eq!(
        stats.objects_parsed, 0,
        "{what}: a classic section is no object, and nothing else is parsed: {stats:?}"
    );
    assert!(
        stats.xref_entries_read >= changed,
        "{what}: every changed object's row is read: {stats:?}"
    );
    assert!(
        stats.objects_parsed + stats.xref_entries_read <= changed + 1,
        "{what}: {} changed objects and the free head, and {stats:?}",
        appended.changed.len()
    );
}

/// Every page of `got` renders, warnings included, and extracts as the same
/// page of `want` does, at `scale`, with its annotations read alike, and the
/// two documents state the same ladder level and raise the same warnings —
/// as sets, since a reopened document's are its base's as they stand and then
/// its own, so a warning a read raised on the base before the reopen stands
/// earlier in its list than in a fresh open's.
#[track_caller]
fn reads_alike(got: &Document, want: &Document, scale: f64, what: &str) {
    assert_eq!(
        got.page_count(),
        want.page_count(),
        "{what}: the page count"
    );
    let options = RenderOptions {
        scale,
        ..RenderOptions::default()
    };
    for index in 0..want.page_count() {
        let (Some(mine), Some(theirs)) = (got.page(index), want.page(index)) else {
            panic!("{what}: page {index} is missing");
        };
        let (drawn, fresh) = (mine.render(&options), theirs.render(&options));
        assert_eq!(
            (drawn.width, drawn.height, drawn.format),
            (fresh.width, fresh.height, fresh.format),
            "{what}: page {index}'s bitmap"
        );
        assert!(drawn.data == fresh.data, "{what}: page {index}'s pixels");
        assert_eq!(
            drawn.warnings, fresh.warnings,
            "{what}: page {index}'s warnings"
        );
        assert_eq!(
            format!("{:?}", mine.text()),
            format!("{:?}", theirs.text()),
            "{what}: page {index}'s text"
        );
        assert_eq!(
            mine.annotations(),
            theirs.annotations(),
            "{what}: page {index}'s annotations"
        );
    }
    assert_eq!(
        got.ladder_level(),
        want.ladder_level(),
        "{what}: the ladder"
    );
    let raised: HashSet<Warning> = got.warnings().into_iter().collect();
    let fresh: HashSet<Warning> = want.warnings().into_iter().collect();
    assert_eq!(raised, fresh, "{what}: the warnings");
}

/// **An annotation edit saved and reopened parses only what changed.** A
/// generated 1 000-page file whose page 501 carries a 100 by 100 pt square
/// annotation, the annotation's appearance replaced and appended: the reopen
/// reads the section alone, its rows are the changed object's and the free
/// head's, it parses no object, and every count is the one the same edit of a
/// 10-page file gives, where a full open of the same update reads every row
/// the file has. Every page of the reopened document renders and extracts as
/// a fresh open of the joined bytes does, and the edited page shows the edit.
/// An annotation added on page 501 is held to the same bound: the page, the
/// annotation and its appearance.
#[test]
fn an_annotation_edit_saved_and_reopened_parses_only_what_changed() {
    let thousand = annotated(1000);
    let mut counts = Vec::new();
    for (pages, original) in [(1000, thousand.clone()), (10, annotated(10))] {
        let what = format!("{pages} pages");
        let doc = Document::open(original.clone()).expect("it opens");
        let target = appearance(&doc, middle(pages));
        let editor = recolour(&doc, pages);
        let appended = append(&doc, &editor, &counted()).expect("it appends");
        assert_eq!(
            appended.changed,
            vec![target.num],
            "{what}: the appearance alone"
        );
        within_the_bound(&appended, &what);
        counts.push(stats(&appended));

        let fresh = authored(
            &format!("{what}, an annotation edited"),
            joined(&original, &[&appended]),
        );
        reads_alike(&appended.document, &fresh, 0.25, &what);
        let page = middle(pages);
        let options = RenderOptions::default();
        let before = doc.page(page).expect("the page").render(&options);
        let after = appended
            .document
            .page(page)
            .expect("the page")
            .render(&options);
        let full = fresh.page(page).expect("the page").render(&options);
        assert!(
            after.data == full.data && after.warnings == full.warnings,
            "{what}: the edited page at full size"
        );
        assert!(
            before.data != after.data,
            "{what}: the square is drawn blue"
        );

        let (editor, added) = annotate(&doc, page);
        let appended = append(&doc, &editor, &counted()).expect("it appends");
        let page_ref = doc.editor().page_ref(page).expect("the page");
        assert!(
            appended.changed.contains(&page_ref.num) && appended.changed.contains(&added.num),
            "{what}: the page and the annotation: {:?}",
            appended.changed
        );
        within_the_bound(&appended, &format!("{what}, an annotation added"));
        let fresh = authored(
            &format!("{what}, an annotation added"),
            joined(&original, &[&appended]),
        );
        let drawn = appended
            .document
            .page(page)
            .expect("the page")
            .render(&options);
        let want = fresh.page(page).expect("the page").render(&options);
        assert!(
            drawn.data == want.data && drawn.warnings == want.warnings,
            "{what}: the annotated page"
        );
        assert_eq!(
            appended
                .document
                .page(page)
                .expect("the page")
                .annotations(),
            fresh.page(page).expect("the page").annotations(),
            "{what}: two annotations, as a fresh open reads them"
        );
    }

    // Nothing the reopen counts grows with the document.
    let (large, small) = (counts[0], counts[1]);
    assert_eq!(
        (
            large.xref_entries_read,
            large.objects_parsed,
            large.tokens_lexed
        ),
        (
            small.xref_entries_read,
            small.objects_parsed,
            small.tokens_lexed
        ),
        "a thousand pages count {large:?}, ten {small:?}"
    );
    assert_eq!(large.work(), small.work());

    // The same edit over a document read from a source is opened whole, and a
    // full open reads every row the file has.
    let streamed =
        Document::open_streaming(Arc::new(SliceSource::new(thousand))).expect("it opens");
    let editor = recolour(&streamed, 1000);
    let whole = append(&streamed, &editor, &counted()).expect("it appends");
    assert_eq!(whole.full_open, Some(FullOpen::Streamed));
    let all = stats(&whole);
    assert!(
        all.xref_entries_read >= u64::from(streamed.cos().max_object_number()),
        "a full open reads every row: {all:?}"
    );
    assert!(all.xref_entries_read > 100 * large.xref_entries_read);
}

/// **The update is the incremental save's section**, byte for byte: the
/// document's bytes followed by it are what `DocumentEditor::save` writes
/// incrementally, compressed or not and on threads or not — the threads
/// writing the serial bytes — and a compressed update deflates what it
/// carries. An uncounted append hands back no counts and the same bytes as a
/// counted one.
#[test]
fn the_update_is_the_incremental_saves_section() {
    let original = annotated(4);
    let doc = Document::open(original.clone()).expect("it opens");
    let (mut editor, _) = annotate(&doc, 1);
    // A stream deflate makes smaller, so that a compressed update has one.
    let repeated = editor.allocate();
    editor.put_stream(
        repeated,
        StreamData {
            dict: Dict::new(),
            data: "0 0 1 rg 10 10 50 50 re f\n".repeat(64).into_bytes(),
        },
    );
    let mut compressed: Option<Arc<[u8]>> = None;
    for (compress, parallelism) in [
        (false, Parallelism::Serial),
        (true, Parallelism::Serial),
        (true, Parallelism::Threads(3)),
    ] {
        let what = format!("compress {compress}, {parallelism:?}");
        let options = AppendOptions {
            compress,
            parallelism,
            stats: false,
        };
        let appended = append(&doc, &editor, &options).expect("it appends");
        assert!(appended.stats.is_none(), "{what}: nobody asked");
        let save = editor.save(&WriteOptions {
            compress,
            parallelism,
            ..incremental()
        });
        assert!(
            joined(&original, &[&appended]) == save,
            "{what}: the document's bytes and the update are the save"
        );
        assert_eq!(
            appended
                .update
                .windows(b"/FlateDecode".len())
                .any(|window| window == b"/FlateDecode"),
            compress,
            "{what}: deflated exactly when asked"
        );
        authored(&what, save.clone());
        reads_alike(
            &appended.document,
            &Document::open(save).expect("it opens"),
            0.5,
            &what,
        );
        if compress {
            let serial = compressed.get_or_insert_with(|| Arc::clone(&appended.update));
            assert!(
                *serial == appended.update,
                "{what}: the threads write the serial bytes"
            );
        }
    }
    let quiet = append(&doc, &editor, &AppendOptions::default()).expect("it appends");
    let loud = append(&doc, &editor, &counted()).expect("it appends");
    assert!(quiet.stats.is_none() && loud.stats.is_some());
    assert!(
        quiet.update == loud.update,
        "counting writes the same bytes"
    );
    reads_alike(&quiet.document, &loud.document, 0.5, "counted or not");
}

/// **An editor of another document is refused**, and nothing is written: an
/// editor over the same bytes opened again is another document, while a
/// clone of the document, and an editor over the handle the document's editor
/// shares, are the document itself. An editor over a reopened document makes
/// updates to it, never to the document it was reopened from.
#[test]
fn an_editor_of_another_document_is_refused() {
    let original = annotated(2);
    let doc = Document::open(original.clone()).expect("it opens");
    let twin = Document::open(original).expect("it opens");
    let (editor, _) = annotate(&twin, 0);
    let refused = append(&doc, &editor, &AppendOptions::default()).err();
    assert_eq!(refused, Some(AppendError::NotThisDocument));
    assert!(refused.is_some_and(|error| error.to_string().contains("another document")));

    assert!(append(&twin.clone(), &editor, &AppendOptions::default()).is_ok());
    let shared = DocumentEditor::new(editor.shared_document());
    assert!(append(&twin, &shared, &AppendOptions::default()).is_ok());

    let appended = append(&twin, &editor, &AppendOptions::default()).expect("it appends");
    let (next, _) = annotate(&appended.document, 1);
    assert_eq!(
        append(&twin, &next, &AppendOptions::default()).err(),
        Some(AppendError::NotThisDocument),
        "an editor of the reopened document is not one of its base"
    );
    assert!(append(&appended.document, &next, &AppendOptions::default()).is_ok());
}

/// A provider that answers nothing and counts how often it is asked.
#[derive(Default)]
struct Counting(AtomicUsize);

impl Counting {
    fn asked(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}

impl FontProvider for Counting {
    fn substitute(&self, _request: &FontRequest) -> Option<Arc<Vec<u8>>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        None
    }
}

/// **The reopened document carries the document's font provider and archive
/// report.** A page of the reopened document asks the provider the document
/// was opened with for the face it does not embed, and renders as a fresh
/// open handed the same provider does; a document synthesised from Markdown
/// keeps its report, the same one, and its update is held as the conversion's
/// own writing.
#[test]
fn the_reopened_document_carries_the_font_provider_and_the_archive_report() {
    let provider = Arc::new(Counting::default());
    let original = annotated(2);
    let doc = Document::open_with(
        original.clone(),
        &OpenOptions::default().with_fonts(provider.clone()),
    )
    .expect("it opens");
    let (editor, _) = annotate(&doc, 0);
    let appended = append(&doc, &editor, &AppendOptions::default()).expect("it appends");
    let asked = provider.asked();
    let _ = appended
        .document
        .page(0)
        .expect("the page")
        .render(&RenderOptions::default());
    assert!(
        provider.asked() > asked,
        "the reopened document asks the document's provider"
    );
    let fresh = Document::open_with(
        joined(&original, &[&appended]),
        &OpenOptions::default().with_fonts(provider.clone()),
    )
    .expect("it opens");
    reads_alike(&appended.document, &fresh, 1.0, "a provider carried");

    let book = Document::open_markdown(
        b"# A heading\n\nA paragraph of text, set as a book.\n".to_vec(),
        &OpenOptions::default(),
    )
    .expect("it converts");
    let report = book.archive().expect("a conversion reports");
    let mut editor = book.editor();
    assert!(editor.rotate_page(0, 90));
    let appended = append(&book, &editor, &AppendOptions::default()).expect("it appends");
    let carried = appended.document.archive().expect("the report is carried");
    assert!(std::ptr::eq(report, carried), "the same report");
    assert_eq!(appended.document.page(0).expect("the page").rotation(), 90);
    let held = synthesised("a converted book's update", appended.document.clone());
    assert_eq!(held.page(0).expect("the page").rotation(), 90);
}

/// **The reopened document keeps the document's cache budget, in a cache of
/// its own.** A document opened under a budget draws its middle page, so its
/// cache holds what that page's annotation drew, and the annotation's
/// appearance is then replaced and appended: the reopened document states
/// the same budget and draws the replaced appearance — what a fresh open of
/// the joined bytes under the budget draws — and not what the document kept
/// of the one before. A document with no budget reopens with none.
#[test]
fn the_reopened_document_keeps_the_cache_budget_in_a_cache_of_its_own() {
    let pages = 3;
    let original = annotated(pages);
    let limits = CacheLimits::new(64 << 20);
    let budget = OpenOptions::default().with_cache_limits(limits);
    let doc = Document::open_with(original.clone(), &budget).expect("it opens");
    let index = middle(pages);
    let before = doc
        .page(index)
        .expect("the page")
        .render(&RenderOptions::default());
    let appended =
        append(&doc, &recolour(&doc, pages), &AppendOptions::default()).expect("it appends");
    assert_eq!(
        appended.document.cache_limits(),
        Some(limits),
        "the document's budget"
    );
    let after = appended
        .document
        .page(index)
        .expect("the page")
        .render(&RenderOptions::default());
    assert!(
        after.data != before.data,
        "the replaced appearance is what the page draws"
    );
    let fresh = Document::open_with(joined(&original, &[&appended]), &budget).expect("it opens");
    reads_alike(&appended.document, &fresh, 1.0, "a budget carried");

    let plain = Document::open(original.clone()).expect("it opens");
    let appended =
        append(&plain, &recolour(&plain, pages), &AppendOptions::default()).expect("it appends");
    assert_eq!(
        appended.document.cache_limits(),
        None,
        "no budget, no cache"
    );
}

/// **An encrypted document appends under its own key** (7.6.2) and reopens
/// with the security it was authenticated with: the update reads only its
/// section, a note's text is ciphertext in it and plain in the reopened
/// document, and the reopened document reads as a fresh open of the joined
/// bytes given the password.
#[test]
fn an_encrypted_document_appends_under_its_own_key() {
    let fixture = encrypted();
    let doc = Document::open(fixture.clone()).expect("it opens");
    doc.authenticate(PASSWORD).expect("the password opens it");
    let mut editor = doc.editor();
    let note = annot::text_note(
        doc.cos(),
        Rect {
            x0: 20.0,
            y0: 20.0,
            x1: 40.0,
            y1: 40.0,
        },
        "a note nobody may read",
        false,
    );
    editor.add_annotation(0, note).expect("page one is there");
    let appended = append(&doc, &editor, &counted()).expect("it appends");
    within_the_bound(&appended, "an encrypted document");
    assert!(appended.document.is_encrypted());
    assert_eq!(appended.document.auth_level(), doc.auth_level());
    assert!(
        !appended
            .update
            .windows(b"nobody".len())
            .any(|window| window == b"nobody"),
        "the note is sealed"
    );
    let contents: Vec<Option<String>> = appended
        .document
        .page(0)
        .expect("page one")
        .annotations()
        .into_iter()
        .map(|annotation| annotation.contents)
        .collect();
    assert!(
        contents.contains(&Some("a note nobody may read".to_string())),
        "and reads decrypted: {contents:?}"
    );
    let fresh = rewritten(
        "an encrypted document's update",
        Locked {
            bytes: fixture.clone(),
            key: Key::Password(PASSWORD),
        },
        Locked {
            bytes: joined(&fixture, &[&appended]),
            key: Key::Password(PASSWORD),
        },
    );
    reads_alike(&appended.document, &fresh, 0.5, "an encrypted document");
}

/// **An untouched editor appends a section that changes nothing**: no object,
/// one row — the free head's — and a document that reads as its base with a
/// revision more.
#[test]
fn an_untouched_editor_appends_a_section_that_changes_nothing() {
    let original = annotated(3);
    let doc = Document::open(original.clone()).expect("it opens");
    let appended = append(&doc, &doc.editor(), &counted()).expect("it appends");
    assert!(appended.changed.is_empty());
    within_the_bound(&appended, "an untouched editor");
    assert_eq!(appended.document.cos().revisions().len(), 2);
    let fresh = authored(
        "an update that changes nothing",
        joined(&original, &[&appended]),
    );
    reads_alike(&appended.document, &fresh, 0.5, "an untouched editor");
    reads_alike(&appended.document, &doc, 0.5, "the base itself");
}

/// **An update to an update reads only its own section**: an editor over the
/// reopened document appends a second update, whose reopen reads it alone and
/// is held to the same bound, and the document it makes reads as a fresh open
/// of the file and both updates.
#[test]
fn an_update_to_an_update_reads_only_its_own_section() {
    let original = annotated(10);
    let doc = Document::open(original.clone()).expect("it opens");
    let first = append(&doc, &recolour(&doc, 10), &counted()).expect("it appends");
    within_the_bound(&first, "the first update");
    let (editor, _) = annotate(&first.document, 2);
    let second = append(&first.document, &editor, &counted()).expect("it appends");
    within_the_bound(&second, "the second update");
    assert_eq!(second.document.cos().revisions().len(), 3);
    let fresh = authored(
        "two updates, one over the other",
        joined(&original, &[&first, &second]),
    );
    reads_alike(&second.document, &fresh, 0.5, "the second update");
}

/// **A document counting its editor's work counts the reopen into it**: the
/// editor's tally holds the reopen's rows whether or not the append asked for
/// them, and an append that asked hands the whole tally back — the editor's
/// own work and the reopen's.
#[test]
fn a_counting_editor_counts_the_reopen_too() {
    let doc = Document::open(annotated(3)).expect("it opens");
    let (plain, _) = annotate(&doc, 1);
    let alone = stats(&append(&doc, &plain, &counted()).expect("it appends"));

    let mut editor = doc.editor();
    editor.count_work();
    let square = annot::square(
        doc.cos(),
        Rect {
            x0: 300.0,
            y0: 300.0,
            x1: 400.0,
            y1: 400.0,
        },
        annot::Color {
            r: 0.0,
            g: 0.5,
            b: 0.0,
        },
        2.0,
    );
    editor.add_annotation(1, square).expect("the page is there");
    let quiet = append(&doc, &editor, &AppendOptions::default()).expect("it appends");
    assert!(quiet.stats.is_none(), "nobody asked the append");
    let tally = editor.work().expect("the editor counts").stats();
    assert!(
        tally.xref_entries_read >= alone.xref_entries_read,
        "the editor's tally holds the reopen: {tally:?}"
    );
    let loud = stats(&append(&doc, &editor, &counted()).expect("it appends"));
    assert!(
        loud.xref_entries_read >= 2 * alone.xref_entries_read && loud.work() > alone.work(),
        "both reopens and the editor's own work: {loud:?}, against {alone:?}"
    );
}

/// The bytes of `bytes` past the start of its last `%%EOF`, cut off: a base
/// that opens on its table and has no end-of-file marker of its own.
fn without_eof(mut bytes: Vec<u8>) -> Vec<u8> {
    let eof = bytes
        .windows(5)
        .rposition(|window| window == b"%%EOF")
        .expect("an end of file");
    bytes.truncate(eof);
    bytes
}

/// `bytes` with the offset its table states for object one moved a byte on,
/// past the object's header, which an open repairs from a scan (`Patch`).
fn misplaced(mut bytes: Vec<u8>) -> Vec<u8> {
    let table = bytes
        .windows(6)
        .rposition(|window| window == b"\nxref\n")
        .expect("a table")
        + 6;
    // `0 N`, then object zero's twenty-byte row, then object one's.
    let first = table
        + bytes[table..]
            .iter()
            .position(|b| *b == b'\n')
            .expect("a subsection line")
        + 1
        + 20;
    let offset: u64 = std::str::from_utf8(&bytes[first..first + 10])
        .expect("digits")
        .parse()
        .expect("an offset");
    bytes[first..first + 10].copy_from_slice(format!("{:010}", offset + 1).as_bytes());
    bytes
}

/// `bytes` from its first byte to its cross-reference table, the table, the
/// trailer and `startxref` cut off: a file an open rebuilds from a scan
/// (`Rescan`).
fn tableless(mut bytes: Vec<u8>) -> Vec<u8> {
    let table = bytes
        .windows(6)
        .rposition(|window| window == b"\nxref\n")
        .expect("a table");
    bytes.truncate(table + 1);
    bytes
}

/// A full open, held: `appended` says `want`, its counts are a full open's —
/// every row the joined file has — and its document reads as `fresh`.
#[track_caller]
fn read_whole(appended: &Appended, fresh: &Document, want: FullOpen, what: &str) {
    assert_eq!(appended.full_open, Some(want), "{what}");
    let all = stats(appended);
    assert!(
        all.xref_entries_read > appended.changed.len() as u64 + 1
            || fresh.ladder_level() == LadderLevel::Rescan,
        "{what}: a full open reads more than the section: {all:?}"
    );
    reads_alike(&appended.document, fresh, 0.5, what);
}

/// **Every document the reopen cannot keep is read whole, and says why.**
/// Through the facade: a linearized file opened from its head, a file read
/// from a source, a patched and a rescanned file, one whose header is not at
/// byte zero, one with no end-of-file marker of its own, and a chain of
/// appends as deep as a reopen goes, whose full open starts a chain of its
/// own. Each reopened document reads as a fresh open of the joined bytes.
#[test]
fn every_document_the_reopen_cannot_keep_is_read_whole_and_says_why() {
    // Linearized, opened from its head.
    let linear = {
        let doc = Document::open(annotated(3)).expect("it opens");
        doc.editor().save(&WriteOptions {
            linearize: true,
            ..WriteOptions::default()
        })
    };
    authored("a linearized file", linear.clone());
    let doc =
        Document::open_streaming(Arc::new(SliceSource::new(linear.clone()))).expect("it opens");
    assert!(doc.first_page_end().is_some(), "the premise: from its head");
    let (editor, _) = annotate(&doc, 2);
    let appended = append(&doc, &editor, &counted()).expect("it appends");
    // An update de-linearizes the file by nature, so the joined bytes are
    // read rather than held as clean.
    let fresh = Document::open(joined(&linear, &[&appended])).expect("it opens");
    read_whole(&appended, &fresh, FullOpen::Linearized, "a linearized file");

    // Read from a source.
    let original = annotated(3);
    let doc =
        Document::open_streaming(Arc::new(SliceSource::new(original.clone()))).expect("it opens");
    let (editor, _) = annotate(&doc, 2);
    let appended = append(&doc, &editor, &counted()).expect("it appends");
    let fresh = authored("a streamed file's update", joined(&original, &[&appended]));
    read_whole(&appended, &fresh, FullOpen::Streamed, "a streamed file");
    assert!(doc.whole_file_fetched(), "its bytes were fetched whole");

    // Patched and rescanned.
    for (what, damaged, held) in [
        ("a patched file", misplaced(annotated(2)), true),
        ("a rescanned file", tableless(annotated(2)), false),
    ] {
        let doc = Document::open(damaged.clone()).expect("it opens");
        assert_ne!(
            doc.ladder_level(),
            LadderLevel::Trust,
            "{what}: the premise"
        );
        let (editor, _) = annotate(&doc, 0);
        let appended = append(&doc, &editor, &counted()).expect("it appends");
        let bytes = joined(&damaged, &[&appended]);
        let fresh = if held {
            rewritten(what, damaged.clone(), bytes)
        } else {
            Document::open(bytes).expect("it opens")
        };
        read_whole(&appended, &fresh, FullOpen::Repaired, what);
    }

    // A header after other bytes (7.5.2).
    let shifted = [&b"bytes before the header\n"[..], &annotated(2)[..]].concat();
    let doc = Document::open(shifted.clone()).expect("it opens");
    let (editor, _) = annotate(&doc, 0);
    let appended = append(&doc, &editor, &counted()).expect("it appends");
    let fresh = Document::open(joined(&shifted, &[&appended])).expect("it opens");
    read_whole(&appended, &fresh, FullOpen::Shifted, "a shifted file");

    // No end-of-file marker of its own: a full open of the joined bytes
    // stops the base's revision at the update's.
    let unended = without_eof(annotated(2));
    let doc = Document::open(unended.clone()).expect("it opens");
    assert_eq!(doc.ladder_level(), LadderLevel::Trust, "the premise");
    let (editor, _) = annotate(&doc, 1);
    let appended = append(&doc, &editor, &counted()).expect("it appends");
    let fresh = Document::open(joined(&unended, &[&appended])).expect("it opens");
    read_whole(
        &appended,
        &fresh,
        FullOpen::Unchained,
        "no end-of-file marker",
    );

    // As deep as a chain of reopens goes, and one more, and one after that.
    let mut bytes = annotated(2);
    let mut doc = Document::open(bytes.clone()).expect("it opens");
    for depth in 1..=MAX_REOPEN_DEPTH + 2 {
        let mut editor = doc.editor();
        let note = editor.allocate();
        editor.put(note, Object::Int(i64::from(depth)));
        let appended = append(&doc, &editor, &counted()).expect("it appends");
        bytes.extend_from_slice(&appended.update);
        let what = format!("append {depth}");
        if depth == MAX_REOPEN_DEPTH + 1 {
            let fresh = Document::open(bytes.clone()).expect("it opens");
            read_whole(&appended, &fresh, FullOpen::Deep, &what);
        } else {
            within_the_bound(&appended, &what);
        }
        doc = appended.document;
    }
    let fresh = authored("a chain of appends", bytes);
    reads_alike(&doc, &fresh, 0.5, "a chain of appends");
}

/// A source over `bytes` that cannot supply `hole`: every read meeting it
/// misses, naming what it asked for.
struct Holed {
    bytes: Arc<[u8]>,
    hole: Range<u64>,
}

impl ByteSource for Holed {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        let end = range.end.min(self.len());
        if range.start < self.hole.end && self.hole.start < end {
            return Err(SourceMiss::at(range));
        }
        let (Ok(start), Ok(end)) = (usize::try_from(range.start), usize::try_from(end)) else {
            return Ok(Arc::from(&[][..]));
        };
        Ok(Arc::from(self.bytes.get(start..end).unwrap_or_default()))
    }
}

/// **A document read from a source that cannot supply its bytes is refused by
/// name.** A file read through a source with a hole in a stream no page
/// draws: its pages read and its edit is made, and its append, which reads
/// the whole file to open it with the update, misses, naming a range that
/// meets the hole; the document is as it was, and a second try says the same.
#[test]
fn a_source_that_cannot_supply_the_file_is_refused_by_name() {
    let doc = Document::open(annotated(3)).expect("it opens");
    let mut editor = doc.editor();
    let blob = editor.allocate();
    editor.put_stream(
        blob,
        StreamData {
            dict: Dict::new(),
            data: vec![b'x'; 16 * CHUNK_SIZE as usize],
        },
    );
    let bytes = editor.save(&WriteOptions::default());
    authored("a file with a stream no page draws", bytes.clone());
    let run = bytes
        .windows(64)
        .position(|window| window.iter().all(|b| *b == b'x'))
        .expect("the stream's bytes") as u64;
    let hole = run + 8 * CHUNK_SIZE..run + 8 * CHUNK_SIZE + 1;
    let streamed = Document::open_streaming(Arc::new(Holed {
        bytes: bytes.into(),
        hole: hole.clone(),
    }))
    .expect("it opens from its head and its tail");
    let mut editor = streamed.editor();
    assert!(editor.rotate_page(0, 90), "page one is read");
    for attempt in ["the first", "a second"] {
        match append(&streamed, &editor, &AppendOptions::default()) {
            Err(AppendError::Reopen(OpenError::SourceUnavailable(miss))) => assert!(
                miss.needed.start < hole.end && hole.start < miss.needed.end,
                "{attempt} try names {:?}, which misses the hole {hole:?}",
                miss.needed
            ),
            other => panic!(
                "{attempt} try: {:?}",
                other.map(|appended| appended.full_open)
            ),
        }
    }
    let refused = append(&streamed, &editor, &AppendOptions::default())
        .err()
        .expect("refused");
    assert!(refused.to_string().contains("did not reopen"), "{refused}");
    assert_eq!(
        streamed.page(0).map(|page| page.rotation()),
        Some(0),
        "the document is as it was"
    );
}
