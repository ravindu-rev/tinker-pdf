package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import "unsafe"

// LabelStyle is how a page-label range writes its number (12.4.2).
type LabelStyle int

// The styles, transcribed from TpdfLabelStyle.
const (
	LabelDecimal      LabelStyle = 0
	LabelRomanUpper   LabelStyle = 1
	LabelRomanLower   LabelStyle = 2
	LabelLettersUpper LabelStyle = 3
	LabelLettersLower LabelStyle = 4
	LabelNone         LabelStyle = 5
)

// PageLabelRange is one run of page labels; a nil Prefix writes no /P.
type PageLabelRange struct {
	FirstPage uint32
	Style     LabelStyle
	Prefix    *string
	Start     uint32
}

// Date is a date (7.9.4); a nil offset is an unspecified zone.
type Date struct {
	Year, Month, Day, Hour, Minute, Second int32
	UTCOffsetMinutes                       *int32
}

func (d Date) raw() C.TpdfDate {
	raw := C.TpdfDate{
		year: C.int32_t(d.Year), month: C.int32_t(d.Month), day: C.int32_t(d.Day),
		hour: C.int32_t(d.Hour), minute: C.int32_t(d.Minute), second: C.int32_t(d.Second),
	}
	if d.UTCOffsetMinutes != nil {
		raw.has_utc_offset = 1
		raw.utc_offset_minutes = C.int32_t(*d.UTCOffsetMinutes)
	}
	return raw
}

// EmbeddedFile is a file to embed (7.11.4).
type EmbeddedFile struct {
	Name, Filename        string
	Description, MimeType *string
	Created, Modified     *Date
	Data                  []byte
}

// MetadataSync is what a metadata write did to the other statement of the
// same metadata.
type MetadataSync int

// The answers, transcribed from TpdfMetadataSync.
const (
	SyncAlone              MetadataSync = 0
	SyncOtherHalfUnchanged MetadataSync = 1
)

// PageBoundary is one of a page's five boundaries (14.11.2).
type PageBoundary int

// The boundaries, transcribed from TpdfPageBoundary.
const (
	MediaBox PageBoundary = 0
	CropBox  PageBoundary = 1
	BleedBox PageBoundary = 2
	TrimBox  PageBoundary = 3
	ArtBox   PageBoundary = 4
)

// Removal is why a sanitise removed something.
type Removal int

// The reasons, transcribed from TpdfRemoval.
const (
	RemovedJavaScript         Removal = 0
	RemovedDocumentJavaScript Removal = 1
	RemovedCalculationOrder   Removal = 2
	RemovedXfaForm            Removal = 3
	RemovedAction             Removal = 4
	RemovedEmbeddedFileTree   Removal = 5
	RemovedEmbeddedFile       Removal = 6
	RemovedInfo               Removal = 7
	RemovedMetadata           Removal = 8
)

// Sanitise is what a sanitise takes out; all four is Sanitise::ALL.
type Sanitise struct {
	JavaScript, Actions, EmbeddedFiles, Metadata bool
}

// PathStep is one step of a removed entry's path: a dictionary key, or an
// array position counted in the array as it was.
type PathStep struct {
	Key     []byte
	IsIndex bool
	Index   uint64
}

// RemovedEntry is one entry removed from an object that stays, or from the
// trailer (a nil Holder).
type RemovedEntry struct {
	Holder *Ref
	Path   []PathStep
	What   Removal
	Action []byte
}

// DeletedObject is one object deleted because only removed entries reached it.
type DeletedObject struct {
	Object Ref
	What   Removal
	Action []byte
}

// SanitiseReport is everything a sanitise took out.
type SanitiseReport struct {
	Removed []RemovedEntry
	Deleted []DeletedObject
}

func flag(on bool) C.int {
	if on {
		return 1
	}
	return 0
}

// SetPageLabels sets the page labels (12.4.2), replacing any.
func (e *Editor) SetPageLabels(ranges []PageLabelRange) error {
	raw := make([]C.TpdfPageLabelRange, len(ranges))
	for i, r := range ranges {
		raw[i] = C.TpdfPageLabelRange{
			first_page: C.uint32_t(r.FirstPage),
			style:      C.enum_TpdfLabelStyle(r.Style),
			start:      C.uint32_t(r.Start),
		}
		if r.Prefix != nil {
			prefix := cString(*r.Prefix)
			defer C.free(unsafe.Pointer(prefix))
			raw[i].prefix = prefix
		}
	}
	return call(func() C.enum_TpdfStatus {
		if len(raw) == 0 {
			return C.tpdf_editor_set_page_labels(e.ptr, nil, 0)
		}
		return C.tpdf_editor_set_page_labels(e.ptr, &raw[0], C.size_t(len(raw)))
	})
}

// AttachFile embeds a file and returns its file specification's reference.
func (e *Editor) AttachFile(file EmbeddedFile) (Ref, error) {
	var allocated []unsafe.Pointer
	defer func() {
		for _, p := range allocated {
			C.free(p)
		}
	}()
	str := func(s string) *C.char {
		p := cString(s)
		allocated = append(allocated, unsafe.Pointer(p))
		return p
	}
	raw := C.TpdfEmbeddedFile{name: str(file.Name), filename: str(file.Filename)}
	if file.Description != nil {
		raw.description = str(*file.Description)
	}
	if file.MimeType != nil {
		raw.mime_type = str(*file.MimeType)
	}
	if file.Created != nil {
		date := (*C.TpdfDate)(C.malloc(C.size_t(unsafe.Sizeof(C.TpdfDate{}))))
		allocated = append(allocated, unsafe.Pointer(date))
		*date = file.Created.raw()
		raw.created = date
	}
	if file.Modified != nil {
		date := (*C.TpdfDate)(C.malloc(C.size_t(unsafe.Sizeof(C.TpdfDate{}))))
		allocated = append(allocated, unsafe.Pointer(date))
		*date = file.Modified.raw()
		raw.modified = date
	}
	if len(file.Data) > 0 {
		data := C.CBytes(file.Data)
		allocated = append(allocated, data)
		raw.data = (*C.uint8_t)(data)
		raw.data_len = C.size_t(len(file.Data))
	}
	var number C.uint32_t
	var generation C.uint16_t
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_attach_file(e.ptr, &raw, &number, &generation)
	})
	return Ref{Object: uint32(number), Generation: uint16(generation)}, err
}

// SetOutline replaces the outline, consuming each entry.
func (e *Editor) SetOutline(entries ...*OutlineEntry) error {
	raw := entryArray(entries)
	return call(func() C.enum_TpdfStatus {
		if len(raw) == 0 {
			return C.tpdf_editor_set_outline(e.ptr, nil, 0)
		}
		return C.tpdf_editor_set_outline(e.ptr, &raw[0], C.size_t(len(raw)))
	})
}

// SetInfo sets an /Info text entry; the date keys are SetInfoDate's.
func (e *Editor) SetInfo(key InfoKey, value string) (MetadataSync, error) {
	cvalue := cString(value)
	defer C.free(unsafe.Pointer(cvalue))
	var sync C.enum_TpdfMetadataSync
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_set_info(e.ptr, C.enum_TpdfInfoKey(key), cvalue, &sync)
	})
	return MetadataSync(sync), err
}

// SetInfoDate sets /CreationDate or /ModDate.
func (e *Editor) SetInfoDate(key InfoKey, date Date) (MetadataSync, error) {
	raw := date.raw()
	var sync C.enum_TpdfMetadataSync
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_set_info_date(e.ptr, C.enum_TpdfInfoKey(key), &raw, &sync)
	})
	return MetadataSync(sync), err
}

// SetTrapped sets /Info /Trapped; TrappedAbsent is refused.
func (e *Editor) SetTrapped(trapped Trapped) (MetadataSync, error) {
	var sync C.enum_TpdfMetadataSync
	err := call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_set_trapped(e.ptr, C.enum_TpdfTrapped(trapped), &sync)
	})
	return MetadataSync(sync), err
}

// SetXMPMetadata makes packet the XMP metadata (14.3.2), verbatim.
func (e *Editor) SetXMPMetadata(packet []byte) (MetadataSync, error) {
	var sync C.enum_TpdfMetadataSync
	err := call(func() C.enum_TpdfStatus {
		data, length := bytesArg(packet)
		return C.tpdf_editor_set_xmp_metadata(e.ptr, data, length, &sync)
	})
	return MetadataSync(sync), err
}

// SetPageBoundary sets one of a page's boundaries (14.11.2) — and with it
// set_bleed_box, set_trim_box and set_art_box, which are this call with the
// boundary named.
func (e *Editor) SetPageBoundary(index uint32, boundary PageBoundary, x0, y0, x1, y1 float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_editor_set_page_boundary(e.ptr, C.uint32_t(index), C.enum_TpdfPageBoundary(boundary),
			C.double(x0), C.double(y0), C.double(x1), C.double(y1))
	})
}

// PageBox is one of a page's boundaries, resolved the way the reader
// resolves an absent one.
func (d *Document) PageBox(index uint32, boundary PageBoundary) (x0, y0, x1, y1 float64, err error) {
	var a, b, c, e C.double
	err = call(func() C.enum_TpdfStatus {
		return C.tpdf_page_boundary(d.ptr, C.uint32_t(index), C.enum_TpdfPageBoundary(boundary), &a, &b, &c, &e)
	})
	return float64(a), float64(b), float64(c), float64(e), err
}

// Sanitise takes out what what names and reports every change it made.
func (e *Editor) Sanitise(what Sanitise) (*SanitiseReport, error) {
	raw := C.TpdfSanitise{
		javascript:     flag(what.JavaScript),
		actions:        flag(what.Actions),
		embedded_files: flag(what.EmbeddedFiles),
		metadata:       flag(what.Metadata),
	}
	var report *C.TpdfSanitiseReport
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_editor_sanitise(e.ptr, &raw, &report) }); err != nil {
		return nil, err
	}
	defer C.tpdf_sanitise_report_free(report)

	out := &SanitiseReport{}
	for list := 0; list < 2; list++ {
		which := C.enum_TpdfSanitiseList(list)
		count := uint32(C.tpdf_sanitise_report_count(report, which))
		for i := uint32(0); i < count; i++ {
			index := C.uint32_t(i)
			var what C.enum_TpdfRemoval
			var has C.int
			var number C.uint32_t
			var generation C.uint16_t
			var data *C.uint8_t
			var length C.size_t
			if err := call(func() C.enum_TpdfStatus {
				return C.tpdf_sanitise_report_entry(report, which, index, &what, &has, &number, &generation)
			}); err != nil {
				return nil, err
			}
			if err := call(func() C.enum_TpdfStatus {
				return C.tpdf_sanitise_report_action(report, which, index, &data, &length)
			}); err != nil {
				return nil, err
			}
			ref := Ref{Object: uint32(number), Generation: uint16(generation)}
			action := borrowed(data, length)
			if list == 1 {
				out.Deleted = append(out.Deleted, DeletedObject{Object: ref, What: Removal(what), Action: action})
				continue
			}
			entry := RemovedEntry{What: Removal(what), Action: action}
			if has != 0 {
				entry.Holder = &ref
			}
			steps := uint32(C.tpdf_sanitise_report_path_count(report, index))
			for s := uint32(0); s < steps; s++ {
				var isIndex C.int
				var position C.uint64_t
				var key *C.uint8_t
				var keyLength C.size_t
				if err := call(func() C.enum_TpdfStatus {
					return C.tpdf_sanitise_report_path_step(report, index, C.uint32_t(s), &isIndex, &position, &key, &keyLength)
				}); err != nil {
					return nil, err
				}
				entry.Path = append(entry.Path, PathStep{Key: borrowed(key, keyLength), IsIndex: isIndex != 0, Index: uint64(position)})
			}
			out.Removed = append(out.Removed, entry)
		}
	}
	return out, nil
}
