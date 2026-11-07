//! `UserStore::export_phrases` at N = 64, 256, 1024 phrases, one
//! pronunciation each, over the libpinyin session the shipped libraries
//! open: the export walks the user library's arena and renders each
//! reading. The backend is whichever store feature the bench is compiled
//! with; it decides the user bigram container only.
//!
//! Each session is seeded once per N outside the criterion loop; only the
//! export call is timed. Synthetic data is deterministic: phrase `i` is
//! `phrase_{i}` with one key per character, keys drawn from the frozen
//! syllable inventory, fixed count. No model fixture.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxpinyin_core::SyllableKey;
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin_user::{PinyinKey, SystemOriginals, UserConfLaw, UserStore};

/// The pin's NULL-user-directory session over the stock layout.
fn session() -> UserStore {
    UserStore::open_transient(
        SystemOriginals::new(Default::default(), UserFileLayout::stock()),
        SystemVersions::for_this_build(7, 14),
        UserConfLaw::Pinyin,
    )
    .expect("open_transient")
}

/// A slice of the frozen syllable inventory the seeded readings draw on.
const SYLLABLES: &[&str] = &[
    "a", "ai", "an", "ba", "bai", "ban", "bei", "ben", "bi", "bo", "bu", "cha", "chen", "chi",
    "da", "de", "di", "dong", "du", "fa", "fei", "fu", "ge", "gu", "guo", "hao", "he", "hui",
    "jia", "jie", "jin", "ke", "lai", "li", "ma", "mei", "ming", "ni", "ren", "shi", "shu", "ta",
    "tian", "wo", "xi", "xin", "ya", "yi", "you", "zai", "zhi", "zhong",
];

/// The `index`th syllable of the slice above, toneless.
fn key(index: usize) -> PinyinKey {
    let syllable =
        SyllableKey::from_text(SYLLABLES[index % SYLLABLES.len()]).expect("frozen syllable");
    PinyinKey::try_from(syllable.index()).expect("frozen syllable inventory fits u16")
}

fn seed(store: &mut UserStore, n: u32) {
    for i in 0..n {
        let text = format!("phrase_{i}");
        let keys: Vec<PinyinKey> = (0..text.chars().count())
            .map(|j| key(1 + i as usize + j))
            .collect();
        store
            .add_phrase(&oxpinyin_core::PhraseText::from(&text), &keys, Some(3))
            .expect("add_phrase");
    }
}

fn bench_export(c: &mut Criterion) {
    let mut group = c.benchmark_group("user_export_phrases");
    for n in [64u32, 256, 1024] {
        let mut store = session();
        seed(&mut store, n);
        let rows = store.export_phrases().expect("export");
        assert_eq!(rows.len(), n as usize, "every seeded phrase renders");

        group.throughput(Throughput::Elements(u64::from(n)));
        group.bench_with_input(BenchmarkId::new("phrases", n), &n, |b, _| {
            b.iter(|| store.export_phrases().expect("export"));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_export);
criterion_main!(benches);
