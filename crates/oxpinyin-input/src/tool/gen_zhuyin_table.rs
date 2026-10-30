//! Additional edge calls of `gen_zhuyin_table.cpp` at 074a2219.
use crate::native_tool;
use std::ffi::CStr;

/// `fprintf(stderr, "wrong line format:%s\n", linebuf)` (:156-159).
pub fn wrong_line(line: &CStr) {
    native_tool::gzt_wrong_line(line);
}

/// Normal refusal where `atoi(strs[1])` would dereference NULL (:153).
pub fn missing_frequency(line: &CStr) {
    native_tool::gzt_refuse(c"missing frequency", line);
}

/// Normal refusal where `g_ucs4_to_utf8` would read a NULL phrase (:286-287).
pub fn unconverted_phrase(phrase: &CStr) {
    native_tool::gzt_refuse(c"invalid UTF-8 phrase", phrase);
}
