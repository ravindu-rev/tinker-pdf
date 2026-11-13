//! The pattern-file reader and the trie builder.
//!
//! One reader for both of its callers: `build.rs` compiles the vendored
//! dictionaries with it, and [`super::Dictionary::parse`] reads a pattern
//! file with it at run time, so the tables a build carries and the ones a test
//! builds cannot be read two ways. `build.rs` includes this file **by path**,
//! which is why nothing here names anything outside it but the standard
//! library.
//!
//! # The file
//!
//! A pattern file is the format the published pattern collection writes
//! (`docs/design/hyphenation.md`): `%` begins a comment that runs to the end
//! of its line; `\patterns{…}` holds whitespace-separated patterns, each a
//! string of letters with at most one digit — its level — before, between
//! and after them, and `.` first or last for a word's edge; `\hyphenation{…}`
//! holds exceptions, whole words with a hyphen at each point. The header's
//! comments state the dictionary's minimums in a `hyphenmins` block, whose
//! `typesetting` entry gives `left` and `right`. Anything else outside a
//! comment is refused rather than skipped: a vendored file this reader does
//! not understand fails the build instead of compiling to a table that
//! quietly lost part of it.
//!
//! # The table
//!
//! Four arrays of bytes, multi-byte values little-endian, so every target
//! reads the same table:
//!
//! - `alphabet`: each letter's code point, four bytes, sorted. A letter's
//!   symbol is [`FIRST_LETTER`] plus its index.
//! - `nodes`: [`NODE_BYTES`] a node — the symbol on the edge into it, the
//!   index of its first child (four bytes), and its vector (two bytes, zero
//!   for none, otherwise the vector's index plus one) — in breadth-first order
//!   with each node's children sorted by symbol, and one sentinel record after
//!   the last, so a node's children run from its first child to the next
//!   node's.
//! - `offsets` and `values`: each distinct vector once, its start in `values`
//!   as four bytes, `offsets` ending with the length of `values`. A pattern's
//!   vector holds a level for each gap of its path; an exception's, a level for
//!   each gap of its word, one at each point.
//!
//! A pattern is the path of its symbols from the root. An exception is the
//! path through [`EXCEPTION`] and then its letters, which no pattern's path
//! enters, since no pattern starts with that symbol.

use std::collections::{BTreeMap, BTreeSet};

/// The symbol of the word boundary, `.`.
pub const BOUNDARY: u8 = 0;

/// The symbol an exception's path starts with.
pub const EXCEPTION: u8 = 1;

/// The symbol of a dictionary's first letter.
pub const FIRST_LETTER: u8 = 2;

/// The most letters one dictionary may have: a symbol is one byte, and two
/// are taken.
pub const MAX_LETTERS: usize = 254;

/// The bytes of one node record: its symbol, its first child, its vector.
pub const NODE_BYTES: usize = 7;

/// What a pattern file holds, read and not yet compiled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    /// The patterns, in the order the file writes them.
    pub patterns: Vec<Pattern>,
    /// The exceptions, in the order the file writes them.
    pub exceptions: Vec<Exception>,
    /// The `left` and `right` of the header's `hyphenmins` block's
    /// `typesetting` entry, where the header has one.
    pub minimums: Option<(u8, u8)>,
}

/// One pattern: its characters, and a level before each and after the last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    /// The characters, `.` included where it holds the pattern to an edge.
    pub chars: Vec<char>,
    /// `chars.len() + 1` levels: the one before each character, then the one
    /// after the last. Zero where the file writes no digit.
    pub levels: Vec<u8>,
}

/// One exception: a word and the points the file marks in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exception {
    /// The word's letters.
    pub letters: Vec<char>,
    /// Each point, as the number of letters before it, ascending.
    pub points: Vec<usize>,
}

/// Why a pattern file was not read or not compiled.
///
/// Each carries the line it was refused at where there is one, so a vendored
/// file that fails the build says where.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternError {
    /// The file has no `\patterns{` block.
    NoPatterns,
    /// A block that is never closed, at the line it opened on.
    Unterminated {
        /// The line.
        line: usize,
    },
    /// Text outside a comment and every block, or a brace or a backslash
    /// inside a block.
    Stray {
        /// The line.
        line: usize,
    },
    /// A pattern with two digits in a row: one gap, two levels.
    TwoDigits {
        /// The line.
        line: usize,
    },
    /// A pattern with a `.` that is neither its first character nor its last.
    InnerBoundary {
        /// The line.
        line: usize,
    },
    /// A pattern with no letter in it.
    NoLetter {
        /// The line.
        line: usize,
    },
    /// A pattern whose characters a pattern before it already had.
    RepeatedPattern {
        /// The line.
        line: usize,
    },
    /// An exception a hyphen starts, ends or doubles, or one holding a digit
    /// or a `.`.
    MalformedException {
        /// The line.
        line: usize,
    },
    /// An exception whose word was listed already.
    RepeatedException {
        /// The line.
        line: usize,
    },
    /// A `left` or `right` in the header that is not a number from 0 to 255,
    /// or a `typesetting` entry that gives only one of them.
    Minimums {
        /// The line.
        line: usize,
    },
    /// No minimums: the header states none and the caller gave none.
    NoMinimums,
    /// More letters than [`MAX_LETTERS`].
    TooManyLetters {
        /// How many the patterns and exceptions use.
        letters: usize,
    },
    /// More distinct vectors than a node's two bytes can name.
    TooManyVectors,
    /// More nodes than a node's four bytes can index.
    TooManyNodes,
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternError::NoPatterns => write!(f, "no \\patterns block"),
            PatternError::Unterminated { line } => {
                write!(f, "line {line}: a block that is never closed")
            }
            PatternError::Stray { line } => write!(f, "line {line}: text outside a block"),
            PatternError::TwoDigits { line } => {
                write!(f, "line {line}: a pattern with two digits in a row")
            }
            PatternError::InnerBoundary { line } => {
                write!(f, "line {line}: a pattern with `.` inside it")
            }
            PatternError::NoLetter { line } => write!(f, "line {line}: a pattern with no letter"),
            PatternError::RepeatedPattern { line } => {
                write!(f, "line {line}: a pattern read already")
            }
            PatternError::MalformedException { line } => {
                write!(f, "line {line}: a malformed exception")
            }
            PatternError::RepeatedException { line } => {
                write!(f, "line {line}: an exception listed already")
            }
            PatternError::Minimums { line } => write!(f, "line {line}: unreadable minimums"),
            PatternError::NoMinimums => write!(f, "no minimums"),
            PatternError::TooManyLetters { letters } => {
                write!(f, "{letters} letters, past {MAX_LETTERS}")
            }
            PatternError::TooManyVectors => write!(f, "more vectors than two bytes name"),
            PatternError::TooManyNodes => write!(f, "more nodes than four bytes index"),
        }
    }
}

impl std::error::Error for PatternError {}

/// Which block the reader is inside.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Outside,
    Patterns,
    Exceptions,
}

/// Reads a pattern file.
///
/// # Errors
/// A [`PatternError`] naming the line it was refused at.
pub fn read(text: &str) -> Result<Source, PatternError> {
    let mut source = Source {
        minimums: minimums(text)?,
        ..Source::default()
    };
    let mut block = Block::Outside;
    let mut opened = 0usize;
    let mut patterns_seen = false;
    let mut seen_patterns: BTreeSet<Vec<char>> = BTreeSet::new();
    let mut seen_words: BTreeSet<Vec<char>> = BTreeSet::new();
    let mut entry = String::new();
    for (number, raw) in text.lines().enumerate() {
        let line = number + 1;
        let mut rest = match raw.find('%') {
            Some(at) => raw.get(..at).unwrap_or(""),
            None => raw,
        };
        loop {
            if block == Block::Outside {
                rest = rest.trim_start();
                if rest.is_empty() {
                    break;
                }
                let (next, after) = if let Some(after) = rest.strip_prefix("\\patterns") {
                    (Block::Patterns, after)
                } else if let Some(after) = rest.strip_prefix("\\hyphenation") {
                    (Block::Exceptions, after)
                } else {
                    return Err(PatternError::Stray { line });
                };
                rest = after
                    .trim_start()
                    .strip_prefix('{')
                    .ok_or(PatternError::Stray { line })?;
                patterns_seen |= next == Block::Patterns;
                block = next;
                opened = line;
                continue;
            }
            // Inside a block: entries up to the closing brace or the end of
            // the line, which ends an entry as a space does.
            let mut closed = None;
            for (at, c) in rest.char_indices() {
                if c == '}' {
                    closed = Some(at + c.len_utf8());
                    break;
                }
                if c.is_whitespace() {
                    take(
                        &mut entry,
                        block,
                        line,
                        &mut source,
                        &mut seen_patterns,
                        &mut seen_words,
                    )?;
                } else if c == '{' || c == '\\' {
                    return Err(PatternError::Stray { line });
                } else {
                    entry.push(c);
                }
            }
            take(
                &mut entry,
                block,
                line,
                &mut source,
                &mut seen_patterns,
                &mut seen_words,
            )?;
            match closed {
                Some(after) => {
                    block = Block::Outside;
                    rest = rest.get(after..).unwrap_or("");
                }
                None => break,
            }
        }
    }
    if block != Block::Outside {
        return Err(PatternError::Unterminated { line: opened });
    }
    if !patterns_seen {
        return Err(PatternError::NoPatterns);
    }
    Ok(source)
}

/// Reads the entry gathered so far, if there is one, into the block it is in.
fn take(
    entry: &mut String,
    block: Block,
    line: usize,
    source: &mut Source,
    seen_patterns: &mut BTreeSet<Vec<char>>,
    seen_words: &mut BTreeSet<Vec<char>>,
) -> Result<(), PatternError> {
    if entry.is_empty() {
        return Ok(());
    }
    let text = std::mem::take(entry);
    match block {
        Block::Patterns => {
            let read = pattern(&text, line)?;
            if !seen_patterns.insert(read.chars.clone()) {
                return Err(PatternError::RepeatedPattern { line });
            }
            source.patterns.push(read);
        }
        Block::Exceptions => {
            let read = exception(&text, line)?;
            if !seen_words.insert(read.letters.clone()) {
                return Err(PatternError::RepeatedException { line });
            }
            source.exceptions.push(read);
        }
        Block::Outside => return Err(PatternError::Stray { line }),
    }
    Ok(())
}

/// One pattern: a level is the one ASCII digit written in a gap.
fn pattern(text: &str, line: usize) -> Result<Pattern, PatternError> {
    let mut chars = Vec::new();
    let mut levels = vec![0u8];
    let mut written = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if written {
                return Err(PatternError::TwoDigits { line });
            }
            let level = c
                .to_digit(10)
                .and_then(|digit| u8::try_from(digit).ok())
                .unwrap_or(0);
            if let Some(last) = levels.last_mut() {
                *last = level;
            }
            written = true;
        } else {
            chars.push(c);
            levels.push(0);
            written = false;
        }
    }
    let last = chars.len().saturating_sub(1);
    if chars
        .iter()
        .enumerate()
        .any(|(at, &c)| c == '.' && at != 0 && at != last)
    {
        return Err(PatternError::InnerBoundary { line });
    }
    if chars.iter().all(|&c| c == '.') {
        return Err(PatternError::NoLetter { line });
    }
    Ok(Pattern { chars, levels })
}

/// One exception: a hyphen marks a point after the letters before it.
fn exception(text: &str, line: usize) -> Result<Exception, PatternError> {
    let mut letters = Vec::new();
    let mut points = Vec::new();
    // At the start a hyphen would mark a point before any letter.
    let mut after_hyphen = true;
    for c in text.chars() {
        if c == '-' {
            if after_hyphen {
                return Err(PatternError::MalformedException { line });
            }
            points.push(letters.len());
            after_hyphen = true;
        } else if c.is_ascii_digit() || c == '.' {
            return Err(PatternError::MalformedException { line });
        } else {
            letters.push(c);
            after_hyphen = false;
        }
    }
    if after_hyphen {
        return Err(PatternError::MalformedException { line });
    }
    Ok(Exception { letters, points })
}

/// The `left` and `right` of the header's `hyphenmins` block's `typesetting`
/// entry, found by their comments' indentation.
fn minimums(text: &str) -> Result<Option<(u8, u8)>, PatternError> {
    let comments: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .filter_map(|(number, raw)| {
            raw.find('%')
                .map(|at| (number + 1, raw.get(at + 1..).unwrap_or("")))
        })
        .collect();
    let indent = |text: &str| text.len() - text.trim_start().len();
    let Some(start) = comments
        .iter()
        .position(|(_, comment)| comment.trim() == "hyphenmins:")
    else {
        return Ok(None);
    };
    let base = comments
        .get(start)
        .map_or(0, |&(_, comment)| indent(comment));
    let mut at = start + 1;
    while let Some(&(_, comment)) = comments.get(at) {
        if !comment.trim().is_empty() && indent(comment) <= base {
            break;
        }
        if comment.trim() == "typesetting:" {
            let inner = indent(comment);
            let (mut left, mut right) = (None, None);
            let mut last = at;
            for &(line, entry) in comments.iter().skip(at + 1) {
                if entry.trim().is_empty() || indent(entry) <= inner {
                    break;
                }
                last = line;
                let entry = entry.trim();
                if let Some(value) = entry.strip_prefix("left:") {
                    left = Some(level(value, line)?);
                } else if let Some(value) = entry.strip_prefix("right:") {
                    right = Some(level(value, line)?);
                }
            }
            return match (left, right) {
                (Some(left), Some(right)) => Ok(Some((left, right))),
                _ => Err(PatternError::Minimums { line: last }),
            };
        }
        at += 1;
    }
    Ok(None)
}

/// A minimum's value: the number its entry starts with, a comment after it
/// allowed.
fn level(value: &str, line: usize) -> Result<u8, PatternError> {
    let digits: String = value
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits
        .parse::<u8>()
        .map_err(|_| PatternError::Minimums { line })
}

/// A compiled dictionary's four arrays and the three numbers beside them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    /// Each letter's code point, four bytes little-endian, sorted.
    pub alphabet: Vec<u8>,
    /// [`NODE_BYTES`] a node, breadth-first, and the sentinel.
    pub nodes: Vec<u8>,
    /// Each vector's start in `values`, four bytes, and the length of
    /// `values` after the last.
    pub offsets: Vec<u8>,
    /// Every vector's levels, one byte each.
    pub values: Vec<u8>,
    /// The longest pattern, in symbols, a word's edge counted.
    pub longest: u16,
    /// The fewest letters a point may follow.
    pub left: u8,
    /// The fewest letters a point may precede.
    pub right: u8,
}

/// A trie node while the trie is built: its children by symbol, and the
/// vector its path ends at, if one does.
#[derive(Default)]
struct Draft {
    children: BTreeMap<u8, usize>,
    vector: Option<usize>,
}

/// Every vector once, by its levels.
#[derive(Default)]
struct Vectors {
    index: BTreeMap<Vec<u8>, usize>,
    order: Vec<Vec<u8>>,
}

impl Vectors {
    fn intern(&mut self, levels: Vec<u8>) -> usize {
        if let Some(&at) = self.index.get(&levels) {
            return at;
        }
        let at = self.order.len();
        self.index.insert(levels.clone(), at);
        self.order.push(levels);
        at
    }
}

/// Compiles what [`read`] read into the four arrays, at the given minimums.
///
/// # Errors
/// [`PatternError::TooManyLetters`], [`PatternError::TooManyVectors`] or
/// [`PatternError::TooManyNodes`] where a table would not fit its format.
pub fn compile(source: &Source, (left, right): (u8, u8)) -> Result<Compiled, PatternError> {
    let mut letters: BTreeSet<char> = BTreeSet::new();
    for pattern in &source.patterns {
        letters.extend(pattern.chars.iter().copied().filter(|&c| c != '.'));
    }
    for exception in &source.exceptions {
        letters.extend(exception.letters.iter().copied());
    }
    if letters.len() > MAX_LETTERS {
        return Err(PatternError::TooManyLetters {
            letters: letters.len(),
        });
    }
    let alphabet: Vec<char> = letters.into_iter().collect();
    let symbol = |c: char| -> u8 {
        if c == '.' {
            return BOUNDARY;
        }
        // Every letter is in the alphabet, which is at most `MAX_LETTERS`
        // long, so the index plus two fits a byte; the fallbacks are never
        // taken.
        let index = alphabet.binary_search(&c).unwrap_or(0);
        u8::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(FIRST_LETTER))
            .unwrap_or(u8::MAX)
    };

    let mut drafts: Vec<Draft> = vec![Draft::default()];
    let mut vectors = Vectors::default();
    let mut longest = 0usize;
    for pattern in &source.patterns {
        longest = longest.max(pattern.chars.len());
        let node = insert(&mut drafts, pattern.chars.iter().map(|&c| symbol(c)));
        let vector = vectors.intern(pattern.levels.clone());
        if let Some(draft) = drafts.get_mut(node) {
            draft.vector = Some(vector);
        }
    }
    for exception in &source.exceptions {
        let node = insert(
            &mut drafts,
            std::iter::once(EXCEPTION).chain(exception.letters.iter().map(|&c| symbol(c))),
        );
        let mut levels = vec![0u8; exception.letters.len() + 1];
        for &point in &exception.points {
            if let Some(level) = levels.get_mut(point) {
                *level = 1;
            }
        }
        let vector = vectors.intern(levels);
        if let Some(draft) = drafts.get_mut(node) {
            draft.vector = Some(vector);
        }
    }
    if vectors.order.len() > usize::from(u16::MAX) {
        return Err(PatternError::TooManyVectors);
    }
    if u32::try_from(drafts.len()).is_err() {
        return Err(PatternError::TooManyNodes);
    }

    // Breadth first: a node's children are appended as it is visited, so
    // they are contiguous and sorted, and each node's first child is where
    // the list stood when it was reached.
    let mut nodes = Vec::with_capacity((drafts.len() + 1) * NODE_BYTES);
    let mut order: Vec<(usize, u8)> = vec![(0, 0)];
    let mut visited = 0usize;
    while let Some(&(draft, label)) = order.get(visited) {
        let first = order.len();
        if let Some(draft) = drafts.get(draft) {
            for (&symbol, &child) in &draft.children {
                order.push((child, symbol));
            }
        }
        let vector = drafts
            .get(draft)
            .and_then(|draft| draft.vector)
            .and_then(|vector| u16::try_from(vector + 1).ok())
            .unwrap_or(0);
        record(&mut nodes, label, first, vector);
        visited += 1;
    }
    record(&mut nodes, 0, order.len(), 0);

    let mut offsets = Vec::with_capacity((vectors.order.len() + 1) * 4);
    let mut values = Vec::new();
    for levels in &vectors.order {
        offsets.extend_from_slice(&index_bytes(values.len()));
        values.extend_from_slice(levels);
    }
    offsets.extend_from_slice(&index_bytes(values.len()));

    let mut letters = Vec::with_capacity(alphabet.len() * 4);
    for c in &alphabet {
        letters.extend_from_slice(&u32::from(*c).to_le_bytes());
    }
    Ok(Compiled {
        alphabet: letters,
        nodes,
        offsets,
        values,
        longest: u16::try_from(longest).unwrap_or(u16::MAX),
        left,
        right,
    })
}

/// The node a path ends at, made where it does not exist yet.
fn insert(drafts: &mut Vec<Draft>, path: impl Iterator<Item = u8>) -> usize {
    let mut node = 0usize;
    for step in path {
        let next = drafts.len();
        let child = match drafts.get_mut(node) {
            Some(draft) => *draft.children.entry(step).or_insert(next),
            None => return node,
        };
        if child == next {
            drafts.push(Draft::default());
        }
        node = child;
    }
    node
}

/// One node record.
fn record(nodes: &mut Vec<u8>, label: u8, first: usize, vector: u16) {
    nodes.push(label);
    nodes.extend_from_slice(&index_bytes(first));
    nodes.extend_from_slice(&vector.to_le_bytes());
}

/// An index as four little-endian bytes. Every index here was checked to fit
/// before it is written, so the saturation is never reached.
fn index_bytes(index: usize) -> [u8; 4] {
    u32::try_from(index).unwrap_or(u32::MAX).to_le_bytes()
}
