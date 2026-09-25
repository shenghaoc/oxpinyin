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
use oxpinyin_store::{DEFAULT_STORE_DB_FORMAT, DEFAULT_STORE_EXT, DEFAULT_STORE_IS_LIBPINYIN_DBM};

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

    /// The file name for the compiled-in backend: libpinyin's own on
    /// Kyoto Cabinet, tkrzw and Berkeley DB.
    #[must_use]
    pub fn file_name(self) -> String {
        if DEFAULT_STORE_IS_LIBPINYIN_DBM {
            self.libpinyin_name().to_owned()
        } else {
            format!("{}.{DEFAULT_STORE_EXT}", self.stem())
        }
    }

    /// Whether the file is a hash container (`user_bigram.db` is a KC
    /// `HashDB` / tkrzw `HashDBM`; the two index files are trees).
    #[must_use]
    pub const fn is_hash(self) -> bool {
        matches!(self, Self::Bigram)
    }
}

/// The `USER_FILE` sub-indexes' chunk files by nibble — `table.conf`'s
/// `default …_DICTIONARY` `USER_FILE` rows' user filenames.
pub const USER_LIBRARY_FILES: &[(u8, &str)] =
    &[(5, "addon.bin"), (6, "network.bin"), (7, "user.bin")];

/// The `SYSTEM_FILE` libraries' diff-log files by nibble.
///
/// `table.conf`'s `default …_DICTIONARY` `SYSTEM_FILE` rows' user
/// filenames (the `.dbin` logs `_write_files` writes through
/// `FacadePhraseIndex::diff`).
pub const SYSTEM_LOG_FILES: &[(u8, &str)] = &[
    (1, "gb_char.dbin"),
    (2, "gbk_char.dbin"),
    (3, "opengram.dbin"),
    (4, "merged.dbin"),
];

/// The version triple `user.conf` conforms against — the system
/// `table.conf`'s identity lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemVersions {
    /// `binary format version:` (7 at the pin).
    pub binary_format_version: u32,
    /// `model data version:` (14 at the pin).
    pub model_data_version: u32,
    /// `database format:` — the DBM the writing libpinyin was built
    /// against (`BerkeleyDB`, `KyotoCabinet`, `Tkrzw`; this build's own
    /// token).
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

    /// The versions a system `table.conf` declares; the pin's values
    /// (`7` / `14`, stable across 2.8.1→pin) when the file is absent
    /// or silent.
    #[must_use]
    pub fn from_table_conf(text: &str) -> Self {
        let mut binary_format_version = PINNED_BINARY_FORMAT_VERSION;
        let mut model_data_version = PINNED_MODEL_DATA_VERSION;
        for line in text.lines() {
            if let Some(parsed) = line
                .strip_prefix("binary format version:")
                .and_then(|value| value.trim().parse().ok())
            {
                binary_format_version = parsed;
            } else if let Some(parsed) = line
                .strip_prefix("model data version:")
                .and_then(|value| value.trim().parse().ok())
            {
                model_data_version = parsed;
            }
        }
        Self::for_this_build(binary_format_version, model_data_version)
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
/// The first two lines are required; `database format:` and
/// `open counter:` default (`UNKNOWN`/0) when absent, matching
/// upstream's `fscanf` tolerance. The counter is read as that `fscanf`'s
/// `%d` reads it ([`UserTableInfo::parse`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserTableInfo {
    /// `binary format version:`.
    pub binary_format_version: u32,
    /// `model data version:`.
    pub model_data_version: u32,
    /// `database format:` — `None` when the line is absent or names no
    /// token upstream recognises (`UNKNOWN_FORMAT`).
    pub database_format: Option<String>,
    /// `open counter:` — upstream's `int m_open_counter`
    /// (`table_info.h:102`), negative whenever `%d` reads a negative
    /// value.
    pub open_counter: i32,
}

/// The `database format:` tokens upstream recognises
/// (`to_table_database_format_type`); anything else is `UNKNOWN_FORMAT`.
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

    /// Parses `user.conf` text. See the type doc for the line set and
    /// the defaults for absent lines.
    ///
    /// The counter is read the way upstream's
    /// `fscanf(input, "open counter:%d\n", &counter)` reads it
    /// (`table_info.cpp:356-359`). The first line that holds the literal
    /// counts, found after any white space the previous `fscanf`'s
    /// trailing `\n` directive leaves unread (`:352`). The literal's space
    /// matches any run of white space, an empty one included. `%d` then
    /// runs on over the rest of the file, as glibc's does, and a failed
    /// conversion reads 0. The line model is the pre-existing one — this
    /// reads the counter's VALUE as `%d` does, not the whole marker as a
    /// sequence of `fscanf` calls.
    ///
    /// # Errors
    ///
    /// Fails when either version line is missing or does not parse.
    pub fn parse(text: &str) -> Result<Self, UserConfError> {
        let mut binary_format_version = None;
        let mut model_data_version = None;
        let mut database_format = None;
        let mut open_counter = None;

        let bytes = text.as_bytes();
        // Counter discovery is a single monotonic pass. `probe` is the
        // first non-white-space byte at or after the last probed line's
        // start: every byte before it is spent — a white-space run is
        // skipped once, not once per line, so a marker padded with blank
        // lines parses in linear time. A landing that failed its literal
        // attempt is never re-attempted (`probed`): a later line whose
        // start is at most `probe` sits inside the white space the probe
        // already skipped, so its landing — and its attempt — is the
        // same one, with the same outcome.
        let mut line_start = 0;
        let mut probe = 0_usize;
        let mut probed = usize::MAX;
        for raw in text.split_inclusive('\n') {
            let start = line_start;
            line_start += raw.len();
            let line = raw
                .strip_suffix('\n')
                .map_or(raw, |line| line.strip_suffix('\r').unwrap_or(line));
            if let Some(value) = line.strip_prefix("binary format version:") {
                binary_format_version =
                    Some(parse_u32(value).ok_or(UserConfError::Line("binary format version"))?);
            } else if let Some(value) = line.strip_prefix("model data version:") {
                model_data_version =
                    Some(parse_u32(value).ok_or(UserConfError::Line("model data version"))?);
            } else if let Some(value) = line.strip_prefix("database format:") {
                // Upstream reads the token with %255s and maps it through
                // to_table_database_format_type; unknown → UNKNOWN_FORMAT,
                // not an error.
                let token = value.trim();
                database_format = Some(token.to_owned());
            } else if open_counter.is_none() {
                if probe < start {
                    probe =
                        bytes.len() - skip_c_space(bytes.get(start..).unwrap_or_default()).len();
                    probed = usize::MAX;
                }
                if probe < bytes.len() && probed != probe {
                    // `counter_value` skips white space itself, but the
                    // probe lands on a non-white-space byte, so that skip
                    // costs nothing here.
                    probed = probe;
                    open_counter = counter_value(bytes.get(probe..).unwrap_or_default());
                }
            }
        }

        Ok(Self {
            binary_format_version: binary_format_version
                .ok_or(UserConfError::Line("binary format version"))?,
            model_data_version: model_data_version
                .ok_or(UserConfError::Line("model data version"))?,
            database_format,
            open_counter: open_counter.unwrap_or(0),
        })
    }

    /// Serialises to `user.conf` text — `UserTableInfo::save`'s four
    /// lines, in its order.
    #[must_use]
    pub fn to_text(&self) -> String {
        let database_format = self.database_format.as_deref().unwrap_or("UNKNOWN");
        format!(
            "binary format version:{}\nmodel data version:{}\ndatabase format:{}\nopen counter:{}\n",
            self.binary_format_version, self.model_data_version, database_format, self.open_counter
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

    /// Whether the stored `database format:` token is one upstream
    /// recognises — used to keep a marker written by some future backend
    /// from being silently reinterpreted.
    #[must_use]
    pub fn known_database_format(&self) -> bool {
        self.database_format
            .as_deref()
            .is_some_and(|token| UPSTREAM_DB_FORMATS.contains(&token))
    }
}

/// A `user.conf` that does not parse.
#[derive(Debug)]
pub enum UserConfError {
    /// A required line is missing or malformed.
    Line(&'static str),
}

impl std::fmt::Display for UserConfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let UserConfError::Line(line) = self;
        write!(f, "user.conf: the `{line}` line is missing or malformed")
    }
}

impl std::error::Error for UserConfError {}

/// A version line's value: an unsigned decimal, nothing else.
fn parse_u32(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    trimmed.parse().ok()
}

/// C's `isspace` over ASCII — the white space a `scanf` directive or
/// conversion skips: space, `\t`, `\n`, `\v`, `\f` and `\r`.
/// [`u8::is_ascii_whitespace`] leaves out `\v`.
const fn is_c_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn skip_c_space(input: &[u8]) -> &[u8] {
    let start = input
        .iter()
        .position(|&byte| !is_c_space(byte))
        .unwrap_or(input.len());
    input.get(start..).unwrap_or_default()
}

/// A `scanf` format's literal at the start of `input`: a white-space
/// byte of the format matches any run of white space, an empty one
/// included, and any other byte matches only itself (C11 7.21.6.2p5-6).
/// The input after the match, or `None` where it fails.
fn scanf_literal<'a>(input: &'a [u8], literal: &[u8]) -> Option<&'a [u8]> {
    let mut rest = input;
    for &byte in literal {
        if is_c_space(byte) {
            rest = skip_c_space(rest);
        } else {
            let (&first, tail) = rest.split_first()?;
            if first != byte {
                return None;
            }
            rest = tail;
        }
    }
    Some(rest)
}

/// `fscanf(input, "open counter:%d\n", &counter)` at `rest`, the file from
/// one line's start on (`table_info.cpp:356-359`). That line's leading
/// white space is what the previous `fscanf`'s trailing `\n` directive
/// skips (`:352`) — `parse`'s probe has usually spent that skip already,
/// so this one costs nothing; the literal matches as the format's does;
/// `%d` reads the value, and a failed conversion leaves the counter 0
/// (`:358-359`). `None` when no counter line was reached.
fn counter_value(rest: &[u8]) -> Option<i32> {
    let value = scanf_literal(skip_c_space(rest), b"open counter:")?;
    Some(scan_int(value).unwrap_or(0))
}

/// `fscanf`'s `%d` as the pin's glibc runs it. White space is skipped
/// first — newlines too, so an empty value reads on into the next line.
/// Then comes an optional sign and every decimal digit that follows.
/// glibc converts the digits with `strtol`, which saturates at `long`'s
/// range (64 bits on the pin's LP64 builds), and stores the result
/// through an `int *`, which keeps its low 32 bits. So `2147483648` reads
/// as −2147483648 and `4294967303` as 7. Anything past `long` reads as
/// the low half of `LONG_MAX` (−1) or of `LONG_MIN` (0). `None` is a
/// failed conversion: no digit after the optional sign.
fn scan_int(input: &[u8]) -> Option<i32> {
    let rest = skip_c_space(input);
    let (negative, rest) = match rest.split_first() {
        Some((b'-', tail)) => (true, tail),
        Some((b'+', tail)) => (false, tail),
        _ => (false, rest),
    };
    // `strtol` accumulates toward the sign, so `LONG_MIN` is reachable;
    // `None` once the value has left `long`'s range.
    let mut long = Some(0_i64);
    let mut any_digit = false;
    for &byte in rest.iter().take_while(|byte| byte.is_ascii_digit()) {
        any_digit = true;
        let digit = i64::from(byte - b'0');
        long = long
            .and_then(|value| value.checked_mul(10))
            .and_then(|value| {
                if negative {
                    value.checked_sub(digit)
                } else {
                    value.checked_add(digit)
                }
            });
    }
    if !any_digit {
        return None;
    }
    let long = long.unwrap_or(if negative { i64::MIN } else { i64::MAX });
    // The store through `int *`: the low 32 bits.
    Some(long as i32)
}

/// One `PhraseIndexLogger` record (`phrase_index_logger.h`).
///
/// The stream is records concatenated with no header; every field is
/// native-endian (`u32` `LOG_TYPE`/token, `u16` lengths) and each record
/// carries whole `PhraseItem` chunks as its payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LogRecord {
    /// `LOG_ADD_RECORD` — a phrase item the system chunk lacks.
    Add {
        /// The record's token.
        token: u32,
        /// The new item's bytes (a `PhraseItem` chunk).
        new_item: Vec<u8>,
    },
    /// `LOG_REMOVE_RECORD` — a system phrase item removed from the live
    /// index.
    Remove {
        /// The record's token.
        token: u32,
        /// The removed item's bytes, as the system chunk held them.
        old_item: Vec<u8>,
    },
    /// `LOG_MODIFY_RECORD` — a phrase item whose content changed.
    Modify {
        /// The record's token.
        token: u32,
        /// The system item's bytes.
        old_item: Vec<u8>,
        /// The current item's bytes.
        new_item: Vec<u8>,
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

/// `sizeof(LOG_TYPE)` — the unscoped enum's underlying `int`.
const LOG_TYPE_SIZE: usize = 4;

/// A malformed `PhraseIndexLogger` record stream.
#[derive(Debug)]
pub struct LogDecodeError(String);

impl std::fmt::Display for LogDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "phrase index logger: {}", self.0)
    }
}

impl std::error::Error for LogDecodeError {}

/// Decodes a record stream (`PhraseIndexLogger::next_record`'s loop).
///
/// # Errors
///
/// Fails on an invalid record type, a truncated payload, or a
/// `MODIFY_HEADER` record whose token is not `null_token`.
pub fn decode_log_records(bytes: &[u8]) -> Result<Vec<LogRecord>, LogDecodeError> {
    let mut records = Vec::new();
    let mut offset = 0_usize;

    let take_u16 = |offset: &mut usize| -> Result<u16, LogDecodeError> {
        let lo = *offset;
        let hi = lo + 2;
        if bytes.len() < hi {
            return Err(LogDecodeError(format!("truncated u16 at {lo}")));
        }
        let value = u16::from_le_bytes([bytes[lo], bytes[lo + 1]]);
        *offset = hi;
        Ok(value)
    };
    let take_bytes = |offset: &mut usize, len: u16| -> Result<&[u8], LogDecodeError> {
        let start = *offset;
        let end = start + usize::from(len);
        if bytes.len() < end {
            return Err(LogDecodeError(format!(
                "truncated {}-byte payload at {start}",
                usize::from(len)
            )));
        }
        *offset = end;
        Ok(&bytes[start..end])
    };

    while offset < bytes.len() {
        if bytes.len() < offset + LOG_TYPE_SIZE + 4 {
            return Err(LogDecodeError(format!("truncated record head at {offset}")));
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
                let new_item = take_bytes(&mut offset, len)?.to_vec();
                LogRecord::Add { token, new_item }
            }
            2 => {
                let len = take_u16(&mut offset)?;
                let old_item = take_bytes(&mut offset, len)?.to_vec();
                LogRecord::Remove { token, old_item }
            }
            3 => {
                let old_len = take_u16(&mut offset)?;
                let new_len = take_u16(&mut offset)?;
                let old_item = take_bytes(&mut offset, old_len)?.to_vec();
                let new_item = take_bytes(&mut offset, new_len)?.to_vec();
                LogRecord::Modify {
                    token,
                    old_item,
                    new_item,
                }
            }
            4 => {
                if token != 0 {
                    return Err(LogDecodeError(format!(
                        "MODIFY_HEADER record carries token {token:#010x}"
                    )));
                }
                let len = take_u16(&mut offset)?;
                if usize::from(len) < 4 {
                    return Err(LogDecodeError(
                        "MODIFY_HEADER payload has no total".to_owned(),
                    ));
                }
                // Two consecutive `len`-byte runs: the old totals, then
                // the new — `next_record` hands one run to each payload.
                let old_run = take_bytes(&mut offset, len)?;
                if bytes.len() < offset + usize::from(len) {
                    return Err(LogDecodeError(
                        "MODIFY_HEADER has no new-total run".to_owned(),
                    ));
                }
                let new_run = take_bytes(&mut offset, len)?;
                let old_total =
                    u32::from_le_bytes([old_run[0], old_run[1], old_run[2], old_run[3]]);
                let new_total =
                    u32::from_le_bytes([new_run[0], new_run[1], new_run[2], new_run[3]]);
                LogRecord::ModifyHeader {
                    old_total,
                    new_total,
                }
            }
            other => {
                return Err(LogDecodeError(format!(
                    "invalid record type {other} at {offset}"
                )));
            }
        };
        records.push(record);
    }

    Ok(records)
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
    fn user_dbm_names_follow_the_backend() {
        if DEFAULT_STORE_IS_LIBPINYIN_DBM {
            assert_eq!(UserDbm::Bigram.file_name(), "user_bigram.db");
            assert_eq!(UserDbm::PinyinIndex.file_name(), "user_pinyin_index.bin");
            assert_eq!(UserDbm::PhraseIndex.file_name(), "user_phrase_index.bin");
        } else {
            assert_eq!(
                UserDbm::Bigram.file_name(),
                format!("user_bigram.{DEFAULT_STORE_EXT}")
            );
        }
        assert!(UserDbm::Bigram.is_hash());
        assert!(!UserDbm::PinyinIndex.is_hash());
        assert_eq!(USER_LIBRARY_FILES[2], (7, "user.bin"));
        assert_eq!(SYSTEM_LOG_FILES[0], (1, "gb_char.dbin"));
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
        let parsed = UserTableInfo::parse(&text).expect("parse");
        assert_eq!(parsed, conform);

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
        assert!(other.known_database_format());
        // A token no upstream build ever emits — a permanent negative
        // case for the known-database-format check.
        assert!(!conform_with("NotADbmLibrary").known_database_format());

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

        // The table.conf reader takes the pin's values when the file
        // is silent, and the declared ones when it speaks.
        assert_eq!(
            SystemVersions::from_table_conf("lambda parameter:0.312699\n"),
            SystemVersions::for_this_build(7, 14)
        );
        assert_eq!(
            SystemVersions::from_table_conf(
                "binary format version:9\nmodel data version:20\nlambda parameter:1\n"
            ),
            SystemVersions::for_this_build(9, 20)
        );

        // Absent optional lines default, as upstream's fscanf tolerates.
        let sparse = UserTableInfo::parse("binary format version:7\nmodel data version:14\n")
            .expect("parse");
        assert_eq!(sparse.database_format, None);
        assert_eq!(sparse.open_counter, 0);
        assert!(!sparse.is_conform(&versions));
        assert!(UserTableInfo::parse("binary format version:x\n").is_err());
        assert!(UserTableInfo::parse("").is_err());
    }

    /// What each counter line reads as, per glibc's
    /// `fscanf("open counter:%d\n")` in debian:testing (glibc 2.43), which
    /// the seeded protocol of `tools/bisection/run-open-counter-diff.sh`
    /// checks against the pin.
    #[test]
    fn the_counter_reads_as_fscanfs_percent_d() {
        let head = "binary format version:7\nmodel data version:14\ndatabase format:Tkrzw\n";
        for (tail, counter) in [
            ("open counter:5\n", 5),
            ("open counter:-3\n", -3),
            ("open counter:+3\n", 3),
            ("open counter:+7\n", 7),
            ("open counter:3x\n", 3),
            ("open counter:7x\n", 7),
            ("open counter:3\u{fffd}\n", 3),
            ("open counter: 5\n", 5),
            ("open counter:\t5\n", 5),
            ("open counter:\u{b}5\n", 5),
            ("open counter:\n5\n", 5),
            ("open counter:\r\n5\r\n", 5),
            ("  open counter:5\n", 5),
            ("\nopen counter:5\n", 5),
            ("open   counter:5\n", 5),
            ("opencounter:5\n", 5),
            ("open counter:2147483647\n", i32::MAX),
            ("open counter:2147483648\n", i32::MIN),
            ("open counter:4294967303\n", 7),
            ("open counter:99999999999999999999\n", -1),
            ("open counter:-99999999999999999999\n", 0),
            ("open counter:-2147483649\n", i32::MAX),
            ("open counter:-9223372036854775808\n", 0),
            ("open counter:9223372036854775807\n", -1),
            ("open counter:-0\n", 0),
            ("open counter:\n", 0),
            ("open counter:", 0),
            ("open counter:-\n", 0),
            ("open counter:- 5\n", 0),
            ("open counter:x5\n", 0),
            ("", 0),
            // One conversion only: an empty value reads on, but not into a
            // second counter line.
            ("open counter:\nopen counter:5\n", 0),
            ("open counter:4\nopen counter:5\n", 4),
        ] {
            let info = UserTableInfo::parse(&format!("{head}{tail}")).expect("parse");
            assert_eq!(info.open_counter, counter, "{tail:?}");
        }
    }

    /// Discovery walks the file once: a long run of blank, white-space
    /// and irrelevant lines before the counter, and one with no counter
    /// at all, parse without each line resuming the scan its predecessor
    /// already spent. The run lengths are long enough that the per-line
    /// re-scan this replaced would multiply out to a visible stall.
    #[test]
    fn a_long_blank_run_reaches_the_counter_once() {
        let head = "binary format version:7\nmodel data version:14\ndatabase format:Tkrzw\n";
        let blanks = "\u{b}\t \n".repeat(20_000);
        let junk = "not a counter line\n".repeat(20_000);
        let cases = [
            (format!("{head}{blanks}open counter:5\n"), 5),
            // The literal's own leading skip spans the blank run.
            (format!("{head}{blanks}  open counter:-3\n"), -3),
            // White space after the colon reads on across the blank run.
            (format!("{head}{blanks}open counter:\n{blanks}7\n"), 7),
            (format!("{head}{junk}open counter:9\n"), 9),
            (format!("{head}{junk}{blanks}open counter: 4\n"), 4),
            // Never a counter: the walk still ends in one pass, at 0.
            (format!("{head}{junk}{blanks}"), 0),
        ];
        for (text, counter) in cases {
            let info = UserTableInfo::parse(&text).expect("parse");
            assert_eq!(info.open_counter, counter);
        }
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
            UserTableInfo::parse(&negative.to_text()).expect("parse"),
            negative
        );
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
        bad_header[7] = 1; // corrupt the token's low byte
        assert!(decode_log_records(&bad_header).is_err());
    }
}
