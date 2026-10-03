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
	"fmt"
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
	fmt.Println("GO-SMOKE: RAN, rendered and inked, every declaration called")
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
