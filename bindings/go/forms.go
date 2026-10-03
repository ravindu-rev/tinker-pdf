package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import "unsafe"

// FieldValueKind is the shape of a field's value (12.7.4).
type FieldValueKind int

// The shapes, transcribed from TpdfFieldValueKind.
const (
	ValueNone  FieldValueKind = 0
	ValueText  FieldValueKind = 1
	ValueState FieldValueKind = 2
	ValueMany  FieldValueKind = 3
)

// FormDataWarningKind is what a form-data reader met and did not read, or
// read leniently (ruling 10).
type FormDataWarningKind int

// The kinds, transcribed from TpdfFormDataWarningKind.
const (
	WarningNotRead         FormDataWarningKind = 0
	WarningValueUnreadable FormDataWarningKind = 1
	WarningTreeCut         FormDataWarningKind = 2
	WarningUnnamed         FormDataWarningKind = 3
)

// RadioButton is one button of a radio group: its export value, the page it
// is drawn on and where.
type RadioButton struct {
	Export         string
	Page           uint32
	X0, Y0, X1, Y1 float64
}

// FormDataWarning is one warning: its kind, the key or element not read (nil
// unless WarningNotRead) and the field it was met in (nil for WarningUnnamed,
// empty for the file itself).
type FormDataWarning struct {
	Kind  FormDataWarningKind
	What  *string
	Field *string
}

// FieldOptions are the parts of a new field every kind shares: the caller's
// /Ff bits and the /DA font size, 0 for auto. The zero value is the engine's
// own default.
type FieldOptions struct {
	Flags    int64
	FontSize float64
}

// optionalString is a C copy of s, or null; the caller frees it.
func optionalString(s *string) *C.char {
	if s == nil {
		return nil
	}
	return cString(*s)
}

// cStrings is a C array of C copies; free releases them all.
func cStrings(values []string) (**C.char, func()) {
	if len(values) == 0 {
		return nil, func() {}
	}
	array := (*[1 << 28]*C.char)(C.malloc(C.size_t(len(values)) * C.size_t(unsafe.Sizeof((*C.char)(nil)))))
	for i, value := range values {
		array[i] = cString(value)
	}
	return &array[0], func() {
		for i := range values {
			C.free(unsafe.Pointer(array[i]))
		}
		C.free(unsafe.Pointer(&array[0]))
	}
}

func (e *Editor) added(f func(*C.uint32_t, *C.uint16_t) C.enum_TpdfStatus) (Ref, error) {
	var number C.uint32_t
	var generation C.uint16_t
	if err := call(func() C.enum_TpdfStatus { return f(&number, &generation) }); err != nil {
		return Ref{}, err
	}
	return Ref{Object: uint32(number), Generation: uint16(generation)}, nil
}

// AddTextField creates a text field (12.7.4.3) merged with its one widget.
// A nil value or maxLen is none. Refused, creating nothing, with the
// engine's reason as StatusEditRefused.
func (e *Editor) AddTextField(name string, page uint32, x0, y0, x1, y1 float64, value *string, maxLen *uint32, options FieldOptions) (Ref, error) {
	cname, cvalue := cString(name), optionalString(value)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(cvalue))
	hasMax, max := C.int(0), C.uint32_t(0)
	if maxLen != nil {
		hasMax, max = 1, C.uint32_t(*maxLen)
	}
	return e.added(func(number *C.uint32_t, generation *C.uint16_t) C.enum_TpdfStatus {
		return C.tpdf_editor_add_text_field(e.ptr, cname, C.uint32_t(page), C.double(x0), C.double(y0),
			C.double(x1), C.double(y1), cvalue, hasMax, max, C.int64_t(options.Flags),
			C.double(options.FontSize), number, generation)
	})
}

// AddCheckbox creates a check box (12.7.4.2.3) whose on state is export.
func (e *Editor) AddCheckbox(name string, page uint32, x0, y0, x1, y1 float64, export string, checked bool, options FieldOptions) (Ref, error) {
	cname, cexport := cString(name), cString(export)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(cexport))
	return e.added(func(number *C.uint32_t, generation *C.uint16_t) C.enum_TpdfStatus {
		return C.tpdf_editor_add_checkbox(e.ptr, cname, C.uint32_t(page), C.double(x0), C.double(y0),
			C.double(x1), C.double(y1), cexport, flag(checked), C.int64_t(options.Flags),
			C.double(options.FontSize), number, generation)
	})
}

// AddRadioGroup creates a radio group (12.7.4.2.4): one field and one widget
// per button. A nil selected is none.
func (e *Editor) AddRadioGroup(name string, buttons []RadioButton, selected *string, options FieldOptions) (Ref, error) {
	cname, cselected := cString(name), optionalString(selected)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(cselected))
	var raw *C.TpdfRadioButton
	if len(buttons) > 0 {
		size := C.size_t(len(buttons)) * C.size_t(unsafe.Sizeof(C.TpdfRadioButton{}))
		array := (*[1 << 24]C.TpdfRadioButton)(C.malloc(size))
		defer C.free(unsafe.Pointer(&array[0]))
		for i, button := range buttons {
			export := cString(button.Export)
			defer C.free(unsafe.Pointer(export))
			array[i] = C.TpdfRadioButton{
				export_value: export, page: C.uint32_t(button.Page),
				x0: C.double(button.X0), y0: C.double(button.Y0),
				x1: C.double(button.X1), y1: C.double(button.Y1),
			}
		}
		raw = &array[0]
	}
	return e.added(func(number *C.uint32_t, generation *C.uint16_t) C.enum_TpdfStatus {
		return C.tpdf_editor_add_radio_group(e.ptr, cname, raw, C.size_t(len(buttons)), cselected,
			C.int64_t(options.Flags), C.double(options.FontSize), number, generation)
	})
}

// AddChoiceField creates a choice field (12.7.4.4): a combo box when combo,
// a list box otherwise. A nil value is no initial selection.
func (e *Editor) AddChoiceField(name string, page uint32, x0, y0, x1, y1 float64, choices []string, combo, editable bool, value *string, options FieldOptions) (Ref, error) {
	cname, cvalue := cString(name), optionalString(value)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(cvalue))
	array, free := cStrings(choices)
	defer free()
	return e.added(func(number *C.uint32_t, generation *C.uint16_t) C.enum_TpdfStatus {
		return C.tpdf_editor_add_choice_field(e.ptr, cname, C.uint32_t(page), C.double(x0), C.double(y0),
			C.double(x1), C.double(y1), array, C.size_t(len(choices)), flag(combo), flag(editable),
			cvalue, C.int64_t(options.Flags), C.double(options.FontSize), number, generation)
	})
}

// ApplyFormData imports form data: every field with a value, all or none.
// The outcomes are FillField's.
func (e *Editor) ApplyFormData(data *FormData) ([]SkippedWidget, error) {
	var report *C.TpdfFillReport
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_apply_form_data(e.ptr, data.ptr, &report)
	}); err != nil {
		return nil, err
	}
	return readFillReport(report)
}

// FormData is what an FDF or XFDF file says, or what one will be written
// from: the engine's own copy, so it outlives the document it came from.
type FormData struct {
	ptr *C.TpdfFormData
}

func formData(f func(**C.TpdfFormData) C.enum_TpdfStatus) (*FormData, error) {
	var out *C.TpdfFormData
	if err := call(func() C.enum_TpdfStatus { return f(&out) }); err != nil {
		return nil, err
	}
	return &FormData{ptr: out}, nil
}

// FormData is the data the document's fields hold, in the tree's order.
func (d *Document) FormData() (*FormData, error) {
	return formData(func(out **C.TpdfFormData) C.enum_TpdfStatus { return C.tpdf_document_form_data(d.ptr, out) })
}

// ReadFdf reads an FDF file: every field of it, or StatusFormDataRefused.
func ReadFdf(bytes []byte) (*FormData, error) {
	return formData(func(out **C.TpdfFormData) C.enum_TpdfStatus {
		data, length := bytesArg(bytes)
		return C.tpdf_form_data_read_fdf(data, length, out)
	})
}

// ReadXfdf reads an XFDF file: every field of it, or StatusFormDataRefused.
func ReadXfdf(bytes []byte) (*FormData, error) {
	return formData(func(out **C.TpdfFormData) C.enum_TpdfStatus {
		data, length := bytesArg(bytes)
		return C.tpdf_form_data_read_xfdf(data, length, out)
	})
}

// NewFormData is empty form data, for AddField to fill.
func NewFormData() (*FormData, error) {
	return formData(func(out **C.TpdfFormData) C.enum_TpdfStatus { return C.tpdf_form_data_new(out) })
}

// Close releases the data.
func (f *FormData) Close() {
	if f != nil && f.ptr != nil {
		C.tpdf_form_data_free(f.ptr)
		f.ptr = nil
	}
}

// AddField appends one field: ValueNone takes no strings, ValueText and
// ValueState one, ValueMany any number.
func (f *FormData) AddField(name string, kind FieldValueKind, values ...string) error {
	cname := cString(name)
	defer C.free(unsafe.Pointer(cname))
	array, free := cStrings(values)
	defer free()
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_form_data_add_field(f.ptr, cname, C.int(kind), array, C.size_t(len(values)))
	})
}

// SetSource sets the document the data belongs to; nil clears it.
func (f *FormData) SetSource(source *string) error {
	csource := optionalString(source)
	defer C.free(unsafe.Pointer(csource))
	return call(func() C.enum_TpdfStatus { return C.tpdf_form_data_set_source(f.ptr, csource) })
}

// Source is the document the data belongs to; nil when it names none.
func (f *FormData) Source() (*string, error) {
	var out *C.char
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_form_data_source(f.ptr, &out) }); err != nil {
		return nil, err
	}
	return takeString(out), nil
}

// Count is how many fields.
func (f *FormData) Count() uint32 { return uint32(C.tpdf_form_data_count(f.ptr)) }

// FieldName is a field's fully qualified name.
func (f *FormData) FieldName(index uint32) (string, error) {
	var out *C.char
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_form_data_field_name(f.ptr, C.uint32_t(index), &out)
	}); err != nil {
		return "", err
	}
	if text := takeString(out); text != nil {
		return *text, nil
	}
	return "", nil
}

// ValueKind is the shape of a field's value.
func (f *FormData) ValueKind(index uint32) (FieldValueKind, error) {
	var out C.enum_TpdfFieldValueKind
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_form_data_field_value_kind(f.ptr, C.uint32_t(index), &out)
	})
	return FieldValueKind(out), err
}

// Values is the strings a field's value is made of.
func (f *FormData) Values(index uint32) ([]string, error) {
	count := uint32(C.tpdf_form_data_field_value_count(f.ptr, C.uint32_t(index)))
	values := make([]string, 0, count)
	for i := uint32(0); i < count; i++ {
		var out *C.char
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_form_data_field_value(f.ptr, C.uint32_t(index), C.uint32_t(i), &out)
		}); err != nil {
			return nil, err
		}
		text := takeString(out)
		if text == nil {
			values = append(values, "")
		} else {
			values = append(values, *text)
		}
	}
	return values, nil
}

// Warnings is every warning the reader left.
func (f *FormData) Warnings() ([]FormDataWarning, error) {
	count := uint32(C.tpdf_form_data_warning_count(f.ptr))
	warnings := make([]FormDataWarning, 0, count)
	for i := uint32(0); i < count; i++ {
		var kind C.enum_TpdfFormDataWarningKind
		var what, field *C.char
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_form_data_warning(f.ptr, C.uint32_t(i), &kind, &what, &field)
		}); err != nil {
			return nil, err
		}
		warnings = append(warnings, FormDataWarning{
			Kind: FormDataWarningKind(kind), What: takeString(what), Field: takeString(field),
		})
	}
	return warnings, nil
}

// ToFdf is the data written as an FDF file (12.7.8).
func (f *FormData) ToFdf() ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_form_data_to_fdf(f.ptr, &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}

// ToXfdf is the data written as an XFDF file, UTF-8; StatusFormDataRefused
// for a value XML 1.0 cannot carry.
func (f *FormData) ToXfdf() ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_form_data_to_xfdf(f.ptr, &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}
