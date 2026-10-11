//! Optional content (8.11) on an existing document: which layers the default
//! configuration shows.

use super::{without, DocumentEditor};
use crate::object::{Dict, ObjRef, Object};
use crate::resolve::Resolve;

impl DocumentEditor {
    /// Shows or hides the optional content group at `group` in the
    /// document's **default configuration** — the catalog's `/OCProperties
    /// /D`, which is what a reader applies when it opens the file (8.11.4.3).
    ///
    /// Table 101 decides a group's state by `/BaseState` and then `/ON` and
    /// `/OFF`, so the group is taken out of both lists and put back into
    /// whichever one disagrees with `/BaseState`, if either does: a group
    /// shown under a base state of `/ON` is in neither list, which is the
    /// shape a producer would have written. An emptied list is removed rather
    /// than written empty.
    ///
    /// Each dictionary on the way is changed where it lives: an indirect
    /// `/OCProperties` or `/D` is replaced at its own number, a direct one is
    /// rewritten inside its parent, and the catalog goes through
    /// [`DocumentEditor::update_catalog`]. Nothing else in the document is
    /// touched, so an incremental save carries one or two objects.
    ///
    /// `/AS` (8.11.4.4), which asks a viewer to change states on events such
    /// as printing, is left as it is: it is a statement about those events,
    /// and not about the default this sets.
    ///
    /// Returns false, changing nothing, when the document declares no optional
    /// content or when `/OCGs` does not list `group` — a group the catalog
    /// never declared is not one of the document's layers, and 8.11.4.3's
    /// lists only mean anything for the ones it did.
    pub fn set_layer_visible(&mut self, group: ObjRef, visible: bool) -> bool {
        let Some(catalog) = self.catalog() else {
            return false;
        };
        let properties_key = self.intern(b"OCProperties");
        let Some((properties_ref, mut properties)) = self.dict_at(catalog.get(properties_key))
        else {
            return false;
        };
        let listed = self.refs_in(properties.get(self.intern(b"OCGs")));
        if !listed.contains(&group) {
            return false;
        }

        let d_key = self.intern(b"D");
        let (d_ref, mut configuration) = self
            .dict_at(properties.get(d_key))
            .unwrap_or((None, Dict::new()));
        let base_off = configuration
            .get_name(self.intern(b"BaseState"))
            .and_then(|n| self.doc.name_bytes(n))
            .is_some_and(|n| n.as_ref() == b"OFF");

        let on_key = self.intern(b"ON");
        let off_key = self.intern(b"OFF");
        let mut on: Vec<ObjRef> = self.refs_in(configuration.get(on_key));
        let mut off: Vec<ObjRef> = self.refs_in(configuration.get(off_key));
        on.retain(|r| *r != group);
        off.retain(|r| *r != group);
        match (visible, base_off) {
            (true, true) => on.push(group),
            (false, false) => off.push(group),
            _ => {}
        }
        for (key, list) in [(on_key, on), (off_key, off)] {
            if list.is_empty() {
                configuration = without(&configuration, key);
            } else {
                configuration.insert(
                    key,
                    Object::Array(list.into_iter().map(Object::Ref).collect()),
                );
            }
        }

        // An indirect `/D` is replaced where it lives, and then nothing above
        // it has changed and nothing above it is written.
        if let Some(reference) = d_ref {
            self.put(reference, Object::Dict(configuration));
            return true;
        }
        properties.insert(d_key, Object::Dict(configuration));
        match properties_ref {
            Some(reference) => {
                self.put(reference, Object::Dict(properties));
                true
            }
            None => self.update_catalog(|catalog| {
                catalog.insert(properties_key, Object::Dict(properties));
            }),
        }
    }

    /// A dictionary value as this editor has it, and its own number when it
    /// is indirect.
    fn dict_at(&self, value: Option<&Object>) -> Option<(Option<ObjRef>, Dict)> {
        match value? {
            Object::Ref(r) => {
                let object = Resolve::get(self, *r).ok()?;
                Some((Some(*r), object.as_dict()?.clone()))
            }
            Object::Dict(dict) => Some((None, dict.clone())),
            _ => None,
        }
    }

    /// The references an array value holds, the array itself resolved if it
    /// is indirect; anything that is not a reference is dropped, since
    /// 8.11.4's lists are lists of groups and a group is an indirect object.
    fn refs_in(&self, value: Option<&Object>) -> Vec<ObjRef> {
        let Some(value) = value else {
            return Vec::new();
        };
        let resolved = Resolve::resolve(self, value);
        resolved
            .as_array()
            .map(|items| items.iter().filter_map(Object::as_objref).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::doc::CosDocument;

    fn editor(properties: &str, extra: &str) -> DocumentEditor {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R {properties} >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>\nendobj\n\
10 0 obj\n<< /Type /OCG /Name (A) >>\nendobj\n\
11 0 obj\n<< /Type /OCG /Name (B) >>\nendobj\n\
{extra}\
trailer\n<< /Size 30 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = CosDocument::open(bytes.into_bytes()).expect("it opens");
        DocumentEditor::new(Arc::new(doc))
    }

    /// The configuration's `/ON` and `/OFF`, as reference numbers.
    fn lists(editor: &DocumentEditor) -> (Vec<u32>, Vec<u32>) {
        let catalog = editor.catalog().expect("a catalog");
        let properties = Resolve::resolve_key(editor, &catalog, editor.intern(b"OCProperties"));
        let configuration = Resolve::resolve_key(
            editor,
            properties.as_dict().expect("props"),
            editor.intern(b"D"),
        );
        let configuration = configuration.as_dict().expect("a /D").clone();
        let numbers = |key: &[u8]| -> Vec<u32> {
            editor
                .refs_in(configuration.get(editor.intern(key)))
                .iter()
                .map(|r| r.num)
                .collect()
        };
        (numbers(b"ON"), numbers(b"OFF"))
    }

    #[test]
    fn hiding_a_group_names_it_in_off_and_showing_it_takes_it_out() {
        let mut editor = editor("/OCProperties << /OCGs [10 0 R 11 0 R] /D << >> >>", "");
        assert!(editor.set_layer_visible(ObjRef::new(11, 0), false));
        assert_eq!(lists(&editor), (vec![], vec![11]));
        assert!(editor.set_layer_visible(ObjRef::new(10, 0), false));
        assert_eq!(lists(&editor), (vec![], vec![11, 10]));
        assert!(editor.set_layer_visible(ObjRef::new(11, 0), true));
        assert_eq!(lists(&editor), (vec![], vec![10]));
    }

    /// Under `/BaseState /OFF` showing a group is naming it in `/ON`.
    #[test]
    fn under_base_state_off_showing_a_group_names_it_in_on() {
        let mut editor = editor(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D 12 0 R >>",
            "12 0 obj\n<< /BaseState /OFF /OFF [10 0 R] >>\nendobj\n",
        );
        assert!(editor.set_layer_visible(ObjRef::new(10, 0), true));
        assert_eq!(lists(&editor), (vec![10], vec![]));
        // The indirect configuration was changed where it lives, and the
        // catalog was not touched.
        assert!(editor.overlay.contains_key(&12));
        assert!(!editor.overlay.contains_key(&1));
    }

    #[test]
    fn a_group_the_catalog_never_listed_is_refused() {
        let mut editor = editor("/OCProperties << /OCGs [10 0 R] /D << >> >>", "");
        assert!(!editor.set_layer_visible(ObjRef::new(11, 0), false));
        assert!(!editor.is_dirty());
        let mut none = editor_without_properties();
        assert!(!none.set_layer_visible(ObjRef::new(10, 0), false));
        assert!(!none.is_dirty());
    }

    fn editor_without_properties() -> DocumentEditor {
        editor("", "")
    }

    /// A list the change empties is removed, since `/OFF []` is a statement
    /// where the absence is not.
    #[test]
    fn an_emptied_list_is_removed_not_written_empty() {
        let mut editor = editor(
            "/OCProperties << /OCGs [10 0 R] /D << /OFF [10 0 R] >> >>",
            "",
        );
        assert!(editor.set_layer_visible(ObjRef::new(10, 0), true));
        let catalog = editor.catalog().expect("a catalog");
        let properties = Resolve::resolve_key(&editor, &catalog, editor.intern(b"OCProperties"));
        let configuration = Resolve::resolve_key(
            &editor,
            properties.as_dict().expect("props"),
            editor.intern(b"D"),
        );
        let configuration = configuration.as_dict().expect("a /D");
        assert!(configuration.get(editor.intern(b"OFF")).is_none());
    }
}
