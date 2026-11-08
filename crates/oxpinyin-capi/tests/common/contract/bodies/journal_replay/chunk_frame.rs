//! Chunk frames whose physical length runs one byte past the declared one (#1576).

use super::{close, item_with, modify_record, system_items, unigram_j};
use crate::facade;
use crate::fixtures::{chunk, modify_header, write_user_conf};
use crate::readers::{import_rows, tokens_of};
use crate::support::{J, Kit};

/// Replays one matching MODIFY from an exact or one-byte-long frame.
pub(super) fn journal(k: &mut Kit, trailing: bool) -> J {
    let library = system_items(&k.data, "gb_char.bin");
    let slot = library.first(|item| item[0] == 1 && item[1] == 1);
    let token = 0x0100_0000 | slot;
    let item = &library.items[&slot];
    let record = modify_record(token, item, &item_with(item, Some(42), None));
    let mut frame = chunk(&[modify_header(0, library.total, library.total), record].concat());
    if trailing {
        frame.push(0);
    }
    std::fs::write(k.user.join("gb_char.dbin"), frame).expect("gb_char.dbin");
    write_user_conf(k);
    obj! { "ctx" => !k.ctx().is_null(), "unigram" => unigram_j(k, token) }
}

/// Saves a user item, alters only the frame's physical length, and reopens.
pub(super) fn user(k: &mut Kit, trailing: bool) -> J {
    let reading = if facade::ZHUYIN {
        "ㄉㄚˇ ㄉㄚˇ"
    } else {
        "da3'da3"
    };
    let phrase = "龘龘";
    assert_eq!(import_rows(k, 7, &[(phrase, reading, 5)]), [true]);
    let token = tokens_of(k, phrase)
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a user token");
    assert!(facade::save(k.ctx()));
    close(k);
    if trailing {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(k.user.join("user.bin"))
            .expect("user.bin");
        file.write_all(&[0]).expect("append a byte");
    }
    write_user_conf(k);
    obj! { "ctx" => !k.ctx().is_null(), "unigram" => unigram_j(k, token) }
}
