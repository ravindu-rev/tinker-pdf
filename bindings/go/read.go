package tinkerpdf

/*
#include <stdlib.h>
#include "tinker_pdf.h"
*/
import "C"

import (
	"fmt"
	"math"
	"unsafe"
)

// InfoKey is which /Info entry to read or write (14.3.3, Table 349).
type InfoKey int

// The keys: the header's own TpdfInfoKey constants, so cgo checks each number.
const (
	InfoTitle            InfoKey = C.TPDF_INFO_KEY_TITLE
	InfoAuthor           InfoKey = C.TPDF_INFO_KEY_AUTHOR
	InfoSubject          InfoKey = C.TPDF_INFO_KEY_SUBJECT
	InfoKeywords         InfoKey = C.TPDF_INFO_KEY_KEYWORDS
	InfoCreator          InfoKey = C.TPDF_INFO_KEY_CREATOR
	InfoProducer         InfoKey = C.TPDF_INFO_KEY_PRODUCER
	InfoCreationDate     InfoKey = C.TPDF_INFO_KEY_CREATION_DATE
	InfoModificationDate InfoKey = C.TPDF_INFO_KEY_MODIFICATION_DATE
)

// Trapped is /Trapped (Table 349) with its absence spelled out.
type Trapped int

// The values: the header's own TpdfTrapped constants, so cgo checks each number.
const (
	TrappedAbsent  Trapped = C.TPDF_TRAPPED_ABSENT
	TrappedTrue    Trapped = C.TPDF_TRAPPED_TRUE
	TrappedFalse   Trapped = C.TPDF_TRAPPED_FALSE
	TrappedUnknown Trapped = C.TPDF_TRAPPED_UNKNOWN
)

// DestinationKind is which of a destination's three arms (12.3.2); they are
// never collapsed (ruling 6).
type DestinationKind int

// The kinds: the header's own TpdfDestinationKind constants, so cgo checks each number.
const (
	DestinationAbsent   DestinationKind = C.TPDF_DESTINATION_KIND_ABSENT
	DestinationExplicit DestinationKind = C.TPDF_DESTINATION_KIND_EXPLICIT
	DestinationNamed    DestinationKind = C.TPDF_DESTINATION_KIND_NAMED
	DestinationURI      DestinationKind = C.TPDF_DESTINATION_KIND_URI
)

// ActionKind is which action a link carries (12.6.4).
type ActionKind int

// The kinds: the header's own TpdfActionKind constants, so cgo checks each number.
const (
	ActionAbsent ActionKind = C.TPDF_ACTION_KIND_ABSENT
	ActionGoTo   ActionKind = C.TPDF_ACTION_KIND_GO_TO
	ActionGoToR  ActionKind = C.TPDF_ACTION_KIND_GO_TO_R
	ActionURI    ActionKind = C.TPDF_ACTION_KIND_URI
	ActionNamed  ActionKind = C.TPDF_ACTION_KIND_NAMED
	ActionLaunch ActionKind = C.TPDF_ACTION_KIND_LAUNCH
	ActionOther  ActionKind = C.TPDF_ACTION_KIND_OTHER
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

// PageLabels is every page's label (12.4.2), in page order, read with one
// walk; empty when the document defines none.
func (d *Document) PageLabels() ([]string, error) {
	var labels *C.TpdfPageLabels
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_document_page_labels(d.ptr, &labels) }); err != nil {
		return nil, err
	}
	defer C.tpdf_page_labels_free(labels)
	count := uint32(C.tpdf_page_labels_count(labels))
	found := make([]string, 0, count)
	for i := uint32(0); i < count; i++ {
		index := C.uint32_t(i)
		var out *C.char
		if err := call(func() C.enum_TpdfStatus { return C.tpdf_page_label_text(labels, index, &out) }); err != nil {
			return nil, err
		}
		label := ""
		if text := takeString(out); text != nil {
			label = *text
		}
		found = append(found, label)
	}
	return found, nil
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

// ---- the command stream (docs/design/command-stream.md) ---------------------
//
// Over crates/tinker-pdf-ffi's src/commands.rs, call for call: the cursor and
// a record read by its place, each field as plain values copied out of the
// stream's storage before its handle is closed, the two tables a host draws
// from, the encoder, the stream's own facts and its render. Replay is a loop
// of those reads, handing each record to the device method its kind names.
// cgo lays every struct out from the header; the fields an encoder pushes are
// written into C memory for the length of the call, because Go memory handed
// to C may not hold a pointer to other Go memory.

// CommandStream is a page's drawing as a command stream's versioned bytes,
// which ParseCommandStream reads.
func (d *Document) CommandStream(index uint32) ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_page_command_stream(d.ptr, C.uint32_t(index), &out)
	}); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}

// CommandKind is a command-stream record's kind, the header's TpdfCommandKind:
// numbered from zero in the order the kinds were added, which is not the
// number a kind has on the wire. A record of a kind this build does not know,
// which a stream of a newer minor version holds, reads as -1 (Known).
type CommandKind int

// The kinds: the header's own TpdfCommandKind constants, so cgo checks each number.
const (
	CommandEnd                CommandKind = C.TPDF_COMMAND_KIND_END
	CommandFunction           CommandKind = C.TPDF_COMMAND_KIND_FUNCTION
	CommandHalftone           CommandKind = C.TPDF_COMMAND_KIND_HALFTONE
	CommandStored             CommandKind = C.TPDF_COMMAND_KIND_STORED
	CommandColorants          CommandKind = C.TPDF_COMMAND_KIND_COLORANTS
	CommandSource             CommandKind = C.TPDF_COMMAND_KIND_SOURCE
	CommandState              CommandKind = C.TPDF_COMMAND_KIND_STATE
	CommandBeginText          CommandKind = C.TPDF_COMMAND_KIND_BEGIN_TEXT
	CommandGlyphRun           CommandKind = C.TPDF_COMMAND_KIND_GLYPH_RUN
	CommandEndText            CommandKind = C.TPDF_COMMAND_KIND_END_TEXT
	CommandFillPath           CommandKind = C.TPDF_COMMAND_KIND_FILL_PATH
	CommandStrokePath         CommandKind = C.TPDF_COMMAND_KIND_STROKE_PATH
	CommandClipPath           CommandKind = C.TPDF_COMMAND_KIND_CLIP_PATH
	CommandSaveState          CommandKind = C.TPDF_COMMAND_KIND_SAVE_STATE
	CommandRestoreState       CommandKind = C.TPDF_COMMAND_KIND_RESTORE_STATE
	CommandDrawImage          CommandKind = C.TPDF_COMMAND_KIND_DRAW_IMAGE
	CommandDrawShading        CommandKind = C.TPDF_COMMAND_KIND_DRAW_SHADING
	CommandBeginMarkedContent CommandKind = C.TPDF_COMMAND_KIND_BEGIN_MARKED_CONTENT
	CommandEndMarkedContent   CommandKind = C.TPDF_COMMAND_KIND_END_MARKED_CONTENT
	CommandBeginForm          CommandKind = C.TPDF_COMMAND_KIND_BEGIN_FORM
	CommandEndForm            CommandKind = C.TPDF_COMMAND_KIND_END_FORM
	CommandBeginGroup         CommandKind = C.TPDF_COMMAND_KIND_BEGIN_GROUP
	CommandEndGroup           CommandKind = C.TPDF_COMMAND_KIND_END_GROUP
	CommandBeginSoftMask      CommandKind = C.TPDF_COMMAND_KIND_BEGIN_SOFT_MASK
	CommandEndSoftMask        CommandKind = C.TPDF_COMMAND_KIND_END_SOFT_MASK
	CommandClearSoftMask      CommandKind = C.TPDF_COMMAND_KIND_CLEAR_SOFT_MASK
	CommandGroupBudgetSpent   CommandKind = C.TPDF_COMMAND_KIND_GROUP_BUDGET_SPENT
	CommandRecursionRefused   CommandKind = C.TPDF_COMMAND_KIND_RECURSION_REFUSED
	CommandNestedBudgetSpent  CommandKind = C.TPDF_COMMAND_KIND_NESTED_BUDGET_SPENT
	CommandPage               CommandKind = C.TPDF_COMMAND_KIND_PAGE
	CommandAnnotation         CommandKind = C.TPDF_COMMAND_KIND_ANNOTATION
	CommandMissingFont        CommandKind = C.TPDF_COMMAND_KIND_MISSING_FONT
	CommandRepairedSpace      CommandKind = C.TPDF_COMMAND_KIND_REPAIRED_SPACE
	CommandScope              CommandKind = C.TPDF_COMMAND_KIND_SCOPE
	CommandOutline            CommandKind = C.TPDF_COMMAND_KIND_OUTLINE
	CommandFontIdentity       CommandKind = C.TPDF_COMMAND_KIND_FONT_IDENTITY
	CommandProfile            CommandKind = C.TPDF_COMMAND_KIND_PROFILE
	CommandColorSpace         CommandKind = C.TPDF_COMMAND_KIND_COLOR_SPACE
	CommandImage              CommandKind = C.TPDF_COMMAND_KIND_IMAGE
	CommandInlineImage        CommandKind = C.TPDF_COMMAND_KIND_INLINE_IMAGE
	CommandImageDecode        CommandKind = C.TPDF_COMMAND_KIND_IMAGE_DECODE
	CommandShadingDefinition  CommandKind = C.TPDF_COMMAND_KIND_SHADING_DEFINITION
	CommandShading            CommandKind = C.TPDF_COMMAND_KIND_SHADING
	CommandPattern            CommandKind = C.TPDF_COMMAND_KIND_PATTERN
	CommandCell               CommandKind = C.TPDF_COMMAND_KIND_CELL
)

// commandKindNames are the kinds' names in their order, the facade's
// CommandKind's own.
var commandKindNames = [...]string{
	"End", "Function", "Halftone", "Stored", "Colorants", "Source", "State",
	"BeginText", "GlyphRun", "EndText", "FillPath", "StrokePath", "ClipPath",
	"SaveState", "RestoreState", "DrawImage", "DrawShading",
	"BeginMarkedContent", "EndMarkedContent", "BeginForm", "EndForm",
	"BeginGroup", "EndGroup", "BeginSoftMask", "EndSoftMask", "ClearSoftMask",
	"GroupBudgetSpent", "RecursionRefused", "NestedBudgetSpent", "Page",
	"Annotation", "MissingFont", "RepairedSpace", "Scope", "Outline",
	"FontIdentity", "Profile", "ColorSpace", "Image", "InlineImage",
	"ImageDecode", "ShadingDefinition", "Shading", "Pattern", "Cell",
}

// A name for every kind and no more: an index past [1] does not compile, so a
// kind added to the header without its name here fails the build.
var _ = [1]struct{}{}[len(commandKindNames)-1-int(CommandCell)]

// CommandKinds is every kind this build knows, in their order, the end first.
func CommandKinds() []CommandKind {
	kinds := make([]CommandKind, len(commandKindNames))
	for i := range kinds {
		kinds[i] = CommandKind(i)
	}
	return kinds
}

// Known is whether this build knows the kind.
func (k CommandKind) Known() bool {
	return k >= 0 && int(k) < len(commandKindNames)
}

func (k CommandKind) String() string {
	if !k.Known() {
		return fmt.Sprintf("CommandKind(%d)", int(k))
	}
	return commandKindNames[k]
}

// CommandFieldTag is what a field holds: its tag on the wire, the header's
// TpdfCommandFieldTag.
type CommandFieldTag int

// The tags: the header's own TpdfCommandFieldTag constants, so cgo checks each number.
const (
	FieldNothing  CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_NOTHING
	FieldUnsigned CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_UNSIGNED
	FieldSigned   CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_SIGNED
	FieldReal     CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_REAL
	FieldBytes    CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_BYTES
	FieldReals    CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_REALS
	FieldNaturals CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_NATURALS
	FieldStrings  CommandFieldTag = C.TPDF_COMMAND_FIELD_TAG_STRINGS
)

func (t CommandFieldTag) String() string {
	names := [...]string{"Nothing", "Unsigned", "Signed", "Real", "Bytes", "Reals", "Naturals", "Strings"}
	if t < 0 || int(t) >= len(names) {
		return fmt.Sprintf("CommandFieldTag(%d)", int(t))
	}
	return names[t]
}

// ImageAnswer is what the page's resources answered for an image at one
// reduction, the header's TpdfImageAnswer.
type ImageAnswer int

// The answers: the header's own TpdfImageAnswer constants, so cgo checks each number.
const (
	AnswerAbsent  ImageAnswer = C.TPDF_IMAGE_ANSWER_ABSENT
	AnswerImage   ImageAnswer = C.TPDF_IMAGE_ANSWER_IMAGE
	AnswerRefused ImageAnswer = C.TPDF_IMAGE_ANSWER_REFUSED
)

// Command is one record as the cursor reads it, the header's TpdfCommand.
type Command struct {
	// Kind is the record's kind, or -1 for one this build does not know.
	Kind CommandKind
	// Number is the kind's number on the wire, known or not.
	Number uint32
	// Index is the record's place, from 0, which the stream's accessors name
	// it by; the end stands at the number of records.
	Index uint32
	// FieldCount is how many fields the record holds.
	FieldCount uint32
}

// CommandField is one field of a record: its tag beside the value the tag
// says it holds, the one of Natural, Integer, Real, Bytes, Reals, Naturals and
// Strings the tag names, each slice a copy. Equal compares two by value, a
// real by its bits.
type CommandField struct {
	Tag      CommandFieldTag
	Natural  uint64
	Integer  int64
	Real     float64
	Bytes    []byte
	Reals    []float64
	Naturals []uint64
	Strings  [][]byte
}

// NothingField is an absent value.
func NothingField() CommandField { return CommandField{Tag: FieldNothing} }

// UnsignedField is an unsigned integer. A reference to a record is one.
func UnsignedField(value uint64) CommandField {
	return CommandField{Tag: FieldUnsigned, Natural: value}
}

// SignedField is a signed integer.
func SignedField(value int64) CommandField { return CommandField{Tag: FieldSigned, Integer: value} }

// RealField is a real, its bits as given.
func RealField(value float64) CommandField { return CommandField{Tag: FieldReal, Real: value} }

// BytesField is bytes, copied.
func BytesField(value []byte) CommandField {
	return CommandField{Tag: FieldBytes, Bytes: append([]byte{}, value...)}
}

// RealsField is reals, copied.
func RealsField(value []float64) CommandField {
	return CommandField{Tag: FieldReals, Reals: append([]float64{}, value...)}
}

// NaturalsField is unsigned integers, copied.
func NaturalsField(value []uint64) CommandField {
	return CommandField{Tag: FieldNaturals, Naturals: append([]uint64{}, value...)}
}

// StringsField is byte strings, each copied.
func StringsField(value [][]byte) CommandField {
	copies := make([][]byte, len(value))
	for i, each := range value {
		copies[i] = append([]byte{}, each...)
	}
	return CommandField{Tag: FieldStrings, Strings: copies}
}

// Equal is whether both fields hold one tag and one value, a real by its bits.
func (f CommandField) Equal(other CommandField) bool {
	if f.Tag != other.Tag {
		return false
	}
	switch f.Tag {
	case FieldNothing:
		return true
	case FieldUnsigned:
		return f.Natural == other.Natural
	case FieldSigned:
		return f.Integer == other.Integer
	case FieldReal:
		return math.Float64bits(f.Real) == math.Float64bits(other.Real)
	case FieldBytes:
		return string(f.Bytes) == string(other.Bytes)
	case FieldReals:
		if len(f.Reals) != len(other.Reals) {
			return false
		}
		for i := range f.Reals {
			if math.Float64bits(f.Reals[i]) != math.Float64bits(other.Reals[i]) {
				return false
			}
		}
		return true
	case FieldNaturals:
		if len(f.Naturals) != len(other.Naturals) {
			return false
		}
		for i := range f.Naturals {
			if f.Naturals[i] != other.Naturals[i] {
				return false
			}
		}
		return true
	case FieldStrings:
		if len(f.Strings) != len(other.Strings) {
			return false
		}
		for i := range f.Strings {
			if string(f.Strings[i]) != string(other.Strings[i]) {
				return false
			}
		}
		return true
	}
	return false
}

func (f CommandField) String() string {
	switch f.Tag {
	case FieldNothing:
		return "Nothing"
	case FieldUnsigned:
		return fmt.Sprintf("Unsigned %d", f.Natural)
	case FieldSigned:
		return fmt.Sprintf("Signed %d", f.Integer)
	case FieldReal:
		return fmt.Sprintf("Real %v", f.Real)
	case FieldBytes:
		return fmt.Sprintf("Bytes %v", f.Bytes)
	case FieldReals:
		return fmt.Sprintf("Reals %v", f.Reals)
	case FieldNaturals:
		return fmt.Sprintf("Naturals %v", f.Naturals)
	case FieldStrings:
		return fmt.Sprintf("Strings of %d", len(f.Strings))
	}
	return f.Tag.String()
}

// CommandRecord is a record and its fields, in their places. What each place
// holds is the kind's (docs/design/command-stream.md, "The records"): a
// command's state is its field 0, a reference to the State records before it
// in their order.
type CommandRecord struct {
	// Kind is the record's kind, or -1 for one this build does not know.
	Kind CommandKind
	// Number is the kind's number on the wire.
	Number uint32
	// Index is the record's place in its stream, from 0.
	Index uint32
	// Fields are the record's fields, copied out of the stream.
	Fields []CommandField
}

// GlyphOutline is a glyph's outline record, the header's TpdfCommandOutline.
type GlyphOutline struct {
	// Scope is the scope the glyph was asked in: 0 the page's, n the nth
	// scope record's.
	Scope uint64
	// FontID is the font the glyph is of, as the stream's glyph runs name it.
	FontID uint64
	// Code is the glyph's code.
	Code uint32
	// Found is whether the font gave the glyph an outline; both slices are
	// empty where it did not.
	Found bool
	// Operators are a byte a segment: 0 a move, 1 a line, 2 a quadratic, 3 a
	// cubic, 4 a close.
	Operators []byte
	// Numbers are the segments' numbers in order, in a space where a unit is
	// an em: two for a move or a line, four for a quadratic, six for a cubic.
	Numbers []float64
}

// ImageWindow is the part of a decode an image decode's samples are, its top
// edge counting down.
type ImageWindow struct {
	X, Y, Width, Height uint32
}

// DecodedImage is an image's decode at one reduction, the header's
// TpdfCommandImage.
type DecodedImage struct {
	// Image is the image the decode is of: the nth image record, inline or
	// not, from 0.
	Image uint64
	// Reduction is 1 for the whole image, or what a draw at a smaller size
	// asks a JPEG or a codestream for.
	Reduction uint32
	// Answer is what the page's resources answered.
	Answer ImageAnswer
	// Width and Height are the decode's size in samples.
	Width, Height uint32
	// Window is the window the samples are, or nil where they are all of
	// the decode.
	Window *ImageWindow
	// Stencil is a stencil mask, which paints the fill colour where it is set.
	Stencil bool
	// Interpolate is the image's /Interpolate.
	Interpolate bool
	// Codec is a refused decode's codec; empty otherwise.
	Codec string
	// RGB is three bytes a sample, row-major from the top; empty where
	// TwoColour holds the samples.
	RGB []byte
	// Alpha is opacity, a byte a sample; empty for an opaque image.
	Alpha []byte
	// TwoColour is a byte a sample choosing between Colours' two, zero the
	// first; empty for an image held as RGB.
	TwoColour []byte
	// Colours are red, green, blue and alpha of a sample of zero, then of
	// any other.
	Colours [8]byte
}

// CommandStream is a command stream read, held by the engine. Everything it
// hands back is a copy that outlives it. Bytes the reader refuses are an
// *Error of StatusBadArgument carrying the reader's own sentence.
type CommandStream struct {
	ptr *C.TpdfCommandStream
}

// ParseCommandStream reads a stream's bytes, which are read and not kept.
func ParseCommandStream(data []byte) (*CommandStream, error) {
	var out *C.TpdfCommandStream
	if err := call(func() C.enum_TpdfStatus {
		bytes, length := bytesArg(data)
		return C.tpdf_command_stream_parse(bytes, length, &out)
	}); err != nil {
		return nil, err
	}
	return &CommandStream{ptr: out}, nil
}

// Close releases the stream. What was read from it stays valid.
func (s *CommandStream) Close() {
	if s != nil && s.ptr != nil {
		C.tpdf_command_stream_free(s.ptr)
		s.ptr = nil
	}
}

// Count is how many records the stream holds before its end.
func (s *CommandStream) Count() uint32 {
	return uint32(C.tpdf_command_stream_count(s.ptr))
}

func commandOf(raw C.TpdfCommand) Command {
	return Command{
		Kind:       CommandKind(raw.kind),
		Number:     uint32(raw.number),
		Index:      uint32(raw.index),
		FieldCount: uint32(raw.field_count),
	}
}

// Next is the cursor: the next record's command, moving past it. After the
// last record it reads CommandEnd, at the number of records, until Rewind.
func (s *CommandStream) Next() (Command, error) {
	var raw C.TpdfCommand
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_command_stream_next(s.ptr, &raw) }); err != nil {
		return Command{}, err
	}
	return commandOf(raw), nil
}

// Rewind puts the cursor back on the stream's first record.
func (s *CommandStream) Rewind() error {
	return call(func() C.enum_TpdfStatus { return C.tpdf_command_stream_rewind(s.ptr) })
}

// CommandAt is record index's command, wherever the cursor stands.
func (s *CommandStream) CommandAt(index uint32) (Command, error) {
	var raw C.TpdfCommand
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_command_stream_record(s.ptr, C.uint32_t(index), &raw)
	}); err != nil {
		return Command{}, err
	}
	return commandOf(raw), nil
}

// lent copies count values of T the stream lends at data; empty for none.
func lent[T any](data unsafe.Pointer, count C.size_t) []T {
	if data == nil || count == 0 {
		return []T{}
	}
	return append([]T{}, unsafe.Slice((*T)(data), int(count))...)
}

// FieldAt is field field of record index, copied.
func (s *CommandStream) FieldAt(index, field uint32) (CommandField, error) {
	var raw C.TpdfCommandField
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_command_stream_field(s.ptr, C.uint32_t(index), C.uint32_t(field), &raw)
	}); err != nil {
		return CommandField{}, err
	}
	switch CommandFieldTag(raw.tag) {
	case FieldNothing:
		return NothingField(), nil
	case FieldUnsigned:
		return UnsignedField(uint64(raw.natural)), nil
	case FieldSigned:
		return SignedField(int64(raw.integer)), nil
	case FieldReal:
		return RealField(float64(raw.real)), nil
	case FieldBytes:
		return CommandField{Tag: FieldBytes, Bytes: lent[byte](raw.data, raw.count)}, nil
	case FieldReals:
		return CommandField{Tag: FieldReals, Reals: lent[float64](raw.data, raw.count)}, nil
	case FieldNaturals:
		return CommandField{Tag: FieldNaturals, Naturals: lent[uint64](raw.data, raw.count)}, nil
	case FieldStrings:
		list := lent[C.TpdfCommandBytes](raw.data, raw.count)
		strings := make([][]byte, len(list))
		for i, each := range list {
			strings[i] = lent[byte](unsafe.Pointer(each.data), each.len)
		}
		return CommandField{Tag: FieldStrings, Strings: strings}, nil
	}
	return CommandField{}, fmt.Errorf("tinker-pdf: field %d of record %d is of tag %d, which this binding does not read",
		field, index, int(raw.tag))
}

// RecordAt is record index: its command and every one of its fields.
func (s *CommandStream) RecordAt(index uint32) (CommandRecord, error) {
	command, err := s.CommandAt(index)
	if err != nil {
		return CommandRecord{}, err
	}
	fields := make([]CommandField, command.FieldCount)
	for at := uint32(0); at < command.FieldCount; at++ {
		if fields[at], err = s.FieldAt(index, at); err != nil {
			return CommandRecord{}, err
		}
	}
	return CommandRecord{Kind: command.Kind, Number: command.Number, Index: command.Index, Fields: fields}, nil
}

// Records is every record, in the stream's order.
func (s *CommandStream) Records() ([]CommandRecord, error) {
	count := s.Count()
	records := make([]CommandRecord, 0, count)
	for index := uint32(0); index < count; index++ {
		record, err := s.RecordAt(index)
		if err != nil {
			return nil, err
		}
		records = append(records, record)
	}
	return records, nil
}

// OutlineAt is outline record index's glyph. A record of another kind is
// refused, naming both.
func (s *CommandStream) OutlineAt(index uint32) (GlyphOutline, error) {
	var raw C.TpdfCommandOutline
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_command_stream_outline(s.ptr, C.uint32_t(index), &raw)
	}); err != nil {
		return GlyphOutline{}, err
	}
	return GlyphOutline{
		Scope:     uint64(raw.scope),
		FontID:    uint64(raw.font_id),
		Code:      uint32(raw.code),
		Found:     raw.found != 0,
		Operators: lent[byte](unsafe.Pointer(raw.operators.data), raw.operators.len),
		Numbers:   lent[float64](unsafe.Pointer(raw.numbers), raw.number_count),
	}, nil
}

// ImageAt is image decode record index. A record of another kind is refused,
// naming both.
func (s *CommandStream) ImageAt(index uint32) (DecodedImage, error) {
	var raw C.TpdfCommandImage
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_command_stream_image(s.ptr, C.uint32_t(index), &raw)
	}); err != nil {
		return DecodedImage{}, err
	}
	image := DecodedImage{
		Image:       uint64(raw.image),
		Reduction:   uint32(raw.reduction),
		Answer:      ImageAnswer(raw.answer),
		Width:       uint32(raw.width),
		Height:      uint32(raw.height),
		Stencil:     raw.stencil != 0,
		Interpolate: raw.interpolate != 0,
		Codec:       string(lent[byte](unsafe.Pointer(raw.codec.data), raw.codec.len)),
		RGB:         lent[byte](unsafe.Pointer(raw.rgb.data), raw.rgb.len),
		Alpha:       lent[byte](unsafe.Pointer(raw.alpha.data), raw.alpha.len),
		TwoColour:   lent[byte](unsafe.Pointer(raw.two_colour.data), raw.two_colour.len),
	}
	if raw.has_window != 0 {
		image.Window = &ImageWindow{
			X:      uint32(raw.window_x),
			Y:      uint32(raw.window_y),
			Width:  uint32(raw.window_width),
			Height: uint32(raw.window_height),
		}
	}
	for i := range image.Colours {
		image.Colours[i] = byte(raw.colours[i])
	}
	return image, nil
}

// ToBytes is the stream's bytes, written back at its own version.
func (s *CommandStream) ToBytes() ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_command_stream_to_bytes(s.ptr, &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}

// Version is the stream's major and minor version.
func (s *CommandStream) Version() (uint16, uint16, error) {
	var major, minor C.uint16_t
	err := call(func() C.enum_TpdfStatus { return C.tpdf_command_stream_version(s.ptr, &major, &minor) })
	return uint16(major), uint16(minor), err
}

// PageIndex is which page of its document the stream is of.
func (s *CommandStream) PageIndex() (uint32, error) {
	var index C.uint32_t
	err := call(func() C.enum_TpdfStatus { return C.tpdf_command_stream_page_index(s.ptr, &index) })
	return uint32(index), err
}

// Size is the page's size in points after rotation.
func (s *CommandStream) Size() (float64, float64, error) {
	var width, height C.double
	err := call(func() C.enum_TpdfStatus { return C.tpdf_command_stream_size(s.ptr, &width, &height) })
	return float64(width), float64(height), err
}

// Render draws the stream as a render draws its page, through the engine's
// own renderer, at a scale, 1.0 being 72 dots per inch.
func (s *CommandStream) Render(scale float64, format PixelFormat) (*Bitmap, error) {
	var out *C.TpdfBitmap
	if err := call(func() C.enum_TpdfStatus {
		return C.tpdf_command_stream_render(s.ptr, C.double(scale), C.int(format), &out)
	}); err != nil {
		return nil, err
	}
	return &Bitmap{ptr: out}, nil
}

// CommandDevice is what a replay hands records to: one method a kind, each
// handed the record, and Unknown for a kind this build does not know. A
// method's error ends the replay and is returned.
//
// A device embeds IgnoreCommands and implements the kinds it draws, the rest
// passed over. A record's tables, its glyphs' outlines and its images'
// decodes, are the stream's OutlineAt and ImageAt, which a device that draws
// them holds the stream for. Go's standard library has no 2D vector canvas
// (image/draw composites rasters and draws no paths), so the binding ships
// the interface alone.
type CommandDevice interface {
	Function(CommandRecord) error
	Halftone(CommandRecord) error
	Stored(CommandRecord) error
	Colorants(CommandRecord) error
	Source(CommandRecord) error
	State(CommandRecord) error
	BeginText(CommandRecord) error
	GlyphRun(CommandRecord) error
	EndText(CommandRecord) error
	FillPath(CommandRecord) error
	StrokePath(CommandRecord) error
	ClipPath(CommandRecord) error
	SaveState(CommandRecord) error
	RestoreState(CommandRecord) error
	DrawImage(CommandRecord) error
	DrawShading(CommandRecord) error
	BeginMarkedContent(CommandRecord) error
	EndMarkedContent(CommandRecord) error
	BeginForm(CommandRecord) error
	EndForm(CommandRecord) error
	BeginGroup(CommandRecord) error
	EndGroup(CommandRecord) error
	BeginSoftMask(CommandRecord) error
	EndSoftMask(CommandRecord) error
	ClearSoftMask(CommandRecord) error
	GroupBudgetSpent(CommandRecord) error
	RecursionRefused(CommandRecord) error
	NestedBudgetSpent(CommandRecord) error
	Page(CommandRecord) error
	Annotation(CommandRecord) error
	MissingFont(CommandRecord) error
	RepairedSpace(CommandRecord) error
	Scope(CommandRecord) error
	Outline(CommandRecord) error
	FontIdentity(CommandRecord) error
	Profile(CommandRecord) error
	ColorSpace(CommandRecord) error
	Image(CommandRecord) error
	InlineImage(CommandRecord) error
	ImageDecode(CommandRecord) error
	ShadingDefinition(CommandRecord) error
	Shading(CommandRecord) error
	Pattern(CommandRecord) error
	Cell(CommandRecord) error
	Unknown(CommandRecord) error
}

// IgnoreCommands passes over every record: a device embeds it and implements
// the methods of the kinds it draws.
type IgnoreCommands struct{}

func (IgnoreCommands) Function(CommandRecord) error           { return nil }
func (IgnoreCommands) Halftone(CommandRecord) error           { return nil }
func (IgnoreCommands) Stored(CommandRecord) error             { return nil }
func (IgnoreCommands) Colorants(CommandRecord) error          { return nil }
func (IgnoreCommands) Source(CommandRecord) error             { return nil }
func (IgnoreCommands) State(CommandRecord) error              { return nil }
func (IgnoreCommands) BeginText(CommandRecord) error          { return nil }
func (IgnoreCommands) GlyphRun(CommandRecord) error           { return nil }
func (IgnoreCommands) EndText(CommandRecord) error            { return nil }
func (IgnoreCommands) FillPath(CommandRecord) error           { return nil }
func (IgnoreCommands) StrokePath(CommandRecord) error         { return nil }
func (IgnoreCommands) ClipPath(CommandRecord) error           { return nil }
func (IgnoreCommands) SaveState(CommandRecord) error          { return nil }
func (IgnoreCommands) RestoreState(CommandRecord) error       { return nil }
func (IgnoreCommands) DrawImage(CommandRecord) error          { return nil }
func (IgnoreCommands) DrawShading(CommandRecord) error        { return nil }
func (IgnoreCommands) BeginMarkedContent(CommandRecord) error { return nil }
func (IgnoreCommands) EndMarkedContent(CommandRecord) error   { return nil }
func (IgnoreCommands) BeginForm(CommandRecord) error          { return nil }
func (IgnoreCommands) EndForm(CommandRecord) error            { return nil }
func (IgnoreCommands) BeginGroup(CommandRecord) error         { return nil }
func (IgnoreCommands) EndGroup(CommandRecord) error           { return nil }
func (IgnoreCommands) BeginSoftMask(CommandRecord) error      { return nil }
func (IgnoreCommands) EndSoftMask(CommandRecord) error        { return nil }
func (IgnoreCommands) ClearSoftMask(CommandRecord) error      { return nil }
func (IgnoreCommands) GroupBudgetSpent(CommandRecord) error   { return nil }
func (IgnoreCommands) RecursionRefused(CommandRecord) error   { return nil }
func (IgnoreCommands) NestedBudgetSpent(CommandRecord) error  { return nil }
func (IgnoreCommands) Page(CommandRecord) error               { return nil }
func (IgnoreCommands) Annotation(CommandRecord) error         { return nil }
func (IgnoreCommands) MissingFont(CommandRecord) error        { return nil }
func (IgnoreCommands) RepairedSpace(CommandRecord) error      { return nil }
func (IgnoreCommands) Scope(CommandRecord) error              { return nil }
func (IgnoreCommands) Outline(CommandRecord) error            { return nil }
func (IgnoreCommands) FontIdentity(CommandRecord) error       { return nil }
func (IgnoreCommands) Profile(CommandRecord) error            { return nil }
func (IgnoreCommands) ColorSpace(CommandRecord) error         { return nil }
func (IgnoreCommands) Image(CommandRecord) error              { return nil }
func (IgnoreCommands) InlineImage(CommandRecord) error        { return nil }
func (IgnoreCommands) ImageDecode(CommandRecord) error        { return nil }
func (IgnoreCommands) ShadingDefinition(CommandRecord) error  { return nil }
func (IgnoreCommands) Shading(CommandRecord) error            { return nil }
func (IgnoreCommands) Pattern(CommandRecord) error            { return nil }
func (IgnoreCommands) Cell(CommandRecord) error               { return nil }
func (IgnoreCommands) Unknown(CommandRecord) error            { return nil }

// Replay hands every record, in the stream's order, to the method of device
// named for its kind, and a record of a kind this build does not know to
// Unknown. A method's error ends the replay and is returned.
func (s *CommandStream) Replay(device CommandDevice) error {
	count := s.Count()
	for index := uint32(0); index < count; index++ {
		record, err := s.RecordAt(index)
		if err != nil {
			return err
		}
		if err := replayOne(device, record); err != nil {
			return err
		}
	}
	return nil
}

// replayOne hands record to the method its kind names.
func replayOne(device CommandDevice, record CommandRecord) error {
	switch record.Kind {
	// The end is where a stream stops: no record is of its kind.
	case CommandEnd:
		return nil
	case CommandFunction:
		return device.Function(record)
	case CommandHalftone:
		return device.Halftone(record)
	case CommandStored:
		return device.Stored(record)
	case CommandColorants:
		return device.Colorants(record)
	case CommandSource:
		return device.Source(record)
	case CommandState:
		return device.State(record)
	case CommandBeginText:
		return device.BeginText(record)
	case CommandGlyphRun:
		return device.GlyphRun(record)
	case CommandEndText:
		return device.EndText(record)
	case CommandFillPath:
		return device.FillPath(record)
	case CommandStrokePath:
		return device.StrokePath(record)
	case CommandClipPath:
		return device.ClipPath(record)
	case CommandSaveState:
		return device.SaveState(record)
	case CommandRestoreState:
		return device.RestoreState(record)
	case CommandDrawImage:
		return device.DrawImage(record)
	case CommandDrawShading:
		return device.DrawShading(record)
	case CommandBeginMarkedContent:
		return device.BeginMarkedContent(record)
	case CommandEndMarkedContent:
		return device.EndMarkedContent(record)
	case CommandBeginForm:
		return device.BeginForm(record)
	case CommandEndForm:
		return device.EndForm(record)
	case CommandBeginGroup:
		return device.BeginGroup(record)
	case CommandEndGroup:
		return device.EndGroup(record)
	case CommandBeginSoftMask:
		return device.BeginSoftMask(record)
	case CommandEndSoftMask:
		return device.EndSoftMask(record)
	case CommandClearSoftMask:
		return device.ClearSoftMask(record)
	case CommandGroupBudgetSpent:
		return device.GroupBudgetSpent(record)
	case CommandRecursionRefused:
		return device.RecursionRefused(record)
	case CommandNestedBudgetSpent:
		return device.NestedBudgetSpent(record)
	case CommandPage:
		return device.Page(record)
	case CommandAnnotation:
		return device.Annotation(record)
	case CommandMissingFont:
		return device.MissingFont(record)
	case CommandRepairedSpace:
		return device.RepairedSpace(record)
	case CommandScope:
		return device.Scope(record)
	case CommandOutline:
		return device.Outline(record)
	case CommandFontIdentity:
		return device.FontIdentity(record)
	case CommandProfile:
		return device.Profile(record)
	case CommandColorSpace:
		return device.ColorSpace(record)
	case CommandImage:
		return device.Image(record)
	case CommandInlineImage:
		return device.InlineImage(record)
	case CommandImageDecode:
		return device.ImageDecode(record)
	case CommandShadingDefinition:
		return device.ShadingDefinition(record)
	case CommandShading:
		return device.Shading(record)
	case CommandPattern:
		return device.Pattern(record)
	case CommandCell:
		return device.Cell(record)
	}
	return device.Unknown(record)
}

// CommandEncoder takes records back: each pushed as its kind and fields,
// copied, and finished into a stream's bytes at this build's version, read
// back as ParseCommandStream reads them. A kind or a tag the C ABI does not
// declare, the end pushed, and records whose stream the reader refuses are an
// *Error of StatusBadArgument naming them. It keeps its records, so it
// finishes again.
type CommandEncoder struct {
	ptr *C.TpdfCommandEncoder
}

// NewCommandEncoder is an encoder holding no records.
func NewCommandEncoder() (*CommandEncoder, error) {
	var out *C.TpdfCommandEncoder
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_command_encoder_new(&out) }); err != nil {
		return nil, err
	}
	return &CommandEncoder{ptr: out}, nil
}

// Close releases the encoder.
func (e *CommandEncoder) Close() {
	if e != nil && e.ptr != nil {
		C.tpdf_command_encoder_free(e.ptr)
		e.ptr = nil
	}
}

// arena is C memory taken for one call and freed after it.
type arena []unsafe.Pointer

// take is size bytes of C memory, freed with the arena; nil for none.
func (a *arena) take(size uintptr) unsafe.Pointer {
	if size == 0 {
		return nil
	}
	pointer := C.malloc(C.size_t(size))
	*a = append(*a, pointer)
	return pointer
}

func (a *arena) free() {
	for _, pointer := range *a {
		C.free(pointer)
	}
	*a = nil
}

// laid copies values into C memory the arena holds.
func laid[T any](a *arena, values []T) (unsafe.Pointer, C.size_t) {
	if len(values) == 0 {
		return nil, 0
	}
	var one T
	pointer := a.take(uintptr(len(values)) * unsafe.Sizeof(one))
	copy(unsafe.Slice((*T)(pointer), len(values)), values)
	return pointer, C.size_t(len(values))
}

// lay is field as the header's plain values, its arrays in C memory the
// arena holds.
func lay(a *arena, field CommandField) C.TpdfCommandField {
	raw := C.TpdfCommandField{tag: C.int(field.Tag)}
	switch field.Tag {
	case FieldUnsigned:
		raw.natural = C.uint64_t(field.Natural)
	case FieldSigned:
		raw.integer = C.int64_t(field.Integer)
	case FieldReal:
		raw.real = C.double(field.Real)
	case FieldBytes:
		raw.data, raw.count = laid(a, field.Bytes)
	case FieldReals:
		raw.data, raw.count = laid(a, field.Reals)
	case FieldNaturals:
		raw.data, raw.count = laid(a, field.Naturals)
	case FieldStrings:
		list := make([]C.TpdfCommandBytes, len(field.Strings))
		for i, each := range field.Strings {
			data, length := laid(a, each)
			list[i] = C.TpdfCommandBytes{data: (*C.uint8_t)(data), len: length}
		}
		raw.data, raw.count = laid(a, list)
	}
	return raw
}

// Push appends a record of kind and its fields.
func (e *CommandEncoder) Push(kind CommandKind, fields []CommandField) error {
	var memory arena
	defer memory.free()
	raw := make([]C.TpdfCommandField, len(fields))
	for i, field := range fields {
		raw[i] = lay(&memory, field)
	}
	data, count := laid(&memory, raw)
	return call(func() C.enum_TpdfStatus {
		return C.tpdf_command_encoder_push(e.ptr, C.int(kind), (*C.TpdfCommandField)(data), count)
	})
}

// PushRecord appends record's kind and fields; a kind this build does not
// know crosses as the C ABI's -1, which it refuses.
func (e *CommandEncoder) PushRecord(record CommandRecord) error {
	return e.Push(record.Kind, record.Fields)
}

// Finish is the records pushed so far as a stream's bytes.
func (e *CommandEncoder) Finish() ([]byte, error) {
	var out *C.TpdfBuffer
	if err := call(func() C.enum_TpdfStatus { return C.tpdf_command_encoder_finish(e.ptr, &out) }); err != nil {
		return nil, err
	}
	return takeBuffer(out), nil
}
