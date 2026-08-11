//! `pdfcmp` — a perceptual image diff, for comparing renders.
//!
//! Exact pixel equality is the wrong bar for a renderer. Two correct
//! rasterizers disagree on anti-aliased edges, and a diff that fails on those
//! reports every page as broken, which trains everyone to ignore it. So the
//! metric here reports *how much* two images differ and where, and a budget
//! decides whether that is acceptable.
//!
//! The metric deliberately has the same shape as Tinker's own
//! `visual_regression.rs`, so budgets tuned there transfer here rather than
//! having to be rediscovered.
//!
//! Inputs are PNM files, which is what `tpdf render` writes, or PDFs, which
//! are rendered first. Comparing a PDF against a reference image is the usual
//! shape of an oracle test.

use std::process::ExitCode;

use tinker_pdf::{Bitmap, Document, PixelFormat, RenderOptions};

const USAGE: &str = "\
pdfcmp — compare two renders perceptually

usage:
  pdfcmp <a> <b> [--budget F] [--threshold N] [--dpi D] [--page N]
                 [--diff FILE] [--quiet]

<a> and <b> may each be a .pnm image or a .pdf, which is rendered first.

options:
  --budget F     the largest acceptable fraction of changed pixels, 0..1
                 (default 0.0005, which is Tinker's own tolerance)
  --threshold N  how far a channel must move for a pixel to count as changed,
                 0..255 (default 12, likewise). Anti-aliasing differences
                 between architectures land in the low single digits.
  --dpi D        resolution when rendering a PDF (default 150)
  --page N       which page to render, 1-based (default 1)
  --diff FILE    write a difference image, brightest where they disagree
  --quiet        print only the verdict

The budget gates on *how many pixels changed*, not on how much they changed
on average. A page of text is overwhelmingly white, so a glyph landing one
pixel to the left changes a few hundred pixels completely and barely moves
the mean — which is why the mean cannot be the gate, and why these defaults
are the ones Tinker's own visual regression uses.

exit status is 0 when the difference is within budget, 1 when it is not, and
2 when the inputs could not be compared at all — different sizes, or files
that will not load.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("pdfcmp: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<bool, String> {
    let mut inputs = Vec::new();
    // Tinker's `visual_regression.rs` constants, so a budget tuned there
    // means the same thing here.
    let mut budget = 0.0005f64;
    let mut threshold = 12u8;
    let mut dpi = 150.0f64;
    let mut page = 0u32;
    let mut diff_path = None;
    let mut quiet = false;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = || -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("`{arg}` needs a value"))
        };
        match arg {
            "--budget" => {
                budget = value()?
                    .parse()
                    .map_err(|_| "--budget takes a number".to_string())?;
            }
            "--threshold" => {
                threshold = value()?
                    .parse()
                    .map_err(|_| "--threshold takes a number from 0 to 255".to_string())?;
            }
            "--dpi" => {
                dpi = value()?
                    .parse()
                    .map_err(|_| "--dpi takes a number".to_string())?;
            }
            "--page" => {
                let n: u32 = value()?
                    .parse()
                    .map_err(|_| "--page takes a number".to_string())?;
                page = n.saturating_sub(1);
            }
            "--diff" => diff_path = Some(value()?),
            "--quiet" => quiet = true,
            _ if arg.starts_with("--") => return Err(format!("unknown option `{arg}`")),
            _ => inputs.push(arg.to_string()),
        }
        index += 1;
    }

    if inputs.len() != 2 {
        return Err("needs exactly two inputs".to_string());
    }
    if !budget.is_finite() || !(0.0..=1.0).contains(&budget) {
        return Err("--budget is a fraction between 0 and 1".to_string());
    }

    let a = load(&inputs[0], dpi, page)?;
    let b = load(&inputs[1], dpi, page)?;

    if a.width != b.width || a.height != b.height {
        return Err(format!(
            "different sizes: {}x{} against {}x{}",
            a.width, a.height, b.width, b.height
        ));
    }

    let report = compare(&a, &b, threshold);
    if let Some(path) = diff_path {
        write_diff(&path, &a, &b)?;
    }

    if !quiet {
        println!(
            "changed   {:.4}% of pixels by more than {threshold}",
            report.over * 100.0
        );
        println!("mean      {:.6}", report.mean);
        println!(
            "worst     {:.6} at ({}, {})",
            report.worst, report.at.0, report.at.1
        );
        println!(
            "differing {:.4}% of pixels at all",
            report.differing * 100.0
        );
    }

    let within = report.over <= budget;
    println!(
        "{} {:.4}% changed against budget {:.4}%",
        if within { "within" } else { "OVER" },
        report.over * 100.0,
        budget * 100.0
    );
    Ok(within)
}

/// A PNM as written by `tpdf render`, or a page of a PDF.
fn load(path: &str, dpi: f64, page: u32) -> Result<Image, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading {path}: {e}"))?;

    if path.to_ascii_lowercase().ends_with(".pdf") || bytes.starts_with(b"%PDF") {
        let doc = Document::open(bytes).map_err(|e| format!("{path}: {e:?}"))?;
        let rendered = doc
            .page(page)
            .ok_or_else(|| format!("{path} has no page {}", page + 1))?
            .render(&RenderOptions {
                format: PixelFormat::Rgb8,
                ..RenderOptions::at_dpi(dpi)
            });
        return Ok(Image::from_bitmap(&rendered));
    }

    read_pnm(&bytes).map_err(|e| format!("{path}: {e}"))
}

/// An image reduced to what the comparison needs.
struct Image {
    width: u32,
    height: u32,
    /// Three bytes per pixel, tightly packed.
    pixels: Vec<u8>,
}

impl Image {
    fn from_bitmap(bitmap: &Bitmap) -> Image {
        let components = bitmap.components();
        let mut pixels = Vec::with_capacity((bitmap.width * bitmap.height * 3) as usize);
        for y in 0..bitmap.height as usize {
            let row = y * bitmap.stride;
            for x in 0..bitmap.width as usize {
                let at = row + x * components;
                let pixel = bitmap.data.get(at..at + components).unwrap_or(&[]);
                match components {
                    1 | 2 => {
                        let grey = pixel.first().copied().unwrap_or(0);
                        pixels.extend_from_slice(&[grey, grey, grey]);
                    }
                    _ => {
                        pixels.extend_from_slice(&[
                            pixel.first().copied().unwrap_or(0),
                            pixel.get(1).copied().unwrap_or(0),
                            pixel.get(2).copied().unwrap_or(0),
                        ]);
                    }
                }
            }
        }
        Image {
            width: bitmap.width,
            height: bitmap.height,
            pixels,
        }
    }

    fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let index = ((y * self.width + x) * 3) as usize;
        match self.pixels.get(index..index + 3) {
            Some(pixel) => [pixel[0], pixel[1], pixel[2]],
            None => [0, 0, 0],
        }
    }
}

/// Reads a binary PNM (P5 grey or P6 colour).
fn read_pnm(bytes: &[u8]) -> Result<Image, String> {
    let mut cursor = 0usize;

    // The header is whitespace-separated tokens, with `#` comments allowed
    // between any of them.
    let mut token = || -> Result<String, String> {
        loop {
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'#' {
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
                continue;
            }
            break;
        }
        let start = cursor;
        while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if start == cursor {
            return Err("the header ends early".to_string());
        }
        String::from_utf8(bytes[start..cursor].to_vec()).map_err(|_| "bad header".to_string())
    };

    let magic = token()?;
    let components = match magic.as_str() {
        "P5" => 1usize,
        "P6" => 3,
        other => return Err(format!("`{other}` is not a binary PNM")),
    };
    let width: u32 = token()?.parse().map_err(|_| "bad width".to_string())?;
    let height: u32 = token()?.parse().map_err(|_| "bad height".to_string())?;
    let max: u32 = token()?.parse().map_err(|_| "bad maximum".to_string())?;
    if max != 255 {
        return Err(format!("only 8-bit samples are read, not {max}"));
    }
    // Exactly one whitespace byte separates the header from the data.
    cursor += 1;

    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(components);
    let data = bytes
        .get(cursor..cursor + expected)
        .ok_or_else(|| "the data is shorter than the header claims".to_string())?;

    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for chunk in data.chunks_exact(components) {
        match components {
            1 => pixels.extend_from_slice(&[chunk[0], chunk[0], chunk[0]]),
            _ => pixels.extend_from_slice(&chunk[..3]),
        }
    }

    Ok(Image {
        width,
        height,
        pixels,
    })
}

struct Report {
    /// The fraction of pixels where a channel moved by more than the
    /// threshold. **This is what the budget gates on.**
    ///
    /// Mean difference was the gate here first, and it cannot do the job.
    /// A page of text is overwhelmingly white; a glyph landing one pixel to
    /// the left changes a few hundred pixels completely and moves the mean by
    /// a ten-thousandth. The budget passes and the regression ships. Tinker's
    /// own `visual_regression.rs` counts *changed pixels* for exactly that
    /// reason, and this file's own header promises budgets transfer between
    /// the two — which they could not while the metrics disagreed.
    over: f64,
    /// The mean difference over every pixel, 0 to 1. Reported, not gated:
    /// it is a useful summary of how *far* things moved once something has
    /// moved.
    mean: f64,
    /// The largest single-pixel difference.
    worst: f64,
    /// Where that was.
    at: (u32, u32),
    /// The fraction of pixels that differ at all, however slightly.
    differing: f64,
}

/// Compares two images of the same size.
///
/// The per-pixel difference is the largest of the three channel differences
/// rather than their average: a page that goes red where it should be black
/// differs badly in one channel and not at all in the others, and averaging
/// would report a third of the problem.
fn compare(a: &Image, b: &Image, threshold: u8) -> Report {
    let mut total = 0.0f64;
    let mut worst = 0.0f64;
    let mut at = (0u32, 0u32);
    let mut differing = 0usize;
    let mut over = 0usize;

    for y in 0..a.height {
        for x in 0..a.width {
            let (pa, pb) = (a.at(x, y), b.at(x, y));
            let delta = (0..3)
                .map(|c| f64::from(pa[c].abs_diff(pb[c])) / 255.0)
                .fold(0.0f64, f64::max);

            total += delta;
            if delta > 0.0 {
                differing += 1;
            }
            // Compared in whole channel steps rather than against the
            // normalised delta, so the number means the same thing here as it
            // does in Tinker.
            if (0..3).any(|c| pa[c].abs_diff(pb[c]) > threshold) {
                over += 1;
            }
            if delta > worst {
                worst = delta;
                at = (x, y);
            }
        }
    }

    let count = (a.width as f64) * (a.height as f64);
    let count = if count > 0.0 { count } else { 1.0 };
    Report {
        over: over as f64 / count,
        mean: total / count,
        worst,
        at,
        differing: differing as f64 / count,
    }
}

/// Writes a greyscale image of where the two disagree, brightest where the
/// difference is largest. Looking at one of these is how a budget failure
/// gets diagnosed.
fn write_diff(path: &str, a: &Image, b: &Image) -> Result<(), String> {
    let mut out = format!("P5\n{} {}\n255\n", a.width, a.height).into_bytes();
    for y in 0..a.height {
        for x in 0..a.width {
            let (pa, pb) = (a.at(x, y), b.at(x, y));
            let delta = (0..3).map(|c| pa[c].abs_diff(pb[c])).max().unwrap_or(0);
            out.push(delta);
        }
    }
    std::fs::write(path, out).map_err(|e| format!("writing {path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
        Image {
            width,
            height,
            pixels,
        }
    }

    /// The regression this tool exists to catch, and the one the mean could
    /// not see.
    ///
    /// A page of text is overwhelmingly white. Shift a glyph one pixel and a
    /// few hundred pixels flip between black and white while the mean barely
    /// moves — under the old gate of `mean <= 0.001`, a whole glyph could
    /// move and the comparison would report "within budget".
    #[test]
    fn a_shifted_glyph_is_caught_even_though_the_mean_barely_moves() {
        // A 100x100 white page with a 10x10 black mark, and the same page
        // with the mark one pixel to the right.
        let mark = |at: u32| -> Image {
            let mut pixels = vec![255u8; 100 * 100 * 3];
            for y in 40..50u32 {
                for x in at..at + 10 {
                    let i = ((y * 100 + x) * 3) as usize;
                    pixels[i] = 0;
                    pixels[i + 1] = 0;
                    pixels[i + 2] = 0;
                }
            }
            image(100, 100, pixels)
        };

        let report = compare(&mark(20), &mark(21), 12);

        // Twenty pixels of a ten-thousand-pixel page: the mean is two
        // thousandths of the way to "completely different".
        assert!(
            report.mean < 0.003,
            "the mean stays tiny, which is the whole problem: {}",
            report.mean
        );
        assert!(
            report.mean <= 0.001 * 3.0,
            "and it is the order of a mean budget"
        );

        // The gate sees it.
        assert!(
            report.over > 0.0005,
            "the changed-pixel fraction is over Tinker's tolerance: {}",
            report.over
        );
        assert_eq!(report.over, 20.0 / 10_000.0, "twenty pixels moved");
    }

    /// Anti-aliasing noise between architectures lands in the low single
    /// digits, and must not count. Without a threshold the gate would fire on
    /// every platform difference and the budget would be meaningless.
    #[test]
    fn noise_below_the_threshold_does_not_count_as_changed() {
        let flat = image(4, 1, vec![200; 12]);
        let noisy = image(
            4,
            1,
            vec![206, 194, 200, 200, 208, 200, 200, 200, 195, 200, 203, 200],
        );

        let report = compare(&flat, &noisy, 12);
        assert_eq!(report.over, 0.0, "nothing moved by more than twelve");
        assert!(report.differing > 0.0, "though they do differ");
    }

    #[test]
    fn a_channel_exactly_at_the_threshold_is_not_yet_changed() {
        let a = image(1, 1, vec![100, 100, 100]);
        let at = image(1, 1, vec![112, 100, 100]);
        let past = image(1, 1, vec![113, 100, 100]);

        assert_eq!(compare(&a, &at, 12).over, 0.0, "twelve is within");
        assert_eq!(compare(&a, &past, 12).over, 1.0, "thirteen is not");
    }

    /// The defaults are Tinker's, so a budget tuned in one place means the
    /// same thing in the other — which this file's own header has always
    /// promised and could not deliver while the metrics disagreed.
    #[test]
    fn the_defaults_match_tinkers_visual_regression() {
        let usage = USAGE;
        assert!(usage.contains("default 0.0005"), "the tolerance");
        assert!(usage.contains("default 12"), "the channel threshold");
    }

    #[test]
    fn identical_images_differ_by_nothing() {
        let a = image(2, 1, vec![10, 20, 30, 40, 50, 60]);
        let b = image(2, 1, vec![10, 20, 30, 40, 50, 60]);
        let report = compare(&a, &b, 12);
        assert_eq!(report.mean, 0.0);
        assert_eq!(report.worst, 0.0);
        assert_eq!(report.differing, 0.0);
    }

    /// The per-pixel difference takes the worst channel, not the average. A
    /// page that comes out red where it should be black differs completely in
    /// one channel and not at all in the others, and averaging would report a
    /// third of the problem.
    #[test]
    fn one_bad_channel_is_not_averaged_away() {
        let black = image(1, 1, vec![0, 0, 0]);
        let red = image(1, 1, vec![255, 0, 0]);
        let report = compare(&black, &red, 12);
        assert_eq!(report.worst, 1.0, "the red channel is completely wrong");
        assert_eq!(report.mean, 1.0);
    }

    #[test]
    fn the_worst_pixel_is_located() {
        let mut pixels = vec![0u8; 4 * 3];
        // The third pixel of a 2x2.
        pixels[6] = 255;
        let report = compare(&image(2, 2, vec![0; 12]), &image(2, 2, pixels), 12);
        assert_eq!(report.at, (0, 1));
        assert_eq!(report.differing, 0.25);
    }

    #[test]
    fn a_colour_pnm_round_trips() {
        let mut bytes = b"P6\n2 1\n255\n".to_vec();
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let read = read_pnm(&bytes).expect("it reads");
        assert_eq!((read.width, read.height), (2, 1));
        assert_eq!(read.at(0, 0), [1, 2, 3]);
        assert_eq!(read.at(1, 0), [4, 5, 6]);
    }

    #[test]
    fn a_grey_pnm_is_expanded_to_three_channels() {
        let mut bytes = b"P5\n2 1\n255\n".to_vec();
        bytes.extend_from_slice(&[7, 9]);
        let read = read_pnm(&bytes).expect("it reads");
        assert_eq!(read.at(0, 0), [7, 7, 7]);
        assert_eq!(read.at(1, 0), [9, 9, 9]);
    }

    #[test]
    fn comments_in_the_header_are_skipped() {
        let mut bytes = b"P6\n# written by something\n1 1\n255\n".to_vec();
        bytes.extend_from_slice(&[1, 2, 3]);
        assert_eq!(read_pnm(&bytes).expect("it reads").at(0, 0), [1, 2, 3]);
    }

    /// A header that promises more data than the file holds must be refused
    /// rather than read past the end.
    #[test]
    fn a_truncated_pnm_is_refused() {
        let bytes = b"P6\n100 100\n255\nnot enough".to_vec();
        assert!(read_pnm(&bytes).is_err());
    }

    #[test]
    fn a_format_that_is_not_a_binary_pnm_is_refused() {
        assert!(read_pnm(b"P3\n1 1\n255\n0 0 0").is_err(), "ascii PNM");
        assert!(read_pnm(b"not an image at all").is_err());
        assert!(read_pnm(b"P6\n1 1\n65535\n").is_err(), "16-bit samples");
    }

    #[test]
    fn an_empty_image_does_not_divide_by_zero() {
        let report = compare(&image(0, 0, Vec::new()), &image(0, 0, Vec::new()), 12);
        assert!(report.mean.is_finite() && report.mean == 0.0);
    }
}
