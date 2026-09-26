//! The content-stream walk the font and colour groups both need.
//!
//! # Why this is a module and not two scans
//!
//! Two ISO 19005 clause families are about what a page *does* rather than
//! about what the file *contains*: fonts, because 6.3 constrains fonts "used
//! for rendering", and colour, because 6.2.3.3 constrains device colour spaces
//! that are used. Both need the same walk — every page's content, the form
//! XObjects it invokes, the appearance streams its annotations carry, with the
//! resource dictionary in scope and the text state tracked through `q` and `Q`
//! — and differ only in which operators they care about.
//!
//! Written twice, the two would drift, and the drift would be invisible: a
//! form-XObject cycle guard fixed on one side and not the other is a hang
//! nobody sees until a hostile file finds it. So the walk is here and the
//! groups are visitors over it.
//!
//! # What this is not
//!
//! Not an interpreter. It tracks the text rendering mode and the selected font
//! and nothing else: no matrices, no clipping, no colour state. A rule that
//! needed the current transformation matrix would need
//! `tinker_pdf_content::interpret` and a `Device`, which is a renderer, and a
//! validator that rendered every file to answer a question about a name would
//! be paying for a picture nobody looks at.

use std::collections::BTreeSet;

use tinker_pdf_content::{Token, Tokenizer};
use tinker_pdf_cos::{pages, CosDocument, Dict, ObjRef, Object};

/// How many pages the walk visits.
const MAX_PAGES: usize = 1 << 14;

/// How deep a form XObject may invoke another.
const MAX_FORM_DEPTH: u32 = 12;

/// How many streams the walk tokenizes in one document.
///
/// A validator is asked to run on untrusted input (ruling 1), and a walk whose
/// only bound is the file's own is a denial of service with a clause number.
const MAX_STREAMS: usize = 1 << 14;

/// How many tokens one content stream contributes before the walk moves on.
const MAX_TOKENS: usize = 1 << 22;

/// How many operands one operator may carry into a visitor.
const MAX_OPERANDS: usize = 64;

/// The text rendering mode ISO 32000-1 9.3.6 gives to invisible text.
pub(super) const RENDER_MODE_INVISIBLE: f64 = 3.0;

/// One operator, with everything a rule needs to judge it.
pub(super) struct Op<'a> {
    /// The resource dictionary in scope, which is the stream's own or the one
    /// it inherited from the stream that invoked it (7.8.3).
    pub(super) resources: Option<&'a Dict>,
    /// The operator's bytes, without a leading slash — `rg`, `Do`, `Tj`.
    pub(super) operator: &'a [u8],
    /// Its operands, in the order they were written.
    pub(super) operands: &'a [Token],
    /// The text rendering mode in force.
    pub(super) mode: f64,
    /// The resource name the last `Tf` selected.
    pub(super) font: Option<&'a [u8]>,
}

impl Op<'_> {
    /// The first operand, when it is a name.
    pub(super) fn first_name(&self) -> Option<&[u8]> {
        match self.operands.first() {
            Some(Token::Name(name)) => Some(name),
            _ => None,
        }
    }
}

/// Calls `visit` for every operator of every content stream the document
/// renders.
pub(super) fn walk(doc: &CosDocument, visit: &mut impl FnMut(&Op<'_>)) {
    let mut walker = Walker {
        doc,
        streams: MAX_STREAMS,
        visited: BTreeSet::new(),
    };
    for page in pages::collect_upto(doc, MAX_PAGES) {
        let resources = page.resources.clone();
        let content = pages::content_bytes(doc, &page);
        walker.stream(&content, resources.as_ref(), 0, 0.0, visit);

        // 12.5.5: an annotation's appearance stream is drawn by the reader and
        // is as much a rendering of the file as the page's own content. A
        // widget whose `/N` is a sub-dictionary of states has one stream per
        // state and every one of them can be shown.
        let Ok(page_dict) = doc.get(page.reference) else {
            continue;
        };
        let Some(page_dict) = page_dict.as_dict() else {
            continue;
        };
        let annots = doc.resolve_key(page_dict, doc.intern(b"Annots"));
        let Some(annots) = annots.as_array() else {
            continue;
        };
        for annot in annots.iter().take(MAX_STREAMS) {
            let annot = doc.resolve(annot);
            let Some(annot) = annot.as_dict() else {
                continue;
            };
            let appearance = doc.resolve_key(annot, doc.intern(b"AP"));
            let Some(appearance) = appearance.as_dict() else {
                continue;
            };
            for (_, slot) in appearance.entries() {
                walker.appearance(slot, resources.as_ref(), visit);
            }
        }
    }
}

/// A named resource's indirect reference, through the resource dictionary's
/// own sub-dictionary.
pub(super) fn lookup(
    doc: &CosDocument,
    resources: &Dict,
    category: &[u8],
    name: &[u8],
) -> Option<ObjRef> {
    let category = doc.resolve_key(resources, doc.intern(category));
    category.as_dict()?.get_ref(doc.intern(name))
}

struct Walker<'a> {
    doc: &'a CosDocument,
    /// How many more streams may be tokenized.
    streams: usize,
    /// Streams already entered, so a form that invokes itself is walked once
    /// rather than until the depth bound.
    visited: BTreeSet<ObjRef>,
}

impl Walker<'_> {
    /// One `/AP` slot, which is either a stream or a dictionary of states.
    fn appearance(
        &mut self,
        slot: &Object,
        inherited: Option<&Dict>,
        visit: &mut impl FnMut(&Op<'_>),
    ) {
        let resolved = self.doc.resolve(slot);
        match resolved.as_ref() {
            Object::Stream(_) => {
                if let Some(reference) = slot.as_objref() {
                    self.form(reference, inherited, 0, 0.0, visit);
                }
            }
            Object::Dict(states) => {
                for (_, state) in states.entries() {
                    if let Some(reference) = state.as_objref() {
                        self.form(reference, inherited, 0, 0.0, visit);
                    }
                }
            }
            _ => {}
        }
    }

    /// One form XObject or appearance stream, with its own resources when it
    /// has them and the invoking stream's when it does not (7.8.3).
    fn form(
        &mut self,
        reference: ObjRef,
        inherited: Option<&Dict>,
        depth: u32,
        mode: f64,
        visit: &mut impl FnMut(&Op<'_>),
    ) {
        if depth > MAX_FORM_DEPTH || !self.visited.insert(reference) {
            return;
        }
        let Ok(object) = self.doc.get(reference) else {
            return;
        };
        let Some(stream) = object.as_stream() else {
            return;
        };
        let own = self
            .doc
            .resolve_key(&stream.dict, self.doc.intern(b"Resources"));
        let own = own.as_dict().cloned();
        let Ok(bytes) = self.doc.stream_decoded(reference) else {
            return;
        };
        let resources = own.as_ref().or(inherited);
        self.stream(&bytes, resources, depth, mode, visit);
    }

    /// Tokenizes one content stream, tracking the text state.
    ///
    /// The rendering mode is **carried into a form XObject**, because 8.10.1
    /// makes a form's content part of the invoking stream's rendering rather
    /// than a fresh one: a form invoked while mode 3 is in force paints
    /// nothing either.
    fn stream(
        &mut self,
        bytes: &[u8],
        resources: Option<&Dict>,
        depth: u32,
        start_mode: f64,
        visit: &mut impl FnMut(&Op<'_>),
    ) {
        if self.streams == 0 {
            return;
        }
        self.streams -= 1;

        let mut tokenizer = Tokenizer::new(bytes);
        let mut operands: Vec<Token> = Vec::new();
        let mut mode = start_mode;
        let mut font: Option<Vec<u8>> = None;
        // 8.4.2: `q` and `Q` save and restore the whole graphics state, and
        // 9.3 puts the text font and the text rendering mode in it.
        let mut saved: Vec<(f64, Option<Vec<u8>>)> = Vec::new();
        let mut seen = 0usize;

        while let Some(token) = tokenizer.next_token() {
            seen += 1;
            if seen > MAX_TOKENS {
                return;
            }
            let Token::Operator(operator) = &token else {
                if operands.len() < MAX_OPERANDS {
                    operands.push(token);
                }
                continue;
            };
            match operator.as_slice() {
                b"q" => saved.push((mode, font.clone())),
                b"Q" => {
                    if let Some((old_mode, old_font)) = saved.pop() {
                        mode = old_mode;
                        font = old_font;
                    }
                }
                b"Tf" => {
                    font = operands.iter().find_map(|token| match token {
                        Token::Name(name) => Some(name.clone()),
                        _ => None,
                    });
                }
                b"Tr" => {
                    if let Some(Token::Number(value)) = operands.first() {
                        mode = *value;
                    }
                }
                // 8.9.7: an inline image's data is not tokenizable, so it is
                // skipped at the byte level. A byte scan for `EI`, not a second
                // parser: the tokenizer keeps its own position and this moves
                // it past the picture.
                b"BI" => {
                    let consumed = skip_inline_image(tokenizer.rest());
                    let at = tokenizer.position() + consumed;
                    tokenizer.seek(at);
                }
                _ => {}
            }

            visit(&Op {
                resources,
                operator,
                operands: &operands,
                mode,
                font: font.as_deref(),
            });

            if operator.as_slice() == b"Do" {
                if let (Some(Token::Name(name)), Some(resources)) = (operands.first(), resources) {
                    if let Some(reference) = lookup(self.doc, resources, b"XObject", name) {
                        let is_form = self
                            .doc
                            .get(reference)
                            .ok()
                            .and_then(|object| {
                                object.as_stream().map(|stream| {
                                    self.doc
                                        .resolve_key(&stream.dict, self.doc.intern(b"Subtype"))
                                        .as_name()
                                        .and_then(|name| self.doc.name_bytes(name))
                                        .is_some_and(|name| name.as_ref() == b"Form")
                                })
                            })
                            .unwrap_or(false);
                        if is_form {
                            let owned = resources.clone();
                            self.form(reference, Some(&owned), depth + 1, mode, visit);
                        }
                    }
                }
            }
            operands.clear();
        }
    }
}

/// How many bytes of `rest` an inline image occupies, up to and including its
/// `EI`.
///
/// The `EI` has to be delimited on both sides: the two bytes appear inside
/// compressed image data often enough that an undelimited match resumes
/// tokenizing in the middle of a picture.
fn skip_inline_image(rest: &[u8]) -> usize {
    let mut i = 0usize;
    while i + 1 < rest.len() {
        if rest[i] == b'E' && rest[i + 1] == b'I' {
            let before = i == 0
                || rest
                    .get(i - 1)
                    .is_some_and(|b| b.is_ascii_whitespace() || *b == 0);
            let after = rest
                .get(i + 2)
                .is_none_or(|b| b.is_ascii_whitespace() || *b == 0);
            if before && after {
                return i + 2;
            }
        }
        i += 1;
    }
    rest.len()
}
