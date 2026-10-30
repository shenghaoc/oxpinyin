//! `gen_pinyin_table` against results derived from the pin's source
//! (libpinyin 074a2219 `utils/storage/gen_pinyin_table.cpp`), run as the
//! built binary so the C streams, exit statuses and GLib lines are real.

use std::path::Path;
use std::process::{Command, Output};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gen_pinyin_table"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run gen_pinyin_table")
}

#[test]
fn rows_follow_length_then_first_reading_then_phrase_bytes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("in.txt"),
        "你好 ni'hao 10\n你 ni 5\n尼 ni 2\n你好 nihao 4\n呢 ne 1\n好 hao3 7\n好 hao4 2\n\
         你们 ni 3\n一二三四五六七八九十一二三四五六 yi 1\n",
    )
    .unwrap();
    let out = run(dir.path(), &["-t", "3", "-o", "out.table", "in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "nnodes: 5\n");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Duplicate item: phrase:你好\tpinyin:nihao\tfreq:4\n\
         Invalid pinyin:你们\tni\t3\n\
         Too long phrase:一二三四五六七八九十一二三四五六\tyi\t1\n"
    );
    // gen_phrase_file writes a frequency under 3 as 3: `/* avoid zero
    // freq. */ if (freq < 3) freq = 3;` (`gen_pinyin_table.cpp:311-312` at
    // 074a2219), before the row's fprintf (`:314-316`). Equal first readings
    // keep the tree's order: `memcmp` over the host-order UCS-4 words.
    // U+5C3C 尼 sorts before U+4F60 你 on a little-endian host
    // (0x3C < 0x60) and after it on a big-endian one.
    let (first, second) = if cfg!(target_endian = "little") {
        ("ni\t尼\t50331651\t3\n", "ni\t你\t50331652\t5\n")
    } else {
        ("ni\t你\t50331651\t5\n", "ni\t尼\t50331652\t3\n")
    };
    let expected = format!(
        "hao3\t好\t50331649\t7\nhao4\t好\t50331649\t3\nne\t呢\t50331650\t3\n\
         {first}{second}ni'hao\t你好\t50331653\t14\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("out.table")).unwrap(),
        expected
    );
}

#[test]
fn the_default_output_is_temp_out_and_a_last_row_without_newline_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in.txt"), "你 ni 5\n好 hao 3").unwrap();
    let out = run(dir.path(), &["in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "nnodes: 1\n");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
        "ni\t你\t1\t5\n"
    );
}

#[test]
fn a_missing_input_exits_enoent_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["absent.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Can't open file absent.txt.\n"
    );
    assert!(!dir.path().join("temp.out").exists());
}

#[test]
fn an_unknown_option_exits_einval_with_the_glib_message() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["--bogus"]);
    assert_eq!(out.status.code(), Some(22));
    assert!(
        String::from_utf8_lossy(&out.stdout).starts_with("option parsing failed:"),
        "{out:?}"
    );
}

#[test]
fn an_empty_phrase_counts_as_a_node_and_meets_glibs_null_array_check() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in.txt"), b"\0 - 1\n\xe4\xbd\xa0 ni 5\n").unwrap();
    let out = run(dir.path(), &["in.txt"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "nnodes: 2\n");
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("g_array_append_vals: assertion 'array' failed"),
        "{out:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
        "ni\t你\t1\t5\n"
    );
}

/// An unconverted phrase reaches the output stage as the pin's NULL
/// `uniphrase` does: every input is read and the output file is opened
/// (`gen_pinyin_table.cpp:110-112`, `:261`) before the row that reads it
/// (`:280-281`). There the pin dies of undefined behaviour; oxpinyin names
/// the phrase and exits 1, flushing what it wrote (class (b)).
#[test]
fn an_unconvertible_phrase_is_refused_at_the_pins_row_after_the_output_opens() {
    for (input, nnodes, phrase, rows) in [
        (&b"\xff a 1\n"[..], "nnodes: 1\n", &b"\xff"[..], ""),
        (
            &b"\xe4\xbd\xa0 ni 5\n\xff\xff a'a 1\n"[..],
            "nnodes: 2\n",
            &b"\xff\xff"[..],
            "ni\t你\t1\t5\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("in.txt"), input).unwrap();
        let out = run(dir.path(), &["in.txt"]);
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), nnodes);
        assert_eq!(
            out.stderr,
            [
                &b"gen_pinyin_table: invalid UTF-8 phrase: "[..],
                phrase,
                b"\n"
            ]
            .concat()
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("temp.out")).unwrap(),
            rows
        );
    }
}
