//! What the session holds of a system slot's state before its first write.
//!
//! The pin's `SubPhraseIndex` keeps none (`phrase_index.h:259-262` at
//! 074a2219): `merge` edits the mapped chunk in place (`phrase_index.cpp:439-520`)
//! and a save diffs the file on disk again (`pinyin.cpp:955`, `:963`). The
//! session needs one fact per slot: whether `get_phrase_item` found an item
//! there before the slot's first write. It tells an item a journal ADD or an
//! import put into the library from one the chunk shipped, and a removed item
//! from a slot that never held one.
//!
//! Each test opens a mapped chunk through the session's own load, so the
//! chunk's mapping is the live buffer, and reads the answers the lookups use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

fn bytes(item: &ChunkItem) -> Vec<u8> {
    encode_phrase_item(item).expect("item bytes")
}

fn modify(slot: u32, old: &[u8], new: &[u8]) -> LogRecord {
    LogRecord::Modify {
        token: slot,
        old_item: old.to_vec(),
        new_item: new.to_vec(),
    }
}

fn add(slot: u32, new: &[u8]) -> LogRecord {
    LogRecord::Add {
        token: slot,
        new_item: new.to_vec(),
    }
}

fn remove(slot: u32, old: &[u8]) -> LogRecord {
    LogRecord::Remove {
        token: slot,
        old_item: old.to_vec(),
    }
}

/// The shipped chunk: items at slots 1, 2, 3 and 5, a hole at slot 4, six
/// slots in the offset array.
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
        BTreeMap::from([(
            1_u8,
            SystemLibrary::from_library(PhraseLibrary::open(&self.chunk).expect("map")),
        )])
        .into()
    }

    pub(super) fn open(&self) -> UserStore {
        UserStore::open_libpinyin(&self.dir, self.originals(), versions(), UserConfLaw::Pinyin)
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

/// Whether the session reads `slot`'s item as one the chunk lacked.
fn appended(store: &UserStore, slot: u32) -> bool {
    store.with_appended_item(LIBRARY | slot, |_| ()).is_some()
}

/// Whether the session reads `slot`'s item as removed from the chunk.
fn removed(store: &UserStore, slot: u32) -> bool {
    store.system_item_removed(LIBRARY | slot)
}

fn chunk_is_mapped(path: &Path) -> bool {
    std::fs::read_to_string("/proc/self/maps").is_ok_and(|maps| {
        maps.lines()
            .any(|line| line.ends_with(path.to_str().expect("path")))
    })
}

/// A log of same-header in-place MODIFY records is the ordinary profile:
/// every slot it names held an item, keeps it readable, and no slot's
/// provenance needs a record.
#[test]
fn a_log_of_in_place_modifies_records_nothing() {
    let [(_, one), (_, two), ..] = shipped();
    let (mut one_new, mut two_new) = (one.clone(), two.clone());
    one_new.unigram = 120;
    two_new.unigram = 77;
    let profile = Profile::records(
        "plain",
        &[
            header(185, 232),
            modify(1, &bytes(&one), &bytes(&one_new)),
            modify(2, &bytes(&two), &bytes(&two_new)),
        ],
    );
    let store = profile.open();
    assert_eq!(store.system_unigram(LIBRARY | 1), Some(120));
    assert_eq!(store.system_unigram(LIBRARY | 2), Some(77));
    assert_eq!(store.system_library_total(1), Some(232));
    assert!(!store.has_removed_system_items());
    assert_eq!(store.system_item_end(1), None, "no item was added");
    for slot in 1..=5 {
        assert!(!appended(&store, slot), "slot {slot}");
        assert!(!removed(&store, slot), "slot {slot}");
    }
    assert!(!store.system_provenance_recorded(1));
    if cfg!(target_os = "linux") {
        assert!(chunk_is_mapped(&profile.chunk), "the chunk is the buffer");
    }
}

/// ADD into a hole of the loaded offset array and past its end: both items
/// are ones the chunk lacked. Only the hole needs a record.
#[test]
fn an_add_into_a_hole_and_one_past_the_array_are_appended_items() {
    let (hole, past) = (bytes(&item(0x56db, 40)), bytes(&item(0x516d, 60)));
    let profile = Profile::records("add", &[header(185, 185), add(4, &hole), add(9, &past)]);
    let store = profile.open();
    assert!(appended(&store, 4));
    assert!(appended(&store, 9));
    assert!(
        !appended(&store, 1),
        "a shipped item is not an appended one"
    );
    assert!(!removed(&store, 4));
    assert!(!store.has_removed_system_items());
    assert_eq!(store.system_item_end(1), Some(LIBRARY | 10));
    assert_eq!(store.unigram_delta(LIBRARY | 4).expect("delta"), 40);
    assert_eq!(store.system_library_total(1), Some(285));
    assert!(store.system_provenance_recorded(1));
}

/// REMOVE of a shipped item after a plain MODIFY of another: the removed slot
/// held an item, the modified one still does. The REMOVE makes the log
/// exceptional, so the earlier MODIFY's slot is handed back and classified.
#[test]
fn a_remove_of_a_shipped_item_is_a_removal_and_earlier_modifies_stand() {
    let [(_, one), (_, two), ..] = shipped();
    let mut one_new = one.clone();
    one_new.unigram = 120;
    let profile = Profile::records(
        "remove",
        &[
            header(185, 205),
            modify(1, &bytes(&one), &bytes(&one_new)),
            remove(2, &bytes(&two)),
        ],
    );
    let store = profile.open();
    assert!(store.has_removed_system_items());
    assert!(removed(&store, 2));
    assert!(!removed(&store, 1));
    assert!(!removed(&store, 4), "a hole was never an item");
    assert!(!appended(&store, 1));
    assert!(!appended(&store, 2));
    assert_eq!(store.system_unigram(LIBRARY | 1), Some(120));
    assert_eq!(store.system_unigram(LIBRARY | 2), None);
    assert_eq!(store.system_library_total(1), Some(155));
    assert_eq!(store.system_item_end(1), None);
}

/// ADD into a hole, then REMOVE of that item: the slot never held a shipped
/// item, so nothing of the chunk was removed.
#[test]
fn a_removed_added_item_is_not_a_removed_shipped_item() {
    let hole = bytes(&item(0x56db, 40));
    let profile = Profile::records(
        "add-remove",
        &[header(185, 185), add(4, &hole), remove(4, &hole)],
    );
    let store = profile.open();
    assert!(!store.has_removed_system_items());
    assert!(!removed(&store, 4));
    assert!(!appended(&store, 4), "the slot holds no item");
    assert_eq!(store.system_library_total(1), Some(185));
}

/// REMOVE of a shipped item, then ADD under the same slot: the slot held a
/// shipped item at its first write, so the new item is not an appended one.
#[test]
fn an_add_over_a_removed_shipped_item_is_not_an_appended_item() {
    let [_, (_, two), ..] = shipped();
    let again = bytes(&item(0x597d, 90));
    let profile = Profile::records(
        "remove-add",
        &[header(185, 185), remove(2, &bytes(&two)), add(2, &again)],
    );
    let store = profile.open();
    assert!(!appended(&store, 2));
    assert!(!removed(&store, 2));
    assert_eq!(store.system_unigram(LIBRARY | 2), Some(90));
    assert_eq!(store.system_item_end(1), None);
}

/// REMOVE of a slot that holds no item, with an old chunk that is an item:
/// the pin dereferences NULL (row 107); here it clears the offset word and
/// goes on. The slot's first write found it empty, so a later ADD is an
/// appended item.
#[test]
fn a_remove_of_an_absent_slot_then_an_add_is_an_appended_item() {
    let hole = bytes(&item(0x56db, 40));
    let profile = Profile::records(
        "remove-absent",
        &[header(185, 185), remove(4, &hole), add(4, &hole)],
    );
    let store = profile.open();
    assert!(appended(&store, 4));
    assert!(!removed(&store, 4));
    assert!(!store.has_removed_system_items());
}

/// A MODIFY that grows the item moves it to the content's end: the slot held
/// a shipped item and still names one.
#[test]
fn a_growing_modify_keeps_the_slot_a_shipped_one() {
    let [(_, one), ..] = shipped();
    let mut grown = one.clone();
    grown.prons.push((
        vec![ChewingKey::from_pinyin("hao").expect("key").to_packed()],
        7,
    ));
    let profile = Profile::records(
        "grow",
        &[header(185, 185), modify(1, &bytes(&one), &bytes(&grown))],
    );
    let store = profile.open();
    assert!(!appended(&store, 1));
    assert!(!removed(&store, 1));
    assert!(!store.has_removed_system_items());
    assert_eq!(store.live_pronunciation_count(LIBRARY | 1), Some(2));
    assert_eq!(store.system_item_end(1), None);
}

/// A plain MODIFY of slot 2, then two writes through slot 1 that reach into
/// slot 2: the first claims a longer item than was stored, the second writes
/// slot 2's header through it, so slot 2's item ends past the buffer (row
/// 107). Slot 2 has no exceptional record of its own. It is classified all
/// the same: the first over-length write hands the earlier plain MODIFY
/// back, and the finish finds its item unreadable.
#[test]
fn a_plain_modify_before_an_overlapping_write_is_classified() {
    let [(_, one), (_, two), ..] = shipped();
    let mut two_new = two.clone();
    two_new.unigram = 77;
    let (one_bytes, two_bytes) = (bytes(&one), bytes(&two_new));
    // Six bytes that claim two two-word readings: 30 bytes, 14 of them slot
    // 2's.
    let expanded = {
        let mut header = vec![2, 2];
        header.extend_from_slice(&200_u32.to_le_bytes());
        header
    };
    let old = [
        expanded.clone(),
        one_bytes[6..].to_vec(),
        two_bytes[..14].to_vec(),
    ]
    .concat();
    let mut new = old.clone();
    // Slot 2's two header bytes: a length no buffer holds.
    new[one_bytes.len()] = 255;
    new[one_bytes.len() + 1] = 255;
    let profile = Profile::records(
        "overlap",
        &[
            header(185, 185),
            modify(2, &bytes(&two), &two_bytes),
            modify(1, &one_bytes, &expanded),
            modify(1, &old, &new),
        ],
    );
    let store = profile.open();
    assert!(store.has_removed_system_items());
    assert!(removed(&store, 2), "slot 2 held an item and reads none");
    assert_eq!(store.live_pronunciation_count(LIBRARY | 2), None);
    assert_eq!(store.system_unigram(LIBRARY | 2), None);
    assert!(!removed(&store, 3));
    assert!(!appended(&store, 2));
}

/// A payload past the log's end reverses the applied records: the ADD's
/// provenance goes with its item.
#[test]
fn a_rolled_back_replay_leaves_no_provenance() {
    let hole = bytes(&item(0x56db, 40));
    let mut log = encode_log_records(&[header(185, 185), add(4, &hole)]).expect("log");
    // An ADD whose payload runs past the log.
    log.extend_from_slice(&1_u32.to_le_bytes());
    log.extend_from_slice(&9_u32.to_le_bytes());
    log.extend_from_slice(&64_u16.to_le_bytes());
    log.extend_from_slice(&[1, 1, 0]);
    let profile = Profile::new("rollback", &log);
    let store = profile.open();
    assert!(!appended(&store, 4));
    assert!(!appended(&store, 9));
    assert_eq!(store.system_item_end(1), None);
    assert_eq!(store.system_library_total(1), Some(185));
    assert!(!store.system_provenance_recorded(1));
}

/// A reload and an unload replace or drop the live buffer, and the slots'
/// provenance with it.
#[test]
fn a_reload_and_an_unload_replace_the_provenance_with_the_buffer() {
    let hole = bytes(&item(0x56db, 40));
    let profile = Profile::records("reload", &[header(185, 185), add(4, &hole)]);
    let mut store = profile.open();
    assert!(appended(&store, 4));
    assert!(store.system_provenance_recorded(1));
    // The saved log is replaced by one without the ADD: the reload's buffer
    // is the freshly mapped chunk with that log replayed.
    std::fs::write(
        profile.dir.join("gb_char.dbin"),
        oxpinyin_data::chunk_format::build_memory_chunk(
            &encode_log_records(&[header(185, 185)]).expect("log"),
        )
        .expect("log chunk"),
    )
    .expect("replace log");
    let mut originals = profile.originals();
    store
        .load_system_libraries_retaining(&mut originals)
        .expect("reload");
    assert!(!appended(&store, 4));
    assert!(!store.system_provenance_recorded(1));
    // An import into the reloaded buffer, then the unload.
    let keys =
        super::packed_to_pinyin_keys(&[ChewingKey::from_pinyin("si").expect("key").to_packed()])
            .expect("keys");
    let token = store
        .add_system_phrase_in(1, None, &"四".into(), &keys, Some(5))
        .expect("import");
    assert!(appended(&store, token & 0x00ff_ffff));
    store.reset_system_library(1);
    assert!(!appended(&store, token & 0x00ff_ffff));
    assert!(!store.system_provenance_recorded(1));
}

/// Runtime writes into a present item's fields record nothing; an import's
/// new item and its replacement by a merged reading stay appended.
#[test]
fn runtime_writes_keep_the_first_write_presence() {
    let profile = Profile::records("runtime", &[header(185, 185)]);
    let mut store = profile.open();
    assert!(
        store
            .add_system_unigram_frequency(LIBRARY | 1, 3)
            .expect("add")
    );
    assert!(!store.system_provenance_recorded(1));
    assert_eq!(store.system_unigram(LIBRARY | 1), Some(103));
    let si =
        super::packed_to_pinyin_keys(&[ChewingKey::from_pinyin("si").expect("key").to_packed()])
            .expect("keys");
    let token = store
        .add_system_phrase_in(1, None, &"四".into(), &si, Some(5))
        .expect("import");
    let slot = token & 0x00ff_ffff;
    assert!(appended(&store, slot));
    // A second reading: the pin removes the item and adds the merged one
    // (`pinyin.cpp:567-581`). The slot stays an appended one.
    let shi =
        super::packed_to_pinyin_keys(&[ChewingKey::from_pinyin("shi").expect("key").to_packed()])
            .expect("keys");
    store
        .add_system_phrase_in(1, Some(token), &"四".into(), &shi, Some(5))
        .expect("merge a reading");
    assert!(appended(&store, slot));
    assert!(!removed(&store, slot));
    assert_eq!(store.user_pronunciation_count(token), Some(2));
    assert!(!appended(&store, 1));
    assert!(!removed(&store, 1));
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
    assert!(!store.system_provenance_recorded(1));
    assert!(!removed(&store, 2));
    assert!(!appended(&store, 2));
    store.mark_modified();
    assert!(store.save().expect("save"));
    drop(store);
    let reopened = profile.open();
    assert_eq!(reopened.system_unigram(LIBRARY | 1), Some(100));
    assert_eq!(reopened.system_unigram(LIBRARY | 2), Some(77));
    assert_eq!(reopened.live_pronunciation_count(LIBRARY | 2), Some(1));
    assert!(!removed(&reopened, 2));
    assert!(!appended(&reopened, 2));
}

/// The first successful MODIFY of B establishes presence before A makes B's
/// header overreach. ADD must not replace that observation with absence.
/// B is slot 1 so save diffs it before A; its longer replacement relocates
/// on reopen before A writes the overlapping bytes again (074a2219
/// phrase_index.cpp:403-434,487-508).
#[test]
fn earlier_modify_presence_wins_over_later_add_after_overlap() {
    let b = bytes(&item(0x4e59, 50));
    let mut modified_b = b.clone();
    modified_b[2..6].copy_from_slice(&77_u32.to_le_bytes());
    let mut a = vec![1, 3];
    a.extend_from_slice(&100_u32.to_le_bytes());
    a.extend_from_slice(&0x4e19_u32.to_le_bytes());
    a.extend_from_slice(&b);
    a.extend_from_slice(&[0, 0]);
    let mut old_a = a.clone();
    old_a[10..26].copy_from_slice(&modified_b);
    let mut new_a = old_a.clone();
    new_a[10..12].copy_from_slice(&[255, 255]);
    let mut added_b = modified_b.clone();
    added_b[1] = 2;
    added_b.extend_from_slice(&modified_b[10..16]);
    let profile = Profile::records(
        "ordered-overlap",
        &[
            header(150, 150),
            modify(1, &b, &modified_b),
            modify(2, &old_a, &new_a),
            add(1, &added_b),
        ],
    );
    let mut body = vec![0; 8];
    body.extend_from_slice(&a);
    let content = 30;
    let mut payload = Vec::new();
    for word in [150, 17, content, content + body.len() as u32 + 1] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.push(b'#');
    for offset in [0_u32, 18, 8] {
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
    for reopened in [false, true] {
        assert!(!appended(&store, 1));
        assert!(!removed(&store, 1));
        assert_eq!(store.system_unigram(LIBRARY | 1), Some(77));
        assert_eq!(store.live_pronunciation_count(LIBRARY | 1), Some(2));
        let live =
            crate::LiveSystemItems::unmasked(store.originals().phrase_libraries(), Some(&store));
        assert!(!live.is_imported(LIBRARY | 1).expect("classification"));
        assert_eq!(
            store
                .system_item_override(LIBRARY | 1)
                .expect("override")
                .expect("item")
                .unigram,
            77
        );
        if !reopened {
            store.mark_modified();
            assert!(store.save().expect("save"));
            drop(store);
            store = profile.open();
        }
    }
}
