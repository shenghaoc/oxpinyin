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

```contract-diff-policy
{
  "schema": 2,
  "record": "eio-zhuyin-missing-frequency",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "zhuyin-missing-frequency": {
      "database_free": {
        "pin": {
          "sha256": "3ee58f2cbc49c4f3f44c75e898d271debcd6a35f17d857e99d084f938f0f6424",
          "fields": {
            "exit": "2e42a91d2375f9a3",
            "files": "f52b969b8af026c8",
            "stderr": "12ae32cb1ec02d01",
            "stdout": "12ae32cb1ec02d01"
          }
        },
        "subject": {
          "sha256": "79c3f50e8f5db863a87ae76f0560895c12b83f90e3089dad842afb45b248551f",
          "fields": {
            "exit": "6b86b273ff34fce1",
            "files": "f52b969b8af026c8",
            "stderr": "3392713bb47f3ae7",
            "stdout": "12ae32cb1ec02d01"
          }
        },
        "divergence": [
          "exit: pin -11; subject 1",
          "stderr: pin \"\"; subject \"gen_zhuyin_table: missing frequency: 你好\\n\""
        ]
      }
    }
  }
}
```
