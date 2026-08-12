//! Optional content: which layers a document's default configuration shows
//! (8.11).
//!
//! A layer is how a file carries a CAD drawing's construction lines, a map's
//! alternate languages, a proof's annotation overlay. The catalog's
//! `/OCProperties` `/D` configuration says which of them a reader shows when
//! it opens the file with no further instruction, and that is the whole of
//! what this module answers. Toggling one afterwards is a separate feature —
//! `docs/plans/08-rendering-device.md` defers it — and nothing here mutates.
//!
//! # Everything not understood is visible
//!
//! Every fallback in this file resolves to *visible*, and the direction is
//! deliberate rather than lazy. Content wrongly shown is a reader's to ignore;
//! content wrongly hidden is invisible to them, so a misparse that hides is
//! unrecoverable from the outside. A malformed `/VE`, a `/P` this build does
//! not know, an `/OCGs` naming a group the catalog never listed, a reference
//! cycle — each one renders.
//!
//! Scope and exit criteria: `docs/plans/gaps/06-optional-content.md`.

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_content::Layer;
use tinker_pdf_cos::{decode_text_string, CosDocument, Dict, ObjRef, Object};

/// How deep a `/VE` visibility expression may nest before it is refused
/// (8.11.2.3).
///
/// A refusal means *visible*, so an adversarial expression costs a bounded
/// walk and shows its content rather than hiding it.
const MAX_EXPRESSION_DEPTH: u32 = 16;

/// How many members of a `/VE` or `/OCGs` list are examined.
const MAX_OPERANDS: usize = 256;

/// The document's default optional-content configuration, resolved once.
///
/// Built at bind time from the catalog, not consulted per operator: the
/// answer for a given group cannot change during a render, because this build
/// has no layer-toggle API to change it with.
#[derive(Debug, Default)]
pub(crate) struct OptionalContent {
    /// Every group the configuration has an opinion about, and that opinion.
    ///
    /// A `BTreeMap` rather than a `HashMap` because ruling 4 makes iteration
    /// order a correctness question everywhere in this engine, and a map that
    /// is only ever looked up today is a map somebody iterates tomorrow.
    states: BTreeMap<ObjRef, bool>,
}

impl OptionalContent {
    /// Reads the catalog's `/OCProperties` `/D` configuration (8.11.4.3).
    ///
    /// A document without `/OCProperties` — which is nearly all of them —
    /// yields an empty set, and an empty set says every layer is visible.
    pub(crate) fn bind(doc: &CosDocument) -> OptionalContent {
        let mut states = BTreeMap::new();

        let Some(catalog) = doc.catalog() else {
            return OptionalContent { states };
        };
        let entry = doc.resolve_key(&catalog, doc.intern(b"OCProperties"));
        let Some(properties) = entry.as_dict() else {
            return OptionalContent { states };
        };

        // 8.11.4.2: /OCGs lists every group in the document, and /D is the
        // configuration a reader applies when it opens the file.
        let all = refs_of(doc, properties, b"OCGs");
        let configuration = doc.resolve_key(properties, doc.intern(b"D"));
        let default = configuration.as_dict().cloned().unwrap_or_default();

        // 8.11.4.3 Table 101: /BaseState sets every group listed in /OCGs,
        // and /ON and /OFF then name the exceptions. Its default is /ON, so a
        // configuration that says nothing shows everything.
        let base_off = default
            .get_name(doc.intern(b"BaseState"))
            .and_then(|n| doc.name_bytes(n))
            .is_some_and(|n| n.as_ref() == b"OFF");
        for reference in &all {
            states.insert(*reference, !base_off);
        }

        // The order is /ON then /OFF. The spec does not say what a group named
        // by both means; applying /OFF last is what every reader does, and it
        // is also the reading that keeps a file's intent to hide something
        // legible rather than accidental.
        for reference in refs_of(doc, &default, b"ON") {
            states.insert(reference, true);
        }
        for reference in refs_of(doc, &default, b"OFF") {
            states.insert(reference, false);
        }

        OptionalContent { states }
    }

    /// Whether the group at `reference` is on.
    ///
    /// `None` for a group the configuration never mentioned — which is not
    /// the same as off, and must not be treated as off.
    fn group_state(&self, reference: ObjRef) -> Option<bool> {
        self.states.get(&reference).copied()
    }

    /// Resolves an `/OC` entry — a group, a membership dictionary, or
    /// something neither — into what a warning can name and a device can act
    /// on.
    ///
    /// `fallback` labels the layer when the dictionary has no `/Name` of its
    /// own: the resource name is what the file called it, and a warning that
    /// cannot say which layer it hid is not actionable (ruling 10).
    pub(crate) fn layer_of(&self, doc: &CosDocument, object: &Object, fallback: &str) -> Layer {
        let reference = object.as_objref();
        let resolved = doc.resolve(object);
        let Some(dict) = resolved.as_dict() else {
            // Not a dictionary at all: nothing to hide by.
            return Layer {
                visible: true,
                label: fallback.to_string(),
            };
        };

        let name = doc.resolve_key(dict, doc.intern(b"Name"));
        let label = name
            .as_string()
            .map(|s| decode_text_string(&s.bytes))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback.to_string());

        let type_entry = doc.resolve_key(dict, doc.intern(b"Type"));
        let kind = type_entry
            .as_name()
            .and_then(|n| doc.name_bytes(n))
            .map(|n| n.to_vec());

        let visible = match kind.as_deref() {
            // 8.11.2.2: a membership dictionary combines groups.
            Some(b"OCMD") => self.membership_visible(doc, dict),
            // 8.11.2.1: a group is on or off on its own. An /OC that is a
            // direct dictionary rather than a reference cannot be matched
            // against the configuration, and so is visible.
            _ => reference.and_then(|r| self.group_state(r)).unwrap_or(true),
        };

        Layer { visible, label }
    }

    /// 8.11.2.2: an optional content membership dictionary.
    fn membership_visible(&self, doc: &CosDocument, dict: &Dict) -> bool {
        // 8.11.2.3: "If this entry is present, the /OCGs and /P entries shall
        // be ignored" — /VE is the whole answer when it is there and is
        // understood. An expression this build cannot read falls back to
        // /OCGs and /P rather than to nothing, because a file that wrote both
        // wrote the second as the simpler statement of the first.
        let expression = doc.resolve_key(dict, doc.intern(b"VE"));
        if let Some(items) = expression.as_array() {
            if let Some(value) = self.evaluate(doc, items, 0) {
                return value;
            }
        }

        let groups = refs_of(doc, dict, b"OCGs");
        // 8.11.2.2: a membership dictionary with no groups constrains
        // nothing, so what it encloses is visible.
        if groups.is_empty() {
            return true;
        }

        // A group the configuration never listed is a hole in the expression,
        // and a hole resolves to visible rather than to a guess. Deciding it
        // is "off" would hide content on a file that merely forgot to list a
        // group in /OCProperties /OCGs, which is a common malformation.
        let mut states = Vec::with_capacity(groups.len());
        for reference in groups {
            match self.group_state(reference) {
                Some(state) => states.push(state),
                None => return true,
            }
        }

        let entry = doc.resolve_key(dict, doc.intern(b"P"));
        let policy = entry
            .as_name()
            .and_then(|n| doc.name_bytes(n))
            .map(|n| n.to_vec());

        // 8.11.2.2 Table 99. Two of these read backwards to anyone who has
        // not just read the table: /AnyOff is visible when *any* group is
        // OFF, and /AllOff when *every* one is. They are how a file says
        // "show this instead" — the alternate-language layer, the
        // construction lines' explanatory note — so implementing them as the
        // negation of the /AnyOn pair hides exactly the content that was
        // meant to appear when a layer was turned off.
        match policy.as_deref() {
            // The default (Table 99), which is what a missing /P means.
            None | Some(b"AnyOn") => states.iter().any(|on| *on),
            Some(b"AllOn") => states.iter().all(|on| *on),
            Some(b"AnyOff") => states.iter().any(|on| !*on),
            Some(b"AllOff") => states.iter().all(|on| !*on),
            // A policy from a later specification, or a typo. Falling back to
            // the /AnyOn default would be a guess that can hide content; this
            // is the same guess made in the direction that cannot.
            Some(_) => true,
        }
    }

    /// 8.11.2.3: a visibility expression, `[/Not ve]`, `[/And ve…]` or
    /// `[/Or ve…]`, whose leaves are optional content groups.
    ///
    /// `None` means the expression was not understood — a missing operator, a
    /// `/Not` with the wrong number of operands, a group the configuration
    /// never listed, a nesting depth that ran away — and every caller turns
    /// that into visible.
    fn evaluate(&self, doc: &CosDocument, items: &[Object], depth: u32) -> Option<bool> {
        if depth > MAX_EXPRESSION_DEPTH {
            return None;
        }

        let head = doc.resolve(items.first()?);
        let operator = head
            .as_name()
            .and_then(|n| doc.name_bytes(n))
            .map(|n| n.to_vec())?;
        let operands = items.get(1..)?;
        if operands.is_empty() || operands.len() > MAX_OPERANDS {
            return None;
        }

        let mut values = Vec::with_capacity(operands.len());
        for operand in operands {
            // An operand is either a nested expression or a group reference.
            // A nested *array* is only ever an expression: 8.11.2.3 has no
            // other array-shaped leaf.
            let resolved = doc.resolve(operand);
            if let Some(nested) = resolved.as_array() {
                values.push(self.evaluate(doc, nested, depth + 1)?);
                continue;
            }
            values.push(self.group_state(operand.as_objref()?)?);
        }

        match operator.as_slice() {
            // 8.11.2.3: /Not takes exactly one operand. Two would be a file
            // saying something this build cannot read, not a NAND.
            b"Not" => match values.as_slice() {
                [only] => Some(!*only),
                _ => None,
            },
            b"And" => Some(values.iter().all(|v| *v)),
            b"Or" => Some(values.iter().any(|v| *v)),
            _ => None,
        }
    }
}

/// The indirect references an array-valued key holds, deduplicated and
/// bounded.
///
/// A single reference where an array was expected is accepted: `/OCGs 5 0 R`
/// is legal in a membership dictionary (8.11.2.2) and appears in the wild for
/// the configuration arrays too.
fn refs_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Vec<ObjRef> {
    let value = dict.get(doc.intern(key)).cloned().unwrap_or(Object::Null);
    if let Some(reference) = value.as_objref() {
        // The key may point at the array indirectly, or *be* the one group.
        let resolved = doc.resolve(&value);
        let Some(items) = resolved.as_array() else {
            return vec![reference];
        };
        return collect(items);
    }
    match value.as_array() {
        Some(items) => collect(items),
        None => Vec::new(),
    }
}

fn collect(items: &[Object]) -> Vec<ObjRef> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in items.iter().take(MAX_OPERANDS) {
        if let Some(reference) = item.as_objref() {
            if seen.insert(reference) {
                out.push(reference);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::OptionalContent;
    use std::sync::Arc;
    use tinker_pdf_cos::{CosDocument, ObjRef, Object};

    /// A document whose catalog carries the given `/OCProperties`, plus two
    /// groups at 10 0 R and 11 0 R.
    ///
    /// `extra` is spliced in as further objects, so a test can add whatever
    /// membership dictionary it is about — 12 0 R by convention below.
    fn document(properties: &str, extra: &str) -> Arc<CosDocument> {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R {properties} >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>\nendobj\n\
10 0 obj\n<< /Type /OCG /Name (Alpha) >>\nendobj\n\
11 0 obj\n<< /Type /OCG /Name (Beta) >>\nendobj\n\
{extra}\
trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        page.doc.clone()
    }

    fn objref(num: u32) -> Object {
        Object::Ref(ObjRef { num, gen: 0 })
    }

    /// M1's exit criterion: a fixture with two groups, one `/OFF`, reports
    /// the right set.
    #[test]
    fn the_default_configuration_decides_each_group() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> >>",
            "",
        );
        let oc = OptionalContent::bind(&doc);

        assert!(
            oc.layer_of(&doc, &objref(10), "?").visible,
            "the group /OFF does not name stays on"
        );
        assert!(
            !oc.layer_of(&doc, &objref(11), "?").visible,
            "and the one it names goes off"
        );
    }

    /// A document with no `/OCProperties` at all — which is nearly all of
    /// them — shows everything, including a reference that names nothing.
    #[test]
    fn a_document_without_optional_content_shows_everything() {
        let doc = document("", "");
        let oc = OptionalContent::bind(&doc);

        assert!(oc.layer_of(&doc, &objref(10), "?").visible);
        assert!(oc.layer_of(&doc, &objref(99), "?").visible);
    }

    /// 8.11.4.3: `/BaseState /OFF` turns every listed group off, and `/ON`
    /// names the exceptions.
    #[test]
    fn base_state_off_inverts_the_starting_point() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] \
             /D << /BaseState /OFF /ON [10 0 R] >> >>",
            "",
        );
        let oc = OptionalContent::bind(&doc);

        assert!(oc.layer_of(&doc, &objref(10), "?").visible, "named by /ON");
        assert!(
            !oc.layer_of(&doc, &objref(11), "?").visible,
            "left at the base state"
        );
    }

    /// The label a warning gets: the group's own `/Name`, which is what a
    /// viewer's layer panel shows, falling back to the resource name.
    #[test]
    fn a_layer_is_named_by_its_own_name_entry() {
        let doc = document("/OCProperties << /OCGs [10 0 R] /D << >> >>", "");
        let oc = OptionalContent::bind(&doc);

        assert_eq!(oc.layer_of(&doc, &objref(10), "MC0").label, "Alpha");
        // 99 0 R is not an object at all, so there is no /Name to read.
        assert_eq!(oc.layer_of(&doc, &objref(99), "MC0").label, "MC0");
    }

    /// The four `/P` policies of 8.11.2.2, over every arrangement of two
    /// groups.
    ///
    /// Written as a table because two of the four read backwards — `/AnyOff`
    /// is visible when any group is **off** — and the inverted reading of
    /// either one agrees with the correct reading on four of the six rows it
    /// touches. Only the mixed row separates `/AnyOn` from `/AnyOff`, and
    /// only the all-on and all-off rows separate them from `/AllOn` and
    /// `/AllOff`. A test that used one arrangement would pin nothing.
    #[test]
    fn every_membership_policy_decides_correctly() {
        // (what /D turns off, policy, expected visibility)
        let cases: &[(&str, &str, bool)] = &[
            // Both groups on.
            ("", "AnyOn", true),
            ("", "AllOn", true),
            ("", "AnyOff", false),
            ("", "AllOff", false),
            // One on, one off.
            ("11 0 R", "AnyOn", true),
            ("11 0 R", "AllOn", false),
            ("11 0 R", "AnyOff", true),
            ("11 0 R", "AllOff", false),
            // Both off.
            ("10 0 R 11 0 R", "AnyOn", false),
            ("10 0 R 11 0 R", "AllOn", false),
            ("10 0 R 11 0 R", "AnyOff", true),
            ("10 0 R 11 0 R", "AllOff", true),
        ];

        for (off, policy, expected) in cases {
            let doc = document(
                &format!("/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [{off}] >> >>"),
                &format!(
                    "12 0 obj\n<< /Type /OCMD /OCGs [10 0 R 11 0 R] /P /{policy} >>\nendobj\n"
                ),
            );
            let oc = OptionalContent::bind(&doc);
            assert_eq!(
                oc.layer_of(&doc, &objref(12), "?").visible,
                *expected,
                "/P /{policy} with [{off}] off",
            );
        }
    }

    /// A missing `/P` is `/AnyOn` (Table 99); a `/P` this build does not know
    /// is *visible*, whatever the groups say.
    ///
    /// Both groups are off here, so `/AnyOn` would hide — which is what makes
    /// the second assertion mean something. Guessing the default for an
    /// unknown policy is a guess that can hide content, and this is the same
    /// guess made in the direction a reader can recover from.
    #[test]
    fn a_missing_policy_defaults_and_an_unknown_one_renders() {
        let build = |entry: &str| {
            document(
                "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [10 0 R 11 0 R] >> >>",
                &format!("12 0 obj\n<< /Type /OCMD /OCGs [10 0 R 11 0 R] {entry} >>\nendobj\n"),
            )
        };

        let doc = build("");
        assert!(
            !OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible,
            "no /P is /AnyOn, and nothing is on"
        );

        let doc = build("/P /AnyOnOrSomething");
        assert!(
            OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible,
            "an unrecognised /P renders rather than guessing"
        );
    }

    /// A membership dictionary naming a group `/OCProperties` never listed is
    /// a hole in the expression, and a hole renders.
    #[test]
    fn a_group_the_configuration_never_listed_renders() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R] /D << /OFF [10 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /OCGs [10 0 R 11 0 R] /P /AllOn >>\nendobj\n",
        );
        let oc = OptionalContent::bind(&doc);

        assert!(
            oc.layer_of(&doc, &objref(12), "?").visible,
            "11 0 R is unlisted, so /AllOn cannot be decided and the content \
             is shown"
        );
    }

    /// An empty `/OCGs` constrains nothing (8.11.2.2).
    #[test]
    fn a_membership_dictionary_with_no_groups_renders() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R] /D << /OFF [10 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /OCGs [] /P /AnyOn >>\nendobj\n",
        );
        assert!(
            OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible
        );
    }

    /// 8.11.2.3: `/VE` overrides `/OCGs` and `/P` entirely, which is the only
    /// way to tell a build that evaluates it from one that ignores it — so
    /// the two disagree here on purpose.
    #[test]
    fn a_visibility_expression_overrides_the_policy() {
        // 10 on, 11 off. /P /AllOn would hide; /Or is true because 10 is on.
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /OCGs [10 0 R 11 0 R] /P /AllOn \
             /VE [/Or 10 0 R 11 0 R] >>\nendobj\n",
        );
        assert!(
            OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible,
            "the expression decided, not the policy"
        );

        // And the other way: /P /AnyOn would show, /And hides.
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /OCGs [10 0 R 11 0 R] /P /AnyOn \
             /VE [/And 10 0 R 11 0 R] >>\nendobj\n",
        );
        assert!(
            !OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible
        );
    }

    /// `/Not`, and nesting, which is what an expression is for.
    #[test]
    fn an_expression_nests_and_negates() {
        // 10 on, 11 off:  /And [ /Not 11 ] 10  ->  true and true.
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /VE [/And [/Not 11 0 R] 10 0 R] >>\nendobj\n",
        );
        assert!(
            OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible
        );

        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /VE [/Not 10 0 R] >>\nendobj\n",
        );
        assert!(
            !OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible,
            "/Not of an on group is off"
        );
    }

    /// Every shape of broken expression renders. The groups are all off and
    /// there is no `/OCGs` to fall back to, so a build that reads any of
    /// these as "hidden" fails here.
    #[test]
    fn a_malformed_expression_renders() {
        for expression in [
            "[]",                      // no operator
            "[/Not]",                  // no operand
            "[/Not 10 0 R 11 0 R]",    // /Not takes exactly one
            "[/Xor 10 0 R 11 0 R]",    // not an operator 8.11.2.3 defines
            "[/And 10 0 R 99 0 R]",    // a group the configuration never saw
            "[/Or (a string) 10 0 R]", // a leaf that is not a group
            "[/And [/Or] 10 0 R]",     // a nested expression that is itself broken
            "/Not",                    // not an array at all
        ] {
            let doc = document(
                "/OCProperties << /OCGs [10 0 R 11 0 R] \
                 /D << /OFF [10 0 R 11 0 R] >> >>",
                &format!("12 0 obj\n<< /Type /OCMD /VE {expression} >>\nendobj\n"),
            );
            assert!(
                OptionalContent::bind(&doc)
                    .layer_of(&doc, &objref(12), "?")
                    .visible,
                "/VE {expression} should render",
            );
        }
    }

    /// A cycle: the expression refers to itself. It must terminate, and it
    /// must terminate on the visible side.
    #[test]
    fn a_cyclic_expression_terminates_and_renders() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R] /D << /OFF [10 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /VE [/Not 13 0 R] >>\nendobj\n\
             13 0 obj\n[/Not 13 0 R]\nendobj\n",
        );
        assert!(
            OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible
        );
    }

    /// A membership dictionary whose `/OCGs` is one reference rather than an
    /// array (8.11.2.2 permits both).
    #[test]
    fn a_single_group_reference_is_read_as_a_list_of_one() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R] /D << /OFF [10 0 R] >> >>",
            "12 0 obj\n<< /Type /OCMD /OCGs 10 0 R >>\nendobj\n",
        );
        assert!(
            !OptionalContent::bind(&doc)
                .layer_of(&doc, &objref(12), "?")
                .visible
        );
    }
}
