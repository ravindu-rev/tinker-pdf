package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import "unsafe"

// TagText is which text property SetText sets.
type TagText int

// The properties, transcribed from TpdfTagText.
const (
	TagTitle      TagText = 0
	TagLang       TagText = 1
	TagAlt        TagText = 2
	TagActualText TagText = 3
	TagExpansion  TagText = 4
)

// Tag is a structure element to open (14.7.2): its type and properties.
type Tag struct {
	ptr *C.TpdfTag
}

// NewTag is an element of structure type kind.
func NewTag(kind []byte) (*Tag, error) {
	var out *C.TpdfTag
	if err := call(func() C.enum_TpdfStatus {
		k, kl := bytesArg(kind)
		return C.tpdf_tag_new(k, kl, &out)
	}); err != nil {
		return nil, err
	}
	return &Tag{ptr: out}, nil
}

// Close releases the tag; an element it opened stays open.
func (t *Tag) Close() {
	if t != nil && t.ptr != nil {
		C.tpdf_tag_free(t.ptr)
		t.ptr = nil
	}
}

// SetText sets /T, /Lang, /Alt, /ActualText or /E.
func (t *Tag) SetText(which TagText, text string) error {
	ctext := cString(text)
	defer C.free(unsafe.Pointer(ctext))
	return call(func() C.enum_TpdfStatus { return C.tpdf_tag_set_text(t.ptr, C.int(which), ctext) })
}

// SetID sets /ID.
func (t *Tag) SetID(id []byte) error {
	return call(func() C.enum_TpdfStatus {
		i, il := bytesArg(id)
		return C.tpdf_tag_set_id(t.ptr, i, il)
	})
}

// SetKey names the element so halves drawn apart are one element.
func (t *Tag) SetKey(key, order uint64) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_tag_set_key(t.ptr, C.uint64_t(key), C.uint64_t(order)) })
}

// KeepEmpty writes the element even with nothing drawn inside it.
func (t *Tag) KeepEmpty() error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_tag_keep_empty(t.ptr) })
}

// OpenTag opens the element tag describes, until the matching CloseTag.
func (p *PageBuilder) OpenTag(tag *Tag) error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_page_builder_open_tag(p.ptr, tag.ptr) })
}

// CloseTag closes the innermost element OpenTag opened.
func (p *PageBuilder) CloseTag() error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_page_builder_close_tag(p.ptr) })
}

// SetLanguage sets the catalog's /Lang.
func (b *Builder) SetLanguage(language string) error {
	clanguage := cString(language)
	defer C.free(unsafe.Pointer(clanguage))
	return call(func() C.enum_TpdfStatus { return C.tpdf_builder_set_language(b.ptr, clanguage) })
}

// MapRole maps a structure type of the caller's own to a standard one.
func (b *Builder) MapRole(custom, standard []byte) error {
	return call(func() C.enum_TpdfStatus {
		c, cl := bytesArg(custom)
		s, sl := bytesArg(standard)
		return C.tpdf_builder_map_role(b.ptr, c, cl, s, sl)
	})
}
