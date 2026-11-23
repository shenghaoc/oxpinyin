use std::path::Path;

use super::{LibraryError, PhraseLibrary, RowFault, map_header};

// 074a2219 memory_chunk.h:479-497 checks size before reading the two header words.
#[test]
fn mapping_header_size_and_read_failure_follow_pin() {
    let path = Path::new("chunk");
    let calls = std::cell::Cell::new(0);
    let read = || {
        calls.set(calls.get() + 1);
        Err(std::io::ErrorKind::Other.into())
    };
    assert!(matches!(
        map_header(path, 7, true, read),
        Err(LibraryError::Unmappable { .. })
    ));
    assert_eq!(calls.get(), 0);
    assert!(matches!(
        map_header(path, 8, true, read),
        Err(LibraryError::Row(RowFault::ChunkHeaderRead))
    ));
    assert_eq!(calls.get(), 1);
    let mut reads = 0;
    assert!(matches!(
        map_header(path, 8, true, || {
            reads += 1;
            if reads == 1 {
                Ok([0; 4])
            } else {
                Err(std::io::ErrorKind::Other.into())
            }
        }),
        Err(LibraryError::Row(RowFault::ChunkHeaderRead))
    ));
    assert_eq!(reads, 2);
    assert!(map_header(path, 8, true, || Ok([0; 4])).is_ok());
}

// 074a2219 memory_chunk.h:492-497 asserts on either failed header read.
#[rstest::rstest]
#[case::length(1)]
#[case::checksum(2)]
fn mapped_header_read_aborts(#[case] failed_read: usize) {
    let thread = std::thread::current();
    let name = thread.name().expect("libtest case name");
    crate::store::death_test::assert_aborts(name, || {
        let mut reads = 0;
        let _ = map_header(Path::new("chunk"), 8, false, || {
            reads += 1;
            if reads == failed_read {
                Err(std::io::ErrorKind::Other.into())
            } else {
                Ok([0; 4])
            }
        });
    });
}

// Rust pronunciation_count requires both header bytes within the indexed record.
#[test]
fn count_checks_both_header_bytes_within_the_record() {
    let mut payload = Vec::new();
    let content = [vec![0; 8], vec![255, 17, 255, 29]].concat();
    let offsets = [0_u32, 8, 9, 10, 12];
    let index_two = 17 + offsets.len() as u32 * 4 + 1;
    for value in [0, 17, index_two, index_two + content.len() as u32 + 1] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(b'#');
    for offset in offsets {
        payload.extend_from_slice(&offset.to_le_bytes());
    }
    payload.push(b'#');
    payload.extend_from_slice(&content);
    payload.push(b'#');
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("counts.bin");
    std::fs::write(
        &path,
        crate::data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write");
    let library = PhraseLibrary::open(&path).expect("open");
    assert_eq!(library.pronunciation_count(3), Some(29));
    assert!(library.item(3).is_none(), "the body is truncated");
    for token in [0, 1, 2, 4, 5, u32::MAX] {
        assert_eq!(library.pronunciation_count(token), None, "token {token}");
    }
}

#[cfg(all(test, unix, target_pointer_width = "64"))]
mod sparse_mapping_tests {
    use super::super::map_chunk;
    use std::io::Write;

    // Row 109 retains the whole mapping across a wrapped payload length.
    #[test]
    fn a_sparse_chunk_maps_the_whole_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("merged.bin");
        let mut file = std::fs::File::create(&path).expect("create");
        file.set_len((1_u64 << 32) + 8).expect("sparse length");
        file.write_all(&[0_u8; 8]).expect("header");
        drop(file);
        let mapped = map_chunk(&path, false).expect("map");
        assert_eq!(
            mapped.as_slice().len(),
            (1_usize << 32) + 8,
            "the sparse mapping covers the whole file"
        );
    }
}

mod phrase_library {
    use crate::data::chunk_format::{CHUNK_HEADER_SIZE, chunk_checksum};
    use crate::data::phrase_library::{PhraseLibrary, verify_chunk};
    use crate::data::support::{ChunkBuilder, write_temp};

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

    // 074a2219 phrase_index.cpp:180-205,624-640 resolves item fields through the masked slot.
    #[test]
    fn reads_items_texts_and_pronunciations() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let path = write_temp(scratch.path(), "sample", &sample());
        let library = PhraseLibrary::open(&path).expect("valid chunk");
        assert_eq!(library.total_freq(), 9);
        assert_eq!(library.token_range(), 1..10);

        let item = library.item_at_slot(4).expect("item at slot 4");
        assert_eq!(item.phrase_length(), 1);
        assert_eq!(item.n_pronunciations(), 1);
        assert_eq!(item.unigram(), 5);
        assert_eq!(
            item.phrase_text().as_ref().and_then(|text| text.as_str()),
            Some("你")
        );
        let pronunciations: Vec<_> = item.pronunciations().collect();
        assert_eq!(pronunciations.len(), 1);
        assert_eq!(pronunciations[0].keys, &[0x12, 0x34]);
        assert_eq!(pronunciations[0].freq, 5);

        let item = library.item_at_slot(9).expect("item at slot 9");
        assert_eq!(
            item.phrase_text().as_ref().and_then(|text| text.as_str()),
            Some("你好")
        );
        let pronunciations: Vec<_> = item.pronunciations().collect();
        assert_eq!(pronunciations.len(), 2);
        assert_eq!(pronunciations[1].keys, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(pronunciations[1].freq, 1);
        assert!(item.pronunciation(2).is_none());

        assert!(library.item(0x0100_0004).is_some());
        assert!(library.item(0x0F00_0004).is_some());

        assert!(library.item_at_slot(0).is_none());
        assert!(library.item_at_slot(5).is_none());
        assert!(library.item_at_slot(10).is_none());
        assert!(library.item_at_slot(usize::MAX).is_none());

        assert_eq!(library.items().count(), 2);
    }

    // Rust PhraseLibrary::open checks section bounds, separators and offset-array length.
    #[test]
    fn sub_index_layout_damage_is_refused() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let sample = sample();
        let cases: Vec<(&str, Vec<u8>)> = vec![
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
                let mut bytes = sample.clone();
                bytes[8 + 4..8 + 8].copy_from_slice(&18_u32.to_le_bytes());
                fix_checksum(&mut bytes);
                bytes
            }),
        ];
        for (name, bytes) in cases {
            let path = write_temp(scratch.path(), name, &bytes);
            let result = PhraseLibrary::open(&path);
            assert!(result.is_err(), "{name} must be refused, got {result:?}");
        }
    }

    // Rust item_at_slot returns None for an incomplete item header.
    #[test]
    fn item_damage_answers_no_item_never_panics() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let mut bytes = sample();
        let payload_len = bytes.len() - 8;
        let index_two = u32_at(&bytes[16..20]);
        let content_start = 8 + index_two as usize;
        let near_end = u32::try_from(8 + payload_len - 2 - content_start) // content-relative
            .expect("near_end fits u32");
        bytes[8 + 17 + 4 * 4..8 + 17 + 4 * 4 + 4].copy_from_slice(&near_end.to_le_bytes());
        fix_checksum(&mut bytes);
        let path = write_temp(scratch.path(), "damaged-item", &bytes);
        let library = PhraseLibrary::open(&path).expect("layout still validates");
        assert!(
            library.item_at_slot(4).is_none(),
            "malformed item is no item"
        );
    }

    fn u32_at(bytes: &[u8]) -> u32 {
        u32::from_le_bytes(*bytes.first_chunk::<4>().expect("whole fixture word"))
    }

    fn fix_checksum(bytes: &mut [u8]) {
        let sum = chunk_checksum(&bytes[CHUNK_HEADER_SIZE..]);
        bytes[4..CHUNK_HEADER_SIZE].copy_from_slice(&sum.to_le_bytes());
    }

    #[cfg(unix)]
    // 074a2219 memory_chunk.h:479-497 applies the size gate before reading a directory.
    #[test]
    fn a_directory_chunk_follows_the_size_the_file_system_reports() {
        use std::io::{Seek, SeekFrom};
        let scratch = tempfile::tempdir().expect("scratch directory");
        let path = scratch.path().join("directory-chunk");
        std::fs::create_dir(&path).expect("create temp dir");
        std::fs::write(path.join("e".repeat(25)), b"x").expect("fill the directory");
        let size = std::fs::File::open(&path).and_then(|mut dir| dir.seek(SeekFrom::End(0)));
        if matches!(size, Ok(n) if n >= 8) {
            crate::store::death_test::assert_aborts(
                "data::phrase_library::tests::phrase_library::a_directory_chunk_follows_the_size_the_file_system_reports",
                || {
                    let _ = PhraseLibrary::open(&path);
                },
            );
        } else {
            assert_eq!(
                PhraseLibrary::open(&path).unwrap_err().unmappable_path(),
                Some(path.as_path()),
            );
        }
    }

    // 074a2219 memory_chunk.h:475-519 and phrase_index.cpp:335-364 distinguish mapping from layout refusal.
    #[test]
    fn chunk_level_refusals_name_the_file_and_layout_faults_do_not() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let sample = sample();
        let chunk_faults: Vec<(&str, Vec<u8>)> = vec![
            ("short header", sample[..7].to_vec()),
            ("payload truncation", sample[..sample.len() - 1].to_vec()),
            ("checksum drift", {
                let mut bytes = sample.clone();
                bytes[4] ^= 0xFF;
                bytes
            }),
        ];
        for (name, bytes) in chunk_faults {
            let path = write_temp(scratch.path(), name, &bytes);
            let error = PhraseLibrary::open(&path).expect_err(name);
            assert_eq!(error.unmappable_path(), Some(path.as_path()), "{name}");
            let error = verify_chunk(&path).expect_err(name);
            assert_eq!(error.unmappable_path(), Some(path.as_path()), "{name}");
        }
        let absent = scratch.path().join("absent.bin");
        let error = PhraseLibrary::open(&absent).expect_err("absent file");
        assert_eq!(error.unmappable_path(), Some(absent.as_path()));

        let mut bytes = sample;
        bytes[8 + 16] = b'!';
        fix_checksum(&mut bytes);
        let path = write_temp(scratch.path(), "layout", &bytes);
        let error = PhraseLibrary::open(&path).expect_err("layout fault");
        assert!(error.unmappable_path().is_none(), "{error}");
        verify_chunk(&path).expect("the chunk itself maps");
    }
}

#[cfg(feature = "tkrzw")]
use crate::user::contract_data;

#[cfg(feature = "tkrzw")]
mod package_libraries {
    use super::contract_data;
    use crate::data::phrase_library::PhraseLibrary;

    // Frozen 074a2219 package contains 138,096 system items and the stored readings of 的.
    #[test]
    fn reads_the_pinned_package_libraries() {
        let dir = contract_data::contract_data_dir();

        let mut libraries = Vec::new();
        let mut items = 0_u64;
        for name in ["gb_char", "gbk_char", "opengram", "merged"] {
            let library = PhraseLibrary::open(&dir.join(format!("{name}.bin")))
                .unwrap_or_else(|e| panic!("{name}.bin: {e}"));
            assert!(library.total_freq() > 0, "{name} carries unigram mass");
            assert_eq!(library.token_range().start, 1, "{name} tokens start at 1");
            let count = library.items().count() as u64;
            assert!(count > 0, "{name} has items");
            items += count;
            libraries.push(library);
        }
        assert_eq!(items, 138_096);

        let gb = &libraries[0];
        let item = gb.item(0x0100_05DB).expect("的 present in gb_char");
        assert_eq!(
            item.phrase_text().as_ref().and_then(|text| text.as_str()),
            Some("的")
        );
        let mut freqs: Vec<u32> = item
            .pronunciations()
            .map(|pronunciation| pronunciation.freq)
            .collect();
        freqs.sort_unstable();
        assert_eq!(freqs, [11_000, 2_213_855]);
    }
}

// Row 130 preserves fields before a reading that extends past the content end.
#[test]
fn a_shipped_item_with_a_reading_past_the_content_end_keeps_its_fields() {
    let scratch = tempfile::tempdir().unwrap();
    let system = crate::user::native_fixture::truncated_second_reading_system(scratch.path());
    let library = PhraseLibrary::open(&system.join("gb_char.bin")).unwrap();
    let item = library.item_fields(0x0100_0001).unwrap();
    assert_eq!(item.unigram(), 100);
    assert_eq!(item.phrase_text(), Some("丙".into()));
    assert_eq!(
        item.pronunciation(0).unwrap().keys,
        0x1081_u16.to_le_bytes()
    );
    assert!(item.pronunciation(1).is_none());
}
