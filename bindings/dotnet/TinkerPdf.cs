// A safe C# wrapper over tinker-pdf's C ABI.
//
// The P/Invoke declarations below mirror `crates/tinker-pdf-ffi`. They are
// written out here rather than generated so the binding builds with nothing
// but the .NET SDK; regenerating them with csbindgen is a build-time
// convenience, not a requirement, and either way this file adds no logic of
// its own (ruling 11).
//
// Handle lifetime is the whole job of this wrapper. Every native handle lives
// in a SafeHandle, so a document or bitmap is released exactly once even if an
// exception unwinds past it, and the pixel span a caller gets is tied to the
// bitmap that owns it.

using System;
using System.IO;
using System.Runtime.InteropServices;

namespace TinkerPdf;

/// <summary>How far a password got.</summary>
public enum AuthLevel
{
    /// <summary>Unencrypted, or no password accepted yet.</summary>
    None = 0,

    /// <summary>The user password matched; the document's permissions apply.</summary>
    User = 1,

    /// <summary>The owner password matched; restrictions are lifted.</summary>
    Owner = 2,
}

/// <summary>How a bitmap stores its pixels.</summary>
public enum PixelFormat
{
    /// <summary>One byte of grey.</summary>
    Gray8 = 0,

    /// <summary>Grey and alpha.</summary>
    GrayA8 = 1,

    /// <summary>Red, green, blue.</summary>
    Rgb8 = 2,

    /// <summary>Red, green, blue, alpha.</summary>
    Rgba8 = 3,
}

/// <summary>Thrown when the engine reports a failure.</summary>
public sealed class PdfException : Exception
{
    internal PdfException(string message) : base(message) { }
}

internal static class Native
{
    private const string Library = "tinker_pdf_ffi";

    [DllImport(Library)]
    internal static extern IntPtr tpdf_last_error_message();

    [DllImport(Library)]
    internal static extern IntPtr tpdf_version();

    [DllImport(Library)]
    internal static extern int tpdf_document_open(byte[] bytes, nuint len, out IntPtr doc);

    [DllImport(Library)]
    internal static extern void tpdf_document_free(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_document_set_fonts(
        IntPtr doc,
        byte[]? regular, nuint regularLen,
        byte[]? bold, nuint boldLen,
        byte[]? italic, nuint italicLen,
        byte[]? boldItalic, nuint boldItalicLen);

    [DllImport(Library)]
    internal static extern uint tpdf_document_page_count(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_document_is_encrypted(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_document_authenticate(
        IntPtr doc, byte[] password, out int level);

    [DllImport(Library)]
    internal static extern int tpdf_document_may_print(IntPtr doc);

    [DllImport(Library)]
    internal static extern int tpdf_page_size(
        IntPtr doc, uint index, out double width, out double height);

    [DllImport(Library)]
    internal static extern int tpdf_page_text(IntPtr doc, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern void tpdf_string_free(IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_page_render(
        IntPtr doc, uint index, double scale, int format, out IntPtr bitmap);

    [DllImport(Library)]
    internal static extern uint tpdf_bitmap_width(IntPtr bitmap);

    [DllImport(Library)]
    internal static extern uint tpdf_bitmap_height(IntPtr bitmap);

    [DllImport(Library)]
    internal static extern nuint tpdf_bitmap_stride(IntPtr bitmap);

    [DllImport(Library)]
    internal static extern IntPtr tpdf_bitmap_data(IntPtr bitmap, out nuint len);

    [DllImport(Library)]
    internal static extern void tpdf_bitmap_free(IntPtr bitmap);

    internal static void Check(int status)
    {
        if (status == 0)
        {
            return;
        }
        var pointer = tpdf_last_error_message();
        var message = pointer == IntPtr.Zero
            ? $"tinker-pdf error {status}"
            : Marshal.PtrToStringUTF8(pointer) ?? $"tinker-pdf error {status}";
        throw new PdfException(message);
    }
}

internal sealed class DocumentHandle : SafeHandle
{
    internal DocumentHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_document_free(handle);
        return true;
    }
}

internal sealed class BitmapHandle : SafeHandle
{
    internal BitmapHandle() : base(IntPtr.Zero, ownsHandle: true) { }

    public override bool IsInvalid => handle == IntPtr.Zero;

    protected override bool ReleaseHandle()
    {
        Native.tpdf_bitmap_free(handle);
        return true;
    }
}

/// <summary>A rendered page.</summary>
public sealed class Bitmap : IDisposable
{
    private readonly BitmapHandle _handle;

    internal Bitmap(IntPtr raw)
    {
        _handle = new BitmapHandle();
        Marshal.InitHandle(_handle, raw);
    }

    /// <summary>Width in pixels.</summary>
    public uint Width => Native.tpdf_bitmap_width(_handle.DangerousGetHandle());

    /// <summary>Height in pixels.</summary>
    public uint Height => Native.tpdf_bitmap_height(_handle.DangerousGetHandle());

    /// <summary>Bytes per row.</summary>
    public nuint Stride => Native.tpdf_bitmap_stride(_handle.DangerousGetHandle());

    /// <summary>
    /// The pixels, as a span over the engine's own memory.
    /// </summary>
    /// <remarks>
    /// Zero-copy, and therefore only valid while this bitmap is alive. Copy it
    /// with <c>ToArray</c> if it must outlive the bitmap.
    /// </remarks>
    public ReadOnlySpan<byte> Pixels
    {
        get
        {
            var pointer = Native.tpdf_bitmap_data(_handle.DangerousGetHandle(), out var len);
            if (pointer == IntPtr.Zero)
            {
                return ReadOnlySpan<byte>.Empty;
            }
            unsafe
            {
                return new ReadOnlySpan<byte>(pointer.ToPointer(), checked((int)len));
            }
        }
    }

    /// <summary>Releases the bitmap.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>An open PDF document.</summary>
public sealed class Document : IDisposable
{
    private readonly DocumentHandle _handle;

    private Document(IntPtr raw)
    {
        _handle = new DocumentHandle();
        Marshal.InitHandle(_handle, raw);
    }

    /// <summary>The engine's version.</summary>
    public static string Version =>
        Marshal.PtrToStringUTF8(Native.tpdf_version()) ?? "unknown";

    /// <summary>Opens a document from bytes.</summary>
    public static Document Open(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        Native.Check(Native.tpdf_document_open(bytes, (nuint)bytes.Length, out var raw));
        return new Document(raw);
    }

    /// <summary>Opens a document from a file.</summary>
    public static Document OpenFile(string path) => Open(File.ReadAllBytes(path));

    /// <summary>The number of pages.</summary>
    public uint PageCount => Native.tpdf_document_page_count(_handle.DangerousGetHandle());

    /// <summary>Whether the document is encrypted.</summary>
    public bool IsEncrypted =>
        Native.tpdf_document_is_encrypted(_handle.DangerousGetHandle()) != 0;

    /// <summary>
    /// Whether the document permits printing.
    /// </summary>
    /// <remarks>
    /// PDF permissions are advisory: a document that says printing is denied
    /// is asking, not enforcing.
    /// </remarks>
    public bool MayPrint => Native.tpdf_document_may_print(_handle.DangerousGetHandle()) != 0;

    /// <summary>Tries a password, reporting which one matched.</summary>
    public AuthLevel Authenticate(string password)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(password + '\0');
        Native.Check(Native.tpdf_document_authenticate(
            _handle.DangerousGetHandle(), bytes, out var level));
        return (AuthLevel)level;
    }

    /// <summary>A page's size in points.</summary>
    public (double Width, double Height) PageSize(uint index)
    {
        Native.Check(Native.tpdf_page_size(
            _handle.DangerousGetHandle(), index, out var w, out var h));
        return (w, h);
    }

    /// <summary>A page's text.</summary>
    public string PageText(uint index)
    {
        Native.Check(Native.tpdf_page_text(_handle.DangerousGetHandle(), index, out var raw));
        try
        {
            return Marshal.PtrToStringUTF8(raw) ?? string.Empty;
        }
        finally
        {
            Native.tpdf_string_free(raw);
        }
    }

    /// <summary>
    /// Supplies a font for documents that embed none.
    /// </summary>
    /// <remarks>
    /// Without one, such a document extracts its text perfectly and draws none
    /// of it: the standard-14 metrics are built in, the outlines are not. The
    /// engine bundles no faces and reads no font directories, so a host that
    /// wants text drawn says where to find it — on .NET, usually a face read
    /// from the system font folder or shipped beside the application.
    ///
    /// The bytes are copied, so the arrays may be reused or collected after
    /// the call. <paramref name="regular"/> is required; the others fall back
    /// to it.
    /// </remarks>
    public void SetFonts(
        byte[] regular,
        byte[]? bold = null,
        byte[]? italic = null,
        byte[]? boldItalic = null)
    {
        ArgumentNullException.ThrowIfNull(regular);

        Native.Check(Native.tpdf_document_set_fonts(
            _handle.DangerousGetHandle(),
            regular, (nuint)regular.Length,
            bold, (nuint)(bold?.Length ?? 0),
            italic, (nuint)(italic?.Length ?? 0),
            boldItalic, (nuint)(boldItalic?.Length ?? 0)));
    }

    /// <summary>Renders a page. A scale of 1.0 is 72 dpi.</summary>
    public Bitmap Render(uint index, double scale = 1.0, PixelFormat format = PixelFormat.Rgb8)
    {
        Native.Check(Native.tpdf_page_render(
            _handle.DangerousGetHandle(), index, scale, (int)format, out var raw));
        return new Bitmap(raw);
    }

    /// <summary>Releases the document.</summary>
    public void Dispose() => _handle.Dispose();
}
