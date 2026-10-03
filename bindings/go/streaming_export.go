package tinkerpdf

// The Go side of TpdfSourceVtable. A file with //export may only declare in
// its preamble, never define, which is why the vtable is built in
// streaming.go and these live here.

/*
#include <stdint.h>
*/
import "C"

import (
	"runtime/cgo"
	"unsafe"
)

func sourceOf(ctx unsafe.Pointer) Source {
	return cgo.Handle(uintptr(ctx)).Value().(Source)
}

//export tinkerGoSourceLen
func tinkerGoSourceLen(ctx unsafe.Pointer) C.uint64_t {
	return C.uint64_t(sourceOf(ctx).Len())
}

//export tinkerGoSourceRead
func tinkerGoSourceRead(ctx unsafe.Pointer, offset C.uint64_t, out *C.uint8_t, capacity C.uint64_t) C.int64_t {
	return C.int64_t(sourceOf(ctx).ReadAt(uint64(offset), unsafe.Slice((*byte)(unsafe.Pointer(out)), int(capacity))))
}

//export tinkerGoSourceFree
func tinkerGoSourceFree(ctx unsafe.Pointer) {
	cgo.Handle(uintptr(ctx)).Delete()
}
