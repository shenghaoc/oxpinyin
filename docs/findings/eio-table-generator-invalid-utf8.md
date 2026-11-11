# Table-generator invalid UTF-8 — normal refusal

Status: accepted class (b), 2026-10-30 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219c90feaf962d0d24f034514033ece5f99, read from
`/workspace/eio-env/libpinyin-src` at that commit before the cases ran.

## Pin behaviour

`utils/storage/gen_pinyin_table.cpp:162-186` and
`utils/storage/gen_zhuyin_table.cpp:168-192` count UTF-8 characters and
convert the phrase before they check the parsed key count. A byte `ff`
counts as one character, but it makes `g_utf8_to_ucs4` return NULL. With a
one-key reading, the phrase enters the phrase tree. Equal-length tree
comparisons can dereference that NULL (`gen_pinyin_table.cpp:85-94`,
`gen_zhuyin_table.cpp:90-98`). If they do not, output conversion
dereferences it (`gen_pinyin_table.cpp:280-281`, `gen_zhuyin_table.cpp:286-287`).
This is undefined behaviour. The pin in the reference cell terminates with SIGSEGV.

## oxpinyin behaviour

The Rust generators do these steps:

1. Keep unconverted phrases.
2. Read the remaining inputs.
3. Print the node count.
4. Open the output.
5. Refuse at the unconverted row with
   `<utility>: invalid UTF-8 phrase: <bytes>\n` on stderr and a normal exit 1.

C `exit` flushes the preceding rows and stdout. oxpinyin does not reproduce
the buffer loss of the pin's crash. Invalid phrases of the same length sort
before valid phrases, and the sort reproduces no null read. Truncated UTF-8
lead bytes get zero padding, so no read goes outside the allocation.
This record extends the existing pinyin policy of #1436 to the matching zhuyin
utility. No shipping library, C API or backend data format changes.

The accepted cases are `pinyin-invalid-utf8`, `pinyin-invalid-utf8-after-row`,
`zhuyin-invalid-utf8`, and `zhuyin-invalid-utf8-after-row`. In each case, the pin
dies with SIGSEGV and writes nothing. oxpinyin prints the node count,
writes the rows before the invalid one, refuses on stderr and exits 1. The
cases in `crates/oxpinyin-utils/tests/gen_pinyin_table.rs` and
`gen_zhuyin_table.rs` hold oxpinyin's complete exit, stdout, stderr and files.
Ordinary input, an empty phrase, and a parse-length mismatch are controls.
An empty phrase with an empty reading is defined: it meets GLib's NULL-array
precondition in `store_one_item` and is omitted with GLib's critical message.
