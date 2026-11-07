//! A token's unigram around an import and around one constrained training (#808).

use crate::facade;
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, n_candidate, tokens_of, unigram_of,
};
use crate::support::Kit;

#[derive(Debug, PartialEq)]
pub struct UserUnigram {
    pub imported: bool,
    pub token: String,
    pub before: (bool, u32),
    pub add: bool,
    pub after: (bool, u32),
}

/// Imports 你好你好 into library 7 with count nine, then adds seven to its unigram.
pub fn token_unigram_user(k: &mut Kit) -> UserUnigram {
    let phrase = "你好你好";
    let reading = if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni3'hao3'ni3'hao3"
    };
    let ctx = k.ctx();
    let it = facade::begin_add_phrases(ctx, 7);
    assert!(!it.is_null());
    let imported = facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), 9);
    facade::end_add_phrases(it);
    assert!(imported);
    let tokens: Vec<u32> = tokens_of(k, phrase)
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect();
    assert_eq!(tokens.len(), 1, "{tokens:?}");
    let token = tokens[0];
    let before = unigram_of(k, token);
    let add = facade::token_add_unigram_frequency(k.inst(), token, 7);
    let after = unigram_of(k, token);
    UserUnigram {
        imported,
        token: format!("{token:#x}"),
        before,
        add,
        after,
    }
}

#[derive(Debug, PartialEq)]
pub struct Training {
    pub chosen: String,
    pub kind: i32,
    pub cursor: i32,
    pub after_choose: (bool, u32),
    pub guess: bool,
    pub train: bool,
}

/// `train_selected_top`: chooses the NORMAL candidate `wanted`, guesses the
/// n-best list and trains result 0 (`pinyin.cpp:2578-2588`,
/// `zhuyin.cpp:1646-1666`, `phonetic_lookup.h:874-929`).
pub fn train_selected_top(k: &mut Kit, wanted: &str, token: u32) -> Training {
    let inst = k.inst();
    if facade::ZHUYIN {
        assert!(facade::guess_candidates_after_cursor(inst, 0));
    } else {
        // SORT_WITHOUT_SENTENCE would train during choose (pinyin.cpp:2566-2576).
        assert!(facade::guess_candidates(inst, 0, 0x1e));
    }
    for index in 0..n_candidate(inst) {
        let cand = candidate_at(inst, index).expect("candidate");
        let text = candidate_text(inst, cand).expect("candidate text");
        let kind = candidate_type(inst, cand);
        if kind == 2 && text == wanted {
            let cursor = facade::choose_candidate(inst, 0, cand);
            assert!(cursor > 0, "normal phrase selection failed");
            let after_choose = unigram_of(k, token);
            let guess = facade::guess_sentence(inst);
            assert!(guess, "training needs a guessed n-best result");
            let train = facade::train_top(inst);
            assert!(train, "training n-best result 0 failed");
            return Training {
                chosen: wanted.to_owned(),
                kind,
                cursor,
                after_choose,
                guess,
                train,
            };
        }
    }
    panic!("missing normal candidate: {wanted}");
}

#[derive(Debug, PartialEq)]
pub struct Trained {
    pub token: String,
    pub parsed: usize,
    pub before: (bool, u32),
    pub training: Training,
    pub after: (bool, u32),
}

/// 癫痫's unigram around one constrained training of its NORMAL row.
pub fn token_unigram_trained(k: &mut Kit) -> Trained {
    let token = 0x0100_3e57;
    assert_eq!(tokens_of(k, "癫痫"), [token]);
    let text: &[u8] = if facade::ZHUYIN {
        b"2u04vu04"
    } else {
        b"dian4xian4"
    };
    let inst = k.inst();
    let parsed = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c(text).as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c(text).as_ptr())
    };
    assert_eq!(parsed, text.len(), "the whole training input must parse");
    let before = unigram_of(k, token);
    let training = train_selected_top(k, "癫痫", token);
    assert_eq!(training.after_choose, before, "selection must not train");
    let after = unigram_of(k, token);
    Trained {
        token: format!("{token:#x}"),
        parsed,
        before,
        training,
        after,
    }
}
