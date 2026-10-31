//! `eval_correction_rate` (`utils/training/eval_correction_rate.cpp` at
//! libpinyin 074a2219).
//!
//! Reads `evals2.text` from the current directory, tokenises it at
//! `null_token` boundaries, and for each sentence lays out one key per cell
//! for the highest-frequency pronunciation of each token, then decodes the
//! key chain with `PhoneticLookup<1, 1>`'s single best path and compares the
//! two sentences. The final `correction rate:%f` is
//! `passed_count / tested_count`, a C double division.
//!
//! The decode is `oxpinyin_training::eval`'s port of `PhoneticLookup<1, 1>`
//! (`crates/oxpinyin-training/src/eval/decode.rs`); its model here is built
//! from the data directory's own chunks and `bigram.db` instead of an
//! `interpolation2.text`, and its phrase lookup is the pinyin index
//! (`FacadeChewingTable2`) resolved through the loaded phrase libraries.
//! The pin's failing `user_bigram.attach(NULL, ATTACH_CREATE|ATTACH_READWRITE)`
//! (`:156-157`) leaves the user bigram `NULL`, so `merge_single_gram`
//! always sees the system row alone and no user row is read.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use oxpinyin_core::{ChewingKey, Dictionary, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_data::phrase_library::{LibraryError, RowFault};
use oxpinyin_data::{DictError, PhraseLibraries, PinyinIndexTable};
use oxpinyin_input::tool;
use oxpinyin_input::tool::LineReader;
use oxpinyin_training::eval::{EvalLanguageModel, PhraseSource, correction_rate};

use crate::training::{NULL_TOKEN, all_tokens, load_gram, load_table_conf, parse_segmented_line};

/// The evaluation corpus name (`eval_correction_rate.cpp:133`).
const EVALS_TEXT: &std::ffi::CStr = c"evals2.text";

/// `main` (`eval_correction_rate.cpp:132-208`).
pub fn main(_args: impl IntoIterator<Item = OsString>) {
    let conf = load_table_conf();

    // `FacadeChewingTable2 largetable; largetable.load(SYSTEM_PINYIN_INDEX,
    // NULL);` — the answer is ignored: a failed load leaves an empty table,
    // whose searches find nothing and reach the result assertion below.
    let pinyin = PinyinIndexTable::open(Path::new("pinyin_index.bin")).ok();

    // `load_phrase_index(phrase_files, &phrase_index)`: every default
    // `SYSTEM_FILE` row; a refused chunk writes `load %s failed!` and ends
    // the program at `ENOENT`.
    let libraries = load_libraries(&conf);

    // `system_bigram.attach(SYSTEM_BIGRAM, ATTACH_READONLY)`: ignored.
    let bigram = crate::storage::dbm::Hash::open_read_only(Path::new("bigram.db")).ok();

    // `system_table_info.get_lambda()`, the `gfloat` the pin passes to
    // `PhoneticLookup<1, 1>`.
    let lambda = conf.lambda_f32();
    let model = build_model(&libraries, bigram.as_ref(), lambda);
    let source = LibraryPhraseSource {
        libraries: &libraries,
    };
    let dictionary = EvalDictionary {
        pinyin: pinyin.as_ref(),
        libraries: &libraries,
    };

    // `std::ifstream evals_file(evals_text); if (!evals_file)`.
    let mut evals = LineReader::open(EVALS_TEXT);
    if !evals.is_open() {
        tool::training::report_cant_open_file(EVALS_TEXT);
        tool::exit(tool::ENOENT);
    }

    let mut tested_count = 0_usize;
    let mut passed_count = 0_usize;
    let mut tokens: Vec<u32> = Vec::new();

    while evals.getline() {
        let token = parse_segmented_line(evals.c_str(), |token| token_text(&libraries, token));
        if token == NULL_TOKEN {
            // `/* one test. */`
            if !tokens.is_empty() {
                if do_one_test(&dictionary, &model, &source, &tokens) {
                    passed_count += 1;
                }
                tested_count += 1;
                tokens.clear();
            }
        } else {
            tokens.push(token);
        }
    }

    if !tokens.is_empty() {
        if do_one_test(&dictionary, &model, &source, &tokens) {
            passed_count += 1;
        }
        tested_count += 1;
    }

    // `parameter_t rate = passed_count / (parameter_t) tested_count;`
    let rate = passed_count as f64 / tested_count as f64;
    tool::training::print_correction_rate(rate);
}

/// `load_phrase_index` over `table.conf`'s default `SYSTEM_FILE` rows
/// (`utils/utils_helper.h:73-95`), into the data layer's facade so the
/// evaluation model and the pinyin lookup read the same items.
fn load_libraries(conf: &oxpinyin_data::table_info::TableConf) -> PhraseLibraries {
    let mut libraries = PhraseLibraries::empty();
    for (index, row) in conf.system_libraries() {
        let Some(name) = row.system.as_ref() else {
            // `MemoryChunk::load(NULL)`.
            tool::abort();
        };
        let c_name = oxpinyin_input::c_path(Path::new(name.as_ref()));
        match libraries.load(index, Path::new(name.as_ref())) {
            Ok(_) => {}
            // `MemoryChunk::load`'s header `assert` on a directory named as
            // a chunk (`memory_chunk.h:429`).
            Err(LibraryError::Row(RowFault::ChunkHeaderRead)) => tool::abort(),
            Err(_) => {
                tool::storage::report_load_failed(c_name.as_deref());
                tool::exit(tool::ENOENT);
            }
        }
    }
    libraries
}

/// The pin's evaluation model over the binary tables.
fn build_model(
    libraries: &PhraseLibraries,
    bigram: Option<&crate::storage::dbm::Hash>,
    lambda: f32,
) -> EvalLanguageModel {
    let mut unigrams = BTreeMap::new();
    for nibble in 0..16_u32 {
        let base = nibble << 24;
        let Some(library) = libraries.library(base) else {
            continue;
        };
        for (slot, item) in library.items() {
            unigrams.insert(base | slot, u64::from(item.unigram()));
        }
    }
    let unigram_total = u64::from(libraries.unigram_total());

    let mut bigrams = BTreeMap::new();
    let mut bigram_totals = BTreeMap::new();
    for token in all_tokens(bigram) {
        let Some(gram) = load_gram(bigram, token) else {
            continue;
        };
        bigram_totals.insert(token, u64::from(gram.total));
        for (next, count) in gram.records {
            bigrams.entry((token, next)).or_insert(u64::from(count));
        }
    }

    EvalLanguageModel::from_binary(unigrams, unigram_total, bigrams, bigram_totals, lambda)
}

/// One test (`do_one_test`, `eval_correction_rate.cpp:84-130`): `true` when
/// the decoded sentence equals the source sentence.
///
/// The pin's `assert(1 == results.size())` and its `check_result` /
/// `get_nth_pronunciation` assertions are the decode port's errors; each
/// aborts here after the pin's preceding work, as do a failed `strcmp` of
/// two decoded sentences (the port keeps them as strings).
fn do_one_test(
    dictionary: &EvalDictionary<'_>,
    model: &EvalLanguageModel,
    source: &LibraryPhraseSource<'_>,
    tokens: &[u32],
) -> bool {
    let sentence: Vec<PhraseToken> = tokens.iter().copied().map(PhraseToken::new).collect();
    let report = match correction_rate(dictionary, model, source, std::slice::from_ref(&sentence)) {
        Ok(report) => report,
        Err(_) => tool::abort(),
    };
    match report.mismatches.first() {
        None => true,
        Some((expected, guessed)) => {
            tool::training::report_test_sentence(&c_string(expected));
            tool::training::report_guessed_sentence(&c_string(guessed));
            tool::training::report_mismatch();
            false
        }
    }
}

/// The bytes of `text` up to its first NUL, as `%s` prints a C string.
fn c_string(text: &str) -> std::ffi::CString {
    crate::segment::c_string(text.as_bytes()).unwrap_or_default()
}

/// `taglib_token_to_string` (`src/storage/tag_utility.cpp:384-405`) over
/// the data layer's loaded libraries: `"<start>"` for token 1, the item's
/// text otherwise, and the pin's `error: unknown token:%d.` line before
/// `None`.
fn token_text(libraries: &PhraseLibraries, token: u32) -> Option<std::ffi::CString> {
    if crate::storage::library_of(token) == 0 {
        if token == 1 {
            return Some(c"<start>".to_owned());
        }
        tool::storage::report_unknown_token(token as core::ffi::c_int);
        return None;
    }
    let Some(library) = libraries.library(token) else {
        tool::storage::report_unknown_token(token as core::ffi::c_int);
        return None;
    };
    let Some(item) = library.item(token) else {
        tool::storage::report_unknown_token(token as core::ffi::c_int);
        return None;
    };
    // `item.get_phrase_string(buffer)` then `g_ucs4_to_utf8(buffer,
    // length)`: a word that cannot be read leaves the pin's buffer partly
    // unset (class (b)); a word at or above `0x8000_0000` makes the call
    // answer NULL, which the caller's `strcmp` reads (class (b)).
    let words: Option<Vec<u32>> = item.phrase_codes().collect();
    let words = words?;
    crate::segment::c_string(&tool::ucs4_to_utf8(&words)?)
}

/// `get_possible_pinyin` and `convert_to_utf8` over the loaded phrase
/// libraries (`eval_correction_rate.cpp:36-63`, `:113-116`).
struct LibraryPhraseSource<'a> {
    libraries: &'a PhraseLibraries,
}

impl PhraseSource for LibraryPhraseSource<'_> {
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>> {
        // `get_possible_pinyin` keeps the first pronunciation of the
        // highest frequency (`freq > max_freq`), then appends its keys.
        let best = self
            .libraries
            .pronunciations(token.value())?
            .into_iter()
            .reduce(|best, next| if next.freq > best.freq { next } else { best })?;
        best.spelling
            .split('\'')
            .map(SyllableKey::from_text)
            .collect::<Option<Vec<_>>>()
    }

    fn text(&self, token: PhraseToken) -> Option<String> {
        self.libraries
            .phrase_text(token.value())
            .and_then(|text| text.as_str().map(ToOwned::to_owned))
    }

    fn lexicon_tokens(&self) -> Vec<PhraseToken> {
        let mut tokens = Vec::new();
        for nibble in 0..16_u32 {
            let base = nibble << 24;
            let Some(library) = self.libraries.library(base) else {
                continue;
            };
            for (slot, _item) in library.items() {
                tokens.push(PhraseToken::new(base | slot));
            }
        }
        tokens
    }
}

/// The pinyin index resolved through the loaded phrase libraries: the
/// `Dictionary` the decode's `search_matrix` and `SEARCH_CONTINUED` need.
struct EvalDictionary<'a> {
    pinyin: Option<&'a PinyinIndexTable>,
    libraries: &'a PhraseLibraries,
}

impl Dictionary for EvalDictionary<'_> {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;
    type PathMemo = ();

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, DictError> {
        let Some(pinyin) = self.pinyin else {
            return Ok(Vec::new());
        };
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(Vec::new());
        };
        let tokens = pinyin.search(&keys)?;
        let mut entries = Vec::new();
        for token in tokens {
            // `resolve_item`: a token whose library is unloaded or whose
            // item cannot be read is dropped, upstream's `NULL == head`
            // skip.
            let Some(library) = self.libraries.library(token) else {
                continue;
            };
            let Some(item) = library.item(token) else {
                continue;
            };
            let Some(text) = item.phrase_text() else {
                continue;
            };
            let mut entry = PhraseEntry::new(PhraseToken::new(token), text);
            if let Some((matched, total)) = self.libraries.pronunciation_possibility(token, &keys) {
                entry = entry.with_pronunciation_possibility(matched, total);
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    /// `SEARCH_CONTINUED`: the query's index key exists in the DBM.
    fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> Result<bool, DictError> {
        let Some(pinyin) = self.pinyin else {
            return Ok(false);
        };
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(false);
        };
        pinyin.key_exists(&keys)
    }
}

/// `ChewingKey` from a tone-less spelling, as the lookup's query keys are.
fn syllables_to_chewing_keys(syllables: &[SyllableKey]) -> Option<Vec<ChewingKey>> {
    syllables
        .iter()
        .map(|s| ChewingKey::from_pinyin(s.text()))
        .collect()
}
