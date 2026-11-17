use super::{
    BigramExportWalk, BigramStep, ExportedBigramRow, GramLoadFault, RenderedItem, SENTENCE_START,
};
use oxpinyin_core::{PhraseText, PhraseTextConcat};
use std::collections::HashMap;

/// The pair the walk holds, concatenated as the pin's `m_phrase` is
/// (`pinyin.cpp:821-829`); `None` after `drop_phrase`.
fn pair_text(walk: &BigramExportWalk) -> Option<PhraseText> {
    walk.pair().map(|(first, second)| {
        let mut phrase = PhraseTextConcat::new();
        phrase.push(first);
        phrase.push(second);
        phrase.finish()
    })
}

struct TestWalk {
    walk: BigramExportWalk,
    /// Planted grams; `None` plants a value too short for its total.
    grams: HashMap<u32, Option<Vec<(u32, u32)>>>,
    /// Planted items, handed to the walk's `render`.
    items: HashMap<u32, RenderedItem>,
}

impl TestWalk {
    fn has_next(&mut self) -> Result<bool, GramLoadFault> {
        let Self { walk, grams, items } = self;
        Self::advance(walk, grams, items)
    }

    fn get_next(&mut self) -> BigramStep {
        let Self { walk, grams, items } = self;
        walk.next_row(|walk| Self::advance(walk, grams, items))
    }

    fn advance(
        walk: &mut BigramExportWalk,
        grams: &HashMap<u32, Option<Vec<(u32, u32)>>>,
        items: &HashMap<u32, RenderedItem>,
    ) -> Result<bool, GramLoadFault> {
        walk.advance(
            |token| Self::gram(grams.get(&token), token),
            |token| items.get(&token).cloned(),
        )
    }

    fn gram(
        value: Option<&Option<Vec<(u32, u32)>>>,
        token: u32,
    ) -> Result<Vec<(u32, u32)>, GramLoadFault> {
        match value {
            Some(Some(gram)) => Ok(gram.clone()),
            Some(None) => Err(GramLoadFault::ShortTotal { token }),
            None => Ok(Vec::new()),
        }
    }
}

fn walk(items: &[u32], grams: &[(u32, &[(u32, u32)])]) -> TestWalk {
    TestWalk {
        walk: BigramExportWalk {
            items: items.iter().copied().collect(),
            ..BigramExportWalk::default()
        },
        items: [
            (0x0100_0001, "你", "ni"),
            (0x0100_0002, "好", "hao"),
            (0x0100_0003, "我", "wo"),
            (0x0100_0004, "爱", "ai"),
        ]
        .into_iter()
        .map(|(token, text, pinyin)| (token, (PhraseText::from(text), vec![pinyin.to_owned()])))
        .collect(),
        grams: grams
            .iter()
            .map(|(prev, gram)| (*prev, Some(gram.to_vec())))
            .collect(),
    }
}

/// A caller that trusts `get_next`'s return: stops on the first `false`.
fn drain_by_return(walk: &mut TestWalk) -> Vec<(PhraseText, bool)> {
    let mut rows = Vec::new();
    let mut more = walk.has_next().expect("no short gram");
    while more {
        let phrase = pair_text(&walk.walk);
        let BigramStep::Row(_, next) = walk.get_next() else {
            break;
        };
        let next = next.expect("no short gram");
        rows.push((phrase.expect("a loaded pair"), next));
        more = next;
    }
    rows
}

/// ibus-libpinyin's loop (`PYLibPinyin.cc:317-329`): `while (has_next)
/// get_next`.
fn drain_by_has_next(walk: &mut TestWalk) -> Vec<(PhraseText, bool)> {
    let mut rows = Vec::new();
    while walk.has_next().expect("no short gram") {
        let phrase = pair_text(&walk.walk);
        let BigramStep::Row(_, next) = walk.get_next() else {
            break;
        };
        rows.push((phrase.expect("a loaded pair"), next.expect("no short gram")));
    }
    rows
}

fn scenario() -> TestWalk {
    walk(
        &[SENTENCE_START, 0x0100_0001, 0x0100_0003],
        &[
            (SENTENCE_START, &[(0x0100_0003, 69)]),
            (0x0100_0001, &[(0x0100_0002, 138)]),
            (0x0100_0003, &[(0x0100_0004, 69)]),
        ],
    )
}

/// Register row 36 and the pin's walk (`pinyin.cpp:790-911`):
/// `get_next` answers `has_next` after the row, so a caller that
/// trusts it stops before the last predecessor, whose gram was loaded
/// but never scanned; a `sentence_start` gram's successors are scanned
/// under the next predecessor.
#[test]
fn the_walk_is_the_pins_state_machine() {
    let mut walk = scenario();
    assert_eq!(
        drain_by_return(&mut walk),
        vec![
            (PhraseText::from("你我"), true),
            (PhraseText::from("你好"), false)
        ]
    );
    // A further has_next scans the gram loaded but never scanned.
    assert_eq!(walk.has_next(), Ok(true));
    assert_eq!(pair_text(&walk.walk), Some("我爱".into()));
    assert_eq!(
        walk.get_next(),
        BigramStep::Row(
            ExportedBigramRow {
                pinyin: "wo'ai".to_owned(),
                count: 138,
            },
            Ok(false),
        )
    );
    assert_eq!(walk.has_next(), Ok(false));
    assert_eq!(walk.get_next(), BigramStep::Undefined);
}

/// ibus-libpinyin's `while (has_next)` loop therefore exports the last
/// predecessor's rows too.
#[test]
fn a_has_next_loop_sees_the_last_predecessor() {
    assert_eq!(
        drain_by_has_next(&mut scenario()),
        vec![
            (PhraseText::from("你我"), true),
            (PhraseText::from("你好"), false),
            (PhraseText::from("我爱"), false),
        ]
    );
}

/// The pin's NULL `m_phrase` (`pinyin.cpp:822`, `:830`): the rows
/// taken carry no phrase, and the next `has_next` loads the next pair
/// instead of answering from the dropped one.
#[test]
fn a_dropped_phrase_is_the_pins_null_m_phrase() {
    let mut walk = scenario();
    assert_eq!(walk.has_next(), Ok(true));
    assert_eq!(walk.walk.loads(), 1);
    assert_eq!(
        walk.walk.pair(),
        Some((&PhraseText::from("你"), &PhraseText::from("我")))
    );
    walk.walk.drop_phrase();
    assert_eq!(pair_text(&walk.walk), None);
    let BigramStep::Row(row, more) = walk.get_next() else {
        panic!("a row");
    };
    assert_eq!(row.pinyin, "ni'wo");
    assert_eq!(more, Ok(true));
    assert_eq!(walk.walk.loads(), 2);
    assert_eq!(
        walk.walk.pair(),
        Some((&PhraseText::from("你"), &PhraseText::from("好")))
    );
}

#[test]
fn get_next_without_a_predecessor_is_the_pins_abort() {
    let mut walk = walk(&[], &[]);
    assert_eq!(walk.has_next(), Ok(false));
    assert_eq!(walk.get_next(), BigramStep::Aborts);
}

/// Register row 85 (`pinyin.cpp:889`, `ngram.cpp:133-141`): the walk
/// faults where it loads a predecessor too short for its total, in
/// `get_all_items` order, after handing out the rows of the
/// predecessors before it; a `has_next` that loads it faults too, also
/// inside `get_next`.
#[test]
fn a_short_predecessor_faults_where_the_walk_loads_it() {
    let short = 0x0100_0003;
    let mut planted = walk(
        &[0x0100_0001, short, 0x0100_0004],
        &[(0x0100_0001, &[(0x0100_0002, 138), (0x0100_0004, 69)])],
    );
    planted.grams.insert(short, None);
    assert_eq!(planted.has_next(), Ok(true));
    assert_eq!(pair_text(&planted.walk), Some(PhraseText::from("你好")));
    let BigramStep::Row(_, Ok(true)) = planted.get_next() else {
        panic!("the first predecessor's first row");
    };
    // The second row is handed out, then the `has_next` inside
    // `get_next` loads the short predecessor; the row comes back with
    // the fault, for the C adapter to write before it aborts.
    let BigramStep::Row(row, more) = planted.get_next() else {
        panic!("the first predecessor's second row");
    };
    assert_eq!(row.count, 138);
    assert_eq!(more, Err(GramLoadFault::ShortTotal { token: short }));
    // A walk that reaches it from `has_next` faults the same way.
    let mut direct = walk(&[short], &[]);
    direct.grams.insert(short, None);
    assert_eq!(
        direct.has_next(),
        Err(GramLoadFault::ShortTotal { token: short })
    );
}

use super::{ContextCore, ExportAssert, ExportRow, ExportStep, PhraseExportCursor};
use oxpinyin_core::{ChewingKey, SyllableKey};
use oxpinyin_user::{KEY_TONE_SHIFT, MAX_KEY_TONE, USER_DICTIONARY, UserConfLaw, toned_key};

/// A transient context over the committed mini tables of whichever
/// backend this build carries (a NULL user directory: imports work, the
/// store is in memory, #642).
fn open() -> ContextCore {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
    let extension = oxpinyin_data::DEFAULT_STORE_EXT;
    ContextCore::open(
        &root.join(extension),
        None,
        crate::PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("committed backend fixtures")
}

#[test]
fn an_empty_bigram_export_stays_empty_after_the_first_write() {
    let mut ctx = open();
    let store = ctx.user_store_mut();
    assert_eq!(store.bigram_row(1).unwrap(), None);
    assert_eq!(store.bigram_count(1, 2).unwrap(), 0);
    assert_eq!(store.bigram_total(1).unwrap(), 0);
    assert!(store.bigram_successors(1).unwrap().is_empty());
    assert!(store.bigram_predecessors().unwrap().is_empty());
    assert!(store.bigram_predecessors().unwrap().is_empty());
    store.mask_out(u32::MAX, 0x0700_0001).unwrap();
    let mut walk = ctx.bigram_export_walk().unwrap();
    assert!(!walk.has_next(&ctx).unwrap());
    ctx.user_store_mut().set_bigram_count(1, 2, 69).unwrap();
    assert_eq!(ctx.user_store().bigram_count(1, 2).unwrap(), 69);
    assert_eq!(ctx.user_store().bigram_predecessors().unwrap(), [1]);
    assert!(!walk.has_next(&ctx).unwrap());
}

fn user_keys(spellings: &[&str], tone: u8) -> Vec<oxpinyin_user::PinyinKey> {
    spellings
        .iter()
        .map(|spelling| {
            toned_key(
                SyllableKey::from_text(spelling).expect("syllable").index(),
                tone,
            )
            .expect("key")
        })
        .collect()
}

/// A row as the C iterator writes it: phrase bytes, `'`-joined
/// pinyin and count.
type Row = (String, String, u64);

fn row(text: &str, pinyin: &str, count: u64) -> Row {
    (text.to_owned(), pinyin.to_owned(), count)
}

/// The row through the writers `pinyin_iterator_get_next_phrase`
/// uses; `None` for a NULL phrase.
fn render(row: &ExportRow<'_>) -> Option<Row> {
    let text = row.with_text_bytes(<[u8]>::to_vec)?;
    let pinyin: Vec<String> = row
        .pinyin_strings()
        .map(|(spelling, digit)| {
            let mut key = spelling.to_owned();
            key.extend(digit.map(char::from));
            key
        })
        .collect();
    Some((
        String::from_utf8(text).expect("scalar fixture text"),
        pinyin.join("'"),
        row.count(),
    ))
}

/// One `get_next`.
fn next_row(ctx: &ContextCore, cursor: &mut PhraseExportCursor) -> Option<Row> {
    match cursor.get_next_with(ctx, render) {
        ExportStep::Row(row) => row,
        ExportStep::Assert(_) => None,
    }
}

/// Every row the cursor still yields, driven as a C caller drives the
/// iterator (`while has_next { get_next }`); `None` once a call fails.
fn drain(ctx: &ContextCore, cursor: &mut PhraseExportCursor) -> Option<Vec<Row>> {
    let mut rows = Vec::new();
    while cursor.has_next() {
        rows.push(next_row(ctx, cursor)?);
    }
    Some(rows)
}

#[test]
fn begin_probes_one_token_and_the_guint8_alias_drains_like_library_1() {
    let ctx = open();
    let mut cursor = ctx.phrase_export_cursor(257);
    assert!(cursor.has_next());
    assert_eq!(cursor.next_token >> 24, 1, "the guint8 alias of library 1");
    assert_eq!(cursor.next_pronunciation, 0);
    let all = drain(&ctx, &mut cursor).expect("every row");
    assert!(!all.is_empty());
    let mut library = ctx.phrase_export_cursor(1);
    assert_eq!(all, drain(&ctx, &mut library).expect("every row"));
}

#[test]
fn an_unused_nibble_and_the_pins_asserts_are_reported() {
    let ctx = open();
    for index in [0, 8, 15, 16, 255] {
        let mut cursor = ctx.phrase_export_cursor(index);
        assert!(!cursor.has_next(), "nibble {index}");
        assert!(matches!(
            cursor.get_next_with(&ctx, |_| ()),
            ExportStep::Assert(ExportAssert::Item)
        ));
    }
    assert!(!PhraseExportCursor::default().has_next());
}

/// Promotion creates the missing sub-index (074a2219 phrase_index.h:663-670).
/// Export probes that live range (phrase_index.cpp:610-646,
/// pinyin.cpp:670-685,748-767).
#[test]
fn an_addon_promoted_into_a_missing_library_is_exported() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT);
    let system = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), system.path().join(entry.file_name())).unwrap();
    }
    let conf = system.path().join("table.conf");
    let stock = std::fs::read_to_string(&conf).unwrap();
    let user_row = "default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE";
    assert!(stock.contains(user_row));
    std::fs::write(
        &conf,
        stock.replace(user_row, "default ADDON_DICTIONARY NULL NULL NULL NOT_USED"),
    )
    .unwrap();
    let mut ctx = ContextCore::open(
        system.path(),
        None,
        crate::PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .unwrap();
    assert_eq!(ctx.user_store().addon_promotion_range_end(), None);
    assert!(!ctx.phrase_export_cursor(5).has_next());

    ctx.user_store_mut()
        .promote_addon_phrase(
            0x0500_0001,
            &oxpinyin_data::chunk_write::encode_phrase_item(
                &oxpinyin_data::chunk_write::ChunkItem {
                    phrase: "二簧".chars().map(u32::from).collect(),
                    unigram: 9,
                    prons: vec![(
                        ["er", "huang"]
                            .iter()
                            .map(|spelling| {
                                SyllableKey::from_text(spelling)
                                    .expect("syllable")
                                    .chewing_key()
                                    .to_packed()
                            })
                            .collect(),
                        7,
                    )],
                },
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        ctx.user_store().user_unigram_frequency(0x0500_0001),
        Some(9)
    );
    let mut cursor = ctx.phrase_export_cursor(5);
    assert_eq!(
        drain(&ctx, &mut cursor),
        Some(vec![row("二簧", "er'huang", 7)])
    );
}

/// `pinyin.cpp:728` → `chewing_key.cpp:48`: a reading the probe counts
/// (`:682`) but `get_pinyin_string` cannot spell is read and refused
/// with the cursor unmoved, so the next call refuses again (the parent
/// build skipped the reading). The content-table assert (`:50`) has no
/// case: a session key always names a syllable.
#[test]
fn a_reading_the_engine_cannot_spell_is_the_pins_pinyin_string_assert() {
    let jia = SyllableKey::from_text("jia").expect("syllable").index();
    let past_the_tones = toned_key(jia, MAX_KEY_TONE).expect("key") + (1 << KEY_TONE_SHIFT);
    let mut ctx = open();
    let store = ctx.user_store_mut();
    store
        .add_phrase_in(
            USER_DICTIONARY,
            &"你好".into(),
            &user_keys(&["ni", "hao"], 0),
            Some(5),
        )
        .expect("first");
    store
        .add_phrase_in(USER_DICTIONARY, &"甲".into(), &[past_the_tones], Some(2))
        .expect("representable session key");
    let mut cursor = ctx.phrase_export_cursor(u32::from(USER_DICTIONARY));
    assert_eq!(next_row(&ctx, &mut cursor), Some(row("你好", "ni'hao", 5)));
    assert!(cursor.has_next(), "the probe selects the item");
    for _ in 0..2 {
        assert!(matches!(
            cursor.get_next_with(&ctx, |_| ()),
            ExportStep::Assert(ExportAssert::KeyTone)
        ));
    }
}

/// `pinyin.cpp:748-767`: every `get_next_phrase` re-reads the range
/// and the next item, so a reading imported under the token the cursor
/// is about to read is exported with it.
#[test]
fn the_re_probe_reads_a_reading_imported_under_the_next_system_token() {
    let mut ctx = open();
    let mut cursor = ctx.phrase_export_cursor(1);
    let first = next_row(&ctx, &mut cursor);
    assert!(first.is_some());
    let next = cursor.next_token;
    assert_eq!(next >> 24, 1);
    assert_eq!(
        cursor.next_pronunciation, 0,
        "the next token's first reading"
    );
    let dict = ctx.runtime.dict();
    let (text, spellings, stored) = dict
        .live_items()
        .read(next, |item| {
            let text = item.phrase_text().expect("text");
            let spellings: Vec<&'static str> = item
                .pronunciation(0)
                .expect("reading")
                .keys
                .iter()
                .map(|bits| ChewingKey::from_packed(bits).pinyin_spelling())
                .collect();
            let stored: Vec<_> = item
                .pronunciations()
                .map(|pronunciation| {
                    let words: Vec<_> = pronunciation
                        .keys
                        .iter()
                        .map(|bits| ChewingKey::from_packed(bits).pinyin_spelling())
                        .collect();
                    (words.join("'"), u64::from(pronunciation.frequency))
                })
                .collect();
            (text, spellings, stored)
        })
        .found()
        .expect("item");
    // The same syllables with tone 1: a reading the item does not hold.
    let keys = user_keys(&spellings, 1);
    ctx.user_store_mut()
        .add_system_phrase_in(
            1,
            Some(next),
            &oxpinyin_core::PhraseText::from(text.as_str().unwrap()),
            &keys,
            Some(5),
        )
        .expect("import");
    let toned = spellings
        .iter()
        .map(|spelling| format!("{spelling}1"))
        .collect::<Vec<_>>()
        .join("'");
    let mut rows = Vec::new();
    for _ in 0..=stored.len() {
        rows.push(next_row(&ctx, &mut cursor).expect("row"));
    }
    let mut expected: Vec<_> = stored
        .iter()
        .map(|(pinyin, count)| row(text.as_str().unwrap(), pinyin, *count))
        .collect();
    expected.push(row(text.as_str().unwrap(), &toned, 5));
    assert_eq!(rows, expected);
}

/// The same for a `USER_FILE` library: a phrase appended after the
/// begin and a reading merged into the item under the cursor are both
/// exported, the pin's token walk over the live sub-index.
#[test]
fn a_user_library_import_after_begin_is_exported() {
    let mut ctx = open();
    let store = ctx.user_store_mut();
    store
        .add_phrase_in(
            USER_DICTIONARY,
            &"你好".into(),
            &user_keys(&["ni", "hao"], 0),
            Some(5),
        )
        .expect("first");
    let mut cursor = ctx.phrase_export_cursor(u32::from(USER_DICTIONARY));
    assert!(cursor.has_next());
    let store = ctx.user_store_mut();
    store
        .add_phrase_in(
            USER_DICTIONARY,
            &"好你".into(),
            &user_keys(&["hao", "ni"], 0),
            Some(7),
        )
        .expect("appended");
    store
        .add_phrase_in(
            USER_DICTIONARY,
            &"你好".into(),
            &user_keys(&["ni", "hao"], 2),
            Some(3),
        )
        .expect("merged reading");
    let rows = drain(&ctx, &mut cursor).expect("every row");
    assert_eq!(
        rows,
        vec![
            row("你好", "ni'hao", 5),
            row("你好", "ni2'hao2", 3),
            row("好你", "hao'ni", 7),
        ]
    );
}
