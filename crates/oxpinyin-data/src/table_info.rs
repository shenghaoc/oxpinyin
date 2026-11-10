//! The system `table.conf`, read the way libpinyin's
//! `SystemTableInfo2::load` reads it (`src/storage/table_info.cpp:194-294`
//! at `074a2219`), and the library set its rows describe.
//!
//! The file is scanned with the pin's own `setlocale`, `fopen`, `fscanf`,
//! `feof` and `atoi` calls through [`oxpinyin_input::config`]: five header
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

use std::ffi::{CStr, OsStr, OsString};
use std::path::Path;

use oxpinyin_input::config::SystemTableInfoReader;

use crate::phrase_library::directory_reads_as_a_chunk;
use crate::table_conf::{Lambda, PINNED_LAMBDA};

/// `PHRASE_INDEX_LIBRARY_COUNT` (`novel_types.h:43`).
pub const PHRASE_INDEX_LIBRARY_COUNT: usize = 16;

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
pub enum DefaultRowStep<'a> {
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
/// directory by `oxpinyin_input::path::build_path` — never as `Path::join`
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
        Self(oxpinyin_input::path::path_from_bytes(bytes).into_os_string())
    }

    /// The name with `suffix` appended (`g_strdup_printf("%s.tmp", name)`).
    #[must_use]
    pub fn with_suffix(&self, suffix: &str) -> Self {
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
    lambda_unit: Option<Lambda>,
    phonetic: PhoneticType,
    database_format: DatabaseFormat,
    default: Vec<TableRow>,
    addon: Vec<TableRow>,
}

impl TableConf {
    /// The layout every libpinyin install ships (`data/table.conf.in`),
    /// with `database_format` the backend this build is compiled against.
    /// What a data directory without a `table.conf` stands for.
    #[must_use]
    pub fn stock() -> Self {
        let mut conf = Self {
            binary_format_version: 7,
            model_data_version: 14,
            lambda: PINNED_LAMBDA,
            lambda_unit: Some(Lambda::PINNED),
            phonetic: PhoneticType::Pinyin,
            database_format: DatabaseFormat::from_token(oxpinyin_store::DEFAULT_STORE_DB_FORMAT)
                .unwrap_or(DatabaseFormat::BerkeleyDb),
            default: vec![TableRow::unused(); PHRASE_INDEX_LIBRARY_COUNT],
            addon: vec![TableRow::unused(); PHRASE_INDEX_LIBRARY_COUNT],
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
        for &(index, stem) in crate::system_files::SYSTEM_LIBRARY_NAMES {
            system(&mut conf, usize::from(index), stem);
        }
        for (index, file) in [(5, "addon.bin"), (6, "network.bin"), (7, "user.bin")] {
            if let Some(slot) = conf.default.get_mut(index) {
                *slot = TableRow::named(b"NULL", b"NULL", file.as_bytes(), FileType::UserFile);
            }
        }
        for &(index, stem) in crate::system_files::ADDON_LIBRARY_NAMES {
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
    /// the pin leaves it (policy row 39).
    ///
    /// # Errors
    ///
    /// [`TableConfError`] where the pin returns `false` or dies.
    pub fn load(
        filename: &(impl oxpinyin_input::FileName + ?Sized),
    ) -> Result<Self, TableConfError> {
        let input =
            SystemTableInfoReader::open(filename).map_err(|_| TableConfError::Unopenable)?;
        Self::read(input)
    }

    fn read(mut input: SystemTableInfoReader) -> Result<Self, TableConfError> {
        // Each early return drops `input`: the pin's `fclose`, without its
        // closing `setlocale` (`:209-224`).
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
        // The two format directives' answers are not checked (`:228-233`):
        // each mapping reads whatever `str` holds by then.
        let _ = input.source_table_format();
        let phonetic = match input.str().map(CStr::to_bytes) {
            None => return Err(TableConfError::UninitialisedSourceFormat),
            Some(b"pinyin") => PhoneticType::Pinyin,
            Some(b"zhuyin") => PhoneticType::Zhuyin,
            Some(_) => return Err(TableConfError::Abort(TableConfSite::SourceFormat)),
        };
        let _ = input.database_format();
        let database_format = input
            .str()
            .and_then(|token| token.to_str().ok())
            .and_then(DatabaseFormat::from_token)
            .ok_or(TableConfError::Abort(TableConfSite::DatabaseFormat))?;

        let mut conf = Self {
            binary_format_version,
            model_data_version,
            lambda,
            lambda_unit: Lambda::from_f32(lambda),
            phonetic,
            database_format,
            default: vec![TableRow::unused(); PHRASE_INDEX_LIBRARY_COUNT],
            addon: vec![TableRow::unused(); PHRASE_INDEX_LIBRARY_COUNT],
        };

        // `:254-287`, each row's checks in the pin's order.
        while !input.eof() {
            let Some([target, index, table, system, user, file_type]) = input.table_row() else {
                continue;
            };
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
                usize::from(oxpinyin_input::config::atoi(index) as u8)
            };
            if index >= PHRASE_INDEX_LIBRARY_COUNT {
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
                conf.default.get_mut(index)
            } else {
                conf.addon.get_mut(index)
            };
            if let Some(slot) = slot {
                *slot = TableRow::named(
                    table.to_bytes(),
                    system.to_bytes(),
                    user.to_bytes(),
                    file_type,
                );
            }
        }
        input.finish();
        Ok(conf)
    }

    /// [`Self::load`] of `path`.
    ///
    /// # Errors
    ///
    /// As [`Self::load`]; a path holding a NUL is
    /// [`TableConfError::Unopenable`].
    pub fn load_path(path: &Path) -> Result<Self, TableConfError> {
        if oxpinyin_input::path::path_to_bytes(path).contains(&0) {
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
        Self::load_path(&oxpinyin_input::path::build_path(
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

    /// `lambda parameter:` as the exact rational the scanned `gfloat`'s
    /// shortest round-tripping decimal denotes when that is a plain decimal
    /// in `[0, 1]` — the decoder's integer path ([`Lambda::from_f32`]);
    /// `None` for anything else.
    #[must_use]
    pub const fn lambda_unit(&self) -> Option<Lambda> {
        self.lambda_unit
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

    /// The default row of sub-index `index` (`NOT_USED` when no row names it).
    #[must_use]
    pub fn default_row(&self, index: u8) -> Option<&TableRow> {
        self.default.get(usize::from(index))
    }

    /// The addon row of sub-index `index`.
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
    pub fn user_libraries(&self) -> impl Iterator<Item = (u8, &TableRow)> {
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
    pub fn default_row_steps(&self, user_dir: Option<&Path>) -> Vec<(u8, DefaultRowStep<'_>)> {
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
mod tests {
    use super::*;

    const STOCK: &str = "binary format version:7\nmodel data version:14\n\
        lambda parameter:0.312699\n\nsource table format:pinyin\n\
        database format:BerkeleyDB\n\n\
        default RESERVED NULL NULL NULL NOT_USED\n\
        default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n\
        default USER_DICTIONARY NULL NULL user.bin USER_FILE\n\n\
        addon 4 art.table art.bin NULL DICTIONARY\n";

    fn parse_bytes(bytes: &[u8]) -> Result<TableConf, TableConfError> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("table.conf");
        std::fs::write(&path, bytes).expect("write");
        TableConf::load_path(&path)
    }

    fn parse(text: &str) -> Result<TableConf, TableConfError> {
        parse_bytes(text.as_bytes())
    }

    // Pin memory_chunk.h:424-434: a null user filename reads the user directory.
    #[test]
    fn null_chunk_names_follow_directory_size_decision() {
        let user = parse(&STOCK.replace("gb_char.dbin SYSTEM_FILE", "NULL SYSTEM_FILE")).unwrap();
        use crate::phrase_library::directory_size_reads_as_a_chunk;
        let null_user_file = |conf: &TableConf, reads: bool| {
            conf.default_row_steps_with(reads)
                .into_iter()
                .any(|(index, step)| {
                    index == 1 && matches!(step, DefaultRowStep::System { null_user_file, .. } if null_user_file)
                })
        };
        for size in [Ok(7), Err(std::io::ErrorKind::Other.into())] {
            assert!(!null_user_file(
                &user,
                directory_size_reads_as_a_chunk(size)
            ));
        }
        assert!(null_user_file(
            &user,
            directory_size_reads_as_a_chunk(Ok(8))
        ));
        let user_file = parse(&STOCK.replace("user.bin USER_FILE", "NULL USER_FILE")).unwrap();
        let row_seven = |reads| {
            user_file
                .default_row_steps_with(reads)
                .into_iter()
                .find_map(|(index, step)| (index == 7).then_some(step))
        };
        assert_eq!(
            row_seven(directory_size_reads_as_a_chunk(Ok(7))),
            Some(DefaultRowStep::User)
        );
        assert_eq!(
            row_seven(directory_size_reads_as_a_chunk(Ok(8))),
            Some(DefaultRowStep::Stop(TableConfSite::NullUserFile))
        );
        let system =
            parse(&STOCK.replace("gb_char.bin gb_char.dbin", "NULL gb_char.dbin")).unwrap();
        for reads in [false, true] {
            assert_eq!(
                system
                    .default_row_steps_with(reads)
                    .first()
                    .map(|row| row.1),
                Some(DefaultRowStep::Stop(TableConfSite::NullSystemFile))
            );
        }
    }

    #[test]
    fn rows_fill_their_slots() {
        let conf = parse(STOCK).expect("parses");
        assert_eq!(conf.binary_format_version(), 7);
        assert_eq!(conf.model_data_version(), 14);
        assert_eq!(conf.lambda_unit(), Some(Lambda::PINNED));
        assert_eq!(conf.database_format(), DatabaseFormat::BerkeleyDb);
        let system: Vec<_> = conf.system_libraries().map(|(index, _)| index).collect();
        assert_eq!(system, [1]);
        let user: Vec<_> = conf.user_libraries().map(|(index, _)| index).collect();
        assert_eq!(user, [7]);
        assert_eq!(
            conf.addon_row(4)
                .and_then(|row| row.system.as_ref().map(|name| name.display().into_owned())),
            Some("art.bin".to_owned())
        );
        assert_eq!(conf.default_row(2), Some(&TableRow::unused()));
    }

    #[test]
    fn a_later_row_replaces_an_earlier_one_and_tsi_is_gb() {
        let text = format!("{STOCK}default TSI_DICTIONARY x.table x.bin x.dbin NOT_USED\n");
        let conf = parse(&text).expect("parses");
        assert_eq!(conf.system_libraries().count(), 0);
    }

    /// tests/storage/test_table_info.cpp:26-83 at 074a2219: each default
    /// and addon row by its index, file type and NULL-able names.
    #[test]
    fn addon_replacement_and_null_columns_keep_their_indexed_types() {
        let text = format!(
            "{STOCK}default GB_DICTIONARY NULL NULL NULL SYSTEM_FILE\naddon 15 old.table old.bin old.user SYSTEM_FILE\naddon 15 new.table new.bin NULL DICTIONARY\naddon 3 NULL NULL NULL USER_FILE\n"
        );
        let conf = parse(&text).unwrap();
        let system = conf.default_row(1).unwrap();
        assert_eq!(system.file_type, FileType::SystemFile);
        assert_eq!(
            (&system.table, &system.system, &system.user),
            (&None, &None, &None)
        );
        let addon = conf.addon_row(15).unwrap();
        assert_eq!(addon.file_type, FileType::Dictionary);
        assert_eq!(addon.table, Some(FileName::from_bytes(b"new.table")));
        assert_eq!(addon.system, Some(FileName::from_bytes(b"new.bin")));
        assert_eq!(addon.user, None);
        let user = conf.addon_row(3).unwrap();
        assert_eq!(user.file_type, FileType::UserFile);
        assert_eq!(user.user, None);
    }

    /// `to_index_of_addon_tables` is `atoi` cut to a `guint8`, and only the
    /// assert after it refuses (`table_info.cpp:159-161`, `:276`).
    #[test]
    fn addon_indexes_are_atoi_cut_to_a_byte() {
        for (word, slot) in [("256", 0), ("abc", 0), ("4x", 4), ("259", 3)] {
            let text = format!("{STOCK}addon {word} w.table w.bin NULL DICTIONARY\n");
            let conf = parse(&text).expect(word);
            assert_eq!(
                conf.addon_row(slot)
                    .and_then(|row| row.system.as_ref())
                    .map(|file| file.display().into_owned()),
                Some("w.bin".to_owned()),
                "{word}"
            );
        }
        for word in ["-1", "16", "99", "255"] {
            let text = format!("{STOCK}addon {word} w.table w.bin NULL DICTIONARY\n");
            assert_eq!(
                parse(&text).unwrap_err(),
                TableConfError::Abort(TableConfSite::AddonIndex),
                "{word}"
            );
        }
    }

    /// Fewer than six words before the end of input is a `continue`
    /// (`:259-260`); a comment is three ordinary words.
    #[test]
    fn short_trailing_rows_are_dropped_not_errors() {
        let text = format!("{STOCK}default GB_DICTIONARY a b c\n");
        assert!(parse(&text).is_ok());
        let text = format!("{STOCK}# a comment\n");
        assert!(parse(&text).is_ok());
    }

    /// The scanner groups words six at a time, wherever the lines break.
    #[test]
    fn rows_are_six_words_across_line_breaks() {
        let text =
            format!("{STOCK}default GBK_DICTIONARY gbk.table\ngbk.bin gbk.dbin\nSYSTEM_FILE\n");
        let conf = parse(&text).expect("parses");
        let system: Vec<_> = conf.system_libraries().map(|(index, _)| index).collect();
        assert_eq!(system, [1, 2]);
        // A short row takes the first word of the next line as its sixth.
        let text = format!(
            "{STOCK}default GBK_DICTIONARY a b c\ndefault RESERVED NULL NULL NULL NOT_USED\n"
        );
        assert_eq!(
            parse(&text).unwrap_err(),
            TableConfError::Abort(TableConfSite::FileType)
        );
    }

    /// `%255s` stores 255 bytes and leaves the rest for the next word.
    #[test]
    fn a_long_word_is_cut_into_two() {
        let name = "a".repeat(300);
        let text = format!("{STOCK}addon 3 art.table {name} NULL DICTIONARY\n");
        assert_eq!(
            parse(&text).unwrap_err(),
            TableConfError::Abort(TableConfSite::FileType)
        );
    }

    #[test]
    fn each_abort_site_is_named() {
        let cases = [
            (
                "source table format:pinyin",
                "source table format:foo",
                TableConfSite::SourceFormat,
            ),
            (
                "database format:BerkeleyDB",
                "database format:LMDB",
                TableConfSite::DatabaseFormat,
            ),
        ];
        for (from, to, site) in cases {
            let text = STOCK.replace(from, to);
            assert_eq!(parse(&text).unwrap_err(), TableConfError::Abort(site));
        }
        for (row, site) in [
            ("foo GB_DICTIONARY a b c SYSTEM_FILE", TableConfSite::Target),
            (
                "default FOO_DICTIONARY a b c SYSTEM_FILE",
                TableConfSite::DefaultIndex,
            ),
            (
                "default GB_DICTIONARY a b c FOO_FILE",
                TableConfSite::FileType,
            ),
        ] {
            let text = format!("{STOCK}{row}\n");
            assert_eq!(
                parse(&text).unwrap_err(),
                TableConfError::Abort(site),
                "{row}"
            );
        }
    }

    #[test]
    fn header_misses_are_the_ordinary_false_or_the_uninitialised_read() {
        let no_lambda = STOCK.replace("lambda parameter:0.312699\n", "");
        assert_eq!(parse(&no_lambda).unwrap_err(), TableConfError::Header);
        let no_source = STOCK.replace("source table format:pinyin\n", "");
        assert_eq!(
            parse(&no_source).unwrap_err(),
            TableConfError::UninitialisedSourceFormat
        );
        let no_database = STOCK.replace("database format:BerkeleyDB\n", "");
        assert_eq!(
            parse(&no_database).unwrap_err(),
            TableConfError::Abort(TableConfSite::DatabaseFormat)
        );
    }

    /// λ is the host scanner's `%f`; its rational routing value is the
    /// shortest decimal of that `gfloat` (maintainer ruling 2026-10-28 UTC,
    /// D1).
    #[test]
    fn lambda_is_the_scanned_float() {
        for (text, expected) in [
            ("0.5", 0.5_f32),
            ("2", 2.0),
            ("-0.5", -0.5),
            ("5e-1", 0.5),
            ("0x1p-1", 0.5),
            ("1e-30", 1e-30),
        ] {
            let conf = parse(&STOCK.replace("0.312699", text)).expect(text);
            assert_eq!(conf.lambda_f32(), expected, "{text}");
        }
        let unit = |text: &str| {
            parse(&STOCK.replace("0.312699", text))
                .expect(text)
                .lambda_unit()
        };
        // `table_conf::tests` holds the routing of each gfloat; this is the
        // connection from the scanned value.
        assert_eq!(unit("5e-1"), Lambda::from_decimal("0.5"));
        assert!(
            parse(&STOCK.replace("0.312699", "nan"))
                .expect("nan")
                .lambda_f32()
                .is_nan()
        );
    }

    /// Each header directive is one `fscanf`: whitespace directives cross
    /// line breaks, and a field stops where the conversion stops.
    #[test]
    fn headers_follow_the_scanner() {
        assert!(parse(&STOCK.replace('\n', " ")).is_ok());
        assert!(
            parse(&STOCK.replace("source table format:pinyin", "source table format:\npinyin"))
                .is_ok()
        );
        assert_eq!(
            parse(&STOCK.replace("version:7", "version:7x")).unwrap_err(),
            TableConfError::Header
        );
        assert_eq!(
            parse(&STOCK.replace("version:7", "version:x")).unwrap_err(),
            TableConfError::Header
        );
    }

    #[test]
    fn a_missing_file_is_unopenable() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            TableConf::load_from_dir(dir.path()).unwrap_err(),
            TableConfError::Unopenable
        );
    }

    // Unix only: byte-string file names.
    #[cfg(unix)]
    #[test]
    fn file_name_columns_keep_their_raw_bytes() {
        use std::os::unix::ffi::OsStrExt as _;
        // The word holds the byte 0xFF, which is not UTF-8.
        let mut bytes = STOCK.as_bytes().to_vec();
        let at = bytes
            .windows(12)
            .position(|window| window == b"gb_char.dbin")
            .expect("marker");
        bytes.splice(at..at + 12, *b"gb\xff.dbin");
        let conf = parse_bytes(&bytes).expect("parses");
        let user = conf
            .default_row(1)
            .and_then(|row| row.user.as_ref())
            .expect("user file");
        assert_eq!(AsRef::<OsStr>::as_ref(user).as_bytes(), b"gb\xff.dbin");
    }

    #[test]
    fn the_stock_layout_is_the_shipped_table_conf() {
        let stock = TableConf::stock();
        let libraries: Vec<_> = stock.system_libraries().map(|(index, _)| index).collect();
        assert_eq!(libraries, [1, 2, 3, 4]);
        let user: Vec<_> = stock.user_libraries().map(|(index, _)| index).collect();
        assert_eq!(user, [5, 6, 7]);
        assert_eq!(stock.addon_rows().count(), 12);
    }
}
