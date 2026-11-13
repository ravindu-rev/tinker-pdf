//! Listing a document's optional content groups (8.11), behind
//! [`Document::layers`].
//!
//! [`Document::layers`]: crate::Document::layers
//!
//! The projection boundary is argued in [`crate::fontlist`]; this module is
//! the second of the three places it applies. Here it lands on the **owned**
//! side, and for a reason worth stating: `tinker_pdf_content::Layer` already
//! exists and is already on the facade, but it is the *renderer's* type — one
//! `Layer` per `/OC` occurrence in a content stream, carrying the label that
//! occurrence should be reported under, including the resource-name fallback
//! for a group with no `/Name`. A document's layer list is a different
//! question: it is the catalog's `/OCGs` array (8.11.4.2), each entry once,
//! with its own address. Reusing the renderer's type would mean a caller
//! could not tell a group listed by the catalog from a group only some
//! content mentioned, and those are exactly the two the reader already keeps
//! apart internally.
//!
//! **Nothing here is written**, and nothing needs to be: the write half is
//! `DocumentBuilder::add_layer` with `PageBuilder::optional`, and
//! `DocumentEditor::set_layer_visible`, which takes the
//! [`OptionalGroup::reference`] this list hands out. `writer_layers.rs` holds
//! the two halves to each other — a layer written, listed here and drawn.
//!
//! The list's companion is the tree a layer panel draws, a configuration's
//! `/Order` ([`LayerTree`]): the same groups, nested and labelled as the file
//! presents them, each with the state the same reader gives it.

use tinker_pdf_cos::{decode_text_string, CosDocument, ObjRef, Object};

use crate::optional::OptionalContent;
use crate::resources::{read_key, read_resolved};
use crate::LayerOptions;

/// One optional content group, as the catalog lists it (8.11.2.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionalGroup {
    /// The group dictionary's own reference.
    ///
    /// Always present: 8.11.4.2's `/OCGs` is an array of indirect references,
    /// and a group with no address is one no `/OC` could name.
    pub reference: ObjRef,
    /// `/Name`, decoded as a text string (8.11.2.1 Table 98).
    ///
    /// Required by the clause, and empty when a file omits it — which happens,
    /// and which is reported as an empty name rather than by dropping the
    /// group, because a caller toggling layers still has to see it.
    pub name: String,
    /// Whether the document's default configuration shows this group
    /// (8.11.4.3 Table 101).
    ///
    /// This is `/BaseState` as modified by `/ON` and `/OFF`, in that order,
    /// and `/D`'s `/Intent` (8.11.2.3): a group none of whose intents `/D`
    /// holds has no effect on visibility and is visible — the same answer the
    /// renderer acts on, from the same reader, so a caller cannot be told one
    /// thing here and shown another on the page.
    pub visible: bool,
}

/// The document's optional content groups, in `/OCProperties /OCGs` order.
///
/// Empty for the great majority of documents, which carry no `/OCProperties`
/// at all — an ordinary answer, not an error.
///
/// # Order
///
/// The catalog's own array order, deduplicated. Not sorted: `/OCGs` is the
/// order a producer wrote its layers in and it is what a layer panel shows.
/// Ruling 4 wants iteration order to be a decision rather than an accident,
/// and this is the decision.
pub(crate) fn of_document(doc: &CosDocument) -> Vec<OptionalGroup> {
    let content = OptionalContent::bind(doc);
    let mut out = Vec::new();
    for reference in content.listed() {
        let Ok(object) = doc.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let name = doc
            .resolve_key(dict, doc.intern(b"Name"))
            .as_string()
            .map(|s| decode_text_string(&s.bytes))
            .unwrap_or_default();
        out.push(OptionalGroup {
            reference,
            name,
            // **From the reader, never recomputed.** An inverted default
            // here — reporting `/OFF` as visible — is the injection
            // `a_group_turned_off_is_reported_off` catches, and the only way
            // to be sure the list and the page agree is for both to ask the
            // same object.
            visible: content.is_visible(reference),
        });
    }
    out
}

/// One alternate optional-content configuration, an entry of
/// `/OCProperties /Configs` (8.11.4.3 Table 99), as
/// [`crate::Document::layer_configurations`] lists it: what a render names by
/// its index ([`crate::LayerOptions::configuration`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerConfiguration {
    /// `/Name`, decoded as a text string: "A name for the configuration,
    /// suitable for presentation in an interactive PDF processor's user
    /// interface". `None` where the configuration has none.
    pub name: Option<String>,
    /// `/Creator`, decoded as a text string: "Name of the application or
    /// feature that created this configuration dictionary". `None` where it
    /// has none.
    pub creator: Option<String>,
}

/// The document's `/Configs` in their array's order, each a configuration's
/// name and creator or `None` for an entry that is no dictionary — the array
/// a render's configuration index reads, read as far (`MAX_CONFIGURATIONS`,
/// in the optional-content reader), so the two cannot disagree about an
/// index.
pub(crate) fn configurations_of(doc: &CosDocument) -> Vec<Option<LayerConfiguration>> {
    crate::optional::configurations(doc)
        .into_iter()
        .map(|entry| {
            entry.map(|dict| {
                let text = |key: &[u8]| {
                    doc.resolve_key(&dict, doc.intern(key))
                        .as_string()
                        .map(|s| decode_text_string(&s.bytes))
                };
                LayerConfiguration {
                    name: text(b"Name"),
                    creator: text(b"Creator"),
                }
            })
        })
        .collect()
}

/// How many arrays deep a configuration's `/Order` is read, the top-level
/// array the first: a visibility expression's bound
/// (`MAX_EXPRESSION_DEPTH`, in the optional-content reader).
const MAX_ORDER_DEPTH: u32 = 16;

/// How many nodes a configuration's tree holds in all.
const MAX_ORDER_NODES: usize = 4096;

/// A configuration's `/Order` as a tree (8.11.4.3 Table 99): "the order for
/// presentation of optional content groups in an interactive PDF processor's
/// user interface", as [`crate::Document::layer_tree`] reads it. Read only;
/// what is written is ED-40's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerTree {
    /// The top level, in the array's order. Empty for a configuration that
    /// presents no groups: `/D`'s `/Order` is "an empty array" by default,
    /// and "Any groups not listed in this array shall not be presented in
    /// any user interface that uses the configuration".
    pub nodes: Vec<LayerNode>,
    /// Whether the tree is cut: an array nested more than sixteen deep, the
    /// top level the first, or a node past the 4 096th, was not read, so
    /// there is more in the file than here.
    pub cut: bool,
}

/// One node of a [`LayerTree`] (8.11.4.3 Table 99, its two examples).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerNode {
    /// An optional content group, "whose Name entry shall be displayed in
    /// the user interface".
    Group {
        /// The group dictionary's reference, the one
        /// [`crate::Document::layers`] hands out.
        reference: ObjRef,
        /// Its `/Name`, decoded as a text string; empty where it has none.
        name: String,
        /// Whether the configuration shows it: its states with no usage and
        /// no override, `/Intent` read (8.11.2.3), as [`OptionalGroup::visible`]
        /// reports `/D`'s. A group `/OCGs` does not list is on, as every
        /// unlisted group is.
        visible: bool,
        /// Its sublayers: the nested array with no label that follows it in
        /// its array — "To reflect actual nesting of groups in the content,
        /// such as for layers with sublayers, nested arrays of groups without
        /// a text label shall be used".
        children: Vec<LayerNode>,
    },
    /// "a text string to be used as a non-selectable label": first in a
    /// nested array, the label of a collection of the array's other entries,
    /// its children — "to present collections of related optional content
    /// groups, and not to communicate actual nesting"; anywhere else, a label
    /// with no children, kept so that nothing the file presents is lost.
    Label {
        /// The text string, decoded.
        text: String,
        /// The rest of its array, where it labels one.
        children: Vec<LayerNode>,
    },
    /// A nested array with no label that follows no group: a node with no
    /// label of its own, its entries its children.
    Nested {
        /// The array's entries.
        children: Vec<LayerNode>,
    },
}

/// The tree configuration `configuration` presents — `None` for `/D`,
/// `Some(i)` for the `i`th entry of `/Configs` — read from its `/Order`, or
/// `/D`'s where it has none, its groups in that configuration's states with
/// no usage and no override. An index naming no configuration reads `/D`'s,
/// as a render naming it draws `/D`'s. Bounded in depth and in size, with
/// every cycle read once and every entry that is no node passed over
/// ([`OrderReader`]).
pub(crate) fn tree_of(doc: &CosDocument, configuration: Option<usize>) -> LayerTree {
    let order = crate::optional::order(doc, configuration);
    let states = OptionalContent::bind_with(
        doc,
        None,
        &LayerOptions {
            configuration,
            ..LayerOptions::default()
        },
        1.0,
    );
    let mut reader = OrderReader {
        doc,
        states: &states,
        nodes: 0,
        cut: false,
        open: Vec::new(),
    };
    let nodes = reader
        .array(&order, 1)
        .map(|(_, nodes)| nodes)
        .unwrap_or_default();
    LayerTree {
        nodes,
        cut: reader.cut,
    }
}

/// What an `/Order` entry is, read where it lies.
enum Entry {
    /// A nested array, or a reference to one.
    Array,
    /// A text string, decoded.
    Text(String),
    /// A group: a dictionary whose `/Type` is `/OCG`, reached by reference.
    Group(ObjRef, String),
    /// Anything else — a number, a name, a membership dictionary, a group
    /// written in place — which is no node and is passed over.
    Other,
}

/// Reads one configuration's `/Order` into its tree (8.11.4.3), bounded:
/// [`MAX_ORDER_DEPTH`] arrays deep and [`MAX_ORDER_NODES`] nodes in all, an
/// array past either not read and the tree marked cut, and an array reached
/// by a reference while that reference is being read — a cycle — not read a
/// second time. Nothing in it can panic or loop.
struct OrderReader<'a> {
    doc: &'a CosDocument,
    /// The configuration's states, which give each group its `visible`.
    states: &'a OptionalContent,
    /// The nodes made so far.
    nodes: usize,
    /// Whether a bound stopped the reading.
    cut: bool,
    /// The references of the arrays being read, outermost first.
    open: Vec<ObjRef>,
}

impl OrderReader<'_> {
    /// The array `value` is, or refers to, at `depth`, read: its label, where
    /// it is a nested array whose first entry is a text string, and its other
    /// entries as nodes. `None` for a value that is no array, an array whose
    /// reference is being read already, and one past [`MAX_ORDER_DEPTH`],
    /// which cuts the tree.
    fn array(&mut self, value: &Object, depth: u32) -> Option<(Option<String>, Vec<LayerNode>)> {
        let reference = value.as_objref();
        if reference.is_some_and(|reference| self.open.contains(&reference)) {
            return None;
        }
        let doc = self.doc;
        read_resolved(doc, value, |resolved| {
            let items = resolved.as_array()?;
            if depth > MAX_ORDER_DEPTH {
                self.cut = true;
                return None;
            }
            if let Some(reference) = reference {
                self.open.push(reference);
            }
            let read = self.entries(items, depth);
            if reference.is_some() {
                self.open.pop();
            }
            Some(read)
        })
    }

    /// The entries of an array at `depth`: the label its first entry gives
    /// it, for a nested array, and its nodes in order.
    fn entries(&mut self, items: &[Object], depth: u32) -> (Option<String>, Vec<LayerNode>) {
        let mut label = None;
        let mut nodes: Vec<LayerNode> = Vec::new();
        // The group just read, which a nested array with no label after it
        // gives its sublayers to; any other node between closes it.
        let mut parent: Option<usize> = None;
        for (position, item) in items.iter().enumerate() {
            if self.nodes >= MAX_ORDER_NODES {
                self.cut = true;
                break;
            }
            match self.entry(item) {
                Entry::Other => {}
                Entry::Text(text) if position == 0 && depth > 1 => label = Some(text),
                Entry::Text(text) => {
                    self.nodes += 1;
                    nodes.push(LayerNode::Label {
                        text,
                        children: Vec::new(),
                    });
                    parent = None;
                }
                Entry::Group(reference, name) => {
                    self.nodes += 1;
                    nodes.push(LayerNode::Group {
                        reference,
                        visible: self.states.is_visible(reference),
                        name,
                        children: Vec::new(),
                    });
                    parent = Some(nodes.len() - 1);
                }
                Entry::Array => {
                    // Counted before what it holds, so the bound holds
                    // whatever it turns out to be, and given back where it is
                    // a group's sublayers or is not read.
                    let sublayers = parent.take();
                    self.nodes += 1;
                    let Some((text, children)) = self.array(item, depth + 1) else {
                        self.nodes -= 1;
                        continue;
                    };
                    match (text, sublayers) {
                        (Some(text), _) => nodes.push(LayerNode::Label { text, children }),
                        (None, Some(at)) => {
                            self.nodes -= 1;
                            if let Some(LayerNode::Group { children: held, .. }) = nodes.get_mut(at)
                            {
                                *held = children;
                            }
                        }
                        (None, None) => nodes.push(LayerNode::Nested { children }),
                    }
                }
            }
        }
        (label, nodes)
    }

    /// What `item` is, read where it lies.
    fn entry(&self, item: &Object) -> Entry {
        let doc = self.doc;
        let reference = item.as_objref();
        read_resolved(doc, item, |resolved| {
            if resolved.as_array().is_some() {
                return Entry::Array;
            }
            if let Some(text) = resolved.as_string() {
                return Entry::Text(decode_text_string(&text.bytes));
            }
            let (Some(reference), Some(dict)) = (reference, resolved.as_dict()) else {
                return Entry::Other;
            };
            let group = read_key(doc, dict, b"Type", |kind| {
                kind.as_name()
                    .and_then(|name| doc.name_bytes(name))
                    .is_some_and(|name| name.as_ref() == b"OCG")
            });
            if !group {
                return Entry::Other;
            }
            let name = read_key(doc, dict, b"Name", |name| {
                name.as_string().map(|s| decode_text_string(&s.bytes))
            })
            .unwrap_or_default();
            Entry::Group(reference, name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(properties: &str, extra: &str) -> crate::Document {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R {properties} >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>\nendobj\n\
10 0 obj\n<< /Type /OCG /Name (Alpha) >>\nendobj\n\
11 0 obj\n<< /Type /OCG /Name (Beta) >>\nendobj\n\
{extra}\
trailer\n<< /Size 30 /Root 1 0 R >>\n%%EOF\n"
        );
        crate::Document::open(bytes.into_bytes()).expect("it opens")
    }

    /// The ordinary document: no `/OCProperties`, no layers, no error.
    #[test]
    fn a_document_without_optional_content_has_no_layers() {
        assert!(document("", "").layers().is_empty());
    }

    /// 8.11.4.3 Table 101: `/OFF` names the groups the default configuration
    /// hides, and the list says so by name.
    #[test]
    fn a_group_turned_off_is_reported_off() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> >>",
            "",
        );
        let layers = doc.layers();
        assert_eq!(layers.len(), 2);

        assert_eq!(layers[0].name, "Alpha");
        assert!(layers[0].visible, "a group /OFF does not name is on");
        assert_eq!(layers[1].name, "Beta");
        assert!(!layers[1].visible, "a group named by /OFF is off");
    }

    /// `/BaseState /OFF` turns every listed group off, and `/ON` names the
    /// exceptions (8.11.4.3 Table 101).
    #[test]
    fn base_state_off_inverts_the_default() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] \
             /D << /BaseState /OFF /ON [10 0 R] >> >>",
            "",
        );
        let layers = doc.layers();
        assert_eq!(layers.len(), 2);
        assert!(layers[0].visible, "/ON names the exception");
        assert!(!layers[1].visible, "/BaseState /OFF turns the rest off");
    }

    /// The list is `/OCGs` order, which is the order a layer panel shows.
    #[test]
    fn the_order_is_the_catalogs_own() {
        let doc = document("/OCProperties << /OCGs [11 0 R 10 0 R] /D << >> >>", "");
        let names: Vec<String> = doc.layers().into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["Beta", "Alpha"]);
    }

    /// A group with no `/Name` is listed with an empty one rather than
    /// dropped: 8.11.2.1 requires the entry, and a file that omits it still
    /// has a layer a caller has to be able to see.
    #[test]
    fn a_nameless_group_is_still_listed() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 12 0 R] /D << >> >>",
            "12 0 obj\n<< /Type /OCG >>\nendobj\n",
        );
        let layers = doc.layers();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[1].name, "");
        assert_eq!(layers[1].reference.num, 12);
    }

    /// **The configurations are listed in `/Configs` order** (8.11.4.3): each
    /// entry's `/Name` and `/Creator` decoded as text strings, an entry
    /// reached by reference read through it, one with neither key listed
    /// with neither, and one that is no dictionary listed as `None`, so that
    /// an index read off the list is the index a render takes; a document
    /// with no `/Configs`, or none that is an array, lists none.
    #[test]
    fn the_configurations_are_listed_in_configs_order() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << >> \
             /Configs [<< /Name (Print view) /Creator (Producer) >> 20 0 R 7 << >> \
             << /Name <FEFF00C9> >>] >>",
            "20 0 obj\n<< /Name (By reference) /BaseState /OFF >>\nendobj\n",
        );
        let named = |name: &str, creator: Option<&str>| {
            Some(LayerConfiguration {
                name: Some(name.to_string()),
                creator: creator.map(str::to_string),
            })
        };
        assert_eq!(
            doc.layer_configurations(),
            vec![
                named("Print view", Some("Producer")),
                named("By reference", None),
                None,
                Some(LayerConfiguration {
                    name: None,
                    creator: None,
                }),
                named("\u{c9}", None),
            ]
        );
        for properties in [
            "/OCProperties << /OCGs [10 0 R] /D << >> >>",
            "/OCProperties << /OCGs [10 0 R] /D << >> /Configs 5 >>",
            "",
        ] {
            assert!(
                document(properties, "").layer_configurations().is_empty(),
                "{properties}"
            );
        }
    }

    /// The listing and the renderer read the same configuration.
    ///
    /// Not a round trip: both sides are the one `OptionalContent::bind`, and
    /// this test's job is to keep it that way — the failure it guards against
    /// is a later change that recomputes visibility here from the raw
    /// dictionary, which is how a list and a page come to disagree.
    #[test]
    fn the_listing_is_the_renderers_own_configuration() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [10 0 R 11 0 R] >> >>",
            "",
        );
        let page = doc.page(0).expect("a page");
        let bound = OptionalContent::bind(&page.doc);
        for layer in doc.layers() {
            assert_eq!(layer.visible, bound.is_visible(layer.reference));
        }
    }

    /// A group node, by object number.
    fn group(num: u32, name: &str, visible: bool, children: Vec<LayerNode>) -> LayerNode {
        LayerNode::Group {
            reference: ObjRef { num, gen: 0 },
            name: name.to_string(),
            visible,
            children,
        }
    }

    /// A label node.
    fn label(text: &str, children: Vec<LayerNode>) -> LayerNode {
        LayerNode::Label {
            text: text.to_string(),
            children,
        }
    }

    /// The tree whole: `nodes`, not cut.
    fn whole(nodes: Vec<LayerNode>) -> LayerTree {
        LayerTree { nodes, cut: false }
    }

    /// **The order tree is the configuration's own** (RC-14.5, 8.11.4.3 Table
    /// 99): each `/Order` below gives its known tree. The clause's two
    /// examples — labelled collections, and sublayers in an unlabelled array
    /// after their group; an unlabelled array after no group, after a label
    /// or after a group's sublayers, and after entries passed over; a
    /// labelled array after a group; a text string elsewhere; a group listed
    /// twice, one `/OCGs` does not list, and names decoded or absent;
    /// entries that are no node; an array nested past the bound, a tree past
    /// its node bound, an `/Order` containing itself directly and through
    /// another array; a configuration without `/Order` reading `/D`'s in its
    /// own states, an index naming no configuration reading `/D`'s, and a
    /// `/D` without `/Order`, which presents nothing.
    #[test]
    fn the_order_tree_is_the_configurations_own() {
        let groups = "12 0 obj\n<< /Type /OCG /Name <FEFF00C9> >>\nendobj\n\
                      13 0 obj\n<< /Type /OCG >>\nendobj\n\
                      14 0 obj\n<< /Type /OCMD /OCGs [10 0 R] >>\nendobj\n";
        let tree = |order: &str, extra: &str| {
            document(
                &format!(
                    "/OCProperties << /OCGs [10 0 R 11 0 R 13 0 R] \
                     /D << /OFF [11 0 R] /Order {order} >> >>"
                ),
                &format!("{groups}{extra}"),
            )
            .layer_tree(None)
        };
        let alpha = |children| group(10, "Alpha", true, children);
        let beta = |children| group(11, "Beta", false, children);
        let cases: Vec<(&str, &str, LayerTree)> = vec![
            (
                "[[(Frog Anatomy) 10 0 R 11 0 R] [(Tree Anatomy) 12 0 R 13 0 R]]",
                "",
                whole(vec![
                    label("Frog Anatomy", vec![alpha(vec![]), beta(vec![])]),
                    label(
                        "Tree Anatomy",
                        vec![
                            group(12, "\u{c9}", true, vec![]),
                            group(13, "", true, vec![]),
                        ],
                    ),
                ]),
            ),
            (
                "[10 0 R [11 0 R 13 0 R]]",
                "",
                whole(vec![alpha(vec![beta(vec![]), group(13, "", true, vec![])])]),
            ),
            (
                "[[10 0 R 11 0 R]]",
                "",
                whole(vec![LayerNode::Nested {
                    children: vec![alpha(vec![]), beta(vec![])],
                }]),
            ),
            (
                "[10 0 R [11 0 R] [13 0 R]]",
                "",
                whole(vec![
                    alpha(vec![beta(vec![])]),
                    LayerNode::Nested {
                        children: vec![group(13, "", true, vec![])],
                    },
                ]),
            ),
            (
                "[10 0 R (Note) [11 0 R]]",
                "",
                whole(vec![
                    alpha(vec![]),
                    label("Note", vec![]),
                    LayerNode::Nested {
                        children: vec![beta(vec![])],
                    },
                ]),
            ),
            (
                "[10 0 R 5 /Skip [11 0 R]]",
                "",
                whole(vec![alpha(vec![beta(vec![])])]),
            ),
            (
                "[10 0 R [(Grouped) 11 0 R]]",
                "",
                whole(vec![alpha(vec![]), label("Grouped", vec![beta(vec![])])]),
            ),
            (
                "[(Top) 10 0 R [(Inner) (Second) 11 0 R]]",
                "",
                whole(vec![
                    label("Top", vec![]),
                    alpha(vec![]),
                    label("Inner", vec![label("Second", vec![]), beta(vec![])]),
                ]),
            ),
            (
                "[10 0 R 11 0 R 10 0 R]",
                "",
                whole(vec![alpha(vec![]), beta(vec![]), alpha(vec![])]),
            ),
            (
                "[5 2.5 /Name true 14 0 R << /Type /OCG /Name (Inline) >> 99 0 R [] 10 0 R]",
                "",
                whole(vec![LayerNode::Nested { children: vec![] }, alpha(vec![])]),
            ),
            (
                "[20 0 R 21 0 R]",
                "20 0 obj\n[10 0 R]\nendobj\n21 0 obj\n(By reference)\nendobj\n",
                whole(vec![
                    LayerNode::Nested {
                        children: vec![alpha(vec![])],
                    },
                    label("By reference", vec![]),
                ]),
            ),
            (
                "20 0 R",
                "20 0 obj\n[10 0 R 20 0 R 11 0 R]\nendobj\n",
                whole(vec![alpha(vec![]), beta(vec![])]),
            ),
            (
                "20 0 R",
                "20 0 obj\n[10 0 R 21 0 R]\nendobj\n21 0 obj\n[(Loop) 20 0 R 11 0 R]\nendobj\n",
                whole(vec![alpha(vec![]), label("Loop", vec![beta(vec![])])]),
            ),
            ("[]", "", whole(vec![])),
            ("5", "", whole(vec![])),
        ];
        for (order, extra, want) in cases {
            assert_eq!(tree(order, extra), want, "{order}");
        }

        // Twenty arrays deep: the top level and fifteen nested arrays are
        // read, the one past them is not, and the tree says so.
        let deep = format!("{}10 0 R{}", "[".repeat(20), "]".repeat(20));
        let mut want = LayerNode::Nested { children: vec![] };
        for _ in 0..14 {
            want = LayerNode::Nested {
                children: vec![want],
            };
        }
        assert_eq!(
            tree(&deep, ""),
            LayerTree {
                nodes: vec![want],
                cut: true
            },
            "sixteen arrays deep"
        );
        let sixteen = format!("{}10 0 R{}", "[".repeat(16), "]".repeat(16));
        assert!(!tree(&sixteen, "").cut, "sixteen deep is whole");

        // Five thousand entries: the first 4 096 are nodes.
        let wide = format!("[{}]", "10 0 R ".repeat(5000));
        let tree_wide = tree(&wide, "");
        assert!(tree_wide.cut, "past the node bound");
        assert_eq!(tree_wide.nodes.len(), 4096);
        let nested_wide = format!("[[{}] 11 0 R]", "10 0 R ".repeat(5000));
        let tree_nested = tree(&nested_wide, "");
        assert!(tree_nested.cut);
        match tree_nested.nodes.as_slice() {
            [LayerNode::Nested { children }] => assert_eq!(children.len(), 4095),
            other => panic!("one nested node, its array cut: {other:?}"),
        }

        // A configuration's own states over `/D`'s `/Order`, and an `/Order`
        // of its own; an unlisted group is on.
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] \
             /D << /OFF [11 0 R] /Order [10 0 R 11 0 R 12 0 R] >> \
             /Configs [<< /BaseState /OFF >> << /Order [11 0 R] /ON [11 0 R] >> \
             << /Order null >> 7] >>",
            groups,
        );
        let unlisted = group(12, "\u{c9}", true, vec![]);
        assert_eq!(
            doc.layer_tree(None),
            whole(vec![alpha(vec![]), beta(vec![]), unlisted.clone()])
        );
        assert_eq!(
            doc.layer_tree(Some(0)),
            whole(vec![
                group(10, "Alpha", false, vec![]),
                beta(vec![]),
                unlisted.clone(),
            ]),
            "/D's /Order in the configuration's states"
        );
        assert_eq!(
            doc.layer_tree(Some(1)),
            whole(vec![group(11, "Beta", true, vec![])]),
            "its own /Order"
        );
        assert_eq!(
            doc.layer_tree(Some(2)),
            whole(vec![
                group(10, "Alpha", true, vec![]),
                group(11, "Beta", true, vec![]),
                unlisted.clone()
            ]),
            "a null /Order is an absent one"
        );
        for index in [3, 4, usize::MAX] {
            assert_eq!(
                doc.layer_tree(Some(index)),
                doc.layer_tree(None),
                "configuration {index} names none, and reads /D's"
            );
        }
        let doc = document("/OCProperties << /OCGs [10 0 R] /D << >> >>", "");
        assert_eq!(doc.layer_tree(None), LayerTree::default(), "no /Order");
        assert_eq!(document("", "").layer_tree(None), LayerTree::default());
    }
}
