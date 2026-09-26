//! `DocumentEditor::sanitise`: what leaves, what stays, and a report that
//! accounts for every change (tier 5, "Sanitise").
//!
//! # The fixture carries every kind, in every place a viewer looks
//!
//! JavaScript as `/OpenAction` (with a `/Next` chain), in the catalog's `/AA`
//! written inline and by reference, in `/Names /JavaScript`, in a page's
//! `/AA` with its script in a stream, in a text field's four `/AA` triggers,
//! in a file attachment annotation's `/AA`, as a `javascript:` URI, and inside
//! an outline item's `/Next` array; outward actions as a link's `/URI`, a
//! `/Launch` by reference, a button's `/SubmitForm` and an outline item's
//! `/GoToR`; embedded files in the attachment tree and under a file
//! attachment annotation; metadata as `/Info`, the catalog's XMP and an
//! image's own `/Metadata`; and one orphaned script object nothing names.
//! Beside them, what must survive: a `/GoTo` in the same `/AA`, a named
//! destination tree, a border style whose `/S /D` is not an action, viewer
//! preferences, the pages and their content.
//!
//! `script_summary` is the form walkers' own count, and the test of the sweep
//! is that it reads zero afterwards — those walkers miss the page and
//! annotation `/AA`, so the fixture carries both and asserts them by hand.

use std::collections::BTreeMap;
use std::sync::Arc;

use tinker_pdf_cos::{
    attachments, metadata, script_summary, xmp_metadata, CosDocument, DeletedObject, Dict,
    DocumentEditor, EntryHolder, ObjRef, Object, PathStep, Removal, RemovedEntry, Sanitise,
    SanitiseReport, WriteMode, WriteOptions,
};

const FIXTURE: &str = "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /OpenAction 10 0 R
   /AA << /WC << /S /JavaScript /JS (close) >> /DP 11 0 R >>
   /Names << /JavaScript 12 0 R /EmbeddedFiles 13 0 R /Dests 40 0 R >>
   /AcroForm 20 0 R /Metadata 30 0 R /Outlines 50 0 R
   /ViewerPreferences << /DisplayDocTitle true >> >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 5 0 R
   /Resources << /Font << /F1 6 0 R >> /XObject << /Im1 7 0 R >> >>
   /AA << /O << /S /JavaScript /JS 14 0 R >> /C << /S /GoTo /D [4 0 R /Fit] >> >>
   /Annots [21 0 R 22 0 R 23 0 R 24 0 R 25 0 R 32 0 R] >>
endobj
4 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]
   /AA << /O << /S /Rendition /OP 0 /JS (rendered) >> >> >>
endobj
5 0 obj
<< /Length 35 >>
stream
BT /F1 12 Tf 10 10 Td (Hello) Tj ET
endstream
endobj
6 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
7 0 obj
<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray
   /BitsPerComponent 8 /Metadata 31 0 R /Length 1 >>
stream
\u{7f}
endstream
endobj
10 0 obj
<< /S /JavaScript /JS (print) /Next 15 0 R >>
endobj
11 0 obj
<< /Type /Action /S /JavaScript /JS (didprint) >>
endobj
12 0 obj
<< /Names [(init) 16 0 R] >>
endobj
13 0 obj
<< /Names [(data.txt) 17 0 R] >>
endobj
14 0 obj
<< /Length 10 >>
stream
pageopened
endstream
endobj
15 0 obj
<< /S /GoTo /D [4 0 R /Fit] >>
endobj
16 0 obj
<< /S /JavaScript /JS (function helper) >>
endobj
17 0 obj
<< /Type /Filespec /F (data.txt) /UF (data.txt) /EF << /F 18 0 R >> >>
endobj
18 0 obj
<< /Type /EmbeddedFile /Params << /Size 5 >> /Length 5 >>
stream
hello
endstream
endobj
20 0 obj
<< /Fields [21 0 R 32 0 R] /CO [21 0 R] /XFA (xdp) /DA (/Helv 0 Tf 0 g) >>
endobj
21 0 obj
<< /Type /Annot /Subtype /Widget /FT /Tx /T (amount) /Rect [10 10 100 30] /P 3 0 R
   /AA << /K << /S /JavaScript /JS (keystroke) >> /F << /S /JavaScript /JS (format) >>
          /V << /S /JavaScript /JS (validate) >> /C << /S /JavaScript /JS (calculate) >> >> >>
endobj
22 0 obj
<< /Type /Annot /Subtype /Link /Rect [10 40 100 60] /BS << /S /D /W 1 >>
   /A << /S /URI /URI (https://example.org/) >> >>
endobj
23 0 obj
<< /Type /Annot /Subtype /Link /Rect [10 70 100 90] /A << /S /URI /URI (javascript:alert) >> >>
endobj
24 0 obj
<< /Type /Annot /Subtype /Link /Rect [10 100 100 120] /A 26 0 R >>
endobj
25 0 obj
<< /Type /Annot /Subtype /FileAttachment /Rect [10 130 30 150] /FS 27 0 R
   /AA << /E << /S /JavaScript /JS (entered) >> >> >>
endobj
26 0 obj
<< /S /Launch /F (calc.exe) >>
endobj
27 0 obj
<< /Type /Filespec /F (a.bin) /EF << /F 28 0 R >> >>
endobj
28 0 obj
<< /Type /EmbeddedFile /Length 3 >>
stream
abc
endstream
endobj
30 0 obj
<< /Type /Metadata /Subtype /XML /Length 12 >>
stream
<x:xmpmeta/>
endstream
endobj
31 0 obj
<< /Type /Metadata /Subtype /XML /Length 12 >>
stream
<x:xmpmeta/>
endstream
endobj
32 0 obj
<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (send) /Rect [10 160 100 180] /P 3 0 R
   /A << /S /SubmitForm /F << /FS /URL /F (https://example.org/submit) >> >> >>
endobj
40 0 obj
<< /Names [(chapter) [4 0 R /Fit]] >>
endobj
50 0 obj
<< /Type /Outlines /First 51 0 R /Last 52 0 R /Count 2 >>
endobj
51 0 obj
<< /Title (One) /Parent 50 0 R /Next 52 0 R
   /A << /S /GoTo /D [3 0 R /Fit] /Next [<< /S /JavaScript /JS (next) >> << /S /GoTo /D [4 0 R /Fit] >>] >> >>
endobj
52 0 obj
<< /Title (Two) /Parent 50 0 R /Prev 51 0 R /A << /S /GoToR /F (other.pdf) /D [0 /Fit] >> >>
endobj
70 0 obj
<< /Title (Secret) /Author (Somebody) >>
endobj
80 0 obj
<< /S /JavaScript /JS (orphan) >>
endobj
trailer
<< /Root 1 0 R /Info 70 0 R >>
%%EOF
";

/// Hand-written bytes carry no cross-reference table, so they open through
/// the repair scanner; a rewrite gives the same graph, at the same object
/// numbers, inside a well-formed file.
fn fixture() -> Arc<CosDocument> {
    let raw = Arc::new(CosDocument::open(FIXTURE.as_bytes().to_vec()).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

fn r(num: u32) -> ObjRef {
    ObjRef::new(num, 0)
}

fn key(editor: &DocumentEditor, object: &Object, name: &[u8]) -> Option<Object> {
    object.as_dict()?.get(editor.intern(name)).cloned()
}

/// Every object the editor has, as it has it.
fn objects(editor: &DocumentEditor) -> BTreeMap<u32, Object> {
    let mut out = BTreeMap::new();
    for (num, _) in editor.document().xref().iter() {
        if num == 0 {
            continue;
        }
        if let Some(object) = editor.get(r(num)) {
            out.insert(num, object);
        }
    }
    out
}

fn saved(editor: &DocumentEditor, mode: WriteMode) -> (Vec<u8>, CosDocument) {
    let bytes = editor.save(&WriteOptions {
        mode,
        ..WriteOptions::default()
    });
    let reopened = CosDocument::open(bytes.clone()).expect("the sanitised file reopens");
    let defects = tinker_pdf_cos::validate(&reopened);
    assert!(
        defects.is_empty(),
        "{mode:?}: strict validator: {defects:?}"
    );
    (bytes, reopened)
}

fn deleted(report: &SanitiseReport) -> Vec<u32> {
    report.deleted.iter().map(|d| d.object.num).collect()
}

fn removed_at(report: &SanitiseReport, num: u32) -> Vec<(Vec<PathStep>, Removal)> {
    report
        .removed
        .iter()
        .filter(|r| r.holder == EntryHolder::Object(ObjRef::new(num, 0)))
        .map(|r| (r.path.clone(), r.what.clone()))
        .collect()
}

fn k(name: &str) -> PathStep {
    PathStep::Key(name.as_bytes().to_vec())
}

// ---- the exit criterion -----------------------------------------------------

/// Everything at once: no script `script_summary` can see, nor the page and
/// annotation `/AA` it cannot, no outward action, no embedded file, no
/// metadata — and the navigation, the named destinations and the content
/// still there, in a file the strict validator passes, both ways it can be
/// saved.
#[test]
fn sanitising_everything_leaves_no_script_attachment_or_metadata() {
    let doc = fixture();
    let before = script_summary(&doc);
    assert_eq!(before.document_scripts, 1, "{before:?}");
    assert_eq!(before.catalog_actions, 2);
    assert_eq!(before.fields_with_scripts, 1);
    assert_eq!(before.calculation_order, 1);
    assert_eq!(attachments(&doc).len(), 1);
    assert!(xmp_metadata(&doc).is_some());
    assert_eq!(metadata(&doc).title.as_deref(), Some("Secret"));

    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let report = editor.sanitise(&Sanitise::ALL);

    for mode in [WriteMode::Incremental, WriteMode::Rewrite] {
        let (bytes, after) = saved(&editor, mode);
        let summary = script_summary(&after);
        assert!(summary.is_empty(), "{mode:?}: {}", summary.describe());
        assert!(attachments(&after).is_empty(), "{mode:?}");
        assert_eq!(xmp_metadata(&after), None);
        assert_eq!(metadata(&after), tinker_pdf_cos::Metadata::default());

        let view = DocumentEditor::new(Arc::new(after));
        let page = view.get(r(3)).expect("the page stays");
        let aa = key(&view, &page, b"AA").expect("the page keeps its /AA");
        assert!(key(&view, &aa, b"O").is_none(), "the open script is gone");
        assert!(key(&view, &aa, b"C").is_some(), "the /GoTo beside it stays");
        let annots: Vec<u32> = key(&view, &page, b"Annots")
            .and_then(|a| {
                a.as_array().map(|a| {
                    a.iter()
                        .filter_map(|o| o.as_objref())
                        .map(|o| o.num)
                        .collect()
                })
            })
            .expect("the page keeps /Annots");
        assert_eq!(
            annots,
            [21, 25, 32],
            "the three links went with their actions; the field, the attachment and the button stay"
        );
        let attachment = view.get(r(25)).expect("the annotation stays");
        assert!(key(&view, &attachment, b"AA")
            .is_some_and(|aa| aa.as_dict().is_some_and(Dict::is_empty)));
        let spec = view.get(r(27)).expect("its file specification stays");
        assert!(key(&view, &spec, b"EF").is_none(), "without its bytes");
        let item = view.get(r(51)).expect("the outline item stays");
        let action = key(&view, &item, b"A").expect("its /GoTo stays");
        let next = key(&view, &action, b"Next").expect("and its /Next");
        assert_eq!(
            next.as_array().map(<[Object]>::len),
            Some(1),
            "the script left the chain"
        );
        assert!(
            view.get(r(40)).is_some_and(|o| o.as_dict().is_some()),
            "/Dests stays"
        );
        let catalog = view.catalog().expect("a catalog");
        assert!(catalog.get(view.intern(b"ViewerPreferences")).is_some());
        assert!(catalog.get(view.intern(b"OpenAction")).is_none());

        if mode == WriteMode::Rewrite {
            // A rewrite is where the removal is real: nothing of any of it is
            // left in the file's bytes for a scanner to find.
            for needle in [
                &b"/JavaScript"[..],
                b"didprint",
                b"pageopened",
                b"rendered",
                b"function helper",
                b"orphan",
                b"calc.exe",
                b"xmpmeta",
                b"Secret",
                b"hello",
                b"submit",
                b"other.pdf",
                b"/XFA",
            ] {
                assert!(
                    !bytes.windows(needle.len()).any(|w| w == needle),
                    "{} is still in the rewrite",
                    String::from_utf8_lossy(needle)
                );
            }
        }
    }

    // What left, by object: every script and payload object, and the chain
    // behind the opening script.
    let gone = deleted(&report);
    for num in [
        10, 11, 12, 13, 14, 15, 16, 17, 18, 22, 23, 24, 26, 28, 30, 31, 70, 80,
    ] {
        assert!(
            gone.contains(&num),
            "object {num} was not deleted: {gone:?}"
        );
    }
    for num in [2, 3, 4, 5, 6, 7, 20, 21, 25, 27, 32, 40, 50, 51, 52] {
        assert!(
            !gone.contains(&num),
            "object {num} was deleted and is still reached"
        );
    }
}

/// The report is the whole story: every object that differs afterwards is
/// named — as a holder of removed entries or as deleted — every named entry
/// was there before and is not after, and an object named nowhere is exactly
/// as it was.
#[test]
fn every_change_is_in_the_report_and_nothing_else_changed() {
    let doc = fixture();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let before = objects(&editor);
    let report = editor.sanitise(&Sanitise::ALL);
    let after = objects(&editor);

    let holders: Vec<u32> = report
        .removed
        .iter()
        .filter_map(|r| match r.holder {
            EntryHolder::Object(o) => Some(o.num),
            EntryHolder::Trailer => None,
        })
        .collect();
    let gone = deleted(&report);
    for (num, old) in &before {
        let new = after.get(num).expect("every object still resolves");
        if old == new {
            assert!(
                !holders.contains(num) && !gone.contains(num),
                "object {num} is reported and did not change"
            );
        } else if gone.contains(num) {
            assert_eq!(*new, Object::Null, "object {num} is reported deleted");
        } else {
            assert!(
                holders.contains(num),
                "object {num} changed and is not in the report"
            );
        }
    }

    for RemovedEntry { holder, path, what } in &report.removed {
        let EntryHolder::Object(at) = holder else {
            assert_eq!(path, &[k("Info")]);
            assert_eq!(*what, Removal::Info);
            continue;
        };
        let lookup = |object: &Object, steps: &[PathStep]| -> Option<Object> {
            let mut value = object.clone();
            for step in steps {
                value = match step {
                    PathStep::Key(name) => key(&editor, &value, name)?,
                    PathStep::Index(i) => value.as_array()?.get(*i)?.clone(),
                };
            }
            Some(value)
        };
        let old = before.get(&at.num).expect("the holder existed");
        assert!(lookup(old, path).is_some(), "{holder:?} {path:?} was there");
        let new = after.get(&at.num).expect("the holder stays");
        match path.last() {
            Some(PathStep::Key(_)) => {
                assert!(lookup(new, path).is_none(), "{holder:?} {path:?} is gone")
            }
            Some(PathStep::Index(_)) => {
                let parent = &path[..path.len() - 1];
                let len =
                    |o: &Object| lookup(o, parent).and_then(|a| a.as_array().map(<[Object]>::len));
                assert!(len(new) < len(old), "{holder:?} {path:?}: the array shrank");
            }
            None => panic!("an empty path"),
        }
    }

    // The trailer lost `/Info` and nothing else.
    let trailer = tinker_pdf_cos::Resolve::trailer(&editor).into_owned();
    assert!(trailer.get(tinker_pdf_cos::Name::INFO).is_none());
    assert_eq!(trailer.len(), doc.trailer().len() - 1);
}

/// Named, by holder and path, for the places the form walkers never look.
#[test]
fn the_places_the_script_walkers_miss_are_named() {
    let doc = fixture();
    let mut editor = DocumentEditor::new(doc);
    let report = editor.sanitise(&Sanitise {
        javascript: true,
        ..Sanitise::default()
    });
    assert_eq!(
        removed_at(&report, 3),
        [
            (vec![k("AA"), k("O")], Removal::JavaScript),
            // A `javascript:` URI is script, and the link it made has nowhere
            // else to go, so it leaves the page with it.
            (vec![k("Annots"), PathStep::Index(2)], Removal::JavaScript),
        ],
        "a page's /AA, and a link whose only action was script"
    );
    assert_eq!(
        removed_at(&report, 25),
        [(vec![k("AA"), k("E")], Removal::JavaScript)],
        "an annotation's /AA"
    );
    assert_eq!(
        removed_at(&report, 4),
        [(vec![k("AA"), k("O")], Removal::JavaScript)],
        "a rendition carrying /JS (12.6.4.14) is script"
    );
    assert!(report.deleted.contains(&DeletedObject {
        object: r(23),
        what: Removal::JavaScript
    }));
    assert_eq!(
        removed_at(&report, 51),
        [(
            vec![k("A"), k("Next"), PathStep::Index(0)],
            Removal::JavaScript
        )],
        "an element of an action's /Next"
    );
    let catalog = removed_at(&report, 1);
    for expected in [
        (vec![k("OpenAction")], Removal::JavaScript),
        (vec![k("AA"), k("WC")], Removal::JavaScript),
        (vec![k("AA"), k("DP")], Removal::JavaScript),
        (
            vec![k("Names"), k("JavaScript")],
            Removal::DocumentJavaScript,
        ),
    ] {
        assert!(catalog.contains(&expected), "{expected:?} in {catalog:?}");
    }
    assert_eq!(
        removed_at(&report, 20),
        [
            (vec![k("CO")], Removal::CalculationOrder),
            (vec![k("XFA")], Removal::XfaForm)
        ]
    );
    assert!(report.deleted.contains(&DeletedObject {
        object: r(80),
        what: Removal::JavaScript
    }));
}

// ---- each switch alone -------------------------------------------------------

#[test]
fn javascript_alone_leaves_the_outward_actions_the_files_and_the_metadata() {
    let doc = fixture();
    let mut editor = DocumentEditor::new(doc);
    editor.sanitise(&Sanitise {
        javascript: true,
        ..Sanitise::default()
    });
    let (_, after) = saved(&editor, WriteMode::Rewrite);
    assert!(script_summary(&after).is_empty());
    assert_eq!(attachments(&after).len(), 1);
    assert!(xmp_metadata(&after).is_some());
    assert_eq!(metadata(&after).title.as_deref(), Some("Secret"));
    let view = DocumentEditor::new(Arc::new(after));
    for (num, name) in [(22, "A"), (24, "A"), (32, "A"), (52, "A")] {
        let object = view.get(r(num)).expect("stays");
        assert!(
            key(&view, &object, name.as_bytes()).is_some(),
            "object {num} kept /{name}"
        );
    }
    let link = view.get(r(22)).expect("the web link stays");
    assert!(
        key(&view, &link, b"BS").is_some(),
        "a border style's /S /D is not an action"
    );
}

#[test]
fn actions_alone_leave_the_scripts() {
    let doc = fixture();
    let mut editor = DocumentEditor::new(doc);
    let report = editor.sanitise(&Sanitise {
        actions: true,
        ..Sanitise::default()
    });
    // The three links leave the page with their actions — a javascript: URI
    // is still a URI to this switch — and the button, a field, stays without
    // its /SubmitForm.
    let uri = Removal::Action(b"URI".to_vec());
    assert_eq!(
        removed_at(&report, 3),
        [
            (vec![k("Annots"), PathStep::Index(1)], uri.clone()),
            (vec![k("Annots"), PathStep::Index(2)], uri),
            (
                vec![k("Annots"), PathStep::Index(3)],
                Removal::Action(b"Launch".to_vec())
            ),
        ]
    );
    assert_eq!(
        removed_at(&report, 32),
        [(vec![k("A")], Removal::Action(b"SubmitForm".to_vec()))]
    );
    assert_eq!(
        removed_at(&report, 52),
        [(vec![k("A")], Removal::Action(b"GoToR".to_vec()))]
    );
    assert_eq!(
        removed_at(&report, 4),
        [(
            vec![k("AA"), k("O")],
            Removal::Action(b"Rendition".to_vec())
        )],
        "media is outward whether or not it carries script"
    );
    assert_eq!(
        deleted(&report),
        [22, 23, 24, 26],
        "the links and the /Launch object, and only those"
    );
    let (_, after) = saved(&editor, WriteMode::Rewrite);
    assert!(
        !script_summary(&after).is_empty(),
        "the scripts were not asked for"
    );
}

#[test]
fn embedded_files_alone_take_the_bytes_and_keep_the_annotation() {
    let doc = fixture();
    let mut editor = DocumentEditor::new(doc);
    let report = editor.sanitise(&Sanitise {
        embedded_files: true,
        ..Sanitise::default()
    });
    assert_eq!(
        removed_at(&report, 1),
        [(
            vec![k("Names"), k("EmbeddedFiles")],
            Removal::EmbeddedFileTree
        )]
    );
    assert_eq!(
        removed_at(&report, 27),
        [(vec![k("EF")], Removal::EmbeddedFile)]
    );
    assert_eq!(deleted(&report), [13, 17, 18, 28]);
    let (_, after) = saved(&editor, WriteMode::Rewrite);
    assert!(attachments(&after).is_empty());
    assert!(!script_summary(&after).is_empty());
    assert!(xmp_metadata(&after).is_some());
}

#[test]
fn metadata_alone_takes_info_and_every_packet() {
    let doc = fixture();
    let mut editor = DocumentEditor::new(doc);
    let report = editor.sanitise(&Sanitise {
        metadata: true,
        ..Sanitise::default()
    });
    assert!(report.removed.contains(&RemovedEntry {
        holder: EntryHolder::Trailer,
        path: vec![k("Info")],
        what: Removal::Info,
    }));
    assert_eq!(
        removed_at(&report, 7),
        [(vec![k("Metadata")], Removal::Metadata)],
        "an image's own packet, on a stream"
    );
    assert_eq!(deleted(&report), [30, 31, 70]);
    for mode in [WriteMode::Incremental, WriteMode::Rewrite] {
        let (_, after) = saved(&editor, mode);
        assert_eq!(
            metadata(&after),
            tinker_pdf_cos::Metadata::default(),
            "{mode:?}"
        );
        assert_eq!(xmp_metadata(&after), None);
        assert_eq!(attachments(&after).len(), 1);
        // The image kept its samples when its dictionary lost a key.
        assert_eq!(after.stream_decoded(r(7)).expect("decodes"), [0x7f]);
    }
}

/// Asking for nothing changes nothing, and a second pass over a sanitised
/// document finds nothing more.
#[test]
fn nothing_asked_for_is_nothing_done_and_a_second_pass_finds_nothing() {
    let doc = fixture();
    let mut editor = DocumentEditor::new(doc);
    assert!(editor.sanitise(&Sanitise::default()).is_empty());
    assert!(!editor.is_dirty());
    assert!(!editor.sanitise(&Sanitise::ALL).is_empty());
    assert!(editor.sanitise(&Sanitise::ALL).is_empty(), "idempotent");
}

// ---- actions dressed as something else ---------------------------------------

/// Every action here is one a viewer runs — a viewer looks at where a
/// dictionary sits and at its `/S`, resolving a reference, and at nothing
/// else — and each is dressed so that a test asking more of an action than
/// that lets it through: an extra `/K` or `/P` (the keys a structure element
/// has), a `/Type` other than `/Action`, an `/S` written as a reference, an
/// indirect action with an odd `/Type` whose `/Next` is an indirect array,
/// and a navigation action carrying `/P` whose `/Next` is script. Beside
/// them, a structure element role-mapped to `URI` under the structure tree —
/// in no place a viewer runs an action — stays.
const DRESSED: &str = "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /OpenAction 10 0 R /StructTreeRoot 30 0 R
   /AA << /WC << /S /JavaScript /JS (close) /K 0 >>
          /DP << /Type /Whatever /S /JavaScript /JS (didprint) >>
          /WS 12 0 R >> >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [20 0 R]
   /AA << /O << /S /GoTo /D [3 0 R /Fit] /P 3 0 R
                /Next [<< /S /JavaScript /JS (chained) /K 0 >>] >> >> >>
endobj
9 0 obj
/JavaScript
endobj
10 0 obj
<< /S 9 0 R /JS (opened) >>
endobj
11 0 obj
[<< /S /JavaScript /JS (listed) /P 3 0 R >>]
endobj
12 0 obj
<< /Type /Whatever /S /GoTo /D [3 0 R /Fit] /Next 11 0 R >>
endobj
20 0 obj
<< /Type /Annot /Subtype /Link /Rect [10 10 100 30]
   /A << /S /Launch /F (calc.exe) /P 3 0 R >> >>
endobj
30 0 obj
<< /Type /StructTreeRoot /K 31 0 R /RoleMap << /URI /Span >> >>
endobj
31 0 obj
<< /Type /StructElem /S /URI /P 30 0 R /Pg 3 0 R >>
endobj
trailer
<< /Root 1 0 R >>
%%EOF
";

fn dressed() -> Arc<CosDocument> {
    let raw = Arc::new(CosDocument::open(DRESSED.as_bytes().to_vec()).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// An action is what sits where a viewer runs one: `Sanitise::ALL` takes
/// every dressed action out, `script_summary` reads nothing afterwards, and
/// none of their scripts is left in the rewrite's bytes.
#[test]
fn an_action_dressed_as_something_else_is_still_taken_out() {
    let doc = dressed();
    assert_eq!(
        script_summary(&doc).catalog_actions,
        2,
        "the premise: the walkers see the /K and the /Type /Whatever scripts"
    );
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    let report = editor.sanitise(&Sanitise::ALL);

    let catalog = removed_at(&report, 1);
    for expected in [
        (vec![k("OpenAction")], Removal::JavaScript),
        (vec![k("AA"), k("WC")], Removal::JavaScript),
        (vec![k("AA"), k("DP")], Removal::JavaScript),
    ] {
        assert!(catalog.contains(&expected), "{expected:?} in {catalog:?}");
    }
    assert!(
        !catalog.iter().any(|(path, _)| path == &[k("AA"), k("WS")]),
        "the navigation action stays: {catalog:?}"
    );
    assert_eq!(
        removed_at(&report, 3),
        [
            (
                vec![k("Annots"), PathStep::Index(0)],
                Removal::Action(b"Launch".to_vec())
            ),
            (
                vec![k("AA"), k("O"), k("Next"), PathStep::Index(0)],
                Removal::JavaScript
            ),
        ]
    );
    assert_eq!(
        removed_at(&report, 11),
        [(vec![PathStep::Index(0)], Removal::JavaScript)],
        "an indirect /Next array of an indirect action with an odd /Type"
    );
    let gone = deleted(&report);
    for num in [9, 10, 20] {
        assert!(gone.contains(&num), "object {num} is deleted: {gone:?}");
    }
    for num in [11, 12, 30, 31] {
        assert!(
            !gone.contains(&num),
            "object {num} is still reached: {gone:?}"
        );
    }

    for mode in [WriteMode::Incremental, WriteMode::Rewrite] {
        let (bytes, after) = saved(&editor, mode);
        let summary = script_summary(&after);
        assert!(summary.is_empty(), "{mode:?}: {}", summary.describe());
        let view = DocumentEditor::new(Arc::new(after));
        let catalog = view.catalog().expect("a catalog");
        assert!(catalog.get(view.intern(b"OpenAction")).is_none());
        let element = view.get(r(31)).expect("the structure element stays");
        assert_eq!(
            key(&view, &element, b"S").and_then(|s| s.as_name()),
            Some(view.intern(b"URI")),
            "a structure element is not an action"
        );
        if mode == WriteMode::Rewrite {
            for needle in [
                &b"close"[..],
                b"didprint",
                b"opened",
                b"chained",
                b"listed",
                b"calc.exe",
            ] {
                assert!(
                    !bytes.windows(needle.len()).any(|w| w == needle),
                    "{} is still in the rewrite",
                    String::from_utf8_lossy(needle)
                );
            }
        }
    }
}

// ---- pages a save has not yet put in the tree --------------------------------

/// A page with text in a font, and a link whose `/P` names the page — which
/// `import_page` follows, so the copied link's `/P` names a copy of the
/// source page that shares the imported page's content and font.
const LINKED: &str = "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 5 0 R
   /Resources << /Font << /F1 6 0 R >> >> /Annots [7 0 R] >>
endobj
5 0 obj
<< /Length 35 >>
stream
BT /F1 12 Tf 10 10 Td (Hello) Tj ET
endstream
endobj
6 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
7 0 obj
<< /Type /Annot /Subtype /Link /Rect [10 40 100 60] /P 3 0 R
   /A << /S /URI /URI (https://example.org/) >> >>
endobj
trailer
<< /Root 1 0 R >>
%%EOF
";

/// A page `import_page` put in the order is in the document, though only a
/// save writes it into `/Kids`: sanitising takes its link and keeps its
/// content and font, and both saves draw what the source drew.
#[test]
fn an_imported_page_keeps_its_content_when_its_link_goes() {
    let source = CosDocument::open(LINKED.as_bytes().to_vec()).expect("the source opens");
    let source_page = &tinker_pdf_cos::pages::collect(&source)[0];
    let drawn = tinker_pdf_cos::pages::content_bytes(&source, source_page);

    let mut builder = tinker_pdf_cos::DocumentBuilder::new();
    builder.add_page(200.0, 200.0, |_| {});
    let target = Arc::new(CosDocument::open(builder.finish()).expect("the target opens"));
    let mut editor = DocumentEditor::new(target);
    let imported = editor.import_page(&source, 0, 1).expect("the page imports");
    let page = editor.get(imported).expect("the imported page");
    let contents = key(&editor, &page, b"Contents")
        .and_then(|c| c.as_objref())
        .expect("an indirect /Contents");

    let report = editor.sanitise(&Sanitise {
        actions: true,
        ..Sanitise::default()
    });
    assert_eq!(
        removed_at(&report, imported.num),
        [(
            vec![k("Annots"), PathStep::Index(0)],
            Removal::Action(b"URI".to_vec())
        )]
    );
    let gone = deleted(&report);
    assert!(
        !gone.contains(&contents.num) && !gone.contains(&imported.num),
        "the imported page and its content are still in the document: {gone:?}"
    );

    for mode in [WriteMode::Incremental, WriteMode::Rewrite] {
        let (_, after) = saved(&editor, mode);
        let pages = tinker_pdf_cos::pages::collect(&after);
        assert_eq!(pages.len(), 2, "{mode:?}");
        assert_eq!(
            tinker_pdf_cos::pages::content_bytes(&after, &pages[1]),
            drawn,
            "{mode:?}: the imported page draws what it drew"
        );
    }
}

// ---- a trailer entry taken out stays out -------------------------------------

/// An information dictionary something else still names, so taking `/Info`
/// out of the trailer leaves the object in the file.
const NAMED_INFO: &str = "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]
   /PieceInfo << /Example << /LastModified (D:20260926) /Private 70 0 R >> >> >>
endobj
70 0 obj
<< /Title (Secret) >>
endobj
trailer
<< /Root 1 0 R /Info 70 0 R >>
%%EOF
";

/// A reader that merges an update's trailer with the ones before it — this
/// crate's does, newest first — would find `/Info` in the earlier trailer
/// if the update's trailer merely left the key out. So an incremental
/// update writes `/Info null` (7.3.9: the entry absent), and a rewrite,
/// which has no earlier trailer, leaves it out.
#[test]
fn info_taken_out_does_not_come_back_from_an_earlier_trailer() {
    let raw = Arc::new(CosDocument::open(NAMED_INFO.as_bytes().to_vec()).expect("it opens"));
    let doc = Arc::new(
        CosDocument::open(DocumentEditor::new(raw).save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        }))
        .expect("the rewrite reopens"),
    );
    assert_eq!(metadata(&doc).title.as_deref(), Some("Secret"), "premise");
    let mut editor = DocumentEditor::new(doc);
    let report = editor.sanitise(&Sanitise {
        metadata: true,
        ..Sanitise::default()
    });
    assert!(
        report.deleted.is_empty(),
        "the page still names it: {report:?}"
    );
    for mode in [WriteMode::Incremental, WriteMode::Rewrite] {
        let (_, after) = saved(&editor, mode);
        assert_eq!(
            metadata(&after),
            tinker_pdf_cos::Metadata::default(),
            "{mode:?}"
        );
        let info = after.trailer().get(tinker_pdf_cos::Name::INFO);
        match mode {
            WriteMode::Incremental => assert_eq!(info, Some(&Object::Null)),
            _ => assert_eq!(info, None, "{mode:?}"),
        }
    }
}
