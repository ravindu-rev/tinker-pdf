package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import "unsafe"

// BlendMode is one of 11.3.5's sixteen blend modes.
type BlendMode int

// The blend modes, transcribed from TpdfBlendMode.
const (
	BlendNormal     BlendMode = 0
	BlendMultiply   BlendMode = 1
	BlendScreen     BlendMode = 2
	BlendOverlay    BlendMode = 3
	BlendDarken     BlendMode = 4
	BlendLighten    BlendMode = 5
	BlendColorDodge BlendMode = 6
	BlendColorBurn  BlendMode = 7
	BlendHardLight  BlendMode = 8
	BlendSoftLight  BlendMode = 9
	BlendDifference BlendMode = 10
	BlendExclusion  BlendMode = 11
	BlendHue        BlendMode = 12
	BlendSaturation BlendMode = 13
	BlendColor      BlendMode = 14
	BlendLuminosity BlendMode = 15
)

// SoftMask is which /SMask a graphics state writes.
type SoftMask int

// The soft masks, transcribed from TpdfSoftMask.
const (
	SoftMaskAbsent SoftMask = 0
	SoftMaskNone   SoftMask = 1
	SoftMaskGroup  SoftMask = 2
)

// MaskKind is what a group soft mask derives its alpha from.
type MaskKind int

// The mask kinds, transcribed from TpdfMaskKind.
const (
	MaskAlpha      MaskKind = 0
	MaskLuminosity MaskKind = 1
)

// DeviceSpace is a device colour space.
type DeviceSpace int

// The device spaces, transcribed from TpdfDeviceSpace.
const (
	DeviceGray DeviceSpace = 0
	DeviceRgb  DeviceSpace = 1
	DeviceCmyk DeviceSpace = 2
)

// TilingType is a tiling pattern's /TilingType, counted from zero.
type TilingType int

// The tiling types, transcribed from TpdfTilingType.
const (
	TilingConstantSpacing TilingType = 0
	TilingNoDistortion    TilingType = 1
	TilingFasterTiling    TilingType = 2
)

// ExtGState is a graphics state's overrides (Table 58); a nil field writes
// no entry, and the zero value writes none at all.
type ExtGState struct {
	FillAlpha, StrokeAlpha *float64
	BlendMode              *BlendMode
	SoftMask               SoftMask
	MaskKind               MaskKind
	MaskForm               []byte
	Backdrop               []float64
}

// TransparencyGroup is a form's /Group (11.6.6).
type TransparencyGroup struct {
	ColorSpace         DeviceSpace
	Isolated, Knockout bool
}

// Matrix is a /Matrix's six numbers.
type Matrix [6]float64

// matrixArg points at six doubles for one call, or is nil for the identity.
func matrixArg(matrix *Matrix) (*C.double, *[6]C.double) {
	if matrix == nil {
		return nil, nil
	}
	var raw [6]C.double
	for i, v := range matrix {
		raw[i] = C.double(v)
	}
	return &raw[0], &raw
}

// NewBuilderWithVersion starts a document whose header declares PDF
// major.minor (7.5.2).
func NewBuilderWithVersion(major, minor uint32) (*Builder, error) {
	var out *C.TpdfBuilder
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_builder_new_with_version(C.uint32_t(major), C.uint32_t(minor), &out)
	}); err != nil {
		return nil, err
	}
	return &Builder{ptr: out}, nil
}

// ClearImageResources stops later pages inheriting the images registered so far.
func (b *Builder) ClearImageResources() error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_builder_clear_image_resources(b.ptr) })
}

// AddNamedFont registers one of the standard 14 under an /Encoding of glyph
// names from firstCode, with their widths.
func (b *Builder) AddNamedFont(resource, baseFont []byte, firstCode uint32, names []string, widths []uint16) error {
	array, free := cStrings(names)
	defer free()
	var rawWidths *C.uint16_t
	if len(widths) > 0 {
		w := make([]C.uint16_t, len(widths))
		for i, v := range widths {
			w[i] = C.uint16_t(v)
		}
		rawWidths = &w[0]
	}
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		f, fl := bytesArg(baseFont)
		return C.tpdf_builder_add_named_font(b.ptr, r, rl, f, fl, C.uint32_t(firstCode), array,
			C.size_t(len(names)), rawWidths, C.size_t(len(widths)))
	})
}

// AddExtGState registers a graphics state under a resource name, starting
// from tpdf_ext_gstate_init.
func (b *Builder) AddExtGState(resource []byte, state ExtGState) error {
	var raw C.TpdfExtGState
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_ext_gstate_init(&raw) }); err != nil {
		return err
	}
	if state.FillAlpha != nil {
		raw.fill_alpha = C.double(*state.FillAlpha)
	}
	if state.StrokeAlpha != nil {
		raw.stroke_alpha = C.double(*state.StrokeAlpha)
	}
	if state.BlendMode != nil {
		raw.has_blend_mode = 1
		raw.blend_mode = C.enum_TpdfBlendMode(*state.BlendMode)
	}
	raw.soft_mask = C.enum_TpdfSoftMask(state.SoftMask)
	raw.mask_kind = C.enum_TpdfMaskKind(state.MaskKind)
	if state.MaskForm != nil {
		form := C.CBytes(state.MaskForm)
		defer C.free(form)
		raw.mask_form = (*C.uint8_t)(form)
		raw.mask_form_len = C.size_t(len(state.MaskForm))
	}
	if state.Backdrop != nil {
		backdrop := (*[1 << 20]C.double)(C.malloc(C.size_t(len(state.Backdrop)+1) * C.size_t(unsafe.Sizeof(C.double(0)))))
		defer C.free(unsafe.Pointer(&backdrop[0]))
		for i, v := range state.Backdrop {
			backdrop[i] = C.double(v)
		}
		raw.backdrop = &backdrop[0]
		raw.backdrop_len = C.size_t(len(state.Backdrop))
	}
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		return C.tpdf_builder_add_ext_gstate(b.ptr, r, rl, &raw)
	})
}

// AddForm registers a form XObject (8.10); a nil matrix is the identity and
// a nil group none.
func (b *Builder) AddForm(resource []byte, x0, y0, x1, y1 float64, matrix *Matrix, group *TransparencyGroup, content []byte) error {
	m, keep := matrixArg(matrix)
	var rawGroup *C.TpdfTransparencyGroup
	if group != nil {
		rawGroup = &C.TpdfTransparencyGroup{
			color_space: C.enum_TpdfDeviceSpace(group.ColorSpace),
			isolated:    C.int32_t(flag(group.Isolated)),
			knockout:    C.int32_t(flag(group.Knockout)),
		}
	}
	err := call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		c, cl := bytesArg(content)
		return C.tpdf_builder_add_form(b.ptr, r, rl, C.double(x0), C.double(y0), C.double(x1), C.double(y1),
			m, rawGroup, c, cl)
	})
	_ = keep
	return err
}

// AddTilingPattern registers a coloured tiling pattern (8.7.3); a nil matrix
// is the identity.
func (b *Builder) AddTilingPattern(resource []byte, x0, y0, x1, y1, xStep, yStep float64, matrix *Matrix, tiling TilingType, content []byte) error {
	m, keep := matrixArg(matrix)
	err := call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		c, cl := bytesArg(content)
		return C.tpdf_builder_add_tiling_pattern(b.ptr, r, rl, C.double(x0), C.double(y0), C.double(x1), C.double(y1),
			C.double(xStep), C.double(yStep), m, C.enum_TpdfTilingType(tiling), c, cl)
	})
	_ = keep
	return err
}

// SetBleedBox sets the page's /BleedBox (14.11.2).
func (p *PageBuilder) SetBleedBox(x0, y0, x1, y1 float64) error {
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_page_builder_set_bleed_box(p.ptr, C.double(x0), C.double(y0), C.double(x1), C.double(y1))
	})
}

// EncodedText writes codes the caller chose with a character and a word
// spacing; characters are what they stand for, recorded and not written.
func (p *PageBuilder) EncodedText(font []byte, size, x, y, characterSpacing, wordSpacing float64, codes []byte, characters string) error {
	ccharacters := cString(characters)
	defer C.free(unsafe.Pointer(ccharacters))
	return call(func() C.enum_TpdfStatus {
		f, fl := bytesArg(font)
		c, cl := bytesArg(codes)
		return C.tpdf_page_builder_encoded_text(p.ptr, f, fl, C.double(size), C.double(x), C.double(y),
			C.double(characterSpacing), C.double(wordSpacing), c, cl, ccharacters)
	})
}

// SetExtGState applies a registered graphics state (gs).
func (p *PageBuilder) SetExtGState(resource []byte) error {
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		return C.tpdf_page_builder_set_ext_gstate(p.ptr, r, rl)
	})
}

// Form draws a registered form XObject (Do).
func (p *PageBuilder) Form(resource []byte) error {
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		return C.tpdf_page_builder_form(p.ptr, r, rl)
	})
}

// SetFillPattern sets the non-stroking colour to a registered tiling pattern.
func (p *PageBuilder) SetFillPattern(resource []byte) error {
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		return C.tpdf_page_builder_set_fill_pattern(p.ptr, r, rl)
	})
}

// SetStrokePattern sets the stroking colour to a registered tiling pattern.
func (p *PageBuilder) SetStrokePattern(resource []byte) error {
	return call(func() C.enum_TpdfStatus {
		r, rl := bytesArg(resource)
		return C.tpdf_page_builder_set_stroke_pattern(p.ptr, r, rl)
	})
}
