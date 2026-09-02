//! Integration tests for `SystemDictionary` — the production dictionary
//! over libpinyin's own files.
//!
//! Builds a data directory the way the P5 producer does — bare-keyspace
//! rows for the pinyin and phrase DBMs through the compiled-in backend,
//! a `gb_char.bin` chunk file through the shared `ChunkBuilder` — and
//! exercises the `Dictionary` trait plus the runtime's extra surface.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::{ChewingKey, Completeness, Dictionary, SyllableKey};
use oxpinyin_data::{SystemDbm, SystemDictionary};
use oxpinyin_store::DefaultStore;

mod support;
use support::{ChunkBuilder, write_raw_rows};

static FIXTURE_COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let n = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-system-dict-test-{}-{}",
            std::process::id(),
            n,
        ));
        std::fs::create_dir_all(&dir).unwrap();
        write_pinyin_index(&dir.join(SystemDbm::PinyinIndex.file_name()));
        write_phrase_index(&dir.join(SystemDbm::PhraseIndex.file_name()));
        write_gb_char_chunk(&dir.join("gb_char.bin"));
        Self { dir }
    }

    fn dict(&self) -> SystemDictionary {
        SystemDictionary::open(&self.dir).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ── Encoding helpers ─────────────────────────────────────────────

fn key(spelling: &str) -> ChewingKey {
    ChewingKey::from_pinyin(spelling).unwrap()
}

fn encode_complete(keys: &[ChewingKey]) -> Vec<u8> {
    keys.iter()
        .flat_map(|k| {
            ChewingKey::new(k.initial, k.middle, k.final_, 0)
                .to_packed()
                .to_le_bytes()
        })
        .collect()
}

fn encode_incomplete(keys: &[ChewingKey]) -> Vec<u8> {
    keys.iter()
        .flat_map(|k| {
            ChewingKey::new(k.initial, 0, 0, 0)
                .to_packed()
                .to_le_bytes()
        })
        .collect()
}

fn item2_stride(phrase_length: usize) -> usize {
    (4 + 2 * phrase_length + 3) & !3
}

fn encode_items(entries: &[(u32, &[ChewingKey])]) -> Vec<u8> {
    let mut buf = Vec::new();
    for (token, keys) in entries {
        let mut record = vec![0u8; item2_stride(keys.len())];
        record[..4].copy_from_slice(&token.to_le_bytes());
        for (j, k) in keys.iter().enumerate() {
            record[4 + j * 2..6 + j * 2].copy_from_slice(&k.to_packed().to_le_bytes());
        }
        buf.extend_from_slice(&record);
    }
    buf
}

fn encode_ucs4_key(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| (ch as u32).to_le_bytes())
        .collect()
}

fn encode_tokens(tokens: &[u32]) -> Vec<u8> {
    tokens.iter().flat_map(|t| t.to_le_bytes()).collect()
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
            encode_items(&[(0x01000010, &[ni3][..]), (0x02000010, &[ni2][..])]),
        ),
        (
            encode_complete(&[key("hao")]),
            encode_items(&[(0x01000011, &[hao3][..])]),
        ),
        (
            encode_complete(&[ni, key("hao")]),
            encode_items(&[(0x01000099, &[ni3, hao3][..])]),
        ),
        (
            encode_complete(&[ni, men]),
            encode_items(&[(0x02000098, &[ni3, men][..])]),
        ),
        (
            encode_complete(&[key("zhong")]),
            encode_items(&[(0x01000020, &[zhong1][..])]),
        ),
        (
            encode_complete(&[key("guo")]),
            encode_items(&[(0x01000021, &[guo2][..])]),
        ),
        (
            encode_complete(&[key("zhong"), key("guo")]),
            encode_items(&[(0x010000A0, &[zhong1, guo2][..])]),
        ),
        // Incomplete keyspace: the same records under initial-only keys.
        (
            encode_incomplete(&[n]),
            encode_items(&[(0x01000010, &[ni3][..]), (0x02000010, &[ni2][..])]),
        ),
        (
            encode_incomplete(&[h]),
            encode_items(&[(0x01000011, &[hao3][..])]),
        ),
        (
            encode_incomplete(&[n, h]),
            encode_items(&[(0x01000099, &[ni3, hao3][..])]),
        ),
        (
            encode_incomplete(&[n, m]),
            encode_items(&[(0x02000098, &[ni3, men][..])]),
        ),
        (
            encode_incomplete(&[z]),
            encode_items(&[(0x01000020, &[zhong1][..])]),
        ),
        (
            encode_incomplete(&[g]),
            encode_items(&[(0x01000021, &[guo2][..])]),
        ),
        (
            encode_incomplete(&[z, g]),
            encode_items(&[(0x010000A0, &[zhong1, guo2][..])]),
        ),
    ];
    write_raw_rows::<DefaultStore>(path, &rows);
}

fn write_phrase_index(path: &std::path::Path) {
    let rows: Vec<(Vec<u8>, Vec<u8>)> = vec![
        (
            encode_ucs4_key("你"),
            encode_tokens(&[0x01000010, 0x02000010]),
        ),
        (encode_ucs4_key("好"), encode_tokens(&[0x01000011])),
        (encode_ucs4_key("中"), encode_tokens(&[0x01000020])),
        (encode_ucs4_key("国"), encode_tokens(&[0x01000021])),
        (encode_ucs4_key("你好"), encode_tokens(&[0x01000099])),
        (encode_ucs4_key("你们"), encode_tokens(&[0x02000098])),
        (encode_ucs4_key("中国"), encode_tokens(&[0x010000A0])),
        // A phrase of two tokens from two libraries.
        (
            encode_ucs4_key("的"),
            encode_tokens(&[0x010005DB, 0x020005DB]),
        ),
    ];
    write_raw_rows::<DefaultStore>(path, &rows);
}

/// The gb_char chunk file: the nibble-1 items with their text and
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
    assert_eq!(entries[0].token().value(), 0x01000010);
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
    assert_eq!(dict.phrase_text(0x01000099).as_deref(), Some("你好"));
    assert_eq!(dict.phrase_text(0x02000010), None, "unloaded library");
    assert_eq!(dict.phrase_text(0xFFFFFFFF), None);
    assert_eq!(dict.unigram_count(0x01000010), Some(5));
    let prons = dict.pronunciations(0x01000010);
    assert_eq!(prons.len(), 2);
    assert_eq!(prons[0], ("ni".to_owned(), 5));
    assert_eq!(prons[1], ("ni".to_owned(), 3));
    assert_eq!(dict.phrase_index_item_count().unwrap(), 6);
}

#[test]
fn tokens_for_text_reads_the_phrase_dbm() {
    let fix = Fixture::new();
    let dict = fix.dict();
    assert_eq!(dict.tokens_for_text("你好").unwrap(), vec![0x01000099]);
    let tokens = dict.tokens_for_text("的").unwrap();
    assert_eq!(tokens, vec![0x010005DB, 0x020005DB]);
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
    assert_eq!(rows, vec![(0x01000099, "你好".to_owned())]);
    assert!(dict.suggest_after("你好").unwrap().is_empty());
    assert!(dict.suggest_after("无").unwrap().is_empty());
    assert!(dict.suggest_after("").unwrap().is_empty());
}

#[test]
fn a_missing_dbm_fails_open() {
    let fix = Fixture::new();
    std::fs::remove_file(fix.dir.join(SystemDbm::PhraseIndex.file_name())).unwrap();
    assert!(SystemDictionary::open(&fix.dir).is_err());
}
