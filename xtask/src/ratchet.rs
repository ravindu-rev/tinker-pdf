//! The bar, and what it means to clear it.
//!
//! ## Why the arithmetic is integers
//!
//! The obvious comparator divides and compares: `passed_now / total_now >=
//! passed_before / total_before`. It is wrong in two ways at once.
//!
//! The small way is that binary floating point cannot represent most of these
//! ratios, so a run that passed exactly the same files can compare as very
//! slightly worse and fail; the usual patch is an epsilon, which is a hole of
//! a size nobody chose.
//!
//! The large way is what happens when a corpus grows. `960/976` is 0.9836.
//! Add forty upstream files, pass thirty of them, and the run is `990/1016` =
//! 0.9744 — a regression, correctly, because ten new files fail. But if the
//! comparison is written the other way round anywhere, or a rate is stored
//! instead of a pair of counts, growth *dilutes* the bar and a run that broke
//! files can clear it. Storing counts and cross-multiplying removes the
//! division entirely:
//!
//! ```text
//! passed_now * total_before >= passed_before * total_now
//! ```
//!
//! Both sides are exact integers, there is no epsilon, and a corpus whose
//! size changed is compared on the same terms as one that did not.
//!
//! ## What is ratcheted and what is only reported
//!
//! The **pass rate** is the bar: a regression fails CI, which is what plan 23
//! asks for.
//!
//! The **degradation rate** — the share of files that rendered with something
//! reported — is compared and printed, and does not fail by default. It is the
//! honest choice rather than the lenient one: a release that starts *naming* a
//! leniency it previously performed in silence makes this number go up, and
//! that is an improvement in exactly the property ruling 10 is about. Failing
//! on it would put a standing incentive on not reporting. `--strict` turns it
//! into a failure for anyone who wants that trade.
//!
//! ## What is refused rather than compared
//!
//! A comparison whose two sides are not the same measurement is refused, not
//! resolved. An incomplete run, a corpus that vanished, and a run whose
//! `--fonts` setting differs from the recorded one all stop the comparison
//! with a message. The font case is the subtle one: this engine bundles no
//! faces, so supplying them removes most of the warnings, and letting that
//! land in the same slot as a no-faces figure would either look like a large
//! improvement or bake in a bar no plain run can clear.

use std::collections::BTreeMap;

use crate::json::Json;
use crate::report::Run;

/// One corpus's recorded bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub name: String,
    pub total: u64,
    pub passed: u64,
    pub degraded: u64,
    /// How many files the strict pass ran on (ruling 13): opened cleanly,
    /// unencrypted, and rewritten.
    pub strict_eligible: u64,
    /// How many of those rewrites carried no structural defect.
    pub strict_clean: u64,
    /// Per metamorphic relation (roadmap step 7): how many files it was asked
    /// of, and how many it held on.
    ///
    /// A map rather than fields, because the relations are a list that grows:
    /// a bar recorded before a relation existed simply has no entry for it,
    /// and a run that adds one is an improvement rather than a refusal.
    pub metamorphic: BTreeMap<String, (u64, u64)>,
    /// What the structure tree walk reached (ISO 32000-1 14.7), or `None` in a
    /// bar recorded before the walk existed.
    ///
    /// `None` is an improvement rather than a refusal, for the reason an
    /// absent metamorphic relation is: a bar that predates a measurement
    /// cannot be regressed against.
    pub tagged: Option<TaggedBar>,
    /// The most memory any one child needed, or `None` in a bar recorded
    /// before the measurement existed — which is the three bars committed
    /// today.
    ///
    /// `None` for [`Bar::tagged`]'s reason, and it is what keeps `SCHEMA` at 2:
    /// a bar that predates a measurement cannot be regressed against, so an
    /// absent key is an improvement rather than a refusal and no committed
    /// file has to be rewritten to stay readable.
    pub peak: Option<PeakBar>,
    /// How many files carried each capability the child's scanner names, or an
    /// empty map in a bar recorded before this axis existed.
    ///
    /// **This one guards the measurement rather than the engine.** A
    /// capability count is a property of the *corpus* — how many files name an
    /// `ICCBased` space, carry a JBIG2 stream, hold a signature — so a run that
    /// finds fewer of them than the bar did has not got worse at rendering,
    /// it has stopped *seeing* them. That is the drift nine of this
    /// repository's thirteen censuses were found in when the nightly first ran
    /// them, and it is invisible to every other axis here: a scanner that
    /// quietly stops recognising a filter name makes the pass rate go up.
    ///
    /// Empty is an improvement rather than a refusal, for [`Bar::tagged`]'s
    /// reason.
    pub capabilities: BTreeMap<String, u64>,
    /// The five runs' time figures, or `None` where none were measured for
    /// this corpus.
    pub time: Option<TimeBar>,
    /// How many files ended each way when the bar was recorded, by the
    /// report's word: `passed`, `failed`, `crashed`, `timed_out`, and
    /// `stalled` where any did. A word the bar does not state counts none,
    /// so a bar without the key, or without `stalled`, counts no stall
    /// (MS-20).
    pub outcomes: BTreeMap<String, u64>,
}

/// The peak-memory bar for one corpus.
///
/// **The only absolute band in this file, and the only `<=`.** Every other
/// axis here is a rate compared by cross-multiplication, because a count
/// compared against a count changes meaning when the corpus grows.
///
/// A maximum does not. Adding a file can only raise it if that file genuinely
/// costs more than anything already in the corpus, and *that is the thing
/// being ratcheted* — "no file in this corpus ever needs more than N bytes" is
/// a statement about the engine that a larger corpus can only make harder to
/// satisfy, never easier. It is precisely the opposite of the orphan ceiling
/// [`compare`] argues against three comparisons up: a ceiling on orphans is
/// regressed by reading *more* text, so it punishes the run for doing more
/// work, whereas nothing about reading more files raises the memory one file
/// needs.
///
/// So there is no cross-multiplication: this is a maximum against a maximum,
/// in exact integer arithmetic.
///
/// **There is an epsilon, and it is two percent, because the measurement's own
/// swing is not zero.** This comment used to say there was none, and told
/// whoever recorded a band to "expect the next one to sit within a megabyte
/// either side" — which is a swing described and then not allowed for. It cost
/// exactly what that costs: on 6 September 2026 a `--check` run of the same
/// release binary over the same files reported `verapdf` at 55 902 208 bytes
/// against a band of 55 480 320, a regression of **421 888 bytes, 0.76 %**,
/// with nothing changed between the two runs.
///
/// The two measurements this repository has of that swing are 0.06 % (two runs
/// over 974 `pdfjs` files, 924 033 024 and 923 512 832 bytes) and 0.76 %. A
/// high-water mark is not a clock and does not depend on how busy the machine
/// was, but it does depend on what the allocator asked the kernel for, and
/// where it asked. [`PEAK_TOLERANCE_PERCENT`] is set above both, and a band
/// that admits two percent of a memory ceiling still refuses every regression
/// anybody is going to write: the caps this ratchet is a backstop for are
/// counted in megabytes, not in kilobytes.
/// How far above a recorded peak a run may sit before it is a regression.
///
/// Measured rather than chosen: see [`PeakBar`]. Two percent is above both
/// swings this repository has measured — 0.06 % and 0.76 % — and far below any
/// regression worth reporting, since the allocations this backstops are
/// bounded in megabytes.
pub const PEAK_TOLERANCE_PERCENT: u64 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeakBar {
    /// The largest peak resident set any child reached, in bytes.
    pub bytes: u64,
    /// How many children reported one, which is the maximum's denominator: a
    /// maximum taken over fewer children is a different measurement.
    pub files: u64,
}

/// How many dispatches set a bar's peak bands: three runs of `corpus.yml` on
/// `develop` at one revision, read by `corpus-peaks --record` (MS-19).
///
/// A band from one run would set the bar on whichever machine that run landed
/// on, the reason a time figure is five runs' and never one's.
pub const PEAK_RUNS: usize = 3;

/// The dispatches a bar's peak bands were read from, at its top beside
/// `time_runs` and in the shape of that block's `dispatches` (MS-19).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeakRuns {
    /// The revision of `develop` the runs ran at.
    pub revision: String,
    /// Each run's dispatch and the host it landed on.
    pub dispatches: Vec<Dispatch>,
}

/// The structure-tree bar for one corpus.
///
/// Four counts, compared three ways, and none of them a stored rate — see
/// [`compare`] for which comparison each takes and why. The counts are over
/// the corpus, not averaged over its files: a per-file rate averaged is not
/// the rate over the corpus, and a corpus of mostly-untagged files would
/// otherwise let one enormous tree carry the figure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaggedBar {
    /// Files carrying a `/StructTreeRoot` this engine could read.
    pub files: u64,
    /// Structure elements reached by the `/K` walk, summed.
    pub elements: u64,
    /// Characters a structure element claimed.
    pub matched: u64,
    /// Characters carrying an `/MCID` no element on their page claimed.
    pub orphans: u64,
}

/// How many runs set a corpus's time figures: five dispatches of `corpus.yml`
/// at one revision (MS-07).
pub const TIME_RUNS: usize = 5;

/// The narrowest band a time figure is held to, in percent.
///
/// The swing the bench's noisiest operation showed within one of the fleet's
/// two groups, so five runs that all landed on one group are not held tighter
/// than that group has been seen to move.
pub const TIME_BAND_FLOOR_PERCENT: u128 = 20;

/// One run's time figures for one corpus: what a report's `summary.time`
/// carries, and what a bar's `time.runs` holds five of (MS-07).
///
/// Counts and sums rather than rates, as every other axis here: the two means
/// are a sum over a count and are compared by cross-multiplication. The three
/// percentiles are over the corpus's pages, not its files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimeFigures {
    /// Files whose open and count was timed: those that opened.
    pub files: u64,
    /// Their open-and-count spans, summed, in microseconds.
    pub open_micros: u64,
    /// Pages whose render was timed.
    pub pages: u64,
    /// Their render spans, summed, in microseconds.
    pub render_micros: u64,
    /// The 50th percentile page render, in microseconds, by nearest rank.
    pub p50_micros: u64,
    /// The 95th.
    pub p95_micros: u64,
    /// The 99th.
    pub p99_micros: u64,
}

impl TimeFigures {
    /// The object a report's summary and a bar's run carry.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("files", Json::count(self.files)),
            ("open_micros", Json::count(self.open_micros)),
            ("pages", Json::count(self.pages)),
            ("render_micros", Json::count(self.render_micros)),
            ("p50_micros", Json::count(self.p50_micros)),
            ("p95_micros", Json::count(self.p95_micros)),
            ("p99_micros", Json::count(self.p99_micros)),
        ])
    }

    fn from_json(json: &Json, name: &str) -> Result<TimeFigures, String> {
        let read = |key: &str| -> Result<u64, String> {
            json.get(key)
                .and_then(Json::as_u64)
                .ok_or_else(|| format!("`{name}`'s timed run has no whole-number `{key}`"))
        };
        Ok(TimeFigures {
            files: read("files")?,
            open_micros: read("open_micros")?,
            pages: read("pages")?,
            render_micros: read("render_micros")?,
            p50_micros: read("p50_micros")?,
            p95_micros: read("p95_micros")?,
            p99_micros: read("p99_micros")?,
        })
    }
}

/// The five runs a bar's time figures come from, and what they ran under.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeRuns {
    /// The resolution every page was rendered at.
    pub dpi: f64,
    /// How many children ran at once: a span is wall time, and its neighbours
    /// are part of it.
    pub jobs: u64,
    /// The revision of `develop` the five ran at.
    pub revision: String,
    /// When they were recorded.
    pub recorded: String,
    /// Each run's dispatch and the host it landed on, in the order every
    /// corpus's `time.runs` holds them.
    pub dispatches: Vec<Dispatch>,
}

/// One of the five dispatches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dispatch {
    /// The workflow run's id.
    pub id: String,
    /// The machine it ran on, as its report states it.
    pub host: String,
}

/// One corpus's time block: the five runs' figures.
///
/// No band is a key: each is derived from the runs by [`band`], so none is
/// typed by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeBar {
    /// The corpus's file count when the five ran. A run over another count is
    /// a different measurement.
    pub total: u64,
    /// The five runs' figures, in the order `time_runs` names the dispatches.
    pub runs: Vec<TimeFigures>,
}

/// A time a figure stands for: `micros` over `over`, so a mean is compared by
/// cross-multiplication and never divided. A percentile is over one page, and
/// a figure over nothing was not measured.
#[derive(Clone, Copy, Debug)]
struct Time {
    micros: u64,
    over: u64,
}

impl Time {
    /// Whether this is no slower than `other`.
    fn at_most(self, other: Time) -> bool {
        u128::from(self.micros) * u128::from(other.over)
            <= u128::from(other.micros) * u128::from(self.over)
    }

    /// Whether this is at most `band` percent slower than `fastest`.
    fn within(self, fastest: Time, band: u128) -> bool {
        let now = u128::from(self.micros)
            .saturating_mul(u128::from(fastest.over))
            .saturating_mul(100);
        let allowed = (100 + band)
            .saturating_mul(u128::from(fastest.micros))
            .saturating_mul(u128::from(self.over));
        now <= allowed
    }

    /// In microseconds, for a message. Nothing compares it.
    fn mean(self) -> f64 {
        self.micros as f64 / self.over.max(1) as f64
    }
}

/// The five figures a corpus is held to (MS-07).
#[derive(Clone, Copy, Debug)]
enum Figure {
    PagesPerSecond,
    OpenAndCount,
    P50,
    P95,
    P99,
}

impl Figure {
    const ALL: [Figure; 5] = [
        Figure::PagesPerSecond,
        Figure::OpenAndCount,
        Figure::P50,
        Figure::P95,
        Figure::P99,
    ];

    /// The figure as a time, so every band reads one way: pages a second is
    /// held as its reciprocal, the mean time a page took.
    fn time(self, figures: &TimeFigures) -> Time {
        let page = u64::from(figures.pages > 0);
        match self {
            Figure::PagesPerSecond => Time {
                micros: figures.render_micros,
                over: figures.pages,
            },
            Figure::OpenAndCount => Time {
                micros: figures.open_micros,
                over: figures.files,
            },
            Figure::P50 => Time {
                micros: figures.p50_micros,
                over: page,
            },
            Figure::P95 => Time {
                micros: figures.p95_micros,
                over: page,
            },
            Figure::P99 => Time {
                micros: figures.p99_micros,
                over: page,
            },
        }
    }

    /// The figure as a message states it.
    fn show(self, time: Time) -> String {
        let micros = time.mean();
        match self {
            Figure::PagesPerSecond => format!(
                "{:.1} pages/s ({micros:.1} µs a page)",
                1_000_000.0 / micros.max(f64::MIN_POSITIVE)
            ),
            Figure::OpenAndCount => format!("an open and count of {micros:.1} µs a file"),
            Figure::P50 => format!("a p50 page render of {micros:.0} µs"),
            Figure::P95 => format!("a p95 page render of {micros:.0} µs"),
            Figure::P99 => format!("a p99 page render of {micros:.0} µs"),
        }
    }
}

/// The band five runs set for one figure, in percent.
///
/// The runs' spread -- slowest over fastest, less one -- times 2.295, rounded
/// up to the next 5 %, and never under [`TIME_BAND_FLOOR_PERCENT`]. 2.295 is
/// the 1.53 the bench's widest spread grew by from five runs to fifteen, made
/// half again as wide, the bench's own rule: the band is the spread fifteen
/// runs would show, set as the bench sets one from fifteen. Integers
/// throughout, as every comparison here is.
fn band(fastest: Time, slowest: Time) -> u128 {
    // The spread as a fraction: (slowest - fastest) / fastest.
    let num = (u128::from(slowest.micros) * u128::from(fastest.over))
        .saturating_sub(u128::from(fastest.micros) * u128::from(slowest.over));
    let den = u128::from(fastest.micros) * u128::from(slowest.over);
    if den == 0 {
        return TIME_BAND_FLOOR_PERCENT;
    }
    // 2.295 x the spread x 100 %, in whole steps of 5 %.
    let steps = num.saturating_mul(2295).div_ceil(den.saturating_mul(50));
    steps.saturating_mul(5).max(TIME_BAND_FLOOR_PERCENT)
}

/// A committed ratchet.
#[derive(Clone, Debug, PartialEq)]
pub struct Ratchet {
    pub bars: Vec<Bar>,
    /// Whether the run that produced it covered everything.
    pub complete: bool,
    /// The `--fonts` setting it was measured under.
    pub fonts: String,
    /// The five runs its time figures come from, or `None` in a bar recorded
    /// before any were measured.
    ///
    /// `None` is an improvement rather than a refusal, for [`Bar::peak`]'s
    /// reason, and it is what keeps `SCHEMA` at 2.
    pub time_runs: Option<TimeRuns>,
    /// The dispatches its peak bands were read from, or `None` in a bar whose
    /// bands predate the block, which reads as it did.
    pub peak_runs: Option<PeakRuns>,
}

impl Ratchet {
    fn bar(&self, name: &str) -> Option<&Bar> {
        self.bars.iter().find(|b| b.name == name)
    }
}

/// The comparison itself: does the bar still hold?
///
/// Integers throughout. See the module note for why this is not a division.
#[must_use]
pub fn holds(passed_before: u64, total_before: u64, passed_now: u64, total_now: u64) -> bool {
    // `u128` because nothing forces a corpus to stay small, and a silent wrap
    // in the comparator that decides whether the engine regressed is the last
    // place anyone would look.
    let left = u128::from(passed_now) * u128::from(total_before);
    let right = u128::from(passed_before) * u128::from(total_now);
    left >= right
}

/// What a comparison found.
#[derive(Clone, Debug, Default)]
pub struct Comparison {
    /// Reasons the run is worse than the bar. Any entry fails CI.
    pub regressions: Vec<String>,
    /// Reasons the comparison could not be made at all. Also fails, and
    /// separately, because "refused to compare" and "compared and lost" want
    /// different fixes.
    pub refusals: Vec<String>,
    /// Numbers that moved the right way, or corpora that are new.
    pub improvements: Vec<String>,
    /// Everything else worth printing, degradation included.
    pub notes: Vec<String>,
}

impl Comparison {
    pub fn failed(&self) -> bool {
        !self.regressions.is_empty() || !self.refusals.is_empty()
    }
}

/// Compares a run against the committed bar.
pub fn compare(before: &Ratchet, now: &Run, strict: bool) -> Comparison {
    let mut out = Comparison::default();

    // Refused before anything is compared. A sampled run's numbers are not
    // wrong, they are about a different set of files, and letting them lower
    // the bar is how a corpus quietly stops being a corpus.
    if !now.complete() {
        out.refusals.push(format!(
            "this run is incomplete, so its counts are not comparable with a \
             recorded bar: {}",
            now.limits.join("; ")
        ));
    }
    if !before.complete {
        out.refusals.push(
            "the committed ratchet was recorded from an incomplete run and \
             cannot be a bar; re-record it from a complete one"
                .to_string(),
        );
    }
    if before.fonts != now.settings.fonts {
        out.refusals.push(format!(
            "the bar was recorded with fonts `{}` and this run used `{}`; the \
             two are different measurements, because this engine bundles no \
             faces and supplying them removes most of the warnings",
            before.fonts, now.settings.fonts
        ));
    }
    // The time figures are a measurement of their own, refused as `--fonts`
    // is when this run is not the measurement they were: a page rendered at
    // another resolution, or beside another number of neighbours, is another
    // span. `None` is a bar with no figures, `Some(false)` figures refused.
    let times = before.time_runs.as_ref().map(|runs| {
        let mut agree = true;
        if runs.dpi.to_bits() != now.settings.dpi.to_bits() {
            out.refusals.push(format!(
                "the time figures were measured at {} dpi and this run rendered at {}; a page rendered at another resolution is another measurement",
                runs.dpi, now.settings.dpi
            ));
            agree = false;
        }
        if runs.jobs != now.settings.jobs as u64 {
            out.refusals.push(format!(
                "the time figures were measured {} children at a time and this run ran {}; a span is wall time, and its neighbours are part of it",
                runs.jobs, now.settings.jobs
            ));
            agree = false;
        }
        agree
    });

    for bar in &before.bars {
        let Some(corpus) = now.corpora.iter().find(|c| c.name == bar.name) else {
            out.refusals.push(format!(
                "`{}` is in the ratchet and was not run; a bar cannot be \
                 cleared by not attempting it",
                bar.name
            ));
            continue;
        };
        let (total, passed) = (corpus.total(), corpus.passed());

        // Checked before the cross-multiplication, which would otherwise
        // approve it: with `total_now` at zero both sides are zero and the
        // comparison holds, so a corpus that failed to extract would clear
        // every bar in the file.
        if total == 0 && bar.total > 0 {
            out.refusals.push(format!(
                "`{}` ran zero files against a bar of {}; that is an empty \
                 directory, not a pass rate",
                bar.name, bar.total
            ));
            continue;
        }

        if holds(bar.passed, bar.total, passed, total) {
            if passed * bar.total > bar.passed * total {
                out.improvements.push(format!(
                    "{}: {passed}/{total} passed, up from {}/{}",
                    bar.name, bar.passed, bar.total
                ));
            }
        } else {
            out.regressions.push(format!(
                "{}: {passed}/{total} passed, which is worse than the recorded \
                 {}/{} ({passed} * {} = {} < {} = {} * {total})",
                bar.name,
                bar.passed,
                bar.total,
                bar.total,
                u128::from(passed) * u128::from(bar.total),
                u128::from(bar.passed) * u128::from(total),
                bar.passed,
            ));
            // Still a regression — the shortfall is real and this does not
            // forgive it. But a shortfall made entirely of timeouts is a
            // different fact from one made of files that stopped rendering,
            // and the message is the only place a reader learns which. Two
            // pdf.js files sit within a factor of two of the 20-second limit,
            // so a run competing with other work on the machine flips them,
            // and twice that arrived looking like an engine that had broken.
            let timed_out = corpus.outcomes().get("timed_out").copied().unwrap_or(0);
            let short = bar.passed.saturating_sub(passed);
            if timed_out > 0 && short <= timed_out {
                out.regressions.push(format!(
                    "{}: and {timed_out} of those timed out, which accounts for the whole shortfall of {short}. A file near the limit flips when the machine is busy; re-run before believing this is the engine",
                    bar.name
                ));
            }
        }

        // A child that stops finishing is a regression in its own name,
        // whatever the pass rate does (MS-20): two files the bar counts as
        // failing that now open lift the pass rate over a file that stalled.
        // A stall is named with the phase it stopped in, a crash with what
        // the child last said. `timed_out` stays the note beside the pass
        // rate it is, since a file near its limit flips with the machine.
        for word in ["stalled", "crashed"] {
            let counted = bar.outcomes.get(word).copied().unwrap_or(0);
            let ended: Vec<String> = corpus
                .files
                .iter()
                .filter_map(|file| match &file.outcome {
                    crate::runner::Outcome::Stalled { at } if word == "stalled" => Some(format!(
                        "{} at {}",
                        file.path,
                        if at.is_empty() { "no phase" } else { at }
                    )),
                    crate::runner::Outcome::Crashed(detail) if word == "crashed" => {
                        Some(format!("{} ({detail})", file.path))
                    }
                    _ => None,
                })
                .collect();
            if ended.len() as u64 > counted {
                out.regressions.push(format!(
                    "{}: {} files {word}, where the bar counts {counted}: {}",
                    bar.name,
                    ended.len(),
                    ended.join("; ")
                ));
            }
        }

        // The second axis. Lower is better, so the inequality is the mirror
        // of the one above: `degraded_now * total_before <= ...`.
        let degraded = corpus.degraded();
        let worse = u128::from(degraded) * u128::from(bar.total)
            > u128::from(bar.degraded) * u128::from(total);
        if worse {
            let message = format!(
                "{}: {degraded}/{total} rendered with warnings, up from {}/{}",
                bar.name, bar.degraded, bar.total
            );
            if strict {
                out.regressions.push(message);
            } else {
                out.notes
                    .push(format!("{message} (not a failure; --strict)"));
            }
        } else if degraded * bar.total < bar.degraded * total {
            out.improvements.push(format!(
                "{}: {degraded}/{total} rendered with warnings, down from {}/{}",
                bar.name, bar.degraded, bar.total
            ));
        }

        // The third axis (ruling 13): of the files this engine read cleanly,
        // how many produced a rewrite that holds up to ISO 32000 read
        // strictly. Only the structural tier counts, because a rewrite
        // inherits the page tree and the annotations from its source and a
        // backwards `/Rect` in somebody's 2003 invoice is the invoice's.
        //
        // Eligibility is compared as well as cleanliness. A change that made
        // fewer files readable would otherwise raise the rate by shrinking
        // the denominator, which is the same trick as sampling.
        let (eligible, clean) = (corpus.strict_eligible(), corpus.strict_clean());
        if eligible == 0 && bar.strict_eligible > 0 {
            out.regressions.push(format!(
                "{}: the strict pass ran on no file at all against a bar of {}",
                bar.name, bar.strict_eligible
            ));
        } else if !holds(bar.strict_clean, bar.strict_eligible, clean, eligible) {
            out.regressions.push(format!(
                "{}: {clean}/{eligible} rewrites validate, which is worse than the recorded {}/{}",
                bar.name, bar.strict_clean, bar.strict_eligible
            ));
        } else if u128::from(clean) * u128::from(bar.strict_eligible)
            > u128::from(bar.strict_clean) * u128::from(eligible)
        {
            out.improvements.push(format!(
                "{}: {clean}/{eligible} rewrites validate, up from {}/{}",
                bar.name, bar.strict_clean, bar.strict_eligible
            ));
        }
        if !holds(bar.strict_eligible, bar.total, eligible, total) {
            out.regressions.push(format!(
                "{}: the strict pass ran on {eligible}/{total} files, down from {}/{}; a smaller denominator is not a better rate",
                bar.name, bar.strict_eligible, bar.total
            ));
        }

        // The fourth axis (roadmap step 7): the metamorphic relations. Two
        // comparisons per relation and for the strict pass's reason — a
        // relation that quietly stopped being asked would otherwise raise its
        // own hold rate by shrinking what it is a rate of.
        let compared_now = corpus.metamorphic_compared();
        let held_now = corpus.metamorphic_held();
        for (relation, (compared_before, held_before)) in &bar.metamorphic {
            let compared = compared_now.get(relation).copied().unwrap_or(0);
            let held = held_now.get(relation).copied().unwrap_or(0);
            if compared == 0 && *compared_before > 0 {
                out.regressions.push(format!(
                    "{}: `{relation}` was asked of no file at all against a bar of {}",
                    bar.name, compared_before
                ));
                continue;
            }
            if !holds(*held_before, *compared_before, held, compared) {
                out.regressions.push(format!(
                    "{}: `{relation}` held on {held}/{compared}, worse than the \
                     recorded {held_before}/{compared_before}",
                    bar.name
                ));
            } else if u128::from(held) * u128::from(*compared_before)
                > u128::from(*held_before) * u128::from(compared)
            {
                out.improvements.push(format!(
                    "{}: `{relation}` held on {held}/{compared}, up from \
                     {held_before}/{compared_before}",
                    bar.name
                ));
            }
            if !holds(*compared_before, bar.total, compared, total) {
                out.regressions.push(format!(
                    "{}: `{relation}` was asked of {compared}/{total} files, down \
                     from {compared_before}/{}; declining the hard files is not a \
                     better rate",
                    bar.name, bar.total
                ));
            }
        }
        for relation in compared_now.keys() {
            if !bar.metamorphic.contains_key(relation) {
                out.improvements.push(format!(
                    "{}: `{relation}` held on {}/{} — new, with no recorded bar",
                    bar.name,
                    held_now.get(relation).copied().unwrap_or(0),
                    compared_now.get(relation).copied().unwrap_or(0),
                ));
            }
        }

        // Capabilities: a floor per name, and a floor for the reason above —
        // this is the only axis that fails when the *scanner* regresses rather
        // than the engine. Compared as absolute counts and not as rates,
        // because a capability is carried by a file or it is not and the
        // denominator is the same corpus on both sides; `total` is checked
        // separately above.
        let now_capabilities = corpus.capabilities();
        for (capability, before_count) in &bar.capabilities {
            let now_count = now_capabilities.get(capability).copied().unwrap_or(0);
            if now_count < *before_count {
                out.regressions.push(format!(
                    "{}: {now_count} files carry `{capability}` and the bar is \
                     {before_count} — the corpus did not change, so something \
                     stopped recognising it",
                    bar.name
                ));
            } else if now_count > *before_count {
                out.improvements.push(format!(
                    "{}: `{capability}` {before_count} to {now_count}",
                    bar.name,
                ));
            }
        }
        for (capability, count) in &now_capabilities {
            if !bar.capabilities.contains_key(capability) {
                out.improvements.push(format!(
                    "{}: `{capability}` is newly counted at {count}",
                    bar.name
                ));
            }
        }

        // The fifth axis (tagged PDF milestone 4): what the structure tree
        // walk reached. Three comparisons, because the three counts fail in
        // three different directions and one of them is not a floor.
        let now_tagged = corpus.tagged();
        match bar.tagged {
            // A first measurement of zero is not an improvement. It is a
            // corpus with no tagged files in it, or a walk that found none,
            // and the two are told apart by looking rather than by a word
            // that says the number went the right way.
            None if now_tagged.files == 0 => out.notes.push(format!(
                "{}: no file yielded a structure tree, and there is no recorded bar",
                bar.name
            )),
            None => out.improvements.push(format!(
                "{}: {} files yield a structure tree with {} elements — new, with no recorded bar",
                bar.name, now_tagged.files, now_tagged.elements
            )),
            Some(before) => {
                // (1) Files with a tree, as a share of the corpus. This is the
                // one the seeded regression trips: a walk that stops resolving
                // `/StructTreeRoot` finds no trees anywhere.
                if now_tagged.files == 0 && before.files > 0 {
                    out.regressions.push(format!(
                        "{}: no file yielded a structure tree at all, against a bar of {}",
                        bar.name, before.files
                    ));
                } else if !holds(before.files, bar.total, now_tagged.files, total) {
                    out.regressions.push(format!(
                        "{}: {}/{total} files yield a structure tree, down from {}/{}",
                        bar.name, now_tagged.files, before.files, bar.total
                    ));
                } else if u128::from(now_tagged.files) * u128::from(bar.total)
                    > u128::from(before.files) * u128::from(total)
                {
                    out.improvements.push(format!(
                        "{}: {}/{total} files yield a structure tree, up from {}/{}",
                        bar.name, now_tagged.files, before.files, bar.total
                    ));
                }

                // (2) Elements per file in the corpus, not per tagged file.
                // Per tagged file would let a walk that lost the large trees
                // hold its rate by also losing the small ones.
                if !holds(before.elements, bar.total, now_tagged.elements, total) {
                    out.regressions.push(format!(
                        "{}: the walk reached {} structure elements over {total} files, down from {} over {}",
                        bar.name, now_tagged.elements, before.elements, bar.total
                    ));
                }

                // (3) Orphans are the count that is better small, so the
                // comparison is on the share of marked characters the tree
                // *claimed* — matched against matched-plus-orphaned. An
                // absolute ceiling on orphans would be regressed by reading
                // more files, which is the opposite of what it is for.
                let (m, o) = (now_tagged.matched, now_tagged.orphans);
                if !holds(before.matched, before.matched + before.orphans, m, m + o) {
                    out.regressions.push(format!(
                        "{}: the tree claimed {m} of {} marked characters, a smaller share than the recorded {} of {}",
                        bar.name,
                        m + o,
                        before.matched,
                        before.matched + before.orphans
                    ));
                }
            }
        }

        // The sixth axis (tier 0's memory row): the most memory any one child
        // needed. See [`PeakBar`] for why this one is an absolute `<=` where
        // every other axis in this file is a ratio.
        let now_peak = corpus.peak();
        match bar.peak {
            // No bar, and nothing measured either. Said rather than passed
            // over, because a corpus whose children report no peak is a run
            // that is already incomplete — `corpus::run` puts that in
            // `limits`, and the refusal at the top of this function is what
            // actually stops it. This note is how a reader learns which of the
            // two silences they are looking at.
            None if now_peak.files == 0 => out.notes.push(format!(
                "{}: no child reported a peak resident set, and there is no recorded bar",
                bar.name
            )),
            None => out.improvements.push(format!(
                "{}: the largest peak resident set is {} bytes over {} children — new, with no recorded bar",
                bar.name, now_peak.bytes, now_peak.files
            )),
            Some(before) => {
                if now_peak.files == 0 {
                    out.regressions.push(format!(
                        "{}: no child reported a peak resident set at all, against a bar of {} bytes over {} children",
                        bar.name, before.bytes, before.files
                    ));
                } else if u128::from(now_peak.bytes) * 100
                    > u128::from(before.bytes) * u128::from(100 + PEAK_TOLERANCE_PERCENT)
                {
                    out.regressions.push(format!(
                        "{}: the largest peak resident set is {} bytes, above the recorded band of {} bytes and its {PEAK_TOLERANCE_PERCENT} % tolerance",
                        bar.name, now_peak.bytes, before.bytes
                    ));
                } else if now_peak.bytes < before.bytes {
                    out.improvements.push(format!(
                        "{}: the largest peak resident set is {} bytes, down from {}",
                        bar.name, now_peak.bytes, before.bytes
                    ));
                }
                // And the denominator, for the strict pass's reason: a
                // maximum over half as many children is a smaller maximum
                // without the engine having improved. A ratio, because this
                // one *is* a share of the corpus.
                if !holds(before.files, bar.total, now_peak.files, total) {
                    out.regressions.push(format!(
                        "{}: {}/{total} children reported a peak resident set, down from {}/{}; a maximum over fewer children is not a smaller maximum",
                        bar.name, now_peak.files, before.files, bar.total
                    ));
                }
            }
        }

        // The seventh axis (MS-07): time, each figure held to the band its
        // five runs set.
        let now_time = corpus.time();
        match (times, &bar.time) {
            // Refused above, once for the whole bar.
            (Some(false), _) => {}
            (Some(true), Some(time)) => hold_times(&mut out, &bar.name, total, &now_time, time),
            // No band: a bar recorded before the times, or a corpus new to
            // them. A first measurement sets none, for the peak's reason.
            _ if now_time.files == 0 && now_time.pages == 0 => out.notes.push(format!(
                "{}: no open or page render was timed, and there is no recorded band",
                bar.name
            )),
            _ => out.improvements.push(format!(
                "{}: {} over {} pages, and {} over {} files — new, with no recorded band",
                bar.name,
                Figure::PagesPerSecond.show(Figure::PagesPerSecond.time(&now_time)),
                now_time.pages,
                Figure::OpenAndCount.show(Figure::OpenAndCount.time(&now_time)),
                now_time.files
            )),
        }
    }

    for corpus in &now.corpora {
        if before.bar(&corpus.name).is_none() {
            out.improvements.push(format!(
                "{}: {}/{} passed — new, with no recorded bar",
                corpus.name,
                corpus.passed(),
                corpus.total()
            ));
        }
    }

    out
}

/// Holds one corpus's time figures to the bands its five runs set (MS-07).
///
/// Each figure is judged alone, so one past its band fails the run by itself.
fn hold_times(out: &mut Comparison, name: &str, total: u64, now: &TimeFigures, time: &TimeBar) {
    if total != time.total {
        out.refusals.push(format!(
            "`{name}` ran {total} files and its times were measured over {}; a corpus that changed is another measurement until five runs re-measure it",
            time.total
        ));
        return;
    }
    // A figure over fewer files or pages is a smaller measurement, not a
    // faster one: each is held to the fewest of the five, as a peak is to its
    // children.
    let fewest_files = time.runs.iter().map(|run| run.files).min().unwrap_or(0);
    if now.files < fewest_files {
        out.regressions.push(format!(
            "{name}: the open of {} files was timed, fewer than the {fewest_files} the fewest of the five runs timed; a figure over fewer files is not a faster one",
            now.files
        ));
    }
    let fewest_pages = time.runs.iter().map(|run| run.pages).min().unwrap_or(0);
    if now.pages < fewest_pages {
        out.regressions.push(format!(
            "{name}: {} page renders were timed, fewer than the {fewest_pages} the fewest of the five runs timed; a figure over fewer pages is not a faster one",
            now.pages
        ));
    }
    for figure in Figure::ALL {
        let runs: Vec<Time> = time.runs.iter().map(|run| figure.time(run)).collect();
        let now_time = figure.time(now);
        // A figure a run did not measure is held to nothing; the counts above
        // are what catch a measurement that stopped.
        if now_time.over == 0 || runs.iter().any(|run| run.over == 0) {
            continue;
        }
        let fastest = runs
            .iter()
            .copied()
            .reduce(|a, b| if b.at_most(a) { b } else { a });
        let slowest = runs
            .iter()
            .copied()
            .reduce(|a, b| if a.at_most(b) { b } else { a });
        let (Some(fastest), Some(slowest)) = (fastest, slowest) else {
            continue;
        };
        let band = band(fastest, slowest);
        if !now_time.within(fastest, band) {
            out.regressions.push(format!(
                "{name}: {}, past its band of {band} % over the five runs' fastest, {}",
                figure.show(now_time),
                figure.show(fastest)
            ));
        } else if !fastest.at_most(now_time) {
            // Faster than all five. One run measures no band, so it moves
            // nothing and is said rather than counted.
            out.notes.push(format!(
                "{name}: {}, faster than the five runs' fastest, {}; one run measures no band",
                figure.show(now_time),
                figure.show(fastest)
            ));
        }
    }
}

/// The bar `--record` writes: `recorded`, every axis from its run, with the
/// time figures of `committed` carried over unchanged (MS-07).
///
/// A time figure from one run would ratchet the bar onto whichever host that
/// run landed on, and the next run on the fleet's other group would fail on
/// the machine rather than on the code. So a run writes none: `time_runs` and
/// each corpus's `time` are the five runs' the committed bar holds, and stay
/// until five more re-measure them. A corpus the run no longer has takes its
/// block with it.
#[must_use]
pub fn carry_times(committed: &Json, recorded: Json) -> Json {
    let Json::Object(mut top) = recorded else {
        return recorded;
    };
    if let Some(runs) = committed.get("time_runs") {
        top.insert("time_runs".to_string(), runs.clone());
    }
    let blocks: BTreeMap<&str, &Json> = committed
        .get("corpora")
        .and_then(Json::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|corpus| Some((corpus.get("name")?.as_str()?, corpus.get("time")?)))
        .collect();
    if let Some(Json::Array(corpora)) = top.get_mut("corpora") {
        for corpus in corpora {
            let Json::Object(fields) = corpus else {
                continue;
            };
            let block = fields
                .get("name")
                .and_then(Json::as_str)
                .and_then(|name| blocks.get(name))
                .map(|block| (*block).clone());
            if let Some(block) = block {
                fields.insert("time".to_string(), block);
            }
        }
    }
    Json::Object(top)
}

/// The bar `--record` writes, with `committed`'s peak bands and the runs they
/// were read from carried over unchanged, and none of `recorded`'s own
/// (MS-19).
///
/// A band from one run would set the bar on whichever machine that run landed
/// on, so a run writes none: each corpus's `peak` and the bar's `peak_runs`
/// are the dispatches' `corpus-peaks --record` read them from, and stay until
/// it reads them again. A corpus the committed bar holds no band for gets
/// none, and one the run no longer has takes its band with it.
#[must_use]
pub fn carry_peaks(committed: Option<&Json>, recorded: Json) -> Json {
    let Json::Object(mut top) = recorded else {
        return recorded;
    };
    top.remove("peak_runs");
    if let Some(runs) = committed.and_then(|committed| committed.get("peak_runs")) {
        top.insert("peak_runs".to_string(), runs.clone());
    }
    let bands: BTreeMap<&str, &Json> = committed
        .and_then(|committed| committed.get("corpora"))
        .and_then(Json::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(|corpus| Some((corpus.get("name")?.as_str()?, corpus.get("peak")?)))
        .collect();
    if let Some(Json::Array(corpora)) = top.get_mut("corpora") {
        for corpus in corpora {
            let Json::Object(fields) = corpus else {
                continue;
            };
            let band = fields
                .get("name")
                .and_then(Json::as_str)
                .and_then(|name| bands.get(name))
                .map(|band| (*band).clone());
            match band {
                Some(band) => {
                    fields.insert("peak".to_string(), band);
                }
                None => {
                    fields.remove("peak");
                }
            }
        }
    }
    Json::Object(top)
}

/// Reads a bar's `time_runs`.
fn parse_time_runs(object: &Json) -> Result<TimeRuns, String> {
    let dpi = match object.get("dpi") {
        Some(Json::Number(dpi)) if dpi.is_finite() && *dpi > 0.0 => *dpi,
        _ => return Err("`time_runs` has no `dpi`".to_string()),
    };
    let jobs = object
        .get("jobs")
        .and_then(Json::as_u64)
        .filter(|jobs| *jobs > 0)
        .ok_or("`time_runs` has no whole-number `jobs`")?;
    let text = |json: &Json, key: &str| -> Result<String, String> {
        json.get(key)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("`time_runs` has no `{key}`"))
    };
    let dispatches = parse_dispatches(object, "time_runs")?;
    if dispatches.len() != TIME_RUNS {
        return Err(format!(
            "`time_runs` names {} dispatches, and a band is set from {TIME_RUNS}",
            dispatches.len()
        ));
    }
    Ok(TimeRuns {
        dpi,
        jobs,
        revision: text(object, "revision")?,
        recorded: text(object, "recorded")?,
        dispatches,
    })
}

/// Reads a block's `dispatches`, each run's id and the host it landed on.
fn parse_dispatches(object: &Json, block: &str) -> Result<Vec<Dispatch>, String> {
    let text = |json: &Json, key: &str| -> Result<String, String> {
        json.get(key)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("`{block}` has no `{key}`"))
    };
    object
        .get("dispatches")
        .and_then(Json::as_array)
        .ok_or_else(|| format!("`{block}` has no `dispatches` array"))?
        .iter()
        .map(|dispatch| {
            Ok(Dispatch {
                id: text(dispatch, "id")?,
                host: text(dispatch, "host")?,
            })
        })
        .collect()
}

/// Reads a bar's `peak_runs`: the revision and the [`PEAK_RUNS`] dispatches
/// its peak bands were read from, none of them twice.
fn parse_peak_runs(object: &Json) -> Result<PeakRuns, String> {
    let revision = object
        .get("revision")
        .and_then(Json::as_str)
        .filter(|revision| !revision.is_empty())
        .ok_or("`peak_runs` has no `revision`")?
        .to_string();
    let dispatches = parse_dispatches(object, "peak_runs")?;
    if dispatches.len() != PEAK_RUNS {
        return Err(format!(
            "`peak_runs` names {} dispatches, and a band is read from {PEAK_RUNS}",
            dispatches.len()
        ));
    }
    for (index, dispatch) in dispatches.iter().enumerate() {
        if dispatches[..index]
            .iter()
            .any(|other| other.id == dispatch.id)
        {
            return Err(format!("`peak_runs` names dispatch {} twice", dispatch.id));
        }
    }
    Ok(PeakRuns {
        revision,
        dispatches,
    })
}

/// Reads one corpus's `time`, which needs the bar's `time_runs` to say what
/// its runs were.
fn parse_time(object: &Json, name: &str, time_runs: Option<&TimeRuns>) -> Result<TimeBar, String> {
    let Some(time_runs) = time_runs else {
        return Err(format!(
            "`{name}` records times and the bar has no `time_runs` naming the runs they came from"
        ));
    };
    let total = object
        .get("total")
        .and_then(Json::as_u64)
        .ok_or_else(|| format!("`{name}`'s `time` has no whole-number `total`"))?;
    let runs = object
        .get("runs")
        .and_then(Json::as_array)
        .ok_or_else(|| format!("`{name}`'s `time` has no `runs` array"))?;
    if runs.len() != time_runs.dispatches.len() {
        return Err(format!(
            "`{name}` records {} timed runs and `time_runs` names {} dispatches; each run is one dispatch's",
            runs.len(),
            time_runs.dispatches.len()
        ));
    }
    let runs = runs
        .iter()
        .map(|run| TimeFigures::from_json(run, name))
        .collect::<Result<Vec<_>, String>>()?;
    for run in &runs {
        if run.files > total {
            return Err(format!(
                "`{name}` records the open of {} files timed out of {total}, which cannot be",
                run.files
            ));
        }
        // Refused at the bar, for a peak of zero bytes' reason: a band set
        // from zero is a band nothing can sit under.
        let zero_open = run.files > 0 && run.open_micros == 0;
        let zero_page = run.pages > 0
            && [
                run.render_micros,
                run.p50_micros,
                run.p95_micros,
                run.p99_micros,
            ]
            .contains(&0);
        if zero_open || zero_page {
            return Err(format!(
                "`{name}` records a time of zero over {} files and {} pages that were timed, which is a band nothing can sit under",
                run.files, run.pages
            ));
        }
    }
    Ok(TimeBar { total, runs })
}

/// Reads a committed ratchet.
pub fn parse(text: &str) -> Result<Ratchet, String> {
    let json = crate::json::parse(text)?;
    let schema = json
        .get("schema")
        .and_then(Json::as_u64)
        .ok_or("the ratchet has no `schema`")?;
    if schema != crate::report::SCHEMA {
        return Err(format!(
            "the ratchet is schema {schema} and this build writes schema {}; \
             a bar read under the wrong schema is not a bar",
            crate::report::SCHEMA
        ));
    }

    // Absent means false. A ratchet that forgot to say whether its run was
    // complete must not be assumed to have been.
    let complete = matches!(json.get("complete"), Some(Json::Bool(true)));
    let fonts = json
        .get("settings")
        .and_then(|s| s.get("fonts"))
        .and_then(Json::as_str)
        .ok_or("the ratchet does not say what `--fonts` it was measured with")?
        .to_string();
    // Absent in a bar recorded before the time figures, which compares
    // against nothing on that axis and refuses nothing.
    let time_runs = match json.get("time_runs") {
        None => None,
        Some(object) => Some(parse_time_runs(object)?),
    };
    // Absent in a bar whose peak bands predate it, which reads as it did.
    let peak_runs = match json.get("peak_runs") {
        None => None,
        Some(object) => Some(parse_peak_runs(object)?),
    };

    let entries = json
        .get("corpora")
        .and_then(Json::as_array)
        .ok_or("the ratchet has no `corpora` array")?;
    let mut bars = Vec::new();
    for entry in entries {
        let name = entry
            .get("name")
            .and_then(Json::as_str)
            .ok_or("a corpus in the ratchet has no name")?
            .to_string();
        let count = |key: &str| -> Result<u64, String> {
            entry
                .get(key)
                .and_then(Json::as_u64)
                .ok_or_else(|| format!("`{name}` has no whole-number `{key}`"))
        };
        let total = count("total")?;
        let passed = count("passed")?;
        let degraded = count("degraded")?;
        let strict_eligible = count("strict_eligible")?;
        let strict_clean = count("strict_clean")?;
        let mut metamorphic = BTreeMap::new();
        if let Some(Json::Object(relations)) = entry.get("metamorphic") {
            for (relation, verdict) in relations {
                let read = |key: &str| -> Result<u64, String> {
                    verdict.get(key).and_then(Json::as_u64).ok_or_else(|| {
                        format!("`{name}`'s `{relation}` has no whole-number `{key}`")
                    })
                };
                let (compared, held) = (read("compared")?, read("held")?);
                if held > compared || compared > total {
                    return Err(format!(
                        "`{name}` records `{relation}` holding on {held} of {compared} \
                         files out of {total}, which cannot be"
                    ));
                }
                metamorphic.insert(relation.clone(), (compared, held));
            }
        }
        if passed > total || degraded > total {
            return Err(format!(
                "`{name}` records {passed} passed and {degraded} degraded out \
                 of {total}, which cannot be"
            ));
        }
        if strict_eligible > total || strict_clean > strict_eligible {
            return Err(format!(
                "`{name}` records {strict_clean} clean rewrites out of \
                 {strict_eligible} eligible of {total}, which cannot be"
            ));
        }
        // Absent in a bar recorded before this axis: an empty map compares
        // against nothing and refuses nothing.
        let mut capabilities: BTreeMap<String, u64> = BTreeMap::new();
        if let Some(object) = entry.get("capabilities") {
            let Json::Object(members) = object else {
                return Err(format!("`{name}`'s `capabilities` is not an object"));
            };
            for (capability, value) in members {
                let count = value.as_u64().ok_or_else(|| {
                    format!("`{name}`'s `capabilities.{capability}` is not a whole number")
                })?;
                if count > total {
                    return Err(format!(
                        "`{name}` says {count} files carry `{capability}` and the \
                         corpus is {total} files"
                    ));
                }
                capabilities.insert(capability.clone(), count);
            }
        }

        let mut outcomes: BTreeMap<String, u64> = BTreeMap::new();
        if let Some(object) = entry.get("outcomes") {
            let Json::Object(members) = object else {
                return Err(format!("`{name}`'s `outcomes` is not an object"));
            };
            for (word, value) in members {
                let count = value
                    .as_u64()
                    .ok_or_else(|| format!("`{name}`'s `outcomes.{word}` is not a whole number"))?;
                if count > total {
                    return Err(format!(
                        "`{name}` says {count} files ended {word} and the corpus is {total} files"
                    ));
                }
                outcomes.insert(word.clone(), count);
            }
        }

        let tagged = match entry.get("tagged") {
            None => None,
            Some(object) => {
                let read = |key: &str| -> Result<u64, String> {
                    object
                        .get(key)
                        .and_then(Json::as_u64)
                        .ok_or_else(|| format!("`{name}`'s `tagged` has no whole-number `{key}`"))
                };
                let tagged = TaggedBar {
                    files: read("files")?,
                    elements: read("elements")?,
                    matched: read("matched")?,
                    orphans: read("orphans")?,
                };
                if tagged.files > total {
                    return Err(format!(
                        "`{name}` records {} files with a structure tree out of {total}, which cannot be",
                        tagged.files
                    ));
                }
                // A bar with trees but no elements is what the seeded
                // regression writes, and it must be refused at the *bar*
                // rather than only compared against: a committed floor of
                // zero elements is a floor nothing can fall below.
                if tagged.files > 0 && tagged.elements == 0 {
                    return Err(format!(
                        "`{name}` records {} files with a structure tree and no structure elements at all, which is a floor nothing can fall below",
                        tagged.files
                    ));
                }
                Some(tagged)
            }
        };
        let peak = match entry.get("peak") {
            // Absent is a bar recorded before the measurement existed, which
            // is every bar committed today. `SCHEMA` stays 2 for exactly this:
            // no committed file has to be rewritten to stay readable, and none
            // of them can be regressed against on an axis they never measured.
            None => None,
            Some(object) => {
                let read = |key: &str| -> Result<u64, String> {
                    object
                        .get(key)
                        .and_then(Json::as_u64)
                        .ok_or_else(|| format!("`{name}`'s `peak` has no whole-number `{key}`"))
                };
                let peak = PeakBar {
                    bytes: read("bytes")?,
                    files: read("files")?,
                };
                if peak.files > total {
                    return Err(format!(
                        "`{name}` records a peak resident set over {} children out of {total}, which cannot be",
                        peak.files
                    ));
                }
                // Refused at the *bar* rather than only compared against, for
                // the reason a tagged bar of zero elements is: a band of zero
                // bytes over children that reported something is a ceiling
                // nothing can sit under, so every future run would regress
                // against it and the message would name memory rather than
                // the ratchet that was written wrong.
                if peak.files > 0 && peak.bytes == 0 {
                    return Err(format!(
                        "`{name}` records {} children reporting a peak resident set of zero bytes, which is a band nothing can sit under",
                        peak.files
                    ));
                }
                // And the mirror: a band with no children behind it is not a
                // measurement, so it must not be read as one.
                if peak.files == 0 {
                    return Err(format!(
                        "`{name}` records a peak resident set of {} bytes over no children at all",
                        peak.bytes
                    ));
                }
                Some(peak)
            }
        };
        let time = match entry.get("time") {
            None => None,
            Some(object) => Some(parse_time(object, &name, time_runs.as_ref())?),
        };
        if bars.iter().any(|b: &Bar| b.name == name) {
            return Err(format!("`{name}` appears twice in the ratchet"));
        }
        bars.push(Bar {
            name,
            total,
            passed,
            degraded,
            strict_eligible,
            strict_clean,
            metamorphic,
            tagged,
            capabilities,
            peak,
            time,
            outcomes,
        });
    }
    if bars.is_empty() {
        return Err("the ratchet records no corpora at all".to_string());
    }
    Ok(Ratchet {
        bars,
        complete,
        fonts,
        time_runs,
        peak_runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{CorpusReport, Settings};
    use crate::runner::{FileResult, Outcome, PageTime, Tagged};
    use std::collections::{BTreeMap, BTreeSet};

    fn files(passed: u64, failed: u64, degraded: u64) -> Vec<FileResult> {
        let mut out = Vec::new();
        for index in 0..passed + failed {
            let outcome = if index < passed {
                Outcome::Passed
            } else {
                Outcome::Failed("no".to_string())
            };
            let warnings = if index < degraded {
                BTreeMap::from([("render:UnreadableFont".to_string(), 1usize)])
            } else {
                BTreeMap::new()
            };
            out.push(FileResult {
                path: format!("{index}.pdf"),
                outcome,
                cost: crate::runner::Cost::default(),
                bundled_faces: false,
                pages: 1,
                rendered: 1,
                warnings,
                capabilities: BTreeSet::new(),
                millis: 1,
                open_micros: None,
                page_micros: Vec::new(),
                strict: crate::runner::Strict::Checked {
                    structure: 0,
                    semantics: 0,
                    kinds: BTreeMap::new(),
                },
                metamorphic: BTreeMap::new(),
                tagged: None,
                producer: None,
                peak: None,
                marks: Vec::new(),
                rss_file: None,
                last: None,
            });
        }
        out
    }

    /// A run where `timed_out` of the non-passing files ran out of time
    /// rather than failing.
    fn run_with_timeouts(name: &str, passed: u64, timed_out: u64) -> Run {
        let mut run = run(name, passed, timed_out, 0);
        for file in run.corpora[0].files.iter_mut().skip(passed as usize) {
            file.outcome = Outcome::TimedOut {
                at: "page 1".to_string(),
            };
        }
        run
    }

    /// A shortfall made entirely of timeouts is still a regression, and it
    /// says so — but it also says *why*, because the two causes want
    /// different responses.
    ///
    /// This is not hypothetical. Two pdf.js files render within a factor of
    /// two of the 20-second limit, so a run competing with a build for the
    /// machine flips them, and the result arrived twice looking like an
    /// engine that had stopped rendering.
    ///
    /// Counted injection: removing the explanation leaves 1 assertion
    /// failing here and 0 anywhere else — which is the point of writing it
    /// down, since nothing else in the suite looks at the cause of a
    /// shortfall.
    #[test]
    fn a_shortfall_made_of_timeouts_is_still_a_regression_and_says_which() {
        let committed = bar("pdfjs", 963, 974, 0);
        let out = compare(&committed, &run_with_timeouts("pdfjs", 962, 12), true);
        assert!(out.failed(), "a timeout is not forgiven: {out:?}");
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("timed out") && r.contains("accounts for the whole shortfall")),
            "{:?}",
            out.regressions
        );
    }

    /// A bar recorded with eight of ten passing, two failing and no file
    /// stalled or crashed.
    fn bar_with_outcomes() -> Ratchet {
        let mut committed = bar("safedocs", 8, 10, 0);
        committed.bars[0].outcomes = BTreeMap::from([
            ("passed".to_string(), 8),
            ("failed".to_string(), 2),
            ("crashed".to_string(), 0),
            ("timed_out".to_string(), 0),
        ]);
        committed
    }

    /// **A file that stops finishing is a regression however the pass rate
    /// moves**: one file more passes and one stalls, so the pass rate reads
    /// better, and the stall fails the run by name, at the phase it stopped
    /// in.
    #[test]
    fn a_file_that_stops_finishing_is_a_regression_however_the_pass_rate_moves() {
        let mut now = run("safedocs", 9, 1, 0);
        now.corpora[0].files[9].outcome = Outcome::Stalled {
            at: "page 1/5".to_string(),
        };
        let out = compare(&bar_with_outcomes(), &now, false);
        assert!(out.failed(), "{out:#?}");
        assert!(
            out.improvements
                .iter()
                .any(|i| i.contains("9/10 passed, up from 8/10")),
            "the pass rate reads better: {:?}",
            out.improvements
        );
        assert!(
            out.regressions.iter().any(|r| r
                .contains("safedocs: 1 files stalled, where the bar counts 0: 9.pdf at page 1/5")),
            "{:?}",
            out.regressions
        );
    }

    /// A crash the bar does not count is a regression too, named with what
    /// the child last said.
    #[test]
    fn a_crash_its_bar_does_not_count_is_a_regression() {
        let mut now = run("safedocs", 9, 1, 0);
        now.corpora[0].files[9].outcome = Outcome::Crashed("thread 'main' overflowed".to_string());
        let out = compare(&bar_with_outcomes(), &now, false);
        assert!(
            out.regressions.iter().any(|r| r.contains(
                "safedocs: 1 files crashed, where the bar counts 0: 9.pdf (thread 'main' overflowed)"
            )),
            "{:?}",
            out.regressions
        );

        // One the bar counts is not.
        let mut counted = bar_with_outcomes();
        counted.bars[0].outcomes.insert("crashed".to_string(), 1);
        let out = compare(&counted, &now, false);
        assert!(
            !out.regressions.iter().any(|r| r.contains("crashed")),
            "{:?}",
            out.regressions
        );
    }

    /// The committed bars state no `stalled`, and a bar without the key, or
    /// without `outcomes` at all, counts none: a run with no stall holds and
    /// one with a stall does not.
    #[test]
    fn a_bar_without_a_stalled_count_counts_none() {
        for text in [
            r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
                "corpora":[{"name":"safedocs","total":10,"passed":8,"degraded":0,
                "strict_eligible":10,"strict_clean":10,
                "outcomes":{"crashed":0,"failed":2,"passed":8,"timed_out":0}}]}"#,
            r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
                "corpora":[{"name":"safedocs","total":10,"passed":8,"degraded":0,
                "strict_eligible":10,"strict_clean":10}]}"#,
        ] {
            let committed = parse(text).expect("the bar reads");
            assert_eq!(committed.bars[0].outcomes.get("stalled"), None);
            let held = compare(&committed, &run("safedocs", 9, 1, 0), false);
            assert!(!held.failed(), "{held:#?}");
            let mut stalled = run("safedocs", 9, 1, 0);
            stalled.corpora[0].files[9].outcome = Outcome::Stalled { at: String::new() };
            let out = compare(&committed, &stalled, false);
            assert!(
                out.regressions
                    .iter()
                    .any(|r| r
                        .contains("1 files stalled, where the bar counts 0: 9.pdf at no phase")),
                "{:?}",
                out.regressions
            );
        }
    }

    /// A shortfall bigger than the timeout count is not explained by them,
    /// and the message must not say it is.
    ///
    /// The dangerous direction: a run with one flaky timeout *and* ten real
    /// failures would otherwise be read as "just the machine being busy".
    #[test]
    fn a_shortfall_larger_than_the_timeouts_is_not_explained_away() {
        let committed = bar("pdfjs", 963, 974, 0);
        // 950 passed against a bar of 963 is 13 short, and only one timed out.
        let mut now = run_with_timeouts("pdfjs", 950, 24);
        for file in now.corpora[0].files.iter_mut().skip(951) {
            file.outcome = Outcome::Failed("no".to_string());
        }
        let out = compare(&committed, &now, true);
        assert!(out.failed());
        assert!(
            !out.regressions
                .iter()
                .any(|r| r.contains("accounts for the whole shortfall")),
            "{:?}",
            out.regressions
        );
    }

    /// A run whose strict pass ran on every file and found nothing.
    fn run(name: &str, passed: u64, failed: u64, degraded: u64) -> Run {
        Run {
            corpora: vec![CorpusReport {
                name: name.to_string(),
                files: files(passed, failed, degraded),
            }],
            limits: Vec::new(),
            settings: Settings {
                timeout_seconds: 20,
                dpi: 72.0,
                fonts: "none".to_string(),
                jobs: 4,
                host: None,
            },
        }
    }

    fn bar(name: &str, passed: u64, total: u64, degraded: u64) -> Ratchet {
        Ratchet {
            bars: vec![Bar {
                name: name.to_string(),
                total,
                passed,
                degraded,
                strict_eligible: total,
                strict_clean: total,
                metamorphic: BTreeMap::new(),
                tagged: None,
                capabilities: BTreeMap::new(),
                peak: None,
                time: None,
                outcomes: BTreeMap::new(),
            }],
            complete: true,
            fonts: "none".to_string(),
            time_runs: None,
            peak_runs: None,
        }
    }

    /// A run of ten files, each opened in `open` µs, whose twenty pages took
    /// `pages`, two to a file.
    fn timed_run(name: &str, open: u64, pages: [u64; 20]) -> Run {
        let mut run = run(name, 10, 0, 0);
        for (index, file) in run.corpora[0].files.iter_mut().enumerate() {
            file.pages = 2;
            file.rendered = 2;
            file.open_micros = Some(open);
            file.page_micros = vec![
                PageTime {
                    page: 1,
                    micros: pages[2 * index],
                },
                PageTime {
                    page: 2,
                    micros: pages[2 * index + 1],
                },
            ];
        }
        run
    }

    /// Five runs of that corpus: ten files opened in 3 000 µs each, twenty
    /// pages at 2 000 to 2 080 µs a page, a p50 of 2 000 µs, a p95 of 4 000
    /// and a p99 of 5 000 to 5 200. Every spread is 4 % or none, so every
    /// band is the floor's 20 %.
    fn five_runs() -> [TimeFigures; 5] {
        std::array::from_fn(|index| {
            let index = index as u64;
            TimeFigures {
                files: 10,
                open_micros: 30_000,
                pages: 20,
                render_micros: 40_000 + 400 * index,
                p50_micros: 2_000,
                p95_micros: 4_000,
                p99_micros: 5_000 + 50 * index,
            }
        })
    }

    fn timed_bar(name: &str, runs: [TimeFigures; 5]) -> Ratchet {
        let mut committed = bar(name, 10, 10, 0);
        committed.time_runs = Some(TimeRuns {
            dpi: 72.0,
            jobs: 4,
            revision: "0123abcd".to_string(),
            recorded: "the five runs".to_string(),
            dispatches: (1..=5)
                .map(|id| Dispatch {
                    id: id.to_string(),
                    host: "a machine, 4 logical CPUs".to_string(),
                })
                .collect(),
        });
        committed.bars[0].time = Some(TimeBar {
            total: 10,
            runs: runs.to_vec(),
        });
        committed
    }

    /// A committed bar's text with a time block: `dispatches` dispatches and
    /// these runs' figures, each `(files, open_micros, p99_micros)` over
    /// twenty pages.
    fn timed_bar_text(dispatches: usize, runs: &[(u64, u64, u64)]) -> String {
        let dispatches: Vec<String> = (0..dispatches)
            .map(|id| format!(r#"{{"id": "{id}", "host": "a machine"}}"#))
            .collect();
        let runs: Vec<String> = runs
            .iter()
            .map(|(files, open, p99)| {
                format!(
                    r#"{{"files": {files}, "open_micros": {open}, "pages": 20,
                    "render_micros": 40000, "p50_micros": 2000, "p95_micros": 4000,
                    "p99_micros": {p99}}}"#
                )
            })
            .collect();
        format!(
            r#"{{"schema": 2, "complete": true, "settings": {{"fonts": "none"}},
            "time_runs": {{"dpi": 72, "jobs": 4, "revision": "0123abcd",
            "recorded": "the five runs", "dispatches": [{}]}},
            "corpora": [{{"name": "pdfjs", "total": 10, "passed": 10, "degraded": 0,
            "strict_eligible": 10, "strict_clean": 10,
            "time": {{"total": 10, "runs": [{}]}}}}]}}"#,
            dispatches.join(", "),
            runs.join(", ")
        )
    }

    /// **One figure past its band fails the run, alone.** The p99 here is
    /// 7 000 µs against the five runs' fastest 5 000 and a band of 20 %; the
    /// mean page, the open and the p50 sit inside theirs, and the p95 is
    /// faster than all five, which is a note and moves nothing.
    #[test]
    fn a_time_figure_past_its_band_fails_alone() {
        let committed = timed_bar("pdfjs", five_runs());
        let mut pages = [2_000; 20];
        pages[7] = 7_000;
        let out = compare(&committed, &timed_run("pdfjs", 3_000, pages), true);
        assert!(out.failed(), "{out:#?}");
        assert!(out.refusals.is_empty(), "{:?}", out.refusals);
        assert_eq!(out.regressions.len(), 1, "{:?}", out.regressions);
        assert!(
            out.regressions[0].contains(
                "pdfjs: a p99 page render of 7000 µs, past its band of 20 % over the five runs' fastest, a p99 page render of 5000 µs"
            ),
            "{:?}",
            out.regressions
        );
        assert!(
            out.notes
                .iter()
                .any(|n| n
                    .contains("a p95 page render of 2000 µs, faster than the five runs' fastest")),
            "{:?}",
            out.notes
        );

        // Inside every band, nothing fails.
        let out = compare(&committed, &timed_run("pdfjs", 3_000, [2_000; 20]), true);
        assert!(!out.failed(), "{out:#?}");
    }

    /// The bars committed before the time figures carry none. A bar that
    /// predates a measurement cannot be regressed against, so the run's
    /// figures are new rather than refused, which is what keeps `SCHEMA` at 2.
    #[test]
    fn a_bar_without_times_is_an_improvement() {
        let text = r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
            "corpora":[{"name":"pdfjs","total":10,"passed":10,"degraded":0,
            "strict_eligible":10,"strict_clean":10}]}"#;
        let committed = parse(text).expect("a bar with no times is still a bar");
        assert_eq!(committed.time_runs, None);
        assert_eq!(committed.bars[0].time, None);
        let out = compare(&committed, &timed_run("pdfjs", 3_000, [2_000; 20]), true);
        assert!(!out.failed(), "{out:#?}");
        assert!(
            out.improvements.iter().any(|i| i.contains(
                "pdfjs: 500.0 pages/s (2000.0 µs a page) over 20 pages, and an open and count of 3000.0 µs a file over 10 files — new, with no recorded band"
            )),
            "{:?}",
            out.improvements
        );

        // A corpus the five runs did not time is new in the same way.
        let mut committed = timed_bar("pdfjs", five_runs());
        committed.bars[0].time = None;
        let out = compare(&committed, &timed_run("pdfjs", 3_000, [2_000; 20]), true);
        assert!(!out.failed(), "{out:#?}");
        assert!(
            out.improvements
                .iter()
                .any(|i| i.contains("new, with no recorded band")),
            "{:?}",
            out.improvements
        );

        // And a run that timed nothing claims nothing, and says so.
        let out = compare(&committed, &run("pdfjs", 10, 0, 0), true);
        assert!(
            out.notes
                .iter()
                .any(|n| n.contains("no open or page render was timed")),
            "{:?}",
            out.notes
        );
    }

    /// The bar records the run's ten slowest files, slowest first and ties by
    /// path, each under its `/Producer`; a file with no span is not ranked,
    /// and the printed table escapes a producer's pipe as the producer table
    /// does. The bar carries no time figure of the run's own.
    #[test]
    fn the_slowest_ten_name_their_producers() {
        let mut run = run("pdfjs", 12, 0, 0);
        let pages = [
            0, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000, 8_000, 9_000, 9_000, 500, 700,
        ];
        for (index, file) in run.corpora[0].files.iter_mut().enumerate() {
            file.producer = Some(format!("Producer {index}"));
            // The first file timed nothing.
            if index > 0 {
                file.open_micros = Some(10);
                file.page_micros = vec![PageTime {
                    page: 1,
                    micros: pages[index],
                }];
            }
        }
        run.corpora[0].files[8].producer = Some("Tool | Version".to_string());

        let recorded = run.to_ratchet_json("n");
        let corpus = recorded
            .get("corpora")
            .and_then(Json::as_array)
            .and_then(|corpora| corpora.first())
            .expect("the corpus");
        let slowest = corpus
            .get("slowest")
            .and_then(Json::as_array)
            .expect("the bar lists the slowest");
        let paths: Vec<&str> = slowest
            .iter()
            .filter_map(|file| file.get("path").and_then(Json::as_str))
            .collect();
        assert_eq!(
            paths,
            [
                "8.pdf", "9.pdf", "7.pdf", "6.pdf", "5.pdf", "4.pdf", "3.pdf", "2.pdf", "1.pdf",
                "11.pdf"
            ]
        );
        assert_eq!(
            slowest[0].get("producer").and_then(Json::as_str),
            Some("Tool | Version")
        );
        assert_eq!(slowest[0].get("micros").and_then(Json::as_u64), Some(9_010));
        assert_eq!(slowest[0].get("pages").and_then(Json::as_u64), Some(1));
        assert!(corpus.get("time").is_none(), "no figure of the run's own");

        let table = run.slowest_table();
        assert!(
            table.contains("| Tool \\| Version | `pdfjs` | `8.pdf` | 1 | 9.0 ms |"),
            "{table}"
        );
        assert!(!table.contains("`10.pdf`"), "the eleventh: {table}");
        assert!(!table.contains("`0.pdf`"), "untimed: {table}");
    }

    /// The band rule on the design's own figures: a spread of 3 % is banded
    /// at 20 %, one of 10 % at 25 % and one of 36 % at 85 %, and each band
    /// holds the slowest of the runs that set it. A mean's spread is the same
    /// as the times it averages.
    #[test]
    fn the_band_is_the_spread_fifteen_runs_would_show() {
        let time = |micros| Time { micros, over: 1 };
        for (slowest, expected) in [(100, 20), (103, 20), (110, 25), (136, 85)] {
            let band = band(time(100), time(slowest));
            assert_eq!(band, expected, "a spread of {} %", slowest - 100);
            assert!(time(slowest).within(time(100), band), "{slowest}");
        }
        let mean = |micros| Time { micros, over: 20 };
        assert_eq!(band(mean(40_000), mean(44_000)), 25);
    }

    /// A run at another resolution or job count is another measurement, as a
    /// run under another `--fonts` is, and so is a corpus whose file count is
    /// not the one its times were measured over: each is refused by name.
    #[test]
    fn a_time_measured_otherwise_is_refused_by_name() {
        let committed = timed_bar("pdfjs", five_runs());
        let mut other = timed_run("pdfjs", 3_000, [2_000; 20]);
        other.settings.dpi = 150.0;
        other.settings.jobs = 8;
        let out = compare(&committed, &other, true);
        assert!(out.failed(), "{out:#?}");
        for expected in [
            "measured at 72 dpi and this run rendered at 150",
            "measured 4 children at a time and this run ran 8",
        ] {
            assert!(
                out.refusals.iter().any(|r| r.contains(expected)),
                "{expected}: {:?}",
                out.refusals
            );
        }

        let mut repinned = timed_bar("pdfjs", five_runs());
        if let Some(time) = &mut repinned.bars[0].time {
            time.total = 12;
        }
        let out = compare(&repinned, &timed_run("pdfjs", 3_000, [2_000; 20]), true);
        assert!(
            out.refusals
                .iter()
                .any(|r| r.contains("`pdfjs` ran 10 files and its times were measured over 12")),
            "{:?}",
            out.refusals
        );
    }

    /// A run that timed fewer files or pages than the fewest of the five has
    /// a smaller measurement, not a faster one, and fails as a peak over fewer
    /// children does.
    #[test]
    fn a_figure_over_fewer_files_or_pages_is_not_a_faster_one() {
        let committed = timed_bar("pdfjs", five_runs());
        let mut thinned = timed_run("pdfjs", 3_000, [2_000; 20]);
        for file in thinned.corpora[0].files.iter_mut().take(3) {
            file.open_micros = None;
            file.page_micros.clear();
        }
        let out = compare(&committed, &thinned, true);
        assert!(out.failed(), "{out:#?}");
        for expected in [
            "the open of 7 files was timed, fewer than the 10",
            "14 page renders were timed, fewer than the 20",
        ] {
            assert!(
                out.regressions.iter().any(|r| r.contains(expected)),
                "{expected}: {:?}",
                out.regressions
            );
        }
    }

    /// A time block the band rule cannot be read from is refused when the bar
    /// is read: other than five runs, runs that do not match the dispatches,
    /// a run over more files than the corpus, and a zero time over files or
    /// pages that were timed, which is a band nothing can sit under.
    #[test]
    fn a_time_block_that_cannot_be_is_refused_at_the_ratchet() {
        let good = [(10, 30_000, 5_000); 5];
        let read = parse(&timed_bar_text(5, &good)).expect("five runs over five dispatches");
        assert_eq!(read.time_runs.map(|runs| runs.dispatches.len()), Some(5));
        assert_eq!(
            read.bars[0].time.as_ref().map(|time| time.runs.len()),
            Some(5)
        );

        let first = |run: (u64, u64, u64)| {
            let mut runs = good;
            runs[0] = run;
            timed_bar_text(5, &runs)
        };
        for (text, expected) in [
            (timed_bar_text(4, &good[..4]), "a band is set from 5"),
            (
                timed_bar_text(5, &good[..4]),
                "records 4 timed runs and `time_runs` names 5 dispatches",
            ),
            (
                first((11, 30_000, 5_000)),
                "the open of 11 files timed out of 10",
            ),
            (first((10, 0, 5_000)), "a band nothing can sit under"),
            (first((10, 30_000, 0)), "a band nothing can sit under"),
        ] {
            let error = parse(&text).expect_err(expected);
            assert!(error.contains(expected), "expected `{expected}`: {error}");
        }

        let orphan = r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
            "corpora":[{"name":"pdfjs","total":10,"passed":10,"degraded":0,
            "strict_eligible":10,"strict_clean":10,"time":{"total":10,"runs":[]}}]}"#;
        let error = parse(orphan).expect_err("times with no runs named");
        assert!(error.contains("no `time_runs` naming the runs"), "{error}");
    }

    /// `--record` writes every other axis from its run and none of its
    /// times: the committed bar's `time_runs` and each corpus's `time` come
    /// over unchanged, and the bar still reads as the one it was.
    #[test]
    fn a_record_carries_the_five_runs_and_writes_no_time_of_its_own() {
        let text = timed_bar_text(5, &[(10, 30_000, 5_000); 5]);
        let committed = crate::json::parse(&text).expect("the committed bar");
        let run = timed_run("pdfjs", 3_333, [7_777; 20]);
        let recorded = carry_times(&committed, run.to_ratchet_json("n"));
        assert_eq!(recorded.get("time_runs"), committed.get("time_runs"));

        let read = parse(&recorded.to_pretty()).expect("the recorded bar reads");
        let before = parse(&text).expect("the committed bar reads");
        assert_eq!(read.time_runs, before.time_runs);
        assert_eq!(read.bars[0].time, before.bars[0].time);
        assert!(
            !recorded.to_pretty().contains("\"p50_micros\": 7777"),
            "the run's own figure was written"
        );
    }

    /// The three bars committed today go on reading as bars: a time block
    /// that is absent is a bar with no band, never a refusal.
    #[test]
    fn the_three_committed_bars_still_parse() {
        for path in [
            crate::corpus::RATCHET_PATH,
            crate::corpus::RATCHET_FONTS_PATH,
            crate::corpus::RATCHET_BUNDLED_PATH,
        ] {
            let text = std::fs::read_to_string(crate::repo_root().join(path))
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            let bar = parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(bar.complete, "{path} is a bar");
        }
    }

    /// A run whose files carry the given structure counts, spread evenly.
    ///
    /// Evenly because the comparison is over corpus totals: how the elements
    /// are distributed across files cannot change any of the three answers,
    /// and a helper that pretended otherwise would be testing itself.
    fn tagged_run(name: &str, files_with_tree: u64, per_file: Tagged) -> Run {
        let mut run = run(name, 10, 0, 0);
        for (index, file) in run.corpora[0].files.iter_mut().enumerate() {
            if (index as u64) < files_with_tree {
                file.tagged = Some(per_file);
            }
        }
        run
    }

    fn tagged_bar(name: &str, tagged: TaggedBar) -> Ratchet {
        let mut committed = bar(name, 10, 10, 0);
        committed.bars[0].tagged = Some(tagged);
        committed
    }

    /// The seeded regression tagged PDF milestone 4 names: cap the walk's
    /// elements at zero and the check must fail. It fails twice over — the
    /// files-with-a-tree share collapses and the element floor is breached —
    /// and both messages are asserted, because a single message could be
    /// produced by a comparison that happened to be looking elsewhere.
    #[test]
    fn a_structure_walk_that_finds_nothing_is_a_regression() {
        let committed = tagged_bar(
            "verapdf",
            TaggedBar {
                files: 6,
                elements: 600,
                matched: 900,
                orphans: 100,
            },
        );
        let now = tagged_run("verapdf", 0, Tagged::default());
        let out = compare(&committed, &now, true);
        assert!(out.failed(), "{out:?}");
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("no file yielded a structure tree at all")),
            "{:?}",
            out.regressions
        );
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("structure elements")),
            "{:?}",
            out.regressions
        );
    }

    /// The subtler shape: every tree is still found and every element is still
    /// reached, but the join stopped claiming characters — a `/ParentTree` or
    /// `/MCID` regression that the element floor cannot see. The share of
    /// marked characters the tree claimed is what catches it.
    #[test]
    fn a_join_that_orphans_what_it_used_to_claim_is_a_regression() {
        let committed = tagged_bar(
            "verapdf",
            TaggedBar {
                files: 6,
                elements: 600,
                matched: 900,
                orphans: 100,
            },
        );
        let now = tagged_run(
            "verapdf",
            6,
            Tagged {
                elements: 100,
                matched: 50,
                orphans: 116,
                ..Tagged::default()
            },
        );
        let out = compare(&committed, &now, true);
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("a smaller share")),
            "{:?}",
            out.regressions
        );
    }

    /// A run that reaches more of everything is not a regression, and the two
    /// improvements are printed rather than passed over in silence.
    #[test]
    fn reaching_more_structure_is_an_improvement() {
        let committed = tagged_bar(
            "verapdf",
            TaggedBar {
                files: 5,
                elements: 500,
                matched: 900,
                orphans: 100,
            },
        );
        let now = tagged_run(
            "verapdf",
            6,
            Tagged {
                elements: 100,
                matched: 190,
                orphans: 10,
                ..Tagged::default()
            },
        );
        let out = compare(&committed, &now, true);
        assert!(!out.failed(), "{out:?}");
        assert!(
            out.improvements
                .iter()
                .any(|i| i.contains("yield a structure tree, up from")),
            "{:?}",
            out.improvements
        );
    }

    /// A bar recorded before the walk existed cannot be regressed against, so
    /// the first run that measures one is an improvement and never a refusal.
    #[test]
    fn a_bar_with_no_structure_row_is_new_rather_than_broken() {
        let committed = bar("verapdf", 10, 10, 0);
        let now = tagged_run(
            "verapdf",
            6,
            Tagged {
                elements: 100,
                matched: 190,
                orphans: 10,
                ..Tagged::default()
            },
        );
        let out = compare(&committed, &now, true);
        assert!(!out.failed(), "{out:?}");
        assert!(
            out.improvements
                .iter()
                .any(|i| i.contains("new, with no recorded bar")),
            "{:?}",
            out.improvements
        );
    }

    /// The floor a seeded regression would otherwise be recorded *as*. A
    /// committed bar of zero elements over files that have trees is a floor
    /// nothing can fall below, so it is refused when the ratchet is read
    /// rather than compared against and passed.
    #[test]
    fn a_committed_bar_of_zero_elements_is_refused_at_the_ratchet() {
        let text = r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
            "corpora":[{"name":"verapdf","total":10,"passed":10,"degraded":0,
            "strict_eligible":10,"strict_clean":10,
            "tagged":{"files":6,"elements":0,"matched":0,"orphans":0}}]}"#;
        let error = parse(text).expect_err("a floor of zero is not a floor");
        assert!(error.contains("no structure elements at all"), "{error}");
    }

    /// A bar's `peak_runs` names the dispatches its bands were read from: read
    /// where it is, absent where the bands predate it, and refused where it
    /// names other than three runs, or one run twice.
    #[test]
    fn a_bar_parses_with_peak_runs_and_without() {
        let text = |runs: &str| {
            format!(
                r#"{{"schema":2,"complete":true,"settings":{{"fonts":"none"}},{runs}
                "corpora":[{{"name":"verapdf","total":10,"passed":10,"degraded":0,
                "strict_eligible":10,"strict_clean":10,
                "peak":{{"bytes":200000000,"files":10}}}}]}}"#
            )
        };
        let block = |ids: &[&str]| {
            let dispatches: Vec<String> = ids
                .iter()
                .map(|id| format!(r#"{{"id":"{id}","host":"Example CPU x 4"}}"#))
                .collect();
            format!(
                r#""peak_runs":{{"revision":"0123abcd","dispatches":[{}]}},"#,
                dispatches.join(",")
            )
        };

        let without = parse(&text("")).expect("a bar whose bands predate their runs");
        assert_eq!(without.peak_runs, None);
        assert!(without.bars[0].peak.is_some());

        let with = parse(&text(&block(&["1", "2", "3"]))).expect("a bar that names its runs");
        let runs = with.peak_runs.expect("its runs");
        assert_eq!(runs.revision, "0123abcd");
        assert_eq!(
            runs.dispatches
                .iter()
                .map(|dispatch| dispatch.id.as_str())
                .collect::<Vec<_>>(),
            ["1", "2", "3"]
        );

        let two = parse(&text(&block(&["1", "2"]))).expect_err("two runs");
        assert!(two.contains("names 2 dispatches"), "{two}");
        let twice = parse(&text(&block(&["1", "2", "1"]))).expect_err("one run twice");
        assert!(twice.contains("names dispatch 1 twice"), "{twice}");
    }

    /// A run where the given files each reached the given peak.
    fn peak_run(name: &str, children: u64, bytes: u64) -> Run {
        let mut run = run(name, 10, 0, 0);
        for (index, file) in run.corpora[0].files.iter_mut().enumerate() {
            if (index as u64) < children {
                // Descending, so the maximum is the first file's rather than
                // whichever one the helper happened to write last: a test that
                // could pass by reading a sum or a mean is not testing a
                // maximum.
                file.peak = Some(bytes - index as u64);
            }
        }
        run
    }

    fn peak_bar(name: &str, peak: PeakBar) -> Ratchet {
        let mut committed = bar(name, 10, 10, 0);
        committed.bars[0].peak = Some(peak);
        committed
    }

    /// **The band, its tolerance, and the direction it is a band in.** A run
    /// whose largest child needs more than the recorded maximum *plus its
    /// measured tolerance* has regressed; one inside the tolerance has not,
    /// which is what stops a high-water mark's own swing failing a build; and
    /// one that needs less is an improvement.
    #[test]
    fn a_child_that_needs_more_memory_than_the_band_is_a_regression() {
        let committed = peak_bar(
            "verapdf",
            PeakBar {
                bytes: 200_000_000,
                files: 10,
            },
        );

        // Two percent of 200 MB is 4 MB, so this is a byte past the tolerance.
        let worse = compare(&committed, &peak_run("verapdf", 10, 204_000_001), false);
        assert!(worse.failed(), "{worse:#?}");
        assert!(
            worse.regressions[0].contains("above the recorded band"),
            "{:?}",
            worse.regressions
        );

        // Exactly the band is not a regression. An absolute maximum compared
        // with `<` would fail a run that changed nothing.
        let level = compare(&committed, &peak_run("verapdf", 10, 200_000_000), true);
        assert!(!level.failed(), "{level:#?}");

        // And neither is the swing this tolerance was measured for: 0.76 % of
        // the band, which is what a `--check` run of an unchanged binary
        // actually produced on 6 September 2026.
        let swung = compare(&committed, &peak_run("verapdf", 10, 201_520_000), true);
        assert!(
            !swung.failed(),
            "a high-water mark's own swing must not fail a build: {swung:#?}"
        );

        let better = compare(&committed, &peak_run("verapdf", 10, 100_000_000), false);
        assert!(!better.failed(), "{better:#?}");
        assert!(
            better
                .improvements
                .iter()
                .any(|i| i.contains("down from 200000000")),
            "{:?}",
            better.improvements
        );
    }

    /// And the denominator, which is the trick the maximum would otherwise be
    /// open to: a run where the measurement stopped happening on most children
    /// reports a smaller maximum without the engine having improved at all.
    #[test]
    fn a_maximum_over_fewer_children_is_not_a_smaller_maximum() {
        let committed = peak_bar(
            "verapdf",
            PeakBar {
                bytes: 200_000_000,
                files: 10,
            },
        );

        let thinned = compare(&committed, &peak_run("verapdf", 2, 100_000_000), false);
        assert!(thinned.failed(), "{thinned:#?}");
        assert!(
            thinned
                .regressions
                .iter()
                .any(|r| r.contains("2/10 children reported a peak resident set")),
            "{:?}",
            thinned.regressions
        );

        // And none at all is its own message, for the reason the tagged axis
        // separates "no tree anywhere" from "fewer trees": a build that stopped
        // measuring is a different fix from one that measures less.
        let silent = compare(&committed, &peak_run("verapdf", 0, 0), false);
        assert!(silent.failed(), "{silent:#?}");
        assert!(
            silent
                .regressions
                .iter()
                .any(|r| r.contains("no child reported a peak resident set at all")),
            "{:?}",
            silent.regressions
        );
    }

    /// The three bars committed today have no `peak` key, and they must go on
    /// parsing and go on being compared against. A bar that predates a
    /// measurement cannot be regressed against, which is the same rule the
    /// structure walk arrived under and the reason `SCHEMA` stays 2.
    #[test]
    fn a_bar_recorded_before_the_measurement_still_parses_and_still_holds() {
        let text = r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
            "corpora":[{"name":"verapdf","total":10,"passed":10,"degraded":0,
            "strict_eligible":10,"strict_clean":10}]}"#;
        let committed = parse(text).expect("a bar with no peak key is still a bar");
        assert_eq!(committed.bars[0].peak, None);

        let out = compare(&committed, &peak_run("verapdf", 10, 200_000_000), true);
        assert!(!out.failed(), "{out:#?}");
        assert!(
            out.improvements
                .iter()
                .any(|i| i.contains("new, with no recorded bar")),
            "{:?}",
            out.improvements
        );
    }

    /// A band of zero bytes over children that reported something is the
    /// mirror of a structure floor of zero elements: nothing can sit under it,
    /// so every run afterwards would regress and the message would blame the
    /// engine for a ratchet written wrong. Refused when the file is read.
    #[test]
    fn a_committed_band_of_zero_bytes_is_refused_at_the_ratchet() {
        let text = r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
            "corpora":[{"name":"verapdf","total":10,"passed":10,"degraded":0,
            "strict_eligible":10,"strict_clean":10,
            "peak":{"bytes":0,"files":6}}]}"#;
        let error = parse(text).expect_err("a band of zero is not a band");
        assert!(error.contains("a band nothing can sit under"), "{error}");

        // And the mirror: a figure with no children behind it was measured by
        // nobody, so it is not a measurement.
        let empty = r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
            "corpora":[{"name":"verapdf","total":10,"passed":10,"degraded":0,
            "strict_eligible":10,"strict_clean":10,
            "peak":{"bytes":200000000,"files":0}}]}"#;
        let error = parse(empty).expect_err("no children is no measurement");
        assert!(error.contains("over no children at all"), "{error}");
    }

    /// Ruling 13's axis: a rewrite that stops validating is a regression, and
    /// so is a run that measured fewer files. The second is the one worth
    /// stating — a change that made fewer documents readable would otherwise
    /// raise the rate by shrinking its denominator.
    #[test]
    fn the_strict_axis_fails_both_ways() {
        let mut worse = run("pdfjs", 10, 0, 0);
        worse.corpora[0].files[0].strict = crate::runner::Strict::Checked {
            structure: 1,
            semantics: 0,
            kinds: BTreeMap::from([("free-head-missing".to_string(), 1usize)]),
        };
        let outcome = compare(&bar("pdfjs", 10, 10, 0), &worse, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.regressions[0].contains("9/10 rewrites validate"),
            "{:?}",
            outcome.regressions
        );

        let mut fewer = run("pdfjs", 10, 0, 0);
        fewer.corpora[0].files[0].strict =
            crate::runner::Strict::Ineligible("the file is encrypted".to_string());
        let outcome = compare(&bar("pdfjs", 10, 10, 0), &fewer, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.regressions[0].contains("ran on 9/10 files"),
            "{:?}",
            outcome.regressions
        );
    }

    /// And a semantic defect is not one: a rewrite inherits the page tree and
    /// the annotations from its source, so a backwards `/Rect` in somebody
    /// else's document is not this writer's regression.
    #[test]
    fn what_a_rewrite_inherited_does_not_move_the_bar() {
        let mut inherited = run("pdfjs", 10, 0, 0);
        inherited.corpora[0].files[0].strict = crate::runner::Strict::Checked {
            structure: 0,
            semantics: 4,
            kinds: BTreeMap::from([("annot-rect-unordered".to_string(), 4usize)]),
        };
        let outcome = compare(&bar("pdfjs", 10, 10, 0), &inherited, true);
        assert!(!outcome.failed(), "{outcome:#?}");
    }

    #[test]
    fn a_regression_fails() {
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &run("pdfjs", 950, 26, 0), false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.regressions[0].contains("950/976"),
            "{:?}",
            outcome.regressions
        );
    }

    #[test]
    fn an_improvement_passes_and_prints_the_numbers_to_paste() {
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &run("pdfjs", 970, 6, 0), false);
        assert!(!outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.improvements[0].contains("970/976 passed, up from 960/976"),
            "{:?}",
            outcome.improvements
        );
    }

    /// The heart of milestone 3. A corpus that grew is compared on the same
    /// terms as one that did not: the bar is a *ratio of counts*, and forty
    /// extra files do not lower it.
    ///
    /// 960/976 = 0.98361. The run below is 985/1016 = 0.96949 — worse, and it
    /// must fail even though more files passed in absolute terms. A
    /// comparator that compared `passed` alone, or that stored a rate and
    /// diluted it, would call this an improvement of twenty-five files.
    #[test]
    fn a_corpus_that_grew_does_not_reset_the_bar() {
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &run("pdfjs", 985, 31, 0), false);
        assert!(
            outcome.failed(),
            "985 of 1016 is a lower rate than 960 of 976: {outcome:#?}"
        );

        // And the same growth carrying its weight passes: 1000/1016 = 0.98425.
        let outcome = compare(
            &bar("pdfjs", 960, 976, 0),
            &run("pdfjs", 1000, 16, 0),
            false,
        );
        assert!(!outcome.failed(), "{outcome:#?}");
    }

    /// The arithmetic itself, on the pairs that a float comparison gets
    /// wrong. 49999999/50000000 and 4999999/5000000 differ in the eighth
    /// decimal place; `f64` has room, `f32` does not, and nobody should have
    /// to know which one the comparator used.
    #[test]
    fn the_comparison_is_exact_rather_than_approximate() {
        assert!(holds(1, 3, 1, 3), "an unchanged rate holds");
        assert!(!holds(2, 3, 1, 2), "1/2 is below 2/3");
        assert!(holds(2, 3, 3, 4), "3/4 is above 2/3");
        assert!(
            !holds(4_999_999, 5_000_000, 49_999_989, 50_000_000),
            "a rate lower by one part in five million is still lower"
        );
        assert!(holds(4_999_999, 5_000_000, 49_999_990, 50_000_000));
    }

    /// A corpus that failed to extract runs zero files. Both sides of the
    /// cross-multiplication are then zero and the bar "holds" — so this is
    /// caught before the arithmetic, not by it.
    #[test]
    fn a_corpus_that_ran_nothing_is_refused_rather_than_passed() {
        let mut empty = run("pdfjs", 0, 0, 0);
        empty.corpora[0].files.clear();
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &empty, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.refusals[0].contains("ran zero files"),
            "{:?}",
            outcome.refusals
        );
    }

    #[test]
    fn a_corpus_in_the_bar_and_not_in_the_run_is_refused() {
        let outcome = compare(&bar("verapdf", 100, 100, 0), &run("pdfjs", 10, 0, 0), false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.refusals.iter().any(|r| r.contains("was not run")),
            "{:?}",
            outcome.refusals
        );
    }

    /// No silent caps, enforced rather than documented: a sampled run's
    /// numbers describe a different set of files, so they may not touch the
    /// bar in either direction.
    #[test]
    fn an_incomplete_run_may_not_be_compared_at_all() {
        let mut sampled = run("pdfjs", 970, 6, 0);
        sampled
            .limits
            .push("sampled the first 976 of 4000".to_string());
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &sampled, false);
        assert!(outcome.failed(), "an improvement, and still refused");
        assert!(
            outcome.refusals[0].contains("incomplete"),
            "{:?}",
            outcome.refusals
        );
    }

    /// The correction that makes the second axis mean anything. A run with
    /// real faces has far fewer warnings than one without, and comparing the
    /// two would either read as a large improvement or set a bar no ordinary
    /// run can clear.
    #[test]
    fn a_run_with_fonts_is_not_compared_against_a_bar_without_them() {
        let mut with_faces = run("pdfjs", 960, 16, 10);
        with_faces.settings.fonts = "corpus/files/pdfjs/external/standard_fonts".to_string();
        let outcome = compare(&bar("pdfjs", 960, 976, 800), &with_faces, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.refusals[0].contains("different measurements"),
            "{:?}",
            outcome.refusals
        );
    }

    /// Degradation is reported both ways and fails only under `--strict`.
    #[test]
    fn degradation_is_a_note_by_default_and_a_regression_under_strict() {
        let worse = run("pdfjs", 976, 0, 900);
        let outcome = compare(&bar("pdfjs", 976, 976, 800), &worse, false);
        assert!(!outcome.failed(), "{outcome:#?}");
        assert!(
            outcome
                .notes
                .iter()
                .any(|n| n.contains("rendered with warnings, up from")),
            "{:?}",
            outcome.notes
        );

        let outcome = compare(&bar("pdfjs", 976, 976, 800), &worse, true);
        assert!(outcome.failed(), "{outcome:#?}");
    }

    #[test]
    fn a_ratchet_round_trips_through_its_own_writer() {
        let run = run("pdfjs", 960, 16, 100);
        let text = run.to_ratchet_json("a note").to_pretty();
        let read = parse(&text).expect("it parses");
        assert_eq!(read.bars[0].total, 976);
        assert_eq!(read.bars[0].passed, 960);
        assert_eq!(read.bars[0].degraded, 100);
        assert!(read.complete);
        assert_eq!(read.fonts, "none");
        assert!(
            !compare(&read, &run, true).failed(),
            "it clears its own bar"
        );
    }

    #[test]
    fn a_malformed_ratchet_is_rejected_with_a_useful_message() {
        let good = run("pdfjs", 960, 16, 100).to_ratchet_json("n").to_pretty();
        for (text, expected) in [
            (
                // Written against the constant rather than against a literal,
                // because a schema bump would otherwise turn this case into a
                // no-op and the test would pass by not testing anything.
                good.replace(
                    &format!("\"schema\": {}", crate::report::SCHEMA),
                    "\"schema\": 7",
                ),
                "schema 7",
            ),
            (
                good.replace("\"passed\": 960", "\"passed\": 9999"),
                "cannot be",
            ),
            (
                good.replace("\"fonts\": \"none\"", "\"fnots\": \"none\""),
                "--fonts",
            ),
            (good.replace("\"corpora\"", "\"korpora\""), "no `corpora`"),
            (
                good.replace("\"total\": 976", "\"total\": 97.6"),
                "whole-number",
            ),
        ] {
            let error = parse(&text).expect_err(&format!("must be rejected: {expected}"));
            assert!(
                error.contains(expected),
                "expected `{expected}`, got: {error}"
            );
        }
    }

    /// A ratchet that does not say whether its run was complete is not
    /// assumed to have been.
    #[test]
    fn a_ratchet_missing_its_completeness_flag_is_not_a_bar() {
        let text = run("pdfjs", 960, 16, 0)
            .to_ratchet_json("n")
            .to_pretty()
            .replace("\"complete\": true", "\"complete\": null");
        let read = parse(&text).expect("it still parses");
        assert!(!read.complete);
        assert!(compare(&read, &run("pdfjs", 970, 6, 0), false).failed());
    }

    /// **A capability the scanner stops seeing is a regression**, even though
    /// every other number improves when it happens.
    ///
    /// This is the axis's whole reason. A capability count is a property of the
    /// corpus rather than of the engine: the same thousand files carry the same
    /// thousand `ICCBased` spaces whatever this build does with them. So a run
    /// that finds fewer has not got worse at rendering — it has stopped looking
    /// — and the pass rate goes *up*, because a file whose capability is not
    /// recognised is a file whose capability cannot be reported as degraded.
    #[test]
    fn a_capability_the_scanner_stops_finding_is_a_regression() {
        let mut committed = bar("pdfjs", 10, 10, 0);
        committed.bars[0]
            .capabilities
            .insert("iccbased".to_string(), 4);

        // The same ten files, all passing, with the capability seen twice.
        let mut now = run("pdfjs", 10, 0, 0);
        for file in now.corpora[0].files.iter_mut().take(2) {
            file.capabilities.insert("iccbased".to_string());
        }
        let out = compare(&committed, &now, false);
        assert!(out.failed(), "a scanner that lost two files did not fail");
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("stopped recognising it")),
            "{:?}",
            out.regressions
        );

        // And finding more of them is an improvement, not a refusal: the
        // corpus grew, or the scanner learned a name.
        let mut now = run("pdfjs", 10, 0, 0);
        for file in now.corpora[0].files.iter_mut().take(6) {
            file.capabilities.insert("iccbased".to_string());
        }
        let out = compare(&committed, &now, false);
        assert!(!out.failed(), "{:?}", out.regressions);
        assert!(
            out.improvements.iter().any(|i| i.contains("iccbased")),
            "{:?}",
            out.improvements
        );
    }

    /// A bar recorded before this axis existed compares against nothing.
    #[test]
    fn a_bar_with_no_capabilities_refuses_nothing() {
        let committed = bar("pdfjs", 10, 10, 0);
        let mut now = run("pdfjs", 10, 0, 0);
        now.corpora[0].files[0]
            .capabilities
            .insert("jbig2".to_string());
        let out = compare(&committed, &now, false);
        assert!(!out.failed(), "{:?}", out.regressions);
        assert!(
            out.improvements.iter().any(|i| i.contains("newly counted")),
            "{:?}",
            out.improvements
        );
    }
}
