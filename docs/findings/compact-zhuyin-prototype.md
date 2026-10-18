# Compact Zhuyin prototype (acceptance pending)

Audit timestamp: 2026-10-18T17:43:54Z. Base: `dd55aeb5` (origin/main fetched on this host).
The base contains `e6b3f368`, PR #881's packaging `--gc-sections` change.
Only ZHUYIN_PINYIN_MAP is prototyped. No expansion is authorized by results yet.
No final timing or performance comparison has run: the maintainer has reserved
this host for two concurrent correctness/build tasks. Await an explicit slot.

## Source and compatibility

Read upstream with `git -C /home/sheng/repos/libpinyin show
074a2219c90feaf962d0d24f034514033ece5f99:<path>`; the checkout itself has a
different HEAD and was not modified. The table blob at the pin is
`b1a5f9550e8f7376104c4782a979f42a9467fd2a`.
`src/storage/pinyin_parser_table.h:1492-2985` holds the global index;
`:3976-4418` holds canonical content. `src/storage/zhuyin_parser2.cpp:44-101`
gates incomplete/correction options, uses strcmp/equal_range, and resolves
content by index. `src/storage/chewing_key.cpp:47-89` renders pinyin and
Zhuyin, omitting first-tone marks. These files are byte-identical between
the old comment pin `0c5e80e1` and actual pin `074a2219` (git diff).
The frozen bopomofo-spec remains unchanged.

An independent source audit parsed the pinned header, joined its content IDs,
and compared every original Rust tuple: 1493 exact matches, in order;
417 zero flags, 1062 shuffle, 14 incomplete. The SHA-256 of UTF-8 rows joined
as tab-separated fields and newline-separated rows, without final newline:
`8cff639ca29c9dd11061059e3702085a620d904485a8da37ecd2079923139240`.
There are 1924 distinct strings, 13212 unique UTF-8 bytes, 28834 bytes before
string deduplication, max string length 9 bytes, and 431 canonical pairs.
These are logical data figures, not realized ELF savings.

## Designs

A: deduplicated immutable UTF-8 pool, three checked u16/u8 spans and original
u32 flags. Selected for the first experiment: straightforward borrowed strings,
no runtime construction, heap, cache, unsafe, dependency, or platform API.
UTF-8 boundaries and integer conversions are checked by build.rs. Runtime
access uses checked str::get. Binary search still uses partition_point;
only input spans decode during comparison. The returned tuple decodes one
row. Rendering retains first canonical match order, checking equal pooled
spans before decoding pinyin, then returning its canonical span.

B: canonical ID could replace two spans. With 431 canonical pairs it needs
a u16 ID and another mapping; it would add an extra dependent access and
possibly duplicate chewing canonical data. Not implemented or claimed faster.
Evaluate only if the simpler A prototype's measured trade-off warrants it.

C: existing build.rs now extracts the original frozen Rust literals from
zhuyin_rows.rs and deterministically interns strings in first-occurrence order.
The literals are authoritative, compiled directly only in tests; generated
production data lives only in OUT_DIR. This generator does actual pooling and
bounds validation rather than merely relocating literals. No separately
maintained generated source is committed. Equivalence tests compare generated
rows to the original independently compiled tuples, and exhaustively compare
five option words and canonical first-match rendering over all original rows.

## Scope audit

On x86_64, original map tuples have size 56/alignment 8: 83608 bytes,
4479 pointer fields. HSU: 500/28000 bytes/1500 pointers; ETEN26:
482/26992/1446. CONTENT_TABLE: 441 rows of 104 bytes, alignment 8,
45864 bytes/2646 string pointer fields. CHEWING_KEY_TABLE: 1728 i16,
3456 bytes/alignment 2, zero pointers: leave it unchanged.
LUOMA and SECONDARY each have 406 pairs, 32 bytes/alignment 8,
12992 descriptor bytes/812 pointers. Both are public re-exports and remain
unchanged. Pointer counts are not measured relocation counts.

Production map readers: scheme::search_zhuyin_index (direct, Simple and CP26)
and zhuyin_map::zhuyin_for_pinyin (canonical first-match scan). HSU/ETEN26
use the same search's tuple implementation and remain independent tables.
The private zhuyin_map module exposes only its rendering functions through
core; no public signature changes. CONTENT_TABLE consumers are ChewingKey's
content, from_pinyin, and rendering paths. CHEWING_KEY_TABLE maps components
to a content index. Full-pinyin scheme lookup consumes the public pair slices.

## Build provenance and remaining gates

Linux x86_64-unknown-linux-gnu; rustc 1.97.1 (8bab26f4f), LLVM 22.1.6;
cargo-c 0.10.24+cargo-0.98.0; Ubuntu GCC 15.3.0; GNU ld 2.47;
Valgrind 3.27.1. Backend BDB. No model modifications or uploads.
Shipping build, each tree separately, CARGO_BUILD_JOBS=1 and an absolute
CARGO_TARGET_DIR dedicated to that tree:

    tools/packaging/install.sh libpinyin --prefix=<stage> --libdir=<stage>/lib -- --locked --release --jobs 1 --no-default-features --features bdb,shipped
    tools/packaging/install.sh libzhuyin --prefix=<stage> --libdir=<stage>/lib -- --locked --release --jobs 1 --no-default-features --features bdb

libzhuyin has no shipped feature (a rejected build invocation confirmed this);
its packaging recipe therefore uses bdb only. Final strip uses strip
--strip-unneeded on a copy, preserving unstripped symbols for attribution.
Baseline source is a separate detached worktree, not a modified candidate.
An early build begun before editing in the candidate tree is excluded from
all baseline comparisons because source changes overlapped its compilation.

Still pending: clean shipping artifacts/ELF attribution, final verification
summary, debug BDB differentials, controlled base/candidate/pin instruction,
latency, allocations and RSS measurements. No prototype acceptance claim.
No PR, reviewer request, bot invocation, merge or auto-merge is authorized.

## Interim verification and ELF evidence

`CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=<task>/target-test cargo test -p
oxpinyin-core -p oxpinyin-chewing --locked`: 199 passed, zero failed,
zero ignored. Includes exhaustive 1493-row equivalence and five option words,
first-match renderer equivalence, all existing scheme and key tests and
property tests. `cargo fmt --all --check` passes. `cargo clippy -p
oxpinyin-core -p oxpinyin-chewing --all-targets --locked -- -D warnings`
passes after moving the test module below production items (no lint suppression).
The three AGENTS.md backend checks each output nothing: no store paths,
no matching backend feature condition in changed non-Markdown files, no
backend Cargo.toml changes. BDB only is selected mechanically.

The clean unmodified base libpinyin is 2519792 stripped bytes (SHA-256
131f5faf22746a3c0799bed83692083d1a140eac59bf4f0d130d8d5aff0aaa52).
Baseline libzhuyin is 2335472 (af34d4130e82d9995da3ff5fc41cfa15f98c02999794c1d8e196d937a7254f8e);
candidate libzhuyin is 2167536 (38a5a021651b6212f07e75ef41ad56cb3500b104c16a8bf33bfc84d2aea36acb),
a 167936-byte reduction. These artifacts were packaged with the recipe above,
then `strip --strip-unneeded <copy>`, `stat -c %s <copy>` and
`sha256sum <copy>`. Inspect sections with `readelf -WS <copy>` and symbols
with `nm -S -C <unstripped>`; read relocation offsets with `readelf -Wr
<unstripped>` and count offsets in each symbol's half-open address range.

Both base libraries contain all audited tables. The global table is 83608
bytes plus 4479 RELA entries (107496 bytes). HSU: 28000 + 1500 (36000);
ETEN26: 26992 + 1446 (34704). Content: 45864 + 2604 (62496), fewer
relocations than its pointer-field count because empty string descriptors
need no relocated storage. Each public full-pinyin index: 12992 + 812
(19488). CHEWING_KEY_TABLE: 3456 + zero relocations. In the candidate
libzhuyin the 23888-byte ZHUYIN_ROWS symbol has zero relocations. The old
83608-byte table is absent; its old name survives only as a zero-size marker.

Reading all 4479 baseline descriptors by ELF virtual address, grouping their
UTF-8 contents and comparing pointer values confirms exactly 1924 distinct
strings, and exactly one address per equal string. Compiler/linker string
sharing already exists: repeated literal bytes must not be counted as savings.

For libzhuyin: text 1481637 -> 1482597 (+960); rodata 76608 -> 104832
(+28224); data.rel.ro 257944 -> 174336 (-83608); rela.dyn 337920 ->
230424 (-107496); eh_frame 109280 -> 109288 (+8); eh_frame_hdr 12540
unchanged; gcc_except_table 38732 -> 38724 (-8). BSS stays 160 bytes and
is memory-only, not part of the stripped file byte total. Section changes
are reported separately from the actual file saving; padding and other
sections explain their difference. No RSS claim follows from these sizes.

Both libraries' NEEDED list: libglib-2.0.so.0, libdb-5.3.so,
libgcc_s.so.1, libc.so.6, ld-linux-x86-64.so.2 (readelf -d).
Remaining: candidate pinyin final artifact, paired accounting, padding,
BDB debug C differentials, and the held controlled measurement session.

## Paired shipping result and debug differential checkpoint

Candidate pinyin stripped SHA-256:
`3bd4745017544a3c2b234df5db5da30c257187201e9006ab2dfc08d5007a6d0d`.

| File/section (bytes) | pinyin base | pinyin candidate | delta | zhuyin base | zhuyin candidate | delta |
|---|---:|---:|---:|---:|---:|---:|
| Stripped file | 2519792 | 2360048 | -159744 | 2335472 | 2167536 | -167936 |
| .text | 1628853 | 1629541 | +688 | 1481637 | 1482597 | +960 |
| .rodata | 80376 | 108600 | +28224 | 76608 | 104832 | +28224 |
| .data.rel.ro | 266656 | 183048 | -83608 | 257944 | 174336 | -83608 |
| .rela.dyn | 347760 | 240264 | -107496 | 337920 | 230424 | -107496 |
| .eh_frame | 122824 | 122824 | 0 | 109280 | 109288 | +8 |
| .eh_frame_hdr | 13796 | 13796 | 0 | 12540 | 12540 | 0 |
| .gcc_except_table | 43180 | 43172 | -8 | 38732 | 38724 | -8 |
| File padding | 1795 | 4251 | +2456 | 8038 | 2022 | -6016 |
| .bss (memory only) | 160 | 160 | 0 | 160 | 160 | 0 |

Paired stripped file: 4855264 -> 4527584 bytes, -327680 bytes (320 KiB).
All other section sizes unchanged. ELF/program/section headers total 2544
bytes in each stripped file. Padding is file length minus headers minus
all section sizes excluding SHT_NOBITS; it includes alignment gaps, not RSS.
Allocated file-section sums: pinyin 2515095 -> 2352895; zhuyin 2324532 ->
2162612. Global compact rows are 23888 bytes/alignment 4 in both libraries,
with zero dynamic relocations. Pool is 13212 bytes; the extra rodata accounts
for pooling versus existing shared literal storage and other compiler output.
No fat-pointer global map remains. Artifact size is measured without timing;
all runtime performance/memory comparisons remain unrun.

The baseline is detached dd55aeb583abd475a6f9d72a592726a2d32ed1de under
`<task>/baseline`; candidate production representation is checkpoint
9d12cd31767987b2207dfb20dc2c3aabe734ceb9, with the subsequent checkpoint
moving a test module only and adding evidence. Candidate target directory
is `<task>/target-base` (historical name, not a baseline); clean base uses
`<task>/target-clean-base`. Installed trees are stage-candidate and
stage-clean-base. An early stage-base tree is excluded.

Debug artifacts were built with:

    CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=<task>/target-test cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi --no-default-features --features bdb

BDB pin prefix: `/home/sheng/.local/opt/pinyin-oracle/bdb`; manifest identifies
074a2219 + model20 SHA 59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155.
Pin library hashes: pinyin 754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30;
zhuyin cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b.
Task-local system-bdb links the pin's immutable data files and the cached
model20 interpolation2.text. It does not change the external model directory.

`run-scheme-diff.sh bopomofo <k>` wrapper attempts failed before parsing:
its copied temporary data omitted table.conf. No harness/policy change was
made. Using its existing strict-compiled chewing-diff.c directly, each library
with a fresh SCHEME_DIFF_USER_DIR and the complete system-bdb directory,
returned 0 and byte-identical full logs for k=1,2,3,4,5,6,8,9; respectively
1512,412,367,367,367,425,412,1013 lines. Reproduction: compile with
`gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o <driver> tools/bisection/chewing-diff.c -ldl`;
run `SCHEME_DIFF_USER_DIR=<fresh> <driver> <debug-so-or-pin-so> <system-bdb> <k>`
for each side, then cmp stdout. This retains all existing profile probes.
The wrapper failure is a source-identified setup limitation, not a scored
differential result; no baseline wrapper run was substituted with a release
library. The direct driver result is the compatibility evidence.

`PINYIN_ORACLE_DBM=bdb PINYIN_ORACLE_PREFIX=<prefix> OXPINYIN_CAPI_SO=<debug-pinyin>
KEY_SYSTEM=<system-bdb> tools/bisection/run-key-surface-diff.sh`:
IDENTICAL, all 2131 probe lines, including renderers and option profiles.

`PINYIN_ORACLE_DBM=bdb ZHUYIN_ORACLE_PREFIX=<prefix>
ZHUYIN_ORACLE_DATA=<system-bdb> ZHUYIN_RUST_DATA=<system-bdb>
ZHUYIN_RUST_SO=<debug-zhuyin> tools/bisection/run-zhuyin-diff.sh`:
IDENTICAL, 4558 lines. The runner byte-checks the oracle-data overlay against
all pinned source data files before using it.

Ready for controlled measurement slot. Pending action: alternating BDB
baseline/candidate/pin timing, instruction, initialization, allocation and
memory comparisons on this Linux host. Reason: maintainer-held uncontended
slot while #889 correctness batteries run. Status: pending explicit release,
not denied or uncertain. No performance claim, acceptance or Phase B expansion.
Recommended first review boundary is only the global index; reject/revert it
if the controlled trade-off fails. Subsequent HSU/ETEN26 and content work
must remain independent measured boundaries. Public full-pinyin tables stay
follow-up-only. Logs/captures/models were not uploaded or attached.
