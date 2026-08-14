//! Criterion baseline for the window-scan construction.
//!
//! Groups:
//! - `keystroke_cycle`: per-character `type_pinyin` on [`CYCLE_INPUTS`]
//! - `prefix_probe`: isolated `SEARCH_CONTINUED` binary search
//! - `parse_interpolation2`: isolated model-file parse

#![allow(missing_docs)]

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use pinyin_data::parse_interpolation2;

#[path = "support/mod.rs"]
mod harness;

use harness::{
    CYCLE_INPUTS, load_prefix_tables, load_real_tables, prefix_probe, real_session, type_keystrokes,
};

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

fn prefix_probe_isolated(criterion: &mut Criterion) {
    let (pinyin_keys, initial_keys) = load_prefix_tables();
    let samples = [
        "ni",
        "ni'hao",
        "zhong'guo",
        "xian",
        "xi'an",
        "fan",
        "fang'an",
        "q",
        "q'q'q",
        "chua",
        "caisho",
        "wai'meng'gu",
    ];
    criterion.bench_function("prefix_probe_12_needles", |bencher| {
        bencher.iter(|| {
            let mut hits = 0_usize;
            for needle in samples {
                if prefix_probe(black_box(&pinyin_keys), needle) {
                    hits += 1;
                }
                if prefix_probe(black_box(&initial_keys), needle) {
                    hits += 1;
                }
            }
            black_box(hits);
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

fn config() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(4))
}

criterion_group! {
    name = benches;
    config = config();
    targets = keystroke_cycle, prefix_probe_isolated, parse_interp
}
criterion_main!(benches);
