//! `UserStore::phrase(token)` over a pre-seeded store: one point-get of the
//! phrase text plus a range scan of the token's 16 pronunciation rows —
//! the public surface over the private `pronunciation_range` bounds. The
//! backend is whichever store feature the bench is compiled with.
//!
//! Synthetic data is deterministic: 64 phrases `phrase_{i}`, 16 distinct
//! pronunciations each, keys in 1..=300 (renderable syllables). No model
//! fixture.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxpinyin_user::{Token, UserStore};
use std::hint::black_box;
use std::path::{Path, PathBuf};

const PHRASES: u32 = 64;
const PRONUNCIATIONS_PER_PHRASE: u16 = 16;

fn temp_path() -> PathBuf {
    let path = std::env::temp_dir().join(oxpinyin_store::default_store_file(&format!(
        "oxpinyin-bench-phrase-read-{}",
        std::process::id()
    )));
    cleanup(&path);
    path
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    // LMDB's NO_SUB_DIR lock sidecar.
    let _ = std::fs::remove_file(format!("{}-lock", path.display()));
}

/// Seed the store; returns the tokens in insertion order.
fn seed(store: &mut UserStore) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(PHRASES as usize);
    for i in 0..PHRASES {
        let text = format!("phrase_{i}");
        let chars = text.chars().count();
        let mut token = 0;
        for j in 0..PRONUNCIATIONS_PER_PHRASE {
            // Same text, distinct key sequence: one token, 16 pronunciations.
            let keys: Vec<u16> = (0..chars)
                .map(|k| {
                    u16::try_from(1 + (i as usize + k) % 200).unwrap_or(u16::MAX)
                        + u16::from(k == 0) * j
                })
                .collect();
            token = store.add_phrase(&text, &keys, Some(3)).expect("add_phrase");
        }
        tokens.push(token);
    }
    tokens
}

// Canary for the phrase-read path: calls UserStore::phrase (the public
// path over pronunciation_range), a point-get plus a range scan.
// This bench cannot resolve sub-µs allocation changes (F1/F3 in
// #340): the dominant cost is the redb read-txn + cursor open
// (~2.4 µs), which swamps the removed 4-byte Vec allocations.
// Its value is as a regression detector, not an improvement signal.
fn bench_phrase_read(c: &mut Criterion) {
    let path = temp_path();
    let mut store = UserStore::create_standalone(&path).expect("create");
    let tokens = seed(&mut store);
    let token = tokens[PHRASES as usize / 2];
    let sanity = store.phrase(token).expect("read").expect("present");
    assert_eq!(
        sanity.pronunciations().len(),
        usize::from(PRONUNCIATIONS_PER_PHRASE)
    );

    let mut group = c.benchmark_group("user_phrase_read");
    group.bench_with_input(
        BenchmarkId::new("pron_range", PRONUNCIATIONS_PER_PHRASE),
        &token,
        |b, &token| {
            b.iter(|| store.phrase(black_box(token)).expect("read"));
        },
    );
    group.finish();

    drop(store);
    cleanup(&path);
}

criterion_group!(benches, bench_phrase_read);
criterion_main!(benches);
