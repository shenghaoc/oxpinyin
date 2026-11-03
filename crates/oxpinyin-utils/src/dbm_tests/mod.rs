//! Pin 074a2219's three DBM test programs. No argv or stdin is read.
use oxpinyin_input::tool;
use oxpinyin_input::tool::dbm_tests as output;
use oxpinyin_input::tool::dbm_tests::Message;
use oxpinyin_store::{DefaultStore, RawReadStore, StoreError};

pub mod flexible;
pub mod ngram;
pub mod punct;

fn check(result: bool) {
    if !result {
        tool::abort();
    }
}

fn required<T>(result: Result<T, StoreError>) -> T {
    result.unwrap_or_else(|_| tool::abort())
}

fn present<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| tool::abort())
}

/// The pin asserts every key is sizeof(phrase_token_t).
fn token(key: &[u8]) -> u32 {
    let bytes: [u8; 4] = key.try_into().unwrap_or_else(|_| tool::abort());
    u32::from_ne_bytes(bytes)
}

fn keys(store: &DefaultStore) -> Vec<u32> {
    required(store.native_keys())
        .iter()
        .map(|key| token(key))
        .collect()
}
