# Debian ships libpinyin on tkrzw; Ubuntu's tkrzw silently corrupts records

Status: **reference** (the mechanism and the per-distro matrix).

tkrzw is oxpinyin's default backend and reference cell. Development builds
and CI install the repository's vcpkg overlay port (tkrzw 1.0.34, `9ee8416`;
the revision is in `vcpkg-overlays/tkrzw/pin.json`): Linux jobs through
`.github/actions/tkrzw`, Windows through `tools.dev.vcpkg`. Debian release
packaging (`release-packages.yml` installs `libtkrzw-dev`), shipping-size
measurements and oracle/reference images use Debian's library. The
[verification runbook §9](../runbooks/verification.md#9-sound-tkrzw-library)
states how to run against a sound library and how to triage a failing save.

Ubuntu's packaged libtkrzw breaks tkrzw's pointer-identity protocol.
`./configure && make` on the same sources is correct. tkrzw 1.0.34 fixes
defect 1 upstream and leaves defect 2; everything below about defect 1
describes 1.0.33 and earlier.

**There are two independent defects, with two different causes.** Both
come from build flags that Ubuntu applies to every package and Debian
applies to none. Both are silent, and neither fix repairs the other:

| | Cause | Breaks | Symptom |
| --- | --- | --- | --- |
| **1** | `-flto` | `RecordProcessor::NOOP` / `REMOVE` — *data* | `Remove()` stores a tombstone instead of deleting; a NOOP processor overwrites the record; `Rebuild` mis-counts |
| **2** | `-Wl,-Bsymbolic-functions` | the key comparators — *functions* | TreeDBM records comparator type 255 and can never reopen the file |

A fix that only disables LTO resolves defect 1 and leaves defect 2.

## Affected builds

Every Ubuntu release that carries tkrzw has both defects:

| Ubuntu release | tkrzw package | `remove` | `.tkt` round-trip |
| --- | --- | --- | --- |
| 24.04 LTS (noble) | `1.0.27-1.1build1` | **tombstone** | **unreadable** |
| 25.10 (questing) | `1.0.32-1` | **tombstone** | **unreadable** |
| 26.04 LTS (resolute) | `1.0.32-1build1` | **tombstone** | **unreadable** |

Questing's version string `1.0.32-1` is identical to Debian's: it is a
direct source sync, but Ubuntu built the binary. The `buildN` suffix does
not mark an affected package; a build by Ubuntu does.

Debian's source package (1.0.27 and 1.0.32) built with Debian's flags has
neither defect, and so does upstream `./configure && make`; built with
Ubuntu's flags it has both. LTO alone causes defect 1, and
`-Wl,-Bsymbolic-functions` alone causes defect 2. Nothing else in Ubuntu's
flag set is involved.

"unreadable" is literal. `tkrzw_dbm_util create x.tkt` succeeds, and
every later open of that file by the same binary fails with
`BROKEN_DATA_ERROR: invalid_key_comparator`. Defect 2 does not affect
SkipDBM (`.tks`), which stores no comparator. Every DBM type is exposed to
defect 1.

## Defect 2 — the comparators

tkrzw identifies some values by the *address* of a symbol, not by its
contents, and documents this. `tkrzw_dbm.h` states that the
`RecordProcessor::NOOP` sentinel must be checked with
`your_value.data() == NOOP.data()`. `TreeDBM` does the same for
comparators. `tkrzw_dbm_tree.cc:1150` turns the caller's function pointer
into the on-disk type byte by pointer equality only:

```cpp
if (key_comparator_ == nullptr || key_comparator_ == LexicalKeyComparator) {
  key_comp_type = 1;
} else if (key_comparator_ == LexicalCaseKeyComparator) {
  ...
} else {
  key_comp_type = 255;          // "a comparator I cannot name"
}
```

On reopen, `LoadMetadata` maps 1..5 and 101..105 back to a built-in.
Byte 255 means "the caller must supply the same custom comparator again".
Nothing in tkrzw can do that alone, so the file is unreadable
(`tkrzw_dbm_tree.cc:1237`).

The five comparators are `inline` functions in a header
(`tkrzw_key_comparators.h:43`). Any translation unit that takes their
address emits its own COMDAT copy. So `tkrzw_dbm_util` has one copy and
`libtkrzw.so` has another. C++ requires the two to compare equal. ELF
does this by routing the library's address-taking through the GOT: a
`R_X86_64_GLOB_DAT` relocation that the dynamic linker resolves to the one
canonical definition. A correct build of 1.0.32 keeps 18 such
relocations. A broken build keeps none.

`-Wl,-Bsymbolic-functions` is the flag that removes them. It binds a
shared library's function references to that library's own definitions at
link time. `libtkrzw.so` then compares against its own copy, and
`tkrzw_dbm_util` passes its own copy. So tkrzw never recognises a built-in
comparator. `--comparator` defaults to `lex` (`tkrzw_dbm_util.cc:630`), so
*every* `.tkt` that the CLI creates records 255.

## Defect 1 — the sentinels

`NOOP` and `REMOVE` are not functions but static data, each a
`string_view` over a five-byte literal (`tkrzw_dbm.cc:25`):

```cpp
const std::string_view DBM::RecordProcessor::NOOP("\x00\xBE\xEF\x02\x11", 5);
const std::string_view DBM::RecordProcessor::REMOVE("\x00\xDE\xAD\x02\x11", 5);
```

`-Bsymbolic-functions` cannot affect these, because it binds functions
only. LTO breaks them. Without LTO, `libtkrzw.so` holds exactly one copy
of each backing literal, so the exported `NOOP` object and every
comparison site agree. With LTO, GCC's partitioning gives most partitions
their own copy (ten of `NOOP` and eleven of `REMOVE` in Ubuntu's build).
The address that a comparison site was compiled against is then not the
address that the exported object carries, and
`value.data() == NOOP.data()` fails. `Remove` then writes the REMOVE
sentinel as the record's value and reports success.

## It is not only the CLI

Both defects reach any *client* of the library. Against a library built
with Ubuntu's flags:

- A `key_comparator` left at its `nullptr` default records type 1 and
  reopens, because the decision stays inside the library, where both sides
  of the comparison are the same copy.
- A client that passes `LexicalKeyComparator` itself records type 255, and
  the reopen fails with `BROKEN_DATA_ERROR: invalid_key_comparator`.
- A NOOP-returning processor overwrites the record with `00 be ef 02 11`.
- `Remove()` leaves the record in place with value `00 de ad 02 11`.
- `Rebuild()` answers `CANCELED_ERROR`.

Against Debian's flags every one of these behaves as documented.
`oxpinyin-store` installs no comparator
([store key-ordering contract](store-key-ordering.md)), so defect 2 does
not reach its files. `oxpinyin-store` *is* exposed to the processor,
`Remove()` and `Rebuild()` faults, and they are silent: a `Remove` that
stores the sentinel as the value looks like a successful write.

The sentinel fault is not a client/library boundary problem.
`dbm.Remove(key)` passes no sentinel across any boundary and still
corrupts, because LTO duplicated the literal *inside* libtkrzw. Any
consumer is exposed, in any language binding. `DBM::Remove` is itself a
record processor: `RecordProcessorRemove` answers `REMOVE` for a stored
record and `NOOP` for an absent one (`tkrzw_dbm.h:217-242`) through the
same `Process`. On an LTO 1.0.32, on `TreeDBM`, `HashDBM`, `BabyDBM` and
`TinyDBM` alike, removing a stored key returns `true` and leaves the record
with value `00 de ad 02 11`, and removing an absent key returns `false`
and creates a record with value `00 be ef 02 11`. The comparator fault is
narrower: it needs a caller that names a built-in comparator itself, which
is why `tkrzw_dbm_util` triggers it and libpinyin does not.

## What decides it, per distro

Each defect depends on one bit of the distro's defaults:

| Distro | Source | `-Bsymbolic-functions` | LTO by default |
| --- | --- | --- | --- |
| Ubuntu | `Dpkg::Vendor::Ubuntu:156` | **yes** | **yes** |
| Debian | `Dpkg::Vendor::Debian` | no | no (opt-in per package) |
| Devuan, PureOS | their `Dpkg::Vendor::*` | no | no |
| Arch | `/etc/makepkg.conf` (`pacman 7.1.0.r9`) | no | **yes** (`OPTIONS=(... lto)`) |
| Fedora | `redhat-rpm-config` | no | **yes** globally, but `tkrzw.spec` sets `%_lto_cflags %{nil}` |
| RHEL 10 (via EPEL) | EPEL, Fedora spec lineage | no | no (`tkrzw.spec` opts out); not in RHEL's own repos |

The dpkg figures are from dpkg's own git at tag 1.23.7. Arch's `-fno-plt`
is not a hazard: it changes how calls are routed, not how addresses are
taken. A package can still add either flag itself; tkrzw's `debian/rules`
does not (it never sets `LDFLAGS`).

Per distro (Arch: a source build under its stock `makepkg.conf`, because
Arch packages no tkrzw):

| Family | Distro | tkrzw package | defect 1 (`remove`) | defect 2 (comparator) | Result |
| --- | --- | --- | --- | --- | --- |
| `.deb` | Ubuntu 26.04 LTS | `1.0.32-1build1` | **broken** | **broken** (type 255, 0 relocs) | **broken** |
| `.deb` | Debian testing (forky) | `1.0.32-1+b2` | ok | ok (type 1, 18 relocs) | healthy |
| `.rpm` | Fedora Rawhide | `1.0.32-5.fc45` | ok | ok (type 1, 18 relocs) | healthy |
| `.rpm` | RHEL 10.2 + EPEL | `1.0.32-2.el10_1` | ok | ok (type 1, 18 relocs) | healthy |
| Arch | Arch, from source | none packaged | **broken** | ok (type 1, 18 relocs) | **broken (defect 1)** |

Fedora enables LTO by default, but `tkrzw.spec` sets
`%global _lto_cflags %{nil}` ("Disabled LTO, since it causes test failures
on all file-based database tests"). EPEL 10 follows the Fedora spec
lineage; RHEL's own repositories carry no tkrzw. Arch has no distro-wide
`_lto_cflags` opt-out, so a tkrzw built there is exposed to defect 1
unless its own PKGBUILD disables LTO.

## Which libpinyin builds use tkrzw

| Distro | libpinyin | DBM backend |
| --- | --- | --- |
| Debian sid / forky | `2.11.91-1` | **Tkrzw** |
| Debian trixie / bookworm | `2.8.1-1` | Berkeley DB |
| Ubuntu 26.04 LTS | `2.10.3-1` | Berkeley DB |
| Fedora Rawhide | `2.11.91` | Kyoto Cabinet |
| RHEL 10 (EPEL) | `2.8.1-9.el10` | Kyoto Cabinet |
| Arch | `2.10.3` | Kyoto Cabinet |
| openSUSE Tumbleweed | `2.10.3` | Kyoto Cabinet |
| NixOS (nixpkgs) | `2.11.91` | Kyoto Cabinet |

Debian `libpinyin 2.11.91-1` (unstable and testing) sets `--with-dbm=Tkrzw`,
links `libtkrzw.so.1` and depends on `libtkrzw1t64 (>= 1.0.32)`; its
`debian/NEWS` warns that previous user data is lost after the switch.
Upstream's `configure.ac` still defaults to Berkeley DB. Debian's own tkrzw
is healthy, so no shipped cell is on tkrzw **and** broken. The exposure is
Ubuntu: its tkrzw is broken on both defects, and it syncs `libpinyin` from
Debian. An Ubuntu release that syncs `--with-dbm=Tkrzw` onto Ubuntu's
libtkrzw puts defect 1 on every user dictionary.

The exposure of libpinyin itself:

- **The comparator fault does not reach libpinyin**, although two of its
  five tkrzw backends use TreeDBM. Its `src/` has no `OpenAdvanced`, no
  `TuningParameters` and no `key_comparator`. So the comparator stays
  `nullptr` and is resolved inside libtkrzw, where both sides of the
  comparison are the same copy.
- **The sentinel fault does reach libpinyin.** Three `Remove()` call sites
  would leave the record in place, with the five-byte `REMOVE` sentinel
  as its value: `ngram_tkrzwdb.cpp:140` (`Bigram::remove`),
  `punct_table_tkrzwdb.cpp:157` and `flexible_ngram_tkrzwdb.h:263` (the
  user n-gram). That is user-dictionary data.
- The NOOP-returning processors are safe as written.
  `KeyCollectProcessor` and `FlexibleKeyCollectProcessor` both run under
  `ProcessEach(&processor, false)`, and the read-only traversal never
  writes a return value back.

## What this means for oxpinyin

`build.rs` requires `pkg-config` to find tkrzw. It does **not** verify the
origin or health of the tkrzw that `pkg-config` finds, so a broken distro
`libtkrzw-dev` on `PKG_CONFIG_PATH` would link. A result against a
defective libtkrzw is invalid
([verification runbook §9](../runbooks/verification.md#9-sound-tkrzw-library)).

The store suite's `tkrzw_library_honours_its_sentinel_protocol` checks
defect 1 on each DBM class the backend opens: the two file classes, the
libpinyin session's in-memory `BabyDBM` and the user bigram's `TinyDBM`.

On a defect-1 library, an import or a train saves and matches the pin. A
removal does not: `pinyin_mask_out`, `pinyin_remove_user_candidate` and a
train that empties a predecessor's row issue removals that leave the rows
in place with `REMOVE`'s five bytes, and the call still answers `true`. The
next save that has something to write reads the surviving row as a corrupt
value and answers `false`. The pin, on the same library, saves `true`: its
phrase items are not tkrzw records, and it masks its index tables by
rewriting records with a writable `ProcessEach`, not by removing them
(`src/pinyin.cpp:1224-1296`; `chewing_large_table2_tkrzwdb.cpp:419-491`;
`phrase_large_table3_tkrzwdb.cpp:277-315`). Its one tkrzw removal is the
user bigram's `Bigram::remove` (`ngram_tkrzwdb.cpp:140`), `DBM::Remove` on
a `TinyDBM`, which goes through the same sentinel.

## tkrzw 1.0.34

Upstream commit `9ee8416` (release 1.0.34) changes how the sentinels are
stored:

```cpp
// tkrzw_dbm.cc at 9ee8416 (1.0.34)
namespace {
const char DBM_RECORD_PROCESSOR_NOOP[] = "\x00\xBE\xEF\x02\x11";
const char DBM_RECORD_PROCESSOR_REMOVE[] = "\x00\xDE\xAD\x02\x11";
}  // namespace
const std::string_view DBM::RecordProcessor::NOOP(
    DBM_RECORD_PROCESSOR_NOOP, sizeof(DBM_RECORD_PROCESSOR_NOOP) - 1);
const std::string_view DBM::RecordProcessor::REMOVE(
    DBM_RECORD_PROCESSOR_REMOVE, sizeof(DBM_RECORD_PROCESSOR_REMOVE) - 1);
```

The check is still `value.data() == NOOP.data()`, an address identity, but
a named array, not a string literal, backs each sentinel, so there is one
address to compare against. `DBM::ANY_DATA` gets the same change. The type,
the five bytes and the size are the same, and the installed headers are
byte-identical, so the backend needs no change for either version. The
other commits in `bcaa0fb..9ee8416` are not involved: `9db46db` frees a
leaked iterator key buffer (keys longer than 128 bytes), and `17926f7` is
the version number and a regenerated `configure`.

- **Defect 1 is fixed at the mechanism.** An LTO build of 1.0.34 carries
  one copy of each literal in `.rodata`, on arm64 and amd64, where an LTO
  build of 1.0.32 carries nine or more.
- **Defect 2 is unchanged.** With Ubuntu's two flags, 1.0.34 still keeps no
  comparator GOT relocation, and a client that names a built-in comparator
  still records type 255. That does not reach this backend or libpinyin,
  which install no comparator. It is still unsound for `tkrzw_dbm_util`
  and any client that names a comparator. Ubuntu's own 1.0.34 package, when
  it has one, is not assumed to be sound.

A version gate that refused 1.0.34 would refuse the one release that is
immune to defect 1. Below 1.0.34 the version tells nothing: 1.0.33 and
earlier are healthy from `./configure && make` and from Debian, Fedora and
EPEL, and broken from Ubuntu and under Arch's default flags.
