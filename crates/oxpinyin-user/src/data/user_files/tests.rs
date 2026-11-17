use super::*;

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

#[test]
fn chunk_payload_round_trip_through_the_frame() {
    let payload = encode_log_records(&[LogRecord::Add {
        token: 7,
        new_item: vec![1, 2, 3, 4],
    }])
    .expect("encode");
    let framed = crate::data::chunk_format::build_memory_chunk(&payload).expect("frame");
    assert_eq!(read_chunk_payload(&framed).expect("frame"), &payload[..]);

    // Hostile frames answer typed errors, never panic.
    assert!(matches!(
        read_chunk_payload(&framed[..7]),
        Err(ChunkReadError::ShortHeader)
    ));
    let mut truncated = framed.clone();
    truncated.truncate(framed.len() - 1);
    assert!(matches!(
        read_chunk_payload(&truncated),
        Err(ChunkReadError::TruncatedPayload)
    ));
    let mut trailing = framed.clone();
    trailing.push(0);
    assert!(matches!(
        read_chunk_payload(&trailing),
        Err(ChunkReadError::TruncatedPayload)
    ));
    let mut flipped = framed.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xFF;
    assert!(matches!(
        read_chunk_payload(&flipped),
        Err(ChunkReadError::ChecksumMismatch)
    ));
}

fn parse(bytes: &[u8]) -> Result<UserTableInfo, UserConfError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("user.conf");
    std::fs::write(&path, bytes).expect("write");
    UserTableInfo::load_path(&path)
}

#[test]
fn user_conf_round_trips_and_conforms() {
    let versions = SystemVersions::for_this_build(7, 14);
    let conform = UserTableInfo::conform_to(&versions);
    assert!(conform.is_conform(&versions));

    let text = conform.to_text();
    assert_eq!(
        text,
        format!(
            "binary format version:7\nmodel data version:14\ndatabase format:{DEFAULT_STORE_DB_FORMAT}\nopen counter:0\n"
        )
    );
    let parsed = parse(text.as_bytes()).expect("parse");
    assert_eq!(parsed, conform);
    // `save` uses the pin's `fopen("w")` and `fprintf`: the host C
    // runtime writes CRLF on Windows and LF on Linux/macOS.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("user.conf");
    let filename = oxpinyin_input::c_path(&path).expect("c path");
    assert!(conform.save(&filename));
    let newline = oxpinyin_input::TEXT_NEWLINE;
    let expected = format!(
        "binary format version:7{newline}model data version:14{newline}database format:{DEFAULT_STORE_DB_FORMAT}{newline}open counter:0{newline}"
    );
    assert_eq!(std::fs::read(&path).expect("read"), expected.as_bytes());
    // Read the file the boundary actually wrote through its `fopen("r")`
    // and scanner, including native text-mode translation.
    let reloaded = UserTableInfo::load(&filename).expect("reload saved marker");
    assert_eq!(reloaded, conform);
    assert!(reloaded.is_conform(&versions));

    // A stale model version never conforms.
    let stale = UserTableInfo {
        model_data_version: 13,
        ..conform.clone()
    };
    assert!(!stale.is_conform(&versions));

    // A cross-backend marker never conforms — the ruling's mechanism.
    let mut other = conform.clone();
    if DEFAULT_STORE_DB_FORMAT == "KyotoCabinet" {
        other.database_format = Some("Tkrzw".to_owned());
    } else {
        other.database_format = Some("KyotoCabinet".to_owned());
    }
    assert!(!other.is_conform(&versions));
    // A token no upstream build ever emits never conforms — and is
    // not even parsable any more: the `database format:` directive
    // refuses it where upstream's mapper aborts (#590).
    assert!(!conform_with("NotADbmLibrary").is_conform(&versions));
    assert!(matches!(
        parse(b"binary format version:7\nmodel data version:14\ndatabase format:NotADbmLibrary\n"),
        Err(UserConfError::UnknownDatabaseFormat)
    ));

    // The open-counter rebuild limit.
    let tired = UserTableInfo {
        open_counter: OPEN_COUNTER_LIMIT + 1,
        ..conform.clone()
    };
    assert!(!tired.is_conform(&versions));
    let rested = UserTableInfo {
        open_counter: OPEN_COUNTER_LIMIT,
        ..conform
    };
    assert!(rested.is_conform(&versions));
    // get_open_counter reads a counter past the limit as 0, and any
    // other value as itself.
    assert_eq!(get_open_counter(OPEN_COUNTER_LIMIT), OPEN_COUNTER_LIMIT);
    assert_eq!(get_open_counter(OPEN_COUNTER_LIMIT + 1), 0);
    assert_eq!(get_open_counter(i32::MAX), 0);
    assert_eq!(get_open_counter(0), 0);
    assert_eq!(get_open_counter(-3), -3);
    assert_eq!(get_open_counter(i32::MIN), i32::MIN);

    // The marker conforms against what table.conf declares.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("table.conf"),
        b"binary format version:9\nmodel data version:20\nlambda parameter:1\n\
              source table format:pinyin\ndatabase format:Tkrzw\n",
    )
    .expect("write");
    let declared = crate::data::table_info::TableConf::load_from_dir(dir.path()).expect("parses");
    assert_eq!(
        SystemVersions::from_conf(&declared),
        SystemVersions {
            binary_format_version: 9,
            model_data_version: 20,
            database_format: "Tkrzw",
        }
    );

    // The third directive running off the end of the file leaves
    // `format` at UNKNOWN_FORMAT — the one outcome that is not
    // upstream's abort.
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

/// tests/storage/test_table_info.cpp:85-97 at 074a2219: a fresh marker
/// does not conform, `make_conform` does, and so does its saved copy.
#[test]
fn a_fresh_zero_version_marker_requires_conformance_for_either_source_format() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("table.conf");
    let profile = dir.path().join("user.conf");
    let filename = oxpinyin_input::c_path(&profile).unwrap();
    for (source, backend) in [("pinyin", "Tkrzw"), ("zhuyin", "KyotoCabinet")] {
        std::fs::write(&config, format!("binary format version:0\nmodel data version:0\nlambda parameter:0.5\nsource table format:{source}\ndatabase format:{backend}\n")).unwrap();
        let conf = crate::data::table_info::TableConf::load_from_dir(dir.path()).unwrap();
        let versions = SystemVersions::from_conf(&conf);
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
        assert_eq!(reloaded.database_format.as_deref(), Some(backend));
        assert!(reloaded.is_conform(&versions));
        assert_eq!(reloaded, conformed);
    }
    let directory = oxpinyin_input::c_path(dir.path()).unwrap();
    assert!(!UserTableInfo::load(&filename).unwrap().save(&directory));
    assert!(profile.is_file());
}

/// Each directive is one `fscanf`: a field stops where `%d` stops, a
/// whitespace directive crosses line breaks, a counter that does not
/// convert is 0 and the database token is the C string `%255s` stored.
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
            .expect("C string")
            .database_format
            .as_deref(),
        Some("BerkeleyDB")
    );
    assert_eq!(
        parse(good.replace('\n', " ").as_bytes())
            .expect("one line")
            .open_counter,
        3
    );
    let missing = good.replace("open counter:3\n", "");
    assert_eq!(parse(missing.as_bytes()).unwrap().open_counter, 0);
    // A directive that does not match leaves `str` unwritten: the abort
    // point, not `UNKNOWN_FORMAT`.
    assert!(matches!(
        parse(good.replace("database format:", "database:").as_bytes()),
        Err(UserConfError::UnknownDatabaseFormat)
    ));
}

#[test]
fn a_negative_counter_conforms() {
    let versions = SystemVersions::for_this_build(7, 14);
    let negative = UserTableInfo {
        open_counter: -3,
        ..UserTableInfo::conform_to(&versions)
    };
    assert!(negative.is_conform(&versions));
    assert!(negative.to_text().ends_with("open counter:-3\n"));
    assert_eq!(
        parse(negative.to_text().as_bytes()).expect("parse"),
        negative
    );
}

#[test]
fn an_out_of_int_version_is_written_signed() {
    // The pin reads `%d` into an `int` and writes `%d`; a value past
    // `INT_MAX` therefore appears as its two's-complement value.
    let versions = SystemVersions::for_this_build(u32::MAX, 0x8000_0000);
    let info = UserTableInfo::conform_to(&versions);
    let text = info.to_text();
    assert!(
        text.starts_with("binary format version:-1\nmodel data version:-2147483648\n"),
        "{text}"
    );
    assert_eq!(parse(text.as_bytes()).expect("parse"), info);
}

fn conform_with(token: &str) -> UserTableInfo {
    UserTableInfo {
        binary_format_version: 7,
        model_data_version: 14,
        database_format: Some(token.to_owned()),
        open_counter: 0,
    }
}

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

    // Hand-check the first two records' bytes against
    // append_record's layout.
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

#[test]
fn log_encode_rejects_a_payload_beyond_u16() {
    // 70_001 bytes: over the u16 length field, under nothing else.
    let huge = vec![0_u8; 70_001];
    assert!(
        encode_log_records(&[LogRecord::Add {
            token: 7,
            new_item: huge,
        }])
        .is_err(),
        "an oversized payload must be refused, not truncated"
    );
}

#[test]
fn the_walk_reports_why_it_ended() {
    let add = record(1, 0x0100_0001, &[2, 0, 1, 0]);
    let end = |bytes: &[u8]| {
        let mut walk = log_records(bytes);
        assert_eq!(walk.end(), None);
        walk.by_ref().for_each(drop);
        walk.end()
    };
    assert_eq!(end(&add), Some(LogEnd::Complete));
    assert_eq!(
        end(&[add.clone(), record(9, 0, &[])].concat()),
        Some(LogEnd::UnknownType)
    );
    // A type word cut short is `LOG_INVALID_RECORD`, `m_error` too.
    assert_eq!(
        end(&[add.clone(), vec![1, 0]].concat()),
        Some(LogEnd::UnknownType)
    );
    assert_eq!(
        end(&record(1, 0x0100_0001, &[6, 0, 1])),
        Some(LogEnd::OverRead)
    );
    assert_eq!(end(&record(4, 0x0100_0001, &[0, 0])), None);
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

#[test]
fn an_unknown_or_missing_type_ends_the_walk_after_the_records_before_it() {
    let add = record(1, 7, &[6, 0, 0, 0, 1, 0, 0, 0]);
    let add_record = LogRecord::Add {
        token: 7,
        new_item: vec![0, 0, 1, 0, 0, 0],
    };
    // `m_error` on an unknown type: the earlier record stays.
    let unknown = [add.clone(), record(9, 0, &[1, 2, 3])].concat();
    assert_eq!(owned(&unknown), vec![Ok(add_record.clone())]);
    assert_eq!(owned(&record(0, 0, &[])), vec![]);
    // Fewer than four bytes: the type is `LOG_INVALID_RECORD`.
    let short_head = [add, vec![1, 0, 0]].concat();
    assert_eq!(owned(&short_head), vec![Ok(add_record)]);
}

#[test]
fn a_field_past_the_end_reads_as_its_initial_value() {
    // A type without its token: `null_token`, then a length of 0.
    assert_eq!(
        owned(&1_u32.to_le_bytes()),
        vec![Ok(LogRecord::Add {
            token: 0,
            new_item: vec![],
        })]
    );
    assert_eq!(
        owned(&record(2, 5, &[3])),
        vec![Ok(LogRecord::Remove {
            token: 5,
            old_item: vec![],
        })]
    );
    // MODIFY reads the new length after a failed old one.
    assert_eq!(
        owned(&record(3, 5, &[])),
        vec![Ok(LogRecord::Modify {
            token: 5,
            old_item: vec![],
            new_item: vec![],
        })]
    );
    // A header without a length, or with runs shorter than a total.
    assert_eq!(
        owned(&record(4, 0, &[])),
        vec![Ok(LogRecord::ModifyHeader {
            old_total: 0,
            new_total: 0,
        })]
    );
    assert_eq!(
        owned(&record(4, 0, &[3, 0, 1, 2, 3, 4, 5, 6])),
        vec![Ok(LogRecord::ModifyHeader {
            old_total: 0,
            new_total: 0,
        })]
    );
}

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

#[test]
fn a_header_token_faults_where_the_header_is_read() {
    // Pin phrase_index_logger.h:202 checks the header token before its payload.
    assert!(matches!(
        owned(&record(4, 0, &[4, 0, 1, 0, 0, 0, 2, 0, 0, 0])).as_slice(),
        [Ok(LogRecord::ModifyHeader {
            old_total: 1,
            new_total: 2
        })]
    ));
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
    // A token cut short reads as `null_token`: no assert.
    assert_eq!(
        owned(
            &4_u32
                .to_le_bytes()
                .into_iter()
                .chain([1, 0, 0])
                .collect::<Vec<_>>()
        ),
        vec![Ok(LogRecord::ModifyHeader {
            old_total: 0,
            new_total: 0,
        })]
    );
    assert_eq!(
        owned(&record(4, 0x0100_0001, &[])),
        vec![Err(LogFault::HeaderToken(0x0100_0001))]
    );
}

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
