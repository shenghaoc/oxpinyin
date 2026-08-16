//! Read-only dump of a legacy libpinyin user directory via the C++ storage
//! shim (`storage_dump.cpp`).
//!
//! The shim links libpinyin's own `libstorage.a` and reads `user.bin` (phrase
//! index) plus `user_bigram.db` (bigram) through the very classes that wrote
//! them, so no binary layout is reimplemented here and no DBM-backend
//! variation is decoded. It returns a flat little-endian byte buffer; this
//! module copies that buffer into owned Rust values and parses it into the
//! typed [`LegacyDump`]. All parsing is bounds-checked and returns `Err`
//! rather than panicking (constitution §4).

use std::ffi::{CStr, CString, c_char, c_int};
use std::path::Path;

/// A parsed legacy user directory: every phrase and bigram the shim read,
/// plus the iteration counters it reported.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LegacyDump {
    /// Number of token slots iterated (`range_end - range_begin`).
    pub tokens_iterated: u32,
    /// Slots whose phrase item was deallocated (`ERROR_NO_ITEM`).
    pub null_slot_count: u32,
    /// Items present but unreadable, and bigram predecessors that failed to
    /// load. These are skipped; the shim reports them so a corrupt legacy
    /// store is auditable rather than silently half-imported.
    pub corrupt_count: u32,
    /// Phrase records in token order.
    pub phrases: Vec<LegacyPhrase>,
    /// Bigram records, one per predecessor, ascending by predecessor token.
    pub bigrams: Vec<LegacyBigram>,
}

/// One legacy user phrase: token, verbatim unigram frequency, text, and its
/// pronunciations (pinyin strings still carrying the tone-digit suffix).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LegacyPhrase {
    /// Full 32-bit phrase token (library nibble `USER_DICTIONARY`).
    pub token: u32,
    /// `PhraseItem::get_unigram_frequency()` verbatim.
    pub unigram_freq: u32,
    /// Phrase text (UTF-8).
    pub text: String,
    /// Pronunciations in stored order.
    pub pronunciations: Vec<LegacyPronunciation>,
}

/// One pronunciation: the per-syllable pinyin spellings and its frequency.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LegacyPronunciation {
    /// Per-syllable pinyin strings (e.g. `ni3`, zero tone plain `ni`).
    pub syllables: Vec<String>,
    /// `get_nth_pronunciation`'s frequency, verbatim.
    pub freq: u32,
}

/// One bigram predecessor: verbatim per-predecessor total and its successors.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LegacyBigram {
    /// Predecessor token, verbatim (`sentence_start`, system, or user token).
    pub prev: u32,
    /// `SingleGram::get_total_freq()` verbatim.
    pub total_freq: u32,
    /// Successor `(token, count)` pairs in stored order.
    pub successors: Vec<(u32, u32)>,
}

unsafe extern "C" {
    /// Dumps `user_bin` + `user_bigram_db` into a malloc'd buffer. Returns
    /// `0` on success (buffer described by the out-pointers, freed by
    /// `legacy_dump_free`), `-1` on failure with a NUL-terminated message in
    /// `err`.
    fn legacy_dump(
        user_bin: *const c_char,
        user_bigram_db: *const c_char,
        out_buf: *mut *mut u8,
        out_len: *mut usize,
        err: *mut c_char,
        err_len: usize,
    ) -> c_int;

    /// Frees a buffer returned by `legacy_dump`.
    fn legacy_dump_free(buf: *mut u8);
}

/// Dumps the legacy user directory at `legacy_dir` (expects `user.bin` and
/// `user_bigram.db` directly inside it) and parses the result.
pub fn dump(legacy_dir: &Path) -> Result<LegacyDump, String> {
    let user_bin = path_cstring(&legacy_dir.join("user.bin"))?;
    let user_bigram_db = path_cstring(&legacy_dir.join("user_bigram.db"))?;

    let mut out_buf: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;
    let mut err_buf = [0_u8; 4096];

    // SAFETY: all pointers are live and correctly typed for the duration of
    // the call. `legacy_dump` writes at most `err_len` bytes into `err_buf`
    // (NUL-terminated) and either nulls or fills the out-pointers; both
    // conditions are checked before anything is read back.
    let ret = unsafe {
        legacy_dump(
            user_bin.as_ptr(),
            user_bigram_db.as_ptr(),
            &mut out_buf,
            &mut out_len,
            err_buf.as_mut_ptr().cast::<c_char>(),
            err_buf.len(),
        )
    };

    if ret != 0 {
        // On failure the shim guarantees a NUL-terminated message in
        // `err_buf`; `from_bytes_until_nul` stops at the first NUL and cannot
        // over-read the fixed-size array.
        let message = CStr::from_bytes_until_nul(&err_buf)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown error (no message)".to_owned());
        return Err(format!("legacy dump failed: {message}"));
    }

    if out_buf.is_null() || out_len == 0 {
        return Err("legacy dump returned an empty buffer".into());
    }

    // SAFETY: `out_buf` points to at least `out_len` readable bytes (both
    // set by a successful `legacy_dump`). The slice is copied to an owned
    // Vec before `legacy_dump_free` is called, so the buffer may be freed
    // immediately after.
    let bytes = unsafe { std::slice::from_raw_parts(out_buf, out_len) }.to_vec();
    // SAFETY: `out_buf` is non-null and was returned by `legacy_dump`; freed
    // exactly once here, and never used again.
    unsafe { legacy_dump_free(out_buf) };

    parse(&bytes)
}

fn path_cstring(path: &Path) -> Result<CString, String> {
    CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| {
        format!("path contains a NUL byte: {}", path.display())
    })
}

/// Little-endian cursor with bounds-checked reads; every method returns `Err`
/// on truncation or overflow instead of panicking.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn u32(&mut self) -> Result<u32, String> {
        let end = self
            .pos
            .checked_add(4)
            .ok_or_else(|| "dump buffer: length overflow".to_owned())?;
        let bytes = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| "dump buffer: truncated u32".to_owned())?;
        self.pos = end;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| "dump buffer: length overflow".to_owned())?;
        let bytes = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| "dump buffer: truncated byte run".to_owned())?;
        self.pos = end;
        Ok(bytes)
    }

    fn utf8(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        let bytes = self.bytes(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| "dump buffer: invalid UTF-8 in string field".to_owned())
    }
}

fn parse(buf: &[u8]) -> Result<LegacyDump, String> {
    let mut c = Cursor { buf, pos: 0 };

    if c.bytes(4)? != b"MIGR" {
        return Err("dump buffer: bad magic".into());
    }
    let version = c.u32()?;
    if version != 1 {
        return Err(format!("dump buffer: unsupported version {version}"));
    }
    let tokens_iterated = c.u32()?;
    let null_slot_count = c.u32()?;
    let corrupt_count = c.u32()?;
    let phrase_count = c.u32()?;
    let bigram_count = c.u32()?;

    let mut phrases = Vec::with_capacity(phrase_count as usize);
    for _ in 0..phrase_count {
        let token = c.u32()?;
        let unigram_freq = c.u32()?;
        let text = c.utf8()?;
        let n_pronunciations = c.u32()?;
        let mut pronunciations = Vec::with_capacity(n_pronunciations as usize);
        for _ in 0..n_pronunciations {
            let n_syllables = c.u32()?;
            let mut syllables = Vec::with_capacity(n_syllables as usize);
            for _ in 0..n_syllables {
                syllables.push(c.utf8()?);
            }
            let freq = c.u32()?;
            pronunciations.push(LegacyPronunciation { syllables, freq });
        }
        phrases.push(LegacyPhrase {
            token,
            unigram_freq,
            text,
            pronunciations,
        });
    }

    let mut bigrams = Vec::with_capacity(bigram_count as usize);
    for _ in 0..bigram_count {
        let prev = c.u32()?;
        let total_freq = c.u32()?;
        let n_successors = c.u32()?;
        let mut successors = Vec::with_capacity(n_successors as usize);
        for _ in 0..n_successors {
            let cur = c.u32()?;
            let count = c.u32()?;
            successors.push((cur, count));
        }
        bigrams.push(LegacyBigram {
            prev,
            total_freq,
            successors,
        });
    }

    if c.pos != buf.len() {
        return Err(format!(
            "dump buffer: {} trailing bytes after last record",
            buf.len() - c.pos
        ));
    }

    Ok(LegacyDump {
        tokens_iterated,
        null_slot_count,
        corrupt_count,
        phrases,
        bigrams,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    #[test]
    fn parses_a_minimal_dump() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"MIGR");
        buf.extend_from_slice(&u32_le(1)); // version
        buf.extend_from_slice(&u32_le(3)); // tokens_iterated
        buf.extend_from_slice(&u32_le(1)); // null_slot_count
        buf.extend_from_slice(&u32_le(0)); // corrupt_count
        buf.extend_from_slice(&u32_le(1)); // phrase_count
        buf.extend_from_slice(&u32_le(0)); // bigram_count

        // phrase: token=0x07000001, unigram=7, text="你好", 1 pronunciation
        buf.extend_from_slice(&u32_le(0x0700_0001));
        buf.extend_from_slice(&u32_le(7));
        let text = "你好".as_bytes();
        buf.extend_from_slice(&u32_le(text.len() as u32));
        buf.extend_from_slice(text);
        buf.extend_from_slice(&u32_le(1)); // n_pronunciations
        buf.extend_from_slice(&u32_le(2)); // n_syllables
        for s in ["ni3", "hao3"] {
            let b = s.as_bytes();
            buf.extend_from_slice(&u32_le(b.len() as u32));
            buf.extend_from_slice(b);
        }
        buf.extend_from_slice(&u32_le(5)); // freq

        let dump = parse(&buf).unwrap();
        assert_eq!(dump.tokens_iterated, 3);
        assert_eq!(dump.null_slot_count, 1);
        assert_eq!(dump.corrupt_count, 0);
        assert_eq!(dump.phrases.len(), 1);
        assert_eq!(dump.phrases[0].token, 0x0700_0001);
        assert_eq!(dump.phrases[0].unigram_freq, 7);
        assert_eq!(dump.phrases[0].text, "你好");
        assert_eq!(dump.phrases[0].pronunciations.len(), 1);
        assert_eq!(
            dump.phrases[0].pronunciations[0].syllables,
            vec!["ni3".to_owned(), "hao3".to_owned()]
        );
        assert_eq!(dump.phrases[0].pronunciations[0].freq, 5);
        assert!(dump.bigrams.is_empty());
    }

    #[test]
    fn rejects_truncated_input() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"MIGR");
        buf.extend_from_slice(&u32_le(1));
        buf.extend_from_slice(&u32_le(1));
        buf.extend_from_slice(&u32_le(0));
        buf.extend_from_slice(&u32_le(0));
        // missing phrase_count and bigram_count
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"NOPE");
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"MIGR");
        buf.extend_from_slice(&u32_le(1));
        buf.extend_from_slice(&u32_le(0));
        buf.extend_from_slice(&u32_le(0));
        buf.extend_from_slice(&u32_le(0));
        buf.extend_from_slice(&u32_le(0));
        buf.extend_from_slice(&u32_le(0));
        buf.extend_from_slice(&[0xAA]); // trailing
        assert!(parse(&buf).is_err());
    }
}
