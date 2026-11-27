//! Loads and saves the user's native phrase libraries, indexes, bigram data and profile
//! marker. [`crate::data::user_files`] describes the file formats. The user indexes
//! retain the records actually written, including which readings were indexed.
//!
//! [`UserConfLaw`] selects the save order. Pinyin writes temporary files before its
//! rename pass. Zhuyin renames each file as it goes. A hard failure can leave temporary
//! files or a mix of old and new files. Finalization applies the selected facade's marker
//! policy.

// pinyin.cpp:569–582,933–1130; zhuyin.cpp:585–698. Bigram::load_db copies into memory,
// save_db writes a fresh file. See docs/findings/user-store.md §4.

#[cfg(test)]
use crate::data::phrase_library::build_chunk;
use crate::data::phrase_library::{LibraryError, RowFault};
#[cfg(test)]
use crate::data::row_format::pinyin_index::PinyinIndexItem;
#[cfg(test)]
use crate::parser::ChewingKey;
#[cfg(test)]
use crate::user::UserState;

use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::data::chunk_format::build_memory_chunk;
use crate::data::chunk_format::{CHUNK_HEADER_SIZE, finish_memory_chunk};
use crate::data::phrase_library::ChunkItem;
use crate::data::phrase_library::{CHEWING_KEY_SIZE, ITEM_HEADER_SIZE};
#[cfg(test)]
use crate::data::user_files::encode_log_records;
#[cfg(test)]
use crate::data::user_files::read_chunk_payload;
use crate::data::user_files::{
    ChunkPayloadError, LogEnd, LogFault, LogRecord, SystemVersions, UserDbm, UserFileLayout,
    UserTableInfo, append_log_record, get_open_counter, load_chunk_payload, log_records,
    try_load_chunk_payload,
};
use crate::data::{FileName, PhraseLibraries};
use crate::input::{build_path, rename, unlink};
use crate::store::{DefaultStore, DefaultUserBigramDb, RawReadStore, StoreError, UserBigramDb};
use crate::vocab::PHRASE_MASK;

use crate::user::replay::{JournalFault, Merge, ReplayedLibrary, Stop, raw_unigram};
use crate::user::user_index::UserIndex;
use crate::user::user_library::UserLibrary;
use crate::user::user_phrase_table::UserPhraseTable;

/// `USER_TABLE_INFO` (`pinyin_internal.h:56`).
const USER_CONF: &str = "user.conf";

/// `UserTableInfo::load` of `filename` (`pinyin.cpp:178`, `zhuyin.cpp:132`),
/// as read by `check_format`.
///
/// `None` is the load's `false`: `fopen` failed, after the pin's `open
/// %s failed.` line, or a version directive did not convert
/// (`table_info.cpp:330-348`). Upstream then runs its conformance check
/// against the reset defaults, so the profile reads as non-conforming and is
/// wiped — the session load does the same.
///
/// # Aborts
///
/// An unknown `database format:` aborts in the reader before this function
/// can judge the profile or change a file (`table_info.cpp:122-133`).
fn load_user_conf(filename: &Path) -> Option<UserTableInfo> {
    UserTableInfo::load_path(filename).ok()
}

/// Which facade's `user.conf` lifecycle a profile follows. libpinyin and
/// libzhuyin read and write the same marker through the same
/// `UserTableInfo`, but at different points and with different counters,
/// so the facade that opens a profile names its law and every write below
/// follows it (the load, the reporting save's counter, [`fini`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserConfLaw {
    /// libpinyin: every `pinyin_init` raises the open counter and writes
    /// the marker (`check_format`, `pinyin.cpp:185-187`); `pinyin_save`
    /// writes the context's counter back (`mark_version`, `:1143`,
    /// `:220-232`); `pinyin_fini` lowers it and writes again, with or
    /// without a save before it (`:1194-1200`). A process that dies
    /// between init and fini leaves the counter raised, and the init that
    /// reads it past `OPEN_COUNTER_LIMIT` wipes the profile.
    Pinyin,
    /// libzhuyin: `zhuyin_init` only reads the marker (`check_format`,
    /// `zhuyin.cpp:126-162`: conformance and the wipe, no counter step and
    /// no write); `zhuyin_save` writes a fresh `UserTableInfo`, so its
    /// counter is 0 (`mark_version`, `:164-176`, called at `:695`);
    /// `zhuyin_fini` writes nothing (`:741-757`).
    Zhuyin,
}

/// One system library's original state — the shipped chunk as loaded,
/// the diff base of its `.dbin` log.
#[derive(Clone, Copy, Debug)]
pub struct SystemLibrary<
    M = crate::data::phrase_library::PhraseLibrary,
    I = BTreeMap<u32, ChunkItem>,
> {
    /// The chunk's `total_freq`.
    pub total: u32,
    /// The pin's trimmed allocation range end (phrase_index.cpp:624-646).
    pub range_end: u32,
    /// Items by slot (`token & PHRASE_MASK`).
    pub items: I,
    /// The baseline mapping; read interfaces borrow it for one operation.
    pub mapped: Option<M>,
}

/// What a save's current-on-disk remap says about one selected system
/// library. The save always diffs the live sub-index against the file as it
/// is on disk now, never against the chunk the session loaded.
pub enum SaveBaseline {
    /// There is no file to map again (a transient or fixture session): the
    /// retained library stands for the file.
    Keep,
    /// The chunk as it is on disk now, mapped again: the old side of the
    /// diff against the live sub-index (`SubPhraseIndex::diff`).
    Changed(SystemLibrary),
    /// The library is unloaded: `get_range` answers
    /// `ERROR_NO_SUB_PHRASE_INDEX`, so `_write_files` and `_rename_files`
    /// skip it and its file on disk stays as it is (074a2219
    /// `pinyin.cpp:928-933`, `:1032-1035`; `zhuyin.cpp:563-568`).
    Unloaded,
    /// `MemoryChunk::mmap` refuses the library's system file at this point of
    /// the loop (`pinyin.cpp:956`, `zhuyin.cpp:589`). The
    /// pin writes `mmap %s failed!` and dies; the registered safe outcome
    /// stops the save here: the earlier libraries' files stay as written
    /// (libzhuyin has renamed them), nothing later is written or renamed, no
    /// marker is written, the session stays dirty and the save answers
    /// `false`.
    Unmappable,
}

/// Why [`read_save_baseline`] could not give a baseline for a
/// system library the save maps again at its `_write_files` position
/// (`pinyin.cpp:955`, `zhuyin.cpp:588`).
#[derive(Debug)]
pub(crate) enum SaveRemapFault {
    /// `MemoryChunk::mmap` refuses the file: the pin writes
    /// `mmap %s failed!`.
    Unmappable(PathBuf),
}

/// Read a save baseline while its unique owner performs the operation.
///
/// # Errors
/// The system-file path that cannot be mapped again.
pub(crate) fn read_save_baseline(
    originals: &crate::user::SystemOriginals,
    unloaded: u32,
    nibble: u8,
) -> Result<SaveBaseline, SaveRemapFault> {
    // Nibble 0 is the reserved row the loop never visits.
    if nibble == 0 {
        return Ok(SaveBaseline::Keep);
    }
    // `_write_files` skips a default library whose sub-index is gone
    // (`pinyin.cpp:928-933`, `zhuyin.cpp:563-568`), a `USER_FILE`
    // library included: an unloaded library is neither remapped nor
    // written, and its file on disk stays.
    if unloaded & (1 << nibble) != 0 {
        return Ok(SaveBaseline::Unloaded);
    }
    let Some((path, _)) = originals.phrase_libraries().loaded_path_library(nibble) else {
        return Ok(SaveBaseline::Keep);
    };
    match crate::data::phrase_library::PhraseLibrary::open_retaining_layout(path) {
        Ok(current) => Ok(SaveBaseline::Changed(SystemLibrary::from_library(current))),
        Err(LibraryError::Row(RowFault::ChunkHeaderRead)) => std::process::abort(),
        Err(
            error @ (LibraryError::Io(_)
            | LibraryError::Format(_)
            | LibraryError::Layout(_)
            | LibraryError::Unmappable { .. }
            | LibraryError::Row(RowFault::NullChunkName)),
        ) => match error.unmappable_path() {
            Some(path) => Err(SaveRemapFault::Unmappable(path.to_path_buf())),
            None => Ok(SaveBaseline::Keep),
        },
    }
}

impl<M, I: Default> Default for SystemLibrary<M, I> {
    fn default() -> Self {
        Self {
            total: 0,
            range_end: 0,
            items: I::default(),
            mapped: None,
        }
    }
}

impl SystemLibrary {
    #[cfg(test)]
    pub(crate) fn fixture_copy(&self) -> Self {
        assert!(
            self.mapped.is_none(),
            "mapped fixtures must reopen their file"
        );
        Self {
            total: self.total,
            range_end: self.range_end,
            items: self.items.clone(),
            mapped: None,
        }
    }

    /// A current on-disk chunk as a diff baseline, with items read through
    /// the uniquely owned immutable mapping.
    #[must_use]
    pub(crate) fn from_library(library: crate::data::phrase_library::PhraseLibrary) -> Self {
        Self {
            total: library.total_freq(),
            range_end: library.token_range().end,
            items: BTreeMap::new(),
            mapped: Some(library),
        }
    }

    /// The empty sub-index the pin's failed `SubPhraseIndex::load` leaves:
    /// the local total it read, no items, `get_range` `1..1`.
    #[must_use]
    pub fn failed(total: u32) -> Self {
        Self {
            total,
            range_end: 1,
            items: BTreeMap::new(),
            mapped: None,
        }
    }

    /// Borrows the baseline for one read operation.
    pub fn view(&self) -> SystemLibraryRef<'_> {
        SystemLibrary {
            total: self.total,
            range_end: self.range_end,
            items: &self.items,
            mapped: self.mapped.as_ref(),
        }
    }
}

/// One operation's borrowed baseline metadata and bytes.
pub type SystemLibraryRef<'a> =
    SystemLibrary<&'a crate::data::phrase_library::PhraseLibrary, &'a BTreeMap<u32, ChunkItem>>;

impl<
    M: std::borrow::Borrow<crate::data::phrase_library::PhraseLibrary>,
    I: std::borrow::Borrow<BTreeMap<u32, ChunkItem>>,
> SystemLibrary<M, I>
{
    /// The wrapping item sum `FacadePhraseIndex::compact` rebuilds,
    /// independent of the stored header. Borrow fields without cloning items.
    pub(crate) fn item_total(&self) -> u32 {
        let inline = self
            .items
            .borrow()
            .iter()
            .filter(|(slot, _)| **slot >= 1)
            .fold(0_u32, |total, (_, item)| total.wrapping_add(item.unigram));
        self.mapped.as_ref().map_or(inline, |library| {
            self.items
                .borrow()
                .keys()
                .filter(|slot| **slot >= 1)
                .filter_map(|slot| library.borrow().item(*slot))
                .fold(
                    inline.wrapping_add(library.borrow().item_total()),
                    |total, item| total.wrapping_sub(item.unigram()),
                )
        })
    }

    /// Whether the baseline holds an item under `slot`: an inline item, or a
    /// mapped one. A library whose mapping moved to its live buffer holds
    /// none here.
    pub(crate) fn contains_item(&self, slot: u32) -> bool {
        self.items.borrow().contains_key(&slot)
            || self
                .mapped
                .as_ref()
                .is_some_and(|library| library.borrow().item(slot).is_some())
    }

    pub(crate) fn unigram(&self, slot: u32) -> Option<u32> {
        self.items
            .borrow()
            .get(&slot)
            .map(|item| item.unigram)
            .or_else(|| Some(self.mapped.as_ref()?.borrow().item(slot)?.unigram()))
    }

    pub(crate) fn item(&self, slot: u32) -> Option<ChunkItem> {
        if let Some(item) = self.items.borrow().get(&slot) {
            return Some(item.clone());
        }
        let view = self.mapped.as_ref()?.borrow().item(slot)?;
        Some(ChunkItem {
            phrase: view.phrase_codes().collect::<Option<Vec<u32>>>()?,
            unigram: view.unigram(),
            prons: view
                .pronunciations()
                .map(|pron| {
                    let keys = pron
                        .keys
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|bytes| u16::from_le_bytes(*bytes))
                        .collect();
                    (keys, pron.freq)
                })
                .collect(),
        })
    }

    /// The item `get_phrase_item` finds at `slot` as bytes: the shipped
    /// chunk's header-length bytes, read in place from the mapping; an inline
    /// item (a fixture store) is serialised. `None` when there is no item.
    pub(crate) fn visible_bytes(&self, slot: u32) -> Option<std::borrow::Cow<'_, [u8]>> {
        if let Some(item) = self.items.borrow().get(&slot) {
            return Some(std::borrow::Cow::Owned(journal_item_bytes(item)));
        }
        let view = self.mapped.as_ref()?.borrow().item(slot)?;
        Some(std::borrow::Cow::Borrowed(view.as_bytes()))
    }
}

/// The `SYSTEM_FILE` libraries' originals together with the user dir's
/// file layout (`table.conf`'s user file names): everything a profile's
/// load and save need to know about the system side.
#[derive(Debug)]
pub struct SystemOriginals {
    libraries: crate::user::LibraryTable<SystemLibrary>,
    mapped: PhraseLibraries,
    layout: UserFileLayout,
}

impl SystemOriginals {
    /// `libraries` under `layout`.
    #[must_use]
    pub fn new(
        mut libraries: crate::user::LibraryTable<SystemLibrary>,
        layout: UserFileLayout,
    ) -> Self {
        let mut mapped = PhraseLibraries::empty();
        for (&nibble, library) in &mut libraries {
            if let Some(mapping) = library.mapped.take() {
                let path = mapping
                    .source_path()
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf();
                mapped.insert_mapping(nibble, path, mapping);
            }
        }
        Self {
            libraries,
            mapped,
            layout,
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture_copy(&self) -> Self {
        assert!(
            (0..16).all(|nibble| !self.mapped.is_loaded(nibble)),
            "mapped fixtures must reopen their file"
        );
        Self {
            libraries: self
                .libraries
                .iter()
                .map(|(&nibble, library)| (nibble, library.fixture_copy()))
                .collect(),
            mapped: PhraseLibraries::empty(),
            layout: self.layout.clone(),
        }
    }

    /// Sets library `nibble`'s original total: the mapped total after
    /// `FacadePhraseIndex::compact` rebuilt a layout-failed sub-index.
    pub(crate) fn set_total(&mut self, nibble: u8, total: u32) {
        if let Some(library) = self.libraries.get_mut(&nibble) {
            library.total = total;
        }
    }

    /// The uniquely owned baseline mappings, borrowed by dictionary and LM reads.
    pub const fn phrase_libraries(&self) -> &PhraseLibraries {
        &self.mapped
    }

    /// Exclusive access for a library transition.
    pub(crate) fn phrase_libraries_mut(&mut self) -> &mut PhraseLibraries {
        &mut self.mapped
    }

    /// Transfer each mapped system library's sole byte owner into its live
    /// sub-index; the library entries retain only load-time metadata.
    pub(crate) fn take_live_libraries(
        &mut self,
    ) -> crate::user::LibraryTable<crate::user::replay::ReplayedLibrary> {
        let nibbles: Vec<u8> = self.libraries.keys().copied().collect();
        nibbles
            .into_iter()
            .filter_map(|nibble| Some((nibble, self.take_live_library(nibble)?)))
            .collect()
    }

    /// [`Self::take_live_libraries`] of one library.
    pub(crate) fn take_live_library(
        &mut self,
        nibble: u8,
    ) -> Option<crate::user::replay::ReplayedLibrary> {
        let total = self.libraries.get(&nibble)?.total;
        let content = self.mapped.take_live_content(nibble)?;
        Some(crate::user::replay::ReplayedLibrary::from_mapping(
            content, total,
        ))
    }

    /// Maps one default library from `system_dir` where the init loop
    /// reaches it, and records its metadata if the layout logs it.
    pub(crate) fn map_library(
        &mut self,
        nibble: u8,
        system_dir: &Path,
        file: &FileName,
        unmapped: &mut dyn FnMut(&Path),
    ) -> Result<(), LibraryError> {
        self.mapped
            .map_library(nibble, system_dir, file.as_ref(), unmapped)?;
        if self.layout.system_logs().iter().any(|(n, _)| *n == nibble)
            && let Some(library) = library_metadata(&self.mapped, nibble)
        {
            self.libraries.insert(nibble, library);
        }
        Ok(())
    }

    /// One operation's original library view.
    pub fn get(&self, nibble: &u8) -> Option<SystemLibraryRef<'_>> {
        let library = self.libraries.get(nibble)?;
        Some(SystemLibrary {
            total: library.total,
            range_end: library.range_end,
            items: &library.items,
            mapped: library.mapped.as_ref().or_else(|| {
                self.mapped
                    .library(crate::vocab::phrase_index_make_token(*nibble, 0))
            }),
        })
    }

    /// Original library views, in nibble order.
    pub fn iter(&self) -> impl Iterator<Item = (&u8, SystemLibraryRef<'_>)> {
        self.libraries.iter().map(|(nibble, library)| {
            (
                nibble,
                SystemLibrary {
                    total: library.total,
                    range_end: library.range_end,
                    items: &library.items,
                    mapped: library.mapped.as_ref().or_else(|| {
                        self.mapped
                            .library(crate::vocab::phrase_index_make_token(*nibble, 0))
                    }),
                },
            )
        })
    }

    /// Install an opened file with its remap path.
    pub(crate) fn insert_mapping(
        &mut self,
        nibble: u8,
        path: PathBuf,
        library: crate::data::phrase_library::PhraseLibrary,
    ) {
        self.libraries.insert(
            nibble,
            SystemLibrary {
                total: library.total_freq(),
                range_end: library.token_range().end,
                items: Default::default(),
                mapped: None,
            },
        );
        self.mapped.insert_mapping(nibble, path, library);
    }

    /// Publish dictionary remaps when replay has not replaced its originals.
    /// Retain the journal baseline when a refused replay or a transient reload
    /// gives the dictionary a different mapping.
    pub(crate) fn publish_current(&mut self, mut incoming: Self) {
        for nibble in incoming.libraries.keys().copied() {
            let Some((path, mapping)) = incoming.mapped.take_mapping(nibble) else {
                continue;
            };
            if let Some(original) = self.libraries.get_mut(&nibble) {
                let old = self.mapped.take_mapping(nibble);
                if original.mapped.is_none() {
                    original.mapped = old.map(|(_, mapping)| mapping);
                }
            }
            self.mapped.insert_mapping(nibble, path, mapping);
        }
    }

    /// Move all originals out after a successful transition.
    pub(crate) fn take(&mut self) -> Self {
        let empty = Self::new(Default::default(), self.layout.clone());
        std::mem::replace(self, empty)
    }

    /// Transfer the selected baselines after replay, without cloning byte owners.
    pub fn append(&mut self, mut incoming: Self) {
        for (nibble, library) in incoming.libraries {
            self.mapped.unload(nibble);
            if let Some((path, mapping)) = incoming.mapped.take_mapping(nibble) {
                self.mapped.insert_mapping(nibble, path, mapping);
            }
            self.libraries.insert(nibble, library);
        }
    }

    /// The user dir's file layout.
    #[must_use]
    pub const fn layout(&self) -> &UserFileLayout {
        &self.layout
    }
}

impl std::ops::Deref for SystemOriginals {
    type Target = crate::user::LibraryTable<SystemLibrary>;

    fn deref(&self) -> &Self::Target {
        &self.libraries
    }
}

impl std::ops::DerefMut for SystemOriginals {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.libraries
    }
}

/// The libraries under the stock layout.
impl From<crate::user::LibraryTable<SystemLibrary>> for SystemOriginals {
    fn from(libraries: crate::user::LibraryTable<SystemLibrary>) -> Self {
        Self::new(libraries, UserFileLayout::stock())
    }
}

/// Builds the `SYSTEM_FILE` libraries' originals from the runtime's
/// opened chunks — the `.dbin` diff base, and the conformance source
/// for replaying a profile's logs.
///
/// Items whose UCS-4 text does not
/// decode are skipped (a malformed entry, never a panic).
/// Only library metadata and uniquely owned mappings are retained here: decoding
/// phrases and pronunciations is deferred to a touched slot's [`SystemLibrary`].
/// A fresh profile therefore pays no full-item pass (#588).
#[must_use]
pub fn system_originals(libraries: PhraseLibraries, layout: UserFileLayout) -> SystemOriginals {
    let out = layout
        .system_logs()
        .iter()
        .filter_map(|&(nibble, _)| Some((nibble, library_metadata(&libraries, nibble)?)))
        .collect();
    SystemOriginals {
        libraries: out,
        mapped: libraries,
        layout,
    }
}

/// A mapped library's load-time metadata, without its bytes or items.
fn library_metadata(libraries: &PhraseLibraries, nibble: u8) -> Option<SystemLibrary> {
    let library = libraries.library(crate::vocab::phrase_index_make_token(nibble, 0))?;
    Some(SystemLibrary {
        total: library.total_freq(),
        range_end: library.token_range().end,
        items: BTreeMap::new(),
        mapped: None,
    })
}

/// A persistence failure: I/O, a container, or a byte stream that does
/// not parse.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    /// Filesystem failure.
    #[error("user file io: {0}")]
    Io(std::io::Error),
    /// A store container failed to open, write or read.
    #[error("user file store: {0}")]
    Store(StoreError),
    /// A byte stream did not parse under its frozen format.
    #[error("user file codec error")]
    Codec,
    /// `MemoryChunk::mmap` refused a system library's file at the save's
    /// remap: the save stopped at that library.
    #[error("a system library's file cannot be mapped again (upstream: mmap failed!)")]
    SystemRemapFailed,
    /// A bulk copy into a fresh chunk reaches the pin's unsafe threshold:
    /// the save stops there and answers `false`,
    /// keeping the earlier files as written, and the session stays dirty.
    #[error("a save copy reaches the pin's unsafe size (memory_chunk.h:95-98)")]
    SaveTooLarge,
}

impl PersistenceError {
    /// Whether a save carries on past this failure of one file, as the
    /// pin's `_write_files` and `zhuyin_save` do: a filesystem or container
    /// failure the pin answers with `false`.
    fn is_soft_write(&self) -> bool {
        match self {
            Self::Io(_) => true,
            Self::Store(_) => true,
            Self::Codec | Self::SystemRemapFailed | Self::SaveTooLarge => false,
        }
    }
}

impl From<StoreError> for PersistenceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

/// What the library loop reads of a profile, beside the session's own structures.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    /// The decoded fixture state (empty on a fresh or wiped profile).
    #[cfg(test)]
    pub state: crate::user::UserState,
    /// The open counter every later `user.conf` write of this session
    /// starts from: under [`UserConfLaw::Pinyin`] the value the load
    /// raised and wrote, which the reporting save writes back and [`fini`] lowers;
    /// under [`UserConfLaw::Zhuyin`] 0, the counter of the fresh
    /// `UserTableInfo` libzhuyin's `mark_version` saves. Upstream's `int`
    /// (`table_info.h:102`), so a negative marker raises to a negative
    /// value.
    pub open_counter: i32,
    /// A non-conform profile was found and its files removed —
    /// `check_format`'s `_clean_user_files`, the ecosystem's own
    /// mechanism for "backend or model change discards user data".
    #[cfg(test)]
    pub wiped: bool,
    /// Files whose bytes failed to parse; the profile continues without
    /// them (upstream's own degrade — `chunk->load` failure leaves an
    /// empty library, never a failed init).
    #[cfg(test)]
    pub skipped: Vec<String>,
    /// The pin assertion a `SYSTEM_FILE` library's `.dbin` replay reached,
    /// in library order: the first library whose log reaches one, after the
    /// libraries before it replayed. The pin dies there
    /// ([`JournalFault`]); the session open returns it and the C adapters
    /// abort. No later library is replayed.
    pub journal_fault: Option<JournalFault>,
    /// The total each library the profile filled holds once loaded, as the
    /// pin keeps it: a `USER_FILE` library's is the total stored in its
    /// file (074a2219 `phrase_index.cpp:345`), not a sum of its items; a
    /// system library's is its original's after the `.dbin` replay, which
    /// sets it from each `MODIFY_HEADER` and moves it with every item added
    /// or removed (`phrase_index.cpp:439-520`). The facade adds each to its
    /// own total (`phrase_index.cpp:237-248`), so the session prices with
    /// these and not with the items' sum.
    pub(crate) library_totals: crate::user::LibraryTable<u32>,
}

impl Loaded {
    #[cfg_attr(
        not(test),
        expect(
            clippy::unused_self,
            reason = "Production has no skip-note sink; tests use the receiver's skip notes"
        )
    )]
    #[cfg_attr(
        test,
        expect(
            clippy::unnecessary_wraps,
            reason = "Tests supply the note sink; the non-test build returns None"
        )
    )]
    fn skip_notes(&mut self) -> Option<&mut Vec<String>> {
        #[cfg(test)]
        return Some(&mut self.skipped);
        #[cfg(not(test))]
        None
    }
}

/// Preserve diagnostic details for the replay and load tests. Production
/// callers supply no sink and never format or retain these observations.
pub(crate) fn record_skip(
    skipped: &mut Option<&mut Vec<String>>,
    message: std::fmt::Arguments<'_>,
) {
    if let Some(skipped) = skipped {
        skipped.push(message.to_string());
    }
}

/// The private result of a profile parse: the public [`Loaded`] plus
/// the user pinyin index's key metadata needed by session wiring.
#[derive(Debug, Default)]
pub(crate) struct LoadedProfile {
    /// USER_FILE arenas moved into the session after profile parsing.
    pub(crate) user_libraries: crate::user::LibraryTable<UserLibrary>,
    /// Completed private views retained by session opens.
    pub(crate) replayed: crate::user::LibraryTable<ReplayedLibrary>,
    /// The public load result.
    pub(crate) loaded: Loaded,
    /// The user pinyin index's readings longer than `MAX_PHRASE_LENGTH`
    /// syllables, as packed `ChewingKey` words — the pin's user
    /// `ChewingLargeTable2` carries one DB key per reading, and a key
    /// past the table's 16-syllable instantiation drives
    /// `chewing_large_table2_bdb.cpp:282`/`:529` into `switch`'s
    /// `default: abort()`. The value model cannot hold such a reading
    /// (its `(token, keys)` item exists only for a `user.bin` reading of
    /// at most `MAX_PHRASE_LENGTH`), so the raw keys ride here as a
    /// sibling of the loaded libraries for the two abort sites — recorded from
    /// the DB key's length alone, independent of the value decoding. A key
    /// with no whole word (empty, or one byte) is recorded as an empty
    /// reading: the same `switch` has no case for a word count of 0, so
    /// `mask_out` aborts on it (`:529`).
    pub(crate) overlong_index_keys: Vec<Vec<u16>>,
    /// The user pinyin index's stored records, loaded once from the DBM
    /// and edited in the session's overlay.
    pub(crate) index: UserIndex,
    /// The user phrase table's stored records, loaded once from the DBM
    /// and edited in the session's overlay.
    pub(crate) phrase_table: UserPhraseTable,
}

/// The readings `add_index` saw, as the pinyin index's rows: those the
/// libraries carry and `state.indexed` names, and the indexed readings of
/// items added to a system library.
#[cfg(test)]
pub(crate) fn chewing_rows(state: &UserState, originals: &SystemOriginals) -> Vec<PinyinIndexItem> {
    chewing_rows_inner(state, originals)
}

#[cfg(test)]
fn chewing_rows_inner(state: &UserState, originals: &SystemOriginals) -> Vec<PinyinIndexItem> {
    // The pinyin index holds the readings `add_index` saw. The phrase
    // index is independently stored and copied verbatim below.
    let mut chewing_rows: Vec<PinyinIndexItem> = Vec::new();
    for (&nibble, items) in &state.libraries {
        for (&slot, item) in items {
            let token = crate::vocab::phrase_index_make_token(nibble, slot);
            for (packed, _freq) in &item.prons {
                if !state.indexed.contains(&(token, packed.clone())) {
                    continue;
                }
                chewing_rows.push(PinyinIndexItem {
                    token,
                    keys: packed
                        .iter()
                        .map(|&bits| ChewingKey::from_packed(bits))
                        .collect(),
                });
            }
        }
    }
    // Both add_index calls target the user trees even for a system token
    // (074a2219, facade_phrase_table3.h:166-172 / facade_chewing_table2.h:167-172).
    for (&nibble, overrides) in &state.system_overrides {
        for (&slot, item) in overrides {
            if originals
                .get(&nibble)
                .is_some_and(|base| base.contains_item(slot))
            {
                continue;
            }
            let Some(item) = item else {
                continue;
            };
            let token = crate::vocab::phrase_index_make_token(nibble, slot);
            for (packed, _) in &item.prons {
                if state.indexed.contains(&(token, packed.clone())) {
                    chewing_rows.push(PinyinIndexItem {
                        token,
                        keys: packed
                            .iter()
                            .map(|&bits| ChewingKey::from_packed(bits))
                            .collect(),
                    });
                }
            }
        }
    }
    // The index also holds entries no item reading backs: a dropped item's,
    // kept by the pin's index tables (074a2219 `phrase_index.cpp:261-268`),
    // and a profile's own. They are written as they stand.
    let written: BTreeSet<(u32, Vec<u16>)> = chewing_rows
        .iter()
        .map(|row| {
            (
                row.token,
                row.keys.iter().map(|key| key.to_packed()).collect(),
            )
        })
        .collect();
    for (token, packed) in state.indexed.difference(&written) {
        if packed.is_empty() || packed.len() > crate::vocab::MAX_PHRASE_LENGTH {
            continue;
        }
        chewing_rows.push(PinyinIndexItem {
            token: *token,
            keys: packed
                .iter()
                .map(|&bits| ChewingKey::from_packed(bits))
                .collect(),
        });
    }
    chewing_rows
}

/// What `check_format` decided about a profile: the open counter this
/// session starts from and whether the files are kept.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct ProfileCheck {
    open_counter: i32,
    pub(crate) conform: bool,
}

/// `check_format`, the first half of a profile load: judge the `user.conf` marker, raise
/// the counter, write the marker and wipe a non-conform profile, in the
/// pin's order (`pinyin.cpp:172-218`, `zhuyin.cpp:126-162`). The pin runs it
/// first in `pinyin_init`, before any library is loaded
/// (`pinyin.cpp:337-346`), so a facade whose init can fail later runs it
/// ahead of those loads, then reads the profile with `load_checked`.
///
/// The marker save's answer is ignored, as `check_format` ignores it
/// (`pinyin.cpp:187`): a marker that
/// cannot be written leaves the pin's `write %s failed.` line and the
/// profile is judged and used as it would be otherwise.
///
/// # Aborts
///
/// Unknown database format aborts before any write or wipe.
pub(crate) fn check_format(
    dir: &Path,
    versions: &SystemVersions,
    layout: &UserFileLayout,
    law: UserConfLaw,
) -> ProfileCheck {
    let existing = load_user_conf(&build_path(dir, OsStr::new(USER_CONF)));

    let conform = existing
        .as_ref()
        .is_some_and(|info| info.is_conform(versions));

    // libpinyin's raise, exactly `check_format`'s arithmetic
    // (`pinyin.cpp:185-186`): `get_open_counter() + 1`, where a missing
    // or unparsable marker reads 0 (`UserTableInfo::load` resets before
    // it parses, `table_info.cpp:325-326`). A conform profile raised to 7
    // fails `is_conform` at the next init, which wipes and writes 1.
    // libzhuyin keeps no counter of its own: its writes carry the fresh
    // `UserTableInfo`'s 0.
    let open_counter = match law {
        UserConfLaw::Pinyin => {
            get_open_counter(existing.as_ref().map_or(0, |info| info.open_counter)) + 1
        }
        UserConfLaw::Zhuyin => 0,
    };

    // libpinyin saves the marker before it wipes (`:187`, `:194-215`): a
    // profile file named `user.conf` is unlinked after the save.
    if law == UserConfLaw::Pinyin {
        // The save's answer is ignored, and no extra line of oxpinyin's own
        // follows a failure.
        let _ = write_marker(dir, versions, open_counter);
    }
    if !conform {
        clean_user_files(dir, layout, law);
    }
    ProfileCheck {
        open_counter,
        conform,
    }
}

/// User pinyin records and load results that precede the system phrase attach.
pub(crate) struct LoadedPinyinIndex<'a> {
    conform_dir: Option<&'a Path>,
    loaded: Loaded,
    index: UserIndex,
    overlong: Vec<Vec<u16>>,
}

impl<'a> LoadedPinyinIndex<'a> {
    pub(crate) fn open(user: Option<(&'a Path, ProfileCheck)>) -> Self {
        let mut loaded = Loaded {
            open_counter: user.map_or(0, |(_, check)| check.open_counter),
            #[cfg(test)]
            wiped: user.is_some_and(|(_, check)| !check.conform),
            ..Loaded::default()
        };
        let conform_dir = user.filter(|(_, check)| check.conform).map(|(dir, _)| dir);
        let mut index = UserIndex::default();
        let mut overlong = Vec::new();
        if let Some(dir) = conform_dir {
            load_user_pinyin_index(dir, &mut loaded, &mut overlong, &mut index);
        }
        Self {
            conform_dir,
            loaded,
            index,
            overlong,
        }
    }

    /// The second user index, after the system phrase attach.
    pub(crate) fn load_phrase_table(mut self) -> LoadedProfile {
        let mut phrase_table = UserPhraseTable::default();
        if let Some(dir) = self.conform_dir {
            load_user_phrase_index(dir, &mut self.loaded, &mut phrase_table);
        }
        LoadedProfile {
            loaded: self.loaded,
            index: self.index,
            overlong_index_keys: self.overlong,
            phrase_table,
            user_libraries: Default::default(),
            replayed: Default::default(),
        }
    }
}

impl LoadedProfile {
    pub(crate) fn load_libraries(
        &mut self,
        user: Option<(&Path, ProfileCheck)>,
        originals: &mut SystemOriginals,
        rows: &crate::user::LibraryLoopRows<'_>,
        unmapped: &mut dyn FnMut(&Path),
    ) -> Result<(), crate::user::LibraryLoopStop> {
        if let Some((user_dir, check)) = user {
            crate::user::library_loop::run(
                rows.steps(),
                rows.system_dir(),
                originals,
                Some(SessionProfile {
                    user_dir,
                    profile: self,
                    conform: check.conform,
                }),
                unmapped,
            )
        } else {
            rows.map_libraries(originals, unmapped)?;
            self.replayed = originals.take_live_libraries();
            Ok(())
        }
    }
}

/// Load a standalone user profile without system DBM attachments.
pub(crate) fn load_session_in_loop(
    dir: &Path,
    originals: &mut SystemOriginals,
    rows: &crate::user::LibraryLoopRows<'_>,
    check: &ProfileCheck,
) -> (LoadedProfile, Result<(), crate::user::LibraryLoopStop>) {
    let user = Some((dir, *check));
    let mut profile = LoadedPinyinIndex::open(user).load_phrase_table();
    let stop = profile.load_libraries(user, originals, rows, &mut |_| {});
    (profile, stop)
}

/// The library loop's steps over a user profile.
pub(crate) struct SessionProfile<'a> {
    pub(crate) user_dir: &'a Path,
    pub(crate) profile: &'a mut LoadedProfile,
    pub(crate) conform: bool,
}

impl SessionProfile<'_> {
    pub(crate) fn merge_log(
        &mut self,
        originals: &SystemOriginals,
        index: u8,
    ) -> Result<(), JournalFault> {
        if !self.conform {
            return Ok(());
        }
        let Some((_, file)) = originals
            .layout()
            .system_logs()
            .iter()
            .find(|(nibble, _)| *nibble == index)
        else {
            return Ok(());
        };
        replay_log(
            self.user_dir,
            originals,
            &mut self.profile.loaded,
            index,
            file,
            None,
            false,
            &mut self.profile.replayed,
            None,
        )
    }

    pub(crate) fn read_user_library(&mut self, originals: &SystemOriginals, index: u8) {
        if self.conform {
            load_libraries(
                self.user_dir,
                originals.layout(),
                &mut self.profile.loaded,
                index,
                &mut self.profile.user_libraries,
            );
        }
    }
}

/// `pinyin_fini`'s arithmetic (`pinyin.cpp:1196-1197`): the counter the
/// init raised, read through `get_open_counter`, lowered by one and
/// floored at 0 — `counter > 1 ? counter - 1 : 0`. A raised 7 therefore
/// reads 0 and lowers to 0, not to 6; a negative counter lowers to 0.
#[must_use]
pub const fn lowered_open_counter(open_counter: i32) -> i32 {
    let counter = get_open_counter(open_counter);
    if counter > 1 { counter - 1 } else { 0 }
}

// pinyin_fini and mark_version, pinyin.cpp:1195–1200; zhuyin_fini, zhuyin.cpp:741–757.
/// Applies the facade's final profile-marker policy. Pinyin lowers the open counter and
/// attempts the marker write even if no save preceded it. With no directory it attempts
/// the empty filename. Zhuyin skips the write. The write result is ignored.
pub fn fini(dir: Option<&Path>, versions: &SystemVersions, law: UserConfLaw, open_counter: i32) {
    if law == UserConfLaw::Pinyin {
        let counter = lowered_open_counter(open_counter);
        let _ = match dir {
            Some(dir) => write_marker(dir, versions, counter),
            None => write_unnamed_marker(versions, counter),
        };
    }
}

// check_format uses g_build_filename(NULL, "user.conf", NULL), pinyin.cpp:176–187 and
// zhuyin.cpp:130–132.
/// Checks the empty marker filename for a context opened without a user directory. The
/// pinyin policy also attempts to write that marker.
pub fn check_format_unnamed(versions: &SystemVersions, law: UserConfLaw) {
    let _ = UserTableInfo::load(c"");
    if law == UserConfLaw::Pinyin {
        let _ = write_unnamed_marker(versions, get_open_counter(0) + 1);
    }
}

/// `mark_version`'s write: the conform marker (`make_conform`) with
/// `open_counter`, through `UserTableInfo::save`'s own `fopen("w")` and
/// `fprintf` calls (`table_info.cpp:377-397`). Answers the save's `bool`,
/// `false` after its `write %s failed.` line.
fn write_marker(dir: &Path, versions: &SystemVersions, open_counter: i32) -> bool {
    let mut marker = UserTableInfo::conform_to(versions);
    marker.open_counter = open_counter;
    marker.save_path(&build_path(dir, OsStr::new(USER_CONF)))
}

/// [`write_marker`] for a context without a user dir: `UserTableInfo::save("")`.
fn write_unnamed_marker(versions: &SystemVersions, open_counter: i32) -> bool {
    let mut marker = UserTableInfo::conform_to(versions);
    marker.open_counter = open_counter;
    marker.save(c"")
}

/// `_clean_user_files` + the fixed names, in the pin's order: the default
/// rows' user files by sub-index, libpinyin's addon rows too
/// (`pinyin.cpp:194-215`; libzhuyin stops after the default rows,
/// `zhuyin.cpp:141-159`), then `USER_PINYIN_INDEX`, `USER_PHRASE_INDEX`
/// and `USER_BIGRAM`. Every result is ignored, as the pin ignores
/// `unlink`'s. `user.conf` is not among them: libpinyin has already
/// rewritten it by then, and libzhuyin leaves the non-conform marker in
/// place until a save.
fn clean_user_files(dir: &Path, layout: &UserFileLayout, law: UserConfLaw) {
    for (_, name) in layout.default_cleanup() {
        unlink(&build_path(dir, name.as_ref()));
    }
    if law == UserConfLaw::Pinyin {
        for (_, name) in layout.addon_cleanup() {
            unlink(&build_path(dir, name.as_ref()));
        }
    }
    for dbm in [UserDbm::PinyinIndex, UserDbm::PhraseIndex, UserDbm::Bigram] {
        unlink(&build_path(dir, OsStr::new(&dbm.file_name())));
    }
}

/// One walk of `user_pinyin_index.bin` into the session's native tree, plus the over-long readings the pin's mask `switch` refuses. An
/// absent file or one that does not open leaves the table empty, as an
/// empty `ChewingLargeTable2` does. A copy that fails part way on tkrzw or
/// Berkeley DB aborts at [`RawReadStore::copy_raw`].
fn load_user_pinyin_index(
    dir: &Path,
    loaded: &mut Loaded,
    overlong: &mut Vec<Vec<u16>>,
    index: &mut UserIndex,
) {
    let path = build_path(dir, OsStr::new(&UserDbm::PinyinIndex.file_name()));
    if !path.exists() {
        return;
    }
    let store = match DefaultStore::open_user_index(&path) {
        Ok(store) => store,
        Err(error) => {
            record_skip(
                &mut loaded.skip_notes(),
                format_args!("{}: {error}", UserDbm::PinyinIndex.file_name()),
            );
            return;
        }
    };
    let mut exceptional = Vec::new();
    // `ChewingLargeTable2::load_db`'s copy into its in-memory table: on
    // tkrzw and Berkeley DB a failed walk is fatal, as the pin's copy
    // asserts; Kyoto Cabinet's snapshot load keeps what it read.
    match store.copy_raw(|key, value| {
        index
            .push_record(key, value)
            .unwrap_or_else(|_| std::process::abort());
        if key.len() < CHEWING_KEY_SIZE
            || key.len() / CHEWING_KEY_SIZE > crate::vocab::MAX_PHRASE_LENGTH
        {
            exceptional.push(key.to_vec());
        }
    }) {
        Ok(()) => {}
        Err(error) => record_skip(
            &mut loaded.skip_notes(),
            format_args!("{}: {error}", UserDbm::PinyinIndex.file_name()),
        ),
    }
    *overlong = exceptional
        .into_iter()
        .map(|key| {
            key.as_chunks::<2>()
                .0
                .iter()
                .map(|word| u16::from_le_bytes(*word))
                .collect()
        })
        .collect();
}

/// Copy `user_phrase_index.bin` records without interpreting either key or value,
/// as the pin's `PhraseLargeTable3::load_db` does. A missing file or one that
/// does not open leaves the table empty; a failed copy is as for the pinyin
/// index.
fn load_user_phrase_index(dir: &Path, loaded: &mut Loaded, arena: &mut UserPhraseTable) {
    let path = build_path(dir, OsStr::new(&UserDbm::PhraseIndex.file_name()));
    if !path.exists() {
        return;
    }
    let store = match DefaultStore::open_user_index(&path) {
        Ok(store) => store,
        Err(error) => {
            record_skip(
                &mut loaded.skip_notes(),
                format_args!("{}: {error}", UserDbm::PhraseIndex.file_name()),
            );
            return;
        }
    };
    // `PhraseLargeTable3::load_db`'s copy, as for the pinyin index.
    let copied = store.copy_raw(|key, value| {
        arena
            .push_record(key, value)
            .unwrap_or_else(|_| std::process::abort());
    });
    match copied {
        Ok(()) => {}
        Err(error) => record_skip(
            &mut loaded.skip_notes(),
            format_args!("{}: {error}", UserDbm::PhraseIndex.file_name()),
        ),
    }
}

/// `pinyin_init`'s `m_user_bigram->load_db(user_bigram.db)`
/// (`pinyin.cpp:399-401`): the pin's own in-memory container, filled from
/// the file in the file's walk order. Opened after the profile load, so a profile
/// `check_format` wiped loads empty, as the pin's does.
///
/// # Errors
///
/// Returns [`PersistenceError`] when the in-memory container cannot be
/// created.
pub(crate) fn load_user_bigram_db(dir: &Path) -> Result<DefaultUserBigramDb, PersistenceError> {
    Ok(DefaultUserBigramDb::load_db(&build_path(
        dir,
        OsStr::new(&UserDbm::Bigram.file_name()),
    ))?)
}

/// Library `only`'s `USER_FILE` chunk store.
fn load_libraries(
    dir: &Path,
    layout: &UserFileLayout,
    loaded: &mut Loaded,
    only: u8,
    libraries: &mut crate::user::LibraryTable<UserLibrary>,
) {
    for (nibble, file) in layout.user_libraries() {
        let nibble = *nibble;
        if only != nibble {
            continue;
        }
        let name = file.display();
        let path = build_path(dir, file.as_ref());
        let bytes = match load_chunk_payload(&path) {
            Ok(bytes) => bytes,
            // Absent is the normal fresh-profile case. Any other error
            // is an unreadable existing file. Keep the same empty
            // sub-index fallback and record details only for tests.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                record_skip(&mut loaded.skip_notes(), format_args!("{name}: {e}"));
                continue;
            }
        };
        match UserLibrary::load_payload(&bytes) {
            Some(library) => {
                loaded.library_totals.insert(nibble, library.total());
                // An absent library and an empty one are the same state;
                // keep the map absent so save/load are byte- and
                // value-stable round trips.
                if !library.is_empty() {
                    libraries.insert(nibble, library);
                }
            }
            None => {
                record_skip(
                    &mut loaded.skip_notes(),
                    format_args!("{name}: invalid sub-index"),
                );
            }
        }
    }
}

/// A reload's result: the live buffer each replayed library keeps, and the
/// total each library the profile filled holds.
pub(crate) type Reloaded = (
    crate::user::LibraryTable<ReplayedLibrary>,
    crate::user::LibraryTable<u32>,
);

/// One reloaded library's saved state: the `.dbin` log in the user
/// directory replayed onto the freshly mapped system original —
/// `_load_phrase_library`'s `load` then `merge` (074a2219
/// `pinyin.cpp:264-277`, `zhuyin.cpp:208-221`). Only the libraries in
/// `originals` are read; unsaved session state never enters it.
///
/// # Errors
///
/// The [`JournalFault`] the merge reaches.
#[cfg(test)]
pub(crate) fn load_library_logs(
    dir: &Path,
    originals: &SystemOriginals,
) -> Result<Reloaded, JournalFault> {
    let mut loaded = Loaded::default();
    let mut replayed = crate::user::LibraryTable::new();
    replay_logs(dir, originals, &mut loaded, None, true, &mut replayed, None)?;
    Ok((replayed, loaded.library_totals))
}

pub(crate) fn load_library_logs_live(
    dir: &Path,
    originals: &mut SystemOriginals,
) -> Result<Reloaded, JournalFault> {
    let mut loaded = Loaded::default();
    let mut replayed = originals.take_live_libraries();
    replay_logs(dir, originals, &mut loaded, None, true, &mut replayed, None)?;
    Ok((replayed, loaded.library_totals))
}

/// One reloaded `USER_FILE` library's saved state: its chunk in the user
/// directory, or nothing when the file is absent or unreadable —
/// `_load_phrase_library`'s `USER_FILE` branch, which loads the chunk or
/// creates an empty sub-index (074a2219 `pinyin.cpp:303-321`,
/// `zhuyin.cpp:247-265`). Unsaved session state never enters it.
pub(crate) fn load_user_library(
    dir: &Path,
    layout: &UserFileLayout,
    nibble: u8,
) -> Option<UserLibrary> {
    let mut loaded = Loaded::default();
    let mut libraries = crate::user::LibraryTable::new();
    load_libraries(dir, layout, &mut loaded, nibble, &mut libraries);
    libraries.remove(&nibble).or_else(|| {
        loaded
            .library_totals
            .get(&nibble)
            .map(|&total| UserLibrary::empty_with_total(total))
    })
}

/// `mask_out`'s reload of each loaded system library: the log is read from
/// the user directory before the library bits are checked, then selected
/// records are filtered by `mask_out_phrase_index_logger` and merged over the freshly mapped
/// original (074a2219 `phrase_index.cpp:301-330`, `:844-869`), library by
/// library in the mask loop's order (`pinyin.cpp:1234-1291`,
/// `zhuyin.cpp:769-826`).
///
/// # Errors
///
/// The [`JournalFault`] the first library's log reaches, including
/// `_compute_new_header`'s read; no later library is read.
#[cfg(test)]
pub(crate) fn load_masked_logs(
    dir: &Path,
    originals: &SystemOriginals,
    mask: u32,
    value: u32,
) -> Result<Reloaded, JournalFault> {
    let mut loaded = Loaded::default();
    let mut replayed = crate::user::LibraryTable::new();
    replay_logs(
        dir,
        originals,
        &mut loaded,
        Some((mask, value)),
        false,
        &mut replayed,
        None,
    )?;
    Ok((replayed, loaded.library_totals))
}

pub(crate) fn load_masked_logs_live(
    dir: &Path,
    originals: &mut SystemOriginals,
    mask: u32,
    value: u32,
    preread: &mut MaskLogs,
) -> Result<Reloaded, JournalFault> {
    let mut loaded = Loaded::default();
    let mut replayed = originals.take_live_libraries();
    replay_logs(
        dir,
        originals,
        &mut loaded,
        Some((mask, value)),
        false,
        &mut replayed,
        Some(preread),
    )?;
    Ok((replayed, loaded.library_totals))
}

/// The first stop found by the mask walk, after the preceding libraries replay.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum MaskStop {
    Journal(JournalFault),
    /// A failed header read from the system chunk or its saved log.
    Directory,
}

/// The validated payloads (or failed loads) `mask_out`'s walk already read,
/// by sub-index, so replay consumes the result of that one read. A library absent
/// here is read when the replay reaches it.
///
/// The walk stops at the first fault, so it holds the bytes of the journals
/// it reached and no others: linear in their total size, over at most 15
/// library slots (sub-index 0 is never remapped), one `Vec` each. The pin
/// holds the same bytes, one library at a time, in `MemoryChunk::load`.
#[derive(Default)]
pub struct MaskLogs(crate::user::LibraryTable<Result<Vec<u8>, ChunkPayloadError>>);

/// Read library `nibble`'s saved log once into `logs` and return the header
/// fault `merge_with_mask` reaches in it, without touching any state:
/// `_peek_header` asserts on a non-null header token or on a second
/// header, and the masked totals read a kept item shorter than its
/// unigram. `None` includes a successfully loaded library excluded by the
/// mask (its records are not parsed), an absent or unreadable log, a log with
/// no header, and merge faults that need the library's content. A failed chunk
/// header read stops the walk, including for an excluded library.
pub(crate) fn read_mask_log(
    dir: &Path,
    layout: &UserFileLayout,
    logs: &mut MaskLogs,
    nibble: u8,
    mask: u32,
    value: u32,
) -> Option<MaskStop> {
    let (_, file) = layout.system_logs().iter().find(|(n, _)| *n == nibble)?;
    let read = try_load_chunk_payload(&build_path(dir, file.as_ref()));
    let selected = (nibble & crate::vocab::phrase_index_library_index(mask))
        == crate::vocab::phrase_index_library_index(value);
    let fault = match &read {
        Err(ChunkPayloadError::HeaderRead) => Some(MaskStop::Directory),
        Ok(payload) if selected => masked_log(payload, mask, value, nibble)
            .err()
            .map(MaskStop::Journal),
        Ok(_) | Err(ChunkPayloadError::Load(_)) => None,
    };
    logs.0.insert(nibble, read);
    fault
}

/// The replay of every `SYSTEM_FILE` library's log, in sub-index order: the
/// pin's merge (or, with `mask`, `merge_with_mask`) per library. The first
/// assertion ends it.
fn replay_logs(
    dir: &Path,
    originals: &SystemOriginals,
    loaded: &mut Loaded,
    mask: Option<(u32, u32)>,
    only_originals: bool,
    replayed: &mut crate::user::LibraryTable<ReplayedLibrary>,
    mut preread: Option<&mut MaskLogs>,
) -> Result<(), JournalFault> {
    for (nibble, file) in originals.layout().system_logs() {
        replay_log(
            dir,
            originals,
            loaded,
            *nibble,
            file,
            mask,
            only_originals,
            replayed,
            preread.as_deref_mut(),
        )?;
    }
    Ok(())
}

/// One library's log payload for the replay: the bytes the mask pre-read
/// took, else the file's. `None` is an empty log: an absent or unreadable
/// `.dbin` (`MemoryChunk::load` fails and leaves the chunk empty, and the
/// merge reads nothing). An unreadable one also records a skip note.
fn read_log_payload(
    dir: &Path,
    file: &FileName,
    nibble: u8,
    preread: Option<&mut MaskLogs>,
    loaded: &mut Loaded,
) -> Option<Vec<u8>> {
    let read = preread
        .and_then(|logs| logs.0.remove(&nibble))
        .unwrap_or_else(|| {
            load_chunk_payload(&build_path(dir, file.as_ref())).map_err(ChunkPayloadError::Load)
        });
    match read {
        Ok(bytes) => Some(bytes),
        // The walk recorded the stop. This library has no log to merge.
        Err(ChunkPayloadError::HeaderRead) => None,
        Err(ChunkPayloadError::Load(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(ChunkPayloadError::Load(e)) => {
            let name = file.display();
            record_skip(&mut loaded.skip_notes(), format_args!("{name}: {e}"));
            None
        }
    }
}

/// Record what one library's finished merge leaves in the load: its
/// retained buffer, its total, and an override without an item for each
/// slot that answers none.
fn record_merge(
    merge: Merge,
    loaded: &mut Loaded,
    replayed: &mut crate::user::LibraryTable<ReplayedLibrary>,
    nibble: u8,
    name: &impl std::fmt::Display,
) {
    let (total, _gone, retained) = merge.finish_views(&mut loaded.skip_notes(), &name);
    if let Some(retained) = retained {
        replayed.insert(nibble, retained);
    }
    if let Some(total) = total {
        loaded.library_totals.insert(nibble, total);
    }
    // An empty override set and an absent one are the same state. A slot that
    // answers no item is an override without an item.
    #[cfg(test)]
    if !_gone.is_empty() {
        let overrides = _gone.into_iter().map(|slot| (slot, None)).collect();
        loaded.state.system_overrides.insert(nibble, overrides);
    }
}

/// One library's turn in [`replay_logs`]: `FacadePhraseIndex::merge`, or with
/// `mask`, `merge_with_mask`. Init's loop calls it for each library right
/// after mapping that library ([`crate::user::library_loop`]).
#[expect(
    clippy::too_many_arguments,
    reason = "replay_logs' parameters and one library's identity"
)]
fn replay_log(
    dir: &Path,
    originals: &SystemOriginals,
    loaded: &mut Loaded,
    nibble: u8,
    file: &FileName,
    mask: Option<(u32, u32)>,
    only_originals: bool,
    replayed: &mut crate::user::LibraryTable<ReplayedLibrary>,
    preread: Option<&mut MaskLogs>,
) -> Result<(), JournalFault> {
    if only_originals && !originals.contains_key(&nibble) {
        return Ok(());
    }
    // The pin skips unloaded libraries, but loads each remaining log
    // before merge_with_mask checks the library bits.
    if mask.is_some() && !originals.contains_key(&nibble) {
        return Ok(());
    }
    let name = file.display();
    let Some(bytes) = read_log_payload(dir, file, nibble, preread, loaded) else {
        return Ok(());
    };
    if mask.is_some_and(|(mask, value)| {
        (nibble & crate::vocab::phrase_index_library_index(mask))
            != crate::vocab::phrase_index_library_index(value)
    }) {
        return Ok(());
    }
    let payload = bytes.as_slice();
    // The pin emits local slots, but its reader masks high bits even
    // in hand-written records (074a2219 phrase_index.cpp:180-215, 219-232).
    // Compare each old payload as wire bytes and stop at the first mismatch.
    // A replacement uses only its own record's bytes.
    // A masked logger with no header is empty. Decide that before
    // taking ownership so rollback remains exclusive to payload over-read.
    let masked = if let Some((mask, value)) = mask {
        let Some(records) = masked_log(payload, mask, value, nibble)? else {
            return Ok(());
        };
        Some(records)
    } else {
        None
    };
    let live = replayed.remove(&nibble);
    let mut merge = match live {
        Some(live) => Merge::from_live(originals.get(&nibble), live),
        None => Merge::new(originals.get(&nibble)),
    };
    match masked {
        None => {
            // A payload past the log's end is the pin's over-read (class
            // (b)): where the walk reaches it, the log is skipped whole, as
            // the parent's `load_logs_with_mask` did when
            // `validate_log_records` refused it. Every fault or stop
            // before it comes first, as in the pin's lazy walk; the merge
            // is local until `finish`, so dropping it skips the log.
            let end = merge_log(
                &mut merge,
                || log_records(payload),
                nibble,
                &mut loaded.skip_notes(),
                &name,
            )?;
            if end == MergeEnd::OverRead {
                record_skip(
                    &mut loaded.skip_notes(),
                    format_args!("{name}: a record's payload runs past the log"),
                );
                // The library keeps its pre-replay content: the applied
                // records are reversed, not dropped with the buffer.
                replayed.insert(nibble, merge.rollback());
                return Ok(());
            }
        }
        Some(records) => {
            merge_log(
                &mut merge,
                || records.iter().cloned().map(Ok),
                nibble,
                &mut loaded.skip_notes(),
                &name,
            )?;
        }
    }
    record_merge(merge, loaded, replayed, nibble, &name);
    Ok(())
}

/// How [`merge_log`]'s walk ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MergeEnd {
    /// The walk ended, or a mismatch stopped the merge, before any over-read.
    Applied,
    /// The walk reached a payload past the log's end.
    OverRead,
}

/// `SubPhraseIndex::merge`'s loop over one logger (074a2219
/// `phrase_index.cpp:439-517`): each record the walk answers is applied in
/// order, as `next_record` reads it lazily (`:445-450`). A mismatch ends the
/// merge with the earlier records applied (`:468-471`, `:483-484`), so the
/// walk never reaches a later over-read.
///
/// # Errors
///
/// [`JournalFault::HeaderToken`] where `next_record` asserts
/// (`phrase_index_logger.h:202`) and [`JournalFault::ShortAdd`] where
/// `add_phrase_item` does (`memory_chunk.h:390`), each only where the walk
/// reaches it.
fn merge_log<'a, I>(
    merge: &mut Merge,
    records: impl Fn() -> I,
    library: u8,
    skipped: &mut Option<&mut Vec<String>>,
    name: &str,
) -> Result<MergeEnd, JournalFault>
where
    I: Iterator<Item = Result<LogRecord<&'a [u8]>, LogFault>>,
{
    for record in records() {
        let outcome = match record {
            Ok(record) => merge.apply(record),
            Err(LogFault::MissingHeaderTotals) => merge.apply_header_without_totals(),
            Err(LogFault::HeaderToken(token)) => {
                return Err(JournalFault::HeaderToken { library, token });
            }
            // The masked records hold only the walk's prefix, so only the
            // unmasked replay reaches this.
            Err(LogFault::OverRead) => return Ok(MergeEnd::OverRead),
        };
        if let Err(stop) = outcome {
            match stop {
                Stop::ShortAdd(token) => return Err(JournalFault::ShortAdd { library, token }),
                Stop::Mismatch("header", _) => {
                    record_skip(
                        skipped,
                        format_args!("{name}: replay stopped at a mismatched header"),
                    );
                }
                Stop::Mismatch(kind, token) => {
                    record_skip(
                        skipped,
                        format_args!(
                            "{name}: replay stopped at a mismatched {kind} for {token:#010x}"
                        ),
                    );
                }
            }
            break;
        }
    }
    Ok(MergeEnd::Applied)
}

/// `mask_out_phrase_index_logger` over one log (074a2219
/// `phrase_index.cpp:721-869`), walk by walk as the pin takes them:
///
/// 1. `_peek_header` walks every record, so a non-null header token anywhere
///    asserts first (`phrase_index_logger.h:202`); then it asserts on more
///    than one header (`:745`). No header is an empty new logger: `None`.
/// 2. `_compute_new_header` moves the old total by every record the mask
///    keeps, reading each kept payload's unigram (`:778-795`), which
///    asserts on a payload under six bytes (`memory_chunk.h:390`).
/// 3. `_mask_out_records` appends each kept record with both chunk pointers
///    (`:838`); a kept ADD or REMOVE asserts in `append_record`
///    (`phrase_index_logger.h:245`, `:257`), so the new logger can only hold
///    the header, written first, and the kept MODIFY records.
///
/// A record type the walk does not know ends `_peek_header` with `m_error`
/// set, which the rewinds keep: after the header checks the new logger is the
/// header alone, with the total unchanged (`phrase_index.cpp:847-869`).
///
/// A payload past the log's end (the pin's over-read) ends every
/// walk there, keeping the records before it, as the parent's
/// `validate_system_logs` snapshot of `decode_log_records_partial` did.
fn masked_log(
    payload: &[u8],
    mask: u32,
    value: u32,
    library: u8,
) -> Result<Option<Vec<LogRecord<&[u8]>>>, JournalFault> {
    let mask = TokenMask {
        mask: mask & PHRASE_MASK,
        value: value & PHRASE_MASK,
    };
    let walk = read_log(payload, library)?;
    let Some(header) = walk.header else {
        return Ok(None);
    };
    let old_total = match header {
        ReadHeader::MissingTotals => None,
        ReadHeader::Total(total) => Some(total),
    };
    // `_peek_header` ended on a record type it does not know: `m_error`
    // survives both rewinds (`phrase_index_logger.h:128-129`, `:141-144`),
    // so `_compute_new_header` and `_mask_out_records` read nothing
    // (`phrase_index.cpp:851-869`) and the new logger is the header alone,
    // its total unchanged.
    if walk.end == Some(LogEnd::UnknownType) {
        return Ok(old_total.map(|total| {
            vec![LogRecord::ModifyHeader {
                old_total: total,
                new_total: total,
            }]
        }));
    }
    let new_total = masked_total(&walk.records, old_total, mask, library)?;
    retained_records(walk.records, old_total.zip(new_total), mask, library)
}

/// The mask of `mask_out_phrase_index_logger`, over the token's slot bits.
#[derive(Clone, Copy)]
struct TokenMask {
    mask: u32,
    value: u32,
}

impl TokenMask {
    /// Whether the mask keeps a record of `token` in the new logger.
    const fn keeps(self, token: u32) -> bool {
        (token & self.mask) != self.value
    }
}

enum ReadHeader {
    MissingTotals,
    Total(u32),
}

struct LogWalk<'a> {
    records: Vec<LogRecord<&'a [u8]>>,
    header: Option<ReadHeader>,
    end: Option<LogEnd>,
}

/// `_peek_header`'s walk: each record up to the log's end or a payload past it.
///
/// # Errors
/// [`JournalFault::HeaderToken`] for a header with a non-null token, then
/// [`JournalFault::MultipleHeaders`] when more than one header was read.
fn read_log(payload: &[u8], library: u8) -> Result<LogWalk<'_>, JournalFault> {
    let mut records = Vec::new();
    let mut header = None;
    let mut multiple = false;
    let mut walk = log_records(payload);
    for record in walk.by_ref() {
        let next_header = match record {
            Ok(LogRecord::ModifyHeader { old_total, .. }) => ReadHeader::Total(old_total),
            Err(LogFault::MissingHeaderTotals) => ReadHeader::MissingTotals,
            Ok(record) => {
                records.push(record);
                continue;
            }
            Err(LogFault::HeaderToken(token)) => {
                return Err(JournalFault::HeaderToken { library, token });
            }
            Err(LogFault::OverRead) => break,
        };
        multiple |= header.replace(next_header).is_some();
    }
    if multiple {
        return Err(JournalFault::MultipleHeaders { library });
    }
    Ok(LogWalk {
        records,
        header,
        end: walk.end(),
    })
}

/// `_compute_new_header`: check each kept record and update any decoded total.
///
/// # Errors
///
/// [`JournalFault::ShortMaskedItem`] for the first kept payload under six
/// bytes, in record order.
fn masked_total(
    records: &[LogRecord<&[u8]>],
    old_total: Option<u32>,
    mask: TokenMask,
    library: u8,
) -> Result<Option<u32>, JournalFault> {
    let unigram = |item: &[u8], token: u32| {
        if item.len() < ITEM_HEADER_SIZE {
            Err(JournalFault::ShortMaskedItem { library, token })
        } else {
            Ok(raw_unigram(item))
        }
    };
    let mut new_total = old_total;
    for record in records {
        match *record {
            LogRecord::Add { token, new_item } if mask.keeps(token) => {
                let count = unigram(new_item, token)?;
                new_total = new_total.map(|total| total.wrapping_add(count));
            }
            LogRecord::Remove { token, old_item } if mask.keeps(token) => {
                let count = unigram(old_item, token)?;
                new_total = new_total.map(|total| total.wrapping_sub(count));
            }
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } if mask.keeps(token) => {
                let old = unigram(old_item, token)?;
                let new = unigram(new_item, token)?;
                new_total = new_total.map(|total| total.wrapping_sub(old).wrapping_add(new));
            }
            LogRecord::Add { .. }
            | LogRecord::Remove { .. }
            | LogRecord::Modify { .. }
            | LogRecord::ModifyHeader { .. } => {}
        }
    }
    Ok(new_total)
}

/// `_mask_out_records`: check kept record types and build a new log when
/// the original header had totals.
///
/// # Errors
///
/// [`JournalFault::SurvivingRecord`] for the first kept ADD or REMOVE record,
/// in record order.
fn retained_records(
    records: Vec<LogRecord<&[u8]>>,
    totals: Option<(u32, u32)>,
    mask: TokenMask,
    library: u8,
) -> Result<Option<Vec<LogRecord<&[u8]>>>, JournalFault> {
    let mut masked = totals.map(|(old_total, new_total)| {
        vec![LogRecord::ModifyHeader {
            old_total,
            new_total,
        }]
    });
    for record in records {
        match record {
            LogRecord::Add { token, .. } | LogRecord::Remove { token, .. } if mask.keeps(token) => {
                return Err(JournalFault::SurvivingRecord { library, token });
            }
            LogRecord::Modify { token, .. } if mask.keeps(token) => {
                if let Some(masked) = &mut masked {
                    masked.push(record);
                }
            }
            LogRecord::Add { .. }
            | LogRecord::Remove { .. }
            | LogRecord::Modify { .. }
            | LogRecord::ModifyHeader { .. } => {}
        }
    }
    Ok(masked)
}

/// An item serialised as the pin stores it (`PhraseItem`: the two length
/// bytes, the unigram, the phrase, then each reading's keys and count),
/// without judging its shape: the bytes of an item as it is.
#[must_use]
pub fn journal_item_bytes(item: &ChunkItem) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(ITEM_HEADER_SIZE + 4 * item.phrase.len());
    bytes.push(u8::try_from(item.phrase.len()).unwrap_or(u8::MAX));
    bytes.push(u8::try_from(item.prons.len()).unwrap_or(u8::MAX));
    bytes.extend_from_slice(&item.unigram.to_le_bytes());
    for &code in &item.phrase {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    for (keys, frequency) in &item.prons {
        for key in keys {
            bytes.extend_from_slice(&key.to_le_bytes());
        }
        bytes.extend_from_slice(&frequency.to_le_bytes());
    }
    bytes
}

/// What a save writes: the session's user libraries and its two user
/// tables, and its loaded system libraries.
#[derive(Clone, Copy)]
pub(crate) struct SaveSource<'a> {
    /// The session's arenas, its user pinyin index and phrase table, and
    /// its unloaded libraries.
    pub(crate) libraries: &'a crate::user::user_library::SessionPhrases,
    /// The nibbles whose live empty sub-index the pin's `compact` rebuilt
    /// from no items; their log header's new total is zero.
    pub(crate) compacted: &'a [u8],
    /// The session's loaded system libraries: their live buffers and
    /// totals, which the save diffs against the files on disk.
    pub(crate) live: &'a crate::user::system_items::SystemItems,
}

/// The save's I/O parameters and its current-on-disk baseline
/// hook. The hook is called once per selected system library, in the pin's
/// `_write_files` order, immediately before that library's `.tmp` is
/// written; the facade maps the current chunk there, so a layout critical
/// is emitted after the earlier libraries' writes and before the failing
/// one (`pinyin.cpp:956`, `zhuyin.cpp:589`).
pub(crate) struct SaveReporting<'a, B> {
    pub(crate) versions: &'a SystemVersions,
    pub(crate) open_counter: i32,
    pub(crate) bigram_db: &'a DefaultUserBigramDb,
    pub(crate) interleaved: bool,
    pub(crate) baseline: B,
}

/// The files of a save in the pin's rename order (`_rename_files`,
/// `pinyin.cpp:1025-1130`): the libraries by index, then the two indices
/// and the bigram.
fn pin_rename_order(layout: &UserFileLayout, unloaded: &[u8]) -> Vec<FileName> {
    let mut order: Vec<FileName> = libraries_in_pin_order(layout)
        .into_iter()
        .filter(|(nibble, _, _)| !unloaded.contains(nibble))
        .map(|(_, name, _)| name.clone())
        .collect();
    for dbm in [UserDbm::PinyinIndex, UserDbm::PhraseIndex, UserDbm::Bigram] {
        order.push(FileName::from_bytes(dbm.file_name().as_bytes()));
    }
    order
}

/// Which kind of library file a save writes.
#[derive(Clone, Copy)]
enum LibraryFile {
    /// A `SYSTEM_FILE` library's `.dbin` diff log.
    Log,
    /// A `USER_FILE` library's chunk.
    User,
}

/// The library files in the pin's loop order: one pass over the sub-indices
/// (`_write_files`, `_rename_files`, `pinyin.cpp:933-1090`), a stable sort
/// keeping the log before the chunk where a layout gives one index both.
fn libraries_in_pin_order(layout: &UserFileLayout) -> Vec<(u8, &FileName, LibraryFile)> {
    let mut libraries: Vec<(u8, &FileName, LibraryFile)> = layout
        .system_logs()
        .iter()
        .map(|(nibble, name)| (*nibble, name, LibraryFile::Log))
        .chain(
            layout
                .user_libraries()
                .iter()
                .map(|(nibble, name)| (*nibble, name, LibraryFile::User)),
        )
        .collect();
    crate::stable_sort::sort_by_key(&mut libraries, |&(nibble, _, _)| u128::from(nibble));
    libraries
}

/// The save with the current-on-disk baseline hook.
pub(crate) fn save_reporting<B: FnMut(u8) -> SaveBaseline>(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &SystemOriginals,
    reporting: &mut SaveReporting<'_, B>,
) -> Result<(), PersistenceError> {
    // `_write_files` writes every file whatever the one before did
    // (`pinyin.cpp:940-1020`): a write that fails leaves no `.tmp` and the
    // next file is written anyway.
    let mut staged = Staged {
        immediate: reporting.interleaved,
        ..Staged::default()
    };
    stage_all(dir, source, originals, reporting, &mut staged)?;
    // `_rename_files` then renames every file of the set in its own order and
    // prints the ones that fail (`:1025-1130`), a file whose write failed
    // among them: the unaffected files are updated and the profile can mix
    // two saves.
    // `zhuyin_save` has already renamed each file as it was written.
    if !reporting.interleaved {
        for name in pin_rename_order(originals.layout(), &staged.unloaded) {
            let tmp = build_path(dir, name.with_suffix(".tmp").as_ref());
            let final_path = build_path(dir, name.as_ref());
            if rename(&tmp, &final_path).is_err() {
                report_rename_failed(&tmp, &final_path);
            }
        }
    }
    // `mark_version` (`pinyin.cpp:1143`, `zhuyin.cpp:695`): a failed save
    // has written the pin's `write %s failed.` line itself.
    write_marker(dir, reporting.versions, reporting.open_counter);
    Ok(())
}
/// The line a failed rename of a reporting save writes where the pin's
/// `_rename_files` (`pinyin.cpp:1061-1123`) or `zhuyin_save`
/// (`zhuyin.cpp:611-689`) writes it: as the rename fails, with the pin's
/// `fprintf` on the stream `mark_version`'s own `write %s failed.` line
/// follows it on.
fn report_rename_failed(tmp: &Path, final_path: &Path) {
    crate::data::rename_failed(tmp, final_path);
}

/// The files a save has written to `.tmp` siblings. libpinyin writes them
/// all (`_write_files`) and then renames them all (`_rename_files`); libzhuyin
/// renames each as soon as it is written (`zhuyin_save`), so a name two roles
/// share ends with the other role's contents.
#[derive(Default)]
struct Staged {
    /// Rename on the spot (libzhuyin).
    immediate: bool,
    /// The unloaded libraries the write pass skipped, which the rename pass
    /// skips too (`pinyin.cpp:1032-1035`).
    unloaded: Vec<u8>,
}

impl Staged {
    /// Answers `written` for the file staged at `tmp`. libpinyin renames in
    /// its own pass; libzhuyin renames now, even after a soft write failure
    /// (074a2219 zhuyin.cpp:612-617, :645-650, :669-691). A header failure
    /// aborts before the rename; the reporting caller refuses it instead.
    fn finish(
        &self,
        tmp: &Path,
        final_path: &Path,
        written: Result<(), PersistenceError>,
    ) -> Result<(), PersistenceError> {
        let renames = written
            .as_ref()
            .map_or_else(PersistenceError::is_soft_write, |()| true);
        if self.immediate && renames && rename(tmp, final_path).is_err() {
            report_rename_failed(tmp, final_path);
        }
        written
    }
}

/// [`save_reporting`]'s staging half: every file of the set, written to its
/// `.tmp` sibling.
fn stage_all<B: FnMut(u8) -> SaveBaseline>(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &SystemOriginals,
    reporting: &mut SaveReporting<'_, B>,
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    // `_write_files` writes the libraries by sub-index, then the user pinyin
    // index, the user phrase index and the bigram (`pinyin.cpp:933-1020`):
    // where a `table.conf` gives two roles one name, the last writer's
    // contents are what the first rename moves into place.
    stage_rest(dir, source, originals, reporting, staged)?;
    tolerate(stage_dbm(
        dir,
        UserDbm::Bigram,
        |path| reporting.bigram_db.save_db(path),
        staged,
    ))
}

/// The save carries on past a file it could not write, as the pin's
/// `_write_files` does. A chunk header write the pin `assert`s on and an
/// encoding failure stop it, and so does a failed record copy, a
/// DBM copy that the pin asserts on: none of them is passed through here.
fn tolerate<T>(result: Result<T, PersistenceError>) -> Result<(), PersistenceError> {
    match result {
        Ok(_) => Ok(()),
        Err(error) if error.is_soft_write() => Ok(()),
        Err(error) => Err(error),
    }
}

/// The chunk bytes of one `USER_FILE` library.
///
/// # Errors
///
/// [`PersistenceError::SaveTooLarge`] where `SubPhraseIndex::store` would
/// copy the compacted content into a fresh chunk in one call: the save stops here, after the earlier files, without writing later files.
fn user_library_chunk(
    libraries: &crate::user::user_library::SessionPhrases,
    nibble: u8,
) -> Result<Vec<u8>, PersistenceError> {
    let empty = UserLibrary::default();
    let library = libraries.libraries.get(&nibble).unwrap_or(&empty);
    if library.store_copy_is_unsafe() {
        return Err(PersistenceError::SaveTooLarge);
    }
    library.store().ok_or(PersistenceError::Codec)
}

/// The chunk bytes of one `SYSTEM_FILE` library's journal.
///
/// # Errors
///
/// [`PersistenceError::SaveTooLarge`] where `PhraseIndexLogger::store` would
/// copy the whole journal into a fresh chunk in one call.
fn log_chunk(
    live: &crate::user::system_items::SystemItems,
    compacted: &[u8],
    original: SystemLibraryRef<'_>,
    nibble: u8,
    disk: SystemLibraryRef<'_>,
) -> Result<Vec<u8>, PersistenceError> {
    // A layout-failed library's save compact rebuilds its sub-index from the
    // items the session added (074a2219 `pinyin.cpp:1139`,
    // `phrase_index.cpp:648-675`), and the journal's header takes that total
    // (`phrase_index.cpp:395-400`): the readable item sum.
    let new_total = compacted
        .contains(&nibble)
        .then(|| live.compacted_total(nibble, original));
    let mut file = vec![0; CHUNK_HEADER_SIZE];
    encode_live_diff(
        live.live_library(nibble, original),
        disk,
        new_total.unwrap_or_else(|| live.library_total(nibble, Some(original)).unwrap_or(0)),
        &mut file,
    )
    .ok_or(PersistenceError::Codec)?;
    // Encode every record before the bulk-copy refusal: a later u16 length
    // failure takes precedence over an earlier crossing of the copy limit.
    if (file.len() - CHUNK_HEADER_SIZE) as u64 >= crate::user::user_library::copy_limit() {
        return Err(PersistenceError::SaveTooLarge);
    }
    finish_memory_chunk(file).ok_or(PersistenceError::Codec)
}

/// Every file of the set but the user bigram, staged as `.tmp` siblings.
fn stage_rest<B: FnMut(u8) -> SaveBaseline>(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &SystemOriginals,
    reporting: &mut SaveReporting<'_, B>,
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    let SaveSource {
        compacted,
        libraries,
        live,
    } = source;
    {
        // ---- the library files, by sub-index -----------------------------
        let empty_library = SystemLibrary::default();
        for (nibble, name, kind) in libraries_in_pin_order(originals.layout()) {
            // `_write_files` and `_rename_files` skip a library whose
            // sub-index is gone (`pinyin.cpp:929-932`, `:1032-1035`;
            // `zhuyin.cpp:562-565`): its file on disk stays as it is. The
            // session knows its unloaded `USER_FILE` libraries; the hook
            // answers for a system library.
            if matches!(kind, LibraryFile::User) && libraries.unloaded.contains(&nibble) {
                staged.unloaded.push(nibble);
                continue;
            }
            let bytes = match kind {
                LibraryFile::User => user_library_chunk(libraries, nibble)?,
                LibraryFile::Log => {
                    // The pin maps the current chunk at this point in the
                    // loop (`pinyin.cpp:956`), after the earlier libraries'
                    // files are written. The hook maps it and, for a layout
                    // fault, emits the critical here.
                    let baseline = (reporting.baseline)(nibble);
                    let original = originals.get(&nibble).unwrap_or(empty_library.view());
                    // The pin's current on-disk chunk is the old side of the
                    // diff.
                    let disk = match &baseline {
                        SaveBaseline::Unloaded => {
                            staged.unloaded.push(nibble);
                            continue;
                        }
                        // Register row 76: `MemoryChunk::mmap` refused the
                        // file. The save stops at this library with every
                        // earlier file as written.
                        SaveBaseline::Unmappable => {
                            return Err(PersistenceError::SystemRemapFailed);
                        }
                        SaveBaseline::Changed(disk) => disk.view(),
                        SaveBaseline::Keep => original,
                    };
                    log_chunk(live, compacted, original, nibble, disk)?
                }
            };
            tolerate(stage_chunk(dir, name, &bytes, staged))?;
        }

        // `_write_files` saves the user pinyin index, then the user phrase
        // index (`pinyin.cpp:996-1012`). Save each live container.
        tolerate(stage_dbm(
            dir,
            UserDbm::PinyinIndex,
            |path| libraries.index.save(path),
            staged,
        ))?;
        tolerate(stage_dbm(
            dir,
            UserDbm::PhraseIndex,
            |path| libraries.phrase_table.save(path),
            staged,
        ))?;

        Ok(())
    }
}

/// The live side of a save's diff: a library's live buffer, or its shipped
/// chunk where no write has made one.
#[derive(Clone, Copy)]
pub(crate) enum LiveSubIndex<'a> {
    /// The library's live buffer.
    Buffer(&'a crate::user::replay::ReplayedLibrary),
    /// The chunk as shipped.
    Chunk(SystemLibraryRef<'a>),
}

impl LiveSubIndex<'_> {
    fn end(&self) -> u32 {
        match self {
            Self::Buffer(buffer) => buffer.range_end(),
            Self::Chunk(chunk) => chunk.range_end,
        }
    }

    fn item(&self, slot: u32) -> Option<std::borrow::Cow<'_, [u8]>> {
        match self {
            Self::Buffer(buffer) => buffer.item_bytes(slot),
            Self::Chunk(chunk) => chunk.visible_bytes(slot),
        }
    }
}

/// `SubPhraseIndex::diff` (074a2219 `phrase_index.cpp:394-442`): the header
/// from the old sub-index's total to the live one's, then, over the union of
/// the two ranges in token order, an ADD where only the live side holds the
/// item, a REMOVE where only the old side does, and a MODIFY where both hold
/// it and the bytes differ. The old side is the chunk mapped again from disk
/// (`pinyin.cpp:952-959`, `zhuyin.cpp:585-592`); the live side is read in
/// place, an item as `get_phrase_item` finds it.
pub(crate) fn encode_live_diff(
    live: LiveSubIndex<'_>,
    disk: SystemLibraryRef<'_>,
    new_total: u32,
    out: &mut Vec<u8>,
) -> Option<()> {
    append_log_record(
        out,
        &LogRecord::<&[u8]>::ModifyHeader {
            old_total: disk.total,
            new_total,
        },
    )?;
    let end = live.end().max(disk.range_end);
    for slot in 1..end {
        let old = disk.visible_bytes(slot);
        let new = live.item(slot);
        let record = match (&old, &new) {
            (Some(old), Some(new)) if old != new => LogRecord::Modify {
                token: slot,
                old_item: old.as_ref(),
                new_item: new.as_ref(),
            },
            (Some(old), None) => LogRecord::Remove {
                token: slot,
                old_item: old.as_ref(),
            },
            (None, Some(new)) => LogRecord::Add {
                token: slot,
                new_item: new.as_ref(),
            },
            _ => continue,
        };
        append_log_record(out, &record)?;
    }
    Some(())
}

/// Writes one of the two index DBMs to its `.tmp` sibling and registers
/// the rename. The user bigram takes a separate route in [`stage_all`],
/// its container's own `save_db`, since the container is backend-specific. `_write_files` unlinks
/// the `.tmp` path first (`pinyin.cpp:999`, `:1008`), then saves the container.
fn stage_dbm(
    dir: &Path,
    dbm: UserDbm,
    save: impl FnOnce(&Path) -> Result<(), StoreError>,
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    let final_path = build_path(dir, OsStr::new(dbm.file_name()));
    let tmp = build_path(dir, OsStr::new(&format!("{}.tmp", dbm.file_name())));
    // `unlink(tmpfilename)` before `save_db` (`pinyin.cpp:1017`); the pin
    // ignores its result, and the backend's `save_db` then unlinks again as
    // the pin's does.
    unlink(&tmp);
    let written = save(&tmp).map_err(PersistenceError::from);
    staged.finish(&tmp, &final_path, written)
}

/// Writes one chunk file to its `.tmp` sibling and registers the rename.
fn stage_chunk(
    dir: &Path,
    name: &FileName,
    bytes: &[u8],
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    let final_path = build_path(dir, name.as_ref());
    let tmp = build_path(dir, name.with_suffix(".tmp").as_ref());
    let written = write_chunk_file(&tmp, bytes);
    staged.finish(&tmp, &final_path, written)
}

/// `MemoryChunk::save` (`memory_chunk.h:536-557`), made by the boundary's
/// chunk save: the pin's `open`, two header `write`s, one payload `write`,
/// `g_fsync` and `close`. A short header write is the failure the pin
/// `assert`s on; the open and a short payload write are
/// the pin's soft `false`, carried as `Io`.
fn write_chunk_file(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    crate::input::chunk::save(path, bytes).map_err(PersistenceError::Io)
}

#[cfg(test)]
mod tests;
