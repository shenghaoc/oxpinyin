//! `gen_k_mixture_model` (`utils/training/gen_k_mixture_model.cpp` at
//! `074a2219`): each input file is one document.
//!
//! A document is counted into the pin's two per-document `GHashTable`s —
//! unigram `token → count` and pair `token1 → (token2 → count)`
//! (`read_document`, `:65-135`) — with a null token ending a sentence and
//! `sentence_start` beginning one unless `--skip-pi-gram-training`. The pair
//! counts are then folded into the container (`train_second_word`,
//! `:245-281`): each `(W1, W2)` is `train_word_pair` (`:137-210`), whose
//! maximum-occurs filter subtracts an over-cap pair's count back out of the
//! unigram, and an all-skipped document leaves its W1 row unstored
//! (`delta == 0`). The surviving unigram counts are added to the array
//! headers and the magic total in `post_processing_unigram` (`:283-311`).
//!
//! Both `GHashTable`s are GLib's own, through
//! [`oxpinyin_input::tool::DirectHashTable`]: the pin walks them with
//! `g_hash_table_iter_next` to choose the DBM write order, so reproducing
//! the container's record order needs GLib's order, not a Rust map's.
//! `m_Mr` is the only cross-document state each pair carries, so documents
//! are processed in `argv` order. A document that cannot be opened prints
//! `can't open file: %s.\n` and `error:%s.\n` and exits with the saved
//! `errno` (`:352-358`).

use std::ffi::{CStr, CString};

use oxpinyin_input::tool::{self, DirectHashTable, LineReader, k_mixture_model as edge};

use crate::database::Database;
use crate::phrase::PhraseIndex;
use crate::records::{KMixtureModelArrayItem, KmmGram};

/// Default `--maximum-occurs-allowed` (`gen_k_mixture_model.cpp:50`).
pub const DEFAULT_MAX_OCCURS: i32 = 20;
/// Default `--maximum-increase-rates-allowed` (`:51`).
pub const DEFAULT_MAX_INCREASE_RATE: f64 = 3.0;

/// `null_token` (`src/include/novel_types.h:121`).
const NULL_TOKEN: u32 = 0;
/// `sentence_start` (`:122`).
const SENTENCE_START: u32 = 1;

/// The two command-line knobs and `--skip-pi-gram-training`
/// (`:50-62`).
#[derive(Clone, Copy, Debug)]
pub struct GenerateParams {
    /// `g_maximum_occurs`.
    pub max_occurs: i32,
    /// `g_maximum_increase_rates`.
    pub max_increase_rate: f64,
    /// `g_train_pi_gram`; false after `--skip-pi-gram-training`, whose
    /// `G_OPTION_FLAG_REVERSE` inverts a default of true.
    pub train_pi_gram: bool,
}

impl Default for GenerateParams {
    fn default() -> Self {
        Self {
            max_occurs: DEFAULT_MAX_OCCURS,
            max_increase_rate: DEFAULT_MAX_INCREASE_RATE,
            train_pi_gram: true,
        }
    }
}

/// `HashofDocument`: `token1 → (token2 → count)`. The inner tables live in
/// `seconds`, whose index is the outer table's value, so the outer
/// `GHashTable` keeps the pin's iteration order and the inner ones keep
/// theirs.
struct Document {
    pairs: DirectHashTable,
    seconds: Vec<DirectHashTable>,
    unigram: DirectHashTable,
}

impl Document {
    fn new() -> Self {
        Self {
            pairs: DirectHashTable::new(),
            seconds: Vec::new(),
            unigram: DirectHashTable::new(),
        }
    }

    fn second_words(&mut self, token1: u32) -> &mut DirectHashTable {
        let index = match self.pairs.lookup(token1) {
            Some(index) => index as usize,
            None => {
                self.seconds.push(DirectHashTable::new());
                let index = self.seconds.len() - 1;
                self.pairs.insert(token1, index as u32);
                index
            }
        };
        &mut self.seconds[index]
    }
}

/// `main`'s body (`:313-403`). Each name in `documents` is one document, in
/// order.
pub fn run(
    filename: Option<&CStr>,
    params: GenerateParams,
    documents: &[&CStr],
    phrases: &dyn PhraseIndex,
) {
    let bigram = Database::open_read_write(filename);

    for &document_filename in documents {
        let mut document = LineReader::open(document_filename);
        if !document.is_open() {
            let err_saved = document.open_error().unwrap_or(tool::ENOENT);
            edge::report_cant_open_file(document_filename);
            edge::report_error(&tool::strerror(err_saved));
            tool::exit(err_saved);
        }

        let mut hash_of_document = read_document(phrases, &mut document, params.train_pi_gram);

        for token1 in hash_of_document.pairs.keys() {
            let Document {
                pairs,
                seconds,
                unigram,
            } = &mut hash_of_document;
            train_second_word(unigram, &bigram, pairs, seconds, token1, params);
        }

        let mut magic_header = bigram.magic_header_checked();
        magic_header.m_n = magic_header.m_n.wrapping_add(1);
        bigram.set_magic_header_checked(&magic_header);

        post_processing_unigram(&bigram, &hash_of_document.unigram);
    }
}

/// `read_document` (`:65-135`): the per-document pair and unigram tables.
fn read_document<R: std::io::BufRead>(
    phrases: &dyn PhraseIndex,
    document: &mut LineReader<R>,
    train_pi_gram: bool,
) -> Document {
    let mut hash_of_document = Document::new();
    let mut cur_token = NULL_TOKEN;

    while document.getline() {
        let token = parse_segmented_line(phrases, document.c_str());
        let last_token = cur_token;
        cur_token = token;

        if cur_token == NULL_TOKEN {
            continue;
        }

        let freq = hash_of_document.unigram.lookup(cur_token).unwrap_or(0);
        hash_of_document
            .unigram
            .insert(cur_token, freq.wrapping_add(1));

        let last_token = if last_token == NULL_TOKEN {
            if !train_pi_gram {
                continue;
            }
            SENTENCE_START
        } else {
            last_token
        };

        let second_words = hash_of_document.second_words(last_token);
        let count = second_words.lookup(cur_token).unwrap_or(0);
        second_words.insert(cur_token, count.wrapping_add(1));
    }

    hash_of_document
}

/// `TAGLIB_PARSE_SEGMENTED_LINE` (`utils/utils_helper.h:51-70`): an empty
/// line is the null token; otherwise `g_strsplit_set(line, " \t", 2)` must
/// answer two tokens (else `abort()`), the first is `atoi`'s token and the
/// second is validated against the phrase index when the token is not the
/// null token.
fn parse_segmented_line(phrases: &dyn PhraseIndex, line: &CStr) -> u32 {
    if line.to_bytes().is_empty() {
        return NULL_TOKEN;
    }
    let parts = tool::strsplit_set(line, c" \t", 2);
    if parts.len() != 2 {
        std::process::abort();
    }
    let token_text = CString::new(parts[0].clone()).unwrap_or_default();
    let token = tool::atoi(&token_text) as u32;
    if token != NULL_TOKEN && !phrases.validate_token_with_string(token, &parts[1]) {
        std::process::abort();
    }
    token
}

/// `train_second_word` (`:245-281`).
fn train_second_word(
    hash_of_unigram: &mut DirectHashTable,
    bigram: &Database,
    pairs: &DirectHashTable,
    seconds: &[DirectHashTable],
    token1: u32,
    params: GenerateParams,
) -> bool {
    let mut single_gram = bigram.load(token1).unwrap_or_default();
    let saved_array_header_wc = single_gram.header().m_wc;

    let second_index = match pairs.lookup(token1) {
        Some(index) => index as usize,
        None => std::process::abort(),
    };
    let Some(hash_of_second_word) = seconds.get(second_index) else {
        std::process::abort();
    };
    for token2 in hash_of_second_word.keys() {
        let count = hash_of_second_word.lookup(token2).unwrap_or(0);
        train_word_pair(
            hash_of_unigram,
            &mut single_gram,
            token2,
            count,
            params,
        );
    }

    let delta = single_gram.header().m_wc.wrapping_sub(saved_array_header_wc);
    if delta == 0 {
        return false;
    }

    bigram.store_checked(token1, &single_gram);

    let mut magic_header = bigram.get_magic_header().unwrap_or_default();
    if magic_header.m_wc.wrapping_add(delta) < magic_header.m_wc {
        edge::report_magic_wc_overflow();
        return false;
    }
    magic_header.m_wc = magic_header.m_wc.wrapping_add(delta);
    bigram.set_magic_header_checked(&magic_header);

    true
}

/// `train_word_pair` (`:137-210`).
fn train_word_pair(
    hash_of_unigram: &mut DirectHashTable,
    single_gram: &mut KmmGram,
    token2: u32,
    count: u32,
    params: GenerateParams,
) {
    if let Some(mut array_item) = single_gram.get_array_item(token2) {
        let maximum_occurs_allowed = (params.max_occurs as u32)
            .max(ceil_mul(array_item.m_mr, params.max_increase_rate));
        if count > maximum_occurs_allowed {
            subtract_from_unigram(hash_of_unigram, token2, count);
            return;
        }
        array_item.m_wc = array_item.m_wc.wrapping_add(count);
        array_item.m_n_n_0 = array_item.m_n_n_0.wrapping_add(1);
        if count == 1 {
            array_item.m_n_1 = array_item.m_n_1.wrapping_add(1);
        }
        array_item.m_mr = array_item.m_mr.max(count);
        if !single_gram.set_array_item(token2, array_item) {
            std::process::abort();
        }
    } else {
        if count > params.max_occurs as u32 {
            subtract_from_unigram(hash_of_unigram, token2, count);
            return;
        }
        let array_item = KMixtureModelArrayItem {
            m_wc: count,
            m_n_n_0: 1,
            m_n_1: u32::from(count == 1),
            m_mr: count,
        };
        if !single_gram.insert_array_item(token2, array_item) {
            std::process::abort();
        }
    }

    let mut array_header = *single_gram.header();
    array_header.m_wc = array_header.m_wc.wrapping_add(count);
    single_gram.set_header(array_header);
}

/// `train_word_pair`'s over-cap subtraction (`:152-166`, `:178-193`): store
/// the difference, steal the key on zero; the `freq < 0` branch is
/// unreachable for a `guint32`.
fn subtract_from_unigram(hash_of_unigram: &mut DirectHashTable, token2: u32, count: u32) {
    let Some(freq) = hash_of_unigram.lookup(token2) else {
        std::process::abort();
    };
    let freq = freq.wrapping_sub(count);
    if freq > 0 {
        hash_of_unigram.insert(token2, freq);
    } else if freq == 0 {
        hash_of_unigram.steal(token2);
    } else {
        std::process::abort();
    }
}

/// `(guint32)ceil(m_Mr * g_maximum_increase_rates)` (`:144-146`).
fn ceil_mul(mr: u32, rate: f64) -> u32 {
    (f64::from(mr) * rate).ceil() as u32
}

/// `post_processing_unigram` (`:283-311`).
fn post_processing_unigram(bigram: &Database, hash_of_unigram: &DirectHashTable) {
    let mut total_freq = 0_u32;
    for token in hash_of_unigram.keys() {
        let freq = hash_of_unigram.lookup(token).unwrap_or(0);
        let mut array_header = bigram.get_array_header(token).unwrap_or_default();
        array_header.m_freq = array_header.m_freq.wrapping_add(freq);
        total_freq = total_freq.wrapping_add(freq);
        bigram.set_array_header(token, &array_header);
    }

    let mut magic_header = bigram.magic_header_checked();
    if magic_header.m_total_freq.wrapping_add(total_freq) < magic_header.m_total_freq {
        edge::report_magic_total_freq_overflow();
        return;
    }
    magic_header.m_total_freq = magic_header.m_total_freq.wrapping_add(total_freq);
    bigram.set_magic_header_checked(&magic_header);
}
