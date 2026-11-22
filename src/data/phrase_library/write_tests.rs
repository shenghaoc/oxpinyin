use super::*;

/// A hand-built two-item chunk: verifies header, checksum, slot
/// addressing, the 8-byte entry-area reservation, and item encoding.
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

    // Offset array: 4 slots (0..3); slot 1 → 8, slot 3 → 8 + item0 size.
    let item0_len = 6 + 4 + (2 + 4);
    let offs: Vec<u32> = (0..4)
        .map(|s| {
            let p = usize::try_from(i1).unwrap() + s * 4;
            u32::from_le_bytes(payload[p..p + 4].try_into().unwrap())
        })
        .collect();
    assert_eq!(offs, vec![0, 8, 0, 8 + u32::try_from(item0_len).unwrap()]);

    // Item 0 at entry-area offset 8: header {1, 1, 3}, text, one pron.
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

/// The reader half of this crate must accept what this writer emits.
#[test]
fn build_chunk_reads_back_through_phrase_library() {
    let items = [(
        1,
        &ChunkItem {
            phrase: vec![0x4f60, 0x597d],
            unigram: 9,
            prons: vec![(vec![0x0011, 0x0022], 9)],
        },
    )];
    let file = build_chunk(&items).expect("build");
    let dir = tempfile::Builder::new()
        .prefix("oxpinyin-chunks-test-")
        .tempdir()
        .expect("tmpdir");
    let path = dir.path().join("test.bin");
    std::fs::write(&path, &file).expect("write");
    let lib = crate::data::phrase_library::PhraseLibrary::open(&path).expect("open");
    assert_eq!(lib.total_freq(), 9);
    assert!(lib.has_items());
    let item = lib.item(0x0000_0001).expect("item");
    assert_eq!(
        item.phrase_text().as_ref().and_then(|text| text.as_str()),
        Some("你好")
    );
    assert_eq!(item.unigram(), 9);
    let pron = item.pronunciation(0).expect("pronunciation");
    assert_eq!(pron.keys, &[0x11_u8, 0x00, 0x22, 0x00]);
    assert_eq!(pron.freq, 9);
    drop(lib);
    dir.close().expect("cleanup");
}

/// An empty library is the 16-byte header plus three separators — no
/// 8-byte entry-area reservation, because `add_phrase_item` never ran.
/// The bytes are the pin's own `gen_binary_files` output for an empty
/// `.table` (the toned mini model's `culture.table`).
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
    let dir = tempfile::Builder::new()
        .prefix("oxpinyin-chunks-empty-")
        .tempdir()
        .expect("tmpdir");
    let path = dir.path().join("empty.bin");
    std::fs::write(&path, &file).expect("write");
    let lib = crate::data::phrase_library::PhraseLibrary::open(&path).expect("open");
    assert_eq!(lib.total_freq(), 0);
    assert!(lib.item(0x0000_0001).is_none());
    assert_eq!(lib.items().count(), 0);
    assert!(!lib.has_items());
    drop(lib);
    dir.close().expect("cleanup");
}

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
        ..item
    };
    assert!(build_chunk(&[(1, &empty)]).is_none());
}

#[test]
fn phrase_item_codec_round_trips_and_matches_the_entry_area() {
    let item = ChunkItem {
        phrase: vec![0x4f60, 0x597d],
        unigram: 483,
        prons: vec![(vec![0x1234, 0x5678], 7), (vec![0x1235, 0x5679], 2)],
    };
    let bytes = encode_phrase_item(&item).expect("item encodes");
    // {u8 len, u8 npron, u32 unigram, 2×u32 text, 2×(2×u16 keys, u32 freq)}
    assert_eq!(bytes.len(), 6 + 8 + 2 * (4 + 4));
    assert_eq!(&bytes[..6], &[2, 2, 0xe3, 0x01, 0x00, 0x00]);
    assert_eq!(decode_phrase_item(&bytes).expect("decode"), item);

    // The entry area a built chunk stores for the item decodes the
    // same way — one wire form, two containers.
    let file = build_chunk(&[(1, &item)]).expect("build");
    let body = &file[crate::data::chunk_format::CHUNK_HEADER_SIZE..];
    let index_two = usize::try_from(u32::from_le_bytes(body[8..12].try_into().unwrap())).unwrap();
    let entry_area = &body[index_two..];
    assert_eq!(&entry_area[8..8 + bytes.len()], &bytes[..]);
}

#[test]
fn phrase_view_matches_decoder_boundaries_and_permissive_values() {
    for length in [0_u8, 1, 2, 255] {
        for prons in [0_u8, 1, 2, 255] {
            let text_end = 6 + 4 * usize::from(length);
            let keys = 2 * usize::from(length);
            let row = keys + 4;
            let end = text_end + usize::from(prons) * row;
            // u32::MAX text, u16::MAX keys and wrapping-sized counts
            // are all legal to the decoder; validation adds no policy.
            let mut bytes = vec![255; end + 3];
            bytes[0] = length;
            bytes[1] = prons;
            let mut cuts: Vec<_> = (0..=text_end).collect();
            for pron in 0..usize::from(prons) {
                let start = text_end + pron * row;
                cuts.extend([start, start + keys, start + keys + 3, start + row]);
                if keys != 0 {
                    cuts.push(start + keys - 1);
                }
            }
            cuts.extend([end, end + 1, end + 3]);
            cuts.sort_unstable();
            cuts.dedup();
            for cut in cuts {
                assert_eq!(
                    crate::data::phrase_library::PhraseItemView::from_bytes(&bytes[..cut])
                        .is_some(),
                    decode_phrase_item(&bytes[..cut]).is_some(),
                    "length={length}, prons={prons}, cut={cut}"
                );
            }
            assert!(decode_phrase_item(&bytes).is_some());
        }
    }
}
