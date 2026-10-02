//! Watermarks and stamps on existing pages: a resource registered on a page
//! that already has resources of its own, and a form XObject drawn over or
//! under what the page already draws.
//!
//! Three things make this harder than appending operators, and each has a
//! paragraph below: a page's `/Resources` may be inherited from the page tree
//! or shared by reference with other pages, so it is copied before it is
//! changed; the name a stamp is registered under must not be one the page
//! already uses; and the page's own content may leave the graphics state
//! changed, or a save, a text object or a marked-content sequence open, and a
//! stamp drawn after it would inherit the one and be drawn inside the other.

use std::collections::HashMap;

use super::DocumentEditor;
use crate::build::{all_finite, is_box, FormXObject};
use crate::doc::CosDocument;
use crate::lexer::{Lexer, TokenKind};
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages;
use crate::resolve::Resolve;
use crate::warn::WarningSink;
use crate::write::{StreamData, Written};

/// Where [`DocumentEditor::stamp`] draws a form relative to the page's own
/// content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StampPlacement {
    /// After the page's content, so the stamp paints **on top** of it: a
    /// "DRAFT" across the page, an approval mark.
    Over,
    /// Before the page's content, so the page paints on top of the stamp: a
    /// letterhead, a background, a watermark meant to sit behind text.
    Under,
}

/// Operators that leave the graphics state as they found it once they are
/// done (8.4.4, Table 57): path construction and painting, the text-object
/// operators whose effects end at `ET`, marked content, and the two that
/// paint something self-contained.
///
/// `Do` is here because 8.10.1 brackets a form's content in an implicit
/// `q`/`Q`, and an image changes no parameter at all; `sh` paints within the
/// clip and changes nothing either. Everything **not** here — `cm`, the
/// colour operators, `gs`, `w`, the clip operators `W` and `W*`, and the text
/// *state* operators (`Tf`, `Tc`, `Tw`, `Tz`, `TL`, `Tr`, `Ts`), whose values
/// outlive `ET` — is a change a stamp drawn afterwards would inherit. Two text
/// operators that look like positioning are among them, because Table 108
/// gives each a text-state side effect: `TD` is `-ty TL tx ty Td` and sets the
/// leading, and `"` sets the word and character spacing.
///
/// `q`, `Q`, `BT`, `ET`, `BMC`, `BDC` and `EMC` are counted rather than
/// listed: what matters about them is whether each is closed.
const NEUTRAL: [&[u8]; 27] = [
    b"m", b"l", b"c", b"v", b"y", b"h", b"re", b"S", b"s", b"f", b"F", b"f*", b"B", b"B*", b"b",
    b"b*", b"n", b"Td", b"Tm", b"T*", b"Tj", b"TJ", b"'", b"MP", b"DP", b"Do", b"sh",
];

/// Something a page's content opened and may leave open when it ends
/// (7.8.2: a page's content streams are one stream, so nothing closes at a
/// stream boundary).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opened {
    /// `q` (8.4.2), closed by `Q`.
    Save,
    /// `BT` (9.4.1), closed by `ET`.
    Text,
    /// `BMC` or `BDC` (14.6), closed by `EMC`.
    Marked,
}

impl Opened {
    const fn closer(self) -> &'static [u8] {
        match self {
            Opened::Save => b"Q",
            Opened::Text => b"ET",
            Opened::Marked => b"EMC",
        }
    }
}

/// What an over-stamp needs around a page's content to run in the page's
/// **initial** graphics state, outside every sequence the page opened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Bracket {
    /// A `q` before the page's content: the content changes the state
    /// outside any `q` of its own, so only a save taken before it can put
    /// the initial state back.
    open: bool,
    /// Operators after the page's content and before the stamp: a closer for
    /// everything the content left open, innermost first — `EMC` for a
    /// marked-content sequence, `ET` for a text object, `Q` for a save — and
    /// then the `Q` that matches [`Self::open`].
    close: Vec<&'static [u8]>,
}

impl Bracket {
    /// What content that cannot be read gets: one `q`/`Q` pair around it,
    /// which isolates the stamp exactly when the content's own `q` and `Q`
    /// balance and nothing is left open — the most that can be done without
    /// knowing what the content does.
    fn blind() -> Bracket {
        Bracket {
            open: true,
            close: vec![b"Q"],
        }
    }
}

impl DocumentEditor {
    /// Registers `object` in page `page`'s `/Resources /<category>` under a
    /// name the page does not already use, and returns that name.
    ///
    /// `category` is 7.8.3's sub-dictionary key — `XObject`, `Font`,
    /// `ExtGState`, `ColorSpace`, `Pattern`, `Shading`, `Properties` — and
    /// the name is `prefix` followed by the smallest number that makes it new
    /// in that sub-dictionary, so a stamp registered as `Stamp` on a page that
    /// already names a `Stamp0` becomes `Stamp1`.
    ///
    /// # Copy on write
    ///
    /// The page's resource dictionary may not be the page's own. 7.7.3.4 lets
    /// a page inherit `/Resources` from the page tree, and a producer may point
    /// several pages at one indirect dictionary. Changing either in place
    /// would register the resource on pages nobody asked about. So the
    /// **effective** dictionary — the page's own, or the nearest ancestor's —
    /// is copied, the category's sub-dictionary copied with it, and the copy
    /// written onto this page directly. The shared or inherited original is
    /// not touched, and an incremental save carries the page and nothing else
    /// of the tree.
    ///
    /// Returns `None`, changing nothing, for a page that does not exist or is
    /// not a dictionary, or an empty `category`.
    pub fn add_resource(
        &mut self,
        page: u32,
        category: &[u8],
        prefix: &[u8],
        object: Object,
    ) -> Option<Vec<u8>> {
        if category.is_empty() {
            return None;
        }
        let reference = self.page_refs().get(page as usize).copied()?;
        let Some(Object::Dict(mut dict)) = DocumentEditor::get(self, reference) else {
            return None;
        };
        let mut resources = self.effective_resources(&dict);
        let category_key = self.intern(category);
        let mut table = match resources.get(category_key) {
            Some(value) => Resolve::resolve(self, value)
                .as_dict()
                .cloned()
                .unwrap_or_default(),
            None => Dict::new(),
        };

        // A table of `len` entries can hold at most `len` of the candidates,
        // so one of the first `len + 1` is free.
        let mut name = None;
        for n in 0..=table.len() {
            let mut candidate = prefix.to_vec();
            candidate.extend_from_slice(n.to_string().as_bytes());
            if table.get(self.intern(&candidate)).is_none() {
                name = Some(candidate);
                break;
            }
        }
        let name = name?;
        table.insert(self.intern(&name), object);
        resources.insert(category_key, Object::Dict(table));
        dict.insert(Name::RESOURCES, Object::Dict(resources));
        self.put(reference, Object::Dict(dict));
        Some(name)
    }

    /// Draws the form XObject at `form` on page `page`, over or under what the
    /// page already draws, and returns the name it was registered under.
    ///
    /// The form is registered with [`DocumentEditor::add_resource`] — so the
    /// page's resources are copied before they are changed, and the name is
    /// new — and a content stream invoking it is added to the page's
    /// `/Contents` **array**. The page's own content streams are not rewritten
    /// and not copied: they stay the objects they were, byte for byte, which
    /// is what keeps an incremental save small and leaves a signature over
    /// them able to say the content it signed is still what the page draws.
    /// The form's own `/Matrix` places it; a caller wanting it elsewhere makes
    /// the form with a different matrix.
    ///
    /// # When the page's content is wrapped in `q` … `Q`
    ///
    /// A stamp drawn **over** the page runs in whatever graphics state the
    /// page's content left behind (8.4.2), and inside whatever it left open. A
    /// page ending with a `cm` outside any `q`/`Q` would scale and move the
    /// stamp; a clip would cut it; a `Tf` or a `TD` would change the text a
    /// stamp sets without choosing its own; an `/OC … BDC` never closed would
    /// hide the stamp with the layer. So the page's content is read first:
    ///
    /// - if every `q` has its `Q`, every `BT` its `ET` and every `BDC` or
    ///   `BMC` its `EMC`, and nothing outside a `q` changes the state — see
    ///   [`NEUTRAL`] — the stamp is simply appended;
    /// - otherwise the stamp's stream first closes what the content left
    ///   open, innermost first — an `EMC` per marked-content sequence, an
    ///   `ET` for a text object, a `Q` per save — and when the content changed
    ///   the state outside any `q` of its own, a new stream holding `q` goes
    ///   before the page's streams and one more `Q` after them. The stamp then
    ///   runs in the page's initial state, outside every sequence, and the
    ///   original streams are still untouched.
    ///
    /// An inline image's data is skipped to its `EI` as the interpreter skips
    /// it. Content that cannot be read at all — a stream that will not decode,
    /// or a `Q` with no `q` to restore, which readers disagree about — gets one
    /// `q` before and one `Q` after, and **that isolates the stamp only when
    /// the content's own operators balance**: nothing more can be said about
    /// content nothing here can follow.
    ///
    /// A stamp **under** the page needs no bracket at all: it runs first, in
    /// the page's initial state, and `Do` restores whatever the form changes
    /// (8.10.1) before the page's content starts.
    ///
    /// Returns `None`, changing nothing, for a page that does not exist or a
    /// `form` that is not a form XObject stream.
    pub fn stamp(&mut self, page: u32, form: ObjRef, placement: StampPlacement) -> Option<Vec<u8>> {
        if !self.is_form(form) {
            return None;
        }
        let reference = self.page_refs().get(page as usize).copied()?;
        let Some(Object::Dict(_)) = DocumentEditor::get(self, reference) else {
            return None;
        };
        let existing = self.content_parts(reference);
        let bracket = match placement {
            StampPlacement::Over => self.bracket_for(&existing),
            StampPlacement::Under => Bracket::default(),
        };

        let name = self.add_resource(page, b"XObject", b"Stamp", Object::Ref(form))?;
        let mut invocation = Vec::with_capacity(name.len() + 8 + 4 * bracket.close.len());
        for closer in &bracket.close {
            invocation.push(b'\n');
            invocation.extend_from_slice(closer);
        }
        invocation.push(b'\n');
        crate::write::write_name(&mut invocation, &name);
        invocation.extend_from_slice(b" Do\n");
        let drawn = self.new_content(invocation);

        let mut parts = Vec::with_capacity(existing.len() + 2);
        match placement {
            StampPlacement::Under => {
                parts.push(drawn);
                parts.extend_from_slice(&existing);
            }
            StampPlacement::Over => {
                if bracket.open {
                    parts.push(self.new_content(b"q\n".to_vec()));
                }
                parts.extend_from_slice(&existing);
                parts.push(drawn);
            }
        }

        let Some(Object::Dict(mut dict)) = DocumentEditor::get(self, reference) else {
            return None;
        };
        dict.insert(
            Name::CONTENTS,
            Object::Array(parts.into_iter().map(Object::Ref).collect()),
        );
        self.put(reference, Object::Dict(dict));
        Some(name)
    }

    /// Writes a form XObject (8.10) as a new object and returns it, for
    /// [`DocumentEditor::stamp`] or anything else that draws one.
    ///
    /// `resources` is the form's own `/Resources`, whose values are this
    /// editor's objects — a font put with [`DocumentEditor::put`], say. The
    /// form's `/BBox`, `/Matrix` and `/Group` are
    /// [`crate::build::DocumentBuilder::add_form`]'s, checked the same way.
    ///
    /// Returns `None` for a degenerate `/BBox` or a non-finite `/Matrix`.
    pub fn add_form(&mut self, form: &FormXObject<'_>, resources: Dict) -> Option<ObjRef> {
        if !is_box(&form.bbox) || form.matrix.is_some_and(|m| !all_finite(&m)) {
            return None;
        }
        let real =
            |values: &[f64]| Object::Array(values.iter().map(|v| Object::Real(*v)).collect());
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.intern(b"XObject")));
        dict.insert(self.intern(b"Subtype"), Object::Name(self.intern(b"Form")));
        dict.insert(self.intern(b"BBox"), real(&form.bbox));
        if let Some(matrix) = form.matrix {
            dict.insert(self.intern(b"Matrix"), real(&matrix));
        }
        if let Some(group) = form.group {
            let mut entry = Dict::new();
            entry.insert(
                self.intern(b"S"),
                Object::Name(self.intern(b"Transparency")),
            );
            entry.insert(
                self.intern(b"CS"),
                Object::Name(self.intern(group.color_space.pdf_name())),
            );
            if group.isolated {
                entry.insert(self.intern(b"I"), Object::Bool(true));
            }
            if group.knockout {
                entry.insert(self.intern(b"K"), Object::Bool(true));
            }
            dict.insert(self.intern(b"Group"), Object::Dict(entry));
        }
        dict.insert(Name::RESOURCES, Object::Dict(resources));
        let reference = self.allocate();
        self.put_stream(
            reference,
            StreamData {
                dict,
                data: form.content.to_vec(),
            },
        );
        Some(reference)
    }

    /// Turns page `page` of another document into a form XObject in this one,
    /// and returns it — a letterhead or a watermark drawn once, in its own
    /// file, and stamped onto every page of this one.
    ///
    /// The page's content streams are decoded and joined as the form's
    /// content, its `/BBox` is the page's crop box, and its resources are
    /// copied with everything they reach by the deep copy
    /// [`DocumentEditor::import_page`] uses, renumbered into this document.
    /// `/Rotate` is **not** applied — a form has no such key — so a caller
    /// importing a rotated page states the turn in `matrix`, which becomes
    /// the form's `/Matrix`.
    ///
    /// Returns `None` for a page the source does not have, a degenerate crop
    /// box, or a non-finite `matrix`.
    pub fn import_page_as_form(
        &mut self,
        source: &CosDocument,
        page: u32,
        matrix: Option<[f64; 6]>,
    ) -> Option<ObjRef> {
        let pages = pages::collect(source);
        let from = pages.get(page as usize)?;
        let crop = from.crop_box;
        let bbox = [crop.x0, crop.y0, crop.x1, crop.y1];
        if !is_box(&bbox) || matrix.is_some_and(|m| !all_finite(&m)) {
            return None;
        }
        let content = pages::content_bytes(source, from);
        let resources = Object::Dict(from.resources.clone().unwrap_or_default());
        let mut mapping: HashMap<u32, ObjRef> = HashMap::new();
        let Some(Object::Dict(resources)) = self.copy_value(source, &resources, &mut mapping, 0)
        else {
            return None;
        };
        self.add_form(
            &FormXObject {
                bbox,
                matrix,
                group: None,
                content: &content,
            },
            resources,
        )
    }

    /// The `/Resources` a page draws with: its own, or the nearest
    /// ancestor's (7.7.3.4), resolved. Empty when neither has one.
    fn effective_resources(&self, page: &Dict) -> Dict {
        let mut node = page.clone();
        for _ in 0..=limits::MAX_NEST_DEPTH {
            if let Some(value) = node.get(Name::RESOURCES) {
                return Resolve::resolve(self, value)
                    .as_dict()
                    .cloned()
                    .unwrap_or_default();
            }
            let Some(parent) = node.get_ref(Name::PARENT) else {
                break;
            };
            let Some(Object::Dict(next)) = DocumentEditor::get(self, parent) else {
                break;
            };
            node = next;
        }
        Dict::new()
    }

    /// Whether `form` is a form XObject stream as this editor has it.
    fn is_form(&self, form: ObjRef) -> bool {
        if self.deleted.contains(&form.num) {
            return false;
        }
        let dict = match self.overlay.get(&form.num) {
            Some(Written::Stream(stream)) => stream.dict.clone(),
            Some(Written::Object(_)) => return false,
            None => match self.doc.get(form) {
                Ok(object) => match object.as_stream() {
                    Some(stream) => stream.dict.clone(),
                    None => return false,
                },
                Err(_) => return false,
            },
        };
        dict.get_name(self.intern(b"Subtype")) == Some(self.intern(b"Form"))
    }

    /// The streams a page's `/Contents` names, as this editor has the page:
    /// one reference, or an array of them, the array itself possibly indirect.
    fn content_parts(&self, page: ObjRef) -> Vec<ObjRef> {
        let Some(Object::Dict(dict)) = DocumentEditor::get(self, page) else {
            return Vec::new();
        };
        match dict.get(Name::CONTENTS) {
            Some(Object::Ref(r)) => match DocumentEditor::get(self, *r) {
                Some(Object::Array(items)) => items.iter().filter_map(Object::as_objref).collect(),
                _ => vec![*r],
            },
            Some(Object::Array(items)) => items.iter().filter_map(Object::as_objref).collect(),
            _ => Vec::new(),
        }
    }

    /// What an over-stamp needs around a page's content, run to its end: see
    /// [`Bracket`]. [`Bracket::blind`] whenever the content cannot be
    /// followed.
    fn bracket_for(&self, parts: &[ObjRef]) -> Bracket {
        let mut content = Vec::new();
        for part in parts {
            let Some(bytes) = self.stream_bytes(*part) else {
                return Bracket::blind();
            };
            content.extend_from_slice(&bytes);
            // 7.8.2: the parts divide at token boundaries only if separated.
            content.push(b'\n');
        }

        let mut lexer = Lexer::new(&content);
        let mut sink = WarningSink::new();
        // What is open, outermost first, and how many of it are saves.
        let mut open: Vec<Opened> = Vec::new();
        let mut saves = 0usize;
        let mut in_text = false;
        // Whether an operator outside every `q` changed the state.
        let mut changed = false;
        let mut at = 0u64;
        // Every token consumes at least a byte, so this bounds the walk; the
        // progress check below makes the bound unnecessary rather than load
        // bearing.
        for _ in 0..=content.len() {
            let token = lexer.next_token(&mut sink);
            match token.kind {
                TokenKind::Eof => {
                    let mut bracket = Bracket {
                        open: changed,
                        close: open.iter().rev().map(|opened| opened.closer()).collect(),
                    };
                    if changed {
                        bracket.close.push(b"Q");
                    }
                    return bracket;
                }
                TokenKind::Unknown => {
                    let Some(operator) = usize::try_from(token.start)
                        .ok()
                        .zip(usize::try_from(token.end).ok())
                        .and_then(|(start, end)| content.get(start..end))
                    else {
                        return Bracket::blind();
                    };
                    match operator {
                        b"q" => {
                            open.push(Opened::Save);
                            saves += 1;
                        }
                        b"Q" => {
                            // A `Q` with nothing to restore is an error some
                            // readers ignore and others do not, which makes
                            // it content whose state this cannot follow.
                            if !close_last(&mut open, Opened::Save) {
                                return Bracket::blind();
                            }
                            saves -= 1;
                        }
                        // 9.4.1: text objects do not nest; a second `BT` is
                        // the same object, and one `ET` ends it.
                        b"BT" if !in_text => {
                            open.push(Opened::Text);
                            in_text = true;
                        }
                        b"BT" => {}
                        b"ET" => {
                            close_last(&mut open, Opened::Text);
                            in_text = false;
                        }
                        b"BMC" | b"BDC" => open.push(Opened::Marked),
                        // A stray `EMC` or `ET` closes nothing and changes no
                        // state, so it costs a stamp nothing.
                        b"EMC" => {
                            close_last(&mut open, Opened::Marked);
                        }
                        // 8.9.7: an inline image's data is not tokenizable,
                        // so it is skipped to its `EI` by the rule the
                        // interpreter skips it by.
                        b"BI" => {
                            let from = usize::try_from(token.end).unwrap_or(content.len());
                            let rest = content.get(from..).unwrap_or_default();
                            let end = from.saturating_add(skip_inline_image(rest));
                            lexer.seek(u64::try_from(end).unwrap_or(u64::MAX));
                        }
                        // `ID` and `EI` outside what `BI` skipped mean the
                        // skip and the content disagree about where an
                        // image's data ends.
                        b"ID" | b"EI" => return Bracket::blind(),
                        other if saves == 0 && !NEUTRAL.contains(&other) => changed = true,
                        _ => {}
                    }
                    // Deeper than any reader follows — Annex C puts the `q`
                    // limit at 28, and this repository's interpreter keeps 64
                    // saves — so past the parser's own nesting bound what a
                    // reader restores is its own business. The bound also
                    // keeps `close_last`'s search short.
                    if open.len() > limits::MAX_NEST_DEPTH as usize {
                        return Bracket::blind();
                    }
                }
                _ => {}
            }
            if token.end <= at {
                return Bracket::blind();
            }
            at = token.end;
        }
        Bracket::blind()
    }

    /// A new, unfiltered content stream holding `data`.
    fn new_content(&mut self, data: Vec<u8>) -> ObjRef {
        let reference = self.allocate();
        self.put_stream(
            reference,
            StreamData {
                dict: Dict::new(),
                data,
            },
        );
        reference
    }
}

/// Removes the innermost `kind` from `open`, and says whether there was one.
///
/// The innermost of that kind rather than the innermost of all: content that
/// interleaves a save with a marked-content sequence still closes each where
/// a reader closes it.
fn close_last(open: &mut Vec<Opened>, kind: Opened) -> bool {
    match open.iter().rposition(|opened| *opened == kind) {
        Some(at) => {
            open.remove(at);
            true
        }
        None => false,
    }
}

/// How many bytes after `BI` an inline image runs, to the end of its `EI`
/// (8.9.7) — `EI` preceded by white space and followed by white space, a
/// delimiter or the end — or to the end of `rest` when there is none.
///
/// The rule `tinker-pdf-content`'s interpreter skips an inline image by,
/// restated because that crate reads this one and not the other way round: a
/// skip that disagreed would count operators inside the image's data that the
/// page does not run.
fn skip_inline_image(rest: &[u8]) -> usize {
    let delimiter = |c: u8| {
        matches!(
            c,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
    };
    let mut i = 0usize;
    while i + 1 < rest.len() {
        if rest.get(i) == Some(&b'E') && rest.get(i + 1) == Some(&b'I') {
            let before_ok = i == 0
                || rest
                    .get(i - 1)
                    .is_some_and(|b| b.is_ascii_whitespace() || *b == 0);
            let after_ok = rest
                .get(i + 2)
                .is_none_or(|b| b.is_ascii_whitespace() || delimiter(*b));
            if before_ok && after_ok {
                return i + 2;
            }
        }
        i += 1;
    }
    rest.len()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// An editor over a one-page document whose content is `content`.
    fn editor(content: &str) -> (DocumentEditor, Vec<ObjRef>) {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
            content.len()
        );
        let doc = CosDocument::open(bytes.into_bytes()).expect("it opens");
        let editor = DocumentEditor::new(Arc::new(doc));
        let parts = editor.content_parts(ObjRef::new(3, 0));
        (editor, parts)
    }

    fn bracket(content: &str) -> Bracket {
        let (editor, parts) = editor(content);
        editor.bracket_for(&parts)
    }

    fn balanced(content: &str) -> bool {
        // Nothing before the page's content and nothing after it: the stamp
        // simply follows.
        bracket(content) == Bracket::default()
    }

    #[test]
    fn content_that_restores_what_it_changes_is_left_alone() {
        assert!(balanced(""));
        assert!(balanced("q 2 0 0 2 0 0 cm 1 0 0 rg 0 0 1 1 re f Q"));
        assert!(balanced("0 0 m 1 1 l S /Fm0 Do /Sh0 sh BT (x) Tj ET"));
        assert!(balanced(
            "q q 0.5 g Q 1 w Q /P <</MCID 0>> BDC 0 0 1 1 re f EMC"
        ));
        assert!(
            balanced("q (a Q string) Tj Q"),
            "a Q inside a string is text"
        );
        assert!(
            balanced("BT 0 -20 Td T* (x) ' ET"),
            "Td, T* and ' move within the text object only"
        );
        assert!(
            balanced("q /P BMC Q EMC"),
            "interleaved, and each closed where a reader closes it"
        );
        assert!(balanced("ET EMC"), "a stray closer closes nothing");
        // An inline image is skipped to its `EI` as the interpreter skips it,
        // so bytes in its data that spell operators are not counted.
        assert!(balanced("BI /W 1 /H 1 /BPC 8 /CS /G ID \u{0} EI"));
        assert!(balanced("BI /W 2 /H 1 /BPC 8 /CS /G ID q  EI"));
    }

    #[test]
    fn content_that_leaves_its_state_changed_is_not() {
        for content in [
            "2 0 0 2 0 0 cm",
            "1 0 0 rg 0 0 1 1 re f",
            "q 0 0 1 1 re f",
            "0 0 1 1 re f Q",
            "0 0 10 10 re W n",
            "BT /F1 12 Tf (x) Tj ET",
            "1 2 (x) \"",
            // Table 108: `TD` is `-ty TL tx ty Td`, and `TL` outlives `ET`.
            "BT 0 -20 TD ET",
            "/OC /L BDC 0 0 1 1 re f",
            "BI /W 1 /H 1 /BPC 8 /CS /G ID \u{0} EI 2 0 0 2 0 0 cm",
        ] {
            assert!(!balanced(content), "{content:?}");
        }
    }

    /// What the content left open is closed innermost first, and a `q` goes
    /// before it only when something outside every `q` changed the state —
    /// in which case one more `Q` follows the closers.
    #[test]
    fn what_is_left_open_is_closed_innermost_first() {
        let cases: [(&str, bool, &[&[u8]]); 8] = [
            ("q 0 0 1 1 re f", false, &[b"Q"]),
            ("/OC /L BDC 0 0 1 1 re f", false, &[b"EMC"]),
            ("q /P BMC BT (x) Tj", false, &[b"ET", b"EMC", b"Q"]),
            ("BT BT (x) Tj", false, &[b"ET"]),
            // The review's two pages: a `cm` before an unclosed `q`, whose
            // `Q` alone would restore the scaled state; and a layer opened
            // and never closed, which would hide the stamp with it.
            ("2 0 0 2 0 0 cm q 0 0 1 rg", true, &[b"Q", b"Q"]),
            ("/OC /L BDC 0 0 1 rg 5 5 10 10 re f", true, &[b"EMC", b"Q"]),
            ("BT 0 -20 TD ET", true, &[b"Q"]),
            ("1 w q /P BMC", true, &[b"EMC", b"Q", b"Q"]),
        ];
        for (content, open, close) in cases {
            assert_eq!(
                bracket(content),
                Bracket {
                    open,
                    close: close.to_vec()
                },
                "{content:?}"
            );
        }
    }

    /// Content this cannot follow gets one pair around it and no more.
    #[test]
    fn content_that_cannot_be_followed_is_bracketed_blind() {
        assert_eq!(bracket("0 0 1 1 re f Q"), Bracket::blind(), "Q underflows");
        assert_eq!(bracket("ID EI"), Bracket::blind(), "image data with no BI");
        assert_eq!(
            bracket(&"q ".repeat(limits::MAX_NEST_DEPTH as usize + 1)),
            Bracket::blind(),
            "deeper than any reader follows"
        );
        assert_eq!(
            bracket(&"q ".repeat(limits::MAX_NEST_DEPTH as usize))
                .close
                .len(),
            limits::MAX_NEST_DEPTH as usize,
            "and as deep as that is followed"
        );
    }

    #[test]
    fn a_name_is_the_prefix_and_the_smallest_free_number() {
        let (mut editor, _) = editor("");
        let first = editor.add_resource(0, b"XObject", b"Stamp", Object::Null);
        let second = editor.add_resource(0, b"XObject", b"Stamp", Object::Null);
        assert_eq!(first.as_deref(), Some(&b"Stamp0"[..]));
        assert_eq!(second.as_deref(), Some(&b"Stamp1"[..]));
    }
}
