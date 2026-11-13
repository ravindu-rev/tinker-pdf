//! RFC 1952, the gzip file format: a member's header, its deflate data and its
//! trailer, read over [`inflate_raw`] and [`Crc32`].
//!
//! A member (2.3) is a ten-byte header — ID1 and ID2, CM, FLG, MTIME, XFL and
//! OS — then the optional fields FLG declares, in the order 2.3 lists them
//! (the extra field, the original file name, the comment, the header's
//! CRC16), then the compressed blocks, then CRC32 and ISIZE. Every number of
//! more than one byte is stored least significant byte first (2.1). A gzip
//! file is members one after another, with nothing between or after them
//! (2.2).
//!
//! 2.3.1.2 is what a decompressor owes, and this reader does all of it: it
//! checks ID1, ID2 and CM, reads each optional field FLG declares far enough
//! to skip it, and refuses a member with a reserved FLG bit set, since such a
//! bit "could indicate the presence of a new field". It checks what the
//! clause leaves optional too — the CRC16 when FHCRC is set, CRC32 and ISIZE
//! always — because a member whose checks disagree with its data is a damaged
//! member, and every failure is refused by name ([`GzipError`]) rather than
//! repaired: the caller this exists for, the `SVG ` table's compressed
//! documents (`docs/design/colour-fonts.md`), draws a document whole or not at
//! all.
//!
//! Bounded on any input (ruling 1): the header's lengths are held to the
//! input, a name or a comment ends at the input's end if its zero does not
//! come first, and the data inflates under the caller's [`Limits`], past which
//! the member is refused rather than cut.

use crate::{inflate_raw, Crc32, Limits, Warning};

/// ID1 and ID2 (2.3.1): `\037\213`.
const MAGIC: [u8; 2] = [0x1f, 0x8b];

/// CM 8, deflate (2.3.1). 0 to 7 are reserved.
const DEFLATE: u8 = 8;

/// FLG's five defined bits (2.3.1).
const FTEXT: u8 = 0x01;
const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;

/// Bits 5 to 7 of FLG, which 2.3.1 reserves and 2.3.1.2 makes an error.
const RESERVED: u8 = 0xe0;

/// The fixed part of a header: ID1, ID2, CM, FLG, four bytes of MTIME, XFL
/// and OS.
const FIXED_HEADER: usize = 10;

/// CRC32 and ISIZE.
const TRAILER: usize = 8;

/// A member's header (2.3.1), its fields as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GzipHeader<'a> {
    /// FTEXT: the compressor's guess that the data is text, which 2.3.1.2 lets
    /// a decompressor ignore, and this one does.
    pub text: bool,
    /// MTIME: the original file's modification time in seconds since 1970
    /// UTC, or when compression started; 0 for none.
    pub mtime: u32,
    /// XFL: 2 for the slowest deflate, 4 for the fastest.
    pub extra_flags: u8,
    /// OS: the file system compression ran on, 255 for unknown.
    pub os: u8,
    /// FEXTRA's XLEN bytes: the subfields 2.3.1.1 lays out, as written.
    pub extra: Option<&'a [u8]>,
    /// FNAME: the original file name, without its terminating zero, in
    /// ISO 8859-1 as 2.3.1 has it.
    pub name: Option<&'a [u8]>,
    /// FCOMMENT: the comment, without its terminating zero.
    pub comment: Option<&'a [u8]>,
    /// FHCRC's CRC16, which matched the header before it.
    pub header_crc: Option<u16>,
}

/// One member, read and checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GzipMember<'a> {
    /// The header.
    pub header: GzipHeader<'a>,
    /// The inflated data, which CRC32 and ISIZE agree with.
    pub data: Vec<u8>,
    /// CRC32, the CRC-32 of [`GzipMember::data`].
    pub crc: u32,
    /// The input bytes the member took, its header to its trailer, so
    /// `input[end..]` is what follows it — the next member, in a file of
    /// several.
    pub end: usize,
}

/// Why a member, or a file of them, is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GzipError {
    /// Fewer than the ten bytes of a header, or ID1 and ID2 not 31 and 139.
    NotGzip,
    /// A compression method other than 8, deflate: 2.3.1 reserves 0 to 7 and
    /// defines nothing past 8.
    Method(u8),
    /// FLG with a reserved bit (5, 6 or 7) set, the flags as written: 2.3.1.2
    /// makes it an error, since the bit may announce a field this reader
    /// would read as data.
    ReservedFlags(u8),
    /// The input ends inside the header: before XLEN's bytes, before a name's
    /// or a comment's terminating zero, or before the CRC16.
    TruncatedHeader,
    /// FHCRC is set and the CRC16 is not the two least significant bytes of
    /// the CRC-32 of the header before it.
    HeaderCrc {
        /// The CRC16 the member carries.
        stated: u16,
        /// The CRC16 of the header as read.
        computed: u16,
    },
    /// The compressed blocks are not RFC 1951 deflate: a reserved block type,
    /// an invalid code, a distance before the data.
    CorruptData,
    /// The input ends inside the compressed blocks.
    TruncatedData,
    /// The input ends inside the eight-byte trailer.
    TruncatedTrailer,
    /// CRC32 is not the CRC-32 of the inflated data.
    Crc {
        /// The CRC32 the member carries.
        stated: u32,
        /// The CRC-32 of what inflated.
        computed: u32,
    },
    /// ISIZE is not the inflated data's length modulo 2^32.
    Size {
        /// The ISIZE the member carries.
        stated: u32,
        /// What inflated, modulo 2^32.
        computed: u32,
    },
    /// The data inflates past the caller's [`Limits::max_output`]: refused,
    /// not cut, for a document cut short is a different document.
    ExceedsOutputLimit {
        /// The caller's ceiling.
        limit: usize,
    },
    /// Bytes after a member that begin no member: 2.2 puts nothing between
    /// or after the members of a file.
    TrailingBytes {
        /// Where they begin.
        at: usize,
    },
}

impl core::fmt::Display for GzipError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotGzip => f.write_str("not a gzip member: no 1F 8B header"),
            Self::Method(method) => write!(f, "gzip compression method {method}"),
            Self::ReservedFlags(flags) => write!(f, "gzip FLG {flags:#04x} sets a reserved bit"),
            Self::TruncatedHeader => f.write_str("the gzip header is cut off"),
            Self::HeaderCrc { stated, computed } => {
                write!(
                    f,
                    "gzip header CRC16 {stated:#06x}, computed {computed:#06x}"
                )
            }
            Self::CorruptData => f.write_str("the gzip member's deflate data is corrupt"),
            Self::TruncatedData => f.write_str("the gzip member's deflate data is cut off"),
            Self::TruncatedTrailer => f.write_str("the gzip trailer is cut off"),
            Self::Crc { stated, computed } => {
                write!(f, "gzip CRC32 {stated:#010x}, computed {computed:#010x}")
            }
            Self::Size { stated, computed } => {
                write!(f, "gzip ISIZE {stated}, inflated {computed}")
            }
            Self::ExceedsOutputLimit { limit } => {
                write!(
                    f,
                    "the gzip data inflates past the caller's ceiling of {limit}"
                )
            }
            Self::TrailingBytes { at } => write!(f, "bytes at {at} after a gzip member"),
        }
    }
}

impl std::error::Error for GzipError {}

fn le16(input: &[u8], at: usize) -> Option<u16> {
    let b = input.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn le32(input: &[u8], at: usize) -> Option<u32> {
    let b = input.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The bytes from `at` to the next zero, and where the byte after that zero
/// is: 2.3.1's FNAME and FCOMMENT are "zero-terminated".
fn zero_terminated(input: &[u8], at: usize) -> Result<(&[u8], usize), GzipError> {
    let rest = input.get(at..).ok_or(GzipError::TruncatedHeader)?;
    let length = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(GzipError::TruncatedHeader)?;
    Ok((&rest[..length], at + length + 1))
}

/// Reads one gzip member from the start of `input`, inflating its data under
/// `limits`.
///
/// What follows the member is not read: [`GzipMember::end`] says where it
/// begins, and [`gzip_decode`] reads a file's members in series.
///
/// # Errors
/// [`GzipError`], each variant one way a member fails 2.3's layout, its
/// checks or the caller's ceiling.
pub fn gzip_member<'a>(input: &'a [u8], limits: &Limits) -> Result<GzipMember<'a>, GzipError> {
    if input.len() < FIXED_HEADER || input[..2] != MAGIC {
        return Err(GzipError::NotGzip);
    }
    let method = input[2];
    if method != DEFLATE {
        return Err(GzipError::Method(method));
    }
    let flags = input[3];
    if flags & RESERVED != 0 {
        return Err(GzipError::ReservedFlags(flags));
    }
    let mtime = le32(input, 4).ok_or(GzipError::NotGzip)?;
    let (extra_flags, os) = (input[8], input[9]);

    // 2.3: the optional fields, each present as its bit says and in this
    // order.
    let mut at = FIXED_HEADER;
    let extra = if flags & FEXTRA != 0 {
        // 2.3.1: "XLEN ... the length of the optional extra field", which
        // 2.3.1.1 divides into subfields; skipping it is all 2.3.1.2 asks.
        let length = usize::from(le16(input, at).ok_or(GzipError::TruncatedHeader)?);
        let field = input
            .get(at + 2..at + 2 + length)
            .ok_or(GzipError::TruncatedHeader)?;
        at += 2 + length;
        Some(field)
    } else {
        None
    };
    let name = if flags & FNAME != 0 {
        let (name, after) = zero_terminated(input, at)?;
        at = after;
        Some(name)
    } else {
        None
    };
    let comment = if flags & FCOMMENT != 0 {
        let (comment, after) = zero_terminated(input, at)?;
        at = after;
        Some(comment)
    } else {
        None
    };
    let header_crc = if flags & FHCRC != 0 {
        let stated = le16(input, at).ok_or(GzipError::TruncatedHeader)?;
        // 2.3.1: "the two least significant bytes of the CRC32 for all bytes
        // of the gzip header up to and not including the CRC16".
        let mut crc = Crc32::new();
        crc.update(&input[..at]);
        let computed = (crc.finish() & 0xffff) as u16;
        if stated != computed {
            return Err(GzipError::HeaderCrc { stated, computed });
        }
        at += 2;
        Some(stated)
    } else {
        None
    };

    let body = input.get(at..).unwrap_or_default();
    let inflated = inflate_raw(body, limits);
    if inflated.capped {
        return Err(GzipError::ExceedsOutputLimit {
            limit: limits.max_output,
        });
    }
    if !inflated.complete {
        return Err(if inflated.warnings.contains(&Warning::TruncatedInput) {
            GzipError::TruncatedData
        } else {
            GzipError::CorruptData
        });
    }

    // `end` is meaningful because the inflate completed: the final block
    // ends there, on the byte it ends inside.
    let trailer_at = at + inflated.end;
    let (Some(stated_crc), Some(stated_size)) =
        (le32(input, trailer_at), le32(input, trailer_at + 4))
    else {
        return Err(GzipError::TruncatedTrailer);
    };
    let mut crc = Crc32::new();
    crc.update(&inflated.data);
    let computed_crc = crc.finish();
    if stated_crc != computed_crc {
        return Err(GzipError::Crc {
            stated: stated_crc,
            computed: computed_crc,
        });
    }
    // 2.3.1: ISIZE is "the size of the original (uncompressed) input data
    // modulo 2^32"; the mask is that modulo, and a `usize` is at most 64 bits.
    let computed_size = (inflated.data.len() as u64 & 0xffff_ffff) as u32;
    if stated_size != computed_size {
        return Err(GzipError::Size {
            stated: stated_size,
            computed: computed_size,
        });
    }

    Ok(GzipMember {
        header: GzipHeader {
            text: flags & FTEXT != 0,
            mtime,
            extra_flags,
            os,
            extra,
            name,
            comment,
            header_crc,
        },
        data: inflated.data,
        crc: computed_crc,
        end: trailer_at + TRAILER,
    })
}

/// Reads a gzip file: its members in series (2.2), their data concatenated,
/// the whole held to `limits` together.
///
/// The first member starts the input and every byte after it must begin
/// another; a file is at least one member.
///
/// # Errors
/// The first member's [`GzipError`] where it fails, and the first failing
/// later member's after it; [`GzipError::TrailingBytes`] where bytes after a
/// member do not begin with ID1 and ID2; [`GzipError::ExceedsOutputLimit`]
/// where the members together inflate past the ceiling.
pub fn gzip_decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>, GzipError> {
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        let rest = input.get(at..).unwrap_or_default();
        if at > 0 && !rest.starts_with(&MAGIC) {
            return Err(GzipError::TrailingBytes { at });
        }
        let room = limits.max_output.saturating_sub(out.len());
        let member = gzip_member(rest, &Limits::new(room)).map_err(|error| match error {
            GzipError::ExceedsOutputLimit { .. } => GzipError::ExceedsOutputLimit {
                limit: limits.max_output,
            },
            other => other,
        })?;
        at += member.end;
        // The first member's data is the file's so far, moved rather than
        // copied, so a file of one member holds its data once.
        if out.is_empty() {
            out = member.data;
        } else {
            out.extend_from_slice(&member.data);
        }
        if at >= input.len() {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Hello, hello, hello!" as GNU gzip 1.14 wrote it with `-9` from a file
    /// named `hello.txt` whose modification time was 2024-01-02 03:04:05 UTC:
    /// FLG 0x08 (FNAME), MTIME 0x65937D25, XFL 2, OS 3 (Unix), the name, the
    /// deflate data, CRC32 0xE42EEBAC and ISIZE 20. Generated once by that
    /// program and committed; nothing here asks it anything.
    const GNU_GZIP_NAMED: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x08, 0x25, 0x7d, 0x93, 0x65, 0x02, 0x03, b'h', b'e', b'l', b'l', b'o',
        b'.', b't', b'x', b't', 0x00, 0xf3, 0x48, 0xcd, 0xc9, 0xc9, 0xd7, 0x51, 0xc8, 0x40, 0xa2,
        0x14, 0x01, 0xac, 0xeb, 0x2e, 0xe4, 0x14, 0x00, 0x00, 0x00,
    ];

    /// The same text through `gzip -n -1` from a pipe: no name, MTIME 0, XFL 4
    /// (the fastest), OS 3, and the same deflate data, CRC32 and ISIZE.
    const GNU_GZIP_PLAIN: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x03, 0xf3, 0x48, 0xcd, 0xc9, 0xc9,
        0xd7, 0x51, 0xc8, 0x40, 0xa2, 0x14, 0x01, 0xac, 0xeb, 0x2e, 0xe4, 0x14, 0x00, 0x00, 0x00,
    ];

    const HELLO: &[u8] = b"Hello, hello, hello!";

    /// The deflate data both GNU members carry: fixed Huffman codes ending
    /// part-way through their last byte.
    const HELLO_DEFLATE: &[u8] = &[
        0xf3, 0x48, 0xcd, 0xc9, 0xc9, 0xd7, 0x51, 0xc8, 0x40, 0xa2, 0x14, 0x01,
    ];

    /// The CRC-32 of "123456789", the check value the CRC catalogues publish
    /// for this polynomial and this reflection.
    const CHECK_CRC: u32 = 0xcbf4_3926;

    const LIMIT: Limits = Limits::new(1 << 16);

    /// RFC 1951 3.2.4's stored block, final, holding `data`.
    fn stored(data: &[u8]) -> Vec<u8> {
        let length = u16::try_from(data.len()).expect("a stored block holds 65 535 bytes");
        let mut out = vec![0x01];
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    /// A member laid out as 2.3 lays it out, from its parts, with the CRC16
    /// computed over the header when `header_crc` asks for one.
    struct Parts<'a> {
        flags: u8,
        mtime: u32,
        extra: Option<&'a [u8]>,
        name: Option<&'a [u8]>,
        comment: Option<&'a [u8]>,
        header_crc: bool,
        deflate: Vec<u8>,
        crc: u32,
        size: u32,
    }

    impl Parts<'_> {
        fn of(deflate: Vec<u8>, crc: u32, size: u32) -> Self {
            Parts {
                flags: 0,
                mtime: 0,
                extra: None,
                name: None,
                comment: None,
                header_crc: false,
                deflate,
                crc,
                size,
            }
        }

        fn build(&self) -> Vec<u8> {
            let mut flags = self.flags;
            if self.extra.is_some() {
                flags |= FEXTRA;
            }
            if self.name.is_some() {
                flags |= FNAME;
            }
            if self.comment.is_some() {
                flags |= FCOMMENT;
            }
            if self.header_crc {
                flags |= FHCRC;
            }
            let mut out = vec![0x1f, 0x8b, 8, flags];
            out.extend_from_slice(&self.mtime.to_le_bytes());
            out.extend_from_slice(&[0, 255]);
            if let Some(extra) = self.extra {
                let length = u16::try_from(extra.len()).expect("XLEN is two bytes");
                out.extend_from_slice(&length.to_le_bytes());
                out.extend_from_slice(extra);
            }
            for field in [self.name, self.comment].into_iter().flatten() {
                out.extend_from_slice(field);
                out.push(0);
            }
            if self.header_crc {
                let mut crc = Crc32::new();
                crc.update(&out);
                let crc16 = (crc.finish() & 0xffff) as u16;
                out.extend_from_slice(&crc16.to_le_bytes());
            }
            out.extend_from_slice(&self.deflate);
            out.extend_from_slice(&self.crc.to_le_bytes());
            out.extend_from_slice(&self.size.to_le_bytes());
            out
        }
    }

    /// "123456789" in a stored block, under its published CRC-32.
    fn check_member() -> Parts<'static> {
        Parts::of(stored(b"123456789"), CHECK_CRC, 9)
    }

    #[test]
    fn a_member_inflates_with_its_crc_and_size_checked() {
        // A real producer's member, its name field and all.
        let member = gzip_member(GNU_GZIP_NAMED, &LIMIT).expect("GNU gzip's member reads");
        assert_eq!(member.data, HELLO);
        assert_eq!(member.crc, 0xe42e_ebac);
        assert_eq!(member.end, GNU_GZIP_NAMED.len());
        assert_eq!(
            member.header,
            GzipHeader {
                text: false,
                mtime: 0x6593_7d25,
                extra_flags: 2,
                os: 3,
                extra: None,
                name: Some(b"hello.txt"),
                comment: None,
                header_crc: None,
            }
        );

        let plain = gzip_member(GNU_GZIP_PLAIN, &LIMIT).expect("the nameless member reads");
        assert_eq!(plain.data, HELLO);
        assert_eq!(plain.header.name, None);
        assert_eq!((plain.header.mtime, plain.header.extra_flags), (0, 4));
        assert_eq!(plain.end, GNU_GZIP_PLAIN.len());

        // A stored block under the CRC-32 every catalogue publishes for
        // "123456789", so the check is held to a number this crate did not
        // compute.
        let bytes = check_member().build();
        let member = gzip_member(&bytes, &LIMIT).expect("the stored member reads");
        assert_eq!(member.data, b"123456789");
        assert_eq!(member.crc, CHECK_CRC);
        assert_eq!(member.end, bytes.len());

        // An empty member: a final stored block of nothing, CRC-32 0, ISIZE 0.
        let empty = Parts::of(stored(b""), 0, 0).build();
        let member = gzip_member(&empty, &LIMIT).expect("an empty member reads");
        assert!(member.data.is_empty());
        assert_eq!(member.end, empty.len());

        // What follows a member is not read, and `end` lands on it.
        let mut followed = GNU_GZIP_PLAIN.to_vec();
        followed.extend_from_slice(b"after");
        let member = gzip_member(&followed, &LIMIT).expect("the member reads");
        assert_eq!(&followed[member.end..], b"after");
    }

    #[test]
    fn every_optional_header_field_is_skipped_as_its_flag_says() {
        let crc = {
            let mut crc = Crc32::new();
            crc.update(HELLO);
            crc.finish()
        };
        assert_eq!(crc, 0xe42e_ebac, "the CRC GNU gzip wrote for the text");
        // 2.3.1.1's layout: SI1, SI2, LEN and LEN bytes, twice.
        let extra: &[u8] = &[b'A', b'P', 2, 0, 0xde, 0xad, b'Z', b'z', 0, 0];
        // ISO 8859-1, as 2.3.1 has a name: `café` with its `é` as one byte.
        let name: &[u8] = b"caf\xe9.svg";
        let comment: &[u8] = b"a comment\nof two lines";
        for mask in 0u8..32 {
            let mut parts = Parts::of(HELLO_DEFLATE.to_vec(), crc, 20);
            parts.flags = mask & FTEXT;
            parts.mtime = 0x0102_0304;
            parts.extra = (mask & FEXTRA != 0).then_some(extra);
            parts.name = (mask & FNAME != 0).then_some(name);
            parts.comment = (mask & FCOMMENT != 0).then_some(comment);
            parts.header_crc = mask & FHCRC != 0;
            let bytes = parts.build();
            let member = gzip_member(&bytes, &LIMIT)
                .unwrap_or_else(|error| panic!("flags {mask:#04x}: {error:?}"));
            assert_eq!(member.data, HELLO, "flags {mask:#04x}");
            assert_eq!(member.end, bytes.len(), "flags {mask:#04x}");
            let header = member.header;
            assert_eq!(header.text, mask & FTEXT != 0, "flags {mask:#04x}");
            assert_eq!(header.mtime, 0x0102_0304);
            assert_eq!((header.extra_flags, header.os), (0, 255));
            assert_eq!(header.extra, parts.extra, "flags {mask:#04x}");
            assert_eq!(header.name, parts.name, "flags {mask:#04x}");
            assert_eq!(header.comment, parts.comment, "flags {mask:#04x}");
            assert_eq!(
                header.header_crc.is_some(),
                mask & FHCRC != 0,
                "flags {mask:#04x}"
            );
        }

        // An extra field of no bytes, and a name of none: each present, empty.
        let mut parts = check_member();
        parts.extra = Some(&[]);
        parts.name = Some(b"");
        let bytes = parts.build();
        let member = gzip_member(&bytes, &LIMIT).expect("empty fields read");
        assert_eq!(member.header.extra, Some(&[][..]));
        assert_eq!(member.header.name, Some(&b""[..]));
        assert_eq!(member.data, b"123456789");

        // A zero inside the extra field is data: XLEN, not a terminator,
        // decides where the field ends.
        let mut parts = check_member();
        parts.extra = Some(&[0, 0, 0, 0]);
        parts.name = Some(b"n");
        let bytes = parts.build();
        let member = gzip_member(&bytes, &LIMIT).expect("the extra field is skipped by XLEN");
        assert_eq!(member.header.name, Some(&b"n"[..]));
    }

    #[test]
    fn a_member_with_a_wrong_crc_or_size_is_refused() {
        // The CRC32 off by one bit.
        let mut wrong_crc = check_member();
        wrong_crc.crc ^= 1;
        assert_eq!(
            gzip_member(&wrong_crc.build(), &LIMIT),
            Err(GzipError::Crc {
                stated: CHECK_CRC ^ 1,
                computed: CHECK_CRC,
            })
        );
        // ISIZE one too many, and one too few.
        for size in [10, 8] {
            let mut wrong_size = check_member();
            wrong_size.size = size;
            assert_eq!(
                gzip_member(&wrong_size.build(), &LIMIT),
                Err(GzipError::Size {
                    stated: size,
                    computed: 9,
                })
            );
        }
        // GNU gzip's member with a byte of its trailer flipped, each byte in
        // turn: the first four are CRC32's, the last four ISIZE's.
        let trailer = GNU_GZIP_NAMED.len() - TRAILER;
        for at in trailer..GNU_GZIP_NAMED.len() {
            let mut bytes = GNU_GZIP_NAMED.to_vec();
            bytes[at] ^= 0x40;
            let refused = gzip_member(&bytes, &LIMIT);
            if at < trailer + 4 {
                assert!(
                    matches!(refused, Err(GzipError::Crc { .. })),
                    "{at}: {refused:?}"
                );
            } else {
                assert!(
                    matches!(refused, Err(GzipError::Size { .. })),
                    "{at}: {refused:?}"
                );
            }
        }
        // A CRC16 that does not match the header it follows.
        let mut parts = check_member();
        parts.name = Some(b"glyphs.svg");
        parts.header_crc = true;
        let mut bytes = parts.build();
        let crc16_at = FIXED_HEADER + b"glyphs.svg".len() + 1;
        bytes[crc16_at] ^= 0xff;
        assert!(matches!(
            gzip_member(&bytes, &LIMIT),
            Err(GzipError::HeaderCrc { .. })
        ));
        // And a header byte changed under a CRC16 that was right for the
        // header as it was.
        let mut bytes = parts.build();
        bytes[4] ^= 1;
        assert!(matches!(
            gzip_member(&bytes, &LIMIT),
            Err(GzipError::HeaderCrc { .. })
        ));
    }

    #[test]
    fn a_member_past_its_limit_is_refused() {
        // Twenty bytes inflate under a ceiling of twenty, and not of nineteen.
        let member = gzip_member(GNU_GZIP_NAMED, &Limits::new(20)).expect("at the ceiling");
        assert_eq!(member.data.len(), 20);
        assert_eq!(
            gzip_member(GNU_GZIP_NAMED, &Limits::new(19)),
            Err(GzipError::ExceedsOutputLimit { limit: 19 })
        );
        assert_eq!(
            gzip_member(GNU_GZIP_NAMED, &Limits::new(0)),
            Err(GzipError::ExceedsOutputLimit { limit: 0 })
        );
        // A stored block's bytes are held to the ceiling as a coded block's
        // are, and an empty member is under every ceiling.
        assert_eq!(
            gzip_member(&check_member().build(), &Limits::new(8)),
            Err(GzipError::ExceedsOutputLimit { limit: 8 })
        );
        let empty = Parts::of(stored(b""), 0, 0).build();
        assert!(gzip_member(&empty, &Limits::new(0)).is_ok());

        // A file's ceiling is its members' together: two twenty-byte members
        // fit forty and not thirty-nine.
        let mut two = GNU_GZIP_NAMED.to_vec();
        two.extend_from_slice(GNU_GZIP_PLAIN);
        assert_eq!(
            gzip_decode(&two, &Limits::new(40)).map(|data| data.len()),
            Ok(40)
        );
        assert_eq!(
            gzip_decode(&two, &Limits::new(39)),
            Err(GzipError::ExceedsOutputLimit { limit: 39 })
        );
    }

    #[test]
    fn a_header_that_is_not_a_gzip_header_is_refused() {
        assert_eq!(gzip_member(b"", &LIMIT), Err(GzipError::NotGzip));
        assert_eq!(
            gzip_member(&GNU_GZIP_PLAIN[..9], &LIMIT),
            Err(GzipError::NotGzip)
        );
        for (at, value) in [(0usize, 0x1eu8), (1, 0x8a)] {
            let mut bytes = GNU_GZIP_PLAIN.to_vec();
            bytes[at] = value;
            assert_eq!(gzip_member(&bytes, &LIMIT), Err(GzipError::NotGzip), "{at}");
        }
        // A zlib stream is not a gzip member.
        assert_eq!(
            gzip_member(&crate::zlib_compress(HELLO), &LIMIT),
            Err(GzipError::NotGzip)
        );
        for method in [0u8, 7, 9, 255] {
            let mut bytes = GNU_GZIP_PLAIN.to_vec();
            bytes[2] = method;
            assert_eq!(gzip_member(&bytes, &LIMIT), Err(GzipError::Method(method)));
        }
        for bit in [0x20u8, 0x40, 0x80] {
            let mut bytes = GNU_GZIP_PLAIN.to_vec();
            bytes[3] |= bit;
            assert_eq!(
                gzip_member(&bytes, &LIMIT),
                Err(GzipError::ReservedFlags(bit))
            );
        }
    }

    /// Every prefix of a member is refused, by where it stops — the header,
    /// the data or the trailer — and the whole member alone reads.
    #[test]
    fn every_truncation_is_refused_by_where_it_stops() {
        let mut parts = Parts::of(HELLO_DEFLATE.to_vec(), 0xe42e_ebac, 20);
        parts.extra = Some(&[b'S', b'V', 1, 0, 7]);
        parts.name = Some(b"glyphs.svg");
        parts.comment = Some(b"c");
        parts.header_crc = true;
        let bytes = parts.build();
        let data_at = FIXED_HEADER + 2 + 5 + b"glyphs.svg".len() + 1 + 2 + 2;
        let trailer_at = data_at + HELLO_DEFLATE.len();
        assert_eq!(trailer_at + TRAILER, bytes.len());
        for end in 0..bytes.len() {
            let refused = gzip_member(&bytes[..end], &LIMIT);
            let expected = if end < FIXED_HEADER {
                GzipError::NotGzip
            } else if end < data_at {
                GzipError::TruncatedHeader
            } else if end < trailer_at {
                GzipError::TruncatedData
            } else {
                GzipError::TruncatedTrailer
            };
            assert_eq!(refused, Err(expected), "a member cut at {end}");
        }
        assert!(gzip_member(&bytes, &LIMIT).is_ok());

        // A name whose zero never comes runs to the end of the input, and is
        // refused there rather than read past it.
        let mut bytes = GNU_GZIP_NAMED[..19].to_vec();
        bytes.extend_from_slice(b"no terminator");
        assert_eq!(gzip_member(&bytes, &LIMIT), Err(GzipError::TruncatedHeader));
        // An XLEN past the input.
        let mut bytes = GNU_GZIP_PLAIN[..FIXED_HEADER].to_vec();
        bytes[3] = FEXTRA;
        bytes.extend_from_slice(&[0xff, 0xff, 1, 2, 3]);
        assert_eq!(gzip_member(&bytes, &LIMIT), Err(GzipError::TruncatedHeader));
    }

    #[test]
    fn corrupt_deflate_data_is_refused() {
        // BTYPE 3, which RFC 1951 3.2.3 reserves.
        let reserved = Parts::of(vec![0x07, 0x00], 0, 0).build();
        assert_eq!(gzip_member(&reserved, &LIMIT), Err(GzipError::CorruptData));
        // A stored block whose NLEN is not LEN's complement.
        let mut block = stored(b"123456789");
        block[3] ^= 1;
        let mismatched = Parts::of(block, CHECK_CRC, 9).build();
        assert_eq!(
            gzip_member(&mismatched, &LIMIT),
            Err(GzipError::CorruptData)
        );
    }

    #[test]
    fn members_in_series_decode_to_their_concatenation() {
        let mut file = GNU_GZIP_NAMED.to_vec();
        file.extend_from_slice(&check_member().build());
        file.extend_from_slice(GNU_GZIP_PLAIN);
        let mut expected = HELLO.to_vec();
        expected.extend_from_slice(b"123456789");
        expected.extend_from_slice(HELLO);
        assert_eq!(gzip_decode(&file, &LIMIT), Ok(expected));

        assert_eq!(gzip_decode(GNU_GZIP_PLAIN, &LIMIT), Ok(HELLO.to_vec()));
        assert_eq!(gzip_decode(b"", &LIMIT), Err(GzipError::NotGzip));

        // Bytes after the last member that begin no member are refused by
        // where they start; bytes that begin a member are read as one, and
        // its own failure is the file's.
        let mut junk = GNU_GZIP_PLAIN.to_vec();
        junk.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(
            gzip_decode(&junk, &LIMIT),
            Err(GzipError::TrailingBytes {
                at: GNU_GZIP_PLAIN.len()
            })
        );
        let mut damaged = GNU_GZIP_PLAIN.to_vec();
        let mut second = check_member();
        second.crc ^= 0x8000_0000;
        damaged.extend_from_slice(&second.build());
        assert!(matches!(
            gzip_decode(&damaged, &LIMIT),
            Err(GzipError::Crc { .. })
        ));
        let mut cut = GNU_GZIP_PLAIN.to_vec();
        cut.extend_from_slice(&GNU_GZIP_PLAIN[..4]);
        assert_eq!(gzip_decode(&cut, &LIMIT), Err(GzipError::NotGzip));
    }

    #[test]
    fn every_byte_flip_of_a_member_answers_without_panicking() {
        let mut parts = Parts::of(HELLO_DEFLATE.to_vec(), 0xe42e_ebac, 20);
        parts.extra = Some(&[b'S', b'V', 1, 0, 7]);
        parts.name = Some(b"n");
        parts.comment = Some(b"c");
        parts.header_crc = true;
        let bytes = parts.build();
        for at in 0..bytes.len() {
            for flip in [0x01u8, 0x80, 0xff] {
                let mut flipped = bytes.clone();
                flipped[at] ^= flip;
                if let Ok(member) = gzip_member(&flipped, &LIMIT) {
                    assert!(member.end <= flipped.len());
                    assert!(member.data.len() <= LIMIT.max_output);
                }
                let _ = gzip_decode(&flipped, &LIMIT);
            }
        }
    }
}
