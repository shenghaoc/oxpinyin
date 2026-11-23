use super::*;

const STOCK: &str = "binary format version:7\nmodel data version:14\n\
        lambda parameter:0.312699\n\nsource table format:pinyin\n\
        database format:BerkeleyDB\n\n\
        default RESERVED NULL NULL NULL NOT_USED\n\
        default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n\
        default USER_DICTIONARY NULL NULL user.bin USER_FILE\n\n\
        addon 4 art.table art.bin NULL DICTIONARY\n";

fn parse_bytes(bytes: &[u8]) -> Result<TableConf, TableConfError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("table.conf");
    std::fs::write(&path, bytes).expect("write");
    TableConf::load_path(&path)
}

fn parse(text: &str) -> Result<TableConf, TableConfError> {
    parse_bytes(text.as_bytes())
}

// 074a2219 pinyin.cpp:377-392 and memory_chunk.h:424-434 decide how a null row name is used.
#[test]
fn null_chunk_names_follow_directory_size_decision() {
    let user = parse(&STOCK.replace("gb_char.dbin SYSTEM_FILE", "NULL SYSTEM_FILE")).unwrap();
    use crate::data::phrase_library::directory_size_reads_as_a_chunk;
    let null_user_file = |conf: &TableConf, reads: bool| {
        conf.default_row_steps_with(reads)
            .into_iter()
            .any(|(index, step)| {
                index == 1 && matches!(step, DefaultRowStep::System { null_user_file, .. } if null_user_file)
            })
    };
    for size in [Ok(7), Err(std::io::ErrorKind::Other.into())] {
        assert!(!null_user_file(
            &user,
            directory_size_reads_as_a_chunk(size)
        ));
    }
    assert!(null_user_file(
        &user,
        directory_size_reads_as_a_chunk(Ok(8))
    ));
    let user_file = parse(&STOCK.replace("user.bin USER_FILE", "NULL USER_FILE")).unwrap();
    let row_seven = |reads| {
        user_file
            .default_row_steps_with(reads)
            .into_iter()
            .find_map(|(index, step)| (index == 7).then_some(step))
    };
    assert_eq!(
        row_seven(directory_size_reads_as_a_chunk(Ok(7))),
        Some(DefaultRowStep::User)
    );
    assert_eq!(
        row_seven(directory_size_reads_as_a_chunk(Ok(8))),
        Some(DefaultRowStep::Stop(TableConfSite::NullUserFile))
    );
    let system = parse(&STOCK.replace("gb_char.bin gb_char.dbin", "NULL gb_char.dbin")).unwrap();
    for reads in [false, true] {
        assert_eq!(
            system
                .default_row_steps_with(reads)
                .first()
                .map(|row| row.1),
            Some(DefaultRowStep::Stop(TableConfSite::NullSystemFile))
        );
    }
}

// 074a2219 table_info.cpp:194-294 reads the headers and writes indexed rows.
#[test]
fn rows_fill_their_slots() {
    let conf = parse(STOCK).expect("parses");
    assert_eq!(conf.binary_format_version(), 7);
    assert_eq!(conf.model_data_version(), 14);
    assert_eq!(conf.lambda_f32(), 0.312_699);
    assert_eq!(conf.database_format(), DatabaseFormat::BerkeleyDb);
    let system: Vec<_> = conf.system_libraries().map(|(index, _)| index).collect();
    assert_eq!(system, [1]);
    let user: Vec<_> = conf.user_libraries().map(|(index, _)| index).collect();
    assert_eq!(user, [7]);
    assert_eq!(
        conf.addon_row(4)
            .and_then(|row| row.system.as_ref().map(|name| name.display().into_owned())),
        Some("art.bin".to_owned())
    );
    assert_eq!(conf.default_row(2), Some(&TableRow::unused()));
}

// 074a2219 table_info.cpp:145-157,254-287 maps TSI and GB to the same slot.
#[test]
fn a_later_row_replaces_an_earlier_one_and_tsi_is_gb() {
    let text = format!("{STOCK}default TSI_DICTIONARY x.table x.bin x.dbin NOT_USED\n");
    let conf = parse(&text).expect("parses");
    assert_eq!(conf.system_libraries().count(), 0);
}

// 074a2219 tests/storage/test_table_info.cpp:26-83 specifies indexed row types and names.
#[test]
fn addon_replacement_and_null_columns_keep_their_indexed_types() {
    let text = format!(
        "{STOCK}default GB_DICTIONARY NULL NULL NULL SYSTEM_FILE\naddon 15 old.table old.bin old.user SYSTEM_FILE\naddon 15 new.table new.bin NULL DICTIONARY\naddon 3 NULL NULL NULL USER_FILE\n"
    );
    let conf = parse(&text).unwrap();
    let system = conf.default_row(1).unwrap();
    assert_eq!(system.file_type, FileType::SystemFile);
    assert_eq!(
        (&system.table, &system.system, &system.user),
        (&None, &None, &None)
    );
    let addon = conf.addon_row(15).unwrap();
    assert_eq!(addon.file_type, FileType::Dictionary);
    assert_eq!(addon.table, Some(FileName::from_bytes(b"new.table")));
    assert_eq!(addon.system, Some(FileName::from_bytes(b"new.bin")));
    assert_eq!(addon.user, None);
    let user = conf.addon_row(3).unwrap();
    assert_eq!(user.file_type, FileType::UserFile);
    assert_eq!(user.user, None);
}

// 074a2219 table_info.cpp:159-161,276 narrows atoi before checking the index.
#[test]
fn addon_indexes_are_atoi_cut_to_a_byte() {
    for (word, slot) in [("256", 0), ("abc", 0), ("4x", 4), ("259", 3)] {
        let text = format!("{STOCK}addon {word} w.table w.bin NULL DICTIONARY\n");
        let conf = parse(&text).expect(word);
        assert_eq!(
            conf.addon_row(slot)
                .and_then(|row| row.system.as_ref())
                .map(|file| file.display().into_owned()),
            Some("w.bin".to_owned()),
            "{word}"
        );
    }
    let text = format!("{STOCK}addon 16 w.table w.bin NULL DICTIONARY\n");
    assert_eq!(
        parse(&text).unwrap_err(),
        TableConfError::Abort(TableConfSite::AddonIndex)
    );
}

// 074a2219 table_info.cpp:254-287 checks the sixth scanned word as the file type.
#[test]
fn rows_are_six_words_across_line_breaks() {
    let text =
        format!("{STOCK}default GBK_DICTIONARY a b c\ndefault RESERVED NULL NULL NULL NOT_USED\n");
    assert_eq!(
        parse(&text).unwrap_err(),
        TableConfError::Abort(TableConfSite::FileType)
    );
}

// 074a2219 table_info.cpp:110-176,254-287 distinguishes the conversion abort sites.
#[test]
fn each_abort_site_is_named() {
    let cases = [
        (
            "source table format:pinyin",
            "source table format:foo",
            TableConfSite::SourceFormat,
        ),
        (
            "database format:BerkeleyDB",
            "database format:LMDB",
            TableConfSite::DatabaseFormat,
        ),
    ];
    for (from, to, site) in cases {
        let text = STOCK.replace(from, to);
        assert_eq!(parse(&text).unwrap_err(), TableConfError::Abort(site));
    }
    for (row, site) in [
        ("foo GB_DICTIONARY a b c SYSTEM_FILE", TableConfSite::Target),
        (
            "default FOO_DICTIONARY a b c SYSTEM_FILE",
            TableConfSite::DefaultIndex,
        ),
        (
            "default GB_DICTIONARY a b c FOO_FILE",
            TableConfSite::FileType,
        ),
    ] {
        let text = format!("{STOCK}{row}\n");
        assert_eq!(
            parse(&text).unwrap_err(),
            TableConfError::Abort(site),
            "{row}"
        );
    }
}

// 074a2219 table_info.cpp:209-245 distinguishes failed numeric and format assignments.
#[test]
fn header_misses_are_the_ordinary_false_or_the_uninitialised_read() {
    for (from, to, expected) in [
        ("version:7", "version:x", TableConfError::Header),
        ("version:7", "version:7x", TableConfError::Header),
        ("lambda parameter:0.312699\n", "", TableConfError::Header),
        (
            "source table format:pinyin\n",
            "",
            TableConfError::UninitialisedSourceFormat,
        ),
        (
            "database format:BerkeleyDB\n",
            "",
            TableConfError::Abort(TableConfSite::DatabaseFormat),
        ),
    ] {
        assert_eq!(parse(&STOCK.replace(from, to)).unwrap_err(), expected);
    }
}

// Row 83 accepts the scanned f32 lambda, including out-of-range values.
#[test]
fn lambda_is_the_scanned_float() {
    for (text, expected) in [("0.5", 0.5_f32), ("2", 2.0), ("-0.5", -0.5), ("5e-1", 0.5)] {
        let conf = parse(&STOCK.replace("0.312699", text)).expect(text);
        assert_eq!(conf.lambda_f32(), expected, "{text}");
    }
    assert!(
        parse(&STOCK.replace("0.312699", "nan"))
            .expect("nan")
            .lambda_f32()
            .is_nan()
    );
}

// 074a2219 table_info.cpp:199-202 refuses an unopenable configuration.
#[test]
fn a_missing_file_is_unopenable() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        TableConf::load_from_dir(dir.path()).unwrap_err(),
        TableConfError::Unopenable
    );
}

#[cfg(unix)]
// Rust FileName preserves the scanned Unix filename bytes.
#[test]
fn file_name_columns_keep_their_raw_bytes() {
    use std::os::unix::ffi::OsStrExt as _;
    let mut bytes = STOCK.as_bytes().to_vec();
    let at = bytes
        .windows(12)
        .position(|window| window == b"gb_char.dbin")
        .expect("marker");
    bytes.splice(at..at + 12, *b"gb\xff.dbin");
    let conf = parse_bytes(&bytes).expect("parses");
    let user = conf
        .default_row(1)
        .and_then(|row| row.user.as_ref())
        .expect("user file");
    assert_eq!(AsRef::<OsStr>::as_ref(user).as_bytes(), b"gb\xff.dbin");
}

// Frozen 074a2219 data/table.conf.in:8-28 defines the stock row layout.
#[test]
fn the_stock_layout_is_the_shipped_table_conf() {
    let stock = TableConf::stock(0.312_699);
    let libraries: Vec<_> = stock.system_libraries().map(|(index, _)| index).collect();
    assert_eq!(libraries, [1, 2, 3, 4]);
    let user: Vec<_> = stock.user_libraries().map(|(index, _)| index).collect();
    assert_eq!(user, [5, 6, 7]);
    assert_eq!(stock.addon_rows().count(), 12);
}
