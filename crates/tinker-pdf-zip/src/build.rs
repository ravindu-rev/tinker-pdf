//! A test-only ZIP writer.
//!
//! Hand-built archives, byte by byte, because there are none in the tree and a
//! reader tested only against what its own writer emits proves less than it
//! looks. Gap 18 M6 found a JPEG 2000 plane count that was correct *only* for
//! the code-blocks its in-tree encoder could produce — every round trip took
//! the one working case, for milestones. So this writer exists to build the
//! shapes a real archiver emits **and the ones it does not**: a truncated
//! directory, a streamed entry, a name that is CP437 rather than UTF-8, an
//! offset that points at nothing.
//!
//! Nothing here ships. `#[cfg(test)]` at the module declaration.

use tinker_pdf_filters::{crc32, deflate};

use crate::local::{CENTRAL_SIG, DESCRIPTOR_SIG, EOCD_SIG, LOCAL_SIG};

/// One entry to write.
pub(crate) struct File {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    /// Compressed bytes to write verbatim, instead of compressing `data`.
    ///
    /// This is how a test builds a deflate stream no in-tree encoder emits —
    /// the stored-block-first shape that a zlib header sniff misreads as its
    /// own. Without it the reader would only ever be tested against what this
    /// file's own `deflate` call produces, which is the round-trip trap gap 18
    /// M6 fell into.
    pub raw: Option<Vec<u8>>,
    /// Method 8 rather than method 0.
    pub deflated: bool,
    /// Bit 3: the sizes and CRC live in a descriptor after the data, and the
    /// local header carries zeroes.
    pub streamed: bool,
    /// Bit 0.
    pub encrypted: bool,
    /// Bit 11: the name is UTF-8 rather than CP437.
    pub utf8: bool,
    /// Bytes for the **local** header's extra area, and only the local one.
    ///
    /// 4.4.11 lets the two areas differ, so writing one on one side is a legal
    /// archive rather than a damaged one — and it is the shape
    /// `Archive::local_extra` exists to answer, since a central-directory field
    /// would have said the wrong thing about it.
    pub local_extra: Vec<u8>,
}

impl File {
    pub(crate) fn stored(name: &[u8], data: &[u8]) -> File {
        File {
            name: name.to_vec(),
            data: data.to_vec(),
            raw: None,
            deflated: false,
            streamed: false,
            encrypted: false,
            utf8: true,
            local_extra: Vec::new(),
        }
    }

    pub(crate) fn deflated(name: &[u8], data: &[u8]) -> File {
        File {
            deflated: true,
            ..File::stored(name, data)
        }
    }

    fn flags(&self) -> u16 {
        let mut flags = 0u16;
        if self.encrypted {
            flags |= 1;
        }
        if self.streamed {
            flags |= 1 << 3;
        }
        if self.utf8 {
            flags |= 1 << 11;
        }
        flags
    }

    fn payload(&self) -> Vec<u8> {
        if let Some(raw) = &self.raw {
            return raw.clone();
        }
        if self.deflated {
            deflate(&self.data)
        } else {
            self.data.clone()
        }
    }
}

/// How the archive should be damaged, if at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Damage {
    None,
    /// No end-of-central-directory record: the recovery rung.
    NoDirectory,
    /// The record is there and its offset points nowhere.
    DirectoryOffsetWrong,
    /// The record claims more entries than it wrote.
    DirectoryCountTooHigh,
    /// One entry's stored CRC is wrong.
    CorruptCrc,
}

/// Builds an archive.
pub(crate) fn archive(files: &[File], damage: Damage) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for file in files {
        let offset = out.len() as u32;
        let payload = file.payload();
        let mut crc = crc32(&file.data);
        if damage == Damage::CorruptCrc {
            crc ^= 0xFFFF_FFFF;
        }
        let (header_crc, header_c, header_u) = if file.streamed {
            (0, 0, 0)
        } else {
            (crc, payload.len() as u32, file.data.len() as u32)
        };

        out.extend_from_slice(&LOCAL_SIG);
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&file.flags().to_le_bytes());
        out.extend_from_slice(&if file.deflated { 8u16 } else { 0 }.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&header_crc.to_le_bytes());
        out.extend_from_slice(&header_c.to_le_bytes());
        out.extend_from_slice(&header_u.to_le_bytes());
        out.extend_from_slice(&(file.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(file.local_extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&file.name);
        out.extend_from_slice(&file.local_extra);
        out.extend_from_slice(&payload);

        if file.streamed {
            // 4.3.9, signed, which is what every real archiver writes.
            out.extend_from_slice(&DESCRIPTOR_SIG);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(file.data.len() as u32).to_le_bytes());
        }

        central.extend_from_slice(&CENTRAL_SIG);
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&file.flags().to_le_bytes());
        central.extend_from_slice(&if file.deflated { 8u16 } else { 0 }.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(file.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(file.name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&file.name);
    }

    if damage == Damage::NoDirectory {
        return out;
    }

    let directory_at = out.len() as u32;
    out.extend_from_slice(&central);

    let count = match damage {
        Damage::DirectoryCountTooHigh => files.len() as u16 + 5,
        _ => files.len() as u16,
    };
    let start = match damage {
        Damage::DirectoryOffsetWrong => 0xFFFF_FFF0u32,
        _ => directory_at,
    };

    out.extend_from_slice(&EOCD_SIG);
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // directory disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out
}
