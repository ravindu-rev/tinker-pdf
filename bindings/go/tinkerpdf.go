// Package tinkerpdf is the Go binding: cgo over the C ABI in
// crates/tinker-pdf-ffi, compiled against its committed header
// crates/tinker-pdf-ffi/include/tinker_pdf.h.
//
// Ruling 11 is the whole design: the facade is the only public surface, and
// a binding projects it and adds no logic, caching or defaults of its own.
// Every exported function here is one C call, or a loop of them over a list
// the engine hands back, and nothing else. Scope and packaging:
// docs/features/bindings.md.
//
// The library is linked from target/release (cargo build -p tinker-pdf-ffi
// --release), with that directory as the run-time search path, so a program
// built from this checkout finds the engine it was built against. A host that
// installs the library elsewhere overrides both through CGO_LDFLAGS.
//
// Ownership is the C ABI's: every handle the engine allocates is released by
// its Close, which is safe to call twice and on a nil receiver, and every byte
// slice or string handed back is a Go copy that outlives its handle.
package tinkerpdf

/*
#cgo CFLAGS: -I${SRCDIR}/../../crates/tinker-pdf-ffi/include
#cgo LDFLAGS: -L${SRCDIR}/../../target/release -Wl,-rpath,${SRCDIR}/../../target/release -ltinker_pdf_ffi
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import (
	"fmt"
	"runtime"
	"unsafe"
)

// Status is how a C call went. The numbers are the ABI, append only, and
// pinned by the C crate's own tests.
type Status int

// The statuses, transcribed from TpdfStatus.
const (
	StatusOk                 Status = 0
	StatusBadArgument        Status = 1
	StatusNotAPdf            Status = 2
	StatusNeedsPassword      Status = 3
	StatusWrongPassword      Status = 4
	StatusNoSuchPage         Status = 5
	StatusNotEncrypted       Status = 6
	StatusUnsupportedHandler Status = 7
	StatusNoSuchSignature    Status = 8
	StatusNoSuchField        Status = 9
	StatusValueRefused       Status = 10
	StatusFieldUnreadable    Status = 11
	StatusSpentHandle        Status = 12
	StatusEditRefused        Status = 13
	StatusSourceMiss         Status = 14
	StatusScriptRefused      Status = 15
	StatusStreamUnreadable   Status = 16
	StatusFormDataRefused    Status = 17
)

// Error is a failure the engine reported: its status, which a caller branches
// on, and the engine's own sentence, which names the call and the argument.
type Error struct {
	Status  Status
	Message string
}

func (e *Error) Error() string {
	return fmt.Sprintf("tinker-pdf: %s (status %d)", e.Message, int(e.Status))
}

// call runs one C call and turns a non-Ok status into an *Error.
//
// The OS thread is locked for the call and the message read after it, because
// tpdf_last_error_message is per thread and a goroutine may otherwise move
// between the two and read another call's message — or none.
func call(f func() C.enum_TpdfStatus) error {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	status := f()
	if status == C.TPDF_STATUS_OK {
		return nil
	}
	message := fmt.Sprintf("tinker-pdf error %d", int(status))
	if p := C.tpdf_last_error_message(); p != nil {
		message = C.GoString(p)
	}
	return &Error{Status: Status(status), Message: message}
}

// takeString copies an engine-allocated string and frees it; nil for null,
// which on the C ABI means "the document does not say".
func takeString(p *C.char) *string {
	if p == nil {
		return nil
	}
	s := C.GoString(p)
	C.tpdf_string_free(p)
	return &s
}

// cString allocates a C copy of s; the caller frees it.
func cString(s string) *C.char {
	return C.CString(s)
}

// bytesArg points at a Go byte slice for the length of one call. The C ABI
// refuses a null pointer even with a zero length, so an empty slice points at
// a byte that is never read.
func bytesArg(b []byte) (*C.uint8_t, C.size_t) {
	if len(b) == 0 {
		var zero [1]C.uint8_t
		return &zero[0], 0
	}
	return (*C.uint8_t)(unsafe.Pointer(&b[0])), C.size_t(len(b))
}

// borrowed copies a pointer and length the engine lends; nil for null.
func borrowed(data *C.uint8_t, length C.size_t) []byte {
	if data == nil {
		return nil
	}
	return C.GoBytes(unsafe.Pointer(data), C.int(length))
}

// takeBuffer copies a TpdfBuffer's bytes and frees it; nil for a null
// buffer, which is "the document names none".
func takeBuffer(buffer *C.TpdfBuffer) []byte {
	if buffer == nil {
		return nil
	}
	defer C.tpdf_buffer_free(buffer)
	var length C.size_t
	data := C.tpdf_buffer_data(buffer, &length)
	if data == nil {
		return []byte{}
	}
	return C.GoBytes(unsafe.Pointer(data), C.int(C.tpdf_buffer_len(buffer)))
}

// Version is the engine's version.
func Version() string {
	return C.GoString(C.tpdf_version())
}

// Document is an open PDF.
type Document struct {
	ptr *C.TpdfDocument
}

// Open opens a document from bytes, which are copied.
func Open(data []byte) (*Document, error) {
	var out *C.TpdfDocument
	err := call(func() C.enum_TpdfStatus {
		bytes, length := bytesArg(data)
		return C.tpdf_document_open(bytes, length, &out)
	})
	if err != nil {
		return nil, err
	}
	return &Document{ptr: out}, nil
}

// Close releases the document. Handles taken from it stay valid.
func (d *Document) Close() {
	if d != nil && d.ptr != nil {
		C.tpdf_document_free(d.ptr)
		d.ptr = nil
	}
}

// PageCount is the number of pages.
func (d *Document) PageCount() uint32 {
	return uint32(C.tpdf_document_page_count(d.ptr))
}

// IsEncrypted is whether the document is encrypted.
func (d *Document) IsEncrypted() bool {
	return C.tpdf_document_is_encrypted(d.ptr) != 0
}

// PageSize is a page's size in points.
func (d *Document) PageSize(index uint32) (float64, float64, error) {
	var w, h C.double
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_page_size(d.ptr, C.uint32_t(index), &w, &h)
	})
	return float64(w), float64(h), err
}

// PageText is a page's text, in logical order (ruling 14).
func (d *Document) PageText(index uint32) (string, error) {
	var out *C.char
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_page_text(d.ptr, C.uint32_t(index), &out)
	})
	if err != nil {
		return "", err
	}
	text := takeString(out)
	if text == nil {
		return "", nil
	}
	return *text, nil
}

// SetFonts supplies the face a document that embeds none is drawn with. The
// engine bundles no faces and reads no font directories.
func (d *Document) SetFonts(regular []byte) error {
	return call(func() C.enum_TpdfStatus {
		data, length := bytesArg(regular)
		return C.tpdf_document_set_fonts(d.ptr, data, length, nil, 0, nil, 0, nil, 0)
	})
}

// PixelFormat is how a bitmap stores its pixels.
type PixelFormat int

// The pixel formats, transcribed from TpdfPixelFormat.
const (
	Gray8  PixelFormat = 0
	GrayA8 PixelFormat = 1
	Rgb8   PixelFormat = 2
	Rgba8  PixelFormat = 3
)

// Bitmap is a rendered page.
type Bitmap struct {
	ptr *C.TpdfBitmap
}

// Render draws a page at a scale, 1.0 being 72 dots per inch.
func (d *Document) Render(index uint32, scale float64, format PixelFormat) (*Bitmap, error) {
	var out *C.TpdfBitmap
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_page_render(d.ptr, C.uint32_t(index), C.double(scale),
			C.int(format), &out)
	})
	if err != nil {
		return nil, err
	}
	return &Bitmap{ptr: out}, nil
}

// Width in pixels.
func (b *Bitmap) Width() uint32 { return uint32(C.tpdf_bitmap_width(b.ptr)) }

// Height in pixels.
func (b *Bitmap) Height() uint32 { return uint32(C.tpdf_bitmap_height(b.ptr)) }

// Stride is the bytes per row.
func (b *Bitmap) Stride() int { return int(C.tpdf_bitmap_stride(b.ptr)) }

// Pixels is a copy of the pixels.
func (b *Bitmap) Pixels() []byte {
	var length C.size_t
	data := C.tpdf_bitmap_data(b.ptr, &length)
	return borrowed(data, length)
}

// Close releases the bitmap.
func (b *Bitmap) Close() {
	if b != nil && b.ptr != nil {
		C.tpdf_bitmap_free(b.ptr)
		b.ptr = nil
	}
}

// Defect is one finding of the strict structural validator: the rule it
// broke and the engine's sentence about where.
type Defect struct {
	Rule    string
	Message string
}

// Validate runs the strict structural validator (ruling 13); an empty list is
// a clean document.
func (d *Document) Validate() ([]Defect, error) {
	var defects *C.TpdfDefects
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_document_validate(d.ptr, &defects)
	}); err != nil {
		return nil, err
	}
	defer C.tpdf_defects_free(defects)
	count := uint32(C.tpdf_defects_count(defects))
	found := make([]Defect, 0, count)
	for i := uint32(0); i < count; i++ {
		var rule, message *C.char
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_defect_rule(defects, C.uint32_t(i), &rule)
		}); err != nil {
			return nil, err
		}
		ruleText := takeString(rule)
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_defect_message(defects, C.uint32_t(i), &message)
		}); err != nil {
			return nil, err
		}
		messageText := takeString(message)
		defect := Defect{}
		if ruleText != nil {
			defect.Rule = *ruleText
		}
		if messageText != nil {
			defect.Message = *messageText
		}
		found = append(found, defect)
	}
	return found, nil
}
