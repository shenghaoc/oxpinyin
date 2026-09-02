//! Integration tests for `BigramLanguageModel` over `bigram.db` and the
//! chunk-file unigrams.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::{LanguageModel, PhraseToken, UserCountDelta};
use oxpinyin_data::{BigramLanguageModel, PhraseLibraries, SYSTEM_LIBRARY_FILES, SystemDbm};
use oxpinyin_store::{DefaultStore, WriteStore};

mod support;
use support::ChunkBuilder;

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

const NI: u32 = 0x0100_0010;
const HAO: u32 = 0x0100_0011;
const NIHAO: u32 = 0x0100_0099;
const GBK: u32 = 0x0200_0001;

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("oxpinyin-lm-test-{}-{}", std::process::id(), n,));
        std::fs::create_dir_all(&dir).unwrap();
        // gb_char: 你 count 9 (+1 = 10), 好 count 4 (+1 = 5), 你好 never
        // seen (+1 = 1) → total 16, items 3.
        let mut gb = ChunkBuilder::new(16);
        gb.add(0x10, 10, "你", vec![(vec![0, 0], 1)]);
        gb.add(0x11, 5, "好", vec![(vec![0, 0], 1)]);
        gb.add(0x99, 1, "你好", vec![(vec![0, 0, 0, 0], 1)]);
        std::fs::write(dir.join("gb_char.bin"), gb.build()).unwrap();
        // gbk_char: one item, count 2 (+1 = 3).
        let mut gbk = ChunkBuilder::new(3);
        gbk.add(0x01, 3, "鎄", vec![(vec![0, 0], 1)]);
        std::fs::write(dir.join("gbk_char.bin"), gbk.build()).unwrap();
        // bigram.db: 你 → 好 ×7 (row total 7).
        let store = DefaultStore::create_hash(&dir.join(SystemDbm::Bigram.file_name())).unwrap();
        store
            .write(|txn| {
                let mut value = 7_u32.to_le_bytes().to_vec();
                value.extend_from_slice(&HAO.to_le_bytes());
                value.extend_from_slice(&7_u32.to_le_bytes());
                txn.put_raw(&NI.to_le_bytes(), &value)
            })
            .unwrap();
        Self { dir }
    }

    fn open(&self) -> (BigramLanguageModel, Arc<AtomicU32>) {
        let libraries = Arc::new(PhraseLibraries::open(&self.dir, SYSTEM_LIBRARY_FILES).unwrap());
        let mask = Arc::new(AtomicU32::new(0));
        let lm = BigramLanguageModel::open_with_mask(
            &self.dir.join(SystemDbm::Bigram.file_name()),
            libraries,
            Arc::clone(&mask),
        )
        .unwrap();
        (lm, mask)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn unigrams_are_the_chunk_item_fields() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    assert_eq!(
        lm.unigram_count(NI),
        Some(10),
        "\\1-gram 9 + gen_unigram's 1"
    );
    assert_eq!(
        lm.unigram_count(NIHAO),
        Some(1),
        "never seen: still 1, never 0"
    );
    assert_eq!(lm.unigram_count(GBK), Some(3));
    assert_eq!(lm.unigram_count(0x0300_0001), None, "no such library");
    assert_eq!(lm.unigram_total(), 16 + 3, "Σ item over both chunks");
    assert!(lm.has_unigrams());
    assert!(lm.has_real_unigrams());
    assert_eq!(
        LanguageModel::unigram_freq(&lm, &PhraseToken::new(NI)).unwrap(),
        Some(10)
    );
    assert_eq!(LanguageModel::unigram_total(&lm).unwrap(), Some(19));
}

#[test]
fn an_unloaded_library_leaves_the_model() {
    let fix = Fixture::new();
    let (lm, mask) = fix.open();
    mask.store(1 << 2, Ordering::SeqCst);
    assert_eq!(lm.unigram_count(GBK), None);
    assert_eq!(lm.unigram_total(), 16, "gbk's total leaves");
    assert_eq!(lm.unigram_count(NI), Some(10));
}

#[test]
fn bigram_rows_are_point_reads() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let row = lm.load_successors(NI).unwrap().expect("你 has a row");
    assert_eq!(row.total, 7);
    assert_eq!(row.records, vec![(HAO, 7)]);
    assert!(lm.load_successors(HAO).unwrap().is_none());
}

#[test]
fn scoring_blends_bigram_and_unigram_over_the_corpus_counts() {
    let fix = Fixture::new();
    let (lm, _) = fix.open();
    let ni = PhraseToken::new(NI);
    let hao = PhraseToken::new(HAO);
    let nihao = PhraseToken::new(NIHAO);
    // 好 after 你: a seen transition costs less than the unigram alone.
    let seen = lm.score(&[ni], &hao, 0).unwrap();
    let unseen = lm.score(&[hao], &ni, 0).unwrap();
    assert!(seen < unseen, "seen {seen} vs unseen {unseen}");
    // A never-seen phrase carries gen_unigram's 1: finite, never the floor.
    let rare = lm.score(&[], &nihao, 0).unwrap();
    assert!(rare < oxpinyin_core::cost::UNKNOWN_COST);
    assert!(rare > lm.score(&[], &ni, 0).unwrap(), "rarer than 你");
    // A token with no item at all does floor.
    let absent = lm.score(&[], &PhraseToken::new(0x0300_0001), 0).unwrap();
    assert_eq!(absent, oxpinyin_core::cost::UNKNOWN_COST);
    // The user overlay raises the rare one.
    let raised = lm
        .score_with_user_delta(
            &[],
            &nihao,
            0,
            UserCountDelta {
                unigram_delta: 3,
                unigram_total_delta: 3,
                ..UserCountDelta::ZERO
            },
        )
        .unwrap();
    assert!(raised < rare);
    // n-best step costs carry both branches for the seen transition.
    let step = lm.nbest_step_costs(&ni, &hao).unwrap();
    assert!(step.blended.is_some());
    assert!(step.unigram.is_some());
}
