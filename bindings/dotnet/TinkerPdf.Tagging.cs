// Tagged writing over the C ABI: structure elements opened and closed on a
// page, the document's /Lang and its role map.
//
// Transcribed from crates/tinker-pdf-ffi/include/tinker_pdf.h. A Tag is a
// SafeHandle-owned engine copy; opening one copies it into the page. No logic
// of its own (ruling 11).

using System;
using System.Runtime.InteropServices;

namespace TinkerPdf;

/// <summary>Which text property <see cref="Tag.SetText"/> sets.</summary>
public enum TagText
{
    /// <summary><c>/T</c>, the element's title.</summary>
    Title = 0,
    /// <summary><c>/Lang</c>: a BCP 47 tag, or empty for unknown.</summary>
    Lang = 1,
    /// <summary><c>/Alt</c>, a description of content that is not text.</summary>
    Alt = 2,
    /// <summary><c>/ActualText</c>, what the content is where the glyphs do not say it.</summary>
    ActualText = 3,
    /// <summary><c>/E</c>, the expansion of an abbreviation.</summary>
    Expansion = 4,
}

internal static partial class Native
{
    [DllImport(Library)]
    internal static extern int tpdf_tag_new(byte[] kind, nuint kindLen, out IntPtr tag);

    [DllImport(Library)]
    internal static extern int tpdf_tag_set_text(IntPtr tag, int which, byte[] text);

    [DllImport(Library)]
    internal static extern int tpdf_tag_set_id(IntPtr tag, byte[] id, nuint idLen);

    [DllImport(Library)]
    internal static extern int tpdf_tag_set_key(IntPtr tag, ulong key, ulong order);

    [DllImport(Library)]
    internal static extern int tpdf_tag_keep_empty(IntPtr tag);

    [DllImport(Library)]
    internal static extern void tpdf_tag_free(IntPtr tag);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_open_tag(IntPtr page, IntPtr tag);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_close_tag(IntPtr page);

    [DllImport(Library)]
    internal static extern int tpdf_builder_set_language(IntPtr builder, byte[] language);

    [DllImport(Library)]
    internal static extern int tpdf_builder_map_role(
        IntPtr builder, byte[] custom, nuint customLen, byte[] standard, nuint standardLen);
}

/// <summary>A structure element to open (14.7.2): its type and properties.</summary>
public sealed class Tag : IDisposable
{
    private readonly ReadHandle _handle;

    /// <summary>An element of structure type <paramref name="kind"/>.</summary>
    public Tag(byte[] kind)
    {
        ArgumentNullException.ThrowIfNull(kind);
        Native.Check(Native.tpdf_tag_new(kind, (nuint)kind.Length, out var raw));
        _handle = new ReadHandle(raw, Native.tpdf_tag_free);
    }

    internal IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Sets <c>/T</c>, <c>/Lang</c>, <c>/Alt</c>, <c>/ActualText</c> or <c>/E</c>.</summary>
    public void SetText(TagText which, string text)
    {
        ArgumentNullException.ThrowIfNull(text);
        Native.Check(Native.tpdf_tag_set_text(Raw, (int)which, Native.Utf8(text)));
    }

    /// <summary>Sets <c>/ID</c>.</summary>
    public void SetId(byte[] id)
    {
        ArgumentNullException.ThrowIfNull(id);
        Native.Check(Native.tpdf_tag_set_id(Raw, id, (nuint)id.Length));
    }

    /// <summary>Names the element so its halves drawn apart are one element.</summary>
    public void SetKey(ulong key, ulong order) => Native.Check(Native.tpdf_tag_set_key(Raw, key, order));

    /// <summary>Writes the element even with nothing drawn inside it.</summary>
    public void KeepEmpty() => Native.Check(Native.tpdf_tag_keep_empty(Raw));

    /// <summary>Releases the tag; an element it opened stays open.</summary>
    public void Dispose() => _handle.Dispose();
}

public sealed partial class PageBuilder
{
    /// <summary>Opens the element <paramref name="tag"/> describes, until the matching
    /// <see cref="CloseTag"/>, across calls and pages.</summary>
    public void OpenTag(Tag tag)
    {
        ArgumentNullException.ThrowIfNull(tag);
        Native.Check(Native.tpdf_page_builder_open_tag(Raw, tag.Raw));
    }

    /// <summary>Closes the innermost element <see cref="OpenTag"/> opened.</summary>
    public void CloseTag() => Native.Check(Native.tpdf_page_builder_close_tag(Raw));
}

public sealed partial class DocumentBuilder
{
    /// <summary>Sets the catalog's <c>/Lang</c>: a BCP 47 tag, or empty for unknown.</summary>
    public void SetLanguage(string language)
    {
        ArgumentNullException.ThrowIfNull(language);
        Native.Check(Native.tpdf_builder_set_language(Raw, Native.Utf8(language)));
    }

    /// <summary>Maps a structure type of the caller's own to a standard one.</summary>
    public void MapRole(byte[] custom, byte[] standard)
    {
        ArgumentNullException.ThrowIfNull(custom);
        ArgumentNullException.ThrowIfNull(standard);
        Native.Check(Native.tpdf_builder_map_role(
            Raw, custom, (nuint)custom.Length, standard, (nuint)standard.Length));
    }
}
