//! The user_index_phrase_mask family's case bodies: each answers the result document the old case returned.
//!
//! The user index tables under `mask_out` and removal (inventory row 52, #784,
//! #785, #814, #1418): keys left empty, the mask walk's tails, tone-variant
//! readings of one token and predicted bigrams over faulty grams.

use crate::facade;
use crate::fixtures::{TOKEN_REUSE_PHRASES, craft_hash, token_reuse_files, write_user_conf};
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, garray_u32, n_candidate, predicted_rows,
};
use crate::support::{J, Kit, P};

/// One step of [`index_key_sequence`].
#[derive(Clone, Copy, Debug)]
enum Step {
    /// `('add', n)`: import phrase n with count 100.
    Add(usize),
    /// `('save',)`, recording the user files.
    Save,
    /// `('mask', mask, value)`.
    Mask(u32, u32),
    /// `('reopen',)`: finalise without saving and initialise again.
    Reopen,
    /// `('unload',)` of library 7.
    Unload,
    /// `('load',)` of library 7.
    Load,
    /// `('remove', n)`: remove phrase n's first NORMAL user candidate.
    Remove(usize),
    /// `('guess', pinyin, zhuyin)`: candidates of that input.
    Guess(&'static str, &'static str),
    /// `('lookup', n)`: what a fresh instance shows for phrase n.
    Lookup(usize),
}

use Step::{Add, Load, Lookup, Mask, Remove, Reopen, Save, Unload};

/// A phrase set: text, pinyin and zhuyin readings.
type Phrases = &'static [(&'static str, &'static str, &'static str)];

/// The facade's parse input for each phrase of a set: pinyin, then zhuyin.
type Readings = (&'static [&'static str], &'static [&'static str]);

/// `INDEX_KEY_READINGS`.
const INDEX_KEY_READINGS: Readings = (
    &["dada", "tata", "bingbing", "xianxian", "cucu", "yuanyuan"],
    &[
        "286286", "w84w84", "1u/31u/3", "vu0 vu0 ", "hj hj ", "m06m06",
    ],
);

/// `TAIL_PHRASES`.
const TAIL_PHRASES: Phrases = &[
    ("龘龘", "da2'da2", "ㄉㄚˊ ㄉㄚˊ"),
    ("龖龖", "ta4'ta4", "ㄊㄚˋ ㄊㄚˋ"),
    ("靐靐", "bing3'bing3", "ㄅㄧㄥˇ ㄅㄧㄥˇ"),
    ("鬥鬥", "dou4'dou4", "ㄉㄡˋ ㄉㄡˋ"),
    ("龘", "da2", "ㄉㄚˊ"),
    ("鱻鱻", "xian1'xian1", "ㄒㄧㄢ ㄒㄧㄢ"),
    ("龗龗", "da2'da2", "ㄉㄚˊ ㄉㄚˊ"),
];

/// `TAIL_READINGS`.
const TAIL_READINGS: Readings = (
    &[
        "dada", "tata", "bingbing", "doudou", "da", "xianxian", "dada",
    ],
    &[
        "286286", "w84w84", "1u/31u/3", "2.42.4", "286", "vu0 vu0 ", "286286",
    ],
);

/// `TONE_PHRASES`.
const TONE_PHRASES: Phrases = &[
    ("龘龘", "da2'da2", "ㄉㄚˊ ㄉㄚˊ"),
    ("龘龘", "da4'da4", "ㄉㄚˋ ㄉㄚˋ"),
];

/// `TONE_READINGS`: pinyin only.
const TONE_READINGS: Readings = (&["da2'da2", "da4'da4"], &[]);

/// `MIDDLE` and `USER_ALL`.
const MIDDLE: Step = Mask(0xffff_ffff, 0x0700_0002);
const USER_ALL: Step = Mask(0x0f00_0000, 0x0700_0000);
/// `MASK_TA`.
const MASK_TA: Step = Mask(0xffff_ffff, 0x0700_0004);
/// `DD`, `D`.
const DD: Step = Step::Guess("dada", "286286");
/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    match id {
        "abort-guess-predicted-empty-bigram-gram" => return predicted(k, "我", &[7, 0, 0, 0]),
        "guess-predicted-unrelated-short-bigram-value" => return predicted(k, "你", &[7, 0, 0]),
        "issue1418-tone-variant-readings-pinyin" => {
            let steps = [
                Add(0),
                Add(1),
                Step::Guess("dada", ""),
                Step::Guess("da2'da", ""),
                Step::Guess("da4'da4", ""),
                Lookup(0),
                Save,
                Reopen,
                Step::Guess("dada", ""),
                Lookup(1),
            ];
            return index_key_sequence(k, &steps, TONE_PHRASES, TONE_READINGS);
        }
        _ => {}
    }
    if let Some(name) = id.strip_prefix("issue785-pinyin-tails-") {
        let name = strip_mode(name);
        return index_key_sequence(k, &tail_steps(name), TAIL_PHRASES, TAIL_READINGS);
    }
    let name = id
        .strip_prefix("issue784-index-keys-")
        .unwrap_or_else(|| panic!("no user_index_phrase_mask case {id}"));
    let steps = if name.starts_with("remove-") {
        remove_steps(name)
    } else {
        index_key_steps(strip_mode(name))
    };
    index_key_sequence(k, &steps, &TOKEN_REUSE_PHRASES, INDEX_KEY_READINGS)
}

/// A case name without the facade's `-pinyin` / `-zhuyin` suffix.
fn strip_mode(name: &str) -> &str {
    let suffix = if facade::ZHUYIN { "-zhuyin" } else { "-pinyin" };
    name.strip_suffix(suffix)
        .unwrap_or_else(|| panic!("{name} is not a case of this facade"))
}

/// `SAVED`: three imports and a save.
fn saved() -> Vec<Step> {
    vec![Add(0), Add(1), Add(2), Save]
}

/// `SHARED`: the tail sequences' five imports.
fn shared() -> Vec<Step> {
    vec![Add(0), Add(6), Add(3), Add(1), Add(2)]
}

/// `SAVED` followed by `rest`.
fn after_saved(rest: &[Step]) -> Vec<Step> {
    let mut steps = saved();
    steps.extend_from_slice(rest);
    steps
}

/// `INDEX_KEY_SEQUENCES`' steps for `name`.
fn index_key_steps(name: &str) -> Vec<Step> {
    match name {
        "mask-middle" => after_saved(&[MIDDLE, Add(3), Save]),
        "mask-reopen-lookup" => after_saved(&[MIDDLE, Add(3), Save, Reopen, Lookup(1), Lookup(0)]),
        other => panic!("no index key sequence {other}"),
    }
}
/// `REMOVE_SEQUENCES`' steps for `name`.
fn remove_steps(name: &str) -> Vec<Step> {
    match name {
        "remove-middle" => after_saved(&[Remove(1), Add(3), Save]),
        other => panic!("no removal sequence {other}"),
    }
}
/// `TAIL_SEQUENCES`' steps for `name`.
fn tail_steps(name: &str) -> Vec<Step> {
    let rest: Vec<Step> = match name {
        "hit" => vec![Save, DD, MASK_TA, Add(5), Save],
        "hit-order" => vec![
            Save,
            Step::Guess("dadatata", "286286w84w84"),
            MASK_TA,
            Add(5),
            Save,
        ],
        "mask-all" => vec![DD, USER_ALL, Add(5), Save],
        "reopen" => vec![DD, MASK_TA, Save, Reopen, Add(5), Save],
        other => panic!("no tail sequence {other}"),
    };
    let mut steps = shared();
    steps.extend(rest);
    steps
}
/// `index_key_sequence`'s `tokens`: `lookup_tokens` of `text` and the tokens as hex.
fn tokens(inst: P, text: &str) -> J {
    // SAFETY: a fresh GArray of four-byte elements; it stays allocated, as the old case left it.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    let ret = facade::lookup_tokens(inst, c(text).as_ptr(), array.cast());
    let found: Vec<String> = garray_u32(array)
        .iter()
        .map(|token| format!("{token:#x}"))
        .collect();
    J::from((ret, found))
}

/// `index_key_sequence`'s `candidates`: the count and up to twelve rows.
fn candidates(inst: P) -> J {
    let count = n_candidate(inst);
    let rows: Vec<(i32, String)> = (0..count.min(12))
        .map(|index| {
            let cand = candidate_at(inst, index).unwrap_or(std::ptr::null_mut());
            (
                candidate_type(inst, cand),
                candidate_text(inst, cand).expect("candidate text"),
            )
        })
        .collect();
    J::from((count, rows))
}

/// `index_key_sequence`'s `guess`: parse `reading` with the facade's parser and
/// list the candidates at offset 0, no sentence guessed.
fn guess(inst: P, reading: &str) {
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c(reading).as_ptr());
        facade::guess_candidates_after_cursor(inst, 0);
    } else {
        facade::parse_more_full_pinyins(inst, c(reading).as_ptr());
        facade::guess_candidates(inst, 0, 0);
    }
}

/// `_user_candidate_rows`: the NORMAL user-dictionary candidates of the list.
fn user_candidate_rows(inst: P) -> Vec<P> {
    (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .filter(|&cand| candidate_type(inst, cand) == 2 && facade::is_user_candidate(inst, cand))
        .collect()
}

/// `index_key_sequence`'s `lookup`: phrase `n`'s candidates and tokens, its
/// first character's tokens and (pinyin) predictions.
fn lookup(ctx: P, text: &str, reading: &str) -> J {
    let inst = facade::alloc_instance(ctx);
    guess(inst, reading);
    let first: String = text.chars().next().expect("a character").to_string();
    let mut seen = obj! {
        "candidates" => candidates(inst),
        "tokens" => tokens(inst, text),
        "prefix tokens" => tokens(inst, &first),
    };
    if !facade::ZHUYIN {
        seen.set("predicted", predicted_rows(inst, &first, 12));
    }
    facade::free_instance(inst);
    J::from(("lookup", text, seen))
}

/// `index_key_sequence`: runs `steps` over the user library and records each
/// save's user files and each observation.
fn index_key_sequence(k: &mut Kit, steps: &[Step], phrases: Phrases, readings: Readings) -> J {
    let mut ctx = k.ctx();
    let readings = if facade::ZHUYIN {
        readings.1
    } else {
        readings.0
    };
    let mut out = Vec::new();
    for step in steps {
        match *step {
            Add(n) => {
                let it = facade::begin_add_phrases(ctx, 7);
                assert!(!it.is_null(), "the import iterator");
                let (text, pinyin, zhuyin) = phrases[n];
                let reading = if facade::ZHUYIN { zhuyin } else { pinyin };
                let added =
                    facade::iterator_add_phrase(it, c(text).as_ptr(), c(reading).as_ptr(), 100);
                out.push(J::from(("add", added)));
                facade::end_add_phrases(it);
            }
            Save => out.push(J::from(("save", facade::save(ctx), token_reuse_files(k)))),
            Mask(mask, value) => out.push(J::from(("mask", facade::mask_out(ctx, mask, value)))),
            Reopen => {
                facade::fini(ctx);
                ctx = k.init_default();
                assert!(!ctx.is_null(), "the reopen failed");
            }
            Unload => out.push(J::from(("unload", facade::unload_phrase_library(ctx, 7)))),
            Load => out.push(J::from(("load", facade::load_phrase_library(ctx, 7)))),
            Remove(n) => {
                let inst = facade::alloc_instance(ctx);
                guess(inst, readings[n]);
                let rows = user_candidate_rows(inst);
                let first = *rows.first().expect("no user candidate to remove");
                out.push(J::from((
                    "remove",
                    facade::remove_user_candidate(inst, first),
                )));
                facade::free_instance(inst);
            }
            Step::Guess(pinyin, zhuyin) => {
                let inst = facade::alloc_instance(ctx);
                guess(inst, if facade::ZHUYIN { zhuyin } else { pinyin });
                out.push(J::from(("guess", pinyin, candidates(inst))));
                facade::free_instance(inst);
            }
            Lookup(n) => out.push(lookup(ctx, phrases[n].0, readings[n])),
        }
    }
    obj! { "steps" => out }
}

/// `predicted_bigram_context`: a context whose user bigram carries `value`
/// under every phrase-table token of `phrase`; the context is not cached.
fn predicted_bigram_context(k: &Kit, phrase: &str, value: &[u8]) -> P {
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the first init failed");
    let inst = facade::alloc_instance(ctx);
    // SAFETY: a fresh GArray of four-byte elements; it stays allocated, as the old fixture left it.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    assert!(facade::lookup_tokens(
        inst,
        c(phrase).as_ptr(),
        array.cast()
    ));
    let found = garray_u32(array);
    facade::free_instance(inst);
    facade::fini(ctx);
    let rows: Vec<(Vec<u8>, Vec<u8>)> = found
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), value.to_vec()))
        .collect();
    craft_hash(&k.user.join("user_bigram.db"), &rows);
    write_user_conf(k);
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the reopen failed");
    ctx
}

/// Predicts after 我 over a user bigram planted with `value` under `phrase`'s tokens.
fn predicted(k: &mut Kit, phrase: &str, value: &[u8]) -> J {
    let ctx = predicted_bigram_context(k, phrase, value);
    let inst = facade::alloc_instance(ctx);
    obj! { "ret" => facade::guess_predicted_candidates(inst, c("我").as_ptr()) }
}
