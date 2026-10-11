// The forms surface over the C ABI: creating fields, and form data -- FDF and
// XFDF (12.7.8) -- read, written and applied.
//
// Transcribed from crates/tinker-pdf-ffi/include/tinker_pdf.h. The engine's
// NewField has four kinds with four payloads, so the C ABI has four calls and
// so does this; FormData is a SafeHandle-owned engine copy on the Signatures
// pattern. No logic of its own (ruling 11).

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace TinkerPdf;

/// <summary>The shape of a field's value (12.7.4).</summary>
public enum FieldValueKind
{
    /// <summary>Named with no value; an import leaves the field alone.</summary>
    None = 0,

    /// <summary>A text string: a text field's value, or a choice field's one selection.</summary>
    Text = 1,

    /// <summary>A name: a check box's or radio group's state.</summary>
    State = 2,

    /// <summary>Several selections of a multiple-choice list.</summary>
    Many = 3,
}

/// <summary>What a form-data reader met and did not read, or read leniently (ruling 10).</summary>
public enum FormDataWarningKind
{
    /// <summary>A key or element this reader does not read, named once per place.</summary>
    NotRead = 0,

    /// <summary>A <c>/V</c> that is neither a string, a name nor an array of them.</summary>
    ValueUnreadable = 1,

    /// <summary>A <c>/Kids</c> entry already walked, or past the depth cap.</summary>
    TreeCut = 2,

    /// <summary>A field whose fully qualified name is empty; not read.</summary>
    Unnamed = 3,
}

/// <summary>One button of a radio group: its export value, page and rectangle.</summary>
public readonly record struct RadioButton(
    string ExportValue,
    uint Page,
    double X0,
    double Y0,
    double X1,
    double Y1);

/// <summary>One warning: its kind, the key or element (null unless NotRead) and the field.</summary>
public readonly record struct FormDataWarning(
    FormDataWarningKind Kind,
    string? What,
    string? Field);

/// <summary><c>TpdfRadioButton</c>, field for field.</summary>
[StructLayout(LayoutKind.Sequential)]
internal struct RadioButtonRaw
{
    internal IntPtr ExportValue;
    internal uint Page;
    internal double X0;
    internal double Y0;
    internal double X1;
    internal double Y1;
}

internal static partial class Native
{
    [DllImport(Library)]
    internal static extern int tpdf_editor_add_text_field(
        IntPtr editor, byte[] name, uint page, double x0, double y0, double x1, double y1,
        byte[]? value, int hasMaxLen, uint maxLen, long flags, double fontSize,
        out uint objectNumber, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_editor_add_checkbox(
        IntPtr editor, byte[] name, uint page, double x0, double y0, double x1, double y1,
        byte[] exportValue, int isChecked, long flags, double fontSize,
        out uint objectNumber, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_editor_add_radio_group(
        IntPtr editor, byte[] name, RadioButtonRaw[] buttons, nuint count, byte[]? selected,
        long flags, double fontSize, out uint objectNumber, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_editor_add_choice_field(
        IntPtr editor, byte[] name, uint page, double x0, double y0, double x1, double y1,
        IntPtr[] options, nuint optionCount, int combo, int editable, byte[]? value,
        long flags, double fontSize, out uint objectNumber, out ushort generation);

    [DllImport(Library)]
    internal static extern int tpdf_document_form_data(IntPtr doc, out IntPtr data);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_read_fdf(byte[] bytes, nuint len, out IntPtr data);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_read_xfdf(byte[] bytes, nuint len, out IntPtr data);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_new(out IntPtr data);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_add_field(
        IntPtr data, byte[] name, int kind, IntPtr[] values, nuint count);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_set_source(IntPtr data, byte[]? source);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_to_fdf(IntPtr data, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_to_xfdf(IntPtr data, out IntPtr buffer);

    [DllImport(Library)]
    internal static extern uint tpdf_form_data_count(IntPtr data);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_field_name(IntPtr data, uint index, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_field_value_kind(IntPtr data, uint index, out int kind);

    [DllImport(Library)]
    internal static extern uint tpdf_form_data_field_value_count(IntPtr data, uint index);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_field_value(
        IntPtr data, uint index, uint value, out IntPtr text);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_source(IntPtr data, out IntPtr text);

    [DllImport(Library)]
    internal static extern uint tpdf_form_data_warning_count(IntPtr data);

    [DllImport(Library)]
    internal static extern int tpdf_form_data_warning(
        IntPtr data, uint index, out int kind, out IntPtr what, out IntPtr field);

    [DllImport(Library)]
    internal static extern void tpdf_form_data_free(IntPtr data);

    [DllImport(Library)]
    internal static extern int tpdf_editor_apply_form_data(
        IntPtr editor, IntPtr data, out IntPtr report);
}

/// <summary>Pinned null-terminated UTF-8 copies, freed together.</summary>
internal sealed class PinnedStrings : IDisposable
{
    private readonly List<GCHandle> _handles = new();

    internal IntPtr[] Pin(IReadOnlyList<string> texts)
    {
        var pointers = new IntPtr[texts.Count];
        for (var i = 0; i < texts.Count; i++)
        {
            var handle = GCHandle.Alloc(Native.Utf8(texts[i]), GCHandleType.Pinned);
            _handles.Add(handle);
            pointers[i] = handle.AddrOfPinnedObject();
        }
        return pointers;
    }

    public void Dispose()
    {
        foreach (var handle in _handles)
        {
            handle.Free();
        }
        _handles.Clear();
    }
}

/// <summary>What an FDF or XFDF file says, or what one will be written from.</summary>
/// <remarks>The engine's own copy, so it outlives the document it came from.</remarks>
public sealed class FormData : IDisposable
{
    private readonly ReadHandle _handle;

    internal FormData(IntPtr raw) => _handle = new ReadHandle(raw, Native.tpdf_form_data_free);

    internal IntPtr Raw => _handle.DangerousGetHandle();

    /// <summary>Empty form data, for <see cref="AddField"/> to fill.</summary>
    public FormData() : this(New()) { }

    private static IntPtr New()
    {
        Native.Check(Native.tpdf_form_data_new(out var raw));
        return raw;
    }

    /// <summary>Reads an FDF file: every field of it, or <see cref="Status.FormDataRefused"/> and none.</summary>
    public static FormData ReadFdf(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        Native.Check(Native.tpdf_form_data_read_fdf(bytes, (nuint)bytes.Length, out var raw));
        return new FormData(raw);
    }

    /// <summary>Reads an XFDF file: every field of it, or <see cref="Status.FormDataRefused"/> and none.</summary>
    public static FormData ReadXfdf(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        Native.Check(Native.tpdf_form_data_read_xfdf(bytes, (nuint)bytes.Length, out var raw));
        return new FormData(raw);
    }

    /// <summary>How many fields.</summary>
    public uint Count => Native.tpdf_form_data_count(Raw);

    /// <summary>A field's fully qualified name.</summary>
    public string FieldName(uint index)
    {
        Native.Check(Native.tpdf_form_data_field_name(Raw, index, out var text));
        return Native.TakeString(text) ?? string.Empty;
    }

    /// <summary>The shape of a field's value.</summary>
    public FieldValueKind ValueKind(uint index)
    {
        Native.Check(Native.tpdf_form_data_field_value_kind(Raw, index, out var kind));
        return (FieldValueKind)kind;
    }

    /// <summary>The strings a field's value is made of: none, one, or each selection.</summary>
    public string[] Values(uint index)
    {
        var count = Native.tpdf_form_data_field_value_count(Raw, index);
        var values = new string[count];
        for (uint i = 0; i < count; i++)
        {
            Native.Check(Native.tpdf_form_data_field_value(Raw, index, i, out var text));
            values[i] = Native.TakeString(text) ?? string.Empty;
        }
        return values;
    }

    /// <summary>Appends one field: None takes no strings, Text and State one, Many any number.</summary>
    public void AddField(string name, FieldValueKind kind, params string[] values)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(values);
        using var pinned = new PinnedStrings();
        var pointers = pinned.Pin(values);
        Native.Check(Native.tpdf_form_data_add_field(
            Raw, Native.Utf8(name), (int)kind, pointers, (nuint)pointers.Length));
    }

    /// <summary>The document the data belongs to; null when it names none.</summary>
    public string? Source
    {
        get
        {
            Native.Check(Native.tpdf_form_data_source(Raw, out var text));
            return Native.TakeString(text);
        }
        set => Native.Check(Native.tpdf_form_data_set_source(
            Raw, value is null ? null : Native.Utf8(value)));
    }

    /// <summary>Every warning the reader left.</summary>
    public FormDataWarning[] Warnings
    {
        get
        {
            var count = Native.tpdf_form_data_warning_count(Raw);
            var warnings = new FormDataWarning[count];
            for (uint i = 0; i < count; i++)
            {
                Native.Check(Native.tpdf_form_data_warning(
                    Raw, i, out var kind, out var what, out var fieldName));
                warnings[i] = new FormDataWarning(
                    (FormDataWarningKind)kind, Native.TakeString(what), Native.TakeString(fieldName));
            }
            return warnings;
        }
    }

    /// <summary>The data as an FDF file (12.7.8).</summary>
    public byte[] ToFdf()
    {
        Native.Check(Native.tpdf_form_data_to_fdf(Raw, out var buffer));
        return Buffers.Take(buffer);
    }

    /// <summary>The data as an XFDF file, UTF-8; <see cref="Status.FormDataRefused"/> for a
    /// value XML 1.0 cannot carry.</summary>
    public byte[] ToXfdf()
    {
        Native.Check(Native.tpdf_form_data_to_xfdf(Raw, out var buffer));
        return Buffers.Take(buffer);
    }

    /// <summary>Releases the data.</summary>
    public void Dispose() => _handle.Dispose();
}

public sealed partial class Editor
{
    private static byte[]? OptionalUtf8(string? text) => text is null ? null : Native.Utf8(text);

    /// <summary>Creates a text field (12.7.4.3) merged with its one widget.</summary>
    /// <remarks><paramref name="flags"/> are the caller's <c>/Ff</c> bits and
    /// <paramref name="fontSize"/> the <c>/DA</c> size, 0 for auto. Throws
    /// <see cref="Status.EditRefused"/> with the engine's reason, creating nothing,
    /// when the field is refused.</remarks>
    public (uint Object, ushort Generation) AddTextField(
        string name, uint page, double x0, double y0, double x1, double y1,
        string? value = null, uint? maxLen = null, long flags = 0, double fontSize = 0)
    {
        ArgumentNullException.ThrowIfNull(name);
        Native.Check(Native.tpdf_editor_add_text_field(
            Raw, Native.Utf8(name), page, x0, y0, x1, y1, OptionalUtf8(value),
            maxLen.HasValue ? 1 : 0, maxLen ?? 0, flags, fontSize,
            out var number, out var generation));
        return (number, generation);
    }

    /// <summary>Creates a check box (12.7.4.2.3) whose on state is <paramref name="exportValue"/>.</summary>
    public (uint Object, ushort Generation) AddCheckbox(
        string name, uint page, double x0, double y0, double x1, double y1,
        string exportValue, bool isChecked, long flags = 0, double fontSize = 0)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(exportValue);
        Native.Check(Native.tpdf_editor_add_checkbox(
            Raw, Native.Utf8(name), page, x0, y0, x1, y1, Native.Utf8(exportValue),
            isChecked ? 1 : 0, flags, fontSize, out var number, out var generation));
        return (number, generation);
    }

    /// <summary>Creates a radio group (12.7.4.2.4): one field, one widget per button.</summary>
    public (uint Object, ushort Generation) AddRadioGroup(
        string name, RadioButton[] buttons, string? selected = null, long flags = 0, double fontSize = 0)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(buttons);
        using var pinned = new PinnedStrings();
        var exports = new string[buttons.Length];
        for (var i = 0; i < buttons.Length; i++)
        {
            exports[i] = buttons[i].ExportValue;
        }
        var pointers = pinned.Pin(exports);
        var raw = new RadioButtonRaw[buttons.Length];
        for (var i = 0; i < buttons.Length; i++)
        {
            raw[i] = new RadioButtonRaw
            {
                ExportValue = pointers[i],
                Page = buttons[i].Page,
                X0 = buttons[i].X0,
                Y0 = buttons[i].Y0,
                X1 = buttons[i].X1,
                Y1 = buttons[i].Y1,
            };
        }
        Native.Check(Native.tpdf_editor_add_radio_group(
            Raw, Native.Utf8(name), raw, (nuint)raw.Length, OptionalUtf8(selected),
            flags, fontSize, out var number, out var generation));
        return (number, generation);
    }

    /// <summary>Creates a choice field (12.7.4.4): a combo box or a list box.</summary>
    public (uint Object, ushort Generation) AddChoiceField(
        string name, uint page, double x0, double y0, double x1, double y1,
        string[] options, bool combo, bool editable = false, string? value = null,
        long flags = 0, double fontSize = 0)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(options);
        using var pinned = new PinnedStrings();
        var pointers = pinned.Pin(options);
        Native.Check(Native.tpdf_editor_add_choice_field(
            Raw, Native.Utf8(name), page, x0, y0, x1, y1, pointers, (nuint)pointers.Length,
            combo ? 1 : 0, editable ? 1 : 0, OptionalUtf8(value), flags, fontSize,
            out var number, out var generation));
        return (number, generation);
    }

    /// <summary>
    /// Imports form data: every field with a value, all of them or none.
    /// </summary>
    /// <remarks>The three outcomes are <see cref="FillField"/>'s: a throw means
    /// nothing was written; an empty array, every widget drawn; a non-empty one,
    /// the widgets that took a value and could not be drawn.</remarks>
    public SkippedWidget[] ApplyFormData(FormData data)
    {
        ArgumentNullException.ThrowIfNull(data);
        Native.Check(Native.tpdf_editor_apply_form_data(Raw, data.Raw, out var report));
        if (report == IntPtr.Zero)
        {
            return Array.Empty<SkippedWidget>();
        }
        try
        {
            var count = Native.tpdf_fill_report_count(report);
            var skipped = new SkippedWidget[count];
            for (uint i = 0; i < count; i++)
            {
                Native.Check(Native.tpdf_fill_report_widget(
                    report, i, out var number, out var generation));
                Native.Check(Native.tpdf_fill_report_defect(report, i, out var defect));
                Native.Check(Native.tpdf_fill_report_message(report, i, out var text));
                skipped[i] = new SkippedWidget(
                    number, generation, (WidgetDefect)defect,
                    Native.TakeString(text) ?? string.Empty);
            }
            return skipped;
        }
        finally
        {
            Native.tpdf_fill_report_free(report);
        }
    }
}

public sealed partial class Document
{
    /// <summary>The data the document's fields hold, in the tree's order.</summary>
    public FormData ReadFormData()
    {
        Native.Check(Native.tpdf_document_form_data(_handle.DangerousGetHandle(), out var raw));
        return new FormData(raw);
    }
}
