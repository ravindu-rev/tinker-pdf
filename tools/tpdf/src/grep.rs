//! `tpdf grep`: a needle searched across files (`docs/design/document-search.md`,
//! "`tpdf grep`"), a wrapper over `Document::search` (ruling 11).
//!
//! The first argument is the needle and every other a file. The page search's
//! options and the keep- options are flags, the sources one flag, and each
//! file is opened as the reading verbs open one: with `--password` where it is
//! encrypted, the empty password otherwise. Each hit prints one line,
//!
//! ```text
//! <file>:<place>: <before>[<text>]<after>
//! ```
//!
//! and a file that does not open, or that its password does not open, is
//! named on standard error, as is each attachment the search reached and did
//! not read. The status tells the three outcomes apart, as `grep` does: 0 when
//! a hit was found and everything was read, 1 when none was and everything was
//! read, and 2 when anything could not be read, whatever was found elsewhere.

use std::io::Write;

use tinker_pdf::{
    AnnotationText, AttachmentText, DocumentSearchOptions, HitPlace, SearchHit, SearchOptions,
    SearchSources, Unsearched, UnsearchedReason, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_DEPTH,
    MAX_SEARCHED_ATTACHMENTS,
};

use crate::{open, Options, Reads};

/// The status a run that found a hit and read everything exits with.
pub(crate) const FOUND: u8 = 0;
/// The status a run that found no hit and read everything exits with.
pub(crate) const NOT_FOUND: u8 = 1;
/// The status a run that could not read a file or an attachment exits with,
/// and an argument the verb cannot use.
pub(crate) const UNREAD: u8 = 2;

/// `--sources LIST`'s names, each one source, beside `all`.
pub(crate) const SOURCES: [&str; 6] = [
    "pages",
    "annotations",
    "fields",
    "outline",
    "attachments",
    "attached",
];

/// `--sources LIST` read: a comma-separated list of [`SOURCES`] names, or
/// `all`; a name it does not know is refused, since a typo that fell back to
/// every source would search what the caller asked to be spared.
pub(crate) fn sources(raw: &str) -> Result<SearchSources, String> {
    let mut sources = SearchSources {
        pages: false,
        annotations: false,
        form_fields: false,
        outline: false,
        attachments: false,
        attached_documents: false,
    };
    for name in raw.split(',').map(str::trim) {
        match name {
            "all" => sources = SearchSources::ALL,
            "pages" => sources.pages = true,
            "annotations" => sources.annotations = true,
            "fields" => sources.form_fields = true,
            "outline" => sources.outline = true,
            "attachments" => sources.attachments = true,
            "attached" => sources.attached_documents = true,
            _ => {
                return Err(format!(
                    "`--sources {raw}`: `{name}` is no source; the sources are `{}` and `all`",
                    SOURCES.join("`, `")
                ))
            }
        }
    }
    Ok(sources)
}

/// The document search the flags ask for.
fn search_options(options: &Options) -> DocumentSearchOptions {
    DocumentSearchOptions {
        search: SearchOptions {
            case_sensitive: options.case_sensitive,
            whole_word: options.whole_word,
            diacritic_insensitive: options.ignore_diacritics,
        },
        keep_lines: options.keep_lines,
        keep_paragraphs: options.keep_paragraphs,
        keep_hyphens: options.keep_hyphens,
        sources: options.sources.unwrap_or(SearchSources::ALL),
    }
}

/// The verb: every hit of the needle in every file written to `out`, what
/// could not be read to `err`; the exit status.
pub(crate) fn run(options: &Options, out: &mut impl Write, err: &mut impl Write) -> u8 {
    let Some((needle, files)) = options.files.split_first() else {
        let _ = writeln!(err, "tpdf: grep needs a needle and at least one file");
        return UNREAD;
    };
    if files.is_empty() {
        let _ = writeln!(err, "tpdf: grep needs a needle and at least one file");
        return UNREAD;
    }
    let search = search_options(options);
    let mut found = false;
    let mut unread = false;
    for file in files {
        let doc = match open(file, Reads::Whole, options.password.as_deref(), None) {
            Ok((doc, _)) => doc,
            Err(message) => {
                let _ = writeln!(err, "tpdf grep: {message}");
                unread = true;
                continue;
            }
        };
        let mut cursor = doc.search(needle, &search);
        for hit in cursor.by_ref() {
            found = true;
            let _ = writeln!(out, "{}", line(file, &hit));
        }
        for entry in cursor.unsearched() {
            unread = true;
            let _ = writeln!(err, "tpdf grep: {file}: {}", unsearched(entry));
        }
    }
    if unread {
        UNREAD
    } else if found {
        FOUND
    } else {
        NOT_FOUND
    }
}

/// The line a hit prints: its file, its place and the text around it, the hit
/// itself in brackets.
pub(crate) fn line(file: &str, hit: &SearchHit) -> String {
    format!(
        "{file}:{}: {}[{}]{}",
        place(hit),
        hit.before,
        hit.text,
        hit.after
    )
}

/// Where a hit is, as a line names it: its attachments, then its source.
fn place(hit: &SearchHit) -> String {
    let mut out = String::new();
    for name in &hit.attachment_path {
        out.push_str(&format!("attached {name} > "));
    }
    out.push_str(&match &hit.place {
        HitPlace::Pages(pages) => match (pages.first(), pages.last()) {
            (Some(first), Some(last)) if first.page != last.page => {
                format!("pages {}-{}", first.page + 1, last.page + 1)
            }
            (Some(first), _) => format!("page {}", first.page + 1),
            _ => "page".to_string(),
        },
        HitPlace::Annotation {
            page, index, text, ..
        } => format!(
            "annotation {} on page {} ({})",
            index + 1,
            page + 1,
            match text {
                AnnotationText::Contents => "contents",
                AnnotationText::RichText => "rich text",
                AnnotationText::Subject => "subject",
                AnnotationText::Author => "author",
            }
        ),
        HitPlace::FormField { name, .. } => format!("field {name}"),
        HitPlace::Outline { titles, .. } => format!("outline {}", titles.join(" > ")),
        HitPlace::Attachment { name, text, .. } => format!(
            "attachment {name} ({})",
            match text {
                AttachmentText::Name => "name",
                AttachmentText::FileName => "file name",
                AttachmentText::Description => "description",
            }
        ),
        other => format!("{other:?}"),
    });
    out
}

/// What a line on standard error says of something the search did not read.
fn unsearched(entry: &Unsearched) -> String {
    let why = match entry.reason {
        UnsearchedReason::PasswordRequired => "a password is required".to_string(),
        UnsearchedReason::UnsupportedEncryption => {
            "its encryption is one this build does not implement".to_string()
        }
        UnsearchedReason::TooDeep => {
            format!("it lies past {MAX_ATTACHMENT_DEPTH} levels of attached document")
        }
        UnsearchedReason::TooLarge => {
            format!("it decodes to more than {} MiB", MAX_ATTACHMENT_BYTES >> 20)
        }
        UnsearchedReason::TooMany => {
            format!("it lies past {MAX_SEARCHED_ATTACHMENTS} attachments read in one search")
        }
        UnsearchedReason::Unreadable => "it cannot be read".to_string(),
        other => format!("{other:?}"),
    };
    if entry.attachment_path.is_empty() {
        format!("not searched: {why}")
    } else {
        let path: Vec<String> = entry
            .attachment_path
            .iter()
            .map(|name| format!("attached {name}"))
            .collect();
        format!("{} not searched: {why}", path.join(" > "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed fixtures, by path from this crate.
    const FIXTURES: [&str; 5] = [
        "../../testdata/simple-text.pdf",
        "../../testdata/outline-3level.pdf",
        "../../testdata/form-fields.pdf",
        "../../testdata/encrypted-aes256.pdf",
        "../../testdata/permissions-noprint.pdf",
    ];

    /// A run of the verb over `args`, its standard output and standard error
    /// as lines, and its status.
    fn grep(args: &[&str]) -> (Vec<String>, Vec<String>, u8) {
        let all: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        let options = Options::parse(&all).expect("the arguments parse");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(&options, &mut out, &mut err);
        let lines = |bytes: Vec<u8>| -> Vec<String> {
            String::from_utf8(bytes)
                .expect("text")
                .lines()
                .map(str::to_string)
                .collect()
        };
        (lines(out), lines(err), status)
    }

    /// The fixtures' paths from this crate's directory.
    fn fixtures() -> Vec<String> {
        FIXTURES
            .iter()
            .map(|name| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(name)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    /// **The verb finds the known hits across the `testdata/` fixtures**
    /// (FT-21.5): `page 3 of 3` once, on the third page of `simple-text.pdf`,
    /// and `Chapter 1` once, in `outline-3level.pdf`'s outline and on none of
    /// its pages, with the two encrypted fixtures named when no `--password`
    /// is given and the status the one that says something was not read.
    #[test]
    fn grep_finds_the_known_hits_across_the_testdata_fixtures() {
        let files = fixtures();
        let mut args = vec!["page 3 of 3"];
        args.extend(files.iter().map(String::as_str));
        let (out, err, status) = grep(&args);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(
            out[0].ends_with("simple-text.pdf:page 3: Tinker fixture, [page 3 of 3]"),
            "{out:?}"
        );
        for locked in ["encrypted-aes256.pdf", "permissions-noprint.pdf"] {
            assert!(
                err.iter().any(|line| line.contains(locked)),
                "{locked} is named: {err:?}"
            );
        }
        assert_eq!(err.len(), 2, "{err:?}");
        assert_eq!(status, UNREAD);

        let mut args = vec!["Chapter 1"];
        args.extend(files.iter().map(String::as_str));
        let (out, err, status) = grep(&args);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(
            out[0].ends_with("outline-3level.pdf:outline Part One > Chapter 1: [Chapter 1]"),
            "{out:?}"
        );
        assert_eq!(err.len(), 2, "{err:?}");
        assert_eq!(status, UNREAD);

        // The pages alone: `Chapter 1` is on none of them.
        let (out, _, _) = grep(&["Chapter 1", "--sources", "pages", &files[1]]);
        assert!(out.is_empty(), "{out:?}");
    }

    /// **The status tells no hit apart from an unread file**: 0 for a hit
    /// with everything read, 1 for none, 2 where a file did not open; and
    /// with its password an encrypted fixture is read like any other.
    #[test]
    fn the_status_tells_a_hit_none_and_an_unread_file_apart() {
        let files = fixtures();
        let (out, err, status) = grep(&["page 3 of 3", &files[0]]);
        assert_eq!((out.len(), err.len(), status), (1, 0, FOUND));
        let (out, err, status) = grep(&["absent words", &files[0], &files[1]]);
        assert_eq!((out.len(), err.len(), status), (0, 0, NOT_FOUND));
        let (out, err, status) = grep(&["page 3 of 3", &files[0], &files[3]]);
        assert_eq!((out.len(), err.len(), status), (1, 1, UNREAD));
        let missing = std::env::temp_dir().join("tpdf-grep-no-such-file.pdf");
        let (_, err, status) = grep(&["x", &missing.to_string_lossy()]);
        assert_eq!((err.len(), status), (1, UNREAD));

        let (out, err, status) = grep(&[
            "encrypted with AES-256",
            "--password",
            "open-sesame",
            &files[3],
        ]);
        assert_eq!(err, Vec::<String>::new());
        assert!(!out.is_empty(), "{out:?}");
        assert!(out.iter().all(|line| line.contains(":page ")), "{out:?}");
        assert_eq!(status, FOUND);

        // A needle and no file is no run.
        let (_, err, status) = grep(&["only a needle"]);
        assert_eq!((err.len(), status), (1, UNREAD));
    }

    /// **The flags reach the search**: case, whole word, diacritics, the
    /// keep- options and the sources, each as the facade reads it; and a
    /// source the verb does not know is refused by name.
    #[test]
    fn the_flags_are_the_search_options() {
        let files = fixtures();
        let simple = files[0].as_str();
        let count = |args: &[&str]| grep(args).0.len();
        assert_eq!(count(&["TINKER FIXTURE", simple]), 3);
        assert_eq!(count(&["TINKER FIXTURE", "--case-sensitive", simple]), 0);
        assert_eq!(count(&["fix", simple]), 3);
        assert_eq!(count(&["fix", "--whole-word", simple]), 0);
        assert_eq!(
            count(&["3 tinker", simple]),
            2,
            "each page's end and the next page's start"
        );
        assert_eq!(count(&["3 tinker", "--keep-lines", simple]), 0);
        assert_eq!(count(&["3 tinker", "--keep-paragraphs", simple]), 0);
        assert_eq!(count(&["3 tinker", "--keep-hyphens", simple]), 2);
        assert_eq!(count(&["tinker", "--ignore-diacritics", simple]), 3);

        let parsed = Options::parse(&[
            "x".to_string(),
            "--case-sensitive".to_string(),
            "--whole-word".to_string(),
            "--ignore-diacritics".to_string(),
            "--keep-lines".to_string(),
            "--keep-paragraphs".to_string(),
            "--keep-hyphens".to_string(),
            "--sources".to_string(),
            "outline,attached".to_string(),
            simple.to_string(),
        ])
        .expect("parses");
        let options = search_options(&parsed);
        assert!(options.search.case_sensitive && options.search.whole_word);
        assert!(options.search.diacritic_insensitive);
        assert!(options.keep_lines && options.keep_paragraphs && options.keep_hyphens);
        assert_eq!(
            options.sources,
            SearchSources {
                pages: false,
                annotations: false,
                form_fields: false,
                outline: true,
                attachments: false,
                attached_documents: true,
            }
        );
        let default = Options::parse(&["x".to_string(), simple.to_string()]).expect("parses");
        assert_eq!(search_options(&default), DocumentSearchOptions::default());
        assert_eq!(sources("all"), Ok(SearchSources::ALL));
        assert!(sources("pages,pagez").is_err_and(|why| why.contains("`pagez`")));
        assert!(Options::parse(&[
            "--sources".to_string(),
            "everything".to_string(),
            "x".to_string(),
            simple.to_string(),
        ])
        .is_err());
    }

    /// **`tpdf --help` lists the verb and its flags.**
    #[test]
    fn the_usage_names_the_verb_and_its_flags() {
        assert!(crate::USAGE.contains("tpdf grep"));
        for flag in [
            "--case-sensitive",
            "--whole-word",
            "--ignore-diacritics",
            "--keep-lines",
            "--keep-paragraphs",
            "--keep-hyphens",
            "--sources",
        ] {
            assert!(crate::USAGE.contains(flag), "{flag}");
        }
        for name in SOURCES {
            assert!(crate::USAGE.contains(name), "{name}");
        }
    }

    /// Each place a hit can be is named as the design says, a hit inside an
    /// attachment prefixed by the attachments it is under.
    #[test]
    fn each_place_is_named_on_its_line() {
        use tinker_pdf::{PageQuads, Quad};
        let quad = Quad {
            ul: (0.0, 1.0),
            ur: (1.0, 1.0),
            ll: (0.0, 0.0),
            lr: (1.0, 0.0),
        };
        let file = "testdata/a.pdf";
        let doc = tinker_pdf::Document::open(std::fs::read(&fixtures()[0]).expect("the fixture"))
            .expect("it opens");
        let mut hit = doc
            .search("page 3 of 3", &DocumentSearchOptions::default())
            .next()
            .expect("a hit");
        assert_eq!(
            line(file, &hit),
            "testdata/a.pdf:page 3: Tinker fixture, [page 3 of 3]"
        );
        hit.place = HitPlace::Pages(vec![
            PageQuads {
                page: 1,
                quads: vec![quad],
            },
            PageQuads {
                page: 2,
                quads: vec![quad],
            },
        ]);
        assert!(line(file, &hit).contains(":pages 2-3: "));
        hit.attachment_path = vec!["a.pdf".to_string(), "b.pdf".to_string()];
        assert!(line(file, &hit).contains(":attached a.pdf > attached b.pdf > pages 2-3: "));
        hit.attachment_path.clear();
        for (place, named) in [
            (
                HitPlace::Annotation {
                    page: 0,
                    index: 1,
                    rect: (0.0, 0.0, 1.0, 1.0),
                    text: AnnotationText::RichText,
                    range: 0..1,
                },
                "annotation 2 on page 1 (rich text)",
            ),
            (
                HitPlace::FormField {
                    name: "address.city".to_string(),
                    widgets: Vec::new(),
                    value: 0,
                    range: 0..1,
                },
                "field address.city",
            ),
            (
                HitPlace::Outline {
                    path: vec![0, 1],
                    titles: vec!["Part".to_string(), "Two".to_string()],
                    range: 0..1,
                },
                "outline Part > Two",
            ),
            (
                HitPlace::Attachment {
                    name: "data.csv".to_string(),
                    text: AttachmentText::Description,
                    range: 0..1,
                },
                "attachment data.csv (description)",
            ),
        ] {
            hit.place = place;
            assert!(line(file, &hit).contains(&format!(":{named}: ")), "{named}");
        }
        let entry = |path: &[&str], reason| Unsearched {
            attachment_path: path.iter().map(|name| (*name).to_string()).collect(),
            reason,
        };
        assert_eq!(
            unsearched(&entry(&[], UnsearchedReason::PasswordRequired)),
            "not searched: a password is required"
        );
        assert_eq!(
            unsearched(&entry(&["a.pdf", "b.pdf"], UnsearchedReason::TooDeep)),
            "attached a.pdf > attached b.pdf not searched: it lies past 4 levels of attached document"
        );
        assert!(unsearched(&entry(&["big"], UnsearchedReason::TooLarge)).contains("32 MiB"));
        assert!(unsearched(&entry(&["n"], UnsearchedReason::TooMany)).contains("256"));
        assert!(unsearched(&entry(&["u"], UnsearchedReason::Unreadable)).contains("cannot be read"));
        assert!(
            unsearched(&entry(&["e"], UnsearchedReason::UnsupportedEncryption))
                .contains("encryption")
        );
    }
}
