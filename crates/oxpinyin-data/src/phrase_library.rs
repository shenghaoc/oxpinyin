//! Reader for the per-library phrase-index files (the `MemoryChunk` +
//! `SubPhraseIndex` pair).
//!
//! P1 of the data-layer rework
//! (`docs/findings/libpinyin-system-data-formats-2026-09-01.md`): these
//! are the native library files on every backend (`gb_char.bin`,
//! `gbk_char.bin`, `opengram.bin`, `merged.bin`, and the addon `*.bin`
//! libraries), the same ones a libpinyin build ships — no conversion.
//! It is the Rust equivalent of the pinned upstream reader:
//!
//! - `src/include/memory_chunk.h` — `MemoryChunk::mmap`: an 8-byte
//!   header `{length: u32, checksum: u32}` then the payload; `mmap`
//!   verifies `length == (file_size − 8) as guint32` (the truncation at
//!   `:499`) and recomputes the checksum over exactly that many payload
//!   bytes before handing out a pointer. The chunk it exposes is
//!   `header + data_len`, not the whole file when the two differ.
//! - `src/storage/phrase_index.cpp` — `SubPhraseIndex::load`: the
//!   payload is `[total_freq: u32][index_one, index_two, index_three:
//!   u32×3]['#']`, an offset array at `index_one` (one `u32` per
//!   `token & PHRASE_MASK` slot, `0` = no item), the entry area at
//!   `index_two`, both closed off by `'#'` separators
//!   (`novel_types.h:126`), `index_three == payload length`.
//! - `SubPhraseIndex::get_phrase_item`: slot offset → an item
//!   `{u8 phrase_length, u8 n_pronunciations, u32 unigram,
//!   ucs4_t phrase[L], {ChewingKey[L], u32 freq} × n_pronunciations}`
//!   — `6 + 4·L + n·(2·L + 4)` bytes (`phrase_item_header`,
//!   `phrase_index.h:56`; `sizeof(ChewingKey) == 2`, verified against
//!   the pinned headers).
//! - `SubPhraseIndex::get_range`: tokens occupy `1 .. range_end`
//!   where `range_end` is the offset-array length with trailing zero
//!   slots trimmed; an empty library answers `1..1`.
//!
//! Everything is little-endian on the supported targets (the fields
//! are host-endian upstream; every target oxpinyin and the pin share
//! is little-endian).
//!
//! Robustness: a malformed or truncated file never panics and never
//! reads out of bounds. The header, checksum, separators, and section
//! bounds are validated once at open; every item access re-checks its
//! own arithmetic and decodes its UCS-4 before handing anything out,
//! degrading to `None` where upstream would answer `ERROR_NO_ITEM` /
//! `ERROR_FILE_CORRUPTION`.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[cfg(not(target_endian = "little"))]
compile_error!(
    "phrase_library: the MemoryChunk format uses host-endian fields; \
     this reader decodes them as little-endian. Big-endian targets are \
     not supported."
);

use crate::chunk_format::{
    CHEWING_KEY_SIZE, CHUNK_HEADER_SIZE, ITEM_HEADER_SIZE, PHRASE_MASK, SEPARATOR, chunk_checksum,
};
use crate::sparse_bytes::Span;

/// Bytes of stack the dictionary-hit path gives a phrase's UTF-8 before
/// it falls back to the heap ([`PhraseItemView::phrase_text_into`]):
/// eight characters of any width, or ten three-byte CJK characters. The
/// typical hit is one to four CJK characters
/// (`docs/perf/perf-alloc-2026-08.md`); the entry type the text lands in
/// keeps up to 24 bytes inline, so a text that fits here but not there
/// costs the one allocation a longer text costs anyway.
pub(crate) const PHRASE_TEXT_STACK_BYTES: usize = 32;

/// The pin's `G_STRFUNC` for `SubPhraseIndex::load`: the C++
/// `__PRETTY_FUNCTION__` the measured critical carries (the pin logs it in
/// domain NULL at level `G_LOG_LEVEL_CRITICAL`).
pub const SUB_PHRASE_LOAD_SIGNATURE: &str =
    "bool pinyin::SubPhraseIndex::load(pinyin::MemoryChunk*, table_offset_t, table_offset_t)";

/// A `SubPhraseIndex::load` check the pin reaches through a read inside the
/// chunk and reports by `g_return_val_if_fail` (`phrase_index.cpp:354-356`),
/// which every caller ignores. The load then answers success with an empty
/// sub-index. Only checks whose bytes are inside the chunk are represented
/// here; a layout the pin reaches through an out-of-bounds read stays a
/// [`LibraryError::Format`] refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutFault {
    /// The failed assertion's expression, as the pin's `#expr` spells it.
    pub expression: &'static str,
    /// `m_total_freq` as the pin read it before the failing check — 0 when
    /// the four-word header itself is short.
    pub total_freq: u32,
}

impl LayoutFault {
    /// The pin's critical message, as `g_return_if_fail_warning` formats it:
    /// `G_STRFUNC: assertion 'expr' failed`.
    #[must_use]
    pub fn critical_message(&self) -> String {
        format!(
            "{SUB_PHRASE_LOAD_SIGNATURE}: assertion '{}' failed",
            self.expression
        )
    }
}

/// Why a phrase library could not be opened.
#[derive(Debug)]
pub enum LibraryError {
    /// The file could not be read or mapped.
    Io(std::io::Error),
    /// The chunk header, checksum, or sub-index layout is not what the
    /// upstream reader would accept.
    Format(String),
    /// The chunk header and checksum verified, but `SubPhraseIndex::load`
    /// refused the layout through a check the pin reports with a critical
    /// and ignores. The caller loads an empty sub-index and reports the
    /// success the pin reports.
    Layout(LayoutFault),
    /// Upstream's `MemoryChunk::mmap` answers `false` for `path`: the file
    /// cannot be opened or mapped, is shorter than the chunk header, its
    /// length word is not the (truncated) payload length, or its checksum
    /// does not verify. The pin then writes `mmap %s failed!`
    /// (`pinyin.cpp:256`) and goes on with an empty chunk. A layout fault
    /// past the chunk (`SubPhraseIndex::load`'s checks) writes no such line:
    /// a check reached inside the chunk is [`LibraryError::Layout`], one
    /// the pin reaches through an out-of-bounds read is
    /// [`LibraryError::Format`]. `cause` is the [`LibraryError::Io`] or
    /// [`LibraryError::Format`] that says which.
    Unmappable {
        /// The file, as the caller named it.
        path: PathBuf,
        /// Why the chunk did not map.
        cause: Box<LibraryError>,
    },
    /// The library's `table.conf` row is one the pin `assert`s on at this
    /// call (`pinyin.cpp:491`, `memory_chunk.h:493`).
    Row(RowFault),
}

/// What is wrong with a library's `table.conf` row, where the pin's load
/// path dies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowFault {
    /// `assert(DICTIONARY == table_info->m_file_type)`
    /// (`pinyin.cpp:491`): the addon row is neither `DICTIONARY` nor
    /// `NOT_USED`.
    NotDictionary,
    /// The row names no system chunk (`NULL`), so the path is the
    /// directory itself and `MemoryChunk::mmap` asserts on reading it
    /// (`memory_chunk.h:493`).
    NullChunkName,
    /// The path is a directory whose `lseek(SEEK_END)` size is a header or
    /// more: the header check passes, `read` fails with `EISDIR`, and
    /// `MemoryChunk::mmap` asserts on the length word
    /// (`memory_chunk.h:493`). Which pin site a directory reaches follows
    /// the size the file system reports for it; a smaller one is the
    /// ordinary [`LibraryError::Unmappable`].
    ChunkHeaderRead,
}

impl LibraryError {
    /// The file `MemoryChunk::mmap` refused, when this error is that
    /// refusal; `None` for any other fault.
    #[must_use]
    pub fn unmappable_path(&self) -> Option<&Path> {
        match self {
            Self::Unmappable { path, .. } => Some(path),
            Self::Io(_) | Self::Format(_) | Self::Layout(_) | Self::Row(_) => None,
        }
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "phrase library I/O error: {e}"),
            Self::Format(message) => write!(f, "phrase library format error: {message}"),
            Self::Layout(fault) => write!(f, "phrase library layout error: {}", fault.expression),
            Self::Unmappable { cause, .. } => cause.fmt(f),
            Self::Row(RowFault::NotDictionary) => {
                f.write_str("addon table.conf row is not a DICTIONARY")
            }
            Self::Row(RowFault::NullChunkName) => f.write_str("table.conf row names no chunk file"),
            Self::Row(RowFault::ChunkHeaderRead) => {
                f.write_str("chunk path is a directory the header cannot be read from")
            }
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Format(_) | Self::Layout(_) | Self::Row(_) => None,
            Self::Unmappable { cause, .. } => Some(cause.as_ref()),
        }
    }
}

impl From<std::io::Error> for LibraryError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Reads a little-endian `u32` at `offset`, or `None` past the end.
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

// ── mapped file ─────────────────────────────────────────────────────

/// File mapping: the one place in this crate (and the shipping tree)
/// with `unsafe`, under the constitution's documented mmap exception.
mod map {
    #![expect(
        unsafe_code,
        reason = "the constitution's documented mmap exception; see the module docs"
    )]

    use super::LibraryError;
    use crate::sparse_bytes::{Ascending, SparseBytes};

    /// The chunk extent as one slice on all targets, mapped once.
    ///
    /// Before any other handle shares it, an in-place journal replay may
    /// write into the same private mapping, as the pin's merge writes into
    /// its own (`memory_chunk.h:505-506`, `phrase_index.cpp:497-498` at
    /// 074a2219): the first write that changes a byte makes the mapping
    /// writable (`make_mut`), and [`MappedFile::seal`] makes it read-only
    /// again (`make_read_only`). While the replay writes, a read-only view of
    /// the chunk as loaded stands beside the mapping; the seal keeps the
    /// original value of every byte the replay changed and unmaps that view,
    /// so the chunk still reads as loaded through one mapping.
    pub struct MappedFile {
        bytes: Bytes,
        file: Option<std::fs::File>,
        /// The chunk as loaded, mapped read-only from the first changing
        /// write to the seal: what the replay reads as loaded, and what the
        /// seal compares the replayed bytes against.
        loaded: Option<memmap2::Mmap>,
        /// One bit per [`WRITTEN_PAGE`] bytes, set where a replay wrote since
        /// the mapping was made writable.
        written: Vec<u64>,
        /// The original value of each byte a sealed replay changed.
        original: SparseBytes,
        /// Fail the next `make_read_only`, as a refused `mprotect` would.
        #[cfg(test)]
        pub(super) fail_seal: bool,
    }

    /// The bytes one bit of [`MappedFile::written`] stands for.
    const WRITTEN_PAGE: usize = 4096;

    /// Each variant owns its bytes. All are `Send + Sync`.
    enum Bytes {
        /// A private mapping of the chunk extent at offset zero.
        Mapped(memmap2::Mmap),
        /// The same mapping, writable for an in-place replay.
        Writable(memmap2::MmapMut),
        /// Empty bytes for a zero-length file or a refused sub-index.
        Heap(Box<[u8]>),
    }

    /// The first `len` bytes of `file` as a private read-only mapping.
    fn map_read_only(file: &std::fs::File, len: usize) -> std::io::Result<memmap2::Mmap> {
        // SAFETY: The file is open. The offset is zero. The explicit
        // length is the chunk extent checked by map_chunk. Mmap owns
        // the mapping. Each returned slice borrows that owner. The only
        // writes through the mapping are an in-place journal replay's,
        // made through memmap2's make_mut while one handle owns the
        // MappedFile, before it is shared; make_read_only follows them.
        // On Unix the mapping is MAP_PRIVATE, so those writes are
        // copy-on-write pages of this process and never reach the file.
        // Do not change a table file while it is mapped. These are system
        // and addon libraries. This runtime does not write them.
        // On Unix, map_copy_read_only uses PROT_READ and MAP_PRIVATE.
        // A private mapping is not a snapshot. An in-place file write
        // can change its bytes. Truncation can cause SIGBUS on a later
        // read. A rename replacement leaves the view on the old file.
        // On Windows, memmap2 uses FILE_MAP_COPY with PAGE_WRITECOPY
        // or PAGE_EXECUTE_WRITECOPY. It can then reduce page access
        // with VirtualProtect; make_mut restores PAGE_WRITECOPY, so a
        // replay write is copy-on-write there too. On Windows CI,
        // std::fs::write fails with error 1224 when it tries to
        // truncate a mapped file for an in-place rewrite. Deletion
        // through std::fs::remove_file succeeds in the addon test.
        // Replacement by rename is not tested on Windows. These
        // checks do not test a write without truncation on Windows.
        // The truncation/SIGBUS case is Unix only.
        // The pin maps these files on targets with mmap. Without mmap,
        // the pin reads the file into the heap (memory_chunk.h:31-33,
        // :446-452; CMakeLists.txt:64 at 074a2219). Thus, on Windows
        // this is an internal storage change. The returned bytes are
        // the same. The mapping keeps the file alive after this File
        // is closed.
        unsafe {
            memmap2::MmapOptions::new()
                .len(len)
                .map_copy_read_only(file)
        }
    }

    /// Eight bytes as a little-endian word.
    fn word(bytes: &[u8]) -> u64 {
        u64::from_le_bytes(bytes.try_into().unwrap_or_default())
    }

    /// The first and last indices where `current` and `new` differ, eight
    /// bytes at a time: the first and last eight bytes, which may overlap,
    /// then the words between them from each end.
    fn changed_span(current: &[u8], new: &[u8]) -> Option<(usize, usize)> {
        let len = current.len().min(new.len());
        if len < 8 {
            let first = (0..len).find(|&at| current[at] != new[at])?;
            let last = (0..len).rfind(|&at| current[at] != new[at])?;
            return Some((first, last));
        }
        let differ = |at: usize| word(&current[at..at + 8]) ^ word(&new[at..at + 8]);
        let (head, tail) = (differ(0), differ(len - 8));
        let first = if head != 0 {
            head.trailing_zeros() as usize / 8
        } else {
            let mut at = 8;
            loop {
                if at + 8 > len {
                    if tail == 0 {
                        return None;
                    }
                    break len - 8 + tail.trailing_zeros() as usize / 8;
                }
                let differ = differ(at);
                if differ != 0 {
                    break at + differ.trailing_zeros() as usize / 8;
                }
                at += 8;
            }
        };
        let last = if tail != 0 {
            len - 1 - tail.leading_zeros() as usize / 8
        } else {
            // A difference lies before the last eight bytes, at or after
            // `first`: the words from there back find the last of them.
            let mut at = len - 8;
            loop {
                at = at.saturating_sub(8).max(first);
                let differ = differ(at);
                if differ != 0 {
                    break at + 7 - differ.leading_zeros() as usize / 8;
                }
                if at == first {
                    break first;
                }
            }
        };
        Some((first, last))
    }

    /// Whether every page holding positions `first..=last` is marked written.
    #[inline]
    fn all_written(written: &[u64], first: usize, last: usize) -> bool {
        (first / WRITTEN_PAGE..=last / WRITTEN_PAGE).all(|page| {
            written
                .get(page / 64)
                .is_some_and(|bits| bits >> (page % 64) & 1 == 1)
        })
    }

    /// Marks the pages holding positions `first..=last` written.
    #[inline]
    fn mark_written(written: &mut [u64], first: usize, last: usize) {
        for page in first / WRITTEN_PAGE..=last / WRITTEN_PAGE {
            if let Some(bits) = written.get_mut(page / 64) {
                *bits |= 1 << (page % 64);
            }
        }
    }

    /// The loaded value of each byte the replay changed: `current` against
    /// `loaded` over the pages `written` marks, a page at a time in
    /// ascending order.
    fn changes(current: &[u8], loaded: &[u8], written: &[u64]) -> SparseBytes {
        // At most one page record per written page, and no spare capacity
        // while the journal is still in memory.
        let pages = written.iter().map(|bits| bits.count_ones() as usize).sum();
        let end = written
            .iter()
            .rposition(|&bits| bits != 0)
            .map_or(0, |word| {
                word * 64 + 64 - written[word].leading_zeros() as usize
            });
        let mut original = Ascending::with_pages(pages, end);
        for (index, &bits) in written.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                let page = index * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let start = page * WRITTEN_PAGE;
                let end = start.saturating_add(WRITTEN_PAGE).min(current.len());
                if let (Some(now), Some(then)) = (current.get(start..end), loaded.get(start..end)) {
                    original.push_changes(start, now, then);
                }
            }
        }
        original.finish()
    }

    impl MappedFile {
        /// No bytes and no mapping: the payload of a chunk whose header was
        /// accepted but whose sub-index layout the pin refused.
        pub(crate) fn empty() -> Self {
            Self::with_bytes(Bytes::Heap(Box::new([])), None)
        }

        fn with_bytes(bytes: Bytes, file: Option<std::fs::File>) -> Self {
            Self {
                bytes,
                file,
                loaded: None,
                written: Vec::new(),
                original: SparseBytes::new(),
                #[cfg(test)]
                fail_seal: false,
            }
        }

        /// Map the first `len` bytes of `file`.
        /// This is the header plus data length used by `MemoryChunk::mmap`.
        /// Do not map the rest of the file if the header has a shorter length.
        pub(crate) fn open_len(file: std::fs::File, len: usize) -> Result<Self, LibraryError> {
            if len == 0 {
                // Keep an empty heap slice. Do not map a zero-length file.
                return Ok(Self::empty());
            }
            let mapped = map_read_only(&file, len).map_err(LibraryError::Io)?;
            Ok(Self::with_bytes(Bytes::Mapped(mapped), Some(file)))
        }

        /// The mapped bytes, as replayed. The slice borrows `self`, so the
        /// mapping outlives every view taken over it.
        #[inline]
        pub(crate) fn as_slice(&self) -> &[u8] {
            match &self.bytes {
                Bytes::Mapped(mapped) => mapped,
                Bytes::Writable(mapped) => mapped,
                Bytes::Heap(heap) => heap,
            }
        }

        /// The chunk as loaded in one slice, where there is one: the view
        /// kept while a replay writes, or the mapping no replay changed.
        /// `None` once a sealed replay changed bytes: [`Self::original`]
        /// then holds their loaded values.
        #[inline]
        pub(crate) fn loaded(&self) -> Option<&[u8]> {
            match &self.loaded {
                Some(loaded) => Some(loaded),
                None => self.original.is_empty().then(|| self.as_slice()),
            }
        }

        /// The original value of each byte a sealed replay changed.
        pub(crate) const fn original(&self) -> &SparseBytes {
            &self.original
        }

        /// Whether a replay has made the mapping writable and not sealed it.
        #[cfg(test)]
        pub(crate) const fn is_writable(&self) -> bool {
            matches!(self.bytes, Bytes::Writable(_))
        }

        /// Writes `bytes` at `at`. Writes nothing, and maps nothing writable,
        /// when no byte changes. The caller checks the bounds.
        ///
        /// # Errors
        ///
        /// The mapping could not be made writable; it is left as it was.
        #[inline]
        pub(crate) fn write(&mut self, at: usize, bytes: &[u8]) -> Result<(), LibraryError> {
            let end = at.saturating_add(bytes.len());
            if let Bytes::Writable(mapped) = &mut self.bytes
                && end > at
                && all_written(&self.written, at, end - 1)
            {
                // Every page under the bytes holds a change already, so it
                // is this process's copy: write them as they stand.
                if let Some(target) = mapped.get_mut(at..end) {
                    target.copy_from_slice(bytes);
                }
                return Ok(());
            }
            if !matches!(self.bytes, Bytes::Writable(_)) {
                let changes = self
                    .as_slice()
                    .get(at..end)
                    .is_some_and(|current| changed_span(current, bytes).is_some());
                if !changes {
                    return Ok(());
                }
                self.promote()?;
            }
            let Bytes::Writable(mapped) = &mut self.bytes else {
                return Err(LibraryError::Io(std::io::Error::other("not writable")));
            };
            let Some(current) = mapped.get_mut(at..end) else {
                return Ok(());
            };
            if let Some((first, last)) = changed_span(current, bytes) {
                current[first..=last].copy_from_slice(&bytes[first..=last]);
                mark_written(&mut self.written, at + first, at + last);
            }
            Ok(())
        }

        /// Makes the mapping writable: `make_mut`, the pin's
        /// `PROT_READ|PROT_WRITE` on the same private mapping. A view of the
        /// chunk as loaded is mapped beside it until the seal.
        #[cold]
        #[inline(never)]
        fn promote(&mut self) -> Result<(), LibraryError> {
            match self.bytes {
                Bytes::Writable(_) => return Ok(()),
                Bytes::Heap(_) => {
                    return Err(LibraryError::Io(std::io::Error::other("no mapping")));
                }
                Bytes::Mapped(_) if !self.original.is_empty() => {
                    // A failed `make_mut` would take a sealed replay's
                    // bytes with it: such a library is not written again.
                    return Err(LibraryError::Io(std::io::Error::other(
                        "a sealed replay's mapping is not written again",
                    )));
                }
                Bytes::Mapped(_) => {}
            }
            // `make_mut` takes the mapping and drops it when `mprotect`
            // fails: the view of the chunk as loaded then stands in.
            let loaded = self.view()?;
            match std::mem::replace(&mut self.bytes, Bytes::Heap(Box::new([]))) {
                Bytes::Mapped(mapped) => match mapped.make_mut() {
                    Ok(writable) => {
                        self.written = vec![0; writable.len().div_ceil(WRITTEN_PAGE).div_ceil(64)];
                        self.bytes = Bytes::Writable(writable);
                        self.loaded = Some(loaded);
                        Ok(())
                    }
                    Err(error) => {
                        self.bytes = Bytes::Mapped(loaded);
                        Err(LibraryError::Io(error))
                    }
                },
                other => {
                    self.bytes = other;
                    Ok(())
                }
            }
        }

        /// Ends a replay: keeps the loaded value of each byte it changed,
        /// makes the mapping read-only again (`make_read_only`) and unmaps
        /// the view of the chunk as loaded.
        ///
        /// # Errors
        ///
        /// `make_read_only` failed and took the replayed mapping with it:
        /// the chunk is mapped again as it was loaded, with no records, and
        /// the caller replays its journal another way.
        pub(crate) fn seal(&mut self) -> Result<(), LibraryError> {
            let Some(loaded) = self.loaded.take() else {
                return Ok(());
            };
            let written = std::mem::take(&mut self.written);
            match std::mem::replace(&mut self.bytes, Bytes::Heap(Box::new([]))) {
                Bytes::Writable(mapped) => {
                    self.original = changes(&mapped, &loaded, &written);
                    #[cfg(test)]
                    let sealed = if std::mem::take(&mut self.fail_seal) {
                        drop(mapped);
                        Err(std::io::Error::other("injected mprotect failure"))
                    } else {
                        mapped.make_read_only()
                    };
                    #[cfg(not(test))]
                    let sealed = mapped.make_read_only();
                    match sealed {
                        Ok(read_only) => {
                            self.bytes = Bytes::Mapped(read_only);
                            Ok(())
                        }
                        Err(error) => {
                            self.bytes = Bytes::Mapped(loaded);
                            self.original = SparseBytes::new();
                            Err(LibraryError::Io(error))
                        }
                    }
                }
                other => {
                    self.bytes = other;
                    Ok(())
                }
            }
        }

        /// A second read-only view of the same file and extent.
        fn view(&self) -> Result<memmap2::Mmap, LibraryError> {
            let file = self
                .file
                .as_ref()
                .ok_or_else(|| LibraryError::Io(std::io::Error::other("no mapped file")))?;
            map_read_only(file, self.as_slice().len()).map_err(LibraryError::Io)
        }
    }

    // `PhraseLibrary` shares one mapping across threads through an `Arc`;
    // keep that a compile error to lose, now that the bounds are inferred.
    const _: () = {
        const fn send_sync<T: Send + Sync>() {}
        send_sync::<MappedFile>();
    };

    #[cfg(test)]
    mod tests {
        use super::MappedFile;

        /// The mapping hands out exactly the file's bytes: empty, one byte,
        /// and lengths on and across page boundaries.
        #[test]
        fn maps_exactly_the_file_bytes() {
            let dir = tempfile::Builder::new()
                .prefix("oxpinyin-mapped-file-")
                .tempdir()
                .expect("tmpdir");
            for len in [0_usize, 1, 7, 8, 4095, 4096, 4097, 3 * 4096 + 1] {
                let path = dir.path().join(format!("len-{len}"));
                let bytes: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
                std::fs::write(&path, &bytes).unwrap();
                let mapped =
                    MappedFile::open_len(std::fs::File::open(&path).unwrap(), len).unwrap();
                assert_eq!(mapped.as_slice(), std::fs::read(&path).unwrap(), "{len}");
                // Reopening while the first mapping lives gives an
                // independent mapping of the same bytes.
                let again = MappedFile::open_len(std::fs::File::open(&path).unwrap(), len).unwrap();
                assert_eq!(again.as_slice(), mapped.as_slice(), "{len}");
            }
        }

        fn mapped(len: usize) -> (tempfile::TempDir, std::path::PathBuf, MappedFile) {
            let dir = tempfile::tempdir().expect("tmpdir");
            let path = dir.path().join("chunk");
            let bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            std::fs::write(&path, &bytes).unwrap();
            let file = MappedFile::open_len(std::fs::File::open(&path).unwrap(), len).unwrap();
            (dir, path, file)
        }

        /// A write lands in the mapping, never in the file. While the replay
        /// writes, the chunk reads as loaded from a view beside the mapping;
        /// the seal keeps the loaded value of each changed byte and unmaps
        /// that view. An unchanging write maps nothing writable.
        #[test]
        fn writes_are_private_and_keep_their_originals() {
            let (_dir, path, mut file) = mapped(3 * 4096 + 5);
            let on_disk = std::fs::read(&path).unwrap();
            let same = file.as_slice()[100..110].to_vec();
            file.write(100, &same).unwrap();
            assert!(!file.is_writable(), "an unchanging write promotes nothing");
            assert_eq!(file.loaded(), Some(&on_disk[..]));
            // Two writes on one page, one across a page boundary, one in the
            // last partial word, and one that puts a byte back.
            let mut replayed = on_disk.clone();
            let mut write = |file: &mut MappedFile, at: usize, bytes: &[u8]| {
                file.write(at, bytes).unwrap();
                replayed[at..at + bytes.len()].copy_from_slice(bytes);
            };
            write(&mut file, 4090, &[0xEE; 12]);
            assert!(file.is_writable());
            write(&mut file, 4000, &[1, 2, 3]);
            write(&mut file, 3 * 4096 + 1, &[0xAB, 0xCD, 0xEF, 0x01]);
            write(&mut file, 4095, &[on_disk[4095]]);
            assert_eq!(file.as_slice(), replayed);
            assert_eq!(file.loaded(), Some(&on_disk[..]));
            assert_eq!(std::fs::read(&path).unwrap(), on_disk);
            file.seal().unwrap();
            assert!(!file.is_writable());
            assert_eq!(file.as_slice(), replayed);
            assert_eq!(file.loaded(), None);
            let mut original = file.as_slice().to_vec();
            file.original().overlay(0, &mut original);
            assert_eq!(original, on_disk);
            let kept = (0..on_disk.len())
                .filter(|&at| file.original().get(at).is_some())
                .count();
            let changed = (0..on_disk.len())
                .filter(|&at| on_disk[at] != replayed[at])
                .count();
            assert_eq!(kept, changed, "only the changed bytes are kept");
            assert!(
                file.write(0, &[!on_disk[0]]).is_err(),
                "a sealed replay's mapping is not written again"
            );
        }

        /// A refused `make_read_only` leaves the chunk mapped as loaded,
        /// with no records, and says so.
        #[test]
        fn a_failed_seal_maps_the_chunk_again_as_loaded() {
            let (_dir, path, mut file) = mapped(2 * 4096);
            file.write(10, &[0xEE; 4]).unwrap();
            file.fail_seal = true;
            assert!(file.seal().is_err());
            assert!(!file.is_writable());
            assert_eq!(file.as_slice(), std::fs::read(&path).unwrap());
            assert!(file.original().is_empty());
            assert_eq!(file.loaded(), Some(file.as_slice()));
        }

        /// The changed span of two equal-length byte runs, against a plain
        /// scan, for every placement of one or two differences.
        #[test]
        fn the_changed_span_is_the_first_and_last_difference() {
            for len in [0_usize, 1, 7, 8, 9, 12, 16, 17, 23, 30, 40, 65] {
                let current: Vec<u8> = (0..len).map(|i| i as u8).collect();
                assert_eq!(super::changed_span(&current, &current), None, "{len}");
                for first in 0..len {
                    for last in first..len {
                        let mut new = current.clone();
                        new[first] ^= 0x80;
                        new[last] ^= 0x01;
                        let want = (0..len).find(|&i| new[i] != current[i]).map(|first| {
                            (first, (0..len).rfind(|&i| new[i] != current[i]).unwrap())
                        });
                        assert_eq!(
                            super::changed_span(&current, &new),
                            want,
                            "{len} {first} {last}"
                        );
                    }
                }
            }
        }
    }
}

// ── phrase item view ────────────────────────────────────────────────

/// One pronunciation of a phrase item.
///
/// The packed `ChewingKey` bytes (`2·phrase_length` of them — opaque at
/// this layer; P2's chewing reader owns their bitfield semantics) and the
/// pronunciation's frequency.
#[derive(Clone)]
pub struct PronunciationView<'a> {
    /// The packed `ChewingKey[L]` bytes, in stored order.
    pub keys: Cow<'a, [u8]>,
    /// This pronunciation's frequency (the `.table` count column
    /// verbatim).
    pub freq: u32,
}

/// A phrase item as it sits in the mapped entry area — upstream's
/// `PhraseItem` over `SubPhraseIndex::get_phrase_item`'s view, without
/// the copy.
///
/// `get_phrase_length`/`get_n_pronunciation`/
/// `get_unigram_frequency`/`get_phrase_string`/
/// `get_nth_pronunciation` become reads below. Every read gives the item as
/// the library was loaded: where an in-place replay changed a byte of it,
/// the read takes that byte's original value.
#[derive(Clone, Copy)]
pub struct PhraseItemView<'a> {
    bytes: &'a [u8],
    /// The original values of the bytes a replay changed in this item;
    /// `None` when it changed none.
    original: Option<Original<'a>>,
}

/// The bytes a sealed replay changed in one item, and their original values.
#[derive(Clone, Copy)]
struct Original<'a> {
    /// The records over the item's changed bytes.
    span: Span<'a>,
    /// The item's first byte, as a chunk position.
    base: usize,
    /// Bit `n` is set when the item's byte `n` changed, for its first
    /// [`HEAD`] bytes.
    head: u64,
    /// Whether a byte past the first [`HEAD`] changed.
    tail: bool,
}

/// The item bytes [`Original::head`] has a bit for.
const HEAD: usize = 64;

impl Original<'_> {
    /// Puts the original values back into `bytes`, which hold item bytes
    /// `at..`: the one out-of-line step of a read of a changed field.
    #[inline(never)]
    fn restore(&self, at: usize, bytes: &mut [u8]) {
        self.span.overlay(self.base + at, bytes);
    }

    /// Whether a byte of item bytes `at..at + len` changed.
    #[inline]
    fn changed(&self, at: usize, len: usize) -> bool {
        let end = at.saturating_add(len);
        let head = at < end.min(HEAD)
            && self.head >> at & (u64::MAX >> (HEAD - (end.min(HEAD) - at))) != 0;
        head || (end > HEAD && self.tail)
    }
}

impl<'a> PhraseItemView<'a> {
    /// A view of bytes no replay changed.
    const fn plain(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            original: None,
        }
    }

    /// The item's length: the extent its header gives, as
    /// `get_phrase_item` hands it out from its offset.
    #[must_use]
    #[expect(
        clippy::len_without_is_empty,
        reason = "an item always has its two-byte header"
    )]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// The item's bytes as the library was loaded, the header-derived
    /// length from its offset and nothing beyond it.
    #[must_use]
    pub fn to_bytes(&self) -> Cow<'a, [u8]> {
        self.range(0, self.bytes.len())
            .unwrap_or(Cow::Borrowed(self.bytes))
    }

    /// The original bytes `at..at + len`.
    #[inline]
    fn range(&self, at: usize, len: usize) -> Option<Cow<'a, [u8]>> {
        let bytes = self.bytes.get(at..at.checked_add(len)?)?;
        match self.original {
            Some(original) if original.changed(at, len) => {
                let mut owned = bytes.to_vec();
                original.restore(at, &mut owned);
                Some(Cow::Owned(owned))
            }
            _ => Some(Cow::Borrowed(bytes)),
        }
    }

    /// The original byte at `at`.
    #[inline]
    fn byte(&self, at: usize) -> Option<u8> {
        let mut byte = [*self.bytes.get(at)?];
        if let Some(original) = self.original
            && original.changed(at, 1)
        {
            original.restore(at, &mut byte);
        }
        Some(byte[0])
    }

    /// The original little-endian `u32` at `at`.
    #[inline]
    fn word(&self, at: usize) -> Option<u32> {
        let mut word: [u8; 4] = self.bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
        if let Some(original) = self.original
            && original.changed(at, 4)
        {
            original.restore(at, &mut word);
        }
        Some(u32::from_le_bytes(word))
    }
}

impl PhraseItemView<'_> {
    /// `PhraseItem::get_phrase_length`.
    #[must_use]
    #[inline]
    pub fn phrase_length(&self) -> usize {
        self.byte(0).unwrap_or(0) as usize
    }

    /// `PhraseItem::get_n_pronunciation`.
    #[must_use]
    #[inline]
    pub fn n_pronunciations(&self) -> usize {
        self.byte(1).unwrap_or(0) as usize
    }

    /// `PhraseItem::get_unigram_frequency` — the item's stored unigram
    /// count (the field `gen_unigram`'s +1 sweep and
    /// `add_unigram_frequency` write).
    #[must_use]
    #[inline]
    pub fn unigram(&self) -> u32 {
        self.word(2).unwrap_or(0)
    }

    /// The phrase text, decoded from its UCS-4 characters —
    /// `PhraseItem::get_phrase_string`. `None` when any scalar does
    /// not decode (a malformed item, never a panic).
    #[must_use]
    pub fn phrase_text(&self) -> Option<String> {
        self.phrase_chars().collect()
    }

    /// The phrase's UCS-4 scalars as an iterator.
    /// The phrase's code points in order, each `None` where the stored
    /// `ucs4_t` is not a Unicode scalar value: what `phrase_text` collects,
    /// for a reader that writes the characters itself instead of
    /// collecting a `String` first (the §9 export iterator).
    pub fn phrase_chars(&self) -> impl Iterator<Item = Option<char>> + '_ {
        let length = self.phrase_length();
        (0..length).map(move |index| {
            let offset = ITEM_HEADER_SIZE.checked_add(4 * index)?;
            let code = self.word(offset)?;
            char::from_u32(code)
        })
    }

    /// [`Self::phrase_text`] written into `buf` while it fits, for the
    /// dictionary-hit path: the same characters, the same `None` on the
    /// same malformed items, and no heap allocation for a phrase whose
    /// UTF-8 is at most `buf.len()` bytes. A longer phrase falls back to
    /// one owned [`String`] that carries the bytes already written and
    /// the rest of the text; nothing is decoded twice.
    ///
    /// The boundary is in UTF-8 bytes, not characters: a buffer of
    /// [`PHRASE_TEXT_STACK_BYTES`] holds any phrase of eight characters
    /// whatever their width, and every phrase of up to ten CJK
    /// characters.
    pub(crate) fn phrase_text_into<'b>(
        &self,
        buf: &'b mut [u8; PHRASE_TEXT_STACK_BYTES],
    ) -> Option<Cow<'b, str>> {
        let length = self.phrase_length();
        let mut written = 0_usize;
        let mut chars = self.phrase_chars().enumerate();
        for (index, ch) in chars.by_ref() {
            let ch = ch?;
            let width = ch.len_utf8();
            match buf.get_mut(written..written + width) {
                Some(slot) => {
                    ch.encode_utf8(slot);
                    written += width;
                }
                None => {
                    // The text outgrows the buffer: keep what is encoded
                    // and finish into one owned string sized for the
                    // worst case (four bytes per remaining character), so
                    // it never grows again.
                    let remaining = length.saturating_sub(index);
                    let mut text = String::with_capacity(written + 4 * remaining);
                    text.push_str(core::str::from_utf8(&buf[..written]).ok()?);
                    text.push(ch);
                    for (_, ch) in chars {
                        text.push(ch?);
                    }
                    return Some(Cow::Owned(text));
                }
            }
        }
        // Every byte of `buf[..written]` was produced by `encode_utf8`
        // for one scalar after another, so the prefix is valid UTF-8; the
        // check is kept rather than trusted, at the cost of one scan.
        core::str::from_utf8(&buf[..written])
            .ok()
            .map(Cow::Borrowed)
    }

    /// `PhraseItem::get_nth_pronunciation` — pronunciation `index`'s
    /// packed keys and frequency, bounds-checked.
    #[must_use]
    pub fn pronunciation(&self, index: usize) -> Option<PronunciationView<'_>> {
        if index >= self.n_pronunciations() {
            return None;
        }
        let stride = CHEWING_KEY_SIZE
            .checked_mul(self.phrase_length())?
            .checked_add(4)?;
        let start = ITEM_HEADER_SIZE
            .checked_add(4 * self.phrase_length())?
            .checked_add(index.checked_mul(stride)?)?;
        let keys_start = start;
        let freq_start = keys_start.checked_add(stride.checked_sub(4)?)?;
        let keys = self.range(keys_start, CHEWING_KEY_SIZE * self.phrase_length())?;
        let freq = self.word(freq_start)?;
        Some(PronunciationView { keys, freq })
    }

    /// Every pronunciation, in stored order.
    pub fn pronunciations(&self) -> impl Iterator<Item = PronunciationView<'_>> + '_ {
        (0..self.n_pronunciations()).map(|index| {
            self.pronunciation(index).unwrap_or(PronunciationView {
                keys: Cow::Borrowed(&[]),
                freq: 0,
            })
        })
    }

    /// The item's total byte length in the entry area —
    /// `phrase_item_header + L·4 + n·(L·sizeof(ChewingKey) + 4)`
    /// (`SubPhraseIndex::get_phrase_item`). `None` when the stored
    /// header claims a length that overflows or leaves the entry area.
    #[inline]
    fn total_size(&self) -> Option<usize> {
        let length = self.phrase_length();
        let per_pron = CHEWING_KEY_SIZE
            .checked_mul(length)?
            .checked_add(4)?
            .checked_mul(self.n_pronunciations())?;
        ITEM_HEADER_SIZE
            .checked_add(4 * length)?
            .checked_add(per_pron)
    }
}

// ── the library ─────────────────────────────────────────────────────

/// `MemoryChunk::mmap` (`memory_chunk.h:470-520`): maps the file and checks
/// that the header's length word is the payload length and that the
/// checksum verifies. Every refusal is [`LibraryError::Unmappable`], the
/// one error after which the pin writes `mmap %s failed!`.
fn map_chunk(path: &Path) -> Result<map::MappedFile, LibraryError> {
    let unmappable = |cause: LibraryError| LibraryError::Unmappable {
        path: path.to_path_buf(),
        cause: Box::new(cause),
    };
    let bad = |message: &str| unmappable(LibraryError::Format(message.to_owned()));
    // The size probe is POSIX `lseek` behaviour; elsewhere a directory keeps
    // failing in `MappedFile::open` as before.
    #[cfg(unix)]
    if path.is_dir() {
        return Err(directory_chunk_fault(path));
    }
    use std::io::{Read as _, Seek as _, SeekFrom};

    let file = std::fs::File::open(path).map_err(|error| unmappable(LibraryError::Io(error)))?;
    let file_size = file
        .metadata()
        .map_err(|error| unmappable(LibraryError::Io(error)))?
        .len();
    if file_size < CHUNK_HEADER_SIZE as u64 {
        return Err(bad("chunk shorter than its header"));
    }
    let payload_u64 = file_size - CHUNK_HEADER_SIZE as u64;
    if payload_u64 <= u64::from(u32::MAX) {
        // The common path — the whole file is the chunk, exactly as before
        // this reader learned the pin's truncation. Map once and read the
        // header and checksum out of the mapping; no extra syscall.
        let len =
            usize::try_from(file_size).map_err(|_| bad("file too large for this platform"))?;
        let file = map::MappedFile::open_len(file, len).map_err(unmappable)?;
        let bytes = file.as_slice();
        let declared_len = u32_at(bytes, 0).ok_or_else(|| bad("chunk shorter than its header"))?;
        if declared_len != payload_u64 as u32 {
            return Err(bad("chunk length word does not match the file"));
        }
        let checksum = u32_at(bytes, 4).ok_or_else(|| bad("chunk checksum unreadable"))?;
        if checksum != chunk_checksum(&bytes[CHUNK_HEADER_SIZE..]) {
            return Err(bad("chunk checksum mismatch"));
        }
        return Ok(file);
    }

    // `MemoryChunk::mmap`: `guint32 data_len = file_size - header`
    // (`memory_chunk.h:499`). The file is larger than the pin's chunk: the
    // pin compares the truncated word and checksums exactly that many
    // payload bytes, then `set_chunk`s `header + data_len` as the chunk.
    // Read the header, then map only that extent — the sparse 2^32+8 case
    // maps 8 bytes, not 4 GiB.
    let mut file = file;
    let mut header = [0u8; CHUNK_HEADER_SIZE];
    file.read_exact(&mut header)
        .map_err(|error| unmappable(LibraryError::Io(error)))?;
    let declared_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let payload_len = payload_u64 as u32;
    if declared_len != payload_len {
        return Err(bad("chunk length word does not match the file"));
    }
    let chunk_len = usize::try_from(u64::from(payload_len) + CHUNK_HEADER_SIZE as u64)
        .map_err(|_| bad("file too large for this platform"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| unmappable(LibraryError::Io(error)))?;
    let file = map::MappedFile::open_len(file, chunk_len).map_err(unmappable)?;
    let checksum = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if checksum != chunk_checksum(&file.as_slice()[CHUNK_HEADER_SIZE..]) {
        return Err(bad("chunk checksum mismatch"));
    }
    Ok(file)
}

/// What `MemoryChunk::mmap` does with a directory (`memory_chunk.h:479-493`):
/// it opens it, takes `file_size = lseek(fd, 0, SEEK_END)` and refuses a size
/// under the 8-byte header (an ordinary `false`, [`LibraryError::Unmappable`]);
/// from 8 up the `read` of the length word fails with `EISDIR` and the
/// `assert` at `:493` kills the pin ([`RowFault::ChunkHeaderRead`]). The size
/// is the file system's: ext4 reports the htree end of file for any
/// directory, btrfs twice the summed length of the entry names. A directory
/// that cannot be opened or sought is the same `false`. Unix only: opening
/// and seeking a directory is POSIX behaviour, and the pin is Linux-first.
#[cfg(unix)]
fn directory_chunk_fault(path: &Path) -> LibraryError {
    match directory_end(path) {
        Ok(size) if size >= CHUNK_HEADER_SIZE as u64 => {
            LibraryError::Row(RowFault::ChunkHeaderRead)
        }
        Ok(_) => LibraryError::Unmappable {
            path: path.to_path_buf(),
            cause: Box::new(LibraryError::Format(
                "directory shorter than the chunk header".to_owned(),
            )),
        },
        Err(error) => LibraryError::Unmappable {
            path: path.to_path_buf(),
            cause: Box::new(LibraryError::Io(error)),
        },
    }
}

/// `file_size = lseek(fd, 0, SEEK_END)` on a directory, as `MemoryChunk::load`
/// and `::mmap` take it (`memory_chunk.h:420-427`, `:479-486`): the one
/// file-system question every directory-as-chunk site asks.
fn directory_end(dir: &Path) -> std::io::Result<u64> {
    use std::io::Seek as _;
    std::fs::File::open(dir).and_then(|mut file| file.seek(std::io::SeekFrom::End(0)))
}

/// Whether `MemoryChunk::load` or `::mmap` on the directory `dir` gets past
/// `file_size < header` and asserts on the `read` of the length word
/// (`memory_chunk.h:427-434`, `:486-493`): `lseek(SEEK_END)` answers at
/// least the 8-byte chunk header. Where it fails or answers less (tmpfs
/// refuses `SEEK_END` on a directory), the load is an ordinary `false`
/// instead (`:427-430`, `:486-489`).
#[must_use]
pub fn directory_reads_as_a_chunk(dir: &Path) -> bool {
    directory_end(dir).is_ok_and(|size| size >= CHUNK_HEADER_SIZE as u64)
}

/// Whether `MemoryChunk::mmap` accepts the file at `path` now, keeping
/// nothing: the pin maps a system library again on every save, mask-out and
/// reload, and writes `mmap %s failed!` when it cannot.
///
/// # Errors
///
/// [`LibraryError::Unmappable`] when the chunk does not map.
pub fn verify_chunk(path: &Path) -> Result<(), LibraryError> {
    map_chunk(path).map(drop)
}

/// Where a library's sub-index sits in its chunk: what
/// `SubPhraseIndex::load` reads once (`phrase_index.cpp:335-360` at
/// 074a2219), kept apart from the bytes it addresses.
#[derive(Clone, Debug)]
struct Layout {
    /// The payload after the 8-byte chunk header.
    payload: Range<usize>,
    /// `SubPhraseIndex::m_total_freq`.
    total_freq: u32,
    /// Offset-array slots, `[index_one, index_two)` in payload bytes.
    offsets: Range<usize>,
    /// The entry area, `[index_two, index_three)` in payload bytes.
    content: Range<usize>,
    /// True for the empty sub-index the pin leaves behind when its
    /// `SubPhraseIndex::load` refused the layout; the save path re-checks
    /// such a library exactly as the pin re-maps it.
    layout_failed: bool,
}

impl Layout {
    /// The entry area's first byte, as a chunk position.
    const fn area_start(&self) -> usize {
        self.payload.start + self.content.start
    }
}

/// One per-library phrase index over a mapped libpinyin `*.bin` chunk
/// file — the `SubPhraseIndex` of `FacadePhraseIndex`, upstream's
/// mmap-backed half of the system data.
///
/// The chunk is mapped once. A session open may replay a `.dbin` journal
/// into that mapping before the library is shared, as the pin merges into
/// the chunk it loaded ([`Self::write_item_area`], [`Self::seal`]); the
/// library keeps the original value of every byte the replay changed, and
/// every reader below sees the chunk as it was loaded. The replayed bytes
/// are read through [`Self::current_item_area`].
#[derive(Clone)]
pub struct PhraseLibrary {
    file: std::sync::Arc<map::MappedFile>,
    layout: Layout,
    /// Immutable mapped item sum, shared by clones and filled at first compact.
    item_total: std::sync::Arc<std::sync::OnceLock<u32>>,
}

impl PhraseLibrary {
    /// The wrapping sum of mapped item frequencies, excluding slot zero.
    /// The loaded items never change, so repeated compaction shares one scan.
    #[must_use]
    pub fn item_total(&self) -> u32 {
        *self.item_total.get_or_init(|| {
            self.items()
                .filter(|(slot, _)| *slot >= 1)
                .fold(0_u32, |total, (_, item)| total.wrapping_add(item.unigram()))
        })
    }

    /// The pin's empty sub-index after `SubPhraseIndex::load` refused the
    /// layout: no mapping, `total_freq` as the pin read it before the
    /// failing check, and `get_range`'s `1..1`.
    pub(crate) fn empty(total_freq: u32) -> Self {
        Self {
            file: std::sync::Arc::new(map::MappedFile::empty()),
            layout: Layout {
                payload: 0..0,
                total_freq,
                offsets: 0..0,
                content: 0..0,
                layout_failed: true,
            },
            item_total: std::sync::Arc::default(),
        }
    }

    /// `FacadePhraseIndex::compact` rebuilds the sub-index from its items
    /// (`phrase_index.cpp:648-675`). The layout-failed sub-index has no
    /// items, so its newly built local total is zero; a well-formed
    /// library's stored total already equals its item sum. The facade does
    /// not adjust its own aggregate here.
    pub(crate) fn compact(&mut self) {
        if self.layout.layout_failed {
            self.layout.total_freq = 0;
        }
    }

    /// Whether this is the empty sub-index a refused layout left behind,
    /// as opposed to a mapped chunk that merely holds no items.
    #[must_use]
    pub const fn is_layout_failed(&self) -> bool {
        self.layout.layout_failed
    }

    /// Maps and validates one library file (`MemoryChunk::mmap` +
    /// `SubPhraseIndex::load`).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError`] when the file cannot be read, the chunk
    /// header or checksum does not verify, or the sub-index layout is
    /// not what the upstream loader accepts. A layout check the pin
    /// reaches through an in-chunk read is [`LibraryError::Layout`]: the
    /// caller loads an empty sub-index and reports the pin's success. A
    /// layout the pin would reach through an out-of-bounds read, or state
    /// the pin would expose as a huge/overlapping chunk, stays
    /// [`LibraryError::Format`] and is refused.
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        let file = map_chunk(path)?;
        let layout = read_layout(file.as_slice())?;
        Ok(Self {
            file: std::sync::Arc::new(file),
            layout,
            item_total: std::sync::Arc::default(),
        })
    }

    /// The payload bytes this library covers (for tests and tooling).
    #[inline]
    fn payload(&self) -> &[u8] {
        &self.file.as_slice()[self.layout.payload.clone()]
    }

    /// `SubPhraseIndex::get_phrase_index_total_freq`.
    #[must_use]
    pub const fn total_freq(&self) -> u32 {
        self.layout.total_freq
    }

    /// `SubPhraseIndex::get_range`: the library-local token range —
    /// `1 .. range_end` with trailing empty slots trimmed, `1..1` for
    /// an empty library.
    #[must_use]
    pub fn token_range(&self) -> Range<u32> {
        let offsets = &self.layout.offsets;
        let slots = (offsets.end - offsets.start) / 4;
        let mut end = slots;
        while end > 1 {
            let slot = u32_at(self.payload(), offsets.start + 4 * (end - 1));
            if slot.is_some_and(|offset| offset != 0) {
                break;
            }
            end -= 1;
        }
        1..u32::try_from(end.max(1)).unwrap_or(u32::MAX)
    }

    /// Whether the library stores any item content, without walking its
    /// slots. Every chunk the pin writes is compacted first, so its
    /// content holds only items its index points at (074a2219
    /// `phrase_index.cpp:648-675`, `pinyin.cpp:1139`).
    #[must_use]
    pub const fn has_items(&self) -> bool {
        self.layout.content.start < self.layout.content.end
    }

    /// The offset-array entry of a slot, or `None` outside the array.
    /// Checked arithmetic throughout: a caller-supplied slot of any
    /// `usize` value answers `None`, never an overflow. A replay writes
    /// only the entry area, so the array is always as loaded.
    #[inline]
    fn slot_offset(&self, slot: usize) -> Option<u32> {
        let start = slot
            .checked_mul(4)?
            .checked_add(self.layout.offsets.start)?;
        let end = start.checked_add(4)?;
        if end > self.layout.offsets.end {
            return None;
        }
        u32_at(self.payload(), start)
    }

    /// `SubPhraseIndex::get_phrase_item`: the item behind `token`'s
    /// library-local slot. `None` for an absent item (upstream
    /// `ERROR_NO_ITEM`), an out-of-range slot (`ERROR_OUT_OF_RANGE`),
    /// or a malformed entry (never a panic; upstream would answer
    /// `ERROR_FILE_CORRUPTION`).
    #[must_use]
    pub fn item(&self, token: u32) -> Option<PhraseItemView<'_>> {
        self.item_at_slot((token & PHRASE_MASK) as usize)
    }

    /// The entry area of `chunk`, a whole chunk's bytes.
    #[inline]
    fn area<'b>(&self, chunk: &'b [u8]) -> &'b [u8] {
        let start = self.layout.area_start();
        chunk
            .get(start..start + self.layout.content.len())
            .unwrap_or_default()
    }

    /// The byte at entry-area offset `at` as loaded.
    fn original_byte(&self, at: usize) -> Option<u8> {
        if let Some(loaded) = self.file.loaded() {
            return self.area(loaded).get(at).copied();
        }
        let byte = *self.current_item_area().get(at)?;
        Some(
            self.file
                .original()
                .get(self.layout.area_start() + at)
                .unwrap_or(byte),
        )
    }

    /// Read the count byte after the pin's offset and header checks.
    /// The pin does not read the item body here (074a2219
    /// `phrase_index.cpp:180-205`, `phrase_index.h:112-115`).
    #[must_use]
    pub fn pronunciation_count(&self, token: u32) -> Option<u32> {
        let offset = self.slot_offset((token & PHRASE_MASK) as usize)? as usize;
        if offset == 0 {
            return None;
        }
        self.original_byte(offset)?;
        self.original_byte(offset.checked_add(1)?).map(u32::from)
    }

    /// The item behind a raw library-local slot.
    #[must_use]
    pub fn item_at_slot(&self, slot: usize) -> Option<PhraseItemView<'_>> {
        let offset = self.slot_offset(slot)? as usize;
        if offset == 0 {
            return None;
        }
        // Offsets are 0-based into the entry area; the first item sits
        // at `FIRST_ITEM_OFFSET` (`SubPhraseIndex::add_phrase_item`
        // reserves those bytes so 0 stays the "no item" sentinel).
        match self.file.loaded() {
            Some(loaded) => {
                let content = self.area(loaded);
                let total = PhraseItemView::plain(content.get(offset..)?).total_size()?;
                Some(PhraseItemView::plain(
                    content.get(offset..offset.checked_add(total)?)?,
                ))
            }
            None => self.replayed_item_at(offset),
        }
    }

    /// `self.item(token).map(|item| item.unigram())`, read without the view.
    #[must_use]
    #[inline]
    pub fn item_unigram(&self, token: u32) -> Option<u32> {
        let offset = self.slot_offset((token & PHRASE_MASK) as usize)? as usize;
        if offset == 0 {
            return None;
        }
        let Some(loaded) = self.file.loaded() else {
            return self.replayed_item_at(offset).map(|item| item.unigram());
        };
        let item = self.area(loaded).get(offset..)?;
        let total = PhraseItemView::plain(item).total_size()?;
        Some(u32_at(item.get(..total)?, 2).unwrap_or(0))
    }

    /// [`Self::item_at_slot`] once a sealed replay changed bytes: the item's
    /// header and every read of it take the original values. Inline, so a
    /// session's first reads run in its callers' ordered code.
    #[inline(always)]
    fn replayed_item_at(&self, offset: usize) -> Option<PhraseItemView<'_>> {
        let item = self.current_item_area().get(offset..)?;
        let original = self.file.original();
        let base = self.layout.area_start() + offset;
        if !original.pages_touch(base, base.saturating_add(HEAD)) {
            // No record near the item's start: its header is as loaded.
            let total = PhraseItemView::plain(item).total_size()?;
            let bytes = item.get(..total)?;
            if total <= HEAD || !original.touches(base + HEAD, base + total) {
                return Some(PhraseItemView::plain(bytes));
            }
        }
        // One lookup covers the header and the first bytes of the item.
        let span = original.span(base, base.saturating_add(HEAD));
        let head = span.bits(base, HEAD);
        let mut header: [u8; 2] = item.get(..2)?.try_into().ok()?;
        if head & 0b11 != 0 {
            span.overlay(base, &mut header);
        }
        let total = PhraseItemView::plain(&header).total_size()?;
        let bytes = item.get(..total)?;
        let head = head & (u64::MAX >> (HEAD - total.min(HEAD)));
        let tail = total > HEAD && original.touches(base + HEAD, base + total);
        let original = match (head, tail) {
            (0, false) => None,
            (_, false) => Some(Original {
                span,
                base,
                head,
                tail,
            }),
            (_, true) => Some(Original {
                span: original.span(base, base + total),
                base,
                head,
                tail,
            }),
        };
        Some(PhraseItemView { bytes, original })
    }

    /// The length of the entry area — `SubPhraseIndex::m_phrase_content` as
    /// loaded (`SubPhraseIndex::load`'s `[index_two, index_three)`
    /// sub-chunk, `phrase_index.cpp:301-330`).
    #[must_use]
    #[inline]
    pub fn item_area_len(&self) -> usize {
        self.layout.content.len()
    }

    /// Entry-area bytes `range` as loaded; `None` past the area.
    #[must_use]
    pub fn original_area(&self, range: Range<usize>) -> Option<Cow<'_, [u8]>> {
        if let Some(loaded) = self.file.loaded() {
            return self.area(loaded).get(range).map(Cow::Borrowed);
        }
        let bytes = self.current_item_area().get(range.clone())?;
        let start = self.layout.area_start() + range.start;
        let original = self.file.original();
        if original.touches(start, start + bytes.len()) {
            let mut owned = bytes.to_vec();
            original.overlay(start, &mut owned);
            Some(Cow::Owned(owned))
        } else {
            Some(Cow::Borrowed(bytes))
        }
    }

    /// The entry area as an in-place replay left it: the bytes the item
    /// offsets address in the pin's merged sub-index. Equal to the loaded
    /// bytes until [`Self::write_item_area`] changes them.
    #[must_use]
    #[inline]
    pub fn current_item_area(&self) -> &[u8] {
        self.area(self.file.as_slice())
    }

    /// Writes `bytes` at entry-area offset `at` in this library's own
    /// mapping: the in-place half of `SubPhraseIndex::merge`, a `memmove`
    /// into the loaded chunk (`phrase_index.cpp:497-498` at 074a2219). The
    /// first write that changes a byte makes the private mapping writable
    /// and maps a view of the chunk as loaded beside it; [`Self::seal`]
    /// keeps the original value of each changed byte and unmaps that view.
    /// Every reader but [`Self::current_item_area`] sees the library as
    /// loaded throughout. `Ok(false)`, writing nothing, when the bytes do not
    /// fit the area.
    ///
    /// # Errors
    ///
    /// Another handle shares this library (an in-place write needs the only
    /// one), or the mapping could not be made writable. Nothing is written.
    #[inline]
    pub fn write_item_area(&mut self, at: usize, bytes: &[u8]) -> Result<bool, LibraryError> {
        if at
            .checked_add(bytes.len())
            .is_none_or(|end| end > self.layout.content.len())
        {
            return Ok(false);
        }
        let start = self.layout.area_start();
        let file = std::sync::Arc::get_mut(&mut self.file).ok_or_else(|| {
            LibraryError::Io(std::io::Error::other(
                "an in-place replay needs the only handle to the library",
            ))
        })?;
        file.write(start + at, bytes)?;
        Ok(true)
    }

    /// Ends an in-place replay: a mapping it made writable becomes read-only
    /// again (`make_read_only`) before the library is shared. A library no
    /// replay wrote, or one already shared, is left as it is.
    ///
    /// # Errors
    ///
    /// The mapping could not be made read-only. The library is then mapped
    /// again as it was loaded, without the replay's writes.
    pub fn seal(&mut self) -> Result<(), LibraryError> {
        std::sync::Arc::get_mut(&mut self.file).map_or(Ok(()), map::MappedFile::seal)
    }

    /// Whether `self` and `other` are handles to one mapping.
    #[must_use]
    pub fn shares_mapping(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.file, &other.file)
    }

    /// The heap the library keeps for the bytes a replay changed.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.file.original().heap_bytes()
    }

    /// The raw entry-area offset of `token`'s slot, `None` when the slot has
    /// no item. The offset indexes the entry area.
    #[must_use]
    #[inline]
    pub fn content_offset(&self, token: u32) -> Option<u32> {
        let offset = self.slot_offset((token & PHRASE_MASK) as usize)?;
        (offset != 0).then_some(offset)
    }

    /// Whether the mapped chunk is byte-for-byte the same as `other` —
    /// the full `header + data_len` extent `MemoryChunk::mmap` exposes,
    /// each side as it was loaded. A save compares the current on-disk
    /// chunk against the snapshot the session loaded with this, rather than
    /// path, size or mtime: a replacement may keep all three
    /// (`_write_files` remaps the path at `pinyin.cpp:956`,
    /// `zhuyin.cpp:589`).
    #[must_use]
    pub fn byte_identical(&self, other: &Self) -> bool {
        let len = self.file.as_slice().len();
        if len != other.file.as_slice().len() {
            return false;
        }
        if let (Some(mine), Some(theirs)) = (self.file.loaded(), other.file.loaded()) {
            return mine == theirs;
        }
        // A page at a time, each side's changed bytes put back.
        let (mut left, mut right) = ([0_u8; LOADED_STEP], [0_u8; LOADED_STEP]);
        let mut start = 0;
        while start < len {
            let end = (start + LOADED_STEP).min(len);
            if as_loaded(&self.file, start..end, &mut left)
                != as_loaded(&other.file, start..end, &mut right)
            {
                return false;
            }
            start = end;
        }
        true
    }

    /// Every `(slot, item)` pair with a resident item, slot order — a
    /// tooling/test surface (upstream never iterates; its reverse
    /// lookup goes through the phrase DBM).
    pub fn items(&self) -> impl Iterator<Item = (u32, PhraseItemView<'_>)> + '_ {
        let slots = (self.layout.offsets.end - self.layout.offsets.start) / 4;
        (0..slots).filter_map(move |slot| {
            self.item_at_slot(slot)
                .map(|item| (u32::try_from(slot).unwrap_or(u32::MAX), item))
        })
    }
}

/// The span [`PhraseLibrary::byte_identical`] compares at a time.
const LOADED_STEP: usize = 4096;

/// Chunk bytes `range` as loaded: borrowed when no replay changed them,
/// else copied into `buffer` with their original values put back.
fn as_loaded<'b>(
    file: &'b map::MappedFile,
    range: Range<usize>,
    buffer: &'b mut [u8; LOADED_STEP],
) -> &'b [u8] {
    if let Some(loaded) = file.loaded() {
        return loaded.get(range).unwrap_or_default();
    }
    let original = file.original();
    let Some(span) = file.as_slice().get(range.clone()) else {
        return &[];
    };
    if !original.touches(range.start, range.end) {
        return span;
    }
    let buffer = &mut buffer[..span.len()];
    buffer.copy_from_slice(span);
    original.overlay(range.start, buffer);
    buffer
}

/// `SubPhraseIndex::load` over a mapped chunk: the four words, the
/// separators and the sections, refused where the pin refuses them.
fn read_layout(bytes: &[u8]) -> Result<Layout, LibraryError> {
    let bad = |message: &str| LibraryError::Format(message.to_owned());
    let payload = CHUNK_HEADER_SIZE..bytes.len();
    let payload_bytes = bytes
        .get(payload.clone())
        .ok_or_else(|| bad("chunk shorter than its header"))?;
    // The pin's `get_content` leaves a failed read's destination at 0;
    // `m_total_freq` is read before the layout checks.
    let total_freq = u32_at(payload_bytes, 0).unwrap_or(0);
    let layout = |expression: &'static str| {
        LibraryError::Layout(LayoutFault {
            expression,
            total_freq,
        })
    };

    // `SubPhraseIndex::load`: the four words, then the separator at
    // payload[16], then `index_two - 1`, then `index_three - 1`.
    // A payload of 16 bytes or fewer makes the first read leave the
    // chunk; the failure is the same one, reproduced without the read.
    if payload_bytes.len() < 17 || payload_bytes[16] != SEPARATOR {
        return Err(layout("*(buf_begin + offset) == c_separate"));
    }
    let index_one = u32_at(payload_bytes, 4).ok_or_else(|| bad("payload too short"))? as usize;
    let index_two = u32_at(payload_bytes, 8).ok_or_else(|| bad("payload too short"))? as usize;
    let index_three = u32_at(payload_bytes, 12).ok_or_else(|| bad("payload too short"))? as usize;
    // The second and third checks read payload[index - 1]. Only an
    // index in `1..=len` keeps that read inside the chunk; index 0 or
    // one past the chunk is the pin's out-of-bounds read, which this
    // reader refuses as Format rather than reproduce.
    let second = index_two
        .checked_sub(1)
        .filter(|&at| at < payload_bytes.len())
        .ok_or_else(|| bad("sub-index offset-array separator out of bounds"))?;
    if payload_bytes[second] != SEPARATOR {
        return Err(layout("*(buf_begin + index_two - 1) == c_separate"));
    }
    let third = index_three
        .checked_sub(1)
        .filter(|&at| at < payload_bytes.len())
        .ok_or_else(|| bad("sub-index entry-area separator out of bounds"))?;
    if payload_bytes[third] != SEPARATOR {
        return Err(layout("*(buf_begin + index_three - 1) == c_separate"));
    }
    // The pin's fourth check (`index_three <= end`) is then true.
    // These guards cover the state the pin would expose as a
    // huge/overlapping chunk through `set_chunk`'s unsigned
    // subtraction; they stay refusals.
    //
    // `index_one` is 17 in every upstream file (4 words + separator)
    // and carries no alignment requirement — the loader reads u32s
    // at unaligned offsets; only the array's length is u32-sized.
    // `index_three > index_two` accounts for the entry area's own
    // closing separator, which `SubPhraseIndex::store` always emits —
    // the range built below subtracts it. The entry area itself may
    // still be empty.
    if index_two <= index_one || index_three <= index_two {
        return Err(bad("sub-index sections out of order"));
    }
    if !(index_two - index_one - 1).is_multiple_of(4) {
        return Err(bad("offset array is not u32-sized"));
    }
    Ok(Layout {
        payload,
        total_freq,
        // load's sub-chunk views: the offset array runs to the
        // byte before its separator, the entry area to the byte
        // before the final separator (`phrase_index.cpp:356-359`).
        offsets: index_one..index_two - 1,
        content: index_two..index_three - 1,
        layout_failed: false,
    })
}

impl fmt::Debug for PhraseLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhraseLibrary")
            .field("total_freq", &self.layout.total_freq)
            .field("token_range", &self.token_range())
            .finish()
    }
}

#[cfg(all(test, unix))]
mod sparse_mapping_tests {
    use super::map_chunk;
    use std::io::Write;

    #[test]
    fn a_sparse_chunk_maps_only_its_chunk_extent() {
        // The pin maps the whole 2^32+8 file and retains it after fini; the
        // subject's mapping layer takes only header + data_len (the 8-byte
        // header for this header-only sparse chunk, the layout refusal
        // happening only after the map).
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("merged.bin");
        let mut file = std::fs::File::create(&path).expect("create");
        file.set_len((1_u64 << 32) + 8).expect("sparse length");
        file.write_all(&[0_u8; 8]).expect("header");
        drop(file);
        let mapped = map_chunk(&path).expect("map");
        assert_eq!(
            mapped.as_slice().len(),
            8,
            "the sparse map is the chunk extent"
        );
    }
}

#[cfg(test)]
mod pronunciation_count_tests {
    use super::PhraseLibrary;

    #[test]
    fn count_checks_offset_and_two_header_bytes_only() {
        let mut payload = Vec::new();
        let content = [vec![0; 8], vec![255, 17]].concat();
        let offsets = [0_u32, 8, 9, 10, 0];
        let index_two = 17 + offsets.len() as u32 * 4 + 1;
        for value in [0, 17, index_two, index_two + content.len() as u32 + 1] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(b'#');
        for offset in offsets {
            payload.extend_from_slice(&offset.to_le_bytes());
        }
        payload.push(b'#');
        payload.extend_from_slice(&content);
        payload.push(b'#');
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("counts.bin");
        std::fs::write(
            &path,
            crate::chunk_format::build_memory_chunk(&payload).expect("frame"),
        )
        .expect("write");
        let library = PhraseLibrary::open(&path).expect("open");
        assert_eq!(library.pronunciation_count(1), Some(17));
        assert!(library.item(1).is_none(), "the body is truncated");
        for token in [0, 2, 3, 4, 5, u32::MAX] {
            assert_eq!(library.pronunciation_count(token), None, "token {token}");
        }
    }
}

#[cfg(test)]
mod phrase_text_into_tests {
    //! `phrase_text_into` against `phrase_text`: the same bytes and the
    //! same `None`s on every shape of item, and the stack buffer taken
    //! exactly when the UTF-8 fits it.

    use std::borrow::Cow;

    use super::{PHRASE_TEXT_STACK_BYTES, PhraseItemView};

    /// An item with `codes` as its UCS-4 text and no pronunciations:
    /// `{u8 len, u8 npron = 0, u32 unigram}` then the raw little-endian
    /// words, invalid values included.
    fn item_bytes(codes: &[u32]) -> Vec<u8> {
        let mut bytes = vec![u8::try_from(codes.len()).expect("test phrase fits u8"), 0];
        bytes.extend_from_slice(&7_u32.to_le_bytes());
        for code in codes {
            bytes.extend_from_slice(&code.to_le_bytes());
        }
        bytes
    }

    fn codes(text: &str) -> Vec<u32> {
        text.chars().map(u32::from).collect()
    }

    /// Both decoders over one item: `(phrase_text, phrase_text_into,
    /// borrowed?)`.
    fn both(bytes: &[u8]) -> (Option<String>, Option<String>, Option<bool>) {
        let item = PhraseItemView::plain(bytes);
        let old = item.phrase_text();
        let mut buf = [0_u8; PHRASE_TEXT_STACK_BYTES];
        let new = item.phrase_text_into(&mut buf);
        let borrowed = new.as_ref().map(|text| matches!(text, Cow::Borrowed(_)));
        (old, new.map(Cow::into_owned), borrowed)
    }

    fn assert_same(text: &str, expect_borrowed: bool) {
        let bytes = item_bytes(&codes(text));
        let (old, new, borrowed) = both(&bytes);
        assert_eq!(old.as_deref(), Some(text), "phrase_text of {text:?}");
        assert_eq!(new, old, "phrase_text_into of {text:?}");
        assert_eq!(
            borrowed,
            Some(expect_borrowed),
            "{text:?} ({} bytes) should {}use the stack buffer",
            text.len(),
            if expect_borrowed { "" } else { "not " }
        );
    }

    #[test]
    fn empty_phrase_is_the_empty_string_on_the_stack() {
        assert_same("", true);
    }

    #[test]
    fn ascii_phrases_match() {
        assert_same("a", true);
        assert_same("nihao", true);
        assert_same("abcdefghijklmnopqrstuvw", true); // 23 bytes
        assert_same("abcdefghijklmnopqrstuvwx", true); // 24 bytes
        assert_same("abcdefghijklmnopqrstuvwxy", true); // 25 bytes
        assert_same("abcdefghijklmnopqrstuvwxyzABCDEF", true); // 32 bytes
        assert_same("abcdefghijklmnopqrstuvwxyzABCDEFG", false); // 33 bytes
    }

    #[test]
    fn cjk_phrases_of_every_length_match() {
        let han = "中华人民共和国万岁大家一起来学习";
        for chars in [1_usize, 2, 4, 7, 8, 9, 10, 11, 16] {
            let text: String = han.chars().take(chars).collect();
            // 3 bytes per character: 10 characters fill the 30 of 32
            // bytes, 11 overflow.
            assert_same(&text, text.len() <= PHRASE_TEXT_STACK_BYTES);
        }
    }

    #[test]
    fn inline_boundary_bytes_23_24_25_match() {
        // 7 Han + 2 ASCII = 23, 8 Han = 24, 8 Han + 1 ASCII = 25.
        assert_same("中华人民共和国ab", true);
        assert_same("中华人民共和国万", true);
        assert_same("中华人民共和国万a", true);
    }

    #[test]
    fn every_utf8_width_and_mixed_text_match() {
        assert_same("a", true);
        assert_same("\u{e9}", true);
        assert_same("\u{4e2d}", true);
        assert_same("\u{20000}", true);
        assert_same("a\u{e9}\u{4e2d}\u{20000}", true);
        // Eight four-byte scalars are exactly the buffer; nine overflow
        // in the middle of a phrase, which is where the fallback starts.
        let eight: String = std::iter::repeat_n('\u{20000}', 8).collect();
        assert_same(&eight, true);
        let nine: String = std::iter::repeat_n('\u{20000}', 9).collect();
        assert_same(&nine, false);
        let long_mixed = "a\u{e9}\u{4e2d}\u{20000}\u{4e2d}\u{4e2d}\u{4e2d}\u{4e2d}\u{4e2d}\u{4e2d}\u{4e2d}\u{4e2d}";
        assert_same(long_mixed, false);
    }

    #[test]
    fn embedded_nul_is_kept_as_a_character() {
        // `char::from_u32(0)` is `Some('\0')` for both decoders; where that
        // becomes observable is the C-string edge, not here.
        assert_same("\u{4e2d}\0\u{6587}", true);
        assert_same("\0", true);
    }

    #[test]
    fn invalid_ucs4_is_none_wherever_it_sits() {
        let han = codes("中华人民共和国万岁大家一起来");
        for bad in [
            0xD800_u32,
            0xDBFF,
            0xDC00,
            0xDFFF,
            0x11_0000,
            0x7FFF_FFFF,
            0xFFFF_FFFF,
        ] {
            for len in [1_usize, 2, 8, 12] {
                for position in [0_usize, len / 2, len - 1] {
                    let mut text: Vec<u32> = han[..len].to_vec();
                    text[position] = bad;
                    let (old, new, _) = both(&item_bytes(&text));
                    assert_eq!(old, None, "phrase_text {bad:#x} at {position} of {len}");
                    assert_eq!(
                        new, None,
                        "phrase_text_into {bad:#x} at {position} of {len}"
                    );
                }
            }
        }
    }

    #[test]
    fn invalid_scalar_after_the_fallback_point_is_still_none() {
        // 11 Han (33 bytes) overflows the buffer at the 11th; make the
        // 12th invalid so the owned tail is what refuses.
        let mut text = codes("中华人民共和国万岁大家一");
        text.push(0xD800);
        let (old, new, _) = both(&item_bytes(&text));
        assert_eq!(old, None);
        assert_eq!(new, None);
    }

    #[test]
    fn truncated_items_are_none_for_both() {
        let full = item_bytes(&codes("中华人"));
        // Whole missing character, then a character cut mid-word, then a
        // header with no text at all, then nothing but one length byte.
        for cut in [full.len() - 4, full.len() - 1, 6, 1] {
            let (old, new, _) = both(&full[..cut]);
            assert_eq!(old, None, "cut at {cut}");
            assert_eq!(new, None, "cut at {cut}");
        }
        // An item whose header claims more characters than the buffer
        // holds, cut after the buffer would have filled: the fallback
        // path meets the truncation.
        let long = item_bytes(&codes("中华人民共和国万岁大家一起来学习"));
        let (old, new, _) = both(&long[..long.len() - 3]);
        assert_eq!(old, None);
        assert_eq!(new, None);
    }

    #[test]
    fn a_length_byte_of_zero_with_trailing_bytes_is_the_empty_string() {
        // `phrase_length` is the header's word; extra bytes belong to the
        // pronunciations, not the text.
        let mut bytes = item_bytes(&[]);
        bytes.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        let (old, new, borrowed) = both(&bytes);
        assert_eq!(old.as_deref(), Some(""));
        assert_eq!(new.as_deref(), Some(""));
        assert_eq!(borrowed, Some(true));
    }

    #[test]
    fn generated_valid_scalar_sequences_agree() {
        // A deterministic walk over scalar values of every width and
        // lengths across both sides of the buffer boundary.
        let pool: [char; 7] = [
            'a',
            '\u{7f}',
            '\u{e9}',
            '\u{7ff}',
            '\u{4e2d}',
            '\u{ffff}',
            '\u{10ffff}',
        ];
        let mut state = 0x9E37_79B9_u32;
        for len in 0..=20_usize {
            for _ in 0..8 {
                let text: String = (0..len)
                    .map(|_| {
                        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        pool[(state >> 29) as usize % pool.len()]
                    })
                    .collect();
                assert_same(&text, text.len() <= PHRASE_TEXT_STACK_BYTES);
            }
        }
    }
}

#[cfg(test)]
mod in_place_tests {
    //! A journal replay written into the library's own mapping: the readers
    //! keep the loaded chunk, the replayed bytes are the current entry area,
    //! and nothing is mapped twice.

    use super::PhraseLibrary;
    use crate::chunk_write::{ChunkItem, build_chunk};

    /// Items enough to fill three pages, so writes land on several pages.
    fn items() -> Vec<(u32, ChunkItem)> {
        (1..=600_u32)
            .map(|slot| {
                (
                    slot,
                    ChunkItem {
                        phrase: vec![0x4e00 + slot, 0x4e01],
                        unigram: slot,
                        prons: vec![(vec![u16::try_from(slot).unwrap(), 2], slot * 3)],
                    },
                )
            })
            .collect()
    }

    fn library() -> (tempfile::TempDir, std::path::PathBuf, PhraseLibrary) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("lib.bin");
        std::fs::write(&path, build_chunk(&items()).expect("chunk")).expect("write");
        let library = PhraseLibrary::open(&path).expect("open");
        (dir, path, library)
    }

    /// Every reader of the loaded chunk: the items with their text, unigram,
    /// readings and counts, the entry area, the item sum and the chunk.
    fn readings(library: &PhraseLibrary) -> Vec<String> {
        let mut out: Vec<String> = library
            .items()
            .map(|(slot, item)| {
                let prons: Vec<_> = item
                    .pronunciations()
                    .map(|pron| (pron.keys.to_vec(), pron.freq))
                    .collect();
                format!(
                    "{slot} {:?} {} {prons:?} {:?} {:?}",
                    item.phrase_text(),
                    item.unigram(),
                    library.pronunciation_count(slot),
                    item.to_bytes(),
                )
            })
            .collect();
        out.push(format!(
            "{:?}",
            library.original_area(0..library.item_area_len())
        ));
        out.push(format!("total {}", library.item_total()));
        out
    }

    /// `path`'s mappings in this process, by permissions.
    #[cfg(target_os = "linux")]
    fn mappings(path: &std::path::Path) -> Vec<String> {
        let path = path.canonicalize().expect("path");
        std::fs::read_to_string("/proc/self/maps")
            .expect("maps")
            .lines()
            .filter(|line| line.ends_with(path.to_str().expect("utf-8 path")))
            .map(|line| {
                line.split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn a_replayed_library_still_reads_as_loaded() {
        let (_dir, path, mut library) = library();
        let before = readings(&library);
        let fresh = PhraseLibrary::open(&path).expect("open again");
        // Rewrite every item's frequency in place, as a MODIFY does, and
        // one item's header to fewer readings.
        let offsets: Vec<_> = (1..=600_u32)
            .map(|slot| library.content_offset(slot).expect("offset") as usize)
            .collect();
        for (index, &offset) in offsets.iter().enumerate() {
            let item = library
                .item(index as u32 + 1)
                .expect("item")
                .to_bytes()
                .into_owned();
            let mut new = item.clone();
            let freq = new.len() - 4;
            new[freq..].copy_from_slice(&(9_000 + index as u32).to_le_bytes());
            new[2..6].copy_from_slice(&(7_000 + index as u32).to_le_bytes());
            assert!(library.write_item_area(offset, &new).expect("write"));
        }
        assert!(
            library
                .write_item_area(offsets[9], &[1, 0])
                .expect("header")
        );
        assert!(
            !library
                .write_item_area(library.item_area_len(), &[1])
                .expect("past the area")
        );
        // While the replay writes, the readers see the view of the chunk as
        // loaded beside the writable mapping.
        assert_eq!(readings(&library), before);
        library.seal().expect("seal");

        assert_eq!(readings(&library), before);
        assert!(library.byte_identical(&fresh));
        assert!(fresh.byte_identical(&library));
        let current = library.current_item_area();
        assert_eq!(
            &current[offsets[0] + 18..offsets[0] + 22],
            &9_000_u32.to_le_bytes()
        );
        assert_eq!(&current[offsets[9]..offsets[9] + 2], &[1, 0]);
        assert_ne!(
            current,
            &*fresh.original_area(0..fresh.item_area_len()).expect("area")
        );
        // Only the changed bytes are kept: the readings' and unigrams'
        // low halves and the one header byte, not the whole items.
        let area = library.item_area_len();
        assert!(
            library.retained_bytes() < area / 2,
            "{} of {area}",
            library.retained_bytes()
        );
        #[cfg(target_os = "linux")]
        {
            drop(fresh);
            assert_eq!(mappings(&path), ["r--p"], "one mapping, read-only again");
        }
    }

    /// Items longer than the change mask's 64 bytes, with changes before,
    /// across and past it, and in the header, read as loaded once sealed.
    #[test]
    fn long_items_read_as_loaded_past_their_first_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("long.bin");
        let items: Vec<(u32, ChunkItem)> = (1..=300_u32)
            .map(|slot| {
                let phrase: Vec<u32> = (0..6).map(|index| 0x4e00 + slot + index).collect();
                let prons = (0..3_u16)
                    .map(|reading| {
                        let keys = (0..6).map(|key| reading * 7 + key + 1).collect();
                        (keys, slot * 10 + u32::from(reading))
                    })
                    .collect();
                (
                    slot,
                    ChunkItem {
                        phrase,
                        unigram: slot,
                        prons,
                    },
                )
            })
            .collect();
        std::fs::write(&path, build_chunk(&items).expect("chunk")).expect("write");
        let mut library = PhraseLibrary::open(&path).expect("open");
        let before = readings(&library);
        for slot in 1..=300_u32 {
            let offset = library.content_offset(slot).expect("offset") as usize;
            let mut item = library.item(slot).expect("item").to_bytes().into_owned();
            assert!(item.len() > 64, "{}", item.len());
            match slot % 4 {
                // The last reading's frequency, past the first 64 bytes.
                0 => {
                    let at = item.len() - 4;
                    item[at] ^= 0x5A;
                }
                // A key byte across the 64th.
                1 => {
                    item[63] ^= 1;
                    item[64] ^= 1;
                }
                // The unigram and a byte past the mask.
                2 => {
                    item[2] ^= 0x10;
                    item[70] ^= 0x20;
                }
                // The header: one reading fewer.
                _ => item[1] = 2,
            }
            assert!(library.write_item_area(offset, &item).expect("write"));
        }
        assert_eq!(readings(&library), before);
        library.seal().expect("seal");
        assert_eq!(readings(&library), before);
        let fresh = PhraseLibrary::open(&path).expect("open again");
        assert!(library.byte_identical(&fresh));
    }

    /// A replay that changes items on the first page only: the items on
    /// the pages without records read as they stand, the others as loaded.
    #[test]
    fn a_partly_replayed_library_reads_every_item_as_loaded() {
        let (_dir, path, mut library) = library();
        let before = readings(&library);
        for slot in 1..=40_u32 {
            let offset = library.content_offset(slot).expect("offset") as usize;
            let mut item = library.item(slot).expect("item").to_bytes().into_owned();
            let at = item.len() - 4;
            item[at] ^= 0x33;
            assert!(library.write_item_area(offset, &item).expect("write"));
        }
        library.seal().expect("seal");
        assert!(library.retained_bytes() > 0);
        assert_eq!(readings(&library), before);
        let fresh = PhraseLibrary::open(&path).expect("open again");
        assert!(library.byte_identical(&fresh));
    }

    #[test]
    fn an_unchanging_replay_maps_nothing_writable() {
        let (_dir, path, mut library) = library();
        let offset = library.content_offset(3).expect("offset") as usize;
        let same = library.item(3).expect("item").to_bytes().into_owned();
        assert!(library.write_item_area(offset, &same).expect("write"));
        library.seal().expect("seal");
        assert_eq!(library.retained_bytes(), 0);
        #[cfg(target_os = "linux")]
        {
            let smaps = std::fs::read_to_string("/proc/self/smaps").expect("smaps");
            let path = path.canonicalize().expect("path");
            let mut lines = smaps.lines();
            // A copied page of a file mapping is an anonymous page; the
            // page cache's own dirty pages are not.
            let mut copied = None;
            while let Some(line) = lines.next() {
                if line.ends_with(path.to_str().expect("utf-8 path")) {
                    assert!(line.contains(" r--p "), "{line}");
                    copied = lines
                        .find(|field| field.starts_with("Anonymous:"))
                        .and_then(|field| field.split_whitespace().nth(1))
                        .map(str::to_owned);
                }
            }
            assert_eq!(copied.as_deref(), Some("0"));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = path;
    }

    #[test]
    fn a_shared_library_is_not_written_in_place() {
        let (_dir, _path, mut library) = library();
        let other = library.clone();
        let offset = library.content_offset(1).expect("offset") as usize;
        assert!(library.write_item_area(offset, &[9, 9]).is_err());
        assert!(library.shares_mapping(&other));
        assert_eq!(library.current_item_area(), other.current_item_area());
        drop(other);
        assert!(
            library
                .write_item_area(offset, &[9, 9])
                .expect("now the only handle")
        );
    }

    #[test]
    fn a_refused_seal_leaves_the_library_as_loaded() {
        let (_dir, path, mut library) = library();
        let before = readings(&library);
        let offset = library.content_offset(1).expect("offset") as usize;
        assert!(library.write_item_area(offset, &[0, 0, 7]).expect("write"));
        std::sync::Arc::get_mut(&mut library.file)
            .expect("only handle")
            .fail_seal = true;
        assert!(library.seal().is_err());
        assert_eq!(readings(&library), before);
        assert_eq!(library.retained_bytes(), 0);
        let fresh = PhraseLibrary::open(&path).expect("open again");
        assert_eq!(
            library.current_item_area(),
            &*fresh.original_area(0..fresh.item_area_len()).expect("area")
        );
    }
}
