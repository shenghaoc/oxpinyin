# BerkeleyDB compat — Phase 2

Date: 2026-08-28 · Status: **backend implemented and measured; three of
the brief's eight items are blocked and named below** · Branch:
`feat/bdb-backend`.

Phase 1's survey is `berkeleydb-compat-phase1.md`; its checklist is
`berkeleydb-compat-open-items.md`. Both are inherited unchanged except
where measurement below sharpened them.

## §5 — the #180 decision, and why it goes the recommended way

The invariant:

> The canonical linguistic source is the source of truth. No oxpinyin
> backend may require libpinyin-generated runtime data as its input.

**Resolved as "require" = *cannot function without*.** The system-data
half of Phase 2 proceeds. This is not a coin-flip between two readings;
`datagen-model20.md` states its own subject in four places, and every one
is about production:

- the title is "Native model20 data production — the canonical-source
  invariant";
- "Every runtime-data **producer** consumes that archive directly",
  above a diagram whose every node is a producer;
- it "replaces the retired `oxpinyin-migrate` route, which was the
  opposite architecture" — and `oxpinyin-migrate` was a *producer* that
  consumed an oracle export;
- "CI stays free of model data entirely", i.e. nothing in the build or
  test pipeline depends on any libpinyin output.

Nothing in it addresses runtime consumption. The stricter reading would
also forbid the drop-in program outright — a drop-in that may not open
the files the libpinyin package installed is not a drop-in — so it
cannot be what the invariant meant.

No counter-evidence was found: `#180` appears in no other in-tree
document, and `datagen-model20.md` uses "consume" only of producers.

The clarification Phase 1 proposed is added to `datagen-model20.md` in
this change, verbatim, so the reading is written down rather than
implied.

## What was measured, not assumed

Everything in this section was run against the **real installed
libpinyin 2.8.1 package** on this machine
(`/usr/lib/x86_64-linux-gnu/libpinyin/data`), not against a fixture.

### The `SingleGram` layout, over 56,359 real records

A C probe driving libdb directly — no Rust — walked the whole 25.9 MB
system `bigram.db`:

| Check | Result |
|---|---|
| records | 56,359 |
| successor items | 1,849,609 |
| keys not 4 bytes | 0 |
| values failing `4 + 8n` | 0 |
| item arrays not ascending | 0 |
| `total_freq` ≠ Σ item freq | 0 |
| zero items but non-zero total | 0 |

Phase 1's layout is confirmed exactly. The two counts are also what
`oxpinyin-datagen` derives independently from `model20`
(`datagen-model20.md`'s equivalence proof: 56,359 entries / 1,849,609
successor records) — two unrelated routes to the same pair of numbers,
which is what makes this a format check and not a self-consistency one.

### The B-tree order, confirmed experimentally

Phase 1 said the `DB_BTREE` order "must be confirmed experimentally
against a real file before anything depends on it". It now has been. A
`DB_BTREE` opened exactly as libpinyin opens one — `NULL` environment,
`NULL` transaction, **no** `set_bt_compare` — loaded with LE `u32` array
keys crossing 256 in the first and in a later element walks:

```
00000001            0x01000000
0000000102010000    0x01000000 0x00000102
00000100            0x00010000
0000010002010000    0x00010000 0x00000102
0000ff00            0x00ff0000
...
ffff0000            0x0000ffff
```

Two things this settles:

1. **Raw-byte order, not integer order.** The decoded first elements run
   `0x01000000, 0x00010000, 0x00ff0000, 0x00000100, 0x07000100,
   0x00000200, 0x00000001, …` — not ascending, and not big-endian order
   either.
2. **Then length.** The 1-element key `00000001` immediately precedes the
   2-element key `0000000102010000` that extends it: `memcmp` over the
   shared prefix, shorter first.

That is the store's existing one rule exactly (`store-key-ordering.md`),
so the backend satisfies it without configuration — and setting a
comparator would silently reorder files libpinyin wrote. The harness is
`tools/bdb/btree-order.c`.

### Byte compatibility, in both directions

The gate for a write path is not "does it round-trip through us" but
"does it produce the bytes libpinyin would have produced", because a
mismatch corrupts a user's profile with no error anywhere.

- **Read → re-encode.** For every one of the 56,359 records in the real
  file, `SingleGram::encode(decode(x))` equals the stored bytes. Test:
  `every_record_of_the_real_system_bigram_round_trips_byte_for_byte`.
- **Write → read with no Rust.** A `bigram.db` this backend creates,
  handed to the C harness driving libdb directly, passes every invariant
  in the table above. Example: `bdb_write_profile`.

Both checks were confirmed non-vacuous by reverting the implementation:
encoding big-endian instead of native fails the round-trip on the first
record, and dropping `insert_freq`'s `lower_bound` placement for a
`push` fails the ordering test.

### Sanitizers

`tools/bdb/run-sanitizers.sh`, clean. Two halves, because they cover
different code, and what they do **not** cover matters as much:

- **Rust under AddressSanitizer** (+ LeakSanitizer, on by default on
  Linux): 51 tests including the full 25.9 MB walk. Covers this
  backend's FFI and, through ASan's malloc interposition, heap misuse of
  the memory libdb hands back.
- **A C harness under `-fsanitize=address,undefined`**: the same libdb
  call sequence and the same chunk arithmetic. This is where UBSan has
  something to instrument — misaligned `DBT` loads, out-of-bounds chunk
  indexing, overflow in `(size - 4) / 8`.

**rustc has no `undefined` sanitizer.** `-Zsanitizer=` accepts address,
cfi, dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress, leak,
memory, memtag, safestack, shadow-call-stack, thread and realtime, and
nothing else; UBSan is a C/C++ instrumentation and cannot be applied to
Rust. **Neither half instruments libdb itself** — the system library is
not rebuilt, so UB inside Berkeley DB is invisible to both. Covering
that would mean building libdb 5.3.28 with the sanitizers, which is a
separate job.

## The four hazards

| Hazard | Answer |
|---|---|
| (a) cross-language unwind | Not reachable: libdb is C and Rust calls into it, never the reverse. `Db` and `Cursor` `Drop` impls close on unwind. |
| (b) `Send`/`Sync` | Neither is implemented, and a raw pointer field means neither is derived. Handles are opened without `DB_THREAD`, as libpinyin opens them. See the blocker on item 7. |
| (c) cursor lifetime | `Cursor::get` takes `&mut self` and returns `Record<'_>` borrowing `&self`, so a second `get` while a record is held is a **compile error**. libdb's prose rule is enforced by the borrow checker. |
| (d) null returns | `db_create` and `DB->cursor` are checked for a null out-parameter on success. Every `DB`/`DBC` member is an `Option<unsafe extern "C" fn>`; the `method!` macro turns a null member into an error rather than a call through null. |

## Bindings: generated fresh, not checked in

`DB`, `DBT` and `DBC` are ABI structs with version-specific layouts. A
checked-in `bindings.rs` would freeze 5.3.28's layout and silently
misread any other libdb — writing through a struct whose fields have
moved is the silent-corruption failure this backend must not have.
Generating from the installed header makes the layout right by
construction. A runtime `db_version` check refuses a major.minor this
survey did not cover, rather than guessing.

The cost is a build-time libclang, which is acceptable **because the
feature is opt-in**; it would not be if BDB were the default (see
below). `libdb` / `libdb-sys` were rejected in Phase 1 for statically
linking a vendored Berkeley DB, which is the wrong shape when the point
is to read files the *system* libdb wrote.

## Blocked, with reasons

Five of the brief's eight items landed (1, 2, 4, 5, and the compat
bigram read/write of 6 and 8). Three did not, and none of them is a
matter of effort.

### Item 7 — BDB as the default backend: two independent blockers

1. **`DefaultStore` must be `Send`, and this backend is not.**
   `registry.rs` holds `static OPEN_STORES: OnceLock<Mutex<HashMap<PathBuf,
   Weak<StoreInner<DefaultStore>>>>>`. A `static` requires `Sync`;
   `Mutex<T>: Sync` requires `T: Send`; `StoreInner<S>` holds
   `Mutex<S>`. So `DefaultStore: Send` is a compile-level requirement.
   Making the handle `Send` means opening with `DB_THREAD`, which
   requires `DB_DBT_MALLOC`, `DB_DBT_REALLOC` or `DB_DBT_USERMEM` on
   every `DBT` — a copy of every record on every read, including the
   full-file walks, and the end of the zero-copy cursor path that
   hazard (c) exists to make safe.
2. **It breaks the portable CI job and contradicts the steering map.**
   `.kiro/steering/structure.md` marks oxpinyin-store, -data and -user
   "Portable: yes", and `ci.yml`'s `test-portable` runs `-p
   oxpinyin-data -p oxpinyin-user` on macOS and Windows. Neither has
   libdb. Flipping the default therefore needs a CI matrix change and a
   steering-map change — both "edit CI policy without ask" under
   AGENTS.md's hard forbids.

The backend is fully wired as `--features bdb` and runs the same shared
read/write suites and cross-backend conformance tests as every other
backend, so flipping the default is a one-line change once those two
decisions are made.

### Item 3 — the bigram codec simplification

Correct as analysis: the blob-per-prev model does remove
`bigram_successors`'s range scan, which is the bigram key's only reason
for integer-ordered bytes. But **the user store has no format-version
field** — `structure.md` says "format-version from day one" and there is
none in `crates/oxpinyin-user/`. Changing the native bigram key encoding
would make every existing native profile misread with no error, which is
the same silent-corruption class the brief STOPs on for writes. Doing it
safely means adding the missing version guard first, which is its own
change with its own ask.

### Items 6 and 8 — the halves that are not Berkeley DB

The bigram half of both is done: `BigramDb` reads and writes libpinyin's
`bigram.db` byte-compatibly, and the same type serves the system file and
the user's (only the open mode differs).

The rest of item 8 is **not BDB work at all**. `phrase_index.bin` (7.8 M)
and `pinyin_index.bin` (10.6 M) are libpinyin `MemoryChunk` images — a
different format, with the phrase-item record layout, per-library
sub-chunks and the `table.conf` manifest — and **there is no MemoryChunk
reader in this tree**. `oxpinyin-data` reads oxpinyin's own
store-backed tables (`fixtures/w3/`), not libpinyin's `.bin` files.
Writing that reader is comparable in size to this whole change and
belongs in its own PR. The detector in `oxpinyin-data::layout` already
classifies a real installed directory correctly, so it is the seam that
reader plugs into.

### A frozen SPEC this reverses

`user-store.md` §10 non-goal 1, called there "the headline decision of
this finding":

> **Not** reproducing libpinyin's binary user-data format — `user_bigram.db`
> (a DBM/BerkeleyDB store) … redb is the store; only the **values and
> semantics** are the target.

Brief item 6 ("training must write back into libpinyin's files
byte-compatibly") reverses exactly this. The reversal is recorded in
that SPEC rather than left to contradict the code; the drop-in program
is the reason, and it postdates the W6 decision.

## Not verified

- **The drop-in test did not run.** ibus-libpinyin is not installed here
  (`libpinyin15`, `libpinyin-data` and `libpinyin-utils` are; the IBus
  engine is not), and the installed library is **2.8.1** while the pinned
  oracle is **2.11.91**, so even with the frontend present the comparison
  would be against a different release than every other pin in this tree.
  The gate stands unmet: nothing here proves end-to-end drop-in.
- **The frozen candidate and sentence pins were not re-measured.** They
  are measured against the pin-built oracle, which cannot be provisioned
  in this environment — `tools/oracle/build-oracle.sh` fetches
  SHA-pinned tarballs from `codeload.github.com`, answered 403. This
  change adds a backend behind an off-by-default feature and touches no
  decode path, so it cannot move them; that is an argument, not a
  measurement.
- **`libpinyin-utils` offers no bigram accessor**, and 2.8.1's public
  `pinyin.h` exposes none either, so a cross-check of `bigram.db`
  contents *through libpinyin's own API* was not available. The C-libdb
  cross-check above is the closest substitute: it shares no code with the
  Rust reader.
