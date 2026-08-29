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
// The same two scripts `crates/tinker-pdf/examples/write_parity.rs`,
// `bindings/python/tests/write_parity.py` and
// `bindings/js/tests/write_parity.mjs` run. `cargo xtask bindings-parity`
// requires all four to print the same SHA-256s: ruling 11 says a binding
// projects the facade 1:1 and adds no logic of its own, so four surfaces
// disagreeing means one of them added something.
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
