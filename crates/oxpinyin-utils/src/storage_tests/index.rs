//! tests/storage/test_phrase_index.cpp:9-122 at 074a2219.
use super::*;
use oxpinyin_core::ChewingKey;
use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_input::tool::phrase_tests as output;

/// Run the fixed synthetic assertions and the configured text-library checks.
pub fn main() {
    let key1 = present(ChewingKey::from_pinyin("cheng"));
    let key2 = present(ChewingKey::from_pinyin("shang"));
    let mut item = ChunkItem {
        phrase: vec![2],
        unigram: 0,
        prons: Vec::new(),
    };
    oxpinyin_user::add_system_reading(&mut item, &[key1.to_packed()], 100);
    oxpinyin_user::add_system_reading(&mut item, &[key2.to_packed()], 300);
    check(item.phrase.len() == 1);
    check(item.prons[0] == (vec![key1.to_packed()], 100));
    check(item.prons[1] == (vec![key2.to_packed()], 300));
    let mut index = Index::default();
    index.add(1, &item);
    output::possibility(true, present(index.possibility(1, &[key1])));
    check(item.unigram == 0);
    check(item.phrase == [2]);
    index = reload(&index);
    let start = tool::timer::record_time();
    for _ in 0..100000 {
        let item = present(index.item(1));
        check(item.unigram == 0 && item.prons.len() == 2 && item.phrase.len() == 1);
        check(index.possibility(1, &[key2]) == Some(0.75));
    }
    tool::timer::print_time(start, 100000);
    index.increase(1, &[key1], 200);
    check(index.possibility(1, &[key1]) == Some(0.5));
    let possibility = present(index.possibility(1, &[key1]));
    output::possibility(false, possibility);
    check(possibility == 0.5);

    let conf = conf();
    let mut indices = load_text(&conf);
    for index in indices.iter_mut().flatten() {
        index.compact();
    }
    for slot in [1, 2] {
        let index = indices[slot].take().unwrap_or_else(|| tool::abort());
        indices[slot] = Some(reload(&index));
    }
    for index in indices.iter_mut().flatten() {
        index.compact();
    }
    let index = indices[1].as_mut().unwrap_or_else(|| tool::abort());
    let item = present(index.item(16870553 & 0x00ff_ffff));
    check(item.phrase.len() == 14 && item.prons.len() == 1);
    let text = present(tool::ucs4_to_utf8(&item.phrase));
    output::text(&present(CString::new(text).ok()));
    index.add_unigram(16870553 & 0x00ff_ffff, 3);
    check(present(index.item(16870553 & 0x00ff_ffff)).unigram == 3);
    let item = present(index.item(16777222 & 0x00ff_ffff));
    check(item.phrase.len() == 1 && item.prons.len() == 2);
}
