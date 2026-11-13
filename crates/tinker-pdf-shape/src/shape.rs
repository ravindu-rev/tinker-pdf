//! The default shaper: text in, positioned glyphs out.
//!
//! Milestone 1 of `docs/design/shaping.md` built the machinery that executes a
//! lookup list against a [`Buffer`]; this is the layer that decides **which**
//! lookups run and **in what order**, maps the caller's text to glyphs through
//! the face's `cmap`, and keeps every glyph pointing back at the byte of text
//! it came from.
//!
//! # The pipeline, and where each stage is
//!
//! Per paragraph: [`crate::bidi::Paragraph::new`] resolves embedding levels,
//! [`itemize`] cuts the text into runs of one script at one level, and
//! [`Shaper::shape`] shapes one run. The caller breaks lines over the
//! *logical* text and calls [`crate::bidi::Paragraph::line`] per line.
//! [`Shaper::shape_text`] is the three-line version of the first three steps,
//! for a caller that has one paragraph and wants glyphs.
//!
//! # Logical order, in and out
//!
//! A run is shaped in logical order and comes back in logical order, whichever
//! way it reads. That is the specification's own model — `GSUB` and `GPOS`
//! rules are written over the text as typed — and it is what makes
//! [`ShapedGlyph::cluster`] monotonic, which milestone 7 needs to rebuild
//! `/ToUnicode` from a ligature. Turning logical order into visual order is
//! [`crate::bidi::reorder`]'s job and happens per line, after breaking.
//!
//! Milestone 2 recorded that no right-to-left run had a glyph-level fixture,
//! so the claim about one was that it was deterministic and that its levels
//! were right, not that its glyphs were. **Milestone 4 closed that**:
//! text-rendering-tests SHARAN-1 shapes six Nasta‘līq words of Urdu through
//! this function, and `tests/text_rendering.rs` checks every glyph and every
//! position against the fixture — after reordering the runs with
//! [`crate::bidi::reorder`] and walking a right-to-left run's glyphs
//! backwards, which is the two-step a consumer does and the only place
//! direction is read outside this crate.
//!
//! # Columns
//!
//! A vertical paragraph goes through [`itemize_vertical`] instead, which cuts
//! each run a third way, by how it stands in the column ([`Orientation`]).
//! An upright run is shaped top to bottom ([`Direction::TopToBottom`]) with
//! [`VERT`] in its plan and the face's vertical metrics; a sideways run is
//! shaped along a line, with [`VRTR`], and marked for its consumer to turn.
//! [`Shaper::shape_text_vertical`] is the three-line version.
//! `docs/design/shaping.md`, *Vertical shaping*, is the reading of the
//! OpenType feature registry, UAX #50 and CSS Writing Modes 3 this follows.
//!
//! # Joining scripts take a different plan
//!
//! A run whose text contains a character that cursively joins gets
//! [`JOINING_GSUB_STAGES`] rather than [`DEFAULT_GSUB_FEATURES`]: seven stages
//! instead of one, with the four joining features restricted to the glyphs
//! whose letters are in that form. Which run that is comes from the text and
//! not from a list of scripts; `crate::arabic` says why.
//!
//! # Integer, throughout
//!
//! Every number that leaves here is a font design unit, and
//! [`ShapedRun::units_per_em`] is beside them so a consumer can scale. The
//! crate denies `clippy::float_arithmetic`; the reason is ruling 4 and it is
//! written up in `lib.rs`.

use core::ops::Range;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tinker_pdf_font::{Sfnt, VerticalOrigins};

use crate::aat::{Feat, Kerx, Morx, Selection};
use crate::apply::LookupIndex;
use crate::arabic;
use crate::bidi::{BaseDirection, Level, Paragraph};
use crate::buffer::{Buffer, Direction, ShapedGlyph};
use crate::common::Tag;
use crate::graphite::{self, FeatureValues, Features, Languages, Refusal};
use crate::limits::Limits;
use crate::read::Bytes;
use crate::unicode::{self, Script};
use crate::universal;
use crate::{Layout, LayoutTable, MarkWidths, Table, Warning};

/// The `GSUB` features the default shaper turns on, in the order they are
/// requested.
///
/// The order in this array does **not** decide the order they run in:
/// [`crate::LayoutTable::lookups_for`] sorts by lookup index, because
/// ISO/IEC 14496-22 says lookups are applied in the order they appear in the
/// lookup list and not in the order the features naming them do. The array is
/// written in the specification's own order anyway, so that reading it tells
/// the truth about intent.
///
/// # Which of these the fixtures adjudicate, and which they do not
///
/// `docs/design/shaping.md` names `ccmp` and `liga` for this milestone. Two
/// more are here because the conformance corpus *requires* them and would fail
/// without them, which is the strongest reason available under ruling 13:
///
/// - `calt` — text-rendering-tests **GSUB-1** puts its whole test behind it.
/// - `ccmp` — **GSUB-2**, the Ethiopic numerals, likewise.
/// - `rlig` — **GSUB-3**, the billion-laughs face.
///
/// `liga`, `clig` and `locl` are on because the OpenType feature registry
/// marks them applied by default in horizontal text, and **no fixture in this
/// corpus reaches any of them**. That is stated rather than implied: three of
/// the six entries below are evidence and three are the registry's word.
pub const DEFAULT_GSUB_FEATURES: &[Tag] = &[
    Tag::new(b"locl"),
    Tag::new(b"ccmp"),
    Tag::new(b"rlig"),
    Tag::new(b"liga"),
    Tag::new(b"clig"),
    Tag::new(b"calt"),
];

/// The `GPOS` features the default shaper turns on.
///
/// `kern`, `mark` and `mkmk` are the three `docs/design/shaping.md` names for
/// milestone 2 and all three are adjudicated: GPOS-1 and GPOS-2 are `kern`,
/// GPOS-3 is `mark`, GPOS-4 is `mkmk`. `curs` was milestone 4's subject and is
/// now adjudicated too — SHARAN-1 is nothing but cursive attachment, and it is
/// what settled which of the two readings of that lookup this crate follows;
/// see [`crate::gpos`]'s type 3.
///
/// `dist`, `abvm` and `blwm` are the registry's other default-on positioning
/// features in horizontal text. `abvm` and `blwm` are here for milestone 5 —
/// the Kannada faces of the `SHKNDA` sections put their mark positioning
/// behind `blwm` — and `dist` is reached by no fixture in this corpus at all.
/// Adding the two moved nothing: the twelve sections milestone 2 was graded on
/// use none of them, and their counts are unchanged.
pub const DEFAULT_GPOS_FEATURES: &[Tag] = &[
    Tag::new(b"abvm"),
    Tag::new(b"blwm"),
    Tag::new(b"kern"),
    Tag::new(b"dist"),
    Tag::new(b"curs"),
    Tag::new(b"mark"),
    Tag::new(b"mkmk"),
];

/// The `GSUB` features a run of a joining script turns on, **in stages**.
///
/// Each entry is one stage: every lookup the stage's features name is run over
/// the whole buffer before the next stage starts. That is not how
/// [`DEFAULT_GSUB_FEATURES`] works — there the lookups of every feature are
/// merged and sorted by lookup index, which is the specification's rule for a
/// single application — and the difference is load-bearing here.
///
/// # Why the four forms are four stages and not one
///
/// A Nasta‘līq face's `medi` lookup is written expecting `init` **not** to
/// have run yet, or the other way round; the two rewrite the same glyphs and
/// the face's designer chose an order. Merging them and sorting by lookup
/// index would apply whichever the face happened to lay out first, which is a
/// property of how the table was compiled rather than of what it means. So
/// each is its own stage, in the order the OpenType feature registry lists
/// them for Arabic: isolated, final, medial, initial.
///
/// # The masks
///
/// The four form features carry a per-glyph mask and the rest are global. A
/// glyph's mask says which single form [`crate::arabic::forms`] put it in, so
/// the `init` lookup reaches the first letter of a word and no other.
///
/// # Which of these a fixture adjudicates
///
/// SHARAN-1 — the corpus's one Arabic-script section, and milestone 4's whole
/// bar — uses `ccmp`, `isol`, `fina`, `medi`, `init` and `rlig`. Six of the
/// nine below are therefore evidence. `locl`, `rclt` and `mset` are the
/// registry's word: `locl` and `rclt` are applied by default in horizontal
/// text, `mset` is the legacy Arabic mark-positioning feature, and **no
/// fixture in this corpus reaches any of the three**.
///
/// `liga` and `clig` are deliberately absent rather than forgotten. A joining
/// script's ligatures are `rlig` — required ligatures, which lam-alef is —
/// and the discretionary ones are turned off, because a reader who typed
/// `ZWNJ` between two letters asked for them not to be joined and a `liga`
/// lookup would join them anyway.
pub const JOINING_GSUB_STAGES: &[&[Tag]] = &[
    &[Tag::new(b"ccmp"), Tag::new(b"locl")],
    &[Tag::new(b"isol")],
    &[Tag::new(b"fina")],
    &[Tag::new(b"medi")],
    &[Tag::new(b"init")],
    &[Tag::new(b"rlig"), Tag::new(b"rclt"), Tag::new(b"calt")],
    &[Tag::new(b"mset")],
];

/// The vertical alternates an upright run adds to its plan's last `GSUB`
/// stage: `vert`, which the OpenType feature registry has "active by default
/// in vertical writing mode".
///
/// The registry gives `vert` and `vrt2` to two kinds of layout engine: `vert`
/// (with [`VRTR`] for sideways runs) to one that turns its sideways runs
/// itself, which this crate's consumers do with [`Orientation::Sideways`], and
/// [`VRT2`] "in lieu of" both to one that depends on the face for pre-rotated
/// glyphs. See [`Shaper::with_settings`] for how a caller of the second kind
/// asks for it.
pub const VERT: Tag = Tag::new(b"vert");

/// `vrt2`, the vertical alternates with rotation: off by default.
///
/// The registry's `vert` is "a subset" of it and it "overrides" `vert`, and
/// the two are never applied together. So turned on through
/// [`Shaper::with_settings`], it takes [`VERT`]'s place in an upright run
/// where the face carries it for the run's script and language; where the
/// face does not, `vert` stays, because nothing overrides it. A sideways run
/// never takes it: its consumer turns it, and a pre-rotated glyph would be
/// turned twice.
pub const VRT2: Tag = Tag::new(b"vrt2");

/// `vrtr`, the vertical alternates for rotation, which a sideways run adds to
/// its plan's last stage: the registry has it "always active by default for
/// sideways runs in vertical writing mode".
pub const VRTR: Tag = Tag::new(b"vrtr");

/// The `GPOS` features an upright run turns on.
///
/// [`DEFAULT_GPOS_FEATURES`] with `vkrn` in place of `kern` — the OpenType
/// feature registry's `kern` "is not intended for use in vertical text
/// layout", and `vkrn` "should be active by default" in most of it — and
/// `valt`, which the registry has "active by default in vertical-setting
/// contexts". `abvm`, `blwm`, `dist`, `curs`, `mark` and `mkmk` are a line's,
/// none of them restricted to horizontal text.
///
/// The registry's other vertical positioning features are off: `vhal`,
/// `vpal` and `vapk`, which it has off by default, and `vchw`, which it turns
/// on only for a layout engine that does no CJK layout of its own — a leaf
/// crate cannot know that of its caller, and `chws` is off along a line for
/// the same reason. Each is turned on through [`Shaper::with_settings`], and
/// the interactions the registry states among them are applied there; see
/// `docs/design/shaping.md`, *Vertical shaping*.
///
/// # What a fixture adjudicates
///
/// Nothing yet: no face in either vendored corpus carries a vertical
/// feature, and every column test is a synthetic face whose expectations
/// the test computes from its own tables. H-64's person-written fixtures
/// (FT-07b.10) are what would.
pub const VERTICAL_GPOS_FEATURES: &[Tag] = &[
    Tag::new(b"abvm"),
    Tag::new(b"blwm"),
    VALT,
    VKRN,
    Tag::new(b"dist"),
    Tag::new(b"curs"),
    Tag::new(b"mark"),
    Tag::new(b"mkmk"),
];

/// `valt`, the alternate vertical metrics: on by default in a column.
const VALT: Tag = Tag::new(b"valt");
/// `vkrn`, vertical kerning: on by default in a column.
const VKRN: Tag = Tag::new(b"vkrn");
/// `vapk`, kerning for `vpal`'s proportional heights: off by default.
const VAPK: Tag = Tag::new(b"vapk");
/// `vhal`, the alternate half heights: off by default.
const VHAL: Tag = Tag::new(b"vhal");
/// `vpal`, the proportional heights: off by default.
const VPAL: Tag = Tag::new(b"vpal");
/// `vchw`, the contextual half-width spacing: off by default.
const VCHW: Tag = Tag::new(b"vchw");

/// What a glyph stands on in a column, in the face's design units.
///
/// The metrics an upright run gives each glyph before `GPOS` runs —
/// `y_advance` is `-advance` and the offsets are `(-origin_x, -origin_y)`, the
/// glyph's vertical origin at the pen — reported by
/// [`Shaper::vertical_metrics`] for a consumer that places glyphs by metrics
/// of its own and wants only what `GPOS` changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VerticalMetrics {
    /// The advance height: how far down the pen moves past the glyph.
    pub advance: i32,
    /// The vertical origin's x from the glyph's horizontal origin: half its
    /// advance width, rounded down.
    pub origin_x: i32,
    /// The vertical origin's y from the glyph's horizontal origin.
    pub origin_y: i32,
    /// Whether the face had no vertical metrics of its own for the glyph, so
    /// its advance or its origin was synthesized: one em down, the origin
    /// `880/1000` of an em above the baseline.
    pub synthesized: bool,
}

/// What every glyph of a face stands on in a column
/// ([`Shaper::column_metrics`]): one view of the face's vertical origins,
/// asked once per glyph.
#[derive(Clone, Debug)]
pub struct ColumnMetrics<'a> {
    shaper: Shaper<'a>,
    origins: VerticalOrigins<'a, 'a>,
}

impl ColumnMetrics<'_> {
    /// [`Shaper::vertical_metrics`] of `glyph`.
    #[must_use]
    pub fn of(&self, glyph: u16) -> VerticalMetrics {
        self.shaper.metrics_of(&self.origins, glyph)
    }
}

/// How a run stands: along a line, or in a column, upright or sideways.
///
/// `docs/design/shaping.md`, *Vertical shaping*. [`itemize`] makes every run
/// of a line [`Orientation::Horizontal`]; [`itemize_vertical`] makes each run
/// of a column [`Orientation::Upright`] or [`Orientation::Sideways`] by UAX
/// #50's `Vertical_Orientation`, and a caller whose consumer draws every glyph
/// upright sets a run's orientation itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Along a horizontal line, the way its level reads.
    #[default]
    Horizontal,
    /// Upright in a column: the orientation the code charts show, set top to
    /// bottom with the vertical plan and the face's vertical metrics.
    Upright,
    /// Sideways in a column: set along a line, by the plan and the metrics a
    /// line takes, and turned 90° clockwise by its consumer to stand in the
    /// column.
    Sideways,
}

/// One stretch of text in one script at one embedding level, standing one
/// way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The byte range of the paragraph's text this run covers.
    pub text: Range<usize>,
    /// The script UAX #24 gives it.
    pub script: Script,
    /// The embedding level UAX #9 gave it.
    pub level: Level,
    /// How it stands: along a line, or upright or sideways in a column.
    pub orientation: Orientation,
}

impl Run {
    /// Which way the run is set: top to bottom where it stands upright in a
    /// column, and otherwise from the parity of its level — a sideways run
    /// is shaped along a line, and reads as its level does there.
    #[must_use]
    pub const fn direction(&self) -> Direction {
        match self.orientation {
            Orientation::Upright => Direction::TopToBottom,
            Orientation::Horizontal | Orientation::Sideways => self.level.direction(),
        }
    }
}

/// A shaped run: glyphs, positions, and what it took to get them.
#[derive(Clone, Debug)]
pub struct ShapedRun {
    glyphs: Vec<ShapedGlyph>,
    direction: Direction,
    orientation: Orientation,
    units_per_em: u16,
    text: Range<usize>,
    warnings: Vec<Warning>,
}

impl ShapedRun {
    /// The glyphs, in logical order.
    #[must_use]
    pub fn glyphs(&self) -> &[ShapedGlyph] {
        &self.glyphs
    }

    /// Which way the run is set.
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// How the run stands, as its [`Run`] said.
    ///
    /// [`Orientation::Sideways`] is the mark for rotation: the glyphs were
    /// shaped along a line, and the consumer turns the run 90° clockwise to
    /// stand it in its column, so its pen runs down the column and its
    /// glyphs' tops face the column's right.
    #[must_use]
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// The face's units per em, which every number in [`ShapedRun::glyphs`]
    /// is measured in.
    ///
    /// A consumer scales by `units * size / units_per_em` — one multiply and
    /// one divide, both correctly rounded by IEEE 754 and therefore identical
    /// on every target, which is the side of ruling 4's line the rule allows.
    #[must_use]
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// The byte range of the original text this run covers.
    #[must_use]
    pub fn text(&self) -> Range<usize> {
        self.text.clone()
    }

    /// The run's length along its own axis: the sum of its glyphs'
    /// horizontal advances along a line, a sideways run's included, and down
    /// a column the sum of their vertical advances negated, since the pen
    /// moves down there.
    #[must_use]
    pub fn advance(&self) -> i32 {
        if self.direction.is_vertical() {
            return self
                .glyphs
                .iter()
                .fold(0i32, |sum, glyph| sum.saturating_sub(glyph.y_advance));
        }
        self.glyphs
            .iter()
            .fold(0i32, |sum, glyph| sum.saturating_add(glyph.x_advance))
    }

    /// Everything the face asked for that this crate refused to do.
    ///
    /// Ruling 10: empty is not the same fact as "it shaped". A run that hit
    /// [`Limits::max_glyphs`] shaped, and says so here.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

/// A face's lookup plans and coverage indexes, built when first asked and
/// kept for every [`Shaper`] made over the face with [`Shaper::with_plans`].
///
/// `docs/design/shaping.md`, *Lookup plans and coverage indexes*. A **plan**
/// is the lookups one stage resolves for one `(table, script, language,
/// features)`, and an **index**, built with the first plan that names a
/// lookup, is the glyphs each of its subtables' coverage holds, so the
/// runner passes a position no subtable could apply at without resolving or
/// probing one. Both are kept here rather than in the [`Shaper`], which is
/// `Copy` and made afresh at every call site: a `Plans` holds no borrow, so
/// it sits beside the face's bytes in whatever owns them, and every shaper
/// made over the face borrows it.
///
/// It answers for the face it was first used with. It keeps the address and
/// length of that face's `GSUB` and `GPOS`, and starts again when a shaper
/// hands it other tables, so a `Plans` used with the wrong face costs time
/// and never output. What it holds and reads is bounded by
/// [`Limits::max_index_bytes`], past which a stage is resolved per run and a
/// lookup probed at every position, as with no `Plans` at all.
///
/// The store is behind a `Mutex`, so a `Plans` may be shared by shapers on
/// several threads; each entry is an `Arc`, handed out under the lock and
/// read outside it. A clone starts empty: what a `Plans` holds is a cache of
/// its face, not a value of its own.
#[derive(Debug, Default)]
pub struct Plans {
    store: Mutex<Store>,
}

impl Clone for Plans {
    fn clone(&self) -> Plans {
        Plans::new()
    }
}

impl Plans {
    /// An empty store, which builds as shapers ask.
    #[must_use]
    pub fn new() -> Plans {
        Plans::default()
    }

    /// How many plans have been built: one for each distinct `(table,
    /// script, language, features)` a stage has asked for, and again for
    /// each after the store started over for another face.
    #[must_use]
    pub fn plans_built(&self) -> u64 {
        self.lock().plans_built
    }

    /// How many lookups' indexes have been built — once per lookup a plan
    /// names, shared by every plan naming it — counting a lookup that turned
    /// out to have none.
    #[must_use]
    pub fn indexes_built(&self) -> u64 {
        self.lock().indexes_built
    }

    /// The store, whatever a thread that panicked holding it left: every
    /// entry is complete when it is inserted, so a store is never half
    /// written.
    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The lookups one stage of `table` runs, with their indexes: the kept
    /// plan for the key, or one built now and kept, or — past
    /// [`Limits::max_index_bytes`] — resolved for this run alone.
    fn lookups(
        &self,
        layout: &Layout<'_>,
        table: &LayoutTable<'_>,
        script: Tag,
        language: Option<Tag>,
        features: Vec<(Tag, u32)>,
        limits: &Limits,
    ) -> Lookups {
        let mut store = self.lock();
        store.adopt(layout);
        let key = PlanKey {
            table: table.table(),
            script,
            language,
            features,
        };
        if let Some(kept) = store.plans.get(&key) {
            return Lookups::Kept(Arc::clone(kept));
        }
        let lookups = table.lookups_for_masked(script, language, &key.features);
        let cost = plan_cost(&key, &lookups);
        let left = limits.max_index_bytes.saturating_sub(store.spent);
        if cost > left {
            return Lookups::Fresh(lookups);
        }
        store.spent = store.spent.saturating_add(cost);
        store.plans_built = store.plans_built.saturating_add(1);
        let covered = lookups
            .iter()
            .map(|(lookup, _)| store.index(table, *lookup, limits))
            .collect();
        let plan = Arc::new(LookupPlan { lookups, covered });
        store.plans.insert(key, Arc::clone(&plan));
        Lookups::Kept(plan)
    }

    /// Every lookup index the store holds, by table and lookup, for the
    /// tests that ask which planned lookups have one.
    #[cfg(test)]
    pub(crate) fn indexes(&self) -> Vec<((Table, u16), bool)> {
        self.lock()
            .indexes
            .iter()
            .map(|(key, index)| (*key, index.is_some()))
            .collect()
    }
}

/// What a [`Plans`] holds, behind its lock.
#[derive(Debug, Default)]
struct Store {
    /// The address and length of the `GSUB` and `GPOS` the entries were
    /// built from; zero for a table the face has not.
    face: Option<[(usize, usize); 2]>,
    plans: BTreeMap<PlanKey, Arc<LookupPlan>>,
    /// Each lookup indexed, by table and lookup; `None` for one a probe must
    /// visit everywhere, or whose index the budget could not pay for.
    indexes: BTreeMap<(Table, u16), Option<Arc<LookupIndex>>>,
    /// The bytes charged against [`Limits::max_index_bytes`].
    spent: usize,
    plans_built: u64,
    indexes_built: u64,
}

impl Store {
    /// Takes the face `layout` was read from as the store's, starting again
    /// if the store's entries were built from another face's tables.
    fn adopt(&mut self, layout: &Layout<'_>) {
        let face = [
            layout.gsub().map_or((0, 0), LayoutTable::key),
            layout.gpos().map_or((0, 0), LayoutTable::key),
        ];
        if self.face != Some(face) {
            if self.face.is_some() {
                self.plans.clear();
                self.indexes.clear();
                self.spent = 0;
            }
            self.face = Some(face);
        }
    }

    /// The index of `lookup` in `table`, built if it is the first plan to
    /// name it. Whatever building it read is charged, built or not.
    fn index(
        &mut self,
        table: &LayoutTable<'_>,
        lookup: u16,
        limits: &Limits,
    ) -> Option<Arc<LookupIndex>> {
        let key = (table.table(), lookup);
        if let Some(kept) = self.indexes.get(&key) {
            return kept.clone();
        }
        self.indexes_built = self.indexes_built.saturating_add(1);
        let left = limits.max_index_bytes.saturating_sub(self.spent);
        let mut budget = left;
        let built = table
            .lookups()
            .get(usize::from(lookup))
            .and_then(|found| LookupIndex::build(table.table(), &found, &mut budget))
            .map(Arc::new);
        self.spent = self.spent.saturating_add(left.saturating_sub(budget));
        self.indexes.insert(key, built.clone());
        built
    }
}

/// What a plan is asked with: the stage's table, script, language and
/// requested `(feature, mask)` pairs, in the order asked.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PlanKey {
    table: Table,
    script: Tag,
    language: Option<Tag>,
    features: Vec<(Tag, u32)>,
}

/// What a plan costs against [`Limits::max_index_bytes`]: a fixed part, its
/// key's features, and for each lookup its pair, its index slot and the
/// store's entry for its index.
fn plan_cost(key: &PlanKey, lookups: &[(u16, u32)]) -> usize {
    64usize
        .saturating_add(key.features.len().saturating_mul(8))
        .saturating_add(lookups.len().saturating_mul(64))
}

/// One stage's lookups, and each one's index where it has one.
#[derive(Debug)]
pub(crate) struct LookupPlan {
    lookups: Vec<(u16, u32)>,
    covered: Vec<Option<Arc<LookupIndex>>>,
}

/// The lookups a stage runs: a kept plan, or resolved for one run.
pub(crate) enum Lookups {
    /// Resolved for this run alone, with no index.
    Fresh(Vec<(u16, u32)>),
    /// A [`Plans`] entry.
    Kept(Arc<LookupPlan>),
}

impl Lookups {
    /// The `(lookup, mask)` pairs, in the order they run.
    pub(crate) fn lookups(&self) -> &[(u16, u32)] {
        match self {
            Lookups::Fresh(lookups) => lookups,
            Lookups::Kept(plan) => &plan.lookups,
        }
    }

    /// Each lookup's index, by its place in [`Lookups::lookups`]; empty for
    /// a run with none.
    pub(crate) fn covered(&self) -> &[Option<Arc<LookupIndex>>] {
        match self {
            Lookups::Fresh(_) => &[],
            Lookups::Kept(plan) => &plan.covered,
        }
    }
}

/// One face, ready to shape runs.
#[derive(Clone, Copy, Debug)]
pub struct Shaper<'a> {
    face: &'a Sfnt<'a>,
    layout: Layout<'a>,
    language: Option<Tag>,
    limits: Option<Limits>,
    /// `None` is "whatever the run needs" — [`DEFAULT_GSUB_FEATURES`] in one
    /// stage, or [`JOINING_GSUB_STAGES`] where the run joins. `Some` is a
    /// caller who said exactly what they wanted, and is taken at their word.
    gsub_features: Option<&'a [Tag]>,
    gpos_features: Option<&'a [Tag]>,
    /// Features turned on or off **over** the plan, by a caller who wants
    /// the default plan with changes rather than a plan of their own. See
    /// [`Shaper::with_settings`].
    settings: &'a [(Tag, u32)],
    /// The face's kept lookup plans and coverage indexes, where the caller
    /// keeps them. See [`Shaper::with_plans`].
    plans: Option<&'a Plans>,
    /// The layout table the face's runs are shaped without, where its only
    /// layout tables are of a kind this crate does not run: `Silf`, for a
    /// Graphite description. See [`Warning::TableNotRun`].
    not_run: Option<Tag>,
    /// The AAT tables the face's runs are shaped through, where it carries
    /// no OpenType Layout table to shape them through. See [`aat_tables`].
    aat: Option<AatTables<'a>>,
    /// The AAT tables the face's runs are shaped without, where it carries no
    /// OpenType Layout table either: for substitution, a `morx` this crate
    /// cannot read, or a `mort` where it has no `morx`; for kerning, a `kerx`
    /// it cannot read. See [`aat_not_run`].
    aat_not_run: [Option<Tag>; 2],
}

/// The AAT tables a face's runs are shaped through (`docs/design/shaping.md`,
/// *AAT shaping*, *Precedence*): its `morx` for substitution and its `kerx`
/// for kerning, each where it has one this crate can read, and its `feat`,
/// which says which features' settings exclude one another.
#[derive(Clone, Copy, Debug)]
struct AatTables<'a> {
    morx: Option<Morx<'a>>,
    kerx: Option<Kerx<'a>>,
    feat: Option<Feat<'a>>,
}

/// `Silf`, the table a Graphite description's rules live in.
const SILF: u32 = u32::from_be_bytes(*b"Silf");
/// `morx`, the AAT table a face's substitutions live in.
const MORX: u32 = u32::from_be_bytes(*b"morx");
/// `mort`, the table `morx` extends, which a face may still carry alone.
const MORT: u32 = u32::from_be_bytes(*b"mort");
/// `feat`, the AAT feature name table.
const FEAT: u32 = u32::from_be_bytes(*b"feat");
/// `kerx`, the AAT table a face's kerning lives in.
const KERX: u32 = u32::from_be_bytes(*b"kerx");
/// `kern`, the feature tag whose setting of 0 is `font-kerning: none`.
const KERN: Tag = Tag::new(b"kern");

/// The table a face's runs go without, where its only layout tables are a
/// Graphite description's: a `Silf`, and no readable `GSUB` or `GPOS` and no
/// `morx` beside it (`docs/design/shaping.md`, *Graphite shaping*,
/// *Precedence*). A face that carries either OpenType Layout table shapes
/// through it as it always has, whatever else it carries.
fn table_not_run(face: &Sfnt<'_>, layout: &Layout<'_>) -> Option<Tag> {
    let graphite_only = face.table(SILF).is_some()
        && layout.gsub().is_none()
        && layout.gpos().is_none()
        && face.table(MORX).is_none();
    graphite_only.then(|| Tag::new(b"Silf"))
}

/// The AAT tables a face's runs are shaped through (`docs/design/shaping.md`,
/// *AAT shaping*, *Precedence*): where the face carries no readable `GSUB` or
/// `GPOS`, its `morx` and its `kerx`, each where this crate can read it, and
/// its `feat` beside them; `None` where it can read neither. A face carrying
/// either OpenType Layout table shapes through it as it always has, whatever
/// AAT tables it carries beside it — a face with `GSUB` and `kerx` and no
/// `GPOS` is not kerned by its `kerx`, which would change a face that
/// carries OpenType Layout.
fn aat_tables<'a>(face: &Sfnt<'a>, layout: &Layout<'a>) -> Option<AatTables<'a>> {
    if layout.gsub().is_some() || layout.gpos().is_some() {
        return None;
    }
    let morx = face.table(MORX).and_then(Morx::parse);
    let kerx = face.table(KERX).and_then(Kerx::parse);
    (morx.is_some() || kerx.is_some()).then(|| AatTables {
        morx,
        kerx,
        feat: face.table(FEAT).and_then(Feat::parse),
    })
}

/// The AAT tables a face's runs go without (`docs/design/shaping.md`, *AAT
/// shaping*, *Precedence*), where the face carries no readable `GSUB` or
/// `GPOS`: for substitution, a `morx` this crate cannot read — a version
/// other than 2 or 3, or a header cut short — or, where it carries no `morx`
/// and no Graphite description, its `mort`, the predecessor table the design
/// names a non-goal; and for kerning, a `kerx` it cannot read — a version
/// other than 2, 3 or 4, or a header cut short. A face carrying either
/// OpenType Layout table shapes through it as it always has, and is named
/// for none of them.
///
/// The manual has a face carrying both tables ignore its `mort` (*The 'mort'
/// table*, *Platform-specific Information*: "If a font has both a 'mort'
/// table and a 'morx' (extended metamorphosis) table, the 'mort' table is
/// ignored"), so such a face is shaped through its `morx` and named for
/// neither; and a `Silf` beside a `mort` is the Graphite description
/// [`table_not_run`] names.
fn aat_not_run(face: &Sfnt<'_>, layout: &Layout<'_>) -> [Option<Tag>; 2] {
    if layout.gsub().is_some() || layout.gpos().is_some() {
        return [None, None];
    }
    let substitution = match face.table(MORX) {
        Some(bytes) => Morx::parse(bytes).is_none().then(|| Tag::new(b"morx")),
        None => {
            (face.table(MORT).is_some() && face.table(SILF).is_none()).then(|| Tag::new(b"mort"))
        }
    };
    let kerning = face
        .table(KERX)
        .is_some_and(|bytes| Kerx::parse(bytes).is_none())
        .then(|| Tag::new(b"kerx"));
    [substitution, kerning]
}

impl<'a> Shaper<'a> {
    /// Reads a face's layout tables, once, for however many runs follow.
    #[must_use]
    pub fn new(face: &'a Sfnt<'a>) -> Self {
        let layout = Layout::parse(face);
        Self {
            face,
            layout,
            language: None,
            limits: None,
            gsub_features: None,
            gpos_features: None,
            settings: &[],
            plans: None,
            not_run: table_not_run(face, &layout),
            aat: aat_tables(face, &layout),
            aat_not_run: aat_not_run(face, &layout),
        }
    }

    /// The face's [`Plans`], which this shaper reads its lookup plans and
    /// coverage indexes from, and builds into where one is missing.
    ///
    /// A caller keeps one `Plans` per face beside the face's bytes and makes
    /// every shaper over the face with it, so a plan is resolved and an
    /// index built once however many shapers, runs and feature settings
    /// follow. The output is what the same shaper without it gives, glyph
    /// for glyph and warning for warning: a plan is a pure function of the
    /// table and what is asked of it, and an index passes only positions
    /// where a probe could do nothing.
    #[must_use]
    pub const fn with_plans(mut self, plans: &'a Plans) -> Self {
        self.plans = Some(plans);
        self
    }

    /// Features other than [`DEFAULT_GSUB_FEATURES`] and
    /// [`DEFAULT_GPOS_FEATURES`].
    ///
    /// For a caller that wants a ligature off, a stylistic set on, or — as
    /// `tests/text_rendering.rs` does — **nothing at all**, which is how the
    /// discriminating half of that suite's counts is measured: a case that
    /// produces the same answer with every feature switched off is a case that
    /// proves nothing about the features.
    ///
    /// A caller who says this is taken at their word in one further respect:
    /// the run gets **one stage**, even where its script joins. The staging of
    /// [`JOINING_GSUB_STAGES`] is a default plan, and a caller who has named
    /// their own features has replaced the plan rather than reordered it.
    ///
    /// Over a face shaped through its `morx` chain the two lists are one
    /// request: each chain's flags start at none rather than at its
    /// `defaultFlags`, and each tag named asks for its AAT feature's
    /// on-setting by `docs/design/shaping.md`'s table, so naming nothing runs
    /// no subtable, and a tag no AAT feature answers is named
    /// ([`Warning::FeatureNotInAat`]). Such a face is kerned by its `kerx`
    /// only where `kern` is among the tags named, as a `GPOS` plan runs `kern`
    /// only where it is named.
    #[must_use]
    pub const fn with_features(mut self, gsub: &'a [Tag], gpos: &'a [Tag]) -> Self {
        self.gsub_features = Some(gsub);
        self.gpos_features = Some(gpos);
        self
    }

    /// Features switched on or off **over** whatever plan the run gets:
    /// `css-fonts-4` §6.12's `font-feature-settings`, and §6.4's
    /// `font-kerning: none` as `kern` off.
    ///
    /// # Why this is not [`Shaper::with_features`]
    ///
    /// That one *replaces* the plan, and a replaced plan is one stage: a
    /// joining run that asked for `smcp` would lose the staging of
    /// [`JOINING_GSUB_STAGES`] and with it the forms its letters take. A
    /// setting is a change to the plan: a feature set to `0` is taken out of
    /// every stage it is in and out of the positioning list, and one set to
    /// anything else that the plan does not already apply is added to the
    /// plan's **last** substitution stage and to the positioning list — both,
    /// because a tag says nothing about which table its lookups are in, and a
    /// table that does not declare the feature finds no lookups for it. The
    /// stages keep their number and their order, so a staged plan's
    /// reordering pause still falls where it does.
    ///
    /// Where one tag is set twice the last setting wins, which is §6.12's
    /// rule for a list that repeats a feature. The value is an on or off and
    /// nothing more: an alternate index above one is a parameter this crate's
    /// alternate substitution does not carry (it takes the first, see
    /// `gsub.rs`), so a caller that needs one must refuse it rather than pass
    /// it here.
    ///
    /// # In a column
    ///
    /// Settings apply over a column's plans as over a line's, with two rules
    /// the OpenType feature registry states (`docs/design/shaping.md`,
    /// *Vertical shaping*). [`VRT2`] turned on takes [`VERT`]'s place in an
    /// upright run where the face carries it, the two never applied together;
    /// that is how a caller whose consumer draws every glyph upright — and so
    /// depends on the face for pre-rotated glyphs — asks for the face's own
    /// rotation. And a sideways run takes neither `vert` nor `vrt2`, whatever
    /// the settings, because its consumer turns it and a vertical form would
    /// be turned twice.
    ///
    /// # Over a face shaped through its `morx` chain
    ///
    /// A setting asks the chain for an AAT feature setting
    /// ([`Selection::from_opentype`]): a tag set above 0 its feature's
    /// on-setting, a tag set to 0 a non-exclusive feature's off-setting and
    /// nothing of an exclusive one, the chain's defaults standing where
    /// nothing asks otherwise. `kern` asks nothing of the chain: set to 0 it
    /// turns the face's `kerx` off, `css-fonts-4` §6.4's `font-kerning:
    /// none`, as it takes `kern` out of a `GPOS` plan. A tag no AAT feature
    /// answers is named ([`Warning::FeatureNotInAat`]).
    #[must_use]
    pub const fn with_settings(mut self, settings: &'a [(Tag, u32)]) -> Self {
        self.settings = settings;
        self
    }

    /// The effective setting of one tag, the last one written.
    fn setting(&self, tag: Tag) -> Option<u32> {
        self.settings
            .iter()
            .rev()
            .find(|(t, _)| *t == tag)
            .map(|(_, value)| *value)
    }

    /// The tags [`Shaper::with_settings`] turns on, each once, in the order
    /// they were first written.
    fn turned_on(&self) -> Vec<Tag> {
        let mut on: Vec<Tag> = Vec::new();
        for (tag, _) in self.settings {
            if self.setting(*tag).is_some_and(|value| value > 0) && !on.contains(tag) {
                on.push(*tag);
            }
        }
        on
    }

    /// The plan's substitution stages with the settings applied, and
    /// `excluded` kept out of every stage whatever the settings say.
    fn gsub_plan(&self, stages: Vec<Vec<Tag>>, excluded: &[Tag]) -> Vec<Vec<Tag>> {
        let planned: Vec<Tag> = stages.iter().flatten().copied().collect();
        let mut out: Vec<Vec<Tag>> = stages
            .into_iter()
            .map(|stage| {
                stage
                    .into_iter()
                    .filter(|tag| self.setting(*tag) != Some(0) && !excluded.contains(tag))
                    .collect()
            })
            .collect();
        if let Some(last) = out.last_mut() {
            for tag in self.turned_on() {
                if !planned.contains(&tag) && !excluded.contains(&tag) {
                    last.push(tag);
                }
            }
        }
        out
    }

    /// The substitution stages a run of `plan` takes standing `orientation`,
    /// before the settings: a line's as they are, an upright run's with
    /// [`VERT`] — or [`VRT2`] in its place — in its last stage, a sideways
    /// run's with [`VRTR`]. A caller who named its own features gets those
    /// and no more, in a column as along a line.
    fn oriented_stages(
        &self,
        plan: Plan,
        orientation: Orientation,
        gsub: &LayoutTable<'a>,
        script: Tag,
    ) -> Vec<Vec<Tag>> {
        let mut stages: Vec<Vec<Tag>> = plan
            .gsub_stages(self.gsub_features)
            .iter()
            .map(|stage| stage.to_vec())
            .collect();
        if plan == Plan::AsAsked {
            return stages;
        }
        let added = match orientation {
            Orientation::Horizontal => return stages,
            Orientation::Upright => self.vertical_alternates(gsub, script),
            Orientation::Sideways => VRTR,
        };
        if let Some(last) = stages.last_mut() {
            if !last.contains(&added) {
                last.push(added);
            }
        }
        stages
    }

    /// The features a sideways run never takes, whatever the settings: the
    /// two vertical alternates, since its consumer turns the run and a
    /// vertical form would be turned twice. A caller who named its own
    /// features is taken at its word.
    fn excluded(&self, plan: Plan, orientation: Orientation) -> &'static [Tag] {
        if orientation == Orientation::Sideways && plan != Plan::AsAsked {
            &[VERT, VRT2]
        } else {
            &[]
        }
    }

    /// An upright run's vertical alternates: [`VERT`], or [`VRT2`] in its
    /// place where a setting turns `vrt2` on and the face carries it for the
    /// run's script and language. See [`VRT2`].
    fn vertical_alternates(&self, gsub: &LayoutTable<'a>, script: Tag) -> Tag {
        let asked = self.setting(VRT2).is_some_and(|value| value > 0);
        if asked
            && !gsub
                .lookups_for_masked(script, self.language, &[(VRT2, Buffer::GLOBAL)])
                .is_empty()
        {
            VRT2
        } else {
            VERT
        }
    }

    /// The positioning features with the settings applied: a line's, or
    /// [`VERTICAL_GPOS_FEATURES`] with the registry's interactions for an
    /// upright run, or exactly what a caller named.
    fn gpos_plan(&self, orientation: Orientation, gpos: &LayoutTable<'a>, script: Tag) -> Vec<Tag> {
        let upright = orientation == Orientation::Upright;
        let base = match self.gpos_features {
            Some(asked) => asked,
            None if upright => VERTICAL_GPOS_FEATURES,
            None => DEFAULT_GPOS_FEATURES,
        };
        let mut out: Vec<Tag> = base
            .iter()
            .copied()
            .filter(|tag| self.setting(*tag) != Some(0))
            .collect();
        if upright && self.gpos_features.is_none() {
            self.glyph_heights(&mut out, gpos, script);
        }
        for tag in self.turned_on() {
            if !out.contains(&tag) {
                out.push(tag);
            }
        }
        out
    }

    /// The interactions the OpenType feature registry states among a
    /// column's glyph-height features, applied to an upright run's
    /// positioning list before the settings' own additions.
    ///
    /// - `valt`, `vhal`, `vpal` and `vchw` exclude one another: a caller who
    ///   turns on one of the last three has chosen it, and `valt`, which is
    ///   on only by default, is withdrawn.
    /// - Where `valt` applies — the face carries it for the run's script and
    ///   language — or `vhal` or `vchw` is on, `vkrn` and `vapk` "should be
    ///   disabled for that run": both are withdrawn, and a caller who turns
    ///   either on puts it back, as a setting does anywhere. `vpal` keeps
    ///   them, which the registry allows.
    ///
    /// Not applied: `vkrn` standing off, by default, from the glyphs a face's
    /// `vpal` covers where `vpal` is off and the face has no `vapk` — the
    /// exception `kern` has with `palt`, which a line does not apply either.
    fn glyph_heights(&self, out: &mut Vec<Tag>, gpos: &LayoutTable<'a>, script: Tag) {
        let on = |tag: Tag| self.setting(tag).is_some_and(|value| value > 0);
        if on(VHAL) || on(VPAL) || on(VCHW) {
            out.retain(|tag| *tag != VALT);
        }
        let valt = out.contains(&VALT)
            && !gpos
                .lookups_for_masked(script, self.language, &[(VALT, Buffer::GLOBAL)])
                .is_empty();
        if valt || on(VHAL) || on(VCHW) {
            out.retain(|tag| *tag != VKRN && *tag != VAPK);
        }
    }

    /// The OpenType language system to ask for, such as `TRK ` for Turkish.
    ///
    /// `None` — the default — takes the script's default language system,
    /// which is what a face's own designer wrote for everybody they did not
    /// single out.
    #[must_use]
    pub const fn with_language(mut self, language: Tag) -> Self {
        self.language = Some(language);
        self
    }

    /// Ceilings other than the ones [`Limits::for_glyphs`] would pick.
    #[must_use]
    pub const fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// The face's layout tables, for a caller that wants to look.
    #[must_use]
    pub const fn layout(&self) -> &Layout<'a> {
        &self.layout
    }

    /// The value each of the face's Graphite features takes for a run this
    /// shaper shapes, and what was ignored on the way; `None` where the face
    /// carries no `Silf` and so is no Graphite description.
    ///
    /// Made, each over the last, from the face's `Feat` defaults, the `Sill`
    /// settings of the language [`Shaper::with_language`] names, every
    /// feature whose id a tag of [`Shaper::with_features`] spells set to 1,
    /// and [`Shaper::with_settings`] (`docs/design/shaping.md`, *Feature
    /// values, from the shaper's settings*). A tag naming no feature the face
    /// declares, and a value the feature does not declare, is ignored and
    /// named. A face with a `Silf` and no `Feat` has no features to value; a
    /// `Feat` or `Sill` that cannot be read is answered without, and says
    /// why. No run reads these values yet: the face's passes are not run.
    #[must_use]
    pub fn graphite_features(&self) -> Option<(FeatureValues, Vec<Refusal>)> {
        self.face.table(SILF)?;
        let mut refusals: Vec<Refusal> = Vec::new();
        let features = match self.face.table(graphite::FEAT.0).map(Features::parse) {
            Some(Ok(features)) => features,
            Some(Err(refusal)) => return Some((FeatureValues::default(), vec![refusal])),
            None => return Some((FeatureValues::default(), refusals)),
        };
        let languages = match self.face.table(graphite::SILL.0).map(Languages::parse) {
            Some(Ok(languages)) => Some(languages),
            Some(Err(refusal)) => {
                refusals.push(refusal);
                None
            }
            None => None,
        };
        let mut turned_on: Vec<Tag> = Vec::new();
        for tag in self
            .gsub_features
            .unwrap_or(&[])
            .iter()
            .chain(self.gpos_features.unwrap_or(&[]))
        {
            if !turned_on.contains(tag) {
                turned_on.push(*tag);
            }
        }
        let (values, ignored) = FeatureValues::resolve(
            &features,
            languages.as_ref(),
            self.language,
            &turned_on,
            self.settings,
        );
        for refusal in ignored {
            if !refusals.contains(&refusal) {
                refusals.push(refusal);
            }
        }
        Some((values, refusals))
    }

    /// Resolves a paragraph, itemizes it, and shapes every run.
    ///
    /// The whole of steps 1 to 3 of `docs/design/shaping.md`'s pipeline. The
    /// [`Paragraph`] comes back with it because the caller needs it for step 5
    /// — reordering each line after breaking — and rebuilding it there would
    /// resolve the same levels twice.
    #[must_use]
    pub fn shape_text(&self, text: &str, direction: BaseDirection) -> (Paragraph, Vec<ShapedRun>) {
        let paragraph = Paragraph::new(text, direction);
        let runs = itemize(text, &paragraph)
            .iter()
            .map(|run| self.shape(text, run))
            .collect();
        (paragraph, runs)
    }

    /// The same for a vertical paragraph: its runs cut by orientation as
    /// well ([`itemize_vertical`]), each upright run shaped top to bottom and
    /// each sideways one along a line and marked for rotation.
    ///
    /// The [`Paragraph`]'s levels are what a consumer reads a sideways run's
    /// direction from, and nothing here lays the column out: breaking it and
    /// placing a sideways run across it are layout's.
    #[must_use]
    pub fn shape_text_vertical(
        &self,
        text: &str,
        direction: BaseDirection,
    ) -> (Paragraph, Vec<ShapedRun>) {
        let paragraph = Paragraph::new(text, direction);
        let runs = itemize_vertical(text, &paragraph)
            .iter()
            .map(|run| self.shape(text, run))
            .collect();
        (paragraph, runs)
    }

    /// What `glyph` stands on in a column: the advance height and the
    /// vertical origin an upright run gives it before `GPOS` runs.
    ///
    /// The advance is `vmtx`'s, the origin's y the `vmtx` clause's
    /// ([`tinker_pdf_font::Sfnt::vertical_origin`]) and its x half the glyph's
    /// advance width. Where the face has none of its own the shaper
    /// synthesizes them — one em down, the origin `880/1000` of an em above
    /// the baseline, CSS Writing Modes 3 §5.1.1 requiring a layout engine to
    /// synthesize what a face lacks and leaving the heuristic open — and says
    /// so in [`VerticalMetrics::synthesized`]. A consumer that places glyphs
    /// by metrics of its own subtracts these to keep only what `GPOS`
    /// changed. For a CFF face without `VORG`, each call parses the face's
    /// `CFF ` table; shaping a run parses it once, and a caller asking for
    /// many glyphs makes one [`Shaper::column_metrics`] and asks it.
    #[must_use]
    pub fn vertical_metrics(&self, glyph: u16) -> VerticalMetrics {
        self.column_metrics().of(glyph)
    }

    /// [`Shaper::vertical_metrics`] for as many glyphs as a caller asks,
    /// through one view of the face's vertical origins.
    #[must_use]
    pub fn column_metrics(&self) -> ColumnMetrics<'a> {
        ColumnMetrics {
            shaper: *self,
            origins: self.face.vertical_origins(),
        }
    }

    /// [`Shaper::vertical_metrics`], through a view of the face's origins
    /// made once for a run.
    fn metrics_of(&self, origins: &VerticalOrigins<'_, '_>, glyph: u16) -> VerticalMetrics {
        let em = i32::from(self.face.units_per_em);
        let advance = self.face.vertical_advance(glyph).map(i32::from);
        let origin_y = origins.origin(glyph);
        VerticalMetrics {
            advance: advance.unwrap_or(em),
            // `vmtx` leaves the vertical origin's x unspecified, and ISO
            // 32000-2 9.7.4.3 puts a PDF glyph's at half its width.
            origin_x: i32::from(self.face.advance(glyph).unwrap_or(0)) / 2,
            // The ideographic em box, and a PDF CIDFont's `/DW2` default:
            // `[880 -1000]`. At most 65 535 × 880, inside `i32`.
            origin_y: origin_y.unwrap_or(em * 880 / 1000),
            synthesized: advance.is_none() || origin_y.is_none(),
        }
    }

    /// Shapes one run of one script at one level.
    ///
    /// `text` is the whole paragraph and `run.text` the part to shape, so that
    /// [`ShapedGlyph::cluster`] is an offset into the text the caller has
    /// rather than into a substring it would have to add back.
    ///
    /// The run's [`Orientation`] picks the plan and the metrics: a line's
    /// along a line, the upright plan and the face's vertical metrics in a
    /// column, and a line's again for a sideways run, with `vrtr` added and
    /// `vert` and `vrt2` kept out (`docs/design/shaping.md`, *Vertical
    /// shaping*).
    ///
    /// A face carrying no readable `GSUB` or `GPOS` and a `morx` is shaped
    /// through its chain (`docs/design/shaping.md`, *AAT shaping*): the run's
    /// text mapped as typed, the chain run in the run's direction and asked
    /// for what [`Shaper::with_features`] and [`Shaper::with_settings`] say,
    /// the glyphs it deletes removed and the advances filled after it. Such a
    /// face, or one whose AAT table is a `kerx` alone, is kerned by its `kerx`
    /// where `GPOS` would run, unless `kern` is turned off.
    ///
    /// A face whose only layout tables are a Graphite description is shaped
    /// as though it carried none, and every run over it says so, once:
    /// [`Warning::TableNotRun`], naming `Silf`. So is a face whose only
    /// layout table is AAT's and one this crate does not run: a `mort`, or a
    /// `morx` it cannot read.
    #[must_use]
    pub fn shape(&self, text: &str, run: &Run) -> ShapedRun {
        let slice = text.get(run.text.clone()).unwrap_or("");
        let mut buffer = Buffer::new();
        buffer.set_direction(run.direction());
        let orientation = run.orientation;

        // Whether this run joins is asked of the text and not of the script;
        // `crate::arabic::joins` says why. A caller that asked for its own
        // feature list is taken at its word and gets one stage, because the
        // staging below is the *default* plan for a joining script and not a
        // property of the script itself.
        let plan = self.plan(slice, orientation);
        // A face shaped through its AAT chain takes its text as typed: the
        // decomposition, the joining forms and the syllables prepare the
        // OpenType plans, and the face's chain is its whole statement of how
        // its glyphs are formed. A face whose only AAT table is its `kerx`
        // has no chain, and maps its text as a face with no layout tables
        // does.
        let chain = self
            .aat
            .and_then(|aat| aat.morx.map(|morx| (morx, aat.feat)));
        let opentype = chain.is_none();
        let characters =
            self.characters(slice, run.text.start, opentype && plan == Plan::Universal);
        let from = self.map(&characters, &mut buffer);
        if opentype && plan == Plan::Joining {
            self.mark_joining_forms(&characters, &from, &mut buffer);
        }
        if opentype && plan == Plan::Universal {
            self.mark_syllables(&characters, &from, &mut buffer, run.script);
        }
        #[cfg(test)]
        {
            crate::trace::note(format!("plan {plan:?}"));
            crate::trace::stage("cmap", &buffer);
        }

        let limits = self
            .limits
            .unwrap_or_else(|| Limits::for_glyphs(buffer.len()));
        let mut warnings = Vec::new();
        // Ruling 3: a description this crate does not run yet is refused by
        // name, once a run, and the run is shaped as the face's `cmap` and
        // `hmtx` alone shape it.
        if let Some(table) = self.not_run {
            warnings.push(Warning::TableNotRun { table });
        }
        for table in self.aat_not_run.into_iter().flatten() {
            warnings.push(Warning::TableNotRun { table });
        }
        if let Some((morx, feat)) = chain {
            self.run_chain(&morx, feat.as_ref(), &mut buffer, &limits, &mut warnings);
        } else if let Some(gsub) = self.layout.gsub() {
            let script = self.script_tag(gsub, run.script);
            #[cfg(test)]
            crate::trace::note(format!("GSUB asked for script {script}"));
            let stages = self.gsub_plan(
                self.oriented_stages(plan, orientation, gsub, script),
                self.excluded(plan, orientation),
            );
            for (index, stage) in stages.iter().enumerate() {
                // The pair `rphf` was offered has to still be a pair. See
                // [`withdraw_broken_repha_pairs`].
                if plan == Plan::Universal && index == USE_RPHF_STAGE {
                    withdraw_broken_repha_pairs(&mut buffer);
                }
                let wanted: Vec<(Tag, u32)> =
                    stage.iter().map(|tag| (*tag, feature_mask(*tag))).collect();
                let lookups = self.lookups_for(gsub, script, wanted, &limits);
                warnings.extend(self.layout.substitute_planned(
                    &mut buffer,
                    lookups.lookups(),
                    lookups.covered(),
                    limits,
                ));
                #[cfg(test)]
                crate::trace::stage(
                    &format!(
                        "GSUB {index}: {}",
                        stage
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                    &buffer,
                );
                // Which syllables actually got a reph, asked here and nowhere
                // else because here is the only moment the answer is
                // unambiguous; see [`Buffer::set_repha`] and [`mark_repha`].
                if plan == Plan::Universal && index == USE_RPHF_STAGE {
                    mark_repha(&mut buffer);
                }
                // The Universal Shaping Engine's one reordering pause. It sits
                // where it does because the basic features build the conjuncts
                // and the presentation features expect them already in visual
                // order; see `crate::universal`.
                if plan == Plan::Universal && index == USE_REORDER_AFTER {
                    reorder_syllables(&mut buffer);
                    #[cfg(test)]
                    crate::trace::stage("reorder", &buffer);
                }
            }
        }

        // The joiners leave here: after the last `GSUB` stage, so that every
        // lookup saw them, and before the advances are filled, so that a face
        // which gives `ZWNJ` an outline and a width cannot spend either. See
        // [`Buffer::set_ignorable`].
        buffer.delete_ignorable();

        // The advances are filled in **after** substitution and not before,
        // and the ordering is the whole of a bug this crate had for an
        // afternoon. A substitution replaces a glyph and leaves its position
        // alone — deliberately, because `GPOS` may already have moved it — so
        // a buffer given `hmtx` widths first carries the width of the glyph
        // that *used to be* there. text-rendering-tests GSUB-2 is what caught
        // it: an Ethiopic numeral whose medial form is 985 units wide kept the
        // 1149 of the isolated form it was substituted from, and every glyph
        // after it in the line was 164 units too far right.
        if orientation == Orientation::Upright {
            self.stand_upright(&mut buffer, &mut warnings);
        } else {
            for at in 0..buffer.len() {
                let Some(glyph) = buffer.glyph(at).map(|g| g.glyph) else {
                    continue;
                };
                let advance = i32::from(self.face.advance(glyph).unwrap_or(0));
                if let Some(slot) = buffer.glyph_mut(at) {
                    slot.x_advance = advance;
                }
            }
        }

        // A column's positioning runs on the buffer's direction: a value
        // record's advance is a height there, and a cursive join and a
        // mark's attachment are resolved down the column (`crate::gpos`'s
        // `adjust` and `join`, `Buffer::propagate_attachments`).
        if let Some(gpos) = self.layout.gpos() {
            let script = self.script_tag(gpos, run.script);
            let wanted: Vec<(Tag, u32)> = self
                .gpos_plan(orientation, gpos, script)
                .into_iter()
                .map(|tag| (tag, Buffer::GLOBAL))
                .collect();
            let lookups = self.lookups_for(gpos, script, wanted, &limits);
            warnings.extend(self.layout.position_planned(
                &mut buffer,
                lookups.lookups(),
                lookups.covered(),
                limits,
                plan.mark_widths(),
            ));
        } else {
            // A face shaped through its AAT tables is kerned by its `kerx`,
            // in the positioning stage where `GPOS` would run, unless the
            // caller turned kerning off (`css-fonts-4` §6.4's
            // `font-kerning: none`, `kern` set to 0).
            if let Some(kerx) = self.aat.and_then(|aat| aat.kerx) {
                if self.kerning_on() {
                    warnings.extend(kerx.run(&mut buffer, &limits));
                }
            }
            if plan.mark_widths() == MarkWidths::ZeroByGdef {
                // A face with no `GPOS` still has marks in it, and a combining
                // accent drawn with the advance `hmtx` gave it pushes the pen
                // along and takes the word apart. `Layout::position` does this
                // at the end of positioning; without a `GPOS` nothing would.
                //
                // Conditioned for the same reason the `GPOS` branch is: a
                // Brahmic face with no `GPOS` at all would otherwise keep the
                // behaviour this commit is removing, and its syllables would
                // stack.
                self.layout.zero_marks(&mut buffer);
            }
        }
        #[cfg(test)]
        crate::trace::stage("GPOS", &buffer);

        ShapedRun {
            glyphs: buffer.glyphs().to_vec(),
            direction: run.direction(),
            orientation,
            units_per_em: self.face.units_per_em,
            text: run.text.clone(),
            warnings,
        }
    }

    /// Runs the face's `morx` chain over `buffer` (`docs/design/shaping.md`,
    /// *AAT shaping*): in the buffer's direction, asked for what
    /// [`Shaper::with_features`] and [`Shaper::with_settings`] say
    /// ([`Selection::from_opentype`], the face's `feat` saying which features
    /// are exclusive), the glyphs it deletes gone from the run when it
    /// returns. What the chain and the selection refused is added to
    /// `warnings`.
    fn run_chain(
        &self,
        morx: &Morx<'a>,
        feat: Option<&Feat<'a>>,
        buffer: &mut Buffer,
        limits: &Limits,
        warnings: &mut Vec<Warning>,
    ) {
        let named = self.named_features();
        let (selection, refused) = Selection::from_opentype(named.as_deref(), self.settings, feat);
        warnings.extend(refused);
        warnings.extend(morx.run(buffer, &selection, limits));
    }

    /// Whether a face shaped through its AAT tables is kerned by its `kerx`:
    /// unless [`Shaper::with_settings`] sets `kern` to 0 — `css-fonts-4`
    /// §6.4's `font-kerning: none` — or a caller who named its own features
    /// with [`Shaper::with_features`] named no `kern`, a setting above 0
    /// turning it back on.
    fn kerning_on(&self) -> bool {
        match self.setting(KERN) {
            Some(value) => value > 0,
            None => self
                .named_features()
                .is_none_or(|named| named.contains(&KERN)),
        }
    }

    /// The tags [`Shaper::with_features`] named — both of its lists, each tag
    /// once, in the order named — or `None` where it was not called: a tag
    /// says nothing about which table its feature is in, and on the AAT path
    /// there are no two tables to tell apart.
    fn named_features(&self) -> Option<Vec<Tag>> {
        let gsub = self.gsub_features?;
        let mut named: Vec<Tag> = Vec::new();
        for tag in gsub.iter().chain(self.gpos_features.unwrap_or(&[])) {
            if !named.contains(tag) {
                named.push(*tag);
            }
        }
        Some(named)
    }

    /// Gives every glyph of an upright run its vertical metrics: the pen
    /// moves down by the advance height, and the offsets put the glyph's
    /// vertical origin at the pen (`docs/design/shaping.md`, *The
    /// convention*). A run any glyph of which took synthesized metrics says
    /// so, once.
    fn stand_upright(&self, buffer: &mut Buffer, warnings: &mut Vec<Warning>) {
        let origins = self.face.vertical_origins();
        let mut synthesized = false;
        for at in 0..buffer.len() {
            let Some(glyph) = buffer.glyph(at).map(|g| g.glyph) else {
                continue;
            };
            let metrics = self.metrics_of(&origins, glyph);
            synthesized |= metrics.synthesized;
            if let Some(slot) = buffer.glyph_mut(at) {
                slot.x_advance = 0;
                slot.y_advance = metrics.advance.saturating_neg();
                slot.x_offset = metrics.origin_x.saturating_neg();
                slot.y_offset = metrics.origin_y.saturating_neg();
            }
        }
        if synthesized {
            warnings.push(Warning::VerticalMetricsSynthesized);
        }
    }

    /// The lookups one stage of `table` runs for `wanted`, and their coverage
    /// indexes: from the face's [`Plans`] where the caller keeps one, and
    /// resolved for this run alone, with no index, where it does not.
    fn lookups_for(
        &self,
        table: &LayoutTable<'a>,
        script: Tag,
        wanted: Vec<(Tag, u32)>,
        limits: &Limits,
    ) -> Lookups {
        match self.plans {
            Some(plans) => {
                plans.lookups(&self.layout, table, script, self.language, wanted, limits)
            }
            None => Lookups::Fresh(table.lookups_for_masked(script, self.language, &wanted)),
        }
    }

    /// Which plan this run gets. See [`Plan`].
    ///
    /// An upright run never joins: CSS Writing Modes 3 §5.1.1 shapes a
    /// cursive script set upright in its isolated forms, so its text takes
    /// the default plan, or the Universal Shaping Engine's where it holds a
    /// Brahmic character.
    fn plan(&self, text: &str, orientation: Orientation) -> Plan {
        if self.gsub_features.is_some() {
            return Plan::AsAsked;
        }
        if orientation != Orientation::Upright && arabic::joins(text) {
            return Plan::Joining;
        }
        if text.chars().any(universal::is_brahmic) {
            return Plan::Universal;
        }
        Plan::Default
    }

    /// Numbers each glyph with the Brahmic cluster its character belongs to,
    /// and records what the cluster model calls it.
    ///
    /// Both are read off the **character**, here, before any lookup has run —
    /// and both then travel on the glyph. The category has to, because by the
    /// time [`reorder_syllables`] runs there may be no character left to ask:
    /// a conjunct is one glyph standing for three, and asking the text how
    /// many glyphs a syllable ought to have is what made milestone 5 skip the
    /// clusters it most needed to reorder.
    ///
    /// Which clusters there are is the run script's question: the published
    /// grammar's for a script the Universal Shaping Engine lists, milestone
    /// 5's for the rest ([`universal::clusters`]).
    fn mark_syllables(
        &self,
        characters: &[(char, u32)],
        from: &[usize],
        buffer: &mut Buffer,
        script: Script,
    ) {
        let letters: Vec<char> = characters.iter().map(|(c, _)| *c).collect();
        let syllables: Vec<u16> = universal::clusters(&letters, script)
            .into_iter()
            .map(|(number, _)| number)
            .collect();
        let categories: Vec<universal::Category> =
            letters.iter().map(|c| universal::category(*c)).collect();
        let rphf = repha_positions(&syllables, &categories);
        for (at, index) in from.iter().enumerate() {
            if let Some(syllable) = syllables.get(*index) {
                buffer.set_syllable(at, *syllable);
            }
            if let Some(category) = categories.get(*index) {
                buffer.set_category(at, *category);
            }
            if rphf.get(*index) == Some(&true) {
                buffer.set_mask(at, Buffer::GLOBAL | MASK_RPHF);
            }
        }
    }

    /// Puts each glyph in the joining form its character is in.
    ///
    /// The forms are computed over the **text**, before `cmap`, because that
    /// is where the property lives; the mask then travels with the glyph
    /// through every substitution.
    fn mark_joining_forms(&self, characters: &[(char, u32)], from: &[usize], buffer: &mut Buffer) {
        let letters: String = characters.iter().map(|(c, _)| *c).collect();
        let forms = arabic::forms(&letters);
        for (at, index) in from.iter().enumerate() {
            if let Some(form) = forms.get(*index) {
                buffer.set_mask(at, Buffer::GLOBAL | form_mask(*form));
            }
        }
    }

    /// The OpenType script tag to ask this table for.
    ///
    /// The first of [`Script::opentype_tags`] the face actually declares, then
    /// the registry's default rule, then `DFLT`. Asked per table because a
    /// face may declare a script in `GSUB` and not in `GPOS`, and because the
    /// two are consulted separately anyway.
    ///
    /// This is where the version-2 Indic tags earn their place: a face that
    /// declares `knd2` is asked for `knd2`, and a face that declares only
    /// `knda` is asked for `knda`, without either being told about the other.
    fn script_tag(&self, table: &crate::LayoutTable<'a>, script: Script) -> Tag {
        let scripts = table.scripts();
        for tag in script.opentype_tags() {
            if scripts.find(tag).is_some() {
                return tag;
            }
        }
        let default = script.opentype_tag();
        if scripts.find(default).is_some() {
            return default;
        }
        Tag::DEFAULT_SCRIPT
    }

    /// `cmap`: text to glyph indices, with the cluster each one came from.
    ///
    /// # Variation selectors, and the one `cmap` subtable this crate reads
    /// itself
    ///
    /// `tinker_pdf_font::Sfnt::glyph_for_char` reads `cmap` formats 0, 4, 6
    /// and 12, which is every format a *renderer* needs: a PDF names glyphs by
    /// code and a variation sequence never reaches it. A **shaper** is where a
    /// variation selector is consumed — it selects a glyph and then vanishes,
    /// exactly like a joining form does — so format 14, the Unicode Variation
    /// Sequences subtable, is read here.
    ///
    /// That is the one place this crate parses a `cmap` subtable rather than
    /// asking the font crate, and the boundary is deliberate: the base
    /// mapping stays `Sfnt`'s, so there is still one reader of formats 0, 4, 6
    /// and 12 in this workspace.
    ///
    /// A selector the face has nothing to say about is **dropped**, not mapped
    /// to `.notdef`: `FVS` and `VS1`–`VS256` are `Default_Ignorable_Code_Point`
    /// and a reader that drew a box for one would put a box in the middle of a
    /// Han sentence.
    fn map(&self, characters: &[(char, u32)], buffer: &mut Buffer) -> Vec<usize> {
        let uvs = variation_subtable(self.face);
        let mut from = Vec::with_capacity(characters.len());
        let mut at = 0usize;
        while let Some((c, cluster)) = characters.get(at).copied() {
            at += 1;
            if is_variation_selector(c) {
                // A selector with no base before it selects nothing. It is
                // still ignorable, so it is dropped rather than drawn.
                continue;
            }
            let selector = characters
                .get(at)
                .map(|(next, _)| *next)
                .filter(|next| is_variation_selector(*next));
            let mut glyph = None;
            if let Some(selector) = selector {
                at += 1;
                glyph = uvs.and_then(|data| variation_glyph(data, c, selector, self.face));
            }
            let glyph = glyph.or_else(|| self.face.glyph_for_char(c)).unwrap_or(0);
            buffer.push(glyph, cluster);
            // A joiner is pushed like anything else and marked for deletion at
            // the end of `GSUB`. It has to be *in* the buffer for the whole of
            // substitution, because blocking a ligature is the whole of what
            // it is for; see [`Buffer::set_ignorable`].
            if is_joiner(c) {
                buffer.set_ignorable(buffer.len().saturating_sub(1), true);
            }
            from.push(at - 1 - usize::from(selector.is_some()));
        }
        from
    }

    /// The characters this run is shaped from, each with the cluster it stands
    /// for.
    ///
    /// Ordinarily this is just the run's own characters and their byte
    /// offsets. For a Brahmic run it is those characters **canonically
    /// decomposed**, which is the one preprocessing step the cluster model
    /// cannot do without.
    ///
    /// # Why decomposition is a shaping step and not a nicety
    ///
    /// `U+1B40 BALINESE VOWEL SIGN TALING TEDUNG` is
    /// `Indic_Positional_Category` `Left_And_Right`: it is drawn on **both**
    /// sides of its consonant. As one character it has one position and one
    /// class, and `crate::universal::category` can only call the whole of it
    /// a pre-base vowel, its right half included. Its canonical decomposition
    /// — `U+1B3E` (`Left`) and `U+1B35` (`Right`) — is two characters with two
    /// positions, and the reordering pause can then move the first and leave
    /// the second: USE's *Split vowel handling*.
    ///
    /// Every part keeps the **whole** character's cluster, so a decomposition
    /// invents no offsets the caller's text does not have and `/ToUnicode`
    /// still rebuilds the original.
    ///
    /// # Three limits, each deliberate
    ///
    /// - It is applied only in a run the cluster model shapes — one holding a
    ///   character Unicode gives an `Indic_Syllabic_Category`
    ///   (`crate::universal::is_brahmic`). A Latin run's precomposed accents
    ///   are left alone: nothing here needs their halves, and decomposing them
    ///   would change every Latin run's glyphs for no fixture's benefit.
    /// - It is applied only when the face has a glyph for **every** part.
    ///   Decomposing into a `.notdef` would replace a drawable character with
    ///   an undrawable one, which is worse than not decomposing.
    /// - Canonical **ordering** — the `Canonical_Combining_Class` sort that
    ///   makes a decomposition NFD rather than merely decomposed — is not
    ///   done. Nothing in the vendored corpus reaches a Brahmic cluster whose
    ///   parts are out of canonical order; `docs/features/fonts.md` records
    ///   it, and Hangul, whose decomposition is algorithmic rather than
    ///   tabulated, is not decomposed at all.
    fn characters(&self, text: &str, base: usize, decompose: bool) -> Vec<(char, u32)> {
        let mut out = Vec::with_capacity(text.len());
        for (at, c) in text.char_indices() {
            let cluster = u32::try_from(base.saturating_add(at)).unwrap_or(u32::MAX);
            let parts = decompose
                .then(|| unicode::canonical_decomposition(c))
                .flatten()
                .filter(|parts| {
                    parts
                        .iter()
                        .all(|part| self.face.glyph_for_char(*part).is_some_and(|g| g != 0))
                });
            match parts {
                Some(parts) => out.extend(parts.iter().map(|part| (*part, cluster))),
                None => out.push((c, cluster)),
            }
        }
        out
    }
}

/// Which of the three plans a run gets.
///
/// The choice is made from the **text**, never from a transcribed list of
/// scripts, for the reason `crate::arabic::joins` gives: a list of scripts here
/// would be somebody else's list and would be wrong the day Unicode gives an
/// existing script a joining letter or a dependent vowel, and the properties
/// themselves cannot be.
///
/// A run can satisfy both tests — Arabic-script text with a Brahmic character
/// in it does — and joining wins, because the joining forms are what a reader
/// of that run would notice first and because no fixture in the corpus mixes
/// them. That precedence is a choice and is recorded as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    /// One stage, [`DEFAULT_GSUB_FEATURES`]: Latin, Greek, Han, Hebrew.
    Default,
    /// [`JOINING_GSUB_STAGES`], and a form per glyph.
    Joining,
    /// [`USE_GSUB_STAGES`], syllables, and one reordering pause.
    Universal,
    /// A caller who named their own features, taken at their word.
    AsAsked,
}

impl Plan {
    /// Whether this plan zeroes the advance of a glyph `GDEF` calls a mark.
    ///
    /// The two families want opposite answers and the corpus is what says so.
    ///
    /// A **joining** script's marks are drawn on top of the letter they belong
    /// to, and a face that gives one a real `hmtx` advance would push the pen
    /// along and take the word apart -- so those are zeroed. Every `GDEF` mark
    /// in `TestShapeAran` and in the three `TestGPOS` faces has an advance of
    /// zero already, which is the same statement made by the faces.
    ///
    /// A **Brahmic** script's are not marks in that sense. Thirteen of
    /// `NotoSansKannada`'s fourteen `GDEF` marks are *spacing* matras with real
    /// advances -- U+0CC2 UU is 1526 units at 2048 per em -- and
    /// text-rendering-tests SHKNDA-3's expected positions are exactly the
    /// cumulative `hmtx` sum with those advances intact. Zeroing them stacked
    /// every glyph of a syllable at one x.
    ///
    /// # What this is not conditioned on, and why
    ///
    /// `General_Category` is the obvious refinement -- zero `Mn`, keep `Mc` --
    /// and it is **wrong**. U+0CBF (advance 669) and U+0CC6 (712) are `Mn`, and
    /// the corpus counts their advances. It was tried and refuted before this
    /// was written.
    ///
    /// Nor on a list of scripts: the plan is already chosen from the text (see
    /// [`Plan`]), so conditioning on the plan inherits that and adds no list.
    const fn mark_widths(self) -> MarkWidths {
        match self {
            // 14.7's own answer for a caller who named their own features: the
            // default, because a caller asking for `liga` on Latin is not
            // asking about matras.
            Plan::Default | Plan::Joining | Plan::AsAsked => MarkWidths::ZeroByGdef,
            Plan::Universal => MarkWidths::AsSupplied,
        }
    }

    /// The stages this plan runs, in order.
    fn gsub_stages(self, asked: Option<&[Tag]>) -> Vec<&[Tag]> {
        match self {
            Plan::AsAsked => vec![asked.unwrap_or(DEFAULT_GSUB_FEATURES)],
            Plan::Default => vec![DEFAULT_GSUB_FEATURES],
            Plan::Joining => JOINING_GSUB_STAGES.to_vec(),
            Plan::Universal => USE_GSUB_STAGES.to_vec(),
        }
    }
}

/// The `GSUB` features a Brahmic run turns on, in stages.
///
/// The Universal Shaping Engine's own order. Each entry is a stage for the
/// same reason [`JOINING_GSUB_STAGES`]'s are: the face's `blwf` lookup is
/// written expecting `rphf` to have finished, and merging them would apply
/// whichever the compiler happened to lay out first.
///
/// # The four groups
///
/// 1. **Normalisation.** `locl`, `ccmp`, `nukt`, `akhn` — get the cluster into
///    the shape the rest of the plan expects.
/// 2. **Reordering group.** `rphf` then `pref`, each its own stage because
///    each records something the next depends on.
/// 3. **Orthographic unit shaping.** The seven that build the conjuncts:
///    `rkrf`, `abvf`, `blwf`, `half`, `pstf`, `vatu`, `cjct`. The reordering
///    pause is **after** this group, at [`USE_REORDER_AFTER`].
/// 4. **Presentation.** `abvs`, `blws`, `haln`, `pres`, `psts`, applied to a
///    cluster that is by then in the order it will be drawn in.
///
/// The topographical group — `isol`, `init`, `medi`, `fina` — is deliberately
/// **absent**. USE applies them to scripts that join, and the four masks this
/// crate has are set by `crate::arabic::forms`, which a Brahmic run does not
/// run; requesting them unmasked would apply a joining form to every glyph of
/// every syllable. No face in the vendored corpus declares them for a Brahmic
/// script, so nothing here is lost that a fixture could see, and the omission
/// is named in `docs/features/fonts.md` rather than left to be discovered.
///
/// The default features are the last stage rather than the first, so that a
/// Brahmic face's `liga` and `calt` see the cluster after it has been built.
pub const USE_GSUB_STAGES: &[&[Tag]] = &[
    &[
        Tag::new(b"locl"),
        Tag::new(b"ccmp"),
        Tag::new(b"nukt"),
        Tag::new(b"akhn"),
    ],
    &[Tag::new(b"rphf")],
    &[Tag::new(b"pref")],
    &[
        Tag::new(b"rkrf"),
        Tag::new(b"abvf"),
        Tag::new(b"blwf"),
        Tag::new(b"half"),
        Tag::new(b"pstf"),
        Tag::new(b"vatu"),
        Tag::new(b"cjct"),
    ],
    &[
        Tag::new(b"abvs"),
        Tag::new(b"blws"),
        Tag::new(b"haln"),
        Tag::new(b"pres"),
        Tag::new(b"psts"),
    ],
    DEFAULT_GSUB_FEATURES,
];

/// Moves every pre-base glyph of every syllable in front of its base.
///
/// # The permutation is over glyphs, and that is the whole of the fix
///
/// This was computed over the **characters** of each syllable and applied to
/// the *glyphs* carrying that syllable's number, which is the same thing only
/// while substitution has not changed how many glyphs a character stands for.
/// Where it had — a conjunct built out of three characters, a decomposition
/// that made two glyphs out of one — the two lengths differed and the syllable
/// was **left alone**, so a face that forms its conjuncts before the
/// reordering pause got no reordering in exactly the clusters where it
/// mattered. text-rendering-tests SHKNDA-2 is where that cost the most.
///
/// The categories now travel on the glyphs ([`Buffer::set_category`]), so
/// there is no length to disagree about: whatever `GSUB` did to the syllable,
/// every glyph in it still says what the cluster model calls it, and a
/// ligature says what its first component did.
fn reorder_syllables(buffer: &mut Buffer) {
    for range in buffer.syllable_ranges() {
        let of_syllable: Vec<universal::Category> = range
            .clone()
            .filter_map(|at| buffer.props_category(at))
            .collect();
        if of_syllable.len() != range.len() {
            continue;
        }
        if let Some(order) = universal::reorder(&of_syllable) {
            buffer.reorder(range.clone(), &order);
        }
        // And then the reph, which is a different move for a different reason
        // and is therefore not `universal::reorder`'s: that function answers
        // the cluster model's question about categories, and this one is about
        // what a face's `rphf` did. See [`move_repha`].
        move_repha(buffer, range);
    }
}

/// Which stage of [`USE_GSUB_STAGES`] the reordering pause follows.
///
/// Index 3, the orthographic-unit-shaping group. Before it a pre-base vowel is
/// still where it was typed, which is what the conjunct-forming lookups are
/// written against; after it the cluster is in the order it will be drawn in,
/// which is what the presentation lookups are written against.
const USE_REORDER_AFTER: usize = 3;

/// Which stage of [`USE_GSUB_STAGES`] is `rphf`'s.
///
/// Index 1, its own stage, and [`the_stage_indices_name_the_stages_they_mean`]
/// is what stops this and [`USE_REORDER_AFTER`] drifting off the array they
/// index.
const USE_RPHF_STAGE: usize = 1;

/// Where `rphf` may fire, as a flag per character.
///
/// # A repha is a consonant that has a base to sit on
///
/// `rphf` rewrites a syllable-initial `RA` and its halant into a reph — the
/// mark drawn above the syllable — and the *whole* of what makes it a reph
/// rather than a dead consonant is that there is something after it for the
/// syllable to be about. A word-final `RA` + halant is not a reph; it is a
/// consonant with its vowel killed, and the face's `haln` lookup is what draws
/// it.
///
/// This crate asked the face for `rphf` over every syllable, so
/// text-rendering-tests `SHKNDA-2/7` — `ಜಾ಼ಕಿರ್`, which ends U+0CB0 U+0CCD —
/// got the reph gid94 where the fixture wants the halant form gid193.
///
/// So the condition is stated here and carried as a mask: the first two
/// characters of the syllable are a base and a halant, and a base follows them
/// **inside the same syllable**. Both of the two are marked, because a
/// feature's mask is checked against every glyph of a rule's *input* and
/// `rphf` is a ligature over the pair; see [`crate::apply`]'s `Skipper::mask`.
///
/// # What this is not
///
/// It does not say which consonant `RA` is, and it must not: that is the
/// face's own `rphf` coverage table, which is the only place in the system
/// that knows what a face means by a repha. This narrows *where* the lookup is
/// offered a position, never *which* glyph it accepts. A face whose `rphf`
/// covers nothing loses nothing.
fn repha_positions(syllables: &[u16], categories: &[universal::Category]) -> Vec<bool> {
    let mut out = vec![false; syllables.len()];
    let mut at = 0usize;
    while at < syllables.len() {
        let syllable = syllables.get(at).copied().unwrap_or(0);
        let mut end = at;
        while syllables.get(end) == Some(&syllable) {
            end = end.saturating_add(1);
        }
        let after = at.saturating_add(2);
        let role = |at: usize| categories.get(at).map(|category| category.role());
        if syllable != 0
            && role(at) == Some(universal::Role::Base)
            && role(at.saturating_add(1)) == Some(universal::Role::Halant)
            && categories.get(after..end).is_some_and(|rest| {
                rest.iter()
                    .any(|category| category.role() == universal::Role::Base)
            })
        {
            for flag in out.get_mut(at..after).unwrap_or_default() {
                *flag = true;
            }
        }
        at = end.max(at.saturating_add(1));
    }
    out
}

/// Takes [`MASK_RPHF`] back from a syllable whose pair is no longer a pair.
///
/// # The normalisation stage can eat the halant, and this is what it costs
///
/// [`repha_positions`] marks two *characters*, and by the time `rphf` runs the
/// stage before it has had a turn at them. `akhn` is the one that matters:
/// KA + VIRAMA + SSA is the classic akhand ligature and every Indic face has
/// it, and the one glyph it produces keeps KA's props — including the bit.
///
/// The syllable then looks exactly like a reph that has already formed: one
/// glyph carrying the bit followed by one that does not. text-rendering-tests
/// `SHKNDA-3/31` is where that showed: `ಕ್ಷಿ` became gid282 with the bit still
/// on it, [`mark_repha`] called it a reph, and moving it to the end of the
/// syllable stopped `ಕ್ಷ` and its `I` ligating into gid285.
///
/// So the bit is withdrawn from any syllable whose first *two* glyphs do not
/// both still carry it, immediately before the stage that would use it. That
/// makes both readings of the bit exact: `rphf` is offered only a pair that is
/// still two glyphs, and one glyph carrying the bit afterwards means the
/// lookup fired.
fn withdraw_broken_repha_pairs(buffer: &mut Buffer) {
    for range in buffer.syllable_ranges() {
        let second = range.start.saturating_add(1);
        let intact = second < range.end
            && buffer.props_mask(range.start) & MASK_RPHF != 0
            && buffer.props_mask(second) & MASK_RPHF != 0;
        if intact {
            continue;
        }
        for at in range {
            let mask = buffer.props_mask(at) & !MASK_RPHF;
            buffer.set_mask(at, mask);
        }
    }
}

/// Records, per syllable, whether `rphf` actually produced a reph.
///
/// # The question the mask cannot answer on its own
///
/// [`repha_positions`] says where the lookup was *offered* a position. Whether
/// it took one is the face's answer, and the buffer is where it is written:
/// the pair went in as two glyphs both carrying [`MASK_RPHF`], and a reph
/// comes out as **one** glyph carrying it with the halant gone.
///
/// So the test is exactly that — the syllable's first glyph carries the bit
/// and its second does not — and it is asked immediately after the `rphf`
/// stage, at [`USE_RPHF_STAGE`], because that is the only moment it is
/// unambiguous. `half` in the stage after next forms the same shape out of the
/// same two characters and is not a reph; asking at the reordering pause would
/// call it one.
fn mark_repha(buffer: &mut Buffer) {
    for range in buffer.syllable_ranges() {
        let first = range.start;
        let second = first.saturating_add(1);
        let fired = buffer.props_mask(first) & MASK_RPHF != 0
            && second < range.end
            && buffer.props_mask(second) & MASK_RPHF == 0;
        if fired {
            buffer.set_repha(first, true);
        }
    }
}

/// Moves a reph to the end of its syllable.
///
/// # One case says this, and it says it exactly
///
/// text-rendering-tests `SHKNDA-2/12` is `ಮಾರ್ಚ್` — `MA AA RA VIRAMA CHA
/// VIRAMA` — and its second syllable reaches the reordering pause as gid94
/// (the reph `rphf` just made), gid25 (`CHA`) and gid70 (the virama). The
/// fixture expects gid172 then gid94: the reph **last**, and `haln` in the
/// stage after the pause is what turns the remaining `CHA` and virama into
/// gid172. Moving the reph to the end is what puts those two next to each
/// other for it.
///
/// # What is claimed, and what is a guess
///
/// The Indic model gives a face several places a reph may be repositioned to —
/// after the base, after the first matra, before a post-base matra, at the end
/// of the syllable — and reads which from the face's own tables. **This
/// implements one of them and does not read anything.** `SHKNDA-2/12` is the
/// only case in either vendored corpus with a reph in it, so "the end of the
/// syllable" is what one fixture says and not a position this crate chose
/// between alternatives it could see. A face wanting one of the others has no
/// fixture here and would fail here first, which is the honest state to leave
/// it in.
fn move_repha(buffer: &mut Buffer, range: Range<usize>) {
    let Some(at) = range.clone().find(|at| buffer.props_repha(*at)) else {
        return;
    };
    if at.saturating_add(1) >= range.end {
        return;
    }
    let mut order: Vec<usize> = (0..range.len()).collect();
    let lifted = order.remove(at.saturating_sub(range.start));
    order.push(lifted);
    buffer.reorder(range, &order);
}

/// The mask bit of each joining form.
///
/// Bit 0 is [`Buffer::GLOBAL`] and is on every glyph, so these start at bit 1.
/// Four bits and no more: a glyph is in exactly one of the four forms, or in
/// none.
const MASK_ISOL: u32 = 1 << 1;
/// See [`MASK_ISOL`].
const MASK_FINA: u32 = 1 << 2;
/// See [`MASK_ISOL`].
const MASK_MEDI: u32 = 1 << 3;
/// See [`MASK_ISOL`].
const MASK_INIT: u32 = 1 << 4;

/// The mask bit of a position `rphf` is offered. See [`repha_positions`].
///
/// Bit 5, and it cannot collide with the four above: a run gets **one** plan,
/// [`Plan::Joining`] is the only one that sets a form bit and [`Plan::Universal`]
/// is the only one that sets this, so no glyph in any run carries bits from
/// both sets.
const MASK_RPHF: u32 = 1 << 5;

/// The mask a glyph in this form carries.
const fn form_mask(form: arabic::Form) -> u32 {
    match form {
        arabic::Form::Isolated => MASK_ISOL,
        arabic::Form::Final => MASK_FINA,
        arabic::Form::Medial => MASK_MEDI,
        arabic::Form::Initial => MASK_INIT,
        arabic::Form::None => 0,
    }
}

/// The mask a feature is requested under.
///
/// The four joining features are restricted to the glyphs in their form;
/// everything else is global. The match is on the tag rather than on a flag
/// beside it in [`JOINING_GSUB_STAGES`], so that a caller who names `fina` in
/// [`Shaper::with_features`] gets the same restriction the default plan would
/// have given it — one answer to "what does `fina` mean", not two.
const fn feature_mask(tag: Tag) -> u32 {
    match tag.0 {
        t if t == Tag::new(b"isol").0 => MASK_ISOL,
        t if t == Tag::new(b"fina").0 => MASK_FINA,
        t if t == Tag::new(b"medi").0 => MASK_MEDI,
        t if t == Tag::new(b"init").0 => MASK_INIT,
        // And the same restriction for `rphf`, for the same reason and by the
        // same route: the face's lookup covers every `RA` in the font and the
        // *position* is what says whether one is a repha. See
        // [`repha_positions`].
        t if t == Tag::new(b"rphf").0 => MASK_RPHF,
        _ => Buffer::GLOBAL,
    }
}

/// UAX #24 itemization: runs of one script at one embedding level.
///
/// # What the rule is
///
/// `Common` and `Inherited` characters have no script of their own — a space,
/// a full stop, a combining accent — so they take the script of what they
/// follow, and, at the start of the text, of what they precede. A run of
/// nothing else stays `Common`, which resolves to an OpenType tag no face
/// declares and therefore to `DFLT`, which is the right answer for a line of
/// mathematical symbols.
///
/// # What it is not
///
/// UAX #24's full algorithm uses `Script_Extensions` — the set of scripts a
/// character is used in, which is a different and larger property than
/// `Script` — and resolves paired brackets to a common script. Neither is
/// vendored here. The consequence is a run boundary in the wrong place for a
/// character several scripts share, such as U+0964 DEVANAGARI DANDA between
/// Devanagari and Bengali; every script that reaches is scheduled for
/// milestone 5, and none of this milestone's fixtures contains one.
///
/// Every run is [`Orientation::Horizontal`]: this is a line's itemization.
/// A column's is [`itemize_vertical`].
#[must_use]
pub fn itemize(text: &str, paragraph: &Paragraph) -> Vec<Run> {
    cut(text, paragraph, &[])
}

/// A vertical paragraph's itemization: runs of one script at one embedding
/// level, standing one way in the column.
///
/// The scripts and levels are [`itemize`]'s, and each run is cut a third way,
/// by orientation: a character stands [`Orientation::Upright`] where its UAX
/// #50 `Vertical_Orientation` is `U`, `Tu` or `Tr`, and
/// [`Orientation::Sideways`] where it is `R` — CSS Writing Modes 3 §5.1.2's
/// rule for mixed orientation (see
/// [`crate::unicode::VerticalOrientation::is_upright`] for `Tr`).
///
/// # A grapheme cluster stands as one
///
/// UAX #50 §3.2.1 orients a cluster as a whole: as its first character
/// stands, or upright where it holds an enclosing mark — a Latin letter in a
/// combining circle is a symbol set upright, whatever the letter alone would
/// be. The cluster is read from what this crate vendors: a character
/// continues the one before it when its `General_Category` is a mark (`Mn`,
/// `Mc`, `Me`) or its `Script` is `Inherited`, which takes in `ZWJ`, `ZWNJ`
/// and the variation selectors. That is a base and the marks after it; the
/// rest of UAX #29 is not computed, and `docs/features/fonts.md` names what
/// that leaves out. A mark that opens the text has nothing to continue, and
/// stands as its own value says.
#[must_use]
pub fn itemize_vertical(text: &str, paragraph: &Paragraph) -> Vec<Run> {
    cut(text, paragraph, &column_orientations(text))
}

/// How each character of a vertical paragraph stands, by its grapheme
/// cluster. See [`itemize_vertical`].
fn column_orientations(text: &str) -> Vec<Orientation> {
    let mut out: Vec<Orientation> = Vec::with_capacity(text.len());
    let mut cluster = 0usize;
    for c in text.chars() {
        let mark = unicode::mark_category(c);
        let continues =
            !out.is_empty() && (mark.is_some() || unicode::script(c) == Script::Inherited);
        if continues {
            let first = out.get(cluster).copied().unwrap_or(Orientation::Sideways);
            out.push(first);
        } else {
            cluster = out.len();
            out.push(if unicode::vertical_orientation(c).is_upright() {
                Orientation::Upright
            } else {
                Orientation::Sideways
            });
        }
        if mark == Some(unicode::MarkCategory::Me) {
            for slot in out.get_mut(cluster..).unwrap_or_default() {
                *slot = Orientation::Upright;
            }
        }
    }
    out
}

/// Runs of one script, one level and one orientation; `orientations`, one
/// per character, empty for a line, whose runs are all horizontal.
fn cut(text: &str, paragraph: &Paragraph, orientations: &[Orientation]) -> Vec<Run> {
    let mut scripts: Vec<(usize, Script)> = Vec::new();
    for (at, c) in text.char_indices() {
        scripts.push((at, unicode::script(c)));
    }
    // Forwards: a Common or Inherited character joins what precedes it.
    let mut carried = Script::Common;
    for (_, script) in &mut scripts {
        if matches!(*script, Script::Common | Script::Inherited) {
            *script = carried;
        } else {
            carried = *script;
        }
    }
    // Backwards: and the ones at the very start join what follows, since
    // there was nothing in front of them to join.
    let mut carried = Script::Common;
    for (_, script) in scripts.iter_mut().rev() {
        if matches!(*script, Script::Common | Script::Inherited) {
            *script = carried;
        } else {
            carried = *script;
        }
    }

    let levels = paragraph.levels();
    let mut runs: Vec<Run> = Vec::new();
    for (index, (at, script)) in scripts.iter().enumerate() {
        let level = levels.get(index).copied().unwrap_or(paragraph.base_level());
        let orientation = orientations.get(index).copied().unwrap_or_default();
        match runs.last_mut() {
            Some(run)
                if run.script == *script
                    && run.level == level
                    && run.orientation == orientation =>
            {
                run.text.end = *at;
            }
            _ => runs.push(Run {
                text: *at..*at,
                script: *script,
                level,
                orientation,
            }),
        }
    }
    if let Some(run) = runs.last_mut() {
        run.text.end = text.len();
    }
    // The `end` of every run but the last was set to the start of the
    // character that ended it, which is where the next run begins.
    for index in 0..runs.len().saturating_sub(1) {
        runs[index].text.end = runs[index + 1].text.start;
    }
    runs
}

/// `ZWJ` and `ZWNJ`, and nothing else.
///
/// # Which property, and which one deliberately not
///
/// `Indic_Syllabic_Category`'s `Joiner` and `Non_Joiner`, which this crate
/// already parses for [`universal::category`] — so the predicate costs no new
/// table and cannot drift from the one the cluster model reads.
///
/// The obvious alternative is `Default_Ignorable_Code_Point`, which is the
/// property a shaper is *usually* written against and which covers these two
/// along with the variation selectors, the Mongolian free variation selectors,
/// `U+00AD SOFT HYPHEN` and some seventy more. It is deliberately **not** used
/// here: nothing in either vendored corpus reaches a default-ignorable
/// character that is not one of these two, so vendoring a hundred-odd-entry
/// property to widen a predicate no fixture exercises would be a table nobody
/// could adjudicate. The narrower predicate is named as narrower, in
/// `docs/features/fonts.md`, rather than being quietly the whole answer.
///
/// The variation selectors *are* default-ignorable and *are* dropped — in
/// [`Shaper::map`], where they are consumed rather than deleted, because a
/// selector chooses a glyph and a joiner does not.
fn is_joiner(c: char) -> bool {
    matches!(
        unicode::indic_syllabic(c),
        unicode::IndicSyllabic::Joiner | unicode::IndicSyllabic::NonJoiner
    )
}

/// The variation selectors: `VS1`–`VS16`, and the 240 ideographic ones.
///
/// The Mongolian free variation selectors at U+180B–U+180F are deliberately
/// not here: they are `Mn`, they take part in Mongolian shaping rather than in
/// `cmap` format 14, and treating one as a selector would swallow it.
const fn is_variation_selector(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// The face's `cmap` format 14 subtable, if it has one.
fn variation_subtable<'a>(face: &Sfnt<'a>) -> Option<Bytes<'a>> {
    let cmap = Bytes::new(face.table(u32::from_be_bytes(*b"cmap"))?);
    let count = usize::from(cmap.u16(2)?);
    for n in 0..count.min(64) {
        let at = 4usize.checked_add(n.checked_mul(8)?)?;
        let sub = cmap.offset32(at.checked_add(4)?)?;
        if sub.u16(0) == Some(14) {
            return Some(sub);
        }
    }
    None
}

/// One `UnicodeValue`, the 24-bit code point a format 14 subtable stores.
fn u24(data: Bytes<'_>, at: usize) -> Option<u32> {
    Some(u32::from(data.u8(at)?) << 16 | u32::from(data.u16(at.checked_add(1)?)?))
}

/// A `(base, selector)` pair's glyph, through `cmap` format 14.
///
/// Three outcomes, and the difference between the second and the third is the
/// whole reason this subtable exists:
///
/// - the pair is a **non-default** sequence, and the subtable names the glyph;
/// - the pair is a **default** sequence, meaning "the ordinary glyph for the
///   base is already the right one", so the base `cmap` answers;
/// - the subtable has never heard of the pair, and the selector is ignored,
///   which also means the base `cmap` answers.
fn variation_glyph(data: Bytes<'_>, base: char, selector: char, face: &Sfnt<'_>) -> Option<u16> {
    let records = data.u32(6)?;
    let wanted = u32::from(selector);
    // The records are sorted by selector, and there are at most 256 of them.
    let mut found = None;
    for n in 0..records.min(1024) {
        let at = 10usize.checked_add(usize::try_from(n).ok()?.checked_mul(11)?)?;
        if u24(data, at)? == wanted {
            found = Some(at);
            break;
        }
    }
    let at = found?;
    let default_uvs = data.u32(at.checked_add(3)?)?;
    let non_default_uvs = data.u32(at.checked_add(7)?)?;
    let code = u32::from(base);

    if non_default_uvs != 0 {
        let table = data.at(usize::try_from(non_default_uvs).ok()?)?;
        let count = table.u32(0)?;
        for n in 0..count.min(0x0011_0000) {
            let entry = 4usize.checked_add(usize::try_from(n).ok()?.checked_mul(5)?)?;
            if u24(table, entry)? == code {
                return table.u16(entry.checked_add(3)?);
            }
        }
    }
    if default_uvs != 0 {
        let table = data.at(usize::try_from(default_uvs).ok()?)?;
        let count = table.u32(0)?;
        for n in 0..count.min(0x0011_0000) {
            let entry = 4usize.checked_add(usize::try_from(n).ok()?.checked_mul(4)?)?;
            let start = u24(table, entry)?;
            let extra = u32::from(table.u8(entry.checked_add(3)?)?);
            if code >= start && code <= start.saturating_add(extra) {
                // A default sequence says the base glyph is already right.
                return face.glyph_for_char(base);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{is_joiner, is_variation_selector, itemize, Plan, Plans, ShapedRun, Shaper};
    use crate::bidi::{BaseDirection, Paragraph};
    use crate::buffer::ShapedGlyph;
    use crate::common::Tag;
    use crate::limits::Limits;
    use crate::unicode::Script;
    use crate::{MarkWidths, Warning};
    use tinker_pdf_font::Sfnt;

    /// The Nasta'liq test face, and Urdu in the letters it maps.
    const ARABIC_FACE: &[u8] =
        include_bytes!("../data/text-rendering-tests/fonts/TestShapeAran.ttf");
    const URDU: &str = "\u{646}\u{633}\u{62a}\u{639}\u{644}\u{6cc}\u{642} \u{62e}\u{637} \
                        \u{641}\u{646} \u{642}\u{627}\u{646}\u{648}\u{646} \u{6a9}\u{644}";

    /// A Kannada face, and Kannada with a reph and a conjunct.
    const KANNADA_FACE: &[u8] =
        include_bytes!("../data/text-rendering-tests/fonts/NotoSansKannada-Regular.ttf");
    const KANNADA: &str = "\u{c95}\u{cb0}\u{ccd}\u{ca8}\u{cbe}\u{c9f}\u{c95} \
                           \u{caa}\u{cc1}\u{cb8}\u{ccd}\u{ca4}\u{c95}";

    /// Each run's glyphs and warnings.
    fn output(runs: &[ShapedRun]) -> Vec<(Vec<ShapedGlyph>, Vec<Warning>)> {
        runs.iter()
            .map(|run| (run.glyphs().to_vec(), run.warnings().to_vec()))
            .collect()
    }

    /// `text` shaped through `face`, with `plans` where it is given.
    fn shaped(
        face: &Sfnt<'_>,
        text: &str,
        limits: Limits,
        plans: Option<&Plans>,
    ) -> Vec<(Vec<ShapedGlyph>, Vec<Warning>)> {
        let shaper = Shaper::new(face).with_limits(limits);
        let shaper = match plans {
            Some(plans) => shaper.with_plans(plans),
            None => shaper,
        };
        output(&shaper.shape_text(text, BaseDirection::Auto).1)
    }

    /// Past its ceiling a `Plans` keeps less or nothing, and the text shapes
    /// as it does with none, whatever the ceiling.
    #[test]
    fn a_plans_past_its_ceiling_shapes_as_one_never_kept() {
        let face = Sfnt::parse(ARABIC_FACE).expect("the face parses");
        for ceiling in [0usize, 1, 100, 300, 1_000, 4_000, 20_000, usize::MAX] {
            let limits = Limits {
                max_index_bytes: ceiling,
                ..Limits::DEFAULT
            };
            let plans = Plans::new();
            let plain = shaped(&face, URDU, limits, None);
            assert_eq!(
                shaped(&face, URDU, limits, Some(&plans)),
                plain,
                "{ceiling}"
            );
            assert_eq!(
                shaped(&face, URDU, limits, Some(&plans)),
                plain,
                "{ceiling}"
            );
            if ceiling < 64 {
                assert_eq!(
                    plans.plans_built(),
                    0,
                    "a plan no ceiling of {ceiling} pays for"
                );
                assert_eq!(plans.indexes_built(), 0);
            }
        }
        let ample = Plans::new();
        let _ = shaped(&face, URDU, Limits::DEFAULT, Some(&ample));
        assert!(ample.plans_built() > 0 && ample.indexes_built() > 0);
        assert!(ample.indexes().iter().all(|(_, has)| *has));
    }

    /// A `Plans` handed a shaper over another face starts again, so each
    /// face shapes as it would with none, and back again.
    #[test]
    fn a_plans_handed_another_face_starts_again() {
        let arabic = Sfnt::parse(ARABIC_FACE).expect("the Arabic face parses");
        let kannada = Sfnt::parse(KANNADA_FACE).expect("the Kannada face parses");
        let limits = Limits::DEFAULT;
        let plans = Plans::new();
        let urdu = shaped(&arabic, URDU, limits, None);
        let indic = shaped(&kannada, KANNADA, limits, None);

        assert_eq!(shaped(&arabic, URDU, limits, Some(&plans)), urdu);
        let first = plans.plans_built();
        assert_eq!(shaped(&kannada, KANNADA, limits, Some(&plans)), indic);
        let second = plans.plans_built();
        assert!(second > first, "the Kannada face built plans of its own");
        assert_eq!(shaped(&arabic, URDU, limits, Some(&plans)), urdu);
        assert!(plans.plans_built() > second, "and the Arabic face's again");

        // The same bytes parsed twice are the same face.
        let again = Sfnt::parse(ARABIC_FACE).expect("the Arabic face parses");
        let built = plans.plans_built();
        assert_eq!(shaped(&again, URDU, limits, Some(&plans)), urdu);
        assert_eq!(
            plans.plans_built(),
            built,
            "a second parse of one face rebuilt"
        );
    }

    #[test]
    fn a_clone_of_a_plans_starts_empty() {
        let face = Sfnt::parse(ARABIC_FACE).expect("the face parses");
        let plans = Plans::new();
        let plain = shaped(&face, URDU, Limits::DEFAULT, None);
        let _ = shaped(&face, URDU, Limits::DEFAULT, Some(&plans));
        let copy = plans.clone();
        assert_eq!((copy.plans_built(), copy.indexes_built()), (0, 0));
        assert_eq!(shaped(&face, URDU, Limits::DEFAULT, Some(&copy)), plain);
        assert!(plans.plans_built() > 0, "the original keeps its own");
    }

    /// One `Plans` serves shapers on several threads at once, each shaping
    /// as it would with none.
    #[test]
    fn one_plans_serves_shapers_on_several_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<Plans>();
        shared::<Shaper<'static>>();

        let face = Sfnt::parse(KANNADA_FACE).expect("the face parses");
        let plain = shaped(&face, KANNADA, Limits::DEFAULT, None);
        let plans = Plans::new();
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| shaped(&face, KANNADA, Limits::DEFAULT, Some(&plans))))
                .collect();
            for handle in handles {
                assert_eq!(handle.join().expect("a shaping thread"), plain);
            }
        });
        let built = plans.plans_built();
        let _ = shaped(&face, KANNADA, Limits::DEFAULT, Some(&plans));
        assert_eq!(plans.plans_built(), built, "every plan was kept once");
    }

    /// Two stage indices point into [`USE_GSUB_STAGES`], and this is what
    /// stops either drifting off it.
    ///
    /// Both are plain integers because the array is a `const` of slices and
    /// searching it at runtime for a tag would be work done per run for an
    /// answer that never changes. The cost of that is exactly this test.
    #[test]
    fn the_stage_indices_name_the_stages_they_mean() {
        assert_eq!(
            super::USE_GSUB_STAGES.get(super::USE_RPHF_STAGE),
            Some(&[Tag::new(b"rphf")].as_slice()),
            "USE_RPHF_STAGE no longer indexes the rphf stage"
        );
        assert!(
            super::USE_GSUB_STAGES
                .get(super::USE_REORDER_AFTER)
                .is_some_and(|stage| stage.contains(&Tag::new(b"blwf"))),
            "USE_REORDER_AFTER no longer indexes the orthographic-unit group"
        );
        const { assert!(super::USE_RPHF_STAGE < super::USE_REORDER_AFTER) };
    }

    /// Where `rphf` is offered a position, over the four shapes that matter.
    ///
    /// The pair that adjudicates it is text-rendering-tests `SHKNDA-2/7`
    /// against `SHKNDA-2/12`: both end a syllable with U+0CB0 U+0CCD, and only
    /// one of them is a reph, because only one of them has a base after it.
    /// Offering the lookup at both cost the first case and got the second
    /// right for the wrong reason.
    #[test]
    fn rphf_is_offered_only_where_the_syllable_has_a_base_for_it() {
        fn offered(text: &str) -> Vec<bool> {
            let letters: Vec<char> = text.chars().collect();
            let categories: Vec<crate::universal::Category> = letters
                .iter()
                .map(|c| crate::universal::category(*c))
                .collect();
            let syllables: Vec<u16> = crate::universal::clusters(&letters, Script::Kannada)
                .into_iter()
                .map(|(number, _)| number)
                .collect();
            super::repha_positions(&syllables, &categories)
        }

        // KANNADA RA, VIRAMA, KA: a repha, and both of its two characters are
        // marked, because the mask is checked against every glyph of the
        // ligature's input.
        assert_eq!(offered("\u{0CB0}\u{0CCD}\u{0C95}"), [true, true, false]);
        // The same two characters with nothing after them: a dead consonant,
        // which `haln` draws. `SHKNDA-2/7` ends this way.
        assert_eq!(offered("\u{0CB0}\u{0CCD}"), [false, false]);
        // `SHKNDA-2/12`'s second syllable — RA VIRAMA CHA VIRAMA — is a repha
        // in front of a base that is itself dead.
        assert_eq!(
            offered("\u{0CB0}\u{0CCD}\u{0C9A}\u{0CCD}"),
            [true, true, false, false]
        );
        // A base with no halant after it offers nothing, and neither does a
        // run with no Brahmic character in it at all.
        assert_eq!(offered("\u{0CB0}\u{0C95}"), [false, false]);
        assert_eq!(offered("ab"), [false, false]);
    }

    /// The two joiners, and the width of the predicate stated as a limit.
    ///
    /// The behavioural half of this is in `tests/text_rendering.rs` and is a
    /// pair rather than a single case, because a joiner has to do two opposite
    /// things: `SHKNDA-3/31` says it must not reach the output, and
    /// `SHLANA-5/10` and `SHLANA-5/12` say it must still block a ligature on
    /// the way. Deleting it at `cmap` time satisfies the first and breaks the
    /// second, and that injection costs exactly those two cases.
    ///
    /// What is here instead is the **narrowness**. Every character below is
    /// `Default_Ignorable_Code_Point` and none of them is deleted, which is
    /// the whole difference between this predicate and the one a shaper is
    /// usually written against; [`is_joiner`] says why the wider property is
    /// not vendored.
    #[test]
    fn the_only_ignorable_characters_are_the_two_joiners() {
        assert!(is_joiner('\u{200C}'), "ZWNJ");
        assert!(is_joiner('\u{200D}'), "ZWJ");
        for c in [
            '\u{00AD}', // SOFT HYPHEN
            '\u{200B}', // ZERO WIDTH SPACE
            '\u{2060}', // WORD JOINER
            '\u{180B}', // MONGOLIAN FREE VARIATION SELECTOR ONE
            '\u{FE00}', // VARIATION SELECTOR-1, consumed in `map` instead
            '\u{0CBE}', // KANNADA VOWEL SIGN AA, which draws
            'a',
        ] {
            assert!(!is_joiner(c), "{c:?} is not one of the two");
        }
    }

    /// Every plan's mark-width answer, tabled.
    ///
    /// A table test and not a behavioural one, and the reason is a finding
    /// rather than a shortcut. **No face in either conformance corpus can tell
    /// the two answers apart for a non-Brahmic run.** `TestShapeAran` — the
    /// only Arabic face here — and the three `TestGPOS` faces have *zero*
    /// `GDEF` marks with a non-zero `hmtx` advance between them, so
    /// `ZeroByGdef` and `AsSupplied` produce identical output for every one of
    /// their cases. Setting `AsSupplied` for all four plans was injected and
    /// **failed nothing at all**.
    ///
    /// So the joining half of this condition rests on the argument in
    /// [`Plan::mark_widths`] and on nothing that runs. Real Arabic faces do
    /// give marks non-zero advances — it is why [`MarkWidths::ZeroByGdef`] is
    /// the default and what its own doc comment describes — but this
    /// repository has no such face, and saying so is worth more than a guard
    /// that looks measured and is not.
    ///
    /// What this test does buy: a future edit that flips a plan's answer fails
    /// here and has to argue with the sentence above.
    #[test]
    fn each_plan_answers_the_mark_width_question_for_its_own_reason() {
        assert_eq!(Plan::Universal.mark_widths(), MarkWidths::AsSupplied);
        for plan in [Plan::Default, Plan::Joining, Plan::AsAsked] {
            assert_eq!(plan.mark_widths(), MarkWidths::ZeroByGdef, "{plan:?}");
        }
    }

    fn runs(text: &str) -> Vec<(std::ops::Range<usize>, Script, u8)> {
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        itemize(text, &paragraph)
            .into_iter()
            .map(|run| (run.text, run.script, run.level.number()))
            .collect()
    }

    #[test]
    fn one_script_is_one_run() {
        assert_eq!(runs("hello"), vec![(0..5, Script::Latin, 0)]);
    }

    #[test]
    fn a_space_joins_the_word_before_it_rather_than_starting_a_run() {
        assert_eq!(runs("a a"), vec![(0..3, Script::Latin, 0)]);
    }

    #[test]
    fn a_leading_common_character_joins_what_follows() {
        // The full stop has no script of its own and there is nothing before
        // it, so it takes Ethiopic from the character after it.
        assert_eq!(runs(".\u{1208}"), vec![(0..4, Script::Ethiopic, 0)]);
    }

    #[test]
    fn two_scripts_are_two_runs() {
        let out = runs("a\u{1208}");
        assert_eq!(
            out,
            vec![(0..1, Script::Latin, 0), (1..4, Script::Ethiopic, 0)]
        );
    }

    /// A change of embedding level cuts a run even where the script does not,
    /// because a run is what one call to `GPOS` sees and two directions in one
    /// call would kern across the seam.
    #[test]
    fn a_change_of_level_cuts_a_run() {
        let out = runs("a\u{05D0}");
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0].2, 0);
        assert_eq!(out[1].2, 1);
    }

    #[test]
    fn text_with_nothing_in_it_has_no_runs() {
        assert!(runs("").is_empty());
        let paragraph = Paragraph::new("", BaseDirection::Auto);
        assert!(super::itemize_vertical("", &paragraph).is_empty());
    }

    /// A line's runs are all horizontal, whatever their characters would
    /// stand as in a column, and are cut exactly where they were before runs
    /// had an orientation: by script and level alone.
    #[test]
    fn a_line_is_itemized_horizontally() {
        let text = "\u{6F22}\u{5B57}abc\u{20DD}\u{1820}\u{1823}";
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        let runs = itemize(text, &paragraph);
        assert!(runs
            .iter()
            .all(|run| run.orientation == super::Orientation::Horizontal));
        let cuts: Vec<(std::ops::Range<usize>, Script)> =
            runs.into_iter().map(|run| (run.text, run.script)).collect();
        assert_eq!(
            cuts,
            vec![
                (0..6, Script::Han),
                (6..12, Script::Latin),
                (12..18, Script::Mongolian),
            ]
        );
    }

    #[test]
    fn the_selectors_are_the_two_blocks_and_not_the_mongolian_ones() {
        assert!(is_variation_selector('\u{FE00}'));
        assert!(is_variation_selector('\u{FE0F}'));
        assert!(is_variation_selector('\u{E0100}'));
        assert!(is_variation_selector('\u{E01EF}'));
        assert!(!is_variation_selector('\u{180B}'));
        assert!(!is_variation_selector('a'));
    }

    /// A table directory of `tables` and nothing else, which is all
    /// `Sfnt::parse` and `Layout::parse` read.
    fn directory(tables: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let count = u16::try_from(tables.len()).expect("a small directory");
        let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
        out.extend(count.to_be_bytes());
        out.extend([0u8; 6]);
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, bytes) in tables {
            out.extend_from_slice(*tag);
            out.extend(0u32.to_be_bytes());
            out.extend(u32::try_from(offset).expect("small").to_be_bytes());
            out.extend(u32::try_from(bytes.len()).expect("small").to_be_bytes());
            offset += bytes.len();
            body.extend_from_slice(bytes);
        }
        out.extend(body);
        out
    }

    /// Only a face whose layout is a Graphite description alone goes without
    /// it: a readable `GSUB` or `GPOS`, or a `morx`, beside the `Silf` takes
    /// the path it always took, and a face with no `Silf` has nothing to go
    /// without.
    #[test]
    fn only_a_face_whose_only_layout_is_graphite_goes_without_it() {
        let silf: &[u8] = &[0, 4, 0, 0, 0, 4, 0, 2, 0, 0, 0, 0];
        // Version 1.0 and three absent offsets: readable, and empty.
        let layout: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        // A major version this crate does not read.
        let unreadable: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        let gdef: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let silf_tag = Some(Tag::new(b"Silf"));
        let cases: Vec<(Vec<(&[u8; 4], &[u8])>, Option<Tag>)> = vec![
            (vec![(b"Silf", silf)], silf_tag),
            (vec![(b"GDEF", gdef), (b"Silf", silf)], silf_tag),
            (vec![(b"GSUB", unreadable), (b"Silf", silf)], silf_tag),
            (vec![(b"GSUB", layout), (b"Silf", silf)], None),
            (vec![(b"GPOS", layout), (b"Silf", silf)], None),
            (vec![(b"Silf", silf), (b"morx", &[0u8, 2, 0, 0][..])], None),
            (
                vec![
                    (b"Glat", &[0u8, 1, 0, 0][..]),
                    (b"Gloc", &[0u8, 1, 0, 0][..]),
                ],
                None,
            ),
            (vec![], None),
        ];
        for (tables, expected) in cases {
            let bytes = directory(&tables);
            let face = Sfnt::parse(&bytes).expect("a directory parses");
            let layout = crate::Layout::parse(&face);
            let names: Vec<&[u8; 4]> = tables.iter().map(|(tag, _)| *tag).collect();
            assert_eq!(super::table_not_run(&face, &layout), expected, "{names:?}");
        }
    }

    /// A face is shaped through its AAT tables where it has no OpenType
    /// Layout table to take instead and its `morx` or its `kerx` can be read,
    /// each read where it can be; a readable `GSUB` or `GPOS` beside them
    /// takes the path it always took. The `feat` beside them is read with
    /// them.
    #[test]
    fn only_a_face_whose_layout_is_aat_alone_takes_its_aat_tables() {
        let morx: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0];
        let three: &[u8] = &[0, 3, 0, 0, 0, 0, 0, 0];
        let old: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
        let mort: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
        let silf: &[u8] = &[0, 4, 0, 0, 0, 4, 0, 2, 0, 0, 0, 0];
        let layout: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        let unreadable: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        let gdef: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let feat: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let kerx: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0];
        let kerx_four: &[u8] = &[0, 4, 0, 0, 0, 0, 0, 0];
        let kerx_old: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
        // (tables, a morx read, a kerx read, a feat read); a face reading
        // neither takes no AAT tables.
        let cases: Vec<(Vec<(&[u8; 4], &[u8])>, bool, bool, bool)> = vec![
            (vec![(b"morx", morx)], true, false, false),
            (vec![(b"morx", three)], true, false, false),
            (vec![(b"feat", feat), (b"morx", morx)], true, false, true),
            (vec![(b"GDEF", gdef), (b"morx", morx)], true, false, false),
            (
                vec![(b"GSUB", unreadable), (b"morx", morx)],
                true,
                false,
                false,
            ),
            (
                vec![(b"GPOS", unreadable), (b"morx", morx)],
                true,
                false,
                false,
            ),
            (vec![(b"morx", morx), (b"mort", mort)], true, false, false),
            (vec![(b"Silf", silf), (b"morx", morx)], true, false, false),
            (vec![(b"kerx", kerx), (b"morx", morx)], true, true, false),
            (vec![(b"kerx", kerx)], false, true, false),
            (vec![(b"kerx", kerx_four)], false, true, false),
            (vec![(b"feat", feat), (b"kerx", kerx)], false, true, true),
            (
                vec![(b"kerx", kerx_old), (b"morx", morx)],
                true,
                false,
                false,
            ),
            (vec![(b"kerx", kerx), (b"morx", old)], false, true, false),
            (vec![(b"morx", old)], false, false, false),
            (vec![(b"morx", &morx[..3])], false, false, false),
            (vec![(b"kerx", kerx_old)], false, false, false),
            (vec![(b"mort", mort)], false, false, false),
            (
                vec![(b"GSUB", layout), (b"morx", morx)],
                false,
                false,
                false,
            ),
            (
                vec![(b"GPOS", layout), (b"morx", morx)],
                false,
                false,
                false,
            ),
            (
                vec![(b"GSUB", layout), (b"kerx", kerx)],
                false,
                false,
                false,
            ),
            (vec![(b"feat", feat)], false, false, false),
            (vec![], false, false, false),
        ];
        for (tables, has_morx, has_kerx, has_feat) in cases {
            let bytes = directory(&tables);
            let face = Sfnt::parse(&bytes).expect("a directory parses");
            let layout = crate::Layout::parse(&face);
            let names: Vec<&[u8; 4]> = tables.iter().map(|(tag, _)| *tag).collect();
            let aat = super::aat_tables(&face, &layout);
            assert_eq!(aat.is_some(), has_morx || has_kerx, "{names:?}");
            assert_eq!(
                aat.is_some_and(|aat| aat.morx.is_some()),
                has_morx,
                "{names:?}"
            );
            assert_eq!(
                aat.is_some_and(|aat| aat.kerx.is_some()),
                has_kerx,
                "{names:?}"
            );
            assert_eq!(
                aat.is_some_and(|aat| aat.feat.is_some()),
                has_feat,
                "{names:?}"
            );
        }
    }

    /// A face goes without an AAT table, named, only where it has no
    /// OpenType Layout table to take instead and the table is one this crate
    /// does not run: for substitution, a `morx` it cannot read, or a `mort`
    /// where it carries no `morx` and no Graphite description; for kerning,
    /// a `kerx` it cannot read. A readable `morx` or `kerx` is run and named
    /// for nothing, a `mort` beside a `morx` ignored as the manual says; a
    /// readable `GSUB` or `GPOS` takes the path it always took and names
    /// none of them.
    #[test]
    fn only_an_aat_table_this_crate_does_not_run_goes_without_it() {
        let morx: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0];
        let old: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
        let mort: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
        let silf: &[u8] = &[0, 4, 0, 0, 0, 4, 0, 2, 0, 0, 0, 0];
        let layout: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        let unreadable: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        let gdef: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let kerx: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0];
        let kerx_old: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
        let kerx_five: &[u8] = &[0, 5, 0, 0, 0, 0, 0, 0];
        let named_morx = Some(Tag::new(b"morx"));
        let named_mort = Some(Tag::new(b"mort"));
        let named_kerx = Some(Tag::new(b"kerx"));
        let cases: Vec<(Vec<(&[u8; 4], &[u8])>, [Option<Tag>; 2])> = vec![
            (vec![(b"morx", old)], [named_morx, None]),
            (vec![(b"morx", &morx[..5])], [named_morx, None]),
            (vec![(b"GDEF", gdef), (b"morx", old)], [named_morx, None]),
            (
                vec![(b"GSUB", unreadable), (b"morx", old)],
                [named_morx, None],
            ),
            (vec![(b"morx", old), (b"mort", mort)], [named_morx, None]),
            (vec![(b"Silf", silf), (b"morx", old)], [named_morx, None]),
            (vec![(b"mort", mort)], [named_mort, None]),
            (vec![(b"GDEF", gdef), (b"mort", mort)], [named_mort, None]),
            (vec![(b"kerx", kerx_old)], [None, named_kerx]),
            (vec![(b"kerx", kerx_five)], [None, named_kerx]),
            (vec![(b"kerx", &kerx[..5])], [None, named_kerx]),
            (
                vec![(b"kerx", kerx_old), (b"morx", morx)],
                [None, named_kerx],
            ),
            (
                vec![(b"kerx", kerx_old), (b"morx", old)],
                [named_morx, named_kerx],
            ),
            (
                vec![(b"kerx", kerx_old), (b"mort", mort)],
                [named_mort, named_kerx],
            ),
            (vec![(b"morx", morx)], [None, None]),
            (vec![(b"morx", morx), (b"mort", mort)], [None, None]),
            (vec![(b"GSUB", unreadable), (b"morx", morx)], [None, None]),
            (vec![(b"kerx", kerx), (b"morx", morx)], [None, None]),
            (vec![(b"GSUB", layout), (b"morx", old)], [None, None]),
            (vec![(b"GPOS", layout), (b"morx", morx)], [None, None]),
            (vec![(b"GSUB", layout), (b"mort", mort)], [None, None]),
            (vec![(b"GPOS", layout), (b"mort", mort)], [None, None]),
            (vec![(b"GSUB", layout), (b"kerx", kerx_old)], [None, None]),
            (vec![(b"Silf", silf), (b"mort", mort)], [None, None]),
            (vec![(b"kerx", kerx)], [None, None]),
            (vec![(b"Silf", silf)], [None, None]),
            (vec![], [None, None]),
        ];
        for (tables, expected) in cases {
            let bytes = directory(&tables);
            let face = Sfnt::parse(&bytes).expect("a directory parses");
            let layout = crate::Layout::parse(&face);
            let names: Vec<&[u8; 4]> = tables.iter().map(|(tag, _)| *tag).collect();
            assert_eq!(super::aat_not_run(&face, &layout), expected, "{names:?}");
            // A face named for an AAT table is not shaped through it.
            let aat = super::aat_tables(&face, &layout);
            if expected[0].is_some() {
                assert!(aat.is_none_or(|aat| aat.morx.is_none()), "{names:?}");
            }
            if expected[1].is_some() {
                assert!(aat.is_none_or(|aat| aat.kerx.is_none()), "{names:?}");
            }
        }
    }

    /// The refusals never name one face twice for its substitution: a face
    /// is a Graphite description, or AAT's, or neither, so a run carries at
    /// most one [`Warning::TableNotRun`] for it, and a face with no `kerx`
    /// is named for none.
    #[test]
    fn a_face_is_named_for_one_table_at_most() {
        let silf: &[u8] = &[0, 4, 0, 0, 0, 4, 0, 2, 0, 0, 0, 0];
        let morx: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 0];
        let mort: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
        let gdef: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let tables: [(&[u8; 4], &[u8]); 4] = [
            (b"Silf", silf),
            (b"morx", morx),
            (b"mort", mort),
            (b"GDEF", gdef),
        ];
        // Every subset of the four tables, by bitmask.
        for mask in 0u8..16 {
            let chosen: Vec<(&[u8; 4], &[u8])> = tables
                .iter()
                .enumerate()
                .filter(|(at, _)| mask & (1 << at) != 0)
                .map(|(_, table)| *table)
                .collect();
            let bytes = directory(&chosen);
            let face = Sfnt::parse(&bytes).expect("a directory parses");
            let layout = crate::Layout::parse(&face);
            let [substitution, kerning] = super::aat_not_run(&face, &layout);
            let named = usize::from(super::table_not_run(&face, &layout).is_some())
                + usize::from(substitution.is_some());
            assert!(named <= 1, "mask {mask:04b} named {named} tables");
            assert!(kerning.is_none(), "mask {mask:04b}");
        }
    }
}
