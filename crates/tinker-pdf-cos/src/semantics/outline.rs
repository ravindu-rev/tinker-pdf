//! The document outline (12.3.3).
//!
//! An absent `/Outlines` is an empty vector, not an error: most documents have
//! no outline, and that is an ordinary answer.
//!
//! The walk guards both axes with one visited set — child cycles through
//! `/First` and sibling cycles through `/Next` — and truncates at the first
//! revisit with a warning.

use std::collections::HashSet;

use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::semantics::action::Target;
use crate::warn::{WarningKind, WarningSink};

/// One outline entry (12.3.3).
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineItem {
    /// `/Title`, decoded as a text string (7.9.2.2).
    pub title: String,
    /// `/Dest` or `/A`, whichever the entry carried. `None` is ordinary: an
    /// entry that only groups its children needs no target.
    pub target: Option<Target>,
    /// Whether the entry is displayed expanded, from the sign of `/Count`.
    pub open: bool,
    /// Nested entries, in order.
    pub children: Vec<OutlineItem>,
}

/// One outline entry in the flattened view, carrying its nesting depth.
#[derive(Clone, Debug, PartialEq)]
pub struct FlatOutlineItem {
    /// Nesting depth; top-level entries are 0.
    pub depth: u32,
    /// `/Title`, decoded.
    pub title: String,
    /// The entry's target, unresolved.
    pub target: Option<Target>,
    /// Whether the entry is displayed expanded.
    pub open: bool,
    /// The zero-based page the target resolves to, when it resolves at all.
    /// A dead target keeps its entry — a bookmark with a broken destination
    /// still belongs in the sidebar.
    pub page_index: Option<u32>,
}

impl CosDocument {
    /// The outline tree (12.3.3). Empty when the document has none.
    pub fn outline(&self) -> Vec<OutlineItem> {
        let mut sink = WarningSink::new();
        let catalog = self.catalog();
        let root = self.resolve_key(&catalog, self.names.sem.outlines);
        let items = match root.as_dict() {
            Some(root) => {
                let mut visited: HashSet<u32> = HashSet::new();
                let mut budget = limits::MAX_OUTLINE_ITEMS;
                self.siblings(root.get(Name::FIRST), 0, &mut visited, &mut budget, &mut sink)
            }
            None => Vec::new(),
        };
        self.absorb(sink);
        items
    }

    /// The outline flattened depth-first, each entry carrying its depth and its
    /// resolved page.
    pub fn outline_flat(&self) -> Vec<FlatOutlineItem> {
        let mut out = Vec::new();
        self.flatten(&self.outline(), 0, &mut out);
        out
    }

    fn flatten(&self, items: &[OutlineItem], depth: u32, out: &mut Vec<FlatOutlineItem>) {
        for item in items {
            out.push(FlatOutlineItem {
                depth,
                title: item.title.clone(),
                target: item.target.clone(),
                open: item.open,
                page_index: item
                    .target
                    .as_ref()
                    .and_then(|target| self.resolve_target(target))
                    .map(|resolved| resolved.page_index),
            });
            self.flatten(&item.children, depth.saturating_add(1), out);
        }
    }

    /// Walks one `/Next` chain, recursing into `/First` for children.
    ///
    /// Recursion is bounded by [`limits::MAX_OUTLINE_DEPTH`] frames, and the
    /// sibling chain is a loop rather than a recursion, so a document with a
    /// hundred thousand top-level entries costs one stack frame.
    fn siblings(
        &self,
        first: Option<&Object>,
        depth: u32,
        visited: &mut HashSet<u32>,
        budget: &mut usize,
        sink: &mut WarningSink,
    ) -> Vec<OutlineItem> {
        let mut out = Vec::new();
        if depth >= limits::MAX_OUTLINE_DEPTH {
            if first.is_some() {
                sink.warn(0, WarningKind::OutlineCapHit);
            }
            return out;
        }
        let mut next: Option<Object> = first.cloned();
        while let Some(object) = next.take() {
            if *budget == 0 {
                sink.warn(0, WarningKind::OutlineCapHit);
                break;
            }
            let reference = object.as_objref();
            if let Some(r) = reference {
                if !visited.insert(r.num) {
                    sink.warn_at(0, Some(r), WarningKind::OutlineCycle);
                    break;
                }
            }
            let resolved = self.resolve(&object);
            let Some(dict) = resolved.as_dict() else {
                break;
            };
            *budget -= 1;
            out.push(self.outline_item(dict, reference, depth, visited, budget, sink));
            next = dict.get(self.names.sem.next).cloned();
        }
        out
    }

    fn outline_item(
        &self,
        dict: &Dict,
        reference: Option<ObjRef>,
        depth: u32,
        visited: &mut HashSet<u32>,
        budget: &mut usize,
        sink: &mut WarningSink,
    ) -> OutlineItem {
        let title = match self.resolve_key(dict, self.names.sem.title).as_string() {
            Some(s) => self.text_string_at(&s.bytes, reference, sink),
            None => String::new(),
        };
        // 12.3.3: a positive /Count means the entry is open and shows that many
        // descendants; a negative one means closed.
        let open = dict.get_int(Name::COUNT).is_some_and(|count| count > 0);
        let target = self.target_in(dict, sink);
        let children = self.siblings(
            dict.get(Name::FIRST),
            depth.saturating_add(1),
            visited,
            budget,
            sink,
        );
        OutlineItem {
            title,
            target,
            open,
            children,
        }
    }
}
