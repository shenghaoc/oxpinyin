# Pinned scripts2 Rust tables

`just rust-tables` checks the Rust tables from libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99` against a temporary tree and fails
if any committed output differs. Use `just rust-tables --write` to regenerate;
`--check` remains an explicit spelling of the default. Both modes use the
managed Python selected by `.python-version` through the existing `tools.dev`
entry point. Provision the interpreter once with `just python`. The table recipe
passes `--offline --no-python-downloads` to uv and does not bootstrap Python; a
missing interpreter is an error. The generator/check needs no network,
subprocess, Rust toolchain or external input.

The maintainer authorized this repository tool under constitution §7. It replaces
unrecorded hand conversion, keeps the pin's table values, and does not expand the
scripts2 drop-in-tool scope. It does not generate the project's C headers or alter
parser algorithms. The five committed upstream headers are acceptance oracles.

## Sources and formatting

`upstream/` contains unmodified copies of the pin's `scripts2/` (including its
input-independent data, templates and distiller), five `src/storage/` headers and
`COPYING`. `manifest.json` records each upstream path, git blob hash and SHA-256.
All 21 scripts2 blobs and the five headers are byte-identical between `0c5e80e1`
and `074a2219`. Copyright and GPL-2.0-or-later notices remain intact; the maintainer
approved their use with this GPL-3.0-or-later project.

`tools/dev/rust_tables.py` imports the untouched modules and calls the `gen_*`
functions selected by `generateheader.get_table_content`. It verifies every
returned section against the committed header and reconstructs all five complete
headers byte for byte. It then formats the generated rows as Rust. Upstream
membership, row ordering, flags, content indexes and special-table frequency/
distance computations remain upstream's. The Rust projections retain only the
fields the existing Rust tables store; no new parser logic is introduced.
`NULL` becomes `None`, enum values use their upstream ordinals, strings and
sentinel handling follow the existing Rust representation. Valid-tone booleans
are packed into the existing masks in `SyllableKey` ID order.

`inventory.tsv` is the complete inventory found with `git grep` at direct parent
`218f597a272dea4bc5c1bb56a3956c59ceb69b8d` (#1522). It maps original Rust locations
to header markers and upstream functions, including downstream build artifacts
and handwritten consumers. The generated destinations are:

| Destination (under `crates/`) | Data and identity against #1522 |
| --- | --- |
| `oxpinyin-chewing/src/chewing_key_tables.rs` | Three enum counts; 1,728 inverse content indexes; exact values/order |
| `oxpinyin-chewing/src/content_table_source.rs` | 441 complete content rows; row initializer bytes unchanged |
| `oxpinyin-chewing/src/tone_symbols.rs` | Zero-tone enum and tone-symbol match; exact values |
| `oxpinyin-core/src/table_data/full_pinyin_syllables.rs` | 405 full syllables in `fullpinyin.PINYIN_DICT` numeric-ID order; exact values/order |
| `oxpinyin-core/src/table_data/syllable_inventory.rs` | Counts, maximum length, 23 incomplete, 2 option-only, 10 Zhuyin-only keys; exact values/order |
| `oxpinyin-core/src/table_data/full_pinyin_indexes.rs` | 406 Luoma and 406 secondary-Zhuyin rows and counts; exact values/order |
| `oxpinyin-core/src/table_data/full_pinyin_counts.rs` | Existing duplicate public counts; exact values |
| `oxpinyin-core/src/table_data/zhuyin_indexes.rs` | 1,493 standard, 500 Hsu, 482 Eten26 rows with consulted flags; exact values/order |
| `oxpinyin-core/src/table_data/zhuyin_tones.rs` | Three counts and 440 valid-tone masks; exact values/order; fallback below |
| `oxpinyin-core/src/table_data/double_pinyin.rs` | Six schemes' sheng/yun tables and three fallback lists; exact values/order |
| `oxpinyin-core/src/table_data/zhuyin_keyboards.rs` | Eight keyboards' 24 symbol/component/tone lists; exact values/order |
| `oxpinyin-core/src/table_data/tone_symbols.rs` | Existing public tone-symbol match; exact values |
| `oxpinyin-engine/src/session/split_tables.rs` | 85 resplit and 20 divided rows; exact values/order |

Whole-file identity is value identity because banners, formatting and locations
change. `acceptance.json` records 67 original Rust literal declarations from the
direct parent, independently of the formatter. Its digests preserve row order,
strings, flags, fields and indexes while ignoring comments, whitespace, commas
and equivalent integer spelling. Every generation checks them. Tone match
literals are checked against the header's literal `chewing_tone_table` and
`chewing.ASCII_CHEWING_TONE_LIST`. The existing Rust `tables_cross_validate` and
other table identity tests are unchanged. Mixed files retain handwritten code
and include generated declarations; existing build scripts still create their
packed string pools and PHF maps from these source tables.

## Accepted pinyins input

The maintainer explicitly authorized committing this derived fixture, a narrow
exception to the model-data no-vendoring policy. No model archive or source table
is committed. The approved model20 archive is
`https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz`, SHA-256
`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`.
Its `gb_char.table` and `gbk_char.table` are placed in `data/` beside `scripts2/`;
from `scripts2/`, the unmodified command is `python tools/distill.py`.
The resulting `inputs/pinyins.txt` comes from the pin's `scripts2/` distiller
and is GPL-3.0-or-later, as the pin. It is 295,767 bytes, SHA-256
`97d6cafcc6205e2d01b23fb1620105291a74105afb83b12160917ae34901ff8a`.
The pin's own `generateheader.py templates/special_table.h.in` reproduces the
committed 27,700-byte `special_table.h` byte for byte with that fixture.

The recorded validation step uses local downloads only and verifies the archive
hash, distilled fixture, and all five headers with the pin's own programs:

```sh
just rust-tables --model-archive /path/to/model20.text.tar.gz --tsi-table /path/to/upstream/tsi.table
```

## Bounded tsi input search and fallback

Two actual upstream data sources were found and tested; neither was synthesized
from the expected output:

| Upstream selection | Archive SHA-256 | `tsi.table` bytes / SHA-256 |
| --- | --- | --- |
| libzhuyin `b0217b44f0025d563def780109daff3fcfa0840f`, `data/Makefile.am`, model9 | `ea8eb5198931851d6e008e33e6dee9a1c22df38845123a71c8579b7929e62228` | 6,570,578 / `ffed1c695b9983dcd9d16d74aaa2bb4d050da8946a9a0b420693a7672a3aae1c` |
| ibus-libzhuyin `fd495e0a2f8260a63e3a1de176202605409a6a2f`, `data/model/Makefile.am`, model13 | `3c89bfc5842bb2c03d34c70ba6728b29aee2a3a06f0014984c5020c597e9f8f2` | 6,570,603 / `ef6b7454b0747cdf3887fdab37cde3069a95e0a8e3480b4fe539e62442a816fe` |

Archive URLs are `https://downloads.sourceforge.net/libzhuyin/models/model9.text.tar.gz`
and `https://downloads.sourceforge.net/libzhuyin/models/model13.text.tar.gz`.
Both candidates with the accepted pinyins input reproduce the committed
153,465-byte `zhuyin_table.h` and `special_table.h` byte for byte. Model9 also
passed the recorded validation command for all five headers. Neither archive
contains a licence or attribution file establishing redistribution terms for
`tsi.table`; the upstream program's GPL notice alone is not data-licence evidence.
Their size and missing licence evidence rule out vendoring either here.

The sole fallback is `@VALID_ZHUYIN_TABLE@` from committed `zhuyin_table.h`, git
blob `d096901adba1a0374be856460586f3ad181099ea`. `just rust-tables` extracts that
section only after checking the complete header blob. It then emits
`VALID_ZHUYIN_TONES`; keyboard tables still come from `bopomofotable` generators.
The explicit refresh/extraction step is:

```sh
just rust-tables --write --from-pin /path/to/libpinyin
just rust-tables --check
```

The refresh reads `git show 074a2219:src/storage/...`, verifies recorded blobs,
then runs the same formatter. Check mode refuses external-input flags and never
downloads. It uses only committed upstream sources and the accepted fixture.

## Verification

`just test-tools tools/dev/rust_tables.test.py` integrates source hashes, header
roundtrips, exact Rust values, offline checks, hash-seed stability and drift
rejection into the existing Python tooling test job. No CI job is added.

The named mutation `content-ni-initial` changes the generated `ni` content row's
`initial: 11` to `initial: 1`. The tool test verifies that `--check` rejects it
without rewriting the altered file and that the acceptance digests reject it.
For the parser witness, build the mutated subject with `just cell-artifacts` and
run the existing Rust contract test `default_option_single_key` against the unchanged
pin; then restore with `just rust-tables --write` and rebuild. PR results record that
witness, the selected parser/scheme cases, and the ordinary Rust/CI gates.
