use super::*;
use tinker_pdf::{Document as Pdf, LadderLevel, WriteMode, WriteOptions};

fn inputs(count: usize, longest: usize) -> impl Iterator<Item = Vec<u8>> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..count).map(move |n| {
        let len = (n * 113) % longest;
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

/// Every generated document opens on `Trust`, with no warning, and holds
/// its one page.
#[test]
fn every_document_opens_without_repair_with_its_page() {
    for (n, input) in inputs(300, 2000).enumerate() {
        let generated = document(&mut Choices::new(&input));
        let doc = Pdf::open(generated.bytes).unwrap_or_else(|e| panic!("input {n}: {e}"));
        assert_eq!(doc.ladder_level(), LadderLevel::Trust, "input {n}");
        assert!(doc.warnings().is_empty(), "input {n}: {:?}", doc.warnings());
        assert_eq!(doc.page_count(), 1, "input {n}");
    }
}

/// The plainest input is `content_stream`'s plainest page: one section, a
/// table, every token in its plainest spelling.
#[test]
fn the_empty_input_is_the_plainest_page_in_one_table() {
    let generated = document(&mut Choices::new(&[]));
    assert_eq!(generated.sections, 1);
    assert_eq!(generated.streams, 0);
    assert_eq!(generated.bytes, content::page(&mut Choices::new(&[])).bytes);
}

/// `render_rewrite`'s relation, on stable: a document read cleanly draws
/// the same after a rewrite and a reopen, byte for byte and warning for
/// warning.
#[test]
fn a_clean_document_draws_the_same_after_its_rewrite() {
    let mut compared = 0;
    for (n, input) in inputs(120, 2000).enumerate() {
        let generated = document(&mut Choices::new(&input));
        let doc = Pdf::open(generated.bytes).expect("opens");
        let page = doc.page(0).expect("one page");
        let before = page.render(&generated.options);
        if !doc.warnings().is_empty() {
            continue;
        }
        let saved = doc.editor().save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        let again = Pdf::open(saved).unwrap_or_else(|e| panic!("input {n}: {e}"));
        assert_eq!(again.page_count(), 1, "input {n}");
        let after = again.page(0).expect("one page").render(&generated.options);
        assert_eq!(
            (before.width, before.height, before.stride, before.format),
            (after.width, after.height, after.stride, after.format),
            "input {n}"
        );
        assert!(before.data == after.data, "input {n} draws otherwise");
        assert_eq!(before.warnings, after.warnings, "input {n}");
        compared += 1;
    }
    assert!(
        compared > 100,
        "only {compared} documents were read cleanly"
    );
}

/// Every layout the generator counts is written by some input: each number
/// of sections, cross-reference streams, packed objects, updates that write
/// the page's objects again and updates that free.
#[test]
fn every_layout_is_written() {
    let all: Vec<Document> = inputs(600, 2000)
        .map(|input| document(&mut Choices::new(&input)))
        .collect();
    for sections in 1..=cos::MAX_SECTIONS {
        assert!(
            all.iter().any(|d| d.sections == sections),
            "{sections} sections"
        );
    }
    assert!(all.iter().any(|d| d.streams > 0));
    assert!(all.iter().any(|d| d.packed > 0));
    assert!(all.iter().any(|d| d.redefined > 0));
    assert!(all.iter().any(|d| d.freed > 0));
}

fn holds(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

/// The document an input writes.
fn written(data: &[u8]) -> Document {
    document(&mut Choices::new(data))
}

/// The page an input writes, in the plainest spelling: the document's
/// objects are the page's, read from the same choices first, and its own
/// spellings may escape a name a search would look for.
fn plain(data: &[u8]) -> Vec<u8> {
    content::page(&mut Choices::new(data)).bytes
}

type Feature = fn(&[u8]) -> bool;

/// Rewrites `fuzz/corpus/render_rewrite/`: one seed per feature, each the
/// first input of `first_input`'s sequence whose document has it and opens
/// cleanly.
#[test]
#[ignore = "writes fuzz/corpus/render_rewrite"]
fn write_the_fuzz_seeds() {
    let features: &[(&str, Feature)] = &[
        ("four-sections", |data| {
            let d = written(data);
            d.sections == 4 && d.redefined >= 2
        }),
        ("cross-reference-streams", |data| written(data).streams >= 2),
        ("object-streams", |data| written(data).packed >= 4),
        ("a-freed-object", |data| written(data).freed >= 1),
        ("type3-text", |data| {
            let page = plain(data);
            holds(&page, b"/Type3") && holds(&page, b"Tj")
        }),
        ("images-and-inline-images", |data| {
            let page = plain(data);
            holds(&page, b"/Subtype/Image") && holds(&page, b"BI")
        }),
        ("patterns-and-shadings", |data| {
            let page = plain(data);
            holds(&page, b"/PatternType") && holds(&page, b"/ShadingType")
        }),
        ("transparency", |data| {
            let page = plain(data);
            holds(&page, b"/SMask") || holds(&page, b"/Transparency")
        }),
        ("annotations", |data| holds(&plain(data), b"/Annots")),
        ("a-region", |data| {
            let d = written(data);
            d.options.region.is_some() && d.sections >= 2
        }),
    ];
    let clean = |data: &[u8]| {
        Pdf::open(written(data).bytes)
            .map(|doc| doc.ladder_level() == LadderLevel::Trust && doc.warnings().is_empty())
            .unwrap_or(false)
    };
    let read = |data: &[u8]| {
        let mut choices = Choices::new(data);
        let _ = document(&mut choices);
        choices.consumed()
    };
    let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
    for (name, has) in features {
        let taken: Vec<Vec<u8>> = seeds.iter().map(|(_, seed)| seed.clone()).collect();
        let seed = crate::first_input(2000, 200_000, |data| has(data) && clean(data), read, &taken)
            .unwrap_or_else(|| panic!("no input has {name}"));
        assert!(has(&seed), "{name} lost its feature when cut");
        seeds.push((name, seed));
    }
    crate::write_seeds("render_rewrite", &seeds);
}

/// **The documents a corpus generates**, for `wasm_parity.rs`: each entry of
/// `FUZZ_REPLAY_FROM` run through the generator the target
/// `FUZZ_REPLAY_TARGET` names writes with — `content_stream`'s page,
/// `render_rewrite`'s document, or a `render_page` entry as it stands, which
/// is a document already — and written to `FUZZ_REPLAY_TO` as
/// `<target>-<entry>.pdf`, so the three corpora's documents share one
/// directory.
#[test]
#[ignore = "writes the documents a fuzz corpus generates; set FUZZ_REPLAY_TARGET, FUZZ_REPLAY_FROM and FUZZ_REPLAY_TO"]
fn write_the_documents_a_corpus_generates() {
    let (Ok(target), Some(from), Some(to)) = (
        std::env::var("FUZZ_REPLAY_TARGET"),
        std::env::var_os("FUZZ_REPLAY_FROM"),
        std::env::var_os("FUZZ_REPLAY_TO"),
    ) else {
        println!("replay: SKIPPED (set FUZZ_REPLAY_TARGET, FUZZ_REPLAY_FROM and FUZZ_REPLAY_TO)");
        return;
    };
    let write: fn(&[u8]) -> Vec<u8> = match target.as_str() {
        "render_page" => |data| data.to_vec(),
        "content_stream" => |data| content::page(&mut Choices::new(data)).bytes,
        "render_rewrite" => |data| document(&mut Choices::new(data)).bytes,
        other => panic!("{other} is not one of the three targets wasm_parity.rs reads"),
    };
    let to = std::path::PathBuf::from(to);
    std::fs::create_dir_all(&to).unwrap_or_else(|e| panic!("{to:?}: {e}"));
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&from)
        .unwrap_or_else(|e| panic!("{from:?}: {e}"))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "{from:?} holds no entry");
    for entry in &entries {
        let data = std::fs::read(entry).unwrap_or_else(|e| panic!("{entry:?}: {e}"));
        let name = entry
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let out = to.join(format!("{target}-{name}.pdf"));
        std::fs::write(&out, write(&data)).unwrap_or_else(|e| panic!("{out:?}: {e}"));
    }
    println!("replay: RAN over {} entries of {target}", entries.len());
}
