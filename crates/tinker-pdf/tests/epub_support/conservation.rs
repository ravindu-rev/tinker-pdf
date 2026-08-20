//! Text conservation: the one invariant that survives every level of CSS
//! partiality (gap 31, milestone 4).
//!
//! > **Every character of text in every content document in the spine appears
//! > exactly once in the paginated output, in document order.**
//!
//! Gap 31's plan calls this the fourth of its five honesty devices and the one
//! that makes an *intermediate* state honest. It is scheduled **here**, with
//! the placeholders, rather than with the first real layout, and that is
//! deliberate: built at milestone 8 it would be a test written to pass; built
//! at milestone 4 against grey pages it is a test written before there is
//! anything to make it pass, and nine later milestones inherit it as a
//! constraint rather than acquire it as a formality.
//!
//! # What it catches that a rendered comparison cannot
//!
//! Every one of these produces a book that renders beautifully:
//!
//! - a float that pushed content off the page bottom and lost it — **missing**;
//! - a `display: none` honoured where it should not have been — **missing**;
//! - a `display: none` *not* honoured — **extra**;
//! - a fragmentation bug that repeated a paragraph across a page break —
//!   **extra**, then the repeat resynchronises;
//! - a spine item whose XHTML failed to parse and was skipped — **a whole
//!   chapter missing**, which is gap 29's page-renumbering hazard at book
//!   scale.
//!
//! # Why the source side does not use this repository's own reader
//!
//! [`spine_text`] walks `META-INF/container.xml`, the package document and the
//! content documents with its own crude scanners, out of bytes, exactly as
//! milestone 1's censuses do. Using `epub::package::parse` would be shorter and
//! would make the harness blind to the thing it is for: **the spine's order is
//! one of the claims being checked**, and an oracle that asks the engine which
//! order the engine chose has checked nothing. The scanners are the crudest
//! thing that can answer the question and each is tested on its own in
//! `tests/epub_conservation.rs`, which is milestone 1's finding about its own
//! scanners applied before it can bite again.
//!
//! # Whitespace is not conservable, and saying so is part of the definition
//!
//! Layout reflows white space by construction: `css-text-3` §4.1.1 collapses
//! runs of it, a line break replaces a space, and an indented source is
//! indistinguishable from an unindented one once it is set. So the conserved
//! stream is the stream of **non-whitespace characters**, which is the largest
//! stream that can survive a layout engine at all. Every other character is
//! compared exactly, in order, and duplication is an error.

use tinker_pdf::Document;
use tinker_pdf_zip::{Archive, Limits};

// ---- the source side --------------------------------------------------------

/// One spine item's text, as the book wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpineSource {
    /// The container path the content document was read from, or the `idref`
    /// that named nothing.
    pub path: String,
    /// Its characters, markup removed, whitespace kept as written.
    pub text: String,
}

/// Every spine item's text, in reading order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpineText {
    /// In spine order, one per `<itemref>`.
    pub items: Vec<SpineSource>,
}

impl SpineText {
    /// Every conservable character of the whole book, concatenated in reading
    /// order.
    #[must_use]
    pub fn conservable(&self) -> Vec<char> {
        self.items
            .iter()
            .flat_map(|item| item.text.chars())
            .filter(|c| !c.is_whitespace())
            .collect()
    }
}

/// Reads a book's spine and every content document in it, out of the archive's
/// own bytes.
///
/// A spine item that resolves to nothing contributes an **empty** entry rather
/// than no entry, so the item count is the spine's length whatever the book
/// gets wrong — which is what lets a caller assert the position of a chapter
/// as well as its text.
#[must_use]
pub fn spine_text(book: &[u8]) -> SpineText {
    let mut archive = match Archive::open(book, &Limits::DEFAULT) {
        Ok(archive) => archive,
        Err(_) => return SpineText::default(),
    };
    let read = |archive: &mut Archive<'_>, path: &str| -> Option<String> {
        let index = archive.entries().iter().position(|e| e.name == path)?;
        let bytes = archive.read(index).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };

    let Some(container) = read(&mut archive, "META-INF/container.xml") else {
        return SpineText::default();
    };
    let Some(opf_path) = attribute_of(&container, "rootfile", "full-path") else {
        return SpineText::default();
    };
    let Some(opf) = read(&mut archive, &opf_path) else {
        return SpineText::default();
    };

    let items = manifest_items(&opf);
    let mut out = SpineText::default();
    for idref in spine_idrefs(&opf) {
        let path = items
            .iter()
            .find(|(id, _)| *id == idref)
            .map(|(_, href)| join(&opf_path, href));
        let text = path
            .as_deref()
            .and_then(|path| read(&mut archive, path))
            .map(|markup| visible_text(&markup))
            .unwrap_or_default();
        out.items.push(SpineSource {
            path: path.unwrap_or(idref),
            text,
        });
    }
    out
}

/// Every `<item>`'s `id` and `href`, in document order.
///
/// Crude on purpose: it finds each `<item` and reads two attributes out of it.
/// It cannot tell an `<item>` in the manifest from one somewhere else, and no
/// package document in either corpus has one anywhere else.
#[must_use]
pub fn manifest_items(opf: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for tag in tags_named(opf, "item") {
        let (Some(id), Some(href)) = (attribute(&tag, "id"), attribute(&tag, "href")) else {
            continue;
        };
        out.push((id, href));
    }
    out
}

/// Every `<itemref>`'s `idref`, in document order.
#[must_use]
pub fn spine_idrefs(opf: &str) -> Vec<String> {
    tags_named(opf, "itemref")
        .iter()
        .filter_map(|tag| attribute(tag, "idref"))
        .collect()
}

/// Every start tag whose local name is `name`, as its raw text.
///
/// `<item` and `<itemref` share a prefix, so the character after the name has
/// to be a delimiter — the bug a `starts_with` would have, and the one
/// milestone 1's doctype scanner was injected with.
fn tags_named(markup: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = markup.as_bytes();
    let mut at = 0usize;
    while let Some(found) = markup[at..].find('<') {
        let start = at + found;
        let mut end = start + 1;
        while end < bytes.len() && bytes[end] != b'>' {
            end += 1;
        }
        let tag = &markup[start..end.min(bytes.len())];
        at = end.min(bytes.len()).max(start + 1);
        let Some(rest) = tag.strip_prefix('<') else {
            continue;
        };
        // The **element name** is what carries a prefix, and it ends at the
        // first white space or `/`. Splitting the whole tag on `:` instead
        // reads `properties="svg calibre:title-page"` as the name, which is
        // real: calibre writes exactly that on the first manifest item of every
        // EPUB 3 it produces, and the first run of this harness lost that item.
        let qualified: &str = rest
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap_or("");
        let local = qualified.split(':').next_back().unwrap_or("");
        if local == name && !rest.starts_with('/') {
            out.push(tag.to_owned());
        }
    }
    out
}

/// The value of an attribute in one start tag, single or double quoted.
#[must_use]
pub fn attribute(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut at = 0usize;
    while let Some(found) = tag[at..].find(name) {
        let start = at + found;
        at = start + name.len();
        // The character before must be a delimiter, or `href` would match
        // inside `xhref`; the one after must be `=` or white space, or `id`
        // would match inside `idref`.
        let before_ok = start == 0
            || bytes
                .get(start - 1)
                .is_some_and(|b| b.is_ascii_whitespace() || *b == b'<');
        let mut after = at;
        while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
            after += 1;
        }
        if !before_ok || bytes.get(after) != Some(&b'=') {
            continue;
        }
        after += 1;
        while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
            after += 1;
        }
        let quote = *bytes.get(after)?;
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        let value_start = after + 1;
        let end = tag[value_start..].find(quote as char)? + value_start;
        return Some(decode_entities(&tag[value_start..end]));
    }
    None
}

/// The first `name` element's attribute anywhere in a document.
#[must_use]
pub fn attribute_of(markup: &str, element: &str, name: &str) -> Option<String> {
    tags_named(markup, element)
        .iter()
        .find_map(|tag| attribute(tag, name))
}

/// An `href` joined onto the path of the document that wrote it.
///
/// RFC 3986 §5.2.3's merge and §5.2.4's dot-segment removal, the crude version:
/// enough for the twenty-six books in the two corpora and for every fixture,
/// and deliberately not the engine's own [`tinker_pdf::epub::ocf`] resolver,
/// for the reason in this module's header.
#[must_use]
pub fn join(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let cut = base.rfind('/').map_or(0, |at| at + 1);
    let merged = format!("{}{href}", &base[..cut]);
    let mut segments: Vec<&str> = Vec::new();
    for segment in merged.split('/') {
        match segment {
            "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// The visible characters of an XHTML document: markup removed, `<head>`,
/// `<script>`, `<style>` and `[hidden]` contents dropped, references decoded.
///
/// # The four removals, and where the fourth came from
///
/// HTML's rendering section names exactly four things that generate no box at
/// all for an unstyled document, and this scanner implements the same four out
/// of the bytes. `<head>` goes because its `<title>` is chrome rather than the
/// book's text — no reading system sets it into the flow — and `<script>` and
/// `<style>` go because CSS and JavaScript are not text either.
///
/// **`[hidden]` is the fourth and it arrived at milestone 8**, which is the
/// first milestone at which any of the four could be observed: until there was
/// a user-agent stylesheet there was nothing on the engine's side that removed
/// anything, so the list here was a list of three and nothing could tell.
/// Three of the six committed books carry a `<nav … hidden="hidden">` — the
/// landmarks list every pandoc book writes — and honouring HTML §15.3.1's own
/// `[hidden] { display: none }` takes twenty-four to twenty-nine characters out
/// of each of them.
///
/// The alternative was to leave the engine not honouring `hidden`, which would
/// have meant a book that sets text every reading system and every browser
/// hides — including the browser this milestone compares `y` offsets against.
/// So the definition of the conserved stream is what was wrong, and it is the
/// definition that is corrected. **This scanner still asks nothing of the
/// engine**: it reads the attribute out of the bytes with its own crude parser,
/// exactly as it reads `<head>`, and `the_source_side_drops_hidden_and_keeps
/// _everything_else` asserts it in both directions.
///
/// A build that included any of the four would report *extra* text on every
/// page of every book, and one that dropped more would report *missing* — both
/// directions fail loudly rather than quietly.
#[must_use]
pub fn visible_text(markup: &str) -> String {
    let bytes = markup.as_bytes();
    let mut out = String::with_capacity(markup.len());
    let mut at = 0usize;
    // The element being skipped and how many **nested copies of the same
    // name** are open inside it. A counter and not a flag: a `<div hidden>`
    // holding a `<div>` would otherwise end at the inner one's `</div>` and
    // let the rest of the hidden subtree through, which is a defect no fixture
    // written from the corpus would show because no book here nests one.
    let mut skipping: Option<(String, usize)> = None;
    while at < bytes.len() {
        if bytes[at] != b'<' {
            // **The character, not the byte.** `markup` is already decoded, so
            // pushing `bytes[at] as char` reads UTF-8 as Latin-1 and turns one
            // em dash into three characters. Milestone 8 found it: until pages
            // carried text there was nothing for the mangled stream to
            // disagree with, and the corpus sweep compared 0 against 0 and
            // passed. `the_source_side_decodes_and_does_not_transliterate` is
            // the assertion that would have caught it at milestone 4.
            let ch = markup[at..]
                .chars()
                .next()
                .unwrap_or(char::REPLACEMENT_CHARACTER);
            if skipping.is_none() {
                out.push(ch);
            }
            at += ch.len_utf8();
            continue;
        }
        // A comment, a CDATA section, a doctype or a processing instruction.
        if markup[at..].starts_with("<!--") {
            at = markup[at..].find("-->").map_or(bytes.len(), |e| at + e + 3);
            continue;
        }
        if markup[at..].starts_with("<![CDATA[") {
            let end = markup[at..].find("]]>").map_or(bytes.len(), |e| at + e);
            if skipping.is_none() {
                out.push_str(&markup[at + 9..end]);
            }
            at = (end + 3).min(bytes.len());
            continue;
        }
        let end = markup[at..].find('>').map_or(bytes.len(), |e| at + e + 1);
        let tag = &markup[at..end];
        at = end;
        if tag.starts_with("<!") || tag.starts_with("<?") {
            continue;
        }
        let closing = tag.starts_with("</");
        let name: String = tag
            .trim_start_matches('<')
            .trim_start_matches('/')
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/')
            .collect();
        let local = name
            .split(':')
            .next_back()
            .unwrap_or("")
            .to_ascii_lowercase();
        match &mut skipping {
            Some((open, depth)) => {
                if local != *open {
                    // Not this element's name at all, so it changes nothing
                    // about where the skip ends.
                } else if closing {
                    if *depth == 0 {
                        skipping = None;
                    } else {
                        *depth -= 1;
                    }
                } else if !tag.ends_with("/>") {
                    *depth += 1;
                }
            }
            None => {
                let removed = matches!(local.as_str(), "head" | "script" | "style")
                    || has_hidden_attribute(tag);
                if !closing && !tag.ends_with("/>") && removed {
                    skipping = Some((local, 0));
                } else {
                    // A tag boundary is a place two words may not run together:
                    // `<p>a</p><p>b</p>` is two words and `ab` is one.
                    out.push(' ');
                }
            }
        }
    }
    decode_entities(&out)
}

/// Whether a start tag carries HTML's `hidden` attribute.
///
/// Written as a scan for the **name** rather than for `hidden="`, because the
/// attribute is a boolean one and all three of `hidden`, `hidden=""` and
/// `hidden="hidden"` mean the same thing — pandoc writes the third and a
/// hand-written book writes the first. The scan refuses a name that is only a
/// suffix of another (`aria-hidden`, which is on three `<figcaption>`s in this
/// corpus and hides nothing), by requiring the character before it to be white
/// space and the character after it to end the name.
#[must_use]
pub fn has_hidden_attribute(tag: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0usize;
    while let Some(at) = lower[from..].find("hidden") {
        let start = from + at;
        let end = start + "hidden".len();
        let before = start.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(end).copied();
        let opens = matches!(before, Some(b) if b.is_ascii_whitespace());
        let closes = matches!(after, None | Some(b'=') | Some(b'>') | Some(b'/'))
            || matches!(after, Some(b) if b.is_ascii_whitespace());
        if opens && closes {
            return true;
        }
        from = end;
    }
    false
}

/// XML 1.0's five predefined entities and both numeric forms.
///
/// Milestone 1's census found **zero** named references outside the five across
/// all 270 content documents in both corpora, which is why there is no table
/// here and why milestone 2 refuses one by name. An undeclared name is left as
/// written, so a book that used one would report it as text rather than
/// silently dropping characters.
#[must_use]
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(end) = tail.find(';').filter(|e| *e <= 12) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => numeric(name),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn numeric(name: &str) -> Option<char> {
    let digits = name.strip_prefix('#')?;
    let value = match digits.strip_prefix(['x', 'X']) {
        Some(hex) if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) => {
            u32::from_str_radix(hex, 16).ok()?
        }
        Some(_) => return None,
        None if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
            digits.parse().ok()?
        }
        None => return None,
    };
    char::from_u32(value)
}

// ---- the paginated side ------------------------------------------------------

/// Every page's text, in page order, through the surface a host uses.
///
/// `Page::text()` drives `TextDevice` over the same content stream `Page::render`
/// drives the rasteriser over, so this is what the page *says* rather than a
/// second opinion about it. Gap 31's design section is explicit that a second
/// producer for text would be a second layout engine.
#[must_use]
pub fn paginated_text(doc: &Document) -> Vec<String> {
    (0..doc.page_count())
        .map(|at| doc.page(at).expect("a page in range").text().plain_text())
        .collect()
}

// ---- the comparison ----------------------------------------------------------

/// Where a paginated book stopped agreeing with the book it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// Characters of the source that no page carries.
    Missing {
        /// Offset into the source's conservable stream.
        at: usize,
        /// What was skipped, truncated for a message.
        text: String,
    },
    /// Characters on a page that the source does not have there.
    ///
    /// A paragraph repeated across a page break is this, and so is text a
    /// `display: none` should have removed.
    Extra {
        /// Offset into the paginated conservable stream.
        at: usize,
        /// What appeared, truncated for a message.
        text: String,
    },
    /// The two streams stopped matching and neither resynchronised inside the
    /// window.
    Unrecoverable {
        /// Offset into the source's conservable stream.
        source_at: usize,
        /// Offset into the paginated conservable stream.
        page_at: usize,
    },
}

/// What the comparison found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Conservable characters the book holds.
    pub source: usize,
    /// Conservable characters the pages carry.
    pub paginated: usize,
    /// How many of the source's characters appear, in order, exactly once.
    pub conserved: usize,
    /// How many of the source's characters do not appear at all.
    pub missing: usize,
    /// How many characters appear on a page that the source does not have.
    pub extra: usize,
    /// Everything that went wrong, in the order it was met.
    pub divergences: Vec<Divergence>,
}

impl Verdict {
    /// Whether every character is conserved, exactly once, in order.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.missing == 0 && self.extra == 0 && self.conserved == self.source
    }

    /// The conservation figure, as a fraction of the book's own characters.
    ///
    /// Recorded per book rather than asserted as a boolean, because it is the
    /// number a partial build is judged on: at milestone 4 it is **0 of
    /// everything**, and each later milestone raises it.
    #[must_use]
    pub fn figure(&self) -> (usize, usize) {
        (self.conserved, self.source)
    }
}

/// How far apart the two streams may drift before the comparison gives up.
///
/// A window rather than a full edit-distance diff, because the source stream of
/// a real book is hundreds of thousands of characters and an O(n·m) table over
/// two of those is minutes of CPU per book. What it costs is that a divergence
/// longer than this reports [`Divergence::Unrecoverable`] rather than its own
/// size — and that is a defect nobody needs a finer measurement of.
pub const RESYNC_WINDOW: usize = 4_096;

/// How many characters must agree before a resynchronisation is believed.
///
/// One matching character after a divergence is a coincidence — English is
/// mostly twenty-six symbols — and a run of sixteen is a sentence fragment.
pub const RESYNC_RUN: usize = 16;

/// Compares a book's own characters against the ones its pages carry.
///
/// The walk is greedy with a bounded resynchronisation, which is the shape a
/// diff has to be at this size. Where the two streams disagree it looks ahead
/// in each for a run of [`RESYNC_RUN`] characters that puts them back together
/// inside [`RESYNC_WINDOW`]: finding it in the *source* means the pages lost
/// something, finding it in the *pages* means they gained something, and
/// finding neither means the comparison cannot say more than that they diverged.
#[must_use]
pub fn compare(source: &SpineText, pages: &[String]) -> Verdict {
    let s: Vec<char> = source.conservable();
    let p: Vec<char> = pages
        .iter()
        .flat_map(|page| page.chars())
        .filter(|c| !c.is_whitespace())
        .collect();

    let mut verdict = Verdict {
        source: s.len(),
        paginated: p.len(),
        conserved: 0,
        missing: 0,
        extra: 0,
        divergences: Vec::new(),
    };

    let (mut i, mut j) = (0usize, 0usize);
    while i < s.len() && j < p.len() {
        if s[i] == p[j] {
            verdict.conserved += 1;
            i += 1;
            j += 1;
            continue;
        }
        match resync(&s, i, &p, j) {
            Some(Shift::Source(d)) => {
                verdict.divergences.push(Divergence::Missing {
                    at: i,
                    text: s[i..i + d].iter().take(64).collect(),
                });
                verdict.missing += d;
                i += d;
            }
            Some(Shift::Pages(d)) => {
                verdict.divergences.push(Divergence::Extra {
                    at: j,
                    text: p[j..j + d].iter().take(64).collect(),
                });
                verdict.extra += d;
                j += d;
            }
            None => {
                verdict.divergences.push(Divergence::Unrecoverable {
                    source_at: i,
                    page_at: j,
                });
                break;
            }
        }
    }

    // Whatever is left over on either side is the same two facts as above.
    if i < s.len() {
        verdict.divergences.push(Divergence::Missing {
            at: i,
            text: s[i..].iter().take(64).collect(),
        });
        verdict.missing += s.len() - i;
    }
    if j < p.len() {
        verdict.divergences.push(Divergence::Extra {
            at: j,
            text: p[j..].iter().take(64).collect(),
        });
        verdict.extra += p.len() - j;
    }
    verdict
}

/// Which stream has to move for the two to agree again.
enum Shift {
    Source(usize),
    Pages(usize),
}

fn resync(s: &[char], i: usize, p: &[char], j: usize) -> Option<Shift> {
    for d in 1..=RESYNC_WINDOW {
        if i + d < s.len() && run_matches(s, i + d, p, j) {
            return Some(Shift::Source(d));
        }
        if j + d < p.len() && run_matches(s, i, p, j + d) {
            return Some(Shift::Pages(d));
        }
        if i + d >= s.len() && j + d >= p.len() {
            break;
        }
    }
    None
}

/// Whether the two streams agree for [`RESYNC_RUN`] characters, or to the end
/// of the shorter of them.
fn run_matches(s: &[char], i: usize, p: &[char], j: usize) -> bool {
    let run = RESYNC_RUN.min(s.len() - i).min(p.len() - j);
    if run == 0 {
        return false;
    }
    (0..run).all(|k| s[i + k] == p[j + k])
}

/// The whole harness in one call: read the book, page it, compare.
#[must_use]
pub fn conservation(book: &[u8], doc: &Document) -> Verdict {
    compare(&spine_text(book), &paginated_text(doc))
}
