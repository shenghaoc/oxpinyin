//! The token_unigram_training family's case bodies: each answers the result document the old case returned.
//!
//! Token unigrams and training (inventory row 30, #540, #576, #808, #809,
//! #1418): stored unigrams of system and user tokens, zhuyin-only readings,
//! the library total under repeated training and predicted-row acceptance.

use std::collections::BTreeSet;

use crate::facade;
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, garray_u16, guess_out, import_rows,
    n_candidate, tokens_of, unigram_of,
};
use crate::support::{J, Kit, P, UNTOUCHED};

/// `FACADE_TOTAL`: the oracle data's loaded libraries, summed.
const FACADE_TOTAL: u64 = 51_051_831;

/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let stem = if facade::ZHUYIN {
        id.strip_prefix("zhuyin-")
            .unwrap_or_else(|| panic!("{id} is not a zhuyin case"))
    } else {
        id
    };
    match stem {
        "system-zhuyin-only-readings" => zhuyin_only_readings(k, &[1, 2, 3, 4]),
        "user-zhuyin-only-readings" => zhuyin_only_readings(k, &[5, 6, 7]),
        "token-unigram-system" => token_unigram_system(k),
        "token-unigram-user-libraries" => token_unigram_user_libraries(k),
        "train-unigram-total" => train_unigram_total(k),
        "predicted-unigram-overflow" => predicted_unigram_overflow(k),
        "predicted-prefix-user-phrase-unigram" => predicted_prefix_user_phrase_unigram(k),
        _ => panic!("no token_unigram_training case {id}"),
    }
}

/// `token_unigram_system`: 你's stored unigram around adding seven, and two
/// absent ids.
fn token_unigram_system(k: &mut Kit) -> J {
    let token = 0x0100_1225;
    let mut out = obj! { "token" => format!("{token:#x}"), "before" => unigram_of(k, token) };
    out.set(
        "add",
        facade::token_add_unigram_frequency(k.inst(), token, 7),
    );
    out.set("after", unigram_of(k, token));
    out.set("absent-item", unigram_of(k, 0x01FF_FFFF));
    out.set("absent-library", unigram_of(k, 0x0FFF_FFFF));
    out
}

/// `ZHUYIN_ONLY` with `ZHUYIN_ONLY_WORDS`: the ten zhuyin-only syllables, their
/// zhuyin and the word each plants.
const ZHUYIN_ONLY: [(&str, &str, &str); 10] = [
    ("chua", "ㄔㄨㄚ", "龘鱻"),
    ("den", "ㄉㄣ", "麤靐"),
    ("din", "ㄉㄧㄣ", "驫齉"),
    ("fe", "ㄈㄜ", "爨馕"),
    ("kei", "ㄎㄟ", "鬻厵"),
    ("len", "ㄌㄣ", "龘麤"),
    ("nia", "ㄋㄧㄚ", "鱻靐"),
    ("rua", "ㄖㄨㄚ", "驫爨"),
    ("yai", "ㄧㄞ", "齉鬻"),
    ("zhei", "ㄓㄟ", "馕厵"),
];

/// One library's and syllable's row of `zhuyin_only_readings`: plain and
/// third-tone readings imported, then the matching tokens' pronunciations.
fn zhuyin_only_row(k: &mut Kit, library: u8, pinyin: &str, zhuyin: &str, word: &str) -> J {
    let readings = if facade::ZHUYIN {
        [format!("{zhuyin} {zhuyin}"), format!("{zhuyin}ˇ {zhuyin}ˇ")]
    } else {
        [format!("{pinyin}'{pinyin}"), format!("{pinyin}3'{pinyin}3")]
    };
    let it = facade::begin_add_phrases(k.ctx(), library);
    let added: Vec<bool> = readings
        .iter()
        .map(|reading| facade::iterator_add_phrase(it, c(word).as_ptr(), c(reading).as_ptr(), 5))
        .collect();
    facade::end_add_phrases(it);
    let mut row = obj! { "added" => added };
    let tokens: Vec<u32> = tokens_of(k, word)
        .into_iter()
        .filter(|token| token >> 24 == u32::from(library))
        .collect();
    let hexes: Vec<String> = tokens.iter().map(|token| format!("{token:#x}")).collect();
    row.set("tokens", hexes);
    let inst = k.inst();
    for token in tokens {
        let mut count = UNTOUCHED;
        let ret = facade::token_get_n_pronunciation(inst, token, &raw mut count);
        row.set("n_pron", (ret, count));
        for nth in 0..count.min(4) {
            // SAFETY: a fresh GArray of two-byte elements; it stays allocated, as the old case left it.
            let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
            let ret = facade::token_get_nth_pronunciation(inst, token, nth, keys.cast());
            let bytes: Vec<u8> = garray_u16(keys)
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect();
            row.set(
                format!("nth {nth}"),
                (ret, crate::fixtures::hex_bytes(&bytes)),
            );
        }
    }
    row
}

/// `zhuyin_only_readings`: each zhuyin-only syllable imported into each of `libraries`.
fn zhuyin_only_readings(k: &mut Kit, libraries: &[u8]) -> J {
    let mut out = J::map();
    for &library in libraries {
        for (pinyin, zhuyin, word) in ZHUYIN_ONLY {
            let row = zhuyin_only_row(k, library, pinyin, zhuyin, word);
            out.set(format!("lib {library} {pinyin}"), row);
        }
    }
    out
}

/// `token_unigram_user_libraries`: a phrase planted in each of libraries 5, 6
/// and 7, its unigram, three absent neighbours and an add of seven.
fn token_unigram_user_libraries(k: &mut Kit) -> J {
    let plants: [(u8, &str, &str, &str); 3] = [
        (
            5,
            "你好你好",
            "ni3'hao3'ni3'hao3",
            "ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ",
        ),
        (
            6,
            "再见再见",
            "zai4'jian4'zai4'jian4",
            "ㄗㄞˋ ㄐㄧㄢˋ ㄗㄞˋ ㄐㄧㄢˋ",
        ),
        (
            7,
            "你好你好呀",
            "ni3'hao3'ni3'hao3'ya1",
            "ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ ㄧㄚ",
        ),
    ];
    let mut out = J::map();
    for (library, phrase, pinyin, zhuyin) in plants {
        let reading = if facade::ZHUYIN { zhuyin } else { pinyin };
        let it = facade::begin_add_phrases(k.ctx(), library);
        assert!(!it.is_null());
        let added = facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), 9);
        facade::end_add_phrases(it);
        assert!(added);
        let lib = u32::from(library);
        let tokens: Vec<u32> = tokens_of(k, phrase)
            .into_iter()
            .filter(|token| token >> 24 == lib)
            .collect();
        assert_eq!(tokens.len(), 1, "{tokens:?}");
        let token = tokens[0];
        out.set(format!("lib {library} token"), format!("{token:#x}"));
        out.set(format!("lib {library} item"), unigram_of(k, token));
        for delta in [0, 2, 100] {
            let absent = if delta == 0 {
                token - 1
            } else {
                (lib << 24) | ((token & 0x00FF_FFFF) + delta)
            };
            out.set(
                format!("lib {library} absent {absent:#x}"),
                unigram_of(k, absent),
            );
        }
        out.set(
            format!("lib {library} add"),
            facade::token_add_unigram_frequency(k.inst(), token, 7),
        );
        out.set(format!("lib {library} after"), unigram_of(k, token));
    }
    out
}

/// `choose_text`: candidates at `offset` under sort option 0 and the choice of
/// the first whose text is `wanted`; `None` when none is.
fn choose_text(inst: P, offset: usize, wanted: &str) -> Option<i32> {
    facade::guess_candidates(inst, offset, 0);
    (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .find(|&cand| candidate_text(inst, cand).as_deref() == Some(wanted))
        .map(|cand| facade::choose_candidate(inst, offset, cand))
}

/// `nihao` parsed and guessed on the cached instance.
fn nihao_guessed(k: &mut Kit) -> P {
    let inst = k.inst();
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    inst
}

/// 你 and 好 chosen where available and the sentence guessed again: the two choices.
fn choose_nihao(inst: P) -> (Option<i32>, Option<i32>) {
    let first = choose_text(inst, 0, "你");
    let second = first
        .filter(|&first| first != 0)
        .and_then(|first| choose_text(inst, usize::try_from(first).expect("an offset"), "好"));
    facade::guess_sentence(inst);
    (first, second)
}

/// `train-unigram-total`: 你's and 好's unigrams at training checkpoints up to 30,000.
fn train_unigram_total(k: &mut Kit) -> J {
    let inst = nihao_guessed(k);
    let ni = tokens_of(k, "你")[0];
    let hao = tokens_of(k, "好")[0];
    let chosen = choose_nihao(inst);
    let mut out = obj! {
        "chosen" => chosen,
        "before" => (unigram_of(k, ni), unigram_of(k, hao)),
    };
    let mut done = 0;
    for count in [1, 9999, 10000, 10000] {
        let rets: BTreeSet<bool> = (0..count).map(|_| facade::train_top(inst)).collect();
        done += count;
        out.set(
            format!("after {done}"),
            (
                rets.into_iter().collect::<Vec<_>>(),
                unigram_of(k, ni),
                unigram_of(k, hao),
            ),
        );
    }
    out
}

/// `predicted-unigram-overflow`: accepted predicted rows after 20,000 trainings.
fn predicted_unigram_overflow(k: &mut Kit) -> J {
    let inst = nihao_guessed(k);
    let chosen = choose_nihao(inst);
    for _ in 0..20000 {
        facade::train_top(inst);
    }
    facade::guess_predicted_candidates(inst, c("你").as_ptr());
    let mut pick = None;
    for index in 0..n_candidate(inst) {
        let cand = candidate_at(inst, index).unwrap_or(std::ptr::null_mut());
        let kind = candidate_type(inst, cand);
        let text = candidate_text(inst, cand).expect("candidate text");
        if kind == 5 && tokens_of(k, &text).first().copied().unwrap_or(0) >> 24 == 1 {
            pick = Some(cand);
            break;
        }
    }
    let mut out = obj! { "chosen" => chosen, "found" => pick.is_some() };
    let Some(pick) = pick else { return out };
    let mut accepted = 0;
    while accepted < 1000 && facade::choose_predicted_candidate(inst, pick) {
        accepted += 1;
    }
    out.set("accepted before the first refusal", accepted);
    out
}

/// `predicted-prefix-user-phrase-unigram`: a user phrase's predicted prefix row
/// chosen, its unigram, and the facade total read through a below-wrap guess.
fn predicted_prefix_user_phrase_unigram(k: &mut Kit) -> J {
    let phrase = "龘龘鱻";
    assert_eq!(import_rows(k, 7, &[(phrase, "da2'da2'xian1", 5)]), [true]);
    let tokens: Vec<u32> = tokens_of(k, phrase)
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect();
    assert_eq!(tokens.len(), 1, "{tokens:?}");
    let token = tokens[0];
    let inst = k.inst();
    facade::guess_predicted_candidates(inst, c("龘龘").as_ptr());
    let pick = (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .find(|&cand| {
            candidate_type(inst, cand) == 5 && candidate_text(inst, cand).as_deref() == Some("鱻")
        })
        .expect("no predicted prefix row for the user phrase");
    let mut out = obj! { "token" => format!("{token:#x}"), "before" => unigram_of(k, token) };
    out.set("choose", facade::choose_predicted_candidate(inst, pick));
    out.set("after", unigram_of(k, token));
    let expected = FACADE_TOTAL + 3 * 5 + 69 * 7;
    let delta = u32::try_from((1_u64 << 32) - 1 - expected).expect("a u32");
    out.set(
        "below-wrap-add",
        facade::token_add_unigram_frequency(inst, (1 << 24) | 1, delta),
    );
    out.set("below-wrap", guess_out(k, b"ni3'hao3"));
    out
}
