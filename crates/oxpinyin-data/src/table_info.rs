//! The system `table.conf`, read the way libpinyin's
//! `SystemTableInfo2::load` reads it (`src/storage/table_info.cpp:194-294`
//! at `074a2219`), and the library set its rows describe.
//!
//! The pin reads the file with `fscanf` over one stream: five header
//! directives in order, then rows of six white-space separated words.
//! This module is that sequence, not a line parser — [`Scan`] follows the C
//! library's rules for the directives the pin uses (a space in a directive
//! matches any run of white space, an empty one included; `%255s` skips
//! white space across newlines and takes at most 255 bytes; a directive that
//! does not match leaves the stream where it stopped):
//!
//! ```c
//! fscanf(input, "binary format version:%d\n", &binver);      // :208
//! fscanf(input, "model data version:%d\n", &modelver);       // :214
//! fscanf(input, "lambda parameter:%f\n", &lambda);           // :220
//! fscanf(input, "source table format:%255s\n", str);         // :228
//! fscanf(input, "database format:%255s\n", str);             // :232
//! while (!feof(input))
//!     fscanf(input, "%255s %255s %255s %255s %255s %255s\n", …);  // :255
//! ```
//!
//! # What the rows mean
//!
//! A `default` row names one of the sixteen default sub-indices by its
//! enum name (`RESERVED`, `GB_DICTIONARY`/`TSI_DICTIONARY` = 1,
//! `GBK_DICTIONARY` = 2, `OPENGRAM_DICTIONARY` = 3, `MERGED_DICTIONARY` = 4,
//! `ADDON_DICTIONARY` = 5, `NETWORK_DICTIONARY` = 6, `USER_DICTIONARY` = 7);
//! an `addon` row names its sub-index by number (`atoi`, truncated to a
//! `guint8`). The three file columns are the model source table, the
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
/// directory the way `g_build_filename` does — never as `Path::join` does.
///
/// A name that starts with a separator stays beneath the directory
/// (`g_build_filename("/u", "/tmp/x", NULL)` is `/u/tmp/x`); only an empty
/// directory leaves the name as it is. The type is not `AsRef<Path>` so that
/// no caller can join it by accident.
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

    /// `g_build_filename(dir, name, NULL)`.
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

/// `g_build_filename(dir, name, NULL)`: the leading separators of `name` are
/// dropped when `dir` is not empty, so a name never leaves its directory
/// by being absolute; an empty `name` is `dir`; an empty `dir` leaves `name`
/// as it is. (`..` stays, as in GLib.)
#[must_use]
pub fn pin_join(dir: &Path, name: &OsStr) -> PathBuf {
    if dir.as_os_str().is_empty() {
        return PathBuf::from(name);
    }
    #[cfg(unix)]
    let relative: OsString = {
        use std::os::unix::ffi::OsStrExt as _;
        let bytes = name.as_bytes();
        let skip = bytes.iter().take_while(|&&byte| byte == b'/').count();
        OsStr::from_bytes(bytes.get(skip..).unwrap_or(&[])).to_owned()
    };
    #[cfg(not(unix))]
    let relative: OsString = OsString::from(name.to_string_lossy().trim_start_matches(['/', '\\']));
    if relative.is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(relative)
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
    /// An `abort()` or `assert` of the pin.
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
        let mut scan = Scan::new(bytes);

        // :208-224 — three directives, each must convert one value.
        if !scan.literal(b"binary format version:") {
            return Err(TableConfError::Header);
        }
        let binary_format_version = scan.int().ok_or(TableConfError::Header)?;
        scan.skip_space();
        if !scan.literal(b"model data version:") {
            return Err(TableConfError::Header);
        }
        let model_data_version = scan.int().ok_or(TableConfError::Header)?;
        scan.skip_space();
        if !scan.literal(b"lambda parameter:") {
            return Err(TableConfError::Header);
        }
        let lambda_text = scan.float().ok_or(TableConfError::Header)?;
        scan.skip_space();
        let lambda = c_float(&lambda_text).ok_or(TableConfError::Header)?;
        let lambda_unit = Lambda::from_decimal(&lambda_text);

        // :228-229 — a directive that does not convert leaves `str`
        // unwritten, and `to_table_phonetic_type` reads it.
        let word = scan
            .literal(b"source table format:")
            .then(|| scan.word())
            .flatten()
            .ok_or(TableConfError::UninitialisedSourceFormat)?;
        scan.skip_space();
        let phonetic = match c_string(&word) {
            b"pinyin" => PhoneticType::Pinyin,
            b"zhuyin" => PhoneticType::Zhuyin,
            _ => return Err(TableConfError::Abort(TableConfSite::SourceFormat)),
        };

        // :232-233 — a directive that does not convert leaves the previous
        // word in `str`, which is neither a database format.
        let word = scan
            .literal(b"database format:")
            .then(|| scan.word())
            .flatten()
            .ok_or(TableConfError::Abort(TableConfSite::DatabaseFormat))?;
        scan.skip_space();
        let database_format = std::str::from_utf8(c_string(&word))
            .ok()
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

        // :254-287 — rows of six words; an incomplete last row is dropped.
        let mut words = Vec::new();
        while let Some(word) = scan.word() {
            words.push(word);
        }
        for row in words.chunks_exact(6) {
            let [target, index, table, system, user, file_type] = row else {
                continue;
            };
            let is_default = match c_string(target) {
                b"default" => true,
                b"addon" => false,
                _ => return Err(TableConfError::Abort(TableConfSite::Target)),
            };
            let index = if is_default {
                default_index(c_string(index))
                    .ok_or(TableConfError::Abort(TableConfSite::DefaultIndex))?
            } else {
                let index = atoi_guint8(c_string(index));
                if usize::from(index) >= PHRASE_INDEX_LIBRARY_COUNT {
                    return Err(TableConfError::Abort(TableConfSite::AddonIndex));
                }
                usize::from(index)
            };
            let file_type = match c_string(file_type) {
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
    /// question. The system directory is never that small.
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

/// Whether `MemoryChunk::load` on `dir` gets past `file_size < header` and
/// asserts on the `read` of a directory: `lseek(fd, 0, SEEK_END)` answers at
/// least the 8-byte chunk header (`memory_chunk.h:424-434`).
fn directory_reads_as_a_chunk(dir: &Path) -> bool {
    use std::io::Seek as _;
    std::fs::File::open(dir)
        .and_then(|mut file| file.seek(std::io::SeekFrom::End(0)))
        .is_ok_and(|size| size >= 8)
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

/// `strtol` over the digits after an optional sign: the value, saturating at
/// `LONG_MIN`/`LONG_MAX` (accumulated toward the sign, so `LONG_MIN` is
/// reachable), and how many digits it took.
fn strtol_digits(negative: bool, digits: &[u8]) -> (i64, usize) {
    let mut value = Some(0_i64);
    let mut taken = 0_usize;
    for &byte in digits.iter().take_while(|byte| byte.is_ascii_digit()) {
        let digit = i64::from(byte - b'0');
        value = value
            .and_then(|value| value.checked_mul(10))
            .and_then(|value| {
                if negative {
                    value.checked_sub(digit)
                } else {
                    value.checked_add(digit)
                }
            });
        taken += 1;
    }
    (
        value.unwrap_or(if negative { i64::MIN } else { i64::MAX }),
        taken,
    )
}

/// `atoi` returned through a `guint8` (`:159-161`): `strtol`'s saturating
/// value, cut to the low byte.
fn atoi_guint8(word: &[u8]) -> u8 {
    let mut rest = word;
    while let [first, tail @ ..] = rest {
        if is_space(*first) {
            rest = tail;
        } else {
            break;
        }
    }
    let negative = matches!(rest.first(), Some(b'-'));
    if matches!(rest.first(), Some(b'-' | b'+')) {
        rest = rest.get(1..).unwrap_or(&[]);
    }
    let (value, _) = strtol_digits(negative, rest);
    // `(int)` of a `long`, then `(guint8)` of that.
    (value & 0xFF) as u8
}

/// The `gfloat` `strtof` makes of the text [`Scan::float`] took.
fn c_float(text: &str) -> Option<f32> {
    let (negative, body) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..)?),
        Some(b'+') => (false, text.get(1..)?),
        _ => (false, text),
    };
    let lower = body.to_ascii_lowercase();
    let magnitude = if lower.starts_with("nan") {
        f32::NAN
    } else if lower.starts_with("inf") {
        f32::INFINITY
    } else if let Some(hex) = lower.strip_prefix("0x") {
        hex_float(hex)?
    } else {
        lower.parse::<f32>().ok()?
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// A hexadecimal float (`0x` already removed): `hexdigits[.hexdigits][p[+-]digits]`,
/// rounded to nearest even as `strtof` does.
fn hex_float(text: &str) -> Option<f32> {
    let (mantissa, exponent) = match text.split_once('p') {
        Some((mantissa, exponent)) => (mantissa, exponent),
        None => (text, "0"),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut accumulated: u64 = 0;
    let mut scale: i64 = 0;
    let mut sticky = false;
    for (digit, after_point) in whole
        .chars()
        .map(|digit| (digit, false))
        .chain(fraction.chars().map(|digit| (digit, true)))
    {
        let digit = u64::from(digit.to_digit(16)?);
        if accumulated >> 60 == 0 {
            accumulated = accumulated * 16 + digit;
            if after_point {
                scale -= 4;
            }
        } else {
            sticky |= digit != 0;
            if !after_point {
                scale += 4;
            }
        }
    }
    let (negative, digits) = match exponent.as_bytes().first() {
        Some(b'-') => (true, exponent.get(1..)?),
        Some(b'+') => (false, exponent.get(1..)?),
        _ => (false, exponent),
    };
    let (power, _) = strtol_digits(negative, digits.as_bytes());
    let power = power.clamp(-100_000, 100_000);
    Some(round_to_f32(accumulated, scale + power, sticky))
}

/// `mantissa × 2^exponent`, with `sticky` marking nonzero bits already
/// dropped below the mantissa, rounded to the nearest `f32` (ties to even,
/// subnormals and overflow included).
fn round_to_f32(mantissa: u64, exponent: i64, sticky: bool) -> f32 {
    if mantissa == 0 {
        return 0.0;
    }
    let leading = i64::from(mantissa.leading_zeros());
    let mantissa = mantissa << leading;
    // The value lies in [2^top, 2^(top+1)).
    let top = exponent - leading + 63;
    if top > 127 {
        return f32::INFINITY;
    }
    let keep = if top >= -126 { 24 } else { 24 - (-126 - top) };
    if keep < 0 {
        return 0.0;
    }
    let shift = 64 - keep;
    let (mut kept, remainder) = if shift >= 64 {
        (0_u64, mantissa)
    } else {
        (mantissa >> shift, mantissa & ((1_u64 << shift) - 1))
    };
    let half = 1_u64 << (shift - 1);
    if remainder > half || (remainder == half && (sticky || kept & 1 == 1)) {
        kept += 1;
    }
    let scale = top - keep + 1;
    (kept as f64 * 2_f64.powi(i32::try_from(scale).unwrap_or(i32::MIN))) as f32
}

const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The C library's view of an input stream for the directives above.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Scan<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(is_space) {
            self.at += 1;
        }
    }

    /// A directive's literal text: a white-space byte matches any run of
    /// white space, any other byte only itself; the first mismatch stops.
    fn literal(&mut self, text: &[u8]) -> bool {
        for &byte in text {
            if is_space(byte) {
                self.skip_space();
            } else if self.peek() == Some(byte) {
                self.at += 1;
            } else {
                return false;
            }
        }
        true
    }

    /// `%d`: white space, an optional sign, at least one digit; `strtol`
    /// saturates at `long`, and the store into an `int` keeps the low 32 bits.
    fn int(&mut self) -> Option<i32> {
        self.skip_space();
        let negative = self.peek() == Some(b'-');
        let sign = usize::from(matches!(self.peek(), Some(b'-' | b'+')));
        let digits = self.bytes.get(self.at + sign..).unwrap_or(&[]);
        let (value, taken) = strtol_digits(negative, digits);
        if taken == 0 {
            return None;
        }
        self.at += sign + taken;
        Some((value & 0xFFFF_FFFF) as u32 as i32)
    }

    /// `%f`: white space, then what glibc's scanner takes for `strtof`: a
    /// sign; `inf` or `infinity`; `nan` with an optional parenthesised
    /// `[0-9A-Za-z_]*`; a decimal with an optional exponent; or a hexadecimal
    /// float `0x` hex digits, an optional `.`, an optional `p` exponent. Unlike
    /// `strtod`, the scanner does not back up: an `e` or `p` with no digits
    /// after it, a `0x` with no hex digit, or an `infi…` that is not
    /// `infinity` fails the conversion. Returns the text taken.
    fn float(&mut self) -> Option<String> {
        self.skip_space();
        let start = self.at;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let rest = self.bytes.get(self.at..).unwrap_or(&[]);
        let lower: Vec<u8> = rest.iter().take(8).map(u8::to_ascii_lowercase).collect();
        if lower.starts_with(b"inf") {
            if lower.get(3).is_some_and(|byte| *byte == b'i') {
                if !lower.starts_with(b"infinity") {
                    return None;
                }
                self.at += 8;
            } else {
                self.at += 3;
            }
        } else if lower.starts_with(b"nan") {
            self.at += 3;
            if self.peek() == Some(b'(') {
                self.at += 1;
                while self
                    .peek()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    self.at += 1;
                }
                if self.peek() != Some(b')') {
                    return None;
                }
                self.at += 1;
            }
        } else if lower.starts_with(b"0x") {
            self.at += 2;
            let mut digits = 0;
            while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                self.at += 1;
                digits += 1;
            }
            if self.peek() == Some(b'.') {
                self.at += 1;
                while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                    self.at += 1;
                    digits += 1;
                }
            }
            if digits == 0 {
                return None;
            }
            self.exponent(b'p')?;
        } else {
            let mut digits = 0;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
                digits += 1;
            }
            if self.peek() == Some(b'.') {
                self.at += 1;
                while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.at += 1;
                    digits += 1;
                }
            }
            if digits == 0 {
                return None;
            }
            self.exponent(b'e')?;
        }
        let text = self.bytes.get(start..self.at)?;
        Some(String::from_utf8_lossy(text).into_owned())
    }

    /// An optional exponent introduced by `marker` (either case): once the
    /// marker is read the scanner needs a digit after the optional sign.
    fn exponent(&mut self, marker: u8) -> Option<()> {
        if self.peek().map(|byte| byte.to_ascii_lowercase()) != Some(marker) {
            return Some(());
        }
        self.at += 1;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let digits = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        (self.at > digits).then_some(())
    }

    /// `%255s`: skips white space (newlines included) and takes up to 255
    /// bytes; `None` at the end of the file.
    fn word(&mut self) -> Option<Vec<u8>> {
        self.skip_space();
        let start = self.at;
        while self.at - start < 255 && self.peek().is_some_and(|byte| !is_space(byte)) {
            self.at += 1;
        }
        (self.at > start)
            .then(|| self.bytes.get(start..self.at).map(<[u8]>::to_vec))
            .flatten()
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
    fn addon_indexes_wrap_through_a_guint8() {
        // `atoi("256")` is 256, the `guint8` is 0; `atoi("abc")` is 0.
        for (word, expected) in [("256", 0), ("abc", 0), ("4", 4), ("-1", 255), ("16", 16)] {
            assert_eq!(atoi_guint8(word.as_bytes()), expected, "{word}");
        }
        let text = format!("{STOCK}addon 256 w.table w.bin NULL DICTIONARY\n");
        let conf = parse(&text).expect("256 wraps to a valid slot");
        assert_eq!(
            conf.addon_row(0)
                .and_then(|row| row.system.as_ref().map(|name| name.display().into_owned())),
            Some("w.bin".to_owned())
        );
    }

    #[test]
    fn short_and_trailing_words_are_dropped_not_errors() {
        let text = format!("{STOCK}default GB_DICTIONARY a b c\n");
        assert!(parse(&text).is_ok());
        let text = format!("{STOCK}# comment\n");
        assert!(parse(&text).is_ok());
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
        let conf = parse(&STOCK.replace("0.312699", "nan")).expect("parses");
        assert!(conf.lambda_f32().is_nan());
    }

    /// What glibc's `sscanf("%f%n")` answered for each spelling, measured on
    /// the pin's platform (glibc 2.43): the value and the bytes taken, or a
    /// failed conversion.
    #[test]
    fn float_spellings_read_like_glibc() {
        let cases: &[(&str, Option<(f32, usize)>)] = &[
            ("0x1p-1 Z", Some((0.5, 6))),
            ("0x.8 Z", Some((0.5, 4))),
            ("0x Z", None),
            ("0x1p Z", None),
            ("0xg Z", None),
            ("0x.p1 Z", None),
            ("0x1.8p+1z Z", Some((3.0, 8))),
            ("0X1P1 Z", Some((2.0, 5))),
            ("0x1.p0 Z", Some((1.0, 6))),
            ("  +0x10 Z", Some((16.0, 7))),
            ("0x1e Z", Some((30.0, 4))),
            ("inf Z", Some((f32::INFINITY, 3))),
            ("infinity Z", Some((f32::INFINITY, 8))),
            ("infin Z", None),
            ("1e Z", None),
            ("1e+ Z", None),
            (".e1 Z", None),
            ("+.5 Z", Some((0.5, 3))),
            ("1_0 Z", Some((1.0, 1))),
            ("1.5.5 Z", Some((1.5, 3))),
            ("1e5000 Z", Some((f32::INFINITY, 6))),
            ("- Z", None),
        ];
        for &(input, expected) in cases {
            let mut scan = Scan::new(input.as_bytes());
            let got = scan
                .float()
                .and_then(|text| c_float(&text))
                .map(|value| (value, scan.at));
            assert_eq!(got, expected, "{input:?}");
        }
        for (input, taken) in [
            ("nan(1) Z", 6),
            ("nan(1_a) Z", 8),
            ("nan() Z", 5),
            ("-nan Z", 4),
            ("nanx Z", 3),
        ] {
            let mut scan = Scan::new(input.as_bytes());
            let value = scan.float().and_then(|text| c_float(&text));
            assert!(
                value.is_some_and(f32::is_nan) && scan.at == taken,
                "{input:?}"
            );
        }
        for input in ["nan( Z", "nan(1 Z"] {
            assert!(Scan::new(input.as_bytes()).float().is_none(), "{input:?}");
        }
    }

    #[test]
    fn hex_floats_round_like_strtof() {
        // 24 bits kept; a tie goes to even, a nonzero tail breaks it upward.
        let bits = |text: &str| hex_float(text).map(f32::to_bits);
        assert_eq!(bits("1.000001p0"), Some(0x3f80_0000));
        assert_eq!(bits("1.000002p0"), Some(0x3f80_0001));
        assert_eq!(bits("1.000003p0"), Some(0x3f80_0002));
        assert_eq!(bits("1.0000010000000001p0"), Some(0x3f80_0001));
        // The smallest subnormal, half of it (a tie to even), and overflow.
        assert_eq!(hex_float("1p-149"), Some(f32::from_bits(1)));
        assert_eq!(hex_float("1p-150"), Some(0.0));
        assert_eq!(hex_float("1.8p-150"), Some(f32::from_bits(1)));
        assert_eq!(hex_float("1p128"), Some(f32::INFINITY));
        assert_eq!(hex_float("1.ffffffp127"), Some(f32::INFINITY));
        assert_eq!(hex_float("0p0"), Some(0.0));
    }

    #[test]
    fn integers_saturate_at_long_min_like_strtol() {
        assert_eq!(strtol_digits(true, b"9223372036854775808"), (i64::MIN, 19));
        assert_eq!(strtol_digits(true, b"99999999999999999999"), (i64::MIN, 20));
        assert_eq!(strtol_digits(false, b"9223372036854775808"), (i64::MAX, 19));
        assert_eq!(atoi_guint8(b"-99999999999999999999"), 0);
        assert_eq!(atoi_guint8(b"-256"), 0);
        assert_eq!(atoi_guint8(b"-1"), 255);
        let mut scan = Scan::new(b"-9223372036854775808x");
        assert_eq!(scan.int(), Some(0));
        assert_eq!(scan.at, 20);
    }

    #[test]
    fn file_names_stay_beneath_their_directory() {
        let under = |dir: &str, name: &[u8]| FileName::from_bytes(name).under(Path::new(dir));
        assert_eq!(under("/u", b"/tmp/x"), Path::new("/u/tmp/x"));
        assert_eq!(under("/u", b"//x//y/"), Path::new("/u/x//y/"));
        assert_eq!(under("/u", b"../x"), Path::new("/u/../x"));
        assert_eq!(under("/u", b""), Path::new("/u"));
        assert_eq!(under("", b"/tmp/x"), Path::new("/tmp/x"));
        assert_eq!(under("a/", b"x"), Path::new("a/x"));
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
