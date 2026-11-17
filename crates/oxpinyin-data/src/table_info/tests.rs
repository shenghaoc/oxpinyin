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

// Pin memory_chunk.h:424-434: a null user filename reads the user directory.
#[test]
fn null_chunk_names_follow_directory_size_decision() {
    let user = parse(&STOCK.replace("gb_char.dbin SYSTEM_FILE", "NULL SYSTEM_FILE")).unwrap();
    use crate::phrase_library::directory_size_reads_as_a_chunk;
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

#[test]
fn rows_fill_their_slots() {
    let conf = parse(STOCK).expect("parses");
    assert_eq!(conf.binary_format_version(), 7);
    assert_eq!(conf.model_data_version(), 14);
    assert_eq!(conf.lambda_f32(), PINNED_LAMBDA);
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

#[test]
fn a_later_row_replaces_an_earlier_one_and_tsi_is_gb() {
    let text = format!("{STOCK}default TSI_DICTIONARY x.table x.bin x.dbin NOT_USED\n");
    let conf = parse(&text).expect("parses");
    assert_eq!(conf.system_libraries().count(), 0);
}

/// tests/storage/test_table_info.cpp:26-83 at 074a2219: each default
/// and addon row by its index, file type and NULL-able names.
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

/// `to_index_of_addon_tables` is `atoi` cut to a `guint8`, and only the
/// assert after it refuses (`table_info.cpp:159-161`, `:276`).
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
    for word in ["-1", "16", "99", "255"] {
        let text = format!("{STOCK}addon {word} w.table w.bin NULL DICTIONARY\n");
        assert_eq!(
            parse(&text).unwrap_err(),
            TableConfError::Abort(TableConfSite::AddonIndex),
            "{word}"
        );
    }
}

/// Fewer than six words before the end of input is a `continue`
/// (`:259-260`); a comment is three ordinary words.
#[test]
fn short_trailing_rows_are_dropped_not_errors() {
    let text = format!("{STOCK}default GB_DICTIONARY a b c\n");
    assert!(parse(&text).is_ok());
    let text = format!("{STOCK}# a comment\n");
    assert!(parse(&text).is_ok());
}

/// The scanner groups words six at a time, wherever the lines break.
#[test]
fn rows_are_six_words_across_line_breaks() {
    let text = format!("{STOCK}default GBK_DICTIONARY gbk.table\ngbk.bin gbk.dbin\nSYSTEM_FILE\n");
    let conf = parse(&text).expect("parses");
    let system: Vec<_> = conf.system_libraries().map(|(index, _)| index).collect();
    assert_eq!(system, [1, 2]);
    // A short row takes the first word of the next line as its sixth.
    let text =
        format!("{STOCK}default GBK_DICTIONARY a b c\ndefault RESERVED NULL NULL NULL NOT_USED\n");
    assert_eq!(
        parse(&text).unwrap_err(),
        TableConfError::Abort(TableConfSite::FileType)
    );
}

/// `%255s` stores 255 bytes and leaves the rest for the next word.
#[test]
fn a_long_word_is_cut_into_two() {
    let name = "a".repeat(300);
    let text = format!("{STOCK}addon 3 art.table {name} NULL DICTIONARY\n");
    assert_eq!(
        parse(&text).unwrap_err(),
        TableConfError::Abort(TableConfSite::FileType)
    );
}

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

#[test]
fn header_misses_are_the_ordinary_false_or_the_uninitialised_read() {
    let no_lambda = STOCK.replace("lambda parameter:0.312699\n", "");
    assert_eq!(parse(&no_lambda).unwrap_err(), TableConfError::Header);
    let no_source = STOCK.replace("source table format:pinyin\n", "");
    assert_eq!(
        parse(&no_source).unwrap_err(),
        TableConfError::UninitialisedSourceFormat
    );
    let no_database = STOCK.replace("database format:BerkeleyDB\n", "");
    assert_eq!(
        parse(&no_database).unwrap_err(),
        TableConfError::Abort(TableConfSite::DatabaseFormat)
    );
}

/// λ is the host scanner's `%f` result, used directly as a `gfloat`.
#[test]
fn lambda_is_the_scanned_float() {
    for (text, expected) in [
        ("0.5", 0.5_f32),
        ("2", 2.0),
        ("-0.5", -0.5),
        ("5e-1", 0.5),
        ("0x1p-1", 0.5),
        ("1e-30", 1e-30),
    ] {
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

/// Each header directive is one `fscanf`: whitespace directives cross
/// line breaks, and a field stops where the conversion stops.
#[test]
fn headers_follow_the_scanner() {
    assert!(parse(&STOCK.replace('\n', " ")).is_ok());
    assert!(
        parse(&STOCK.replace("source table format:pinyin", "source table format:\npinyin")).is_ok()
    );
    assert_eq!(
        parse(&STOCK.replace("version:7", "version:7x")).unwrap_err(),
        TableConfError::Header
    );
    assert_eq!(
        parse(&STOCK.replace("version:7", "version:x")).unwrap_err(),
        TableConfError::Header
    );
}

#[test]
fn a_missing_file_is_unopenable() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        TableConf::load_from_dir(dir.path()).unwrap_err(),
        TableConfError::Unopenable
    );
}

// Unix only: byte-string file names.
#[cfg(unix)]
#[test]
fn file_name_columns_keep_their_raw_bytes() {
    use std::os::unix::ffi::OsStrExt as _;
    // The word holds the byte 0xFF, which is not UTF-8.
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

#[test]
fn the_stock_layout_is_the_shipped_table_conf() {
    let stock = TableConf::stock();
    let libraries: Vec<_> = stock.system_libraries().map(|(index, _)| index).collect();
    assert_eq!(libraries, [1, 2, 3, 4]);
    let user: Vec<_> = stock.user_libraries().map(|(index, _)| index).collect();
    assert_eq!(user, [5, 6, 7]);
    assert_eq!(stock.addon_rows().count(), 12);
}
