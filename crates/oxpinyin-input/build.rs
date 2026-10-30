//! Compiles the C shim against the target's own headers, so `FILE`,
//! `LC_NUMERIC` and the scanner formats are the C library's, not guessed.
//! With the `tools` feature it also compiles the utilities' GLib shim
//! against GLib's headers, found with the `pkg-config` crate as
//! `oxpinyin-store`'s build finds its backends, and their C++ stream shim.

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
    #[cfg(feature = "tools")]
    tools();
}

#[cfg(feature = "tools")]
fn tools() {
    println!("cargo:rerun-if-changed=native/tool.c");
    println!("cargo:rerun-if-changed=native/tool_stream.cc");
    // The probe emits the link-search paths and the library, and tracks the
    // PKG_CONFIG_* variables. System paths stay out, as pkg-config leaves
    // them without the allow-system variables.
    let Ok(glib) = pkg_config::Config::new()
        .print_system_cflags(false)
        .print_system_libs(false)
        .probe("glib-2.0")
    else {
        panic!("the tools feature needs GLib: `pkg-config --cflags glib-2.0` failed");
    };
    let mut shim = cc::Build::new();
    shim.file("native/tool.c")
        .warnings(true)
        .flag_if_supported("-Wformat=2")
        .flag_if_supported("-fvisibility=hidden");
    for dir in &glib.include_paths {
        shim.include(dir);
    }
    for (name, value) in &glib.defines {
        shim.define(name, value.as_deref());
    }
    shim.compile("oxpinyin_input_tool");
    cc::Build::new()
        .cpp(true)
        .file("native/tool_stream.cc")
        .warnings(true)
        .flag_if_supported("-fvisibility=hidden")
        .compile("oxpinyin_input_tool_stream");
}
