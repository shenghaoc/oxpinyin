//! Compiles the C shim against the target's own headers, so `FILE`,
//! `LC_NUMERIC` and the scanner formats are the C library's, not guessed.
//! With the `tools` feature it also compiles the utilities' GLib shim
//! against GLib's headers, found with `pkg-config` as `oxpinyin-store`'s
//! build finds its backends, and their C++ stream shim.

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
    if std::env::var_os("CARGO_FEATURE_TOOLS").is_some() {
        tools();
    }
}

/// `pkg-config <flag> glib-2.0`, split on whitespace; `None` when
/// `pkg-config` is missing or does not know GLib.
fn glib(flag: &str) -> Option<Vec<String>> {
    let output = std::process::Command::new("pkg-config")
        .args([flag, "glib-2.0"])
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    })
}

fn tools() {
    println!("cargo:rerun-if-changed=native/tool.c");
    println!("cargo:rerun-if-changed=native/tool_stream.cc");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    let Some(cflags) = glib("--cflags") else {
        panic!("the tools feature needs GLib: `pkg-config --cflags glib-2.0` failed");
    };
    let mut shim = cc::Build::new();
    shim.file("native/tool.c")
        .warnings(true)
        .flag_if_supported("-Wformat=2")
        .flag_if_supported("-fvisibility=hidden");
    for flag in &cflags {
        shim.flag(flag);
    }
    shim.compile("oxpinyin_input_tool");
    for flag in glib("--libs").unwrap_or_else(|| vec!["-lglib-2.0".to_owned()]) {
        if let Some(dir) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(name) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={name}");
        }
    }
    cc::Build::new()
        .cpp(true)
        .file("native/tool_stream.cc")
        .warnings(true)
        .flag_if_supported("-fvisibility=hidden")
        .compile("oxpinyin_input_tool_stream");
}
