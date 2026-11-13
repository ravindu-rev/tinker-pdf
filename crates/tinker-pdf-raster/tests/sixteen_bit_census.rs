//! The price of each relation a 16-bit sample could keep with today's byte
//! (`docs/design/sixteen-bit.md`, RC-07.2).
//!
//! Two candidates, both test code and never engine code, so nothing here moves
//! a render:
//!
//! - **(A) today's arithmetic refined in a low byte**: the stage's formula at
//!   sixteen bits, held into the 256-wide interval of the byte today's
//!   arithmetic gives, so its top byte is today's byte by construction. The
//!   owner's answer to H-140 (ruling 23), which every later step implements.
//! - **(B) a value carried at sixteen bits throughout**: the same formula at
//!   sixteen bits and nothing else, nearer the exact value and free of today's
//!   bytes.
//!
//! Today's byte comes from `Canvas` and `fill` themselves, never from a
//! formula written here, and each candidate's value from arithmetic written
//! here from the design. For every case the census counts, per candidate, how
//! many top bytes differ from today's, how many values (A) holds at an
//! interval's end, and how far each value lies from the exact one, in 16-bit
//! units.
//!
//! # The domain
//!
//! Each stage of the compositing core is priced on its own, its 8-bit
//! operands widened to sixteen bits as the design widens a byte (`257·b`):
//!
//! - **the effective alpha**: every 8-bit coverage against every constant
//!   alpha, a white source over an opaque black backdrop, whose result is the
//!   product the core composites by;
//! - **the opaque branch**: every effective alpha, source and backdrop over an
//!   opaque backdrop, `blend`'s fast path;
//! - **the straight-alpha branch**: every effective alpha against every
//!   backdrop alpha short of opaque, over a grid of sources and backdrops a
//!   seventeenth apart (11.3.6's straight-alpha composite);
//! - **coverage**: every unit count from 0 to 4 096 the filler accumulates in
//!   a pixel, each made by a shape whose units the sampling grid states.
//!
//! The figures it prints are committed, when it runs, as
//! `sixteen_bit_census.tsv` beside this file, with the command that
//! regenerates them (ruling 16):
//!
//! ```text
//! cargo test --release -p tinker-pdf-raster --test sixteen_bit_census -- --ignored --nocapture
//! ```

use tinker_pdf_raster::{fill, Canvas, Color, FillRule, Mask, Path, PixelFormat};

/// A byte at sixteen bits: the byte in its top byte and again in its low one,
/// so `b / 255` is exactly `widen(b) / 65 535`.
fn widen(byte: u8) -> u64 {
    257 * u64::from(byte)
}

/// `a·b / 65 535`, rounded to nearest. 65 535 is odd and `2·a·b` even, so the
/// quotient is never a half and the integer form has no tie to break.
fn mul16(a: u64, b: u64) -> u64 {
    (a * b + 32_767) / 65_535
}

/// (A) at one rounding point: `value` held into the interval of `byte`.
fn hold(value: u64, byte: u8) -> u64 {
    let low = 256 * u64::from(byte);
    value.clamp(low, low + 255)
}

/// What one candidate made of one stage's cases.
#[derive(Default)]
struct Tally {
    cases: u64,
    /// Top bytes that are not today's byte.
    differing: u64,
    /// Values (A) moved to an interval's end; always zero for (B).
    held: u64,
    /// The largest distance from the exact value, in 16-bit units.
    largest: f64,
    /// The sum of the distances, for the mean.
    total: f64,
}

impl Tally {
    fn add(&mut self, value: u64, today: u8, exact: f64, held: bool) {
        self.cases += 1;
        if value >> 8 != u64::from(today) {
            self.differing += 1;
        }
        if held {
            self.held += 1;
        }
        let distance = (value as f64 - exact).abs();
        self.largest = self.largest.max(distance);
        self.total += distance;
    }

    fn line(&self, stage: &str, candidate: &str, channel: &str) -> String {
        let mean = if self.cases == 0 {
            0.0
        } else {
            self.total / self.cases as f64
        };
        format!(
            "{stage}\t{candidate}\t{channel}\t{}\t{}\t{}\t{:.3}\t{:.3}",
            self.cases, self.differing, self.held, self.largest, mean
        )
    }
}

/// Both candidates' tallies for one channel of one stage.
#[derive(Default)]
struct Pair {
    a: Tally,
    b: Tally,
}

impl Pair {
    /// One case: `wide` is the formula at sixteen bits, `today` the byte
    /// today's arithmetic gave, `exact` the exact value in 16-bit units.
    fn add(&mut self, wide: u64, today: u8, exact: f64) {
        let held = hold(wide, today);
        self.a.add(held, today, exact, held != wide);
        self.b.add(wide, today, exact, false);
    }

    fn lines(&self, stage: &str, channel: &str) -> [String; 2] {
        [
            self.a.line(stage, "A", channel),
            self.b.line(stage, "B", channel),
        ]
    }
}

/// A grey-and-alpha canvas `width` by `height` whose pixel `(x, y)` holds the
/// grey and alpha `backdrop` gives it.
fn backdrop(width: u32, height: u32, backdrop: impl Fn(u32, u32) -> (u8, u8)) -> Canvas {
    let mut canvas = Canvas::new(width, height, PixelFormat::GrayA8, Color::TRANSPARENT);
    for y in 0..height {
        for x in 0..width {
            let (grey, alpha) = backdrop(x, y);
            let at = (y as usize) * canvas.stride + (x as usize) * 2;
            canvas.data[at] = grey;
            canvas.data[at + 1] = alpha;
        }
    }
    canvas
}

/// A mask over the canvas whose pixel `(x, y)` reads `coverage(x, y)`.
fn mask(width: u32, height: u32, coverage: impl Fn(u32, u32) -> u8) -> Mask {
    let mut mask = Mask::empty(0, 0, width, height);
    for y in 0..height {
        for x in 0..width {
            mask.data[(y as usize) * (width as usize) + x as usize] = coverage(x, y);
        }
    }
    mask
}

/// The grey and alpha today's canvas holds at `(x, y)`.
fn read(canvas: &Canvas, x: u32, y: u32) -> (u8, u8) {
    let at = (y as usize) * canvas.stride + (x as usize) * 2;
    (canvas.data[at], canvas.data[at + 1])
}

/// An opaque grey.
fn grey(value: u8) -> Color {
    Color::rgb(value, value, value)
}

/// The effective alpha: every coverage `c` against every constant alpha `α`,
/// white over opaque black, so today's pixel is the product the core
/// composites by. (B) is `mul16(widen(c), widen(α))`; the exact value is
/// `c·α / 255²` of the whole.
fn effective_alpha() -> Pair {
    let mut pair = Pair::default();
    let black = backdrop(256, 1, |_, _| (0, 255));
    let coverage = mask(256, 1, |x, _| x as u8);
    for alpha in 0..=255u8 {
        let mut canvas = black.clone();
        canvas.fill_mask(&coverage, Color::WHITE, f64::from(alpha) / 255.0);
        for c in 0..=255u8 {
            let (today, _) = read(&canvas, u32::from(c), 0);
            let wide = mul16(widen(c), widen(alpha));
            let exact = 65_535.0 * f64::from(c) * f64::from(alpha) / 65_025.0;
            pair.add(wide, today, exact);
        }
    }
    pair
}

/// The opaque branch: every effective alpha `e` (the coverage, at a constant
/// alpha of one), source `cs` and backdrop `cb`, over an opaque backdrop.
/// (B) is the source at a whole alpha, the backdrop untouched at none, and
/// `min(mul16(cs, e) + mul16(cb, 65 535 − e), 65 535)` between; the exact
/// value is `cs·e/255 + cb·(1 − e/255)`.
fn opaque_branch() -> Pair {
    let mut pair = Pair::default();
    let backdrops = backdrop(256, 256, |x, _| (x as u8, 255));
    let coverage = mask(256, 256, |_, y| y as u8);
    for source in 0..=255u8 {
        let mut canvas = backdrops.clone();
        canvas.fill_mask(&coverage, grey(source), 1.0);
        for e in 0..=255u8 {
            for cb in 0..=255u8 {
                let (today, _) = read(&canvas, u32::from(cb), u32::from(e));
                let (cs16, cb16, e16) = (widen(source), widen(cb), widen(e));
                let wide = match e {
                    0 => cb16,
                    255 => cs16,
                    _ => (mul16(cs16, e16) + mul16(cb16, 65_535 - e16)).min(65_535),
                };
                let exact = 257.0
                    * (f64::from(source) * f64::from(e) + f64::from(cb) * f64::from(255 - e))
                    / 255.0;
                pair.add(wide, today, exact);
            }
        }
    }
    pair
}

/// The straight-alpha branch: every effective alpha `e` against every
/// backdrop alpha `ab` short of opaque, over sources and backdrops a
/// seventeenth apart, under `Normal`. (B) is 11.3.6's composite as `blend`
/// writes it, at sixteen bits; the exact colour is `(cs·a + cb·b·(1 − a)) /
/// (a + b·(1 − a))` and the exact alpha `a + b·(1 − a)`, with `a = e/255` and
/// `b = ab/255`.
fn straight_branch() -> (Pair, Pair) {
    let (mut colour, mut alpha) = (Pair::default(), Pair::default());
    let coverage = mask(255, 256, |_, y| y as u8);
    let grid: Vec<u8> = (0..=15u8).map(|step| step * 17).collect();
    for &cb in &grid {
        let backdrops = backdrop(255, 256, |x, _| (cb, x as u8));
        for &source in &grid {
            let mut canvas = backdrops.clone();
            canvas.fill_mask(&coverage, grey(source), 1.0);
            for e in 0..=255u8 {
                for ab in 0..=254u8 {
                    let (today_colour, today_alpha) = read(&canvas, u32::from(ab), u32::from(e));
                    let (cs16, cb16, e16, ab16) = (widen(source), widen(cb), widen(e), widen(ab));
                    let (wide_colour, wide_alpha) = match e {
                        0 => (cb16, ab16),
                        255 => (cs16, 65_535),
                        _ => {
                            // `Normal`'s mix is the source; the weighting is
                            // `blend`'s, the backdrop's alpha being its own.
                            let mixed = cs16;
                            let blended = mul16(65_535 - ab16, cs16) + mul16(ab16, mixed);
                            let union = e16 + mul16(ab16, 65_535 - e16);
                            let weighted =
                                mul16(blended, e16) + mul16(mul16(cb16, ab16), 65_535 - e16);
                            let composite = (weighted * 65_535 + union / 2)
                                .checked_div(union)
                                .unwrap_or(0)
                                .min(65_535);
                            (composite, union.min(65_535))
                        }
                    };
                    let a = f64::from(e) / 255.0;
                    let b = f64::from(ab) / 255.0;
                    let union = a + b * (1.0 - a);
                    let exact_colour = if union > 0.0 {
                        257.0 * (f64::from(source) * a + f64::from(cb) * b * (1.0 - a)) / union
                    } else {
                        0.0
                    };
                    colour.add(wide_colour, today_colour, exact_colour);
                    alpha.add(wide_alpha, today_alpha, 65_535.0 * union);
                }
            }
        }
    }
    (colour, alpha)
}

/// Coverage: every unit count `u` from 0 to 4 096, made as a shape the grid
/// states — sixteen sub-scanlines `q` units across beside `r` sub-scanlines
/// one unit across, `u = 16·q + r` — and today's byte read from the mask
/// `fill` returns. (A) is `min(16·u, 65 535)`, the design's wide coverage,
/// which needs no holding; (B) is `round(65 535·u / 4 096)`; the exact value
/// is `65 535·u / 4 096`.
fn coverage() -> Pair {
    let mut pair = Pair::default();
    for units in 0..=4_096u32 {
        let (q, r) = (units / 16, units % 16);
        let mut path = Path::new();
        if q > 0 {
            path.rect(0.0, 0.0, f64::from(q) / 256.0, 1.0);
        }
        if r > 0 {
            path.rect(f64::from(q) / 256.0, 0.0, 1.0 / 256.0, f64::from(r) / 16.0);
        }
        let mask = fill(&path, FillRule::NonZero, 0, 0, 1, 1, 0.1, None);
        let today = mask.data.first().copied().unwrap_or(0);
        let exact = 65_535.0 * f64::from(units) / 4_096.0;
        let a = (16 * u64::from(units)).min(65_535);
        let b = (65_535 * u64::from(units) + 2_048) / 4_096;
        pair.a.add(a, today, exact, false);
        pair.b.add(b, today, exact, false);
    }
    pair
}

/// **The two relations priced against today's arithmetic.** Prints, for each
/// stage and candidate, the cases, the top bytes that are not today's, the
/// values (A) holds, and the largest and mean distance from the exact value
/// in 16-bit units, as tab-separated lines. Holds nothing: the figures are
/// the owner's to read, committed as `sixteen_bit_census.tsv` when it runs.
#[test]
#[ignore = "a census for the owner: prints the price of each 16-bit relation and holds nothing"]
fn the_sixteen_bit_candidates_priced_against_today_s_arithmetic() {
    println!("RAN the_sixteen_bit_candidates_priced_against_today_s_arithmetic");
    println!("stage\tcandidate\tchannel\tcases\ttop_bytes_differing\theld\tlargest\tmean");
    let (colour, alpha) = straight_branch();
    let lines = [
        effective_alpha().lines("effective-alpha", "alpha"),
        opaque_branch().lines("opaque-branch", "colour"),
        colour.lines("straight-branch", "colour"),
        alpha.lines("straight-branch", "alpha"),
        coverage().lines("coverage", "coverage"),
    ];
    for line in lines.iter().flatten() {
        println!("{line}");
    }
}
