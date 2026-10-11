// The parity scripts, run through the Go binding.
//
//	go run ./cmd/parity <path to testdata/form-fields.pdf>
//
// The same scripts crates/tinker-pdf/examples/write_parity.rs runs against
// the facade and every other binding runs through its own surface; `cargo
// xtask bindings-parity` requires every surface to print the same SHA-256s.
// Ruling 11 is what makes that the right test: a binding projects the facade
// 1:1 and adds no logic of its own, so surfaces disagreeing means one of them
// added something. The read texts are specified byte for byte in the facade
// example's module documentation.
//
// Every written artefact goes through the engine's own strict structural
// validator before its hash is printed, because byte-identical outputs
// agreeing tells you nothing if all of them are wrong.
package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	tp "github.com/ravindu-rev/tinker-pdf/bindings/go"
)

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "GO-PARITY: FAILED:", err)
		os.Exit(1)
	}
}

func check(ok bool, message string) {
	if !ok {
		fmt.Fprintln(os.Stderr, "GO-PARITY: FAILED:", message)
		os.Exit(1)
	}
}

func sum(data []byte) string {
	digest := sha256.Sum256(data)
	return hex.EncodeToString(digest[:])
}

// report validates an artefact, then prints the line bindings-parity reads.
func report(script string, data []byte) {
	document, err := tp.Open(data)
	must(err)
	defects, err := document.Validate()
	must(err)
	document.Close()
	check(len(defects) == 0, fmt.Sprintf("%s: the artefact does not pass the strict validator: %v", script, defects))
	fmt.Printf("WROTE sha256=%s surface=go script=%s bytes=%d\n", sum(data), script, len(data))
}

func reportRead(script, text string) {
	if os.Getenv("TINKER_PARITY_DUMP") != "" {
		fmt.Print(text)
	}
	fmt.Printf("READ sha256=%s surface=go script=%s bytes=%d\n", sum([]byte(text)), script, len(text))
}

// The eight-by-eight grey image every surface builds, from the same formula.
func parityImage() []byte {
	image := make([]byte, 64)
	for i := range image {
		image[i] = byte((i * 7) % 256)
	}
	return image
}

// options is the engine's default options but the mode, which is the
// script's choice.
func options(mode tp.WriteMode) tp.WriteOptions {
	options, err := tp.DefaultWriteOptions()
	must(err)
	options.Mode = mode
	return options
}

func fillAndSave(fixture []byte) []byte {
	document, err := tp.Open(fixture)
	must(err)
	editor, err := document.Editor()
	must(err)
	// The editor holds its own reference to the object store.
	document.Close()
	defer editor.Close()

	skipped, err := editor.FillField("name", "Ada Lovelace")
	must(err)
	check(len(skipped) == 1, "the /Rect-less widget must be reported, not swallowed")
	check(skipped[0].Message == "7 0 R: no usable /Rect (12.5.2)", "unexpected report: "+skipped[0].Message)
	check(skipped[0].Object == 7 && skipped[0].Generation == 0, "the report lost the widget it names")
	clean, err := editor.FillField("notes", "every surface writes this")
	must(err)
	check(len(clean) == 0, "the control field is well formed, so nothing is skipped")
	must(editor.SetCheckbox("agree", true))
	must(editor.SelectRadio("colour", "red"))
	saved, err := editor.Save(options(tp.Incremental))
	must(err)
	return saved
}

func buildADocument() []byte {
	builder, err := tp.NewBuilder()
	must(err)
	defer builder.Close()
	must(builder.AddBaseFont([]byte("F1"), []byte("Helvetica")))
	must(builder.AddImage([]byte("Im1"), tp.ImageGray8, 8, 8, parityImage()))

	one, err := builder.BeginPage(200, 200)
	must(err)
	must(one.Text([]byte("F1"), 14, 20, 170, "Page one"))
	must(one.FillRect(20, 40, 60, 60, 0.25))
	must(one.Image([]byte("Im1"), 100, 40, 60, 60))
	must(builder.PushPage(one))
	one.Close()

	two, err := builder.BeginPage(200, 200)
	must(err)
	must(two.Text([]byte("F1"), 14, 20, 170, "Page two"))
	must(builder.PushPage(two))
	two.Close()

	must(builder.SetInfo([]byte("Title"), "tinker-pdf write parity"))
	first, err := tp.NewOutlineEntry("Page one")
	must(err)
	must(first.SetTarget(tp.PageTarget(0, tp.View{Kind: tp.Fit})))
	second, err := tp.NewOutlineEntry("Page two")
	must(err)
	must(second.SetTarget(tp.PageTarget(1, tp.View{Kind: tp.Fit})))
	must(builder.SetOutline(first, second))
	first.Close()
	second.Close()
	built, err := builder.Finish()
	must(err)
	return built
}

func created() tp.Date {
	zero := int32(0)
	return tp.Date{Year: 2026, Month: 10, Day: 3, Hour: 12, UTCOffsetMinutes: &zero}
}

var packet = []byte("<x:xmpmeta xmlns:x='adobe:ns:meta/'/>")

func documentOps(outline []byte) []byte {
	document, err := tp.Open(outline)
	must(err)
	editor, err := document.Editor()
	must(err)
	document.Close()
	defer editor.Close()
	prefix := "A-"
	must(editor.SetPageLabels([]tp.PageLabelRange{
		{FirstPage: 0, Style: tp.LabelRomanLower, Start: 1},
		{FirstPage: 2, Style: tp.LabelDecimal, Prefix: &prefix, Start: 1},
	}))
	description, mime, when := "the numbers", "text/csv", created()
	_, err = editor.AttachFile(tp.EmbeddedFile{
		Name: "data.csv", Filename: "data.csv", Description: &description, MimeType: &mime,
		Created: &when, Data: []byte("a,b\n1,2\n"),
	})
	must(err)
	sync, err := editor.SetInfo(tp.InfoTitle, "Document operations")
	must(err)
	check(sync == tp.SyncAlone, "no XMP packet yet, so the title is alone")
	_, err = editor.SetInfo(tp.InfoAuthor, "tinker-pdf")
	must(err)
	_, err = editor.SetInfoDate(tp.InfoCreationDate, created())
	must(err)
	_, err = editor.SetTrapped(tp.TrappedFalse)
	must(err)
	sync, err = editor.SetXMPMetadata(packet)
	must(err)
	check(sync == tp.SyncOtherHalfUnchanged, "/Info has entries the packet was not checked against")
	must(editor.SetPageBoundary(0, tp.TrimBox, 10, 10, 585, 832))
	must(editor.SetPageBoundary(1, tp.BleedBox, 0, 0, 595, 842))
	only, err := tp.NewOutlineEntry("Only entry")
	must(err)
	must(only.SetTarget(tp.PageTarget(3, tp.View{Kind: tp.FitH, Top: tp.Number(700)})))
	must(editor.SetOutline(only))
	only.Close()
	saved, err := editor.Save(options(tp.Rewrite))
	must(err)
	return saved
}

// saveOptions is the options a save takes, every one the C ABI carries but
// encryption away from its default, after two edits that give them something
// to act on: the deleted page is what garbage collection drops, and the
// appended operators are the one stream nobody has encoded, which is what
// compression compresses. save-linearized is the same save linearized; the
// linearizer sets object streams and compression aside, so it is a second
// script.
func saveOptions(operated []byte, linearize bool) []byte {
	document, err := tp.Open(operated)
	must(err)
	editor, err := document.Editor()
	must(err)
	document.Close()
	defer editor.Close()
	must(editor.DeletePage(1))
	must(editor.AppendContent(0, []byte("0 0 m 100 100 l S")))
	options := options(tp.Rewrite)
	options.Linearize = linearize
	options.VersionMajor, options.VersionMinor = 2, 0
	options.ObjectStreams, options.Compress, options.GarbageCollect = true, true, true
	saved, err := editor.Save(options)
	must(err)
	return saved
}

var removalNames = map[tp.Removal]string{
	tp.RemovedJavaScript: "javascript", tp.RemovedDocumentJavaScript: "document-javascript",
	tp.RemovedCalculationOrder: "calculation-order", tp.RemovedXfaForm: "xfa-form",
	tp.RemovedAction: "action", tp.RemovedEmbeddedFileTree: "embedded-file-tree",
	tp.RemovedEmbeddedFile: "embedded-file", tp.RemovedInfo: "info", tp.RemovedMetadata: "metadata",
}

func sanitise(operated []byte) ([]byte, string) {
	document, err := tp.Open(operated)
	must(err)
	editor, err := document.Editor()
	must(err)
	document.Close()
	defer editor.Close()
	report, err := editor.Sanitise(tp.Sanitise{JavaScript: true, Actions: true, EmbeddedFiles: true, Metadata: true})
	must(err)
	var text strings.Builder
	for _, entry := range report.Removed {
		holder := "trailer"
		if entry.Holder != nil {
			holder = fmt.Sprintf("%d.%d", entry.Holder.Object, entry.Holder.Generation)
		}
		steps := make([]string, 0, len(entry.Path))
		for _, step := range entry.Path {
			if step.IsIndex {
				steps = append(steps, "i:"+strconv.FormatUint(step.Index, 10))
			} else {
				steps = append(steps, "k:"+hex.EncodeToString(step.Key))
			}
		}
		fmt.Fprintf(&text, "removed %s %s %s %s\n", removalNames[entry.What], holder, strings.Join(steps, "/"), bytesToken(entry.Action))
	}
	for _, entry := range report.Deleted {
		fmt.Fprintf(&text, "deleted %s %d.%d %s\n", removalNames[entry.What], entry.Object.Object, entry.Object.Generation, bytesToken(entry.Action))
	}
	saved, err := editor.Save(options(tp.Rewrite))
	must(err)
	return saved, text.String()
}

func linkedDocument() []byte {
	builder, err := tp.NewBuilder()
	must(err)
	defer builder.Close()
	must(builder.AddBaseFont([]byte("F1"), []byte("Helvetica")))
	one, err := builder.BeginPage(200, 200)
	must(err)
	must(one.Text([]byte("F1"), 12, 20, 170, "Links"))
	must(one.Link(10, 10, 60, 30, tp.URITarget("https://example.org/parity")))
	must(one.Link(70, 10, 120.5, 30.25, tp.PageTarget(1, tp.View{Kind: tp.Xyz, Left: tp.Number(10), Zoom: tp.Number(1.5)})))
	must(builder.PushPage(one))
	one.Close()
	two, err := builder.BeginPage(200, 200)
	must(err)
	must(builder.PushPage(two))
	two.Close()
	must(builder.SetInfo([]byte("Title"), "Read surface — parity"))
	must(builder.SetInfo([]byte("Author"), ""))
	heading, err := tp.NewOutlineEntry("Part one")
	must(err)
	must(heading.SetOpen(true))
	chapter, err := tp.NewOutlineEntry("Chapter one")
	must(err)
	must(chapter.SetTarget(tp.PageTarget(1, tp.View{Kind: tp.FitH, Top: tp.Number(150)})))
	must(heading.AddChild(chapter))
	chapter.Close()
	elsewhere, err := tp.NewOutlineEntry("Elsewhere")
	must(err)
	must(elsewhere.SetTarget(tp.URITarget("https://example.org/")))
	must(builder.SetOutline(heading, elsewhere))
	heading.Close()
	elsewhere.Close()
	built, err := builder.Finish()
	must(err)
	return built
}

// The contract's tokens.
func textToken(value *string) string {
	if value == nil {
		return "-"
	}
	return "s:" + hex.EncodeToString([]byte(*value))
}

func bytesToken(value []byte) string {
	if value == nil {
		return "-"
	}
	return "b:" + hex.EncodeToString(value)
}

func number(value *float64) string {
	if value == nil {
		return "-"
	}
	return fmt.Sprintf("f:%016x", math.Float64bits(*value))
}

func plain(value float64) string { return number(&value) }

func reference(value *tp.Ref) string {
	if value == nil {
		return "-"
	}
	return fmt.Sprintf("%d.%d", value.Object, value.Generation)
}

func digest(value []byte) string {
	if value == nil {
		return "-"
	}
	return sum(value)
}

func viewToken(view tp.View) string {
	switch view.Kind {
	case tp.Xyz:
		return "xyz " + number(view.Left) + " " + number(view.Top) + " " + number(view.Zoom)
	case tp.FitH:
		return "fith " + number(view.Top)
	case tp.FitV:
		return "fitv " + number(view.Left)
	case tp.FitR:
		return "fitr " + number(view.Left) + " " + number(view.Bottom) + " " + number(view.Right) + " " + number(view.Top)
	case tp.FitB:
		return "fitb"
	case tp.FitBH:
		return "fitbh " + number(view.Top)
	case tp.FitBV:
		return "fitbv " + number(view.Left)
	default:
		return "fit"
	}
}

func destinationToken(d *tp.Destination) string {
	if d == nil {
		return "-"
	}
	switch d.Kind {
	case tp.DestinationExplicit:
		page := "-"
		if d.PageIndex != nil {
			page = strconv.FormatUint(uint64(*d.PageIndex), 10)
		}
		return "explicit " + page + " " + reference(d.PageRef) + " " + viewToken(d.View)
	case tp.DestinationNamed:
		return "named " + bytesToken(d.Bytes)
	default:
		return "uri " + bytesToken(d.Bytes)
	}
}

func actionToken(link tp.Link) string {
	switch link.Action {
	case tp.ActionAbsent:
		return "-"
	case tp.ActionGoTo:
		return "goto " + destinationToken(link.Destination)
	case tp.ActionGoToR:
		return "gotor " + bytesToken(link.ActionBytes) + " " + destinationToken(link.Destination)
	case tp.ActionURI:
		return "uri " + bytesToken(link.ActionBytes)
	case tp.ActionNamed:
		return "named " + bytesToken(link.ActionBytes)
	case tp.ActionLaunch:
		return "launch " + bytesToken(link.ActionBytes)
	default:
		return "other " + bytesToken(link.ActionBytes)
	}
}

func readDump(name string, document *tp.Document, out *strings.Builder) {
	line := func(format string, args ...any) { fmt.Fprintf(out, format+"\n", args...) }
	version, err := document.PDFVersion()
	must(err)
	line("document %s", name)
	line("version %s", textToken(&version))
	line("pages %d", document.PageCount())
	for _, key := range []struct {
		key  tp.InfoKey
		name string
	}{
		{tp.InfoTitle, "title"}, {tp.InfoAuthor, "author"}, {tp.InfoSubject, "subject"},
		{tp.InfoKeywords, "keywords"}, {tp.InfoCreator, "creator"}, {tp.InfoProducer, "producer"},
		{tp.InfoCreationDate, "creation-date"}, {tp.InfoModificationDate, "modification-date"},
	} {
		value, err := document.Info(key.key)
		must(err)
		line("info %s %s", key.name, textToken(value))
	}
	trapped, err := document.Trapped()
	must(err)
	line("trapped %s", map[tp.Trapped]string{
		tp.TrappedAbsent: "absent", tp.TrappedTrue: "true", tp.TrappedFalse: "false", tp.TrappedUnknown: "unknown",
	}[trapped])
	labels, err := document.PageLabels()
	must(err)
	for index := range labels {
		line("label %d %s", index, textToken(&labels[index]))
	}
	for index := uint32(0); index < document.PageCount(); index++ {
		for _, box := range []struct {
			boundary tp.PageBoundary
			name     string
		}{
			{tp.MediaBox, "media"}, {tp.CropBox, "crop"}, {tp.BleedBox, "bleed"}, {tp.TrimBox, "trim"}, {tp.ArtBox, "art"},
		} {
			x0, y0, x1, y1, err := document.PageBox(index, box.boundary)
			must(err)
			line("box %d %s %s %s %s %s", index, box.name, plain(x0), plain(y0), plain(x1), plain(y1))
		}
	}
	items, err := document.Outline()
	must(err)
	for _, item := range items {
		open := 0
		if item.Open {
			open = 1
		}
		title := item.Title
		line("outline %d %d %s %s", item.Depth, open, textToken(&title), destinationToken(item.Destination))
	}
	for index := uint32(0); index < document.PageCount(); index++ {
		links, err := document.Links(index)
		must(err)
		for _, link := range links {
			line("link %d %s %s %s %s %s %s", index, plain(link.X0), plain(link.Y0), plain(link.X1), plain(link.Y1),
				reference(link.Reference), actionToken(link))
		}
	}
	attachments, err := document.Attachments()
	must(err)
	for index := uint32(0); index < attachments.Count(); index++ {
		name, err := attachments.Name(index)
		must(err)
		filename, err := attachments.Filename(index)
		must(err)
		description, err := attachments.Description(index)
		must(err)
		size, err := attachments.Size(index)
		must(err)
		data, err := attachments.Data(index)
		if err != nil {
			data = nil
		}
		sizeToken := "-"
		if size != nil {
			sizeToken = strconv.FormatInt(*size, 10)
		}
		line("attachment %s %s %s %s %s", textToken(name), textToken(filename), textToken(description), sizeToken, digest(data))
	}
	attachments.Close()
	xmp, err := document.XMPMetadata()
	must(err)
	line("xmp %s", digest(xmp))
	warnings, err := document.Warnings()
	must(err)
	for _, warning := range warnings {
		message := warning.Message
		line("warning %d %s %s %s", warning.Offset, reference(warning.Object), warning.Kind, textToken(&message))
	}
}

func readSurface(outline, operated []byte) string {
	var out strings.Builder
	for _, document := range []struct {
		name  string
		bytes []byte
	}{
		{"shifted", append([]byte("JUNK\n"), outline...)},
		{"linked", linkedDocument()},
		{"operated", operated},
	} {
		opened, err := tp.Open(document.bytes)
		must(err)
		readDump(document.name, opened, &out)
		opened.Close()
	}
	return out.String()
}

var coverageNames = map[tp.Coverage]string{
	tp.CoverageWholeFile: "whole-file", tp.CoverageRevision: "revision", tp.CoverageSuspicious: "suspicious",
}

var chainNames = map[tp.Chain]string{
	tp.ChainAnchoredTo: "anchored-to", tp.ChainSelfSigned: "self-signed", tp.ChainIncomplete: "incomplete",
	tp.ChainBroken: "broken", tp.ChainNoAnchors: "no-anchors", tp.ChainNoSignerCertificate: "no-signer-certificate",
}

var weaknessNames = map[tp.Weakness]string{
	tp.WeaknessSha1Digest: "sha1-digest", tp.WeaknessSha1Signature: "sha1-signature",
	tp.WeaknessShortRsaKey: "short-rsa-key", tp.WeaknessCoversOnlyARevision: "covers-only-a-revision",
	tp.WeaknessCoverageSuspicious: "coverage-suspicious", tp.WeaknessOutsideValidity: "outside-validity",
}

func signatures(support string) string {
	var out strings.Builder
	// The name written down, the fixture it is made from, and its root. The
	// altered one is ecdsa-p256.pdf with its first `verdict path` changed to
	// `verdict PATH`: only its digest moves, which is what tells the digest
	// and the signature check apart.
	for _, signed := range []struct{ name, file, root string }{
		{"ecdsa-p256", "ecdsa-p256", "ecdsa-p256-root"}, {"pkcs7-sha1", "pkcs7-sha1", "pkcs7-sha1-root"},
		{"document-timestamp", "document-timestamp", ""}, {"ecdsa-p256-altered", "ecdsa-p256", "ecdsa-p256-root"},
	} {
		data, err := os.ReadFile(filepath.Join(support, signed.file+".pdf"))
		must(err)
		if signed.name != signed.file {
			check(bytes.Contains(data, []byte("verdict path")), "ecdsa-p256.pdf carries the reason the alteration changes")
			data = bytes.Replace(data, []byte("verdict path"), []byte("verdict PATH"), 1)
		}
		document, err := tp.Open(data)
		must(err)
		anchors := tp.NewTrustAnchors()
		if signed.root != "" {
			der, err := os.ReadFile(filepath.Join(support, signed.root+".der"))
			must(err)
			must(anchors.Add(der))
		}
		check(anchors.Count() == map[bool]uint32{true: 1, false: 0}[signed.root != ""], "the anchors hold what was added")
		fmt.Fprintf(&out, "document %s\n", signed.name)
		read, err := document.Signatures()
		must(err)
		for index, s := range read {
			spans := make([]string, 0, len(s.Spans))
			for _, span := range s.Spans {
				spans = append(spans, fmt.Sprintf("%d:%d", span.Start, span.Length))
			}
			spanToken := "-"
			if len(spans) > 0 {
				spanToken = strings.Join(spans, ",")
			}
			whole, usage := 0, 0
			if s.CoversWholeFile {
				whole = 1
			}
			if s.IsUsageRights {
				usage = 1
			}
			fmt.Fprintf(&out, "signature %d %s %s %s %s %s %s %d %d %d %s\n", index,
				textToken(s.FieldName), textToken(s.SubFilter), textToken(s.Reason), textToken(s.Location),
				textToken(s.Name), coverageNames[s.Coverage], whole, usage, s.CertificationLevel, spanToken)
		}
		zero := int64(0)
		for _, at := range []*int64{nil, &zero} {
			verdicts, err := document.VerifySignatures(anchors, at)
			must(err)
			for index, v := range verdicts {
				judged := "-"
				if at != nil {
					judged = strconv.FormatInt(*at, 10)
				}
				validity := "- -"
				if v.SignerValidity != nil {
					validity = fmt.Sprintf("%d %d", v.SignerValidity[0], v.SignerValidity[1])
				}
				names := make([]string, 0, len(v.Weaknesses))
				for _, w := range v.Weaknesses {
					names = append(names, weaknessNames[w])
				}
				weaknesses := "-"
				if len(names) > 0 {
					weaknesses = strings.Join(names, ",")
				}
				fmt.Fprintf(&out, "verdict %s %d %s %s %s %s %s %s %s %s\n", judged, index,
					[]string{"read", "absent", "unreadable"}[v.Cms],
					[]string{"matches", "differs", "not-checked"}[v.DocumentDigest],
					[]string{"verified", "failed", "not-checked"}[v.Signature],
					chainNames[v.Chain], textToken(v.SignerSubject), textToken(v.SignerIssuer), validity, weaknesses)
			}
		}
		anchors.Close()
		document.Close()
	}
	return out.String()
}

func str(s string) *string { return &s }

// forms creates a field of every kind, applies an XFDF fixture and saves; it
// returns the artefact and the form-data text's first lines.
func forms(fixture []byte, formDataDir string) ([]byte, []string) {
	document, err := tp.Open(fixture)
	must(err)
	editor, err := document.Editor()
	must(err)
	document.Close()
	defer editor.Close()

	var lines []string
	added := func(name string, ref tp.Ref, err error) {
		must(err)
		lines = append(lines, fmt.Sprintf("added %s %d.%d", textToken(&name), ref.Object, ref.Generation))
	}
	maxLen := uint32(20)
	ref, err := editor.AddTextField("person.given", 0, 300, 700, 500, 720, str("Ada"), &maxLen, tp.FieldOptions{})
	added("person.given", ref, err)
	ref, err = editor.AddCheckbox("subscribe", 0, 300, 660, 320, 680, "Yes", true, tp.FieldOptions{Flags: 2})
	added("subscribe", ref, err)
	ref, err = editor.AddRadioGroup("size", []tp.RadioButton{
		{Export: "S", Page: 0, X0: 300, Y0: 620, X1: 320, Y1: 640},
		{Export: "M", Page: 0, X0: 330, Y0: 620, X1: 350, Y1: 640},
	}, str("M"), tp.FieldOptions{})
	added("size", ref, err)
	ref, err = editor.AddChoiceField("country", 0, 300, 580, 400, 600, []string{"NZ", "LK", "UK"}, true, false,
		str("LK"), tp.FieldOptions{FontSize: 10})
	added("country", ref, err)
	ref, err = editor.AddChoiceField("languages", 0, 300, 500, 400, 560, []string{"en", "fr"}, false, false,
		nil, tp.FieldOptions{})
	added("languages", ref, err)

	raw, err := os.ReadFile(filepath.Join(formDataDir, "form-fields.xfdf"))
	must(err)
	data, err := tp.ReadXfdf(raw)
	must(err)
	skipped, err := editor.ApplyFormData(data)
	must(err)
	data.Close()
	widgets := make([]string, 0, len(skipped))
	for _, w := range skipped {
		widgets = append(widgets, fmt.Sprintf("%d.%d", w.Object, w.Generation))
	}
	applied := "-"
	if len(widgets) > 0 {
		applied = strings.Join(widgets, ",")
	}
	lines = append(lines, "applied "+applied)
	saved, err := editor.Save(options(tp.Rewrite))
	must(err)
	return saved, lines
}

var valueKindNames = map[tp.FieldValueKind]string{
	tp.ValueNone: "none", tp.ValueText: "text", tp.ValueState: "state", tp.ValueMany: "many",
}

var formWarningNames = map[tp.FormDataWarningKind]string{
	tp.WarningNotRead: "not-read", tp.WarningValueUnreadable: "value-unreadable",
	tp.WarningTreeCut: "tree-cut", tp.WarningUnnamed: "unnamed",
}

func formDataDump(label string, data *tp.FormData, lines []string) []string {
	lines = append(lines, "data "+label)
	source, err := data.Source()
	must(err)
	lines = append(lines, "source "+textToken(source))
	for i := uint32(0); i < data.Count(); i++ {
		name, err := data.FieldName(i)
		must(err)
		kind, err := data.ValueKind(i)
		must(err)
		values, err := data.Values(i)
		must(err)
		line := fmt.Sprintf("field %s %s", textToken(&name), valueKindNames[kind])
		for _, value := range values {
			line += " " + textToken(str(value))
		}
		lines = append(lines, line)
	}
	warnings, err := data.Warnings()
	must(err)
	for _, w := range warnings {
		lines = append(lines, fmt.Sprintf("warning %s %s %s", formWarningNames[w.Kind], textToken(w.What), textToken(w.Field)))
	}
	fdf, err := data.ToFdf()
	must(err)
	lines = append(lines, "fdf "+sum(fdf))
	xfdf, err := data.ToXfdf()
	if e, ok := err.(*tp.Error); ok && e.Status == tp.StatusFormDataRefused {
		lines = append(lines, "xfdf refused")
	} else {
		must(err)
		lines = append(lines, "xfdf "+sum(xfdf))
	}
	return lines
}

// hostile is hierarchy.fdf altered three ways, each the first occurrence
// replaced: the three warnings the fixtures never reach (the facade example
// says why).
func hostile(raw []byte) []byte {
	for _, pair := range [][2]string{
		{"/V (plain)", "/V 12345"},
		{"/T (untouched)", "/X (untouched)"},
		{"/V (through a reference)", "/Kids [ 2 0 R ]"},
	} {
		check(bytes.Contains(raw, []byte(pair[0])), "hierarchy.fdf carries what the alteration changes")
		raw = bytes.Replace(raw, []byte(pair[0]), []byte(pair[1]), 1)
	}
	return raw
}

func formDataText(formed []byte, lines []string, formDataDir string) string {
	document, err := tp.Open(formed)
	must(err)
	own, err := document.FormData()
	must(err)
	lines = formDataDump("document", own, lines)
	own.Close()
	document.Close()
	for _, file := range []string{"form-fields.fdf", "hierarchy.fdf", "form-fields.xfdf", "hierarchy.xfdf"} {
		raw, err := os.ReadFile(filepath.Join(formDataDir, file))
		must(err)
		read := tp.ReadFdf
		if strings.HasSuffix(file, ".xfdf") {
			read = tp.ReadXfdf
		}
		data, err := read(raw)
		must(err)
		lines = formDataDump(file, data, lines)
		data.Close()
	}
	raw, err := os.ReadFile(filepath.Join(formDataDir, "hierarchy.fdf"))
	must(err)
	altered, err := tp.ReadFdf(hostile(raw))
	must(err)
	lines = formDataDump("hostile.fdf", altered, lines)
	altered.Close()

	built, err := tp.NewFormData()
	must(err)
	must(built.SetSource(str("built.pdf")))
	must(built.AddField("a.b", tp.ValueText, "x é"))
	must(built.AddField("a.c", tp.ValueState, "On"))
	must(built.AddField("list", tp.ValueMany, "1", "2"))
	must(built.AddField("nothing", tp.ValueMany))
	must(built.AddField("empty", tp.ValueNone))
	lines = formDataDump("built", built, lines)
	built.Close()

	unrepresentable, err := tp.NewFormData()
	must(err)
	must(unrepresentable.AddField("bell", tp.ValueText, "\u0007"))
	lines = formDataDump("unrepresentable", unrepresentable, lines)
	unrepresentable.Close()

	for _, probe := range []struct {
		label string
		read  func([]byte) (*tp.FormData, error)
		raw   string
	}{{"read-fdf", tp.ReadFdf, "not form data"}, {"read-xfdf", tp.ReadXfdf, "<root/>"}} {
		data, err := probe.read([]byte(probe.raw))
		if e, ok := err.(*tp.Error); ok && e.Status == tp.StatusFormDataRefused {
			lines = append(lines, probe.label+" refused")
			continue
		}
		must(err)
		data.Close()
		lines = append(lines, probe.label+" accepted")
	}
	var out strings.Builder
	for _, line := range lines {
		out.WriteString(line)
		out.WriteByte('\n')
	}
	return out.String()
}

// graphics builds a document with every graphics resource the builder
// registers, each one used.
func graphics() []byte {
	builder, err := tp.NewBuilderWithVersion(2, 0)
	must(err)
	defer builder.Close()
	must(builder.AddBaseFont([]byte("F1"), []byte("Helvetica")))
	must(builder.AddNamedFont([]byte("F2"), []byte("Helvetica"), 128, []string{"Euro", "uni0141"}, []uint16{556, 611}))
	must(builder.AddForm([]byte("Fm0"), 0, 0, 100, 100, &tp.Matrix{1, 0, 0, 1, 10, 10},
		&tp.TransparencyGroup{ColorSpace: tp.DeviceGray, Isolated: true}, []byte("0.5 g 0 0 100 100 re f")))
	must(builder.AddForm([]byte("Fm1"), 0, 0, 50, 50, nil, nil, []byte("0 0 1 rg 10 10 30 30 re f")))
	fill, stroke, multiply := 0.5, 0.25, tp.BlendMultiply
	must(builder.AddExtGState([]byte("GS0"), tp.ExtGState{
		FillAlpha: &fill, StrokeAlpha: &stroke, BlendMode: &multiply,
		SoftMask: tp.SoftMaskGroup, MaskKind: tp.MaskLuminosity, MaskForm: []byte("Fm0"), Backdrop: []float64{0.5},
	}))
	must(builder.AddExtGState([]byte("GS1"), tp.ExtGState{SoftMask: tp.SoftMaskNone}))
	must(builder.AddTilingPattern([]byte("P0"), 0, 0, 5, 5, 8, 8, &tp.Matrix{2, 0, 0, 2, 0, 0},
		tp.TilingNoDistortion, []byte("1 0 0 rg 0 0 5 5 re f")))
	must(builder.AddImage([]byte("Im1"), tp.ImageGray8, 2, 2, []byte{0, 85, 170, 255}))

	page, err := builder.BeginPage(200, 200)
	must(err)
	must(page.SetBleedBox(5, 5, 195, 195))
	must(page.EncodedText([]byte("F2"), 12, 20, 170, 0.5, 1.5, []byte{128, 129}, "€Ł"))
	must(page.Raw([]byte("q")))
	must(page.SetExtGState([]byte("GS0")))
	must(page.Form([]byte("Fm1")))
	must(page.SetFillPattern([]byte("P0")))
	must(page.Raw([]byte("60 60 40 40 re f")))
	must(page.SetStrokePattern([]byte("P0")))
	must(page.Raw([]byte("4 w 110 110 40 40 re S")))
	must(page.SetExtGState([]byte("GS1")))
	must(page.Raw([]byte("Q")))
	must(page.Image([]byte("Im1"), 150, 20, 20, 20))
	must(builder.PushPage(page))
	page.Close()
	must(builder.ClearImageResources())
	page, err = builder.BeginPage(200, 200)
	must(err)
	must(page.Form([]byte("Fm0")))
	must(builder.PushPage(page))
	page.Close()
	bytes, err := builder.Finish()
	must(err)
	return bytes
}

// tagged writes a structure tree through OpenTag and CloseTag.
func tagged() []byte {
	builder, err := tp.NewBuilder()
	must(err)
	defer builder.Close()
	must(builder.AddBaseFont([]byte("F1"), []byte("Helvetica")))
	must(builder.SetLanguage("en-GB"))
	must(builder.MapRole([]byte("Heading"), []byte("H1")))
	open := func(page *tp.PageBuilder, kind string, configure func(*tp.Tag)) {
		tag, err := tp.NewTag([]byte(kind))
		must(err)
		configure(tag)
		must(page.OpenTag(tag))
		tag.Close()
	}
	one, err := builder.BeginPage(200, 200)
	must(err)
	open(one, "Heading", func(t *tp.Tag) { must(t.SetText(tp.TagTitle, "Introduction")) })
	must(one.Text([]byte("F1"), 14, 20, 170, "Tagged parity"))
	must(one.CloseTag())
	open(one, "P", func(t *tp.Tag) {
		must(t.SetText(tp.TagLang, "fr"))
		must(t.SetText(tp.TagActualText, "Bonjour"))
	})
	must(one.Text([]byte("F1"), 12, 20, 150, "Bon"))
	must(builder.PushPage(one))
	one.Close()
	two, err := builder.BeginPage(200, 200)
	must(err)
	must(two.Text([]byte("F1"), 12, 20, 170, "jour"))
	must(two.CloseTag())
	e, ok := two.CloseTag().(*tp.Error)
	check(ok && e.Status == tp.StatusEditRefused, "a close with nothing open must be refused")
	open(two, "Figure", func(t *tp.Tag) { must(t.SetText(tp.TagAlt, "A grey square")) })
	must(two.FillRect(20, 100, 40, 40, 0.5))
	must(two.CloseTag())
	open(two, "Span", func(t *tp.Tag) {
		must(t.SetText(tp.TagExpansion, "Portable Document Format"))
		must(t.SetID([]byte("pdf-1")))
	})
	must(two.Text([]byte("F1"), 12, 20, 80, "PDF"))
	must(two.CloseTag())
	open(two, "Div", func(t *tp.Tag) { must(t.KeepEmpty()) })
	must(two.CloseTag())
	open(two, "P", func(t *tp.Tag) { must(t.SetKey(7, 1)) })
	must(two.Text([]byte("F1"), 12, 20, 60, "read second"))
	must(two.CloseTag())
	open(two, "P", func(t *tp.Tag) { must(t.SetKey(7, 0)) })
	must(two.Text([]byte("F1"), 12, 20, 40, "read first"))
	must(two.CloseTag())
	must(builder.PushPage(two))
	two.Close()
	bytes, err := builder.Finish()
	must(err)
	return bytes
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: parity <form-fields.pdf>")
		os.Exit(2)
	}
	fixturePath, err := filepath.Abs(os.Args[1])
	must(err)
	fixture, err := os.ReadFile(fixturePath)
	must(err)
	outline, err := os.ReadFile(filepath.Join(filepath.Dir(fixturePath), "outline-3level.pdf"))
	must(err)

	report("fill-and-save", fillAndSave(fixture))
	report("build-a-document", buildADocument())
	operated := documentOps(outline)
	report("document-ops", operated)
	sanitised, removed := sanitise(operated)
	report("sanitise", sanitised)
	report("save-options", saveOptions(operated, false))
	report("save-linearized", saveOptions(operated, true))
	reportRead("sanitise-report", removed)
	reportRead("read-surface", readSurface(outline, operated))
	support := filepath.Join(filepath.Dir(filepath.Dir(fixturePath)), "crates", "tinker-pdf", "tests", "signature_support")
	reportRead("signatures", signatures(support))
	formDataDir := filepath.Join(filepath.Dir(support), "form_data")
	formed, lines := forms(fixture, formDataDir)
	report("forms", formed)
	reportRead("form-data", formDataText(formed, lines, formDataDir))
	report("graphics", graphics())
	report("tagged", tagged())
	fmt.Println("GO-PARITY: RAN")
}
