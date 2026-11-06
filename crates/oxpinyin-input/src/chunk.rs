//! MemoryChunk::mmap and ::save: owned private mappings and the pin's
//! open, two header writes, payload write,
//! g_fsync and close (074a2219 include/memory_chunk.h:536-557).
//!
//! The calls are the pin's: open(filename, O_CREAT|O_WRONLY|O_TRUNC|O_BINARY,
//! 0644), one write per header word, one write for the payload,
//! g_fsync(fd), then close(fd). A short header write is the pin's live
//! assert (class (c): the caller refuses instead of aborting); a short or
//! failed payload write is the pin's soft false, with no sync and no close
//! result check.

use std::io;
use std::path::Path;

use crate::native;
#[cfg(unix)]
use crate::path;

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
    /// The pin's assert expression at this write, for the facade's
    /// class-(c) warning.
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
mod tests {
    use super::*;

    // Pin memory_chunk.h:424-438: reject a short header before reading either word.
    #[test]
    fn header_size_and_read_failure_follow_pin_load() {
        let calls = std::cell::Cell::new(0);
        let read = || {
            calls.set(calls.get() + 1);
            Err(io::ErrorKind::Other.into())
        };
        assert!(matches!(
            load_header(7, read),
            Err(ChunkLoadError::ShortHeader)
        ));
        assert_eq!(calls.get(), 0);
        assert!(matches!(
            load_header(8, read),
            Err(ChunkLoadError::Header(_))
        ));
        assert_eq!(calls.get(), 1);
        let mut reads = 0;
        assert!(matches!(
            load_header(8, || {
                reads += 1;
                if reads == 1 {
                    Ok([0; 4])
                } else {
                    Err(io::ErrorKind::Other.into())
                }
            }),
            Err(ChunkLoadError::Header(_))
        ));
        assert_eq!(reads, 2);
        assert!(load_header(8, || Ok([0; 4])).is_ok());
    }

    #[test]
    fn short_payload_is_checksummed_at_the_returned_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chunk");
        // The checksum names the single returned byte, not the whole payload.
        std::fs::write(&path, [4, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 4]).unwrap();
        let calls = std::cell::Cell::new(0);
        let chunk = load_with(&path, |source, buffer| {
            calls.set(calls.get() + 1);
            source.read_payload(buffer, Some(1))
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(chunk.length, 4_u32.to_le_bytes());
        assert_eq!(chunk.finish(|bytes| u32::from(bytes[0])), Some(vec![1]));

        // The same short result against a different checksum is a refusal.
        std::fs::write(&path, [4, 0, 0, 0, 2, 0, 0, 0, 1, 2, 3, 4]).unwrap();
        let chunk =
            load_with(&path, |source, buffer| source.read_payload(buffer, Some(1))).unwrap();
        assert_eq!(chunk.finish(|bytes| u32::from(bytes[0])), None);
    }

    #[test]
    fn interrupted_payload_read_is_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chunk");
        std::fs::write(&path, [4, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 4]).unwrap();
        let calls = std::cell::Cell::new(0);
        let error = match load_with(&path, |_, _| {
            calls.set(calls.get() + 1);
            Err(io::ErrorKind::Interrupted.into())
        }) {
            Err(ChunkLoadError::Io(error)) => error,
            _ => panic!("interrupted payload must fail"),
        };
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn file_size_is_narrowed_before_length_comparison() {
        let declared = 4_u32;
        assert_eq!(available_payload_length(8 + 4), declared);
        assert_eq!(available_payload_length(8 + 4 + (1_u64 << 32)), declared);
        assert_ne!(available_payload_length(8 + 5 + (1_u64 << 32)), declared);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chunk");
        std::fs::write(&path, [4, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5]).unwrap();
        let called = std::cell::Cell::new(false);
        assert!(matches!(
            load_with(&path, |_, _| {
                called.set(true);
                Ok(0)
            }),
            Err(ChunkLoadError::Io(_))
        ));
        assert!(!called.get(), "framing fails before reading the payload");
    }

    #[cfg(windows)]
    #[test]
    fn mapping_opens_a_non_ascii_windows_path() {
        let root = tempfile::tempdir().unwrap();
        let dir = crate::path::build_chunk_path(root.path(), std::ffi::OsStr::new("目录"));
        std::fs::create_dir(&dir).unwrap();
        let path = crate::path::build_chunk_path(&dir, std::ffi::OsStr::new("词库.bin"));
        save(&path, &[1_u8, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let mut source = MappingSource::open(&path).unwrap();
        assert_eq!(source.file_size(), 8);
        assert_eq!(source.read_word().unwrap(), [1, 2, 3, 4]);
        assert_eq!(source.read_word().unwrap(), [5, 6, 7, 8]);
        assert_eq!(
            source.map().unwrap().finish().as_slice(),
            &[1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn one_mapping_owns_all_file_bytes_and_keeps_writes_private() {
        let dir = tempfile::tempdir().unwrap();
        for len in [1_usize, 7, 8, 4095, 4096, 4097, 3 * 4096 + 1] {
            let path = dir.path().join(format!("len-{len}"));
            let bytes: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            std::fs::write(&path, &bytes).unwrap();
            let source = MappingSource::open(&path).unwrap();
            assert_eq!(source.file_size(), len as u64);
            let pending = source.map().unwrap();
            assert_eq!(pending.as_slice(), bytes, "{len}");
            let mut mapping = pending.finish();
            #[cfg(target_os = "linux")]
            {
                let ranges = file_mappings(&path);
                assert_eq!(ranges.len(), 1);
                assert_eq!(ranges[0].split_whitespace().nth(1), Some("rw-p"));
            }
            mapping.as_mut_slice()[0] = 255;
            assert_eq!(mapping.as_slice()[0], 255);
            assert_eq!(&mapping.as_slice()[1..], &bytes[1..]);
            assert_eq!(
                std::fs::read(&path).unwrap(),
                bytes,
                "private writes stay private"
            );
        }
    }

    #[cfg(target_os = "linux")]
    fn open_descriptors(path: &Path) -> usize {
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| std::fs::read_link(entry.path()).is_ok_and(|target| target == path))
            .count()
    }

    #[cfg(target_os = "linux")]
    fn file_mappings(path: &Path) -> Vec<String> {
        let suffix = format!(" {}", path.display());
        std::fs::read_to_string("/proc/self/maps")
            .unwrap()
            .lines()
            .filter(|line| line.ends_with(&suffix))
            .map(str::to_owned)
            .collect()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn validation_owns_the_descriptor_and_finish_closes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("descriptor");
        std::fs::write(&path, [0_u8; 8]).unwrap();
        let source = MappingSource::open(&path).unwrap();
        assert_eq!(open_descriptors(&path), 1);
        let pending = source.map().unwrap();
        assert_eq!(open_descriptors(&path), 1);
        assert_eq!(file_mappings(&path).len(), 1);
        let mapping = pending.finish();
        assert_eq!(open_descriptors(&path), 0);
        assert_eq!(file_mappings(&path).len(), 1);
        drop(mapping);
        assert!(file_mappings(&path).is_empty());

        let rejected = MappingSource::open(&path).unwrap().map().unwrap();
        assert_eq!(open_descriptors(&path), 1);
        drop(rejected);
        assert_eq!(open_descriptors(&path), 0);
        assert!(file_mappings(&path).is_empty());
    }

    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    #[test]
    fn sparse_file_has_one_whole_writable_private_mapping_and_no_residual() {
        // Row 109's source-derived witness: the pin's u32 data length wraps
        // to zero, but mmap still receives the full 2^32 + 8 file size.
        // Only sparse file length and virtual extent grow; no payload is read.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sparse");
        let file_size = (1_u64 << 32) + 8;
        std::fs::File::create(&path)
            .unwrap()
            .set_len(file_size)
            .unwrap();
        let mut source = MappingSource::open(&path).unwrap();
        assert_eq!(source.file_size(), file_size);
        assert_eq!(source.read_word().unwrap(), [0; 4]);
        assert_eq!(source.read_word().unwrap(), [0; 4]);
        let mapping = source.map().unwrap().finish();
        assert_eq!(mapping.as_slice().len() as u64, file_size);
        let ranges = file_mappings(&path);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].split_whitespace().nth(1), Some("rw-p"));
        assert_eq!(open_descriptors(&path), 0);
        drop(mapping);
        assert!(file_mappings(&path).is_empty());
    }

    #[test]
    fn header_reads_are_separate_words_and_short_reads_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("header");
        std::fs::write(&path, [1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();
        let mut source = MappingSource::open(&path).unwrap();
        assert_eq!(source.read_word().unwrap(), [1, 2, 3, 4]);
        assert_eq!(source.read_word().unwrap(), [5, 6, 7, 8]);
        assert_eq!(
            source.read_word().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    fn chunk(len: usize) -> Vec<u8> {
        let mut bytes = vec![0_u8; 8 + len];
        bytes[..4].copy_from_slice(&(len as u32).to_ne_bytes());
        for (at, byte) in bytes[8..].iter_mut().enumerate() {
            *byte = at as u8;
        }
        bytes
    }

    #[test]
    fn a_chunk_is_the_written_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chunk");
        let bytes = chunk(3);
        save(&path, &bytes).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn an_unopenable_path_is_the_soft_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("chunk");
        assert!(matches!(save(&path, &chunk(0)), Err(ChunkSaveError::Io(_))));
    }

    #[cfg(windows)]
    #[test]
    fn saving_over_a_read_only_file_is_permission_denied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("readonly.bin");
        std::fs::write(&path, chunk(0)).unwrap();
        let original = std::fs::metadata(&path).unwrap().permissions();
        let mut permissions = original.clone();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let result = save(&path, &chunk(1));
        std::fs::set_permissions(&path, original).unwrap();
        let Err(ChunkSaveError::Io(error)) = result else {
            panic!("expected an I/O error")
        };
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(error.raw_os_error(), Some(5));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_header_write_failure_is_the_abort_site() {
        // /dev/full opens for writing and refuses every write, which is the
        // pin's first header `assert` (memory_chunk.h:543).
        assert!(matches!(
            save(Path::new("/dev/full"), &chunk(0)),
            Err(ChunkSaveError::Header {
                field: ChunkHeaderField::Length,
                ..
            })
        ));
    }
}
