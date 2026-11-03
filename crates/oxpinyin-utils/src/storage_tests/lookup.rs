//! tests/storage/test_matrix.cpp and tests/lookup/test_*_lookup.cpp, 074a2219.
use super::*;
use oxpinyin_core::tool::Key;
use oxpinyin_core::{ChewingKey, PINYIN_AMB_ALL, PINYIN_CORRECT_ALL, PINYIN_INCOMPLETE, USE_TONE};
use oxpinyin_engine::tool::{parse_full, physical_matrix, prepare_lookup};
use oxpinyin_input::tool::lookup_tests as output;
use oxpinyin_runtime::tool::Lookup;

fn load(phonetic: bool, bigram: bool) -> Lookup {
    let conf = conf();
    Lookup::open(&conf, phonetic, bigram).unwrap_or_else(|error| {
        if matches!(
            error.cause,
            oxpinyin_data::phrase_library::LibraryError::Row(_)
        ) {
            tool::abort();
        }
        let name = error.name.as_deref().and_then(oxpinyin_input::c_path);
        tool::phrase_tests::open_failed(name.as_deref());
        tool::exit(tool::ENOENT);
    })
}
fn dump(matrix: &[Vec<Key>]) {
    for (position, column) in matrix.iter().enumerate() {
        if column.is_empty() {
            continue;
        }
        output::column(position);
        for key in column {
            output::key(
                &present(CString::new(key.key.pinyin_string()).ok()),
                key.span.start as u16,
                key.span.end as u16,
            );
        }
    }
}
fn continued(data: &Lookup, matrix: &[Vec<Key>], start: usize, end: usize) -> bool {
    fn recurse(
        data: &Lookup,
        matrix: &[Vec<Key>],
        start: usize,
        end: usize,
        keys: &mut Vec<ChewingKey>,
        longest: &mut usize,
    ) -> bool {
        if start > end {
            return false;
        }
        if start == end {
            return keys.len() <= 16 && (keys.is_empty() || data.key_exists(keys));
        }
        let column = &matrix[start];
        check(!column.is_empty());
        let mut found = false;
        for key in column {
            if key.key == ChewingKey::default() {
                check(column.len() == 1);
                return recurse(data, matrix, key.span.end, end, keys, longest);
            }
            keys.push(key.key);
            *longest = (*longest).max(key.span.end);
            found |= recurse(data, matrix, key.span.end, end, keys, longest);
            keys.pop();
        }
        found
    }
    if matrix[start].is_empty() {
        return false;
    }
    if matrix[end].is_empty() {
        return true;
    }
    let mut longest = 0;
    recurse(data, matrix, start, end, &mut Vec::new(), &mut longest) || longest > end
}
/// Timed parse, matrix construction and complete search-index trace.
pub fn matrix_main() {
    let data = load(true, false);
    let options = PINYIN_CORRECT_ALL | PINYIN_AMB_ALL | PINYIN_INCOMPLETE;
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let mut parsed = Default::default();
        let mut matrix = Vec::new();
        let start = tool::timer::record_time();
        for _ in 0..1000 {
            parsed = parse_full(input.line(), options);
            matrix = physical_matrix(&parsed, options);
        }
        tool::timer::print_time(start, 1000);
        output::parsed(parsed.consumed, parsed.keys.len());
        dump(&matrix);
        for start in 0..matrix.len() {
            for end in start + 1..matrix.len() {
                output::search(start, end);
                if !continued(&data, &matrix, start, end) {
                    break;
                }
            }
        }
    }
}
/// Matrix dump and timed existing PhoneticLookup<2,3> results.
pub fn pinyin_main() {
    let data = load(true, true);
    let options = USE_TONE | PINYIN_CORRECT_ALL | PINYIN_INCOMPLETE;
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let Ok(prepared) = prepare_lookup(input.line(), options) else {
            continue;
        };
        if prepared.parsed.keys.is_empty() {
            continue;
        }
        dump(&physical_matrix(&prepared.parsed, options));
        let start = tool::timer::record_time();
        let mut rows = Vec::new();
        for _ in 0..100 {
            rows = data.phonetic(&prepared).unwrap_or_else(|_| tool::abort());
        }
        tool::timer::print_time(start, 100);
        for row in rows {
            for (position, token) in row.tokens {
                output::token(position, token);
            }
            output::newline();
            let text = present(tool::ucs4_to_utf8(
                &row.text.outward_words().collect::<Vec<_>>(),
            ));
            output::sentence(&present(CString::new(text).ok()));
        }
    }
}
/// Raw-text PhraseLookup with token-prefixed newline-separated output.
pub fn phrase_main() {
    let _ = tool::set_locale_from_environment();
    let data = load(false, true);
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let line = present(
            CString::new(
                input
                    .line()
                    .split(|&byte| byte == 0)
                    .next()
                    .unwrap_or_default(),
            )
            .ok(),
        );
        let count = tool::utf8_strlen(&line);
        let (words, written) = tool::utf8_to_ucs4(&line);
        if written != count {
            output::non_ucs4();
            continue;
        }
        let words = words.unwrap_or_default();
        let tokens = data.phrase(&words).unwrap_or_else(|_| tool::abort());
        let mut entries = Vec::new();
        for token in tokens.into_iter().filter(|token| token.value() != 0) {
            let phrase = present(data.text(token.value()));
            let text = present(tool::ucs4_to_utf8(
                &phrase.outward_words().collect::<Vec<_>>(),
            ));
            let mut entry = format!("{} ", token.value()).into_bytes();
            entry.extend(text);
            entries.push(entry);
        }
        if entries.is_empty() {
            output::unsegmentable();
        } else {
            output::sentence(&present(CString::new(entries.join(&b'\n')).ok()));
        }
    }
}
