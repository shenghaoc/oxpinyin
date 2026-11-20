//! User profile filenames and codecs. All three database backends use `user_bigram.db`
//! for the hash database and `user_pinyin_index.bin` and `user_phrase_index.bin` for the
//! tree indexes. The configured library rows name the individual chunks and change logs.
//! [`UserTableInfo`] reads and writes the text marker `user.conf`.
//!
//! In the stock configuration, `user.bin`, `addon.bin` and `network.bin` contain whole
//! user libraries. `gb_char.dbin`, `gbk_char.dbin`, `opengram.dbin` and `merged.dbin`
//! contain changes to system libraries. The profile marker is plain text.

// _write_files and _rename_files in pinyin.cpp; fixed names in pinyin_internal.h:55–66
// and stock rows in data/table.conf.in:8–28.

#[cfg(test)]
use crate::data::chunk_format::CHUNK_HEADER_SIZE;
use crate::data::chunk_format::chunk_checksum;
use std::ffi::CString;

use crate::data::table_info::FileName;
use crate::input::config::{
    Scanned, UserTableInfoFields, UserTableInfoReader, save_user_table_info,
};
#[cfg(any(test, feature = "test-hooks"))]
use crate::store::DEFAULT_STORE_DB_FORMAT;

/// A `MemoryChunk` file that does not frame a valid payload.
#[derive(Debug)]
pub enum ChunkReadError {
    /// Fewer bytes than the 8-byte header.
    ShortHeader,
    /// The declared length does not match the narrowed physical payload length.
    TruncatedPayload,
    /// The stored checksum does not match the payload.
    ChecksumMismatch,
}

impl std::fmt::Display for ChunkReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ShortHeader => "shorter than the 8-byte header",
            Self::TruncatedPayload => "payload length does not match the file size",
            Self::ChecksumMismatch => "checksum mismatch",
        };
        write!(f, "memory chunk: {message}")
    }
}

impl std::error::Error for ChunkReadError {}

/// Strips and verifies a `MemoryChunk` file frame, returning the payload
/// — the check `MemoryChunk::load` performs before any consumer sees
/// bytes.
///
/// The `.dbin` diff logs frame their logger record stream this
/// way (`log->save` writes a plain `MemoryChunk`).
///
/// # Errors
///
/// Fails on a short header, a length mismatch, or a checksum mismatch.
/// The physical payload size is narrowed to `u32` before the comparison,
/// as in `MemoryChunk::load` and `::mmap`; a match exposes only the
/// declared extent, even when the file is larger by a multiple of 2^32.
#[cfg(test)]
pub(crate) fn read_chunk_payload(bytes: &[u8]) -> Result<&[u8], ChunkReadError> {
    let header = bytes
        .get(..CHUNK_HEADER_SIZE)
        .ok_or(ChunkReadError::ShortHeader)?;
    let length = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let checksum = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if length as u32 != (bytes.len() - CHUNK_HEADER_SIZE) as u32 {
        return Err(ChunkReadError::TruncatedPayload);
    }
    let end = CHUNK_HEADER_SIZE
        .checked_add(length)
        .ok_or(ChunkReadError::TruncatedPayload)?;
    let payload = bytes
        .get(CHUNK_HEADER_SIZE..end)
        .ok_or(ChunkReadError::TruncatedPayload)?;
    if chunk_checksum(payload) != checksum {
        return Err(ChunkReadError::ChecksumMismatch);
    }
    Ok(payload)
}

/// Loads and validates a chunk through the pin's single-read boundary.
/// A short payload is checked at the length actually read. Framing uses
/// the physical end-seek length before the single payload read.
///
/// # Errors
/// Returns an I/O error or an invalid-data error on a failed chunk load.
pub(crate) fn load_chunk_payload(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    match try_load_chunk_payload(path) {
        Ok(bytes) => Ok(bytes),
        Err(ChunkPayloadError::Load(error)) => Err(error),
        Err(ChunkPayloadError::HeaderRead) => std::process::abort(),
    }
}

#[derive(Debug)]
pub(crate) enum ChunkPayloadError {
    Load(std::io::Error),
    HeaderRead,
}

/// The mask walk must replay earlier libraries before acting on a failed
/// header read (`pinyin.cpp:1281-1285`, `memory_chunk.h:434-438`).
pub(crate) fn try_load_chunk_payload(path: &std::path::Path) -> Result<Vec<u8>, ChunkPayloadError> {
    use crate::input::chunk::{ChunkLoadError, load};
    let invalid = |reason| {
        ChunkPayloadError::Load(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
    };
    let chunk = load(path).map_err(|error| match error {
        ChunkLoadError::Io(error) => ChunkPayloadError::Load(error),
        ChunkLoadError::ShortHeader => invalid(ChunkReadError::ShortHeader),
        ChunkLoadError::Header => ChunkPayloadError::HeaderRead,
    })?;
    chunk
        .finish(chunk_checksum)
        .ok_or_else(|| invalid(ChunkReadError::ChecksumMismatch))
}

/// One of the three DBM files of a user directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserDbm {
    /// The user `Bigram` (`user_bigram.db`) — a hash container.
    Bigram,
    /// The user chewing table (`user_pinyin_index.bin`).
    PinyinIndex,
    /// The user phrase table (`user_phrase_index.bin`).
    PhraseIndex,
}

impl UserDbm {
    /// The file's base name without any extension.
    #[cfg(test)]
    #[must_use]
    pub const fn stem(self) -> &'static str {
        match self {
            Self::Bigram => "user_bigram",
            Self::PinyinIndex => "user_pinyin_index",
            Self::PhraseIndex => "user_phrase_index",
        }
    }

    /// The name libpinyin gives the file (`pinyin_internal.h:58,61-63`).
    ///
    /// The two `.bin` names are DBM tree files despite the extension —
    /// `ChewingLargeTable2::load_db`/`save_db` and
    /// `PhraseLargeTable3::load_db`/`save_db` open the build DBM, the
    /// same as their system-side twins.
    #[must_use]
    pub const fn libpinyin_name(self) -> &'static str {
        match self {
            Self::Bigram => "user_bigram.db",
            Self::PinyinIndex => "user_pinyin_index.bin",
            Self::PhraseIndex => "user_phrase_index.bin",
        }
    }

    /// The file name: libpinyin's own, the same on Kyoto Cabinet, tkrzw and
    /// Berkeley DB (`pinyin_internal.h:58,61-63` does not vary with the DBM).
    #[must_use]
    pub fn file_name(self) -> String {
        self.libpinyin_name().to_owned()
    }

    /// Whether the file is a hash container (`user_bigram.db` is a KC
    /// `HashDB` / tkrzw `HashDBM`; the two index files are trees).
    #[cfg(test)]
    #[must_use]
    pub const fn is_hash(self) -> bool {
        matches!(self, Self::Bigram)
    }
}

/// The user dir's library files, as the system `table.conf` lists them:
/// the `USER_FILE` rows' chunk files and the `SYSTEM_FILE` rows' `.dbin`
/// diff logs (`pinyin.cpp:150-169`, `:933-1090`), by default sub-index.
///
/// A row typed `NOT_USED`, or without a user file name, owns no file: it
/// is neither loaded, saved nor cleaned (`NULL == userfilename` skips it
/// in `_write_files`/`_rename_files`/`_clean_user_files`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserFileLayout {
    user: Vec<(u8, FileName)>,
    logs: Vec<(u8, FileName)>,
    /// Every default row's user file name in sub-index order, which is the
    /// order `_clean_user_files` unlinks them (`pinyin.cpp:152-167`).
    clean: Vec<(u8, FileName)>,
    /// Every `addon` row's user file name in sub-index order; libpinyin
    /// cleans these too and libzhuyin does not (`pinyin.cpp:199-200`).
    addon_clean: Vec<(u8, FileName)>,
}

impl UserFileLayout {
    /// The stock profile layout: `addon.bin`, `network.bin` and `user.bin`
    /// for sub-indices 5–7, and four `.dbin` logs for 1–4.
    ///
    /// An installed `table.conf` can name different files;
    /// [`Self::from_conf`] builds the layout for that configuration.
    #[must_use]
    pub fn stock() -> Self {
        Self::from_conf(&crate::data::table_info::TableConf::stock())
    }

    /// The files `conf`'s default rows name.
    #[must_use]
    pub fn from_conf(conf: &crate::data::table_info::TableConf) -> Self {
        // `_clean_user_files`, `_write_files` and `_rename_files` all start at
        // sub-index 1: "skip the reserved zero phrase library".
        let named = |(index, row): (u8, &crate::data::table_info::TableRow)| {
            (index != 0 && row.file_type != crate::data::table_info::FileType::NotUsed)
                .then(|| row.user.clone())
                .flatten()
                .map(|name| (index, name))
        };
        Self {
            user: conf.user_libraries().filter_map(named).collect(),
            logs: conf.system_libraries().filter_map(named).collect(),
            clean: conf.default_rows().filter_map(named).collect(),
            addon_clean: conf.addon_rows().filter_map(named).collect(),
        }
    }

    /// The `USER_FILE` chunk files by sub-index.
    #[must_use]
    pub(crate) fn user_libraries(&self) -> &[(u8, FileName)] {
        &self.user
    }

    /// The `SYSTEM_FILE` diff logs by sub-index.
    #[must_use]
    pub(crate) fn system_logs(&self) -> &[(u8, FileName)] {
        &self.logs
    }

    /// The default rows' user file names in the pin's cleanup order.
    #[must_use]
    pub(crate) fn default_cleanup(&self) -> &[(u8, FileName)] {
        &self.clean
    }

    /// The addon rows' user file names in the pin's cleanup order.
    #[must_use]
    pub(crate) fn addon_cleanup(&self) -> &[(u8, FileName)] {
        &self.addon_clean
    }

    /// Whether sub-index `index` is a `USER_FILE` library with a file.
    #[must_use]
    pub(crate) fn has_user_library(&self, index: u8) -> bool {
        self.user.iter().any(|&(nibble, _)| nibble == index)
    }
}

/// The version triple `user.conf` conforms against — the system
/// `table.conf`'s identity lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemVersions {
    /// `binary format version:` (7 at the pin).
    pub binary_format_version: u32,
    /// `model data version:` (14 at the pin).
    pub model_data_version: u32,
    /// `database format:` as the system `table.conf` declares it
    /// (`BerkeleyDB`, `KyotoCabinet`, `Tkrzw`).
    pub database_format: &'static str,
}

impl SystemVersions {
    /// This build's identity — the versions of the system `table.conf`
    /// it opens, paired with its own backend token.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub const fn for_this_build(binary_format_version: u32, model_data_version: u32) -> Self {
        Self {
            binary_format_version,
            model_data_version,
            database_format: DEFAULT_STORE_DB_FORMAT,
        }
    }

    /// The versions and database format a system `table.conf` declares
    /// (`UserTableInfo::is_conform`, `table_info.cpp:399-413`, compares the
    /// user marker against exactly these three fields).
    #[must_use]
    pub fn from_conf(conf: &crate::data::table_info::TableConf) -> Self {
        Self {
            binary_format_version: u32::from_ne_bytes(conf.binary_format_version().to_ne_bytes()),
            model_data_version: u32::from_ne_bytes(conf.model_data_version().to_ne_bytes()),
            database_format: conf.database_format().token(),
        }
    }
}

/// `OPEN_COUNTER_LIMIT` (`table_info.cpp:32`): an open counter above this
/// marks the profile non-conform — upstream's periodic-rebuild heuristic.
pub(crate) const OPEN_COUNTER_LIMIT: i32 = 6;

/// `UserTableInfo::get_open_counter` (`table_info.cpp:422-426`): a counter
/// above [`OPEN_COUNTER_LIMIT`] reads as 0. Both ends of libpinyin's open
/// counter go through it — `check_format`'s raise and `pinyin_fini`'s
/// lowering — so a profile that crossed the limit restarts from 0 either
/// way. A negative counter passes through as itself.
#[must_use]
pub(crate) const fn get_open_counter(open_counter: i32) -> i32 {
    if open_counter > OPEN_COUNTER_LIMIT {
        0
    } else {
        open_counter
    }
}

/// `user.conf` — `UserTableInfo` (`table_info.cpp`), the version marker
/// whose conformance check decides whether the previous library's user
/// files are kept or discarded.
///
/// ```text
/// binary format version:7
/// model data version:14
/// database format:KyotoCabinet
/// open counter:3
/// ```
///
/// [`UserTableInfo::load`] and [`UserTableInfo::save`] make the pin's own
/// `fopen`, `fscanf`, `fprintf` and `setlocale` calls through
/// `crate::input::config`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserTableInfo {
    /// `binary format version:`.
    pub binary_format_version: u32,
    /// `model data version:`.
    pub model_data_version: u32,
    /// `database format:` — `None` when the third directive ran off the
    /// end of the file before it converted anything (`UNKNOWN_FORMAT`).
    /// Upstream recognises exactly `UPSTREAM_DB_FORMATS`; any other
    /// token reaches `to_table_database_format_type`'s `abort()`, which
    /// [`UserTableInfo::load`] answers with
    /// [`UserConfError::UnknownDatabaseFormat`] instead.
    pub database_format: Option<String>,
    /// `open counter:` — upstream's `int m_open_counter`
    /// (`table_info.h:102`), negative whenever `%d` reads a negative
    /// value.
    pub open_counter: i32,
}

/// The `database format:` tokens upstream recognises
/// (`to_table_database_format_type`, `table_info.cpp:122-133`); any other
/// token reaches its `abort()` (`:132`). `UNKNOWN_FORMAT` is only the
/// initial value a missing directive leaves.
const UPSTREAM_DB_FORMATS: [&str; 3] = ["BerkeleyDB", "KyotoCabinet", "Tkrzw"];

impl UserTableInfo {
    /// A fresh, conform marker for `versions` with a zero open counter —
    /// what `make_conform` + `check_format` write on a profile's first
    /// save.
    #[must_use]
    pub fn conform_to(versions: &SystemVersions) -> Self {
        Self {
            binary_format_version: versions.binary_format_version,
            model_data_version: versions.model_data_version,
            database_format: Some(versions.database_format.to_owned()),
            open_counter: 0,
        }
    }

    // UserTableInfo::load, table_info.cpp:325–375. The caller handles the pin’s
    // unknown-format abort; the counter conversion is at 356–359.
    /// Reads format versions, the database format and the open counter from the user marker.
    /// A missing database-format value at end of input leaves the format unspecified.
    /// Otherwise the file must contain the `database format:` directive.
    ///
    /// # Errors
    ///
    /// [`UserConfError::Unopenable`] reports an open failure, [`UserConfError::Line`] a
    /// required field that could not be read, and [`UserConfError::UnknownDatabaseFormat`] an
    /// unrecognized format name.
    pub fn load(filename: &(impl crate::input::FileName + ?Sized)) -> Result<Self, UserConfError> {
        let input = UserTableInfoReader::open(filename).map_err(|_| UserConfError::Unopenable)?;
        Self::read(input)
    }

    fn read(mut input: UserTableInfoReader) -> Result<Self, UserConfError> {
        // Each early return drops `input`: the pin's `fclose`, without its
        // closing `setlocale` (`:339-348`).
        let binary_format_version = input
            .binary_format_version()
            .assigned()
            .ok_or(UserConfError::Line("binary format version"))?;
        let model_data_version = input
            .model_data_version()
            .assigned()
            .ok_or(UserConfError::Line("model data version"))?;
        let database_format = match input.database_format() {
            Scanned::Eof => None,
            Scanned::Unassigned => return Err(UserConfError::UnknownDatabaseFormat),
            Scanned::Assigned(token) => Some(
                UPSTREAM_DB_FORMATS
                    .into_iter()
                    .find(|known| known.as_bytes() == token.to_bytes())
                    .ok_or(UserConfError::UnknownDatabaseFormat)?
                    .to_owned(),
            ),
        };
        let open_counter = input.open_counter().assigned().unwrap_or(0);
        input.finish();
        Ok(Self {
            binary_format_version: binary_format_version.cast_unsigned(),
            model_data_version: model_data_version.cast_unsigned(),
            database_format,
            open_counter,
        })
    }

    /// [`Self::load`] of `path`.
    ///
    /// # Errors
    ///
    /// As [`Self::load`]; a path holding a NUL is
    /// [`UserConfError::Unopenable`].
    pub fn load_path(path: &std::path::Path) -> Result<Self, UserConfError> {
        if crate::input::path::path_to_bytes(path).contains(&0) {
            return Err(UserConfError::Unopenable);
        }
        Self::load(path)
    }

    /// `UserTableInfo::save(filename)` (`table_info.cpp:377-397`): `false`
    /// only when `fopen(filename, "w")` failed, after the pin's
    /// `write %s failed.` line. The writes themselves are not checked, as
    /// the pin does not check them.
    ///
    /// A marker without a database format never reaches a save at the pin
    /// (`make_conform` copies the system's first); here it writes nothing
    /// and answers `false`.
    #[must_use]
    pub fn save(&self, filename: &(impl crate::input::FileName + ?Sized)) -> bool {
        let Some(database_format) = self
            .database_format
            .as_deref()
            .and_then(|token| CString::new(token).ok())
        else {
            return false;
        };
        save_user_table_info(
            filename,
            &UserTableInfoFields {
                binary_format_version: self.binary_format_version.cast_signed(),
                model_data_version: self.model_data_version.cast_signed(),
                database_format: &database_format,
                open_counter: self.open_counter,
            },
        )
    }

    /// [`Self::save`] to `path`; `false`, without a call, for a path
    /// holding a NUL, which no C caller can pass.
    #[must_use]
    pub(crate) fn save_path(&self, path: &std::path::Path) -> bool {
        !crate::input::path::path_to_bytes(path).contains(&0) && self.save(path)
    }

    /// The four lines [`Self::save`] writes, in its order, for composing
    /// fixtures; the save itself prints them with the pin's `fprintf`.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn to_text(&self) -> String {
        let database_format = self.database_format.as_deref().unwrap_or("UNKNOWN");
        // `UserTableInfo::save` uses `%d`, not `%u`: the pin's fields are
        // `int m_binary_format_version` / `int m_model_data_version`
        // (`table_info.h:99-100`). A `table.conf` value outside `int` is
        // stored as its C cast and written back signed.
        format!(
            "binary format version:{}\nmodel data version:{}\ndatabase format:{}\nopen counter:{}\n",
            i32::from_ne_bytes(self.binary_format_version.to_ne_bytes()),
            i32::from_ne_bytes(self.model_data_version.to_ne_bytes()),
            database_format,
            self.open_counter
        )
    }

    /// `UserTableInfo::is_conform`: every identity line matches the
    /// system's, and the open counter has not passed the rebuild limit.
    /// A missing or unknown `database format:` line never conforms
    /// (`UNKNOWN_FORMAT != any system format`).
    #[must_use]
    pub fn is_conform(&self, versions: &SystemVersions) -> bool {
        if self.binary_format_version != versions.binary_format_version {
            return false;
        }
        if self.model_data_version != versions.model_data_version {
            return false;
        }
        let Some(format) = self.database_format.as_deref() else {
            return false;
        };
        if format != versions.database_format {
            return false;
        }
        self.open_counter <= OPEN_COUNTER_LIMIT
    }
}

/// What upstream makes of a `user.conf` its `load` cannot complete.
#[derive(Debug)]
pub enum UserConfError {
    /// `fopen` answered NULL, which is the load's `false` (`:330-334`) after
    /// its `open %s failed.` line.
    Unopenable,
    /// A version directive did not convert, which is the load's `false`
    /// (`table_info.cpp:339-342`, `:345-348`): upstream leaves the marker
    /// at its reset defaults and the profile reads as non-conforming.
    Line(&'static str),
    /// The `database format:` directive reached
    /// `to_table_database_format_type`, which `abort()`s on any token it
    /// does not know (`table_info.cpp:122-133`, called at `:353-354`). The
    /// token was either read
    /// and unrecognised, or never read at all: a matching failure leaves
    /// upstream's `str` unwritten and maps it anyway, which is the same
    /// abort from an indeterminate buffer. Both answer `Err`, and the
    /// C adapters abort on it (register row 44).
    UnknownDatabaseFormat,
}

impl std::fmt::Display for UserConfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unopenable => f.write_str("user.conf: cannot be opened"),
            Self::Line(line) => {
                write!(
                    f,
                    "user.conf: the `{line}` directive is missing or malformed"
                )
            }
            Self::UnknownDatabaseFormat => write!(
                f,
                "user.conf: unknown database format (upstream aborts, table_info.cpp:122-133)"
            ),
        }
    }
}

impl std::error::Error for UserConfError {}

/// One `PhraseIndexLogger` record (`phrase_index_logger.h`).
///
/// The stream is records concatenated with no header; every field is
/// native-endian (`u32` `LOG_TYPE`/token, `u16` lengths) and each record
/// carries whole `PhraseItem` chunks as its payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LogRecord<P = Vec<u8>> {
    /// `LOG_ADD_RECORD` — a phrase item the system chunk lacks.
    Add {
        /// The record's token.
        token: u32,
        /// The new item's bytes (a `PhraseItem` chunk).
        new_item: P,
    },
    /// `LOG_REMOVE_RECORD` — a system phrase item removed from the live
    /// index.
    Remove {
        /// The record's token.
        token: u32,
        /// The removed item's bytes, as the system chunk held them.
        old_item: P,
    },
    /// `LOG_MODIFY_RECORD` — a phrase item whose content changed.
    Modify {
        /// The record's token.
        token: u32,
        /// The system item's bytes.
        old_item: P,
        /// The current item's bytes.
        new_item: P,
    },
    /// `LOG_MODIFY_HEADER` — the sub-index `total_freq` change; the
    /// token is `null_token` and both payloads are the same 4-byte
    /// total (old, new — upstream reads one run into both).
    ModifyHeader {
        /// The system sub-index total.
        old_total: u32,
        /// The current sub-index total.
        new_total: u32,
    },
}

impl LogRecord<&[u8]> {
    /// Copy a borrowed record's payloads into an owned record.
    #[cfg(test)]
    #[must_use]
    pub fn into_owned(self) -> LogRecord {
        match self {
            Self::Add { token, new_item } => LogRecord::Add {
                token,
                new_item: new_item.to_vec(),
            },
            Self::Remove { token, old_item } => LogRecord::Remove {
                token,
                old_item: old_item.to_vec(),
            },
            Self::Modify {
                token,
                old_item,
                new_item,
            } => LogRecord::Modify {
                token,
                old_item: old_item.to_vec(),
                new_item: new_item.to_vec(),
            },
            Self::ModifyHeader {
                old_total,
                new_total,
            } => LogRecord::ModifyHeader {
                old_total,
                new_total,
            },
        }
    }
}

/// Why a [`LogReader`] could not decode a record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogFault {
    /// A `MODIFY_HEADER` record whose token is not `null_token`: the pin's
    /// `assert(token == null_token)` (`phrase_index_logger.h:202`) aborts
    /// while reading it.
    HeaderToken(u32),
    /// A complete header record whose bounded runs lack a whole total.
    /// The walk can continue at the next record.
    MissingHeaderTotals,
    /// A payload whose declared length runs past the logger's end: the pin's
    /// `set_content` copies that many bytes from past its chunk, an
    /// over-read with no defined result (class (b)). The walk stops here.
    OverRead,
}

/// How a [`LogReader`] walk ended, read with [`LogReader::end`] once it has
/// yielded its last item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogEnd {
    /// The offset reached the log's end: `has_next_record` is false
    /// (`phrase_index_logger.h:131`).
    Complete,
    /// A record type the walk does not know, or none at all: `next_record`
    /// sets `m_error` (`phrase_index_logger.h:214-216`), which `rewind()`
    /// leaves set (`:141-144`), so every later walk of the same logger
    /// reads nothing (`:128-129`).
    UnknownType,
    /// The last record is missing a required fixed field.
    Truncated,
    /// [`LogFault::OverRead`].
    OverRead,
}

impl std::fmt::Display for LogFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderToken(token) => write!(
                f,
                "phrase index logger: MODIFY_HEADER record carries token {token:#010x}"
            ),
            Self::MissingHeaderTotals => {
                f.write_str("phrase index logger: header totals are missing")
            }
            Self::OverRead => write!(f, "phrase index logger: a payload runs past the log"),
        }
    }
}

impl std::error::Error for LogFault {}

/// `PhraseIndexLogger`'s record walk over a log's bytes, record by record,
/// as `has_next_record` and `next_record` take it (074a2219
/// `src/storage/phrase_index_logger.h:127-215`).
///
/// * The walk goes on while the offset is inside the log.
/// * A missing or unknown type ends the walk.
/// * A record must contain its token and length fields. A missing fixed
///   field ends the walk at that record.
/// * A payload whose declared length exceeds the rest of the log is
///   [`LogFault::OverRead`].
/// * A `MODIFY_HEADER` with a non-null token is [`LogFault::HeaderToken`],
///   before its length is read. Its totals occupy the first four bytes of
///   each run (`phrase_index.cpp:503-514`, `:721-746`). A complete frame
///   with shorter runs still counts as one header.
///
/// The walk continues after [`LogFault::MissingHeaderTotals`].
#[derive(Clone, Debug)]
pub(crate) struct LogReader<'a> {
    bytes: &'a [u8],
    offset: usize,
    done: bool,
    end: Option<LogEnd>,
}

/// Walk a logger's records as borrowed byte slices ([`LogReader`]).
/// Payloads keep their raw sizes, trailing bytes included.
#[must_use]
pub(crate) fn log_records(bytes: &[u8]) -> LogReader<'_> {
    LogReader {
        bytes,
        offset: 0,
        done: false,
        end: None,
    }
}

impl LogReader<'_> {
    /// Why the walk ended, once it has: `None` while it can still yield, and
    /// after a [`LogFault::HeaderToken`] (the pin's assert).
    #[must_use]
    pub const fn end(&self) -> Option<LogEnd> {
        self.end
    }
}

impl<'a> Iterator for LogReader<'a> {
    type Item = Result<LogRecord<&'a [u8]>, LogFault>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if self.offset >= self.bytes.len() {
            self.done = true;
            self.end = Some(LogEnd::Complete);
            return None;
        }
        match self.step() {
            Ok((record, next)) => {
                self.offset = next;
                Some(record.ok_or(LogFault::MissingHeaderTotals))
            }
            Err(LogStop::Fault(fault)) => {
                self.done = true;
                if fault == LogFault::OverRead {
                    self.end = Some(LogEnd::OverRead);
                }
                Some(Err(fault))
            }
            Err(LogStop::End(end)) => {
                self.done = true;
                self.end = Some(end);
                None
            }
        }
    }
}

enum LogStop {
    End(LogEnd),
    Fault(LogFault),
}

type ReadStep<'a> = Result<(Option<LogRecord<&'a [u8]>>, usize), LogStop>;

impl<'a> LogReader<'a> {
    fn u32_field(bytes: &[u8]) -> Option<u32> {
        bytes.first_chunk().copied().map(u32::from_le_bytes)
    }

    fn length_at(&self, at: usize) -> Result<usize, LogStop> {
        self.bytes
            .get(at..)
            .and_then(<[u8]>::first_chunk)
            .copied()
            .map(u16::from_le_bytes)
            .map(usize::from)
            .ok_or(LogStop::End(LogEnd::Truncated))
    }

    fn payload_at(&self, at: usize, len: usize) -> Result<&'a [u8], LogStop> {
        self.bytes
            .get(at..at.saturating_add(len))
            .ok_or(LogStop::Fault(LogFault::OverRead))
    }

    fn header_step(&self, token: u32, mut offset: usize) -> ReadStep<'a> {
        if token != 0 {
            return Err(LogStop::Fault(LogFault::HeaderToken(token)));
        }
        let len = self.length_at(offset)?;
        offset += 2;
        let old_run = self.payload_at(offset, len)?;
        offset += len;
        let new_run = self.payload_at(offset, len)?;
        offset += len;
        let record =
            Self::u32_field(old_run)
                .zip(Self::u32_field(new_run))
                .map(|(old_total, new_total)| LogRecord::ModifyHeader {
                    old_total,
                    new_total,
                });
        Ok((record, offset))
    }

    fn step(&self) -> ReadStep<'a> {
        let mut offset = self.offset;
        let log_type = Self::u32_field(&self.bytes[offset..])
            .filter(|tag| matches!(tag, 1..=4))
            .ok_or(LogStop::End(LogEnd::UnknownType))?;
        offset += LOG_TYPE_SIZE;
        let token =
            Self::u32_field(&self.bytes[offset..]).ok_or(LogStop::End(LogEnd::Truncated))?;
        offset += 4;
        let record = match log_type {
            1 | 2 => {
                let len = self.length_at(offset)?;
                offset += 2;
                let payload = self.payload_at(offset, len)?;
                offset += len;
                if log_type == 1 {
                    LogRecord::Add {
                        token,
                        new_item: payload,
                    }
                } else {
                    LogRecord::Remove {
                        token,
                        old_item: payload,
                    }
                }
            }
            3 => {
                let old_len = self.length_at(offset)?;
                let new_len = self.length_at(offset + 2)?;
                offset += 4;
                let old_item = self.payload_at(offset, old_len)?;
                offset += old_len;
                let new_item = self.payload_at(offset, new_len)?;
                offset += new_len;
                LogRecord::Modify {
                    token,
                    old_item,
                    new_item,
                }
            }
            4 => return self.header_step(token, offset),
            _ => return Err(LogStop::End(LogEnd::UnknownType)),
        };
        Ok((Some(record), offset))
    }
}

/// `sizeof(LOG_TYPE)` — the unscoped enum's underlying `int`.
const LOG_TYPE_SIZE: usize = 4;

/// A record payload whose length does not fit the format's `u16` length
/// field.
///
/// `build_chunk` accepts items up to 255 characters × 255
/// pronunciations, which encodes past `u16::MAX`; upstream's
/// `append_record` truncates silently (`guint16 len = newone->size()`),
/// and we refuse instead of writing a record stream the reader would
/// misparse — a write-side validation, not a read behaviour.
#[derive(Debug)]
pub struct LogEncodeError(pub String);

impl std::fmt::Display for LogEncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "log record payload too large for u16: {}", self.0)
    }
}

impl std::error::Error for LogEncodeError {}

/// The `u16` length field of one payload — an error where the payload
/// does not fit, rather than upstream's silent truncation.
fn u16_len(payload: &[u8], what: &str) -> Result<u16, LogEncodeError> {
    u16::try_from(payload.len()).map_err(|_| LogEncodeError(what.to_owned()))
}

/// Encodes a record stream (`PhraseIndexLogger::append_record`'s
/// layout). Records are written in the given order — the diff's walk
/// order.
///
/// # Errors
///
/// Fails on a payload longer than the format's `u16` length field.
pub fn encode_log_records(records: &[LogRecord]) -> Result<Vec<u8>, LogEncodeError> {
    let mut out = Vec::new();
    let push_head = |log_type: u32, token: u32, out: &mut Vec<u8>| {
        out.extend_from_slice(&log_type.to_le_bytes());
        out.extend_from_slice(&token.to_le_bytes());
    };
    for record in records {
        match record {
            LogRecord::Add { token, new_item } => {
                push_head(1, *token, &mut out);
                out.extend_from_slice(&u16_len(new_item, "ADD payload")?.to_le_bytes());
                out.extend_from_slice(new_item);
            }
            LogRecord::Remove { token, old_item } => {
                push_head(2, *token, &mut out);
                out.extend_from_slice(&u16_len(old_item, "REMOVE payload")?.to_le_bytes());
                out.extend_from_slice(old_item);
            }
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => {
                push_head(3, *token, &mut out);
                out.extend_from_slice(&u16_len(old_item, "MODIFY old payload")?.to_le_bytes());
                out.extend_from_slice(&u16_len(new_item, "MODIFY new payload")?.to_le_bytes());
                out.extend_from_slice(old_item);
                out.extend_from_slice(new_item);
            }
            LogRecord::ModifyHeader {
                old_total,
                new_total,
            } => {
                push_head(4, 0, &mut out);
                // Upstream's MODIFY_HEADER carries the payload length
                // once, then two runs of it — the diff appends a 4-byte
                // old-header chunk and a 4-byte new-header chunk, so each
                // run is one `u32` total.
                out.extend_from_slice(&4_u16.to_le_bytes());
                out.extend_from_slice(&old_total.to_le_bytes());
                out.extend_from_slice(&new_total.to_le_bytes());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
