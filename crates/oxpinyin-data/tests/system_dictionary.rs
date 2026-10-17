//! Integration tests for `SystemDictionary` — the production dictionary
//! over libpinyin's own files.
//!
//! Builds a data directory the way the P5 producer does — bare-keyspace
//! rows for the pinyin and phrase DBMs through the compiled-in backend,
//! a `gb_char.bin` chunk file through the shared `ChunkBuilder` — and
//! exercises the `Dictionary` trait plus the runtime's extra surface.

use oxpinyin_core::{
    ChewingKey, Completeness, Dictionary, PHRASE_INDEX_LIBRARY_COUNT, PhraseIndexRanges,
    SyllableKey,
};
// The fixtures are written with the crate's own row schema, not a copy of
// it: a layout change has to break the reader's behaviour here, never pass
// because the test harness drifted along with it.
use oxpinyin_data::row_format::phrase_index::{encode_tokens, encode_ucs4_key};
use oxpinyin_data::row_format::pinyin_index::{
    encode_complete_key as encode_complete, encode_incomplete_key as encode_incomplete, encode_item,
};
use oxpinyin_data::{SystemDbm, SystemDictionary};
use oxpinyin_store::DefaultStore;

mod support;
use support::{ChunkBuilder, write_raw_rows};
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = scratch_dir("oxpinyin-system-dict-test-");
        write_pinyin_index(&dir.path().join(SystemDbm::PinyinIndex.file_name()));
        write_phrase_index(&dir.path().join(SystemDbm::PhraseIndex.file_name()));
        write_gb_char_chunk(&dir.path().join("gb_char.bin"));
        Self { dir }
    }

    fn dict(&self) -> SystemDictionary {
        SystemDictionary::open(self.dir.path()).unwrap()
    }
}

/// A fresh, uniquely named data dir, removed on drop (including unwind).
fn scratch_dir(prefix: &str) -> TempDir {
    tempfile::Builder::new().prefix(prefix).tempdir().unwrap()
}

// ── Encoding helpers ─────────────────────────────────────────────

fn key(spelling: &str) -> ChewingKey {
    ChewingKey::from_pinyin(spelling).unwrap()
}

fn encode_items(entries: &[(u32, &[ChewingKey])]) -> Vec<u8> {
    let mut buf = Vec::new();
    for (token, keys) in entries {
        buf.extend_from_slice(&encode_item(*token, keys));
    }
    buf
}

// ── Fixture writers ──────────────────────────────────────────────

/// The pinyin index the P5 producer writes: every row in both keyspaces
/// with its full keys, every proper prefix an empty marker.
fn write_pinyin_index(path: &std::path::Path) {
    let ni3 = key("ni").with_tone(3);
    let ni2 = key("ni").with_tone(2);
    let hao3 = key("hao").with_tone(3);
    let zhong1 = key("zhong").with_tone(1);
    let guo2 = key("guo").with_tone(2);
    let men = key("men");
    let ni = key("ni");
    let n = ChewingKey::new(ni.initial, 0, 0, 0);
    let h = ChewingKey::new(hao3.initial, 0, 0, 0);
    let m = ChewingKey::new(men.initial, 0, 0, 0);
    let z = ChewingKey::new(zhong1.initial, 0, 0, 0);
    let g = ChewingKey::new(guo2.initial, 0, 0, 0);

    let rows: Vec<(Vec<u8>, Vec<u8>)> = vec![
        // Complete keyspace.
        (
            encode_complete(&[ni]),
            encode_items(&[(0x0100_0010, &[ni3][..]), (0x0200_0010, &[ni2][..])]),
        ),
        (
            encode_complete(&[key("hao")]),
            encode_items(&[(0x0100_0011, &[hao3][..])]),
        ),
        (
            encode_complete(&[ni, key("hao")]),
            encode_items(&[(0x0100_0099, &[ni3, hao3][..])]),
        ),
        (
            encode_complete(&[ni, men]),
            encode_items(&[(0x0200_0098, &[ni3, men][..])]),
        ),
        (
            encode_complete(&[key("zhong")]),
            encode_items(&[(0x0100_0020, &[zhong1][..])]),
        ),
        (
            encode_complete(&[key("guo")]),
            encode_items(&[(0x0100_0021, &[guo2][..])]),
        ),
        (
            encode_complete(&[key("zhong"), key("guo")]),
            encode_items(&[(0x0100_00A0, &[zhong1, guo2][..])]),
        ),
        // Incomplete keyspace: the same records under initial-only keys.
        (
            encode_incomplete(&[n]),
            encode_items(&[(0x0100_0010, &[ni3][..]), (0x0200_0010, &[ni2][..])]),
        ),
        (
            encode_incomplete(&[h]),
            encode_items(&[(0x0100_0011, &[hao3][..])]),
        ),
        (
            encode_incomplete(&[n, h]),
            encode_items(&[(0x0100_0099, &[ni3, hao3][..])]),
        ),
        (
            encode_incomplete(&[n, m]),
            encode_items(&[(0x0200_0098, &[ni3, men][..])]),
        ),
        (
            encode_incomplete(&[z]),
            encode_items(&[(0x0100_0020, &[zhong1][..])]),
        ),
        (
            encode_incomplete(&[g]),
            encode_items(&[(0x0100_0021, &[guo2][..])]),
        ),
        (
            encode_incomplete(&[z, g]),
            encode_items(&[(0x0100_00A0, &[zhong1, guo2][..])]),
        ),
    ];
    write_raw_rows::<DefaultStore>(path, &rows);
}

fn write_phrase_index(path: &std::path::Path) {
    let rows: Vec<(Vec<u8>, Vec<u8>)> = vec![
        (
            encode_ucs4_key("你"),
            encode_tokens(&[0x0100_0010, 0x0200_0010]),
        ),
        (encode_ucs4_key("好"), encode_tokens(&[0x0100_0011])),
        (encode_ucs4_key("中"), encode_tokens(&[0x0100_0020])),
        (encode_ucs4_key("国"), encode_tokens(&[0x0100_0021])),
        (encode_ucs4_key("你好"), encode_tokens(&[0x0100_0099])),
        (encode_ucs4_key("你们"), encode_tokens(&[0x0200_0098])),
        (encode_ucs4_key("中国"), encode_tokens(&[0x0100_00A0])),
        // A phrase of two tokens from two libraries.
        (
            encode_ucs4_key("的"),
            encode_tokens(&[0x0100_05DB, 0x0200_05DB]),
        ),
    ];
    write_raw_rows::<DefaultStore>(path, &rows);
}

/// The `gb_char` chunk file: the nibble-1 items with their text and
/// pronunciations. 你 carries two toned readings (ni3 ×5, ni2 ×3).
fn write_gb_char_chunk(path: &std::path::Path) {
    let ni3 = key("ni").with_tone(3);
    let ni2 = key("ni").with_tone(2);
    let hao3 = key("hao").with_tone(3);
    let zhong1 = key("zhong").with_tone(1);
    let guo2 = key("guo").with_tone(2);

    let pron1 = |key: ChewingKey, freq: u32| (key.to_packed().to_le_bytes().to_vec(), freq);
    let pron2 = |a: ChewingKey, b: ChewingKey| {
        let mut keys = a.to_packed().to_le_bytes().to_vec();
        keys.extend_from_slice(&b.to_packed().to_le_bytes());
        vec![(keys, 5_u32)]
    };

    let mut builder = ChunkBuilder::new(30);
    builder.add(0x10, 5, "你", vec![pron1(ni3, 5), pron1(ni2, 3)]);
    builder.add(0x11, 5, "好", vec![pron1(hao3, 5)]);
    builder.add(0x20, 5, "中", vec![pron1(zhong1, 5)]);
    builder.add(0x21, 5, "国", vec![pron1(guo2, 5)]);
    builder.add(0x99, 5, "你好", pron2(ni3, hao3));
    builder.add(0xA0, 5, "中国", pron2(zhong1, guo2));
    std::fs::write(path, builder.build()).unwrap();
}

// ── Helpers ──────────────────────────────────────────────────────

fn syl(text: &str) -> SyllableKey {
    SyllableKey::from_text(text).expect("frozen syllable")
}

fn syls(text: &str) -> Vec<SyllableKey> {
    text.split(',').map(syl).collect()
}

// ── Tests ────────────────────────────────────────────────────────

#[test]
fn open_costs_handles_only() {
    let fix = Fixture::new();
    let dict = fix.dict();
    // gbk_char.bin is absent: nibble 2 is simply unloaded.
    assert_eq!(dict.item_count(), 6);
    assert_eq!(dict.unigram_total(), 30);
    assert!(dict.libraries().is_loaded(1));
    assert!(!dict.libraries().is_loaded(2));
}

#[test]
fn single_syllable_lookup_resolves_only_loaded_libraries() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let entries = dict.lookup(&syls("ni")).unwrap();
    // 0x02000010 sits under nibble 2, whose chunk file is absent: dropped,
    // upstream's NULL-array skip.
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].text(), "你");
    assert_eq!(entries[0].token().value(), 0x0100_0010);
}

#[test]
fn lookup_carries_the_pronunciation_possibility() {
    let fix = Fixture::new();
    let dict = fix.dict();
    // A tone-less query matches both toned readings: (5 + 3) / 8.
    let entries = dict.lookup(&syls("ni")).unwrap();
    assert_eq!(entries[0].pronunciation_possibility(), Some((8, 8)));
    let entries = dict.lookup(&syls("ni,hao")).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text(), "你好");
    assert_eq!(entries[0].pronunciation_possibility(), Some((5, 5)));
}

#[test]
fn incomplete_syllables_search_the_initial_keyspace() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let n = SyllableKey::from_text("n").expect("initial-only key");
    assert_eq!(n.completeness(), Completeness::Partial);
    // "n" alone: 你 (nibble 1 only).
    let entries = dict.lookup(&[n]).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text(), "你");
    // "ni" + "h": 你好 through the incomplete keyspace, filtered by the
    // complete first syllable.
    let h = SyllableKey::from_text("h").expect("initial-only key");
    let entries = dict.lookup(&[syl("ni"), h]).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text(), "你好");
    // "n" + "m": 你们 is nibble 2 → unloaded → nothing, but the key exists.
    let m = SyllableKey::from_text("m").expect("initial-only key");
    assert!(dict.lookup(&[n, m]).unwrap().is_empty());
    assert!(dict.phrase_prefix_exists(&[n, m]).unwrap());
    assert!(dict.phrase_prefix_exists(&[n]).unwrap());
}

#[test]
fn empty_and_unknown_lookups_answer_empty() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert!(dict.lookup(&[]).unwrap().is_empty());
    assert!(dict.lookup(&syls("xian")).unwrap().is_empty());
}

#[test]
fn phrase_prefix_exists_follows_search_continued() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert!(dict.phrase_prefix_exists(&syls("ni")).unwrap());
    assert!(dict.phrase_prefix_exists(&syls("ni,hao")).unwrap());
    assert!(!dict.phrase_prefix_exists(&syls("guo,guo")).unwrap());
    assert!(dict.phrase_prefix_exists(&[]).unwrap());
}

#[test]
fn visible_prefix_probe_hides_unloaded_libraries() {
    let fix = Fixture::new();
    let dict = fix.dict();
    // 你们 (nibble 2) is the only extension of "ni,men"; its library is
    // absent, so no visible phrase extends the prefix even though the
    // key exists.
    assert!(dict.phrase_prefix_exists(&syls("ni,men")).unwrap());
    assert!(
        !dict
            .phrase_prefix_exists_visible(&syls("ni,men"), |_| true)
            .unwrap()
    );
    // "ni" extends to 你好 (nibble 1): visible unless nibble 1 is masked.
    assert!(
        dict.phrase_prefix_exists_visible(&syls("ni"), |_| true)
            .unwrap()
    );
    assert!(
        !dict
            .phrase_prefix_exists_visible(&syls("ni"), |token| token >> 24 != 1)
            .unwrap()
    );
}

#[test]
fn lookup_into_matches_lookup() {
    let fix = Fixture::new();
    let dict = fix.dict();
    let ni = syls("ni");
    let lookup_result = dict.lookup(&ni).unwrap();
    let mut into_result = Vec::new();
    dict.lookup_into(&ni, &mut into_result).unwrap();
    assert_eq!(lookup_result, into_result);
}

#[test]
fn token_surface_resolves_through_the_chunk_file() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert_eq!(dict.phrase_text(0x0100_0099).as_deref(), Some("你好"));
    assert_eq!(dict.phrase_text(0x0200_0010), None, "unloaded library");
    assert_eq!(dict.phrase_text(0xFFFF_FFFF), None);
    assert_eq!(dict.unigram_count(0x0100_0010), Some(5));
    let prons = dict.pronunciations(0x0100_0010);
    assert_eq!(prons.len(), 2);
    assert_eq!(prons[0], ("ni".to_owned(), 5));
    assert_eq!(prons[1], ("ni".to_owned(), 3));
    assert_eq!(dict.phrase_index_item_count().unwrap(), 6);
}

#[test]
fn tokens_for_text_reads_the_phrase_dbm() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert_eq!(dict.tokens_for_text("你好").unwrap(), vec![0x0100_0099]);
    let tokens = dict.tokens_for_text("的").unwrap();
    assert_eq!(tokens, vec![0x0100_05DB, 0x0200_05DB]);
    assert!(dict.tokens_for_text("不存在").unwrap().is_empty());
    assert!(dict.tokens_for_text("").unwrap().is_empty());
    let via_trait = Dictionary::tokens_for_text(&dict, "你");
    assert_eq!(via_trait.len(), 2);
}

#[test]
fn suggest_after_walks_the_longer_phrases() {
    let fix = Fixture::new();
    let dict = fix.dict();
    // 你 → 你好 (loaded) and 你们 (unloaded, dropped); defined order.
    let rows = dict.suggest_after("你").unwrap();
    assert_eq!(rows, vec![(0x0100_0099, "你好".to_owned())]);
    assert!(dict.suggest_after("你好").unwrap().is_empty());
    assert!(dict.suggest_after("无").unwrap().is_empty());
    assert!(dict.suggest_after("").unwrap().is_empty());
}

#[test]
fn a_missing_dbm_fails_open() {
    let fix = Fixture::new();
    std::fs::remove_file(fix.dir.path().join(SystemDbm::PhraseIndex.file_name())).unwrap();
    assert!(SystemDictionary::open(fix.dir.path()).is_err());
}

/// A data dir whose only pinyin row is `ni`, holding (in stored order)
/// 0x0100_0010 ni3, 0x0200_0001 ni3 (library 2: no chunk, unloaded),
/// 0x0100_0011 ni3, 0x0100_0014 ni2, 0x0100_0012 ni3 and 0x0100_0013 ni3
/// (library 1, but no chunk item: unreadable).
fn convert_fixture() -> Fixture {
    let dir = scratch_dir("oxpinyin-system-dict-convert-");
    let ni = key("ni");
    let ni3 = ni.with_tone(3);
    let ni2 = ni.with_tone(2);
    let items = encode_items(&[
        (0x0100_0010, &[ni3][..]),
        (0x0200_0001, &[ni3][..]),
        (0x0100_0011, &[ni3][..]),
        (0x0100_0014, &[ni2][..]),
        (0x0100_0012, &[ni3][..]),
        (0x0100_0013, &[ni3][..]),
    ]);
    let n_key = ChewingKey::new(ni.initial, 0, 0, 0);
    write_raw_rows::<DefaultStore>(
        &dir.path().join(SystemDbm::PinyinIndex.file_name()),
        &[
            (encode_complete(&[ni]), items.clone()),
            // An empty marker: the key exists for longer phrases only.
            (encode_complete(&[ni, ni]), Vec::new()),
            (encode_incomplete(&[n_key]), items),
        ],
    );
    write_raw_rows::<DefaultStore>(&dir.path().join(SystemDbm::PhraseIndex.file_name()), &[]);
    let pron = |key: ChewingKey| vec![(key.to_packed().to_le_bytes().to_vec(), 5_u32)];
    let mut builder = ChunkBuilder::new(20);
    builder.add(0x10, 5, "你", pron(ni3));
    builder.add(0x11, 5, "拟", pron(ni3));
    builder.add(0x12, 5, "逆", pron(ni3));
    builder.add(0x14, 5, "泥", pron(ni2));
    std::fs::write(dir.path().join("gb_char.bin"), builder.build()).unwrap();
    Fixture { dir }
}

type Spans = Vec<Vec<(u32, u32)>>;

fn convert_ranges(dict: &SystemDictionary, tones: &[u8]) -> (Spans, Vec<String>) {
    let mut ranges = PhraseIndexRanges::new();
    dict.trellis_ranges(&syls("ni"), tones, &mut ranges)
        .unwrap();
    let spans = (0..PHRASE_INDEX_LIBRARY_COUNT)
        .map(|library| {
            ranges
                .ranges(library)
                .iter()
                .map(|range| (range.first(), range.last()))
                .collect()
        })
        .collect();
    let texts = dict
        .trellis_records(&syls("ni"), tones)
        .unwrap()
        .iter()
        .map(|entry| entry.text().to_owned())
        .collect();
    (spans, texts)
}

/// `convert` over the stored items with a toned query: the ni2 item is
/// filtered by the search, so 0x10..=0x13 form one run — the unloaded
/// library-2 item between 0x10 and 0x11 is skipped without closing it,
/// and the unreadable 0x13 keeps its place with no entry.
#[test]
fn trellis_ranges_merge_runs_past_unloaded_items_under_the_tone_filter() {
    let fix = convert_fixture();
    let dict = fix.dict();
    let (spans, texts) = convert_ranges(&dict, &[3]);
    assert_eq!(spans[1], [(0x0100_0010, 0x0100_0013)]);
    assert!(spans[2].is_empty(), "library 2 is unloaded: {spans:?}");
    assert_eq!(texts, ["你", "拟", "逆"]);
    let records: Vec<u32> = dict
        .trellis_records(&syls("ni"), &[3])
        .unwrap()
        .iter()
        .map(|entry| entry.token().value())
        .collect();
    assert_eq!(records, [0x0100_0010, 0x0100_0011, 0x0100_0012]);
}

/// A tone-less query keeps the ni2 item, which splits the run in stored
/// order; the walk is range order, not token order.
#[test]
fn trellis_ranges_split_runs_in_stored_order_without_a_tone() {
    let fix = convert_fixture();
    let dict = fix.dict();
    let (spans, texts) = convert_ranges(&dict, &[]);
    assert_eq!(
        spans[1],
        [
            (0x0100_0010, 0x0100_0011),
            (0x0100_0014, 0x0100_0014),
            (0x0100_0012, 0x0100_0013),
        ]
    );
    assert_eq!(texts, ["你", "拟", "泥", "逆"]);
}

/// The fused single-get answer is the pair it replaces: the ranges of
/// `trellis_ranges` and the probe of `phrase_prefix_exists`, over a row of
/// items, an empty marker (the key exists, so the probe is true), an
/// absent key, an incomplete spelling and each tone filter.
#[test]
fn trellis_ranges_continued_matches_the_ranges_and_prefix_pair() {
    let fix = convert_fixture();
    let dict = fix.dict();
    let mut seen = (0_u32, 0_u32, 0_u32);
    for path in ["ni", "ni,ni", "ni,hao", "n", "n,n", "hao"] {
        for tones in [&[][..], &[3], &[2], &[3, 3], &[4, 0]] {
            let path = syls(path);
            let mut pair = PhraseIndexRanges::new();
            dict.trellis_ranges(&path, tones, &mut pair).unwrap();
            let continued = dict.phrase_prefix_exists(&path).unwrap();
            let mut fused = PhraseIndexRanges::new();
            let fused_continued = dict
                .trellis_ranges_continued(&path, tones, &mut fused)
                .unwrap();
            assert_eq!(
                format!("{pair:?}"),
                format!("{fused:?}"),
                "{path:?} {tones:?}"
            );
            assert_eq!(continued, fused_continued, "{path:?} {tones:?}");
            seen = (
                seen.0 + u32::from(fused.iter().next().is_some()),
                seen.1 + u32::from(fused_continued && fused.iter().next().is_none()),
                seen.2 + u32::from(!fused_continued),
            );
        }
    }
    // Ranged rows, markers that range nothing, and absent keys all occur.
    assert!(seen.0 > 0 && seen.1 > 0 && seen.2 > 0, "{seen:?}");
}
