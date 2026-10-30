//! `gen_pinyin_table` (libpinyin 074a2219
//! `utils/storage/gen_pinyin_table.cpp`): reads `phrase pinyin freq` rows
//! from each file named on the command line and writes the
//! `pinyin\tphrase\ttoken\tfreq` table, phrases numbered by length, then by
//! their first reading's keys.
//!
//! The phrase tree is `g_chewing_tree` (`:85-94`, `:101`): ordered by
//! length, then by `memcmp` over the phrase's UCS-4 words in the host's
//! byte order, which decides the order of phrases whose first readings are
//! equal, because `g_array_sort_with_data` is stable (GLib's documented
//! guarantee since 2.32). That byte order makes the order a
//! native-host result (constitution §6).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString, OsString, c_int};

use oxpinyin_core::graph::SegmentGraph;
use oxpinyin_core::{ChewingKey, OptionBits, PINYIN_CORRECT_ALL, USE_TONE};
use oxpinyin_data::table_entries::exact_compare2;
use oxpinyin_input::tool::gen_pinyin_table::{self as edge, TableInput};
use oxpinyin_input::tool::{
    self, OptionArg, OptionEntry, OptionValue, OutputStream, ucs4_to_utf8, utf8_to_ucs4,
};

/// `MAX_PHRASE_LENGTH` (`src/include/novel_types.h:119`).
pub const MAX_PHRASE_LENGTH: usize = 16;

/// `PHRASE_INDEX_LIBRARY_MASK` and `PHRASE_MASK`
/// (`src/include/novel_types.h:40-41`).
const LIBRARY_MASK: u32 = 0x0F00_0000;
const PHRASE_MASK: u32 = 0x00FF_FFFF;

/// The pin's `static GOptionEntry entries[]` (`:38-43`): `phrase_index`
/// starts at 0 and `outputfile` at `"temp.out"`.
pub(crate) fn entries() -> [OptionEntry; 2] {
    [
        OptionEntry {
            long_name: c"phraseindex",
            short_name: b't',
            flags: 0,
            arg: OptionArg::Int(0),
            description: Some(c"phrase index"),
            arg_description: None,
        },
        OptionEntry {
            long_name: c"outputfile",
            short_name: b'o',
            flags: 0,
            arg: OptionArg::Filename(Some(c"temp.out")),
            description: Some(c"output filename"),
            arg_description: None,
        },
    ]
}

/// A key of `g_chewing_tree`: `phrase_item` (`:58-61`).
#[derive(Clone, Debug, Eq, PartialEq)]
struct PhraseItem {
    /// `g_utf8_strlen(phrase, -1)`.
    length: usize,
    /// `g_utf8_to_ucs4(phrase, …)`.
    uniphrase: Uniphrase,
}

/// What `g_utf8_to_ucs4` made of a phrase.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Uniphrase {
    /// Its NULL, for a phrase that is not valid UTF-8: the scanned bytes,
    /// kept to name it (class (b), see [`Sorted::write`]).
    Unconverted(Vec<u8>),
    /// The UCS-4 words.
    Converted(Vec<u32>),
}

impl Ord for PhraseItem {
    /// `phrase_item_compare` (`:85-94`): length, then `memcmp` of the
    /// UCS-4 words as the host stores them. The pin's `memcmp` reads
    /// through a NULL `uniphrase`; here an unconverted phrase sorts before
    /// the converted ones of its length, by its bytes.
    fn cmp(&self, other: &Self) -> Ordering {
        self.length
            .cmp(&other.length)
            .then_with(|| match (&self.uniphrase, &other.uniphrase) {
                (Uniphrase::Unconverted(lhs), Uniphrase::Unconverted(rhs)) => lhs.cmp(rhs),
                (Uniphrase::Unconverted(_), Uniphrase::Converted(_)) => Ordering::Less,
                (Uniphrase::Converted(_), Uniphrase::Unconverted(_)) => Ordering::Greater,
                (Uniphrase::Converted(lhs), Uniphrase::Converted(rhs)) => {
                    let bytes = |words: &[u32]| {
                        words
                            .iter()
                            .flat_map(|word| word.to_ne_bytes())
                            .collect::<Vec<u8>>()
                    };
                    bytes(lhs).cmp(&bytes(rhs))
                }
            })
    }
}

impl PartialOrd for PhraseItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `chewing_and_freq_item` (`:63-67`); the key rests are never read.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Reading {
    keys: Vec<ChewingKey>,
    freq: u32,
}

/// `g_chewing_tree`.
#[derive(Debug, Default)]
pub struct Table {
    tree: BTreeMap<PhraseItem, Vec<Reading>>,
}

/// `FullPinyinParser2::parse` under `PINYIN_CORRECT_ALL | USE_TONE`
/// (`:178-182`): the keys of the longest parsable prefix, fewest keys, as
/// the import iterator parses (`pinyin.cpp:629-637`).
fn parse_keys(pinyin: &[u8]) -> Vec<ChewingKey> {
    let options = OptionBits::from_bits(PINYIN_CORRECT_ALL | USE_TONE);
    let Ok(graph) = SegmentGraph::build_with_options(pinyin, options) else {
        return Vec::new();
    };
    graph
        .fewest_keys(false)
        .iter()
        .map(|edge| {
            ChewingKey::from_pinyin(edge.key().text()).map(|key| key.with_tone(edge.tone()))
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

impl Table {
    /// `g_tree_nnodes(g_chewing_tree)`.
    #[must_use]
    pub fn nnodes(&self) -> usize {
        self.tree.len()
    }

    /// `feed_line` (`:160-232`) for the row `input` holds. A phrase
    /// `g_utf8_to_ucs4` refuses (`:174`) is kept unconverted, as the pin
    /// keeps its NULL `uniphrase`, so the run reaches the output as the
    /// pin's does.
    pub fn feed_line(&mut self, input: &TableInput) {
        self.feed_parts(
            input.phrase(),
            input.pinyin(),
            input.freq(),
            usize::try_from(input.phrase_utf8_strlen()).unwrap_or(0),
            parse_keys,
        );
    }

    /// Shared phrase/reading tree; each caller supplies its own pin parser.
    pub(crate) fn feed_parts(
        &mut self,
        phrase: &CStr,
        pinyin: &CStr,
        freq: u32,
        length: usize,
        parse: fn(&[u8]) -> Vec<ChewingKey>,
    ) {
        if length >= MAX_PHRASE_LENGTH {
            edge::report_too_long(phrase, pinyin, freq);
            return;
        }
        let (uniphrase, _) = utf8_to_ucs4(phrase);
        let keys = parse(pinyin.to_bytes());
        if keys.len() != length {
            edge::report_invalid_pinyin(phrase, pinyin, freq);
            return;
        }
        let uniphrase = uniphrase.map_or_else(
            || Uniphrase::Unconverted(phrase.to_bytes().to_vec()),
            Uniphrase::Converted,
        );
        let item = PhraseItem { length, uniphrase };
        let reading = Reading { keys, freq };
        let Some(readings) = self.tree.get_mut(&item) else {
            self.tree.insert(item, vec![reading]);
            return;
        };
        let mut found = false;
        for current in readings.iter_mut() {
            if exact_compare2(&reading.keys, &current.keys).is_eq() {
                edge::report_duplicate(phrase, pinyin, freq);
                current.freq = current.freq.wrapping_add(reading.freq);
                found = true;
            }
        }
        if !found {
            readings.push(reading);
        }
    }

    /// `store_one_item` over the tree in order (`:235-242`), then each
    /// length's stable sort by its first reading's keys (`:124-126`,
    /// `:245-258`). A phrase of length zero goes to the NULL
    /// `g_item_array[0]` and is not stored.
    #[must_use]
    pub fn sorted(&self) -> Sorted<'_> {
        let mut arrays: Vec<Vec<(&PhraseItem, &Vec<Reading>)>> =
            vec![Vec::new(); MAX_PHRASE_LENGTH + 1];
        for (item, readings) in &self.tree {
            if item.length == 0 {
                edge::store_into_null_array();
                continue;
            }
            arrays[item.length].push((item, readings));
        }
        for array in arrays.iter_mut().skip(1) {
            array.sort_by(|lhs, rhs| exact_compare2(&lhs.1[0].keys, &rhs.1[0].keys));
        }
        Sorted(arrays)
    }
}

/// `g_item_array` after the sort: one array per phrase length.
#[derive(Debug)]
pub struct Sorted<'a>(Vec<Vec<(&'a PhraseItem, &'a Vec<Reading>)>>);

impl Sorted<'_> {
    /// `gen_phrase_file`'s rows (`:269-321`) into `output`, which the
    /// caller opened and closes. The library bits are `(phrase_index << 24)
    /// & PHRASE_INDEX_LIBRARY_MASK` with the shift wrapping, where the
    /// pin's `int` shift overflows for an index of 128 and more.
    ///
    /// An unconverted phrase is where the pin's `g_ucs4_to_utf8`
    /// (`:280-281`) reads through its NULL `uniphrase` and dies: undefined
    /// behaviour, class (b) (`docs/findings/upstream-divergences.md`,
    /// "gen_pinyin_table invalid UTF-8 phrase"). The output file is already
    /// open there (`:261`), as the pin's is; oxpinyin names the phrase on
    /// `stderr` and exits 1 through `exit`, which flushes what was
    /// written.
    pub fn write(&self, output: &OutputStream, phrase_index: c_int) {
        self.write_with(
            output,
            phrase_index,
            ChewingKey::pinyin_string,
            edge::report_unconverted_phrase,
        );
    }

    /// The generators share numbering/output calls; only the key renderer and
    /// the class (b) refusal name differ (gen_zhuyin_table.cpp:286-325).
    pub(crate) fn write_with(
        &self,
        output: &OutputStream,
        phrase_index: c_int,
        render: fn(ChewingKey) -> String,
        refuse: fn(&CStr),
    ) {
        let library = phrase_index.cast_unsigned().wrapping_shl(24) & LIBRARY_MASK;
        let mut token: u32 = 1;
        for array in self.0.iter().skip(1) {
            for (item, readings) in array {
                let uniphrase = match &item.uniphrase {
                    Uniphrase::Converted(words) => words,
                    Uniphrase::Unconverted(bytes) => {
                        refuse(&CString::new(bytes.clone()).unwrap_or_default());
                        tool::exit(1);
                    }
                };
                let phrase_str = ucs4_to_utf8(uniphrase).and_then(|bytes| CString::new(bytes).ok());
                for reading in readings.iter() {
                    let pinyins: Vec<String> =
                        reading.keys.iter().map(|&key| render(key)).collect();
                    let pinyin_str = CString::new(pinyins.join("'")).unwrap_or_default();
                    // `if (freq < 3) freq = 3;` (`:311-312`).
                    let freq = reading.freq.max(3);
                    let value = (library | (token & PHRASE_MASK)).cast_signed();
                    edge::print_row(output, &pinyin_str, phrase_str.as_deref(), value, freq);
                }
                token = token.wrapping_add(1);
            }
        }
    }
}

/// `feed_file` (`:134-157`).
fn feed_file(table: &mut Table, filename: &CString) {
    let Some(mut input) = TableInput::open(filename) else {
        edge::report_open_failed(filename);
        tool::exit(tool::enoent());
    };
    while !input.eof() {
        if input.scan() != 3 {
            continue;
        }
        if input.eof() {
            break;
        }
        table.feed_line(&input);
    }
    input.close();
}

/// `main` (`:97-131`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let Ok(parsed) = tool::parse_options(c"- generate pinyin table", &entries(), args) else {
        tool::exit(tool::einval());
    };
    let phrase_index = match parsed.values[0] {
        OptionValue::Int(value) => value,
        _ => 0,
    };
    let outputfile = match &parsed.values[1] {
        OptionValue::Filename(Some(name)) => CString::new(name.clone()).unwrap_or_default(),
        _ => CString::default(),
    };
    let mut table = Table::default();
    for arg in parsed.args.iter().skip(1) {
        feed_file(&mut table, &CString::new(arg.clone()).unwrap_or_default());
    }
    edge::print_nnodes(c_int::try_from(table.nnodes()).unwrap_or(c_int::MAX));
    let sorted = table.sorted();
    let Some(output) = OutputStream::open_write(&outputfile) else {
        edge::report_write_failed(&outputfile);
        tool::exit(tool::enoent());
    };
    sorted.write(&output, phrase_index);
    output.close();
}
