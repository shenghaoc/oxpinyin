//! tests/storage/test_ngram.cpp:5-95 at 074a2219.
use super::*;
use oxpinyin_core::single_gram as gram;
use oxpinyin_store::{DefaultUserBigramDb, UserBigramDb, WriteStore};

fn print_gram(bytes: &[u8]) {
    let total = present(gram::get_total_freq(bytes));
    for (token, freq) in gram::search(bytes, 0, 8) {
        output::probability(token, freq as f32 / total as f32);
    }
}

/// `bigram.store(token, &single_gram)`, whose answer the program ignores
/// (`tests/storage/test_ngram.cpp:41, 46` at 074a2219).
fn store_gram(bigram: &DefaultStore, token: u32, single: &[u8]) {
    let _ = bigram.tool_set(&token.to_ne_bytes(), single);
}

/// Execute the program under its fixed names ([`crate::fixed_tmp`]).
pub fn main() {
    let mut single = gram::new_gram();
    gram::set_total_freq(&mut single, 16);
    for (token, freq) in [(2, 1), (6, 2), (4, 4), (3, 8), (1, 16), (3, 32)] {
        if gram::get_freq(&single, token).is_some() {
            check(gram::set_freq(&mut single, token, freq));
        } else {
            check(gram::insert_freq(&mut single, token, freq));
        }
    }
    check(gram::get_freq(&single, 3) == Some(32));
    output::message(Message::Separator);
    print_gram(&single);
    check(gram::get_total_freq(&single) == Some(16));

    let bigram = required(DefaultStore::create_hash(
        crate::fixed_tmp::path("test.db").as_path(),
    ));
    store_gram(&bigram, 1, &single);
    check(gram::insert_freq(&mut single, 5, 8));
    check(gram::remove_freq(&mut single, 1).is_some());
    gram::set_total_freq(&mut single, 32);
    store_gram(&bigram, 2, &single);
    for predecessor in 1_u32..=2 {
        output::message(Message::Separator);
        let mut loaded = Vec::new();
        check(required(
            bigram.get_bigram_raw(&predecessor.to_ne_bytes(), &mut loaded),
        ));
        print_gram(&loaded);
    }
    output::message(Message::Separator);
    output::total(present(gram::get_total_freq(&single)));
    let items = keys(&bigram);
    output::message(Message::System);
    for item in items {
        output::token(item);
    }
    required(bigram.save_bigram_db(crate::fixed_tmp::path("snapshot.db").as_path()));
    // load_db resets/closes the attached handle before loading the snapshot.
    drop(bigram);
    let bigram = required(DefaultUserBigramDb::load_db(
        crate::fixed_tmp::path("snapshot.db").as_path(),
    ));
    // mask_out(0,0) removes every predecessor, never reaches a gram's mask_out.
    for key in required(bigram.keys()) {
        required(bigram.remove(&present(key)));
    }
}
