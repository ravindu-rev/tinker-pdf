//! CMaps: `/ToUnicode` and CID mappings (9.7.5, 9.10.3).
//!
//! A CMap is PostScript-flavoured but only a handful of operators matter, so
//! this is a small scanner rather than an interpreter — the same decision every
//! reader makes, and the reason a malformed CMap degrades to partial coverage
//! instead of taking a document down.
//!
//! Codespace ranges are what make a CMap more than a table: they say how many
//! bytes each code occupies, and a font with mixed one- and two-byte codes is
//! unreadable without them.
//!
//! A CMap may also be written *differentially* (9.7.5.3): `usecmap` in the
//! body, or `/UseCMap` in the stream dictionary, names a parent whose whole
//! definition the child inherits and then overrides in the few places it
//! cares about. Both spellings are followed here. Neither can be fetched
//! here — a parent may be a document stream and this is a leaf crate that
//! knows no PDF types (ruling 8) — so the caller supplies a resolver closure,
//! the same shape `tinker-pdf-cos`'s `build_chain` uses to follow an indirect
//! `/DecodeParms`.

use std::collections::HashMap;

/// The most `usecmap` links one chain may follow (9.7.5.3).
///
/// Real CMaps nest one deep, occasionally two. Four is generous, and being
/// finite is the property that matters: the names come out of the document
/// and a cycle guard alone would not stop a chain that never repeats itself.
pub const MAX_USECMAP_DEPTH: usize = 4;

/// One codespace range (9.7.6.2): codes of a fixed byte length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodespaceRange {
    /// How many bytes a code in this range occupies, 1 to 4.
    pub bytes: u8,
    /// Inclusive low bound, as a big-endian integer.
    pub low: u32,
    /// Inclusive high bound.
    pub high: u32,
}

/// A parsed CMap.
#[derive(Clone, Debug, Default)]
pub struct CMap {
    codespaces: Vec<CodespaceRange>,
    single: HashMap<u32, Vec<char>>,
    ranges: Vec<(u32, u32, Vec<char>)>,
    cid_single: HashMap<u32, u32>,
    cid_ranges: Vec<(u32, u32, u32)>,
    /// True for `Identity-H` and `Identity-V`, where code equals CID.
    identity: bool,
    /// True for `Identity-V` and other vertical CMaps (9.7.4.3).
    vertical: bool,
    /// Whether this CMap's own source wrote `/WMode`.
    ///
    /// 9.7.5.3 makes the child's writing mode win over an inherited one, so
    /// "declared horizontal" and "said nothing" have to be different states.
    /// `vertical` alone cannot tell them apart.
    wmode_declared: bool,
    /// True when this stands in for a predefined CMap whose data is not built
    /// in, so the code-to-CID mapping is assumed rather than known.
    approximate: bool,
}

impl CMap {
    /// The identity CMap of `Identity-H` or `Identity-V`: two-byte codes whose
    /// value is the CID.
    #[must_use]
    pub fn identity(vertical: bool) -> CMap {
        CMap {
            codespaces: vec![CodespaceRange {
                bytes: 2,
                low: 0,
                high: 0xFFFF,
            }],
            identity: true,
            vertical,
            ..CMap::default()
        }
    }

    /// One of the predefined CMaps, by name (9.7.5.2).
    ///
    /// Only the identity pair is built in. The CJK collections are large data
    /// files; a font using one still lays out, because its codes are two bytes
    /// and its `/W` array supplies the advances — only the CID mapping is
    /// approximate, and that is reported by [`CMap::is_approximate`].
    #[must_use]
    pub fn predefined(name: &[u8]) -> Option<CMap> {
        if name == b"Identity-H" {
            return Some(CMap::identity(false));
        }
        if name == b"Identity-V" {
            return Some(CMap::identity(true));
        }

        // The CJK collections are recognized by their registry prefixes. Their
        // real code-to-CID tables are large data files that are not built in,
        // so this stands in with the one property they all share: two-byte
        // codes. Advances still come from the font's own `/W` array, so text
        // positions correctly; only the CID mapping is assumed, and
        // `is_approximate` says so.
        const CJK_PREFIXES: [&[u8]; 14] = [
            b"UniJIS", b"UniGB", b"UniCNS", b"UniKS", b"GBK-", b"GB-", b"ETen", b"90ms", b"90pv",
            b"B5pc", b"KSC", b"Add-", b"Ext-", b"RKSJ",
        ];
        let known =
            CJK_PREFIXES.iter().any(|p| name.starts_with(p)) || name == b"H" || name == b"V";
        known.then(|| CMap {
            codespaces: vec![CodespaceRange {
                bytes: 2,
                low: 0,
                high: 0xFFFF,
            }],
            identity: true,
            vertical: name == b"V" || name.ends_with(b"-V"),
            approximate: true,
            ..CMap::default()
        })
    }

    /// Whether this CMap stands in for one whose data is not built in.
    #[must_use]
    pub fn is_approximate(&self) -> bool {
        self.approximate
    }

    /// Whether the writing mode is vertical (9.7.4.3).
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        self.vertical
    }

    /// Splits a string into codes, using the codespace ranges.
    ///
    /// A byte sequence matching no range is consumed one byte at a time, which
    /// is what 9.7.6.3 prescribes and keeps a damaged string from swallowing
    /// the rest of the text.
    #[must_use]
    pub fn decode_codes(&self, bytes: &[u8]) -> Vec<(u32, u8)> {
        let mut out = Vec::new();
        let mut i = 0usize;

        // With no codespace at all, a simple font's one-byte default applies.
        let default_len = if self.codespaces.is_empty() {
            if self.identity {
                2
            } else {
                1
            }
        } else {
            0
        };

        while i < bytes.len() {
            let mut matched = None;
            if default_len == 0 {
                for len in 1..=4usize {
                    let Some(slice) = bytes.get(i..i + len) else {
                        continue;
                    };
                    let value = slice.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b));
                    if self
                        .codespaces
                        .iter()
                        .any(|r| usize::from(r.bytes) == len && (r.low..=r.high).contains(&value))
                    {
                        matched = Some((value, len as u8));
                        break;
                    }
                }
            }

            let (value, len) = matched.unwrap_or_else(|| {
                let len = if default_len > 0 { default_len } else { 1 }.min(bytes.len() - i);
                let slice = bytes.get(i..i + len).unwrap_or_default();
                (
                    slice.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b)),
                    len as u8,
                )
            });

            out.push((value, len));
            i += usize::from(len).max(1);
        }

        out
    }

    /// The text a code maps to, if this CMap is a `/ToUnicode`.
    #[must_use]
    pub fn to_unicode(&self, code: u32) -> Option<&[char]> {
        if let Some(chars) = self.single.get(&code) {
            return Some(chars);
        }
        for (low, high, base) in &self.ranges {
            if (*low..=*high).contains(&code) {
                return Some(base);
            }
        }
        None
    }

    /// The text a code maps to, with a range's offset applied.
    ///
    /// `bfrange` with a destination string means consecutive codes take
    /// consecutive characters, which matters for the common case of an entire
    /// alphabet in one entry.
    #[must_use]
    pub fn to_unicode_string(&self, code: u32) -> Option<String> {
        if let Some(chars) = self.single.get(&code) {
            return Some(chars.iter().collect());
        }
        for (low, high, base) in &self.ranges {
            if (*low..=*high).contains(&code) {
                let offset = code - low;
                let mut chars = base.clone();
                // Only the final character advances; the rest is a prefix.
                if let Some(last) = chars.last_mut() {
                    if let Some(shifted) = char::from_u32(u32::from(*last) + offset) {
                        *last = shifted;
                    }
                }
                return Some(chars.into_iter().collect());
            }
        }
        None
    }

    /// The CID a code maps to (9.7.5).
    ///
    /// A mapping this CMap states wins over `identity`, which is the
    /// fallthrough rather than a shortcut. The order only becomes visible
    /// once [`CMap::inherit_from`] exists: a child with real `cidrange`
    /// entries can now have an identity parent — every CJK stub
    /// [`CMap::predefined`] returns is one — and testing `identity` first
    /// would have thrown the child's own mappings away.
    #[must_use]
    pub fn cid(&self, code: u32) -> Option<u32> {
        self.cid_mapped(code)
            .or_else(|| self.identity.then_some(code))
    }

    /// The CID this CMap's own entries give a code, ignoring `identity`.
    fn cid_mapped(&self, code: u32) -> Option<u32> {
        if let Some(cid) = self.cid_single.get(&code) {
            return Some(*cid);
        }
        for (low, high, base) in &self.cid_ranges {
            if (*low..=*high).contains(&code) {
                return Some(base + (code - low));
            }
        }
        None
    }

    /// Merges `parent` beneath this CMap: 9.7.5.3's `usecmap`.
    ///
    /// A merge, not a replacement. This CMap's mappings override the
    /// parent's where they overlap and the parent's survive where they do
    /// not, which is the entire point of a differential CMap: it states the
    /// handful of codes that changed and inherits everything else, including
    /// the codespace ranges that decide where one code ends and the next
    /// begins. A child that declared one `cidrange` and nothing else gets
    /// only that one range without this, so `decode_codes` falls back to one
    /// byte per code and the string splits in the wrong places.
    ///
    /// Merge order is the failure this is most likely to get silently wrong,
    /// because a parent that overrode its child would still produce plausible
    /// CIDs. Two rules keep it right, and each has a test:
    ///
    /// - a range lookup returns the *first* match, so the child's ranges stay
    ///   in front and the parent's are appended behind them;
    /// - a single is consulted before any range, so a parent single that the
    ///   child's ranges already answer is dropped rather than inserted, where
    ///   it would outrank them.
    pub fn inherit_from(&mut self, parent: &CMap) {
        // Singles first, while `self` still holds only the child's ranges:
        // the skip test asks whether the child answers this code at all.
        // Iteration order over a `HashMap` does not reach the result, since
        // the test depends on nothing this loop writes.
        for (code, chars) in &parent.single {
            if self.to_unicode(*code).is_none() {
                self.single.insert(*code, chars.clone());
            }
        }
        for (code, cid) in &parent.cid_single {
            if self.cid_mapped(*code).is_none() {
                self.cid_single.insert(*code, *cid);
            }
        }

        self.ranges.extend(parent.ranges.iter().cloned());
        self.cid_ranges.extend(parent.cid_ranges.iter().copied());

        // 9.7.6.2 forbids overlapping codespace ranges, and `decode_codes`
        // asks only whether a value falls inside one, shortest length first
        // — so appending the parent's cannot shadow the child's.
        self.codespaces.extend(parent.codespaces.iter().copied());

        // A code neither states falls through to the parent's assumption, so
        // both the identity fallthrough and the admission that the mapping is
        // guesswork are inherited. Inheriting from a stub must not read as
        // inheriting from the real thing.
        self.identity |= parent.identity;
        self.approximate |= parent.approximate;

        // 9.7.5.3: the child's `/WMode` wins where it declares one.
        if !self.wmode_declared {
            self.vertical = parent.vertical;
        }
    }
}

/// One of the mapping sections a CMap body is built from (9.7.5.3).
///
/// Named rather than anonymous because a warning about a truncated section
/// has to say *which* section stopped early to be worth anything (ruling 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    /// `begincodespacerange`.
    CodespaceRange,
    /// `beginbfchar`.
    BfChar,
    /// `beginbfrange`.
    BfRange,
    /// `begincidchar`.
    CidChar,
    /// `begincidrange`.
    CidRange,
}

impl Section {
    /// The operator that closes this section.
    const fn end(self) -> &'static [u8] {
        match self {
            Section::CodespaceRange => b"endcodespacerange",
            Section::BfChar => b"endbfchar",
            Section::BfRange => b"endbfrange",
            Section::CidChar => b"endcidchar",
            Section::CidRange => b"endcidrange",
        }
    }

    /// A short stable identifier, for a warning name or a test.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Section::CodespaceRange => "codespacerange",
            Section::BfChar => "bfchar",
            Section::BfRange => "bfrange",
            Section::CidChar => "cidchar",
            Section::CidRange => "cidrange",
        }
    }
}

/// Every leniency a CMap parse performs, as data rather than a log line
/// (ruling 10).
///
/// The object these are attributed to is the CMap stream, which the caller
/// knows and this crate cannot (ruling 8) — so a caller re-exporting these as
/// its own typed warnings supplies the address and these supply the reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Warning {
    /// A `begin*` section ended at something that was not its `end*`
    /// operator: end of input, or another keyword. What had parsed was kept,
    /// and the keyword was left unread so the section it opens still parses.
    SectionUnterminated(Section),
    /// A `usecmap` parent was named and could not be found — neither a
    /// predefined CMap (9.7.5.2) nor anything the caller's resolver knew.
    /// The child keeps only what it declared itself.
    ParentUnresolved,
    /// A `usecmap` chain reached [`MAX_USECMAP_DEPTH`]; the parents past that
    /// point were not read.
    ParentChainCapped,
    /// A CMap in a `usecmap` chain used one already on that chain. The link
    /// was refused, which is what makes a self-referential CMap terminate.
    ParentCycle,
}

impl Warning {
    /// A short stable identifier, for a debug dump or a test name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Warning::SectionUnterminated(_) => "cmap-section-unterminated",
            Warning::ParentUnresolved => "cmap-parent-unresolved",
            Warning::ParentChainCapped => "cmap-parent-chain-capped",
            Warning::ParentCycle => "cmap-parent-cycle",
        }
    }
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Warning::SectionUnterminated(s) => {
                write!(f, "cmap-section-unterminated ({})", s.as_str())
            }
            other => f.write_str(other.as_str()),
        }
    }
}

/// A parent CMap that a parse cannot fetch for itself.
///
/// This is the whole of what this crate can say about where a parent lives.
/// Resolving it needs either the predefined set or a document, and a leaf
/// crate has no business holding the second (ruling 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParentRef<'a> {
    /// The name a `usecmap` operator gave, or a `/UseCMap` name entry, that
    /// [`CMap::predefined`] did not recognise.
    Named(&'a [u8]),
    /// The `/UseCMap` entry (Table 120) of the stream dictionary belonging to
    /// the CMap whose source bytes these are. Only a caller holding the
    /// document can address a stream, so only a caller can answer this.
    Dictionary(&'a [u8]),
}

/// What a caller may answer a [`ParentRef`] with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParentSource {
    /// The parent's CMap source, to be parsed in turn — an embedded stream.
    Source(Vec<u8>),
    /// The parent is whatever this name stands for, which is the `/UseCMap`
    /// name form. It goes through the predefined set (9.7.5.2) and then back
    /// through the resolver, so both spellings take one path.
    Named(Vec<u8>),
}

/// A parsed CMap and everything the parse had to tolerate.
#[derive(Clone, Debug)]
pub struct Parsed {
    /// The CMap, with whatever its `usecmap` chain contributed merged in.
    pub cmap: CMap,
    /// Every leniency, in the order it happened. A parent's warnings arrive
    /// before the child's own, because the parent is read first.
    pub warnings: Vec<Warning>,
}

/// Parses an embedded CMap stream.
///
/// The scanner reads only the operators that carry mappings; everything else,
/// including the PostScript prologue every CMap opens with, is skipped. A
/// truncated or malformed section ends that section rather than the parse.
///
/// `usecmap` still resolves against the predefined set, so this is the right
/// entry point for a caller that holds no document. A caller that does should
/// use [`parse_embedded`], which can also follow a parent that is a stream —
/// and which reports what it tolerated instead of discarding it.
#[must_use]
pub fn parse(bytes: &[u8]) -> CMap {
    parse_embedded(bytes, &mut |_| None).cmap
}

/// Parses an embedded CMap and its `usecmap` chain (9.7.5.3).
///
/// `resolve` is how the chain leaves this crate: it is asked for a parent by
/// name, or for the `/UseCMap` entry of a stream dictionary this crate cannot
/// see, and answers with that parent's source or with a name to try instead.
/// Answering `None` to everything is exactly [`parse`].
///
/// The chain is capped at [`MAX_USECMAP_DEPTH`] links and refuses to follow a
/// source already on it, so a CMap that uses itself — directly, or around a
/// chain of any length — terminates (ruling 1).
#[must_use]
pub fn parse_embedded(
    bytes: &[u8],
    resolve: &mut dyn FnMut(ParentRef<'_>) -> Option<ParentSource>,
) -> Parsed {
    let mut chain = Chain {
        resolve,
        visited: vec![bytes.to_vec()],
        depth: 0,
        warnings: Vec::new(),
    };
    let cmap = chain.parse(bytes);
    Parsed {
        cmap,
        warnings: chain.warnings,
    }
}

/// One `usecmap` chain being walked: how far down it is, and what is on it.
struct Chain<'r> {
    resolve: &'r mut dyn FnMut(ParentRef<'_>) -> Option<ParentSource>,
    /// The source of every CMap from the root of the chain down to the one
    /// being parsed. Comparing sources rather than names catches a cycle
    /// whatever the links are called, including one dressed up as two names
    /// for the same stream.
    visited: Vec<Vec<u8>>,
    depth: usize,
    warnings: Vec<Warning>,
}

impl Chain<'_> {
    /// Parses one CMap and merges whatever it inherits.
    fn parse(&mut self, bytes: &[u8]) -> CMap {
        let (mut cmap, named) = parse_body(bytes, &mut self.warnings);

        // 9.7.5.3: the `usecmap` operator and the stream dictionary's
        // `/UseCMap` say the same thing and are required to agree, so either
        // spelling is followed. The body is tried first only because it is
        // the one this crate can see; where the body names a parent that
        // cannot be resolved, the dictionary still gets its turn.
        let mut parent = named.and_then(|name| self.parent(ParentSource::Named(name)));
        if parent.is_none() {
            if let Some(source) = (self.resolve)(ParentRef::Dictionary(bytes)) {
                parent = self.parent(source);
            }
        }
        if let Some(parent) = parent {
            cmap.inherit_from(&parent);
        }
        cmap
    }

    /// Resolves one link of the chain, guarded by depth and by cycle.
    ///
    /// Only parsing a source spends a link. A name that resolves to another
    /// name has not moved down the chain, it has only been spelled again, so
    /// the indirection is a bounded loop rather than a recursion that would
    /// make the cap mean half of what it says.
    fn parent(&mut self, source: ParentSource) -> Option<CMap> {
        if self.depth >= MAX_USECMAP_DEPTH {
            self.warnings.push(Warning::ParentChainCapped);
            return None;
        }

        let mut source = source;
        for _ in 0..MAX_USECMAP_DEPTH {
            match source {
                // 9.7.5.2 reserves the predefined names, so they answer first
                // and a document cannot redefine one. What that set contains
                // is gap 03's problem; that it is consulted is this one's.
                ParentSource::Named(name) => match CMap::predefined(&name) {
                    Some(predefined) => return Some(predefined),
                    None => match (self.resolve)(ParentRef::Named(&name)) {
                        Some(next) => source = next,
                        None => {
                            self.warnings.push(Warning::ParentUnresolved);
                            return None;
                        }
                    },
                },
                ParentSource::Source(bytes) => {
                    if self.visited.contains(&bytes) {
                        self.warnings.push(Warning::ParentCycle);
                        return None;
                    }
                    self.depth += 1;
                    self.visited.push(bytes.clone());
                    let parent = self.parse(&bytes);
                    self.visited.pop();
                    self.depth -= 1;
                    return Some(parent);
                }
            }
        }

        self.warnings.push(Warning::ParentChainCapped);
        None
    }
}

/// Scans one CMap's own source, returning it and the name its `usecmap`
/// operator gave, if any.
fn parse_body(bytes: &[u8], warnings: &mut Vec<Warning>) -> (CMap, Option<Vec<u8>>) {
    let mut cmap = CMap::default();
    let mut parent: Option<Vec<u8>> = None;
    let mut tokens = Tokenizer::new(bytes);
    let mut stack: Vec<Token> = Vec::new();

    while let Some(token) = tokens.next_token() {
        match &token {
            Token::Keyword(k) if k == b"begincodespacerange" => {
                read_codespaces(&mut tokens, &mut cmap, warnings);
                stack.clear();
            }
            Token::Keyword(k) if k == b"beginbfchar" => {
                read_bfchar(&mut tokens, &mut cmap, warnings);
                stack.clear();
            }
            Token::Keyword(k) if k == b"beginbfrange" => {
                read_bfrange(&mut tokens, &mut cmap, warnings);
                stack.clear();
            }
            Token::Keyword(k) if k == b"begincidchar" => {
                read_cidchar(&mut tokens, &mut cmap, warnings);
                stack.clear();
            }
            Token::Keyword(k) if k == b"begincidrange" => {
                read_cidrange(&mut tokens, &mut cmap, warnings);
                stack.clear();
            }
            // 9.7.5.3: `/Name usecmap` inherits everything /Name defines.
            // The first one wins; a second is a file contradicting itself,
            // and picking the earlier statement is at least repeatable.
            Token::Keyword(k) if k == b"usecmap" => {
                if parent.is_none() {
                    if let Some(Token::Name(name)) = stack.last() {
                        parent = Some(name.clone());
                    }
                }
                stack.clear();
            }
            Token::Keyword(k) if k == b"def" => {
                // `/WMode 1 def` selects vertical writing (9.7.5.1).
                if let (Some(Token::Number(n)), Some(Token::Name(name))) =
                    (stack.last(), stack.get(stack.len().wrapping_sub(2)))
                {
                    if name == b"WMode" {
                        cmap.vertical = *n == 1;
                        cmap.wmode_declared = true;
                    }
                }
                stack.clear();
            }
            _ => {
                stack.push(token);
                if stack.len() > 8 {
                    stack.remove(0);
                }
            }
        }
    }

    (cmap, parent)
}

/// One step through a `begin*` section.
enum Step {
    /// This section's own `end*` operator: a clean close.
    Closed,
    /// A token belonging to a record.
    Token(Token),
    /// End of input, or a keyword that is not this section's `end*`. The
    /// keyword is left unread, so whatever it opens is still parsed.
    Broken,
}

impl Tokenizer<'_> {
    /// The next token inside `section`, or how the section ended.
    ///
    /// Matching the `end*` operator by name is what makes a truncated section
    /// noticeable. Terminating on the first token of an unexpected *shape*,
    /// which is what this did before, cannot tell `endcidrange` from
    /// `begincidchar` from end of input — so a section that stopped early
    /// ended exactly as quietly as one that finished, and it swallowed the
    /// next section's opening keyword on the way out.
    fn step(&mut self, section: Section) -> Step {
        let at = self.pos;
        match self.next_token() {
            Some(Token::Keyword(k)) if k == section.end() => Step::Closed,
            Some(Token::Keyword(_)) => {
                self.pos = at;
                Step::Broken
            }
            Some(token) => Step::Token(token),
            None => Step::Broken,
        }
    }
}

fn read_codespaces(tokens: &mut Tokenizer, cmap: &mut CMap, warnings: &mut Vec<Warning>) {
    const SECTION: Section = Section::CodespaceRange;
    loop {
        // Only a record *boundary* may be the end of the section; a `Closed`
        // anywhere else is a record cut in half, and warns like any other.
        let low = match tokens.step(SECTION) {
            Step::Closed => return,
            Step::Token(Token::HexString(low)) => low,
            _ => break,
        };
        let Step::Token(Token::HexString(high)) = tokens.step(SECTION) else {
            break;
        };
        let bytes = low.len().clamp(1, 4) as u8;
        cmap.codespaces.push(CodespaceRange {
            bytes,
            low: be(&low),
            high: be(&high),
        });
    }
    warnings.push(Warning::SectionUnterminated(SECTION));
}

fn read_bfchar(tokens: &mut Tokenizer, cmap: &mut CMap, warnings: &mut Vec<Warning>) {
    const SECTION: Section = Section::BfChar;
    loop {
        let code = match tokens.step(SECTION) {
            Step::Closed => return,
            Step::Token(Token::HexString(code)) => code,
            _ => break,
        };
        match tokens.step(SECTION) {
            Step::Token(Token::HexString(dst)) => {
                cmap.single.insert(be(&code), utf16be_chars(&dst));
            }
            Step::Token(Token::Name(name)) => {
                if let Some(c) =
                    crate::encoding::glyph_name_to_char(&String::from_utf8_lossy(&name))
                {
                    cmap.single.insert(be(&code), vec![c]);
                }
            }
            _ => break,
        }
    }
    warnings.push(Warning::SectionUnterminated(SECTION));
}

fn read_bfrange(tokens: &mut Tokenizer, cmap: &mut CMap, warnings: &mut Vec<Warning>) {
    const SECTION: Section = Section::BfRange;
    'section: loop {
        let low = match tokens.step(SECTION) {
            Step::Closed => return,
            Step::Token(Token::HexString(low)) => low,
            _ => break,
        };
        let Step::Token(Token::HexString(high)) = tokens.step(SECTION) else {
            break;
        };
        match tokens.step(SECTION) {
            Step::Token(Token::HexString(dst)) => {
                cmap.ranges.push((be(&low), be(&high), utf16be_chars(&dst)));
            }
            // The array form gives one destination per code (9.10.3).
            Step::Token(Token::ArrayOpen) => {
                let mut code = be(&low);
                loop {
                    match tokens.step(SECTION) {
                        Step::Token(Token::HexString(dst)) => {
                            cmap.single.insert(code, utf16be_chars(&dst));
                            code = code.saturating_add(1);
                        }
                        Step::Token(Token::ArrayClose) => break,
                        // `endbfrange` inside the array, or anything else:
                        // the array never closed.
                        _ => break 'section,
                    }
                }
            }
            _ => break,
        }
    }
    warnings.push(Warning::SectionUnterminated(SECTION));
}

fn read_cidchar(tokens: &mut Tokenizer, cmap: &mut CMap, warnings: &mut Vec<Warning>) {
    const SECTION: Section = Section::CidChar;
    loop {
        let code = match tokens.step(SECTION) {
            Step::Closed => return,
            Step::Token(Token::HexString(code)) => code,
            _ => break,
        };
        let Step::Token(Token::Number(cid)) = tokens.step(SECTION) else {
            break;
        };
        if let Ok(cid) = u32::try_from(cid) {
            cmap.cid_single.insert(be(&code), cid);
        }
    }
    warnings.push(Warning::SectionUnterminated(SECTION));
}

fn read_cidrange(tokens: &mut Tokenizer, cmap: &mut CMap, warnings: &mut Vec<Warning>) {
    const SECTION: Section = Section::CidRange;
    loop {
        let low = match tokens.step(SECTION) {
            Step::Closed => return,
            Step::Token(Token::HexString(low)) => low,
            _ => break,
        };
        let Step::Token(Token::HexString(high)) = tokens.step(SECTION) else {
            break;
        };
        let Step::Token(Token::Number(cid)) = tokens.step(SECTION) else {
            break;
        };
        if let Ok(cid) = u32::try_from(cid) {
            cmap.cid_ranges.push((be(&low), be(&high), cid));
        }
    }
    warnings.push(Warning::SectionUnterminated(SECTION));
}

fn be(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0u32, |a, &b| (a << 8) | u32::from(b))
}

/// A CMap destination string is UTF-16BE (9.10.3).
fn utf16be_chars(bytes: &[u8]) -> Vec<char> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
        .collect();
    if units.is_empty() {
        // A single byte destination occurs in hand-written CMaps.
        return bytes.iter().map(|&b| char::from(b)).collect();
    }
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    HexString(Vec<u8>),
    Name(Vec<u8>),
    Number(i64),
    Keyword(Vec<u8>),
    ArrayOpen,
    ArrayClose,
}

struct Tokenizer<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(bytes: &'a [u8]) -> Tokenizer<'a> {
        Tokenizer { bytes, pos: 0 }
    }

    fn next_token(&mut self) -> Option<Token> {
        loop {
            let b = *self.bytes.get(self.pos)?;
            if b.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if b == b'%' {
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| c != b'\n' && c != b'\r')
                {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }

        let b = *self.bytes.get(self.pos)?;
        match b {
            b'<' => {
                self.pos += 1;
                let mut hex = Vec::new();
                let mut nibbles = Vec::new();
                while let Some(&c) = self.bytes.get(self.pos) {
                    self.pos += 1;
                    if c == b'>' {
                        break;
                    }
                    if let Some(v) = (c as char).to_digit(16) {
                        nibbles.push(v as u8);
                    }
                }
                if nibbles.len() % 2 == 1 {
                    nibbles.push(0);
                }
                for pair in nibbles.chunks_exact(2) {
                    hex.push((pair[0] << 4) | pair[1]);
                }
                Some(Token::HexString(hex))
            }
            b'/' => {
                self.pos += 1;
                let start = self.pos;
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| !is_delimiter(c) && !c.is_ascii_whitespace())
                {
                    self.pos += 1;
                }
                Some(Token::Name(
                    self.bytes.get(start..self.pos).unwrap_or_default().to_vec(),
                ))
            }
            b'[' => {
                self.pos += 1;
                Some(Token::ArrayOpen)
            }
            b']' => {
                self.pos += 1;
                Some(Token::ArrayClose)
            }
            b'0'..=b'9' | b'-' | b'+' | b'.' => {
                let start = self.pos;
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
                {
                    self.pos += 1;
                }
                let text = String::from_utf8_lossy(self.bytes.get(start..self.pos)?);
                Some(Token::Number(
                    text.parse::<f64>().ok().map(|v| v as i64).unwrap_or(0),
                ))
            }
            _ => {
                let start = self.pos;
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| !is_delimiter(c) && !c.is_ascii_whitespace())
                {
                    self.pos += 1;
                }
                if self.pos == start {
                    self.pos += 1;
                }
                Some(Token::Keyword(
                    self.bytes.get(start..self.pos).unwrap_or_default().to_vec(),
                ))
            }
        }
    }
}

fn is_delimiter(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bfchar_entries_map_single_codes() {
        let src = b"
            /CIDInit /ProcSet findresource begin
            1 begincodespacerange <00> <FF> endcodespacerange
            2 beginbfchar
            <41> <0041>
            <42> <00420043>
            endbfchar
        ";
        let cmap = parse(src);
        assert_eq!(cmap.to_unicode_string(0x41).as_deref(), Some("A"));
        assert_eq!(
            cmap.to_unicode_string(0x42).as_deref(),
            Some("BC"),
            "a destination may be more than one character"
        );
        assert_eq!(cmap.to_unicode_string(0x43), None);
    }

    #[test]
    fn bfrange_advances_the_last_character() {
        let src = b"
            1 begincodespacerange <0000> <FFFF> endcodespacerange
            1 beginbfrange
            <0041> <0043> <0061>
            endbfrange
        ";
        let cmap = parse(src);
        assert_eq!(cmap.to_unicode_string(0x41).as_deref(), Some("a"));
        assert_eq!(cmap.to_unicode_string(0x42).as_deref(), Some("b"));
        assert_eq!(cmap.to_unicode_string(0x43).as_deref(), Some("c"));
        assert_eq!(cmap.to_unicode_string(0x44), None, "past the range");
    }

    #[test]
    fn bfrange_array_form_gives_one_destination_each() {
        let src = b"
            1 beginbfrange
            <0001> <0003> [<0058> <0059> <005A>]
            endbfrange
        ";
        let cmap = parse(src);
        assert_eq!(cmap.to_unicode_string(1).as_deref(), Some("X"));
        assert_eq!(cmap.to_unicode_string(2).as_deref(), Some("Y"));
        assert_eq!(cmap.to_unicode_string(3).as_deref(), Some("Z"));
    }

    #[test]
    fn codespace_ranges_decide_code_length() {
        let src = b"
            2 begincodespacerange
            <00> <80>
            <8140> <9FFC>
            endcodespacerange
        ";
        let cmap = parse(src);
        // A one-byte code, then a two-byte one.
        let codes = cmap.decode_codes(&[0x41, 0x81, 0x50]);
        assert_eq!(codes, vec![(0x41, 1), (0x8150, 2)]);
    }

    #[test]
    fn identity_maps_two_byte_codes_to_themselves() {
        let cmap = CMap::identity(false);
        assert_eq!(cmap.decode_codes(&[0x00, 0x41]), vec![(0x41, 2)]);
        assert_eq!(cmap.cid(0x41), Some(0x41));
        assert!(!cmap.is_vertical());
        assert!(CMap::identity(true).is_vertical());
    }

    #[test]
    fn cid_ranges_offset_from_their_base() {
        let src = b"
            1 begincidrange
            <0020> <007E> 1
            endcidrange
        ";
        let cmap = parse(src);
        assert_eq!(cmap.cid(0x20), Some(1));
        assert_eq!(cmap.cid(0x21), Some(2));
        assert_eq!(cmap.cid(0x7E), Some(95));
        assert_eq!(cmap.cid(0x7F), None);
    }

    #[test]
    fn vertical_writing_is_read_from_wmode() {
        assert!(parse(b"/WMode 1 def").is_vertical());
        assert!(!parse(b"/WMode 0 def").is_vertical());
    }

    #[test]
    fn malformed_input_ends_a_section_not_the_parse() {
        // A truncated bfchar must not lose the codespace before it.
        let cmap = parse(b"1 begincodespacerange <00> <FF> endcodespacerange 5 beginbfchar <41>");
        assert_eq!(cmap.decode_codes(&[0x41]), vec![(0x41, 1)]);
        for bytes in [b"".as_slice(), b"garbage", b"<<<<", b"beginbfrange"] {
            let _ = parse(bytes);
        }
    }

    // -----------------------------------------------------------------
    // `usecmap` (gap 04, 9.7.5.3).
    // -----------------------------------------------------------------

    /// The parent every inheritance test below uses, built here rather than
    /// taken from the predefined set.
    ///
    /// **This is not a real predefined CMap and these tests prove nothing
    /// about one.** Gap 03 is what makes `90ms-RKSJ-H` and its family real;
    /// until it lands, [`CMap::predefined`] answers those names with a
    /// two-byte identity stub, so a test inheriting from one would be
    /// measuring the stub and would keep passing when the stub was replaced
    /// by something with different contents. What is proved here is the
    /// mechanism: whatever the parent turns out to be, the child inherits all
    /// of it and overrides exactly the part it restates.
    ///
    /// Its CID range is the **decoy**. It covers the same codes the child
    /// covers and answers them differently — 100 against the child's 200 —
    /// so a merge that let the parent win could not pass by accident.
    const PARENT: &[u8] = b"/CIDInit /ProcSet findresource begin
        begincmap
        1 begincodespacerange <0000> <FFFF> endcodespacerange
        1 begincidrange <0041> <005A> 100 endcidrange
        1 begincidchar <0061> 900 endcidchar
        1 beginbfrange <0041> <005A> <0391> endbfrange
        endcmap";

    /// A differential child: one `cidrange`, no codespace, no bf anything.
    ///
    /// This is the shape the gap exists for. Without inheritance it has no
    /// codespace at all, so `decode_codes` falls back to one byte per code
    /// and every two-byte string splits in the wrong place.
    const CHILD: &[u8] = b"/TestParent usecmap
        1 begincidrange <0041> <005A> 200 endcidrange";

    /// A resolver that answers one name with one source and nothing else.
    fn from_named(
        name: &'static [u8],
        source: &'static [u8],
    ) -> impl FnMut(ParentRef<'_>) -> Option<ParentSource> {
        move |want| match want {
            ParentRef::Named(asked) if asked == name => Some(ParentSource::Source(source.to_vec())),
            _ => None,
        }
    }

    /// Milestone 1's exit criterion, first half: a child declaring only one
    /// `cidrange` inherits the parent's codespaces.
    #[test]
    fn a_child_inherits_the_codespaces_it_did_not_declare() {
        let alone = parse(CHILD);
        assert_eq!(
            alone.decode_codes(&[0x00, 0x41]),
            vec![(0x00, 1), (0x41, 1)],
            "with no parent the child has no codespace, so this splits wrongly \
             — which is the defect, stated as an assertion"
        );

        let parsed = parse_embedded(CHILD, &mut from_named(b"TestParent", PARENT));
        assert_eq!(
            parsed.cmap.decode_codes(&[0x00, 0x41]),
            vec![(0x41, 2)],
            "the parent's two-byte codespace now decides where the code ends"
        );
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    /// The risk this gap's own table names as the one most likely to be
    /// silently wrong: merge order, with the parent overriding the child.
    ///
    /// Both define `<0041>` to `<005A>` and they disagree, so there is no
    /// value either could produce by accident. The child says 200; the parent
    /// says 100 and would go on saying it if the ranges were appended the
    /// other way round, or if the parent were merged over the top instead of
    /// underneath.
    #[test]
    fn the_child_s_ranges_win_where_they_overlap_the_parent_s() {
        let parent = parse(PARENT);
        assert_eq!(parent.cid(0x41), Some(100), "what the parent alone says");

        let parsed = parse_embedded(CHILD, &mut from_named(b"TestParent", PARENT));
        let cmap = &parsed.cmap;
        assert_eq!(
            cmap.cid(0x41),
            Some(200),
            "the child's base, not the parent's"
        );
        assert_eq!(cmap.cid(0x42), Some(201), "and it still offsets from there");
        assert_eq!(cmap.cid(0x5A), Some(225), "to the top of the child's range");
        assert_eq!(
            cmap.cid(0x61),
            Some(900),
            "where the child says nothing the parent survives"
        );
        assert_eq!(cmap.cid(0x5B), None, "and where neither does, nothing");
    }

    /// A single is consulted before any range, so a parent `cidchar` inside a
    /// child `cidrange` would outrank it — the same merge-order failure one
    /// level down, and the one a Vec-order fix alone would leave behind.
    #[test]
    fn a_parent_single_does_not_outrank_a_child_range() {
        const SINGLE_PARENT: &[u8] = b"1 begincidchar <0041> 900 endcidchar
            1 begincidchar <0062> 901 endcidchar";
        let parsed = parse_embedded(CHILD, &mut from_named(b"TestParent", SINGLE_PARENT));
        assert_eq!(
            parsed.cmap.cid(0x41),
            Some(200),
            "the child's range covers 0x41, so the parent's single is dropped"
        );
        assert_eq!(
            parsed.cmap.cid(0x62),
            Some(901),
            "the single outside it is kept"
        );
    }

    /// The `bfrange` half of the same rule, since `/ToUnicode` merges too.
    #[test]
    fn text_mappings_merge_the_same_way() {
        const TEXT_CHILD: &[u8] = b"/TestParent usecmap
            1 beginbfchar <0041> <0061> endbfchar";
        let parsed = parse_embedded(TEXT_CHILD, &mut from_named(b"TestParent", PARENT));
        assert_eq!(
            parsed.cmap.to_unicode_string(0x41).as_deref(),
            Some("a"),
            "the child's own entry, not the parent's Greek"
        );
        assert_eq!(
            parsed.cmap.to_unicode_string(0x42).as_deref(),
            Some("\u{392}"),
            "and the parent's range covers what the child did not restate"
        );
    }

    /// Milestone 1's exit criterion, second half. The chain compares sources
    /// rather than names, so a CMap that uses itself under any name stops.
    #[test]
    fn a_cmap_that_uses_itself_terminates() {
        const LOOP: &[u8] = b"/Itself usecmap
            1 begincidrange <0041> <0041> 5 endcidrange";
        let parsed = parse_embedded(LOOP, &mut |want| match want {
            ParentRef::Named(_) => Some(ParentSource::Source(LOOP.to_vec())),
            ParentRef::Dictionary(_) => None,
        });
        assert!(
            parsed.warnings.contains(&Warning::ParentCycle),
            "{:?}",
            parsed.warnings
        );
        assert_eq!(parsed.cmap.cid(0x41), Some(5), "and it keeps its own work");
    }

    /// A cycle that only closes two links later is the same failure with a
    /// longer fuse, and a name-based guard would miss it if the names differ.
    #[test]
    fn a_cycle_around_a_longer_chain_terminates() {
        const A: &[u8] = b"/B usecmap";
        const B: &[u8] = b"/A usecmap";
        let parsed = parse_embedded(A, &mut |want| match want {
            ParentRef::Named(b"B") => Some(ParentSource::Source(B.to_vec())),
            ParentRef::Named(b"A") => Some(ParentSource::Source(A.to_vec())),
            _ => None,
        });
        assert!(
            parsed.warnings.contains(&Warning::ParentCycle),
            "{:?}",
            parsed.warnings
        );
    }

    /// A chain that never repeats itself is stopped by the cap instead. Four
    /// sources are followed, which is what the plan asks for.
    #[test]
    fn a_chain_deeper_than_the_cap_stops_at_it() {
        let mut links = 0usize;
        let parsed = parse_embedded(b"/Link usecmap", &mut |want| match want {
            ParentRef::Named(_) => {
                links += 1;
                Some(ParentSource::Source(
                    format!("/Link{links} usecmap").into_bytes(),
                ))
            }
            ParentRef::Dictionary(_) => None,
        });
        assert!(
            parsed.warnings.contains(&Warning::ParentChainCapped),
            "{:?}",
            parsed.warnings
        );
        assert_eq!(
            links, MAX_USECMAP_DEPTH,
            "four sources followed, and the fifth link refused before the \
             resolver was asked for it"
        );
    }

    /// 9.7.5.2 reserves the predefined names, so one answers before the
    /// resolver is ever asked and a document cannot redefine it.
    #[test]
    fn a_predefined_parent_answers_before_the_resolver() {
        let mut asked = 0usize;
        let parsed = parse_embedded(b"/Identity-H usecmap", &mut |_| {
            asked += 1;
            None
        });
        assert_eq!(asked, 0, "no name reached the resolver");
        assert_eq!(parsed.cmap.decode_codes(&[0x00, 0x41]), vec![(0x41, 2)]);
        assert_eq!(parsed.cmap.cid(0x41), Some(0x41));
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    /// `parse` is `parse_embedded` with a resolver that knows nothing, so a
    /// caller holding no document still inherits from the predefined set.
    #[test]
    fn parse_without_a_resolver_still_follows_a_predefined_parent() {
        let cmap = parse(b"/Identity-V usecmap 1 begincidchar <0041> 7 endcidchar");
        assert!(cmap.is_vertical(), "the parent's writing mode carries");
        assert_eq!(cmap.cid(0x41), Some(7), "the child's own entry wins");
        assert_eq!(cmap.cid(0x42), Some(0x42), "the parent's identity survives");
    }

    /// A parent nobody can find is reported rather than assumed, because the
    /// child that is left is a differential CMap with nothing to differ from.
    #[test]
    fn an_unresolvable_parent_is_reported() {
        let parsed = parse_embedded(CHILD, &mut |_| None);
        assert_eq!(parsed.warnings, vec![Warning::ParentUnresolved]);
        assert_eq!(parsed.cmap.cid(0x41), Some(200), "what it declared itself");
    }

    /// Inheriting from a stub must not read as inheriting from the real
    /// thing. Gap 03 replaces the stub; until then this is what says so.
    #[test]
    fn inheriting_from_an_approximate_parent_stays_approximate() {
        let parsed = parse_embedded(b"/90ms-RKSJ-H usecmap", &mut |_| None);
        assert!(parsed.cmap.is_approximate());
        assert!(!parse(PARENT).is_approximate(), "a real one is not");
    }

    /// 9.7.5.3: the child's `/WMode` wins where it declares one, and where it
    /// does not the parent's applies.
    #[test]
    fn the_child_s_wmode_wins_over_the_parent_s() {
        let inherited = parse(b"/Identity-V usecmap");
        assert!(inherited.is_vertical(), "nothing declared, so the parent's");

        let overridden = parse(b"/Identity-V usecmap /WMode 0 def");
        assert!(
            !overridden.is_vertical(),
            "declared horizontal, which beats a vertical parent"
        );

        let raised = parse(b"/Identity-H usecmap /WMode 1 def");
        assert!(raised.is_vertical(), "and the other way round");
    }

    // -----------------------------------------------------------------
    // `end*` matched by name (gap 04 milestone 3).
    // -----------------------------------------------------------------

    /// Milestone 3's exit criterion: a truncated section produces a warning
    /// rather than ending silently, and the warning says which section.
    #[test]
    fn a_truncated_section_is_reported_and_named() {
        for (source, section) in [
            (
                b"1 begincodespacerange <00>".as_slice(),
                Section::CodespaceRange,
            ),
            (b"1 beginbfchar <41>", Section::BfChar),
            (b"1 beginbfrange <41> <42>", Section::BfRange),
            (b"1 begincidchar <41>", Section::CidChar),
            (b"1 begincidrange <41> <5A>", Section::CidRange),
        ] {
            let parsed = parse_embedded(source, &mut |_| None);
            assert_eq!(
                parsed.warnings,
                vec![Warning::SectionUnterminated(section)],
                "{}",
                String::from_utf8_lossy(source)
            );
        }
    }

    /// A section that closes properly says nothing, which is what makes the
    /// warning above worth anything.
    #[test]
    fn a_well_formed_cmap_reports_nothing() {
        let parsed = parse_embedded(
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange
              1 begincidrange <0041> <005A> 7 endcidrange
              1 beginbfchar <0041> <0061> endbfchar
              1 beginbfrange <0042> <0044> [<0058> <0059> <005A>] endbfrange
              /WMode 0 def",
            &mut |_| None,
        );
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        assert_eq!(parsed.cmap.cid(0x41), Some(7));
        assert_eq!(parsed.cmap.to_unicode_string(0x43).as_deref(), Some("Y"));
    }

    /// The reason to match by name rather than by shape: a section that stops
    /// early used to consume the keyword that ended it, so the *next*
    /// section's `begin` disappeared with it and everything after was read as
    /// loose tokens. The keyword is now left for the outer scan.
    #[test]
    fn a_truncated_section_does_not_swallow_the_next_one() {
        let parsed = parse_embedded(
            b"1 beginbfchar <0041> <0061> begincidrange <0041> <005A> 7 endcidrange",
            &mut |_| None,
        );
        assert_eq!(
            parsed.warnings,
            vec![Warning::SectionUnterminated(Section::BfChar)]
        );
        assert_eq!(
            parsed.cmap.cid(0x41),
            Some(7),
            "the cidrange after the broken bfchar was still read"
        );
        assert_eq!(parsed.cmap.to_unicode_string(0x41).as_deref(), Some("a"));
    }

    /// A section closed by the wrong `end*` is malformed, not merely
    /// finished. Absorbing it silently is how a producer bug becomes an
    /// engine one.
    #[test]
    fn the_wrong_end_operator_does_not_close_a_section() {
        let parsed = parse_embedded(b"1 begincidchar <0041> 7 endbfchar", &mut |_| None);
        assert_eq!(
            parsed.warnings,
            vec![Warning::SectionUnterminated(Section::CidChar)]
        );
        assert_eq!(parsed.cmap.cid(0x41), Some(7), "what parsed is still kept");
    }

    /// Ruling 1, for every path this gap added. None of these may hang or
    /// panic, whatever the resolver does.
    #[test]
    fn hostile_chains_terminate() {
        let sources: [&[u8]; 6] = [
            b"usecmap",
            b"/ usecmap",
            b"/A usecmap /B usecmap /C usecmap",
            b"/A usecmap 1 begincidrange <41> <5A>",
            b"begincodespacerange begincidrange beginbfchar",
            b"/A usecmap endcidrange endbfchar",
        ];
        for source in sources {
            // A resolver that always answers with the caller's own bytes, and
            // one that answers with something new every time.
            let mut counter = 0usize;
            let _ = parse_embedded(source, &mut |_| Some(ParentSource::Source(source.to_vec())));
            let _ = parse_embedded(source, &mut |_| {
                counter += 1;
                Some(ParentSource::Named(format!("N{counter}").into_bytes()))
            });
            let _ = parse_embedded(source, &mut |want| match want {
                ParentRef::Dictionary(bytes) => Some(ParentSource::Source(bytes.to_vec())),
                ParentRef::Named(name) => Some(ParentSource::Named(name.to_vec())),
            });
        }
    }
}
