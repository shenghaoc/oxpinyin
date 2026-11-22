//! Arbitrary mutation interleavings over the user store
//! (`docs/testing/upstream-test-coverage.md`, "Fuzz harnesses"): no target mutated `UserStore`, so the
//! add/remove/train/mask algebra had no hostile-sequence evidence.
//!
//! Each input drives a fresh libpinyin session over a new user directory,
//! the session the shipped libraries open. The system directory is the
//! read-only fixture `fuzz/fixtures/user-store-ops`, parsed in `main` before
//! the input; the session and its user directory are closed and removed when
//! the input ends.
//!
//! The target runs one input per child (`LOOP_COUNT`): in
//! a persistent child, the same input covered different edges depending on
//! the input before it, although each input opens a new session over a new
//! user directory. LeakSanitizer checks at exit.
//!
//! # Input format
//!
//! An input is a sequence of commands. Each command is one selector byte,
//! reduced modulo 8, then its own arguments. A command consumes its
//! arguments, so no argument byte is also read as a selector. An argument
//! past the end of the input reads as zero bytes.
//!
//! - A *byte* is one byte.
//! - A *count* is one width byte, reduced modulo 4, that selects 1, 2, 4 or
//!   8 little-endian bytes. The 8-byte form reaches past `u32::MAX`.
//! - An *optional count* is one width byte: modulo 5, 4 is no count (the
//!   default count) and 0 to 3 select a count's width as above.
//! - A *reading* of `n` characters is `n` little-endian `u16` syllable
//!   selectors, each reduced modulo the 375 syllables of [`SYLLABLES`].
//! - A *phrase* is one length byte, reduced modulo 15, plus one: 1 to 15
//!   characters. Each character is a three-byte little-endian scalar,
//!   reduced modulo `0x11_0000` (a surrogate becomes U+FFFD), then its
//!   syllable selector.
//!
//! | Selector | Command | Arguments |
//! |---|---|---|
//! | 0 | add a phrase with the default count | phrase |
//! | 1 | add a phrase with a count | phrase, count |
//! | 2 | add a tracked phrase again, under a new reading | byte (token pick), reading, optional count |
//! | 3 | the two typed `InvalidPhrase` refusals | none |
//! | 4 | remove a tracked phrase, then an absent token | byte (token pick) |
//! | 5 | bigram writes | byte (mode), byte (left token), byte (right token), reserved count in modes 0 and 3 |
//! | 6 | `mask_out` | byte (mode), byte (mask), byte (token pick) |
//! | 7 | save | none |
//!
//! Command 2 picks a tracked phrase and reads one selector per character of
//! its text; with no tracked phrase it reads no reading and adds nothing.
//!
//! The bigram mode byte, modulo 4, selects training (0 or 1), prediction
//! (2), or training followed by prediction (3). The
//! left token is the byte modulo 8 above `FIRST_USER_TOKEN`; the right
//! token is one above that.
//!
//! # Invariants
//!
//! Checked after **every** command:
//!
//! - no panic on any command sequence (constitution rule 4) — invalid
//!   phrases are the typed `UserStoreError::InvalidPhrase`, never a crash;
//! - the bigram contracts of the store, per left-hand token the stream
//!   touched (see "Bigram rows");
//! - the phrase index stays self-consistent: `token_for_phrase` and
//!   `phrase` are inverse on the live rows (checked in full while the
//!   store is small, on the freshest token once it grows).
//!
//! # Bigram rows
//!
//! Each tracked row has a law, decided from what the stream did to it:
//!
//! - *exact*: the total equals the sum of the successor counts. A new row
//!   starts exact, and an empty row with a zero total is exact again.
//! - *modular*: the total equals that sum modulo 2^32. An exact row becomes
//!   modular when a prediction wraps its total; the store documents that
//!   both fields wrap there.
//! - A training observation answers a seed of at most `CEILING_SEED`, and
//!   `INITIAL_SEED` for an absent pair. It adds the seed to the count and
//!   the total, or, when the total cannot take the seed, changes nothing.
//! - A prediction observation answers `INITIAL_SEED` and adds it to the
//!   count and the total, both wrapping, or refuses with
//!   `UnigramTotalOverflow` and changes no bigram.
//! - `mask_out` keeps each row's law.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use oxpinyin::data::TableConf;
use oxpinyin::data::user_files::{SystemVersions, UserFileLayout};
use oxpinyin::user::{
    CEILING_SEED, FIRST_USER_TOKEN, INITIAL_SEED, LibraryLoopRows, PinyinKey, SystemOriginals,
    UserConfLaw, UserStore, UserStoreError,
};
use oxpinyin::vocab::{
    NETWORK_DICTIONARY, PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PhraseText, SyllableKey,
    USER_DICTIONARY, phrase_index_library_index, phrase_index_make_token,
};

/// The complete frozen syllables, the whole inventory the oracle fixtures
/// exercise; a two-byte selector indexes this list to build a valid key.
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

/// The masks the mask command selects from: the library index, the whole
/// token, the lowest bit and the low byte (they split the sequential user
/// tokens), the slot's high byte, and the four bits above the library
/// index, which take no part in the pin's test (`novel_types.h:41-44`).
const MASKS: [u32; 6] = [
    PHRASE_INDEX_LIBRARY_MASK,
    PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK,
    0x0000_0001,
    0x0000_00FF,
    0x00FF_0000,
    0xF000_0000,
];

/// The key for a syllable selector.
fn key_for(selector: u16) -> PinyinKey {
    let text = SYLLABLES[usize::from(selector) % SYLLABLES.len()];
    let index = SyllableKey::from_text(text)
        .unwrap_or_else(|| panic!("{text:?} must be a frozen syllable"))
        .index();
    PinyinKey::try_from(index).unwrap_or_else(|_| panic!("{text:?} index must fit u16"))
}

/// The bytes of one input the commands have not consumed.
struct Input<'a> {
    rest: &'a [u8],
}

impl Input<'_> {
    /// The next byte, or `None` at the end of the input.
    fn selector(&mut self) -> Option<u8> {
        let (&byte, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(byte)
    }

    /// A byte argument, 0 past the end.
    fn byte(&mut self) -> u8 {
        self.selector().unwrap_or(0)
    }

    /// `width` little-endian bytes, zero past the end.
    fn little_endian(&mut self, width: usize) -> u64 {
        (0..width).fold(0, |value, shift| {
            value | (u64::from(self.byte()) << (8 * shift))
        })
    }

    /// A count of 1, 2, 4 or 8 bytes.
    fn count(&mut self) -> u64 {
        let width = 1 << (self.byte() % 4);
        self.little_endian(width)
    }

    /// A count, or `None` for the default count.
    fn optional_count(&mut self) -> Option<u64> {
        match self.byte() % 5 {
            4 => None,
            width => Some(self.little_endian(1 << width)),
        }
    }

    /// One syllable selector.
    fn key(&mut self) -> PinyinKey {
        key_for(self.little_endian(2) as u16)
    }

    /// A phrase of 1 to 15 characters with one key per character.
    fn phrase(&mut self) -> (String, Vec<PinyinKey>) {
        let len = usize::from(self.byte()) % 15 + 1;
        let mut text = String::new();
        let mut keys = Vec::with_capacity(len);
        for _ in 0..len {
            let scalar = (self.little_endian(3) % 0x11_0000) as u32;
            text.push(char::from_u32(scalar).unwrap_or(char::REPLACEMENT_CHARACTER));
            keys.push(self.key());
        }
        (text, keys)
    }
}

/// The system directory every session opens over, with its parsed
/// `table.conf`: a committed, read-only fixture whose only rows are the
/// stock layout's addon, network and user libraries, so the library loop
/// maps no system chunk. `main` builds it before the first input.
fn system() -> &'static (&'static Path, TableConf) {
    static SYSTEM: OnceLock<(&'static Path, TableConf)> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let dir = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/user-store-ops"
        ));
        let conf = TableConf::load_from_dir(dir).expect("a table.conf");
        (dir, conf)
    })
}

/// A session over `dir` with no system library, opened as an init opens
/// one: the profile check, then the library loop over the layout's user,
/// network and addon rows. `save` writes the pin's file set there, so the
/// dirty flag clears as the pin's does.
fn open_session(dir: &Path) -> UserStore {
    let (system, conf) = system();
    let layout = UserFileLayout::from_conf(conf);
    let versions = SystemVersions::for_this_build(7, 14);
    let check = UserStore::check_profile(dir, &versions, &layout, UserConfLaw::Pinyin);
    let rows = LibraryLoopRows::new(system, conf, Some(dir));
    let mut originals = SystemOriginals::new(Default::default(), layout);
    UserStore::open_libpinyin_checked(
        dir,
        &mut originals,
        &rows,
        versions,
        UserConfLaw::Pinyin,
        check,
    )
    .expect("a fresh session opens")
}

/// What a tracked bigram row is known to satisfy ("Bigram rows").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Law {
    Exact,
    Modular,
}

/// One left-hand token's stored row: its total and its successors in
/// stored order.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    total: u64,
    successors: Vec<(u32, u64)>,
}

impl Row {
    fn read(store: &UserStore, prev: u32) -> Self {
        Row {
            total: store.bigram_total(prev).expect("total read"),
            successors: store.bigram_successors(prev).expect("successors read"),
        }
    }

    fn sum(&self) -> u64 {
        self.successors.iter().map(|&(_, count)| count).sum()
    }

    /// The count the store reads for `cur`: the first match.
    fn count(&self, cur: u32) -> Option<u64> {
        self.successors
            .iter()
            .find(|&&(token, _)| token == cur)
            .map(|&(_, count)| count)
    }

    /// The successors other than `cur`, in stored order.
    fn others(&self, cur: u32) -> Vec<(u32, u64)> {
        self.successors
            .iter()
            .copied()
            .filter(|&(token, _)| token != cur)
            .collect()
    }
}

const U32_MAX: u64 = u32::MAX as u64;

/// Checks each tracked row against its law, and makes an empty row with a
/// zero total exact again.
fn check_laws(store: &UserStore, laws: &mut BTreeMap<u32, Law>) {
    for (&prev, law) in laws.iter_mut() {
        let row = Row::read(store, prev);
        match law {
            Law::Exact => assert_eq!(
                row.sum(),
                row.total,
                "an exact row's total must equal the sum of its successor counts (prev {prev})"
            ),
            Law::Modular => assert_eq!(
                row.sum() % (U32_MAX + 1),
                row.total,
                "a modular row's total must equal the sum of its counts modulo 2^32 (prev {prev})"
            ),
        }
        if row.successors.is_empty() && row.total == 0 {
            *law = Law::Exact;
        }
    }
}

/// The training observation's contract on one row.
fn observe_selection(store: &mut UserStore, prev: u32, cur: u32) {
    let before = Row::read(store, prev);
    let seed = store
        .observe_selection(prev, cur)
        .expect("selection observation");
    let after = Row::read(store, prev);
    assert!(
        seed <= CEILING_SEED,
        "a training seed is at most CEILING_SEED, not {seed}"
    );
    let Some(count) = before.count(cur) else {
        assert_eq!(seed, INITIAL_SEED, "an absent pair trains the initial seed");
        if before.total + seed > U32_MAX {
            assert_eq!(after, before, "a refused training changes nothing");
        } else {
            assert_eq!(after.count(cur), Some(seed));
            assert_eq!(after.total, before.total + seed);
            assert_eq!(after.others(cur), before.others(cur));
        }
        return;
    };
    if seed == 0 || before.total + seed > U32_MAX {
        assert_eq!(
            after, before,
            "a training that the total cannot take, or a zero seed, changes nothing"
        );
    } else {
        assert_eq!(
            after.count(cur),
            Some((count + seed) % (U32_MAX + 1)),
            "training adds the seed to the count (prev {prev}, cur {cur})"
        );
        assert_eq!(
            after.total,
            before.total + seed,
            "training adds the seed to the total"
        );
        assert_eq!(after.others(cur), before.others(cur));
    }
}

/// The prediction observation's contract on one row.
fn observe_predicted(store: &mut UserStore, law: &mut Law, prev: u32, cur: u32) {
    let before = Row::read(store, prev);
    match store.observe_predicted(prev, cur) {
        Ok(seed) => {
            assert_eq!(seed, INITIAL_SEED, "a prediction trains the flat seed");
            let after = Row::read(store, prev);
            let wrap = |value: u64| (value + seed) % (U32_MAX + 1);
            assert_eq!(
                after.count(cur),
                Some(wrap(before.count(cur).unwrap_or(0))),
                "a prediction adds the seed to the count, wrapping (prev {prev}, cur {cur})"
            );
            assert_eq!(
                after.total,
                wrap(before.total),
                "a prediction adds the seed to the total, wrapping"
            );
            assert_eq!(after.others(cur), before.others(cur));
            if *law == Law::Exact && before.total + seed > U32_MAX {
                *law = Law::Modular;
            }
        }
        Err(UserStoreError::UnigramTotalOverflow) => {
            assert_eq!(
                Row::read(store, prev),
                before,
                "a refused prediction trains no bigram"
            );
        }
        Err(err) => panic!("prediction observation: {err:?}"),
    }
}

/// The phrase index inverse law, in full for small stores and on the
/// freshest row once the store grows (keeping per-op cost bounded).
fn check_phrase_index(store: &UserStore, freshest: Option<u32>) {
    let phrases = store.phrases();
    if phrases.len() <= 128 {
        for phrase in &phrases {
            let token = phrase.token();
            let words = phrase.words();
            let back = store.phrase(token).expect("live row");
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
    } else if let Some(token) = freshest
        && let Some(row) = store.phrase(token)
        && let Some(text) = row
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
            .is_some_and(|row| row.words() == words.as_slice()),
        _ => true,
    }
}

/// One input per child: in a persistent child, the same input covered
/// different edges depending on the input before it, although each input
/// opens a new session and user directory. LeakSanitizer's check at exit then
/// names that input. `AFL_FUZZER_LOOPCOUNT` overrides it.
const LOOP_COUNT: usize = 1;

fn main() {
    system();
    oxpinyin_fuzz::fuzz(LOOP_COUNT, &[], run);
}

/// Adds `text` under `keys`; a refusal is legal only where
/// [`holds_foreign_token`] says the pin asserts.
fn add(
    store: &mut UserStore,
    added: &mut Vec<u32>,
    text: &str,
    keys: &[PinyinKey],
    count: Option<u64>,
) {
    let refusable = holds_foreign_token(store, text);
    match store.add_phrase(&PhraseText::from(text), keys, count) {
        Ok(token) => {
            if !added.contains(&token) {
                added.push(token);
            }
        }
        Err(UserStoreError::InvalidPhrase) if refusable => {}
        Err(err) => panic!("valid add (count {count:?}): {err:?}"),
    }
}

/// Runs one input.
fn run(data: &[u8]) {
    let dir = tempfile::Builder::new()
        .prefix("oxpinyin-fuzz-user-store-")
        .tempdir()
        .expect("a fresh user directory");
    let mut store = open_session(dir.path());

    let mut input = Input { rest: data };
    let mut added: Vec<u32> = Vec::new();
    let mut laws: BTreeMap<u32, Law> = BTreeMap::new();
    // Set once a mask_out has run: tracked tokens may legitimately be
    // gone from that point on.
    let mut masked_out = false;
    while let Some(command) = input.selector() {
        match command % 8 {
            0 => {
                let (text, keys) = input.phrase();
                add(&mut store, &mut added, &text, &keys, None);
            }
            1 => {
                let (text, keys) = input.phrase();
                let count = input.count();
                add(&mut store, &mut added, &text, &keys, Some(count));
            }
            // the same text under another reading: the merge path
            2 => {
                let pick = usize::from(input.byte());
                let token = added.get(pick % added.len().max(1)).copied();
                let text = token
                    .and_then(|token| store.phrase(token))
                    .and_then(|row| row.phrase_text())
                    .and_then(|text| text.as_str().map(str::to_owned));
                let length = text.as_ref().map_or(0, |text| text.chars().count());
                let keys: Vec<PinyinKey> = (0..length).map(|_| input.key()).collect();
                let count = input.optional_count();
                if let Some(text) = text {
                    add(&mut store, &mut added, &text, &keys, count);
                }
            }
            // the typed invalid-phrase refusal: length ceiling violated
            // (exactly 16 chars with 16 keys), and length/key mismatch
            3 => {
                let over_limit: String = std::iter::repeat_n('词', 16).collect();
                let keys16: Vec<PinyinKey> = (0..16).map(key_for).collect();
                assert!(matches!(
                    store.add_phrase(&PhraseText::from(&over_limit), &keys16, None),
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
                let pick = usize::from(input.byte()) % added.len().max(1);
                if let Some(&token) = added.get(pick) {
                    // A phrase is removable only while every reading is
                    // indexed (`pinyin.cpp:3759`): a second reading merged
                    // by a later add is never indexed, so the removal is the
                    // pin's reading-removal assert.
                    let removable = store
                        .phrase(token)
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
                    // key. That shape is covered by the `oxpinyin` unit
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
                    if removed || store.phrase(token).is_none() {
                        added.retain(|&other| other != token);
                    }
                }
                assert!(
                    !store
                        .remove_user_phrase(u32::MAX - 7)
                        .expect("absent remove is Ok(false)")
                );
            }
            // bigram writes, each against its contract ("Bigram rows")
            5 => {
                let mode = input.byte() % 4;
                let prev = u32::from(input.byte()) % 8 + FIRST_USER_TOKEN;
                let cur = u32::from(input.byte()) % 8 + FIRST_USER_TOKEN + 1;
                let law = laws.entry(prev).or_insert(Law::Exact);
                if mode == 0 || mode == 3 {
                    // Consume the count field already present in the corpus.
                    let _ = input.count();
                }
                if mode != 2 {
                    observe_selection(&mut store, prev, cur);
                }
                if mode == 2 || mode == 3 {
                    observe_predicted(&mut store, law, prev, cur);
                }
            }
            // mask_out; each row's law must survive the rewrite. The pin
            // accepts any mask and value: the facade tests the library
            // through the four-bit library index of both, and the sub-index
            // tests each token under their low 24 bits
            // (`phrase_index.cpp:677-717` at 074a2219).
            //
            // The mode byte selects the library, the mask byte the mask and
            // the pick byte a tracked token. The value comes from that
            // token, so the matching mode removes a live phrase; a value
            // from one argument byte is 0 under every mask with clear low
            // bits.
            6 => {
                let other_library = input.byte() & 1 == 1;
                let mask = MASKS[usize::from(input.byte()) % MASKS.len()];
                let pick = usize::from(input.byte());
                let token = added
                    .get(pick % added.len().max(1))
                    .copied()
                    .unwrap_or(FIRST_USER_TOKEN);
                let was_live = store.phrase(token).is_some();
                let (mask, value) = if other_library {
                    // The token's value moved to NETWORK_DICTIONARY, under a
                    // mask that tests the whole library index: the facade's
                    // library check refuses the user library
                    // (`phrase_index.cpp:707-711`), and no user token
                    // matches in the index tables.
                    let mask = mask | PHRASE_INDEX_LIBRARY_MASK;
                    let value = (token & mask & !PHRASE_INDEX_LIBRARY_MASK)
                        | phrase_index_make_token(NETWORK_DICTIONARY, 0);
                    (mask, value)
                } else {
                    // `token & mask == value` holds for the token itself.
                    (mask, token & mask)
                };
                match store.mask_out(mask, value) {
                    Ok(()) => {}
                    Err(err) => panic!("mask_out: {err:?}"),
                }
                masked_out = true;
                if other_library {
                    assert!(
                        !was_live || store.phrase(token).is_some(),
                        "a mask of another library must keep token {token:#010x} \
                         (mask {mask:#010x}, value {value:#010x})"
                    );
                } else {
                    assert!(
                        store.phrase(token).is_none(),
                        "a matching mask must remove token {token:#010x} \
                         (mask {mask:#010x}, value {value:#010x})"
                    );
                }
            }
            // save, then the dirty flag is clear
            _ => {
                store.save_reporting().expect("save");
                assert!(!store.is_modified(), "save clears the dirty flag");
            }
        }
        check_laws(&store, &mut laws);
        // Read after the mutation so a fresh add is the row checked.
        let freshest = added.last().copied();
        check_phrase_index(&store, freshest);
    }
    // The fini guard writes `user.conf` on drop, inside the directory; the
    // directory goes after it.
    drop(store);
    drop(dir);
}
