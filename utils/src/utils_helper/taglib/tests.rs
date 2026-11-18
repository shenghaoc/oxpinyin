use super::*;

fn split(line: &CStr) -> Result<Option<Vec<String>>, Undefined> {
    split_line(line).map(|tokens| {
        tokens.map(|tokens| {
            tokens
                .iter()
                .map(|token| token.to_string_lossy().into_owned())
                .collect()
        })
    })
}

#[test]
fn split_line_keeps_quotes_escapes_and_empty_tokens() {
    assert_eq!(
        split(c"\\data model \"k mixture model\" count 1").unwrap(),
        Some(vec![
            "\\data".into(),
            "model".into(),
            "k mixture model".into(),
            "count".into(),
            "1".into()
        ])
    );
    // The escape stays in the token; the closing quote is stepped over.
    assert_eq!(
        split(c"\"a\\\"b\"c d").unwrap(),
        Some(vec!["a\\\"b".into(), "c".into(), "d".into()])
    );
    // A character neither space nor graph ends a token, and starts an
    // empty one (U+200B ZERO WIDTH SPACE).
    assert_eq!(
        split(c"a\xe2\x80\x8bb").unwrap(),
        Some(vec!["a".into(), "b".into()])
    );
    assert_eq!(
        split(c"\xe2\x80\x8bb").unwrap(),
        Some(vec![String::new(), "b".into()])
    );
    assert_eq!(split(c"   ").unwrap(), Some(vec![]));
    assert_eq!(split(c"\"open").unwrap(), Some(vec!["open".into()]));
}

#[test]
fn split_line_answers_the_pins_null_and_its_undefined_reads() {
    assert_eq!(split(c"\"a\\").unwrap(), None);
    assert_eq!(split(c"a \xe4"), Err(Undefined::PastLineEnd));
    assert_eq!(split(c"\"\xe4"), Err(Undefined::PastLineEnd));
    // Inside the line, a truncated sequence only makes odd tokens.
    // The lead byte's step of three lands on `b`.
    assert_eq!(
        split(c"\xe4 abc").unwrap(),
        Some(vec![String::new(), "bc".into()])
    );
}

/// POLICY-ACCEPTED
/// (`docs/findings/upstream-divergences.md#g_utf8_next_char-from-a-byte-that-leads-no-sequence`):
/// a byte that leads no UTF-8 sequence steps one byte. The pin steps
/// GLib's table, 5 at `0xF8..=0xFB` and 6 at `0xFC..=0xFD`, so it
/// answers `["", "e"]`, `["", "f"]`, and reads past the NUL of
/// `"\xf8ab"`.
#[test]
fn split_line_steps_one_byte_from_a_byte_that_leads_no_sequence() {
    assert_eq!(
        split(c"\xf8abcd e").unwrap(),
        Some(vec![String::new(), "abcd".into(), "e".into()])
    );
    assert_eq!(
        split(c"\xfcabcde f").unwrap(),
        Some(vec![String::new(), "abcde".into(), "f".into()])
    );
    assert_eq!(
        split(c"\xf8ab").unwrap(),
        Some(vec![String::new(), "ab".into()])
    );
    // Control: 0xF5 leads a four-byte form on both sides, and its step
    // of four lands on the space.
    assert_eq!(
        split(c"\xf5abc d").unwrap(),
        Some(vec![String::new(), "d".into()])
    );
}

fn tags() -> TagLib {
    let mut taglib = TagLib::init();
    assert!(taglib.add_tag(1, c"\\item", 2, c"freq", c"count"));
    taglib
}

#[test]
fn read_walks_values_then_pairs() {
    let taglib = tags();
    let mut line_type = 0;
    let mut values = Vec::new();
    let mut required = Required::default();
    let read = taglib.read(
        c"\\item 10 甲 count 2 freq 3 freq 4",
        &mut line_type,
        &mut values,
        &mut required,
    );
    assert_eq!(read, Ok(true));
    assert_eq!(line_type, 1);
    assert_eq!(values, [c"10".to_owned(), c"甲".to_owned()]);
    assert_eq!(required[&b"freq".to_vec()].as_c_str(), c"4");
    assert_eq!(
        taglib.read(c"\\item 10", &mut line_type, &mut values, &mut required),
        Ok(false)
    );
    assert_eq!(
        taglib.read(c"\\other", &mut line_type, &mut values, &mut required),
        Ok(false)
    );
    assert_eq!(
        taglib.read(c"", &mut line_type, &mut values, &mut required),
        Err(Undefined::NoToken)
    );
}

#[test]
fn add_tag_refuses_a_duplicate_type_or_tag_in_the_top_state() {
    let mut taglib = tags();
    assert!(!taglib.add_tag(1, c"\\other", 0, c"", c""));
    assert!(!taglib.add_tag(2, c"\\item", 0, c"", c""));
    taglib.push_state();
    assert!(!taglib.add_tag(2, c"\\item", 0, c"", c""));
    taglib.pop_state();
    assert!(taglib.add_tag(2, c"\\other", 0, c"", c""));
}
