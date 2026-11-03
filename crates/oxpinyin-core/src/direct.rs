//! Shared separator walk for the pin's direct parsers.

use core::ops::Range;

/// `pinyin_parser2.cpp:675-715`, `zhuyin_parser2.cpp:914-960` at 074a2219.
/// Append successful whole keys with their raw spans; consume the following
/// separator run even when the next key fails. No second key parse is needed
/// to recover the return length or key rests.
pub(crate) fn parse<K>(
    bytes: &[u8],
    mut parse_key: impl FnMut(Range<usize>) -> Option<K>,
    mut append: impl FnMut(K, Range<usize>),
) -> usize {
    let separator = |byte: u8| byte == b' ' || byte == b'\'';
    let mut cur = 0;
    while cur < bytes.len() {
        let next = bytes[cur..]
            .iter()
            .position(|&byte| separator(byte))
            .map_or(bytes.len(), |offset| cur + offset);
        let Some(key) = parse_key(cur..next) else {
            break;
        };
        append(key, cur..next);
        cur = bytes[next..]
            .iter()
            .position(|&byte| !separator(byte))
            .map_or(bytes.len(), |offset| next + offset);
    }
    cur
}
