//! What the reviewed-golden files share: the header a golden carries, its
//! refusal of a field absent, blank or white space, the byte-for-byte
//! comparison, the size ceilings and the writer.
//!
//! `render_goldens.rs` holds one golden per operator family and
//! `surface_goldens.rs` one per rendered surface — synthesised appearances,
//! pages made from other formats, SVG output — and both are held by this one
//! mechanism, so a golden reads as reviewed or not by the same rule wherever
//! it lives ([`docs/design/render-verification.md`] milestone 6, K-08).
//!
//! # The header, in its two forms
//!
//! A golden carries the record of its review in its own bytes: `family`,
//! `clause`, `reviewer`, `reviewed` and `built-by`, each `key: value`, none of
//! them absent and none blank or white space — those are the ways a review
//! gets recorded as having happened when it did not. A raster golden is a
//! binary PPM and carries them as `#` comment lines after its magic. An SVG
//! golden is text and carries them in an XML comment after the XML
//! declaration, so the file stays a document a viewer opens; the golden is
//! the writer's output with that comment and its line feed inserted, and
//! [`svg_body`] takes them out again.

#![allow(
    dead_code,
    reason = "shared by two test binaries; each uses a different subset"
)]

use std::path::{Path, PathBuf};

use tinker_pdf::Bitmap;

/// The fields every header carries, in the order a golden writes them.
pub const FIELDS: [&str; 5] = ["family", "clause", "reviewer", "reviewed", "built-by"];

/// The line that opens every header, in either form.
pub const BANNER: &str = "tinker-pdf reviewed golden";

/// What a reviewer's field says before a person has read the golden.
pub const UNREVIEWED_MARK: &str = "unreviewed";

/// What a golden's header says the golden is.
#[derive(Debug, PartialEq)]
pub struct Header {
    pub family: String,
    pub clause: String,
    pub reviewer: String,
    pub reviewed: String,
    pub built_by: String,
}

/// Why a header was refused. Every variant but the format's is a field
/// absent or blank, because those are the two ways a review gets recorded
/// as having happened when it did not.
#[derive(Debug, PartialEq)]
pub enum HeaderError {
    NotAPpm,
    NotAnSvgGolden,
    Missing(&'static str),
    Empty(&'static str),
}

/// The five fields out of a header's lines, each the line that starts with
/// `{marker}{key}:`, refusing one absent or blank: there is no default and no
/// way to write a golden without one.
fn fields<'a>(
    lines: impl Iterator<Item = &'a str> + Clone,
    marker: &str,
) -> Result<Header, HeaderError> {
    let field = |key: &'static str| -> Result<String, HeaderError> {
        let prefix = format!("{marker}{key}:");
        let line = lines
            .clone()
            .find(|line| line.starts_with(&prefix))
            .ok_or(HeaderError::Missing(key))?;
        let value = line[prefix.len()..].trim().to_string();
        if value.is_empty() {
            return Err(HeaderError::Empty(key));
        }
        Ok(value)
    };
    Ok(Header {
        family: field("family")?,
        clause: field("clause")?,
        reviewer: field("reviewer")?,
        reviewed: field("reviewed")?,
        built_by: field("built-by")?,
    })
}

/// Reads a PPM golden's `#` comment lines, refusing a blank field the way
/// `pdfa_ledger.rs`'s `read_ledger` refuses a blank reason.
pub fn read_header(bytes: &[u8]) -> Result<Header, HeaderError> {
    if !bytes.starts_with(b"P6\n") {
        return Err(HeaderError::NotAPpm);
    }
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    fields(text.lines(), "# ")
}

/// Reads an SVG golden's header comment, refusing a blank field the same
/// way.
pub fn read_svg_header(text: &str) -> Result<Header, HeaderError> {
    let comment = header_comment(text).ok_or(HeaderError::NotAnSvgGolden)?;
    fields(comment.lines(), "")
}

/// Where the XML declaration ends, so the header comment can follow it: just
/// past the declaration's line feed, or the start of a text with none.
fn after_declaration(text: &str) -> usize {
    if text.starts_with("<?xml") {
        text.find("?>\n").map_or(0, |at| at + 3)
    } else {
        0
    }
}

/// The text inside an SVG golden's header comment, the banner first.
fn header_comment(text: &str) -> Option<&str> {
    let rest = &text[after_declaration(text)..];
    let inner = rest.strip_prefix("<!--\n")?;
    let end = inner.find("-->\n")?;
    let comment = &inner[..end];
    comment.starts_with(BANNER).then_some(comment)
}

/// A header's five lines, `key: value`, in [`FIELDS`]' order.
fn header_lines(
    family: &str,
    clause: &str,
    reviewer: &str,
    reviewed: &str,
    built_by: &str,
) -> [String; 5] {
    [
        format!("family: {family}"),
        format!("clause: {clause}"),
        format!("reviewer: {reviewer}"),
        format!("reviewed: {reviewed}"),
        format!("built-by: {built_by}"),
    ]
}

/// A golden's identity: what its header says it is, apart from its review.
#[derive(Clone, Copy, Debug)]
pub struct Golden<'a> {
    /// The family, which is also the golden's file name.
    pub family: &'a str,
    /// The clause the golden draws.
    pub clause: &'a str,
    /// The test file and function that build it, `file.rs::family`.
    pub built_by: &'a str,
}

/// A rendered page as a binary PPM, with the review recorded in its own
/// bytes.
pub fn to_ppm(bitmap: &Bitmap, golden: Golden<'_>, reviewer: &str, reviewed: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"P6\n");
    out.extend_from_slice(format!("# {BANNER}\n").as_bytes());
    for line in header_lines(
        golden.family,
        golden.clause,
        reviewer,
        reviewed,
        golden.built_by,
    ) {
        out.extend_from_slice(format!("# {line}\n").as_bytes());
    }
    out.extend_from_slice(format!("{} {}\n255\n", bitmap.width, bitmap.height).as_bytes());
    out.extend_from_slice(&rgb_samples(bitmap));
    out
}

/// An SVG writer's output as a golden: the header comment inserted after
/// the XML declaration.
pub fn to_svg_golden(svg: &str, golden: Golden<'_>, reviewer: &str, reviewed: &str) -> String {
    let at = after_declaration(svg);
    let mut comment = format!("<!--\n{BANNER}\n");
    for line in header_lines(
        golden.family,
        golden.clause,
        reviewer,
        reviewed,
        golden.built_by,
    ) {
        // XML 1.0 2.5: a comment holds no `--`.
        assert!(!line.contains("--"), "a header line holds `--`: {line}");
        comment.push_str(&line);
        comment.push('\n');
    }
    comment.push_str("-->\n");
    format!("{}{comment}{}", &svg[..at], &svg[at..])
}

/// An SVG golden past its header comment: the writer's output it was made
/// from, exactly, or `None` for a text with no header comment.
pub fn svg_body(golden: &str) -> Option<String> {
    let at = after_declaration(golden);
    let rest = golden[at..].strip_prefix("<!--\n")?;
    let end = rest.find("-->\n")? + "-->\n".len();
    header_comment(golden)?;
    Some(format!("{}{}", &golden[..at], &rest[end..]))
}

/// A bitmap's colour samples, three bytes a pixel, which is what a PPM
/// holds: any alpha is dropped.
pub fn rgb_samples(bitmap: &Bitmap) -> Vec<u8> {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .flat_map(|pixel| pixel[..3].to_vec())
        .collect()
}

/// The samples of a PPM, past whatever header it carries.
pub fn ppm_samples(bytes: &[u8]) -> &[u8] {
    // P6, then comments and three integers, the last followed by exactly one
    // whitespace byte (Netpbm's rule) before the raster begins.
    let mut at = 3;
    let mut numbers = 0;
    while at < bytes.len() && numbers < 3 {
        if bytes[at] == b'#' {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
        } else if bytes[at].is_ascii_digit() {
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            numbers += 1;
            continue;
        }
        at += 1;
    }
    // Netpbm: exactly one whitespace byte separates the maxval from the
    // raster, and it belongs to the header rather than to the picture.
    &bytes[(at + 1).min(bytes.len())..]
}

/// A raster golden as the picture it holds: an `Rgb8` bitmap of the width and
/// height its header states, its samples [`ppm_samples`]'s. `None` for bytes
/// that are not a binary PPM of eight-bit samples whose raster is whole.
pub fn ppm_bitmap(bytes: &[u8]) -> Option<Bitmap> {
    if bytes.get(..2) != Some(b"P6".as_slice()) {
        return None;
    }
    // The same walk `ppm_samples` makes, keeping the three numbers.
    let mut at = 2;
    let mut numbers = Vec::with_capacity(3);
    while at < bytes.len() && numbers.len() < 3 {
        let byte = bytes[at];
        if byte == b'#' {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
        } else if byte.is_ascii_digit() {
            let start = at;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
            numbers.push(
                std::str::from_utf8(&bytes[start..at])
                    .ok()?
                    .parse::<u32>()
                    .ok()?,
            );
            continue;
        }
        at += 1;
    }
    let [width, height, max] = numbers[..] else {
        return None;
    };
    let samples = ppm_samples(bytes);
    let length = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(3)?;
    if max != 255 || width == 0 || height == 0 || samples.len() < length {
        return None;
    }
    Some(Bitmap {
        width,
        height,
        format: tinker_pdf::PixelFormat::Rgb8,
        stride: width as usize * 3,
        data: samples[..length].to_vec(),
        warnings: Vec::new(),
        premultiplied: false,
        stats: None,
    })
}

/// How a committed raster golden and what this build draws differ, or
/// `None` when they are byte for byte the same picture.
pub fn ppm_mismatch(family: &str, committed: &[u8], bitmap: &Bitmap) -> Option<String> {
    let samples = rgb_samples(bitmap);
    let stored = ppm_samples(committed);
    (stored != samples.as_slice()).then(|| {
        let differing = stored
            .iter()
            .zip(samples.iter())
            .filter(|(a, b)| a != b)
            .count();
        format!(
            "{family}: {differing} of {} bytes differ (stored {} bytes, drew {})",
            samples.len(),
            stored.len(),
            samples.len()
        )
    })
}

/// How a committed SVG golden and what this build writes differ, or `None`
/// when the golden, past its header comment, is the output byte for byte.
pub fn svg_mismatch(family: &str, committed: &str, svg: &str) -> Option<String> {
    let Some(stored) = svg_body(committed) else {
        return Some(format!("{family}: the golden has no header comment"));
    };
    (stored != svg).then(|| {
        let first = stored
            .bytes()
            .zip(svg.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| stored.len().min(svg.len()));
        format!(
            "{family}: the SVG differs from byte {first} (stored {} bytes, wrote {})",
            stored.len(),
            svg.len()
        )
    })
}

/// The failure message for goldens a build does not reproduce.
pub fn disagreement(wrong: &[String]) -> String {
    format!(
        "the goldens and this build disagree. If the change is deliberate, \
         re-run the writer with --ignored and have the new pictures reviewed \
         again -- a golden replaced without a second reading is a regression \
         test that agreed with whatever arrived:\n{}",
        wrong.join("\n")
    )
}

/// The goldens in `dir` with one of `extensions`, by file stem, sorted: an
/// absent directory holds none.
pub fn goldens_in(dir: &Path, extensions: &[&str]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if !extensions.contains(&path.extension()?.to_str()?) {
                return None;
            }
            path.file_stem()?.to_str().map(str::to_string)
        })
        .collect();
    found.sort();
    found
}

/// Holds every golden of `paths` to `per_golden` bytes and all of them to
/// `altogether`: a golden stays small enough that reviewing one is a real
/// act.
pub fn assert_small_enough(paths: &[(String, PathBuf)], per_golden: u64, altogether: u64) {
    let mut total = 0;
    for (name, path) in paths {
        let size = std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("{name}: the golden is not there ({e})"))
            .len();
        assert!(
            size <= per_golden,
            "{name} is {size} bytes, past the {per_golden} a reviewer is asked for"
        );
        total += size;
    }
    assert!(
        total <= altogether,
        "the goldens are {total} bytes altogether, past {altogether}"
    );
}

/// Who signed a golden the writer writes, and when: `TINKER_GOLDEN_REVIEWER`
/// and `TINKER_GOLDEN_DATE`, each `unreviewed` when unset.
pub fn reviewer_and_date() -> (String, String) {
    let read = |key: &str| std::env::var(key).unwrap_or_else(|_| UNREVIEWED_MARK.into());
    (read("TINKER_GOLDEN_REVIEWER"), read("TINKER_GOLDEN_DATE"))
}
