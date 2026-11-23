//! Hand-computable correction-rate fixture for the `eval_correction_rate`
//! decode.
//!
//! A three-token vocabulary with one homophone pair pins the decode exactly:
//! `中`(slot 10) and `钟`(slot 20) both spell `zhong`, `国`(slot 30) spells `guo`. `中` is
//! far more frequent than `钟` in both the unigram (100 vs 10) and after
//! `sentence_start` (bigram 30 vs 1), so `zhong` always decodes to `中` for
//! **any** λ ∈ [0, 1]. Therefore:
//!
//! * `中国` (中→国, bigram 40) decodes back to `中国` — passes;
//! * `钟` decodes to `中`, not `钟` — fails.
//!
//! Correction rate = 1 passed / 2 tested = 0.5, independent of λ.

use super::fixture_doubles;
use super::{EvalLanguageModel, LibraryPhraseSource, correction_rate};
use crate::storage::Hash;
use oxpinyin::data::PhraseLibraries;
use oxpinyin::data::phrase_library::{ChunkItem, build_chunk};
use oxpinyin::data::row_format::encode_token_key;
use oxpinyin::model::PhraseList;
use oxpinyin::parser::ChewingKey;
use oxpinyin::single_gram::encode_single_gram;
use oxpinyin::vocab::PhraseEntry;
use oxpinyin::vocab::{PhraseToken, SyllableKey};

const VOCAB: &str = "\
token=16777226\tkeys=zhong\ttext=中\tunigram=100
token=16777236\tkeys=zhong\ttext=钟\tunigram=10
token=16777246\tkeys=guo\ttext=国\tunigram=50
";

fn sentences() -> Vec<Vec<PhraseToken>> {
    vec![
        vec![0x0100_000a, 0x0100_001e], // 中国
        vec![0x0100_0014],              // 钟
    ]
}

#[test]
fn correction_rate_is_lambda_independent_here() {
    // The homophone ranking holds for every λ, so the rate is 0.5 whether λ
    // is 0 (pure unigram), 1 (pure bigram), or anything between.
    let dictionary = FixtureDictionary::parse(VOCAB).expect("vocab");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gb.bin");
    // The binary model after import_interpolation and gen_unigram: the
    // authored 100/10/50 counts plus their existing +1 floor.
    let items: Vec<_> = [
        (10, '中', "zhong", 101),
        (20, '钟', "zhong", 11),
        (30, '国', "guo", 51),
    ]
    .into_iter()
    .map(|(slot, text, spelling, unigram)| {
        (
            slot,
            ChunkItem {
                phrase: vec![text as u32],
                unigram,
                prons: vec![(
                    vec![ChewingKey::from_pinyin(spelling).unwrap().to_packed()],
                    1,
                )],
            },
        )
    })
    .collect();
    let items: Vec<_> = items.iter().map(|(slot, item)| (*slot, item)).collect();
    std::fs::write(&path, build_chunk(&items).unwrap()).unwrap();
    let mut libraries = PhraseLibraries::empty();
    oxpinyin_tool_support::data::load_heap_library(&mut libraries, 1, &path).unwrap();
    let source = LibraryPhraseSource {
        libraries: &libraries,
    };
    let bigram = Hash::create(&dir.path().join("bigram.db")).unwrap();
    bigram
        .put(
            &encode_token_key(1),
            &encode_single_gram(31, &[(0x0100_000a, 30), (0x0100_0014, 1)]),
        )
        .unwrap();
    bigram
        .put(
            &encode_token_key(0x0100_000a),
            &encode_single_gram(40, &[(0x0100_001e, 40)]),
        )
        .unwrap();
    for lambda in [0.0, 0.312_699, 1.0] {
        let model = EvalLanguageModel::new(&libraries, Some(&bigram), lambda);
        // The gen_unigram floor: every lexicon token carries count + 1
        // (101, 11, 51) and the total is their sum, 163. Without the floor
        // this would be 100 / 160.
        let floored = model.unigram_poss(0x0100_000a);
        assert!(
            (floored - 101.0 / 163.0).abs() < 1e-12,
            "λ={lambda} floored P_uni(中) {floored}"
        );
        let report = correction_rate(&dictionary, &model, &source, &sentences()).expect("evaluate");
        assert_eq!(report.tested, 2, "λ={lambda}");
        assert_eq!(report.passed, 1, "λ={lambda}: 中国 passes, 钟 fails");
        assert!(
            (report.rate - 0.5).abs() < 1e-12,
            "λ={lambda} rate {}",
            report.rate
        );
        // The one failure is 钟 decoded as the more frequent homophone 中.
        assert_eq!(report.mismatches, vec![("钟".to_owned(), "中".to_owned())]);
    }
}

// The shared fixture uses tone-free inventory keys. Adapt only these
// authored test inputs; the evaluator keeps packed keys in production.
pub(super) struct FixtureDictionary(fixture_doubles::FixtureDictionary);

impl FixtureDictionary {
    pub(super) fn parse(text: &str) -> Result<Self, fixture_doubles::FixtureError> {
        fixture_doubles::FixtureDictionary::parse(text).map(Self)
    }
}

impl PhraseList for FixtureDictionary {
    type Syllable = ChewingKey;
    type Error = fixture_doubles::FixtureError;

    fn lookup(&self, keys: &[ChewingKey]) -> Result<Vec<PhraseEntry>, Self::Error> {
        self.0.lookup(
            &keys
                .iter()
                .copied()
                .map(SyllableKey::from_matrix_key)
                .collect::<Vec<_>>(),
        )
    }

    fn phrase_prefix_exists(&self, keys: &[ChewingKey]) -> Result<bool, Self::Error> {
        self.0.phrase_prefix_exists(
            &keys
                .iter()
                .copied()
                .map(SyllableKey::from_matrix_key)
                .collect::<Vec<_>>(),
        )
    }
}
