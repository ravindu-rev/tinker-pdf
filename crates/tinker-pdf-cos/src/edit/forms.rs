//! Filling, resetting and recalculating interactive form fields (12.7).

use super::{without, DocumentEditor};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::{fill, form};

/// Why a field could not be filled at all (12.7.4.3).
///
/// Every variant means **nothing was written**: the editor is exactly as it
/// was. The value a caller cannot express in a `bool` is the fourth outcome --
/// written, and not wholly drawable -- which is [`SkippedWidget`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FillError {
    /// No field carries that fully qualified name (12.7.3.2).
    NoSuchField,
    /// The field will not take this value: it is read-only (Table 227), the
    /// value is longer than `/MaxLen`, or a non-editable list does not offer
    /// it. Refusing beats truncating, which hides a data error inside a file
    /// that then looks correctly filled.
    ValueRefused,
    /// The field's own object is not a dictionary, so there is nowhere to put
    /// `/V`.
    FieldUnreadable,
}

impl core::fmt::Display for FillError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            FillError::NoSuchField => "no such field",
            FillError::ValueRefused => "the field does not accept that value",
            FillError::FieldUnreadable => "the field object is not a dictionary",
        })
    }
}

/// Who is writing a field value.
///
/// The distinction exists for exactly one rule — ReadOnly (12.7.4.1 table
/// 227) binds the user and not the document's own calculate action — and it is
/// an enum rather than a `bool` so that the call sites read as what they are
/// rather than as `true`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Writer {
    /// A caller filling the form, which is what ReadOnly refuses.
    User,
    /// A calculate action the document itself carries.
    Calculation,
}

/// Which field refused, and why (ruling 10: a warning names its object).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FillRejection {
    /// The fully qualified name the caller asked for (12.7.3.2).
    pub field: String,
    /// Why it refused.
    pub reason: FillError,
}

impl core::fmt::Display for FillRejection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

/// What is wrong with a widget that could not be given an appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetDefect {
    /// 12.5.2 Table 164: `/Rect` is required for every annotation, and this
    /// one has none, or one that is not a usable rectangle. There is no box to
    /// lay the value out in and nowhere on the page to draw it.
    RectMissing,
}

impl core::fmt::Display for WidgetDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            WidgetDefect::RectMissing => "no usable /Rect (12.5.2)",
        })
    }
}

/// A widget whose appearance was **not** regenerated, and which object it is
/// (ruling 10).
///
/// A field that appears on two pages has two widgets. Ruling 2 says degrade
/// rather than fail, so the value is still written and the widgets that can be
/// drawn are drawn -- refusing the whole field because a damaged file lost one
/// `/Rect` would leave the form unfillable. What ruling 2 does not licence is
/// silence: without this, such a field reported success while one widget kept
/// whatever it was showing before, which is a document that looks filled and
/// is wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkippedWidget {
    /// The widget annotation left as it was.
    pub widget: ObjRef,
    /// What is wrong with it.
    pub reason: WidgetDefect,
}

impl core::fmt::Display for SkippedWidget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.widget, self.reason)
    }
}

impl DocumentEditor {
    /// The document's form fields (12.7), **as this editor has them**.
    ///
    /// The tree walk reads the document underneath the overlay, so a field
    /// this editor has already written would otherwise come back with the
    /// value it was saved with. Nothing depended on that before — `accepts`
    /// looks at the kind and the flags, not the value — but a calculation
    /// reads the values it computes from, and one that read them from under
    /// its own writes would compute a total from inputs the file no longer
    /// has. The value is taken from the overlay whenever the field's object is
    /// in it, present *or* absent, because 12.7.5.3's reset removes `/V`
    /// rather than blanking it and "absent" is an answer.
    #[must_use]
    pub fn fields(&self) -> Vec<form::Field> {
        self.fields_within(&mut form::ScriptBudget::new())
    }

    /// The same field list, spending a script budget the caller owns.
    ///
    /// The recalculation pass uses this so that the field tree's `/AA` and
    /// `/Names /JavaScript` share one [`form::ScriptBudget`] rather than
    /// starting from the total apiece.
    #[must_use]
    pub fn fields_within(&self, budget: &mut form::ScriptBudget) -> Vec<form::Field> {
        let mut fields = form::fields_within(&self.doc, budget);
        if self.overlay.is_empty() && self.deleted.is_empty() {
            return fields;
        }
        let v = self.intern(b"V");
        for field in &mut fields {
            if !self.overlay.contains_key(&field.reference.num) {
                continue;
            }
            let Some(Object::Dict(dict)) = self.get(field.reference) else {
                continue;
            };
            field.value = form::field_value(&self.doc, dict.get(v), field.kind);
        }
        fields
    }

    /// Fills a text or choice field, rebuilding its appearance.
    ///
    /// Returns false when there is no such field, when it is read-only, or
    /// when the value is one the field does not accept — a value over
    /// `/MaxLen`, or one a non-editable list does not offer. Refusing is the
    /// point: writing a truncated value would hide a data error inside a file
    /// that then looks correctly filled.
    ///
    /// It also returns false — **having changed nothing** — when the value
    /// could be written but one of the field's widgets could not be drawn.
    /// A `bool` cannot say "partly", and the answer it used to give was
    /// `true`: a field on two pages came out with one appearance regenerated
    /// and one stale, reported as success. Use [`DocumentEditor::fill_field`]
    /// where the difference matters; it applies what can be applied and names
    /// what could not.
    pub fn set_field_value(&mut self, name: &str, value: &str) -> bool {
        self.transaction(|tx| match tx.fill_field(name, value) {
            Ok(skipped) if skipped.is_empty() => Ok(()),
            _ => Err(()),
        })
        .is_ok()
    }

    /// Fills a text or choice field, saying exactly what happened.
    ///
    /// - `Err` — nothing was written at all, and [`FillError`] says why.
    /// - `Ok(skipped)` with `skipped` empty — `/V` was written and every
    ///   widget's appearance was regenerated.
    /// - `Ok(skipped)` non-empty — `/V` was written, and those widgets were
    ///   left showing whatever they were showing before, because 12.5.2's
    ///   required `/Rect` is missing from them and there is nowhere to draw.
    ///   Ruling 2 degrades rather than failing; ruling 10 requires that the
    ///   degradation name the object it happened to, which is what the return
    ///   value is for. A caller that wants all-or-nothing checks
    ///   `skipped.is_empty()`, or uses [`DocumentEditor::set_field_value`],
    ///   which does exactly that.
    pub fn fill_field(&mut self, name: &str, value: &str) -> Result<Vec<SkippedWidget>, FillError> {
        self.write_field(name, value, Writer::User)
    }

    /// The one implementation behind every field write.
    ///
    /// The only thing [`Writer`] changes is whether ReadOnly applies; every
    /// other rule about the value is [`fill::accepts_value`], shared, so the
    /// two doors cannot drift apart.
    fn write_field(
        &mut self,
        name: &str,
        value: &str,
        writer: Writer,
    ) -> Result<Vec<SkippedWidget>, FillError> {
        let Some(field) = self.fields().into_iter().find(|f| f.name == name) else {
            return Err(FillError::NoSuchField);
        };
        let allowed = match writer {
            Writer::User => fill::accepts(&field, value),
            Writer::Calculation => fill::accepts_value(&field, value),
        };
        if !allowed {
            return Err(FillError::ValueRefused);
        }
        let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
            return Err(FillError::FieldUnreadable);
        };

        // Every reason to refuse has been checked, so from here nothing can
        // fail and leave the field half written.
        dict.insert(
            self.intern(b"V"),
            fill::value_object_in(value, self.text_version()),
        );
        self.put(field.reference, Object::Dict(dict));
        let skipped = self.regenerate_text(&field, value);
        self.clear_need_appearances();
        Ok(skipped)
    }

    /// Fills several fields as one edit: all of them, or none of them.
    ///
    /// The first field that refuses rolls back every field before it and
    /// returns which one it was (12.7.3.2's fully qualified name) and why.
    /// That is the shape a calculated form needs: a script that sets three
    /// fields and fails on the fourth must not leave a document whose totals
    /// disagree with its inputs, which is the worst outcome a form has.
    ///
    /// `Ok` carries every widget that could not be drawn, across all the
    /// fields, in the order they were given — see
    /// [`DocumentEditor::fill_field`] for why that is a report rather than a
    /// failure.
    pub fn set_field_values(
        &mut self,
        values: &[(&str, &str)],
    ) -> Result<Vec<SkippedWidget>, FillRejection> {
        self.write_fields(values, Writer::User)
    }

    /// The same all-or-nothing apply, for values a calculation produced.
    ///
    /// One difference from [`DocumentEditor::set_field_values`], and it is the
    /// whole reason this exists: a ReadOnly field is written. 12.7.4.1 table
    /// 227 makes ReadOnly a rule about **the user**, and a calculated total is
    /// flagged read-only precisely so that nothing but the document's own
    /// calculate action writes it — refusing here would make every properly
    /// authored calculated form uncomputable. Every other rule about the value
    /// is unchanged, because both paths run [`fill::accepts_value`].
    ///
    /// Not a private helper: a host that reads the scripts as data (which is
    /// what the field model surfaces) and computes them itself needs exactly
    /// this door, and would otherwise have to clear the flag and put it back.
    pub fn set_calculated_values(
        &mut self,
        values: &[(&str, &str)],
    ) -> Result<Vec<SkippedWidget>, FillRejection> {
        self.write_fields(values, Writer::Calculation)
    }

    fn write_fields(
        &mut self,
        values: &[(&str, &str)],
        writer: Writer,
    ) -> Result<Vec<SkippedWidget>, FillRejection> {
        self.transaction(|tx| {
            let mut skipped = Vec::new();
            for (name, value) in values {
                match tx.write_field(name, value, writer) {
                    Ok(widgets) => skipped.extend(widgets),
                    Err(reason) => {
                        return Err(FillRejection {
                            field: (*name).to_string(),
                            reason,
                        })
                    }
                }
            }
            Ok(skipped)
        })
    }

    /// Runs the form's calculate actions and applies the result (12.6.3).
    ///
    /// See [`crate::calc::recalculate`], which this delegates to whole: the
    /// pass is long enough to deserve its own module, and putting it there
    /// keeps the interpreter's only entry point next to the ordering rule it
    /// depends on.
    ///
    /// # Errors
    ///
    /// Any script that cannot be run refuses the **whole** pass, and nothing
    /// is written.
    pub fn recalculate(&mut self) -> Result<crate::calc::Recalculation, crate::calc::CalcError> {
        crate::calc::recalculate(self)
    }

    /// The same pass, under a [`crate::script::ScriptPolicy`] the host chose.
    ///
    /// # Errors
    ///
    /// `CalcError::Refused` when the policy denies a trigger class this form
    /// carries; otherwise exactly what [`DocumentEditor::recalculate`]
    /// returns. Nothing is written in either case.
    pub fn recalculate_under(
        &mut self,
        policy: crate::script::ScriptPolicy,
    ) -> Result<crate::calc::Recalculation, crate::calc::CalcError> {
        crate::calc::recalculate_under(self, policy)
    }

    /// The text a field's format action would display (12.7.3.3).
    ///
    /// `Ok(None)` for a field with no format action, which is most fields.
    /// The answer is a [`crate::calc::DisplayString`] and not a `String`,
    /// because a display string that reached `/V` would give a form whose
    /// export reads "GBP 1,234.00" where a consumer expects 1234 — see the
    /// type, and the compile-refusal proof beside it.
    ///
    /// # Errors
    ///
    /// See [`crate::calc::formatted_value_under`].
    pub fn formatted_value(
        &self,
        name: &str,
        policy: crate::script::ScriptPolicy,
    ) -> Result<Option<crate::calc::DisplayString>, crate::calc::CalcError> {
        crate::calc::formatted_value_under(self, name, policy)
    }

    /// Offers a keystroke to a field's `/AA /K` action (12.6.4.16 table 196).
    ///
    /// It takes an event because it has to: what is being typed, where, and
    /// whether this is the commit are facts a host has and a reader does not,
    /// which is exactly why keystroke actions could never run implicitly.
    /// Nothing in a recalculation reaches this.
    ///
    /// Under the default [`crate::script::ScriptPolicy`] a field that carries
    /// a keystroke action answers `CalcError::Refused`; one that carries none
    /// accepts the keystroke as offered.
    ///
    /// # Errors
    ///
    /// See [`crate::calc::keystroke`]. A script that refuses the keystroke is
    /// not an error — that is `EventVerdict::Refused`.
    pub fn keystroke(
        &self,
        name: &str,
        event: &crate::calc::Keystroke,
        policy: crate::script::ScriptPolicy,
    ) -> Result<crate::calc::EventVerdict, crate::calc::CalcError> {
        crate::calc::keystroke(self, name, event, policy)
    }

    /// Offers a committed value to a field's `/AA /V` action (12.6.4.16
    /// table 196).
    ///
    /// Nothing is written either way: this answers whether the form would
    /// take the value, and applying it is still
    /// [`DocumentEditor::fill_field`]'s job.
    ///
    /// # Errors
    ///
    /// See [`crate::calc::validate`].
    pub fn validate(
        &self,
        name: &str,
        value: &str,
        policy: crate::script::ScriptPolicy,
    ) -> Result<crate::calc::EventVerdict, crate::calc::CalcError> {
        crate::calc::validate(self, name, value, policy)
    }

    /// Turns a checkbox on or off.
    ///
    /// The on state is whatever the widget's appearance dictionary calls it,
    /// which is `/Yes` by convention and something else often enough that
    /// assuming `/Yes` ticks a box the file cannot draw.
    ///
    /// All of the field's widgets or none: a box that shows ticked on one page
    /// and clear on another is worse than one that refuses to be ticked.
    pub fn set_checkbox(&mut self, name: &str, on: bool) -> bool {
        self.transaction(|tx| {
            let Some(field) = tx
                .fields()
                .into_iter()
                .find(|f| f.name == name && f.kind == form::FieldKind::Checkbox)
            else {
                return Err(());
            };
            if field.is_read_only() {
                return Err(());
            }

            let off = tx.intern(b"Off");
            let state = if on {
                let Some(widget) = field.widgets.first() else {
                    return Err(());
                };
                match form::on_state(&tx.doc, *widget) {
                    Some(state) => state,
                    // Without an appearance for the on state there is nothing
                    // to draw, and setting /V alone would leave a box that
                    // reads as ticked and displays as empty.
                    None => return Err(()),
                }
            } else {
                off
            };

            let Some(Object::Dict(mut dict)) = tx.get(field.reference) else {
                return Err(());
            };
            dict.insert(tx.intern(b"V"), Object::Name(state));
            tx.put(field.reference, Object::Dict(dict));

            for widget in &field.widgets {
                if !tx.set_appearance_state(*widget, state) {
                    return Err(());
                }
            }
            tx.clear_need_appearances();
            Ok(())
        })
        .is_ok()
    }

    /// Selects one button of a radio group.
    ///
    /// 12.7.4.2: the group holds one value, and every widget's `/AS` follows
    /// it — the one whose appearance offers that state shows on, the rest show
    /// off. Setting only the chosen widget leaves the previous one still
    /// drawn, which is how two options end up looking selected at once.
    ///
    /// So this is all of the group's widgets or none of them.
    pub fn select_radio(&mut self, name: &str, option: &str) -> bool {
        self.transaction(|tx| {
            let Some(field) = tx
                .fields()
                .into_iter()
                .find(|f| f.name == name && f.kind == form::FieldKind::Radio)
            else {
                return Err(());
            };
            if field.is_read_only() {
                return Err(());
            }

            let wanted = tx.intern(option.as_bytes());
            let off = tx.intern(b"Off");
            let offers = |editor: &DocumentEditor, widget: ObjRef| -> bool {
                editor
                    .get(widget)
                    .and_then(|o| o.as_dict().cloned())
                    .and_then(|d| d.get_dict(editor.intern(b"AP")).cloned())
                    .and_then(|ap| ap.get_dict(editor.intern(b"N")).cloned())
                    .is_some_and(|states| states.get(wanted).is_some())
            };

            if !field.widgets.iter().any(|w| offers(tx, *w)) {
                return Err(());
            }

            let Some(Object::Dict(mut dict)) = tx.get(field.reference) else {
                return Err(());
            };
            dict.insert(tx.intern(b"V"), Object::Name(wanted));
            tx.put(field.reference, Object::Dict(dict));

            for widget in &field.widgets {
                let state = if offers(tx, *widget) { wanted } else { off };
                if !tx.set_appearance_state(*widget, state) {
                    return Err(());
                }
            }
            tx.clear_need_appearances();
            Ok(())
        })
        .is_ok()
    }

    /// Restores every field to its default value (12.7.5.3).
    ///
    /// A field with no `/DV` loses its `/V` entirely rather than gaining an
    /// empty one, because "never filled" and "filled with nothing" are
    /// different states and a submitted form distinguishes them.
    ///
    /// Returns the widgets whose appearance could not be rebuilt, for the same
    /// reason [`DocumentEditor::fill_field`] does: a reset that silently left
    /// one widget showing the old value is the same defect as a fill that did.
    /// A reset is not all-or-nothing — it restores every field it can, which
    /// is what 12.7.5.3's action does — so wrap it in
    /// [`DocumentEditor::transaction`] if a partial one is unacceptable.
    pub fn reset_form(&mut self) -> Vec<SkippedWidget> {
        let mut skipped = Vec::new();
        for field in self.fields() {
            let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
                continue;
            };
            let v = self.intern(b"V");
            let dv = self.intern(b"DV");

            match dict.get(dv).cloned() {
                Some(default) => {
                    dict.insert(v, default);
                }
                None => {
                    dict = without(&dict, v);
                }
            }
            self.put(field.reference, Object::Dict(dict));

            match field.kind {
                form::FieldKind::Checkbox | form::FieldKind::Radio => {
                    let off = self.intern(b"Off");
                    let state = self
                        .get(field.reference)
                        .and_then(|o| o.as_dict().and_then(|d| d.get_name(self.intern(b"V"))))
                        .unwrap_or(off);
                    for widget in &field.widgets {
                        self.set_appearance_state(*widget, state);
                    }
                }
                _ => {
                    let value = self
                        .get(field.reference)
                        .map(|o| {
                            o.as_dict()
                                .and_then(|d| d.get(self.intern(b"V")).cloned())
                                .map_or(String::new(), |v| {
                                    v.as_string()
                                        .map(|s| crate::text_string::decode_text_string(&s.bytes))
                                        .unwrap_or_default()
                                })
                        })
                        .unwrap_or_default();
                    skipped.extend(self.regenerate_text(&field, &value));
                }
            }
        }
        self.clear_need_appearances();
        skipped
    }

    /// Rewrites a text field's appearance for a value already stored,
    /// returning the widgets it could not draw.
    fn regenerate_text(&mut self, field: &form::Field, value: &str) -> Vec<SkippedWidget> {
        let resources = form::default_resources(&self.doc);
        let quadding = fill::quadding(&self.doc, field);
        let multiline = field.flags & fill::MULTILINE != 0;
        let comb = (field.flags & fill::COMB != 0)
            .then_some(field.max_len)
            .flatten();

        let mut skipped = Vec::new();
        for widget in &field.widgets {
            let Some(rect) = fill::widget_rect(&self.doc, *widget) else {
                // 12.5.2 Table 164 makes /Rect required, so this is a damaged
                // file rather than a widget with nothing to draw. It used to
                // be a bare `continue` and the field still reported success.
                skipped.push(SkippedWidget {
                    widget: *widget,
                    reason: WidgetDefect::RectMissing,
                });
                continue;
            };
            let da = fill::appearance_string(&self.doc, field, *widget);
            let stream = fill::text_appearance(
                &self.doc,
                rect,
                value,
                &fill::TextLayout {
                    da: &da,
                    quadding,
                    multiline,
                    comb,
                    resources: resources.as_ref(),
                    // Ruling 10: a character the `/DA` font cannot draw is
                    // reported against the field it was written into.
                    field: Some(field.reference),
                },
            );
            let form_ref = self.allocate();
            self.put_stream(form_ref, stream);
            self.set_normal_appearance(*widget, Object::Ref(form_ref));
        }
        skipped
    }

    /// Points a widget's `/AP` `/N` at something.
    fn set_normal_appearance(&mut self, widget: ObjRef, normal: Object) {
        let Some(Object::Dict(mut dict)) = self.get(widget) else {
            return;
        };
        let ap = self.intern(b"AP");
        let n = self.intern(b"N");
        let mut states = dict.get_dict(ap).cloned().unwrap_or_else(Dict::new);
        states.insert(n, normal);
        dict.insert(ap, Object::Dict(states));
        // A widget showing a single appearance has no state to select, and a
        // leftover /AS naming one would suppress it entirely.
        let dict = without(&dict, self.intern(b"AS"));
        self.put(widget, Object::Dict(dict));
    }

    /// Selects which of a widget's appearance states is shown.
    ///
    /// False when the widget is not a dictionary, which is a file that cannot
    /// be filled correctly rather than one with nothing to show: 12.7.4.2 has
    /// every widget of a button follow the group's value, and one left behind
    /// is how two radio options end up looking selected at once.
    fn set_appearance_state(&mut self, widget: ObjRef, state: Name) -> bool {
        let Some(Object::Dict(mut dict)) = self.get(widget) else {
            return false;
        };
        dict.insert(self.intern(b"AS"), Object::Name(state));
        self.put(widget, Object::Dict(dict));
        true
    }

    /// Drops `/NeedAppearances` now that the appearances are right.
    ///
    /// Leaving it set asks every viewer to throw away what was just written
    /// and rebuild it from its own idea of the field, which is how a correctly
    /// filled form comes out looking different in each one.
    fn clear_need_appearances(&mut self) {
        let Some(catalog) = self.doc.catalog() else {
            return;
        };
        let Some(form_ref) = catalog.get_ref(self.intern(b"AcroForm")) else {
            // A direct /AcroForm cannot be replaced without rewriting the
            // catalog, which is done here rather than skipped.
            let key = self.intern(b"AcroForm");
            let Some(form) = catalog.get_dict(key).cloned() else {
                return;
            };
            let cleaned = without(&form, self.intern(b"NeedAppearances"));
            let Some(root) = self.doc.trailer().get_ref(Name::ROOT) else {
                return;
            };
            let mut updated = (*catalog).clone();
            updated.insert(key, Object::Dict(cleaned));
            self.put(root, Object::Dict(updated));
            return;
        };

        let Some(Object::Dict(form)) = self.get(form_ref) else {
            return;
        };
        let cleaned = without(&form, self.intern(b"NeedAppearances"));
        self.put(form_ref, Object::Dict(cleaned));
    }

    /// The interactive form, read **through this editor's own changes**, and
    /// where writing it back has to go.
    ///
    /// Reading it from `self.doc` instead is a trap worth naming: two edits to
    /// the form in one save then each start from the file's original catalog,
    /// so the second silently discards the first. That is exactly what
    /// happened when adding a field and setting `/SigFlags` were two passes —
    /// the file came out structurally valid, strict-clean, and carrying a
    /// signature dictionary no field pointed at.
    pub(super) fn acroform(&mut self) -> Option<(FormHome, Dict)> {
        let root = self.doc.trailer().get_ref(Name::ROOT)?;
        let Some(Object::Dict(catalog)) = self.get(root) else {
            return None;
        };
        let key = self.intern(b"AcroForm");
        match catalog.get(key) {
            Some(Object::Ref(form_ref)) => {
                let form = match self.get(*form_ref) {
                    Some(Object::Dict(dict)) => dict,
                    _ => Dict::new(),
                };
                Some((FormHome::Indirect(*form_ref), form))
            }
            Some(Object::Dict(form)) => Some((FormHome::InCatalog(root), form.clone())),
            // A document with no `/AcroForm` gets one, written into the
            // catalog: an indirect form would need an object number for a
            // dictionary with two entries in it.
            _ => Some((FormHome::InCatalog(root), Dict::new())),
        }
    }

    pub(super) fn put_acroform(&mut self, home: FormHome, form: Dict) {
        match home {
            FormHome::Indirect(form_ref) => self.put(form_ref, Object::Dict(form)),
            FormHome::InCatalog(root) => {
                let Some(Object::Dict(mut catalog)) = self.get(root) else {
                    return;
                };
                let key = self.intern(b"AcroForm");
                catalog.insert(key, Object::Dict(form));
                self.put(root, Object::Dict(catalog));
            }
        }
    }
}

/// Where a document's `/AcroForm` lives, and therefore where an edit to it
/// has to be written.
#[derive(Clone, Copy)]
pub(super) enum FormHome {
    /// Its own object.
    Indirect(ObjRef),
    /// Directly inside the catalog, whose reference this is.
    InCatalog(ObjRef),
}
