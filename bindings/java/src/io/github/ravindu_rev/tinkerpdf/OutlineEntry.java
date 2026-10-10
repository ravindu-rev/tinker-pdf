package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.ADDRESS;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.util.List;

/**
 * One outline entry under construction (12.3.3). {@link #addChild} and the
 * {@code setOutline} calls consume what they take; the entry still needs its
 * {@link #close}.
 */
public final class OutlineEntry implements AutoCloseable {
    private MemorySegment pointer;

    public OutlineEntry(String title) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment out = Native.slot(arena, ADDRESS);
            Native.check(Native.tpdf_outline_entry_new, Native.cString(arena, title), out);
            pointer = out.get(ADDRESS, 0);
        }
    }

    MemorySegment pointer() {
        return pointer;
    }

    public void setTarget(TinkerPdf.Target target) {
        try (Arena arena = Arena.ofConfined()) {
            Native.check(Native.tpdf_outline_entry_set_target, pointer, target.write(arena));
        }
    }

    /** Whether the entry is shown expanded. */
    public void setOpen(boolean open) {
        Native.check(Native.tpdf_outline_entry_set_open, pointer, Native.flag(open));
    }

    /** Nests an entry under this one, consuming the child. */
    public void addChild(OutlineEntry child) {
        Native.check(Native.tpdf_outline_entry_add_child, pointer, child.pointer);
    }

    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_outline_entry_free, pointer);
            pointer = null;
        }
    }

    /** An array of entry handles for a {@code setOutline} call. */
    static MemorySegment array(Arena arena, List<OutlineEntry> entries) {
        MemorySegment array = Native.slot(arena, Math.max(1, entries.size()) * ADDRESS.byteSize());
        for (int i = 0; i < entries.size(); i++) {
            array.setAtIndex(ADDRESS, i, entries.get(i).pointer);
        }
        return array;
    }
}
