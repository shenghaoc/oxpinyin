//! `utils/storage/gen_zhuyin_table.cpp` at libpinyin 074a2219.
//! Rows are `phrase frequency zhuyin`; the tree, stable sort and token
//! numbering are shared with `gen_pinyin_table` (:90-98, :197-237, :241-334).
use crate::gen_pinyin_table::{Table, entries};
use oxpinyin_core::parse_zhuyin_direct;
use oxpinyin_core::{ChewingKey, FORCE_TONE, USE_TONE};
use oxpinyin_input::tool::gen_pinyin_table as common;
use oxpinyin_input::tool::gen_zhuyin_table as edge;
use oxpinyin_input::tool::{self, LineReader, OptionValue, OutputStream};
use std::ffi::{CString, OsString};

/// `ZhuyinDirectParser2::parse` (:187-188; zhuyin_parser2.cpp:858-958).
fn parse_keys(bytes: &[u8]) -> Vec<ChewingKey> {
    // The direct parser stops at the first unparseable segment. Keep a valid
    // prefix before malformed UTF-8; never replace bytes with U+FFFD.
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default(),
    };
    // A malformed suffix within a segment invalidates that entire key.
    let text = if text.len() == bytes.len() {
        text
    } else {
        text.rfind([' ', '\'']).map_or("", |end| &text[..=end])
    };
    parse_zhuyin_direct(text, USE_TONE | FORCE_TONE)
        .into_iter()
        .filter_map(|(key, tone)| {
            ChewingKey::from_pinyin(key.text()).map(|key| key.with_tone(tone))
        })
        .collect()
}

/// `std::ifstream`, `getline`, `g_strsplit_set(..., " ", 3)`, then `atoi`
/// before the field-count check (:140-164). No locale call in this main.
fn feed_file(table: &mut Table, filename: &CString) {
    let mut input = LineReader::open(filename);
    if !input.is_open() {
        common::report_open_failed(filename);
        tool::exit(tool::ENOENT);
    }
    while input.getline() {
        let line = input.c_str();
        let fields = tool::strsplit_set(line, c" ", 3);
        let Some(frequency) = fields.get(1) else {
            edge::missing_frequency(line);
            tool::exit(1);
        };
        let frequency =
            tool::atoi(&CString::new(frequency.clone()).unwrap_or_default()).cast_unsigned();
        let [phrase, _, pinyin] = fields.as_slice() else {
            edge::wrong_line(line);
            continue;
        };
        let phrase = CString::new(phrase.clone()).unwrap_or_default();
        let pinyin = CString::new(pinyin.clone()).unwrap_or_default();
        // Pad the GLib scan against a truncated final UTF-8 lead byte. The
        // pin reads past its allocation there (class b); valid text is exact.
        let mut padded = phrase.as_bytes_with_nul().to_vec();
        padded.extend_from_slice(&[0; 8]);
        let phrase = std::ffi::CStr::from_bytes_until_nul(&padded).unwrap_or_default();
        let length = usize::try_from(tool::utf8_strlen(phrase)).unwrap_or(0);
        table.feed_parts(phrase, &pinyin, frequency, length, parse_keys);
    }
}

/// `main` (:101-138), with the pin's option strings (including "pinyin").
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let Ok(parsed) = tool::parse_options(c"- generate pinyin table", &entries(), args) else {
        tool::exit(tool::EINVAL);
    };
    let phrase_index = if let OptionValue::Int(value) = parsed.values[0] {
        value
    } else {
        0
    };
    let outputfile = if let OptionValue::Filename(Some(name)) = &parsed.values[1] {
        CString::new(name.clone()).unwrap_or_default()
    } else {
        CString::default()
    };
    let mut table = Table::default();
    for arg in parsed.args.iter().skip(1) {
        feed_file(&mut table, &CString::new(arg.clone()).unwrap_or_default());
    }
    common::print_nnodes(i32::try_from(table.nnodes()).unwrap_or(i32::MAX));
    let sorted = table.sorted();
    let Some(output) = OutputStream::open_write(&outputfile) else {
        common::report_write_failed(&outputfile);
        tool::exit(tool::ENOENT);
    };
    sorted.write_with(
        &output,
        phrase_index,
        ChewingKey::zhuyin_string,
        edge::unconverted_phrase,
    );
    output.close();
}
