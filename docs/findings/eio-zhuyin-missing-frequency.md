# gen_zhuyin_table missing frequency — normal refusal

Status: accepted class (b), 2026-10-30 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219, read from `/workspace/eio-env/libpinyin-src` at
that commit before the cases ran.

`utils/storage/gen_zhuyin_table.cpp:148-159` splits each line on a single
space into at most three fields. It calls `atoi(strs[1])` at line 153
before it checks `g_strv_length` at line 156. An empty or single-field line
has no frequency. For such a line, the call dereferences NULL (or an
out-of-bounds pointer for the empty vector), which is undefined behaviour.
The reference pin dies with SIGSEGV.

The Rust utility refuses with
`gen_zhuyin_table: missing frequency: <line>\n` on stderr and a normal exit 1.
It has not opened the output, and stdout is empty. A line with two fields
has a frequency. For that line, the same `atoi` call comes before
`wrong line format:<line>\n`, and processing continues, as in the pin.

The `zhuyin-missing-frequency` case in `crates/oxpinyin-utils/tests/gen_zhuyin_table.rs`
holds oxpinyin's side. `zhuyin-normal` includes the two-field control.
No shipping library, C API or backend format changes.
