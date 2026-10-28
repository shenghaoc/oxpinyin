//! Diagnostic: open a system data directory and print the outcome.
//!
//! `cargo run -p oxpinyin-runtime --example open_probe -- <system_dir>`
//!
//! The C ABI's `pinyin_init` deliberately collapses every open failure to
//! NULL; this probe surfaces the underlying [`oxpinyin_runtime::OpenError`]
//! — which open step failed, naming the offending file — for a directory
//! that refuses to open.

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: open_probe <system_dir>");
        std::process::exit(2);
    };
    match oxpinyin_runtime::Runtime::open(std::path::Path::new(&dir), None) {
        Ok(runtime) => {
            let dict = runtime.dict();
            println!(
                "open OK: {} phrase-index items",
                oxpinyin_core::Dictionary::phrase_index_item_count(&dict).unwrap_or(0)
            );
        }
        Err(error) => {
            eprintln!("open FAILED: {error}");
            std::process::exit(1);
        }
    }
}
