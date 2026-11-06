//! The file a command opens: a [`ByteSource`] that reads it at offsets, on
//! demand; the whole-file read, in place; and the open every reading command
//! makes by the rule (`docs/design/streaming-open.md`, "A file-backed source
//! in the hosts that own files").
//!
//! Names nothing but `std` and `tinker_pdf`, so a probe that opens a file as
//! `tpdf` does includes this by path rather than copying it.

use std::fs::File;
use std::io;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use tinker_pdf::{ByteSource, Document, OpenError, SourceMiss};

/// A file, read at offsets.
///
/// Its length is the open handle's, read once: every offset a PDF states is
/// measured against the file as it was when it opened, so bytes appended
/// after the open are never read. A read answers its whole range or misses,
/// and one that fails, or ends inside that length because the file shrank,
/// is a [`SourceMiss`] naming its range; the first is kept, with its error,
/// for [`FileSource::failure`]. Reads take their offset from every call and
/// share no cursor, so the engine's threads read at once.
pub struct FileSource {
    file: File,
    len: u64,
    failure: Mutex<Option<Failure>>,
}

/// The first read a [`FileSource`] could not make: its range, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The bytes it was asked for.
    pub range: Range<u64>,
    /// What reading them said.
    pub error: String,
}

impl FileSource {
    /// The file at `path`, opened for reading.
    ///
    /// # Errors
    /// The file's own: absent, unreadable, or without a length.
    pub fn open(path: &str) -> io::Result<FileSource> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(FileSource {
            file,
            len,
            failure: Mutex::new(None),
        })
    }

    /// The first read that failed, when one has.
    pub fn failure(&self) -> Option<Failure> {
        match self.failure.lock() {
            Ok(failure) => failure.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// `range`, which lies inside the length, in one buffer of its size,
    /// filled in place: no `Vec` is copied into an `Arc`.
    fn fill(&self, range: Range<u64>) -> io::Result<Arc<[u8]>> {
        let len = usize::try_from(range.end - range.start)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "a range past memory"))?;
        let mut bytes: Arc<[u8]> = std::iter::repeat_n(0u8, len).collect();
        let Some(buffer) = Arc::get_mut(&mut bytes) else {
            return Err(io::Error::other("a fresh buffer is shared"));
        };
        read_at(&self.file, buffer, range.start)?;
        Ok(bytes)
    }
}

impl ByteSource for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        let end = range.end.min(self.len);
        if range.start >= end {
            return Ok(Arc::from(&[][..]));
        }
        let wanted = range.start..end;
        self.fill(wanted.clone()).map_err(|error| {
            let mut failure = match self.failure.lock() {
                Ok(failure) => failure,
                Err(poisoned) => poisoned.into_inner(),
            };
            failure.get_or_insert_with(|| Failure {
                range: wanted.clone(),
                error: error.to_string(),
            });
            SourceMiss::at(wanted)
        })
    }
}

/// `buffer.len()` bytes of `file` at `offset`: `pread` in a loop that retries
/// an interrupted call.
#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buffer, offset)
}

/// `buffer.len()` bytes of `file` at `offset`: `seek_read` takes its offset
/// from every call, and a short answer is asked again from where it stopped.
#[cfg(windows)]
fn read_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buffer.is_empty() {
        match file.seek_read(buffer, offset) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(read) => {
                buffer = &mut buffer[read..];
                offset += read as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// `buffer.len()` bytes of `file` at `offset`, through the handle's cursor,
/// on a target with no positioned read: none `tpdf` builds for has threads
/// without one.
#[cfg(not(any(unix, windows)))]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut cursor = file;
    cursor.seek(SeekFrom::Start(offset))?;
    cursor.read_exact(buffer)
}

/// The whole file at `path`, read in place into one buffer of its length.
///
/// # Errors
/// `reading {path}: {error}`.
pub fn read_whole(path: &str) -> Result<Arc<[u8]>, String> {
    let file = FileSource::open(path).map_err(|e| format!("reading {path}: {e}"))?;
    file.fill(0..file.len)
        .map_err(|e| format!("reading {path}: {e}"))
}

/// Why `path` did not open: its bytes would not read, or the engine would not
/// open them.
#[derive(Debug)]
pub enum Unopened {
    /// `reading {path}: {error}`.
    Read(String),
    /// The buffered open's answer.
    Open(OpenError),
}

/// A document opened by the rule, through `source`: streamed, and kept when
/// it answers as a buffered open of the same bytes would
/// (`CosDocument::answers_as_buffered`, asked before anything else is read);
/// otherwise `path` read whole and opened from the buffer.
///
/// # Errors
/// The whole read's, or the buffered open's.
pub fn open_by_rule(path: &str, source: Arc<dyn ByteSource>) -> Result<Document, Unopened> {
    if let Ok(document) = Document::open_streaming(source) {
        if document.cos().answers_as_buffered() {
            return Ok(document);
        }
    }
    let bytes = read_whole(path).map_err(Unopened::Read)?;
    Document::open(bytes).map_err(Unopened::Open)
}

/// `path` opened on demand, by the rule, and its file source when the
/// document kept it.
///
/// # Errors
/// As [`open_by_rule`].
pub fn open_on_demand(path: &str) -> Result<(Document, Option<Arc<FileSource>>), Unopened> {
    let file = Arc::new(
        FileSource::open(path).map_err(|e| Unopened::Read(format!("reading {path}: {e}")))?,
    );
    let document = open_by_rule(path, Arc::clone(&file) as Arc<dyn ByteSource>)?;
    let kept = document.cos().is_streamed().then_some(file);
    Ok((document, kept))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file written for one test, in the system's temporary directory.
    fn written(name: &str, bytes: &[u8]) -> String {
        let path = std::env::temp_dir().join(format!("tpdf-source-{name}-{}", std::process::id()));
        std::fs::write(&path, bytes).expect("the file is written");
        path.to_string_lossy().into_owned()
    }

    /// The length is fixed at open, so a read inside it that the file no
    /// longer has, because it shrank, misses naming the range asked for, and
    /// the failure is kept with its range; a range past the length is empty.
    #[test]
    fn a_read_inside_a_file_that_shrank_is_a_miss_naming_its_range() {
        let path = written("shrank", &[7u8; 4096]);
        let source = FileSource::open(&path).expect("it opens");
        assert_eq!(source.read(100..200).expect("inside").len(), 100);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_len(1024))
            .expect("the file shrinks");
        assert_eq!(source.len(), 4096, "the length is the open's");
        assert_eq!(source.read(2000..3000), Err(SourceMiss::at(2000..3000)));
        let failure = source.failure().expect("the failure is kept");
        assert_eq!(failure.range, 2000..3000);
        assert!(source.read(5000..6000).expect("past the end").is_empty());
        assert_eq!(
            source.failure().map(|failure| failure.range),
            Some(2000..3000),
            "the first failure stays the one kept"
        );
        let _ = std::fs::remove_file(&path);
    }
}
