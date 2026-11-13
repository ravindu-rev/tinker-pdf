//! Optional content: which layers a document's configuration shows (8.11).
//!
//! A layer is how a file carries a CAD drawing's construction lines, a map's
//! alternate languages, a proof's annotation overlay. The catalog's
//! `/OCProperties` `/D` configuration says which of them a reader shows when
//! it opens the file with no further instruction, an entry of its `/Configs`
//! another set a render may choose (8.11.4.3), the chosen configuration's
//! `/AS` entries how a view, a print or an export adjusts that (ISO 32000-2
//! 8.11.4.4), for a render that names its usage, a render's own overrides
//! which groups it turns on or off over them, under the configuration's
//! `/RBGroups` and `/Locked`, and its `/Intent` which groups count at all
//! (8.11.2.3); that is the whole of what this module answers, once a render
//! (`docs/design/render-usage.md`, `docs/design/optional-content-config.md`).
//! Nothing here mutates the document.
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
//! Feature documentation: `docs/features/rendering.md`.

use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_content::Layer;
use tinker_pdf_cos::{decode_text_string, CosDocument, Dict, ObjRef, Object};

use crate::resources::{read_key, read_resolved};
use crate::{LayerOptions, LayerUser, Usage};

/// How deep a `/VE` visibility expression may nest before it is refused
/// (8.11.2.3).
///
/// A refusal means *visible*, so an adversarial expression costs a bounded
/// walk and shows its content rather than hiding it.
const MAX_EXPRESSION_DEPTH: u32 = 16;

/// How many members of a `/VE` or `/OCGs` list are examined.
const MAX_OPERANDS: usize = 256;

/// How many entries of `/OCProperties /Configs` are read: a configuration a
/// render names past them renders as `/D`, as one past the array does, and
/// the listing stops there too.
pub(crate) const MAX_CONFIGURATIONS: usize = MAX_OPERANDS;

/// The states a render's optional-content configuration gives the document's
/// groups — `/D`'s or a chosen entry of `/Configs` — resolved once, for one
/// usage or for none, with the render's overrides over them.
///
/// Built at bind time from the catalog, not consulted per operator: the
/// answer for a given group cannot change during a render, whose usage,
/// configuration and overrides are fixed before it starts. Two bindings that
/// are equal answer every question alike — equality is the states and the
/// listing, not what a render reports of its overrides — which is what lets
/// a display list recorded under one configuration replay a render of another
/// whose states are the same.
#[derive(Debug, Default)]
pub(crate) struct OptionalContent {
    /// Every group the configuration has an opinion about, and that opinion.
    ///
    /// A `BTreeMap` rather than a `HashMap` because ruling 4 makes iteration
    /// order a correctness question everywhere in this engine, and a map that
    /// is only ever looked up today is a map somebody iterates tomorrow.
    states: BTreeMap<ObjRef, bool>,
    /// `/OCProperties /OCGs` in the order the catalog wrote it (8.11.4.2).
    ///
    /// The map above answers "is this group on", which is all the renderer
    /// ever asks and all it needed before [`crate::layers`] existed. A list
    /// of the document's layers is a different question and the map cannot
    /// answer it: `BTreeMap` order is object-number order, and a producer's
    /// `/OCGs` order is the order its layer panel shows. Keeping both is four
    /// words of state and the alternative is a second walk of the catalog
    /// that could disagree with this one.
    listed: Vec<ObjRef>,
    /// The locked groups the render's overrides would have changed, each
    /// once, by the label `RenderWarning::LockedLayer` reports, in the order
    /// the overrides met them: what the render reports, and no question of
    /// visibility.
    refused: Vec<String>,
    /// Whether a `/Zoom` entry recommended a state: whether a render at
    /// another magnification may bind other states, which a display list and
    /// a form's kept drawing ask. No question of visibility either.
    reads_magnification: bool,
}

impl PartialEq for OptionalContent {
    /// The states and the listing: what every question a scope asks is
    /// answered from.
    fn eq(&self, other: &OptionalContent) -> bool {
        self.states == other.states && self.listed == other.listed
    }
}

impl Eq for OptionalContent {}

impl OptionalContent {
    /// Reads the catalog's `/OCProperties` `/D` configuration (8.11.4.3): the
    /// initial state of every group, which a render naming no usage draws.
    ///
    /// A document without `/OCProperties` — which is nearly all of them —
    /// yields an empty set, and an empty set says every layer is visible.
    pub(crate) fn bind(doc: &CosDocument) -> OptionalContent {
        OptionalContent::bind_for(doc, None)
    }

    /// [`OptionalContent::bind`] for a render naming `usage`: the initial
    /// states, and over them the configuration's `/AS` entries whose
    /// `/Event` is the usage's (ISO 32000-2 8.11.4.4,
    /// `docs/design/render-usage.md`). With no usage, the initial states.
    pub(crate) fn bind_for(doc: &CosDocument, usage: Option<Usage>) -> OptionalContent {
        OptionalContent::bind_with(doc, usage, &LayerOptions::default(), 1.0)
    }

    /// [`OptionalContent::bind_for`] under the layer options a render names
    /// (`docs/design/optional-content-config.md`), at its magnification: its
    /// configuration — `None` for `/D`, `Some(i)` for the `i`th entry of
    /// `/Configs`, an index past them, or an entry that is no dictionary,
    /// binding `/D`'s — its overrides, and its language and user, which with
    /// the magnification are what the `/Zoom`, `/Language` and `/User`
    /// categories read. The states are built in one order: `/D`'s initial
    /// states —
    /// `/BaseState` over every group `/OCGs` lists, then its `/ON`, then its
    /// `/OFF` (8.11.4.5) — then the chosen configuration over them, its
    /// `/BaseState` `/ON` or `/OFF` setting every listed group and
    /// `/Unchanged` leaving each as `/D` set it, then its own `/ON` and `/OFF`
    /// (Table 99); then the chosen configuration's own `/AS` entries for
    /// `usage` — a configuration with none adjusts nothing, as Table 99 gives
    /// `/AS` no default ([`recommendations`]); then the render's overrides, in order, under the
    /// configuration's `/RBGroups` and `/Locked` ([`apply_overrides`]); and
    /// last its `/Intent` decides which groups count at all (8.11.2.3): a
    /// group none of whose intents the configuration's set holds has no
    /// effect, and reads as one the configuration never mentioned does,
    /// visible.
    pub(crate) fn bind_with(
        doc: &CosDocument,
        usage: Option<Usage>,
        layers: &LayerOptions,
        magnification: f64,
    ) -> OptionalContent {
        #[cfg(test)]
        binds::counted();
        let mut states = BTreeMap::new();

        let Some(catalog) = doc.catalog() else {
            return OptionalContent::default();
        };
        let entry = doc.resolve_key(&catalog, doc.intern(b"OCProperties"));
        let Some(properties) = entry.as_dict() else {
            return OptionalContent::default();
        };

        // 8.11.4.2: /OCGs lists every group in the document, and /D is the
        // configuration a reader applies when it opens the file.
        let all = refs_of(doc, properties, b"OCGs");
        let d = doc.resolve_key(properties, doc.intern(b"D"));
        let default = d.as_dict().cloned().unwrap_or_default();

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

        // Table 99: the chosen configuration is applied to the document as
        // opened, so `/Unchanged` leaves each group as `/D` set it; a
        // `/BaseState` that is missing or none of the three names is `/ON`,
        // its default; `/OFF` is applied after `/ON`, as `/D`'s is.
        let chosen = layers
            .configuration
            .and_then(|index| configuration_at(doc, properties, index));
        if let Some(chosen) = &chosen {
            let base = chosen
                .get_name(doc.intern(b"BaseState"))
                .and_then(|n| doc.name_bytes(n));
            match base.as_deref() {
                Some(b"Unchanged") => {}
                Some(b"OFF") => {
                    for reference in &all {
                        states.insert(*reference, false);
                    }
                }
                _ => {
                    for reference in &all {
                        states.insert(*reference, true);
                    }
                }
            }
            for reference in refs_of(doc, chosen, b"ON") {
                states.insert(reference, true);
            }
            for reference in refs_of(doc, chosen, b"OFF") {
                states.insert(reference, false);
            }
        }
        let applied = chosen.as_ref().unwrap_or(&default);

        let mut reads_magnification = false;
        if let Some(usage) = usage {
            let inputs = Inputs {
                magnification: self::magnification(magnification),
                language: layers.language.as_deref().filter(|tag| !tag.is_empty()),
                user: &layers.user,
            };
            let recommended = recommendations(doc, applied, usage, &inputs);
            reads_magnification = recommended.zoomed;
            for (reference, on) in recommended.states {
                states.insert(reference, on);
            }
        }

        // The render's own changes, last of the four (RC-14.3).
        let refused = if layers.overrides.is_empty() {
            Vec::new()
        } else {
            // Table 99: a configuration other than `/D` without `/RBGroups`
            // takes `/D`'s; `/Locked`'s default is an empty array, never
            // `/D`'s. A null value is an absent one (7.3.9).
            let inherits = chosen.as_ref().is_some_and(|chosen| {
                read_key(doc, chosen, b"RBGroups", |value| {
                    matches!(value, Object::Null)
                })
            });
            let collections = radio_collections(doc, if inherits { &default } else { applied });
            let locked: BTreeSet<ObjRef> = refs_of(doc, applied, b"Locked").into_iter().collect();
            apply_overrides(
                &layers.overrides,
                &Constraints {
                    listed: all.iter().copied().collect(),
                    locked,
                    collections,
                },
                &mut states,
            )
            .into_iter()
            .map(|group| group_label(doc, group))
            .collect()
        };

        // 8.11.2.3: "If there is no match, the group shall have no effect on
        // visibility", and an empty set of intents uses no group at all.
        let intents = configuration_intents(doc, applied);
        states.retain(|reference, _| counts(doc, *reference, &intents));

        OptionalContent {
            states,
            listed: all,
            refused,
            reads_magnification,
        }
    }

    /// Whether a `/Zoom` entry recommended a group a state in this binding,
    /// so that a render at another magnification may bind other states. A
    /// binding that read none is the same at every magnification.
    pub(crate) fn reads_magnification(&self) -> bool {
        self.reads_magnification
    }

    /// The locked groups the render's overrides would have changed, each once
    /// and in the order they were met, as `RenderWarning::LockedLayer` names
    /// them: a group's `/Name`, or its reference where it has none.
    pub(crate) fn refused(&self) -> &[String] {
        &self.refused
    }

    /// The groups `/OCProperties /OCGs` names, in the catalog's own order.
    ///
    /// Only the listed ones: a group some content stream's `/OC` mentions but
    /// the catalog never declared is not a layer of this document, and
    /// [`Self::layer_of`] already resolves it to visible where it is used.
    /// Listing it here would invent a layer entry for a malformation.
    pub(crate) fn listed(&self) -> Vec<ObjRef> {
        self.listed.clone()
    }

    /// Whether the default configuration shows the group at `reference`.
    ///
    /// A group the configuration never mentioned reads as visible, which is
    /// this module's one direction of fallback throughout.
    pub(crate) fn is_visible(&self, reference: ObjRef) -> bool {
        self.group_state(reference).unwrap_or(true)
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
    ///
    /// The entry is read where it lies ([`read_resolved`]) and never copied,
    /// and so is every value read out of it ([`read_key`]) — a group's
    /// `/Name` and `/Type`, a membership dictionary's `/VE`, `/OCGs` and `/P`:
    /// every `/OC /name BDC` asks, and any of them is as long as the file
    /// makes it.
    pub(crate) fn layer_of(&self, doc: &CosDocument, object: &Object, fallback: &str) -> Layer {
        let reference = object.as_objref();
        read_resolved(doc, object, |resolved| match resolved.as_dict() {
            Some(dict) => self.layer_of_dict(doc, dict, reference, fallback),
            // Not a dictionary at all: nothing to hide by.
            None => Layer {
                visible: true,
                label: fallback.to_string(),
            },
        })
    }

    /// [`Self::layer_of`] for the dictionary `object` resolved to, and the
    /// reference it was reached by, if any.
    fn layer_of_dict(
        &self,
        doc: &CosDocument,
        dict: &Dict,
        reference: Option<ObjRef>,
        fallback: &str,
    ) -> Layer {
        // Every value read where it lies, as the dictionary is: a copy of
        // one is its size at every `BDC` naming the group.
        let label = read_key(doc, dict, b"Name", |name| {
            name.as_string().map(|s| decode_text_string(&s.bytes))
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string());

        let kind = read_key(doc, dict, b"Type", |entry| name_of(doc, entry));

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
        //
        // Every value here is read where it lies, as `layer_of`'s are: an
        // expression is evaluated a bounded way in (`MAX_OPERANDS`), but a
        // copy of one is all of it, at every `BDC`.
        let understood = read_key(doc, dict, b"VE", |expression| {
            expression
                .as_array()
                .and_then(|items| self.evaluate(doc, items, 0))
        });
        if let Some(value) = understood {
            return value;
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

        let policy = read_key(doc, dict, b"P", |entry| name_of(doc, entry));

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

        // The head and each operand read where they lie (`read_resolved`): a
        // nested expression is copied whole before its length is checked.
        let operator = read_resolved(doc, items.first()?, |head| name_of(doc, head))?;
        let operands = items.get(1..)?;
        if operands.is_empty() || operands.len() > MAX_OPERANDS {
            return None;
        }

        let mut values = Vec::with_capacity(operands.len());
        for operand in operands {
            // An operand is either a nested expression or a group reference.
            // A nested *array* is only ever an expression: 8.11.2.3 has no
            // other array-shaped leaf.
            let nested = read_resolved(doc, operand, |resolved| {
                resolved
                    .as_array()
                    .map(|nested| self.evaluate(doc, nested, depth + 1))
            });
            match nested {
                Some(value) => values.push(value?),
                None => values.push(self.group_state(operand.as_objref()?)?),
            }
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

/// What a configuration says about the changes a render may make (Table
/// 99): the groups `/OCGs` lists, which alone an override reaches; the
/// groups `/Locked` lists; and the `/RBGroups` collections.
struct Constraints {
    /// `/OCProperties /OCGs`: "all the optional content groups in the
    /// document" (8.11.4.2). A reference outside it names none of them.
    listed: BTreeSet<ObjRef>,
    /// The chosen configuration's `/Locked`.
    locked: BTreeSet<ObjRef>,
    /// The radio-button collections, each its groups in order, once each.
    collections: Vec<Vec<ObjRef>>,
}

/// Applies `overrides` to `states` in order (RC-14.3, 8.11.4.3 Table 99),
/// and returns the locked groups an override or its consequence would have
/// changed, each once, in the order met.
///
/// An override of a group `/OCGs` does not list changes nothing. One of a
/// locked group is refused whatever it asks — "The state of a locked group
/// cannot be changed through the user interface", and a render's change is
/// the caller's — so the group keeps its state and nothing follows from it.
/// Any other sets its group's state, and one turning a group on then turns
/// off every other listed member of each collection holding it that is on:
/// "If one group is turned ON, all others shall be turned OFF. However,
/// turning a group from ON to OFF does not force any other group to be
/// turned ON." A member that is locked is not turned off, and is refused as
/// an override of it is. The collections constrain the overrides alone, so a
/// configuration whose own `/ON` shows two members of one shows both until
/// an override turns one of them on.
fn apply_overrides(
    overrides: &[(ObjRef, bool)],
    constraints: &Constraints,
    states: &mut BTreeMap<ObjRef, bool>,
) -> Vec<ObjRef> {
    let mut refused = Vec::new();
    let refuse = |group: ObjRef, refused: &mut Vec<ObjRef>| {
        if !refused.contains(&group) {
            refused.push(group);
        }
    };
    for &(group, on) in overrides {
        if !constraints.listed.contains(&group) {
            continue;
        }
        if constraints.locked.contains(&group) {
            refuse(group, &mut refused);
            continue;
        }
        states.insert(group, on);
        if !on {
            continue;
        }
        for collection in &constraints.collections {
            if !collection.contains(&group) {
                continue;
            }
            for &member in collection {
                if member == group
                    || !constraints.listed.contains(&member)
                    || states.get(&member) != Some(&true)
                {
                    continue;
                }
                if constraints.locked.contains(&member) {
                    refuse(member, &mut refused);
                } else {
                    states.insert(member, false);
                }
            }
        }
    }
    refused
}

/// A configuration's `/RBGroups` (Table 99): each array of it a collection of
/// the groups it references, read [`MAX_OPERANDS`] collections in and as many
/// members of each, an array reached by reference read where it lies; an
/// entry that is no array, a member that is no reference and a value that is
/// no array are passed over, so what this build cannot read constrains
/// nothing.
fn radio_collections(doc: &CosDocument, configuration: &Dict) -> Vec<Vec<ObjRef>> {
    read_key(doc, configuration, b"RBGroups", |value| {
        let Some(entries) = value.as_array() else {
            return Vec::new();
        };
        entries
            .iter()
            .take(MAX_OPERANDS)
            .filter_map(|entry| read_resolved(doc, entry, |entry| entry.as_array().map(collect)))
            .filter(|collection| collection.len() > 1)
            .collect()
    })
}

/// What `RenderWarning::LockedLayer` calls the group at `reference`: its
/// `/Name` decoded as a text string (8.11.2.1), or the reference itself,
/// `12 0 R`, where it has no `/Name` or an empty one.
fn group_label(doc: &CosDocument, reference: ObjRef) -> String {
    doc.get(reference)
        .ok()
        .and_then(|object| {
            object.as_dict().and_then(|dict| {
                read_key(doc, dict, b"Name", |name| {
                    name.as_string().map(|s| decode_text_string(&s.bytes))
                })
            })
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| reference.to_string())
}

/// Entry `index` of `/OCProperties /Configs`, a configuration dictionary
/// (8.11.4.3), or `None` past the array, past [`MAX_CONFIGURATIONS`], where
/// there is no array, and for an entry that is no dictionary — each a
/// configuration a render renders as `/D`.
fn configuration_at(doc: &CosDocument, properties: &Dict, index: usize) -> Option<Dict> {
    if index >= MAX_CONFIGURATIONS {
        return None;
    }
    read_key(doc, properties, b"Configs", |configs| {
        let entry = configs.as_array()?.get(index)?;
        read_resolved(doc, entry, |entry| entry.as_dict().cloned())
    })
}

/// The entries of `/OCProperties /Configs` in order, each a configuration
/// dictionary or `None` for one that is not, read [`MAX_CONFIGURATIONS`] in;
/// empty for a document with no `/Configs`. What a render takes the index
/// into, so the listing and the render read one array.
pub(crate) fn configurations(doc: &CosDocument) -> Vec<Option<Dict>> {
    let Some(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let entry = doc.resolve_key(&catalog, doc.intern(b"OCProperties"));
    let Some(properties) = entry.as_dict() else {
        return Vec::new();
    };
    read_key(doc, properties, b"Configs", |configs| {
        configs
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .take(MAX_CONFIGURATIONS)
                    .map(|entry| read_resolved(doc, entry, |entry| entry.as_dict().cloned()))
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Whether a render naming configuration `index` finds it: an entry of
/// `/Configs` that is a configuration dictionary. One that is not renders as
/// `/D` and is reported (`RenderWarning::UnreadableLayerConfiguration`).
pub(crate) fn configuration_readable(doc: &CosDocument, index: usize) -> bool {
    let Some(catalog) = doc.catalog() else {
        return false;
    };
    let entry = doc.resolve_key(&catalog, doc.intern(b"OCProperties"));
    entry
        .as_dict()
        .is_some_and(|properties| configuration_at(doc, properties, index).is_some())
}

/// A configuration's set of intents (Table 99): every intent for `/All`
/// among them, and otherwise the names it states.
#[derive(Debug)]
enum Intents {
    /// `/All`: "the set of all intents, including those not yet defined".
    All,
    /// The names stated, `/View` where the configuration states none; empty
    /// for an empty array, which uses no group at all.
    Named(Vec<Vec<u8>>),
}

/// The names an `/Intent` value states: a name, or an array's names read
/// [`MAX_OPERANDS`] in; `None` for anything else, which is the default.
fn intent_names(doc: &CosDocument, value: &Object) -> Option<Vec<Vec<u8>>> {
    if let Some(name) = name_of(doc, value) {
        return Some(vec![name]);
    }
    value.as_array().map(|items| {
        items
            .iter()
            .take(MAX_OPERANDS)
            .filter_map(|item| read_resolved(doc, item, |item| name_of(doc, item)))
            .collect()
    })
}

/// `configuration`'s `/Intent` (Table 99), `/View` by default.
fn configuration_intents(doc: &CosDocument, configuration: &Dict) -> Intents {
    match read_key(doc, configuration, b"Intent", |value| {
        intent_names(doc, value)
    }) {
        Some(names) if names.iter().any(|name| name.as_slice() == b"All") => Intents::All,
        Some(names) => Intents::Named(names),
        None => Intents::Named(vec![b"View".to_vec()]),
    }
}

/// Whether the group at `reference` is used in determining visibility under
/// `intents` (8.11.2.3): "If one or more of a group's intents is contained in
/// the current configuration's set of intents, the group shall be used". A
/// group's `/Intent` is `/View` by default (Table 98), and so is that of a
/// reference that is no group dictionary.
fn counts(doc: &CosDocument, reference: ObjRef, intents: &Intents) -> bool {
    let set = match intents {
        Intents::All => return true,
        Intents::Named(set) => set,
    };
    if set.is_empty() {
        return false;
    }
    let group = doc
        .get(reference)
        .ok()
        .and_then(|object| {
            object
                .as_dict()
                .map(|dict| read_key(doc, dict, b"Intent", |value| intent_names(doc, value)))
        })
        .flatten()
        .unwrap_or_else(|| vec![b"View".to_vec()]);
    group.iter().any(|intent| set.contains(intent))
}

/// The indirect references an array-valued key holds, deduplicated and
/// bounded.
///
/// A single reference where an array was expected is accepted: `/OCGs 5 0 R`
/// is legal in a membership dictionary (8.11.2.2) and appears in the wild for
/// the configuration arrays too.
///
/// Read where it lies, never cloned: a membership dictionary's `/OCGs` is
/// read at every `BDC` naming it, and only `MAX_OPERANDS` of it is looked
/// at, so a copy was the whole array to read the head of it.
fn refs_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Vec<ObjRef> {
    let Some(value) = dict.get(doc.intern(key)) else {
        return Vec::new();
    };
    if let Some(reference) = value.as_objref() {
        // The key may point at the array indirectly, or *be* the one group.
        let resolved = doc.resolve(value);
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

/// How many of a usage application dictionary's `/Category` names are read.
/// Table 100 defines nine usage entries, six of which carry a state this
/// build reads.
const MAX_CATEGORIES: usize = 16;

/// What a render states for the categories whose recommendation is not a
/// state written in the group (8.11.4.4): the magnification `/Zoom` reads,
/// the language `/Language` reads and the identification `/User` reads.
struct Inputs<'a> {
    /// The render's magnification, read as 1 where it is not a finite number
    /// above zero ([`magnification`]).
    magnification: f64,
    /// The application's language tag; `None` states none.
    language: Option<&'a str>,
    /// The user's identification, of each kind.
    user: &'a LayerUser,
}

/// The recommendations a usage's `/AS` entries make ([`recommendations`]),
/// and whether any of them came from a `/Zoom` entry, which another
/// magnification may answer otherwise.
#[derive(Default)]
struct Recommended {
    /// Each group something recommended a state for, and that state.
    states: BTreeMap<ObjRef, bool>,
    /// Whether a `/Zoom` entry recommended a state to any group.
    zoomed: bool,
}

impl Recommended {
    /// `on` recommended for `group`, where a group already recommended OFF
    /// stays OFF.
    fn add(&mut self, group: ObjRef, on: bool) {
        let all_on = self.states.entry(group).or_insert(true);
        *all_on = *all_on && on;
    }
}

/// The state each group a usage's `/AS` entries name is set to (ISO 32000-2
/// 8.11.4.4): `configuration`'s entries whose `/Event` is `usage`'s, each
/// group's recommendations across all of them combined.
///
/// "For each of the groups in OCGs, the entries in its usage dictionary …
/// specified by Category shall be examined to yield a recommended state for
/// the group. If all the entries yield a recommended state of ON, the group's
/// state shall be set to ON; otherwise, its state shall be set to OFF", and a
/// group in several entries "shall be ON only if all categories in all the
/// usage application dictionaries it appears in have a state of ON". A
/// category with no entry in a group's `/Usage` recommends nothing — the
/// clause's example: "Object 4 has none; therefore, it is not affected by
/// zoom level changes" — so a group is here only where something recommended
/// a state for it, and every other group keeps the one `/D` gave it.
///
/// Whatever is malformed recommends nothing and so leaves its group as `/D`
/// set it: an `/AS` that is not an array, an entry that is not a dictionary,
/// an `/Event` that is none of the three, a `/Category` missing or not an
/// array. Every array is read a bounded way in, and every value where it
/// lies.
fn recommendations(
    doc: &CosDocument,
    configuration: &Dict,
    usage: Usage,
    inputs: &Inputs<'_>,
) -> Recommended {
    let mut recommended = Recommended::default();
    read_key(doc, configuration, b"AS", |entries| {
        let Some(entries) = entries.as_array() else {
            return;
        };
        for entry in entries.iter().take(MAX_OPERANDS) {
            read_resolved(doc, entry, |entry| {
                if let Some(entry) = entry.as_dict() {
                    recommend(doc, entry, usage, inputs, &mut recommended);
                }
            });
        }
    });
    recommended
}

/// One usage application dictionary's recommendations, added to
/// `recommended`, where a group already recommended OFF stays OFF.
fn recommend(
    doc: &CosDocument,
    entry: &Dict,
    usage: Usage,
    inputs: &Inputs<'_>,
    recommended: &mut Recommended,
) {
    let event = read_key(doc, entry, b"Event", |event| name_of(doc, event));
    if event.as_deref() != Some(usage.event()) {
        return;
    }
    let Some(categories) = read_key(doc, entry, b"Category", |categories| {
        categories.as_array().map(|names| {
            names
                .iter()
                .take(MAX_CATEGORIES)
                .filter_map(|name| read_resolved(doc, name, |name| name_of(doc, name)))
                .collect::<Vec<_>>()
        })
    }) else {
        return;
    };
    // Table 101: `/OCGs` defaults to "an empty array, indicating that no
    // groups shall be affected".
    let groups = refs_of(doc, entry, b"OCGs");
    // The language category is decided over the entry's groups together.
    let languages = inputs
        .language
        .filter(|_| categories.iter().any(|name| name.as_slice() == b"Language"))
        .map(|wanted| Languages::of(doc, &groups, wanted));
    for group in groups {
        let Ok(object) = doc.get(group) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        read_key(doc, dict, b"Usage", |usage_dict| {
            let Some(usage_dict) = usage_dict.as_dict() else {
                return;
            };
            for category in &categories {
                let state = match category.as_slice() {
                    b"Zoom" => {
                        let state = zoom_state(doc, usage_dict, inputs.magnification);
                        recommended.zoomed |= state.is_some();
                        state
                    }
                    b"Language" => languages.as_ref().and_then(|found| found.state(group)),
                    b"User" => user_state(doc, usage_dict, inputs.user),
                    category => category_state(doc, usage_dict, category),
                };
                if let Some(on) = state {
                    recommended.add(group, on);
                }
            }
        });
    }
}

/// What one of the three categories whose state is an event's recommends in
/// a group's usage dictionary: `Some(true)` for ON and `Some(false)` for OFF,
/// `None` where it recommends nothing (8.11.4.4): "View: The state shall be
/// the value of the ViewState entry", "Print: The state shall be the value of
/// the PrintState entry. If PrintState is not present, the state of the
/// optional content group shall be left unchanged", "Export: The state shall
/// be the value of the ExportState entry". `/CreatorInfo` and `/PageElement`
/// carry no state and recommend nothing, as an absent entry does and as every
/// name Table 100 does not define does; so does a state that is neither `/ON`
/// nor `/OFF`.
fn category_state(doc: &CosDocument, usage: &Dict, category: &[u8]) -> Option<bool> {
    let (entry, state): (&[u8], &[u8]) = match category {
        b"View" => (b"View", b"ViewState"),
        b"Print" => (b"Print", b"PrintState"),
        b"Export" => (b"Export", b"ExportState"),
        _ => return None,
    };
    read_key(doc, usage, entry, |entry| {
        let entry = entry.as_dict()?;
        read_key(doc, entry, state, |value| on_or_off(doc, value))
    })
}

/// `/ON` as `true` and `/OFF` as `false`; `None` for anything else.
fn on_or_off(doc: &CosDocument, value: &Object) -> Option<bool> {
    match name_of(doc, value)?.as_slice() {
        b"ON" => Some(true),
        b"OFF" => Some(false),
        _ => None,
    }
}

/// A magnification as a render reads it: 1 where it is not a finite number
/// above zero, as a scale is (`crate::RenderOptions::magnification`).
pub(crate) fn magnification(magnification: f64) -> f64 {
    if magnification.is_finite() && magnification > 0.0 {
        magnification
    } else {
        1.0
    }
}

/// What a group's `/Usage /Zoom` recommends at `magnification` (8.11.4.4):
/// "If the current magnification level of the document is greater than or
/// equal to min and less than max, the ON state shall be used; otherwise, OFF
/// shall be used", `/min` 0 and `/max` infinity where absent (Table 100).
/// `None` where there is no `/Zoom`, where it is no dictionary, or where a
/// bound is present and no number this build can compare — what this build
/// cannot read leaves the group as configured.
fn zoom_state(doc: &CosDocument, usage: &Dict, magnification: f64) -> Option<bool> {
    read_key(doc, usage, b"Zoom", |zoom| {
        let zoom = zoom.as_dict()?;
        let bound = |key: &[u8], default: f64| {
            read_key(doc, zoom, key, |value| match value {
                // 7.3.9: a null value is an absent one.
                Object::Null => Some(default),
                value => value.as_number().filter(|bound| !bound.is_nan()),
            })
        };
        let min = bound(b"min", 0.0)?;
        let max = bound(b"max", f64::INFINITY)?;
        Some(magnification >= min && magnification < max)
    })
}

/// What a group's `/Usage /User` recommends for `user` (8.11.4.4): "The Name
/// entry shall specify a name or names to match with the user's
/// identification. The Type entry determines how the Name entry shall be
/// interpreted (name, title, or organisation). If there is an exact match,
/// the ON state shall be used; otherwise OFF shall be used." The
/// identification compared is the one of the kind `/Type` names — `/Ind`,
/// `/Ttl` or `/Org` — and a `/Name`, a text string or an array of them read
/// [`MAX_OPERANDS`] in, matches where one of them, decoded, equals it.
/// `None` — nothing recommended — where the render states no identification
/// of that kind, and where there is no `/User`, it is no dictionary, its
/// `/Type` is none of the three, or its `/Name` is neither a text string nor
/// an array of them.
fn user_state(doc: &CosDocument, usage: &Dict, user: &LayerUser) -> Option<bool> {
    read_key(doc, usage, b"User", |entry| {
        let entry = entry.as_dict()?;
        let kind = read_key(doc, entry, b"Type", |kind| name_of(doc, kind))?;
        let identification = match kind.as_slice() {
            b"Ind" => user.name.as_deref(),
            b"Ttl" => user.title.as_deref(),
            b"Org" => user.organisation.as_deref(),
            _ => None,
        }?;
        let names = read_key(doc, entry, b"Name", |names| text_strings(doc, names))?;
        Some(names.iter().any(|name| name == identification))
    })
}

/// A text string, or an array of text strings read [`MAX_OPERANDS`] in, each
/// decoded (7.9.2.2); `None` for anything else, and for an array holding
/// anything else.
fn text_strings(doc: &CosDocument, value: &Object) -> Option<Vec<String>> {
    if let Some(text) = value.as_string() {
        return Some(vec![decode_text_string(&text.bytes)]);
    }
    value
        .as_array()?
        .iter()
        .take(MAX_OPERANDS)
        .map(|item| {
            read_resolved(doc, item, |item| {
                item.as_string().map(|text| decode_text_string(&text.bytes))
            })
        })
        .collect()
}

/// How a group's `/Lang` stands to the application's language (8.11.4.4):
/// "an exact match to the language and locale", or "a partial match (that
/// is, the language matches but not the locale)", or neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Match {
    /// The whole tag, ignoring ASCII case: 14.9.2's language identifiers are
    /// language tags, whose case carries no meaning.
    Exact,
    /// The primary language subtag — what precedes the first hyphen — and
    /// not the rest.
    Partial,
    /// Neither.
    Neither,
}

impl Match {
    /// How the tag `lang` stands to the tag `wanted`.
    fn of(lang: &str, wanted: &str) -> Match {
        if lang.eq_ignore_ascii_case(wanted) {
            return Match::Exact;
        }
        let primary = |tag: &str| tag.split('-').next().unwrap_or_default().to_string();
        let (lang, wanted) = (primary(lang), primary(wanted));
        if !lang.is_empty() && lang.eq_ignore_ascii_case(&wanted) {
            Match::Partial
        } else {
            Match::Neither
        }
    }
}

/// One usage application dictionary's `/Language` category, decided over the
/// groups its `/OCGs` lists (8.11.4.4): "If an exact match to the language
/// and locale is found among the Lang entries of the optional content groups
/// in the usage application dictionary's OCGs list, all groups that have
/// exact matches shall receive an ON recommendation. If no exact match is
/// found, but a partial match is found …, all partially matching groups that
/// have Preferred entries with a value of ON shall receive an ON
/// recommendation. All other groups shall receive an OFF recommendation."
/// The other groups are those with a `/Language` entry: a group without one
/// has nothing for the category to examine, and one whose entry this build
/// cannot read — no dictionary, a `/Lang` that is no text string, a
/// `/Preferred` that is neither `/ON` nor `/OFF` — is recommended nothing and
/// takes no part in the match.
struct Languages {
    /// Each group with a readable `/Language`: how its `/Lang` matches, and
    /// whether its `/Preferred` is `/ON` (`/OFF` by default).
    spoken: BTreeMap<ObjRef, (Match, bool)>,
    /// Whether any group matches exactly.
    exact: bool,
    /// Whether any group matches in part.
    partial: bool,
}

impl Languages {
    /// The category over `groups` for the application's language `wanted`.
    fn of(doc: &CosDocument, groups: &[ObjRef], wanted: &str) -> Languages {
        let mut spoken = BTreeMap::new();
        for &group in groups {
            let Ok(object) = doc.get(group) else {
                continue;
            };
            let Some(dict) = object.as_dict() else {
                continue;
            };
            let entry = read_key(doc, dict, b"Usage", |usage| {
                let usage = usage.as_dict()?;
                read_key(doc, usage, b"Language", |entry| {
                    let entry = entry.as_dict()?;
                    let lang = read_key(doc, entry, b"Lang", |lang| {
                        lang.as_string().map(|text| decode_text_string(&text.bytes))
                    })?;
                    let preferred = read_key(doc, entry, b"Preferred", |value| match value {
                        Object::Null => Some(false),
                        value => on_or_off(doc, value),
                    })?;
                    Some((Match::of(&lang, wanted), preferred))
                })
            });
            if let Some(entry) = entry {
                spoken.insert(group, entry);
            }
        }
        let exact = spoken.values().any(|(found, _)| *found == Match::Exact);
        let partial = spoken.values().any(|(found, _)| *found == Match::Partial);
        Languages {
            spoken,
            exact,
            partial,
        }
    }

    /// What the category recommends `group`, `None` for a group without a
    /// readable `/Language`.
    fn state(&self, group: ObjRef) -> Option<bool> {
        let (found, preferred) = *self.spoken.get(&group)?;
        Some(if self.exact {
            found == Match::Exact
        } else if self.partial {
            found == Match::Partial && preferred
        } else {
            false
        })
    }
}

/// Configurations bound, counted for the guard that a render binds one:
/// compiled only under `cfg(test)` and kept per thread, so tests running side
/// by side cannot add into each other's.
#[cfg(test)]
pub(crate) mod binds {
    use std::cell::Cell;

    std::thread_local! {
        static BINDS: Cell<u64> = const { Cell::new(0) };
    }

    /// One configuration bound.
    pub(crate) fn counted() {
        BINDS.with(|binds| binds.set(binds.get() + 1));
    }

    /// The binds counted on this thread since the last time, which starts the
    /// count again from zero.
    pub(crate) fn take() -> u64 {
        BINDS.with(|binds| binds.replace(0))
    }
}

/// The name `object` is, as bytes; `None` for anything else.
fn name_of(doc: &CosDocument, object: &Object) -> Option<Vec<u8>> {
    object
        .as_name()
        .and_then(|n| doc.name_bytes(n))
        .map(|n| n.to_vec())
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
    use super::{binds, OptionalContent};
    use crate::Usage;
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

    /// A document listing groups 10, 11 and 13, group 13 carrying `usage`
    /// as its `/Usage` entries, and `/D`'s body `d`.
    fn usage_document(d: &str, usage: &str, extra: &str) -> Arc<CosDocument> {
        document(
            &format!("/OCProperties << /OCGs [10 0 R 11 0 R 13 0 R] /D << {d} >> >>"),
            &format!(
                "13 0 obj\n<< /Type /OCG /Name (Gamma) /Usage << {usage} >> >>\nendobj\n{extra}"
            ),
        )
    }

    /// Group `num`'s state under `oc`, `None` where the configuration says
    /// nothing of it.
    fn state(oc: &OptionalContent, num: u32) -> Option<bool> {
        oc.group_state(ObjRef { num, gen: 0 })
    }

    /// The layer options naming `configuration` and nothing else.
    fn configured(configuration: Option<usize>) -> crate::LayerOptions {
        crate::LayerOptions {
            configuration,
            ..crate::LayerOptions::default()
        }
    }

    /// The usages a render can name, and none.
    const USAGES: [Option<Usage>; 4] = [
        None,
        Some(Usage::View),
        Some(Usage::Print),
        Some(Usage::Export),
    ];

    /// **A usage applies its own event's entries and no other's** (ISO
    /// 32000-2 8.11.4.4): one entry for each event, each turning group 13 the
    /// way its category's state says, over a `/D` that turns it off; a
    /// render naming no usage keeps the initial state, and groups no entry
    /// names keep theirs under every usage.
    #[test]
    fn a_usage_applies_its_own_events_entries_and_no_others() {
        let doc = usage_document(
            "/OFF [11 0 R 13 0 R] /AS [\
             << /Event /View /Category [/View] /OCGs [13 0 R] >>\
             << /Event /Print /Category [/Print] /OCGs [13 0 R] >>\
             << /Event /Export /Category [/Export] /OCGs [13 0 R] >>]",
            "/View << /ViewState /ON >> /Print << /PrintState /ON >> /Export << /ExportState /OFF >>",
            "",
        );
        for (usage, gamma) in [
            (None, false),
            (Some(Usage::View), true),
            (Some(Usage::Print), true),
            (Some(Usage::Export), false),
        ] {
            let oc = OptionalContent::bind_for(&doc, usage);
            assert_eq!(state(&oc, 13), Some(gamma), "{usage:?}");
            assert_eq!(state(&oc, 10), Some(true), "{usage:?}: untouched");
            assert_eq!(state(&oc, 11), Some(false), "{usage:?}: untouched");
        }
        // `bind` is the initial states.
        assert_eq!(state(&OptionalContent::bind(&doc), 13), Some(false));
    }

    /// **A group is on only if every recommendation it has is**: across its
    /// entry's categories and across entries of the event; a category with no
    /// entry, or no state, recommends nothing, and so do `/Language` and
    /// `/User` where the render states no language and no user; a `/Zoom`
    /// recommends by the magnification, 1 for `bind_for`; and a group with no
    /// recommendation keeps `/D`'s state.
    #[test]
    fn every_recommendation_must_be_on() {
        let print = Some(Usage::Print);
        // (/D's body, /AS, group 13's usage entries, 13 in print)
        let cases = [
            (
                "",
                "[<< /Event /Print /Category [/View /Print] /OCGs [13 0 R] >>]",
                "/View << /ViewState /OFF >> /Print << /PrintState /ON >>",
                Some(false),
            ),
            (
                "/OFF [13 0 R]",
                "[<< /Event /Print /Category [/View /Print] /OCGs [13 0 R] >>]",
                "/View << /ViewState /ON >> /Print << /PrintState /ON >>",
                Some(true),
            ),
            (
                "",
                "[<< /Event /Print /Category [/Print] /OCGs [13 0 R] >> \
                  << /Event /Print /Category [/View] /OCGs [11 0 R 13 0 R] >>]",
                "/View << /ViewState /OFF >> /Print << /PrintState /ON >>",
                Some(false),
            ),
            (
                "/OFF [13 0 R]",
                "[<< /Event /Print /Category [/Export /Print] /OCGs [13 0 R] >>]",
                "/Print << /PrintState /ON >>",
                Some(true),
            ),
            (
                "/OFF [13 0 R]",
                "[<< /Event /Print /Category [/Print] /OCGs [13 0 R] >>]",
                "/Print << /Subtype /Watermark >>",
                Some(false),
            ),
            (
                "",
                "[<< /Event /Print /Category [/Language /User /CreatorInfo /PageElement] \
                  /OCGs [13 0 R] >>]",
                "/Language << /Lang (xx) >> /User << /Type /Ind /Name (x) >>",
                Some(true),
            ),
            (
                "",
                "[<< /Event /Print /Category [/Zoom /CreatorInfo] /OCGs [13 0 R] >>]",
                "/Zoom << /max 0.5 >>",
                Some(false),
            ),
        ];
        for (index, (d, auto_state, usage, expected)) in cases.iter().enumerate() {
            let doc = usage_document(&format!("{d} /AS {auto_state}"), usage, "");
            assert_eq!(
                state(&OptionalContent::bind_for(&doc, print), 13),
                *expected,
                "case {index}"
            );
        }
        // Group 11 has no `/Usage`: listed in the third case's view entry,
        // it recommends nothing and stays on.
        let doc = usage_document(
            "/AS [<< /Event /Print /Category [/View] /OCGs [11 0 R] >>]",
            "",
            "",
        );
        assert_eq!(
            state(&OptionalContent::bind_for(&doc, print), 11),
            Some(true)
        );
    }

    /// **Whatever is malformed leaves the group as `/D` set it**, in every
    /// usage: an `/AS` that is no array, entries that are no dictionaries, an
    /// unknown or absent `/Event`, a `/Category` absent or no array, an
    /// absent `/OCGs`, a usage or a category that is no dictionary, a state
    /// that is neither name.
    #[test]
    fn whatever_is_malformed_leaves_d() {
        let cases = [
            ("/AS 5", "/Print << /PrintState /OFF >>"),
            ("/AS (no)", "/Print << /PrintState /OFF >>"),
            ("/AS [5 /Print (x) [1 2]]", "/Print << /PrintState /OFF >>"),
            (
                "/AS [<< /Event /Printing /Category [/Print] /OCGs [13 0 R] >>]",
                "/Print << /PrintState /OFF >>",
            ),
            (
                "/AS [<< /Event (Print) /Category [/Print] /OCGs [13 0 R] >>]",
                "/Print << /PrintState /OFF >>",
            ),
            (
                "/AS [<< /Category [/Print] /OCGs [13 0 R] >>]",
                "/Print << /PrintState /OFF >>",
            ),
            (
                "/AS [<< /Event /Print /Category /Print /OCGs [13 0 R] >>]",
                "/Print << /PrintState /OFF >>",
            ),
            (
                "/AS [<< /Event /Print /OCGs [13 0 R] >>]",
                "/Print << /PrintState /OFF >>",
            ),
            (
                "/AS [<< /Event /Print /Category [/Print] >>]",
                "/Print << /PrintState /OFF >>",
            ),
            (
                "/AS [<< /Event /Print /Category [/Print] /OCGs [13 0 R] >>]",
                "/Print /OFF",
            ),
            (
                "/AS [<< /Event /Print /Category [/Print] /OCGs [13 0 R] >>]",
                "/Print << /PrintState /off >>",
            ),
            (
                "/AS [<< /Event /Print /Category [/Print] /OCGs [13 0 R] >>]",
                "/Print << /PrintState 0 >>",
            ),
        ];
        for (auto_state, usage) in cases {
            let doc = usage_document(&format!("/ON [13 0 R] {auto_state}"), usage, "");
            for render in USAGES {
                assert_eq!(
                    state(&OptionalContent::bind_for(&doc, render), 13),
                    Some(true),
                    "{auto_state} with {usage}, {render:?}"
                );
            }
        }
        // A `/Usage` that is no dictionary, and an entry reached by reference.
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 13 0 R] /D << /AS [14 0 R] >> >>",
            "13 0 obj\n<< /Type /OCG /Name (Gamma) /Usage 10 >>\nendobj\n\
             14 0 obj\n<< /Event /Print /Category [/Print] /OCGs [10 0 R 13 0 R] >>\nendobj\n",
        );
        let oc = OptionalContent::bind_for(&doc, Some(Usage::Print));
        assert_eq!(state(&oc, 13), Some(true), "a /Usage that is no dictionary");
        assert_eq!(state(&oc, 10), Some(true), "10 has no /Usage");
    }

    /// An entry reached by reference applies, and a group `/OCGs` does not
    /// list takes its recommendation as a listed one does, as `/D`'s own
    /// `/ON` and `/OFF` reach one.
    #[test]
    fn an_indirect_entry_applies_and_an_unlisted_group_takes_its_state() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R] /D << /AS 14 0 R >> >>",
            "13 0 obj\n<< /Type /OCG /Name (Unlisted) /Usage << /Export << /ExportState /OFF >> >> >>\nendobj\n\
             14 0 obj\n[15 0 R]\nendobj\n\
             15 0 obj\n<< /Event /Export /Category [/Export] /OCGs [13 0 R] >>\nendobj\n",
        );
        assert_eq!(
            state(&OptionalContent::bind_for(&doc, Some(Usage::Export)), 13),
            Some(false)
        );
        assert_eq!(
            state(&OptionalContent::bind_for(&doc, None), 13),
            None,
            "never mentioned"
        );
        assert!(OptionalContent::bind(&doc).is_visible(ObjRef { num: 13, gen: 0 }));
    }

    /// A page whose content, a form's, a tiling cell's and an annotation's
    /// appearance each mark content with group 10, which the default
    /// configuration's print entry turns off: every scope a render opens asks
    /// about optional content. The annotation's `/F` sets Print, so a print
    /// render draws it too (12.5.3).
    fn every_scope_page() -> Vec<u8> {
        let marked = "/OC /L BDC 0 0 2 2 re f EMC";
        let props = "/Properties << /L 10 0 R >>";
        let stream = |dict: &str| {
            format!(
                "<< {dict} /Resources << {props} >> /Length {} >>\nstream\n{marked}\nendstream",
                marked.len()
            )
        };
        let content = format!("{marked} /Fm Do /Pattern cs /P0 scn 10 0 10 10 re f");
        format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [10 0 R] \
/D << /AS [<< /Event /Print /Category [/Print] /OCGs [10 0 R] >>] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 30 30] /Annots [8 0 R]\n\
   /Resources << {props} /XObject << /Fm 6 0 R >> /Pattern << /P0 7 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
6 0 obj\n{}\nendobj\n\
7 0 obj\n{}\nendobj\n\
8 0 obj\n<< /Type /Annot /Subtype /Square /F 4 /Rect [20 20 30 30] /AP << /N 9 0 R >> >>\nendobj\n\
9 0 obj\n{}\nendobj\n\
10 0 obj\n<< /Type /OCG /Name (Alpha) /Usage << /Print << /PrintState /OFF >> >> >>\nendobj\n\
trailer\n<< /Size 11 /Root 1 0 R >>\n%%EOF\n",
            content.len(),
            stream("/Type /XObject /Subtype /Form /BBox [0 0 5 5]"),
            stream("/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 5 5] /XStep 5 /YStep 5"),
            stream("/Type /XObject /Subtype /Form /BBox [0 0 5 5]"),
        )
        .into_bytes()
    }

    /// **One render binds one configuration** (RC-13.2): a render whose page,
    /// form, tiling cell and annotation appearance each ask about optional
    /// content binds the states once, named usage or none, where each scope
    /// bound them for itself; so do a text extraction and a display list's
    /// recording, each one operation; a replay of the list's own usage binds
    /// once too, for the tiling cell it interprets as it draws it, and a
    /// render of another usage is a direct render, binding once.
    #[test]
    fn one_render_binds_one_configuration() {
        let doc = crate::Document::open(every_scope_page()).expect("it opens");
        let page = doc.page(0).expect("a page");
        for usage in USAGES {
            binds::take();
            let bitmap = page.render(&crate::RenderOptions {
                usage,
                ..crate::RenderOptions::default()
            });
            assert_eq!(binds::take(), 1, "a render for {usage:?}");
            assert_eq!(
                bitmap.warnings.iter().any(|warning| matches!(
                    warning,
                    crate::RenderWarning::HiddenOptionalContent { .. }
                )),
                usage == Some(Usage::Print),
                "{usage:?}: print hides the layer, nothing else does"
            );
        }
        binds::take();
        let _ = page.text();
        assert_eq!(binds::take(), 1, "a text extraction");

        let list = page.display_list_for(Some(Usage::Print));
        assert_eq!(binds::take(), 1, "the list's recording");
        let print = crate::RenderOptions {
            usage: Some(Usage::Print),
            ..crate::RenderOptions::default()
        };
        let _ = list.render(&print);
        assert_eq!(binds::take(), 1, "a replay, for the cell it draws");
        let _ = list.render(&crate::RenderOptions::default());
        assert_eq!(binds::take(), 1, "another usage, the direct way");
    }

    /// **A configuration is applied over `/D`'s initial states** (8.11.4.3
    /// Table 99, RC-14.2): `/BaseState /OFF` sets every listed group off,
    /// `/Unchanged` keeps `/D`'s and then takes its own `/ON` and `/OFF`, and
    /// a `/BaseState` none of the three names is `/ON`; an entry that is no
    /// dictionary, and an index past the array, bind `/D`'s states, and are
    /// what `configuration_readable` refuses and the listing holds as `None`.
    #[test]
    fn a_configuration_is_applied_over_d_s_initial_states() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> \
             /Configs [<< /BaseState /OFF >> \
             << /BaseState /Unchanged /ON [11 0 R] /OFF [10 0 R] >> \
             << /BaseState /Wrong >> 4] >>",
            "",
        );
        for (configuration, alpha, beta) in [
            (None, true, false),
            (Some(0), false, false),
            (Some(1), false, true),
            (Some(2), true, true),
            (Some(3), true, false),
            (Some(9), true, false),
            (Some(usize::MAX), true, false),
        ] {
            let oc = OptionalContent::bind_with(&doc, None, &configured(configuration), 1.0);
            assert_eq!(state(&oc, 10), Some(alpha), "{configuration:?}: Alpha");
            assert_eq!(state(&oc, 11), Some(beta), "{configuration:?}: Beta");
        }
        assert_eq!(
            OptionalContent::bind_with(&doc, None, &configured(Some(3)), 1.0),
            OptionalContent::bind(&doc),
            "an unreadable configuration binds /D's states"
        );
        for (index, readable) in [(0, true), (1, true), (2, true), (3, false), (4, false)] {
            assert_eq!(
                super::configuration_readable(&doc, index),
                readable,
                "{index}"
            );
        }
        assert_eq!(
            super::configurations(&doc)
                .iter()
                .map(Option::is_some)
                .collect::<Vec<_>>(),
            vec![true, true, true, false]
        );
    }

    /// **A chosen configuration is bound once a render** (RC-14.2): a direct
    /// render naming one binds once; a list recorded under it binds once, and
    /// its replay once more, for the tiling cell it interprets as it draws;
    /// a render of that list under another configuration binds the render's
    /// states once, to compare them with the list's, and draws with that
    /// binding — the direct way where they differ, a replay where they agree.
    #[test]
    fn a_chosen_configuration_is_bound_once_a_render() {
        let bytes = String::from_utf8(every_scope_page())
            .expect("the page is text")
            .replace(
                "/OCProperties << /OCGs [10 0 R] ",
                "/OCProperties << /OCGs [10 0 R] /Configs [<< /BaseState /OFF >> << >>] ",
            );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        let configuration = |configuration| crate::RenderOptions {
            layers: configured(configuration),
            ..crate::RenderOptions::default()
        };
        binds::take();
        let direct = page.render(&configuration(Some(0)));
        assert_eq!(binds::take(), 1, "a direct render naming a configuration");
        let list = page.display_list_with(None, &configured(Some(0)));
        assert_eq!(binds::take(), 1, "the list's recording");
        let replayed = list.render(&configuration(Some(0)));
        assert_eq!(binds::take(), 1, "its replay, for the cell it draws");
        assert!(replayed.data == direct.data);
        let other = list.render(&configuration(None));
        assert_eq!(binds::take(), 1, "another configuration, the direct way");
        assert!(other.data == page.render(&configuration(None)).data);
        binds::take();
        // The second entry, an empty dictionary, shows every group: the
        // states of `/D`, whose list replays it, binding once to compare.
        let d_list = page.display_list();
        binds::take();
        let alike = d_list.render(&configuration(Some(1)));
        assert_eq!(binds::take(), 1, "states alike, a replay");
        assert!(alike.data == page.render(&configuration(None)).data);
    }

    /// The layer options naming `configuration` and turning each of
    /// `overrides`' groups, by object number, on or off.
    fn overriding(configuration: Option<usize>, overrides: &[(u32, bool)]) -> crate::LayerOptions {
        crate::LayerOptions {
            configuration,
            overrides: overrides
                .iter()
                .map(|&(num, on)| (ObjRef { num, gen: 0 }, on))
                .collect(),
            ..crate::LayerOptions::default()
        }
    }

    /// **Overrides apply last, in order, under the configuration's radio
    /// buttons and locks** (RC-14.3, 8.11.4.3 Table 99): under `/D`, turning
    /// Alpha on turns Beta off and leaves 12, locked, on, refused by its
    /// reference since it has no `/Name`; under a configuration with no
    /// `/RBGroups` of its own, `/D`'s collection constrains and `/D`'s lock does
    /// not; under one with an empty collection list and a lock of its own,
    /// nothing is turned off and the locked Alpha is refused by its `/Name`,
    /// once however often it is asked; a group `/OCGs` does not list is
    /// passed over. What a binding refused is no part of its equality, and
    /// the collections do not constrain a configuration's own `/ON`.
    #[test]
    fn overrides_apply_in_order_under_radio_buttons_and_locks() {
        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R 12 0 R] \
             /D << /RBGroups [[10 0 R 11 0 R 12 0 R]] /Locked [12 0 R] >> \
             /Configs [<< /BaseState /OFF >> << /RBGroups [] /Locked [10 0 R] >>] >>",
            "12 0 obj\n<< /Type /OCG >>\nendobj\n",
        );
        let bound = |configuration, overrides: &[(u32, bool)]| {
            OptionalContent::bind_with(&doc, None, &overriding(configuration, overrides), 1.0)
        };
        let states = |oc: &OptionalContent| (state(oc, 10), state(oc, 11), state(oc, 12));

        let oc = bound(None, &[(10, true)]);
        assert_eq!(states(&oc), (Some(true), Some(false), Some(true)), "/D");
        assert_eq!(oc.refused(), ["12 0 R"], "the lock, by reference");

        let oc = bound(Some(0), &[(12, true), (11, true)]);
        assert_eq!(
            states(&oc),
            (Some(false), Some(true), Some(false)),
            "/D's collection, and not /D's lock"
        );
        assert!(oc.refused().is_empty());

        let oc = bound(Some(1), &[(10, false), (11, true), (13, false), (10, true)]);
        assert_eq!(
            states(&oc),
            (Some(true), Some(true), Some(true)),
            "its own empty collections and its own lock"
        );
        assert_eq!(oc.refused(), ["Alpha"], "once, by /Name");
        assert_eq!(state(&oc, 13), None, "an unlisted group is passed over");

        assert_eq!(bound(None, &[]), OptionalContent::bind(&doc));
        assert!(bound(None, &[]).refused().is_empty());
        assert_eq!(
            bound(None, &[(12, false)]),
            OptionalContent::bind(&doc),
            "a refused override binds the states it found"
        );

        let doc = document(
            "/OCProperties << /OCGs [10 0 R 11 0 R] \
             /D << /RBGroups [[10 0 R 11 0 R]] >> >>",
            "",
        );
        let oc = OptionalContent::bind(&doc);
        assert_eq!(
            (state(&oc, 10), state(&oc, 11)),
            (Some(true), Some(true)),
            "/D's own states show both members"
        );
    }

    /// **A language tag matches whole, or by its primary subtag** (RC-14.4,
    /// 8.11.4.4): the whole tag ignoring ASCII case is an exact match; the
    /// subtag before the first hyphen alike, the rest not, a partial one;
    /// anything else, an empty tag's primary subtag among it, neither.
    #[test]
    fn a_language_tag_matches_whole_or_by_its_primary_subtag() {
        use super::Match;
        for (lang, wanted, found) in [
            ("en-US", "en-US", Match::Exact),
            ("en-us", "EN-US", Match::Exact),
            ("en", "en", Match::Exact),
            ("es-MX", "es-ES", Match::Partial),
            ("en", "en-GB", Match::Partial),
            ("en-GB", "en", Match::Partial),
            ("zh-Hant-TW", "ZH-cn", Match::Partial),
            ("fr", "de", Match::Neither),
            ("en", "eng", Match::Neither),
            ("", "en", Match::Neither),
            ("-US", "-GB", Match::Neither),
            ("", "", Match::Exact),
        ] {
            assert_eq!(Match::of(lang, wanted), found, "{lang} for {wanted}");
        }
    }

    /// **The zoom, language and user categories read the render's inputs**
    /// (RC-14.4, 8.11.4.4): a `/Zoom` recommends by the magnification, which
    /// the binding says it read; a `/Language` decides over its entry's
    /// groups together; a `/User` compares the identification of its
    /// `/Type`'s kind; and a binding no `/Zoom` entry recommended for reads no
    /// magnification, whatever else it read.
    #[test]
    fn the_zoom_language_and_user_categories_read_the_renders_inputs() {
        let doc = document(
            "/OCProperties << /OCGs [12 0 R 13 0 R 14 0 R 15 0 R] /D << /AS [\
             << /Event /View /Category [/Zoom] /OCGs [12 0 R] >> \
             << /Event /View /Category [/Language] /OCGs [13 0 R 14 0 R] >> \
             << /Event /View /Category [/User] /OCGs [15 0 R] >>] >> >>",
            "12 0 obj\n<< /Type /OCG /Usage << /Zoom << /min 2 >> >> >>\nendobj\n\
             13 0 obj\n<< /Type /OCG /Usage << /Language << /Lang (pt-BR) >> >> >>\nendobj\n\
             14 0 obj\n<< /Type /OCG /Usage << /Language << /Lang (pt-PT) /Preferred /ON >> >> >>\nendobj\n\
             15 0 obj\n<< /Type /OCG /Usage << /User << /Type /Org /Name (Tinker) >> >> >>\nendobj\n",
        );
        let layers = |language: Option<&str>, organisation: Option<&str>| crate::LayerOptions {
            language: language.map(str::to_string),
            user: crate::LayerUser {
                organisation: organisation.map(str::to_string),
                ..crate::LayerUser::default()
            },
            ..crate::LayerOptions::default()
        };
        let view = Some(Usage::View);

        let oc = OptionalContent::bind_with(&doc, view, &layers(None, None), 1.0);
        assert_eq!(state(&oc, 12), Some(false), "below /min");
        assert!(oc.reads_magnification());
        assert_eq!(
            (state(&oc, 13), state(&oc, 14), state(&oc, 15)),
            (Some(true), Some(true), Some(true)),
            "no language and no user stated"
        );
        let oc =
            OptionalContent::bind_with(&doc, view, &layers(Some("pt-BR"), Some("Tinker")), 4.0);
        assert_eq!(
            (
                state(&oc, 12),
                state(&oc, 13),
                state(&oc, 14),
                state(&oc, 15)
            ),
            (Some(true), Some(true), Some(false), Some(true))
        );
        let oc = OptionalContent::bind_with(&doc, view, &layers(Some("pt-AO"), Some("Other")), 4.0);
        assert_eq!(
            (state(&oc, 13), state(&oc, 14), state(&oc, 15)),
            (Some(false), Some(true), Some(false)),
            "a partial match takes its preference"
        );
        assert_eq!(
            OptionalContent::bind_with(&doc, view, &layers(None, None), f64::NAN),
            OptionalContent::bind_with(&doc, view, &layers(None, None), 1.0),
            "a magnification that is no number above zero is 1"
        );
        let oc =
            OptionalContent::bind_with(&doc, Some(Usage::Print), &layers(Some("pt-BR"), None), 4.0);
        assert!(!oc.reads_magnification(), "no print entry reads it");
        assert_eq!(state(&oc, 12), Some(true), "/D's own state");
        assert!(!OptionalContent::bind(&doc).reads_magnification());
    }
}
