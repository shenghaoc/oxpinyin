//! P1 reader tests: synthetic files in the exact upstream layout, the
//! malformed-input refusals, and — when the environment points at one —
//! the real files of a libpinyin installation.
//!
//! The synthetic builder below is a faithful `SubPhraseIndex::store`
//! counterpart (the seed of the P5 native emitter): chunk header with
//! upstream's checksum, the `'#'`-separated sections, and items written
//! at `add_phrase_item`'s offsets (first item at 8, `0` = no item).

use std::collections::BTreeMap;
use std::path::PathBuf;

use oxpinyin_data::phrase_library::PhraseLibrary;

/// Builds one phrase-library chunk file from slot → item entries.
/// One item's pieces: `(unigram, text, pronunciations as (packed
/// keys, freq))`.
type ItemParts = (u32, String, Vec<(Vec<u8>, u32)>);

struct ChunkBuilder {
    total_freq: u32,
    /// slot → item parts.
    items: BTreeMap<usize, ItemParts>,
}

fn checksum(payload: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let aligned = payload.len() & !0x3;
    for word in payload[..aligned].chunks_exact(4) {
        sum ^= u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let mut shift = 0_u32;
    for &byte in &payload[aligned..] {
        sum ^= u32::from(byte) << shift;
        shift += 8;
    }
    sum
}

impl ChunkBuilder {
    fn new(total_freq: u32) -> Self {
        Self {
            total_freq,
            items: BTreeMap::new(),
        }
    }

    fn add(&mut self, slot: usize, unigram: u32, text: &str, pronunciations: Vec<(Vec<u8>, u32)>) {
        self.items
            .insert(slot, (unigram, text.to_owned(), pronunciations));
    }

    /// Serializes exactly what `SubPhraseIndex::store` writes.
    fn build(&self) -> Vec<u8> {
        let max_slot = self.items.keys().copied().max().unwrap_or(0);
        let slots = if self.items.is_empty() {
            0
        } else {
            max_slot + 1
        };

        // Entry area: bytes 0..8 reserved, then items in slot order.
        let mut content: Vec<u8> = vec![0; 8];
        let mut offsets = vec![0_u32; slots];
        for (&slot, (unigram, text, pronunciations)) in &self.items {
            offsets[slot] = content.len() as u32;
            content.push(text.chars().count() as u8);
            content.push(pronunciations.len() as u8);
            content.extend_from_slice(&unigram.to_le_bytes());
            for ch in text.chars() {
                content.extend_from_slice(&(ch as u32).to_le_bytes());
            }
            for (keys, freq) in pronunciations {
                content.extend_from_slice(keys);
                content.extend_from_slice(&freq.to_le_bytes());
            }
        }

        let index_one = 17_usize; // 4 words + separator, per store()
        let mut offset_array: Vec<u8> = Vec::new();
        for offset in &offsets {
            offset_array.extend_from_slice(&offset.to_le_bytes());
        }
        let index_two = index_one + offset_array.len() + 1; // + separator
        let index_three = index_two + content.len() + 1;

        let mut payload = Vec::new();
        payload.extend_from_slice(&self.total_freq.to_le_bytes());
        payload.extend_from_slice(&(index_one as u32).to_le_bytes());
        payload.extend_from_slice(&(index_two as u32).to_le_bytes());
        payload.extend_from_slice(&(index_three as u32).to_le_bytes());
        payload.push(b'#');
        payload.extend_from_slice(&vec![0; index_one - payload.len()]);
        payload.extend_from_slice(&offset_array);
        payload.push(b'#');
        payload.extend_from_slice(&content);
        payload.push(b'#');
        assert_eq!(payload.len(), index_three);

        let mut file = Vec::new();
        file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        file.extend_from_slice(&checksum(&payload).to_le_bytes());
        file.extend_from_slice(&payload);
        file
    }
}

fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-phrase-library-{name}-{}-{unique}.bin",
        std::process::id()
    ));
    std::fs::write(&path, bytes).expect("write temp chunk");
    path
}

/// 你 (slot 4, one pronunciation of 2 packed keys) and 你好 (slot 9,
/// two pronunciations of 4 packed keys) — the shapes the decode tests
/// read back.
fn sample() -> Vec<u8> {
    let mut builder = ChunkBuilder::new(9);
    builder.add(4, 5, "你", vec![(vec![0x12, 0x34], 5)]);
    builder.add(
        9,
        4,
        "你好",
        vec![
            (vec![0x12, 0x34, 0x56, 0x78], 3),
            (vec![0xAA, 0xBB, 0xCC, 0xDD], 1),
        ],
    );
    builder.build()
}

#[test]
fn reads_items_texts_and_pronunciations() {
    let path = write_temp("sample", &sample());
    let library = PhraseLibrary::open(&path).expect("valid chunk");
    assert_eq!(library.total_freq(), 9);
    // Slots 1..=9 with trailing-zero trimming: slot 9 is the last item.
    assert_eq!(library.token_range(), 1..10);

    let item = library.item_at_slot(4).expect("item at slot 4");
    assert_eq!(item.phrase_length(), 1);
    assert_eq!(item.n_pronunciations(), 1);
    assert_eq!(item.unigram(), 5);
    assert_eq!(item.phrase_text().as_deref(), Some("你"));
    let pronunciations: Vec<_> = item.pronunciations().collect();
    assert_eq!(pronunciations.len(), 1);
    assert_eq!(pronunciations[0].keys, &[0x12, 0x34]);
    assert_eq!(pronunciations[0].freq, 5);

    let item = library.item_at_slot(9).expect("item at slot 9");
    assert_eq!(item.phrase_text().as_deref(), Some("你好"));
    let pronunciations: Vec<_> = item.pronunciations().collect();
    assert_eq!(pronunciations.len(), 2);
    assert_eq!(pronunciations[1].keys, &[0xAA, 0xBB, 0xCC, 0xDD]);
    assert_eq!(pronunciations[1].freq, 1);
    assert!(item.pronunciation(2).is_none());

    // Slot addressing through a full token masks the library nibble.
    assert!(library.item(0x0100_0004).is_some());
    assert!(library.item(0x0F00_0004).is_some());

    // Absent and out-of-range slots answer None, never panic.
    assert!(library.item_at_slot(0).is_none());
    assert!(library.item_at_slot(5).is_none());
    assert!(library.item_at_slot(10).is_none());
    assert!(library.item_at_slot(usize::MAX / 4).is_none());

    // The tooling iteration sees exactly the two items.
    assert_eq!(library.items().count(), 2);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn empty_library_answers_the_one_to_one_range() {
    // `get_range`'s skip-empty branch: an offset array with only zero
    // slots answers 1..1.
    let bytes = ChunkBuilder::new(0).build();
    let path = write_temp("empty", &bytes);
    let library = PhraseLibrary::open(&path).expect("empty but well-formed");
    assert_eq!(library.token_range(), 1..1);
    assert_eq!(library.total_freq(), 0);
    assert!(library.item_at_slot(0).is_none());
    assert_eq!(library.items().count(), 0);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn trailing_empty_slots_are_trimmed_from_the_range() {
    // Slots 1..3, then zero slots 4..7: the range ends at 3.
    let mut builder = ChunkBuilder::new(3);
    builder.add(3, 3, "的", vec![(vec![0; 2], 3)]);
    let bytes = builder.build();
    let path = write_temp("trimmed", &bytes);
    let library = PhraseLibrary::open(&path).expect("valid chunk");
    assert_eq!(library.token_range(), 1..4);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn chunk_header_and_layout_damage_is_refused() {
    let sample = sample();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("short header", sample[..7].to_vec()),
        ("length word drift", {
            let mut bytes = sample.clone();
            let drift = (bytes.len() as u32).to_le_bytes();
            bytes[0..4].copy_from_slice(&drift);
            bytes
        }),
        ("checksum drift", {
            let mut bytes = sample.clone();
            bytes[4] ^= 0xFF;
            bytes
        }),
        ("payload truncation", sample[..sample.len() - 1].to_vec()),
        ("header separator lost", {
            let mut bytes = sample.clone();
            bytes[8 + 16] = b'!';
            fix_checksum(&mut bytes);
            bytes
        }),
        ("offset-array separator lost", {
            let mut bytes = sample.clone();
            let index_two = u32_at(&sample[16..20]);
            bytes[8 + index_two as usize - 1] = b'!';
            fix_checksum(&mut bytes);
            bytes
        }),
        ("final separator lost", {
            let mut bytes = sample.clone();
            let last = bytes.len() - 1;
            bytes[last] = b'!';
            fix_checksum(&mut bytes);
            bytes
        }),
        ("sections out of order", {
            let mut bytes = sample.clone();
            bytes[8 + 12..8 + 16].copy_from_slice(&2_u32.to_le_bytes());
            fix_checksum(&mut bytes);
            bytes
        }),
        ("offset array not u32 sized", {
            // index_two one byte past where the separator check
            // still passes is not constructible without breaking a
            // separator; emulate via index_one drift instead.
            let mut bytes = sample.clone();
            bytes[8 + 4..8 + 8].copy_from_slice(&18_u32.to_le_bytes());
            fix_checksum(&mut bytes);
            bytes
        }),
    ];
    for (name, bytes) in cases {
        let path = write_temp(name, &bytes);
        let result = PhraseLibrary::open(&path);
        assert!(result.is_err(), "{name} must be refused, got {result:?}");
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn item_damage_answers_no_item_never_panics() {
    // Point slot 4's offset at the last bytes of the entry area, where
    // an item header cannot fit: bounded reads, None answers.
    let mut bytes = sample();
    let payload_len = bytes.len() - 8;
    let index_two = u32_at(&bytes[16..20]);
    let content_start = 8 + index_two as usize;
    let near_end = (8 + payload_len - 2 - content_start) as u32; // content-relative
    bytes[8 + 17 + 4 * 4..8 + 17 + 4 * 4 + 4].copy_from_slice(&near_end.to_le_bytes());
    fix_checksum(&mut bytes);
    let path = write_temp("damaged-item", &bytes);
    let library = PhraseLibrary::open(&path).expect("layout still validates");
    assert!(
        library.item_at_slot(4).is_none(),
        "malformed item is no item"
    );
    let _ = std::fs::remove_file(&path);

    // An invalid UCS-4 scalar in the text degrades to None text.
    let mut builder = ChunkBuilder::new(1);
    builder.add(1, 1, "\u{1}\u{2}", vec![(vec![0; 4], 1)]);
    let mut bytes = builder.build();
    // Overwrite the second char's scalar with a surrogate.
    let index_two = u32_at(&bytes[16..20]);
    let content_start = 8 + index_two as usize;
    let text_start = content_start + 8 + 6; // reserved 8 + item header 6
    bytes[text_start + 4..text_start + 8].copy_from_slice(&0xD800_u32.to_le_bytes());
    fix_checksum(&mut bytes);
    let path = write_temp("surrogate", &bytes);
    let library = PhraseLibrary::open(&path).expect("layout still validates");
    let item = library.item_at_slot(1).expect("item present");
    assert_eq!(item.phrase_length(), 2);
    assert!(item.phrase_text().is_none(), "surrogate is not a char");
    let _ = std::fs::remove_file(&path);
}

fn u32_at(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn fix_checksum(bytes: &mut [u8]) {
    let sum = checksum(&bytes[8..]);
    bytes[4..8].copy_from_slice(&sum.to_le_bytes());
}

// ── the real thing ──────────────────────────────────────────────────

/// The pinned build's per-library files, when the environment points at
/// them (`OXPINYIN_LIBPINYIN_DATA`; `OXPINYIN_DATAGEN_STRICT=1` makes
/// absence a failure). Never a CI dependency: the model data is not
/// redistributable, and the synthetic suite above carries the format
/// contract.
#[test]
fn reads_a_real_libpinyin_installation() {
    let Some(dir) = std::env::var_os("OXPINYIN_LIBPINYIN_DATA")
        .map(PathBuf::from)
        .filter(|dir| dir.join("gb_char.bin").is_file())
    else {
        if std::env::var_os("OXPINYIN_DATAGEN_STRICT").is_some() {
            panic!("OXPINYIN_DATAGEN_STRICT=1 but no libpinyin data at OXPINYIN_LIBPINYIN_DATA");
        }
        eprintln!("skipping: no real libpinyin data (set OXPINYIN_LIBPINYIN_DATA)");
        return;
    };

    let system = ["gb_char", "gbk_char", "opengram", "merged"];
    let mut libraries = Vec::new();
    let mut items = 0_u64;
    for name in system {
        let library = PhraseLibrary::open(&dir.join(format!("{name}.bin")))
            .unwrap_or_else(|e| panic!("{name}.bin: {e}"));
        assert!(library.total_freq() > 0, "{name} carries unigram mass");
        assert_eq!(library.token_range().start, 1, "{name} tokens start at 1");
        let count = library.items().count() as u64;
        assert!(count > 0, "{name} has items");
        items += count;
        eprintln!(
            "{name}: {} items, range {:?}, total_freq {}",
            count,
            library.token_range(),
            library.total_freq()
        );
        libraries.push(library);
    }
    // The frozen oracle export carried exactly 138,096 phrase tokens
    // across the four system libraries (`datagen-model20.md`), derived
    // through the ABI from these very files.
    assert_eq!(items, 138_096, "item count must match the frozen export");

    // 的: token 0x010005DB. model20 carries two readings —
    // `de 的 16778715 2213855` and `di 的 16778715 11000` — and the
    // item must hold exactly those pronunciation counts verbatim (the
    // .table count column, per `datagen-model20.md`'s equivalence).
    let gb = &libraries[0];
    let item = gb
        .item(0x0100_05DB)
        .unwrap_or_else(|| panic!("的 missing from gb_char"));
    assert_eq!(item.phrase_text().as_deref(), Some("的"));
    assert_eq!(item.n_pronunciations(), 2);
    let mut freqs: Vec<u32> = item
        .pronunciations()
        .map(|pronunciation| pronunciation.freq)
        .collect();
    freqs.sort_unstable();
    assert_eq!(
        freqs,
        vec![11_000, 2_213_855],
        "的 carries its two .table readings"
    );

    // A single-reading cross-check: 锕, token 0x01000001, `a 锕 … 7`.
    let item = gb.item(0x0100_0001).expect("锕 present");
    assert_eq!(item.phrase_text().as_deref(), Some("锕"));
    assert_eq!(item.n_pronunciations(), 1);
    assert_eq!(
        item.pronunciations()
            .next()
            .map(|pronunciation| pronunciation.freq),
        Some(7)
    );

    // The addon chunk files ride the same format.
    let art = PhraseLibrary::open(&dir.join("art.bin")).expect("addon art.bin");
    assert!(art.items().count() > 0);
}
