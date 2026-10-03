package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import "unsafe"

// AuthLevel is which password a document accepted.
type AuthLevel int

// The authentication levels, transcribed from TpdfAuthLevel.
const (
	AuthNone  AuthLevel = 0
	AuthUser  AuthLevel = 1
	AuthOwner AuthLevel = 2
)

// ScriptPolicy is a bitmask of the form-script triggers a call may run.
type ScriptPolicy uint32

// The policy bits, transcribed from the TPDF_SCRIPT_* constants.
const (
	ScriptCalculate ScriptPolicy = C.TPDF_SCRIPT_CALCULATE
	ScriptFormat    ScriptPolicy = C.TPDF_SCRIPT_FORMAT
	ScriptKeystroke ScriptPolicy = C.TPDF_SCRIPT_KEYSTROKE
	ScriptValidate  ScriptPolicy = C.TPDF_SCRIPT_VALIDATE
	ScriptDocument  ScriptPolicy = C.TPDF_SCRIPT_DOCUMENT
	ScriptCatalog   ScriptPolicy = C.TPDF_SCRIPT_CATALOG
	ScriptDefault   ScriptPolicy = C.TPDF_SCRIPT_DEFAULT
)

// IsStreamed is whether the document's bytes come from a Source.
func (d *Document) IsStreamed() bool { return C.tpdf_document_is_streamed(d.ptr) != 0 }

// MayPrint is whether the document permits printing at the level reached.
// PDF permissions are advisory.
func (d *Document) MayPrint() bool { return C.tpdf_document_may_print(d.ptr) != 0 }

// Authenticate tries a password and returns the level it reached. A wrong one
// is StatusWrongPassword, and an unencrypted document StatusNotEncrypted.
func (d *Document) Authenticate(password string) (AuthLevel, error) {
	cpassword := cString(password)
	defer C.free(unsafe.Pointer(cpassword))
	var level C.enum_TpdfAuthLevel
	err := call(func() C.enum_TpdfStatus { return C.tpdf_document_authenticate(d.ptr, cpassword, &level) })
	return AuthLevel(level), err
}

// IsDirty is whether the editor holds edits not yet saved.
func (e *Editor) IsDirty() bool { return C.tpdf_editor_is_dirty(e.ptr) != 0 }

// PageCount is how many pages the document has as this editor sees it.
func (e *Editor) PageCount() uint32 { return uint32(C.tpdf_editor_page_count(e.ptr)) }

// DeletePage removes a page.
func (e *Editor) DeletePage(index uint32) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_editor_delete_page(e.ptr, C.uint32_t(index)) })
}

// MovePage moves a page to a new position.
func (e *Editor) MovePage(from, to uint32) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_editor_move_page(e.ptr, C.uint32_t(from), C.uint32_t(to)) })
}

// RotatePage turns a page by a quarter-turn multiple, relative to its current
// rotation; any other turn is StatusEditRefused.
func (e *Editor) RotatePage(index uint32, degrees int64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_rotate_page(e.ptr, C.uint32_t(index), C.int64_t(degrees))
	})
}

// InsertPage inserts a blank page at index, which may equal the page count.
func (e *Editor) InsertPage(index uint32, width, height float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_insert_page(e.ptr, C.uint32_t(index), C.double(width), C.double(height))
	})
}

// SetCropBox sets a page's /CropBox (14.11.2).
func (e *Editor) SetCropBox(index uint32, x0, y0, x1, y1 float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_set_crop_box(e.ptr, C.uint32_t(index), C.double(x0), C.double(y0), C.double(x1), C.double(y1))
	})
}

// AppendContent appends operators to a page's content stream.
func (e *Editor) AppendContent(page uint32, operators []byte) error {
	return call(func() C.enum_TpdfStatus {
		data, length := bytesArg(operators)
		return C.tpdf_editor_append_content(e.ptr, C.uint32_t(page), data, length)
	})
}

// FieldCount is how many form fields the document has.
func (e *Editor) FieldCount() uint32 { return uint32(C.tpdf_editor_field_count(e.ptr)) }

func (e *Editor) fieldText(index uint32, f func(C.uint32_t, **C.char) C.enum_TpdfStatus) (string, error) {
	var out *C.char
	if err := call(func() C.enum_TpdfStatus { return f(C.uint32_t(index), &out) }); err != nil {
		return "", err
	}
	if text := takeString(out); text != nil {
		return *text, nil
	}
	return "", nil
}

// FieldName is a field's fully qualified name (12.7.3.2).
func (e *Editor) FieldName(index uint32) (string, error) {
	return e.fieldText(index, func(i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_editor_field_name(e.ptr, i, out)
	})
}

// FieldValue is a field's current value as text; empty when it has none.
func (e *Editor) FieldValue(index uint32) (string, error) {
	return e.fieldText(index, func(i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_editor_field_value(e.ptr, i, out)
	})
}

// Checkpoint is an editor's state, for Restore to put back as often as needed.
type Checkpoint struct {
	ptr *C.TpdfCheckpoint
}

// Checkpoint takes the editor's state as a value.
func (e *Editor) Checkpoint() (*Checkpoint, error) {
	var out *C.TpdfCheckpoint
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_editor_checkpoint(e.ptr, &out) }); err != nil {
		return nil, err
	}
	return &Checkpoint{ptr: out}, nil
}

// Restore puts the editor back to what a checkpoint recorded. Idempotent.
func (e *Editor) Restore(checkpoint *Checkpoint) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_editor_restore(e.ptr, checkpoint.ptr) })
}

// Close releases the checkpoint; nothing was pending, so nothing commits.
func (c *Checkpoint) Close() {
	if c != nil && c.ptr != nil {
		C.tpdf_checkpoint_free(c.ptr)
		c.ptr = nil
	}
}

// ChangedField is a field a recalculation gave a new value.
type ChangedField struct {
	Name  string
	Value string
}

// Recalculation is what a recalculation pass did and did not do.
type Recalculation struct {
	Changed     []ChangedField
	Skipped     uint32
	CascadesCut []string
	Refused     []string
}

func reportNames(count C.uint32_t, f func(C.uint32_t, **C.char) C.enum_TpdfStatus) ([]string, error) {
	names := make([]string, 0, uint32(count))
	for i := C.uint32_t(0); i < count; i++ {
		var out *C.char
		if err := call(func() C.enum_TpdfStatus { return f(i, &out) }); err != nil {
			return nil, err
		}
		if name := takeString(out); name != nil {
			names = append(names, *name)
		}
	}
	return names, nil
}

// Recalculate runs the form's calculate actions under policy, all or nothing:
// a non-nil error means nothing was written.
func (e *Editor) Recalculate(policy ScriptPolicy) (*Recalculation, error) {
	var report *C.TpdfRecalculation
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_recalculate(e.ptr, C.uint32_t(policy), &report)
	}); err != nil {
		return nil, err
	}
	defer C.tpdf_recalculation_free(report)
	names, err := reportNames(C.tpdf_recalculation_changed_count(report), func(i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_recalculation_changed_name(report, i, out)
	})
	if err != nil {
		return nil, err
	}
	values, err := reportNames(C.tpdf_recalculation_changed_count(report), func(i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_recalculation_changed_value(report, i, out)
	})
	if err != nil {
		return nil, err
	}
	result := &Recalculation{Skipped: uint32(C.tpdf_recalculation_skipped_count(report))}
	for i := range names {
		result.Changed = append(result.Changed, ChangedField{Name: names[i], Value: values[i]})
	}
	if result.CascadesCut, err = reportNames(C.tpdf_recalculation_cascades_cut_count(report), func(i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_recalculation_cascades_cut_name(report, i, out)
	}); err != nil {
		return nil, err
	}
	if result.Refused, err = reportNames(C.tpdf_recalculation_refused_count(report), func(i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_recalculation_refused_name(report, i, out)
	}); err != nil {
		return nil, err
	}
	return result, nil
}

// FormattedValue is what the field's format action displays; nil when it
// carries none.
func (e *Editor) FormattedValue(name string, policy ScriptPolicy) (*string, error) {
	cname := cString(name)
	defer C.free(unsafe.Pointer(cname))
	var out *C.char
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_formatted_value(e.ptr, cname, C.uint32_t(policy), &out)
	}); err != nil {
		return nil, err
	}
	return takeString(out), nil
}

// Keystroke offers a keystroke to a field's keystroke action. A refusal is the
// form working, not an error: accepted is false and change is nil.
func (e *Editor) Keystroke(name, change string, selStart, selEnd int64, willCommit bool,
	policy ScriptPolicy) (accepted bool, changed *string, err error) {
	cname, cchange := cString(name), cString(change)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(cchange))
	var ok C.int
	var out *C.char
	err = call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_keystroke(e.ptr, cname, cchange, C.int64_t(selStart), C.int64_t(selEnd),
			flag(willCommit), C.uint32_t(policy), &ok, &out)
	})
	if err != nil {
		return false, nil, err
	}
	return ok != 0, takeString(out), nil
}

// Validate offers a committed value to a field's validate action. Nothing is
// written either way.
func (e *Editor) Validate(name, value string, policy ScriptPolicy) (accepted bool, validated *string, err error) {
	cname, cvalue := cString(name), cString(value)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(cvalue))
	var ok C.int
	var out *C.char
	err = call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_validate(e.ptr, cname, cvalue, C.uint32_t(policy), &ok, &out)
	})
	if err != nil {
		return false, nil, err
	}
	return ok != 0, takeString(out), nil
}

// AddEmbeddedFont embeds a TrueType or CFF program under a resource name.
func (b *Builder) AddEmbeddedFont(resource, baseFont, program []byte) error {
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		f, fl := bytesArg(baseFont)
		p, pl := bytesArg(program)
		return C.tpdf_builder_add_embedded_font(b.ptr, r, rl, f, fl, p, pl)
	})
}

// SetSubsetFonts says whether embedded fonts are subset to the glyphs drawn.
func (b *Builder) SetSubsetFonts(subset bool) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_builder_set_subset_fonts(b.ptr, flag(subset)) })
}

// SetFillRGB sets the fill colour.
func (p *PageBuilder) SetFillRGB(r, g, b float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_page_builder_set_fill_rgb(p.ptr, C.double(r), C.double(g), C.double(b))
	})
}

// SetStrokeRGB sets the stroke colour.
func (p *PageBuilder) SetStrokeRGB(r, g, b float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_page_builder_set_stroke_rgb(p.ptr, C.double(r), C.double(g), C.double(b))
	})
}

// SetCropBox sets the page's /CropBox.
func (p *PageBuilder) SetCropBox(x0, y0, x1, y1 float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_page_builder_set_crop_box(p.ptr, C.double(x0), C.double(y0), C.double(x1), C.double(y1))
	})
}

// Raw writes content-stream operators as they are.
func (p *PageBuilder) Raw(operators []byte) error {
	return call(func() C.enum_TpdfStatus {
		data, length := bytesArg(operators)
		return C.tpdf_page_builder_raw(p.ptr, data, length)
	})
}
