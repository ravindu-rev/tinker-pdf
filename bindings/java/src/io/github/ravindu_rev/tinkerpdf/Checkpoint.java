package io.github.ravindu_rev.tinkerpdf;

import java.lang.foreign.MemorySegment;

/** An editor's state, borrowed by {@link Editor#restore} as often as it is needed. */
public final class Checkpoint implements AutoCloseable {
    private MemorySegment pointer;

    Checkpoint(MemorySegment pointer) {
        this.pointer = pointer;
    }

    MemorySegment pointer() {
        return pointer;
    }

    /** Releases the checkpoint; nothing was pending, so nothing commits. */
    @Override
    public void close() {
        if (pointer != null) {
            Native.call(Native.tpdf_checkpoint_free, pointer);
            pointer = null;
        }
    }
}
