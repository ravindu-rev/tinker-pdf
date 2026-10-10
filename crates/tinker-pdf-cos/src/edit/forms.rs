//! Creating, filling, resetting and recalculating interactive form fields
//! (12.7).

use super::{without, DocumentEditor};
use crate::appearance::{self, ButtonStyle};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages::Rect;
use crate::resolve::Resolve;
use crate::text_string::{decode_text_string, encode_text_string};
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
    /// The tree is walked through the editor itself ([`crate::Resolve`]), so
    /// every object on the way is read overlay first: a field this editor has
    /// put and listed in `/Fields` is found, and a value it has written is
    /// the value read. A calculation reads the values it computes from, and
    /// one that read them from under its own writes would compute a total
    /// from inputs the file no longer has. An absent `/V` is read as absent,
    /// or as the parent's where there is one (12.7.3.1), which is what a
    /// reader of the saved file sees — 12.7.5.3's reset removes `/V` rather
    /// than blanking it, and "absent" is an answer.
    ///
    /// This used to walk the *file* and then patch in the `/V` of any field
    /// whose own object the overlay held, which found no field the editor had
    /// added and no value inherited from a parent it had changed.
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
        form::fields_in(self, budget)
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

    /// Fills a field, saying exactly what happened.
    ///
    /// A text or choice field takes its text. A **check box or radio group**
    /// takes the *name* of the state to show — its export value, such as `On`
    /// or `blue`, or `Off` — which is what an FDF or XFDF file carries for one
    /// and what [`crate::form::FieldValue::State`] reads back (12.7.4.2.3,
    /// 12.7.4.2.4). The state must be one some widget's `/AP /N` offers,
    /// because a `/V` naming a state nothing can draw is a box that reads as
    /// ticked and displays as empty; every widget's `/AS` follows `/V`, all of
    /// them or none, as [`DocumentEditor::set_checkbox`] and
    /// [`DocumentEditor::select_radio`] do.
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
    /// For a text or choice field the only thing [`Writer`] changes is whether
    /// ReadOnly applies; every other rule about the value is
    /// [`fill::accepts_value`], shared, so the two doors cannot drift apart.
    ///
    /// A check box or radio group is the **user's** door only: it takes a
    /// state through `write_button`, ReadOnly and all. A calculation is
    /// refused one, as it was before buttons could be filled at all, because
    /// [`fill::accepts_value`] accepts no button — so
    /// [`DocumentEditor::set_calculated_values`] cannot tick a ReadOnly check
    /// box, and the calculation path is what it was.
    fn write_field(
        &mut self,
        name: &str,
        value: &str,
        writer: Writer,
    ) -> Result<Vec<SkippedWidget>, FillError> {
        let Some(field) = self.fields().into_iter().find(|f| f.name == name) else {
            return Err(FillError::NoSuchField);
        };
        if writer == Writer::User
            && matches!(
                field.kind,
                form::FieldKind::Checkbox | form::FieldKind::Radio
            )
        {
            return self.write_button(&field, value);
        }
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

    /// A check box or radio group, set to the state `value` names.
    ///
    /// Everything that can refuse is checked before anything is written: the
    /// field and every widget must be dictionaries, and the state must be
    /// `Off` or one a widget's normal appearance offers. A state no widget
    /// offers is refused rather than written, for the reason
    /// [`DocumentEditor::set_checkbox`] gives.
    fn write_button(
        &mut self,
        field: &form::Field,
        value: &str,
    ) -> Result<Vec<SkippedWidget>, FillError> {
        if field.is_read_only() {
            return Err(FillError::ValueRefused);
        }
        // 7.3.5: a name is one to 127 bytes, and an empty one names nothing a
        // widget could offer.
        if value.is_empty() {
            return Err(FillError::ValueRefused);
        }
        let off = self.intern(b"Off");
        let wanted = self.intern(value.as_bytes());
        let ap = self.intern(b"AP");
        let n = self.intern(b"N");

        let mut shows = Vec::with_capacity(field.widgets.len());
        for widget in &field.widgets {
            let Some(Object::Dict(dict)) = self.get(*widget) else {
                return Err(FillError::FieldUnreadable);
            };
            let offers = wanted != off
                && self
                    .resolve_key(&dict, ap)
                    .as_dict()
                    .map(|ap| self.resolve_key(ap, n))
                    .is_some_and(|states| {
                        states
                            .as_dict()
                            .is_some_and(|states| states.get(wanted).is_some())
                    });
            shows.push((*widget, offers));
        }
        if wanted != off && !shows.iter().any(|(_, offers)| *offers) {
            return Err(FillError::ValueRefused);
        }
        let Some(Object::Dict(mut dict)) = self.get(field.reference) else {
            return Err(FillError::FieldUnreadable);
        };

        dict.insert(self.intern(b"V"), Object::Name(wanted));
        self.put(field.reference, Object::Dict(dict));
        for (widget, offers) in shows {
            // Checked above to be a dictionary, and nothing since has
            // replaced it with anything else.
            self.set_appearance_state(widget, if offers { wanted } else { off });
        }
        self.clear_need_appearances();
        Ok(Vec::new())
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
                match form::on_state_in(&*tx, *widget) {
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
        // Read through the overlay: a widget, a form or a `/DR` font this
        // editor has changed or added is drawn as it now is.
        let resources = form::default_resources_in(self);
        let quadding = fill::quadding_in(self, field);
        let multiline = field.flags & fill::MULTILINE != 0;
        let comb = (field.flags & fill::COMB != 0)
            .then_some(field.max_len)
            .flatten();

        let mut skipped = Vec::new();
        for widget in &field.widgets {
            let Some(rect) = fill::widget_rect_in(self, *widget) else {
                // 12.5.2 Table 164 makes /Rect required, so this is a damaged
                // file rather than a widget with nothing to draw. It used to
                // be a bare `continue` and the field still reported success.
                skipped.push(SkippedWidget {
                    widget: *widget,
                    reason: WidgetDefect::RectMissing,
                });
                continue;
            };
            let da = fill::appearance_string_in(self, field, *widget);
            let stream = fill::text_appearance_in(
                &*self,
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
    ///
    /// The form is found through this editor's own catalog. It was read from
    /// the file's, which cost twice when `/AcroForm` sits directly in the
    /// catalog: the flag an earlier edit had set was not seen, and the file's
    /// catalog was written back over every earlier change to it.
    fn clear_need_appearances(&mut self) {
        let key = self.intern(b"AcroForm");
        let flag = self.intern(b"NeedAppearances");
        let Some(catalog) = self.catalog() else {
            return;
        };
        match catalog.get(key) {
            Some(Object::Ref(form_ref)) => {
                let form_ref = *form_ref;
                let Some(Object::Dict(form)) = self.get(form_ref) else {
                    return;
                };
                self.put(form_ref, Object::Dict(without(&form, flag)));
            }
            // A direct /AcroForm cannot be replaced without rewriting the
            // catalog, which is done here rather than skipped.
            Some(Object::Dict(form)) => {
                let cleaned = without(form, flag);
                self.update_catalog(|catalog| {
                    catalog.insert(key, Object::Dict(cleaned));
                });
            }
            _ => {}
        }
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
        let catalog = self.catalog()?;
        let key = self.intern(b"AcroForm");
        match catalog.get(key) {
            Some(Object::Ref(form_ref)) => {
                let form = match self.get(*form_ref) {
                    Some(Object::Dict(dict)) => dict,
                    _ => Dict::new(),
                };
                Some((FormHome::Indirect(*form_ref), form))
            }
            Some(Object::Dict(form)) => Some((FormHome::InCatalog, form.clone())),
            // A document with no `/AcroForm` gets one, written into the
            // catalog: an indirect form would need an object number for a
            // dictionary with two entries in it.
            _ => Some((FormHome::InCatalog, Dict::new())),
        }
    }

    pub(super) fn put_acroform(&mut self, home: FormHome, form: Dict) {
        match home {
            FormHome::Indirect(form_ref) => self.put(form_ref, Object::Dict(form)),
            FormHome::InCatalog => {
                let key = self.intern(b"AcroForm");
                self.update_catalog(|catalog| {
                    catalog.insert(key, Object::Dict(form));
                });
            }
        }
    }
}

// ---- creating fields (12.7.3, 12.7.4) --------------------------------------

/// 12.7.4.2.1 Table 226: a button that is one of a radio group.
const FF_RADIO: i64 = 1 << 15;
/// 12.7.4.2.1 Table 226: a button that only acts.
const FF_PUSHBUTTON: i64 = 1 << 16;
/// 12.7.4.4 Table 230: a choice field that drops down.
const FF_COMBO: i64 = 1 << 17;
/// 12.7.4.4 Table 230: a combo box whose text may be typed as well as picked.
const FF_EDIT: i64 = 1 << 18;
/// The `/Ff` bits that decide what a field *is* rather than how it behaves.
/// [`NewFieldKind`] says that, so a caller's own flags may not.
const KIND_BITS: i64 = FF_RADIO | FF_PUSHBUTTON | FF_COMBO | FF_EDIT;

/// The resource name a created field's `/DA` names in the form's `/DR`.
const DEFAULT_FONT: &[u8] = b"Helv";

/// Annex C's limit on a name's length, which an export value becomes.
const MAX_NAME_BYTES: usize = 127;

/// One button of a radio group [`DocumentEditor::add_field`] creates.
#[derive(Clone, Debug, PartialEq)]
pub struct RadioButton {
    /// The button's on state — its export value, and the name `/V` holds
    /// while this button is the one selected (12.7.4.2.4). Not `Off`, which
    /// 12.7.4.2.3 reserves for the state every button also has.
    pub export: String,
    /// The zero-based page the button is drawn on.
    pub page: u32,
    /// Where on that page, in default user space.
    pub rect: Rect,
}

/// What [`DocumentEditor::add_field`] creates, and where its widgets go.
///
/// Every initial value is written as both `/V` and `/DV`, so a reset
/// (12.7.5.3) comes back to the value the field was created with.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum NewFieldKind {
    /// A text field (12.7.4.3), merged with its one widget (12.7.3.3).
    Text {
        /// The zero-based page the widget is drawn on.
        page: u32,
        /// Where on that page, in default user space.
        rect: Rect,
        /// The initial value, if any.
        value: Option<String>,
        /// `/MaxLen`, the most characters the field takes.
        max_len: Option<u32>,
    },
    /// A check box (12.7.4.2.3), merged with its one widget.
    Checkbox {
        /// The zero-based page the widget is drawn on.
        page: u32,
        /// Where on that page, in default user space.
        rect: Rect,
        /// The on state's name — `Yes` by convention, and anything but `Off`.
        export: String,
        /// Whether it starts ticked.
        checked: bool,
    },
    /// A radio group (12.7.4.2.4): one field, and one widget per button.
    Radio {
        /// The buttons, at least one, with export values all different.
        buttons: Vec<RadioButton>,
        /// The export value of the button that starts selected, if any.
        selected: Option<String>,
    },
    /// A choice field (12.7.4.4), merged with its one widget: a combo box or
    /// a list box.
    Choice {
        /// The zero-based page the widget is drawn on.
        page: u32,
        /// Where on that page, in default user space.
        rect: Rect,
        /// `/Opt`: each option is its own export value and display text.
        options: Vec<String>,
        /// A combo box (a drop-down) rather than a list box.
        combo: bool,
        /// A combo box whose text may be typed as well as picked. Refused on
        /// a list box, where 12.7.4.4 gives the flag no meaning.
        editable: bool,
        /// The initial selection, if any. One of `options` unless the field
        /// is an editable combo box.
        value: Option<String>,
    },
}

/// A field for [`DocumentEditor::add_field`] to create.
#[derive(Clone, Debug, PartialEq)]
pub struct NewField {
    /// The fully qualified name (12.7.3.2): the partial names of the field
    /// and its ancestors, joined by periods. An ancestor the form already has
    /// is joined; one it does not is created as a non-terminal field.
    pub name: String,
    /// What kind of field, and where its widgets go.
    pub kind: NewFieldKind,
    /// `/Ff` bits the caller adds — ReadOnly `1`, Required `2`, NoExport `4`
    /// (12.7.4.1 Table 227), Multiline `1 << 12` and the rest of the
    /// kind's own table. The four bits that decide the kind — Radio,
    /// Pushbutton, Combo and Edit — are [`NewFieldKind`]'s to set and are
    /// refused here.
    pub flags: i64,
    /// The `/DA` font size of a text or choice field; `0` auto-sizes
    /// (12.7.4.3).
    pub font_size: f64,
}

impl NewField {
    /// A field of `kind` named `name`, with no flags of the caller's and an
    /// auto-sized font.
    #[must_use]
    pub fn new(name: impl Into<String>, kind: NewFieldKind) -> NewField {
        NewField {
            name: name.into(),
            kind,
            flags: 0,
            font_size: 0.0,
        }
    }
}

/// Why [`DocumentEditor::add_field`] created nothing.
///
/// Every variant means the editor is exactly as it was: the field is built
/// inside a transaction, and everything that can refuse is checked before
/// anything is written.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddFieldError {
    /// The name is empty, or one of its partial names is — a leading,
    /// trailing or doubled period (12.7.3.2).
    NameMalformed,
    /// A field of that fully qualified name exists already, or one exists
    /// beneath it; two fields answering to one name are one field a filler
    /// cannot address.
    NameTaken(String),
    /// A field on the way down the name is a terminal field, which has
    /// widgets rather than kids (12.7.3.1).
    AncestorIsTerminal(String),
    /// There is no page at that index.
    NoSuchPage(u32),
    /// A rectangle with no area, or with a coordinate that is not a number:
    /// 12.5.2 Table 164 requires a real box to draw in.
    RectUnusable,
    /// A button's export value is empty, is `Off`, repeats another button's,
    /// or cannot be a name (7.3.5: no NUL byte, and Annex C's 127 bytes).
    ExportUnusable(String),
    /// A radio group with no buttons.
    NoButtons,
    /// An initial value the field would refuse from a user: longer than
    /// `/MaxLen`, not among a list's options, or not one of the radio
    /// group's export values.
    ValueRefused,
    /// Flags that contradict the kind: one of the four bits the kind decides,
    /// or Edit on a list box.
    FlagsContradictKind,
    /// A font size that is negative or not a number.
    FontSizeUnusable,
    /// The document has no catalog to hang a form on.
    NoCatalog,
}

impl core::fmt::Display for AddFieldError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AddFieldError::NameMalformed => f.write_str("the field name has an empty part"),
            AddFieldError::NameTaken(name) => write!(f, "a field named {name:?} exists"),
            AddFieldError::AncestorIsTerminal(name) => {
                write!(f, "{name:?} is a terminal field and cannot have kids")
            }
            AddFieldError::NoSuchPage(page) => write!(f, "no page {page}"),
            AddFieldError::RectUnusable => f.write_str("the rectangle has no area"),
            AddFieldError::ExportUnusable(export) => {
                write!(f, "{export:?} cannot be a button's export value")
            }
            AddFieldError::NoButtons => f.write_str("a radio group needs a button"),
            AddFieldError::ValueRefused => f.write_str("the field would refuse that value"),
            AddFieldError::FlagsContradictKind => {
                f.write_str("the flags contradict the kind of field")
            }
            AddFieldError::FontSizeUnusable => f.write_str("the font size is not usable"),
            AddFieldError::NoCatalog => f.write_str("the document has no catalog"),
        }
    }
}

impl std::error::Error for AddFieldError {}

/// The partial names of a fully qualified one, each non-empty (12.7.3.2).
fn partial_names(name: &str) -> Result<Vec<&str>, AddFieldError> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(AddFieldError::NameMalformed);
    }
    Ok(parts)
}

/// `rect` ordered, or `None` when it has no area or is not finite.
pub(super) fn usable_rect(rect: Rect) -> Option<Rect> {
    let values = [rect.x0, rect.y0, rect.x1, rect.y1];
    if !values.iter().all(|v| v.is_finite()) {
        return None;
    }
    let ordered = Rect {
        x0: rect.x0.min(rect.x1),
        y0: rect.y0.min(rect.y1),
        x1: rect.x0.max(rect.x1),
        y1: rect.y0.max(rect.y1),
    };
    (ordered.x1 - ordered.x0 > 0.0 && ordered.y1 - ordered.y0 > 0.0).then_some(ordered)
}

/// Whether `export` can be a button's on state: a name (7.3.5) that is not
/// `Off`.
fn usable_export(export: &str) -> Result<(), AddFieldError> {
    if export.is_empty()
        || export == "Off"
        || export.contains('\0')
        || export.len() > MAX_NAME_BYTES
    {
        return Err(AddFieldError::ExportUnusable(export.to_string()));
    }
    Ok(())
}

/// One widget a new field places: its page, its rectangle and, for a button,
/// the state it turns on.
struct Placement<'a> {
    page: ObjRef,
    rect: Rect,
    export: Option<&'a str>,
}

impl DocumentEditor {
    /// Creates an interactive form field (12.7.3), with a widget on the page
    /// and an appearance for every state the field can be in.
    ///
    /// A text or choice field is drawn by the same layout a fill uses, from
    /// a `/DA` naming `/Helv` in the form's `/DR` — which is added, as
    /// Helvetica, to a form that has no `/Helv` — so creating a field and
    /// filling it produce the same appearance for the same value. A check box
    /// and every radio button get an `/Off` appearance and one for their on
    /// state, keyed by the export value, and `/AS` selects between them
    /// (12.7.4.2). Nothing asks a viewer to draw them: `/NeedAppearances` is
    /// neither set nor needed.
    ///
    /// The field is found by [`DocumentEditor::fields`] at once, and filled
    /// by [`DocumentEditor::fill_field`] like any field the file already had.
    /// Returns the terminal field's object — for a radio group, the field
    /// whose kids are the buttons' widgets.
    ///
    /// A dotted name creates the hierarchy it describes (12.7.3.2): `a.b.c`
    /// joins or creates the non-terminal fields `a` and `a.b`, and puts `c`
    /// beneath them.
    ///
    /// # Errors
    ///
    /// [`AddFieldError`], and then nothing was written.
    pub fn add_field(&mut self, spec: &NewField) -> Result<ObjRef, AddFieldError> {
        self.transaction(|tx| tx.create_field(spec))
    }

    fn create_field(&mut self, spec: &NewField) -> Result<ObjRef, AddFieldError> {
        let partials = partial_names(&spec.name)?;
        let Some((last, ancestors)) = partials.split_last() else {
            return Err(AddFieldError::NameMalformed);
        };
        if spec.flags & KIND_BITS != 0 {
            return Err(AddFieldError::FlagsContradictKind);
        }
        if !spec.font_size.is_finite() || spec.font_size < 0.0 {
            return Err(AddFieldError::FontSizeUnusable);
        }
        let placements = self.placements(&spec.kind)?;

        let below = format!("{}.", spec.name);
        for field in self.fields() {
            if field.name.is_empty() {
                continue;
            }
            if field.name == spec.name || field.name.starts_with(&below) {
                return Err(AddFieldError::NameTaken(spec.name.clone()));
            }
            if spec.name.starts_with(&format!("{}.", field.name)) {
                return Err(AddFieldError::AncestorIsTerminal(field.name));
            }
        }
        if self.catalog().is_none() {
            return Err(AddFieldError::NoCatalog);
        }

        // Everything that can refuse has been checked; from here on the
        // field is built.
        self.ensure_default_font();
        let parent = self.field_ancestors(ancestors)?;
        let field_ref = self.allocate();
        let da = Object::String(crate::object::PdfString::literal(
            format!(
                "/{} {} Tf 0 g",
                String::from_utf8_lossy(DEFAULT_FONT),
                crate::build::number(spec.font_size)
            )
            .into_bytes(),
        ));
        let version = self.text_version();

        let mut widgets: Vec<(ObjRef, ObjRef)> = Vec::new();
        let mut text_value: Option<String> = None;
        match &spec.kind {
            NewFieldKind::Text { value, max_len, .. } => {
                let place = placements.first().ok_or(AddFieldError::RectUnusable)?;
                let mut dict = self.widget_dict(place);
                self.field_entries(&mut dict, b"Tx", last, parent, spec.flags);
                dict.insert(self.intern(b"DA"), da);
                if let Some(max) = max_len {
                    dict.insert(self.intern(b"MaxLen"), Object::Int(i64::from(*max)));
                }
                if let Some(value) = value {
                    let v = fill::value_object_in(value, version);
                    dict.insert(self.intern(b"V"), v.clone());
                    dict.insert(self.intern(b"DV"), v);
                }
                self.put(field_ref, Object::Dict(dict));
                widgets.push((field_ref, place.page));
                text_value = Some(value.clone().unwrap_or_default());
            }
            NewFieldKind::Choice {
                options,
                combo,
                editable,
                value,
                ..
            } => {
                let place = placements.first().ok_or(AddFieldError::RectUnusable)?;
                let mut flags = spec.flags;
                if *combo {
                    flags |= FF_COMBO;
                }
                if *editable {
                    flags |= FF_EDIT;
                }
                let mut dict = self.widget_dict(place);
                self.field_entries(&mut dict, b"Ch", last, parent, flags);
                dict.insert(self.intern(b"DA"), da);
                let opt = options
                    .iter()
                    .map(|o| Object::String(encode_text_string(o, version)))
                    .collect();
                dict.insert(self.intern(b"Opt"), Object::Array(opt));
                if let Some(value) = value {
                    let v = fill::value_object_in(value, version);
                    dict.insert(self.intern(b"V"), v.clone());
                    dict.insert(self.intern(b"DV"), v);
                }
                self.put(field_ref, Object::Dict(dict));
                widgets.push((field_ref, place.page));
                text_value = Some(value.clone().unwrap_or_default());
            }
            NewFieldKind::Checkbox { checked, .. } => {
                let place = placements.first().ok_or(AddFieldError::RectUnusable)?;
                let export = self.intern(place.export.unwrap_or("Yes").as_bytes());
                let state = if *checked {
                    export
                } else {
                    self.intern(b"Off")
                };
                let mut dict = self.widget_dict(place);
                self.field_entries(&mut dict, b"Btn", last, parent, spec.flags);
                dict.insert(self.intern(b"V"), Object::Name(state));
                dict.insert(self.intern(b"DV"), Object::Name(state));
                dict.insert(self.intern(b"AS"), Object::Name(state));
                let ap = self.button_states(place.rect, ButtonStyle::Check, export);
                dict.insert(self.intern(b"AP"), Object::Dict(ap));
                self.put(field_ref, Object::Dict(dict));
                widgets.push((field_ref, place.page));
            }
            NewFieldKind::Radio { selected, .. } => {
                let off = self.intern(b"Off");
                let state = selected
                    .as_deref()
                    .map_or(off, |s| self.intern(s.as_bytes()));
                let mut kids = Vec::with_capacity(placements.len());
                for place in &placements {
                    let widget = self.allocate();
                    let export = self.intern(place.export.unwrap_or("Off").as_bytes());
                    let mut dict = self.widget_dict(place);
                    dict.insert(self.intern(b"Parent"), Object::Ref(field_ref));
                    let shown = if export == state { export } else { off };
                    dict.insert(self.intern(b"AS"), Object::Name(shown));
                    let ap = self.button_states(place.rect, ButtonStyle::Radio, export);
                    dict.insert(self.intern(b"AP"), Object::Dict(ap));
                    self.put(widget, Object::Dict(dict));
                    kids.push(Object::Ref(widget));
                    widgets.push((widget, place.page));
                }
                let mut dict = Dict::new();
                self.field_entries(&mut dict, b"Btn", last, parent, spec.flags | FF_RADIO);
                dict.insert(self.intern(b"V"), Object::Name(state));
                dict.insert(self.intern(b"DV"), Object::Name(state));
                dict.insert(Name::KIDS, Object::Array(kids));
                self.put(field_ref, Object::Dict(dict));
            }
        }

        self.attach_field(parent, field_ref);
        for (widget, page) in widgets {
            self.append_to_page_annots(page, widget);
        }
        if let Some(value) = text_value {
            // Drawn by the fill layer's own path, from the field as the tree
            // walk reads it back: what `fields()` says about the field is what
            // the appearance was laid out from.
            if let Some(field) = self
                .fields()
                .into_iter()
                .find(|field| field.reference == field_ref)
            {
                self.regenerate_text(&field, &value);
            }
        }
        Ok(field_ref)
    }

    /// Every widget `kind` places, after checking everything about the kind
    /// that could refuse.
    fn placements<'a>(&self, kind: &'a NewFieldKind) -> Result<Vec<Placement<'a>>, AddFieldError> {
        let pages = self.page_refs();
        let place = |page: u32, rect: Rect, export: Option<&'a str>| {
            let page_ref = pages
                .get(page as usize)
                .copied()
                .ok_or(AddFieldError::NoSuchPage(page))?;
            let rect = usable_rect(rect).ok_or(AddFieldError::RectUnusable)?;
            Ok::<_, AddFieldError>(Placement {
                page: page_ref,
                rect,
                export,
            })
        };
        match kind {
            NewFieldKind::Text {
                page,
                rect,
                value,
                max_len,
            } => {
                if let (Some(value), Some(max)) = (value, max_len) {
                    // `/MaxLen 0` caps nothing, which is how the fill layer
                    // reads it too.
                    if *max > 0 && value.chars().count() > *max as usize {
                        return Err(AddFieldError::ValueRefused);
                    }
                }
                Ok(vec![place(*page, *rect, None)?])
            }
            NewFieldKind::Checkbox {
                page, rect, export, ..
            } => {
                usable_export(export)?;
                Ok(vec![place(*page, *rect, Some(export.as_str()))?])
            }
            NewFieldKind::Radio { buttons, selected } => {
                if buttons.is_empty() {
                    return Err(AddFieldError::NoButtons);
                }
                let mut out = Vec::with_capacity(buttons.len());
                for (index, button) in buttons.iter().enumerate() {
                    usable_export(&button.export)?;
                    if buttons[..index].iter().any(|b| b.export == button.export) {
                        return Err(AddFieldError::ExportUnusable(button.export.clone()));
                    }
                    out.push(place(
                        button.page,
                        button.rect,
                        Some(button.export.as_str()),
                    )?);
                }
                if let Some(selected) = selected {
                    if !buttons.iter().any(|b| &b.export == selected) {
                        return Err(AddFieldError::ValueRefused);
                    }
                }
                Ok(out)
            }
            NewFieldKind::Choice {
                page,
                rect,
                options,
                combo,
                editable,
                value,
            } => {
                if *editable && !*combo {
                    return Err(AddFieldError::FlagsContradictKind);
                }
                if let Some(value) = value {
                    if !(*editable || options.iter().any(|o| o == value)) {
                        return Err(AddFieldError::ValueRefused);
                    }
                }
                Ok(vec![place(*page, *rect, None)?])
            }
        }
    }

    /// A widget annotation's own entries (12.5.2, 12.5.6.19).
    fn widget_dict(&self, place: &Placement<'_>) -> Dict {
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.intern(b"Annot")));
        dict.insert(
            self.intern(b"Subtype"),
            Object::Name(self.intern(b"Widget")),
        );
        dict.insert(
            self.intern(b"Rect"),
            Object::Array(vec![
                Object::Real(place.rect.x0),
                Object::Real(place.rect.y0),
                Object::Real(place.rect.x1),
                Object::Real(place.rect.y1),
            ]),
        );
        // 12.5.3 Table 165: Print, so the field is on paper as well as on
        // screen.
        dict.insert(self.intern(b"F"), Object::Int(4));
        dict.insert(self.intern(b"P"), Object::Ref(place.page));
        dict
    }

    /// A field dictionary's own entries (12.7.3.1 Table 226).
    ///
    /// `/Ff` is written even when it is zero: it is inheritable, and a field
    /// created under an existing parent would otherwise take the parent's —
    /// a check box under a node carrying the Radio bit would read back as a
    /// radio group.
    fn field_entries(
        &self,
        dict: &mut Dict,
        kind: &[u8],
        partial: &str,
        parent: Option<ObjRef>,
        flags: i64,
    ) {
        dict.insert(self.intern(b"FT"), Object::Name(self.intern(kind)));
        dict.insert(
            self.intern(b"T"),
            Object::String(encode_text_string(partial, self.text_version())),
        );
        dict.insert(self.intern(b"Ff"), Object::Int(flags));
        if let Some(parent) = parent {
            dict.insert(self.intern(b"Parent"), Object::Ref(parent));
        }
    }

    /// A button widget's `/AP`: an `/N` dictionary with the on state under
    /// `export` and `/Off` beside it (12.7.4.2.3).
    fn button_states(&mut self, rect: Rect, style: ButtonStyle, export: Name) -> Dict {
        let (w, h) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
        let on = self.allocate();
        let stream = appearance::button(&self.doc, w, h, style, true);
        self.put_stream(on, stream);
        let off = self.allocate();
        let stream = appearance::button(&self.doc, w, h, style, false);
        self.put_stream(off, stream);
        let mut states = Dict::new();
        states.insert(export, Object::Ref(on));
        states.insert(self.intern(b"Off"), Object::Ref(off));
        let mut ap = Dict::new();
        ap.insert(self.intern(b"N"), Object::Dict(states));
        ap
    }

    /// The fields directly beneath `parent`, or the form's `/Fields` when
    /// there is none.
    fn field_kids(&self, parent: Option<ObjRef>) -> Vec<ObjRef> {
        let list = match parent {
            Some(parent) => self
                .get(parent)
                .and_then(|o| o.as_dict().map(|d| self.resolve_key(d, Name::KIDS))),
            None => form::acro_form_in(self).map(|f| self.resolve_key(&f, self.intern(b"Fields"))),
        };
        list.and_then(|l| {
            l.as_array()
                .map(|items| items.iter().filter_map(Object::as_objref).collect())
        })
        .unwrap_or_default()
    }

    /// Joins or creates the non-terminal fields `ancestors` names, from the
    /// root down, and returns the deepest.
    fn field_ancestors(&mut self, ancestors: &[&str]) -> Result<Option<ObjRef>, AddFieldError> {
        let t = self.intern(b"T");
        let widget = self.intern(b"Widget");
        let subtype = self.intern(b"Subtype");
        let mut parent: Option<ObjRef> = None;
        for (depth, partial) in ancestors.iter().enumerate() {
            let found = self.field_kids(parent).into_iter().find_map(|kid| {
                let dict = self.get(kid)?.as_dict()?.clone();
                let name = dict
                    .get(t)
                    .and_then(Object::as_string)
                    .map(|s| decode_text_string(&s.bytes))?;
                (name == *partial).then_some((kid, dict))
            });
            let node = match found {
                // A widget is a terminal field's own drawing, and cannot hold
                // fields. The name check has already refused every terminal
                // field `fields()` reports; this is the one that reports none
                // because it has no type.
                Some((_, dict)) if dict.get_name(subtype) == Some(widget) => {
                    return Err(AddFieldError::AncestorIsTerminal(
                        ancestors[..=depth].join("."),
                    ));
                }
                Some((kid, _)) => kid,
                None => {
                    let node = self.allocate();
                    let mut dict = Dict::new();
                    dict.insert(
                        t,
                        Object::String(encode_text_string(partial, self.text_version())),
                    );
                    dict.insert(Name::KIDS, Object::Array(Vec::new()));
                    if let Some(parent) = parent {
                        dict.insert(self.intern(b"Parent"), Object::Ref(parent));
                    }
                    self.put(node, Object::Dict(dict));
                    self.attach_field(parent, node);
                    node
                }
            };
            parent = Some(node);
        }
        Ok(parent)
    }

    /// Appends `child` to `parent`'s `/Kids`, or to the form's `/Fields`
    /// when there is no parent — through the array's own object where it
    /// is one.
    fn attach_field(&mut self, parent: Option<ObjRef>, child: ObjRef) {
        match parent {
            Some(parent) => {
                let Some(Object::Dict(mut dict)) = self.get(parent) else {
                    return;
                };
                if self.push_to_list(&mut dict, Name::KIDS, child) {
                    self.put(parent, Object::Dict(dict));
                }
            }
            None => {
                let Some((home, mut form)) = self.acroform() else {
                    return;
                };
                let key = self.intern(b"Fields");
                if self.push_to_list(&mut form, key, child) {
                    self.put_acroform(home, form);
                }
            }
        }
    }

    /// Appends `item` to the array at `holder[key]`, making one if there is
    /// none. True when `holder` itself changed; an array that is its own
    /// object is written there instead.
    fn push_to_list(&mut self, holder: &mut Dict, key: Name, item: ObjRef) -> bool {
        match holder.get(key).cloned() {
            Some(Object::Ref(array_ref)) => {
                let mut list = match self.get(array_ref) {
                    Some(Object::Array(items)) => items,
                    _ => Vec::new(),
                };
                list.push(Object::Ref(item));
                self.put(array_ref, Object::Array(list));
                false
            }
            other => {
                let mut list = match other {
                    Some(Object::Array(items)) => items,
                    _ => Vec::new(),
                };
                list.push(Object::Ref(item));
                holder.insert(key, Object::Array(list));
                true
            }
        }
    }

    /// Gives the form's `/DR` a `/Helv` where it has none (12.7.3.3).
    ///
    /// Helvetica, because it is one of 9.6.2.2's standard fonts: nothing is
    /// embedded, and every reader has its metrics. An existing `/Helv` is
    /// left as it is and used, whatever it names — which is what a fill of
    /// any field naming `/Helv` in that form already does.
    fn ensure_default_font(&mut self) {
        let Some((home, mut form)) = self.acroform() else {
            return;
        };
        let dr_key = self.intern(b"DR");
        match form.get(dr_key).cloned() {
            Some(Object::Ref(dr_ref)) => {
                let mut dr = match self.get(dr_ref) {
                    Some(Object::Dict(dict)) => dict,
                    _ => Dict::new(),
                };
                if self.add_default_font(&mut dr) {
                    self.put(dr_ref, Object::Dict(dr));
                }
            }
            other => {
                let mut dr = match other {
                    Some(Object::Dict(dict)) => dict,
                    _ => Dict::new(),
                };
                if self.add_default_font(&mut dr) {
                    form.insert(dr_key, Object::Dict(dr));
                    self.put_acroform(home, form);
                }
            }
        }
    }

    /// Adds `/Font /Helv` to `dr` when it has none. True when `dr` itself
    /// changed; a `/Font` that is its own object is written there instead.
    fn add_default_font(&mut self, dr: &mut Dict) -> bool {
        let font_key = self.intern(b"Font");
        let helv = self.intern(DEFAULT_FONT);
        match dr.get(font_key).cloned() {
            Some(Object::Ref(fonts_ref)) => {
                let mut fonts = match self.get(fonts_ref) {
                    Some(Object::Dict(dict)) => dict,
                    _ => Dict::new(),
                };
                if fonts.get(helv).is_none() {
                    let font = self.helvetica();
                    fonts.insert(helv, font);
                    self.put(fonts_ref, Object::Dict(fonts));
                }
                false
            }
            other => {
                let mut fonts = match other {
                    Some(Object::Dict(dict)) => dict,
                    _ => Dict::new(),
                };
                if fonts.get(helv).is_some() {
                    return false;
                }
                let font = self.helvetica();
                fonts.insert(helv, font);
                dr.insert(font_key, Object::Dict(fonts));
                true
            }
        }
    }

    /// A new Helvetica font object (9.6.2.2), in `WinAnsiEncoding` so a
    /// value's Latin-1 characters draw as themselves.
    fn helvetica(&mut self) -> Object {
        let r = self.allocate();
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.intern(b"Font")));
        dict.insert(self.intern(b"Subtype"), Object::Name(self.intern(b"Type1")));
        dict.insert(
            self.intern(b"BaseFont"),
            Object::Name(self.intern(b"Helvetica")),
        );
        dict.insert(
            self.intern(b"Encoding"),
            Object::Name(self.intern(b"WinAnsiEncoding")),
        );
        self.put(r, Object::Dict(dict));
        Object::Ref(r)
    }
}

/// Where a document's `/AcroForm` lives, and therefore where an edit to it
/// has to be written.
#[derive(Clone, Copy)]
pub(super) enum FormHome {
    /// Its own object.
    Indirect(ObjRef),
    /// Directly inside the catalog, which is written back through
    /// [`DocumentEditor::update_catalog`].
    InCatalog,
}
