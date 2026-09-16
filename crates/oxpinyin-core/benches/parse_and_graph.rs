//! Benchmarks for the latency-critical parsing and graph paths in
//! `oxpinyin-core`: full-pinyin parsing, segment-graph construction,
//! fewest-keys extraction, k-best search, and syllable-key lookups.
//!
//! These are the Stage 2 baseline measurements for the IME's input-to-
//! candidate decode chain. No I/O, no external data — the parser, graph
//! and k-best search are pure functions over the frozen syllable inventory.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::kbest;
use oxpinyin_core::scoring::{ScoringConfig, expand_keys};
use oxpinyin_core::{
    Cost, FullPinyinParser, InputParser, OptionBits, PINYIN_INCOMPLETE, SyllableKey,
};

// ── parsing ──────────────────────────────────────────────────────────

fn bench_parse(c: &mut Criterion) {
    let parser = FullPinyinParser;
    let mut group = c.benchmark_group("core_parse");

    let cases: &[(&str, &[u8])] = &[
        ("short_ni", b"ni"),
        ("medium_nihao", b"nihao"),
        ("long_nihaoshijie", b"nihaoshijie"),
        ("ambiguous_xian", b"xian"),
        ("ambiguous_fangan", b"fangan"),
        ("separated", b"xi'an"),
        ("partial_nih", b"nih"),
        ("initials_only", b"zzzzzzzz"),
    ];

    for (label, input) in cases {
        group.bench_with_input(BenchmarkId::new("parse", label), input, |b, input| {
            b.iter(|| parser.parse(black_box(input)));
        });
    }
    group.finish();
}

fn bench_parse_with_options(c: &mut Criterion) {
    let parser = FullPinyinParser;
    let options = OptionBits::from_bits(
        PINYIN_INCOMPLETE | oxpinyin_core::PINYIN_CORRECT_ALL | oxpinyin_core::PINYIN_AMB_ALL,
    );
    let mut group = c.benchmark_group("core_parse_with_options");

    for (label, input) in &[
        ("medium_nihao", b"nihao" as &[u8]),
        ("long_nihaoshijie", b"nihaoshijie"),
        ("ambiguous_xian", b"xian"),
    ] {
        group.bench_with_input(BenchmarkId::new("options", label), input, |b, input| {
            b.iter(|| parser.parse_with_options(black_box(input), options));
        });
    }
    group.finish();
}

// ── segment graph ────────────────────────────────────────────────────

fn bench_graph_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_graph_build");

    let cases: &[(&str, &[u8])] = &[
        ("short_ni", b"ni"),
        ("medium_nihao", b"nihao"),
        ("long_nihaoshijie", b"nihaoshijie"),
        ("sentence", b"zhongguorenminjiefangjun"),
        ("initials_zz", b"zzzzzzzz"),
        ("separated", b"ni'hao'shi'jie"),
    ];

    for (label, input) in cases {
        group.bench_with_input(BenchmarkId::new("build", label), input, |b, input| {
            b.iter(|| SegmentGraph::build(black_box(input)));
        });
    }
    group.finish();
}

fn bench_graph_build_with_options(c: &mut Criterion) {
    let options = OptionBits::from_bits(PINYIN_INCOMPLETE);
    let mut group = c.benchmark_group("core_graph_build_with_options");

    for (label, input) in &[
        ("medium_nihao", b"nihao" as &[u8]),
        ("sentence", b"zhongguorenminjiefangjun"),
    ] {
        group.bench_with_input(BenchmarkId::new("options", label), input, |b, input| {
            b.iter(|| SegmentGraph::build_with_options(black_box(input), options));
        });
    }
    group.finish();
}

fn bench_fewest_keys(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_fewest_keys");

    let cases: &[(&str, &[u8])] = &[
        ("medium_nihao", b"nihao"),
        ("long_nihaoshijie", b"nihaoshijie"),
        ("sentence", b"zhongguorenminjiefangjun"),
        ("ambiguous_xian", b"xian"),
    ];

    for (label, input) in cases {
        let graph = SegmentGraph::build(input).expect("test inputs are short");
        group.bench_with_input(
            BenchmarkId::new("complete_only", label),
            &graph,
            |b, graph| {
                b.iter(|| graph.fewest_keys(black_box(false)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("allow_incomplete", label),
            &graph,
            |b, graph| {
                b.iter(|| graph.fewest_keys(black_box(true)));
            },
        );
    }
    group.finish();
}

// ── k-best search ────────────────────────────────────────────────────

/// One cost unit per edge: the cheapest path is the one with fewest edges.
fn per_edge(
    _previous: Option<&oxpinyin_core::graph::Edge>,
    _edge: &oxpinyin_core::graph::Edge,
) -> Cost {
    1
}

fn bench_k_best(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_k_best");

    let cases: &[(&str, &[u8], usize)] = &[
        ("nihao_k1", b"nihao", 1),
        ("nihao_k8", b"nihao", 8),
        ("nihaoshijie_k1", b"nihaoshijie", 1),
        ("nihaoshijie_k8", b"nihaoshijie", 8),
        ("sentence_k1", b"zhongguorenminjiefangjun", 1),
        ("sentence_k8", b"zhongguorenminjiefangjun", 8),
        ("ambiguous_fangan_k8", b"fangan", 8),
        ("xian_k4", b"xian", 4),
    ];

    for (label, input, k) in cases {
        let graph = SegmentGraph::build(input).expect("test inputs are short");
        group.bench_with_input(
            BenchmarkId::new("k_best", label),
            &(&graph, *k),
            |b, (graph, k)| {
                b.iter(|| kbest::k_best(black_box(graph), &per_edge, *k));
            },
        );
    }
    group.finish();
}

// ── syllable key lookups ─────────────────────────────────────────────

fn bench_syllable_key_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_syllable_key");

    let spellings: &[&str] = &[
        "ni", "hao", "zhong", "guo", "ren", "min", "a", "zhuang", "lv",
    ];

    group.bench_function("from_text_hit", |b| {
        b.iter(|| {
            for spelling in spellings {
                black_box(SyllableKey::from_text(black_box(spelling)));
            }
        });
    });

    group.bench_function("from_text_miss", |b| {
        b.iter(|| {
            for miss in &["qqq", "NI", "ni2", "", "xyz"] {
                black_box(SyllableKey::from_text(black_box(miss)));
            }
        });
    });

    let options_all =
        OptionBits::from_bits(oxpinyin_core::PINYIN_CORRECT_ALL | oxpinyin_core::PINYIN_AMB_ALL);
    let aliases: &[&str] = &["agn", "amg", "diou", "duei", "lue", "jv"];

    group.bench_function("from_option_text_alias", |b| {
        b.iter(|| {
            for alias in aliases {
                black_box(SyllableKey::from_option_text(black_box(alias), options_all));
            }
        });
    });

    group.finish();
}

// ── key expansion ────────────────────────────────────────────────────

fn bench_expand_keys(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_expand_keys");
    let config = ScoringConfig::default();

    // Complete keys only: 1 sequence.
    let complete = vec![
        SyllableKey::from_text("ni").unwrap(),
        SyllableKey::from_text("hao").unwrap(),
    ];
    group.bench_function("complete_nihao", |b| {
        b.iter(|| expand_keys(black_box(&complete), config.expansion_limit));
    });

    // One incomplete key: Cartesian product with one initial.
    let one_incomplete = vec![
        SyllableKey::from_text("ni").unwrap(),
        SyllableKey::from_text("h").unwrap(),
    ];
    group.bench_function("one_incomplete_nih", |b| {
        b.iter(|| expand_keys(black_box(&one_incomplete), config.expansion_limit));
    });

    // Two incomplete keys: product may exceed the limit.
    let two_incomplete = vec![
        SyllableKey::from_text("h").unwrap(),
        SyllableKey::from_text("h").unwrap(),
    ];
    group.bench_function("two_incomplete_hh", |b| {
        b.iter(|| expand_keys(black_box(&two_incomplete), config.expansion_limit));
    });

    group.finish();
}

// ── fuzzy alternatives ───────────────────────────────────────────────

fn bench_fuzzy_alternatives(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_fuzzy");

    let options = OptionBits::from_bits(oxpinyin_core::PINYIN_AMB_ALL);
    let keys: Vec<SyllableKey> = ["can", "lan", "zhong", "ni", "shi"]
        .iter()
        .map(|s| SyllableKey::from_text(s).unwrap())
        .collect();

    group.bench_function("alternatives_all_amb", |b| {
        b.iter(|| {
            for key in &keys {
                black_box(key.fuzzy_alternatives(options));
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_parse_with_options,
    bench_graph_build,
    bench_graph_build_with_options,
    bench_fewest_keys,
    bench_k_best,
    bench_syllable_key_lookup,
    bench_expand_keys,
    bench_fuzzy_alternatives,
);
criterion_main!(benches);
