// Proves the Go binding is the engine, not merely a package that compiles.
//
//	go run ./cmd/smoke <testdata/simple-text.pdf> <face.ttf>
//
// The trap is the one every smoke test here is written against.
// testdata/simple-text.pdf names Helvetica and embeds no font program, and
// the engine bundles no faces and reads no font directories — so a render of
// it is a correctly sized, entirely blank bitmap, and "a bitmap of the right
// size came back" passes on a library whose renderer does nothing at all. So
// the render is asserted twice: blank without a face, inked with one. The
// program prints GO-SMOKE: RAN on success, which CI greps for, because a
// program that exits 0 having checked nothing looks exactly like a pass.
package main

import (
	"bytes"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"

	tp "github.com/ravindu-rev/tinker-pdf/bindings/go"
)

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "GO-SMOKE: FAILED: "+format+"\n", args...)
	os.Exit(1)
}

// ink counts pixels that are not white, one byte per component of RGB.
func ink(pixels []byte) int {
	painted := 0
	for i := 0; i+2 < len(pixels); i += 3 {
		if pixels[i] != 0xFF {
			painted++
		}
	}
	return painted
}

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: smoke <file.pdf> <face.ttf>")
		os.Exit(2)
	}
	fmt.Println("engine version", tp.Version())
	data, err := os.ReadFile(os.Args[1])
	if err != nil {
		fail("%v", err)
	}
	face, err := os.ReadFile(os.Args[2])
	if err != nil {
		fail("%v", err)
	}

	document, err := tp.Open(data)
	if err != nil {
		fail("open: %v", err)
	}
	defer document.Close()
	if document.PageCount() != 3 {
		fail("pageCount %d", document.PageCount())
	}
	text, err := document.PageText(0)
	if err != nil || !strings.Contains(text, "Tinker fixture") {
		fail("text %q %v", text, err)
	}
	fmt.Printf("text=%q\n", strings.TrimSpace(text))

	bare, err := document.Render(0, 1.0, tp.Rgb8)
	if err != nil {
		fail("render: %v", err)
	}
	if painted := ink(bare.Pixels()); painted != 0 {
		fail("this fixture embeds no font, so it must draw nothing yet; it drew %d", painted)
	}
	bare.Close()

	if err := document.SetFonts(face); err != nil {
		fail("set fonts: %v", err)
	}
	drawn, err := document.Render(0, 1.0, tp.Rgb8)
	if err != nil {
		fail("render: %v", err)
	}
	defer drawn.Close()
	pixels := drawn.Pixels()
	painted := ink(pixels)
	fmt.Printf("bitmap %dx%d stride=%d bytes=%d ink=%d\n", drawn.Width(), drawn.Height(), drawn.Stride(), len(pixels), painted)
	if painted < 100 {
		fail("only %d pixels of ink with a face supplied", painted)
	}
	if len(pixels) != drawn.Stride()*int(drawn.Height()) {
		fail("the pixel slice is %d bytes", len(pixels))
	}

	// A page past the end is the engine's refusal, with its status, rather
	// than a crash or an empty answer.
	if _, err := document.PageText(99); err == nil {
		fail("a page past the end must be refused")
	} else if e, ok := err.(*tp.Error); !ok || e.Status != tp.StatusNoSuchPage {
		fail("a page past the end is %v, not NoSuchPage", err)
	}
	// So is a number its enum does not declare: a Go enum is an int, it
	// crosses as a C int, and the engine checks it (review of lane 7C).
	expectStatus(func() error { _, err := document.Info(tp.InfoKey(8)); return err }, tp.StatusBadArgument,
		"an info key of 8")
	expectStatus(func() error { _, err := document.Render(0, 1.0, tp.PixelFormat(4)); return err },
		tp.StatusBadArgument, "a pixel format of 4")
	if document.IsEncrypted() || !document.MayPrint() || document.IsStreamed() {
		fail("simple-text.pdf is unencrypted, printable and opened from bytes")
	}
	if w, h, err := document.PageSize(0); err != nil || w != 595 || h != 842 {
		fail("page size %v x %v %v", w, h, err)
	}
	expectStatus(func() error { _, err := document.Authenticate("anything"); return err }, tp.StatusNotEncrypted,
		"authenticating an unencrypted document")

	fixtures := filepath.Dir(os.Args[1])
	streamed(data)
	locks(fixtures)
	edits(fixtures)
	builds(face)
	absolute, err := filepath.Abs(fixtures)
	must(err)
	commandStreams(filepath.Dir(absolute))
	fmt.Println("GO-SMOKE: RAN, rendered and inked, every declaration called, the command stream written back")
}

// expectStatus asserts f fails with exactly status.
func expectStatus(f func() error, status tp.Status, what string) {
	err := f()
	if e, ok := err.(*tp.Error); !ok || e.Status != status {
		fail("%s: %v, not status %d", what, err, int(status))
	}
}

func must(err error) {
	if err != nil {
		fail("%v", err)
	}
}

// memory is a Source over bytes in hand, which can be told to withhold them.
type memory struct {
	data     []byte
	withhold bool
}

func (m *memory) Len() uint64 { return uint64(len(m.data)) }

func (m *memory) ReadAt(offset uint64, out []byte) int64 {
	if m.withhold {
		return -1
	}
	return int64(copy(out, m.data[offset:]))
}

// The rest of the header, each call once, against the fixtures that answer
// it. The parity program covers what it scripts; this covers what it does
// not, so a declaration wrapped wrongly fails here rather than in a caller.
func streamed(data []byte) {
	document, err := tp.OpenStreaming(&memory{data: data})
	must(err)
	defer document.Close()
	if !document.IsStreamed() || document.PageCount() != 3 {
		fail("a streamed open is streamed and has three pages")
	}
	if !document.AnswersAsBuffered() {
		fail("a streamed open of a clean file answers as its bytes do")
	}
	if text, err := document.PageText(0); err != nil || !strings.Contains(text, "Tinker fixture") {
		fail("streamed text %q %v", text, err)
	}
	expectStatus(func() error { _, err := tp.OpenStreaming(&memory{data: data, withhold: true}); return err },
		tp.StatusSourceMiss, "a source that has none of the bytes")
}

func locks(fixtures string) {
	data, err := os.ReadFile(filepath.Join(fixtures, "encrypted-aes256.pdf"))
	must(err)
	locked, err := tp.Open(data)
	must(err)
	defer locked.Close()
	if !locked.IsEncrypted() {
		fail("encrypted-aes256.pdf is encrypted")
	}
	expectStatus(func() error { _, err := locked.Authenticate("not it"); return err }, tp.StatusWrongPassword,
		"a wrong password")
	if level, err := locked.Authenticate("open-sesame"); err != nil || level != tp.AuthUser {
		fail("the user password reached %v %v", level, err)
	}
	data, err = os.ReadFile(filepath.Join(fixtures, "permissions-noprint.pdf"))
	must(err)
	restricted, err := tp.Open(data)
	must(err)
	defer restricted.Close()
	_, err = restricted.Authenticate("user")
	must(err)
	if restricted.MayPrint() {
		fail("permissions-noprint.pdf denies printing to its user")
	}
}

func edits(fixtures string) {
	data, err := os.ReadFile(filepath.Join(fixtures, "form-fields.pdf"))
	must(err)
	form, err := tp.Open(data)
	must(err)
	editor, err := form.Editor()
	must(err)
	form.Close()
	defer editor.Close()
	names := []string{}
	notes := uint32(0)
	for i := uint32(0); i < editor.FieldCount(); i++ {
		name, err := editor.FieldName(i)
		must(err)
		if name == "notes" {
			notes = i
		}
		names = append(names, name)
	}
	sort.Strings(names)
	if strings.Join(names, ",") != "agree,colour,name,notes" {
		fail("fields %v", names)
	}
	if editor.IsDirty() {
		fail("nothing is edited yet")
	}
	checkpoint, err := editor.Checkpoint()
	must(err)
	skipped, err := editor.FillField("name", "Ada")
	must(err)
	if skipped[0].Defect != tp.WidgetRectMissing {
		fail("the skipped widget's defect is %v", skipped[0].Defect)
	}
	_, err = editor.FillField("notes", "kept for a moment")
	must(err)
	if value, _ := editor.FieldValue(notes); value != "kept for a moment" || !editor.IsDirty() {
		fail("notes is %q", value)
	}
	must(editor.Restore(checkpoint))
	must(editor.Restore(checkpoint))
	checkpoint.Close()
	if value, _ := editor.FieldValue(notes); value != "" {
		fail("restored notes is %q", value)
	}
	recalculation, err := editor.Recalculate(tp.ScriptDefault)
	must(err)
	if len(recalculation.Changed) != 0 || recalculation.Skipped != 0 || len(recalculation.Refused) != 0 {
		fail("a form with no calculations changed %+v", recalculation)
	}
	if formatted, err := editor.FormattedValue("notes", tp.ScriptDefault); err != nil || formatted != nil {
		fail("notes carries no format action: %v %v", formatted, err)
	}
	if accepted, change, err := editor.Keystroke("notes", "x", 0, 0, false, tp.ScriptKeystroke); err != nil ||
		!accepted || change == nil || *change != "x" {
		fail("a field with no keystroke action took %v %v %v", accepted, change, err)
	}
	if accepted, value, err := editor.Validate("notes", "v", tp.ScriptValidate); err != nil ||
		!accepted || value == nil || *value != "v" {
		fail("a field with no validate action took %v %v %v", accepted, value, err)
	}

	must(editor.InsertPage(1, 200, 300))
	must(editor.RotatePage(0, 90))
	must(editor.MovePage(1, 0))
	must(editor.SetCropBox(0, 0, 0, 100, 100))
	must(editor.AppendContent(0, []byte("0 0 m 10 10 l S")))
	expectStatus(func() error { return editor.RotatePage(0, 45) }, tp.StatusEditRefused,
		"a turn that is not a quarter-turn multiple")
	if editor.PageCount() != 2 {
		fail("the editor sees %d pages", editor.PageCount())
	}
	must(editor.DeletePage(1))
	options, err := tp.DefaultWriteOptions()
	must(err)
	entropy := make([]byte, tp.EntropyLen)
	for i := range entropy {
		entropy[i] = byte(i)
	}
	options.Encryption = &tp.Encryption{UserPassword: "u", OwnerPassword: "o", Permissions: -4, Entropy: entropy}
	saved, err := editor.Save(options)
	must(err)
	reopened, err := tp.Open(saved)
	must(err)
	defer reopened.Close()
	if !reopened.IsEncrypted() {
		fail("the save asked for encryption")
	}
	if level, err := reopened.Authenticate("u"); err != nil || level != tp.AuthUser {
		fail("the user password opens it: %v %v", level, err)
	}
	if x0, y0, x1, y1, err := reopened.PageBox(0, tp.MediaBox); err != nil || x0 != 0 || y0 != 0 || x1 != 200 || y1 != 300 {
		fail("the inserted page is not first: %v %v %v %v %v", x0, y0, x1, y1, err)
	}
	if w, h, err := reopened.PageSize(0); err != nil || w != 100 || h != 100 {
		fail("the crop box is the page's size: %v x %v %v", w, h, err)
	}
}

func builds(face []byte) {
	builder, err := tp.NewBuilder()
	must(err)
	defer builder.Close()
	must(builder.AddEmbeddedFont([]byte("F2"), []byte("DejaVuSans"), face))
	must(builder.SetSubsetFonts(true))
	page, err := builder.BeginPage(200, 200)
	must(err)
	must(page.SetFillRGB(0.2, 0.4, 0.6))
	must(page.SetStrokeRGB(0.6, 0.4, 0.2))
	must(page.SetCropBox(0, 0, 150, 150))
	must(page.Raw([]byte("10 10 m 140 140 l S")))
	must(page.Text([]byte("F2"), 18, 20, 100, "Embedded"))
	must(builder.PushPage(page))
	page.Close()
	bytes, err := builder.Finish()
	must(err)
	built, err := tp.Open(bytes)
	must(err)
	defer built.Close()
	if defects, err := built.Validate(); err != nil || len(defects) != 0 {
		fail("the built document has defects %v %v", defects, err)
	}
	if text, err := built.PageText(0); err != nil || !strings.Contains(text, "Embedded") {
		fail("the built text is %q %v", text, err)
	}
	if x0, y0, x1, y1, err := built.PageBox(0, tp.CropBox); err != nil || x0 != 0 || y0 != 0 || x1 != 150 || y1 != 150 {
		fail("the crop box is %v %v %v %v %v", x0, y0, x1, y1, err)
	}
	bitmap, err := built.Render(0, 1.0, tp.Rgb8)
	must(err)
	defer bitmap.Close()
	if ink(bitmap.Pixels()) == 0 {
		fail("the embedded face draws")
	}
	if fit, err := tp.FitView(); err != nil || fit.Kind != tp.Fit {
		fail("the engine fits by default: %v %v", fit, err)
	}
}

// ---- the command stream -----------------------------------------------------
//
// The stream of a page holding every kind of record -- the C ABI's test page
// (crates/tinker-pdf-ffi/src/commands/tests.rs, every_kind_page), built again
// from the face and the profile the checkout holds -- decoded through the
// binding's own types and written back to the facade's bytes, beside a stream
// of the three kinds a page reaches only past a budget or through a recording
// that composites colorants, pushed by hand. Between the two, every kind but
// the end. A recording device hears every kind in the stream's order, the
// tables' accessors are their records' fields, the replay through the engine
// draws what the page's render draws, and bytes that are no stream are
// refused with the reader's sentence.

// check fails the leg unless ok.
func check(ok bool, format string, args ...any) {
	if !ok {
		fail("command stream: "+format, args...)
	}
}

// streamObject is a stream object of dictionary holding data.
func streamObject(dictionary string, data []byte) []byte {
	out := fmt.Appendf(nil, "<< %s /Length %d >>\nstream\n", dictionary, len(data))
	out = append(out, data...)
	return append(out, "\nendstream"...)
}

// pdf is a document of objects, numbered from one, the first its catalog,
// with its cross-reference table.
func pdf(objects [][]byte) []byte {
	out := []byte("%PDF-1.7\n")
	offsets := make([]int, len(objects))
	for i, object := range objects {
		offsets[i] = len(out)
		out = append(out, fmt.Sprintf("%d 0 obj\n", i+1)...)
		out = append(out, object...)
		out = append(out, "\nendobj\n"...)
	}
	xref := len(out)
	out = append(out, fmt.Sprintf("xref\n0 %d\n0000000000 65535 f \n", len(objects)+1)...)
	for _, offset := range offsets {
		out = append(out, fmt.Sprintf("%010d 00000 n \n", offset)...)
	}
	return append(out, fmt.Sprintf("trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n",
		len(objects)+1, xref)...)
}

// everyKindPage is the 200 x 160 page whose stream holds every kind a page's
// stream is written with: a state with stored parameters, a black
// generation, a transfer function and a halftone; a clip and an axial
// shading in an ICC space; a fill in a /Separation space and one in a Lab
// space whose range is written backwards; text in an embedded face and in a
// font no name resolves; an image XObject and an inline image; marked
// content, a group under a luminosity soft mask then cleared; a tiling
// pattern, a form with resources of its own, a form that draws itself, and a
// square annotation with an appearance.
func everyKindPage(serif, profile []byte) []byte {
	widths := strings.TrimSpace(strings.Repeat("500 ", 95))
	content := []byte("q /Fancy gs 0 0 1 RG 2 w 10 10 m 100 10 l S 1 0 0 rg 10 20 30 20 re f Q\n" +
		"q 10 40 40 30 re W n /Sh0 sh Q\n" +
		"q /CS0 cs 0.7 scn 60 20 20 20 re f Q\n" +
		"q /Lab0 cs 50 10 10 sc 90 20 10 10 re f Q\n" +
		"BT /F0 12 Tf 10 120 Td (Glyphs) Tj ET\n" +
		"BT /Nope 12 Tf 10 140 Td (x) Tj ET\n" +
		"q 20 0 0 20 120 20 cm /Im0 Do Q\n" +
		"q 10 0 0 10 150 20 cm BI /W 2 /H 2 /CS /G /BPC 8 ID ")
	content = append(content, 0x00, 0x55, 0xAA, 0xFF)
	content = append(content, " EI Q\n"+
		"/Span << /MCID 0 >> BDC 0 g 120 60 5 5 re f EMC\n"+
		"q /Masked gs 1 0 0 1 120 80 cm /G Do /NoMask gs 0 1 0 rg 0 0 5 5 re f Q\n"+
		"q /Pattern cs /P0 scn 150 60 30 30 re f Q\n"+
		"q 1 0 0 1 150 100 cm /Fm0 Do Q\n"+
		"/Rec Do\n"...)
	rgb := []byte{255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0}
	return pdf([][]byte{
		[]byte("<< /Type /Catalog /Pages 2 0 R >>"),
		[]byte("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
		[]byte("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] /Resources 4 0 R " +
			"/Contents 5 0 R /Annots [17 0 R] >>"),
		[]byte("<< /Font << /F0 6 0 R >> " +
			"/XObject << /Im0 9 0 R /G 10 0 R /Fm0 11 0 R /Rec 12 0 R >> " +
			"/Shading << /Sh0 13 0 R >> /Pattern << /P0 15 0 R >> " +
			"/ColorSpace << " +
			"/CS0 [/Separation /Spot /DeviceRGB " +
			"<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>] " +
			"/Lab0 [/Lab << /WhitePoint [0.9505 1 1.089] /Range [100 -100 -100 100] >>] >> " +
			"/ExtGState << " +
			"/Fancy << /RI /Saturation " +
			"/BG << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> " +
			"/TR << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >> " +
			"/HT << /Type /Halftone /HalftoneType 1 /Frequency 60 /Angle 45 " +
			"/SpotFunction /Round >> >> " +
			"/Masked << /SMask << /Type /Mask /S /Luminosity /G 16 0 R >> >> " +
			"/NoMask << /SMask /None >> >> >>"),
		streamObject("", content),
		[]byte("<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 " +
			"/LastChar 126 /Widths [" + widths + "] /FontDescriptor 7 0 R " +
			"/Encoding /WinAnsiEncoding >>"),
		[]byte("<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 " +
			"/FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 " +
			"/CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"),
		streamObject(fmt.Sprintf("/Length1 %d", len(serif)), serif),
		streamObject("/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB "+
			"/BitsPerComponent 8", rgb),
		streamObject("/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >>",
			[]byte("0 0.8 0 rg 0 0 20 20 re f")),
		streamObject("/Type /XObject /Subtype /Form /BBox [0 0 40 40] "+
			"/Resources << /XObject << /ImF 9 0 R >> >>", []byte("q 10 0 0 10 0 0 cm /ImF Do Q")),
		streamObject("/Type /XObject /Subtype /Form /BBox [0 0 10 10] "+
			"/Resources << /XObject << /Rec 12 0 R >> >>", []byte("0 0 1 rg 0 0 2 2 re f /Rec Do")),
		[]byte("<< /ShadingType 2 /ColorSpace [/ICCBased 14 0 R] /Coords [10 0 50 0] " +
			"/Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> " +
			"/Extend [true true] >>"),
		streamObject("/N 3", profile),
		streamObject("/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 "+
			"/YStep 10 /Resources << >>", []byte("1 0 0 rg 0 0 5 5 re f")),
		streamObject("/Type /XObject /Subtype /Form /BBox [0 0 200 160] "+
			"/Group << /S /Transparency /CS /DeviceGray >>", []byte("0.5 g 0 0 200 160 re f")),
		[]byte("<< /Type /Annot /Subtype /Square /Rect [150 120 190 150] /AP << /N 18 0 R >> >>"),
		streamObject("/Type /XObject /Subtype /Form /BBox [0 0 40 30]", []byte("0 0 1 RG 2 w 1 1 38 28 re S")),
	})
}

// rebuilt is fields, each made again from its tag and value through the
// binding's constructors, and checked equal to what it was made from.
func rebuilt(fields []tp.CommandField) []tp.CommandField {
	again := make([]tp.CommandField, len(fields))
	for i, field := range fields {
		switch field.Tag {
		case tp.FieldNothing:
			again[i] = tp.NothingField()
		case tp.FieldUnsigned:
			again[i] = tp.UnsignedField(field.Natural)
		case tp.FieldSigned:
			again[i] = tp.SignedField(field.Integer)
		case tp.FieldReal:
			again[i] = tp.RealField(field.Real)
		case tp.FieldBytes:
			again[i] = tp.BytesField(field.Bytes)
		case tp.FieldReals:
			again[i] = tp.RealsField(field.Reals)
		case tp.FieldNaturals:
			again[i] = tp.NaturalsField(field.Naturals)
		case tp.FieldStrings:
			again[i] = tp.StringsField(field.Strings)
		default:
			fail("command stream: a field of tag %v", field.Tag)
		}
		check(again[i].Equal(field), "a field rebuilds: %v", field)
	}
	return again
}

// natural is field at's unsigned value.
func natural(record tp.CommandRecord, at int) uint64 {
	field := record.Fields[at]
	check(field.Tag == tp.FieldUnsigned, "%v's field %d is %v", record.Kind, at, field)
	return field.Natural
}

// bytesOf is field at's bytes.
func bytesOf(record tp.CommandRecord, at int) []byte {
	field := record.Fields[at]
	check(field.Tag == tp.FieldBytes, "%v's field %d is %v", record.Kind, at, field)
	return field.Bytes
}

// sameReals is whether two lists of reals are one list, each real by its bits.
func sameReals(a, b []float64) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if math.Float64bits(a[i]) != math.Float64bits(b[i]) {
			return false
		}
	}
	return true
}

// kindsOf is the kinds of records, in their order.
func kindsOf(records []tp.CommandRecord) []tp.CommandKind {
	kinds := make([]tp.CommandKind, len(records))
	for i, record := range records {
		kinds[i] = record.Kind
	}
	return kinds
}

// sameKinds is whether two lists of kinds are one list.
func sameKinds(a, b []tp.CommandKind) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// sameFields is whether two lists of fields are one list.
func sameFields(a, b []tp.CommandField) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if !a[i].Equal(b[i]) {
			return false
		}
	}
	return true
}

func commandStreams(root string) {
	serif, err := os.ReadFile(filepath.Join(root, "crates", "tinker-pdf-font", "data", "liberation",
		"LiberationSerif-Regular.ttf"))
	must(err)
	profile, err := os.ReadFile(filepath.Join(root, "fuzz", "corpus", "icc_profile", "rgb-gamma-curve.icc"))
	must(err)
	everyKind, err := tp.Open(everyKindPage(serif, profile))
	must(err)
	written, err := everyKind.CommandStream(0)
	must(err)
	pageWidth, pageHeight, err := everyKind.PageSize(0)
	must(err)
	page, err := everyKind.Render(0, 1.0, tp.Rgb8)
	must(err)
	direct, directWidth, directHeight := page.Pixels(), page.Width(), page.Height()
	page.Close()
	expectStatus(func() error { _, err := everyKind.CommandStream(1); return err }, tp.StatusNoSuchPage,
		"a stream of a page past the end")
	everyKind.Close()

	stream, err := tp.ParseCommandStream(written)
	must(err)
	defer stream.Close()
	back, err := stream.ToBytes()
	must(err)
	check(bytes.Equal(back, written), "the stream writes back its own bytes")
	index, err := stream.PageIndex()
	check(err == nil && index == 0, "the stream is page 0's, not %d (%v)", index, err)
	width, height, err := stream.Size()
	check(err == nil && width == pageWidth && height == pageHeight, "the stream's page is the page's size")

	records, err := stream.Records()
	must(err)
	check(uint32(len(records)) == stream.Count(), "every record, counted")
	check(records[0].Kind == tp.CommandPage, "the page record leads: %v", records[0].Kind)
	for _, record := range records {
		command, err := stream.Next()
		must(err)
		check(command.Kind.Known(), "kind %d is one this build knows", record.Number)
		at, err := stream.CommandAt(record.Index)
		must(err)
		check(command == at, "the cursor reads record %d as its place does", record.Index)
		check(command.Kind == record.Kind && command.Number == record.Number &&
			command.FieldCount == uint32(len(record.Fields)), "record %d's command", record.Index)
	}
	for twice := 0; twice < 2; twice++ {
		end, err := stream.Next()
		must(err)
		check(end.Kind == tp.CommandEnd && end.Number == 0 && end.Index == uint32(len(records)) &&
			end.FieldCount == 0, "the end, after the last record: %+v", end)
	}
	must(stream.Rewind())
	first, err := stream.Next()
	must(err)
	zeroth, err := stream.CommandAt(0)
	must(err)
	check(first == zeroth, "a rewind puts the cursor back on the page record")
	expectStatus(func() error { _, err := stream.CommandAt(uint32(len(records))); return err },
		tp.StatusBadArgument, "a record past the end")
	expectStatus(func() error { _, err := stream.FieldAt(0, uint32(len(records[0].Fields))); return err },
		tp.StatusBadArgument, "a field past the page record's last")

	encoder, err := tp.NewCommandEncoder()
	must(err)
	for _, record := range records {
		must(encoder.Push(record.Kind, rebuilt(record.Fields)))
	}
	again, err := encoder.Finish()
	must(err)
	check(bytes.Equal(again, written),
		"the stream decoded and written back through the binding's types is the facade's bytes")
	encoder.Close()

	onThePage := map[tp.CommandKind]bool{}
	for _, record := range records {
		onThePage[record.Kind] = true
	}
	byHand := map[tp.CommandKind]bool{tp.CommandColorants: true, tp.CommandGroupBudgetSpent: true,
		tp.CommandNestedBudgetSpent: true}
	for _, kind := range tp.CommandKinds() {
		if kind != tp.CommandEnd && !byHand[kind] {
			check(onThePage[kind], "the page's stream holds a %v record", kind)
		}
	}

	colorants := []tp.CommandField{
		tp.UnsignedField(1),
		tp.StringsField([][]byte{[]byte("Spot"), []byte("Other")}),
		tp.BytesField([]byte{128, 64}),
		tp.NaturalsField([]uint64{1}),
		tp.BytesField([]byte{0, 0, 0, 255}),
		tp.NothingField(),
		tp.BytesField([]byte{10, 20, 30, 40}),
	}
	hands, err := tp.NewCommandEncoder()
	must(err)
	must(hands.PushRecord(records[0]))
	must(hands.Push(tp.CommandColorants, colorants))
	must(hands.Push(tp.CommandGroupBudgetSpent, nil))
	must(hands.Push(tp.CommandNestedBudgetSpent, nil))
	hand, err := hands.Finish()
	must(err)
	expectStatus(func() error { return hands.Push(tp.CommandEnd, nil) }, tp.StatusBadArgument, "the end pushed")
	expectStatus(func() error { return hands.Push(tp.CommandKind(99), nil) }, tp.StatusBadArgument,
		"a kind the C ABI does not declare")
	expectStatus(func() error { return hands.Push(tp.CommandFunction, []tp.CommandField{{Tag: tp.CommandFieldTag(9)}}) },
		tp.StatusBadArgument, "a tag the C ABI does not declare")
	twice, err := hands.Finish()
	must(err)
	check(bytes.Equal(twice, hand), "an encoder keeps its records, and a refused push adds none")
	hands.Close()
	lonely, err := tp.NewCommandEncoder()
	must(err)
	must(lonely.Push(tp.CommandGroupBudgetSpent, nil))
	expectStatus(func() error { _, err := lonely.Finish(); return err }, tp.StatusBadArgument,
		"records with no page record")
	lonely.Close()

	heard := map[tp.CommandKind]bool{}
	reread, err := tp.ParseCommandStream(hand)
	must(err)
	defer reread.Close()
	readBack, err := reread.Records()
	must(err)
	check(sameKinds(kindsOf(readBack), []tp.CommandKind{tp.CommandPage, tp.CommandColorants,
		tp.CommandGroupBudgetSpent, tp.CommandNestedBudgetSpent}),
		"the records pushed by hand read back in order: %v", kindsOf(readBack))
	check(sameFields(readBack[0].Fields, records[0].Fields) && sameFields(readBack[1].Fields, colorants) &&
		len(readBack[2].Fields) == 0 && len(readBack[3].Fields) == 0, "the records pushed by hand read back as pushed")
	major, minor, err := reread.Version()
	must(err)
	pageMajor, pageMinor, err := stream.Version()
	must(err)
	check(major == pageMajor && minor == pageMinor,
		"the page's stream is at the version the encoder writes, this build's")
	writer, err := tp.NewCommandEncoder()
	must(err)
	for _, record := range readBack {
		must(writer.Push(record.Kind, rebuilt(record.Fields)))
	}
	rewritten, err := writer.Finish()
	must(err)
	check(bytes.Equal(rewritten, hand), "and write back through the binding's types")
	writer.Close()
	for _, each := range []*tp.CommandStream{stream, reread} {
		heardHere := &recorder{}
		must(each.Replay(heardHere))
		all, err := each.Records()
		must(err)
		check(sameKinds(heardHere.seen, kindsOf(all)), "the device receives every record in the decoder's order")
		for _, kind := range heardHere.seen {
			heard[kind] = true
		}
	}
	for _, kind := range tp.CommandKinds() {
		if kind != tp.CommandEnd {
			check(heard[kind], "the device heard a %v record", kind)
		}
	}

	glyphs := &glyphRuns{}
	must(stream.Replay(glyphs))
	runs := 0
	for _, record := range records {
		if record.Kind == tp.CommandGlyphRun {
			runs++
		}
	}
	check(glyphs.runs == runs, "a device naming one method hears only that kind")
	err = stream.Replay(refusing{})
	check(errors.Is(err, errRefused), "the device's error ends the replay and is returned: %v", err)

	outlines, decodes := 0, 0
	for _, record := range records {
		switch record.Kind {
		case tp.CommandOutline:
			outline, err := stream.OutlineAt(record.Index)
			must(err)
			check(outline.Scope == natural(record, 0) && outline.FontID == natural(record, 1) &&
				uint64(outline.Code) == natural(record, 2) && outline.Found == (natural(record, 3) == 1) &&
				bytes.Equal(outline.Operators, bytesOf(record, 4)) &&
				sameReals(outline.Numbers, record.Fields[5].Reals),
				"outline record %d is its fields", record.Index)
			outlines++
		case tp.CommandImageDecode:
			image, err := stream.ImageAt(record.Index)
			must(err)
			check(image.Image == natural(record, 0) && uint64(image.Reduction) == natural(record, 1) &&
				uint64(image.Answer) == natural(record, 2),
				"image decode record %d is its image, reduction and answer", record.Index)
			switch image.Answer {
			case tp.AnswerImage:
				held := record.Fields[8].Tag == tp.FieldBytes
				twoColour, colours := []byte{}, make([]byte, 8)
				if held {
					twoColour, colours = bytesOf(record, 8), bytesOf(record, 9)
				}
				check(uint64(image.Width) == natural(record, 4) && uint64(image.Height) == natural(record, 5) &&
					bytes.Equal(image.RGB, bytesOf(record, 6)) && bytes.Equal(image.Alpha, bytesOf(record, 7)) &&
					bytes.Equal(image.TwoColour, twoColour) && bytes.Equal(image.Colours[:], colours) &&
					image.Stencil == (natural(record, 10) == 1) && image.Interpolate == (natural(record, 11) == 1),
					"image decode record %d is its samples and colours", record.Index)
				window := record.Fields[15]
				if window.Tag == tp.FieldNaturals {
					edges := window.Naturals
					check(image.Window != nil && uint64(image.Window.X) == edges[0] &&
						uint64(image.Window.Y) == edges[1] && uint64(image.Window.Width) == edges[2] &&
						uint64(image.Window.Height) == edges[3], "image decode record %d's window", record.Index)
				} else {
					check(image.Window == nil, "image decode record %d is all of its decode", record.Index)
				}
			case tp.AnswerRefused:
				check(image.Codec == string(bytesOf(record, 3)), "a refused decode names its codec")
			}
			decodes++
		}
	}
	check(outlines > 0 && decodes > 0, "%d outlines and %d decodes", outlines, decodes)
	expectStatus(func() error { _, err := stream.OutlineAt(0); return err }, tp.StatusBadArgument,
		"an outline asked of the page record")
	expectStatus(func() error { _, err := stream.ImageAt(0); return err }, tp.StatusBadArgument,
		"a decode asked of the page record")

	replayed, err := stream.Render(1.0, tp.Rgb8)
	must(err)
	check(replayed.Width() == directWidth && replayed.Height() == directHeight &&
		bytes.Equal(replayed.Pixels(), direct), "the replay draws what a render of the page draws")
	replayed.Close()

	for _, refused := range [][]byte{{}, []byte("not a command stream"), written[:len(written)-1]} {
		expectStatus(func() error {
			parsed, err := tp.ParseCommandStream(refused)
			parsed.Close()
			return err
		}, tp.StatusBadArgument, "bytes that are no stream")
	}
	_, err = tp.ParseCommandStream([]byte("plainly not one"))
	var refusal *tp.Error
	check(errors.As(err, &refusal) && strings.Contains(refusal.Message, "not a command stream"),
		"the reader's sentence: %v", err)
	fmt.Printf("command stream: %d records, every kind, written back to the facade's bytes\n", len(records))
}

// recorder notes every record it is handed, each by the method its kind
// names: the command stream leg's recording device.
type recorder struct {
	seen []tp.CommandKind
}

func (r *recorder) note(kind tp.CommandKind, record tp.CommandRecord) error {
	if record.Kind != kind {
		return fmt.Errorf("the method for %v was handed a %v", kind, record.Kind)
	}
	r.seen = append(r.seen, kind)
	return nil
}

func (r *recorder) Function(c tp.CommandRecord) error  { return r.note(tp.CommandFunction, c) }
func (r *recorder) Halftone(c tp.CommandRecord) error  { return r.note(tp.CommandHalftone, c) }
func (r *recorder) Stored(c tp.CommandRecord) error    { return r.note(tp.CommandStored, c) }
func (r *recorder) Colorants(c tp.CommandRecord) error { return r.note(tp.CommandColorants, c) }
func (r *recorder) Source(c tp.CommandRecord) error    { return r.note(tp.CommandSource, c) }
func (r *recorder) State(c tp.CommandRecord) error     { return r.note(tp.CommandState, c) }
func (r *recorder) BeginText(c tp.CommandRecord) error { return r.note(tp.CommandBeginText, c) }
func (r *recorder) GlyphRun(c tp.CommandRecord) error  { return r.note(tp.CommandGlyphRun, c) }
func (r *recorder) EndText(c tp.CommandRecord) error   { return r.note(tp.CommandEndText, c) }
func (r *recorder) FillPath(c tp.CommandRecord) error  { return r.note(tp.CommandFillPath, c) }
func (r *recorder) StrokePath(c tp.CommandRecord) error {
	return r.note(tp.CommandStrokePath, c)
}
func (r *recorder) ClipPath(c tp.CommandRecord) error  { return r.note(tp.CommandClipPath, c) }
func (r *recorder) SaveState(c tp.CommandRecord) error { return r.note(tp.CommandSaveState, c) }
func (r *recorder) RestoreState(c tp.CommandRecord) error {
	return r.note(tp.CommandRestoreState, c)
}
func (r *recorder) DrawImage(c tp.CommandRecord) error { return r.note(tp.CommandDrawImage, c) }
func (r *recorder) DrawShading(c tp.CommandRecord) error {
	return r.note(tp.CommandDrawShading, c)
}
func (r *recorder) BeginMarkedContent(c tp.CommandRecord) error {
	return r.note(tp.CommandBeginMarkedContent, c)
}
func (r *recorder) EndMarkedContent(c tp.CommandRecord) error {
	return r.note(tp.CommandEndMarkedContent, c)
}
func (r *recorder) BeginForm(c tp.CommandRecord) error  { return r.note(tp.CommandBeginForm, c) }
func (r *recorder) EndForm(c tp.CommandRecord) error    { return r.note(tp.CommandEndForm, c) }
func (r *recorder) BeginGroup(c tp.CommandRecord) error { return r.note(tp.CommandBeginGroup, c) }
func (r *recorder) EndGroup(c tp.CommandRecord) error   { return r.note(tp.CommandEndGroup, c) }
func (r *recorder) BeginSoftMask(c tp.CommandRecord) error {
	return r.note(tp.CommandBeginSoftMask, c)
}
func (r *recorder) EndSoftMask(c tp.CommandRecord) error {
	return r.note(tp.CommandEndSoftMask, c)
}
func (r *recorder) ClearSoftMask(c tp.CommandRecord) error {
	return r.note(tp.CommandClearSoftMask, c)
}
func (r *recorder) GroupBudgetSpent(c tp.CommandRecord) error {
	return r.note(tp.CommandGroupBudgetSpent, c)
}
func (r *recorder) RecursionRefused(c tp.CommandRecord) error {
	return r.note(tp.CommandRecursionRefused, c)
}
func (r *recorder) NestedBudgetSpent(c tp.CommandRecord) error {
	return r.note(tp.CommandNestedBudgetSpent, c)
}
func (r *recorder) Page(c tp.CommandRecord) error       { return r.note(tp.CommandPage, c) }
func (r *recorder) Annotation(c tp.CommandRecord) error { return r.note(tp.CommandAnnotation, c) }
func (r *recorder) MissingFont(c tp.CommandRecord) error {
	return r.note(tp.CommandMissingFont, c)
}
func (r *recorder) RepairedSpace(c tp.CommandRecord) error {
	return r.note(tp.CommandRepairedSpace, c)
}
func (r *recorder) Scope(c tp.CommandRecord) error   { return r.note(tp.CommandScope, c) }
func (r *recorder) Outline(c tp.CommandRecord) error { return r.note(tp.CommandOutline, c) }
func (r *recorder) FontIdentity(c tp.CommandRecord) error {
	return r.note(tp.CommandFontIdentity, c)
}
func (r *recorder) Profile(c tp.CommandRecord) error    { return r.note(tp.CommandProfile, c) }
func (r *recorder) ColorSpace(c tp.CommandRecord) error { return r.note(tp.CommandColorSpace, c) }
func (r *recorder) Image(c tp.CommandRecord) error      { return r.note(tp.CommandImage, c) }
func (r *recorder) InlineImage(c tp.CommandRecord) error {
	return r.note(tp.CommandInlineImage, c)
}
func (r *recorder) ImageDecode(c tp.CommandRecord) error {
	return r.note(tp.CommandImageDecode, c)
}
func (r *recorder) ShadingDefinition(c tp.CommandRecord) error {
	return r.note(tp.CommandShadingDefinition, c)
}
func (r *recorder) Shading(c tp.CommandRecord) error { return r.note(tp.CommandShading, c) }
func (r *recorder) Pattern(c tp.CommandRecord) error { return r.note(tp.CommandPattern, c) }
func (r *recorder) Cell(c tp.CommandRecord) error    { return r.note(tp.CommandCell, c) }
func (r *recorder) Unknown(c tp.CommandRecord) error {
	return fmt.Errorf("a record of kind %d, which this build does not know", c.Number)
}

// glyphRuns is a device naming one method, which hears only that kind.
type glyphRuns struct {
	tp.IgnoreCommands
	runs int
}

func (g *glyphRuns) GlyphRun(record tp.CommandRecord) error {
	if record.Kind != tp.CommandGlyphRun {
		return fmt.Errorf("GlyphRun was handed a %v", record.Kind)
	}
	g.runs++
	return nil
}

// errRefused is what refusing's one method returns.
var errRefused = errors.New("the device refuses")

// refusing is a device whose error ends the replay.
type refusing struct {
	tp.IgnoreCommands
}

func (refusing) BeginText(tp.CommandRecord) error { return errRefused }
