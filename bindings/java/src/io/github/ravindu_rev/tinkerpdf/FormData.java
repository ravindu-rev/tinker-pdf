package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.ADDRESS;
import static io.github.ravindu_rev.tinkerpdf.Native.INT;

import io.github.ravindu_rev.tinkerpdf.TinkerPdf.FieldValueKind;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.FormDataWarning;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.FormDataWarningKind;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.ArrayList;
import java.util.List;

/**
 * What an FDF or XFDF file says (12.7.8), or what one will be written from:
 * the engine's own copy, so it outlives the document it came from.
 */
public final class FormData implements AutoCloseable {
    private MemorySegment pointer;

    FormData(MemorySegment pointer) {
        this.pointer = pointer;
    }

    MemorySegment pointer() {
        return pointer;
    }

    /** Empty form data, for {@link #addField} to fill. */
    public static FormData empty() {
        return new FormData(Document.handle(Native.tpdf_form_data_new));
    }

    /** Reads an FDF file: every field of it, or {@code FORM_DATA_REFUSED} and none. */
    public static FormData readFdf(byte[] bytes) {
        try (Arena arena = Arena.ofConfined()) {
            return new FormData(Document.handle(Native.tpdf_form_data_read_fdf, Native.bytes(arena, bytes),
                    (long) bytes.length));
        }
    }

    /** Reads an XFDF file: every field of it, or {@code FORM_DATA_REFUSED} and none. */
    public static FormData readXfdf(byte[] bytes) {
        try (Arena arena = Arena.ofConfined()) {
            return new FormData(Document.handle(Native.tpdf_form_data_read_xfdf, Native.bytes(arena, bytes),
                    (long) bytes.length));
        }
    }

    /** A {@code const char *const *} of NUL-terminated copies, alive as long as the arena. */
    static MemorySegment strings(Arena arena, List<String> values) {
        MemorySegment array = Native.slot(arena, Math.max(8L, 8L * values.size()));
        for (int i = 0; i < values.size(); i++) {
            array.set(ADDRESS, 8L * i, Native.cString(arena, values.get(i)));
        }
        return array;
    }

    /** Appends one field: NONE takes no strings, TEXT and STATE one, MANY any number. */
    public void addField(String name, FieldValueKind kind, String... values) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_form_data_add_field, pointer, Native.cString(arena, name), kind.ordinal(),
                    strings(arena, List.of(values)), (long) values.length);
        }
    }

    /** The document the data belongs to; null when it names none. */
    public String source() {
        return Document.text(Native.tpdf_form_data_source, pointer);
    }

    /** Sets the source; null clears it. */
    public void setSource(String source) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_form_data_set_source, pointer, Native.cStringOrNull(arena, source));
        }
    }

    /** How many fields. */
    public int count() {
        return Native.callInt(Native.tpdf_form_data_count, pointer);
    }

    /** A field's fully qualified name. */
    public String fieldName(int index) {
        return Document.text(Native.tpdf_form_data_field_name, pointer, index);
    }

    /** The shape of a field's value. */
    public FieldValueKind valueKind(int index) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, INT);
            Native.check(Native.tpdf_form_data_field_value_kind, pointer, index, out);
            return FieldValueKind.values()[out.get(INT, 0)];
        }
    }

    /** The strings a field's value is made of. */
    public List<String> values(int index) {
        int count = Native.callInt(Native.tpdf_form_data_field_value_count, pointer, index);
        List<String> values = new ArrayList<>(count);
        for (int i = 0; i < count; i++) {
            values.add(Document.text(Native.tpdf_form_data_field_value, pointer, index, i));
        }
        return values;
    }

    /** Every warning the reader left. */
    public List<FormDataWarning> warnings() {
        int count = Native.callInt(Native.tpdf_form_data_warning_count, pointer);
        List<FormDataWarning> warnings = new ArrayList<>(count);
        try (Arena arena = Arena.ofConfined()) {
            for (int i = 0; i < count; i++) {
                MemorySegment kind = Native.slot(arena, INT);
                MemorySegment what = Native.slot(arena, ADDRESS);
                MemorySegment field = Native.slot(arena, ADDRESS);
                Native.check(Native.tpdf_form_data_warning, pointer, i, kind, what, field);
                warnings.add(new FormDataWarning(FormDataWarningKind.values()[kind.get(INT, 0)],
                        Native.takeString(what), Native.takeString(field)));
            }
        }
        return warnings;
    }

    /** The data written as an FDF file. */
    public byte[] toFdf() {
        return Native.takeBuffer(Document.handle(Native.tpdf_form_data_to_fdf, pointer));
    }

    /** The data written as an XFDF file, UTF-8; {@code FORM_DATA_REFUSED} for a value XML cannot carry. */
    public byte[] toXfdf() {
        return Native.takeBuffer(Document.handle(Native.tpdf_form_data_to_xfdf, pointer));
    }

    /** Releases the data. */
    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_form_data_free, pointer);
            pointer = null;
        }
    }
}
