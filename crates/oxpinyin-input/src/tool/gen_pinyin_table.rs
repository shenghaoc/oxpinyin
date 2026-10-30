//! `gen_pinyin_table`'s own calls at its edge
//! (`utils/storage/gen_pinyin_table.cpp` at 074a2219): the input `FILE *`
//! and its `fscanf`, the GLib calls on the scanned phrase, and its
//! formatted lines. What the scanned words mean, the phrase tree and the
//! output order belong to the utility.

use std::ffi::{CStr, c_int, c_long};

use super::OutputStream;
use crate::native_tool::{self, Input, PINYIN_TABLE_FIELD};

/// One input file of `feed_file` (`:134-157`) and its `char phrase[1024],
/// pinyin[1024]` and `guint32 freq` locals, which keep their bytes from one
/// `fscanf` to the next as the pin's do. They start zeroed where the pin's
/// start indeterminate.
pub struct TableInput {
    stream: Input,
    phrase: Box<[u8; PINYIN_TABLE_FIELD]>,
    pinyin: Box<[u8; PINYIN_TABLE_FIELD]>,
    freq: u32,
}

impl TableInput {
    /// `fopen(filename, "r")` (`:138`); `None` for NULL.
    #[must_use]
    pub fn open(filename: &CStr) -> Option<Self> {
        Input::open_read(filename).map(|stream| Self {
            stream,
            phrase: Box::new([0; PINYIN_TABLE_FIELD]),
            pinyin: Box::new([0; PINYIN_TABLE_FIELD]),
            freq: 0,
        })
    }

    /// `feof(infile)` (`:144`, `:151`).
    pub fn eof(&mut self) -> bool {
        self.stream.eof()
    }

    /// `fscanf(infile, "%1023s %1023s %u", phrase, pinyin, &freq)`
    /// (`:145-146`): the conversions assigned, or `EOF` (negative).
    pub fn scan(&mut self) -> c_int {
        self.stream
            .scan_pinyin_table_line(&mut self.phrase, &mut self.pinyin, &mut self.freq)
    }

    /// The `phrase` buffer as `%s` reads it.
    #[must_use]
    pub fn phrase(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.phrase[..]).unwrap_or_default()
    }

    /// The `pinyin` buffer as `%s` and `strlen` read it.
    #[must_use]
    pub fn pinyin(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.pinyin[..]).unwrap_or_default()
    }

    /// `freq`.
    #[must_use]
    pub const fn freq(&self) -> u32 {
        self.freq
    }

    /// `g_utf8_strlen(phrase, -1)` (`:162`) over the buffer itself: a
    /// truncated sequence at the word's end steps past its NUL into the
    /// bytes an earlier, longer word left, as the pin's does.
    #[must_use]
    pub fn phrase_utf8_strlen(&self) -> c_long {
        native_tool::utf8_strlen_in_field(&self.phrase)
    }

    /// `fclose(infile)` (`:156`).
    pub fn close(self) {
        self.stream.close();
    }
}

/// `Can't open file %s.\n` on `stderr` (`:140`).
pub fn report_open_failed(filename: &CStr) {
    native_tool::gpt_report_open_failed(filename);
}

/// `Can't write file %s.\n` on `stderr` (`:263`).
pub fn report_write_failed(filename: &CStr) {
    native_tool::gpt_report_write_failed(filename);
}

/// `printf("nnodes: %d\n", g_tree_nnodes(g_chewing_tree))` (`:114`).
pub fn print_nnodes(nnodes: c_int) {
    native_tool::gpt_print_nnodes(nnodes);
}

/// `Too long phrase:%s\t%s\t%d\n` on `stderr` (`:169`).
pub fn report_too_long(phrase: &CStr, pinyin: &CStr, freq: u32) {
    native_tool::gpt_report_too_long(phrase, pinyin, freq);
}

/// `Invalid pinyin:%s\t%s\t%d\n` on `stderr` (`:186`).
pub fn report_invalid_pinyin(phrase: &CStr, pinyin: &CStr, freq: u32) {
    native_tool::gpt_report_invalid_pinyin(phrase, pinyin, freq);
}

/// `Duplicate item: phrase:%s\tpinyin:%s\tfreq:%u\n` on `stderr`
/// (`:215-216`).
pub fn report_duplicate(phrase: &CStr, pinyin: &CStr, freq: u32) {
    native_tool::gpt_report_duplicate(phrase, pinyin, freq);
}

/// `gen_pinyin_table: invalid UTF-8 phrase: %s\n` on `stderr`: not a pin
/// line. oxpinyin's report where the pin reads through the NULL
/// `g_utf8_to_ucs4` left for `phrase` (`:174`, `:280-281`; class (b)).
pub fn report_unconverted_phrase(phrase: &CStr) {
    native_tool::gpt_report_unconverted_phrase(phrase);
}

/// `store_one_item`'s append to `g_item_array[0]`, which is NULL (`:117`,
/// `:240`), for a phrase of length zero: GLib's own critical, nothing
/// stored.
pub fn store_into_null_array() {
    native_tool::gpt_store_into_null_array();
}

/// `fprintf(outfile, "%s\t%s\t%d\t%d\n", pinyin_str, phrase_str, token,
/// freq)` (`:314-316`).
pub fn print_row(
    output: &OutputStream,
    pinyin_str: &CStr,
    phrase_str: Option<&CStr>,
    token: c_int,
    freq: u32,
) {
    native_tool::gpt_print_row(&output.0, pinyin_str, phrase_str, token, freq);
}
