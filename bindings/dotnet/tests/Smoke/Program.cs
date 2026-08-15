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
using System.IO;
using TinkerPdf;

if (args.Length != 2)
{
    Console.Error.WriteLine("usage: Smoke <file.pdf> <face.ttf>");
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
return 0;
