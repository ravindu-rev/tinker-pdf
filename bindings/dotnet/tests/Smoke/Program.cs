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
//
// Then `OpenFile`'s read on demand is held to `Open(bytes)`: over a generated
// four-megabyte file it is streamed and reads the same, and over a damaged
// file and a linearized one updated since it reads the file whole and warns
// what the bytes warn.
//
// Then the command stream: the stream of a page holding every kind of record,
// decoded through the binding's own types and written back to the facade's
// bytes, and replayed to a recording device that hears every kind in the
// stream's order.

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

// ---- a path opened on demand ------------------------------------------------
//
// `OpenFile` reads a file on demand by the rule `tpdf` and the Python binding
// follow: a file whose streamed open answers as its bytes would stays
// streamed and answers as its bytes do, and a damaged file and a linearized
// one updated since are read whole and warn what their bytes warn. The files
// are written by formula, as `wheel_smoke.py` writes them.

static byte[] Ascii(string text) => System.Text.Encoding.ASCII.GetBytes(text);

// A PDF of `bodies`, numbered from 1, the first its catalog, with a classic
// cross-reference table.
static byte[] Pdf(IReadOnlyList<byte[]> bodies)
{
    var output = new MemoryStream();
    output.Write(Ascii("%PDF-1.7\n"));
    var offsets = new List<long>();
    for (var number = 1; number <= bodies.Count; number++)
    {
        offsets.Add(output.Length);
        output.Write(Ascii($"{number} 0 obj\n"));
        output.Write(bodies[number - 1]);
        output.Write(Ascii("\nendobj\n"));
    }
    var table = output.Length;
    output.Write(Ascii($"xref\n0 {bodies.Count + 1}\n0000000000 65535 f \n"));
    foreach (var offset in offsets)
    {
        output.Write(Ascii($"{offset:D10} 00000 n \n"));
    }
    output.Write(Ascii(
        $"trailer\n<< /Size {bodies.Count + 1} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n"));
    return output.ToArray();
}

// `pages` pages of a line of Helvetica text and about `size` bytes of drawing
// each: few objects and many bytes, as a scan or a drawing is.
static byte[] Heavy(int pages, int size)
{
    var font = 3 + 2 * pages;
    var kids = new List<string>();
    for (var at = 0; at < pages; at++)
    {
        kids.Add($"{3 + 2 * at} 0 R");
    }
    var bodies = new List<byte[]>
    {
        Ascii("<< /Type /Catalog /Pages 2 0 R >>"),
        Ascii($"<< /Type /Pages /Count {pages} /Kids [{string.Join(" ", kids)}] >>"),
    };
    var drawing = new System.Text.StringBuilder();
    for (var square = 0; square < size / 13; square++)
    {
        drawing.Append("0 0 1 1 re f\n");
    }
    for (var at = 0; at < pages; at++)
    {
        bodies.Add(Ascii(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]" +
            $" /Resources << /Font << /F1 {font} 0 R >> >> /Contents {4 + 2 * at} 0 R >>"));
        var content = $"BT /F1 12 Tf 10 50 Td (page {at + 1} on demand) Tj ET\n{drawing}";
        bodies.Add(Ascii($"<< /Length {content.Length} >>\nstream\n{content}\nendstream"));
    }
    bodies.Add(Ascii("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"));
    return Pdf(bodies);
}

// A catalog and an empty tree whose table's entry for the tree points into the
// header, which an open of the bytes patches.
static byte[] Damaged()
{
    var data = Pdf(new[]
    {
        Ascii("<< /Type /Catalog /Pages 2 0 R >>"),
        Ascii("<< /Type /Pages /Count 0 /Kids [] >>"),
    });
    var table = Ascii("xref\n0 3\n");
    var at = data.AsSpan().IndexOf(table) + table.Length + 2 * 20;
    Ascii("0000000004").CopyTo(data, at);
    return data;
}

// Three pages saved linearized, then a title appended as an update, which
// leaves the length the linearization states behind.
static byte[] Updated()
{
    byte[] linearized;
    using (var plain = Document.Open(Heavy(3, 0)))
    using (var editor = plain.CreateEditor())
    {
        linearized = editor.Save(new WriteOptions { Mode = WriteMode.Rewrite, Linearize = true });
    }
    using var saved = Document.Open(linearized);
    using var update = saved.CreateEditor();
    update.SetInfo(InfoKey.Title, "updated");
    return update.Save(new WriteOptions { Mode = WriteMode.Incremental });
}

// Every warning a document has raised, as its kind, place and sentence.
static List<string> Warned(Document document)
{
    var lines = new List<string>();
    using var warnings = document.ReadWarnings();
    for (uint index = 0; index < warnings.Count; index++)
    {
        var (offset, objectRef) = warnings.Location(index);
        var at = objectRef is null ? "-" : $"{objectRef.Value.Object}.{objectRef.Value.Generation}";
        lines.Add($"{warnings.Kind(index)} {offset} {at} {warnings.Message(index)}");
    }
    return lines;
}

static void OnDemand()
{
    var directory = Directory.CreateTempSubdirectory("tinker-pdf-smoke-");
    try
    {
        var data = Heavy(4, 1 << 20);
        var path = Path.Combine(directory.FullName, "heavy.pdf");
        File.WriteAllBytes(path, data);
        using (var byPath = Document.OpenFile(path))
        using (var byBytes = Document.Open(data))
        {
            if (byPath.PageCount != 4 || byBytes.PageCount != 4)
            {
                throw new Exception($"pages {byPath.PageCount} by path, {byBytes.PageCount} from bytes");
            }
            var text = byPath.PageText(0);
            if (text != byBytes.PageText(0) || !text.Contains("page 1 on demand", StringComparison.Ordinal))
            {
                throw new Exception($"page one reads {text} by path and {byBytes.PageText(0)} from bytes");
            }
            if (!byPath.IsStreamed)
            {
                throw new Exception("a file its bytes answer for stays on demand");
            }
            if (byBytes.IsStreamed)
            {
                throw new Exception("a document opened from bytes is not streamed");
            }
            Console.WriteLine($"on demand: {data.Length} bytes, {byPath.PageCount} pages, streamed");
        }
        foreach (var (name, bytes) in new[] { ("damaged", Damaged()), ("updated", Updated()) })
        {
            var file = Path.Combine(directory.FullName, name + ".pdf");
            File.WriteAllBytes(file, bytes);
            using var byPath = Document.OpenFile(file);
            using var byBytes = Document.Open(bytes);
            var fromPath = Warned(byPath);
            var fromBytes = Warned(byBytes);
            if (string.Join("\n", fromPath) != string.Join("\n", fromBytes))
            {
                throw new Exception(
                    $"{name}: warned [{string.Join("; ", fromPath)}] by path and " +
                    $"[{string.Join("; ", fromBytes)}] from bytes");
            }
            if (name == "damaged" && fromPath.Count == 0)
            {
                throw new Exception("damaged: an open of its bytes patches its table, and says so");
            }
            if (byPath.IsStreamed)
            {
                throw new Exception($"{name}: read whole");
            }
            Console.WriteLine($"{name}: read whole, {fromPath.Count} warnings, as its bytes");
        }
    }
    finally
    {
        directory.Delete(recursive: true);
    }
}

OnDemand();

// ---- the command stream -------------------------------------------------------
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

static void Check(bool ok, string what)
{
    if (!ok)
    {
        throw new Exception($"command stream: {what}");
    }
}

static void Expect(Action call, Status status, string what)
{
    try
    {
        call();
    }
    catch (PdfException e) when (e.Status == status)
    {
        return;
    }
    catch (PdfException e)
    {
        throw new Exception($"command stream: {what}: {e.Status} ({e.Message}), not {status}");
    }
    throw new Exception($"command stream: {what} must be refused with {status}");
}

// A stream object of `dictionary` holding `data`.
static byte[] StreamObject(string dictionary, byte[] data)
{
    var output = new MemoryStream();
    output.Write(Ascii($"<< {dictionary} /Length {data.Length} >>\nstream\n"));
    output.Write(data);
    output.Write(Ascii("\nendstream"));
    return output.ToArray();
}

// The 200 x 160 page whose stream holds every kind a page's stream is written
// with: a state with stored parameters, a black generation, a transfer
// function and a halftone; a clip and an axial shading in an ICC space; a fill
// in a /Separation space and one in a Lab space whose range is written
// backwards; text in an embedded face and in a font no name resolves; an image
// XObject and an inline image; marked content, a group under a luminosity soft
// mask then cleared; a tiling pattern, a form with resources of its own, a
// form that draws itself, and a square annotation with an appearance.
static byte[] EveryKindPage(byte[] serif, byte[] iccProfile)
{
    var widthList = new string[95];
    Array.Fill(widthList, "500");
    var widths = string.Join(" ", widthList);
    var content = new MemoryStream();
    content.Write(Ascii(
        "q /Fancy gs 0 0 1 RG 2 w 10 10 m 100 10 l S 1 0 0 rg 10 20 30 20 re f Q\n" +
        "q 10 40 40 30 re W n /Sh0 sh Q\n" +
        "q /CS0 cs 0.7 scn 60 20 20 20 re f Q\n" +
        "q /Lab0 cs 50 10 10 sc 90 20 10 10 re f Q\n" +
        "BT /F0 12 Tf 10 120 Td (Glyphs) Tj ET\n" +
        "BT /Nope 12 Tf 10 140 Td (x) Tj ET\n" +
        "q 20 0 0 20 120 20 cm /Im0 Do Q\n" +
        "q 10 0 0 10 150 20 cm BI /W 2 /H 2 /CS /G /BPC 8 ID "));
    content.Write(new byte[] { 0x00, 0x55, 0xAA, 0xFF });
    content.Write(Ascii(
        " EI Q\n" +
        "/Span << /MCID 0 >> BDC 0 g 120 60 5 5 re f EMC\n" +
        "q /Masked gs 1 0 0 1 120 80 cm /G Do /NoMask gs 0 1 0 rg 0 0 5 5 re f Q\n" +
        "q /Pattern cs /P0 scn 150 60 30 30 re f Q\n" +
        "q 1 0 0 1 150 100 cm /Fm0 Do Q\n" +
        "/Rec Do\n"));
    var rgb = new byte[] { 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0 };
    return Pdf(new List<byte[]>
    {
        Ascii("<< /Type /Catalog /Pages 2 0 R >>"),
        Ascii("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        Ascii("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] /Resources 4 0 R " +
              "/Contents 5 0 R /Annots [17 0 R] >>"),
        Ascii("<< /Font << /F0 6 0 R >> " +
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
        StreamObject("", content.ToArray()),
        Ascii("<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 " +
              $"/LastChar 126 /Widths [{widths}] /FontDescriptor 7 0 R " +
              "/Encoding /WinAnsiEncoding >>"),
        Ascii("<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 " +
              "/FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 " +
              "/CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"),
        StreamObject($"/Length1 {serif.Length}", serif),
        StreamObject(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB " +
            "/BitsPerComponent 8",
            rgb),
        StreamObject(
            "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >>",
            Ascii("0 0.8 0 rg 0 0 20 20 re f")),
        StreamObject(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 40] " +
            "/Resources << /XObject << /ImF 9 0 R >> >>",
            Ascii("q 10 0 0 10 0 0 cm /ImF Do Q")),
        StreamObject(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] " +
            "/Resources << /XObject << /Rec 12 0 R >> >>",
            Ascii("0 0 1 rg 0 0 2 2 re f /Rec Do")),
        Ascii("<< /ShadingType 2 /ColorSpace [/ICCBased 14 0 R] /Coords [10 0 50 0] " +
              "/Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> " +
              "/Extend [true true] >>"),
        StreamObject("/N 3", iccProfile),
        StreamObject(
            "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 " +
            "/YStep 10 /Resources << >>",
            Ascii("1 0 0 rg 0 0 5 5 re f")),
        StreamObject(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 160] " +
            "/Group << /S /Transparency /CS /DeviceGray >>",
            Ascii("0.5 g 0 0 200 160 re f")),
        Ascii("<< /Type /Annot /Subtype /Square /Rect [150 120 190 150] /AP << /N 18 0 R >> >>"),
        StreamObject("/Type /XObject /Subtype /Form /BBox [0 0 40 30]", Ascii("0 0 1 RG 2 w 1 1 38 28 re S")),
    });
}

// `fields`, each made again from its tag and value through the binding's
// constructors.
static CommandField[] Rebuilt(IReadOnlyList<CommandField> fields)
{
    var rebuilt = new CommandField[fields.Count];
    for (var at = 0; at < fields.Count; at++)
    {
        var field = fields[at];
        var again = field.Tag switch
        {
            CommandFieldTag.Nothing => CommandField.Nothing(),
            CommandFieldTag.Unsigned => CommandField.Unsigned((ulong)field.Value!),
            CommandFieldTag.Signed => CommandField.Signed((long)field.Value!),
            CommandFieldTag.Real => CommandField.Real((double)field.Value!),
            CommandFieldTag.Bytes => CommandField.Bytes((byte[])field.Value!),
            CommandFieldTag.Reals => CommandField.Reals((double[])field.Value!),
            CommandFieldTag.Naturals => CommandField.Naturals((ulong[])field.Value!),
            CommandFieldTag.Strings => CommandField.Strings((IReadOnlyList<byte[]>)field.Value!),
            _ => throw new Exception($"command stream: a field of tag {field.Tag}"),
        };
        Check(again.Equals(field) && again.GetHashCode() == field.GetHashCode(), $"a field rebuilds: {field}");
        rebuilt[at] = again;
    }
    return rebuilt;
}

// Field `at`'s unsigned value.
static ulong Natural(CommandRecord record, int at)
{
    var field = record.Fields[at];
    Check(field.Tag == CommandFieldTag.Unsigned, $"{record.Kind}'s field {at} is {field}");
    return (ulong)field.Value!;
}

// Field `at`'s bytes.
static byte[] BytesOf(CommandRecord record, int at)
{
    var field = record.Fields[at];
    Check(field.Tag == CommandFieldTag.Bytes, $"{record.Kind}'s field {at} is {field}");
    return (byte[])field.Value!;
}

static long[] Bits(double[] reals) => Array.ConvertAll(reals, BitConverter.DoubleToInt64Bits);

static void CommandStreams(string root)
{
    var serif = File.ReadAllBytes(
        Path.Combine(root, "crates", "tinker-pdf-font", "data", "liberation", "LiberationSerif-Regular.ttf"));
    var iccProfile = File.ReadAllBytes(Path.Combine(root, "fuzz", "corpus", "icc_profile", "rgb-gamma-curve.icc"));
    byte[] written;
    (double Width, double Height) pageSize;
    byte[] direct;
    (uint Width, uint Height) directSize;
    using (var everyKind = Document.Open(EveryKindPage(serif, iccProfile)))
    {
        written = everyKind.CommandStream(0);
        pageSize = everyKind.PageSize(0);
        using (var bitmap = everyKind.Render(0, 1.0, PixelFormat.Rgb8))
        {
            direct = bitmap.Pixels.ToArray();
            directSize = (bitmap.Width, bitmap.Height);
        }
        Expect(() => everyKind.CommandStream(1), Status.NoSuchPage, "a stream of a page past the end");
    }

    using var stream = CommandStream.Parse(written);
    Check(stream.ToBytes().AsSpan().SequenceEqual(written), "the stream writes back its own bytes");
    Check(stream.PageIndex == 0, $"the stream is page 0's, not {stream.PageIndex}");
    Check(stream.Size == pageSize, "the stream's page is the page's size");

    var records = stream.Records();
    Check(records.Count == stream.Count, "every record, counted");
    Check(records[0].Kind == CommandKind.Page, $"the page record leads: {records[0].Kind}");
    foreach (var record in records)
    {
        var command = stream.Next();
        Check(command.Kind is not null, $"kind {record.Number} is one this build knows");
        Check(command == stream.CommandAt(record.Index), $"the cursor reads record {record.Index} as its place does");
        Check(
            command.Kind == record.Kind && command.Number == record.Number && command.FieldCount == record.Fields.Count,
            $"record {record.Index}'s command");
    }
    for (var twice = 0; twice < 2; twice++)
    {
        var end = stream.Next();
        Check(
            end.Kind == CommandKind.End && end.Number == 0 && end.Index == records.Count && end.FieldCount == 0,
            $"the end, after the last record: {end}");
    }
    stream.Rewind();
    Check(stream.Next() == stream.CommandAt(0), "a rewind puts the cursor back on the page record");
    Expect(() => stream.CommandAt((uint)records.Count), Status.BadArgument, "a record past the end");
    Expect(() => stream.FieldAt(0, (uint)records[0].Fields.Count), Status.BadArgument, "a field past the page record's last");

    using (var encoder = new CommandEncoder())
    {
        foreach (var record in records)
        {
            encoder.Push(record.Kind!.Value, Rebuilt(record.Fields));
        }
        Check(
            encoder.Finish().AsSpan().SequenceEqual(written),
            "the stream decoded and written back through the binding's types is the facade's bytes");
    }

    var onThePage = new HashSet<CommandKind>();
    foreach (var record in records)
    {
        onThePage.Add(record.Kind!.Value);
    }
    var byHand = new[] { CommandKind.Colorants, CommandKind.GroupBudgetSpent, CommandKind.NestedBudgetSpent };
    foreach (var kind in Enum.GetValues<CommandKind>())
    {
        if (kind != CommandKind.End && Array.IndexOf(byHand, kind) < 0)
        {
            Check(onThePage.Contains(kind), $"the page's stream holds a {kind} record");
        }
    }

    var colorants = new[]
    {
        CommandField.Unsigned(1),
        CommandField.Strings(new[] { Ascii("Spot"), Ascii("Other") }),
        CommandField.Bytes(new byte[] { 128, 64 }),
        CommandField.Naturals(new ulong[] { 1 }),
        CommandField.Bytes(new byte[] { 0, 0, 0, 255 }),
        CommandField.Nothing(),
        CommandField.Bytes(new byte[] { 10, 20, 30, 40 }),
    };
    byte[] hand;
    using (var encoder = new CommandEncoder())
    {
        encoder.Push(records[0]);
        encoder.Push(CommandKind.Colorants, colorants);
        encoder.Push(CommandKind.GroupBudgetSpent, Array.Empty<CommandField>());
        encoder.Push(CommandKind.NestedBudgetSpent, Array.Empty<CommandField>());
        hand = encoder.Finish();
        Expect(() => encoder.Push(CommandKind.End, Array.Empty<CommandField>()), Status.BadArgument, "the end pushed");
        Expect(
            () => encoder.Push((CommandKind)99, Array.Empty<CommandField>()),
            Status.BadArgument,
            "a kind the C ABI does not declare");
        Check(encoder.Finish().AsSpan().SequenceEqual(hand), "an encoder keeps its records, and a refused push adds none");
    }
    using (var lonely = new CommandEncoder())
    {
        lonely.Push(CommandKind.GroupBudgetSpent, Array.Empty<CommandField>());
        Expect(() => lonely.Finish(), Status.BadArgument, "records with no page record");
    }

    var heard = new HashSet<CommandKind>();
    using (var reread = CommandStream.Parse(hand))
    {
        var back = reread.Records();
        var kinds = new List<CommandKind?>();
        foreach (var record in back)
        {
            kinds.Add(record.Kind);
        }
        Check(
            System.Linq.Enumerable.SequenceEqual(kinds, new CommandKind?[]
            {
                CommandKind.Page, CommandKind.Colorants, CommandKind.GroupBudgetSpent, CommandKind.NestedBudgetSpent,
            }),
            $"the records pushed by hand read back in order: {string.Join(", ", kinds)}");
        Check(
            System.Linq.Enumerable.SequenceEqual(back[0].Fields, records[0].Fields)
                && System.Linq.Enumerable.SequenceEqual(back[1].Fields, colorants)
                && back[2].Fields.Count == 0
                && back[3].Fields.Count == 0,
            "the records pushed by hand read back as pushed");
        Check(reread.Version == stream.Version, "the page's stream is at the version the encoder writes, this build's");
        using (var encoder = new CommandEncoder())
        {
            foreach (var record in back)
            {
                encoder.Push(record.Kind!.Value, Rebuilt(record.Fields));
            }
            Check(encoder.Finish().AsSpan().SequenceEqual(hand), "and write back through the binding's types");
        }
        foreach (var each in new[] { stream, reread })
        {
            var recorder = new Recorder();
            each.Replay(recorder);
            var order = new List<CommandKind?>();
            foreach (var record in each.Records())
            {
                order.Add(record.Kind);
            }
            Check(
                System.Linq.Enumerable.SequenceEqual(recorder.Seen, order),
                "the device receives every record in the decoder's order");
            foreach (var kind in recorder.Seen)
            {
                heard.Add(kind!.Value);
            }
        }
    }
    foreach (var kind in Enum.GetValues<CommandKind>())
    {
        if (kind != CommandKind.End)
        {
            Check(heard.Contains(kind), $"the device heard a {kind} record");
        }
    }

    var glyphs = new GlyphRuns();
    stream.Replay(glyphs);
    var runs = 0;
    foreach (var record in records)
    {
        if (record.Kind == CommandKind.GlyphRun)
        {
            runs += 1;
        }
    }
    Check(glyphs.Runs == runs, "a device naming one method hears only that kind");
    try
    {
        stream.Replay(new Refusing());
        throw new Exception("command stream: a device's exception ends the replay");
    }
    catch (InvalidOperationException e)
    {
        Check(e.Message == "the device refuses", $"the device's exception is thrown: {e.Message}");
    }

    var outlines = 0;
    var decodes = 0;
    foreach (var record in records)
    {
        if (record.Kind == CommandKind.Outline)
        {
            var outline = stream.OutlineAt(record.Index);
            Check(
                outline.Scope == Natural(record, 0)
                    && outline.FontId == Natural(record, 1)
                    && outline.Code == Natural(record, 2)
                    && outline.Found == (Natural(record, 3) == 1)
                    && outline.Operators.AsSpan().SequenceEqual(BytesOf(record, 4))
                    && Bits(outline.Numbers).AsSpan().SequenceEqual(Bits((double[])record.Fields[5].Value!)),
                $"outline record {record.Index} is its fields");
            outlines += 1;
        }
        else if (record.Kind == CommandKind.ImageDecode)
        {
            var image = stream.ImageAt(record.Index);
            Check(
                image.Image == Natural(record, 0)
                    && image.Reduction == Natural(record, 1)
                    && (ulong)image.Answer == Natural(record, 2),
                $"image decode record {record.Index} is its image, reduction and answer");
            if (image.Answer == ImageAnswer.Image)
            {
                var held = record.Fields[8].Tag == CommandFieldTag.Bytes;
                Check(
                    image.Width == Natural(record, 4)
                        && image.Height == Natural(record, 5)
                        && image.Rgb.AsSpan().SequenceEqual(BytesOf(record, 6))
                        && image.Alpha.AsSpan().SequenceEqual(BytesOf(record, 7))
                        && image.TwoColour.AsSpan().SequenceEqual(held ? BytesOf(record, 8) : Array.Empty<byte>())
                        && image.Colours.AsSpan().SequenceEqual(held ? BytesOf(record, 9) : new byte[8])
                        && image.Stencil == (Natural(record, 10) == 1)
                        && image.Interpolate == (Natural(record, 11) == 1),
                    $"image decode record {record.Index} is its samples and colours");
                var window = record.Fields[15];
                if (window.Tag == CommandFieldTag.Naturals)
                {
                    var edges = (ulong[])window.Value!;
                    Check(
                        image.Window is { } at
                            && at.X == edges[0]
                            && at.Y == edges[1]
                            && at.Width == edges[2]
                            && at.Height == edges[3],
                        $"image decode record {record.Index}'s window");
                }
                else
                {
                    Check(image.Window is null, $"image decode record {record.Index} is all of its decode");
                }
            }
            else if (image.Answer == ImageAnswer.Refused)
            {
                Check(
                    image.Codec == System.Text.Encoding.UTF8.GetString(BytesOf(record, 3)),
                    "a refused decode names its codec");
            }
            decodes += 1;
        }
    }
    Check(outlines > 0 && decodes > 0, $"{outlines} outlines and {decodes} decodes");
    Expect(() => stream.OutlineAt(0), Status.BadArgument, "an outline asked of the page record");
    Expect(() => stream.ImageAt(0), Status.BadArgument, "a decode asked of the page record");

    using (var replayed = stream.Render(1.0, PixelFormat.Rgb8))
    {
        Check(
            replayed.Width == directSize.Width
                && replayed.Height == directSize.Height
                && replayed.Pixels.SequenceEqual(direct),
            "the replay draws what a render of the page draws");
    }

    foreach (var refused in new[] { Array.Empty<byte>(), Ascii("not a command stream"), written[..^1] })
    {
        Expect(() => CommandStream.Parse(refused).Dispose(), Status.BadArgument, "bytes that are no stream");
    }
    try
    {
        CommandStream.Parse(Ascii("plainly not one")).Dispose();
        throw new Exception("command stream: plainly not a stream, and read");
    }
    catch (PdfException e)
    {
        Check(e.Message.Contains("not a command stream", StringComparison.Ordinal), $"the reader's sentence: {e.Message}");
    }
    Console.WriteLine(
        $"command stream: {records.Count} records, every kind, written back to the facade's bytes");
}

CommandStreams(Path.GetDirectoryName(Path.GetDirectoryName(Path.GetFullPath(args[0])))!);

Console.WriteLine("DOTNET-SMOKE: RAN, rendered, inked, opened on demand and the command stream written back");

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
    using (var labels = document.ReadPageLabels())
    {
        for (uint index = 0; index < labels.Count; index++)
        {
            lines.Add("label " + index + " " + TextToken(labels.Label(index)));
        }
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
using (var formEditor = form.CreateEditor())
{
    void Added(string label, (uint Object, ushort Generation) reference) =>
        formLines.Add($"added {TextToken(label)} {reference.Object}.{reference.Generation}");
    Added("person.given", formEditor.AddTextField(
        "person.given", 0, 300, 700, 500, 720, value: "Ada", maxLen: 20));
    Added("subscribe", formEditor.AddCheckbox(
        "subscribe", 0, 300, 660, 320, 680, "Yes", true, flags: 2));
    Added("size", formEditor.AddRadioGroup(
        "size",
        new[] { new RadioButton("S", 0, 300, 620, 320, 640), new RadioButton("M", 0, 330, 620, 350, 640) },
        selected: "M"));
    Added("country", formEditor.AddChoiceField(
        "country", 0, 300, 580, 400, 600, new[] { "NZ", "LK", "UK" }, true, value: "LK", fontSize: 10));
    Added("languages", formEditor.AddChoiceField(
        "languages", 0, 300, 500, 400, 560, new[] { "en", "fr" }, false));
    using var fixtureData = FormData.ReadXfdf(File.ReadAllBytes(Path.Combine(formDataDir, "form-fields.xfdf")));
    var widgets = new List<string>();
    foreach (var widget in formEditor.ApplyFormData(fixtureData))
    {
        widgets.Add($"{widget.ObjectNumber}.{widget.Generation}");
    }
    formLines.Add("applied " + (widgets.Count == 0 ? "-" : string.Join(",", widgets)));
    formed = formEditor.Save(new WriteOptions());
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

// Script eight: tagged. Structure elements through OpenTag and CloseTag.
static void OpenTagged(PageBuilder page, string kind, Action<Tag> configure)
{
    using var tag = new Tag(System.Text.Encoding.ASCII.GetBytes(kind));
    configure(tag);
    page.OpenTag(tag);
}

byte[] taggedBytes;
using (var taggedBuilder = new DocumentBuilder())
{
    taggedBuilder.AddBaseFont("F1"u8.ToArray(), "Helvetica"u8.ToArray());
    taggedBuilder.SetLanguage("en-GB");
    taggedBuilder.MapRole("Heading"u8.ToArray(), "H1"u8.ToArray());
    using (var one = taggedBuilder.BeginPage(200.0, 200.0))
    {
        OpenTagged(one, "Heading", t => t.SetText(TagText.Title, "Introduction"));
        one.Text("F1"u8.ToArray(), 14, 20, 170, "Tagged parity");
        one.CloseTag();
        OpenTagged(one, "P", t =>
        {
            t.SetText(TagText.Lang, "fr");
            t.SetText(TagText.ActualText, "Bonjour");
        });
        one.Text("F1"u8.ToArray(), 12, 20, 150, "Bon");
        taggedBuilder.PushPage(one);
    }
    using (var two = taggedBuilder.BeginPage(200.0, 200.0))
    {
        two.Text("F1"u8.ToArray(), 12, 20, 170, "jour");
        two.CloseTag();
        var refusedClose = false;
        try
        {
            two.CloseTag();
        }
        catch (PdfException e) when (e.Status == Status.EditRefused)
        {
            refusedClose = true;
        }
        if (!refusedClose)
        {
            throw new Exception("a close with nothing open must be refused");
        }
        OpenTagged(two, "Figure", t => t.SetText(TagText.Alt, "A grey square"));
        two.FillRect(20, 100, 40, 40, 0.5);
        two.CloseTag();
        OpenTagged(two, "Span", t =>
        {
            t.SetText(TagText.Expansion, "Portable Document Format");
            t.SetId("pdf-1"u8.ToArray());
        });
        two.Text("F1"u8.ToArray(), 12, 20, 80, "PDF");
        two.CloseTag();
        OpenTagged(two, "Div", t => t.KeepEmpty());
        two.CloseTag();
        OpenTagged(two, "P", t => t.SetKey(7, 1));
        two.Text("F1"u8.ToArray(), 12, 20, 60, "read second");
        two.CloseTag();
        OpenTagged(two, "P", t => t.SetKey(7, 0));
        two.Text("F1"u8.ToArray(), 12, 20, 40, "read first");
        two.CloseTag();
        taggedBuilder.PushPage(two);
    }
    taggedBytes = taggedBuilder.Finish();
}
Report("tagged", taggedBytes);

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

// A device that notes every record it is handed, each by the method its kind
// names: the command stream leg's recording device.
sealed class Recorder : ICommandDevice
{
    internal readonly List<CommandKind?> Seen = new();

    private void Note(CommandKind kind, CommandRecord record)
    {
        if (record.Kind != kind)
        {
            throw new Exception($"command stream: the method for {kind} was handed a {record.Kind}");
        }
        Seen.Add(kind);
    }

    public void Function(CommandRecord record) => Note(CommandKind.Function, record);
    public void Halftone(CommandRecord record) => Note(CommandKind.Halftone, record);
    public void Stored(CommandRecord record) => Note(CommandKind.Stored, record);
    public void Colorants(CommandRecord record) => Note(CommandKind.Colorants, record);
    public void Source(CommandRecord record) => Note(CommandKind.Source, record);
    public void State(CommandRecord record) => Note(CommandKind.State, record);
    public void BeginText(CommandRecord record) => Note(CommandKind.BeginText, record);
    public void GlyphRun(CommandRecord record) => Note(CommandKind.GlyphRun, record);
    public void EndText(CommandRecord record) => Note(CommandKind.EndText, record);
    public void FillPath(CommandRecord record) => Note(CommandKind.FillPath, record);
    public void StrokePath(CommandRecord record) => Note(CommandKind.StrokePath, record);
    public void ClipPath(CommandRecord record) => Note(CommandKind.ClipPath, record);
    public void SaveState(CommandRecord record) => Note(CommandKind.SaveState, record);
    public void RestoreState(CommandRecord record) => Note(CommandKind.RestoreState, record);
    public void DrawImage(CommandRecord record) => Note(CommandKind.DrawImage, record);
    public void DrawShading(CommandRecord record) => Note(CommandKind.DrawShading, record);
    public void BeginMarkedContent(CommandRecord record) => Note(CommandKind.BeginMarkedContent, record);
    public void EndMarkedContent(CommandRecord record) => Note(CommandKind.EndMarkedContent, record);
    public void BeginForm(CommandRecord record) => Note(CommandKind.BeginForm, record);
    public void EndForm(CommandRecord record) => Note(CommandKind.EndForm, record);
    public void BeginGroup(CommandRecord record) => Note(CommandKind.BeginGroup, record);
    public void EndGroup(CommandRecord record) => Note(CommandKind.EndGroup, record);
    public void BeginSoftMask(CommandRecord record) => Note(CommandKind.BeginSoftMask, record);
    public void EndSoftMask(CommandRecord record) => Note(CommandKind.EndSoftMask, record);
    public void ClearSoftMask(CommandRecord record) => Note(CommandKind.ClearSoftMask, record);
    public void GroupBudgetSpent(CommandRecord record) => Note(CommandKind.GroupBudgetSpent, record);
    public void RecursionRefused(CommandRecord record) => Note(CommandKind.RecursionRefused, record);
    public void NestedBudgetSpent(CommandRecord record) => Note(CommandKind.NestedBudgetSpent, record);
    public void Page(CommandRecord record) => Note(CommandKind.Page, record);
    public void Annotation(CommandRecord record) => Note(CommandKind.Annotation, record);
    public void MissingFont(CommandRecord record) => Note(CommandKind.MissingFont, record);
    public void RepairedSpace(CommandRecord record) => Note(CommandKind.RepairedSpace, record);
    public void Scope(CommandRecord record) => Note(CommandKind.Scope, record);
    public void Outline(CommandRecord record) => Note(CommandKind.Outline, record);
    public void FontIdentity(CommandRecord record) => Note(CommandKind.FontIdentity, record);
    public void Profile(CommandRecord record) => Note(CommandKind.Profile, record);
    public void ColorSpace(CommandRecord record) => Note(CommandKind.ColorSpace, record);
    public void Image(CommandRecord record) => Note(CommandKind.Image, record);
    public void InlineImage(CommandRecord record) => Note(CommandKind.InlineImage, record);
    public void ImageDecode(CommandRecord record) => Note(CommandKind.ImageDecode, record);
    public void ShadingDefinition(CommandRecord record) => Note(CommandKind.ShadingDefinition, record);
    public void Shading(CommandRecord record) => Note(CommandKind.Shading, record);
    public void Pattern(CommandRecord record) => Note(CommandKind.Pattern, record);
    public void Cell(CommandRecord record) => Note(CommandKind.Cell, record);

    public void Unknown(CommandRecord record) =>
        throw new Exception($"command stream: a record of kind {record.Number}, which this build does not know");
}

// A device naming one method, which hears only that kind.
sealed class GlyphRuns : ICommandDevice
{
    internal int Runs;

    public void GlyphRun(CommandRecord record)
    {
        if (record.Kind != CommandKind.GlyphRun)
        {
            throw new Exception($"command stream: GlyphRun was handed a {record.Kind}");
        }
        Runs += 1;
    }
}

// A device whose exception ends the replay.
sealed class Refusing : ICommandDevice
{
    public void BeginText(CommandRecord record) => throw new InvalidOperationException("the device refuses");
}
