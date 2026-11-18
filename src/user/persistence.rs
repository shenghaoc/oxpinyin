//! The user directory as the backend's native user files — drop-in
//! task 9's persistence half.
//!
//! The store's durable representation is the pin's file set
//! (`docs/findings/user-store.md` §4 for the save cycle's semantics,
//! `crate::data::user_files` for the inventory and codecs):
//!
//! * load — `check_format` (user.conf conformance, the open counter,
//!   `_clean_user_files` on a non-conform profile), then the bigram
//!   hash, the `USER_FILE` chunk stores (`user.bin`, `addon.bin`,
//!   `network.bin`), and the `SYSTEM_FILE` `.dbin` logs replayed onto the
//!   original system chunks, and the user pinyin index's records.
//!   `user_phrase_index.bin` records are loaded and saved verbatim
//!   ([`UserState::phrase_table`]); only phrase-table operations edit them.
//!   `user_pinyin_index.bin` is
//!   not a derivative — a reading merged into an existing phrase is
//!   never indexed
//!   (`pinyin.cpp:569-582`) — so its records are loaded
//!   ([`UserState::indexed`]) and saved back.
//! * save — the pin's `_write_files` + `_rename_files`: every file is
//!   written whole to a `.tmp` sibling, then renamed over its final one
//!   after another in the pin's order: libraries by index, the pinyin
//!   index, the phrase index, then the bigram. A failure before the first
//!   rename leaves the old profile. A failure or crash between renames
//!   can leave a mix of old and new files, as in libpinyin
//!   (`pinyin.cpp:1025-1130` at 074a2219).
//! * fini — `pinyin_fini`'s `user.conf` write: the open counter the load
//!   raised, lowered again.
//!
//! The two facades share the files and the `user.conf` codec but not its
//! lifecycle: [`UserConfLaw`] names which one a profile follows.
//!
//! Nothing here holds a container open: the DBM files are created,
//! written and closed during save, and opened read-only during load —
//! the pin's own lifecycle (`Bigram::load_db` copies into memory,
//! `save_db` writes a fresh file).

#[cfg(test)]
use crate::ChewingKey;
#[cfg(test)]
use crate::data::chunk_write::build_chunk;
use crate::data::phrase_library::{LibraryError, PhraseLibrary, RowFault};
#[cfg(test)]
use crate::data::row_format::pinyin_index::PinyinIndexItem;

use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::PHRASE_MASK;
use crate::data::chunk_format::build_memory_chunk;
use crate::data::chunk_write::{ChunkItem, ChunkWriteError, encode_phrase_item};
use crate::data::phrase_libraries::PhraseLibraries;
use crate::data::table_info::FileName;
#[cfg(test)]
use crate::data::user_files::read_chunk_payload;
use crate::data::user_files::{
    LogEnd, LogFault, LogRecord, SystemVersions, UserConfError, UserDbm, UserFileLayout,
    UserTableInfo, encode_log_records, get_open_counter, load_chunk_payload, log_records,
};
use crate::input::path::{build_path, rename, unlink};
use crate::store::{DefaultStore, DefaultUserBigramDb, RawReadStore, StoreError, UserBigramDb};

use crate::user::replay::{JournalFault, Merge, ReplayedLibrary, Stop, raw_unigram};
use crate::user::user_index::UserIndex;
use crate::user::user_library::UserLibrary;
use crate::user::user_phrase_table::UserPhraseTable;

/// `USER_TABLE_INFO` (`pinyin_internal.h:56`).
const USER_CONF: &str = "user.conf";

/// `UserTableInfo::load` of `filename` (`pinyin.cpp:178`, `zhuyin.cpp:132`),
/// splitting upstream's two failure kinds the way `check_format` meets them.
///
/// `Ok(None)` is the load's `false`: `fopen` failed, after the pin's `open
/// %s failed.` line, or a version directive did not convert
/// (`table_info.cpp:330-348`). Upstream then runs its conformance check
/// against the reset defaults, so the profile reads as non-conforming and is
/// wiped — the session load does the same.
///
/// `Err` is the pin's abort site: the `database format:` directive reached
/// `to_table_database_format_type`, which `abort()`s upstream
/// (`:122-133`, called at `:353-354`). No profile is judged and nothing
/// is written or cleaned — the open fails here, as `check_format` never
/// gets to run, and the C adapters abort on it.
fn load_user_conf(filename: &Path) -> Result<Option<UserTableInfo>, PersistenceError> {
    match UserTableInfo::load_path(filename) {
        Ok(info) => Ok(Some(info)),
        Err(UserConfError::Unopenable | UserConfError::Line(_)) => Ok(None),
        Err(UserConfError::UnknownDatabaseFormat) => Err(PersistenceError::UnknownDatabaseFormat),
    }
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
    /// the loop (`pinyin.cpp:956`, `zhuyin.cpp:589`; register row 76). The
    /// pin writes `mmap %s failed!` and dies; the registered safe outcome
    /// stops the save here: the earlier libraries' files stay as written
    /// (libzhuyin has renamed them), nothing later is written or renamed, no
    /// marker is written, the session stays dirty and the save answers
    /// `false`.
    Unmappable,
    /// The library's system file is a directory whose header read
    /// `MemoryChunk::mmap` asserts on at this point of the loop
    /// (`pinyin.cpp:955`, `zhuyin.cpp:588`, `memory_chunk.h:493`; register
    /// row 96): the save stops here, with the earlier libraries' `.tmp`
    /// files written (and, for libzhuyin, renamed) and nothing else done.
    ChunkHeaderRead,
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

    /// Whether [`Self::item`] would answer `Some` for `slot`, without building
    /// the item: an inline item, or a mapped one whose phrase text decodes.
    pub(crate) fn is_readable(&self, slot: u32) -> bool {
        self.items.borrow().contains_key(&slot)
            || self
                .mapped
                .as_ref()
                .is_some_and(|library| library.borrow().item(slot).is_some())
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

/// An operation's selection of original libraries. It retains no byte owner.
pub struct SelectedOriginals<'a> {
    owner: &'a SystemOriginals,
    mask: u16,
}

/// Baseline reads shared by the whole owner and an operation's selection.
pub trait OriginalLibraries {
    /// Configured profile filenames.
    fn layout(&self) -> &UserFileLayout;
    /// Borrow one selected original library.
    fn get(&self, nibble: &u8) -> Option<SystemLibraryRef<'_>>;
    /// Borrow selected libraries in nibble order.
    fn iter(&self) -> impl Iterator<Item = (&u8, SystemLibraryRef<'_>)>;
    /// Whether this operation includes a library.
    fn contains_key(&self, nibble: &u8) -> bool {
        self.get(nibble).is_some()
    }
    /// Selected library nibbles in order.
    fn keys(&self) -> impl Iterator<Item = &u8> {
        self.iter().map(|(nibble, _)| nibble)
    }
}

impl OriginalLibraries for SystemOriginals {
    fn layout(&self) -> &UserFileLayout {
        &self.layout
    }
    fn get(&self, nibble: &u8) -> Option<SystemLibraryRef<'_>> {
        self.get(nibble)
    }
    fn iter(&self) -> impl Iterator<Item = (&u8, SystemLibraryRef<'_>)> {
        self.iter()
    }
}

impl OriginalLibraries for SelectedOriginals<'_> {
    fn layout(&self) -> &UserFileLayout {
        self.owner.layout()
    }
    fn get(&self, nibble: &u8) -> Option<SystemLibraryRef<'_>> {
        if *nibble >= 16 || self.mask & (1 << *nibble) == 0 {
            return None;
        }
        self.owner.get(nibble)
    }
    fn iter(&self) -> impl Iterator<Item = (&u8, SystemLibraryRef<'_>)> {
        self.owner
            .iter()
            .filter(|(nibble, _)| self.mask & (1 << **nibble) != 0)
    }
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

    /// Select existing baselines for a transition without transferring their bytes.
    pub fn selected(&self, mask: u16) -> SelectedOriginals<'_> {
        SelectedOriginals { owner: self, mask }
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
    ) -> Result<(), LibraryError> {
        self.mapped.map_library(nibble, system_dir, file.as_ref())?;
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
                    .library(crate::phrase_index_make_token(*nibble, 0))
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
                            .library(crate::phrase_index_make_token(*nibble, 0))
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
    /// The old implementation's dictionary and journal baseline could name
    /// different mappings after a refused replay or a transient reload. Move
    /// the old mapping into the original's slot before replacing the current
    /// dictionary slot; both versions remain uniquely owned.
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
    let library = libraries.library(crate::phrase_index_make_token(nibble, 0))?;
    Some(SystemLibrary {
        total: library.total_freq(),
        range_end: library.token_range().end,
        items: BTreeMap::new(),
        mapped: None,
    })
}

/// The user state, in the value shapes the file set carries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserState {
    /// The user bigram's rows verbatim, `(key, SingleGram value)` in the
    /// file's walk order: what a test reads back and seeds a session with.
    /// A session never fills it — its grams live in the
    /// pin's own container, which its saves write instead.
    pub bigram: Vec<(Vec<u8>, Vec<u8>)>,
    /// The user phrase table's raw UCS-4 keys and token-array values.
    /// Loaded and saved verbatim, including prefix markers, duplicate tokens,
    /// and partial bytes, as `PhraseLargeTable3::load_db` / `save_db` copy
    /// the records (074a2219, `phrase_large_table3_bdb.cpp:128-174`).
    /// Phrase-table operations alone edit these records; library items do
    /// not reconstruct their contents.
    pub phrase_table: BTreeMap<Vec<u8>, Vec<u8>>,
    /// The `USER_FILE` sub-indexes (5, 6, 7) by nibble, items by slot.
    pub libraries: crate::user::LibraryTable<BTreeMap<u32, ChunkItem>>,
    /// The `SYSTEM_FILE` libraries' touched items by nibble and slot:
    /// `Some(item)` modifies the original, `None` removes it. A slot
    /// absent here still answers its original item.
    pub system_overrides: crate::user::LibraryTable<BTreeMap<u32, Option<ChunkItem>>>,
    /// The `USER_FILE` readings the user pinyin index carries, as
    /// `(token, packed ChewingKey words)`. `_add_phrase` indexes the
    /// reading a phrase is created with and never one merged into an
    /// existing phrase (`pinyin.cpp:569-600`), so the index is not a
    /// derivative of the items: it is loaded from `user_pinyin_index.bin`
    /// and saved from this set.
    pub indexed: std::collections::BTreeSet<(u32, Vec<u16>)>,
}

impl UserState {
    /// Fixture state whose `USER_FILE` phrases and readings were all indexed
    /// when created. Production state never reconstructs phrase-table rows.
    #[cfg(test)]
    pub(crate) fn index_every_reading(mut self) -> Self {
        let mut phrases = Vec::new();
        for (&nibble, items) in &self.libraries {
            for (&slot, item) in items {
                phrases.push((
                    item.phrase.clone(),
                    crate::phrase_index_make_token(nibble, slot),
                ));
                for (packed, _) in &item.prons {
                    self.indexed
                        .insert((crate::phrase_index_make_token(nibble, slot), packed.clone()));
                }
            }
        }
        self.phrase_table = crate::data::table_entries::phrase_index_entries(&phrases)
            .into_iter()
            .collect();
        self
    }
}

/// Which of `MemoryChunk::save`'s two header writes the filesystem
/// refused, from the boundary's own chunk save: the `length` word
/// (`memory_chunk.h:543`) or the `checksum` word (`:547`). Both sit under
/// `pinyin_save` / `zhuyin_save`, and the pin is built with asserts live,
/// so a full filesystem or an `RLIMIT_FSIZE` that cuts the header kills the
/// process. The save answers the failure, typed, and the C adapters abort.
pub use crate::input::chunk::ChunkHeaderField;

/// A persistence failure: I/O, a container, or a byte stream that does
/// not parse.
#[derive(Debug)]
pub enum PersistenceError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// A store container failed to open, write or read.
    Store(StoreError),
    /// A byte stream did not parse under its frozen format.
    Codec(String),
    /// A library chunk's `MemoryChunk::save` header write failed
    /// (`memory_chunk.h:543`/`:547`): the pin `assert`s and dies of
    /// SIGABRT. Carried apart from [`Self::Io`] because a short *payload*
    /// write is the pin's own soft failure (`pinyin_save` answers `false` and
    /// every caller ignores it), not an abort.
    ChunkHeaderWrite {
        /// Which header word the write refused.
        field: ChunkHeaderField,
        /// The underlying write error.
        source: std::io::Error,
    },
    /// A system library's file, mapped again at the save's `_write_files`
    /// position, is a directory whose header read `MemoryChunk::mmap`
    /// asserts on (`memory_chunk.h:493`, register row 96): the save stops
    /// there, as the pin dies there.
    LibraryChunkHeaderRead,
    /// `MemoryChunk::mmap` refused a system library's file at the save's
    /// remap (register row 76): the save stopped at that library.
    SystemRemapFailed,
    /// `user.conf` names a database format upstream's
    /// `to_table_database_format_type` does not know, where that function
    /// `abort()`s (`table_info.cpp:122-133`). The open fails here, with
    /// nothing cleaned or written, and the C adapters abort.
    UnknownDatabaseFormat,
    /// A bulk copy into a fresh chunk reaches the pin's unsafe threshold
    /// (register row 112, #1514): the save stops there and answers `false`,
    /// keeping the earlier files as written, and the session stays dirty.
    SaveTooLarge,
    /// A tkrzw or Berkeley DB record copy failed where the pin's
    /// `copy_tkrzwdb` or `copy_bdb` asserts ([`StoreError::FatalCopy`]).
    /// No soft path absorbs it: the load or save stops there, before any
    /// later rename or marker write, and the C adapters abort.
    FatalCopy,
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "user file io: {error}"),
            Self::Store(error) => write!(f, "user file store: {error}"),
            Self::Codec(message) => write!(f, "user file codec: {message}"),
            Self::ChunkHeaderWrite { field, source } => write!(
                f,
                "user chunk header ({}): {source}",
                field.assert_expression()
            ),
            Self::LibraryChunkHeaderRead => write!(
                f,
                "a system library is a directory whose chunk header cannot be read \
                 (upstream asserts, memory_chunk.h:493)"
            ),
            Self::SystemRemapFailed => write!(
                f,
                "a system library's file cannot be mapped again (upstream: mmap failed!)"
            ),
            Self::UnknownDatabaseFormat => write!(
                f,
                "user.conf: unknown database format (upstream aborts, \
                 table_info.cpp:122-133, so the open is refused)"
            ),
            Self::SaveTooLarge => write!(
                f,
                "a save copy reaches the pin's unsafe size (memory_chunk.h:95-98)"
            ),
            Self::FatalCopy => write!(f, "user file store: {}", StoreError::FatalCopy),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl PersistenceError {
    /// Whether a save carries on past this failure of one file, as the
    /// pin's `_write_files` and `zhuyin_save` do: a filesystem or container
    /// failure the pin answers with `false`. A copy the pin asserts on is
    /// never one, however it is wrapped.
    fn is_soft_write(&self) -> bool {
        match self {
            Self::Io(_) => true,
            Self::Store(error) => !matches!(error, StoreError::FatalCopy),
            Self::Codec(_)
            | Self::ChunkHeaderWrite { .. }
            | Self::LibraryChunkHeaderRead
            | Self::SystemRemapFailed
            | Self::UnknownDatabaseFormat
            | Self::SaveTooLarge
            | Self::FatalCopy => false,
        }
    }
}

impl From<std::io::Error> for PersistenceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StoreError> for PersistenceError {
    fn from(error: StoreError) -> Self {
        // Never a soft [`Self::Store`]: `tolerate` and `Staged::finish`
        // pass those through.
        if matches!(error, StoreError::FatalCopy) {
            Self::FatalCopy
        } else {
            Self::Store(error)
        }
    }
}

impl From<crate::data::chunk_format::ChunkFrameError> for PersistenceError {
    fn from(error: crate::data::chunk_format::ChunkFrameError) -> Self {
        PersistenceError::Codec(error.to_string())
    }
}

impl From<crate::data::chunk_write::ChunkWriteError> for PersistenceError {
    fn from(error: crate::data::chunk_write::ChunkWriteError) -> Self {
        Self::Codec(error.to_string())
    }
}

/// What the library loop reads of a profile, beside the session's own structures.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    /// The decoded state (empty on a fresh or wiped profile).
    pub state: UserState,
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
    /// sibling of [`UserState`] for the two abort sites — recorded from
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
pub(crate) fn chewing_rows(
    state: &UserState,
    originals: &impl OriginalLibraries,
) -> Vec<PinyinIndexItem> {
    chewing_rows_inner(state, originals)
}

#[cfg(test)]
fn chewing_rows_inner(
    state: &UserState,
    originals: &impl OriginalLibraries,
) -> Vec<PinyinIndexItem> {
    // The pinyin index holds the readings `add_index` saw. The phrase
    // index is independently stored and copied verbatim below.
    let mut chewing_rows: Vec<PinyinIndexItem> = Vec::new();
    for (&nibble, items) in &state.libraries {
        for (&slot, item) in items {
            let token = crate::phrase_index_make_token(nibble, slot);
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
            let token = crate::phrase_index_make_token(nibble, slot);
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
        if packed.is_empty() || packed.len() > crate::MAX_PHRASE_LENGTH {
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
/// (`pinyin.cpp:187`; maintainer ruling 2026-10-28 UTC, D4): a marker that
/// cannot be written leaves the pin's `write %s failed.` line and the
/// profile is judged and used as it would be otherwise.
///
/// # Errors
///
/// [`PersistenceError::UnknownDatabaseFormat`], the abort point, before
/// any write or wipe.
pub(crate) fn check_format(
    dir: &Path,
    versions: &SystemVersions,
    layout: &UserFileLayout,
    law: UserConfLaw,
) -> Result<ProfileCheck, PersistenceError> {
    let existing = load_user_conf(&build_path(dir, OsStr::new(USER_CONF)))?;

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
        let _ = write_marker(dir, versions, open_counter);
    }
    if !conform {
        clean_user_files(dir, layout, law);
    }
    Ok(ProfileCheck {
        open_counter,
        conform,
    })
}

/// The init's profile load: the user tables first (`pinyin.cpp:350-373`),
/// then the library loop, each row judged, mapped and merged before the next
/// (`:377-392`). Without a conforming profile the loop only maps: the wipe
/// left nothing to read. The stop is the loop's first fault, after everything
/// the pin does before it.
///
/// # Errors
///
/// [`PersistenceError::FatalCopy`] where a user index copy fails, before
/// the loop (`pinyin.cpp:350-373` precede it).
pub(crate) fn load_session_in_loop(
    dir: &Path,
    originals: &mut SystemOriginals,
    rows: &crate::user::library_loop::LibraryLoopRows<'_>,
    check: &ProfileCheck,
) -> Result<
    (
        LoadedProfile,
        Result<(), crate::user::library_loop::LibraryLoopStop>,
    ),
    PersistenceError,
> {
    let mut profile = begin_session_profile(dir, check)?;
    let stop = crate::user::library_loop::run(
        rows.steps(),
        rows.system_dir(),
        originals,
        Some(SessionProfile {
            user_dir: dir,
            profile: &mut profile,
            conform: check.conform,
        }),
    );
    Ok((profile, stop))
}

/// The part of a session load that precedes the library loop.
///
/// # Errors
///
/// [`PersistenceError::FatalCopy`] where a user index copy fails.
pub(crate) fn begin_session_profile(
    dir: &Path,
    check: &ProfileCheck,
) -> Result<LoadedProfile, PersistenceError> {
    let mut profile = LoadedProfile {
        loaded: Loaded {
            open_counter: check.open_counter,
            #[cfg(test)]
            wiped: !check.conform,
            ..Loaded::default()
        },
        replayed: crate::user::LibraryTable::new(),
        user_libraries: crate::user::LibraryTable::new(),
        overlong_index_keys: Vec::new(),
        index: UserIndex::default(),
        phrase_table: UserPhraseTable::default(),
    };
    if check.conform {
        load_user_pinyin_index(
            dir,
            &mut profile.loaded,
            &mut profile.overlong_index_keys,
            &mut profile.index,
        )?;
        load_user_phrase_index(dir, &mut profile.loaded, &mut profile.phrase_table)?;
    }
    Ok(profile)
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
                Some(index),
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

/// The facade's fini-time `user.conf` write under `law`: libpinyin
/// lowers the counter the load raised and writes the conform marker
/// (`pinyin_fini`, `pinyin.cpp:1195-1200`, whose `mark_version` runs
/// whether or not a save came first); libzhuyin writes nothing
/// (`zhuyin_fini`, `zhuyin.cpp:741-757`). Nothing else of the profile
/// is touched. `dir` is `None` for a context without a user dir, whose
/// marker the pin names `""`.
///
/// A process that never reaches its fini leaves the load's raised value
/// on disk, as upstream's does. The save's answer is ignored, as
/// `pinyin_fini` ignores it (`:1200`).
pub fn fini(dir: Option<&Path>, versions: &SystemVersions, law: UserConfLaw, open_counter: i32) {
    if law == UserConfLaw::Pinyin {
        let counter = lowered_open_counter(open_counter);
        let _ = match dir {
            Some(dir) => write_marker(dir, versions, counter),
            None => write_unnamed_marker(versions, counter),
        };
    }
}

/// `check_format` for a context without a user dir: the pin keeps
/// `m_user_dir` NULL, so `g_build_filename(NULL, "user.conf", NULL)` is
/// `""` (`pinyin.cpp:176-178`, `zhuyin.cpp:130-132`). The load's `fopen("")`
/// fails with its `open  failed.` line, and libpinyin's save
/// (`pinyin.cpp:187`) fails with `write  failed.`. The wipe that follows
/// names files under `""` too and removes nothing.
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
/// Berkeley DB is [`PersistenceError::FatalCopy`] ([`RawReadStore::copy_raw`]).
fn load_user_pinyin_index(
    dir: &Path,
    loaded: &mut Loaded,
    overlong: &mut Vec<Vec<u16>>,
    index: &mut UserIndex,
) -> Result<(), PersistenceError> {
    let path = build_path(dir, OsStr::new(&UserDbm::PinyinIndex.file_name()));
    if !path.exists() {
        return Ok(());
    }
    let store = match DefaultStore::open_user_index(&path) {
        Ok(store) => store,
        Err(error) => {
            record_skip(
                &mut loaded.skip_notes(),
                format_args!("{}: {error}", UserDbm::PinyinIndex.file_name()),
            );
            return Ok(());
        }
    };
    let mut exceptional = Vec::new();
    // `ChewingLargeTable2::load_db`'s copy into its in-memory table: on
    // tkrzw and Berkeley DB a failed walk is fatal, as the pin's copy
    // asserts; Kyoto Cabinet's snapshot load keeps what it read.
    let mut set_failed = false;
    match store.copy_raw(|key, value| {
        set_failed |= index.push_record(key, value).is_err();
        if key.len() < 2 || key.len() / 2 > crate::MAX_PHRASE_LENGTH {
            exceptional.push(key.to_vec());
        }
    }) {
        Ok(()) => {}
        Err(StoreError::FatalCopy) => return Err(PersistenceError::FatalCopy),
        Err(error) => record_skip(
            &mut loaded.skip_notes(),
            format_args!("{}: {error}", UserDbm::PinyinIndex.file_name()),
        ),
    }
    if set_failed {
        return Err(PersistenceError::FatalCopy);
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
    Ok(())
}

/// Copy `user_phrase_index.bin` records without interpreting either key or value,
/// as the pin's `PhraseLargeTable3::load_db` does. A missing file or one that
/// does not open leaves the table empty; a failed copy is as for the pinyin
/// index.
fn load_user_phrase_index(
    dir: &Path,
    loaded: &mut Loaded,
    arena: &mut UserPhraseTable,
) -> Result<(), PersistenceError> {
    let path = build_path(dir, OsStr::new(&UserDbm::PhraseIndex.file_name()));
    if !path.exists() {
        return Ok(());
    }
    let store = match DefaultStore::open_user_index(&path) {
        Ok(store) => store,
        Err(error) => {
            record_skip(
                &mut loaded.skip_notes(),
                format_args!("{}: {error}", UserDbm::PhraseIndex.file_name()),
            );
            return Ok(());
        }
    };
    // `PhraseLargeTable3::load_db`'s copy, as for the pinyin index.
    let mut set_failed = false;
    let copied = store.copy_raw(|key, value| set_failed |= arena.push_record(key, value).is_err());
    if set_failed {
        return Err(PersistenceError::FatalCopy);
    }
    match copied {
        Ok(()) => {}
        Err(StoreError::FatalCopy) => return Err(PersistenceError::FatalCopy),
        Err(error) => record_skip(
            &mut loaded.skip_notes(),
            format_args!("{}: {error}", UserDbm::PhraseIndex.file_name()),
        ),
    }
    Ok(())
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

/// The `USER_FILE` chunk stores, or only library `only`'s.
fn load_libraries(
    dir: &Path,
    layout: &UserFileLayout,
    loaded: &mut Loaded,
    only: Option<u8>,
    libraries: &mut crate::user::LibraryTable<UserLibrary>,
) {
    for (nibble, file) in layout.user_libraries() {
        let nibble = *nibble;
        if only.is_some_and(|only| only != nibble) {
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
            Ok(library) => {
                loaded.library_totals.insert(nibble, library.total());
                // An absent library and an empty one are the same state;
                // keep the map absent so save/load are byte- and
                // value-stable round trips.
                if !library.is_empty() {
                    libraries.insert(nibble, library);
                }
            }
            Err(error) => {
                record_skip(&mut loaded.skip_notes(), format_args!("{name}: {error}"));
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
/// The [`JournalFault`] the merge reaches (rows 91 and 108).
#[cfg(test)]
pub(crate) fn load_library_logs(
    dir: &Path,
    originals: &impl OriginalLibraries,
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
    load_libraries(dir, layout, &mut loaded, Some(nibble), &mut libraries);
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
/// The [`JournalFault`] the first library's log reaches (rows 91, 92, 97 and
/// 108's read in `_compute_new_header`); no later library is read.
#[cfg(test)]
pub(crate) fn load_masked_logs(
    dir: &Path,
    originals: &impl OriginalLibraries,
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

/// The validated payloads (or failed loads) `mask_out`'s walk already read,
/// by sub-index, so replay consumes the result of that one read. A library absent
/// here is read when the replay reaches it.
///
/// The walk stops at the first fault, so it holds the bytes of the journals
/// it reached and no others: linear in their total size, over at most 15
/// library slots (sub-index 0 is never remapped), one `Vec` each. The pin
/// holds the same bytes, one library at a time, in `MemoryChunk::load`.
#[derive(Default)]
pub struct MaskLogs(crate::user::LibraryTable<std::io::Result<Vec<u8>>>);

/// Read library `nibble`'s saved log once into `logs` and return the header
/// fault `merge_with_mask` reaches in it, without touching any state:
/// `_peek_header` asserts on a non-null header token (row 91) or on a second
/// header (row 92), and the masked totals read a kept item shorter than its
/// unigram. `None` includes a successfully loaded library excluded by the
/// mask (its records are not parsed), an absent or unreadable log, a log with
/// no header, and merge faults that need the library's content. A failed chunk
/// header read aborts during the load, including for an excluded library.
pub(crate) fn read_mask_log(
    dir: &Path,
    layout: &UserFileLayout,
    logs: &mut MaskLogs,
    nibble: u8,
    mask: u32,
    value: u32,
) -> Option<JournalFault> {
    let (_, file) = layout.system_logs().iter().find(|(n, _)| *n == nibble)?;
    let read = load_chunk_payload(&build_path(dir, file.as_ref()));
    let selected = (nibble & crate::phrase_index_library_index(mask))
        == crate::phrase_index_library_index(value);
    let fault = if selected {
        read.as_ref()
            .ok()
            .and_then(|payload| masked_log(payload, mask, value, nibble).err())
    } else {
        None
    };
    logs.0.insert(nibble, read);
    fault
}

/// The replay of every `SYSTEM_FILE` library's log, in sub-index order: the
/// pin's merge (or, with `mask`, `merge_with_mask`) per library. The first
/// assertion ends it.
fn replay_logs(
    dir: &Path,
    originals: &impl OriginalLibraries,
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
        .unwrap_or_else(|| load_chunk_payload(&build_path(dir, file.as_ref())));
    match read {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
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
    merge: Merge<'_, '_>,
    loaded: &mut Loaded,
    replayed: &mut crate::user::LibraryTable<ReplayedLibrary>,
    nibble: u8,
    name: &impl std::fmt::Display,
) {
    let (total, gone, retained) = merge.finish_views(&mut loaded.skip_notes(), &name);
    if let Some(retained) = retained {
        replayed.insert(nibble, retained);
    }
    if let Some(total) = total {
        loaded.library_totals.insert(nibble, total);
    }
    // An empty override set and an absent one are the same state. A slot that
    // answers no item is an override without an item.
    if !gone.is_empty() {
        let overrides = gone.into_iter().map(|slot| (slot, None)).collect();
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
    originals: &impl OriginalLibraries,
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
        (nibble & crate::phrase_index_library_index(mask))
            != crate::phrase_index_library_index(value)
    }) {
        return Ok(());
    }
    let payload = bytes.as_slice();
    // The pin emits local slots, but its reader masks high bits even
    // in hand-written records (074a2219 phrase_index.cpp:180-215, 219-232).
    // `SubPhraseIndex::merge` over the records, in the pin's own terms:
    // bytes compared as bytes, payloads kept whole, the first failure the
    // end (`phrase_index.cpp:439-517`). Session opens retain the byte
    // views.
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
    /// The walk reached a payload past the log's end (class (b)).
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
    merge: &mut Merge<'_, 'a>,
    records: impl Fn() -> I,
    library: u8,
    skipped: &mut Option<&mut Vec<String>>,
    name: &str,
) -> Result<MergeEnd, JournalFault>
where
    I: Iterator<Item = Result<LogRecord<&'a [u8]>, LogFault>>,
{
    for record in records() {
        let record = match record {
            Ok(record) => record,
            Err(LogFault::HeaderToken(token)) => {
                return Err(JournalFault::HeaderToken { library, token });
            }
            // The masked records hold only the walk's prefix, so only the
            // unmasked replay reaches this.
            Err(LogFault::OverRead) => return Ok(MergeEnd::OverRead),
        };
        let outcome = merge.apply(record);
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
/// A payload past the log's end (the pin's over-read, class (b)) ends every
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
    let (records, end) = read_log(payload, library)?;
    let Some(old_total) = sole_header(&records, library)? else {
        return Ok(None);
    };
    // `_peek_header` ended on a record type it does not know: `m_error`
    // survives both rewinds (`phrase_index_logger.h:128-129`, `:141-144`),
    // so `_compute_new_header` and `_mask_out_records` read nothing
    // (`phrase_index.cpp:851-869`) and the new logger is the header alone,
    // its total unchanged.
    if end == Some(LogEnd::UnknownType) {
        return Ok(Some(vec![LogRecord::ModifyHeader {
            old_total,
            new_total: old_total,
        }]));
    }
    let new_total = masked_total(&records, old_total, mask, library)?;
    retained_records(records, old_total, new_total, mask, library).map(Some)
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

/// A log's records and how its walk ended.
type LogWalk<'a> = (Vec<LogRecord<&'a [u8]>>, Option<LogEnd>);

/// `_peek_header`'s walk: every record up to the log's end or a payload past
/// it, and how the walk ended.
///
/// # Errors
///
/// [`JournalFault::HeaderToken`] for a header record with a non-null token.
fn read_log(payload: &[u8], library: u8) -> Result<LogWalk<'_>, JournalFault> {
    let mut records = Vec::new();
    let mut walk = log_records(payload);
    for record in walk.by_ref() {
        match record {
            Ok(record) => records.push(record),
            Err(LogFault::HeaderToken(token)) => {
                return Err(JournalFault::HeaderToken { library, token });
            }
            Err(LogFault::OverRead) => break,
        }
    }
    Ok((records, walk.end()))
}

/// The old total of the log's one header; `None` when it has no header.
///
/// # Errors
///
/// [`JournalFault::MultipleHeaders`] for more than one header.
fn sole_header(records: &[LogRecord<&[u8]>], library: u8) -> Result<Option<u32>, JournalFault> {
    let mut headers = records.iter().filter_map(|record| {
        if let LogRecord::ModifyHeader { old_total, .. } = record {
            Some(*old_total)
        } else {
            None
        }
    });
    let Some(old_total) = headers.next() else {
        return Ok(None);
    };
    if headers.next().is_some() {
        return Err(JournalFault::MultipleHeaders { library });
    }
    Ok(Some(old_total))
}

/// `_compute_new_header`: the old total moved by every record the mask keeps.
///
/// # Errors
///
/// [`JournalFault::ShortMaskedItem`] for the first kept payload under six
/// bytes, in record order.
fn masked_total(
    records: &[LogRecord<&[u8]>],
    old_total: u32,
    mask: TokenMask,
    library: u8,
) -> Result<u32, JournalFault> {
    let unigram = |item: &[u8], token: u32| {
        if item.len() < 6 {
            Err(JournalFault::ShortMaskedItem { library, token })
        } else {
            Ok(raw_unigram(item))
        }
    };
    let mut new_total = old_total;
    for record in records {
        match *record {
            LogRecord::Add { token, new_item } if mask.keeps(token) => {
                new_total = new_total.wrapping_add(unigram(new_item, token)?);
            }
            LogRecord::Remove { token, old_item } if mask.keeps(token) => {
                new_total = new_total.wrapping_sub(unigram(old_item, token)?);
            }
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } if mask.keeps(token) => {
                new_total = new_total.wrapping_sub(unigram(old_item, token)?);
                new_total = new_total.wrapping_add(unigram(new_item, token)?);
            }
            LogRecord::Add { .. }
            | LogRecord::Remove { .. }
            | LogRecord::Modify { .. }
            | LogRecord::ModifyHeader { .. } => {}
        }
    }
    Ok(new_total)
}

/// `_mask_out_records`: the new logger's records, the header first and then
/// each kept MODIFY record.
///
/// # Errors
///
/// [`JournalFault::SurvivingRecord`] for the first kept ADD or REMOVE record,
/// in record order.
fn retained_records(
    records: Vec<LogRecord<&[u8]>>,
    old_total: u32,
    new_total: u32,
    mask: TokenMask,
    library: u8,
) -> Result<Vec<LogRecord<&[u8]>>, JournalFault> {
    let mut masked = vec![LogRecord::ModifyHeader {
        old_total,
        new_total,
    }];
    for record in records {
        match record {
            LogRecord::Add { token, .. } | LogRecord::Remove { token, .. } if mask.keeps(token) => {
                return Err(JournalFault::SurvivingRecord { library, token });
            }
            LogRecord::Modify { token, .. } if mask.keeps(token) => masked.push(record),
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
pub(crate) fn journal_item_bytes(item: &ChunkItem) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(6 + 4 * item.phrase.len());
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

/// Encode a typed persisted item, independently of phrase-import validation.
/// An empty phrase occupies `6 + 4 * n_prons` bytes at a nonzero offset
/// (074a2219 `phrase_index.cpp:180-217`); each pronunciation still needs
/// exactly one key per phrase word. The pin's `diff` writes these items
/// (`:392-436`). Public imports keep the strict [`encode_phrase_item`].
pub(crate) fn encode_persisted_item(item: &ChunkItem) -> Result<Vec<u8>, ChunkWriteError> {
    if item.phrase.is_empty()
        && item.prons.len() <= usize::from(u8::MAX)
        && item.prons.iter().all(|(keys, _)| keys.is_empty())
    {
        return Ok(journal_item_bytes(item));
    }
    encode_phrase_item(item)
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

/// The save's I/O parameters and its optional current-on-disk baseline
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
    pub(crate) baseline: Option<B>,
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
    libraries.sort_by_key(|&(nibble, _, _)| nibble);
    libraries
}

/// The save with the current-on-disk baseline hook.
pub(crate) fn save_reporting<B: FnMut(u8) -> SaveBaseline>(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &impl OriginalLibraries,
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
    crate::data::c_stderr::rename_failed(tmp, final_path);
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
    /// libzhuyin attempts the rename even after a soft write failure
    /// (074a2219 zhuyin.cpp:612-617, :645-650, :669-691). A header failure
    /// aborts before the rename; the reporting caller refuses it instead.
    fn finish(
        &mut self,
        paths: (PathBuf, PathBuf),
        written: Result<(), PersistenceError>,
    ) -> Result<(), PersistenceError> {
        if written.is_ok()
            || (self.immediate && written.as_ref().is_err_and(PersistenceError::is_soft_write))
        {
            self.push(paths);
        }
        written
    }

    fn push(&mut self, (tmp, final_path): (PathBuf, PathBuf)) {
        if self.immediate && rename(&tmp, &final_path).is_err() {
            report_rename_failed(&tmp, &final_path);
        }
    }
}

/// [`save_reporting`]'s staging half: every file of the set, written to its
/// `.tmp` sibling.
fn stage_all<B: FnMut(u8) -> SaveBaseline>(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &impl OriginalLibraries,
    reporting: &mut SaveReporting<'_, B>,
    staged: &mut Staged,
) -> Result<(), PersistenceError> {
    // `_write_files` writes the libraries by sub-index, then the user pinyin
    // index, the user phrase index and the bigram (`pinyin.cpp:933-1020`):
    // where a `table.conf` gives two roles one name, the last writer's
    // contents are what the first rename moves into place.
    stage_rest(dir, source, originals, reporting, staged)?;
    let final_path = build_path(dir, OsStr::new(&UserDbm::Bigram.file_name()));
    let tmp = build_path(
        dir,
        OsStr::new(&format!("{}.tmp", UserDbm::Bigram.file_name())),
    );
    // `unlink(tmpfilename)` before `save_db` (`pinyin.cpp:1017`); the
    // pin ignores its result, and the backend's `save_db` then unlinks
    // again as the pin's does.
    unlink(&tmp);
    let written = reporting
        .bigram_db
        .save_db(&tmp)
        .map_err(PersistenceError::from);
    tolerate(staged.finish((tmp, final_path), written))
}

/// The save carries on past a file it could not write, as the pin's
/// `_write_files` does. A chunk header write the pin `assert`s on and an
/// encoding failure stop it, and so does [`PersistenceError::FatalCopy`], a
/// DBM copy that the pin asserts on: none of them is passed through here.
fn tolerate<T>(result: Result<T, PersistenceError>) -> Result<(), PersistenceError> {
    match result {
        Ok(_) => Ok(()),
        Err(error) if error.is_soft_write() => Ok(()),
        Err(error) => Err(error),
    }
}

/// One `SYSTEM_FILE` library's journal as `_write_files` frames it
/// (`pinyin.cpp:944-975`): the live sub-index diffed against `baseline`'s
/// chunk, or the open-time original where there is none.
fn log_payload(
    live: &crate::user::system_items::SystemItems,
    nibble: u8,
    original: SystemLibraryRef<'_>,
    baseline: Option<&SaveBaseline>,
    new_total: Option<u32>,
) -> Result<Vec<u8>, PersistenceError> {
    // The pin's `_write_files`: the chunk on disk now is the old side, the
    // live sub-index the new one.
    let disk = match baseline {
        Some(SaveBaseline::Changed(disk)) => disk.view(),
        Some(
            SaveBaseline::Keep
            | SaveBaseline::Unloaded
            | SaveBaseline::Unmappable
            | SaveBaseline::ChunkHeaderRead,
        )
        | None => original,
    };
    let records = diff_live_records(
        live.live_library(nibble, original),
        disk,
        new_total.unwrap_or_else(|| live.library_total(nibble, Some(original)).unwrap_or(0)),
    );
    encode_log_records(&records).map_err(|e| PersistenceError::Codec(e.to_string()))
}

/// The current-on-disk baseline of one `SYSTEM_FILE` library's journal:
/// the caller's hook, else the remap of a detached library's current chunk.
/// `None` diffs against the open-time original.
fn log_baseline<B: FnMut(u8) -> SaveBaseline>(
    hook: Option<&mut B>,
    originals: &impl OriginalLibraries,
    nibble: u8,
) -> Option<SaveBaseline> {
    hook.map(|hook| hook(nibble)).or_else(|| {
        let original = originals.get(&nibble)?;
        let mapped = original.mapped?;
        if !mapped.is_detached() {
            return None;
        }
        let path = mapped.source_path()?;
        Some(match PhraseLibrary::open_retaining_layout(path) {
            Ok(current) => SaveBaseline::Changed(SystemLibrary::from_library(current)),
            Err(LibraryError::Row(RowFault::ChunkHeaderRead)) => SaveBaseline::ChunkHeaderRead,
            Err(
                LibraryError::Io(_)
                | LibraryError::Format(_)
                | LibraryError::Layout(_)
                | LibraryError::Unmappable { .. }
                | LibraryError::Row(RowFault::NotDictionary | RowFault::NullChunkName),
            ) => SaveBaseline::Unmappable,
        })
    })
}

/// The chunk bytes of one `USER_FILE` library.
///
/// # Errors
///
/// [`PersistenceError::SaveTooLarge`] where `SubPhraseIndex::store` would
/// copy the compacted content into a fresh chunk in one call (register row
/// 112): the save stops here, after the earlier files, as row 76's stop does.
fn user_library_chunk(
    libraries: &crate::user::user_library::SessionPhrases,
    nibble: u8,
) -> Result<Vec<u8>, PersistenceError> {
    let empty = UserLibrary::default();
    let library = libraries.libraries.get(&nibble).unwrap_or(&empty);
    if library.store_copy_is_unsafe() {
        return Err(PersistenceError::SaveTooLarge);
    }
    library.store().map_err(PersistenceError::Codec)
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
    baseline: Option<&SaveBaseline>,
) -> Result<Vec<u8>, PersistenceError> {
    // A layout-failed library's save compact rebuilds its sub-index from the
    // items the session added (074a2219 `pinyin.cpp:1139`,
    // `phrase_index.cpp:648-675`), and the journal's header takes that total
    // (`phrase_index.cpp:395-400`): the readable item sum.
    let new_total = compacted
        .contains(&nibble)
        .then(|| live.compacted_total(nibble, original));
    let payload = log_payload(live, nibble, original, baseline, new_total)?;
    if payload.len() as u64 >= crate::user::user_library::copy_limit() {
        return Err(PersistenceError::SaveTooLarge);
    }
    build_memory_chunk(&payload).map_err(PersistenceError::from)
}

/// Every file of the set but the user bigram, staged as `.tmp` siblings.
fn stage_rest<B: FnMut(u8) -> SaveBaseline>(
    dir: &Path,
    source: SaveSource<'_>,
    originals: &impl OriginalLibraries,
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
            // The pin maps the current chunk at this point in the loop
            // (`pinyin.cpp:956`), after the earlier libraries' files are
            // written. The hook maps it and, for a layout fault, emits the
            // critical here.
            let baseline = match kind {
                LibraryFile::Log => log_baseline(reporting.baseline.as_mut(), originals, nibble),
                LibraryFile::User => None,
            };
            if matches!(baseline, Some(SaveBaseline::Unloaded)) {
                staged.unloaded.push(nibble);
                continue;
            }
            // Register row 76: `MemoryChunk::mmap` refused the file. The save
            // stops at this library with every earlier file as written.
            if matches!(baseline, Some(SaveBaseline::Unmappable)) {
                return Err(PersistenceError::SystemRemapFailed);
            }
            // Register row 96: the pin's `MemoryChunk::mmap` asserts here,
            // after the earlier libraries' files were written (libzhuyin
            // renamed each as it went).
            if matches!(baseline, Some(SaveBaseline::ChunkHeaderRead)) {
                return Err(PersistenceError::LibraryChunkHeaderRead);
            }
            let bytes = match kind {
                LibraryFile::User => user_library_chunk(libraries, nibble)?,
                LibraryFile::Log => {
                    let original = originals.get(&nibble).unwrap_or(empty_library.view());
                    log_chunk(live, compacted, original, nibble, baseline.as_ref())?
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
pub(crate) fn diff_live_records(
    live: LiveSubIndex<'_>,
    disk: SystemLibraryRef<'_>,
    new_total: u32,
) -> Vec<LogRecord> {
    let mut records = vec![LogRecord::ModifyHeader {
        old_total: disk.total,
        new_total,
    }];
    let end = live.end().max(disk.range_end);
    for slot in 1..end {
        let old = disk.visible_bytes(slot);
        let new = live.item(slot);
        match (old, new) {
            (Some(old), Some(new)) => {
                if old != new {
                    records.push(LogRecord::Modify {
                        token: slot,
                        old_item: old.into_owned(),
                        new_item: new.into_owned(),
                    });
                }
            }
            (Some(old), None) => records.push(LogRecord::Remove {
                token: slot,
                old_item: old.into_owned(),
            }),
            (None, Some(new)) => records.push(LogRecord::Add {
                token: slot,
                new_item: new.into_owned(),
            }),
            (None, None) => {}
        }
    }
    records
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
    let final_path = build_path(dir, OsStr::new(&dbm.file_name()));
    let tmp = build_path(dir, OsStr::new(&format!("{}.tmp", dbm.file_name())));
    unlink(&tmp);
    let written = save(&tmp).map_err(PersistenceError::from);
    staged.finish((tmp, final_path), written)
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
    staged.finish((tmp, final_path), written)
}

/// `MemoryChunk::save` (`memory_chunk.h:536-557`), made by the boundary's
/// chunk save: the pin's `open`, two header `write`s, one payload `write`,
/// `g_fsync` and `close`. A short header write is the failure the pin
/// `assert`s on, returned typed; the open and a short payload write are
/// the pin's soft `false`, carried as `Io`.
fn write_chunk_file(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    match crate::input::chunk::save(path, bytes) {
        Ok(()) => Ok(()),
        Err(crate::input::chunk::ChunkSaveError::Io(error)) => Err(PersistenceError::Io(error)),
        Err(crate::input::chunk::ChunkSaveError::Header { field, source }) => {
            Err(PersistenceError::ChunkHeaderWrite { field, source })
        }
    }
}

#[cfg(test)]
mod tests;
