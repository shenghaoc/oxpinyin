//! tests/storage/test_flexible_ngram.cpp:25-139 at 074a2219.
use super::*;
use oxpinyin_store::{
    AttachMode, DefaultFlexibleBigram, FixedRecord, FlexibleGramSpec, SingleGram,
};

#[derive(Clone)]
struct Word(u32);

impl FixedRecord for Word {
    const LEN: usize = 4;
    fn zeroed() -> Self {
        Self(0)
    }
    fn encode(&self) -> Vec<u8> {
        self.0.to_ne_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        let bytes = bytes
            .try_into()
            .map_err(|_| StoreError::Backend("test gram word is not four bytes".into()))?;
        Ok(Self(u32::from_ne_bytes(bytes)))
    }
}

struct Test;
impl FlexibleGramSpec for Test {
    const MAGIC_NUMBER: [u8; 4] = *b"TEST";
    type MagicHeader = Word;
    type ArrayHeader = Word;
    type ArrayItem = Word;
}

fn print_gram(gram: &SingleGram<Word, Word>) {
    let mut array = Vec::new();
    check(gram.search(0, 8, &mut array));
    for entry in array {
        output::item(entry.token, entry.item.0);
    }
}

/// Execute the program under its fixed name ([`crate::fixed_tmp`]).
pub fn main() {
    let mut single = SingleGram::<Word, Word>::new();
    single.set_header(Word(16));
    for (token, freq) in [(2, 1), (6, 2), (4, 4), (3, 8), (1, 16), (3, 32)] {
        if single.get_array_item(token).is_some() {
            check(single.set_array_item(token, Word(freq)));
        } else {
            check(single.insert_array_item(token, Word(freq)));
        }
    }
    check(present(single.get_array_item(3)).0 == 32);
    output::message(Message::Separator);
    print_gram(&single);
    check(single.header().0 == 16);

    let bigram = required(DefaultFlexibleBigram::<Test>::attach(
        Some(crate::fixed_tmp::path("training.db").as_path()),
        AttachMode::ReadWriteCreate,
    ));
    check(bigram.attached());
    let _ = bigram.store(1, &single);
    check(single.insert_array_item(5, Word(8)));
    check(single.remove_array_item(1).is_some());
    single.set_header(Word(32));
    output::new_header(single.header().0);
    let _ = bigram.store(2, &single);
    for predecessor in 1..=2 {
        output::message(Message::Separator);
        print_gram(&present(required(bigram.load(predecessor))));
    }
    let items = present(bigram.get_all_items());
    output::message(Message::Items);
    for item in items {
        output::token(item);
    }
    output::message(Message::Magic);
    let _ = bigram.set_magic_header(&Word(16));
    let magic = present(required(bigram.get_magic_header()));
    check(magic.0 == 16);
    output::magic(magic.0);
    output::message(Message::Array);
    for token in 1..=2 {
        output::header(token, present(required(bigram.get_array_header(token))).0);
    }
    let _ = bigram.set_array_header(1, &Word(1));
    output::message(Message::Array);
    for token in 1..=2 {
        output::header(token, present(required(bigram.get_array_header(token))).0);
    }
    for predecessor in 1..=2 {
        output::message(Message::Separator);
        print_gram(&present(required(bigram.load(predecessor))));
    }
    check(required(bigram.remove(1)));
    let items = present(bigram.get_all_items());
    output::message(Message::Items);
    for item in items {
        output::token(item);
    }
}
