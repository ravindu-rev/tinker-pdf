//! The write half: `merge`, `split`, `rotate`, `encrypt`, `decrypt`,
//! `attach`, `stamp` and `sanitise` — the commands that write a document out
//! again.
//!
//! Each is a wrapper over the facade with no logic of its own (ruling 11). It
//! opens its inputs, makes the editor calls its name says —
//! [`DocumentEditor::import_page`], [`DocumentEditor::keep_pages`],
//! [`DocumentEditor::rotate_page`], [`DocumentEditor::attach_file`],
//! [`DocumentEditor::import_page_as_form`] with [`DocumentEditor::stamp`],
//! [`DocumentEditor::sanitise`], or none at all for the two that only change
//! the cipher — and saves through [`tinker_pdf::write::save`], the facade's
//! save door. That door is the one that carries the font policy, so **every
//! one of these takes `--font-policy`**, and subsets by default because the
//! facade does: the caller who needed the embedded programs cut down to the
//! glyphs the document still draws is the caller who would have forgotten to
//! ask. What the pass did is printed with every save, a program left whole
//! named with its reason, because a subsetting pass that left one face whole
//! left every outline in it in the file (ruling 10).
//!
//! The same door carries the image policy, and so every command takes that
//! too — `--images`, `--bilevel` and `--max-ppi`, [`image_policy`] — off
//! unless one is given, as the facade's [`ImagePolicy::Keep`] is.
//!
//! What each command prints is built rather than printed, so a test can hold
//! it; and each test reopens what was written through the facade and asks it,
//! rather than spawning this binary (ruling 13, `cargo xtask oracles`).
//!
//! # Every write is a rewrite
//!
//! [`WriteMode::Rewrite`], always, with no flag for an incremental update.
//! An update appends (7.5.6), so the original bytes — the pages a split left
//! out, the scripts a sanitise took away, the plaintext an encrypt was asked
//! to hide — would still be in the file's prefix, and a command whose whole
//! point is that something has left the file would be one that left it in.
//! The price is the one a rewrite always has: a signature over the original
//! bytes no longer covers the new ones.
//!
//! Two of the commands set one more [`WriteOptions`] field, each the option
//! the editor call it makes documents as its other half. `split` sets
//! [`WriteOptions::garbage_collect`]: a rewrite keeps objects nothing reaches
//! unless asked otherwise, so without it every piece would carry every
//! page's content, unreferenced and readable by anyone who scans the file —
//! [`DocumentEditor::keep_pages`] calls itself "the split half of
//! split-and-merge" for exactly that pairing. `merge` sets
//! [`WriteOptions::deduplicate_streams`]: [`DocumentEditor::import_page`]
//! copies each page with everything it reaches, so pages sharing one face or
//! one picture arrive with a copy each, and its documentation names that
//! option as what merges them again.
//!
//! # An encrypted input
//!
//! A rewrite of an encrypted document that asks for no encryption writes it
//! **decrypted** — the facade drops `/Encrypt` rather than carry it over
//! plaintext. So every command but `encrypt` and `decrypt` refuses an
//! encrypted input outright, naming the two that handle one: `rotate` on an
//! encrypted file quietly producing an unencrypted one is the wrong kind of
//! forgiving.
//!
//! `decrypt`, and `encrypt` over a file that already is, replace the owner's
//! restrictions with none or with new ones, so they want the owner's
//! authority: a user password is enough only when the owner withheld nothing
//! from the user. PDF permissions are advisory and the facade reports them
//! rather than enforcing them ([`Document::permissions`]); this is the one
//! place a command would *erase* them, and it honours them instead.

use std::io::Read;
use std::path::Path;

use tinker_pdf::write::{save, SaveOptions};
use tinker_pdf::{
    BilevelCodec, ContinuousCodec, Document, DocumentEditor, EmbeddedFile, Encryption, EntryHolder,
    ImageOutcome, ImagePolicy, ImageRecoding, JpegTables, PathStep, Removal, Sanitise,
    StampPlacement, SubsetOutcome, WriteMode, WriteOptions,
};

use crate::{open, Options};

/// Where `encrypt` reads its 48 bytes of randomness when `--entropy` names
/// no file: the operating system's own source.
///
/// The engine links no random number generator — `wasm32-unknown-unknown`
/// has nothing to ask — so a host supplies the bytes, and this is a host.
/// A platform without the device has no default, and `encrypt` there says to
/// name a file rather than inventing randomness of its own.
#[cfg(unix)]
const SYSTEM_ENTROPY: Option<&str> = Some("/dev/urandom");
#[cfg(not(unix))]
const SYSTEM_ENTROPY: Option<&str> = None;

/// `--images`: [`ContinuousCodec`]'s three codings by name, before
/// `--jpeg-tables` has been read for the third.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Continuous {
    /// [`ContinuousCodec::Keep`].
    Keep,
    /// [`ContinuousCodec::Flate`].
    Flate,
    /// [`ContinuousCodec::Jpeg`], with the tables `--jpeg-tables` names.
    Jpeg,
}

/// Prints what a writing command reports, unless `--quiet` asked for
/// failures only.
pub(crate) fn print(options: &Options, report: Result<Vec<String>, String>) -> Result<(), String> {
    let lines = report?;
    if !options.quiet {
        for line in lines {
            println!("{line}");
        }
    }
    Ok(())
}

/// `tpdf merge <a.pdf> <b.pdf>... --out FILE`: the first file, with every
/// page of each later one appended in order.
///
/// The first document is the one written out, so its catalog — outline,
/// form, page labels, metadata — is the output's; the later ones contribute
/// their pages and what those pages reach, which is what
/// [`DocumentEditor::import_page`] copies.
pub(crate) fn merge(options: &Options) -> Result<Vec<String>, String> {
    let out = needs_out(options, "merge", "FILE")?;
    let Some((first, rest)) = options.files.split_first() else {
        return Err("no input file".to_string());
    };
    let base = open_clear(first, options)?;
    let mut editor = base.editor();
    let mut lines = vec![format!("  {first}: {}", pages(base.page_count()))];
    for path in rest {
        let doc = open_clear(path, options)?;
        let count = doc.page_count();
        for page in 0..count {
            let at = u32::try_from(editor.page_refs().len())
                .map_err(|_| "more pages than a page index can count".to_string())?;
            editor
                .import_page(doc.cos(), page, at)
                .ok_or_else(|| format!("{path}: page {} could not be imported", page + 1))?;
        }
        lines.push(format!("  {path}: {}", pages(count)));
    }
    let write = WriteOptions {
        deduplicate_streams: true,
        ..rewrite()
    };
    lines.extend(save_to(options, &mut editor, out, write)?);
    Ok(lines)
}

/// `tpdf split <file.pdf> --out DIR [--pages LIST]`: one file per item of
/// the list — `--pages 1-3,5` writes pages one to three as one file and page
/// five as another — or one per page when no list is given.
///
/// Files are `<stem>-NNNN.pdf` for one page and `<stem>-NNNN-NNNN.pdf` for a
/// range, numbered from 1 as the list is.
///
/// A piece carries what its pages reach and, with one exception that is the
/// facade's, nothing else: a page something in the catalog still names — an
/// outline item, a named destination, a link on a kept page — stays in the
/// file outside the page tree, because [`DocumentEditor::keep_pages`] keeps
/// the catalog and the garbage collector follows every reference in it
/// (`a_page_the_outline_names_stays_in_a_piece_that_dropped_it`). A split is
/// not a redaction.
pub(crate) fn split(options: &Options) -> Result<Vec<String>, String> {
    let dir = needs_out(options, "split", "DIR")?;
    let path = only_input(options, "split")?;
    let doc = open_clear(path, options)?;
    let count = doc.page_count();
    let pieces = match options.ranges() {
        Some(ranges) => ranges,
        None => (0..count).map(|page| (page, page)).collect(),
    };
    within(&pieces, count, path)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {dir}: {e}"))?;

    let stem = stem(path);
    let mut lines = vec![format!(
        "  {path}: {}, {}",
        pages(count),
        plural(pieces.len(), "piece", "pieces")
    )];
    for (first, last) in pieces {
        let mut editor = doc.editor();
        let keep: Vec<u32> = (first..=last).collect();
        if !editor.keep_pages(&keep) {
            return Err(format!(
                "{path}: pages {}-{} were refused",
                first + 1,
                last + 1
            ));
        }
        let file = match first == last {
            true => format!("{dir}/{stem}-{:04}.pdf", first + 1),
            false => format!("{dir}/{stem}-{:04}-{:04}.pdf", first + 1, last + 1),
        };
        let write = WriteOptions {
            garbage_collect: true,
            ..rewrite()
        };
        lines.extend(save_to(options, &mut editor, &file, write)?);
    }
    Ok(lines)
}

/// `tpdf rotate <file.pdf> --by DEGREES --out FILE [--page N | --pages LIST]`:
/// each page named, or every page, turned clockwise by a quarter-turn
/// multiple on top of the turn it already has (7.7.3.3).
pub(crate) fn rotate(options: &Options) -> Result<Vec<String>, String> {
    let by = options
        .by
        .ok_or_else(|| "rotate needs --by DEGREES".to_string())?;
    let out = needs_out(options, "rotate", "FILE")?;
    let path = only_input(options, "rotate")?;
    let doc = open_clear(path, options)?;
    let turned = write_pages(options, doc.page_count(), path)?;
    let mut editor = doc.editor();
    for &page in &turned {
        if !editor.rotate_page(page, by) {
            return Err(format!(
                "{path}: page {} was not turned by {by}: `rotate_page` turns a page by a \
                 multiple of 90 and refuses any other turn",
                page + 1
            ));
        }
    }
    let mut lines = vec![format!("  {path}: {} turned by {by}", pages(turned.len()))];
    lines.extend(save_to(options, &mut editor, out, rewrite())?);
    Ok(lines)
}

/// `tpdf encrypt <file.pdf> [--user-password U] [--owner-password O]
/// [--permissions P] [--entropy FILE] --out FILE`: the document written
/// again at R6 (AES-256), the facade's one revision on save.
///
/// The two passwords are [`Encryption`]'s two fields, each empty unless
/// given, and what an empty one means is the facade's: an empty user
/// password is a file anyone opens, whose permissions ask; an empty owner
/// password makes the user password the owner's too, so a file with only a
/// user password opens with it and nothing else, and its permissions bind
/// nobody who can open it. `--permissions` is `/P` as Table 22 stores it and
/// defaults to `-1`, everything permitted, as the bindings' does. The 48
/// bytes of entropy come from `--entropy FILE`, or the system's source where
/// there is one; a file of predictable bytes makes a predictable key, which
/// is the caller's decision and is only for reproducing output.
pub(crate) fn encrypt(options: &Options) -> Result<Vec<String>, String> {
    let out = needs_out(options, "encrypt", "FILE")?;
    let path = only_input(options, "encrypt")?;
    let doc = open(path, options.password.as_deref(), None)?;
    if doc.is_encrypted() {
        owner_authority(path, &doc, "encrypting it again")?;
    }
    let encryption = Encryption {
        user_password: options.user_password.clone().unwrap_or_default(),
        owner_password: options.owner_password.clone().unwrap_or_default(),
        permissions: options.permissions.unwrap_or(-1),
        entropy: entropy(options)?,
    };
    let write = WriteOptions {
        encryption: Some(encryption),
        ..rewrite()
    };
    let mut editor = doc.editor();
    save_to(options, &mut editor, out, write)
}

/// `tpdf decrypt <file.pdf> --password P --out FILE`: the document written
/// again without its encryption.
///
/// Needs the owner password unless the owner withheld nothing from the user
/// (this module's documentation).
pub(crate) fn decrypt(options: &Options) -> Result<Vec<String>, String> {
    let out = needs_out(options, "decrypt", "FILE")?;
    let path = only_input(options, "decrypt")?;
    let doc = open(path, options.password.as_deref(), None)?;
    if !doc.is_encrypted() {
        return Err(format!("{path} is not encrypted"));
    }
    owner_authority(path, &doc, "decrypting it")?;
    let mut editor = doc.editor();
    save_to(options, &mut editor, out, rewrite())
}

/// `tpdf attach <file.pdf> --attach FILE [--name NAME] [--mime TYPE]
/// [--description TEXT] --out FILE`: one file embedded (7.11.4).
///
/// Filed under `--name`, or the attached file's own name when none is given,
/// which is also the file name a viewer offers when it is saved out. A name
/// already filed is the facade's refusal, `AttachError::NameTaken`, and
/// nothing is written.
pub(crate) fn attach(options: &Options) -> Result<Vec<String>, String> {
    let file = options
        .attach
        .as_deref()
        .ok_or_else(|| "attach needs --attach FILE".to_string())?;
    let out = needs_out(options, "attach", "FILE")?;
    let path = only_input(options, "attach")?;
    let doc = open_clear(path, options)?;
    let data = std::fs::read(file).map_err(|e| format!("reading {file}: {e}"))?;
    let filename = Path::new(file).file_name().map_or_else(
        || file.to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let embedded = EmbeddedFile {
        name: options.name.clone().unwrap_or_else(|| filename.clone()),
        filename,
        description: options.description.clone(),
        mime_type: options.mime.clone(),
        created: None,
        modified: None,
        data,
    };
    let mut editor = doc.editor();
    editor
        .attach_file(&embedded)
        .map_err(|e| format!("{path}: {e}"))?;
    let mut lines = vec![format!(
        "  {path}: {file} attached as `{}` ({} bytes)",
        embedded.name,
        embedded.data.len()
    )];
    lines.extend(save_to(options, &mut editor, out, rewrite())?);
    Ok(lines)
}

/// `tpdf stamp <file.pdf> --stamp FILE [--stamp-page N] [--under]
/// [--page N | --pages LIST] --out FILE`: a page of another document drawn
/// over — or with `--under`, beneath — each page named, or every page.
///
/// The stamp is the facade's form of that page: its content, its crop box as
/// the form's `/BBox`, in the stamped page's own user space and unscaled. Its
/// `/Rotate` is not applied, since a form has none, and this states no
/// matrix of its own to make up for it.
pub(crate) fn stamp(options: &Options) -> Result<Vec<String>, String> {
    let source = options
        .stamp
        .as_deref()
        .ok_or_else(|| "stamp needs --stamp FILE".to_string())?;
    let out = needs_out(options, "stamp", "FILE")?;
    let path = only_input(options, "stamp")?;
    let doc = open_clear(path, options)?;
    let stamp = open_clear(source, options)?;
    let stamped = write_pages(options, doc.page_count(), path)?;

    let mut editor = doc.editor();
    let form = editor
        .import_page_as_form(stamp.cos(), options.stamp_page, None)
        .ok_or_else(|| {
            format!(
                "{source}: page {} cannot be a stamp: it has {}, or the page's crop box has no area",
                options.stamp_page + 1,
                pages(stamp.page_count())
            )
        })?;
    let placement = match options.under {
        true => StampPlacement::Under,
        false => StampPlacement::Over,
    };
    for &page in &stamped {
        editor
            .stamp(page, form, placement)
            .ok_or_else(|| format!("{path}: page {} could not be stamped", page + 1))?;
    }
    let mut lines = vec![format!(
        "  {path}: {} stamped {} with page {} of {source}",
        pages(stamped.len()),
        if options.under { "under" } else { "over" },
        options.stamp_page + 1
    )];
    lines.extend(save_to(options, &mut editor, out, rewrite())?);
    Ok(lines)
}

/// `tpdf sanitise <file.pdf> [--javascript] [--actions] [--embedded-files]
/// [--metadata] --out FILE`: what each flag names taken out, or with none of
/// them all four ([`Sanitise::ALL`]), and every entry removed and object
/// deleted listed.
pub(crate) fn sanitise(options: &Options) -> Result<Vec<String>, String> {
    let out = needs_out(options, "sanitise", "FILE")?;
    let path = only_input(options, "sanitise")?;
    let doc = open_clear(path, options)?;
    let what = match options.sanitise == Sanitise::default() {
        true => Sanitise::ALL,
        false => options.sanitise,
    };
    let mut editor = doc.editor();
    let report = editor.sanitise(&what);

    let mut lines = vec![format!(
        "  {path}: {} removed, {} deleted",
        plural(report.removed.len(), "entry", "entries"),
        plural(report.deleted.len(), "object", "objects")
    )];
    for entry in &report.removed {
        let holder = match &entry.holder {
            EntryHolder::Trailer => "the trailer".to_string(),
            EntryHolder::Object(r) => format!("{} {} R", r.num, r.gen),
        };
        let steps: String = entry
            .path
            .iter()
            .map(|step| match step {
                PathStep::Key(key) => format!(" /{}", String::from_utf8_lossy(key)),
                PathStep::Index(index) => format!(" [{index}]"),
            })
            .collect();
        lines.push(format!(
            "  removed {holder}{steps}: {}",
            removal(&entry.what)
        ));
    }
    for gone in &report.deleted {
        lines.push(format!(
            "  deleted {} {} R: {}",
            gone.object.num,
            gone.object.gen,
            removal(&gone.what)
        ));
    }
    lines.extend(save_to(options, &mut editor, out, rewrite())?);
    Ok(lines)
}

/// Why [`DocumentEditor::sanitise`] took something out, in words.
fn removal(what: &Removal) -> String {
    match what {
        Removal::JavaScript => "a JavaScript action".to_string(),
        Removal::DocumentJavaScript => "the document-level scripts".to_string(),
        Removal::CalculationOrder => "the calculation order".to_string(),
        Removal::XfaForm => "an XFA form".to_string(),
        Removal::Action(kind) => format!("a /{} action", String::from_utf8_lossy(kind)),
        Removal::EmbeddedFileTree => "the embedded file tree".to_string(),
        Removal::EmbeddedFile => "an embedded file".to_string(),
        Removal::Info => "the document information dictionary".to_string(),
        Removal::Metadata => "a metadata stream".to_string(),
    }
}

/// The options every command here writes with: a rewrite, as this module's
/// documentation says, and otherwise the facade's defaults.
fn rewrite() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    }
}

/// Saves through the facade's door with the font and image policies asked
/// for, writes the file, and says what was written and what each pass did.
fn save_to(
    options: &Options,
    editor: &mut DocumentEditor,
    out: &str,
    write: WriteOptions,
) -> Result<Vec<String>, String> {
    let saved = save(
        editor,
        &SaveOptions {
            write,
            fonts: options.font_policy,
            images: image_policy(options)?,
        },
    );
    std::fs::write(out, &saved.bytes).map_err(|e| format!("writing {out}: {e}"))?;
    let mut lines = vec![format!(
        "wrote {out} ({}, {} bytes)",
        pages(editor.page_refs().len()),
        saved.bytes.len()
    )];
    lines.extend(font_lines(&saved.fonts));
    lines.extend(image_lines(&saved.images));
    Ok(lines)
}

/// `--images`, `--bilevel` and `--max-ppi` as the facade's [`ImagePolicy`]:
/// [`ImagePolicy::Keep`], its default, when none of the three is given, and
/// otherwise a recoding whose unnamed kind is kept as stored.
///
/// `--images jpeg` needs `--jpeg-tables`: the facade codes with the caller's
/// quantisation tables and has no quality setting, and a table this command
/// chose would be a default of its own (ruling 11). A table the encoder will
/// not take — a zero entry — is the facade's to refuse, image by image, in
/// the report.
fn image_policy(options: &Options) -> Result<ImagePolicy, String> {
    let jpeg = options.images == Some(Continuous::Jpeg);
    if !jpeg && (options.jpeg_tables.is_some() || options.jpeg_subsampled) {
        return Err("--jpeg-tables and --jpeg-subsampled are for --images jpeg".to_string());
    }
    if options.images.is_none() && options.bilevel.is_none() && options.max_ppi.is_none() {
        return Ok(ImagePolicy::Keep);
    }
    let continuous = match options.images {
        None | Some(Continuous::Keep) => ContinuousCodec::Keep,
        Some(Continuous::Flate) => ContinuousCodec::Flate,
        Some(Continuous::Jpeg) => ContinuousCodec::Jpeg(jpeg_tables(options)?),
    };
    let recoding = ImageRecoding::new(continuous, options.bilevel.unwrap_or(BilevelCodec::Keep));
    Ok(ImagePolicy::Recode(match options.max_ppi {
        Some(ppi) => recoding.with_max_ppi(ppi),
        None => recoding,
    }))
}

/// `--jpeg-tables FILE`: exactly 128 bytes, the luminance table and then the
/// chrominance one, each in natural row-major order as [`JpegTables`] takes
/// them.
fn jpeg_tables(options: &Options) -> Result<JpegTables, String> {
    let path = options.jpeg_tables.as_deref().ok_or_else(|| {
        "--images jpeg needs --jpeg-tables FILE: the facade codes with the caller's \
         quantisation tables, and this command chooses none"
            .to_string()
    })?;
    let bytes = std::fs::read(path).map_err(|e| format!("--jpeg-tables {path}: {e}"))?;
    let (Some(luminance), Some(chrominance), 128) = (
        bytes.get(..64).and_then(|t| <[u8; 64]>::try_from(t).ok()),
        bytes
            .get(64..128)
            .and_then(|t| <[u8; 64]>::try_from(t).ok()),
        bytes.len(),
    ) else {
        return Err(format!(
            "--jpeg-tables {path}: {} bytes, where two tables of 64 are 128",
            bytes.len()
        ));
    };
    Ok(JpegTables {
        luminance,
        chrominance,
        subsampled: options.jpeg_subsampled,
    })
}

/// What the image pass did: nothing to say when it did not run, and
/// otherwise a line for the totals and one for each image, recoded or left
/// as stored with the facade's reason (ruling 10).
fn image_lines(outcome: &ImageOutcome) -> Vec<String> {
    let Some(report) = outcome.report() else {
        return Vec::new();
    };
    let mut lines = vec![format!(
        "  images: {} recoded, {} left as stored; {} bytes of image before, {} after",
        report.recoded.len(),
        report.untouched.len(),
        report.bytes_before(),
        report.bytes_after()
    )];
    for done in &report.recoded {
        let mut line = format!(
            "  images: image {} {} R recoded as {:?}, {}x{} to {}x{}, {} bytes to {}",
            done.image.num,
            done.image.gen,
            done.coding,
            done.size.0,
            done.size.1,
            done.resized.0,
            done.resized.1,
            done.before,
            done.after
        );
        if let Some(why) = &done.resolution_kept {
            line.push_str(&format!("; its resolution kept: {why}"));
        }
        lines.push(line);
    }
    for whole in &report.untouched {
        lines.push(format!("  images: {whole}"));
    }
    if matches!(outcome, ImageOutcome::RecodedButTheOriginalsRemain(_)) {
        lines.push("  images: the original streams are still in the file".to_string());
    }
    lines
}

/// What the font pass did, a line for the totals and one for each program it
/// left whole, with the reason the facade gives (ruling 10).
fn font_lines(outcome: &SubsetOutcome) -> Vec<String> {
    let Some(report) = outcome.report() else {
        return vec![
            "  fonts: every program written through as it arrived (--font-policy keep)".to_string(),
        ];
    };
    let measured = report.subsetted.len()
        + report.untouched.len()
        + report.type3.len()
        + report.type3_untouched.len();
    if measured == 0 {
        return vec!["  fonts: nothing embedded to cut".to_string()];
    }
    let mut lines = vec![format!(
        "  fonts: {} cut to the glyphs drawn, {} measured; {} bytes of program before, {} after",
        plural(report.subsetted.len(), "program", "programs"),
        plural(report.type3.len(), "Type 3 font", "Type 3 fonts"),
        report.bytes_before(),
        report.bytes_after()
    )];
    for whole in &report.untouched {
        lines.push(format!("  fonts: {whole}"));
    }
    for whole in &report.type3_untouched {
        lines.push(format!("  fonts: Type 3 {whole}"));
    }
    if matches!(outcome, SubsetOutcome::CutButTheOriginalsRemain(_)) {
        lines.push("  fonts: the original programs are still in the file".to_string());
    }
    lines
}

/// `--out`, which every command here needs: a file, or for `split` a
/// directory.
fn needs_out<'a>(options: &'a Options, command: &str, what: &str) -> Result<&'a str, String> {
    options
        .out
        .as_deref()
        .ok_or_else(|| format!("{command} needs --out {what}"))
}

/// The one file a command that rewrites a single document was given.
///
/// More than one is refused rather than looped over, as the reading commands
/// do: they would all be written to the one `--out`, each over the last.
fn only_input<'a>(options: &'a Options, command: &str) -> Result<&'a str, String> {
    match options.files.as_slice() {
        [path] => Ok(path.as_str()),
        files => Err(format!(
            "{command} rewrites one file and was given {}",
            files.len()
        )),
    }
}

/// Opens an input that will be written out unencrypted, refusing one that is
/// encrypted (this module's documentation).
fn open_clear(path: &str, options: &Options) -> Result<Document, String> {
    let doc = open(path, options.password.as_deref(), None)?;
    if doc.is_encrypted() {
        return Err(format!(
            "{path} is encrypted, and a rewrite would write it decrypted; \
             `tpdf decrypt` it first, and `tpdf encrypt` the result if it should stay encrypted"
        ));
    }
    Ok(doc)
}

/// Refuses an operation that would lift restrictions the owner set, unless
/// the owner's authority is what opened the document.
fn owner_authority(path: &str, doc: &Document, doing: &str) -> Result<(), String> {
    // Every named bit of Table 22. `permissions()` already answers "all" for
    // the owner, so the list is empty exactly when nothing is withheld from
    // whoever opened it.
    let p = doc.permissions();
    let withheld: Vec<&str> = [
        ("print", p.print()),
        ("modify", p.modify()),
        ("copy", p.copy()),
        ("annotate", p.annotate()),
        ("fill forms", p.fill_forms()),
        ("extract for accessibility", p.accessibility()),
        ("assemble", p.assemble()),
        ("print at high resolution", p.print_high_res()),
    ]
    .into_iter()
    .filter(|(_, granted)| !granted)
    .map(|(name, _)| name)
    .collect();
    if withheld.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{path}: opened as its user, from whom the owner withholds {}; \
         {doing} would lift that, so it needs the owner password",
        withheld.join(", ")
    ))
}

/// Refuses a page a writing command was asked for that the document does
/// not have.
///
/// A reading command skips one ([`Options::pages`]): over a directory of
/// files that is the normal case. A writing command that did would write a
/// document short of what it was asked for, and say nothing.
fn within(ranges: &[(u32, u32)], count: u32, path: &str) -> Result<(), String> {
    match ranges.iter().find(|(_, last)| *last >= count) {
        Some((_, last)) => Err(format!(
            "{path} has {}, so there is no page {}",
            pages(count),
            last + 1
        )),
        None => Ok(()),
    }
}

/// The pages a writing command acts on, in the order given: `--page`,
/// `--pages`, or every page.
///
/// Counted out only after [`within`] has held every range to the document,
/// so a list costs at most what the document has a range.
fn write_pages(options: &Options, count: u32, path: &str) -> Result<Vec<u32>, String> {
    match options.ranges() {
        None => Ok((0..count).collect()),
        Some(ranges) => {
            within(&ranges, count, path)?;
            Ok(ranges
                .into_iter()
                .flat_map(|(first, last)| first..=last)
                .collect())
        }
    }
}

/// `--entropy FILE`'s first 48 bytes, or the system source's.
fn entropy(options: &Options) -> Result<[u8; 48], String> {
    let path = options
        .entropy
        .as_deref()
        .or(SYSTEM_ENTROPY)
        .ok_or_else(|| {
            "this platform has no system entropy source here; name a file of at least 48 \
         random bytes with --entropy FILE"
                .to_string()
        })?;
    let mut bytes = [0u8; 48];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|e| format!("--entropy {path}: 48 bytes could not be read: {e}"))?;
    Ok(bytes)
}

fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| "page".to_string(), |s| s.to_string_lossy().into_owned())
}

/// `count` pages, from a page count or a list's length.
fn pages(count: impl TryInto<usize>) -> String {
    plural(count.try_into().unwrap_or(usize::MAX), "page", "pages")
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf::{
        AuthLevel, Dict, DocumentBuilder, ImageData, ObjRef, Object, PdfString, Target, XrefEntry,
    };

    /// An empty directory of this test's own, named after it.
    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("tpdf-writing-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir.to_string_lossy().replace('\\', "/")
    }

    fn parse(args: &[&str]) -> Options {
        let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        Options::parse(&args).expect("the arguments parse")
    }

    fn fixture(name: &str) -> String {
        format!("{}/../../testdata/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    fn write(dir: &str, name: &str, bytes: &[u8]) -> String {
        let path = format!("{dir}/{name}");
        std::fs::write(&path, bytes).expect("a scratch file");
        path
    }

    /// The file as the facade reads it, with `password` when it needs one.
    fn reopen(path: &str, password: Option<&str>) -> Document {
        let doc = Document::open(std::fs::read(path).expect(path)).expect("the output opens");
        if let Some(password) = password {
            doc.authenticate(password).expect("the password opens it");
        }
        doc
    }

    /// Each page's text, trimmed.
    fn texts(doc: &Document) -> Vec<String> {
        (0..doc.page_count())
            .map(|index| {
                doc.page(index)
                    .expect("the page")
                    .text()
                    .plain_text()
                    .trim()
                    .to_string()
            })
            .collect()
    }

    /// The strict validator finds nothing: a written file is a valid PDF,
    /// not merely one this engine's tolerant reader opens.
    fn assert_clean(doc: &Document, what: &str) {
        let defects = doc.validate();
        assert!(defects.is_empty(), "{what}: {defects:?}");
    }

    /// Every object the file's table lists, as the facade parsed it.
    fn objects(doc: &Document) -> Vec<(ObjRef, std::sync::Arc<Object>)> {
        let cos = doc.cos();
        cos.xref()
            .iter()
            .filter_map(|(number, entry)| {
                let generation = match entry {
                    XrefEntry::Offset { gen, .. } => gen,
                    XrefEntry::InStream { .. } => 0,
                    XrefEntry::Free { .. } => return None,
                };
                let r = ObjRef::new(number, generation);
                cos.get(r).ok().map(|object| (r, object))
            })
            .collect()
    }

    /// How many objects are dictionaries or streams whose `key` is the name
    /// `value`.
    fn count_named(doc: &Document, key: &[u8], value: &[u8]) -> usize {
        let cos = doc.cos();
        let (key, value) = (cos.intern(key), cos.intern(value));
        objects(doc)
            .iter()
            .filter(|(_, object)| {
                let dict = match object.as_ref() {
                    Object::Dict(dict) => dict,
                    Object::Stream(stream) => &stream.dict,
                    _ => return false,
                };
                dict.get_name(key) == Some(value)
            })
            .count()
    }

    /// Whether any stream in the file decodes to bytes containing `needle`.
    fn any_stream_says(doc: &Document, needle: &[u8]) -> bool {
        objects(doc).iter().any(|(r, object)| {
            matches!(object.as_ref(), Object::Stream(_))
                && doc
                    .cos()
                    .stream_decoded(*r)
                    .is_ok_and(|data| data.windows(needle.len()).any(|w| w == needle))
        })
    }

    /// A page of text per entry, in Helvetica, so no program is embedded.
    fn pages_saying(lines: &[&str]) -> Vec<u8> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for line in lines {
            builder.add_page(200.0, 100.0, |page| {
                page.text(b"F0", 12.0, 10.0, 50.0, line);
            });
        }
        builder.finish()
    }

    /// Liberation Serif, whole: the face `bundled-fonts` embeds, and the one
    /// the facade's own subsetting tests embed.
    fn face() -> Vec<u8> {
        let path = format!(
            "{}/../../crates/tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(path).expect("the vendored Liberation face")
    }

    /// One page of text over the whole face, as a producer that does not
    /// subset writes it.
    fn whole_face_document() -> Vec<u8> {
        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"LiberationSerif", &face()));
        builder.add_page(300.0, 100.0, |page| {
            page.text(b"F0", 24.0, 20.0, 40.0, "Hello");
        });
        builder.finish()
    }

    /// The program the output embeds for its one font.
    fn only_program(doc: &Document) -> Vec<u8> {
        let fonts = doc.fonts();
        assert_eq!(fonts.len(), 1, "{fonts:?}");
        fonts[0].program_bytes().expect("the program is embedded")
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn merge_appends_every_page_of_each_later_file_in_order() {
        let dir = scratch("merge");
        let out = format!("{dir}/merged.pdf");
        let (first, second) = (fixture("simple-text.pdf"), fixture("outline-3level.pdf"));
        let lines = merge(&parse(&[&first, &second, "--out", &out])).expect("merges");
        assert_eq!(lines[0], format!("  {first}: 3 pages"), "{lines:?}");
        assert_eq!(lines[1], format!("  {second}: 6 pages"), "{lines:?}");

        let merged = reopen(&out, None);
        assert_eq!(merged.page_count(), 9);
        let mut expected: Vec<String> = (1..=3)
            .map(|n| format!("Tinker fixture, page {n} of 3"))
            .collect();
        expected.extend((1..=6).map(|n| format!("Outline fixture, page {n}")));
        assert_eq!(texts(&merged), expected);
        assert_clean(&merged, "the merged file");
    }

    /// Pages sharing one picture arrive with a copy each from `import_page`,
    /// and the merge writes it once.
    #[test]
    fn a_merge_writes_a_resource_its_pages_share_once() {
        let dir = scratch("merge-shared");
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Gray8 {
                width: 2,
                height: 2,
                data: &[0, 85, 170, 255],
            }
        ));
        for _ in 0..3 {
            builder.add_page(50.0, 50.0, |page| page.image(b"Im0", 0.0, 0.0, 50.0, 50.0));
        }
        let shared = write(&dir, "shared.pdf", &builder.finish());
        assert_eq!(count_named(&reopen(&shared, None), b"Subtype", b"Image"), 1);

        let out = format!("{dir}/merged.pdf");
        let first = fixture("simple-text.pdf");
        merge(&parse(&[&first, &shared, "--out", &out])).expect("merges");
        let merged = reopen(&out, None);
        assert_eq!(merged.page_count(), 6);
        assert_eq!(
            count_named(&merged, b"Subtype", b"Image"),
            1,
            "three pages drawing one image carry one image"
        );
        for index in 3..6 {
            assert_eq!(merged.page(index).expect("page").images().len(), 1);
        }
        assert_clean(&merged, "the merged file");
    }

    #[test]
    fn split_writes_a_file_per_item_and_each_carries_only_its_own_pages() {
        let dir = scratch("split");
        let source = write(
            &dir,
            "three.pdf",
            &pages_saying(&["first words", "second words", "third words"]),
        );
        let out = format!("{dir}/pieces");
        let lines = split(&parse(&[&source, "--pages", "3,1-2", "--out", &out])).expect("splits");
        assert_eq!(lines[0], format!("  {source}: 3 pages, 2 pieces"));

        let last = reopen(&format!("{out}/three-0003.pdf"), None);
        assert_eq!(texts(&last), vec!["third words"]);
        assert!(
            !any_stream_says(&last, b"first words") && !any_stream_says(&last, b"second words"),
            "a piece carries nothing of the pages it left out"
        );
        assert_eq!(count_named(&last, b"Type", b"Page"), 1);
        assert_clean(&last, "the one-page piece");

        let front = reopen(&format!("{out}/three-0001-0002.pdf"), None);
        assert_eq!(texts(&front), vec!["first words", "second words"]);
        assert!(!any_stream_says(&front, b"third words"));
        assert_clean(&front, "the two-page piece");

        let names: Vec<String> = std::fs::read_dir(&out)
            .expect("the directory")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");

        let every = format!("{dir}/every");
        split(&parse(&[&source, "--out", &every])).expect("splits");
        for (n, words) in ["first words", "second words", "third words"]
            .iter()
            .enumerate()
        {
            let piece = reopen(&format!("{every}/three-{:04}.pdf", n + 1), None);
            assert_eq!(texts(&piece), vec![*words]);
        }
    }

    /// **A named limit, pinned.** `keep_pages` keeps the catalog, and the
    /// garbage-collecting rewrite follows every reference in it, so a page an
    /// outline item still names stays in a piece that dropped it — out of the
    /// page tree, and readable by anyone who scans the file. The facade's to
    /// change, not this wrapper's (ruling 11); `docs/features/editing.md`
    /// names it. When the facade prunes such references this fails, and the
    /// docs change with it.
    #[test]
    fn a_page_the_outline_names_stays_in_a_piece_that_dropped_it() {
        let dir = scratch("split-outline");
        let source = fixture("outline-3level.pdf");
        split(&parse(&[&source, "--pages", "6", "--out", &dir])).expect("splits");
        let piece = reopen(&format!("{dir}/outline-3level-0006.pdf"), None);
        assert_eq!(piece.page_count(), 1);
        assert_eq!(texts(&piece), vec!["Outline fixture, page 6"]);
        assert_eq!(
            count_named(&piece, b"Type", b"Page"),
            5,
            "page 6, and the four pages the outline's items name"
        );
        assert!(any_stream_says(&piece, b"page 1"));
    }

    #[test]
    fn a_writing_command_refuses_a_page_the_document_does_not_have() {
        let dir = scratch("past-the-end");
        let source = fixture("simple-text.pdf");
        let refused = split(&parse(&[&source, "--pages", "2-4", "--out", &dir]));
        assert_eq!(
            refused.err().as_deref(),
            Some(format!("{source} has 3 pages, so there is no page 4").as_str())
        );
        let out = format!("{dir}/turned.pdf");
        let refused = rotate(&parse(&[
            &source, "--by", "90", "--page", "9", "--out", &out,
        ]));
        assert_eq!(
            refused.err().as_deref(),
            Some(format!("{source} has 3 pages, so there is no page 9").as_str())
        );
        assert_eq!(
            std::fs::read_dir(&dir).expect("the directory").count(),
            0,
            "nothing was written, no piece and no file"
        );
    }

    #[test]
    fn rotate_turns_the_pages_named_on_top_of_their_turn_and_no_others() {
        let dir = scratch("rotate");
        let once = format!("{dir}/once.pdf");
        let source = fixture("simple-text.pdf");
        let lines = rotate(&parse(&[
            &source, "--by", "90", "--pages", "1,3", "--out", &once,
        ]))
        .expect("rotates");
        assert_eq!(lines[0], format!("  {source}: 2 pages turned by 90"));
        let turned = reopen(&once, None);
        let rotation = |doc: &Document| -> Vec<u16> {
            (0..doc.page_count())
                .map(|index| doc.page(index).expect("page").rotation())
                .collect()
        };
        assert_eq!(rotation(&turned), vec![90, 0, 90]);
        assert_eq!(texts(&turned), texts(&reopen(&source, None)));
        assert_clean(&turned, "the rotated file");

        let twice = format!("{dir}/twice.pdf");
        rotate(&parse(&[&once, "--by", "-180", "--out", &twice])).expect("rotates");
        assert_eq!(rotation(&reopen(&twice, None)), vec![270, 180, 270]);

        // A turn that is not a quarter is the facade's to refuse, as it is
        // for every surface, and nothing is written.
        let refused = format!("{dir}/refused.pdf");
        assert_eq!(
            rotate(&parse(&[&source, "--by", "45", "--out", &refused]))
                .err()
                .as_deref(),
            Some(
                format!(
                    "{source}: page 1 was not turned by 45: `rotate_page` turns a page by a \
                     multiple of 90 and refuses any other turn"
                )
                .as_str()
            )
        );
        assert!(!Path::new(&refused).exists());
        let args = strings(&[&source, "--by", "right", "--out", &twice]);
        assert_eq!(
            Options::parse(&args).err().as_deref(),
            Some("`--by right` is not a number")
        );
        assert_eq!(
            rotate(&parse(&[&source, "--out", &twice])).err().as_deref(),
            Some("rotate needs --by DEGREES")
        );
    }

    /// The row's second half: every command that rewrites a document takes
    /// the font policy, and the default is the facade's, subset.
    #[test]
    fn the_font_policy_subsets_by_default_and_keeps_the_face_whole_when_asked() {
        let dir = scratch("fonts");
        let source = write(&dir, "face.pdf", &whole_face_document());
        let whole = face();

        let cut = format!("{dir}/cut.pdf");
        let lines = rotate(&parse(&[&source, "--by", "90", "--out", &cut])).expect("rotates");
        let program = only_program(&reopen(&cut, None));
        assert!(
            program.len() < whole.len(),
            "{} is not smaller than the face's {}",
            program.len(),
            whole.len()
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("  fonts: 1 program cut to the glyphs drawn")),
            "{lines:?}"
        );

        let kept = format!("{dir}/kept.pdf");
        let lines = rotate(&parse(&[
            &source,
            "--by",
            "90",
            "--font-policy",
            "keep",
            "--out",
            &kept,
        ]))
        .expect("rotates");
        assert_eq!(only_program(&reopen(&kept, None)), whole, "the face, whole");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("written through as it arrived")),
            "{lines:?}"
        );

        // On the shared save path, so it reaches every command that writes:
        // the merge of one file is that file, rewritten.
        let merged = format!("{dir}/merged.pdf");
        merge(&parse(&[&source, "--out", &merged])).expect("merges");
        assert!(only_program(&reopen(&merged, None)).len() < whole.len());

        let args = strings(&[&source, "--font-policy", "whole"]);
        assert_eq!(
            Options::parse(&args).err().as_deref(),
            Some("`--font-policy whole`: the policies are `subset` and `keep`")
        );
    }

    #[test]
    fn encrypt_writes_a_file_each_password_opens_with_its_own_authority() {
        let dir = scratch("encrypt");
        let entropy = write(&dir, "entropy", &[7u8; 48]);
        let source = fixture("simple-text.pdf");
        let out = format!("{dir}/locked.pdf");
        let args = [
            source.as_str(),
            "--user-password",
            "open",
            "--owner-password",
            "owner",
            "--permissions",
            "-2056",
            "--entropy",
            &entropy,
            "--out",
            &out,
        ];
        encrypt(&parse(&args)).expect("encrypts");

        let locked = reopen(&out, None);
        assert!(locked.is_encrypted());
        assert!(locked.authenticate("").is_err(), "the empty password fails");
        assert_eq!(locked.authenticate("open"), Ok(AuthLevel::User));
        assert!(!locked.permissions().print(), "/P -2056 withholds printing");
        assert!(locked.permissions().copy());
        assert_eq!(texts(&locked), texts(&reopen(&source, None)));
        assert_clean(&locked, "the encrypted file");
        assert_eq!(
            reopen(&out, None).authenticate("owner"),
            Ok(AuthLevel::Owner)
        );

        // The bytes come from `--entropy`, so the same file twice is the same
        // output — which is what the flag is for, and why it is not for keys.
        let again = format!("{dir}/again.pdf");
        let mut repeat = args;
        repeat[10] = &again;
        encrypt(&parse(&repeat)).expect("encrypts");
        assert_eq!(std::fs::read(&out).ok(), std::fs::read(&again).ok());

        let short = write(&dir, "short", &[1u8; 47]);
        let mut starved = args;
        starved[8] = &short;
        assert!(encrypt(&parse(&starved))
            .err()
            .is_some_and(|e| e.starts_with(&format!("--entropy {short}: 48 bytes"))));
    }

    /// Encrypting a file that already is replaces its passwords, so it wants
    /// the authority `decrypt` does.
    #[test]
    fn encrypt_over_an_encrypted_file_replaces_its_passwords_with_the_owners_authority() {
        let dir = scratch("encrypt-again");
        let entropy = write(&dir, "entropy", &[3u8; 48]);
        let out = format!("{dir}/again.pdf");
        let restricted = fixture("permissions-noprint.pdf");
        let again = |source: &str, password: &str| {
            encrypt(&parse(&[
                source,
                "--password",
                password,
                "--owner-password",
                "new owner",
                "--user-password",
                "new user",
                "--entropy",
                &entropy,
                "--out",
                &out,
            ]))
        };
        assert!(again(&restricted, "user")
            .err()
            .is_some_and(|e| e.contains("encrypting it again would lift that")));
        assert!(!Path::new(&out).exists());

        again(&restricted, "owner").expect("encrypts");
        let doc = reopen(&out, None);
        assert!(doc.authenticate("user").is_err() && doc.authenticate("owner").is_err());
        assert_eq!(doc.authenticate("new user"), Ok(AuthLevel::User));
        assert!(doc.permissions().print(), "the new /P, -1, replaced -2056");
        assert_eq!(texts(&doc), texts(&reopen(&restricted, Some("owner"))));
        assert_clean(&doc, "the re-encrypted file");
    }

    /// With no owner password, or an empty one, the command writes what the
    /// facade writes for one: the user password is the owner's too
    /// ([`Encryption::owner_password`]), so the file opens with it and not
    /// with the empty password every reader tries first. This command used to
    /// refuse that case on its own, a decision the facade and the bindings did
    /// not make (ruling 11); the facade now makes it for all of them.
    #[test]
    fn encrypt_without_an_owner_password_locks_the_file_with_the_users() {
        let dir = scratch("encrypt-empty-owner");
        let entropy = write(&dir, "entropy", &[9u8; 48]);
        let source = fixture("simple-text.pdf");
        let out = format!("{dir}/locked.pdf");
        for owner in [None, Some("")] {
            let mut args = vec![source.as_str(), "--user-password", "u", "--out", &out];
            args.extend(["--entropy", &entropy, "--permissions", "-2056"]);
            if let Some(owner) = owner {
                args.extend(["--owner-password", owner]);
            }
            encrypt(&parse(&args)).expect("encrypts");
            let locked = reopen(&out, None);
            assert!(
                locked.authenticate("").is_err(),
                "the empty password opens nothing ({owner:?})"
            );
            assert_eq!(locked.authenticate("u"), Ok(AuthLevel::Owner));
            assert_eq!(texts(&locked), texts(&reopen(&source, None)));
            assert_clean(&locked, "the file locked with one password");
        }

        // The facade's save with the same fields writes the same bytes: the
        // command adds nothing to what an empty owner password means.
        let mut editor = reopen(&source, None).editor();
        let saved = save(
            &mut editor,
            &SaveOptions {
                write: WriteOptions {
                    encryption: Some(Encryption {
                        user_password: "u".to_string(),
                        owner_password: String::new(),
                        permissions: -2056,
                        entropy: [9u8; 48],
                    }),
                    ..rewrite()
                },
                ..SaveOptions::default()
            },
        );
        assert_eq!(std::fs::read(&out).ok(), Some(saved.bytes));
    }

    #[test]
    fn decrypt_needs_the_owners_authority_where_the_user_is_restricted() {
        let dir = scratch("decrypt");
        let out = format!("{dir}/open.pdf");
        let restricted = fixture("permissions-noprint.pdf");
        let refused = decrypt(&parse(&[&restricted, "--password", "user", "--out", &out]));
        assert_eq!(
            refused.err().as_deref(),
            Some(
                format!(
                    "{restricted}: opened as its user, from whom the owner withholds \
                     print, print at high resolution; decrypting it would lift that, so it \
                     needs the owner password"
                )
                .as_str()
            )
        );
        assert!(!Path::new(&out).exists());

        decrypt(&parse(&[&restricted, "--password", "owner", "--out", &out])).expect("decrypts");
        let clear = reopen(&out, None);
        assert!(!clear.is_encrypted());
        assert_eq!(texts(&clear), texts(&reopen(&restricted, Some("owner"))));
        assert_clean(&clear, "the decrypted file");

        // A user the owner restricted in nothing has nothing to lift.
        let open_to_all = fixture("encrypted-aes256.pdf");
        let out = format!("{dir}/aes.pdf");
        decrypt(&parse(&[
            &open_to_all,
            "--password",
            "open-sesame",
            "--out",
            &out,
        ]))
        .expect("decrypts");
        let clear = reopen(&out, None);
        assert!(!clear.is_encrypted());
        assert_eq!(
            texts(&clear),
            vec!["This document is encrypted with AES-256."]
        );

        let plain = fixture("simple-text.pdf");
        assert_eq!(
            decrypt(&parse(&[&plain, "--out", &out])).err().as_deref(),
            Some(format!("{plain} is not encrypted").as_str())
        );
    }

    /// A rewrite asking for no encryption writes the plaintext, so every
    /// command but the two about the cipher refuses an encrypted input.
    #[test]
    fn a_rewrite_refuses_an_encrypted_input_rather_than_decrypt_it_quietly() {
        let dir = scratch("encrypted-input");
        let out = format!("{dir}/out.pdf");
        let locked = fixture("encrypted-aes256.pdf");
        let (plain, readme) = (fixture("simple-text.pdf"), fixture("README.md"));
        let options = parse(&[
            &locked,
            "--password",
            "owner-secret",
            "--by",
            "90",
            "--stamp",
            &plain,
            "--attach",
            &readme,
            "--out",
            &out,
        ]);
        for (command, refused) in [
            ("merge", merge(&options)),
            ("rotate", rotate(&options)),
            ("attach", attach(&options)),
            ("stamp", stamp(&options)),
            ("sanitise", sanitise(&options)),
        ] {
            assert!(
                refused
                    .as_ref()
                    .err()
                    .is_some_and(|e| e.starts_with(&format!("{locked} is encrypted"))),
                "{command}: {refused:?}"
            );
        }
        let pieces = format!("{dir}/pieces");
        let mut split_options = options;
        split_options.out = Some(pieces.clone());
        assert!(split(&split_options)
            .err()
            .is_some_and(|e| e.starts_with(&format!("{locked} is encrypted"))));
        assert!(!Path::new(&out).exists() && !Path::new(&pieces).exists());
    }

    #[test]
    fn attach_files_the_bytes_under_their_name_and_refuses_a_name_taken() {
        let dir = scratch("attach");
        let data = b"a,b\n1,2\n";
        let csv = write(&dir, "numbers.csv", data);
        let source = fixture("simple-text.pdf");
        let out = format!("{dir}/attached.pdf");
        attach(&parse(&[
            &source,
            "--attach",
            &csv,
            "--mime",
            "text/csv",
            "--description",
            "the numbers",
            "--out",
            &out,
        ]))
        .expect("attaches");

        let doc = reopen(&out, None);
        let attachments = doc.attachments();
        assert_eq!(attachments.len(), 1);
        let filed = &attachments[0];
        assert_eq!(
            (filed.name.as_str(), filed.filename.as_str()),
            ("numbers.csv", "numbers.csv")
        );
        assert_eq!(filed.description.as_deref(), Some("the numbers"));
        let stream = filed.stream.expect("the embedded file stream");
        assert_eq!(
            doc.cos().stream_decoded(stream).ok().as_deref(),
            Some(&data[..])
        );
        let cos = doc.cos();
        let subtype = cos
            .get(stream)
            .ok()
            .and_then(|object| object.as_stream().map(|s| s.dict.clone()))
            .and_then(|dict| dict.get_name(cos.intern(b"Subtype")));
        assert_eq!(subtype, Some(cos.intern(b"text/csv")));
        assert_eq!(texts(&doc), texts(&reopen(&source, None)));
        assert_clean(&doc, "the file with an attachment");

        let named = format!("{dir}/named.pdf");
        attach(&parse(&[
            &out, "--attach", &csv, "--name", "copy", "--out", &named,
        ]))
        .expect("a second name is a second file");
        let names: Vec<String> = reopen(&named, None)
            .attachments()
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(names, vec!["copy", "numbers.csv"]);

        let taken = format!("{dir}/taken.pdf");
        assert_eq!(
            attach(&parse(&[&out, "--attach", &csv, "--out", &taken]))
                .err()
                .as_deref(),
            Some(format!("{out}: a file is already attached as \"numbers.csv\"").as_str())
        );
        assert!(!Path::new(&taken).exists(), "nothing was written");
    }

    #[test]
    fn stamp_draws_another_documents_page_over_or_under_the_pages_named() {
        let dir = scratch("stamp");
        let mark = write(&dir, "mark.pdf", &pages_saying(&["unused", "DRAFT"]));
        let source = fixture("simple-text.pdf");

        let over = format!("{dir}/over.pdf");
        let lines = stamp(&parse(&[
            &source,
            "--stamp",
            &mark,
            "--stamp-page",
            "2",
            "--pages",
            "1,3",
            "--out",
            &over,
        ]))
        .expect("stamps");
        assert_eq!(
            lines[0],
            format!("  {source}: 2 pages stamped over with page 2 of {mark}")
        );
        let stamped = reopen(&over, None);
        assert_eq!(
            texts(&stamped),
            vec![
                "Tinker fixture, page 1 of 3\nDRAFT",
                "Tinker fixture, page 2 of 3",
                "Tinker fixture, page 3 of 3\nDRAFT",
            ],
            "drawn after the page's own content, and on the pages named only"
        );
        assert_clean(&stamped, "the stamped file");

        let under = format!("{dir}/under.pdf");
        stamp(&parse(&[
            &source,
            "--stamp",
            &mark,
            "--stamp-page",
            "2",
            "--under",
            "--out",
            &under,
        ]))
        .expect("stamps");
        let beneath = reopen(&under, None);
        assert!(
            texts(&beneath)
                .iter()
                .all(|text| text.starts_with("DRAFT\nTinker fixture")),
            "drawn before the page's own content, on every page: {:?}",
            texts(&beneath)
        );
        assert_clean(&beneath, "the file stamped underneath");

        let missing = stamp(&parse(&[
            &source,
            "--stamp",
            &mark,
            "--stamp-page",
            "3",
            "--out",
            &under,
        ]));
        assert!(missing
            .err()
            .is_some_and(|e| e.starts_with(&format!("{mark}: page 3 cannot be a stamp"))));
    }

    /// A document carrying one of each thing `sanitise` takes out: a
    /// document-level script, a link that runs a `/URI` action, an
    /// attachment, and `/Info`.
    fn everything_to_take_out(dir: &str) -> String {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        assert!(builder.set_info(b"Title", "a title"));
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "the page stays");
            assert!(page.link(
                0.0,
                0.0,
                50.0,
                50.0,
                &Target::Uri("https://example.com/".to_string())
            ));
        });
        let doc = Document::open(builder.finish()).expect("it opens");
        let mut editor = doc.editor();

        let mut script = Dict::new();
        script.insert(
            editor.intern(b"S"),
            Object::Name(editor.intern(b"JavaScript")),
        );
        script.insert(
            editor.intern(b"JS"),
            Object::String(PdfString::literal(b"app.alert(1)".to_vec())),
        );
        let action = editor.allocate();
        editor.put(action, Object::Dict(script));
        let tree = editor
            .add_name_tree(vec![(b"hello".to_vec(), Object::Ref(action))])
            .expect("a one-entry tree");
        let (names, javascript) = (editor.intern(b"Names"), editor.intern(b"JavaScript"));
        assert!(editor.update_catalog(|catalog| {
            let mut entries = Dict::new();
            entries.insert(javascript, Object::Ref(tree));
            catalog.insert(names, Object::Dict(entries));
        }));
        editor
            .attach_file(&EmbeddedFile {
                name: "notes.txt".to_string(),
                filename: "notes.txt".to_string(),
                data: b"private".to_vec(),
                ..EmbeddedFile::default()
            })
            .expect("attaches");
        write(
            dir,
            "everything.pdf",
            &editor.save(&WriteOptions::default()),
        )
    }

    #[test]
    fn sanitise_takes_out_what_each_flag_names_and_all_four_by_default() {
        let dir = scratch("sanitise");
        let source = everything_to_take_out(&dir);
        let before = reopen(&source, None);
        assert_eq!(before.script_summary().document_scripts, 1);
        assert_eq!(before.attachments().len(), 1);
        assert_eq!(before.metadata().title.as_deref(), Some("a title"));
        assert_eq!(before.page(0).expect("page").annotations().len(), 1);

        let scripts = format!("{dir}/scripts.pdf");
        let lines =
            sanitise(&parse(&[&source, "--javascript", "--out", &scripts])).expect("sanitises");
        assert!(
            lines
                .iter()
                .any(|l| l.ends_with(": the document-level scripts")),
            "{lines:?}"
        );
        let doc = reopen(&scripts, None);
        assert_eq!(doc.script_summary().document_scripts, 0);
        assert_eq!(doc.attachments().len(), 1, "only what was asked for");
        assert_eq!(doc.metadata().title.as_deref(), Some("a title"));
        assert_eq!(doc.page(0).expect("page").annotations().len(), 1);
        assert!(!any_stream_says(&doc, b"app.alert"));
        assert_clean(&doc, "the file without scripts");

        let all = format!("{dir}/all.pdf");
        let lines = sanitise(&parse(&[&source, "--out", &all])).expect("sanitises");
        for what in [
            ": the document-level scripts",
            ": a /URI action",
            ": the embedded file tree",
            ": the document information dictionary",
        ] {
            assert!(lines.iter().any(|l| l.ends_with(what)), "{what}: {lines:?}");
        }
        let doc = reopen(&all, None);
        assert_eq!(doc.script_summary().document_scripts, 0);
        assert!(doc.attachments().is_empty());
        assert_eq!(doc.metadata().title, None);
        assert_eq!(
            doc.page(0).expect("page").annotations().len(),
            0,
            "a link whose action went goes with it"
        );
        assert!(
            !any_stream_says(&doc, b"private"),
            "the attachment's bytes left the file"
        );
        assert_eq!(texts(&doc), vec!["the page stays"]);
        assert_clean(&doc, "the sanitised file");
    }

    /// A single-input command given two files refuses rather than write them
    /// both to one `--out`, and each says what it is missing.
    #[test]
    fn each_command_says_what_it_needs() {
        let source = fixture("simple-text.pdf");
        let two = parse(&[&source, &source, "--by", "90", "--out", "x.pdf"]);
        assert_eq!(
            rotate(&two).err().as_deref(),
            Some("rotate rewrites one file and was given 2")
        );
        let bare = parse(&[&source]);
        for (refused, why) in [
            (merge(&bare), "merge needs --out FILE"),
            (split(&bare), "split needs --out DIR"),
            (encrypt(&bare), "encrypt needs --out FILE"),
            (decrypt(&bare), "decrypt needs --out FILE"),
            (attach(&bare), "attach needs --attach FILE"),
            (stamp(&bare), "stamp needs --stamp FILE"),
            (sanitise(&bare), "sanitise needs --out FILE"),
        ] {
            assert_eq!(refused.err().as_deref(), Some(why));
        }
    }

    /// A 64 by 64 RGB image of 8 by 8 flat blocks and a 64 by 64 one-bit
    /// image, half black, each stored unfiltered and drawn at 64 points
    /// square — 72 pixels an inch — so every coding and a halving of the
    /// resolution has something to make smaller.
    fn two_images() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let rgb: Vec<u8> = (0..64u32 * 64)
            .flat_map(|i| {
                let (x, y) = ((i % 64 / 8) as u8, (i / 64 / 8) as u8);
                [x * 32, y * 32, 128]
            })
            .collect();
        let bits: Vec<u8> = (0..64 * 8)
            .map(|i| if i % 8 < 4 { 0x00 } else { 0xFF })
            .collect();
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 64,
                height: 64,
                data: &rgb,
            }
        ));
        assert!(builder.add_image(
            b"Im1",
            &ImageData::Compressed(tinker_pdf::CompressedImage {
                width: 64,
                height: 64,
                bits_per_component: 1,
                color_space: tinker_pdf::ImageColorSpace::DeviceGray,
                filter: None,
                data: &bits,
                color_key_mask: None,
                soft_mask: None,
            })
        ));
        builder.add_page(200.0, 100.0, |page| {
            page.image(b"Im0", 0.0, 0.0, 64.0, 64.0);
            page.image(b"Im1", 100.0, 0.0, 64.0, 64.0);
        });
        (builder.finish(), rgb, bits)
    }

    /// Each image XObject's `/Filter` name, in object order, empty for none.
    fn image_filters(doc: &Document) -> Vec<String> {
        let cos = doc.cos();
        let (subtype, image, filter) = (
            cos.intern(b"Subtype"),
            cos.intern(b"Image"),
            cos.intern(b"Filter"),
        );
        objects(doc)
            .iter()
            .filter_map(|(_, object)| match object.as_ref() {
                Object::Stream(stream) if stream.dict.get_name(subtype) == Some(image) => Some(
                    stream
                        .dict
                        .get_name(filter)
                        .map_or_else(String::new, |name| {
                            String::from_utf8_lossy(&cos.name_bytes(name).unwrap_or_default())
                                .into_owned()
                        }),
                ),
                _ => None,
            })
            .collect()
    }

    /// The image policy is on the shared save path, so every writer takes it;
    /// none of its flags is the facade's default, every image as stored.
    #[test]
    fn the_image_policy_recodes_and_resamples_when_asked_and_keeps_otherwise() {
        let dir = scratch("images");
        let (bytes, rgb, bits) = two_images();
        let source = write(&dir, "images.pdf", &bytes);
        assert_eq!(image_filters(&reopen(&source, None)), vec!["", ""]);

        let kept = format!("{dir}/kept.pdf");
        let lines = merge(&parse(&[&source, "--out", &kept])).expect("merges");
        assert!(lines.iter().all(|l| !l.contains("images:")), "{lines:?}");
        assert_eq!(image_filters(&reopen(&kept, None)), vec!["", ""]);

        let coded = format!("{dir}/coded.pdf");
        let lines = merge(&parse(&[
            &source,
            "--images",
            "flate",
            "--bilevel",
            "g4",
            "--out",
            &coded,
        ]))
        .expect("merges");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("  images: 2 recoded, 0 left")),
            "{lines:?}"
        );
        let doc = reopen(&coded, None);
        assert_eq!(image_filters(&doc), vec!["FlateDecode", "CCITTFaxDecode"]);
        let drawn = doc.page(0).expect("page").images();
        assert_eq!(drawn[0].samples, rgb, "deflate is lossless");
        assert_eq!(drawn[1].samples, bits, "and so is G4");
        assert_clean(&doc, "the recoded file");

        let halved = format!("{dir}/halved.pdf");
        let lines = rotate(&parse(&[
            &source,
            "--by",
            "0",
            "--max-ppi",
            "36",
            "--out",
            &halved,
        ]))
        .expect("rotates");
        let drawn = reopen(&halved, None).page(0).expect("page").images();
        assert_eq!((drawn[0].width, drawn[0].height), (32, 32), "72 ppi to 36");
        assert_eq!(
            (drawn[1].width, drawn[1].height),
            (64, 64),
            "one bit is not box-filtered: {lines:?}"
        );
        assert!(
            lines.iter().any(
                |l| l.ends_with("left as stored (512 bytes): one bit: a box filter makes grey")
            ),
            "{lines:?}"
        );

        let ones = write(&dir, "ones", &[1u8; 128]);
        let jpeg = format!("{dir}/jpeg.pdf");
        merge(&parse(&[
            &source,
            "--images",
            "jpeg",
            "--jpeg-tables",
            &ones,
            "--out",
            &jpeg,
        ]))
        .expect("merges");
        assert_eq!(image_filters(&reopen(&jpeg, None)), vec!["DCTDecode", ""]);

        let short = write(&dir, "short", &[1u8; 100]);
        for (args, why) in [
            (
                vec![source.as_str(), "--images", "jpeg", "--out", &jpeg],
                "--images jpeg needs --jpeg-tables FILE",
            ),
            (
                vec![source.as_str(), "--jpeg-tables", &ones, "--out", &jpeg],
                "--jpeg-tables and --jpeg-subsampled are for --images jpeg",
            ),
            (
                vec![
                    source.as_str(),
                    "--images",
                    "jpeg",
                    "--jpeg-tables",
                    &short,
                    "--out",
                    &jpeg,
                ],
                "--jpeg-tables",
            ),
        ] {
            assert!(
                merge(&parse(&args))
                    .err()
                    .is_some_and(|e| e.starts_with(why)),
                "{args:?}"
            );
        }
        for (flag, raw, why) in [
            ("--max-ppi", "0", "`--max-ppi 0` is not a resolution"),
            ("--max-ppi", "fine", "`--max-ppi fine` is not a number"),
            (
                "--images",
                "png",
                "`--images png`: the codings are `keep`, `flate` and `jpeg`",
            ),
            (
                "--bilevel",
                "g3",
                "`--bilevel g3`: the codings are `keep`, `flate`, `g4` and `jbig2`",
            ),
        ] {
            let args = strings(&[&source, flag, raw]);
            assert_eq!(Options::parse(&args).err().as_deref(), Some(why));
        }
    }
}
