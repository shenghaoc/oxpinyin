//! Reader for the per-library phrase-index files (the `MemoryChunk` +
//! `SubPhraseIndex` pair).
//!
//! The layout record is `docs/findings/data-formats.md` §2. These
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

/// `c_separate` (`novel_types.h:126`).
pub const SEPARATOR: u8 = b'#';

/// `SubPhraseIndex::store`'s `index_one`: the payload's `total_freq` plus
/// the three section offsets (`u32×4`) and the separator that closes
/// them, so the offset array starts at byte 17.
///
/// Constant in every
/// upstream file, and no alignment requirement rides on it — the loader
/// reads its `u32`s at unaligned offsets.
pub const INDEX_ONE: u32 = 17;

/// The entry area's first item offset.
///
/// `SubPhraseIndex::add_phrase_item`
/// bumps a zero content size to 8 on the first item, reserving bytes
/// `0..8` so that `0` stays the offset array's "no item" sentinel. A
/// library with no items reserves nothing and has an empty entry area
/// (measured on the pin's own `gen_binary_files` output for an empty
/// `.table`).
pub const FIRST_ITEM_OFFSET: u32 = 8;

/// `phrase_item_header` (`phrase_index.h:56`): `{u8 phrase_length,
/// u8 n_pronunciations, u32 unigram}` ahead of every item's UCS-4 text.
pub const ITEM_HEADER_SIZE: usize = 6;

/// `sizeof(ChewingKey)` — a 16-bit bitfield (`chewing_key.h:41`), the
/// width one pronunciation key occupies inside an item.
pub const CHEWING_KEY_SIZE: usize = size_of::<u16>();

mod write;
pub use write::{ChunkItem, build_chunk, encode_phrase_item};
pub(crate) use write::{decode_phrase_item, sub_phrase_index_sections};

use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[cfg(not(target_endian = "little"))]
compile_error!(
    "phrase_library: the MemoryChunk format uses host-endian fields; \
     this reader decodes them as little-endian. Big-endian targets are \
     not supported."
);

use crate::data::chunk_format::{CHUNK_HEADER_SIZE, chunk_checksum};

use crate::vocab::{PHRASE_MASK, PhraseText};

// SubPhraseIndex::load returns false at phrase_index.cpp:354–356. Its callers retain the
// empty sub-index. The g_return_val_if_fail critical is omitted
// (docs/findings/upstream-divergences.md#phrase-library-layout-faults-write-no-critical).
/// A failed sub-index layout check whose input bytes lie inside the chunk. An access
/// beyond the supplied chunk instead requires [`LibraryError::Format`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutFault {
    /// `m_total_freq` as the pin read it before the failing check — 0 when
    /// the four-word header itself is short.
    pub total_freq: u32,
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
    /// refused the layout through a check whose `false` the pin ignores. The caller loads an empty sub-index and reports the
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
    pub(crate) fn unmappable_path(&self) -> Option<&Path> {
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
            Self::Layout(_) => f.write_str("phrase library layout error: separator check failed"),
            Self::Unmappable { cause, .. } => cause.fmt(f),
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

/// Reads a little-endian `u32` at `offset`, or `None` past the end.
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(*bytes.get(offset..)?.first_chunk()?))
}

// ── phrase item view ────────────────────────────────────────────────

/// One pronunciation of a phrase item: packed `ChewingKey` bytes and frequency.
#[derive(Clone, Copy)]
pub struct PronunciationView<'a> {
    /// The packed `ChewingKey[L]` bytes, in stored order.
    pub keys: &'a [u8],
    /// This pronunciation's frequency.
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
    /// Read a complete item from its wire bytes. Refuse a truncated header, text or reading.
    #[must_use]
    pub fn from_bytes(bytes: &'a [u8]) -> Option<Self> {
        let view = Self {
            bytes: bytes.get(..ITEM_HEADER_SIZE)?,
        };
        Some(Self {
            bytes: bytes.get(..view.total_size()?)?,
        })
    }

    // PhraseItem field accessors, phrase_index.cpp:180–205.
    /// Borrows an item beginning at `bytes`. The six-byte header must be present. A view can
    /// end before the last declared field, so check each field before using it.
    #[must_use]
    pub fn fields(bytes: &'a [u8]) -> Option<Self> {
        let header = bytes.get(..ITEM_HEADER_SIZE)?;
        let total = PhraseItemView { bytes: header }.total_size()?;
        Some(Self {
            bytes: bytes.get(..total).unwrap_or(bytes),
        })
    }

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

    // PhraseItem::get_phrase_string and g_ucs4_to_utf8, lookup/lookup.cpp:39–68.
    /// Reads the declared UCS-4 words for one phrase. Every word must be readable. Before the
    /// first zero, values must be below `0x8000_0000`. The first zero ends the outward text,
    /// and accepted non-scalar words retain their stored representation.
    #[must_use]
    pub fn phrase_text(&self) -> Option<PhraseText> {
        PhraseText::from_stored_words(self.phrase_codes())
    }

    /// Whether every declared word of the text lies inside the item's bytes.
    #[must_use]
    pub(crate) fn phrase_words_present(&self) -> bool {
        self.phrase_codes().all(|word| word.is_some())
    }

    /// The phrase's stored UCS-4 words in order, `None` where the item's
    /// bytes end before a declared word.
    pub fn phrase_codes(&self) -> impl Iterator<Item = Option<u32>> + '_ {
        let length = self.phrase_length();
        (0..length).map(move |index| {
            let offset = ITEM_HEADER_SIZE.checked_add(4 * index)?;
            u32_at(self.bytes, offset)
        })
    }

    // PhraseItem::get_nth_pronunciation, phrase_index.cpp:180–205.
    /// Borrows one reading's packed keys and frequency. Supply a stored reading index whose
    /// keys and frequency are present.
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

    /// Visits every declared pronunciation position, in stored order. An incomplete reading,
    /// whose packed keys or frequency the item's bytes do not hold, still yields a placeholder
    /// with empty keys and zero frequency. [`Self::pronunciation`] returns `None` for such a
    /// reading, so it tells a missing reading apart from a stored one.
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
fn map_chunk(
    path: &Path,
    defer_header_fault: bool,
) -> Result<crate::input::chunk::Mapping, LibraryError> {
    use crate::input::chunk::MappingSource;

    let unmappable = |cause: LibraryError| LibraryError::Unmappable {
        path: path.to_path_buf(),
        cause: Box::new(cause),
    };
    let bad = |message: &str| unmappable(LibraryError::Format(message.to_owned()));
    let mut source =
        MappingSource::open(path).map_err(|error| unmappable(LibraryError::Io(error)))?;
    let file_size = source.file_size();
    let (length, checksum) =
        map_header(path, file_size, defer_header_fault, || source.read_word())?;
    let declared = u32::from_le_bytes(length);
    let checksum = u32::from_le_bytes(checksum);
    let payload_len = (file_size - CHUNK_HEADER_SIZE as u64) as u32;
    if declared != payload_len {
        return Err(bad("chunk length word does not match the file"));
    }
    let pending = source
        .map()
        .map_err(|error| unmappable(LibraryError::Io(error)))?;
    let end = CHUNK_HEADER_SIZE
        .checked_add(payload_len as usize)
        .ok_or_else(|| bad("chunk extent does not fit this platform"))?;
    let payload = pending
        .as_slice()
        .get(CHUNK_HEADER_SIZE..end)
        .ok_or_else(|| bad("chunk shorter than its declared extent"))?;
    if checksum != chunk_checksum(payload) {
        return Err(bad("chunk checksum mismatch"));
    }
    Ok(pending.finish())
}

/// Whether the pin writes its `mmap %s failed!` line for a row that names no
/// chunk. `g_build_filename(dir, NULL, NULL)` is `dir`, so `MemoryChunk::mmap`
/// opens the directory itself (`pinyin.cpp:252-256`, `:286-290`,
/// `zhuyin.cpp:196-200` at 074a2219). Where the directory reads as a chunk,
/// the `read` of its length word fails and the pin asserts
/// (`memory_chunk.h:493`) before writing anything. Where `lseek(SEEK_END)`
/// fails or answers under the header (tmpfs), `mmap` refuses it
/// (`memory_chunk.h:486-489`), the pin writes `mmap %s failed!` naming `dir`
/// and then dies of SIGSEGV on the empty chunk (`phrase_index.cpp:354`). The
/// answer and warning stay the abort site's either way. The boundary says
/// whether this platform lets the chunk loader open a directory; where it
/// does not, the pin reaches neither branch.
pub(crate) fn writes_null_chunk_line(
    dir: &Path,
    reads_as_a_chunk: impl FnOnce(&Path) -> bool,
) -> bool {
    crate::input::chunk::opens_directories() && !reads_as_a_chunk(dir)
}

pub(crate) fn null_chunk_writes_mmap_line(dir: &Path) -> bool {
    writes_null_chunk_line(dir, directory_reads_as_a_chunk)
}

fn map_header(
    path: &Path,
    file_size: u64,
    defer_header_fault: bool,
    mut read_word: impl FnMut() -> std::io::Result<[u8; 4]>,
) -> Result<([u8; 4], [u8; 4]), LibraryError> {
    if file_size < CHUNK_HEADER_SIZE as u64 {
        return Err(LibraryError::Unmappable {
            path: path.to_path_buf(),
            cause: Box::new(LibraryError::Format(
                "chunk shorter than its header".to_owned(),
            )),
        });
    }
    let mut read = || match read_word() {
        Ok(word) => Ok(word),
        Err(_) if defer_header_fault => Err(LibraryError::Row(RowFault::ChunkHeaderRead)),
        Err(_) => std::process::abort(),
    };
    let length = read()?;
    let checksum = read()?;
    Ok((length, checksum))
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
    directory_size_reads_as_a_chunk(directory_end(dir))
}

pub(crate) fn directory_size_reads_as_a_chunk(size: std::io::Result<u64>) -> bool {
    size.is_ok_and(|size| size >= CHUNK_HEADER_SIZE as u64)
}

/// Whether `MemoryChunk::mmap` accepts the file at `path` now, keeping
/// nothing: the pin maps a system library again on every save, mask-out and
/// reload, and writes `mmap %s failed!` when it cannot.
///
/// # Errors
///
/// [`LibraryError::Unmappable`] when the chunk does not map.
///
/// # Aborts
/// A failed header word read aborts at the pin's assertion.
pub fn verify_chunk(path: &Path) -> Result<(), LibraryError> {
    map_chunk(path, false).map(drop)
}

/// A writable private view of one library's offset array and phrase-content
/// area. Changes never reach the system file or other library views.
pub(crate) struct PhraseContent {
    mapping: crate::input::chunk::Mapping,
    offsets: Range<usize>,
    area: Range<usize>,
    bounds: ItemBounds,
}

impl PhraseContent {
    /// Borrow the current content bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.mapping.as_slice()[self.area.clone()]
    }

    /// Borrow the offset array: little-endian `u32` slots, in stored order.
    #[must_use]
    pub fn offsets(&self) -> &[u8] {
        &self.mapping.as_slice()[self.offsets.clone()]
    }

    pub(crate) fn slot_offset(&self, slot: u32) -> Option<u32> {
        u32_at(self.offsets(), (slot as usize).checked_mul(4)?).filter(|&offset| offset != 0)
    }

    pub(crate) fn item_fields(&self, slot: u32) -> Option<&[u8]> {
        let offset = self.slot_offset(slot)?;
        self.bounds
            .record(self.offsets(), self.as_slice(), slot as usize, offset)
            .filter(|record| record.len() >= 2)
    }
}

/// End of each indexed record in physical order. Build it on first item access,
/// keeping subsequent lookups constant time and allowing out-of-order slots.
#[derive(Default)]
struct ItemBounds(std::cell::OnceCell<Box<[u32]>>);

impl ItemBounds {
    fn record<'a>(
        &self,
        offsets: &[u8],
        content: &'a [u8],
        slot: usize,
        start: u32,
    ) -> Option<&'a [u8]> {
        if start == 0 || start as usize >= content.len() {
            return None;
        }
        let ends = self
            .0
            .get_or_init(|| Self::record_ends(offsets, content.len()));
        content.get(start as usize..*ends.get(slot)? as usize)
    }

    fn record_ends(offsets: &[u8], content_len: usize) -> Box<[u32]> {
        let offsets = offsets.as_chunks::<4>().0;
        let mut ends = vec![0; offsets.len()];
        let mut physical: Vec<_> = offsets
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .enumerate()
            .filter(|&(_, start)| start != 0 && (start as usize) < content_len)
            .collect();
        physical.sort_unstable_by_key(|&(_, start)| start);
        let mut next_start = content_len as u32;
        for group in physical
            .chunk_by(|&(_, left), &(_, right)| left == right)
            .rev()
        {
            for &(slot, _) in group {
                ends[slot] = next_start;
            }
            next_start = group[0].1;
        }
        ends.into_boxed_slice()
    }
}

use backing::LibraryBacking;
pub(crate) use backing::LibraryBytes;

pub(crate) mod backing {
    /// Owned or mapped input to the common phrase-library layout reader.
    pub struct LibraryBytes(pub(super) LibraryBacking);

    pub(super) enum LibraryBacking {
        Mapped(crate::input::chunk::Mapping),
        Owned(Vec<u8>),
    }

    impl From<Vec<u8>> for LibraryBytes {
        fn from(bytes: Vec<u8>) -> Self {
            Self(LibraryBacking::Owned(bytes))
        }
    }

    impl LibraryBytes {
        pub(super) fn as_slice(&self) -> &[u8] {
            match &self.0 {
                LibraryBacking::Mapped(mapping) => mapping.as_slice(),
                LibraryBacking::Owned(payload) => payload,
            }
        }
    }
}

/// One per-library phrase index over a mapped libpinyin `*.bin` chunk
/// file — the `SubPhraseIndex` of `FacadePhraseIndex`, upstream's
/// mmap-backed half of the system data.
pub struct PhraseLibrary {
    file: Option<LibraryBytes>,
    source_path: Option<PathBuf>,
    /// The payload after the 8-byte chunk header.
    payload: std::ops::Range<usize>,
    /// `SubPhraseIndex::m_total_freq`.
    total_freq: u32,
    /// Offset-array slots, `[index_one, index_two)` in payload bytes.
    offsets: Range<usize>,
    /// The entry area, `[index_two, index_three)` in payload bytes.
    content: Range<usize>,
    bounds: ItemBounds,
    /// True for the empty sub-index the pin leaves behind when its
    /// `SubPhraseIndex::load` refused the layout; the save path re-checks
    /// such a library exactly as the pin re-maps it.
    layout_failed: bool,
    /// Live mapped item sum, filled at first compact.
    item_total: std::cell::OnceCell<u32>,
    /// `get_range`'s end at the moment the mapping moved to its live owner.
    detached_range_end: Option<u32>,
}

impl PhraseLibrary {
    /// The wrapping sum of mapped item frequencies, excluding slot zero.
    /// Compaction queries share one scan after the first request.
    #[must_use]
    pub(crate) fn item_total(&self) -> u32 {
        *self.item_total.get_or_init(|| {
            self.items()
                .filter(|(slot, _)| *slot >= 1)
                .fold(0_u32, |total, (_, item)| total.wrapping_add(item.unigram()))
        })
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
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        let library = Self::open_retaining_layout(path)?;
        if library.layout_failed {
            return Err(LibraryError::Layout(LayoutFault {
                total_freq: library.total_freq,
            }));
        }
        Ok(library)
    }

    /// Map a chunk and retain its owner when `SubPhraseIndex::load` refuses
    /// an in-bounds layout check, as the pin's empty sub-index does.
    ///
    /// # Errors
    /// Returns the same mapping and out-of-bounds layout errors as [`Self::open`].
    ///
    /// # Aborts
    /// A failed header word read aborts at the pin's assertion.
    pub(crate) fn open_retaining_layout(path: &Path) -> Result<Self, LibraryError> {
        Self::open_with_header_policy(path, false)
    }

    /// Defer a header read fault until the mask walk has replayed preceding libraries.
    pub(crate) fn open_for_mask_prefetch(path: &Path) -> Result<Self, LibraryError> {
        Self::open_with_header_policy(path, true)
    }

    fn open_with_header_policy(
        path: &Path,
        defer_header_fault: bool,
    ) -> Result<Self, LibraryError> {
        let file = LibraryBytes(LibraryBacking::Mapped(map_chunk(path, defer_header_fault)?));
        let bytes = file.as_slice();
        let payload = CHUNK_HEADER_SIZE
            ..CHUNK_HEADER_SIZE + ((bytes.len() - CHUNK_HEADER_SIZE) as u32 as usize);
        Self::from_bytes(file, payload, Some(path.to_path_buf()))
    }

    /// Validates the phrase-library layout over the selected payload.
    ///
    /// # Errors
    /// Returns an error when the layout exceeds the supplied bytes.
    #[doc(hidden)]
    pub fn from_bytes(
        file: LibraryBytes,
        payload: Range<usize>,
        source_path: Option<PathBuf>,
    ) -> Result<Self, LibraryError> {
        let bytes = file.as_slice();
        let bad = |message: &str| LibraryError::Format(message.to_owned());
        let payload_bytes = &bytes[payload.clone()];
        // The pin's `get_content` leaves a failed read's destination at 0;
        // `m_total_freq` is read before the layout checks.
        let total_freq = u32_at(payload_bytes, 0).unwrap_or(0);
        let layout = |file, payload| Self {
            file: Some(file),
            source_path: source_path.clone(),
            payload,
            total_freq,
            offsets: 0..0,
            content: 0..0,
            bounds: ItemBounds::default(),
            layout_failed: true,
            item_total: std::cell::OnceCell::new(),
            detached_range_end: None,
        };

        // `SubPhraseIndex::load`: the four words, then the separator at
        // payload[16], then `index_two - 1`, then `index_three - 1`.
        // A payload of 16 bytes or fewer makes the first read leave the
        // chunk; the failure is the same one, reproduced without the read.
        if payload_bytes.len() < INDEX_ONE as usize
            || payload_bytes[INDEX_ONE as usize - 1] != SEPARATOR
        {
            return Ok(layout(file, payload));
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
            return Ok(layout(file, payload));
        }
        let third = index_three
            .checked_sub(1)
            .filter(|&at| at < payload_bytes.len())
            .ok_or_else(|| bad("sub-index entry-area separator out of bounds"))?;
        if payload_bytes[third] != SEPARATOR {
            return Ok(layout(file, payload));
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
            file: Some(file),
            source_path,
            payload,
            total_freq,
            // load's sub-chunk views: the offset array runs to the
            // byte before its separator, the entry area to the byte
            // before the final separator (`phrase_index.cpp:356-359`).
            offsets: index_one..index_two - 1,
            content: index_two..index_three - 1,
            bounds: ItemBounds::default(),
            layout_failed: false,
            item_total: std::cell::OnceCell::new(),
            detached_range_end: None,
        })
    }

    /// The payload bytes this library covers (for tests and tooling).
    fn payload(&self) -> &[u8] {
        self.file
            .as_ref()
            .map_or(&[] as &[u8], |mapping| mapping.as_slice())
            .get(self.payload.clone())
            .unwrap_or(&[])
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
        if let Some(range_end) = self.detached_range_end {
            return 1..range_end;
        }
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

    /// Read the count byte when the record contains both leading header bytes.
    #[must_use]
    pub(crate) fn pronunciation_count(&self, token: u32) -> Option<u32> {
        self.item_bytes((token & PHRASE_MASK) as usize)?
            .get(1)
            .copied()
            .map(u32::from)
    }

    /// The item's readable fields. Each field must fit in the indexed record.
    /// Whole-item consumers use [`Self::item`].
    #[must_use]
    pub(crate) fn item_fields(&self, token: u32) -> Option<PhraseItemView<'_>> {
        PhraseItemView::fields(self.item_bytes((token & PHRASE_MASK) as usize)?)
    }

    /// The item behind a raw library-local slot.
    #[must_use]
    pub(crate) fn item_at_slot(&self, slot: usize) -> Option<PhraseItemView<'_>> {
        PhraseItemView::from_bytes(self.item_bytes(slot)?)
    }

    pub(crate) fn item_bytes(&self, slot: usize) -> Option<&[u8]> {
        let offset = self.slot_offset(slot)?;
        let content = self.payload().get(self.content.clone())?;
        self.bounds
            .record(self.offset_array(), content, slot, offset)
    }

    /// The offset array: little-endian `u32` slots, `SubPhraseIndex`'s
    /// `m_phrase_index` as loaded.
    #[must_use]
    pub fn offset_array(&self) -> &[u8] {
        &self.payload()[self.offsets.clone()]
    }

    /// Path from which this chunk was mapped.
    #[must_use]
    pub(crate) fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    /// Transfer the sole mapping to its live sub-index without scanning
    /// slots. The live owner reads the loaded offsets from this mapping.
    pub(crate) fn take_live_content(&mut self) -> Option<PhraseContent> {
        if self.detached_range_end.is_some() {
            return None;
        }
        let range_end = self.token_range().end;
        let Some(LibraryBytes(LibraryBacking::Mapped(_))) = self.file.as_ref() else {
            return None;
        };
        let LibraryBytes(LibraryBacking::Mapped(mapping)) = self.file.take()? else {
            return None;
        };
        self.detached_range_end = Some(range_end);
        Some(PhraseContent {
            mapping,
            offsets: (self.payload.start + self.offsets.start)
                ..(self.payload.start + self.offsets.end),
            area: (self.payload.start + self.content.start)
                ..(self.payload.start + self.content.end),
            bounds: std::mem::take(&mut self.bounds),
        })
    }

    /// Whether the chunk's mapping has moved to its live owner.
    #[must_use]
    pub const fn is_detached(&self) -> bool {
        self.detached_range_end.is_some()
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

#[cfg(test)]
mod tests;
