//! Stage-2 Criterion groups over the W8 C-ABI surface.
//!
//! Groups:
//! - `parse_more_full_pinyins`: short / medium / junk-leading
//! - `guess_candidates`: offset 0 and mid-phrase
//! - `guess_sentence_get_sentence_0`: n-best lookup plus row 0
//! - `user_store_count_delta_hot_token`: cached overlay on one token
//!
//! Uses the pinned model dir. Prints the pin SHA in the run header.
//! Does **not** assert the 10177-class candidate pins — those stay in
//! `pinyin-oracle` `real_tables_integration`.
//!
//! `noise_threshold(0.50)`: a `--baseline` comparison only flags a change
//! larger than 50%. That is the optional large-regression gate; CI does
//! not run these benches.

#![allow(missing_docs)]

use std::ffi::CString;
use std::hint::black_box;
use std::os::raw::{c_char, c_uint, c_void};
use std::path::PathBuf;
use std::time::Duration;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use oxpinyin_user::{SENTENCE_START, UserStore};

#[path = "support/mod.rs"]
mod support;

use support::{
    CapiBench, GUESS_MID_OFFSET, GUESS_PHRASE, HOT_TOKEN, Instance, PARSE_JUNK_LEADING,
    PARSE_MEDIUM, PARSE_SHORT, W8_SORT, pinyin_get_n_candidate, pinyin_get_sentence,
    pinyin_guess_candidates, pinyin_guess_sentence, pinyin_parse_more_full_pinyins, pinyin_reset,
    print_pin_header,
};

unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

fn parse_more_full_pinyins(criterion: &mut Criterion) {
    let capi = CapiBench::open();
    let short = CString::new(PARSE_SHORT).expect("short");
    let medium = CString::new(PARSE_MEDIUM).expect("medium");
    let junk = CString::new(PARSE_JUNK_LEADING).expect("junk");

    let mut group = criterion.benchmark_group("parse_more_full_pinyins");
    group.bench_function("short", |bencher| {
        bencher.iter(|| {
            // SAFETY: `capi.instance` is a live handle for this bench.
            unsafe {
                let _ = pinyin_reset(capi.instance);
                black_box(pinyin_parse_more_full_pinyins(
                    capi.instance,
                    short.as_ptr(),
                ))
            }
        });
    });
    group.bench_function("medium", |bencher| {
        bencher.iter(|| {
            // SAFETY: `capi.instance` is a live handle for this bench.
            unsafe {
                let _ = pinyin_reset(capi.instance);
                black_box(pinyin_parse_more_full_pinyins(
                    capi.instance,
                    medium.as_ptr(),
                ))
            }
        });
    });
    group.bench_function("junk_leading", |bencher| {
        bencher.iter(|| {
            // SAFETY: `capi.instance` is a live handle for this bench.
            unsafe {
                let _ = pinyin_reset(capi.instance);
                black_box(pinyin_parse_more_full_pinyins(capi.instance, junk.as_ptr()))
            }
        });
    });
    group.finish();
}

fn guess_candidates(criterion: &mut Criterion) {
    let capi = CapiBench::open();
    let phrase = CString::new(GUESS_PHRASE).expect("phrase");

    let mut group = criterion.benchmark_group("guess_candidates");
    group.bench_function("offset_0", |bencher| {
        bencher.iter_batched(
            || {
                // SAFETY: `capi.instance` is a live handle for this bench.
                unsafe {
                    let _ = pinyin_reset(capi.instance);
                    pinyin_parse_more_full_pinyins(capi.instance, phrase.as_ptr())
                }
            },
            |_| {
                // SAFETY: setup just parsed into the same live instance.
                unsafe {
                    let guessed = pinyin_guess_candidates(capi.instance, 0, W8_SORT as c_uint);
                    let mut count: c_uint = 0;
                    let _ = pinyin_get_n_candidate(capi.instance, &mut count);
                    black_box((guessed, count))
                }
            },
            BatchSize::SmallInput,
        );
    });
    group.bench_function("mid_phrase", |bencher| {
        bencher.iter_batched(
            || {
                // SAFETY: `capi.instance` is a live handle for this bench.
                unsafe {
                    let _ = pinyin_reset(capi.instance);
                    pinyin_parse_more_full_pinyins(capi.instance, phrase.as_ptr())
                }
            },
            |_| {
                // SAFETY: setup just parsed into the same live instance.
                unsafe {
                    let guessed =
                        pinyin_guess_candidates(capi.instance, GUESS_MID_OFFSET, W8_SORT as c_uint);
                    let mut count: c_uint = 0;
                    let _ = pinyin_get_n_candidate(capi.instance, &mut count);
                    black_box((guessed, count))
                }
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn take_sentence(instance: Instance) -> bool {
    let mut sentence: *mut c_char = std::ptr::null_mut();
    // SAFETY: `instance` is a live handle; `pinyin_get_sentence` writes a
    // malloc'd buffer the caller frees with libc `free`.
    let ok = unsafe { pinyin_get_sentence(instance, 0, &mut sentence) };
    if !sentence.is_null() {
        // SAFETY: the buffer came from `pinyin_get_sentence` (libc malloc).
        unsafe {
            free(sentence.cast());
        }
    }
    ok
}

fn guess_sentence_get_sentence(criterion: &mut Criterion) {
    let capi = CapiBench::open();
    let phrase = CString::new(PARSE_MEDIUM).expect("medium");

    criterion.bench_function("guess_sentence_get_sentence_0", |bencher| {
        bencher.iter_batched(
            || {
                // SAFETY: `capi.instance` is a live handle for this bench.
                unsafe {
                    let _ = pinyin_reset(capi.instance);
                    pinyin_parse_more_full_pinyins(capi.instance, phrase.as_ptr())
                }
            },
            |_| {
                // SAFETY: setup just parsed into the same live instance.
                let guessed = unsafe { pinyin_guess_sentence(capi.instance) };
                let fetched = take_sentence(capi.instance);
                black_box((guessed, fetched))
            },
            BatchSize::SmallInput,
        );
    });
}

struct TempStorePath(PathBuf);

impl Drop for TempStorePath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn user_store_count_delta(criterion: &mut Criterion) {
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-stage2-count-delta-{}.redb",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let guard = TempStorePath(path.clone());
    let mut store = UserStore::open(&path).expect("user store opens");
    store
        .observe_selection(SENTENCE_START, HOT_TOKEN)
        .expect("populate hot token");

    criterion.bench_function("user_store_count_delta_hot_token", |bencher| {
        bencher.iter(|| {
            black_box(
                store
                    .count_delta(Some(SENTENCE_START), HOT_TOKEN)
                    .expect("count_delta"),
            )
        });
    });
    drop(guard);
}

fn config() -> Criterion {
    print_pin_header();
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(4))
        // Optional large-regression gate vs `--save-baseline` / `--baseline`:
        // changes under 50% are treated as noise. CI does not run this.
        .noise_threshold(0.50)
}

criterion_group! {
    name = benches;
    config = config();
    targets = parse_more_full_pinyins, guess_candidates, guess_sentence_get_sentence, user_store_count_delta
}
criterion_main!(benches);
