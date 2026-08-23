//! `tpdf` — the engine's command-line front end.
//!
//! It exists for two reasons. It is how a person looks at what the engine
//! thinks of a file without writing Rust, which is most of debugging a corpus
//! failure; and it is the thing a corpus runner invokes, so every capability
//! the runner needs has to be reachable from here.
//!
//! Argument parsing is hand-rolled along with everything else. It is a
//! sub-command plus flags, which needs no library.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use tinker_pdf::{
    Bitmap, CosDocument, Dict, Document, LadderLevel, ObjRef, Object, Page, RenderOptions,
    SimpleFontProvider, StreamObj, Tier, WriteMode, WriteOptions, XrefEntry,
};

const USAGE: &str = "\
tpdf — inspect and convert PDFs with the tinker-pdf engine

usage:
  tpdf info    <file.pdf> [--password P]
  tpdf text    <file.pdf> [--page N] [--password P]
  tpdf render  <file.pdf> --out DIR [--page N] [--dpi D] [--no-annotations]
  tpdf fields  <file.pdf> [--password P]
  tpdf outline <file.pdf> [--password P]
  tpdf objects <file.pdf> [--object N [--stream [--raw]]] [--password P]
  tpdf check   <file.pdf>... [--strict]
  tpdf probe   <file.pdf>... [--dpi D] [--fonts PATH]

options:
  --page N     one page, 1-based; the default is every page
  --object N   one object, by number; the default is a summary of all
  --stream     with --object, write that object's stream data to stdout
  --raw        with --stream, before the filters rather than after
  --dpi D      resolution for render (default 150)
  --out DIR    where render writes its PNMs
  --fonts PATH a face, or a directory of faces, for documents that embed none
  --password P the password to open an encrypted file with
  --quiet      only report failures
  --strict     with check, also validate against ISO 32000 strictly

`check` opens each file and reports its warnings, exiting non-zero if any
file failed to open at all. It never renders, so it is the fast pass over a
corpus.

`--strict` adds the validator of ruling 13: the file is read again with the
leniency ladder off and held to the structures a tolerant read never consults
-- the cross-reference sections as the bytes spell them, stream extents
against `endstream`, the trailer against Table 15. Any defect exits non-zero,
because the question `--strict` asks is not whether it opened but whether it
is right.

`probe` is the one the corpus runner spawns, one child process per file. It
opens the file, renders every page, rewrites it and validates the rewrite, and
writes a line-oriented record of what happened to stdout, ending in `done`. That last line is the whole point: a
child that panicked, aborted or was killed for hanging leaves a record with no
`done` in it, so the runner can tell a file that failed from a file that never
finished, which a summary line printed at the end could not.

It exits 0 whenever it finished, including for a file that would not open — a
file that fails to open is a result to be counted, not an error in the tool,
and an exit code that conflated them would make every unopenable file look
like a crashed run.

`objects` is the view underneath every other command: what the engine
actually parsed, object by object. It is what a corpus failure gets looked
at with, which is why it prints the cross-reference kind alongside each
object — an object the table says lives in an object stream and an object
found by the repair scanner read the same afterwards, and the difference
is usually the bug.
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
        "outline" => run(&options, outline),
        "objects" => run(&options, objects),
        "check" => check(&options),
        "probe" => probe(&options),
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
    out: Option<String>,
    fonts: Option<String>,
    password: Option<String>,
    annotations: bool,
    quiet: bool,
    raw: bool,
    stream: bool,
    strict: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let mut options = Options {
            files: Vec::new(),
            page: None,
            object: None,
            dpi: 150.0,
            out: None,
            fonts: None,
            password: None,
            annotations: true,
            quiet: false,
            raw: false,
            stream: false,
            strict: false,
        };

        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
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
                _ if arg.starts_with("--") => return Err(format!("unknown option `{arg}`")),
                _ => options.files.push(arg.to_string()),
            }
            index += 1;
        }

        if options.files.is_empty() {
            return Err("no input file".to_string());
        }
        Ok(options)
    }

    /// The pages to act on: the one asked for, or all of them.
    fn pages(&self, doc: &Document) -> Vec<u32> {
        match self.page {
            Some(n) => vec![n],
            None => (0..doc.page_count()).collect(),
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

fn open(
    path: &str,
    password: Option<&str>,
    fonts: Option<&Arc<SimpleFontProvider>>,
) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading {path}: {e}"))?;
    let doc = Document::open(bytes).map_err(|e| format!("{path}: {e:?}"))?;
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

/// Runs a command over every file given.
///
/// One unreadable file reports itself and the rest still run: a command given
/// a whole directory is the normal case, and stopping at the first
/// password-protected file would hide everything after it.
fn run(
    options: &Options,
    each: fn(&Options, &str, &Document) -> Result<(), String>,
) -> Result<(), String> {
    let fonts = options.font_provider()?;
    let mut failed = 0usize;
    for path in &options.files {
        let outcome = open(path, options.password.as_deref(), fonts.as_ref())
            .and_then(|doc| each(options, path, &doc));
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
    let metadata = doc.metadata();
    println!("{path}");
    println!("  pages       {}", doc.page_count());
    println!("  version     {}", doc.pdf_version());
    println!("  ladder      {:?}", doc.ladder_level());
    println!("  encrypted   {}", doc.is_encrypted());
    if doc.is_encrypted() {
        println!("  authorized  {:?}", doc.auth_level());
        let permissions = doc.permissions();
        println!(
            "  may         print={} copy={} modify={} annotate={}",
            permissions.print(),
            permissions.copy(),
            permissions.modify(),
            permissions.annotate()
        );
    }

    for (label, value) in [
        ("title", &metadata.title),
        ("author", &metadata.author),
        ("subject", &metadata.subject),
        ("creator", &metadata.creator),
        ("producer", &metadata.producer),
    ] {
        if let Some(value) = value {
            println!("  {label:11} {value}");
        }
    }

    // Printed only when the document names it: /Trapped absent and /Trapped
    // /Unknown are different statements, and a line that appeared either way
    // would collapse them back together.
    if let Some(trapped) = metadata.trapped {
        println!("  {:11} {trapped:?}", "trapped");
    }

    if let Some(page) = doc.page(0) {
        let (w, h) = page.size();
        println!("  first page  {w} x {h} pt, rotated {}", page.rotation());
    }

    let warnings = doc.warnings();
    if !warnings.is_empty() {
        println!("  warnings    {}", warnings.len());
        for warning in warnings.iter().take(10) {
            println!("    {:?}", warning.kind);
        }
        if warnings.len() > 10 {
            println!("    ... and {} more", warnings.len() - 10);
        }
    }
    Ok(())
}

fn text(options: &Options, _path: &str, doc: &Document) -> Result<(), String> {
    for index in options.pages(doc) {
        let Some(page) = doc.page(index) else {
            continue;
        };
        print!("{}", page.text().plain_text());
        // A form feed between pages, which is what every other text extractor
        // emits and what makes the output splittable again.
        if options.page.is_none() {
            print!("\u{c}");
        }
    }
    Ok(())
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

    for index in options.pages(doc) {
        let Some(page) = doc.page(index) else {
            continue;
        };
        let bitmap = page.render(&RenderOptions {
            annotations: options.annotations,
            ..RenderOptions::at_dpi(options.dpi)
        });

        let out = format!("{dir}/{stem}-{:04}.pnm", index + 1);
        write_pnm(&out, &bitmap)?;
        if !options.quiet {
            println!("{out} {}x{}", bitmap.width, bitmap.height);
            for warning in &bitmap.warnings {
                println!("  {warning:?}");
            }
        }
    }
    Ok(())
}

/// Writes a binary PNM, which needs no encoder and which every image tool
/// reads. A PNG writer would mean a deflate encoder in a test tool, and the
/// engine already has one it should not depend on from here.
fn write_pnm(path: &str, bitmap: &tinker_pdf::Bitmap) -> Result<(), String> {
    let components = bitmap.components();
    let (magic, out_components) = match components {
        1 | 2 => ("P5", 1),
        _ => ("P6", 3),
    };

    let mut out = format!("{magic}\n{} {}\n255\n", bitmap.width, bitmap.height).into_bytes();
    for y in 0..bitmap.height as usize {
        let row = y * bitmap.stride;
        for x in 0..bitmap.width as usize {
            let at = row + x * components;
            let Some(pixel) = bitmap.data.get(at..at + components) else {
                continue;
            };
            // Alpha is dropped rather than composited: these are debugging
            // images, and a surprising background would mislead more than a
            // missing one.
            out.extend_from_slice(&pixel[..out_components.min(pixel.len())]);
        }
    }

    std::fs::write(path, out).map_err(|e| format!("writing {path}: {e}"))
}

fn fields(_options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    let found = doc.form_fields();
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

fn outline(_options: &Options, path: &str, doc: &Document) -> Result<(), String> {
    let items = doc.outline();
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
fn check(options: &Options) -> Result<(), String> {
    let fonts = options.font_provider()?;
    let mut failed = 0usize;
    let mut warned = 0usize;
    let mut invalid = 0usize;

    for path in &options.files {
        match open(path, options.password.as_deref(), fonts.as_ref()) {
            Ok(doc) => {
                let warnings = doc.warnings();
                let pages = doc.page_count();
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
                    println!(
                        "ok    {path}  {pages} pages, {:?}, {} warnings",
                        doc.ladder_level(),
                        warnings.len()
                    );
                }
                for defect in &defects {
                    println!("      {} {defect}", defect.kind.tier().as_str());
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
    if failed > 0 {
        return Err(format!("{failed} files could not be opened"));
    }
    if invalid > 0 {
        return Err(format!("{invalid} files did not validate"));
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
/// One percent sits above the noise and far below anything structural: a
/// rotation applied to the geometry and not to the clip, or to the text and not
/// to the images, moves whole regions rather than the rims of glyphs.
const ROTATE_BUDGET: f64 = 0.01;

/// How long a file may already have taken before its relations are skipped.
///
/// The relations cost roughly what opening and rendering the first page cost,
/// twice over, so a file already well into the corpus runner's twenty-second
/// budget is one where asking them risks the timeout — and a timeout would
/// move the *pass* rate, which is a measurement this one must not disturb.
///
/// **This is a clock, and a clock decides a number the ratchet compares.**
/// That is worth saying plainly rather than burying: `compared` is the
/// denominator of the metamorphic rate, so a file sitting near this line can
/// be asked on one run and declined on the next, and the bar moves by one file
/// for no reason anybody changed. It happened: a run recorded `dpi` at 572 of
/// 579 and the next said 572 of 580.
///
/// Nothing deterministic replaces it. The `cost` line beside this reports the
/// three properties of a document that ought to bound the work — bytes,
/// objects and first-page pixels — and measured against the corpus they do
/// not: `qpdf/numeric-and-string-2.pdf` is 16 KB with 22 objects and takes 4.9
/// seconds, while files a hundred times its size take a tenth of that.
///
/// So the line is **sited** rather than chosen: 3 100 ms is the middle of the
/// widest gap in the measured distribution. Every corpus file between one and
/// six seconds was timed (4 525 files, 72 dpi, August 2026); the slowest
/// admitted is 2 885 ms and the fastest declined is 3 385 ms, so the nearest
/// file either side is 7 % away rather than the 3 % that two seconds gave.
/// Moving this number re-records the bar, which is a commit somebody reviews.
const META_BUDGET_MS: u64 = 3_100;

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

/// One relation's verdict, in the record's own words.
enum Relation {
    /// The relation held.
    Held,
    /// It did not, and this is what was measured.
    Broke(String),
    /// It could not be asked — an empty page, a page too large to render
    /// twice, a rewrite that would not reopen.
    Skipped(&'static str),
}

impl Relation {
    fn print(&self, name: &str) {
        match self {
            Relation::Held => println!("meta {name} held"),
            Relation::Broke(detail) => println!("meta {name} broke {}", one_line(detail)),
            Relation::Skipped(why) => println!("meta {name} skipped {why}"),
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
/// page of a four-thousand-file corpus would turn a twenty-second timeout into
/// the thing being measured.
fn metamorphic(doc: &Document, options: &Options, spent: std::time::Duration) {
    // **The relations may not cost the file its outcome**, and this line is
    // there because they did. Each one re-renders the first page and two of
    // them save and reopen the document, so on a slow file the extra work ran
    // the corpus runner's twenty-second timeout out: the first recorded run
    // turned three pdf.js files that had always passed into timeouts, and
    // wrote that in as the new bar.
    //
    // A file that has already spent this much of its budget opening and
    // rendering is one whose relations are *not asked*, which the record says
    // in its own words. The alternative — a longer timeout — would change what
    // the pass rate means, and the pass rate is a different measurement that
    // was here first.
    if spent > std::time::Duration::from_millis(META_BUDGET_MS) {
        for name in ["rotate", "crop", "dpi"] {
            Relation::Skipped("the file spent its budget opening and rendering").print(name);
        }
        return;
    }
    if doc.page_count() == 0 {
        for name in ["rotate", "crop", "dpi"] {
            Relation::Skipped("the document has no pages").print(name);
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
    // A page that came back empty is a page the relations cannot speak about:
    // every one of them holds trivially over nothing.
    if base.width == 0 || base.height == 0 {
        for name in ["rotate", "crop", "dpi"] {
            Relation::Skipped("the page rendered to nothing").print(name);
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
            println!("phase meta-rotate");
            rotation(doc, &base, &render).print("rotate");
            println!("phase meta-crop");
            cropping(doc, &page, &base, &render).print("crop");
        }
        Some(why) => {
            Relation::Skipped(why).print("rotate");
            Relation::Skipped(why).print("crop");
        }
    }
    // `dpi` rewrites nothing, so it is asked of every document that rendered.
    println!("phase meta-dpi");
    resolution(&page, &base, &render).print("dpi");
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
fn rotation(doc: &Document, base: &Bitmap, render: &RenderOptions) -> Relation {
    let mut editor = doc.editor();
    if !editor.rotate_page(0, 90) {
        return Relation::Skipped("the page would not rotate");
    }
    let Ok(turned) = Document::open(editor.save(&WriteOptions::default())) else {
        return Relation::Skipped("the rotated document would not reopen");
    };
    let Some(page) = turned.page(0) else {
        return Relation::Skipped("the rotated document lost its page");
    };
    let rotated = page.render(render);

    if rotated.width != base.height || rotated.height != base.width {
        return Relation::Broke(format!(
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
    let total = u64::from(rotated.width) * u64::from(rotated.height);
    if share(moved, total) <= ROTATE_BUDGET {
        Relation::Held
    } else {
        Relation::Broke(format!(
            "{moved} of {total} pixels ({:.1}%) are not the transposition, over a              budget of {:.1}%",
            share(moved, total) * 100.0,
            ROTATE_BUDGET * 100.0
        ))
    }
}

/// **A cropped render is the sub-rectangle of the full one** (ruling 5's tile
/// equality, generalised from a tile to the page box).
///
/// The crop box is the middle half of the page, in whole pixels at the scale
/// being rendered, so the comparison needs no resampling: every pixel of the
/// cropped render has a pixel of the full one it must equal exactly.
fn cropping(doc: &Document, page: &Page, base: &Bitmap, render: &RenderOptions) -> Relation {
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

    let mut editor = doc.editor();
    if !editor.set_crop_box(0, x0 + inset_x, y0 + inset_y, x1 - inset_x, y1 - inset_y) {
        return Relation::Skipped("the crop box would not be set");
    }
    let Ok(cropped) = Document::open(editor.save(&WriteOptions::default())) else {
        return Relation::Skipped("the cropped document would not reopen");
    };
    let Some(page) = cropped.page(0) else {
        return Relation::Skipped("the cropped document lost its page");
    };
    let small = page.render(render);
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
    if moved == 0 {
        Relation::Held
    } else {
        let total = u64::from(small.width) * u64::from(small.height);
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
fn resolution(page: &Page, base: &Bitmap, render: &RenderOptions) -> Relation {
    // A page big enough that rendering it twice over is the corpus runner's
    // whole budget is one this relation declines rather than times out on.
    if u64::from(base.width) * u64::from(base.height) > 4_000_000 {
        return Relation::Skipped("the page is too large to render twice");
    }
    let doubled = page.render(&RenderOptions {
        scale: render.scale * 2.0,
        ..render.clone()
    });
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
        Relation::Held
    } else {
        Relation::Broke(format!(
            "{moved} of {total} pixels ({:.1}%) differ, over a budget of {:.1}%",
            share(moved, total) * 100.0,
            DPI_BUDGET * 100.0
        ))
    }
}

// ---- probe: the record one corpus child writes ---------------------------

/// The record's format version, bumped when a reader would misread the old
/// shape. The runner refuses a record whose version it does not know rather
/// than reading the fields it recognises and inventing the rest.
const PROBE_VERSION: u32 = 3;

/// Opens and renders one file at a time, writing a record per file.
///
/// Never returns `Err` for anything the file did: the runner reads outcomes
/// from the record, and reserves the exit code for "this process did not
/// finish", which is the one thing a record cannot say about itself.
fn probe(options: &Options) -> Result<(), String> {
    let fonts = options.font_provider()?;
    for path in &options.files {
        probe_one(options, path, fonts.as_ref());
    }
    Ok(())
}

fn probe_one(options: &Options, path: &str, fonts: Option<&Arc<SimpleFontProvider>>) {
    let started = std::time::Instant::now();
    println!("probe {PROBE_VERSION}");
    println!("file {path}");

    // `phase` lines are the corpus runner's progress signal, and the reason
    // they work needs no protocol: Rust's stdout is a `LineWriter`, so each
    // one reaches the capture file when it is printed. A child killed at the
    // timeout having printed nothing for half of it was not making progress,
    // and the runner reports that as `stalled` rather than as a slow file.
    // `parse_record` ignores keys it does not know, so these cost an older
    // runner nothing.
    println!("phase open");
    let doc = match open(path, options.password.as_deref(), fonts) {
        Ok(doc) => doc,
        Err(message) => {
            // On one line, and last but for the sentinel, so a reason
            // containing anything at all cannot be mistaken for another key.
            println!("opened no {}", one_line(&message));
            // Said in its own words rather than left out: a record that simply
            // omits the strict pass reads as a child that skipped it, and "the
            // file did not open" is a different fact from "this build does not
            // run the pass".
            println!("strict ineligible the file did not open");
            println!("ms {}", started.elapsed().as_millis());
            println!("done");
            return;
        }
    };

    println!("opened yes");
    println!("ladder {:?}", doc.ladder_level());
    let pages = doc.page_count();
    println!("pages {pages}");

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
        println!("cap {capability}");
    }

    let render = RenderOptions {
        annotations: options.annotations,
        ..RenderOptions::at_dpi(options.dpi)
    };
    println!("phase render");
    let mut rendered = 0u32;
    for index in options.pages(&doc) {
        let Some(page) = doc.page(index) else {
            continue;
        };
        // Before the page rather than after it, so the line names the page
        // being worked on when a kill arrives rather than the last one that
        // finished. The runner never reads the number, only the fact that the
        // capture file grew; the number is for whoever reads the report.
        println!("page {}/{pages}", index + 1);
        let bitmap = page.render(&render);
        // Ruling 2's definition, and the one the ratchet counts: a bitmap
        // came back. A page rendered with a JBIG2 placeholder on it rendered.
        rendered += 1;
        for warning in &bitmap.warnings {
            *kinds
                .entry(format!("render:{}", render_warning_label(warning)))
                .or_default() += 1;
        }
    }
    println!("rendered {rendered}");

    // What this document costs to work on, as properties of the document.
    // Read by nothing yet; measured so the metamorphic gate can stop being a
    // clock. See `META_BUDGET_MS`.
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
    println!("cost bytes {bytes} objects {objects} pixels {pixels}");

    println!("phase strict");
    strict(&doc);
    metamorphic(&doc, options, started.elapsed());

    for (kind, count) in &kinds {
        println!("warn {kind} {count}");
    }
    println!("ms {}", started.elapsed().as_millis());
    println!("done");
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

fn strict(doc: &Document) {
    if let Some(why) = cleanly_read(doc) {
        println!("strict ineligible {why}");
        return;
    }

    println!("strict eligible");
    let saved = doc.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    let Ok(again) = Document::open(saved) else {
        // Not an ineligibility: a rewrite this engine cannot re-open is the
        // worst defect on this axis, and filing it as "not measured" would
        // make the number go up by hiding it.
        println!("strict structure 1");
        println!("strict semantics 0");
        println!("strict kind rewrite-did-not-open 1");
        return;
    };

    let defects = again.validate();
    let structure = defects
        .iter()
        .filter(|defect| defect.kind.tier() == Tier::Structure)
        .count();
    println!("strict structure {structure}");
    println!(
        "strict semantics {}",
        defects.len().saturating_sub(structure)
    );
    // By kind rather than one line per defect: a damaged source can produce
    // thousands of the same finding, and the report wants to know which rules
    // a corpus breaks.
    for (kind, count) in tinker_pdf::kind_counts(&defects) {
        println!("strict kind {kind} {count}");
    }
}

/// Anything that would break the one-record-per-line format, flattened.
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
        W::PageScaledDown { .. } => "PageScaledDown".to_string(),
        W::EmptyTextClip => "EmptyTextClip".to_string(),
        W::UnreadableFont => "UnreadableFont".to_string(),
        W::UnsupportedShading { kind } => format!("UnsupportedShading({kind})"),
        W::UnsupportedPattern { .. } => "UnsupportedPattern".to_string(),
        W::HiddenOptionalContent { .. } => "HiddenOptionalContent".to_string(),
        W::GroupBudgetSpent { .. } => "GroupBudgetSpent".to_string(),
        W::Cancelled => "Cancelled".to_string(),
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
    use tinker_pdf::{DocumentBuilder, PdfString};

    fn document() -> Document {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.set_info(b"Title", "a document");
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "hello");
        });
        Document::open(builder.finish()).expect("it opens")
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
}
