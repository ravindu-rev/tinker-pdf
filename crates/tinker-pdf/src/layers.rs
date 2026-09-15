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
//! **Nothing here is written.** `DocumentBuilder::add_layer` and the editor's
//! default-configuration toggle are the other half of the roadmap's optional
//! content row and are left for their own commit; this is the read half.

use tinker_pdf_cos::{decode_text_string, CosDocument, ObjRef};

use crate::optional::OptionalContent;

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
    /// This is `/BaseState` as modified by `/ON` and `/OFF`, in that order —
    /// the same answer the renderer acts on, from the same reader, so a
    /// caller cannot be told one thing here and shown another on the page.
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
}
