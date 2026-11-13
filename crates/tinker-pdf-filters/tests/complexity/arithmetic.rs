// The restart interval `complexity.rs`'s arithmetic JPEG shape repeats
// (`docs/design/resource-exhaustion.md`, "The families"). It is committed
// because the coder that writes it is `qm.rs`'s test-only QM encoder, which
// no integration test can reach: `jpeg.rs`'s unit test
// `the_complexity_shapes_arithmetic_interval_is_the_qm_encoders_flat_block`
// includes this file and holds these bytes to that encoder's output, so they
// cannot drift from the coder that made them.

/// One MCU of a one-component SOF9 frame, a flat block of mid-grey, as `qm.rs`'s
/// QM encoder codes it: T.81 F.1.4.4.1's decision that the block's DC
/// difference is zero, at `S0` with `Da = 0`, and F.1.4.4.2's end of block at
/// `SE` for `K = 1`, each in a fresh context, as every restart interval
/// begins (F.1.4.4.1.5, F.1.4.4.2.2).
///
/// **The interval is no bytes at all.** Both decisions shrink the interval
/// without moving its base, so `Flush`'s `Clear_final_bits` leaves `C` at
/// zero, both bytes D.1.8 writes are zero, and `Discard_final_zeros` drops
/// them. The decoder reads its two decisions out of the zeros D.2.4 feeds it
/// once it meets the restart marker that follows.
pub const FLAT_INTERVAL: &[u8] = &[];
