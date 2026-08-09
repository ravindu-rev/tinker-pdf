//! Link annotations (12.5.6.5).
//!
//! The scope fence for this phase: `/Annots` is read, `/Subtype /Link` is kept,
//! and every other annotation subtype is invisible here. Markup annotations,
//! widgets and appearance streams belong to later phases, and reaching for them
//! from this one is how an annotation model gets written twice.

use crate::name::Name;
use crate::object::ObjRef;
use crate::semantics::action::Target;
use crate::semantics::geom::{rect_from_object, Rect};
use crate::semantics::pages::Page;
use crate::warn::WarningSink;

/// One link annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The annotation object, when `/Annots` named it indirectly.
    pub reference: Option<ObjRef>,
    /// `/Rect` (12.5.2), normalized.
    pub rect: Rect,
    /// Where the link goes. `None` for a `/Link` annotation that carries
    /// neither `/Dest` nor a usable `/A` — legal, useless, and worth reporting
    /// rather than hiding.
    pub target: Option<Target>,
}

impl Page<'_> {
    /// The page's link annotations, in `/Annots` order.
    pub fn links(&self) -> Vec<Link> {
        let doc = self.document();
        let sem = &doc.names.sem;
        let mut sink = WarningSink::new();
        let mut out = Vec::new();

        let annots = doc.resolve_key(self.dict(), sem.annots);
        if let Some(items) = annots.as_array() {
            for item in items {
                let reference = item.as_objref();
                let resolved = doc.resolve(item);
                let Some(dict) = resolved.as_dict() else {
                    continue;
                };
                if dict.get_name(sem.subtype) != Some(sem.link) {
                    continue;
                }
                let rect = doc
                    .resolve_key(dict, sem.rect)
                    .as_ref()
                    .pipe_rect(doc)
                    .unwrap_or_default();
                out.push(Link {
                    reference,
                    rect,
                    target: doc.target_in(dict, &mut sink),
                });
            }
        }
        doc.absorb(sink);
        out
    }
}

/// A tiny extension so the rectangle read stays one expression; `/Rect` is
/// resolved before it is parsed because its elements may be indirect.
trait PipeRect {
    fn pipe_rect(&self, doc: &crate::doc::CosDocument) -> Option<Rect>;
}

impl PipeRect for crate::object::Object {
    fn pipe_rect(&self, doc: &crate::doc::CosDocument) -> Option<Rect> {
        rect_from_object(doc, self)
    }
}

// `/Rect` and `/Subtype` are Table 164 keys; `Name::TYPE` is not used here
// because 12.5.2 makes `/Type /Annot` optional in practice.
const _: Name = Name::TYPE;
