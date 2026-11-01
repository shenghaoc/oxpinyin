//! MemoryChunk::save: the pin's open, two header writes, payload write,
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
use crate::path;

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
    match native::save_chunk(&path::c_string(path.as_os_str()), bytes) {
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
