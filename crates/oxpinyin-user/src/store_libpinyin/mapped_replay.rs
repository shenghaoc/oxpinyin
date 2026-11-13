//! Mapped journal replay fixtures and the overlapping-item round trip.

use std::path::PathBuf;

use oxpinyin_core::ChewingKey;
use oxpinyin_data::chunk_write::{ChunkItem, build_chunk, encode_phrase_item};
use oxpinyin_data::phrase_library::PhraseLibrary;
use oxpinyin_data::user_files::{LogRecord, SystemVersions, encode_log_records};

use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
use crate::store::UserStore;

pub(super) const LIBRARY: u32 = 0x0100_0000;

fn versions() -> SystemVersions {
    SystemVersions::for_this_build(7, 14)
}

fn tempdir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("oxpinyin-provenance-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmpdir");
    dir
}

fn item(code: u32, unigram: u32) -> ChunkItem {
    ChunkItem {
        phrase: vec![code],
        unigram,
        prons: vec![(
            vec![ChewingKey::from_pinyin("ni").expect("key").to_packed()],
            unigram,
        )],
    }
}

fn modify(slot: u32, old: &[u8], new: &[u8]) -> LogRecord {
    LogRecord::Modify {
        token: slot,
        old_item: old.to_vec(),
        new_item: new.to_vec(),
    }
}

fn shipped() -> [(u32, ChunkItem); 4] {
    [
        (1, item(0x4f60, 100)),
        (2, item(0x597d, 50)),
        (3, item(0x4e09, 25)),
        (5, item(0x4e94, 10)),
    ]
}

pub(super) struct Profile {
    dir: PathBuf,
    chunk: PathBuf,
}

impl Profile {
    /// A user directory whose `gb_char.dbin` holds `log`, beside a shipped
    /// chunk file.
    fn new(name: &str, log: &[u8]) -> Self {
        let dir = tempdir(name);
        let shipped_dir = tempdir(&format!("{name}-shipped"));
        let chunk = shipped_dir.join("gb_char.bin");
        std::fs::write(&chunk, build_chunk(&shipped()).expect("chunk")).expect("write chunk");
        std::fs::write(
            dir.join("gb_char.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(log).expect("log chunk"),
        )
        .expect("write log");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        Self { dir, chunk }
    }

    pub(super) fn records(name: &str, records: &[LogRecord]) -> Self {
        Self::new(name, &encode_log_records(records).expect("log"))
    }

    fn originals(&self) -> SystemOriginals {
        crate::LibraryTable::from([(
            1_u8,
            SystemLibrary::from_library(PhraseLibrary::open(&self.chunk).expect("map")),
        )])
        .into()
    }

    pub(super) fn open(&self) -> UserStore {
        crate::test_session::open(&self.dir, self.originals(), versions(), UserConfLaw::Pinyin)
            .expect("open")
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        if let Some(parent) = self.chunk.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

pub(super) fn header(old_total: u32, new_total: u32) -> LogRecord {
    LogRecord::ModifyHeader {
        old_total,
        new_total,
    }
}

/// A journal ADD writes the slot's offset, and the next item read uses that
/// word (074a2219 phrase_index.cpp:180-215,453-457).
#[test]
fn a_journal_add_into_a_mapped_hole_is_readable() {
    let original = item(0x4e94, 10);
    let mut grown = original.clone();
    grown
        .prons
        .push((vec![ChewingKey::from_pinyin("wu").unwrap().to_packed()], 11));
    let profile = Profile::records(
        "add-into-hole",
        &[
            header(185, 185),
            LogRecord::Add {
                token: 4,
                new_item: encode_phrase_item(&item(0x56db, 7)).unwrap(),
            },
            modify(
                5,
                &encode_phrase_item(&original).unwrap(),
                &encode_phrase_item(&grown).unwrap(),
            ),
        ],
    );
    let mut store = profile.open();
    assert!(
        store
            .originals()
            .get(&1)
            .unwrap()
            .mapped
            .unwrap()
            .is_detached()
    );
    assert_eq!(store.originals().get(&1).unwrap().range_end, 6);
    assert_eq!(store.system_unigram(LIBRARY | 4), Some(7));
    assert_eq!(
        store.with_user_item(LIBRARY | 4, |item| item.unigram()),
        Some(7)
    );
    assert_eq!(store.live_pronunciation_count(LIBRARY | 5), Some(2));
    assert!(store.with_user_item(LIBRARY | 5, |_| ()).is_none());
    // The pin merges a reading into the live item at the token found by
    // the phrase table (pinyin.cpp:545-569).
    let keys = super::packed_to_pinyin_keys(&item(0x56db, 7).prons[0].0).unwrap();
    assert_eq!(
        store
            .add_system_phrase_in(1, Some(LIBRARY | 4), &"四".into(), &keys, Some(3))
            .unwrap(),
        LIBRARY | 4
    );
    assert_eq!(
        store.with_user_item(LIBRARY | 4, |item| item.pronunciation(0).unwrap().freq),
        Some(10)
    );
}

/// B starts inside A's pronunciation area. A same-header MODIFY changes B's
/// header and count; B's enlarged extent stays inside the content buffer
/// (row 107's defined in-buffer overlap). The pin writes through A and saves
/// both slots, then stops at B's already-applied comparison on reopen:
/// `phrase_index.cpp:180-205,403-434,487-508` at 074a2219.
#[test]
fn internally_overlapping_slots_survive_same_header_modify_and_reopen() {
    let mut old = vec![1, 2];
    old.extend_from_slice(&100_u32.to_le_bytes());
    old.extend_from_slice(&0x4f60_u32.to_le_bytes());
    old.extend_from_slice(&[1, 0]);
    old.extend_from_slice(&50_u32.to_le_bytes());
    old.extend_from_slice(&0x597d_u32.to_le_bytes());
    old.extend_from_slice(&[0, 0]);
    let mut new = old.clone();
    new[11] = 1;
    new[12..16].copy_from_slice(&77_u32.to_le_bytes());
    let profile = Profile::records(
        "internal-overlap",
        &[header(150, 150), modify(1, &old, &new)],
    );
    let mut body = vec![0; 8];
    body.extend_from_slice(&old);
    body.extend_from_slice(&7_u32.to_le_bytes());
    let content = 17 + 3 * 4 + 1;
    let mut payload = Vec::new();
    for word in [150, 17, content, content + body.len() as u32 + 1] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.push(b'#');
    for offset in [0_u32, 8, 18] {
        payload.extend_from_slice(&offset.to_le_bytes());
    }
    payload.push(b'#');
    payload.extend_from_slice(&body);
    payload.push(b'#');
    std::fs::write(
        &profile.chunk,
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("chunk"),
    )
    .expect("write overlapping chunk");
    let mut store = profile.open();
    assert_eq!(store.system_unigram(LIBRARY | 1), Some(100));
    assert_eq!(store.system_unigram(LIBRARY | 2), Some(77));
    assert_eq!(store.live_pronunciation_count(LIBRARY | 2), Some(1));
    store.mark_modified();
    store.save_reporting().expect("save");
    drop(store);
    let reopened = profile.open();
    assert_eq!(reopened.system_unigram(LIBRARY | 1), Some(100));
    assert_eq!(reopened.system_unigram(LIBRARY | 2), Some(77));
    assert_eq!(reopened.live_pronunciation_count(LIBRARY | 2), Some(1));
}
