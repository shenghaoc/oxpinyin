use super::*;
use crate::data::chunk_format::build_memory_chunk;
use crate::data::chunk_write::{ChunkItem, build_chunk, encode_phrase_item};
use crate::data::user_files::read_chunk_payload;

fn items() -> Vec<(u32, ChunkItem)> {
    vec![
        (
            1,
            ChunkItem {
                phrase: vec![0x4f60],
                unigram: 3,
                prons: vec![(vec![0x1234], 7)],
            },
        ),
        (
            3,
            ChunkItem {
                phrase: vec![0x597d, 0],
                unigram: 5,
                prons: vec![(vec![0xffff, 0], 11), (vec![0, 0xffff], 2)],
            },
        ),
    ]
}

/// Hand-built USER_FILE: slot 3 is present even with no phrase words;
/// the two empty-key readings retain their order and frequencies.
#[test]
fn persisted_empty_user_item_survives_load_compact_and_store() {
    let body = [0, 2, 37, 0, 0, 0, 13, 0, 0, 0, 5, 0, 0, 0];
    let file = |body: &[u8]| {
        let mut payload = Vec::new();
        for word in [91_u32, 17, 34, 34 + 8 + body.len() as u32 + 1] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        payload.push(b'#');
        for offset in [0_u32, 0, 0, 8] {
            payload.extend_from_slice(&offset.to_le_bytes());
        }
        payload.push(b'#');
        payload.extend_from_slice(&[0; 8]);
        payload.extend_from_slice(body);
        payload.push(b'#');
        build_memory_chunk(&payload).unwrap()
    };
    let mut library = UserLibrary::load(&file(&body)).unwrap();
    assert_eq!(library.total(), 91, "load retains the header total");
    assert!(!library.is_empty());
    assert!(library.item(1).is_none());
    assert_eq!(library.item(3).unwrap().as_bytes(), body);
    library.compact();
    assert_eq!(library.total(), 37, "compact rebuilds the item sum");
    let saved = library.store().unwrap();
    let loaded = UserLibrary::load(&saved).unwrap();
    assert_eq!(loaded.total(), 37);
    assert_eq!(
        loaded.items().map(|(slot, _)| slot).collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(loaded.item(3).unwrap().as_bytes(), body);
    assert_eq!(saved, loaded.store().unwrap());

    // At 074a2219, load keeps the stored total and slots
    // (phrase_index.cpp:335-362,624-646). The count reads only the item
    // header (pinyin.cpp:2786-2798); row 111 bounds reads of the body.
    let truncated = UserLibrary::load(&file(&body[..body.len() - 1])).unwrap();
    assert_eq!(truncated.total(), 91);
    assert_eq!(truncated.token_end(), 4);
    assert_eq!(truncated.pronunciation_count(3), Some(2));
    assert!(truncated.item(3).is_none());
    assert!(library.replace(4, &body[..body.len() - 1]).is_none());
    assert_eq!(library.store().unwrap(), saved);
}

/// 074a2219 phrase_index.cpp:56-93, :150-178, :219-235,
/// :624-675: ordered readings, prefix overflow, sparse token reuse,
/// and compaction produce the same item bytes and offset layout.
#[test]
fn mutations_compact_to_the_pins_item_and_store_layout() {
    let mut library = UserLibrary::load(&build_chunk(&items()).unwrap()).unwrap();
    let mut expected = items();
    library.add_reading(1, &[0x1234], 4).unwrap();
    expected[0].1.prons[0].1 += 4;
    library.add_reading(1, &[0x2234], 9).unwrap();
    expected[0].1.prons.push((vec![0x2234], 9));
    assert!(library.add_unigram(1, 11));
    expected[0].1.unigram += 11;
    assert!(library.remove(3));
    expected.pop();
    assert_eq!(library.token_end(), 2);
    library.compact();
    assert_eq!(library.store().unwrap(), build_chunk(&expected).unwrap());
    assert_eq!(library.content.len(), library.compacted_content_len());
}

#[test]
fn exact_reading_overflow_and_count_wrap_follow_the_pin() {
    let mut item = items().remove(0).1;
    item.prons = vec![(vec![0x1234], u32::MAX), (vec![0x2234], 1)];
    let mut library = UserLibrary::load(&build_chunk(&[(1, item)]).unwrap()).unwrap();
    library.add_reading(1, &[0x1234], 1).unwrap();
    assert_eq!(
        library.item(1).unwrap().pronunciation(0).unwrap().freq,
        u32::MAX
    );
    library.add_reading(1, &[0x2234], 1).unwrap();
    assert_eq!(library.item(1).unwrap().pronunciation(1).unwrap().freq, 2);
    for key in 1..=253 {
        library.add_reading(1, &[key], 1).unwrap();
    }
    assert_eq!(library.item(1).unwrap().n_pronunciations(), 255);
    library.add_reading(1, &[254], 1).unwrap();
    assert_eq!(library.item(1).unwrap().n_pronunciations(), 0);
    library.add_reading(1, &[255], 2).unwrap();
    let view = library.item(1).unwrap();
    assert_eq!(view.n_pronunciations(), 1);
    assert_eq!(view.pronunciation(0).unwrap().freq, 2);
}

#[test]
fn user_library_store_matches_build_chunk_with_offset_holes() {
    for slot in [3, 1024] {
        let mut items = items();
        items[1].0 = slot;
        let file = build_chunk(&items).unwrap();
        let library = UserLibrary::load(&file).unwrap();

        assert_eq!(library.total(), 8, "the two item unigrams sum to 8");
        assert_eq!(
            library.items().map(|(slot, _)| slot).collect::<Vec<_>>(),
            [1, slot],
            "the loaded slots"
        );
        assert_eq!(library.item(1).unwrap().n_pronunciations(), 1);
        assert_eq!(
            library.item(slot).unwrap().as_bytes(),
            encode_phrase_item(&items[1].1).unwrap(),
            "the loaded item bytes"
        );
        assert_eq!(library.item(slot).unwrap().n_pronunciations(), 2);
        assert_eq!(
            library.store().unwrap(),
            file,
            "stored bytes equal the file"
        );
    }

    let file = build_chunk(&[]).unwrap();
    let library = UserLibrary::load(&file).unwrap();
    assert!(library.is_empty());
    assert_eq!(library.store().unwrap(), file);
    assert!(UserLibrary::default().content.is_empty());
}

fn assert_no_panic(file: &[u8]) {
    // The production loader must answer without panicking for every truncation.
    let _ = UserLibrary::load(file);
}

#[test]
fn user_library_malformed_frames_match_main_without_panicking() {
    let file = build_chunk(&items()).unwrap();
    for cut in 0..=file.len() {
        assert_no_panic(&file[..cut]);
    }
    let payload = read_chunk_payload(&file).unwrap();
    for cut in 0..=payload.len() {
        assert_no_panic(&build_memory_chunk(&payload[..cut]).unwrap());
    }
    for offset in [1_u32, 7, 8, 1024, u32::MAX] {
        let mut bytes = payload.to_vec();
        bytes[21..25].copy_from_slice(&offset.to_le_bytes());
        assert_no_panic(&build_memory_chunk(&bytes).unwrap());
    }
}

/// A one-character, one-reading item: sixteen bytes.
fn small(unigram: u32) -> Vec<u8> {
    crate::data::chunk_write::encode_phrase_item(&ChunkItem {
        phrase: vec![0x4e00 + unigram],
        unigram,
        prons: vec![(vec![0x1234], unigram + 100)],
    })
    .unwrap()
}

fn fill(library: &mut UserLibrary, unigrams: &[u32]) {
    for (slot, &unigram) in (1..).zip(unigrams) {
        library.replace(slot, &small(unigram)).unwrap();
    }
}

/// A wrapped allocation lands on a live slot (register row 119):
/// `add_phrase_item` alone moves the offset word and adds the new
/// unigram, and the overwritten item's unigram stays in `m_total_freq`
/// (`phrase_index.cpp:208-217`), where `replace` takes it out first.
#[test]
fn an_add_over_a_live_slot_keeps_the_overwritten_unigram_in_the_total() {
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2, 3]);
    assert_eq!(library.total(), 6);
    library.add(2, &small(20)).unwrap();
    assert_eq!(library.item(2).unwrap().unigram(), 20);
    assert_eq!(library.total(), 26, "slot 2's old unigram stays");
    assert_eq!(library.items().count(), 3);
    library.replace(2, &small(30)).unwrap();
    assert_eq!(library.total(), 36, "a replace removes first");
}

/// POLICY-ACCEPTED: reading a slot cannot consume the next item's bytes.
#[test]
fn a_read_crossing_the_item_boundary_answers_no_item() {
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2]);
    let start = library.slots.get(&1).unwrap().start;
    library.content[start + 1] = 2;
    assert!(library.item(1).is_none());
    library.content[start + 1] = 255;
    assert!(library.item(1).is_none());
    assert_eq!(library.item(2).unwrap().unigram(), 2);
}

/// Row 112: the save copies the compacted content into a fresh chunk;
/// from the limit on, the copy is unsafe.
#[test]
fn the_store_copy_threshold_follows_the_compacted_content() {
    let _limits = TestLimits::set(8 + 16 * 3);
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2]);
    assert!(!library.store_copy_is_unsafe());
    library.replace(3, &small(3)).unwrap();
    assert!(
        library.store_copy_is_unsafe(),
        "8 + 3 * 16 reaches the limit"
    );
    assert!(library.remove(3));
    assert!(!library.store_copy_is_unsafe());
    assert!(!UserLibrary::default().store_copy_is_unsafe());
}

/// Mask-out removes slots and leaves the compaction to the pin's point.
#[test]
fn removing_a_batch_collects_nothing_until_the_compaction() {
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2, 3, 4]);
    let size = library.content.len();
    library.remove_matching(|slot| slot != 4);
    assert_eq!(library.content.len(), size);
    assert_eq!(library.total(), 4);
    library.compact();
    assert_eq!(library.content.len(), 8 + 16);
    assert_eq!(library.total(), 4);
}

/// `store` writes the pin's `guint32` `m_total_freq`, which a journal ADD
/// wraps (`phrase_index.cpp:208-217`, `:365-367`): the file carries the
/// wrapped sum and loads back.
#[test]
fn a_total_past_u32_is_stored_wrapped() {
    let mut library = UserLibrary::default();
    library.replace(1, &small(0x8000_0000)).unwrap();
    library.replace(2, &small(0x8000_0005)).unwrap();
    assert_eq!(library.total(), 5);
    let saved = library.store().unwrap();
    assert_eq!(UserLibrary::load(&saved).unwrap().total(), 5);
}

/// At 074a2219 the USER_FILE load installs the whole offset and content
/// sections (phrase_index.cpp:335-362). A bad offset fails only the item
/// read (phrase_index.cpp:180-205); neighbouring items remain readable.
#[test]
fn a_bad_user_item_offset_leaves_neighbouring_items_readable() {
    use crate::data::chunk_format::build_memory_chunk;
    use crate::data::chunk_write::{ChunkItem, build_chunk};
    use crate::user::native_fixture::{TempUserDir, system_dir, write_user_conf};

    let user = TempUserDir::new("user-file-bad-item");
    write_user_conf(&user.path, &system_dir());
    let rows = [(1, "你", 0x302b_u16), (2, "好", 0x30a7), (3, "吗", 0x5083)];
    let items = rows.map(|(slot, phrase, key)| {
        (
            slot,
            ChunkItem {
                phrase: phrase.chars().map(u32::from).collect(),
                unigram: 17,
                prons: vec![(vec![key], 5)],
            },
        )
    });
    let chunk = build_chunk(&items).expect("three-item user chunk");
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_two = offsets_start as usize + 2 * 4;
    payload[slot_two..slot_two + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");

    let library = UserLibrary::load(&std::fs::read(user.path.join("user.bin")).unwrap()).unwrap();
    for (slot, expected, key) in [rows[0], rows[2]] {
        let item = library.item(slot).unwrap();
        assert_eq!(item.phrase_length(), 1);
        assert_eq!(item.phrase_text(), Some(expected.into()));
        assert_eq!(library.pronunciation_count(slot), Some(1));
        assert_eq!(item.pronunciation(0).unwrap().keys, key.to_le_bytes());
    }
    assert!(library.item(2).is_none());
    assert_eq!(library.pronunciation_count(2), None);
}
