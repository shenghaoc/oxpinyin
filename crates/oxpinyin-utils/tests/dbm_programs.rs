//! Frozen pin observations of the pin's three DBM test programs
//! (`tests/storage/test_ngram.cpp`, `test_flexible_ngram.cpp`,
//! `test_punct_table.cpp` at 074a2219): the stdout and the rows the programs
//! leave in their fixed `/tmp` files. Each case moves those files to its own
//! directory through `fixed_tmp::OVERRIDE`.
#[cfg(unix)]
mod common;

// Unix only: the frozen pin runs are Linux and macOS observations (signal statuses, Unix paths and line endings).
#[cfg(unix)]
mod frozen {
    mod test_flexible_ngram {
        use crate::common::expected::*;
        use crate::common::{
            CELL, inventory, mask_glib_prefix, snapshot_inputs, undo_tmp_override,
        };
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::os::unix::process::ExitStatusExt as _;
        use std::time::Duration;

        const FLEXIBLE_NGRAM_ITEMS: &[u8] = b"--------------------------------------------------------\nitem:1:16\nitem:2:1\nitem:3:32\nitem:4:4\nitem:6:2\n";
        const FLEXIBLE_NGRAM_ROWS: File =
            Rows("9228bec49062adb96bff08ceafb53a08dd76af97e288ca226edb494c8ddf4e87");
        fn flexible_ngram_files() -> &'static [(&'static str, File)] {
            match CELL {
                "bdb" | "tkrzw" => &[("tmp/training.db", FLEXIBLE_NGRAM_ROWS)],
                "kc" => &[(
                    "tmp/training.db",
                    Sha256(
                        "d70b443120c7a960709094bd651cd09640198146ec8bb5afeb14af7a9255d273",
                        6297880,
                    ),
                )],
                _ => unreachable!("unknown cell {CELL}"),
            }
        }
        fn flexible_ngram_session() -> Stdout {
            match CELL {
                "bdb" => Exact(include_bytes!(
                    "fixtures/test_flexible_ngram/session-bdb-order.txt"
                )),
                "kc" | "tkrzw" => Exact(include_bytes!(
                    "fixtures/test_flexible_ngram/session-kc-tkrzw-order.txt"
                )),
                _ => unreachable!("unknown cell {CELL}"),
            }
        }

        #[rstest]
        // flexible-ngram-attach-directory: tests/storage/test_flexible_ngram.cpp:25-139;
        // src/storage/flexible_single_gram.h:118-139 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::flexible_ngram_attach_directory(
            &[],
            b"",
            &["tmp/training.db"],
            &[("tmp/training.db/child", b"occupied".as_slice())],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/training.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    Exact(FLEXIBLE_NGRAM_ITEMS)
                } else {
                    Exact(b"")
                },
                stderr: b"",
                files: &[],
            },
        )]
        // flexible-ngram-empty-file: tests/storage/test_flexible_ngram.cpp:25-139;
        // src/storage/flexible_single_gram.h:118-139 at 074a2219.
        // Policy-accepted on kc, tkrzw: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: a written DBM container's size and bytes follow the 16
        // KiB page, against 4 KiB on Linux.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::flexible_ngram_empty_file(
            &[],
            b"",
            &[],
            &[("tmp/training.db", b"".as_slice())],
            (if CELL == "bdb" { &["tmp/training.db"][..] } else { &[] }, &["tmp/training.db"][..]),
            Run {
                status: match CELL {
                    "bdb" => Exit(0),
                    "kc" | "tkrzw" => Signal(6),
                    _ => unreachable!("unknown cell {CELL}"),
                },
                stdout: match CELL {
                    "bdb" => Exact(include_bytes!("fixtures/test_flexible_ngram/session-bdb-order.txt")),
                    "kc" | "tkrzw" => if cfg!(target_os = "macos") {
                        Exact(FLEXIBLE_NGRAM_ITEMS)
                    } else {
                        Exact(b"")
                    },
                    _ => unreachable!("unknown cell {CELL}"),
                },
                stderr: match CELL {
                    "bdb" => b"BDB0004 fop_read_meta: /tmp/training.db: unexpected file type or format\nBDB0004 fop_read_meta: /tmp/training.db: unexpected file type or format\n",
                    "kc" | "tkrzw" => b"",
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: match CELL {
                    "bdb" => &[
                        ("tmp/training.db", FLEXIBLE_NGRAM_ROWS),
                    ],
                    "kc" => &[
                        ("tmp/training.db", KC_EMPTY_TREE_DB),
                    ],
                    "tkrzw" => if cfg!(target_os = "macos") {
                        &[
                            ("tmp/training.db", Sha256("38eed544d3007d439ac66ae65c3f8db9d3b78bd3bbe874abb71df68d43ffb41f", 4210688)),
                        ]
                    } else {
                        &[
                            ("tmp/training.db", Sha256("795769011a4e03526a6929dc7218163bec83e82ac29ac2f1fd4e6722b75bec1e", 4198400)),
                        ]
                    },
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        // flexible-ngram-ignored-input: tests/storage/test_flexible_ngram.cpp:25-139;
        // src/storage/flexible_single_gram.h:118-139 at 074a2219.
        // Also the witness of a fresh run: the arguments and input are ignored.
        #[case::flexible_ngram_ignored_input(
            &["--help", "ignored"],
            b"ignored\nquit\n",
            &[],
            &[],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/training.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Exit(0),
                stdout: flexible_ngram_session(),
                stderr: b"",
                files: flexible_ngram_files(),
            },
        )]
        fn captured_cases(
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] directories: &[&str],
            #[case] files: &[(&str, &[u8])],
            #[case] file_checks: (&[&str], &[&str]),
            #[case] expected: Run,
        ) {
            let (dbm_files, unclosed_hash_files) = file_checks;
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("tmp")
                .create_dir_all()
                .expect("private fixed names");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("test_flexible_ngram")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .env(
                    oxpinyin_utils::fixed_tmp::OVERRIDE,
                    directory.path().join("tmp"),
                )
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_flexible_ngram");
            output.stderr = undo_tmp_override(
                &mask_glib_prefix(&output.stderr),
                &directory.path().join("tmp"),
            );
            let aborted = output.status.signal() == Some(6);
            check(
                output,
                &inventory(
                    directory.path(),
                    &inputs,
                    dbm_files,
                    &[],
                    &[],
                    if CELL == "tkrzw" && aborted {
                        unclosed_hash_files
                    } else {
                        &[]
                    },
                ),
                &inputs,
                &expected,
            );
        }
    }
    mod test_ngram {
        use crate::common::expected::*;
        use crate::common::{
            CELL, inventory, mask_glib_prefix, snapshot_inputs, undo_tmp_override,
        };
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::os::unix::process::ExitStatusExt as _;
        use std::time::Duration;

        const NGRAM_ROWS: File =
            Rows("1151079d6f22bfbe9617d0feaeb4bb08c14d2e4008c7f4bebf5a874ecd72a6cb");
        fn ngram_files() -> &'static [(&'static str, File)] {
            match CELL {
                "bdb" | "tkrzw" => &[("tmp/snapshot.db", NGRAM_ROWS), ("tmp/test.db", NGRAM_ROWS)],
                "kc" => &[
                    (
                        "tmp/snapshot.db",
                        Sha256(
                            "3a15f06288d4718649d9f41ad01cd9d2dbb34d3bb78614cd14734a09e37570ba",
                            109,
                        ),
                    ),
                    (
                        "tmp/test.db",
                        Sha256(
                            "2c4989dc102aaacb047913d00422b71c1f289de8a672552f71e24aefdb647d42",
                            6297848,
                        ),
                    ),
                ],
                _ => unreachable!("unknown cell {CELL}"),
            }
        }
        fn ngram_session() -> Stdout {
            match CELL {
                "bdb" => Exact(include_bytes!("fixtures/test_ngram/session-bdb-order.txt")),
                "kc" | "tkrzw" => Exact(include_bytes!(
                    "fixtures/test_ngram/session-kc-tkrzw-order.txt"
                )),
                _ => unreachable!("unknown cell {CELL}"),
            }
        }

        #[rstest]
        // ngram-attach-directory: tests/storage/test_ngram.cpp:5-95;
        // src/storage/ngram.cpp:31-271 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::ngram_attach_directory(
            &[],
            b"",
            &["tmp/test.db"],
            &[("tmp/test.db/child", b"occupied".as_slice())],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/test.db", "tmp/snapshot.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    Exact(b"--------------------------------------------------------\nitem:1:1.000000\nitem:2:0.062500\nitem:3:2.000000\nitem:4:0.250000\nitem:6:0.125000\n")
                } else {
                    Exact(b"")
                },
                stderr: b"",
                files: &[],
            },
        )]
        // ngram-empty-file: tests/storage/test_ngram.cpp:5-95; src/storage/ngram.cpp:31-271 at
        // 074a2219.
        #[case::ngram_empty_file(
            &[],
            b"",
            &[],
            &[("tmp/test.db", b"".as_slice())],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/test.db", "tmp/snapshot.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Exit(0),
                stdout: ngram_session(),
                stderr: match CELL {
                    "bdb" => b"BDB0004 fop_read_meta: /tmp/test.db: unexpected file type or format\n",
                    "kc" | "tkrzw" => b"",
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: ngram_files(),
            },
        )]
        // ngram-ignored-input: tests/storage/test_ngram.cpp:5-95; src/storage/ngram.cpp:31-271
        // at 074a2219.
        // Also the witness of a fresh run: the arguments and input are ignored.
        #[case::ngram_ignored_input(
            &["--help", "ignored"],
            b"ignored\nquit\n",
            &[],
            &[],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/test.db", "tmp/snapshot.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Exit(0),
                stdout: ngram_session(),
                stderr: b"",
                files: ngram_files(),
            },
        )]
        // ngram-snapshot-directory: tests/storage/test_ngram.cpp:5-95;
        // src/storage/ngram.cpp:31-271 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: a written DBM container's size and bytes follow the 16
        // KiB page, against 4 KiB on Linux.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::ngram_snapshot_directory(
            &[],
            b"",
            &["tmp/snapshot.db"],
            &[("tmp/snapshot.db/child", b"occupied".as_slice())],
            (if CELL == "bdb" { &["tmp/test.db", "tmp/snapshot.db"][..] } else { &[] }, &["tmp/test.db"][..]),
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    match CELL {
                        "bdb" => Exact(include_bytes!("fixtures/test_ngram/session-bdb-order.txt")),
                        "kc" | "tkrzw" => Exact(include_bytes!("fixtures/test_ngram/session-kc-tkrzw-order.txt")),
                        _ => unreachable!("unknown cell {CELL}"),
                    }
                } else {
                    Exact(b"")
                },
                stderr: b"",
                files: match CELL {
                    "bdb" => &[
                        ("tmp/test.db", NO_ROWS),
                    ],
                    "kc" => if cfg!(target_os = "macos") {
                        &[
                            ("tmp/test.db", Sha256("e29167e48e1336863f068c8a9c7ba0e684d3fe5640743f5ef0fe4de88c634928", 9453568)),
                        ]
                    } else {
                        &[
                            ("tmp/test.db", Sha256("50b2896de34a85f3a198bbc62eaeeb95bb85e2b329cd17c84658b78d35323cad", 9449472)),
                        ]
                    },
                    "tkrzw" => if cfg!(target_os = "macos") {
                        &[
                            ("tmp/test.db", Sha256("11069e51fc1407ae30a8c9adf7174edd138971085a4ba4181964f154de4fee81", 4210688)),
                        ]
                    } else {
                        &[
                            ("tmp/test.db", Sha256("8cb39f7e2735508e4fce6682beecc2b916143ffdb037944a6f6589419055065a", 8396800)),
                        ]
                    },
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        fn captured_cases(
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] directories: &[&str],
            #[case] files: &[(&str, &[u8])],
            #[case] file_checks: (&[&str], &[&str]),
            #[case] expected: Run,
        ) {
            let (dbm_files, unclosed_hash_files) = file_checks;
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("tmp")
                .create_dir_all()
                .expect("private fixed names");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("test_ngram")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .env(
                    oxpinyin_utils::fixed_tmp::OVERRIDE,
                    directory.path().join("tmp"),
                )
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_ngram");
            output.stderr = undo_tmp_override(
                &mask_glib_prefix(&output.stderr),
                &directory.path().join("tmp"),
            );
            let aborted = output.status.signal() == Some(6);
            check(
                output,
                &inventory(
                    directory.path(),
                    &inputs,
                    dbm_files,
                    &[],
                    &[],
                    if CELL == "tkrzw" && aborted {
                        unclosed_hash_files
                    } else {
                        &[]
                    },
                ),
                &inputs,
                &expected,
            );
        }
    }
    mod test_punct_table {
        use crate::common::expected::*;
        use crate::common::{
            CELL, inventory, mask_glib_prefix, snapshot_inputs, undo_tmp_override,
        };
        use assert_cmd::cargo::cargo_bin_cmd;
        use assert_fs::{TempDir, prelude::*};
        use rstest::rstest;
        use std::os::unix::process::ExitStatusExt as _;
        use std::time::Duration;

        const PUNCT_TABLE_ROWS: File =
            Rows("ae0465b244d2ef0254080ab366507afaa96fb980ad2039cdc028ad0970b1f75a");
        fn punct_table_files() -> &'static [(&'static str, File)] {
            match CELL {
                "bdb" | "tkrzw" => &[
                    ("tmp/punct.bin", PUNCT_TABLE_ROWS),
                    ("tmp/snapshot.db", PUNCT_TABLE_ROWS),
                ],
                "kc" => &[
                    (
                        "tmp/punct.bin",
                        Sha256(
                            "c8b7869479efa13371cc98906ece5bfbbe44944a0b18f1ce204cc64644a7bb44",
                            400128,
                        ),
                    ),
                    (
                        "tmp/snapshot.db",
                        Sha256(
                            "90cbec5f40ff86feff4fadf181d8a2740f958a44e9689840d2002f6907fe7817",
                            34,
                        ),
                    ),
                ],
                _ => unreachable!("unknown cell {CELL}"),
            }
        }

        #[rstest]
        // punct-table-attach-directory: tests/storage/test_punct_table.cpp:25-77;
        // src/storage/punct_table.cpp:117-177 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        #[case::punct_table_attach_directory(
            &[],
            b"",
            &["tmp/punct.bin"],
            &[("tmp/punct.bin/child", b"occupied".as_slice())],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/punct.bin", "tmp/snapshot.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Signal(6),
                stdout: Exact(b""),
                stderr: b"",
                files: &[],
            },
        )]
        // punct-table-empty-file: tests/storage/test_punct_table.cpp:25-77;
        // src/storage/punct_table.cpp:117-177 at 074a2219.
        #[case::punct_table_empty_file(
            &[],
            b"",
            &[],
            &[("tmp/punct.bin", b"".as_slice())],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/punct.bin", "tmp/snapshot.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_punct_table/session.txt")),
                stderr: match CELL {
                    "bdb" => b"BDB0004 fop_read_meta: /tmp/punct.bin: unexpected file type or format\n",
                    "kc" | "tkrzw" => b"",
                    _ => unreachable!("unknown cell {CELL}"),
                },
                files: punct_table_files(),
            },
        )]
        // punct-table-ignored-input: tests/storage/test_punct_table.cpp:25-77;
        // src/storage/punct_table.cpp:117-177 at 074a2219.
        // Also the witness of a fresh run: the arguments and input are ignored.
        #[case::punct_table_ignored_input(
            &["--help", "ignored"],
            b"ignored\nquit\n",
            &[],
            &[],
            (if ["bdb", "tkrzw"].contains(&CELL) { &["tmp/punct.bin", "tmp/snapshot.db"][..] } else { &[] }, &[][..]),
            Run {
                status: Exit(0),
                stdout: Exact(include_bytes!("fixtures/test_punct_table/session.txt")),
                stderr: b"",
                files: punct_table_files(),
            },
        )]
        // punct-table-snapshot-directory: tests/storage/test_punct_table.cpp:25-77;
        // src/storage/punct_table.cpp:117-177 at 074a2219.
        // Policy-accepted on every cell: docs/findings/eio-tool-check-messages.md; the pin's
        // own result is in fixtures/contract-policy/eio-tool-check-messages.json.
        // Platform exception, macos: a written DBM container's size and bytes follow the 16
        // KiB page, against 4 KiB on Linux.
        // Platform exception, macos: abort() flushes stdio on macOS, so buffered stdout
        // survives; glibc's discards it.
        #[case::punct_table_snapshot_directory(
            &[],
            b"",
            &["tmp/snapshot.db"],
            &[("tmp/snapshot.db/child", b"occupied".as_slice())],
            (if CELL == "bdb" { &["tmp/punct.bin", "tmp/snapshot.db"][..] } else { &[] }, &["tmp/punct.bin"][..]),
            Run {
                status: Signal(6),
                stdout: if cfg!(target_os = "macos") {
                    Exact("created table.\ninsert some punctuations.\ntoken: 1\nPunctuations: …… … ？\nremove some punctuations.\ntoken: 1\nPunctuations: …… ？\n".as_bytes())
                } else {
                    Exact(b"")
                },
                stderr: b"",
                files: match CELL {
                    "bdb" => &[
                        ("tmp/punct.bin", NO_ROWS),
                    ],
                    "kc" => if cfg!(target_os = "macos") {
                        &[
                            ("tmp/punct.bin", Sha256("2f91b99252d661fdc6e1c66047d48ce0e22e68e980fc248e1ee9b1878cf276d4", 606208)),
                        ]
                    } else {
                        &[
                            ("tmp/punct.bin", Sha256("92717a1a3de2adf73b2bb1587097db0225b9550fa51a25d105a5d05a7070a235", 602112)),
                        ]
                    },
                    "tkrzw" => &[
                        ("tmp/punct.bin", Sha256("2f80a5e1ab8cb2be5d500f05723276116f3d07baa1e5b03b5aa0583d0d6b23ec", 529408)),
                    ],
                    _ => unreachable!("unknown cell {CELL}"),
                },
            },
        )]
        fn captured_cases(
            #[case] args: &[&str],
            #[case] stdin: &[u8],
            #[case] directories: &[&str],
            #[case] files: &[(&str, &[u8])],
            #[case] file_checks: (&[&str], &[&str]),
            #[case] expected: Run,
        ) {
            let (dbm_files, unclosed_hash_files) = file_checks;
            let directory = TempDir::new().expect("scratch directory");
            directory
                .child("tmp")
                .create_dir_all()
                .expect("private fixed names");
            for name in directories {
                directory
                    .child(name)
                    .create_dir_all()
                    .expect("input directory");
            }
            for (name, content) in files {
                directory
                    .child(name)
                    .write_binary(content)
                    .expect("input bytes");
            }
            let inputs = snapshot_inputs(directory.path());
            let mut output = cargo_bin_cmd!("test_punct_table")
                .current_dir(directory.path())
                .env("LC_ALL", "C")
                .env(
                    oxpinyin_utils::fixed_tmp::OVERRIDE,
                    directory.path().join("tmp"),
                )
                .args(args)
                .write_stdin(stdin)
                .timeout(Duration::from_secs(300))
                .output()
                .expect("run test_punct_table");
            output.stderr = undo_tmp_override(
                &mask_glib_prefix(&output.stderr),
                &directory.path().join("tmp"),
            );
            let aborted = output.status.signal() == Some(6);
            check(
                output,
                &inventory(
                    directory.path(),
                    &inputs,
                    dbm_files,
                    &[],
                    &[],
                    if CELL == "tkrzw" && aborted {
                        unclosed_hash_files
                    } else {
                        &[]
                    },
                ),
                &inputs,
                &expected,
            );
        }
    }
}
