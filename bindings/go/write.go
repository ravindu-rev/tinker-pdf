package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import (
	"math"
	"unsafe"
)

// WriteMode is the shape of output a save produces (7.5.6).
type WriteMode int

// The write modes, transcribed from TpdfWriteMode.
const (
	Rewrite     WriteMode = 0
	Incremental WriteMode = 1
)

// SkippedWidget is a widget a fill wrote a value for and could not draw: the
// fourth outcome, which is reported rather than flattened into failure.
type SkippedWidget struct {
	Object     uint32
	Generation uint16
	Defect     WidgetDefect
	Message    string
}

// WidgetDefect is why a widget could not be drawn.
type WidgetDefect int

// The widget defects, transcribed from TpdfWidgetDefect.
const (
	// WidgetRectMissing: 12.5.2 Table 164 requires /Rect and this widget has
	// no usable one.
	WidgetRectMissing WidgetDefect = 0
)

// Editor is an editor over a document. It holds its own reference to the
// engine's object store, so the document may be closed first.
type Editor struct {
	ptr *C.TpdfEditor
}

// Editor opens an editor over the document.
func (d *Document) Editor() (*Editor, error) {
	var out *C.TpdfEditor
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_editor(d.ptr, &out) }); err != nil {
		return nil, err
	}
	return &Editor{ptr: out}, nil
}

// Close releases the editor.
func (e *Editor) Close() {
	if e != nil && e.ptr != nil {
		C.tpdf_editor_free(e.ptr)
		e.ptr = nil
	}
}

// FillField sets a text field's value. A non-nil error means nothing was
// written; a nil error with skipped widgets means the value was written and
// those widgets were left showing what they showed before.
func (e *Editor) FillField(name, value string) ([]SkippedWidget, error) {
	cname, cvalue := cString(name), cString(value)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(cvalue))
	var report *C.TpdfFillReport
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_fill_field(e.ptr, cname, cvalue, &report)
	}); err != nil {
		return nil, err
	}
	return readFillReport(report)
}

// readFillReport copies a fill report's widgets out and frees it.
func readFillReport(report *C.TpdfFillReport) ([]SkippedWidget, error) {
	defer C.tpdf_fill_report_free(report)
	count := uint32(C.tpdf_fill_report_count(report))
	skipped := make([]SkippedWidget, 0, count)
	for i := uint32(0); i < count; i++ {
		var number C.uint32_t
		var generation C.uint16_t
		var defect C.enum_TpdfWidgetDefect
		var message *C.char
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_fill_report_widget(report, C.uint32_t(i), &number, &generation)
		}); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_fill_report_defect(report, C.uint32_t(i), &defect)
		}); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_fill_report_message(report, C.uint32_t(i), &message)
		}); err != nil {
			return nil, err
		}
		text := takeString(message)
		widget := SkippedWidget{Object: uint32(number), Generation: uint16(generation), Defect: WidgetDefect(defect)}
		if text != nil {
			widget.Message = *text
		}
		skipped = append(skipped, widget)
	}
	return skipped, nil
}

// SetCheckbox ticks or clears a checkbox.
func (e *Editor) SetCheckbox(name string, on bool) error {
	cname := cString(name)
	defer C.free(unsafe.Pointer(cname))
	flag := C.int(0)
	if on {
		flag = 1
	}
	return call(func() C.enum_TpdfStatus { return C.tpdf_editor_set_checkbox(e.ptr, cname, flag) })
}

// SelectRadio selects one option of a radio group.
func (e *Editor) SelectRadio(name, option string) error {
	cname, coption := cString(name), cString(option)
	defer C.free(unsafe.Pointer(cname))
	defer C.free(unsafe.Pointer(coption))
	return call(func() C.enum_TpdfStatus { return C.tpdf_editor_select_radio(e.ptr, cname, coption) })
}

// EntropyLen is how many caller-supplied random bytes an encrypted save
// takes: the 32-byte file key and two 8-byte salts (TPDF_ENTROPY_LEN).
const EntropyLen = C.TPDF_ENTROPY_LEN

// Encryption is how to encrypt on save. There is no entropy default: the
// engine has no opinion about where randomness comes from, and the bytes are
// the one input that makes encrypted output non-reproducible.
type Encryption struct {
	UserPassword  string
	OwnerPassword string
	Permissions   int32
	Entropy       []byte
}

// WriteOptions is TpdfWriteOptions field for field. Start from
// DefaultWriteOptions, the engine's own defaults, and change what you mean.
type WriteOptions struct {
	Mode           WriteMode
	Linearize      bool
	VersionMajor   uint32
	VersionMinor   uint32
	ObjectStreams  bool
	Compress       bool
	GarbageCollect bool
	Encryption     *Encryption
}

// DefaultWriteOptions is what tpdf_write_options_init fills in.
func DefaultWriteOptions() (WriteOptions, error) {
	var raw C.TpdfWriteOptions
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_write_options_init(&raw) }); err != nil {
		return WriteOptions{}, err
	}
	return WriteOptions{
		Mode:           WriteMode(raw.mode),
		Linearize:      raw.linearize != 0,
		VersionMajor:   uint32(raw.version_major),
		VersionMinor:   uint32(raw.version_minor),
		ObjectStreams:  raw.object_streams != 0,
		Compress:       raw.compress != 0,
		GarbageCollect: raw.garbage_collect != 0,
	}, nil
}

// Save writes the edited document under options.
func (e *Editor) Save(options WriteOptions) ([]byte, error) {
	raw := (*C.TpdfWriteOptions)(C.calloc(1, C.sizeof_TpdfWriteOptions))
	defer C.free(unsafe.Pointer(raw))
	raw.mode = C.int(options.Mode)
	raw.linearize = flag(options.Linearize)
	raw.version_major = C.uint32_t(options.VersionMajor)
	raw.version_minor = C.uint32_t(options.VersionMinor)
	raw.object_streams = flag(options.ObjectStreams)
	raw.compress = flag(options.Compress)
	raw.garbage_collect = flag(options.GarbageCollect)
	if options.Encryption != nil {
		// Every field in C memory, so the struct C reads holds no Go pointer.
		encryption := (*C.TpdfEncryption)(C.calloc(1, C.sizeof_TpdfEncryption))
		defer C.free(unsafe.Pointer(encryption))
		encryption.user_password = cString(options.Encryption.UserPassword)
		defer C.free(unsafe.Pointer(encryption.user_password))
		encryption.owner_password = cString(options.Encryption.OwnerPassword)
		defer C.free(unsafe.Pointer(encryption.owner_password))
		encryption.permissions = C.int32_t(options.Encryption.Permissions)
		entropy := C.CBytes(options.Encryption.Entropy)
		defer C.free(entropy)
		encryption.entropy = (*C.uint8_t)(entropy)
		encryption.entropy_len = C.size_t(len(options.Encryption.Entropy))
		raw.encryption = encryption
	}
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_editor_save(e.ptr, raw, &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}

// DestKind is how a destination positions its page (12.3.2.2, Table 151).
type DestKind int

// The destination kinds, transcribed from TpdfDestKind.
const (
	Xyz   DestKind = 0
	Fit   DestKind = 1
	FitH  DestKind = 2
	FitV  DestKind = 3
	FitR  DestKind = 4
	FitB  DestKind = 5
	FitBH DestKind = 6
	FitBV DestKind = 7
)

// View is a destination's view; a nil number is the file's null, "retain the
// current value", which the C ABI spells NaN.
type View struct {
	Kind                           DestKind
	Left, Bottom, Right, Top, Zoom *float64
}

// Number is a convenience for building a View's numbers.
func Number(value float64) *float64 { return &value }

func nanFor(value *float64) C.double {
	if value == nil {
		return C.double(math.NaN())
	}
	return C.double(*value)
}

func nullable(value C.double) *float64 {
	if math.IsNaN(float64(value)) {
		return nil
	}
	v := float64(value)
	return &v
}

func (v View) raw() C.TpdfDestination {
	return C.TpdfDestination{
		kind:   C.int(v.Kind),
		left:   nanFor(v.Left),
		bottom: nanFor(v.Bottom),
		right:  nanFor(v.Right),
		top:    nanFor(v.Top),
		zoom:   nanFor(v.Zoom),
	}
}

// FitView is the engine's own /Fit view, from tpdf_destination_init_fit.
func FitView() (View, error) {
	var raw C.TpdfDestination
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_destination_init_fit(&raw) }); err != nil {
		return View{}, err
	}
	return viewOf(raw), nil
}

func viewOf(raw C.TpdfDestination) View {
	return View{
		Kind:   DestKind(raw.kind),
		Left:   nullable(raw.left),
		Bottom: nullable(raw.bottom),
		Right:  nullable(raw.right),
		Top:    nullable(raw.top),
		Zoom:   nullable(raw.zoom),
	}
}

// Target is where a link or an outline entry goes: a page and a view, or a
// URI. Set exactly one.
type Target struct {
	Page *uint32
	View View
	URI  *string
}

// PageTarget points at a page with a view.
func PageTarget(index uint32, view View) Target { return Target{Page: &index, View: view} }

// URITarget points at a URI.
func URITarget(uri string) Target { return Target{URI: &uri} }

// withTarget builds a TpdfTarget whose URI, if any, lives in C memory for the
// length of f.
func withTarget(target Target, f func(*C.TpdfTarget) C.enum_TpdfStatus) C.enum_TpdfStatus {
	raw := C.TpdfTarget{view: target.View.raw()}
	if target.URI != nil {
		uri := cString(*target.URI)
		defer C.free(unsafe.Pointer(uri))
		raw.kind = C.TPDF_TARGET_KIND_URI
		raw.uri = uri
	} else if target.Page != nil {
		raw.kind = C.TPDF_TARGET_KIND_PAGE
		raw.page_index = C.uint32_t(*target.Page)
	}
	return f(&raw)
}

// OutlineEntry is one outline entry under construction (12.3.3). AddChild,
// Builder.SetOutline and Editor.SetOutline consume what they take; the entry
// still needs its Close.
type OutlineEntry struct {
	ptr *C.TpdfOutlineEntry
}

// NewOutlineEntry starts an entry with a title and no destination.
func NewOutlineEntry(title string) (*OutlineEntry, error) {
	ctitle := cString(title)
	defer C.free(unsafe.Pointer(ctitle))
	var out *C.TpdfOutlineEntry
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_outline_entry_new(ctitle, &out) }); err != nil {
		return nil, err
	}
	return &OutlineEntry{ptr: out}, nil
}

// SetTarget points the entry somewhere.
func (o *OutlineEntry) SetTarget(target Target) error {
	return call(func() C.enum_TpdfStatus {
		return withTarget(target, func(raw *C.TpdfTarget) C.enum_TpdfStatus {
			return C.tpdf_outline_entry_set_target(o.ptr, raw)
		})
	})
}

// SetOpen says whether the entry is shown expanded.
func (o *OutlineEntry) SetOpen(open bool) error {
	flag := C.int(0)
	if open {
		flag = 1
	}
	return call(func() C.enum_TpdfStatus { return C.tpdf_outline_entry_set_open(o.ptr, flag) })
}

// AddChild nests an entry under this one, consuming the child.
func (o *OutlineEntry) AddChild(child *OutlineEntry) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_outline_entry_add_child(o.ptr, child.ptr) })
}

// Close releases the entry.
func (o *OutlineEntry) Close() {
	if o != nil && o.ptr != nil {
		C.tpdf_outline_entry_free(o.ptr)
		o.ptr = nil
	}
}

func entryArray(entries []*OutlineEntry) []*C.TpdfOutlineEntry {
	raw := make([]*C.TpdfOutlineEntry, len(entries))
	for i, entry := range entries {
		raw[i] = entry.ptr
	}
	return raw
}

// ImageKind is which image description a payload carries.
type ImageKind int

// The image kinds, transcribed from TpdfImageKind.
const (
	ImageJpeg  ImageKind = 0
	ImageRgb8  ImageKind = 1
	ImageGray8 ImageKind = 2
)

// Builder assembles a document from pages, fonts and images.
type Builder struct {
	ptr *C.TpdfBuilder
}

// NewBuilder starts a document.
func NewBuilder() (*Builder, error) {
	var out *C.TpdfBuilder
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_builder_new(&out) }); err != nil {
		return nil, err
	}
	return &Builder{ptr: out}, nil
}

// Close releases the builder, finished or not.
func (b *Builder) Close() {
	if b != nil && b.ptr != nil {
		C.tpdf_builder_free(b.ptr)
		b.ptr = nil
	}
}

// AddBaseFont registers one of the standard 14 fonts under a resource name.
func (b *Builder) AddBaseFont(resource, baseFont []byte) error {
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		f, fl := bytesArg(baseFont)
		return C.tpdf_builder_add_base_font(b.ptr, r, rl, f, fl)
	})
}

// AddImage registers an image under a resource name.
func (b *Builder) AddImage(resource []byte, kind ImageKind, width, height uint32, data []byte) error {
	samples := C.CBytes(data)
	defer C.free(samples)
	image := C.TpdfImage{
		kind:     C.int(kind),
		width:    C.uint32_t(width),
		height:   C.uint32_t(height),
		data:     (*C.uint8_t)(samples),
		data_len: C.size_t(len(data)),
	}
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		return C.tpdf_builder_add_image(b.ptr, r, rl, &image)
	})
}

// SetInfo sets an /Info entry such as Title.
func (b *Builder) SetInfo(key []byte, value string) error {
	cvalue := cString(value)
	defer C.free(unsafe.Pointer(cvalue))
	return call(func() C.enum_TpdfStatus {
		k, kl := bytesArg(key)
		return C.tpdf_builder_set_info(b.ptr, k, kl, cvalue)
	})
}

// SetOutline sets the outline from top-level entries, consuming each.
func (b *Builder) SetOutline(entries ...*OutlineEntry) error {
	raw := entryArray(entries)
	return call(func() C.enum_TpdfStatus {
		if len(raw) == 0 {
			return C.tpdf_builder_set_outline(b.ptr, nil, 0)
		}
		return C.tpdf_builder_set_outline(b.ptr, &raw[0], C.size_t(len(raw)))
	})
}

// PageBuilder is a page being drawn, owned until it is pushed.
type PageBuilder struct {
	ptr *C.TpdfPageBuilder
}

// BeginPage starts a page. The resource snapshot happens here.
func (b *Builder) BeginPage(width, height float64) (*PageBuilder, error) {
	var out *C.TpdfPageBuilder
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_builder_begin_page(b.ptr, C.double(width), C.double(height), &out)
	}); err != nil {
		return nil, err
	}
	return &PageBuilder{ptr: out}, nil
}

// PushPage adds a finished page, consuming it.
func (b *Builder) PushPage(page *PageBuilder) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_builder_push_page(b.ptr, page.ptr) })
}

// Finish finishes the document and returns its bytes, consuming the builder.
func (b *Builder) Finish() ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_builder_finish(b.ptr, &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}

// Close releases the page; one never pushed leaves no trace.
func (p *PageBuilder) Close() {
	if p != nil && p.ptr != nil {
		C.tpdf_page_builder_free(p.ptr)
		p.ptr = nil
	}
}

// Text draws text with a registered font.
func (p *PageBuilder) Text(font []byte, size, x, y float64, text string) error {
	ctext := cString(text)
	defer C.free(unsafe.Pointer(ctext))
	return call(func() C.enum_TpdfStatus {
		f, fl := bytesArg(font)
		return C.tpdf_page_builder_text(p.ptr, f, fl, C.double(size), C.double(x), C.double(y), ctext)
	})
}

// FillRect fills a rectangle in device grey.
func (p *PageBuilder) FillRect(x, y, w, h, grey float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_page_builder_fill_rect(p.ptr, C.double(x), C.double(y), C.double(w), C.double(h), C.double(grey))
	})
}

// Image draws a registered image into a rectangle.
func (p *PageBuilder) Image(resource []byte, x, y, w, h float64) error {
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		return C.tpdf_page_builder_image(p.ptr, r, rl, C.double(x), C.double(y), C.double(w), C.double(h))
	})
}

// Link adds a link annotation over a rectangle (12.5.6.5).
func (p *PageBuilder) Link(x0, y0, x1, y1 float64, target Target) error {
	return call(func() C.enum_TpdfStatus {
		return withTarget(target, func(raw *C.TpdfTarget) C.enum_TpdfStatus {
			return C.tpdf_page_builder_link(p.ptr, C.double(x0), C.double(y0), C.double(x1), C.double(y1), raw)
		})
	})
}
