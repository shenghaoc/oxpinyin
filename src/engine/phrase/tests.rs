use super::{phrase_segment, phrase_segment_ucs4};
use crate::PhraseEntry;

use crate::{
    PhraseToken, SyllableKey,
    model::{LanguageModel, NbestStepCosts, PhraseList},
};

/// A tiny dictionary: the text → tokens reverse map the span DP probes.
struct Dict {
    entries: Vec<(String, PhraseToken)>,
}

impl Dict {
    fn parse(rows: &[(&str, u32)]) -> Self {
        Self {
            entries: rows
                .iter()
                .map(|(text, token)| ((*text).to_owned(), PhraseToken::new(*token)))
                .collect(),
        }
    }
}

impl PhraseList for Dict {
    type Syllable = SyllableKey;
    type Error = core::convert::Infallible;

    fn lookup(&self, _syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error> {
        Ok(Vec::new())
    }

    fn tokens_for_text(&self, text: &str) -> Vec<PhraseToken> {
        self.entries
            .iter()
            .filter(|(stored, _)| stored == text)
            .map(|(_, token)| *token)
            .collect()
    }
}

/// A dictionary whose raw UCS-4 rows are the only lookup source.
struct RawDict {
    entries: Vec<(Vec<u32>, PhraseToken)>,
}

impl PhraseList for RawDict {
    type Syllable = SyllableKey;
    type Error = core::convert::Infallible;

    fn lookup(&self, _syllables: &[Self::Syllable]) -> Result<Vec<PhraseEntry>, Self::Error> {
        Ok(Vec::new())
    }

    fn tokens_for_ucs4(&self, ucs4: &[u32]) -> Vec<PhraseToken> {
        self.entries
            .iter()
            .filter(|(stored, _)| stored == ucs4)
            .map(|(_, token)| *token)
            .collect()
    }
}

/// A model whose row read reports the checked-read failure the pin
/// asserts on.
struct CheckedModel;

impl LanguageModel for CheckedModel {
    type Token = PhraseToken;
    type Error = &'static str;

    fn nbest_successors_in_row<'a>(
        &self,
        _prev: &Self::Token,
        _row: &'a mut crate::model::NbestStepRow,
    ) -> Result<Option<crate::model::NbestSuccessors<'a>>, Self::Error> {
        Err("read after check")
    }

    fn is_read_after_check(&self, _error: &Self::Error) -> bool {
        true
    }
}

#[test]
fn a_raw_surrogate_key_segments_to_its_token() {
    // The pin searches the phrase table with the raw UCS-4 words the
    // converter produced. A stored `[0xd800]` row must reach its token
    // even though no `char` or `str` exists for that word.
    let dictionary = RawDict {
        entries: vec![(vec![0xd800], PhraseToken::new(3))],
    };
    let gram = [1_u32, 3, 1]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let model = Rows {
        row: crate::model::MergedGram::merge(None, Some(gram)).expect("whole row"),
    };
    let (matched, result) =
        phrase_segment_ucs4(&dictionary, &model, &[0xd800]).expect("infallible model");
    assert!(matched);
    assert_eq!(result[0].value(), 3);
}

#[test]
fn a_scalar_prefix_row_is_read_before_the_non_scalar() {
    use crate::engine::nbest::SENTENCE_START;
    use crate::model::{NbestStepCosts, NbestStepRow, NbestSuccessors};

    struct Recording(std::cell::RefCell<Vec<u32>>);
    impl LanguageModel for Recording {
        type Token = PhraseToken;
        type Error = core::convert::Infallible;

        fn nbest_successors_in_row<'a>(
            &self,
            prev: &PhraseToken,
            _: &'a mut NbestStepRow,
        ) -> Result<Option<NbestSuccessors<'a>>, Self::Error> {
            self.0.borrow_mut().push(prev.value());
            Ok(Some(NbestSuccessors::Miss))
        }

        fn nbest_item_costs_in_row(
            &self,
            _: &PhraseToken,
            _: &PhraseToken,
            _: Option<(u32, u32)>,
            _: &mut NbestStepRow,
        ) -> Result<NbestStepCosts, Self::Error> {
            Ok(NbestStepCosts {
                blended: None,
                unigram: Some(2.0_f64.powf(-0.001)),

                unigram_lambda: 1.0,
            })
        }
    }

    let dictionary = RawDict {
        entries: vec![(vec![0x4f60], PhraseToken::new(3))],
    };
    let model = Recording(std::cell::RefCell::default());
    let (matched, result) = phrase_segment_ucs4(&dictionary, &model, &[0x4f60, 0xd800]).unwrap();
    assert!(!matched);
    assert_eq!(result, [PhraseToken::new(0), PhraseToken::new(0)]);
    assert!(model.0.borrow().contains(&SENTENCE_START));
}

#[test]
fn row_read_names_only_the_checked_read_failure() {
    use crate::engine::error::EngineError;
    // Every row read that maps a model error goes through this: phrase
    // segmentation, the n-best predecessor rows and the guess's
    // `DYNAMIC_ADJUST` row.
    assert_eq!(
        EngineError::row_read(&CheckedModel, 7, &"read after check"),
        EngineError::ReadAfterCheck { token: 7 }
    );
    struct Unchecked;
    impl LanguageModel for Unchecked {
        type Token = PhraseToken;
        type Error = &'static str;
    }
    assert!(matches!(
        EngineError::row_read(&Unchecked, 7, &"closed"),
        EngineError::Scoring(_)
    ));
}

#[test]
fn a_checked_read_failure_is_the_predecessors_read_after_check_fault() {
    use crate::engine::error::EngineError;
    use crate::engine::nbest::SENTENCE_START;
    let dictionary = Dict::parse(&[("你", 3)]);
    assert_eq!(
        phrase_segment(&dictionary, &CheckedModel, "你").unwrap_err(),
        EngineError::ReadAfterCheck {
            token: SENTENCE_START
        }
    );
}

#[test]
fn prefix_faults_follow_spans_before_predecessors() {
    struct Ordered(std::cell::RefCell<Vec<(u32, u32)>>);
    impl LanguageModel for Ordered {
        type Token = PhraseToken;
        type Error = &'static str;

        fn nbest_item_costs_in_row(
            &self,
            prev: &PhraseToken,
            token: &PhraseToken,
            _: Option<(u32, u32)>,
            _: &mut crate::model::NbestStepRow,
        ) -> Result<NbestStepCosts, Self::Error> {
            let pair = (prev.value(), token.value());
            self.0.borrow_mut().push(pair);
            match pair {
                (4, 5) => Err("first span's second predecessor"),
                (3, 6) => Err("later span's first predecessor"),
                _ => Ok(NbestStepCosts {
                    blended: Some(0.5),
                    unigram: Some(0.5),

                    unigram_lambda: 1.0,
                }),
            }
        }
    }
    let dictionary = Dict::parse(&[("a", 3), ("a", 4), ("b", 5), ("bc", 6)]);
    let model = Ordered(std::cell::RefCell::default());
    let error = phrase_segment(&dictionary, &model, "abc").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("first span's second predecessor")
    );
    assert_eq!(*model.0.borrow(), [(1, 3), (1, 4), (3, 5), (4, 5)]);
}

/// A missing row (a failed system read is one at the data layer):
/// `search_bigram2` loads it once per predecessor for the span, however
/// many tokens the span holds, and `search_unigram2` still runs
/// (`phrase_lookup.cpp:145-146,263-270`), so `final_step` finds the path.
#[test]
fn a_missing_row_is_read_once_per_span_and_the_unigram_branch_goes_on() {
    use crate::model::{NbestStepRow, NbestSuccessors};

    struct Missing(std::cell::RefCell<Vec<u32>>);
    impl LanguageModel for Missing {
        type Token = PhraseToken;
        type Error = core::convert::Infallible;

        fn nbest_successors_in_row<'a>(
            &self,
            prev: &PhraseToken,
            row: &'a mut NbestStepRow,
        ) -> Result<Option<NbestSuccessors<'a>>, Self::Error> {
            row.get_or_try_init::<Self::Error>(|| {
                self.0.borrow_mut().push(prev.value());
                Ok(None)
            })?;
            Ok(Some(NbestSuccessors::Miss))
        }

        fn nbest_item_costs_in_row(
            &self,
            _: &PhraseToken,
            _: &PhraseToken,
            _: Option<(u32, u32)>,
            _: &mut NbestStepRow,
        ) -> Result<NbestStepCosts, Self::Error> {
            Ok(NbestStepCosts {
                blended: None,
                unigram: Some(0.25),
                unigram_lambda: 1.0,
            })
        }
    }

    let dictionary = Dict::parse(&[("a", 3), ("a", 5), ("b", 4)]);
    let model = Missing(std::cell::RefCell::default());
    let (matched, result) = phrase_segment(&dictionary, &model, "ab").unwrap();
    assert!(matched);
    assert_eq!(result, [PhraseToken::new(3), PhraseToken::new(4)]);
    // Sentence start once for "a" (two tokens), then one row per node
    // of the next step for "b".
    assert_eq!(*model.0.borrow(), [1, 3, 5]);
}

/// A model whose every predecessor's merged row is `row`, pricing the
/// bigram branch only for an item read from it.
struct Rows {
    row: crate::model::MergedGram,
}

impl LanguageModel for Rows {
    type Token = PhraseToken;
    type Error = core::convert::Infallible;

    fn nbest_successors_in_row<'a>(
        &self,
        _: &Self::Token,
        row: &'a mut crate::model::NbestStepRow,
    ) -> Result<Option<crate::model::NbestSuccessors<'a>>, Self::Error> {
        Ok(Some(
            row.get_or_try_init::<Self::Error>(|| Ok(Some(self.row.clone())))?
                .map_or(
                    crate::model::NbestSuccessors::Miss,
                    crate::model::NbestSuccessors::Row,
                ),
        ))
    }

    fn nbest_item_costs_in_row(
        &self,
        _: &Self::Token,
        _: &Self::Token,
        item: Option<(u32, u32)>,
        _: &mut crate::model::NbestStepRow,
    ) -> Result<NbestStepCosts, Self::Error> {
        Ok(NbestStepCosts {
            blended: item.map(|(freq, total)| f64::from(freq) / f64::from(total)),
            unigram: None,

            unigram_lambda: 1.0,
        })
    }
}

/// N2 refusals 1 and 4 on `phrase_segment`'s merge and `get_freq`
/// (`phrase_lookup.cpp:272-290`): the sentence-start predecessor's row
/// refuses where the pin asserts or reads past it, and a whole row
/// segments.
#[test]
fn a_predecessor_row_the_pin_cannot_read_refuses_the_segment() {
    use crate::engine::error::EngineError;
    use crate::engine::nbest::SENTENCE_START;
    use crate::model::MergedGram;
    let dictionary = Dict::parse(&[("你", 3)]);
    let system = || {
        let mut value = 1_u32.to_le_bytes().to_vec();
        value.extend_from_slice(&3_u32.to_le_bytes());
        value.extend_from_slice(&1_u32.to_le_bytes());
        value
    };
    let short_merge = Rows {
        row: MergedGram::merge(Some(system()), Some(vec![0, 0])).unwrap(),
    };
    assert_eq!(
        phrase_segment(&dictionary, &short_merge, "你").unwrap_err(),
        EngineError::ShortLookupPredecessor {
            token: SENTENCE_START
        }
    );
    assert!(phrase_segment(&dictionary, &short_merge, "").is_ok());
    assert!(phrase_segment(&dictionary, &short_merge, "无").is_ok());
    let short_alone = Rows {
        row: MergedGram::merge(None, Some(vec![0, 0])).unwrap(),
    };
    assert_eq!(
        phrase_segment(&dictionary, &short_alone, "你").unwrap_err(),
        EngineError::UnsafeLookupPredecessor {
            token: SENTENCE_START
        }
    );
    let whole = Rows {
        row: MergedGram::merge(Some(system()), None).unwrap(),
    };
    let (matched, result) = phrase_segment(&dictionary, &whole, "你").unwrap();
    assert!(matched);
    assert_eq!(result[0].value(), 3);
}
#[test]
fn ieee_relax_and_best_keep_first_on_ties_and_nan() {
    use super::{Node, Step};
    let node = |token, poss| Node {
        token: PhraseToken::new(token),
        prev: PhraseToken::new(1),
        last_step: 0,
        poss,
    };
    let mut step = Step::default();
    step.relax(node(3, f32::NAN));
    step.relax(node(3, 10.0));
    step.relax(node(4, f32::INFINITY));
    assert!(step.best().unwrap().poss.is_nan());
    let mut step = Step::default();
    step.relax(node(3, f32::NEG_INFINITY));
    step.relax(node(4, f32::NEG_INFINITY));
    assert_eq!(step.best().unwrap().token.value(), 3);
    step.relax(node(3, f32::NAN));
    assert_eq!(step.best().unwrap().poss, f32::NEG_INFINITY);
    step.relax(node(4, 1.0));
    assert_eq!(step.best().unwrap().token.value(), 4);
}
