//! Sanitising: taking JavaScript, outward-reaching actions, embedded files
//! and metadata out of a document, and saying exactly what left.
//!
//! # A sweep, not the script walkers
//!
//! [`crate::form::script_summary`] and its walkers answer "what script does
//! this document carry" for a *form*: the field tree's `/AA`, the catalog's
//! `/AA`, `/Names /JavaScript`. They do not look at a page's `/AA`, an
//! annotation's `/AA` or `/A`, an outline item's `/A`, or an action's `/Next`
//! chain — every one of which a viewer runs. So removal is a sweep over every
//! object the editor has, and the walkers are how the tests prove the sweep
//! left nothing they can see.
//!
//! # What counts as an action
//!
//! Where a viewer runs one, whatever else it carries. A viewer looks at the
//! place a dictionary sits and at its `/S`, resolving a reference, and at
//! nothing more: an extra `/K`, a `/Type /Whatever` or an `/S 9 0 R` does not
//! stop it running the script. So in an **action slot** — `/A`, `/PA`, `/NA`,
//! `/OpenAction`, every entry of an `/AA`, and an action's `/Next`, a single
//! action or an array of them — a dictionary whose `/S` names one of ISO
//! 32000-2 Table 198's action types is an action, and nothing else about it
//! is asked. An object written on its own sits in every slot a reference to
//! it sits in, found by following the slots out from every object before
//! anything is cleaned, so an indirect `/AA` dictionary or `/Next` array is
//! swept as what it is.
//!
//! Anywhere else, a stricter test finds actions a producer put in a place
//! that list does not name: the `/S` as before, a `/Type` of `/Action` or
//! none, and neither `/P` nor `/K` — the two keys every structure element has
//! (14.7.2 Table 355) and no action needs, so a structure element role-mapped
//! to `URI` in the structure tree, where no viewer runs anything, is not
//! mistaken for one. A value that is an action by the test its place calls
//! for, directly or by reference, is removed wherever it sits.
//!
//! # A link whose action goes, goes with it
//!
//! A `/Link` annotation (12.5.6.5) exists to be followed. One whose `/A` is
//! removed and which has no `/Dest` is a hot spot that goes nowhere — the
//! strict validator refuses it as `LinkWithoutTarget` — so it leaves its
//! page's `/Annots` with its action, for the action's reason. A widget whose
//! `/A` goes stays: it is a form field first. `/Annots` and an array in an
//! action slot are the only arrays anything is removed from, because a name
//! tree's `/Names` pairs are positional and taking one element out elsewhere
//! could shift every key onto the wrong value.
//!
//! # What leaves the file
//!
//! An entry removed is a *reference* removed; what it referred to is deleted
//! only when nothing still in the document reaches it afterwards, which is
//! decided on the document as it will be rather than as it was — the pending
//! page order included, so a page `import_page` added, which only a save
//! writes into `/Kids`, keeps what it draws. So a JavaScript action and its
//! `/JS` stream go, a page an action's `/Next` pointed at stays, and no
//! reference anywhere is left dangling. An action removed takes its `/Next`
//! chain with it — the chain is part of the action (12.6.2) — except for
//! whatever else still reaches.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::save::refs_in;
use super::DocumentEditor;
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::resolve::Resolve;
use crate::write::{StreamData, Written};

/// What [`DocumentEditor::sanitise`] takes out. Each is independent;
/// [`Sanitise::ALL`] is all four.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sanitise {
    /// Every JavaScript action (12.6.4.17) — a `/S /JavaScript`, a `/URI`
    /// action whose URI is a `javascript:` one, a `/Rendition` carrying
    /// `/JS` — wherever an action sits; the document-level scripts in
    /// `/Names /JavaScript` (7.7.4); and from `/AcroForm`, the calculation
    /// order `/CO`, which orders calculate scripts that no longer exist
    /// (12.7.2 Table 218), and `/XFA`, whose packets carry scripts of their
    /// own.
    pub javascript: bool,
    /// Every action that reaches outside the document, sends its data
    /// somewhere or plays media: `/Launch`, `/URI`, `/SubmitForm`,
    /// `/ImportData`, `/GoToR`, `/GoToE`, `/Sound`, `/Movie`, `/Rendition`
    /// and `/RichMediaExecute` (12.6.4). Navigation inside the document —
    /// `/GoTo`, `/Named`, `/Thread`, `/Hide`, `/ResetForm`, `/SetOCGState`,
    /// `/Trans`, `/GoTo3DView`, `/GoToDp` — stays.
    pub actions: bool,
    /// Every embedded file (7.11.4): `/Names /EmbeddedFiles`, and the `/EF`
    /// and `/RF` of every file specification — a file attachment
    /// annotation's among them, which keeps its name and loses its bytes.
    pub embedded_files: bool,
    /// The document information dictionary (14.3.3) — the trailer's `/Info`,
    /// and the catalog's where a producer put one there — and every
    /// `/Metadata` stream (14.3.2), the catalog's and any other object's.
    pub metadata: bool,
}

impl Sanitise {
    /// All four.
    pub const ALL: Sanitise = Sanitise {
        javascript: true,
        actions: true,
        embedded_files: true,
        metadata: true,
    };
}

/// Where a removed entry was.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntryHolder {
    /// The trailer (7.5.5).
    Trailer,
    /// An object still in the document.
    Object(ObjRef),
}

/// One step from a holder to a removed value.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PathStep {
    /// A dictionary key, as its bytes.
    Key(Vec<u8>),
    /// An array position, counted in the array **as it was** before anything
    /// was removed from it.
    Index(usize),
}

/// Why something was removed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Removal {
    /// A JavaScript action ([`Sanitise::javascript`]).
    JavaScript,
    /// `/Names /JavaScript`: the document-level scripts.
    DocumentJavaScript,
    /// `/AcroForm /CO`: the calculation order.
    CalculationOrder,
    /// `/AcroForm /XFA`: an XFA form's packets.
    XfaForm,
    /// An outward-reaching action ([`Sanitise::actions`]), by its `/S`.
    Action(Vec<u8>),
    /// `/Names /EmbeddedFiles`: the attachment tree.
    EmbeddedFileTree,
    /// A file specification's `/EF` or `/RF`, or an embedded file stream.
    EmbeddedFile,
    /// `/Info`.
    Info,
    /// A `/Metadata` stream.
    Metadata,
}

/// One entry removed from an object that stays, or from the trailer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemovedEntry {
    /// The object, or the trailer, it was removed from.
    pub holder: EntryHolder,
    /// The keys and positions from the holder down to the removed value; the
    /// last step is the one removed.
    pub path: Vec<PathStep>,
    /// Why.
    pub what: Removal,
}

/// One object deleted, because only entries this pass removed reached it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeletedObject {
    /// The object.
    pub object: ObjRef,
    /// Why the entry that reached it was removed.
    pub what: Removal,
}

/// Everything [`DocumentEditor::sanitise`] took out, in object order.
///
/// Together the two lists account for every change the pass made: an object
/// in neither is exactly as it was.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SanitiseReport {
    /// Entries removed from objects that are still in the document, and from
    /// the trailer.
    pub removed: Vec<RemovedEntry>,
    /// Objects deleted.
    pub deleted: Vec<DeletedObject>,
}

impl SanitiseReport {
    /// Whether the pass found nothing to take out.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.deleted.is_empty()
    }
}

/// Table 198 of ISO 32000-2: every action type.
const ACTION_TYPES: [&[u8]; 20] = [
    b"GoTo",
    b"GoToR",
    b"GoToE",
    b"GoToDp",
    b"Launch",
    b"Thread",
    b"URI",
    b"Sound",
    b"Movie",
    b"Hide",
    b"Named",
    b"SubmitForm",
    b"ResetForm",
    b"ImportData",
    b"SetOCGState",
    b"Rendition",
    b"Trans",
    b"GoTo3DView",
    b"JavaScript",
    b"RichMediaExecute",
];

/// The action types [`Sanitise::actions`] removes.
const OUTWARD: [&[u8]; 10] = [
    b"Launch",
    b"URI",
    b"SubmitForm",
    b"ImportData",
    b"GoToR",
    b"GoToE",
    b"Sound",
    b"Movie",
    b"Rendition",
    b"RichMediaExecute",
];

/// One removal inside an object: the path to it and why.
type Found = (Vec<PathStep>, Removal);

/// Which dictionary a value is, where that decides what may be removed from
/// it beyond the rules every dictionary gets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Catalog,
    Names,
    AcroForm,
    Other,
}

/// Where a value sits, where that decides what it is and what may be
/// removed from it. Flags rather than one kind, because an object written on
/// its own sits wherever a reference to it does, and a file may name one
/// object from two places: it is swept as everything it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Slot {
    /// A viewer runs what sits here as an action (12.6): a dictionary here
    /// is judged by its `/S` alone, and an array here — an action's `/Next`
    /// (12.6.2) — is a list of actions, each of which may go.
    action: bool,
    /// An additional-actions dictionary (12.6.3): every entry is an action
    /// slot.
    triggers: bool,
    /// A page's `/Annots` (12.5.2): a link this pass leaves targetless goes.
    annots: bool,
}

impl Slot {
    /// Anywhere else: an array here loses nothing, only is cleaned inside.
    const PLAIN: Slot = Slot {
        action: false,
        triggers: false,
        annots: false,
    };

    fn union(self, other: Slot) -> Slot {
        Slot {
            action: self.action || other.action,
            triggers: self.triggers || other.triggers,
            annots: self.annots || other.annots,
        }
    }

    /// The slot an element of an array in this one sits in: an action's
    /// again when this is a list of actions, and nothing in particular
    /// otherwise — an annotation in `/Annots` is a dictionary like any other.
    fn element(self) -> Slot {
        Slot {
            action: self.action,
            ..Slot::PLAIN
        }
    }
}

/// The names the sweep compares against, interned once.
struct Keys {
    s: Name,
    p: Name,
    k: Name,
    next: Name,
    pa: Name,
    na: Name,
    open_action: Name,
    aa: Name,
    js: Name,
    uri: Name,
    metadata: Name,
    ef: Name,
    rf: Name,
    info: Name,
    names: Name,
    acro_form: Name,
    javascript: Name,
    embedded_files: Name,
    co: Name,
    xfa: Name,
    action: Name,
    embedded_file: Name,
    annots: Name,
    subtype: Name,
    link: Name,
    dest: Name,
    a: Name,
}

/// One pass's read-only half: what each object becomes and what was taken
/// out of it, decided before anything is written.
struct Sweep<'e> {
    editor: &'e DocumentEditor,
    what: Sanitise,
    keys: Keys,
    catalog: Option<u32>,
    names: Option<u32>,
    acro_form: Option<u32>,
    /// Link annotations, by object number, that leave their page because
    /// their action does — found before the sweep, since a page is swept
    /// before the annotations its `/Annots` names.
    links: HashMap<u32, Removal>,
    /// The slot every object written on its own sits in, where that is not
    /// a plain one — found before the sweep, for the same reason.
    placed: HashMap<u32, Slot>,
    /// Removals inside the object being cleaned.
    removed: Vec<Found>,
    /// Every value removed anywhere, for the deletion pass to follow.
    roots: Vec<(Object, Removal)>,
}

impl Sweep<'_> {
    fn bytes(&self, name: Name) -> Vec<u8> {
        self.editor
            .name_bytes(name)
            .map(|b| b.to_vec())
            .unwrap_or_default()
    }

    /// Why `value` is removed, when it is an action this pass removes —
    /// written in place or reached through one reference. `slotted` when it
    /// sits in an action slot (see the module documentation for the two
    /// tests).
    ///
    /// Through [`Resolve::get`], which lends the file's objects from its cache
    /// rather than copying them: this is asked of every reference in every
    /// dictionary, `/Parent` included, and a copy of a page tree node per
    /// page is quadratic in the page count.
    fn verdict_of(&self, value: &Object, slotted: bool) -> Option<Removal> {
        match value {
            Object::Dict(dict) => self.verdict(dict, slotted),
            Object::Ref(r) => {
                let object = Resolve::get(self.editor, *r).ok()?;
                self.verdict(object.as_dict()?, slotted)
            }
            _ => None,
        }
    }

    /// The action type `dict` is, when it is an action: in an action slot
    /// (`slotted`) by its `/S` alone, and elsewhere by the stricter test the
    /// module documentation gives.
    ///
    /// The `/S` is resolved when it is a reference, as a viewer resolves it
    /// (7.3.10: any value may be written as one).
    fn action_type(&self, dict: &Dict, slotted: bool) -> Option<Vec<u8>> {
        if !slotted {
            if dict.contains_key(self.keys.p) || dict.contains_key(self.keys.k) {
                return None;
            }
            if let Some(t) = dict.get(Name::TYPE) {
                if t.as_name() != Some(self.keys.action) {
                    return None;
                }
            }
        }
        let s = match dict.get(self.keys.s)? {
            Object::Name(name) => *name,
            Object::Ref(r) => Resolve::get(self.editor, *r).ok()?.as_name()?,
            _ => return None,
        };
        let s = self.bytes(s);
        ACTION_TYPES.contains(&s.as_slice()).then_some(s)
    }

    /// Whether a dictionary in `slot` is an action for the purpose of its
    /// `/Next`: one in an action slot is, whatever its `/S` says, because a
    /// viewer follows the chain of whatever it was handed there.
    fn chains(&self, dict: &Dict, slot: Slot) -> bool {
        slot.action || self.action_type(dict, false).is_some()
    }

    /// The slot the value under `key` sits in, in a dictionary in `slot`
    /// that [`Sweep::chains`] (`chains`) or not.
    fn entry_slot(&self, slot: Slot, chains: bool, key: Name) -> Slot {
        let keys = &self.keys;
        Slot {
            // 12.6.3's triggers, 12.5.6.5's `/A` and `/PA`, 12.4.4.2's
            // navigation node `/NA` and `/PA`, 12.3.3's outline item `/A`,
            // 12.7.4's widget `/A`, 7.7.2's `/OpenAction`, 12.6.2's `/Next`.
            action: slot.triggers
                || key == keys.a
                || key == keys.pa
                || key == keys.na
                || key == keys.open_action
                || (chains && key == keys.next),
            triggers: key == keys.aa,
            annots: key == keys.annots,
        }
    }

    /// Every reference inside `value`, sitting in `slot`, that is in a slot
    /// other than a plain one — with that slot, for the object it names to be
    /// swept as what it is.
    fn mark(&self, value: &Object, slot: Slot, depth: u32, out: &mut Vec<(u32, Slot)>) {
        if depth > limits::MAX_NEST_DEPTH {
            return;
        }
        match value {
            Object::Ref(r) => {
                if slot != Slot::PLAIN {
                    out.push((r.num, slot));
                }
            }
            Object::Dict(_) | Object::Stream(_) => {
                let Some(dict) = value.as_dict() else {
                    return;
                };
                let chains = self.chains(dict, slot);
                for (key, entry) in dict.iter() {
                    self.mark(entry, self.entry_slot(slot, chains, *key), depth + 1, out);
                }
            }
            Object::Array(items) => {
                for item in items {
                    self.mark(item, slot.element(), depth + 1, out);
                }
            }
            _ => {}
        }
    }

    /// Why `dict` is removed, if it is an action this pass removes;
    /// `slotted` as for [`Sweep::verdict_of`].
    fn verdict(&self, dict: &Dict, slotted: bool) -> Option<Removal> {
        let s = self.action_type(dict, slotted)?;
        if self.what.javascript && self.is_javascript(dict, &s) {
            return Some(Removal::JavaScript);
        }
        if self.what.actions && OUTWARD.contains(&s.as_slice()) {
            return Some(Removal::Action(s));
        }
        None
    }

    /// 12.6.4.17's action, and the two other actions that run script: a
    /// `javascript:` URI, which viewers hand to their script engine, and a
    /// rendition action's `/JS` (12.6.4.14).
    fn is_javascript(&self, dict: &Dict, s: &[u8]) -> bool {
        match s {
            b"JavaScript" => true,
            b"Rendition" => dict.contains_key(self.keys.js),
            b"URI" => {
                let uri = self.editor.resolve_key(dict, self.keys.uri);
                uri.as_string().is_some_and(|u| {
                    let trimmed: Vec<u8> = u
                        .bytes
                        .iter()
                        .copied()
                        .skip_while(|b| *b <= b' ')
                        .take(11)
                        .collect();
                    trimmed.eq_ignore_ascii_case(b"javascript:")
                })
            }
            _ => false,
        }
    }

    /// Why a link annotation leaves its page, if it does: a `/Link` with no
    /// `/Dest` whose `/A` this pass removes (12.5.6.5).
    fn link_verdict(&self, dict: &Dict) -> Option<Removal> {
        if dict.get_name(self.keys.subtype) != Some(self.keys.link)
            || dict.contains_key(self.keys.dest)
        {
            return None;
        }
        self.verdict_of(dict.get(self.keys.a)?, true)
    }

    /// The same for one element of an `/Annots` array: a link written in
    /// place, or one by reference the first pass found.
    fn annots_verdict(&self, item: &Object) -> Option<Removal> {
        match item {
            Object::Ref(r) => self.links.get(&r.num).cloned(),
            Object::Dict(dict) => self.link_verdict(dict),
            _ => None,
        }
    }

    /// Why one element of an array in `slot` goes, if it does: an action
    /// this pass removes from a list of actions, a link it leaves targetless
    /// from `/Annots`, and nothing from any other array.
    fn element_verdict(&self, slot: Slot, item: &Object) -> Option<Removal> {
        if slot.action {
            if let Some(why) = self.verdict_of(item, true) {
                return Some(why);
            }
        }
        if slot.annots {
            return self.annots_verdict(item);
        }
        None
    }

    /// Why the entry `key` of a dictionary in `role` is removed whole, if it
    /// is; `inner` is the slot the entry's value sits in.
    fn entry_verdict(&self, role: Role, key: Name, value: &Object, inner: Slot) -> Option<Removal> {
        let keys = &self.keys;
        let what = self.what;
        match role {
            Role::Names if what.javascript && key == keys.javascript => {
                return Some(Removal::DocumentJavaScript)
            }
            Role::Names if what.embedded_files && key == keys.embedded_files => {
                return Some(Removal::EmbeddedFileTree)
            }
            Role::AcroForm if what.javascript && key == keys.co => {
                return Some(Removal::CalculationOrder)
            }
            Role::AcroForm if what.javascript && key == keys.xfa => return Some(Removal::XfaForm),
            Role::Catalog if what.metadata && key == keys.info => return Some(Removal::Info),
            _ => {}
        }
        if what.metadata && key == keys.metadata {
            return Some(Removal::Metadata);
        }
        if what.embedded_files && (key == keys.ef || key == keys.rf) {
            return Some(Removal::EmbeddedFile);
        }
        self.verdict_of(value, inner.action)
    }

    /// Whether a whole object, sitting in `slot`, is itself something this
    /// pass takes out, found on its own rather than through a reference — an
    /// orphan the file still carries.
    fn object_verdict(&self, object: &Object, slot: Slot) -> Option<Removal> {
        let dict = object.as_dict()?;
        if let Some(verdict) = self.verdict(dict, slot.action) {
            return Some(verdict);
        }
        let kind = dict.get_name(Name::TYPE);
        if self.what.embedded_files && kind == Some(self.keys.embedded_file) {
            return Some(Removal::EmbeddedFile);
        }
        if self.what.metadata && kind == Some(self.keys.metadata) {
            return Some(Removal::Metadata);
        }
        None
    }

    /// `value` with everything this pass removes taken out of it, or `None`
    /// when nothing was.
    fn clean(
        &mut self,
        value: &Object,
        role: Role,
        slot: Slot,
        path: &mut Vec<PathStep>,
        depth: u32,
    ) -> Option<Object> {
        if depth > limits::MAX_NEST_DEPTH {
            return None;
        }
        match value {
            // A stream is cleaned as its dictionary (7.3.8), and comes back
            // as one: the data is not this pass's to change.
            Object::Dict(_) | Object::Stream(_) => {
                let dict = value.as_dict()?;
                let chains = self.chains(dict, slot);
                let mut out = Dict::with_capacity(dict.len());
                let mut changed = false;
                for (key, entry) in dict.iter() {
                    path.push(PathStep::Key(self.bytes(*key)));
                    let inner_slot = self.entry_slot(slot, chains, *key);
                    if let Some(why) = self.entry_verdict(role, *key, entry, inner_slot) {
                        self.removed.push((path.clone(), why.clone()));
                        self.roots.push((entry.clone(), why));
                        changed = true;
                    } else {
                        let inner = match (role, *key) {
                            (Role::Catalog, k) if k == self.keys.names => Role::Names,
                            (Role::Catalog, k) if k == self.keys.acro_form => Role::AcroForm,
                            _ => Role::Other,
                        };
                        match self.clean(entry, inner, inner_slot, path, depth + 1) {
                            Some(cleaned) => {
                                out.insert(*key, cleaned);
                                changed = true;
                            }
                            None => {
                                out.insert(*key, entry.clone());
                            }
                        }
                    }
                    path.pop();
                }
                changed.then_some(Object::Dict(out))
            }
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                let mut changed = false;
                for (index, item) in items.iter().enumerate() {
                    path.push(PathStep::Index(index));
                    // 12.6.2: `/Next` may be an array of actions, and 12.5.2's
                    // `/Annots` is a list of annotations; see the module
                    // documentation for why no other array loses an element.
                    if let Some(why) = self.element_verdict(slot, item) {
                        self.removed.push((path.clone(), why.clone()));
                        self.roots.push((item.clone(), why));
                        changed = true;
                    } else {
                        match self.clean(item, Role::Other, slot.element(), path, depth + 1) {
                            Some(cleaned) => {
                                out.push(cleaned);
                                changed = true;
                            }
                            None => out.push(item.clone()),
                        }
                    }
                    path.pop();
                }
                changed.then_some(Object::Array(out))
            }
            _ => None,
        }
    }
}

impl DocumentEditor {
    /// Takes out of the document what `what` names, and reports every entry
    /// and object that left.
    ///
    /// A sweep over **every object the editor has** — the file's and its own —
    /// rather than the form's script walkers, which do not see a page's or an
    /// annotation's `/AA`, an outline item's `/A` or an action's `/Next`. See
    /// [`Sanitise`] for what each switch covers and this module's
    /// documentation for what counts as an action.
    ///
    /// An entry is removed from the object holding it, which stays; an object
    /// that only removed entries reached is deleted, decided on the document
    /// as it will be — so nothing still in the document is left pointing at
    /// something deleted, and nothing still reached is deleted. The report
    /// accounts for every change: an object in neither of its lists is exactly
    /// as it was.
    ///
    /// **Save with [`crate::write::WriteMode::Rewrite`] for the removal to be
    /// real.** An incremental update appends (7.5.6): the original objects
    /// are still in the file's prefix, and a reader that scans bytes rather
    /// than following the trailer finds every script there — the caveat
    /// redaction carries, for the same reason.
    pub fn sanitise(&mut self, what: &Sanitise) -> SanitiseReport {
        let mut report = SanitiseReport::default();
        if *what == Sanitise::default() {
            return report;
        }
        let intern = |b: &[u8]| self.intern(b);
        let keys = Keys {
            s: intern(b"S"),
            p: intern(b"P"),
            k: intern(b"K"),
            next: intern(b"Next"),
            pa: intern(b"PA"),
            na: intern(b"NA"),
            open_action: intern(b"OpenAction"),
            aa: intern(b"AA"),
            js: intern(b"JS"),
            uri: intern(b"URI"),
            metadata: intern(b"Metadata"),
            ef: intern(b"EF"),
            rf: intern(b"RF"),
            info: Name::INFO,
            names: intern(b"Names"),
            acro_form: intern(b"AcroForm"),
            javascript: intern(b"JavaScript"),
            embedded_files: intern(b"EmbeddedFiles"),
            co: intern(b"CO"),
            xfa: intern(b"XFA"),
            action: intern(b"Action"),
            embedded_file: intern(b"EmbeddedFile"),
            annots: intern(b"Annots"),
            subtype: intern(b"Subtype"),
            link: intern(b"Link"),
            dest: intern(b"Dest"),
            a: intern(b"A"),
        };
        let catalog_ref = Resolve::trailer(self).get_ref(Name::ROOT);
        let catalog = self.catalog();
        let names_ref = catalog.as_ref().and_then(|c| c.get_ref(keys.names));
        let acro_form_ref = catalog.as_ref().and_then(|c| c.get_ref(keys.acro_form));

        // Every object the editor has, ascending, so the report is in object
        // order whatever order the overlay's map keeps.
        let mut numbers: BTreeSet<u32> = self
            .doc
            .xref()
            .iter()
            .map(|(num, _)| num)
            .filter(|num| *num != 0)
            .collect();
        numbers.extend(self.overlay.keys().copied());
        for num in &self.deleted {
            numbers.remove(num);
        }

        // The read-only half: what every object becomes.
        let mut sweep = Sweep {
            editor: self,
            what: *what,
            keys,
            catalog: catalog_ref.map(|r| r.num),
            names: names_ref.map(|r| r.num),
            acro_form: acro_form_ref.map(|r| r.num),
            links: HashMap::new(),
            placed: HashMap::new(),
            removed: Vec::new(),
            roots: Vec::new(),
        };
        // First, the links that leave with their actions, and the slot every
        // object written on its own sits in: marked out from every object,
        // then again from each object as it is placed, until nothing new is
        // placed. A slot only gains flags, so each object is walked at most
        // once per flag.
        let mut work: Vec<(u32, Slot)> = Vec::new();
        for &num in &numbers {
            let Ok(object) = Resolve::get(sweep.editor, ObjRef::new(num, 0)) else {
                continue;
            };
            if let Some(why) = object.as_dict().and_then(|d| sweep.link_verdict(d)) {
                sweep.links.insert(num, why);
            }
            sweep.mark(&object, Slot::PLAIN, 0, &mut work);
        }
        while let Some((num, slot)) = work.pop() {
            let known = sweep.placed.get(&num).copied().unwrap_or_default();
            let joined = known.union(slot);
            if joined == known {
                continue;
            }
            sweep.placed.insert(num, joined);
            if let Ok(object) = Resolve::get(sweep.editor, ObjRef::new(num, 0)) {
                sweep.mark(&object, joined, 0, &mut work);
            }
        }

        let mut pending: BTreeMap<u32, Object> = BTreeMap::new();
        let mut found: BTreeMap<u32, Vec<Found>> = BTreeMap::new();
        for &num in &numbers {
            let Ok(object) = Resolve::get(sweep.editor, ObjRef::new(num, 0)) else {
                continue;
            };
            let slot = sweep.placed.get(&num).copied().unwrap_or_default();
            // An orphan the file carries — a script nothing runs, a stream
            // nothing names — is found by what it is.
            if let Some(why) = sweep.object_verdict(&object, slot) {
                sweep.roots.push((Object::Ref(ObjRef::new(num, 0)), why));
            }
            let role = if Some(num) == sweep.catalog {
                Role::Catalog
            } else if Some(num) == sweep.names {
                Role::Names
            } else if Some(num) == sweep.acro_form {
                Role::AcroForm
            } else {
                Role::Other
            };
            let mut path = Vec::new();
            if let Some(cleaned) = sweep.clean(&object, role, slot, &mut path, 0) {
                pending.insert(num, cleaned);
                found.insert(num, std::mem::take(&mut sweep.removed));
            }
            sweep.removed.clear();
        }

        // The trailer's `/Info` (7.5.5 Table 15).
        let mut trailer = self.merged_trailer();
        let mut trailer_removed = false;
        if what.metadata {
            if let Some(info) = trailer.get(Name::INFO).cloned() {
                sweep.roots.push((info, Removal::Info));
                trailer = super::without(&trailer, Name::INFO);
                trailer_removed = true;
            }
        }
        let roots = std::mem::take(&mut sweep.roots);

        // What only removed entries reach: everything the removed values
        // reach, as the document was.
        let mut doomed: BTreeMap<u32, Removal> = BTreeMap::new();
        for (value, why) in &roots {
            let mut queue = Vec::new();
            refs_in(value, &mut queue, 0);
            while let Some(r) = queue.pop() {
                if doomed.contains_key(&r.num) || !numbers.contains(&r.num) {
                    continue;
                }
                doomed.insert(r.num, why.clone());
                if let Ok(object) = Resolve::get(self, r) {
                    refs_in(&object, &mut queue, 0);
                }
            }
        }

        // What the document will still reach, read through the pending
        // changes and the pending page order: the deletion is decided on the
        // document as it will be.
        let live = self.reached(&trailer, &pending);

        // The writing half.
        if trailer_removed {
            report.removed.push(RemovedEntry {
                holder: EntryHolder::Trailer,
                path: vec![PathStep::Key(b"Info".to_vec())],
                what: Removal::Info,
            });
            // 7.3.9: a null value is the entry being absent, and the merged
            // trailer drops it.
            self.trailer.insert(Name::INFO, Object::Null);
        }
        for (num, why) in &doomed {
            if live.contains(num) {
                continue;
            }
            let r = ObjRef::new(*num, 0);
            self.delete(r);
            report.deleted.push(DeletedObject {
                object: r,
                what: why.clone(),
            });
        }
        for (num, cleaned) in pending {
            if doomed.contains_key(&num) && !live.contains(&num) {
                continue;
            }
            let r = ObjRef::new(num, 0);
            if !self.replace_keeping_data(r, cleaned) {
                continue;
            }
            let removals = found.remove(&num).unwrap_or_default();
            report
                .removed
                .extend(removals.into_iter().map(|(path, what)| RemovedEntry {
                    holder: EntryHolder::Object(r),
                    path,
                    what,
                }));
        }
        report
    }

    /// Puts `object` at `r`, keeping the stream data when `r` is a stream —
    /// the editor's own, or the file's bytes as stored (decrypted, still
    /// encoded, with the `/Filter` the new dictionary keeps).
    ///
    /// False, putting nothing, for a stream whose bytes cannot be read.
    fn replace_keeping_data(&mut self, r: ObjRef, object: Object) -> bool {
        let data = match self.overlay.get(&r.num) {
            Some(Written::Stream(stream)) => Some(stream.data.clone()),
            Some(Written::Object(_)) => None,
            None => match self.doc.get(r) {
                Ok(original) if original.as_stream().is_some() => match self.doc.stream_raw(r) {
                    Ok(data) => Some(data),
                    Err(_) => return false,
                },
                _ => None,
            },
        };
        match (data, object) {
            (Some(data), Object::Dict(dict)) => self.put_stream(r, StreamData { dict, data }),
            (Some(_), _) => return false,
            (None, object) => self.put(r, object),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PdfString;

    fn action(editor: &DocumentEditor, s: &[u8]) -> Dict {
        let mut dict = Dict::new();
        dict.insert(editor.intern(b"S"), Object::Name(editor.intern(s)));
        dict
    }

    fn editor() -> DocumentEditor {
        let mut builder = crate::build::DocumentBuilder::new();
        builder.add_page(10.0, 10.0, |_| {});
        let doc = crate::doc::CosDocument::open(builder.finish()).expect("opens");
        DocumentEditor::new(std::sync::Arc::new(doc))
    }

    fn sweep(editor: &DocumentEditor, what: Sanitise) -> Sweep<'_> {
        let i = |b: &[u8]| editor.intern(b);
        Sweep {
            editor,
            what,
            keys: Keys {
                s: i(b"S"),
                p: i(b"P"),
                k: i(b"K"),
                next: i(b"Next"),
                pa: i(b"PA"),
                na: i(b"NA"),
                open_action: i(b"OpenAction"),
                aa: i(b"AA"),
                js: i(b"JS"),
                uri: i(b"URI"),
                metadata: i(b"Metadata"),
                ef: i(b"EF"),
                rf: i(b"RF"),
                info: Name::INFO,
                names: i(b"Names"),
                acro_form: i(b"AcroForm"),
                javascript: i(b"JavaScript"),
                embedded_files: i(b"EmbeddedFiles"),
                co: i(b"CO"),
                xfa: i(b"XFA"),
                action: i(b"Action"),
                embedded_file: i(b"EmbeddedFile"),
                annots: i(b"Annots"),
                subtype: i(b"Subtype"),
                link: i(b"Link"),
                dest: i(b"Dest"),
                a: i(b"A"),
            },
            catalog: None,
            names: None,
            acro_form: None,
            links: HashMap::new(),
            placed: HashMap::new(),
            removed: Vec::new(),
            roots: Vec::new(),
        }
    }

    /// Outside an action slot, a structure element role-mapped to a name
    /// that is also an action type is not an action: it has `/P` and `/K`,
    /// which no action needs.
    #[test]
    fn a_structure_element_named_like_an_action_is_not_one() {
        let editor = editor();
        let sweep = sweep(&editor, Sanitise::ALL);
        let mut element = action(&editor, b"URI");
        element.insert(editor.intern(b"P"), Object::Null);
        assert_eq!(sweep.verdict(&element, false), None);
        assert!(sweep.verdict(&action(&editor, b"URI"), false).is_some());
        let mut typed = action(&editor, b"Launch");
        typed.insert(Name::TYPE, Object::Name(editor.intern(b"StructElem")));
        assert_eq!(
            sweep.verdict(&typed, false),
            None,
            "/Type other than /Action"
        );
    }

    /// In an action slot, a viewer asks an action for its `/S` and nothing
    /// else, so neither does the sweep: `/K`, `/P` and an odd `/Type` do not
    /// hide one, and an `/S` written as a reference is resolved. An `/S` no
    /// action type spells is still no action there.
    #[test]
    fn in_an_action_slot_only_the_s_is_asked() {
        let editor = editor();
        let first = sweep(&editor, Sanitise::ALL);
        for (key, value) in [
            (&b"K"[..], Object::Int(0)),
            (b"P", Object::Null),
            (b"Type", Object::Name(editor.intern(b"Whatever"))),
        ] {
            let mut dressed = action(&editor, b"JavaScript");
            dressed.insert(editor.intern(key), value);
            assert_eq!(first.verdict(&dressed, false), None, "strict elsewhere");
            assert_eq!(
                first.verdict(&dressed, true),
                Some(Removal::JavaScript),
                "/{}",
                String::from_utf8_lossy(key)
            );
        }
        let mut editor = editor;
        let name = editor.allocate();
        editor.put(name, Object::Name(editor.intern(b"JavaScript")));
        let second = sweep(&editor, Sanitise::ALL);
        let mut indirect = Dict::new();
        indirect.insert(editor.intern(b"S"), Object::Ref(name));
        assert_eq!(second.verdict(&indirect, true), Some(Removal::JavaScript));
        assert_eq!(second.verdict(&action(&editor, b"D"), true), None);
    }

    /// A border style's `/S /D` (dashed) and a page label's `/S /r` are not
    /// actions: Table 198 does not name them.
    #[test]
    fn a_dictionary_whose_s_is_no_action_type_is_left() {
        let editor = editor();
        let sweep = sweep(&editor, Sanitise::ALL);
        for s in [&b"D"[..], b"r", b"Transparency", b"Alpha"] {
            assert_eq!(sweep.verdict(&action(&editor, s), false), None);
        }
    }

    /// A `javascript:` URI is script however it is capitalised or padded; any
    /// other URI is an outward action and nothing more.
    #[test]
    fn a_javascript_uri_is_javascript() {
        let editor = editor();
        let only_js = sweep(
            &editor,
            Sanitise {
                javascript: true,
                ..Sanitise::default()
            },
        );
        let uri = |text: &[u8]| {
            let mut dict = action(&editor, b"URI");
            dict.insert(
                editor.intern(b"URI"),
                Object::String(PdfString::literal(text.to_vec())),
            );
            dict
        };
        assert_eq!(
            only_js.verdict(&uri(b" \tJavaScript:app.alert(1)"), false),
            Some(Removal::JavaScript)
        );
        assert_eq!(only_js.verdict(&uri(b"https://example.org/"), false), None);
        let only_actions = sweep(
            &editor,
            Sanitise {
                actions: true,
                ..Sanitise::default()
            },
        );
        assert_eq!(
            only_actions.verdict(&uri(b"https://example.org/"), false),
            Some(Removal::Action(b"URI".to_vec()))
        );
        assert_eq!(
            only_actions.verdict(&action(&editor, b"GoTo"), false),
            None,
            "navigation stays"
        );
    }
}
