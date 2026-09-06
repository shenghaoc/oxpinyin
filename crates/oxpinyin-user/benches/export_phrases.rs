//! `UserStore::export_phrases` at N = 64, 256, 1024 phrases, one
//! pronunciation each, so the export walks both the phrase and the
//! pronunciation tables. Exercises the single-walk pronunciation
//! collection (#341): one ordered scan instead of one range transaction
//! per phrase. The backend is whichever store feature the bench is
//! compiled with.
//!
//! Each store is seeded once per N outside the criterion loop; only the
//! export call is timed. Synthetic data is deterministic: phrase `i` is
//! `phrase_{i}` with one key per character, keys in 1..=300 (renderable
//! syllables), fixed count. No model fixture.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxpinyin_user::UserStore;
use std::path::{Path, PathBuf};

fn temp_path(n: u32) -> PathBuf {
    let path = std::env::temp_dir().join(oxpinyin_store::default_store_file(&format!(
        "oxpinyin-bench-export-{n}-{}",
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

fn seed(store: &mut UserStore, n: u32) {
    for i in 0..n {
        let text = format!("phrase_{i}");
        let keys: Vec<u16> = (0..text.chars().count())
            .map(|j| 1 + ((i as usize + j) % 300) as u16)
            .collect();
        store.add_phrase(&text, &keys, Some(3)).expect("add_phrase");
    }
}

fn bench_export(c: &mut Criterion) {
    let mut group = c.benchmark_group("user_export_phrases");
    for n in [64u32, 256, 1024] {
        let path = temp_path(n);
        let mut store = UserStore::create_standalone(&path).expect("create");
        seed(&mut store, n);
        let rows = store.export_phrases().expect("export");
        assert_eq!(rows.len(), n as usize, "every seeded phrase renders");

        group.throughput(Throughput::Elements(u64::from(n)));
        group.bench_with_input(BenchmarkId::new("phrases", n), &n, |b, _| {
            b.iter(|| store.export_phrases().expect("export"));
        });

        drop(store);
        cleanup(&path);
    }
    group.finish();
}

criterion_group!(benches, bench_export);
criterion_main!(benches);
