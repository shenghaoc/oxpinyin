use super::{matrix_columns, remember_walk};
use crate::engine::{MatrixKey, parse_time_keys};
use crate::{OptionBits, graph::SegmentGraph};

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

// Basis: pinyin.cpp:3594–3606,3640–3645 at 074a2219.
#[test]
fn remember_user_input_refuses_a_short_path_before_the_preflight() {
    for (input, readings) in [("ni", ["ni", "hao"]), ("nihao", ["hao", "ni"])] {
        let matrix = full_matrix(input, OptionBits::from_bits_retain(0x28));
        assert_eq!(
            remembered_paths(input, matrix, &[&[readings[0]], &[readings[1]]]),
            (false, vec![])
        );
    }
}

// Basis: pinyin.cpp:3654–3659 at 074a2219.
#[test]
fn remember_user_input_adds_along_every_complete_path() {
    let matrix = full_matrix("nong", OptionBits::from_bits_retain(0x8 | (1 << 15)));
    assert_eq!(
        remembered_paths("nong", matrix, &[&["nong", "long"]]),
        (true, vec![vec!["nong".to_owned()], vec!["long".to_owned()]])
    );
}

// Basis: phonetic_key_matrix.h:92–100 and pinyin.cpp:3594–3606 at 074a2219.
#[test]
fn remember_user_input_writes_again_for_an_identical_complete_path() {
    let mut matrix = full_matrix("nong", OptionBits::from_bits_retain(0x8 | (1 << 15)));
    matrix.push(matrix[0]);
    assert_eq!(
        remembered_paths("nong", matrix, &[&["nong", "long"]]),
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
