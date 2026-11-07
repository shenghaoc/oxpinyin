//! `remember_user_input`'s signed count (#540).

use crate::facade;
use crate::readers::c;
use crate::support::{Kit, take_text};

#[derive(Debug, PartialEq)]
pub struct Remembered {
    pub remember: bool,
    pub rows: Vec<(Option<String>, Option<String>, i32)>,
}

/// Remembers 你好世界 with `count` after parsing its input, then exports library 7.
pub fn remember_count(k: &mut Kit, count: i32) -> Remembered {
    let inst = k.inst();
    facade::parse_more_full_pinyins(inst, c("nihaoshijie").as_ptr());
    let remember = facade::remember_user_input(inst, c("你好世界").as_ptr(), count);
    let it = facade::begin_get_phrases(k.ctx(), 7);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        let (mut phrase, mut pinyin, mut n) = (std::ptr::null_mut(), std::ptr::null_mut(), 0_i32);
        facade::iterator_get_next_phrase(it, &raw mut phrase, &raw mut pinyin, &raw mut n);
        rows.push((take_text(phrase), take_text(pinyin), n));
    }
    Remembered { remember, rows }
}
