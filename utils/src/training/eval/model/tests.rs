use super::EvalLanguageModel;
use crate::storage::Hash;
use oxpinyin::data::PhraseLibraries;
use oxpinyin::data::phrase_library::{ChunkItem, build_chunk};
use oxpinyin::data::row_format::encode_token_key;
use oxpinyin::single_gram::encode_single_gram;

#[test]
fn the_float_probabilities_are_the_stored_ratios() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gb.bin");
    let first = ChunkItem {
        phrase: vec!['中' as u32],
        unigram: 6,
        prons: vec![],
    };
    let second = ChunkItem {
        phrase: vec!['钟' as u32],
        unigram: 2,
        prons: vec![],
    };
    std::fs::write(&path, build_chunk(&[(10, &first), (20, &second)]).unwrap()).unwrap();
    let mut libraries = PhraseLibraries::empty();
    oxpinyin_tool_support::data::load_heap_library(&mut libraries, 1, &path).unwrap();
    let bigram = Hash::create(&dir.path().join("bigram.db")).unwrap();
    // <start> → 中 twice; 中 → 中 once and 中 → 钟 four times.
    bigram
        .put(
            &encode_token_key(1),
            &encode_single_gram(2, &[(0x0100_000a, 2)]),
        )
        .unwrap();
    bigram
        .put(
            &encode_token_key(0x0100_000a),
            &encode_single_gram(5, &[(0x0100_000a, 1), (0x0100_0014, 4)]),
        )
        .unwrap();
    let model = EvalLanguageModel::new(&libraries, Some(&bigram), 0.312_699);
    assert_eq!(
        model.unigram_poss(0x0100_000a).to_bits(),
        0.75_f64.to_bits()
    );
    let row = model.bigram_row(0x0100_000a).unwrap();
    assert_eq!(row.records, [(0x0100_000a, 1), (0x0100_0014, 4)]);
    assert_eq!(row.total, 5);
    assert_eq!(row.records[1].1 as f32 / row.total as f32, 0.8);
    assert!(model.bigram_row(0x0100_0014).is_none());

    // Zero totals and counts are cases of the same stored-ratio partition.
    {
        // phonetic_lookup.h:650-651,678-679 and ngram.cpp:164-171 at
        // 074a2219 divide by stored totals, including zero.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zero.bin");
        let item = ChunkItem {
            phrase: vec!['中' as u32],
            unigram: 0,
            prons: vec![],
        };
        std::fs::write(&path, build_chunk(&[(10, &item)]).unwrap()).unwrap();
        let mut libraries = PhraseLibraries::empty();
        oxpinyin_tool_support::data::load_heap_library(&mut libraries, 1, &path).unwrap();
        let bigram = Hash::create(&dir.path().join("bigram.db")).unwrap();
        // A zero-total row is present even when its records sum to a nonzero
        // count. A stored zero successor is distinct from an absent row.
        bigram
            .put(
                &encode_token_key(1),
                &encode_single_gram(0, &[(0x0100_000a, 4)]),
            )
            .unwrap();
        bigram
            .put(
                &encode_token_key(0x0100_000a),
                &encode_single_gram(0, &[(0x0100_000a, 0)]),
            )
            .unwrap();
        let model = EvalLanguageModel::new(&libraries, Some(&bigram), 0.312_699);
        assert!(model.unigram_poss(0x0100_000a).is_nan());
        let row = model.bigram_row(1).unwrap();
        assert_eq!(row.total, 0);
        assert_eq!(row.records, [(0x0100_000a, 4)]);
        assert!((row.records[0].1 as f32 / row.total as f32).is_infinite());
        let row = model.bigram_row(0x0100_000a).unwrap();
        assert_eq!(row.total, 0);
        assert_eq!(row.records, [(0x0100_000a, 0)]);
        assert!((row.records[0].1 as f32 / row.total as f32).is_nan());
        assert!(model.bigram_row(0x0100_0014).is_none());
    }
}
