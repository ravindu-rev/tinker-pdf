//! Markdown, read as CommonMark 0.31.2 onto the HTML path (tier 5's formats
//! row).
//!
//! A hand-written reader, because the rule is that everything is: no part of
//! this is a third-party crate. It follows the specification's own appendix,
//! *A parsing strategy*, in its two phases — the block structure line by line,
//! then the inline structure of each paragraph and heading — and its output is
//! HTML in the form the specification's examples print. [`to_html`] is that
//! output and nothing else, which is what lets it be **held to the
//! specification's published examples** rather than to a fixture written here:
//! `tests/commonmark_spec.rs` reads the 652 examples of `spec.txt` at the
//! 0.31.2 tag, runs each, and holds a counted floor of exact passes.
//!
//! # Onto the HTML path
//!
//! A Markdown document becomes a PDF the way a loose XHTML file does: the HTML
//! is wrapped in an XHTML document and handed to the EPUB reader as a book of
//! one chapter ([`crate::epub::lay_out_one`]). Two things the specification's
//! HTML may carry and an XML reader may not are decided here instead, and both
//! are named in the report rather than absorbed:
//!
//! - **Raw HTML is set as text.** CommonMark passes `<div>` and `<span>`
//!   through verbatim, and a tag that is not well-formed XML would stop the
//!   reader at that point and lose the rest of the document. So the document
//!   path escapes it — every character of the source still reaches the page —
//!   and counts it in [`crate::standalone::TranslationDefect::RawHtmlAsText`].
//! - **A character XML 1.0 §2.2 forbids** — a C0 control other than tab, line
//!   feed and carriage return — is U+FFFD, as CommonMark already makes U+0000.
//!
//! # What is not CommonMark here, by name
//!
//! - **Named character references are XHTML 1.0's 253**, through
//!   `tinker_pdf_xml::xhtml_entity`, not HTML's 2 231: `&copy;` and `&ouml;`
//!   resolve, `&HilbertSpace;` stays literal. HTML's list is not vendored in
//!   this repository (see the entity sets' section of `THIRDPARTY.md`).
//! - **Container nesting stops at [`MAX_MARKDOWN_NESTING`]**: a block quote or
//!   list item that would open past it is read as text and counted.
//! - **On the document path, inline nesting stops at the depth that cap
//!   promises** (`MAX_XHTML_DEPTH`, 202 elements): an emphasis, strong
//!   emphasis or link that would nest deeper is set without its element, its
//!   text kept, and counted, so the XML reader is never stopped by depth and
//!   nothing after a deep nest is lost. [`to_html`] nests as deep as
//!   CommonMark says.

use crate::standalone::TranslationDefect;

/// The deepest a Markdown document's container blocks — block quotes, lists
/// and list items — may nest.
///
/// | | |
/// | --- | --- |
/// | **This cap** | **100** |
///
/// Each line walks every open container to decide what it continues, so a
/// document that opens a block quote per `>` and then carries many lines costs
/// lines × depth; a megabyte of `>` on one line followed by a megabyte of short
/// lines would otherwise be a trillion steps. The number is the XML reader's
/// own ceiling arriving early: a document element and `<body>` are two levels,
/// a list is two more per level (`<ul>` and `<li>`), so 100 containers is at
/// most 202 elements — inside `tinker_pdf_xml`'s 256, which is what the
/// document path hands the result to. That bound is enforced on the inlines as
/// well, which nest as deep as their delimiters go: see `MAX_XHTML_DEPTH`. A
/// container that would open past this cap is read as the paragraph text it
/// then is, and counted as [`TranslationDefect::NestingTooDeep`].
pub const MAX_MARKDOWN_NESTING: usize = 100;

/// The floor of how many bytes of destination and title a document's
/// reference links may copy out of their definitions, in total.
///
/// | | |
/// | --- | --- |
/// | **This cap** | **100 KiB** |
///
/// The budget is **the larger of this and the document's own length**, which
/// is cmark's rule. A reference is the one construct whose output is not
/// bounded by its input: one definition with a destination of `L` bytes and
/// `k` uses of a three-byte `[a]` cost `L + 3k` to write and copy `k × L` —
/// half a megabyte of each is sixty-two gigabytes of HTML. A document whose
/// links were written by a person spends a small fraction of its own length,
/// so the budget refuses nothing that was meant; past it, a reference reads as
/// the text it is written as and [`TranslationDefect::ReferenceBudgetSpent`]
/// counts it.
pub const MAX_MARKDOWN_REFERENCE_BYTES: usize = 100 << 10;

/// CommonMark's four spaces of indentation.
const CODE_INDENT: usize = 4;

// ---- the trees ---------------------------------------------------------------

/// A doubly linked tree in a vector, which is the shape both phases want: the
/// block parser appends and closes, and the inline parser's emphasis and link
/// passes unlink a run of siblings and re-parent it under a new node.
struct Tree<K> {
    nodes: Vec<Node<K>>,
}

struct Node<K> {
    kind: K,
    parent: Option<usize>,
    first: Option<usize>,
    last: Option<usize>,
    prev: Option<usize>,
    next: Option<usize>,
}

impl<K> Tree<K> {
    fn new(root: K) -> Self {
        Tree {
            nodes: vec![Node {
                kind: root,
                parent: None,
                first: None,
                last: None,
                prev: None,
                next: None,
            }],
        }
    }

    fn push(&mut self, kind: K) -> usize {
        self.nodes.push(Node {
            kind,
            parent: None,
            first: None,
            last: None,
            prev: None,
            next: None,
        });
        self.nodes.len() - 1
    }

    fn append(&mut self, parent: usize, child: usize) {
        self.unlink(child);
        let last = self.nodes[parent].last;
        self.nodes[child].parent = Some(parent);
        self.nodes[child].prev = last;
        match last {
            Some(last) => self.nodes[last].next = Some(child),
            None => self.nodes[parent].first = Some(child),
        }
        self.nodes[parent].last = Some(child);
    }

    fn insert_after(&mut self, at: usize, sibling: usize) {
        self.unlink(sibling);
        let next = self.nodes[at].next;
        let parent = self.nodes[at].parent;
        self.nodes[sibling].parent = parent;
        self.nodes[sibling].prev = Some(at);
        self.nodes[sibling].next = next;
        self.nodes[at].next = Some(sibling);
        match next {
            Some(next) => self.nodes[next].prev = Some(sibling),
            None => {
                if let Some(parent) = parent {
                    self.nodes[parent].last = Some(sibling);
                }
            }
        }
    }

    fn unlink(&mut self, at: usize) {
        let (prev, next, parent) = (
            self.nodes[at].prev,
            self.nodes[at].next,
            self.nodes[at].parent,
        );
        match prev {
            Some(prev) => self.nodes[prev].next = next,
            None => {
                if let Some(parent) = parent {
                    self.nodes[parent].first = next;
                }
            }
        }
        match next {
            Some(next) => self.nodes[next].prev = prev,
            None => {
                if let Some(parent) = parent {
                    self.nodes[parent].last = prev;
                }
            }
        }
        self.nodes[at].prev = None;
        self.nodes[at].next = None;
        self.nodes[at].parent = None;
    }
}

// ---- blocks ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Bullet(u8),
    Ordered(u8),
}

#[derive(Clone, Copy)]
struct ListData {
    kind: ListKind,
    start: u32,
    tight: bool,
    marker_offset: usize,
    padding: usize,
}

enum BlockKind {
    Document,
    BlockQuote,
    List(ListData),
    Item(ListData),
    Paragraph,
    Heading(u8),
    ThematicBreak,
    Code {
        fenced: bool,
        fence_char: u8,
        fence_len: usize,
        fence_offset: usize,
        info: String,
        literal: String,
    },
    Html {
        kind: u8,
        literal: String,
    },
}

struct Block {
    kind: BlockKind,
    open: bool,
    content: String,
    start_line: usize,
    end_line: usize,
    /// The inline tree a paragraph or heading became, after phase 2.
    inlines: Option<Tree<Inline>>,
}

impl Block {
    fn new(kind: BlockKind, line: usize) -> Block {
        Block {
            kind,
            open: true,
            content: String::new(),
            start_line: line,
            end_line: line,
            inlines: None,
        }
    }

    fn accepts_lines(&self) -> bool {
        matches!(
            self.kind,
            BlockKind::Paragraph | BlockKind::Code { .. } | BlockKind::Html { .. }
        )
    }

    fn is_container(&self) -> bool {
        matches!(
            self.kind,
            BlockKind::Document | BlockKind::BlockQuote | BlockKind::List(_) | BlockKind::Item(_)
        )
    }

    fn can_contain(&self, child: &BlockKind) -> bool {
        match self.kind {
            BlockKind::Document | BlockKind::BlockQuote | BlockKind::Item(_) => {
                !matches!(child, BlockKind::Item(_))
            }
            BlockKind::List(_) => matches!(child, BlockKind::Item(_)),
            _ => false,
        }
    }
}

/// What one link reference definition said.
#[derive(Clone)]
struct Reference {
    destination: String,
    title: String,
}

/// The reference definitions of a document, by normalised label, first
/// definition winning (§4.7).
#[derive(Default)]
struct RefMap {
    entries: std::collections::BTreeMap<String, Reference>,
}

impl RefMap {
    fn get(&self, label: &str) -> Option<&Reference> {
        self.entries.get(label)
    }

    fn insert(&mut self, label: String, reference: Reference) {
        self.entries.entry(label).or_insert(reference);
    }
}

/// Phase 1: the block structure.
struct Parser {
    tree: Tree<Block>,
    tip: usize,
    oldtip: usize,
    line: std::rc::Rc<str>,
    line_number: usize,
    offset: usize,
    column: usize,
    next_nonspace: usize,
    next_nonspace_column: usize,
    indent: usize,
    indented: bool,
    blank: bool,
    partially_consumed_tab: bool,
    all_closed: bool,
    last_matched: usize,
    refmap: RefMap,
    /// Open containers, the document included, for [`MAX_MARKDOWN_NESTING`].
    too_deep: usize,
}

fn is_space_or_tab(c: Option<u8>) -> bool {
    matches!(c, Some(b' ' | b'\t'))
}

impl Parser {
    fn new() -> Parser {
        Parser {
            tree: Tree::new(Block::new(BlockKind::Document, 1)),
            tip: 0,
            oldtip: 0,
            line: std::rc::Rc::from(""),
            line_number: 0,
            offset: 0,
            column: 0,
            next_nonspace: 0,
            next_nonspace_column: 0,
            indent: 0,
            indented: false,
            blank: false,
            partially_consumed_tab: false,
            all_closed: true,
            last_matched: 0,
            refmap: RefMap::default(),
            too_deep: 0,
        }
    }

    fn peek(&self, at: usize) -> Option<u8> {
        self.line.as_bytes().get(at).copied()
    }

    fn rest(&self, from: usize) -> &str {
        self.line.get(from..).unwrap_or("")
    }

    fn find_next_nonspace(&mut self) {
        let bytes = self.line.as_bytes();
        let mut i = self.offset;
        let mut cols = self.column;
        let mut c = None;
        while let Some(&b) = bytes.get(i) {
            c = Some(b);
            match b {
                b' ' => {
                    i += 1;
                    cols += 1;
                }
                b'\t' => {
                    i += 1;
                    cols += 4 - (cols % 4);
                }
                _ => break,
            }
            c = None;
        }
        self.blank = matches!(c, None | Some(b'\n' | b'\r'));
        self.next_nonspace = i;
        self.next_nonspace_column = cols;
        self.indent = self.next_nonspace_column - self.column;
        self.indented = self.indent >= CODE_INDENT;
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        while count > 0 {
            let Some(c) = self.peek(self.offset) else {
                break;
            };
            if c == b'\t' {
                let to_tab = 4 - (self.column % 4);
                if columns {
                    self.partially_consumed_tab = to_tab > count;
                    let advance = to_tab.min(count);
                    self.column += advance;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= advance;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                // A multi-byte character is one column and all of its bytes:
                // block starts are ASCII, and an offset that stopped inside a
                // character would be a slice no `get` could answer.
                let width = self
                    .rest(self.offset)
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
                self.offset += width;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn add_line(&mut self) {
        let mut text = String::new();
        if self.partially_consumed_tab {
            self.offset += 1;
            let to_tab = 4 - (self.column % 4);
            text.extend(std::iter::repeat_n(' ', to_tab));
        }
        text.push_str(self.rest(self.offset));
        text.push('\n');
        let tip = self.tip;
        self.tree.nodes[tip].kind.content_mut().push_str(&text);
    }

    fn add_child(&mut self, kind: BlockKind, _offset: usize) -> usize {
        while !self.tree.nodes[self.tip].kind.can_contain(&kind) {
            let tip = self.tip;
            self.finalize(tip, self.line_number.saturating_sub(1));
        }
        let child = self.tree.push(Block::new(kind, self.line_number));
        let tip = self.tip;
        self.tree.append(tip, child);
        self.tip = child;
        child
    }

    fn close_unmatched_blocks(&mut self) {
        if !self.all_closed {
            while self.oldtip != self.last_matched {
                let parent = self.tree.nodes[self.oldtip].parent.unwrap_or(0);
                let oldtip = self.oldtip;
                self.finalize(oldtip, self.line_number.saturating_sub(1));
                self.oldtip = parent;
            }
            self.all_closed = true;
        }
    }

    /// How many containers are open above and including `at`.
    fn depth(&self, mut at: usize) -> usize {
        let mut depth = 0;
        loop {
            if self.tree.nodes[at].kind.is_container() {
                depth += 1;
            }
            match self.tree.nodes[at].parent {
                Some(parent) => at = parent,
                None => return depth,
            }
        }
    }

    fn finalize(&mut self, block: usize, line_number: usize) {
        let above = self.tree.nodes[block].parent;
        self.tree.nodes[block].kind.open = false;
        self.tree.nodes[block].kind.end_line = line_number;
        self.finalize_kind(block);
        self.tip = above.unwrap_or(0);
    }

    fn finalize_kind(&mut self, block: usize) {
        let is_list = matches!(self.tree.nodes[block].kind.kind, BlockKind::List(_));
        let is_item = matches!(self.tree.nodes[block].kind.kind, BlockKind::Item(_));
        let is_paragraph = matches!(self.tree.nodes[block].kind.kind, BlockKind::Paragraph);
        if is_paragraph {
            let content = std::mem::take(&mut self.tree.nodes[block].kind.content);
            let content = strip_references(content, &mut self.refmap);
            // A paragraph is opened only by a line with text on it, so one with
            // none left has given all of it to reference definitions — here,
            // or at a setext underline that then could not use it.
            let empty = content.trim_matches(is_ascii_ws).is_empty();
            self.tree.nodes[block].kind.content = content;
            if empty {
                self.tree.unlink(block);
            }
            return;
        }
        let node = &mut self.tree.nodes[block].kind;
        match &mut node.kind {
            BlockKind::Code {
                fenced,
                info,
                literal,
                ..
            } => {
                let content = std::mem::take(&mut node.content);
                if *fenced {
                    let (first, rest) = content.split_once('\n').unwrap_or((&content, ""));
                    *info = unescape(first.trim_matches(is_ascii_ws));
                    *literal = rest.to_owned();
                } else {
                    let mut lines: Vec<&str> = content.split('\n').collect();
                    while lines
                        .last()
                        .is_some_and(|l| l.bytes().all(|b| b == b' ' || b == b'\t'))
                    {
                        lines.pop();
                    }
                    let mut joined = lines.join("\n");
                    joined.push('\n');
                    *literal = joined;
                }
                return;
            }
            BlockKind::Html { literal, .. } => {
                let content = std::mem::take(&mut node.content);
                // commonmark.js's `/(\n *)+$/`: trailing blank lines go.
                let mut end = content.len();
                loop {
                    let trimmed = content.get(..end).unwrap_or("").trim_end_matches(' ');
                    if trimmed.ends_with('\n') {
                        end = trimmed.len() - 1;
                    } else {
                        break;
                    }
                }
                *literal = content.get(..end).unwrap_or("").to_owned();
                return;
            }
            _ => {}
        }
        if is_list {
            {
                let mut tight = true;
                let mut item = self.tree.nodes[block].first;
                'items: while let Some(at) = item {
                    if self.tree.nodes[at].next.is_some() && self.ends_with_blank_line(at) {
                        tight = false;
                        break;
                    }
                    let mut sub = self.tree.nodes[at].first;
                    while let Some(child) = sub {
                        if self.tree.nodes[child].next.is_some() && self.ends_with_blank_line(child)
                        {
                            tight = false;
                            break 'items;
                        }
                        sub = self.tree.nodes[child].next;
                    }
                    item = self.tree.nodes[at].next;
                }
                if let Some(last) = self.tree.nodes[block].last {
                    let end = self.tree.nodes[last].kind.end_line;
                    self.tree.nodes[block].kind.end_line = end;
                }
                if let BlockKind::List(data) = &mut self.tree.nodes[block].kind.kind {
                    data.tight = tight;
                }
            }
        } else if is_item {
            let end = match self.tree.nodes[block].last {
                Some(last) => self.tree.nodes[last].kind.end_line,
                None => self.tree.nodes[block].kind.start_line,
            };
            self.tree.nodes[block].kind.end_line = end;
        }
    }

    /// Whether a blank line separates `block` from the sibling after it.
    fn ends_with_blank_line(&self, block: usize) -> bool {
        match self.tree.nodes[block].next {
            Some(next) => {
                self.tree.nodes[block].kind.end_line + 1 != self.tree.nodes[next].kind.start_line
            }
            None => false,
        }
    }

    fn incorporate_line(&mut self, line: &str, defects: &mut Defects) {
        let mut all_matched = true;
        let mut container = 0usize;
        self.oldtip = self.tip;
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;
        self.line_number += 1;
        self.line = std::rc::Rc::from(line.replace('\0', "\u{FFFD}"));

        // Phase 1, step 1: which open blocks this line continues.
        while let Some(last) = self.tree.nodes[container].last {
            if !self.tree.nodes[last].kind.open {
                break;
            }
            container = last;
            self.find_next_nonspace();
            match self.continues(container) {
                Continue::Matched => {}
                Continue::Failed => {
                    all_matched = false;
                }
                Continue::Done => return,
            }
            if !all_matched {
                container = self.tree.nodes[container].parent.unwrap_or(0);
                break;
            }
        }

        self.all_closed = container == self.oldtip;
        self.last_matched = container;

        // Step 2: new block starts.
        let mut matched_leaf =
            !matches!(self.tree.nodes[container].kind.kind, BlockKind::Paragraph)
                && self.tree.nodes[container].kind.accepts_lines();
        while !matched_leaf {
            self.find_next_nonspace();
            let special = matches!(
                self.peek(self.next_nonspace),
                Some(
                    b'#' | b'`' | b'~' | b'*' | b'+' | b'_' | b'=' | b'<' | b'>' | b'0'
                        ..=b'9' | b'-'
                )
            );
            if !self.indented && !special {
                self.advance_next_nonspace();
                break;
            }
            match self.try_starts(container, defects) {
                Start::None => {
                    self.advance_next_nonspace();
                    break;
                }
                Start::Container => container = self.tip,
                Start::Leaf => {
                    container = self.tip;
                    matched_leaf = true;
                }
            }
        }

        // Step 3: what remains is text.
        let tip_is_paragraph = matches!(self.tree.nodes[self.tip].kind.kind, BlockKind::Paragraph);
        if !self.all_closed && !self.blank && tip_is_paragraph {
            // A lazy continuation line.
            self.add_line();
        } else {
            self.close_unmatched_blocks();
            if self.tree.nodes[container].kind.accepts_lines() {
                self.add_line();
                if let BlockKind::Html { kind, .. } = self.tree.nodes[container].kind.kind {
                    if (1..=5).contains(&kind) && html_block_closes(kind, self.rest(self.offset)) {
                        let line_number = self.line_number;
                        self.finalize(container, line_number);
                    }
                }
            } else if self.offset < self.line.len() && !self.blank {
                self.add_child(BlockKind::Paragraph, self.offset);
                self.advance_next_nonspace();
                self.add_line();
            }
        }
    }

    fn continues(&mut self, container: usize) -> Continue {
        let has_child = self.tree.nodes[container].first.is_some();
        let shape = match &self.tree.nodes[container].kind.kind {
            BlockKind::Document | BlockKind::List(_) => Shape::Always,
            BlockKind::BlockQuote => Shape::BlockQuote,
            BlockKind::Item(data) => Shape::Item(data.marker_offset + data.padding),
            BlockKind::Heading(_) | BlockKind::ThematicBreak => Shape::Never,
            BlockKind::Code {
                fenced,
                fence_char,
                fence_len,
                fence_offset,
                ..
            } => Shape::Code(*fenced, *fence_char, *fence_len, *fence_offset),
            BlockKind::Html { kind, .. } => Shape::Html(*kind),
            BlockKind::Paragraph => Shape::Paragraph,
        };
        match shape {
            Shape::Always => Continue::Matched,
            Shape::BlockQuote => {
                if !self.indented && self.peek(self.next_nonspace) == Some(b'>') {
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    if is_space_or_tab(self.peek(self.offset)) {
                        self.advance_offset(1, true);
                    }
                    Continue::Matched
                } else {
                    Continue::Failed
                }
            }
            Shape::Item(needed) => {
                if self.blank {
                    if !has_child {
                        return Continue::Failed;
                    }
                    self.advance_next_nonspace();
                    Continue::Matched
                } else if self.indent >= needed {
                    self.advance_offset(needed, true);
                    Continue::Matched
                } else {
                    Continue::Failed
                }
            }
            Shape::Never => Continue::Failed,
            Shape::Code(fenced, fence_char, fence_len, fence_offset) => {
                if fenced {
                    let closing =
                        if self.indent <= 3 && self.peek(self.next_nonspace) == Some(fence_char) {
                            closing_fence(self.rest(self.next_nonspace))
                        } else {
                            None
                        };
                    if closing.is_some_and(|len| len >= fence_len) {
                        let line_number = self.line_number;
                        self.finalize(container, line_number);
                        return Continue::Done;
                    }
                    let mut i = fence_offset;
                    while i > 0 && is_space_or_tab(self.peek(self.offset)) {
                        self.advance_offset(1, true);
                        i -= 1;
                    }
                    Continue::Matched
                } else if self.indent >= CODE_INDENT {
                    self.advance_offset(CODE_INDENT, true);
                    Continue::Matched
                } else if self.blank {
                    self.advance_next_nonspace();
                    Continue::Matched
                } else {
                    Continue::Failed
                }
            }
            Shape::Html(kind) => {
                if self.blank && (kind == 6 || kind == 7) {
                    Continue::Failed
                } else {
                    Continue::Matched
                }
            }
            Shape::Paragraph => {
                if self.blank {
                    Continue::Failed
                } else {
                    Continue::Matched
                }
            }
        }
    }

    fn try_starts(&mut self, container: usize, defects: &mut Defects) -> Start {
        // A second handle on the line rather than a copy of its rest: a line
        // of a million `>` tries a start once per `>`, and a copy each time
        // would be a quadratic of the line's own length.
        let line = std::rc::Rc::clone(&self.line);
        let line_rest = line.get(self.next_nonspace..).unwrap_or("");
        let first = self.peek(self.next_nonspace);
        let container_is_paragraph =
            matches!(self.tree.nodes[container].kind.kind, BlockKind::Paragraph);
        let deep = self.depth(container) >= MAX_MARKDOWN_NESTING;

        // Block quote.
        if !self.indented && first == Some(b'>') {
            if deep {
                self.too_deep += 1;
                defects.note(TranslationDefect::NestingTooDeep);
                return Start::None;
            }
            self.advance_next_nonspace();
            self.advance_offset(1, false);
            if is_space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
            self.close_unmatched_blocks();
            self.add_child(BlockKind::BlockQuote, self.next_nonspace);
            return Start::Container;
        }

        // ATX heading.
        if !self.indented {
            if let Some(level) = atx_marker(line_rest) {
                self.advance_next_nonspace();
                self.advance_offset(level, false);
                self.close_unmatched_blocks();
                let heading = self.add_child(BlockKind::Heading(level as u8), self.next_nonspace);
                let text = atx_content(self.rest(self.offset));
                self.tree.nodes[heading].kind.content = text;
                let len = self.line.len();
                self.offset = len;
                return Start::Leaf;
            }
        }

        // Fenced code block.
        if !self.indented {
            if let Some((fence_char, fence_len)) = opening_fence(line_rest) {
                self.close_unmatched_blocks();
                let indent = self.indent;
                self.add_child(
                    BlockKind::Code {
                        fenced: true,
                        fence_char,
                        fence_len,
                        fence_offset: indent,
                        info: String::new(),
                        literal: String::new(),
                    },
                    self.next_nonspace,
                );
                self.advance_next_nonspace();
                self.advance_offset(fence_len, false);
                return Start::Leaf;
            }
        }

        // HTML block.
        if !self.indented && first == Some(b'<') {
            let maybe_lazy = !self.all_closed
                && !self.blank
                && matches!(self.tree.nodes[self.tip].kind.kind, BlockKind::Paragraph);
            for kind in 1..=7u8 {
                if html_block_opens(kind, line_rest)
                    && (kind < 7 || (!container_is_paragraph && !maybe_lazy))
                {
                    self.close_unmatched_blocks();
                    self.add_child(
                        BlockKind::Html {
                            kind,
                            literal: String::new(),
                        },
                        self.offset,
                    );
                    return Start::Leaf;
                }
            }
        }

        // Setext heading underline.
        if !self.indented && container_is_paragraph {
            if let Some(level) = setext_underline(line_rest) {
                self.close_unmatched_blocks();
                let content = std::mem::take(&mut self.tree.nodes[container].kind.content);
                let content = strip_references(content, &mut self.refmap);
                if !content.is_empty() {
                    let node = &mut self.tree.nodes[container].kind;
                    node.kind = BlockKind::Heading(level);
                    node.content = content;
                    self.tip = container;
                    self.offset = self.line.len();
                    return Start::Leaf;
                }
                self.tree.nodes[container].kind.content = content;
                return Start::None;
            }
        }

        // Thematic break.
        if !self.indented && thematic_break(line_rest) {
            self.close_unmatched_blocks();
            self.add_child(BlockKind::ThematicBreak, self.next_nonspace);
            self.offset = self.line.len();
            return Start::Leaf;
        }

        // List item.
        let container_is_list = matches!(self.tree.nodes[container].kind.kind, BlockKind::List(_));
        if !self.indented || container_is_list {
            if let Some(data) = self.list_marker(container_is_paragraph) {
                // The depth a list item reaches is the list's and the item's.
                if self.depth(container) + 2 > MAX_MARKDOWN_NESTING {
                    self.too_deep += 1;
                    defects.note(TranslationDefect::NestingTooDeep);
                    return Start::None;
                }
                self.close_unmatched_blocks();
                let tip_matches = match self.tree.nodes[self.tip].kind.kind {
                    BlockKind::List(list) => lists_match(&list, &data),
                    _ => false,
                };
                if !tip_matches {
                    self.add_child(BlockKind::List(data), self.next_nonspace);
                }
                self.add_child(BlockKind::Item(data), self.next_nonspace);
                return Start::Container;
            }
        }

        // Indented code block.
        let tip_is_paragraph = matches!(self.tree.nodes[self.tip].kind.kind, BlockKind::Paragraph);
        if self.indented && !tip_is_paragraph && !self.blank {
            self.advance_offset(CODE_INDENT, true);
            self.close_unmatched_blocks();
            self.add_child(
                BlockKind::Code {
                    fenced: false,
                    fence_char: 0,
                    fence_len: 0,
                    fence_offset: 0,
                    info: String::new(),
                    literal: String::new(),
                },
                self.offset,
            );
            return Start::Leaf;
        }
        Start::None
    }

    /// §5.2's list marker at the next non-space, and the item's padding.
    ///
    /// Mutates the offsets only on a match, which is what commonmark.js's
    /// `parseListMarker` does and what the caller relies on.
    fn list_marker(&mut self, in_paragraph: bool) -> Option<ListData> {
        if self.indent >= 4 {
            return None;
        }
        let rest = self.rest(self.next_nonspace);
        let bytes = rest.as_bytes();
        let (kind, start, marker_len) = match bytes.first().copied() {
            Some(c @ (b'*' | b'+' | b'-')) => (ListKind::Bullet(c), 1, 1),
            Some(b'0'..=b'9') => {
                let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
                if digits > 9 {
                    return None;
                }
                let delimiter = bytes.get(digits).copied();
                let Some(delimiter @ (b'.' | b')')) = delimiter else {
                    return None;
                };
                let start: u32 = rest.get(..digits)?.parse().ok()?;
                if in_paragraph && start != 1 {
                    return None;
                }
                (ListKind::Ordered(delimiter), start, digits + 1)
            }
            _ => return None,
        };
        let after = bytes.get(marker_len).copied();
        if !matches!(after, None | Some(b' ' | b'\t')) {
            return None;
        }
        if in_paragraph
            && rest
                .get(marker_len..)
                .unwrap_or("")
                .bytes()
                .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            return None;
        }
        let marker_offset = self.indent;
        self.advance_next_nonspace();
        self.advance_offset(marker_len, true);
        let spaces_start_column = self.column;
        let spaces_start_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            let next = self.peek(self.offset);
            if !(self.column - spaces_start_column < 5 && is_space_or_tab(next)) {
                break;
            }
        }
        let blank_item = self.peek(self.offset).is_none();
        let spaces_after_marker = self.column - spaces_start_column;
        let padding = if !(1..5).contains(&spaces_after_marker) || blank_item {
            self.column = spaces_start_column;
            self.offset = spaces_start_offset;
            if is_space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
            marker_len + 1
        } else {
            marker_len + spaces_after_marker
        };
        Some(ListData {
            kind,
            start,
            tight: true,
            marker_offset,
            padding,
        })
    }
}

impl Block {
    fn content_mut(&mut self) -> &mut String {
        &mut self.content
    }
}

enum Continue {
    Matched,
    Failed,
    Done,
}

/// What [`Parser::continues`] needs of an open block, copied out so the
/// parser's offsets can move while it decides.
enum Shape {
    Always,
    BlockQuote,
    Item(usize),
    Never,
    Code(bool, u8, usize, usize),
    Html(u8),
    Paragraph,
}

enum Start {
    None,
    Container,
    Leaf,
}

fn lists_match(list: &ListData, item: &ListData) -> bool {
    list.kind == item.kind
}

fn is_ascii_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}')
}

/// `^#{1,6}(?:[ \t]+|$)`, answering the number of `#`.
fn atx_marker(rest: &str) -> Option<usize> {
    let hashes = rest.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    match rest.as_bytes().get(hashes) {
        None | Some(b' ' | b'\t') => Some(hashes),
        _ => None,
    }
}

/// An ATX heading's content: the closing sequence of `#`s and the spaces
/// around it removed (§4.2).
fn atx_content(rest: &str) -> String {
    let trimmed = rest.trim_end_matches([' ', '\t']);
    // `^[ \t]*#+[ \t]*$`: nothing but a closing sequence.
    let body = trimmed.trim_start_matches([' ', '\t']);
    if !body.is_empty() && body.bytes().all(|b| b == b'#') {
        return String::new();
    }
    // `[ \t]+#+[ \t]*$`: a closing sequence after white space.
    let without = trimmed.trim_end_matches('#');
    if without.len() < trimmed.len() && without.ends_with([' ', '\t']) {
        return without.trim_end_matches([' ', '\t']).to_owned();
    }
    rest.to_owned()
}

/// `^`{3,}(?!.*`)|^~{3,}`: the fence character and length.
fn opening_fence(rest: &str) -> Option<(u8, usize)> {
    let c = *rest.as_bytes().first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == c).count();
    if len < 3 {
        return None;
    }
    if c == b'`' && rest.get(len..).unwrap_or("").contains('`') {
        return None;
    }
    Some((c, len))
}

/// `^(?:`{3,}|~{3,})(?=[ \t]*$)`: a closing fence's length.
fn closing_fence(rest: &str) -> Option<usize> {
    let c = *rest.as_bytes().first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == c).count();
    if len < 3 {
        return None;
    }
    rest.get(len..)
        .unwrap_or("")
        .bytes()
        .all(|b| b == b' ' || b == b'\t')
        .then_some(len)
}

/// `^(?:=+|-+)[ \t]*$`: the level a setext underline gives.
fn setext_underline(rest: &str) -> Option<u8> {
    let c = *rest.as_bytes().first()?;
    if c != b'=' && c != b'-' {
        return None;
    }
    let len = rest.bytes().take_while(|&b| b == c).count();
    rest.get(len..)
        .unwrap_or("")
        .bytes()
        .all(|b| b == b' ' || b == b'\t')
        .then_some(if c == b'=' { 1 } else { 2 })
}

/// Three or more of one of `*`, `-`, `_`, with only spaces and tabs between.
fn thematic_break(rest: &str) -> bool {
    let Some(&c) = rest.as_bytes().first() else {
        return false;
    };
    if !matches!(c, b'*' | b'-' | b'_') {
        return false;
    }
    let mut count = 0;
    for b in rest.bytes() {
        if b == c {
            count += 1;
        } else if b != b' ' && b != b'\t' {
            return false;
        }
    }
    count >= 3
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// §4.6's seven start conditions.
fn html_block_opens(kind: u8, rest: &str) -> bool {
    let bytes = rest.as_bytes();
    match kind {
        1 => {
            for tag in ["<script", "<pre", "<textarea", "<style"] {
                if starts_with_ignore_case(rest, tag) {
                    return matches!(
                        bytes.get(tag.len()),
                        None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0B' | b'\x0C' | b'>')
                    );
                }
            }
            false
        }
        2 => rest.starts_with("<!--"),
        3 => rest.starts_with("<?"),
        4 => rest.starts_with("<!") && bytes.get(2).is_some_and(u8::is_ascii_alphabetic),
        5 => rest.starts_with("<![CDATA["),
        6 => {
            let after = if rest.starts_with("</") { 2 } else { 1 };
            if bytes.first() != Some(&b'<') {
                return false;
            }
            let name_len = bytes
                .get(after..)
                .unwrap_or_default()
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric())
                .count();
            let Some(name) = rest.get(after..after + name_len) else {
                return false;
            };
            if !BLOCK_TAGS.iter().any(|tag| tag.eq_ignore_ascii_case(name)) {
                return false;
            }
            match bytes.get(after + name_len) {
                None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0B' | b'\x0C' | b'>') => true,
                Some(b'/') => bytes.get(after + name_len + 1) == Some(&b'>'),
                _ => false,
            }
        }
        7 => {
            let tag = open_tag(rest, 0).or_else(|| closing_tag(rest, 0));
            match tag {
                Some(end) => {
                    // `pre`, `script`, `style` and `textarea` are type 1's.
                    rest.get(end..)
                        .unwrap_or("")
                        .bytes()
                        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0B' | b'\x0C'))
                }
                None => false,
            }
        }
        _ => false,
    }
}

/// §4.6's end conditions for kinds 1 to 5, on one line.
fn html_block_closes(kind: u8, line: &str) -> bool {
    match kind {
        1 => {
            let lower = line.to_ascii_lowercase();
            ["</script>", "</pre>", "</textarea>", "</style>"]
                .iter()
                .any(|end| lower.contains(end))
        }
        2 => line.contains("-->"),
        3 => line.contains("?>"),
        4 => line.contains('>'),
        5 => line.contains("]]>"),
        _ => false,
    }
}

// ---- raw HTML grammar (§6.6) -------------------------------------------------

/// Spaces, tabs and up to one line ending, from `at`; the end.
fn html_whitespace(bytes: &[u8], mut at: usize) -> usize {
    let mut newlines = 0;
    while let Some(&b) = bytes.get(at) {
        match b {
            b' ' | b'\t' => at += 1,
            b'\n' if newlines == 0 => {
                newlines += 1;
                at += 1;
            }
            b'\r' if newlines == 0 => {
                newlines += 1;
                at += 1;
                if bytes.get(at) == Some(&b'\n') {
                    at += 1;
                }
            }
            _ => break,
        }
    }
    at
}

/// A tag name at `at`: an ASCII letter and then letters, digits and `-`.
fn tag_name(bytes: &[u8], at: usize) -> Option<usize> {
    if !bytes.get(at)?.is_ascii_alphabetic() {
        return None;
    }
    let len = bytes
        .get(at..)?
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
        .count();
    Some(at + len)
}

/// An open tag starting at `at` (which holds `<`); the end, past `>`.
fn open_tag(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'<') {
        return None;
    }
    let mut pos = tag_name(bytes, at + 1)?;
    loop {
        let ws_end = html_whitespace(bytes, pos);
        // An attribute needs white space before it.
        if ws_end > pos {
            if let Some(after) = attribute(bytes, ws_end) {
                pos = after;
                continue;
            }
        }
        pos = ws_end;
        break;
    }
    if bytes.get(pos) == Some(&b'/') {
        pos += 1;
    }
    (bytes.get(pos) == Some(&b'>')).then_some(pos + 1)
}

/// One attribute: a name and an optional value specification.
fn attribute(bytes: &[u8], at: usize) -> Option<usize> {
    let first = *bytes.get(at)?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b':') {
        return None;
    }
    let mut pos = at + 1;
    while bytes
        .get(pos)
        .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
    {
        pos += 1;
    }
    // An optional value specification.
    let before_eq = html_whitespace(bytes, pos);
    if bytes.get(before_eq) == Some(&b'=') {
        let value_at = html_whitespace(bytes, before_eq + 1);
        match bytes.get(value_at) {
            Some(b'\'') => {
                let close = bytes
                    .get(value_at + 1..)?
                    .iter()
                    .position(|&b| b == b'\'')?;
                return Some(value_at + 1 + close + 1);
            }
            Some(b'"') => {
                let close = bytes.get(value_at + 1..)?.iter().position(|&b| b == b'"')?;
                return Some(value_at + 1 + close + 1);
            }
            Some(_) => {
                let len = bytes
                    .get(value_at..)?
                    .iter()
                    .take_while(|b| {
                        !matches!(
                            b,
                            b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`'
                        )
                    })
                    .count();
                if len == 0 {
                    return None;
                }
                return Some(value_at + len);
            }
            None => return None,
        }
    }
    Some(pos)
}

/// A closing tag at `at`; the end.
fn closing_tag(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(at..at + 2) != Some(b"</".as_slice()) {
        return None;
    }
    let pos = html_whitespace(bytes, tag_name(bytes, at + 2)?);
    (bytes.get(pos) == Some(&b'>')).then_some(pos + 1)
}

// ---- phase 2: inlines --------------------------------------------------------

enum Inline {
    Root,
    Text(String),
    SoftBreak,
    LineBreak,
    Code(String),
    Html(String),
    Emph,
    Strong,
    Link { destination: String, title: String },
    Image { destination: String, title: String },
}

struct Delimiter {
    cc: u8,
    count: usize,
    original: usize,
    node: usize,
    can_open: bool,
    can_close: bool,
    previous: Option<usize>,
    next: Option<usize>,
}

struct Bracket {
    node: usize,
    previous_delimiter: Option<usize>,
    index: usize,
    image: bool,
    active: bool,
    bracket_after: bool,
}

/// Where a search for a construct's end already failed, so a hostile run of
/// openers costs one scan rather than one per opener.
#[derive(Default)]
struct Memo {
    /// Backtick runs, by length, as start offsets.
    ticks: Option<Vec<(usize, usize)>>,
    no_comment_end_from: Option<usize>,
    no_pi_end_from: Option<usize>,
    no_cdata_end_from: Option<usize>,
    no_declaration_end_from: Option<usize>,
}

struct InlineParser<'a> {
    subject: &'a str,
    pos: usize,
    tree: Tree<Inline>,
    delimiters: Vec<Delimiter>,
    top: Option<usize>,
    brackets: Vec<Bracket>,
    /// Every non-image bracket below this index is already inactive.
    inactive_below: usize,
    refmap: &'a RefMap,
    /// Bytes of destination and title references may still copy, for the
    /// whole document.
    budget: &'a mut usize,
    defects: &'a mut Defects,
    memo: Memo,
}

fn is_escapable(c: u8) -> bool {
    c.is_ascii_punctuation()
}

fn is_unicode_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{000C}' | '\r' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

fn is_punctuation(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_punctuation()
    } else {
        tinker_pdf_layout::unicode::is_punctuation_or_symbol(c)
    }
}

impl<'a> InlineParser<'a> {
    fn new(
        subject: &'a str,
        refmap: &'a RefMap,
        budget: &'a mut usize,
        defects: &'a mut Defects,
    ) -> Self {
        InlineParser {
            subject,
            pos: 0,
            tree: Tree::new(Inline::Root),
            delimiters: Vec::new(),
            top: None,
            brackets: Vec::new(),
            inactive_below: 0,
            refmap,
            budget,
            defects,
            memo: Memo::default(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.subject.as_bytes().get(self.pos).copied()
    }

    fn text(&mut self, text: &str) -> usize {
        let node = self.tree.push(Inline::Text(text.to_owned()));
        self.tree.append(0, node);
        node
    }

    fn append(&mut self, inline: Inline) -> usize {
        let node = self.tree.push(inline);
        self.tree.append(0, node);
        node
    }

    fn parse(mut self) -> Tree<Inline> {
        while self.pos < self.subject.len() {
            self.parse_inline();
        }
        self.process_emphasis(None);
        self.tree
    }

    fn parse_inline(&mut self) {
        let Some(c) = self.peek() else {
            return;
        };
        let handled = match c {
            b'\n' => self.newline(),
            b'\\' => self.backslash(),
            b'`' => self.backticks(),
            b'*' | b'_' => self.delimiter(c),
            b'[' => {
                let start = self.pos;
                self.pos += 1;
                let node = self.text("[");
                self.add_bracket(node, start, false);
                true
            }
            b'!' => self.bang(),
            b']' => self.close_bracket(),
            b'<' => self.autolink() || self.html_tag(),
            b'&' => self.entity(),
            _ => self.string(),
        };
        if !handled {
            let len = self
                .subject
                .get(self.pos..)
                .and_then(|rest| rest.chars().next())
                .map_or(1, char::len_utf8);
            let piece = self.subject.get(self.pos..self.pos + len).unwrap_or("");
            let piece = piece.to_owned();
            self.text(&piece);
            self.pos += len;
        }
    }

    /// `[^\n`\[\]\\!<&*_'"]+`.
    fn string(&mut self) -> bool {
        let rest = self.subject.get(self.pos..).unwrap_or("");
        let len = rest
            .bytes()
            .position(|b| {
                matches!(
                    b,
                    b'\n' | b'`' | b'[' | b']' | b'\\' | b'!' | b'<' | b'&' | b'*' | b'_'
                )
            })
            .unwrap_or(rest.len());
        if len == 0 {
            return false;
        }
        let piece = rest.get(..len).unwrap_or("").to_owned();
        self.pos += len;
        self.text(&piece);
        true
    }

    fn newline(&mut self) -> bool {
        self.pos += 1;
        let last = self.tree.nodes[0].last;
        let mut hard = false;
        if let Some(last) = last {
            if let Inline::Text(text) = &mut self.tree.nodes[last].kind {
                if text.ends_with(' ') {
                    hard = text.ends_with("  ");
                    let trimmed = text.trim_end_matches(' ').len();
                    text.truncate(trimmed);
                }
            }
        }
        self.append(if hard {
            Inline::LineBreak
        } else {
            Inline::SoftBreak
        });
        // Leading spaces on the next line are not content.
        while self.peek() == Some(b' ') {
            self.pos += 1;
        }
        true
    }

    fn backslash(&mut self) -> bool {
        self.pos += 1;
        match self.peek() {
            Some(b'\n') => {
                self.pos += 1;
                self.append(Inline::LineBreak);
            }
            Some(c) if is_escapable(c) => {
                self.pos += 1;
                self.text(&(c as char).to_string());
            }
            _ => {
                self.text("\\");
            }
        }
        true
    }

    fn backticks(&mut self) -> bool {
        let bytes = self.subject.as_bytes();
        let start = self.pos;
        let len = bytes
            .get(start..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| b == b'`')
            .count();
        let after_open = start + len;
        // Every run of backticks in the subject, found once.
        let runs = self.memo.ticks.get_or_insert_with(|| {
            let mut runs = Vec::new();
            let mut at = 0;
            while at < bytes.len() {
                if bytes[at] == b'`' {
                    let run = bytes[at..].iter().take_while(|&&b| b == b'`').count();
                    runs.push((at, run));
                    at += run;
                } else {
                    at += 1;
                }
            }
            runs
        });
        let first_after = runs.partition_point(|(at, _)| *at < after_open);
        let closing = runs
            .get(first_after..)
            .unwrap_or_default()
            .iter()
            .find(|(_, run)| *run == len)
            .map(|(at, _)| *at);
        match closing {
            Some(close) => {
                let contents = self
                    .subject
                    .get(after_open..close)
                    .unwrap_or("")
                    .replace(['\n', '\r'], " ");
                let stripped = if contents.len() >= 2
                    && contents.starts_with(' ')
                    && contents.ends_with(' ')
                    && contents.bytes().any(|b| b != b' ')
                {
                    contents.get(1..contents.len() - 1).unwrap_or("").to_owned()
                } else {
                    contents
                };
                self.append(Inline::Code(stripped));
                self.pos = close + len;
            }
            None => {
                let ticks = "`".repeat(len);
                self.text(&ticks);
                self.pos = after_open;
            }
        }
        true
    }

    fn scan_delimiters(&self, cc: u8) -> (usize, bool, bool) {
        let bytes = self.subject.as_bytes();
        let count = bytes
            .get(self.pos..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| b == cc)
            .count();
        let before = self
            .subject
            .get(..self.pos)
            .and_then(|head| head.chars().next_back())
            .unwrap_or('\n');
        let after = self
            .subject
            .get(self.pos + count..)
            .and_then(|tail| tail.chars().next())
            .unwrap_or('\n');
        let after_ws = is_unicode_whitespace(after);
        let after_punct = is_punctuation(after);
        let before_ws = is_unicode_whitespace(before);
        let before_punct = is_punctuation(before);
        let left = !after_ws && (!after_punct || before_ws || before_punct);
        let right = !before_ws && (!before_punct || after_ws || after_punct);
        let (can_open, can_close) = if cc == b'_' {
            (
                left && (!right || before_punct),
                right && (!left || after_punct),
            )
        } else {
            (left, right)
        };
        (count, can_open, can_close)
    }

    fn delimiter(&mut self, cc: u8) -> bool {
        let (count, can_open, can_close) = self.scan_delimiters(cc);
        if count == 0 {
            return false;
        }
        let start = self.pos;
        self.pos += count;
        let piece = self.subject.get(start..self.pos).unwrap_or("").to_owned();
        let node = self.text(&piece);
        if can_open || can_close {
            let index = self.delimiters.len();
            self.delimiters.push(Delimiter {
                cc,
                count,
                original: count,
                node,
                can_open,
                can_close,
                previous: self.top,
                next: None,
            });
            if let Some(top) = self.top {
                self.delimiters[top].next = Some(index);
            }
            self.top = Some(index);
        }
        true
    }

    fn remove_delimiter(&mut self, d: usize) {
        let (previous, next) = (self.delimiters[d].previous, self.delimiters[d].next);
        if let Some(previous) = previous {
            self.delimiters[previous].next = next;
        }
        match next {
            Some(next) => self.delimiters[next].previous = previous,
            None => self.top = previous,
        }
    }

    fn process_emphasis(&mut self, bottom: Option<usize>) {
        let mut openers_bottom: [Option<usize>; 14] = [bottom; 14];
        // The first closer above the bottom.
        let mut closer = self.top;
        while let Some(c) = closer {
            if self.delimiters[c].previous == bottom {
                break;
            }
            closer = self.delimiters[c].previous;
        }
        while let Some(c) = closer {
            if !self.delimiters[c].can_close {
                closer = self.delimiters[c].next;
                continue;
            }
            let cc = self.delimiters[c].cc;
            let index = (if cc == b'_' { 2 } else { 8 })
                + (if self.delimiters[c].can_open { 3 } else { 0 })
                + self.delimiters[c].original % 3;
            let mut opener = self.delimiters[c].previous;
            let mut found = false;
            while let Some(o) = opener {
                if Some(o) == bottom || Some(o) == openers_bottom[index] {
                    break;
                }
                let odd = (self.delimiters[c].can_open || self.delimiters[o].can_close)
                    && self.delimiters[c].original % 3 != 0
                    && (self.delimiters[o].original + self.delimiters[c].original) % 3 == 0;
                if self.delimiters[o].cc == cc && self.delimiters[o].can_open && !odd {
                    found = true;
                    break;
                }
                opener = self.delimiters[o].previous;
            }
            let old_closer = c;
            match (found, opener) {
                (true, Some(o)) => {
                    let used = if self.delimiters[c].count >= 2 && self.delimiters[o].count >= 2 {
                        2
                    } else {
                        1
                    };
                    let (opener_node, closer_node) =
                        (self.delimiters[o].node, self.delimiters[c].node);
                    self.delimiters[o].count -= used;
                    self.delimiters[c].count -= used;
                    for node in [opener_node, closer_node] {
                        if let Inline::Text(text) = &mut self.tree.nodes[node].kind {
                            let keep = text.len().saturating_sub(used);
                            text.truncate(keep);
                        }
                    }
                    let emph = self.tree.push(if used == 1 {
                        Inline::Emph
                    } else {
                        Inline::Strong
                    });
                    let mut tmp = self.tree.nodes[opener_node].next;
                    while let Some(t) = tmp {
                        if t == closer_node {
                            break;
                        }
                        let next = self.tree.nodes[t].next;
                        self.tree.append(emph, t);
                        tmp = next;
                    }
                    self.tree.insert_after(opener_node, emph);
                    // Remove the delimiters between opener and closer.
                    if self.delimiters[o].next != Some(c) {
                        self.delimiters[o].next = Some(c);
                        self.delimiters[c].previous = Some(o);
                    }
                    if self.delimiters[o].count == 0 {
                        self.tree.unlink(opener_node);
                        self.remove_delimiter(o);
                    }
                    if self.delimiters[c].count == 0 {
                        self.tree.unlink(closer_node);
                        let next = self.delimiters[c].next;
                        self.remove_delimiter(c);
                        closer = next;
                    }
                }
                _ => {
                    closer = self.delimiters[c].next;
                    openers_bottom[index] = self.delimiters[old_closer].previous;
                    if !self.delimiters[old_closer].can_open {
                        self.remove_delimiter(old_closer);
                    }
                }
            }
        }
        while let Some(top) = self.top {
            if Some(top) == bottom {
                break;
            }
            self.remove_delimiter(top);
        }
    }

    fn pop_bracket(&mut self) {
        self.brackets.pop();
        // A bracket pushed later at an index below the watermark is active,
        // so the watermark comes down with the stack.
        self.inactive_below = self.inactive_below.min(self.brackets.len());
    }

    fn add_bracket(&mut self, node: usize, index: usize, image: bool) {
        if let Some(last) = self.brackets.last_mut() {
            last.bracket_after = true;
        }
        self.brackets.push(Bracket {
            node,
            previous_delimiter: self.top,
            index,
            image,
            active: true,
            bracket_after: false,
        });
    }

    fn bang(&mut self) -> bool {
        let start = self.pos;
        self.pos += 1;
        if self.peek() == Some(b'[') {
            self.pos += 1;
            let node = self.text("![");
            self.add_bracket(node, start + 1, true);
        } else {
            self.text("!");
        }
        true
    }

    fn close_bracket(&mut self) -> bool {
        self.pos += 1;
        let start = self.pos;
        let Some(opener) = self.brackets.last() else {
            self.text("]");
            return true;
        };
        if !opener.active {
            self.text("]");
            self.pop_bracket();
            return true;
        }
        let (opener_node, opener_index, image, bracket_after, previous_delimiter) = (
            opener.node,
            opener.index,
            opener.image,
            opener.bracket_after,
            opener.previous_delimiter,
        );
        let savepos = self.pos;
        let mut matched = None;
        // An inline link.
        if self.peek() == Some(b'(') {
            self.pos += 1;
            self.spnl();
            if let Some(destination) = self.link_destination() {
                let before_title = self.pos;
                self.spnl();
                // A title needs white space between it and the destination.
                let title = if self.pos > before_title {
                    self.link_title()
                } else {
                    None
                };
                self.spnl();
                if self.peek() == Some(b')') {
                    self.pos += 1;
                    matched = Some((destination, title.unwrap_or_default()));
                }
            }
            if matched.is_none() {
                self.pos = savepos;
            }
        }
        if matched.is_none() {
            let before_label = self.pos;
            let n = self.link_label();
            let label = if n > 2 {
                self.subject.get(before_label..before_label + n)
            } else if !bracket_after {
                self.subject.get(opener_index..start)
            } else {
                None
            };
            if n == 0 {
                self.pos = savepos;
            }
            if let Some(label) = label {
                if let Some(reference) = self.refmap.get(&normalize_label(label)) {
                    // A reference copies its definition's destination and
                    // title into the output, which is the one place a short
                    // input becomes a long one; see
                    // [`MAX_MARKDOWN_REFERENCE_BYTES`].
                    let cost = reference.destination.len() + reference.title.len();
                    if cost <= *self.budget {
                        *self.budget -= cost;
                        matched = Some((reference.destination.clone(), reference.title.clone()));
                    } else {
                        self.defects.note(TranslationDefect::ReferenceBudgetSpent);
                    }
                }
            }
        }
        match matched {
            Some((destination, title)) => {
                let node = self.tree.push(if image {
                    Inline::Image { destination, title }
                } else {
                    Inline::Link { destination, title }
                });
                let mut tmp = self.tree.nodes[opener_node].next;
                while let Some(t) = tmp {
                    let next = self.tree.nodes[t].next;
                    self.tree.append(node, t);
                    tmp = next;
                }
                self.tree.append(0, node);
                self.process_emphasis(previous_delimiter);
                self.pop_bracket();
                self.tree.unlink(opener_node);
                if !image {
                    // §6.3: no links in links, so every `[` still open is
                    // dead. The ones below the watermark already are, which is
                    // what keeps a thousand links after a thousand `[` from
                    // being a million steps.
                    let from = self.inactive_below.min(self.brackets.len());
                    for bracket in self.brackets.get_mut(from..).unwrap_or_default() {
                        if !bracket.image {
                            bracket.active = false;
                        }
                    }
                    self.inactive_below = self.brackets.len();
                }
            }
            None => {
                self.pop_bracket();
                self.pos = start;
                self.text("]");
            }
        }
        true
    }

    /// Optional spaces and tabs, at most one line ending, more spaces.
    fn spnl(&mut self) {
        let bytes = self.subject.as_bytes();
        while matches!(bytes.get(self.pos), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
        if bytes.get(self.pos) == Some(&b'\n') {
            self.pos += 1;
            while matches!(bytes.get(self.pos), Some(b' ' | b'\t')) {
                self.pos += 1;
            }
        }
    }

    fn link_destination(&mut self) -> Option<String> {
        let (destination, end) = link_destination(self.subject, self.pos)?;
        self.pos = end;
        Some(destination)
    }

    fn link_title(&mut self) -> Option<String> {
        let (title, end) = link_title(self.subject, self.pos)?;
        self.pos = end;
        Some(title)
    }

    fn link_label(&mut self) -> usize {
        match link_label(self.subject, self.pos) {
            Some(end) => {
                let n = end - self.pos;
                self.pos = end;
                n
            }
            None => 0,
        }
    }

    fn autolink(&mut self) -> bool {
        let rest = self.subject.get(self.pos..).unwrap_or("");
        if let Some(len) = email_autolink(rest) {
            let address = rest.get(1..len - 1).unwrap_or("").to_owned();
            let link = self.append(Inline::Link {
                destination: normalize_uri(&format!("mailto:{address}")),
                title: String::new(),
            });
            let text = self.tree.push(Inline::Text(address));
            self.tree.append(link, text);
            self.pos += len;
            return true;
        }
        if let Some(len) = uri_autolink(rest) {
            let uri = rest.get(1..len - 1).unwrap_or("").to_owned();
            let link = self.append(Inline::Link {
                destination: normalize_uri(&uri),
                title: String::new(),
            });
            let text = self.tree.push(Inline::Text(uri));
            self.tree.append(link, text);
            self.pos += len;
            return true;
        }
        false
    }

    fn html_tag(&mut self) -> bool {
        let Some(end) = self.raw_html_end() else {
            return false;
        };
        let raw = self.subject.get(self.pos..end).unwrap_or("").to_owned();
        self.append(Inline::Html(raw));
        self.pos = end;
        true
    }

    /// §6.6's HTML tag at the cursor, memoising the searches that can fail
    /// only by reaching the end.
    fn raw_html_end(&mut self) -> Option<usize> {
        let text = self.subject;
        let at = self.pos;
        if let Some(end) = open_tag(text, at).or_else(|| closing_tag(text, at)) {
            return Some(end);
        }
        let rest = text.get(at..).unwrap_or("");
        if rest.starts_with("<!-->") {
            return Some(at + 5);
        }
        if rest.starts_with("<!--->") {
            return Some(at + 6);
        }
        if rest.starts_with("<!--") {
            return memo_search(&mut self.memo.no_comment_end_from, text, at + 4, "-->");
        }
        if rest.starts_with("<?") {
            return memo_search(&mut self.memo.no_pi_end_from, text, at + 2, "?>");
        }
        if rest.starts_with("<![CDATA[") {
            return memo_search(&mut self.memo.no_cdata_end_from, text, at + 9, "]]>");
        }
        if rest.starts_with("<!") && rest.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic) {
            return memo_search(&mut self.memo.no_declaration_end_from, text, at + 3, ">");
        }
        None
    }

    fn entity(&mut self) -> bool {
        let rest = self.subject.get(self.pos..).unwrap_or("");
        match entity(rest) {
            Some((decoded, len)) => {
                self.pos += len;
                self.text(&decoded);
                true
            }
            None => false,
        }
    }
}

/// The end of `needle` searched for from `from`, past it; remembering where a
/// search already failed so a hostile run of openers is not quadratic.
fn memo_search(memo: &mut Option<usize>, text: &str, from: usize, needle: &str) -> Option<usize> {
    if memo.is_some_and(|failed| from >= failed) {
        return None;
    }
    match text.get(from..).and_then(|rest| rest.find(needle)) {
        Some(found) => Some(from + found + needle.len()),
        None => {
            *memo = Some(from);
            None
        }
    }
}

/// §6.5's URI autolink: `<`, a scheme of 2 to 32 characters, `:`, and no
/// space, `<`, `>` or control; the length including both brackets.
fn uri_autolink(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    if !bytes.get(1)?.is_ascii_alphabetic() {
        return None;
    }
    let scheme = bytes
        .get(1..)?
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
        .count();
    if !(2..=32).contains(&scheme) || bytes.get(1 + scheme) != Some(&b':') {
        return None;
    }
    let mut at = 2 + scheme;
    while let Some(&b) = bytes.get(at) {
        match b {
            b'>' => return Some(at + 1),
            b'<' | 0..=0x20 => return None,
            _ => at += 1,
        }
    }
    None
}

/// §6.5's email autolink.
fn email_autolink(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    let local = bytes
        .get(1..)?
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(b))
        .count();
    if local == 0 || bytes.get(1 + local) != Some(&b'@') {
        return None;
    }
    let mut at = 2 + local;
    loop {
        // A label: alphanumeric, then up to 61 alphanumerics or hyphens, ending
        // alphanumeric.
        let start = at;
        if !bytes.get(at)?.is_ascii_alphanumeric() {
            return None;
        }
        while bytes
            .get(at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'-')
        {
            at += 1;
        }
        let label = bytes.get(start..at)?;
        if label.len() > 63 || label.last() == Some(&b'-') {
            return None;
        }
        match bytes.get(at) {
            Some(b'.') => at += 1,
            Some(b'>') => return Some(at + 1),
            _ => return None,
        }
    }
}

/// A link label at `at` (`[`), at most 999 characters inside and no
/// unescaped bracket; the end, past `]`.
fn link_label(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let mut pos = at + 1;
    let mut chars = 0;
    while let Some(&b) = bytes.get(pos) {
        match b {
            b']' => return (chars <= 999).then_some(pos + 1),
            b'[' => return None,
            b'\\' => {
                pos += 1;
                chars += 1;
                if let Some(next) = text.get(pos..).and_then(|r| r.chars().next()) {
                    pos += next.len_utf8();
                    chars += 1;
                }
            }
            _ => {
                let len = text
                    .get(pos..)
                    .and_then(|r| r.chars().next())
                    .map_or(1, char::len_utf8);
                pos += len;
                chars += 1;
            }
        }
        if chars > 1000 {
            return None;
        }
    }
    None
}

/// A link destination at `at`; its normalised text and its end.
fn link_destination(text: &str, at: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(at) == Some(&b'<') {
        let mut pos = at + 1;
        while let Some(&b) = bytes.get(pos) {
            match b {
                b'>' => {
                    let inner = text.get(at + 1..pos).unwrap_or("");
                    return Some((normalize_uri(&unescape(inner)), pos + 1));
                }
                b'<' | b'\n' | b'\r' => return None,
                b'\\' if bytes.get(pos + 1).is_some_and(|c| is_escapable(*c)) => pos += 2,
                _ => pos += 1,
            }
        }
        return None;
    }
    let mut pos = at;
    let mut depth = 0usize;
    while let Some(&b) = bytes.get(pos) {
        match b {
            b'\\' if bytes.get(pos + 1).is_some_and(|c| is_escapable(*c)) => pos += 2,
            b'(' => {
                depth += 1;
                // CommonMark lets an implementation bound this, and asks for
                // three levels at least.
                if depth > 32 {
                    return None;
                }
                pos += 1;
            }
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                pos += 1;
            }
            0..=0x20 | 0x7F => break,
            _ => pos += 1,
        }
    }
    if pos == at && bytes.get(pos) != Some(&b')') {
        return None;
    }
    if depth != 0 {
        return None;
    }
    let raw = text.get(at..pos)?;
    Some((normalize_uri(&unescape(raw)), pos))
}

/// A link title at `at`; its unescaped text and its end.
fn link_title(text: &str, at: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let close = match bytes.get(at)? {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut pos = at + 1;
    while let Some(&b) = bytes.get(pos) {
        if b == b'\\' && bytes.get(pos + 1).is_some() {
            pos += 2;
            continue;
        }
        if b == close {
            let inner = text.get(at + 1..pos).unwrap_or("");
            return Some((unescape(inner), pos + 1));
        }
        if close == b')' && b == b'(' {
            return None;
        }
        pos += 1;
    }
    None
}

/// §4.7's label matching: Unicode case fold, consecutive white space as one
/// space, outer white space gone.
fn normalize_label(label: &str) -> String {
    let inner = label
        .strip_prefix('[')
        .and_then(|l| l.strip_suffix(']'))
        .unwrap_or(label);
    let collapsed = inner
        .split([' ', '\t', '\n', '\r'])
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    collapsed.to_lowercase().to_uppercase()
}

/// A link reference definition at the start of `s` (§4.7), recorded in
/// `refmap`; how many bytes it took, or `None`.
fn parse_reference(s: &str, refmap: &mut RefMap) -> Option<usize> {
    let bytes = s.as_bytes();
    let label_end = link_label(s, 0)?;
    let raw_label = s.get(..label_end)?;
    if bytes.get(label_end) != Some(&b':') {
        return None;
    }
    let mut pos = label_end + 1;
    pos = spnl_at(bytes, pos);
    let (destination, after_destination) = link_destination(s, pos)?;
    pos = after_destination;
    let before_title = pos;
    let mut pos2 = spnl_at(bytes, pos);
    let mut title = None;
    if pos2 != before_title {
        if let Some((t, end)) = link_title(s, pos2) {
            title = Some(t);
            pos2 = end;
        }
    }
    if title.is_none() {
        pos2 = before_title;
    }
    // At a line end?
    let at_line_end = |p: usize| -> Option<usize> {
        let mut q = p;
        while matches!(bytes.get(q), Some(b' ' | b'\t')) {
            q += 1;
        }
        match bytes.get(q) {
            None => Some(q),
            Some(b'\n') => Some(q + 1),
            _ => None,
        }
    };
    let end = match at_line_end(pos2) {
        Some(end) => end,
        // A title that is not at the end of its line is not a title, and the
        // definition may still be one without it.
        None => {
            title.take()?;
            at_line_end(before_title)?
        }
    };
    let label = normalize_label(raw_label);
    if label.is_empty() {
        return None;
    }
    refmap.insert(
        label,
        Reference {
            destination,
            title: title.unwrap_or_default(),
        },
    );
    Some(end)
}

/// The link reference definitions at the start of a paragraph's content,
/// recorded and removed (§4.7), and what is left.
///
/// One cut at the end rather than one per definition: a paragraph of a hundred
/// thousand definitions would otherwise copy its remainder a hundred thousand
/// times.
fn strip_references(content: String, refmap: &mut RefMap) -> String {
    let mut at = 0;
    while content.get(at..).is_some_and(|rest| rest.starts_with('[')) {
        match content
            .get(at..)
            .and_then(|rest| parse_reference(rest, refmap))
        {
            Some(used) if used > 0 => at += used,
            _ => break,
        }
    }
    content.get(at..).unwrap_or("").to_owned()
}

fn spnl_at(bytes: &[u8], mut pos: usize) -> usize {
    while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
        pos += 1;
    }
    if bytes.get(pos) == Some(&b'\n') {
        pos += 1;
        while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
            pos += 1;
        }
    }
    pos
}

/// §2.5's entity and numeric character references at the start of `rest`:
/// what one decodes to and how long it is.
fn entity(rest: &str) -> Option<(String, usize)> {
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'&') {
        return None;
    }
    if bytes.get(1) == Some(&b'#') {
        let (radix, digits_at) = match bytes.get(2) {
            Some(b'x' | b'X') => (16, 3),
            _ => (10, 2),
        };
        let max = if radix == 16 { 6 } else { 7 };
        let digits = bytes
            .get(digits_at..)?
            .iter()
            .take_while(|b| {
                if radix == 16 {
                    b.is_ascii_hexdigit()
                } else {
                    b.is_ascii_digit()
                }
            })
            .count();
        if digits == 0 || digits > max || bytes.get(digits_at + digits) != Some(&b';') {
            return None;
        }
        let value = u32::from_str_radix(rest.get(digits_at..digits_at + digits)?, radix).ok()?;
        let c = match char::from_u32(value) {
            Some('\0') | None => '\u{FFFD}',
            Some(c) => c,
        };
        return Some((c.to_string(), digits_at + digits + 1));
    }
    let name_len = bytes
        .get(1..)?
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    if !(2..=32).contains(&name_len) || bytes.get(1 + name_len) != Some(&b';') {
        return None;
    }
    let name = rest.get(1..1 + name_len)?;
    let c = tinker_pdf_xml::xhtml_entity(name)?;
    Some((c.to_string(), name_len + 2))
}

/// Backslash escapes and entity references resolved, as in a link
/// destination, a title and an info string.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < text.len() {
        let b = bytes[at];
        if b == b'\\' && bytes.get(at + 1).is_some_and(|c| is_escapable(*c)) {
            out.push(bytes[at + 1] as char);
            at += 2;
            continue;
        }
        if b == b'&' {
            if let Some((decoded, len)) = text.get(at..).and_then(entity) {
                out.push_str(&decoded);
                at += len;
                continue;
            }
        }
        let c = text
            .get(at..)
            .and_then(|r| r.chars().next())
            .unwrap_or('\u{FFFD}');
        out.push(c);
        at += c.len_utf8();
    }
    out
}

/// A destination as an `href`: what RFC 3986 allows unescaped kept, a valid
/// `%XX` kept, and everything else percent-encoded as UTF-8 — the
/// normalisation the specification's examples print.
fn normalize_uri(uri: &str) -> String {
    let mut out = String::with_capacity(uri.len());
    let bytes = uri.as_bytes();
    let mut at = 0;
    while let Some(&b) = bytes.get(at) {
        let keep = b.is_ascii_alphanumeric() || b";/?:@&=+$,-_.!~*'()#".contains(&b);
        if keep {
            out.push(b as char);
        } else if b == b'%'
            && bytes.get(at + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(at + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            out.push('%');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        at += 1;
    }
    out
}

// ---- rendering ---------------------------------------------------------------

/// How raw HTML is rendered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RawHtml {
    /// Passed through, as CommonMark specifies.
    Pass,
    /// Escaped as text, for the document path.
    Text,
}

/// Counts of what translating a document had to do.
#[derive(Default)]
pub(crate) struct Defects {
    counts: Vec<(TranslationDefect, usize)>,
}

impl Defects {
    pub(crate) fn note(&mut self, defect: TranslationDefect) {
        match self.counts.iter_mut().find(|(seen, _)| *seen == defect) {
            Some(slot) => slot.1 += 1,
            None => self.counts.push((defect, 1)),
        }
    }

    pub(crate) fn into_counts(self) -> Vec<(TranslationDefect, usize)> {
        self.counts
    }
}

fn escape_into(out: &mut String, text: &str, xml: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 §2.2's Char production, for the document path.
            '\u{0}'..='\u{8}'
            | '\u{B}'
            | '\u{C}'
            | '\u{E}'..='\u{1F}'
            | '\u{FFFE}'
            | '\u{FFFF}'
                if xml =>
            {
                out.push('\u{FFFD}');
            }
            _ => out.push(c),
        }
    }
}

fn cr(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// The deepest an element of [`to_xhtml`]'s document may nest.
///
/// Not a cap of its own but the bound [`MAX_MARKDOWN_NESTING`]'s argument
/// promises, enforced: the XHTML wrapper's two elements and two for each
/// container. Blocks cannot reach it — the nesting cap holds them to 99
/// containers, one element each, under a leaf of at most two — so it binds
/// inlines, which CommonMark nests as deep as their delimiters go: three
/// hundred `*` around a word are three hundred `<em>`, and past
/// `tinker_pdf_xml`'s 256 the reader stops and every block after the nest is
/// lost. An emphasis, strong emphasis or link that would nest its element past
/// this is set without the element, its text kept, and counted as
/// [`TranslationDefect::NestingTooDeep`]. The margin under 256 is the layout's
/// own `MAX_BOX_DEPTH`, also 256, which a box tree reaches a few levels deeper
/// than its elements through anonymous boxes.
const MAX_XHTML_DEPTH: usize = 2 * MAX_MARKDOWN_NESTING + 2;

struct Renderer<'a> {
    out: String,
    raw: RawHtml,
    defects: &'a mut Defects,
    /// Elements open around what is being written, the document's wrapper
    /// included.
    depth: usize,
    /// The most `depth` may reach: [`MAX_XHTML_DEPTH`] on the document path,
    /// and unbounded for [`to_html`], which is CommonMark's output exactly.
    ceiling: usize,
}

impl Renderer<'_> {
    fn esc(&mut self, text: &str) {
        let xml = self.raw == RawHtml::Text;
        escape_into(&mut self.out, text, xml);
    }

    fn raw_html(&mut self, text: &str) {
        match self.raw {
            RawHtml::Pass => self.out.push_str(text),
            RawHtml::Text => {
                self.defects.note(TranslationDefect::RawHtmlAsText);
                self.esc(text);
            }
        }
    }

    /// The block tree, depth first, with an explicit stack: a document is
    /// allowed to nest as deep as [`MAX_MARKDOWN_NESTING`] and its inlines
    /// deeper, and a recursive walk would make the depth a stack size.
    fn blocks(&mut self, tree: &Tree<Block>) {
        let mut stack: Vec<(usize, bool)> = vec![(0, true)];
        while let Some((at, entering)) = stack.pop() {
            let node = &tree.nodes[at];
            let block = &node.kind;
            let tight = node.parent.and_then(|p| tree.nodes[p].parent).is_some_and(
                |gp| matches!(tree.nodes[gp].kind.kind, BlockKind::List(data) if data.tight),
            );
            if entering {
                match &block.kind {
                    BlockKind::Document => {}
                    // A container is one element. The nesting cap holds blocks
                    // far under `ceiling` (see `MAX_XHTML_DEPTH`), so only the
                    // depth is kept here and inlines are what it binds.
                    BlockKind::BlockQuote => {
                        cr(&mut self.out);
                        self.out.push_str("<blockquote>");
                        cr(&mut self.out);
                        self.depth += 1;
                    }
                    BlockKind::List(data) => {
                        cr(&mut self.out);
                        match data.kind {
                            ListKind::Bullet(_) => self.out.push_str("<ul>"),
                            ListKind::Ordered(_) if data.start != 1 => {
                                self.out.push_str(&format!("<ol start=\"{}\">", data.start));
                            }
                            ListKind::Ordered(_) => self.out.push_str("<ol>"),
                        }
                        cr(&mut self.out);
                        self.depth += 1;
                    }
                    BlockKind::Item(_) => {
                        self.out.push_str("<li>");
                        self.depth += 1;
                    }
                    BlockKind::Paragraph => {
                        if !tight {
                            cr(&mut self.out);
                            self.out.push_str("<p>");
                            self.depth += 1;
                        }
                        if let Some(inlines) = &block.inlines {
                            self.inlines(inlines);
                        }
                        if !tight {
                            self.out.push_str("</p>");
                            cr(&mut self.out);
                            self.depth = self.depth.saturating_sub(1);
                        }
                        continue;
                    }
                    BlockKind::Heading(level) => {
                        cr(&mut self.out);
                        self.out.push_str(&format!("<h{level}>"));
                        self.depth += 1;
                        if let Some(inlines) = &block.inlines {
                            self.inlines(inlines);
                        }
                        self.depth = self.depth.saturating_sub(1);
                        self.out.push_str(&format!("</h{level}>"));
                        cr(&mut self.out);
                        continue;
                    }
                    BlockKind::ThematicBreak => {
                        cr(&mut self.out);
                        self.out.push_str("<hr />");
                        cr(&mut self.out);
                        continue;
                    }
                    BlockKind::Code { info, literal, .. } => {
                        cr(&mut self.out);
                        self.out.push_str("<pre><code");
                        let word = info.split([' ', '\t', '\n', '\r']).next().unwrap_or("");
                        if !word.is_empty() {
                            self.out.push_str(" class=\"language-");
                            self.esc(word);
                            self.out.push('"');
                        }
                        self.out.push('>');
                        self.esc(literal);
                        self.out.push_str("</code></pre>");
                        cr(&mut self.out);
                        continue;
                    }
                    BlockKind::Html { literal, .. } => {
                        cr(&mut self.out);
                        if self.raw == RawHtml::Text {
                            // As a paragraph of its own source, so the text
                            // lands in a block of its own.
                            self.out.push_str("<p>");
                            self.raw_html(literal);
                            self.out.push_str("</p>");
                        } else {
                            self.out.push_str(literal);
                        }
                        cr(&mut self.out);
                        continue;
                    }
                }
                stack.push((at, false));
                let mut children = Vec::new();
                let mut child = node.first;
                while let Some(c) = child {
                    children.push(c);
                    child = tree.nodes[c].next;
                }
                for c in children.into_iter().rev() {
                    stack.push((c, true));
                }
            } else {
                match &block.kind {
                    BlockKind::BlockQuote => {
                        cr(&mut self.out);
                        self.out.push_str("</blockquote>");
                        cr(&mut self.out);
                        self.depth = self.depth.saturating_sub(1);
                    }
                    BlockKind::List(data) => {
                        cr(&mut self.out);
                        self.out.push_str(match data.kind {
                            ListKind::Bullet(_) => "</ul>",
                            ListKind::Ordered(_) => "</ol>",
                        });
                        cr(&mut self.out);
                        self.depth = self.depth.saturating_sub(1);
                    }
                    BlockKind::Item(_) => {
                        self.out.push_str("</li>");
                        cr(&mut self.out);
                        self.depth = self.depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
        }
    }

    fn inlines(&mut self, tree: &Tree<Inline>) {
        // `images` counts the images the walk is inside: an image's
        // description is its `alt` text, so nothing inside one is a tag.
        let mut images = 0usize;
        // Whether each emphasis, strong emphasis and link the walk is inside
        // wrote its element: one inside an image writes none, and one past
        // `ceiling` is set without it.
        let mut opened: Vec<bool> = Vec::new();
        let mut stack: Vec<(usize, bool)> = Vec::new();
        let mut child = tree.nodes[0].first;
        let mut roots = Vec::new();
        while let Some(c) = child {
            roots.push(c);
            child = tree.nodes[c].next;
        }
        for c in roots.into_iter().rev() {
            stack.push((c, true));
        }
        while let Some((at, entering)) = stack.pop() {
            let node = &tree.nodes[at];
            if entering {
                match &node.kind {
                    Inline::Root => {}
                    Inline::Text(text) => self.esc(text),
                    Inline::SoftBreak => self.out.push('\n'),
                    Inline::LineBreak => {
                        if images == 0 {
                            self.out.push_str("<br />\n");
                        } else {
                            self.out.push('\n');
                        }
                    }
                    Inline::Code(code) => {
                        if images == 0 {
                            self.out.push_str("<code>");
                        }
                        self.esc(code);
                        if images == 0 {
                            self.out.push_str("</code>");
                        }
                    }
                    Inline::Html(raw) => {
                        if images == 0 {
                            self.raw_html(raw);
                        }
                    }
                    Inline::Emph | Inline::Strong | Inline::Link { .. } => {
                        // Room for this element and a leaf inside it — a
                        // `<code>`, an `<img />` or a `<br />` — which is what
                        // lets a leaf never ask.
                        let room = self.depth + 2 <= self.ceiling;
                        if images == 0 && !room {
                            self.defects.note(TranslationDefect::NestingTooDeep);
                        }
                        let write = images == 0 && room;
                        opened.push(write);
                        if write {
                            self.depth += 1;
                            match &node.kind {
                                Inline::Emph => self.out.push_str("<em>"),
                                Inline::Strong => self.out.push_str("<strong>"),
                                Inline::Link { destination, title } => {
                                    self.out.push_str("<a href=\"");
                                    self.esc(destination);
                                    self.out.push('"');
                                    if !title.is_empty() {
                                        self.out.push_str(" title=\"");
                                        self.esc(title);
                                        self.out.push('"');
                                    }
                                    self.out.push('>');
                                }
                                _ => {}
                            }
                        }
                    }
                    Inline::Image { destination, .. } => {
                        if images == 0 {
                            self.out.push_str("<img src=\"");
                            self.esc(destination);
                            self.out.push_str("\" alt=\"");
                        }
                        images += 1;
                    }
                }
                if matches!(
                    node.kind,
                    Inline::Emph | Inline::Strong | Inline::Link { .. } | Inline::Image { .. }
                ) {
                    stack.push((at, false));
                    let mut children = Vec::new();
                    let mut child = node.first;
                    while let Some(c) = child {
                        children.push(c);
                        child = tree.nodes[c].next;
                    }
                    for c in children.into_iter().rev() {
                        stack.push((c, true));
                    }
                }
            } else {
                match &node.kind {
                    Inline::Emph | Inline::Strong | Inline::Link { .. } => {
                        // Closed in the order they opened, so the last entry
                        // is this node's.
                        if opened.pop() == Some(true) {
                            self.depth = self.depth.saturating_sub(1);
                            self.out.push_str(match node.kind {
                                Inline::Emph => "</em>",
                                Inline::Strong => "</strong>",
                                _ => "</a>",
                            });
                        }
                    }
                    Inline::Image { title, .. } => {
                        images -= 1;
                        if images == 0 {
                            if !title.is_empty() {
                                self.out.push_str("\" title=\"");
                                self.esc(title);
                            }
                            self.out.push_str("\" />");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Phases 1 and 2 over a whole document.
fn parse(text: &str, defects: &mut Defects) -> Parser {
    let mut parser = Parser::new();
    // Line endings are LF, CR or CRLF (§2.1), and a final one ends the last
    // line rather than starting another.
    let mut lines: Vec<&str> = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => {
                lines.push(text.get(start..at).unwrap_or(""));
                at += 1;
                start = at;
            }
            b'\r' => {
                lines.push(text.get(start..at).unwrap_or(""));
                at += 1;
                if bytes.get(at) == Some(&b'\n') {
                    at += 1;
                }
                start = at;
            }
            _ => at += 1,
        }
    }
    if start < bytes.len() {
        lines.push(text.get(start..).unwrap_or(""));
    }
    for line in &lines {
        parser.incorporate_line(line, defects);
    }
    let total = lines.len();
    loop {
        let tip = parser.tip;
        parser.finalize(tip, total);
        if tip == 0 {
            break;
        }
    }
    // Phase 2, over every paragraph and heading still in the tree.
    let mut stack = vec![0usize];
    let mut leaves = Vec::new();
    while let Some(at) = stack.pop() {
        let node = &parser.tree.nodes[at];
        if matches!(node.kind.kind, BlockKind::Paragraph | BlockKind::Heading(_)) {
            leaves.push(at);
        }
        let mut child = node.first;
        while let Some(c) = child {
            stack.push(c);
            child = parser.tree.nodes[c].next;
        }
    }
    let mut budget = text.len().max(MAX_MARKDOWN_REFERENCE_BYTES);
    for at in leaves {
        let content = std::mem::take(&mut parser.tree.nodes[at].kind.content);
        let subject = content.trim_matches(is_ascii_ws);
        let inlines = InlineParser::new(subject, &parser.refmap, &mut budget, defects).parse();
        parser.tree.nodes[at].kind.inlines = Some(inlines);
    }
    parser
}

/// A Markdown document as CommonMark 0.31.2's HTML, in the form its examples
/// print — raw HTML passed through, every block on its own line.
///
/// This is the half held to the specification's own examples; see the module
/// comment for what is not CommonMark here.
#[must_use]
pub fn to_html(text: &str) -> String {
    let mut defects = Defects::default();
    let parser = parse(text, &mut defects);
    let mut renderer = Renderer {
        out: String::new(),
        raw: RawHtml::Pass,
        defects: &mut defects,
        depth: 0,
        ceiling: usize::MAX,
    };
    renderer.blocks(&parser.tree);
    renderer.out
}

/// A Markdown document as a whole XHTML document the EPUB reader can read,
/// and what had to be done to make it one.
///
/// The body is [`to_html`]'s with raw HTML set as text and XML's forbidden
/// characters replaced; the title is the first heading's text, when there is
/// one, so the document has the `/Title` its own first line names.
#[must_use]
pub fn to_xhtml(text: &str) -> (String, Vec<(TranslationDefect, usize)>) {
    let mut defects = Defects::default();
    let parser = parse(text, &mut defects);
    let mut renderer = Renderer {
        out: String::new(),
        raw: RawHtml::Text,
        defects: &mut defects,
        // `<html>` and `<body>`, written around the body below.
        depth: 2,
        ceiling: MAX_XHTML_DEPTH,
    };
    renderer.blocks(&parser.tree);
    let body = std::mem::take(&mut renderer.out);
    let title = first_heading_text(&parser.tree);
    let mut document = String::with_capacity(body.len() + 256);
    document.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>",
    );
    escape_into(&mut document, &title, true);
    document.push_str("</title></head><body>\n");
    document.push_str(&body);
    document.push_str("</body></html>\n");
    (document, defects.into_counts())
}

/// The plain text of the document's first heading, or empty.
fn first_heading_text(tree: &Tree<Block>) -> String {
    let mut stack = vec![0usize];
    while let Some(at) = stack.pop() {
        let node = &tree.nodes[at];
        if matches!(node.kind.kind, BlockKind::Heading(_)) {
            let mut text = String::new();
            if let Some(inlines) = &node.kind.inlines {
                // The tree from its root and not the arena in push order: the
                // bracket pass leaves the `[` and `![` text nodes of every
                // link and image it made in the arena, unlinked, so only a
                // walk reads what the heading shows.
                let mut walk = vec![0usize];
                while let Some(at) = walk.pop() {
                    let Some(inline) = inlines.nodes.get(at) else {
                        continue;
                    };
                    match &inline.kind {
                        Inline::Text(t) | Inline::Code(t) => text.push_str(t),
                        Inline::SoftBreak | Inline::LineBreak => text.push(' '),
                        _ => {}
                    }
                    let mut children = Vec::new();
                    let mut child = inline.first;
                    while let Some(c) = child {
                        children.push(c);
                        child = inlines.nodes.get(c).and_then(|n| n.next);
                    }
                    walk.extend(children.into_iter().rev());
                }
            }
            return text.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        let mut children = Vec::new();
        let mut child = node.first;
        while let Some(c) = child {
            children.push(c);
            child = tree.nodes[c].next;
        }
        stack.extend(children.into_iter().rev());
    }
    String::new()
}
