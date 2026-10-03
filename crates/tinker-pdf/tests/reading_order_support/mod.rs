//! The scores `docs/design/reading-order.md` and
//! `docs/design/table-reconstruction.md` define, computed once for every suite
//! that reports them: the first-party fixtures in `reading_order.rs` and
//! `tables.rs`, and the corpus censuses `reading_order_census.rs` and
//! `table_census.rs`.
//!
//! # What adjudicates
//!
//! The answer key is always a **structure tree**: what a producer wrote into
//! the file about how it reads. For the corpus that producer is a third party;
//! for the EPUB books it is the book's author, through this engine's writer,
//! whose tagged order `epub_structure.rs` holds to the XHTML source; for the
//! builder fixtures it is the fixture, and those are arithmetic fixtures with
//! a known answer rather than evidence about the world. The inference is
//! always run with the tree hidden ([`InferenceOptions::hide_structure`]).
//!
//! # Characters are matched by where they are
//!
//! The tree's view and the inference's are built from two extractions of one
//! page — the inference's keeps `/Artifact` text, the tree's does not — so a
//! character is identified by what both agree on: its origin, to the bit, and
//! its text. Two glyphs drawn at one origin with one text (a fake bold) are
//! told apart by occurrence: the k-th such character of one sequence is the
//! k-th of the other.

#![allow(
    dead_code,
    reason = "shared by several test binaries; each uses a different subset"
)]

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf::{Document, InferenceOptions, InferredOrder, Role, TextChar};

/// What identifies a character across two extractions of one page.
pub type Key = (u64, u64, String);

/// A character's key.
pub fn key(c: &TextChar) -> Key {
    (c.origin.0.to_bits(), c.origin.1.to_bits(), c.text.clone())
}

/// `keys` with each repeated key numbered by occurrence.
fn numbered(keys: &[Key]) -> Vec<(Key, usize)> {
    let mut seen: BTreeMap<&Key, usize> = BTreeMap::new();
    keys.iter()
        .map(|k| {
            let n = seen.entry(k).or_insert(0);
            let out = (k.clone(), *n);
            *n += 1;
            out
        })
        .collect()
}

/// Pair agreement: of the pairs of characters both orders hold, how many
/// they put the same way round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Agreement {
    /// Pairs of characters both orders hold.
    pub pairs: u64,
    /// Of those, the pairs both put the same way round.
    pub agreeing: u64,
}

impl Agreement {
    /// `agreeing / pairs`, or 1 when there are no pairs.
    pub fn score(&self) -> f64 {
        if self.pairs == 0 {
            1.0
        } else {
            self.agreeing as f64 / self.pairs as f64
        }
    }

    /// Whether `self` scores at least `floor_num / floor_den`, by integer
    /// cross-multiplication — the comparison `corpus/ratchet.json`'s bars use.
    pub fn at_least(&self, floor_num: u64, floor_den: u64) -> bool {
        u128::from(self.agreeing) * u128::from(floor_den)
            >= u128::from(floor_num) * u128::from(self.pairs)
    }

    /// Two agreements summed, as a corpus total.
    pub fn plus(self, other: Agreement) -> Agreement {
        Agreement {
            pairs: self.pairs + other.pairs,
            agreeing: self.agreeing + other.agreeing,
        }
    }
}

/// Scores `candidate` against `answer`.
///
/// Every pair of characters that both sequences hold is compared, which is
/// `m(m-1)/2` pairs for `m` shared characters; the disagreeing ones are the
/// inversions of `candidate` read in `answer`'s positions, counted by a merge
/// sort so a page of ten thousand characters costs a fraction of a second.
pub fn pair_agreement(answer: &[Key], candidate: &[Key]) -> Agreement {
    let position: BTreeMap<(Key, usize), usize> = numbered(answer)
        .into_iter()
        .enumerate()
        .map(|(at, k)| (k, at))
        .collect();
    let mut sequence: Vec<usize> = numbered(candidate)
        .into_iter()
        .filter_map(|k| position.get(&k).copied())
        .collect();
    let m = sequence.len() as u64;
    let pairs = m * m.saturating_sub(1) / 2;
    let inversions = inversions(&mut sequence);
    Agreement {
        pairs,
        agreeing: pairs - inversions,
    }
}

fn inversions(values: &mut [usize]) -> u64 {
    let n = values.len();
    if n < 2 {
        return 0;
    }
    let mid = n / 2;
    let mut count = {
        let (left, right) = values.split_at_mut(mid);
        inversions(left) + inversions(right)
    };
    let mut merged = Vec::with_capacity(n);
    let (mut i, mut j) = (0usize, mid);
    while i < mid && j < n {
        if values[i] <= values[j] {
            merged.push(values[i]);
            i += 1;
        } else {
            merged.push(values[j]);
            count += (mid - i) as u64;
            j += 1;
        }
    }
    merged.extend_from_slice(&values[i..mid]);
    merged.extend_from_slice(&values[j..n]);
    values.copy_from_slice(&merged);
    count
}

/// Column crossings: walking `answer`, the sequence of columns `order`
/// assigned, counting every return to a column already left.
///
/// The one score that measures the column inference and nothing else: a
/// producer that tagged column one then column two has said where its
/// columns are without saying the word. A column is a band and an index in
/// it ([`tinker_pdf::InferredBlock::section`]), so the left column under a
/// heading across the page is not the left column above it. Characters the
/// inference placed in no column — a spanner, a running head — are skipped.
pub fn column_crossings(answer: &[Key], order: &InferredOrder) -> usize {
    let mut column_of: BTreeMap<(Key, usize), (usize, usize)> = BTreeMap::new();
    let mut keys = Vec::new();
    let mut columns = Vec::new();
    for block in &order.blocks {
        for line in &block.lines {
            for c in &line.chars {
                keys.push(key(c));
                columns.push(block.column.map(|column| (block.section, column)));
            }
        }
    }
    for (k, column) in numbered(&keys).into_iter().zip(columns) {
        if let Some(column) = column {
            column_of.insert(k, column);
        }
    }
    let mut left: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut current: Option<(usize, usize)> = None;
    let mut crossings = 0usize;
    for k in numbered(answer) {
        let Some(column) = column_of.get(&k).copied() else {
            continue;
        };
        if let Some(was) = current {
            if was != column {
                left.insert(was);
                if left.contains(&column) {
                    crossings += 1;
                }
            }
        }
        current = Some(column);
    }
    crossings
}

/// One page of a tagged document, three ways: the tree's order (the answer
/// key), the stream's, and the inference's with the tree hidden.
pub struct Scored {
    /// The tree's order, from `text_for_page` over `Page::text`.
    pub answer: Vec<Key>,
    /// `Page::text`'s order.
    pub stream: Vec<Key>,
    /// The inference, tree hidden.
    pub inferred: InferredOrder,
}

impl Scored {
    /// Reads page `index` of `doc`, or `None` when the document carries no
    /// structure tree or has no such page.
    pub fn read(doc: &Document, index: u32) -> Option<Scored> {
        let tree = doc.structure()?;
        let page = doc.page(index)?;
        let text = page.text();
        let structured = tree.text_for_page(index, &text);
        let answer: Vec<Key> = structured
            .nodes
            .iter()
            .flat_map(|n| n.chars.iter())
            .map(key)
            .collect();
        let stream: Vec<Key> = text
            .lines()
            .iter()
            .flat_map(|l| l.chars.iter())
            .map(key)
            .collect();
        let inferred = doc.inferred_order(
            index,
            &InferenceOptions {
                hide_structure: true,
            },
        )?;
        Some(Scored {
            answer,
            stream,
            inferred,
        })
    }

    /// The stream's pair agreement with the tree: the baseline.
    pub fn stream_agreement(&self) -> Agreement {
        pair_agreement(&self.answer, &self.stream)
    }

    /// The inference's pair agreement with the tree.
    pub fn inferred_agreement(&self) -> Agreement {
        let keys: Vec<Key> = self.inferred.chars().into_iter().map(key).collect();
        pair_agreement(&self.answer, &keys)
    }

    /// The inference's column crossings, walking the tree.
    pub fn crossings(&self) -> usize {
        column_crossings(&self.answer, &self.inferred)
    }
}

/// The characters of page `index` that only a reading with the tree hidden
/// extracts: what the producer drew inside `/Artifact` scopes (14.8.2.2), which
/// `Page::text` drops. Over a page whose producer marks its furniture
/// `/Pagination`, the truth the running-head score is held to.
pub fn artifact_keys(doc: &Document, index: u32) -> BTreeSet<Key> {
    let Some(page) = doc.page(index) else {
        return BTreeSet::new();
    };
    let shown: BTreeSet<Key> = page
        .text()
        .lines()
        .iter()
        .flat_map(|l| l.chars.iter())
        .map(key)
        .collect();
    let Some(hidden) = doc.inferred_order(
        index,
        &InferenceOptions {
            hide_structure: true,
        },
    ) else {
        return BTreeSet::new();
    };
    hidden
        .chars()
        .into_iter()
        .map(key)
        .filter(|k| !shown.contains(k))
        .collect()
}

/// The artifact characters of page `index` ([`artifact_keys`]) that stand in
/// its margin bands ([`tinker_pdf::reading_order::MARGIN_BAND`]): the truth a
/// running-head score is held to over a corpus.
///
/// By position because the device seam carries a property list's `/MCID` and
/// 14.9's entries and not an artifact's `/Type` or `/Subtype`, so a
/// `/Pagination` artifact cannot be told from a `/Layout` one by name; one in
/// the margin band of the page is what a running head, foot or page number
/// is, and one elsewhere — a watermark, a rule — is left out of the truth.
pub fn furniture_truth(doc: &Document, index: u32) -> BTreeSet<Key> {
    let Some(page) = doc.page(index) else {
        return BTreeSet::new();
    };
    let (_, y0, _, y1) = page.crop_box();
    let band = (y1 - y0).abs() * tinker_pdf::reading_order::MARGIN_BAND;
    let (low, high) = (y0.min(y1) + band, y0.max(y1) - band);
    artifact_keys(doc, index)
        .into_iter()
        .filter(|k| {
            let y = f64::from_bits(k.1);
            y <= low || y >= high
        })
        .collect()
}

/// The characters page `index`'s structure tree claims for `Note` elements
/// (14.8.4.6, after the role map): the truth a footnote score is held to.
pub fn note_keys(doc: &Document, index: u32) -> BTreeSet<Key> {
    let (Some(tree), Some(page)) = (doc.structure(), doc.page(index)) else {
        return BTreeSet::new();
    };
    tree.text_for_page(index, &page.text())
        .nodes
        .iter()
        .filter(|n| n.standard_type == "Note")
        .flat_map(|n| n.chars.iter())
        .map(key)
        .collect()
}

/// How the blocks an inference gave some roles compare with a set of
/// characters known to have them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoleScore {
    /// Blocks the inference gave one of the roles.
    pub called: usize,
    /// Of those, the blocks most of whose characters are in the truth.
    pub correct: usize,
    /// Characters in the truth.
    pub truth: usize,
    /// Of those, the characters the inference gave one of the roles.
    pub found: usize,
}

impl RoleScore {
    /// Every called block was right, compared by cross-multiplication
    /// against `num / den`.
    pub fn precision_at_least(&self, num: usize, den: usize) -> bool {
        self.correct * den >= num * self.called
    }

    /// The share of the truth that was found, against `num / den`.
    pub fn recall_at_least(&self, num: usize, den: usize) -> bool {
        self.found * den >= num * self.truth
    }

    /// Two scores summed.
    pub fn plus(self, other: RoleScore) -> RoleScore {
        RoleScore {
            called: self.called + other.called,
            correct: self.correct + other.correct,
            truth: self.truth + other.truth,
            found: self.found + other.found,
        }
    }
}

/// Scores the blocks of `order` given any of `roles` against `truth`.
pub fn role_score(order: &InferredOrder, roles: &[Role], truth: &BTreeSet<Key>) -> RoleScore {
    let mut score = RoleScore {
        truth: truth.len(),
        ..RoleScore::default()
    };
    for block in &order.blocks {
        if !roles.contains(&block.role) {
            continue;
        }
        score.called += 1;
        let keys: Vec<Key> = block
            .lines
            .iter()
            .flat_map(|l| l.chars.iter())
            .map(key)
            .collect();
        let inside = keys.iter().filter(|k| truth.contains(*k)).count();
        if inside * 2 > keys.len() {
            score.correct += 1;
        }
        score.found += inside;
    }
    score
}
