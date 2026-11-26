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

use oxpinyin_tool_support::input::{self as tool, OutputStream, k_mixture_model as edge};

use super::database::Database;
use crate::phrase_index::SystemPhraseIndex;

/// `main`'s body before the output callbacks (`:112-152`). `filename` is the
/// `--k-mixture-model-file` value.
pub fn run(filename: Option<&CStr>, phrases: &SystemPhraseIndex, output: &mut OutputStream) {
    let bigram = Database::open_read_only(filename);
    if !bigram.attached() {
        edge::report_open_failed(filename);
        tool::exit(tool::ENOENT);
    }
    print_magic_header(&bigram, output);
    print_array_headers(&bigram, phrases, output);
    print_array_items(&bigram, phrases, output);
    edge::print_end(output);
}

/// `print_k_mixture_model_magic_header` (`:35-46`).
fn print_magic_header(bigram: &Database, output: &mut OutputStream) {
    let Some(magic_header) = bigram.get_magic_header() else {
        edge::report_no_magic();
        tool::exit(tool::ENODATA);
    };
    edge::print_data_header(
        output,
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
fn print_array_headers(bigram: &Database, phrases: &SystemPhraseIndex, output: &mut OutputStream) {
    edge::print_gram_1(output);
    for token in bigram.get_all_items() {
        let array_header = bigram.array_header_checked(token);
        let Some(phrase) = phrases.token_to_string(token) else {
            continue;
        };
        let phrase = c_text(&phrase);
        edge::print_unigram_item(
            output,
            token as i32,
            &phrase,
            array_header.m_wc as i32,
            array_header.m_freq as i32,
        );
    }
}

/// `print_k_mixture_model_array_items` (`:69-105`).
fn print_array_items(bigram: &Database, phrases: &SystemPhraseIndex, output: &mut OutputStream) {
    edge::print_gram_2(output);
    for token in bigram.get_all_items() {
        let single_gram = bigram.load_checked(token);
        for item in single_gram.items() {
            let word1 = phrases.token_to_string(token);
            let word2 = phrases.token_to_string(item.token);
            if let (Some(word1), Some(word2)) = (word1, word2) {
                let word1 = c_text(&word1);
                let word2 = c_text(&word2);
                edge::print_bigram_item(
                    output,
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
