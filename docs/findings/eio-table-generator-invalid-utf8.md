# Table-generator invalid UTF-8 — normal refusal

Status: accepted class (b), 2026-10-30 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219c90feaf962d0d24f034514033ece5f99, read from
`/workspace/eio-env/libpinyin-src` at that commit before cases ran.

`utils/storage/gen_pinyin_table.cpp:162-186` and
`utils/storage/gen_zhuyin_table.cpp:168-192` count UTF-8 characters and
convert the phrase before checking the parsed key count. A byte `ff`
counts as one character but makes `g_utf8_to_ucs4` return NULL. With a
one-key reading it enters the phrase tree. Equal-length tree comparisons
can dereference that NULL (`gen_pinyin_table.cpp:85-94`,
`gen_zhuyin_table.cpp:90-98`); otherwise output conversion dereferences it
(`gen_pinyin_table.cpp:280-281`, `gen_zhuyin_table.cpp:286-287`). This is
undefined behavior; the reference-cell pin terminates with SIGSEGV.

The Rust generators keep unconverted phrases, read the remaining inputs,
print the node count, open the output, and refuse at the unconverted row
with `<utility>: invalid UTF-8 phrase: <bytes>\n` on stderr and normal
exit 1. C `exit` flushes preceding rows and stdout. Pin crash buffer loss
is not reproduced. Same-length invalid phrases sort before valid phrases;
no null read is reproduced while ordering them. Truncated UTF-8 lead bytes
are backed with zero padding rather than reading outside the allocation.
This extends #1436's existing pinyin policy to the corresponding zhuyin
utility. No shipping library, C API or backend data format changes.

`tool-diff` cases `pinyin-invalid-utf8`, `pinyin-invalid-utf8-after-row`,
`zhuyin-invalid-utf8`, and `zhuyin-invalid-utf8-after-row` bind the complete
exit, stdout, stderr and file observations below through `just policy-record`.
Ordinary input, an empty phrase, and a parse-length mismatch are controls.
An empty phrase with an empty reading is defined: it meets GLib's NULL-array
precondition in `store_one_item` and is omitted with GLib's critical message.
