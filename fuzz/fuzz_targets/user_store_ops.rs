#![no_main]
//! Arbitrary mutation interleavings over the user store — the second
//! half of the G5 gap in the testing-strategy assessment
//! (`testing-strategy.md` §2): no target mutated `UserStore`, so the
//! add/remove/train/mask algebra had no hostile-sequence evidence.
//!
//! Each input drives a fresh libpinyin session over a fresh user
//! directory, the session the shipped libraries open (the previous
//! input's directory is removed at the start of the next one, so at most
//! one orphan remains per process — the same footprint `capi_commands`
//! leaves). The command stream mirrors `capi_commands`: one byte
//! selects the op, the remaining bytes feed it.
//!
//! Invariants, checked after **every** op:
//!
//! - no panic on any op sequence (constitution rule 4) — invalid
//!   phrases are the typed `UserStoreError::InvalidPhrase`, never a
//!   crash;
//! - the totals law the semantics suite pins for curated sequences
//!   holds after hostile ones too: for every bigram left-hand token the
//!   stream touched, `bigram_total(prev)` equals the sum of
//!   `bigram_successors(prev)` counts;
//! - the phrase index stays self-consistent: `token_for_phrase` and
//!   `phrase` are inverse on the live rows (checked in full while the
//!   store is small, on the freshest token once it grows).

use std::sync::Mutex;

use libfuzzer_sys::fuzz_target;
use oxpinyin_core::SyllableKey;
use oxpinyin_data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin_user::{
    phrase_index_library_index, PinyinKey, SystemOriginals, UserConfLaw, UserStore, UserStoreError,
    USER_DICTIONARY,
};
use tempfile::TempDir;

/// Real frozen syllables (a slice of the inventory the w4 fixtures
/// exercise); fuzz bytes index into this list to build valid keys.
const SYLLABLES: &[&str] = &[
    "a", "ai", "an", "ang", "ao", "ba", "bai", "ban", "bang", "bao", "bei", "ben", "beng", "bi",
    "bian", "biao", "bie", "bin", "bing", "bo", "bu", "cha", "chai", "chan", "chang", "chao",
    "che", "chen", "cheng", "chi", "chong", "chu", "chuan", "chuang", "chui", "chun", "ci", "cong",
    "cu", "cuan", "cui", "da", "dai", "dan", "dang", "dao", "de", "dei", "deng", "di", "dian",
    "diao", "die", "ding", "dong", "dou", "du", "duan", "dui", "dun", "duo", "e", "ei", "en", "er",
    "fa", "fan", "fang", "fei", "fen", "feng", "fo", "fou", "fu", "ga", "gai", "gan", "gang",
    "gao", "ge", "gei", "gen", "geng", "gong", "gou", "gu", "gua", "guai", "guan", "guang", "gui",
    "gun", "guo", "ha", "hai", "han", "hang", "hao", "he", "hei", "hen", "heng", "hong", "hou",
    "hu", "hua", "huai", "huan", "huang", "hui", "hun", "huo", "ji", "jia", "jian", "jiang",
    "jiao", "jie", "jin", "jing", "jiong", "jiu", "ju", "juan", "jue", "jun", "ka", "kai", "kan",
    "kang", "kao", "ke", "ken", "keng", "kong", "kou", "ku", "kua", "kuai", "kuan", "kuang", "kui",
    "kun", "kuo", "la", "lai", "lan", "lang", "lao", "le", "lei", "leng", "li", "lian", "liang",
    "liao", "lie", "lin", "ling", "liu", "long", "lou", "lu", "luan", "lun", "luo", "ma", "mai",
    "man", "mang", "mao", "me", "mei", "men", "meng", "mi", "mian", "miao", "mie", "min", "ming",
    "mo", "mou", "mu", "na", "nai", "nan", "nang", "nao", "ne", "nei", "nen", "neng", "ni", "nian",
    "niang", "niao", "nie", "nin", "ning", "niu", "nong", "nou", "nu", "nuan", "nuo", "o", "ou",
    "pa", "pai", "pan", "pang", "pao", "pei", "pen", "peng", "pi", "pian", "piao", "pie", "pin",
    "ping", "po", "pou", "pu", "qi", "qia", "qian", "qiang", "qiao", "qie", "qin", "qing", "qiong",
    "qiu", "qu", "quan", "que", "ran", "rang", "rao", "re", "ren", "reng", "ri", "rong", "rou",
    "ru", "ruan", "rui", "run", "ruo", "sa", "sai", "san", "sang", "sao", "se", "sen", "seng",
    "sha", "shai", "shan", "shang", "shao", "she", "shei", "shen", "sheng", "shi", "shou", "shu",
    "shua", "shuai", "shuan", "shuang", "shui", "shun", "shuo", "si", "song", "sou", "su", "suan",
    "sui", "sun", "ta", "tai", "tan", "tang", "tao", "te", "teng", "ti", "tian", "tiao", "tie",
    "ting", "tong", "tou", "tu", "tuan", "tui", "tun", "tuo", "wa", "wai", "wan", "wang", "wei",
    "wen", "weng", "wo", "wu", "xi", "xia", "xian", "xiang", "xiao", "xie", "xin", "xing", "xiong",
    "xiu", "xu", "xuan", "xue", "ya", "yan", "yang", "yao", "ye", "yi", "yin", "ying", "yong",
    "you", "yu", "yuan", "yue", "za", "zai", "zan", "zang", "zao", "ze", "zei", "zen", "zeng",
    "zha", "zhai", "zhan", "zhang", "zhao", "zhe", "zhen", "zheng", "zhi", "zhong", "zhou", "zhu",
    "zhua", "zhuai", "zhuan", "zhuang", "zhui", "zhun", "zhuo", "zi", "zong", "zou", "zu", "zuan",
    "zui", "zun",
];

/// The syllable a byte selects, as a store key.
fn key_for(byte: u8) -> PinyinKey {
    let text = SYLLABLES[usize::from(byte) % SYLLABLES.len()];
    let index = SyllableKey::from_text(text)
        .unwrap_or_else(|| panic!("{text:?} must be a frozen syllable"))
        .index();
    PinyinKey::try_from(index).unwrap_or_else(|_| panic!("{text:?} index must fit u16"))
}

/// The previous input's user directory, removed with the pin's file set
/// it holds when the next input takes its own. One orphan directory can
/// remain at process exit.
static LAST_PATH: Mutex<Option<TempDir>> = Mutex::new(None);

/// A user directory no earlier run can have written: a fresh unique
/// name, never a reused one, so `open_libpinyin` loads no profile.
fn fresh_dir() -> TempDir {
    if let Ok(mut last) = LAST_PATH.lock() {
        drop(last.take());
    }
    tempfile::Builder::new()
        .prefix("oxpinyin-fuzz-user-store-")
        .tempdir()
        .expect("a fresh user directory")
}

/// A session over `dir` with no system library: the stock layout's user,
/// network and addon libraries. `save` writes the pin's file set there,
/// so the dirty flag clears as the pin's does.
fn open_session(dir: &std::path::Path) -> UserStore {
    UserStore::open_libpinyin(
        dir,
        SystemOriginals::new(Default::default(), UserFileLayout::stock()),
        SystemVersions::for_this_build(7, 14),
        UserConfLaw::Pinyin,
    )
    .expect("a fresh session opens")
}

/// The totals law: a left-hand token's recorded total equals the sum of
/// its live successor counts.
fn check_totals(store: &UserStore, lefts: &[u32]) {
    for &prev in lefts {
        let successors = store.bigram_successors(prev).expect("successors read");
        let sum: u64 = successors.iter().map(|&(_, count)| count).sum();
        let total = store.bigram_total(prev).expect("total read");
        assert_eq!(
            sum, total,
            "bigram totals must equal the sum of successor counts (prev {prev})"
        );
    }
}

/// The phrase index inverse law, in full for small stores and on the
/// freshest row once the store grows (keeping per-op cost bounded).
fn check_phrase_index(store: &UserStore, freshest: Option<u32>) {
    let phrases = store.phrases().expect("phrase export");
    if phrases.len() <= 128 {
        for phrase in &phrases {
            let token = phrase.token();
            let words = phrase.words();
            let back = store
                .phrase(token)
                .expect("phrase lookup")
                .expect("live row");
            assert_eq!(
                back.words(),
                words,
                "token {token} must resolve to its text"
            );
            // The text index is keyed by scalar text; stored non-scalar
            // words have no `&str` form to look up.
            if let Some(text) = phrase
                .phrase_text()
                .and_then(|text| text.as_str().map(str::to_owned))
            {
                assert_eq!(
                    store.token_for_phrase(&text).expect("text lookup"),
                    Some(token),
                    "text {text:?} must resolve to its token"
                );
            }
        }
    } else if let Some(token) = freshest {
        if let Some(row) = store.phrase(token).expect("phrase lookup") {
            if let Some(text) = row
                .phrase_text()
                .and_then(|text| text.as_str().map(str::to_owned))
            {
                assert_eq!(
                    store.token_for_phrase(&text).expect("text lookup"),
                    Some(token),
                    "the freshest row must stay self-consistent"
                );
            }
        }
    }
}

/// Whether the user phrase table's entry for `text` holds user tokens that
/// are not exactly one token whose phrase is `text`. On tkrzw and Kyoto
/// Cabinet a `mask_out` walk leaves the previous record's tokens in an
/// entry it empties, as the pin's `MaskOutProcessor::ProcessFull` does
/// (`phrase_large_table3_tkrzwdb.cpp:288-289`, #785), and a later removal
/// can drop the row such a token names. `_add_phrase` then asserts at
/// `pinyin.cpp:554` (two tokens in the library), `:568` (another length,
/// or an absent row, which leaves a zero-length item) or `:571` (the same
/// length, other characters); the Rust store returns that error at the
/// pin's point, `InvalidPhrase`, and only the C adapters abort on it. This
/// target drives `UserStore` directly, so the add is refused, not crashed;
/// only a harness that calls the C API must avoid these sequences.
fn holds_foreign_token(store: &UserStore, text: &str) -> bool {
    let words: Vec<u32> = text.chars().map(u32::from).collect();
    let tokens: Vec<u32> = store
        .phrase_table_tokens_for_text(text)
        .expect("phrase table search")
        .into_iter()
        .filter(|&token| phrase_index_library_index(token) == USER_DICTIONARY)
        .collect();
    match tokens.as_slice() {
        [] => false,
        [token] => !store
            .phrase(*token)
            .expect("phrase lookup")
            .is_some_and(|row| row.words() == words.as_slice()),
        _ => true,
    }
}

fuzz_target!(|data: &[u8]| {
    let dir = fresh_dir();
    let mut store = open_session(dir.path());

    let mut added: Vec<u32> = Vec::new();
    let mut lefts: Vec<u32> = Vec::new();
    // Set once a mask_out has run: tracked tokens may legitimately be
    // gone from that point on.
    let mut masked_out = false;
    let mut cursor = 0;
    while cursor < data.len() {
        let command = data[cursor];
        cursor += 1;
        let payload = &data[cursor..];
        match command % 8 {
            // add a phrase: 1..=15 chars from the payload, one key per
            // char (valid by construction)
            0 | 1 => {
                let len = usize::from(payload.first().copied().unwrap_or(1)) % 15 + 1;
                let text: String = payload
                    .iter()
                    .skip(1)
                    .take(len)
                    .map(|&byte| char::from_u32(u32::from(byte)).expect("byte is a scalar"))
                    .collect();
                let keys: Vec<PinyinKey> = (0..text.chars().count())
                    .map(|index| key_for(index as u8))
                    .collect();
                if text.chars().count() == keys.len() && !text.is_empty() {
                    let refusable = holds_foreign_token(&store, &text);
                    match store.add_phrase(&oxpinyin_core::PhraseText::from(&text), &keys, None) {
                        Ok(token) => added.push(token),
                        Err(UserStoreError::InvalidPhrase) if refusable => {}
                        Err(err) => panic!("valid add: {err:?}"),
                    }
                }
            }
            // add with an explicit count derived from the payload
            2 => {
                let count = u64::from(payload.first().copied().unwrap_or(0)) % 1000;
                let keys = vec![key_for(payload.first().copied().unwrap_or(0))];
                let refusable = holds_foreign_token(&store, "词");
                match store.add_phrase(&"词".into(), &keys, Some(count)) {
                    Ok(token) => added.push(token),
                    Err(UserStoreError::InvalidPhrase) if refusable => {}
                    Err(err) => panic!("valid add with count: {err:?}"),
                }
            }
            // the typed invalid-phrase refusal: length ceiling violated
            // (exactly 16 chars with 16 keys), and length/key mismatch
            3 => {
                let over_limit: String = std::iter::repeat('词').take(16).collect();
                let keys16: Vec<PinyinKey> = (0..16).map(|index| key_for(index)).collect();
                assert!(matches!(
                    store.add_phrase(&oxpinyin_core::PhraseText::from(&over_limit), &keys16, None),
                    Err(UserStoreError::InvalidPhrase)
                ));
                assert!(matches!(
                    store.add_phrase(&"ab".into(), &[key_for(0)], None),
                    Err(UserStoreError::InvalidPhrase)
                ));
            }
            // remove: an allocated token first, then an absurd one —
            // absent removal is Ok(false), never a panic
            4 => {
                let pick = payload.first().copied().unwrap_or(0) as usize % added.len().max(1);
                if let Some(&token) = added.get(pick) {
                    // A phrase is removable only while every reading is
                    // indexed (`pinyin.cpp:3759`): a second reading merged
                    // by a later add is never indexed, so the removal is the
                    // pin's reading-removal assert.
                    let removable = store
                        .phrase(token)
                        .expect("phrase lookup")
                        .is_some_and(|row| row.pronunciations().iter().all(|p| p.indexed()));
                    // `pinyin_remove_user_candidate` asserts
                    // `ERROR_OK == retval` for every stored reading
                    // (`pinyin.cpp:3759`). `ChewingLargeTable2::remove_index`
                    // answers `ERROR_REMOVE_ITEM_DONOT_EXISTS`
                    // (`chewing_large_table2.cpp:200-219`) when a reading
                    // merged by `_add_phrase` (`:569-582`) has no index row,
                    // and when a mask left the entry without the token. The
                    // store returns that assert as the typed refusal and
                    // keeps the phrase, so the target must not treat it as a
                    // crash.
                    //
                    // This generator draws only complete syllables
                    // (`SYLLABLES`, `key_for`), so it cannot build an
                    // initial-only reading, whose two index spaces name one
                    // key. That shape is covered by the `oxpinyin-user` unit
                    // test `an_initial_only_reading_answers_the_pins_reading_removal_assert`,
                    // not by this generator.
                    let mut refused = false;
                    let removed = match store.remove_user_phrase(token) {
                        Ok(removed) => removed,
                        Err(UserStoreError::UserReadingRemoval) => {
                            assert!(
                                !removable || masked_out,
                                "the reading-removal refusal needs an unindexed reading or a mask"
                            );
                            refused = true;
                            false
                        }
                        Err(err) => panic!("remove: {err:?}"),
                    };
                    if !refused {
                        // `mask_out` deletes phrases whose token matches its
                        // mask, so a tracked token can already be gone after
                        // one; absent is only legal then or when a reading is
                        // unindexed (`pinyin.cpp:3759`).
                        assert!(
                            removed || masked_out || !removable,
                            "removing a live, fully indexed tracked token must report true"
                        );
                    }
                    // A refusal keeps the phrase, so the token stays tracked.
                    if removed || store.phrase(token).expect("phrase lookup").is_none() {
                        added.retain(|&other| other != token);
                    }
                }
                assert!(!store
                    .remove_user_phrase(u32::MAX - 7)
                    .expect("absent remove is Ok(false)"));
            }
            // bigram writes: explicit set, then the two observation
            // flavors (train-shaped)
            5 => {
                let prev = u32::from(payload.first().copied().unwrap_or(0)) % 8
                    + oxpinyin_user::FIRST_USER_TOKEN;
                let cur = u32::from(payload.get(1).copied().unwrap_or(0)) % 8
                    + oxpinyin_user::FIRST_USER_TOKEN
                    + 1;
                let count = u64::from(payload.get(2).copied().unwrap_or(0)) % 100;
                store
                    .set_bigram_count(prev, cur, count)
                    .expect("bigram set");
                store
                    .observe_selection(prev, cur)
                    .expect("selection observation");
                store
                    .observe_predicted(prev, cur)
                    .expect("prediction observation");
                if !lefts.contains(&prev) {
                    lefts.push(prev);
                }
            }
            // mask_out over a small mask/value pair; the totals law must
            // survive the rewrite, so `lefts` is kept
            6 => {
                let mask = [0xF000_0000, 0x0F00_0000, 0x00FF_0000]
                    [usize::from(payload.first().copied().unwrap_or(0)) % 3];
                let value = u32::from(payload.get(1).copied().unwrap_or(0)) & mask;
                store.mask_out(mask, value).expect("mask_out");
                masked_out = true;
            }
            // save, then the dirty flag is clear
            _ => {
                store.save().expect("save");
                assert!(!store.is_modified(), "save clears the dirty flag");
            }
        }
        check_totals(&store, &lefts);
        // Read after the mutation so a fresh add is the row checked.
        let freshest = added.last().copied();
        check_phrase_index(&store, freshest);
    }
    // The fini guard writes `user.conf` on drop, inside the directory.
    drop(store);
    if let Ok(mut last) = LAST_PATH.lock() {
        *last = Some(dir);
    }
});
