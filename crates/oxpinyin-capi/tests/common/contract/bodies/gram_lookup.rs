//! The gram_lookup family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 62 (#1175, #757, #918): the system and user grams merge as
//! the pin's `merge_single_gram` builds them (`ngram.cpp:277-352`), and the
//! n-best step, DYNAMIC_ADJUST and the phrase segmenter read the merged chunk
//! as the pin does (`phonetic_lookup.h:594-632`, `ngram.cpp:153-176,231-250`,
//! `phrase_lookup.cpp:268-290`). Short user grams abort the pin at
//! `memory_chunk.h:390` wherever a lookup reads their total.

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use crate::facade;
use crate::fixtures::{
    chunk, craft_hash, craft_tree, gram_bytes, private_system, replace_user_bigram, write_user_conf,
};
use crate::readers::{
    c, final_step_sentences, guess_rows, segment_out, tokens_of, zhuyin_guess_rows,
};
use crate::support::{J, Kit, UNTOUCHED};

/// `_BOOST`: a count whose double wraps to zero.
const BOOST: u32 = 1 << 31;
/// `_PAST_RANGES`: a token past every phrase range.
const PAST_RANGES: u32 = 0x0fff_ffff;
/// `_SHORT`: a user gram shorter than its `guint32` total.
const SHORT: &[u8] = b"\x07\x00\x00";
/// `_DALONG`: text, pinyin, zhuyin and count of the imported phrase.
const DALONG: (&str, &str, &str, i32) = ("龘龘", "da2'da2", "ㄉㄚˊ ㄉㄚˊ", 5);

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(answer) = moded_case(k, id) {
        return answer;
    }
    match id {
        "user-gram-unsorted-range-boundary" => range_boundary(k),
        "system-gram-unsorted-guess" => system_gram_lookup(k, "unsorted"),
        "system-gram-duplicate-guess" => system_gram_lookup(k, "duplicate"),
        "user-gram-zero-count-guess" => zero_count_guess(k),
        "abort-guess-sentence-short-user-gram-no-system-row" => short_prefix_guess(k),
        "abort-phrase-segment-short-user-gram" => {
            replace_user_bigram(k, &[(1, SHORT.to_vec())]);
            let inst = k.inst();
            obj! { "ret" => facade::phrase_segment(inst, c("今天天气很好").as_ptr()) }
        }
        "guess-unrelated-short-user-gram" => {
            let token = tokens_of(k, "我")[0];
            replace_user_bigram(k, &[(token, SHORT.to_vec())]);
            guess_out(k, "nihao")
        }
        "guess-candidates-short-user-gram-dynamic-adjust-off" => dynamic_adjust_off(k),
        other => {
            let (kind, operation) = other
                .strip_prefix("user-gram-")
                .and_then(|rest| rest.split_once('-'))
                .unwrap_or_else(|| panic!("no gram_lookup case {other}"));
            user_gram_lookup(k, kind, operation)
        }
    }
}

/// The cases registered once per facade, `-pinyin` or `-zhuyin` suffixed.
fn moded_case(k: &mut Kit, id: &str) -> Option<J> {
    let name = id
        .strip_suffix("-pinyin")
        .or_else(|| id.strip_suffix("-zhuyin"))?;
    Some(match name {
        "abort-guess-sentence-short-user-gram-system-row" => {
            replace_user_bigram(k, &[(1, SHORT.to_vec())]);
            let guessed = guess_out(k, "nihao")
                .get("guessed")
                .expect("guessed")
                .clone();
            obj! { "ret" => guessed }
        }
        "abort-guess-candidates-dynamic-adjust-short-user-gram" => {
            replace_user_bigram(k, &[(1, SHORT.to_vec())]);
            let ret = dynamic_adjust_out(k, "nihao")
                .get("ret")
                .expect("ret")
                .clone();
            obj! { "ret" => ret }
        }
        "parse-dynamic-adjust-short-user-gram" => {
            replace_user_bigram(k, &[(1, SHORT.to_vec())]);
            assert!(facade::set_options(k.ctx(), 0x18a | (1 << 9)));
            let inst = k.inst();
            let parsed = facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
            obj! { "parsed" => parsed, "length" => facade::get_parsed_input_length(inst) }
        }
        "abort-phrase-segment-prefix-short-user-gram" => phrase_prefix_gram(k, true, false, false),
        "phrase-segment-prefix-sound-user-gram" => phrase_prefix_gram(k, false, false, false),
        "phrase-segment-nonscalar-before-short-user-gram" => {
            phrase_prefix_gram(k, true, true, false)
        }
        "phrase-segment-prefix-unrelated-short-user-gram" => {
            phrase_prefix_gram(k, true, false, true)
        }
        "phrase-segment-nonscalar-raw-key" => nonscalar_raw_key(k),
        _ => return None,
    })
}

/// `_guess_out`: parse `text` on the cached instance (zhuyin under option
/// word 1 << 5), guess, and every final-step sentence.
fn guess_out(k: &mut Kit, text: &str) -> J {
    if facade::ZHUYIN {
        assert!(facade::set_options(k.ctx(), 1 << 5));
    }
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let guessed = facade::guess_sentence(inst);
    obj! {
        "parsed" => parsed, "guessed" => guessed,
        "sentences" => final_step_sentences(inst),
    }
}

/// `_dynamic_adjust_out`: option word 0x38a, parse `text` and the facade's
/// candidate guess at 0.
fn dynamic_adjust_out(k: &mut Kit, text: &str) -> J {
    assert!(facade::set_options(k.ctx(), 0x18a | (1 << 9)));
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let rows = if facade::ZHUYIN {
        zhuyin_guess_rows(inst, 0)
    } else {
        guess_rows(inst, 0, 0x1e)
    };
    obj! { "parsed" => parsed, "ret" => rows.ret, "rows" => rows.rows }
}

/// `_start_gram`: the sentence start's planted gram, `unsorted` or `duplicate`.
fn start_gram(kind: &str, token: u32) -> Vec<u8> {
    if kind == "unsorted" {
        return gram_bytes(BOOST + 1, &[(PAST_RANGES, 1), (token, BOOST)]);
    }
    gram_bytes(BOOST, &[(token, BOOST), (token, BOOST.wrapping_neg() + 1)])
}

/// `_gram_items`: a raw SingleGram's total and its items in stored order.
fn gram_items(raw: &[u8]) -> (u32, Vec<(u32, u32)>) {
    let word = |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().expect("a word"));
    let end = raw.len() - (raw.len() - 4) % 8;
    let items = (4..end)
        .step_by(8)
        .map(|at| (word(at), word(at + 4)))
        .collect();
    (word(0), items)
}

/// `_system_start_tokens`: the system bigram's sentence-start successors, as stored.
fn system_start_tokens(k: &Kit) -> Vec<u32> {
    let store =
        DefaultStore::open_hash_read_only(&k.data.join("bigram.db")).expect("the system bigram");
    let mut raw = Vec::new();
    assert!(
        store
            .get_raw(b"\x01\x00\x00\x00", &mut raw)
            .expect("read the start row")
    );
    gram_items(&raw)
        .1
        .into_iter()
        .map(|(token, _)| token)
        .collect()
}

/// The interleave layout's inputs, from the frozen BDB system sentence-start row
/// (`_interleave_miss_layout` and `_pin_lower_bound` at libpinyin 074a2219,
/// `src/storage/ngram.cpp:239`, `:308-349`, `src/include/stl_lite.h:29`; read
/// from the data only, with no libpinyin run). The boosted token sorts at index
/// 14878 of the 49739 merged items, where the pin's first walk token is 16800254
/// and it does not find the boosted token.
const INTERLEAVE_BOOSTED_TOKEN: u32 = 16_800_245;
/// The number of tokens in the system start row the layout was derived from.
const INTERLEAVE_SYSTEM_TOKEN_COUNT: usize = 49_737;
/// The SHA-256 of that row's tokens, little-endian words.
const INTERLEAVE_SYSTEM_TOKENS_SHA256: &str =
    "6b86d8c6f6284dd6f69e61e0727bc49e56a0c322bc14c6fedb1ef0fb54d6d2c9";
/// The user start row's `[token, count]` items, in stored order.
const INTERLEAVE_ITEMS: [(u32, u32); 3] = [
    (16_800_245, 2_147_483_648),
    (16_800_252, 1),
    (16_800_244, 1),
];

/// `_interleave_miss_layout`: the frozen `[token, z, a]` layout for the
/// system start row, checked against the row and the boosted token.
fn interleave_miss_layout(system: &[u32], token: u32) -> Vec<(u32, u32)> {
    use sha2::Digest as _;
    let bytes: Vec<u8> = system.iter().flat_map(|item| item.to_le_bytes()).collect();
    let digest: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        INTERLEAVE_BOOSTED_TOKEN, token,
        "the boosted phrase token changed"
    );
    assert_eq!(
        INTERLEAVE_SYSTEM_TOKEN_COUNT,
        system.len(),
        "the system start gram changed"
    );
    assert_eq!(
        INTERLEAVE_SYSTEM_TOKENS_SHA256, digest,
        "the stored system token sequence changed"
    );
    INTERLEAVE_ITEMS.to_vec()
}

/// `user_gram_lookup`: the user start gram boosts 尼 (今天 for the
/// segmenter), then the operation runs.
fn user_gram_lookup(k: &mut Kit, kind: &str, operation: &str) -> J {
    let segment = operation == "phrase-segment";
    let boosted = tokens_of(k, if segment { "今天" } else { "尼" })[0];
    let value = if segment && kind == "unsorted" {
        let items = interleave_miss_layout(&system_start_tokens(k), boosted);
        gram_bytes(BOOST + 2, &items)
    } else {
        start_gram(kind, boosted)
    };
    replace_user_bigram(k, &[(1, value)]);
    match operation {
        "guess" => guess_out(k, "nihao"),
        "dynamic-adjust" => dynamic_adjust_out(k, "nihao"),
        "phrase-segment" => {
            let inst = k.inst();
            J::from(segment_out(inst, &["今天天气很好"]))
        }
        other => panic!("no operation {other}"),
    }
}

/// `user-gram-unsorted-range-boundary`: the user gram `[你, T, T]` for a ni
/// reading T below 你.
fn range_boundary(k: &mut Kit) -> J {
    let ni = tokens_of(k, "你")[0];
    let below = "泥拟逆腻妮倪匿溺尼"
        .chars()
        .map(|ch| tokens_of(k, &ch.to_string())[0])
        .find(|token| token + 1 < ni)
        .expect("a ni reading below 你");
    let items = [(ni, 1), (below, BOOST), (below, BOOST.wrapping_neg() + 1)];
    replace_user_bigram(k, &[(1, gram_bytes(BOOST, &items))]);
    guess_out(k, "nihao")
}

/// `_replace_system_start_row`: a private system whose bigram.db holds only
/// `value` under the sentence start.
fn replace_system_start_row(k: &mut Kit, value: Vec<u8>) {
    let inst = k.inst_if_any();
    if !inst.is_null() {
        facade::free_instance(inst);
    }
    facade::fini(k.ctx());
    k.forget();
    let copied = private_system(k);
    k.data = copied.clone();
    craft_hash(
        &copied.join("bigram.db"),
        &[(b"\x01\x00\x00\x00".to_vec(), value)],
    );
    write_user_conf(k);
}

/// `system_gram_lookup`: 尼 boosted in a private system start row.
fn system_gram_lookup(k: &mut Kit, kind: &str) -> J {
    let ni2 = tokens_of(k, "尼")[0];
    let value = if kind == "unsorted" {
        start_gram(kind, ni2)
    } else {
        gram_bytes(BOOST + 1, &[(ni2, 1), (ni2, BOOST)])
    };
    replace_system_start_row(k, value);
    guess_out(k, "nihao")
}

/// `user-gram-zero-count-guess`: the best one-syllable sentence of `a` held
/// at count 0 in the user start gram.
fn zero_count_guess(k: &mut Kit) -> J {
    replace_system_start_row(k, gram_bytes(1, &[(PAST_RANGES, 1)]));
    let first = guess_out(k, "a");
    let best: String = first.get("sentences").expect("sentences").items()[0]
        .clone()
        .into();
    let token = tokens_of(k, &best)[0];
    replace_user_bigram(k, &[(1, gram_bytes(0, &[(token, 0)]))]);
    guess_out(k, "a")
}

/// `_plant_user_grams(k, [_DALONG], grams)` with the short gram under the
/// imported token: the phrase added and saved, the context closed.
fn plant_short_dalong(k: &mut Kit) {
    let (text, pinyin, zhuyin, count) = DALONG;
    let it = facade::begin_add_phrases(k.ctx(), 7);
    let reading = if facade::ZHUYIN { zhuyin } else { pinyin };
    assert!(facade::iterator_add_phrase(
        it,
        c(text).as_ptr(),
        c(reading).as_ptr(),
        count
    ));
    facade::end_add_phrases(it);
    let user = tokens_of(k, text)
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("the user token");
    for text in ["你", "好", "号", "我"] {
        tokens_of(k, text);
    }
    assert!(facade::save(k.ctx()));
    replace_user_bigram(k, &[(user, SHORT.to_vec())]);
}

/// `abort-guess-sentence-short-user-gram-no-system-row`: a prefix guess
/// through the imported phrase whose user gram is short.
fn short_prefix_guess(k: &mut Kit) -> J {
    plant_short_dalong(k);
    let inst = k.inst();
    assert_eq!(
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr()),
        5
    );
    obj! { "ret" => facade::guess_sentence_with_prefix(inst, c(DALONG.0).as_ptr()) }
}

/// `phrase_prefix_gram`'s `result`: every phrase token, the slot past them
/// refused and zeroed.
fn phrase_result(inst: crate::support::P) -> Vec<u32> {
    let mut count = UNTOUCHED;
    assert!(facade::get_n_phrase(inst, &raw mut count));
    let tokens = (0..count)
        .map(|index| {
            let mut token = UNTOUCHED;
            assert!(facade::get_phrase_token(inst, index, &raw mut token));
            token
        })
        .collect();
    let mut token = UNTOUCHED;
    assert!(!facade::get_phrase_token(inst, count, &raw mut token));
    assert_eq!(token, 0);
    tokens
}

/// `phrase_prefix_gram`: an old failed segmentation, then 你 beside a
/// surrogate with a short (or sound) gram under the start or under 好.
fn phrase_prefix_gram(k: &mut Kit, short: bool, before: bool, unrelated: bool) -> J {
    let predecessor = if unrelated { tokens_of(k, "好")[0] } else { 1 };
    let value = if short {
        b"\x01\x00\x00".to_vec()
    } else {
        gram_bytes(1, &[])
    };
    replace_user_bigram(k, &[(predecessor, value)]);
    let inst = k.inst();
    let surrogate: &[u8] = b"\xed\xa0\x80";
    assert!(!facade::phrase_segment(
        inst,
        c(surrogate.repeat(3)).as_ptr()
    ));
    let old = phrase_result(inst);
    assert_eq!(old, [0, 0, 0]);
    let text = if before {
        [surrogate, "你".as_bytes()].concat()
    } else {
        ["你".as_bytes(), surrogate].concat()
    };
    let ret = facade::phrase_segment(inst, c(text).as_ptr());
    let tokens = phrase_result(inst);
    if short && !before && !unrelated {
        let mode = if facade::ZHUYIN { "zhuyin" } else { "pinyin" };
        assert_eq!(
            k.messages(),
            [Some(format!(
                "{mode}_phrase_segment: a user bigram value is shorter than a guint32 \
                 total_freq (upstream aborts, memory_chunk.h:390)"
            ))]
        );
        return obj! { "ret" => ret, "untouched_phrase_result" => tokens == old };
    }
    assert_eq!(tokens, [0, 0]);
    assert!(k.messages().is_empty());
    obj! { "ret" => ret, "tokens" => tokens }
}

/// `phrase_segment_nonscalar_raw_key`: a private library 1 holding one item
/// whose raw UCS-4 key is 0xd800, then its segmentation.
fn nonscalar_raw_key(k: &mut Kit) -> J {
    let system = private_system(k);
    let token: u32 = 0x0100_0001;
    let raw_text = 0xd800_u32.to_le_bytes();
    let mut content = vec![0_u8; 8];
    let offsets = [0_u32, 8];
    content.extend([1, 1]);
    content.extend(100_u32.to_le_bytes());
    content.extend(raw_text);
    content.extend(0x1084_u16.to_le_bytes());
    content.extend(10_u32.to_le_bytes());
    let index_two = 17 + 2 * 4 + 1;
    let content_len = u32::try_from(content.len()).expect("content");
    let mut payload: Vec<u8> = [200, 17, index_two, index_two + content_len + 1]
        .iter()
        .flat_map(|value: &u32| value.to_le_bytes())
        .collect();
    payload.push(b'#');
    payload.extend(offsets.iter().flat_map(|value| value.to_le_bytes()));
    payload.push(b'#');
    payload.extend(&content);
    payload.push(b'#');
    let path = system.join("gb_char.bin");
    std::fs::remove_file(&path).expect("unlink gb_char.bin");
    std::fs::write(&path, chunk(&payload)).expect("gb_char.bin");
    craft_tree(
        &system.join("phrase_index.bin"),
        &[(raw_text.to_vec(), token.to_le_bytes().to_vec())],
    );
    write_user_conf(k);
    k.data = system;
    let inst = k.inst();
    assert!(!facade::phrase_segment(
        inst,
        c(b"\xed\xbf\xbf".repeat(3)).as_ptr()
    ));
    let ret = facade::phrase_segment(inst, c(b"\xed\xa0\x80").as_ptr());
    let mut count = UNTOUCHED;
    assert!(facade::get_n_phrase(inst, &raw mut count));
    let tokens: Vec<u32> = (0..count)
        .map(|index| {
            let mut value = UNTOUCHED;
            assert!(facade::get_phrase_token(inst, index, &raw mut value));
            value
        })
        .collect();
    obj! { "ret" => ret, "tokens" => tokens }
}

/// `guess-candidates-short-user-gram-dynamic-adjust-off`: option word 0x18a,
/// the start gram short, the candidate guess at 0.
fn dynamic_adjust_off(k: &mut Kit) -> J {
    replace_user_bigram(k, &[(1, SHORT.to_vec())]);
    assert!(facade::set_options(k.ctx(), 0x18a));
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    let rows = guess_rows(inst, 0, 0x1e);
    obj! { "parsed" => parsed, "ret" => rows.ret, "rows" => rows.rows }
}
