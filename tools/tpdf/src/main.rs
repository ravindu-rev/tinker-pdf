//! `tpdf` — the engine's command-line front end.
//!
//! It exists for three reasons. It is how a person looks at what the engine
//! thinks of a file without writing Rust, which is most of debugging a corpus
//! failure; it is the thing a corpus runner invokes, so every capability the
//! runner needs has to be reachable from here; and its write half (`writing`)
//! is how a person merges, splits, rotates, encrypts, decrypts, attaches to,
//! stamps and sanitises a document without writing Rust either — each a
//! wrapper over the facade (ruling 11), with the two refusals of its own that
//! `writing`'s documentation names and the facade does not make yet.
//!
//! Argument parsing is hand-rolled along with everything else. It is a
//! sub-command plus flags, which needs no library.

#[cfg(test)]
mod doubled_peak;
mod images;
mod source;
mod writing;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

/// `println!` into the probe's record: line-buffered stdout in a run, a
/// buffer a test reads back. A line that cannot be written is lost as a
/// `println!` that cannot be is, and the runner reads what arrived.
macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{
        let _ = writeln!($out, $($arg)*);
    }};
}

use tinker_pdf::{
    BilevelCodec, Bitmap, CosDocument, Dict, Document, FontPolicy, HeaderEvidence,
    InferenceOptions, InferredOrder, LadderLevel, ObjRef, Object, OrderedText, Page, PageTables,
    Parallelism, ReadingOrder, RenderOptions, Sanitise, SimpleFontProvider, StreamObj,
    StructureTree, TableEvidence, TableSource, TextFormat, TextWriter, Tier, WriteMode,
    WriteOptions, XrefEntry,
};

const USAGE: &str = "\
tpdf — inspect and convert PDFs with the tinker-pdf engine

usage:
  tpdf info    <file.pdf> [--password P]
  tpdf text    <file.pdf> [--page N | --pages LIST] [--password P]
                          [--json | --xml | --html | --order ORDER | --tables]
  tpdf render  <file.pdf> --out DIR [--page N | --pages LIST] [--dpi D]
                                    [--jobs N] [--no-annotations]
  tpdf fields  <file.pdf> [--password P]
  tpdf fonts   <file.pdf> [--out DIR] [--password P]
  tpdf images  <file.pdf> [--out DIR] [--page N | --pages LIST] [--password P]
  tpdf outline <file.pdf> [--password P]
  tpdf objects <file.pdf> [--object N [--stream [--raw]]] [--password P]
  tpdf check   <file.pdf>... [--strict] [--pdfa] [--pdfua] [--pdfx]
  tpdf probe   <file.pdf>... [--dpi D] [--fonts PATH] [--stages [--stage-limit S]]

writing (each writes a new file, and takes --font-policy and the image flags):
  tpdf merge    <a.pdf> <b.pdf>... --out FILE
  tpdf split    <file.pdf> --out DIR [--pages LIST]
  tpdf rotate   <file.pdf> --by DEGREES --out FILE [--page N | --pages LIST]
  tpdf encrypt  <file.pdf> --out FILE [--user-password U] [--owner-password O]
                           [--permissions P] [--entropy FILE] [--password P]
  tpdf decrypt  <file.pdf> --out FILE [--password P]
  tpdf attach   <file.pdf> --attach FILE --out FILE [--name NAME]
                           [--mime TYPE] [--description TEXT]
  tpdf stamp    <file.pdf> --stamp FILE --out FILE [--stamp-page N] [--under]
                           [--page N | --pages LIST]
  tpdf sanitise <file.pdf> --out FILE [--javascript] [--actions]
                           [--embedded-files] [--metadata] [--all]

options:
  --page N     one page, 1-based; the default is every page
  --pages LIST pages and ranges, 1-based: `1-3,5`, acted on in that order
  --object N   one object, by number; the default is a summary of all
  --stream     with --object, write that object's stream data to stdout
  --raw        with --stream, before the filters rather than after
  --dpi D      resolution for render (default 150)
  --jobs N     render N pages at once (default 1)
  --out DIR    where render writes its PNGs
  --fonts PATH a face, or a directory of faces, for documents that embed none
  --fonts bundled
               the faces this build carries; refused unless it carries any
  --password P the password to open an encrypted file with
  --stages     with probe, each page's render taken apart into announced,
               timed stages: lookup, fonts, images, record, draw, annotations
  --stage-limit S
               with --stages, cancel draw and annotations after S seconds
  --quiet      only report failures
  --strict     with check, also validate against ISO 32000 strictly
  --pdfa       with check, also validate against ISO 19005 (PDF/A)
  --pdfua      with check, also validate against ISO 14289 (PDF/UA), and
               print the clauses it abstains on
  --pdfx       with check, also validate against ISO 15930 (PDF/X): the 2003
               levels only, and the clauses it did not read
  --json       with text, the structured text as JSON: pages, blocks, lines,
               spans with their font and size, and characters with their boxes
  --xml        the same model as XML
  --html       the same model as an HTML page that shows each line where the
               page puts it
  --order stream|stated|inferred
               with text, the order the text is read in: the content
               stream's (the default); the structure tree's, refused for a
               document that carries none; or one inferred from the page's
               geometry (columns, running heads and feet, page numbers,
               footnotes, ruled tables), which is the tree's wherever the
               document carries one. Standard error says which each page
               got, and what an inference had to tolerate
  --tables     with text, each page's tables instead of its text: the ones its
               structure tree states, or else ones inferred from what the page
               draws, labelled as such; a line for each table, then one for
               each cell

writing options:
  --out FILE   the file a writing command writes; for split, a directory
  --font-policy subset|keep
               what happens to embedded font programs on the way out: cut to
               the glyphs the document still draws (the default), or kept
  --images keep|flate|jpeg
               how images of more than one bit are coded on the way out; with
               none of --images, --bilevel and --max-ppi every image is
               written as the file stored it (the default)
  --jpeg-tables FILE
               with --images jpeg, the quantisation tables: 128 bytes, the
               luminance table then the chrominance one, natural order
  --jpeg-subsampled
               with --images jpeg, 4:2:0 chrominance rather than 4:4:4
  --bilevel keep|flate|g4|jbig2
               how one-bit images are coded on the way out
  --max-ppi N  no placement of an image finer than N pixels an inch
  --by DEGREES with rotate, a multiple of 90, clockwise, on top of the turn
               each page already has
  --owner-password O
               with encrypt, the password that lifts every restriction
               (default none: the user password is the owner's too, so it
               opens the file with every permission)
  --user-password U
               with encrypt, the password a reader needs to open the file
               (default empty: anyone opens it, and the permissions ask; with
               neither password, anyone opens it with every permission)
  --permissions P
               with encrypt, /P as ISO 32000 Table 22 stores it (default -1,
               everything permitted)
  --entropy FILE
               with encrypt, where the 48 random bytes come from (default the
               system's /dev/urandom, where there is one)
  --attach FILE, --name NAME, --mime TYPE, --description TEXT
               with attach, the file, the name it is filed under (default its
               own), its MIME type and its description
  --stamp FILE, --stamp-page N, --under
               with stamp, the document whose page is drawn, which page
               (default 1), and beneath the page's content rather than over it
  --javascript, --actions, --embedded-files, --metadata
               with sanitise, what to take out, at least one of them
  --all        with sanitise, all four

Passwords are taken on the command line, where another user of the machine
can read a running process's arguments and a shell may keep them in its
history. There is no file, environment or prompt alternative yet.

`--jobs` is the one flag that is meant to change nothing but the clock. The
pages of one document are independent — each writes its own file — so
`render` walks them on the engine's own pool, `Document::walk_pages` with
`--jobs` threads: each page is rendered and written to its PNG on a worker,
which writes its page before it takes the next, so at most `--jobs` bitmaps
are in flight, and its lines are handed back in page order and printed after
the walk, so `--jobs 8` and `--jobs 1` write the same bytes to the same files
and to stdout. The default is 1.

`check` opens each file and reports its warnings, exiting non-zero if any
file failed to open at all. It never renders, so it is the fast pass over a
corpus.

`--strict` adds the validator of ruling 13: the file is read again with the
leniency ladder off and held to the structures a tolerant read never consults
-- the cross-reference sections as the bytes spell them, stream extents
against `endstream`, the trailer against Table 15. Any defect exits non-zero,
because the question `--strict` asks is not whether it opened but whether it
is right.

`--pdfa` asks a different question again: not whether the file is a valid
PDF but whether it is a valid *archival* one. A file can be one and not the
other in both directions, which is why this is a separate flag rather than a
level of `--strict`. Each file prints the flavour it claims, every finding
with its clause, and which rule groups ran -- the last of those because this
build does not implement all of ISO 19005, and \"no findings\" from a partial
sweep is not \"it conforms\". A file claiming no flavour is reported and is not
a failure: most PDFs are not PDF/A and are not pretending to be.

`--pdfua` asks the accessibility question of ISO 14289 the same way, with
one line more: most of that standard is a judgement about meaning no reader
can make, so each file also prints how many clauses this build abstained on,
staged and undecidable, beside the groups that ran. A file claiming no part
is reported, numbered as part 1 numbers it, and is not a failure.

`--pdfx` asks the print-exchange question of ISO 15930. Rules run under
PDF/X-1a:2003 and PDF/X-3:2003 only, transcribed from the CGATS application
notes because the standard's own text is not in hand; any other level a file
claims is named and not checked, and the line beside the findings counts the
clauses abstained on, staged and unread. A file claiming no level is reported
and is not a failure.

`probe` is the one the corpus runner spawns, one child process per file. It
opens the file, renders every page, rewrites it and validates the rewrite, and
writes a line-oriented record of what happened to stdout, ending in `done`. That last line is the whole point: a
child that panicked, aborted or was killed for hanging leaves a record with no
`done` in it, so the runner can tell a file that failed from a file that never
finished, which a summary line printed at the end could not.

Two of its lines are times, in microseconds by its own clock: `time open`,
opening the document and counting its pages, with the read left out; and
`time page N` after each page, that page's render alone. The rewrite, its
validation and the metamorphic renders are outside both; `ms` spans them all.

It exits 0 whenever it finished, including for a file that would not open — a
file that fails to open is a result to be counted, not an error in the tool,
and an exit code that conflated them would make every unopenable file look
like a crashed run.

`fonts` lists every font the pages of a document can reach — once each,
whatever number of pages and resource names arrive at it — with its family,
its subset tag, whether the file carries its program, and the names it
answered to. `--out DIR` additionally writes each embedded program to that
directory, which is the only thing here that pays for a font program: the
listing itself reads names and never a stream.

Do not confuse it with the `--fonts` flag above. `fonts` *reads* the faces a
document carries; `--fonts` *supplies* faces to `render` and `probe` for a
document that carries none, which is the opposite direction.

`objects` is the view underneath every other command: what the engine
actually parsed, object by object. It is what a corpus failure gets looked
at with, which is why it prints the cross-reference kind alongside each
object — an object the table says lives in an object stream and an object
found by the repair scanner read the same afterwards, and the difference
is usually the bug.

`images` lists every image each page draws — once however often it is drawn,
with how many times — and its geometry, depth, component count, the codec
that produced its samples and the colour space they are in. `--out DIR`
writes each image's samples to `<stem>-pNNNN-NNN.raw` exactly as the engine
decoded them: before any colour conversion, `/Decode` not applied, rows from
the top padded to a byte. A soft mask's samples go beside them as
`-smask.raw` and a stencil mask's as `-mask.raw`. The colour space goes with
them: a CIE space's parameters on the listing line, an indexed palette as
`-palette.raw`, and every ICC profile in the space as `.icc` — `-base.icc`
for the space a palette's entries are in, `-alternate.icc` for an alternate.
A separation's tint transform is not written; the engine does not hand it
over. Nothing is converted to a picture format, because that would mean
evaluating the colour space; the listing and those files are what reading
the bytes needs instead.

The writing commands each make the editor calls their name says and save
through the library's own save door, so each takes `--font-policy`, and each
subsets the embedded fonts by default as the library does: a program cut down
to the glyphs the document still draws no longer carries the outlines of the
ones it does not. What the pass did is printed with every file written, and a
program it left whole is named with the reason, because that program still
carries every outline it had. The image flags are the library's image policy
on the same door, off unless one is given: a coding per kind of image and a
resolution, each image recoded or named with why it was left as stored. JPEG
codes with the tables `--jpeg-tables` names, because the library takes the
caller's and this tool chooses none.

Every one of them writes the whole file afresh, never an update appended to
the old bytes: an update keeps the original as its prefix, so the pages a
split left out or the scripts a sanitise removed would still be in the file.
A signature over the original bytes does not cover the new ones.

A writing command refuses a flag it would ignore rather than writing the file
without it — `--fonts` (faces to draw with; the font policy is
`--font-policy`), another command's flags, or `--password` on an input that is
not encrypted — and refuses a page `--pages` names twice, which `rotate` would
turn twice and `stamp` stamp twice. `split` pieces may overlap; the same piece
twice is refused.

`merge` keeps the first file's catalog — outline, form, labels — and appends
the pages of each later one, writing a resource those pages share once.
`split` writes one file per item of `--pages`, or one per page, and each piece
carries only what its pages reach, with one exception it does not fix: a page
the outline, a named destination or a link still names stays in the file,
outside the page tree. `split` is not a redaction.

An encrypted input is refused by every writing command but `encrypt` and
`decrypt`, because a rewrite that asks for no encryption writes the
plaintext; so is an encrypted later input to `merge` or `--stamp` file,
whose pages would be written so too. `decrypt`, and `encrypt` over a file
that already is, need the owner password unless the owner withheld nothing
from the user: they would lift the restrictions, which the library reports
rather than enforcing. Both are the library's answers, which this tool asks
before it writes; the library's own save, and its bindings, do not ask yet.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let options = match Options::parse(&args[1..]) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("tpdf: {message}");
            return ExitCode::from(2);
        }
    };

    let result = match args[0].as_str() {
        "info" => run(&options, info),
        "text" => run(&options, text),
        "render" => run(&options, render),
        "fields" => run(&options, fields),
        "fonts" => run(&options, fonts),
        "images" => run(&options, images::images),
        "outline" => run(&options, outline),
        "objects" => run_whole(&options, objects),
        "check" => check(&options),
        "probe" => probe(&options),
        "merge" => writing::print(&options, writing::merge(&options)),
        "split" => writing::print(&options, writing::split(&options)),
        "rotate" => writing::print(&options, writing::rotate(&options)),
        "encrypt" => writing::print(&options, writing::encrypt(&options)),
        "decrypt" => writing::print(&options, writing::decrypt(&options)),
        "attach" => writing::print(&options, writing::attach(&options)),
        "stamp" => writing::print(&options, writing::stamp(&options)),
        "sanitise" => writing::print(&options, writing::sanitise(&options)),
        other => Err(format!("unknown command `{other}`; try --help")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("tpdf: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    files: Vec<String>,
    page: Option<u32>,
    object: Option<u32>,
    dpi: f64,
    /// How many pages `render` draws at once.
    ///
    /// **One by default**, so nothing about an existing invocation changes
    /// unless it is asked for — not the wall time, not the number of threads
    /// in the process, and not one byte of the output either way.
    jobs: usize,
    out: Option<String>,
    fonts: Option<String>,
    password: Option<String>,
    annotations: bool,
    quiet: bool,
    raw: bool,
    stream: bool,
    strict: bool,
    /// Validate against ISO 19005 (PDF/A) as well, and exit by the verdict.
    ///
    /// Separate from `--strict` because they answer different questions and a
    /// caller wants one or the other: `--strict` asks whether the file is a
    /// valid PDF, `--pdfa` asks whether it is a valid *archival* PDF. A file
    /// can be one and not the other in both directions.
    pdfa: bool,
    /// Validate against ISO 14289 (PDF/UA) as well, and exit by the verdict.
    pdfua: bool,
    /// Validate against ISO 15930 (PDF/X) as well, and exit by the verdict.
    pdfx: bool,
    /// `text` in a structured format rather than as plain text: one of
    /// `--json`, `--xml` and `--html`, and at most one.
    format: Option<TextFormat>,
    /// `--order`, for `text`: which order plain text is read in. The
    /// content stream's unless asked, so no existing invocation changes.
    order: ReadingOrder,
    /// `--tables`, for `text`: the page's tables rather than its text.
    tables: bool,
    /// Print the record format version and stop, naming no file.
    ///
    /// The corpus runner asks before it spawns anything, because a child one
    /// version behind writes a complete record the runner then refuses, once
    /// per file — and four thousand refusals read as an engine that stopped
    /// rendering rather than as a binary that needs rebuilding. Asking costs
    /// one process at the start of a run that spawns thousands.
    record_version: bool,
    /// `--stages`, for `probe`: each page's render taken apart into announced
    /// stages in place of its one `page.render` (MS-20).
    stages: bool,
    /// `--stage-limit S`: the seconds `draw` and `annotations` run before
    /// the probe cancels them; `None` cancels nothing.
    stage_limit: Option<u64>,
    /// `--pages LIST`: inclusive ranges, 0-based. Exclusive with `--page`.
    page_ranges: Option<Vec<(u32, u32)>>,
    /// `--font-policy`: what a command that rewrites a document does to its
    /// embedded font programs on the way out. The facade's default, subset.
    font_policy: FontPolicy,
    /// `--by DEGREES`, for `rotate`: a quarter-turn multiple, clockwise.
    by: Option<i64>,
    /// `--user-password`, for `encrypt`: what a reader needs to open the file.
    user_password: Option<String>,
    /// `--owner-password`, for `encrypt`: what lifts the restrictions.
    owner_password: Option<String>,
    /// `--permissions P`, for `encrypt`: `/P` as Table 22 stores it.
    permissions: Option<i32>,
    /// `--entropy FILE`, for `encrypt`: where the 48 random bytes come from.
    entropy: Option<String>,
    /// `--attach FILE`, for `attach`.
    attach: Option<String>,
    /// `--name NAME`, for `attach`: the key the file is filed under.
    name: Option<String>,
    /// `--mime TYPE`, for `attach`: the embedded file stream's `/Subtype`.
    mime: Option<String>,
    /// `--description TEXT`, for `attach`: the file specification's `/Desc`.
    description: Option<String>,
    /// `--stamp FILE`, for `stamp`: the document whose page is drawn.
    stamp: Option<String>,
    /// `--stamp-page N`, for `stamp`: which of its pages, 0-based here.
    stamp_page: u32,
    /// `--under`, for `stamp`: beneath the page's content rather than over it.
    under: bool,
    /// `--javascript`, `--actions`, `--embedded-files` and `--metadata`, for
    /// `sanitise`, each one field; `--all` is `Sanitise::ALL`.
    sanitise: Sanitise,
    /// `--images`, for every writing command: how a continuous-tone image is
    /// coded on the way out. With `--bilevel` and `--max-ppi`, the image
    /// policy; none of the three is the facade's default, keep every image.
    images: Option<writing::Continuous>,
    /// `--bilevel`: how a one-bit image is coded on the way out.
    bilevel: Option<BilevelCodec>,
    /// `--max-ppi N`: the finest resolution any placement of an image is left
    /// at.
    max_ppi: Option<f64>,
    /// `--jpeg-tables FILE`, with `--images jpeg`: the caller's quantisation
    /// tables, 64 luminance bytes and then 64 chrominance, natural order.
    jpeg_tables: Option<String>,
    /// `--jpeg-subsampled`, with `--images jpeg`: 4:2:0 chrominance.
    jpeg_subsampled: bool,
    /// Every flag given, as spelt, in order: what a writing command checks
    /// against the flags it takes, so one it would ignore is refused instead
    /// (`writing::takes`).
    given: Vec<String>,
}

/// `--pages 1-3,5` as inclusive 0-based ranges, in the order given.
///
/// A range that runs backwards is refused rather than reversed: `5-3` is more
/// often a typo than a request for three pages in reverse, and the pages are
/// acted on in the order given, so guessing would produce a different answer.
fn page_ranges(raw: &str) -> Result<Vec<(u32, u32)>, String> {
    let mut ranges = Vec::new();
    for item in raw.split(',') {
        let (first, last) = item.split_once('-').unwrap_or((item, item));
        let number = |text: &str| -> Result<u32, String> {
            let n: u32 = text
                .trim()
                .parse()
                .map_err(|_| format!("`--pages {raw}`: `{item}` is not a page or a range"))?;
            n.checked_sub(1)
                .ok_or_else(|| "pages are numbered from 1".to_string())
        };
        let (first, last) = (number(first)?, number(last)?);
        if first > last {
            return Err(format!("`--pages {raw}`: `{item}` runs backwards"));
        }
        ranges.push((first, last));
    }
    Ok(ranges)
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let mut options = Options {
            files: Vec::new(),
            page: None,
            object: None,
            dpi: 150.0,
            jobs: 1,
            out: None,
            fonts: None,
            password: None,
            annotations: true,
            quiet: false,
            raw: false,
            stream: false,
            strict: false,
            pdfa: false,
            pdfua: false,
            pdfx: false,
            format: None,
            order: ReadingOrder::Stream,
            tables: false,
            record_version: false,
            stages: false,
            stage_limit: None,
            page_ranges: None,
            font_policy: FontPolicy::default(),
            by: None,
            user_password: None,
            owner_password: None,
            permissions: None,
            entropy: None,
            attach: None,
            name: None,
            mime: None,
            description: None,
            stamp: None,
            stamp_page: 0,
            under: false,
            sanitise: Sanitise::default(),
            images: None,
            bilevel: None,
            max_ppi: None,
            jpeg_tables: None,
            jpeg_subsampled: false,
            given: Vec::new(),
        };

        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            if arg.starts_with("--") {
                options.given.push(arg.to_string());
            }
            // A flag that takes a value consumes the next argument, and
            // running off the end is an error rather than a default.
            let mut value = || -> Result<String, String> {
                index += 1;
                args.get(index)
                    .cloned()
                    .ok_or_else(|| format!("`{arg}` needs a value"))
            };

            match arg {
                "--page" => {
                    let raw = value()?;
                    let n: u32 = raw
                        .parse()
                        .map_err(|_| format!("`--page {raw}` is not a number"))?;
                    if n == 0 {
                        return Err("pages are numbered from 1".to_string());
                    }
                    options.page = Some(n - 1);
                }
                "--dpi" => {
                    let raw = value()?;
                    let d: f64 = raw
                        .parse()
                        .map_err(|_| format!("`--dpi {raw}` is not a number"))?;
                    if !d.is_finite() || d <= 0.0 {
                        return Err(format!("`--dpi {raw}` is not a resolution"));
                    }
                    options.dpi = d;
                }
                // Zero is refused rather than clamped: `--jobs 0` is a person
                // asking for something, and a run that silently did the
                // opposite of what the number said would be the wrong kind of
                // forgiving. Same spelling as `cargo xtask corpus-run`'s.
                "--jobs" => {
                    let raw = value()?;
                    options.jobs = raw
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("`--jobs {raw}` is not a positive number"))?;
                }
                "--object" => {
                    let raw = value()?;
                    let n: u32 = raw
                        .parse()
                        .map_err(|_| format!("`--object {raw}` is not a number"))?;
                    options.object = Some(n);
                }
                "--out" => options.out = Some(value()?),
                "--fonts" => options.fonts = Some(value()?),
                "--password" => options.password = Some(value()?),
                "--no-annotations" => options.annotations = false,
                "--quiet" => options.quiet = true,
                "--raw" => options.raw = true,
                "--stream" => options.stream = true,
                "--strict" => options.strict = true,
                "--pdfa" => options.pdfa = true,
                "--pdfua" => options.pdfua = true,
                "--pdfx" => options.pdfx = true,
                "--record-version" => options.record_version = true,
                "--stages" => options.stages = true,
                "--stage-limit" => {
                    let raw = value()?;
                    options.stage_limit = Some(raw.parse().map_err(|_| {
                        format!("`--stage-limit {raw}` is not a number of seconds")
                    })?);
                }
                "--json" | "--xml" | "--html" => {
                    let format = match arg {
                        "--json" => TextFormat::Json,
                        "--xml" => TextFormat::Xml,
                        _ => TextFormat::Html,
                    };
                    if options.format.is_some_and(|f| f != format) {
                        return Err("choose one of --json, --xml and --html".to_string());
                    }
                    options.format = Some(format);
                }
                "--pages" => options.page_ranges = Some(page_ranges(&value()?)?),
                // The three orders `ReadingOrder` names. Anything else is
                // refused: a typo that fell back to the stream would print a
                // page in the one order the caller asked to be spared.
                "--order" => {
                    let raw = value()?;
                    options.order = match raw.as_str() {
                        "stream" => ReadingOrder::Stream,
                        "stated" => ReadingOrder::Stated,
                        "inferred" => ReadingOrder::Inferred,
                        _ => {
                            return Err(format!(
                                "`--order {raw}`: the orders are `stream`, `stated` and `inferred`"
                            ))
                        }
                    };
                }
                "--tables" => options.tables = true,
                // The two values `FontPolicy` has, by the names it gives them.
                // Anything else is refused: a typo that fell back to the
                // default would subset a document somebody asked to keep
                // whole, or the other way round.
                "--font-policy" => {
                    let raw = value()?;
                    options.font_policy = match raw.as_str() {
                        "subset" => FontPolicy::Subset,
                        "keep" => FontPolicy::Keep,
                        _ => {
                            return Err(format!(
                                "`--font-policy {raw}`: the policies are `subset` and `keep`"
                            ))
                        }
                    };
                }
                // Passed to `rotate_page` as given: a turn that is not a
                // multiple of 90 is the facade's to refuse (7.7.3.3).
                "--by" => {
                    let raw = value()?;
                    let degrees: i64 = raw
                        .parse()
                        .map_err(|_| format!("`--by {raw}` is not a number"))?;
                    options.by = Some(degrees);
                }
                "--user-password" => options.user_password = Some(value()?),
                "--owner-password" => options.owner_password = Some(value()?),
                "--permissions" => {
                    let raw = value()?;
                    options.permissions = Some(
                        raw.parse()
                            .map_err(|_| format!("`--permissions {raw}` is not a /P value"))?,
                    );
                }
                "--entropy" => options.entropy = Some(value()?),
                "--attach" => options.attach = Some(value()?),
                "--name" => options.name = Some(value()?),
                "--mime" => options.mime = Some(value()?),
                "--description" => options.description = Some(value()?),
                "--stamp" => options.stamp = Some(value()?),
                "--stamp-page" => {
                    let raw = value()?;
                    let n: u32 = raw
                        .parse()
                        .map_err(|_| format!("`--stamp-page {raw}` is not a number"))?;
                    options.stamp_page = n
                        .checked_sub(1)
                        .ok_or_else(|| "pages are numbered from 1".to_string())?;
                }
                "--under" => options.under = true,
                "--javascript" => options.sanitise.javascript = true,
                "--actions" => options.sanitise.actions = true,
                "--embedded-files" => options.sanitise.embedded_files = true,
                "--metadata" => options.sanitise.metadata = true,
                "--all" => options.sanitise = Sanitise::ALL,
                // The codings `ContinuousCodec` and `BilevelCodec` have, by
                // their names, and refused otherwise for `--font-policy`'s
                // reason.
                "--images" => {
                    let raw = value()?;
                    options.images = Some(match raw.as_str() {
                        "keep" => writing::Continuous::Keep,
                        "flate" => writing::Continuous::Flate,
                        "jpeg" => writing::Continuous::Jpeg,
                        _ => {
                            return Err(format!(
                                "`--images {raw}`: the codings are `keep`, `flate` and `jpeg`"
                            ))
                        }
                    });
                }
                "--bilevel" => {
                    let raw = value()?;
                    options.bilevel = Some(match raw.as_str() {
                        "keep" => BilevelCodec::Keep,
                        "flate" => BilevelCodec::Flate,
                        "g4" => BilevelCodec::CcittG4,
                        "jbig2" => BilevelCodec::Jbig2Generic,
                        _ => {
                            return Err(format!(
                            "`--bilevel {raw}`: the codings are `keep`, `flate`, `g4` and `jbig2`"
                        ))
                        }
                    });
                }
                // The facade resamples nothing for a value that is not a
                // finite positive number; a person who typed one asked for
                // something, so it is refused here rather than ignored there.
                "--max-ppi" => {
                    let raw = value()?;
                    let ppi: f64 = raw
                        .parse()
                        .map_err(|_| format!("`--max-ppi {raw}` is not a number"))?;
                    if !ppi.is_finite() || ppi <= 0.0 {
                        return Err(format!("`--max-ppi {raw}` is not a resolution"));
                    }
                    options.max_ppi = Some(ppi);
                }
                "--jpeg-tables" => options.jpeg_tables = Some(value()?),
                "--jpeg-subsampled" => options.jpeg_subsampled = true,
                _ if arg.starts_with("--") => return Err(format!("unknown option `{arg}`")),
                _ => options.files.push(arg.to_string()),
            }
            index += 1;
        }

        // `--record-version` names no file by design: it asks what this
        // binary writes, which is true before any document exists.
        if options.files.is_empty() && !options.record_version {
            return Err("no input file".to_string());
        }
        if options.page.is_some() && options.page_ranges.is_some() {
            return Err("choose one of --page and --pages".to_string());
        }
        // The structured formats write the text device's model, which is the
        // stream's order; tables are not text in any order.
        if options.format.is_some() && (options.order != ReadingOrder::Stream || options.tables) {
            return Err(
                "--json, --xml and --html write the page in its stream order: \
                 --order and --tables are for plain text"
                    .to_string(),
            );
        }
        if options.tables && options.order != ReadingOrder::Stream {
            return Err("choose one of --order and --tables".to_string());
        }
        Ok(options)
    }

    /// `--page` or `--pages` as inclusive 0-based ranges, or `None` when
    /// neither was given and every page is meant.
    fn ranges(&self) -> Option<Vec<(u32, u32)>> {
        match (&self.page_ranges, self.page) {
            (Some(ranges), _) => Some(ranges.clone()),
            (None, Some(page)) => Some(vec![(page, page)]),
            (None, None) => None,
        }
    }

    /// The pages a reading command acts on: the ones asked for, or all of
    /// them.
    ///
    /// A page past the end is skipped rather than refused, as `--page` always
    /// was here — a reading command over a directory of files is the normal
    /// case — and a range is cut at the last page before it is counted out, so
    /// `--pages 1-4000000000` costs what the document has.
    fn pages(&self, doc: &Document) -> Vec<u32> {
        let count = doc.page_count();
        match self.ranges() {
            Some(ranges) => ranges
                .into_iter()
                .flat_map(|(first, last)| first..=last.min(count.saturating_sub(1)))
                .collect(),
            None => (0..count).collect(),
        }
    }

    /// The faces `--fonts` names, loaded once rather than per file.
    ///
    /// The engine bundles no faces and reads no font directories, so a
    /// document that embeds nothing draws nothing and reports
    /// `UnreadableFont`. That is correct and it is also the single largest
    /// term in any corpus's warning count, which is why the corpus runner
    /// measures both with and without: a number that is mostly "this build
    /// ships no fonts" says very little about the engine.
    fn font_provider(&self) -> Result<Option<Arc<SimpleFontProvider>>, String> {
        let Some(path) = self.fonts.as_deref() else {
            return Ok(None);
        };

        // `--fonts bundled` is not a path: it means "whatever faces this build
        // carries", which the facade supplies without a provider at all. It
        // exists so a corpus run can *say* it measured them, since a child
        // built without the feature would otherwise produce the no-faces
        // numbers and have them recorded as the bundled bar.
        //
        // Refused outright rather than ignored when the feature is off, for
        // that reason: the whole value of the flag is that it cannot be
        // satisfied by a build that has no faces.
        if path == BUNDLED {
            #[cfg(feature = "bundled-fonts")]
            {
                return Ok(None);
            }
            #[cfg(not(feature = "bundled-fonts"))]
            {
                return Err(
                    "--fonts bundled: this build carries no faces. Rebuild with                      `--features bundled-fonts`, or name a path"
                        .to_string(),
                );
            }
        }

        let path = Path::new(path);

        if path.is_file() {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            return Ok(Some(Arc::new(SimpleFontProvider::new(bytes))));
        }
        if !path.is_dir() {
            return Err(format!(
                "--fonts {}: no such file or directory",
                path.display()
            ));
        }

        // A directory is matched by name — `...Bold`, `...Italic`,
        // `...BoldItalic` — which is how every font directory on every
        // platform is laid out. Anything else is ignored rather than guessed
        // at, and a directory yielding no regular face is an error, because a
        // silently empty provider is indistinguishable from `--fonts` never
        // having been passed and would make the two runs report the same
        // number for different reasons.
        let mut faces: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
        let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        for entry in entries.flatten() {
            let file = entry.path();
            if !file.is_file() {
                continue;
            }
            let stem = file
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let extension = file
                .extension()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !matches!(extension.as_str(), "ttf" | "otf" | "cff") {
                continue;
            }
            let bold = stem.contains("bold");
            let italic = stem.contains("italic") || stem.contains("oblique");
            let slot = match (bold, italic) {
                (true, true) => "bold-italic",
                (true, false) => "bold",
                (false, true) => "italic",
                (false, false) => "regular",
            };
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            // First wins, so a directory holding two regular faces picks the
            // alphabetically first one every time rather than whichever the
            // filesystem happened to hand back first.
            faces.entry(slot).or_insert(bytes);
        }

        let Some(regular) = faces.remove("regular") else {
            return Err(format!(
                "--fonts {}: no regular face found (looked for .ttf, .otf and \
                 .cff whose name says neither bold nor italic)",
                path.display()
            ));
        };
        let mut provider = SimpleFontProvider::new(regular);
        if let Some(bytes) = faces.remove("bold") {
            provider = provider.with_bold(bytes);
        }
        if let Some(bytes) = faces.remove("italic") {
            provider = provider.with_italic(bytes);
        }
        if let Some(bytes) = faces.remove("bold-italic") {
            provider = provider.with_bold_italic(bytes);
        }
        Ok(Some(Arc::new(provider)))
    }
}

/// How a command reads its file (`docs/design/streaming-open.md`, "Which
/// commands read on demand").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    /// Through a file source, by the rule: kept streamed when the document
    /// answers as a buffered open of it would, and read whole otherwise.
    OnDemand,
    /// Once, in place, into one buffer of the file's length.
    Whole,
}

/// `path` opened as `reads` says, with its faces and its password, and the
/// file source it reads through when the rule kept it streamed.
fn open(
    path: &str,
    reads: Reads,
    password: Option<&str>,
    fonts: Option<&Arc<SimpleFontProvider>>,
) -> Result<(Document, Option<Arc<source::FileSource>>), String> {
    let (doc, kept) = match reads {
        Reads::OnDemand => source::open_on_demand(path).map_err(|unopened| match unopened {
            source::Unopened::Read(message) => message,
            source::Unopened::Open(e) => format!("{path}: {e:?}"),
        })?,
        Reads::Whole => (
            Document::open(read_file(path)?).map_err(|e| format!("{path}: {e:?}"))?,
            None,
        ),
    };
    Ok((prepared(path, doc, password, fonts)?, kept))
}

/// The file's bytes, read once in place, or the line [`open`] reports when
/// there are none.
fn read_file(path: &str) -> Result<Arc<[u8]>, String> {
    source::read_whole(path)
}

/// [`open`] after the read: the document, its faces and its password.
///
/// Apart from the read so `probe` can time what the engine does with the
/// bytes and leave out what the disk does to fetch them.
fn open_bytes(
    path: &str,
    bytes: Arc<[u8]>,
    password: Option<&str>,
    fonts: Option<&Arc<SimpleFontProvider>>,
) -> Result<Document, String> {
    let doc = Document::open(bytes).map_err(|e| format!("{path}: {e:?}"))?;
    prepared(path, doc, password, fonts)
}

/// An opened document with its faces and its password.
fn prepared(
    path: &str,
    doc: Document,
    password: Option<&str>,
    fonts: Option<&Arc<SimpleFontProvider>>,
) -> Result<Document, String> {
    let doc = match fonts {
        Some(provider) => doc.with_fonts(provider.clone()),
        None => doc,
    };

    if doc.is_encrypted() {
        // An empty password is the usual case for a file encrypted only to
        // restrict permissions, so it is tried before giving up.
        let attempt = password.unwrap_or("");
        doc.authenticate(attempt)
            .map_err(|_| format!("{path}: the password was not accepted"))?;
    }
    Ok(doc)
}

thread_local! {
    /// The file the command on this thread reads, and the source it reads it
    /// through, while the rule kept it streamed: set by [`run_reading`] for one
    /// file at a time, and asked by [`sound`].
    static READING: RefCell<Option<(String, Arc<source::FileSource>)>> =
        const { RefCell::new(None) };
}

/// The file this thread reads through a source, when it does.
fn reading_now() -> Option<(String, Arc<source::FileSource>)> {
    READING.with(|reading| reading.borrow().clone())
}

/// Fails the file this thread reads when a read through its source failed.
///
/// Asked before anything read through the source is printed or written — a
/// file's `info` and `check` lines, each page of the commands that go page by
/// page — because a read that missed reads as absent, and a page count or a
/// page printed after one would be a part-null document's
/// (`docs/design/streaming-open.md`, "A miss, to a host that cannot fetch").
fn sound() -> Result<(), String> {
    sound_of(reading_now().as_ref())
}

/// [`sound`] for `reading`, as a worker thread holds it.
fn sound_of(reading: Option<&(String, Arc<source::FileSource>)>) -> Result<(), String> {
    match reading.and_then(|(path, source)| source.failure().map(|failure| (path, failure))) {
        Some((path, failure)) => Err(format!(
            "reading {path}: {} at bytes {}..{}",
            failure.error, failure.range.start, failure.range.end
        )),
        None => Ok(()),
    }
}

/// Runs `body` with `path`, read through `source`, as the file this thread
/// reads.
fn reading<T>(path: &str, source: Option<Arc<source::FileSource>>, body: impl FnOnce() -> T) -> T {
    READING.with(|reading| *reading.borrow_mut() = source.map(|s| (path.to_string(), s)));
    let out = body();
    READING.with(|reading| *reading.borrow_mut() = None);
    out
}

/// Runs a command over every file given, each read on demand.
fn run(
    options: &Options,
    each: fn(&Options, &str, &Document) -> Result<(), String>,
) -> Result<(), String> {
    run_reading(options, Reads::OnDemand, each)
}

/// Runs a command over every file given, each read whole.
fn run_whole(
    options: &Options,
    each: fn(&Options, &str, &Document) -> Result<(), String>,
) -> Result<(), String> {
    run_reading(options, Reads::Whole, each)
}

/// Runs a command over every file given, each read as `reads` says.
///
/// One unreadable file reports itself and the rest still run: a command given
/// a whole directory is the normal case, and stopping at the first
/// password-protected file would hide everything after it.
fn run_reading(
    options: &Options,
    reads: Reads,
    each: fn(&Options, &str, &Document) -> Result<(), String>,
) -> Result<(), String> {
    let fonts = options.font_provider()?;
    let mut failed = 0usize;
    for path in &options.files {
        let outcome = open(path, reads, options.password.as_deref(), fonts.as_ref())
            .and_then(|(doc, kept)| reading(path, kept, || each(options, path, &doc)));
        if let Err(message) = outcome {
            eprintln!("tpdf: {message}");
            failed += 1;
        }
    }

    match failed {
        0 => Ok(()),
        1 => Err("1 file failed".to_string()),
        n => Err(format!("{n} files failed")),
    }
}

fn info(_options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    for line in info_report(path, doc)? {
        println!("{line}");
    }
    Ok(())
}

/// What `info` prints for `doc`, once its source has been asked whether every
/// read behind it was made.
fn info_report(path: &str, doc: &Document) -> Result<Vec<String>, String> {
    let lines = info_lines(path, doc);
    sound()?;
    Ok(lines)
}

/// The lines `info` prints for `doc`.
fn info_lines(path: &str, doc: &Document) -> Vec<String> {
    let mut lines = Vec::new();
    let metadata = doc.metadata();
    lines.push(path.to_string());
    lines.push(format!("  pages       {}", doc.page_count()));
    lines.push(format!("  version     {}", doc.pdf_version()));
    lines.push(format!("  ladder      {:?}", doc.ladder_level()));
    lines.push(format!("  encrypted   {}", doc.is_encrypted()));
    if doc.is_encrypted() {
        lines.push(format!("  authorized  {:?}", doc.auth_level()));
        let permissions = doc.permissions();
        lines.push(format!(
            "  may         print={} copy={} modify={} annotate={}",
            permissions.print(),
            permissions.copy(),
            permissions.modify(),
            permissions.annotate()
        ));
    }

    for (label, value) in [
        ("title", &metadata.title),
        ("author", &metadata.author),
        ("subject", &metadata.subject),
        ("creator", &metadata.creator),
        ("producer", &metadata.producer),
    ] {
        if let Some(value) = value {
            lines.push(format!("  {label:11} {value}"));
        }
    }

    // Printed only when the document names it: /Trapped absent and /Trapped
    // /Unknown are different statements, and a line that appeared either way
    // would collapse them back together.
    if let Some(trapped) = metadata.trapped {
        lines.push(format!("  {:11} {trapped:?}", "trapped"));
    }

    if let Some(page) = doc.page(0) {
        let (w, h) = page.size();
        lines.push(format!(
            "  first page  {w} x {h} pt, rotated {}",
            page.rotation()
        ));
    }

    let warnings = doc.warnings();
    if !warnings.is_empty() {
        lines.push(format!("  warnings    {}", warnings.len()));
        for warning in warnings.iter().take(10) {
            lines.push(format!("    {:?}", warning.kind));
        }
        if warnings.len() > 10 {
            lines.push(format!("    ... and {} more", warnings.len() - 10));
        }
    }
    lines
}

fn text(options: &Options, _path: &str, doc: &Document) -> Result<(), String> {
    // The model and every escape are the library's (`TextWriter`); this loop
    // only chooses the pages and prints what it is handed, a page at a time.
    if let Some(format) = options.format {
        let mut writer = TextWriter::new(format);
        for index in options.pages(doc) {
            if let Some(page) = doc.page(index) {
                writer.page(&page.text_frame(), &page.text());
                sound()?;
                print!("{}", writer.take());
            }
        }
        print!("{}", writer.finish());
        return Ok(());
    }
    let pages = options.pages(doc);
    // An inferred order is read a run of consecutive pages at a time, each
    // page interpreted once for its own order and its neighbours' margins.
    // A document with a structure tree keeps `text_in`'s answer, the order
    // the tree states, page by page.
    if options.order == ReadingOrder::Inferred && !options.tables && doc.structure().is_none() {
        for (text, label) in inferred_texts(doc, &pages) {
            sound()?;
            print_page(options, &text, Some(label));
        }
        return Ok(());
    }
    for index in pages {
        let Some(page) = doc.page(index) else {
            continue;
        };
        if options.tables {
            let tables = page_tables(&page, index);
            sound()?;
            print!("{tables}");
            continue;
        }
        let (text, label) = page_text(&page, index, options.order)?;
        sound()?;
        print_page(options, &text, label);
    }
    Ok(())
}

/// One page's plain text, after its label on standard error unless
/// `--quiet`.
fn print_page(options: &Options, text: &str, label: Option<String>) {
    if let (Some(label), false) = (label, options.quiet) {
        eprintln!("{label}");
    }
    print!("{text}");
    // A form feed between pages, which is what every other text extractor
    // emits and what makes the output splittable again.
    if options.page.is_none() {
        print!("\u{c}");
    }
}

/// The plain text and the label of each page `pages` lists that exists, in
/// the order listed, in an inferred order — `page_text`'s answer for each,
/// for a document that carries no structure tree — inferred a run of
/// consecutive pages at a time through `Document::inferred_pages`, which
/// interprets each page of the run once where a page alone reads its
/// neighbours again.
fn inferred_texts<'a>(
    doc: &'a Document,
    pages: &'a [u32],
) -> impl Iterator<Item = (String, String)> + 'a {
    let mut runs: Vec<std::ops::Range<u32>> = Vec::new();
    for &index in pages {
        match runs.last_mut() {
            Some(run) if run.end == index && index < u32::MAX => run.end = index + 1,
            _ => runs.push(index..index.saturating_add(1)),
        }
    }
    runs.into_iter().flat_map(move |run| {
        doc.inferred_pages(run, &InferenceOptions::default())
            .map(|(index, order)| (order.plain_text(), inferred_label(index + 1, &order)))
    })
}

/// The line labelling page `number`'s inferred order: its columns, and what
/// the inference had to tolerate.
fn inferred_label(number: u32, inferred: &InferredOrder) -> String {
    let mut label = format!(
        "page {number}: an inferred order, {} column{}",
        inferred.columns,
        if inferred.columns == 1 { "" } else { "s" }
    );
    if !inferred.warnings.is_empty() {
        let warnings: Vec<String> = inferred.warnings.iter().map(|w| format!("{w:?}")).collect();
        label.push_str(&format!("; {}", warnings.join(", ")));
    }
    label
}

/// Page `index`'s plain text in `order`, and, for any order but the stream's,
/// the line that labels it: which order the page got — the tree's, for an
/// inference asked of a tagged page — and what an inference had to tolerate.
/// An error for the stated order of a document that states none.
fn page_text(
    page: &Page,
    index: u32,
    order: ReadingOrder,
) -> Result<(String, Option<String>), String> {
    if order == ReadingOrder::Stream {
        return Ok((page.text().plain_text(), None));
    }
    let number = index + 1;
    let Some(text) = page.text_in(order) else {
        return Err(format!(
            "page {number}: no order is stated: the document carries no structure tree"
        ));
    };
    let label = match &text {
        OrderedText::Stream(_) => format!("page {number}: the content stream's order"),
        OrderedText::Stated(_) => format!("page {number}: the structure tree's order"),
        OrderedText::Inferred(inferred) => inferred_label(number, inferred),
    };
    Ok((text.plain_text(), Some(label)))
}

/// Page `index`'s tables as lines: one naming each table — where it came
/// from, its size, and for an inferred one its evidence and what it had to
/// tolerate — then one for each cell, `row R, column C: text`, a span written
/// as a range. A line a cell, so a table of a million columns that states
/// three cells is three lines.
fn page_tables(page: &Page, index: u32) -> String {
    let number = index + 1;
    let flat = |text: &str| -> String {
        text.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let place = |first: usize, span: usize| -> String {
        if span > 1 {
            format!("{}-{}", first + 1, first.saturating_add(span))
        } else {
            (first + 1).to_string()
        }
    };
    let mut out = String::new();
    match page.tables(TableSource::Inferred) {
        PageTables::Stated(tables) => {
            for (at, table) in tables.iter().enumerate() {
                out.push_str(&format!(
                    "page {number}, table {}: stated by the structure tree, {} rows by {} columns\n",
                    at + 1,
                    table.rows,
                    table.columns
                ));
                for cell in &table.cells {
                    out.push_str(&format!(
                        "  row {}, column {}{}: {}\n",
                        place(cell.row, cell.row_span),
                        place(cell.column, cell.col_span),
                        if cell.header { " (header)" } else { "" },
                        flat(&cell.text)
                    ));
                }
            }
        }
        PageTables::Inferred(found) => {
            for (at, table) in found.tables.iter().enumerate() {
                let evidence = match table.evidence {
                    TableEvidence::Ruled => "inferred from the page's rules",
                    TableEvidence::Aligned => "inferred from aligned text, no rule drawn",
                    _ => "inferred",
                };
                let header = match table.header {
                    HeaderEvidence::FillBeneath => ", the first row shaded",
                    HeaderEvidence::RuleBeneath => ", a heavier rule under the first row",
                    _ => "",
                };
                out.push_str(&format!(
                    "page {number}, table {}: {evidence}, {} rows by {} columns{header}",
                    at + 1,
                    table.rows,
                    table.columns
                ));
                if !table.warnings.is_empty() {
                    let warnings: Vec<String> =
                        table.warnings.iter().map(|w| format!("{w:?}")).collect();
                    out.push_str(&format!("; {}", warnings.join(", ")));
                }
                out.push('\n');
                for cell in &table.cells {
                    out.push_str(&format!(
                        "  row {}, column {}: {}\n",
                        place(cell.row, cell.row_span),
                        place(cell.column, cell.col_span),
                        flat(&cell.text)
                    ));
                }
            }
        }
    }
    out
}

fn render(options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    let Some(dir) = options.out.as_ref() else {
        return Err("render needs --out DIR".to_string());
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {dir}: {e}"))?;

    let stem = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_string());

    // One page that will not write reports itself and the rest still render,
    // which is `run`'s policy over files applied to pages. A `?` here used to
    // abandon every page after the first failed write, and the pages are
    // independent: a full disk on page 3 is no reason to have nothing for
    // pages 4 onwards.
    let mut failed = 0usize;
    for report in render_walk(options, dir, &stem, doc) {
        match report {
            Ok(lines) => {
                if !options.quiet {
                    print!("{lines}");
                }
            }
            Err(message) => {
                eprintln!("tpdf: {message}");
                failed += 1;
            }
        }
    }

    match failed {
        0 => Ok(()),
        1 => Err("1 page failed".to_string()),
        n => Err(format!("{n} pages failed")),
    }
}

/// Renders the pages `--jobs` at a time and returns what each one has to say,
/// in page order, whether or not it succeeded.
fn render_walk(
    options: &Options,
    dir: &str,
    stem: &str,
    doc: &Document,
) -> Vec<Result<String, String>> {
    walk_to_files(options, dir, stem, doc, false)
        .into_iter()
        .map(|(report, _)| report)
        .collect()
}

/// One page of a render walk: what it has to say, or why it failed, and its
/// render's stats where the walk counted them.
type Walked = (Result<String, String>, Option<tinker_pdf::Stats>);

/// Renders the pages on the facade's page walk (`Document::walk_pages`) with
/// `--jobs` threads of the engine's own pool, each page rendered and written
/// to its PNG on the worker that takes it — which writes its page before it
/// takes the next, so at most `--jobs` bitmaps are in flight — and its lines
/// handed back in page order, whichever worker finished first. With `counted`
/// each page's render counts its work, and its stats say where it ran.
///
/// **Ordered by the walk**, for the reason `xtask`'s corpus pool sorts its
/// own results: output that moves between runs is not diffable, and a
/// `--jobs` that reordered stdout would be a flag that changes the answer,
/// where this one is only allowed to change the clock.
fn walk_to_files(
    options: &Options,
    dir: &str,
    stem: &str,
    doc: &Document,
    counted: bool,
) -> Vec<Walked> {
    let pages = options.pages(doc);
    // The workers ask the file's source before they write, and the file is
    // this thread's.
    let reading = reading_now();
    let render = RenderOptions {
        annotations: options.annotations,
        stats: counted,
        ..RenderOptions::at_dpi(options.dpi)
    };
    let mut walked = Vec::with_capacity(pages.len());
    doc.walk_pages(
        pages,
        Parallelism::Threads(options.jobs.max(1)),
        |index, page| {
            // A page the document will not hand back is skipped silently, as
            // it was serially: `pages()` counts what the catalog claims, and
            // a page tree that stops short is already a warning on the
            // document.
            let bitmap = page?.render(&render);
            let out = format!("{dir}/{stem}-{:04}.png", index + 1);
            let report = sound_of(reading.as_ref())
                .and_then(|()| write_png(&out, &bitmap))
                .map(|()| {
                    let mut lines = format!("{out} {}x{}\n", bitmap.width, bitmap.height);
                    for warning in &bitmap.warnings {
                        lines.push_str(&format!("  {warning:?}\n"));
                    }
                    lines
                });
            Some((report, bitmap.stats))
        },
        |_, page| walked.extend(page),
    );
    walked
}

/// Writes the page as a PNG, through the facade's own
/// [`tinker_pdf::Bitmap::to_png`].
///
/// This used to write a binary PNM under a comment that said "a PNG writer
/// would mean a deflate encoder in a test tool, and the engine already has one
/// it should not depend on from here." The premise was right and the
/// conclusion was the wrong way round: the encoder belongs **in** the engine,
/// beside the zlib compressor, the CRC-32 and the row predictors a PNG is made
/// of — all three of which already lived in `tinker-pdf-filters` and nowhere
/// else — and the tool calls it, which is the direction everything else here
/// already runs in. See `crates/tinker-pdf-filters/src/png/encode.rs`.
///
/// **PNG always, and no flag to ask for anything else.** The case for a
/// `--format` switch is the one consumer in this repository that reads what
/// this writes, `tools/pdfcmp`; the case against is that a debug tool with two
/// output paths has one that is rarely taken and eventually wrong, and that
/// PNM was only ever here because there was no encoder. `pdfcmp` reads what
/// this writes through `Bitmap::from_png`, as well as a `.pnm` from any source
/// and a `.pdf` directly.
///
/// Alpha is **kept** rather than dropped, which the PNM path could not do:
/// `Bitmap::to_png` maps `Rgba8` onto colour type 6 and `Gray8` onto type 0.
/// A page comes back `Rgb8` by default, so the ordinary invocation writes the
/// same pixels it always did and the one that asks for alpha stops throwing it
/// away.
fn write_png(path: &str, bitmap: &tinker_pdf::Bitmap) -> Result<(), String> {
    let bytes = bitmap
        .to_png()
        .ok_or_else(|| format!("writing {path}: the page is not a picture"))?;
    std::fs::write(path, bytes).map_err(|e| format!("writing {path}: {e}"))
}

fn fields(_options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    let found = doc.form_fields();
    sound()?;
    if found.is_empty() {
        println!("{path}: no form fields");
        return Ok(());
    }

    println!("{path}: {} fields", found.len());
    for field in found {
        let flags = [
            field.is_read_only().then_some("read-only"),
            field.is_required().then_some("required"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");

        println!(
            "  {:<28} {:?}{}{}",
            field.name,
            field.kind,
            if flags.is_empty() {
                String::new()
            } else {
                format!(" [{flags}]")
            },
            match field.value {
                tinker_pdf::FieldValue::None => String::new(),
                ref value => format!(" = {:?}", value.as_text()),
            }
        );
    }
    Ok(())
}

/// Lists the fonts a document's pages can reach, and with `--out` writes the
/// embedded programs out.
///
/// The listing half costs names and nothing else — `Document::fonts` reads no
/// stream — which is what makes this cheap to run over a corpus. The
/// extraction half is where the bytes are paid for, once per font, and only
/// when a directory was named.
fn fonts(options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    let found = doc.fonts();
    sound()?;
    if found.is_empty() {
        println!("{path}: no fonts");
        return Ok(());
    }

    println!("{path}: {} fonts", found.len());
    for font in &found {
        let name = match font.name.is_empty() {
            true => "(no /BaseFont)",
            false => font.name.as_str(),
        };
        println!(
            "  {:<32} {:<9} {:<13} {}",
            name,
            format!("{:?}", font.kind),
            match font.program {
                Some(program) => format!("{:?}", program.key),
                None => "not embedded".to_string(),
            },
            match &font.subset_tag {
                Some(tag) => format!("subset {tag} as {}", font.resource_names.join(", ")),
                None => format!("as {}", font.resource_names.join(", ")),
            }
        );
    }

    let Some(directory) = options.out.as_deref() else {
        return Ok(());
    };
    std::fs::create_dir_all(directory).map_err(|e| format!("creating {directory}: {e}"))?;
    let mut written = 0usize;
    for (index, font) in found.iter().enumerate() {
        let Some(bytes) = font.program_bytes() else {
            continue;
        };
        sound()?;
        // Numbered as well as named: two subsets of one face share a name
        // after the tag is stripped, and a run that silently overwrote the
        // first with the second would report a count it did not write.
        let stem = sanitised(&font.base_font);
        let file = format!("{directory}/{index:03}-{stem}.{}", extension(&bytes));
        std::fs::write(&file, &bytes).map_err(|e| format!("writing {file}: {e}"))?;
        println!("  wrote {file} ({} bytes)", bytes.len());
        written += 1;
    }
    println!("{path}: {written} embedded programs written to {directory}");
    Ok(())
}

/// A file name that is safe on every host, from a `/BaseFont` that need not be.
///
/// 7.3.5 lets a name hold any byte but the delimiters, so a `/BaseFont` can
/// carry a slash, a colon or a space. Anything outside the ASCII word
/// characters becomes `_`, which is lossy on purpose: the exact spelling is on
/// the line above, and this is only how the file is found afterwards.
fn sanitised(base_font: &str) -> String {
    let cleaned: String = base_font
        .chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                true => c,
                false => '_',
            },
        )
        .collect();
    match cleaned.is_empty() {
        true => "font".to_string(),
        false => cleaned,
    }
}

/// What to call an extracted program, from the bytes rather than from the key.
///
/// `/FontFile3` holds a bare CFF *or* an OpenType wrapper (9.9 Table 126), and
/// the listing does not read the stream's `/Subtype` — so naming the file from
/// the descriptor key would write an OpenType face called `.cff`. The first
/// four bytes say which it is without opening the question.
fn extension(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x00, 0x01, 0x00, 0x00, ..] | [b't', b'r', b'u', b'e', ..] => "ttf",
        [b'O', b'T', b'T', b'O', ..] => "otf",
        [b't', b't', b'c', b'f', ..] => "ttc",
        [0x80, ..] => "pfb",
        [b'%', b'!', ..] => "pfa",
        [0x01, 0x00, ..] => "cff",
        _ => "bin",
    }
}

fn outline(_options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    let items = doc.outline();
    sound()?;
    if items.is_empty() {
        println!("{path}: no outline");
        return Ok(());
    }
    for item in &items {
        print_outline(item, 0);
    }
    Ok(())
}

fn print_outline(item: &tinker_pdf::OutlineItem, depth: usize) {
    let indent = "  ".repeat(depth);
    println!("{indent}{} -> {:?}", item.title, item.destination);
    for child in &item.children {
        print_outline(child, depth + 1);
    }
}

/// Dumps the object model: what the engine parsed, not what the bytes say.
///
/// The two differ exactly where the interesting bugs are — an object recovered
/// by the repair scanner, a `/Length` that disagreed with `endstream`, a
/// stream whose filter chain half-decoded — and a hex editor cannot show the
/// difference because it only has the bytes.
fn objects(options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    let cos = doc.cos();

    if let Some(number) = options.object {
        return one_object(options, cos, number);
    }

    println!("{path}");
    for line in object_lines(cos) {
        println!("{line}");
    }
    Ok(())
}

/// The summary listing, built rather than printed so it can be asserted on.
fn object_lines(cos: &CosDocument) -> Vec<String> {
    let mut lines = vec!["  trailer".to_string()];
    for (key, value) in cos.trailer().iter() {
        lines.push(format!(
            "    /{} {}",
            name_of(cos, *key),
            inline(cos, value)
        ));
    }

    let highest = cos.max_object_number();
    lines.push(format!("  objects (highest number {highest})"));
    // Over the entries the table has, not over the range it spans. A file
    // numbering one object `i32::MAX` — and the corpus has one — otherwise
    // costs two thousand million lookups to list four objects.
    for (number, entry) in cos.xref().iter() {
        if number == 0 {
            continue;
        }
        let (kind, generation) = match entry {
            XrefEntry::Free { gen, .. } => ("free".to_string(), gen),
            XrefEntry::Offset { offset, gen } => (format!("at {offset}"), gen),
            XrefEntry::InStream { stream_num, idx } => (format!("in {stream_num} #{idx}"), 0),
        };
        if matches!(entry, XrefEntry::Free { .. }) {
            lines.push(format!("    {number:>6} {generation:<5} free"));
            continue;
        }

        let summary = match cos.get(ObjRef::new(number, generation)) {
            Ok(object) => summarize(cos, &object),
            // A slot the table promises and the file does not deliver is the
            // single most common corruption, so it is reported per object
            // rather than rolled into a count.
            Err(error) => format!("unreadable: {error:?}"),
        };
        lines.push(format!(
            "    {number:>6} {generation:<5} {kind:<16} {summary}"
        ));
    }
    lines
}

/// One object in full, and optionally its stream.
fn one_object(options: &Options, cos: &CosDocument, number: u32) -> Result<(), String> {
    let generation = match cos.xref().get(number) {
        Some(XrefEntry::Offset { gen, .. }) => gen,
        Some(XrefEntry::InStream { .. }) | None => 0,
        Some(XrefEntry::Free { .. }) => return Err(format!("object {number} is free")),
    };
    let reference = ObjRef::new(number, generation);
    let object = cos
        .get(reference)
        .map_err(|e| format!("object {number}: {e:?}"))?;

    if options.stream {
        let data = if options.raw {
            cos.stream_raw(reference)
        } else {
            cos.stream_decoded(reference)
        }
        .map_err(|e| format!("object {number} stream: {e:?}"))?;

        // Straight to stdout with no framing, so it can be redirected into a
        // file and compared with what another tool produces.
        use std::io::Write;
        return std::io::stdout()
            .write_all(&data)
            .map_err(|e| format!("writing stream: {e}"));
    }

    println!("{number} {generation} obj");
    let mut text = String::new();
    write_object(cos, &object, 0, &mut text);
    println!("{text}");

    if let Object::Stream(_) = object.as_ref() {
        let raw = cos.stream_raw(reference).map(|d| d.len());
        let decoded = cos.stream_decoded(reference).map(|d| d.len());
        println!(
            "stream: {} raw, {} decoded",
            describe_length(raw),
            describe_length(decoded)
        );
    }
    Ok(())
}

fn describe_length(result: Result<usize, tinker_pdf::CosError>) -> String {
    match result {
        Ok(n) => format!("{n} bytes"),
        Err(error) => format!("unreadable ({error:?})"),
    }
}

fn name_of(cos: &CosDocument, name: tinker_pdf::Name) -> String {
    cos.name_bytes(name)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|| "<unknown>".to_string())
}

/// One line saying what an object is, for the summary listing.
fn summarize(cos: &CosDocument, object: &Object) -> String {
    match object {
        Object::Dict(dict) | Object::Stream(StreamObj { dict, .. }) => {
            let kind = if matches!(object, Object::Stream(_)) {
                "stream"
            } else {
                "dict"
            };
            let mut label = String::from(kind);
            for key in ["Type", "Subtype"] {
                if let Some(Object::Name(name)) = dict.get(cos.intern(key.as_bytes())) {
                    label.push_str(&format!(" /{}", name_of(cos, *name)));
                }
            }
            format!("{label} ({} keys)", dict.len())
        }
        Object::Array(items) => format!("array ({} items)", items.len()),
        other => inline(cos, other),
    }
}

/// A short single-line rendering, for values inside a summary.
fn inline(cos: &CosDocument, object: &Object) -> String {
    let mut text = String::new();
    write_object(cos, object, 0, &mut text);
    if text.len() > 96 {
        text.truncate(93);
        text.push_str("...");
    }
    text.replace('\n', " ")
}

/// Writes an object the way the file would spell it.
///
/// Hand-rolled here rather than borrowed from the writer: this is a debugging
/// view and wants indentation the writer would never emit, and a CLI that
/// shared the writer's formatting would start constraining it.
fn write_object(cos: &CosDocument, object: &Object, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth + 1);
    match object {
        Object::Null => out.push_str("null"),
        Object::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Object::Int(value) => out.push_str(&value.to_string()),
        Object::Real(value) => out.push_str(&format!("{value}")),
        Object::Name(name) => {
            out.push('/');
            out.push_str(&name_of(cos, *name));
        }
        Object::Ref(reference) => {
            out.push_str(&format!("{} {} R", reference.num, reference.gen));
        }
        Object::String(string) => out.push_str(&show_string(string)),
        Object::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                write_object(cos, item, depth, out);
            }
            out.push(']');
        }
        Object::Dict(dict) | Object::Stream(StreamObj { dict, .. }) => {
            out.push_str("<<\n");
            for (key, value) in dict.iter() {
                out.push_str(&pad);
                out.push('/');
                out.push_str(&name_of(cos, *key));
                out.push(' ');
                write_object(cos, value, depth + 1, out);
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
            out.push_str(">>");
        }
    }
}

/// Strings as they were spelled where that is readable, and as hex where it
/// is not — a UTF-16 title printed as mojibake is worse than useless.
fn show_string(string: &tinker_pdf::PdfString) -> String {
    let printable = string
        .bytes
        .iter()
        .all(|&b| (0x20..0x7F).contains(&b) || b == b'\n' || b == b'\t');

    if printable && !string.hex {
        let escaped = string
            .bytes
            .iter()
            .map(|&b| match b {
                b'(' => "\\(".to_string(),
                b')' => "\\)".to_string(),
                b'\\' => "\\\\".to_string(),
                b'\n' => "\\n".to_string(),
                b'\t' => "\\t".to_string(),
                other => char::from(other).to_string(),
            })
            .collect::<String>();
        return format!("({escaped})");
    }

    let mut out = String::from("<");
    for byte in &string.bytes {
        out.push_str(&format!("{byte:02X}"));
    }
    out.push('>');
    out
}

/// Opens every file and reports what happened, without rendering.
///
/// This is the corpus pass: it exits non-zero only when a file could not be
/// opened at all, because a file that opens with warnings is the case the
/// leniency ladder exists to handle rather than a failure.
/// How `check` reads its files: whole for a verdict flag, whose passes read
/// the file's bytes whole, and on demand otherwise.
fn check_reads(options: &Options) -> Reads {
    if options.strict || options.pdfa || options.pdfua || options.pdfx {
        Reads::Whole
    } else {
        Reads::OnDemand
    }
}

/// The line `check` prints for a file that opened, and the warnings it
/// counts, taken before the pages are.
fn checked(path: &str, doc: &Document) -> (String, Vec<tinker_pdf::Warning>) {
    let warnings = doc.warnings();
    let pages = doc.page_count();
    let line = format!(
        "ok    {path}  {pages} pages, {:?}, {} warnings",
        doc.ladder_level(),
        warnings.len()
    );
    (line, warnings)
}

fn check(options: &Options) -> Result<(), String> {
    let fonts = options.font_provider()?;
    let mut failed = 0usize;
    let mut warned = 0usize;
    let mut invalid = 0usize;
    let mut nonconforming = 0usize;
    let mut unclaimed = 0usize;
    let mut inaccessible = 0usize;
    let mut ua_unclaimed = 0usize;
    let mut unprintable = 0usize;
    let mut x_unclaimed = 0usize;

    let reads = check_reads(options);
    for path in &options.files {
        match open(path, reads, options.password.as_deref(), fonts.as_ref()) {
            Ok((doc, kept)) => {
                let (line, warnings) = checked(path, &doc);
                if let Err(message) = sound_of(kept.map(|s| (path.clone(), s)).as_ref()) {
                    failed += 1;
                    println!("FAIL  {message}");
                    continue;
                }
                if !warnings.is_empty() {
                    warned += 1;
                }
                // Ruling 13's verdict, and the reason `--strict` exits by it:
                // "it opened" and "it is right" are different questions, and
                // the corpus pass answers the first one already.
                let defects = if options.strict {
                    doc.validate()
                } else {
                    Vec::new()
                };
                if !defects.is_empty() {
                    invalid += 1;
                }
                if !options.quiet {
                    println!("{line}");
                }
                for defect in &defects {
                    println!("      {} {defect}", defect.kind.tier().as_str());
                }

                if options.pdfa {
                    let verdict = doc.validate_pdfa();
                    match verdict.flavour {
                        Some(flavour) => {
                            if !verdict.found_nothing() {
                                nonconforming += 1;
                            }
                            if !options.quiet {
                                println!("      pdfa claims {flavour}");
                            }
                        }
                        // Not a failure. Most PDFs are not PDF/A and are not
                        // pretending to be, so a tool that exited non-zero on
                        // them would be unusable over any real corpus.
                        None => {
                            unclaimed += 1;
                            if !options.quiet {
                                println!("      pdfa claims nothing");
                            }
                        }
                    }
                    for finding in &verdict.findings {
                        println!("      pdfa {finding}");
                    }
                    // Which groups ran, always, even when nothing was found.
                    // "No findings" from a partial sweep is not "it conforms",
                    // and the only way a caller can tell the two apart is if
                    // the coverage is printed beside the verdict rather than
                    // documented somewhere else.
                    if !options.quiet {
                        println!("      pdfa ran {}", verdict.coverage);
                    }
                }

                if options.pdfua {
                    let verdict = doc.validate_pdfua();
                    match verdict.part {
                        Some(part) => {
                            if !verdict.found_nothing() {
                                inaccessible += 1;
                            }
                            if !options.quiet {
                                println!("      pdfua claims {part}");
                            }
                        }
                        // Not a failure, for the reason a missing PDF/A claim
                        // is not: most PDFs claim neither.
                        None => {
                            ua_unclaimed += 1;
                            if !options.quiet {
                                println!("      pdfua claims nothing");
                            }
                        }
                    }
                    for finding in &verdict.findings {
                        println!("      pdfua {finding}");
                    }
                    // The groups that ran and, beside them, how much was not
                    // decided — as counts with their words, never as a rate,
                    // because an abstention is not a pass.
                    if !options.quiet {
                        let staged = verdict
                            .abstained
                            .iter()
                            .filter(|a| a.class == tinker_pdf::PdfUaAbstentionClass::Staged)
                            .count();
                        let undecidable = verdict.abstained.len() - staged;
                        println!(
                            "      pdfua ran {}; abstained on {staged} staged and \
                             {undecidable} undecidable clauses",
                            verdict.coverage
                        );
                    }
                }

                if options.pdfx {
                    let verdict = doc.validate_pdfx();
                    match (&verdict.claim, verdict.flavour) {
                        (Some(_), Some(flavour)) => {
                            if !verdict.found_nothing() {
                                unprintable += 1;
                            }
                            if !options.quiet {
                                let validated = if flavour.is_validated() {
                                    ""
                                } else {
                                    " (not validated by this build)"
                                };
                                println!("      pdfx claims {flavour}{validated}");
                            }
                        }
                        (Some(claim), None) => {
                            if !options.quiet {
                                println!(
                                    "      pdfx claims {:?}, a level this build does not identify",
                                    claim.version
                                );
                            }
                        }
                        // Not a failure, for the reason a missing PDF/A claim
                        // is not.
                        (None, _) => {
                            x_unclaimed += 1;
                            if !options.quiet {
                                println!("      pdfx claims nothing");
                            }
                        }
                    }
                    for finding in &verdict.findings {
                        println!("      pdfx {finding}");
                    }
                    if !options.quiet && verdict.claim.is_some() {
                        let staged = verdict
                            .abstained
                            .iter()
                            .filter(|a| a.class == tinker_pdf::PdfXAbstentionClass::Staged)
                            .count();
                        let unread = verdict.abstained.len() - staged;
                        println!(
                            "      pdfx ran {}; abstained on {staged} staged and \
                             {unread} unread clauses",
                            verdict.coverage
                        );
                    }
                }
            }
            Err(message) => {
                failed += 1;
                println!("FAIL  {message}");
            }
        }
    }

    println!(
        "{} files, {} failed, {} with warnings",
        options.files.len(),
        failed,
        warned
    );
    if options.strict {
        println!("{invalid} with defects");
    }
    if options.pdfa {
        println!("{nonconforming} with conformance findings, {unclaimed} claiming no flavour");
    }
    if options.pdfua {
        println!("{inaccessible} with PDF/UA findings, {ua_unclaimed} claiming no PDF/UA part");
    }
    if options.pdfx {
        println!("{unprintable} with PDF/X findings, {x_unclaimed} claiming no PDF/X level");
    }
    if failed > 0 {
        return Err(format!("{failed} files could not be opened"));
    }
    if invalid > 0 {
        return Err(format!("{invalid} files did not validate"));
    }
    if nonconforming > 0 {
        return Err(format!(
            "{nonconforming} files did not conform to the flavour they claim"
        ));
    }
    if inaccessible > 0 {
        return Err(format!(
            "{inaccessible} files broke a PDF/UA clause of the part they claim"
        ));
    }
    if unprintable > 0 {
        return Err(format!(
            "{unprintable} files broke a PDF/X requirement of the level they claim"
        ));
    }
    Ok(())
}

// ---- metamorphic probes (ruling 13, roadmap step 7) -----------------------

/// How far a render at two resolutions may disagree, as a share of the pixels.
///
/// **Measured, not chosen.** Over the first 119 files of the pdf.js corpus at
/// 72 dpi this relation was *exact* on 118 and over 12% on one. Two percent is
/// therefore not a threshold separating a population — there is no population
/// between zero and twelve — it is a line drawn where nothing sits, so a file
/// that crosses it has done something other than resample an edge.
///
/// What the ratchet records is the count either side of the line, so moving
/// this number is a diff somebody reviews rather than a quiet re-baselining.
const DPI_BUDGET: f64 = 0.02;

/// The same, for a quarter-turn.
///
/// **Rotation is not exact and measuring says why.** Turning the page puts
/// every glyph on a different sampling grid, so the outline that covered 40% of
/// a pixel now covers 60% of its neighbour — the picture is the same and the
/// bytes are not. Over the same 119 files: exact on 83, under a tenth of a
/// percent on 90 of them, and 0.29% at the ninetieth percentile, with one file
/// at 14%.
///
/// One percent sat above that noise and far below anything structural: a
/// rotation applied to the geometry and not to the clip, or to the text and not
/// to the images, moves whole regions rather than the rims of glyphs.
///
/// **Raised to two percent in August 2026, and the reason is that the noise it
/// was measured against has changed.** Every figure above was taken when an
/// image edge was quantised to whole device pixels: images alone did not
/// anti-alias, so they alone transposed exactly, and the 0.29% was glyph rims
/// and nothing else. Image edges are soft now
/// (`docs/design/image-edges.md`), and a soft edge at a fractional offset does
/// not transpose to the byte any more than a glyph's does — on long straight
/// edges there is simply more of it. Two qpdf files sit at 1.7%: hundreds of
/// separately placed, quarter-turned scans, measured at exactly 1.0% with hard
/// edges and 1.7% with soft ones, which is the whole of the difference.
///
/// Two percent keeps the structural distance the original figure was chosen
/// for — a misapplied rotation moves whole regions, tens of percent, and the
/// one file at 14% is still caught — and it is the same figure `DPI_BUDGET`
/// already carries for the same class of reason.
///
/// **Raised again to ten percent on 5 September 2026, and the reason is that
/// the class was finally measured on a page rather than on a thumbnail.**
///
/// `crates/tinker-pdf/tests/metamorphic_classes.rs` puts seven constructs,
/// each alone on a page, through this relation, and two of them move anything:
/// a tiling pattern, which is a rounding of the lattice and a defect this
/// budget must go on catching, and **anti-aliased edges that are not
/// axis-aligned**, which is arithmetic. A quarter turn puts every mark on a
/// transposed sampling grid; a rectangle on integers transposes exactly and a
/// diagonal cannot.
///
/// The first figures for the second class were 2.81% for diagonals and 2.64%
/// for text, and **both were fixture artefacts**: the pages are 64 points
/// square with the construct in a corner, and a budget is a *share of a page*.
/// Measured on pages the construct actually covers, and at sizes a document
/// has:
///
/// | Page | `rotate` |
/// | --- | ---: |
/// | two triangles in a corner of 64 pt | 2.81% |
/// | the same two triangles on 595 pt | 0.03% |
/// | 11-point text filling 200 pt | 7.74% |
/// | 11-point text filling 595 pt | **8.77%** |
/// | 11-point text filling 842 pt | **8.74%** |
/// | diagonal edges covering 595 pt | 23.52% |
/// | a tiling pattern off the grid, 64 pt | 22.05% |
///
/// **A page of text costs 8.8% and the figure is stable across page sizes** —
/// which the corner fixture could not show, because its cost fell with the
/// page while a real document's text does not. So two percent was not
/// slightly tight, it was out by a factor of four, and every text-heavy
/// document in the corpus was failing this relation for arithmetic. That is
/// the one thing a budget exists to prevent.
///
/// Ten percent is above the measured class and below the measured defects: the
/// tiling class is 22%, and a rotation applied to the geometry and not to the
/// clip moves whole regions. Fifteen corpus files still break it, from 10.9%
/// to 75.4%.
///
/// **What this raise does not have, and the last one did: a gap to sit in.**
/// One percent was sited where nothing sat — over 119 files the relation was
/// exact on 83 and the worst was 14%. A whole-corpus run on 5 September, with
/// every hold now carrying its own measurement, says that region is gone: of
/// 4 032 files that hold, 109 hold above 1% and 47 above 1.5%, and of the 181
/// that broke at two percent the smallest was 2.027% with no gap up to 4%. The
/// ablated constructs still leave one — 8.8% for text against 22% for the
/// pattern class — and that is the gap this figure sits in.
///
/// **And the cost is paid rather than waved away.** 166 files stop being
/// reported by this relation, 107 of them between three and ten percent. That
/// is why `ROTATE_WATCH` exists: the same measurement is judged a second time
/// at three percent and recorded as its own relation, so the population that
/// the wider budget stops failing is still counted and still ratcheted.
const ROTATE_BUDGET: f64 = 0.10;

/// The line the same measurement is *watched* against, which decides nothing.
///
/// **A budget wide enough to admit a page of text is too wide to see much,
/// and this is what stops that being a loss.** At ten percent the relation
/// breaks on 15 corpus files where two percent broke on 181, so 166 files
/// stop being reported — and 107 of those sit between three and ten percent,
/// which is a population worth knowing the size of even though no single one
/// of them is a defect.
///
/// So the same `moved of total` is judged twice and reported under two names.
/// `rotate` carries the budget and can fail a run; `rotate-tight` carries this
/// and cannot, because the runner compares every relation it is given by name
/// and the ratchet holds each one's count. A change that makes glyph edges
/// noisier moves `rotate-tight` long before it moves `rotate`, and a change
/// that turns the geometry without the clip moves both.
///
/// Three percent rather than the old two: two was below the constructs this
/// engine's own ablation measures — 2.81% for a page of diagonals on the
/// smallest page — so a watch line there would spend its life above the noise
/// it is watching.
const ROTATE_WATCH: f64 = 0.03;

/// What the watched judgement is called in the record.
const ROTATE_WATCH_NAME: &str = "rotate-tight";

/// A share of a page's pixels, for a relation's report.
#[allow(
    clippy::cast_precision_loss,
    reason = "a share of a page's pixels, reported to one figure"
)]
fn share(moved: u64, total: u64) -> f64 {
    moved as f64 / total.max(1) as f64
}

/// How far one channel may move before a pixel counts as different.
///
/// Eight levels of 255. Resampling moves an edge pixel by a lot and a flat area
/// by nothing, so a smaller threshold measures the anti-aliasing and a larger
/// one measures nothing.
const CHANNEL_TOLERANCE: i32 = 8;

/// One relation's *measurement*, before a budget is applied to it.
///
/// Separated from [`Relation`] because one measurement now answers two
/// questions. `rotate` is reported twice — once against the budget that
/// decides pass or fail, and once against a tighter line that decides nothing
/// and is watched — and rendering the page a second time to ask the second
/// question would be a waste and a lie: two renders can differ.
enum Measured {
    /// `moved` pixels of `total` were not the transposition.
    Moved(u64, u64),
    /// The comparison could not be made at all — a turn that did not
    /// transpose, a rewrite that would not reopen.
    Broke(String),
    /// It was not asked.
    Skipped(&'static str),
}

impl Measured {
    /// The verdict a budget draws from this measurement.
    fn judged(&self, budget: f64) -> Relation {
        match self {
            Measured::Moved(moved, total) => {
                if share(*moved, *total) <= budget {
                    Relation::Held(*moved, *total)
                } else {
                    Relation::Broke(format!(
                        "{moved} of {total} pixels ({:.1}%) are not the transposition, over a budget of {:.1}%",
                        share(*moved, *total) * 100.0,
                        budget * 100.0
                    ))
                }
            }
            Measured::Broke(detail) => Relation::Broke(detail.clone()),
            Measured::Skipped(why) => Relation::Skipped(why),
        }
    }
}

/// One relation's verdict, in the record's own words.
enum Relation {
    /// The relation held, and this is how far it was from not holding:
    /// the pixels that moved, of the pixels compared.
    ///
    /// **A hold carries its measurement because the budgets are sited from
    /// the population and not from a principle.** `ROTATE_BUDGET`'s own
    /// comment sites two percent against a distribution — exact on 83 files
    /// of 119, 0.29 % at the ninetieth percentile — and that distribution
    /// came from a run somebody instrumented by hand, because a record that
    /// says only `held` throws the ninety-eight percent away and keeps the
    /// tail. Re-siting a budget then has nothing to re-site against. Two
    /// files that both held, one at 0 % and one at 1.9 %, are not the same
    /// observation and the record now says so.
    Held(u64, u64),
    /// It did not, and this is what was measured.
    Broke(String),
    /// It could not be asked — an empty page, a page too large to render
    /// twice, a rewrite that would not reopen.
    Skipped(&'static str),
}

impl Relation {
    fn print(&self, out: &mut dyn std::io::Write, name: &str) {
        match self {
            Relation::Held(moved, total) => say!(out, "meta {name} held {moved} of {total}"),
            Relation::Broke(detail) => say!(out, "meta {name} broke {}", one_line(detail)),
            Relation::Skipped(why) => say!(out, "meta {name} skipped {why}"),
        }
    }
}

/// The three relations, checked in this process.
///
/// **In-process on purpose**: the design says no image leaves the child, and it
/// is not squeamishness. A relation checked by writing two bitmaps and diffing
/// them elsewhere would need somewhere to write four thousand pairs of them,
/// and would make the corpus run depend on a comparator outside the run.
///
/// Only the first page. Every relation costs at least one extra render and the
/// rotation and crop ones cost a save and a reopen as well, so asking every
/// page of a four-thousand-file corpus would turn the timeout into the thing
/// being measured. Asking the first page of every file costs 95 seconds over
/// 4 525 of them, which is what made deleting the budget gate affordable.
///
/// **There used to be a clock here, and deleting it is what this comment is
/// for.** `META_BUDGET_MS` declined the relations for any file that had
/// already spent 3 100 ms opening and rendering, on the reasoning that the
/// extra work risked the runner's twenty-second timeout and a timeout would
/// move the *pass* rate. The cost of that was stated in the same comment and
/// then paid every night: `compared` is the denominator of a ratcheted rate,
/// so a file near the line was asked on one run and declined on the next, and
/// the nightly corpus job failed for a week on ten regressions that were all
/// denominators moving under load rather than the engine changing.
///
/// Nothing deterministic could replace it, and that was measured rather than
/// assumed — `qpdf/numeric-and-string-2.pdf` is 16 KB with 22 objects and was
/// declined at 6.3 s, while its sibling `numeric-and-string-1.pdf`, 18 KB and
/// 15 objects, was admitted at 8.9 s. Cost does not predict time here.
///
/// So the gate is gone and the timeout is sixty seconds instead, which the old
/// comment considered and rejected because *"a longer timeout would change what
/// the pass rate means"*. It does, and the change was measured before it was
/// taken (4 525 files, 72 dpi, 4-5 September 2026): the whole corpus runs in
/// **95 seconds**, nothing times out, and every one of the twelve
/// corpus-and-relation counts goes **up or stays equal** — pdf.js `rotate` 838
/// compared to 840, `dpi` 944 to 948, veraPDF's ten-thousand-page
/// implementation-limit fixture finishing for the first time. The pass rate did
/// not fall; it rose by one.
///
/// What is left declining a relation is a property of the document — no pages,
/// encrypted, opened with a warning, a page too large to render twice — so
/// `compared` is a function of the corpus and not of the machine.
fn metamorphic(
    out: &mut dyn std::io::Write,
    marks: &mut Marks,
    doc: &Document,
    options: &Options,
    fonts: Option<&Arc<SimpleFontProvider>>,
) {
    if doc.page_count() == 0 {
        for name in ["rotate", ROTATE_WATCH_NAME, "crop", "dpi"] {
            Relation::Skipped("the document has no pages").print(out, name);
        }
        return;
    }

    let render = RenderOptions {
        annotations: options.annotations,
        ..RenderOptions::at_dpi(options.dpi)
    };
    let Some(page) = doc.page(0) else {
        return;
    };
    let base = page.render(&render);
    // Each part of the phase marks its high-water mark, so a peak reached in
    // it says which part reached it: the render the relations compare with,
    // each relation's rewrite, reopen and render, and the doubled render. Each
    // starts from live memory alone ([`release_freed`]), so a part's peak is
    // its own memory and not what the parts before it left the allocator.
    marks.mark(out, "meta-base");
    // A page that came back empty is a page the relations cannot speak about:
    // every one of them holds trivially over nothing.
    if base.width == 0 || base.height == 0 {
        for name in ["rotate", ROTATE_WATCH_NAME, "crop", "dpi"] {
            Relation::Skipped("the page rendered to nothing").print(out, name);
        }
        return;
    }

    // **The two relations that rewrite are asked only of a document this
    // engine read cleanly**, which is the strict pass's own eligibility and is
    // borrowed rather than reinvented. The reason is the relation's, not the
    // clock's: rotating or cropping a document means saving it and reading it
    // back, so over a file the reader had to *repair* the comparison is
    // between two repairs and not between two renders.
    //
    // It is also what stops a pathological file from eating the corpus
    // runner's budget. `pdfjs/test/pdfs/bug1980958.pdf` is 219 bytes, opens
    // through the rescan ladder with a synthesised root, renders its 10x10
    // page in under two seconds — and its rewrite does not come back at all:
    // three minutes in, `rotate` had not returned. Asking a relation of a
    // document whose structure the reader invented was the mistake; the
    // timeout was the symptom.
    match cleanly_read(doc) {
        None => {
            // A phase line before each: both rewrite and reopen the document,
            // which is several seconds of silence on a large file — and is
            // where a rewrite that does not terminate stops.
            say!(out, "phase meta-rotate");
            let turned = rotation(out, marks, doc, &base, &render, fonts);
            turned.judged(ROTATE_BUDGET).print(out, "rotate");
            turned.judged(ROTATE_WATCH).print(out, ROTATE_WATCH_NAME);
            say!(out, "phase meta-crop");
            cropping(out, marks, doc, &page, &base, &render, fonts).print(out, "crop");
        }
        Some(why) => {
            Relation::Skipped(why).print(out, "rotate");
            Relation::Skipped(why).print(out, ROTATE_WATCH_NAME);
            Relation::Skipped(why).print(out, "crop");
        }
    }
    // `dpi` rewrites nothing, so it is asked of every document that rendered.
    say!(out, "phase meta-dpi");
    resolution(out, marks, &page, &base, &render).print(out, "dpi");
}

/// One pixel of a bitmap, as three channels.
fn channels(bitmap: &Bitmap, x: u32, y: u32) -> (i32, i32, i32) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]))
}

fn differs(a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
    (a.0 - b.0).abs() > CHANNEL_TOLERANCE
        || (a.1 - b.1).abs() > CHANNEL_TOLERANCE
        || (a.2 - b.2).abs() > CHANNEL_TOLERANCE
}

/// **A page rotated a quarter-turn is the same page transposed.**
///
/// `/Rotate 90` turns the page clockwise, so the pixel at `(x, y)` of the
/// rotated render is the pixel at `(x, height - 1 - y)` of the original with
/// its axes swapped. Nothing about the content changes, so this is an equality
/// and not a budget — and it catches every place a rotation is applied to the
/// geometry and not to the clip, or to the text and not to the images.
fn rotation(
    out: &mut dyn std::io::Write,
    marks: &mut Marks,
    doc: &Document,
    base: &Bitmap,
    render: &RenderOptions,
    fonts: Option<&Arc<SimpleFontProvider>>,
) -> Measured {
    let mut editor = doc.editor();
    if !editor.rotate_page(0, 90) {
        return Measured::Skipped("the page would not rotate");
    }
    let rewritten = editor.save(&WriteOptions::default());
    drop(editor);
    marks.mark(out, "meta-rotate-rewrite");
    let Ok(turned) = Document::open(rewritten) else {
        return Measured::Skipped("the rotated document would not reopen");
    };
    marks.mark(out, "meta-rotate-reopen");
    // **With the same faces**, and this is not a detail. The relation compares
    // two renders of one document, so the two must be rendered under the same
    // conditions; reopening without the provider gave the rotated copy no way
    // to draw text that the original drew. Measured: 309 of pdf.js's 839 files
    // failed `rotate` under `--fonts` and 219 without it, and the difference
    // was entirely this line.
    let turned = match fonts {
        Some(provider) => turned.with_fonts(provider.clone()),
        None => turned,
    };
    let Some(page) = turned.page(0) else {
        return Measured::Skipped("the rotated document lost its page");
    };
    let rotated = page.render(render);
    marks.mark(out, "meta-rotate-render");

    if rotated.width != base.height || rotated.height != base.width {
        return Measured::Broke(format!(
            "{}x{} turned is {}x{} and not {}x{}",
            base.width, base.height, rotated.width, rotated.height, base.height, base.width
        ));
    }
    let mut moved = 0u64;
    for y in 0..rotated.height {
        for x in 0..rotated.width {
            let source = channels(base, y, base.height - 1 - x);
            if differs(channels(&rotated, x, y), source) {
                moved += 1;
            }
        }
    }
    Measured::Moved(moved, u64::from(rotated.width) * u64::from(rotated.height))
}

/// **A cropped render is the sub-rectangle of the full one** (ruling 5's tile
/// equality, generalised from a tile to the page box).
///
/// The crop box is the middle half of the page, in whole pixels at the scale
/// being rendered, so the comparison needs no resampling: every pixel of the
/// cropped render has a pixel of the full one it must equal exactly.
fn cropping(
    out: &mut dyn std::io::Write,
    marks: &mut Marks,
    doc: &Document,
    page: &Page,
    base: &Bitmap,
    render: &RenderOptions,
    fonts: Option<&Arc<SimpleFontProvider>>,
) -> Relation {
    let (x0, y0, x1, y1) = page.crop_box();
    let (width, height) = (x1 - x0, y1 - y0);
    if !(width.is_finite() && height.is_finite()) || width < 4.0 || height < 4.0 {
        return Relation::Skipped("the page is too small to crop");
    }
    if page.rotation() != 0 {
        // A rotated page's crop box and its bitmap do not share an axis, and
        // the relation would be about this test's arithmetic rather than about
        // the engine. `rotate` already covers the turning.
        return Relation::Skipped("the page is already rotated");
    }

    // A quarter in from each edge, snapped to whole pixels at this scale so the
    // sub-rectangle lands on pixel boundaries.
    let scale = render.scale;
    let inset_x = ((width / 4.0) * scale).floor() / scale;
    let inset_y = ((height / 4.0) * scale).floor() / scale;
    if inset_x <= 0.0 || inset_y <= 0.0 {
        return Relation::Skipped("the page is too small to crop");
    }

    // From live memory alone: the rotated copy, its rewrite and its render
    // were freed as `rotation` returned, after its last mark, and what they
    // freed goes back before this rewrite starts, as before the doubled render
    // ([`doubled`]).
    release_freed();
    let mut editor = doc.editor();
    if !editor.set_crop_box(0, x0 + inset_x, y0 + inset_y, x1 - inset_x, y1 - inset_y) {
        return Relation::Skipped("the crop box would not be set");
    }
    let rewritten = editor.save(&WriteOptions::default());
    drop(editor);
    marks.mark(out, "meta-crop-rewrite");
    let Ok(cropped) = Document::open(rewritten) else {
        return Relation::Skipped("the cropped document would not reopen");
    };
    marks.mark(out, "meta-crop-reopen");
    // With the same faces, for `rotation`'s reason.
    let cropped = match fonts {
        Some(provider) => cropped.with_fonts(provider.clone()),
        None => cropped,
    };
    let Some(page) = cropped.page(0) else {
        return Relation::Skipped("the cropped document lost its page");
    };
    let small = page.render(render);
    marks.mark(out, "meta-crop-render");
    if small.width == 0 || small.height == 0 {
        return Relation::Skipped("the cropped page rendered to nothing");
    }

    // Where the cropped rectangle starts in the full render. `y` counts down a
    // bitmap and up a page, so the top of the crop is the *far* inset.
    let left = (inset_x * scale).round() as u32;
    let top = (inset_y * scale).round() as u32;
    if left + small.width > base.width || top + small.height > base.height {
        return Relation::Broke(format!(
            "a {}x{} crop at ({left}, {top}) does not fit a {}x{} page",
            small.width, small.height, base.width, base.height
        ));
    }

    let mut moved = 0u64;
    for y in 0..small.height {
        for x in 0..small.width {
            if differs(channels(&small, x, y), channels(base, left + x, top + y)) {
                moved += 1;
            }
        }
    }
    // **No budget, and that is a measurement rather than an oversight.** A crop
    // moves the page box and nothing else, so every pixel of the cropped render
    // has a pixel of the full one on the same sampling grid: over the first 119
    // files of the pdf.js corpus this relation was exact on all 119. Rotation
    // and resolution both change the grid and both need a budget; this does
    // not, and giving it one would hide the only kind of defect it can see.
    let total = u64::from(small.width) * u64::from(small.height);
    if moved == 0 {
        Relation::Held(0, total)
    } else {
        Relation::Broke(format!(
            "{moved} of {total} pixels of the crop are not the page under it"
        ))
    }
}

/// **A render at twice the resolution, halved, is the render at one.**
///
/// Within a budget, and this is the only one of the three that needs one:
/// resampling and anti-aliasing are not the same operation, so an edge pixel
/// legitimately lands somewhere between the two. What the relation catches is
/// a *grid* mistake — a half-pixel offset, an off-by-one in the page-to-device
/// transform, a rounding that only shows at one scale — and those move whole
/// regions rather than edges.
fn resolution(
    out: &mut dyn std::io::Write,
    marks: &mut Marks,
    page: &Page,
    base: &Bitmap,
    render: &RenderOptions,
) -> Relation {
    // A page big enough that rendering it twice over is the corpus runner's
    // whole budget is one this relation declines rather than times out on.
    if u64::from(base.width) * u64::from(base.height) > 4_000_000 {
        marks.mark(out, "meta-dpi");
        return Relation::Skipped("the page is too large to render twice");
    }
    let doubled = doubled(page, render);
    // Marked while the doubled render is held, so the reading holds its
    // canvas as resident rather than as the kernel's kept figure for memory
    // already unmapped, which is the one [`Marks`] says can fall short.
    marks.mark(out, "meta-dpi");
    // **The two renders round their own sizes outward, independently**, so the
    // doubled one can be a pixel short of twice the other: a page 1275.2 pixels
    // wide is 1276 at one scale and 2551 at two, and 2551 is not 2552. The
    // comparison runs over the region both grids cover rather than declining
    // the file — declining it was the first draft, and it declined every page
    // whose width was not a whole number of pixels.
    let across = base.width.min(doubled.width / 2);
    let down = base.height.min(doubled.height / 2);
    if across == 0 || down == 0 {
        return Relation::Skipped("the doubled render was scaled down");
    }

    let mut moved = 0u64;
    let total = u64::from(across) * u64::from(down);
    for y in 0..down {
        for x in 0..across {
            // The box filter: the four pixels of the doubled render that make
            // up this one.
            let mut sum = (0i32, 0i32, 0i32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let p = channels(&doubled, x * 2 + dx, y * 2 + dy);
                    sum = (sum.0 + p.0, sum.1 + p.1, sum.2 + p.2);
                }
            }
            let filtered = (sum.0 / 4, sum.1 / 4, sum.2 / 4);
            if differs(filtered, channels(base, x, y)) {
                moved += 1;
            }
        }
    }
    if share(moved, total) <= DPI_BUDGET {
        Relation::Held(moved, total)
    } else {
        Relation::Broke(format!(
            "{moved} of {total} pixels ({:.1}%) differ, over a budget of {:.1}%",
            share(moved, total) * 100.0,
            DPI_BUDGET * 100.0
        ))
    }
}

/// The render at twice the scale that [`resolution`] compares, made from live
/// memory alone (MS-27).
///
/// What the relations before it freed goes back to the kernel first
/// ([`release_freed`]). Kept by the allocator, it decided the doubled render's
/// peak in their place: the render reused it where a freed block was large
/// enough for its canvas and added its canvas beside it where none was, so
/// the peak moved from run to run of one document with what the allocator
/// held. From live memory, the peak is the render's own canvas and work over
/// the document, the page and the render it is compared with.
fn doubled(page: &Page, render: &RenderOptions) -> Bitmap {
    release_freed();
    page.render(&RenderOptions {
        scale: render.scale * 2.0,
        ..render.clone()
    })
}

// ---- probe: the record one corpus child writes ---------------------------

/// The record's format version, bumped when a reader would misread the old
/// shape. The runner refuses a record whose version it does not know rather
/// than reading the fields it recognises and inventing the rest.
///
/// Version 6 adds `peak`: the child's own peak resident set. It is a bump
/// rather than a quiet new key because the runner *requires* the measurement —
/// a record without it makes the run incomplete — so a runner that read an
/// older child's record would call every corpus unmeasurable rather than
/// naming the stale binary.
///
/// Version 7 adds `time`: `time open` after `pages`, the span over opening the
/// document and counting its pages, and `time page <n> <micros>` after each
/// page's render, n counted from 1 as the progress line counts. Both are whole
/// microseconds, and both leave out the strict and metamorphic passes, which
/// `ms` still spans. A bump for `peak`'s reason: the spans are what a corpus's
/// time figures are made from, so a runner reading an older child's record as
/// this version would see files that timed nothing rather than a binary nobody
/// rebuilt.
const PROBE_VERSION: u32 = 7;

/// The `--fonts` value meaning "whatever faces this build carries".
const BUNDLED: &str = "bundled";

/// Whether this build carries the twelve Liberation faces.
///
/// Written into the record so a run cannot claim to have measured faces a
/// child did not have. It is a `cfg`, so it is the compiler's answer rather
/// than a flag anybody can pass.
const BUNDLED_FACES: bool = cfg!(feature = "bundled-fonts");

/// This process's peak resident set in bytes, or `None` where the platform
/// will not say.
///
/// **Measured in the child rather than by a watcher over it**, which is the
/// whole reason this is here and not in `xtask`. A parent that samples a
/// child's memory sees whatever the scheduler let it see: a page allocated and
/// released between two samples is invisible, and the figure a run records
/// depends on how busy the machine was — which is the same defect as a timing
/// assertion, in a number that is supposed to outlive the machine. Both
/// branches below read a **high-water mark the kernel maintains**, so the
/// answer is the same however often anybody asks for it. On Linux the record
/// states it no lower than the largest of the probe's own readings, for the
/// reason [`Marks`] gives.
///
/// Where neither branch applies the caller omits the line rather than printing
/// a zero. The runner turns that omission into a `limits` entry, so a run that
/// could not measure says so and is refused as a bar; a silent zero would be a
/// ceiling nothing could ever exceed.
fn peak_bytes() -> Option<u64> {
    // Linux: a file read, and no FFI at all. `VmHWM` is the peak resident set
    // size the kernel has tracked since exec (`fs/proc/task_mmu.c`), written
    // in what the file calls `kB` and means KiB.
    #[cfg(target_os = "linux")]
    {
        status_bytes("VmHWM")
    }

    // Windows: `PeakWorkingSetSize` out of `PROCESS_MEMORY_COUNTERS`, which is
    // the same high-water mark under another name.
    //
    // **Hand-written rather than a `windows-sys` dependency**, and the reason
    // is this repository's own: `xtask`'s manifest says of its single
    // dependency "the one dependency, and it is a sibling rather than a third
    // party", and `tpdf` depends on nothing but `tinker-pdf`. Fifteen lines of
    // declaration against a crate graph is not a close call in a workspace
    // whose premise is that it implements its own primitives. `deny.toml` does
    // not forbid `windows-sys` — it is already in the graph under
    // `tempfile`/`proptest` — so this is a choice rather than a workaround.
    //
    // This and [`release_freed`]'s call are the CLI's only `unsafe`. Every
    // engine crate carries `#![forbid(unsafe_code)]`; the debug CLI does not,
    // and the engine stays as it was.
    #[cfg(windows)]
    {
        use std::ffi::c_void;

        /// `PROCESS_MEMORY_COUNTERS`, psapi.h. `#[repr(C)]` and the field
        /// order **are** the ABI; the names are ours. `cb` is the struct's own
        /// size, which is how this API versions itself: a caller that passes a
        /// smaller `cb` than the callee knows about gets the prefix it asked
        /// for, so an older Windows cannot overrun this allocation.
        #[repr(C)]
        #[derive(Default)]
        struct ProcessMemoryCounters {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }

        // `K32GetProcessMemoryInfo` rather than psapi's `GetProcessMemoryInfo`:
        // the K32 name is exported from kernel32.dll itself on Windows 7 and
        // later, so no second import library is needed and there is no
        // psapi/psapi_version split to get wrong.
        #[link(name = "kernel32")]
        extern "system" {
            /// A pseudo-handle to the calling process. It needs no closing.
            fn GetCurrentProcess() -> *mut c_void;
            fn K32GetProcessMemoryInfo(
                process: *mut c_void,
                counters: *mut ProcessMemoryCounters,
                cb: u32,
            ) -> i32;
        }

        let mut counters = ProcessMemoryCounters {
            cb: u32::try_from(std::mem::size_of::<ProcessMemoryCounters>()).ok()?,
            ..Default::default()
        };
        // Safe because the pointer is to a live, fully initialised local of
        // exactly the type and size named in `cb`, the handle is the process
        // pseudo-handle, and the callee writes no further than `cb` bytes.
        let ok = unsafe {
            K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0
        };
        // A zero return is a failure and is reported as "no measurement",
        // never as a peak of zero.
        ok.then_some(counters.peak_working_set_size as u64)
    }

    // Everywhere else — macOS and the two wasm targets among them — there is
    // no answer this build is willing to invent, and the runner is told by the
    // absence of the line rather than by a number that means nothing.
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

/// A field of `/proc/self/status` in bytes, from the KiB the file states it
/// in, or `None` where the field is absent or does not parse.
///
/// The one parser [`peak_bytes`], the phase marks and the file-backed share
/// read the status file through: the file says `kB` and means KiB.
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "only Linux has the file; the test reads it on every platform"
    )
)]
fn status_field(status: &str, field: &str) -> Option<u64> {
    let kib: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix(field)?.strip_prefix(':'))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    kib.checked_mul(1024)
}

/// `field` of this process's `/proc/self/status`, in bytes; `None` off Linux,
/// where there is no such file.
fn status_bytes(field: &str) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        status_field(&std::fs::read_to_string("/proc/self/status").ok()?, field)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = field;
        None
    }
}

/// The high-water marks a probe reads as its parts end, and the largest of
/// them (MS-19, MS-27).
///
/// **The record's peak is the largest mark the probe read, where the kernel's
/// own figure reads lower.** A read of the status file states the resident
/// set as it is at that moment, but the kernel raises its kept high-water mark
/// only when memory is unmapped or released, from per-CPU counters it folds in
/// batches, so what it keeps can fall short of a peak a read already saw:
/// `VmHWM` read after a part had released its memory came back hundreds of
/// kilobytes under the reading taken as that part ended. A high-water mark
/// that falls is not one, so `peak` never reads under a mark.
#[derive(Debug, Default)]
struct Marks {
    /// The largest `VmHWM` read so far, in bytes; `None` until one is.
    highest: Option<u64>,
}

impl Marks {
    /// `hwm <phase> <bytes>`: the kernel's high-water mark as a part of the
    /// probe ends, so a report can say which part reached a file's peak.
    /// Omitted off Linux, as the file-backed share is.
    ///
    /// Then what the allocator holds free goes back to the kernel
    /// ([`release_freed`]), so the next part starts from live memory alone and
    /// what its mark adds is its own.
    fn mark(&mut self, out: &mut dyn std::io::Write, phase: &str) {
        if let Some(bytes) = status_bytes("VmHWM") {
            say!(out, "hwm {phase} {bytes}");
            self.read(bytes);
        }
        release_freed();
    }

    /// Keeps `bytes` if it is the largest reading yet.
    fn read(&mut self, bytes: u64) {
        self.highest = Some(self.highest.map_or(bytes, |highest| highest.max(bytes)));
    }

    /// The peak a record states: the platform's own figure, `measured`, or
    /// the largest mark where that reads higher; either alone where only one
    /// was read.
    fn peak(&self, measured: Option<u64>) -> Option<u64> {
        match (measured, self.highest) {
            (Some(measured), Some(highest)) => Some(measured.max(highest)),
            (measured, highest) => measured.or(highest),
        }
    }
}

/// Hands what the allocator holds free back to the kernel, so the part of the
/// probe that follows starts from live memory alone (MS-27).
///
/// **The platform allocator keeps what a part frees, and what it keeps stays
/// resident.** glibc's gives the top of its heap back only past a threshold it
/// raises as it goes, and never a free page below a live allocation, so what
/// a part leaves behind turns on where each of its allocations landed — and
/// that moves from run to run with the seeds the engine's hash maps draw and
/// with where the address space is laid out. A part that followed another
/// read whatever the allocator happened to keep beside its own memory: the
/// doubled render of a ten-thousand-page document reused a rewrite's freed
/// pages on one run and added its whole canvas on the next, and a one-page
/// document's doubled render sat on a heap top one run trimmed and the next
/// kept. `malloc_trim(3)` gives back the main heap's top and every whole free
/// page inside the heaps, and the probe allocates from the main heap alone on
/// its one thread, so a part's peak is its own memory over what the probe
/// still holds, whatever the parts before it freed and in whatever order.
///
/// glibc's alone: no other target here has the call or the marks, and there
/// this does nothing.
fn release_freed() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            /// `malloc_trim(3)`, glibc 2.8 and later: gives back the main
            /// heap's top beyond `pad` bytes and every whole free page inside
            /// each arena's heaps, and returns 1 if it released any.
            fn malloc_trim(pad: usize) -> i32;
        }
        // Safe because it takes a byte count and works on the allocator's own
        // free lists under the allocator's own locks: it releases only memory
        // no allocation holds, so nothing Rust can reach is touched.
        unsafe {
            malloc_trim(0);
        }
    }
}

/// The record's memory lines: `rss-file`, the file-backed part of the
/// resident set (`RssFile`), which tells the binary's own pages from what the
/// document cost, then `peak`, never under a mark the probe read ([`Marks`]).
/// Each omitted where the platform will not say.
fn print_peak(out: &mut dyn std::io::Write, marks: &Marks) {
    if let Some(bytes) = status_bytes("RssFile") {
        say!(out, "rss-file {bytes}");
    }
    if let Some(peak) = marks.peak(peak_bytes()) {
        say!(out, "peak {peak}");
    }
}

/// What the structure join reached, summed over the pages the probe rendered.
///
/// Three counts and never a rate: `matched` alone says nothing without the
/// characters beside it that the tree did not claim. The two kinds of unclaimed
/// character stay apart for the reason [`StructuredText`] keeps them apart —
/// untagged text is a producer that never marked it, and a marked run nothing
/// claims is a structure tree that lost track of it.
///
/// [`StructuredText`]: tinker_pdf::StructuredText
#[derive(Default)]
struct Join {
    matched: usize,
    orphans: usize,
    unmarked: usize,
}

impl Join {
    fn add(&mut self, tree: &StructureTree, index: u32, page: &Page) {
        let structured = tree.text_for_page(index, &page.text());
        self.matched += structured.matched;
        self.orphans += structured.orphans;
        self.unmarked += structured.unmarked;
    }
}

/// Opens and renders one file at a time, writing a record per file.
///
/// Never returns `Err` for anything the file did: the runner reads outcomes
/// from the record, and reserves the exit code for "this process did not
/// finish", which is the one thing a record cannot say about itself.
fn probe(options: &Options) -> Result<(), String> {
    // Before the font provider, so the answer does not depend on a `--fonts`
    // path being resolvable: the question is what this binary writes, and it
    // writes the same version whatever faces it was pointed at.
    if options.record_version {
        println!("probe {PROBE_VERSION}");
        return Ok(());
    }
    let fonts = options.font_provider()?;
    for path in &options.files {
        probe_one(options, path, fonts.as_ref());
    }
    Ok(())
}

fn probe_one(options: &Options, path: &str, fonts: Option<&Arc<SimpleFontProvider>>) {
    probe_to(&mut std::io::stdout().lock(), options, path, fonts);
}

/// [`probe_one`]'s record, written to `out`.
///
/// Stdout's lock in a run, which is line-buffered, so each line reaches the
/// capture file as it is written; a buffer in a test, which reads it back.
fn probe_to(
    out: &mut dyn std::io::Write,
    options: &Options,
    path: &str,
    fonts: Option<&Arc<SimpleFontProvider>>,
) {
    let started = std::time::Instant::now();
    say!(out, "probe {PROBE_VERSION}");
    say!(out, "file {path}");

    // `phase` lines are the corpus runner's progress signal, and the reason
    // they work needs no protocol: Rust's stdout is a `LineWriter`, so each
    // one reaches the capture file when it is printed. A child killed at the
    // timeout having printed nothing for half of it was not making progress,
    // and the runner reports that as `stalled` rather than as a slow file.
    // `parse_record` ignores keys it does not know, so these cost an older
    // runner nothing.
    say!(out, "phase open");
    // **The open-and-count span**, printed as `time open` once the pages are
    // counted. It starts after the read, which measures the runner's disk
    // rather than the engine, and it holds `Document::open`, the faces, the
    // authentication and the first `page_count` -- the ladder, the producer
    // and every line printed between them stay outside. A file that does not
    // open has no span: its failure is a result, and its time stays in `ms`.
    let opened = read_file(path).and_then(|bytes| {
        let clock = std::time::Instant::now();
        let doc = open_bytes(path, bytes, options.password.as_deref(), fonts)?;
        Ok((doc, clock.elapsed()))
    });
    let (doc, opening) = match opened {
        Ok(opened) => opened,
        Err(message) => {
            // On one line, and last but for the sentinel, so a reason
            // containing anything at all cannot be mistaken for another key.
            say!(out, "opened no {}", one_line(&message));
            // Said in its own words rather than left out: a record that simply
            // omits the strict pass reads as a child that skipped it, and "the
            // file did not open" is a different fact from "this build does not
            // run the pass".
            say!(out, "strict ineligible the file did not open");
            print_peak(out, &Marks::default());
            say!(out, "ms {}", started.elapsed().as_millis());
            say!(out, "done");
            return;
        }
    };

    say!(out, "opened yes");
    if BUNDLED_FACES {
        say!(out, "build bundled-fonts");
    }
    say!(out, "ladder {:?}", doc.ladder_level());

    // **Who wrote the file, when the file says.** The production corpus's
    // whole argument is that its documents were emitted by real producers for
    // real readers, and a failure there is worth nothing until it can be
    // attributed to one of them: "eleven files fail" is a number, and "eleven
    // files fail and nine of them came out of the same generator" is a lead.
    //
    // On one line, through `one_line`, and never omitted for being empty --
    // `(none stated)` is a producer string too, and the commonest one after
    // Adobe's in the SAFEDOCS sample. A file that states nothing is a
    // population, not a gap in the data.
    let producer = doc.metadata().producer.unwrap_or_default();
    let producer = producer.trim();
    say!(
        out,
        "producer {}",
        if producer.is_empty() {
            "(none stated)".to_string()
        } else {
            one_line(producer)
        }
    );

    let counting = std::time::Instant::now();
    let pages = doc.page_count();
    let counting = counting.elapsed();
    say!(out, "pages {pages}");
    say!(
        out,
        "time open {}",
        opening.saturating_add(counting).as_micros()
    );
    let mut marks = Marks::default();
    marks.mark(out, "open");

    // Counted by kind rather than listed: a damaged file can emit tens of
    // thousands of warnings, and the report wants to know which kinds a
    // corpus produces, not to carry every instance of them.
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for warning in doc.warnings() {
        *kinds
            .entry(format!("cos:{}", cos_warning_label(&warning)))
            .or_default() += 1;
    }

    for capability in capabilities(doc.cos()) {
        say!(out, "cap {capability}");
    }

    // Bound once, outside the page loop. `Document::structure()` re-walks
    // `/StructTreeRoot` on every call, so asking per page would make an
    // N-page document cost N walks of a tree that did not change.
    let tree = doc.structure();
    match &tree {
        Some(tree) => say!(
            out,
            "tagged tree yes elements {} content {} objects {}",
            tree.element_count(),
            tree.content_count(),
            tree.object_count()
        ),
        // Its own line rather than an omission: a record with no `tagged` key
        // at all is one from a child that did not look, and that is a
        // different fact from a document with no structure tree.
        None => say!(out, "tagged tree no"),
    }
    marks.mark(out, "scan");

    let render = RenderOptions {
        annotations: options.annotations,
        ..RenderOptions::at_dpi(options.dpi)
    };
    say!(out, "phase render");
    let mut rendered = 0u32;
    let mut join = Join::default();
    let mut fonts_listed = false;
    for index in options.pages(&doc) {
        // With `--stages` the lookup is a stage of its own, announced and
        // timed, since the walk to a page is work a render waits on.
        let page = if options.stages {
            say!(out, "phase lookup");
            let clock = std::time::Instant::now();
            let page = doc.page(index);
            say!(out, "time lookup {}", clock.elapsed().as_micros());
            page
        } else {
            doc.page(index)
        };
        let Some(page) = page else {
            continue;
        };
        // Only where there is a tree to join to. Text extraction is cheap
        // beside rendering, but it is not free, and on the 3 800-odd corpus
        // files carrying no structure tree it would measure nothing. Announced,
        // so a child killed inside a join is not read as one inside the page
        // before it.
        if let Some(tree) = &tree {
            say!(out, "phase join {}/{pages}", index + 1);
            join.add(tree, index, &page);
        }
        // Before the page rather than after it, so the line names the page
        // being worked on when a kill arrives rather than the last one that
        // finished. The runner never reads the number, only the fact that the
        // capture file grew; the number is for whoever reads the report.
        say!(out, "page {}/{pages}", index + 1);
        let warnings = if options.stages {
            stages(out, &doc, &page, &render, options, &mut fonts_listed)
        } else {
            // **A page's span**: the render and nothing else. The lookup
            // above is outside it, because today's walk to a page gathers
            // every leaf before it, so on a long document the lookups grow
            // with the square of its pages and would be most of what a span
            // measured; so are the structure join and the progress line.
            let clock = std::time::Instant::now();
            let bitmap = page.render(&render);
            let micros = clock.elapsed().as_micros();
            say!(out, "time page {} {micros}", index + 1);
            bitmap.warnings
        };
        // Ruling 2's definition, and the one the ratchet counts: a bitmap
        // came back. A page rendered with a JBIG2 placeholder on it rendered.
        rendered += 1;
        for warning in &warnings {
            *kinds
                .entry(format!("render:{}", render_warning_label(warning)))
                .or_default() += 1;
        }
    }
    say!(out, "rendered {rendered}");
    if tree.is_some() {
        say!(
            out,
            "tagged chars matched {} orphans {} unmarked {}",
            join.matched,
            join.orphans,
            join.unmarked
        );
    }
    marks.mark(out, "render");

    // What this document costs to work on, as properties of the document.
    //
    // Reported rather than acted on. It was measured so that the metamorphic
    // gate could stop being a clock, and the measurement said it could not:
    // bytes, objects and pixels do not predict the time a file takes, which is
    // why the gate was deleted rather than replaced. The three numbers stay
    // because a per-file report of what a corpus costs is worth having on its
    // own, and because the next reader deserves the evidence rather than the
    // conclusion.
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let objects = doc.cos().xref().len();
    let pixels = doc.page(0).map_or(0u64, |page| {
        let (x0, y0, x1, y1) = page.crop_box();
        let scale = f64::from(options.dpi as f32) / 72.0;
        let w = ((x1 - x0) * scale).abs().ceil().max(0.0);
        let h = ((y1 - y0) * scale).abs().ceil().max(0.0);
        if w.is_finite() && h.is_finite() {
            (w as u64) * (h as u64)
        } else {
            0
        }
    });
    say!(out, "cost bytes {bytes} objects {objects} pixels {pixels}");

    say!(out, "phase strict");
    strict(out, &doc);
    marks.mark(out, "strict");
    metamorphic(out, &mut marks, &doc, options, fonts);
    marks.mark(out, "metamorphic");

    for (kind, count) in &kinds {
        say!(out, "warn {kind} {count}");
    }
    // Last of the measurements and after every phase, so it is the peak of the
    // whole file's work rather than of the part that had run when it was
    // asked. One file per process is what makes this a per-file number at all:
    // the runner spawns a child per path, so nothing another file allocated is
    // in this high-water mark.
    //
    // Omitted where the platform will not say. See [`peak_bytes`] for why that
    // is an omission rather than a zero.
    print_peak(out, &marks);
    say!(out, "ms {}", started.elapsed().as_millis());
    say!(out, "done");
}

/// Ruling 13's validator, over a **rewrite** of the file rather than over the
/// file itself.
///
/// The corpus is other people's documents, and the question the ratchet asks
/// is about *this writer*: given a document it could read cleanly, does it
/// produce a file that holds up to ISO 32000 read strictly. So the source is
/// re-serialised first and the verdict is passed on the result.
///
/// Only [`Tier::Structure`] is comparable that way, and the tier split is why:
/// a rewrite owns the header, the sections, the offsets and the extents
/// whatever it was handed, and inherits the page tree, the outline and the
/// resource dictionaries from its source. A `/Rect` written backwards in
/// somebody's 2003 invoice is reported here and belongs to the invoice.
///
/// A file that needed the leniency ladder to open is not eligible: its own
/// structure is already known to be damaged, so a rewrite of it says nothing
/// about the writer. An encrypted one is not eligible either, because without
/// the password the rewrite cannot carry its streams.
/// Why a document's own bytes cannot be trusted for a rewrite, or `None`.
///
/// One definition, used by the strict pass and by the metamorphic relations
/// that rewrite. Both are asking the same question — *is a rewrite of this
/// document a comparison or a repair?* — and two spellings of it would drift.
/// `--stages`: one page's render taken apart through announced stages, in
/// place of its one `page.render` (MS-20). Each stage is a `phase` line
/// before it and `time <stage> <micros>` after, so the stage a killed child
/// stopped in is the last one announced: `fonts` (the document's, once),
/// `images` (the decode without the conversion a render adds), `record` (the
/// page's display list), `draw` (the list rendered, annotations off) and
/// `annotations` (the same with them on, passed over after a cancelled
/// `draw`). A replay is the direct render byte for byte, so these are
/// `Page::render`'s own work taken apart.
///
/// Returns the warnings of the render whose annotations are the run's, which
/// the record counts as a page's.
fn stages(
    out: &mut dyn std::io::Write,
    doc: &Document,
    page: &tinker_pdf::Page,
    render: &RenderOptions,
    options: &Options,
    fonts_listed: &mut bool,
) -> Vec<tinker_pdf::RenderWarning> {
    if !*fonts_listed {
        *fonts_listed = true;
        say!(out, "phase fonts");
        let clock = std::time::Instant::now();
        let found = doc.fonts();
        let micros = clock.elapsed().as_micros();
        for font in &found {
            say!(
                out,
                "font {} {:?} {}",
                if font.name.is_empty() {
                    "(no /BaseFont)"
                } else {
                    font.name.as_str()
                },
                font.kind,
                match font.program {
                    Some(program) => format!("{:?}", program.key),
                    None => "not embedded".to_string(),
                }
            );
        }
        say!(out, "time fonts {micros}");
    }

    say!(out, "phase images");
    let clock = std::time::Instant::now();
    let images = page.images();
    let micros = clock.elapsed().as_micros();
    for image in &images {
        say!(out, "image {}", images::describe(image));
    }
    say!(out, "time images {micros}");

    say!(out, "phase record");
    let clock = std::time::Instant::now();
    let list = page.display_list();
    say!(out, "time record {}", clock.elapsed().as_micros());
    // A recording past its budget is not retained, and every render of it is
    // then a direct one, which the line says.
    say!(
        out,
        "list {} retained {}",
        list.len(),
        if list.is_retained() { "yes" } else { "no" }
    );

    let limit = options.stage_limit.map(std::time::Duration::from_secs);
    let mut drawn = Vec::new();
    for (stage, annotations) in [("draw", false), ("annotations", true)] {
        say!(out, "phase {stage}");
        let clock = std::time::Instant::now();
        let bitmap = within_limit(limit, |cancel| {
            list.render(&RenderOptions {
                annotations,
                stats: true,
                cancel,
                ..render.clone()
            })
        });
        say!(out, "time {stage} {}", clock.elapsed().as_micros());
        if let Some(stats) = &bitmap.stats {
            say!(out, "counts {stage} {stats:?}");
            if let Some(line) = most_decoded(doc.cos(), stage, stats) {
                say!(out, "{line}");
            }
        }
        let cancelled = bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::Cancelled);
        if cancelled {
            say!(out, "cancelled {stage}");
        }
        if annotations == options.annotations || cancelled {
            drawn = bitmap.warnings;
        }
        if cancelled {
            break;
        }
    }
    drawn
}

/// Runs `stage` with a cancel token the probe cancels once `limit` has
/// passed, or with none where there is no limit; a limit of zero is
/// cancelled before the stage starts.
fn within_limit<T>(
    limit: Option<std::time::Duration>,
    stage: impl FnOnce(Option<tinker_pdf::CancelToken>) -> T,
) -> T {
    let Some(limit) = limit else {
        return stage(None);
    };
    let token = tinker_pdf::CancelToken::new();
    if limit.is_zero() {
        token.cancel();
        return stage(Some(token));
    }
    let (finished, waiting) = std::sync::mpsc::channel::<()>();
    let watcher = {
        let token = token.clone();
        std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = waiting.recv_timeout(limit) {
                token.cancel();
            }
        })
    };
    let out = stage(Some(token));
    let _ = finished.send(());
    let _ = watcher.join();
    out
}

fn cleanly_read(doc: &Document) -> Option<&'static str> {
    if doc.ladder_level() != LadderLevel::Trust {
        return Some("the file needed the leniency ladder to open");
    }
    if !doc.warnings().is_empty() {
        return Some("the file opened with warnings");
    }
    if doc.is_encrypted() {
        return Some("the file is encrypted");
    }
    None
}

fn strict(out: &mut dyn std::io::Write, doc: &Document) {
    if let Some(why) = cleanly_read(doc) {
        // With the repairs that made it so: objects load lazily, so a
        // leniency a page's drawing needed is a reason as much as one the open
        // needed, and a file that stops being eligible names what it started
        // needing rather than only that it did.
        let warnings = doc.warnings();
        if warnings.is_empty() {
            say!(out, "strict ineligible {why}");
        } else {
            say!(out, "strict ineligible {why}: {}", repairs(&warnings));
        }
        return;
    }

    say!(out, "strict eligible");
    let saved = doc.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    let Ok(again) = Document::open(saved) else {
        // Not an ineligibility: a rewrite this engine cannot re-open is the
        // worst defect on this axis, and filing it as "not measured" would
        // make the number go up by hiding it.
        say!(out, "strict structure 1");
        say!(out, "strict semantics 0");
        say!(out, "strict kind rewrite-did-not-open 1");
        return;
    };

    let defects = again.validate();
    let structure = defects
        .iter()
        .filter(|defect| defect.kind.tier() == Tier::Structure)
        .count();
    say!(out, "strict structure {structure}");
    say!(
        out,
        "strict semantics {}",
        defects.len().saturating_sub(structure)
    );
    // By kind rather than one line per defect: a damaged source can produce
    // thousands of the same finding, and the report wants to know which rules
    // a corpus breaks.
    for (kind, count) in tinker_pdf::kind_counts(&defects) {
        say!(out, "strict kind {kind} {count}");
    }
}

/// Anything that would break the one-record-per-line format, flattened.
/// The stream a stage decoded most, where it decoded one more than once: its
/// reference, how many times, and its dictionary on one line — what the
/// engine read of it, never its data — so a trace names the object a page
/// kept decoding, which the count alone cannot.
fn most_decoded(cos: &CosDocument, stage: &str, stats: &tinker_pdf::Stats) -> Option<String> {
    let reference = stats.most_decoded_stream?;
    if stats.most_decodes_of_one_stream < 2 {
        return None;
    }
    let described = match cos.get(reference) {
        Ok(object) => {
            let mut text = String::new();
            write_object(cos, &object, 0, &mut text);
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        }
        Err(error) => format!("unreadable: {error:?}"),
    };
    Some(format!(
        "decoded {stage} {} {} {} times: {described}",
        reference.num, reference.gen, stats.most_decodes_of_one_stream
    ))
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// A stable label per COS warning kind.
///
/// The variant name only: the payloads are byte offsets and object numbers,
/// which differ per file and would make every count one.
/// The repairs a document's warnings name, for a reason line: each kind once,
/// in the order it first arrived, with the object it was first raised on, and
/// at most [`REPAIRS_NAMED`] of them.
fn repairs(warnings: &[tinker_pdf::Warning]) -> String {
    let mut named: Vec<(String, Option<ObjRef>)> = Vec::new();
    for warning in warnings {
        let kind = repair_label(warning);
        if !named.iter().any(|(known, _)| *known == kind) {
            named.push((kind, warning.object));
        }
    }
    let mut shown: Vec<String> = named
        .iter()
        .take(REPAIRS_NAMED)
        .map(|(kind, object)| match object {
            Some(object) => format!("{kind} in object {} {}", object.num, object.gen),
            None => kind.clone(),
        })
        .collect();
    if named.len() > REPAIRS_NAMED {
        shown.push(format!("{} more", named.len() - REPAIRS_NAMED));
    }
    shown.join(", ")
}

/// How many kinds of repair a reason line names before it counts the rest.
const REPAIRS_NAMED: usize = 4;

/// A repair as a strict reason names it: its kind, and the variant the
/// kind carries where it carries one, so `Filter(AdlerMismatch)` says which
/// of the filters' leniencies a stream needed where `Filter` alone does not
/// (MS-22). No field's value is named, so two repairs of one kind are one.
fn repair_label(warning: &tinker_pdf::Warning) -> String {
    variant_names(&format!("{:?}", warning.kind))
}

/// The variant names at the head of a `Debug` rendering: the outer one, and
/// inside its parentheses the next where one begins there.
fn variant_names(debug: &str) -> String {
    let name: String = debug
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    if name.is_empty() {
        return "Unknown".to_string();
    }
    match debug[name.len()..].strip_prefix('(') {
        Some(inner) if inner.starts_with(|c: char| c.is_ascii_uppercase()) => {
            format!("{name}({})", variant_names(inner))
        }
        _ => name,
    }
}

fn cos_warning_label(warning: &tinker_pdf::Warning) -> String {
    let debug = format!("{:?}", warning.kind);
    debug
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or("Unknown")
        .to_string()
}

/// A stable label per render warning.
///
/// Spelled out rather than derived from `Debug`, because two of these carry
/// the payload that matters: which codec was refused and which shading type
/// was skipped are the hit-rate table's whole content, and a label that
/// dropped them would count every unsupported image together.
fn render_warning_label(warning: &tinker_pdf::RenderWarning) -> String {
    use tinker_pdf::RenderWarning as W;
    match warning {
        W::UnsupportedImage { codec } => format!("UnsupportedImage({codec})"),
        W::DamagedImage { reason, .. } => format!("DamagedImage({reason})"),
        W::RepairedColorSpace { reason, .. } => format!("RepairedColorSpace({reason})"),
        W::PageScaledDown { .. } => "PageScaledDown".to_string(),
        W::EmptyTextClip => "EmptyTextClip".to_string(),
        W::UnreadableFont => "UnreadableFont".to_string(),
        W::UnsupportedShading { kind } => format!("UnsupportedShading({kind})"),
        W::UnsupportedPattern { .. } => "UnsupportedPattern".to_string(),
        W::HiddenOptionalContent { .. } => "HiddenOptionalContent".to_string(),
        W::GroupBudgetSpent { .. } => "GroupBudgetSpent".to_string(),
        W::RecursiveContent => "RecursiveContent".to_string(),
        W::DeviceParametersSpent => "DeviceParametersSpent".to_string(),
        W::NestedBudgetSpent => "NestedBudgetSpent".to_string(),
        W::ApproximatedGroupBlend => "ApproximatedGroupBlend".to_string(),
        W::Cancelled => "Cancelled".to_string(),
        W::RegionClamped { .. } => "RegionClamped".to_string(),
    }
}

/// What a file would need in order to render fully, read from its objects.
///
/// From the object graph rather than from the render warnings, and the
/// difference is the point of the table. A warning says "a page that was
/// rendered wanted JBIG2"; this says "the file contains JBIG2", which is the
/// question gaps 17 and 18 are asking. They differ for every file whose
/// JBIG2 image sits on a page behind an optional-content group, inside an
/// unreferenced object, or on a page a truncated run never reached — and
/// scheduling work by the warning count would systematically undercount
/// exactly the features that are hardest to reach.
fn capabilities(cos: &CosDocument) -> BTreeSet<&'static str> {
    let mut found = BTreeSet::new();
    // The same, and this one was measured: `probe` over
    // `pdfjs/test/pdfs/bug1980958.pdf` took 61 seconds, all of it here, for a
    // 219-byte file that opens and renders in a twentieth of a second.
    for (number, entry) in cos.xref().iter() {
        let generation = match entry {
            XrefEntry::Offset { gen, .. } => gen,
            XrefEntry::InStream { .. } => 0,
            XrefEntry::Free { .. } => continue,
        };
        let Ok(object) = cos.get(ObjRef::new(number, generation)) else {
            continue;
        };
        scan_capabilities(cos, &object, 0, &mut found);
    }
    found
}

fn scan_capabilities(
    cos: &CosDocument,
    object: &Object,
    depth: u32,
    found: &mut BTreeSet<&'static str>,
) {
    // The object graph is a graph rather than a tree, and a malformed file's
    // can cycle. Every capability this looks for sits within a few levels of
    // the object that owns it, so a bound costs nothing real.
    if depth > 8 {
        return;
    }
    match object {
        Object::Dict(dict) | Object::Stream(StreamObj { dict, .. }) => {
            scan_dict(cos, dict, depth, found);
        }
        Object::Array(items) => {
            for item in items {
                scan_capabilities(cos, item, depth + 1, found);
            }
        }
        _ => {}
    }
}

fn scan_dict(cos: &CosDocument, dict: &Dict, depth: u32, found: &mut BTreeSet<&'static str>) {
    for (key, value) in dict.iter() {
        let key = cos.name_bytes(*key).map(|b| b.to_vec()).unwrap_or_default();
        match key.as_slice() {
            b"Filter" | b"F" => {
                for name in filter_names(cos, value) {
                    match name.as_slice() {
                        b"JBIG2Decode" | b"JBIG2" => {
                            found.insert("jbig2");
                        }
                        b"JPXDecode" => {
                            found.insert("jpx");
                        }
                        _ => {}
                    }
                }
            }
            b"ColorSpace" | b"CS" => {
                // An `ICCBased` space names its profile in the second element
                // of an array (8.6.5.5). Counted because it is the highest
                // reachability in the engine — half the corpus's files carry a
                // profile — so the number is worth watching rather than
                // inferring from a warning that no longer fires.
                if names_iccbased(cos, value, depth) {
                    found.insert("iccbased");
                }
            }
            b"ByteRange" => {
                // 12.8.1: only a signature dictionary has one. Counted here
                // rather than inferred from a warning, because reading a
                // signature produces no warning when it succeeds — and the
                // number is what says whether the reader is still finding
                // them all.
                found.insert("signature");
            }
            b"ShadingType" => {
                // 8.7.4.5.5-8: types 4 to 7 are the mesh shadings, which is
                // exactly gap 10's scope. 1 to 3 are built.
                if let Some(4..=7) = value.as_int() {
                    found.insert("mesh-shading");
                }
            }
            _ => {}
        }
        scan_capabilities(cos, value, depth + 1, found);
    }
}

/// Whether a `/ColorSpace` value names an `ICCBased` space.
///
/// The value may be the array itself, a reference to one, or a dictionary of
/// named spaces each of which is one — which is why this walks rather than
/// pattern-matching a single shape.
fn names_iccbased(cos: &CosDocument, value: &Object, depth: u32) -> bool {
    if depth > 8 {
        return false;
    }
    let resolved;
    let value = match value {
        Object::Ref(reference) => match cos.get(*reference) {
            Ok(object) => {
                resolved = object;
                &*resolved
            }
            Err(_) => return false,
        },
        other => other,
    };
    match value {
        Object::Array(items) => {
            let first = items
                .first()
                .and_then(Object::as_name)
                .and_then(|n| cos.name_bytes(n).map(|b| b.to_vec()));
            if first.as_deref() == Some(b"ICCBased") {
                return true;
            }
            items
                .iter()
                .any(|item| names_iccbased(cos, item, depth + 1))
        }
        Object::Dict(dict) => dict
            .iter()
            .any(|(_, entry)| names_iccbased(cos, entry, depth + 1)),
        _ => false,
    }
}

/// Every filter name a `/Filter` entry mentions, in either of 7.4's two
/// shapes, resolving one level of indirection.
fn filter_names(cos: &CosDocument, value: &Object) -> Vec<Vec<u8>> {
    let resolved;
    let value = match value {
        Object::Ref(reference) => match cos.get(*reference) {
            Ok(object) => {
                resolved = object;
                &*resolved
            }
            Err(_) => return Vec::new(),
        },
        other => other,
    };
    match value {
        Object::Name(name) => cos
            .name_bytes(*name)
            .map(|b| vec![b.to_vec()])
            .unwrap_or_default(),
        Object::Array(items) => items
            .iter()
            .filter_map(Object::as_name)
            .filter_map(|n| cos.name_bytes(n).map(|b| b.to_vec()))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf::{DocumentBuilder, ImageData, PdfString};

    /// The status file states its sizes in what it calls `kB` and means KiB;
    /// a field it does not state, or states as something else, is `None`
    /// rather than a zero, as a missing peak is.
    #[test]
    fn a_status_field_is_read_in_kib_and_a_missing_one_is_none() {
        let status = "Name:\ttpdf\nVmPeak:\t  20480 kB\nVmHWM:\t    1234 kB\n\
                      RssAnon:\t     900 kB\nRssFile:\t     334 kB\nVmHWMX:\t 7 kB\n";
        assert_eq!(status_field(status, "VmHWM"), Some(1234 * 1024));
        assert_eq!(status_field(status, "RssFile"), Some(334 * 1024));
        assert_eq!(status_field(status, "RssShmem"), None, "absent");
        assert_eq!(
            status_field("VmHWM:\tmany kB\n", "VmHWM"),
            None,
            "unparseable"
        );
        assert_eq!(
            status_field("VmHWMX:\t 7 kB\n", "VmHWM"),
            None,
            "another field"
        );
    }

    /// **A mark read lower than an earlier one lowers nothing** (MS-27): the
    /// record's peak is the platform's figure or the largest mark, whichever
    /// is higher, and either alone where only one was read.
    #[test]
    fn the_peak_is_never_under_a_mark_the_probe_read() {
        let mut marks = Marks::default();
        assert_eq!(marks.peak(None), None, "nothing read");
        assert_eq!(marks.peak(Some(5)), Some(5), "the platform's figure alone");
        marks.read(100);
        marks.read(80);
        assert_eq!(marks.highest, Some(100), "a lower reading keeps the higher");
        assert_eq!(
            marks.peak(Some(90)),
            Some(100),
            "the kernel's figure fell short"
        );
        assert_eq!(
            marks.peak(Some(120)),
            Some(120),
            "the kernel's figure rose past"
        );
        assert_eq!(marks.peak(None), Some(100), "the final read failed");
    }

    /// **A record states no peak under one of its own marks**: the line the
    /// runner keeps is no lower than any `hwm` the same child wrote before it.
    /// Off Linux a record has neither, and there is nothing to hold.
    #[test]
    fn a_record_states_no_peak_under_one_of_its_marks() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "every part marked");
        });
        let record = probe_record(&builder.finish(), &["--dpi", "72"], "marks");
        let peak = record
            .iter()
            .find_map(|line| line.strip_prefix("peak "))
            .and_then(|bytes| bytes.parse::<u64>().ok());
        let marks: Vec<u64> = record
            .iter()
            .filter_map(|line| line.strip_prefix("hwm "))
            .filter_map(|rest| rest.split_whitespace().nth(1)?.parse().ok())
            .collect();
        if cfg!(target_os = "linux") {
            let peak = peak.expect("a Linux record states a peak");
            assert!(
                !marks.is_empty(),
                "a Linux record marks its parts: {record:?}"
            );
            for mark in &marks {
                assert!(peak >= *mark, "a peak of {peak} under a mark of {mark}");
            }
        } else {
            assert!(marks.is_empty(), "only Linux marks: {record:?}");
        }
    }

    /// The probe's record of `bytes` under `args`, line by line, as the
    /// runner would read it from the capture file.
    fn probe_record(bytes: &[u8], args: &[&str], name: &str) -> Vec<String> {
        let path = std::env::temp_dir().join(format!("tpdf-{name}-{}.pdf", std::process::id()));
        std::fs::write(&path, bytes).expect("the fixture is written");
        let file = path.to_string_lossy().into_owned();
        let mut all: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        all.push(file.clone());
        let options = Options::parse(&all).expect("the options parse");
        let mut out = Vec::new();
        probe_to(&mut out, &options, &file, None);
        let _ = std::fs::remove_file(&path);
        String::from_utf8(out)
            .expect("the record is text")
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The stage lines of a record, `phase <stage>` and `time <stage>`, in
    /// the order written.
    fn stage_lines(record: &[String]) -> Vec<String> {
        const STAGES: [&str; 6] = ["lookup", "fonts", "images", "record", "draw", "annotations"];
        record
            .iter()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                match (words.next(), words.next()) {
                    (Some(kind @ ("phase" | "time")), Some(stage)) if STAGES.contains(&stage) => {
                        Some(format!("{kind} {stage}"))
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// **Every stage is announced before it runs and timed after**, page by
    /// page, over a document with an embedded face, an image and an
    /// annotation on each page: the fonts once, then each page's images,
    /// recording, draw and annotations, with the font, image and `list`
    /// lines between, and a record ending as the probe's does without
    /// `--stages` — the same `rendered`, `warn` and relation lines.
    #[test]
    fn every_stage_is_announced_before_it_runs_and_timed_after() {
        let bytes = mixed_pages(2);
        let staged = probe_record(
            &bytes,
            &["--dpi", "72", "--stages", "--stage-limit", "600"],
            "stages",
        );
        let page = |fonts: bool| -> Vec<String> {
            ["lookup", "fonts", "images", "record", "draw", "annotations"]
                .into_iter()
                .filter(|stage| fonts || *stage != "fonts")
                .flat_map(|stage| [format!("phase {stage}"), format!("time {stage}")])
                .collect()
        };
        let mut expected = page(true);
        expected.extend(page(false));
        assert_eq!(stage_lines(&staged), expected, "{staged:#?}");

        let count = |prefix: &str| staged.iter().filter(|l| l.starts_with(prefix)).count();
        assert!(
            count("font ") >= 2,
            "the embedded face and Helvetica: {staged:#?}"
        );
        assert_eq!(count("image "), 2, "one image a page: {staged:#?}");
        assert_eq!(count("list "), 2, "one recording a page: {staged:#?}");
        assert_eq!(count("counts draw "), 2, "{staged:#?}");
        assert_eq!(count("cancelled "), 0, "nothing reached its limit");
        assert_eq!(staged.last().map(String::as_str), Some("done"));

        let plain = probe_record(&bytes, &["--dpi", "72"], "unstaged");
        let outcome = |record: &[String]| -> Vec<String> {
            record
                .iter()
                .filter(|l| {
                    l.starts_with("rendered ") || l.starts_with("warn ") || l.starts_with("meta ")
                })
                .cloned()
                .collect()
        };
        assert_eq!(outcome(&staged), outcome(&plain));
        assert!(
            outcome(&plain).iter().any(|l| l.starts_with("meta ")),
            "{plain:#?}"
        );
    }

    /// A stage past `--stage-limit` comes back `cancelled` with the counts it
    /// reached, and `annotations` is passed over after a cancelled `draw`.
    #[test]
    fn a_stage_past_its_limit_comes_back_cancelled_with_its_counts() {
        let record = probe_record(
            &mixed_pages(1),
            &["--dpi", "72", "--stages", "--stage-limit", "0"],
            "cancelled",
        );
        assert!(record.iter().any(|l| l == "cancelled draw"), "{record:#?}");
        assert!(
            record.iter().any(|l| l.starts_with("counts draw ")),
            "{record:#?}"
        );
        assert!(
            !record.iter().any(|l| l == "phase annotations"),
            "passed over after a cancelled draw: {record:#?}"
        );
        assert!(record.iter().any(|l| l == "rendered 1"), "{record:#?}");
        assert_eq!(record.last().map(String::as_str), Some("done"));
    }

    /// A stage that decoded a stream more than once names it, with its
    /// dictionary on one line; once, or nothing at all, is no finding.
    #[test]
    fn a_stream_decoded_more_than_once_is_named_with_its_dictionary() {
        let doc = document();
        let cos = doc.cos();
        let content = (1..cos.xref().len() as u32)
            .map(|number| ObjRef::new(number, 0))
            .find(|reference| matches!(cos.get(*reference).as_deref(), Ok(Object::Stream(_))))
            .expect("the page's content stream");
        let mut stats = tinker_pdf::Stats::default();
        assert_eq!(most_decoded(cos, "draw", &stats), None, "nothing decoded");
        stats.most_decoded_stream = Some(content);
        stats.most_decodes_of_one_stream = 1;
        assert_eq!(
            most_decoded(cos, "draw", &stats),
            None,
            "once is no finding"
        );
        stats.most_decodes_of_one_stream = 3;
        let line = most_decoded(cos, "draw", &stats).expect("a stream decoded thrice");
        assert!(
            line.starts_with(&format!("decoded draw {} 0 3 times: <<", content.num)),
            "{line}"
        );
        assert!(line.contains("/Length"), "its dictionary: {line}");
        assert!(!line.contains('\n') && !line.contains("  "), "{line:?}");
    }

    fn document() -> Document {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.set_info(b"Title", "a document");
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "hello");
        });
        Document::open(builder.finish()).expect("it opens")
    }

    /// A document of `count` pages, each doing a different amount of work.
    ///
    /// Unequal on purpose: a pool over pages that all cost the same tends to
    /// finish them in order anyway, and a test that cannot tell completion
    /// order from page order cannot catch a `render` that prints in the first.
    fn many_pages(count: u32) -> Document {
        let mut builder = DocumentBuilder::new();
        for page_number in 0..count {
            builder.add_page(60.0, 40.0, |page| {
                for n in 0..=page_number {
                    page.fill_rect(f64::from(n % 24), 1.0, 2.0, 2.0, 0.25);
                }
            });
        }
        Document::open(builder.finish()).expect("it opens")
    }

    /// An empty directory of this test's own, named after it.
    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("tpdf-render-{name}"));
        // Emptied rather than reused: a file left by an earlier run would make
        // "the page after the failure was written" true for the wrong reason.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir.to_string_lossy().replace('\\', "/")
    }

    fn render_options(dir: &str, jobs: &str) -> Options {
        Options::parse(&[
            "--out".to_string(),
            dir.to_string(),
            "--jobs".to_string(),
            jobs.to_string(),
            "doc.pdf".to_string(),
        ])
        .expect("parses")
    }

    /// `--jobs` changes the clock and nothing else.
    ///
    /// The pool's whole contract: `--jobs 8` renders the same pages to the
    /// same files and prints the same bytes as `--jobs 1`. Workers finish in
    /// whatever order the scheduler gives them, so the lines are buffered per
    /// page and ordered by page afterwards — the same reason `xtask`'s corpus
    /// pool sorts its results, that output which moves between runs is not
    /// diffable.
    ///
    /// Repeated, because one pooled run finishing in page order by luck is not
    /// evidence that it always would.
    #[test]
    fn jobs_changes_the_clock_and_not_one_byte_of_the_output() {
        let doc = many_pages(48);
        let dir = scratch("identical-output");

        let serial = render_walk(&render_options(&dir, "1"), &dir, "doc", &doc);
        assert_eq!(serial.len(), 48);
        assert!(
            serial.iter().all(Result::is_ok),
            "the serial baseline must itself succeed: {serial:?}"
        );
        // Page order absolutely, not merely stably: the first line names page
        // one and the last names page forty-eight.
        assert!(
            serial[0].as_ref().expect("page 1").contains("doc-0001.png"),
            "{:?}",
            serial[0]
        );
        assert!(
            serial[47]
                .as_ref()
                .expect("page 48")
                .contains("doc-0048.png"),
            "{:?}",
            serial[47]
        );

        for attempt in 0..5 {
            let pooled = render_walk(&render_options(&dir, "8"), &dir, "doc", &doc);
            assert_eq!(
                serial, pooled,
                "attempt {attempt}: `--jobs 8` must print exactly what `--jobs 1` printed"
            );
        }
    }

    /// **`tpdf render` renders on the facade's page walk.** At `--jobs 4`
    /// each page's render, counted, says one of the engine's pool's workers
    /// ran it and the caller did not, and at `--jobs 1` the one worker it
    /// starts; counting changes nothing a page prints or writes, and the
    /// pages come back in page order.
    #[test]
    fn render_goes_through_the_facades_page_walk() {
        let doc = many_pages(8);
        for jobs in ["4", "1"] {
            let dir = scratch(&format!("facade-walk-{jobs}"));
            let options = render_options(&dir, jobs);
            let walked = walk_to_files(&options, &dir, "doc", &doc, true);
            assert_eq!(walked.len(), 8, "--jobs {jobs}: every page");
            for (index, (report, stats)) in walked.iter().enumerate() {
                let stats = stats.expect("the walk counted");
                assert_eq!(
                    (stats.pool_items_on_workers, stats.pool_items_on_caller),
                    (1, 0),
                    "--jobs {jobs}: page {} ran on a worker of the pool",
                    index + 1
                );
                assert!(
                    report
                        .as_ref()
                        .is_ok_and(|lines| lines.contains(&format!("doc-{:04}.png", index + 1))),
                    "--jobs {jobs}: page {} in its place: {report:?}",
                    index + 1,
                );
            }
            let counted: Vec<Result<String, String>> =
                walked.into_iter().map(|(report, _)| report).collect();
            assert_eq!(
                counted,
                render_walk(&options, &dir, "doc", &doc),
                "--jobs {jobs}: counting changes nothing printed"
            );
        }
    }

    /// Liberation Serif, the face `bundled-fonts` carries, read from where the
    /// font crate vendors it, as `writing`'s tests read it.
    fn serif() -> Vec<u8> {
        let path = format!(
            "{}/../../crates/tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(path).expect("the vendored Liberation face")
    }

    /// `count` pages, no two alike and no two the same work, each drawing
    /// text in an embedded face, an image, anti-aliased edges and an
    /// annotation with an appearance stream, and naming a face it does not
    /// embed for one line, which `bundled-fonts` draws and the default build
    /// warns about.
    ///
    /// One font program and one image serve every page, because what threads
    /// rendering one document share is its objects. The pages differ so that
    /// a page written under another page's name is a different picture, and
    /// cost different amounts for the reason [`many_pages`] gives.
    fn mixed_pages(count: u32) -> Vec<u8> {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_embedded_font(b"F0", b"LiberationSerif", &serif()));
        builder.add_base_font(b"F1", b"Helvetica");
        let samples: Vec<u8> = (0..48u8).map(|n| n.wrapping_mul(53)).collect();
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 4,
                height: 4,
                data: &samples,
            }
        ));
        for n in 0..count {
            let step = f64::from(n);
            builder.add_page(90.0, 60.0, |page| {
                page.text(
                    b"F0",
                    11.0 + step % 4.0,
                    4.0,
                    44.0,
                    &format!("Page {}", n + 1),
                );
                page.text(b"F1", 7.0, 4.0, 33.0, "tinker-pdf");
                page.image(b"Im0", 52.0 + step % 3.0, 26.0, 31.0, 21.5);
                for k in 0..=n {
                    let k = f64::from(k);
                    page.fill_rect(
                        2.5 + k % 21.0 * 4.1,
                        3.3,
                        2.6,
                        7.0 + k % 5.0,
                        0.2 + k % 9.0 * 0.03,
                    );
                }
            });
        }

        let document = Document::open(builder.finish()).expect("it opens");
        let mut editor = document.editor();
        for n in 0..count {
            let left = 40.0 + f64::from(n % 5);
            let mut square = Dict::new();
            square.insert(
                editor.intern(b"Type"),
                Object::Name(editor.intern(b"Annot")),
            );
            square.insert(
                editor.intern(b"Subtype"),
                Object::Name(editor.intern(b"Square")),
            );
            square.insert(
                editor.intern(b"Rect"),
                Object::Array(vec![
                    Object::Real(left),
                    Object::Real(8.5),
                    Object::Real(left + 40.0),
                    Object::Real(30.5),
                ]),
            );
            square.insert(
                editor.intern(b"C"),
                Object::Array(vec![
                    Object::Real(0.8),
                    Object::Real(0.1),
                    Object::Real(0.2),
                ]),
            );
            // 12.5.2: the print flag, so the page shows it on paper as well.
            square.insert(editor.intern(b"F"), Object::Int(4));
            assert!(
                editor.add_annotation(n, square).is_some(),
                "page {n} takes its annotation"
            );
        }
        editor.save(&WriteOptions::default())
    }

    /// What `tpdf render --out DIR --jobs N --quiet PATH` is given, parsed.
    fn render_into(out: &str, jobs: usize, path: &str) -> Options {
        Options::parse(&[
            "--out".to_string(),
            out.to_string(),
            "--jobs".to_string(),
            jobs.to_string(),
            "--quiet".to_string(),
            path.to_string(),
        ])
        .expect("parses")
    }

    /// Where two PNGs of one page part, in pixels: what the bytes of a
    /// compressed file cannot say.
    fn parted(mine: &[u8], theirs: &[u8]) -> String {
        let (Ok(mine), Ok(theirs)) = (Bitmap::from_png(mine), Bitmap::from_png(theirs)) else {
            return "one of the two does not decode as a PNG".to_string();
        };
        if (mine.width, mine.height, mine.format) != (theirs.width, theirs.height, theirs.format) {
            return format!(
                "a {}x{} {:?} picture against a {}x{} {:?} one",
                mine.width, mine.height, mine.format, theirs.width, theirs.height, theirs.format
            );
        }
        let components = mine.components();
        let pixels = mine
            .data
            .chunks(components)
            .zip(theirs.data.chunks(components));
        let (mut differing, mut worst, mut first) = (0usize, 0u8, None);
        for (at, (a, b)) in pixels.enumerate() {
            if a != b {
                differing += 1;
                for (x, y) in a.iter().zip(b) {
                    worst = worst.max(x.abs_diff(*y));
                }
                first.get_or_insert(at);
            }
        }
        let width = mine.width.max(1) as usize;
        match first {
            Some(at) => format!(
                "{differing} of {} pixels differ, worst component {worst} levels, first at ({}, {})",
                mine.data.len() / components.max(1),
                at % width,
                at / width
            ),
            None => "the same pixels, encoded differently".to_string(),
        }
    }

    /// **A9 for the CLI's walk: `tpdf render` at `--jobs` 2, 3, 4 and 7
    /// writes, byte for byte, the files `--jobs 1` writes.**
    ///
    /// [`jobs_changes_the_clock_and_not_one_byte_of_the_output`] holds what the
    /// pool prints, and both of its runs write into one directory, so the files
    /// of one run are overwritten by the next and never compared. Here each job
    /// count writes into a directory of its own and every file is read back.
    /// Through `run(&options, render)`, which is what `tpdf render` runs once
    /// its flags are parsed (the open, the walk, the encoder and the write),
    /// in-process, because no test here may spawn a program.
    ///
    /// `--jobs 1` goes through the same code as every other job count, so a
    /// defect keyed on a page's place in the queue rather than on the thread
    /// that draws it would write the same wrong bytes at every job count, and
    /// comparing job counts with each other could not see it. So one job's
    /// files are first held to each page drawn here, on the test's own
    /// thread, with the options `render` asks the facade for, through the same
    /// encoder; and the pages are first shown to draw differently without
    /// their annotations, the one option `render` sets besides the scale, so
    /// that a page handed other options is a different file.
    ///
    /// Twenty-three pages, which none of 2, 3, 4 and 7 divides.
    #[test]
    fn every_job_count_writes_the_bytes_one_job_writes() {
        const PAGES: u32 = 23;
        let bytes = mixed_pages(PAGES);
        // The process as well as the test in the name, so that two builds of
        // this binary running at once do not empty each other's directory.
        let dir = scratch(&format!("every-job-count-{}", std::process::id()));
        let path = format!("{dir}/doc.pdf");
        std::fs::write(&path, &bytes).expect("the document is written");

        let written = |jobs: usize| -> BTreeMap<String, Vec<u8>> {
            let out = format!("{dir}/jobs-{jobs}");
            run(&render_into(&out, jobs, &path), render)
                .unwrap_or_else(|message| panic!("`--jobs {jobs}`: {message}"));
            std::fs::read_dir(&out)
                .expect("the directory it wrote")
                .map(|entry| {
                    let entry = entry.expect("an entry");
                    let png = std::fs::read(entry.path()).expect("a page it wrote");
                    (entry.file_name().to_string_lossy().into_owned(), png)
                })
                .collect()
        };

        let flags = render_into(&dir, 1, &path);
        let asked = RenderOptions {
            annotations: flags.annotations,
            ..RenderOptions::at_dpi(flags.dpi)
        };
        let doc = Document::open(bytes).expect("it opens");
        let first = doc.page(0).expect("a first page");
        let bare = RenderOptions {
            annotations: false,
            ..asked.clone()
        };
        assert!(
            first.render(&asked).data != first.render(&bare).data,
            "the annotations must show, or a page drawn without them is the same file"
        );
        let expected: BTreeMap<String, Vec<u8>> = (0..PAGES)
            .map(|index| {
                let bitmap = doc.page(index).expect("a page").render(&asked);
                let png = bitmap.to_png().expect("a picture");
                (format!("doc-{:04}.png", index + 1), png)
            })
            .collect();

        let one = written(1);
        assert_eq!(
            one.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>(),
            "`--jobs 1` writes one file for every page, named for it"
        );
        for (name, png) in &expected {
            assert!(
                one[name] == *png,
                "`--jobs 1` wrote {name} as {} bytes, not the {} of the page drawn \
                 outside the pool: {}",
                one[name].len(),
                png.len(),
                parted(&one[name], png)
            );
        }

        for jobs in [2, 3, 4, 7] {
            let pooled = written(jobs);
            assert_eq!(
                pooled.keys().collect::<Vec<_>>(),
                one.keys().collect::<Vec<_>>(),
                "`--jobs {jobs}` writes the files `--jobs 1` writes"
            );
            for (name, png) in &one {
                assert!(
                    pooled[name] == *png,
                    "`--jobs {jobs}` wrote {name} as {} bytes, not the {} `--jobs 1` \
                     wrote: {}",
                    pooled[name].len(),
                    png.len(),
                    parted(&pooled[name], png)
                );
            }
        }

        // Kept when the test fails, for a person to look at; a passing run's
        // directory would only pile up, one per process.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One job unless asked, and a job count of zero is refused.
    ///
    /// The default matters as much as the parse: an existing invocation must
    /// not start spawning threads because a flag it never passes was added.
    /// Zero is an error rather than a clamp because `--jobs 0` is somebody
    /// asking for something, and a run that quietly did the opposite would be
    /// the wrong kind of forgiving.
    #[test]
    fn jobs_defaults_to_one_and_refuses_a_zero() {
        let unasked = Options::parse(&["a.pdf".to_string()]).expect("parses");
        assert_eq!(unasked.jobs, 1, "no flag, no threads");

        let asked = Options::parse(&["--jobs".to_string(), "4".to_string(), "a.pdf".to_string()])
            .expect("parses");
        assert_eq!(asked.jobs, 4);

        for raw in ["0", "-1", "some", ""] {
            let refused =
                Options::parse(&["--jobs".to_string(), raw.to_string(), "a.pdf".to_string()]);
            assert_eq!(
                refused.err().as_deref(),
                Some(format!("`--jobs {raw}` is not a positive number").as_str()),
                "`--jobs {raw}` must be refused"
            );
        }
    }

    /// Plain text unless a format is asked for, and one format at most: two
    /// would be two documents on one stdout with nothing to split them by.
    #[test]
    fn text_takes_one_structured_format_or_none() {
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        let plain = Options::parse(&args(&["a.pdf"])).expect("parses");
        assert_eq!(plain.format, None);
        for (flag, format) in [
            ("--json", TextFormat::Json),
            ("--xml", TextFormat::Xml),
            ("--html", TextFormat::Html),
        ] {
            let asked = Options::parse(&args(&[flag, "a.pdf"])).expect("parses");
            assert_eq!(asked.format, Some(format), "{flag}");
            let twice = Options::parse(&args(&[flag, flag, "a.pdf"])).expect("parses");
            assert_eq!(twice.format, Some(format), "{flag} twice is still {flag}");
        }
        assert_eq!(
            Options::parse(&args(&["--json", "--html", "a.pdf"]))
                .err()
                .as_deref(),
            Some("choose one of --json, --xml and --html")
        );
    }

    /// The stream's order unless another is asked for, by the names
    /// `ReadingOrder` gives them; neither another order nor tables combine
    /// with a structured format, which writes the stream's model, nor with
    /// each other.
    #[test]
    fn text_takes_an_order_or_tables_for_plain_text_only() {
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        let plain = Options::parse(&args(&["a.pdf"])).expect("parses");
        assert_eq!((plain.order, plain.tables), (ReadingOrder::Stream, false));
        for (raw, order) in [
            ("stream", ReadingOrder::Stream),
            ("stated", ReadingOrder::Stated),
            ("inferred", ReadingOrder::Inferred),
        ] {
            let asked = Options::parse(&args(&["--order", raw, "a.pdf"])).expect("parses");
            assert_eq!(asked.order, order, "{raw}");
        }
        assert_eq!(
            Options::parse(&args(&["--order", "geometric", "a.pdf"]))
                .err()
                .as_deref(),
            Some("`--order geometric`: the orders are `stream`, `stated` and `inferred`")
        );
        assert!(
            Options::parse(&args(&["--tables", "a.pdf"]))
                .expect("parses")
                .tables
        );
        for refused in [
            &["--json", "--order", "inferred", "a.pdf"][..],
            &["--tables", "--html", "a.pdf"][..],
        ] {
            assert!(
                Options::parse(&args(refused))
                    .err()
                    .is_some_and(|e| e.contains("are for plain text")),
                "{refused:?}"
            );
        }
        // The stream's order named is the default, and combines with anything.
        assert!(Options::parse(&args(&["--json", "--order", "stream", "a.pdf"])).is_ok());
        assert_eq!(
            Options::parse(&args(&["--tables", "--order", "inferred", "a.pdf"]))
                .err()
                .as_deref(),
            Some("choose one of --order and --tables")
        );
    }

    /// Two columns of twelve lines drawn across the page, line by line, and a
    /// ruled table of two rows by two columns under them, its second row one
    /// cell across both — no rule between them: untagged.
    fn columns_and_a_table() -> Document {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(612.0, 792.0, |page| {
            for row in 0..12 {
                let y = 680.0 - f64::from(row) * 12.0;
                page.text(
                    b"F0",
                    10.0,
                    72.0,
                    y,
                    &format!("left column line {row} of the page"),
                );
                page.text(
                    b"F0",
                    10.0,
                    324.0,
                    y,
                    &format!("right column line {row} of the page"),
                );
            }
            page.raw(
                b"0.5 w 72 500 m 272 500 l S 72 470 m 272 470 l S 72 440 m 272 440 l S\n\
                  72 440 m 72 500 l S 172 470 m 172 500 l S 272 440 m 272 500 l S\n",
            );
            for (x, y, text) in [(76.0, 482.0, "a"), (176.0, 482.0, "b"), (76.0, 452.0, "c")] {
                page.text(b"F0", 10.0, x, y, text);
            }
        });
        Document::open(builder.finish()).expect("it opens")
    }

    /// **`--order inferred` reads down each column and says so**; the stream
    /// order is the one `text` always printed; `--order stated` of an
    /// untagged document is refused; and `--tables` lists the table's cells.
    #[test]
    fn text_prints_the_order_asked_for_and_the_tables() {
        let doc = columns_and_a_table();
        let page = doc.page(0).expect("a page");
        let (stream, label) = page_text(&page, 0, ReadingOrder::Stream).expect("the stream");
        assert_eq!(stream, page.text().plain_text());
        assert_eq!(label, None);

        let (inferred, label) = page_text(&page, 0, ReadingOrder::Inferred).expect("an inference");
        let lines: Vec<&str> = inferred.lines().collect();
        let left = lines
            .iter()
            .position(|l| *l == "left column line 11 of the page")
            .expect("the left column's last line");
        let right = lines
            .iter()
            .position(|l| *l == "right column line 0 of the page")
            .expect("the right column's first line");
        assert!(left < right, "{inferred}");
        let label = label.expect("a label");
        assert!(
            label.starts_with("page 1: an inferred order, 2 columns"),
            "{label}"
        );
        assert!(label.contains("TableSuspected"), "{label}");

        assert_eq!(
            page_text(&page, 0, ReadingOrder::Stated).err().as_deref(),
            Some("page 1: no order is stated: the document carries no structure tree")
        );

        assert_eq!(
            page_tables(&page, 0),
            "page 1, table 1: inferred from the page's rules, 2 rows by 2 columns\n\
             \x20 row 1, column 1: a\n\
             \x20 row 1, column 2: b\n\
             \x20 row 2, column 1-2: c\n"
        );
    }

    /// **`--order inferred` over many pages prints what each page alone
    /// would**: an untagged book whose running heads and page numbers are
    /// found only by reading the pages around each, listed in runs, out of
    /// order and with a page twice, reads through the batch inference, a run
    /// at a time, exactly the text and the label `page_text` gives each page.
    #[test]
    fn inferred_pages_print_what_each_page_alone_prints() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for at in 0..6u32 {
            builder.add_page(612.0, 792.0, |page| {
                let head = if at % 2 == 0 {
                    "A Treatise On Columns"
                } else {
                    "Chapter Two: The Gap"
                };
                page.text(b"F0", 9.0, 72.0, 760.0, head);
                page.text(b"F0", 9.0, 300.0, 30.0, &(at + 1).to_string());
                for row in 0..12 {
                    let y = 680.0 - f64::from(row) * 12.0;
                    for (x, column) in [(72.0, "left"), (324.0, "right")] {
                        page.text(
                            b"F0",
                            10.0,
                            x,
                            y,
                            &format!("{column} column line {row} of page {at}"),
                        );
                    }
                }
            });
        }
        let doc = Document::open(builder.finish()).expect("it opens");
        assert!(doc.structure().is_none(), "the book is untagged");
        let pages = [4, 5, 0, 1, 2, 2, 3];
        let printed: Vec<(String, String)> = inferred_texts(&doc, &pages).collect();
        assert_eq!(printed.len(), pages.len());
        for (index, (text, label)) in pages.iter().zip(&printed) {
            let page = doc.page(*index).expect("a page");
            let alone = page_text(&page, *index, ReadingOrder::Inferred).expect("an order");
            assert_eq!(
                (text, Some(label)),
                (&alone.0, alone.1.as_ref()),
                "page {index}"
            );
        }
        let (_, head) = printed.first().expect("page 5");
        assert!(
            head.starts_with("page 5: an inferred order, 2 columns"),
            "{head}"
        );
        let first = printed[2].0.lines().next();
        assert_eq!(
            first,
            Some("A Treatise On Columns"),
            "the head is read first"
        );
    }

    /// **`--order stated` prints the order the tree states** (reading-order
    /// milestone 1's exit): on a tagged page drawn last paragraph first, it is
    /// `structured_text().plain_text()` — the tree's order, the paragraphs
    /// first to last — and not the stream's, which `text` prints otherwise;
    /// and it says on standard error that the order is the tree's.
    #[test]
    fn text_prints_the_stated_order_of_a_tagged_page() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        let paragraphs = ["first paragraph", "second paragraph", "third paragraph"];
        builder.add_page(612.0, 792.0, |page| {
            for (at, text) in paragraphs.iter().enumerate().rev() {
                let y = 700.0 - 40.0 * at as f64;
                page.tagged_keyed(b"P", at as u64 + 1, at as u64, |page| {
                    page.text(b"F0", 10.0, 72.0, y, text);
                });
            }
        });
        let doc = Document::open(builder.finish()).expect("it opens");
        let page = doc.page(0).expect("a page");
        let structured = page.structured_text().expect("a tagged page");
        let (stated, label) = page_text(&page, 0, ReadingOrder::Stated).expect("a stated order");
        assert_eq!(stated, structured.plain_text());
        assert_eq!(label.as_deref(), Some("page 1: the structure tree's order"));
        let order: Vec<&str> = stated.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(order, paragraphs);
        let (stream, _) = page_text(&page, 0, ReadingOrder::Stream).expect("the stream");
        let drawn: Vec<&str> = stream.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            drawn,
            ["third paragraph", "second paragraph", "first paragraph"]
        );
    }

    /// `--pages` is pages and ranges, 1-based, in the order given; what it
    /// cannot mean is refused rather than guessed at, and it does not combine
    /// with `--page`.
    #[test]
    fn page_lists_parse_in_order_and_refuse_what_they_cannot_mean() {
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        let asked = Options::parse(&args(&["--pages", "5-6,1,2-4", "a.pdf"])).expect("parses");
        assert_eq!(asked.ranges(), Some(vec![(4, 5), (0, 0), (1, 3)]));
        let one = Options::parse(&args(&["--page", "3", "a.pdf"])).expect("parses");
        assert_eq!(
            one.ranges(),
            Some(vec![(2, 2)]),
            "--page N is the range N-N"
        );
        let neither = Options::parse(&args(&["a.pdf"])).expect("parses");
        assert_eq!(neither.ranges(), None, "every page");

        for (raw, why) in [
            ("3-1", "`--pages 3-1`: `3-1` runs backwards"),
            ("0-2", "pages are numbered from 1"),
            ("1,,2", "`--pages 1,,2`: `` is not a page or a range"),
            ("one", "`--pages one`: `one` is not a page or a range"),
            ("1-2-3", "`--pages 1-2-3`: `1-2-3` is not a page or a range"),
        ] {
            assert_eq!(
                Options::parse(&args(&["--pages", raw, "a.pdf"]))
                    .err()
                    .as_deref(),
                Some(why),
                "--pages {raw}"
            );
        }
        assert_eq!(
            Options::parse(&args(&["--page", "1", "--pages", "2", "a.pdf"]))
                .err()
                .as_deref(),
            Some("choose one of --page and --pages")
        );
    }

    /// A reading command cuts a range at the last page rather than counting
    /// out pages the document does not have.
    #[test]
    fn a_reading_command_cuts_a_range_at_the_last_page() {
        let doc = many_pages(3);
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        let huge = Options::parse(&args(&["--pages", "2-4000000000", "a.pdf"])).expect("parses");
        assert_eq!(huge.pages(&doc), vec![1, 2]);
        let past = Options::parse(&args(&["--pages", "7-9,1", "a.pdf"])).expect("parses");
        assert_eq!(
            past.pages(&doc),
            vec![0],
            "a range wholly past the end is empty"
        );
    }

    /// A page that will not write is counted, and every other page still runs.
    ///
    /// `run`'s policy over files, applied to pages. Rendering used to `?` on
    /// the first failed write and abandon the rest, which turns one full disk
    /// into a directory missing everything after it — and the pages are
    /// independent, each writing its own file.
    ///
    /// At **one** job as well as several, and that is not a formality: a
    /// worker that gives up on the queue when its own page fails is invisible
    /// at three jobs, because the other two drain the queue behind it. One
    /// worker is the only arrangement in which giving up loses pages, so it is
    /// the one that has to be checked.
    #[test]
    fn a_page_that_will_not_write_is_counted_and_the_rest_still_render() {
        for jobs in ["1", "3"] {
            let doc = many_pages(4);
            let dir = scratch(&format!("write-failure-{jobs}"));
            // A directory standing exactly where page two's file must go: no
            // `write` on any platform goes through one, which is the cheap
            // stand-in for the full disk this policy exists for.
            std::fs::create_dir_all(format!("{dir}/doc-0002.png")).expect("the blocking directory");

            let options = render_options(&dir, jobs);
            let reports = render_walk(&options, &dir, "doc", &doc);
            assert_eq!(
                reports.len(),
                4,
                "--jobs {jobs}: every page is accounted for"
            );
            assert!(
                reports[1].is_err(),
                "--jobs {jobs}: page 2 cannot write: {:?}",
                reports[1]
            );
            for (slot, report) in reports.iter().enumerate() {
                if slot == 1 {
                    continue;
                }
                assert!(
                    report.is_ok(),
                    "--jobs {jobs}: page {} still renders: {report:?}",
                    slot + 1
                );
            }
            assert!(
                Path::new(&format!("{dir}/doc-0004.png")).exists(),
                "--jobs {jobs}: the pages after the failure were still written"
            );

            // And the count reaches the exit code rather than being printed
            // and forgotten: `run` turns this into a failed file, and `main`
            // into a 1.
            assert_eq!(
                render(&options, "doc.pdf", &doc).err().as_deref(),
                Some("1 page failed"),
                "--jobs {jobs}: the failure must be counted, not swallowed"
            );
        }
    }

    /// `--pdfa` is off unless it is asked for, and asking for it does not
    /// imply `--strict`.
    ///
    /// The two answer different questions — whether the file is a valid PDF,
    /// and whether it is a valid *archival* PDF — and a flag that quietly
    /// turned the other on would make one verdict's exit code depend on the
    /// other's rules.
    #[test]
    fn the_two_validators_are_independent_flags() {
        let neither = Options::parse(&["a.pdf".to_string()]).expect("parses");
        assert!(!neither.strict && !neither.pdfa);

        let archival =
            Options::parse(&["--pdfa".to_string(), "a.pdf".to_string()]).expect("parses");
        assert!(archival.pdfa, "--pdfa asks for it");
        assert!(!archival.strict, "and does not imply --strict");

        let strict =
            Options::parse(&["--strict".to_string(), "a.pdf".to_string()]).expect("parses");
        assert!(strict.strict && !strict.pdfa, "nor the other way round");

        // ISO 14289 is a third question, and the same independence holds.
        let accessible =
            Options::parse(&["--pdfua".to_string(), "a.pdf".to_string()]).expect("parses");
        assert!(accessible.pdfua && !accessible.pdfa && !accessible.strict);
        assert!(!neither.pdfua);

        // ISO 15930 is a fourth, and implies none of the others.
        let print = Options::parse(&["--pdfx".to_string(), "a.pdf".to_string()]).expect("parses");
        assert!(print.pdfx && !print.pdfua && !print.pdfa && !print.strict);
        assert!(!neither.pdfx && !accessible.pdfx);
    }

    /// `check --pdfx` exits by the verdict, as `--pdfa` and `--pdfua` do and
    /// as `docs/features/pdfx.md` says it does: a file that breaks a
    /// requirement of the level it claims is an error, and a file that claims
    /// no level is not. The review of lane 7A found the count printed and
    /// never returned.
    #[test]
    fn check_pdfx_exits_by_the_verdict() {
        let dir = scratch("check-pdfx");
        let write = |name: &str, claim: Option<&str>| {
            let mut builder = DocumentBuilder::new();
            builder.add_base_font(b"F0", b"Helvetica");
            if let Some(claim) = claim {
                builder.set_info(b"GTS_PDFXVersion", claim);
            }
            builder.add_page(200.0, 100.0, |page| {
                page.text(b"F0", 12.0, 10.0, 50.0, "hello");
            });
            let path = format!("{dir}/{name}.pdf");
            std::fs::write(&path, builder.finish()).expect("written");
            path
        };
        let check_pdfx = |path: String| {
            check(
                &Options::parse(&["--pdfx".to_string(), "--quiet".to_string(), path])
                    .expect("parses"),
            )
        };
        // No output intent, no trim box, an unembedded font: anything but
        // clean under PDF/X-1a:2003.
        let claiming = write("claiming", Some("PDF/X-1a:2003"));
        let verdict = check_pdfx(claiming);
        assert!(
            verdict
                .as_ref()
                .is_err_and(|message| message.contains("PDF/X")),
            "{verdict:?}"
        );
        assert_eq!(check_pdfx(write("unclaimed", None)), Ok(()));
    }

    /// A finding prints its clause first, then its object when it has one.
    ///
    /// The clause leads because a person reading findings is checking them
    /// against a standard organised by clause. An object-less finding must not
    /// print a placeholder: a rule about the file as a whole has no object,
    /// and saying so on every such line is noise.
    #[test]
    fn a_conformance_finding_reads_as_a_clause_and_then_its_object() {
        let about_the_file = tinker_pdf::ConformanceFinding {
            clause: tinker_pdf::Clause("6.1.2".to_string()),
            object: None,
            kind: tinker_pdf::FindingKind::MetadataMissing,
        };
        let text = about_the_file.to_string();
        assert!(text.starts_with("6.1.2: "), "{text}");
        assert!(!text.contains("object"), "{text}");

        let about_an_object = tinker_pdf::ConformanceFinding {
            object: Some(tinker_pdf::ObjRef::new(12, 0)),
            ..about_the_file
        };
        assert!(
            about_an_object
                .to_string()
                .starts_with("6.1.2 object 12 0: "),
            "{about_an_object}"
        );
    }

    /// Coverage names the groups that ran rather than counting them.
    ///
    /// "3 of 4" does not tell a caller *which* rules a clean verdict is silent
    /// about, and that is the entire reason the type exists.
    #[test]
    fn coverage_names_the_groups_that_ran() {
        // The list grows as the milestones land — `fonts` joined it at
        // milestone 5 and `structure` when the strict validator was joined
        // under ISO 19005's clauses — and this assertion is written to
        // *notice* that rather than to pin it: a group that quietly appeared
        // here would be a group this tool started claiming to have run without
        // anybody deciding it should. It has noticed twice now.
        assert_eq!(
            tinker_pdf::PdfACoverage::IMPLEMENTED.to_string(),
            "metadata, syntax, structure, fonts, colour"
        );
        assert_eq!(
            tinker_pdf::PdfACoverage::default().to_string(),
            "nothing",
            "a verdict that ran nothing says so rather than printing an empty line"
        );
    }

    /// The listing names every object the table claims, with what it is and
    /// where the table says it lives. A corpus failure is read off this, so an
    /// object silently missing from it is the whole command failing.
    #[test]
    fn the_summary_lists_every_object_with_its_kind() {
        let doc = document();
        let lines = object_lines(doc.cos()).join("\n");

        assert!(lines.contains("/Root"), "the trailer is shown: {lines}");
        assert!(lines.contains("dict /Catalog"), "the catalog: {lines}");
        assert!(lines.contains("dict /Pages"), "the page tree: {lines}");
        assert!(lines.contains("dict /Page "), "a page: {lines}");
        assert!(lines.contains("stream"), "a content stream: {lines}");
        assert!(
            lines.contains("dict /Font /Type1"),
            "and both /Type and /Subtype, which is what tells two fonts \
             apart: {lines}"
        );
    }

    /// Where the object came from is the point: an object the table places in
    /// an object stream and one at a byte offset read the same afterwards, and
    /// which it was is usually the bug.
    #[test]
    fn the_summary_says_where_each_object_lives() {
        let doc = document();
        let lines = object_lines(doc.cos()).join("\n");
        assert!(lines.contains(" at "), "byte offsets are shown: {lines}");
    }

    #[test]
    fn objects_are_written_the_way_the_file_spells_them() {
        let doc = document();
        let cos = doc.cos();
        let catalog = cos.catalog().expect("a catalog");

        let mut text = String::new();
        write_object(cos, &Object::Dict((*catalog).clone()), 0, &mut text);
        assert!(text.starts_with("<<"), "a dictionary: {text}");
        assert!(text.contains("/Type /Catalog"), "with names: {text}");
        assert!(text.contains(" R"), "and indirect references: {text}");
    }

    #[test]
    fn arrays_and_scalars_round_trip_into_readable_syntax() {
        let doc = document();
        let cos = doc.cos();

        let object = Object::Array(vec![
            Object::Int(1),
            Object::Real(2.5),
            Object::Bool(true),
            Object::Null,
            Object::Ref(ObjRef::new(7, 1)),
        ]);
        let mut text = String::new();
        write_object(cos, &object, 0, &mut text);
        assert_eq!(text, "[1 2.5 true null 7 1 R]");
    }

    /// A title in UTF-16 printed as if it were ASCII is mojibake, which reads
    /// like a decoding bug in the engine rather than a display choice here.
    #[test]
    fn unprintable_strings_are_shown_as_hex() {
        assert_eq!(
            show_string(&PdfString::literal(b"hello".to_vec())),
            "(hello)"
        );
        assert_eq!(
            show_string(&PdfString::literal(vec![0xFE, 0xFF, 0x00, 0x41])),
            "<FEFF0041>"
        );
        assert_eq!(
            show_string(&PdfString::literal(br"a(b)c\d".to_vec())),
            r"(a\(b\)c\\d)",
            "and the characters that would end the string early are escaped"
        );
    }

    /// A file whose table promises an object the bytes do not contain is the
    /// case this command exists for. Whatever the ladder decided — trust the
    /// entry, or rescan and drop it — every number still in the table must
    /// appear in the listing, because an object silently missing from the
    /// diagnostic view is indistinguishable from an object that was never
    /// there.
    #[test]
    fn every_object_the_table_still_claims_is_listed() {
        let mut bytes = Vec::from(*b"%PDF-1.7\n");
        let first = bytes.len();
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        let second = bytes.len();
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n");

        let xref_at = bytes.len();
        bytes.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
        bytes.extend_from_slice(format!("{first:010} 00000 n \n").as_bytes());
        bytes.extend_from_slice(format!("{second:010} 00000 n \n").as_bytes());
        // Object 3 is promised at an offset past the end of the file.
        bytes.extend_from_slice(b"0000009999 00000 n \n");
        bytes.extend_from_slice(b"trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n");
        bytes.extend_from_slice(format!("{xref_at}\n%%EOF\n").as_bytes());

        let doc = Document::open(bytes).expect("it opens");
        let cos = doc.cos();
        let lines = object_lines(cos).join("\n");

        for (number, _) in cos.xref().iter() {
            if number > 0 {
                assert!(
                    lines.contains(&format!("    {number:>6} ")),
                    "object {number} is in the table and must be listed: {lines}"
                );
            }
        }
        assert!(lines.contains("dict /Catalog"), "and read: {lines}");
    }

    // ---- Reading on demand (P-05.4) -------------------------------------------

    /// What `info` may read of [`heavy`]'s four megabytes on demand, in
    /// requests and in bytes the source handed over: about twice what it reads
    /// at the tip, seven requests of 6 869 bytes, and under two per cent of
    /// the file.
    const INFO_READS: usize = 16;
    const INFO_BYTES: u64 = 64 << 10;

    /// `bytes` written to a file of this test's, whose path comes back.
    fn on_disk(name: &str, bytes: &[u8]) -> String {
        let path =
            std::env::temp_dir().join(format!("tpdf-demand-{name}-{}.pdf", std::process::id()));
        std::fs::write(&path, bytes).expect("the fixture is written");
        path.to_string_lossy().into_owned()
    }

    /// `pages` pages of about `size` bytes of content each: few objects and
    /// many bytes, as a scan or a drawing is.
    fn heavy(pages: usize, size: usize) -> Vec<u8> {
        let mut content = Vec::with_capacity(size + 16);
        while content.len() < size {
            content.extend_from_slice(b"0 0 1 1 re f\n");
        }
        let mut builder = DocumentBuilder::new();
        for _ in 0..pages {
            builder.add_page(200.0, 100.0, |page| {
                page.raw(&content);
            });
        }
        builder.finish()
    }

    /// A tree of `pages` pages between two fillers of `filler` bytes each, so a
    /// streamed open reads none of it: the head holds the catalog, and the tail
    /// the table.
    fn hollow(pages: usize, filler: usize) -> Vec<u8> {
        let stream = |len: usize| {
            format!(
                "<< /Length {len} >>
stream
{}
endstream",
                "x".repeat(len)
            )
        };
        let kids: Vec<String> = (0..pages).map(|at| format!("{} 0 R", 4 + at)).collect();
        let mut bodies = vec![
            "<< /Type /Catalog /Pages 3 0 R >>".to_string(),
            stream(filler),
            format!(
                "<< /Type /Pages /Count {pages} /Kids [{}] >>",
                kids.join(" ")
            ),
        ];
        for _ in 0..pages {
            bodies.push("<< /Type /Page /Parent 3 0 R /MediaBox [0 0 200 100] >>".to_string());
        }
        bodies.push(stream(filler));
        let mut bytes = b"%PDF-1.7
"
        .to_vec();
        let mut offsets = Vec::new();
        for (at, body) in bodies.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(
                format!(
                    "{} 0 obj
{body}
endobj
",
                    at + 1
                )
                .as_bytes(),
            );
        }
        let table = bytes.len();
        bytes.extend_from_slice(
            format!(
                "xref
0 {}
0000000000 65535 f 
",
                bodies.len() + 1
            )
            .as_bytes(),
        );
        for offset in &offsets {
            bytes.extend_from_slice(
                format!(
                    "{offset:010} 00000 n 
"
                )
                .as_bytes(),
            );
        }
        bytes.extend_from_slice(
            format!(
                "trailer
<< /Size {} /Root 1 0 R >>
startxref
{table}
%%EOF
",
                bodies.len() + 1
            )
            .as_bytes(),
        );
        bytes
    }

    /// A catalog and an empty tree whose table's second entry points into the
    /// first object: a buffered open patches it at open, a streamed one finds
    /// out at the read.
    fn damaged() -> Vec<u8> {
        let mut bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
xref\n0 3\n\
0000000000 65535 f \n\
0000000009 00000 n \n\
0000000058 00000 n \n\
trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n110\n%%EOF\n"
            .to_vec();
        let table = bytes
            .windows(9)
            .position(|w| w == b"xref\n0 3\n")
            .expect("a classic table");
        let entry = table + 9 + 2 * 20;
        bytes[entry..entry + 10].copy_from_slice(b"0000000004");
        bytes
    }

    /// A file the strict pass declines says which repairs it needed, each
    /// kind once with the object it was raised on: a file that stops being
    /// eligible between two revisions then names what it started needing.
    #[test]
    fn an_ineligible_file_names_the_repairs_it_needed() {
        let doc = Document::open(damaged()).expect("the table is repaired");
        let warnings = doc.warnings();
        assert!(!warnings.is_empty(), "the patched entry is a repair");
        let mut out = Vec::new();
        strict(&mut out, &doc);
        let text = String::from_utf8(out).expect("utf-8");
        let line = text.lines().next().expect("a line");
        assert!(line.starts_with("strict ineligible the file "), "{line}");
        let (_, named) = line.split_once(": ").expect("the repairs named");
        let mut first: Vec<(String, Option<ObjRef>)> = Vec::new();
        for warning in &warnings {
            let kind = repair_label(warning);
            if !first.iter().any(|(known, _)| *known == kind) {
                first.push((kind, warning.object));
            }
        }
        for (kind, object) in first.iter().take(REPAIRS_NAMED) {
            let expected = match object {
                Some(object) => format!("{kind} in object {} {}", object.num, object.gen),
                None => kind.clone(),
            };
            assert!(named.contains(&expected), "{expected} in {line}");
        }
    }

    /// A filter's repair is named by its own leniency, a kind with fields
    /// by its name alone, and a value in parentheses not at all.
    #[test]
    fn a_repair_is_named_by_its_variants_and_never_its_values() {
        assert_eq!(
            variant_names("Filter(AdlerMismatch)"),
            "Filter(AdlerMismatch)"
        );
        assert_eq!(
            variant_names("Filter(Predictor(ShortRow))"),
            "Filter(Predictor(ShortRow))"
        );
        assert_eq!(variant_names("XrefRepaired { at: 5 }"), "XrefRepaired");
        assert_eq!(variant_names("Truncated(5)"), "Truncated");
        assert_eq!(variant_names("PageTreeCycle"), "PageTreeCycle");
        assert_eq!(variant_names(""), "Unknown");
    }

    /// Past four kinds, the rest are counted rather than named.
    #[test]
    fn a_reason_names_four_repairs_and_counts_the_rest() {
        let warnings: Vec<tinker_pdf::Warning> = Document::open(damaged())
            .expect("it opens")
            .warnings()
            .into_iter()
            .take(1)
            .collect();
        let one = repairs(&warnings);
        assert!(!one.contains("more"), "{one}");
        let kinds = [
            tinker_pdf::WarningKind::PageTreeCycle,
            tinker_pdf::WarningKind::PageTreeTruncated,
            tinker_pdf::WarningKind::MediaBoxMissing,
            tinker_pdf::WarningKind::PageCountMismatch,
            tinker_pdf::WarningKind::TreeCycle,
            tinker_pdf::WarningKind::PageTreeTruncated,
        ];
        let many: Vec<tinker_pdf::Warning> = kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| tinker_pdf::Warning {
                offset: index as u64,
                object: Some(ObjRef::new(index as u32 + 1, 0)),
                kind: *kind,
            })
            .collect();
        let named = repairs(&many);
        assert_eq!(named.matches(" in object ").count(), 4, "{named}");
        assert!(named.ends_with(", 1 more"), "{named}");
        assert!(
            named.starts_with("PageTreeCycle in object 1 0, "),
            "{named}"
        );
    }

    /// A page of text behind bytes that are not its header: a buffered open
    /// shifts every entry, and a streamed one would repair each at its first
    /// read.
    fn shifted() -> Vec<u8> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "on demand");
        });
        [
            b"leading bytes, not a header\n".as_slice(),
            &builder.finish(),
        ]
        .concat()
    }

    /// Three pages saved linearized, the root `/Pages` node carrying `/Rotate
    /// 90` when `rotated`, so that page one inherits it.
    fn linearized(rotated: bool) -> Vec<u8> {
        let mut builder = DocumentBuilder::new();
        for _ in 0..3 {
            builder.add_page(200.0, 100.0, |page| {
                page.raw(b"0 0 10 10 re f");
            });
        }
        let doc = Document::open(builder.finish()).expect("it opens");
        let mut editor = doc.editor();
        if rotated {
            let pages = editor.intern(b"Pages");
            let root = editor
                .catalog()
                .and_then(|catalog| catalog.get_ref(pages))
                .expect("a page tree");
            let Some(Object::Dict(mut tree)) = editor.get(root) else {
                panic!("the page tree is a dictionary");
            };
            tree.insert(editor.intern(b"Rotate"), Object::Int(90));
            editor.put(root, Object::Dict(tree));
        }
        editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            linearize: true,
            ..WriteOptions::default()
        })
    }

    /// [`linearized`] with an update appended, which leaves the length its
    /// linearization states behind.
    fn updated() -> Vec<u8> {
        let doc = Document::open(linearized(false)).expect("it opens");
        let mut editor = doc.editor();
        let _ = editor.set_title("updated");
        editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        })
    }

    /// **On demand, the buffer's lines.** Over four megabytes of four heavy
    /// pages, which the rule keeps streamed, `info` prints what a buffered open
    /// prints, and its reads, counted through the file source, stay inside a
    /// budget of a small share of the file.
    #[test]
    fn info_reads_a_large_file_on_demand_and_says_what_a_buffer_says() {
        let bytes = heavy(4, 1 << 20);
        let path = on_disk("large", &bytes);
        let file = Arc::new(source::FileSource::open(&path).expect("it opens"));
        let counting = Arc::new(tinker_pdf::CountingSource::new(Arc::clone(&file)));
        let streamed = source::open_by_rule(
            &path,
            Arc::clone(&counting) as Arc<dyn tinker_pdf::ByteSource>,
        )
        .expect("it opens");
        assert!(
            streamed.cos().is_streamed(),
            "the rule keeps a file a buffer reads the same"
        );
        let lines = info_lines(&path, &streamed);
        assert_eq!(file.failure(), None, "every read was made");
        let buffered = Document::open(bytes.clone()).expect("it opens");
        assert_eq!(lines, info_lines(&path, &buffered));
        let (reads, read) = (counting.reads(), counting.bytes_read());
        assert!(
            reads <= INFO_READS && read <= INFO_BYTES,
            "info read {read} bytes in {reads} reads of a {}-byte file",
            bytes.len()
        );
        println!(
            "RAN info on demand: {read} bytes in {reads} reads of a {}-byte file",
            bytes.len()
        );
        let _ = std::fs::remove_file(&path);
    }

    /// **The rule reads whole where a stream would answer otherwise.** A
    /// damaged file, which a buffered open patches; one behind leading bytes,
    /// whose every entry a buffered open shifts; a linearized file updated
    /// since, whose length is not the one it states; and a linearized file
    /// whose first page inherits `/Rotate`, which Annex F's head would build
    /// without it. On demand, `info` and `check` print a buffered open's lines
    /// for each, and the rotated page renders as a buffered open renders it.
    #[test]
    fn the_rule_reads_whole_where_a_stream_would_answer_otherwise() {
        let rotated = linearized(true);
        let fixtures = [
            ("damaged", damaged()),
            ("shifted", shifted()),
            ("updated", updated()),
            ("rotated", rotated.clone()),
        ];
        for (name, bytes) in fixtures {
            let path = on_disk(name, &bytes);
            let (demand, kept) = open(&path, Reads::OnDemand, None, None).expect("it opens");
            assert!(kept.is_none(), "{name}: read whole");
            let buffered = Document::open(bytes).expect("it opens");
            assert_eq!(
                info_lines(&path, &demand),
                info_lines(&path, &buffered),
                "{name}: info"
            );
            assert_eq!(
                checked(&path, &demand).0,
                checked(&path, &buffered).0,
                "{name}: check"
            );
            if name == "rotated" {
                let page = buffered.page(0).expect("a first page");
                assert_eq!(page.rotation(), 90, "page one inherits its tree's turn");
                let streamed = Document::open_streaming(Arc::new(tinker_pdf::SliceSource::new(
                    rotated.clone(),
                )))
                .expect("it opens streamed");
                assert!(
                    streamed.first_page_end().is_some(),
                    "a streamed open would answer page one from the head"
                );
                assert_eq!(
                    demand
                        .page(0)
                        .expect("a first page")
                        .render(&RenderOptions::default())
                        .data,
                    page.render(&RenderOptions::default()).data,
                    "the turned page renders as the buffer renders it"
                );
            }
            let _ = std::fs::remove_file(&path);
        }
    }

    /// **A failed read fails the file before it prints.** A file the rule
    /// keeps streamed shrinks after it opened, so the tree `info` counts, which
    /// the open never read, is no longer there to read: `info` fails the file,
    /// naming the bytes, and returns before it prints a line.
    #[test]
    fn a_failed_read_fails_the_file_before_it_prints() {
        let bytes = hollow(8, 64 << 10);
        let path = on_disk("shrinks", &bytes);
        let (doc, kept) = open(&path, Reads::OnDemand, None, None).expect("it opens");
        let kept = kept.expect("the rule keeps the file streamed");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_len(16 << 10))
            .expect("the file shrinks");
        let printed = reading(&path, Some(kept), || info_report(&path, &doc));
        let message = printed.expect_err("the file fails");
        assert!(
            message.starts_with(&format!("reading {path}: ")) && message.contains(" at bytes "),
            "{message}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// **A verdict flag reads the whole file.** `check --strict` opens the
    /// linearized file updated since from one buffer, and reports the defects
    /// a buffered open's validation reports.
    #[test]
    fn a_verdict_flag_reads_the_whole_file() {
        let bytes = updated();
        let path = on_disk("strict", &bytes);
        let options =
            Options::parse(&["--strict".to_string(), path.clone()]).expect("the options parse");
        assert_eq!(check_reads(&options), Reads::Whole);
        let (doc, kept) = open(&path, check_reads(&options), None, None).expect("it opens");
        assert!(kept.is_none() && !doc.cos().is_streamed(), "one buffer");
        let buffered = Document::open(bytes).expect("it opens");
        let defects = |doc: &Document| -> Vec<String> {
            doc.validate().iter().map(ToString::to_string).collect()
        };
        assert_eq!(defects(&doc), defects(&buffered));
        assert_eq!(checked(&path, &doc).0, checked(&path, &buffered).0);
        let _ = std::fs::remove_file(&path);
    }
}
