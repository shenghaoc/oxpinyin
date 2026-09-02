//! Criterion baseline for the window-scan construction.
//!
//! Groups:
//! - `keystroke_cycle`: per-character `type_pinyin` on [`CYCLE_INPUTS`]
//! - `prefix_probe`: isolated `SEARCH_CONTINUED` binary search
//! - `parse_interpolation2`: isolated model-file parse
//! - `decode_pass_user_store`: C-API-like LM overlay in three store states

#![allow(missing_docs)]

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use oxpinyin_core::{Cost, LanguageModel, PhraseToken, UserCountDelta};
use oxpinyin_data::{
    BigramLanguageModel, LmError, SystemDbm, SystemDictionary, parse_interpolation2,
};
use oxpinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use oxpinyin_user::{SENTENCE_START, UserStore};

#[path = "support/mod.rs"]
mod harness;

use harness::{CYCLE_INPUTS, load_real_tables, real_session, type_keystrokes};

/// Mirrors `oxpinyin-capi`'s `SharedLm`: dict unigrams plus an optional
/// [`UserStore`] overlay consulted on every `score` / `unigram_freq`.
struct BenchLm<'a> {
    inner: &'a BigramLanguageModel,
    user: Option<UserStore>,
}

struct TempStorePath(PathBuf);

impl Drop for TempStorePath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl LanguageModel for BenchLm<'_> {
    type Token = PhraseToken;
    type Error = LmError;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        let delta = match self.user.as_ref() {
            None => UserCountDelta::ZERO,
            Some(store) => store
                .count_delta(history.last().map(|t| t.value()), token.value())
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        self.inner
            .score_with_user_delta(history, token, edge_cost, delta)
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        let extra = match self.user.as_ref() {
            None => 0,
            Some(store) => store
                .unigram_delta(token.value())
                .map_err(|error| LmError::User(error.to_string()))?,
        };
        Ok(self
            .inner
            .unigram_freq_with_user_delta(token.value(), extra))
    }

    fn has_real_unigrams(&self) -> bool {
        self.inner.has_real_unigrams()
    }
}

/// Dictionary plus a language model whose unigrams come from the dictionary.
///
/// Deliberately not [`harness::load_real_tables`], which loads model20's
/// `interpolation2.text` and asserts `has_real_unigrams`. This bench measures
/// the *store overlay*: all three arms share one `BigramLanguageModel`, so the
/// comparison between them is internally valid, and dropping the model20
/// dependency keeps it runnable from an export dir alone. The scores are not
/// the pinned construction's, so absolute timings here do not compare against
/// `keystroke_cycle`.
fn load_scoring_tables() -> (SystemDictionary, BigramLanguageModel) {
    let export = harness::export_dir();
    let dict = SystemDictionary::open(&export).expect("SystemDictionary opens");
    let mut lm = BigramLanguageModel::open(
        &export.join(SystemDbm::Bigram.file_name()),
        std::sync::Arc::clone(dict.libraries()),
    )
    .expect("BigramLanguageModel opens");
    lm.set_lambda_from_table_conf(&export.join("table.conf"));
    (dict, lm)
}

fn bench_store_path(tag: &str) -> TempStorePath {
    // Suffix follows the compiled-in backend (e.g. `.kct` under the KC
    // default) — the file is opened through `UserStore`, which uses the
    // same backend, so the extension is naming only.
    let path = std::env::temp_dir().join(format!(
        "pinyin-oracle-decode-pass-{tag}-{}.{}",
        std::process::id(),
        oxpinyin_data::DEFAULT_STORE_EXT,
    ));
    let _ = std::fs::remove_file(&path);
    TempStorePath(path)
}

fn decode_session<'a>(
    dict: &'a SystemDictionary,
    lm: &'a BenchLm<'a>,
) -> Session<&'a SystemDictionary, &'a BenchLm<'a>> {
    Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm).expect("Session::new")
}

fn decode_cycle(session: &mut Session<&SystemDictionary, &BenchLm<'_>>) {
    for input in CYCLE_INPUTS {
        harness::type_batch(session, black_box(input));
        black_box(session.candidates().len());
    }
}

fn keystroke_cycle(criterion: &mut Criterion) {
    let (dict, lm) = load_real_tables();
    let mut session = real_session(&dict, &lm);
    criterion.bench_function("keystroke_cycle_20_inputs", |bencher| {
        bencher.iter(|| {
            for input in CYCLE_INPUTS {
                type_keystrokes(&mut session, input);
                black_box(session.candidates().len());
            }
        });
    });
}

fn parse_interp(criterion: &mut Criterion) {
    let path = harness::interpolation2_path();
    criterion.bench_function("parse_interpolation2", |bencher| {
        bencher.iter(|| {
            let table = parse_interpolation2(black_box(&path)).expect("parse");
            black_box(table.len());
        });
    });
}

/// The three store states a decode can face, measured against one LM.
///
/// Read the arms against each other, not against absolute numbers: at
/// `sample_size(20)` on an unisolated machine the run-to-run band on a single
/// arm is roughly ±10%, wide enough to swallow a 13% difference. The
/// `no_store_attached` arm is the control — this bench's subject never runs
/// for it — so a shift on that row measures the machine, not the store. Only
/// a gap clearly wider than the control's own drift is evidence. To resolve
/// anything finer, raise the sample count and warm-up time and pin the CPU.
fn user_store_decode_pass(criterion: &mut Criterion) {
    let (dict, lm) = load_scoring_tables();

    let no_user_lm = BenchLm {
        inner: &lm,
        user: None,
    };
    let empty_path = bench_store_path("empty");
    let empty_store = UserStore::open(&empty_path.0).expect("empty user store opens");
    let empty_lm = BenchLm {
        inner: &lm,
        user: Some(empty_store),
    };
    let populated_path = bench_store_path("populated");
    let mut populated_store =
        UserStore::open(&populated_path.0).expect("populated user store opens");
    populated_store
        .observe_selection(SENTENCE_START, 0x0100_1225)
        .expect("populate common token");
    populated_store
        .observe_selection(0x0100_1225, 0x0100_05db)
        .expect("populate common transition");
    let populated_lm = BenchLm {
        inner: &lm,
        user: Some(populated_store),
    };

    let mut no_user_session = decode_session(&dict, &no_user_lm);
    let mut empty_session = decode_session(&dict, &empty_lm);
    let mut populated_session = decode_session(&dict, &populated_lm);

    let mut group = criterion.benchmark_group("decode_pass_user_store");
    group.bench_function("no_store_attached", |bencher| {
        bencher.iter(|| decode_cycle(&mut no_user_session));
    });
    group.bench_function("empty_store_attached", |bencher| {
        bencher.iter(|| decode_cycle(&mut empty_session));
    });
    group.bench_function("populated_store_attached", |bencher| {
        bencher.iter(|| decode_cycle(&mut populated_session));
    });
    group.finish();
}

fn config() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(4))
}

criterion_group! {
    name = benches;
    config = config();
    targets = keystroke_cycle, parse_interp, user_store_decode_pass
}
criterion_main!(benches);
