// Proves an installed NuGet package is the engine, native library and all.
//
//   dotnet run --project bindings/dotnet/tests/Smoke -- <file.pdf> <face.ttf>
//
// The point of this program is the part a `ProjectReference` would skip. A
// managed assembly that resolves is not evidence: every P/Invoke in
// `TinkerPdf.cs` names `tinker_pdf_ffi`, and if the package did not carry that
// library under `runtimes/<rid>/native/` the first call would throw
// `DllNotFoundException` at run time and nothing earlier would have noticed.
// So this consumes the packed `.nupkg` from a folder feed.
//
// And, as in the Python and JavaScript smoke tests, the render is asserted
// twice. `testdata/simple-text.pdf` names Helvetica and embeds no font
// program; the engine bundles no faces and reads no font directories, so
// rendering it without `SetFonts` returns a correctly sized, entirely blank
// bitmap. "A bitmap of the right size came back" passes on a package whose
// renderer does nothing at all.

using System;
using System.Collections.Generic;
using System.IO;
using TinkerPdf;

if (args.Length is not (2 or 3))
{
    Console.Error.WriteLine("usage: Smoke <file.pdf> <face.ttf> [form-fields.pdf]");
    return 2;
}

static int Ink(ReadOnlySpan<byte> pixels, int components)
{
    var painted = 0;
    for (var i = 0; i < pixels.Length; i += components)
    {
        if (pixels[i] != 0xFF)
        {
            painted += 1;
        }
    }
    return painted;
}

// The first native call. A package with no `runtimes/<rid>/native/` entry
// throws DllNotFoundException here, which is the failure this program exists
// to catch.
Console.WriteLine($"engine version {Document.Version}");

var pdf = File.ReadAllBytes(args[0]);
var face = File.ReadAllBytes(args[1]);

using var document = Document.Open(pdf);
var (width, height) = document.PageSize(0);
Console.WriteLine(
    $"pages={document.PageCount} encrypted={document.IsEncrypted} size={width}x{height}");
if (document.PageCount != 3)
{
    throw new Exception($"pageCount {document.PageCount}");
}

var text = document.PageText(0);
if (!text.Contains("Tinker fixture", StringComparison.Ordinal))
{
    throw new Exception($"text {text}");
}
Console.WriteLine($"text={text.Trim()[..Math.Min(40, text.Trim().Length)]}");

using (var bare = document.Render(0))
{
    if (Ink(bare.Pixels, 3) != 0)
    {
        throw new Exception("this fixture embeds no font, so it must draw nothing yet");
    }
}

document.SetFonts(face);
using var drawn = document.Render(0);
var pixels = drawn.Pixels;
var painted = Ink(pixels, 3);
Console.WriteLine(
    $"bitmap {drawn.Width}x{drawn.Height} stride={drawn.Stride} bytes={pixels.Length} ink={painted}");
if (painted < 100)
{
    throw new Exception($"only {painted} pixels of ink with a face supplied");
}
if (pixels.Length != (int)drawn.Stride * (int)drawn.Height)
{
    throw new Exception($"the pixel span is {pixels.Length} bytes");
}

Console.WriteLine("DOTNET-SMOKE: RAN, rendered and inked");

// ---- the write leg (gap 32 milestone 5) ------------------------------------
//
// The same scripts `crates/tinker-pdf/examples/write_parity.rs`,
// `bindings/python/tests/write_parity.py`, `bindings/js/tests/write_parity.mjs`
// and the Go, Java and Ruby parity programs run. `cargo xtask bindings-parity`
// requires every surface to print the same SHA-256s: ruling 11 says a binding
// projects the facade 1:1 and adds no logic of its own, so surfaces
// disagreeing means one of them added something. The third, read-surface,
// writes down everything the read surface says about two documents in the
// text the facade example's module documentation specifies byte for byte.
//
// Every artefact goes through the engine's own strict structural validator
// before its hash is printed, because four byte-identical outputs agreeing
// tells you nothing if all four are wrong.

if (args.Length < 3)
{
    Console.WriteLine("DOTNET-PARITY: SKIPPED, no form fixture given");
    return 0;
}

static string Sha256(byte[] bytes) =>
    Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(bytes)).ToLowerInvariant();

static void Report(string script, byte[] bytes)
{
    using var reopened = Document.Open(bytes);
    var defects = reopened.Validate();
    if (defects.Length != 0)
    {
        throw new Exception(
            $"{script}: the artefact does not pass the strict validator: " +
            string.Join(",", defects));
    }
    Console.WriteLine(
        $"DOTNET-SMOKE: WROTE sha256={Sha256(bytes)} surface=dotnet script={script} " +
        $"bytes={bytes.Length}");
}

// The eight-by-eight grey image every surface builds, from the same formula.
// A formula rather than a fixture file: a parity suite whose four surfaces
// read the same image *file* proves only that they can read a file.
var parityImage = new byte[64];
for (var i = 0; i < 64; i++)
{
    parityImage[i] = (byte)((i * 7) % 256);
}

var formBytes = File.ReadAllBytes(args[2]);

// Script one: fill-and-save. The document is disposed *before* the editor is
// used, which is the lifetime claim made explicitly rather than relied on: the
// engine's editor holds its own reference to the shared object store, so
// EditorHandle needs no keep-alive on its parent. If that were untrue the rest
// of this block would be a use-after-free rather than a test.
byte[] filled;
Editor editor;
using (var form = Document.Open(formBytes))
{
    editor = form.CreateEditor();
}

using (editor)
{
    var skipped = editor.FillField("name", "Ada Lovelace");
    if (skipped.Length != 1)
    {
        throw new Exception($"the /Rect-less widget must be reported, got {skipped.Length}");
    }
    if (skipped[0].ToString() != "7 0 R: no usable /Rect (12.5.2)")
    {
        throw new Exception($"unexpected report: {skipped[0]}");
    }
    if (skipped[0].ObjectNumber != 7 || skipped[0].Reason != WidgetDefect.RectMissing)
    {
        throw new Exception("the report lost the widget it names");
    }

    var clean = editor.FillField("notes", "every surface writes this");
    if (clean.Length != 0)
    {
        throw new Exception("the control field is well formed, so nothing is skipped");
    }

    editor.SetCheckbox("agree", true);
    editor.SelectRadio("colour", "red");
    filled = editor.Save(new WriteOptions { Mode = WriteMode.Incremental });
}
Report("fill-and-save", filled);

// Script two: build-a-document.
byte[] built;
using (var builder = new DocumentBuilder())
{
    builder.AddBaseFont("F1"u8.ToArray(), "Helvetica"u8.ToArray());
    builder.AddImage("Im1"u8.ToArray(), parityImage, ImageKind.Gray8, 8, 8);

    using (var one = builder.BeginPage(200.0, 200.0))
    {
        one.Text("F1"u8.ToArray(), 14.0, 20.0, 170.0, "Page one");
        one.FillRect(20.0, 40.0, 60.0, 60.0, 0.25);
        one.Image("Im1"u8.ToArray(), 100.0, 40.0, 60.0, 60.0);
        builder.PushPage(one);
    }

    using (var two = builder.BeginPage(200.0, 200.0))
    {
        two.Text("F1"u8.ToArray(), 14.0, 20.0, 170.0, "Page two");
        builder.PushPage(two);
    }

    builder.SetInfo("Title"u8.ToArray(), "tinker-pdf write parity");

    using var first = new OutlineEntry("Page one");
    first.SetPageTarget(0);
    using var second = new OutlineEntry("Page two");
    second.SetPageTarget(1);
    builder.SetOutline(first, second);

    built = builder.Finish();
}
Report("build-a-document", built);

// Script three: read-surface. The tokens are the contract's: hex for strings
// and bytes, IEEE bits for numbers, `-` for absent.
static string Hex(byte[] bytes) => Convert.ToHexString(bytes).ToLowerInvariant();

static string TextToken(string? value) =>
    value is null ? "-" : "s:" + Hex(System.Text.Encoding.UTF8.GetBytes(value));

static string BytesToken(byte[]? value) => value is null ? "-" : "b:" + Hex(value);

static string Number(double? value) =>
    value is null ? "-" : "f:" + BitConverter.DoubleToInt64Bits(value.Value).ToString("x16");

static string Reference((uint Object, ushort Generation)? value) =>
    value is null ? "-" : value.Value.Object + "." + value.Value.Generation;

static string Digest(byte[]? value) => value is null ? "-" : Sha256(value);

static string ViewToken(View view)
{
    switch (view.Kind)
    {
        case DestKind.Xyz:
            return "xyz " + Number(view.Left) + " " + Number(view.Top) + " " + Number(view.Zoom);
        case DestKind.FitH:
            return "fith " + Number(view.Top);
        case DestKind.FitV:
            return "fitv " + Number(view.Left);
        case DestKind.FitR:
            return "fitr " + Number(view.Left) + " " + Number(view.Bottom) + " "
                + Number(view.Right) + " " + Number(view.Top);
        case DestKind.FitB:
            return "fitb";
        case DestKind.FitBH:
            return "fitbh " + Number(view.Top);
        case DestKind.FitBV:
            return "fitbv " + Number(view.Left);
        default:
            return "fit";
    }
}

static string DestinationToken(Destination? destination)
{
    if (destination is null)
    {
        return "-";
    }
    switch (destination.Kind)
    {
        case DestinationKind.Explicit:
            var page = destination.PageIndex is null ? "-" : destination.PageIndex.Value.ToString();
            var view = destination.View ?? new View(DestKind.Fit);
            return "explicit " + page + " " + Reference(destination.PageRef) + " " + ViewToken(view);
        case DestinationKind.Named:
            return "named " + BytesToken(destination.Bytes);
        default:
            return "uri " + BytesToken(destination.Bytes);
    }
}

static string ActionToken(ActionKind kind, Destination? destination, byte[]? bytes)
{
    switch (kind)
    {
        case ActionKind.Absent:
            return "-";
        case ActionKind.GoTo:
            return "goto " + DestinationToken(destination);
        case ActionKind.GoToR:
            return "gotor " + BytesToken(bytes) + " " + DestinationToken(destination);
        case ActionKind.Uri:
            return "uri " + BytesToken(bytes);
        case ActionKind.Named:
            return "named " + BytesToken(bytes);
        case ActionKind.Launch:
            return "launch " + BytesToken(bytes);
        default:
            return "other " + BytesToken(bytes);
    }
}

static void ReadDump(string name, Document document, List<string> lines)
{
    lines.Add("document " + name);
    lines.Add("version " + TextToken(document.PdfVersion));
    lines.Add("pages " + document.PageCount);
    var keys = new (InfoKey Key, string Label)[]
    {
        (InfoKey.Title, "title"),
        (InfoKey.Author, "author"),
        (InfoKey.Subject, "subject"),
        (InfoKey.Keywords, "keywords"),
        (InfoKey.Creator, "creator"),
        (InfoKey.Producer, "producer"),
        (InfoKey.CreationDate, "creation-date"),
        (InfoKey.ModificationDate, "modification-date"),
    };
    foreach (var (key, label) in keys)
    {
        lines.Add("info " + label + " " + TextToken(document.Info(key)));
    }
    var trapped = document.Trapped switch
    {
        Trapped.True => "true",
        Trapped.False => "false",
        Trapped.Unknown => "unknown",
        _ => "absent",
    };
    lines.Add("trapped " + trapped);
    for (uint index = 0; index < document.PageCount; index++)
    {
        var label = document.PageLabel(index);
        if (label is null)
        {
            break;
        }
        lines.Add("label " + index + " " + TextToken(label));
    }
    var boundaries = new (PageBoundary Boundary, string Name)[]
    {
        (PageBoundary.MediaBox, "media"),
        (PageBoundary.CropBox, "crop"),
        (PageBoundary.BleedBox, "bleed"),
        (PageBoundary.TrimBox, "trim"),
        (PageBoundary.ArtBox, "art"),
    };
    for (uint index = 0; index < document.PageCount; index++)
    {
        foreach (var (boundary, boxName) in boundaries)
        {
            var (bx0, by0, bx1, by1) = document.PageBox(index, boundary);
            lines.Add("box " + index + " " + boxName + " " + Number(bx0) + " " + Number(by0) + " "
                + Number(bx1) + " " + Number(by1));
        }
    }
    using (var outline = document.ReadOutline())
    {
        for (uint index = 0; index < outline.Count; index++)
        {
            var (depth, open) = outline.Item(index);
            lines.Add("outline " + depth + " " + (open ? "1" : "0") + " "
                + TextToken(outline.Title(index)) + " "
                + DestinationToken(outline.DestinationOf(index)));
        }
    }
    for (uint page = 0; page < document.PageCount; page++)
    {
        using var links = document.ReadLinks(page);
        for (uint index = 0; index < links.Count; index++)
        {
            var (x0, y0, x1, y1) = links.Rect(index);
            var (kind, destination) = links.Action(index);
            lines.Add("link " + page + " " + Number(x0) + " " + Number(y0) + " " + Number(x1)
                + " " + Number(y1) + " " + Reference(links.Reference(index)) + " "
                + ActionToken(kind, destination, links.ActionBytes(index)));
        }
    }
    using (var attachments = document.ReadAttachments())
    {
        for (uint index = 0; index < attachments.Count; index++)
        {
            byte[]? data;
            try
            {
                data = attachments.Data(index);
            }
            catch (PdfException e) when (e.Status == Status.StreamUnreadable)
            {
                data = null;
            }
            var size = attachments.Size(index);
            lines.Add("attachment " + TextToken(attachments.Name(index)) + " "
                + TextToken(attachments.Filename(index)) + " "
                + TextToken(attachments.Description(index)) + " "
                + (size is null ? "-" : size.Value.ToString()) + " " + Digest(data));
        }
    }
    lines.Add("xmp " + Digest(document.XmpMetadata()));
    using (var warnings = document.ReadWarnings())
    {
        for (uint index = 0; index < warnings.Count; index++)
        {
            var (offset, objectRef) = warnings.Location(index);
            lines.Add("warning " + offset + " " + Reference(objectRef) + " "
                + warnings.Kind(index) + " " + TextToken(warnings.Message(index)));
        }
    }
}

static byte[] LinkedDocument()
{
    using var builder = new DocumentBuilder();
    builder.AddBaseFont("F1"u8.ToArray(), "Helvetica"u8.ToArray());
    using (var one = builder.BeginPage(200.0, 200.0))
    {
        one.Text("F1"u8.ToArray(), 12.0, 20.0, 170.0, "Links");
        one.LinkToUri(10.0, 10.0, 60.0, 30.0, "https://example.org/parity");
        one.LinkToPage(70.0, 10.0, 120.5, 30.25, 1, new View(DestKind.Xyz, Left: 10.0, Zoom: 1.5));
        builder.PushPage(one);
    }
    using (var two = builder.BeginPage(200.0, 200.0))
    {
        builder.PushPage(two);
    }
    builder.SetInfo("Title"u8.ToArray(), "Read surface \u2014 parity");
    builder.SetInfo("Author"u8.ToArray(), "");
    using var heading = new OutlineEntry("Part one");
    heading.SetOpen(true);
    using (var chapter = new OutlineEntry("Chapter one"))
    {
        chapter.SetPageTarget(1, new View(DestKind.FitH, Top: 150.0));
        heading.AddChild(chapter);
    }
    using var elsewhere = new OutlineEntry("Elsewhere");
    elsewhere.SetUriTarget("https://example.org/");
    builder.SetOutline(heading, elsewhere);
    return builder.Finish();
}

static byte[] DocumentOps(byte[] outlineFixture)
{
    var created = new PdfDate(2026, 10, 3, 12, 0, 0, 0);
    using var source = Document.Open(outlineFixture);
    using var ops = source.CreateEditor();
    ops.SetPageLabels(
        new PageLabelRange(0, LabelStyle.RomanLower, null, 1),
        new PageLabelRange(2, LabelStyle.Decimal, "A-", 1));
    ops.AttachFile(new EmbeddedFile(
        "data.csv",
        "data.csv",
        System.Text.Encoding.ASCII.GetBytes("a,b\n1,2\n"),
        Description: "the numbers",
        MimeType: "text/csv",
        Created: created));
    if (ops.SetInfo(InfoKey.Title, "Document operations") != MetadataSync.Alone)
    {
        throw new Exception("no XMP packet yet, so the title is alone");
    }
    ops.SetInfo(InfoKey.Author, "tinker-pdf");
    ops.SetInfoDate(InfoKey.CreationDate, created);
    ops.SetTrapped(Trapped.False);
    if (ops.SetXmpMetadata(System.Text.Encoding.ASCII.GetBytes("<x:xmpmeta xmlns:x='adobe:ns:meta/'/>"))
        != MetadataSync.OtherHalfUnchanged)
    {
        throw new Exception("/Info has entries the packet was not checked against");
    }
    ops.SetTrimBox(0, 10.0, 10.0, 585.0, 832.0);
    ops.SetBleedBox(1, 0.0, 0.0, 595.0, 842.0);
    using var only = new OutlineEntry("Only entry");
    only.SetPageTarget(3, new View(DestKind.FitH, Top: 700.0));
    ops.SetOutline(only);
    return ops.Save();
}

static string RemovalName(Removal removal) => removal switch
{
    Removal.JavaScript => "javascript",
    Removal.DocumentJavaScript => "document-javascript",
    Removal.CalculationOrder => "calculation-order",
    Removal.XfaForm => "xfa-form",
    Removal.Action => "action",
    Removal.EmbeddedFileTree => "embedded-file-tree",
    Removal.EmbeddedFile => "embedded-file",
    Removal.Info => "info",
    _ => "metadata",
};

// The options a save takes, every one the C ABI carries but encryption away
// from its default, after two edits that give them something to act on: the
// deleted page is what garbage collection drops, and the appended operators
// are the one stream nobody has encoded, which is what compression
// compresses. save-linearized is the same save linearized; the linearizer
// sets object streams and compression aside, so it is a second script.
static byte[] SaveOptions(byte[] operated, bool linearize)
{
    using var source = Document.Open(operated);
    using var editor = source.CreateEditor();
    editor.DeletePage(1);
    editor.AppendContent(0, System.Text.Encoding.ASCII.GetBytes("0 0 m 100 100 l S"));
    return editor.Save(new WriteOptions
    {
        Mode = WriteMode.Rewrite,
        Linearize = linearize,
        Version = (2, 0),
        ObjectStreams = true,
        Compress = true,
        GarbageCollect = true,
    });
}

static (byte[] Saved, string Report) SanitiseScript(byte[] operated)
{
    using var source = Document.Open(operated);
    using var cleaner = source.CreateEditor();
    var report = cleaner.Sanitise(new SanitiseOptions(true, true, true, true));
    var text = new System.Text.StringBuilder();
    foreach (var entry in report.Removed)
    {
        var steps = new List<string>();
        foreach (var step in entry.Path)
        {
            steps.Add(step is ulong at ? "i:" + at : "k:" + Hex((byte[])step));
        }
        text.Append("removed ").Append(RemovalName(entry.What)).Append(' ')
            .Append(entry.Holder is null ? "trailer" : entry.Holder.Value.Object + "." + entry.Holder.Value.Generation)
            .Append(' ').Append(string.Join("/", steps)).Append(' ')
            .Append(BytesToken(entry.Action)).Append('\n');
    }
    foreach (var entry in report.Deleted)
    {
        text.Append("deleted ").Append(RemovalName(entry.What)).Append(' ')
            .Append(entry.Object.Object + "." + entry.Object.Generation).Append(' ')
            .Append(BytesToken(entry.Action)).Append('\n');
    }
    return (cleaner.Save(), text.ToString());
}

var outlinePath = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(args[2]))!, "outline-3level.pdf");
var outlineBytes = File.ReadAllBytes(outlinePath);
var operatedBytes = DocumentOps(outlineBytes);
Report("document-ops", operatedBytes);
var (sanitisedBytes, removedText) = SanitiseScript(operatedBytes);
Report("sanitise", sanitisedBytes);
Report("save-options", SaveOptions(operatedBytes, false));
Report("save-linearized", SaveOptions(operatedBytes, true));
var removedBytes = System.Text.Encoding.UTF8.GetBytes(removedText);
if (Environment.GetEnvironmentVariable("TINKER_PARITY_DUMP") is not null)
{
    Console.Write(removedText);
}
Console.WriteLine(
    $"DOTNET-SMOKE: READ sha256={Sha256(removedBytes)} surface=dotnet script=sanitise-report " +
    $"bytes={removedBytes.Length}");
var shiftedBytes = new byte[outlineBytes.Length + 5];
"JUNK\n"u8.ToArray().CopyTo(shiftedBytes, 0);
outlineBytes.CopyTo(shiftedBytes, 5);
var readLines = new List<string>();
using (var shifted = Document.Open(shiftedBytes))
{
    ReadDump("shifted", shifted, readLines);
}
using (var linked = Document.Open(LinkedDocument()))
{
    ReadDump("linked", linked, readLines);
}
using (var operatedDocument = Document.Open(operatedBytes))
{
    ReadDump("operated", operatedDocument, readLines);
}
var dumped = new System.Text.StringBuilder();
foreach (var line in readLines)
{
    dumped.Append(line).Append('\n');
}
var dumpedBytes = System.Text.Encoding.UTF8.GetBytes(dumped.ToString());
if (Environment.GetEnvironmentVariable("TINKER_PARITY_DUMP") is not null)
{
    Console.Write(dumped.ToString());
}
Console.WriteLine(
    $"DOTNET-SMOKE: READ sha256={Sha256(dumpedBytes)} surface=dotnet script=read-surface " +
    $"bytes={dumpedBytes.Length}");

// Script four: signatures. Every signature as read, then every verdict twice —
// judged at no instant and at the epoch — in the contract's text.
static string CoverageName(Coverage coverage) => coverage switch
{
    Coverage.WholeFile => "whole-file",
    Coverage.Revision => "revision",
    _ => "suspicious",
};

static string WeaknessName(Weakness weakness) => weakness switch
{
    Weakness.Sha1Digest => "sha1-digest",
    Weakness.Sha1Signature => "sha1-signature",
    Weakness.ShortRsaKey => "short-rsa-key",
    Weakness.CoversOnlyARevision => "covers-only-a-revision",
    Weakness.CoverageSuspicious => "coverage-suspicious",
    _ => "outside-validity",
};

static string ChainName(Chain chain) => chain switch
{
    Chain.AnchoredTo => "anchored-to",
    Chain.SelfSigned => "self-signed",
    Chain.Incomplete => "incomplete",
    Chain.Broken => "broken",
    Chain.NoAnchors => "no-anchors",
    _ => "no-signer-certificate",
};

// The altered document is ecdsa-p256.pdf with its first `verdict path` changed
// to `verdict PATH`: only its digest moves, which is what tells the digest and
// the signature check apart.
static byte[] Altered(byte[] bytes)
{
    var needle = System.Text.Encoding.ASCII.GetBytes("verdict path");
    for (var at = 0; at + needle.Length <= bytes.Length; at++)
    {
        if (bytes.AsSpan(at, needle.Length).SequenceEqual(needle))
        {
            var copy = (byte[])bytes.Clone();
            System.Text.Encoding.ASCII.GetBytes("PATH").CopyTo(copy, at + "verdict ".Length);
            return copy;
        }
    }
    throw new InvalidOperationException("ecdsa-p256.pdf carries the reason the alteration changes");
}

static void SignaturesDump(string support, string name, string? root, List<string> lines, string? file = null)
{
    var bytes = File.ReadAllBytes(Path.Combine(support, (file ?? name) + ".pdf"));
    using var document = Document.Open(file is null ? bytes : Altered(bytes));
    using var anchors = new TrustAnchors();
    if (root is not null)
    {
        anchors.Add(File.ReadAllBytes(Path.Combine(support, root + ".der")));
    }
    lines.Add("document " + name);
    using (var signatures = document.ReadSignatures())
    {
        for (uint index = 0; index < signatures.Count; index++)
        {
            var spans = new List<string>();
            for (uint span = 0; span < signatures.SpanCount(index); span++)
            {
                var (start, length) = signatures.Span(index, span);
                spans.Add(start + ":" + length);
            }
            lines.Add("signature " + index + " " + TextToken(signatures.FieldName(index)) + " "
                + TextToken(signatures.SubFilter(index)) + " "
                + TextToken(signatures.Reason(index)) + " "
                + TextToken(signatures.Location(index)) + " "
                + TextToken(signatures.SignerName(index)) + " "
                + CoverageName(signatures.CoverageOf(index)) + " "
                + (signatures.CoversWholeFile(index) ? "1" : "0") + " "
                + (signatures.IsUsageRights(index) ? "1" : "0") + " "
                + signatures.CertificationLevel(index) + " "
                + (spans.Count == 0 ? "-" : string.Join(",", spans)));
        }
    }
    foreach (var at in new long?[] { null, 0 })
    {
        using var verdicts = document.VerifySignatures(anchors, at);
        for (uint index = 0; index < verdicts.Count; index++)
        {
            var cms = verdicts.CmsStateOf(index) switch
            {
                CmsState.Read => "read",
                CmsState.Absent => "absent",
                _ => "unreadable",
            };
            var digest = verdicts.DocumentDigestOf(index) switch
            {
                DocumentDigest.Matches => "matches",
                DocumentDigest.Differs => "differs",
                _ => "not-checked",
            };
            var check = verdicts.SignatureCheckOf(index) switch
            {
                SignatureCheck.Verified => "verified",
                SignatureCheck.Failed => "failed",
                _ => "not-checked",
            };
            var validity = verdicts.SignerValidity(index);
            var weaknesses = new List<string>();
            for (uint w = 0; w < verdicts.WeaknessCount(index); w++)
            {
                weaknesses.Add(WeaknessName(verdicts.WeaknessAt(index, w)));
            }
            lines.Add("verdict " + (at is null ? "-" : at.Value.ToString()) + " " + index + " "
                + cms + " " + digest + " " + check + " " + ChainName(verdicts.ChainOf(index)) + " "
                + TextToken(verdicts.SignerSubject(index)) + " "
                + TextToken(verdicts.SignerIssuer(index)) + " "
                + (validity is null ? "- -" : validity.Value.NotBefore + " " + validity.Value.NotAfter)
                + " " + (weaknesses.Count == 0 ? "-" : string.Join(",", weaknesses)));
        }
    }
}

var support = Path.Combine(
    Path.GetDirectoryName(Path.GetDirectoryName(Path.GetFullPath(args[2]))!)!,
    "crates", "tinker-pdf", "tests", "signature_support");
var signedLines = new List<string>();
SignaturesDump(support, "ecdsa-p256", "ecdsa-p256-root", signedLines);
SignaturesDump(support, "pkcs7-sha1", "pkcs7-sha1-root", signedLines);
SignaturesDump(support, "document-timestamp", null, signedLines);
SignaturesDump(support, "ecdsa-p256-altered", "ecdsa-p256-root", signedLines, "ecdsa-p256");
var signed = new System.Text.StringBuilder();
foreach (var line in signedLines)
{
    signed.Append(line).Append('\n');
}
var signedBytes = System.Text.Encoding.UTF8.GetBytes(signed.ToString());
if (Environment.GetEnvironmentVariable("TINKER_PARITY_DUMP") is not null)
{
    Console.Write(signed.ToString());
}
Console.WriteLine(
    $"DOTNET-SMOKE: READ sha256={Sha256(signedBytes)} surface=dotnet script=signatures " +
    $"bytes={signedBytes.Length}");

// Scripts five and six: forms and form-data. A field of every kind created,
// an XFDF fixture applied and the document saved; then what form data says,
// in the text the facade example specifies.
var formDataDir = Path.Combine(Path.GetDirectoryName(support)!, "form_data");
var formLines = new List<string>();
byte[] formed;
using (var form = Document.Open(formBytes))
using (var editor = form.CreateEditor())
{
    void Added(string label, (uint Object, ushort Generation) reference) =>
        formLines.Add($"added {TextToken(label)} {reference.Object}.{reference.Generation}");
    Added("person.given", editor.AddTextField(
        "person.given", 0, 300, 700, 500, 720, value: "Ada", maxLen: 20));
    Added("subscribe", editor.AddCheckbox(
        "subscribe", 0, 300, 660, 320, 680, "Yes", true, flags: 2));
    Added("size", editor.AddRadioGroup(
        "size",
        new[] { new RadioButton("S", 0, 300, 620, 320, 640), new RadioButton("M", 0, 330, 620, 350, 640) },
        selected: "M"));
    Added("country", editor.AddChoiceField(
        "country", 0, 300, 580, 400, 600, new[] { "NZ", "LK", "UK" }, true, value: "LK", fontSize: 10));
    Added("languages", editor.AddChoiceField(
        "languages", 0, 300, 500, 400, 560, new[] { "en", "fr" }, false));
    using var fixtureData = FormData.ReadXfdf(File.ReadAllBytes(Path.Combine(formDataDir, "form-fields.xfdf")));
    var widgets = new List<string>();
    foreach (var widget in editor.ApplyFormData(fixtureData))
    {
        widgets.Add($"{widget.ObjectNumber}.{widget.Generation}");
    }
    formLines.Add("applied " + (widgets.Count == 0 ? "-" : string.Join(",", widgets)));
    formed = editor.Save(new WriteOptions());
}
Report("forms", formed);

static void FormDataDump(string label, FormData data, List<string> lines)
{
    lines.Add($"data {label}");
    lines.Add($"source {TextToken(data.Source)}");
    for (uint i = 0; i < data.Count; i++)
    {
        var kind = data.ValueKind(i) switch
        {
            FieldValueKind.None => "none",
            FieldValueKind.Text => "text",
            FieldValueKind.State => "state",
            FieldValueKind.Many => "many",
            var other => throw new Exception($"a value kind the text has no spelling for: {other}"),
        };
        var line = $"field {TextToken(data.FieldName(i))} {kind}";
        foreach (var value in data.Values(i))
        {
            line += " " + TextToken(value);
        }
        lines.Add(line);
    }
    foreach (var warning in data.Warnings)
    {
        var kind = warning.Kind switch
        {
            FormDataWarningKind.NotRead => "not-read",
            FormDataWarningKind.ValueUnreadable => "value-unreadable",
            FormDataWarningKind.TreeCut => "tree-cut",
            FormDataWarningKind.Unnamed => "unnamed",
            var other => throw new Exception($"a warning the text has no spelling for: {other}"),
        };
        lines.Add($"warning {kind} {TextToken(warning.What)} {TextToken(warning.Field)}");
    }
    lines.Add($"fdf {Sha256(data.ToFdf())}");
    string xfdf;
    try
    {
        xfdf = Sha256(data.ToXfdf());
    }
    catch (PdfException e) when (e.Status == Status.FormDataRefused)
    {
        xfdf = "refused";
    }
    lines.Add($"xfdf {xfdf}");
}

static byte[] Hostile(byte[] bytes)
{
    // hierarchy.fdf altered three ways, each the first occurrence replaced:
    // the three warnings the fixtures never reach (the facade example says why).
    var replacements = new[]
    {
        ("/V (plain)", "/V 12345"),
        ("/T (untouched)", "/X (untouched)"),
        ("/V (through a reference)", "/Kids [ 2 0 R ]"),
    };
    var text = System.Text.Encoding.Latin1.GetString(bytes);
    foreach (var (from, to) in replacements)
    {
        var at = text.IndexOf(from, StringComparison.Ordinal);
        if (at < 0)
        {
            throw new Exception("hierarchy.fdf carries what the alteration changes");
        }
        text = text.Substring(0, at) + to + text.Substring(at + from.Length);
    }
    return System.Text.Encoding.Latin1.GetBytes(text);
}

using (var formedDocument = Document.Open(formed))
using (var own = formedDocument.ReadFormData())
{
    FormDataDump("document", own, formLines);
}
foreach (var file in new[] { "form-fields.fdf", "hierarchy.fdf", "form-fields.xfdf", "hierarchy.xfdf" })
{
    var raw = File.ReadAllBytes(Path.Combine(formDataDir, file));
    using var data = file.EndsWith(".xfdf", StringComparison.Ordinal) ? FormData.ReadXfdf(raw) : FormData.ReadFdf(raw);
    FormDataDump(file, data, formLines);
}
using (var hostile = FormData.ReadFdf(Hostile(File.ReadAllBytes(Path.Combine(formDataDir, "hierarchy.fdf")))))
{
    FormDataDump("hostile.fdf", hostile, formLines);
}
using (var builtData = new FormData())
{
    builtData.Source = "built.pdf";
    builtData.AddField("a.b", FieldValueKind.Text, "x \u00e9");
    builtData.AddField("a.c", FieldValueKind.State, "On");
    builtData.AddField("list", FieldValueKind.Many, "1", "2");
    builtData.AddField("nothing", FieldValueKind.Many);
    builtData.AddField("empty", FieldValueKind.None);
    FormDataDump("built", builtData, formLines);
}
using (var unrepresentable = new FormData())
{
    unrepresentable.AddField("bell", FieldValueKind.Text, "\u0007");
    FormDataDump("unrepresentable", unrepresentable, formLines);
}
foreach (var (label, xml, raw) in new[]
{
    ("read-fdf", false, System.Text.Encoding.ASCII.GetBytes("not form data")),
    ("read-xfdf", true, System.Text.Encoding.ASCII.GetBytes("<root/>")),
})
{
    try
    {
        using var read = xml ? FormData.ReadXfdf(raw) : FormData.ReadFdf(raw);
        formLines.Add($"{label} accepted");
    }
    catch (PdfException e) when (e.Status == Status.FormDataRefused)
    {
        formLines.Add($"{label} refused");
    }
}
var said = new System.Text.StringBuilder();
foreach (var line in formLines)
{
    said.Append(line).Append('\n');
}
var saidBytes = System.Text.Encoding.UTF8.GetBytes(said.ToString());
if (Environment.GetEnvironmentVariable("TINKER_PARITY_DUMP") is not null)
{
    Console.Write(said.ToString());
}
Console.WriteLine(
    $"DOTNET-SMOKE: READ sha256={Sha256(saidBytes)} surface=dotnet script=form-data " +
    $"bytes={saidBytes.Length}");

// Script seven: graphics. The builder's graphics resources, every one used.
byte[] graphicsBytes;
using (var graphicsBuilder = DocumentBuilder.WithVersion(2, 0))
{
    graphicsBuilder.AddBaseFont("F1"u8.ToArray(), "Helvetica"u8.ToArray());
    graphicsBuilder.AddNamedFont("F2"u8.ToArray(), "Helvetica"u8.ToArray(), 128,
        new[] { "Euro", "uni0141" }, new ushort[] { 556, 611 });
    graphicsBuilder.AddForm("Fm0"u8.ToArray(), 0, 0, 100, 100, new double[] { 1, 0, 0, 1, 10, 10 },
        new TransparencyGroup(DeviceSpace.Gray, true, false), "0.5 g 0 0 100 100 re f"u8.ToArray());
    graphicsBuilder.AddForm("Fm1"u8.ToArray(), 0, 0, 50, 50, null, null, "0 0 1 rg 10 10 30 30 re f"u8.ToArray());
    graphicsBuilder.AddExtGState("GS0"u8.ToArray(), new ExtGState(0.5, 0.25, BlendMode.Multiply,
        SoftMask.Group, MaskKind.Luminosity, "Fm0"u8.ToArray(), new[] { 0.5 }));
    graphicsBuilder.AddExtGState("GS1"u8.ToArray(), new ExtGState(SoftMask: SoftMask.None));
    graphicsBuilder.AddTilingPattern("P0"u8.ToArray(), 0, 0, 5, 5, 8, 8, new double[] { 2, 0, 0, 2, 0, 0 },
        TilingType.NoDistortion, "1 0 0 rg 0 0 5 5 re f"u8.ToArray());
    graphicsBuilder.AddImage("Im1"u8.ToArray(), new byte[] { 0, 85, 170, 255 }, ImageKind.Gray8, 2, 2);
    using (var page = graphicsBuilder.BeginPage(200.0, 200.0))
    {
        page.SetBleedBox(5, 5, 195, 195);
        page.EncodedText("F2"u8.ToArray(), 12, 20, 170, 0.5, 1.5, new byte[] { 128, 129 }, "\u20ac\u0141");
        page.Raw_("q"u8.ToArray());
        page.SetExtGState("GS0"u8.ToArray());
        page.Form("Fm1"u8.ToArray());
        page.SetFillPattern("P0"u8.ToArray());
        page.Raw_("60 60 40 40 re f"u8.ToArray());
        page.SetStrokePattern("P0"u8.ToArray());
        page.Raw_("4 w 110 110 40 40 re S"u8.ToArray());
        page.SetExtGState("GS1"u8.ToArray());
        page.Raw_("Q"u8.ToArray());
        page.Image("Im1"u8.ToArray(), 150, 20, 20, 20);
        graphicsBuilder.PushPage(page);
    }
    graphicsBuilder.ClearImageResources();
    using (var page = graphicsBuilder.BeginPage(200.0, 200.0))
    {
        page.Form("Fm0"u8.ToArray());
        graphicsBuilder.PushPage(page);
    }
    graphicsBytes = graphicsBuilder.Finish();
}
Report("graphics", graphicsBytes);

// The callback-taking transaction, which is checkpoint, `try`, restore and
// nothing else. Asserted the only way that cannot be faked: save before, save
// after, compare hashes. And the exception must still escape — a rollback that
// also hid the reason would be the worst of both.
using (var form = Document.Open(formBytes))
using (var tx = form.CreateEditor())
{
    var options = new WriteOptions { Mode = WriteMode.Incremental };
    tx.SetCheckbox("agree", true);
    var before = Sha256(tx.Save(options));

    var threw = false;
    try
    {
        tx.Transaction(() =>
        {
            tx.SelectRadio("colour", "blue");
            tx.FillField("notes", "this must not survive");
            if (Sha256(tx.Save(options)) == before)
            {
                throw new Exception("the body did not change anything");
            }
            throw new InvalidOperationException("deliberate");
        });
    }
    catch (InvalidOperationException e) when (e.Message == "deliberate")
    {
        threw = true;
    }
    if (!threw)
    {
        throw new Exception("the exception was swallowed, which a rollback must never do");
    }

    var after = Sha256(tx.Save(options));
    if (after != before)
    {
        throw new Exception($"the editor was not restored: {before} -> {after}");
    }

    tx.Transaction(() => tx.SelectRadio("colour", "red"));
    if (Sha256(tx.Save(options)) == before)
    {
        throw new Exception("a body that does not throw commits");
    }
    Console.WriteLine("DOTNET-PARITY: transaction rolls back on an exception and commits without one");
}

// A consumed handle refuses rather than producing a second document, and the
// status crosses so a caller can branch on it rather than on a string.
using (var spent = new DocumentBuilder())
{
    spent.AddBaseFont("F1"u8.ToArray(), "Helvetica"u8.ToArray());
    spent.Finish();
    try
    {
        spent.Finish();
        throw new Exception("a second finish must be refused");
    }
    catch (PdfException e) when (e.Status == Status.SpentHandle)
    {
        Console.WriteLine($"DOTNET-PARITY: a spent handle says so: {e.Message}");
    }
}

// **Finalizer-only teardown.** Everything above disposes explicitly. This block
// deliberately does not, so the SafeHandles are released by the finalizer
// instead — which is the path a caller who forgets `using` takes, and the one
// that is never otherwise exercised. `GC.Collect` plus
// `WaitForPendingFinalizers` twice is what actually runs them: the first pass
// queues the finalizers, the second waits for what those resurrect.
static byte[] BuiltWithoutDisposing()
{
    var builder = new DocumentBuilder();
    builder.AddBaseFont("F1"u8.ToArray(), "Helvetica"u8.ToArray());
    var page = builder.BeginPage(100.0, 100.0);
    page.Text("F1"u8.ToArray(), 12.0, 10.0, 50.0, "finalized");
    builder.PushPage(page);
    return builder.Finish();
}

var undisposed = BuiltWithoutDisposing();
for (var pass = 0; pass < 2; pass++)
{
    GC.Collect();
    GC.WaitForPendingFinalizers();
}
using (var reopened = Document.Open(undisposed))
{
    if (reopened.PageCount != 1)
    {
        throw new Exception("the finalizer-only document is not the document");
    }
}
Console.WriteLine("DOTNET-PARITY: finalizer-only teardown released its handles without a crash");

Console.WriteLine("DOTNET-PARITY: RAN");
return 0;
