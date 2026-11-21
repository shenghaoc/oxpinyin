//! Generate the test consumer's C bindings and allocation register from the public interfaces.

#[cfg(feature = "abi-test")]
fn main() {
    use std::env;
    use std::fmt::Write;
    use std::fs;
    use std::path::PathBuf;

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let glib = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("glib-2.0")
        .expect("GLib headers");
    let include_dirs: Vec<_> = glib
        .include_paths
        .iter()
        .map(|dir| format!("-I{}", dir.display()))
        .collect();
    for (facade, dir, library) in [
        ("pinyin", "../../capi", "Pinyin"),
        ("zhuyin", "../../zhuyin-capi", "Zhuyin"),
    ] {
        let header = format!("{facade}.h");
        let wrapper = format!(
            "#include \"{header}\"\nconstexpr guint32 OXPINYIN_USER_TOKEN = PHRASE_INDEX_MAKE_TOKEN(USER_DICTIONARY, null_token);"
        );
        bindgen::Builder::default()
            .header_contents(&format!("{facade}.hpp"), &wrapper)
            .clang_args(["-xc++", "-std=c++17"])
            .clang_arg(format!("-I{dir}"))
            .clang_args(&include_dirs)
            .allowlist_function(format!("{facade}_.*"))
            .allowlist_type("PinyinTableFlag|PinyinAmbiguity2|PinyinCorrection2|DoublePinyinScheme|ZhuyinScheme|PHRASE_INDEX_LIBRARIES|sort_option_t")
            .allowlist_var("PHRASE_MASK|PHRASE_INDEX_LIBRARY_MASK|null_token|OXPINYIN_USER_TOKEN")
            .blocklist_type("_?GArray")
            .raw_line("use glib_sys::GArray;")
            .prepend_enum_name(false)
            .dynamic_library_name(library)
            .dynamic_link_require_all(true)
            .wrap_unsafe_ops(true)
            .generate_comments(false)
            .layout_tests(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("generate the public C bindings")
            .write_to_file(out.join(format!("{facade}.rs")))
            .expect("write C bindings");

        let register = format!("{dir}/lib{facade}.alloc");
        println!("cargo:rerun-if-changed={register}");
        let mut rows = String::from("&[\n");
        for line in fs::read_to_string(register).expect("read .alloc").lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<_> = line.split_whitespace().collect();
            let [symbol, name, release, on_false, _verified] = fields[..] else {
                panic!("invalid {facade} register row: {line}");
            };
            writeln!(rows, "Slot {{ symbol: {symbol:?}, name: {name:?}, release: {release:?}, on_false: {on_false:?}, hit: false, probed: false }},").unwrap();
        }
        rows.push(']');
        fs::write(out.join(format!("{facade}-slots.rs")), rows).expect("write slot register");
    }
}

#[cfg(not(feature = "abi-test"))]
fn main() {}
