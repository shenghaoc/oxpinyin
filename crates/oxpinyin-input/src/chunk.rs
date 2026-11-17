//! MemoryChunk::mmap and ::save: owned private mappings and the pin's
//! open, two header writes, payload write,
//! g_fsync and close (074a2219 include/memory_chunk.h:536-557).
//!
//! The calls are the pin's: open(filename, O_CREAT|O_WRONLY|O_TRUNC|O_BINARY,
//! 0644), one write per header word, one write for the payload,
//! g_fsync(fd), then close(fd). A short header write is the pin's live
//! assert (the pin asserts; the C adapters abort on the returned fault); a short or
//! failed payload write is the pin's soft false, with no sync and no close
//! result check.

use std::io;
use std::path::Path;

use crate::native;
#[cfg(unix)]
use crate::path;

/// Whether the pin's chunk loader opens a directory row as a file and
/// judges it by its reported size.
///
/// Unix opens the directory and seeks it, so `MemoryChunk::mmap` reaches its
/// header checks; Windows refuses the open, so a directory row is skipped
/// without a word.
#[must_use]
pub const fn opens_directories() -> bool {
    cfg!(unix)
}

/// The header and bytes returned by the single payload read.
pub struct LoadedChunk {
    /// The declared payload length, in the file's native header bytes.
    pub length: [u8; 4],
    /// The stored checksum, in the file's native header bytes.
    pub checksum: [u8; 4],
    /// The actual bytes returned by the one payload read.
    pub payload: Vec<u8>,
    /// The end-seek size before the header was read.
    pub file_size: u64,
    source: MappingSource,
}

impl LoadedChunk {
    /// Check the returned bytes, close the descriptor, and return the payload.
    /// A checksum mismatch rejects the chunk while its descriptor is still open.
    #[must_use]
    pub fn finish(self, checksum: impl FnOnce(&[u8]) -> u32) -> Option<Vec<u8>> {
        if checksum(&self.payload) != u32::from_le_bytes(self.checksum) {
            return None;
        }
        let Self {
            payload, source, ..
        } = self;
        drop(source);
        Some(payload)
    }
}

/// A failure to obtain the bytes of a chunk.
#[derive(Debug)]
pub enum ChunkLoadError {
    /// Open, seek, allocation or payload read failed.
    Io(io::Error),
    /// The file is shorter than its header.
    ShortHeader,
    /// One of the pin's two header read assertions failed.
    Header(io::Error),
}

/// Read a chunk using the descriptor, seeks and single reads of
/// `MemoryChunk::load` (074a2219 `memory_chunk.h:420-463`). Domain
/// validation remains with the caller.
///
/// # Errors
/// Returns the failed operation, including a short header read.
pub fn load(path: &Path) -> Result<LoadedChunk, ChunkLoadError> {
    load_with(path, |source, payload| source.read_payload(payload, None))
}

fn load_with(
    path: &Path,
    read_payload: impl FnOnce(&mut native::ReadChunk, &mut Vec<u8>) -> io::Result<usize>,
) -> Result<LoadedChunk, ChunkLoadError> {
    let mut source = MappingSource::open(path).map_err(ChunkLoadError::Io)?;
    let file_size = source.file_size();
    let (length, checksum) = load_header(file_size, || source.read_word())?;
    let available = available_payload_length(file_size);
    let declared = u32::from_le_bytes(length);
    if declared != available {
        return Err(ChunkLoadError::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "chunk length does not match file size",
        )));
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(available as usize)
        .map_err(|error| ChunkLoadError::Io(io::Error::other(error)))?;
    read_payload(&mut source.source, &mut payload).map_err(ChunkLoadError::Io)?;
    Ok(LoadedChunk {
        length,
        checksum,
        payload,
        file_size,
        source,
    })
}

fn load_header(
    file_size: u64,
    mut read_word: impl FnMut() -> io::Result<[u8; 4]>,
) -> Result<([u8; 4], [u8; 4]), ChunkLoadError> {
    if file_size < 8 {
        return Err(ChunkLoadError::ShortHeader);
    }
    let length = read_word().map_err(ChunkLoadError::Header)?;
    let checksum = read_word().map_err(ChunkLoadError::Header)?;
    Ok((length, checksum))
}

// The caller has rejected a size below the header. C narrows the remainder
// to guint32 before comparing it with the declared length.
const fn available_payload_length(file_size: u64) -> u32 {
    (file_size - 8) as u32
}

/// A chunk's open descriptor, used only while preparing its mapping.
/// Dropping this owner closes it. Header interpretation and checksum
/// validation belong to the caller in `oxpinyin-data`.
pub struct MappingSource {
    source: native::ReadChunk,
    file_size: u64,
}

impl MappingSource {
    /// Open read-only, seek to the end, then rewind, as `MemoryChunk::mmap`
    /// does at `src/include/memory_chunk.h:479-484` (074a2219).
    ///
    /// # Errors
    /// Returns the open or end-seek error.
    pub fn open(path: &Path) -> io::Result<Self> {
        let source = native::ReadChunk::open(path)?;
        let file_size = source.size_and_rewind()?;
        Ok(Self { source, file_size })
    }

    /// The whole-file size answered by `lseek(SEEK_END)`.
    #[must_use]
    pub const fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Read one four-byte word with one `read`, without retrying a short
    /// read (`memory_chunk.h:491-497`). The caller interprets its bytes.
    ///
    /// # Errors
    /// Returns a failed or short read; the pin asserts at either header read.
    pub fn read_word(&mut self) -> io::Result<[u8; 4]> {
        self.source.read_word()
    }

    /// Make one whole-file writable private mapping at offset zero.
    ///
    /// Consumes the source so it cannot make a second mapping. The pending
    /// owner keeps the descriptor open for the caller's checksum validation,
    /// as at `memory_chunk.h:505-523`. Finish validation with
    /// [`PendingMapping::finish`]; dropping a rejected pending mapping unmaps
    /// before closing, as the checksum-failure branch does at `:516-517`.
    /// System files must not be modified or truncated while mapped: a
    /// private mapping is not a snapshot, and truncation can cause SIGBUS.
    /// This boundary assumes system and addon tables are installed read-only
    /// and are not modified or truncated by any process while mapped; this
    /// runtime never writes those files.
    ///
    /// # Errors
    /// Returns a size-conversion or mapping error.
    pub fn map(self) -> io::Result<PendingMapping> {
        let len = usize::try_from(self.file_size)
            .map_err(|_| io::Error::other("file too large for this platform"))?;
        let mapping = Mapping(self.source.map_copy(len)?);
        Ok(PendingMapping {
            mapping,
            source: self.source,
        })
    }
}

/// A mapping awaiting the caller's domain validation. Fields drop in
/// declaration order: a rejected mapping is unmapped before its descriptor
/// closes (`memory_chunk.h:516-517`). No checksum logic lives here.
pub struct PendingMapping {
    mapping: Mapping,
    source: native::ReadChunk,
}

impl PendingMapping {
    /// Borrow the whole-file bytes for validation.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.mapping.as_slice()
    }

    /// Close the descriptor and return the sole mapping owner. The caller
    /// has accepted its length, checksum and any required domain checks.
    #[must_use]
    pub fn finish(self) -> Mapping {
        let Self { mapping, source } = self;
        drop(source);
        mapping
    }
}

/// One owned writable private mapping. No raw pointer or descriptor escapes;
/// drop releases the entire mapped range through memmap2 on every platform.
///
/// A reader excludes a writer for as long as the read view is used:
///
/// ```compile_fail
/// use oxpinyin_input::chunk::Mapping;
/// fn overlapping_views(mapping: &mut Mapping) {
///     let before = mapping.as_slice();
///     mapping.as_mut_slice()[0] = 1;
///     assert_eq!(before[0], 1);
/// }
/// ```
pub struct Mapping(memmap2::MmapMut);

impl Mapping {
    /// Borrow the current mapped bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Borrow this mapping exclusively for an in-place write.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

/// Which of MemoryChunk::save's two header writes the filesystem refused:
/// the length word (memory_chunk.h:543, assert(ret_len == sizeof(length)))
/// or the checksum word (:547).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkHeaderField {
    /// write(fd, &length, sizeof(guint32)) - memory_chunk.h:543.
    Length,
    /// write(fd, &checksum, sizeof(guint32)) - memory_chunk.h:547.
    Checksum,
}

impl ChunkHeaderField {
    /// The pin's assert expression at this write, for the error text.
    #[must_use]
    pub const fn assert_expression(self) -> &'static str {
        match self {
            Self::Length => "ret_len == sizeof(length)",
            Self::Checksum => "ret_len == sizeof(checksum)",
        }
    }
}

/// A failed chunk save. Io is the pin's soft false (the open or the payload
/// write); Header is the write the pin asserts on.
#[derive(Debug)]
pub enum ChunkSaveError {
    /// The open or the payload write failed; MemoryChunk::save answers
    /// false and its callers ignore that answer.
    Io(io::Error),
    /// A header word write failed or was short, where the pin aborts.
    Header {
        /// Which header word the write refused.
        field: ChunkHeaderField,
        /// The underlying write error.
        source: io::Error,
    },
}

/// MemoryChunk::save(filename) over path with the chunk's complete bytes.
///
/// # Errors
///
/// ChunkSaveError when the open or a write fails.
pub fn save(path: &Path, bytes: &[u8]) -> Result<(), ChunkSaveError> {
    #[cfg(unix)]
    let result = native::save_chunk(&path::c_string(path.as_os_str()), bytes);
    #[cfg(windows)]
    let result = native::save_chunk(path, bytes);
    match result {
        native::ChunkWrite::Saved => Ok(()),
        native::ChunkWrite::Open(error) | native::ChunkWrite::Payload(error) => {
            Err(ChunkSaveError::Io(error))
        }
        native::ChunkWrite::Header(field, source) => Err(ChunkSaveError::Header {
            field: match field {
                native::HeaderField::Length => ChunkHeaderField::Length,
                native::HeaderField::Checksum => ChunkHeaderField::Checksum,
            },
            source,
        }),
    }
}

#[cfg(test)]
mod tests;
