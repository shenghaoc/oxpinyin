//! The full-pinyin parser result before scan-matrix transformations.

use oxpinyin_core::{
    OptionBits,
    graph::{Edge, SegmentGraph},
    tool::{Key, Parse},
};

/// Reuse the C API's graph, fewest-key selection and trailing-apostrophe
/// computation (`Session::full_parsed_len`). The standalone parser does not
/// perform the later scan-matrix resplit/divide steps: the pin disables its
/// parser post-processing at `pinyin_parser2.cpp:377-379` (074a2219).
#[must_use]
pub fn parse_full(input: &[u8], options: u32) -> Parse {
    let options = OptionBits::from_bits(options);
    let Ok(graph) = SegmentGraph::build_with_options(input, options) else {
        return Parse::default();
    };
    let path = graph.fewest_keys(options.has_incomplete());
    Parse {
        consumed: super::apostrophe_extended(input, path.last().map_or(0, Edge::to)),
        keys: path
            .into_iter()
            .map(|edge| {
                Key::from_syllable(edge.key(), edge.tone(), edge.syllable_start()..edge.to())
            })
            .collect(),
    }
}
