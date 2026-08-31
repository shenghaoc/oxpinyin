//! Reader for libpinyin's per-library phrase-index files (the
//! `MemoryChunk` + `SubPhraseIndex` pair).
//!
//! P1 of the direct-replacement rework
//! (`docs/findings/libpinyin-system-data-formats-2026-09-01.md`): this
//! consumes **the files a libpinyin installation already ships**
//! (`gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin`, and the
//! addon `*.bin` libraries) — no conversion, no oxpinyin-side format.
//! It is the Rust equivalent of the pinned upstream reader:
//!
//! - `src/include/memory_chunk.h` — `MemoryChunk::mmap`: an 8-byte
//!   header `{length: u32, checksum: u32}` then the payload; `mmap`
//!   verifies `length == file_size − 8` and recomputes the checksum
//!   over the payload before handing out a pointer.
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

use std::fmt;
use std::ops::Range;
use std::path::Path;

/// `PHRASE_MASK` (`novel_types.h:41`): the library-local token bits a
/// phrase-index slot is addressed by.
const PHRASE_MASK: u32 = 0x00FF_FFFF;
/// `c_separate` (`novel_types.h:126`).
const SEPARATOR: u8 = b'#';
/// `sizeof(ChewingKey)` — a 16-bit bitfield (`chewing_key.h:41`).
const CHEWING_KEY_SIZE: usize = 2;
/// The MemoryChunk file header: `{length, checksum}`.
const CHUNK_HEADER_SIZE: usize = 8;
/// `phrase_item_header` (`phrase_index.h:56`): length, n-pron,
/// unigram.
const ITEM_HEADER_SIZE: usize = 6;

/// Why a phrase library could not be opened.
#[derive(Debug)]
pub enum LibraryError {
    /// The file could not be read or mapped.
    Io(std::io::Error),
    /// The chunk header, checksum, or sub-index layout is not what the
    /// upstream reader would accept.
    Format(String),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "phrase library I/O error: {e}"),
            Self::Format(message) => write!(f, "phrase library format error: {message}"),
        }
    }
}

impl std::error::Error for LibraryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Format(_) => None,
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

/// `MemoryChunk::get_check_sum` (`memory_chunk.h:131-159`): the XOR of
/// the payload's little-endian `u32` words, with any tail bytes folded
/// in shifted by their position. Reproduced exactly — the header's
/// checksum is what upstream verifies at `mmap` time.
fn chunk_checksum(payload: &[u8]) -> u32 {
    let mut checksum: u32 = 0;
    let aligned = payload.len() & !0x3;
    for word in payload[..aligned].chunks_exact(4) {
        checksum ^= u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let mut shift = 0_u32;
    for &byte in &payload[aligned..] {
        checksum ^= u32::from(byte) << shift;
        shift += 8;
    }
    checksum
}

// ── mapped file ─────────────────────────────────────────────────────

/// File mapping: the one place in this crate (and the shipping tree)
/// with `unsafe`, under the constitution's documented mmap exception.
mod map {
    #![allow(unsafe_code)]

    use super::LibraryError;
    use std::path::Path;

    /// A whole file's bytes as one immutable slice: `mmap`ed on Unix,
    /// read into a heap buffer elsewhere. This mirrors upstream's
    /// `MemoryChunk::mmap` (`memory_chunk.h:470-520`), with
    /// `PROT_READ` where upstream maps read-write-private because its
    /// item views may be mutated in memory; this reader never writes,
    /// and a read-only mapping is strictly safer for the same bytes.
    pub(crate) struct MappedFile {
        /// First byte of the mapping (or of the heap fallback).
        data: *const u8,
        /// Mapping length in bytes.
        len: usize,
        /// `None` when `data` came from `mmap` (released with
        /// `munmap`); `Some` on the non-Unix fallback, where it holds
        /// the bytes `data` points into and frees them itself.
        heap: Option<Box<[u8]>>,
    }

    impl MappedFile {
        /// Maps (or reads) the file at `path`.
        ///
        /// A zero-length file becomes an empty mapping on every
        /// platform; whether empty bytes are a valid chunk is the
        /// parser's call.
        pub(crate) fn open(path: &Path) -> Result<Self, LibraryError> {
            let file = std::fs::File::open(path).map_err(LibraryError::Io)?;
            let len = std::fs::metadata(path).map_err(LibraryError::Io)?.len() as usize;
            Self::from_file(file, len)
        }

        #[cfg(unix)]
        fn from_file(file: std::fs::File, len: usize) -> Result<Self, LibraryError> {
            if len == 0 {
                // mmap rejects zero lengths; an empty heap buffer
                // represents the empty mapping without a syscall.
                return Ok(Self {
                    data: std::ptr::NonNull::<u8>::dangling().as_ptr(),
                    len: 0,
                    heap: Some(Box::new([])),
                });
            }
            use std::os::fd::AsRawFd;
            // Historic cross-Unix values (Linux, macOS, the BSDs agree).
            const PROT_READ: i32 = 0x1;
            const MAP_PRIVATE: i32 = 0x02;
            // SAFETY (mmap): the platform's own mapping entry point,
            // declared below verbatim so no dependency is needed. Its
            // invariants hold by construction on entry: `addr` is null
            // and `offset` zero so the kernel places the mapping; `len`
            // is the file's `fstat` length, so the whole range is
            // file-backed; the protection is read + `MAP_PRIVATE`
            // (never `MAP_SHARED`), so the returned pages are a
            // read-only snapshot — writing them faults and no write can
            // reach the file; the fd is open and may be closed right
            // after, the mapping holds its own reference. `MAP_FAILED`
            // (all-ones) is checked before the pointer is kept.
            let mapped = unsafe {
                mmap(
                    std::ptr::null_mut(),
                    len,
                    PROT_READ,
                    MAP_PRIVATE,
                    file.as_raw_fd(),
                    0,
                )
            };
            if mapped.addr() == usize::MAX {
                return Err(LibraryError::Io(std::io::Error::last_os_error()));
            }
            drop(file);
            Ok(Self {
                data: mapped.cast::<u8>(),
                len,
                heap: None,
            })
        }

        #[cfg(not(unix))]
        fn from_file(mut file: std::fs::File, len: usize) -> Result<Self, LibraryError> {
            use std::io::Read;
            let mut buffer = Vec::with_capacity(len.min(1 << 20));
            file.read_to_end(&mut buffer).map_err(LibraryError::Io)?;
            let len = buffer.len();
            Ok(Self {
                data: buffer.as_ptr(),
                len,
                heap: Some(buffer.into_boxed_slice()),
            })
        }

        /// The mapped bytes. The slice borrows `self`, so the mapping
        /// outlives every view taken over it.
        pub(crate) fn as_slice(&self) -> &[u8] {
            // SAFETY (slice): `data` is non-null and `len` bytes from it
            // are readable for the whole life of `self` — guaranteed by
            // `mmap` (released only in `Drop`, which borrowck keeps
            // after this borrow) or by the heap owner (the `Box` is a
            // field and frees only in `Drop`).
            unsafe { std::slice::from_raw_parts(self.data, self.len) }
        }
    }

    // SAFETY (Send/Sync): the mapping is immutable read-only memory. No
    // method writes through `data`; the pointer never escapes except as
    // `&[u8]` borrows tied to `&self`. Moving or sharing the owner
    // across threads is exactly the mmap contract for
    // `PROT_READ | MAP_PRIVATE` pages — the same guarantee libpinyin's
    // MemoryChunk relies on.
    unsafe impl Send for MappedFile {}
    unsafe impl Sync for MappedFile {}

    impl Drop for MappedFile {
        fn drop(&mut self) {
            #[cfg(unix)]
            if self.heap.is_none() {
                // SAFETY (munmap): the address and length are exactly
                // what `mmap` returned (never modified in between), and
                // `Drop` runs only after every `as_slice` borrow has
                // ended, so no view can outlive the unmap.
                unsafe { munmap(self.data.cast_mut().cast(), self.len) };
            }
            // Non-Unix builds have no mapping to release — the heap
            // fallback frees itself. Reading it here keeps the field
            // live where the munmap branch is compiled out.
            #[cfg(not(unix))]
            let _frees_itself = &self.heap;
        }
    }

    // The platform mmap surface, declared verbatim instead of through a
    // dependency (adding one is a hard-forbid without an ask). Every
    // Unix passes `off_t` as the widest integer; the constants live at
    // the call site above.
    #[cfg(unix)]
    unsafe extern "C" {
        fn mmap(
            addr: *mut std::ffi::c_void,
            len: usize,
            prot: i32,
            flags: i32,
            fd: i32,
            offset: i64,
        ) -> *mut std::ffi::c_void;
        fn munmap(addr: *mut std::ffi::c_void, len: usize) -> i32;
    }
}

// ── phrase item view ────────────────────────────────────────────────

/// One pronunciation of a phrase item: the packed `ChewingKey` bytes
/// (`2·phrase_length` of them — opaque at this layer; P2's chewing
/// reader owns their bitfield semantics) and the pronunciation's
/// frequency.
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
/// the copy: `get_phrase_length`/`get_n_pronunciation`/
/// `get_unigram_frequency`/`get_phrase_string`/
/// `get_nth_pronunciation` become reads below.
#[derive(Clone, Copy)]
pub struct PhraseItemView<'a> {
    bytes: &'a [u8],
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
    fn phrase_chars(&self) -> impl Iterator<Item = Option<char>> + '_ {
        let length = self.phrase_length();
        (0..length).map(move |index| {
            let offset = ITEM_HEADER_SIZE.checked_add(4 * index)?;
            let code = u32_at(self.bytes, offset)?;
            char::from_u32(code)
        })
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

/// One per-library phrase index over a mapped libpinyin `*.bin` chunk
/// file — the `SubPhraseIndex` of `FacadePhraseIndex`, upstream's
/// mmap-backed half of the system data.
pub struct PhraseLibrary {
    file: map::MappedFile,
    /// The payload after the 8-byte chunk header.
    payload: std::ops::Range<usize>,
    /// `SubPhraseIndex::m_total_freq`.
    total_freq: u32,
    /// Offset-array slots, `[index_one, index_two)` in payload bytes.
    offsets: Range<usize>,
    /// The entry area, `[index_two, index_three)` in payload bytes.
    content: Range<usize>,
}

impl PhraseLibrary {
    /// Maps and validates one library file (`MemoryChunk::mmap` +
    /// `SubPhraseIndex::load`).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError`] when the file cannot be read, the chunk
    /// header or checksum does not verify, or the sub-index layout is
    /// not what the upstream loader accepts.
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        let file = map::MappedFile::open(path)?;
        let bytes = file.as_slice();
        let bad = |message: &str| LibraryError::Format(message.to_owned());

        // MemoryChunk::mmap's checks: the header's length word must be
        // the payload length, and the checksum must verify.
        let declared_len = u32_at(bytes, 0).ok_or_else(|| bad("chunk shorter than its header"))?;
        if bytes.len() < CHUNK_HEADER_SIZE
            || declared_len as usize != bytes.len() - CHUNK_HEADER_SIZE
        {
            return Err(bad("chunk length word does not match the file"));
        }
        let payload = CHUNK_HEADER_SIZE..bytes.len();
        let checksum = u32_at(bytes, 4).ok_or_else(|| bad("chunk checksum unreadable"))?;
        if checksum != chunk_checksum(&bytes[payload.clone()]) {
            return Err(bad("chunk checksum mismatch"));
        }

        // SubPhraseIndex::load's checks: the four leading words, the
        // separator at the end of each, and bounds.
        let payload_bytes = &bytes[payload.clone()];
        let total_freq = u32_at(payload_bytes, 0).ok_or_else(|| bad("payload too short"))?;
        let index_one = u32_at(payload_bytes, 4).ok_or_else(|| bad("payload too short"))? as usize;
        let index_two = u32_at(payload_bytes, 8).ok_or_else(|| bad("payload too short"))? as usize;
        let index_three =
            u32_at(payload_bytes, 12).ok_or_else(|| bad("payload too short"))? as usize;
        if payload_bytes.get(16).copied() != Some(SEPARATOR) {
            return Err(bad("missing separator after the sub-index header"));
        }
        // `index_one` is 17 in every upstream file (4 words + separator)
        // and carries no alignment requirement — the loader reads u32s
        // at unaligned offsets; only the array's length is u32-sized.
        if index_two < index_one || index_three < index_two {
            return Err(bad("sub-index sections out of order"));
        }
        if index_three > payload_bytes.len() {
            return Err(bad("sub-index extends past the payload"));
        }
        if payload_bytes.get(index_two.wrapping_sub(1)).copied() != Some(SEPARATOR)
            || payload_bytes.get(index_three.wrapping_sub(1)).copied() != Some(SEPARATOR)
        {
            return Err(bad("missing section separator"));
        }
        if !(index_two - index_one - 1).is_multiple_of(4) {
            return Err(bad("offset array is not u32-sized"));
        }

        Ok(Self {
            file,
            payload,
            total_freq,
            // load's sub-chunk views: the offset array runs to the
            // byte before its separator, the entry area to the byte
            // before the final separator (`phrase_index.cpp:356-359`).
            offsets: index_one..index_two - 1,
            content: index_two..index_three - 1,
        })
    }

    /// The payload bytes this library covers (for tests and tooling).
    fn payload(&self) -> &[u8] {
        &self.file.as_slice()[self.payload.clone()]
    }

    /// `SubPhraseIndex::get_phrase_index_total_freq`.
    #[must_use]
    pub fn total_freq(&self) -> u32 {
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
        1..end.max(1) as u32
    }

    /// The offset-array entry of a slot, or `None` outside the array.
    fn slot_offset(&self, slot: usize) -> Option<u32> {
        u32_at(self.payload(), self.offsets.start.checked_add(4 * slot)?)
            .filter(|_| self.offsets.start + 4 * slot + 4 <= self.offsets.end)
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

    /// The item behind a raw library-local slot.
    #[must_use]
    pub fn item_at_slot(&self, slot: usize) -> Option<PhraseItemView<'_>> {
        let offset = self.slot_offset(slot)? as usize;
        if offset == 0 {
            return None;
        }
        let content = &self.payload()[self.content.clone()];
        // Offsets are 0-based into the entry area; the first item sits
        // at 8 (`SubPhraseIndex::add_phrase_item` reserves bytes 0..8
        // so 0 stays the "no item" sentinel).
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

    /// Every `(slot, item)` pair with a resident item, slot order — a
    /// tooling/test surface (upstream never iterates; its reverse
    /// lookup goes through the phrase DBM).
    pub fn items(&self) -> impl Iterator<Item = (u32, PhraseItemView<'_>)> + '_ {
        let slots = (self.offsets.end - self.offsets.start) / 4;
        (0..slots).filter_map(move |slot| self.item_at_slot(slot).map(|item| (slot as u32, item)))
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
