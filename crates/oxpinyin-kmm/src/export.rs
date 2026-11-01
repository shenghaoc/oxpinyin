//! `export_k_mixture_model` (`utils/training/export_k_mixture_model.cpp` at
//! `074a2219`): the working container as the pin's text model.
//!
//! Both sections walk `get_all_items` in the backend's native order — the
//! order is not sorted — and each single gram's `retrieve_all` is
//! token-ascending. A token whose phrase text `taglib_token_to_string`
//! cannot resolve is skipped after that function's
//! `error: unknown token:%d.\n`; a missing magic header is `ENODATA`, and a
//! container that does not attach is `open %s failed.\n` with `ENOENT`
//! (`:35-152`).

use std::ffi::CStr;
use std::io::Write as _;

use oxpinyin_input::tool::{self, OutputStream, k_mixture_model as edge};

use crate::database::Database;
use crate::phrase::PhraseIndex;

/// The output stream's `fprintf` calls, so the in-memory tests can compare
/// the pin's bytes without a file.
pub trait Sink {
    /// `\data model "k mixture model" count %d N %d total_freq %d\n`.
    fn data_header(&mut self, wc: i32, n: i32, total_freq: i32);
    /// `\1-gram\n`.
    fn gram_1(&mut self);
    /// `\2-gram\n`.
    fn gram_2(&mut self);
    /// `\end\n`.
    fn end(&mut self);
    /// `\item %d %s count %d freq %d\n`.
    fn unigram(&mut self, token: i32, phrase: &CStr, wc: i32, freq: i32);
    /// `\item %d %s %d %s count %d T %d N_n_0 %d n_1 %d Mr %d\n`.
    #[allow(clippy::too_many_arguments)]
    fn bigram(
        &mut self,
        token1: i32,
        word1: &CStr,
        token2: i32,
        word2: &CStr,
        wc: i32,
        n_n_0: i32,
        n_1: i32,
        mr: i32,
    );
}

impl Sink for OutputStream {
    fn data_header(&mut self, wc: i32, n: i32, total_freq: i32) {
        edge::print_data_header(self, wc, n, total_freq);
    }

    fn gram_1(&mut self) {
        edge::print_gram_1(self);
    }

    fn gram_2(&mut self) {
        edge::print_gram_2(self);
    }

    fn end(&mut self) {
        edge::print_end(self);
    }

    fn unigram(&mut self, token: i32, phrase: &CStr, wc: i32, freq: i32) {
        edge::print_unigram_item(self, token, phrase, wc, freq);
    }

    fn bigram(
        &mut self,
        token1: i32,
        word1: &CStr,
        token2: i32,
        word2: &CStr,
        wc: i32,
        n_n_0: i32,
        n_1: i32,
        mr: i32,
    ) {
        edge::print_bigram_item(self, token1, word1, token2, word2, wc, n_n_0, n_1, mr);
    }
}

/// The same lines in memory.
impl Sink for Vec<u8> {
    fn data_header(&mut self, wc: i32, n: i32, total_freq: i32) {
        let _ = writeln!(
            self,
            "\\data model \"k mixture model\" count {wc} N {n} total_freq {total_freq}"
        );
    }

    fn gram_1(&mut self) {
        self.extend_from_slice(b"\\1-gram\n");
    }

    fn gram_2(&mut self) {
        self.extend_from_slice(b"\\2-gram\n");
    }

    fn end(&mut self) {
        self.extend_from_slice(b"\\end\n");
    }

    fn unigram(&mut self, token: i32, phrase: &CStr, wc: i32, freq: i32) {
        let _ = write!(self, "\\item {token} ");
        self.extend_from_slice(phrase.to_bytes());
        let _ = writeln!(self, " count {wc} freq {freq}");
    }

    fn bigram(
        &mut self,
        token1: i32,
        word1: &CStr,
        token2: i32,
        word2: &CStr,
        wc: i32,
        n_n_0: i32,
        n_1: i32,
        mr: i32,
    ) {
        let _ = write!(self, "\\item {token1} ");
        self.extend_from_slice(word1.to_bytes());
        let _ = write!(self, " {token2} ");
        self.extend_from_slice(word2.to_bytes());
        let _ = writeln!(self, " count {wc} T {wc} N_n_0 {n_n_0} n_1 {n_1} Mr {mr}");
    }
}

/// `main`'s body before the output callbacks (`:112-152`). `filename` is the
/// `--k-mixture-model-file` value.
pub fn run(filename: Option<&CStr>, phrases: &dyn PhraseIndex, output: &mut impl Sink) {
    let bigram = Database::open_read_only(filename);
    if !bigram.is_open() {
        edge::report_open_failed(filename);
        tool::exit(tool::ENOENT);
    }
    print_magic_header(&bigram, output);
    print_array_headers(&bigram, phrases, output);
    print_array_items(&bigram, phrases, output);
    output.end();
}

/// `print_k_mixture_model_magic_header` (`:35-46`).
fn print_magic_header(bigram: &Database, output: &mut impl Sink) {
    let Some(magic_header) = bigram.get_magic_header() else {
        edge::report_no_magic();
        tool::exit(tool::ENODATA);
    };
    output.data_header(
        magic_header.m_wc as i32,
        magic_header.m_n as i32,
        magic_header.m_total_freq as i32,
    );
}

/// A phrase as a C string, truncated at its first NUL as `%s` would read it.
fn c_text(bytes: &[u8]) -> std::ffi::CString {
    let bytes = bytes.split(|byte| *byte == 0).next().unwrap_or(&[]);
    std::ffi::CString::new(bytes).unwrap_or_default()
}

/// `print_k_mixture_model_array_headers` (`:48-67`).
fn print_array_headers(bigram: &Database, phrases: &dyn PhraseIndex, output: &mut impl Sink) {
    output.gram_1();
    for token in bigram.get_all_items() {
        let array_header = bigram.array_header_checked(token);
        let Some(phrase) = phrases.token_to_string(token) else {
            continue;
        };
        let phrase = c_text(&phrase);
        output.unigram(
            token as i32,
            &phrase,
            array_header.m_wc as i32,
            array_header.m_freq as i32,
        );
    }
}

/// `print_k_mixture_model_array_items` (`:69-105`).
fn print_array_items(bigram: &Database, phrases: &dyn PhraseIndex, output: &mut impl Sink) {
    output.gram_2();
    for token in bigram.get_all_items() {
        let single_gram = bigram.load_checked(token);
        for item in single_gram.items() {
            let word1 = phrases.token_to_string(token);
            let word2 = phrases.token_to_string(item.token);
            if let (Some(word1), Some(word2)) = (word1, word2) {
                let word1 = c_text(&word1);
                let word2 = c_text(&word2);
                output.bigram(
                    token as i32,
                    &word1,
                    item.token as i32,
                    &word2,
                    item.item.m_wc as i32,
                    item.item.m_n_n_0 as i32,
                    item.item.m_n_1 as i32,
                    item.item.m_mr as i32,
                );
            }
        }
    }
}
