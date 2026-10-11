// The builder's graphics resources over the C ABI: graphics states, form
// XObjects and tiling patterns, a font under a named encoding and text in
// codes the caller chose, a page's bleed box, a declared version, and the
// image list a later page stops inheriting.
//
// Transcribed from crates/tinker-pdf-ffi/include/tinker_pdf.h. A graphics
// state starts from tpdf_ext_gstate_init, never from a zeroed struct (whose
// alphas are 0). No logic of its own (ruling 11).

using System;
using System.Runtime.InteropServices;

namespace TinkerPdf;

/// <summary>11.3.5's sixteen blend modes.</summary>
public enum BlendMode
{
    /// <summary><c>/Normal</c>.</summary>
    Normal = 0,
    /// <summary><c>/Multiply</c>.</summary>
    Multiply = 1,
    /// <summary><c>/Screen</c>.</summary>
    Screen = 2,
    /// <summary><c>/Overlay</c>.</summary>
    Overlay = 3,
    /// <summary><c>/Darken</c>.</summary>
    Darken = 4,
    /// <summary><c>/Lighten</c>.</summary>
    Lighten = 5,
    /// <summary><c>/ColorDodge</c>.</summary>
    ColorDodge = 6,
    /// <summary><c>/ColorBurn</c>.</summary>
    ColorBurn = 7,
    /// <summary><c>/HardLight</c>.</summary>
    HardLight = 8,
    /// <summary><c>/SoftLight</c>.</summary>
    SoftLight = 9,
    /// <summary><c>/Difference</c>.</summary>
    Difference = 10,
    /// <summary><c>/Exclusion</c>.</summary>
    Exclusion = 11,
    /// <summary><c>/Hue</c>.</summary>
    Hue = 12,
    /// <summary><c>/Saturation</c>.</summary>
    Saturation = 13,
    /// <summary><c>/Color</c>.</summary>
    Color = 14,
    /// <summary><c>/Luminosity</c>.</summary>
    Luminosity = 15,
}

/// <summary>Which <c>/SMask</c> a graphics state writes.</summary>
public enum SoftMask
{
    /// <summary>No entry: the mask in force is inherited.</summary>
    Absent = 0,
    /// <summary><c>/SMask /None</c>: the mask in force is turned off.</summary>
    None = 1,
    /// <summary>A mask over a transparency-group form.</summary>
    Group = 2,
}

/// <summary>What a group soft mask derives its alpha from.</summary>
public enum MaskKind
{
    /// <summary><c>/S /Alpha</c>.</summary>
    Alpha = 0,
    /// <summary><c>/S /Luminosity</c>.</summary>
    Luminosity = 1,
}

/// <summary>A device colour space.</summary>
public enum DeviceSpace
{
    /// <summary><c>/DeviceGray</c>.</summary>
    Gray = 0,
    /// <summary><c>/DeviceRGB</c>.</summary>
    Rgb = 1,
    /// <summary><c>/DeviceCMYK</c>.</summary>
    Cmyk = 2,
}

/// <summary><c>/TilingType</c>, counted from zero: ConstantSpacing writes 1.</summary>
public enum TilingType
{
    /// <summary>1: constant spacing.</summary>
    ConstantSpacing = 0,
    /// <summary>2: no distortion.</summary>
    NoDistortion = 1,
    /// <summary>3: constant spacing and faster tiling.</summary>
    FasterTiling = 2,
}

/// <summary>A graphics state's overrides (Table 58); a null member writes no entry.</summary>
public sealed record ExtGState(
    double? FillAlpha = null,
    double? StrokeAlpha = null,
    BlendMode? BlendMode = null,
    SoftMask SoftMask = SoftMask.Absent,
    MaskKind MaskKind = MaskKind.Alpha,
    byte[]? MaskForm = null,
    double[]? Backdrop = null);

/// <summary>A form's <c>/Group</c> (11.6.6).</summary>
public readonly record struct TransparencyGroup(DeviceSpace ColorSpace, bool Isolated, bool Knockout);

/// <summary><c>TpdfExtGState</c>, field for field; 64 bytes.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct ExtGStateRaw
{
    internal double FillAlpha;
    internal double StrokeAlpha;
    internal int HasBlendMode;
    internal int BlendMode;
    internal int SoftMask;
    internal int MaskKind;
    internal IntPtr MaskForm;
    internal nuint MaskFormLen;
    internal IntPtr Backdrop;
    internal nuint BackdropLen;
}

/// <summary><c>TpdfTransparencyGroup</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct TransparencyGroupRaw
{
    internal int ColorSpace;
    internal int Isolated;
    internal int Knockout;
}

internal static partial class Native
{
    [DllImport(Library)]
    internal static extern int tpdf_builder_new_with_version(uint major, uint minor, out IntPtr builder);

    [DllImport(Library)]
    internal static extern int tpdf_builder_clear_image_resources(IntPtr builder);

    [DllImport(Library)]
    internal static extern int tpdf_builder_add_named_font(
        IntPtr builder, byte[] resource, nuint resourceLen, byte[] baseFont, nuint baseFontLen,
        uint firstCode, IntPtr[] names, nuint nameCount, ushort[] widths, nuint widthCount);

    [DllImport(Library)]
    internal static extern int tpdf_ext_gstate_init(out ExtGStateRaw state);

    [DllImport(Library)]
    internal static extern int tpdf_builder_add_ext_gstate(
        IntPtr builder, byte[] resource, nuint resourceLen, ref ExtGStateRaw state);

    [DllImport(Library)]
    internal static extern int tpdf_builder_add_form(
        IntPtr builder, byte[] resource, nuint resourceLen, double x0, double y0, double x1, double y1,
        double[]? matrix, IntPtr group, byte[] content, nuint contentLen);

    [DllImport(Library)]
    internal static extern int tpdf_builder_add_tiling_pattern(
        IntPtr builder, byte[] resource, nuint resourceLen, double x0, double y0, double x1, double y1,
        double xStep, double yStep, double[]? matrix, int tilingType, byte[] content, nuint contentLen);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_set_bleed_box(
        IntPtr page, double x0, double y0, double x1, double y1);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_encoded_text(
        IntPtr page, byte[] font, nuint fontLen, double size, double x, double y,
        double characterSpacing, double wordSpacing, byte[] codes, nuint codesLen, byte[] characters);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_set_ext_gstate(IntPtr page, byte[] resource, nuint resourceLen);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_form(IntPtr page, byte[] resource, nuint resourceLen);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_set_fill_pattern(IntPtr page, byte[] resource, nuint resourceLen);

    [DllImport(Library)]
    internal static extern int tpdf_page_builder_set_stroke_pattern(IntPtr page, byte[] resource, nuint resourceLen);
}

public sealed partial class DocumentBuilder
{
    private DocumentBuilder(IntPtr raw)
    {
        _handle = new BuilderHandle();
        Marshal.InitHandle(_handle, raw);
    }

    /// <summary>Starts a document whose header declares PDF <c>major.minor</c> (7.5.2).</summary>
    public static DocumentBuilder WithVersion(uint major, uint minor)
    {
        Native.Check(Native.tpdf_builder_new_with_version(major, minor, out var raw));
        return new DocumentBuilder(raw);
    }

    /// <summary>Stops later pages inheriting the images registered so far.</summary>
    public void ClearImageResources() => Native.Check(Native.tpdf_builder_clear_image_resources(Raw));

    /// <summary>One of the standard 14 under an <c>/Encoding</c> of glyph names from
    /// <paramref name="firstCode"/>, with their widths.</summary>
    public void AddNamedFont(byte[] resource, byte[] baseFont, uint firstCode, string[] names, ushort[] widths)
    {
        ArgumentNullException.ThrowIfNull(resource);
        ArgumentNullException.ThrowIfNull(baseFont);
        ArgumentNullException.ThrowIfNull(names);
        ArgumentNullException.ThrowIfNull(widths);
        using var pinned = new PinnedStrings();
        var pointers = pinned.Pin(names);
        Native.Check(Native.tpdf_builder_add_named_font(
            Raw, resource, (nuint)resource.Length, baseFont, (nuint)baseFont.Length, firstCode,
            pointers, (nuint)pointers.Length, widths, (nuint)widths.Length));
    }

    /// <summary>A graphics state under a resource name, from <c>tpdf_ext_gstate_init</c>.</summary>
    public void AddExtGState(byte[] resource, ExtGState state)
    {
        ArgumentNullException.ThrowIfNull(resource);
        ArgumentNullException.ThrowIfNull(state);
        Native.Check(Native.tpdf_ext_gstate_init(out var raw));
        if (state.FillAlpha is { } fill)
        {
            raw.FillAlpha = fill;
        }
        if (state.StrokeAlpha is { } stroke)
        {
            raw.StrokeAlpha = stroke;
        }
        if (state.BlendMode is { } mode)
        {
            raw.HasBlendMode = 1;
            raw.BlendMode = (int)mode;
        }
        raw.SoftMask = (int)state.SoftMask;
        raw.MaskKind = (int)state.MaskKind;
        var form = state.MaskForm is null ? default : GCHandle.Alloc(state.MaskForm, GCHandleType.Pinned);
        var backdrop = state.Backdrop is null ? default : GCHandle.Alloc(state.Backdrop, GCHandleType.Pinned);
        try
        {
            if (state.MaskForm is not null)
            {
                raw.MaskForm = form.AddrOfPinnedObject();
                raw.MaskFormLen = (nuint)state.MaskForm.Length;
            }
            if (state.Backdrop is not null)
            {
                raw.Backdrop = backdrop.AddrOfPinnedObject();
                raw.BackdropLen = (nuint)state.Backdrop.Length;
            }
            Native.Check(Native.tpdf_builder_add_ext_gstate(Raw, resource, (nuint)resource.Length, ref raw));
        }
        finally
        {
            if (form.IsAllocated)
            {
                form.Free();
            }
            if (backdrop.IsAllocated)
            {
                backdrop.Free();
            }
        }
    }

    /// <summary>A form XObject (8.10); a null matrix is the identity and a null group none.</summary>
    public void AddForm(byte[] resource, double x0, double y0, double x1, double y1, double[]? matrix,
        TransparencyGroup? group, byte[] content)
    {
        ArgumentNullException.ThrowIfNull(resource);
        ArgumentNullException.ThrowIfNull(content);
        var pointer = IntPtr.Zero;
        try
        {
            if (group is { } g)
            {
                pointer = Marshal.AllocHGlobal(Marshal.SizeOf<TransparencyGroupRaw>());
                Marshal.StructureToPtr(new TransparencyGroupRaw
                {
                    ColorSpace = (int)g.ColorSpace,
                    Isolated = g.Isolated ? 1 : 0,
                    Knockout = g.Knockout ? 1 : 0,
                }, pointer, false);
            }
            Native.Check(Native.tpdf_builder_add_form(
                Raw, resource, (nuint)resource.Length, x0, y0, x1, y1, matrix, pointer,
                content, (nuint)content.Length));
        }
        finally
        {
            if (pointer != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(pointer);
            }
        }
    }

    /// <summary>A coloured tiling pattern (8.7.3); a null matrix is the identity.</summary>
    public void AddTilingPattern(byte[] resource, double x0, double y0, double x1, double y1, double xStep,
        double yStep, double[]? matrix, TilingType tilingType, byte[] content)
    {
        ArgumentNullException.ThrowIfNull(resource);
        ArgumentNullException.ThrowIfNull(content);
        Native.Check(Native.tpdf_builder_add_tiling_pattern(
            Raw, resource, (nuint)resource.Length, x0, y0, x1, y1, xStep, yStep, matrix, (int)tilingType,
            content, (nuint)content.Length));
    }
}

public sealed partial class PageBuilder
{
    /// <summary>This page's <c>/BleedBox</c> (14.11.2).</summary>
    public void SetBleedBox(double x0, double y0, double x1, double y1) =>
        Native.Check(Native.tpdf_page_builder_set_bleed_box(Raw, x0, y0, x1, y1));

    /// <summary>Codes the caller chose, with a character and a word spacing;
    /// <paramref name="characters"/> are what they stand for.</summary>
    public void EncodedText(byte[] font, double size, double x, double y, double characterSpacing,
        double wordSpacing, byte[] codes, string characters)
    {
        ArgumentNullException.ThrowIfNull(font);
        ArgumentNullException.ThrowIfNull(codes);
        ArgumentNullException.ThrowIfNull(characters);
        Native.Check(Native.tpdf_page_builder_encoded_text(
            Raw, font, (nuint)font.Length, size, x, y, characterSpacing, wordSpacing,
            codes, (nuint)codes.Length, Native.Utf8(characters)));
    }

    /// <summary>Applies a registered graphics state (<c>gs</c>).</summary>
    public void SetExtGState(byte[] resource) =>
        Native.Check(Native.tpdf_page_builder_set_ext_gstate(Raw, resource, (nuint)resource.Length));

    /// <summary>Draws a registered form XObject (<c>Do</c>).</summary>
    public void Form(byte[] resource) =>
        Native.Check(Native.tpdf_page_builder_form(Raw, resource, (nuint)resource.Length));

    /// <summary>Sets the non-stroking colour to a registered tiling pattern.</summary>
    public void SetFillPattern(byte[] resource) =>
        Native.Check(Native.tpdf_page_builder_set_fill_pattern(Raw, resource, (nuint)resource.Length));

    /// <summary>Sets the stroking colour to a registered tiling pattern.</summary>
    public void SetStrokePattern(byte[] resource) =>
        Native.Check(Native.tpdf_page_builder_set_stroke_pattern(Raw, resource, (nuint)resource.Length));
}
