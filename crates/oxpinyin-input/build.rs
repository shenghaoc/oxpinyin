//! Compiles the C shim against the target's own headers, so `FILE`,
//! `LC_NUMERIC` and the scanner formats are the C library's, not guessed.
//! The `tools` feature needs no build step: its calls go through the `libc`
//! and `glib-sys` crates.

fn main() {
    println!("cargo:rerun-if-changed=native/config.c");
    println!("cargo:rerun-if-changed=native/diagnostic.c");
    cc::Build::new()
        .file("native/config.c")
        .file("native/diagnostic.c")
        .warnings(true)
        .flag_if_supported("-Wformat=2")
        .flag_if_supported("-fvisibility=hidden")
        .compile("oxpinyin_input_native");
}
