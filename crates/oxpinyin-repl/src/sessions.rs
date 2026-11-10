//! The pin's C-API test programs (074a2219 tests/test_*.cpp).
//! The caller supplies the isolated data path; product call order is retained.

use oxpinyin_input::tool::{LineReader, pin_tests as output};
use pinyin_capi::tool::Driver;

/// tests/test_pinyin.cpp:34-101.
pub fn pinyin(data: &std::ffi::CStr) {
    let options = oxpinyin_core::PINYIN_INCOMPLETE
        | oxpinyin_core::PINYIN_CORRECT_ALL
        | oxpinyin_core::USE_DIVIDED_TABLE
        | oxpinyin_core::USE_RESPLIT_TABLE
        | oxpinyin_core::DYNAMIC_ADJUST;
    let mut driver = Driver::new(data, data, Some(options));
    let mut input = LineReader::stdin();
    loop {
        output::prefix_prompt();
        if !input.getline() {
            break;
        }
        let prefix = input.c_str().to_owned();
        output::pinyin_prompt();
        if !input.getline() || input.line() == b"quit" {
            break;
        }
        let len = driver.parse_full(input.c_str());
        driver.guess(Some(&prefix));
        // SORT_BY_PHRASE_LENGTH | SORT_BY_FREQUENCY (pinyin.h:54-60).
        driver.guess_candidates(0, 0x4 | 0x10);
        for cursor in 0..=len {
            output::auxiliary(driver.full_auxiliary(cursor).as_c_str());
        }
        let count = driver.candidate_count();
        for index in 0..count {
            output::word(driver.candidate(index));
        }
        output::newline();
        driver.train_reset();
        driver.save();
    }
}

/// tests/test_phrase.cpp:34-72.
pub fn phrase(data: &std::ffi::CStr) {
    let mut driver = Driver::new(data, data, None);
    let mut input = LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let count = driver.segment(input.c_str());
        for index in 0..count {
            let token = driver.phrase_token(index);
            if token == 0 {
                continue;
            }
            output::word(driver.phrase(token).as_c_str());
        }
        output::newline();
        driver.save();
    }
}

/// tests/test_chewing.cpp:34-72.
pub fn chewing(data: &std::ffi::CStr) {
    let mut driver = Driver::new(data, data, None);
    let mut input = LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        let len = driver.parse_chewing(input.c_str());
        driver.guess(None);
        {
            let text = driver.sentence();
            if let Some(text) = text.as_c_str() {
                output::sentence(text);
            }
        }
        for cursor in 0..=len {
            output::auxiliary(driver.chewing_auxiliary(cursor).as_c_str());
        }
        driver.train_reset();
        driver.save();
    }
}

/// tests/test_zhuyin.cpp:34-66, from the pin's --enable-libzhuyin build.
pub fn zhuyin(data: &std::ffi::CStr) {
    let mut driver = zhuyin_capi::tool::Driver::new(data, data);
    let mut input = LineReader::stdin();
    while input.getline() {
        if input.line() == b"quit" {
            break;
        }
        driver.parse_guess(input.c_str());
        {
            let text = driver.sentence();
            if let Some(text) = text.as_c_str() {
                output::sentence(text);
            }
        }
        driver.train_reset_save();
    }
}
