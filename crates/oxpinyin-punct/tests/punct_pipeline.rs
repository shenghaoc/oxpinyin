//! End-to-end punctuation pipeline: per-index count+prune, cross-index
//! merge+prune, canonical table — the `genpunct.py` two-stage flow, with
//! small thresholds so a golden is hand-verifiable.

use oxpinyin_punct::PunctCounts;

/// Build a per-index table from segmented docs, pruned at `per_index`.
fn index_table(docs: &[&str], per_index: u64) -> PunctCounts {
    let mut counts = PunctCounts::new();
    for doc in docs {
        counts.add_document(doc).expect("count");
    }
    counts.prune(per_index);
    counts
}

#[test]
fn two_stage_pipeline_golden() {
    // Index A: 甲。×3, 甲，×1 ; Index B: 甲。×2, 乙？×3.
    let a = index_table(
        &["10 甲\n0 。\n10 甲\n0 。\n10 甲\n0 。\n10 甲\n0 ，\n"],
        2, // per-index threshold: drops 甲，×1
    );
    let b = index_table(
        &["10 甲\n0 。\n10 甲\n0 。\n20 乙\n0 ？\n20 乙\n0 ？\n20 乙\n0 ？\n"],
        2,
    );

    // 甲， was pruned in A (freq 1 < 2).
    assert_eq!(a.frequency(10, "甲", "，"), 0);
    assert_eq!(a.frequency(10, "甲", "。"), 3);

    // Cross-index merge then global prune at 4.
    let mut global = PunctCounts::new();
    global.merge(&a);
    global.merge(&b);
    // 甲。 = 3 + 2 = 5 ; 乙？ = 3.
    assert_eq!(global.frequency(10, "甲", "。"), 5);
    assert_eq!(global.frequency(20, "乙", "？"), 3);

    global.prune(4); // drops 乙？ (3 < 4), keeps 甲。 (5)
    assert_eq!(
        global.to_table(),
        "10 甲 。 5\n",
        "only 甲。 survives the global prune"
    );
}

#[test]
fn pipeline_is_deterministic() {
    let docs = ["10 甲\n0 。\n30 丙\n0 ，\n10 甲\n0 。\n", "20 乙\n0 ？\n"];
    let run = || {
        let mut counts = PunctCounts::new();
        for doc in &docs {
            counts.add_document(doc).expect("count");
        }
        counts.to_table()
    };
    assert_eq!(run(), run());
}
