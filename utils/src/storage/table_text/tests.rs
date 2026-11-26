use super::split_line;

fn fields(line: &str) -> Option<Vec<String>> {
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(0);
    split_line(&bytes).map(|fields| {
        fields
            .iter()
            .map(|field| String::from_utf8(field.to_vec()).unwrap())
            .collect()
    })
}

#[test]
fn four_fields_split_on_space_and_tab() {
    assert_eq!(
        fields("a\t\u{5440} 16777218 104\n"),
        Some(vec![
            "a".into(),
            "\u{5440}".into(),
            "16777218".into(),
            "104".into()
        ])
    );
}

#[test]
fn short_blank_and_leading_separator_lines_are_skipped() {
    assert_eq!(fields(""), None);
    assert_eq!(fields("a b 1"), None);
    assert_eq!(fields(" \t \n"), None);
    assert_eq!(fields("\r\n"), None);
}

#[test]
fn extra_fields_are_ignored() {
    assert_eq!(fields("a b 1 2 trailing fields here\n").unwrap().len(), 4);
}
