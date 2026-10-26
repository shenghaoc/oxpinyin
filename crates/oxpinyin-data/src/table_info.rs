//! The system `table.conf`, read the way libpinyin's
//! `SystemTableInfo2::load` reads it (`src/storage/table_info.cpp:194-294`
//! at `074a2219`), and the library set its rows describe.
//!
//! Five line headers declare versions, decimal λ, source and database format,
//! followed by six-word library rows. Parsing uses Rust's decimal conversions;
//! libc scanner extensions are recorded in the #1297 scanner discrepancy.
//!
//! # What the rows mean
//!
//! A `default` row names one of the sixteen default sub-indices by its
//! enum name (`RESERVED`, `GB_DICTIONARY`/`TSI_DICTIONARY` = 1,
//! `GBK_DICTIONARY` = 2, `OPENGRAM_DICTIONARY` = 3, `MERGED_DICTIONARY` = 4,
//! `ADDON_DICTIONARY` = 5, `NETWORK_DICTIONARY` = 6, `USER_DICTIONARY` = 7);
//! an `addon` row names its sub-index by a decimal number in range. The three file columns are the model source table, the
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

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

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
/// directory by [`pin_join`] — never as `Path::join` does.
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
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            Self(OsStr::from_bytes(bytes).to_owned())
        }
        #[cfg(not(unix))]
        {
            Self(OsString::from(String::from_utf8_lossy(bytes).into_owned()))
        }
    }

    /// The name beneath `dir`, as [`pin_join`] spells it.
    #[must_use]
    pub fn under(&self, dir: &Path) -> PathBuf {
        pin_join(dir, &self.0)
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

/// The path `dir` and `name` make together, byte for byte as the pin spells
/// it (`g_build_filename(dir, name, NULL)`, the GLib the reference image
/// builds against): `name` stays beneath `dir` however it begins.
///
/// - an empty `name` is `dir`, and an empty `dir` leaves `name` as it is;
/// - otherwise `dir` keeps its leading separators and loses its trailing
///   ones, `name` loses its leading ones and keeps its trailing ones, and a
///   single separator stands between the two bodies;
/// - a part made only of separators has no body: `dir` alone is kept whole,
///   `name` alone is appended whole.
///
/// `..`, `.` and separators inside either part are not touched, and neither
/// are bytes that are not UTF-8. On Unix the only separator is `/`; the
/// other platforms accept both `/` and `\`, which no pin run covers.
#[must_use]
pub fn pin_join(dir: &Path, name: &OsStr) -> PathBuf {
    let joined = join_bytes(dir.as_os_str().as_encoded_bytes(), name.as_encoded_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        PathBuf::from(OsString::from_vec(joined))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(&joined).into_owned())
    }
}

/// [`pin_join`] on the parts' bytes.
pub(crate) fn join_bytes(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if dir.is_empty() || name.is_empty() {
        return [dir, name].concat();
    }
    let is_separator = |byte: &u8| crate::pin_stderr::is_separator(*byte);
    let leading = |part: &[u8]| part.iter().take_while(|byte| is_separator(byte)).count();
    let trailing = |part: &[u8]| {
        part.iter()
            .rev()
            .take_while(|byte| is_separator(byte))
            .count()
    };
    let dir_start = leading(dir);
    let dir_end = dir_start.max(dir.len() - trailing(dir));
    let name_start = leading(name);
    let name_end = name_start.max(name.len() - trailing(name));
    let mut joined = Vec::with_capacity(dir.len() + name.len() + 1);
    joined.extend_from_slice(&dir[..dir_end]);
    if dir_end > dir_start && name_end > name_start {
        joined.push(std::path::MAIN_SEPARATOR as u8);
    }
    joined.extend_from_slice(&name[name_start..name_end]);
    // The separators after the body; a name with no body is all of them.
    joined.extend_from_slice(&name[if name_end > name_start { name_end } else { 0 }..]);
    joined
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
            Self::Header => None,
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

    /// Reads `path` like `SystemTableInfo2::load`'s `fopen`; `None` when it
    /// cannot be opened as a regular file.
    #[must_use]
    pub fn read(path: &Path) -> Option<Result<Self, TableConfError>> {
        // A FIFO would block `fopen`-style reads forever.
        if !path.is_file() {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        Some(Self::parse(&bytes))
    }

    /// `SystemTableInfo2::load` over the file's bytes.
    ///
    /// # Errors
    ///
    /// [`TableConfError`] where the pin returns `false` or dies.
    pub fn parse(bytes: &[u8]) -> Result<Self, TableConfError> {
        let mut lines = ConfigLines::new(bytes);
        let binary_format_version = lines
            .field(b"binary format version:")
            .and_then(parse_decimal::<i32>)
            .ok_or(TableConfError::Header)?;
        let model_data_version = lines
            .field(b"model data version:")
            .and_then(parse_decimal::<i32>)
            .ok_or(TableConfError::Header)?;
        let lambda_text = lines
            .field(b"lambda parameter:")
            .and_then(|text| std::str::from_utf8(text).ok())
            .ok_or(TableConfError::Header)?;
        // Rust also accepts NaN and infinity; those are outside the decimal
        // grammar. Decimal overflow and subnormal rounding stay f32's.
        let magnitude = lambda_text.trim_start_matches(['+', '-']);
        if !magnitude
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            return Err(TableConfError::Header);
        }
        let lambda = lambda_text
            .parse::<f32>()
            .map_err(|_| TableConfError::Header)?;
        let lambda_unit = Lambda::from_decimal(lambda_text);
        let phonetic = match lines
            .field(b"source table format:")
            .ok_or(TableConfError::UninitialisedSourceFormat)?
        {
            b"pinyin" => PhoneticType::Pinyin,
            b"zhuyin" => PhoneticType::Zhuyin,
            _ => return Err(TableConfError::Abort(TableConfSite::SourceFormat)),
        };
        let database_format = lines
            .field(b"database format:")
            .and_then(|word| std::str::from_utf8(word).ok())
            .and_then(DatabaseFormat::from_token)
            .ok_or(TableConfError::Abort(TableConfSite::DatabaseFormat))?;

        let mut conf = Self {
            binary_format_version,
            model_data_version,
            lambda,
            lambda_unit,
            phonetic,
            database_format,
            default: vec![TableRow::unused(); PHRASE_INDEX_LIBRARY_COUNT],
            addon: vec![TableRow::unused(); PHRASE_INDEX_LIBRARY_COUNT],
        };

        // Each row owns its six fields; malformed short rows do not consume
        // words from a following row (table_info.cpp:254-260).
        for line in lines {
            if line.starts_with(b"#") {
                continue;
            }
            let row: Vec<&[u8]> = line
                .split(u8::is_ascii_whitespace)
                .filter(|word| !word.is_empty())
                .collect();
            let [target, index, table, system, user, file_type] = row.as_slice() else {
                continue;
            };
            let is_default = match *target {
                b"default" => true,
                b"addon" => false,
                _ => return Err(TableConfError::Abort(TableConfSite::Target)),
            };
            let index = if is_default {
                default_index(index).ok_or(TableConfError::Abort(TableConfSite::DefaultIndex))?
            } else {
                parse_decimal::<usize>(index)
                    .filter(|&index| index < PHRASE_INDEX_LIBRARY_COUNT)
                    .ok_or(TableConfError::Abort(TableConfSite::AddonIndex))?
            };
            let file_type = match *file_type {
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
                *slot =
                    TableRow::named(c_string(table), c_string(system), c_string(user), file_type);
            }
        }
        Ok(conf)
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

    /// `lambda parameter:` as the exact rational the decimal denotes when
    /// the value is a plain decimal in `[0, 1]` — the decoder's integer
    /// path; `None` for anything else the `%f` conversion accepts.
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

    /// The lowest default sub-index whose row is `DICTIONARY`, where the
    /// init loop asserts (`pinyin.cpp:388`, `zhuyin.cpp:330`).
    #[must_use]
    pub fn first_default_dictionary(&self) -> Option<u8> {
        self.rows_of(FileType::Dictionary)
            .next()
            .map(|(index, _)| index)
    }

    /// The init loop over the default rows in index order
    /// (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`): the first row the pin
    /// dies on, if any. A `DICTIONARY` row asserts; a library the pin
    /// would read through a name it does not have asserts inside
    /// `MemoryChunk`. `user_dir` is the context's user dir, if any (a
    /// `SYSTEM_FILE` log or a `USER_FILE` chunk is read from it).
    ///
    /// A row without a user file name makes the pin read the user directory
    /// itself as a chunk (`g_build_filename` stops at the `NULL`);
    /// `MemoryChunk::load` asserts on the `read` of a directory only when
    /// `lseek(SEEK_END)` gave it a size of at least the 8-byte header
    /// (`memory_chunk.h:424-434`), which depends on the file system and on
    /// what the directory holds: this asks the file system the same
    /// question. A row without a system chunk is [`TableConfSite::NullSystemFile`]
    /// on any file system: the pin dies on that row either way, of the
    /// assert or of the empty chunk it goes on with (the facade writes the
    /// pin's `mmap %s failed!` line for the second).
    ///
    /// # Errors
    ///
    /// The [`TableConfSite`] of the first fatal row.
    pub fn check_default_rows(&self, user_dir: Option<&Path>) -> Result<(), TableConfSite> {
        let has_user_dir = user_dir.is_some_and(directory_reads_as_a_chunk);
        for row in &self.default {
            match row.file_type {
                FileType::NotUsed => {}
                FileType::Dictionary => return Err(TableConfSite::DefaultDictionary),
                FileType::SystemFile => {
                    if row.system.is_none() {
                        return Err(TableConfSite::NullSystemFile);
                    }
                    if row.user.is_none() && has_user_dir {
                        return Err(TableConfSite::NullUserFile);
                    }
                }
                FileType::UserFile => {
                    if row.user.is_none() && has_user_dir {
                        return Err(TableConfSite::NullUserFile);
                    }
                }
            }
        }
        Ok(())
    }

    fn rows_of(&self, file_type: FileType) -> impl Iterator<Item = (u8, &TableRow)> {
        (0_u8..)
            .zip(&self.default)
            .filter(move |(_, row)| row.file_type == file_type)
    }

    /// The addon libraries by sub-index: every `addon` row, whatever its
    /// type (the load call judges the type).
    pub fn addon_rows(&self) -> impl Iterator<Item = (u8, &TableRow)> {
        (0_u8..)
            .zip(&self.addon)
            .filter(|(_, row)| row.file_type != FileType::NotUsed)
    }
}

/// The bytes of a word up to its first NUL: what the C string sees.
fn c_string(word: &[u8]) -> &[u8] {
    word.split(|&byte| byte == 0).next().unwrap_or(&[])
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

/// Decimal fields are complete tokens with a representable value.
pub(crate) fn parse_decimal<T: std::str::FromStr>(text: &[u8]) -> Option<T> {
    std::str::from_utf8(text).ok()?.parse().ok()
}

/// Nonempty configuration lines, shared by the system and user markers.
pub(crate) struct ConfigLines<'a> {
    lines: std::slice::Split<'a, u8, fn(&u8) -> bool>,
}

impl<'a> ConfigLines<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            lines: bytes.split(|byte| *byte == b'\n'),
        }
    }

    pub(crate) fn field(&mut self, name: &[u8]) -> Option<&'a [u8]> {
        self.next()?.strip_prefix(name).map(<[u8]>::trim_ascii)
    }
}

impl<'a> Iterator for ConfigLines<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.lines
            .by_ref()
            .map(<[u8]>::trim_ascii)
            .find(|line| !line.is_empty())
    }
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

    fn parse(text: &str) -> Result<TableConf, TableConfError> {
        TableConf::parse(text.as_bytes())
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

    #[test]
    fn addon_indexes_require_decimal_slots() {
        for word in ["256", "abc", "-1", "16", "4x", "99999999999999999999"] {
            let text = format!("{STOCK}addon {word} w.table w.bin NULL DICTIONARY\n");
            assert_eq!(
                parse(&text).unwrap_err(),
                TableConfError::Abort(TableConfSite::AddonIndex)
            );
        }
    }

    #[test]
    fn short_and_trailing_words_are_dropped_not_errors() {
        let text = format!("{STOCK}default GB_DICTIONARY a b c\n");
        assert!(parse(&text).is_ok());
        let text = format!("{STOCK}# comment\n");
        assert!(parse(&text).is_ok());
    }

    #[test]
    fn a_long_filename_stays_one_field() {
        let name = "a".repeat(300);
        let text = format!("{STOCK}addon 3 art.table {name} NULL DICTIONARY\n");
        let conf = parse(&text).expect("complete row");
        assert_eq!(
            conf.addon_row(3)
                .and_then(|row| row.system.as_ref())
                .map(|file| file.display().into_owned()),
            Some(name)
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
            (
                "addon 99 x.table x.bin NULL DICTIONARY",
                TableConfSite::AddonIndex,
            ),
            (
                "addon -1 x.table x.bin NULL DICTIONARY",
                TableConfSite::AddonIndex,
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

    #[test]
    fn lambda_is_read_as_a_float_as_written() {
        for (text, expected) in [("0.5", 0.5_f32), ("2", 2.0), ("-0.5", -0.5), ("5e-1", 0.5)] {
            let conf = parse(&STOCK.replace("0.312699", text)).expect("parses");
            assert_eq!(conf.lambda_f32(), expected, "{text}");
        }
        let conf = parse(&STOCK.replace("0.312699", "2")).expect("parses");
        assert_eq!(conf.lambda_unit(), None);
    }

    #[test]
    fn headers_require_complete_decimal_values_on_their_line() {
        for value in [
            "nan", "nan(1)", "inf", "0x1p-1", "1e", "1e+", "1_0", "1.5.5",
        ] {
            assert!(parse(&STOCK.replace("0.312699", value)).is_err(), "{value}");
        }
        for value in ["2147483648", "7x", "999999999999999999999"] {
            assert!(parse(&STOCK.replace("version:7", &format!("version:{value}"))).is_err());
        }
        assert!(
            parse(&STOCK.replace("source table format:pinyin", "source table format:\npinyin"))
                .is_err()
        );
        assert!(parse(&STOCK.replace('\n', " ")).is_err());
    }

    /// `g_build_filename(dir, name, NULL)` at GLib 2.89.4 (`gfileutils.c`,
    /// `g_build_path_va`), worked by hand for each partition.
    #[test]
    #[cfg(unix)]
    fn file_names_stay_beneath_their_directory() {
        // Compared as `OsString`, which is byte for byte: `Path::eq` compares
        // components and would hide every repeated or trailing separator.
        let under = |dir: &str, name: &[u8]| {
            FileName::from_bytes(name)
                .under(Path::new(dir))
                .into_os_string()
        };
        // A name never leaves its directory by starting with a separator.
        assert_eq!(under("/u", b"/tmp/x"), "/u/tmp/x");
        assert_eq!(under("/u", b"//x//y/"), "/u/x//y/");
        assert_eq!(under("/u", b"../x"), "/u/../x");
        // Empty parts.
        assert_eq!(under("/u", b""), "/u");
        assert_eq!(under("", b"/tmp/x"), "/tmp/x");
        assert_eq!(under("", b""), "");
        // The directory loses every trailing separator, keeps the leading ones.
        assert_eq!(under("a/", b"x"), "a/x");
        assert_eq!(under("a///", b"x"), "a/x");
        assert_eq!(under("//a", b"x"), "//a/x");
        assert_eq!(under("/", b"x"), "/x");
        assert_eq!(under("//", b"x"), "//x");
        // A name made only of separators is appended whole, after the
        // directory without its trailing separators.
        assert_eq!(under("/a", b"//"), "/a//");
        assert_eq!(under("/a//", b"/"), "/a/");
        assert_eq!(under("/", b"/"), "//");
        // Dots and the separator-like bytes of other platforms are ordinary.
        assert_eq!(under("/u", b"./x"), "/u/./x");
        assert_eq!(under("/u\\", b"\\x"), "/u\\/\\x");
    }

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
        let conf = TableConf::parse(&bytes).expect("parses");
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
        assert_eq!(stock.first_default_dictionary(), None);
    }
}
