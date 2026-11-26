//! `gen_pinyin_table` (libpinyin 074a2219
//! `utils/storage/gen_pinyin_table.cpp`).

fn main() {
    oxpinyin_utils::gen_pinyin_table::main(std::env::args_os());
}
