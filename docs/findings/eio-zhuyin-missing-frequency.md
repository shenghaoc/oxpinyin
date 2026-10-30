# gen_zhuyin_table missing frequency — normal refusal

Status: accepted class (b), 2026-10-30 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219, read from `/workspace/eio-env/libpinyin-src` at
that commit before cases ran.

`utils/storage/gen_zhuyin_table.cpp:148-159` splits each line on a single
space with at most three fields. It calls `atoi(strs[1])` at line 153
before checking `g_strv_length` at line 156. An empty or single-field line
has no frequency: the call dereferences NULL (or an out-of-bounds pointer
for the empty vector), undefined behavior. The reference pin dies with
SIGSEGV. The Rust utility refuses with
`gen_zhuyin_table: missing frequency: <line>\n` on stderr and normal exit 1.
The output has not been opened and stdout is empty. With two fields the
frequency exists: the same `atoi` call precedes `wrong line format:<line>\n`
and processing continues, matching the pin.

The `zhuyin-missing-frequency` case binds complete observations through
`just policy-record`; `zhuyin-normal` includes the two-field control.
No shipping library, C API or backend format changes.
