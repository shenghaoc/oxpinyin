use super::{
    BigramExportWalk, BigramProbeFault, BigramStep, ExportedBigramRow, GramLoadFault, RenderedItem,
    SENTENCE_START,
};
use crate::ucs4::PhraseTextConcat;
use crate::user::seed_bigram;
use crate::vocab::PhraseText;
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
    fn has_next(&mut self) -> Result<bool, BigramProbeFault> {
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
    ) -> Result<bool, BigramProbeFault> {
        walk.advance(
            |token| Self::gram(grams.get(&token), token),
            |token| items.get(&token).cloned().unwrap_or_default(),
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

/// A planted item with one stored reading, and the assert site of that
/// reading.
fn item(text: &str, pinyin: &str, site: Option<ExportAssert>) -> RenderedItem {
    RenderedItem {
        text: PhraseText::from(text),
        pinyins: vec![pinyin.to_owned()],
        sites: vec![site],
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
        .map(|(token, text, pinyin)| (token, item(text, pinyin, None)))
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

// 074a2219 pinyin.cpp:790–911 advances and reports the successor state after each row.
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

// 074a2219 pinyin.cpp:822,830 loads the next pair after the caller drops the phrase.
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

// 074a2219 pinyin.cpp:790–911 requires a current predecessor for get_next.
#[test]
fn get_next_without_a_predecessor_is_the_pins_abort() {
    let mut walk = walk(&[], &[]);
    assert_eq!(walk.has_next(), Ok(false));
    assert_eq!(walk.get_next(), BigramStep::Aborts);
}

// 074a2219 ngram.cpp:133–141 reads the predecessor when the walk advances.
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
    let BigramStep::Row(row, more) = planted.get_next() else {
        panic!("the first predecessor's second row");
    };
    assert_eq!(row.count, 138);
    assert_eq!(
        more,
        Err(BigramProbeFault::Gram(GramLoadFault::ShortTotal {
            token: short
        }))
    );
    let mut direct = walk(&[short], &[]);
    direct.grams.insert(short, None);
    assert_eq!(
        direct.has_next(),
        Err(BigramProbeFault::Gram(GramLoadFault::ShortTotal {
            token: short
        }))
    );
}

// 074a2219 pinyin.cpp:839–872 renders each first reading before the second readings.
#[test]
fn a_pair_asserts_in_the_pins_render_order() {
    use ExportAssert::{KeyFields, KeyTone};
    assert_eq!(super::pair_assert(&[], &[Some(KeyTone)]), None);
    assert_eq!(super::pair_assert(&[None], &[]), None);
    assert_eq!(
        super::pair_assert(&[None, Some(KeyTone)], &[]),
        Some(KeyTone)
    );
    assert_eq!(
        super::pair_assert(&[Some(KeyFields)], &[Some(KeyTone)]),
        Some(KeyFields)
    );
    assert_eq!(
        super::pair_assert(&[None, Some(KeyFields)], &[None, Some(KeyTone)]),
        Some(KeyTone)
    );
}

use super::{ContextCore, ExportAssert, ExportRow, ExportStep, PhraseExportCursor};
use crate::parser::ChewingKey;
use crate::user::{UserConfLaw, toned_key};
use crate::vocab::{SyllableKey, USER_DICTIONARY};

/// A transient context over the committed mini tables of whichever
/// backend this build carries (a NULL user directory: imports work, the
/// store is in memory, #642).
fn open() -> ContextCore {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/w3");
    let extension = crate::__internal::DEFAULT_STORE_EXT;
    ContextCore::open(
        &root.join(extension),
        None,
        crate::facade::PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("committed backend fixtures")
}

// Rust contract: begin captures the current predecessor list.
#[test]
fn an_empty_bigram_export_stays_empty_after_the_first_write() {
    let mut ctx = open();
    let mut walk = ctx.bigram_export_walk().unwrap();
    assert!(!walk.has_next(&ctx).unwrap());
    seed_bigram(ctx.user_store_mut(), 1, &[(2, 69)]);
    assert!(!walk.has_next(&ctx).unwrap());
}

fn user_keys(spellings: &[&str], tone: u8) -> Vec<crate::user::PinyinKey> {
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

// 074a2219 pinyin.cpp:670–767 exports the stored phrase, reading and frequency.
#[test]
fn export_phrases_render_the_pinned_triples() {
    use crate::vocab::USER_DICTIONARY;
    let user = crate::user::native_fixture::TempUserDir::new("export-stored-triples");
    let mut context = crate::facade::test_fixture::native_context(
        &crate::user::native_fixture::system_dir(),
        &user.path,
        crate::user::UserConfLaw::Pinyin,
    );
    for (text, pinyin, count) in [
        ("你好", "ni'hao", None),
        ("你好", "ni'hao", Some(7)),
        ("世界", "shi'jie", Some(3)),
    ] {
        let keys = crate::facade::test_fixture::native_keys(pinyin);
        context
            .user_store_mut()
            .add_phrase(&text.into(), &keys, count)
            .unwrap();
    }
    let mut cursor = context.phrase_export_cursor(u32::from(USER_DICTIONARY));
    assert_eq!(
        next_row(&context, &mut cursor),
        Some(row("你好", "ni'hao", 12))
    );
    assert_eq!(
        next_row(&context, &mut cursor),
        Some(row("世界", "shi'jie", 3))
    );
    assert!(!cursor.has_next());
}

// 074a2219 pinyin.cpp:670–685 probes the narrowed library index.
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

// 074a2219 pinyin.cpp:670–767 distinguishes unused and invalid library indices.
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

// 074a2219 phrase_index.h:663–670 makes the promoted sub-index visible.
#[test]
fn an_addon_promoted_into_a_missing_library_is_exported() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/w3")
        .join(crate::__internal::DEFAULT_STORE_EXT);
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
        crate::facade::PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .unwrap();
    assert_eq!(ctx.user_store().addon_promotion_range_end(), None);
    assert!(!ctx.phrase_export_cursor(5).has_next());

    ctx.user_store_mut()
        .promote_addon_phrase(
            0x0500_0001,
            &crate::data::phrase_library::encode_phrase_item(
                &crate::data::phrase_library::ChunkItem {
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

// 074a2219 pinyin.cpp:748–767 re-probes the live range after each row.
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
    let keys = user_keys(&spellings, 1);
    ctx.user_store_mut()
        .add_system_phrase_in(
            1,
            Some(next),
            &crate::vocab::PhraseText::from(text.as_str().unwrap()),
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

// 074a2219 pinyin.cpp:748–767 reads imports made after begin.
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

mod native {
    use super::super::{BigramExportWalk, BigramProbeFault, BigramStep, ExportAssert, ExportStep};
    use crate::facade::ContextCore;
    #[cfg(unix)]
    use crate::facade::SaveOutcome;
    use crate::facade::test_fixture::{native_fixture::*, *};
    use crate::user::UserConfLaw;

    #[derive(Debug, PartialEq)]
    enum WalkStep {
        NoNext,
        Row(bool, String, String, i64),
    }

    fn row(context: &ContextCore, walk: &mut BigramExportWalk) -> WalkStep {
        let (first, second) = walk.pair().unwrap();
        let text = format!("{}{}", first.as_str().unwrap(), second.as_str().unwrap());
        let BigramStep::Row(row, more) = walk.get_next(context) else {
            panic!("a buffered row")
        };
        WalkStep::Row(more.unwrap(), text, row.pinyin, row.count)
    }

    fn walk_steps(context: &ContextCore) -> Vec<WalkStep> {
        let mut walk = context.bigram_export_walk().unwrap();
        let mut steps = Vec::new();
        let mut falses = 0;
        while falses < 2 && steps.len() < 16 {
            if !walk.has_next(context).unwrap() {
                falses += 1;
                steps.push(WalkStep::NoNext);
                continue;
            }
            falses = 0;
            steps.push(row(context, &mut walk));
        }
        steps
    }

    fn lay_gram(user: &std::path::Path, predecessor: u32, items: &[(u32, u32)]) {
        let total: u32 = items.iter().map(|(_, count)| count).sum();
        let planted = TempUserDir::new("export-gram-plant");
        plant_bigram(
            &planted.path,
            vec![(predecessor.to_le_bytes().to_vec(), gram_bytes(total, items))],
        );
        std::fs::copy(
            planted.path.join("user_bigram.db"),
            user.join("user_bigram.db"),
        )
        .unwrap();
    }

    fn export_walk(
        tag: &str,
        items: impl FnOnce(u32, u32, u32) -> Vec<(u32, u32)>,
    ) -> Vec<WalkStep> {
        let user = TempUserDir::new(tag);
        let dalong = saved_user_phrase(&user.path);
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let [ni, hao, hao_alt] = ["你", "好", "号"].map(|text| tokens(&context, text)[0]);
        context.finish();
        drop(context);
        lay_gram(&user.path, ni, &items(hao, hao_alt, dalong));
        let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        walk_steps(&context)
    }

    // 074a2219 pinyin.cpp:906 doubles the stored count.
    #[test]
    fn the_bigram_export_count_wraps() {
        let user = TempUserDir::new("d18-export-ffffffff");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let [ni, hao, zhong] = ["你", "好", "中"].map(|text| tokens(&context, text)[0]);
        context.finish();
        drop(context);
        let gram = gram_bytes(u32::MAX, &[(hao, u32::MAX)]);
        plant_bigram(
            &user.path,
            vec![
                (ni.to_le_bytes().to_vec(), gram.clone()),
                (zhong.to_le_bytes().to_vec(), gram),
                (1_u32.to_le_bytes().to_vec(), vec![0; 4]),
            ],
        );
        let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let mut walk = context.bigram_export_walk().unwrap();
        let mut rows = Vec::new();
        while walk.has_next(&context).unwrap() {
            let WalkStep::Row(_, text, reading, count) = row(&context, &mut walk) else {
                unreachable!()
            };
            rows.push((text, reading, count));
        }
        rows.sort();
        assert_eq!(
            rows,
            [
                ("中好".to_owned(), "zhong'hao".to_owned(), -2),
                ("你好".to_owned(), "ni'hao".to_owned(), -2)
            ]
        );
    }

    // 074a2219 chewing_key.cpp:29–57 validates the tone before the key fields.
    #[rstest::rstest]
    #[case::tone(0x6081, ExportAssert::KeyTone)]
    #[case::fields(0x109f, ExportAssert::KeyFields)]
    fn export_refuses_an_invalid_system_reading(#[case] key: u16, #[case] expected: ExportAssert) {
        let scratch = tempfile::tempdir().unwrap();
        let system = items_system(
            &system_dir(),
            scratch.path(),
            &[(&[0x4f60], &[key], 100)],
            1000,
        );
        let user = TempUserDir::new("export-system-key");
        let context = native_context(&system, &user.path, UserConfLaw::Pinyin);
        let mut cursor = context.phrase_export_cursor(1);
        assert!(
            matches!(cursor.get_next_with(&context, |_| ()), ExportStep::Assert(site) if site == expected)
        );
    }

    // 074a2219 pinyin.cpp:846,859,910 probes the next pair after returning the current row.
    #[rstest::rstest]
    #[case::tone(0x6081, ExportAssert::KeyTone)]
    #[case::fields(0x109f, ExportAssert::KeyFields)]
    fn bigram_export_refuses_an_invalid_system_reading(
        #[case] key: u16,
        #[case] expected: ExportAssert,
    ) {
        let scratch = tempfile::tempdir().unwrap();
        let system = items_system(
            &system_dir(),
            scratch.path(),
            &[(&[0x4f60], &[0x1081], 100), (&[0x597d], &[key], 100)],
            1000,
        );
        let (good, bad) = (0x0100_0001_u32, 0x0100_0002_u32);
        let user = TempUserDir::new("export-bigram-system-key");
        plant_bigram(
            &user.path,
            vec![(
                good.to_le_bytes().to_vec(),
                gram_bytes(200, &[(good, 100), (bad, 100)]),
            )],
        );
        let context = native_context(&system, &user.path, UserConfLaw::Pinyin);
        let mut walk = context.bigram_export_walk().unwrap();
        let fault = Err(BigramProbeFault::Key(expected));
        assert_eq!(walk.has_next(&context), Ok(false));
        assert_eq!(walk.has_next(&context), Ok(true));
        let BigramStep::Row(row, more) = walk.get_next(&context) else {
            panic!("the row of the pair before the fault")
        };
        assert_eq!(row.count, 200);
        assert_eq!(more, fault);
        for _ in 0..2 {
            assert_eq!(walk.has_next(&context), fault);
        }
    }

    // 074a2219 ngram.cpp:133–151 returns all stored items in their original order.
    #[rstest::rstest]
    #[case::unsorted(false, &[("你龘龘", "ni'da2'da2", 180), ("你号", "ni'hao", 160), ("你好", "ni'hao", 140)])]
    #[case::duplicate(true, &[("你好", "ni'hao", 140), ("你好", "ni'hao", 160), ("你龘龘", "ni'da2'da2", 180), ("你龘龘", "ni'da2'da2", 200)])]
    fn user_gram_unsorted_export_follows_the_stored_order(
        #[case] duplicate: bool,
        #[case] rows: &[(&str, &str, i64)],
    ) {
        let steps = export_walk("export-stored-order", |hao, hao_alt, dalong| {
            if duplicate {
                vec![(hao, 70), (hao, 80), (dalong, 90), (dalong, 100)]
            } else {
                let mut items = vec![(dalong, 90), (hao_alt, 80), (hao, 70)];
                items.sort_by(|a, b| b.cmp(a));
                items
            }
        });
        let mut expected = vec![WalkStep::NoNext];
        for (index, &(phrase, reading, count)) in rows.iter().enumerate() {
            expected.push(WalkStep::Row(
                index + 1 < rows.len(),
                phrase.to_owned(),
                reading.to_owned(),
                count,
            ));
        }
        expected.extend([WalkStep::NoNext, WalkStep::NoNext]);
        assert_eq!(steps, expected);
    }

    // 074a2219 pinyin.cpp:791–792,809,906 exports and doubles counts above 68.
    #[cfg(unix)]
    #[test]
    fn the_bigram_export_skips_a_count_at_the_threshold() {
        let steps = export_walk("export-threshold", |hao, hao_alt, _| {
            vec![(hao, 68), (hao_alt, 69)]
        });
        assert_eq!(
            steps,
            [
                WalkStep::NoNext,
                WalkStep::Row(false, "你号".to_owned(), "ni'hao".to_owned(), 138),
                WalkStep::NoNext,
                WalkStep::NoNext,
            ]
        );
    }

    // 074a2219 pinyin.cpp:839–872 crosses both items’ readings.
    #[cfg(unix)]
    #[test]
    fn the_bigram_export_crosses_both_items_readings() {
        let user = TempUserDir::new("export-cross-product");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        assert_eq!(
            import_native(
                &mut context,
                7,
                &[("龘龘", "da2'da2", 5), ("龘龘", "da4'da4", 5)]
            ),
            [true, true]
        );
        let dalong = context
            .user_store()
            .token_for_phrase("龘龘")
            .unwrap()
            .unwrap();
        assert_eq!(context.save_user(), SaveOutcome::Saved);
        context.finish();
        drop(context);
        lay_gram(&user.path, dalong, &[(dalong, 100)]);
        let context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
        let row =
            |more, pinyin: &str| WalkStep::Row(more, "龘龘龘龘".to_owned(), pinyin.to_owned(), 200);
        assert_eq!(
            walk_steps(&context),
            [
                WalkStep::NoNext,
                row(true, "da2'da2'da2'da2"),
                row(true, "da2'da2'da4'da4"),
                row(true, "da4'da4'da2'da2"),
                row(false, "da4'da4'da4'da4"),
                WalkStep::NoNext,
                WalkStep::NoNext
            ]
        );
    }

    // 074a2219 pinyin.cpp:839–872 reads pronunciations when the pair loads.
    #[test]
    fn bigram_export_reads_a_new_pronunciation_when_the_next_pair_loads() {
        for live_successor in [false, true] {
            let user = TempUserDir::new("export-live-item");
            let user_token = saved_user_phrase(&user.path);
            let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
            let system_token = tokens(&context, "好")[0];
            context.finish();
            drop(context);
            let (predecessor, successor) = if live_successor {
                (system_token, user_token)
            } else {
                (user_token, system_token)
            };
            lay_gram(&user.path, predecessor, &[(successor, 69), (successor, 70)]);
            let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Pinyin);
            let mut walk = context.bigram_export_walk().unwrap();
            assert!(!walk.has_next(&context).unwrap());
            assert!(walk.has_next(&context).unwrap());
            assert_eq!(
                import_native(&mut context, 7, &[("龘龘", "da4'da4", 5)]),
                [true]
            );
            let expected = |more, reading: &str, count| {
                let (text, pinyin) = if live_successor {
                    ("好龘龘", format!("hao'{reading}"))
                } else {
                    ("龘龘好", format!("{reading}'hao"))
                };
                WalkStep::Row(more, text.to_owned(), pinyin, count)
            };
            assert_eq!(row(&context, &mut walk), expected(true, "da2'da2", 138));
            assert_eq!(row(&context, &mut walk), expected(true, "da2'da2", 140));
            assert_eq!(row(&context, &mut walk), expected(false, "da4'da4", 140));
        }
    }

    // Rust contract: bigram export reads the counts written by ordinary training.
    #[cfg(feature = "tkrzw")]
    #[test]
    fn ordinary_training_exports_its_grams() {
        let user = TempUserDir::new("pinned-nbest-train");
        let mut context = native_context(
            &contract_data::contract_data_dir(),
            &user.path,
            UserConfLaw::Pinyin,
        );
        let mut instance = context.alloc_instance();
        for second in ["浩", "好"] {
            for _ in 0..3 {
                assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
                assert!(guess(&context, &mut instance));
                let first = published(&context, &mut instance, 0, "你");
                assert_eq!(first.end, 2);
                assert_ne!(
                    choose(&context, &mut instance, &first),
                    crate::engine::Selection::Refused
                );
                assert!(guess(&context, &mut instance));
                let second = published(&context, &mut instance, 2, second);
                assert_eq!(second.end, 5);
                assert_ne!(
                    choose(&context, &mut instance, &second),
                    crate::engine::Selection::Refused
                );
                guess(&context, &mut instance);
                assert!(context.train(&instance, 0).unwrap());
                instance.bind(&context.runtime).full_reset();
            }
        }
        assert_eq!(export_native(&context, 7), []);
        let rows: Vec<_> = walk_steps(&context)
            .into_iter()
            .filter_map(|step| match step {
                WalkStep::Row(_, text, pinyin, count) => Some((text, pinyin, count)),
                WalkStep::NoNext => None,
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("你浩".to_owned(), "ni'hao".to_owned(), 1242),
                ("你好".to_owned(), "ni'hao".to_owned(), 1242)
            ]
        );
    }
}
