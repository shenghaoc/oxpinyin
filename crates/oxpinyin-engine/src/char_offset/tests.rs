use super::{matrix_columns, remember_walk};
use crate::{MatrixKey, parse_time_keys};
use oxpinyin_core::{DoublePinyinParser, OptionBits, graph::SegmentGraph};

fn full_matrix(input: &str, options: OptionBits) -> Vec<MatrixKey> {
    let graph = SegmentGraph::build_with_options(input.as_bytes(), options).unwrap();
    assert_eq!(graph.consumed(), input.len());
    let keys: Vec<_> = graph
        .fewest_keys(true)
        .iter()
        .map(|edge| MatrixKey::new(edge.key(), edge.tone(), edge.syllable_start(), edge.to()))
        .collect();
    parse_time_keys(&keys, input.len(), options, true)
}

fn double_nong_matrix() -> Vec<MatrixKey> {
    let options = OptionBits::from_bits_retain(0x8 | (1 << 15));
    let parse = DoublePinyinParser::new().parse_with_options(b"ns", options);
    assert_eq!(parse.consumed(), 2);
    let keys: Vec<_> = parse
        .keys()
        .iter()
        .map(|key| MatrixKey::new(key.key(), key.tone(), key.start(), key.end()))
        .collect();
    parse_time_keys(&keys, 2, options, false)
}

// The callback's accepted readings are the original W3 items (你 and 好),
// or the two imported readings of 弄. The C module tests retain the actual
// dictionary callback, preflight order and writes of counts 3 and 6.
fn remembered_paths(
    input: &str,
    keys: Vec<MatrixKey>,
    readings: &[&[&str]],
) -> (bool, Vec<Vec<String>>) {
    let columns = matrix_columns(input.as_bytes(), input.len(), keys, true).unwrap();
    let mut paths = Vec::new();
    let found = remember_walk::<std::convert::Infallible>(
        &columns,
        readings.len(),
        &mut |key, index| Ok(readings[index].contains(&key.key().pinyin_spelling())),
        &mut |path| {
            paths.push(path.iter().map(|key| key.key().pinyin_string()).collect());
            Ok(true)
        },
    )
    .unwrap();
    (found, paths)
}

/// A one-key path for 你好 never calls the add/preflight callback
/// (`pinyin.cpp:3600-3601` at 074a2219).
#[test]
fn remember_user_input_refuses_a_short_path_before_the_preflight() {
    assert_eq!(
        remembered_paths(
            "ni",
            full_matrix("ni", OptionBits::from_bits_retain(0x28)),
            &[&["ni"], &["hao"]]
        ),
        (false, vec![])
    );
}

/// 好 does not pronounce ni (`pinyin.cpp:3640-3645` at 074a2219).
#[test]
fn remember_user_input_refuses_a_path_the_item_does_not_pronounce() {
    assert_eq!(
        remembered_paths(
            "nihao",
            full_matrix("nihao", OptionBits::from_bits_retain(0x28)),
            &[&["hao"], &["ni"]]
        ),
        (false, vec![])
    );
}

/// The walk uses the retained parse matrix after L_N is cleared by its caller.
#[test]
fn remember_user_input_keeps_the_parse_time_fuzzy_alternatives() {
    let matrix = double_nong_matrix();
    assert_eq!(
        remembered_paths("ns", matrix, &[&["nong", "long"]]),
        (true, vec![vec!["nong".to_owned()], vec!["long".to_owned()]])
    );
}

/// Every complete path calls add (`pinyin.cpp:3654-3659` at 074a2219).
#[test]
fn remember_user_input_adds_along_every_complete_path() {
    let matrix = full_matrix("nong", OptionBits::from_bits_retain(0x8 | (1 << 15)));
    assert_eq!(
        remembered_paths("nong", matrix, &[&["nong", "long"]]),
        (true, vec![vec!["nong".to_owned()], vec!["long".to_owned()]])
    );
}

/// PhoneticTable::append retains identical entries (phonetic_key_matrix.h:92-100).
/// Each complete path still calls add (`pinyin.cpp:3594-3606` at 074a2219).
#[test]
fn remember_user_input_writes_again_for_an_identical_complete_path() {
    let mut matrix = double_nong_matrix();
    matrix.push(matrix[0]);
    assert_eq!(
        remembered_paths("ns", matrix, &[&["nong", "long"]]),
        (
            true,
            vec![
                vec!["nong".to_owned()],
                vec!["long".to_owned()],
                vec!["nong".to_owned()]
            ]
        )
    );
}
