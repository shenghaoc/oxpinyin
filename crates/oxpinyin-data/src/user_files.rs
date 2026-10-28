//! The user directory's file inventory and its two text/record codecs.
//!
//! The pin's `pinyin_init`/`pinyin_save` read and write a fixed file set
//! in the user dir (`src/pinyin.cpp`'s `_write_files`/`_rename_files`,
//! `pinyin_internal.h:55-66`, the per-library user filenames in
//! `data/table.conf.in`):
//!
//! | file | container | writer |
//! |---|---|---|
//! | `user_bigram.db` | the build DBM's **hash** | `Bigram::save_db` |
//! | `user_pinyin_index.bin` | the build DBM's **tree** (despite the extension) | `ChewingLargeTable2::save_db` |
//! | `user_phrase_index.bin` | the build DBM's tree | `PhraseLargeTable3::save_db` |
//! | `user.bin`, `addon.bin`, `network.bin` | `MemoryChunk` images (the `USER_FILE` sub-indexes 7, 5, 6) | `FacadePhraseIndex::store` |
//! | `gb_char.dbin`, `gbk_char.dbin`, `opengram.dbin`, `merged.dbin` | `MemoryChunk` images of `PhraseIndexLogger` records (the `SYSTEM_FILE` libraries' diffs) | `FacadePhraseIndex::diff` |
//! | `user.conf` | text | `UserTableInfo::save` |
//!
//! On the two backends libpinyin itself builds against (Kyoto Cabinet,
//! tkrzw) the DBM names are libpinyin's own, so a same-backend pair is
//! byte-compatible in both directions.
//! live in that backend's container under `<stem>.<ext>` — a libpinyin
//! those backends never had, which [`UserTableInfo`]'s `database format`
//! conformance line records explicitly: nothing upstream ships can read
//! those files, and a same-backend oxpinyin pair is what stays conform.

use crate::chunk_format::{CHUNK_HEADER_SIZE, chunk_checksum};
use std::ffi::{CStr, CString};

use crate::table_info::FileName;
use oxpinyin_input::config::{
    Scanned, UserTableInfoFields, UserTableInfoReader, save_user_table_info,
};
use oxpinyin_store::DEFAULT_STORE_DB_FORMAT;

/// A `MemoryChunk` file that does not frame a valid payload.
#[derive(Debug)]
pub enum ChunkReadError {
    /// Fewer bytes than the 8-byte header.
    ShortHeader,
    /// The payload length runs past the file's end.
    TruncatedPayload,
    /// The stored checksum does not match the payload.
    ChecksumMismatch,
}

impl std::fmt::Display for ChunkReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ShortHeader => "shorter than the 8-byte header",
            Self::TruncatedPayload => "payload length runs past the file end",
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
/// Fails on a short header, a payload length past the file end, or a
/// checksum mismatch. Trailing bytes beyond the framed payload are
/// ignored, as upstream's reader does.
pub fn read_chunk_payload(bytes: &[u8]) -> Result<&[u8], ChunkReadError> {
    let header = bytes
        .get(..CHUNK_HEADER_SIZE)
        .ok_or(ChunkReadError::ShortHeader)?;
    let length = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let checksum = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let payload = bytes
        .get(CHUNK_HEADER_SIZE..CHUNK_HEADER_SIZE + length)
        .ok_or(ChunkReadError::TruncatedPayload)?;
    if chunk_checksum(payload) != checksum {
        return Err(ChunkReadError::ChecksumMismatch);
    }
    Ok(payload)
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
}

impl UserFileLayout {
    /// The layout every libpinyin install ships: `addon.bin`,
    /// `network.bin`, `user.bin` for sub-indices 5–7 and the four `.dbin`
    /// logs for 1–4.
    #[must_use]
    pub fn stock() -> Self {
        Self::from_conf(&crate::table_info::TableConf::stock())
    }

    /// The files `conf`'s default rows name.
    #[must_use]
    pub fn from_conf(conf: &crate::table_info::TableConf) -> Self {
        // `_clean_user_files`, `_write_files` and `_rename_files` all start at
        // sub-index 1: "skip the reserved zero phrase library".
        let named = |(index, row): (u8, &crate::table_info::TableRow)| {
            row.user
                .clone()
                .filter(|_| index != 0)
                .map(|name| (index, name))
        };
        Self {
            user: conf.user_libraries().filter_map(named).collect(),
            logs: conf.system_libraries().filter_map(named).collect(),
        }
    }

    /// The `USER_FILE` chunk files by sub-index.
    #[must_use]
    pub fn user_libraries(&self) -> &[(u8, FileName)] {
        &self.user
    }

    /// The `SYSTEM_FILE` diff logs by sub-index.
    #[must_use]
    pub fn system_logs(&self) -> &[(u8, FileName)] {
        &self.logs
    }

    /// Whether sub-index `index` is a `USER_FILE` library with a file.
    #[must_use]
    pub fn has_user_library(&self, index: u8) -> bool {
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
    pub fn from_conf(conf: &crate::table_info::TableConf) -> Self {
        Self {
            binary_format_version: u32::from_ne_bytes(conf.binary_format_version().to_ne_bytes()),
            model_data_version: u32::from_ne_bytes(conf.model_data_version().to_ne_bytes()),
            database_format: conf.database_format().token(),
        }
    }
}

/// `binary format version` at the pin (`data/table.conf.in`), and at
/// every libpinyin install since 2.8.1.
pub const PINNED_BINARY_FORMAT_VERSION: u32 = 7;
/// `model data version` at the pin — stable 2.8.1 through 2.11.92.
pub const PINNED_MODEL_DATA_VERSION: u32 = 14;

/// `OPEN_COUNTER_LIMIT` (`table_info.cpp:32`): an open counter above this
/// marks the profile non-conform — upstream's periodic-rebuild heuristic.
pub const OPEN_COUNTER_LIMIT: i32 = 6;

/// `UserTableInfo::get_open_counter` (`table_info.cpp:422-426`): a counter
/// above [`OPEN_COUNTER_LIMIT`] reads as 0. Both ends of libpinyin's open
/// counter go through it — `check_format`'s raise and `pinyin_fini`'s
/// lowering — so a profile that crossed the limit restarts from 0 either
/// way. A negative counter passes through as itself.
#[must_use]
pub const fn get_open_counter(open_counter: i32) -> i32 {
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
/// [`oxpinyin_input::config`].
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

    /// `UserTableInfo::load(filename)` (`table_info.cpp:325-375`).
    ///
    /// A missing `database format:` value (the directive ran into the end
    /// of input) is `UNKNOWN_FORMAT`; a counter that does not convert is 0
    /// (`:356-359`).
    ///
    /// # Errors
    ///
    /// [`UserConfError::Unopenable`] and [`UserConfError::Line`] are the
    /// load's `false` (the pin's open line has been written for the first).
    /// [`UserConfError::UnknownDatabaseFormat`] is the abort point.
    pub fn load(filename: &CStr) -> Result<Self, UserConfError> {
        let mut input =
            UserTableInfoReader::open(filename).map_err(|_| UserConfError::Unopenable)?;
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
        // `if (EOF != num) format = to_table_database_format_type(str);`
        // (`:352-354`): a directive that did not match leaves `str` as it
        // was, never written, and the mapping reads it all the same.
        let database_format = match input.database_format() {
            Scanned::Eof => None,
            Scanned::Assigned(()) | Scanned::Unassigned => Some(
                input
                    .str()
                    .and_then(|token| {
                        UPSTREAM_DB_FORMATS
                            .into_iter()
                            .find(|known| known.as_bytes() == token.to_bytes())
                    })
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
        let filename = oxpinyin_input::c_path(path).ok_or(UserConfError::Unopenable)?;
        Self::load(&filename)
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
    pub fn save(&self, filename: &CStr) -> bool {
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
    pub fn save_path(&self, path: &std::path::Path) -> bool {
        oxpinyin_input::c_path(path).is_some_and(|filename| self.save(&filename))
    }

    /// The four lines [`Self::save`] writes, in its order, for composing
    /// fixtures; the save itself prints them with the pin's `fprintf`.
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
    /// does not know (`table_info.cpp:122-133`, called at `:353-354`) —
    /// the class-(c) availability site of
    /// `docs/findings/compatibility-policy.md`. The token was either read
    /// and unrecognised, or never read at all: a matching failure leaves
    /// upstream's `str` unwritten and maps it anyway, which is the same
    /// abort from an indeterminate buffer. Both answer `Err`, and the
    /// caller fails the open instead of aborting the process.
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

impl<P: AsRef<[u8]>> LogRecord<P> {
    /// Borrow a record's payloads without copying either item.
    #[must_use]
    pub fn as_ref(&self) -> LogRecord<&[u8]> {
        match self {
            Self::Add { token, new_item } => LogRecord::Add {
                token: *token,
                new_item: new_item.as_ref(),
            },
            Self::Remove { token, old_item } => LogRecord::Remove {
                token: *token,
                old_item: old_item.as_ref(),
            },
            Self::Modify {
                token,
                old_item,
                new_item,
            } => LogRecord::Modify {
                token: *token,
                old_item: old_item.as_ref(),
                new_item: new_item.as_ref(),
            },
            Self::ModifyHeader {
                old_total,
                new_total,
            } => LogRecord::ModifyHeader {
                old_total: *old_total,
                new_total: *new_total,
            },
        }
    }
}

impl LogRecord<&[u8]> {
    fn into_owned(self) -> LogRecord {
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

/// Walk a logger's records as borrowed byte slices, stopping at its first error.
/// Payloads retain their raw sizes, including trailing bytes.
pub fn log_records(bytes: &[u8]) -> impl Iterator<Item = Result<LogRecord<&[u8]>, LogDecodeError>> {
    let mut offset = 0;
    std::iter::from_fn(move || {
        if offset >= bytes.len() {
            return None;
        }
        match decode_log_record_at(bytes, offset) {
            Ok((record, next)) => {
                offset = next;
                Some(Ok(record))
            }
            Err(error) => {
                offset = bytes.len();
                Some(Err(error))
            }
        }
    })
}

/// `sizeof(LOG_TYPE)` — the unscoped enum's underlying `int`.
const LOG_TYPE_SIZE: usize = 4;

/// A malformed `PhraseIndexLogger` record stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogDecodeError(LogDecodeFault);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogDecodeFault {
    Head(usize),
    U16(usize),
    Payload { start: usize, len: u16 },
    HeaderToken(u32),
    HeaderTotal,
    HeaderNewRun,
    Type { value: u32, offset: usize },
}

impl LogDecodeError {
    /// [`decode_log_records`]'s rejection of a `MODIFY_HEADER` record whose
    /// token is not `null_token`. Upstream `assert(token == null_token)`s
    /// there (`phrase_index_logger.h:202`), so it is a class (c) abort site
    /// rather than a graceful malformed-stream return.
    fn header_token(token: u32) -> Self {
        Self(LogDecodeFault::HeaderToken(token))
    }

    /// The offending token when this error is the non-null-`MODIFY_HEADER`
    /// rejection (`phrase_index_logger.h:202`), `None` for any other
    /// malformed stream.
    #[must_use]
    pub fn non_null_header_token(&self) -> Option<u32> {
        match self.0 {
            LogDecodeFault::HeaderToken(token) => Some(token),
            _ => None,
        }
    }

    /// The decoded records' `MODIFY_HEADER` count when `bytes` decodes
    /// cleanly; `None` when it does not. The pin's `_peek_header` asserts
    /// `1 >= header_count` (`phrase_index.cpp:745`) once a mask-out walks
    /// the stream.
    #[must_use]
    pub fn decoded_header_count(bytes: &[u8]) -> Option<usize> {
        decode_log_records(bytes).ok().map(|records| {
            records
                .iter()
                .filter(|record| matches!(record, LogRecord::ModifyHeader { .. }))
                .count()
        })
    }
}

impl std::fmt::Display for LogDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "phrase index logger: ")?;
        match self.0 {
            LogDecodeFault::Head(offset) => write!(f, "truncated record head at {offset}"),
            LogDecodeFault::U16(offset) => write!(f, "truncated u16 at {offset}"),
            LogDecodeFault::Payload { start, len } => {
                write!(f, "truncated {len}-byte payload at {start}")
            }
            LogDecodeFault::HeaderToken(token) => {
                write!(f, "MODIFY_HEADER record carries token {token:#010x}")
            }
            LogDecodeFault::HeaderTotal => write!(f, "MODIFY_HEADER payload has no total"),
            LogDecodeFault::HeaderNewRun => write!(f, "MODIFY_HEADER has no new-total run"),
            LogDecodeFault::Type { value, offset } => {
                write!(f, "invalid record type {value} at {offset}")
            }
        }
    }
}

impl std::error::Error for LogDecodeError {}

/// Check a whole logger's framing without allocating or constructing records.
/// Item payloads remain opaque: replay, rather than framing, decides what
/// their bytes mean. The first error and its abort-site token match decoding.
///
/// # Errors
/// Returns the same framing error as [`decode_log_records`], including its
/// first-error order. Even the error value needs no allocation.
pub fn validate_log_records(bytes: &[u8]) -> Result<(), LogDecodeError> {
    let mut offset = 0;
    while offset < bytes.len() {
        offset = validate_log_record_at(bytes, offset)?;
    }
    Ok(())
}

fn validate_log_record_at(bytes: &[u8], mut offset: usize) -> Result<usize, LogDecodeError> {
    let take_u16 = |offset: &mut usize| {
        let lo = *offset;
        if bytes.len() < lo + 2 {
            return Err(LogDecodeError(LogDecodeFault::U16(lo)));
        }
        *offset += 2;
        Ok(u16::from_le_bytes([bytes[lo], bytes[lo + 1]]))
    };
    let skip = |offset: &mut usize, len: u16| {
        let start = *offset;
        let end = start + usize::from(len);
        if bytes.len() < end {
            return Err(LogDecodeError(LogDecodeFault::Payload { start, len }));
        }
        *offset = end;
        Ok(())
    };
    if bytes.len() < offset + LOG_TYPE_SIZE + 4 {
        return Err(LogDecodeError(LogDecodeFault::Head(offset)));
    }
    let log_type = u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]);
    offset += LOG_TYPE_SIZE + 4;
    match log_type {
        1 | 2 => {
            let len = take_u16(&mut offset)?;
            skip(&mut offset, len)?;
        }
        3 => {
            let old_len = take_u16(&mut offset)?;
            let new_len = take_u16(&mut offset)?;
            skip(&mut offset, old_len)?;
            skip(&mut offset, new_len)?;
        }
        4 => {
            let token = u32::from_le_bytes([
                bytes[offset - 4],
                bytes[offset - 3],
                bytes[offset - 2],
                bytes[offset - 1],
            ]);
            if token != 0 {
                return Err(LogDecodeError::header_token(token));
            }
            let len = take_u16(&mut offset)?;
            if len < 4 {
                return Err(LogDecodeError(LogDecodeFault::HeaderTotal));
            }
            skip(&mut offset, len)?;
            if bytes.len() < offset + usize::from(len) {
                return Err(LogDecodeError(LogDecodeFault::HeaderNewRun));
            }
            offset += usize::from(len);
        }
        value => return Err(LogDecodeError(LogDecodeFault::Type { value, offset })),
    }
    Ok(offset)
}

/// Decodes a record stream (`PhraseIndexLogger::next_record`'s loop).
///
/// # Errors
///
/// Fails on an invalid record type, a truncated payload, or a
/// `MODIFY_HEADER` record whose token is not `null_token`.
pub fn decode_log_records(bytes: &[u8]) -> Result<Vec<LogRecord>, LogDecodeError> {
    let (records, error) = decode_log_records_partial(bytes);
    match error {
        Some(error) => Err(error),
        None => Ok(records),
    }
}

/// [`decode_log_records`] with the partial result kept: the records that
/// decoded before the first malformed one, plus that error when the
/// stream did not end cleanly.
///
/// Upstream's `_peek_header` counts each successful `next_record` before
/// it breaks on a `false` and asserts on the accumulated count
/// (`phrase_index.cpp:721-746`), so a trailing malformed record does not
/// erase the headers already read. A caller reproducing that assert must
/// see the records up to the error, not an empty list.
#[must_use]
pub fn decode_log_records_partial(bytes: &[u8]) -> (Vec<LogRecord>, Option<LogDecodeError>) {
    let mut records = Vec::new();
    let mut offset = 0_usize;
    while offset < bytes.len() {
        match decode_log_record_at(bytes, offset) {
            Ok((record, next)) => {
                records.push(record.into_owned());
                offset = next;
            }
            Err(error) => return (records, Some(error)),
        }
    }
    (records, None)
}

/// Decodes the record starting at `offset`, returning it with the offset
/// one past its payload.
fn decode_log_record_at(
    bytes: &[u8],
    mut offset: usize,
) -> Result<(LogRecord<&[u8]>, usize), LogDecodeError> {
    let take_u16 = |offset: &mut usize| -> Result<u16, LogDecodeError> {
        let lo = *offset;
        let hi = lo + 2;
        if bytes.len() < hi {
            return Err(LogDecodeError(LogDecodeFault::U16(lo)));
        }
        let value = u16::from_le_bytes([bytes[lo], bytes[lo + 1]]);
        *offset = hi;
        Ok(value)
    };
    let take_bytes = |offset: &mut usize, len: u16| -> Result<&[u8], LogDecodeError> {
        let start = *offset;
        let end = start + usize::from(len);
        if bytes.len() < end {
            return Err(LogDecodeError(LogDecodeFault::Payload { start, len }));
        }
        *offset = end;
        Ok(&bytes[start..end])
    };

    if bytes.len() < offset + LOG_TYPE_SIZE + 4 {
        return Err(LogDecodeError(LogDecodeFault::Head(offset)));
    }
    let log_type = u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]);
    let token = u32::from_le_bytes([
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ]);
    offset += LOG_TYPE_SIZE + 4;

    let record = match log_type {
        1 => {
            let len = take_u16(&mut offset)?;
            let new_item = take_bytes(&mut offset, len)?;
            LogRecord::Add { token, new_item }
        }
        2 => {
            let len = take_u16(&mut offset)?;
            let old_item = take_bytes(&mut offset, len)?;
            LogRecord::Remove { token, old_item }
        }
        3 => {
            let old_len = take_u16(&mut offset)?;
            let new_len = take_u16(&mut offset)?;
            let old_item = take_bytes(&mut offset, old_len)?;
            let new_item = take_bytes(&mut offset, new_len)?;
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            }
        }
        4 => {
            if token != 0 {
                return Err(LogDecodeError::header_token(token));
            }
            let len = take_u16(&mut offset)?;
            if usize::from(len) < 4 {
                return Err(LogDecodeError(LogDecodeFault::HeaderTotal));
            }
            // Two consecutive `len`-byte runs: the old totals, then
            // the new — `next_record` hands one run to each payload.
            let old_run = take_bytes(&mut offset, len)?;
            if bytes.len() < offset + usize::from(len) {
                return Err(LogDecodeError(LogDecodeFault::HeaderNewRun));
            }
            let new_run = take_bytes(&mut offset, len)?;
            let old_total = u32::from_le_bytes([old_run[0], old_run[1], old_run[2], old_run[3]]);
            let new_total = u32::from_le_bytes([new_run[0], new_run[1], new_run[2], new_run[3]]);
            LogRecord::ModifyHeader {
                old_total,
                new_total,
            }
        }
        other => {
            return Err(LogDecodeError(LogDecodeFault::Type {
                value: other,
                offset,
            }));
        }
    };
    Ok((record, offset))
}

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
mod tests {
    use super::*;

    #[test]
    fn user_dbm_names_are_libpinyins_own() {
        assert_eq!(UserDbm::Bigram.file_name(), "user_bigram.db");
        assert_eq!(UserDbm::PinyinIndex.file_name(), "user_pinyin_index.bin");
        assert_eq!(UserDbm::PhraseIndex.file_name(), "user_phrase_index.bin");
        assert!(UserDbm::Bigram.is_hash());
        assert!(!UserDbm::PinyinIndex.is_hash());
        let layout = UserFileLayout::stock();
        assert_eq!(
            layout.user_libraries()[2],
            (7, FileName::from_bytes(b"user.bin"))
        );
        assert_eq!(
            layout.system_logs()[0],
            (1, FileName::from_bytes(b"gb_char.dbin"))
        );
    }

    #[test]
    fn chunk_payload_round_trip_through_the_frame() {
        let payload = encode_log_records(&[LogRecord::Add {
            token: 7,
            new_item: vec![1, 2, 3, 4],
        }])
        .expect("encode");
        let framed = crate::chunk_format::build_memory_chunk(&payload).expect("frame");
        assert_eq!(read_chunk_payload(&framed).expect("frame"), &payload[..]);

        // Hostile frames answer typed errors, never panic.
        assert!(matches!(
            read_chunk_payload(&framed[..7]),
            Err(ChunkReadError::ShortHeader)
        ));
        let mut truncated = framed.clone();
        truncated.truncate(framed.len() - 1);
        assert!(matches!(
            read_chunk_payload(&truncated),
            Err(ChunkReadError::TruncatedPayload)
        ));
        let mut flipped = framed.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0xFF;
        assert!(matches!(
            read_chunk_payload(&flipped),
            Err(ChunkReadError::ChecksumMismatch)
        ));
    }

    fn parse(bytes: &[u8]) -> Result<UserTableInfo, UserConfError> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("user.conf");
        std::fs::write(&path, bytes).expect("write");
        UserTableInfo::load_path(&path)
    }

    #[test]
    fn user_conf_round_trips_and_conforms() {
        let versions = SystemVersions::for_this_build(7, 14);
        let conform = UserTableInfo::conform_to(&versions);
        assert!(conform.is_conform(&versions));

        let text = conform.to_text();
        assert_eq!(
            text,
            format!(
                "binary format version:7\nmodel data version:14\ndatabase format:{DEFAULT_STORE_DB_FORMAT}\nopen counter:0\n"
            )
        );
        let parsed = parse(text.as_bytes()).expect("parse");
        assert_eq!(parsed, conform);
        // `save` writes the same four lines with the pin's `fprintf`.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("user.conf");
        let filename = oxpinyin_input::c_path(&path).expect("c path");
        assert!(conform.save(&filename));
        assert_eq!(std::fs::read_to_string(&path).expect("read"), text);

        // A stale model version never conforms.
        let stale = UserTableInfo {
            model_data_version: 13,
            ..conform.clone()
        };
        assert!(!stale.is_conform(&versions));

        // A cross-backend marker never conforms — the ruling's mechanism.
        let mut other = conform.clone();
        if DEFAULT_STORE_DB_FORMAT == "KyotoCabinet" {
            other.database_format = Some("Tkrzw".to_owned());
        } else {
            other.database_format = Some("KyotoCabinet".to_owned());
        }
        assert!(!other.is_conform(&versions));
        // A token no upstream build ever emits never conforms — and is
        // not even parsable any more: the `database format:` directive
        // refuses it where upstream's mapper aborts (#590).
        assert!(!conform_with("NotADbmLibrary").is_conform(&versions));
        assert!(matches!(
            parse(
                b"binary format version:7\nmodel data version:14\ndatabase format:NotADbmLibrary\n"
            ),
            Err(UserConfError::UnknownDatabaseFormat)
        ));

        // The open-counter rebuild limit.
        let tired = UserTableInfo {
            open_counter: OPEN_COUNTER_LIMIT + 1,
            ..conform.clone()
        };
        assert!(!tired.is_conform(&versions));
        let rested = UserTableInfo {
            open_counter: OPEN_COUNTER_LIMIT,
            ..conform
        };
        assert!(rested.is_conform(&versions));
        // get_open_counter reads a counter past the limit as 0, and any
        // other value as itself.
        assert_eq!(get_open_counter(OPEN_COUNTER_LIMIT), OPEN_COUNTER_LIMIT);
        assert_eq!(get_open_counter(OPEN_COUNTER_LIMIT + 1), 0);
        assert_eq!(get_open_counter(i32::MAX), 0);
        assert_eq!(get_open_counter(0), 0);
        assert_eq!(get_open_counter(-3), -3);
        assert_eq!(get_open_counter(i32::MIN), i32::MIN);

        // The marker conforms against what table.conf declares.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("table.conf"),
            b"binary format version:9\nmodel data version:20\nlambda parameter:1\n\
              source table format:pinyin\ndatabase format:Tkrzw\n",
        )
        .expect("write");
        let declared = crate::table_info::TableConf::load_from_dir(dir.path()).expect("parses");
        assert_eq!(
            SystemVersions::from_conf(&declared),
            SystemVersions {
                binary_format_version: 9,
                model_data_version: 20,
                database_format: "Tkrzw",
            }
        );

        // The third directive running off the end of the file leaves
        // `format` at UNKNOWN_FORMAT — the one outcome that is not
        // upstream's abort.
        let sparse = parse(b"binary format version:7\nmodel data version:14\n").expect("parse");
        assert_eq!(sparse.database_format, None);
        assert_eq!(sparse.open_counter, 0);
        assert!(!sparse.is_conform(&versions));
        assert!(matches!(
            parse(b"binary format version:x\n"),
            Err(UserConfError::Line("binary format version"))
        ));
        assert!(matches!(
            parse(b""),
            Err(UserConfError::Line("binary format version"))
        ));
    }

    /// Each directive is one `fscanf`: a field stops where `%d` stops, a
    /// whitespace directive crosses line breaks, a counter that does not
    /// convert is 0 and the database token is the C string `%255s` stored.
    #[test]
    fn marker_fields_follow_the_scanner() {
        let good = "binary format version:7\nmodel data version:14\ndatabase format:BerkeleyDB\nopen counter:3\n";
        let counter = |value: &str| {
            parse(
                good.replace("counter:3", &format!("counter:{value}"))
                    .as_bytes(),
            )
            .expect(value)
            .open_counter
        };
        assert_eq!(counter("3x"), 3);
        assert_eq!(counter("x"), 0);
        assert_eq!(counter("-3"), -3);
        assert!(matches!(
            parse(good.replace("version:7", "version:7x").as_bytes()),
            Err(UserConfError::Line("model data version"))
        ));
        assert_eq!(
            parse(good.replace("BerkeleyDB", "BerkeleyDB\0suffix").as_bytes())
                .expect("C string")
                .database_format
                .as_deref(),
            Some("BerkeleyDB")
        );
        assert_eq!(
            parse(good.replace('\n', " ").as_bytes())
                .expect("one line")
                .open_counter,
            3
        );
        let missing = good.replace("open counter:3\n", "");
        assert_eq!(parse(missing.as_bytes()).unwrap().open_counter, 0);
        // A directive that does not match leaves `str` unwritten: the abort
        // point, not `UNKNOWN_FORMAT`.
        assert!(matches!(
            parse(good.replace("database format:", "database:").as_bytes()),
            Err(UserConfError::UnknownDatabaseFormat)
        ));
    }

    #[test]
    fn a_negative_counter_conforms() {
        let versions = SystemVersions::for_this_build(7, 14);
        let negative = UserTableInfo {
            open_counter: -3,
            ..UserTableInfo::conform_to(&versions)
        };
        assert!(negative.is_conform(&versions));
        assert!(negative.to_text().ends_with("open counter:-3\n"));
        assert_eq!(
            parse(negative.to_text().as_bytes()).expect("parse"),
            negative
        );
    }

    #[test]
    fn an_out_of_int_version_is_written_signed() {
        // The pin reads `%d` into an `int` and writes `%d`; a value past
        // `INT_MAX` therefore appears as its two's-complement value.
        let versions = SystemVersions::for_this_build(u32::MAX, 0x8000_0000);
        let info = UserTableInfo::conform_to(&versions);
        let text = info.to_text();
        assert!(
            text.starts_with("binary format version:-1\nmodel data version:-2147483648\n"),
            "{text}"
        );
        assert_eq!(parse(text.as_bytes()).expect("parse"), info);
    }

    fn conform_with(token: &str) -> UserTableInfo {
        UserTableInfo {
            binary_format_version: 7,
            model_data_version: 14,
            database_format: Some(token.to_owned()),
            open_counter: 0,
        }
    }

    #[test]
    fn log_records_round_trip_through_the_pin_layout() {
        let records = vec![
            LogRecord::ModifyHeader {
                old_total: 138_096,
                new_total: 138_579,
            },
            LogRecord::Add {
                token: 0x0700_0001,
                new_item: vec![1, 1, 69, 0, 0, 0],
            },
            LogRecord::Modify {
                token: 0x0100_0002,
                old_item: vec![2, 3, 4],
                new_item: vec![5, 6],
            },
            LogRecord::Remove {
                token: 0x0200_0003,
                old_item: vec![7, 8, 9, 10],
            },
        ];
        let bytes = encode_log_records(&records).expect("encode");

        // Hand-check the first two records' bytes against
        // append_record's layout.
        let mut expected = Vec::new();
        expected.extend_from_slice(&4_u32.to_le_bytes()); // MODIFY_HEADER
        expected.extend_from_slice(&0_u32.to_le_bytes()); // null_token
        expected.extend_from_slice(&4_u16.to_le_bytes()); // one u32 total per run
        expected.extend_from_slice(&138_096_u32.to_le_bytes());
        expected.extend_from_slice(&138_579_u32.to_le_bytes());
        expected.extend_from_slice(&1_u32.to_le_bytes()); // ADD
        expected.extend_from_slice(&0x0700_0001_u32.to_le_bytes());
        expected.extend_from_slice(&6_u16.to_le_bytes());
        expected.extend_from_slice(&[1, 1, 69, 0, 0, 0]);
        assert_eq!(&bytes[..expected.len()], &expected[..]);

        assert_eq!(decode_log_records(&bytes).expect("decode"), records);
        assert!(decode_log_records(&[]).expect("decode").is_empty());
    }

    #[test]
    fn log_encode_rejects_a_payload_beyond_u16() {
        // 70_001 bytes: over the u16 length field, under nothing else.
        let huge = vec![0_u8; 70_001];
        assert!(
            encode_log_records(&[LogRecord::Add {
                token: 7,
                new_item: huge,
            }])
            .is_err(),
            "an oversized payload must be refused, not truncated"
        );
    }

    #[test]
    fn log_validation_matches_every_decoder_rejection() {
        let record = |kind: u32, token: u32, tail: &[u8]| {
            [
                kind.to_le_bytes().as_slice(),
                token.to_le_bytes().as_slice(),
                tail,
            ]
            .concat()
        };
        let cases = [
            (vec![0; 3], "truncated record head at 0"),
            (record(9, 0, &[]), "invalid record type 9 at 8"),
            (record(1, 1, &[]), "truncated u16 at 8"),
            (record(2, 1, &[0]), "truncated u16 at 8"),
            (record(3, 1, &[]), "truncated u16 at 8"),
            (record(3, 1, &[3, 0]), "truncated u16 at 10"),
            (record(4, 0, &[]), "truncated u16 at 8"),
            (
                record(1, 1, &[3, 0, 1, 2]),
                "truncated 3-byte payload at 10",
            ),
            (
                record(2, 1, &[3, 0, 1, 2]),
                "truncated 3-byte payload at 10",
            ),
            (
                record(3, 1, &[3, 0, 1, 0, 1, 2]),
                "truncated 3-byte payload at 12",
            ),
            (
                record(3, 1, &[3, 0, 2, 0, 1, 2, 3, 4]),
                "truncated 2-byte payload at 15",
            ),
            (
                record(4, 1, &[]),
                "MODIFY_HEADER record carries token 0x00000001",
            ),
            (record(4, 0, &[3, 0]), "MODIFY_HEADER payload has no total"),
            (
                record(4, 0, &[4, 0, 1, 2, 3]),
                "truncated 4-byte payload at 10",
            ),
            (
                record(4, 0, &[4, 0, 1, 2, 3, 4, 5]),
                "MODIFY_HEADER has no new-total run",
            ),
        ];
        let mut mismatches = Vec::new();
        for (index, (bytes, reason)) in cases.iter().enumerate() {
            let expected = format!("phrase index logger: {reason}");
            let decoded = decode_log_records(bytes).expect_err("decoder rejects");
            let validated = validate_log_records(bytes);
            if validated.as_ref().map_err(ToString::to_string) != Err(expected.clone())
                || decoded.to_string() != expected
                || validated
                    .err()
                    .and_then(|error| error.non_null_header_token())
                    != decoded.non_null_header_token()
            {
                mismatches.push(index);
            }
        }
        assert!(
            mismatches.is_empty(),
            "mismatched rejection paths: {mismatches:?}"
        );
    }

    #[test]
    fn log_validation_matches_prefixes_and_opaque_payloads() {
        let mut bytes = encode_log_records(&[
            LogRecord::Add {
                token: 1,
                new_item: vec![255],
            },
            LogRecord::Remove {
                token: 2,
                old_item: vec![],
            },
            LogRecord::Modify {
                token: 3,
                old_item: vec![255, 255],
                new_item: vec![],
            },
            LogRecord::ModifyHeader {
                old_total: u32::MAX,
                new_total: 0,
            },
        ])
        .expect("encode");
        // A header may carry padding after each total; those bytes are opaque.
        bytes.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 6, 0]);
        bytes.extend_from_slice(&[255; 12]);
        for end in 0..=bytes.len() {
            assert_eq!(
                validate_log_records(&bytes[..end]).map_err(|error| error.to_string()),
                decode_log_records(&bytes[..end])
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                "prefix {end}"
            );
        }
        assert!(validate_log_records(&bytes).is_ok());
        assert!(decode_log_records(&bytes).is_ok());
    }

    #[test]
    fn log_decode_rejects_hostile_streams() {
        // Truncated head.
        assert!(decode_log_records(&[0, 0, 0]).is_err());
        // Invalid type.
        assert!(decode_log_records(&[9, 0, 0, 0, 0, 0, 0, 0]).is_err());
        // Truncated payload.
        let add = encode_log_records(&[LogRecord::Add {
            token: 1,
            new_item: vec![1, 2, 3],
        }])
        .expect("encode");
        assert!(decode_log_records(&add[..add.len() - 1]).is_err());
        // MODIFY_HEADER must carry null_token.
        let mut bad_header = encode_log_records(&[LogRecord::ModifyHeader {
            old_total: 1,
            new_total: 2,
        }])
        .expect("encode");
        bad_header[4] = 1; // corrupt the token's low byte
        let error = decode_log_records(&bad_header).expect_err("non-null header token");
        assert_eq!(error.non_null_header_token(), Some(1));
        // A truncation is a different fault kind, not the abort-site one.
        let truncated = decode_log_records(&add[..add.len() - 1]).expect_err("truncated");
        assert_eq!(truncated.non_null_header_token(), None);
        // The mask-out header count: two headers is the `_peek_header`
        // abort shape (`phrase_index.cpp:745`); one is the normal case.
        let two_headers = encode_log_records(&[
            LogRecord::ModifyHeader {
                old_total: 1,
                new_total: 2,
            },
            LogRecord::ModifyHeader {
                old_total: 2,
                new_total: 3,
            },
        ])
        .expect("encode");
        assert_eq!(LogDecodeError::decoded_header_count(&two_headers), Some(2));
        let one_header = encode_log_records(&[LogRecord::ModifyHeader {
            old_total: 1,
            new_total: 2,
        }])
        .expect("encode");
        assert_eq!(LogDecodeError::decoded_header_count(&one_header), Some(1));
        assert_eq!(LogDecodeError::decoded_header_count(&bad_header), None);
    }
}
