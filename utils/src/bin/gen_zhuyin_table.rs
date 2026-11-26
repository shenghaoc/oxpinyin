//! Pin-compatible zhuyin source table generator.
fn main() {
    oxpinyin_utils::gen_zhuyin_table::main(std::env::args_os());
}
