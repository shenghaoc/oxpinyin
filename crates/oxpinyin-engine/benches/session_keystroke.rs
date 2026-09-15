//! Benchmarks for the engine's end-to-end keystroke path: session
//! construction, keystroke processing (parse -> graph -> score -> candidates),
//! and candidate selection.
//!
//! The session is driven through its public API with fixture-backed test
//! doubles from `oxpinyin-testsupport`. The internal scan matrix and n-best
//! trellis are exercised indirectly — their types are crate-private, so the
//! bench exercises the same code path a real consumer does.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use oxpinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_engine::{EmptyConfigSource, KeyInput, LogicalKey, Session, StoragePaths};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

const MINI_VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const MINI_BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

fn fixture_session() -> Session<FixtureDictionary, FixtureLanguageModel> {
    let dict = FixtureDictionary::parse(MINI_VOCAB).expect("mini-vocab fixture");
    let model = FixtureLanguageModel::parse(MINI_VOCAB, MINI_BIGRAM).expect("mini-bigram fixture");
    Session::new(
        &EmptyConfigSource,
        StoragePaths::new("/tmp/oxpinyin-bench"),
        dict,
        model,
    )
    .expect("session construction")
}

/// An empty session type for isolating session construction cost from
/// keystroke cost — no dictionary overhead.
struct EmptyDict;
impl Dictionary for EmptyDict {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = std::convert::Infallible;
    fn lookup(&self, _: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Self::Error> {
        Ok(Vec::new())
    }
}

struct EmptyModel;
impl LanguageModel for EmptyModel {
    type Token = PhraseToken;
    type Error = std::convert::Infallible;
    fn score(
        &self,
        _: &[PhraseToken],
        _: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        Ok(edge_cost)
    }
}

// ── session construction ─────────────────────────────────────────────

fn bench_session_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_session_new");

    group.bench_function("empty_backends", |b| {
        b.iter(|| {
            Session::new(
                &EmptyConfigSource,
                StoragePaths::new("/tmp/oxpinyin-bench"),
                EmptyDict,
                EmptyModel,
            )
        });
    });

    group.bench_function("fixture_backends", |b| {
        let dict = FixtureDictionary::parse(MINI_VOCAB).expect("mini-vocab fixture");
        let model =
            FixtureLanguageModel::parse(MINI_VOCAB, MINI_BIGRAM).expect("mini-bigram fixture");
        b.iter(|| {
            Session::new(
                &EmptyConfigSource,
                StoragePaths::new("/tmp/oxpinyin-bench"),
                dict.clone(),
                model.clone(),
            )
        });
    });

    group.finish();
}

// ── keystroke processing ─────────────────────────────────────────────

fn bench_process_key(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_process_key");

    // Measure one keystroke on a fresh session — the first character.
    group.bench_function("first_char_n", |b| {
        b.iter_batched(
            fixture_session,
            |mut session| {
                let result = session
                    .process_key(&KeyInput::character('n'))
                    .expect("keystroke");
                black_box(result)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Measure the keystroke that completes a two-syllable input: five
    // characters already typed, then the 'o' of 'nihao'.
    group.bench_function("completing_nihao", |b| {
        b.iter_batched(
            || {
                let mut session = fixture_session();
                for ch in "niha".chars() {
                    session.process_key(&KeyInput::character(ch)).expect("type");
                }
                session
            },
            |mut session| {
                let result = session
                    .process_key(&KeyInput::character('o'))
                    .expect("keystroke");
                black_box(result)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ── full composition: type all characters then commit ────────────────

fn bench_full_composition(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_full_composition");

    let inputs: &[(&str, &str)] = &[
        ("short_ni", "ni"),
        ("medium_nihao", "nihao"),
        ("long_nihaoshijie", "nihaoshijie"),
    ];

    for (label, text) in inputs {
        group.bench_with_input(BenchmarkId::new("compose", label), text, |b, text| {
            b.iter_batched(
                fixture_session,
                |mut session| {
                    for ch in text.chars() {
                        session.process_key(&KeyInput::character(ch)).expect("type");
                    }
                    let outcome = session
                        .process_key(&KeyInput::plain(LogicalKey::Enter))
                        .expect("commit");
                    black_box(outcome)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ── incremental keystroke: type one character at a time ──────────────

fn bench_incremental_typing(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_incremental_typing");

    // Measure the cost of typing "nihao" character by character,
    // including the parse/graph/score rebuild on each keystroke.
    group.bench_function("nihao_5_keystrokes", |b| {
        b.iter_batched(
            fixture_session,
            |mut session| {
                for ch in "nihao".chars() {
                    black_box(session.process_key(&KeyInput::character(ch)).expect("type"));
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("zhongguoren_10_keystrokes", |b| {
        b.iter_batched(
            fixture_session,
            |mut session| {
                for ch in "zhongguoren".chars() {
                    black_box(session.process_key(&KeyInput::character(ch)).expect("type"));
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ── backspace: erase and reparse ─────────────────────────────────────

fn bench_backspace(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine_backspace");

    group.bench_function("erase_last_of_nihao", |b| {
        b.iter_batched(
            || {
                let mut session = fixture_session();
                for ch in "nihao".chars() {
                    session.process_key(&KeyInput::character(ch)).expect("type");
                }
                session
            },
            |mut session| {
                let outcome = session
                    .process_key(&KeyInput::plain(LogicalKey::Backspace))
                    .expect("backspace");
                black_box(outcome)
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_session_new,
    bench_process_key,
    bench_full_composition,
    bench_incremental_typing,
    bench_backspace,
);
criterion_main!(benches);
