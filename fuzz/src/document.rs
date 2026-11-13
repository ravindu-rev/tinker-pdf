//! A generated page's document, laid out the ways a file may hold one: the
//! content generator's objects (`crate::content::model`) in one to four
//! sections (ISO 32000-2 7.5.6), each a cross-reference table or stream with
//! objects packed into an object stream or not (7.5.7, 7.5.8), the updates
//! writing some of the page's objects again as they were, adding objects
//! nothing refers to and freeing them, and every token spelled as the input
//! chooses (`crate::cos::write`) — for `render_rewrite`, whose relation is a
//! render against the rewrite of the same document.
//!
//! The file is written clean: every `/Length` honest and every offset true,
//! so it opens with no repair, and every update leaves the page what it was.

use tinker_pdf::RenderOptions;

use crate::cos::{self, Body, File, Filter, Indirect, Packed, Section, Value, Xref};
use crate::{content, Choices};

/// A generated document, and how it is to be rendered.
pub struct Document {
    /// The file.
    pub bytes: Vec<u8>,
    /// The render's options, generated with the page.
    pub options: RenderOptions,
    /// Sections, cross-reference streams among them, objects packed into
    /// object streams, the page's objects an update wrote again, and objects
    /// an update freed.
    pub sections: usize,
    pub streams: usize,
    pub packed: usize,
    pub redefined: usize,
    pub freed: usize,
}

/// A document built from `c`.
pub fn document(c: &mut Choices) -> Document {
    let (objects, options) = content::model(c);
    let count = 1 + c.below(cos::MAX_SECTIONS);
    // Cross-reference and object streams are PDF 1.5's (7.5.8).
    let version = *c.pick(&[(1, 7), (1, 5), (2, 0), (1, 6)]);
    let mut out = Document {
        bytes: Vec::new(),
        options,
        sections: count,
        streams: 0,
        packed: 0,
        redefined: 0,
        freed: 0,
    };
    let mut next = objects.iter().map(|o| o.num).max().unwrap_or(4) + 1;
    // Objects an update added, which nothing refers to, and so may be freed.
    let mut added: Vec<u32> = Vec::new();
    let mut sections = Vec::with_capacity(count);
    for index in 0..count {
        let mut here: Vec<Indirect> = if index == 0 {
            objects.clone()
        } else {
            Vec::new()
        };
        let mut freed = Vec::new();
        if index > 0 {
            // The page's objects written again as they were: an update that
            // changes nothing the page draws.
            for _ in 0..c.below(3) {
                let object = c.pick(&objects).clone();
                if !here.iter().any(|h| h.num == object.num) {
                    here.push(object);
                    out.redefined += 1;
                }
            }
            if !added.is_empty() && c.below(3) == 2 {
                let num = added.remove(c.below(added.len()));
                freed.push((num, 1));
                out.freed += 1;
            }
            if c.flag() {
                here.push(Indirect {
                    num: next,
                    gen: 0,
                    body: Body::Value(Value::Int(c.range(-9, 9))),
                });
                added.push(next);
                next += 1;
            }
        }
        if here.len() > 1 {
            let by = c.below(here.len());
            here.rotate_left(by);
        }
        let xref = if c.flag() {
            out.streams += 1;
            let packed = if c.flag() {
                // 7.5.7: no stream, and nothing of a generation but zero.
                let mut members = Vec::new();
                let mut kept = Vec::new();
                for object in here {
                    match &object.body {
                        Body::Value(v) if object.gen == 0 && c.below(3) != 0 => {
                            members.push((object.num, v.clone()));
                        }
                        _ => kept.push(object),
                    }
                }
                here = kept;
                if members.is_empty() {
                    None
                } else {
                    out.packed += members.len();
                    let num = next;
                    next += 1;
                    Some(Packed {
                        num,
                        objects: members,
                        flate: c.flag(),
                    })
                }
            } else {
                None
            };
            let num = next;
            next += 1;
            Xref::Stream {
                num,
                wider: [c.below(2) as u8, c.below(3) as u8, c.below(3) as u8],
                index: c.flag(),
                filter: *c.pick(&[Filter::None, Filter::Flate, Filter::FlateUp]),
                packed,
            }
        } else {
            Xref::Table {
                eol: c.below(3) as u8,
            }
        };
        sections.push(Section {
            objects: here,
            freed,
            xref,
            trailer: vec![(b"Root".to_vec(), Value::reference(1))],
        });
    }
    let file = File { version, sections };
    out.bytes = cos::write(&file, c).bytes;
    out
}

#[cfg(test)]
mod tests;
