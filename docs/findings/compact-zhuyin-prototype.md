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
