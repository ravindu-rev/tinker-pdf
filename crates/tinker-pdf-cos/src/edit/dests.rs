//! Named destinations (12.3.2.3) on an existing document.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::DocumentEditor;
use crate::dest::DestKind;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::resolve::Resolve;
use crate::trees;

impl DocumentEditor {
    /// Adds a named destination: `name` resolves to the page at zero-based
    /// `page` in this editor's page order, positioned as `view` says.
    ///
    /// The catalog's `/Names /Dests` tree (7.7.4, 7.9.6) is rewritten as a new
    /// tree holding every entry the old one held plus this one, through the
    /// same tree writer [`crate::build::DocumentBuilder`] uses; the old nodes
    /// are left where they are, unreferenced, so an incremental save touches
    /// the `/Names` dictionary and adds the new nodes and changes nothing
    /// else. A `/Names` dictionary that is an indirect object is replaced at
    /// its own number, so a catalog that shares it is not copied.
    ///
    /// The value written is the explicit destination array, with the page as
    /// an indirect reference — which is the only spelling that survives this
    /// editor's own page reordering.
    ///
    /// Returns false, changing nothing, for an empty name, a name the tree
    /// already holds (7.9.6 maps each key to one value, and which of two the
    /// caller meant is theirs to decide), a page that does not exist, a view
    /// that cannot be written ([`DestKind::is_writable`]), a document with no
    /// catalog, or a tree that would outgrow what this repository's own reader
    /// walks.
    pub fn add_named_destination(&mut self, name: &[u8], page: u32, view: DestKind) -> bool {
        if name.is_empty() || !view.is_writable() {
            return false;
        }
        let Some(page_ref) = self.page_refs().get(page as usize).copied() else {
            return false;
        };
        let Some(catalog) = self.catalog() else {
            return false;
        };

        let names_key = self.intern(b"Names");
        let dests_key = self.intern(b"Dests");

        // The `/Names` dictionary as it stands, and where it lives.
        let (names_ref, mut names_dict) = match catalog.get(names_key) {
            Some(Object::Ref(r)) => {
                let resolved = Resolve::get(self, *r).ok();
                (
                    Some(*r),
                    resolved
                        .as_deref()
                        .and_then(Object::as_dict)
                        .cloned()
                        .unwrap_or_default(),
                )
            }
            Some(Object::Dict(dict)) => (None, dict.clone()),
            _ => (None, Dict::new()),
        };

        let mut entries = self.dests_entries(&names_dict, dests_key);
        if entries.iter().any(|(key, _)| key.as_slice() == name) {
            return false;
        }
        let doc = Arc::clone(&self.doc);
        entries.push((
            name.to_vec(),
            crate::dest::destination_array(doc.names_table(), page_ref, &view),
        ));

        // Every refusal the tree writer can make is made before it writes a
        // node, so an `Err` here leaves the editor exactly as it was.
        let Ok(root) = self.add_name_tree(entries) else {
            return false;
        };
        names_dict.insert(dests_key, Object::Ref(root));
        match names_ref {
            Some(reference) => {
                self.put(reference, Object::Dict(names_dict));
                true
            }
            None => self.update_catalog(|catalog| {
                catalog.insert(names_key, Object::Dict(names_dict));
            }),
        }
    }

    /// The names the catalog's `/Names /Dests` tree holds as this editor has
    /// it, in the shape [`crate::build::Target::write`] asks for.
    ///
    /// Only the keys are consulted there: a named target is written when its
    /// name is held and dropped as dangling when it is not, the rule
    /// [`crate::build::DocumentBuilder`] applies to its own tree. The page and
    /// view filed beside each key here are placeholders for that reason.
    pub(super) fn live_destination_names(&self) -> BTreeMap<Vec<u8>, (u32, DestKind)> {
        let Some(catalog) = self.catalog() else {
            return BTreeMap::new();
        };
        let names_key = self.intern(b"Names");
        let dests_key = self.intern(b"Dests");
        let names = match catalog.get(names_key) {
            Some(Object::Ref(r)) => Resolve::get(self, *r)
                .ok()
                .as_deref()
                .and_then(Object::as_dict)
                .cloned()
                .unwrap_or_default(),
            Some(Object::Dict(d)) => d.clone(),
            _ => Dict::default(),
        };
        self.dests_entries(&names, dests_key)
            .into_iter()
            .map(|(key, _)| (key, (0, DestKind::Fit)))
            .collect()
    }

    /// Every entry of the `/Dests` tree under `names`, as this editor has it.
    ///
    /// The tree's root is almost always indirect, which is the shape the tree
    /// reader takes; a root written inline in `/Names` has its own leaf pairs
    /// read here and its kids walked by the reader, so no entry is lost to the
    /// spelling.
    fn dests_entries(&self, names: &Dict, dests_key: Name) -> Vec<(Vec<u8>, Object)> {
        match names.get(dests_key) {
            Some(Object::Ref(root)) => trees::name_tree_in(self, *root),
            Some(Object::Dict(root)) => {
                let mut out = Vec::new();
                if let Some(pairs) = root.get_array(self.intern(b"Names")) {
                    for pair in pairs.chunks_exact(2) {
                        if let [Object::String(key), value] = pair {
                            out.push((key.bytes.clone(), value.clone()));
                        }
                    }
                }
                if let Some(kids) = root.get_array(Name::KIDS) {
                    let kids: Vec<ObjRef> = kids.iter().filter_map(Object::as_objref).collect();
                    for kid in kids {
                        out.extend(trees::name_tree_in(self, kid));
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }
}
