package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

// InfoKey is which /Info entry to read or write (14.3.3, Table 349).
type InfoKey int

// The keys, transcribed from TpdfInfoKey.
const (
	InfoTitle            InfoKey = 0
	InfoAuthor           InfoKey = 1
	InfoSubject          InfoKey = 2
	InfoKeywords         InfoKey = 3
	InfoCreator          InfoKey = 4
	InfoProducer         InfoKey = 5
	InfoCreationDate     InfoKey = 6
	InfoModificationDate InfoKey = 7
)

// Trapped is /Trapped (Table 349) with its absence spelled out.
type Trapped int

// The values, transcribed from TpdfTrapped.
const (
	TrappedAbsent  Trapped = 0
	TrappedTrue    Trapped = 1
	TrappedFalse   Trapped = 2
	TrappedUnknown Trapped = 3
)

// DestinationKind is which of a destination's three arms (12.3.2); they are
// never collapsed (ruling 6).
type DestinationKind int

// The kinds, transcribed from TpdfDestinationKind.
const (
	DestinationAbsent   DestinationKind = 0
	DestinationExplicit DestinationKind = 1
	DestinationNamed    DestinationKind = 2
	DestinationURI      DestinationKind = 3
)

// ActionKind is which action a link carries (12.6.4).
type ActionKind int

// The kinds, transcribed from TpdfActionKind.
const (
	ActionAbsent ActionKind = 0
	ActionGoTo   ActionKind = 1
	ActionGoToR  ActionKind = 2
	ActionURI    ActionKind = 3
	ActionNamed  ActionKind = 4
	ActionLaunch ActionKind = 5
	ActionOther  ActionKind = 6
)

// Ref is an object reference.
type Ref struct {
	Object     uint32
	Generation uint16
}

// Destination is where an outline entry or a link goes; nil is none.
type Destination struct {
	Kind      DestinationKind
	PageIndex *uint32
	PageRef   *Ref
	View      View
	// Bytes is a named destination's name or a URI destination's URI.
	Bytes []byte
}

func destinationOf(raw C.TpdfDestinationRead, bytes []byte) *Destination {
	kind := DestinationKind(raw.kind)
	if kind == DestinationAbsent {
		return nil
	}
	d := &Destination{Kind: kind, View: viewOf(raw.view), Bytes: bytes}
	if raw.has_page_index != 0 {
		index := uint32(raw.page_index)
		d.PageIndex = &index
	}
	if raw.has_page_ref != 0 {
		d.PageRef = &Ref{Object: uint32(raw.page_object), Generation: uint16(raw.page_generation)}
	}
	return d
}

// Info is one /Info text entry; nil when absent, "" when empty.
func (d *Document) Info(key InfoKey) (*string, error) {
	var out *C.char
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_document_info(d.ptr, C.int(key), &out)
	})
	if err != nil {
		return nil, err
	}
	return takeString(out), nil
}

// Trapped is /Info /Trapped.
func (d *Document) Trapped() (Trapped, error) {
	var out C.enum_TpdfTrapped
	err := call(func() C.enum_TpdfStatus { return C.tpdf_document_trapped(d.ptr, &out) })
	return Trapped(out), err
}

// PDFVersion is the version, as "PDF 1.7", never absent.
func (d *Document) PDFVersion() (string, error) {
	var out *C.char
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_pdf_version(d.ptr, &out) }); err != nil {
		return "", err
	}
	if text := takeString(out); text != nil {
		return *text, nil
	}
	return "", nil
}

// PageLabel is one page's label (12.4.2); nil when the document has none.
func (d *Document) PageLabel(index uint32) (*string, error) {
	var out *C.char
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_document_page_label(d.ptr, C.uint32_t(index), &out)
	})
	if err != nil {
		return nil, err
	}
	return takeString(out), nil
}

// XMPMetadata is the XMP packet (14.3.2), unparsed; nil when there is none.
func (d *Document) XMPMetadata() ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_xmp_metadata(d.ptr, &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}

// OutlineItem is one outline entry, flattened: its depth is 0 for a top-level
// entry, and the nesting is the entries that follow at a greater depth.
type OutlineItem struct {
	Depth       uint32
	Open        bool
	Title       string
	Destination *Destination
}

// Outline is the outline flattened to reading order; empty when none.
func (d *Document) Outline() ([]OutlineItem, error) {
	var outline *C.TpdfOutline
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_outline(d.ptr, &outline) }); err != nil {
		return nil, err
	}
	defer C.tpdf_outline_free(outline)
	count := uint32(C.tpdf_outline_count(outline))
	items := make([]OutlineItem, 0, count)
	for i := uint32(0); i < count; i++ {
		index := C.uint32_t(i)
		var depth C.uint32_t
		var open C.int
		var title *C.char
		var raw C.TpdfDestinationRead
		var data *C.uint8_t
		var length C.size_t
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_outline_item(outline, index, &depth, &open) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_outline_title(outline, index, &title) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_outline_destination(outline, index, &raw) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_outline_destination_bytes(outline, index, &data, &length)
		}); err != nil {
			return nil, err
		}
		item := OutlineItem{Depth: uint32(depth), Open: open != 0, Destination: destinationOf(raw, borrowed(data, length))}
		if text := takeString(title); text != nil {
			item.Title = *text
		}
		items = append(items, item)
	}
	return items, nil
}

// Link is one link annotation (12.5.6.5).
type Link struct {
	X0, Y0, X1, Y1 float64
	Reference      *Ref
	Action         ActionKind
	Destination    *Destination
	// ActionBytes is a /URI's URI, a /Named's name, another type's /S, or a
	// /GoToR's or /Launch's file.
	ActionBytes []byte
}

// Links is a page's link annotations, in /Annots order.
func (d *Document) Links(page uint32) ([]Link, error) {
	var links *C.TpdfLinks
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_page_links(d.ptr, C.uint32_t(page), &links) }); err != nil {
		return nil, err
	}
	defer C.tpdf_links_free(links)
	count := uint32(C.tpdf_links_count(links))
	found := make([]Link, 0, count)
	for i := uint32(0); i < count; i++ {
		index := C.uint32_t(i)
		var x0, y0, x1, y1 C.double
		var present C.int
		var number C.uint32_t
		var generation C.uint16_t
		var kind C.enum_TpdfActionKind
		var raw C.TpdfDestinationRead
		var actionData, destData *C.uint8_t
		var actionLen, destLen C.size_t
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_link_rect(links, index, &x0, &y0, &x1, &y1) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_link_reference(links, index, &present, &number, &generation)
		}); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_link_action(links, index, &kind, &raw) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_link_action_bytes(links, index, &actionData, &actionLen)
		}); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_link_destination_bytes(links, index, &destData, &destLen)
		}); err != nil {
			return nil, err
		}
		link := Link{
			X0: float64(x0), Y0: float64(y0), X1: float64(x1), Y1: float64(y1),
			Action:      ActionKind(kind),
			Destination: destinationOf(raw, borrowed(destData, destLen)),
			ActionBytes: borrowed(actionData, actionLen),
		}
		if present != 0 {
			link.Reference = &Ref{Object: uint32(number), Generation: uint16(generation)}
		}
		found = append(found, link)
	}
	return found, nil
}

// Attachments is every file attached to a document (7.11.4). Listing reads
// no bytes; Data does. It holds its own document, so it outlives this one.
type Attachments struct {
	ptr *C.TpdfAttachments
}

// Attachments lists the document's attachments.
func (d *Document) Attachments() (*Attachments, error) {
	var out *C.TpdfAttachments
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_attachments(d.ptr, &out) }); err != nil {
		return nil, err
	}
	return &Attachments{ptr: out}, nil
}

// Close releases the list.
func (a *Attachments) Close() {
	if a != nil && a.ptr != nil {
		C.tpdf_attachments_free(a.ptr)
		a.ptr = nil
	}
}

// Count is how many attachments.
func (a *Attachments) Count() uint32 { return uint32(C.tpdf_attachments_count(a.ptr)) }

func (a *Attachments) text(index uint32, f func(*C.TpdfAttachments, C.uint32_t, **C.char) C.enum_TpdfStatus) (*string, error) {
	var out *C.char
	if err := call(func() C.enum_TpdfStatus { return f(a.ptr, C.uint32_t(index), &out) }); err != nil {
		return nil, err
	}
	return takeString(out), nil
}

// Name is the name it is filed under.
func (a *Attachments) Name(index uint32) (*string, error) {
	return a.text(index, func(p *C.TpdfAttachments, i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_attachment_name(p, i, out)
	})
}

// Filename is /UF or /F.
func (a *Attachments) Filename(index uint32) (*string, error) {
	return a.text(index, func(p *C.TpdfAttachments, i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_attachment_filename(p, i, out)
	})
}

// Description is /Desc; nil when absent.
func (a *Attachments) Description(index uint32) (*string, error) {
	return a.text(index, func(p *C.TpdfAttachments, i C.uint32_t, out **C.char) C.enum_TpdfStatus {
		return C.tpdf_attachment_description(p, i, out)
	})
}

// Size is /Params /Size; nil when not declared. Advisory.
func (a *Attachments) Size(index uint32) (*int64, error) {
	var present C.int
	var size C.int64_t
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_attachment_size(a.ptr, C.uint32_t(index), &present, &size)
	}); err != nil {
		return nil, err
	}
	if present == 0 {
		return nil, nil
	}
	value := int64(size)
	return &value, nil
}

// Data is the file's bytes, decoded; nil when no stream is named. A stream
// that is named and unreadable is StatusStreamUnreadable.
func (a *Attachments) Data(index uint32) ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_attachment_data(a.ptr, C.uint32_t(index), &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}

// Warning is one thing the engine tolerated (ruling 10).
type Warning struct {
	Offset  uint64
	Object  *Ref
	Kind    string
	Message string
}

// Warnings is everything tolerated so far, in order. Reading a page can
// tolerate more, so asking again later may answer with more.
func (d *Document) Warnings() ([]Warning, error) {
	var warnings *C.TpdfWarnings
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_warnings(d.ptr, &warnings) }); err != nil {
		return nil, err
	}
	defer C.tpdf_warnings_free(warnings)
	count := uint32(C.tpdf_warnings_count(warnings))
	found := make([]Warning, 0, count)
	for i := uint32(0); i < count; i++ {
		index := C.uint32_t(i)
		var offset C.uint64_t
		var has C.int
		var number C.uint32_t
		var generation C.uint16_t
		var kind, message *C.char
		if err := call(func() C.enum_TpdfStatus {
			return C.tpdf_warning_location(warnings, index, &offset, &has, &number, &generation)
		}); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_warning_kind(warnings, index, &kind) }); err != nil {
			return nil, err
		}
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_warning_message(warnings, index, &message) }); err != nil {
			return nil, err
		}
		warning := Warning{Offset: uint64(offset)}
		if has != 0 {
			warning.Object = &Ref{Object: uint32(number), Generation: uint16(generation)}
		}
		if text := takeString(kind); text != nil {
			warning.Kind = *text
		}
		if text := takeString(message); text != nil {
			warning.Message = *text
		}
		found = append(found, warning)
	}
	return found, nil
}
