package tinkerpdf

/*
#include <stdint.h>
#include "tinker_pdf.h"

// The three callbacks are the exported Go functions in streaming_export.go.
// The vtable is built here, in C, because it crosses by value and its
// members are C function pointers, which Go cannot name.
uint64_t tinkerGoSourceLen(void *ctx);
int64_t tinkerGoSourceRead(void *ctx, uint64_t offset, uint8_t *out, uint64_t capacity);
void tinkerGoSourceFree(void *ctx);

static enum TpdfStatus tinker_open_streaming(uintptr_t handle, TpdfDocument **out) {
	TpdfSourceVtable vtable = { tinkerGoSourceLen, tinkerGoSourceRead, tinkerGoSourceFree };
	return tpdf_document_open_streaming(vtable, (void *)handle, out);
}
*/
import "C"

import "runtime/cgo"

// Source supplies a document's bytes on demand (TpdfSourceVtable).
//
// The engine may call it from whatever thread is working, and from several at
// once, so an implementation must be safe for concurrent use.
type Source interface {
	// Len is the document's total length, fixed for the life of the source.
	Len() uint64
	// ReadAt writes up to len(out) bytes from offset and returns how many it
	// wrote, or a negative number to say the range is not here: the open or
	// the read that needed it fails with StatusSourceMiss, naming the range,
	// and the caller fetches it and asks again.
	ReadAt(offset uint64, out []byte) int64
}

// OpenStreaming opens a document whose bytes come from source. The source is
// released when the document is closed, or at once if the open fails.
func OpenStreaming(source Source) (*Document, error) {
	handle := cgo.NewHandle(source)
	var out *C.TpdfDocument
	if err := call(func() C.enum_TpdfStatus {
		return C.tinker_open_streaming(C.uintptr_t(handle), &out)
	}); err != nil {
		return nil, err
	}
	return &Document{ptr: out}, nil
}
