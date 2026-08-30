# Backend-selection alignment: libpinyin vs oxpinyin

Date: 2026-08-30 · Status: **audit closed; no shipping changes required
beyond the tests this document exists alongside**.

The goal was architectural alignment of the store-backend selection
model between libpinyin (the reference implementation) and oxpinyin — not
libpinyin database compatibility. This document is the ground for that
claim: eight questions asked of libpinyin's own source, the same eight
answered against oxpinyin, and the resulting comparison.

## Sources

libpinyin: pinned commit
`0c5e80e1200f84fab185d1c5bde458b770a0636c` (tag `2.11.91`;
`docs/findings/oracle-environment.md`).

- `configure.ac`, `src/storage/Makefile.am`,
  `src/pinyin_internal.h`, `src/storage/ngram_bdb.cpp`,
  `src/storage/ngram_kyotodb.cpp`, `src/storage/ngram_tkrzwdb.cpp` —
  the four files whose combination defines the model.

Distro packaging: `debian/rules`, Fedora `libpinyin.spec`, Arch
`PKGBUILD`, nixpkgs `package.nix` — cross-checked against the linked
DBM library on each distro's installed `libpinyin.so.15` by
`tools/tkrzw/libpinyin-backend-probe.sh` (recorded in
`docs/findings/tkrzw-distro-compat.md` §"Does anything actually ship
against this?").

oxpinyin: current tree (branch
`claude/kyoto-cabinet-default-nlgdug`).

## The eight questions

### 1. What backend is selected when no backend option is supplied?

**libpinyin:** `configure.ac` sets `DBM="BerkeleyDB"` as the fallback
when `--with-dbm=` is not passed, and every distro overrides it —
Debian sid/forky pick `--with-dbm=Tkrzw`, everything else pins
`--with-dbm=KyotoCabinet` (see `docs/findings/tkrzw-distro-compat.md`,
§"The full backend matrix"). No distro ships the upstream default; there
is no "libpinyin default backend" in practice — only "the backend the
distro picked at package time".

**oxpinyin:** `Kyoto Cabinet`. The workspace's default feature set
enables `kyotocabinet` in every store-reaching crate, and
`oxpinyin_store::DefaultStore` resolves to `KcStore` on a plain
`cargo build`. Regression-tested by
`crates/oxpinyin-store/src/lib.rs::tests::default_store_is_kc_when_kyotocabinet_is_on`.

**Difference:** oxpinyin ships one authoritative default (KC).
libpinyin's default is a per-distro packaging choice.

**Intentional?** Yes. libpinyin's fallback is essentially unused (every
distro overrides it) and its choice — Berkeley DB — is refused
outright on the platforms oxpinyin targets. Picking one workspace-wide
default matches the DBM the largest set of downstream distributions
selected for libpinyin.

### 2. How is an alternative backend selected?

**libpinyin:** `./configure --with-dbm=<Name>` at build time, where
`<Name>` is one of `BerkeleyDB`, `KyotoCabinet`, `Tkrzw` (2.11.91-only
for tkrzw). One value; the corresponding `.cpp` files are added
to the library by `src/storage/Makefile.am` under an `AM_CONDITIONAL`
switch. There is no way to enable two backends into one build.

**oxpinyin:** `cargo build --no-default-features --features <peer>` at
build time, where `<peer>` is one of `kyotocabinet`, `redb`, `lmdb`,
`tkrzw`. Regression-tested for each of the four in
`crates/oxpinyin-datagen/tests/backend_default.rs::parse_accepts_each_peer_backend_name`
plus the type-identity check for each in
`crates/oxpinyin-store/src/lib.rs::tests::default_store_is_{kc,redb,lmdb,tkrzw}_...`.

**Difference:** oxpinyin's cargo features are additively unified, so
`--features "kyotocabinet lmdb"` is a legal command that requests both.
libpinyin's `AM_CONDITIONAL` selector has no equivalent — one is chosen.

**Intentional?** The additivity is a property of cargo, not of the
architecture. It is handled defensively: `DefaultStore` resolves the
multi-feature case through the fixed chain
`kyotocabinet > tkrzw > lmdb > redb` — deterministic and documented
(see `crates/oxpinyin-store/src/lib.rs` §"The default backend"). It is
not a hierarchy; it is a tie-break for the "user passed several" case,
which the normal user-facing model does not encourage. The store's own
regression tests pin this behavior.

**Required change:** none. A stronger rule ("reject at compile time
when several backend features are enabled") is achievable
(`compile_error!` behind a `#[cfg(all(...))]` guard) but would break
the intentional selector for consumers that opt in — the workspace's
own default feature set uses exactly the additive form to name KC.

### 3. Is backend selection build-time or runtime?

**libpinyin:** build-time. The DBM library is linked into
`libpinyin.so.15` at build time and appears in `DT_NEEDED` — proven
directly with `tools/tkrzw/libpinyin-backend-probe.sh`, which reads
each distro's `libpinyin.so`'s dynamic tags rather than trusting the
build recipe.

**oxpinyin:** build-time. `DefaultStore` is a `pub type` alias
resolved at compile time; there is no runtime dispatch. After the
libpinyin-compat removal (commit `08e64e8`), the runtime does not
inspect a data directory to choose a backend either — it opens the
tables of its own compiled-in peer through
`oxpinyin_data::default_store_file()`.

**Difference:** none.

### 4. How are generated/installed data files tied to that backend?

**libpinyin:** file **names** are compile-time constants that do NOT
vary with the DBM backend (`src/pinyin_internal.h:56-58`:
`SYSTEM_BIGRAM "bigram.db"`, `USER_BIGRAM "user_bigram.db"`,
`DELETED_BIGRAM "deleted_bigram.db"`). `--with-dbm=BerkeleyDB`,
`--with-dbm=KyotoCabinet` and `--with-dbm=Tkrzw` all produce a file
called `bigram.db` in three mutually unreadable formats.

**oxpinyin:** file **extensions** name the peer backend
(`oxpinyin_store::DEFAULT_STORE_EXT` resolves to `kct` / `redb` /
`lmdb` / `tkt`). Every path helper is `default_store_file(stem)`, which
appends that extension. Regression-tested by
`crates/oxpinyin-store/src/lib.rs::tests::default_store_ext_matches_the_compiled_backend`
and `default_store_file_composes_stem_and_extension`, and by
`crates/oxpinyin-datagen/tests/backend_default.rs::peer_backends_report_their_expected_extensions`.

**Difference:** oxpinyin uses backend-specific extensions; libpinyin
uses one shared name across backends.

**Intentional?** Yes. The rationale is on
`docs/findings/upstream-divergences.md` §"native filenames": a directory
that self-describes its backend cannot be misread through the wrong
engine, which was the class of failure libpinyin had to solve with
runtime magic detection.

**Required change:** none. This is a deliberate, tested divergence.

### 5. What happens when a user changes backend?

**libpinyin:** the storage format changes with the DBM, and old data
does not survive the switch. Debian's `debian/NEWS` for the
`2.11.91-1` upload states it explicitly — "after the engine switch
from BerkeleyDB to Tkrzw... all previous user data will be lost after
the upgrade" (`docs/findings/tkrzw-distro-compat.md` §"Does anything
actually ship against this?"). No automatic migration, no fallback,
no dual-open — the user's `user_bigram.db` becomes an unreadable
file, and the new run creates a fresh one.

**oxpinyin:** identical posture. There is no cross-peer conversion, no
automatic migration, no runtime fallback opening. A user who rebuilds
oxpinyin with a different `--features <peer>` gets a fresh set of
peer-specific tables (`.kct` / `.redb` / `.lmdb` / `.tkt`) and the
peer-specific user store; old files from the previous peer are not
opened.

**Difference:** none.

### 6. Does it transparently migrate old user data?

**libpinyin:** no (per §5).

**oxpinyin:** no (per §5).

**Difference:** none.

### 7. Does it probe existing databases to determine their backend?

**libpinyin:** no. The library is linked against exactly one DBM at
build time and calls its C API directly. It could not probe a
foreign-format file even if it wanted to — the reader isn't compiled
in.

**oxpinyin:** no. After the compat removal, there is no
`layout::detect`, no `CompatLayout::detect`, no `dbm_of_header`. The
`Runtime::open` code path opens each table by its `default_store_file`
name and lets the compiled-in `DefaultStore` refuse a file that isn't
its own format.

**Difference:** none.

### 8. How do build/configuration options propagate into consumers?

**libpinyin:** through the shared `libpinyin.so.15` and its
`libpinyin.pc` — a consumer that `pkg-config --libs libpinyin` inherits
the transitive DBM library through `DT_NEEDED`. Consumers do not choose
the backend; they inherit it from whichever build of libpinyin the
system ships.

**oxpinyin:** through cargo's `[features]` forwarding. Every
store-reaching crate — `oxpinyin-data`, `oxpinyin-user`,
`oxpinyin-runtime`, `oxpinyin-engine` transitively, `oxpinyin-capi`,
`oxpinyin-python`, `oxpinyin-datagen`, `oxpinyin-segment`,
`oxpinyin-counter`, `oxpinyin-emitter`, `oxpinyin-lambda`,
`oxpinyin-dictool`, `pinyin-oracle` — forwards its `{kyotocabinet,
redb, lmdb, tkrzw}` features down onto `oxpinyin-store`, so the peer
choice at the top of any build reaches the single `DefaultStore`
resolution. `oxpinyin-capi/build.rs::database_format` mirrors the
resolved peer into `libpinyin.pc` (`KyotoCabinet` / `redb` / `LMDB` /
`Tkrzw`) so a downstream `pkg-config --variable=database_format`
consumer sees the same peer the binary was linked against.

**Difference:** none — the propagation mechanism is different (cargo
features vs. shared library linkage), but the observed contract is the
same: one peer chosen at build time, propagated end-to-end.

## Alignment summary

| Aspect | libpinyin | oxpinyin | Aligned? |
|---|---|---|---|
| Backend selection is build-time | yes | yes | ✓ |
| One peer chosen per build | yes (`--with-dbm=X`) | yes (`--features X`) | ✓ |
| No runtime probing | yes | yes (post-compat-removal) | ✓ |
| No automatic migration | yes | yes | ✓ |
| Backend transition = data loss | yes (explicit) | yes (explicit) | ✓ |
| Peer choice reaches consumers | yes (DT_NEEDED) | yes (feature forwarding + pkg-config) | ✓ |
| Default when nothing specified | `configure.ac` default (BerkeleyDB, overridden by every distro) | `kyotocabinet` (single authoritative) | intentional divergence |
| On-disk filename model | one name across all backends | one extension per peer | intentional divergence |

Two intentional divergences (default choice, filename model),
justified above and covered by tests. Every other axis matches.

## Definition of done

- [x] libpinyin's actual backend-selection behavior has been verified
      from source and build configuration (references above).
- [x] oxpinyin's backend-selection behavior has been compared against
      it, question by question.
- [x] Intentional differences are documented (§1, §4).
- [x] No accidental differences remain to correct — the audit found none
      that survived the recent compat removal and the peer-language
      cleanup.
- [x] KC is the sole default when no backend is specified — proven by
      the type-identity test in `oxpinyin-store` and the
      `Backend::DEFAULT` test in `oxpinyin-datagen`.
- [x] redb, LMDB and Tkrzw remain equal first-class peers — proven by
      the four `default_store_is_<peer>_when_only_<peer>_is_on` tests
      (one per peer) and the four `Backend::<Peer>.extension()`
      checks.
- [x] Backend selection is build-time (no runtime probing) — the
      compat detection code has been removed (commit `08e64e8`).
- [x] No automatic backend migration exists — nothing was added and
      nothing existed to remove.
- [x] No libpinyin DB compatibility layer is introduced — nothing was
      added.
- [x] datagen and runtime agree on the default backend — the
      `Backend::DEFAULT` associated constant is the single source of
      truth, tested against `oxpinyin_store::DEFAULT_STORE_EXT`.
- [x] Backend-specific filenames follow the selected peer via
      `default_store_file(stem)`.
- [x] Tests cover all four peer selections (KC / redb / LMDB / Tkrzw).
- [x] Documentation uses "default", not "canonical/preferred", for KC —
      the "portability fallback" wording was removed in the C1 pass
      that preceded this audit.
