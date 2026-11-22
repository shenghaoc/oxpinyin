//! tests/storage/test_matrix.cpp and tests/lookup/test_*_lookup.cpp, 074a2219.
use std::cell::Cell;
use std::ffi::CString;
use std::path::{Path, PathBuf};

use oxpinyin::OptionBits;
use oxpinyin::data::{BigramTables, PhraseLibraries, SystemTables, TableConf};
use oxpinyin::engine::EngineError;
use oxpinyin::engine::tool::{LookupRow, PreparedLookup, lookup, phrase_lookup, prepare_lookup};
use oxpinyin::input::tool::{self, lookup_tests as output};
use oxpinyin::parser::ChewingKey;
use oxpinyin::tool::{Key, Parse};
use oxpinyin::vocab::{PhraseText, PhraseToken, SyllableKey};

use crate::{conf, parser::parse_full, present};

fn check(value: bool) {
    if !value {
        tool::abort();
    }
}

fn load(data: &Path, phonetic: bool, bigram: bool) -> Lookup {
    let conf = conf(data);
    Lookup::open(data, &conf, phonetic, bigram).unwrap_or_else(|error| {
        let name = error.name.as_deref().and_then(oxpinyin::input::c_path);
        tool::phrase_tests::open_failed(name.as_deref());
        tool::exit(tool::ENOENT);
    })
}
fn dump(matrix: impl IntoIterator<Item = (usize, Key)>) {
    let mut previous = None;
    for (position, key) in matrix {
        if previous != Some(position) {
            output::column(position);
            previous = Some(position);
        }
        output::key(
            &present(CString::new(key.key.pinyin_string()).ok()),
            key.span.start as u16,
            key.span.end as u16,
        );
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
/// Parse, matrix construction and complete search-index trace.
pub fn matrix_main(data: &Path) {
    let data = load(data, true, false);
    let options =
        OptionBits::PINYIN_CORRECT_ALL | OptionBits::PINYIN_AMB_ALL | OptionBits::PINYIN_INCOMPLETE;
    let mut input = tool::LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let parsed = parse_full(input.line(), options);
        let matrix = physical_matrix(&parsed, options);
        output::parsed(parsed.consumed, parsed.keys.len());
        dump(
            matrix.iter().enumerate().flat_map(|(position, column)| {
                column.iter().cloned().map(move |key| (position, key))
            }),
        );
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
/// Matrix dump and existing PhoneticLookup<2,3> results.
pub fn pinyin_main(data: &Path) {
    let data = load(data, true, true);
    let options =
        OptionBits::USE_TONE | OptionBits::PINYIN_CORRECT_ALL | OptionBits::PINYIN_INCOMPLETE;
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
        dump(prepared.matrix_keys());
        let rows = data.phonetic(&prepared).unwrap_or_else(|_| tool::abort());
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
pub fn phrase_main(data: &Path) {
    let _ = tool::set_locale_from_environment();
    let data = load(data, false, true);
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
        for token in tokens.into_iter().filter(|token| *token != 0) {
            let phrase = present(data.text(token));
            let text = present(tool::ucs4_to_utf8(
                &phrase.outward_words().collect::<Vec<_>>(),
            ));
            let mut entry = format!("{} ", token).into_bytes();
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

// PhoneticKeyMatrix construction, phonetic_key_matrix.cpp:30–83. The utility does not
// enable resplit or divided-table processing here.
/// Builds physical matrix columns from the supplied key spans. Gaps and the terminal
/// position have zero keys. For a nonempty parse, use ordered byte spans inside the
/// consumed prefix and leave room for the terminal column.
///
/// # Panics
///
/// Panics when a key starts past `parsed.consumed`.
/// A nonempty parse also requires a representable column count of `consumed + 1`.
#[must_use]
fn physical_matrix(parsed: &Parse, options: OptionBits) -> Vec<Vec<Key>> {
    if parsed.keys.is_empty() {
        return Vec::new();
    }
    let mut columns = vec![Vec::new(); parsed.consumed + 1];
    for key in &parsed.keys {
        columns[key.span.start].push(key.clone());
    }
    columns[parsed.consumed].push(Key {
        key: ChewingKey::default(),
        span: parsed.consumed..parsed.consumed + 1,
    });
    for (i, key) in parsed.keys.iter().enumerate() {
        let next = parsed
            .keys
            .get(i + 1)
            .map_or(parsed.consumed, |key| key.span.start);
        for (fill, column) in columns.iter_mut().enumerate().take(next).skip(key.span.end) {
            column.push(Key {
                key: ChewingKey::default(),
                span: fill..fill + 1,
            });
        }
    }
    for column in &mut columns {
        let Some(origin) = column.first().cloned() else {
            continue;
        };
        let spelling = origin.key.with_tone(0).pinyin_string();
        if let Some(key) = SyllableKey::from_canonical_text(&spelling) {
            for alternate in key.fuzzy_alternatives(options) {
                column.push(Key {
                    key: alternate.chewing_key().with_tone(origin.key.tone),
                    span: origin.span.clone(),
                });
            }
        }
    }
    columns
}

/// Failure of the test program's required chunk load.
struct LoadError {
    /// The raw configured filename (`None` means the pin's NULL column).
    name: Option<PathBuf>,
}

/// A single native index, heap-loaded phrase facade and optional system bigram.
struct Lookup {
    tables: SystemTables,
    libraries: PhraseLibraries,
    model: Option<BigramTables>,
}
impl Lookup {
    // Native index or model construction failures terminate at the utility boundary. The
    // pin's NULL user-bigram attach fails before native I/O.
    /// Opens the selected phonetic or phrase index and loads the system libraries named in
    /// the configuration. Set `bigram` to attach the system model for decoding.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] for the first required phrase chunk that could not be loaded.
    fn open(
        data: &Path,
        conf: &TableConf,
        phonetic: bool,
        bigram: bool,
    ) -> Result<Self, LoadError> {
        let path = data.join(if phonetic {
            "pinyin_index.bin"
        } else {
            "phrase_index.bin"
        });
        let tables = SystemTables::for_lookup(&path, phonetic).unwrap_or_else(|_| tool::abort());
        let mut libraries = PhraseLibraries::empty();
        for (index, row) in conf.system_libraries() {
            let name = row.system.as_ref().map(|name| PathBuf::from(name.as_ref()));
            let path = oxpinyin::input::build_path(
                data,
                name.as_deref().unwrap_or(Path::new("")).as_os_str(),
            );
            libraries
                .load_heap(index, &path)
                .map_err(|_| LoadError { name })?;
        }
        let model = bigram.then(|| {
            BigramTables::attach(&data.join("bigram.db"), conf.lambda_f32())
                .unwrap_or_else(|_| tool::abort())
        });
        Ok(Self {
            tables,
            libraries,
            model,
        })
    }
    // The matrix driver reads SEARCH_CONTINUED and aborts on a failed table query.
    /// Reports whether the index has a continuation row for `keys`.
    #[must_use]
    fn key_exists(&self, keys: &[ChewingKey]) -> bool {
        self.tables
            .key_exists(keys)
            .unwrap_or_else(|_| tool::abort())
    }
    /// Runs n-best lookup with no user overlay. Returns an empty result when
    /// the lookup was opened with bigrams disabled.
    ///
    /// # Errors
    /// Returns decoder/model errors.
    fn phonetic(&self, prepared: &PreparedLookup) -> Result<Vec<LookupRow>, EngineError> {
        let Some(model) = &self.model else {
            return Ok(Vec::new());
        };
        lookup(
            prepared,
            &self.tables.read(&self.libraries),
            &model.read(&self.libraries, &Cell::new(0)),
        )
    }
    /// Runs phrase segmentation with no user overlay. Returns an empty result
    /// when the lookup was opened with bigrams disabled.
    ///
    /// # Errors
    /// Returns decoder/model errors.
    fn phrase(&self, words: &[u32]) -> Result<Vec<PhraseToken>, EngineError> {
        let Some(model) = &self.model else {
            return Ok(Vec::new());
        };
        phrase_lookup(
            words,
            &self.tables.read(&self.libraries),
            &model.read(&self.libraries, &Cell::new(0)),
        )
    }
    /// Resolve a result token to its native stored phrase.
    #[must_use]
    fn text(&self, token: u32) -> Option<PhraseText> {
        self.libraries.phrase_text(token)
    }
}

#[cfg(test)]
mod tests;
