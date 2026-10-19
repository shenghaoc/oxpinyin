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

    /// Map the chunk extent as one immutable slice on all targets.
    pub struct MappedFile(Bytes);

    /// Each variant owns its bytes. Both are `Send + Sync`.
    enum Bytes {
        /// A private mapping of the chunk extent at offset zero.
        Mapped(memmap2::Mmap),
        /// Empty bytes for a zero-length file or a refused sub-index.
        Heap(Box<[u8]>),
    }

    impl MappedFile {
        /// No bytes and no mapping: the payload of a chunk whose header was
        /// accepted but whose sub-index layout the pin refused.
        pub(crate) fn empty() -> Self {
            Self(Bytes::Heap(Box::new([])))
        }

        /// Map the first `len` bytes of `file`.
        /// This is the header plus data length used by `MemoryChunk::mmap`.
        /// Do not map the rest of the file if the header has a shorter length.
        pub(crate) fn open_len(file: std::fs::File, len: usize) -> Result<Self, LibraryError> {
            Self::from_file(file, len)
        }

        fn from_file(file: std::fs::File, len: usize) -> Result<Self, LibraryError> {
            if len == 0 {
                // Keep an empty heap slice. Do not map a zero-length file.
                return Ok(Self(Bytes::Heap(Box::new([]))));
            }
            // SAFETY: The file is open. The offset is zero. The explicit
            // length is the chunk extent checked by map_chunk. Mmap owns
            // the mapping. Each returned slice borrows that owner. This
            // code does not write through the mapping. Do not change a
            // table file while it is mapped. These are system and addon
            // libraries. This runtime does not write them.
            // On Unix, map_copy_read_only uses PROT_READ and MAP_PRIVATE.
            // A private mapping is not a snapshot. An in-place file write
            // can change its bytes. Truncation can cause SIGBUS on a later
            // read. A rename replacement leaves the view on the old file.
            // On Windows, memmap2 uses FILE_MAP_COPY with PAGE_WRITECOPY
            // or PAGE_EXECUTE_WRITECOPY. It can then reduce page access
            // with VirtualProtect. The Mmap API gives this code no mutable
            // slice. The OS view need not be read-only. On Windows CI,
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
            let mapped = unsafe {
                memmap2::MmapOptions::new()
                    .len(len)
                    .map_copy_read_only(&file)
            }
            .map_err(LibraryError::Io)?;
            drop(file);
            Ok(Self(Bytes::Mapped(mapped)))
        }

        /// The mapped bytes. The slice borrows `self`, so the mapping
        /// outlives every view taken over it.
        pub(crate) fn as_slice(&self) -> &[u8] {
            match &self.0 {
                Bytes::Mapped(mapped) => mapped,
                Bytes::Heap(heap) => heap,
            }
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
    }
}

// ── phrase item view ────────────────────────────────────────────────

/// One pronunciation of a phrase item.
///
/// The packed `ChewingKey` bytes (`2·phrase_length` of them — opaque at
/// this layer; P2's chewing reader owns their bitfield semantics) and the
/// pronunciation's frequency.
#[derive(Clone, Copy)]
pub struct PronunciationView<'a> {
    /// The packed `ChewingKey[L]` bytes, in stored order.
    pub keys: &'a [u8],
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
/// `get_nth_pronunciation` become reads below.
#[derive(Clone, Copy)]
pub struct PhraseItemView<'a> {
    bytes: &'a [u8],
}

impl<'a> PhraseItemView<'a> {
    /// The item's bytes as `get_phrase_item` hands them out: the header-derived
    /// length from its offset, nothing beyond it.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

impl PhraseItemView<'_> {
    /// `PhraseItem::get_phrase_length`.
    #[must_use]
    pub fn phrase_length(&self) -> usize {
        self.bytes.first().copied().unwrap_or(0) as usize
    }

    /// `PhraseItem::get_n_pronunciation`.
    #[must_use]
    pub fn n_pronunciations(&self) -> usize {
        self.bytes.get(1).copied().unwrap_or(0) as usize
    }

    /// `PhraseItem::get_unigram_frequency` — the item's stored unigram
    /// count (the field `gen_unigram`'s +1 sweep and
    /// `add_unigram_frequency` write).
    #[must_use]
    pub fn unigram(&self) -> u32 {
        u32_at(self.bytes, 2).unwrap_or(0)
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
            let code = u32_at(self.bytes, offset)?;
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
        let keys_end = keys_start.checked_add(CHEWING_KEY_SIZE * self.phrase_length())?;
        let keys = self.bytes.get(keys_start..keys_end)?;
        let freq = u32_at(self.bytes, freq_start)?;
        Some(PronunciationView { keys, freq })
    }

    /// Every pronunciation, in stored order.
    pub fn pronunciations(&self) -> impl Iterator<Item = PronunciationView<'_>> + '_ {
        (0..self.n_pronunciations()).map(|index| {
            self.pronunciation(index)
                .unwrap_or(PronunciationView { keys: &[], freq: 0 })
        })
    }

    /// The item's total byte length in the entry area —
    /// `phrase_item_header + L·4 + n·(L·sizeof(ChewingKey) + 4)`
    /// (`SubPhraseIndex::get_phrase_item`). `None` when the stored
    /// header claims a length that overflows or leaves the entry area.
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
    use std::io::{Seek, SeekFrom};
    let sought = std::fs::File::open(path).and_then(|mut dir| dir.seek(SeekFrom::End(0)));
    match sought {
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

/// One per-library phrase index over a mapped libpinyin `*.bin` chunk
/// file — the `SubPhraseIndex` of `FacadePhraseIndex`, upstream's
/// mmap-backed half of the system data.
#[derive(Clone)]
pub struct PhraseLibrary {
    file: std::sync::Arc<map::MappedFile>,
    /// The payload after the 8-byte chunk header.
    payload: std::ops::Range<usize>,
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
    /// Immutable mapped item sum, shared by clones and filled at first compact.
    item_total: std::sync::Arc<std::sync::OnceLock<u32>>,
}

impl PhraseLibrary {
    /// The wrapping sum of mapped item frequencies, excluding slot zero.
    /// The mapping is immutable, so repeated compaction shares one scan.
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
            payload: 0..0,
            total_freq,
            offsets: 0..0,
            content: 0..0,
            layout_failed: true,
            item_total: std::sync::Arc::default(),
        }
    }

    /// `FacadePhraseIndex::compact` rebuilds the sub-index from its items
    /// (`phrase_index.cpp:648-675`). The layout-failed sub-index has no
    /// items, so its newly built local total is zero; a well-formed
    /// library's stored total already equals its item sum. The facade does
    /// not adjust its own aggregate here.
    pub(crate) fn compact(&mut self) {
        if self.layout_failed {
            self.total_freq = 0;
        }
    }

    /// Whether this is the empty sub-index a refused layout left behind,
    /// as opposed to a mapped chunk that merely holds no items.
    #[must_use]
    pub const fn is_layout_failed(&self) -> bool {
        self.layout_failed
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
        let bytes = file.as_slice();
        let bad = |message: &str| LibraryError::Format(message.to_owned());
        let payload = CHUNK_HEADER_SIZE..bytes.len();
        let payload_bytes = &bytes[payload.clone()];
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
        let index_three =
            u32_at(payload_bytes, 12).ok_or_else(|| bad("payload too short"))? as usize;
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

        Ok(Self {
            file: std::sync::Arc::new(file),
            payload,
            total_freq,
            // load's sub-chunk views: the offset array runs to the
            // byte before its separator, the entry area to the byte
            // before the final separator (`phrase_index.cpp:356-359`).
            offsets: index_one..index_two - 1,
            content: index_two..index_three - 1,
            layout_failed: false,
            item_total: std::sync::Arc::default(),
        })
    }

    /// The payload bytes this library covers (for tests and tooling).
    fn payload(&self) -> &[u8] {
        &self.file.as_slice()[self.payload.clone()]
    }

    /// `SubPhraseIndex::get_phrase_index_total_freq`.
    #[must_use]
    pub const fn total_freq(&self) -> u32 {
        self.total_freq
    }

    /// `SubPhraseIndex::get_range`: the library-local token range —
    /// `1 .. range_end` with trailing empty slots trimmed, `1..1` for
    /// an empty library.
    #[must_use]
    pub fn token_range(&self) -> Range<u32> {
        let slots = (self.offsets.end - self.offsets.start) / 4;
        let mut end = slots;
        while end > 1 {
            let slot = u32_at(self.payload(), self.offsets.start + 4 * (end - 1));
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
        self.content.start < self.content.end
    }

    /// The offset-array entry of a slot, or `None` outside the array.
    /// Checked arithmetic throughout: a caller-supplied slot of any
    /// `usize` value answers `None`, never an overflow.
    fn slot_offset(&self, slot: usize) -> Option<u32> {
        let start = slot.checked_mul(4)?.checked_add(self.offsets.start)?;
        let end = start.checked_add(4)?;
        if end > self.offsets.end {
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

    /// Read the count byte after the pin's offset and header checks.
    /// The pin does not read the item body here (074a2219
    /// `phrase_index.cpp:180-205`, `phrase_index.h:112-115`).
    #[must_use]
    pub fn pronunciation_count(&self, token: u32) -> Option<u32> {
        let offset = self.slot_offset((token & PHRASE_MASK) as usize)? as usize;
        if offset == 0 {
            return None;
        }
        let content = &self.payload()[self.content.clone()];
        content.get(offset)?;
        content.get(offset.checked_add(1)?).copied().map(u32::from)
    }

    /// The item behind a raw library-local slot.
    #[must_use]
    pub fn item_at_slot(&self, slot: usize) -> Option<PhraseItemView<'_>> {
        let offset = self.slot_offset(slot)? as usize;
        if offset == 0 {
            return None;
        }
        let content = &self.payload()[self.content.clone()];
        // Offsets are 0-based into the entry area; the first item sits
        // at `FIRST_ITEM_OFFSET` (`SubPhraseIndex::add_phrase_item`
        // reserves those bytes so 0 stays the "no item" sentinel).
        let item_bytes = content.get(offset..)?;
        let item = PhraseItemView { bytes: item_bytes };
        let total = item.total_size()?;
        if offset.checked_add(total)? > content.len() {
            return None;
        }
        Some(PhraseItemView {
            bytes: content.get(offset..offset + total)?,
        })
    }

    /// The entry area — `SubPhraseIndex::m_phrase_content` as loaded, the
    /// bytes the pin's item offsets address (`SubPhraseIndex::load`'s
    /// `[index_two, index_three)` sub-chunk, `phrase_index.cpp:301-330`).
    #[must_use]
    pub fn item_area(&self) -> &[u8] {
        &self.payload()[self.content.clone()]
    }

    /// The raw entry-area offset of `token`'s slot, `None` when the slot has
    /// no item. The offset indexes [`Self::item_area`].
    #[must_use]
    pub fn content_offset(&self, token: u32) -> Option<u32> {
        let offset = self.slot_offset((token & PHRASE_MASK) as usize)?;
        (offset != 0).then_some(offset)
    }

    /// Whether the mapped chunk is byte-for-byte the same as `other` —
    /// the full `header + data_len` extent `MemoryChunk::mmap` exposes.
    /// A save compares the current on-disk chunk against the snapshot the
    /// session loaded with this, rather than path, size or mtime: a
    /// replacement may keep all three (`_write_files` remaps the path at
    /// `pinyin.cpp:956`, `zhuyin.cpp:589`).
    #[must_use]
    pub fn byte_identical(&self, other: &Self) -> bool {
        self.file.as_slice() == other.file.as_slice()
    }

    /// Every `(slot, item)` pair with a resident item, slot order — a
    /// tooling/test surface (upstream never iterates; its reverse
    /// lookup goes through the phrase DBM).
    pub fn items(&self) -> impl Iterator<Item = (u32, PhraseItemView<'_>)> + '_ {
        let slots = (self.offsets.end - self.offsets.start) / 4;
        (0..slots).filter_map(move |slot| {
            self.item_at_slot(slot)
                .map(|item| (u32::try_from(slot).unwrap_or(u32::MAX), item))
        })
    }
}

impl fmt::Debug for PhraseLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhraseLibrary")
            .field("total_freq", &self.total_freq)
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
        let item = PhraseItemView { bytes };
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
