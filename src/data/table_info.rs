//! The system `table.conf`, read the way libpinyin's
//! `SystemTableInfo2::load` reads it (`src/storage/table_info.cpp:194-294`
//! at `074a2219`), and the library set its rows describe.
//!
//! The file is scanned with the pin's own `setlocale`, `fopen`, `fscanf`,
//! `feof` and `atoi` calls through `crate::input::config`: five header
//! directives declare versions, λ, source and database format, then the row
//! loop reads six words at a time, across line breaks, as the scanner
//! groups them. This module keeps what the pin does with each answer.
//!
//! # What the rows mean
//!
//! A `default` row names one of the sixteen default sub-indices by its
//! enum name (`RESERVED`, `GB_DICTIONARY`/`TSI_DICTIONARY` = 1,
//! `GBK_DICTIONARY` = 2, `OPENGRAM_DICTIONARY` = 3, `MERGED_DICTIONARY` = 4,
//! `ADDON_DICTIONARY` = 5, `NETWORK_DICTIONARY` = 6, `USER_DICTIONARY` = 7);
//! an `addon` row names its sub-index by `atoi` of its word, cut to a
//! `guint8`. The three file columns are the model source table, the
//! system chunk and the user chunk (`NULL` is no name); the last column is
//! the file type that decides how the pin loads and saves the library.
//! Slots no row names stay `NOT_USED`; a later row for the same slot
//! replaces the earlier one.
//!
//! # Malformed files
//!
//! Every unknown token the pin `abort()`s on is a [`TableConfError::Abort`]
//! naming its site, and a header directive that does not match is the
//! pin's ordinary `false` return ([`TableConfError::Header`]). A missing
//! `source table format:` line leaves the pin comparing an uninitialised
//! buffer (`:228-229`): [`TableConfError::UninitialisedSourceFormat`].

use crate::vocab::PHRASE_INDEX_LIBRARY_COUNT;
use std::ffi::{CStr, OsStr, OsString};
use std::path::Path;

use crate::input::config::SystemTableInfoReader;

use crate::data::phrase_library::directory_reads_as_a_chunk;

/// `PHRASE_FILE_TYPE` (`table_info.h:44-49`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileType {
    /// `NOT_USED`.
    NotUsed,
    /// `SYSTEM_FILE`: a system chunk plus the user's `.dbin` diff log.
    SystemFile,
    /// `DICTIONARY`: a professional (addon) dictionary, system chunk only.
    Dictionary,
    /// `USER_FILE`: a chunk that lives in the user dir alone.
    UserFile,
}

/// One default row as the pin's init loop meets it
/// ([`TableConf::default_row_steps`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultRowStep<'a> {
    /// The loop dies at this row before it maps anything.
    Stop(TableConfSite),
    /// A `SYSTEM_FILE` row: map `file`, then merge the row's log. With
    /// `null_user_file` the mapping happens and the log's read then asserts.
    System {
        /// The system chunk's file name.
        file: &'a FileName,
        /// The row names no user file, where that is fatal.
        null_user_file: bool,
    },
    /// A `USER_FILE` row: read the library from the user directory.
    User,
}

/// `TABLE_PHONETIC_TYPE`: the `source table format:` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhoneticType {
    /// `pinyin`.
    Pinyin,
    /// `zhuyin`.
    Zhuyin,
}

/// `TABLE_DATABASE_FORMAT_TYPE`: the `database format:` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseFormat {
    /// `BerkeleyDB`.
    BerkeleyDb,
    /// `KyotoCabinet`.
    KyotoCabinet,
    /// `Tkrzw`.
    Tkrzw,
}

impl DatabaseFormat {
    /// The token `table.conf` and `user.conf` spell it with.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::BerkeleyDb => "BerkeleyDB",
            Self::KyotoCabinet => "KyotoCabinet",
            Self::Tkrzw => "Tkrzw",
        }
    }

    /// The format whose token is `token`.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        [Self::BerkeleyDb, Self::KyotoCabinet, Self::Tkrzw]
            .into_iter()
            .find(|format| format.token() == token)
    }
}

/// A file name as a `table.conf` column spells it: the bytes of the `%s`
/// word, kept raw (the pin opens whatever bytes it read), and joined to a
/// directory by `crate::input::build_path` — never as `Path::join`
/// does.
///
/// A name that starts with a separator stays beneath the directory; only an
/// empty directory leaves the name as it is. The type is not `AsRef<Path>`
/// so that no caller can join it by accident.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileName(OsString);

impl FileName {
    /// The name made of `bytes`.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(crate::input::path_from_bytes(bytes).into_os_string())
    }

    /// The name with `suffix` appended (`g_strdup_printf("%s.tmp", name)`).
    #[must_use]
    pub(crate) fn with_suffix(&self, suffix: &str) -> Self {
        let mut name = self.0.clone();
        name.push(suffix);
        Self(name)
    }

    /// The name for a message (invalid bytes replaced).
    #[must_use]
    pub fn display(&self) -> std::borrow::Cow<'_, str> {
        self.0.to_string_lossy()
    }
}

impl AsRef<OsStr> for FileName {
    fn as_ref(&self) -> &OsStr {
        &self.0
    }
}

/// One `pinyin_table_info_t` (`table_info.h:51-57`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableRow {
    /// The model source table (`m_table_filename`); `None` is `NULL`.
    pub table: Option<FileName>,
    /// The system chunk file (`m_system_filename`).
    pub system: Option<FileName>,
    /// The user chunk or diff log (`m_user_filename`).
    pub user: Option<FileName>,
    /// How the pin loads and saves the library.
    pub file_type: FileType,
}

impl TableRow {
    /// A slot no row names (`INIT_TABLE_INFO`).
    #[must_use]
    pub const fn unused() -> Self {
        Self {
            table: None,
            system: None,
            user: None,
            file_type: FileType::NotUsed,
        }
    }

    fn named(table: &[u8], system: &[u8], user: &[u8], file_type: FileType) -> Self {
        // `to_string` (`table_info.cpp:163-167`): `NULL` is no name, any
        // other word is `g_strdup`ed.
        let name = |text: &[u8]| (text != b"NULL").then(|| FileName::from_bytes(text));
        Self {
            table: name(table),
            system: name(system),
            user: name(user),
            file_type,
        }
    }
}

/// The place in the pin a malformed `table.conf` kills the process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableConfSite {
    /// `to_table_phonetic_type`'s `abort()` (`table_info.cpp:119`).
    SourceFormat,
    /// `to_table_database_format_type`'s `abort()` (`:132`).
    DatabaseFormat,
    /// `to_table_target`'s `abort()` (`:142`).
    Target,
    /// `to_index_of_default_tables`'s `abort()` (`:156`).
    DefaultIndex,
    /// `to_file_type`'s `abort()` (`:175`).
    FileType,
    /// `assert(0 <= index && index < PHRASE_INDEX_LIBRARY_COUNT)` (`:276`).
    AddonIndex,
    /// `assert(DICTIONARY != table_info->m_file_type)` in the init loop
    /// (`pinyin.cpp:388`, `zhuyin.cpp:330`), run after `check_format`.
    DefaultDictionary,
    /// A loaded row names no system chunk: `g_build_filename` returns the
    /// directory, and `MemoryChunk::mmap` asserts `ret_len ==
    /// sizeof(length)` on reading it (`memory_chunk.h:493`).
    NullSystemFile,
    /// A loaded row names no user file: the same read through
    /// `MemoryChunk::load` (`memory_chunk.h:434`).
    NullUserFile,
}

impl TableConfSite {
    /// The fixed warning a facade logs, once, where the pin would die.
    #[must_use]
    pub const fn warning(self) -> &'static str {
        match self {
            Self::SourceFormat => {
                "SystemTableInfo2::load: unknown source table format in table.conf"
            }
            Self::DatabaseFormat => "SystemTableInfo2::load: unknown database format in table.conf",
            Self::Target => "SystemTableInfo2::load: unknown table target in table.conf",
            Self::DefaultIndex => {
                "SystemTableInfo2::load: unknown default dictionary name in table.conf"
            }
            Self::FileType => "SystemTableInfo2::load: unknown file type in table.conf",
            Self::AddonIndex => {
                "SystemTableInfo2::load: assertion '0 <= index && index < \
                 PHRASE_INDEX_LIBRARY_COUNT' failed"
            }
            Self::DefaultDictionary => "assertion 'DICTIONARY != table_info->m_file_type' failed",
            Self::NullSystemFile => {
                "MemoryChunk::mmap: assertion 'ret_len == sizeof(length)' failed"
            }
            Self::NullUserFile => "MemoryChunk::load: assertion 'ret_len == sizeof(length)' failed",
        }
    }
}

/// Why a `table.conf` was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableConfError {
    /// `fopen` answered NULL (`:199-202`): the pin has written `open %s
    /// failed.\n` and returns `false`, after which `pinyin_init` writes
    /// `load %s failed!` and answers NULL. A path holding a NUL, which no C
    /// caller can pass, is refused here without a call.
    Unopenable,
    /// One of the first three directives did not match: the pin's ordinary
    /// `false` return (`:209-224`), after which `pinyin_init` writes
    /// `load %s failed!` and answers NULL. No warning.
    Header,
    /// The `source table format:` directive did not match, so the pin
    /// compares a buffer it never wrote (`:228-229`): undefined behaviour.
    UninitialisedSourceFormat,
    /// An invalid directive or slot, carrying its established boundary warning.
    /// Malformed addon fields can also reach this refusal under the native grammar.
    Abort(TableConfSite),
}

impl TableConfError {
    /// The warning a facade logs once for this refusal; `None` for the
    /// pin's ordinary `false` return, which is silent.
    #[must_use]
    pub const fn warning(self) -> Option<&'static str> {
        match self {
            Self::Unopenable | Self::Header => None,
            Self::UninitialisedSourceFormat => {
                Some("SystemTableInfo2::load: source table format line missing in table.conf")
            }
            Self::Abort(site) => Some(site.warning()),
        }
    }
}

impl core::fmt::Display for TableConfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unopenable => f.write_str("table.conf cannot be opened"),
            Self::Header => f.write_str("table.conf header does not match"),
            Self::UninitialisedSourceFormat => {
                f.write_str("table.conf has no source table format line")
            }
            Self::Abort(site) => f.write_str(site.warning()),
        }
    }
}

impl std::error::Error for TableConfError {}

/// A parsed system `table.conf`.
#[derive(Clone, Debug, PartialEq)]
pub struct TableConf {
    binary_format_version: i32,
    model_data_version: i32,
    lambda: f32,
    phonetic: PhoneticType,
    database_format: DatabaseFormat,
    default: Vec<TableRow>,
    addon: Vec<TableRow>,
}

impl TableConf {
    // Stock rows come from data/table.conf.in; pinyin_init loads table.conf
    // (pinyin.cpp:335–340).
    /// Constructs the stock table rows for the selected backend and supplied weight. Runtime
    /// initialization reads the installed configuration from disk.
    #[must_use]
    pub fn stock(lambda: f32) -> Self {
        let mut conf = Self {
            binary_format_version: 7,
            model_data_version: 14,
            lambda,
            phonetic: PhoneticType::Pinyin,
            database_format: DatabaseFormat::from_token(crate::store::DEFAULT_STORE_DB_FORMAT)
                .unwrap_or(DatabaseFormat::BerkeleyDb),
            default: vec![TableRow::unused(); usize::from(PHRASE_INDEX_LIBRARY_COUNT)],
            addon: vec![TableRow::unused(); usize::from(PHRASE_INDEX_LIBRARY_COUNT)],
        };
        let system = |conf: &mut Self, index: usize, stem: &str| {
            if let Some(slot) = conf.default.get_mut(index) {
                *slot = TableRow::named(
                    format!("{stem}.table").as_bytes(),
                    format!("{stem}.bin").as_bytes(),
                    format!("{stem}.dbin").as_bytes(),
                    FileType::SystemFile,
                );
            }
        };
        for &(index, stem) in crate::data::SYSTEM_LIBRARY_NAMES {
            system(&mut conf, usize::from(index), stem);
        }
        for (index, file) in [(5, "addon.bin"), (6, "network.bin"), (7, "user.bin")] {
            if let Some(slot) = conf.default.get_mut(index) {
                *slot = TableRow::named(b"NULL", b"NULL", file.as_bytes(), FileType::UserFile);
            }
        }
        for &(index, stem) in crate::data::ADDON_LIBRARY_NAMES {
            if let Some(slot) = conf.addon.get_mut(usize::from(index)) {
                *slot = TableRow::named(
                    format!("{stem}.table").as_bytes(),
                    format!("{stem}.bin").as_bytes(),
                    b"NULL",
                    FileType::Dictionary,
                );
            }
        }
        conf
    }

    /// `SystemTableInfo2::load(filename)`: the system `table.conf` as the
    /// pin's scanner reads it, or the point at which the pin returns
    /// `false` or dies.
    ///
    /// The process's `LC_NUMERIC` is `"C"` afterwards, on every path, as
    /// the pin leaves it.
    ///
    /// # Errors
    ///
    /// [`TableConfError`] where the pin returns `false` or dies.
    pub fn load(filename: &(impl crate::input::FileName + ?Sized)) -> Result<Self, TableConfError> {
        let input =
            SystemTableInfoReader::open(filename).map_err(|_| TableConfError::Unopenable)?;
        Self::read(input)
    }

    fn read(mut input: SystemTableInfoReader) -> Result<Self, TableConfError> {
        // Each early return drops `input`: the pin's `fclose`, without its
        // closing `setlocale` (`:209-224`).
        let mut conf = Self::read_header(&mut input)?;
        // `:254-287`, each row's checks in the pin's order.
        while !input.eof() {
            let Some(row) = input.table_row() else {
                continue;
            };
            conf.apply_row(row)?;
        }
        input.finish();
        Ok(conf)
    }

    /// The header of `table.conf`: the two versions, the lambda and the two
    /// format directives, with every table row unused.
    fn read_header(input: &mut SystemTableInfoReader) -> Result<Self, TableConfError> {
        let binary_format_version = input
            .binary_format_version()
            .assigned()
            .ok_or(TableConfError::Header)?;
        let model_data_version = input
            .model_data_version()
            .assigned()
            .ok_or(TableConfError::Header)?;
        let lambda = input
            .lambda_parameter()
            .assigned()
            .ok_or(TableConfError::Header)?;
        let phonetic = match input.source_table_format().assigned().map(CStr::to_bytes) {
            None => return Err(TableConfError::UninitialisedSourceFormat),
            Some(b"pinyin") => PhoneticType::Pinyin,
            Some(b"zhuyin") => PhoneticType::Zhuyin,
            Some(_) => return Err(TableConfError::Abort(TableConfSite::SourceFormat)),
        };
        let database_format = input
            .database_format()
            .assigned()
            .and_then(|token| token.to_str().ok())
            .and_then(DatabaseFormat::from_token)
            .ok_or(TableConfError::Abort(TableConfSite::DatabaseFormat))?;

        Ok(Self {
            binary_format_version,
            model_data_version,
            lambda,
            phonetic,
            database_format,
            default: vec![TableRow::unused(); usize::from(PHRASE_INDEX_LIBRARY_COUNT)],
            addon: vec![TableRow::unused(); usize::from(PHRASE_INDEX_LIBRARY_COUNT)],
        })
    }

    /// One table row, with its checks in the pin's order (`:254-287`): the
    /// target, the index, the index bound and the file type, then the write
    /// of the row.
    fn apply_row(
        &mut self,
        [target, index, table, system, user, file_type]: [&CStr; 6],
    ) -> Result<(), TableConfError> {
        let is_default = match target.to_bytes() {
            b"default" => true,
            b"addon" => false,
            _ => return Err(TableConfError::Abort(TableConfSite::Target)),
        };
        let index = if is_default {
            default_index(index.to_bytes())
                .ok_or(TableConfError::Abort(TableConfSite::DefaultIndex))?
        } else {
            // `to_index_of_addon_tables` returns `atoi` as a `guint8`.
            usize::from(crate::input::config::atoi(index) as u8)
        };
        if index >= usize::from(PHRASE_INDEX_LIBRARY_COUNT) {
            return Err(TableConfError::Abort(TableConfSite::AddonIndex));
        }
        let file_type = match file_type.to_bytes() {
            b"NOT_USED" => FileType::NotUsed,
            b"SYSTEM_FILE" => FileType::SystemFile,
            b"DICTIONARY" => FileType::Dictionary,
            b"USER_FILE" => FileType::UserFile,
            _ => return Err(TableConfError::Abort(TableConfSite::FileType)),
        };
        let slot = if is_default {
            self.default.get_mut(index)
        } else {
            self.addon.get_mut(index)
        };
        if let Some(slot) = slot {
            *slot = TableRow::named(
                table.to_bytes(),
                system.to_bytes(),
                user.to_bytes(),
                file_type,
            );
        }
        Ok(())
    }

    /// [`Self::load`] of `path`.
    ///
    /// # Errors
    ///
    /// As [`Self::load`]; a path holding a NUL is
    /// [`TableConfError::Unopenable`].
    pub fn load_path(path: &Path) -> Result<Self, TableConfError> {
        if crate::input::path_to_bytes(path).contains(&0) {
            return Err(TableConfError::Unopenable);
        }
        Self::load(path)
    }

    /// `SystemTableInfo2::load` of `table.conf` in `system_dir`, named as
    /// `g_build_filename(system_dir, "table.conf", NULL)` names it
    /// (`pinyin.cpp:335-337`, `zhuyin.cpp:279-281`).
    ///
    /// # Errors
    ///
    /// As [`Self::load_path`].
    pub fn load_from_dir(system_dir: &Path) -> Result<Self, TableConfError> {
        Self::load_path(&crate::input::build_path(
            system_dir,
            OsStr::new("table.conf"),
        ))
    }

    /// `binary format version:`.
    #[must_use]
    pub const fn binary_format_version(&self) -> i32 {
        self.binary_format_version
    }

    /// `model data version:`.
    #[must_use]
    pub const fn model_data_version(&self) -> i32 {
        self.model_data_version
    }

    /// `lambda parameter:` as the pin holds it, a `gfloat`.
    #[must_use]
    pub const fn lambda_f32(&self) -> f32 {
        self.lambda
    }

    /// `source table format:`.
    #[must_use]
    pub const fn phonetic_type(&self) -> PhoneticType {
        self.phonetic
    }

    /// `database format:`.
    #[must_use]
    pub const fn database_format(&self) -> DatabaseFormat {
        self.database_format
    }

    /// Returns the default row at `index`, including an unused row for an unnamed slot. An
    /// index outside the library range yields [`None`].
    #[must_use]
    pub fn default_row(&self, index: u8) -> Option<&TableRow> {
        self.default.get(usize::from(index))
    }

    /// Returns the addon row at `index`, including an unused row for an unnamed slot. An
    /// index outside the library range yields [`None`].
    #[must_use]
    pub fn addon_row(&self, index: u8) -> Option<&TableRow> {
        self.addon.get(usize::from(index))
    }

    /// `SYSTEM_FILE` default rows, by sub-index: the libraries `pinyin_init`
    /// maps from the system dir.
    pub fn system_libraries(&self) -> impl Iterator<Item = (u8, &TableRow)> {
        self.rows_of(FileType::SystemFile)
    }

    /// `USER_FILE` default rows, by sub-index: the libraries that live in
    /// the user dir alone.
    pub(crate) fn user_libraries(&self) -> impl Iterator<Item = (u8, &TableRow)> {
        self.rows_of(FileType::UserFile)
    }

    /// What the init loop does at each default row, in index order
    /// (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`); a `NOT_USED` row does
    /// nothing and is left out. The pin reaches a row only after every row
    /// before it, so a caller walks these in order and stops at the first
    /// fault.
    ///
    /// `user_dir` decides whether a row without a user file is fatal: the
    /// pin reads the directory itself there (`memory_chunk.h:424-434`), which
    /// asserts only where the directory reads as a chunk.
    #[must_use]
    pub(crate) fn default_row_steps(
        &self,
        user_dir: Option<&Path>,
    ) -> Vec<(u8, DefaultRowStep<'_>)> {
        self.default_row_steps_with(user_dir.is_some_and(directory_reads_as_a_chunk))
    }

    fn default_row_steps_with(&self, has_user_dir: bool) -> Vec<(u8, DefaultRowStep<'_>)> {
        (0_u8..)
            .zip(&self.default)
            .filter_map(|(index, row)| {
                let null_user_file = row.user.is_none() && has_user_dir;
                let step = match row.file_type {
                    FileType::NotUsed => return None,
                    FileType::Dictionary => DefaultRowStep::Stop(TableConfSite::DefaultDictionary),
                    FileType::SystemFile => row.system.as_ref().map_or(
                        DefaultRowStep::Stop(TableConfSite::NullSystemFile),
                        |file| DefaultRowStep::System {
                            file,
                            null_user_file,
                        },
                    ),
                    FileType::UserFile if null_user_file => {
                        DefaultRowStep::Stop(TableConfSite::NullUserFile)
                    }
                    FileType::UserFile => DefaultRowStep::User,
                };
                Some((index, step))
            })
            .collect()
    }

    fn rows_of(&self, file_type: FileType) -> impl Iterator<Item = (u8, &TableRow)> {
        (0_u8..)
            .zip(&self.default)
            .filter(move |(_, row)| row.file_type == file_type)
    }

    /// The default rows by sub-index, whatever their type, in the order
    /// `_clean_user_files` walks them (`pinyin.cpp:152-167`,
    /// `zhuyin.cpp:106-121`).
    pub fn default_rows(&self) -> impl Iterator<Item = (u8, &TableRow)> {
        (0_u8..).zip(&self.default)
    }

    /// The addon libraries by sub-index: every `addon` row, whatever its
    /// type (the load call judges the type).
    pub fn addon_rows(&self) -> impl Iterator<Item = (u8, &TableRow)> {
        (0_u8..)
            .zip(&self.addon)
            .filter(|(_, row)| row.file_type != FileType::NotUsed)
    }
}

/// `to_index_of_default_tables` (`:145-157`): `TSI_DICTIONARY` and
/// `GB_DICTIONARY` are the same index (`novel_types.h:154-155`).
fn default_index(name: &[u8]) -> Option<usize> {
    Some(match name {
        b"RESERVED" => 0,
        b"GB_DICTIONARY" | b"TSI_DICTIONARY" => 1,
        b"GBK_DICTIONARY" => 2,
        b"OPENGRAM_DICTIONARY" => 3,
        b"MERGED_DICTIONARY" => 4,
        b"ADDON_DICTIONARY" => 5,
        b"NETWORK_DICTIONARY" => 6,
        b"USER_DICTIONARY" => 7,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
