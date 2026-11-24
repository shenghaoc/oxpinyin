use super::*;
use crate::data::chunk_format::build_memory_chunk;
use crate::data::phrase_library::{ChunkItem, build_chunk, encode_phrase_item};
use crate::data::user_files::read_chunk_payload;
use crate::user::test_session::borrowed_items;

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

/// Basis: `074a2219 phrase_index.cpp:335-362,624-646` (load keeps total and slots), `pinyin.cpp:2786-2798`;
/// policy row 111 bounds the body read.
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
    assert_eq!(library.total(), 91);
    assert!(!library.is_empty());
    assert!(library.item(1).is_none());
    assert_eq!(library.item(3).unwrap().as_bytes(), body);
    library.compact();
    assert_eq!(library.total(), 37);
    let saved = library.store().unwrap();
    let loaded = UserLibrary::load(&saved).unwrap();
    assert_eq!(loaded.total(), 37);
    assert_eq!(
        loaded.items().map(|(slot, _)| slot).collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(loaded.item(3).unwrap().as_bytes(), body);
    assert_eq!(saved, loaded.store().unwrap());

    let truncated = UserLibrary::load(&file(&body[..body.len() - 1])).unwrap();
    assert_eq!(truncated.total(), 91);
    assert_eq!(truncated.token_end(), 4);
    assert_eq!(truncated.pronunciation_count(3), Some(2));
    assert!(truncated.item(3).is_none());
    assert!(library.replace(4, &body[..body.len() - 1]).is_none());
    assert_eq!(library.store().unwrap(), saved);
}

/// Basis: `074a2219 phrase_index.cpp:56-93,150-178,219-235,624-675`.
#[test]
fn mutations_compact_to_the_pins_item_and_store_layout() {
    let mut library = UserLibrary::load(&build_chunk(&borrowed_items(&items())).unwrap()).unwrap();
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
    assert_eq!(
        library.store().unwrap(),
        build_chunk(&borrowed_items(&expected)).unwrap()
    );
    assert_eq!(library.content.len(), library.compacted_content_len());
}

/// Basis: `phrase_index.cpp:28,76-91` (running-total guard, `guint8` reading count).
#[test]
fn exact_reading_overflow_and_count_wrap_follow_the_pin() {
    let mut item = items().remove(0).1;
    item.prons = vec![(vec![0x1234], u32::MAX), (vec![0x2234], 1)];
    let mut library = UserLibrary::load(&build_chunk(&[(1, &item)]).unwrap()).unwrap();
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

/// Basis: `phrase_index.cpp:335-367` (load and store keep the offset holes).
#[test]
fn user_library_store_matches_build_chunk_with_offset_holes() {
    let items = items();
    let file = build_chunk(&borrowed_items(&items)).unwrap();
    let library = UserLibrary::load(&file).unwrap();
    assert_eq!(library.total(), 8);
    assert_eq!(
        library.items().map(|(slot, _)| slot).collect::<Vec<_>>(),
        [1, 3]
    );
    assert_eq!(
        library.item(3).unwrap().as_bytes(),
        encode_phrase_item(&items[1].1).unwrap()
    );
    assert_eq!(library.store().unwrap(), file);

    let file = build_chunk(&[]).unwrap();
    let library = UserLibrary::load(&file).unwrap();
    assert!(library.is_empty());
    assert_eq!(library.store().unwrap(), file);
    assert!(UserLibrary::default().content.is_empty());
}

fn assert_no_panic(file: &[u8]) {
    let _ = UserLibrary::load(file);
}

/// Basis: Rust contract (the loader answers every truncated or bad-offset frame without a panic).
#[test]
fn user_library_malformed_frames_match_main_without_panicking() {
    let file = build_chunk(&borrowed_items(&items())).unwrap();
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
    crate::data::phrase_library::encode_phrase_item(&ChunkItem {
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

/// Basis: `phrase_index.cpp:208-217` (`add_phrase_item` keeps the overwritten unigram in the total).
#[test]
fn an_add_over_a_live_slot_keeps_the_overwritten_unigram_in_the_total() {
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2, 3]);
    assert_eq!(library.total(), 6);
    library.add(2, &small(20)).unwrap();
    assert_eq!(library.item(2).unwrap().unigram(), 20);
    assert_eq!(library.total(), 26);
    assert_eq!(library.items().count(), 3);
    library.replace(2, &small(30)).unwrap();
    assert_eq!(library.total(), 36);
}

/// Basis: `phrase_index.cpp:180-206` and policy rows 107, 111 and 130 (a read
/// takes the header's extent, across records, and stops at the content end;
/// a write to that extent changes only its addressed item).
#[test]
fn a_read_crossing_the_item_boundary_follows_the_header_extent() {
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2]);
    let start = library.slots.get(&1).unwrap().start;
    library.content[start + 1] = 2;
    assert_eq!(library.item(1).unwrap().as_bytes().len(), 22);
    // The second reading of item 1 is the first bytes of item 2, key 0x0101 and count 2.
    let crossing = crate::parser::ChewingKey::from_packed(0x0101);
    library.train(1, std::iter::once(vec![crossing]), 5);
    assert_eq!(library.item(1).unwrap().pronunciation(1).unwrap().freq, 7);
    assert_eq!(library.item(2).unwrap().unigram(), 2);
    // The write moved item 1 to a copy at the end of the content.
    let start = library.slots.get(&1).unwrap().start;
    library.content[start + 1] = 255;
    assert!(library.item(1).is_none());
    assert_eq!(library.item(2).unwrap().unigram(), 2);
}

/// Basis: policy row 112 (the compacted content copy is checked at the limit).
#[test]
fn the_store_copy_threshold_follows_the_compacted_content() {
    let _limits = TestLimits::set(8 + 16 * 3);
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2]);
    assert!(!library.store_copy_is_unsafe());
    library.replace(3, &small(3)).unwrap();
    assert!(library.store_copy_is_unsafe());
    assert!(library.remove(3));
    assert!(!library.store_copy_is_unsafe());
    assert!(!UserLibrary::default().store_copy_is_unsafe());
}

/// Basis: `phrase_index.cpp:677-715` (mask removes items; compaction comes later).
#[test]
fn removing_a_batch_collects_nothing_until_the_compaction() {
    let mut library = UserLibrary::default();
    fill(&mut library, &[1, 2, 3, 4]);
    let unreadable = library.slots[&3].start;
    library.content[unreadable + 1] = 255;
    let size = library.content.len();
    let visited = std::cell::RefCell::new(Vec::new());
    library.remove_matching(|slot| {
        visited.borrow_mut().push(slot);
        slot != 4
    });
    assert_eq!(visited.into_inner(), [1, 2, 4]);
    assert!(library.slots.contains_key(&3));
    assert_eq!(library.content.len(), size);
    assert_eq!(library.total(), 7);
    library.compact();
    assert_eq!(library.content.len(), 8 + 16);
    assert_eq!(library.total(), 4);
}

/// Basis: `phrase_index.cpp:208-217,365-367` (`guint32` total, stored wrapped).
#[test]
fn a_total_past_u32_is_stored_wrapped() {
    let mut library = UserLibrary::default();
    library.replace(1, &small(0x8000_0000)).unwrap();
    library.replace(2, &small(0x8000_0005)).unwrap();
    assert_eq!(library.total(), 5);
    let saved = library.store().unwrap();
    assert_eq!(UserLibrary::load(&saved).unwrap().total(), 5);
}

/// Basis: `074a2219 phrase_index.cpp:180-205,335-362` (a bad offset fails only its own item read).
#[test]
fn a_bad_user_item_offset_leaves_neighbouring_items_readable() {
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
    let chunk = build_chunk(&borrowed_items(&items)).expect("three-item user chunk");
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_two = offsets_start as usize + 2 * 4;
    payload[slot_two..slot_two + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut library = UserLibrary::load(&build_memory_chunk(&payload).expect("frame")).unwrap();
    for (slot, expected, key) in [rows[0], rows[2]] {
        let item = library.item(slot).unwrap();
        assert_eq!(item.phrase_length(), 1);
        assert_eq!(item.phrase_text(), Some(expected.into()));
        assert_eq!(library.pronunciation_count(slot), Some(1));
        assert_eq!(item.pronunciation(0).unwrap().keys, key.to_le_bytes());
    }
    assert!(library.item(2).is_none());
    assert_eq!(library.pronunciation_count(2), None);
    // An append that lands where the bad offset's range once began does not make it readable.
    library.replace(4, &small(9)).unwrap();
    assert!(library.item(4).is_some());
    assert!(library.item(2).is_none());
    assert_eq!(library.pronunciation_count(2), None);
}
