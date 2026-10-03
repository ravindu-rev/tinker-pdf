package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.ADDRESS;
import static io.github.ravindu_rev.tinkerpdf.Native.DOUBLE;
import static io.github.ravindu_rev.tinkerpdf.Native.INT;
import static io.github.ravindu_rev.tinkerpdf.Native.LONG;
import static io.github.ravindu_rev.tinkerpdf.Native.SHORT;

import io.github.ravindu_rev.tinkerpdf.TinkerPdf.ChangedField;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.DeletedObject;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.EmbeddedFile;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.InfoKey;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.MetadataSync;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.PageBoundary;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.PageLabelRange;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.PathStep;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.RadioButton;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Recalculation;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Ref;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Removal;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.RemovedEntry;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Sanitise;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.SanitiseReport;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.ScriptAnswer;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.SkippedWidget;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.Trapped;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.WidgetDefect;
import io.github.ravindu_rev.tinkerpdf.TinkerPdf.WriteOptions;
import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandle;
import java.util.ArrayList;
import java.util.List;

/** An editor over a document. It holds its own reference to the object store, so the document may be closed first. */
public final class Editor implements AutoCloseable {
    private MemorySegment pointer;

    Editor(MemorySegment pointer) {
        this.pointer = pointer;
    }

    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_editor_free, pointer);
            pointer = null;
        }
    }

    /**
     * Sets a text or choice field. An exception means nothing was written; a
     * non-empty list means the value was written and those widgets were left
     * showing what they showed before.
     */
    public List<SkippedWidget> fillField(String name, String value) {
        MemorySegment report;
        try (Arena arena = Arena.ofConfined()) {
            report = Document.handle(Native.tpdf_editor_fill_field, pointer, Native.cString(arena, name),
                    Native.cString(arena, value));
        }
        return skipped(report);
    }

    /** A fill report's widgets, copied; the report is freed. */
    static List<SkippedWidget> skipped(MemorySegment report) {
        try (Arena arena = Arena.ofConfined()) {
            int count = Native.callInt(Native.tpdf_fill_report_count, report);
            List<SkippedWidget> skipped = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                MemorySegment number = Native.slot(arena, INT);
                MemorySegment generation = Native.slot(arena, SHORT);
                Native.check(Native.tpdf_fill_report_widget, report, i, number, generation);
                MemorySegment defect = Native.slot(arena, INT);
                Native.check(Native.tpdf_fill_report_defect, report, i, defect);
                skipped.add(new SkippedWidget(number.get(INT, 0), Short.toUnsignedInt(generation.get(SHORT, 0)),
                        WidgetDefect.values()[defect.get(INT, 0)],
                        Document.text(Native.tpdf_fill_report_message, report, i)));
            }
            return skipped;
        } finally {
            Native.call(Native.tpdf_fill_report_free, report);
        }
    }

    /**
     * Creates a text field (12.7.4.3) merged with its one widget; returns its
     * reference. A null value or maxLen is none; flags are the caller's /Ff
     * bits and fontSize the /DA size, 0 for auto. Refused, creating nothing,
     * as {@code EDIT_REFUSED} with the engine's reason.
     */
    public Ref addTextField(String name, int page, double x0, double y0, double x1, double y1, String value,
            Integer maxLen, long flags, double fontSize) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment object = Native.slot(arena, INT);
            MemorySegment generation = Native.slot(arena, SHORT);
            Native.check(Native.tpdf_editor_add_text_field, pointer, Native.cString(arena, name), page, x0, y0, x1,
                    y1, Native.cStringOrNull(arena, value), Native.flag(maxLen != null),
                    maxLen == null ? 0 : maxLen.intValue(), flags, fontSize, object, generation);
            return Document.ref(object, generation);
        }
    }

    /** Creates a check box (12.7.4.2.3) whose on state is exportValue. */
    public Ref addCheckbox(String name, int page, double x0, double y0, double x1, double y1, String exportValue,
            boolean checked, long flags, double fontSize) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment object = Native.slot(arena, INT);
            MemorySegment generation = Native.slot(arena, SHORT);
            Native.check(Native.tpdf_editor_add_checkbox, pointer, Native.cString(arena, name), page, x0, y0, x1, y1,
                    Native.cString(arena, exportValue), Native.flag(checked), flags, fontSize, object, generation);
            return Document.ref(object, generation);
        }
    }

    /**
     * Creates a radio group (12.7.4.2.4): one field, one widget per button. A
     * null selected is none. {@code TpdfRadioButton}: export_value pointer @0,
     * page u32 @8, x0..y1 f64 @16..@40; 48 bytes.
     */
    public Ref addRadioGroup(String name, List<RadioButton> buttons, String selected, long flags, double fontSize) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment raw = Native.slot(arena, Math.max(48L, 48L * buttons.size()));
            for (int i = 0; i < buttons.size(); i++) {
                RadioButton button = buttons.get(i);
                long at = 48L * i;
                raw.set(ADDRESS, at, Native.cString(arena, button.exportValue()));
                raw.set(INT, at + 8, button.page());
                raw.set(DOUBLE, at + 16, button.x0());
                raw.set(DOUBLE, at + 24, button.y0());
                raw.set(DOUBLE, at + 32, button.x1());
                raw.set(DOUBLE, at + 40, button.y1());
            }
            MemorySegment object = Native.slot(arena, INT);
            MemorySegment generation = Native.slot(arena, SHORT);
            Native.check(Native.tpdf_editor_add_radio_group, pointer, Native.cString(arena, name), raw,
                    (long) buttons.size(), Native.cStringOrNull(arena, selected), flags, fontSize, object,
                    generation);
            return Document.ref(object, generation);
        }
    }

    /** Creates a choice field (12.7.4.4): a combo box when combo, a list box otherwise. */
    public Ref addChoiceField(String name, int page, double x0, double y0, double x1, double y1,
            List<String> options, boolean combo, boolean editable, String value, long flags, double fontSize) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment object = Native.slot(arena, INT);
            MemorySegment generation = Native.slot(arena, SHORT);
            Native.check(Native.tpdf_editor_add_choice_field, pointer, Native.cString(arena, name), page, x0, y0, x1,
                    y1, FormData.strings(arena, options), (long) options.size(), Native.flag(combo),
                    Native.flag(editable), Native.cStringOrNull(arena, value), flags, fontSize, object, generation);
            return Document.ref(object, generation);
        }
    }

    /** Imports form data, every field or none; the outcomes are {@link #fillField}'s. */
    public List<SkippedWidget> applyFormData(FormData data) {
        return skipped(Document.handle(Native.tpdf_editor_apply_form_data, pointer, data.pointer()));
    }

    public void setCheckbox(String name, boolean on) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_editor_set_checkbox, pointer, Native.cString(arena, name), Native.flag(on));
        }
    }

    /** Selects one option of a radio group (12.7.4.2). */
    public void selectRadio(String name, String option) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_editor_select_radio, pointer, Native.cString(arena, name),
                    Native.cString(arena, option));
        }
    }

    /** The edited document's bytes, under options that start from {@link TinkerPdf#writeOptions()}. */
    public byte[] save(WriteOptions options) {
        try (Arena arena = Arena.ofConfined()) {
            return Native.takeBuffer(Document.handle(Native.tpdf_editor_save, pointer, options.write(arena)));
        }
    }

    public boolean isDirty() {
        return Native.callInt(Native.tpdf_editor_is_dirty, pointer) != 0;
    }

    /** How many pages the document has as this editor sees it. */
    public int pageCount() {
        return Native.callInt(Native.tpdf_editor_page_count, pointer);
    }

    public void deletePage(int index) {
        Native.check(Native.tpdf_editor_delete_page, pointer, index);
    }

    public void movePage(int from, int to) {
        Native.check(Native.tpdf_editor_move_page, pointer, from, to);
    }

    /** Turns a page by a quarter-turn multiple; any other turn is EDIT_REFUSED. */
    public void rotatePage(int index, long degrees) {
        Native.check(Native.tpdf_editor_rotate_page, pointer, index, degrees);
    }

    /** A blank page at index, which may equal the page count. */
    public void insertPage(int index, double width, double height) {
        Native.check(Native.tpdf_editor_insert_page, pointer, index, width, height);
    }

    public void setCropBox(int index, double x0, double y0, double x1, double y1) {
        Native.check(Native.tpdf_editor_set_crop_box, pointer, index, x0, y0, x1, y1);
    }

    public void appendContent(int page, byte[] operators) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_editor_append_content, pointer, page, Native.bytes(arena, operators),
                    (long) operators.length);
        }
    }

    public int fieldCount() {
        return Native.callInt(Native.tpdf_editor_field_count, pointer);
    }

    /** A field's fully qualified name (12.7.3.2). */
    public String fieldName(int index) {
        return Document.text(Native.tpdf_editor_field_name, pointer, index);
    }

    /** A field's value as text; empty when it has none. */
    public String fieldValue(int index) {
        return Document.text(Native.tpdf_editor_field_value, pointer, index);
    }

    /** The editor's state as a value, for {@link #restore} to put back. */
    public Checkpoint checkpoint() {
        return new Checkpoint(Document.handle(Native.tpdf_editor_checkpoint, pointer));
    }

    /** Puts the editor back to what a checkpoint recorded. Idempotent. */
    public void restore(Checkpoint checkpoint) {
        Native.check(Native.tpdf_editor_restore, pointer, checkpoint.pointer());
    }

    private static List<String> names(MethodHandle count, MethodHandle name, MemorySegment report) {
        int total = Native.callInt(count, report);
        List<String> found = new ArrayList<>(total);
        for (int i = 0; i < total; i++) {
            found.add(Document.text(name, report, i));
        }
        return found;
    }

    /** Runs the form's calculate actions under a policy, all or nothing. */
    public Recalculation recalculate(int policy) {
        MemorySegment report = Document.handle(Native.tpdf_editor_recalculate, pointer, policy);
        try {
            int count = Native.callInt(Native.tpdf_recalculation_changed_count, report);
            List<ChangedField> changed = new ArrayList<>(count);
            for (int i = 0; i < count; i++) {
                changed.add(new ChangedField(Document.text(Native.tpdf_recalculation_changed_name, report, i),
                        Document.text(Native.tpdf_recalculation_changed_value, report, i)));
            }
            return new Recalculation(changed, Native.callInt(Native.tpdf_recalculation_skipped_count, report),
                    names(Native.tpdf_recalculation_cascades_cut_count, Native.tpdf_recalculation_cascades_cut_name,
                            report),
                    names(Native.tpdf_recalculation_refused_count, Native.tpdf_recalculation_refused_name, report));
        } finally {
            Native.call(Native.tpdf_recalculation_free, report);
        }
    }

    /** What the field's format action displays; null when it carries none. */
    public String formattedValue(String name, int policy) {
        try (Arena arena = Arena.ofConfined()) {
            return Document.text(Native.tpdf_editor_formatted_value, pointer, Native.cString(arena, name), policy);
        }
    }

    private ScriptAnswer answer(MethodHandle handle, Object... arguments) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment accepted = Native.slot(arena, INT);
            MemorySegment out = Native.slot(arena, ADDRESS);
            Object[] all = java.util.Arrays.copyOf(arguments, arguments.length + 2);
            all[arguments.length] = accepted;
            all[arguments.length + 1] = out;
            Native.check(handle, all);
            return new ScriptAnswer(accepted.get(INT, 0) != 0, Native.takeString(out));
        }
    }

    /** Offers a keystroke to a field's keystroke action. Nothing is written. */
    public ScriptAnswer keystroke(String name, String change, long selStart, long selEnd, boolean willCommit,
            int policy) {
        try (Arena arena = Arena.ofConfined()) {
            return answer(Native.tpdf_editor_keystroke, pointer, Native.cString(arena, name),
                    Native.cString(arena, change), selStart, selEnd, Native.flag(willCommit), policy);
        }
    }

    /** Offers a committed value to a field's validate action. Nothing is written. */
    public ScriptAnswer validate(String name, String value, int policy) {
        try (Arena arena = Arena.ofConfined()) {
            return answer(Native.tpdf_editor_validate, pointer, Native.cString(arena, name),
                    Native.cString(arena, value), policy);
        }
    }

    /** {@code TpdfPageLabelRange}: first_page @0, style @4, prefix @8, start @16; 24 bytes. */
    public void setPageLabels(List<PageLabelRange> ranges) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment raw = Native.slot(arena, Math.max(1, ranges.size()) * 24L);
            for (int i = 0; i < ranges.size(); i++) {
                PageLabelRange range = ranges.get(i);
                long at = i * 24L;
                raw.set(INT, at, range.firstPage());
                raw.set(INT, at + 4, range.style().ordinal());
                raw.set(ADDRESS, at + 8, Native.cStringOrNull(arena, range.prefix()));
                raw.set(INT, at + 16, range.start());
            }
            Native.check(Native.tpdf_editor_set_page_labels, pointer, raw, (long) ranges.size());
        }
    }

    /**
     * Attaches a file; returns its file specification's reference.
     * {@code TpdfEmbeddedFile}: name, filename, description, mime_type, created,
     * modified, data, data_len, eight words; 64 bytes.
     */
    public Ref attachFile(EmbeddedFile file) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment raw = Native.slot(arena, 64);
            raw.set(ADDRESS, 0, Native.cString(arena, file.name()));
            raw.set(ADDRESS, 8, Native.cString(arena, file.filename()));
            raw.set(ADDRESS, 16, Native.cStringOrNull(arena, file.description()));
            raw.set(ADDRESS, 24, Native.cStringOrNull(arena, file.mimeType()));
            raw.set(ADDRESS, 32, file.created() == null ? MemorySegment.NULL : file.created().write(arena));
            raw.set(ADDRESS, 40, file.modified() == null ? MemorySegment.NULL : file.modified().write(arena));
            raw.set(ADDRESS, 48, file.data().length == 0 ? MemorySegment.NULL : Native.bytes(arena, file.data()));
            raw.set(LONG, 56, file.data().length);
            MemorySegment object = Native.slot(arena, INT);
            MemorySegment generation = Native.slot(arena, SHORT);
            Native.check(Native.tpdf_editor_attach_file, pointer, raw, object, generation);
            return Document.ref(object, generation);
        }
    }

    /** The outline, from top-level entries, consuming each. */
    public void setOutline(List<OutlineEntry> entries) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_editor_set_outline, pointer, OutlineEntry.array(arena, entries),
                    (long) entries.size());
        }
    }

    private MetadataSync sync(MethodHandle handle, Object... arguments) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, INT);
            Object[] all = java.util.Arrays.copyOf(arguments, arguments.length + 1);
            all[arguments.length] = out;
            Native.check(handle, all);
            return MetadataSync.values()[out.get(INT, 0)];
        }
    }

    public MetadataSync setInfo(InfoKey key, String value) {
        try (Arena arena = Arena.ofConfined()) {
            return sync(Native.tpdf_editor_set_info, pointer, key.ordinal(), Native.cString(arena, value));
        }
    }

    public MetadataSync setInfoDate(InfoKey key, TinkerPdf.Date date) {
        try (Arena arena = Arena.ofConfined()) {
            return sync(Native.tpdf_editor_set_info_date, pointer, key.ordinal(), date.write(arena));
        }
    }

    public MetadataSync setTrapped(Trapped trapped) {
        return sync(Native.tpdf_editor_set_trapped, pointer, trapped.ordinal());
    }

    public MetadataSync setXmpMetadata(byte[] packet) {
        try (Arena arena = Arena.ofConfined()) {
            return sync(Native.tpdf_editor_set_xmp_metadata, pointer, Native.bytes(arena, packet),
                    (long) packet.length);
        }
    }

    public void setPageBoundary(int index, PageBoundary boundary, double x0, double y0, double x1, double y1) {
        Native.check(Native.tpdf_editor_set_page_boundary, pointer, index, boundary.ordinal(), x0, y0, x1, y1);
    }

    /** Removes what {@code what} names; {@code TpdfSanitise} is four ints in the record's order. */
    public SanitiseReport sanitise(Sanitise what) {
        MemorySegment report;
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment raw = Native.slot(arena, 16);
            raw.set(INT, 0, Native.flag(what.javascript()));
            raw.set(INT, 4, Native.flag(what.actions()));
            raw.set(INT, 8, Native.flag(what.embeddedFiles()));
            raw.set(INT, 12, Native.flag(what.metadata()));
            report = Document.handle(Native.tpdf_editor_sanitise, pointer, raw);
        }
        try (Arena arena = Arena.ofConfined()) {
            List<RemovedEntry> removed = new ArrayList<>();
            List<DeletedObject> deleted = new ArrayList<>();
            for (int list = 0; list < 2; list++) {
                int count = Native.callInt(Native.tpdf_sanitise_report_count, report, list);
                for (int i = 0; i < count; i++) {
                    MemorySegment whatSlot = Native.slot(arena, INT);
                    MemorySegment has = Native.slot(arena, INT);
                    MemorySegment object = Native.slot(arena, INT);
                    MemorySegment generation = Native.slot(arena, SHORT);
                    Native.check(Native.tpdf_sanitise_report_entry, report, list, i, whatSlot, has, object,
                            generation);
                    MemorySegment data = Native.slot(arena, ADDRESS);
                    MemorySegment length = Native.slot(arena, LONG);
                    Native.check(Native.tpdf_sanitise_report_action, report, list, i, data, length);
                    Removal kind = Removal.values()[whatSlot.get(INT, 0)];
                    byte[] action = Native.borrowed(data, length);
                    Ref ref = Document.ref(object, generation);
                    if (list == 1) {
                        deleted.add(new DeletedObject(ref, kind, action));
                        continue;
                    }
                    int steps = Native.callInt(Native.tpdf_sanitise_report_path_count, report, i);
                    List<PathStep> path = new ArrayList<>(steps);
                    for (int s = 0; s < steps; s++) {
                        MemorySegment isIndex = Native.slot(arena, INT);
                        MemorySegment position = Native.slot(arena, LONG);
                        MemorySegment key = Native.slot(arena, ADDRESS);
                        MemorySegment keyLength = Native.slot(arena, LONG);
                        Native.check(Native.tpdf_sanitise_report_path_step, report, i, s, isIndex, position, key,
                                keyLength);
                        path.add(isIndex.get(INT, 0) != 0 ? new PathStep(null, position.get(LONG, 0))
                                : new PathStep(Native.borrowed(key, keyLength), null));
                    }
                    removed.add(new RemovedEntry(has.get(INT, 0) == 0 ? null : ref, path, kind, action));
                }
            }
            return new SanitiseReport(removed, deleted);
        } finally {
            Native.call(Native.tpdf_sanitise_report_free, report);
        }
    }
}
