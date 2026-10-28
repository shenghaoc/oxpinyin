//! Compiles the C shim against the target's own headers, so `FILE`,
//! `LC_NUMERIC` and the scanner formats are the C library's, not guessed.

fn main() {
    println!("cargo:rerun-if-changed=native/config.c");
    cc::Build::new()
        .file("native/config.c")
        .warnings(true)
        .flag_if_supported("-Wformat=2")
        .flag_if_supported("-fvisibility=hidden")
        .compile("oxpinyin_input_native");
}
