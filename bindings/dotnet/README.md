# .NET binding

A safe C# wrapper over `tinker-pdf-ffi`'s C ABI. Handle lifetime is the whole
job of it: every native handle lives in a `SafeHandle`, so a document or bitmap
is released exactly once even if an exception unwinds past it. Scope and
design: [`docs/plans/13-bindings.md`](../../docs/plans/13-bindings.md);
packaging: [gap 26](../../docs/plans/gaps/26-binding-packaging.md).

```csharp
using var document = Document.Open(File.ReadAllBytes("file.pdf"));
Console.WriteLine(document.PageText(0));

// The engine bundles no font faces and reads no font directories, so a
// document that embeds none extracts its text perfectly and draws none of it.
document.SetFonts(File.ReadAllBytes(@"C:\Windows\Fonts\arial.ttf"));

using var bitmap = document.Render(0, scale: 2.0);
ReadOnlySpan<byte> pixels = bitmap.Pixels;   // zero-copy, valid while alive
```

The P/Invoke declarations in `TinkerPdf.cs` are written out rather than
generated, so the binding builds with nothing but the .NET SDK. csbindgen is a
build-time convenience, not a requirement; either way this layer adds no logic
of its own (ruling 11).

## Per-RID natives, which is the part that can be wrong

A NuGet package carries platform binaries as `runtimes/<rid>/native/`, and the
.NET host picks one at run time. Building the package is two steps:

```bash
cargo build --release -p tinker-pdf-ffi
cargo run -p xtask -- nuget-stage      # -> runtimes/<this machine's rid>/native/
dotnet pack bindings/dotnet/TinkerPdf.csproj -c Release -o target/nuget
```

`nuget-stage` reads the host OS and architecture and maps them to a RID —
`win-x64`, `linux-x64`, `osx-arm64` and the three arm/x64 counterparts — and to
the platform's library name. That mapping is in `xtask` with a unit test rather
than in three `cp` lines, because it is the thing that fails silently: a
package built with the wrong RID restores, compiles, and throws
`DllNotFoundException` the first time anybody calls it.

A single machine can build only its own RID. The full package is assembled by
[`.github/workflows/release.yml`](../../.github/workflows/release.yml), which
builds the cdylib on three runners and gathers them before packing — and then
greps the `.nupkg` for all three, because `dotnet pack` on an empty
`runtimes/` produces a perfectly valid managed-only package.

## Proving an installed package works

```bash
dotnet run --project bindings/dotnet/tests/Smoke -c Release -- \
  testdata/simple-text.pdf C:/Windows/Fonts/arial.ttf
```

The smoke project takes a `PackageReference` on `TinkerPdf` from a local
folder feed — **not** a `ProjectReference`. A project reference resolves the
managed assembly and finds the cdylib wherever cargo left it, so it passes on
a package that carries no native library at all, which is the one failure this
milestone is about. Its `nuget.config` clears the source list before adding the
folder, so a restore that cannot find the local package fails rather than
quietly taking something of the same name from nuget.org.

As in the Python and JavaScript smoke tests, the render is asserted twice —
blank without a face, inked with one — because `testdata/simple-text.pdf`
embeds no font program and this engine bundles no faces.

## Nothing has been published

`dotnet add package TinkerPdf` does not work and is not meant to yet. The
pipeline exists and has been exercised as a dry run; the facade is not frozen
until 0.1.0 ([plan 00](../../docs/plans/00-architecture.md)).
