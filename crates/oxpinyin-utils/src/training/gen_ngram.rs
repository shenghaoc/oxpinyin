//! `gen_ngram` (`utils/training/gen_ngram.cpp` at libpinyin 074a2219).
//!
//! Reads segmented lines from standard input, adds one unigram count to
//! each line's token, and raises the system bigram's
//! `last_token → cur_token` frequency and row total. The first token of a
//! sentence follows `sentence_start` unless `--skip-pi-gram-training`
//! clears the training flag. The default `SYSTEM_FILE` chunks are saved at
//! the end.
//!
//! `gen_deleted_ngram` is the same loop over `deleted_bigram.db` without
//! the unigram count or the chunk save; [`crate::training::gen_deleted_ngram`]
//! calls [`run`] with `deleted` set.

use std::ffi::OsString;

use oxpinyin_data::table_info::FileType;
use oxpinyin_input::tool;
use oxpinyin_input::tool::OptionValue;

use crate::storage::dbm::Hash;
use crate::storage::libraries::{Libraries, token_to_string};
use crate::training::{
    NULL_TOKEN, SENTENCE_START, load_gram, load_table_conf, parse_segmented_line, path_of,
    skip_pi_gram_entry, store_gram,
};

/// `main` (`gen_ngram.cpp:45-124`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    run(args, false);
}

/// The shared body of `gen_ngram` and `gen_deleted_ngram`.
pub(crate) fn run(args: impl IntoIterator<Item = OsString>, deleted: bool) {
    let _ = tool::set_locale_from_environment();
    let (parameter_string, entries): (&'static std::ffi::CStr, Vec<_>) = if deleted {
        (
            c"- generate deleted n-gram",
            vec![
                skip_pi_gram_entry(),
                crate::training::deleted_bigram_entry(),
            ],
        )
    } else {
        (
            c"- generate n-gram",
            vec![skip_pi_gram_entry(), crate::training::bigram_entry()],
        )
    };
    let parsed = crate::storage::parse_options(parameter_string, &entries, args);
    let train_pi_gram = matches!(parsed.values.first(), Some(OptionValue::Flag(true)));
    let bigram_name = crate::training::filename_value(&parsed, 1);

    let conf = load_table_conf();
    let mut libraries = Libraries::load_rows(
        &conf,
        false,
        &[FileType::SystemFile],
        if deleted {
            // `gen_deleted_ngram.cpp:72-73`.
            tool::ENODATA
        } else {
            // `gen_ngram.cpp:71-72`.
            tool::ENOENT
        },
    );

    // `bigram.attach(name, ATTACH_CREATE|ATTACH_READWRITE)`: the answer is
    // ignored, and a failed attach leaves every load and store a no-op.
    let bigram = bigram_name
        .as_deref()
        .and_then(|name| Hash::create(&path_of(name)).ok());

    train(&mut libraries, bigram.as_ref(), train_pi_gram, deleted);
    if !deleted {
        libraries.save_rows(&conf, false, FileType::SystemFile);
    }
}

/// The `std::getline(std::cin, linebuf)` loop (`gen_ngram.cpp:77-118`,
/// `gen_deleted_ngram.cpp:78-116`).
fn train(libraries: &mut Libraries, bigram: Option<&Hash>, train_pi_gram: bool, deleted: bool) {
    let mut reader = oxpinyin_input::tool::LineReader::stdin();
    let mut cur_token = NULL_TOKEN;
    let mut last_token;
    while reader.getline() {
        let token = parse_segmented_line(reader.c_str(), |token| token_to_string(libraries, token));
        last_token = cur_token;
        cur_token = token;

        // `/* skip null_token in second word. */`
        if cur_token == NULL_TOKEN {
            continue;
        }

        // `/* training uni-gram */`: `gen_ngram` only.
        if !deleted {
            libraries.add_unigram(cur_token, 1);
        }

        // `/* skip pi-gram training. */`
        if last_token == NULL_TOKEN {
            if !train_pi_gram {
                continue;
            }
            last_token = SENTENCE_START;
        }

        // `/* train bi-gram */`
        let mut gram = load_gram(bigram, last_token).unwrap_or_else(|| crate::training::Gram {
            total: 0,
            records: Vec::new(),
        });
        if let Some(freq) = gram.get_freq(cur_token) {
            // `check_result(single_gram->set_freq(cur_token, freq + 1))`.
            if !gram.set_freq(cur_token, freq.wrapping_add(1)) {
                tool::abort();
            }
        } else if !gram.insert_freq(cur_token, 1) {
            // `check_result(single_gram->insert_freq(cur_token, 1))`.
            tool::abort();
        }
        // `/* increase total freq */`
        gram.total = gram.total.wrapping_add(1);
        store_gram(bigram, last_token, &gram);
    }
}
