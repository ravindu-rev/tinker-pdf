package io.github.ravindu_rev.tinkerpdf;

import static io.github.ravindu_rev.tinkerpdf.Native.ADDRESS;
import static io.github.ravindu_rev.tinkerpdf.Native.BYTE;
import static io.github.ravindu_rev.tinkerpdf.Native.LONG;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;

/**
 * A document's bytes on demand: {@code TpdfSourceVtable}, for
 * {@link Document#openStreaming}.
 *
 * <p>The engine may call it from whatever thread is working, and from several
 * at once, so an implementation must be safe for concurrent use.
 */
public interface Source {
    /** The document's total length, fixed for the life of the source. */
    long length();

    /**
     * Writes up to {@code into.length} bytes from {@code offset} and returns
     * how many it wrote, or a negative number to say the range is not here:
     * the call that needed it fails with {@code SOURCE_MISS}, naming the range,
     * and the caller fetches it and asks again.
     */
    int readAt(long offset, byte[] into);

    /**
     * The vtable, its three upcall stubs allocated in {@code stubs}, which must
     * outlive the document. An exception escaping a callback would end the
     * process, so a {@code readAt} that throws reads as a miss and a
     * {@code length} that throws as zero.
     */
    static MemorySegment vtable(Source source, Arena stubs, Arena arena) {
        try {
            MethodHandles.Lookup lookup = MethodHandles.lookup();
            Bridge bridge = new Bridge(source);
            MemorySegment length = Native.LINKER.upcallStub(
                    lookup.findVirtual(Bridge.class, "length", MethodType.methodType(long.class, MemorySegment.class))
                            .bindTo(bridge),
                    FunctionDescriptor.of(LONG, ADDRESS), stubs);
            MemorySegment read = Native.LINKER.upcallStub(
                    lookup.findVirtual(Bridge.class, "read", MethodType.methodType(long.class, MemorySegment.class,
                            long.class, MemorySegment.class, long.class)).bindTo(bridge),
                    FunctionDescriptor.of(LONG, ADDRESS, LONG, ADDRESS, LONG), stubs);
            MemorySegment vtable = arena.allocate(Native.VTABLE);
            vtable.set(ADDRESS, 0, length);
            vtable.set(ADDRESS, 8, read);
            vtable.set(ADDRESS, 16, MemorySegment.NULL);
            return vtable;
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    /** The callbacks' receiver. {@code free} is null: the stubs' arena is the release. */
    final class Bridge {
        private final Source source;

        Bridge(Source source) {
            this.source = source;
        }

        long length(MemorySegment context) {
            try {
                return source.length();
            } catch (RuntimeException e) {
                return 0;
            }
        }

        long read(MemorySegment context, long offset, MemorySegment out, long capacity) {
            try {
                byte[] into = new byte[(int) Math.min(capacity, Integer.MAX_VALUE)];
                int wrote = source.readAt(offset, into);
                if (wrote > 0) {
                    MemorySegment.copy(into, 0, out.reinterpret(capacity), BYTE, 0, wrote);
                }
                return wrote;
            } catch (RuntimeException e) {
                return -1;
            }
        }
    }
}
