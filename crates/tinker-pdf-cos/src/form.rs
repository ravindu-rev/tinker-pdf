//! Interactive forms: reading the field tree (12.7).
//!
//! A form is a tree whose interior nodes group and whose leaves are fields,
//! and the two are not distinguished by any type entry — a node is a field
//! because it has a field type, inherited from an ancestor if it does not say
//! so itself. A node can also be its own widget, which is why a leaf's `/Kids`
//! may hold widgets rather than more fields, and why counting kids is not a
//! way to tell a leaf from a branch.
//!
//! Filling lives in [`crate::edit`]; this module only reads.

use std::collections::HashSet;

use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::text_string::decode_text_string;

/// What a field does (12.7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// Free text.
    Text,
    /// An on/off box.
    Checkbox,
    /// One of a set, where turning one on turns its siblings off.
    Radio,
    /// A button that only acts.
    PushButton,
    /// A drop-down list, optionally editable.
    ComboBox,
    /// A scrolling list.
    ListBox,
    /// A signature field.
    Signature,
    /// A `/FT` this build does not know.
    Unknown,
}

/// 12.7.4.2 table 228: the flags that change what a field is, rather than how
/// it behaves.
const RADIO: i64 = 1 << 15;
const PUSHBUTTON: i64 = 1 << 16;
const COMBO: i64 = 1 << 17;

/// 12.7.4.1 table 227: flags that apply to every field.
const READ_ONLY: i64 = 1;
const REQUIRED: i64 = 1 << 1;

/// A field's value.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    /// A text value, already decoded from whichever string encoding it used.
    Text(String),
    /// A name, which is how a button records which state it is in.
    State(String),
    /// Several values, from a list box with multiple selection.
    Many(Vec<String>),
    /// No value at all, which is different from an empty one: a field that has
    /// never been filled has no `/V`, and resetting restores that.
    None,
}

impl FieldValue {
    /// The value as text, for the common case of reading one out.
    #[must_use]
    pub fn as_text(&self) -> String {
        match self {
            FieldValue::Text(text) | FieldValue::State(text) => text.clone(),
            FieldValue::Many(values) => values.join(", "),
            FieldValue::None => String::new(),
        }
    }

    /// Whether a button in this state is on.
    #[must_use]
    pub fn is_on(&self) -> bool {
        match self {
            FieldValue::State(name) => name != "Off",
            _ => false,
        }
    }
}

/// One `/AA` script, as the file carries it (12.6.4.16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Script {
    /// The source text, decoded from whichever string encoding it used.
    Source(String),
    /// There is a script and it is this many decoded bytes, which is past
    /// [`limits::MAX_SCRIPT_LEN`] or past what the document had left of
    /// [`limits::MAX_SCRIPT_TOTAL`].
    ///
    /// Surfacing a truncated script would hand a reader source text that means
    /// something different from what the file says — and hand an interpreter
    /// something it would run. The length is what there is to report
    /// (ruling 10).
    Oversize(usize),
}

impl Script {
    /// The source, when there is all of it.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match self {
            Script::Source(text) => Some(text),
            Script::Oversize(_) => None,
        }
    }

    /// Whether the script was too large to surface.
    #[must_use]
    pub fn is_oversize(&self) -> bool {
        matches!(self, Script::Oversize(_))
    }
}

/// The scripts a field's `/AA` additional-actions dictionary carries
/// (12.6.3, table 198).
///
/// Data, not behaviour: these are the source texts the file holds, and reading
/// them runs nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldScripts {
    /// `/C` — recalculate the field's value when another field changes.
    pub calculate: Option<Script>,
    /// `/F` — format the value for display, without changing `/V`.
    pub format: Option<Script>,
    /// `/K` — a keystroke as the user types, and the paste and commit events.
    pub keystroke: Option<Script>,
    /// `/V` — validate a value the user committed.
    pub validate: Option<Script>,
}

impl FieldScripts {
    /// Whether the field carries no script at all, which is the common case.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.calculate.is_none()
            && self.format.is_none()
            && self.keystroke.is_none()
            && self.validate.is_none()
    }

    /// How many of the four are present.
    #[must_use]
    pub fn count(&self) -> usize {
        [
            &self.calculate,
            &self.format,
            &self.keystroke,
            &self.validate,
        ]
        .iter()
        .filter(|script| script.is_some())
        .count()
    }
}

/// One terminal field.
#[derive(Clone, Debug)]
pub struct Field {
    /// The field's own object.
    pub reference: ObjRef,
    /// The fully qualified name: ancestors' `/T` joined with dots (12.7.3.2).
    pub name: String,
    /// What kind of control it is.
    pub kind: FieldKind,
    /// Its current value.
    pub value: FieldValue,
    /// Its default value, which is what a reset restores.
    pub default: FieldValue,
    /// The raw `/Ff` flags.
    pub flags: i64,
    /// The widget annotations that draw it. A field that is its own widget
    /// lists itself here, so this is never empty for a visible field.
    pub widgets: Vec<ObjRef>,
    /// A choice field's options, as the text a user sees.
    pub options: Vec<String>,
    /// `/MaxLen`, when a text field caps its length.
    pub max_len: Option<i64>,
    /// The default appearance string, inherited if the field does not carry
    /// one — `/Helv 12 Tf 0 g` and the like.
    pub default_appearance: Option<Vec<u8>>,
    /// The `/AA` scripts, as source text. Not inherited: 12.7.3.1 makes
    /// `/FT`, `/Ff`, `/V` and `/DV` inheritable and stops there, so a parent's
    /// additional actions are the parent's.
    pub scripts: FieldScripts,
}

impl Field {
    /// Whether the field refuses to be filled.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.flags & READ_ONLY != 0
    }

    /// Whether the field must be filled before submitting.
    #[must_use]
    pub fn is_required(&self) -> bool {
        self.flags & REQUIRED != 0
    }
}

/// The document's `/AcroForm` dictionary.
#[must_use]
pub fn acro_form(doc: &CosDocument) -> Option<Dict> {
    let catalog = doc.catalog()?;
    doc.resolve_key(&catalog, doc.intern(b"AcroForm"))
        .as_dict()
        .cloned()
}

/// Whether the document asks viewers to rebuild every appearance (12.7.2).
///
/// It is set by producers that wrote values without appearances, and it is a
/// request rather than a guarantee: viewers that honour it are why such files
/// look filled, and viewers that do not are why they sometimes look blank.
#[must_use]
pub fn needs_appearances(doc: &CosDocument) -> bool {
    acro_form(doc)
        .and_then(|form| form.get_bool(doc.intern(b"NeedAppearances")))
        .unwrap_or(false)
}

/// The form's default resources, which is where a field's `/DA` finds its
/// font.
#[must_use]
pub fn default_resources(doc: &CosDocument) -> Option<Dict> {
    let form = acro_form(doc)?;
    doc.resolve_key(&form, doc.intern(b"DR")).as_dict().cloned()
}

/// Every terminal field, in the order the form declares them.
#[must_use]
pub fn fields(doc: &CosDocument) -> Vec<Field> {
    let Some(form) = acro_form(doc) else {
        return Vec::new();
    };
    let roots = doc.resolve_key(&form, doc.intern(b"Fields"));
    let Some(roots) = roots.as_array() else {
        return Vec::new();
    };

    // The inherited attributes a field may take from its ancestors, seeded
    // from the form itself: /DA and /Q are documented as form-level defaults.
    let inherited = Inherited {
        kind: None,
        flags: 0,
        value: None,
        default: None,
        appearance: form
            .get(doc.intern(b"DA"))
            .and_then(Object::as_string)
            .map(|s| s.bytes.clone()),
    };

    let mut out = Vec::new();
    let mut visited = HashSet::new();
    // Script source is document-controlled and per-field, so it needs a total
    // as well as a per-item cap.
    let mut budget = limits::MAX_SCRIPT_TOTAL;
    for entry in roots {
        if let Some(reference) = entry.as_objref() {
            walk(
                doc,
                reference,
                "",
                &inherited,
                0,
                &mut visited,
                &mut budget,
                &mut out,
            );
        }
    }
    out
}

/// Attributes a field takes from its ancestors when it does not say (12.7.3.1).
#[derive(Clone)]
struct Inherited {
    kind: Option<Name>,
    flags: i64,
    value: Option<Object>,
    default: Option<Object>,
    appearance: Option<Vec<u8>>,
}

#[allow(clippy::too_many_arguments)]
fn walk(
    doc: &CosDocument,
    reference: ObjRef,
    prefix: &str,
    inherited: &Inherited,
    depth: u32,
    visited: &mut HashSet<u32>,
    budget: &mut usize,
    out: &mut Vec<Field>,
) {
    if depth > limits::MAX_NEST_DEPTH || out.len() >= limits::MAX_PAGES {
        return;
    }
    if !visited.insert(reference.num) {
        // A /Parent pointing back up, or a kid listed twice: either way,
        // following it again never terminates.
        return;
    }

    let Ok(object) = doc.get(reference) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };

    let mut inherited = inherited.clone();
    if let Some(kind) = dict.get_name(doc.intern(b"FT")) {
        inherited.kind = Some(kind);
    }
    if let Some(flags) = dict.get_int(doc.intern(b"Ff")) {
        inherited.flags = flags;
    }
    if let Some(value) = dict.get(doc.intern(b"V")) {
        inherited.value = Some(value.clone());
    }
    if let Some(default) = dict.get(doc.intern(b"DV")) {
        inherited.default = Some(default.clone());
    }
    if let Some(appearance) = dict.get(doc.intern(b"DA")).and_then(Object::as_string) {
        inherited.appearance = Some(appearance.bytes.clone());
    }

    // 12.7.3.2: the qualified name joins the /T of every ancestor that has
    // one. A node without /T contributes nothing and does not add a dot.
    let own = dict
        .get(doc.intern(b"T"))
        .and_then(Object::as_string)
        .map(|s| decode_text_string(&s.bytes));
    let name = match (&own, prefix.is_empty()) {
        (Some(own), true) => own.clone(),
        (Some(own), false) => format!("{prefix}.{own}"),
        (None, _) => prefix.to_string(),
    };

    // Kids that are themselves fields, as opposed to the widgets that draw
    // this one. A widget has no /T and no /FT of its own; anything with
    // either is a field in its own right.
    let kids: Vec<ObjRef> = doc
        .resolve_key(dict, Name::KIDS)
        .as_array()
        .map(|kids| kids.iter().filter_map(Object::as_objref).collect())
        .unwrap_or_default();

    let mut field_kids = Vec::new();
    let mut widget_kids = Vec::new();
    for kid in kids {
        let is_field = doc
            .get(kid)
            .ok()
            .and_then(|o| o.as_dict().cloned())
            .is_some_and(|d| {
                d.get(doc.intern(b"T")).is_some() || d.get(doc.intern(b"FT")).is_some()
            });
        if is_field {
            field_kids.push(kid);
        } else {
            widget_kids.push(kid);
        }
    }

    if !field_kids.is_empty() {
        for kid in field_kids {
            walk(doc, kid, &name, &inherited, depth + 1, visited, budget, out);
        }
        visited.remove(&reference.num);
        return;
    }

    // A leaf. Without a field type there is nothing to fill, so it is a
    // grouping node that happened to have no field children.
    let Some(kind) = inherited.kind else {
        visited.remove(&reference.num);
        return;
    };

    let widgets = if widget_kids.is_empty() {
        // 12.7.3.3: a field with a single widget may be merged with it, in
        // which case the field's own object is the widget.
        vec![reference]
    } else {
        widget_kids
    };

    let kind = classify(doc, kind, inherited.flags);
    out.push(Field {
        reference,
        name,
        kind,
        value: read_value(doc, inherited.value.as_ref(), kind),
        default: read_value(doc, inherited.default.as_ref(), kind),
        flags: inherited.flags,
        widgets,
        options: options(doc, dict),
        max_len: dict.get_int(doc.intern(b"MaxLen")),
        default_appearance: inherited.appearance,
        scripts: field_scripts(doc, dict, budget),
    });
    visited.remove(&reference.num);
}

fn classify(doc: &CosDocument, kind: Name, flags: i64) -> FieldKind {
    let Some(bytes) = doc.name_bytes(kind) else {
        return FieldKind::Unknown;
    };
    match bytes.as_ref() {
        b"Tx" => FieldKind::Text,
        b"Sig" => FieldKind::Signature,
        b"Btn" => {
            // The flags decide, and their order matters: a push button is
            // never a radio, whatever else is set.
            if flags & PUSHBUTTON != 0 {
                FieldKind::PushButton
            } else if flags & RADIO != 0 {
                FieldKind::Radio
            } else {
                FieldKind::Checkbox
            }
        }
        b"Ch" => {
            if flags & COMBO != 0 {
                FieldKind::ComboBox
            } else {
                FieldKind::ListBox
            }
        }
        _ => FieldKind::Unknown,
    }
}

/// Decodes a `/V` or `/DV` entry into a typed value.
///
/// Public because an editor holding a changed `/V` in its overlay has to
/// decode it the same way the tree walk did, and a second decoder is a second
/// answer to give.
#[must_use]
pub fn field_value(doc: &CosDocument, value: Option<&Object>, kind: FieldKind) -> FieldValue {
    read_value(doc, value, kind)
}

fn read_value(doc: &CosDocument, value: Option<&Object>, kind: FieldKind) -> FieldValue {
    let Some(value) = value else {
        return FieldValue::None;
    };
    match doc.resolve(value).as_ref() {
        Object::String(text) => FieldValue::Text(decode_text_string(&text.bytes)),
        Object::Name(name) => FieldValue::State(
            doc.name_bytes(*name)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default(),
        ),
        Object::Array(items) => {
            let values: Vec<String> = items
                .iter()
                .filter_map(|item| match doc.resolve(item).as_ref() {
                    Object::String(text) => Some(decode_text_string(&text.bytes)),
                    Object::Name(name) => doc
                        .name_bytes(*name)
                        .map(|b| String::from_utf8_lossy(&b).into_owned()),
                    _ => None,
                })
                .collect();
            if values.is_empty() {
                FieldValue::None
            } else {
                FieldValue::Many(values)
            }
        }
        // A button with no /V is off, which is a state rather than an absence.
        Object::Null if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) => {
            FieldValue::State("Off".to_string())
        }
        _ => FieldValue::None,
    }
}

/// A choice field's `/Opt`, which is either strings or `[export, display]`
/// pairs (12.7.4.4).
fn options(doc: &CosDocument, dict: &Dict) -> Vec<String> {
    let value = doc.resolve_key(dict, doc.intern(b"Opt"));
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| match doc.resolve(item).as_ref() {
            Object::String(text) => decode_text_string(&text.bytes),
            Object::Array(pair) => pair
                .iter()
                // The display text is the second entry when there is one,
                // because that is what a user picks by.
                .filter_map(|v| v.as_string().map(|s| decode_text_string(&s.bytes)))
                .next_back()
                .unwrap_or_default(),
            _ => String::new(),
        })
        .collect()
}

/// The `/JS` of one action dictionary (12.6.4.16, table 217).
///
/// `/JS` is a text string or a stream holding one — a producer writes the
/// stream form as soon as the script is longer than a line, so reading only
/// the string form finds almost none of the scripts that matter.
fn read_js(doc: &CosDocument, action: &Dict, budget: &mut usize) -> Option<Script> {
    let js = action.get(doc.intern(b"JS"))?;
    let bytes: Vec<u8> = match js {
        Object::String(text) => text.bytes.clone(),
        Object::Ref(r) => {
            let object = doc.get(*r).ok()?;
            match object.as_ref() {
                Object::String(text) => text.bytes.clone(),
                // 7.3.8: a stream is always an indirect object, so this is the
                // only branch that can reach one.
                Object::Stream(_) => doc.stream_decoded(*r).ok()?,
                _ => return None,
            }
        }
        _ => return None,
    };

    let len = bytes.len();
    if len > limits::MAX_SCRIPT_LEN || len > *budget {
        return Some(Script::Oversize(len));
    }
    *budget -= len;
    Some(Script::Source(decode_text_string(&bytes)))
}

/// One entry of an additional-actions dictionary.
fn action_js(doc: &CosDocument, aa: &Dict, key: &[u8], budget: &mut usize) -> Option<Script> {
    let action = doc.resolve_key(aa, doc.intern(key));
    read_js(doc, action.as_dict()?, budget)
}

/// A field's `/AA` scripts (12.6.3, table 198).
fn field_scripts(doc: &CosDocument, dict: &Dict, budget: &mut usize) -> FieldScripts {
    let aa = doc.resolve_key(dict, doc.intern(b"AA"));
    let Some(aa) = aa.as_dict() else {
        return FieldScripts::default();
    };
    FieldScripts {
        calculate: action_js(doc, aa, b"C", budget),
        format: action_js(doc, aa, b"F", budget),
        keystroke: action_js(doc, aa, b"K", budget),
        validate: action_js(doc, aa, b"V", budget),
    }
}

/// A script that belongs to the document rather than to a field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentScript {
    /// Where it came from: the name-tree key for a `/Names /JavaScript`
    /// entry, or the trigger key (`WC`, `WS`, `DS`, `WP`, `DP`) for one of
    /// the catalog's additional actions.
    pub name: String,
    /// Its source.
    pub script: Script,
}

/// The order the form's calculations are meant to run in (12.7.2, table 218).
///
/// `/CO` is an array of references to the field dictionaries that have a
/// calculate action, and the order **is** the semantics: a total that depends
/// on a subtotal has to be computed after it, and the file is where that
/// dependency is recorded because nothing in the scripts states it.
///
/// Empty when the form declares none, which is also the answer for a form
/// whose fields calculate but whose producer left `/CO` out.
#[must_use]
pub fn calculation_order(doc: &CosDocument) -> Vec<ObjRef> {
    let Some(form) = acro_form(doc) else {
        return Vec::new();
    };
    doc.resolve_key(&form, doc.intern(b"CO"))
        .as_array()
        .map(|entries| entries.iter().filter_map(Object::as_objref).collect())
        .unwrap_or_default()
}

/// The document-level scripts, from `/Names /JavaScript` (7.7.4, table 31).
///
/// These run once when the document opens and are where a form keeps the
/// functions its field scripts call. Surfaced as source, and never run: a
/// document-level script is arbitrary program text with no field to write,
/// which is a different problem from a calculation.
#[must_use]
pub fn document_scripts(doc: &CosDocument) -> Vec<DocumentScript> {
    let Some(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let names = doc.resolve_key(&catalog, doc.intern(b"Names"));
    let Some(names) = names.as_dict() else {
        return Vec::new();
    };
    let Some(root) = names.get_ref(doc.intern(b"JavaScript")) else {
        return Vec::new();
    };

    let mut budget = limits::MAX_SCRIPT_TOTAL;
    let mut out = Vec::new();
    for (name, value) in crate::trees::name_tree(doc, root) {
        let resolved = doc.resolve(&value);
        let Some(action) = resolved.as_dict() else {
            continue;
        };
        if let Some(script) = read_js(doc, action, &mut budget) {
            out.push(DocumentScript {
                name: decode_text_string(&name),
                script,
            });
        }
    }
    out
}

/// 12.6.3 table 200: the catalog's additional actions, by trigger.
const CATALOG_TRIGGERS: [&[u8]; 5] = [b"WC", b"WS", b"DS", b"WP", b"DP"];

/// The document catalog's `/AA` scripts (12.6.3, table 200).
#[must_use]
pub fn catalog_scripts(doc: &CosDocument) -> Vec<DocumentScript> {
    let Some(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let aa = doc.resolve_key(&catalog, doc.intern(b"AA"));
    let Some(aa) = aa.as_dict() else {
        return Vec::new();
    };

    let mut budget = limits::MAX_SCRIPT_TOTAL;
    let mut out = Vec::new();
    for key in CATALOG_TRIGGERS {
        if let Some(script) = action_js(doc, aa, key, &mut budget) {
            out.push(DocumentScript {
                name: String::from_utf8_lossy(key).into_owned(),
                script,
            });
        }
    }
    out
}

/// How much script a document carries, for a caller that has to say so.
///
/// This is deliberately not a [`crate::warn::Warning`]: that set is closed and
/// describes repairs the lexer and object parser performed on bytes, collected
/// while parsing. A script is neither a repair nor something the parser sees,
/// and walking the field tree on every open would charge every document for
/// the few that have one. So the report is a value a caller asks for, and
/// [`ScriptSummary::describe`] is the sentence to put in front of a user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptSummary {
    /// Fields carrying at least one `/AA` script.
    pub fields_with_scripts: usize,
    /// Fields carrying a calculate action specifically.
    pub calculate_actions: usize,
    /// Entries in `/CO`.
    pub calculation_order: usize,
    /// Entries in `/Names /JavaScript`.
    pub document_scripts: usize,
    /// Entries in the catalog's `/AA`.
    pub catalog_actions: usize,
    /// Scripts present but past [`limits::MAX_SCRIPT_LEN`] or the document's
    /// [`limits::MAX_SCRIPT_TOTAL`], so surfaced as
    /// [`Script::Oversize`] rather than as source.
    pub oversize: usize,
}

impl ScriptSummary {
    /// Whether the document carries no script anywhere.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == ScriptSummary::default()
    }

    /// One sentence naming what is there, for a caller that warns once per
    /// document.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{} field(s) carry scripts ({} calculate), {} in the calculation order, \
             {} document-level and {} catalog action(s); {} too large to read",
            self.fields_with_scripts,
            self.calculate_actions,
            self.calculation_order,
            self.document_scripts,
            self.catalog_actions,
            self.oversize,
        )
    }
}

/// Counts every script the document carries (12.6.3, 12.7.2).
#[must_use]
pub fn script_summary(doc: &CosDocument) -> ScriptSummary {
    let fields = fields(doc);
    let document = document_scripts(doc);
    let catalog = catalog_scripts(doc);

    let mut summary = ScriptSummary {
        calculation_order: calculation_order(doc).len(),
        document_scripts: document.len(),
        catalog_actions: catalog.len(),
        ..ScriptSummary::default()
    };
    for field in &fields {
        if !field.scripts.is_empty() {
            summary.fields_with_scripts += 1;
        }
        if field.scripts.calculate.is_some() {
            summary.calculate_actions += 1;
        }
        for script in [
            &field.scripts.calculate,
            &field.scripts.format,
            &field.scripts.keystroke,
            &field.scripts.validate,
        ]
        .into_iter()
        .flatten()
        {
            if script.is_oversize() {
                summary.oversize += 1;
            }
        }
    }
    for script in document.iter().chain(catalog.iter()) {
        if script.script.is_oversize() {
            summary.oversize += 1;
        }
    }
    summary
}

/// The states a button's widget can be in, other than off.
///
/// A checkbox's on state is whatever its appearance dictionary calls it — it
/// is `/Yes` by convention and something else often enough that assuming
/// `/Yes` ticks the wrong box.
#[must_use]
pub fn on_state(doc: &CosDocument, widget: ObjRef) -> Option<Name> {
    let object = doc.get(widget).ok()?;
    let dict = object.as_dict()?;
    let ap = doc.resolve_key(dict, doc.intern(b"AP"));
    let normal = doc.resolve_key(ap.as_dict()?, doc.intern(b"N"));
    let states = normal.as_dict()?;

    let off = doc.intern(b"Off");
    states
        .entries()
        .iter()
        .map(|(name, _)| *name)
        .find(|name| *name != off)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A form with one text field, one checkbox whose on state is not /Yes,
    /// and a radio group of two — written by hand because the builder cannot
    /// make forms and the point is to read what other producers write.
    fn form_document() -> CosDocument {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 20 0 R 30 0 R]\n\
   /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]\n\
   /Annots [10 0 R 20 0 R 31 0 R 32 0 R] >>\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
10 0 obj\n<< /FT /Tx /T (name) /V (Ada) /Rect [10 150 190 170] /Subtype /Widget\n\
   /Type /Annot /MaxLen 40 >>\nendobj\n\
20 0 obj\n<< /FT /Btn /T (agree) /V /On /AS /On /Rect [10 120 30 140]\n\
   /Subtype /Widget /Type /Annot\n\
   /AP << /N << /On 21 0 R /Off 22 0 R >> >> >>\nendobj\n\
21 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>\nstream\n\nendstream\nendobj\n\
22 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>\nstream\n\nendstream\nendobj\n\
30 0 obj\n<< /FT /Btn /Ff 32768 /T (colour) /V /red /Kids [31 0 R 32 0 R] >>\nendobj\n\
31 0 obj\n<< /Parent 30 0 R /Subtype /Widget /Type /Annot /Rect [10 90 30 110] /AS /red\n\
   /AP << /N << /red 21 0 R /Off 22 0 R >> >> >>\nendobj\n\
32 0 obj\n<< /Parent 30 0 R /Subtype /Widget /Type /Annot /Rect [40 90 60 110] /AS /Off\n\
   /AP << /N << /blue 21 0 R /Off 22 0 R >> >> >>\nendobj\n\
trailer\n<< /Size 33 /Root 1 0 R >>\n%%EOF\n";
        CosDocument::open(bytes).expect("the form opens")
    }

    #[test]
    fn every_terminal_field_is_found() {
        let doc = form_document();
        let found = fields(&doc);
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["name", "agree", "colour"]);
    }

    #[test]
    fn field_kinds_come_from_the_type_and_the_flags() {
        let doc = form_document();
        let found = fields(&doc);
        assert_eq!(found[0].kind, FieldKind::Text);
        assert_eq!(found[1].kind, FieldKind::Checkbox);
        assert_eq!(
            found[2].kind,
            FieldKind::Radio,
            "the radio flag makes a button a radio, not the name"
        );
    }

    #[test]
    fn values_are_read_in_their_own_form() {
        let doc = form_document();
        let found = fields(&doc);
        assert_eq!(found[0].value, FieldValue::Text("Ada".to_string()));
        assert_eq!(found[1].value, FieldValue::State("On".to_string()));
        assert!(found[1].value.is_on());
        assert_eq!(found[0].max_len, Some(40));
    }

    /// A field merged with its widget is its own widget; a radio group's
    /// widgets are its kids.
    #[test]
    fn widgets_are_found_whether_merged_or_separate() {
        let doc = form_document();
        let found = fields(&doc);
        assert_eq!(found[0].widgets, vec![ObjRef::new(10, 0)], "merged");
        assert_eq!(
            found[2].widgets,
            vec![ObjRef::new(31, 0), ObjRef::new(32, 0)],
            "a radio group's kids are widgets, not fields"
        );
    }

    /// The convention is `/Yes`, and the files that use something else are
    /// exactly the ones a naive filler ticks wrongly.
    #[test]
    fn a_buttons_on_state_is_read_from_its_appearances() {
        let doc = form_document();
        let state = on_state(&doc, ObjRef::new(20, 0)).expect("an on state");
        assert_eq!(doc.name_bytes(state).as_deref(), Some(b"On".as_slice()));

        let blue = on_state(&doc, ObjRef::new(32, 0)).expect("an on state");
        assert_eq!(doc.name_bytes(blue).as_deref(), Some(b"blue".as_slice()));
    }

    #[test]
    fn the_default_appearance_is_inherited_from_the_form() {
        let doc = form_document();
        let found = fields(&doc);
        assert_eq!(
            found[0].default_appearance.as_deref(),
            Some(b"/Helv 0 Tf 0 g".as_slice()),
            "the form's /DA reaches a field that has none"
        );
    }

    #[test]
    fn a_document_with_no_form_has_no_fields() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes).expect("it opens");
        assert!(fields(&doc).is_empty());
        assert!(!needs_appearances(&doc));
    }

    /// A `/Parent` chain that loops must not hang the walk.
    #[test]
    fn a_cyclic_field_tree_terminates() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
10 0 obj\n<< /FT /Tx /T (a) /Kids [11 0 R] >>\nendobj\n\
11 0 obj\n<< /T (b) /Kids [10 0 R] >>\nendobj\n\
trailer\n<< /Size 12 /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes).expect("it opens");
        let found = fields(&doc);
        assert!(found.len() <= 2, "the cycle is cut, not followed");
    }

    /// A form whose total is calculated: two inputs, one total with an `/AA`
    /// `/C` script and a `/F` format script, `/CO` naming the order, and a
    /// document-level script in `/Names /JavaScript`.
    fn calculated_document() -> CosDocument {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Names << /JavaScript 40 0 R >>\n\
   /AA << /WC << /S /JavaScript /JS (app.alert\\('bye'\\);) >> >>\n\
   /AcroForm << /Fields [10 0 R 11 0 R 12 0 R] /CO [12 0 R 11 0 R] >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R 11 0 R 12 0 R] >>\nendobj\n\
10 0 obj\n<< /FT /Tx /T (net) /V (100) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot >>\nendobj\n\
11 0 obj\n<< /FT /Tx /T (vat) /V (0) /Rect [10 120 190 140] /Subtype /Widget /Type /Annot\n\
   /AA << /C << /S /JavaScript /JS 41 0 R >> >> >>\nendobj\n\
12 0 obj\n<< /FT /Tx /T (total) /V (0) /Rect [10 90 190 110] /Subtype /Widget /Type /Annot\n\
   /AA << /C << /S /JavaScript /JS (event.value = 1;) >>\n\
          /F << /S /JavaScript /JS (AFNumber_Format\\(2, 0, 0, 0, \"\", true\\);) >> >> >>\nendobj\n\
40 0 obj\n<< /Names [(helper) 42 0 R] >>\nendobj\n\
41 0 obj\n<< /Length 34 >>\nstream\n\
event.value = this.getField('x');\n\
endstream\nendobj\n\
42 0 obj\n<< /S /JavaScript /JS (function sum\\(a, b\\) { return a + b; }) >>\nendobj\n\
trailer\n<< /Size 43 /Root 1 0 R >>\n%%EOF\n";
        CosDocument::open(bytes).expect("the calculated form opens")
    }

    #[test]
    fn a_fields_additional_actions_are_read_as_source() {
        let doc = calculated_document();
        let found = fields(&doc);
        let total = found.iter().find(|f| f.name == "total").expect("total");
        assert_eq!(
            total.scripts.calculate.as_ref().and_then(Script::source),
            Some("event.value = 1;")
        );
        assert_eq!(
            total.scripts.format.as_ref().and_then(Script::source),
            Some("AFNumber_Format(2, 0, 0, 0, \"\", true);")
        );
        assert!(total.scripts.keystroke.is_none());
        assert_eq!(total.scripts.count(), 2);
    }

    /// 12.6.4.16 allows `/JS` to be a stream, and a producer writes that form
    /// as soon as the script is longer than a line — so reading only the
    /// string form finds almost none of the scripts that matter.
    #[test]
    fn a_script_held_in_a_stream_is_read_too() {
        let doc = calculated_document();
        let found = fields(&doc);
        let vat = found.iter().find(|f| f.name == "vat").expect("vat");
        assert_eq!(
            vat.scripts.calculate.as_ref().and_then(Script::source),
            Some("event.value = this.getField('x');\n")
        );
    }

    #[test]
    fn a_field_with_no_actions_carries_no_scripts() {
        let doc = calculated_document();
        let found = fields(&doc);
        let net = found.iter().find(|f| f.name == "net").expect("net");
        assert!(net.scripts.is_empty());
        assert_eq!(net.scripts.count(), 0);
    }

    /// The order is the semantics: the file says the total comes before the
    /// VAT here, and reading it in array order is the only way to honour that.
    #[test]
    fn the_calculation_order_is_read_in_the_order_declared() {
        let doc = calculated_document();
        assert_eq!(
            calculation_order(&doc),
            vec![ObjRef::new(12, 0), ObjRef::new(11, 0)]
        );
    }

    #[test]
    fn document_and_catalog_scripts_are_read() {
        let doc = calculated_document();
        let document = document_scripts(&doc);
        assert_eq!(document.len(), 1);
        assert_eq!(document[0].name, "helper");
        assert_eq!(
            document[0].script.source(),
            Some("function sum(a, b) { return a + b; }")
        );

        let catalog = catalog_scripts(&doc);
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].name, "WC");
        assert_eq!(catalog[0].script.source(), Some("app.alert('bye');"));
    }

    #[test]
    fn the_summary_counts_what_is_there() {
        let doc = calculated_document();
        let summary = script_summary(&doc);
        assert!(!summary.is_empty());
        assert_eq!(summary.fields_with_scripts, 2);
        assert_eq!(summary.calculate_actions, 2);
        assert_eq!(summary.calculation_order, 2);
        assert_eq!(summary.document_scripts, 1);
        assert_eq!(summary.catalog_actions, 1);
        assert_eq!(summary.oversize, 0);
        assert!(summary.describe().contains("2 field(s) carry scripts"));
    }

    #[test]
    fn a_document_without_scripts_summarises_to_nothing() {
        let doc = form_document();
        let summary = script_summary(&doc);
        assert!(summary.is_empty(), "{summary:?}");
        assert!(calculation_order(&doc).is_empty());
        assert!(document_scripts(&doc).is_empty());
        assert!(catalog_scripts(&doc).is_empty());
    }

    /// A script past the cap is reported as present and too big, never
    /// truncated: truncated source means something different from what the
    /// file says, and the difference is silent.
    #[test]
    fn an_oversized_script_is_named_rather_than_truncated() {
        let huge = "x".repeat(limits::MAX_SCRIPT_LEN + 1);
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
10 0 obj\n<< /FT /Tx /T (a) /AA << /C << /JS ({huge}) >> >> >>\nendobj\n\
trailer\n<< /Size 11 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = CosDocument::open(bytes.into_bytes()).expect("it opens");
        let found = fields(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].scripts.calculate,
            Some(Script::Oversize(limits::MAX_SCRIPT_LEN + 1))
        );
        assert!(found[0].scripts.calculate.as_ref().unwrap().is_oversize());
        assert_eq!(script_summary(&doc).oversize, 1);
    }

    #[test]
    fn qualified_names_join_the_ancestors_that_have_one() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
10 0 obj\n<< /T (address) /Kids [11 0 R] >>\nendobj\n\
11 0 obj\n<< /T (city) /FT /Tx /V (Bath) >>\nendobj\n\
trailer\n<< /Size 12 /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes).expect("it opens");
        let found = fields(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "address.city");
        assert_eq!(found[0].value, FieldValue::Text("Bath".to_string()));
    }
}
