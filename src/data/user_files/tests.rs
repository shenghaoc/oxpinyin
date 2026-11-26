use super::*;

// 074a2219 pinyin_internal.h:58-63 and data/table.conf.in define the user filenames.
#[test]
fn user_dbm_names_are_libpinyins_own() {
    assert_eq!(UserDbm::Bigram.file_name(), "user_bigram.db");
    assert_eq!(UserDbm::PinyinIndex.file_name(), "user_pinyin_index.bin");
    assert_eq!(UserDbm::PhraseIndex.file_name(), "user_phrase_index.bin");
    assert!(UserDbm::Bigram.is_hash());
    assert!(!UserDbm::PinyinIndex.is_hash());
    let layout = UserFileLayout::stock();
    assert_eq!(
        layout.user_libraries()[2],
        (7, FileName::from_bytes(b"user.bin"))
    );
    assert_eq!(
        layout.system_logs()[0],
        (1, FileName::from_bytes(b"gb_char.dbin"))
    );
}

// Rust chunk_payload maps frame faults to their I/O error kinds.
#[test]
fn chunk_payload_round_trip_through_the_frame() {
    let payload = encode_log_records(&[LogRecord::Add {
        token: 7,
        new_item: vec![1, 2, 3, 4],
    }])
    .expect("encode");
    let framed = crate::data::chunk_format::build_memory_chunk(&payload).expect("frame");
    assert_eq!(read_chunk_payload(&framed).expect("frame"), &payload[..]);

    let mut trailing = framed.clone();
    trailing.push(0);
    let mut flipped = framed.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xFF;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("user.bin");
    use std::io::ErrorKind::{InvalidData, UnexpectedEof};
    for (bytes, kind) in [
        (&framed[..7], InvalidData),
        (&framed[..last], UnexpectedEof),
        (&trailing, UnexpectedEof),
        (&flipped, InvalidData),
    ] {
        assert!(read_chunk_payload(bytes).is_none());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(load_chunk_payload(&path).unwrap_err().kind(), kind);
    }
}

fn parse(bytes: &[u8]) -> Result<UserTableInfo, UserConfError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("user.conf");
    std::fs::write(&path, bytes).expect("write");
    UserTableInfo::load_path(&path)
}

// 074a2219 table_info.cpp:325-426 saves marker fields and checks versions, format and counter.
#[test]
fn user_conf_round_trips_and_conforms() {
    let versions = SystemVersions::for_this_build(7, 14);
    let conform = UserTableInfo::conform_to(&versions);
    assert!(conform.is_conform(&versions));

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("user.conf");
    let filename = crate::input::c_path(&path).expect("c path");
    assert!(conform.save(&filename));
    let newline = crate::__internal::TEXT_NEWLINE;
    let expected = format!(
        "binary format version:7{newline}model data version:14{newline}database format:{DEFAULT_STORE_DB_FORMAT}{newline}open counter:0{newline}"
    );
    assert_eq!(std::fs::read(&path).expect("read"), expected.as_bytes());
    let reloaded = UserTableInfo::load(&filename).expect("reload saved marker");
    assert_eq!(reloaded, conform);
    assert!(reloaded.is_conform(&versions));

    let stale = UserTableInfo {
        model_data_version: 13,
        ..conform.clone()
    };
    assert!(!stale.is_conform(&versions));

    let mut other = conform.clone();
    if DEFAULT_STORE_DB_FORMAT == "KyotoCabinet" {
        other.database_format = Some("Tkrzw".to_owned());
    } else {
        other.database_format = Some("KyotoCabinet".to_owned());
    }
    assert!(!other.is_conform(&versions));

    for (counter, conforms, read) in [
        (-3, true, -3),
        (OPEN_COUNTER_LIMIT, true, OPEN_COUNTER_LIMIT),
        (OPEN_COUNTER_LIMIT + 1, false, 0),
    ] {
        let marker = UserTableInfo {
            open_counter: counter,
            ..conform.clone()
        };
        assert_eq!(marker.is_conform(&versions), conforms);
        assert_eq!(get_open_counter(counter), read);
    }

    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("table.conf"),
        b"binary format version:9\nmodel data version:20\nlambda parameter:1\n\
              source table format:pinyin\ndatabase format:Tkrzw\n",
    )
    .expect("write");
    let declared = crate::data::TableConf::load_from_dir(dir.path()).expect("parses");
    assert_eq!(
        SystemVersions::from_conf(&declared),
        SystemVersions {
            binary_format_version: 9,
            model_data_version: 20,
            database_format: "Tkrzw",
        }
    );

    let sparse = parse(b"binary format version:7\nmodel data version:14\n").expect("parse");
    assert_eq!(sparse.database_format, None);
    assert_eq!(sparse.open_counter, 0);
    assert!(!sparse.is_conform(&versions));
    assert!(matches!(
        parse(b"binary format version:x\n"),
        Err(UserConfError::Line("binary format version"))
    ));
    assert!(matches!(
        parse(b""),
        Err(UserConfError::Line("binary format version"))
    ));
}

// 074a2219 tests/storage/test_table_info.cpp:85-97 exercises make-conform, save and reopen.
#[test]
fn a_fresh_zero_version_marker_requires_conformance() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("user.conf");
    let filename = crate::input::c_path(&profile).unwrap();
    let versions = SystemVersions::for_this_build(0, 0);
    let fresh = UserTableInfo {
        binary_format_version: 0,
        model_data_version: 0,
        database_format: None,
        open_counter: 0,
    };
    assert!(!fresh.is_conform(&versions));
    let conformed = UserTableInfo::conform_to(&versions);
    assert!(conformed.save(&filename));
    let reloaded = UserTableInfo::load(&filename).unwrap();
    assert_eq!(
        reloaded.database_format.as_deref(),
        Some(DEFAULT_STORE_DB_FORMAT)
    );
    assert!(reloaded.is_conform(&versions));
    assert_eq!(reloaded, conformed);
    let directory = crate::input::c_path(dir.path()).unwrap();
    assert!(!UserTableInfo::load(&filename).unwrap().save(&directory));
    assert!(profile.is_file());
}

// 074a2219 table_info.cpp:339-359 distinguishes required fields and the optional counter.
#[test]
fn marker_fields_follow_the_scanner() {
    let good = "binary format version:7\nmodel data version:14\ndatabase format:BerkeleyDB\nopen counter:3\n";
    let counter = |value: &str| {
        parse(
            good.replace("counter:3", &format!("counter:{value}"))
                .as_bytes(),
        )
        .expect(value)
        .open_counter
    };
    assert_eq!(counter("3x"), 3);
    assert_eq!(counter("x"), 0);
    assert_eq!(counter("-3"), -3);
    assert!(matches!(
        parse(good.replace("version:7", "version:7x").as_bytes()),
        Err(UserConfError::Line("model data version"))
    ));
    assert_eq!(
        parse(good.replace("BerkeleyDB", "BerkeleyDB\0suffix").as_bytes())
            .unwrap()
            .database_format
            .as_deref(),
        Some("BerkeleyDB")
    );
    let missing = good.replace("open counter:3\n", "");
    assert_eq!(parse(missing.as_bytes()).unwrap().open_counter, 0);
}

fn save_and_read(info: &UserTableInfo) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("user.conf");
    assert!(info.save(path.as_path()));
    std::fs::read_to_string(path).expect("read the saved marker")
}

// 074a2219 table_info.cpp:377-397 prints the version words as signed integers.
#[test]
fn an_out_of_int_version_is_written_signed() {
    let versions = SystemVersions::for_this_build(u32::MAX, 0x8000_0000);
    let info = UserTableInfo::conform_to(&versions);
    let text = save_and_read(&info);
    let newline = crate::__internal::TEXT_NEWLINE;
    assert!(
        text.starts_with(&format!(
            "binary format version:-1{newline}model data version:-2147483648{newline}"
        )),
        "{text}"
    );
    assert_eq!(parse(text.as_bytes()).expect("parse"), info);
}

// 074a2219 phrase_index_logger.h:248-278 writes four record layouts.
#[test]
fn log_records_round_trip_through_the_pin_layout() {
    let records = vec![
        LogRecord::ModifyHeader {
            old_total: 138_096,
            new_total: 138_579,
        },
        LogRecord::Add {
            token: 0x0700_0001,
            new_item: vec![1, 1, 69, 0, 0, 0],
        },
        LogRecord::Modify {
            token: 0x0100_0002,
            old_item: vec![2, 3, 4],
            new_item: vec![5, 6],
        },
        LogRecord::Remove {
            token: 0x0200_0003,
            old_item: vec![7, 8, 9, 10],
        },
    ];
    let bytes = encode_log_records(&records).expect("encode");

    let mut expected = Vec::new();
    expected.extend_from_slice(&4_u32.to_le_bytes()); // MODIFY_HEADER
    expected.extend_from_slice(&0_u32.to_le_bytes()); // null_token
    expected.extend_from_slice(&4_u16.to_le_bytes()); // one u32 total per run
    expected.extend_from_slice(&138_096_u32.to_le_bytes());
    expected.extend_from_slice(&138_579_u32.to_le_bytes());
    expected.extend_from_slice(&1_u32.to_le_bytes()); // ADD
    expected.extend_from_slice(&0x0700_0001_u32.to_le_bytes());
    expected.extend_from_slice(&6_u16.to_le_bytes());
    expected.extend_from_slice(&[1, 1, 69, 0, 0, 0]);
    assert_eq!(&bytes[..expected.len()], &expected[..]);

    assert_eq!(
        owned(&bytes),
        records.into_iter().map(Ok).collect::<Vec<_>>()
    );
    assert!(owned(&[]).is_empty());
}

// Row 123 refuses a payload that exceeds the journal length field.
#[test]
fn log_encode_rejects_a_payload_beyond_u16() {
    let huge = vec![0_u8; 70_001];
    assert!(
        encode_log_records(&[LogRecord::Add {
            token: 7,
            new_item: huge,
        }])
        .is_none(),
        "an oversized payload must be refused, not truncated"
    );
}

// 074a2219 phrase_index_logger.h:127-216 retains earlier records and records the termination reason.
#[test]
fn the_walk_reports_why_it_ended() {
    let add = record(1, 7, &[2, 0, 1, 0]);
    for (tail, expected) in [
        (Vec::new(), LogEnd::Complete),
        (record(9, 0, &[]), LogEnd::UnknownType),
        (vec![1, 0], LogEnd::UnknownType),
    ] {
        let bytes = [add.clone(), tail].concat();
        let mut walk = log_records(&bytes);
        assert_eq!(walk.end(), None);
        assert_eq!(
            walk.next(),
            Some(Ok(LogRecord::Add {
                token: 7,
                new_item: &[1, 0][..]
            }))
        );
        assert!(walk.next().is_none());
        assert_eq!(walk.end(), Some(expected));
    }
    let bytes = record(1, 7, &[6, 0, 1]);
    let mut walk = log_records(&bytes);
    assert_eq!(walk.next(), Some(Err(LogFault::OverRead)));
    assert_eq!(walk.end(), Some(LogEnd::OverRead));
}

fn owned(bytes: &[u8]) -> Vec<Result<LogRecord, LogFault>> {
    log_records(bytes)
        .map(|record| record.map(LogRecord::into_owned))
        .collect()
}

fn record(kind: u32, token: u32, tail: &[u8]) -> Vec<u8> {
    [
        kind.to_le_bytes().as_slice(),
        token.to_le_bytes().as_slice(),
        tail,
    ]
    .concat()
}

// Rust LogReader stops at a missing field without supplying a default.
#[test]
fn incomplete_records_stop_after_the_last_complete_record() {
    let add = record(1, 7, &[6, 0, 0, 0, 1, 0, 0, 0]);
    for tail in [
        1_u32.to_le_bytes().to_vec(),
        record(2, 5, &[3]),
        record(3, 5, &[]),
        record(4, 0, &[]),
    ] {
        let bytes = [add.as_slice(), &tail].concat();
        let mut records = log_records(&bytes);
        assert_eq!(
            records.next(),
            Some(Ok(LogRecord::Add {
                token: 7,
                new_item: &[0_u8, 0, 1, 0, 0, 0][..],
            }))
        );
        assert!(records.next().is_none());
        assert_eq!(records.end(), Some(LogEnd::Truncated));
    }
}

// Rust LogReader reports OverRead for a payload extending past the available bytes.
#[test]
fn a_payload_past_the_end_is_an_over_read() {
    let add = encode_log_records(&[LogRecord::Add {
        token: 1,
        new_item: vec![1, 2, 3],
    }])
    .expect("encode");
    assert_eq!(owned(&add[..add.len() - 1]), vec![Err(LogFault::OverRead)]);
    assert_eq!(
        owned(&record(4, 0, &[4, 0, 1, 2, 3, 4, 5, 6, 7])),
        vec![Err(LogFault::OverRead)]
    );
    assert_eq!(
        owned(&record(3, 1, &[1, 0, 2, 0, 9, 8])),
        vec![Err(LogFault::OverRead)]
    );
}

// 074a2219 phrase_index_logger.h:202 checks the header token before reading the payload length.
#[test]
fn a_header_token_faults_where_the_header_is_read() {
    assert_eq!(
        owned(&record(4, 1, &[])),
        vec![Err(LogFault::HeaderToken(1))]
    );
    let mut header = encode_log_records(&[
        LogRecord::Remove {
            token: 1,
            old_item: vec![1],
        },
        LogRecord::ModifyHeader {
            old_total: 1,
            new_total: 2,
        },
        LogRecord::Remove {
            token: 2,
            old_item: vec![2],
        },
    ])
    .expect("encode");
    header[11 + 4] = 1; // the header's token
    assert_eq!(
        owned(&header),
        vec![
            Ok(LogRecord::Remove {
                token: 1,
                old_item: vec![1],
            }),
            Err(LogFault::HeaderToken(1)),
        ]
    );
    assert!(owned(&[4_u32.to_le_bytes().as_slice(), &[1, 0, 0]].concat()).is_empty());
}

// 074a2219 phrase_index_logger.h:127-215 advances over the full declared runs.
#[test]
fn opaque_payloads_and_header_padding_are_kept() {
    let mut bytes = encode_log_records(&[
        LogRecord::Add {
            token: 1,
            new_item: vec![255],
        },
        LogRecord::Modify {
            token: 3,
            old_item: vec![255, 255],
            new_item: vec![],
        },
    ])
    .expect("encode");
    bytes.extend_from_slice(&record(4, 0, &[6, 0, 1, 0, 0, 0, 9, 9, 2, 0, 0, 0, 9, 9]));
    assert_eq!(
        owned(&bytes),
        vec![
            Ok(LogRecord::Add {
                token: 1,
                new_item: vec![255],
            }),
            Ok(LogRecord::Modify {
                token: 3,
                old_item: vec![255, 255],
                new_item: vec![],
            }),
            Ok(LogRecord::ModifyHeader {
                old_total: 1,
                new_total: 2,
            }),
        ]
    );
}
