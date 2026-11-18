#[test]
fn null_chunk_refusal_logs_once() {
    let logs = crate::test_support::LogCapture::new();
    let warning = oxpinyin::data::table_info::TableConfSite::NullSystemFile.warning();
    assert!(!super::addon_load_answer(Err(warning)));
    assert_eq!(
        logs.take_with_messages(),
        [(
            (Some("libpinyin".to_owned()), glib_sys::G_LOG_LEVEL_WARNING),
            warning.to_owned(),
        )]
    );
}
