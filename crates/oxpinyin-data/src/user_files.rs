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
use crate::table_info::FileName;
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
/// The file is read as upstream's four `fscanf` calls read it, not as
/// lines; [`UserTableInfo::parse`] is that sequence. The first two
/// directives are required, `database format:` and `open counter:` fall
/// back (`UNKNOWN`/0) where upstream's calls do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserTableInfo {
    /// `binary format version:`.
    pub binary_format_version: u32,
    /// `model data version:`.
    pub model_data_version: u32,
    /// `database format:` — `None` when the third directive ran off the
    /// end of the file before it converted anything (`UNKNOWN_FORMAT`).
    /// Upstream recognises exactly [`UPSTREAM_DB_FORMATS`]; any other
    /// token reaches `to_table_database_format_type`'s `abort()`, which
    /// [`UserTableInfo::parse`] answers with
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

    /// Parses `user.conf` bytes the way upstream's `UserTableInfo::load`
    /// reads them: as the four `fscanf` calls of `table_info.cpp:338-359`
    /// (`074a2219`), over one stream, each directive matching from where
    /// the previous one stopped.
    ///
    /// ```c
    /// fscanf(input, "binary format version:%d\n", &binver);   // :338
    /// fscanf(input, "model data version:%d\n", &modelver);    // :344
    /// fscanf(input, "database format:%255s\n", str);          // :352
    /// fscanf(input, "open counter:%d\n", &counter);           // :357
    /// ```
    ///
    /// A white-space byte in a format matches any run of C white space, an
    /// empty one included, and any other byte matches only itself
    /// (C11 7.21.6.2p5-6). So `%d` takes an optional sign and every digit
    /// after it, and `%255s` skips white space — newlines included — and
    /// reads at most 255 bytes, stopping at the first white-space byte.
    /// A call that fails its directive leaves the stream where it stopped
    /// and the next call carries on from there, which is why a foreign
    /// line between two directives is what the following `%d` reads.
    ///
    /// The three fallbacks are upstream's. A version directive that does
    /// not convert is the load's `false` (`:339-342`, `:345-348`): the
    /// marker reads as absent ([`UserConfError::Line`]). A counter
    /// directive that does not convert reads 0 (`:358-359`). The
    /// `database format:` directive ends at
    /// `to_table_database_format_type` (`:353-354`), which `abort()`s on
    /// any token it does not know (`:122-133`) — and which is also called
    /// when the directive *failed*, on a `str` no conversion wrote
    /// (`:351`). Both are [`UserConfError::UnknownDatabaseFormat`], the
    /// class-(c) answer to upstream's abort; the one outcome that is not
    /// an abort is an input failure before the conversion, where upstream
    /// leaves `format` at `UNKNOWN_FORMAT` and the file is simply
    /// non-conforming.
    ///
    /// # Errors
    ///
    /// [`UserConfError::Line`] when a version directive does not convert;
    /// [`UserConfError::UnknownDatabaseFormat`] where upstream's
    /// `to_table_database_format_type` aborts.
    pub fn parse(bytes: &[u8]) -> Result<Self, UserConfError> {
        let mut scan = Scan::new(bytes);

        scan.literal(b"binary format version:")
            .map_err(|_| UserConfError::Line("binary format version"))?;
        let binary_format_version = scan
            .int()
            .ok_or(UserConfError::Line("binary format version"))?;
        scan.skip_space(); // the format's trailing `\n`

        scan.literal(b"model data version:")
            .map_err(|_| UserConfError::Line("model data version"))?;
        let model_data_version = scan
            .int()
            .ok_or(UserConfError::Line("model data version"))?;
        scan.skip_space();

        let database_format = match scan.literal(b"database format:") {
            // `if (EOF != num) format = to_...(str)`: the literal ran off
            // the end of the file, so `format` keeps UNKNOWN_FORMAT.
            Err(Call::Eof) => None,
            // The literal mismatched: `str` was never written, and
            // upstream maps it anyway.
            Err(Call::Mismatch) => return Err(UserConfError::UnknownDatabaseFormat),
            Ok(()) => match scan.token(255) {
                Err(Call::Eof) => None,
                Err(Call::Mismatch) => return Err(UserConfError::UnknownDatabaseFormat),
                Ok(token) => Some(
                    upstream_db_format(token)
                        .ok_or(UserConfError::UnknownDatabaseFormat)?
                        .to_owned(),
                ),
            },
        };
        scan.skip_space();

        // The format's trailing `\n`, then
        // `if (1 != num) counter = 0` — a failed literal or conversion,
        // and an empty value running into the next line, all read 0.
        let open_counter = match scan.literal(b"open counter:") {
            Ok(()) => scan.int().unwrap_or(0),
            Err(_) => 0,
        };

        // Upstream's fields are `int`; these are `u32` holding the same
        // low 32 bits (`%d`'s store through `int *` is that bit
        // pattern). The system's own versions are positive constants, so
        // `is_conform`'s comparison is the signed one upstream makes.
        Ok(Self {
            binary_format_version: binary_format_version.cast_unsigned(),
            model_data_version: model_data_version.cast_unsigned(),
            database_format,
            open_counter,
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
}

/// What upstream makes of a `user.conf` its `load` cannot complete.
#[derive(Debug)]
pub enum UserConfError {
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

/// C's `isspace` over ASCII — the white space a `scanf` directive or
/// conversion skips: space, `\t`, `\n`, `\v`, `\f` and `\r`.
/// [`u8::is_ascii_whitespace`] leaves out `\v`.
const fn is_c_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The `database format:` token upstream recognises, per
/// `to_table_database_format_type` (`table_info.cpp:122-131`); `None` is
/// the token that function `abort()`s on, which [`UserTableInfo::parse`]
/// answers with [`UserConfError::UnknownDatabaseFormat`].
fn upstream_db_format(token: &[u8]) -> Option<&'static str> {
    UPSTREAM_DB_FORMATS
        .into_iter()
        .find(|known| known.as_bytes() == token)
}

/// How a directive fails, where `load` can tell the outcomes apart
/// (C11 7.21.6.2p16). A directive that runs is `Ok`, and the stream then
/// sits where it stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Call {
    /// A matching failure: the literal or the conversion did not match.
    /// No conversion is reported.
    Mismatch,
    /// An input failure before any conversion — the format ran off the
    /// end of the file. `fscanf` returns `EOF`, which the
    /// `database format:` directive alone distinguishes from a mismatch
    /// (`table_info.cpp:353`).
    Eof,
}

/// The stream upstream's four `fscanf` calls share: a byte slice and a
/// position, each directive consuming exactly what glibc's would.
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Scan<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.at..).unwrap_or_default()
    }

    /// A white-space directive in a format: it matches any run of C white
    /// space, an empty one included, and the end of the file is not a
    /// failure for it.
    fn skip_space(&mut self) {
        let skip = self
            .rest()
            .iter()
            .position(|&byte| !is_c_space(byte))
            .unwrap_or(self.rest().len());
        self.at = self.at.saturating_add(skip);
    }

    /// A format's literal run, up to its first conversion: white-space
    /// bytes match as [`Self::skip_space`] does, every other byte must
    /// match the stream exactly. `Err(Call::Eof)` when the stream ends
    /// first.
    fn literal(&mut self, format: &[u8]) -> Result<(), Call> {
        for &byte in format {
            if is_c_space(byte) {
                self.skip_space();
                continue;
            }
            let Some(&first) = self.rest().first() else {
                return Err(Call::Eof);
            };
            if first != byte {
                return Err(Call::Mismatch);
            }
            self.at = self.at.saturating_add(1);
        }
        Ok(())
    }

    /// `%d`: white space, an optional sign, then every decimal digit.
    /// glibc converts with `strtol`, which saturates at `long`'s range
    /// (64 bits on the pin's LP64 builds), and stores through an `int *`,
    /// which keeps the low 32 bits — so `2147483648` reads as
    /// −2147483648 and `4294967303` as 7. `None` is a failed conversion:
    /// no digit after the optional sign.
    fn int(&mut self) -> Option<i32> {
        self.skip_space();
        let (negative, digits) = match self.rest().split_first() {
            Some((&b'-', tail)) => (true, tail),
            Some((&b'+', tail)) => (false, tail),
            _ => (false, self.rest()),
        };
        // `strtol` accumulates toward the sign, so `LONG_MIN` is
        // reachable; `None` once the value has left `long`'s range.
        let mut long = Some(0_i64);
        let mut any_digit = false;
        let mut taken = 0_usize;
        for &byte in digits.iter().take_while(|byte| byte.is_ascii_digit()) {
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
            taken += 1;
        }
        if !any_digit {
            return None;
        }
        // The sign, the digits, and nothing else: the stream sits at the
        // first byte the conversion did not take.
        let sign = usize::from(self.rest().first().is_some_and(|&b| b == b'+' || b == b'-'));
        self.at = self.at.saturating_add(sign + taken);
        let long = long.unwrap_or(if negative { i64::MIN } else { i64::MAX });
        // The store through `int *`: the low 32 bits.
        Some(long as i32)
    }

    /// `%<width>s`: white space, then at most `width` non-white-space
    /// bytes. `Err(Call::Eof)` when the stream ends before a byte can be
    /// taken — the conversion never runs, whatever the width.
    fn token(&mut self, width: usize) -> Result<&'a [u8], Call> {
        self.skip_space();
        if self.rest().is_empty() {
            return Err(Call::Eof);
        }
        let take = self
            .rest()
            .iter()
            .take(width)
            .position(|&byte| is_c_space(byte))
            .unwrap_or_else(|| self.rest().len().min(width));
        let token = self.rest().get(..take).unwrap_or_default();
        self.at = self.at.saturating_add(take);
        Ok(token)
    }
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
        let parsed = UserTableInfo::parse(text.as_bytes()).expect("parse");
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
        // A token no upstream build ever emits never conforms — and is
        // not even parsable any more: the `database format:` directive
        // refuses it where upstream's mapper aborts (#590).
        assert!(!conform_with("NotADbmLibrary").is_conform(&versions));
        assert!(matches!(
            UserTableInfo::parse(
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
        let declared = crate::table_info::TableConf::parse(
            b"binary format version:9\nmodel data version:20\nlambda parameter:1\n\
              source table format:pinyin\ndatabase format:Tkrzw\n",
        )
        .expect("parses");
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
        let sparse = UserTableInfo::parse(b"binary format version:7\nmodel data version:14\n")
            .expect("parse");
        assert_eq!(sparse.database_format, None);
        assert_eq!(sparse.open_counter, 0);
        assert!(!sparse.is_conform(&versions));
        assert!(matches!(
            UserTableInfo::parse(b"binary format version:x\n"),
            Err(UserConfError::Line("binary format version"))
        ));
        assert!(matches!(
            UserTableInfo::parse(b""),
            Err(UserConfError::Line("binary format version"))
        ));
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
            let info = UserTableInfo::parse(format!("{head}{tail}").as_bytes()).expect("parse");
            assert_eq!(info.open_counter, counter, "{tail:?}");
        }
    }

    /// The version directives are `fscanf` calls, not a line model: `%d`
    /// skips white space, takes an optional sign, reads every digit,
    /// saturates at `long` and stores the low 32 bits. Every value below
    /// was executed against the pin-built library (glibc 2.43) and the
    /// pin's own `table_info.cpp` at `074a2219`.
    #[test]
    fn the_version_directives_read_as_fscanfs_percent_d() {
        let tail = "\ndatabase format:Tkrzw\nopen counter:5\n";
        let parse2 = |binver: &str, modelver: &str| {
            UserTableInfo::parse(
                format!("binary format version:{binver}\nmodel data version:{modelver}{tail}")
                    .as_bytes(),
            )
        };
        for (binver, modelver, expected) in [
            ("7", "14", (7_u32, 14_u32)),
            ("+7", "14", (7, 14)),
            ("4294967303", "14", (7, 14)),
            (" 7", "14", (7, 14)),
            ("07", "14", (7, 14)),
            ("7", "+14", (7, 14)),
            ("7", "4294967310", (7, 14)),
            ("-1", "14", ((-1_i32).cast_unsigned(), 14)),
            ("2147483655", "14", ((-2147483641_i32).cast_unsigned(), 14)),
            ("-2147483649", "14", (i32::MAX.cast_unsigned(), 14)),
            ("7", "-1", (7, (-1_i32).cast_unsigned())),
        ] {
            let info = parse2(binver, modelver).expect("parse");
            assert_eq!(
                (info.binary_format_version, info.model_data_version),
                expected,
                "{binver:?} {modelver:?}"
            );
        }
        // Junk after a version line is what the *next* directive meets:
        // after the first line it is that directive's matching failure,
        // after the second it reaches the database-format abort.
        assert!(matches!(
            parse2("7x", "14"),
            Err(UserConfError::Line("model data version"))
        ));
        assert!(matches!(
            parse2("7", "14x"),
            Err(UserConfError::UnknownDatabaseFormat)
        ));
        // A reordered file fails the first literal.
        assert!(matches!(
            UserTableInfo::parse(b"model data version:14\nbinary format version:7\n"),
            Err(UserConfError::Line("binary format version"))
        ));
    }

    /// The `database format:` directive, with upstream's
    /// `if (EOF != num) format = to_table_database_format_type (str);`
    /// behind it (`table_info.cpp:353-354`): a token, `UNKNOWN_FORMAT`
    /// when the call ran off the end of the file, and the class-(c)
    /// `Err` where upstream's mapper `abort()`s — on an unrecognised
    /// token, and on the matching failure that leaves its `str`
    /// unwritten.
    #[test]
    fn the_database_format_directive_maps_or_refuses() {
        let head = "binary format version:7\nmodel data version:14\n";
        let ok = |body: &str| UserTableInfo::parse(format!("{head}{body}").as_bytes());
        assert_eq!(
            ok("database format:Tkrzw\nopen counter:5\n")
                .expect("parse")
                .database_format,
            Some("Tkrzw".to_owned())
        );
        // `%255s` skips white space first, so a blank after the colon is
        // not part of the token.
        assert_eq!(
            ok("database format: Tkrzw\nopen counter:5\n")
                .expect("parse")
                .database_format,
            Some("Tkrzw".to_owned())
        );
        // Any recognised token parses; conformance is a separate check.
        assert_eq!(
            ok("database format:KyotoCabinet\nopen counter:5\n")
                .expect("parse")
                .database_format,
            Some("KyotoCabinet".to_owned())
        );
        // The conversion stops at white space: the trailing `extra` is
        // what the counter directive then meets.
        let tailed = ok("database format:Tkrzw extra\nopen counter:5\n").expect("parse");
        assert_eq!(tailed.database_format, Some("Tkrzw".to_owned()));
        assert_eq!(tailed.open_counter, 0);

        // The one outcome that is not the abort: the call ran off the end
        // of the file, so `format` keeps UNKNOWN_FORMAT (`:350`).
        let sparse = ok("").expect("parse");
        assert_eq!(sparse.database_format, None);
        assert_eq!(sparse.open_counter, 0);
        assert_eq!(ok("database format:").expect("parse").database_format, None);
        assert_eq!(
            UserTableInfo::parse(b"binary format version:7\nmodel data version:14\n")
                .expect("parse")
                .database_format,
            None
        );

        // The abort point, in each shape executed against the pin.
        let long = format!("database format:{}\nopen counter:5\n", "A".repeat(300));
        let trunc = format!("database format:{}open counter:5\n", "A".repeat(255));
        for body in [
            // A token no upstream build knows.
            "database format:NotADbmLibrary\nopen counter:5\n".to_owned(),
            // 300 bytes: `%255s` takes 255 and maps those.
            long,
            // 255 bytes, with the counter on the same line.
            trunc,
            // Nothing after the colon: `%255s` reads the *next* word,
            // across the newline, and `open` is no format either.
            "database format:\nopen counter:5\n".to_owned(),
            // No format directive at all: the literal mismatches, and
            // upstream maps the `str` no conversion wrote.
            "open counter:5\n".to_owned(),
        ] {
            assert!(
                matches!(ok(&body), Err(UserConfError::UnknownDatabaseFormat)),
                "{body:?}"
            );
        }
    }

    /// The counter directive reads from where the `database format:` call
    /// stopped — one call, one attempt at its literal — and a failed
    /// conversion reads 0 (`table_info.cpp:358-359`).
    #[test]
    fn the_counter_directive_reads_where_the_token_left_it() {
        let head = "binary format version:7\nmodel data version:14\ndatabase format:Tkrzw\n";
        let ok =
            |body: &str| UserTableInfo::parse(format!("{head}{body}").as_bytes()).expect("parse");
        assert_eq!(ok("open counter:5\n").open_counter, 5);
        // A foreign line between the token and the counter is what the
        // literal meets: 0, not the counter line below it.
        assert_eq!(ok("junk line\nopen counter:5\n").open_counter, 0);
        // A missing trailing newline is not a failure: the conversion has
        // already run when the whitespace directive sees the end of the
        // file.
        assert_eq!(ok("open counter:5").open_counter, 5);
        assert_eq!(ok("open counter:").open_counter, 0);
    }

    /// The scan is one pass over the bytes: a long run of white space is
    /// spent once by the directive that skipped it, never re-walked per
    /// line, and a long run of irrelevant text is never rescanned. The
    /// run lengths are long enough that a per-line re-scan would multiply
    /// out to a visible stall.
    #[test]
    fn a_long_run_is_walked_once() {
        let head = "binary format version:7\nmodel data version:14\ndatabase format:Tkrzw\n";
        let blanks = "\u{b}\t \n".repeat(20_000);
        let junk = "not a counter line\n".repeat(20_000);
        let cases = [
            (format!("{head}{blanks}open counter:5\n"), 5),
            // The literal's own leading skip spans the blank run.
            (format!("{head}{blanks}  open counter:-3\n"), -3),
            // White space after the colon reads on across the blank run.
            (format!("{head}{blanks}open counter:\n{blanks}7\n"), 7),
            // Anything ahead of the counter literal stops it: one call
            // matches once, at the byte where the previous one stopped.
            (format!("{head}{junk}open counter:9\n"), 0),
            (format!("{head}{junk}{blanks}open counter: 4\n"), 0),
            // Never a counter: the walk still ends in one pass, at 0.
            (format!("{head}{junk}{blanks}"), 0),
        ];
        for (text, counter) in cases {
            let info = UserTableInfo::parse(text.as_bytes()).expect("parse");
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
            UserTableInfo::parse(negative.to_text().as_bytes()).expect("parse"),
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
