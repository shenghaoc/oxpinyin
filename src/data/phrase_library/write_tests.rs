use super::*;

// 074a2219 phrase_index.cpp:208-216,365-398 lays out slots, entries and separators.
#[test]
fn build_chunk_matches_hand_layout() {
    let items = [
        (
            1,
            &ChunkItem {
                phrase: vec![0x4f60], // 你
                unigram: 3,
                prons: vec![(vec![0x1234], 7)],
            },
        ),
        (
            3,
            &ChunkItem {
                phrase: vec![0x597d], // 好
                unigram: 5,
                prons: vec![(vec![0x5678], 11), (vec![0x5679], 2)],
            },
        ),
    ];
    let file = build_chunk(&items).expect("build");
    let (length, csum) = (
        u32::from_le_bytes(file[0..4].try_into().unwrap()),
        u32::from_le_bytes(file[4..8].try_into().unwrap()),
    );
    let payload = &file[8..];
    assert_eq!(usize::try_from(length).unwrap(), payload.len());
    assert_eq!(crate::data::chunk_format::chunk_checksum(payload), csum);

    let total = u32::from_le_bytes(payload[0..4].try_into().unwrap());
    assert_eq!(total, 8); // 3 + 5
    let i1 = u32::from_le_bytes(payload[4..8].try_into().unwrap());
    let i2 = u32::from_le_bytes(payload[8..12].try_into().unwrap());
    let i3 = u32::from_le_bytes(payload[12..16].try_into().unwrap());
    assert_eq!(i1, 17);
    assert_eq!(payload[16], b'#');
    assert_eq!(payload[usize::try_from(i2).unwrap() - 1], b'#');
    assert_eq!(payload[usize::try_from(i3).unwrap() - 1], b'#');
    assert_eq!(i3 as usize, payload.len());

    let item0_len = 6 + 4 + (2 + 4);
    let offs: Vec<u32> = (0..4)
        .map(|s| {
            let p = usize::try_from(i1).unwrap() + s * 4;
            u32::from_le_bytes(payload[p..p + 4].try_into().unwrap())
        })
        .collect();
    assert_eq!(offs, vec![0, 8, 0, 8 + u32::try_from(item0_len).unwrap()]);

    let e = usize::try_from(i2).unwrap();
    assert_eq!(payload[e..e + 8], [0; 8]);
    let p = e + 8;
    assert_eq!(&payload[p..p + 6], &[1, 1, 3, 0, 0, 0]);
    assert_eq!(
        u32::from_le_bytes(payload[p + 6..p + 10].try_into().unwrap()),
        0x4f60
    );
    assert_eq!(
        u16::from_le_bytes(payload[p + 10..p + 12].try_into().unwrap()),
        0x1234
    );
    assert_eq!(
        u32::from_le_bytes(payload[p + 12..p + 16].try_into().unwrap()),
        7
    );
}

// 074a2219 phrase_index.cpp:215 accumulates the stored unigram total at u32 width.
#[test]
fn build_chunk_writes_wrapped_unigram_total() {
    let item = ChunkItem {
        phrase: vec![0x4f60], // 你
        unigram: 3_000_000_000,
        prons: vec![(vec![0x1234], 7)],
    };
    let file = build_chunk(&[(1, &item), (2, &item)]).expect("build");
    let payload = &file[crate::data::chunk_format::CHUNK_HEADER_SIZE..];
    let total = u32::from_le_bytes(payload[..4].try_into().unwrap());
    assert_eq!(total, 1_705_032_704);
}

// Frozen toned mini-model culture.table produces this empty chunk at 074a2219.
#[test]
fn build_chunk_of_no_items_matches_the_pin() {
    let file = build_chunk(&[]).expect("build");
    assert_eq!(
        file,
        [
            19, 0, 0, 0, // payload length
            51, 35, 35, 0, // checksum
            0, 0, 0, 0, // total_freq
            17, 0, 0, 0, // index_one
            18, 0, 0, 0, // index_two: no offset array
            19, 0, 0, 0, // index_three: no entry area
            b'#', b'#', b'#'
        ]
    );
}

// Rust build_chunk requires increasing slots and nonempty phrases with matching key counts.
#[test]
fn build_chunk_rejects_disorder_and_shape_errors() {
    let item = ChunkItem {
        phrase: vec![1],
        unigram: 1,
        prons: vec![(vec![0], 1)],
    };
    assert!(build_chunk(&[(2, &item), (1, &item)]).is_none());
    assert!(build_chunk(&[(1, &item), (1, &item)]).is_none());
    assert!(build_chunk(&[(PHRASE_MASK + 1, &item)]).is_none());
    let bad_keys = ChunkItem {
        prons: vec![(vec![0, 0], 1)],
        ..item.clone()
    };
    assert!(build_chunk(&[(1, &bad_keys)]).is_none());
    let empty = ChunkItem {
        phrase: Vec::new(),
        prons: Vec::new(),
        ..item
    };
    assert!(build_chunk(&[(1, &empty)]).is_none());
}

// Rust library entries and journal item payloads share the PhraseItem encoding.
#[test]
fn phrase_item_codec_round_trips_and_matches_the_entry_area() {
    let item = ChunkItem {
        phrase: vec![0x4f60, 0x597d],
        unigram: 483,
        prons: vec![(vec![0x1234, 0x5678], 7), (vec![0x1235, 0x5679], 2)],
    };
    let bytes = encode_phrase_item(&item).expect("item encodes");
    assert_eq!(bytes.len(), 6 + 8 + 2 * (4 + 4));
    assert_eq!(&bytes[..6], &[2, 2, 0xe3, 0x01, 0x00, 0x00]);
    assert_eq!(decode_phrase_item(&bytes).expect("decode"), item);

    let file = build_chunk(&[(1, &item)]).expect("build");
    let body = &file[crate::data::chunk_format::CHUNK_HEADER_SIZE..];
    let index_two = usize::try_from(u32::from_le_bytes(body[8..12].try_into().unwrap())).unwrap();
    let entry_area = &body[index_two..];
    assert_eq!(&entry_area[8..8 + bytes.len()], &bytes[..]);
}

// Rust borrowed and owned item decoders require all declared fields and accept trailing bytes.
#[test]
fn phrase_view_matches_decoder_boundaries_and_permissive_values() {
    for (length, prons) in [(0_u8, 0_u8), (1, 2), (2, 1), (255, 255)] {
        let text_end = 6 + 4 * usize::from(length);
        let end = text_end + usize::from(prons) * (2 * usize::from(length) + 4);
        let mut bytes = vec![255; end + 1];
        bytes[0] = length;
        bytes[1] = prons;
        for cut in [0, 1, 2, 5, 6, text_end - 1, text_end, end - 1, end, end + 1] {
            let bytes = &bytes[..cut];
            assert_eq!(decode_phrase_item(bytes).is_some(), cut >= end);
            assert_eq!(
                crate::data::phrase_library::PhraseItemView::from_bytes(bytes).is_some(),
                cut >= end
            );
        }
    }
}
