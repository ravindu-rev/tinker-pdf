//! A borrowed view of one table, and the four reads every table is made of.
//!
//! Everything above this file is a shape drawn over [`Bytes`], so this is
//! where ruling 1 is discharged for the whole crate: an OpenType Layout table
//! is a graph of 16-bit offsets that a font is free to point anywhere, and the
//! only defence that scales is for the arithmetic itself to be incapable of
//! reading out of bounds.
//!
//! **Nothing is indexed.** Every read goes through `slice::get`, every offset
//! through `checked_add`, and every accessor returns `Option`. A `numGlyphs`
//! of 65 535 in a twelve-byte table produces `None` on the first element past
//! the end rather than a panic in release and a different panic in debug.
//!
//! **Nothing is copied.** A [`Bytes`] is a slice and nothing else, so
//! descending into a subtable — which is what most of this crate does — costs
//! a bounds check rather than an allocation. A font with two thousand lookups
//! is two thousand slices, not two thousand parsed structures, and the ones
//! no feature selects are never looked at.
//!
//! **A subtable is a suffix.** OpenType writes an offset as a distance from
//! the start of the table that holds it and never writes the length, so the
//! only honest bound on a subtable is "to the end of its parent". That is what
//! [`Bytes::at`] returns, and it is why nesting cannot widen a view: a child
//! is always a suffix of its parent, so a bound checked at the top holds all
//! the way down.

/// A table, or a subtable of one, with every read bounds-checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bytes<'a> {
    data: &'a [u8],
}

impl<'a> Bytes<'a> {
    /// A view of `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// The bytes themselves.
    #[must_use]
    pub const fn as_slice(&self) -> &'a [u8] {
        self.data
    }

    /// How many bytes the view holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the view holds nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The byte at `at`.
    #[must_use]
    pub fn u8(&self, at: usize) -> Option<u8> {
        self.data.get(at).copied()
    }

    /// The big-endian `uint16` at `at`.
    #[must_use]
    pub fn u16(&self, at: usize) -> Option<u16> {
        let end = at.checked_add(2)?;
        let pair = self.data.get(at..end)?;
        Some(u16::from_be_bytes([*pair.first()?, *pair.get(1)?]))
    }

    /// The big-endian `int16` at `at`.
    ///
    /// This is the FWORD every position in this crate is made of. It is read
    /// as `i16` and widened to `i32` by the caller rather than being kept
    /// narrow, because a sum of design units overflows 16 bits long before it
    /// overflows a line of text (ruling 4: the arithmetic is integer, and
    /// integer arithmetic that wraps is not deterministic in any useful
    /// sense).
    #[must_use]
    pub fn i16(&self, at: usize) -> Option<i16> {
        #[allow(clippy::cast_possible_wrap)]
        self.u16(at).map(|value| value as i16)
    }

    /// The big-endian `uint32` at `at`.
    #[must_use]
    pub fn u32(&self, at: usize) -> Option<u32> {
        let end = at.checked_add(4)?;
        let quad = self.data.get(at..end)?;
        Some(u32::from_be_bytes([
            *quad.first()?,
            *quad.get(1)?,
            *quad.get(2)?,
            *quad.get(3)?,
        ]))
    }

    /// Element `index` of a `uint16` array that begins at `at`.
    ///
    /// The multiply is checked, which is the point: `index` comes from a
    /// count the font wrote, and `at + index * 2` is the one place in a table
    /// walker where a 32-bit machine can be made to wrap.
    #[must_use]
    pub fn u16_at(&self, at: usize, index: usize) -> Option<u16> {
        self.u16(at.checked_add(index.checked_mul(2)?)?)
    }

    /// The view starting `offset` bytes in, to the end of this one.
    ///
    /// `None` past the end. A subtable is always a suffix of its parent; see
    /// the module documentation for why that is the only bound available.
    #[must_use]
    pub fn at(&self, offset: usize) -> Option<Bytes<'a>> {
        self.data.get(offset..).map(Bytes::new)
    }

    /// The subtable named by the `Offset16` stored at `at`.
    ///
    /// A zero offset means "absent" everywhere in OpenType Layout, so it is
    /// `None` here rather than a view of the parent from its own start —
    /// which is what a reader that took offset zero literally would hand back,
    /// and it would parse as whatever the parent's first field happens to be.
    #[must_use]
    pub fn offset16(&self, at: usize) -> Option<Bytes<'a>> {
        match self.u16(at)? {
            0 => None,
            offset => self.at(usize::from(offset)),
        }
    }

    /// The subtable named by the `Offset32` stored at `at`, zero meaning
    /// absent.
    #[must_use]
    pub fn offset32(&self, at: usize) -> Option<Bytes<'a>> {
        match self.u32(at)? {
            0 => None,
            offset => self.at(usize::try_from(offset).ok()?),
        }
    }

    /// The subtable named by element `index` of an `Offset16` array at `at`.
    #[must_use]
    pub fn offset16_at(&self, at: usize, index: usize) -> Option<Bytes<'a>> {
        self.offset16(at.checked_add(index.checked_mul(2)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::Bytes;

    #[test]
    fn reads_stop_at_the_end() {
        let bytes = Bytes::new(&[0x00, 0x01, 0x02]);
        assert_eq!(bytes.u16(0), Some(1));
        assert_eq!(bytes.u16(1), Some(0x0102));
        assert_eq!(bytes.u16(2), None);
        assert_eq!(bytes.u32(0), None);
        assert_eq!(bytes.u8(3), None);
    }

    #[test]
    fn an_array_index_cannot_wrap() {
        let bytes = Bytes::new(&[0x00; 8]);
        assert_eq!(bytes.u16_at(0, 3), Some(0));
        assert_eq!(bytes.u16_at(0, 4), None);
        assert_eq!(bytes.u16_at(0, usize::MAX), None);
        assert_eq!(bytes.u16_at(usize::MAX, 1), None);
    }

    #[test]
    fn a_zero_offset_is_absent_rather_than_the_parent() {
        // Two offsets: the first zero, the second pointing at the byte pair
        // that closes the table.
        let bytes = Bytes::new(&[0x00, 0x00, 0x00, 0x04, 0xAB, 0xCD]);
        assert_eq!(bytes.offset16(0), None);
        assert_eq!(bytes.offset16(2).and_then(|b| b.u16(0)), Some(0xABCD));
        assert_eq!(bytes.offset16_at(0, 1).and_then(|b| b.u16(0)), Some(0xABCD));
    }

    #[test]
    fn a_subtable_is_a_suffix_of_its_parent() {
        let bytes = Bytes::new(&[1, 2, 3, 4]);
        let inner = bytes.at(2).expect("in range");
        assert_eq!(inner.len(), 2);
        assert_eq!(inner.at(3), None);
        assert_eq!(bytes.at(4).map(|b| b.is_empty()), Some(true));
        assert_eq!(bytes.at(5), None);
    }
}
